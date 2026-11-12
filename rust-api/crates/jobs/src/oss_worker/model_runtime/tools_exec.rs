//! The tool executors: `build_tools` closures over one run (PIDASHCONV-866).
//!
//! Ports `cloud_agent/tools.py:182-513` (`build_tools`) plus the GitHub
//! twins' sync bodies (`github_mcp.py:19` builds the same closures with
//! `source="mcp"`): the `_audit` ledger protocol, the `_scope` tenant
//! gate, and the 15 operations. [`RunTools`] implements
//! [`ToolExecutor`][super::agent::ToolExecutor] for the agent loop.
//!
//! Composition rules (each cites its probe or port):
//!
//! * The ledger id is always a fresh v4 (`resolve_tool_call_id(None)`):
//!   the loop's call id never reaches the twin across the in-process
//!   hop, and function tools never see the contextvar set (`verify2.py`
//!   `b_ctx`). The `call_id` arguments are ignored.
//! * Pre-audit validators (comment/workpad/create/relation/github
//!   guards) run before the ledger insert: a validation failure leaves
//!   no row (`tools.py:274-277,296-299,345-348,393-414,450-462,482-485`).
//! * `_scope` runs before the ledger insert on both paths; a scope
//!   denial leaves no row (`tools.py:98,131`).
//! * Write path: one transaction covering scope, admission, ledger,
//!   the op, and the success marking; an op error rolls everything
//!   back and propagates with no failed row (`tools.py:96-129`).
//! * Read path: no ambient transaction; an op error marks the row
//!   failed with `type(exc).__name__[:64]` and propagates
//!   (`tools.py:130-155`).
//! * `tool_completed` appends after success only, outside the write
//!   transaction, and its capped-bool is ignored (`tools.py:156`).
//! * No `ModelRetry` escapes: every failure is `Fatal` with `str(exc)`
//!   (`loopprobe.json` `tool_exc` — none of our tools raise it).
//! * The transition op routes through the live
//!   `handle_issue_state_transition` with `moved_by_run` set, which
//!   never dispatches (`scheduling.py:412-436`, ported
//!   `clock.rs:488`): the clock reconciles (queue or park) and the
//!   builder — preflight included — is unreachable. `BinPreflight`
//!   fails that arm closed, pinned by `agent_moves_never_dispatch_now`.
//! * Relation reference pools reuse [`MEMBER_PROJECT_ISSUES_SQL`] as an
//!   `IN` subquery (binds `$1`/`$2`), never a forked predicate list.
//!   `OWN_PROJECT_ISSUES_PREDICATE` slots in as `$3` on the own-project
//!   pool. (Its doc points at the assistant's `SCOPED_ISSUES_SQL`,
//!   whose stateless exclusion PIDASHCONV-1250 fixed null-safe; not
//!   followed here.)
//! * Reference spelling follows the D-12 [`classify_ref`] contract; the
//!   v1 relations handler parses a more lenient UUID grammar
//!   (misplaced hyphens). Realistic refs agree on both; the D-12 golden
//!   is the contract composed here.
//! * Stripped text uses the `MLStripper` port (`ml_strip_tags`): model
//!   `save()` computes through `html_processor.strip_tags`, not
//!   Django's `strip_tags` (`handlers_engage.rs:1210`).
//! * `created_at` renders `datetime.isoformat()` (micros iff nonzero),
//!   the `render_isoformat` precedent.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde_json::{Map, Value};
use sqlx::PgPool;
use uuid::Uuid;

use crate::admission::RedisAdmissionCache;
use crate::dispatch::{append_event, dispatch_agent_run, DispatchDecision};
use crate::issue_transition::WorkerTaskEnqueueHook;
use crate::orchestration::{
    fire_tick_seam::emit_logs, live_inputs::JobsMatcher, preflight_eligibility_or_bounce,
    LiveCreationStore, StoreDeps,
};
use crate::queue::{enqueue, enqueue_in_write_txn, NewJob};
use crate::runner_runs::sweeps_chat::terminal_effects_job;
use crate::tasks_cleanup::deletion::SOFT_DELETE_TASK;
use pidash_db::config::{CloudAgentSettings, ManagedRunnerSettings, RunnerSettings};
use pidash_db::tasks_ticker::models::issue_agent_ticker::IssueAgentTicker;
use pidash_db::tx::Transaction;
use sea_orm::{
    sea_query::Expr, ActiveModelTrait, ColumnTrait, EntityTrait, FromQueryResult, JoinType,
    PaginatorTrait, QueryFilter, QueryOrder, QuerySelect, RelationTrait,
};

/// SeaORM view over the request pool (no I/O; built once per call).
fn sea_conn(pool: &PgPool) -> sea_orm::DatabaseConnection {
    pidash_db::pool::sea_orm_connection(pool)
}

use pidash_services::assistant::markdown::to_safe_html;
use pidash_services::assistant::seams::agent_model_profile_for_user;
use pidash_services::assistant::tools_issues::escape_icontains;
use pidash_services::dispatch::policy::UserFlags;
use pidash_services::dispatch::tool_issues::ToolIssueRow;
use pidash_services::dispatch::tools as plan;
use pidash_services::dispatch::{
    admission::AdmissionCache, consume_admission_token, DeferredConsume, LlmProfile,
};
use pidash_services::domain::activity::TaskEnqueueHook;
use pidash_services::domain::clock::apply_clock_write;
use pidash_services::domain::identity::AgentUserCollisionError;
use pidash_services::domain::transition::IssueTransitionHook;
use pidash_services::domain::{self, WriteActor, WriteCtx, WriteOptions, WriteScope, WriteTxn};
use pidash_services::extensions::CloudAgentToolsetsSeam;
use pidash_services::operations::core::comments as core_comments;
use pidash_services::operations::core::relations as core_relations;
use pidash_services::operations::relations as relations_ops;
use pidash_services::orchestration::clock::{ClockWrite, ProjectClockPolicy};
use pidash_services::orchestration::creation::{
    AdmissionError, CreationError, CreationSeam, DedicatedPinArgs, ExecutionError, ExecutionFields,
    ExecutionRequest, FinalizeAgentRunSeam, IssueView, LockedIssue, NewAgentRun, NewSystemComment,
    PinOutcome, PodView, ProjectView, RenderBundle, RenderedTurn, RunView, RunnerView, StateView,
};
use pidash_services::orchestration::entries::{
    fire_state_transition, BindingView, EntriesSeam, FireOutcome, FireRequest, NewSchedulerRun,
    PreflightSeam, ProjectRoleFacts, SchedulerView, WorkspaceView,
};
use pidash_services::orchestration::relations as rel;
use pidash_services::prompting::composer::{OverrideIndex, OverrideRow};
use pidash_services::runner_runs::{finalization, LifecycleEffect};
use pidash_services::runner_sessions::live_drains::drain_pod_by_id;
use pidash_types::dispatch::AgentExecutorKind;

use super::agent::{ToolExecutor, ToolOutcome};
use super::github::{GithubAppContext, GithubFailure, GithubRestClient};
use super::validation::ValidatedArgs;
use pidash_compat::datetime::render_isoformat;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Every failure a tool op reports, keyed by its Python exception.
///
/// The read path records [`ToolExecError::python_type`] as the ledger
/// `error_code` (`tools.py:147`); both paths surface
/// [`ToolExecError::message`] (`str(exc)`) as the fatal tool result.
/// The write path never marks failures (the rollback drops the ledger
/// row), so `Db`/`Creation` messages only ever reach the model.
#[derive(Debug)]
pub enum ToolExecError {
    /// `ToolDenied` / `ValueError` / `RuntimeError` from the ported
    /// planners (guards, validators, verdicts, bounds).
    Failure(plan::ToolCallFailure),
    /// A GitHub-leg failure: Python type plus message.
    Github(GithubFailure),
    /// `Model.DoesNotExist` (`{Model} matching query does not exist.`,
    /// Django `query.py:639-640`).
    Missing {
        /// `Model._meta.object_name` (`Issue`, `State`, ...).
        model: &'static str,
    },
    /// UUIDField `ValidationError`
    /// (`['“{value}” is not a valid UUID.']` — curly quotes inside the
    /// messages-list repr, probed live on the vendored Django).
    InvalidUuid {
        /// The rejected raw value, verbatim.
        value: String,
    },
    /// `'NoneType' object has no attribute '{attribute}'`: a bound-issue
    /// (or scheduler-binding) dereference on a run that has none
    /// (`tools.py:202,207,280,351` — `select_related` misses read as
    /// `None`, never `RelatedObjectDoesNotExist`).
    AttributeError {
        /// The missing attribute (`id`, `project`, ...).
        attribute: &'static str,
    },
    /// A database failure. Ledger code `IntegrityError` on
    /// 23505/23503/23502 (the codes Django surfaces as
    /// `IntegrityError`), else `OperationalError`; the message is the
    /// driver's text (necessarily not Django's — different driver).
    Db(sqlx::Error),
    /// The transition driver failed. Only the (write-path) transition
    /// op constructs this, so it never reaches the ledger; the code
    /// arm exists so the enum is total.
    Creation(pidash_services::orchestration::creation::CreationError),
}

impl ToolExecError {
    /// `type(exc).__name__` for the read-path ledger `error_code`.
    pub fn python_type(&self) -> &'static str {
        match self {
            ToolExecError::Failure(failure) => failure.python_type_name(),
            ToolExecError::Github(failure) => failure.python_type,
            ToolExecError::Missing { .. } => "DoesNotExist",
            ToolExecError::InvalidUuid { .. } => "ValidationError",
            ToolExecError::AttributeError { .. } => "AttributeError",
            ToolExecError::Db(error) => match error.as_database_error().and_then(|db| db.code()) {
                Some(code)
                    if code.as_ref() == "23505"
                        || code.as_ref() == "23503"
                        || code.as_ref() == "23502" =>
                {
                    "IntegrityError"
                }
                _ => "OperationalError",
            },
            // Unreachable on the read path (see the variant docs).
            ToolExecError::Creation(_) => "OperationalError",
        }
    }

    /// `str(exc)` for the fatal tool result.
    pub fn message(&self) -> String {
        match self {
            ToolExecError::Failure(failure) => failure.message().to_owned(),
            ToolExecError::Github(failure) => failure.message.clone(),
            ToolExecError::Missing { model } => {
                format!("{model} matching query does not exist.")
            }
            // `str(ValidationError)` is `repr(list(self))`: ASCII
            // single quotes around the curly-quoted message.
            ToolExecError::InvalidUuid { value } => {
                format!("['“{value}” is not a valid UUID.']")
            }
            ToolExecError::AttributeError { attribute } => {
                format!("'NoneType' object has no attribute '{attribute}'.")
            }
            ToolExecError::Db(error) => error.to_string(),
            ToolExecError::Creation(error) => error.to_string(),
        }
    }
}

impl From<plan::ToolCallFailure> for ToolExecError {
    fn from(failure: plan::ToolCallFailure) -> Self {
        ToolExecError::Failure(failure)
    }
}

impl From<plan::ToolDenied> for ToolExecError {
    fn from(denied: plan::ToolDenied) -> Self {
        ToolExecError::Failure(plan::ToolCallFailure::Denied(denied))
    }
}

impl From<GithubFailure> for ToolExecError {
    fn from(failure: GithubFailure) -> Self {
        ToolExecError::Github(failure)
    }
}

impl From<sqlx::Error> for ToolExecError {
    fn from(error: sqlx::Error) -> Self {
        ToolExecError::Db(error)
    }
}

impl std::fmt::Display for ToolExecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message())
    }
}

impl std::error::Error for ToolExecError {}

/// `uuid.UUID(hex=value)` acceptance (`UUIDField.to_python`, Django
/// `fields/__init__.py:2681-2693`): strip every `urn:`/`uuid:`
/// fragment, strip brace ends, drop all hyphens, require exactly 32
/// chars, then `int(hex, 16)` (hex digits with single `_` separators,
/// never leading/trailing/doubled). Probed on CPython 3.12: simple,
/// braced, lowercase-`urn:`, misplaced-hyphen, and double-braced
/// spellings accept; uppercase `URN:`, whitespace, and bad lengths
/// reject. The value parses through `u128` (not the hyphenated
/// parser) so the 31-digits-plus-underscore shape accepts exactly as
/// `int()` does.
fn parse_django_uuid(text: &str) -> Option<Uuid> {
    let hex = text.replace("urn:", "").replace("uuid:", "");
    let hex = hex
        .trim_matches(|char| char == '{' || char == '}')
        .replace('-', "");
    if hex.chars().count() != 32 {
        return None;
    }
    if hex.starts_with('_') || hex.ends_with('_') {
        return None;
    }
    let mut digits = String::with_capacity(32);
    let mut previous_underscore = false;
    for char in hex.chars() {
        if char == '_' {
            if previous_underscore {
                return None;
            }
            previous_underscore = true;
            continue;
        }
        previous_underscore = false;
        if !char.is_ascii_hexdigit() {
            return None;
        }
        digits.push(char);
    }
    u128::from_str_radix(&digits, 16).ok().map(Uuid::from_u128)
}

// ---------------------------------------------------------------------------
// Context + scope
// ---------------------------------------------------------------------------

/// The tool executor's static inputs: pool, settings, model-run HTTP
/// client (unused here; the GitHub leg uses the app-context client),
/// and the GitHub App context (pool + cache + secret + API base).
#[derive(Clone)]
pub struct ToolCtx {
    /// Worker primary pool (reads, ledger, audit transactions).
    pub pool: PgPool,
    /// The shared SeaORM connection (stage 8): domain writes run their
    /// transactions on this handle.
    pub orm: pidash_db::DatabaseConnection,
    /// `CLOUD_AGENT_*` (`audit_guards`, bounds, event cap).
    pub cloud: CloudAgentSettings,
    /// Managed-runner settings (transition store inputs).
    pub managed: ManagedRunnerSettings,
    /// HTTP client for the GitHub REST leg.
    pub http: reqwest::Client,
    /// Pool + cache + secret + API base for app auth.
    pub github: GithubAppContext,
    /// Injected cloud-agent toolsets seam (transition store inputs).
    pub toolsets: Arc<dyn CloudAgentToolsetsSeam>,
    /// Shared Redis client (`None` when `REDIS_URL` is unset or
    /// unparsable): the create fire's pod drain (PIDASHCONV-1494).
    pub redis: Option<redis::Client>,
    /// Runner settings (the create fire's pod drain inputs).
    pub runner: RunnerSettings,
    /// The worker's transition hook, registered on every domain write
    /// txn (PIDASHCONV-1596): domain fns resolve it eagerly even when
    /// the chain stays empty, and an unregistered lookup panics the
    /// task. The same instance the sync drivers hold.
    pub transition_hook: Arc<dyn IssueTransitionHook>,
}

/// The `_scope` outcome (`tools.py:46-73`): the run's tenant position
/// plus the resolved project.
pub struct ToolScope {
    /// The run id the tools close over.
    pub run_id: Uuid,
    /// `run.workspace_id`.
    pub workspace_id: Uuid,
    /// `run.workspace.slug` (both membership checks key off it).
    pub workspace_slug: String,
    /// `run.created_by_id` (actor for audit stamps + orchestration).
    pub creator_id: Uuid,
    /// `run.work_item_id` (bound issue, if any).
    pub work_item_id: Option<Uuid>,
    /// `run.scheduler_binding_id` (the create tool reads the
    /// binding's project off it).
    pub scheduler_binding_id: Option<Uuid>,
    /// The `_project_id` resolution (work item, else scheduler
    /// binding, else pod).
    pub project_id: Uuid,
}

/// The project-role gate (`check_project_role` on the slug-named
/// workspace): write allows admin/member `[20, 15]`, read adds guest
/// `[20, 15, 5]`; either bit also passes with any-role membership plus
/// workspace-admin (role 20). One statement, like the SQL gate.
async fn project_role_ok<C: sea_orm::ConnectionTrait>(
    conn: &C,
    user_id: Uuid,
    workspace_slug: &str,
    project_id: Uuid,
    write: bool,
) -> Result<bool, ToolExecError> {
    use pidash_db::entity::{
        project_members as pm_entity, workspace_members as wm_entity, workspaces as ws_entity,
    };
    let roles: &[i16] = if write { &[20, 15] } else { &[20, 15, 5] };
    let admin_exists = Expr::exists(
        sea_orm::sea_query::Query::select()
            .expr(Expr::value(1))
            .from(wm_entity::Entity)
            .join(
                JoinType::InnerJoin,
                ws_entity::Entity,
                Expr::col((ws_entity::Entity, ws_entity::Column::Id))
                    .equals((wm_entity::Entity, wm_entity::Column::WorkspaceId)),
            )
            .and_where(Expr::col((wm_entity::Entity, wm_entity::Column::MemberId)).eq(user_id))
            .and_where(Expr::col((ws_entity::Entity, ws_entity::Column::Slug)).eq(workspace_slug))
            .and_where(Expr::col((wm_entity::Entity, wm_entity::Column::Role)).eq(20))
            .and_where(Expr::col((wm_entity::Entity, wm_entity::Column::IsActive)).eq(true))
            .and_where(Expr::col((wm_entity::Entity, wm_entity::Column::DeletedAt)).is_null())
            .to_owned(),
    );
    let row = pm_entity::Entity::find()
        .join(JoinType::InnerJoin, pm_entity::Relation::Workspaces.def())
        .filter(pm_entity::Column::MemberId.eq(user_id))
        .filter(ws_entity::Column::Slug.eq(workspace_slug))
        .filter(pm_entity::Column::ProjectId.eq(project_id))
        .filter(pm_entity::Column::IsActive.eq(true))
        .filter(pm_entity::Column::DeletedAt.is_null())
        .filter(
            sea_orm::sea_query::Condition::any()
                .add(pm_entity::Column::Role.is_in(roles.to_vec()))
                .add(admin_exists),
        )
        .select_only()
        .column(pm_entity::Column::Id)
        .into_tuple::<Uuid>()
        .one(conn)
        .await
        .map_err(orm_error)?;
    Ok(row.is_some())
}

/// `SCOPE_RUN_SQL` beside-index-free form: joined once, with every
/// same-named `project_id` under its own alias.
#[derive(Debug, FromQueryResult)]
struct ScopeRow {
    workspace_id: Uuid,
    created_by_id: Uuid,
    work_item_id: Option<Uuid>,
    scheduler_binding_id: Option<Uuid>,
    cancel_requested_at: Option<DateTime<Utc>>,
    is_active: bool,
    is_bot: bool,
    slug: String,
    work_item_project_id: Option<Uuid>,
    scheduler_project_id: Option<Uuid>,
    pod_project_id: Option<Uuid>,
}

/// One `FIND_EXISTING_CALL_SQL` row: latest-by-id ledger state.
#[derive(Clone, Debug, PartialEq, FromQueryResult)]
struct LedgerExisting {
    request_fingerprint: String,
    status: String,
    safe_replay_result: Option<Value>,
}

/// `_scope` (`tools.py:46-73`) on one SeaORM connection: the audit
/// transaction handle on the write path, the pool handle on the read
/// path. A missing run row is `AgentRun.DoesNotExist` (`.get()`,
/// `query.py:639-640`); a dangling work-item/scheduler-binding
/// back-pointer reads its project as the `AttributeError` Python's
/// `run.work_item.project` raises.
async fn load_scope<C: sea_orm::ConnectionTrait>(
    conn: &C,
    run_id: Uuid,
    write: bool,
) -> Result<ToolScope, ToolExecError> {
    use pidash_db::entity::{
        agent_run as run_entity, issues as issue_entity, pod as pod_entity,
        scheduler_bindings as sb_entity, users as u_entity, workspaces as ws_entity,
    };
    let row = run_entity::Entity::find_by_id(run_id)
        .select_only()
        .column(run_entity::Column::WorkspaceId)
        .column(run_entity::Column::CreatedById)
        .column(run_entity::Column::WorkItemId)
        .column(run_entity::Column::SchedulerBindingId)
        .column(run_entity::Column::CancelRequestedAt)
        .column_as(u_entity::Column::IsActive, "is_active")
        .column_as(u_entity::Column::IsBot, "is_bot")
        .column_as(ws_entity::Column::Slug, "slug")
        .column_as(issue_entity::Column::ProjectId, "work_item_project_id")
        .column_as(sb_entity::Column::ProjectId, "scheduler_project_id")
        .column_as(pod_entity::Column::ProjectId, "pod_project_id")
        .join(
            JoinType::InnerJoin,
            run_entity::Entity::belongs_to(u_entity::Entity)
                .from(run_entity::Column::CreatedById)
                .to(u_entity::Column::Id)
                .into(),
        )
        .join(
            JoinType::InnerJoin,
            run_entity::Entity::belongs_to(ws_entity::Entity)
                .from(run_entity::Column::WorkspaceId)
                .to(ws_entity::Column::Id)
                .into(),
        )
        .join(
            JoinType::LeftJoin,
            run_entity::Entity::belongs_to(issue_entity::Entity)
                .from(run_entity::Column::WorkItemId)
                .to(issue_entity::Column::Id)
                .into(),
        )
        .join(
            JoinType::LeftJoin,
            run_entity::Entity::belongs_to(sb_entity::Entity)
                .from(run_entity::Column::SchedulerBindingId)
                .to(sb_entity::Column::Id)
                .into(),
        )
        .join(
            JoinType::InnerJoin,
            run_entity::Entity::belongs_to(pod_entity::Entity)
                .from(run_entity::Column::PodId)
                .to(pod_entity::Column::Id)
                .into(),
        )
        .into_model::<ScopeRow>()
        .one(conn)
        .await
        .map_err(orm_error)?;
    let Some(row) = row else {
        return Err(ToolExecError::Missing { model: "AgentRun" });
    };
    let ScopeRow {
        workspace_id,
        created_by_id: creator_id,
        work_item_id,
        scheduler_binding_id,
        cancel_requested_at,
        is_active: creator_active,
        is_bot: creator_is_bot,
        slug: workspace_slug,
        work_item_project_id,
        scheduler_project_id,
        pod_project_id,
    } = row;

    use pidash_db::entity::workspace_members as wm_entity;
    let workspace_member = wm_entity::Entity::find()
        .filter(wm_entity::Column::MemberId.eq(creator_id))
        .filter(wm_entity::Column::WorkspaceId.eq(workspace_id))
        .filter(wm_entity::Column::IsActive.eq(true))
        .filter(wm_entity::Column::DeletedAt.is_null())
        .select_only()
        .column(wm_entity::Column::Id)
        .into_tuple::<Uuid>()
        .one(conn)
        .await
        .map_err(orm_error)?
        .is_some();
    // The project id gates the role check, but the source (hence which
    // column) resolves inside `check_scope` after the actor gates: read
    // the candidate per back-pointer first, then verify the winner.
    let source_hint = plan::scope_project_source(work_item_id, scheduler_binding_id);
    let project_id = match source_hint {
        plan::ProjectSource::WorkItem => work_item_project_id,
        plan::ProjectSource::SchedulerBinding => scheduler_project_id,
        plan::ProjectSource::Pod => pod_project_id,
    };
    // A dangling back-pointer (`select_related` miss) reads `None`,
    // and `run.work_item.project` raises before the role query runs.
    let Some(project_id) = project_id else {
        return Err(ToolExecError::AttributeError {
            attribute: "project",
        });
    };
    // The role gate: project membership (role list per bit) on the
    // slug-named workspace, or any-role membership plus workspace admin
    // (role 20). `EXISTS`-shaped, exactly like the SQL gates it replaces.
    let role_ok =
        project_role_ok(conn, creator_id, workspace_slug.as_str(), project_id, write).await?;
    let actor = plan::ScopeActorVerdicts {
        cancel_requested: cancel_requested_at.is_some(),
        creator_active,
        creator_is_bot,
        workspace_member,
    };
    let source = plan::check_scope(work_item_id, scheduler_binding_id, &actor, role_ok)?;
    debug_assert_eq!(source, source_hint);
    Ok(ToolScope {
        run_id,
        workspace_id,
        workspace_slug,
        creator_id,
        work_item_id,
        scheduler_binding_id,
        project_id,
    })
}

// ---------------------------------------------------------------------------
// Audit (`_audit`, `tools.py:76-157`)
// ---------------------------------------------------------------------------

/// Read-path `_audit` head (`tools.py:76-95,131-142`): fresh call id,
/// fingerprint, kill-switch guards, replay check, then scope (before
/// the insert) and the submitted ledger row. Returns the replay value
/// short-circuit or the scope plus ledger id.
pub enum ReadAudit {
    /// Run the op; mark the ledger row on the way out.
    Proceed {
        /// The read-bit scope.
        scope: ToolScope,
        /// The submitted ledger row id.
        ledger_id: Uuid,
    },
    /// Return the stored replay value (no ledger write, no event).
    Replay(Value),
}

pub async fn read_begin(
    ctx: &ToolCtx,
    run_id: Uuid,
    tool_name: &'static str,
    args: &Value,
    source: &'static str,
    server_key: &'static str,
) -> Result<ReadAudit, ToolExecError> {
    let call_id = plan::resolve_tool_call_id(None);
    let fingerprint = plan::fingerprint(args);
    plan::audit_guards(
        &ctx.cloud,
        tool_name,
        plan::ToolRisk::Read,
        source,
        server_key,
    )?;
    let existing = find_existing(&ctx.pool, run_id, &call_id).await?;
    match plan::replay_decision(existing.as_ref(), &fingerprint)? {
        plan::ReplayDecision::Replay(value) => return Ok(ReadAudit::Replay(value)),
        plan::ReplayDecision::Proceed => {}
    }
    let db = sea_conn(&ctx.pool);
    let scope = load_scope(&db, run_id, false).await?;
    let ledger_id = Uuid::new_v4();
    let now = Utc::now();
    use pidash_db::entity::agent_run_tool_call as ledger_entity;
    ledger_entity::ActiveModel {
        id: sea_orm::Set(ledger_id),
        agent_run_id: sea_orm::Set(run_id),
        tool_call_id: sea_orm::Set(call_id.to_owned()),
        source: sea_orm::Set(source.to_owned()),
        server_key: sea_orm::Set(server_key.to_owned()),
        tool_name: sea_orm::Set(tool_name.to_owned()),
        risk: sea_orm::Set("read".to_owned()),
        status: sea_orm::Set("submitted".to_owned()),
        request_fingerprint: sea_orm::Set(fingerprint),
        result_fingerprint: sea_orm::Set("".to_owned()),
        idempotency_key_hash: sea_orm::Set("".to_owned()),
        external_operation_id: sea_orm::Set("".to_owned()),
        safe_replay_result: sea_orm::Set(None),
        error_code: sea_orm::Set("".to_owned()),
        prepared_at: sea_orm::Set(sea_orm::prelude::DateTimeWithTimeZone::from(now)),
        submitted_at: sea_orm::Set(Some(sea_orm::prelude::DateTimeWithTimeZone::from(now))),
        completed_at: sea_orm::Set(None),
    }
    .insert(&db)
    .await
    .map_err(orm_error)?;
    Ok(ReadAudit::Proceed { scope, ledger_id })
}

/// Read-path `_audit` after the op (`tools.py:143-156`): on error mark
/// the row failed with `type(exc).__name__[:64]` and propagate (a
/// marking failure masks the op error, as Python's except-handler
/// raise does); on success bound, collapse, mark, and emit.
pub async fn read_settle(
    ctx: &ToolCtx,
    run_id: Uuid,
    tool_name: &'static str,
    ledger_id: Uuid,
    outcome: Result<Value, ToolExecError>,
) -> Result<Value, ToolExecError> {
    use pidash_db::entity::agent_run_tool_call as ledger_entity;
    let db = sea_conn(&ctx.pool);
    match outcome {
        Err(error) => {
            let now = Utc::now();
            ledger_entity::Entity::update_many()
                .col_expr(ledger_entity::Column::Status, Expr::value("failed"))
                .col_expr(
                    ledger_entity::Column::ErrorCode,
                    Expr::value(plan::failure_error_code(error.python_type())),
                )
                .col_expr(
                    ledger_entity::Column::CompletedAt,
                    Expr::value(sea_orm::prelude::DateTimeWithTimeZone::from(now)),
                )
                .filter(ledger_entity::Column::Id.eq(ledger_id))
                .exec(&db)
                .await
                .map_err(orm_error)?;
            Err(error)
        }
        Ok(result) => {
            let bounded = plan::bounded(result.clone(), ctx.cloud.max_tool_result_bytes)?;
            let replay = if plan::collapse_replay(plan::canonical(&bounded).len()) {
                plan::collapsed_replay()
            } else {
                bounded.clone()
            };
            let result_fingerprint = plan::fingerprint(&bounded);
            let now = Utc::now();
            ledger_entity::Entity::update_many()
                .col_expr(ledger_entity::Column::Status, Expr::value("succeeded"))
                .col_expr(ledger_entity::Column::SafeReplayResult, Expr::value(replay))
                .col_expr(
                    ledger_entity::Column::ResultFingerprint,
                    Expr::value(result_fingerprint),
                )
                .col_expr(
                    ledger_entity::Column::CompletedAt,
                    Expr::value(sea_orm::prelude::DateTimeWithTimeZone::from(now)),
                )
                .filter(ledger_entity::Column::Id.eq(ledger_id))
                .exec(&db)
                .await
                .map_err(orm_error)?;
            emit_completed(ctx, run_id, tool_name, plan::ToolRisk::Read).await?;
            Ok(result)
        }
    }
}

/// The existing-ledger lookup (`tools.py:89` → [`plan::ExistingCall`]).
async fn find_existing(
    pool: &PgPool,
    run_id: Uuid,
    call_id: &str,
) -> Result<Option<plan::ExistingCall>, ToolExecError> {
    use pidash_db::entity::agent_run_tool_call as ledger_entity;
    let db = sea_conn(pool);
    let row = ledger_entity::Entity::find()
        .filter(ledger_entity::Column::AgentRunId.eq(run_id))
        .filter(ledger_entity::Column::ToolCallId.eq(call_id))
        .order_by_desc(ledger_entity::Column::Id)
        .limit(1)
        .into_model::<LedgerExisting>()
        .one(&db)
        .await
        .map_err(orm_error)?;
    row.map(|row| {
        // A junk stored status compares unequal to `succeeded` in
        // Python (→ `already_submitted`); `Unknown` preserves that arm.
        let status = pidash_db::dispatch::ToolCallStatus::from_value(&row.status)
            .unwrap_or(pidash_db::dispatch::ToolCallStatus::Unknown);
        plan::ExistingCall {
            request_fingerprint: row.request_fingerprint,
            status,
            safe_replay_result: row.safe_replay_result,
        }
    })
    .map(Ok)
    .transpose()
}

/// `events.append(run_id, "tool_completed", ...)` (`tools.py:156`).
async fn emit_completed(
    ctx: &ToolCtx,
    run_id: Uuid,
    tool_name: &str,
    risk: plan::ToolRisk,
) -> Result<(), ToolExecError> {
    let (kind, payload) = plan::tool_completed_event(tool_name, risk);
    let payload = serde_json::to_value(payload).expect("tool-completed payload serializes");
    append_event(
        &ctx.pool,
        ctx.cloud.max_events,
        run_id,
        kind,
        Some(payload),
        Utc::now(),
    )
    .await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Shared reads
// ---------------------------------------------------------------------------

// The bound-issue `select_related("project", "state")` read lives in
// `pidash_services::dispatch::tool_issues` (SeaORM): a plain PK lookup joining
// the project (inner) and state (outer), with NO soft-delete predicate (FK
// follows do not apply the related manager's filter), every joined column
// under its own alias.

/// One bound-issue row: the `_issue_data` inputs.
pub type BoundIssue = ToolIssueRow;

/// Map a shared-read issue view onto the tool row shape the brief
/// renderers consume.
fn tool_row_of(view: &pidash_services::operations::read::IssueView) -> ToolIssueRow {
    ToolIssueRow {
        id: view.id,
        workspace_id: view.workspace_id,
        project_id: view.project_id,
        project_identifier: view.project_identifier.clone(),
        sequence_id: view.sequence_id,
        name: view.name.clone(),
        state_name: view.state_name.clone(),
        state_group: view.state_group.clone(),
        priority: view.priority.clone(),
        description_stripped: view.description_stripped.clone(),
        workpad: view.workpad.clone().unwrap_or_default(),
    }
}

/// `_issue_data(issue, detail=True)` (`tools.py:160-171`) over a
/// shared-read issue view.
fn issue_detail_view(view: &pidash_services::operations::read::IssueView) -> Value {
    let detail = plan::issue_data(
        &view.id.to_string(),
        &view.project_identifier,
        i64::from(view.sequence_id),
        &view.name,
        view.state_name.as_deref(),
        view.state_group.as_deref(),
        &view.priority,
        Some((
            view.description_stripped.as_deref().unwrap_or(""),
            view.workpad.as_deref().unwrap_or(""),
        )),
    );
    serde_json::to_value(detail).expect("issue detail serializes")
}

/// The grouped relations for the run's source over the shared read
/// (`relations.py:280-312`), narrowed to the run's visible pool and
/// rendered with `operations::relations`' renderer.
async fn tool_grouped(
    conn: &impl sea_orm::ConnectionTrait,
    scope: &ToolScope,
    source: &rel::ResolvedIssue,
) -> Result<Value, pidash_services::operations::core::CoreError> {
    let visibility = pidash_services::operations::read::Visibility {
        creator: scope.creator_id,
        slug: workspace_slug_str(scope).to_owned(),
    };
    let grouped =
        pidash_services::operations::read::grouped_relations(conn, source, Some(&visibility))
            .await?;
    let mut out = serde_json::Map::with_capacity(grouped.map.len());
    for (relation, items) in grouped.map.iter() {
        out.insert(
            relation.clone(),
            Value::Array(
                items
                    .iter()
                    .map(relations_ops::render_grouped_item_ref)
                    .collect(),
            ),
        );
    }
    Ok(Value::Object(out))
}

/// Read the bound issue, if the run has one. A missing row reads as
/// `None` (the `select_related` miss), exactly like a run with no
/// bound issue — callers that dereference it raise the
/// `AttributeError` Python's attribute access raises.
async fn bound_issue(
    db: &pidash_db::DatabaseConnection,
    work_item_id: Option<Uuid>,
) -> Result<Option<BoundIssue>, ToolExecError> {
    let Some(issue_id) = work_item_id else {
        return Ok(None);
    };
    let view = pidash_services::operations::read::issue_by_id(db, issue_id, None, false)
        .await
        .map_err(core_error)?;
    Ok(view.map(|view| tool_row_of(&view)))
}

// ---------------------------------------------------------------------------
// Read ops
// ---------------------------------------------------------------------------

/// `pidash_get_current_issue` (`tools.py:200-202`).
async fn op_get_current_issue(ctx: &ToolCtx, scope: &ToolScope) -> Result<Value, ToolExecError> {
    // The shared read (NEWPIDASH-190 §4.6): the bound-issue lookup has
    // no `deleted_at` filter, which the shared `issue_by_id` preserves
    // with `live_only: false`.
    let Some(work_item_id) = scope.work_item_id else {
        return Err(ToolExecError::AttributeError { attribute: "id" });
    };
    let view = pidash_services::operations::read::issue_by_id(&ctx.orm, work_item_id, None, false)
        .await
        .map_err(core_error)?;
    let Some(view) = view else {
        return Err(ToolExecError::AttributeError { attribute: "id" });
    };
    Ok(issue_detail_view(&view))
}

/// `pidash_list_current_issue_comments` (`tools.py:204-213`).
async fn op_list_current_issue_comments(
    ctx: &ToolCtx,
    scope: &ToolScope,
) -> Result<Value, ToolExecError> {
    let Some(work_item_id) = scope.work_item_id else {
        return Err(ToolExecError::AttributeError {
            attribute: "issue_comments",
        });
    };
    // A hard-deleted bound issue still lists (the related manager
    // filters comments, not the issue): no liveness probe here.
    let rows = pidash_services::operations::read::list_comments(
        &ctx.orm,
        work_item_id,
        pidash_services::operations::read::ListOrder::Desc,
        50,
    )
    .await
    .map_err(core_error)?;
    let mut out = Vec::with_capacity(rows.len());
    for row in &rows {
        out.push(plan::comment_row(
            &row.id.to_string(),
            row.comment_stripped.as_deref(),
            &render_isoformat(row.created_at),
        ));
    }
    serde_json::to_value(out)
        .map_err(|error| ToolExecError::Failure(plan::ToolCallFailure::Failed(error.to_string())))
}

/// `pidash_list_project_states` (`tools.py:215-225`).
async fn op_list_project_states(ctx: &ToolCtx, scope: &ToolScope) -> Result<Value, ToolExecError> {
    let rows = pidash_services::operations::read::list_states(&ctx.orm, scope.project_id, false)
        .await
        .map_err(core_error)?;
    let mut out = Vec::with_capacity(rows.len());
    for row in &rows {
        out.push(plan::StateRow {
            id: row.id.to_string(),
            name: row.name.clone(),
            group: row.group.clone(),
        });
    }
    serde_json::to_value(out)
        .map_err(|error| ToolExecError::Failure(plan::ToolCallFailure::Failed(error.to_string())))
}

/// `pidash_search_project_issues` (`tools.py:227-241`).
async fn op_search_project_issues(
    ctx: &ToolCtx,
    scope: &ToolScope,
    query: &str,
    limit: i64,
) -> Result<Value, ToolExecError> {
    let rows = pidash_services::operations::read::search_issues(
        &ctx.orm,
        Some(scope.project_id),
        &escape_icontains(query),
        u64::try_from(limit).unwrap_or(0),
    )
    .await
    .map_err(core_error)?;
    let mut out = Vec::with_capacity(rows.len());
    for row in &rows {
        let detail = plan::issue_data(
            &row.id.to_string(),
            &row.project_identifier,
            i64::from(row.sequence_id),
            &row.name,
            row.state_name.as_deref(),
            row.state_group.as_deref(),
            &row.priority,
            None,
        );
        out.push(plan::issue_brief(&detail));
    }
    serde_json::to_value(out)
        .map_err(|error| ToolExecError::Failure(plan::ToolCallFailure::Failed(error.to_string())))
}

/// `pidash_get_project_issue` (`tools.py:243-251`).
async fn op_get_project_issue(
    ctx: &ToolCtx,
    scope: &ToolScope,
    issue_id: &str,
) -> Result<Value, ToolExecError> {
    let Some(id) = parse_django_uuid(issue_id) else {
        return Err(ToolExecError::InvalidUuid {
            value: issue_id.to_owned(),
        });
    };
    // `project_issue` filters out soft-deleted rows (`live_only:
    // true`); a miss reads as the tool `DoesNotExist`.
    let view =
        pidash_services::operations::read::issue_by_id(&ctx.orm, id, Some(scope.project_id), true)
            .await
            .map_err(core_error)?;
    let Some(view) = view else {
        return Err(ToolExecError::Missing { model: "Issue" });
    };
    Ok(issue_detail_view(&view))
}

/// `pidash_list_linked_code_reviews` (`tools.py:253-272`): a run with
/// no bound issue filters `issue_id=None` and lists nothing — no
/// error.
async fn op_list_linked_code_reviews(
    ctx: &ToolCtx,
    scope: &ToolScope,
) -> Result<Value, ToolExecError> {
    let Some(work_item_id) = scope.work_item_id else {
        return Ok(Value::Array(Vec::new()));
    };
    let rows = pidash_services::operations::read::linked_reviews(
        &ctx.orm,
        work_item_id,
        plan::LINKED_REVIEWS_LIMIT as u64,
    )
    .await
    .map_err(core_error)?;
    let mut out = Vec::with_capacity(rows.len());
    for row in &rows {
        out.push(plan::LinkedReviewRow {
            id: row.id.to_string(),
            provider: row.provider.clone(),
            url: row.url.clone(),
            title: row.title.clone(),
            state: row.state.clone(),
            number: row.number.clone(),
        });
    }
    serde_json::to_value(out)
        .map_err(|error| ToolExecError::Failure(plan::ToolCallFailure::Failed(error.to_string())))
}

// ---------------------------------------------------------------------------
// Write ops (each runs inside its [`WriteAudit`] transaction)
// ---------------------------------------------------------------------------

/// `pidash_add_current_issue_comment` (`tools.py:274-294`). The body is
/// pre-validated by the dispatch; the op dereferences the bound issue
/// (plus its project) and inserts the comment, its `Description` side
/// row, and the link. `impersonate(run.created_by)` surfaces as the
/// explicit actor binds.
async fn op_add_current_issue_comment(
    txn: &mut WriteTxn,
    scope: &ToolScope,
    body: &str,
) -> Result<Value, ToolExecError> {
    let issue = bound_issue_wtxn(txn, scope.work_item_id).await?;
    let Some(issue) = issue else {
        return Err(ToolExecError::AttributeError {
            attribute: "project",
        });
    };
    let html = to_safe_html(Some(body));
    // The shared core (NEWPIDASH-190 §4.3): the same three domain row
    // writes with the per-writer fields riding as the input.
    // `desc_stripped` is the `Description.save` recompute (Django's
    // own `strip_tags`, entities verbatim, `None`-when-empty —
    // `description.py:23-29`), not the comment's ML text
    // (PIDASHCONV-1581 item 1).
    let write_ctx = write_ctx(scope, issue.project_id);
    let saved = pidash_services::operations::core::comments::create(
        txn,
        &write_ctx,
        core_comments::CommentCreate {
            issue_id: issue.id,
            project_id: issue.project_id,
            workspace_id: scope.workspace_id,
            html,
            speaker: pidash_services::operations::core::comments::Speaker::Agent {
                run_id: scope.run_id,
            },
            access: None,
            external: core_comments::CommentExternal::default(),
            created_by: None,
            created_at: chrono::Utc::now(),
            json: None,
            labels: None,
        },
        None,
    )
    .await
    .map_err(|error| match error {
        pidash_services::operations::core::CoreError::Request(message) => {
            ToolExecError::Failure(plan::ToolCallFailure::InvalidArg(message))
        }
        pidash_services::operations::core::CoreError::Missing(model) => {
            ToolExecError::Missing { model }
        }
        pidash_services::operations::core::CoreError::Db(domain_error) => {
            ToolExecError::Db(sqlx::Error::Protocol(domain_error.to_string()))
        }
        pidash_services::operations::core::CoreError::Sqlx(sqlx_error) => {
            ToolExecError::Db(sqlx_error)
        }
    })?;
    let comment_id = saved.id;
    let created = plan::comment_created(&comment_id.to_string());
    serde_json::to_value(created)
        .map_err(|error| ToolExecError::Failure(plan::ToolCallFailure::Failed(error.to_string())))
}

/// The bound-issue read over a [`WriteTxn`] (same shape as
/// [`bound_issue`]): the shared read joins the SeaORM query
/// directly, so no sqlx statement is needed.
async fn bound_issue_wtxn(
    txn: &WriteTxn,
    work_item_id: Option<Uuid>,
) -> Result<Option<BoundIssue>, ToolExecError> {
    let Some(issue_id) = work_item_id else {
        return Ok(None);
    };
    let view =
        pidash_services::operations::read::issue_by_id(&txn.connection(), issue_id, None, false)
            .await
            .map_err(core_error)?;
    Ok(view.map(|view| tool_row_of(&view)))
}

/// A SeaORM read failure as the tool error the old sqlx read raised: the
/// wrapped sqlx error when there is one (so Postgres codes still classify),
/// otherwise its message.
/// A shared-core error as the tool error the write ops map it to: a
/// request refusal is an `InvalidArg`, a row miss the `DoesNotExist`,
/// and a database failure the wrapped sqlx error.
fn core_error(error: pidash_services::operations::core::CoreError) -> ToolExecError {
    match error {
        pidash_services::operations::core::CoreError::Request(message) => {
            ToolExecError::Failure(plan::ToolCallFailure::InvalidArg(message))
        }
        pidash_services::operations::core::CoreError::Missing(model) => {
            ToolExecError::Missing { model }
        }
        pidash_services::operations::core::CoreError::Db(domain_error) => {
            ToolExecError::Db(sqlx::Error::Protocol(domain_error.to_string()))
        }
        pidash_services::operations::core::CoreError::Sqlx(sqlx_error) => {
            ToolExecError::Db(sqlx_error)
        }
    }
}

fn orm_error(error: sea_orm::DbErr) -> ToolExecError {
    match error {
        sea_orm::DbErr::Query(sea_orm::RuntimeErr::SqlxError(inner))
        | sea_orm::DbErr::Exec(sea_orm::RuntimeErr::SqlxError(inner))
        | sea_orm::DbErr::Conn(sea_orm::RuntimeErr::SqlxError(inner)) => ToolExecError::Db(inner),
        sea_orm::DbErr::ConnectionAcquire(sea_orm::ConnAcquireErr::Timeout) => {
            ToolExecError::Db(sqlx::Error::PoolTimedOut)
        }
        sea_orm::DbErr::ConnectionAcquire(sea_orm::ConnAcquireErr::ConnectionClosed) => {
            ToolExecError::Db(sqlx::Error::PoolClosed)
        }
        other => ToolExecError::Db(sqlx::Error::Protocol(other.to_string())),
    }
}

fn write_ctx(scope: &ToolScope, project_id: Uuid) -> WriteCtx {
    WriteCtx::new(WriteActor {
        user_id: scope.creator_id,
        timezone: None,
    })
    .with_scope(WriteScope {
        workspace_id: Some(scope.workspace_id),
        project_id: Some(project_id),
    })
    .with_options(WriteOptions {
        source: pidash_services::domain::WriteSource::CloudAgent {
            run_id: scope.run_id,
        },
    })
}

/// `pidash_update_current_issue_workpad` (`tools.py:296-307`). A run
/// with no bound issue updates zero rows (`pk=None`) and still reports
/// `updated`, with `issue_id` rendering Python's `str(None)`. The write
/// rides `domain::issues::update_workpad` — the `QuerySet.update`
/// equivalent, bare columns with no save signals — inside the audit
/// transaction, exactly where the inline `UPDATE` ran.
async fn op_update_current_issue_workpad(
    ctx: &ToolCtx,
    txn: &mut WriteTxn,
    scope: &ToolScope,
    body: &str,
) -> Result<Value, ToolExecError> {
    if let Some(work_item_id) = scope.work_item_id {
        // `run.work_item.project_id` (the raw attname, not the join).
        let issue = bound_issue_wtxn(txn, scope.work_item_id).await?;
        let Some(issue) = issue else {
            return Err(ToolExecError::AttributeError {
                attribute: "project_id",
            });
        };
        // The shared core (NEWPIDASH-190 §4.3): the bare two-column
        // write, through `core::workpad::set` (Bare mode).
        let write_ctx = write_ctx(scope, issue.project_id);
        let _saved = pidash_services::operations::core::workpad::set(
            txn,
            &write_ctx,
            &ctx.pool,
            pidash_services::operations::core::workpad::WorkpadSet {
                issue_id: work_item_id,
                project_id: issue.project_id,
                slug: String::new(),
                body: body.to_owned(),
                mode: pidash_services::operations::core::workpad::WorkpadMode::Bare,
            },
        )
        .await
        .map_err(|error| match error {
            pidash_services::operations::core::CoreError::Request(message) => {
                ToolExecError::Failure(plan::ToolCallFailure::InvalidArg(message))
            }
            pidash_services::operations::core::CoreError::Missing(model) => {
                ToolExecError::Missing { model }
            }
            pidash_services::operations::core::CoreError::Db(domain_error) => {
                ToolExecError::Db(sqlx::Error::Protocol(domain_error.to_string()))
            }
            pidash_services::operations::core::CoreError::Sqlx(sqlx_error) => {
                ToolExecError::Db(sqlx_error)
            }
        })?;
        let updated = plan::workpad_updated(&work_item_id.to_string());
        serde_json::to_value(updated).map_err(|error| {
            ToolExecError::Failure(plan::ToolCallFailure::Failed(error.to_string()))
        })
    } else {
        let updated = plan::workpad_updated("None");
        serde_json::to_value(updated).map_err(|error| {
            ToolExecError::Failure(plan::ToolCallFailure::Failed(error.to_string()))
        })
    }
}

/// The orchestration state view (the `STATE_SELECT_SQL` read).
async fn state_view_tx<C: sea_orm::ConnectionTrait>(
    conn: &C,
    state_id: Uuid,
) -> Result<Option<pidash_services::orchestration::creation::StateView>, ToolExecError> {
    use pidash_db::entity::states as st_entity;
    let row = st_entity::Entity::find_by_id(state_id)
        .into_model::<IssueStateView>()
        .one(conn)
        .await
        .map_err(orm_error)?;
    Ok(
        row.map(|row| pidash_services::orchestration::creation::StateView {
            id: row.id,
            name: row.name,
            group: row.group,
        }),
    )
}

/// One `STATE_SELECT_SQL` row.
#[derive(Clone, Debug, PartialEq, FromQueryResult)]
struct IssueStateView {
    id: Uuid,
    name: String,
    group: String,
}

/// The create-path deferred project lock (`CREATE_PROJECT_LOCK_SQL`):
/// the live project row under `FOR UPDATE` (a soft-deleted project
/// errors as `Project.DoesNotExist`, matching Django's
/// `SoftDeletionManager`).
fn project_lock_query(project_id: Uuid) -> sea_orm::Select<pidash_db::entity::projects::Entity> {
    use pidash_db::entity::projects as proj_entity;
    proj_entity::Entity::find_by_id(project_id)
        .filter(proj_entity::Column::DeletedAt.is_null())
        .lock_exclusive()
}

/// The transition target lookup (`State.objects.get(pk=..., project_id=...)`
/// in `tools.py:320-345`): the live, non-triage state row in the run's
/// project, projecting every `IssueStateView` field (`id`, `name`,
/// `group`) so the `FromQueryResult` decode succeeds. Extracted so a
/// unit test can pin the projection.
fn transition_state_query(
    target_id: Uuid,
    project_id: Uuid,
) -> sea_orm::Select<pidash_db::entity::states::Entity> {
    use pidash_db::entity::states as st_entity;
    st_entity::Entity::find()
        .filter(st_entity::Column::Id.eq(target_id))
        .filter(st_entity::Column::ProjectId.eq(project_id))
        .filter(st_entity::Column::DeletedAt.is_null())
        .filter(Expr::col(st_entity::Column::Group).ne("triage"))
        .select_only()
        .column(st_entity::Column::Id)
        .column(st_entity::Column::Name)
        .column(st_entity::Column::Group)
}

/// `pidash_create_project_issue` (`tools.py:345-371`) through
/// `Issue.save` (`db/models/issue.py:267-341`): lock the scheduler
/// project, resolve the default/backlog state (plus the save-time
/// repair when the tool resolved none), run the discarded sequence
/// probe, take the advisory lock, scan the effective sequence and
/// sort order, resolve the default pod, stamp `completed_at` only for
/// a tool-supplied completed state, insert the issue plus its
/// sequence row, and return the brief. The title/description are
/// pre-validated by the dispatch; `stripped_title` is what stores.
/// The save rides the audit [`WriteTxn`] (NEWPIDASH-190 §4.4 phase
/// 1.5): the post-commit orchestration fire is queued with
/// [`WriteTxn::on_commit`] and drains after the audit commits.
async fn op_create_project_issue(
    ctx: &ToolCtx,
    txn: &mut WriteTxn,
    scope: &ToolScope,
    description: &str,
    stripped_title: &str,
) -> Result<Value, ToolExecError> {
    // `run.scheduler_binding.project_id` (the raw attname): the tool
    // uses the binding's project even when the run also has a bound
    // issue in another project.
    let Some(binding_id) = scope.scheduler_binding_id else {
        return Err(ToolExecError::AttributeError {
            attribute: "project_id",
        });
    };
    use pidash_db::entity::{
        issues as issue_entity, projects as proj_entity, scheduler_bindings as sb_entity,
        states as st_entity,
    };
    let binding_project_id = sb_entity::Entity::find_by_id(binding_id)
        .select_only()
        .column(sb_entity::Column::ProjectId)
        .into_tuple::<Uuid>()
        .one(&txn.connection())
        .await
        .map_err(orm_error)?;
    let Some(project_id) = binding_project_id else {
        return Err(ToolExecError::AttributeError {
            attribute: "project_id",
        });
    };
    // Existence first, lock later: Django takes `select_for_update`
    // here (`tools.py:351`) and saves in the same transaction. The
    // save now rides the audit `WriteTxn` itself (no second domain
    // transaction), so a single-transaction `FOR UPDATE` throughout
    // is holdable; the lock still lands after the save, inside the
    // audit transaction.
    let exists = proj_entity::Entity::find_by_id(project_id)
        .filter(proj_entity::Column::DeletedAt.is_null())
        .select_only()
        .column(proj_entity::Column::Id)
        .into_tuple::<Uuid>()
        .one(&txn.connection())
        .await
        .map_err(orm_error)?;
    if exists.is_none() {
        return Err(ToolExecError::Missing { model: "Project" });
    }
    let tool_state = st_entity::Entity::find()
        .filter(st_entity::Column::ProjectId.eq(project_id))
        .filter(st_entity::Column::Default.eq(true))
        .filter(st_entity::Column::DeletedAt.is_null())
        .filter(Expr::col(st_entity::Column::Group).ne("triage"))
        .order_by_asc(st_entity::Column::Sequence)
        .limit(1)
        .select_only()
        .column(st_entity::Column::Id)
        .into_tuple::<Uuid>()
        .one(&txn.connection())
        .await
        .map_err(orm_error)?;
    let tool_state = match tool_state {
        Some(state) => Some(state),
        None => st_entity::Entity::find()
            .filter(st_entity::Column::ProjectId.eq(project_id))
            .filter(st_entity::Column::Group.eq("backlog"))
            .filter(st_entity::Column::DeletedAt.is_null())
            .filter(Expr::col(st_entity::Column::Group).ne("triage"))
            .order_by_asc(st_entity::Column::Sequence)
            .limit(1)
            .select_only()
            .column(st_entity::Column::Id)
            .into_tuple::<Uuid>()
            .one(&txn.connection())
            .await
            .map_err(orm_error)?,
    };
    // The tool's `seq` probe executes; `save` then discards its value
    // for the effective sequence.
    let _max_seq: Option<i32> = issue_entity::Entity::find()
        .filter(issue_entity::Column::ProjectId.eq(project_id))
        .filter(issue_entity::Column::DeletedAt.is_null())
        .select_only()
        .column(issue_entity::Column::SequenceId)
        .order_by_desc(issue_entity::Column::SequenceId)
        .limit(1)
        .into_tuple::<i32>()
        .one(&txn.connection())
        .await
        .map_err(orm_error)?;
    // The save rides `domain::issues::create` on the audit `WriteTxn`:
    // The shared core (NEWPIDASH-190 §4.3): insert + chain with the
    // AfterCommit fire rule on the audit `WriteTxn` (hooks repair a
    // missing tool state, take the advisory lock, scan the effective
    // sequence and sort order, resolve the default pod, stamp
    // `completed_at` for a tool-supplied completed state only, and
    // strip the html; the fire and the created activity are both
    // skipped — the after-commit fire step below drives the bespoke
    // fire and Django sends no activity on this path).
    // `to_safe_html` never yields `""` (the `<p></p>` fallback), so the
    // hook's `NULL` arm is defensive, kept for the port's exact shape.
    let html = to_safe_html(Some(description));
    let domain_ctx = write_ctx(scope, project_id);
    let transition = ctx.transition_hook.clone();
    let enqueue: Arc<dyn TaskEnqueueHook> = Arc::new(WorkerTaskEnqueueHook::new(&ctx.pool));
    let pool = ctx.pool.clone();
    txn.hooks_mut().register(transition);
    txn.hooks_mut().register(enqueue);
    let create = pidash_services::operations::core::issues::IssueCreate {
        project_id,
        name: stripped_title.to_owned(),
        description_html: html,
        state: tool_state,
        created_via: Some("cloud_agent".to_owned()),
        apply_default_assignee: false,
        fire: pidash_services::operations::core::issues::CreateFire::AfterCommit,
    };
    let created =
        pidash_services::operations::core::issues::create(txn, &domain_ctx, &pool, &create)
            .await
            .map_err(|error| match error {
                pidash_services::operations::core::CoreError::Request(message) => {
                    ToolExecError::Failure(plan::ToolCallFailure::InvalidArg(message))
                }
                pidash_services::operations::core::CoreError::Missing(model) => {
                    ToolExecError::Missing { model }
                }
                pidash_services::operations::core::CoreError::Db(domain_error) => {
                    ToolExecError::Db(sqlx::Error::Protocol(domain_error.to_string()))
                }
                pidash_services::operations::core::CoreError::Sqlx(sqlx_error) => {
                    ToolExecError::Db(sqlx_error)
                }
            })?;
    let issue_id = created.issue_id;
    // The deferred `select_for_update` (`tools.py:351`; see above):
    // taken on the audit transaction and held until the audit commits.
    let locked = project_lock_query(project_id)
        .select_only()
        .column(proj_entity::Column::Id)
        .into_tuple::<Uuid>()
        .one(&txn.connection())
        .await
        .map_err(orm_error)?;
    if locked.is_none() {
        return Err(ToolExecError::Missing { model: "Project" });
    }
    let state_id = created.state_id;
    let sequence_id = i64::from(created.sequence_id);
    // `{"created": True, **_issue_data(issue)}` (brief, `created`
    // first): the project identifier plus the resolved state.
    let project_identifier = proj_entity::Entity::find_by_id(project_id)
        .select_only()
        .column(proj_entity::Column::Identifier)
        .into_tuple::<String>()
        .one(&txn.connection())
        .await
        .map_err(orm_error)?;
    let (state_name, state_group) = match state_id {
        Some(state) => match state_view_tx(&txn.connection(), state).await? {
            Some(view) => (Some(view.name), Some(view.group)),
            None => (None, None),
        },
        None => (None, None),
    };
    // The orchestration post-save fire (`signals.py:74-107`): Django's
    // `Issue.objects.create` runs it inside the save; here it is an
    // after-commit step on the audit transaction, drained after the
    // audit commits (before the `tool_completed` emit, which
    // `run_write` queues after the op).
    let ctx_fire = ctx.clone();
    let creator_id = scope.creator_id;
    txn.on_commit(move || {
        let c = ctx_fire.clone();
        async move { fire_create_transition(&c, creator_id, issue_id, state_id).await }
    });
    let detail = plan::issue_data(
        &issue_id.to_string(),
        project_identifier.as_deref().unwrap_or(""),
        sequence_id,
        stripped_title,
        state_name.as_deref(),
        state_group.as_deref(),
        "none",
        None,
    );
    let created = plan::issue_created(&plan::issue_brief(&detail));
    let value = serde_json::to_value(created).map_err(|error| {
        ToolExecError::Failure(plan::ToolCallFailure::Failed(error.to_string()))
    })?;
    Ok(value)
}

/// Preflight for the tool's create: the live
/// `preflight_eligibility_or_bounce` driver over the jobs
/// [`JobsMatcher`]. A create is not an agent move (`moved_by_run` is
/// `None`), so `dispatch_now` is reachable and the eligibility gate
/// must run for real — the transition sibling's [`BinPreflight`] would
/// fail closed here. Mirrors the api crate's `LivePreflight`; the deps
/// travel by `Option` because the driver takes them by value while the
/// seam only lends `&mut self`, and the driver consults preflight at
/// most once per fire.
struct LiveToolPreflight<C, H, P> {
    pool: PgPool,
    deps: Option<StoreDeps<C, H, P>>,
    matcher: JobsMatcher,
    now: DateTime<Utc>,
    jitter_secs: f64,
}

impl<C, H, P> PreflightSeam for LiveToolPreflight<C, H, P>
where
    C: AdmissionCache,
    H: Fn(Uuid) -> bool + Clone + Send + Sync,
    P: Fn(Option<Uuid>) -> LlmProfile + Clone + Send + Sync,
{
    async fn preflight_eligibility_or_bounce(
        &mut self,
        issue_id: Uuid,
        creator_id: Uuid,
        pod_id: Uuid,
        triggered_by: &str,
    ) -> Result<bool, pidash_services::orchestration::creation::CreationError> {
        let Some(deps) = self.deps.take() else {
            return Err(pidash_services::orchestration::creation::CreationError::Db(
                "tool create reached preflight twice: the driver consults at most once".to_owned(),
            ));
        };
        let out = preflight_eligibility_or_bounce(
            &self.pool,
            deps,
            &self.matcher,
            self.now,
            self.jitter_secs,
            issue_id,
            Some(creator_id),
            pod_id,
            triggered_by,
        )
        .await
        .map_err(|error| {
            pidash_services::orchestration::creation::CreationError::Db(error.to_string())
        })?;
        emit_logs(&out.outcome.logs);
        Ok(out.outcome.proceed)
    }

    async fn pod_once_preflight(
        &mut self,
        issue_id: Uuid,
        creator_id: Uuid,
        pod_id: Uuid,
    ) -> Result<Option<String>, pidash_services::orchestration::creation::CreationError> {
        use pidash_services::orchestration::creation::CreationError;
        use sea_orm::EntityTrait;
        let conn = pidash_db::pool::sea_orm_connection(&self.pool);
        let issue = pidash_db::entity::issues::Entity::find_by_id(issue_id)
            .one(&conn)
            .await
            .map_err(|error| CreationError::Db(error.to_string()))?
            .ok_or_else(|| CreationError::Db("issue not found".to_owned()))?;
        if let Some(code) = pidash_services::domain::issue_assignment::pod_once_auth_code(
            &conn,
            &issue,
            Some(creator_id),
        )
        .await
        .map_err(|error| CreationError::Db(error.to_string()))?
        {
            return Ok(Some(code.to_owned()));
        }
        let has = pidash_services::orchestration::dispatch::PodRunnerMatcher::pod_has_runner_for_issue_principal(
            &self.matcher, pod_id, issue_id, Some(creator_id), true, true,
        )
        .await
        .map_err(|error| CreationError::Db(error.to_string()))?;
        Ok(if has {
            None
        } else {
            Some("no_eligible_runner_in_pod".to_owned())
        })
    }
}

/// The [`StoreDeps`] behind the create fire: the transition shape
/// ([`drive_transition`]) — creator presence prefetched from the pool,
/// the actor-is-creator closures — over the fire's own clock. Built
/// twice per fire (store + preflight), like the v1 path's
/// `dispatch_deps_for_ids` pair.
fn create_fire_deps(
    ctx: &ToolCtx,
    creator: Uuid,
    has_key: bool,
    now_unix_secs: i64,
) -> StoreDeps<
    RedisAdmissionCache,
    impl Fn(Uuid) -> bool + Clone + Send + Sync,
    impl Fn(Option<Uuid>) -> LlmProfile + Clone + Send + Sync,
> {
    StoreDeps {
        cloud: ctx.cloud.clone(),
        managed: ctx.managed.clone(),
        cache: RedisAdmissionCache::from_env(),
        has_usable_llm_config: move |user: Uuid| user == creator && has_key,
        toolsets: ctx.toolsets.clone(),
        llm_profile: move |user: Option<Uuid>| {
            let key = user == Some(creator) && has_key;
            let profile = agent_model_profile_for_user(key);
            LlmProfile {
                available: profile.available,
                reason_code: profile.reason_code,
            }
        },
        now_unix_secs,
    }
}

/// The create fire's transition seam: the api `LiveTransitionSeam`
/// shape (PIDASHCONV-1216), which the jobs crate cannot reuse across
/// the crate direction.
///
/// The ticker/clock seam methods run pool-direct (autocommit);
/// everything else delegates to the tx-backed [`LiveCreationStore`].
/// The split is load-bearing: the reconcile clock lock must not be
/// held across `preflight_eligibility_or_bounce`, whose bounce opens
/// its own transaction and locks the same ticker row. Autocommit per
/// ticker statement mirrors Django's own separate reconcile atomic
/// (which commits before preflight).
struct LiveToolTransitionSeam<'a, C, H, P> {
    pool: &'a PgPool,
    store: LiveCreationStore<'a, C, H, P>,
}

impl<'a, C, H, P> LiveToolTransitionSeam<'a, C, H, P> {
    fn db(error: sqlx::Error) -> CreationError {
        CreationError::Db(error.to_string())
    }

    /// Recover the transaction (to commit) plus the outboxes (to drain
    /// after commit).
    fn into_parts(
        self,
    ) -> (
        Transaction<'a>,
        &'a PgPool,
        C,
        Vec<Uuid>,
        Vec<DeferredConsume>,
        Vec<Uuid>,
    ) {
        self.store.into_parts()
    }
}

impl<C, H, P> CreationSeam for LiveToolTransitionSeam<'_, C, H, P>
where
    C: AdmissionCache,
    H: Fn(Uuid) -> bool + Clone + Send + Sync,
    P: Fn(Option<Uuid>) -> LlmProfile + Clone + Send + Sync,
{
    async fn issue(&mut self, issue_id: Uuid) -> Result<IssueView, CreationError> {
        self.store.issue(issue_id).await
    }

    async fn project(&mut self, project_id: Uuid) -> Result<ProjectView, CreationError> {
        self.store.project(project_id).await
    }

    async fn state(&mut self, state_id: Option<Uuid>) -> Result<Option<StateView>, CreationError> {
        self.store.state(state_id).await
    }

    async fn dedicated_pin(
        &mut self,
        args: &DedicatedPinArgs<'_>,
    ) -> Result<PinOutcome, CreationError> {
        CreationSeam::dedicated_pin(&mut self.store, args).await
    }

    async fn insert_system_comment(
        &mut self,
        comment: &NewSystemComment,
    ) -> Result<(), CreationError> {
        CreationSeam::insert_system_comment(&mut self.store, comment).await
    }

    async fn clear_parent_thread(&mut self, run_id: Uuid) -> Result<(), CreationError> {
        CreationSeam::clear_parent_thread(&mut self.store, run_id).await
    }

    async fn pod_once_started_comment(
        &mut self,
        issue_id: Uuid,
        pod_id: Uuid,
        actor_id: Uuid,
        now: DateTime<Utc>,
    ) -> Result<NewSystemComment, CreationError> {
        CreationSeam::pod_once_started_comment(&mut self.store, issue_id, pod_id, actor_id, now)
            .await
    }

    async fn latest_prior_run(&mut self, issue_id: Uuid) -> Result<Option<RunView>, CreationError> {
        self.store.latest_prior_run(issue_id).await
    }

    async fn active_run_for(&mut self, issue_id: Uuid) -> Result<Option<RunView>, CreationError> {
        self.store.active_run_for(issue_id).await
    }

    async fn run(&mut self, run_id: Uuid) -> Result<Option<RunView>, CreationError> {
        self.store.run(run_id).await
    }

    async fn runner(&mut self, runner_id: Uuid) -> Result<Option<RunnerView>, CreationError> {
        self.store.runner(runner_id).await
    }

    async fn assigned_pod(&mut self, pod_id: Uuid) -> Result<Option<PodView>, CreationError> {
        self.store.assigned_pod(pod_id).await
    }

    async fn default_pod_for_project(
        &mut self,
        project_id: Uuid,
    ) -> Result<Option<PodView>, CreationError> {
        self.store.default_pod_for_project(project_id).await
    }

    async fn resume_parent_run_id(
        &mut self,
        issue_id: Uuid,
    ) -> Result<Option<Uuid>, CreationError> {
        self.store.resume_parent_run_id(issue_id).await
    }

    async fn work_item_id_for_run(&mut self, run_id: Uuid) -> Result<Option<Uuid>, CreationError> {
        self.store.work_item_id_for_run(run_id).await
    }

    async fn lock_issue_for_handoff(
        &mut self,
        issue_id: Uuid,
    ) -> Result<Option<LockedIssue>, CreationError> {
        self.store.lock_issue_for_handoff(issue_id).await
    }

    async fn lock_run_for_handoff(
        &mut self,
        run_id: Uuid,
    ) -> Result<Option<RunView>, CreationError> {
        self.store.lock_run_for_handoff(run_id).await
    }

    async fn user_flags(&mut self, user_id: Uuid) -> Result<UserFlags, CreationError> {
        self.store.user_flags(user_id).await
    }

    async fn insert_run(&mut self, row: &NewAgentRun) -> Result<RunView, CreationError> {
        self.store.insert_run(row).await
    }

    async fn save_prompt(
        &mut self,
        run_id: Uuid,
        prompt: &str,
        manifest: &Value,
    ) -> Result<(), CreationError> {
        self.store.save_prompt(run_id, prompt, manifest).await
    }

    async fn save_run_config(&mut self, run_id: Uuid, config: &Value) -> Result<(), CreationError> {
        self.store.save_run_config(run_id, config).await
    }

    async fn execution_fields(
        &mut self,
        req: &ExecutionRequest,
    ) -> Result<ExecutionFields, ExecutionError> {
        self.store.execution_fields(req).await
    }

    async fn lock_cloud_creation_capacity(
        &mut self,
        workspace_id: Uuid,
        executor_kind: AgentExecutorKind,
        automatic: bool,
    ) -> Result<Option<AdmissionError>, CreationError> {
        self.store
            .lock_cloud_creation_capacity(workspace_id, executor_kind, automatic)
            .await
    }

    fn dispatch_after_commit(&mut self, run_id: Uuid) {
        self.store.dispatch_after_commit(run_id)
    }

    async fn render_bundle(
        &mut self,
        issue_id: Uuid,
        run_id: Uuid,
        parent_run_id: Option<Uuid>,
        trigger: &str,
        created_by_id: Uuid,
    ) -> Result<RenderBundle, CreationError> {
        self.store
            .render_bundle(issue_id, run_id, parent_run_id, trigger, created_by_id)
            .await
    }

    fn extra_toolsets_schema_tool(&self) -> String {
        self.store.extra_toolsets_schema_tool()
    }
}

impl<C, H, P> FinalizeAgentRunSeam for LiveToolTransitionSeam<'_, C, H, P>
where
    C: AdmissionCache,
    H: Fn(Uuid) -> bool + Clone + Send + Sync,
    P: Fn(Option<Uuid>) -> LlmProfile + Clone + Send + Sync,
{
    async fn finalize_failed_run(
        &mut self,
        run_id: Uuid,
        error_code: &str,
        error: &str,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<RunView, CreationError> {
        self.store
            .finalize_failed_run(run_id, error_code, error, now)
            .await
    }
}

impl<C, H, P> EntriesSeam for LiveToolTransitionSeam<'_, C, H, P>
where
    C: AdmissionCache,
    H: Fn(Uuid) -> bool + Clone + Send + Sync,
    P: Fn(Option<Uuid>) -> LlmProfile + Clone + Send + Sync,
{
    async fn prior_state_id(&mut self, issue_id: Uuid) -> Result<Option<Uuid>, CreationError> {
        use pidash_db::entity::issues as issue_entity;
        let db = pidash_db::pool::sea_orm_connection(self.pool);
        let row = issue_entity::Entity::find_by_id(issue_id)
            .select_only()
            .column(issue_entity::Column::StateId)
            .into_tuple::<Option<Uuid>>()
            .one(&db)
            .await
            .map_err(|error| Self::db(pidash_db::orm_tx::into_sqlx(error)))?;
        Ok(row.flatten())
    }

    async fn queued_follow_up(&mut self, issue_id: Uuid) -> Result<Option<RunView>, CreationError> {
        self.store.queued_follow_up(issue_id).await
    }

    async fn lock_ticker(
        &mut self,
        issue_id: Uuid,
    ) -> Result<Option<IssueAgentTicker>, CreationError> {
        use pidash_db::entity::issue_agent_ticker as tick_entity;
        let db = pidash_db::pool::sea_orm_connection(self.pool);
        let row = tick_entity::Entity::find()
            .filter(tick_entity::Column::IssueId.eq(issue_id))
            .filter(tick_entity::Column::DeletedAt.is_null())
            .order_by_asc(tick_entity::Column::Id)
            .limit(1)
            .lock_exclusive()
            .one(&db)
            .await
            .map_err(|error| Self::db(pidash_db::orm_tx::into_sqlx(error)))?;
        Ok(row.map(IssueAgentTicker::from))
    }

    async fn save_ticker(
        &mut self,
        row: &IssueAgentTicker,
        write: ClockWrite,
    ) -> Result<(), CreationError> {
        // `updated_at` is now-at-write: Python's `save()` stamps
        // `auto_now` at write time (the pre-consolidation bind).
        apply_clock_write(
            &pidash_db::pool::sea_orm_connection(self.pool),
            &write,
            row,
            Utc::now(),
        )
        .await
        .map_err(Self::db)
    }

    fn set_rollback(&mut self) {
        self.store.set_rollback()
    }

    async fn clock_policy(
        &mut self,
        project_id: Uuid,
    ) -> Result<ProjectClockPolicy, CreationError> {
        let db = pidash_db::pool::sea_orm_connection(self.pool);
        let row = pidash_services::orchestration::entries::clock_policy(&db, project_id)
            .await
            .map_err(|error| Self::db(pidash_db::orm_tx::into_sqlx(error)))?;
        let Some((ticking_enabled, max_ticks, interval, interval_review, interval_test)) = row
        else {
            return Err(CreationError::MissingRow(format!(
                "no clock policy {project_id}"
            )));
        };
        Ok(ProjectClockPolicy {
            agent_ticking_enabled: Some(ticking_enabled),
            agent_default_max_ticks: Some(max_ticks),
            agent_default_interval_seconds: Some(i64::from(interval)),
            agent_review_default_interval_seconds: Some(i64::from(interval_review)),
            agent_test_default_interval_seconds: Some(i64::from(interval_test)),
        })
    }

    async fn binding(&mut self, binding_id: Uuid) -> Result<BindingView, CreationError> {
        self.store.binding(binding_id).await
    }

    async fn scheduler_override_pod(
        &mut self,
        pod_id: Uuid,
        project_id: Option<Uuid>,
    ) -> Result<Option<PodView>, CreationError> {
        self.store.scheduler_override_pod(pod_id, project_id).await
    }

    async fn workspace(&mut self, workspace_id: Uuid) -> Result<WorkspaceView, CreationError> {
        self.store.workspace(workspace_id).await
    }

    async fn scheduler_row(&mut self, scheduler_id: Uuid) -> Result<SchedulerView, CreationError> {
        self.store.scheduler_row(scheduler_id).await
    }

    async fn scheduler_override_rows(
        &mut self,
        workspace_id: Uuid,
    ) -> Result<Vec<OverrideRow>, CreationError> {
        self.store.scheduler_override_rows(workspace_id).await
    }

    async fn project_role_facts(
        &mut self,
        user_id: Uuid,
        workspace_slug: &str,
        project_id: Uuid,
    ) -> Result<ProjectRoleFacts, CreationError> {
        self.store
            .project_role_facts(user_id, workspace_slug, project_id)
            .await
    }

    async fn has_usable_llm_config(&mut self, user_id: Uuid) -> Result<bool, CreationError> {
        self.store.has_usable_llm_config(user_id).await
    }

    async fn agent_system_user(
        &mut self,
    ) -> Result<Result<Uuid, AgentUserCollisionError>, CreationError> {
        self.store.agent_system_user().await
    }

    async fn insert_scheduler_run(
        &mut self,
        row: &NewSchedulerRun,
    ) -> Result<RunView, CreationError> {
        self.store.insert_scheduler_run(row).await
    }

    fn compose_scheduler_turn(
        &mut self,
        context: &Value,
        index: &OverrideIndex,
        workspace_id: Option<&str>,
        executor_kind: Option<&str>,
        tool_catalog_version: i64,
    ) -> Result<RenderedTurn, String> {
        self.store.compose_scheduler_turn(
            context,
            index,
            workspace_id,
            executor_kind,
            tool_catalog_version,
        )
    }
}

/// The post-save fire for a tool-created issue
/// (`fire_state_transition`, `signals.py:74-107`): a new row has no
/// prior state, so creating directly in a ticking state arms the clock
/// and creates the entry run — the v1 create fire (`fire_after_save`,
/// PIDASHCONV-1216) over the tool's write.
///
/// Best-effort: the issue already committed, so nothing here fails the
/// tool. A failed handler rolls the fire transaction back and logs the
/// verbatim line (the counter already bumped inside); a lookup storage
/// failure only warns. `created_by` is the run creator — the signal
/// fires inside `impersonate(run.created_by)` (`tools.py:357`), so the
/// crum user stamps the ticker row. The pool-direct clock writes are
/// never rolled back (the api split seam's contract): a bounce or a
/// late failure keeps the reconcile row, like Django.
async fn fire_create_transition(
    ctx: &ToolCtx,
    creator_id: Uuid,
    issue_id: Uuid,
    state_id: Option<Uuid>,
) {
    let Some(current_state_id) = state_id else {
        return;
    };
    let now = Utc::now();
    let has_key = match crate::oss_worker::execute_seams::LiveExecuteSeams::llm_row(
        &ctx.pool, creator_id,
    )
    .await
    {
        Ok(row) => row.as_ref().is_some_and(|row| row.has_api_key()),
        Err(error) => {
            tracing::warn!(%error, "tool create fire: llm presence lookup failed; fire skipped");
            return;
        }
    };
    let now_unix_secs = now.timestamp();
    let store_deps = create_fire_deps(ctx, creator_id, has_key, now_unix_secs);
    let preflight_deps = create_fire_deps(ctx, creator_id, has_key, now_unix_secs);
    let tx = match Transaction::begin(&ctx.pool).await {
        Ok(tx) => tx,
        Err(error) => {
            tracing::warn!(%error, "tool create fire: begin failed; fire skipped");
            return;
        }
    };
    let mut seam = LiveToolTransitionSeam {
        pool: &ctx.pool,
        store: LiveCreationStore::new(tx, &ctx.pool, store_deps),
    };
    let mut preflight = LiveToolPreflight {
        pool: ctx.pool.clone(),
        deps: Some(preflight_deps),
        matcher: JobsMatcher::new(ctx.pool.clone()),
        now,
        jitter_secs: 0.0,
    };
    let request = FireRequest {
        issue_id,
        prev_state_id: None,
        current_state_id: Some(current_state_id),
        dispatch_immediate: true,
        pod_once: false,

        moved_by_run: None,
        now,
        // Caller-drawn jitter (L5 quirk-5): none, like the v1 create
        // fire over the same entered-bucket event.
        jitter_secs: 0.0,
        created_by: Some(creator_id),
    };
    let outcome = fire_state_transition(&mut seam, &mut preflight, &request).await;
    let (tx, pool, cache, dispatches, deferred, terminal_effects) = seam.into_parts();
    let outcome = match outcome {
        Ok(outcome) => outcome,
        Err(error) => {
            let _ = tx.rollback().await;
            tracing::warn!(%error, "tool create fire: state lookup failed; issue stands unfired");
            return;
        }
    };
    if !matches!(outcome, FireOutcome::Called(_)) {
        // NoTransition (unreachable: a `Some` state always differs from
        // the `None` prior) / Failed: nothing this drive wrote is kept.
        let _ = tx.rollback().await;
        if let FireOutcome::Failed { log_line, .. } = outcome {
            tracing::error!("{log_line}");
        }
        return;
    }
    if let Err(error) = tx.commit().await {
        tracing::warn!(%error, "tool create fire: commit failed; issue stands unfired");
        return;
    }
    for run_id in dispatches {
        // The driver only *returns* the decision (`dispatch.py:61-70`
        // drains the pod inline), so a `PodDrain` must be acted on here
        // or the run stays QUEUED (PIDASHCONV-1494). Best-effort, like
        // the rest of the fire: the entry run stands either way.
        match dispatch_agent_run(pool, &ctx.cloud, run_id, now).await {
            Ok(DispatchDecision::PodDrain { pod_id }) => {
                if let Err(error) =
                    drain_pod_by_id(pool, ctx.redis.as_ref(), &ctx.runner, pod_id).await
                {
                    tracing::warn!(%error, "tool create fire: pod drain failed");
                }
            }
            Ok(_) => {}
            Err(error) => {
                tracing::warn!(%error, "tool create fire: entry-run dispatch failed");
            }
        }
    }
    for consume in &deferred {
        consume_admission_token(&cache, consume);
    }
    for effect_id in terminal_effects {
        for effect in finalization::plan_publish_effects(effect_id) {
            match effect {
                LifecycleEffect::PublishTerminalEffects { run_id } => {
                    let job = terminal_effects_job(&run_id);
                    if let Err(error) = enqueue(pool, &job).await {
                        tracing::error!(%error, "failed to publish terminal effects for run {run_id}");
                    }
                }
                LifecycleEffect::ApplyTerminalEffectsInline { run_id } => {
                    tracing::warn!(
                        "skipping inline terminal effects for run {run_id}: no live RunnerRunsSeams \
                         (PIDASHCONV-859); the queued effects task covers the hooks idempotently"
                    );
                }
                _ => {}
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Transition (`pidash_transition_current_issue`, `tools.py:309-343`)
// ---------------------------------------------------------------------------

/// What a state-changing transition hands its caller for the
/// post-commit completion comment-back (`github_signals.py:44-75`):
/// the moved issue, its locked pre-save state (Django's
/// `capture_prior_state` value), and the new state.
/// Post-commit completion comment-back for a state-changing
/// transition (`github_signals.py:44-75`, via the jobs-layer port;
/// the `tools_local_store::enqueue_completion_comment` shape from
/// PIDASHCONV-1252). Resolves the dispatch (group, mirror,
/// idempotency guards) and enqueues the provider's
/// `post_completion_comment` job. Best-effort: failures only warn.
async fn enqueue_completion_comment(
    pool: &PgPool,
    issue_id: &Uuid,
    prev_state_id: Option<&Uuid>,
    current_state_id: Option<&Uuid>,
) {
    use crate::integrations::github_signals::{
        dispatch_completion_comment, dispatch_job, DispatchOutcome,
    };
    let outcome =
        dispatch_completion_comment(pool, issue_id, current_state_id, false, prev_state_id).await;
    let target = match outcome {
        Ok(DispatchOutcome::Dispatch(target)) => target,
        Ok(DispatchOutcome::Skip(_)) => return,
        Err(error) => {
            tracing::warn!(%error, "completion comment dispatch lookup failed; op stands");
            return;
        }
    };
    let job = dispatch_job(&target);
    if let Err(error) = enqueue(pool, &job).await {
        tracing::warn!(%error, task = %job.task, "task enqueue failed; op stands");
    }
}

/// `pidash_transition_current_issue` (`tools.py:309-343`): look the
/// target state up in the run's project, lock the bound issue, no-op
/// on same state, save through the model (audit stamps as the
/// creator), then route through the live
/// `handle_issue_state_transition` with the run stamped as the mover.
/// Runs on the audit [`WriteTxn`] like the other write tools
/// (NEWPIDASH-190 Part 1b); on a state change, the comment-back
/// dispatch is queued with [`WriteTxn::on_commit`] and drains after
/// the audit commits.
async fn op_transition_current_issue(
    ctx: &ToolCtx,
    txn: &mut WriteTxn,
    scope: &ToolScope,
    state_id: &str,
) -> Result<Value, ToolExecError> {
    // `run.work_item.project_id` evaluates before the `.get()` call,
    // so a run with no bound issue raises before parsing `state_id`.
    if scope.work_item_id.is_none() {
        return Err(ToolExecError::AttributeError {
            attribute: "project_id",
        });
    }
    let Some(target_id) = parse_django_uuid(state_id) else {
        return Err(ToolExecError::InvalidUuid {
            value: state_id.to_owned(),
        });
    };
    let target = transition_state_query(target_id, scope.project_id)
        .into_model::<IssueStateView>()
        .one(&txn.connection())
        .await
        .map_err(orm_error)?;
    let Some(target) = target else {
        return Err(ToolExecError::Missing { model: "State" });
    };
    let target_id = target.id;
    let target_name = target.name;
    // The shared core (NEWPIDASH-190 §4.3): lock + same-state no-op +
    // bare three-column write + inline never-dispatch drive on the
    // audit transaction; the project-scoped target lookup above and
    // the completion enqueue below stay with this wrapper.
    let write_ctx = pidash_services::domain::WriteCtx::new(pidash_services::domain::WriteActor {
        user_id: scope.creator_id,
        timezone: None,
    });
    let change = pidash_services::operations::core::issues::StateChange {
        issue_id: scope.work_item_id.expect("bound issue checked above"),
        state_id: target_id,
        moved_by_run: Some(scope.run_id),
        dispatch: pidash_services::operations::core::issues::Dispatch::NeverDispatch,
    };
    let moved =
        pidash_services::operations::core::issues::set_state(txn, &write_ctx, &ctx.pool, &change)
            .await
            .map_err(|error| match error {
                pidash_services::operations::core::CoreError::Request(message) => {
                    ToolExecError::Failure(plan::ToolCallFailure::InvalidArg(message))
                }
                pidash_services::operations::core::CoreError::Missing(model) => {
                    ToolExecError::Missing { model }
                }
                pidash_services::operations::core::CoreError::Db(domain_error) => {
                    ToolExecError::Db(sqlx::Error::Protocol(domain_error.to_string()))
                }
                pidash_services::operations::core::CoreError::Sqlx(sqlx_error) => {
                    ToolExecError::Db(sqlx_error)
                }
            })?;
    if !moved.changed {
        let result = plan::transition_result(false, &target_name, &target_id.to_string());
        let value = serde_json::to_value(result).map_err(|error| {
            ToolExecError::Failure(plan::ToolCallFailure::Failed(error.to_string()))
        })?;
        return Ok(value);
    }
    let from_state_id = moved.from_state_id;
    // `github_signals.trigger_completion_comment`
    // (`bgtasks/github_signals.py:44-75`): a save moving a mirrored issue
    // into a completed-group state enqueues the provider's
    // `post_completion_comment`. Django fires this from `Issue.save()`
    // (`tools.py:335`), so the tool path gets it for free; here it is an
    // after-commit step on the audit transaction (the unchanged-state
    // no-op above returns `None`; the helper re-checks the completed
    // group, the mirror order (git before github) and the
    // already-commented guard). Best-effort like the relation enqueues:
    // the edit already committed, so a lookup or queue failure must not
    // fail the tool.
    let issue_id = scope.work_item_id.expect("bound issue checked above");
    let pool = ctx.pool.clone();
    txn.on_commit(move || {
        let p = pool.clone();
        async move {
            enqueue_completion_comment(&p, &issue_id, from_state_id.as_ref(), Some(&target_id))
                .await
        }
    });
    let result = plan::transition_result(true, &target_name, &target_id.to_string());
    let value = serde_json::to_value(result).expect("transition result serializes");
    Ok(value)
}

// ---------------------------------------------------------------------------
// Relations (`tools.py:373-448` over `orchestration/relations.py`)
// ---------------------------------------------------------------------------

/// The member-issues pool as a SeaORM subquery: the `issues` ids of
/// the user's active project memberships on the slug-named workspace,
/// under `IssueManager` liveness (including Django's
/// `exclude(state__group='triage')` NULL-safe form, which keeps
/// stateless issues). Select-statement copy of the old
/// `MEMBER_PROJECT_ISSUES_SQL` (binds: user id, workspace slug).
fn member_pool_stmt(creator_id: Uuid, workspace_slug: &str) -> sea_orm::sea_query::SelectStatement {
    use pidash_db::entity::{
        issues as i_entity, project_members as pm_entity, projects as proj_entity,
        states as st_entity, workspaces as ws_entity,
    };
    sea_orm::sea_query::Query::select()
        .expr_as(Expr::col((i_entity::Entity, i_entity::Column::Id)), "id")
        .from(i_entity::Entity)
        .join(
            JoinType::LeftJoin,
            st_entity::Entity,
            Expr::col((st_entity::Entity, st_entity::Column::Id))
                .equals((i_entity::Entity, i_entity::Column::StateId)),
        )
        .join(
            JoinType::InnerJoin,
            proj_entity::Entity,
            Expr::col((proj_entity::Entity, proj_entity::Column::Id))
                .equals((i_entity::Entity, i_entity::Column::ProjectId)),
        )
        .join(
            JoinType::InnerJoin,
            pm_entity::Entity,
            Expr::col((pm_entity::Entity, pm_entity::Column::ProjectId))
                .equals((proj_entity::Entity, proj_entity::Column::Id)),
        )
        .join(
            JoinType::InnerJoin,
            ws_entity::Entity,
            Expr::col((ws_entity::Entity, ws_entity::Column::Id))
                .equals((i_entity::Entity, i_entity::Column::WorkspaceId)),
        )
        .and_where(Expr::col((i_entity::Entity, i_entity::Column::DeletedAt)).is_null())
        .and_where(
            Expr::col((st_entity::Entity, st_entity::Column::Group))
                .is_null()
                .or(Expr::col((st_entity::Entity, st_entity::Column::Group)).ne("triage")),
        )
        .and_where(Expr::col((i_entity::Entity, i_entity::Column::ArchivedAt)).is_null())
        .and_where(Expr::col((proj_entity::Entity, proj_entity::Column::ArchivedAt)).is_null())
        .and_where(Expr::col((i_entity::Entity, i_entity::Column::IsDraft)).eq(false))
        .and_where(Expr::col((pm_entity::Entity, pm_entity::Column::IsActive)).eq(true))
        .and_where(Expr::col((pm_entity::Entity, pm_entity::Column::MemberId)).eq(creator_id))
        .and_where(Expr::col((ws_entity::Entity, ws_entity::Column::Slug)).eq(workspace_slug))
        .distinct()
        .to_owned()
}

/// One `resolve_by_id`/`_by_identifier` output row.
#[derive(Clone, Debug, PartialEq, FromQueryResult)]
struct ResolvedIssueRow {
    id: Uuid,
    workspace_id: Uuid,
    project_id: Uuid,
    identifier: String,
    sequence_id: i32,
}

/// `resolve_refs` (`relations.py:138-163`) against the member pool:
/// each raw ref classifies (UUID or `PROJ-123`), resolves with the
/// ported predicates plus Django's `.first()` ordering, and misses
/// collect `ref or str(raw)`. `own_project` narrows to the run's
/// project ([`plan::OWN_PROJECT_ISSUES_PREDICATE`], `$3`). Returns
/// `(found, unresolved)` in request order.
async fn resolve_refs_tx<C: sea_orm::ConnectionTrait>(
    conn: &C,
    scope: &ToolScope,
    refs: &[String],
    own_project: bool,
) -> Result<(Vec<rel::ResolvedIssue>, Vec<String>), ToolExecError> {
    let mut found = Vec::with_capacity(refs.len());
    let mut unresolved = Vec::new();
    for raw in refs {
        let classified = rel::classify_ref(raw);
        let matched = match &classified.kind {
            rel::RefKind::Unresolved => None,
            rel::RefKind::Id(id) => resolve_by_id_tx(conn, scope, own_project, *id).await?,
            rel::RefKind::Identifier {
                project_code,
                sequence_id,
            } => {
                resolve_by_identifier_tx(conn, scope, own_project, project_code, *sequence_id)
                    .await?
            }
        };
        match matched {
            Some(issue) => found.push(issue),
            None => unresolved.push(rel::unresolved_passthrough(raw)),
        }
    }
    Ok((found, unresolved))
}

/// Map a resolve row onto [`rel::ResolvedIssue`].
fn resolved_issue_from_row(row: ResolvedIssueRow) -> rel::ResolvedIssue {
    rel::ResolvedIssue {
        id: row.id,
        workspace_id: row.workspace_id,
        project_id: row.project_id,
        project_identifier: row.identifier,
        sequence_id: row.sequence_id,
    }
}

/// The resolve read over the member pool: the `ResolvedIssue` columns,
/// newest-first, one row (Django's `.first()` under `-created_at`).
async fn resolve_tx<C: sea_orm::ConnectionTrait>(
    conn: &C,
    scope: &ToolScope,
    own_project: bool,
    id: Option<Uuid>,
    project_code: Option<&str>,
    sequence_id: Option<i32>,
) -> Result<Option<rel::ResolvedIssue>, ToolExecError> {
    use pidash_db::entity::{issues as i_entity, projects as proj_entity};
    let pool = member_pool_stmt(scope.creator_id, workspace_slug_str(scope));
    let mut query = i_entity::Entity::find()
        .join(JoinType::InnerJoin, i_entity::Relation::Projects.def())
        .filter(i_entity::Column::Id.in_subquery(pool))
        .order_by_desc(i_entity::Column::CreatedAt);
    if own_project {
        query = query.filter(i_entity::Column::ProjectId.eq(scope.project_id));
    }
    if let Some(id) = id {
        query = query.filter(i_entity::Column::Id.eq(id));
    }
    if let (Some(code), Some(sequence)) = (project_code, sequence_id) {
        query = query
            .filter(
                Expr::expr(sea_orm::sea_query::Func::upper(Expr::col(
                    proj_entity::Column::Identifier,
                )))
                .eq(sea_orm::sea_query::Func::upper(Expr::val(code.to_owned()))),
            )
            .filter(i_entity::Column::SequenceId.eq(sequence));
    }
    let row = query
        .select_only()
        .column(i_entity::Column::Id)
        .column(i_entity::Column::WorkspaceId)
        .column(i_entity::Column::ProjectId)
        .column(i_entity::Column::SequenceId)
        .column_as(proj_entity::Column::Identifier, "identifier")
        .limit(1)
        .into_model::<ResolvedIssueRow>()
        .one(conn)
        .await
        .map_err(orm_error)?;
    Ok(row.map(resolved_issue_from_row))
}

fn workspace_slug_str(scope: &ToolScope) -> &str {
    scope.workspace_slug.as_str()
}

/// The UUID half of `resolve_refs`: pool, `$3` project when narrowed,
/// then the canonical id.
async fn resolve_by_id_tx<C: sea_orm::ConnectionTrait>(
    conn: &C,
    scope: &ToolScope,
    own_project: bool,
    id: Uuid,
) -> Result<Option<rel::ResolvedIssue>, ToolExecError> {
    resolve_tx(conn, scope, own_project, Some(id), None, None).await
}

/// The identifier half of `resolve_refs`: pool, optional narrowing,
/// then code + sequence.
async fn resolve_by_identifier_tx<C: sea_orm::ConnectionTrait>(
    conn: &C,
    scope: &ToolScope,
    own_project: bool,
    project_code: &str,
    sequence_id: i64,
) -> Result<Option<rel::ResolvedIssue>, ToolExecError> {
    resolve_tx(
        conn,
        scope,
        own_project,
        None,
        Some(project_code),
        Some(i32::try_from(sequence_id).unwrap_or(i32::MAX)),
    )
    .await
}

/// `pidash_relate_issues` (`tools.py:416-424` over
/// `relations.py:180-232`): per-target verdict loop with a savepointed
/// insert on `Create` (23505 re-reads and re-verdicts, quirk 5
/// looping back to `Create` when the race resolves to nothing), one
/// `issue_activity` emit for the batch, and the refreshed grouping.
async fn op_relate_issues(
    txn: &mut WriteTxn,
    scope: &ToolScope,
    args: &plan::RelationWriteArgs,
) -> Result<Value, ToolExecError> {
    let (sources, unresolved) = resolve_refs_tx(
        &txn.connection(),
        scope,
        std::slice::from_ref(&args.issue),
        true,
    )
    .await?;
    if !unresolved.is_empty() {
        let refs: Vec<&str> = unresolved.iter().map(String::as_str).collect();
        return Err(plan::ToolCallFailure::InvalidArg(plan::unresolved_refs_message(&refs)).into());
    }
    let (targets, unresolved) =
        resolve_refs_tx(&txn.connection(), scope, &args.related_issues, false).await?;
    if !unresolved.is_empty() {
        let refs: Vec<&str> = unresolved.iter().map(String::as_str).collect();
        return Err(plan::ToolCallFailure::InvalidArg(plan::unresolved_refs_message(&refs)).into());
    }
    let source = sources
        .into_iter()
        .next()
        .expect("one source ref resolves one row");
    // The shared core (NEWPIDASH-190 §4.3): the check, verdict loop
    // and guarded inserts on the audit transaction; emissions and
    // `relations` stay with this wrapper.
    let core_source = core_relations::CoreSource::from(&source);
    let core_targets: Vec<core_relations::CoreTarget> = targets
        .iter()
        .map(core_relations::CoreTarget::from)
        .collect();
    let outcome = core_relations::relate(
        &txn.connection(),
        &core_relations::RelationBatch {
            source: &core_source,
            targets: &core_targets,
            relation_type: &args.relation_type,
            actor_id: scope.creator_id,
            own_project: true,
            pair_exact: false,
            raw_targets: false,
            newest_only: false,
        },
    )
    .await
    .map_err(|error| match error {
        pidash_services::operations::core::CoreError::Request(message) => {
            ToolExecError::Failure(plan::ToolCallFailure::InvalidArg(message))
        }
        pidash_services::operations::core::CoreError::Missing(model) => {
            ToolExecError::Missing { model }
        }
        pidash_services::operations::core::CoreError::Db(domain_error) => {
            ToolExecError::Db(sqlx::Error::Protocol(domain_error.to_string()))
        }
        pidash_services::operations::core::CoreError::Sqlx(sqlx_error) => {
            ToolExecError::Db(sqlx_error)
        }
    })?;
    let created = outcome.created;
    let unchanged = outcome.unchanged;
    let conflicts = outcome.conflicts;
    if !created.is_empty() {
        let created_ids: Vec<Uuid> = created.iter().map(|target| target.id).collect();
        let emit = rel::relate_created_emit(
            &args.relation_type,
            &created_ids,
            &scope.creator_id,
            &source.id,
            &source.project_id,
            Utc::now().timestamp(),
        );
        enqueue_activity(txn, &emit).await?;
    }
    let result = rel::RelateResult {
        issue: source.identifier(),
        relation_type: args.relation_type.clone(),
        created: created.iter().map(|target| target.identifier()).collect(),
        unchanged,
        conflicts,
    };
    let mut map = serde_json::to_value(result)
        .map_err(|error| plan::ToolCallFailure::Failed(error.to_string()))?;
    let grouped = tool_grouped(&txn.connection(), scope, &source)
        .await
        .map_err(core_error)?;
    map = plan::with_grouped_relations(map.as_object().cloned().unwrap_or_default(), grouped);
    Ok(map)
}

/// `pidash_unrelate_issues` (`tools.py:426-431` over
/// `relations.py:235-266`): per-target exact-type delete (soft-delete
/// plus the related-objects sweep per row), one `issue_activity` emit
/// per removed target, and the refreshed grouping.
async fn op_unrelate_issues(
    txn: &mut WriteTxn,
    scope: &ToolScope,
    args: &plan::RelationWriteArgs,
) -> Result<Value, ToolExecError> {
    let (sources, unresolved) = resolve_refs_tx(
        &txn.connection(),
        scope,
        std::slice::from_ref(&args.issue),
        true,
    )
    .await?;
    if !unresolved.is_empty() {
        let refs: Vec<&str> = unresolved.iter().map(String::as_str).collect();
        return Err(plan::ToolCallFailure::InvalidArg(plan::unresolved_refs_message(&refs)).into());
    }
    let (targets, unresolved) =
        resolve_refs_tx(&txn.connection(), scope, &args.related_issues, false).await?;
    if !unresolved.is_empty() {
        let refs: Vec<&str> = unresolved.iter().map(String::as_str).collect();
        return Err(plan::ToolCallFailure::InvalidArg(plan::unresolved_refs_message(&refs)).into());
    }
    let source = sources
        .into_iter()
        .next()
        .expect("one source ref resolves one row");
    // The shared core (NEWPIDASH-190 §4.3): the check, pair fetch,
    // match and soft deletes on the audit transaction; emissions and
    // `relations` stay with this wrapper.
    // The shared core (NEWPIDASH-190 §4.3): the check, pair fetch,
    // match and soft deletes on the audit transaction; emissions and
    // `relations` stay with this wrapper.
    let core_source = core_relations::CoreSource::from(&source);
    let core_targets: Vec<core_relations::CoreTarget> = targets
        .iter()
        .map(core_relations::CoreTarget::from)
        .collect();
    let outcome = core_relations::unrelate(
        &txn.connection(),
        &core_relations::RelationBatch {
            source: &core_source,
            targets: &core_targets,
            relation_type: &args.relation_type,
            actor_id: scope.creator_id,
            own_project: true,
            pair_exact: false,
            raw_targets: false,
            newest_only: false,
        },
    )
    .await
    .map_err(|error| match error {
        pidash_services::operations::core::CoreError::Request(message) => {
            ToolExecError::Failure(plan::ToolCallFailure::InvalidArg(message))
        }
        pidash_services::operations::core::CoreError::Missing(model) => {
            ToolExecError::Missing { model }
        }
        pidash_services::operations::core::CoreError::Db(domain_error) => {
            ToolExecError::Db(sqlx::Error::Protocol(domain_error.to_string()))
        }
        pidash_services::operations::core::CoreError::Sqlx(sqlx_error) => {
            ToolExecError::Db(sqlx_error)
        }
    })?;
    for row_id in &outcome.rows_affected {
        enqueue_sweep(txn, "issuerelation", &row_id.to_string()).await?;
    }
    // One activity per removed target (inside the loop).
    for removed_id in &outcome.removed_ids {
        let emit = rel::unrelate_deleted_emit(
            &args.relation_type,
            removed_id,
            &scope.creator_id,
            &source.id,
            &source.project_id,
            Utc::now().timestamp(),
        );
        enqueue_activity(txn, &emit).await?;
    }
    let removed = outcome.removed;
    let not_related = outcome.not_related;
    let result = rel::UnrelateResult {
        issue: source.identifier(),
        relation_type: args.relation_type.clone(),
        removed,
        not_related,
    };
    let mut map = serde_json::to_value(result)
        .map_err(|error| plan::ToolCallFailure::Failed(error.to_string()))?;
    let grouped = tool_grouped(&txn.connection(), scope, &source)
        .await
        .map_err(core_error)?;
    map = plan::with_grouped_relations(map.as_object().cloned().unwrap_or_default(), grouped);
    Ok(map)
}

/// `pidash_list_issue_relations` (`tools.py:433-448`): the explicit
/// ref resolves against the visible pool; `""` falls back to the
/// bound issue with no pool check (`relations_source` runs inside the
/// op, so its `ValueError` marks the ledger row failed).
async fn op_list_issue_relations(
    ctx: &ToolCtx,
    scope: &ToolScope,
    issue_arg: &str,
) -> Result<Value, ToolExecError> {
    // The read path holds no transaction: resolve_refs_tx and the
    // grouped read both take plain connections.
    let source = match plan::relations_source(issue_arg, scope.work_item_id)? {
        plan::RelationsSource::Ref(reference) => {
            let (found, unresolved) =
                resolve_refs_tx(&ctx.orm, scope, std::slice::from_ref(&reference), false).await?;
            if !unresolved.is_empty() {
                let refs: Vec<&str> = unresolved.iter().map(String::as_str).collect();
                return Err(
                    plan::ToolCallFailure::InvalidArg(plan::unresolved_refs_message(&refs)).into(),
                );
            }
            found.into_iter().next().expect("one ref resolves one row")
        }
        plan::RelationsSource::CurrentIssue => {
            let issue = bound_issue(&ctx.orm, scope.work_item_id).await?;
            let Some(issue) = issue else {
                // `relations.identifier(None)` reads `.project`
                // before `.sequence_id`.
                return Err(ToolExecError::AttributeError {
                    attribute: "project",
                });
            };
            rel::ResolvedIssue {
                id: issue.id,
                workspace_id: issue.workspace_id,
                project_id: issue.project_id,
                project_identifier: issue.project_identifier,
                sequence_id: issue.sequence_id,
            }
        }
    };
    let grouped = tool_grouped(&ctx.orm, scope, &source)
        .await
        .map_err(core_error)?;
    Ok(plan::issue_relations_result(&source.identifier(), grouped))
}

/// Publish one `_log_activity` emit transactionally (the
/// `RelationActivityEmit` contract): same audit transaction as the
/// relate/unrelate write; enqueue errors propagate (Python's `.delay`
/// has no `try`).
async fn enqueue_activity(
    txn: &WriteTxn,
    emit: &rel::RelationActivityEmit,
) -> Result<(), ToolExecError> {
    let job = NewJob::new(
        emit.task_name(),
        Value::Array(emit.args()),
        Value::Object(emit.kwargs()),
    );
    enqueue_in_write_txn(txn, &job).await?;
    Ok(())
}

/// The related-objects sweep `SoftDeleteModel.delete()` enqueues
/// (`.delay(app_label, model_name, pk, using=None)`): positionals
/// `("db", model, pk)`, kwargs `{"using": null}`.
fn soft_delete_sweep(model_name: &str, pk: &str) -> (Vec<Value>, Map<String, Value>) {
    let mut kwargs = Map::with_capacity(1);
    kwargs.insert("using".to_owned(), Value::Null);
    (
        vec![
            Value::String("db".to_owned()),
            Value::String(model_name.to_owned()),
            Value::String(pk.to_owned()),
        ],
        kwargs,
    )
}

/// Enqueue one sweep transactionally (same audit transaction; errors
/// propagate like the activity emits).
async fn enqueue_sweep(txn: &WriteTxn, model_name: &str, pk: &str) -> Result<(), ToolExecError> {
    let (args, kwargs) = soft_delete_sweep(model_name, pk);
    let job = NewJob::new(SOFT_DELETE_TASK, Value::Array(args), Value::Object(kwargs));
    enqueue_in_write_txn(txn, &job).await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// GitHub twins (`tools.py:450-554`)
// ---------------------------------------------------------------------------

/// The verified `_github_context`: the REST client plus the repo
/// coordinates it closes over.
struct GithubOpCtx {
    /// The installation client.
    client: GithubRestClient,
    /// `repo.namespace`.
    namespace: String,
    /// `repo.name`.
    name: String,
    /// `repo.default_branch` (may be NULL).
    default_branch: Option<String>,
}

/// `_github_context` (`tools.py:516-554`): the binding load (`.get()`
/// miss is `GitRepositoryBinding.DoesNotExist`), the verification
/// verdict, the installation lookup, and the installation client.
async fn github_context(ctx: &ToolCtx, scope: &ToolScope) -> Result<GithubOpCtx, ToolExecError> {
    use pidash_db::entity::{
        git_provider_accounts as acc_entity, git_repositories as repo_entity,
        git_repository_bindings as bind_entity, github_app_installations as inst_entity,
    };
    let db = sea_conn(&ctx.pool);
    let row = bind_entity::Entity::find()
        .filter(bind_entity::Column::ProjectId.eq(scope.project_id))
        .filter(bind_entity::Column::DeletedAt.is_null())
        .join(
            JoinType::InnerJoin,
            bind_entity::Entity::belongs_to(repo_entity::Entity)
                .from(bind_entity::Column::RepositoryId)
                .to(repo_entity::Column::Id)
                .into(),
        )
        .join(
            JoinType::InnerJoin,
            bind_entity::Entity::belongs_to(acc_entity::Entity)
                .from(bind_entity::Column::ProviderAccountId)
                .to(acc_entity::Column::Id)
                .into(),
        )
        .limit(1)
        .into_model::<GithubBindingRow>()
        .one(&db)
        .await
        .map_err(orm_error)?;
    let Some(row) = row else {
        return Err(ToolExecError::Missing {
            model: "GitRepositoryBinding",
        });
    };
    // The binding/account/repo columns carry Django's colliding names
    // (`workspace_id`, `provider`, `host_url`); here every joined
    // column decodes under its own alias.
    let verdict = plan::GithubBindingVerdict {
        binding_workspace_id: row.binding_workspace_id,
        run_workspace_id: scope.workspace_id,
        repo_provider: row.repo_provider,
        repo_host_url: row.repo_host_url,
        account_workspace_id: row.account_workspace_id,
        account_provider: row.account_provider,
        account_host_url: row.account_host_url,
        account_auth_type: row.account_auth_type,
        account_status: row.account_status,
        account_verified: row.account_verified_at.is_some(),
        account_workspace_integration_id: row.account_workspace_integration_id,
    };
    plan::check_github_binding(&verdict)?;
    let row_workspace_integration_id = row.account_workspace_integration_id;
    let installation_id = if let Some(integration) = row_workspace_integration_id {
        let row = inst_entity::Entity::find()
            .filter(inst_entity::Column::WorkspaceIntegrationId.eq(integration))
            .filter(inst_entity::Column::SuspendedAt.is_null())
            .filter(inst_entity::Column::VerifiedAt.is_not_null())
            .filter(inst_entity::Column::DeletedAt.is_null())
            .order_by_desc(inst_entity::Column::CreatedAt)
            .limit(1)
            .select_only()
            .column(inst_entity::Column::InstallationId)
            .into_tuple::<i64>()
            .one(&db)
            .await
            .map_err(orm_error)?;
        row
    } else {
        None
    };
    let installation_id = plan::check_github_installation(installation_id)?;
    let timeout_secs = ctx.cloud.tool_timeout_secs.max(1) as u64;
    let client =
        GithubRestClient::for_installation(&ctx.github, &ctx.http, installation_id, timeout_secs)
            .await?;
    Ok(GithubOpCtx {
        client,
        namespace: row.repo_namespace,
        name: row.repo_name,
        default_branch: Some(row.repo_default_branch),
    })
}

/// One `LINKED_PR_SQL` row.
#[derive(Clone, Debug, PartialEq, FromQueryResult)]
struct LinkedPrRow {
    namespace: String,
    repo_name: String,
    external_iid: String,
}

/// One `GITHUB_BINDING_SQL` row with every colliding name aliased.
#[derive(Clone, Debug, PartialEq, FromQueryResult)]
struct GithubBindingRow {
    binding_workspace_id: Uuid,
    repo_provider: String,
    repo_host_url: String,
    repo_namespace: String,
    repo_name: String,
    repo_default_branch: String,
    account_workspace_id: Uuid,
    account_provider: String,
    account_host_url: String,
    account_auth_type: String,
    account_status: String,
    account_verified_at: Option<DateTime<Utc>>,
    account_workspace_integration_id: Option<Uuid>,
}

/// `github_get_file` (`tools.py:450-480`). Path/ref pre-validation
/// runs in the dispatch; the op resolves the ref (`ref or
/// repo.default_branch or project.base_branch`) and reads.
async fn op_github_get_file(
    ctx: &ToolCtx,
    scope: &ToolScope,
    path: &str,
    ref_arg: &str,
) -> Result<Value, ToolExecError> {
    let repo = github_context(ctx, scope).await?;
    use pidash_db::entity::projects as proj_entity;
    let db = sea_conn(&ctx.pool);
    let base_branch = proj_entity::Entity::find_by_id(scope.project_id)
        .select_only()
        .column(proj_entity::Column::BaseBranch)
        .into_tuple::<String>()
        .one(&db)
        .await
        .map_err(orm_error)?;
    let git_ref = plan::resolve_file_ref(
        ref_arg,
        repo.default_branch.as_deref().unwrap_or(""),
        base_branch.as_deref().unwrap_or(""),
    );
    Ok(repo
        .client
        .get_file(&repo.namespace, &repo.name, path, git_ref)
        .await?)
}

/// `github_get_linked_pull_request` (`tools.py:482-510`): the link
/// lookup (a run with no bound issue, or no link, raises
/// `no_linked_pull_request`), the aspect dispatch over the link's
/// coordinates.
async fn op_github_get_linked_pull_request(
    ctx: &ToolCtx,
    scope: &ToolScope,
    aspect: &str,
) -> Result<Value, ToolExecError> {
    let repo = github_context(ctx, scope).await?;
    use pidash_db::entity::git_code_review_links as gc_entity;
    let db = sea_conn(&ctx.pool);
    let link: Option<LinkedPrRow> = match scope.work_item_id {
        Some(issue_id) => gc_entity::Entity::find()
            .filter(gc_entity::Column::IssueId.eq(issue_id))
            .filter(gc_entity::Column::Provider.eq("github"))
            .filter(gc_entity::Column::HostUrl.eq("https://github.com"))
            .filter(gc_entity::Column::Namespace.eq(repo.namespace.as_str()))
            .filter(gc_entity::Column::RepoName.eq(repo.name.as_str()))
            .filter(gc_entity::Column::DeletedAt.is_null())
            .order_by_desc(gc_entity::Column::CreatedAt)
            .into_model::<LinkedPrRow>()
            .one(&db)
            .await
            .map_err(orm_error)?,
        None => None,
    };
    let Some(link) = link else {
        return Err(
            plan::ToolCallFailure::Failed(plan::MSG_NO_LINKED_PULL_REQUEST.to_owned()).into(),
        );
    };
    let (namespace, repo_name, external_iid) = (link.namespace, link.repo_name, link.external_iid);
    let call = plan::linked_pr_call(aspect, &namespace, &repo_name, &external_iid)?;
    let client = &repo.client;
    match call.method {
        "get_pull_request" => Ok(client
            .get_pull_request(&call.namespace, &call.repo_name, call.number)
            .await?),
        "get_pull_request_diff" => Ok(client
            .get_pull_request_diff(&call.namespace, &call.repo_name, call.number)
            .await?),
        "list_pull_request_files" => {
            let files = client
                .list_pull_request_files(&call.namespace, &call.repo_name, call.number)
                .await?;
            serde_json::to_value(files).map_err(|error| {
                ToolExecError::Failure(plan::ToolCallFailure::Failed(error.to_string()))
            })
        }
        "list_pull_request_checks" => Ok(client
            .list_pull_request_checks(&call.namespace, &call.repo_name, call.number)
            .await?),
        "list_pull_request_reviews" => {
            let reviews = client
                .list_pull_request_reviews(&call.namespace, &call.repo_name, call.number)
                .await?;
            serde_json::to_value(reviews).map_err(|error| {
                ToolExecError::Failure(plan::ToolCallFailure::Failed(error.to_string()))
            })
        }
        "list_pull_request_comments" => Ok(client
            .list_pull_request_comments(&call.namespace, &call.repo_name, call.number)
            .await?),
        _ => unreachable!("validate_pr_aspect pins the method set"),
    }
}

// ---------------------------------------------------------------------------
// Dispatch (`build_tools`, `tools.py:182-513`)
// ---------------------------------------------------------------------------

/// Granted tool names (the `TOOL_CATALOG` order, `tools.py:356-370`).
pub const TOOL_GET_CURRENT_ISSUE: &str = "pidash_get_current_issue";
/// Granted tool names (the `TOOL_CATALOG` order, `tools.py:356-370`).
pub const TOOL_LIST_CURRENT_ISSUE_COMMENTS: &str = "pidash_list_current_issue_comments";
/// Granted tool names (the `TOOL_CATALOG` order, `tools.py:356-370`).
pub const TOOL_LIST_PROJECT_STATES: &str = "pidash_list_project_states";
/// Granted tool names (the `TOOL_CATALOG` order, `tools.py:356-370`).
pub const TOOL_SEARCH_PROJECT_ISSUES: &str = "pidash_search_project_issues";
/// Granted tool names (the `TOOL_CATALOG` order, `tools.py:356-370`).
pub const TOOL_GET_PROJECT_ISSUE: &str = "pidash_get_project_issue";
/// Granted tool names (the `TOOL_CATALOG` order, `tools.py:356-370`).
pub const TOOL_LIST_LINKED_CODE_REVIEWS: &str = "pidash_list_linked_code_reviews";
/// Granted tool names (the `TOOL_CATALOG` order, `tools.py:356-370`).
pub const TOOL_ADD_CURRENT_ISSUE_COMMENT: &str = "pidash_add_current_issue_comment";
/// Granted tool names (the `TOOL_CATALOG` order, `tools.py:356-370`).
pub const TOOL_UPDATE_CURRENT_ISSUE_WORKPAD: &str = "pidash_update_current_issue_workpad";
/// Granted tool names (the `TOOL_CATALOG` order, `tools.py:356-370`).
pub const TOOL_TRANSITION_CURRENT_ISSUE: &str = "pidash_transition_current_issue";
/// Granted tool names (the `TOOL_CATALOG` order, `tools.py:356-370`).
pub const TOOL_CREATE_PROJECT_ISSUE: &str = "pidash_create_project_issue";
/// Granted tool names (the `TOOL_CATALOG` order, `tools.py:356-370`).
pub const TOOL_RELATE_ISSUES: &str = "pidash_relate_issues";
/// Granted tool names (the `TOOL_CATALOG` order, `tools.py:356-370`).
pub const TOOL_UNRELATE_ISSUES: &str = "pidash_unrelate_issues";
/// Granted tool names (the `TOOL_CATALOG` order, `tools.py:356-370`).
pub const TOOL_LIST_ISSUE_RELATIONS: &str = "pidash_list_issue_relations";
/// Granted tool names (the `TOOL_CATALOG` order, `tools.py:356-370`).
pub const TOOL_GITHUB_GET_FILE: &str = "github_get_file";
/// Granted tool names (the `TOOL_CATALOG` order, `tools.py:356-370`).
pub const TOOL_GITHUB_GET_LINKED_PULL_REQUEST: &str = "github_get_linked_pull_request";

/// The loop's executor over one run: the shared context plus the run
/// id the tools close over. Failures surface as `Fatal` with
/// `str(exc)` — no `ModelRetry` escapes these tools.
pub struct RunTools {
    ctx: ToolCtx,
    run_id: Uuid,
}

impl RunTools {
    /// Bind the executor to one run.
    pub fn new(ctx: ToolCtx, run_id: Uuid) -> Self {
        Self { ctx, run_id }
    }
}

impl ToolExecutor for RunTools {
    async fn call_function(&self, name: &str, args: ValidatedArgs, _call_id: &str) -> ToolOutcome {
        match dispatch_fn(&self.ctx, self.run_id, name, &args).await {
            Ok(value) => ToolOutcome::Ok(value),
            Err(error) => ToolOutcome::Fatal(error.message()),
        }
    }

    async fn call_mcp(&self, name: &str, args: Map<String, Value>, _call_id: &str) -> ToolOutcome {
        match dispatch_mcp(&self.ctx, self.run_id, name, &args).await {
            Ok(value) => ToolOutcome::Ok(value),
            Err(error) => ToolOutcome::Fatal(error.message()),
        }
    }
}

/// Run one read tool under its audit: begin, op, settle.
async fn run_read(
    ctx: &ToolCtx,
    run_id: Uuid,
    tool: &'static str,
    args: &Value,
    source: &'static str,
    server_key: &'static str,
    op: impl AsyncFnOnce(&ToolScope) -> Result<Value, ToolExecError>,
) -> Result<Value, ToolExecError> {
    match read_begin(ctx, run_id, tool, args, source, server_key).await? {
        ReadAudit::Replay(value) => Ok(value),
        ReadAudit::Proceed { scope, ledger_id } => {
            let outcome = op(&scope).await;
            read_settle(ctx, run_id, tool, ledger_id, outcome).await
        }
    }
}

/// The domain-write error that forces a `domain::run` rollback after
/// the real [`ToolExecError`] is parked in the violation slot (the
/// callback must answer `DomainError`; the slot preserves the exact
/// variant — see [`run_write`]).
/// Map a domain-transaction failure onto the tool errors: begin and
/// commit carry no message of their own, so a `Db` error classifies
/// as `Db` (the wrapped sqlx error when there is one) and a `Work`
/// carries its message.
fn tool_domain_error(error: domain::DomainError) -> ToolExecError {
    match error {
        domain::DomainError::Db(error) => orm_error(error),
        domain::DomainError::Work(message) => ToolExecError::Db(sqlx::Error::Protocol(message)),
    }
}

/// Run one write tool under its audit on a domain [`WriteTxn`]
/// (NEWPIDASH-190 design §4.4, phase 1): kill switches and the replay
/// check stay pre-transaction; scope, admission, the ledger and `op`
/// share the one `WriteTxn`, so a write and its
/// `agent_run_tool_call` row land or roll back together. After the
/// commit (whose after-commit queue drained the op's own post-commit
/// steps, e.g. the create fire and the transition comment-back),
/// `tool_completed` is appended — as the tool contract requires — and
/// an emit failure still fails the tool after the commit.
///
/// The driver is manual `WriteTxn` (the primitive `domain::run` itself
/// uses) rather than `domain::run`: the op's `ToolExecError` variants
/// (`Missing`, `InvalidArg`, `Creation`, ...) must survive into
/// `dispatch_fn` unchanged, and `domain::run`'s `DomainError` cannot
/// carry them losslessly (a synthetic `Work` error would stringify
/// the variant away and change the failure mode `Str(exc)` reports).
///
/// All six write arms (workpad, comment, relate, unrelate, create,
/// transition) run on this wrapper.
async fn run_write(
    ctx: &ToolCtx,
    run_id: Uuid,
    tool: &'static str,
    args: &Value,
    op: impl AsyncFnOnce(&mut WriteTxn, &ToolScope) -> Result<Value, ToolExecError>,
) -> Result<Value, ToolExecError> {
    let call_id = plan::resolve_tool_call_id(None);
    let fingerprint = plan::fingerprint(args);
    plan::audit_guards(
        &ctx.cloud,
        tool,
        plan::ToolRisk::Write,
        plan::SOURCE_INTERNAL,
        "",
    )?;
    let existing = find_existing(&ctx.pool, run_id, &call_id).await?;
    match plan::replay_decision(existing.as_ref(), &fingerprint)? {
        plan::ReplayDecision::Replay(value) => return Ok(value),
        plan::ReplayDecision::Proceed => {}
    }
    let mut txn = WriteTxn::begin(&ctx.orm).await.map_err(tool_domain_error)?;
    let work: Result<Value, ToolExecError> = async {
        let scope = load_scope(&txn.connection(), run_id, true).await?;
        use pidash_db::entity::agent_run_tool_call as ledger_entity;
        let succeeded_writes: u64 = ledger_entity::Entity::find()
            .filter(ledger_entity::Column::AgentRunId.eq(run_id))
            .filter(ledger_entity::Column::Risk.eq("write"))
            .filter(ledger_entity::Column::Status.eq("succeeded"))
            .count(&txn.connection())
            .await
            .map_err(orm_error)?;
        let succeeded_writes: i64 = succeeded_writes.try_into().unwrap_or(i64::MAX);
        let tool_used = ledger_entity::Entity::find()
            .filter(ledger_entity::Column::AgentRunId.eq(run_id))
            .filter(ledger_entity::Column::ToolName.eq(tool))
            .filter(ledger_entity::Column::Risk.eq("write"))
            .filter(ledger_entity::Column::Status.eq("succeeded"))
            .select_only()
            .column(ledger_entity::Column::Id)
            .into_tuple::<Uuid>()
            .one(&txn.connection())
            .await
            .map_err(orm_error)?
            .is_some();
        plan::write_admission(succeeded_writes, tool_used, tool, &ctx.cloud)?;
        let ledger_id = Uuid::new_v4();
        let idempotency_hash =
            plan::fingerprint(&serde_json::json!({"run": run_id.to_string(), "tool": tool}));
        let now = Utc::now();
        ledger_entity::ActiveModel {
            id: sea_orm::Set(ledger_id),
            agent_run_id: sea_orm::Set(run_id),
            tool_call_id: sea_orm::Set(call_id.to_owned()),
            source: sea_orm::Set(plan::SOURCE_INTERNAL.to_owned()),
            server_key: sea_orm::Set("".to_owned()),
            tool_name: sea_orm::Set(tool.to_owned()),
            risk: sea_orm::Set("write".to_owned()),
            status: sea_orm::Set("prepared".to_owned()),
            request_fingerprint: sea_orm::Set(fingerprint),
            result_fingerprint: sea_orm::Set("".to_owned()),
            idempotency_key_hash: sea_orm::Set(idempotency_hash),
            external_operation_id: sea_orm::Set("".to_owned()),
            safe_replay_result: sea_orm::Set(None),
            error_code: sea_orm::Set("".to_owned()),
            prepared_at: sea_orm::Set(sea_orm::prelude::DateTimeWithTimeZone::from(now)),
            submitted_at: sea_orm::Set(None),
            completed_at: sea_orm::Set(None),
        }
        .insert(&txn.connection())
        .await
        .map_err(orm_error)?;
        ledger_entity::Entity::update_many()
            .col_expr(ledger_entity::Column::Status, Expr::value("submitted"))
            .col_expr(
                ledger_entity::Column::SubmittedAt,
                Expr::value(sea_orm::prelude::DateTimeWithTimeZone::from(now)),
            )
            .filter(ledger_entity::Column::Id.eq(ledger_id))
            .exec(&txn.connection())
            .await
            .map_err(orm_error)?;
        let result = op(&mut txn, &scope).await?;
        let bounded = plan::bounded(result.clone(), ctx.cloud.max_tool_result_bytes)?;
        let replay = if plan::collapse_replay(plan::canonical(&bounded).len()) {
            plan::collapsed_replay()
        } else {
            bounded.clone()
        };
        let result_fingerprint = plan::fingerprint(&bounded);
        let marked = Utc::now();
        ledger_entity::Entity::update_many()
            .col_expr(ledger_entity::Column::Status, Expr::value("succeeded"))
            .col_expr(ledger_entity::Column::SafeReplayResult, Expr::value(replay))
            .col_expr(
                ledger_entity::Column::ResultFingerprint,
                Expr::value(result_fingerprint),
            )
            .col_expr(
                ledger_entity::Column::CompletedAt,
                Expr::value(sea_orm::prelude::DateTimeWithTimeZone::from(marked)),
            )
            .filter(ledger_entity::Column::Id.eq(ledger_id))
            .exec(&txn.connection())
            .await
            .map_err(orm_error)?;
        Ok::<_, ToolExecError>(result)
    }
    .await;
    match work {
        Ok(value) => {
            // The op's after-commit steps (create fire, transition
            // comment-back) drained inside `commit`; `tool_completed`
            // appends after the commit.
            txn.commit().await.map_err(tool_domain_error)?;
            emit_completed(ctx, run_id, tool, plan::ToolRisk::Write).await?;
            Ok(value)
        }
        Err(error) => {
            let _ = txn.rollback().await;
            Err(error)
        }
    }
}

/// Dispatch one validated function-tool call: pre-audit validation,
/// audit args, then the op under its audit. Argument extraction
/// unwraps loop invariants (the loop validated required params and
/// applied defaults before calling).
async fn dispatch_fn(
    ctx: &ToolCtx,
    run_id: Uuid,
    name: &str,
    args: &ValidatedArgs,
) -> Result<Value, ToolExecError> {
    match name {
        TOOL_GET_CURRENT_ISSUE => {
            let audit_args = serde_json::json!({});
            run_read(
                ctx,
                run_id,
                TOOL_GET_CURRENT_ISSUE,
                &audit_args,
                plan::SOURCE_INTERNAL,
                "",
                async |scope| op_get_current_issue(ctx, scope).await,
            )
            .await
        }
        TOOL_LIST_CURRENT_ISSUE_COMMENTS => {
            let audit_args = serde_json::json!({});
            run_read(
                ctx,
                run_id,
                TOOL_LIST_CURRENT_ISSUE_COMMENTS,
                &audit_args,
                plan::SOURCE_INTERNAL,
                "",
                async |scope| op_list_current_issue_comments(ctx, scope).await,
            )
            .await
        }
        TOOL_LIST_PROJECT_STATES => {
            let audit_args = serde_json::json!({});
            run_read(
                ctx,
                run_id,
                TOOL_LIST_PROJECT_STATES,
                &audit_args,
                plan::SOURCE_INTERNAL,
                "",
                async |scope| op_list_project_states(ctx, scope).await,
            )
            .await
        }
        TOOL_SEARCH_PROJECT_ISSUES => {
            let query = args.get_str("query").expect("loop validated query");
            let limit =
                plan::clamp_search_limit(args.get_int("limit").expect("loop applied limit"));
            let audit_args = serde_json::json!({"query": query, "limit": limit});
            run_read(
                ctx,
                run_id,
                TOOL_SEARCH_PROJECT_ISSUES,
                &audit_args,
                plan::SOURCE_INTERNAL,
                "",
                async |scope| op_search_project_issues(ctx, scope, query, limit).await,
            )
            .await
        }
        TOOL_GET_PROJECT_ISSUE => {
            let issue_id = args.get_str("issue_id").expect("loop validated issue_id");
            let audit_args = serde_json::json!({"issue_id": issue_id});
            run_read(
                ctx,
                run_id,
                TOOL_GET_PROJECT_ISSUE,
                &audit_args,
                plan::SOURCE_INTERNAL,
                "",
                async |scope| op_get_project_issue(ctx, scope, issue_id).await,
            )
            .await
        }
        TOOL_LIST_LINKED_CODE_REVIEWS => {
            let audit_args = serde_json::json!({});
            run_read(
                ctx,
                run_id,
                TOOL_LIST_LINKED_CODE_REVIEWS,
                &audit_args,
                plan::SOURCE_INTERNAL,
                "",
                async |scope| op_list_linked_code_reviews(ctx, scope).await,
            )
            .await
        }
        TOOL_LIST_ISSUE_RELATIONS => {
            let issue = args.get_str("issue").expect("loop applied issue");
            let audit_args = serde_json::json!({"issue": issue});
            run_read(
                ctx,
                run_id,
                TOOL_LIST_ISSUE_RELATIONS,
                &audit_args,
                plan::SOURCE_INTERNAL,
                "",
                async |scope| op_list_issue_relations(ctx, scope, issue).await,
            )
            .await
        }
        TOOL_ADD_CURRENT_ISSUE_COMMENT => {
            let body = args.get_str("body").expect("loop validated body");
            plan::validate_comment_body(body)?;
            let audit_args = serde_json::json!({"body": body});
            run_write(
                ctx,
                run_id,
                TOOL_ADD_CURRENT_ISSUE_COMMENT,
                &audit_args,
                async move |txn, scope| op_add_current_issue_comment(txn, scope, body).await,
            )
            .await
        }
        TOOL_UPDATE_CURRENT_ISSUE_WORKPAD => {
            let body = args.get_str("body").expect("loop validated body");
            plan::validate_workpad_body(body)?;
            let audit_args = serde_json::json!({"body": body});
            run_write(
                ctx,
                run_id,
                TOOL_UPDATE_CURRENT_ISSUE_WORKPAD,
                &audit_args,
                async move |txn, scope| {
                    op_update_current_issue_workpad(ctx, txn, scope, body).await
                },
            )
            .await
        }
        TOOL_TRANSITION_CURRENT_ISSUE => {
            let state_id = args.get_str("state_id").expect("loop validated state_id");
            let audit_args = serde_json::json!({"state_id": state_id});
            run_write(
                ctx,
                run_id,
                TOOL_TRANSITION_CURRENT_ISSUE,
                &audit_args,
                async move |txn, scope| {
                    op_transition_current_issue(ctx, txn, scope, state_id).await
                },
            )
            .await
        }
        TOOL_CREATE_PROJECT_ISSUE => {
            let title = args.get_str("title").expect("loop validated title");
            let description = args
                .get_str("description")
                .expect("loop applied description");
            let stripped = plan::validate_create_issue(title, description)?;
            let audit_args = serde_json::json!({"title": title, "description": description});
            run_write(
                ctx,
                run_id,
                TOOL_CREATE_PROJECT_ISSUE,
                &audit_args,
                async move |txn, scope| {
                    op_create_project_issue(ctx, txn, scope, description, &stripped).await
                },
            )
            .await
        }
        TOOL_RELATE_ISSUES => {
            let write_args = plan::relation_write_args(
                args.get_str("issue").expect("loop validated issue"),
                args.get_str("relation_type")
                    .expect("loop validated relation_type"),
                args.get_str_list("related_issues")
                    .expect("loop validated related_issues")
                    .to_vec(),
                rel::validate_relation_type,
            )?;
            let audit_args = serde_json::json!({
                "issue": write_args.issue,
                "relation_type": write_args.relation_type,
                "related_issues": write_args.related_issues,
            });
            run_write(
                ctx,
                run_id,
                TOOL_RELATE_ISSUES,
                &audit_args,
                async move |txn, scope| op_relate_issues(txn, scope, &write_args).await,
            )
            .await
        }
        TOOL_UNRELATE_ISSUES => {
            let write_args = plan::relation_write_args(
                args.get_str("issue").expect("loop validated issue"),
                args.get_str("relation_type")
                    .expect("loop validated relation_type"),
                args.get_str_list("related_issues")
                    .expect("loop validated related_issues")
                    .to_vec(),
                rel::validate_relation_type,
            )?;
            let audit_args = serde_json::json!({
                "issue": write_args.issue,
                "relation_type": write_args.relation_type,
                "related_issues": write_args.related_issues,
            });
            run_write(
                ctx,
                run_id,
                TOOL_UNRELATE_ISSUES,
                &audit_args,
                async move |txn, scope| op_unrelate_issues(txn, scope, &write_args).await,
            )
            .await
        }
        // GitHub twins (`TOOL_GITHUB_*`) grant as MCP tools only
        // (`partition_tools` excludes them from the internal build);
        // the loop never routes them — or anything else unknown — here.
        _ => Err(plan::ToolCallFailure::Failed(format!("unexpected tool {name}")).into()),
    }
}

/// Dispatch one MCP-twin call: the loop jsonschema-validated `args`
/// but filled no defaults, so `ref`/`aspect` fall back to the Python
/// signature defaults (`""`/`"summary"`). Same ops as the sync
/// bodies, audited with `source="mcp"`.
async fn dispatch_mcp(
    ctx: &ToolCtx,
    run_id: Uuid,
    name: &str,
    args: &Map<String, Value>,
) -> Result<Value, ToolExecError> {
    match name {
        TOOL_GITHUB_GET_FILE => {
            let path = args
                .get("path")
                .and_then(Value::as_str)
                .expect("loop validated path");
            let ref_arg = args.get("ref").and_then(Value::as_str).unwrap_or("");
            plan::validate_github_path(path)?;
            plan::validate_github_ref(ref_arg)?;
            let audit_args = serde_json::json!({"path": path, "ref": ref_arg});
            run_read(
                ctx,
                run_id,
                TOOL_GITHUB_GET_FILE,
                &audit_args,
                plan::SOURCE_MCP,
                plan::SERVER_KEY_GITHUB,
                async |scope| op_github_get_file(ctx, scope, path, ref_arg).await,
            )
            .await
        }
        TOOL_GITHUB_GET_LINKED_PULL_REQUEST => {
            let aspect = args
                .get("aspect")
                .and_then(Value::as_str)
                .unwrap_or("summary");
            plan::validate_pr_aspect(aspect)?;
            let audit_args = serde_json::json!({"aspect": aspect});
            run_read(
                ctx,
                run_id,
                TOOL_GITHUB_GET_LINKED_PULL_REQUEST,
                &audit_args,
                plan::SOURCE_MCP,
                plan::SERVER_KEY_GITHUB,
                async |scope| op_github_get_linked_pull_request(ctx, scope, aspect).await,
            )
            .await
        }
        _ => Err(plan::ToolCallFailure::Failed(format!("unexpected tool {name}")).into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pidash_db::tasks_ticker::models::issue_agent_ticker::IssueAgentTicker;
    use pidash_services::orchestration::clock::{on_enter_or_move, ClockIssue, ProjectClockPolicy};
    use pidash_types::orchestration::ticker::TickerEvent;

    /// The create-path project lock pins `CREATE_PROJECT_LOCK_SQL`:
    /// the live row only (`deleted_at IS NULL`, Django's
    /// `SoftDeletionManager`) under an exclusive lock.
    #[test]
    fn project_lock_query_pins_deleted_filter_and_lock() {
        use sea_orm::{DbBackend, QueryTrait};
        let sql = project_lock_query(Uuid::nil())
            .build(DbBackend::Postgres)
            .to_string();
        assert!(
            sql.contains(r#""deleted_at" IS NULL"#),
            "lock must exclude soft-deleted projects: {sql}"
        );
        assert!(sql.contains("FOR UPDATE"), "lock takes the row lock: {sql}");
    }

    /// The transition target lookup projects every `IssueStateView`
    /// field (`id`, `name`, `group`) in the run's project, excluding
    /// soft-deleted and triage states — otherwise SeaORM's
    /// `FromQueryResult` fails the decode and every transition call
    /// ends the run `provider_error` (NEWPIDASH-173).
    #[test]
    fn transition_state_query_projects_issue_state_view() {
        use sea_orm::{DbBackend, QueryTrait};
        let sql = transition_state_query(Uuid::nil(), Uuid::nil())
            .build(DbBackend::Postgres)
            .to_string();
        assert!(
            sql.contains(r#""states"."id", "states"."name", "states"."group""#),
            "transition lookup must select id, name and group: {sql}"
        );
        assert!(
            sql.contains(r#""deleted_at" IS NULL"#),
            "lookup must exclude soft-deleted states: {sql}"
        );
    }

    /// The CPython `uuid.UUID(hex=...)` acceptance battery (probed on
    /// 3.12): simple, braced, lowercase-`urn:`, misplaced-hyphen, and
    /// double-braced spellings accept; uppercase `URN:`, whitespace,
    /// bad lengths, and junk reject.
    #[test]
    fn parse_django_uuid_acceptance() {
        let canonical = "12345678-1234-5678-1234-567812345678";
        let expected = Uuid::parse_str(canonical).unwrap();
        let accept = [
            "12345678123456781234567812345678",
            "{12345678-1234-5678-1234-567812345678}",
            "urn:uuid:12345678-1234-5678-1234-567812345678",
            "123456781234-5678-1234-567812345678",
            "{{12345678-1234-5678-1234-567812345678}}",
        ];
        for text in accept {
            assert_eq!(parse_django_uuid(text), Some(expected), "{text}");
        }
        // 31 digits plus one underscore: `int(hex, 16)` accepts, so
        // the value parses to the smaller int.
        let short = parse_django_uuid("1234567_123456781234567812345678").unwrap();
        assert_eq!(
            short,
            Uuid::from_u128(u128::from_str_radix("1234567123456781234567812345678", 16).unwrap())
        );
        let reject = [
            "URN:UUID:12345678-1234-5678-1234-567812345678",
            "  12345678-1234-5678-1234-567812345678  ",
            "12345678-1234-5678-1234-56781234567",
            "12345678-1234-5678-1234-5678123456789",
            "12345678-1234_5678-1234-567812345678",
            "12345678__123456781234567812345678",
            "_1234567812345678123456781234567",
            "1234567812345678123456781234567_",
            "nope",
            "",
            "ZZZZZZZZ-1234-5678-1234-567812345678",
        ];
        for text in reject {
            assert_eq!(parse_django_uuid(text), None, "{text}");
        }
    }

    /// The Django error renderings: `DoesNotExist`, the
    /// `ValidationError` messages-list repr (curly quotes), and the
    /// `NoneType` attribute message.
    #[test]
    fn exec_error_messages() {
        let missing = ToolExecError::Missing { model: "Issue" };
        assert_eq!(missing.python_type(), "DoesNotExist");
        assert_eq!(missing.message(), "Issue matching query does not exist.");
        let invalid = ToolExecError::InvalidUuid {
            value: "nope".to_owned(),
        };
        assert_eq!(invalid.python_type(), "ValidationError");
        assert_eq!(invalid.message(), "['“nope” is not a valid UUID.']");
        let attribute = ToolExecError::AttributeError {
            attribute: "project",
        };
        assert_eq!(attribute.python_type(), "AttributeError");
        assert_eq!(
            attribute.message(),
            "'NoneType' object has no attribute 'project'."
        );
    }

    /// The related-objects sweep shape: positionals
    /// `("db", model, pk)`, kwargs `{"using": null}`.
    #[test]
    fn soft_delete_sweep_shape() {
        let (args, kwargs) = soft_delete_sweep("issuerelation", "some-pk");
        assert_eq!(
            args,
            vec![
                Value::String("db".to_owned()),
                Value::String("issuerelation".to_owned()),
                Value::String("some-pk".to_owned()),
            ]
        );
        assert_eq!(kwargs.len(), 1);
        assert_eq!(kwargs.get("using"), Some(&Value::Null));
    }

    fn agent_ticker() -> IssueAgentTicker {
        let now = Utc::now();
        IssueAgentTicker {
            id: Uuid::new_v4(),
            created_at: now,
            updated_at: now,
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            issue_id: Uuid::new_v4(),
            used: 0,
            granted: 0,
            waited: 0,
            user_disabled: false,
            next_run_at: None,
            last_tick_at: None,
            enabled: true,
            disarm_reason: String::new(),
            pending_entry: false,
            pending_entry_free: false,
            pending_entry_actor_id: None,
            pending_entry_trigger: String::new(),
            resume_parent_run_id: None,
        }
    }

    fn stateless_issue(policy: ProjectClockPolicy) -> ClockIssue<'static> {
        ClockIssue {
            issue_id: Uuid::new_v4(),
            state: None,
            policy,
        }
    }

    /// The contract `BinPreflight` relies on: agent moves
    /// (`moved_by_run` set) never set `dispatch_now` — pool spent
    /// parks, a disabled clock reports `ticking-disabled`, and an
    /// allowed clock queues (`scheduling.py:412-436`). Both entry
    /// events, all three arms.
    #[test]
    fn agent_moves_never_dispatch_now() {
        let mover = Uuid::new_v4();
        for make_event in [
            TickerEvent::moved_stage(Some(mover), None, true, None),
            TickerEvent::entered_bucket(Some(mover), None, true, None),
        ] {
            // Pool spent: park.
            let mut ticker = agent_ticker();
            ticker.used = 5;
            let policy = ProjectClockPolicy {
                agent_default_max_ticks: Some(5),
                ..ProjectClockPolicy::default()
            };
            let outcome = on_enter_or_move(
                &mut ticker,
                &stateless_issue(policy),
                &make_event,
                false,
                Utc::now(),
                0.0,
            );
            assert!(!outcome.decision.dispatch_now, "cap arm dispatches");
            assert!(outcome.decision.parked, "cap arm parks");
            assert!(
                matches!(
                    outcome.write,
                    pidash_services::orchestration::clock::ClockWrite::Update
                ),
                "cap arm persists"
            );

            // Clock disallowed: report, dispatch nothing.
            let mut ticker = agent_ticker();
            ticker.user_disabled = true;
            let outcome = on_enter_or_move(
                &mut ticker,
                &stateless_issue(ProjectClockPolicy::default()),
                &make_event,
                false,
                Utc::now(),
                0.0,
            );
            assert!(!outcome.decision.dispatch_now, "disabled arm dispatches");
            assert!(!outcome.decision.queued, "disabled arm queues");
            assert!(!outcome.decision.parked, "disabled arm parks");
            assert_eq!(outcome.decision.reason.as_str(), "ticking-disabled");

            // Allowed clock: queue (never dispatch).
            let mut ticker = agent_ticker();
            let outcome = on_enter_or_move(
                &mut ticker,
                &stateless_issue(ProjectClockPolicy::default()),
                &make_event,
                false,
                Utc::now(),
                0.0,
            );
            assert!(!outcome.decision.dispatch_now, "queue arm dispatches");
            assert!(outcome.decision.queued, "queue arm queues");
            assert!(
                matches!(
                    outcome.write,
                    pidash_services::orchestration::clock::ClockWrite::Update
                ),
                "queue arm persists"
            );
        }
    }

    /// `datetime.isoformat()`: micros iff nonzero, `+00:00` offset.
    #[test]
    fn isoformat_renders_like_python() {
        use chrono::TimeZone;
        let whole = Utc.with_ymd_and_hms(2026, 3, 4, 5, 6, 7).unwrap();
        assert_eq!(render_isoformat(whole), "2026-03-04T05:06:07+00:00");
        let micro = whole + chrono::Duration::microseconds(123_456);
        assert_eq!(render_isoformat(micro), "2026-03-04T05:06:07.123456+00:00");
        let milli = whole + chrono::Duration::milliseconds(120);
        assert_eq!(render_isoformat(milli), "2026-03-04T05:06:07.120000+00:00");
    }
}

#[cfg(test)]
mod live_tests {
    use super::*;

    async fn live_pool() -> Option<sqlx::PgPool> {
        match std::env::var("DATABASE_URL") {
            Ok(url) => Some(
                sqlx::postgres::PgPoolOptions::new()
                    .max_connections(5)
                    .connect(&url)
                    .await
                    .expect("connect to scratch DATABASE_URL"),
            ),
            Err(_) => {
                eprintln!("skipping live-db test: DATABASE_URL is not set");
                None
            }
        }
    }

    struct Seed {
        scope: ToolScope,
    }

    /// Seed user → workspace → project → issue through the entities.
    /// Every unique column carries a random tag so reruns never
    /// collide.
    async fn seed_world(pool: &sqlx::PgPool) -> Seed {
        use pidash_db::entity::{
            issues as i_entity, projects as proj_entity, states as st_entity, users as u_entity,
            workspaces as ws_entity,
        };
        use sea_orm::{ActiveModelTrait, Set};
        let db = pidash_db::pool::sea_orm_connection(pool);
        let tag: String = Uuid::new_v4().simple().to_string()[..10].to_owned();
        let user_id = Uuid::new_v4();
        let now = chrono::Utc::now();
        let now_tz = sea_orm::prelude::DateTimeWithTimeZone::from(now);
        u_entity::ActiveModel {
            id: Set(user_id),
            password: Set("pbkdf2_sha256$600000$salt$hash".to_owned()),
            email: Set(Some(format!("rs1581-{tag}@example.com"))),
            username: Set(format!("rs1581_{tag}")),
            display_name: Set(String::new()),
            first_name: Set(String::new()),
            last_name: Set(String::new()),
            avatar: Set(String::new()),
            date_joined: Set(now_tz),
            created_at: Set(now_tz),
            updated_at: Set(now_tz),
            last_location: Set(String::new()),
            created_location: Set(String::new()),
            is_superuser: Set(false),
            is_managed: Set(false),
            is_password_expired: Set(false),
            is_active: Set(true),
            is_staff: Set(false),
            is_email_verified: Set(false),
            is_password_autoset: Set(false),
            is_password_reset_required: Set(false),
            token: Set(String::new()),
            last_active: Set(None),
            last_login_ip: Set(String::new()),
            last_logout_ip: Set(String::new()),
            last_login_medium: Set("email".to_owned()),
            last_login_uagent: Set(String::new()),
            is_bot: Set(false),
            user_timezone: Set("UTC".to_owned()),
            is_email_valid: Set(true),
            ..Default::default()
        }
        .insert(&db)
        .await
        .expect("seed user");
        let workspace_id = Uuid::new_v4();
        let slug = format!("rs1581-{tag}");
        ws_entity::ActiveModel {
            id: Set(workspace_id),
            name: Set(format!("RS1581 {tag}")),
            owner_id: Set(user_id),
            slug: Set(slug.clone()),
            timezone: Set("UTC".to_owned()),
            background_color: Set("#ffffff".to_owned()),
            created_at: Set(now_tz),
            updated_at: Set(now_tz),
            ..Default::default()
        }
        .insert(&db)
        .await
        .expect("seed workspace");
        let project_id = Uuid::new_v4();
        proj_entity::ActiveModel {
            id: Set(project_id),
            name: Set(format!("RS1581 {tag}")),
            description: Set(String::new()),
            network: Set(2),
            workspace_id: Set(workspace_id),
            identifier: Set(format!("RS{}", tag.to_uppercase())),
            module_view: Set(false),
            cycle_view: Set(false),
            issue_views_view: Set(false),
            page_view: Set(true),
            intake_view: Set(false),
            is_time_tracking_enabled: Set(false),
            is_issue_type_enabled: Set(false),
            is_default: Set(false),
            guest_view_all_features: Set(false),
            members_can_edit_states: Set(true),
            dedicated_runner_lost_action: Set("fail_to_backlog".to_owned()),
            archive_in: Set(0),
            close_in: Set(0),
            logo_props: Set(serde_json::json!({})),
            timezone: Set("UTC".to_owned()),
            repo_url: Set(String::new()),
            base_branch: Set("main".to_owned()),
            agent_default_interval_seconds: Set(10800),
            agent_default_max_ticks: Set(10),
            agent_review_default_interval_seconds: Set(10800),
            agent_test_default_interval_seconds: Set(10800),
            agent_ticking_enabled: Set(true),
            default_agent_executor: Set("local_runner".to_owned()),
            created_at: Set(now_tz),
            updated_at: Set(now_tz),
            ..Default::default()
        }
        .insert(&db)
        .await
        .expect("seed project");
        let state_id = Uuid::new_v4();
        st_entity::ActiveModel {
            id: Set(state_id),
            workspace_id: Set(workspace_id),
            project_id: Set(project_id),
            name: Set("Backlog".to_owned()),
            description: Set(String::new()),
            color: Set("#ff0000".to_owned()),
            slug: Set(String::new()),
            sequence: Set(65535.0),
            group: Set("backlog".to_owned()),
            is_triage: Set(false),
            default: Set(false),
            created_at: Set(now_tz),
            updated_at: Set(now_tz),
            ..Default::default()
        }
        .insert(&db)
        .await
        .expect("seed state");
        let issue_id = Uuid::new_v4();
        i_entity::ActiveModel {
            id: Set(issue_id),
            workspace_id: Set(workspace_id),
            project_id: Set(project_id),
            state_id: Set(Some(state_id)),
            name: Set("Tool issue".to_owned()),
            description_json: Set(serde_json::json!({})),
            description_html: Set("<p></p>".to_owned()),
            priority: Set("none".to_owned()),
            complexity_score: Set(0),
            sequence_id: Set(1),
            sort_order: Set(65535.0),
            is_draft: Set(false),
            git_work_branch: Set(String::new()),
            workpad: Set(String::new()),
            created_at: Set(now_tz),
            updated_at: Set(now_tz),
            ..Default::default()
        }
        .insert(&db)
        .await
        .expect("seed issue");
        Seed {
            scope: ToolScope {
                run_id: Uuid::new_v4(),
                workspace_id,
                workspace_slug: slug,
                creator_id: user_id,
                work_item_id: Some(issue_id),
                scheduler_binding_id: None,
                project_id,
            },
        }
    }

    async fn cleanup(pool: &sqlx::PgPool, scope: &ToolScope, comment_id: Uuid) {
        use pidash_db::entity::{
            descriptions as d_entity, issue_comments as ic_entity, issue_sequences as iseq_entity,
            issues as i_entity, projects as proj_entity, states as st_entity,
            user_notification_preferences as unp_entity, users as u_entity,
            workspaces as ws_entity,
        };
        use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
        let db = pidash_db::pool::sea_orm_connection(pool);
        let comment = ic_entity::Entity::find_by_id(comment_id)
            .one(&db)
            .await
            .expect("read back comment");
        let description_id = comment.and_then(|c| c.description_id);
        ic_entity::Entity::delete_by_id(comment_id)
            .exec(&db)
            .await
            .expect("teardown comment");
        if let Some(description_id) = description_id {
            d_entity::Entity::delete_by_id(description_id)
                .exec(&db)
                .await
                .expect("teardown description");
        }
        // The `issues` entity's `after_save` hook writes the
        // `issue_sequences` row on insert (Django's `IssueSequence`
        // create); raw-SQL seeds never fired it, so the row is new
        // with the SeaORM seed and must go before the issue (FK).
        iseq_entity::Entity::delete_many()
            .filter(iseq_entity::Column::IssueId.eq(scope.work_item_id.expect("bound issue")))
            .exec(&db)
            .await
            .expect("teardown issue sequence");
        i_entity::Entity::delete_many()
            .filter(i_entity::Column::Id.eq(scope.work_item_id.expect("bound issue")))
            .exec(&db)
            .await
            .expect("teardown issue");
        st_entity::Entity::delete_many()
            .filter(st_entity::Column::ProjectId.eq(scope.project_id))
            .exec(&db)
            .await
            .expect("teardown states");
        proj_entity::Entity::delete_many()
            .filter(proj_entity::Column::Id.eq(scope.project_id))
            .exec(&db)
            .await
            .expect("teardown project");
        ws_entity::Entity::delete_many()
            .filter(ws_entity::Column::Id.eq(scope.workspace_id))
            .exec(&db)
            .await
            .expect("teardown workspace");
        // The `users` entity's `after_save` hook writes the
        // `user_notification_preferences` row on non-bot insert
        // (Django `create_user_notification`); same FK story as the
        // sequence row above.
        unp_entity::Entity::delete_many()
            .filter(unp_entity::Column::UserId.eq(scope.creator_id))
            .exec(&db)
            .await
            .expect("teardown notification prefs");
        u_entity::Entity::delete_many()
            .filter(u_entity::Column::Id.eq(scope.creator_id))
            .exec(&db)
            .await
            .expect("teardown user");
    }

    /// `pidash_add_current_issue_comment` strips each column with its
    /// own stripper (PIDASHCONV-1581 item 1): the comment column
    /// decodes entities (MLStripper, `issue.py:606`), the description
    /// mirror keeps them verbatim (`Description.save`,
    /// `description.py:23-29`).
    #[tokio::test]
    async fn add_comment_splits_strippers_per_column() {
        let Some(pool) = live_pool().await else {
            return;
        };
        let seed = seed_world(&pool).await;
        let db = pidash_db::pool::sea_orm_connection(&pool);
        let mut txn = pidash_services::domain::WriteTxn::begin(&db)
            .await
            .expect("begin");
        let out = op_add_current_issue_comment(&mut txn, &seed.scope, "a & b")
            .await
            .expect("add comment");
        txn.commit().await.expect("commit");
        let comment_id: Uuid = out["comment_id"]
            .as_str()
            .expect("comment_id renders")
            .parse()
            .expect("comment_id parses");
        use pidash_db::entity::{descriptions as d_entity2, issue_comments as ic_entity2};
        use sea_orm::EntityTrait;
        let comment = ic_entity2::Entity::find_by_id(comment_id)
            .one(&db)
            .await
            .expect("read back the comment")
            .expect("comment row");
        assert_eq!(comment.comment_html, "<p>a &amp; b</p>");
        assert_eq!(comment.comment_stripped, "a & b");
        let description_id = comment.description_id.expect("comment links a description");
        let desc = d_entity2::Entity::find_by_id(description_id)
            .one(&db)
            .await
            .expect("read back the description")
            .expect("description row");
        assert_eq!(desc.description_stripped.as_deref(), Some("a &amp; b"));
        cleanup(&pool, &seed.scope, comment_id).await;
    }

    // ------------------------------------------------------------------
    // Phase 0 characterization (NEWPIDASH-190): the six agent write
    // tools through `dispatch_fn` — rows, result JSON, enqueued jobs
    // and ledger atomicity, pinned before the `operations::core`
    // refactor. Must pass on main unchanged.
    // ------------------------------------------------------------------
    use sqlx::Row;

    const ACTIVITY_TASK: &str = "pi_dash.bgtasks.issue_activities_task.issue_activity";
    const SWEEP_TASK: &str = "pi_dash.bgtasks.deletion_task.soft_delete_related_objects";
    const RELATE_CREATED_ACTIVITY: &str = "issue_relation.activity.created";
    const RELATE_DELETED_ACTIVITY: &str = "issue_relation.activity.deleted";

    struct Phase0Seed {
        user_id: Uuid,
        workspace_id: Uuid,
        project_id: Uuid,
        project_identifier: String,
        state_backlog: Uuid,
        state_second: Uuid,
        issue_id: Uuid,
        target_id: Uuid,
        pod_id: Uuid,
        scheduler_id: Uuid,
        binding_id: Uuid,
    }

    /// Test transition hook: the seeded tools never invoke it (the
    /// create fire is skipped in-transaction; the post-commit fire
    /// parks without LLM config), but domain writes resolve it eagerly.
    #[derive(Debug)]
    struct NoopTransitionHook;

    impl pidash_services::domain::transition::IssueTransitionHook for NoopTransitionHook {
        fn enqueue_completion_comment(
            &self,
            _issue_id: Uuid,
            _prev_state_id: Option<Uuid>,
            _current_state_id: Option<Uuid>,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>> {
            Box::pin(async move {})
        }

        fn fire(
            &self,
            _actor_id: Uuid,
            _request: pidash_services::orchestration::entries::FireRequest,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<
                        Output = Result<pidash_services::orchestration::entries::FireOutcome, ()>,
                    > + Send
                    + '_,
            >,
        > {
            Box::pin(async move {
                Ok(pidash_services::orchestration::entries::FireOutcome::NoTransition)
            })
        }
    }

    fn phase0_ctx(pool: &sqlx::PgPool) -> ToolCtx {
        let settings = pidash_db::config::Settings::test_defaults();
        let mut cloud = settings.cloud_agent.clone();
        cloud.enabled = true;
        cloud.writes_enabled = true;
        ToolCtx {
            pool: pool.clone(),
            orm: pidash_db::pool::sea_orm_connection(pool),
            cloud,
            managed: settings.managed_runner.clone(),
            http: reqwest::Client::new(),
            github: crate::oss_worker::model_runtime::github::GithubAppContext::new(
                pool.clone(),
                pidash_db::redis::RedisHandle::from_settings(&settings),
                settings.secret_key.clone(),
            ),
            toolsets: std::sync::Arc::new(pidash_services::extensions::NoExtraToolsets),
            redis: None,
            runner: settings.runner.clone(),
            transition_hook: std::sync::Arc::new(NoopTransitionHook),
        }
    }

    async fn phase0_ensure_queue(pool: &sqlx::PgPool) {
        crate::queue::ensure_schema(pool)
            .await
            .expect("queue schema");
    }

    /// Insert one `agent_run` for the seeded world (work item bound,
    /// or missing-issue when `None`). Returns the run id.
    async fn phase0_run(pool: &sqlx::PgPool, seed: &Phase0Seed, work_item: Option<Uuid>) -> Uuid {
        let run_id = Uuid::new_v4();
        let now = chrono::Utc::now();
        sqlx::query(
            "INSERT INTO agent_run (id, status, prompt, run_config, required_capabilities, \
             thread_id, created_at, workspace_id, pod_id, created_by_id, \
             scheduler_binding_id, work_item_id, llm_model, trigger, executor_kind, \
             dispatch_attempts, cancel_reason, tool_plan, phase_kind, agent_metadata, \
             usage, error_details, pin_kind) VALUES ($1, 'running', '', '{}', '{}', '', $2, \
             $3, $4, $5, $6, $7, '', 'manual', 'cloud_agent', 0, '', '{}', 'main', '{}', \
             '{}', '{}', 'locality')",
        )
        .bind(run_id)
        .bind(now)
        .bind(seed_workspace_id(seed))
        .bind(seed.pod_id)
        .bind(seed.user_id)
        .bind(seed.binding_id)
        .bind(work_item)
        .execute(pool)
        .await
        .expect("seed agent run");
        run_id
    }

    fn seed_workspace_id(seed: &Phase0Seed) -> Uuid {
        seed.workspace_id
    }

    /// Seed the full dispatch world: user → workspace → project →
    /// two states → two issues → pod → scheduler + binding →
    /// workspace/project memberships. The run itself comes from
    /// [`phase0_run`] so missing-issue cases can choose its shape.
    async fn seed_phase0(pool: &sqlx::PgPool) -> Phase0Seed {
        use pidash_db::entity::{
            issues as i_entity, projects as proj_entity, states as st_entity, users as u_entity,
            workspaces as ws_entity,
        };
        use sea_orm::{ActiveModelTrait, Set};
        let db = pidash_db::pool::sea_orm_connection(pool);
        let tag: String = Uuid::new_v4().simple().to_string()[..10].to_owned();
        let user_id = Uuid::new_v4();
        let now = chrono::Utc::now();
        let now_tz = sea_orm::prelude::DateTimeWithTimeZone::from(now);
        u_entity::ActiveModel {
            id: Set(user_id),
            password: Set("pbkdf2_sha256$600000$salt$hash".to_owned()),
            email: Set(Some(format!("p0-{tag}@example.com"))),
            username: Set(format!("p0_{tag}")),
            display_name: Set(String::new()),
            first_name: Set(String::new()),
            last_name: Set(String::new()),
            avatar: Set(String::new()),
            date_joined: Set(now_tz),
            created_at: Set(now_tz),
            updated_at: Set(now_tz),
            last_location: Set(String::new()),
            created_location: Set(String::new()),
            is_superuser: Set(false),
            is_managed: Set(false),
            is_password_expired: Set(false),
            is_active: Set(true),
            is_staff: Set(false),
            is_email_verified: Set(false),
            is_password_autoset: Set(false),
            is_password_reset_required: Set(false),
            token: Set(String::new()),
            last_active: Set(None),
            last_login_ip: Set(String::new()),
            last_logout_ip: Set(String::new()),
            last_login_medium: Set("email".to_owned()),
            last_login_uagent: Set(String::new()),
            is_bot: Set(false),
            user_timezone: Set("UTC".to_owned()),
            is_email_valid: Set(true),
            ..Default::default()
        }
        .insert(&db)
        .await
        .expect("seed user");
        let workspace_id = Uuid::new_v4();
        let slug = format!("p0-{tag}");
        ws_entity::ActiveModel {
            id: Set(workspace_id),
            name: Set(format!("P0 {tag}")),
            owner_id: Set(user_id),
            slug: Set(slug.clone()),
            timezone: Set("UTC".to_owned()),
            background_color: Set("#ffffff".to_owned()),
            created_at: Set(now_tz),
            updated_at: Set(now_tz),
            ..Default::default()
        }
        .insert(&db)
        .await
        .expect("seed workspace");
        let project_id = Uuid::new_v4();
        let project_identifier = format!("P0{}", tag.to_uppercase());
        proj_entity::ActiveModel {
            id: Set(project_id),
            name: Set(format!("P0 {tag}")),
            description: Set(String::new()),
            network: Set(2),
            workspace_id: Set(workspace_id),
            identifier: Set(project_identifier.clone()),
            module_view: Set(false),
            cycle_view: Set(false),
            issue_views_view: Set(false),
            page_view: Set(true),
            intake_view: Set(false),
            is_time_tracking_enabled: Set(false),
            is_issue_type_enabled: Set(false),
            is_default: Set(false),
            guest_view_all_features: Set(false),
            members_can_edit_states: Set(true),
            dedicated_runner_lost_action: Set("fail_to_backlog".to_owned()),
            archive_in: Set(0),
            close_in: Set(0),
            logo_props: Set(serde_json::json!({})),
            timezone: Set("UTC".to_owned()),
            repo_url: Set(String::new()),
            base_branch: Set("main".to_owned()),
            agent_default_interval_seconds: Set(10800),
            agent_default_max_ticks: Set(10),
            agent_review_default_interval_seconds: Set(10800),
            agent_test_default_interval_seconds: Set(10800),
            agent_ticking_enabled: Set(true),
            default_agent_executor: Set("local_runner".to_owned()),
            created_at: Set(now_tz),
            updated_at: Set(now_tz),
            ..Default::default()
        }
        .insert(&db)
        .await
        .expect("seed project");
        let state_backlog = Uuid::new_v4();
        st_entity::ActiveModel {
            id: Set(state_backlog),
            workspace_id: Set(workspace_id),
            project_id: Set(project_id),
            name: Set("Backlog".to_owned()),
            description: Set(String::new()),
            color: Set("#ff0000".to_owned()),
            slug: Set(String::new()),
            sequence: Set(65535.0),
            group: Set("backlog".to_owned()),
            is_triage: Set(false),
            default: Set(true),
            created_at: Set(now_tz),
            updated_at: Set(now_tz),
            ..Default::default()
        }
        .insert(&db)
        .await
        .expect("seed backlog state");
        let state_second = Uuid::new_v4();
        st_entity::ActiveModel {
            id: Set(state_second),
            workspace_id: Set(workspace_id),
            project_id: Set(project_id),
            name: Set("Todo".to_owned()),
            description: Set(String::new()),
            color: Set("#00ff00".to_owned()),
            slug: Set(String::new()),
            sequence: Set(65535.0),
            group: Set("unstarted".to_owned()),
            is_triage: Set(false),
            default: Set(false),
            created_at: Set(now_tz),
            updated_at: Set(now_tz),
            ..Default::default()
        }
        .insert(&db)
        .await
        .expect("seed second state");
        let issue_id = Uuid::new_v4();
        i_entity::ActiveModel {
            id: Set(issue_id),
            workspace_id: Set(workspace_id),
            project_id: Set(project_id),
            state_id: Set(Some(state_backlog)),
            name: Set("Phase0 issue".to_owned()),
            description_json: Set(serde_json::json!({})),
            description_html: Set("<p></p>".to_owned()),
            priority: Set("none".to_owned()),
            complexity_score: Set(0),
            sequence_id: Set(1),
            sort_order: Set(65535.0),
            is_draft: Set(false),
            git_work_branch: Set(String::new()),
            workpad: Set(String::new()),
            created_at: Set(now_tz),
            updated_at: Set(now_tz),
            ..Default::default()
        }
        .insert(&db)
        .await
        .expect("seed issue");
        let target_id = Uuid::new_v4();
        i_entity::ActiveModel {
            id: Set(target_id),
            workspace_id: Set(workspace_id),
            project_id: Set(project_id),
            state_id: Set(Some(state_backlog)),
            name: Set("Phase0 target".to_owned()),
            description_json: Set(serde_json::json!({})),
            description_html: Set("<p></p>".to_owned()),
            priority: Set("none".to_owned()),
            complexity_score: Set(0),
            sequence_id: Set(2),
            sort_order: Set(65535.0),
            is_draft: Set(false),
            git_work_branch: Set(String::new()),
            workpad: Set(String::new()),
            created_at: Set(now_tz),
            updated_at: Set(now_tz),
            ..Default::default()
        }
        .insert(&db)
        .await
        .expect("seed target issue");
        let pod_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO pod (id, name, description, is_default, created_at, updated_at, \
             workspace_id, project_id) VALUES ($1, $2, '', false, $3, $3, $4, $5)",
        )
        .bind(pod_id)
        .bind(format!("pod-{tag}"))
        .bind(now)
        .bind(workspace_id)
        .bind(project_id)
        .execute(pool)
        .await
        .expect("seed pod");
        let scheduler_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO schedulers (id, slug, name, description, prompt, source, \
             is_enabled, workspace_id, color, created_at, updated_at) VALUES ($1, $2, \
             'Phase0 sched', '', '', 'manual', true, $3, '#000000', $4, $4)",
        )
        .bind(scheduler_id)
        .bind(format!("sched-{tag}"))
        .bind(workspace_id)
        .bind(now)
        .execute(pool)
        .await
        .expect("seed scheduler");
        let binding_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO scheduler_bindings (id, created_at, updated_at, extra_context, \
             enabled, last_error, scheduler_id, dtstart, tzid, rrule, rdates, exdates, \
             outcome_mode, workspace_id, project_id, pod_id) VALUES ($1, $2, $2, '', true, \
             '', $3, $2, 'UTC', 'FREQ=DAILY', '[]', '[]', 'create_issue', $4, $5, $6)",
        )
        .bind(binding_id)
        .bind(now)
        .bind(scheduler_id)
        .bind(workspace_id)
        .bind(project_id)
        .bind(pod_id)
        .execute(pool)
        .await
        .expect("seed binding");
        sqlx::query(
            "INSERT INTO workspace_members (id, created_at, updated_at, role, member_id, \
             workspace_id, view_props, default_props, issue_props, is_active, \
             explored_features, getting_started_checklist, tips) VALUES ($1, $2, $2, 20, $3, \
             $4, '{}', '{}', '{}', true, '{}', '{}', '{}')",
        )
        .bind(Uuid::new_v4())
        .bind(now)
        .bind(user_id)
        .bind(workspace_id)
        .execute(pool)
        .await
        .expect("seed workspace member");
        sqlx::query(
            "INSERT INTO project_members (id, created_at, updated_at, role, member_id, \
             project_id, workspace_id, view_props, default_props, sort_order, preferences, \
             is_active) VALUES ($1, $2, $2, 15, $3, $4, $5, '{}', '{}', 0, '{}', true)",
        )
        .bind(Uuid::new_v4())
        .bind(now)
        .bind(user_id)
        .bind(project_id)
        .bind(workspace_id)
        .execute(pool)
        .await
        .expect("seed project member");
        Phase0Seed {
            user_id,
            workspace_id,
            project_id,
            project_identifier,
            state_backlog,
            state_second,
            issue_id,
            target_id,
            pod_id,
            scheduler_id,
            binding_id,
        }
    }

    /// Validate `args` against the tool's golden schema (the loop
    /// path: `invoke` validation → `dispatch_fn`) and dispatch.
    async fn dispatch_tool(
        ctx: &ToolCtx,
        run_id: Uuid,
        tool: &'static str,
        args: Value,
    ) -> Result<Value, ToolExecError> {
        let golden =
            crate::oss_worker::model_runtime::schemas::Golden::load().expect("golden loads");
        let schema = golden.function_schema(tool).expect("function schema");
        let validated =
            crate::oss_worker::model_runtime::validation::validate_args_value(schema, &args)
                .map_err(|details| {
                    ToolExecError::Failure(plan::ToolCallFailure::Failed(format!(
                        "validation failed: {details:?}"
                    )))
                })?;
        dispatch_fn(ctx, run_id, tool, &validated).await
    }

    /// Ledger rows for one run as `(tool_name, risk, status)`.
    async fn ledger_for_run(pool: &sqlx::PgPool, run_id: Uuid) -> Vec<(String, String, String)> {
        let rows = sqlx::query(
            "SELECT tool_name, risk, status FROM agent_run_tool_call WHERE agent_run_id = $1",
        )
        .bind(run_id)
        .fetch_all(pool)
        .await
        .expect("read ledger");
        rows.into_iter()
            .map(|row| {
                (
                    row.try_get::<String, _>("tool_name").expect("tool"),
                    row.try_get::<String, _>("risk").expect("risk"),
                    row.try_get::<String, _>("status").expect("status"),
                )
            })
            .collect()
    }

    /// Queue rows mentioning `needle` (an issue or relation uuid) in
    /// args or kwargs, as `(task, kwargs_type)` where the activity
    /// `type` kwarg (if any) rides along for assertion.
    async fn jobs_for_needle(pool: &sqlx::PgPool, needle: Uuid) -> Vec<(String, Option<String>)> {
        let like = format!("%{}%", needle);
        let rows = sqlx::query(
            "SELECT task, kwargs ->> 'type' AS activity_type FROM rust_job_queue \
             WHERE args::text LIKE $1 OR kwargs::text LIKE $1",
        )
        .bind(&like)
        .fetch_all(pool)
        .await
        .expect("read queue");
        rows.into_iter()
            .map(|row| {
                (
                    row.try_get::<String, _>("task").expect("task"),
                    row.try_get::<Option<String>, _>("activity_type")
                        .unwrap_or(None),
                )
            })
            .collect()
    }

    /// Order-safe teardown for one phase-0 world, including rows the
    /// orchestration drive may leave behind (ticker, queued runs).
    async fn cleanup_phase0(pool: &sqlx::PgPool, seed: &Phase0Seed, extra_needles: &[Uuid]) {
        let mut needles: Vec<Uuid> = vec![seed.issue_id, seed.target_id];
        needles.extend_from_slice(extra_needles);
        for needle in &needles {
            let like = format!("%{}%", needle);
            sqlx::query(
                "DELETE FROM rust_job_queue WHERE args::text LIKE $1 OR kwargs::text LIKE $1",
            )
            .bind(&like)
            .bind(&like)
            .execute(pool)
            .await
            .expect("teardown queue");
        }
        sqlx::query("DELETE FROM agent_run_tool_call WHERE agent_run_id IN (SELECT id FROM agent_run WHERE workspace_id = $1)")
            .bind(seed.workspace_id)
            .execute(pool)
            .await
            .expect("teardown ledger");
        sqlx::query("DELETE FROM agent_run_event WHERE agent_run_id IN (SELECT id FROM agent_run WHERE workspace_id = $1)")
            .bind(seed.workspace_id)
            .execute(pool)
            .await
            .expect("teardown events");
        sqlx::query("DELETE FROM issue_agent_ticker WHERE issue_id IN (SELECT id FROM issues WHERE workspace_id = $1)")
            .bind(seed.workspace_id)
            .execute(pool)
            .await
            .expect("teardown ticker");
        sqlx::query("DELETE FROM agent_run WHERE workspace_id = $1")
            .bind(seed.workspace_id)
            .execute(pool)
            .await
            .expect("teardown runs");
        let desc_ids: Vec<Option<Uuid>> = sqlx::query_scalar(
            "SELECT description_id FROM issue_comments WHERE issue_id IN (SELECT id FROM issues WHERE workspace_id = $1) AND description_id IS NOT NULL",
        )
        .bind(seed.workspace_id)
        .fetch_all(pool)
        .await
        .expect("list descriptions");
        sqlx::query("DELETE FROM issue_comments WHERE issue_id IN (SELECT id FROM issues WHERE workspace_id = $1)")
            .bind(seed.workspace_id)
            .execute(pool)
            .await
            .expect("teardown comments");
        for desc_id in desc_ids.into_iter().flatten() {
            sqlx::query("DELETE FROM descriptions WHERE id = $1")
                .bind(desc_id)
                .execute(pool)
                .await
                .expect("teardown descriptions");
        }
        sqlx::query("DELETE FROM issue_relations WHERE workspace_id = $1")
            .bind(seed.workspace_id)
            .execute(pool)
            .await
            .expect("teardown relations");
        sqlx::query("DELETE FROM issue_sequences WHERE issue_id IN (SELECT id FROM issues WHERE workspace_id = $1)")
            .bind(seed.workspace_id)
            .execute(pool)
            .await
            .expect("teardown sequences");
        sqlx::query("DELETE FROM issues WHERE workspace_id = $1")
            .bind(seed.workspace_id)
            .execute(pool)
            .await
            .expect("teardown issues");
        sqlx::query("DELETE FROM states WHERE project_id = $1")
            .bind(seed.project_id)
            .execute(pool)
            .await
            .expect("teardown states");
        sqlx::query("DELETE FROM scheduler_bindings WHERE id = $1")
            .bind(seed.binding_id)
            .execute(pool)
            .await
            .expect("teardown binding");
        sqlx::query("DELETE FROM schedulers WHERE id = $1")
            .bind(seed.scheduler_id)
            .execute(pool)
            .await
            .expect("teardown scheduler");
        sqlx::query("DELETE FROM pod WHERE id = $1")
            .bind(seed.pod_id)
            .execute(pool)
            .await
            .expect("teardown pod");
        sqlx::query("DELETE FROM project_members WHERE project_id = $1")
            .bind(seed.project_id)
            .execute(pool)
            .await
            .expect("teardown project members");
        sqlx::query("DELETE FROM workspace_members WHERE workspace_id = $1")
            .bind(seed.workspace_id)
            .execute(pool)
            .await
            .expect("teardown workspace members");
        sqlx::query("DELETE FROM projects WHERE id = $1")
            .bind(seed.project_id)
            .execute(pool)
            .await
            .expect("teardown project");
        sqlx::query("DELETE FROM workspaces WHERE id = $1")
            .bind(seed.workspace_id)
            .execute(pool)
            .await
            .expect("teardown workspace");
        sqlx::query("DELETE FROM user_notification_preferences WHERE user_id = $1")
            .bind(seed.user_id)
            .execute(pool)
            .await
            .expect("teardown notif prefs");
        sqlx::query("DELETE FROM users WHERE id = $1")
            .bind(seed.user_id)
            .execute(pool)
            .await
            .expect("teardown user");
    }

    /// Phase 0: the comment write pins attribution, html, ledger
    /// success and zero queued jobs.
    #[tokio::test]
    async fn phase0_comment_write_pins_rows_result_ledger_and_no_jobs() {
        let Some(pool) = live_pool().await else {
            return;
        };
        phase0_ensure_queue(&pool).await;
        let seed = seed_phase0(&pool).await;
        let run_id = phase0_run(&pool, &seed, Some(seed.issue_id)).await;
        let ctx = phase0_ctx(&pool);
        let result = dispatch_tool(
            &ctx,
            run_id,
            TOOL_ADD_CURRENT_ISSUE_COMMENT,
            serde_json::json!({"body": "phase0 hello"}),
        )
        .await
        .expect("add comment");
        assert_eq!(result["created"], true);
        let comment_id: Uuid = result["comment_id"]
            .as_str()
            .expect("comment id")
            .parse()
            .expect("comment id parses");
        assert_eq!(
            result.as_object().expect("object").len(),
            2,
            "result keys: {result}"
        );
        let row = sqlx::query(
            "SELECT comment_html, comment_stripped, access, speaker_type, speaker_label, \
             speaker_agent_run_id, actor_id, created_by_id, project_id, issue_id, \
             description_id FROM issue_comments WHERE id = $1",
        )
        .bind(comment_id)
        .fetch_one(&pool)
        .await
        .expect("read comment");
        assert_eq!(
            row.try_get::<String, _>("comment_html").expect("html"),
            "<p>phase0 hello</p>"
        );
        assert_eq!(
            row.try_get::<String, _>("comment_stripped")
                .expect("stripped"),
            "phase0 hello"
        );
        assert_eq!(
            row.try_get::<String, _>("access").expect("access"),
            "INTERNAL"
        );
        assert_eq!(
            row.try_get::<String, _>("speaker_type").expect("speaker"),
            "agent"
        );
        assert_eq!(
            row.try_get::<String, _>("speaker_label").expect("label"),
            "Pi Dash Cloud Agent"
        );
        assert_eq!(
            row.try_get::<Uuid, _>("speaker_agent_run_id").expect("run"),
            run_id
        );
        assert_eq!(
            row.try_get::<Uuid, _>("actor_id").expect("actor"),
            seed.user_id
        );
        assert_eq!(
            row.try_get::<Uuid, _>("created_by_id").expect("created_by"),
            seed.user_id
        );
        let desc_id = row
            .try_get::<Option<Uuid>, _>("description_id")
            .expect("desc id")
            .expect("desc linked");
        let desc = sqlx::query("SELECT description_stripped FROM descriptions WHERE id = $1")
            .bind(desc_id)
            .fetch_one(&pool)
            .await
            .expect("read description");
        assert_eq!(
            desc.try_get::<Option<String>, _>("description_stripped")
                .expect("desc stripped"),
            Some("phase0 hello".to_owned())
        );
        assert_eq!(
            ledger_for_run(&pool, run_id).await,
            vec![(
                TOOL_ADD_CURRENT_ISSUE_COMMENT.to_owned(),
                "write".to_owned(),
                "succeeded".to_owned(),
            )]
        );
        assert!(
            jobs_for_needle(&pool, seed.issue_id).await.is_empty(),
            "comment enqueues no jobs"
        );
        cleanup_phase0(&pool, &seed, &[]).await;
    }

    /// Phase 0: the workpad write is a bare two-column update
    /// (`updated_by` stays NULL) with no orchestrated effects.
    #[tokio::test]
    async fn phase0_workpad_write_pins_bare_columns_and_no_jobs() {
        let Some(pool) = live_pool().await else {
            return;
        };
        phase0_ensure_queue(&pool).await;
        let seed = seed_phase0(&pool).await;
        let run_id = phase0_run(&pool, &seed, Some(seed.issue_id)).await;
        let ctx = phase0_ctx(&pool);
        let result = dispatch_tool(
            &ctx,
            run_id,
            TOOL_UPDATE_CURRENT_ISSUE_WORKPAD,
            serde_json::json!({"body": "phase0 workpad"}),
        )
        .await
        .expect("update workpad");
        assert_eq!(
            result,
            serde_json::json!({"updated": true, "issue_id": seed.issue_id.to_string()})
        );
        let row = sqlx::query("SELECT workpad, updated_by_id FROM issues WHERE id = $1")
            .bind(seed.issue_id)
            .fetch_one(&pool)
            .await
            .expect("read issue");
        assert_eq!(
            row.try_get::<String, _>("workpad").expect("workpad"),
            "phase0 workpad"
        );
        assert_eq!(
            row.try_get::<Option<Uuid>, _>("updated_by_id")
                .expect("updated_by"),
            None,
            "bare update stamps no updated_by"
        );
        assert_eq!(
            ledger_for_run(&pool, run_id).await,
            vec![(
                TOOL_UPDATE_CURRENT_ISSUE_WORKPAD.to_owned(),
                "write".to_owned(),
                "succeeded".to_owned(),
            )]
        );
        assert!(
            jobs_for_needle(&pool, seed.issue_id).await.is_empty(),
            "workpad enqueues no jobs"
        );
        cleanup_phase0(&pool, &seed, &[]).await;
    }

    /// Phase 0: a run with no bound issue still answers workpad
    /// success with Python's `str(None)`.
    #[tokio::test]
    async fn phase0_workpad_without_bound_issue_still_succeeds() {
        let Some(pool) = live_pool().await else {
            return;
        };
        phase0_ensure_queue(&pool).await;
        let seed = seed_phase0(&pool).await;
        let run_id = phase0_run(&pool, &seed, None).await;
        let ctx = phase0_ctx(&pool);
        let result = dispatch_tool(
            &ctx,
            run_id,
            TOOL_UPDATE_CURRENT_ISSUE_WORKPAD,
            serde_json::json!({"body": "phase0 workpad"}),
        )
        .await
        .expect("update workpad without bound issue");
        assert_eq!(
            result,
            serde_json::json!({"updated": true, "issue_id": "None"})
        );
        assert_eq!(
            ledger_for_run(&pool, run_id).await,
            vec![(
                TOOL_UPDATE_CURRENT_ISSUE_WORKPAD.to_owned(),
                "write".to_owned(),
                "succeeded".to_owned(),
            )]
        );
        cleanup_phase0(&pool, &seed, &[]).await;
    }

    /// Phase 0: the transition write moves the bare three columns.
    #[tokio::test]
    async fn phase0_transition_write_pins_move_and_same_state_noop() {
        let Some(pool) = live_pool().await else {
            return;
        };
        phase0_ensure_queue(&pool).await;
        let seed = seed_phase0(&pool).await;
        let ctx = phase0_ctx(&pool);
        let run_move = phase0_run(&pool, &seed, Some(seed.issue_id)).await;
        let result = dispatch_tool(
            &ctx,
            run_move,
            TOOL_TRANSITION_CURRENT_ISSUE,
            serde_json::json!({"state_id": seed.state_second.to_string()}),
        )
        .await
        .expect("transition");
        assert_eq!(
            result,
            serde_json::json!({
                "updated": true,
                "state": "Todo",
                "state_id": seed.state_second.to_string(),
            })
        );
        let state_id = sqlx::query("SELECT state_id FROM issues WHERE id = $1")
            .bind(seed.issue_id)
            .fetch_one(&pool)
            .await
            .expect("read issue state")
            .try_get::<Option<Uuid>, _>("state_id")
            .expect("state id");
        assert_eq!(state_id, Some(seed.state_second));
        assert_eq!(
            ledger_for_run(&pool, run_move).await,
            vec![(
                TOOL_TRANSITION_CURRENT_ISSUE.to_owned(),
                "write".to_owned(),
                "succeeded".to_owned(),
            )]
        );
        assert!(
            jobs_for_needle(&pool, seed.issue_id).await.is_empty(),
            "transition enqueues no jobs"
        );
        cleanup_phase0(&pool, &seed, &[]).await;
    }

    /// Phase 0: a same-state transition is a successful no-op
    /// (`updated: false`), the state row untouched, no jobs.
    #[tokio::test]
    async fn phase0_transition_write_pins_same_state_noop() {
        let Some(pool) = live_pool().await else {
            return;
        };
        phase0_ensure_queue(&pool).await;
        let seed = seed_phase0(&pool).await;
        // Park the bound issue in the second state up front so the
        // tool call is a same-state no-op on its own run.
        sqlx::query("UPDATE issues SET state_id = $1 WHERE id = $2")
            .bind(seed.state_second)
            .bind(seed.issue_id)
            .execute(&pool)
            .await
            .expect("park issue state");
        let run_id = phase0_run(&pool, &seed, Some(seed.issue_id)).await;
        let ctx = phase0_ctx(&pool);
        let result = dispatch_tool(
            &ctx,
            run_id,
            TOOL_TRANSITION_CURRENT_ISSUE,
            serde_json::json!({"state_id": seed.state_second.to_string()}),
        )
        .await
        .expect("same-state transition");
        assert_eq!(
            result,
            serde_json::json!({
                "updated": false,
                "state": "Todo",
                "state_id": seed.state_second.to_string(),
            })
        );
        let state_id = sqlx::query("SELECT state_id FROM issues WHERE id = $1")
            .bind(seed.issue_id)
            .fetch_one(&pool)
            .await
            .expect("read issue state")
            .try_get::<Option<Uuid>, _>("state_id")
            .expect("state id");
        assert_eq!(state_id, Some(seed.state_second), "no-op leaves state");
        assert_eq!(
            ledger_for_run(&pool, run_id).await,
            vec![(
                TOOL_TRANSITION_CURRENT_ISSUE.to_owned(),
                "write".to_owned(),
                "succeeded".to_owned(),
            )]
        );
        assert!(
            jobs_for_needle(&pool, seed.issue_id).await.is_empty(),
            "same-state transition enqueues no jobs"
        );
        cleanup_phase0(&pool, &seed, &[]).await;
    }

    /// Phase 0: the create write persists the issue + sequence row,
    /// marks the ledger succeeded, and returns the brief. Fixed by
    /// NEWPIDASH-191: the discarded sequence probe now decodes INT4
    /// as i32 (`tools_exec.rs` `_max_seq`, `into_tuple::<i32>()`
    /// over `issues.sequence_id`, Django `IntegerField`).
    #[tokio::test]
    async fn phase0_create_write_pins_current_failure_and_atomicity() {
        let Some(pool) = live_pool().await else {
            return;
        };
        phase0_ensure_queue(&pool).await;
        let seed = seed_phase0(&pool).await;
        let run_id = phase0_run(&pool, &seed, Some(seed.issue_id)).await;
        let ctx = phase0_ctx(&pool);
        let result = dispatch_tool(
            &ctx,
            run_id,
            TOOL_CREATE_PROJECT_ISSUE,
            serde_json::json!({"title": "Phase0 new issue", "description": "body text"}),
        )
        .await
        .expect("create issue");
        assert_eq!(result["created"], serde_json::json!(true));
        assert_eq!(result["name"], serde_json::json!("Phase0 new issue"));
        assert_eq!(result["state"], serde_json::json!("Backlog"));
        assert_eq!(result["state_group"], serde_json::json!("backlog"));
        assert_eq!(result["priority"], serde_json::json!("none"));
        let new_id: Uuid = result["id"]
            .as_str()
            .expect("id renders")
            .parse()
            .expect("id parses");
        assert_eq!(
            result["identifier"],
            serde_json::json!(format!("{}-3", seed.project_identifier))
        );
        assert_eq!(
            ledger_for_run(&pool, run_id).await,
            vec![(
                TOOL_CREATE_PROJECT_ISSUE.to_owned(),
                "write".to_owned(),
                "succeeded".to_owned(),
            )]
        );
        let issue_count =
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM issues WHERE project_id = $1")
                .bind(seed.project_id)
                .fetch_one(&pool)
                .await
                .expect("count issues");
        assert_eq!(issue_count, 3, "the created issue stands");
        let row = sqlx::query("SELECT name, sequence_id, state_id FROM issues WHERE id = $1")
            .bind(new_id)
            .fetch_one(&pool)
            .await
            .expect("read created issue");
        assert_eq!(
            row.try_get::<String, _>("name").expect("name"),
            "Phase0 new issue"
        );
        assert_eq!(row.try_get::<i32, _>("sequence_id").expect("seq"), 3);
        assert_eq!(
            row.try_get::<Option<Uuid>, _>("state_id").expect("state"),
            Some(seed.state_backlog)
        );
        let seq: i64 =
            sqlx::query_scalar("SELECT sequence FROM issue_sequences WHERE issue_id = $1")
                .bind(new_id)
                .fetch_one(&pool)
                .await
                .expect("read sequence row");
        assert_eq!(seq, 3, "issue_sequences row matches");
        assert!(
            jobs_for_needle(&pool, new_id).await.is_empty(),
            "create enqueues no jobs for the new issue without LLM config"
        );
        cleanup_phase0(&pool, &seed, &[new_id]).await;
    }

    /// Phase 0: the relate write pins the relation row, verdict
    /// arrays, grouped relations and exactly one activity job.
    #[tokio::test]
    async fn phase0_relate_write_pins_row_activity_job_and_grouped() {
        let Some(pool) = live_pool().await else {
            return;
        };
        phase0_ensure_queue(&pool).await;
        let seed = seed_phase0(&pool).await;
        let run_id = phase0_run(&pool, &seed, Some(seed.issue_id)).await;
        let ctx = phase0_ctx(&pool);
        let result = dispatch_tool(
            &ctx,
            run_id,
            TOOL_RELATE_ISSUES,
            serde_json::json!({
                "issue": seed.issue_id.to_string(),
                "relation_type": "relates_to",
                "related_issues": [seed.target_id.to_string()],
            }),
        )
        .await
        .expect("relate");
        let source_ident = format!("{}-1", seed.project_identifier);
        let target_ident = format!("{}-2", seed.project_identifier);
        assert_eq!(result["issue"], source_ident);
        assert_eq!(result["relation_type"], "relates_to");
        assert_eq!(result["created"], serde_json::json!([target_ident]));
        assert_eq!(result["unchanged"], serde_json::json!([]));
        assert_eq!(result["conflicts"], serde_json::json!([]));
        assert_eq!(
            result["relations"]["relates_to"],
            serde_json::json!([{
                "id": seed.target_id.to_string(),
                "identifier": target_ident,
                "name": "Phase0 target",
                "state": "Backlog",
                "state_group": "backlog",
            }]),
            "grouped relations: {result}"
        );
        let row = sqlx::query(
            "SELECT related_issue_id, relation_type FROM issue_relations \
             WHERE issue_id = $1 AND deleted_at IS NULL",
        )
        .bind(seed.issue_id)
        .fetch_one(&pool)
        .await
        .expect("read relation");
        assert_eq!(
            row.try_get::<Uuid, _>("related_issue_id").expect("related"),
            seed.target_id
        );
        assert_eq!(
            row.try_get::<String, _>("relation_type").expect("type"),
            "relates_to"
        );
        assert_eq!(
            jobs_for_needle(&pool, seed.issue_id).await,
            vec![(
                ACTIVITY_TASK.to_owned(),
                Some(RELATE_CREATED_ACTIVITY.to_owned())
            )]
        );
        assert_eq!(
            ledger_for_run(&pool, run_id).await,
            vec![(
                TOOL_RELATE_ISSUES.to_owned(),
                "write".to_owned(),
                "succeeded".to_owned(),
            )]
        );
        cleanup_phase0(&pool, &seed, &[]).await;
    }

    /// Phase 0: the unrelate write soft-deletes the row and enqueues
    /// one activity job plus one soft-delete sweep job per row.
    #[tokio::test]
    async fn phase0_unrelate_write_pins_soft_delete_activity_and_sweep_jobs() {
        let Some(pool) = live_pool().await else {
            return;
        };
        phase0_ensure_queue(&pool).await;
        let seed = seed_phase0(&pool).await;
        let now = chrono::Utc::now();
        let relation_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO issue_relations (id, created_at, updated_at, relation_type, \
             created_by_id, issue_id, project_id, related_issue_id, workspace_id) \
             VALUES ($1, $2, $2, 'relates_to', $3, $4, $5, $6, $7)",
        )
        .bind(relation_id)
        .bind(now)
        .bind(seed.user_id)
        .bind(seed.issue_id)
        .bind(seed.project_id)
        .bind(seed.target_id)
        .bind(seed.workspace_id)
        .execute(&pool)
        .await
        .expect("seed relation");
        let run_id = phase0_run(&pool, &seed, Some(seed.issue_id)).await;
        let ctx = phase0_ctx(&pool);
        let result = dispatch_tool(
            &ctx,
            run_id,
            TOOL_UNRELATE_ISSUES,
            serde_json::json!({
                "issue": seed.issue_id.to_string(),
                "relation_type": "relates_to",
                "related_issues": [seed.target_id.to_string()],
            }),
        )
        .await
        .expect("unrelate");
        let target_ident = format!("{}-2", seed.project_identifier);
        assert_eq!(result["removed"], serde_json::json!([target_ident]));
        assert_eq!(result["not_related"], serde_json::json!([]));
        assert_eq!(
            result["relations"]["relates_to"],
            serde_json::json!([]),
            "grouped relations empty after unrelate: {result}"
        );
        let row = sqlx::query("SELECT deleted_at FROM issue_relations WHERE id = $1")
            .bind(relation_id)
            .fetch_one(&pool)
            .await
            .expect("read relation");
        assert!(
            row.try_get::<Option<chrono::DateTime<chrono::Utc>>, _>("deleted_at")
                .expect("deleted_at")
                .is_some(),
            "row is soft-deleted"
        );
        assert_eq!(
            jobs_for_needle(&pool, seed.issue_id).await,
            vec![(
                ACTIVITY_TASK.to_owned(),
                Some(RELATE_DELETED_ACTIVITY.to_owned())
            )]
        );
        assert_eq!(
            jobs_for_needle(&pool, relation_id).await,
            vec![(SWEEP_TASK.to_owned(), None)]
        );
        assert_eq!(
            ledger_for_run(&pool, run_id).await,
            vec![(
                TOOL_UNRELATE_ISSUES.to_owned(),
                "write".to_owned(),
                "succeeded".to_owned(),
            )]
        );
        cleanup_phase0(&pool, &seed, &[relation_id]).await;
    }

    /// Phase 0: an op failure after the audit opens rolls the
    /// transaction back — no ledger row, no data — and a pre-audit
    /// validation failure leaves no ledger row either.
    #[tokio::test]
    async fn phase0_op_failure_rolls_back_write_and_ledger() {
        let Some(pool) = live_pool().await else {
            return;
        };
        phase0_ensure_queue(&pool).await;
        let seed = seed_phase0(&pool).await;
        let ctx = phase0_ctx(&pool);
        // Unknown-state transition: valid UUID outside the project →
        // op error after the ledger insert.
        let run_id = phase0_run(&pool, &seed, Some(seed.issue_id)).await;
        let outcome = dispatch_tool(
            &ctx,
            run_id,
            TOOL_TRANSITION_CURRENT_ISSUE,
            serde_json::json!({"state_id": Uuid::new_v4().to_string()}),
        )
        .await;
        assert!(outcome.is_err(), "unknown state fails: {outcome:?}");
        assert_eq!(
            ledger_for_run(&pool, run_id).await,
            Vec::<(String, String, String)>::new(),
            "op failure leaves no ledger row"
        );
        let state_id = sqlx::query("SELECT state_id FROM issues WHERE id = $1")
            .bind(seed.issue_id)
            .fetch_one(&pool)
            .await
            .expect("read issue state")
            .try_get::<Option<Uuid>, _>("state_id")
            .expect("state id");
        assert_eq!(state_id, Some(seed.state_backlog), "no write survives");
        // Over-cap comment body: the pre-audit validator fails before
        // the transaction (and the ledger) ever opens. Failures leave
        // no ledger row, so reusing the run for a second failing
        // call stays under the write budget with no single-use cost.
        let body = "x".repeat(10_001);
        let outcome = dispatch_tool(
            &ctx,
            run_id,
            TOOL_ADD_CURRENT_ISSUE_COMMENT,
            serde_json::json!({"body": body}),
        )
        .await;
        assert!(outcome.is_err(), "over-cap body fails: {outcome:?}");
        assert_eq!(
            ledger_for_run(&pool, run_id).await,
            Vec::<(String, String, String)>::new(),
            "validation failure leaves no ledger row"
        );
        let comment_count =
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM issue_comments WHERE issue_id = $1")
                .bind(seed.issue_id)
                .fetch_one(&pool)
                .await
                .expect("count comments");
        assert_eq!(comment_count, 0, "no comment written");
        cleanup_phase0(&pool, &seed, &[]).await;
    }
}
