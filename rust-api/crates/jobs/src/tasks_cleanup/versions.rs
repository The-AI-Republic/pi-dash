//! D-09 version-task handlers (stage 5, PIDASHCONV-188).
//!
//! Worker-plane port of `apps/api/pi_dash/bgtasks/issue_version_sync.py`,
//! `issue_description_version_sync.py`, `issue_description_version_task.py`
//! and `page_version_task.py`. Pure payload parsing, Celery-name constants
//! and `.delay()`/`apply_async(countdown=…)` message constructors live here
//! next to the async flows so the contract suite's wire/options/ETA vectors
//! replay without a database; SQL text lives in
//! [`pidash_db::tasks_cleanup::version_queries`], decisions in
//! [`pidash_services::tasks_cleanup::versions`].
//!
//! # Ported semantics (translate, don't redesign)
//!
//! * The seven `@shared_task`s keep their exact Celery names ([`ALL_VERSION_TASKS`]).
//! * Every task body is wrapped the way Python's broad `except` wraps it:
//!   task-domain outcomes (missing row → `DoesNotExist`, bad payload JSON,
//!   unknown diff key ≈ `AttributeError`, unparsable ids ≈ `ValidationError`
//!   / `DataError`) log and **ack** — never requeue. Only transport-level
//!   failures (envelope shape, `sqlx` errors) return `Err`, which the worker
//!   requeues against the Celery retry budget (`max_retries = 3`,
//!   `default_retry_delay = 180s`).
//! * `sync_*` runs inside one transaction (`transaction.atomic()`); the
//!   chained batch is enqueued in that same transaction, so a failed batch
//!   never emits a phantom continuation.
//! * `schedule_*` is `.delay(batch_size=int(batch_size), countdown=…)`:
//!   an immediately-visible job for the sync task at offset 0.
//! * The chain re-fires with `countdown`: `visible_at = now + countdown`
//!   ([`NewJob::delayed`]), the queue-side half of
//!   `apply_async(…, countdown=countdown)`.
//! * Writes take an explicit [`RequestContext`][pidash_db::context::RequestContext]
//!   per the Porting guide. The Python task plane has no tenant gate —
//!   batches routinely span workspaces inside one `atomic()` block — so the
//!   context is threaded (one per affected row) but asserts nothing new;
//!   adding a check Python lacks would diverge, not translate.
//!
//! # Ported bugs and quirks (translate, don't redesign)
//!
//! * The `issue_task` coalesce compares the rendered owner object string
//!   (see the services docs); the update branch is implemented but
//!   effectively dead — ported as-is.
//! * The `issue_task` diff compares native row values against the
//!   deserialized payload with Python `!=` rules ([`py_eq`]): UUID/date
//!   columns never equal their string payload form, so those keys always
//!   read as changed — exactly as `getattr(issue, key) != value` behaves.
//! * Unknown payload keys ack the task (≈ `AttributeError` into the broad
//!   `except`); on the dead coalesce-update path a changed key outside
//!   the version columns acks too (≈ `FieldError` from
//!   `save(update_fields=[…])`).
//! * `issue_task`'s `else` branch NEVER writes: `log_issue_version`
//!   (`issue.py:863`) always raises `FieldError` (`Module` has no `issue`
//!   relation, so `Module.objects.filter(issue=…)` in `:896` fails) and
//!   returns `False`. The branch is ported as no-write + ack.
//! * The 20-cap prune is a soft-delete (`UPDATE deleted_at`): `PageVersion`
//!   rides `SoftDeleteModel`, so `.first().delete()` stamps the marker.
//!
//! Wiring note: the crate root declares `pub mod tasks_cleanup;`
//! (foundation change, per the merged layer-PR precedent); these files are
//! new-files-only for this issue.

use std::collections::HashSet;
use std::sync::Arc;

use chrono::{DateTime, NaiveDate, Utc};
use serde_json::{Map, Value};
use sqlx::postgres::PgRow;
use sqlx::Row;
use uuid::Uuid;

use pidash_db::context::RequestContext;
use pidash_db::tasks_cleanup::version_queries as q;
use pidash_services::tasks_cleanup::versions as v;
use pidash_types::ids::{UserId, WorkspaceId};

use crate::queue::{enqueue, enqueue_exec, JobRow, NewJob};
use crate::worker::{Handler, HandlerError, Registry, Verdict};

// ---------------------------------------------------------------------------
// Celery wire names
// ---------------------------------------------------------------------------

/// `issue_version_sync.py:34`.
pub const TASK_ISSUE_TASK: &str = "pi_dash.bgtasks.issue_version_sync.issue_task";
/// `issue_version_sync.py:181`.
pub const TASK_SYNC_ISSUE_VERSION: &str = "pi_dash.bgtasks.issue_version_sync.sync_issue_version";
/// `issue_version_sync.py:235`.
pub const TASK_SCHEDULE_ISSUE_VERSION: &str =
    "pi_dash.bgtasks.issue_version_sync.schedule_issue_version";
/// `issue_description_version_sync.py:40`.
pub const TASK_SYNC_DESCRIPTION_VERSION: &str =
    "pi_dash.bgtasks.issue_description_version_sync.sync_issue_description_version";
/// `issue_description_version_sync.py:124`.
pub const TASK_SCHEDULE_DESCRIPTION_VERSION: &str =
    "pi_dash.bgtasks.issue_description_version_sync.schedule_issue_description_version";
/// `issue_description_version_task.py:44`.
pub const TASK_DESCRIPTION_TASK: &str =
    "pi_dash.bgtasks.issue_description_version_task.issue_description_version_task";
/// `page_version_task.py:22`.
pub const TASK_TRACK_PAGE_VERSION: &str = "pi_dash.bgtasks.page_version_task.track_page_version";

/// Every task name this module owns (registration + contract-suite parity).
pub const ALL_VERSION_TASKS: [&str; 7] = [
    TASK_ISSUE_TASK,
    TASK_SYNC_ISSUE_VERSION,
    TASK_SCHEDULE_ISSUE_VERSION,
    TASK_SYNC_DESCRIPTION_VERSION,
    TASK_SCHEDULE_DESCRIPTION_VERSION,
    TASK_DESCRIPTION_TASK,
    TASK_TRACK_PAGE_VERSION,
];

// ---------------------------------------------------------------------------
// Payload parsing (pure: Celery v2 positional args + kwargs object)
// ---------------------------------------------------------------------------

/// Envelope failure (missing/shape-wrong args → worker requeues) versus
/// task-level abort (`None` → log + ack, mirroring the broad `except`).
type Parsed<T> = Result<Option<T>, String>;

fn ensure_args_array(args: &Value) -> Result<&Vec<Value>, String> {
    args.as_array()
        .ok_or_else(|| "version task: args is not an array".to_string())
}

/// One Celery string argument. A non-string value aborts the task
/// (`Ok(None)`, ack): Python binds it fine, then the body raises into the
/// broad `except` (`ValidationError`/`TypeError` → `return`). Only a
/// structurally broken envelope (not an array, too few args) is `Err`
/// (requeue — Celery itself would fail the call the same way).
fn task_string(value: &Value) -> Parsed<String> {
    match value {
        Value::String(s) => Ok(Some(s.clone())),
        _ => Ok(None),
    }
}

/// The `updated_issue` payload argument: `None` passes through (the flow
/// maps falsy to `{}`); a non-string aborts the whole call, since
/// `json.loads` would raise into the broad `except`.
fn payload_arg(value: &Value) -> Parsed<Option<String>> {
    match value {
        Value::Null => Ok(Some(None)),
        Value::String(s) => Ok(Some(Some(s.clone()))),
        _ => Ok(None),
    }
}

fn kwargs_map(kwargs: &Value) -> Result<&Map<String, Value>, String> {
    match kwargs {
        Value::Object(map) => Ok(map),
        Value::Null => Err("version task: kwargs missing".to_string()),
        _ => Err("version task: kwargs is not an object".to_string()),
    }
}

/// Python truthiness for the `is_creating` flag (`not is_creating`):
/// missing/`None` → false; numbers → nonzero; strings/containers →
/// non-empty.
pub fn celery_truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => {
            if let Some(i) = n.as_i64() {
                i != 0
            } else if let Some(f) = n.as_f64() {
                f != 0.0
            } else {
                true
            }
        }
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(m)) => !m.is_empty(),
    }
}

/// Strict int for the sync-family kwargs (no `int()` coercion there):
/// JSON ints and bools (`True` is an `int`) pass; floats/strings abort the
/// task the way the `TypeError`/`ValueError` downstream would.
fn strict_int(value: Option<&Value>, default: i64) -> Parsed<i64> {
    match value {
        None | Some(Value::Null) => Ok(Some(default)),
        Some(Value::Bool(b)) => Ok(Some(i64::from(*b))),
        Some(Value::Number(n)) => match n.as_i64() {
            Some(i) => Ok(Some(i)),
            None => Ok(None),
        },
        Some(_) => Ok(None),
    }
}

/// Bind one Celery argument kwargs-first with positional fallback,
/// mirroring how Celery binds a call: every live producer sends
/// kwargs-only, so a positional-only parser would requeue production
/// traffic forever. Missing in both places is an envelope failure
/// (`Err`, requeue): Python raises `TypeError` at call time the same way.
fn bind_arg<'a>(
    map: Option<&'a Map<String, Value>>,
    items: &'a [Value],
    key: &str,
    index: usize,
) -> Result<&'a Value, String> {
    map.and_then(|m| m.get(key))
        .or_else(|| items.get(index))
        .ok_or_else(|| format!("version task: missing argument {key}"))
}

/// `issue_task(updated_issue, issue_id, user_id)`
/// (`issue_version_sync.py:34`).
#[derive(Debug, Clone, PartialEq)]
pub struct IssueTaskCall {
    pub updated_issue: Option<String>,
    pub issue_id: String,
    pub user_id: String,
}

pub fn parse_issue_task_call(args: &Value, kwargs: &Value) -> Parsed<IssueTaskCall> {
    let items = ensure_args_array(args)?;
    let map = kwargs_map(kwargs).ok();
    let updated_issue = match payload_arg(bind_arg(map, items, "updated_issue", 0)?)? {
        Some(updated) => updated,
        None => return Ok(None),
    };
    let issue_id = match task_string(bind_arg(map, items, "issue_id", 1)?)? {
        Some(id) => id,
        None => return Ok(None),
    };
    let user_id = match task_string(bind_arg(map, items, "user_id", 2)?)? {
        Some(id) => id,
        None => return Ok(None),
    };
    Ok(Some(IssueTaskCall {
        updated_issue,
        issue_id,
        user_id,
    }))
}

/// `sync_*(batch_size=…, offset=…, countdown=…)` from kwargs with
/// positional fallback, mirroring Celery argument binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyncCall {
    pub batch_size: i64,
    pub offset: i64,
    pub countdown_secs: i64,
}

pub fn parse_sync_call(
    args: &Value,
    kwargs: &Value,
    default_batch: i64,
    default_countdown: i64,
) -> Parsed<SyncCall> {
    let items = ensure_args_array(args)?;
    let map = kwargs_map(kwargs).ok();
    let slot = |key: &str, index: usize, default: i64| -> Result<Option<i64>, String> {
        let from_kwargs = map.and_then(|m| m.get(key));
        let value = from_kwargs.or_else(|| items.get(index));
        strict_int(value, default)
    };
    // A non-integer in any slot aborts the whole task (the `TypeError`
    // the arithmetic downstream would raise, caught by the broad `except`).
    let batch_size = match slot("batch_size", 0, default_batch)? {
        Some(b) => b,
        None => return Ok(None),
    };
    let offset = match slot("offset", 1, v::SYNC_DEFAULT_OFFSET)? {
        Some(o) => o,
        None => return Ok(None),
    };
    let countdown_secs = match slot("countdown", 2, default_countdown)? {
        Some(c) => c,
        None => return Ok(None),
    };
    Ok(Some(SyncCall {
        batch_size,
        offset,
        countdown_secs,
    }))
}

/// `schedule_*(batch_size=…, countdown=…)` kwargs. `batch_size` goes
/// through `int()` (`schedule_issue_version.py:236`); `countdown` is raw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScheduleCall {
    pub batch_size: i64,
    pub countdown_secs: i64,
}

pub fn parse_schedule_call(kwargs: &Value, default_batch: i64) -> Parsed<ScheduleCall> {
    // A bare `.delay()` with no kwargs envelope still binds Python's
    // parameter defaults (`batch_size=5000, countdown=300`) and enqueues;
    // only an `int()`/`TypeError` failure downstream aborts the task.
    let empty;
    let map = match kwargs_map(kwargs) {
        Ok(m) => m,
        Err(_) => {
            empty = Map::new();
            &empty
        }
    };
    let batch_size = match map.get("batch_size") {
        None | Some(Value::Null) => default_batch,
        Some(v) => match v::coerce_batch_size(v, default_batch) {
            Ok(b) => b,
            Err(_) => return Ok(None),
        },
    };
    let countdown_secs = match strict_int(map.get("countdown"), v::SYNC_DEFAULT_COUNTDOWN_SECS)? {
        Some(c) => c,
        None => return Ok(None),
    };
    Ok(Some(ScheduleCall {
        batch_size,
        countdown_secs,
    }))
}

/// `issue_description_version_task(updated_issue, issue_id, user_id,
/// is_creating=False)` (`issue_description_version_task.py:44`).
#[derive(Debug, Clone, PartialEq)]
pub struct DescriptionTaskCall {
    pub updated_issue: Option<String>,
    pub issue_id: String,
    pub user_id: String,
    pub is_creating: bool,
}

pub fn parse_description_task_call(args: &Value, kwargs: &Value) -> Parsed<DescriptionTaskCall> {
    let items = ensure_args_array(args)?;
    let map = kwargs_map(kwargs).ok();
    let updated_issue = match payload_arg(bind_arg(map, items, "updated_issue", 0)?)? {
        Some(updated) => updated,
        None => return Ok(None),
    };
    let issue_id = match task_string(bind_arg(map, items, "issue_id", 1)?)? {
        Some(id) => id,
        None => return Ok(None),
    };
    let user_id = match task_string(bind_arg(map, items, "user_id", 2)?)? {
        Some(id) => id,
        None => return Ok(None),
    };
    // `is_creating=False` is the fourth parameter: kwargs-first with
    // positional fallback, same Celery binding as the other three.
    let flag = map
        .and_then(|m| m.get("is_creating"))
        .or_else(|| items.get(3));
    let is_creating = celery_truthy(flag);
    Ok(Some(DescriptionTaskCall {
        updated_issue,
        issue_id,
        user_id,
        is_creating,
    }))
}

/// `track_page_version(page_id, existing_instance, user_id)`
/// (`page_version_task.py:22`).
#[derive(Debug, Clone, PartialEq)]
pub struct TrackPageCall {
    pub page_id: String,
    pub existing_instance: Option<String>,
    pub user_id: String,
}

pub fn parse_track_page_call(args: &Value, kwargs: &Value) -> Parsed<TrackPageCall> {
    let items = ensure_args_array(args)?;
    let map = kwargs_map(kwargs).ok();
    // `existing_instance` keeps its `""`-vs-`None` distinction: `None`
    // reads as `{}` but `""` fails `json.loads` — the flow decides.
    let existing_instance = match bind_arg(map, items, "existing_instance", 1)? {
        Value::Null => None,
        Value::String(s) => Some(s.clone()),
        _ => return Ok(None),
    };
    let page_id = match task_string(bind_arg(map, items, "page_id", 0)?)? {
        Some(id) => id,
        None => return Ok(None),
    };
    let user_id = match task_string(bind_arg(map, items, "user_id", 2)?)? {
        Some(id) => id,
        None => return Ok(None),
    };
    Ok(Some(TrackPageCall {
        page_id,
        existing_instance,
        user_id,
    }))
}

// ---------------------------------------------------------------------------
// Message constructors (`.delay()` / `apply_async(countdown=…)` parity)
// ---------------------------------------------------------------------------

/// `sync_*.delay(batch_size=…, countdown=…)` (`:236`, `:125`):
/// immediately-visible job at offset 0.
pub fn sync_delay_job(task: &str, batch_size: i64, countdown_secs: i64) -> NewJob {
    NewJob::new(
        task,
        Value::Array(Vec::new()),
        serde_json::json!({"batch_size": batch_size, "countdown": countdown_secs}),
    )
}

/// `sync_*.apply_async(kwargs={batch_size, offset=end, countdown},
/// countdown=countdown)` (`:218-225`, `:109-116`): the chained batch,
/// visible after the countdown.
pub fn sync_chain_job(task: &str, spec: v::ChainSpec, visible_at: DateTime<Utc>) -> NewJob {
    let mut job = NewJob::new(
        task,
        Value::Array(Vec::new()),
        serde_json::json!({
            "batch_size": spec.batch_size,
            "offset": spec.offset,
            "countdown": spec.countdown_secs,
        }),
    );
    job.visible_at = Some(visible_at);
    job
}

/// `visible_at = now + countdown`, the queue-side half of
/// `apply_async(…, countdown=countdown)`.
pub fn chain_visible_at(now: DateTime<Utc>, countdown_secs: i64) -> DateTime<Utc> {
    now + chrono::Duration::seconds(countdown_secs)
}

// ---------------------------------------------------------------------------
// Python-comparison semantics for the `issue_task` diff
// ---------------------------------------------------------------------------

/// One live row value, typed so [`py_eq`] can apply Python `!=` rules
/// against the deserialized payload.
#[derive(Debug, Clone, PartialEq)]
pub enum LiveValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    Uuid(Uuid),
    Date(NaiveDate),
    DateTime(DateTime<Utc>),
    Json(Value),
    Bytes(Vec<u8>),
}

fn num_eq_int(left: i64, right: &serde_json::Number) -> bool {
    if let Some(r) = right.as_i64() {
        left == r
    } else if let Some(r) = right.as_f64() {
        (left as f64) == r
    } else {
        false
    }
}

/// `getattr(issue, key) != value` with Python typing: a UUID/date/datetime
/// live value never equals its string payload form (different types), so
/// those keys always read as changed; ints/floats/bools cross-compare
/// numerically (`True == 1`); JSON columns deep-compare.
pub fn py_eq(live: &LiveValue, value: &Value) -> bool {
    match (live, value) {
        (LiveValue::Null, Value::Null) => true,
        (LiveValue::Null, _) | (_, Value::Null) => false,
        (LiveValue::Bool(b), Value::Bool(c)) => b == c,
        (LiveValue::Bool(b), Value::Number(n)) => num_eq_int(i64::from(*b), n),
        (LiveValue::Int(i), Value::Number(n)) => num_eq_int(*i, n),
        (LiveValue::Float(f), Value::Number(n)) => n.as_f64().is_some_and(|r| *f == r),
        (LiveValue::Bool(_), _) | (LiveValue::Int(_), _) | (LiveValue::Float(_), _) => false,
        (LiveValue::Str(s), Value::String(t)) => s == t,
        (LiveValue::Str(_), _) => false,
        (LiveValue::Json(j), v) => j == v,
        // UUIDs, dates, datetimes and bytes never equal a JSON scalar.
        _ => false,
    }
}

/// Read one `issues` column as a [`LiveValue`] for the diff.
pub fn issue_live_value(row: &PgRow, column: &str) -> Result<LiveValue, sqlx::Error> {
    match column {
        "id" | "project_id" | "workspace_id" | "parent_id" | "state_id" | "estimate_point_id"
        | "type_id" | "assigned_pod_id" => {
            let id: Uuid = row.try_get(column)?;
            Ok(LiveValue::Uuid(id))
        }
        "created_by_id" | "updated_by_id" | "snoozed_by_id" => Ok(row
            .try_get::<Option<Uuid>, _>(column)?
            .map_or(LiveValue::Null, LiveValue::Uuid)),
        "created_at" | "updated_at" | "completed_at" => {
            let dt: Option<DateTime<Utc>> = row.try_get(column)?;
            Ok(dt.map_or(LiveValue::Null, LiveValue::DateTime))
        }
        "deleted_at" => Ok(row
            .try_get::<Option<DateTime<Utc>>, _>(column)?
            .map_or(LiveValue::Null, LiveValue::DateTime)),
        "start_date" | "target_date" | "archived_at" | "snoozed_at" => Ok(row
            .try_get::<Option<NaiveDate>, _>(column)?
            .map_or(LiveValue::Null, LiveValue::Date)),
        "point" | "complexity_score" | "sequence_id" => Ok(row
            .try_get::<Option<i32>, _>(column)?
            .map_or(LiveValue::Null, |n| LiveValue::Int(i64::from(n)))),
        "sort_order" => Ok(row
            .try_get::<Option<f64>, _>(column)?
            .map_or(LiveValue::Null, LiveValue::Float)),
        "is_draft" => {
            let b: bool = row.try_get(column)?;
            Ok(LiveValue::Bool(b))
        }
        "description_json" => Ok(row
            .try_get::<Option<Value>, _>(column)?
            .map_or(LiveValue::Null, LiveValue::Json)),
        "description_binary" => Ok(row
            .try_get::<Option<Vec<u8>>, _>(column)?
            .map_or(LiveValue::Null, LiveValue::Bytes)),
        // The `Issue` many-to-manys (`columns.json`): `getattr` returns the
        // related manager, which never equals a payload list — these keys
        // always read as changed. (Any other unknown key aborts, matching
        // `AttributeError`; reverse accessors beyond these two never occur
        // in producer payloads.)
        "assignees" | "labels" => Ok(LiveValue::Bytes(Vec::new())),
        _ => Ok(row
            .try_get::<Option<String>, _>(column)?
            .map_or(LiveValue::Null, LiveValue::Str)),
    }
}

/// Column kinds of `issue_versions` for the (dead-in-practice) coalesce
/// update binder: payload JSON values convert the way Django field
/// `to_python` coerces on `save()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VersionColumnKind {
    Uuid,
    UuidList,
    Text,
    Int,
    Float,
    Bool,
    Date,
    DateTime,
    Json,
}

fn version_column_kind(column: &str) -> Option<VersionColumnKind> {
    match column {
        "parent" | "state" | "estimate_point" | "type" | "cycle" | "issue_id" | "activity_id"
        | "owned_by_id" | "project_id" | "workspace_id" | "created_by_id" | "updated_by_id"
        | "id" => Some(VersionColumnKind::Uuid),
        "assignees" | "labels" | "modules" => Some(VersionColumnKind::UuidList),
        "name" | "priority" | "external_source" | "external_id" => Some(VersionColumnKind::Text),
        "sequence_id" => Some(VersionColumnKind::Int),
        "sort_order" => Some(VersionColumnKind::Float),
        "is_draft" => Some(VersionColumnKind::Bool),
        "start_date" | "target_date" | "archived_at" => Some(VersionColumnKind::Date),
        "completed_at" | "last_saved_at" | "created_at" | "updated_at" | "deleted_at" => {
            Some(VersionColumnKind::DateTime)
        }
        "properties" | "meta" => Some(VersionColumnKind::Json),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Row mapping (PgRow → service snapshots)
// ---------------------------------------------------------------------------

fn row_issue_snapshot(row: &PgRow) -> Result<v::IssueSnapshot, String> {
    let get = |col: &str| -> Result<String, String> {
        row.try_get::<Option<String>, _>(col)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("null {col}"))
    };
    let get_uuid_opt = |col: &str| -> Result<Option<Uuid>, String> {
        let raw: Option<Uuid> = row.try_get(col).map_err(|e| e.to_string())?;
        Ok(raw)
    };
    Ok(v::IssueSnapshot {
        workspace_id: row.try_get("workspace_id").map_err(|e| e.to_string())?,
        project_id: row.try_get("project_id").map_err(|e| e.to_string())?,
        created_by: get_uuid_opt("created_by_id")?,
        updated_by: get_uuid_opt("updated_by_id")?,
        id: row.try_get("id").map_err(|e| e.to_string())?,
        parent: get_uuid_opt("parent_id")?,
        state: get_uuid_opt("state_id")?,
        estimate_point: get_uuid_opt("estimate_point_id")?,
        name: get("name")?,
        priority: get("priority")?,
        start_date: row.try_get("start_date").map_err(|e| e.to_string())?,
        target_date: row.try_get("target_date").map_err(|e| e.to_string())?,
        sequence_id: row.try_get("sequence_id").map_err(|e| e.to_string())?,
        sort_order: row.try_get("sort_order").map_err(|e| e.to_string())?,
        completed_at: row.try_get("completed_at").map_err(|e| e.to_string())?,
        archived_at: row.try_get("archived_at").map_err(|e| e.to_string())?,
        is_draft: row.try_get("is_draft").map_err(|e| e.to_string())?,
        external_source: row
            .try_get::<Option<String>, _>("external_source")
            .map_err(|e| e.to_string())?,
        external_id: row
            .try_get::<Option<String>, _>("external_id")
            .map_err(|e| e.to_string())?,
        type_id: get_uuid_opt("type_id")?,
    })
}

fn row_description_snapshot(row: &PgRow) -> Result<v::DescriptionSnapshot, String> {
    Ok(v::DescriptionSnapshot {
        workspace_id: row.try_get("workspace_id").map_err(|e| e.to_string())?,
        project_id: row.try_get("project_id").map_err(|e| e.to_string())?,
        created_by: row.try_get("created_by_id").map_err(|e| e.to_string())?,
        updated_by: row.try_get("updated_by_id").map_err(|e| e.to_string())?,
        issue_id: row.try_get("id").map_err(|e| e.to_string())?,
        binary: row
            .try_get("description_binary")
            .map_err(|e| e.to_string())?,
        html: row.try_get("description_html").map_err(|e| e.to_string())?,
        stripped: row
            .try_get("description_stripped")
            .map_err(|e| e.to_string())?,
        json: row
            .try_get::<Option<Value>, _>("description_json")
            .map_err(|e| e.to_string())?
            .unwrap_or(Value::Null),
    })
}

fn row_page_snapshot(row: &PgRow) -> Result<v::PageSnapshot, String> {
    Ok(v::PageSnapshot {
        workspace_id: row.try_get("workspace_id").map_err(|e| e.to_string())?,
        id: row.try_get("id").map_err(|e| e.to_string())?,
        binary: row
            .try_get("description_binary")
            .map_err(|e| e.to_string())?,
        html: row.try_get("description_html").map_err(|e| e.to_string())?,
        json: row
            .try_get::<Option<Value>, _>("description_json")
            .map_err(|e| e.to_string())?
            .unwrap_or(Value::Null),
    })
}

/// `current.get("description_html")` vs the live html, compared as raw
/// JSON so a missing key reads as `Null` — exactly Python's `.get`
/// returning `None` on both sides.
pub fn html_unchanged(payload: &Map<String, Value>, live_html: Option<&str>) -> bool {
    let current = payload.get("description_html").unwrap_or(&Value::Null);
    let live = live_html.map_or(Value::Null, |s| Value::String(s.to_string()));
    current == &live
}

// ---------------------------------------------------------------------------
// Writes (explicit binds in `_meta` column order)
// ---------------------------------------------------------------------------

fn ctx_for(workspace_id: Uuid, actor: Option<Uuid>) -> RequestContext {
    RequestContext::new(
        WorkspaceId::new(workspace_id.to_string()),
        actor.map(|a| UserId::new(a.to_string())),
    )
}

fn bind_issue_version_row<'a>(
    mut query: sqlx::query::Query<'a, sqlx::Postgres, sqlx::postgres::PgArguments>,
    row: &'a v::NewIssueVersion,
) -> sqlx::query::Query<'a, sqlx::Postgres, sqlx::postgres::PgArguments> {
    query = query
        .bind(row.created_at)
        .bind(row.updated_at)
        .bind(row.created_by)
        .bind(row.updated_by)
        .bind(None::<DateTime<Utc>>)
        .bind(row.id)
        .bind(row.project_id)
        .bind(row.workspace_id)
        .bind(row.parent)
        .bind(row.state)
        .bind(row.estimate_point)
        .bind(row.name.clone())
        .bind(row.priority.clone())
        .bind(row.start_date)
        .bind(row.target_date)
        .bind(row.assignees.clone())
        .bind(row.sequence_id)
        .bind(row.labels.clone())
        .bind(row.sort_order)
        .bind(row.completed_at)
        .bind(row.archived_at)
        .bind(row.is_draft)
        .bind(row.external_source.clone())
        .bind(row.external_id.clone())
        .bind(row.type_id)
        .bind(row.cycle)
        .bind(row.modules.clone())
        .bind(sqlx::types::Json(row.properties.clone()))
        .bind(sqlx::types::Json(row.meta.clone()))
        .bind(row.last_saved_at)
        .bind(row.issue_id)
        .bind(row.activity_id)
        .bind(row.owned_by_id);
    query
}

async fn insert_issue_version_rows(
    conn: &mut sqlx::postgres::PgConnection,
    _ctx: &RequestContext,
    rows: &[v::NewIssueVersion],
) -> Result<(), sqlx::Error> {
    for chunk in rows.chunks(v::ISSUE_VERSION_BULK_BATCH_SIZE) {
        let sql = q::bulk_insert_sql("issue_versions", &q::ISSUE_VERSION_COLUMNS, chunk.len());
        let mut query = sqlx::query(&sql);
        for row in chunk {
            query = bind_issue_version_row(query, row);
        }
        query.execute(&mut *conn).await?;
    }
    Ok(())
}

fn bind_description_version_row<'a>(
    mut query: sqlx::query::Query<'a, sqlx::Postgres, sqlx::postgres::PgArguments>,
    row: &'a v::NewDescriptionVersion,
) -> sqlx::query::Query<'a, sqlx::Postgres, sqlx::postgres::PgArguments> {
    query = query
        .bind(row.created_at)
        .bind(row.updated_at)
        .bind(row.created_by)
        .bind(row.updated_by)
        .bind(None::<DateTime<Utc>>)
        .bind(row.id)
        .bind(row.project_id)
        .bind(row.workspace_id)
        .bind(row.issue_id)
        .bind(row.binary.clone())
        .bind(row.html.clone())
        .bind(row.stripped.clone())
        .bind(sqlx::types::Json(row.json.clone()))
        .bind(row.last_saved_at)
        .bind(row.owned_by_id);
    query
}

async fn insert_description_version_rows(
    conn: &mut sqlx::postgres::PgConnection,
    _ctx: &RequestContext,
    rows: &[v::NewDescriptionVersion],
) -> Result<(), sqlx::Error> {
    // No `batch_size`: one multi-row INSERT (`:105`).
    if rows.is_empty() {
        return Ok(());
    }
    let sql = q::bulk_insert_sql(
        "issue_description_versions",
        &q::ISSUE_DESCRIPTION_VERSION_COLUMNS,
        rows.len(),
    );
    let mut query = sqlx::query(&sql);
    for row in rows {
        query = bind_description_version_row(query, row);
    }
    query.execute(&mut *conn).await?;
    Ok(())
}

async fn insert_page_version_row(
    conn: &mut sqlx::postgres::PgConnection,
    _ctx: &RequestContext,
    row: &v::NewPageVersion,
) -> Result<(), sqlx::Error> {
    let sql = q::insert_sql("page_versions", &q::PAGE_VERSION_COLUMNS);
    sqlx::query(&sql)
        .bind(row.created_at)
        .bind(row.updated_at)
        .bind(None::<Uuid>)
        .bind(None::<Uuid>)
        .bind(None::<DateTime<Utc>>)
        .bind(row.id)
        .bind(row.workspace_id)
        .bind(row.page_id)
        .bind(row.last_saved_at)
        .bind(row.owned_by_id)
        .bind(row.binary.clone())
        .bind(row.html.clone())
        .bind(row.stripped.clone())
        .bind(sqlx::types::Json(row.json.clone()))
        .bind(sqlx::types::Json(row.sub_pages_data.clone()))
        .execute(&mut *conn)
        .await?;
    Ok(())
}

async fn update_description_version_row(
    conn: &mut sqlx::postgres::PgConnection,
    _ctx: &RequestContext,
    id: Uuid,
    snapshot: &v::DescriptionSnapshot,
    now: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    let sql = q::update_fields_sql("issue_description_versions", &q::DESCRIPTION_UPDATE_COLUMNS);
    sqlx::query(&sql)
        .bind(sqlx::types::Json(snapshot.json.clone()))
        .bind(snapshot.html.clone())
        .bind(snapshot.binary.clone())
        .bind(snapshot.stripped.clone())
        .bind(now)
        .bind(id)
        .execute(&mut *conn)
        .await?;
    Ok(())
}

async fn update_page_version_row(
    conn: &mut sqlx::postgres::PgConnection,
    _ctx: &RequestContext,
    id: Uuid,
    update: &v::PageVersionUpdate,
) -> Result<(), sqlx::Error> {
    let sql = q::update_fields_sql("page_versions", &q::PAGE_VERSION_UPDATE_COLUMNS);
    sqlx::query(&sql)
        .bind(update.html.clone())
        .bind(update.binary.clone())
        .bind(sqlx::types::Json(update.json.clone()))
        .bind(update.stripped.clone())
        .bind(sqlx::types::Json(update.sub_pages_data.clone()))
        .bind(update.updated_at)
        .bind(id)
        .execute(&mut *conn)
        .await?;
    Ok(())
}

/// Bind one payload value for the dead-path coalesce update, converting
/// the way Django field `to_python` coerces on `save()`; `Err` mirrors the
/// `FieldError`/`ValidationError` the broad `except` swallows.
fn push_version_value(
    builder: &mut sqlx::QueryBuilder<'_, sqlx::Postgres>,
    column: &str,
    value: &Value,
) -> Result<(), String> {
    let kind =
        version_column_kind(column).ok_or_else(|| format!("not a version column: {column}"))?;
    match kind {
        VersionColumnKind::Uuid => match value {
            Value::String(s) => match Uuid::parse_str(s) {
                Ok(id) => {
                    builder.push_bind(id);
                    Ok(())
                }
                Err(_) => Err(format!("bad uuid for {column}")),
            },
            _ => Err(format!("bad uuid for {column}")),
        },
        VersionColumnKind::UuidList => match value {
            Value::Array(items) => {
                let mut ids = Vec::with_capacity(items.len());
                for item in items {
                    match item {
                        Value::String(s) => match Uuid::parse_str(s) {
                            Ok(id) => ids.push(id),
                            Err(_) => return Err(format!("bad uuid in {column}")),
                        },
                        _ => return Err(format!("bad uuid in {column}")),
                    }
                }
                builder.push_bind(ids);
                Ok(())
            }
            _ => Err(format!("bad uuid list for {column}")),
        },
        VersionColumnKind::Text => match value {
            Value::String(s) => {
                builder.push_bind(s.clone());
                Ok(())
            }
            _ => Err(format!("bad text for {column}")),
        },
        VersionColumnKind::Int => match value.as_i64() {
            Some(n) => {
                let narrowed: i32 = n
                    .try_into()
                    .map_err(|_| format!("int out of range for {column}"))?;
                builder.push_bind(narrowed);
                Ok(())
            }
            None => Err(format!("bad int for {column}")),
        },
        VersionColumnKind::Float => match value.as_f64() {
            Some(f) => {
                builder.push_bind(f);
                Ok(())
            }
            None => Err(format!("bad float for {column}")),
        },
        VersionColumnKind::Bool => match value.as_bool() {
            Some(b) => {
                builder.push_bind(b);
                Ok(())
            }
            None => Err(format!("bad bool for {column}")),
        },
        VersionColumnKind::Date => match value {
            Value::String(s) => match NaiveDate::parse_from_str(s, "%Y-%m-%d") {
                Ok(d) => {
                    builder.push_bind(d);
                    Ok(())
                }
                Err(_) => Err(format!("bad date for {column}")),
            },
            _ => Err(format!("bad date for {column}")),
        },
        VersionColumnKind::DateTime => match value {
            Value::String(s) => match parse_task_datetime(s) {
                Some(dt) => {
                    builder.push_bind(dt);
                    Ok(())
                }
                None => Err(format!("bad datetime for {column}")),
            },
            _ => Err(format!("bad datetime for {column}")),
        },
        VersionColumnKind::Json => {
            builder.push_bind(sqlx::types::Json(value.clone()));
            Ok(())
        }
    }
}

/// Datetime strings Django's `DateTimeField.to_python` accepts, in
/// preference order: RFC 3339 (aware), then naive ISO forms assumed UTC
/// (the project default timezone).
fn parse_task_datetime(value: &str) -> Option<DateTime<Utc>> {
    if let Ok(dt) = value.parse::<DateTime<Utc>>() {
        return Some(dt);
    }
    for format in ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%d %H:%M:%S%.f"] {
        if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(value, format) {
            return Some(DateTime::from_naive_utc_and_offset(naive, Utc));
        }
    }
    if let Ok(date) = NaiveDate::parse_from_str(value, "%Y-%m-%d") {
        return Some(date.and_hms_opt(0, 0, 0)?.and_utc());
    }
    None
}

// ---------------------------------------------------------------------------
// Typed diff (`issue_task`, `issue_version_sync.py:39-42`)
// ---------------------------------------------------------------------------

/// Changed keys of the payload against the live row. `Err` on the first
/// unknown key mirrors the `AttributeError` `getattr(issue, key)` raises,
/// which aborts the whole task into the broad `except` (`:60-64`).
pub fn diff_issue_row(
    payload: &Map<String, Value>,
    live: &std::collections::HashMap<String, LiveValue>,
    known: &HashSet<&str>,
) -> Result<Vec<String>, String> {
    let mut changed = Vec::new();
    for (key, value) in payload {
        if !known.contains(key.as_str()) {
            return Err(key.clone());
        }
        let live_value = live.get(key).ok_or_else(|| key.clone())?;
        if !py_eq(live_value, value) {
            changed.push(key.clone());
        }
    }
    Ok(changed)
}

// ---------------------------------------------------------------------------
// Shared reads
// ---------------------------------------------------------------------------

struct LatestOwner {
    id: Uuid,
    username: String,
    email: String,
    last_saved_at: DateTime<Utc>,
}

async fn fetch_latest_owner(
    conn: &mut sqlx::postgres::PgConnection,
    issue_id: Uuid,
) -> Result<Option<LatestOwner>, sqlx::Error> {
    let row: Option<PgRow> = sqlx::query(q::LATEST_ISSUE_VERSION_OWNER_SQL)
        .bind(issue_id)
        .fetch_optional(&mut *conn)
        .await?;
    row.map(|row| {
        Ok(LatestOwner {
            id: row.try_get("id")?,
            username: row.try_get("username")?,
            email: row.try_get("email")?,
            last_saved_at: row.try_get("last_saved_at")?,
        })
    })
    .transpose()
}

struct LatestVersion {
    id: Uuid,
    owned_by_id: Uuid,
    last_saved_at: DateTime<Utc>,
}

async fn fetch_latest_version(
    conn: &mut sqlx::postgres::PgConnection,
    sql: &str,
    fk: Uuid,
) -> Result<Option<LatestVersion>, sqlx::Error> {
    let row: Option<PgRow> = sqlx::query(sql).bind(fk).fetch_optional(&mut *conn).await?;
    row.map(|row| {
        Ok(LatestVersion {
            id: row.try_get("id")?,
            owned_by_id: row.try_get("owned_by_id")?,
            last_saved_at: row.try_get("last_saved_at")?,
        })
    })
    .transpose()
}

async fn fetch_admin_member(
    conn: &mut sqlx::postgres::PgConnection,
    project_id: Uuid,
) -> Result<Option<Uuid>, sqlx::Error> {
    let row: Option<PgRow> = sqlx::query(q::ADMIN_FALLBACK_SQL)
        .bind(project_id)
        .fetch_optional(&mut *conn)
        .await?;
    row.map(|row| row.try_get("member_id")).transpose()
}

async fn fetch_related(
    conn: &mut sqlx::postgres::PgConnection,
    ids: &[Uuid],
) -> Result<v::RelatedData, sqlx::Error> {
    use v::{group_pairs, latest_per_issue};
    let mut related = v::RelatedData::default();
    if ids.is_empty() {
        return Ok(related);
    }
    macro_rules! fetch_bound {
        ($sql:expr) => {{
            let sql = $sql;
            let mut query = sqlx::query(&sql);
            for id in ids {
                query = query.bind(*id);
            }
            query.fetch_all(&mut *conn).await
        }};
    }
    // Cycle map: `Meta.ordering` (`created_at DESC`); the dict
    // comprehension keeps the last row per issue over this order.
    let rows = fetch_bound!(q::cycle_issues_sql(ids.len()))?;
    for row in rows {
        let issue: Uuid = row.try_get("issue_id")?;
        let cycle: Option<Uuid> = row.try_get("cycle_id")?;
        related
            .cycle_issues
            .insert(issue.to_string(), cycle.map(|c| c.to_string()));
    }
    // Grouped m2m lists, ordered by issue_id for the groupby.
    for (table, value_col, slot) in [
        ("issue_assignees", "assignee_id", 0),
        ("issue_labels", "label_id", 1),
        ("module_issues", "module_id", 2),
    ] {
        let rows: Vec<PgRow> =
            fetch_bound!(q::related_in_sql(table, "issue_id", value_col, ids.len()))?;
        let pairs: Vec<(String, String)> = rows
            .iter()
            .map(|row| {
                let issue: Uuid = row.try_get("issue_id")?;
                let value: Uuid = row.try_get(value_col)?;
                Ok((issue.to_string(), value.to_string()))
            })
            .collect::<Result<_, sqlx::Error>>()?;
        let grouped = group_pairs(&pairs);
        match slot {
            0 => related.assignees = grouped,
            1 => related.labels = grouped,
            _ => related.modules = grouped,
        }
    }
    // Latest activity per issue.
    let rows: Vec<PgRow> = fetch_bound!(q::activities_sql(ids.len()))?;
    let pairs: Vec<(String, String)> = rows
        .iter()
        .map(|row| {
            let issue: Uuid = row.try_get("issue_id")?;
            let activity: Uuid = row.try_get("id")?;
            Ok((issue.to_string(), activity.to_string()))
        })
        .collect::<Result<_, sqlx::Error>>()?;
    related.activities = latest_per_issue(&pairs);
    Ok(related)
}

// ---------------------------------------------------------------------------
// Task flows (each mirrors one `@shared_task` body)
// ---------------------------------------------------------------------------

/// Parse an `updated_issue`-style payload: `None` and `""` (both falsy in
/// Python) read as `{}`; anything unparseable or non-object yields `None`
/// (≈ `json.loads` raising into the broad `except`).
fn payload_map(raw: Option<&str>) -> Option<Map<String, Value>> {
    match raw {
        None | Some("") => Some(Map::new()),
        Some(s) => serde_json::from_str::<Value>(s).ok().and_then(|v| match v {
            Value::Object(map) => Some(map),
            _ => None,
        }),
    }
}

/// `issue_task` (`issue_version_sync.py:34-64`).
pub async fn run_issue_task(pool: &sqlx::PgPool, call: &IssueTaskCall) -> Result<(), String> {
    let payload = match payload_map(call.updated_issue.as_deref()) {
        Some(map) => map,
        None => {
            tracing::warn!("issue_task: unparseable updated_issue payload");
            return Ok(());
        }
    };
    let issue_id = match Uuid::parse_str(&call.issue_id) {
        Ok(id) => id,
        Err(_) => return Ok(()),
    };
    let row = q::fetch_issue_row(pool, issue_id)
        .await
        .map_err(|e| e.to_string())?;
    let row = match row {
        Some(row) => row,
        None => return Ok(()),
    };
    let mut known: HashSet<&str> = q::ISSUE_COLUMNS.iter().copied().collect();
    known.insert("assignees");
    known.insert("labels");
    // Unknown keys must ack (`Ok`), never requeue: `getattr(issue, key)`
    // raises `AttributeError` into the broad `except`. The membership
    // check runs BEFORE any column read — a missing column would
    // otherwise surface as a transport `Err` and requeue.
    for key in payload.keys() {
        if !known.contains(key.as_str()) {
            tracing::warn!("issue_task: unknown payload key {key}");
            return Ok(());
        }
    }
    let mut live = std::collections::HashMap::new();
    for key in payload.keys() {
        live.insert(
            key.clone(),
            issue_live_value(&row, key).map_err(|e| e.to_string())?,
        );
    }
    let changed = match diff_issue_row(&payload, &live, &known) {
        Ok(changed) => changed,
        Err(_) => return Ok(()),
    };
    if !v::issue_task_writes(&changed) {
        return Ok(());
    }
    let now = Utc::now();
    let mut conn = pool.acquire().await.map_err(|e| e.to_string())?;
    let owner = fetch_latest_owner(&mut conn, issue_id)
        .await
        .map_err(|e| e.to_string())?;
    match owner {
        // The `else` branch calls `IssueVersion.log_issue_version`, which
        // ALWAYS raises `FieldError` (`Module` has no `issue` relation:
        // `Module.objects.filter(issue=...)` in `issue.py:896`) and
        // returns `False` — so the branch never writes and always acks.
        // Ported as that observable behavior: no write, ack.
        None => Ok(()),
        Some(o) => {
            let display = v::render_owner_display(&o.username, &o.email);
            let age = (now - o.last_saved_at)
                .num_microseconds()
                .unwrap_or(i64::MAX);
            if v::issue_task_coalesces(&display, &call.user_id, age) {
                update_issue_version(pool, o.id, &payload, &changed, now).await
            } else {
                Ok(())
            }
        }
    }
}

/// The dead-in-practice coalesce branch (`:47-55`): a changed key outside
/// the version columns aborts (≈ `FieldError` from
/// `save(update_fields=[…])`).
async fn update_issue_version(
    pool: &sqlx::PgPool,
    version_id: Uuid,
    payload: &Map<String, Value>,
    changed: &[String],
    now: DateTime<Utc>,
) -> Result<(), String> {
    if changed.iter().any(|k| version_column_kind(k).is_none()) {
        return Ok(());
    }
    let columns: Vec<String> = changed
        .iter()
        .cloned()
        .chain(["last_saved_at".to_string()])
        .collect();
    let refs: Vec<&str> = columns.iter().map(String::as_str).collect();
    let mut builder = sqlx::QueryBuilder::new(q::update_fields_sql("issue_versions", &refs));
    for key in changed {
        let value = payload.get(key).unwrap_or(&Value::Null);
        if push_version_value(&mut builder, key, value).is_err() {
            return Ok(());
        }
    }
    builder.push_bind(now);
    builder.push_bind(version_id);
    builder
        .build()
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// `sync_issue_version` (`issue_version_sync.py:181-231`): one
/// `transaction.atomic()` block — count, window, batch, related, build,
/// chunked bulk insert, same-transaction chain.
pub async fn run_sync_issue_versions(
    pool: &sqlx::PgPool,
    batch_size: i64,
    offset: i64,
    countdown_secs: i64,
) -> Result<(), String> {
    let now = Utc::now();
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
    let total: i64 = sqlx::query_scalar(q::ISSUE_COUNT_SQL)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    let window = match v::batch_window(total, batch_size, offset) {
        Some(window) => window,
        None => return Ok(()),
    };
    let rows = sqlx::query(&q::issues_batch_sql(window.limit, offset))
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    if rows.is_empty() {
        return Ok(());
    }
    let ids: Vec<Uuid> = rows
        .iter()
        .map(|row| row.try_get("id"))
        .collect::<Result<_, _>>()
        .map_err(|e| e.to_string())?;
    let related = fetch_related(&mut tx, &ids)
        .await
        .map_err(|e| e.to_string())?;
    // Workspace order of first appearance (Python inserts in batch order).
    let mut grouped: Vec<(Uuid, Vec<v::NewIssueVersion>)> = Vec::new();
    for row in &rows {
        let snapshot = row_issue_snapshot(row).map_err(|e| e.to_string())?;
        let owner = match (snapshot.updated_by, snapshot.created_by) {
            (Some(id), _) | (None, Some(id)) => Some(id),
            (None, None) => fetch_admin_member(&mut tx, snapshot.project_id)
                .await
                .map_err(|e| e.to_string())?,
        };
        let owner = match owner {
            Some(owner) => owner,
            None => {
                tracing::warn!("sync_issue_version: skipping {}", snapshot.id);
                continue;
            }
        };
        match v::build_issue_version(&snapshot, owner, &related, now, Uuid::new_v4()) {
            Some(version) => match grouped
                .iter_mut()
                .find(|(ws, _)| *ws == snapshot.workspace_id)
            {
                Some((_, versions)) => versions.push(version),
                None => grouped.push((snapshot.workspace_id, vec![version])),
            },
            None => {
                tracing::warn!("sync_issue_version: skipping {}", snapshot.id);
            }
        }
    }
    for (workspace_id, versions) in &grouped {
        let actor = versions.first().map(|r| r.owned_by_id);
        let ctx = ctx_for(*workspace_id, actor);
        insert_issue_version_rows(&mut tx, &ctx, versions)
            .await
            .map_err(|e| e.to_string())?;
    }
    if v::should_chain(window.end_offset, total) {
        let spec = v::chain_spec(batch_size, window.end_offset, countdown_secs);
        let job = sync_chain_job(
            TASK_SYNC_ISSUE_VERSION,
            spec,
            chain_visible_at(now, countdown_secs),
        );
        enqueue_exec(&mut *tx, &job)
            .await
            .map_err(|e| e.to_string())?;
    }
    tracing::info!("Processed Issues: {}", window.end_offset);
    tx.commit().await.map_err(|e| e.to_string())?;
    Ok(())
}

/// `schedule_issue_version` (`issue_version_sync.py:235-236`): immediate
/// `.delay()` of the sync task at offset 0.
pub async fn run_schedule_issue_version(
    pool: &sqlx::PgPool,
    batch_size: i64,
    countdown_secs: i64,
) -> Result<(), String> {
    enqueue(
        pool,
        &sync_delay_job(TASK_SYNC_ISSUE_VERSION, batch_size, countdown_secs),
    )
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// `sync_issue_description_version`
/// (`issue_description_version_sync.py:40-120`).
pub async fn run_sync_description_versions(
    pool: &sqlx::PgPool,
    batch_size: i64,
    offset: i64,
    countdown_secs: i64,
) -> Result<(), String> {
    let now = Utc::now();
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
    let total: i64 = sqlx::query_scalar(q::ISSUE_COUNT_SQL)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    let window = match v::batch_window(total, batch_size, offset) {
        Some(window) => window,
        None => return Ok(()),
    };
    let rows = sqlx::query(&q::issues_batch_limited_sql(window.limit, offset))
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    if rows.is_empty() {
        return Ok(());
    }
    let mut grouped: Vec<(Uuid, Vec<v::NewDescriptionVersion>)> = Vec::new();
    for row in &rows {
        let workspace_id: Option<Uuid> = row.try_get("workspace_id").map_err(|e| e.to_string())?;
        let project_id: Option<Uuid> = row.try_get("project_id").map_err(|e| e.to_string())?;
        let (workspace_id, project_id) = match (workspace_id, project_id) {
            (Some(w), Some(p)) => (w, p),
            _ => {
                tracing::warn!("sync_issue_description_version: skipping row without scope");
                continue;
            }
        };
        let mut snapshot = row_description_snapshot(row).map_err(|e| e.to_string())?;
        snapshot.workspace_id = workspace_id;
        snapshot.project_id = project_id;
        let owner = match (snapshot.updated_by, snapshot.created_by) {
            (Some(id), _) | (None, Some(id)) => Some(id),
            (None, None) => fetch_admin_member(&mut tx, project_id)
                .await
                .map_err(|e| e.to_string())?,
        };
        let owner = match owner {
            Some(owner) => owner,
            None => {
                tracing::warn!("sync_issue_description_version: skipping row without owner");
                continue;
            }
        };
        let version = v::build_description_version(&snapshot, owner, now, Uuid::new_v4());
        match grouped.iter_mut().find(|(ws, _)| *ws == workspace_id) {
            Some((_, versions)) => versions.push(version),
            None => grouped.push((workspace_id, vec![version])),
        }
    }
    for (workspace_id, versions) in &grouped {
        let actor = versions.first().map(|r| r.owned_by_id);
        let ctx = ctx_for(*workspace_id, actor);
        insert_description_version_rows(&mut tx, &ctx, versions)
            .await
            .map_err(|e| e.to_string())?;
    }
    if v::should_chain(window.end_offset, total) {
        let spec = v::chain_spec(batch_size, window.end_offset, countdown_secs);
        let job = sync_chain_job(
            TASK_SYNC_DESCRIPTION_VERSION,
            spec,
            chain_visible_at(now, countdown_secs),
        );
        enqueue_exec(&mut *tx, &job)
            .await
            .map_err(|e| e.to_string())?;
    }
    tx.commit().await.map_err(|e| e.to_string())?;
    Ok(())
}

/// `schedule_issue_description_version`
/// (`issue_description_version_sync.py:124-125`).
pub async fn run_schedule_description_version(
    pool: &sqlx::PgPool,
    batch_size: i64,
    countdown_secs: i64,
) -> Result<(), String> {
    enqueue(
        pool,
        &sync_delay_job(TASK_SYNC_DESCRIPTION_VERSION, batch_size, countdown_secs),
    )
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// `issue_description_version_task`
/// (`issue_description_version_task.py:44-78`).
pub async fn run_description_task(
    pool: &sqlx::PgPool,
    call: &DescriptionTaskCall,
) -> Result<(), String> {
    let payload = match payload_map(call.updated_issue.as_deref()) {
        Some(map) => map,
        None => {
            tracing::warn!("issue_description_version_task: invalid JSON for updated_issue");
            return Ok(());
        }
    };
    let issue_id = match Uuid::parse_str(&call.issue_id) {
        Ok(id) => id,
        Err(_) => return Ok(()),
    };
    let user = match Uuid::parse_str(&call.user_id) {
        Ok(id) => id,
        Err(_) => return Ok(()),
    };
    let row = q::fetch_issue_row(pool, issue_id)
        .await
        .map_err(|e| e.to_string())?;
    let row = match row {
        Some(row) => row,
        None => return Ok(()),
    };
    let snapshot = row_description_snapshot(&row).map_err(|e| e.to_string())?;
    if html_unchanged(&payload, snapshot.html.as_deref()) && !call.is_creating {
        return Ok(());
    }
    let now = Utc::now();
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
    let latest = fetch_latest_version(&mut tx, &q::latest_description_version_sql(), issue_id)
        .await
        .map_err(|e| e.to_string())?;
    let (has, owned, age) = match &latest {
        Some(l) => (
            true,
            l.owned_by_id.to_string(),
            (now - l.last_saved_at)
                .num_microseconds()
                .unwrap_or(i64::MAX),
        ),
        None => (false, String::new(), 0),
    };
    let ctx = ctx_for(snapshot.workspace_id, Some(user));
    match v::should_update_existing(has, &owned, &call.user_id, age) {
        Some(true) => {
            let id = latest.map(|l| l.id).expect("has version");
            update_description_version_row(&mut tx, &ctx, id, &snapshot, now)
                .await
                .map_err(|e| e.to_string())?;
        }
        _ => {
            let version = v::build_description_version(&snapshot, user, now, Uuid::new_v4());
            insert_description_version_rows(&mut tx, &ctx, &[version])
                .await
                .map_err(|e| e.to_string())?;
        }
    }
    tx.commit().await.map_err(|e| e.to_string())?;
    Ok(())
}

/// `track_page_version` (`page_version_task.py:22-81`). No `atomic()` in
/// Python here: each statement commits on its own.
pub async fn run_track_page_version(
    pool: &sqlx::PgPool,
    call: &TrackPageCall,
) -> Result<(), String> {
    // `json.loads(x) if x is not None else {}`: only `None` reads as
    // `{}` — `""` raises inside `json.loads` and aborts the task.
    let existing = match &call.existing_instance {
        None => Map::new(),
        Some(s) => match serde_json::from_str::<Value>(s) {
            Ok(Value::Object(map)) => map,
            _ => return Ok(()),
        },
    };
    let page_id = match Uuid::parse_str(&call.page_id) {
        Ok(id) => id,
        Err(_) => return Ok(()),
    };
    let row = q::fetch_page_row(pool, page_id)
        .await
        .map_err(|e| e.to_string())?;
    let row = match row {
        Some(row) => row,
        None => return Ok(()),
    };
    let snapshot = row_page_snapshot(&row).map_err(|e| e.to_string())?;
    if html_unchanged(&existing, snapshot.html.as_deref()) {
        return Ok(());
    }
    let user = match Uuid::parse_str(&call.user_id) {
        Ok(id) => id,
        Err(_) => return Ok(()),
    };
    let now = Utc::now();
    let ctx = ctx_for(snapshot.workspace_id, Some(user));
    let mut conn = pool.acquire().await.map_err(|e| e.to_string())?;
    let latest = fetch_latest_version(&mut conn, &q::latest_page_version_sql(), page_id)
        .await
        .map_err(|e| e.to_string())?;
    let coalesce = match &latest {
        Some(l) => v::version_coalesces(
            &l.owned_by_id.to_string(),
            &call.user_id,
            (now - l.last_saved_at)
                .num_microseconds()
                .unwrap_or(i64::MAX),
        ),
        None => false,
    };
    if coalesce {
        let id = latest.map(|l| l.id).expect("has version");
        let update = v::build_page_version_update(&snapshot, now);
        update_page_version_row(&mut conn, &ctx, id, &update)
            .await
            .map_err(|e| e.to_string())?;
    } else {
        let version = v::build_page_version(&snapshot, user, now, Uuid::new_v4());
        insert_page_version_row(&mut conn, &ctx, &version)
            .await
            .map_err(|e| e.to_string())?;
    }
    let count: i64 = sqlx::query_scalar(q::PAGE_VERSION_COUNT_SQL)
        .bind(page_id)
        .fetch_one(pool)
        .await
        .map_err(|e| e.to_string())?;
    if v::prune_needed(count) {
        let oldest: Option<Uuid> = sqlx::query_scalar(q::OLDEST_PAGE_VERSION_ID_SQL)
            .bind(page_id)
            .fetch_optional(pool)
            .await
            .map_err(|e| e.to_string())?;
        if let Some(id) = oldest {
            // `.first().delete()` on a `SoftDeleteModel` stamps
            // `deleted_at` — it never removes the row.
            sqlx::query(q::PRUNE_PAGE_VERSION_BY_ID_SQL)
                .bind(now)
                .bind(id)
                .execute(pool)
                .await
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Dispatch + registration
// ---------------------------------------------------------------------------

/// Route one claimed row to its flow. `Ok(())` covers both real success
/// and every task-domain abort Python would `return` out of; `Err` is
/// reserved for envelope and transport failures the worker must requeue.
pub async fn dispatch(
    pool: &sqlx::PgPool,
    task: &str,
    args: &Value,
    kwargs: &Value,
) -> Result<(), HandlerError> {
    match task {
        TASK_ISSUE_TASK => match parse_issue_task_call(args, kwargs) {
            Err(e) => Err(e),
            Ok(None) => Ok(()),
            Ok(Some(call)) => run_issue_task(pool, &call).await,
        },
        TASK_SYNC_ISSUE_VERSION => {
            match parse_sync_call(
                args,
                kwargs,
                v::ISSUE_SYNC_DEFAULT_BATCH_SIZE,
                v::SYNC_DEFAULT_COUNTDOWN_SECS,
            ) {
                Err(e) => Err(e),
                Ok(None) => Ok(()),
                Ok(Some(call)) => {
                    run_sync_issue_versions(pool, call.batch_size, call.offset, call.countdown_secs)
                        .await
                }
            }
        }
        TASK_SCHEDULE_ISSUE_VERSION => {
            match parse_schedule_call(kwargs, v::ISSUE_SYNC_DEFAULT_BATCH_SIZE) {
                Err(e) => Err(e),
                Ok(None) => Ok(()),
                Ok(Some(call)) => {
                    run_schedule_issue_version(pool, call.batch_size, call.countdown_secs).await
                }
            }
        }
        TASK_SYNC_DESCRIPTION_VERSION => {
            match parse_sync_call(
                args,
                kwargs,
                v::DESCRIPTION_SYNC_DEFAULT_BATCH_SIZE,
                v::SYNC_DEFAULT_COUNTDOWN_SECS,
            ) {
                Err(e) => Err(e),
                Ok(None) => Ok(()),
                Ok(Some(call)) => {
                    run_sync_description_versions(
                        pool,
                        call.batch_size,
                        call.offset,
                        call.countdown_secs,
                    )
                    .await
                }
            }
        }
        TASK_SCHEDULE_DESCRIPTION_VERSION => {
            match parse_schedule_call(kwargs, v::DESCRIPTION_SYNC_DEFAULT_BATCH_SIZE) {
                Err(e) => Err(e),
                Ok(None) => Ok(()),
                Ok(Some(call)) => {
                    run_schedule_description_version(pool, call.batch_size, call.countdown_secs)
                        .await
                }
            }
        }
        TASK_DESCRIPTION_TASK => match parse_description_task_call(args, kwargs) {
            Err(e) => Err(e),
            Ok(None) => Ok(()),
            Ok(Some(call)) => run_description_task(pool, &call).await,
        },
        TASK_TRACK_PAGE_VERSION => match parse_track_page_call(args, kwargs) {
            Err(e) => Err(e),
            Ok(None) => Ok(()),
            Ok(Some(call)) => run_track_page_version(pool, &call).await,
        },
        _ => Err(format!("tasks_cleanup: unowned task {task}")),
    }
}

/// Install the worker handlers owning [`ALL_VERSION_TASKS`]. A store
/// failure requeues with the Celery default retry delay (the bare
/// `@shared_task`s carry `max_retries = 3`, `default_retry_delay = 180s`
/// per the jobs plane); the row parks as failed once the budget is spent.
pub fn register_versions(registry: &mut Registry, pool: sqlx::PgPool) {
    for name in ALL_VERSION_TASKS {
        let pool = pool.clone();
        let task = name.to_string();
        let handler: Handler = Arc::new(move |job: JobRow| {
            let pool = pool.clone();
            let task = task.clone();
            Box::pin(async move {
                dispatch(&pool, &task, &job.args, &job.kwargs)
                    .await
                    .map(|()| Verdict::Ack)
            })
        });
        registry.register(name, handler);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use std::collections::HashMap;

    fn args(values: Vec<Value>) -> Value {
        Value::Array(values)
    }

    fn kwargs(pairs: Vec<(&str, Value)>) -> Value {
        Value::Object(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
    }

    fn str_val(s: &str) -> Value {
        Value::String(s.to_string())
    }

    #[test]
    fn wire_names_are_exact() {
        assert_eq!(
            TASK_ISSUE_TASK,
            "pi_dash.bgtasks.issue_version_sync.issue_task"
        );
        assert_eq!(
            TASK_SYNC_ISSUE_VERSION,
            "pi_dash.bgtasks.issue_version_sync.sync_issue_version"
        );
        assert_eq!(
            TASK_SCHEDULE_ISSUE_VERSION,
            "pi_dash.bgtasks.issue_version_sync.schedule_issue_version"
        );
        assert_eq!(
            TASK_SYNC_DESCRIPTION_VERSION,
            "pi_dash.bgtasks.issue_description_version_sync.sync_issue_description_version"
        );
        assert_eq!(
            TASK_SCHEDULE_DESCRIPTION_VERSION,
            "pi_dash.bgtasks.issue_description_version_sync.schedule_issue_description_version"
        );
        assert_eq!(
            TASK_DESCRIPTION_TASK,
            "pi_dash.bgtasks.issue_description_version_task.issue_description_version_task"
        );
        assert_eq!(
            TASK_TRACK_PAGE_VERSION,
            "pi_dash.bgtasks.page_version_task.track_page_version"
        );
        let set: HashSet<&str> = ALL_VERSION_TASKS.iter().copied().collect();
        assert_eq!(set.len(), 7);
    }

    #[test]
    fn issue_task_parsing() {
        let call = parse_issue_task_call(
            &args(vec![
                str_val(r#"{"name": "x"}"#),
                str_val("issue-1"),
                str_val("user-1"),
            ]),
            &Value::Null,
        )
        .unwrap()
        .unwrap();
        assert_eq!(call.updated_issue.as_deref(), Some(r#"{"name": "x"}"#));
        assert_eq!(call.issue_id, "issue-1");
        // Live traffic is kwargs-only (no producer sends positionals):
        // kwargs bind with positional fallback, like Celery.
        let call = parse_issue_task_call(
            &args(vec![]),
            &kwargs(vec![
                ("updated_issue", str_val(r#"{"name": "x"}"#)),
                ("issue_id", str_val("issue-1")),
                ("user_id", str_val("user-1")),
            ]),
        )
        .unwrap()
        .unwrap();
        assert_eq!(call.updated_issue.as_deref(), Some(r#"{"name": "x"}"#));
        assert_eq!(call.issue_id, "issue-1");
        assert_eq!(call.user_id, "user-1");
        // Missing in both places is an envelope failure (requeue), not a
        // task abort.
        assert!(parse_issue_task_call(&args(vec![str_val("a")]), &Value::Null).is_err());
        assert!(parse_issue_task_call(&args(vec![]), &Value::Null).is_err());
        assert!(parse_issue_task_call(&str_val("nope"), &Value::Null).is_err());
        // A non-string issue_id aborts the task (ack): Python binds it,
        // then the body raises into the broad `except`.
        assert!(parse_issue_task_call(
            &args(vec![Value::Null, serde_json::json!(1), str_val("u")]),
            &Value::Null,
        )
        .unwrap()
        .is_none());
        // A non-string payload aborts too (`json.loads` would raise).
        assert!(parse_issue_task_call(
            &args(vec![serde_json::json!(7), str_val("i"), str_val("u")]),
            &Value::Null,
        )
        .unwrap()
        .is_none());
    }

    #[test]
    fn sync_parsing_defaults_overrides_and_aborts() {
        // Bare call → all defaults (5000/0/300).
        let call = parse_sync_call(&args(vec![]), &Value::Null, 5000, 300)
            .unwrap()
            .unwrap();
        assert_eq!(
            call,
            SyncCall {
                batch_size: 5000,
                offset: 0,
                countdown_secs: 300
            }
        );
        // Kwargs override positionals (Celery binding order).
        let call = parse_sync_call(
            &args(vec![
                serde_json::json!(10),
                serde_json::json!(20),
                serde_json::json!(30),
            ]),
            &kwargs(vec![("offset", serde_json::json!(99))]),
            5000,
            300,
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            call,
            SyncCall {
                batch_size: 10,
                offset: 99,
                countdown_secs: 30
            }
        );
        // A float batch aborts the task (downstream `TypeError` parity).
        assert!(parse_sync_call(
            &args(vec![]),
            &kwargs(vec![("batch_size", serde_json::json!(1.5))]),
            5000,
            300
        )
        .unwrap()
        .is_none());
        // A string countdown aborts the task.
        assert!(parse_sync_call(
            &args(vec![]),
            &kwargs(vec![("countdown", str_val("soon"))]),
            5000,
            300
        )
        .unwrap()
        .is_none());
    }

    #[test]
    fn schedule_parsing_applies_int_coercion() {
        // No kwargs envelope still binds Python's parameter defaults
        // (`batch_size=5000, countdown=300`) and enqueues.
        let call = parse_schedule_call(&Value::Null, 5000).unwrap().unwrap();
        assert_eq!(
            call,
            ScheduleCall {
                batch_size: 5000,
                countdown_secs: 300
            }
        );
        let call = parse_schedule_call(
            &kwargs(vec![
                ("batch_size", str_val("25")),
                ("countdown", serde_json::json!(60)),
            ]),
            5000,
        )
        .unwrap()
        .unwrap();
        // `int("25")` coercion from `schedule_issue_version.py:236`.
        assert_eq!(
            call,
            ScheduleCall {
                batch_size: 25,
                countdown_secs: 60
            }
        );
        // Defaults when keys are absent.
        let call = parse_schedule_call(&kwargs(vec![]), 5000).unwrap().unwrap();
        assert_eq!(
            call,
            ScheduleCall {
                batch_size: 5000,
                countdown_secs: 300
            }
        );
    }

    #[test]
    fn description_task_parsing_flag_truthiness() {
        let base = args(vec![Value::Null, str_val("i"), str_val("u")]);
        assert!(
            !parse_description_task_call(&base, &Value::Null)
                .unwrap()
                .unwrap()
                .is_creating
        );
        assert!(
            parse_description_task_call(&base, &kwargs(vec![("is_creating", Value::Bool(true))]))
                .unwrap()
                .unwrap()
                .is_creating
        );
        // Python truthiness: 1/"x" truthy, ""/missing falsy.
        assert!(
            parse_description_task_call(
                &base,
                &kwargs(vec![("is_creating", serde_json::json!(1))])
            )
            .unwrap()
            .unwrap()
            .is_creating
        );
        assert!(
            !parse_description_task_call(&base, &kwargs(vec![("is_creating", str_val(""))]))
                .unwrap()
                .unwrap()
                .is_creating
        );
        assert!(parse_description_task_call(&args(vec![]), &Value::Null).is_err());
        // Live producers send kwargs-only (`intake/base.py:287`, …):
        // kwargs bind with positional fallback, flag included.
        let call = parse_description_task_call(
            &args(vec![]),
            &kwargs(vec![
                ("updated_issue", Value::Null),
                ("issue_id", str_val("i")),
                ("user_id", str_val("u")),
                ("is_creating", Value::Bool(true)),
            ]),
        )
        .unwrap()
        .unwrap();
        assert_eq!(call.issue_id, "i");
        assert_eq!(call.updated_issue, None);
        assert!(call.is_creating);
    }

    #[test]
    fn track_page_parsing() {
        let call = parse_track_page_call(
            &args(vec![str_val("p"), Value::Null, str_val("u")]),
            &Value::Null,
        )
        .unwrap()
        .unwrap();
        assert_eq!(call.page_id, "p");
        assert_eq!(call.existing_instance, None);
        assert_eq!(call.user_id, "u");
        assert!(parse_track_page_call(&args(vec![str_val("p")]), &Value::Null).is_err());
        // Live producers send kwargs-only (`page/base.py:568`,
        // `api/views/page.py:164`), with `existing_instance=None` common.
        let call = parse_track_page_call(
            &args(vec![]),
            &kwargs(vec![
                ("page_id", str_val("p")),
                ("existing_instance", Value::Null),
                ("user_id", str_val("u")),
            ]),
        )
        .unwrap()
        .unwrap();
        assert_eq!(call.page_id, "p");
        assert_eq!(call.existing_instance, None);
        assert_eq!(call.user_id, "u");
    }

    #[test]
    fn message_constructors_match_delay_and_chain() {
        let delay = sync_delay_job(TASK_SYNC_ISSUE_VERSION, 5000, 300);
        assert_eq!(delay.task, TASK_SYNC_ISSUE_VERSION);
        assert_eq!(delay.args, Value::Array(Vec::new()));
        assert_eq!(delay.kwargs["batch_size"], 5000);
        assert_eq!(delay.kwargs["countdown"], 300);
        assert!(delay.kwargs.get("offset").is_none());
        assert_eq!(delay.visible_at, None);
        assert_eq!(delay.max_retries, crate::queue::DEFAULT_MAX_RETRIES);
        // Chain: same batch/countdown, offset = end, visible after countdown.
        let now = Utc.with_ymd_and_hms(2026, 9, 28, 6, 0, 0).unwrap();
        let spec = v::ChainSpec {
            batch_size: 5000,
            offset: 5000,
            countdown_secs: 300,
        };
        let chained = sync_chain_job(TASK_SYNC_ISSUE_VERSION, spec, chain_visible_at(now, 300));
        assert_eq!(chained.kwargs["offset"], 5000);
        assert_eq!(chained.kwargs["batch_size"], 5000);
        assert_eq!(chained.kwargs["countdown"], 300);
        assert_eq!(
            chained.visible_at,
            Some(now + chrono::Duration::seconds(300))
        );
    }

    #[test]
    fn python_equality_rules_for_the_diff() {
        let id = Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap();
        // UUID/date/bytes live values never equal string payloads: those
        // keys always read as changed (the `getattr` asymmetry).
        assert!(!py_eq(
            &LiveValue::Uuid(id),
            &str_val("11111111-1111-1111-1111-111111111111")
        ));
        assert!(!py_eq(&LiveValue::Bytes(vec![1]), &str_val("AQ==")));
        // Strings compare by value.
        assert!(py_eq(&LiveValue::Str("high".to_string()), &str_val("high")));
        assert!(!py_eq(&LiveValue::Str("high".to_string()), &str_val("low")));
        // Ints/floats/bools cross-compare numerically (`True == 1`).
        assert!(py_eq(&LiveValue::Int(42), &serde_json::json!(42)));
        assert!(!py_eq(&LiveValue::Int(42), &serde_json::json!(43)));
        assert!(py_eq(&LiveValue::Bool(true), &serde_json::json!(1)));
        assert!(py_eq(&LiveValue::Bool(false), &serde_json::json!(0)));
        assert!(py_eq(&LiveValue::Float(100.0), &serde_json::json!(100.0)));
        assert!(!py_eq(&LiveValue::Float(100.0), &str_val("100.0")));
        // JSON columns deep-compare.
        assert!(py_eq(
            &LiveValue::Json(serde_json::json!({"a": [1]})),
            &serde_json::json!({"a": [1]})
        ));
        assert!(!py_eq(
            &LiveValue::Json(serde_json::json!({"a": [1]})),
            &serde_json::json!({"a": [2]})
        ));
        // Null rules.
        assert!(py_eq(&LiveValue::Null, &Value::Null));
        assert!(!py_eq(&LiveValue::Null, &str_val("x")));
        assert!(!py_eq(&LiveValue::Str("x".to_string()), &Value::Null));
    }

    #[test]
    fn m2m_keys_always_read_as_changed() {
        // `getattr(issue, "assignees")` is a manager, never equal to a list.
        let known: HashSet<&str> = ["assignees"].into_iter().collect();
        let payload: Map<String, Value> = serde_json::from_str(r#"{"assignees": []}"#).unwrap();
        let live: HashMap<String, LiveValue> =
            [("assignees".to_string(), LiveValue::Bytes(Vec::new()))]
                .into_iter()
                .collect();
        assert_eq!(
            diff_issue_row(&payload, &live, &known).unwrap(),
            vec!["assignees".to_string()]
        );
    }

    #[test]
    fn typed_diff_flags_changed_rejects_unknown() {
        let known: HashSet<&str> = ["name", "priority"].into_iter().collect();
        let payload: Map<String, Value> =
            serde_json::from_str(r#"{"name": "Bug", "priority": "high"}"#).unwrap();
        let live: HashMap<String, LiveValue> = [
            ("name".to_string(), LiveValue::Str("Bug".to_string())),
            ("priority".to_string(), LiveValue::Str("low".to_string())),
        ]
        .into_iter()
        .collect();
        assert_eq!(
            diff_issue_row(&payload, &live, &known).unwrap(),
            vec!["priority".to_string()]
        );
        let bad: Map<String, Value> = serde_json::from_str(r#"{"nope": 1}"#).unwrap();
        assert_eq!(diff_issue_row(&bad, &live, &known), Err("nope".to_string()));
    }

    #[test]
    fn version_column_kinds_cover_every_insert_column() {
        for column in q::ISSUE_VERSION_COLUMNS {
            assert!(version_column_kind(column).is_some(), "{column}");
        }
        assert_eq!(version_column_kind("nope"), None);
    }

    #[test]
    fn html_skip_rule_matches_both_tasks() {
        let payload: Map<String, Value> =
            serde_json::from_str(r#"{"description_html": "<p>x</p>"}"#).unwrap();
        assert!(html_unchanged(&payload, Some("<p>x</p>")));
        assert!(!html_unchanged(&payload, Some("<p>y</p>")));
        // Missing key reads as Null: skips only against a NULL column.
        let empty = Map::new();
        assert!(html_unchanged(&empty, None));
        assert!(!html_unchanged(&empty, Some("<p></p>")));
    }

    #[test]
    fn task_datetime_parsing_matches_to_python() {
        assert!(parse_task_datetime("2026-09-28T06:00:00+00:00").is_some());
        assert!(parse_task_datetime("2026-09-28 06:00:00").is_some());
        assert!(parse_task_datetime("2026-09-28").is_some());
        assert_eq!(parse_task_datetime("not-a-date"), None);
    }
}
