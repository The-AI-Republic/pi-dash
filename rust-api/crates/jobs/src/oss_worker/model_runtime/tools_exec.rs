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
use sqlx::postgres::PgRow;
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::admission::RedisAdmissionCache;
use crate::dispatch::{append_event, dispatch_agent_run, DispatchDecision};
use crate::orchestration::{
    fire_tick_seam::{
        create_ticker_sql, emit_logs, lock_ticker_by_issue_sql, map_ticker, save_clock_sql,
    },
    live_inputs::JobsMatcher,
    preflight_eligibility_or_bounce, LiveCreationStore, StoreDeps,
};
use crate::queue::{enqueue, enqueue_in, NewJob};
use crate::runner_runs::sweeps_chat::terminal_effects_job;
use crate::tasks_cleanup::deletion::SOFT_DELETE_TASK;
use pidash_db::app_pages::strip::ml_strip_tags;
use pidash_db::config::{CloudAgentSettings, ManagedRunnerSettings, RunnerSettings};
use pidash_db::orchestration::querysets::MEMBER_PROJECT_ISSUES_SQL;
use pidash_db::tasks_ticker::models::issue_agent_ticker::IssueAgentTicker;
use pidash_db::tx::Transaction;
use pidash_services::assistant::markdown::to_safe_html;
use pidash_services::assistant::seams::agent_model_profile_for_user;
use pidash_services::assistant::tools_issues::{escape_icontains, PROJECT_WRITE_GATE_SQL};
use pidash_services::dispatch::policy::UserFlags;
use pidash_services::dispatch::tools as plan;
use pidash_services::dispatch::{
    admission::AdmissionCache, consume_admission_token, DeferredConsume, LlmProfile,
};
use pidash_services::domain::identity::AgentUserCollisionError;
use pidash_services::extensions::CloudAgentToolsetsSeam;
use pidash_services::orchestration::clock::{ClockWrite, ProjectClockPolicy};
use pidash_services::orchestration::creation::{
    AdmissionError, CreationError, CreationSeam, ExecutionError, ExecutionFields, ExecutionRequest,
    FinalizeAgentRunSeam, IssueView, LockedIssue, NewAgentRun, PodView, ProjectView, RenderBundle,
    RenderedTurn, RunView, RunnerView, StateView, STATE_SELECT_SQL,
};
use pidash_services::orchestration::entries::{
    fire_state_transition, handle_issue_state_transition, BindingView, EntriesSeam, FireOutcome,
    FireRequest, NewSchedulerRun, PreflightSeam, ProjectRoleFacts, SchedulerView,
    TransitionRequest, WorkspaceView, CLOCK_POLICY_SQL, PRIOR_STATE_SELECT_SQL,
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
use pidash_compat::db::is_unique_violation;

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

/// `_scope` (`tools.py:46-73`) over one connection: the caller's
/// connection (the audit transaction on the write path, a fresh pool
/// connection on the read path). A missing run row is
/// `AgentRun.DoesNotExist` (`.get()`, `query.py:639-640`); a dangling
/// work-item/scheduler-binding back-pointer reads its project as the
/// `AttributeError` Python's `run.work_item.project` raises.
async fn load_scope(
    conn: &mut sqlx::PgConnection,
    run_id: Uuid,
    write: bool,
) -> Result<ToolScope, ToolExecError> {
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(plan::SCOPE_RUN_SQL)
        .bind(run_id)
        .fetch_optional(&mut *conn)
        .await?;
    let Some(row) = row else {
        return Err(ToolExecError::Missing { model: "AgentRun" });
    };
    let workspace_id: Uuid = row.try_get("workspace_id")?;
    let creator_id: Uuid = row.try_get("created_by_id")?;
    let work_item_id: Option<Uuid> = row.try_get("work_item_id")?;
    let scheduler_binding_id: Option<Uuid> = row.try_get("scheduler_binding_id")?;
    let cancel_requested_at: Option<DateTime<Utc>> = row.try_get("cancel_requested_at")?;
    let creator_active: bool = row.try_get("is_active")?;
    let creator_is_bot: bool = row.try_get("is_bot")?;
    let workspace_slug: String = row.try_get("slug")?;
    // `SCOPE_RUN_SQL` projects three same-named `project_id` columns
    // (one per source); positional reads disambiguate them in select
    // order (work item 10, scheduler binding 11, pod 12).
    let work_item_project_id: Option<Uuid> = row.try_get(10)?;
    let scheduler_project_id: Option<Uuid> = row.try_get(11)?;
    let pod_project_id: Option<Uuid> = row.try_get(12)?;

    let workspace_member: bool = sqlx::query(plan::SCOPE_WORKSPACE_MEMBER_SQL)
        .bind(creator_id)
        .bind(workspace_id)
        .fetch_optional(&mut *conn)
        .await?
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
    let gate_sql = if write {
        PROJECT_WRITE_GATE_SQL
    } else {
        plan::PROJECT_READ_GATE_SQL
    };
    let role_ok: bool = sqlx::query_scalar(gate_sql)
        .bind(creator_id)
        .bind(workspace_slug.as_str())
        .bind(project_id)
        .fetch_one(&mut *conn)
        .await?;
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

/// An opened write audit: the transaction, the scope, and the prepared
/// ledger row. Borrows the pool through the dispatch's hold on `ctx`;
/// dropping it without commit rolls back (the op-error path relies on
/// this, explicitly rolling back first for a clean error).
pub struct WriteAudit<'a> {
    /// The audit transaction (scope, admission, ledger, op, marking).
    pub tx: Transaction<'a>,
    /// The write-bit scope.
    pub scope: ToolScope,
    /// The prepared ledger row id.
    pub ledger_id: Uuid,
}

/// `_audit` short-circuit: a replay value returns without running the
/// op (`tools.py:93-94`).
pub enum AuditBegin<'a> {
    /// Run the op under this audit.
    Proceed(WriteAudit<'a>),
    /// Return the stored replay value (no ledger write, no event).
    Replay(Value),
}

/// Write-path `_audit` through the submit flip (`tools.py:76-129`):
/// fresh call id, fingerprint, kill-switch guards, replay check, then
/// the transaction over scope, admission, ledger insert, and submit.
/// Guard/denial failures leave no row; the caller rolls the
/// transaction back on op errors (or drops it, which rolls back).
pub async fn write_begin<'a>(
    ctx: &'a ToolCtx,
    run_id: Uuid,
    tool_name: &'static str,
    args: &Value,
    source: &'static str,
    server_key: &'static str,
) -> Result<AuditBegin<'a>, ToolExecError> {
    let call_id = plan::resolve_tool_call_id(None);
    let fingerprint = plan::fingerprint(args);
    plan::audit_guards(
        &ctx.cloud,
        tool_name,
        plan::ToolRisk::Write,
        source,
        server_key,
    )?;
    let existing = find_existing(&ctx.pool, run_id, &call_id).await?;
    match plan::replay_decision(existing.as_ref(), &fingerprint)? {
        plan::ReplayDecision::Replay(value) => return Ok(AuditBegin::Replay(value)),
        plan::ReplayDecision::Proceed => {}
    }
    let mut tx = Transaction::begin(&ctx.pool).await?;
    let scope = match load_scope(tx.inner(), run_id, true).await {
        Ok(scope) => scope,
        Err(error) => {
            let _ = tx.rollback().await;
            return Err(error);
        }
    };
    let succeeded_writes: i64 = sqlx::query_scalar(plan::COUNT_SUCCEEDED_WRITES_SQL)
        .bind(run_id)
        .fetch_one(&mut **tx.inner())
        .await?;
    let tool_used: bool = sqlx::query(plan::WRITE_TOOL_USED_SQL)
        .bind(run_id)
        .bind(tool_name)
        .fetch_optional(&mut **tx.inner())
        .await?
        .is_some();
    if let Err(denied) = plan::write_admission(succeeded_writes, tool_used, tool_name, &ctx.cloud) {
        let _ = tx.rollback().await;
        return Err(denied.into());
    }
    let ledger_id = Uuid::new_v4();
    let idempotency_hash =
        plan::fingerprint(&serde_json::json!({"run": run_id.to_string(), "tool": tool_name}));
    sqlx::query(plan::LEDGER_INSERT_WRITE_SQL)
        .bind(ledger_id)
        .bind(run_id)
        .bind(call_id)
        .bind(source)
        .bind(server_key)
        .bind(tool_name)
        .bind(fingerprint)
        .bind(idempotency_hash)
        .bind(Utc::now())
        .execute(&mut **tx.inner())
        .await?;
    sqlx::query(plan::LEDGER_MARK_SUBMITTED_SQL)
        .bind(ledger_id)
        .bind(Utc::now())
        .execute(&mut **tx.inner())
        .await?;
    Ok(AuditBegin::Proceed(WriteAudit {
        tx,
        scope,
        ledger_id,
    }))
}

/// Write-path `_audit` after the op (`tools.py:123-129,156`): bound
/// the result, collapse the replay past 4 KiB, mark succeeded, commit,
/// then append `tool_completed` outside the transaction. A commit or
/// event failure propagates (the committed writes stand, as in
/// Python); the capped-bool is ignored.
pub async fn write_commit(
    ctx: &ToolCtx,
    audit: WriteAudit<'_>,
    tool_name: &'static str,
    result: Value,
) -> Result<Value, ToolExecError> {
    let run_id = audit.scope.run_id;
    let value = write_commit_tx(ctx, audit, result).await?;
    emit_completed(ctx, run_id, tool_name, plan::ToolRisk::Write).await?;
    Ok(value)
}

/// Ledger success-mark plus commit: the write-path
/// `transaction.atomic()` body (`tools.py:97-129`), without the
/// `tool_completed` append (`tools.py:156`). The create caller commits
/// through this so its post-save fire runs on commit-success — Django's
/// signal fires inside the save, before the append — while the tool
/// result still covers the append exactly as [`write_commit`] does.
async fn write_commit_tx(
    ctx: &ToolCtx,
    audit: WriteAudit<'_>,
    result: Value,
) -> Result<Value, ToolExecError> {
    let WriteAudit {
        mut tx,
        scope: _,
        ledger_id,
    } = audit;
    let bounded = plan::bounded(result.clone(), ctx.cloud.max_tool_result_bytes)?;
    let replay = if plan::collapse_replay(plan::canonical(&bounded).len()) {
        plan::collapsed_replay()
    } else {
        bounded.clone()
    };
    let result_fingerprint = plan::fingerprint(&bounded);
    sqlx::query(plan::LEDGER_MARK_SUCCEEDED_SQL)
        .bind(ledger_id)
        .bind(replay)
        .bind(result_fingerprint)
        .bind(Utc::now())
        .execute(&mut **tx.inner())
        .await?;
    tx.commit().await?;
    Ok(result)
}

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
    let mut conn = ctx.pool.acquire().await?;
    let scope = load_scope(&mut conn, run_id, false).await?;
    let ledger_id = Uuid::new_v4();
    let now = Utc::now();
    sqlx::query(plan::LEDGER_INSERT_READ_SQL)
        .bind(ledger_id)
        .bind(run_id)
        .bind(call_id)
        .bind(source)
        .bind(server_key)
        .bind(tool_name)
        .bind(fingerprint)
        .bind(now)
        .bind(now)
        .execute(&ctx.pool)
        .await?;
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
    match outcome {
        Err(error) => {
            sqlx::query(plan::LEDGER_MARK_FAILED_SQL)
                .bind(ledger_id)
                .bind(plan::failure_error_code(error.python_type()))
                .bind(Utc::now())
                .execute(&ctx.pool)
                .await?;
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
            sqlx::query(plan::LEDGER_MARK_SUCCEEDED_SQL)
                .bind(ledger_id)
                .bind(replay)
                .bind(result_fingerprint)
                .bind(Utc::now())
                .execute(&ctx.pool)
                .await?;
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
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(plan::FIND_EXISTING_CALL_SQL)
        .bind(run_id)
        .bind(call_id)
        .fetch_optional(pool)
        .await?;
    row.map(|row| {
        let status: String = row.try_get("status")?;
        // A junk stored status compares unequal to `succeeded` in
        // Python (→ `already_submitted`); `Unknown` preserves that arm.
        let status = pidash_db::dispatch::ToolCallStatus::from_value(&status)
            .unwrap_or(pidash_db::dispatch::ToolCallStatus::Unknown);
        Ok(plan::ExistingCall {
            request_fingerprint: row.try_get("request_fingerprint")?,
            status,
            safe_replay_result: row.try_get("safe_replay_result")?,
        })
    })
    .transpose()
    .map_err(ToolExecError::Db)
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

/// The bound-issue `select_related("project", "state")` mirror: a plain
/// PK lookup joining the project (inner) and state (outer), with NO
/// soft-delete predicate — FK follows do not apply the related
/// manager's filter, so a soft-deleted bound issue still reads (unlike
/// [`plan::GET_PROJECT_ISSUE_SQL`], which serves the by-id tool's
/// manager-filtered `.get()`). `$1` is the bound issue id.
pub const BOUND_ISSUE_SQL: &str = "SELECT issues.id, issues.workspace_id, issues.project_id, \
     issues.state_id, issues.sequence_id, issues.name, issues.priority, \
     issues.description_stripped, issues.workpad, states.name, states.group, \
     projects.identifier FROM issues \
     INNER JOIN projects ON projects.id = issues.project_id \
     LEFT OUTER JOIN states ON states.id = issues.state_id \
     WHERE issues.id = $1 LIMIT 1";

/// One bound-issue row: the `_issue_data` inputs.
pub struct BoundIssue {
    /// `str(issue.id)`.
    pub id: Uuid,
    /// `issue.workspace_id` (the relations default source reads it
    /// off the issue, not the run).
    pub workspace_id: Uuid,
    /// `issue.project_id`.
    pub project_id: Uuid,
    /// `issue.sequence_id`.
    pub sequence_id: i32,
    /// `issue.name`.
    pub name: String,
    /// `issue.priority`.
    pub priority: String,
    /// `issue.description_stripped` (may be NULL).
    pub description_stripped: Option<String>,
    /// `issue.workpad` (may be NULL).
    pub workpad: Option<String>,
    /// `issue.state.name` (None when stateless).
    pub state_name: Option<String>,
    /// `issue.state.group` (None when stateless).
    pub state_group: Option<String>,
    /// `issue.project.identifier`.
    pub project_identifier: String,
}

impl BoundIssue {
    /// `_issue_data(issue, detail=True)` (`tools.py:160-171`).
    fn detail_value(&self) -> Value {
        let detail = plan::issue_data(
            &self.id.to_string(),
            &self.project_identifier,
            i64::from(self.sequence_id),
            &self.name,
            self.state_name.as_deref(),
            self.state_group.as_deref(),
            &self.priority,
            Some((
                self.description_stripped.as_deref().unwrap_or(""),
                self.workpad.as_deref().unwrap_or(""),
            )),
        );
        serde_json::to_value(detail).expect("issue detail serializes")
    }
}

/// Read the bound issue, if the run has one. A missing row reads as
/// `None` (the `select_related` miss), exactly like a run with no
/// bound issue — callers that dereference it raise the
/// `AttributeError` Python's attribute access raises.
async fn bound_issue(
    pool: &PgPool,
    work_item_id: Option<Uuid>,
) -> Result<Option<BoundIssue>, ToolExecError> {
    let Some(issue_id) = work_item_id else {
        return Ok(None);
    };
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(BOUND_ISSUE_SQL)
        .bind(issue_id)
        .fetch_optional(pool)
        .await?;
    row.map(|row| {
        // `name` collides (issue 5, state 9): the state pair reads
        // positionally, the rest by (unique) name.
        Ok(BoundIssue {
            id: row.try_get("id")?,
            workspace_id: row.try_get("workspace_id")?,
            project_id: row.try_get("project_id")?,
            sequence_id: row.try_get("sequence_id")?,
            name: row.try_get("name")?,
            priority: row.try_get("priority")?,
            description_stripped: row.try_get("description_stripped")?,
            workpad: row.try_get("workpad")?,
            state_name: row.try_get(9)?,
            state_group: row.try_get(10)?,
            project_identifier: row.try_get("identifier")?,
        })
    })
    .transpose()
    .map_err(ToolExecError::Db)
}

// ---------------------------------------------------------------------------
// Read ops
// ---------------------------------------------------------------------------

/// `pidash_get_current_issue` (`tools.py:200-202`).
async fn op_get_current_issue(ctx: &ToolCtx, scope: &ToolScope) -> Result<Value, ToolExecError> {
    let issue = bound_issue(&ctx.pool, scope.work_item_id).await?;
    let Some(issue) = issue else {
        return Err(ToolExecError::AttributeError { attribute: "id" });
    };
    Ok(issue.detail_value())
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
    let rows = sqlx::query(plan::CURRENT_ISSUE_COMMENTS_SQL)
        .bind(work_item_id)
        .fetch_all(&ctx.pool)
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in &rows {
        let id: Uuid = row.try_get("id").map_err(ToolExecError::Db)?;
        let stripped: Option<String> =
            row.try_get("comment_stripped").map_err(ToolExecError::Db)?;
        let created_at: DateTime<Utc> = row.try_get("created_at").map_err(ToolExecError::Db)?;
        out.push(plan::comment_row(
            &id.to_string(),
            stripped.as_deref(),
            &render_isoformat(created_at),
        ));
    }
    serde_json::to_value(out)
        .map_err(|error| ToolExecError::Failure(plan::ToolCallFailure::Failed(error.to_string())))
}

/// `pidash_list_project_states` (`tools.py:215-225`).
async fn op_list_project_states(ctx: &ToolCtx, scope: &ToolScope) -> Result<Value, ToolExecError> {
    let rows = sqlx::query(plan::PROJECT_STATES_SQL)
        .bind(scope.project_id)
        .fetch_all(&ctx.pool)
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in &rows {
        let id: Uuid = row.try_get("id").map_err(ToolExecError::Db)?;
        let name: String = row.try_get("name").map_err(ToolExecError::Db)?;
        let group: String = row.try_get("group").map_err(ToolExecError::Db)?;
        out.push(plan::StateRow {
            id: id.to_string(),
            name,
            group,
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
    let rows = sqlx::query(plan::SEARCH_ISSUES_SQL)
        .bind(scope.project_id)
        .bind(escape_icontains(query))
        .bind(limit)
        .fetch_all(&ctx.pool)
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in &rows {
        let id: Uuid = row.try_get("id").map_err(ToolExecError::Db)?;
        let sequence_id: i32 = row.try_get("sequence_id").map_err(ToolExecError::Db)?;
        let name: String = row.try_get("name").map_err(ToolExecError::Db)?;
        let priority: String = row.try_get("priority").map_err(ToolExecError::Db)?;
        let state_name: Option<String> = row.try_get(6).map_err(ToolExecError::Db)?;
        let state_group: Option<String> = row.try_get(7).map_err(ToolExecError::Db)?;
        let project_identifier: String = row.try_get("identifier").map_err(ToolExecError::Db)?;
        let detail = plan::issue_data(
            &id.to_string(),
            &project_identifier,
            i64::from(sequence_id),
            &name,
            state_name.as_deref(),
            state_group.as_deref(),
            &priority,
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
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(plan::GET_PROJECT_ISSUE_SQL)
        .bind(id)
        .bind(scope.project_id)
        .fetch_optional(&ctx.pool)
        .await?;
    let Some(row) = row else {
        return Err(ToolExecError::Missing { model: "Issue" });
    };
    let found: Uuid = row.try_get("id")?;
    let sequence_id: i32 = row.try_get("sequence_id")?;
    let name: String = row.try_get("name")?;
    let priority: String = row.try_get("priority")?;
    let description_stripped: Option<String> = row.try_get("description_stripped")?;
    let workpad: Option<String> = row.try_get("workpad")?;
    let state_name: Option<String> = row.try_get(8)?;
    let state_group: Option<String> = row.try_get(9)?;
    let project_identifier: String = row.try_get("identifier")?;
    let detail = plan::issue_data(
        &found.to_string(),
        &project_identifier,
        i64::from(sequence_id),
        &name,
        state_name.as_deref(),
        state_group.as_deref(),
        &priority,
        Some((
            description_stripped.as_deref().unwrap_or(""),
            workpad.as_deref().unwrap_or(""),
        )),
    );
    serde_json::to_value(detail)
        .map_err(|error| ToolExecError::Failure(plan::ToolCallFailure::Failed(error.to_string())))
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
    let rows = sqlx::query(plan::LINKED_REVIEWS_SQL)
        .bind(work_item_id)
        .fetch_all(&ctx.pool)
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in &rows {
        let id: Uuid = row.try_get("id").map_err(ToolExecError::Db)?;
        let provider: String = row.try_get("provider").map_err(ToolExecError::Db)?;
        let url: String = row.try_get("url").map_err(ToolExecError::Db)?;
        let title: String = row.try_get("title").map_err(ToolExecError::Db)?;
        let state: String = row.try_get("state").map_err(ToolExecError::Db)?;
        let external_iid: String = row.try_get("external_iid").map_err(ToolExecError::Db)?;
        out.push(plan::LinkedReviewRow {
            id: id.to_string(),
            provider,
            url,
            title,
            state,
            number: external_iid,
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
    tx: &mut Transaction<'_>,
    scope: &ToolScope,
    body: &str,
) -> Result<Value, ToolExecError> {
    let issue = bound_issue_tx(tx, scope.work_item_id).await?;
    let Some(issue) = issue else {
        return Err(ToolExecError::AttributeError {
            attribute: "project",
        });
    };
    let html = to_safe_html(Some(body));
    let stripped = if html.is_empty() {
        String::new()
    } else {
        ml_strip_tags(&html)
    };
    let comment_id = Uuid::new_v4();
    let description_id = Uuid::new_v4();
    let now = Utc::now();
    sqlx::query(plan::COMMENT_INSERT_SQL)
        .bind(comment_id)
        .bind(now)
        .bind(scope.creator_id)
        .bind(scope.workspace_id)
        .bind(issue.project_id)
        .bind(stripped.as_str())
        .bind(html.as_str())
        .bind(issue.id)
        .bind(scope.run_id)
        .execute(&mut **tx.inner())
        .await?;
    sqlx::query(plan::COMMENT_DESCRIPTION_INSERT_SQL)
        .bind(description_id)
        .bind(now)
        .bind(scope.creator_id)
        .bind(scope.workspace_id)
        .bind(issue.project_id)
        .bind(html.as_str())
        .bind(stripped.as_str())
        .execute(&mut **tx.inner())
        .await?;
    sqlx::query(plan::COMMENT_DESCRIPTION_LINK_SQL)
        .bind(comment_id)
        .bind(description_id)
        .execute(&mut **tx.inner())
        .await?;
    let created = plan::comment_created(&comment_id.to_string());
    serde_json::to_value(created)
        .map_err(|error| ToolExecError::Failure(plan::ToolCallFailure::Failed(error.to_string())))
}

/// The bound-issue read over the audit transaction (same shape as
/// [`bound_issue`]).
async fn bound_issue_tx(
    tx: &mut Transaction<'_>,
    work_item_id: Option<Uuid>,
) -> Result<Option<BoundIssue>, ToolExecError> {
    let Some(issue_id) = work_item_id else {
        return Ok(None);
    };
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(BOUND_ISSUE_SQL)
        .bind(issue_id)
        .fetch_optional(&mut **tx.inner())
        .await?;
    row.map(|row| {
        Ok(BoundIssue {
            id: row.try_get("id")?,
            workspace_id: row.try_get("workspace_id")?,
            project_id: row.try_get("project_id")?,
            sequence_id: row.try_get("sequence_id")?,
            name: row.try_get("name")?,
            priority: row.try_get("priority")?,
            description_stripped: row.try_get("description_stripped")?,
            workpad: row.try_get("workpad")?,
            state_name: row.try_get(9)?,
            state_group: row.try_get(10)?,
            project_identifier: row.try_get("identifier")?,
        })
    })
    .transpose()
    .map_err(ToolExecError::Db)
}

/// `pidash_update_current_issue_workpad` (`tools.py:296-307`). A run
/// with no bound issue updates zero rows (`pk=None`) and still reports
/// `updated`, with `issue_id` rendering Python's `str(None)`.
async fn op_update_current_issue_workpad(
    tx: &mut Transaction<'_>,
    scope: &ToolScope,
    body: &str,
) -> Result<Value, ToolExecError> {
    if let Some(work_item_id) = scope.work_item_id {
        // `run.work_item.project_id` (the raw attname, not the join).
        let issue = bound_issue_tx(tx, scope.work_item_id).await?;
        let Some(issue) = issue else {
            return Err(ToolExecError::AttributeError {
                attribute: "project_id",
            });
        };
        sqlx::query(plan::WORKPAD_UPDATE_SQL)
            .bind(work_item_id)
            .bind(issue.project_id)
            .bind(body)
            .bind(Utc::now())
            .execute(&mut **tx.inner())
            .await?;
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

/// One [`STATE_SELECT_SQL`] row: the orchestration state view.
async fn state_view_tx(
    tx: &mut Transaction<'_>,
    state_id: Uuid,
) -> Result<Option<pidash_services::orchestration::creation::StateView>, ToolExecError> {
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(STATE_SELECT_SQL)
        .bind(state_id)
        .fetch_optional(&mut **tx.inner())
        .await?;
    row.map(|row| {
        Ok(pidash_services::orchestration::creation::StateView {
            id: row.try_get("id")?,
            name: row.try_get("name")?,
            group: row.try_get("group")?,
        })
    })
    .transpose()
    .map_err(ToolExecError::Db)
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
/// Returns the brief plus the post-commit fire inputs — the caller
/// drives [`fire_create_transition`] once the audit commits.
async fn op_create_project_issue(
    tx: &mut Transaction<'_>,
    scope: &ToolScope,
    description: &str,
    stripped_title: &str,
) -> Result<(Value, CreateDispatch), ToolExecError> {
    // `run.scheduler_binding.project_id` (the raw attname): the tool
    // uses the binding's project even when the run also has a bound
    // issue in another project.
    let Some(binding_id) = scope.scheduler_binding_id else {
        return Err(ToolExecError::AttributeError {
            attribute: "project_id",
        });
    };
    let binding_project_id: Option<Uuid> =
        sqlx::query_scalar("SELECT project_id FROM scheduler_bindings WHERE id = $1 LIMIT 1")
            .bind(binding_id)
            .fetch_optional(&mut **tx.inner())
            .await?;
    let Some(project_id) = binding_project_id else {
        return Err(ToolExecError::AttributeError {
            attribute: "project_id",
        });
    };
    let locked: Option<Uuid> = sqlx::query_scalar(plan::CREATE_PROJECT_LOCK_SQL)
        .bind(project_id)
        .fetch_optional(&mut **tx.inner())
        .await?;
    if locked.is_none() {
        return Err(ToolExecError::Missing { model: "Project" });
    }
    let tool_state: Option<Uuid> = sqlx::query_scalar(plan::CREATE_DEFAULT_STATE_SQL)
        .bind(project_id)
        .fetch_optional(&mut **tx.inner())
        .await?
        .flatten();
    let tool_state = match tool_state {
        Some(state) => Some(state),
        None => sqlx::query_scalar(plan::CREATE_BACKLOG_STATE_SQL)
            .bind(project_id)
            .fetch_optional(&mut **tx.inner())
            .await?
            .flatten(),
    };
    // The tool's `seq` probe executes; `save` then discards its value
    // for the effective sequence.
    let _: Option<i64> = sqlx::query_scalar(plan::CREATE_SEQ_MAX_SQL)
        .bind(project_id)
        .fetch_optional(&mut **tx.inner())
        .await?
        .flatten();
    let state_id = match tool_state {
        Some(state) => Some(state),
        None => {
            let repaired: Option<Uuid> = sqlx::query_scalar(plan::CREATE_SAVE_DEFAULT_STATE_SQL)
                .bind(project_id)
                .fetch_optional(&mut **tx.inner())
                .await?
                .flatten();
            match repaired {
                Some(state) => Some(state),
                None => sqlx::query_scalar(plan::CREATE_SAVE_FALLBACK_STATE_SQL)
                    .bind(project_id)
                    .fetch_optional(&mut **tx.inner())
                    .await?
                    .flatten(),
            }
        }
    };
    sqlx::query(plan::CREATE_ADVISORY_LOCK_SQL)
        .bind(plan::advisory_lock_key(&project_id))
        .execute(&mut **tx.inner())
        .await?;
    let last_sequence: Option<i64> = sqlx::query_scalar(plan::CREATE_SEQ_SCAN_SQL)
        .bind(project_id)
        .fetch_optional(&mut **tx.inner())
        .await?
        .flatten();
    let sequence_id = plan::saved_sequence_id(last_sequence);
    let largest_sort_order: Option<f64> = match state_id {
        Some(state) => sqlx::query_scalar(plan::CREATE_SORT_ORDER_MAX_SQL)
            .bind(project_id)
            .bind(state)
            .fetch_optional(&mut **tx.inner())
            .await?
            .flatten(),
        None => sqlx::query_scalar(
            "SELECT MAX(sort_order) FROM issues \
                 WHERE project_id = $1 AND state_id IS NULL AND deleted_at IS NULL",
        )
        .bind(project_id)
        .fetch_optional(&mut **tx.inner())
        .await?
        .flatten(),
    };
    let sort_order = plan::saved_sort_order(largest_sort_order);
    let pod_id: Option<Uuid> = sqlx::query_scalar(plan::CREATE_DEFAULT_POD_SQL)
        .bind(project_id)
        .fetch_optional(&mut **tx.inner())
        .await?
        .flatten();
    // `completed_at` stamps only when the TOOL supplied the state and
    // its group is `completed` — a save-repaired state never stamps.
    let now = Utc::now();
    let completed_at: Option<DateTime<Utc>> = match tool_state {
        Some(state) => {
            let group = state_view_tx(tx, state).await?.map(|view| view.group);
            if group.as_deref() == Some("completed") {
                Some(now)
            } else {
                None
            }
        }
        None => None,
    };
    // `to_safe_html` never yields `""` (the `<p></p>` fallback), so the
    // `NULL` arm is defensive, kept for the port's exact shape.
    let html = to_safe_html(Some(description));
    let stripped: Option<String> = if html.is_empty() {
        None
    } else {
        Some(ml_strip_tags(&html))
    };
    let issue_id = Uuid::new_v4();
    let sequence_row_id = Uuid::new_v4();
    sqlx::query(plan::CREATE_ISSUE_INSERT_SQL)
        .bind(issue_id)
        .bind(now)
        .bind(now)
        .bind(scope.creator_id)
        .bind(project_id)
        .bind(scope.workspace_id)
        .bind(state_id)
        .bind(stripped_title)
        .bind(html.as_str())
        .bind(stripped.as_deref())
        .bind(sequence_id)
        .bind(sort_order)
        .bind(completed_at)
        .bind(pod_id)
        .execute(&mut **tx.inner())
        .await?;
    sqlx::query(plan::CREATE_ISSUE_SEQUENCE_INSERT_SQL)
        .bind(sequence_row_id)
        .bind(now)
        .bind(scope.creator_id)
        .bind(project_id)
        .bind(scope.workspace_id)
        .bind(issue_id)
        .bind(sequence_id)
        .execute(&mut **tx.inner())
        .await?;
    // `{"created": True, **_issue_data(issue)}` (brief, `created`
    // first): the project identifier plus the resolved state.
    let project_identifier: Option<String> =
        sqlx::query_scalar("SELECT identifier FROM projects WHERE id = $1 LIMIT 1")
            .bind(project_id)
            .fetch_optional(&mut **tx.inner())
            .await?
            .flatten();
    let (state_name, state_group) = match state_id {
        Some(state) => match state_view_tx(tx, state).await? {
            Some(view) => (Some(view.name), Some(view.group)),
            None => (None, None),
        },
        None => (None, None),
    };
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
    Ok((value, CreateDispatch { issue_id, state_id }))
}

/// What a create hands its caller for the post-commit orchestration
/// fire (`signals.py:74-107`): the new issue plus its resolved state.
/// `state_id` is `None` when the project has no state at all — the
/// fire's early return (Django's `prev == current` no-op).
struct CreateDispatch {
    issue_id: Uuid,
    state_id: Option<Uuid>,
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

/// One [`CLOCK_POLICY_SQL`] row: ticking flag, pool, impl/review/test
/// cadence columns (the scheduler-dispatch alias shape).
type ClockPolicyRow = (
    Option<bool>,
    Option<i32>,
    Option<i32>,
    Option<i32>,
    Option<i32>,
);

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
        let row: Option<(Uuid, Option<Uuid>)> = sqlx::query_as(PRIOR_STATE_SELECT_SQL)
            .bind(issue_id)
            .fetch_optional(self.pool)
            .await
            .map_err(Self::db)?;
        Ok(row.and_then(|(_, state_id)| state_id))
    }

    async fn queued_follow_up(&mut self, issue_id: Uuid) -> Result<Option<RunView>, CreationError> {
        self.store.queued_follow_up(issue_id).await
    }

    async fn lock_ticker(
        &mut self,
        issue_id: Uuid,
    ) -> Result<Option<IssueAgentTicker>, CreationError> {
        let row: Option<PgRow> = sqlx::query(&lock_ticker_by_issue_sql())
            .bind(issue_id)
            .fetch_optional(self.pool)
            .await
            .map_err(Self::db)?;
        row.map(|row| map_ticker(&row))
            .transpose()
            .map_err(Self::db)
    }

    async fn save_ticker(
        &mut self,
        row: &IssueAgentTicker,
        write: ClockWrite,
    ) -> Result<(), CreationError> {
        match write {
            ClockWrite::None => Ok(()),
            ClockWrite::Insert => {
                sqlx::query(&create_ticker_sql())
                    .bind(row.created_at)
                    .bind(row.created_by_id)
                    .bind(row.id)
                    .bind(row.issue_id)
                    .bind(row.next_run_at)
                    .bind(row.enabled)
                    .bind(row.disarm_reason.as_str())
                    .bind(row.pending_entry)
                    .bind(row.pending_entry_free)
                    .bind(row.pending_entry_actor_id)
                    .bind(row.pending_entry_trigger.as_str())
                    .bind(row.resume_parent_run_id)
                    .execute(self.pool)
                    .await
                    .map_err(Self::db)?;
                Ok(())
            }
            ClockWrite::Update => {
                sqlx::query(&save_clock_sql())
                    .bind(Utc::now())
                    .bind(row.granted)
                    .bind(row.next_run_at)
                    .bind(row.enabled)
                    .bind(row.disarm_reason.as_str())
                    .bind(row.pending_entry)
                    .bind(row.pending_entry_free)
                    .bind(row.pending_entry_actor_id)
                    .bind(row.pending_entry_trigger.as_str())
                    .bind(row.resume_parent_run_id)
                    .bind(row.id)
                    .execute(self.pool)
                    .await
                    .map_err(Self::db)?;
                Ok(())
            }
        }
    }

    fn set_rollback(&mut self) {
        self.store.set_rollback()
    }

    async fn clock_policy(
        &mut self,
        project_id: Uuid,
    ) -> Result<ProjectClockPolicy, CreationError> {
        let row: Option<ClockPolicyRow> = sqlx::query_as(CLOCK_POLICY_SQL)
            .bind(project_id)
            .fetch_optional(self.pool)
            .await
            .map_err(Self::db)?;
        let Some((ticking_enabled, max_ticks, interval, interval_review, interval_test)) = row
        else {
            return Err(CreationError::MissingRow(format!(
                "no clock policy {project_id}"
            )));
        };
        Ok(ProjectClockPolicy {
            agent_ticking_enabled: ticking_enabled,
            agent_default_max_ticks: max_ticks,
            agent_default_interval_seconds: interval.map(i64::from),
            agent_review_default_interval_seconds: interval_review.map(i64::from),
            agent_test_default_interval_seconds: interval_test.map(i64::from),
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

/// Preflight for the tool's transition: unreachable. Agent moves
/// (`moved_by_run` set, always on this path) never set `dispatch_now`
/// (`scheduling.py:412-436`, ported `clock.rs:488`), so the driver
/// returns before the builder — queue, park, or a non-trigger reason.
/// If the clock contract ever changes, this fails the tool closed
/// (loud, audit-rolled-back) instead of mis-dispatching. Pinned by
/// `agent_moves_never_dispatch_now`.
struct BinPreflight;

impl PreflightSeam for BinPreflight {
    async fn preflight_eligibility_or_bounce(
        &mut self,
        _issue_id: Uuid,
        _creator_id: Uuid,
        _pod_id: Uuid,
        _triggered_by: &str,
    ) -> Result<bool, pidash_services::orchestration::creation::CreationError> {
        Err(pidash_services::orchestration::creation::CreationError::Db(
            "tool transition reached preflight: agent moves never dispatch_now".to_owned(),
        ))
    }
}

/// What a state-changing transition hands its caller for the
/// post-commit completion comment-back (`github_signals.py:44-75`):
/// the moved issue, its locked pre-save state (Django's
/// `capture_prior_state` value), and the new state.
struct TransitionDispatch {
    issue_id: Uuid,
    from_state_id: Option<Uuid>,
    target_id: Uuid,
}

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
/// Takes the audit transaction by value (the creation store owns it
/// while driving) and returns it for commit or rollback, plus — only
/// when the state changed — the [`TransitionDispatch`] the caller
/// fires after the commit lands.
async fn op_transition_current_issue<'a>(
    ctx: &'a ToolCtx,
    tx: Transaction<'a>,
    scope: &ToolScope,
    state_id: &str,
) -> (
    Transaction<'a>,
    Result<(Value, Option<TransitionDispatch>), ToolExecError>,
) {
    let mut tx = tx;
    // `run.work_item.project_id` evaluates before the `.get()` call,
    // so a run with no bound issue raises before parsing `state_id`.
    if scope.work_item_id.is_none() {
        return (
            tx,
            Err(ToolExecError::AttributeError {
                attribute: "project_id",
            }),
        );
    }
    let Some(target_id) = parse_django_uuid(state_id) else {
        return (
            tx,
            Err(ToolExecError::InvalidUuid {
                value: state_id.to_owned(),
            }),
        );
    };
    let target: Option<(Uuid, String)> =
        match sqlx::query_as::<_, (Uuid, String)>(plan::TRANSITION_STATE_GET_SQL)
            .bind(target_id)
            .bind(scope.project_id)
            .fetch_optional(&mut **tx.inner())
            .await
        {
            Ok(row) => row,
            Err(error) => return (tx, Err(error.into())),
        };
    let Some((target_id, target_name)) = target else {
        return (tx, Err(ToolExecError::Missing { model: "State" }));
    };
    let locked: Option<(Uuid, Option<Uuid>)> =
        match sqlx::query_as::<_, (Uuid, Option<Uuid>)>(plan::TRANSITION_ISSUE_LOCK_SQL)
            .bind(scope.work_item_id)
            .fetch_optional(&mut **tx.inner())
            .await
        {
            Ok(row) => row,
            Err(error) => return (tx, Err(error.into())),
        };
    let Some((issue_id, from_state_id)) = locked else {
        return (tx, Err(ToolExecError::Missing { model: "Issue" }));
    };
    if plan::transition_same_state(from_state_id, target_id) {
        let result = plan::transition_result(false, &target_name, &target_id.to_string());
        let value = serde_json::to_value(result)
            .map(|value| (value, None))
            .map_err(|error| {
                ToolExecError::Failure(plan::ToolCallFailure::Failed(error.to_string()))
            });
        return (tx, value);
    }
    let now = Utc::now();
    if let Err(error) = sqlx::query(plan::TRANSITION_ISSUE_SAVE_SQL)
        .bind(issue_id)
        .bind(target_id)
        .bind(now)
        .bind(scope.creator_id)
        .execute(&mut **tx.inner())
        .await
    {
        return (tx, Err(error.into()));
    }
    // The orchestration states: `None` when stateless (or on a dangling
    // from-pointer, the `select_related` miss). The target group comes
    // from a second read — the ported target lookup projects id+name
    // only.
    let from_state = match from_state_id {
        Some(id) => match state_view_tx(&mut tx, id).await {
            Ok(view) => view,
            Err(error) => return (tx, Err(error)),
        },
        None => None,
    };
    let to_state = match state_view_tx(&mut tx, target_id).await {
        Ok(Some(view)) => view,
        Ok(None) => {
            // TOCTOU: the target vanished between the lookup and the
            // group read (Python holds the object and would proceed).
            // Fail closed; the audit rolls back.
            return (tx, Err(ToolExecError::Missing { model: "State" }));
        }
        Err(error) => return (tx, Err(error)),
    };
    let outcome = drive_transition(ctx, tx, scope, issue_id, from_state, to_state, now).await;
    let (tx, outcome) = outcome;
    let value = outcome.map(|()| {
        let result = plan::transition_result(true, &target_name, &target_id.to_string());
        let value = serde_json::to_value(result).expect("transition result serializes");
        let dispatch = TransitionDispatch {
            issue_id,
            from_state_id,
            target_id,
        };
        (value, Some(dispatch))
    });
    (tx, value)
}

/// Drive `handle_issue_state_transition` over the audit transaction:
/// build the live store (creator presence prefetched; the actor is
/// always the run creator on this path, so the fallback chain never
/// consults), run the driver, recover the transaction plus outboxes,
/// and drain. No creation is reachable (agent moves never dispatch),
/// so the outboxes are always empty — the drain loops are the
/// documented driver shape, kept for exactness.
async fn drive_transition<'a>(
    ctx: &'a ToolCtx,
    tx: Transaction<'a>,
    scope: &ToolScope,
    issue_id: Uuid,
    from_state: Option<pidash_services::orchestration::creation::StateView>,
    to_state: pidash_services::orchestration::creation::StateView,
    now: DateTime<Utc>,
) -> (Transaction<'a>, Result<(), ToolExecError>) {
    let creator = scope.creator_id;
    let has_key =
        match crate::oss_worker::execute_seams::LiveExecuteSeams::llm_row(&ctx.pool, creator).await
        {
            Ok(row) => row.as_ref().is_some_and(|row| row.has_api_key()),
            Err(error) => return (tx, Err(ToolExecError::Db(error))),
        };
    let deps = StoreDeps {
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
        now_unix_secs: now.timestamp(),
    };
    // `jitter_secs: 0.0` is exact: Python draws jitter only inside the
    // retime paths (`scheduling.py:222,813`), which agent moves never
    // reach. `created_by: None`: `impersonate` wraps the save only —
    // orchestration runs outside it, as in Python.
    let request = TransitionRequest {
        issue_id,
        from_state,
        to_state: Some(to_state),
        actor: Some(creator),
        dispatch_immediate: true,
        moved_by_run: Some(scope.run_id),
        now,
        jitter_secs: 0.0,
        created_by: None,
    };
    let mut store = LiveCreationStore::new(tx, &ctx.pool, deps);
    let mut preflight = BinPreflight;
    let outcome = handle_issue_state_transition(&mut store, &mut preflight, &request).await;
    let (tx, pool, cache, dispatches, deferred, terminal_effects) = store.into_parts();
    match outcome {
        Err(error) => {
            let creation = match error {
                pidash_services::orchestration::entries::EntriesError::Store(store) => store,
                other => {
                    pidash_services::orchestration::creation::CreationError::Db(other.to_string())
                }
            };
            (tx, Err(ToolExecError::Creation(creation)))
        }
        Ok(_) => {
            for run_id in dispatches {
                if let Err(error) = dispatch_agent_run(pool, &ctx.cloud, run_id, now).await {
                    return (tx, Err(ToolExecError::Db(error)));
                }
            }
            for consume in &deferred {
                consume_admission_token(&cache, consume);
            }
            if !terminal_effects.is_empty() {
                return (
                    tx,
                    Err(ToolExecError::Creation(
                        pidash_services::orchestration::creation::CreationError::Db(
                            "tool transition finalized a run: unreachable, no creation on this path"
                                .to_owned(),
                        ),
                    )),
                );
            }
            (tx, Ok(()))
        }
    }
}

// ---------------------------------------------------------------------------
// Relations (`tools.py:373-448` over `orchestration/relations.py`)
// ---------------------------------------------------------------------------

/// Render `:named` parameters to `$N`: single pass, `:` + ident-char
/// tokens only, so `:id` never corrupts `:issue_id`.
fn render_named_params(sql: &str, params: &[(&str, usize)]) -> String {
    let mut out = String::with_capacity(sql.len());
    let mut chars = sql.chars().peekable();
    while let Some(char) = chars.next() {
        if char != ':' {
            out.push(char);
            continue;
        }
        let mut name = String::new();
        while let Some(next) = chars.peek() {
            if next.is_ascii_alphanumeric() || *next == '_' {
                name.push(*next);
                chars.next();
            } else {
                break;
            }
        }
        match params.iter().find(|(key, _)| *key == name) {
            Some((_, n)) => out.push_str(&format!("${n}")),
            None => {
                out.push(':');
                out.push_str(&name);
            }
        }
    }
    out
}

/// The member-issues pool as an `IN` subquery over
/// [`MEMBER_PROJECT_ISSUES_SQL`] (binds `$1` user id, `$2` workspace
/// slug — the const's own numbering, preserved textually).
fn member_pool_subquery() -> String {
    format!("SELECT pool.id FROM ({MEMBER_PROJECT_ISSUES_SQL}) AS pool")
}

/// `resolve_refs` (`relations.py:138-163`) against the member pool:
/// each raw ref classifies (UUID or `PROJ-123`), resolves with the
/// ported predicates plus Django's `.first()` ordering, and misses
/// collect `ref or str(raw)`. `own_project` narrows to the run's
/// project ([`plan::OWN_PROJECT_ISSUES_PREDICATE`], `$3`). Returns
/// `(found, unresolved)` in request order.
async fn resolve_refs_tx(
    tx: &mut Transaction<'_>,
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
            rel::RefKind::Id(id) => resolve_by_id_tx(tx, scope, own_project, *id).await?,
            rel::RefKind::Identifier {
                project_code,
                sequence_id,
            } => {
                resolve_by_identifier_tx(tx, scope, own_project, project_code, *sequence_id).await?
            }
        };
        match matched {
            Some(issue) => found.push(issue),
            None => unresolved.push(rel::unresolved_passthrough(raw)),
        }
    }
    Ok((found, unresolved))
}

/// The UUID half of `resolve_refs`: `$1`/`$2` pool, `$3` project when
/// narrowed, then the canonical id.
async fn resolve_by_id_tx(
    tx: &mut Transaction<'_>,
    scope: &ToolScope,
    own_project: bool,
    id: Uuid,
) -> Result<Option<rel::ResolvedIssue>, ToolExecError> {
    let ref_bind = if own_project { 4 } else { 3 };
    let predicate = render_named_params(
        &rel::resolve_id_predicate("issues"),
        &[("ref_id", ref_bind)],
    );
    let sql = resolve_sql(own_project, &predicate);
    let mut query = sqlx::query(&sql)
        .bind(scope.creator_id)
        .bind(scope.workspace_slug.as_str());
    if own_project {
        query = query.bind(scope.project_id);
    }
    let row: Option<sqlx::postgres::PgRow> =
        query.bind(id).fetch_optional(&mut **tx.inner()).await?;
    row.map(resolved_issue_from_row).transpose()
}

/// The identifier half of `resolve_refs`: pool, optional narrowing,
/// then code + sequence.
async fn resolve_by_identifier_tx(
    tx: &mut Transaction<'_>,
    scope: &ToolScope,
    own_project: bool,
    project_code: &str,
    sequence_id: i64,
) -> Result<Option<rel::ResolvedIssue>, ToolExecError> {
    let (code_bind, seq_bind) = if own_project { (4, 5) } else { (3, 4) };
    let predicate = render_named_params(
        &rel::resolve_identifier_predicate("issues", "projects"),
        &[("project_code", code_bind), ("sequence_id", seq_bind)],
    );
    let sql = resolve_sql(own_project, &predicate);
    let mut query = sqlx::query(&sql)
        .bind(scope.creator_id)
        .bind(scope.workspace_slug.as_str());
    if own_project {
        query = query.bind(scope.project_id);
    }
    let row: Option<sqlx::postgres::PgRow> = query
        .bind(project_code)
        .bind(sequence_id)
        .fetch_optional(&mut **tx.inner())
        .await?;
    row.map(resolved_issue_from_row).transpose()
}

/// The resolve statement: the `ResolvedIssue` columns over the member
/// pool plus the ref predicate, newest-first with `LIMIT 1` (Django's
/// `.first()` under `-created_at` ordering).
fn resolve_sql(own_project: bool, predicate: &str) -> String {
    let pool = member_pool_subquery();
    let narrow = if own_project {
        format!("AND {}", plan::OWN_PROJECT_ISSUES_PREDICATE)
    } else {
        String::new()
    };
    format!(
        "SELECT issues.id, issues.workspace_id, issues.project_id, \
         projects.identifier, issues.sequence_id FROM issues \
         INNER JOIN projects ON projects.id = issues.project_id \
         WHERE issues.id IN ({pool}) {narrow} AND ({predicate}) \
         ORDER BY issues.created_at DESC LIMIT 1"
    )
}

/// Map one resolve row onto [`rel::ResolvedIssue`].
fn resolved_issue_from_row(
    row: sqlx::postgres::PgRow,
) -> Result<rel::ResolvedIssue, ToolExecError> {
    Ok(rel::ResolvedIssue {
        id: row.try_get("id").map_err(ToolExecError::Db)?,
        workspace_id: row.try_get("workspace_id").map_err(ToolExecError::Db)?,
        project_id: row.try_get("project_id").map_err(ToolExecError::Db)?,
        project_identifier: row.try_get("identifier").map_err(ToolExecError::Db)?,
        sequence_id: row.try_get("sequence_id").map_err(ToolExecError::Db)?,
    })
}

/// `_pair_rows` (`relations.py:117-120`) over the audit transaction.
async fn pair_rows_tx(
    tx: &mut Transaction<'_>,
    a_id: Uuid,
    b_id: Uuid,
) -> Result<Vec<rel::PairRow>, ToolExecError> {
    let sql = render_named_params(&rel::pair_rows_sql(), &[("a_id", 1), ("b_id", 2)]);
    let rows = sqlx::query(&sql)
        .bind(a_id)
        .bind(b_id)
        .fetch_all(&mut **tx.inner())
        .await?;
    rows.iter()
        .map(|row| {
            Ok(rel::PairRow {
                id: row.try_get("id").map_err(ToolExecError::Db)?,
                issue_id: row.try_get("issue_id").map_err(ToolExecError::Db)?,
                related_issue_id: row.try_get("related_issue_id").map_err(ToolExecError::Db)?,
                relation_type: row.try_get("relation_type").map_err(ToolExecError::Db)?,
            })
        })
        .collect()
}

/// `pidash_relate_issues` (`tools.py:416-424` over
/// `relations.py:180-232`): per-target verdict loop with a savepointed
/// insert on `Create` (23505 re-reads and re-verdicts, quirk 5
/// looping back to `Create` when the race resolves to nothing), one
/// `issue_activity` emit for the batch, and the refreshed grouping.
async fn op_relate_issues(
    tx: &mut Transaction<'_>,
    scope: &ToolScope,
    args: &plan::RelationWriteArgs,
) -> Result<Value, ToolExecError> {
    let (sources, unresolved) =
        resolve_refs_tx(tx, scope, std::slice::from_ref(&args.issue), true).await?;
    if !unresolved.is_empty() {
        let refs: Vec<&str> = unresolved.iter().map(String::as_str).collect();
        return Err(plan::ToolCallFailure::InvalidArg(plan::unresolved_refs_message(&refs)).into());
    }
    let (targets, unresolved) = resolve_refs_tx(tx, scope, &args.related_issues, false).await?;
    if !unresolved.is_empty() {
        let refs: Vec<&str> = unresolved.iter().map(String::as_str).collect();
        return Err(plan::ToolCallFailure::InvalidArg(plan::unresolved_refs_message(&refs)).into());
    }
    let source = sources
        .into_iter()
        .next()
        .expect("one source ref resolves one row");
    let targets = rel::check_targets(&source, &targets)
        .map_err(|error| plan::ToolCallFailure::InvalidArg(error.to_string()))?;
    let mut created: Vec<rel::ResolvedIssue> = Vec::new();
    let mut unchanged: Vec<String> = Vec::new();
    let mut conflicts: Vec<rel::RelateConflict> = Vec::new();
    for target in &targets {
        loop {
            let existing = pair_rows_tx(tx, source.id, target.id).await?;
            match rel::relate_verdict(&existing, &source.id, &args.relation_type) {
                rel::RelateVerdict::Unchanged => {
                    unchanged.push(target.identifier());
                    break;
                }
                rel::RelateVerdict::Conflict { existing_relation } => {
                    conflicts.push(rel::RelateConflict {
                        identifier: target.identifier(),
                        existing_relation,
                    });
                    break;
                }
                rel::RelateVerdict::Create => {
                    let row = rel::relate_insert_row(
                        Uuid::new_v4(),
                        &source,
                        &args.relation_type,
                        target,
                        scope.creator_id,
                    );
                    match relate_insert_tx(tx, &row).await {
                        Ok(()) => {
                            created.push(target.clone());
                            break;
                        }
                        Err(error) => {
                            let raced =
                                matches!(&error, ToolExecError::Db(db) if is_unique_violation(db));
                            if raced {
                                // A concurrent writer won the race;
                                // re-read and re-verdict (quirk 5
                                // loops here when the race resolves to
                                // nothing).
                                continue;
                            }
                            return Err(error);
                        }
                    }
                }
            }
        }
    }
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
        enqueue_activity(tx, &emit).await?;
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
    let grouped = grouped_tx(tx, scope, &source).await?;
    map = plan::with_grouped_relations(map.as_object().cloned().unwrap_or_default(), grouped);
    Ok(map)
}

/// One savepointed relate `INSERT` (`relations.py:198-207`): the
/// savepoint scopes the 23505 rollback — anything else aborts the
/// audit transaction, as Python's uncaught raise does.
async fn relate_insert_tx(
    tx: &mut Transaction<'_>,
    row: &rel::RelateInsert,
) -> Result<(), ToolExecError> {
    let sql = render_named_params(
        &rel::relate_insert_sql(),
        &[
            ("related_issue_id", 7),
            ("workspace_id", 5),
            ("stored_type", 8),
            ("project_id", 4),
            ("actor_id", 3),
            ("issue_id", 6),
            ("now", 2),
            ("id", 1),
        ],
    );
    let now = Utc::now();
    sqlx::query("SAVEPOINT relate_target")
        .execute(&mut **tx.inner())
        .await?;
    let inserted = sqlx::query(&sql)
        .bind(row.id)
        .bind(now)
        .bind(row.created_by_id)
        .bind(row.project_id)
        .bind(row.workspace_id)
        .bind(row.issue_id)
        .bind(row.related_issue_id)
        .bind(row.relation_type.as_str())
        .execute(&mut **tx.inner())
        .await;
    match inserted {
        Ok(_) => {
            sqlx::query("RELEASE SAVEPOINT relate_target")
                .execute(&mut **tx.inner())
                .await?;
            Ok(())
        }
        Err(error) => {
            // Scope the rollback to the savepoint; cleanup failures
            // cannot mask the original (Python's atomic exit raises
            // the `IntegrityError`, not a teardown error).
            let _ = sqlx::query("ROLLBACK TO SAVEPOINT relate_target")
                .execute(&mut **tx.inner())
                .await;
            let _ = sqlx::query("RELEASE SAVEPOINT relate_target")
                .execute(&mut **tx.inner())
                .await;
            Err(error.into())
        }
    }
}

/// `pidash_unrelate_issues` (`tools.py:426-431` over
/// `relations.py:235-266`): per-target exact-type delete (soft-delete
/// plus the related-objects sweep per row), one `issue_activity` emit
/// per removed target, and the refreshed grouping.
async fn op_unrelate_issues(
    tx: &mut Transaction<'_>,
    scope: &ToolScope,
    args: &plan::RelationWriteArgs,
) -> Result<Value, ToolExecError> {
    let (sources, unresolved) =
        resolve_refs_tx(tx, scope, std::slice::from_ref(&args.issue), true).await?;
    if !unresolved.is_empty() {
        let refs: Vec<&str> = unresolved.iter().map(String::as_str).collect();
        return Err(plan::ToolCallFailure::InvalidArg(plan::unresolved_refs_message(&refs)).into());
    }
    let (targets, unresolved) = resolve_refs_tx(tx, scope, &args.related_issues, false).await?;
    if !unresolved.is_empty() {
        let refs: Vec<&str> = unresolved.iter().map(String::as_str).collect();
        return Err(plan::ToolCallFailure::InvalidArg(plan::unresolved_refs_message(&refs)).into());
    }
    let source = sources
        .into_iter()
        .next()
        .expect("one source ref resolves one row");
    let targets = rel::check_targets(&source, &targets)
        .map_err(|error| plan::ToolCallFailure::InvalidArg(error.to_string()))?;
    let mut removed: Vec<String> = Vec::new();
    let mut not_related: Vec<String> = Vec::new();
    for target in &targets {
        let existing = pair_rows_tx(tx, source.id, target.id).await?;
        let matching = rel::unrelate_matches(&existing, &source.id, &args.relation_type);
        if matching.is_empty() {
            not_related.push(target.identifier());
            continue;
        }
        for row in matching {
            let now = Utc::now();
            let sql = render_named_params(
                &rel::unrelate_delete_sql(),
                &[("actor_id", 2), ("row_id", 3), ("now", 1)],
            );
            sqlx::query(&sql)
                .bind(now)
                .bind(scope.creator_id)
                .bind(row.id)
                .execute(&mut **tx.inner())
                .await?;
            enqueue_sweep(tx, "issuerelation", &row.id.to_string()).await?;
        }
        removed.push(target.identifier());
        let emit = rel::unrelate_deleted_emit(
            &args.relation_type,
            &target.id,
            &scope.creator_id,
            &source.id,
            &source.project_id,
            Utc::now().timestamp(),
        );
        enqueue_activity(tx, &emit).await?;
    }
    let result = rel::UnrelateResult {
        issue: source.identifier(),
        relation_type: args.relation_type.clone(),
        removed,
        not_related,
    };
    let mut map = serde_json::to_value(result)
        .map_err(|error| plan::ToolCallFailure::Failed(error.to_string()))?;
    let grouped = grouped_tx(tx, scope, &source).await?;
    map = plan::with_grouped_relations(map.as_object().cloned().unwrap_or_default(), grouped);
    Ok(map)
}

/// `grouped_relations` (`relations.py:280-313`) over the audit
/// transaction: the live rows touching the source plus the visible
/// pool narrowed to the wanted other ends, shaped by the ported
/// assembler.
async fn grouped_tx(
    tx: &mut Transaction<'_>,
    scope: &ToolScope,
    source: &rel::ResolvedIssue,
) -> Result<Value, ToolExecError> {
    let sql = render_named_params(
        &rel::grouped_relations_sql(),
        &[("issue_id", 1), ("workspace_id", 2)],
    );
    let rows = sqlx::query(&sql)
        .bind(source.id)
        .bind(source.workspace_id)
        .fetch_all(&mut **tx.inner())
        .await?;
    let pair_rows: Vec<rel::PairRow> = rows
        .iter()
        .map(|row| {
            Ok(rel::PairRow {
                id: row.try_get("id").map_err(ToolExecError::Db)?,
                issue_id: row.try_get("issue_id").map_err(ToolExecError::Db)?,
                related_issue_id: row.try_get("related_issue_id").map_err(ToolExecError::Db)?,
                relation_type: row.try_get("relation_type").map_err(ToolExecError::Db)?,
            })
        })
        .collect::<Result<Vec<_>, ToolExecError>>()?;
    let mut wanted: Vec<Uuid> = Vec::new();
    for row in &pair_rows {
        let other = if row.issue_id == source.id {
            row.related_issue_id
        } else {
            row.issue_id
        };
        if !wanted.contains(&other) {
            wanted.push(other);
        }
    }
    let mut pool: Vec<rel::GroupedIssue> = Vec::new();
    if !wanted.is_empty() {
        let pool_query = member_pool_subquery();
        let rows = sqlx::query(&format!(
            "SELECT issues.id, issues.name, issues.sequence_id, \
             projects.identifier AS project_identifier, \
             states.name AS state_name, states.group AS state_group \
             FROM issues \
             LEFT OUTER JOIN states ON states.id = issues.state_id \
             INNER JOIN projects ON projects.id = issues.project_id \
             WHERE issues.id IN ({pool_query}) \
             AND issues.id = ANY($3) AND issues.workspace_id = $4"
        ))
        .bind(scope.creator_id)
        .bind(scope.workspace_slug.as_str())
        .bind(wanted.as_slice())
        .bind(source.workspace_id)
        .fetch_all(&mut **tx.inner())
        .await?;
        for row in &rows {
            pool.push(rel::GroupedIssue {
                id: row.try_get("id").map_err(ToolExecError::Db)?,
                project_identifier: row
                    .try_get("project_identifier")
                    .map_err(ToolExecError::Db)?,
                sequence_id: row.try_get("sequence_id").map_err(ToolExecError::Db)?,
                name: row.try_get("name").map_err(ToolExecError::Db)?,
                state_name: row.try_get("state_name").map_err(ToolExecError::Db)?,
                state_group: row.try_get("state_group").map_err(ToolExecError::Db)?,
            });
        }
    }
    let grouped = rel::grouped_relations(&source.id, &pair_rows, &pool);
    serde_json::to_value(grouped)
        .map_err(|error| ToolExecError::Failure(plan::ToolCallFailure::Failed(error.to_string())))
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
    // The resolve/grouped helpers run over a transaction; the read
    // path holds none, so list inside a scratch transaction that always
    // rolls back (read-only shape — the rollback is a formality).
    let mut tx = Transaction::begin(&ctx.pool).await?;
    let source = match plan::relations_source(issue_arg, scope.work_item_id)? {
        plan::RelationsSource::Ref(reference) => {
            let (found, unresolved) =
                resolve_refs_tx(&mut tx, scope, std::slice::from_ref(&reference), false).await?;
            if !unresolved.is_empty() {
                let refs: Vec<&str> = unresolved.iter().map(String::as_str).collect();
                return Err(
                    plan::ToolCallFailure::InvalidArg(plan::unresolved_refs_message(&refs)).into(),
                );
            }
            found.into_iter().next().expect("one ref resolves one row")
        }
        plan::RelationsSource::CurrentIssue => {
            let issue = bound_issue(&ctx.pool, scope.work_item_id).await?;
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
    let grouped = grouped_tx(&mut tx, scope, &source).await;
    let _ = tx.rollback().await;
    Ok(plan::issue_relations_result(&source.identifier(), grouped?))
}

/// Publish one `_log_activity` emit transactionally (the
/// `RelationActivityEmit` contract): same audit transaction as the
/// relate/unrelate write; enqueue errors propagate (Python's `.delay`
/// has no `try`).
async fn enqueue_activity(
    tx: &mut Transaction<'_>,
    emit: &rel::RelationActivityEmit,
) -> Result<(), ToolExecError> {
    let job = NewJob::new(
        emit.task_name(),
        Value::Array(emit.args()),
        Value::Object(emit.kwargs()),
    );
    enqueue_in(tx, &job).await?;
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
async fn enqueue_sweep(
    tx: &mut Transaction<'_>,
    model_name: &str,
    pk: &str,
) -> Result<(), ToolExecError> {
    let (args, kwargs) = soft_delete_sweep(model_name, pk);
    let job = NewJob::new(SOFT_DELETE_TASK, Value::Array(args), Value::Object(kwargs));
    enqueue_in(tx, &job).await?;
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
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(plan::GITHUB_BINDING_SQL)
        .bind(scope.project_id)
        .fetch_optional(&ctx.pool)
        .await?;
    let Some(row) = row else {
        return Err(ToolExecError::Missing {
            model: "GitRepositoryBinding",
        });
    };
    // `workspace_id` (1, 9), `provider` (4, 10), and `host_url` (5,
    // 11) collide: all three pairs read positionally.
    let verified_at: Option<DateTime<Utc>> = row.try_get(14)?;
    let verdict = plan::GithubBindingVerdict {
        binding_workspace_id: row.try_get(1)?,
        run_workspace_id: scope.workspace_id,
        repo_provider: row.try_get(4)?,
        repo_host_url: row.try_get(5)?,
        account_workspace_id: row.try_get(9)?,
        account_provider: row.try_get(10)?,
        account_host_url: row.try_get(11)?,
        account_auth_type: row.try_get(12)?,
        account_status: row.try_get(13)?,
        account_verified: verified_at.is_some(),
        account_workspace_integration_id: row.try_get(15)?,
    };
    let integration_id = plan::check_github_binding(&verdict)?;
    let installation_id: Option<i64> = sqlx::query_scalar(plan::GITHUB_INSTALLATION_SQL)
        .bind(integration_id)
        .fetch_optional(&ctx.pool)
        .await?
        .flatten();
    let installation_id = plan::check_github_installation(installation_id)?;
    let timeout_secs = ctx.cloud.tool_timeout_secs.max(1) as u64;
    let client =
        GithubRestClient::for_installation(&ctx.github, &ctx.http, installation_id, timeout_secs)
            .await?;
    Ok(GithubOpCtx {
        client,
        namespace: row.try_get(6)?,
        name: row.try_get(7)?,
        default_branch: row.try_get(8)?,
    })
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
    let base_branch: Option<String> =
        sqlx::query_scalar("SELECT base_branch FROM projects WHERE id = $1 LIMIT 1")
            .bind(scope.project_id)
            .fetch_optional(&ctx.pool)
            .await?
            .flatten();
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
    let link: Option<(String, String, String)> = match scope.work_item_id {
        Some(issue_id) => {
            sqlx::query_as::<_, (String, String, String)>(plan::LINKED_PR_SQL)
                .bind(issue_id)
                .bind(repo.namespace.as_str())
                .bind(repo.name.as_str())
                .fetch_optional(&ctx.pool)
                .await?
        }
        None => None,
    };
    let Some((namespace, repo_name, external_iid)) = link else {
        return Err(
            plan::ToolCallFailure::Failed(plan::MSG_NO_LINKED_PULL_REQUEST.to_owned()).into(),
        );
    };
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
            match write_begin(
                ctx,
                run_id,
                TOOL_ADD_CURRENT_ISSUE_COMMENT,
                &audit_args,
                plan::SOURCE_INTERNAL,
                "",
            )
            .await?
            {
                AuditBegin::Replay(value) => Ok(value),
                AuditBegin::Proceed(audit) => {
                    let WriteAudit {
                        mut tx,
                        scope,
                        ledger_id,
                    } = audit;
                    match op_add_current_issue_comment(&mut tx, &scope, body).await {
                        Ok(result) => {
                            write_commit(
                                ctx,
                                WriteAudit {
                                    tx,
                                    scope,
                                    ledger_id,
                                },
                                TOOL_ADD_CURRENT_ISSUE_COMMENT,
                                result,
                            )
                            .await
                        }
                        Err(error) => {
                            let _ = tx.rollback().await;
                            Err(error)
                        }
                    }
                }
            }
        }
        TOOL_UPDATE_CURRENT_ISSUE_WORKPAD => {
            let body = args.get_str("body").expect("loop validated body");
            plan::validate_workpad_body(body)?;
            let audit_args = serde_json::json!({"body": body});
            match write_begin(
                ctx,
                run_id,
                TOOL_UPDATE_CURRENT_ISSUE_WORKPAD,
                &audit_args,
                plan::SOURCE_INTERNAL,
                "",
            )
            .await?
            {
                AuditBegin::Replay(value) => Ok(value),
                AuditBegin::Proceed(audit) => {
                    let WriteAudit {
                        mut tx,
                        scope,
                        ledger_id,
                    } = audit;
                    match op_update_current_issue_workpad(&mut tx, &scope, body).await {
                        Ok(result) => {
                            write_commit(
                                ctx,
                                WriteAudit {
                                    tx,
                                    scope,
                                    ledger_id,
                                },
                                TOOL_UPDATE_CURRENT_ISSUE_WORKPAD,
                                result,
                            )
                            .await
                        }
                        Err(error) => {
                            let _ = tx.rollback().await;
                            Err(error)
                        }
                    }
                }
            }
        }
        TOOL_TRANSITION_CURRENT_ISSUE => {
            let state_id = args.get_str("state_id").expect("loop validated state_id");
            let audit_args = serde_json::json!({"state_id": state_id});
            match write_begin(
                ctx,
                run_id,
                TOOL_TRANSITION_CURRENT_ISSUE,
                &audit_args,
                plan::SOURCE_INTERNAL,
                "",
            )
            .await?
            {
                AuditBegin::Replay(value) => Ok(value),
                AuditBegin::Proceed(audit) => {
                    let WriteAudit {
                        tx,
                        scope,
                        ledger_id,
                    } = audit;
                    let (tx, outcome) =
                        op_transition_current_issue(ctx, tx, &scope, state_id).await;
                    match outcome {
                        Ok((result, dispatch)) => {
                            let committed = write_commit(
                                ctx,
                                WriteAudit {
                                    tx,
                                    scope,
                                    ledger_id,
                                },
                                TOOL_TRANSITION_CURRENT_ISSUE,
                                result,
                            )
                            .await;
                            // `github_signals.trigger_completion_comment`
                            // (`bgtasks/github_signals.py:44-75`): a save
                            // moving a mirrored issue into a
                            // completed-group state enqueues the
                            // provider's `post_completion_comment`.
                            // Django fires this from `Issue.save()`
                            // (`tools.py:335`), so the tool path gets it
                            // for free; here signals are explicit calls
                            // after commit. `None` is the unchanged-state
                            // no-op (`updated: false`); the dispatch
                            // helper re-checks the completed group, the
                            // mirror order (git before github) and the
                            // already-commented guard. Best-effort like
                            // the relation enqueues: the edit already
                            // committed, so a lookup or queue failure
                            // must not fail the tool.
                            if committed.is_ok() {
                                if let Some(dispatch) = dispatch {
                                    enqueue_completion_comment(
                                        &ctx.pool,
                                        &dispatch.issue_id,
                                        dispatch.from_state_id.as_ref(),
                                        Some(&dispatch.target_id),
                                    )
                                    .await;
                                }
                            }
                            committed
                        }
                        Err(error) => {
                            let _ = tx.rollback().await;
                            Err(error)
                        }
                    }
                }
            }
        }
        TOOL_CREATE_PROJECT_ISSUE => {
            let title = args.get_str("title").expect("loop validated title");
            let description = args
                .get_str("description")
                .expect("loop applied description");
            let stripped = plan::validate_create_issue(title, description)?;
            let audit_args = serde_json::json!({"title": title, "description": description});
            match write_begin(
                ctx,
                run_id,
                TOOL_CREATE_PROJECT_ISSUE,
                &audit_args,
                plan::SOURCE_INTERNAL,
                "",
            )
            .await?
            {
                AuditBegin::Replay(value) => Ok(value),
                AuditBegin::Proceed(audit) => {
                    let WriteAudit {
                        mut tx,
                        scope,
                        ledger_id,
                    } = audit;
                    match op_create_project_issue(&mut tx, &scope, description, &stripped).await {
                        Ok((result, fire)) => {
                            let creator_id = scope.creator_id;
                            let run_id = scope.run_id;
                            // The orchestration post-save fire
                            // (`signals.py:74-107`): Django's
                            // `Issue.objects.create` runs it inside the
                            // save; here signals are explicit calls after
                            // the commit lands (the v1 create shape,
                            // PIDASHCONV-1216). The fire runs on
                            // commit-success even when the
                            // `tool_completed` append below fails
                            // (Django appends at `tools.py:156`, after
                            // the save's signal). Best-effort: the issue
                            // already committed.
                            match write_commit_tx(
                                ctx,
                                WriteAudit {
                                    tx,
                                    scope,
                                    ledger_id,
                                },
                                result,
                            )
                            .await
                            {
                                Ok(value) => {
                                    fire_create_transition(
                                        ctx,
                                        creator_id,
                                        fire.issue_id,
                                        fire.state_id,
                                    )
                                    .await;
                                    emit_completed(
                                        ctx,
                                        run_id,
                                        TOOL_CREATE_PROJECT_ISSUE,
                                        plan::ToolRisk::Write,
                                    )
                                    .await?;
                                    Ok(value)
                                }
                                Err(error) => Err(error),
                            }
                        }
                        Err(error) => {
                            let _ = tx.rollback().await;
                            Err(error)
                        }
                    }
                }
            }
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
            match write_begin(
                ctx,
                run_id,
                TOOL_RELATE_ISSUES,
                &audit_args,
                plan::SOURCE_INTERNAL,
                "",
            )
            .await?
            {
                AuditBegin::Replay(value) => Ok(value),
                AuditBegin::Proceed(audit) => {
                    let WriteAudit {
                        mut tx,
                        scope,
                        ledger_id,
                    } = audit;
                    match op_relate_issues(&mut tx, &scope, &write_args).await {
                        Ok(result) => {
                            write_commit(
                                ctx,
                                WriteAudit {
                                    tx,
                                    scope,
                                    ledger_id,
                                },
                                TOOL_RELATE_ISSUES,
                                result,
                            )
                            .await
                        }
                        Err(error) => {
                            let _ = tx.rollback().await;
                            Err(error)
                        }
                    }
                }
            }
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
            match write_begin(
                ctx,
                run_id,
                TOOL_UNRELATE_ISSUES,
                &audit_args,
                plan::SOURCE_INTERNAL,
                "",
            )
            .await?
            {
                AuditBegin::Replay(value) => Ok(value),
                AuditBegin::Proceed(audit) => {
                    let WriteAudit {
                        mut tx,
                        scope,
                        ledger_id,
                    } = audit;
                    match op_unrelate_issues(&mut tx, &scope, &write_args).await {
                        Ok(result) => {
                            write_commit(
                                ctx,
                                WriteAudit {
                                    tx,
                                    scope,
                                    ledger_id,
                                },
                                TOOL_UNRELATE_ISSUES,
                                result,
                            )
                            .await
                        }
                        Err(error) => {
                            let _ = tx.rollback().await;
                            Err(error)
                        }
                    }
                }
            }
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

    /// `:named` rendering never lets a short name corrupt a longer one
    /// (`:id` inside `:issue_id`).
    #[test]
    fn render_named_params_overlapping_names() {
        let sql = "SELECT :id, :now, :actor_id, :issue_id, :related_issue_id, :stored_type";
        assert_eq!(
            render_named_params(
                sql,
                &[
                    ("related_issue_id", 7),
                    ("stored_type", 8),
                    ("actor_id", 3),
                    ("issue_id", 6),
                    ("now", 2),
                    ("id", 1),
                ],
            ),
            "SELECT $1, $2, $3, $6, $7, $8"
        );
        // Unknown names pass through untouched.
        assert_eq!(
            render_named_params("SELECT :known, :mystery", &[("known", 1)]),
            "SELECT $1, :mystery"
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
