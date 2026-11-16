#![forbid(unsafe_code)]

//! Executor resolve + creation capacity (`creation.py`, `policy.py`).
//!
//! Moved verbatim from `pidash_jobs::dispatch` (PIDASHCONV-1521 Part D):
//! the `execution_fields` resolve (cloud/local after #2903 — the
//! managed creation decision is deleted), the
//! `lock_cloud_creation_capacity` hard gate, their executor-generic SQL
//! helpers, and the pure verdict cores. The jobs module re-exports every
//! public item, so all `pidash_jobs::dispatch::…` paths keep working.
//! The services home lets the SeaORM-backed creation seam (the domain
//! move path, which cannot depend on jobs) call the resolve directly.
//!
//! The managed decision core was deleted by #2903 (the `W`-only
//! managed surface lives in `policy.rs`/admission); its tests still run
//! from the jobs module through the re-exports.

use std::sync::Arc;

use serde_json::{Map, Value};
use uuid::Uuid;

use pidash_db::config::{CloudAgentSettings, ManagedRunnerSettings};
use pidash_db::tx::Transaction;
use pidash_types::dispatch::AgentExecutorKind;

use super::admission::AdmissionCache;
use super::admission::CloudAgentAdmissionError;
use super::admission::DeferredConsume;
use super::enforce_creation_rate;
use super::policy::build_tool_plan;
use super::policy::resolve_executor_kind;
use super::policy::user_has_llm_config;
use super::policy::CloudAgentUnavailable;
use super::policy::CloudCapabilityUnavailable;
use super::policy::ResolveExecutorError;
use super::policy::UserFlags;
use crate::extensions::CloudAgentToolsetsSeam;

/// (Managed creation retired by #2903: `_managed_execution_fields` and
/// `_MANAGED_REFUSAL_DETAIL` were deleted from `creation.py`; anything
/// that is not cloud returns the local shape.)
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

/// The queued-cloud count (`creation.py:57-61,163-167`): `COUNT(*)` with the
/// workspace first (the `$1` param); the compiler's own conjunct order
/// differs textually — equivalent, since `AND` is commutative. Params: `$1`
/// workspace id (uuid).
pub const QUEUED_CLOUD_COUNT_SQL: &str = "SELECT COUNT(*) FROM agent_run \
    WHERE workspace_id = $1 AND executor_kind = 'cloud_agent' AND status = 'queued'";

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

/// SeaORM form of [`QUEUED_CLOUD_COUNT_SQL`]: the workspace's queued
/// cloud runs, `COUNT(*)` under the output name `n`.
pub fn queued_cloud_count_select(workspace_id: Uuid) -> sea_orm::sea_query::SelectStatement {
    use pidash_db::entity::agent_run;
    use sea_orm::sea_query::{Alias, Asterisk, Expr, Query};
    let col = |column: agent_run::Column| Expr::col((agent_run::Entity, column));
    Query::select()
        .expr_as(Expr::col(Asterisk).count(), Alias::new("n"))
        .from(agent_run::Entity)
        .and_where(col(agent_run::Column::WorkspaceId).eq(workspace_id))
        .and_where(col(agent_run::Column::ExecutorKind).eq("cloud_agent"))
        .and_where(col(agent_run::Column::Status).eq("queued"))
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

/// The EE-overlayable seams `execution_fields` consults, each only when the
/// Python short-circuit reaches it.
pub struct ExecutionSeams<F> {
    /// `has_usable_llm_config(user)` (`agent_execution.py:78-80`): consulted
    /// only for a present, active, non-bot actor on the cloud path.
    pub has_usable_llm_config: F,
    /// `extra_toolsets_enabled_for(creator)` (`toolsets.py:23-32`): the
    /// injected toolsets seam, awaited only when a creator is present.
    pub toolsets: Arc<dyn CloudAgentToolsetsSeam>,
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

/// Every failure `execution_fields` and [`lock_cloud_creation_capacity`]
/// report: the Python raises (`ValueError` / `RuntimeError` at the
/// orchestration call sites) plus the database.
#[derive(Debug, thiserror::Error)]
pub enum ExecutionFieldsError {
    /// `resolve_executor_kind` refused (`creation.py:26`): unknown executor
    /// or cloud not configured.
    #[error(transparent)]
    Resolve(#[from] ResolveExecutorError),
    /// The creator has no LLM config (`creation.py:36-39`).
    #[error(transparent)]
    Unavailable(CloudAgentUnavailable),
    /// A manual gate refused (`creation.py:49-50,68-69`).
    #[error(transparent)]
    Admission(#[from] CloudAgentAdmissionError),
    /// A required capability is not plannable (`creation.py:73-81` via
    /// `build_tool_plan`).
    #[error(transparent)]
    Capability(#[from] CloudCapabilityUnavailable),
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

async fn count_queued_cloud(
    conn: &mut sqlx::PgConnection,
    workspace_id: Uuid,
) -> Result<i64, sqlx::Error> {
    use sea_orm::{ConnectionTrait, DbBackend};
    let tx_conn = pidash_db::orm_tx::TxConn::new(conn);
    let row = tx_conn
        .query_one(DbBackend::Postgres.build(&queued_cloud_count_select(workspace_id)))
        .await
        .map_err(pidash_db::orm_tx::into_sqlx)?
        .ok_or(sqlx::Error::RowNotFound)?;
    row.try_get("", "n").map_err(pidash_db::orm_tx::into_sqlx)
}

/// SeaORM form of the `GITHUB_BINDING_EXISTS_SQL` probe (policy.py):
/// the four-join provider chain, no deleted_at scope on joined
/// tables (Django filters only the base table).
fn github_binding_exists_select(
    project: &ProjectScope,
) -> sea_orm::Select<pidash_db::entity::git_repository_bindings::Entity> {
    use pidash_db::entity::{
        git_provider_accounts, git_repositories, git_repository_bindings, github_app_installations,
        workspace_integrations,
    };
    use sea_orm::{ColumnTrait, EntityTrait, JoinType, QueryFilter, QuerySelect, RelationTrait};
    git_repository_bindings::Entity::find()
        .select_only()
        .column(git_repository_bindings::Column::Id)
        .join(
            JoinType::InnerJoin,
            git_repository_bindings::Relation::GitRepositories.def(),
        )
        .join(
            JoinType::InnerJoin,
            git_repository_bindings::Relation::GitProviderAccounts.def(),
        )
        .join(
            JoinType::InnerJoin,
            git_provider_accounts::Relation::WorkspaceIntegrations.def(),
        )
        .join(
            JoinType::InnerJoin,
            workspace_integrations::Relation::GithubAppInstallations.def(),
        )
        .filter(git_repository_bindings::Column::ProjectId.eq(project.project_id))
        .filter(git_repository_bindings::Column::WorkspaceId.eq(project.workspace_id))
        .filter(git_repository_bindings::Column::DeletedAt.is_null())
        .filter(git_repositories::Column::Provider.eq("github"))
        .filter(git_repositories::Column::HostUrl.eq("https://github.com"))
        .filter(git_provider_accounts::Column::WorkspaceId.eq(project.workspace_id))
        .filter(git_provider_accounts::Column::Provider.eq("github"))
        .filter(git_provider_accounts::Column::HostUrl.eq("https://github.com"))
        .filter(git_provider_accounts::Column::AuthType.eq("github_app"))
        .filter(git_provider_accounts::Column::Status.eq("connected"))
        .filter(git_provider_accounts::Column::VerifiedAt.is_not_null())
        .filter(github_app_installations::Column::VerifiedAt.is_not_null())
        .filter(github_app_installations::Column::SuspendedAt.is_null())
        .limit(1)
}

async fn github_binding_exists(
    conn: &mut sqlx::PgConnection,
    project: &ProjectScope,
) -> Result<bool, sqlx::Error> {
    let tx_conn = pidash_db::orm_tx::TxConn::new(conn);
    let found = github_binding_exists_select(project)
        .into_tuple::<Uuid>()
        .one(&tx_conn)
        .await
        .map_err(pidash_db::orm_tx::into_sqlx)?;
    Ok(found.is_some())
}

/// Resolve the executor-specific `AgentRun` fields for one run creation
/// (`execution_fields`, `creation.py:11-89`).
///
/// The capacity gate runs on the caller's transaction — the ambient
/// `atomic` at `creation.py:55-70` made explicit. Orchestration callers
/// (D-12) open a short transaction that commits before the fields are built
/// (today's inner-atomic shape); the direct-run view
/// (`runner/views/runs.py:262-264`) passes its insertion transaction, so the
/// workspace lock is held across gate+insert exactly as the savepoint does in
/// Python and the queue cap stays a hard cap for concurrent creates. The
/// deferred admission consumes collect into `on_commit` and the caller runs
/// them after commit — a rolled-back creation burns no quota, while consumes
/// registered before a later raise stay registered, as on Django's ambient
/// transaction. This function never commits or rolls back `tx` itself.
#[allow(clippy::too_many_arguments)]
pub async fn execution_fields<C, F>(
    tx: &mut Transaction<'_>,
    inputs: &ExecutionInputs<'_>,
    cloud: &CloudAgentSettings,
    _managed: &ManagedRunnerSettings,
    cache: &C,
    on_commit: &mut (dyn FnMut(DeferredConsume) + Send),
    seams: ExecutionSeams<F>,
) -> Result<ExecutionFields, ExecutionFieldsError>
where
    C: AdmissionCache,
    F: FnOnce() -> bool,
{
    let executor = resolve_executor_kind(
        inputs.requested,
        &inputs.project.default_agent_executor,
        cloud,
    )?;
    if executor == AgentExecutorKind::CloudAgent {
        return execution_fields_cloud(tx.inner(), inputs, cloud, cache, on_commit, seams).await;
    }
    // #2903: anything that is not cloud returns the local shape (the
    // managed branch is gone).
    Ok(ExecutionFields {
        executor_kind: executor,
        tool_plan: Value::Object(Map::new()),
        pinned_runner_id: None,
        error_code: None,
        cloud_admission_error: None,
    })
}

async fn execution_fields_cloud<C, F>(
    conn: &mut sqlx::PgConnection,
    inputs: &ExecutionInputs<'_>,
    cloud: &CloudAgentSettings,
    cache: &C,
    on_commit: &mut (dyn FnMut(DeferredConsume) + Send),
    seams: ExecutionSeams<F>,
) -> Result<ExecutionFields, ExecutionFieldsError>
where
    C: AdmissionCache,
    F: FnOnce() -> bool,
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
    // The point-in-time gate on the caller's transaction (`creation.py:52-70`):
    // orchestration callers commit their short transaction after this returns
    // (releasing the lock before the fields are built, as the inner atomic
    // does), while the direct-run view holds the lock across gate+insert.
    // Either way the insertion transaction re-checks via
    // `lock_cloud_creation_capacity`.
    lock_workspace(&mut *conn, inputs.project.workspace_id).await?;
    let queued = count_queued_cloud(&mut *conn, inputs.project.workspace_id).await?;
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
        Some(github_binding_exists(&mut *conn, inputs.project).await?)
    } else {
        None
    };
    // The run executes as its creator, so their preference is what the
    // snapshot records (`creation.py:78-80`); no creator, no seam consult.
    let extra_toolsets = match inputs.actor {
        Some(actor) => {
            let verdict = seams.toolsets.extra_toolsets_enabled(actor.id).await;
            Some(move || verdict)
        }
        None => None,
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
    lock_workspace(tx.inner(), workspace_id).await?;
    let queued = count_queued_cloud(tx.inner(), workspace_id).await?;
    Ok(quota_verdict(
        queued,
        cloud.max_queued_per_workspace,
        cloud.dispatch_scan_interval_secs,
        automatic,
    )?
    .into_iter()
    .next())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn scope() -> ProjectScope {
        ProjectScope {
            project_id: Uuid::nil(),
            workspace_id: Uuid::max(),
            default_agent_executor: String::new(),
        }
    }

    #[test]
    fn github_probe_statement_shape() {
        let sql = sea_orm::QueryTrait::build(
            &github_binding_exists_select(&scope()),
            sea_orm::DbBackend::Postgres,
        )
        .to_string();
        assert!(sql.contains("git_repository_bindings"), "base: {sql}");
        assert!(sql.contains("git_repositories"), "join r: {sql}");
        assert!(sql.contains("git_provider_accounts"), "join a: {sql}");
        assert!(sql.contains("workspace_integrations"), "join w: {sql}");
        assert!(sql.contains("github_app_installations"), "join i: {sql}");
        assert!(sql.contains("github"), "provider filter: {sql}");
        assert!(sql.contains("connected"), "status filter: {sql}");
        assert!(sql.contains("LIMIT 1"), "first-row: {sql}");
    }
}
