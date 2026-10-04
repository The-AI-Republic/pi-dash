//! Web runners/machines/pods endpoints (D-13 handlers-B, PIDASHCONV-591).
//!
//! Ports `runner/views/runners.py:62-125,289-425` (`DevMachineListEndpoint`,
//! `RunnerListEndpoint`, `RunnerDetailEndpoint` get+patch) and
//! `runner/views/pods.py:45-267` (`PodListEndpoint` get+post,
//! `PodDetailEndpoint` get+patch+delete) — the five session-auth units,
//! mounted under `/api/runners/` (`runner/web_urls.py`).
//!
//! # Execution model
//!
//! * Session preamble via [`crate::license::resolve_actor`] (401
//!   `UNAUTHENTICATED_BODY` when anonymous); every other gate answers
//!   the view-inline `{"error": …}` bodies below.
//! * SQL text comes from the merged queries builders
//!   ([`manage_reads`](pidash_services::runner_enroll::queries::manage_reads)
//!   M-series/R-series, [`catalog_reads`](pidash_services::runner_enroll::queries::catalog_reads)
//!   P-series); this module binds the documented `$N` params positionally
//!   and owns the `BEGIN`/`COMMIT` boundaries (the pod-move patch tx, the
//!   promote-demote tx, the delete tx) plus the guard→error mapping.
//! * Row decode is positional over the builders' pinned column orders
//!   (the `chat.rs` `fetch_runner_detail` precedent — 21+ columns exceed
//!   sqlx's 16-tuple `FromRow`); rendering goes through the merged
//!   [`shapes`](pidash_services::runner_enroll::serializers::shapes) kernels,
//!   so key order and omission rules are never re-derived here.
//! * Request bodies parse through the shared CPython-envelope layer
//!   ([`crate::v1_cycles_modules::json_cpython`]); unparseable JSON is
//!   DRF's `ParseError` (`{"detail": "JSON parse error - …"}`), past the
//!   depth cap is Django's JSON 500. Non-JSON content types proxy to
//!   Django (the `app_scheduler` `read_request_data` precedent — form
//!   posts stay on the Python plane).
//! * `now()` values are truncated to microseconds before bind+render:
//!   Django datetimes are microsecond-exact, while `chrono::Utc::now()`
//!   carries nanos that Postgres would *round* on store.
//!
//! # Ported bugs and quirks (translate, don't redesign; also in the PR)
//!
//! * QUIRK-runner-count (`serializers.py:71-72` vs `pods.py:226-228`):
//!   `runner_count` counts revoked runners, the delete guard excludes
//!   them — the `:227` comment claims they match, but they do not.
//! * QUIRK-create-conflict-500 (`pods.py:118-127`): the unique violation
//!   on `pod_unique_name_per_project_when_active` is NOT caught — a
//!   double create 500s (no handler wraps the insert).
//! * QUIRK-demote-leaves-no-default (`pods.py:198-200`): demoting the
//!   default pod is allowed and leaves the project with NO default.
//! * QUIRK-patch-bool (`pods.py:188`): `wants_default =
//!   bool(request.data.get("is_default"))` — Python truthiness, so
//!   `bool("false")` is `True` ([`j_truthy`]).
//! * QUIRK-patch-non-dict (`runners.py:372,381,384`, `pods.py:165,184,187`):
//!   PATCH key tests are Python `in` — on a JSON array body they are
//!   element-equality tests, on a JSON string body substring tests
//!   ([`data_has`]); numbers/bools/null `TypeError` to 500. A body of
//!   `"xyz"` (or `[]`) answers 200 with no writes.
//! * QUIRK-post-non-dict (`pods.py:91-93`): POST touches `.get`, so any
//!   non-object body `AttributeError`s to 500 ([`data_get`]).
//! * QUIRK-pod-null (`runners.py:386`): `"pod": null` reads as missing
//!   (`filter(pk=None)` matches nothing) → 400 `pod does not exist …`,
//!   while any other non-string pod value 500s.
//! * QUIRK-name-500 (`runners.py:373`, `pods.py:92,166`): a truthy
//!   non-string `name` (e.g. `123`) has no `.strip()` → 500.
//! * QUIRK-description-str (`pods.py:93,185`): `description` is `(value or
//!   "")` with NO strip and NO type check — `123` stores `"123"`,
//!   `true` stores `"True"`, containers store their Python `str()`
//!   ([`j_description`]).
//! * QUIRK-denorm-read (`models.py:145-159`): `Pod.save()` runs the
//!   workspace-denorm check on EVERY save, firing one project read
//!   unless `pod.project` is cached — the rename path caches it, the
//!   description/demote-only patch and the delete stamp do not.
//!
//! # Documented approximations
//!
//! * Unhandled failures answer the JSON 500 (`SERVER_ERROR_BODY`): Django
//!   renders its HTML error page here, so only the status is
//!   contract-pinned (the `runner_runs` precedent, documented there).
//! * Non-UUID path segments answer the view's JSON 404 (`{"error":"not
//!   found"}`): Django's `<uuid:…>` converter 404s at URL-resolve with
//!   its HTML page — status-exact, body per the `chat.rs` precedent.
//! * Lone-surrogate strings in *stored* positions (runner rename,
//!   description) answer 500: Django 500s encoding them for Postgres.
//!   In compared/validated positions (UUIDs, pod names, query filters)
//!   they flow through the lossy spelling, which decides identically
//!   (a surrogate is one non-whitespace char that fails every charset).
//! * The member+admin gates collapse Django's two membership reads
//!   (`EXISTS` + role `SELECT`) into the one role `SELECT` (the
//!   `chat.rs` `workspace_role` precedent) — same outcomes, one query.
//! * `updated_at`/`created_at`/`deleted_at` binds are this request's
//!   microsecond `now()`s, like Django's view-`now()` + `auto_now` calls
//!   (two separate calls where the source makes two).

// Every handler returns a fully-rendered `Response` by design (the
// intake `parse_body` precedent, which carries the same allow).
#![allow(clippy::result_large_err)]

use axum::extract::{Extension, Path, Query, Request, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Router;
use chrono::{DateTime, Utc};
use http_body_util::BodyExt as _;
use serde_json::Value;
use sqlx::{PgPool, Row};
use uuid::Uuid;

use pidash_auth::permissions::membership;
use pidash_auth::permissions::runner as runner_perm;
use pidash_auth::scope::TenantScope;
use pidash_db::runner_enroll::columns::{dev_machine as dm_cols, runner as r_cols};
use pidash_services::runner_enroll::pod_naming;
use pidash_services::runner_enroll::queries::{catalog_reads, manage_reads};
use pidash_services::runner_enroll::serializers::shapes;
use pidash_types::{UserId, WorkspaceId};

use crate::middleware::SessionHandle;
use crate::runner_runs::{json_response, pool_of, server_error};
use crate::state::AppState;
use crate::v1_cycles_modules::json_cpython::{self, JObject, JVal, JsonFail};

// ---------------------------------------------------------------------------
// Error bodies (D13-F7 `handlers/endpoints.golden.json`, key order verbatim
// from the Python dict literals — `error` before `code`)
// ---------------------------------------------------------------------------

/// `{"error": "workspace is required"}` — 400 (`runners.py:77,306`,
/// `machine_commands.py` siblings use the same text).
pub const WORKSPACE_REQUIRED_BODY: &str = r#"{"error":"workspace is required"}"#;
/// `{"error": "forbidden"}` — 403 (non-member / non-manager).
pub const FORBIDDEN_BODY: &str = r#"{"error":"forbidden"}"#;
/// `{"error": "not found"}` — 404 (miss, or present-but-unviewable).
pub const NOT_FOUND_BODY: &str = r#"{"error":"not found"}"#;
/// `{"error": "project or workspace is required"}` — 400 (`pods.py:76`).
pub const PROJECT_OR_WORKSPACE_REQUIRED_BODY: &str =
    r#"{"error":"project or workspace is required"}"#;
/// `{"error": "project not found"}` — 404 (`pods.py:65,102`).
pub const PROJECT_NOT_FOUND_BODY: &str = r#"{"error":"project not found"}"#;
/// `{"error": "project and name are required"}` — 400 (`pods.py:96`).
pub const PROJECT_AND_NAME_REQUIRED_BODY: &str = r#"{"error":"project and name are required"}"#;
/// `{"error": "workspace admin required"}` — 403 (`pods.py:107`).
pub const WORKSPACE_ADMIN_REQUIRED_BODY: &str = r#"{"error":"workspace admin required"}"#;
/// `{"error": "name cannot be empty"}` — 400 (`runners.py:376`, `pods.py:169`).
pub const NAME_CANNOT_BE_EMPTY_BODY: &str = r#"{"error":"name cannot be empty"}"#;
/// `{"error": "pod does not exist or has been deleted"}` — 400
/// (`runners.py:389`).
pub const POD_MISSING_BODY: &str = r#"{"error":"pod does not exist or has been deleted"}"#;
/// `{"error": "pod is in a different workspace"}` — 400 (`runners.py:394`).
pub const POD_OTHER_WORKSPACE_BODY: &str = r#"{"error":"pod is in a different workspace"}"#;
/// Runner busy guard — 409 (`runners.py:414-417`).
pub const RUNNER_BUSY_BODY: &str = r#"{"error":"runner has an in-flight or queued run; wait for it to finish or cancel it first","code":"runner_busy"}"#;
/// Pod delete guard 1 — 409 (`pods.py:230-234`).
pub const POD_HAS_RUNNERS_BODY: &str =
    r#"{"error":"pod has runners; move or revoke them first","code":"pod_has_runners"}"#;
/// Pod delete guard 2 — 409 (`pods.py:238-242`).
pub const POD_HAS_ACTIVE_RUNS_BODY: &str =
    r#"{"error":"pod has non-terminal runs; cancel or wait","code":"pod_has_active_runs"}"#;
/// Pod delete guard 3 — 409 (`pods.py:250-256`).
pub const DEFAULT_POD_UNDELETABLE_BODY: &str = r#"{"error":"cannot delete the project's default pod; promote another pod to default first","code":"default_pod_undeletable"}"#;

fn bad_request(body: &str) -> Response {
    json_response(StatusCode::BAD_REQUEST, body.to_owned())
}

fn forbidden() -> Response {
    json_response(StatusCode::FORBIDDEN, FORBIDDEN_BODY.to_owned())
}

fn not_found() -> Response {
    json_response(StatusCode::NOT_FOUND, NOT_FOUND_BODY.to_owned())
}

fn conflict(body: &str) -> Response {
    json_response(StatusCode::CONFLICT, body.to_owned())
}

// ---------------------------------------------------------------------------
// Preamble (the `chat.rs` `web_actor` + `workspace_role` precedent)
// ---------------------------------------------------------------------------

/// Session → `(pool, user_id)`: anonymous answers the DRF
/// `NotAuthenticated` 401, pool/db failures the JSON 500.
async fn web_actor(
    state: &AppState,
    extension: Option<Extension<SessionHandle>>,
) -> Result<(PgPool, Uuid), Response> {
    let pool = pool_of(state)?.clone();
    let secret = state.settings().secret_key.clone();
    let actor = crate::license::resolve_actor(&pool, secret.as_bytes(), extension)
        .await
        .map_err(|_| server_error())?;
    let Some(actor) = actor else {
        return Err(json_response(
            StatusCode::UNAUTHORIZED,
            crate::license::UNAUTHENTICATED_BODY.to_owned(),
        ));
    };
    Ok((pool, actor.id))
}

/// `workspace_role` (`core/permissions.py:37-45`): the caller's active
/// role, or `None`. `WorkspaceMember.objects` is the
/// `SoftDeletionManager`, so tombstones do not count; `Meta.ordering`
/// is `-created_at` with `.first()` → `LIMIT 1`.
async fn workspace_role(
    executor: &PgPool,
    workspace_id: Uuid,
    user_id: Uuid,
) -> Result<Option<i32>, Response> {
    workspace_role_in(executor, workspace_id, user_id).await
}

async fn workspace_role_in(
    executor: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    workspace_id: Uuid,
    user_id: Uuid,
) -> Result<Option<i32>, Response> {
    let role: Option<i16> = sqlx::query_scalar(
        r#"SELECT "role" FROM "workspace_members"
           WHERE ("workspace_id" = $1 AND "member_id" = $2
             AND "is_active" AND "deleted_at" IS NULL)
           ORDER BY "created_at" DESC LIMIT 1"#,
    )
    .bind(workspace_id)
    .bind(user_id)
    .fetch_optional(executor)
    .await
    .map_err(|_| server_error())?;
    Ok(role.map(i32::from))
}

/// Django `request.query_params.get`: the last value wins on repeats
/// (`QueryDict`), and a missing or empty param disables the filter
/// (every source gate is an `if …:` truthiness check).
fn query_param(params: &crate::license::QueryMap, key: &str) -> Option<String> {
    crate::license::query_last(params, key).filter(|raw| !raw.is_empty())
}

/// UUID-typed query/body strings: Django's `UUIDField.get_prep_value`
/// raises `ValidationError` (an unhandled 500) on garbage — the
/// `chat_sessions_list` precedent answers [`server_error`].
fn parse_uuid(raw: &str) -> Result<Uuid, Response> {
    raw.parse().map_err(|_| server_error())
}

/// `timezone.now()` truncated to microseconds (Django datetimes are
/// microsecond-exact; Postgres would round stored nanos).
fn now_micros() -> DateTime<Utc> {
    let now = Utc::now();
    DateTime::from_timestamp_micros(now.timestamp_micros()).expect("micros in range")
}

// ---------------------------------------------------------------------------
// Request bodies (the `app_scheduler` envelope over `json_cpython`)
// ---------------------------------------------------------------------------

/// Read `request.data` for a POST/PATCH body: content-length 0
/// validates as `{}` with the body ignored; a non-JSON content type
/// proxies to Django (form posts stay on the Python plane);
/// unparsable JSON 400s with DRF's `ParseError` Detail; past the
/// depth cap is Django's JSON 500.
async fn read_request_data(state: &AppState, req: Request) -> Result<JVal, Response> {
    let content_length = req
        .headers()
        .get(header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|raw| raw.trim().parse::<usize>().ok())
        .unwrap_or(0);
    if content_length == 0 {
        return Ok(JVal::Object(JObject::new()));
    }
    let raw_type = req
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    // `parse_header_parameters`: the main type lowercases; parameters
    // are ignored for parser selection (`_MediaType.match`).
    let main = raw_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_lowercase();
    if main != "application/json" {
        return Err(crate::edge::proxy(State(state.clone()), req).await);
    }
    let (_parts, body) = req.into_parts();
    let bytes = body
        .collect()
        .await
        .map(|collected| collected.to_bytes())
        .map_err(|_| server_error())?;
    match json_cpython::parse_request_data(&bytes) {
        Ok(value) => Ok(value),
        Err(JsonFail::Message(detail)) => Err(json_response(
            StatusCode::BAD_REQUEST,
            parse_error_body(&detail),
        )),
        Err(JsonFail::Recursion) => Err(server_error()),
    }
}

/// DRF `ParseError` body (`rest_framework/parsers.py`): lowercase
/// `detail`, the `JSON parse error - ` prefix, CPython's message.
fn parse_error_body(detail: &str) -> String {
    format!(
        "{{\"Detail\":{}}}",
        serde_json::to_string(&format!("{}{detail}", json_cpython::JSON_PARSE_PREFIX))
            .expect("json string")
    )
}

/// Python truthiness over a parsed value (`None`/`False`/`0`/`""`/`[]`/`{}`
/// are falsy; everything else is truthy) — `QUIRK-patch-bool`: note
/// `bool("false")` is `True`, and `-0.0`/underflows are falsy.
fn j_truthy(value: &JVal) -> bool {
    match value {
        JVal::Null => false,
        JVal::Bool(flag) => *flag,
        JVal::Num(number) => !number.is_zero(),
        JVal::Str(text) => !text.is_empty(),
        JVal::Array(items) => !items.is_empty(),
        JVal::Object(map) => !map.is_empty(),
    }
}

/// Python `key in request.data` for the PATCH branches
/// (`QUIRK-patch-non-dict`): dict membership; on an array,
/// element-equality against the clean key; on a string, substring
/// search; anything else is Python's `TypeError` → 500.
fn data_has(data: &JVal, key: &str) -> Result<bool, Response> {
    match data {
        JVal::Object(map) => Ok(map.contains_key(key)),
        JVal::Array(items) => Ok(items
            .iter()
            .any(|item| matches!(item, JVal::Str(text) if text.eq_str(key)))),
        JVal::Str(text) => Ok(text.contains_str(key)),
        JVal::Null | JVal::Bool(_) | JVal::Num(_) => Err(server_error()),
    }
}

/// `request.data.get(key)` (`QUIRK-post-non-dict`): dict lookup — `None`
/// for a missing key; any non-object is Python's `AttributeError` → 500.
fn data_get<'a>(data: &'a JVal, key: &str) -> Result<Option<&'a JVal>, Response> {
    match data {
        JVal::Object(map) => Ok(map.get(key)),
        JVal::Null | JVal::Bool(_) | JVal::Num(_) | JVal::Str(_) | JVal::Array(_) => {
            Err(server_error())
        }
    }
}

/// Python `str.strip()` parity (the services-layer `py_strip` twin —
/// services is read-only from this crate's layer boundary, so the
/// predicate is mirrored, not imported): Python strips
/// `str.isspace()` — Unicode `White_Space` plus U+001C-U+001F and
/// U+0085 — while Rust `trim()` strips `White_Space` only.
fn py_strip(text: &str) -> &str {
    text.trim_matches(|c: char| {
        c.is_whitespace() || c == '\u{85}' || ('\u{1c}'..='\u{1f}').contains(&c)
    })
}

/// `(request.data.get(key) or "").strip()` (`QUIRK-name-500`): falsy
/// maps to `""`, strings strip, truthy non-strings are the source's
/// `AttributeError` → 500. Surrogate-carrying strings flow through the
/// lossy spelling (documented approximation — a surrogate is one
/// non-whitespace char that fails every downstream check identically).
fn or_empty_stripped(value: Option<&JVal>) -> Result<String, Response> {
    let Some(value) = value else {
        return Ok(String::new());
    };
    if !j_truthy(value) {
        return Ok(String::new());
    }
    match value {
        JVal::Str(text) => Ok(py_strip(&text.to_lossy_string()).to_owned()),
        JVal::Null | JVal::Bool(_) | JVal::Num(_) | JVal::Array(_) | JVal::Object(_) => {
            Err(server_error())
        }
    }
}

/// `(request.data.get(key) or "")` for `description`
/// (`QUIRK-description-str`): falsy maps to `""`, strings pass through
/// UNstripped, truthy non-strings render via Python `str()`
/// (`123` → `"123"`, `true` → `"True"`, containers via `py_str`).
/// Dirty (surrogate-carrying) results 500 — Django 500s encoding them
/// for Postgres — as does a `str()` recursion failure.
fn j_description(value: Option<&JVal>) -> Result<String, Response> {
    let Some(value) = value else {
        return Ok(String::new());
    };
    if !j_truthy(value) {
        return Ok(String::new());
    }
    let rendered = json_cpython::py_str(value).map_err(|_| server_error())?;
    rendered.to_clean_string().ok_or_else(server_error)
}

/// A UUID-typed body member that already passed its truthiness gate
/// (project/pod ids): strings parse (`ValidationError` → 500 on
/// garbage), anything else is the filter's `TypeError`/`ValueError` →
/// 500. (`Null` is only passable where the source reaches the filter
/// with it — the runner PATCH pod — handled by [`pod_member_id`].)
fn body_uuid(value: &JVal) -> Result<Uuid, Response> {
    match value {
        JVal::Str(text) => match text.to_clean_string() {
            Some(clean) => parse_uuid(&clean),
            None => Err(server_error()),
        },
        JVal::Null | JVal::Bool(_) | JVal::Num(_) | JVal::Array(_) | JVal::Object(_) => {
            Err(server_error())
        }
    }
}

/// The runner-PATCH `pod` member (`QUIRK-pod-null`): explicit `null`
/// reads as missing (`filter(pk=None)` matches nothing → the 400 arm),
/// strings parse, everything else 500s.
fn pod_member_id(value: &JVal) -> Result<Option<Uuid>, Response> {
    match value {
        JVal::Null => Ok(None),
        other => body_uuid(other).map(Some),
    }
}

// ---------------------------------------------------------------------------
// Rows (positional decode over the builders' pinned column orders)
// ---------------------------------------------------------------------------

/// Owned `pod` row: `COLUMNS` order (`id`, `workspace_id`, `project_id`,
/// `name`, `description`, `created_by_id`, `is_default`, `deleted_at`,
/// `created_at`, `updated_at`).
struct PodOwned {
    id: Uuid,
    workspace_id: Uuid,
    project_id: Uuid,
    name: String,
    description: String,
    created_by: Option<Uuid>,
    is_default: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

/// Decode a `pod` row at `base` (the P-series full-row reads and the pod
/// block of the R-series joins — same column order everywhere).
fn decode_pod(row: &sqlx::postgres::PgRow, base: usize) -> Result<PodOwned, Response> {
    Ok(PodOwned {
        id: row.try_get(base).map_err(|_| server_error())?,
        workspace_id: row.try_get(base + 1).map_err(|_| server_error())?,
        project_id: row.try_get(base + 2).map_err(|_| server_error())?,
        name: row.try_get(base + 3).map_err(|_| server_error())?,
        description: row.try_get(base + 4).map_err(|_| server_error())?,
        created_by: row.try_get(base + 5).map_err(|_| server_error())?,
        is_default: row.try_get(base + 6).map_err(|_| server_error())?,
        created_at: row.try_get(base + 8).map_err(|_| server_error())?,
        updated_at: row.try_get(base + 9).map_err(|_| server_error())?,
    })
}

/// Owned `dev_machine` row + the M3/M5 annotations (`COLUMNS` order plus
/// `runner_count`, `online_runner_count`, `last_heartbeat_at`,
/// `control_online`).
struct DevMachineOwned {
    id: Uuid,
    host_label: String,
    label: String,
    visibility: i16,
    last_seen_at: Option<DateTime<Utc>>,
    revoked_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    runner_count: i64,
    online_runner_count: i64,
    last_heartbeat_at: Option<DateTime<Utc>>,
    control_online: bool,
}

/// Decode an M3 list row: the 10 machine columns then the 4 annotations.
fn decode_machine_list_row(row: &sqlx::postgres::PgRow) -> Result<DevMachineOwned, Response> {
    Ok(DevMachineOwned {
        id: row.try_get(0).map_err(|_| server_error())?,
        host_label: row.try_get(2).map_err(|_| server_error())?,
        label: row.try_get(3).map_err(|_| server_error())?,
        visibility: row.try_get(4).map_err(|_| server_error())?,
        last_seen_at: row.try_get(6).map_err(|_| server_error())?,
        revoked_at: row.try_get(7).map_err(|_| server_error())?,
        created_at: row.try_get(8).map_err(|_| server_error())?,
        updated_at: row.try_get(9).map_err(|_| server_error())?,
        runner_count: row.try_get(10).map_err(|_| server_error())?,
        online_runner_count: row.try_get(11).map_err(|_| server_error())?,
        last_heartbeat_at: row.try_get(12).map_err(|_| server_error())?,
        control_online: row.try_get(13).map_err(|_| server_error())?,
    })
}

/// Owned `runner` row: the 29 `COLUMNS` (token internals and
/// `free_worktrees` skipped — never serialized here).
struct RunnerOwned {
    id: Uuid,
    owner_id: Uuid,
    workspace_id: Uuid,
    dev_machine_id: Option<Uuid>,
    pod_id: Uuid,
    name: String,
    host_label: String,
    provisioning: String,
    visibility: i16,
    enrolled_at: Option<DateTime<Utc>>,
    capabilities: Value,
    status: String,
    os: String,
    arch: String,
    runner_version: String,
    dev_metadata: Value,
    protocol_version: i32,
    last_heartbeat_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    revoked_at: Option<DateTime<Utc>>,
    revoked_reason: String,
}

/// Decode the runner block at `base` (R1/R2 select it first, so `base`
/// is 0; kept explicit for readability).
fn decode_runner(row: &sqlx::postgres::PgRow, base: usize) -> Result<RunnerOwned, Response> {
    Ok(RunnerOwned {
        id: row.try_get(base).map_err(|_| server_error())?,
        owner_id: row.try_get(base + 1).map_err(|_| server_error())?,
        workspace_id: row.try_get(base + 2).map_err(|_| server_error())?,
        dev_machine_id: row.try_get(base + 3).map_err(|_| server_error())?,
        pod_id: row.try_get(base + 4).map_err(|_| server_error())?,
        name: row.try_get(base + 5).map_err(|_| server_error())?,
        host_label: row.try_get(base + 6).map_err(|_| server_error())?,
        provisioning: row.try_get(base + 7).map_err(|_| server_error())?,
        visibility: row.try_get(base + 8).map_err(|_| server_error())?,
        enrolled_at: row.try_get(base + 16).map_err(|_| server_error())?,
        capabilities: row.try_get(base + 17).map_err(|_| server_error())?,
        status: row.try_get(base + 18).map_err(|_| server_error())?,
        os: row.try_get(base + 19).map_err(|_| server_error())?,
        arch: row.try_get(base + 20).map_err(|_| server_error())?,
        runner_version: row.try_get(base + 21).map_err(|_| server_error())?,
        dev_metadata: row.try_get(base + 22).map_err(|_| server_error())?,
        protocol_version: row.try_get(base + 23).map_err(|_| server_error())?,
        last_heartbeat_at: row.try_get(base + 24).map_err(|_| server_error())?,
        created_at: row.try_get(base + 26).map_err(|_| server_error())?,
        updated_at: row.try_get(base + 27).map_err(|_| server_error())?,
        revoked_at: row.try_get(base + 28).map_err(|_| server_error())?,
        revoked_reason: row.try_get(base + 29).map_err(|_| server_error())?,
    })
}

/// Owned `dev_machine` mini (`serializers.py:145-153`).
struct DevMachineMiniOwned {
    id: String,
    host_label: String,
    label: String,
}

/// Decode the dev-machine block of an R1/R2 join at `base`, or `None`
/// when the `LEFT OUTER JOIN` missed (legacy `pidash connect`
/// enrollments with no `dev_machine` FK).
fn decode_machine_mini(
    row: &sqlx::postgres::PgRow,
    base: usize,
) -> Result<Option<DevMachineMiniOwned>, Response> {
    let id: Option<Uuid> = row.try_get(base).map_err(|_| server_error())?;
    let Some(id) = id else {
        return Ok(None);
    };
    let host_label: String = row.try_get(base + 2).map_err(|_| server_error())?;
    let label: String = row.try_get(base + 3).map_err(|_| server_error())?;
    Ok(Some(DevMachineMiniOwned {
        id: id.to_string(),
        host_label,
        label,
    }))
}

/// Owned live-state snapshot (`RunnerLiveState`, `models.py:1454-1500`).
struct LiveStateOwned {
    observed_run_id: Option<String>,
    last_event_at: Option<String>,
    last_event_kind: Option<String>,
    last_event_summary: Option<String>,
    agent_pid: Option<i64>,
    agent_subprocess_alive: Option<bool>,
    approvals_pending: Option<i64>,
    usage: Value,
    llm_model: Option<String>,
    turn_count: Option<i64>,
    updated_at: String,
}

/// The reverse one-to-one `runner.live_state` read: `RunnerLiveState`
/// columns in `_meta` order over `runner_id`, `LIMIT 1` (the
/// queries-A QUIRK-get-no-limit precedent omits `.get()`'s `LIMIT 21`
/// probe — behaviorally identical for a PK). `None` when the row is
/// missing (pre-flag runners render `live_state: null`).
async fn fetch_live_state(
    pool: &PgPool,
    runner_id: Uuid,
) -> Result<Option<LiveStateOwned>, Response> {
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(
        r#"SELECT "runner_id", "observed_run_id", "last_event_at", "last_event_kind",
              "last_event_summary", "agent_pid", "agent_subprocess_alive",
              "approvals_pending", "usage", "llm_model", "turn_count", "updated_at"
           FROM "runner_live_state" WHERE "runner_live_state"."runner_id" = $1 LIMIT 1"#,
    )
    .bind(runner_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| server_error())?;
    let Some(row) = row else {
        return Ok(None);
    };
    let observed: Option<Uuid> = row.try_get(1).map_err(|_| server_error())?;
    let last_event_at: Option<DateTime<Utc>> = row.try_get(2).map_err(|_| server_error())?;
    let updated_at: DateTime<Utc> = row.try_get(11).map_err(|_| server_error())?;
    let agent_pid: Option<i32> = row.try_get(5).map_err(|_| server_error())?;
    let approvals_pending: Option<i32> = row.try_get(7).map_err(|_| server_error())?;
    let turn_count: Option<i32> = row.try_get(10).map_err(|_| server_error())?;
    Ok(Some(LiveStateOwned {
        observed_run_id: observed.map(|id| id.to_string()),
        last_event_at: last_event_at
            .as_ref()
            .map(crate::serializer::render_datetime),
        last_event_kind: row.try_get(3).map_err(|_| server_error())?,
        last_event_summary: row.try_get(4).map_err(|_| server_error())?,
        agent_pid: agent_pid.map(i64::from),
        agent_subprocess_alive: row.try_get(6).map_err(|_| server_error())?,
        approvals_pending: approvals_pending.map(i64::from),
        usage: row.try_get(8).map_err(|_| server_error())?,
        llm_model: row.try_get(9).map_err(|_| server_error())?,
        turn_count: turn_count.map(i64::from),
        updated_at: crate::serializer::render_datetime(&updated_at),
    }))
}

/// The two `projects` facts the pod/runner serializers need:
/// `workspace_id` (gates) and `identifier` (the mini `slug`).
struct ProjectFacts {
    workspace_id: Uuid,
    identifier: String,
}

/// [`catalog_reads::project_by_id_sql`] decoded to the two facts (by
/// name — the single-table row has no duplicate columns).
async fn fetch_project_facts(
    pool: &PgPool,
    project_id: Uuid,
) -> Result<Option<ProjectFacts>, Response> {
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(&catalog_reads::project_by_id_sql())
        .bind(project_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| server_error())?;
    let Some(row) = row else {
        return Ok(None);
    };
    Ok(Some(ProjectFacts {
        workspace_id: row.try_get("workspace_id").map_err(|_| server_error())?,
        identifier: row.try_get("identifier").map_err(|_| server_error())?,
    }))
}

/// [`catalog_reads::project_by_id_sql`] inside a transaction: the
/// `Pod.save()` denorm read (`QUIRK-denorm-read`). The value is only
/// ever compared, never rendered — the mismatch arm is unreachable on
/// these paths (neither `workspace_id` nor `project_id` changes), so
/// only the read itself is ported.
async fn denorm_project_read(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    project_id: Uuid,
) -> Result<(), Response> {
    sqlx::query(&catalog_reads::project_by_id_sql())
        .bind(project_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|_| server_error())?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Renders (owned strings borrowed into the shapes kernels)
// ---------------------------------------------------------------------------

/// Owned render buffer for one pod + its count + project slug.
struct PodRender {
    id: String,
    name: String,
    description: String,
    is_default: bool,
    workspace: String,
    project: String,
    project_identifier: String,
    created_by: Option<String>,
    runner_count: i64,
    created_at: String,
    updated_at: String,
}

impl PodRender {
    fn new(pod: &PodOwned, project_identifier: String, runner_count: i64) -> Self {
        Self {
            id: pod.id.to_string(),
            name: pod.name.clone(),
            description: pod.description.clone(),
            is_default: pod.is_default,
            workspace: pod.workspace_id.to_string(),
            project: pod.project_id.to_string(),
            project_identifier,
            created_by: pod.created_by.map(|id| id.to_string()),
            runner_count,
            created_at: crate::serializer::render_datetime(&pod.created_at),
            updated_at: crate::serializer::render_datetime(&pod.updated_at),
        }
    }

    fn to_json(&self) -> Result<String, Response> {
        let row = shapes::PodRow {
            id: &self.id,
            name: &self.name,
            description: &self.description,
            is_default: self.is_default,
            workspace: &self.workspace,
            project: &self.project,
            project_identifier: &self.project_identifier,
            created_by: self.created_by.as_deref(),
            runner_count: self.runner_count,
            created_at: &self.created_at,
            updated_at: &self.updated_at,
        };
        serde_json::to_string(&shapes::pod_to_representation(&row)).map_err(|_| server_error())
    }
}

/// Serialize one pod: the unfiltered runner count (N+1, like the
/// source's `get_runner_count`) plus the `project.identifier` read
/// (N+1 — the source never `select_related`s it on these paths).
async fn render_pod(
    pool: &PgPool,
    pod: &PodOwned,
    project_identifier: Option<String>,
) -> Result<String, Response> {
    let runner_count: i64 = sqlx::query_scalar(&catalog_reads::pod_runner_count_sql())
        .bind(pod.id)
        .fetch_one(pool)
        .await
        .map_err(|_| server_error())?;
    let identifier = match project_identifier {
        Some(identifier) => identifier,
        None => fetch_project_facts(pool, pod.project_id)
            .await?
            .map(|facts| facts.identifier)
            .ok_or_else(server_error)?,
    };
    PodRender::new(pod, identifier, runner_count).to_json()
}

/// Owned render buffer for one dev machine + its M3 annotations.
struct DevMachineRender {
    id: String,
    host_label: String,
    label: String,
    visibility: i64,
    runner_count: i64,
    online_runner_count: i64,
    control_online: bool,
    last_seen_at: Option<String>,
    last_heartbeat_at: Option<String>,
    revoked_at: Option<String>,
    created_at: String,
    updated_at: String,
}

impl DevMachineRender {
    fn to_json(&self) -> Result<String, Response> {
        let row = shapes::DevMachineRow {
            id: &self.id,
            host_label: &self.host_label,
            label: &self.label,
            visibility: self.visibility,
            runner_count: Some(self.runner_count),
            online_runner_count: Some(self.online_runner_count),
            control_online: Some(self.control_online),
            last_seen_at: self.last_seen_at.as_deref(),
            last_heartbeat_at: Some(self.last_heartbeat_at.as_deref()),
            revoked_at: self.revoked_at.as_deref(),
            created_at: &self.created_at,
            updated_at: &self.updated_at,
        };
        serde_json::to_string(&shapes::dev_machine_to_representation(&row))
            .map_err(|_| server_error())
    }
}

fn render_machine(machine: &DevMachineOwned) -> Result<String, Response> {
    DevMachineRender {
        id: machine.id.to_string(),
        host_label: machine.host_label.clone(),
        label: machine.label.clone(),
        visibility: i64::from(machine.visibility),
        runner_count: machine.runner_count,
        online_runner_count: machine.online_runner_count,
        control_online: machine.control_online,
        last_seen_at: machine
            .last_seen_at
            .as_ref()
            .map(crate::serializer::render_datetime),
        last_heartbeat_at: machine
            .last_heartbeat_at
            .as_ref()
            .map(crate::serializer::render_datetime),
        revoked_at: machine
            .revoked_at
            .as_ref()
            .map(crate::serializer::render_datetime),
        created_at: crate::serializer::render_datetime(&machine.created_at),
        updated_at: crate::serializer::render_datetime(&machine.updated_at),
    }
    .to_json()
}

/// Owned render buffer for one runner + its pod/dev-machine minis.
struct RunnerRender {
    id: String,
    name: String,
    status: String,
    host_label: String,
    provisioning: String,
    os: String,
    arch: String,
    runner_version: String,
    dev_metadata: Value,
    protocol_version: i64,
    capabilities: Value,
    last_heartbeat_at: Option<String>,
    owner: String,
    dev_machine: Option<String>,
    dev_machine_detail: Option<DevMachineMiniOwned>,
    visibility: i64,
    pod: String,
    pod_detail: PodMiniRender,
    live_state: Option<LiveStateOwned>,
    enrolled_at: Option<String>,
    revoked_at: Option<String>,
    revoked_reason: String,
    created_at: String,
    updated_at: String,
}

/// Owned pod mini (`serializers.py:133-142`).
struct PodMiniRender {
    id: String,
    name: String,
    is_default: bool,
    project: String,
    project_identifier: String,
}

impl RunnerRender {
    fn to_json(&self) -> Result<String, Response> {
        let pod = shapes::PodMiniRow {
            id: &self.pod_detail.id,
            name: &self.pod_detail.name,
            is_default: self.pod_detail.is_default,
            project: &self.pod_detail.project,
            project_identifier: &self.pod_detail.project_identifier,
        };
        let dev_machine =
            self.dev_machine_detail
                .as_ref()
                .map(|detail| shapes::DevMachineMiniRow {
                    id: &detail.id,
                    host_label: &detail.host_label,
                    label: &detail.label,
                });
        let live_state = self.live_state.as_ref().map(|live| shapes::LiveStateRow {
            observed_run_id: live.observed_run_id.as_deref(),
            last_event_at: live.last_event_at.as_deref(),
            last_event_kind: live.last_event_kind.as_deref(),
            last_event_summary: live.last_event_summary.as_deref(),
            agent_pid: live.agent_pid,
            agent_subprocess_alive: live.agent_subprocess_alive,
            approvals_pending: live.approvals_pending,
            usage: &live.usage,
            llm_model: live.llm_model.as_deref(),
            turn_count: live.turn_count,
            updated_at: &live.updated_at,
        });
        let row = shapes::RunnerRow {
            id: &self.id,
            name: &self.name,
            status: &self.status,
            host_label: &self.host_label,
            provisioning: &self.provisioning,
            os: &self.os,
            arch: &self.arch,
            runner_version: &self.runner_version,
            dev_metadata: &self.dev_metadata,
            protocol_version: self.protocol_version,
            capabilities: &self.capabilities,
            last_heartbeat_at: self.last_heartbeat_at.as_deref(),
            owner: &self.owner,
            dev_machine: self.dev_machine.as_deref(),
            dev_machine_detail: dev_machine,
            visibility: self.visibility,
            pod: &self.pod,
            pod_detail: pod,
            live_state,
            enrolled_at: self.enrolled_at.as_deref(),
            revoked_at: self.revoked_at.as_deref(),
            revoked_reason: &self.revoked_reason,
            created_at: &self.created_at,
            updated_at: &self.updated_at,
        };
        serde_json::to_string(&shapes::runner_to_representation(&row)).map_err(|_| server_error())
    }
}

/// Select-list block widths in the R1/R2 joins (runner, dev_machine,
/// pod, then projects — the select order never moves).
const RUNNER_BLOCK_COLS: usize = r_cols::COLUMNS.len();
const MACHINE_BLOCK_COLS: usize = dm_cols::COLUMNS.len();

/// Build the runner render buffer from decoded parts (pure — the
/// live-state read stays with the caller).
fn runner_render(
    runner: &RunnerOwned,
    pod: &PodOwned,
    pod_identifier: String,
    dev_machine_detail: Option<DevMachineMiniOwned>,
    live_state: Option<LiveStateOwned>,
) -> RunnerRender {
    RunnerRender {
        id: runner.id.to_string(),
        name: runner.name.clone(),
        status: runner.status.clone(),
        host_label: runner.host_label.clone(),
        provisioning: runner.provisioning.clone(),
        os: runner.os.clone(),
        arch: runner.arch.clone(),
        runner_version: runner.runner_version.clone(),
        dev_metadata: runner.dev_metadata.clone(),
        protocol_version: i64::from(runner.protocol_version),
        capabilities: runner.capabilities.clone(),
        last_heartbeat_at: runner
            .last_heartbeat_at
            .as_ref()
            .map(crate::serializer::render_datetime),
        owner: runner.owner_id.to_string(),
        dev_machine: runner.dev_machine_id.map(|id| id.to_string()),
        dev_machine_detail,
        visibility: i64::from(runner.visibility),
        pod: runner.pod_id.to_string(),
        pod_detail: PodMiniRender {
            id: pod.id.to_string(),
            name: pod.name.clone(),
            is_default: pod.is_default,
            project: pod.project_id.to_string(),
            project_identifier: pod_identifier,
        },
        live_state,
        enrolled_at: runner
            .enrolled_at
            .as_ref()
            .map(crate::serializer::render_datetime),
        revoked_at: runner
            .revoked_at
            .as_ref()
            .map(crate::serializer::render_datetime),
        revoked_reason: runner.revoked_reason.clone(),
        created_at: crate::serializer::render_datetime(&runner.created_at),
        updated_at: crate::serializer::render_datetime(&runner.updated_at),
    }
}

/// Serialize one runner from decoded parts: the buffer plus a fresh
/// live-state read (the source never prefetches it — N+1 on serialize).
async fn render_runner_parts(
    pool: &PgPool,
    runner: &RunnerOwned,
    pod: &PodOwned,
    pod_identifier: String,
    dev_machine_detail: Option<DevMachineMiniOwned>,
) -> Result<String, Response> {
    let live_state = fetch_live_state(pool, runner.id).await?;
    runner_render(runner, pod, pod_identifier, dev_machine_detail, live_state).to_json()
}

/// Serialize one R1/R2 join row: the runner + dev-machine + pod blocks
/// decode positionally; `projects.identifier` reads by name (the only
/// `identifier` column across the four tables).
async fn render_runner_row(pool: &PgPool, row: &sqlx::postgres::PgRow) -> Result<String, Response> {
    let runner = decode_runner(row, 0)?;
    let dev_machine_detail = decode_machine_mini(row, RUNNER_BLOCK_COLS)?;
    let pod = decode_pod(row, RUNNER_BLOCK_COLS + MACHINE_BLOCK_COLS)?;
    let identifier: String = row.try_get("identifier").map_err(|_| server_error())?;
    render_runner_parts(pool, &runner, &pod, identifier, dev_machine_detail).await
}

// ---------------------------------------------------------------------------
// Dev-machine list (`runners.py:62-125`)
// ---------------------------------------------------------------------------

/// `GET /api/runners/dev-machines/` (`runners.py:62-125`): workspace
/// required/member gate, the runner+token machine-id union with the
/// three annotations + `control_online`, `-last_seen_at, -created_at`
/// order. Missing `?workspace=` → 400, non-member → 403, garbage UUID →
/// 500 (Django's `ValidationError`).
pub async fn dev_machines_list(
    State(state): State<AppState>,
    extension: Option<Extension<SessionHandle>>,
    Query(params): Query<crate::license::QueryMap>,
) -> Response {
    let (pool, user_id) = match web_actor(&state, extension).await {
        Ok(preamble) => preamble,
        Err(response) => return response,
    };
    let Some(raw) = query_param(&params, "workspace") else {
        return bad_request(WORKSPACE_REQUIRED_BODY);
    };
    let workspace_id = match parse_uuid(&raw) {
        Ok(workspace_id) => workspace_id,
        Err(response) => return response,
    };
    let role = match workspace_role(&pool, workspace_id, user_id).await {
        Ok(role) => role,
        Err(response) => return response,
    };
    if !membership::is_workspace_member(role) {
        return forbidden();
    }
    // The M1 cutoff is computed ONCE per request and crossed as the
    // `$13` bind — never recomputed per row.
    let visibility: i16 = runner_perm::VISIBILITY_PRIVATE as i16;
    let cutoff =
        now_micros() - chrono::Duration::seconds(manage_reads::CONTROL_PRESENCE_WINDOW_SECS);
    let rows: Vec<sqlx::postgres::PgRow> = match sqlx::query(&manage_reads::machine_list_sql())
        .bind(user_id)
        .bind(visibility)
        .bind(workspace_id)
        .bind(user_id)
        .bind(visibility)
        .bind(workspace_id)
        .bind("online")
        .bind("busy")
        .bind(user_id)
        .bind(visibility)
        .bind(workspace_id)
        .bind(1i32)
        .bind(cutoff)
        .bind(user_id)
        .bind(visibility)
        .bind(workspace_id)
        .bind(user_id)
        .bind(workspace_id)
        .bind(user_id)
        .bind(visibility)
        .fetch_all(&pool)
        .await
    {
        Ok(rows) => rows,
        Err(_) => return server_error(),
    };
    let mut bodies = Vec::with_capacity(rows.len());
    for row in &rows {
        let machine = match decode_machine_list_row(row) {
            Ok(machine) => machine,
            Err(response) => return response,
        };
        match render_machine(&machine) {
            Ok(body) => bodies.push(body),
            Err(response) => return response,
        }
    }
    json_response(StatusCode::OK, format!("[{}]", bodies.join(",")))
}

// ---------------------------------------------------------------------------
// Runner list (`runners.py:289-333`)
// ---------------------------------------------------------------------------

/// `GET /api/runners/` (`runners.py:289-333`): workspace gate, the
/// owner+private visibility predicate, `?pod=`/`?project=` narrowing,
/// desktop-bundled exclusion unless `?include_bundled=` is exactly
/// `1`/`true`/`yes`, `-updated_at` order.
pub async fn runners_list(
    State(state): State<AppState>,
    extension: Option<Extension<SessionHandle>>,
    Query(params): Query<crate::license::QueryMap>,
) -> Response {
    let (pool, user_id) = match web_actor(&state, extension).await {
        Ok(preamble) => preamble,
        Err(response) => return response,
    };
    let Some(raw) = query_param(&params, "workspace") else {
        return bad_request(WORKSPACE_REQUIRED_BODY);
    };
    let workspace_id = match parse_uuid(&raw) {
        Ok(workspace_id) => workspace_id,
        Err(response) => return response,
    };
    let role = match workspace_role(&pool, workspace_id, user_id).await {
        Ok(role) => role,
        Err(response) => return response,
    };
    if !membership::is_workspace_member(role) {
        return forbidden();
    }
    // Filter UUIDs parse at queryset evaluation — after the member
    // gate, so a non-member with a garbage filter still answers 403.
    let mut pod_id = None;
    if let Some(raw) = query_param(&params, "pod") {
        match parse_uuid(&raw) {
            Ok(id) => pod_id = Some(id),
            Err(response) => return response,
        }
    }
    let mut project_id = None;
    if let Some(raw) = query_param(&params, "project") {
        match parse_uuid(&raw) {
            Ok(id) => project_id = Some(id),
            Err(response) => return response,
        }
    }
    let exclude_bundled = !manage_reads::include_bundled(
        crate::license::query_last(&params, "include_bundled").as_deref(),
    );
    let sql =
        manage_reads::runner_list_sql(pod_id.is_some(), exclude_bundled, project_id.is_some());
    let visibility: i16 = runner_perm::VISIBILITY_PRIVATE as i16;
    let mut query = sqlx::query(&sql)
        .bind(workspace_id)
        .bind(user_id)
        .bind(visibility);
    if let Some(pod) = pod_id {
        query = query.bind(pod);
    }
    if exclude_bundled {
        query = query.bind("desktop_bundled");
    }
    if let Some(project) = project_id {
        query = query.bind(project);
    }
    let rows: Vec<sqlx::postgres::PgRow> = match query.fetch_all(&pool).await {
        Ok(rows) => rows,
        Err(_) => return server_error(),
    };
    let mut bodies = Vec::with_capacity(rows.len());
    for row in &rows {
        match render_runner_row(&pool, row).await {
            Ok(body) => bodies.push(body),
            Err(response) => return response,
        }
    }
    json_response(StatusCode::OK, format!("[{}]", bodies.join(",")))
}

// ---------------------------------------------------------------------------
// Runner detail (`runners.py:336-425`)
// ---------------------------------------------------------------------------

/// A resolved runner row: the R2 join row plus the caller's role in the
/// runner's workspace (fetched once, shared by the member/view/manage
/// gates).
struct RunnerHit {
    row: sqlx::postgres::PgRow,
    role: Option<i32>,
}

/// `_get_runner` (`runners.py:342-352`): the R2 read, then the
/// None-vs-False contract — missing row (or present-but-unviewable) →
/// 404 `not found`, non-member of the runner's workspace → 403
/// `forbidden`. The member query runs only for a present row.
async fn get_runner(pool: &PgPool, user_id: Uuid, runner_id: Uuid) -> Result<RunnerHit, Response> {
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(&manage_reads::runner_detail_sql())
        .bind(runner_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| server_error())?;
    let Some(row) = row else {
        return Err(not_found());
    };
    let runner = decode_runner(&row, 0)?;
    let role = workspace_role(pool, runner.workspace_id, user_id).await?;
    let can_view = runner_perm::can_view_runner(&runner_perm::RunnerFacts {
        workspace: WorkspaceId::from(runner.workspace_id.to_string()),
        authenticated: true,
        visibility: i32::from(runner.visibility),
        owned_by_requester: runner.owner_id == user_id,
    });
    match manage_reads::detail_outcome(true, membership::is_workspace_member(role), can_view) {
        manage_reads::DetailOutcome::Found => Ok(RunnerHit { row, role }),
        manage_reads::DetailOutcome::Missing => Err(not_found()),
        manage_reads::DetailOutcome::Forbidden => Err(forbidden()),
    }
}

/// `can_manage_runner` (`permissions.py:126-137`) over the resolved
/// row: private runners are owner-managed only (the only reachable
/// arm — non-private rows already fail the view gate).
fn can_manage_hit(runner: &RunnerOwned, user_id: Uuid, role: Option<i32>) -> bool {
    let workspace = WorkspaceId::from(runner.workspace_id.to_string());
    let scope = TenantScope::new(workspace.clone());
    runner_perm::can_manage_runner(
        &scope,
        &runner_perm::ManageFacts {
            workspace,
            requester: Some(UserId::from(user_id.to_string())),
            visibility: i32::from(runner.visibility),
            owned_by_requester: runner.owner_id == user_id,
            is_workspace_admin: membership::is_workspace_admin(role),
        },
    )
}

/// `GET /api/runners/<runner_id>/` (`runners.py:354-360`).
pub async fn runner_detail(
    State(state): State<AppState>,
    extension: Option<Extension<SessionHandle>>,
    Path(raw_id): Path<String>,
) -> Response {
    let (pool, user_id) = match web_actor(&state, extension).await {
        Ok(preamble) => preamble,
        Err(response) => return response,
    };
    let runner_id: Uuid = match raw_id.parse() {
        Ok(runner_id) => runner_id,
        Err(_) => return not_found(),
    };
    let hit = match get_runner(&pool, user_id, runner_id).await {
        Ok(hit) => hit,
        Err(response) => return response,
    };
    match render_runner_row(&pool, &hit.row).await {
        Ok(body) => json_response(StatusCode::OK, body),
        Err(response) => response,
    }
}

/// `PATCH /api/runners/<runner_id>/` (`runners.py:362-425`): rename
/// validation, then the pod branch — locked pod read, workspace
/// check, the busy 409 on a real move — all inside one transaction
/// (which opens ONLY when `"pod"` is in the body). A rename-only body
/// saves outside any transaction. The response serializes the
/// in-memory row: uncached relations re-read exactly as the source's
/// lazy serializers do (the new pod's project slug when moved, the
/// live state always).
pub async fn runner_patch(
    State(state): State<AppState>,
    extension: Option<Extension<SessionHandle>>,
    Path(raw_id): Path<String>,
    req: Request,
) -> Response {
    let (pool, user_id) = match web_actor(&state, extension).await {
        Ok(preamble) => preamble,
        Err(response) => return response,
    };
    let runner_id: Uuid = match raw_id.parse() {
        Ok(runner_id) => runner_id,
        Err(_) => return not_found(),
    };
    let hit = match get_runner(&pool, user_id, runner_id).await {
        Ok(hit) => hit,
        Err(response) => return response,
    };
    let mut runner = match decode_runner(&hit.row, 0) {
        Ok(runner) => runner,
        Err(response) => return response,
    };
    if !can_manage_hit(&runner, user_id, hit.role) {
        return forbidden();
    }
    // `request.data` is first touched AFTER the guards (`:372`), so a
    // 404/403 beats a 400/500 from the body — parse here, not up front.
    let data = match read_request_data(&state, req).await {
        Ok(data) => data,
        Err(response) => return response,
    };
    let mut new_name: Option<String> = None;
    match data_has(&data, "name") {
        Ok(true) => {
            let raw = match data_get(&data, "name") {
                Ok(raw) => raw,
                Err(response) => return response,
            };
            match or_empty_stripped(raw) {
                Ok(stripped) if stripped.is_empty() => {
                    return bad_request(NAME_CANNOT_BE_EMPTY_BODY);
                }
                Ok(stripped) => new_name = Some(stripped),
                Err(response) => return response,
            }
        }
        Ok(false) => {}
        Err(response) => return response,
    }
    let mut new_pod: Option<PodOwned> = None;
    let want_pod = match data_has(&data, "pod") {
        Ok(want_pod) => want_pod,
        Err(response) => return response,
    };
    if want_pod {
        let mut tx = match pool.begin().await {
            Ok(tx) => tx,
            Err(_) => return server_error(),
        };
        let member = match data_get(&data, "pod") {
            Ok(member) => member,
            Err(response) => return response,
        };
        // `request.data.get("pod")` — present-but-null reads as missing.
        let target_id = match member {
            None => None,
            Some(value) => match pod_member_id(value) {
                Ok(target_id) => target_id,
                Err(response) => return response,
            },
        };
        let target: Option<PodOwned> = match target_id {
            None => None,
            Some(target_id) => {
                let row: Option<sqlx::postgres::PgRow> =
                    match sqlx::query(&manage_reads::pod_locked_read_sql())
                        .bind(target_id)
                        .fetch_optional(&mut *tx)
                        .await
                    {
                        Ok(row) => row,
                        Err(_) => return server_error(),
                    };
                match row {
                    None => None,
                    Some(row) => match decode_pod(&row, 0) {
                        Ok(pod) => Some(pod),
                        Err(response) => return response,
                    },
                }
            }
        };
        let Some(target) = target else {
            return bad_request(POD_MISSING_BODY);
        };
        if target.workspace_id != runner.workspace_id {
            return bad_request(POD_OTHER_WORKSPACE_BODY);
        }
        if manage_reads::is_real_move(&target.id, &runner.pod_id) {
            let guard_sql = manage_reads::runner_busy_guard_sql();
            let mut guard = sqlx::query(&guard_sql)
                .bind(1i32)
                .bind(runner.id)
                .bind(runner.id);
            for status in manage_reads::NON_TERMINAL_STATUSES {
                guard = guard.bind(*status);
            }
            let busy: Option<i32> = match guard.fetch_optional(&mut *tx).await {
                Ok(busy) => busy.map(|_| 1i32),
                Err(_) => return server_error(),
            };
            if busy.is_some() {
                return conflict(RUNNER_BUSY_BODY);
            }
        }
        // `runner.save(update_fields=…)` — the `auto_now` stamp is a
        // fresh `now()`; the pod auto-resolve is skipped (`pod_id` set).
        let now = now_micros();
        let sql = manage_reads::runner_patch_sql(true, new_name.is_some());
        let mut update = sqlx::query(&sql).bind(target.id);
        if let Some(name) = &new_name {
            update = update.bind(name);
        }
        if update
            .bind(now)
            .bind(runner.id)
            .execute(&mut *tx)
            .await
            .is_err()
        {
            return server_error();
        }
        if tx.commit().await.is_err() {
            return server_error();
        }
        runner.pod_id = target.id;
        runner.updated_at = now;
        if let Some(name) = new_name.clone() {
            runner.name = name;
        }
        new_pod = Some(target);
    } else if let Some(name) = new_name.clone() {
        // Rename-only: a standalone autocommit save, no transaction.
        let now = now_micros();
        if sqlx::query(&manage_reads::runner_patch_sql(false, true))
            .bind(&name)
            .bind(now)
            .bind(runner.id)
            .execute(&pool)
            .await
            .is_err()
        {
            return server_error();
        }
        runner.name = name;
        runner.updated_at = now;
    }
    // Serialize the in-memory row: the pod mini comes from the moved-to
    // pod (whose project slug re-reads — the lock-read instance never
    // cached `project`) or from the `_get_runner` join; the dev-machine
    // mini stays cached; the live state re-reads.
    let (pod, pod_identifier, dev_machine_detail) = match new_pod {
        Some(target) => {
            let identifier = match fetch_project_facts(&pool, target.project_id).await {
                Ok(Some(facts)) => facts.identifier,
                Ok(None) | Err(_) => return server_error(),
            };
            let mini = match decode_machine_mini(&hit.row, RUNNER_BLOCK_COLS) {
                Ok(mini) => mini,
                Err(response) => return response,
            };
            (target, identifier, mini)
        }
        None => {
            let pod = match decode_pod(&hit.row, RUNNER_BLOCK_COLS + MACHINE_BLOCK_COLS) {
                Ok(pod) => pod,
                Err(response) => return response,
            };
            let identifier: String = match hit.row.try_get("identifier") {
                Ok(identifier) => identifier,
                Err(_) => return server_error(),
            };
            let mini = match decode_machine_mini(&hit.row, RUNNER_BLOCK_COLS) {
                Ok(mini) => mini,
                Err(response) => return response,
            };
            (pod, identifier, mini)
        }
    };
    match render_runner_parts(&pool, &runner, &pod, pod_identifier, dev_machine_detail).await {
        Ok(body) => json_response(StatusCode::OK, body),
        Err(response) => response,
    }
}

// ---------------------------------------------------------------------------
// Pod list (`pods.py:45-130`)
// ---------------------------------------------------------------------------

/// `GET /api/runners/pods/` (`pods.py:55-86`): `?project=` wins over
/// `?workspace=`; project mode 404s on an unknown project and 403s for
/// non-members of the project's workspace; workspace mode 400s when
/// neither filter is present. Both lists order `-is_default,
/// created_at`.
pub async fn pods_list(
    State(state): State<AppState>,
    extension: Option<Extension<SessionHandle>>,
    Query(params): Query<crate::license::QueryMap>,
) -> Response {
    let (pool, user_id) = match web_actor(&state, extension).await {
        Ok(preamble) => preamble,
        Err(response) => return response,
    };
    let rows: Vec<sqlx::postgres::PgRow> = if let Some(raw) = query_param(&params, "project") {
        let project_id = match parse_uuid(&raw) {
            Ok(project_id) => project_id,
            Err(response) => return response,
        };
        let facts = match fetch_project_facts(&pool, project_id).await {
            Ok(facts) => facts,
            Err(response) => return response,
        };
        let Some(facts) = facts else {
            return json_response(StatusCode::NOT_FOUND, PROJECT_NOT_FOUND_BODY.to_owned());
        };
        let role = match workspace_role(&pool, facts.workspace_id, user_id).await {
            Ok(role) => role,
            Err(response) => return response,
        };
        if !membership::is_workspace_member(role) {
            return forbidden();
        }
        match sqlx::query(&catalog_reads::pods_by_project_sql())
            .bind(project_id)
            .fetch_all(&pool)
            .await
        {
            Ok(rows) => rows,
            Err(_) => return server_error(),
        }
    } else {
        let Some(raw) = query_param(&params, "workspace") else {
            return bad_request(PROJECT_OR_WORKSPACE_REQUIRED_BODY);
        };
        let workspace_id = match parse_uuid(&raw) {
            Ok(workspace_id) => workspace_id,
            Err(response) => return response,
        };
        let role = match workspace_role(&pool, workspace_id, user_id).await {
            Ok(role) => role,
            Err(response) => return response,
        };
        if !membership::is_workspace_member(role) {
            return forbidden();
        }
        match sqlx::query(&catalog_reads::pods_by_workspace_sql())
            .bind(workspace_id)
            .fetch_all(&pool)
            .await
        {
            Ok(rows) => rows,
            Err(_) => return server_error(),
        }
    };
    let mut bodies = Vec::with_capacity(rows.len());
    for row in &rows {
        let pod = match decode_pod(row, 0) {
            Ok(pod) => pod,
            Err(response) => return response,
        };
        match render_pod(&pool, &pod, None).await {
            Ok(body) => bodies.push(body),
            Err(response) => return response,
        }
    }
    json_response(StatusCode::OK, format!("[{}]", bodies.join(",")))
}

/// Render a `validate_user_pod_name` failure: `{"error": <message>}`.
fn naming_denial(message: &str) -> Response {
    bad_request(&naming_denial_body(message))
}

/// Pure body behind [`naming_denial`].
fn naming_denial_body(message: &str) -> String {
    serde_json::to_string(&serde_json::json!({"error": message})).expect("error body renders")
}

/// `POST /api/runners/pods/` (`pods.py:88-130`): admin-only create —
/// project+name required, unknown project 404s, non-admin 403s, the
/// bare suffix re-prefixes with the project identifier, then the full
/// naming validator runs. `is_default` is always false; a name
/// collision 500s (`QUIRK-create-conflict-500`).
pub async fn pods_create(
    State(state): State<AppState>,
    extension: Option<Extension<SessionHandle>>,
    req: Request,
) -> Response {
    let (pool, user_id) = match web_actor(&state, extension).await {
        Ok(preamble) => preamble,
        Err(response) => return response,
    };
    // `request.data` is touched first (`:91`) — before every guard.
    let data = match read_request_data(&state, req).await {
        Ok(data) => data,
        Err(response) => return response,
    };
    let project_value = match data_get(&data, "project") {
        Ok(project_value) => project_value,
        Err(response) => return response,
    };
    // The strip runs BEFORE the required check (`:92-94`), so a truthy
    // non-string name 500s even when the project is missing.
    let name = match data_get(&data, "name") {
        Ok(name_value) => match or_empty_stripped(name_value) {
            Ok(name) => name,
            Err(response) => return response,
        },
        Err(response) => return response,
    };
    let description = match data_get(&data, "description") {
        Ok(description_value) => match j_description(description_value) {
            Ok(description) => description,
            Err(response) => return response,
        },
        Err(response) => return response,
    };
    let has_project = project_value.map(j_truthy).unwrap_or(false);
    if !has_project || name.is_empty() {
        return bad_request(PROJECT_AND_NAME_REQUIRED_BODY);
    }
    let project_id = match body_uuid(project_value.expect("truthy project")) {
        Ok(project_id) => project_id,
        Err(response) => return response,
    };
    let facts = match fetch_project_facts(&pool, project_id).await {
        Ok(facts) => facts,
        Err(response) => return response,
    };
    let Some(facts) = facts else {
        return json_response(StatusCode::NOT_FOUND, PROJECT_NOT_FOUND_BODY.to_owned());
    };
    let role = match workspace_role(&pool, facts.workspace_id, user_id).await {
        Ok(role) => role,
        Err(response) => return response,
    };
    if !membership::is_workspace_admin(role) {
        return json_response(
            StatusCode::FORBIDDEN,
            WORKSPACE_ADMIN_REQUIRED_BODY.to_owned(),
        );
    }
    let final_name = catalog_reads::prefixed_pod_name(&name, &facts.identifier);
    if let Some(message) = pod_naming::validate_user_pod_name(&final_name, &facts.identifier) {
        return naming_denial(&message);
    }
    // `auto_now_add` + `auto_now` are two separate `now()` calls; the
    // denorm check no-ops (`workspace_id == project.workspace_id`).
    let created_at = now_micros();
    let updated_at = now_micros();
    let pod_id = Uuid::new_v4();
    if sqlx::query(&catalog_reads::pod_insert_sql())
        .bind(pod_id)
        .bind(facts.workspace_id)
        .bind(project_id)
        .bind(&final_name)
        .bind(&description)
        .bind(user_id)
        .bind(false)
        .bind(None::<DateTime<Utc>>)
        .bind(created_at)
        .bind(updated_at)
        .execute(&pool)
        .await
        .is_err()
    {
        return server_error();
    }
    let pod = PodOwned {
        id: pod_id,
        workspace_id: facts.workspace_id,
        project_id,
        name: final_name,
        description,
        created_by: Some(user_id),
        is_default: false,
        created_at,
        updated_at,
    };
    // The project object stays cached on the created pod — no re-read.
    match render_pod(&pool, &pod, Some(facts.identifier)).await {
        Ok(body) => json_response(StatusCode::CREATED, body),
        Err(response) => response,
    }
}

// ---------------------------------------------------------------------------
// Pod detail (`pods.py:133-267`)
// ---------------------------------------------------------------------------

/// A resolved pod row plus the caller's role in the pod's workspace.
struct PodHit {
    pod: PodOwned,
    role: Option<i32>,
}

/// `_get_pod` (`pods.py:139-145`): the P3 read, then the None-vs-False
/// contract — missing (or tombstoned) → 404, non-member → 403.
async fn get_pod(pool: &PgPool, user_id: Uuid, pod_id: Uuid) -> Result<PodHit, Response> {
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(&catalog_reads::pod_by_id_sql())
        .bind(pod_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| server_error())?;
    let Some(row) = row else {
        return Err(not_found());
    };
    let pod = decode_pod(&row, 0)?;
    let role = workspace_role(pool, pod.workspace_id, user_id).await?;
    if !membership::is_workspace_member(role) {
        return Err(forbidden());
    }
    Ok(PodHit { pod, role })
}

/// `_can_manage_pod` (`pods.py:40-42`): workspace admin OR the pod's
/// creator.
fn can_manage_pod(role: Option<i32>, pod: &PodOwned, user_id: Uuid) -> bool {
    membership::is_workspace_admin(role) || pod.created_by == Some(user_id)
}

/// `GET /api/runners/pods/<pod_id>/` (`pods.py:147-153`).
pub async fn pod_detail(
    State(state): State<AppState>,
    extension: Option<Extension<SessionHandle>>,
    Path(raw_id): Path<String>,
) -> Response {
    let (pool, user_id) = match web_actor(&state, extension).await {
        Ok(preamble) => preamble,
        Err(response) => return response,
    };
    let pod_id: Uuid = match raw_id.parse() {
        Ok(pod_id) => pod_id,
        Err(_) => return not_found(),
    };
    let hit = match get_pod(&pool, user_id, pod_id).await {
        Ok(hit) => hit,
        Err(response) => return response,
    };
    match render_pod(&pool, &hit.pod, None).await {
        Ok(body) => json_response(StatusCode::OK, body),
        Err(response) => response,
    }
}

/// `PATCH /api/runners/pods/<pod_id>/` (`pods.py:155-204`): rename
/// (bare-suffix re-prefix + full validator), description, and the
/// default promote/demote branches — the promote demotes the project's
/// siblings inside its own transaction, committed before the final
/// save. The final save runs only when at least one key applied.
pub async fn pod_patch(
    State(state): State<AppState>,
    extension: Option<Extension<SessionHandle>>,
    Path(raw_id): Path<String>,
    req: Request,
) -> Response {
    let (pool, user_id) = match web_actor(&state, extension).await {
        Ok(preamble) => preamble,
        Err(response) => return response,
    };
    let pod_id: Uuid = match raw_id.parse() {
        Ok(pod_id) => pod_id,
        Err(_) => return not_found(),
    };
    let hit = match get_pod(&pool, user_id, pod_id).await {
        Ok(hit) => hit,
        Err(response) => return response,
    };
    if !can_manage_pod(hit.role, &hit.pod, user_id) {
        return forbidden();
    }
    // `request.data` is first touched AFTER the guards (`:165`).
    let data = match read_request_data(&state, req).await {
        Ok(data) => data,
        Err(response) => return response,
    };
    let mut pod = hit.pod;
    // The cached `pod.project` (`None` until the rename branch or the
    // save's denorm check loads it).
    let mut project_identifier: Option<String> = None;
    let mut update_name = false;
    let mut update_description = false;
    let mut update_is_default = false;
    match data_has(&data, "name") {
        Ok(true) => {
            let raw = match data_get(&data, "name") {
                Ok(raw) => raw,
                Err(response) => return response,
            };
            let stripped = match or_empty_stripped(raw) {
                Ok(stripped) => stripped,
                Err(response) => return response,
            };
            if stripped.is_empty() {
                return bad_request(NAME_CANNOT_BE_EMPTY_BODY);
            }
            let facts = match fetch_project_facts(&pool, pod.project_id).await {
                Ok(Some(facts)) => facts,
                Ok(None) | Err(_) => return server_error(),
            };
            let final_name = catalog_reads::prefixed_pod_name(&stripped, &facts.identifier);
            if let Some(message) =
                pod_naming::validate_user_pod_name(&final_name, &facts.identifier)
            {
                return naming_denial(&message);
            }
            pod.name = final_name;
            project_identifier = Some(facts.identifier);
            update_name = true;
        }
        Ok(false) => {}
        Err(response) => return response,
    }
    match data_has(&data, "description") {
        Ok(true) => {
            let raw = match data_get(&data, "description") {
                Ok(raw) => raw,
                Err(response) => return response,
            };
            match j_description(raw) {
                Ok(description) => {
                    pod.description = description;
                    update_description = true;
                }
                Err(response) => return response,
            }
        }
        Ok(false) => {}
        Err(response) => return response,
    }
    match data_has(&data, "is_default") {
        Ok(true) => {
            let raw = match data_get(&data, "is_default") {
                Ok(raw) => raw,
                Err(response) => return response,
            };
            let wants_default = raw.map(j_truthy).unwrap_or(false);
            if wants_default && !pod.is_default {
                let mut tx = match pool.begin().await {
                    Ok(tx) => tx,
                    Err(_) => return server_error(),
                };
                if sqlx::query(&catalog_reads::pod_demote_siblings_sql())
                    .bind(false)
                    .bind(pod.project_id)
                    .bind(pod.id)
                    .execute(&mut *tx)
                    .await
                    .is_err()
                {
                    return server_error();
                }
                if tx.commit().await.is_err() {
                    return server_error();
                }
                pod.is_default = true;
                update_is_default = true;
            } else if !wants_default && pod.is_default {
                pod.is_default = false;
                update_is_default = true;
            }
        }
        Ok(false) => {}
        Err(response) => return response,
    }
    if update_name || update_description || update_is_default {
        if project_identifier.is_none() {
            // `Pod.save()` denorm check (`QUIRK-denorm-read`): the
            // project read the rename branch would have cached.
            let facts = match fetch_project_facts(&pool, pod.project_id).await {
                Ok(Some(facts)) => facts,
                Ok(None) | Err(_) => return server_error(),
            };
            project_identifier = Some(facts.identifier);
        }
        let now = now_micros();
        let sql =
            catalog_reads::pod_patch_update_sql(update_name, update_description, update_is_default);
        let mut update = sqlx::query(&sql);
        if update_name {
            update = update.bind(&pod.name);
        }
        if update_description {
            update = update.bind(&pod.description);
        }
        if update_is_default {
            update = update.bind(pod.is_default);
        }
        if update.bind(now).bind(pod.id).execute(&pool).await.is_err() {
            return server_error();
        }
        pod.updated_at = now;
    }
    match render_pod(&pool, &pod, project_identifier).await {
        Ok(body) => json_response(StatusCode::OK, body),
        Err(response) => response,
    }
}

/// `DELETE /api/runners/pods/<pod_id>/` (`pods.py:206-267`): the three
/// §7.2 guards run inside the transaction with the pod row locked
/// (runners → active runs → default, first hit wins), then the
/// soft-delete stamp and the `Issue.assigned_pod` sweep. Answers 204
/// with an empty body.
pub async fn pod_delete(
    State(state): State<AppState>,
    extension: Option<Extension<SessionHandle>>,
    Path(raw_id): Path<String>,
) -> Response {
    let (pool, user_id) = match web_actor(&state, extension).await {
        Ok(preamble) => preamble,
        Err(response) => return response,
    };
    let pod_id: Uuid = match raw_id.parse() {
        Ok(pod_id) => pod_id,
        Err(_) => return not_found(),
    };
    let hit = match get_pod(&pool, user_id, pod_id).await {
        Ok(hit) => hit,
        Err(response) => return response,
    };
    if !can_manage_pod(hit.role, &hit.pod, user_id) {
        return forbidden();
    }
    // The view never touches `request.data` — no body parse, so even an
    // unparseable body cannot fail this path.
    let mut tx = match pool.begin().await {
        Ok(tx) => tx,
        Err(_) => return server_error(),
    };
    let locked_row: Option<sqlx::postgres::PgRow> =
        match sqlx::query(&catalog_reads::pod_locked_read_sql())
            .bind(pod_id)
            .fetch_optional(&mut *tx)
            .await
        {
            Ok(locked_row) => locked_row,
            Err(_) => return server_error(),
        };
    let Some(locked_row) = locked_row else {
        return not_found();
    };
    let locked = match decode_pod(&locked_row, 0) {
        Ok(locked) => locked,
        Err(response) => return response,
    };
    let runners_hit: Option<i32> = match sqlx::query(&catalog_reads::pod_runners_exist_sql())
        .bind(pod_id)
        .bind("revoked")
        .fetch_optional(&mut *tx)
        .await
    {
        Ok(hit) => hit.map(|_| 1i32),
        Err(_) => return server_error(),
    };
    if runners_hit.is_some() {
        return conflict(POD_HAS_RUNNERS_BODY);
    }
    let guard_sql = catalog_reads::pod_active_runs_exist_sql();
    let mut guard = sqlx::query(&guard_sql).bind(pod_id);
    for status in catalog_reads::NON_TERMINAL_STATUSES {
        guard = guard.bind(*status);
    }
    let runs_hit: Option<i32> = match guard.fetch_optional(&mut *tx).await {
        Ok(hit) => hit.map(|_| 1i32),
        Err(_) => return server_error(),
    };
    if runs_hit.is_some() {
        return conflict(POD_HAS_ACTIVE_RUNS_BODY);
    }
    if locked.is_default {
        return conflict(DEFAULT_POD_UNDELETABLE_BODY);
    }
    // `Pod.save()` denorm read, then the stamp, then the sweep — the
    // `save()` order (`QUIRK-denorm-read`).
    if denorm_project_read(&mut tx, locked.project_id)
        .await
        .is_err()
    {
        return server_error();
    }
    let deleted_at = now_micros();
    let updated_at = now_micros();
    if sqlx::query(&catalog_reads::pod_soft_delete_sql())
        .bind(false)
        .bind(deleted_at)
        .bind(updated_at)
        .bind(pod_id)
        .execute(&mut *tx)
        .await
        .is_err()
    {
        return server_error();
    }
    if sqlx::query(&catalog_reads::issue_assigned_pod_clear_sql())
        .bind(pod_id)
        .execute(&mut *tx)
        .await
        .is_err()
    {
        return server_error();
    }
    if tx.commit().await.is_err() {
        return server_error();
    }
    StatusCode::NO_CONTENT.into_response()
}

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------

/// An owned path: the listed methods serve from Rust, every other
/// method falls through to Django (the `app_scheduler` precedent).
/// `HEAD` routes explicitly (the D-19/D-20 direction): axum would
/// otherwise answer it from the `GET` handler, but Django 405s
/// after auth — the proxy preserves that byte for byte.
fn owned(
    handler: axum::routing::MethodRouter<AppState>,
    unowned: &[&str],
) -> axum::routing::MethodRouter<AppState> {
    let mut router = handler;
    for method in unowned {
        router = match *method {
            "POST" => router.post(crate::edge::proxy),
            "PUT" => router.put(crate::edge::proxy),
            "PATCH" => router.patch(crate::edge::proxy),
            "DELETE" => router.delete(crate::edge::proxy),
            "HEAD" => router.head(crate::edge::proxy),
            "OPTIONS" => router.options(crate::edge::proxy),
            _ => router.get(crate::edge::proxy),
        };
    }
    router
}

/// Register the web runners/machines/pods routes
/// (`runner/web_urls.py`: the dev-machine list, the runner list,
/// runner detail get+patch, the pod list get+post, pod detail
/// get+patch+delete). Merged under `RouteGroup::RunnerWeb` at the F-10
/// seam; sibling handler issues extend the merge, keeping both sides.
pub fn routes() -> Router<AppState> {
    use axum::routing::get;
    const GET_ONLY: &[&str] = &["POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];
    const GET_PATCH: &[&str] = &["POST", "PUT", "DELETE", "HEAD", "OPTIONS"];
    const GET_POST: &[&str] = &["PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];
    const GET_PATCH_DELETE: &[&str] = &["POST", "PUT", "HEAD", "OPTIONS"];
    Router::new()
        .route(
            "/api/runners/dev-machines/",
            owned(get(dev_machines_list), GET_ONLY),
        )
        .route("/api/runners/", owned(get(runners_list), GET_ONLY))
        .route(
            "/api/runners/{runner_id}/",
            owned(get(runner_detail).patch(runner_patch), GET_PATCH),
        )
        .route(
            "/api/runners/pods/",
            owned(get(pods_list).post(pods_create), GET_POST),
        )
        .route(
            "/api/runners/pods/{pod_id}/",
            owned(
                get(pod_detail).patch(pod_patch).delete(pod_delete),
                GET_PATCH_DELETE,
            ),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::v1_cycles_modules::json_cpython::{JNum, JStr};
    use pidash_db::runner_enroll::columns::pod as pod_cols;

    const FIXTURE_ENDPOINTS: &str =
        include_str!("../../../../fixtures/runner_enroll/handlers/endpoints.golden.json");

    fn web_fixture(key: &str) -> serde_json::Value {
        let fixture: serde_json::Value = serde_json::from_str(FIXTURE_ENDPOINTS).unwrap();
        fixture["web"][key].clone()
    }

    /// Every `errors[]` body recorded for `key` must equal one of the
    /// given consts (compared as parsed JSON — key order pinned
    /// separately below).
    fn assert_errors_match(key: &str, consts: &[&str]) {
        let entry = web_fixture(key);
        let errors = entry["errors"].as_array().unwrap();
        assert!(!errors.is_empty(), "{key} records no errors");
        for error in errors {
            let body = &error["body"];
            assert!(
                consts
                    .iter()
                    .any(|c| serde_json::from_str::<serde_json::Value>(c).unwrap() == *body),
                "{key}: fixture body {body} matches no const"
            );
        }
    }

    fn jstr(text: &str) -> JVal {
        JVal::Str(JStr::from_text(text))
    }

    fn jobject(pairs: &[(&str, JVal)]) -> JVal {
        let mut map = JObject::new();
        for (key, value) in pairs {
            map.insert(JStr::from_text(key), value.clone());
        }
        JVal::Object(map)
    }

    // -- D13-F7: every recorded error body for these 10 routes --

    #[test]
    fn f7_dev_machine_list_bodies() {
        assert_errors_match(
            "GET_dev_machines",
            &[WORKSPACE_REQUIRED_BODY, FORBIDDEN_BODY],
        );
    }

    #[test]
    fn f7_runner_list_bodies() {
        assert_errors_match("GET_runners", &[WORKSPACE_REQUIRED_BODY, FORBIDDEN_BODY]);
    }

    #[test]
    fn f7_runner_detail_bodies() {
        assert_errors_match("GET_runners_rid", &[NOT_FOUND_BODY, FORBIDDEN_BODY]);
        assert_errors_match(
            "PATCH_runners_rid",
            &[
                NOT_FOUND_BODY,
                FORBIDDEN_BODY,
                NAME_CANNOT_BE_EMPTY_BODY,
                POD_MISSING_BODY,
                POD_OTHER_WORKSPACE_BODY,
                RUNNER_BUSY_BODY,
            ],
        );
    }

    #[test]
    fn f7_pod_list_create_bodies() {
        assert_errors_match(
            "GET_pods",
            &[
                PROJECT_NOT_FOUND_BODY,
                FORBIDDEN_BODY,
                PROJECT_OR_WORKSPACE_REQUIRED_BODY,
            ],
        );
        // The create validator arm records a `<pod_naming message>`
        // placeholder — the naming kernel owns that text (D13-F6);
        // every concrete arm pins here.
        assert_errors_match(
            "POST_pods",
            &[
                PROJECT_AND_NAME_REQUIRED_BODY,
                PROJECT_NOT_FOUND_BODY,
                WORKSPACE_ADMIN_REQUIRED_BODY,
                r#"{"error":"<pod_naming message>"}"#,
            ],
        );
    }

    #[test]
    fn f7_pod_detail_bodies() {
        assert_errors_match("GET_pods_pid", &[NOT_FOUND_BODY, FORBIDDEN_BODY]);
        assert_errors_match(
            "PATCH_pods_pid",
            &[
                NOT_FOUND_BODY,
                FORBIDDEN_BODY,
                NAME_CANNOT_BE_EMPTY_BODY,
                r#"{"error":"<pod_naming message>"}"#,
            ],
        );
        assert_errors_match(
            "DELETE_pods_pid",
            &[
                NOT_FOUND_BODY,
                FORBIDDEN_BODY,
                POD_HAS_RUNNERS_BODY,
                POD_HAS_ACTIVE_RUNS_BODY,
                DEFAULT_POD_UNDELETABLE_BODY,
            ],
        );
    }

    #[test]
    fn f7_statuses_match() {
        let entry = web_fixture("PATCH_runners_rid");
        let errors = entry["errors"].as_array().unwrap();
        let status_of = |error: &str| {
            errors.iter().find(|e| e["body"]["error"] == error).unwrap()["status"]
                .as_u64()
                .unwrap()
        };
        assert_eq!(status_of("not found"), 404);
        assert_eq!(status_of("forbidden"), 403);
        assert_eq!(status_of("name cannot be empty"), 400);
        let entry = web_fixture("DELETE_pods_pid");
        let codes: Vec<&str> = entry["errors"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|e| e["body"]["code"].as_str())
            .collect();
        assert_eq!(
            codes,
            vec![
                "pod_has_runners",
                "pod_has_active_runs",
                "default_pod_undeletable"
            ]
        );
    }

    /// Wire key order is the Python dict-literal order (`error` before
    /// `code`) — the fixture parses order-insensitively, so pin the
    /// bytes here.
    #[test]
    fn error_first_key_order() {
        for body in [
            RUNNER_BUSY_BODY,
            POD_HAS_RUNNERS_BODY,
            POD_HAS_ACTIVE_RUNS_BODY,
            DEFAULT_POD_UNDELETABLE_BODY,
        ] {
            assert!(
                body.starts_with(r#"{"error":"#),
                "error key must lead: {body}"
            );
            assert!(body.contains(r#","code":"#), "code key present: {body}");
        }
    }

    // -- Python-truthiness + body helpers --

    #[test]
    fn truthy_matrix() {
        assert!(!j_truthy(&JVal::Null));
        assert!(!j_truthy(&JVal::Bool(false)));
        assert!(j_truthy(&JVal::Bool(true)));
        assert!(!j_truthy(&JVal::Num(JNum::int("0".to_owned()))));
        assert!(j_truthy(&JVal::Num(JNum::int("2".to_owned()))));
        assert!(!j_truthy(&JVal::Num(JNum::float("0.0".to_owned()))));
        assert!(!j_truthy(&JVal::Num(JNum::float("-0.0".to_owned()))));
        assert!(j_truthy(&JVal::Num(JNum::float("0.5".to_owned()))));
        assert!(!j_truthy(&jstr("")));
        // QUIRK-patch-bool: a non-empty string is truthy, whatever it says.
        assert!(j_truthy(&jstr("false")));
        assert!(!j_truthy(&JVal::Array(vec![])));
        assert!(j_truthy(&JVal::Array(vec![JVal::Null])));
        assert!(!j_truthy(&JVal::Object(JObject::new())));
        assert!(j_truthy(&jobject(&[("a", JVal::Null)])));
    }

    #[test]
    fn data_has_matrix() {
        let object = jobject(&[("name", jstr("x"))]);
        assert!(data_has(&object, "name").unwrap());
        assert!(!data_has(&object, "pod").unwrap());
        // `in` on an array is element equality …
        let array = JVal::Array(vec![jstr("name"), JVal::Num(JNum::int("1".to_owned()))]);
        assert!(data_has(&array, "name").unwrap());
        assert!(!data_has(&array, "pod").unwrap());
        // … on a string it is substring search …
        assert!(data_has(&jstr("rename me"), "name").unwrap());
        // … and on scalars it is TypeError → 500.
        for scalar in [
            JVal::Null,
            JVal::Bool(true),
            JVal::Num(JNum::int("1".to_owned())),
        ] {
            assert!(data_has(&scalar, "name").is_err());
        }
    }

    #[test]
    fn data_get_matrix() {
        let object = jobject(&[("name", jstr("x"))]);
        assert!(data_get(&object, "name").unwrap().is_some());
        assert!(data_get(&object, "pod").unwrap().is_none());
        // `.get` on a non-object is AttributeError → 500.
        for other in [
            JVal::Null,
            JVal::Bool(true),
            JVal::Num(JNum::int("1".to_owned())),
            jstr("name"),
            JVal::Array(vec![]),
        ] {
            assert!(data_get(&other, "name").is_err());
        }
    }

    #[test]
    fn strip_matrix() {
        assert_eq!(or_empty_stripped(None).unwrap(), "");
        assert_eq!(or_empty_stripped(Some(&JVal::Null)).unwrap(), "");
        assert_eq!(or_empty_stripped(Some(&jstr(""))).unwrap(), "");
        assert_eq!(or_empty_stripped(Some(&jstr("  x  "))).unwrap(), "x");
        assert_eq!(or_empty_stripped(Some(&jstr("  "))).unwrap(), "");
        // Extended Python strip set (U+0085, U+001C-U+001F).
        assert_eq!(
            or_empty_stripped(Some(&jstr("\u{85}x\u{1c}"))).unwrap(),
            "x"
        );
        // Falsy non-strings collapse to "" …
        assert_eq!(or_empty_stripped(Some(&JVal::Bool(false))).unwrap(), "");
        assert_eq!(
            or_empty_stripped(Some(&JVal::Num(JNum::int("0".to_owned())))).unwrap(),
            ""
        );
        // … truthy non-strings are AttributeError → 500.
        for bad in [
            JVal::Bool(true),
            JVal::Num(JNum::int("123".to_owned())),
            JVal::Array(vec![jstr("x")]),
            jobject(&[("a", jstr("x"))]),
        ] {
            assert!(or_empty_stripped(Some(&bad)).is_err());
        }
    }

    #[test]
    fn description_matrix() {
        assert_eq!(j_description(None).unwrap(), "");
        assert_eq!(j_description(Some(&JVal::Null)).unwrap(), "");
        assert_eq!(j_description(Some(&jstr(""))).unwrap(), "");
        // No strip on descriptions — verbatim.
        assert_eq!(j_description(Some(&jstr("  x  "))).unwrap(), "  x  ");
        // Python str() spellings.
        assert_eq!(
            j_description(Some(&JVal::Num(JNum::int("123".to_owned())))).unwrap(),
            "123"
        );
        assert_eq!(
            j_description(Some(&JVal::Num(JNum::float("1.5".to_owned())))).unwrap(),
            "1.5"
        );
        assert_eq!(j_description(Some(&JVal::Bool(true))).unwrap(), "True");
        assert_eq!(
            j_description(Some(&JVal::Array(vec![JVal::Num(JNum::int(
                "1".to_owned()
            ))])))
            .unwrap(),
            "[1]"
        );
        let obj = jobject(&[("a", JVal::Num(JNum::int("1".to_owned())))]);
        assert_eq!(j_description(Some(&obj)).unwrap(), "{'a': 1}");
        // Falsy containers collapse to "".
        assert_eq!(j_description(Some(&JVal::Array(vec![]))).unwrap(), "");
    }

    #[test]
    fn uuid_members() {
        let id = Uuid::new_v4();
        assert_eq!(body_uuid(&jstr(&id.to_string())).unwrap(), id);
        assert!(body_uuid(&jstr("nope")).is_err());
        assert!(body_uuid(&JVal::Num(JNum::int("123".to_owned()))).is_err());
        assert!(body_uuid(&JVal::Null).is_err());
        assert_eq!(pod_member_id(&JVal::Null).unwrap(), None);
        assert_eq!(pod_member_id(&jstr(&id.to_string())).unwrap(), Some(id));
    }

    #[test]
    fn naming_denial_shape() {
        let left =
            naming_denial_body("name suffix may only contain letters, digits, '.', '_', '-'");
        let right = "{\"error\":\"name suffix may only contain letters, digits, '.', '_', '-'\"}";
        assert_eq!(
            left.as_bytes(),
            right.as_bytes(),
            "left={left:?} right={right:?}"
        );
    }

    #[test]
    fn parse_error_body_shape() {
        assert_eq!(
            parse_error_body("Expecting value: line 1 column 1 (char 0)"),
            r#"{"Detail":"JSON parse error - Expecting value: line 1 column 1 (char 0)"}"#
        );
    }

    // -- Structural pins --

    #[test]
    fn join_block_widths() {
        // Positional decode offsets — fail loudly if the column consts move.
        assert_eq!(RUNNER_BLOCK_COLS, 30);
        assert_eq!(MACHINE_BLOCK_COLS, 10);
        assert_eq!(pod_cols::COLUMNS.len(), 10);
        assert_eq!(r_cols::COLUMNS[29], "revoked_reason");
        assert_eq!(dm_cols::COLUMNS[2], "host_label");
        assert_eq!(pod_cols::COLUMNS[5], "created_by_id");
    }

    #[test]
    fn binds_cover_builder_slots() {
        // Every `$N` slot the handlers bind must exist in the builder
        // text (a missing slot would 500 at runtime via sqlx arg-count).
        let m3 = manage_reads::machine_list_sql();
        for slot in 1..=20 {
            assert!(m3.contains(&format!("${slot}")), "M3 missing ${slot}");
        }
        for (pod, bundled, project, top) in [
            (false, false, false, 3),
            (true, false, false, 4),
            (false, true, false, 4),
            (false, false, true, 4),
            (true, true, true, 6),
        ] {
            let sql = manage_reads::runner_list_sql(pod, bundled, project);
            for slot in 1..=top {
                assert!(
                    sql.contains(&format!("${slot}")),
                    "R1({pod},{bundled},{project}) missing ${slot}"
                );
            }
        }
        let busy = manage_reads::runner_busy_guard_sql();
        for slot in 1..=11 {
            assert!(
                busy.contains(&format!("${slot}")),
                "R3 busy missing ${slot}"
            );
        }
        assert_eq!(manage_reads::NON_TERMINAL_STATUSES.len(), 8);
        assert_eq!(catalog_reads::NON_TERMINAL_STATUSES.len(), 8);
        let guards = catalog_reads::pod_active_runs_exist_sql();
        for slot in 1..=9 {
            assert!(
                guards.contains(&format!("${slot}")),
                "P5 guard 2 missing ${slot}"
            );
        }
        // Patch UPDATEs: trailing slot is always the row id.
        assert!(manage_reads::runner_patch_sql(true, true).ends_with("= $4"));
        assert!(manage_reads::runner_patch_sql(true, false).ends_with("= $3"));
        assert!(manage_reads::runner_patch_sql(false, true).ends_with("= $3"));
        assert!(catalog_reads::pod_patch_update_sql(true, true, true).ends_with("= $5"));
        assert!(catalog_reads::pod_patch_update_sql(false, false, true).ends_with("= $3"));
    }

    #[test]
    fn now_micros_has_no_sub_micro_part() {
        let now = now_micros();
        assert_eq!(now.timestamp_subsec_nanos() % 1000, 0);
    }

    #[test]
    fn routes_register() {
        // The router builds and carries exactly the five owned paths
        // (axum has no route introspection — construction is the pin).
        let _router: Router<AppState> = routes();
    }

    // -- D13-F2: byte-exact renders (key order + datetime format) --

    fn moment(raw: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(raw)
            .unwrap()
            .with_timezone(&Utc)
    }

    fn fixed_pod() -> PodOwned {
        PodOwned {
            id: Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap(),
            workspace_id: Uuid::parse_str("22222222-2222-2222-2222-222222222222").unwrap(),
            project_id: Uuid::parse_str("33333333-3333-3333-3333-333333333333").unwrap(),
            name: "WEB_beefy".to_owned(),
            description: "d".to_owned(),
            created_by: Some(Uuid::parse_str("44444444-4444-4444-4444-444444444444").unwrap()),
            is_default: false,
            created_at: moment("2026-10-01T12:34:56.789012+00:00"),
            updated_at: moment("2026-10-02T01:02:03+00:00"),
        }
    }

    #[test]
    fn pod_render_is_byte_exact() {
        let body = PodRender::new(&fixed_pod(), "WEB".to_owned(), 2)
            .to_json()
            .unwrap();
        assert_eq!(
            body,
            r#"{"id":"11111111-1111-1111-1111-111111111111","name":"WEB_beefy","description":"d","is_default":false,"workspace":"22222222-2222-2222-2222-222222222222","project":"33333333-3333-3333-3333-333333333333","project_identifier":"WEB","created_by":"44444444-4444-4444-4444-444444444444","runner_count":2,"created_at":"2026-10-01T12:34:56.789012Z","updated_at":"2026-10-02T01:02:03Z"}"#
        );
    }

    #[test]
    fn machine_render_is_byte_exact() {
        let machine = DevMachineOwned {
            id: Uuid::parse_str("55555555-5555-5555-5555-555555555555").unwrap(),
            host_label: "mac-mini".to_owned(),
            label: String::new(),
            visibility: 0,
            last_seen_at: None,
            revoked_at: None,
            created_at: moment("2026-10-01T12:34:56.789012+00:00"),
            updated_at: moment("2026-10-02T01:02:03+00:00"),
            runner_count: 1,
            online_runner_count: 0,
            last_heartbeat_at: None,
            control_online: false,
        };
        assert_eq!(
            render_machine(&machine).unwrap(),
            r#"{"id":"55555555-5555-5555-5555-555555555555","host_label":"mac-mini","label":"","visibility":0,"runner_count":1,"online_runner_count":0,"control_online":false,"last_seen_at":null,"last_heartbeat_at":null,"revoked_at":null,"created_at":"2026-10-01T12:34:56.789012Z","updated_at":"2026-10-02T01:02:03Z"}"#
        );
    }

    #[test]
    fn runner_render_is_byte_exact() {
        let runner = RunnerOwned {
            id: Uuid::parse_str("66666666-6666-6666-6666-666666666666").unwrap(),
            owner_id: Uuid::parse_str("44444444-4444-4444-4444-444444444444").unwrap(),
            workspace_id: Uuid::parse_str("22222222-2222-2222-2222-222222222222").unwrap(),
            dev_machine_id: None,
            pod_id: Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap(),
            name: "r1".to_owned(),
            host_label: "h".to_owned(),
            provisioning: "manual".to_owned(),
            visibility: 0,
            enrolled_at: None,
            capabilities: serde_json::json!([]),
            status: "offline".to_owned(),
            os: String::new(),
            arch: String::new(),
            runner_version: String::new(),
            dev_metadata: serde_json::json!({}),
            protocol_version: 4,
            last_heartbeat_at: None,
            created_at: moment("2026-10-01T12:34:56.789012+00:00"),
            updated_at: moment("2026-10-02T01:02:03+00:00"),
            revoked_at: None,
            revoked_reason: String::new(),
        };
        let live = LiveStateOwned {
            observed_run_id: None,
            last_event_at: Some("2026-10-02T01:02:03Z".to_owned()),
            last_event_kind: None,
            last_event_summary: None,
            agent_pid: None,
            agent_subprocess_alive: Some(true),
            approvals_pending: None,
            usage: serde_json::json!({}),
            llm_model: Some("m".to_owned()),
            turn_count: Some(3),
            updated_at: "2026-10-02T01:02:03Z".to_owned(),
        };
        let body = runner_render(&runner, &fixed_pod(), "WEB".to_owned(), None, Some(live))
            .to_json()
            .unwrap();
        assert_eq!(
            body,
            r#"{"id":"66666666-6666-6666-6666-666666666666","name":"r1","status":"offline","host_label":"h","provisioning":"manual","os":"","arch":"","runner_version":"","dev_metadata":{},"protocol_version":4,"capabilities":[],"last_heartbeat_at":null,"owner":"44444444-4444-4444-4444-444444444444","dev_machine":null,"dev_machine_detail":null,"visibility":0,"pod":"11111111-1111-1111-1111-111111111111","pod_detail":{"id":"11111111-1111-1111-1111-111111111111","name":"WEB_beefy","is_default":false,"project":"33333333-3333-3333-3333-333333333333","project_identifier":"WEB"},"live_state":{"observed_run_id":null,"last_event_at":"2026-10-02T01:02:03Z","last_event_kind":null,"last_event_summary":null,"agent_pid":null,"agent_subprocess_alive":true,"approvals_pending":null,"input_tokens":null,"output_tokens":null,"total_tokens":null,"usage":{},"llm_model":"m","turn_count":3,"updated_at":"2026-10-02T01:02:03Z"},"enrolled_at":null,"revoked_at":null,"revoked_reason":"","created_at":"2026-10-01T12:34:56.789012Z","updated_at":"2026-10-02T01:02:03Z"}"#
        );
    }
}
