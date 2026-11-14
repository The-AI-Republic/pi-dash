#![forbid(unsafe_code)]

//! Executor resolve + creation capacity (`creation.py`, `policy.py`).
//!
//! Collapsed by P3 step B (agent naming convention): `execution_fields`
//! is now `resolve_executor_kind` plus an empty tool plan. The cloud
//! creation gate (`execution_fields_cloud`,
//! `lock_cloud_creation_capacity`, `quota_verdict`, the queued-cloud
//! counts) died with the retired plane. `lock_workspace` stays —
//! shared with the dispatch engine.

use serde_json::{Map, Value};
use pidash_types::agent_runs::AgentExecutorKind;
use uuid::Uuid;

<<<<<<<< HEAD:rust-api/crates/services/src/dispatch/execution.rs
use pidash_types::dispatch::AgentExecutorKind;

use super::policy::resolve_executor_kind;
use super::policy::ResolveExecutorError;
use super::policy::UserFlags;
========
use super::executor_policy::resolve_executor_kind;
use super::executor_policy::ResolveExecutorError;
use super::executor_policy::UserFlags;
>>>>>>>> eb1cb02bc (refactor(p5): redo module moves with the spec names):rust-api/crates/services/src/run_creation/execution.rs

/// The captured quota shape (`creation.py:70,175`): `{code, detail}` — the
/// `retry_after_seconds` the raised error carries is dropped on capture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeferredAdmissionError {
    pub code: String,
    pub detail: String,
}

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

/// SeaORM form of [`WORKSPACE_LOCK_SQL`]: the live workspace row,
/// `FOR UPDATE`, projecting a constant (`SELECT 1`). No row means the
/// workspace is missing or soft-deleted, as `.get()` raising does.
pub fn workspace_lock_select(workspace_id: Uuid) -> sea_orm::sea_query::SelectStatement {
    use pidash_db::entity::workspaces;
    use sea_orm::sea_query::{Expr, LockType, Query};
    Query::select()
        .expr(Expr::val(1))
        .from(workspaces::Entity)
        .and_where(Expr::col((workspaces::Entity, workspaces::Column::Id)).eq(workspace_id))
        .and_where(Expr::col((workspaces::Entity, workspaces::Column::DeletedAt)).is_null())
        .lock(LockType::Update)
        .to_owned()
}
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



/// The executor-specific `AgentRun` fields for one run creation
/// (`execution_fields`, `creation.py:11-89`, #2903).
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
    /// The pin: always `None` on the cloud/local paths (#2903: the
    /// managed pin is gone).
    pub pinned_runner_id: Option<Uuid>,
    /// `None` everywhere (#2903: the managed `desktop_not_connected`
    /// waiting marker is gone).
    pub error_code: Option<String>,
    /// The deferred quota refusal on the automatic cloud path (not a column).
    pub cloud_admission_error: Option<DeferredAdmissionError>,
}

/// Every failure `execution_fields` reports: `resolve_executor_kind`
/// failures plus the database. (P3 step B: the cloud variants died
/// with the cloud creation path.)
#[derive(Debug, thiserror::Error)]
pub enum ExecutionFieldsError {
    /// `resolve_executor_kind` refused (`creation.py:26`): unknown executor.
    #[error(transparent)]
    Resolve(#[from] ResolveExecutorError),
    /// The database failed.
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
}

/// The workspace mutex take, shared with the dispatch engine: `lock_workspace`
/// runs on the caller's transaction connection (the dispatch span's
/// short transaction or the insertion transaction).
pub async fn lock_workspace(
    conn: &mut sqlx::PgConnection,
    workspace_id: Uuid,
) -> Result<(), sqlx::Error> {
    use sea_orm::{ConnectionTrait, DbBackend};
    let tx_conn = pidash_db::orm_tx::TxConn::new(conn);
    let row = tx_conn
        .query_one(DbBackend::Postgres.build(&workspace_lock_select(workspace_id)))
        .await
        .map_err(pidash_db::orm_tx::into_sqlx)?;
    match row {
        Some(_) => Ok(()),
        None => Err(sqlx::Error::RowNotFound),
    }
}

/// Resolve the executor-specific `AgentRun` fields for one run creation
/// (`execution_fields`, `creation.py:11-89`).
///
/// P3 step B: collapsed to `resolve_executor_kind` plus an empty tool
/// plan — the cloud gate, admission rate limits, quota verdict, and
/// tool-plan build died with the retired plane.
/// P4: the admission/cache/seams parameters died with the cache family.
pub async fn execution_fields(
    inputs: &ExecutionInputs<'_>,
) -> Result<ExecutionFields, ExecutionFieldsError> {
    let executor = resolve_executor_kind(
        inputs.requested,
        &inputs.project.default_agent_executor,
    )?;
    Ok(ExecutionFields {
        executor_kind: executor,
        tool_plan: Value::Object(Map::new()),
        pinned_runner_id: None,
        error_code: None,
        cloud_admission_error: None,
    })
}
