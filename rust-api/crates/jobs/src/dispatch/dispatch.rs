#![forbid(unsafe_code)]

//! Creation + dispatch tasks for the Cloud Agent (D-11 L6, PIDASHCONV-487).
//!
//! Port of the creation seam and the dispatch engine:
//!
//! * `execution_fields` (`cloud_agent/creation.py:11-89`) → [`execution_fields`].
//! * `_managed_execution_fields` (`creation.py:92-132`) → [`managed_execution_decision`]
//!   (pure branch core) composed by [`execution_fields`]; `_MANAGED_REFUSAL_DETAIL`
//!   (`creation.py:135-145`) → [`managed_refusal_detail`].
//! * `dispatch_after_commit` (`creation.py:148-152`) → [`dispatch_after_commit`].
//! * `lock_cloud_creation_capacity` (`creation.py:155-177`) →
//!   [`lock_cloud_creation_capacity`] + [`quota_verdict`].
//! * `dispatch_waiting` (`cloud_agent/dispatch.py:20-48`) → [`dispatch_waiting`].
//! * `_publish` (`dispatch.py:51-58`) → [`run_cloud_agent_job`]: one queue row per
//!   offered id, enqueued in the same transaction as the lease updates, so the
//!   fan-out commits or rolls back with the dispatch. The worker forwards the
//!   row to the Python plane in Celery protocol v2 while the registry holds no
//!   local handler (F-09 ownership routing); per-id broker-failure tolerance
//!   lives on that forward path (retry with the default delay, never a drop),
//!   which is strictly stronger than Python's log-and-leave-leased.
//! * `dispatch_agent_run` (`dispatch.py:61-70`) → [`dispatch_agent_run`].
//! * `events.append` (`cloud_agent/events.py:9-25`) → [`append_event`] +
//!   [`event_cap_decision`].
//!
//! Out of scope here (owned by siblings): the executor-policy inputs L3/L4
//! already ported (`resolve_executor_kind`, `build_tool_plan`,
//! `user_has_llm_config`, `enforce_creation_rate`, `managed_runner_availability`
//! — reused, not redefined); the runner-owned `drain_pod_by_id` callee, which
//! surfaces as [`DispatchDecision::PodDrain`] for the runner plane (D-13+) to
//! serve; the `run_cloud_agent` task body (L7, PIDASHCONV-488).
//!
//! Translation notes:
//!
//! * Seams arrive as inputs, never as reimplemented logic (the L3/L4
//!   precedent): the admission cache as [`AdmissionCache`][pidash_services],
//!   the `has_usable_llm_config` / `extra_toolsets_enabled_for` /
//!   `managed_llm_profile` seams as `FnOnce` closures consulted only when the
//!   Python short-circuit reaches them (provable in tests), and the deferred
//!   admission consumes as an `on_commit` collector — callbacks registered so
//!   far stay registered even when a later gate raises, exactly like Django's
//!   ambient-transaction `on_commit`.
//! * `execution_fields` runs its capacity gate in its own short transaction
//!   (the inner `atomic` at `creation.py:55-70`, which commits before the
//!   fields are built); [`lock_cloud_creation_capacity`] instead runs on the
//!   caller's insertion transaction so the lock is held until the row lands.
//!   `dispatch_waiting` / `append_event` own their transaction (every call
//!   site invokes them outside any open transaction).
//! * SQL consts project only consumed columns (the services-layer convention);
//!   fixed enum values are literals, caller-supplied ids are `$n` params, each
//!   documented on its const. Statement shapes are cross-checked against the
//!   Django query compiler in review.
//!
//! Fixture: `rust-api/fixtures/dispatch/fx-disp-06-dispatch.golden.json`
//! (FX-DISP-06), replayed by the suite below.
//!
//! Ported bugs and quirks (translate, don't redesign):
//!
//! * The quota capture drops `retry_after_seconds`: the automatic path keeps
//!   `{code, detail}` only (`creation.py:70,175`).
//! * The truncation marker takes `seq = count + 1`, not `max(seq) + 1`
//!   (`events.py:21`) — a deleted middle row could collide; ported as-is.
//! * `payload or {}` collapses every falsy payload (`None`, `{}`, `[]`, `""`,
//!   `0`) to `{}` while truthy non-dicts pass through unchanged
//!   (`events.py:10`).
//! * `dispatch_waiting` appends every selected id to the publish list without
//!   checking the lease update's row count (`dispatch.py:41-45`).
//! * The transient check re-queries `enrolled.exists()` and the available path
//!   re-queries `online_managed_runner` instead of reusing the availability
//!   verdicts (`creation.py:117,123`).
//! * `ORDER BY last_heartbeat_at DESC` sorts never-heartbeated (`NULL`)
//!   enrolled runners first (Postgres `DESC` nulls-first); Django emits the
//!   same bare `DESC` (`creation.py:125`).
//!
//! [`AdmissionCache`][pidash_services]: pidash_services::dispatch::AdmissionCache

use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde_json::{Map, Value};
use sqlx::PgPool;
use uuid::Uuid;

use pidash_db::config::{CloudAgentSettings, ManagedRunnerSettings};
use pidash_db::dispatch::event::KIND_MAX_LENGTH;
use pidash_db::tx::Transaction;
use pidash_services::dispatch::HEARTBEAT_GRACE_SECS;
use pidash_services::dispatch::{
    build_tool_plan, enforce_creation_rate, managed_runner_availability, resolve_executor_kind,
    user_has_llm_config, AdmissionCache, CloudAgentAdmissionError, CloudAgentUnavailable,
    CloudCapabilityUnavailable, DeferredConsume, LlmProfile, ResolveExecutorError, UserFlags,
    ENROLLED_MANAGED_RUNNERS_EXISTS_SQL, GITHUB_BINDING_EXISTS_SQL, ONLINE_MANAGED_RUNNER_SQL,
};
use pidash_types::dispatch::{AgentExecutorKind, ManagedRunnerReason, ManagedRunnerUnavailable};

use crate::celery::CeleryTaskMessage;
use crate::queue::{enqueue_exec, NewJob};

// ---------------------------------------------------------------------------
// Celery wire: `run_cloud_agent.delay(run_id)` (`tasks.py:75-82`)
// ---------------------------------------------------------------------------

/// The cloud execution task (`@shared_task(name=...)`, `tasks.py:76`).
/// Owned here as the publisher; the body is L7's (PIDASHCONV-488), which
/// registers the local handler and flips ownership — until then the worker
/// forwards these rows to the Python plane.
pub const RUN_CLOUD_AGENT_TASK: &str = "cloud_agent.run_agent_run";

/// The `.delay(run_id)` message (`dispatch.py:56`): one positional arg — the
/// hyphenated run id (`dispatch.py:45` appends `str(run.pk)`) — and no kwargs.
/// The worker forward path rebuilds this exact v2 body from the queue row,
/// whichever plane serves the task.
pub fn run_cloud_agent_message(run_id: &str) -> CeleryTaskMessage {
    CeleryTaskMessage::new(
        RUN_CLOUD_AGENT_TASK,
        vec![Value::String(run_id.to_owned())],
        Map::new(),
    )
}

/// Lift the `.delay(run_id)` message into its queue row: `args=[run_id]` with
/// empty kwargs, so the forward path rebuilds the identical Celery v2 body.
/// Retry policy is the [`NewJob::new`] default (`max_retries = 3`, 180s
/// delay): the row budget governs *delivery* to a worker, while the task's
/// own `max_retries=0` (`tasks.py:78`) governs the Python body after delivery.
pub fn run_cloud_agent_job(run_id: &str) -> NewJob {
    let message = run_cloud_agent_message(run_id);
    NewJob::new(
        message.task.clone(),
        Value::Array(message.args.clone()),
        Value::Object(message.kwargs.clone()),
    )
}

// ---------------------------------------------------------------------------
// Pure cores
// ---------------------------------------------------------------------------

/// Offer capacity (`dispatch.py:31`): `max(0, MAX_RUNNING - running - offered)`.
pub fn dispatch_capacity(max_running: i64, running: i64, offered: i64) -> i64 {
    (max_running - running - offered).max(0)
}

/// Lease expiry for an offered run (`dispatch.py:40`): `now + LEASE_SECONDS`.
pub fn lease_expiry_at(now: DateTime<Utc>, lease_secs: i64) -> DateTime<Utc> {
    now + ChronoDuration::seconds(lease_secs)
}

/// `kind[:64]` (`events.py:24`): code-point truncation, never panicking on a
/// UTF-8 boundary (Semantic traps: slicing).
pub fn truncate_kind(kind: &str) -> String {
    kind.chars().take(KIND_MAX_LENGTH).collect()
}

/// Whether a JSON payload survives `payload or {}` (`events.py:10`).
fn is_truthy(payload: &Value) -> bool {
    match payload {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                int != 0
            } else if let Some(uint) = number.as_u64() {
                uint != 0
            } else {
                number.as_f64().is_some_and(|float| float != 0.0)
            }
        }
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
    }
}

/// `payload or {}` (`events.py:10`): falsy payloads collapse to `{}`, truthy
/// ones — including truthy non-dicts — pass through unchanged.
pub fn coalesce_payload(payload: Option<Value>) -> Value {
    match payload {
        Some(value) if is_truthy(&value) => value,
        _ => Value::Object(Map::new()),
    }
}

/// Where one `events.append` lands (`events.py:19-24`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventCapDecision {
    /// Below the cap: insert the event at `max(seq) + 1`, report `true`.
    Append,
    /// At the cap with room for the marker and none written yet: insert the
    /// `events_truncated` marker at `count + 1`, report `false`.
    TruncatedMarker,
    /// At the cap with the marker already written (or no room for it):
    /// write nothing, report `false`.
    Capped,
}

/// The event-cap state machine (`events.py:19-22`).
///
/// `marker_exists` is consulted only when `count` leaves room for the marker
/// (`count < max(0, limit - 1)`), mirroring the `and` short-circuit — callers
/// must skip the marker query otherwise.
pub fn event_cap_decision(count: i64, max_events: i64, marker_exists: bool) -> EventCapDecision {
    if count < (max_events - 2).max(0) {
        return EventCapDecision::Append;
    }
    if count < (max_events - 1).max(0) && !marker_exists {
        return EventCapDecision::TruncatedMarker;
    }
    EventCapDecision::Capped
}

/// The single `events_truncated` marker kind (`events.py:20-21`).
pub const EVENTS_TRUNCATED_KIND: &str = "events_truncated";

/// `_MANAGED_REFUSAL_DETAIL` (`creation.py:135-145`) with the
/// `.get(reason, ...)` default (`creation.py:132`).
pub fn managed_refusal_detail(reason: &str) -> &'static str {
    match reason {
        "managed_runner_disabled" => "Pi Dash Agent is not enabled on this instance.",
        "desktop_not_connected" => "Open the Pi Dash desktop app on the machine you want this to run on.",
        "llm_config_missing" => {
            "The run creator has no AI provider configured. Configure one in Pi Dash AI settings."
        }
        "gateway_scopes_missing" => "Sign in to Pi Dash again to refresh your AI access.",
        "byok_not_supported_on_desktop" => {
            "Pi Dash Agent on desktop uses OpenHub. Switch your AI provider to OpenHub to run here; \
             Pi Dash AI and the Cloud Agent keep using your own key."
        }
        "no_managed_runner_for_project" => "This project has no Pi Dash Agent on your desktop yet.",
        _ => "Pi Dash Agent is not available",
    }
}

/// The pinned-runner + waiting-marker half of `_managed_execution_fields`
/// (`creation.py:115-131`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ManagedFields {
    /// The selected runner id, if the re-query found one (a lost race pins
    /// nothing — Python assigns the `None` too).
    pub pinned_runner_id: Option<Uuid>,
    /// `desktop_not_connected` on the automatic-waiting path only.
    pub error_code: Option<&'static str>,
}

/// Pure branch core of `_managed_execution_fields` (`creation.py:114-132`).
///
/// `available` / `reason` are the `managed_runner_availability` verdict;
/// `enrolled_exists` is the re-queried transient check (`creation.py:123`,
/// evaluated only for `desktop_not_connected`); `online_id` /
/// `latest_enrolled_id` are the re-queried pins (`creation.py:117,125`).
pub fn managed_execution_decision(
    available: bool,
    reason: &str,
    enrolled_exists: bool,
    online_id: Option<Uuid>,
    latest_enrolled_id: Option<Uuid>,
    automatic: bool,
) -> Result<ManagedFields, ManagedRunnerUnavailable> {
    if available {
        return Ok(ManagedFields {
            pinned_runner_id: online_id,
            error_code: None,
        });
    }
    // "Offline right now" is the only transient failure (`creation.py:120-123`).
    let transient = reason == ManagedRunnerReason::NOT_CONNECTED && enrolled_exists;
    if automatic && transient {
        return Ok(ManagedFields {
            pinned_runner_id: latest_enrolled_id,
            error_code: Some(ManagedRunnerReason::NOT_CONNECTED),
        });
    }
    Err(ManagedRunnerUnavailable::new(
        reason,
        managed_refusal_detail(reason),
    ))
}

/// The captured quota shape (`creation.py:70,175`): `{code, detail}` — the
/// `retry_after_seconds` the raised error carries is dropped on capture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeferredAdmissionError {
    pub code: String,
    pub detail: String,
}

/// Capture an admission refusal the way the automatic paths do
/// (`creation.py:51,70,175`): code + `str(exc)`, no retry-after.
pub fn defer_admission_error(exc: &CloudAgentAdmissionError) -> DeferredAdmissionError {
    DeferredAdmissionError {
        code: exc.code().to_owned(),
        detail: exc.detail().to_owned(),
    }
}

/// Pure verdict core of the workspace queue-cap gate (`creation.py:62-70,
/// 163-176`): `Ok(None)` admits, `Ok(Some(_))` defers for automatic runs,
/// `Err(_)` raises for manual ones.
pub fn quota_verdict(
    queued: i64,
    max_queued_per_workspace: i64,
    dispatch_scan_interval_secs: i64,
    automatic: bool,
) -> Result<Option<DeferredAdmissionError>, CloudAgentAdmissionError> {
    if queued < max_queued_per_workspace {
        return Ok(None);
    }
    let error = CloudAgentAdmissionError::new(
        CloudAgentAdmissionError::RUN_QUOTA_EXCEEDED,
        "Cloud Agent queue is full for this workspace",
        Some(dispatch_scan_interval_secs),
    );
    if automatic {
        return Ok(Some(defer_admission_error(&error)));
    }
    Err(error)
}

// ---------------------------------------------------------------------------
// SQL
// ---------------------------------------------------------------------------

/// The workspace mutex (`creation.py:56`, `creation.py:162`,
/// `dispatch.py:27`): `Workspace.objects.select_for_update().get(pk=...)`.
/// Projects the constant — the row itself is never read, only locked — and a
/// missing row surfaces as `RowNotFound`, as `.get()` raises. The default
/// manager's `deleted_at IS NULL` guard is load-bearing (a soft-deleted
/// workspace raises instead of locking); the `ORDER BY` the compiler adds is
/// unobservable on a primary-key lookup and omitted. Param: `$1` workspace id
/// (uuid).
pub const WORKSPACE_LOCK_SQL: &str =
    "SELECT 1 FROM workspaces WHERE id = $1 AND deleted_at IS NULL FOR UPDATE";

/// The queued-cloud count (`creation.py:57-61,163-167`): `COUNT(*)` in the
/// `filter()` kwarg order. Params: `$1` workspace id (uuid).
pub const QUEUED_CLOUD_COUNT_SQL: &str = "SELECT COUNT(*) FROM agent_run \
    WHERE workspace_id = $1 AND executor_kind = 'cloud_agent' AND status = 'queued'";

/// The running-cloud count (`dispatch.py:29`): the shared `base`
/// (`executor_kind`, `workspace_id`) plus `status = 'running'`. Params: `$1`
/// workspace id (uuid).
pub const RUNNING_CLOUD_COUNT_SQL: &str = "SELECT COUNT(*) FROM agent_run \
    WHERE executor_kind = 'cloud_agent' AND workspace_id = $1 AND status = 'running'";

/// The offered count (`dispatch.py:30`): queued rows whose lease is still in
/// the future. Params: `$1` workspace id (uuid), `$2` now (timestamptz).
pub const OFFERED_COUNT_SQL: &str = "SELECT COUNT(*) FROM agent_run \
    WHERE executor_kind = 'cloud_agent' AND workspace_id = $1 AND status = 'queued' \
    AND lease_expires_at > $2";

/// The due set (`dispatch.py:34-39`): queued, unlocked-or-expired, oldest
/// first, up to capacity, skipping rows a concurrent dispatcher locked.
/// Projects the id only (`dispatch.py:45` consumes `run.pk`). Django renders
/// the locking clause after `LIMIT`; the `Q` span keeps its parens. Params:
/// `$1` workspace id (uuid), `$2` now (timestamptz), `$3` capacity (bigint).
pub const DUE_RUNS_SQL: &str = "SELECT id FROM agent_run \
    WHERE executor_kind = 'cloud_agent' AND workspace_id = $1 AND status = 'queued' \
    AND (lease_expires_at IS NULL OR lease_expires_at <= $2) \
    ORDER BY created_at LIMIT $3 FOR UPDATE SKIP LOCKED";

/// The per-row lease (`dispatch.py:42-44`): `SET` order mirrors the
/// `.update()` kwargs; the `status = 'queued'` guard mirrors the re-filter.
/// Params: `$1` lease expiry (timestamptz), `$2` run id (uuid).
pub const LEASE_RUN_SQL: &str = "UPDATE agent_run SET lease_expires_at = $1, \
    dispatch_attempts = dispatch_attempts + 1 WHERE id = $2 AND status = 'queued'";

/// The single-run read (`dispatch.py:62`): `.only(...)` in call order (column
/// order is unobservable — rows are read by name), `.first()` as `LIMIT 1`.
/// Param: `$1` run id (uuid).
pub const DISPATCH_RUN_SELECT_SQL: &str =
    "SELECT id, status, executor_kind, pod_id, workspace_id FROM agent_run WHERE id = $1 LIMIT 1";

/// The event-parent lock (`events.py:13`): `.only("id")` projected exactly —
/// a queryset cannot lock the empty event set, so the parent is locked
/// instead. A missing row surfaces as `RowNotFound`, as `.get()` raises.
/// Param: `$1` run id (uuid).
pub const AGENT_RUN_LOCK_SQL: &str = "SELECT id FROM agent_run WHERE id = $1 FOR UPDATE";

/// The event count (`events.py:15`): `.count()` over the filtered set.
/// The compiler's select form joins `agent_run` (the `Meta.ordering`
/// `["agent_run", "seq"]` orders by the related row), but the join filters
/// nothing — the FK is database-enforced — and `.count()` clears the
/// ordering, so the plain `COUNT(*)` is exactly equivalent. Param: `$1` run
/// id (uuid).
pub const EVENT_COUNT_SQL: &str = "SELECT COUNT(*) FROM agent_run_event WHERE agent_run_id = $1";

/// The marker probe (`events.py:20`): `.exists()` as `LIMIT 1` (same
/// join-is-a-no-op argument as [`EVENT_COUNT_SQL`]). Param: `$1` run id
/// (uuid).
pub const EVENT_MARKER_EXISTS_SQL: &str = "SELECT 1 FROM agent_run_event \
    WHERE agent_run_id = $1 AND kind = 'events_truncated' LIMIT 1";

/// The current top sequence (`events.py:23`): `.order_by("-seq")` +
/// `.first()`. Param: `$1` run id (uuid).
pub const EVENT_MAX_SEQ_SQL: &str =
    "SELECT seq FROM agent_run_event WHERE agent_run_id = $1 ORDER BY seq DESC LIMIT 1";

/// The event insert (`events.py:21,24`): both `.create()` sites share one
/// shape. `id` (BigAutoField) is the only database default; `created_at`
/// (`auto_now_add`) is Django-side, so Rust inserts supply it explicitly (the
/// L2 application-defaults rule). Params: `$1` run id (uuid), `$2` seq
/// (int), `$3` kind (text), `$4` payload (jsonb), `$5` created_at
/// (timestamptz).
pub const EVENT_INSERT_SQL: &str = "INSERT INTO agent_run_event \
    (agent_run_id, seq, kind, payload, created_at) VALUES ($1, $2, $3, $4, $5)";

/// The automatic-waiting pin (`creation.py:125`):
/// `enrolled_managed_runners(...).order_by("-last_heartbeat_at").first()`.
/// Same `WHERE` shape as L4's [`ENROLLED_MANAGED_RUNNERS_EXISTS_SQL`] plus
/// the bare `DESC` — which sorts never-heartbeated (`NULL`) runners first,
/// exactly as Django does — projecting the id (the pin handle the creation
/// sites pass as `pinned_runner`). Params: `$1` owner id (uuid), `$2`
/// project id (uuid), `$3` workspace id (uuid).
pub const ENROLLED_LATEST_RUNNER_SQL: &str = "SELECT runner.id FROM runner INNER JOIN pod \
    ON pod.id = runner.pod_id WHERE runner.owner_id = $1 \
    AND runner.provisioning = 'desktop_bundled' AND pod.project_id = $2 \
    AND runner.workspace_id = $3 AND runner.revoked_at IS NULL \
    ORDER BY runner.last_heartbeat_at DESC LIMIT 1";

// ---------------------------------------------------------------------------
// Creation seam (`creation.py:11-89`)
// ---------------------------------------------------------------------------

/// The project half of `execution_fields` (`creation.py:14,19,26`): the pod
/// scope the executor resolves under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectScope {
    /// `project.id`.
    pub project_id: Uuid,
    /// `project.workspace_id`.
    pub workspace_id: Uuid,
    /// `project.default_agent_executor` (inherited when `requested` is
    /// `None` or `""`).
    pub default_agent_executor: String,
}

/// The actor half of `execution_fields` (`creation.py:17`): `None` is the
/// anonymous caller (`getattr(actor, "id", None)` → `None`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActorScope {
    /// `actor.id`.
    pub id: Uuid,
    /// `is_active` / `is_bot` for the LLM-config gate.
    pub flags: UserFlags,
}

/// The inputs `execution_fields` reads off its caller (`creation.py:11-20`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionInputs<'a> {
    /// The project the run is created on.
    pub project: &'a ProjectScope,
    /// `run_kind` (`"issue"`, `"scheduler"`, …).
    pub run_kind: &'a str,
    /// Whether the run binds a work item.
    pub has_issue: bool,
    /// `required_capabilities` (`()` by default).
    pub required_capabilities: &'a [&'a str],
    /// The run creator (`None` for the anonymous caller).
    pub actor: Option<&'a ActorScope>,
    /// Ticker/scheduler runs wait visibly instead of raising on soft gates.
    pub automatic: bool,
    /// The per-issue execution-target override (`Issue.agent_executor`);
    /// `None` inherits the project default.
    pub requested: Option<&'a str>,
    /// `int(time.time())` for the admission buckets.
    pub now_unix_secs: i64,
}

/// The EE-overlayable seams `execution_fields` consults, each only when the
/// Python short-circuit reaches it.
pub struct ExecutionSeams<F, E, P> {
    /// `has_usable_llm_config(user)` (`agent_execution.py:78-80`): consulted
    /// only for a present, active, non-bot actor on the cloud path.
    pub has_usable_llm_config: F,
    /// `extra_toolsets_enabled_for(creator)` (`toolsets.py:23-32`): consulted
    /// only when a creator is present.
    pub extra_toolsets_enabled: E,
    /// `managed_llm_profile(user)` (`managed_runner/policy.py:27-36`):
    /// consulted only when the managed path reaches the profile gate.
    pub llm_profile: P,
}

/// The executor-specific `AgentRun` fields for one run creation
/// (`execution_fields`, `creation.py:11-89`).
///
/// Consumption mirrors the orchestration call sites: `executor_kind` +
/// `tool_plan` (+ `error_code` when set) splat into `objects.create`;
/// `pinned_runner_id` passes explicitly (the sites `pop` it with a default);
/// `cloud_admission_error` is popped before the splat — `_cloud_admission_error`
/// is a pseudo-key, not an `AgentRun` column (`creation.py:84-85`).
#[derive(Debug, Clone, PartialEq)]
pub struct ExecutionFields {
    /// The resolved executor kind.
    pub executor_kind: AgentExecutorKind,
    /// The tool plan (`ToolPlan` JSON on the cloud path, `{}` elsewhere).
    pub tool_plan: Value,
    /// The pin: always `None` on the cloud/local paths, the selected runner
    /// on the managed path.
    pub pinned_runner_id: Option<Uuid>,
    /// `desktop_not_connected` on the managed automatic-waiting path only —
    /// a real `AgentRun` column the sweep reads back (`creation.py:126-131`).
    pub error_code: Option<String>,
    /// The deferred quota refusal on the automatic cloud path (not a column).
    pub cloud_admission_error: Option<DeferredAdmissionError>,
}

/// Every failure `execution_fields` and [`lock_cloud_creation_capacity`]
/// report: the Python raises (`ValueError` / `RuntimeError` at the
/// orchestration call sites) plus the database.
#[derive(Debug, thiserror::Error)]
pub enum ExecutionFieldsError {
    /// `resolve_executor_kind` refused (`creation.py:26`): unknown executor,
    /// cloud not configured, or managed disabled at the instance level.
    #[error(transparent)]
    Resolve(#[from] ResolveExecutorError),
    /// The creator has no LLM config (`creation.py:36-39`).
    #[error(transparent)]
    Unavailable(CloudAgentUnavailable),
    /// A manual gate refused (`creation.py:49-50,68-69`).
    #[error(transparent)]
    Admission(#[from] CloudAgentAdmissionError),
    /// A managed gate refused (`creation.py:132`).
    #[error(transparent)]
    Managed(#[from] ManagedRunnerUnavailable),
    /// A required capability is not plannable (`creation.py:73-81` via
    /// `build_tool_plan`).
    #[error(transparent)]
    Capability(#[from] CloudCapabilityUnavailable),
    /// The database failed.
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
}

async fn lock_workspace(
    executor: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    workspace_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query_scalar::<_, i32>(WORKSPACE_LOCK_SQL)
        .bind(workspace_id)
        .fetch_one(executor)
        .await?;
    Ok(())
}

async fn count_queued_cloud(
    executor: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    workspace_id: Uuid,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar::<_, i64>(QUEUED_CLOUD_COUNT_SQL)
        .bind(workspace_id)
        .fetch_one(executor)
        .await
}

async fn github_binding_exists(pool: &PgPool, project: &ProjectScope) -> Result<bool, sqlx::Error> {
    let found = sqlx::query_scalar::<_, i32>(GITHUB_BINDING_EXISTS_SQL)
        .bind(project.project_id)
        .bind(project.workspace_id)
        .fetch_optional(pool)
        .await?;
    Ok(found.is_some())
}

async fn enrolled_exists(
    pool: &PgPool,
    project: &ProjectScope,
    owner_id: Uuid,
) -> Result<bool, sqlx::Error> {
    let found = sqlx::query_scalar::<_, i32>(ENROLLED_MANAGED_RUNNERS_EXISTS_SQL)
        .bind(owner_id)
        .bind(project.project_id)
        .bind(project.workspace_id)
        .fetch_optional(pool)
        .await?;
    Ok(found.is_some())
}

async fn online_managed_runner_id(
    pool: &PgPool,
    project: &ProjectScope,
    owner_id: Uuid,
    heartbeat_threshold: DateTime<Utc>,
) -> Result<Option<Uuid>, sqlx::Error> {
    sqlx::query_scalar::<_, Uuid>(ONLINE_MANAGED_RUNNER_SQL)
        .bind(owner_id)
        .bind(project.project_id)
        .bind(project.workspace_id)
        .bind(heartbeat_threshold)
        .fetch_optional(pool)
        .await
}

async fn latest_enrolled_runner_id(
    pool: &PgPool,
    project: &ProjectScope,
    owner_id: Uuid,
) -> Result<Option<Uuid>, sqlx::Error> {
    sqlx::query_scalar::<_, Uuid>(ENROLLED_LATEST_RUNNER_SQL)
        .bind(owner_id)
        .bind(project.project_id)
        .bind(project.workspace_id)
        .fetch_optional(pool)
        .await
}

/// Resolve the executor-specific `AgentRun` fields for one run creation
/// (`execution_fields`, `creation.py:11-89`).
///
/// The capacity gate runs in its own short transaction (the inner `atomic`
/// at `creation.py:55-70`, which commits before the fields are built); the
/// deferred admission consumes collect into `on_commit` and the caller runs
/// them after commit — a rolled-back creation burns no quota, while consumes
/// registered before a later raise stay registered, as on Django's ambient
/// transaction.
#[allow(clippy::too_many_arguments)]
pub async fn execution_fields<C, F, E, P>(
    pool: &PgPool,
    inputs: &ExecutionInputs<'_>,
    cloud: &CloudAgentSettings,
    managed: &ManagedRunnerSettings,
    cache: &C,
    on_commit: &mut dyn FnMut(DeferredConsume),
    seams: ExecutionSeams<F, E, P>,
) -> Result<ExecutionFields, ExecutionFieldsError>
where
    C: AdmissionCache,
    F: FnOnce() -> bool,
    E: FnOnce() -> bool,
    P: FnOnce() -> LlmProfile,
{
    let executor = resolve_executor_kind(
        inputs.requested,
        &inputs.project.default_agent_executor,
        cloud,
        managed,
    )?;
    if executor == AgentExecutorKind::CloudAgent {
        return execution_fields_cloud(pool, inputs, cloud, cache, on_commit, seams).await;
    }
    if executor == AgentExecutorKind::ManagedRunner {
        return execution_fields_managed(pool, inputs, managed, seams.llm_profile).await;
    }
    Ok(ExecutionFields {
        executor_kind: executor,
        tool_plan: Value::Object(Map::new()),
        pinned_runner_id: None,
        error_code: None,
        cloud_admission_error: None,
    })
}

async fn execution_fields_cloud<C, F, E, P>(
    pool: &PgPool,
    inputs: &ExecutionInputs<'_>,
    cloud: &CloudAgentSettings,
    cache: &C,
    on_commit: &mut dyn FnMut(DeferredConsume),
    seams: ExecutionSeams<F, E, P>,
) -> Result<ExecutionFields, ExecutionFieldsError>
where
    C: AdmissionCache,
    F: FnOnce() -> bool,
    E: FnOnce() -> bool,
    P: FnOnce() -> LlmProfile,
{
    let actor_flags = inputs.actor.map(|actor| &actor.flags);
    // A run without a funded principal can never start (`creation.py:33-39`).
    if !user_has_llm_config(actor_flags, seams.has_usable_llm_config) {
        return Err(ExecutionFieldsError::Unavailable(
            CloudAgentUnavailable::new(
                "The run creator has no AI provider configured. Configure one in Pi Dash AI settings.",
            ),
        ));
    }
    let actor_id = inputs.actor.map(|actor| actor.id.to_string());
    let mut admission_error = None;
    if let Err(exc) = enforce_creation_rate(
        cloud,
        inputs.now_unix_secs,
        &inputs.project.workspace_id.to_string(),
        actor_id.as_deref(),
        inputs.automatic,
        cache,
        on_commit,
    ) {
        if !inputs.automatic {
            return Err(ExecutionFieldsError::Admission(exc));
        }
        admission_error = Some(defer_admission_error(&exc));
    }
    // The point-in-time gate: the inner atomic commits (releasing the lock)
    // before the fields are built; the insertion transaction re-checks via
    // `lock_cloud_creation_capacity` (`creation.py:52-70`).
    let queued = {
        let mut gate = Transaction::begin(pool).await?;
        lock_workspace(&mut **gate.inner(), inputs.project.workspace_id).await?;
        let queued = count_queued_cloud(&mut **gate.inner(), inputs.project.workspace_id).await?;
        gate.commit().await?;
        queued
    };
    match quota_verdict(
        queued,
        cloud.max_queued_per_workspace,
        cloud.dispatch_scan_interval_secs,
        inputs.automatic,
    )? {
        None => {}
        Some(deferred) => admission_error = Some(deferred),
    }
    // The github leg runs only with the kill switch on (the `policy.py:119`
    // short-circuit); the project is always present here.
    let github_available = if cloud.github_tools_enabled {
        Some(github_binding_exists(pool, inputs.project).await?)
    } else {
        None
    };
    // The run executes as its creator, so their preference is what the
    // snapshot records (`creation.py:78-80`); no creator, no seam consult.
    let extra_toolsets = if inputs.actor.is_some() {
        Some(seams.extra_toolsets_enabled)
    } else {
        None
    };
    let plan = build_tool_plan(
        inputs.run_kind,
        inputs.has_issue,
        inputs.required_capabilities,
        github_available.map(|available| move || available),
        extra_toolsets,
        cloud,
    )?;
    Ok(ExecutionFields {
        executor_kind: AgentExecutorKind::CloudAgent,
        tool_plan: serde_json::to_value(&plan).expect("ToolPlan serializes to JSON"),
        pinned_runner_id: None,
        error_code: None,
        cloud_admission_error: admission_error,
    })
}

async fn execution_fields_managed<P>(
    pool: &PgPool,
    inputs: &ExecutionInputs<'_>,
    managed: &ManagedRunnerSettings,
    llm_profile: P,
) -> Result<ExecutionFields, ExecutionFieldsError>
where
    P: FnOnce() -> LlmProfile,
{
    // The availability verdicts (`creation.py:114`); an absent viewer
    // short-circuits inside `managed_runner_availability` before any seam or
    // verdict is read, so no query runs for it.
    let (enrolled, online) = match inputs.actor {
        Some(actor) => {
            let enrolled = enrolled_exists(pool, inputs.project, actor.id).await?;
            let threshold = Utc::now() - ChronoDuration::seconds(HEARTBEAT_GRACE_SECS);
            let online = online_managed_runner_id(pool, inputs.project, actor.id, threshold)
                .await?
                .is_some();
            (enrolled, online)
        }
        None => (false, false),
    };
    let verdict = managed_runner_availability(
        managed,
        inputs.actor.map(|actor| &actor.flags),
        llm_profile,
        enrolled,
        online,
    );
    // The re-queries (`creation.py:117,123,125`): the available path and the
    // transient check re-read rather than reuse the verdicts.
    let online_id = if verdict.available {
        match inputs.actor {
            Some(actor) => {
                let threshold = Utc::now() - ChronoDuration::seconds(HEARTBEAT_GRACE_SECS);
                online_managed_runner_id(pool, inputs.project, actor.id, threshold).await?
            }
            None => None,
        }
    } else {
        None
    };
    let transient_enrolled = verdict.reason_code == ManagedRunnerReason::NOT_CONNECTED
        && match inputs.actor {
            Some(actor) => enrolled_exists(pool, inputs.project, actor.id).await?,
            None => false,
        };
    let latest_enrolled_id = if inputs.automatic && transient_enrolled {
        match inputs.actor {
            Some(actor) => latest_enrolled_runner_id(pool, inputs.project, actor.id).await?,
            None => None,
        }
    } else {
        None
    };
    let fields = managed_execution_decision(
        verdict.available,
        &verdict.reason_code,
        transient_enrolled,
        online_id,
        latest_enrolled_id,
        inputs.automatic,
    )?;
    Ok(ExecutionFields {
        executor_kind: AgentExecutorKind::ManagedRunner,
        tool_plan: Value::Object(Map::new()),
        pinned_runner_id: fields.pinned_runner_id,
        error_code: fields.error_code.map(str::to_string),
        cloud_admission_error: None,
    })
}

/// Repeat hard admission while holding the caller's insertion transaction
/// (`lock_cloud_creation_capacity`, `creation.py:155-177`).
///
/// Non-cloud executors return `Ok(None)` without touching the database.
/// Over capacity, automatic runs defer (`Ok(Some(_))`) and manual runs raise —
/// and the caller folds the deferral over any earlier capture
/// (`lock(...) or admission_error` at the orchestration sites).
pub async fn lock_cloud_creation_capacity(
    tx: &mut Transaction<'_>,
    workspace_id: Uuid,
    executor_kind: &AgentExecutorKind,
    automatic: bool,
    cloud: &CloudAgentSettings,
) -> Result<Option<DeferredAdmissionError>, ExecutionFieldsError> {
    if *executor_kind != AgentExecutorKind::CloudAgent {
        return Ok(None);
    }
    lock_workspace(&mut **tx.inner(), workspace_id).await?;
    let queued = count_queued_cloud(&mut **tx.inner(), workspace_id).await?;
    Ok(quota_verdict(
        queued,
        cloud.max_queued_per_workspace,
        cloud.dispatch_scan_interval_secs,
        automatic,
    )?
    .into_iter()
    .next())
}

// ---------------------------------------------------------------------------
// Dispatch engine (`dispatch.py:20-70`)
// ---------------------------------------------------------------------------

/// One `agent_run` row as `dispatch_agent_run` reads it (`dispatch.py:62`).
#[derive(Debug, Clone, sqlx::FromRow)]
struct DispatchRunRow {
    // Selected (`.only("id", …)`) but unread, as in Python.
    #[allow(dead_code)]
    id: Uuid,
    status: String,
    executor_kind: String,
    pod_id: Uuid,
    workspace_id: Uuid,
}

/// What `dispatch_agent_run` did with one run (`dispatch.py:61-70`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispatchDecision {
    /// Missing or not queued: nothing happened.
    Ignored,
    /// A cloud run: the workspace dispatch ran and offered this many rows.
    CloudDispatched {
        /// The workspace that was dispatched.
        workspace_id: Uuid,
        /// Rows leased and enqueued.
        offered: usize,
    },
    /// A machine-executor run: the runner plane must drain this pod
    /// (`drain_pod_by_id`, runner-owned, D-13+).
    PodDrain {
        /// The pod to drain.
        pod_id: Uuid,
    },
}

/// Lease and offer the oldest queued rows up to workspace capacity
/// (`dispatch_waiting`, `dispatch.py:20-48`).
///
/// Disabled instances return `0` without touching the database. Otherwise the
/// lease updates and the fan-out enqueue commit atomically: one
/// [`run_cloud_agent_job`] row per offered id joins the same transaction, so a
/// rollback retracts the offer entirely — the transactional form of
/// `transaction.on_commit(lambda: _publish(ids))` (`dispatch.py:47`).
pub async fn dispatch_waiting(
    pool: &PgPool,
    cloud: &CloudAgentSettings,
    workspace_id: Uuid,
    now: DateTime<Utc>,
) -> Result<usize, sqlx::Error> {
    if !cloud.enabled {
        return Ok(0);
    }
    let mut tx = Transaction::begin(pool).await?;
    lock_workspace(&mut **tx.inner(), workspace_id).await?;
    let running = sqlx::query_scalar::<_, i64>(RUNNING_CLOUD_COUNT_SQL)
        .bind(workspace_id)
        .fetch_one(&mut **tx.inner())
        .await?;
    let offered = sqlx::query_scalar::<_, i64>(OFFERED_COUNT_SQL)
        .bind(workspace_id)
        .bind(now)
        .fetch_one(&mut **tx.inner())
        .await?;
    let capacity = dispatch_capacity(cloud.max_running_per_workspace, running, offered);
    if capacity == 0 {
        tx.commit().await?;
        return Ok(0);
    }
    let due: Vec<Uuid> = sqlx::query_scalar::<_, Uuid>(DUE_RUNS_SQL)
        .bind(workspace_id)
        .bind(now)
        .bind(capacity)
        .fetch_all(&mut **tx.inner())
        .await?;
    let lease = lease_expiry_at(now, cloud.dispatch_lease_secs);
    let mut ids = Vec::with_capacity(due.len());
    for run_id in &due {
        // No row-count check: the id is offered even if a concurrent claim
        // stole the row first (`dispatch.py:41-45`).
        sqlx::query(LEASE_RUN_SQL)
            .bind(lease)
            .bind(run_id)
            .execute(&mut **tx.inner())
            .await?;
        let id = run_id.to_string();
        enqueue_exec(&mut **tx.inner(), &run_cloud_agent_job(&id)).await?;
        ids.push(id);
    }
    tx.commit().await?;
    Ok(ids.len())
}

/// Dispatch one run (`dispatch_agent_run`, `dispatch.py:61-70`).
///
/// Missing or non-queued rows are ignored. Cloud runs dispatch their
/// workspace; every other executor kind drains its pod on the runner plane
/// (returned as [`DispatchDecision::PodDrain`] — `drain_pod_by_id` is
/// runner-owned and intentionally not inlined here).
pub async fn dispatch_agent_run(
    pool: &PgPool,
    cloud: &CloudAgentSettings,
    run_id: Uuid,
    now: DateTime<Utc>,
) -> Result<DispatchDecision, sqlx::Error> {
    let run = sqlx::query_as::<_, DispatchRunRow>(DISPATCH_RUN_SELECT_SQL)
        .bind(run_id)
        .fetch_optional(pool)
        .await?;
    let Some(run) = run else {
        return Ok(DispatchDecision::Ignored);
    };
    if run.status != "queued" {
        return Ok(DispatchDecision::Ignored);
    }
    if AgentExecutorKind::from_value(&run.executor_kind) == Some(AgentExecutorKind::CloudAgent) {
        let offered = dispatch_waiting(pool, cloud, run.workspace_id, now).await?;
        return Ok(DispatchDecision::CloudDispatched {
            workspace_id: run.workspace_id,
            offered,
        });
    }
    Ok(DispatchDecision::PodDrain { pod_id: run.pod_id })
}

/// Dispatch one run after the surrounding creation commits
/// (`dispatch_after_commit`, `creation.py:148-152`):
/// `transaction.on_commit(lambda: dispatch_agent_run(run_id))`.
///
/// The run id collects into the caller's `on_commit` outbox (the
/// `DeferredConsume` precedent); the caller drains it after commit by awaiting
/// [`dispatch_agent_run`] per id. Nothing executes inline — provably: this
/// function performs no I/O.
pub fn dispatch_after_commit(on_commit: &mut dyn FnMut(Uuid), run_id: Uuid) {
    on_commit(run_id);
}

// ---------------------------------------------------------------------------
// Run events (`events.py:9-25`)
// ---------------------------------------------------------------------------

/// Append one bounded, sanitized semantic event (`events.append`,
/// `events.py:9-25`).
///
/// Owns its transaction (every call site invokes it outside any open
/// transaction). The parent run row is locked because a queryset cannot lock
/// the empty event set; one row each is reserved for the mandatory terminal
/// event and the truncation marker, so the configured maximum includes both.
/// `now` stamps `created_at` (Django's `auto_now_add`, supplied explicitly).
/// Returns whether the event was recorded.
pub async fn append_event(
    pool: &PgPool,
    max_events: i64,
    run_id: Uuid,
    kind: &str,
    payload: Option<Value>,
    now: DateTime<Utc>,
) -> Result<bool, sqlx::Error> {
    let mut tx = Transaction::begin(pool).await?;
    sqlx::query_scalar::<_, Uuid>(AGENT_RUN_LOCK_SQL)
        .bind(run_id)
        .fetch_one(&mut **tx.inner())
        .await?;
    let count = sqlx::query_scalar::<_, i64>(EVENT_COUNT_SQL)
        .bind(run_id)
        .fetch_one(&mut **tx.inner())
        .await?;
    // The marker probe runs only when the count leaves room for the marker
    // (the `and` short-circuit at `events.py:20`); above that the probe value
    // is irrelevant to the decision.
    let marker_threshold = (max_events - 1).max(0);
    let marker_exists = if count < marker_threshold {
        sqlx::query_scalar::<_, i32>(EVENT_MARKER_EXISTS_SQL)
            .bind(run_id)
            .fetch_optional(&mut **tx.inner())
            .await?
            .is_some()
    } else {
        false
    };
    let recorded = match event_cap_decision(count, max_events, marker_exists) {
        EventCapDecision::Append => {
            let top = sqlx::query_scalar::<_, i32>(EVENT_MAX_SEQ_SQL)
                .bind(run_id)
                .fetch_optional(&mut **tx.inner())
                .await?;
            let seq = top.unwrap_or(0) + 1;
            sqlx::query(EVENT_INSERT_SQL)
                .bind(run_id)
                .bind(seq)
                .bind(truncate_kind(kind))
                .bind(coalesce_payload(payload))
                .bind(now)
                .execute(&mut **tx.inner())
                .await?;
            true
        }
        EventCapDecision::TruncatedMarker => {
            sqlx::query(EVENT_INSERT_SQL)
                .bind(run_id)
                .bind(count as i32 + 1)
                .bind(EVENTS_TRUNCATED_KIND)
                .bind(Value::Object(Map::new()))
                .bind(now)
                .execute(&mut **tx.inner())
                .await?;
            false
        }
        EventCapDecision::Capped => false,
    };
    tx.commit().await?;
    Ok(recorded)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::rc::Rc;

    static FIXTURE: &str =
        include_str!("../../../../fixtures/dispatch/fx-disp-06-dispatch.golden.json");

    fn fixture() -> Value {
        serde_json::from_str(FIXTURE).expect("fixture parses")
    }

    fn django_cloud_settings() -> CloudAgentSettings {
        CloudAgentSettings {
            enabled: false,
            writes_enabled: false,
            github_tools_enabled: true,
            disabled_tools: Vec::new(),
            reconcile_interval_secs: 30,
            model_request_timeout_secs: 60,
            execution_timeout_secs: 285,
            run_soft_limit_secs: 300,
            run_hard_limit_secs: 330,
            stale_grace_secs: 60,
            dispatch_lease_secs: 60,
            dispatch_backoff_secs: 10,
            dispatch_scan_interval_secs: 10,
            sweep_interval_secs: 30,
            dispatch_scan_batch: 100,
            max_queue_age_secs: 900,
            model_request_limit: 25,
            tool_call_limit: 20,
            write_call_limit: 3,
            input_token_limit: 144_000,
            output_token_limit: 16_000,
            total_token_limit: 160_000,
            max_output_tokens_per_request: 4096,
            max_queued_per_workspace: 20,
            max_running_per_workspace: 2,
            user_creation_rate_per_minute: 6,
            workspace_creation_rate_per_minute: 30,
            tool_timeout_secs: 20,
            max_tool_result_bytes: 65536,
            max_prompt_bytes: 262_144,
            max_final_result_bytes: 65536,
            max_events: 500,
            block_private_urls: true,
        }
    }

    fn django_managed_settings() -> ManagedRunnerSettings {
        ManagedRunnerSettings {
            enabled: false,
            max_per_user_project: 1,
            queued_max_age_secs: 43200,
            graceful_stop_secs: 30,
            sweep_interval_secs: 300,
            desktop_min_version: String::new(),
        }
    }

    #[derive(Debug)]
    struct CacheError(String);

    impl std::fmt::Display for CacheError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "cache error: {}", self.0)
        }
    }

    impl std::error::Error for CacheError {}

    /// In-memory [`AdmissionCache`]: preset counts, recorded consumes.
    #[derive(Default)]
    struct FakeCache {
        counts: HashMap<String, i64>,
        down: bool,
        consumed: Rc<RefCell<Vec<(String, i64)>>>,
    }

    impl AdmissionCache for FakeCache {
        type Error = CacheError;

        fn bucket_count(&self, key: &str) -> Result<Option<i64>, Self::Error> {
            if self.down {
                return Err(CacheError("down".to_owned()));
            }
            Ok(self.counts.get(key).copied())
        }

        fn add_or_incr(&self, key: &str, timeout_secs: i64) -> Result<(), Self::Error> {
            self.consumed
                .borrow_mut()
                .push((key.to_owned(), timeout_secs));
            Ok(())
        }
    }

    /// A pool that never connects (no test below issues a query through it —
    /// any attempt fails loudly instead of hanging).
    fn lazy_pool() -> PgPool {
        sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://localhost:1/dispatch-no-db")
            .expect("lazy pool builds")
    }

    fn project(default: &str) -> ProjectScope {
        ProjectScope {
            project_id: Uuid::new_v4(),
            workspace_id: Uuid::new_v4(),
            default_agent_executor: default.to_owned(),
        }
    }

    fn actor(active: bool, bot: bool) -> ActorScope {
        ActorScope {
            id: Uuid::new_v4(),
            flags: UserFlags {
                is_active: active,
                is_bot: bot,
            },
        }
    }

    fn panic_seams(
    ) -> ExecutionSeams<impl FnOnce() -> bool, impl FnOnce() -> bool, impl FnOnce() -> LlmProfile>
    {
        ExecutionSeams {
            has_usable_llm_config: || panic!("llm seam must not be consulted"),
            extra_toolsets_enabled: || panic!("toolsets seam must not be consulted"),
            llm_profile: || panic!("profile seam must not be consulted"),
        }
    }

    // -- fixture: refusal map ------------------------------------------------

    #[test]
    fn managed_refusal_details_match_fixture_verbatim() {
        let details = &fixture()["managed_refusal_detail"];
        for (reason, expected) in [
            ("managed_runner_disabled", "Pi Dash Agent is not enabled on this instance."),
            (
                "desktop_not_connected",
                "Open the Pi Dash desktop app on the machine you want this to run on.",
            ),
            (
                "llm_config_missing",
                "The run creator has no AI provider configured. Configure one in Pi Dash AI settings.",
            ),
            (
                "gateway_scopes_missing",
                "Sign in to Pi Dash again to refresh your AI access.",
            ),
            (
                "byok_not_supported_on_desktop",
                "Pi Dash Agent on desktop uses OpenHub. Switch your AI provider to OpenHub to run here; \
                 Pi Dash AI and the Cloud Agent keep using your own key.",
            ),
            (
                "no_managed_runner_for_project",
                "This project has no Pi Dash Agent on your desktop yet.",
            ),
        ] {
            assert_eq!(details[reason], expected, "fixture row {reason}");
            assert_eq!(managed_refusal_detail(reason), expected, "port {reason}");
        }
        // The `.get(reason, …)` default (`creation.py:132`) has no fixture row.
        assert_eq!(
            managed_refusal_detail("something_new"),
            "Pi Dash Agent is not available"
        );
    }

    // -- execution_fields: paths that raise before any database read --------

    #[tokio::test]
    async fn local_path_returns_bare_fields_without_consulting_anything() {
        let pool = lazy_pool();
        let project = project("local_runner");
        let actor = actor(true, false);
        let inputs = ExecutionInputs {
            project: &project,
            run_kind: "issue",
            has_issue: true,
            required_capabilities: &[],
            actor: Some(&actor),
            automatic: false,
            requested: None,
            now_unix_secs: 1_700_000_000,
        };
        let cache = FakeCache::default();
        let mut deferred = Vec::new();
        let mut collect = |consume: DeferredConsume| deferred.push(consume);
        let fields = execution_fields(
            &pool,
            &inputs,
            &django_cloud_settings(),
            &django_managed_settings(),
            &cache,
            &mut collect,
            panic_seams(),
        )
        .await
        .expect("local resolves");
        // `{"executor_kind": executor, "tool_plan": {}}` (`creation.py:89`):
        // no pin key, no waiting marker, no pseudo-key.
        assert_eq!(fields.executor_kind, AgentExecutorKind::LocalRunner);
        assert_eq!(fields.tool_plan, json!({}));
        assert_eq!(fields.pinned_runner_id, None);
        assert_eq!(fields.error_code, None);
        assert_eq!(fields.cloud_admission_error, None);
        assert!(deferred.is_empty(), "no admission bucket is touched");
        assert!(cache.consumed.borrow().is_empty());
        assert_eq!(
            fixture()["execution_fields"]["local_runner"]["tool_plan"],
            json!({})
        );
    }

    #[tokio::test]
    async fn unknown_executor_raises_before_any_seam_or_bucket() {
        let pool = lazy_pool();
        let project = project("local_runner");
        let actor = actor(true, false);
        let inputs = ExecutionInputs {
            project: &project,
            run_kind: "issue",
            has_issue: true,
            required_capabilities: &[],
            actor: Some(&actor),
            automatic: false,
            requested: Some("bogus"),
            now_unix_secs: 1_700_000_000,
        };
        let cache = FakeCache::default();
        let mut deferred = Vec::new();
        let mut collect = |consume: DeferredConsume| deferred.push(consume);
        let err = execution_fields(
            &pool,
            &inputs,
            &django_cloud_settings(),
            &django_managed_settings(),
            &cache,
            &mut collect,
            panic_seams(),
        )
        .await
        .expect_err("unknown executor raises");
        assert!(matches!(
            err,
            ExecutionFieldsError::Resolve(ResolveExecutorError::UnknownExecutor)
        ));
        assert_eq!(err.to_string(), "unknown agent executor");
        assert!(deferred.is_empty());
    }

    #[tokio::test]
    async fn cloud_path_refuses_creator_without_llm_before_admission() {
        // None / inactive / bot actors never reach the seam (`creation.py:36`).
        for actor in [None, Some(actor(false, false)), Some(actor(true, true))] {
            let pool = lazy_pool();
            let project = project("cloud_agent");
            let cloud = CloudAgentSettings {
                enabled: true,
                ..django_cloud_settings()
            };
            let inputs = ExecutionInputs {
                project: &project,
                run_kind: "issue",
                has_issue: true,
                required_capabilities: &[],
                actor: actor.as_ref(),
                automatic: false,
                requested: None,
                now_unix_secs: 1_700_000_000,
            };
            let cache = FakeCache::default();
            let mut deferred = Vec::new();
            let mut collect = |consume: DeferredConsume| deferred.push(consume);
            let err = execution_fields(
                &pool,
                &inputs,
                &cloud,
                &django_managed_settings(),
                &cache,
                &mut collect,
                panic_seams(),
            )
            .await
            .expect_err("no funded principal raises");
            assert!(matches!(err, ExecutionFieldsError::Unavailable(_)));
            assert_eq!(
                err.to_string(),
                "The run creator has no AI provider configured. Configure one in Pi Dash AI settings."
            );
            assert!(deferred.is_empty(), "admission never runs");
        }
    }

    #[tokio::test]
    async fn manual_admission_refusal_raises_before_the_capacity_gate() {
        let pool = lazy_pool();
        let project = project("cloud_agent");
        let actor = actor(true, false);
        let cloud = CloudAgentSettings {
            enabled: true,
            ..django_cloud_settings()
        };
        let now = 1_700_000_000i64;
        let bucket = now.div_euclid(60);
        let inputs = ExecutionInputs {
            project: &project,
            run_kind: "issue",
            has_issue: true,
            required_capabilities: &[],
            actor: Some(&actor),
            automatic: false,
            requested: None,
            now_unix_secs: now,
        };
        // Fill the workspace bucket to its limit (30).
        let mut cache = FakeCache::default();
        cache.counts.insert(
            format!(
                "cloud-agent:admission:workspace:{}:{bucket}",
                project.workspace_id
            ),
            30,
        );
        let mut deferred = Vec::new();
        let mut collect = |consume: DeferredConsume| deferred.push(consume);
        let err = execution_fields(
            &pool,
            &inputs,
            &cloud,
            &django_managed_settings(),
            &cache,
            &mut collect,
            ExecutionSeams {
                has_usable_llm_config: || true,
                extra_toolsets_enabled: || panic!("no plan is built on refuse"),
                llm_profile: || panic!("managed seam unreachable here"),
            },
        )
        .await
        .expect_err("manual quota raises");
        assert!(matches!(err, ExecutionFieldsError::Admission(_)));
        assert_eq!(err.to_string(), "Cloud Agent creation rate exceeded");
        // The failed bucket registers nothing (`creation.py:49-50` returns).
        assert!(deferred.is_empty());
    }

    #[tokio::test]
    async fn managed_disabled_raises_at_resolve_without_touching_db() {
        let pool = lazy_pool();
        let project = project("managed_runner");
        let actor = actor(true, false);
        let inputs = ExecutionInputs {
            project: &project,
            run_kind: "issue",
            has_issue: true,
            required_capabilities: &[],
            actor: Some(&actor),
            automatic: false,
            requested: None,
            now_unix_secs: 1_700_000_000,
        };
        let cache = FakeCache::default();
        let mut deferred = Vec::new();
        let mut collect = |consume: DeferredConsume| deferred.push(consume);
        let err = execution_fields(
            &pool,
            &inputs,
            &django_cloud_settings(),
            &django_managed_settings(),
            &cache,
            &mut collect,
            panic_seams(),
        )
        .await
        .expect_err("disabled managed raises");
        match err {
            ExecutionFieldsError::Resolve(ResolveExecutorError::ManagedRunner(unavailable)) => {
                assert_eq!(unavailable.code(), ManagedRunnerReason::DISABLED);
            }
            other => panic!("wrong variant: {other:?}"),
        }
        assert!(deferred.is_empty());
    }

    // -- quota verdict (pure core of both capacity gates) --------------------

    #[test]
    fn quota_verdict_matrix() {
        // Under the cap: admitted either way.
        assert_eq!(quota_verdict(19, 20, 10, false), Ok(None));
        assert_eq!(quota_verdict(19, 20, 10, true), Ok(None));
        // At the cap, manual raises with the retry-after (`creation.py:169-176`).
        let err = quota_verdict(20, 20, 10, false).expect_err("manual raises");
        assert_eq!(err.code(), "run_quota_exceeded");
        assert_eq!(
            err.to_string(),
            "Cloud Agent queue is full for this workspace"
        );
        assert_eq!(err.retry_after_seconds(), Some(10));
        // At the cap, automatic captures `{code, detail}` — the retry-after
        // the raised error carries is dropped (`creation.py:70,175`).
        let deferred = quota_verdict(20, 20, 10, true).expect("automatic defers");
        assert_eq!(
            deferred,
            Some(DeferredAdmissionError {
                code: "run_quota_exceeded".to_owned(),
                detail: "Cloud Agent queue is full for this workspace".to_owned(),
            })
        );
    }

    #[test]
    fn admission_capture_drops_retry_after() {
        let exc = CloudAgentAdmissionError::new("run_quota_exceeded", "detail held", Some(42));
        assert_eq!(
            defer_admission_error(&exc),
            DeferredAdmissionError {
                code: "run_quota_exceeded".to_owned(),
                detail: "detail held".to_owned(),
            }
        );
    }

    // -- managed branches (pure core) ----------------------------------------

    #[test]
    fn managed_decision_matrix() {
        let pin = Uuid::new_v4();
        // Available: pinned to the re-queried online runner, no marker.
        assert_eq!(
            managed_execution_decision(true, "", false, Some(pin), None, false),
            Ok(ManagedFields {
                pinned_runner_id: Some(pin),
                error_code: None,
            })
        );
        // A lost race pins nothing — Python assigns the `None` too.
        assert_eq!(
            managed_execution_decision(true, "", false, None, None, false),
            Ok(ManagedFields {
                pinned_runner_id: None,
                error_code: None,
            })
        );
        // Automatic + transient: pinned to the latest heartbeat, waiting marker.
        assert_eq!(
            managed_execution_decision(
                false,
                ManagedRunnerReason::NOT_CONNECTED,
                true,
                None,
                Some(pin),
                true,
            ),
            Ok(ManagedFields {
                pinned_runner_id: Some(pin),
                error_code: Some(ManagedRunnerReason::NOT_CONNECTED),
            })
        );
        // Manual + transient: refused (`creation.py:124` needs `automatic`).
        let err = managed_execution_decision(
            false,
            ManagedRunnerReason::NOT_CONNECTED,
            true,
            None,
            Some(pin),
            false,
        )
        .expect_err("manual transient raises");
        assert_eq!(err.code(), ManagedRunnerReason::NOT_CONNECTED);
        assert_eq!(
            err.to_string(),
            "Open the Pi Dash desktop app on the machine you want this to run on."
        );
        // Structural failures refuse automatic runs too (`creation.py:120-122`).
        for reason in [
            ManagedRunnerReason::DISABLED,
            ManagedRunnerReason::LLM_CONFIG_MISSING,
            ManagedRunnerReason::NO_RUNNER_FOR_PROJECT,
        ] {
            let err = managed_execution_decision(false, reason, true, None, None, true)
                .expect_err("structural raises");
            assert_eq!(err.code(), reason);
            assert_eq!(err.to_string(), managed_refusal_detail(reason));
        }
        // Offline but nothing enrolled: not transient, refused.
        let err = managed_execution_decision(
            false,
            ManagedRunnerReason::NOT_CONNECTED,
            false,
            None,
            None,
            true,
        )
        .expect_err("unenrolled raises");
        assert_eq!(err.code(), ManagedRunnerReason::NOT_CONNECTED);
    }

    // -- dispatch math --------------------------------------------------------

    #[test]
    fn capacity_clamps_at_zero() {
        assert_eq!(dispatch_capacity(2, 0, 0), 2);
        assert_eq!(dispatch_capacity(2, 1, 1), 0);
        assert_eq!(dispatch_capacity(2, 2, 0), 0);
        assert_eq!(dispatch_capacity(2, 0, 5), 0);
        assert_eq!(dispatch_capacity(2, 9, 9), 0);
    }

    #[test]
    fn lease_expiry_adds_lease_seconds() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-10-01T00:00:00Z")
            .expect("fixed time")
            .with_timezone(&Utc);
        assert_eq!(
            lease_expiry_at(now, 60),
            now + ChronoDuration::seconds(60),
            "now + CLOUD_AGENT_DISPATCH_LEASE_SECONDS"
        );
    }

    // -- event shaping --------------------------------------------------------

    #[test]
    fn kind_truncation_counts_code_points() {
        assert_eq!(truncate_kind("tool_completed"), "tool_completed");
        assert_eq!(truncate_kind(&"k".repeat(64)).len(), 64);
        assert_eq!(truncate_kind(&"k".repeat(65)).len(), 64);
        // Multibyte: 70 × U+00E9 truncates to 64 chars, never splitting a
        // code point (the `events.py:24` slice counts code points too).
        let wide = "é".repeat(70);
        let cut = truncate_kind(&wide);
        assert_eq!(cut.chars().count(), 64);
        assert!(cut.is_char_boundary(cut.len()));
        let emoji = "🚀".repeat(70);
        assert_eq!(truncate_kind(&emoji).chars().count(), 64);
    }

    #[test]
    fn payload_coalescing_matches_python_or() {
        // Falsy collapses to `{}` (`events.py:10`).
        for falsy in [
            None,
            Some(json!({})),
            Some(json!([])),
            Some(json!("")),
            Some(json!(0)),
            Some(json!(0.0)),
            Some(json!(false)),
            Some(json!(null)),
        ] {
            assert_eq!(coalesce_payload(falsy), json!({}));
        }
        // Truthy passes through unchanged — including truthy non-dicts.
        for truthy in [
            json!({"tool": "x"}),
            json!("x"),
            json!([1]),
            json!(1),
            json!(0.5),
            json!(true),
        ] {
            assert_eq!(coalesce_payload(Some(truthy.clone())), truthy);
        }
    }

    #[test]
    fn event_cap_walk_matches_django_oracle() {
        // The `test_event_limit_reserves_terminal_and_truncation_rows` walk
        // with `CLOUD_AGENT_MAX_EVENTS=4`: two appends, then the marker, then
        // capped — the terminal event bypasses `append` (finalization writes
        // it directly), filling the 4th row.
        assert_eq!(
            event_cap_decision(0, 4, false),
            EventCapDecision::Append,
            "one"
        );
        assert_eq!(
            event_cap_decision(1, 4, false),
            EventCapDecision::Append,
            "two"
        );
        assert_eq!(
            event_cap_decision(2, 4, false),
            EventCapDecision::TruncatedMarker,
            "three writes the marker"
        );
        assert_eq!(
            event_cap_decision(3, 4, true),
            EventCapDecision::Capped,
            "four writes nothing"
        );
        // The marker is written once: a second shot at the threshold with the
        // marker present caps instead.
        assert_eq!(
            event_cap_decision(2, 4, true),
            EventCapDecision::Capped,
            "no second marker"
        );
        // Past the marker threshold the probe value is irrelevant.
        assert_eq!(
            event_cap_decision(3, 4, false),
            EventCapDecision::Capped,
            "no room for a marker"
        );
    }

    #[test]
    fn event_cap_edges() {
        // Degenerate limits cap (or mark) immediately, exactly like the
        // `max(0, …)` guards in `events.py:19-20`.
        assert_eq!(
            event_cap_decision(0, 0, false),
            EventCapDecision::Capped,
            "limit 0"
        );
        assert_eq!(
            event_cap_decision(0, 1, false),
            EventCapDecision::Capped,
            "limit 1"
        );
        assert_eq!(
            event_cap_decision(0, 2, false),
            EventCapDecision::TruncatedMarker,
            "limit 2: only the marker fits"
        );
        assert_eq!(
            event_cap_decision(0, 500, false),
            EventCapDecision::Append,
            "production default"
        );
    }

    // -- Celery wire ----------------------------------------------------------

    #[test]
    fn delay_message_matches_kombu_shape() {
        let run_id = "12345678-1234-5678-1234-567812345678";
        let message = run_cloud_agent_message(run_id);
        assert_eq!(message.task, RUN_CLOUD_AGENT_TASK);
        assert_eq!(message.task, "cloud_agent.run_agent_run");
        assert_eq!(message.args, vec![json!(run_id)]);
        assert!(message.kwargs.is_empty());
        assert_eq!(message.retries, 0);
        let headers = message.headers();
        assert_eq!(headers["task"], "cloud_agent.run_agent_run");
        assert_eq!(headers["argsrepr"], format!("('{run_id}',)"));
        assert_eq!(headers["kwargsrepr"], "{}");
        let mut embed = Map::new();
        embed.insert("callbacks".to_owned(), Value::Null);
        embed.insert("errbacks".to_owned(), Value::Null);
        embed.insert("chain".to_owned(), Value::Null);
        embed.insert("chord".to_owned(), Value::Null);
        assert_eq!(
            message.body(),
            json!([[run_id], {}, embed]),
            "protocol v2 [args, kwargs, embed]"
        );
    }

    #[test]
    fn job_row_carries_delay_defaults() {
        let run_id = Uuid::new_v4().to_string();
        let job = run_cloud_agent_job(&run_id);
        assert_eq!(job.task, RUN_CLOUD_AGENT_TASK);
        assert_eq!(job.args, json!([run_id]));
        assert_eq!(job.kwargs, json!({}));
        assert_eq!(job.queue, crate::queue::DEFAULT_QUEUE);
        assert_eq!(job.max_retries, crate::queue::DEFAULT_MAX_RETRIES);
        assert!(
            job.visible_at.is_none(),
            "delayed means immediately visible"
        );
    }

    // -- tool-plan snapshot shape ---------------------------------------------

    #[test]
    fn tool_plan_snapshot_keeps_key_order_and_limits() {
        // The cloud `tool_plan` half of `execution_fields` (`creation.py:73-81`)
        // through the real L3 builder: key order, Django-default limits, the
        // `extra_toolsets` sibling flag.
        let cloud = CloudAgentSettings {
            enabled: true,
            github_tools_enabled: false,
            ..django_cloud_settings()
        };
        let plan = build_tool_plan::<fn() -> bool, fn() -> bool>(
            "issue",
            true,
            &[],
            None,
            Some(|| true),
            &cloud,
        )
        .expect("empty requirements plan");
        let value = serde_json::to_value(&plan).expect("plan serializes");
        let keys: Vec<&str> = value
            .as_object()
            .expect("plan is an object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            [
                "v",
                "catalog_version",
                "tools",
                "required_tools",
                "limits",
                "unavailable_capabilities",
                "extra_toolsets",
            ],
            "creation.py:135-158 dict order"
        );
        assert_eq!(
            value["limits"],
            json!({
                "model_requests": 25,
                "tool_calls": 20,
                "writes": 3,
                "input_tokens": 144_000,
                "output_tokens": 16_000,
                "total_tokens": 160_000,
                "wall_seconds": 285,
            }),
            "settings/common.py defaults"
        );
        assert_eq!(value["extra_toolsets"], true);
        assert_eq!(
            value["unavailable_capabilities"],
            json!(["filesystem", "shell", "worktree"])
        );
    }

    // -- post-commit dispatch --------------------------------------------------

    #[test]
    fn dispatch_after_commit_collects_exactly_once_without_executing() {
        let run_id = Uuid::new_v4();
        let mut pending = Vec::new();
        dispatch_after_commit(&mut |id| pending.push(id), run_id);
        assert_eq!(pending, vec![run_id]);
        // No inline execution: the function performs no I/O (provable by
        // inspection — it only invokes the caller's collector).
        assert_eq!(
            fixture()["dispatch_after_commit"],
            "transaction.on_commit(lambda: dispatch_agent_run(run_id))"
        );
    }

    // -- SQL -------------------------------------------------------------------

    fn parse_postgres(sql: &str) {
        let statements =
            sqlparser::parser::Parser::parse_sql(&sqlparser::dialect::PostgreSqlDialect {}, sql)
                .expect("dispatch SQL must parse as Postgres");
        assert!(!statements.is_empty(), "unparsed: {sql}");
    }

    #[test]
    fn all_statements_parse_as_postgres() {
        for sql in [
            WORKSPACE_LOCK_SQL,
            QUEUED_CLOUD_COUNT_SQL,
            RUNNING_CLOUD_COUNT_SQL,
            OFFERED_COUNT_SQL,
            DUE_RUNS_SQL,
            LEASE_RUN_SQL,
            DISPATCH_RUN_SELECT_SQL,
            AGENT_RUN_LOCK_SQL,
            EVENT_COUNT_SQL,
            EVENT_MARKER_EXISTS_SQL,
            EVENT_MAX_SEQ_SQL,
            EVENT_INSERT_SQL,
            ENROLLED_LATEST_RUNNER_SQL,
        ] {
            parse_postgres(sql);
        }
    }

    #[test]
    fn due_set_skips_locked_oldest_first() {
        assert!(DUE_RUNS_SQL.contains("FOR UPDATE SKIP LOCKED"));
        assert!(DUE_RUNS_SQL.contains("ORDER BY created_at LIMIT $3 FOR UPDATE"));
        assert!(DUE_RUNS_SQL.contains("(lease_expires_at IS NULL OR lease_expires_at <= $2)"));
        assert!(DUE_RUNS_SQL.starts_with("SELECT id FROM agent_run"));
    }

    #[test]
    fn lease_bumps_attempts_on_queued_rows_only() {
        assert!(LEASE_RUN_SQL.contains("dispatch_attempts = dispatch_attempts + 1"));
        assert!(LEASE_RUN_SQL.contains("WHERE id = $2 AND status = 'queued'"));
    }

    #[test]
    fn locks_block_on_the_row() {
        assert!(WORKSPACE_LOCK_SQL.ends_with("FOR UPDATE"));
        assert!(AGENT_RUN_LOCK_SQL.ends_with("FOR UPDATE"));
        assert!(!WORKSPACE_LOCK_SQL.contains("SKIP LOCKED"));
        assert!(!AGENT_RUN_LOCK_SQL.contains("SKIP LOCKED"));
        // The default-manager soft-delete guard: a soft-deleted workspace
        // raises instead of locking (compiler-verified).
        assert!(WORKSPACE_LOCK_SQL.contains("deleted_at IS NULL"));
    }

    #[test]
    fn counts_scope_cloud_and_time() {
        assert!(OFFERED_COUNT_SQL.contains("lease_expires_at > $2"));
        assert!(RUNNING_CLOUD_COUNT_SQL.contains("status = 'running'"));
        assert!(QUEUED_CLOUD_COUNT_SQL.contains("status = 'queued'"));
        for sql in [
            QUEUED_CLOUD_COUNT_SQL,
            RUNNING_CLOUD_COUNT_SQL,
            OFFERED_COUNT_SQL,
        ] {
            assert!(sql.contains("executor_kind = 'cloud_agent'"));
        }
    }

    #[test]
    fn event_statements_match_create_sites() {
        assert!(EVENT_INSERT_SQL.contains("(agent_run_id, seq, kind, payload, created_at)"));
        assert!(EVENT_MAX_SEQ_SQL.contains("ORDER BY seq DESC LIMIT 1"));
        assert!(EVENT_MARKER_EXISTS_SQL.contains("kind = 'events_truncated'"));
    }

    #[test]
    fn statements_touch_dispatch_tables_only() {
        for sql in [
            WORKSPACE_LOCK_SQL,
            QUEUED_CLOUD_COUNT_SQL,
            RUNNING_CLOUD_COUNT_SQL,
            OFFERED_COUNT_SQL,
            DUE_RUNS_SQL,
            LEASE_RUN_SQL,
            DISPATCH_RUN_SELECT_SQL,
            AGENT_RUN_LOCK_SQL,
            EVENT_COUNT_SQL,
            EVENT_MARKER_EXISTS_SQL,
            EVENT_MAX_SEQ_SQL,
            EVENT_INSERT_SQL,
            ENROLLED_LATEST_RUNNER_SQL,
        ] {
            assert!(
                sql.contains("agent_run")
                    || sql.contains("workspaces")
                    || sql.contains("runner")
                    || sql.contains("pod"),
                "unexpected table in: {sql}"
            );
        }
        assert!(
            ENROLLED_LATEST_RUNNER_SQL.contains("ORDER BY runner.last_heartbeat_at DESC LIMIT 1")
        );
    }

    // -- live Postgres (ignored; needs a migrated scratch DB) ------------------

    /// Before/after-row verification against the real Django schema. Needs a
    /// live Postgres — export `DATABASE_URL` on its own line first — with
    /// Django migrations applied (`manage.py migrate`) and never the shared
    /// `postgres` database. Ignored by default so CI without a database stays
    /// green. Every test seeds a uuid-keyed graph and deletes it afterwards,
    /// so tests stay isolated under parallel execution.
    mod live {
        use super::*;
        use sqlx::postgres::PgPoolOptions;

        async fn pool() -> PgPool {
            let url =
                std::env::var("DATABASE_URL").expect("export DATABASE_URL for live dispatch tests");
            let pool = PgPoolOptions::new()
                .max_connections(5)
                .connect(&url)
                .await
                .expect("connect scratch database");
            let db: String = sqlx::query_scalar("SELECT current_database()")
                .fetch_one(&pool)
                .await
                .expect("current database");
            assert_ne!(
                db, "postgres",
                "never test against the shared postgres database"
            );
            crate::queue::ensure_schema(&pool)
                .await
                .expect("queue schema");
            pool
        }

        fn cloud_on() -> CloudAgentSettings {
            CloudAgentSettings {
                enabled: true,
                ..django_cloud_settings()
            }
        }

        fn managed_on() -> ManagedRunnerSettings {
            ManagedRunnerSettings {
                enabled: true,
                ..django_managed_settings()
            }
        }

        struct Graph {
            ws: Uuid,
            user: Uuid,
            project: Uuid,
            pod: Uuid,
        }

        /// Seed workspace → user → project → pod with Django-side defaults
        /// (verified against the model fields) and uuid-suffixed uniques.
        async fn seed_graph(pool: &PgPool, tag: &str) -> Graph {
            let now = Utc::now();
            let user = Uuid::new_v4();
            let ws = Uuid::new_v4();
            let project = Uuid::new_v4();
            let pod = Uuid::new_v4();
            let slug = format!("l6-{tag}-{}", &user.to_string()[..8]);
            sqlx::query(
                "INSERT INTO users (id, username, email, password, first_name, last_name, \
                 display_name, avatar, created_location, last_location, last_login_ip, \
                 last_login_medium, last_login_uagent, last_logout_ip, user_timezone, token, \
                 is_active, is_bot, is_email_valid, is_email_verified, is_managed, \
                 is_password_autoset, is_password_expired, is_password_reset_required, \
                 is_staff, is_superuser, created_at, updated_at, date_joined) \
                 VALUES ($1, $2, $3, '!', '', '', '', '', '', '', '', 'email', '', '', 'UTC', \
                 $4, true, false, false, false, false, false, false, false, false, false, \
                 $5, $5, $5)",
            )
            .bind(user)
            .bind(format!("l6-{tag}-{}", &user.to_string()[..8]))
            .bind(format!("l6-{tag}-{}@example.com", &user.to_string()[..8]))
            .bind(Uuid::new_v4().to_string())
            .bind(now)
            .execute(pool)
            .await
            .expect("seed user");
            sqlx::query(
                "INSERT INTO workspaces (id, created_at, updated_at, name, background_color, \
                 owner_id, slug, timezone) \
                 VALUES ($1, $2, $2, $3, '#f6c8dB', $4, $5, 'UTC')",
            )
            .bind(ws)
            .bind(now)
            .bind(format!("l6 ws {tag}"))
            .bind(user)
            .bind(slug)
            .execute(pool)
            .await
            .expect("seed workspace");
            sqlx::query(
                "INSERT INTO projects (id, created_at, updated_at, name, identifier, description, \
                 network, module_view, cycle_view, issue_views_view, page_view, intake_view, \
                 is_time_tracking_enabled, is_issue_type_enabled, is_default, \
                 guest_view_all_features, members_can_edit_states, archive_in, close_in, \
                 logo_props, timezone, repo_url, base_branch, agent_default_interval_seconds, \
                 agent_default_max_ticks, agent_review_default_interval_seconds, \
                 agent_test_default_interval_seconds, agent_ticking_enabled, \
                 default_agent_executor, workspace_id) \
                 VALUES ($1, $2, $2, $3, $4, '', 2, false, false, false, true, false, false, \
                 false, false, false, true, 0, 0, '{}', 'UTC', '', 'main', 10800, 10, 10800, \
                 10800, true, 'local_runner', $5)",
            )
            .bind(project)
            .bind(now)
            .bind(format!("l6 proj {tag} {}", &project.to_string()[..8]))
            .bind(format!("L6{}", &project.to_string()[..8]))
            .bind(ws)
            .execute(pool)
            .await
            .expect("seed project");
            sqlx::query(
                "INSERT INTO pod (id, created_at, updated_at, description, is_default, name, \
                 project_id, workspace_id) \
                 VALUES ($1, $2, $2, '', false, $3, $4, $5)",
            )
            .bind(pod)
            .bind(now)
            .bind(format!("l6-pod-{tag}"))
            .bind(project)
            .bind(ws)
            .execute(pool)
            .await
            .expect("seed pod");
            Graph {
                ws,
                user,
                project,
                pod,
            }
        }

        /// Seed one run with Django-side defaults; only the dispatch-touched
        /// columns vary.
        async fn seed_run(
            pool: &PgPool,
            graph: &Graph,
            status: &str,
            executor: &str,
            created_at: DateTime<Utc>,
            lease: Option<DateTime<Utc>>,
        ) -> Uuid {
            let id = Uuid::new_v4();
            sqlx::query(
                "INSERT INTO agent_run (id, workspace_id, created_by_id, pod_id, status, \
                 executor_kind, dispatch_attempts, cancel_reason, error_code, tool_plan, \
                 prompt, trigger, phase_kind, run_config, required_capabilities, thread_id, \
                 agent_metadata, lease_expires_at, error, refusal_category, llm_model, usage, \
                 created_at) \
                 VALUES ($1, $2, $3, $4, $5, $6, 0, '', '', '{}', '', 'direct', '', '{}', \
                 '[]', '', '{}', $7, '', '', '', '{}', $8)",
            )
            .bind(id)
            .bind(graph.ws)
            .bind(graph.user)
            .bind(graph.pod)
            .bind(status)
            .bind(executor)
            .bind(lease)
            .bind(created_at)
            .execute(pool)
            .await
            .expect("seed run");
            id
        }

        async fn seed_runner(
            pool: &PgPool,
            graph: &Graph,
            owner: Uuid,
            status: &str,
            heartbeat: Option<DateTime<Utc>>,
        ) -> Uuid {
            let id = Uuid::new_v4();
            sqlx::query(
                "INSERT INTO runner (id, created_at, updated_at, owner_id, workspace_id, pod_id, \
                 name, host_label, provisioning, visibility, refresh_token_hash, \
                 refresh_token_fingerprint, refresh_token_generation, \
                 previous_refresh_token_hash, access_token_signing_key_version, \
                 enrollment_token_hash, enrollment_token_fingerprint, capabilities, status, \
                 os, arch, runner_version, dev_metadata, protocol_version, last_heartbeat_at, \
                 revoked_reason) \
                 VALUES ($1, $2, $2, $3, $4, $5, $6, '', 'desktop_bundled', 0, '', '', 0, '', \
                 1, '', '', '[]', $7, '', '', '', '{}', 1, $8, '')",
            )
            .bind(id)
            .bind(Utc::now())
            .bind(owner)
            .bind(graph.ws)
            .bind(graph.pod)
            .bind(format!("l6-{}", &id.to_string()[..8]))
            .bind(status)
            .bind(heartbeat)
            .execute(pool)
            .await
            .expect("seed runner");
            id
        }

        async fn cleanup(pool: &PgPool, graph: &Graph, runs: &[Uuid], runners: &[Uuid]) {
            for run in runs {
                sqlx::query(
                    "DELETE FROM rust_job_queue WHERE task = 'cloud_agent.run_agent_run' \
                     AND args->>0 = $1",
                )
                .bind(run.to_string())
                .execute(pool)
                .await
                .expect("cleanup queue rows");
                sqlx::query("DELETE FROM agent_run_event WHERE agent_run_id = $1")
                    .bind(run)
                    .execute(pool)
                    .await
                    .expect("cleanup events");
                sqlx::query("DELETE FROM agent_run WHERE id = $1")
                    .bind(run)
                    .execute(pool)
                    .await
                    .expect("cleanup runs");
            }
            for runner in runners {
                sqlx::query("DELETE FROM runner WHERE id = $1")
                    .bind(runner)
                    .execute(pool)
                    .await
                    .expect("cleanup runners");
            }
            sqlx::query("DELETE FROM pod WHERE id = $1")
                .bind(graph.pod)
                .execute(pool)
                .await
                .expect("cleanup pod");
            sqlx::query("DELETE FROM projects WHERE id = $1")
                .bind(graph.project)
                .execute(pool)
                .await
                .expect("cleanup project");
            sqlx::query("DELETE FROM workspaces WHERE id = $1")
                .bind(graph.ws)
                .execute(pool)
                .await
                .expect("cleanup workspace");
            sqlx::query("DELETE FROM users WHERE id = $1")
                .bind(graph.user)
                .execute(pool)
                .await
                .expect("cleanup user");
        }

        async fn attempts_and_lease(pool: &PgPool, run: Uuid) -> (i32, Option<DateTime<Utc>>) {
            sqlx::query_as::<_, (i32, Option<DateTime<Utc>>)>(
                "SELECT dispatch_attempts, lease_expires_at FROM agent_run WHERE id = $1",
            )
            .bind(run)
            .fetch_one(pool)
            .await
            .expect("read run")
        }

        #[tokio::test]
        #[ignore = "needs migrated scratch DB via DATABASE_URL (`manage.py migrate` first)"]
        async fn live_dispatch_waiting_leases_oldest_first() {
            let pool = pool().await;
            let graph = seed_graph(&pool, "lease").await;
            let now = Utc::now();
            let oldest = seed_run(
                &pool,
                &graph,
                "queued",
                "cloud_agent",
                now - ChronoDuration::minutes(30),
                None,
            )
            .await;
            let middle = seed_run(
                &pool,
                &graph,
                "queued",
                "cloud_agent",
                now - ChronoDuration::minutes(20),
                None,
            )
            .await;
            let _newest = seed_run(
                &pool,
                &graph,
                "queued",
                "cloud_agent",
                now - ChronoDuration::minutes(10),
                None,
            )
            .await;
            let running = seed_run(
                &pool,
                &graph,
                "running",
                "cloud_agent",
                now - ChronoDuration::minutes(5),
                None,
            )
            .await;
            // Capacity is 2 - 1 running - 0 offered = 1: only the oldest goes.
            let offered = dispatch_waiting(&pool, &cloud_on(), graph.ws, now)
                .await
                .expect("dispatch");
            assert_eq!(offered, 1);
            let (attempts, lease) = attempts_and_lease(&pool, oldest).await;
            assert_eq!(attempts, 1);
            // Postgres timestamptz keeps microseconds; chrono keeps nanos.
            assert_eq!(
                lease.expect("leased").timestamp_micros(),
                (now + ChronoDuration::seconds(60)).timestamp_micros()
            );
            let (attempts, lease) = attempts_and_lease(&pool, middle).await;
            assert_eq!((attempts, lease), (0, None), "second row untouched");
            // One queue row, `.delay()` wire shape.
            let row: (String, Value, Value) = sqlx::query_as(
                "SELECT task, args, kwargs FROM rust_job_queue \
                 WHERE task = 'cloud_agent.run_agent_run' AND args->>0 = $1",
            )
            .bind(oldest.to_string())
            .fetch_one(&pool)
            .await
            .expect("queue row");
            assert_eq!(row.0, "cloud_agent.run_agent_run");
            assert_eq!(row.1, serde_json::json!([oldest.to_string()]));
            assert_eq!(row.2, serde_json::json!({}));
            // The duplicate offer is a no-op (the lease is held): capacity is
            // 2 - 1 running - 1 offered = 0.
            let offered = dispatch_waiting(&pool, &cloud_on(), graph.ws, now)
                .await
                .expect("re-dispatch");
            assert_eq!(offered, 0);
            let (attempts, _) = attempts_and_lease(&pool, oldest).await;
            assert_eq!(attempts, 1, "no double lease");
            cleanup(&pool, &graph, &[oldest, middle, _newest, running], &[]).await;
        }

        #[tokio::test]
        #[ignore = "needs migrated scratch DB via DATABASE_URL (`manage.py migrate` first)"]
        async fn live_dispatch_waiting_disabled_writes_nothing() {
            let pool = pool().await;
            let graph = seed_graph(&pool, "disabled").await;
            let now = Utc::now();
            let run = seed_run(&pool, &graph, "queued", "cloud_agent", now, None).await;
            let offered = dispatch_waiting(&pool, &django_cloud_settings(), graph.ws, now)
                .await
                .expect("dispatch");
            assert_eq!(offered, 0);
            assert_eq!(attempts_and_lease(&pool, run).await, (0, None));
            let queued: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM rust_job_queue WHERE args->>0 = $1")
                    .bind(run.to_string())
                    .fetch_one(&pool)
                    .await
                    .expect("count queue rows");
            assert_eq!(queued, 0);
            cleanup(&pool, &graph, &[run], &[]).await;
        }

        #[tokio::test]
        #[ignore = "needs migrated scratch DB via DATABASE_URL (`manage.py migrate` first)"]
        async fn live_dispatch_agent_run_branches() {
            let pool = pool().await;
            let graph = seed_graph(&pool, "branches").await;
            let now = Utc::now();
            let cloud = seed_run(&pool, &graph, "queued", "cloud_agent", now, None).await;
            let local = seed_run(&pool, &graph, "queued", "local_runner", now, None).await;
            let done = seed_run(&pool, &graph, "running", "cloud_agent", now, None).await;
            assert_eq!(
                dispatch_agent_run(&pool, &cloud_on(), Uuid::new_v4(), now)
                    .await
                    .expect("missing"),
                DispatchDecision::Ignored,
                "missing row"
            );
            assert_eq!(
                dispatch_agent_run(&pool, &cloud_on(), done, now)
                    .await
                    .expect("non-queued"),
                DispatchDecision::Ignored,
                "non-queued row"
            );
            assert_eq!(
                dispatch_agent_run(&pool, &cloud_on(), cloud, now)
                    .await
                    .expect("cloud"),
                DispatchDecision::CloudDispatched {
                    workspace_id: graph.ws,
                    offered: 1,
                },
                "cloud dispatches its workspace"
            );
            assert_eq!(
                dispatch_agent_run(&pool, &cloud_on(), local, now)
                    .await
                    .expect("local"),
                DispatchDecision::PodDrain { pod_id: graph.pod },
                "machine executors drain their pod"
            );
            cleanup(&pool, &graph, &[cloud, local, done], &[]).await;
        }

        #[tokio::test]
        #[ignore = "needs migrated scratch DB via DATABASE_URL (`manage.py migrate` first)"]
        async fn live_append_event_cap_walk() {
            let pool = pool().await;
            let graph = seed_graph(&pool, "events").await;
            let run = seed_run(&pool, &graph, "queued", "cloud_agent", Utc::now(), None).await;
            let now = Utc::now();
            assert!(append_event(&pool, 4, run, "one", None, now)
                .await
                .expect("one"));
            assert!(
                append_event(&pool, 4, run, "two", Some(serde_json::json!({"n": 2})), now)
                    .await
                    .expect("two")
            );
            assert!(!append_event(&pool, 4, run, "three", None, now)
                .await
                .expect("three"));
            assert!(!append_event(&pool, 4, run, "four", None, now)
                .await
                .expect("four"));
            let rows: Vec<(i32, String, Value)> = sqlx::query_as(
                "SELECT seq, kind, payload FROM agent_run_event WHERE agent_run_id = $1 ORDER BY seq",
            )
            .bind(run)
            .fetch_all(&pool)
            .await
            .expect("event rows");
            assert_eq!(
                rows,
                vec![
                    (1, "one".to_owned(), serde_json::json!({})),
                    (2, "two".to_owned(), serde_json::json!({"n": 2})),
                    (3, "events_truncated".to_owned(), serde_json::json!({})),
                ],
                "two events plus the single marker"
            );
            // Kind truncation and payload coalescing land on the row.
            let wide = seed_run(&pool, &graph, "queued", "cloud_agent", Utc::now(), None).await;
            assert!(append_event(&pool, 500, wide, &"k".repeat(70), None, now)
                .await
                .expect("wide"));
            let stored: (String, Value) =
                sqlx::query_as("SELECT kind, payload FROM agent_run_event WHERE agent_run_id = $1")
                    .bind(wide)
                    .fetch_one(&pool)
                    .await
                    .expect("wide row");
            assert_eq!(stored.0.len(), 64);
            assert_eq!(stored.1, serde_json::json!({}));
            cleanup(&pool, &graph, &[run, wide], &[]).await;
        }

        #[tokio::test]
        #[ignore = "needs migrated scratch DB via DATABASE_URL (`manage.py migrate` first)"]
        async fn live_lock_capacity_paths() {
            let pool = pool().await;
            let graph = seed_graph(&pool, "lockcap").await;
            let now = Utc::now();
            let cloud = cloud_on();
            // Non-cloud returns without touching the database: no workspace
            // row exists for this id, yet no RowNotFound surfaces.
            let mut tx = Transaction::begin(&pool).await.expect("begin");
            assert_eq!(
                lock_cloud_creation_capacity(
                    &mut tx,
                    Uuid::new_v4(),
                    &AgentExecutorKind::LocalRunner,
                    false,
                    &cloud,
                )
                .await
                .expect("non-cloud no-op"),
                None
            );
            tx.rollback().await.expect("rollback");
            // Under the cap: admitted.
            let mut tx = Transaction::begin(&pool).await.expect("begin");
            assert_eq!(
                lock_cloud_creation_capacity(
                    &mut tx,
                    graph.ws,
                    &AgentExecutorKind::CloudAgent,
                    false,
                    &cloud,
                )
                .await
                .expect("under cap"),
                None
            );
            tx.rollback().await.expect("rollback");
            // At the cap: manual raises, automatic defers.
            let mut runs = Vec::new();
            for _ in 0..20 {
                runs.push(seed_run(&pool, &graph, "queued", "cloud_agent", now, None).await);
            }
            let mut tx = Transaction::begin(&pool).await.expect("begin");
            let err = lock_cloud_creation_capacity(
                &mut tx,
                graph.ws,
                &AgentExecutorKind::CloudAgent,
                false,
                &cloud,
            )
            .await
            .expect_err("manual raises at cap");
            match err {
                ExecutionFieldsError::Admission(refused) => {
                    assert_eq!(refused.code(), "run_quota_exceeded");
                    assert_eq!(refused.retry_after_seconds(), Some(10));
                }
                other => panic!("wrong variant: {other:?}"),
            }
            tx.rollback().await.expect("rollback");
            let mut tx = Transaction::begin(&pool).await.expect("begin");
            assert_eq!(
                lock_cloud_creation_capacity(
                    &mut tx,
                    graph.ws,
                    &AgentExecutorKind::CloudAgent,
                    true,
                    &cloud,
                )
                .await
                .expect("automatic defers"),
                Some(DeferredAdmissionError {
                    code: "run_quota_exceeded".to_owned(),
                    detail: "Cloud Agent queue is full for this workspace".to_owned(),
                })
            );
            tx.rollback().await.expect("rollback");
            // A soft-deleted workspace refuses the lock, as `.get()` raises.
            sqlx::query("UPDATE workspaces SET deleted_at = now() WHERE id = $1")
                .bind(graph.ws)
                .execute(&pool)
                .await
                .expect("soft-delete workspace");
            let mut tx = Transaction::begin(&pool).await.expect("begin");
            let err = lock_cloud_creation_capacity(
                &mut tx,
                graph.ws,
                &AgentExecutorKind::CloudAgent,
                false,
                &cloud,
            )
            .await
            .expect_err("deleted workspace raises");
            assert!(matches!(
                err,
                ExecutionFieldsError::Db(sqlx::Error::RowNotFound)
            ));
            tx.rollback().await.expect("rollback");
            sqlx::query("UPDATE workspaces SET deleted_at = NULL WHERE id = $1")
                .bind(graph.ws)
                .execute(&pool)
                .await
                .expect("restore workspace");
            cleanup(&pool, &graph, &runs, &[]).await;
        }

        #[tokio::test]
        #[ignore = "needs migrated scratch DB via DATABASE_URL (`manage.py migrate` first)"]
        async fn live_execution_fields_cloud_happy() {
            let pool = pool().await;
            let graph = seed_graph(&pool, "cloudhappy").await;
            let project = ProjectScope {
                project_id: graph.project,
                workspace_id: graph.ws,
                default_agent_executor: "cloud_agent".to_owned(),
            };
            let actor = ActorScope {
                id: graph.user,
                flags: UserFlags {
                    is_active: true,
                    is_bot: false,
                },
            };
            let inputs = ExecutionInputs {
                project: &project,
                run_kind: "issue",
                has_issue: true,
                required_capabilities: &[],
                actor: Some(&actor),
                automatic: false,
                requested: None,
                now_unix_secs: Utc::now().timestamp(),
            };
            let cache = FakeCache::default();
            let mut deferred = Vec::new();
            let mut collect = |consume: DeferredConsume| deferred.push(consume);
            let fields = execution_fields(
                &pool,
                &inputs,
                &cloud_on(),
                &django_managed_settings(),
                &cache,
                &mut collect,
                ExecutionSeams {
                    has_usable_llm_config: || true,
                    extra_toolsets_enabled: || false,
                    llm_profile: || panic!("managed seam unreachable on the cloud path"),
                },
            )
            .await
            .expect("cloud fields");
            assert_eq!(fields.executor_kind, AgentExecutorKind::CloudAgent);
            assert_eq!(fields.tool_plan["v"], 1);
            assert_eq!(fields.tool_plan["limits"]["model_requests"], 25);
            assert_eq!(fields.tool_plan["extra_toolsets"], false);
            let tools = fields.tool_plan["tools"].as_array().expect("tools array");
            assert!(
                !tools.iter().any(|tool| tool == "github_get_file"),
                "no binding seeded"
            );
            assert!(
                !tools.iter().any(|tool| tool == "pidash_get_project_issue"),
                "non-scheduler run"
            );
            assert_eq!(fields.pinned_runner_id, None);
            assert_eq!(fields.error_code, None);
            assert_eq!(fields.cloud_admission_error, None);
            // Both buckets deferred (manual run with an actor); nothing
            // consumed until the caller runs the deferred consumes.
            assert_eq!(deferred.len(), 2);
            assert!(cache.consumed.borrow().is_empty());
            for consume in &deferred {
                pidash_services::dispatch::consume_admission_token(&cache, consume);
            }
            assert_eq!(cache.consumed.borrow().len(), 2);
            cleanup(&pool, &graph, &[], &[]).await;
        }

        #[tokio::test]
        #[ignore = "needs migrated scratch DB via DATABASE_URL (`manage.py migrate` first)"]
        async fn live_execution_fields_managed_branches() {
            let pool = pool().await;
            let graph = seed_graph(&pool, "managed").await;
            let now = Utc::now();
            let runner = seed_runner(&pool, &graph, graph.user, "online", Some(now)).await;
            let project = ProjectScope {
                project_id: graph.project,
                workspace_id: graph.ws,
                default_agent_executor: "managed_runner".to_owned(),
            };
            let actor = ActorScope {
                id: graph.user,
                flags: UserFlags {
                    is_active: true,
                    is_bot: false,
                },
            };
            let profile = || LlmProfile {
                available: true,
                reason_code: String::new(),
            };
            let inputs_for = |automatic| ExecutionInputs {
                project: &project,
                run_kind: "issue",
                has_issue: true,
                required_capabilities: &[],
                actor: Some(&actor),
                automatic,
                requested: None,
                now_unix_secs: now.timestamp(),
            };
            let cache = FakeCache::default();
            // Online: pinned to the creator's own runner, no marker.
            let mut deferred = Vec::new();
            let mut collect = |consume: DeferredConsume| deferred.push(consume);
            let fields = execution_fields(
                &pool,
                &inputs_for(false),
                &django_cloud_settings(),
                &managed_on(),
                &cache,
                &mut collect,
                ExecutionSeams {
                    has_usable_llm_config: || panic!("cloud seam unreachable here"),
                    extra_toolsets_enabled: || panic!("cloud seam unreachable here"),
                    llm_profile: profile,
                },
            )
            .await
            .expect("managed fields");
            assert_eq!(fields.executor_kind, AgentExecutorKind::ManagedRunner);
            assert_eq!(fields.tool_plan, serde_json::json!({}));
            assert_eq!(fields.pinned_runner_id, Some(runner));
            assert_eq!(fields.error_code, None);
            // Offline but enrolled: manual refuses, automatic waits visibly.
            sqlx::query(
                "UPDATE runner SET status = 'offline', last_heartbeat_at = $1 WHERE id = $2",
            )
            .bind(now - ChronoDuration::hours(1))
            .bind(runner)
            .execute(&pool)
            .await
            .expect("take runner offline");
            let mut deferred = Vec::new();
            let mut collect = |consume: DeferredConsume| deferred.push(consume);
            let err = execution_fields(
                &pool,
                &inputs_for(false),
                &django_cloud_settings(),
                &managed_on(),
                &cache,
                &mut collect,
                ExecutionSeams {
                    has_usable_llm_config: || panic!("cloud seam unreachable here"),
                    extra_toolsets_enabled: || panic!("cloud seam unreachable here"),
                    llm_profile: profile,
                },
            )
            .await
            .expect_err("manual offline raises");
            match err {
                ExecutionFieldsError::Managed(refused) => {
                    assert_eq!(refused.code(), ManagedRunnerReason::NOT_CONNECTED);
                }
                other => panic!("wrong variant: {other:?}"),
            }
            let mut deferred = Vec::new();
            let mut collect = |consume: DeferredConsume| deferred.push(consume);
            let fields = execution_fields(
                &pool,
                &inputs_for(true),
                &django_cloud_settings(),
                &managed_on(),
                &cache,
                &mut collect,
                ExecutionSeams {
                    has_usable_llm_config: || panic!("cloud seam unreachable here"),
                    extra_toolsets_enabled: || panic!("cloud seam unreachable here"),
                    llm_profile: profile,
                },
            )
            .await
            .expect("automatic waits");
            assert_eq!(fields.pinned_runner_id, Some(runner));
            assert_eq!(
                fields.error_code.as_deref(),
                Some(ManagedRunnerReason::NOT_CONNECTED)
            );
            cleanup(&pool, &graph, &[], &[runner]).await;
        }

        #[tokio::test]
        #[ignore = "needs migrated scratch DB via DATABASE_URL (`manage.py migrate` first)"]
        async fn live_due_set_skips_locked_rows() {
            let pool = pool().await;
            let graph = seed_graph(&pool, "skiplocked").await;
            let now = Utc::now();
            let oldest = seed_run(
                &pool,
                &graph,
                "queued",
                "cloud_agent",
                now - ChronoDuration::minutes(30),
                None,
            )
            .await;
            let newer = seed_run(
                &pool,
                &graph,
                "queued",
                "cloud_agent",
                now - ChronoDuration::minutes(10),
                None,
            )
            .await;
            // A concurrent dispatcher holds the oldest row: our dispatch must
            // skip it and lease the newer one.
            let mut holder = Transaction::begin(&pool).await.expect("begin holder");
            sqlx::query_scalar::<_, Uuid>("SELECT id FROM agent_run WHERE id = $1 FOR UPDATE")
                .bind(oldest)
                .fetch_one(&mut **holder.inner())
                .await
                .expect("hold oldest");
            let offered = dispatch_waiting(&pool, &cloud_on(), graph.ws, now)
                .await
                .expect("dispatch");
            assert_eq!(offered, 1);
            assert_eq!(
                attempts_and_lease(&pool, oldest).await,
                (0, None),
                "locked row skipped"
            );
            let (attempts, lease) = attempts_and_lease(&pool, newer).await;
            assert_eq!(attempts, 1);
            assert!(lease.is_some());
            holder.rollback().await.expect("release holder");
            cleanup(&pool, &graph, &[oldest, newer], &[]).await;
        }
    }
}
