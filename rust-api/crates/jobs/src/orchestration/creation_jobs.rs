#![forbid(unsafe_code)]

//! Run-creation jobs: the live [`CreationSeam`][pidash_services::orchestration::creation::CreationSeam]
//! store + transaction drivers (D-12 L6, stage 5).
//!
//! [`LiveCreationStore`] implements the services-side seam traits over
//! one insertion [`Transaction`][pidash_db::tx::Transaction]:
//!
//! * every store query is built with SeaORM on the insertion
//!   transaction through [`TxConn`][pidash_db::orm_tx::TxConn]
//!   (no `query!` macros — there is no build-time database);
//! * the three `cloud_agent.creation` calls delegate to the merged
//!   [`crate::dispatch`] functions (`execution_fields` on its own
//!   short transaction, like the inner atomic at `creation.py:55-70`;
//!   `lock_cloud_creation_capacity` on the insertion transaction;
//!   `dispatch_after_commit` collected, never inline);
//! * `finalize_failed_run` lands the `finalize_agent_run` row effects
//!   (status/ended_at/NULLs + updates, the cloud terminal event) and
//!   captures the terminal-effects callback without executing it —
//!   terminal effects are D-15's pipeline.
//!
//! The drivers ([`create_and_dispatch_run`], [`create_continuation_run`],
//! [`create_project_move_handoff_run`], [`complete_project_move_handoff`])
//! open the insertion transaction, run the services control flow,
//! commit, then drain: dispatches via [`dispatch_agent_run`][crate::dispatch::dispatch_agent_run],
//! deferred admission consumes via
//! [`consume_admission_token`][pidash_services::dispatch::consume_admission_token].
//! Terminal effects are returned pending for D-15 — never executed here.
//!
//! The render-bundle loader mirrors the api prompt-preview fetch side
//! (`api/src/prompting`, `load_issue_context`) query for query, with
//! the ancestor walk's cycle guard + depth-50 cap.
//!
//! Fixture: `rust-api/fixtures/orchestration/fx06_creation/` (FX-ORCH-06),
//! replayed by the live suite below.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use sea_orm::{
    ActiveModelTrait, ActiveValue, ColumnTrait, Condition, DbErr, EntityTrait, FromQueryResult,
    JoinType, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect, RelationTrait, Set,
};
use sea_query::{Asterisk, Expr, Func, Query, SimpleExpr};
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

use pidash_db::config::{CloudAgentSettings, ManagedRunnerSettings};
use pidash_db::dispatch::status::AgentRunStatus;
use pidash_db::entity::{
    agent_run, agent_run_event, dev_machine, git_code_review_links, git_repositories,
    git_repository_bindings, issue_agent_ticker, issue_assignees, issue_comments, issue_labels,
    issue_relations, issues, labels, pod, projects, prompt_section_override, runner, states, users,
    workspaces,
};
use pidash_db::orchestration::runs::ACTIVE_STATUSES;
use pidash_db::orm_tx::{self, TxConn};
use pidash_db::runner_runs::agent_run as run_bag;
use pidash_db::tx::Transaction;
use pidash_services::dispatch::{
    consume_admission_token, AdmissionCache, DeferredConsume, LlmProfile, UserFlags,
};
use pidash_services::extensions::CloudAgentToolsetsSeam;
#[cfg(test)]
use pidash_services::extensions::NoExtraToolsets;
use pidash_services::orchestration::creation::{
    state_name_group, user_id_for_run, AdmissionError as ServiceAdmissionError, CreationError,
    CreationSeam, ExecutionError, ExecutionFields as ServiceExecutionFields, ExecutionRequest,
    FinalizeAgentRunSeam, IssueView, LockedIssue, NewAgentRun, PodView, ProjectView, RenderBundle,
    RunView, RunnerView, StateView, TickerBudget,
};
use pidash_services::prompting::{composer, context};
use pidash_types::dispatch::AgentExecutorKind;

use crate::dispatch::{
    dispatch_after_commit as collect_dispatch, dispatch_agent_run,
    lock_cloud_creation_capacity as lock_capacity, ActorScope, DispatchDecision,
    ExecutionFieldsError, ExecutionInputs, ExecutionSeams, ProjectScope,
};
use crate::dispatch::{execution_fields as resolve_execution_fields, DeferredAdmissionError};
use pidash_compat::datetime::render_isoformat;

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

/// The jobs-owned inputs behind a [`LiveCreationStore`].
pub struct StoreDeps<C, H, P> {
    pub cloud: CloudAgentSettings,
    pub managed: ManagedRunnerSettings,
    pub cache: C,
    /// `has_usable_llm_config(user)` (`agent_execution.py:78-80`).
    pub has_usable_llm_config: H,
    /// `extra_toolsets_enabled_for(creator)` (`toolsets.py:23-32`): the
    /// injected toolsets seam (also answers the schema-tool name).
    pub toolsets: Arc<dyn CloudAgentToolsetsSeam>,
    /// `managed_llm_profile(user)` (`managed_runner/policy.py:27-36`).
    pub llm_profile: P,
    /// `int(time.time())` for the admission buckets.
    pub now_unix_secs: i64,
}

/// The live creation store: the services seam traits over one
/// insertion transaction plus the D-11 delegation inputs.
///
/// `H`/`P` are `Clone` so each `execution_fields` call can wrap a
/// lazy `FnOnce` over the call's actor without borrowing the store.
pub struct LiveCreationStore<'c, C, H, P> {
    tx: Transaction<'c>,
    pool: &'c PgPool,
    cloud: CloudAgentSettings,
    managed: ManagedRunnerSettings,
    cache: C,
    has_usable_llm_config: H,
    toolsets: Arc<dyn CloudAgentToolsetsSeam>,
    llm_profile: P,
    now_unix_secs: i64,
    dispatches: Vec<Uuid>,
    deferred: Vec<DeferredConsume>,
    terminal_effects: Vec<Uuid>,
    rollback: bool,
}

impl<'c, C, H, P> LiveCreationStore<'c, C, H, P> {
    pub fn new(tx: Transaction<'c>, pool: &'c PgPool, deps: StoreDeps<C, H, P>) -> Self {
        Self {
            tx,
            pool,
            cloud: deps.cloud,
            managed: deps.managed,
            cache: deps.cache,
            has_usable_llm_config: deps.has_usable_llm_config,
            toolsets: deps.toolsets,
            llm_profile: deps.llm_profile,
            now_unix_secs: deps.now_unix_secs,
            dispatches: Vec::new(),
            deferred: Vec::new(),
            terminal_effects: Vec::new(),
            rollback: false,
        }
    }

    /// Recover the transaction (to commit) plus the outboxes (to drain
    /// after commit).
    pub fn into_parts(
        self,
    ) -> (
        Transaction<'c>,
        &'c PgPool,
        C,
        Vec<Uuid>,
        Vec<DeferredConsume>,
        Vec<Uuid>,
    ) {
        (
            self.tx,
            self.pool,
            self.cache,
            self.dispatches,
            self.deferred,
            self.terminal_effects,
        )
    }

    fn db(error: sqlx::Error) -> CreationError {
        CreationError::Db(error.to_string())
    }

    // -- L8 dispatch sharing -------------------------------------------------
    // The D-12 L8 dispatch drivers (`fire_tick_seam`) run on the same
    // insertion transaction the guards read (the single-transaction
    // grant+dispatch with rollback), so the services `DispatchSeam` is
    // implemented for this store. These accessors are the whole L6
    // surface it needs beyond the seam traits; nothing here changes L6
    // behavior.

    /// Borrow the insertion transaction for the dispatch seam reads and
    /// writes.
    pub(crate) fn dispatch_tx(&mut self) -> &mut Transaction<'c> {
        &mut self.tx
    }

    /// `has_usable_llm_config(user)` (`agent_execution.py:78-80`).
    pub(crate) fn dispatch_has_usable_llm_config(&self, user_id: Uuid) -> bool
    where
        H: Fn(Uuid) -> bool,
    {
        (self.has_usable_llm_config)(user_id)
    }

    /// `managed_llm_profile(user)` (`managed_runner/policy.py:27-36`).
    pub(crate) fn dispatch_llm_profile_for(&self, user_id: Uuid) -> LlmProfile
    where
        P: Fn(Option<Uuid>) -> LlmProfile,
    {
        (self.llm_profile)(Some(user_id))
    }

    /// The operator kill switch (`managed_runner_is_enabled`, L3).
    pub(crate) fn dispatch_managed_runner_enabled(&self) -> bool {
        pidash_services::dispatch::managed_runner_is_enabled(&self.managed)
    }

    /// Record an entries driver's rollback request
    /// (`transaction.set_rollback(True)`): the executing driver rolls
    /// the transaction back and drains nothing.
    pub(crate) fn dispatch_request_rollback(&mut self) {
        self.rollback = true;
    }

    /// Whether an entries driver requested a rollback.
    pub(crate) fn dispatch_rollback_requested(&self) -> bool {
        self.rollback
    }
}

/// Every failure the jobs drivers report.
#[derive(Debug, thiserror::Error)]
pub enum CreationJobsError {
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
    #[error(transparent)]
    Store(#[from] CreationError),
    #[error("dispatch error: {0}")]
    Dispatch(#[from] ExecutionFieldsError),
}

// ---------------------------------------------------------------------------
// Row mappers (SeaORM fetch rows, decoded by name)
// ---------------------------------------------------------------------------

/// Map a SeaORM failure onto the store error, keeping the sqlx text the
/// old readers produced.
fn sea_db(error: DbErr) -> CreationError {
    CreationError::Db(orm_tx::into_sqlx(error).to_string())
}

/// The store error for a missed `fetch_one` (the old row readers errored
/// when the row was absent instead of returning `None`).
fn row_missing() -> CreationError {
    CreationError::Db(sqlx::Error::RowNotFound.to_string())
}

/// The `ISSUE_SELECT_SQL` projection, decoded by name.
#[derive(Debug, FromQueryResult)]
struct IssueRow {
    id: Uuid,
    workspace_id: Uuid,
    project_id: Uuid,
    state_id: Option<Uuid>,
    parent_id: Option<Uuid>,
    created_by_id: Option<Uuid>,
    assigned_pod_id: Option<Uuid>,
    agent_executor: Option<String>,
    git_work_branch: String,
    workpad: String,
    name: String,
    description_stripped: Option<String>,
    priority: String,
    sequence_id: i32,
    target_date: Option<chrono::NaiveDate>,
}

impl From<IssueRow> for IssueView {
    fn from(row: IssueRow) -> Self {
        Self {
            id: row.id,
            workspace_id: row.workspace_id,
            project_id: Some(row.project_id),
            state_id: row.state_id,
            parent_id: row.parent_id,
            created_by_id: row.created_by_id,
            assigned_pod_id: row.assigned_pod_id,
            agent_executor: row.agent_executor,
            git_work_branch: Some(row.git_work_branch),
            workpad: Some(row.workpad),
            name: Some(row.name),
            description_stripped: row.description_stripped,
            priority: Some(row.priority),
            sequence_id: row.sequence_id,
            target_date: row.target_date,
        }
    }
}

/// The `PROJECT_SELECT_SQL` projection, decoded by name.
#[derive(Debug, FromQueryResult)]
struct ProjectRow {
    id: Uuid,
    workspace_id: Uuid,
    identifier: String,
    name: String,
    description: String,
    repo_url: String,
    base_branch: String,
    default_agent_executor: String,
    project_lead_id: Option<Uuid>,
    default_assignee_id: Option<Uuid>,
    agent_default_max_ticks: i32,
    agent_default_interval_seconds: i32,
    agent_review_default_interval_seconds: i32,
    agent_test_default_interval_seconds: i32,
}

impl From<ProjectRow> for ProjectView {
    fn from(row: ProjectRow) -> Self {
        Self {
            id: row.id,
            workspace_id: row.workspace_id,
            identifier: row.identifier,
            name: row.name,
            description: Some(row.description),
            repo_url: Some(row.repo_url),
            base_branch: Some(row.base_branch),
            default_agent_executor: row.default_agent_executor,
            project_lead_id: row.project_lead_id,
            default_assignee_id: row.default_assignee_id,
            pool: i64::from(row.agent_default_max_ticks),
            interval_impl: i64::from(row.agent_default_interval_seconds),
            interval_review: i64::from(row.agent_review_default_interval_seconds),
            interval_test: i64::from(row.agent_test_default_interval_seconds),
        }
    }
}

/// The consumed `agent_run` projection (`RUN_VIEW_COLUMNS`), decoded by
/// name.
#[derive(Debug, FromQueryResult)]
struct RunRow {
    id: Uuid,
    workspace_id: Uuid,
    created_by_id: Uuid,
    pod_id: Uuid,
    runner_id: Option<Uuid>,
    pinned_runner_id: Option<Uuid>,
    parent_run_id: Option<Uuid>,
    work_item_id: Option<Uuid>,
    status: String,
    trigger: String,
    executor_kind: String,
    phase_kind: String,
    run_config: Value,
    tool_plan: Value,
    error_details: Value,
    prompt: String,
    prompt_manifest: Option<Value>,
    ended_at: Option<sea_orm::prelude::DateTimeWithTimeZone>,
}

fn issue_columns() -> Vec<issues::Column> {
    vec![
        issues::Column::Id,
        issues::Column::WorkspaceId,
        issues::Column::ProjectId,
        issues::Column::StateId,
        issues::Column::ParentId,
        issues::Column::CreatedById,
        issues::Column::AssignedPodId,
        issues::Column::AgentExecutor,
        issues::Column::GitWorkBranch,
        issues::Column::Workpad,
        issues::Column::Name,
        issues::Column::DescriptionStripped,
        issues::Column::Priority,
        issues::Column::SequenceId,
        issues::Column::TargetDate,
    ]
}

fn project_columns() -> Vec<projects::Column> {
    vec![
        projects::Column::Id,
        projects::Column::WorkspaceId,
        projects::Column::Identifier,
        projects::Column::Name,
        projects::Column::Description,
        projects::Column::RepoUrl,
        projects::Column::BaseBranch,
        projects::Column::DefaultAgentExecutor,
        projects::Column::ProjectLeadId,
        projects::Column::DefaultAssigneeId,
        projects::Column::AgentDefaultMaxTicks,
        projects::Column::AgentDefaultIntervalSeconds,
        projects::Column::AgentReviewDefaultIntervalSeconds,
        projects::Column::AgentTestDefaultIntervalSeconds,
    ]
}

fn run_columns() -> Vec<agent_run::Column> {
    vec![
        agent_run::Column::Id,
        agent_run::Column::WorkspaceId,
        agent_run::Column::CreatedById,
        agent_run::Column::PodId,
        agent_run::Column::RunnerId,
        agent_run::Column::PinnedRunnerId,
        agent_run::Column::ParentRunId,
        agent_run::Column::WorkItemId,
        agent_run::Column::Status,
        agent_run::Column::Trigger,
        agent_run::Column::ExecutorKind,
        agent_run::Column::PhaseKind,
        agent_run::Column::RunConfig,
        agent_run::Column::ToolPlan,
        agent_run::Column::ErrorDetails,
        agent_run::Column::Prompt,
        agent_run::Column::PromptManifest,
        agent_run::Column::EndedAt,
    ]
}

/// Unknown stored statuses/executors are unreachable through the checks
/// but keep the old strict decode (a decode failure, not a silent map).
fn map_run_row(row: RunRow) -> Result<RunView, CreationError> {
    let status = AgentRunStatus::from_value(&row.status)
        .ok_or_else(|| CreationError::Db(format!("unknown agent_run.status {:?}", row.status)))?;
    let executor_kind = AgentExecutorKind::from_value(&row.executor_kind).ok_or_else(|| {
        CreationError::Db(format!(
            "unknown agent_run.executor_kind {:?}",
            row.executor_kind
        ))
    })?;
    Ok(RunView {
        id: row.id,
        workspace_id: row.workspace_id,
        created_by_id: row.created_by_id,
        pod_id: row.pod_id,
        runner_id: row.runner_id,
        pinned_runner_id: row.pinned_runner_id,
        parent_run_id: row.parent_run_id,
        work_item_id: row.work_item_id,
        status,
        trigger: row.trigger,
        executor_kind,
        phase_kind: row.phase_kind,
        run_config: row.run_config,
        tool_plan: row.tool_plan,
        error_details: row.error_details,
        prompt: row.prompt,
        prompt_manifest: row.prompt_manifest,
        ended_at: row.ended_at.map(|at| at.with_timezone(&Utc)),
    })
}

/// Map an inserted full row onto the view (the `RETURNING` projection of
/// the old insert, same strict status/executor decode).
fn map_run_model(model: agent_run::Model) -> Result<RunView, CreationError> {
    let status = AgentRunStatus::from_value(&model.status)
        .ok_or_else(|| CreationError::Db(format!("unknown agent_run.status {:?}", model.status)))?;
    let executor_kind = AgentExecutorKind::from_value(&model.executor_kind).ok_or_else(|| {
        CreationError::Db(format!(
            "unknown agent_run.executor_kind {:?}",
            model.executor_kind
        ))
    })?;
    Ok(RunView {
        id: model.id,
        workspace_id: model.workspace_id,
        created_by_id: model.created_by_id,
        pod_id: model.pod_id,
        runner_id: model.runner_id,
        pinned_runner_id: model.pinned_runner_id,
        parent_run_id: model.parent_run_id,
        work_item_id: model.work_item_id,
        status,
        trigger: model.trigger,
        executor_kind,
        phase_kind: model.phase_kind,
        run_config: model.run_config,
        tool_plan: model.tool_plan,
        error_details: model.error_details,
        prompt: model.prompt,
        prompt_manifest: model.prompt_manifest,
        ended_at: model.ended_at.map(|at| at.with_timezone(&Utc)),
    })
}

/// The `RUNNER_SELECT_SQL` projection, decoded by name.
#[derive(Debug, FromQueryResult)]
struct RunnerRow {
    id: Uuid,
    pod_id: Uuid,
    status: String,
    last_heartbeat_at: Option<sea_orm::prelude::DateTimeWithTimeZone>,
    paused_at: Option<sea_orm::prelude::DateTimeWithTimeZone>,
    provisioning: String,
    pod_dispatch: String,
    dev_machine_id: Option<Uuid>,
}

/// The `users.is_active/is_bot` flags, decoded by name.
#[derive(Debug, FromQueryResult)]
struct UserFlagsRow {
    is_active: bool,
    is_bot: bool,
}

/// The `pod` projections, decoded by name.
#[derive(Debug, FromQueryResult)]
struct PodRow {
    id: Uuid,
    project_id: Uuid,
}

// ---------------------------------------------------------------------------
// Seam implementation
// ---------------------------------------------------------------------------

impl<C, H, P> CreationSeam for LiveCreationStore<'_, C, H, P>
where
    C: AdmissionCache,
    H: Fn(Uuid) -> bool + Clone + Send + Sync,
    P: Fn(Option<Uuid>) -> LlmProfile + Clone + Send + Sync,
{
    async fn issue(&mut self, issue_id: Uuid) -> Result<IssueView, CreationError> {
        let row = {
            let conn = TxConn::new(self.tx.inner());
            issues::Entity::find()
                .select_only()
                .columns(issue_columns())
                .filter(issues::Column::Id.eq(issue_id))
                .into_model::<IssueRow>()
                .one(&conn)
                .await
                .map_err(sea_db)?
        };
        row.map(IssueView::from).ok_or_else(row_missing)
    }

    async fn project(&mut self, project_id: Uuid) -> Result<ProjectView, CreationError> {
        let row = {
            let conn = TxConn::new(self.tx.inner());
            projects::Entity::find()
                .select_only()
                .columns(project_columns())
                .filter(projects::Column::Id.eq(project_id))
                .into_model::<ProjectRow>()
                .one(&conn)
                .await
                .map_err(sea_db)?
        };
        row.map(ProjectView::from).ok_or_else(row_missing)
    }

    async fn state(&mut self, state_id: Option<Uuid>) -> Result<Option<StateView>, CreationError> {
        let Some(state_id) = state_id else {
            return Ok(None);
        };
        let found = {
            let conn = TxConn::new(self.tx.inner());
            state_name_group(&conn, state_id).await.map_err(sea_db)?
        };
        match found {
            Some((name, group)) => Ok(Some(StateView {
                id: state_id,
                name,
                group,
            })),
            None => Err(CreationError::MissingRow(format!("no state {state_id}"))),
        }
    }

    async fn latest_prior_run(&mut self, issue_id: Uuid) -> Result<Option<RunView>, CreationError> {
        let row = {
            let conn = TxConn::new(self.tx.inner());
            agent_run::Entity::find()
                .select_only()
                .columns(run_columns())
                .filter(agent_run::Column::WorkItemId.eq(issue_id))
                .order_by_desc(agent_run::Column::CreatedAt)
                .limit(1)
                .into_model::<RunRow>()
                .one(&conn)
                .await
                .map_err(sea_db)?
        };
        row.map(map_run_row).transpose()
    }

    async fn active_run_for(&mut self, issue_id: Uuid) -> Result<Option<RunView>, CreationError> {
        let statuses: Vec<&str> = ACTIVE_STATUSES.iter().map(|s| s.value()).collect();
        let row = {
            let conn = TxConn::new(self.tx.inner());
            agent_run::Entity::find()
                .select_only()
                .columns(run_columns())
                .filter(agent_run::Column::WorkItemId.eq(issue_id))
                .filter(agent_run::Column::Status.is_in(statuses))
                .order_by_desc(agent_run::Column::CreatedAt)
                .limit(1)
                .into_model::<RunRow>()
                .one(&conn)
                .await
                .map_err(sea_db)?
        };
        row.map(map_run_row).transpose()
    }

    async fn run(&mut self, run_id: Uuid) -> Result<Option<RunView>, CreationError> {
        let row = {
            let conn = TxConn::new(self.tx.inner());
            agent_run::Entity::find()
                .select_only()
                .columns(run_columns())
                .filter(agent_run::Column::Id.eq(run_id))
                .into_model::<RunRow>()
                .one(&conn)
                .await
                .map_err(sea_db)?
        };
        row.map(map_run_row).transpose()
    }

    async fn runner(&mut self, runner_id: Uuid) -> Result<Option<RunnerView>, CreationError> {
        let (row, engine_paused) = {
            let conn = TxConn::new(self.tx.inner());
            let row = runner::Entity::find()
                .select_only()
                .columns([
                    runner::Column::Id,
                    runner::Column::PodId,
                    runner::Column::Status,
                    runner::Column::LastHeartbeatAt,
                    runner::Column::PausedAt,
                    runner::Column::Provisioning,
                    runner::Column::PodDispatch,
                    runner::Column::DevMachineId,
                ])
                .filter(runner::Column::Id.eq(runner_id))
                .into_model::<RunnerRow>()
                .one(&conn)
                .await
                .map_err(sea_db)?;
            let engine_paused = match row.as_ref().and_then(|r| r.dev_machine_id) {
                Some(machine_id) => {
                    let machine = dev_machine::Entity::find_by_id(machine_id)
                        .one(&conn)
                        .await
                        .map_err(sea_db)?;
                    machine.is_some_and(|m| {
                        m.engine_paused_by_owner || !m.engine_auto_pause_reason.is_empty()
                    })
                }
                None => false,
            };
            (row, engine_paused)
        };
        Ok(row.map(|row| RunnerView {
            id: row.id,
            pod_id: row.pod_id,
            status: row.status,
            last_heartbeat_at: row.last_heartbeat_at.map(|at| at.with_timezone(&Utc)),
            paused_at: row.paused_at.map(|at| at.with_timezone(&Utc)),
            provisioning: row.provisioning,
            pod_dispatch: row.pod_dispatch,
            engine_paused,
        }))
    }

    async fn assigned_pod(&mut self, pod_id: Uuid) -> Result<Option<PodView>, CreationError> {
        let row = {
            let conn = TxConn::new(self.tx.inner());
            pod::Entity::find()
                .select_only()
                .columns([pod::Column::Id, pod::Column::ProjectId])
                .filter(pod::Column::Id.eq(pod_id))
                .filter(pod::Column::DeletedAt.is_null())
                .into_model::<PodRow>()
                .one(&conn)
                .await
                .map_err(sea_db)?
        };
        Ok(row.map(|row| PodView {
            id: row.id,
            project_id: row.project_id,
        }))
    }

    async fn default_pod_for_project(
        &mut self,
        project_id: Uuid,
    ) -> Result<Option<PodView>, CreationError> {
        let row = {
            let conn = TxConn::new(self.tx.inner());
            pod::Entity::find()
                .select_only()
                .columns([pod::Column::Id, pod::Column::ProjectId])
                .filter(pod::Column::ProjectId.eq(project_id))
                .filter(pod::Column::IsDefault.eq(true))
                .filter(pod::Column::DeletedAt.is_null())
                .order_by_asc(pod::Column::Id)
                .limit(1)
                .into_model::<PodRow>()
                .one(&conn)
                .await
                .map_err(sea_db)?
        };
        Ok(row.map(|row| PodView {
            id: row.id,
            project_id: row.project_id,
        }))
    }

    async fn resume_parent_run_id(
        &mut self,
        issue_id: Uuid,
    ) -> Result<Option<Uuid>, CreationError> {
        let row = {
            let conn = TxConn::new(self.tx.inner());
            issue_agent_ticker::Entity::find()
                .select_only()
                .column(issue_agent_ticker::Column::ResumeParentRunId)
                .filter(issue_agent_ticker::Column::IssueId.eq(issue_id))
                .into_tuple::<Option<Uuid>>()
                .one(&conn)
                .await
                .map_err(sea_db)?
        };
        Ok(row.flatten())
    }

    async fn work_item_id_for_run(&mut self, run_id: Uuid) -> Result<Option<Uuid>, CreationError> {
        let row = {
            let conn = TxConn::new(self.tx.inner());
            agent_run::Entity::find()
                .select_only()
                .column(agent_run::Column::WorkItemId)
                .filter(agent_run::Column::Id.eq(run_id))
                .into_tuple::<Option<Uuid>>()
                .one(&conn)
                .await
                .map_err(sea_db)?
        };
        Ok(row.flatten())
    }

    async fn lock_issue_for_handoff(
        &mut self,
        issue_id: Uuid,
    ) -> Result<Option<LockedIssue>, CreationError> {
        let locked = {
            let conn = TxConn::new(self.tx.inner());
            issues::Entity::find_by_id(issue_id)
                .lock_exclusive()
                .one(&conn)
                .await
                .map_err(sea_db)?
        };
        if locked.is_none() {
            return Ok(None);
        }
        let issue = self.issue(issue_id).await?;
        let project_id = issue
            .project_id
            .ok_or_else(|| CreationError::MissingRow("issue has no project".to_owned()))?;
        let project = self.project(project_id).await?;
        let state = self.state(issue.state_id).await?;
        Ok(Some(LockedIssue {
            issue,
            project,
            state,
        }))
    }

    async fn lock_run_for_handoff(
        &mut self,
        run_id: Uuid,
    ) -> Result<Option<RunView>, CreationError> {
        let row = {
            let conn = TxConn::new(self.tx.inner());
            agent_run::Entity::find()
                .select_only()
                .columns(run_columns())
                .filter(agent_run::Column::Id.eq(run_id))
                .lock_exclusive()
                .into_model::<RunRow>()
                .one(&conn)
                .await
                .map_err(sea_db)?
        };
        row.map(map_run_row).transpose()
    }

    async fn user_flags(&mut self, user_id: Uuid) -> Result<UserFlags, CreationError> {
        let row = {
            let conn = TxConn::new(self.tx.inner());
            users::Entity::find()
                .select_only()
                .columns([users::Column::IsActive, users::Column::IsBot])
                .filter(users::Column::Id.eq(user_id))
                .into_model::<UserFlagsRow>()
                .one(&conn)
                .await
                .map_err(sea_db)?
        };
        let row = row.ok_or_else(row_missing)?;
        Ok(UserFlags {
            is_active: row.is_active,
            is_bot: row.is_bot,
        })
    }

    async fn insert_run(&mut self, row: &NewAgentRun) -> Result<RunView, CreationError> {
        let now: sea_orm::prelude::DateTimeWithTimeZone = row.now.into();
        let model = agent_run::ActiveModel {
            id: Set(row.id),
            status: Set("queued".to_owned()),
            prompt: Set(String::new()),
            run_config: Set(row.run_config.clone()),
            required_capabilities: Set(serde_json::json!([])),
            thread_id: Set(String::new()),
            lease_expires_at: Set(None),
            done_payload: Set(None),
            created_at: Set(now),
            assigned_at: Set(None),
            started_at: Set(None),
            ended_at: Set(None),
            owner_id: Set(None),
            workspace_id: Set(row.workspace_id),
            runner_id: Set(None),
            work_item_id: Set(Some(row.work_item_id)),
            parent_run_id: Set(row.parent_run_id),
            pod_id: Set(row.pod_id),
            created_by_id: Set(row.created_by_id),
            pinned_runner_id: Set(row.pinned_runner_id),
            scheduler_binding_id: Set(None),
            llm_model: Set(String::new()),
            trigger: Set(row.trigger.clone()),
            prompt_manifest: Set(None),
            executor_kind: Set(row.executor_kind.value().to_owned()),
            dispatch_attempts: Set(0),
            cancel_requested_at: Set(None),
            cancel_reason: Set(String::new()),
            tool_plan: Set(row.tool_plan.clone()),
            terminal_hooks_applied_at: Set(None),
            terminal_capacity_released_at: Set(None),
            phase_kind: Set(row.phase_kind.clone()),
            agent_metadata: Set(serde_json::json!({})),
            usage: Set(serde_json::json!({})),
            // Generated columns (the `JSONKey*Field`s): left out of the
            // insert, exactly like the old `RUN_INSERT_SQL`.
            input_tokens: ActiveValue::NotSet,
            output_tokens: ActiveValue::NotSet,
            total_tokens: ActiveValue::NotSet,
            error_details: Set(row.error_details.clone()),
            refusal_category: ActiveValue::NotSet,
        };
        let inserted = {
            let conn = TxConn::new(self.tx.inner());
            model.insert(&conn).await.map_err(sea_db)?
        };
        map_run_model(inserted)
    }

    async fn save_prompt(
        &mut self,
        run_id: Uuid,
        prompt: &str,
        manifest: &Value,
    ) -> Result<(), CreationError> {
        let conn = TxConn::new(self.tx.inner());
        agent_run::Entity::update_many()
            .col_expr(agent_run::Column::Prompt, Expr::value(prompt.to_owned()))
            .col_expr(
                agent_run::Column::PromptManifest,
                Expr::value(manifest.clone()),
            )
            .filter(agent_run::Column::Id.eq(run_id))
            .exec(&conn)
            .await
            .map_err(sea_db)?;
        Ok(())
    }

    async fn save_run_config(&mut self, run_id: Uuid, config: &Value) -> Result<(), CreationError> {
        let conn = TxConn::new(self.tx.inner());
        agent_run::Entity::update_many()
            .col_expr(agent_run::Column::RunConfig, Expr::value(config.clone()))
            .filter(agent_run::Column::Id.eq(run_id))
            .exec(&conn)
            .await
            .map_err(sea_db)?;
        Ok(())
    }

    async fn execution_fields(
        &mut self,
        req: &ExecutionRequest,
    ) -> Result<ServiceExecutionFields, ExecutionError> {
        let project = ProjectScope {
            project_id: req.project_id,
            workspace_id: req.workspace_id,
            default_agent_executor: req.default_agent_executor.clone(),
        };
        let actor = req.actor.map(|actor| ActorScope {
            id: actor.id,
            flags: actor.flags,
        });
        let actor_id = actor.as_ref().map(|actor| actor.id);
        // Split borrows: the short transaction borrows the pool only,
        // so the outbox + delegation inputs stay usable.
        let Self {
            pool,
            cache,
            cloud,
            managed,
            has_usable_llm_config,
            toolsets,
            now_unix_secs,
            deferred,
            ..
        } = self;
        let mut short = Transaction::begin(pool)
            .await
            .map_err(|error| ExecutionError::Store(CreationError::Db(error.to_string())))?;
        let inputs = ExecutionInputs {
            project: &project,
            run_kind: &req.run_kind,
            has_issue: req.has_issue,
            required_capabilities: &[],
            actor: actor.as_ref(),
            automatic: req.automatic,
            requested: req.requested.as_deref(),
            now_unix_secs: *now_unix_secs,
        };
        let has_llm = has_usable_llm_config.clone();
        let toolsets = toolsets.clone();
        let mut collect = |consume: DeferredConsume| deferred.push(consume);
        let fields = resolve_execution_fields(
            &mut short,
            &inputs,
            cloud,
            managed,
            cache,
            &mut collect,
            ExecutionSeams {
                has_usable_llm_config: move || actor_id.is_some_and(&has_llm),
                toolsets,
            },
        )
        .await;
        short
            .commit()
            .await
            .map_err(|error| ExecutionError::Store(CreationError::Db(error.to_string())))?;
        fields
            .map(map_execution_fields)
            .map_err(map_execution_error)
    }

    async fn lock_cloud_creation_capacity(
        &mut self,
        workspace_id: Uuid,
        executor_kind: AgentExecutorKind,
        automatic: bool,
    ) -> Result<Option<ServiceAdmissionError>, CreationError> {
        let outcome = lock_capacity(
            &mut self.tx,
            workspace_id,
            &executor_kind,
            automatic,
            &self.cloud,
        )
        .await;
        match outcome {
            Ok(deferred) => Ok(deferred.map(map_admission_error)),
            Err(ExecutionFieldsError::Admission(exc)) => {
                Err(CreationError::CapacityRefused(exc.detail().to_owned()))
            }
            Err(ExecutionFieldsError::Db(error)) => Err(Self::db(error)),
            // Unreachable: the lock path only locks, counts and
            // verdicts (no resolve/LLM/tool gates).
            Err(other) => Err(CreationError::Db(format!("unexpected lock error: {other}"))),
        }
    }

    fn dispatch_after_commit(&mut self, run_id: Uuid) {
        collect_dispatch(&mut |id| self.dispatches.push(id), run_id);
    }

    async fn render_bundle(
        &mut self,
        issue_id: Uuid,
        run_id: Uuid,
        parent_run_id: Option<Uuid>,
        trigger: &str,
        created_by_id: Uuid,
    ) -> Result<RenderBundle, CreationError> {
        load_render_bundle(
            &mut self.tx,
            issue_id,
            run_id,
            parent_run_id,
            trigger,
            created_by_id,
        )
        .await
    }

    fn extra_toolsets_schema_tool(&self) -> String {
        self.toolsets.schema_tool_name().to_owned()
    }
}

/// Map one D-11 fields struct onto the services mirror. Key presence
/// follows the executor (`creation.py` shapes): cloud always carries
/// `pinned_runner: None`, managed carries the pick, local carries no
/// key.
fn map_execution_fields(fields: crate::dispatch::ExecutionFields) -> ServiceExecutionFields {
    let pinned_runner_entry = match fields.executor_kind {
        AgentExecutorKind::CloudAgent => Some(None),
        AgentExecutorKind::ManagedRunner => Some(fields.pinned_runner_id),
        AgentExecutorKind::LocalRunner => None,
    };
    ServiceExecutionFields {
        executor_kind: fields.executor_kind,
        tool_plan: fields.tool_plan,
        pinned_runner_entry,
        error_code: fields.error_code,
        cloud_admission_error: fields.cloud_admission_error.map(map_admission_error),
    }
}

/// Map one D-11 failure: `str(exc)` for the caught tuple, store error
/// for the database (which propagates).
fn map_execution_error(error: ExecutionFieldsError) -> ExecutionError {
    match error {
        ExecutionFieldsError::Db(error) => {
            ExecutionError::Store(CreationError::Db(error.to_string()))
        }
        ExecutionFieldsError::Admission(exc) => ExecutionError::Refused(exc.detail().to_owned()),
        other => ExecutionError::Refused(other.to_string()),
    }
}

fn map_admission_error(error: DeferredAdmissionError) -> ServiceAdmissionError {
    ServiceAdmissionError {
        code: error.code,
        detail: error.detail,
    }
}

// ---------------------------------------------------------------------------
// Render-bundle loader (mirrors the api prompt-preview fetch side)
// ---------------------------------------------------------------------------

/// `issue.labels`: the M2M join through the label's live default manager,
/// newest first. Soft-deleted links still render (no link-table guard).
#[derive(Debug, FromQueryResult)]
struct LabelNameRow {
    name: String,
}

fn labels_query(issue_id: Uuid) -> sea_orm::Select<issue_labels::Entity> {
    issue_labels::Entity::find()
        .select_only()
        .column_as(labels::Column::Name, "name")
        .join(JoinType::InnerJoin, issue_labels::Relation::Labels.def())
        .filter(issue_labels::Column::IssueId.eq(issue_id))
        .filter(labels::Column::DeletedAt.is_null())
        .order_by_desc(labels::Column::CreatedAt)
}

async fn bundle_labels(
    tx: &mut Transaction<'_>,
    issue_id: Uuid,
) -> Result<Vec<String>, CreationError> {
    let rows = {
        let conn = TxConn::new(tx.inner());
        labels_query(issue_id)
            .into_model::<LabelNameRow>()
            .all(&conn)
            .await
            .map_err(sea_db)?
    };
    Ok(rows.into_iter().map(|row| row.name).collect())
}

/// `issue.assignees`: display name / email pairs, newest user first.
#[derive(Debug, FromQueryResult)]
struct AssigneeRow {
    display_name: String,
    email: Option<String>,
}

fn assignees_query(issue_id: Uuid) -> sea_orm::Select<issue_assignees::Entity> {
    issue_assignees::Entity::find()
        .select_only()
        .column_as(users::Column::DisplayName, "display_name")
        .column_as(users::Column::Email, "email")
        .join(JoinType::InnerJoin, issue_assignees::Relation::Users3.def())
        .filter(issue_assignees::Column::IssueId.eq(issue_id))
        .order_by_desc(users::Column::CreatedAt)
}

async fn bundle_assignees(
    tx: &mut Transaction<'_>,
    issue_id: Uuid,
) -> Result<Vec<String>, CreationError> {
    let rows = {
        let conn = TxConn::new(tx.inner());
        assignees_query(issue_id)
            .into_model::<AssigneeRow>()
            .all(&conn)
            .await
            .map_err(sea_db)?
    };
    Ok(rows
        .into_iter()
        .map(|row| context::assignee_display(Some(&row.display_name), row.email.as_deref()))
        .collect())
}

/// `State.objects.filter(project=…)`: triage excluded, `sequence` order.
#[derive(Debug, FromQueryResult)]
struct ProjectStateRow {
    name: String,
    group: String,
    description: String,
}

async fn bundle_project_states(
    tx: &mut Transaction<'_>,
    project_id: Uuid,
) -> Result<Vec<context::ProjectStateView>, CreationError> {
    let rows = {
        let conn = TxConn::new(tx.inner());
        states::Entity::find()
            .select_only()
            .columns([
                states::Column::Name,
                states::Column::Group,
                states::Column::Description,
            ])
            .filter(states::Column::ProjectId.eq(project_id))
            .filter(states::Column::DeletedAt.is_null())
            .filter(states::Column::Group.ne("triage"))
            .order_by_asc(states::Column::Sequence)
            .into_model::<ProjectStateRow>()
            .all(&conn)
            .await
            .map_err(sea_db)?
    };
    Ok(rows
        .into_iter()
        .map(|row| context::ProjectStateView {
            name: row.name,
            group: row.group,
            description: Some(row.description),
        })
        .collect())
}

/// Direct children: live, non-triage, non-archived, non-draft, oldest
/// first. Joined columns are aliased so every output name is unique.
#[derive(Debug, FromQueryResult)]
struct ChildRow {
    issue_name: String,
    issue_sequence_id: i32,
    project_identifier: String,
    state_name: Option<String>,
}

fn children_query(issue_id: Uuid) -> sea_orm::Select<issues::Entity> {
    issues::Entity::find()
        .select_only()
        .column_as(issues::Column::Name, "issue_name")
        .column_as(issues::Column::SequenceId, "issue_sequence_id")
        .column_as(projects::Column::Identifier, "project_identifier")
        .column_as(states::Column::Name, "state_name")
        .join(JoinType::InnerJoin, issues::Relation::Projects.def())
        .join(JoinType::LeftJoin, issues::Relation::States.def())
        .filter(issues::Column::ParentId.eq(issue_id))
        .filter(issues::Column::DeletedAt.is_null())
        .filter(states::Column::Group.ne("triage"))
        .filter(issues::Column::ArchivedAt.is_null())
        .filter(projects::Column::ArchivedAt.is_null())
        .filter(issues::Column::IsDraft.eq(false))
        .order_by_asc(issues::Column::CreatedAt)
}

async fn bundle_children(
    tx: &mut Transaction<'_>,
    issue_id: Uuid,
) -> Result<Vec<context::IssueRef>, CreationError> {
    let rows = {
        let conn = TxConn::new(tx.inner());
        children_query(issue_id)
            .into_model::<ChildRow>()
            .all(&conn)
            .await
            .map_err(sea_db)?
    };
    Ok(rows
        .into_iter()
        .map(|row| context::IssueRef {
            identifier: context::issue_identifier(
                &row.project_identifier,
                i64::from(row.issue_sequence_id),
            ),
            title: row.issue_name,
            state: row.state_name.unwrap_or_default(),
        })
        .collect())
}

async fn bundle_relations(
    tx: &mut Transaction<'_>,
    issue_id: Uuid,
) -> Result<
    (
        Vec<context::RelationRow>,
        HashMap<String, context::IssueRef>,
        HashMap<String, context::DirectionalRef>,
    ),
    CreationError,
> {
    let rows = {
        let conn = TxConn::new(tx.inner());
        issue_relations::Entity::find()
            .select_only()
            .columns([
                issue_relations::Column::IssueId,
                issue_relations::Column::RelatedIssueId,
                issue_relations::Column::RelationType,
            ])
            .filter(
                Condition::any()
                    .add(issue_relations::Column::IssueId.eq(issue_id))
                    .add(issue_relations::Column::RelatedIssueId.eq(issue_id)),
            )
            .filter(issue_relations::Column::DeletedAt.is_null())
            .order_by_desc(issue_relations::Column::CreatedAt)
            .into_model::<RelationRow>()
            .all(&conn)
            .await
            .map_err(sea_db)?
    };
    // The other end of each row, deduplicated, self-links dropped.
    let mut other_ids: Vec<Uuid> = Vec::new();
    let mut seen_ids = HashSet::new();
    for row in &rows {
        let other = if row.issue_id == issue_id {
            row.related_issue_id
        } else {
            row.issue_id
        };
        if other != issue_id && seen_ids.insert(other) {
            other_ids.push(other);
        }
    }
    let targets: Vec<TargetRow> = if other_ids.is_empty() {
        Vec::new()
    } else {
        let conn = TxConn::new(tx.inner());
        issues::Entity::find()
            .select_only()
            .column_as(issues::Column::Id, "issue_id")
            .column_as(issues::Column::Name, "issue_name")
            .column_as(issues::Column::SequenceId, "issue_sequence_id")
            .column_as(projects::Column::Identifier, "project_identifier")
            .column_as(states::Column::Name, "state_name")
            .column_as(states::Column::Group, "state_group")
            .join(JoinType::InnerJoin, issues::Relation::Projects.def())
            .join(JoinType::LeftJoin, issues::Relation::States.def())
            .filter(issues::Column::Id.is_in(other_ids))
            .filter(issues::Column::DeletedAt.is_null())
            .into_model::<TargetRow>()
            .all(&conn)
            .await
            .map_err(sea_db)?
    };
    let mut refs = HashMap::new();
    let mut directional_refs = HashMap::new();
    for target in targets {
        let key = target.issue_id.to_string();
        let title = target.issue_name;
        let state_name = target.state_name.unwrap_or_default();
        let sequence = target.issue_sequence_id;
        let project_identifier = target.project_identifier;
        let group = target.state_group;
        refs.insert(
            key.clone(),
            context::IssueRef {
                identifier: context::issue_identifier(&project_identifier, i64::from(sequence)),
                title: title.clone(),
                state: state_name.clone(),
            },
        );
        directional_refs.insert(
            key,
            context::DirectionalRef {
                identifier: context::issue_identifier(&project_identifier, i64::from(sequence)),
                title,
                state: state_name,
                state_group: group.unwrap_or_default(),
            },
        );
    }
    let relation_rows = rows
        .into_iter()
        .map(|row| context::RelationRow {
            issue_id: row.issue_id.to_string(),
            related_issue_id: row.related_issue_id.to_string(),
            relation_type: row.relation_type,
        })
        .collect();
    Ok((relation_rows, refs, directional_refs))
}

/// Relation rows, both directions, newest first.
#[derive(Debug, FromQueryResult)]
struct RelationRow {
    issue_id: Uuid,
    related_issue_id: Uuid,
    relation_type: String,
}

/// Relation targets: live issues with their own project's identifier.
#[derive(Debug, FromQueryResult)]
struct TargetRow {
    issue_id: Uuid,
    issue_name: String,
    issue_sequence_id: i32,
    project_identifier: String,
    state_name: Option<String>,
    state_group: Option<String>,
}

/// Unfolded comments, chronological, with the actor's columns from a
/// LEFT join (all `None` when the actor is gone).
#[derive(Debug, FromQueryResult)]
struct CommentRow {
    stripped: String,
    speaker_type: String,
    speaker_label: String,
    run_id: Option<Uuid>,
    created_at: sea_orm::prelude::DateTimeWithTimeZone,
    display_name: Option<String>,
    email: Option<String>,
    username: Option<String>,
    is_bot: Option<bool>,
    labels: Vec<String>,
}

fn comments_query(issue_id: Uuid) -> sea_orm::Select<issue_comments::Entity> {
    issue_comments::Entity::find()
        .select_only()
        .column_as(issue_comments::Column::CommentStripped, "stripped")
        .column_as(issue_comments::Column::SpeakerType, "speaker_type")
        .column_as(issue_comments::Column::SpeakerLabel, "speaker_label")
        .column_as(issue_comments::Column::SpeakerAgentRunId, "run_id")
        .column_as(issue_comments::Column::CreatedAt, "created_at")
        .column_as(users::Column::DisplayName, "display_name")
        .column_as(users::Column::Email, "email")
        .column_as(users::Column::Username, "username")
        .column_as(users::Column::IsBot, "is_bot")
        .column_as(issue_comments::Column::Labels, "labels")
        .join(JoinType::LeftJoin, issue_comments::Relation::Users3.def())
        .filter(issue_comments::Column::IssueId.eq(issue_id))
        .filter(issue_comments::Column::DeletedAt.is_null())
        .order_by_asc(issue_comments::Column::CreatedAt)
}

async fn bundle_comments(
    tx: &mut Transaction<'_>,
    issue_id: Uuid,
) -> Result<Vec<context::CommentView>, CreationError> {
    let rows = {
        let conn = TxConn::new(tx.inner());
        comments_query(issue_id)
            .into_model::<CommentRow>()
            .all(&conn)
            .await
            .map_err(sea_db)?
    };
    Ok(rows
        .into_iter()
        // `fold`-labeled rows excluded (`labels @> ARRAY['fold']`).
        .filter(|row| !row.labels.iter().any(|label| label == "fold"))
        .map(|row| {
            let actor = match (&row.display_name, &row.email, &row.username, &row.is_bot) {
                (None, None, None, None) => None,
                (display, email, username, is_bot) => Some(context::ActorView {
                    display_name: display.clone(),
                    email: email.clone(),
                    username: username.clone(),
                    is_bot: is_bot.unwrap_or(false),
                }),
            };
            context::CommentView {
                body: row.stripped,
                speaker_type: row.speaker_type,
                speaker_label: Some(row.speaker_label),
                actor,
                created_at_iso: Some(render_isoformat(row.created_at.with_timezone(&Utc))),
                run_id: row.run_id.map(|id| id.to_string()),
            }
        })
        .collect())
}

/// Attached review links, newest first.
#[derive(Debug, FromQueryResult)]
struct ReviewRow {
    url: String,
    title: String,
    state: String,
    merged: bool,
    draft: bool,
    provider: String,
    external_iid: String,
}

async fn bundle_reviews(
    tx: &mut Transaction<'_>,
    issue_id: Uuid,
) -> Result<Vec<context::CodeReviewView>, CreationError> {
    let rows = {
        let conn = TxConn::new(tx.inner());
        git_code_review_links::Entity::find()
            .select_only()
            .columns([
                git_code_review_links::Column::Url,
                git_code_review_links::Column::Title,
                git_code_review_links::Column::State,
                git_code_review_links::Column::Merged,
                git_code_review_links::Column::Draft,
                git_code_review_links::Column::Provider,
                git_code_review_links::Column::ExternalIid,
            ])
            .filter(git_code_review_links::Column::IssueId.eq(issue_id))
            .filter(git_code_review_links::Column::DeletedAt.is_null())
            .order_by_desc(git_code_review_links::Column::CreatedAt)
            .into_model::<ReviewRow>()
            .all(&conn)
            .await
            .map_err(sea_db)?
    };
    Ok(rows
        .into_iter()
        .map(|row| context::CodeReviewView {
            url: row.url,
            title: Some(row.title),
            state: row.state,
            merged: row.merged,
            draft: row.draft,
            provider: row.provider,
            external_iid: row.external_iid,
        })
        .collect())
}

/// The first bound remote: oldest live binding.
#[derive(Debug, FromQueryResult)]
struct RemoteRow {
    provider: String,
    host_url: String,
    full_name: String,
}

fn remote_query(project_id: Uuid) -> sea_orm::Select<git_repository_bindings::Entity> {
    git_repository_bindings::Entity::find()
        .select_only()
        .column_as(git_repositories::Column::Provider, "provider")
        .column_as(git_repositories::Column::HostUrl, "host_url")
        .column_as(git_repositories::Column::FullName, "full_name")
        .join(
            JoinType::InnerJoin,
            git_repository_bindings::Relation::GitRepositories.def(),
        )
        .filter(git_repository_bindings::Column::ProjectId.eq(project_id))
        .filter(git_repository_bindings::Column::DeletedAt.is_null())
        .filter(git_repositories::Column::DeletedAt.is_null())
        .order_by_asc(git_repository_bindings::Column::CreatedAt)
        .limit(1)
}

async fn bundle_remote(
    tx: &mut Transaction<'_>,
    project_id: Uuid,
) -> Result<(Option<context::RemoteView>, Option<context::AdapterNames>), CreationError> {
    let row = {
        let conn = TxConn::new(tx.inner());
        remote_query(project_id)
            .into_model::<RemoteRow>()
            .one(&conn)
            .await
            .map_err(sea_db)?
    };
    let Some(row) = row else {
        return Ok((None, None));
    };
    let (provider, host_url, full_name) = (row.provider, row.host_url, row.full_name);
    let adapter = match provider.to_lowercase().as_str() {
        "github" => Some(context::AdapterNames {
            display_name: "GitHub".to_owned(),
            code_review_term: "pull request".to_owned(),
        }),
        "gitlab" => Some(context::AdapterNames {
            display_name: "GitLab".to_owned(),
            code_review_term: "merge request".to_owned(),
        }),
        // Unknown provider: the `KeyError` branch (`None` renders
        // `provider.title()` in `repo_context`).
        _ => None,
    };
    Ok((
        Some(context::RemoteView {
            provider,
            host_url,
            full_name,
        }),
        adapter,
    ))
}

/// One ancestor-chain node: id + own-project identifier + title.
struct ChainNode {
    id: Uuid,
    project_identifier: String,
    title: String,
}

/// `_ancestor_chain` (`context.py:55-75`): `[issue, parent, … root]`
/// with the visited-id cycle guard and the depth-50 cap. Plain FK
/// follows — no soft-delete filtering, exactly like the ORM walk.
async fn bundle_ancestors(
    tx: &mut Transaction<'_>,
    issue: &IssueView,
    project_identifier: &str,
) -> Result<Vec<ChainNode>, CreationError> {
    let mut chain = vec![ChainNode {
        id: issue.id,
        project_identifier: project_identifier.to_owned(),
        title: issue.name.clone().unwrap_or_default(),
    }];
    let mut seen = HashSet::from([issue.id]);
    let mut next = issue.parent_id;
    while let Some(id) = next {
        if chain.len() >= 50 || !seen.insert(id) {
            break;
        }
        let row = {
            let conn = TxConn::new(tx.inner());
            issues::Entity::find()
                .select_only()
                .columns([
                    issues::Column::Name,
                    issues::Column::ProjectId,
                    issues::Column::ParentId,
                ])
                .filter(issues::Column::Id.eq(id))
                .into_model::<AncestorHopRow>()
                .one(&conn)
                .await
                .map_err(sea_db)?
        };
        let Some(row) = row else {
            break;
        };
        let identifier = {
            let conn = TxConn::new(tx.inner());
            projects::Entity::find()
                .select_only()
                .column(projects::Column::Identifier)
                .filter(projects::Column::Id.eq(row.project_id))
                .into_model::<IdentifierRow>()
                .one(&conn)
                .await
                .map_err(sea_db)?
        };
        let project_identifier = identifier.map(|row| row.identifier).unwrap_or_default();
        chain.push(ChainNode {
            id,
            project_identifier,
            title: row.name,
        });
        let parent_id = row.parent_id;
        next = parent_id;
    }
    Ok(chain)
}

/// One ancestor-chain hop: `name, project_id, parent_id` with no
/// soft-delete filtering.
#[derive(Debug, FromQueryResult)]
struct AncestorHopRow {
    name: String,
    project_id: Uuid,
    parent_id: Option<Uuid>,
}

/// Chain-node project identifier.
#[derive(Debug, FromQueryResult)]
struct IdentifierRow {
    identifier: String,
}

/// The direct parent's inlined columns with its live comment count as
/// one correlated subquery (stays one statement).
#[derive(Debug, FromQueryResult)]
struct ParentColsRow {
    name: String,
    state_id: Option<Uuid>,
    git_work_branch: String,
    comments_count: i64,
}

fn parent_query(issue_id: Uuid) -> sea_orm::Select<issues::Entity> {
    issues::Entity::find()
        .select_only()
        .columns([
            issues::Column::Name,
            issues::Column::StateId,
            issues::Column::GitWorkBranch,
        ])
        .expr_as(parent_comment_count(), "comments_count")
        .filter(issues::Column::Id.eq(issue_id))
}

/// The `parent` block plus the `lineage` trail (`context.py:582-604`).
async fn bundle_parent_and_lineage(
    tx: &mut Transaction<'_>,
    ancestors: &[ChainNode],
) -> Result<(Option<context::ParentView>, Vec<context::LineageNode>), CreationError> {
    let parent = match ancestors.get(1) {
        None => None,
        Some(node) => {
            let row = {
                let conn = TxConn::new(tx.inner());
                parent_query(node.id)
                    .into_model::<ParentColsRow>()
                    .one(&conn)
                    .await
                    .map_err(sea_db)?
            };
            match row {
                None => None,
                Some(row) => {
                    let state_name = match row.state_id {
                        Some(id) => {
                            let conn = TxConn::new(tx.inner());
                            state_name_group(&conn, id)
                                .await
                                .map_err(sea_db)?
                                .map(|(name, _group)| name)
                        }
                        None => None,
                    };
                    let description = {
                        let conn = TxConn::new(tx.inner());
                        issues::Entity::find()
                            .select_only()
                            .column(issues::Column::DescriptionStripped)
                            .filter(issues::Column::Id.eq(node.id))
                            .into_tuple::<Option<String>>()
                            .one(&conn)
                            .await
                            .map_err(sea_db)?
                    }
                    .flatten();
                    let sequence = issue_sequence(tx, node.id).await?;
                    Some(context::ParentView {
                        identifier: context::issue_identifier(&node.project_identifier, sequence),
                        title: Some(row.name),
                        state_name,
                        work_branch: Some(row.git_work_branch),
                        description_stripped: description,
                        comments_count: row.comments_count,
                    })
                }
            }
        }
    };
    // Multi-level lineage only when a grandparent exists
    // (`len(ancestors) > 2`); each node keeps its own identifier.
    let lineage = if ancestors.len() > 2 {
        let mut nodes = Vec::new();
        for node in ancestors {
            let sequence = issue_sequence(tx, node.id).await?;
            nodes.push(context::LineageNode {
                identifier: context::issue_identifier(&node.project_identifier, sequence),
                title: node.title.clone(),
            });
        }
        nodes
    } else {
        Vec::new()
    };
    Ok((parent, lineage))
}

/// `(SELECT COUNT(*) FROM issue_comments c WHERE c.issue_id =
/// issues.id AND c.deleted_at IS NULL)` as one correlated scalar
/// subquery on the outer `issues.id`.
fn parent_comment_count() -> SimpleExpr {
    let subquery = Query::select()
        .expr(Func::count(Expr::col(Asterisk)))
        .from(issue_comments::Entity)
        .and_where(
            Expr::col((issue_comments::Entity, issue_comments::Column::IssueId))
                .equals((issues::Entity, issues::Column::Id)),
        )
        .and_where(Expr::col((issue_comments::Entity, issue_comments::Column::DeletedAt)).is_null())
        .to_owned();
    SimpleExpr::SubQuery(None, Box::new(subquery.into_sub_query_statement()))
}

async fn issue_sequence(tx: &mut Transaction<'_>, issue_id: Uuid) -> Result<i64, CreationError> {
    let row = {
        let conn = TxConn::new(tx.inner());
        issues::Entity::find()
            .select_only()
            .column(issues::Column::SequenceId)
            .filter(issues::Column::Id.eq(issue_id))
            .into_tuple::<i32>()
            .one(&conn)
            .await
            .map_err(sea_db)?
    };
    Ok(row.map(i64::from).unwrap_or_default())
}

async fn bundle_done_payload(
    tx: &mut Transaction<'_>,
    run_id: Uuid,
) -> Result<Option<Value>, CreationError> {
    let conn = TxConn::new(tx.inner());
    agent_run::Entity::find()
        .select_only()
        .column(agent_run::Column::DonePayload)
        .filter(agent_run::Column::Id.eq(run_id))
        .into_tuple::<Option<Value>>()
        .one(&conn)
        .await
        .map_err(sea_db)
        .map(|row| row.flatten())
}

/// Active overrides for the compose index: the workspace rows plus, when
/// a user applies, their rows.
#[derive(Debug, FromQueryResult)]
struct OverrideRow {
    section_key: String,
    body: String,
    version: i32,
    user_id: Option<Uuid>,
}

pub(crate) async fn bundle_overrides(
    tx: &mut Transaction<'_>,
    workspace_id: Uuid,
    user_id: Option<Uuid>,
) -> Result<Vec<composer::OverrideRow>, CreationError> {
    let rows = {
        let conn = TxConn::new(tx.inner());
        let user_filter = match user_id {
            Some(user) => Condition::any()
                .add(prompt_section_override::Column::UserId.is_null())
                .add(prompt_section_override::Column::UserId.eq(user)),
            None => Condition::any().add(prompt_section_override::Column::UserId.is_null()),
        };
        prompt_section_override::Entity::find()
            .select_only()
            .columns([
                prompt_section_override::Column::SectionKey,
                prompt_section_override::Column::Body,
                prompt_section_override::Column::Version,
                prompt_section_override::Column::UserId,
            ])
            .filter(prompt_section_override::Column::IsActive.eq(true))
            .filter(prompt_section_override::Column::WorkspaceId.eq(workspace_id))
            .filter(user_filter)
            .into_model::<OverrideRow>()
            .all(&conn)
            .await
            .map_err(sea_db)?
    };
    Ok(rows
        .into_iter()
        .map(|row| composer::OverrideRow {
            workspace_id: workspace_id.to_string(),
            section_key: row.section_key,
            body: row.body,
            version: i64::from(row.version),
            is_active: true,
            user_id: row.user_id.map(|id| id.to_string()),
        })
        .collect())
}

/// The workspace slug + name.
#[derive(Debug, FromQueryResult)]
struct WorkspaceRow {
    slug: String,
    name: String,
}

/// The ticker budget row.
#[derive(Debug, FromQueryResult)]
struct TickerRow {
    used: i32,
    waited: i32,
    granted: i32,
    enabled: bool,
}

/// Preload every prompt input for the new run (the `BUNDLE_*_SQL`
/// queries, same-transaction snapshot — the run row exists by now, so
/// the attempt count excludes it by id).
async fn load_render_bundle(
    tx: &mut Transaction<'_>,
    issue_id: Uuid,
    run_id: Uuid,
    parent_run_id: Option<Uuid>,
    trigger: &str,
    created_by_id: Uuid,
) -> Result<RenderBundle, CreationError> {
    let issue_row = {
        let conn = TxConn::new(tx.inner());
        issues::Entity::find()
            .select_only()
            .columns(issue_columns())
            .filter(issues::Column::Id.eq(issue_id))
            .into_model::<IssueRow>()
            .one(&conn)
            .await
            .map_err(sea_db)?
    };
    let issue = IssueView::from(issue_row.ok_or_else(row_missing)?);
    let project_id = issue
        .project_id
        .ok_or_else(|| CreationError::MissingRow("issue has no project".to_owned()))?;
    let project_row = {
        let conn = TxConn::new(tx.inner());
        projects::Entity::find()
            .select_only()
            .columns(project_columns())
            .filter(projects::Column::Id.eq(project_id))
            .into_model::<ProjectRow>()
            .one(&conn)
            .await
            .map_err(sea_db)?
    };
    let project = ProjectView::from(project_row.ok_or_else(row_missing)?);
    let state = match issue.state_id {
        Some(id) => {
            let conn = TxConn::new(tx.inner());
            state_name_group(&conn, id)
                .await
                .map_err(sea_db)?
                .map(|(name, group)| StateView { id, name, group })
        }
        None => None,
    };
    let workspace = {
        let conn = TxConn::new(tx.inner());
        workspaces::Entity::find()
            .select_only()
            .columns([workspaces::Column::Slug, workspaces::Column::Name])
            .filter(workspaces::Column::Id.eq(issue.workspace_id))
            .into_model::<WorkspaceRow>()
            .one(&conn)
            .await
            .map_err(sea_db)?
    }
    .ok_or_else(row_missing)?;
    let labels = bundle_labels(tx, issue_id).await?;
    let assignees = bundle_assignees(tx, issue_id).await?;
    let project_states = bundle_project_states(tx, project_id).await?;
    let children = bundle_children(tx, issue_id).await?;
    let (relation_rows, refs, directional_refs) = bundle_relations(tx, issue_id).await?;
    let comments = bundle_comments(tx, issue_id).await?;
    let reviews = bundle_reviews(tx, issue_id).await?;
    let (remote, adapter) = bundle_remote(tx, project_id).await?;
    let ancestors = bundle_ancestors(tx, &issue, &project.identifier).await?;
    let (parent, lineage) = bundle_parent_and_lineage(tx, &ancestors).await?;
    let prior_run_count = {
        let conn = TxConn::new(tx.inner());
        agent_run::Entity::find()
            .filter(agent_run::Column::WorkItemId.eq(issue_id))
            .filter(agent_run::Column::Id.ne(run_id))
            .count(&conn)
            .await
            .map_err(sea_db)?
    };
    let ticker = {
        let conn = TxConn::new(tx.inner());
        issue_agent_ticker::Entity::find()
            .select_only()
            .columns([
                issue_agent_ticker::Column::Used,
                issue_agent_ticker::Column::Waited,
                issue_agent_ticker::Column::Granted,
                issue_agent_ticker::Column::Enabled,
            ])
            .filter(issue_agent_ticker::Column::IssueId.eq(issue_id))
            .into_model::<TickerRow>()
            .one(&conn)
            .await
            .map_err(sea_db)?
    };
    let direct_parent_payload = match parent_run_id {
        Some(id) => bundle_done_payload(tx, id).await?,
        None => None,
    };
    // A parent run with a NULL payload must render the fallback, never
    // the ticker payload — so its existence travels separately from the
    // payload itself (PIDASHCONV-1232).
    let has_parent_run = parent_run_id.is_some();
    let ticker_parent_payload = {
        let resume = {
            let conn = TxConn::new(tx.inner());
            issue_agent_ticker::Entity::find()
                .select_only()
                .column(issue_agent_ticker::Column::ResumeParentRunId)
                .filter(issue_agent_ticker::Column::IssueId.eq(issue_id))
                .into_tuple::<Option<Uuid>>()
                .one(&conn)
                .await
                .map_err(sea_db)?
        }
        .flatten();
        match resume {
            Some(id) => bundle_done_payload(tx, id).await?,
            None => None,
        }
    };
    let user_id = user_id_for_run(trigger, created_by_id)
        .map(|id| id.parse::<Uuid>())
        .transpose()
        .map_err(|_| CreationError::MissingRow("bad user id".to_owned()))?;
    let override_rows = bundle_overrides(tx, issue.workspace_id, user_id).await?;
    Ok(RenderBundle {
        issue,
        project,
        workspace_slug: workspace.slug,
        workspace_name: workspace.name,
        state,
        labels,
        assignees,
        project_states,
        children,
        relation_rows,
        refs,
        directional_refs,
        comments,
        reviews,
        remote,
        adapter,
        parent,
        lineage,
        prior_run_count: i64::try_from(prior_run_count).unwrap_or(i64::MAX),
        ticker: ticker.map(|row| TickerBudget {
            used: i64::from(row.used),
            waited: i64::from(row.waited),
            granted: i64::from(row.granted),
            enabled: row.enabled,
        }),
        direct_parent_payload,
        has_parent_run,
        ticker_parent_payload,
        override_rows,
    })
}

// ---------------------------------------------------------------------------
// Finalize seam implementation (row effects only — D-15 owns the pipeline)
// ---------------------------------------------------------------------------

impl<C, H, P> FinalizeAgentRunSeam for LiveCreationStore<'_, C, H, P>
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
        now: DateTime<Utc>,
    ) -> Result<RunView, CreationError> {
        // First-writer-wins: an already-terminal row skips the write
        // (`finalize_agent_run` returns False) and the caller
        // refreshes anyway.
        let terminal: Vec<&str> = [
            AgentRunStatus::Completed,
            AgentRunStatus::Failed,
            AgentRunStatus::Cancelled,
            AgentRunStatus::Blocked,
            AgentRunStatus::Refused,
        ]
        .iter()
        .map(|status| status.value())
        .collect();
        let locked = {
            let conn = TxConn::new(self.tx.inner());
            agent_run::Entity::find()
                .select_only()
                .columns(run_columns())
                .filter(agent_run::Column::Id.eq(run_id))
                .filter(agent_run::Column::Status.is_not_in(terminal))
                .lock_exclusive()
                .into_model::<RunRow>()
                .one(&conn)
                .await
                .map_err(sea_db)?
        };
        if locked.is_none() {
            return self.run(run_id).await?.ok_or_else(row_missing);
        }
        let locked_row = locked.expect("checked");
        // The folded patch merges in Rust over the locked row (`merge_error_details`).
        let patch_text = |value: &str| {
            if value.is_empty() {
                Value::Null
            } else {
                Value::String(value.to_owned())
            }
        };
        let merged = run_bag::merge_error_details(
            &locked_row.error_details,
            &[
                ("error_code", patch_text(error_code)),
                ("error", patch_text(error)),
            ],
        );
        let locked = map_run_row(locked_row)?;
        {
            let conn = TxConn::new(self.tx.inner());
            let ended_at: sea_orm::prelude::DateTimeWithTimeZone = now.into();
            agent_run::Entity::update_many()
                .col_expr(agent_run::Column::Status, Expr::value("failed"))
                .col_expr(agent_run::Column::EndedAt, Expr::value(ended_at))
                .col_expr(
                    agent_run::Column::TerminalHooksAppliedAt,
                    Expr::value(Option::<sea_orm::prelude::DateTimeWithTimeZone>::None),
                )
                .col_expr(
                    agent_run::Column::TerminalCapacityReleasedAt,
                    Expr::value(Option::<sea_orm::prelude::DateTimeWithTimeZone>::None),
                )
                .col_expr(agent_run::Column::ErrorDetails, Expr::value(merged))
                .filter(agent_run::Column::Id.eq(run_id))
                .exec(&conn)
                .await
                .map_err(sea_db)?;
        }
        if locked.executor_kind == AgentExecutorKind::CloudAgent {
            let exists = {
                let conn = TxConn::new(self.tx.inner());
                agent_run_event::Entity::find()
                    .filter(agent_run_event::Column::AgentRunId.eq(run_id))
                    .filter(agent_run_event::Column::Kind.eq("terminal"))
                    .count(&conn)
                    .await
                    .map_err(sea_db)?
                    > 0
            };
            if !exists {
                let seq = {
                    let conn = TxConn::new(self.tx.inner());
                    agent_run_event::Entity::find()
                        .select_only()
                        .column(agent_run_event::Column::Seq)
                        .filter(agent_run_event::Column::AgentRunId.eq(run_id))
                        .order_by_desc(agent_run_event::Column::Seq)
                        .limit(1)
                        .into_tuple::<i32>()
                        .one(&conn)
                        .await
                        .map_err(sea_db)?
                        .unwrap_or(0)
                        + 1
                };
                let payload = serde_json::json!({"status": "failed", "error_code": error_code});
                let created_at: sea_orm::prelude::DateTimeWithTimeZone = now.into();
                let event = agent_run_event::ActiveModel {
                    id: ActiveValue::NotSet,
                    agent_run_id: Set(run_id),
                    seq: Set(seq),
                    kind: Set("terminal".to_owned()),
                    payload: Set(payload),
                    created_at: Set(created_at),
                };
                let conn = TxConn::new(self.tx.inner());
                event.insert(&conn).await.map_err(sea_db)?;
            }
        }
        // The `on_commit` lambda (`_publish_effects`): captured, never
        // executed — the celery delay + terminal hooks are D-15's
        // pipeline.
        self.terminal_effects.push(run_id);
        self.run(run_id).await?.ok_or_else(row_missing)
    }
}

// ---------------------------------------------------------------------------
// Transaction drivers (commit, then drain post-commit)
// ---------------------------------------------------------------------------

/// One committed creation: the services outcome plus the drained
/// post-commit effects. `terminal_effects` is returned pending for
/// D-15 — never executed here.
#[derive(Debug)]
pub struct CreationOutcome<T> {
    pub outcome: T,
    /// The collected dispatch ids, in capture order.
    pub dispatched: Vec<Uuid>,
    /// The per-id dispatch verdicts, in drain order.
    pub dispatch_decisions: Vec<(Uuid, DispatchDecision)>,
    /// Finalized run ids whose terminal effects D-15 must still run.
    pub terminal_effects: Vec<Uuid>,
}

async fn drain<C: AdmissionCache>(
    pool: &PgPool,
    cloud: &CloudAgentSettings,
    cache: &C,
    dispatches: Vec<Uuid>,
    deferred: Vec<DeferredConsume>,
    now: DateTime<Utc>,
) -> Result<Vec<(Uuid, DispatchDecision)>, CreationJobsError> {
    let mut decisions = Vec::with_capacity(dispatches.len());
    for run_id in dispatches {
        let decision = dispatch_agent_run(pool, cloud, run_id, now).await?;
        decisions.push((run_id, decision));
    }
    for consume in &deferred {
        consume_admission_token(cache, consume);
    }
    Ok(decisions)
}

/// Open the insertion transaction, run
/// [`create_and_dispatch_run`][pidash_services::orchestration::creation::create_and_dispatch_run],
/// commit, then drain post-commit dispatch.
pub async fn create_and_dispatch_run<C, H, P>(
    pool: &PgPool,
    deps: StoreDeps<C, H, P>,
    req: &pidash_services::orchestration::creation::CreateDispatchRequest,
) -> Result<CreationOutcome<pidash_types::orchestration::TransitionOutcome>, CreationJobsError>
where
    C: AdmissionCache,
    H: Fn(Uuid) -> bool + Clone + Send + Sync,
    P: Fn(Option<Uuid>) -> LlmProfile + Clone + Send + Sync,
{
    let cloud = deps.cloud.clone();
    let tx = Transaction::begin(pool).await?;
    let mut store = LiveCreationStore::new(tx, pool, deps);
    let outcome =
        pidash_services::orchestration::creation::create_and_dispatch_run(&mut store, req).await?;
    let (tx, pool, cache, dispatches, deferred, terminal_effects) = store.into_parts();
    tx.commit().await?;
    let dispatch_decisions = drain(pool, &cloud, &cache, dispatches, deferred, req.now).await?;
    Ok(CreationOutcome {
        outcome,
        dispatched: dispatch_decisions.iter().map(|(id, _)| *id).collect(),
        dispatch_decisions,
        terminal_effects,
    })
}

/// Open the insertion transaction, run
/// [`create_continuation_run`][pidash_services::orchestration::creation::create_continuation_run],
/// commit, then drain post-commit dispatch.
pub async fn create_continuation_run<C, H, P>(
    pool: &PgPool,
    deps: StoreDeps<C, H, P>,
    req: &pidash_services::orchestration::creation::ContinuationRequest,
) -> Result<CreationOutcome<pidash_types::orchestration::ContinuationOutcome>, CreationJobsError>
where
    C: AdmissionCache,
    H: Fn(Uuid) -> bool + Clone + Send + Sync,
    P: Fn(Option<Uuid>) -> LlmProfile + Clone + Send + Sync,
{
    let cloud = deps.cloud.clone();
    let tx = Transaction::begin(pool).await?;
    let mut store = LiveCreationStore::new(tx, pool, deps);
    let outcome =
        pidash_services::orchestration::creation::create_continuation_run(&mut store, req).await?;
    let (tx, pool, cache, dispatches, deferred, terminal_effects) = store.into_parts();
    tx.commit().await?;
    let dispatch_decisions = drain(pool, &cloud, &cache, dispatches, deferred, req.now).await?;
    Ok(CreationOutcome {
        outcome,
        dispatched: dispatch_decisions.iter().map(|(id, _)| *id).collect(),
        dispatch_decisions,
        terminal_effects,
    })
}

/// Open the insertion transaction, run
/// [`create_project_move_handoff_run`][pidash_services::orchestration::creation::create_project_move_handoff_run],
/// commit, then drain post-commit dispatch.
pub async fn create_project_move_handoff_run<C, H, P>(
    pool: &PgPool,
    deps: StoreDeps<C, H, P>,
    req: &pidash_services::orchestration::creation::HandoffCreateRequest,
) -> Result<CreationOutcome<RunView>, CreationJobsError>
where
    C: AdmissionCache,
    H: Fn(Uuid) -> bool + Clone + Send + Sync,
    P: Fn(Option<Uuid>) -> LlmProfile + Clone + Send + Sync,
{
    let cloud = deps.cloud.clone();
    let tx = Transaction::begin(pool).await?;
    let mut store = LiveCreationStore::new(tx, pool, deps);
    let outcome =
        pidash_services::orchestration::creation::create_project_move_handoff_run(&mut store, req)
            .await?;
    let (tx, pool, cache, dispatches, deferred, terminal_effects) = store.into_parts();
    tx.commit().await?;
    let dispatch_decisions = drain(pool, &cloud, &cache, dispatches, deferred, req.now).await?;
    Ok(CreationOutcome {
        outcome,
        dispatched: dispatch_decisions.iter().map(|(id, _)| *id).collect(),
        dispatch_decisions,
        terminal_effects,
    })
}

/// Open the transaction, run
/// [`complete_project_move_handoff`][pidash_services::orchestration::creation::complete_project_move_handoff],
/// commit, then drain post-commit dispatch.
pub async fn complete_project_move_handoff<C, H, P>(
    pool: &PgPool,
    deps: StoreDeps<C, H, P>,
    run_id: Uuid,
    now: DateTime<Utc>,
) -> Result<CreationOutcome<Option<RunView>>, CreationJobsError>
where
    C: AdmissionCache,
    H: Fn(Uuid) -> bool + Clone + Send + Sync,
    P: Fn(Option<Uuid>) -> LlmProfile + Clone + Send + Sync,
{
    let cloud = deps.cloud.clone();
    let tx = Transaction::begin(pool).await?;
    let mut store = LiveCreationStore::new(tx, pool, deps);
    let outcome = pidash_services::orchestration::creation::complete_project_move_handoff(
        &mut store, run_id, now,
    )
    .await?;
    let (tx, pool, cache, dispatches, deferred, terminal_effects) = store.into_parts();
    tx.commit().await?;
    let dispatch_decisions = drain(pool, &cloud, &cache, dispatches, deferred, now).await?;
    Ok(CreationOutcome {
        outcome,
        dispatched: dispatch_decisions.iter().map(|(id, _)| *id).collect(),
        dispatch_decisions,
        terminal_effects,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pidash_db::dispatch::status::AgentRunTrigger;
    use pidash_services::dispatch::CloudAgentAdmissionError;
    use pidash_services::orchestration::creation::{
        ContinuationRequest, CreateDispatchRequest, HandoffCreateRequest,
    };
    use pidash_types::orchestration::PROJECT_MOVE_HANDOFF_CONFIG_KEY;
    use serde_json::json;
    use sqlx::postgres::PgPoolOptions;
    use std::cell::RefCell;
    use std::sync::Mutex;

    static CREATE_DISPATCH: &str = include_str!(
        "../../../../fixtures/orchestration/fx06_creation/create_dispatch.before_after.json"
    );
    static CONTINUATION: &str = include_str!(
        "../../../../fixtures/orchestration/fx06_creation/continuation.before_after.json"
    );
    static HANDOFF: &str =
        include_str!("../../../../fixtures/orchestration/fx06_creation/handoff.before_after.json");

    fn fixture(raw: &str) -> Value {
        serde_json::from_str(raw).expect("fixture parses")
    }

    // -- offline: delegation mappings -------------------------------------

    #[test]
    fn pin_key_presence_follows_executor() {
        let base = crate::dispatch::ExecutionFields {
            executor_kind: AgentExecutorKind::LocalRunner,
            tool_plan: json!({}),
            pinned_runner_id: None,
            error_code: None,
            cloud_admission_error: None,
        };
        // Local carries no key: the computed pin survives.
        assert_eq!(map_execution_fields(base.clone()).pinned_runner_entry, None);
        // Cloud always carries `None`: the computed pin is clobbered.
        let cloud = crate::dispatch::ExecutionFields {
            executor_kind: AgentExecutorKind::CloudAgent,
            ..base.clone()
        };
        assert_eq!(map_execution_fields(cloud).pinned_runner_entry, Some(None));
        // Managed carries the pick.
        let pin = Uuid::new_v4();
        let managed = crate::dispatch::ExecutionFields {
            executor_kind: AgentExecutorKind::ManagedRunner,
            pinned_runner_id: Some(pin),
            ..base
        };
        assert_eq!(
            map_execution_fields(managed).pinned_runner_entry,
            Some(Some(pin))
        );
    }

    #[test]
    fn execution_error_routing() {
        // Refusals surface `str(exc)` for the catch …
        let refused = map_execution_error(ExecutionFieldsError::Admission(
            CloudAgentAdmissionError::new("run_quota_exceeded", "Cloud Agent queue is full", None),
        ));
        assert!(
            matches!(refused, ExecutionError::Refused(message) if message == "Cloud Agent queue is full")
        );
        // … while database failures propagate.
        let db = map_execution_error(ExecutionFieldsError::Db(sqlx::Error::RowNotFound));
        assert!(matches!(db, ExecutionError::Store(CreationError::Db(_))));
    }

    #[test]
    fn comment_timestamps_use_isoformat_offset() {
        // Mirrors `prompting::tests::comment_timestamps_use_isoformat_offset`:
        // this file's `render_isoformat` must not fork the api helper.
        let dt = DateTime::parse_from_rfc3339("2026-09-28T04:52:20.217834Z")
            .expect("parse")
            .with_timezone(&Utc);
        assert_eq!(render_isoformat(dt), "2026-09-28T04:52:20.217834+00:00");
        let whole = DateTime::parse_from_rfc3339("2026-09-28T04:52:20Z")
            .expect("parse")
            .with_timezone(&Utc);
        assert_eq!(render_isoformat(whole), "2026-09-28T04:52:20+00:00");
        // Millisecond-exact micros keep six digits (where chrono AutoSi
        // would trim to ".123"): Python `isoformat()` always prints six.
        let millis = DateTime::parse_from_rfc3339("2026-09-28T04:52:20.123Z")
            .expect("parse")
            .with_timezone(&Utc);
        assert_eq!(render_isoformat(millis), "2026-09-28T04:52:20.123000+00:00");
    }

    // -- live harness ------------------------------------------------------

    async fn pool() -> PgPool {
        let url =
            std::env::var("DATABASE_URL").expect("export DATABASE_URL for live creation tests");
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

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-06-01T12:00:00Z")
            .expect("frozen clock")
            .with_timezone(&Utc)
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

    #[derive(Default)]
    struct FakeCache {
        counts: Mutex<HashMap<String, i64>>,
        consumed: RefCell<Vec<(String, i64)>>,
    }

    impl AdmissionCache for FakeCache {
        type Error = CacheError;

        fn bucket_count(&self, key: &str) -> Result<Option<i64>, Self::Error> {
            Ok(self.counts.lock().expect("cache lock").get(key).copied())
        }

        fn add_or_incr(&self, key: &str, timeout_secs: i64) -> Result<(), Self::Error> {
            *self
                .counts
                .lock()
                .expect("cache lock")
                .entry(key.to_owned())
                .or_insert(0) += 1;
            self.consumed
                .borrow_mut()
                .push((key.to_owned(), timeout_secs));
            Ok(())
        }
    }

    fn no_llm(_user: Uuid) -> bool {
        false
    }

    fn llm_ok(_user: Uuid) -> bool {
        true
    }

    fn unavailable_profile(_user: Option<Uuid>) -> LlmProfile {
        LlmProfile {
            available: false,
            reason_code: "fx6-no-profile".to_owned(),
        }
    }

    type TestDeps = StoreDeps<FakeCache, fn(Uuid) -> bool, fn(Option<Uuid>) -> LlmProfile>;

    fn deps() -> TestDeps {
        StoreDeps {
            cloud: django_cloud_settings(),
            managed: django_managed_settings(),
            cache: FakeCache::default(),
            has_usable_llm_config: no_llm,
            toolsets: Arc::new(NoExtraToolsets),
            llm_profile: unavailable_profile,
            now_unix_secs: now().timestamp(),
        }
    }

    /// A replacement seam naming its schema tool: the store answers the
    /// injected name, not the CE default.
    #[derive(Debug)]
    struct AcmeToolsets;

    impl CloudAgentToolsetsSeam for AcmeToolsets {
        fn schema_tool_name(&self) -> &'static str {
            "acme_get_tool_schema"
        }
    }

    #[tokio::test]
    #[ignore = "needs migrated scratch DB via DATABASE_URL"]
    async fn live_store_schema_tool_name_flows_from_injected_seam() {
        let pool = pool().await;
        let tx = Transaction::begin(&pool).await.expect("begin");
        let mut store_deps = deps();
        store_deps.toolsets = Arc::new(AcmeToolsets);
        let store = LiveCreationStore::new(tx, &pool, store_deps);
        assert_eq!(
            store.extra_toolsets_schema_tool(),
            "acme_get_tool_schema".to_owned()
        );
        let (tx, _, _, _, _, _) = store.into_parts();
        tx.rollback().await.expect("rollback");
    }

    struct Graph {
        ws: Uuid,
        ws_slug: String,
        user: Uuid,
        project: Uuid,
        pod: Uuid,
        states: HashMap<String, Uuid>,
        issue: Uuid,
    }

    /// Seed the fx6 graph: workspace + local-runner project + default
    /// pod + the five ticking states + one `FX6 C1` issue on
    /// In Progress. Names/identifiers match the fixture; ids and the
    /// workspace slug are tag-unique (parallel-safe) and normalized
    /// away in the byte-compare.
    async fn seed_graph(pool: &PgPool, tag: &str) -> Graph {
        let at = now();
        let user = Uuid::new_v4();
        let ws = Uuid::new_v4();
        // Unique per invocation (parallel + rerun safe); the slug is
        // normalized away in the byte-compare.
        let uniq = format!("{tag}-{}", &Uuid::new_v4().to_string()[..8]);
        let ws_slug = format!("fx6-ws-{uniq}");
        let project = Uuid::new_v4();
        let pod = Uuid::new_v4();
        let issue = Uuid::new_v4();
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
        .bind(format!("fx6-{uniq}"))
        .bind(format!("fx6-{uniq}@example.com"))
        .bind(Uuid::new_v4().to_string())
        .bind(at)
        .execute(pool)
        .await
        .expect("seed user");
        sqlx::query(
            "INSERT INTO workspaces (id, created_at, updated_at, name, background_color, \
             owner_id, slug, timezone) \
             VALUES ($1, $2, $2, $3, '#f6c8dB', $4, $5, 'UTC')",
        )
        .bind(ws)
        .bind(at)
        .bind("FX6 ws")
        .bind(user)
        .bind(&ws_slug)
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
             VALUES ($1, $2, $2, 'FX6', 'FX6', '', 2, false, false, false, true, false, false, \
             false, false, false, true, 0, 0, '{}', 'UTC', 'https://example.com/fx6.git', \
             'main', 10800, 10, 10800, 10800, true, 'local_runner', $3)",
        )
        .bind(project)
        .bind(at)
        .bind(ws)
        .execute(pool)
        .await
        .expect("seed project");
        sqlx::query(
            "INSERT INTO pod (id, created_at, updated_at, description, is_default, name, \
             project_id, workspace_id) \
             VALUES ($1, $2, $2, '', true, $3, $4, $5)",
        )
        .bind(pod)
        .bind(at)
        .bind(format!("fx6-pod-{uniq}"))
        .bind(project)
        .bind(ws)
        .execute(pool)
        .await
        .expect("seed pod");
        let mut states = HashMap::new();
        for (index, (name, group)) in [
            ("Todo", "unstarted"),
            ("In Progress", "started"),
            ("In Review", "review"),
            ("In Test", "test"),
            ("Done", "completed"),
        ]
        .into_iter()
        .enumerate()
        {
            let id = Uuid::new_v4();
            sqlx::query(
                "INSERT INTO states (id, created_at, updated_at, name, description, color, slug, \
                 project_id, workspace_id, sequence, \"group\", \"default\", is_triage) \
                 VALUES ($1, $2, $2, $3, '', '', $4, $5, $6, $7, $8, false, false)",
            )
            .bind(id)
            .bind(at)
            .bind(name)
            .bind(format!(
                "fx6-{uniq}-{}-{index}",
                name.to_lowercase().replace(' ', "-")
            ))
            .bind(project)
            .bind(ws)
            .bind(index as f64)
            .bind(group)
            .execute(pool)
            .await
            .expect("seed state");
            states.insert(name.to_owned(), id);
        }
        sqlx::query(
            "INSERT INTO issues (id, created_at, updated_at, name, description_json, priority, \
             sequence_id, created_by_id, project_id, state_id, workspace_id, description_html, \
             description_stripped, sort_order, is_draft, git_work_branch, workpad, \
             complexity_score) \
             VALUES ($1, $2, $2, 'FX6 C1', '{}', 'none', 1, $3, $4, $5, $6, '', NULL, 0.0, \
             false, '', '', 0)",
        )
        .bind(issue)
        .bind(at)
        .bind(user)
        .bind(project)
        .bind(states["In Progress"])
        .bind(ws)
        .execute(pool)
        .await
        .expect("seed issue");
        Graph {
            ws,
            ws_slug,
            user,
            project,
            pod,
            states,
            issue,
        }
    }

    /// Seed one run with Django-side defaults; the creation-touched
    /// columns vary.
    #[allow(clippy::too_many_arguments)]
    async fn seed_run(
        pool: &PgPool,
        graph: &Graph,
        status: &str,
        trigger: &str,
        executor: &str,
        phase_kind: &str,
        parent: Option<Uuid>,
        runner: Option<Uuid>,
        pinned: Option<Uuid>,
        run_config: Value,
        created_at: DateTime<Utc>,
    ) -> Uuid {
        let id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO agent_run (id, workspace_id, created_by_id, pod_id, runner_id, \
             pinned_runner_id, parent_run_id, work_item_id, status, executor_kind, \
             dispatch_attempts, cancel_reason, tool_plan, prompt, trigger, \
             phase_kind, run_config, required_capabilities, thread_id, agent_metadata, \
             error_details, llm_model, usage, created_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, 0, '', '{}', '', $11, \
             $12, $13, '[]', '', '{}', '{}', '', '{}', $14)",
        )
        .bind(id)
        .bind(graph.ws)
        .bind(graph.user)
        .bind(graph.pod)
        .bind(runner)
        .bind(pinned)
        .bind(parent)
        .bind(graph.issue)
        .bind(status)
        .bind(executor)
        .bind(trigger)
        .bind(phase_kind)
        .bind(&run_config)
        .bind(created_at)
        .execute(pool)
        .await
        .expect("seed run");
        id
    }

    async fn seed_runner(pool: &PgPool, graph: &Graph, pod: Uuid, status: &str) -> Uuid {
        let id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO runner (id, created_at, updated_at, owner_id, workspace_id, pod_id, \
             name, host_label, provisioning, pod_dispatch, visibility, refresh_token_hash, \
             refresh_token_fingerprint, refresh_token_generation, \
             previous_refresh_token_hash, access_token_signing_key_version, \
             enrollment_token_hash, enrollment_token_fingerprint, capabilities, status, \
             os, arch, runner_version, dev_metadata, protocol_version, revoked_reason) \
             VALUES ($1, $2, $2, $3, $4, $5, $6, '', 'desktop_bundled', 'pinned_only', 0, '', '', 0, '', \
             1, '', '', '[]', $7, '', '', '', '{}', 1, '')",
        )
        .bind(id)
        .bind(now())
        .bind(graph.user)
        .bind(graph.ws)
        .bind(pod)
        .bind(format!("fx6-runner-{}", &id.to_string()[..8]))
        .bind(status)
        .execute(pool)
        .await
        .expect("seed runner");
        id
    }

    async fn seed_ticker(pool: &PgPool, graph: &Graph, resume_parent: Option<Uuid>) -> Uuid {
        let id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO issue_agent_ticker (id, created_at, updated_at, user_disabled, used, \
             enabled, issue_id, disarm_reason, resume_parent_run_id, granted, pending_entry, \
             pending_entry_free, pending_entry_trigger, waited) \
             VALUES ($1, $2, $2, false, 0, true, $3, '', $4, 0, false, false, '', 0)",
        )
        .bind(id)
        .bind(now())
        .bind(graph.issue)
        .bind(resume_parent)
        .execute(pool)
        .await
        .expect("seed ticker");
        id
    }

    async fn seed_override(
        pool: &PgPool,
        graph: &Graph,
        section_key: &str,
        body: &str,
        user_id: Option<Uuid>,
    ) -> Uuid {
        let id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO prompt_section_override (id, section_key, body, is_active, version, \
             needs_attention, created_at, updated_at, user_id, workspace_id) \
             VALUES ($1, $2, $3, true, 1, false, $4, $4, $5, $6)",
        )
        .bind(id)
        .bind(section_key)
        .bind(body)
        .bind(now())
        .bind(user_id)
        .bind(graph.ws)
        .execute(pool)
        .await
        .expect("seed override");
        id
    }

    /// The creation-touched run columns for before/after assertions.
    #[allow(clippy::type_complexity)]
    async fn run_row(
        pool: &PgPool,
        id: Uuid,
    ) -> (
        String,
        Option<Uuid>,
        Option<Uuid>,
        Option<Uuid>,
        Option<Uuid>,
        String,
        String,
        String,
        String,
        String,
        Option<DateTime<Utc>>,
        String,
        Option<Value>,
        Value,
        Value,
    ) {
        sqlx::query_as(
            "SELECT status, parent_run_id, runner_id, pinned_runner_id, owner_id, trigger, \
             phase_kind, executor_kind, COALESCE(error_details->>'code', '') AS error_code, \
             COALESCE(error_details->>'message', '') AS error, ended_at, prompt, prompt_manifest, \
             run_config, tool_plan FROM agent_run WHERE id = $1",
        )
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("run row")
    }

    #[allow(clippy::too_many_arguments)]
    async fn cleanup(
        pool: &PgPool,
        graph: Graph,
        runs: &[Uuid],
        runners: &[Uuid],
        extra_issues: &[Uuid],
        extra_pods: &[Uuid],
        overrides: &[Uuid],
        tickers: &[Uuid],
    ) {
        for id in overrides {
            sqlx::query("DELETE FROM prompt_section_override WHERE id = $1")
                .bind(id)
                .execute(pool)
                .await
                .expect("cleanup override");
        }
        for id in tickers {
            sqlx::query("DELETE FROM issue_agent_ticker WHERE id = $1")
                .bind(id)
                .execute(pool)
                .await
                .expect("cleanup ticker");
        }
        for run in runs.iter().rev() {
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
        for issue in extra_issues {
            sqlx::query("DELETE FROM issues WHERE id = $1")
                .bind(issue)
                .execute(pool)
                .await
                .expect("cleanup issue");
        }
        sqlx::query("DELETE FROM issues WHERE id = $1")
            .bind(graph.issue)
            .execute(pool)
            .await
            .expect("cleanup issue");
        sqlx::query("DELETE FROM states WHERE project_id = $1")
            .bind(graph.project)
            .execute(pool)
            .await
            .expect("cleanup states");
        for pod in extra_pods {
            sqlx::query("DELETE FROM pod WHERE id = $1")
                .bind(pod)
                .execute(pool)
                .await
                .expect("cleanup pod");
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

    /// Normalize the tag-unique seeds out of a rendered prompt so it
    /// compares byte-for-byte with the fixture text.
    fn normalize_prompt(prompt: &str, graph: &Graph, run_id: Uuid) -> String {
        prompt
            .replace(&graph.ws_slug, "fx6-ws")
            .replace(
                &graph.project.to_string(),
                "66666666-aaaa-bbbb-cccc-000000000606",
            )
            .replace(
                &graph.issue.to_string(),
                "66666666-aaaa-bbbb-cccc-000000006101",
            )
            .replace(&run_id.to_string(), "<run-id>")
    }

    /// Assert two long texts equal, showing the first divergence.
    fn assert_prompt_eq(actual: &str, expected: &str) {
        if actual == expected {
            return;
        }
        let at = actual
            .char_indices()
            .zip(expected.char_indices())
            .find(|((_, a), (_, b))| a != b)
            .map(|((index, _), _)| index)
            .unwrap_or_else(|| actual.len().min(expected.len()));
        let mut lo = at.saturating_sub(200);
        while lo > 0 && !actual.is_char_boundary(lo) {
            lo -= 1;
        }
        let mut hi = (at + 300).min(actual.len().min(expected.len()));
        while hi < actual.len().min(expected.len())
            && (!actual.is_char_boundary(hi) || !expected.is_char_boundary(hi))
        {
            hi += 1;
        }
        panic!(
            "prompt diverges at char {at} (len {} vs {}):\nactual  : {:?}\nexpected: {:?}",
            actual.len(),
            expected.len(),
            &actual[lo..hi.min(actual.len())],
            &expected[lo..hi.min(expected.len())]
        );
    }

    fn dispatch_req(graph: &Graph, parent: Option<RunView>) -> CreateDispatchRequest {
        CreateDispatchRequest {
            issue_id: graph.issue,
            parent,
            creator_id: graph.user,
            pod_id: graph.pod,
            fresh_session: false,
            trigger: AgentRunTrigger::StateTransition.value().to_owned(),
            now: now(),
        }
    }

    // -- live replays (FX-ORCH-06) ------------------------------------------

    #[tokio::test]
    #[ignore = "needs migrated scratch DB via DATABASE_URL"]
    async fn live_c1_created_no_parent() {
        let pool = pool().await;
        let graph = seed_graph(&pool, "c1").await;
        let cases = fixture(CREATE_DISPATCH);
        let case = &cases["cases"]["C1_created_no_parent"];
        let before: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM agent_run WHERE work_item_id = $1")
                .bind(graph.issue)
                .fetch_one(&pool)
                .await
                .expect("before count");
        assert_eq!(before, 0);
        let result = create_and_dispatch_run(&pool, deps(), &dispatch_req(&graph, None))
            .await
            .expect("creates");
        assert_eq!(result.outcome.reason, case["reason"].as_str().unwrap());
        let created = result.outcome.created_run.expect("created");
        assert_eq!(result.dispatched, vec![created]);
        assert!(result.terminal_effects.is_empty());
        assert!(matches!(
            result.dispatch_decisions.as_slice(),
            [(id, DispatchDecision::PodDrain { pod_id })]
                if *id == created && *pod_id == graph.pod
        ));
        let row = run_row(&pool, created).await;
        let after = &case["after"];
        assert_eq!(row.0, after["status"].as_str().unwrap());
        assert_eq!(row.1, None);
        assert_eq!(row.2, None);
        assert_eq!(row.3, None);
        assert_eq!(row.4, None, "owner stays NULL");
        assert_eq!(row.5, after["trigger"].as_str().unwrap());
        assert_eq!(row.6, after["phase_kind"].as_str().unwrap());
        assert_eq!(row.7, after["executor_kind"].as_str().unwrap());
        assert_eq!(row.8, "");
        assert_eq!(row.9, "");
        assert_eq!(row.10, None);
        assert_eq!(
            row.13,
            json!({
                "repo_url": "https://example.com/fx6.git",
                "repo_ref": "main",
                "git_work_branch": null,
            })
        );
        assert_eq!(row.14, json!({}));
        // Byte-compare the rendered prompt + manifest.
        assert_prompt_eq(
            &normalize_prompt(&row.11, &graph, created),
            after["prompt"].as_str().unwrap(),
        );
        assert_eq!(row.12.as_ref().unwrap(), &after["prompt_manifest"]);
        cleanup(&pool, graph, &[created], &[], &[], &[], &[], &[]).await;
    }

    #[tokio::test]
    #[ignore = "needs migrated scratch DB via DATABASE_URL"]
    async fn live_c2_parent_linkage_and_pin() {
        let pool = pool().await;
        let graph = seed_graph(&pool, "c2").await;
        let runner = seed_runner(&pool, &graph, graph.pod, "online").await;
        let parent = seed_run(
            &pool,
            &graph,
            "completed",
            "state_transition",
            "local_runner",
            "coding-task",
            None,
            Some(runner),
            None,
            json!({}),
            now(),
        )
        .await;
        // The driver takes the pre-resolved parent row.
        let tx = Transaction::begin(&pool).await.expect("tx");
        let mut store = LiveCreationStore::new(tx, &pool, deps());
        let parent_view = CreationSeam::run(&mut store, parent)
            .await
            .expect("parent")
            .expect("row");
        let (tx, _, _, _, _, _) = store.into_parts();
        tx.rollback().await.expect("rollback");
        let result =
            create_and_dispatch_run(&pool, deps(), &dispatch_req(&graph, Some(parent_view)))
                .await
                .expect("creates");
        assert_eq!(result.outcome.reason, "created");
        let created = result.outcome.created_run.expect("created");
        let row = run_row(&pool, created).await;
        assert_eq!(row.1, Some(parent));
        assert_eq!(row.3, Some(runner));
        assert_eq!(result.dispatched, vec![created]);
        cleanup(
            &pool,
            graph,
            &[parent, created],
            &[runner],
            &[],
            &[],
            &[],
            &[],
        )
        .await;
    }

    #[tokio::test]
    #[ignore = "needs migrated scratch DB via DATABASE_URL"]
    async fn live_c3_fresh_session() {
        let pool = pool().await;
        let graph = seed_graph(&pool, "c3").await;
        sqlx::query("UPDATE issues SET state_id = $1 WHERE id = $2")
            .bind(graph.states["In Review"])
            .bind(graph.issue)
            .execute(&pool)
            .await
            .expect("move to review");
        let runner = seed_runner(&pool, &graph, graph.pod, "online").await;
        let parent = seed_run(
            &pool,
            &graph,
            "completed",
            "state_transition",
            "local_runner",
            "coding-task",
            None,
            Some(runner),
            None,
            json!({}),
            now(),
        )
        .await;
        let tx = Transaction::begin(&pool).await.expect("tx");
        let mut store = LiveCreationStore::new(tx, &pool, deps());
        let parent_view = CreationSeam::run(&mut store, parent)
            .await
            .expect("parent")
            .expect("row");
        let (tx, _, _, _, _, _) = store.into_parts();
        tx.rollback().await.expect("rollback");
        let mut req = dispatch_req(&graph, Some(parent_view));
        req.fresh_session = true;
        let result = create_and_dispatch_run(&pool, deps(), &req)
            .await
            .expect("creates");
        assert_eq!(result.outcome.reason, "created");
        let created = result.outcome.created_run.expect("created");
        let row = run_row(&pool, created).await;
        assert_eq!(row.1, None);
        assert_eq!(row.3, None);
        assert_eq!(row.6, "review");
        assert!(row.11.contains("You are reviewing"));
        assert_eq!(row.12.as_ref().unwrap().as_array().unwrap().len(), 10);
        cleanup(
            &pool,
            graph,
            &[parent, created],
            &[runner],
            &[],
            &[],
            &[],
            &[],
        )
        .await;
    }

    #[tokio::test]
    #[ignore = "needs migrated scratch DB via DATABASE_URL"]
    async fn live_c4_admission_error_failed() {
        let pool = pool().await;
        let graph = seed_graph(&pool, "c4").await;
        let cases = fixture(CREATE_DISPATCH);
        let case = &cases["cases"]["C4_admission_error_failed"];
        sqlx::query("UPDATE projects SET default_agent_executor = 'cloud_agent' WHERE id = $1")
            .bind(graph.project)
            .execute(&pool)
            .await
            .expect("cloud project");
        let mut settings = deps();
        settings.cloud.enabled = true;
        settings.cloud.github_tools_enabled = false;
        settings.cloud.max_queued_per_workspace = 0;
        settings.has_usable_llm_config = llm_ok;
        let mut req = dispatch_req(&graph, None);
        req.trigger = AgentRunTrigger::Tick.value().to_owned();
        let result = create_and_dispatch_run(&pool, settings, &req)
            .await
            .expect("failed, not error");
        assert_eq!(result.outcome.reason, case["reason"].as_str().unwrap());
        let created = result.outcome.created_run.expect("row exists");
        assert!(result.dispatched.is_empty());
        assert!(result.dispatch_decisions.is_empty());
        assert_eq!(result.terminal_effects, vec![created]);
        let row = run_row(&pool, created).await;
        let after = &case["after"];
        assert_eq!(row.0, after["status"].as_str().unwrap());
        assert_eq!(row.5, "tick");
        assert_eq!(row.7, "cloud_agent");
        assert_eq!(row.8, after["error_code"].as_str().unwrap());
        assert_eq!(row.9, after["error"].as_str().unwrap());
        assert_eq!(row.10, Some(now()));
        assert!(row.11.is_empty());
        assert_eq!(row.12, None);
        // The cloud terminal event lands exactly once.
        let events: Vec<(i32, String, Value)> = sqlx::query_as(
            "SELECT seq, kind, payload FROM agent_run_event WHERE agent_run_id = $1",
        )
        .bind(created)
        .fetch_all(&pool)
        .await
        .expect("events");
        assert_eq!(
            events,
            vec![(
                1,
                "terminal".to_owned(),
                json!({"status": "failed", "error_code": "run_quota_exceeded"})
            )]
        );
        cleanup(&pool, graph, &[created], &[], &[], &[], &[], &[]).await;
    }

    #[tokio::test]
    #[ignore = "needs migrated scratch DB via DATABASE_URL"]
    async fn live_c5_render_failed() {
        let pool = pool().await;
        let graph = seed_graph(&pool, "c5").await;
        let broken = seed_override(&pool, &graph, "autonomy", "{{ broken", None).await;
        let result = create_and_dispatch_run(&pool, deps(), &dispatch_req(&graph, None))
            .await
            .expect("failed, not error");
        assert_eq!(result.outcome.reason, "render-failed");
        let created = result.outcome.created_run.expect("row exists");
        assert!(result.dispatched.is_empty());
        assert_eq!(result.terminal_effects, vec![created]);
        let row = run_row(&pool, created).await;
        assert_eq!(row.0, "failed");
        assert_eq!(row.8, "prompt_build_failed");
        assert!(row.9.starts_with("prompt build failed: "));
        assert_eq!(row.10, Some(now()));
        assert!(row.11.is_empty());
        cleanup(&pool, graph, &[created], &[], &[], &[], &[broken], &[]).await;
    }

    #[tokio::test]
    #[ignore = "needs migrated scratch DB via DATABASE_URL"]
    async fn live_c6_executor_unavailable_no_run() {
        let pool = pool().await;
        let graph = seed_graph(&pool, "c6").await;
        sqlx::query("UPDATE projects SET default_agent_executor = 'fx6-no-executor' WHERE id = $1")
            .bind(graph.project)
            .execute(&pool)
            .await
            .expect("unknown executor");
        let result = create_and_dispatch_run(&pool, deps(), &dispatch_req(&graph, None))
            .await
            .expect("reason, not error");
        // The real D-11 `str(exc)` (`policy.py:60`); the fixture stubs
        // its own literal for the same shape.
        assert_eq!(result.outcome.reason, "unknown agent executor");
        assert_eq!(result.outcome.created_run, None);
        assert!(result.dispatched.is_empty());
        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM agent_run WHERE work_item_id = $1")
                .bind(graph.issue)
                .fetch_one(&pool)
                .await
                .expect("count");
        assert_eq!(count, 0);
        cleanup(&pool, graph, &[], &[], &[], &[], &[], &[]).await;
    }

    fn continuation_req(graph: &Graph, parent: RunView) -> ContinuationRequest {
        ContinuationRequest {
            issue_id: graph.issue,
            parent,
            creator_id: graph.user,
            pod_id: graph.pod,
            trigger: AgentRunTrigger::CommentAndRun.value().to_owned(),
            now: now(),
        }
    }

    #[tokio::test]
    #[ignore = "needs migrated scratch DB via DATABASE_URL"]
    async fn live_k1_created_with_parent_and_pin() {
        let pool = pool().await;
        let graph = seed_graph(&pool, "k1").await;
        let cases = fixture(CONTINUATION);
        let case = &cases["cases"]["K1_created_with_parent_and_pin"];
        let runner = seed_runner(&pool, &graph, graph.pod, "online").await;
        let parent = seed_run(
            &pool,
            &graph,
            "completed",
            "state_transition",
            "local_runner",
            "coding-task",
            None,
            Some(runner),
            None,
            json!({}),
            now(),
        )
        .await;
        let tx = Transaction::begin(&pool).await.expect("tx");
        let mut store = LiveCreationStore::new(tx, &pool, deps());
        let parent_view = CreationSeam::run(&mut store, parent)
            .await
            .expect("parent")
            .expect("row");
        let (tx, _, _, _, _, _) = store.into_parts();
        tx.rollback().await.expect("rollback");
        let result = create_continuation_run(&pool, deps(), &continuation_req(&graph, parent_view))
            .await
            .expect("creates");
        assert_eq!(result.outcome.reason, case["reason"].as_str().unwrap());
        assert_eq!(result.outcome.coalesced_into, None);
        let created = result.outcome.created_run.expect("created");
        assert_eq!(result.dispatched, vec![created]);
        let row = run_row(&pool, created).await;
        assert_eq!(row.0, "queued");
        assert_eq!(row.1, Some(parent));
        assert_eq!(row.3, Some(runner));
        assert_eq!(row.5, "comment_and_run");
        assert!(!row.11.is_empty());
        assert_eq!(row.12.as_ref().unwrap().as_array().unwrap().len(), 16);
        cleanup(
            &pool,
            graph,
            &[parent, created],
            &[runner],
            &[],
            &[],
            &[],
            &[],
        )
        .await;
    }

    async fn continuation_parent(pool: &PgPool, graph: &Graph) -> RunView {
        let parent = seed_run(
            pool,
            graph,
            "completed",
            "state_transition",
            "local_runner",
            "coding-task",
            None,
            None,
            None,
            json!({}),
            now(),
        )
        .await;
        let tx = Transaction::begin(pool).await.expect("tx");
        let mut store = LiveCreationStore::new(tx, pool, deps());
        let view = CreationSeam::run(&mut store, parent)
            .await
            .expect("parent")
            .expect("row");
        let (tx, _, _, _, _, _) = store.into_parts();
        tx.rollback().await.expect("rollback");
        view
    }

    #[tokio::test]
    #[ignore = "needs migrated scratch DB via DATABASE_URL"]
    async fn live_k2_admission_error_failed() {
        let pool = pool().await;
        let graph = seed_graph(&pool, "k2").await;
        let cases = fixture(CONTINUATION);
        let case = &cases["cases"]["K2_admission_error_failed"];
        sqlx::query("UPDATE projects SET default_agent_executor = 'cloud_agent' WHERE id = $1")
            .bind(graph.project)
            .execute(&pool)
            .await
            .expect("cloud project");
        let mut settings = deps();
        settings.cloud.enabled = true;
        settings.cloud.github_tools_enabled = false;
        settings.cloud.max_queued_per_workspace = 0;
        settings.has_usable_llm_config = llm_ok;
        let parent = continuation_parent(&pool, &graph).await;
        let parent_id = parent.id;
        let mut req = continuation_req(&graph, parent);
        req.trigger = AgentRunTrigger::Tick.value().to_owned();
        let result = create_continuation_run(&pool, settings, &req)
            .await
            .expect("failed, not error");
        assert_eq!(result.outcome.reason, case["reason"].as_str().unwrap());
        assert_eq!(result.outcome.coalesced_into, None);
        let created = result.outcome.created_run.expect("row exists");
        assert!(result.dispatched.is_empty());
        let row = run_row(&pool, created).await;
        assert_eq!(row.0, "failed");
        assert_eq!(row.8, "run_quota_exceeded");
        assert_eq!(row.10, Some(now()));
        cleanup(&pool, graph, &[parent_id, created], &[], &[], &[], &[], &[]).await;
    }

    #[tokio::test]
    #[ignore = "needs migrated scratch DB via DATABASE_URL"]
    async fn live_k3_render_failed() {
        let pool = pool().await;
        let graph = seed_graph(&pool, "k3").await;
        let broken = seed_override(&pool, &graph, "autonomy", "{{ broken", None).await;
        let parent = continuation_parent(&pool, &graph).await;
        let parent_id = parent.id;
        let result = create_continuation_run(&pool, deps(), &continuation_req(&graph, parent))
            .await
            .expect("failed, not error");
        assert_eq!(result.outcome.reason, "render-failed");
        let created = result.outcome.created_run.expect("row exists");
        let row = run_row(&pool, created).await;
        assert_eq!(row.8, "prompt_build_failed");
        cleanup(
            &pool,
            graph,
            &[parent_id, created],
            &[],
            &[],
            &[],
            &[broken],
            &[],
        )
        .await;
    }

    #[tokio::test]
    #[ignore = "needs migrated scratch DB via DATABASE_URL"]
    async fn live_k4_executor_unavailable_no_run() {
        let pool = pool().await;
        let graph = seed_graph(&pool, "k4").await;
        sqlx::query("UPDATE projects SET default_agent_executor = 'fx6-cloud-down' WHERE id = $1")
            .bind(graph.project)
            .execute(&pool)
            .await
            .expect("unknown executor");
        let parent = seed_run(
            &pool,
            &graph,
            "completed",
            "state_transition",
            "local_runner",
            "coding-task",
            None,
            None,
            None,
            json!({}),
            now(),
        )
        .await;
        let tx = Transaction::begin(&pool).await.expect("tx");
        let mut store = LiveCreationStore::new(tx, &pool, deps());
        let parent_view = CreationSeam::run(&mut store, parent)
            .await
            .expect("parent")
            .expect("row");
        let (tx, _, _, _, _, _) = store.into_parts();
        tx.rollback().await.expect("rollback");
        let result = create_continuation_run(&pool, deps(), &continuation_req(&graph, parent_view))
            .await
            .expect("reason, not error");
        assert_eq!(result.outcome.reason, "unknown agent executor");
        assert_eq!(result.outcome.created_run, None);
        cleanup(&pool, graph, &[parent], &[], &[], &[], &[], &[]).await;
    }

    fn handoff_marker(pod: Uuid, project: Uuid) -> Value {
        let mut marker = serde_json::Map::new();
        marker.insert("target_pod_id".to_owned(), json!(pod.to_string()));
        marker.insert("target_project_id".to_owned(), json!(project.to_string()));
        let mut config = serde_json::Map::new();
        config.insert("repo_url".to_owned(), json!("https://example.com/old.git"));
        config.insert(
            PROJECT_MOVE_HANDOFF_CONFIG_KEY.to_owned(),
            Value::Object(marker),
        );
        Value::Object(config)
    }

    fn handoff_marker_null_pod(project: Uuid) -> Value {
        let mut marker = serde_json::Map::new();
        marker.insert("target_pod_id".to_owned(), Value::Null);
        marker.insert("target_project_id".to_owned(), json!(project.to_string()));
        let mut config = serde_json::Map::new();
        config.insert(
            PROJECT_MOVE_HANDOFF_CONFIG_KEY.to_owned(),
            Value::Object(marker),
        );
        Value::Object(config)
    }

    #[tokio::test]
    #[ignore = "needs migrated scratch DB via DATABASE_URL"]
    async fn live_h2_happy_path() {
        let pool = pool().await;
        let graph = seed_graph(&pool, "h2").await;
        let cases = fixture(HANDOFF);
        let case = &cases["cases"]["H2_happy_path"];
        let source = seed_run(
            &pool,
            &graph,
            "cancelled",
            "state_transition",
            "local_runner",
            "coding-task",
            None,
            None,
            None,
            handoff_marker(graph.pod, graph.project),
            now(),
        )
        .await;
        let result = complete_project_move_handoff(&pool, deps(), source, now())
            .await
            .expect("handoff");
        let replacement = result.outcome.expect("replacement");
        assert_eq!(result.dispatched, vec![replacement.id]);
        assert!(result.terminal_effects.is_empty());
        assert!(matches!(
            result.dispatch_decisions.as_slice(),
            [(id, DispatchDecision::PodDrain { pod_id })]
                if *id == replacement.id && *pod_id == graph.pod
        ));
        let row = run_row(&pool, replacement.id).await;
        let want = &case["replacement"];
        assert_eq!(row.0, want["status"].as_str().unwrap());
        assert_eq!(row.1, Some(source));
        assert_eq!(row.3, None);
        assert_eq!(row.5, want["trigger"].as_str().unwrap());
        assert_eq!(row.6, want["phase_kind"].as_str().unwrap());
        assert_eq!(row.7, want["executor_kind"].as_str().unwrap());
        assert_eq!(row.14, json!({}));
        // The marker carries the replacement id; the replacement's own
        // config refreshed the snapshot and dropped the marker.
        let source_config: Value =
            sqlx::query_scalar("SELECT run_config FROM agent_run WHERE id = $1")
                .bind(source)
                .fetch_one(&pool)
                .await
                .expect("source config");
        assert_eq!(
            source_config[PROJECT_MOVE_HANDOFF_CONFIG_KEY]["replacement_run_id"],
            json!(replacement.id.to_string())
        );
        assert_eq!(
            row.13,
            json!({
                "repo_url": "https://example.com/fx6.git",
                "repo_ref": "main",
                "git_work_branch": null,
            })
        );
        assert!(!row.11.is_empty());
        assert_eq!(row.12.as_ref().unwrap().as_array().unwrap().len(), 16);
        cleanup(
            &pool,
            graph,
            &[source, replacement.id],
            &[],
            &[],
            &[],
            &[],
            &[],
        )
        .await;
    }

    #[tokio::test]
    #[ignore = "needs migrated scratch DB via DATABASE_URL"]
    async fn live_h3_replacement_id_idempotent() {
        let pool = pool().await;
        let graph = seed_graph(&pool, "h3").await;
        let source = seed_run(
            &pool,
            &graph,
            "cancelled",
            "state_transition",
            "local_runner",
            "coding-task",
            None,
            None,
            None,
            handoff_marker(graph.pod, graph.project),
            now(),
        )
        .await;
        let first = complete_project_move_handoff(&pool, deps(), source, now())
            .await
            .expect("handoff")
            .outcome
            .expect("replacement");
        let second = complete_project_move_handoff(&pool, deps(), source, now())
            .await
            .expect("handoff")
            .outcome
            .expect("replacement");
        assert_eq!(second.id, first.id);
        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM agent_run WHERE work_item_id = $1")
                .bind(graph.issue)
                .fetch_one(&pool)
                .await
                .expect("count");
        assert_eq!(count, 2);
        cleanup(&pool, graph, &[source, first.id], &[], &[], &[], &[], &[]).await;
    }

    #[tokio::test]
    #[ignore = "needs migrated scratch DB via DATABASE_URL"]
    async fn live_h4_moved_again_suppressed() {
        let pool = pool().await;
        let graph = seed_graph(&pool, "h4").await;
        let cases = fixture(HANDOFF);
        let case = &cases["cases"]["H4_moved_again_suppressed"];
        let source = seed_run(
            &pool,
            &graph,
            "cancelled",
            "state_transition",
            "local_runner",
            "coding-task",
            None,
            None,
            None,
            handoff_marker(graph.pod, Uuid::new_v4()),
            now(),
        )
        .await;
        let result = complete_project_move_handoff(&pool, deps(), source, now())
            .await
            .expect("handoff");
        assert_eq!(result.outcome, None);
        assert!(result.dispatched.is_empty());
        let stamped: Value = sqlx::query_scalar("SELECT run_config FROM agent_run WHERE id = $1")
            .bind(source)
            .fetch_one(&pool)
            .await
            .expect("stamped");
        assert_eq!(
            stamped[PROJECT_MOVE_HANDOFF_CONFIG_KEY]["suppressed"],
            case["marker_after"]["suppressed"]
        );
        cleanup(&pool, graph, &[source], &[], &[], &[], &[], &[]).await;
    }

    #[tokio::test]
    #[ignore = "needs migrated scratch DB via DATABASE_URL"]
    async fn live_h5_active_run_wins() {
        let pool = pool().await;
        let graph = seed_graph(&pool, "h5").await;
        let source = seed_run(
            &pool,
            &graph,
            "cancelled",
            "state_transition",
            "local_runner",
            "coding-task",
            None,
            None,
            None,
            handoff_marker(graph.pod, graph.project),
            now(),
        )
        .await;
        let winner = seed_run(
            &pool,
            &graph,
            "running",
            "run_ai",
            "local_runner",
            "coding-task",
            None,
            None,
            None,
            json!({}),
            now(),
        )
        .await;
        let result = complete_project_move_handoff(&pool, deps(), source, now())
            .await
            .expect("handoff");
        assert_eq!(result.outcome.map(|run| run.id), Some(winner));
        assert!(result.dispatched.is_empty());
        let stamped: Value = sqlx::query_scalar("SELECT run_config FROM agent_run WHERE id = $1")
            .bind(source)
            .fetch_one(&pool)
            .await
            .expect("stamped");
        assert_eq!(
            stamped[PROJECT_MOVE_HANDOFF_CONFIG_KEY]["replacement_run_id"],
            json!(winner.to_string())
        );
        cleanup(&pool, graph, &[source, winner], &[], &[], &[], &[], &[]).await;
    }

    #[tokio::test]
    #[ignore = "needs migrated scratch DB via DATABASE_URL"]
    async fn live_h6_no_target_pod() {
        let pool = pool().await;
        let graph = seed_graph(&pool, "h6").await;
        sqlx::query("UPDATE pod SET is_default = false WHERE id = $1")
            .bind(graph.pod)
            .execute(&pool)
            .await
            .expect("undefault pod");
        let source = seed_run(
            &pool,
            &graph,
            "cancelled",
            "state_transition",
            "local_runner",
            "coding-task",
            None,
            None,
            None,
            handoff_marker_null_pod(graph.project),
            now(),
        )
        .await;
        let result = complete_project_move_handoff(&pool, deps(), source, now())
            .await
            .expect("handoff");
        assert_eq!(result.outcome, None);
        cleanup(&pool, graph, &[source], &[], &[], &[], &[], &[]).await;
    }

    #[tokio::test]
    #[ignore = "needs migrated scratch DB via DATABASE_URL"]
    async fn live_h7_guards_return_none() {
        let pool = pool().await;
        let graph = seed_graph(&pool, "h7").await;
        // H7a: unknown run.
        let missing = complete_project_move_handoff(&pool, deps(), Uuid::new_v4(), now())
            .await
            .expect("handoff");
        assert_eq!(missing.outcome, None);
        // H7b: source not terminal.
        let running = seed_run(
            &pool,
            &graph,
            "running",
            "state_transition",
            "local_runner",
            "coding-task",
            None,
            None,
            None,
            handoff_marker(graph.pod, graph.project),
            now(),
        )
        .await;
        let live = complete_project_move_handoff(&pool, deps(), running, now())
            .await
            .expect("handoff");
        assert_eq!(live.outcome, None);
        // H7c: no marker.
        let unmarked = seed_run(
            &pool,
            &graph,
            "cancelled",
            "state_transition",
            "local_runner",
            "coding-task",
            None,
            None,
            None,
            json!({}),
            now(),
        )
        .await;
        let bare = complete_project_move_handoff(&pool, deps(), unmarked, now())
            .await
            .expect("handoff");
        assert_eq!(bare.outcome, None);
        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM agent_run WHERE work_item_id = $1")
                .bind(graph.issue)
                .fetch_one(&pool)
                .await
                .expect("count");
        assert_eq!(count, 2);
        cleanup(&pool, graph, &[running, unmarked], &[], &[], &[], &[], &[]).await;
    }

    #[tokio::test]
    #[ignore = "needs migrated scratch DB via DATABASE_URL"]
    async fn live_h8_executor_fallback_local() {
        let pool = pool().await;
        let graph = seed_graph(&pool, "h8").await;
        // Target project wants cloud, but the instance has none: the
        // executor seam falls back to a local row.
        sqlx::query("UPDATE projects SET default_agent_executor = 'cloud_agent' WHERE id = $1")
            .bind(graph.project)
            .execute(&pool)
            .await
            .expect("cloud project");
        let source = seed_run(
            &pool,
            &graph,
            "cancelled",
            "state_transition",
            "local_runner",
            "coding-task",
            None,
            None,
            None,
            handoff_marker(graph.pod, graph.project),
            now(),
        )
        .await;
        let result = complete_project_move_handoff(&pool, deps(), source, now())
            .await
            .expect("handoff");
        let replacement = result.outcome.expect("replacement");
        assert_eq!(result.dispatched, vec![replacement.id]);
        let row = run_row(&pool, replacement.id).await;
        assert_eq!(row.0, "queued");
        assert_eq!(row.7, "local_runner");
        assert_eq!(row.14, json!({}));
        assert!(!row.11.is_empty());
        cleanup(
            &pool,
            graph,
            &[source, replacement.id],
            &[],
            &[],
            &[],
            &[],
            &[],
        )
        .await;
    }

    #[tokio::test]
    #[ignore = "needs migrated scratch DB via DATABASE_URL"]
    async fn live_h9_active_race_returns_existing() {
        let pool = pool().await;
        let graph = seed_graph(&pool, "h9").await;
        let source = seed_run(
            &pool,
            &graph,
            "cancelled",
            "state_transition",
            "local_runner",
            "coding-task",
            None,
            None,
            None,
            handoff_marker(graph.pod, graph.project),
            now(),
        )
        .await;
        let racer = seed_run(
            &pool,
            &graph,
            "running",
            "run_ai",
            "local_runner",
            "coding-task",
            None,
            None,
            None,
            json!({}),
            now(),
        )
        .await;
        let tx = Transaction::begin(&pool).await.expect("tx");
        let mut store = LiveCreationStore::new(tx, &pool, deps());
        let parent = CreationSeam::run(&mut store, source)
            .await
            .expect("source")
            .expect("row");
        let (tx, _, _, _, _, _) = store.into_parts();
        tx.rollback().await.expect("rollback");
        let result = create_project_move_handoff_run(
            &pool,
            deps(),
            &HandoffCreateRequest {
                issue_id: graph.issue,
                parent,
                pod_id: graph.pod,
                now: now(),
            },
        )
        .await
        .expect("race");
        assert_eq!(result.outcome.id, racer);
        assert!(result.dispatched.is_empty());
        cleanup(&pool, graph, &[source, racer], &[], &[], &[], &[], &[]).await;
    }

    // -- live resolvers + parenting ----------------------------------------

    async fn resolve_creator(pool: &PgPool, issue: Uuid) -> Option<Uuid> {
        let tx = Transaction::begin(pool).await.expect("tx");
        let mut store = LiveCreationStore::new(tx, pool, deps());
        let out =
            pidash_services::orchestration::creation::resolve_fallback_creator(&mut store, issue)
                .await
                .expect("resolver");
        let (tx, _, _, _, _, _) = store.into_parts();
        tx.rollback().await.expect("rollback");
        out
    }

    async fn resolve_pod(pool: &PgPool, issue: Uuid) -> Option<Uuid> {
        let tx = Transaction::begin(pool).await.expect("tx");
        let mut store = LiveCreationStore::new(tx, pool, deps());
        let out =
            pidash_services::orchestration::creation::resolve_pod_for_issue(&mut store, issue)
                .await
                .expect("resolver");
        let (tx, _, _, _, _, _) = store.into_parts();
        tx.rollback().await.expect("rollback");
        out
    }

    #[tokio::test]
    #[ignore = "needs migrated scratch DB via DATABASE_URL"]
    async fn live_resolvers_r1_r6() {
        let pool = pool().await;
        let graph = seed_graph(&pool, "rslv").await;
        // R1: issue creator.
        assert_eq!(resolve_creator(&pool, graph.issue).await, Some(graph.user));
        // R2: project lead (same user, lead column).
        sqlx::query("UPDATE issues SET created_by_id = NULL WHERE id = $1")
            .bind(graph.issue)
            .execute(&pool)
            .await
            .expect("clear creator");
        sqlx::query("UPDATE projects SET project_lead_id = $1 WHERE id = $2")
            .bind(graph.user)
            .bind(graph.project)
            .execute(&pool)
            .await
            .expect("set lead");
        assert_eq!(resolve_creator(&pool, graph.issue).await, Some(graph.user));
        // R3: default assignee.
        sqlx::query(
            "UPDATE projects SET project_lead_id = NULL, default_assignee_id = $1 WHERE id = $2",
        )
        .bind(graph.user)
        .bind(graph.project)
        .execute(&pool)
        .await
        .expect("set assignee");
        assert_eq!(resolve_creator(&pool, graph.issue).await, Some(graph.user));
        // R4: none.
        sqlx::query("UPDATE projects SET default_assignee_id = NULL WHERE id = $1")
            .bind(graph.project)
            .execute(&pool)
            .await
            .expect("clear assignee");
        assert_eq!(resolve_creator(&pool, graph.issue).await, None);
        // R5: assigned pod wins.
        let extra = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO pod (id, created_at, updated_at, description, is_default, name, \
             project_id, workspace_id) VALUES ($1, $2, $2, '', false, 'fx6-extra', $3, $4)",
        )
        .bind(extra)
        .bind(now())
        .bind(graph.project)
        .bind(graph.ws)
        .execute(&pool)
        .await
        .expect("extra pod");
        sqlx::query("UPDATE issues SET assigned_pod_id = $1 WHERE id = $2")
            .bind(extra)
            .bind(graph.issue)
            .execute(&pool)
            .await
            .expect("assign pod");
        assert_eq!(resolve_pod(&pool, graph.issue).await, Some(extra));
        // R6: project default.
        sqlx::query("UPDATE issues SET assigned_pod_id = NULL WHERE id = $1")
            .bind(graph.issue)
            .execute(&pool)
            .await
            .expect("unassign pod");
        assert_eq!(resolve_pod(&pool, graph.issue).await, Some(graph.pod));
        // (R7 dangling FK + R8 project-less issue are in-memory-only
        // shapes — the DB FKs forbid them; the services suite covers
        // both against the fake.)
        cleanup(&pool, graph, &[], &[], &[], &[extra], &[], &[]).await;
    }

    #[tokio::test]
    #[ignore = "needs migrated scratch DB via DATABASE_URL"]
    async fn live_p5_handback_async() {
        let pool = pool().await;
        let graph = seed_graph(&pool, "p5").await;
        let latest = seed_run(
            &pool,
            &graph,
            "completed",
            "state_transition",
            "local_runner",
            "review",
            None,
            None,
            None,
            json!({}),
            now(),
        )
        .await;
        let resume = seed_run(
            &pool,
            &graph,
            "completed",
            "state_transition",
            "local_runner",
            "coding-task",
            None,
            None,
            None,
            json!({}),
            now(),
        )
        .await;
        let ticker = seed_ticker(&pool, &graph, Some(resume)).await;
        let tx = Transaction::begin(&pool).await.expect("tx");
        let mut store = LiveCreationStore::new(tx, &pool, deps());
        let (parent, fresh) = pidash_services::orchestration::creation::parent_for_next_run(
            &mut store,
            graph.issue,
            None,
            None,
        )
        .await
        .expect("parents");
        let (tx, _, _, _, _, _) = store.into_parts();
        tx.rollback().await.expect("rollback");
        assert!(!fresh);
        assert_eq!(parent.map(|run| run.id), Some(resume));
        assert_ne!(latest, resume);
        cleanup(
            &pool,
            graph,
            &[latest, resume],
            &[],
            &[],
            &[],
            &[],
            &[ticker],
        )
        .await;
    }

    #[tokio::test]
    #[ignore = "needs migrated scratch DB via DATABASE_URL"]
    async fn live_unknown_trigger_reads_carry_raw_value() {
        // Django never validates agent_run.trigger on read: TextChoices
        // are choices-only (no DB check), migration 0029 documents
        // blocker_completed rows reading back as an unknown trigger, and
        // the contract harness seeds trigger='human'. Every read path
        // must carry such values through instead of failing to decode.
        let pool = pool().await;
        let graph = seed_graph(&pool, "trig-raw").await;
        let id = seed_run(
            &pool,
            &graph,
            "running",
            "human",
            "local_runner",
            "coding-task",
            None,
            None,
            None,
            json!({}),
            now(),
        )
        .await;
        let tx = Transaction::begin(&pool).await.expect("tx");
        let mut store = LiveCreationStore::new(tx, &pool, deps());
        let active = CreationSeam::active_run_for(&mut store, graph.issue)
            .await
            .expect("active read tolerates unknown trigger");
        assert_eq!(active.as_ref().map(|run| run.id), Some(id));
        assert_eq!(
            active.as_ref().map(|run| run.trigger.as_str()),
            Some("human")
        );
        let row = CreationSeam::run(&mut store, id)
            .await
            .expect("direct read tolerates unknown trigger")
            .expect("row");
        assert_eq!(row.trigger, "human");
        let prior = CreationSeam::latest_prior_run(&mut store, graph.issue)
            .await
            .expect("prior read tolerates unknown trigger")
            .expect("row");
        assert_eq!(prior.trigger, "human");
        let (tx, _, _, _, _, _) = store.into_parts();
        tx.rollback().await.expect("rollback");
        cleanup(&pool, graph, &[id], &[], &[], &[], &[], &[]).await;
    }

    // -- offline: the migrated statements ------------------------------------

    #[test]
    fn statement_shapes_pin_joins_filters_order_and_aliases() {
        use sea_orm::{DbBackend, QueryTrait};
        let issue = Uuid::new_v4();
        let labels = labels_query(issue).build(DbBackend::Postgres).to_string();
        assert!(labels.contains("INNER JOIN"), "{labels}");
        assert!(
            labels.contains("\"labels\".\"deleted_at\" IS NULL"),
            "{labels}"
        );
        assert!(labels.contains("DESC"), "{labels}");
        assert!(labels.contains("AS \"name\""), "{labels}");

        let assignees = assignees_query(issue)
            .build(DbBackend::Postgres)
            .to_string();
        assert!(assignees.contains("INNER JOIN"), "{assignees}");
        assert!(assignees.contains("AS \"display_name\""), "{assignees}");
        assert!(assignees.contains("DESC"), "{assignees}");

        let children = children_query(issue).build(DbBackend::Postgres).to_string();
        assert!(
            children.contains("INNER JOIN") && children.contains("LEFT JOIN"),
            "{children}"
        );
        assert!(children.contains("<> 'triage'"), "{children}");
        assert!(children.contains("AS \"state_name\""), "{children}");
        assert!(children.contains("ASC"), "{children}");

        let comments = comments_query(issue).build(DbBackend::Postgres).to_string();
        assert!(comments.contains("LEFT JOIN"), "{comments}");
        assert!(comments.contains("AS \"labels\""), "{comments}");
        assert!(comments.contains("ASC"), "{comments}");

        let remote = remote_query(issue).build(DbBackend::Postgres).to_string();
        assert!(remote.contains("INNER JOIN"), "{remote}");
        assert!(remote.contains("LIMIT 1"), "{remote}");

        let parent = parent_query(issue).build(DbBackend::Postgres).to_string();
        assert!(parent.contains("COUNT(*)"), "{parent}");
        assert!(parent.contains("AS \"comments_count\""), "{parent}");
    }

    // -- live: new SeaORM queries over temp-table shadows ---------------------

    async fn shadow_pool() -> Option<PgPool> {
        let Ok(url) = std::env::var("DATABASE_URL") else {
            eprintln!("skipping temp-table test: DATABASE_URL is not set");
            return None;
        };
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .expect("connect scratch database");
        let migrated: Option<String> =
            sqlx::query_scalar("SELECT to_regclass('public.issues')::text")
                .fetch_one(&pool)
                .await
                .expect("probe");
        if migrated.is_none() {
            eprintln!("skipping temp-table test: the database is not migrated");
            return None;
        }
        Some(pool)
    }

    async fn shadow_tables(tx: &mut Transaction<'_>, tables: &[&str]) {
        for table in tables {
            let ddl = format!(
                "CREATE TEMP TABLE {table} ON COMMIT DROP AS SELECT * FROM public.{table} WITH NO DATA"
            );
            sqlx::query(&ddl)
                .execute(&mut **tx.inner())
                .await
                .expect("temp table");
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn seed_shadow_issue(
        tx: &mut Transaction<'_>,
        project: Uuid,
        parent: Uuid,
        id: Uuid,
        name: &str,
        state: Option<Uuid>,
        archived: bool,
        draft: bool,
        created: DateTime<Utc>,
    ) {
        let archived_at: Option<chrono::NaiveDate> = archived.then(|| created.date_naive());
        sqlx::query("INSERT INTO issues (id, name, sequence_id, project_id, parent_id, state_id, deleted_at, archived_at, is_draft, created_at, updated_at) VALUES ($1, $2, 1, $3, $4, $5, NULL, $6, $7, $8, $8)")
            .bind(id).bind(name).bind(project).bind(parent).bind(state)
            .bind(archived_at)
            .bind(draft).bind(created)
            .execute(&mut **tx.inner()).await.expect("issue");
    }

    #[tokio::test]
    async fn live_labels_join_scopes_and_orders() {
        let Some(pool) = shadow_pool().await else {
            return;
        };
        let mut tx = Transaction::begin(&pool).await.expect("tx");
        shadow_tables(&mut tx, &["issue_labels", "labels"]).await;
        let issue = Uuid::new_v4();
        let (older, newer, deleted) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        let early = now();
        let late = now() + chrono::Duration::hours(1);
        let stamps = [early, late, late + chrono::Duration::hours(1)];
        for (id, name, stamp, deleted_at) in [
            (older, "older", stamps[0], None),
            (newer, "newer", stamps[1], None),
            (deleted, "gone", stamps[2], Some(stamps[2])),
        ] {
            sqlx::query("INSERT INTO labels (id, name, created_at, updated_at, deleted_at) VALUES ($1, $2, $3, $3, $4)")
                .bind(id).bind(name).bind(stamp).bind(deleted_at)
                .execute(&mut **tx.inner()).await.expect("label");
        }
        // The soft-deleted link still renders; the deleted label does not.
        sqlx::query("INSERT INTO issue_labels (id, issue_id, label_id, created_at, updated_at, deleted_at) VALUES ($1, $2, $3, $4, $4, $5)")
            .bind(Uuid::new_v4()).bind(issue).bind(older).bind(early).bind(Some(early))
            .execute(&mut **tx.inner()).await.expect("link");
        sqlx::query("INSERT INTO issue_labels (id, issue_id, label_id, created_at, updated_at, deleted_at) VALUES ($1, $2, $3, $4, $4, NULL)")
            .bind(Uuid::new_v4()).bind(issue).bind(newer).bind(late)
            .execute(&mut **tx.inner()).await.expect("link");
        sqlx::query("INSERT INTO issue_labels (id, issue_id, label_id, created_at, updated_at, deleted_at) VALUES ($1, $2, $3, $4, $4, NULL)")
            .bind(Uuid::new_v4()).bind(issue).bind(deleted).bind(stamps[2])
            .execute(&mut **tx.inner()).await.expect("link");
        let names = bundle_labels(&mut tx, issue).await.expect("labels");
        assert_eq!(names, vec!["newer".to_owned(), "older".to_owned()]);
        tx.rollback().await.expect("rollback");
    }

    #[tokio::test]
    async fn live_children_excludes_triage_stateless_draft_archived() {
        let Some(pool) = shadow_pool().await else {
            return;
        };
        let mut tx = Transaction::begin(&pool).await.expect("tx");
        shadow_tables(&mut tx, &["issues", "projects", "states"]).await;
        let project = Uuid::new_v4();
        let parent = Uuid::new_v4();
        let todo = Uuid::new_v4();
        let triage = Uuid::new_v4();
        let at = now();
        sqlx::query("INSERT INTO projects (id, identifier, archived_at) VALUES ($1, 'T', NULL)")
            .bind(project)
            .execute(&mut **tx.inner())
            .await
            .expect("project");
        for (id, name, group) in [(todo, "To do", "todo"), (triage, "Triage", "triage")] {
            sqlx::query("INSERT INTO states (id, name, \"group\") VALUES ($1, $2, $3)")
                .bind(id)
                .bind(name)
                .bind(group)
                .execute(&mut **tx.inner())
                .await
                .expect("state");
        }
        seed_shadow_issue(
            &mut tx, project, parent, parent, "parent", None, false, false, at,
        )
        .await;
        seed_shadow_issue(
            &mut tx,
            project,
            parent,
            Uuid::new_v4(),
            "kept",
            Some(todo),
            false,
            false,
            at + chrono::Duration::minutes(1),
        )
        .await;
        seed_shadow_issue(
            &mut tx,
            project,
            parent,
            Uuid::new_v4(),
            "triage",
            Some(triage),
            false,
            false,
            at + chrono::Duration::minutes(2),
        )
        .await;
        seed_shadow_issue(
            &mut tx,
            project,
            parent,
            Uuid::new_v4(),
            "stateless",
            None,
            false,
            false,
            at + chrono::Duration::minutes(3),
        )
        .await;
        seed_shadow_issue(
            &mut tx,
            project,
            parent,
            Uuid::new_v4(),
            "draft",
            Some(todo),
            false,
            true,
            at + chrono::Duration::minutes(4),
        )
        .await;
        seed_shadow_issue(
            &mut tx,
            project,
            parent,
            Uuid::new_v4(),
            "archived",
            Some(todo),
            true,
            false,
            at + chrono::Duration::minutes(5),
        )
        .await;
        let children = bundle_children(&mut tx, parent).await.expect("children");
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].title, "kept");
        tx.rollback().await.expect("rollback");
    }

    #[tokio::test]
    async fn live_comments_excludes_fold_and_keeps_order() {
        let Some(pool) = shadow_pool().await else {
            return;
        };
        let mut tx = Transaction::begin(&pool).await.expect("tx");
        shadow_tables(&mut tx, &["issue_comments", "users"]).await;
        let issue = Uuid::new_v4();
        let at = now();
        for (body, labels, minutes) in [
            ("first", vec!["note"], 1),
            ("folded", vec!["fold"], 2),
            ("third", Vec::<&str>::new(), 3),
        ] {
            sqlx::query("INSERT INTO issue_comments (id, comment_stripped, speaker_type, speaker_label, issue_id, labels, created_at, updated_at) VALUES ($1, $2, 'human', '', $3, $4, $5, $5)")
                .bind(Uuid::new_v4()).bind(body).bind(issue).bind(labels)
                .bind(at + chrono::Duration::minutes(minutes))
                .execute(&mut **tx.inner()).await.expect("comment");
        }
        let comments = bundle_comments(&mut tx, issue).await.expect("comments");
        assert_eq!(comments.len(), 2);
        assert_eq!(comments[0].body, "first");
        assert_eq!(comments[1].body, "third");
        tx.rollback().await.expect("rollback");
    }

    #[tokio::test]
    async fn live_overrides_scope_to_workspace_and_user() {
        let Some(pool) = shadow_pool().await else {
            return;
        };
        let mut tx = Transaction::begin(&pool).await.expect("tx");
        shadow_tables(&mut tx, &["prompt_section_override"]).await;
        let (workspace, user, other) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        for (key, owner, active) in [
            ("ws", None, true),
            ("user", Some(user), true),
            ("other", Some(other), true),
            ("off", None, false),
        ] {
            sqlx::query("INSERT INTO prompt_section_override (id, section_key, body, is_active, version, workspace_id, user_id) VALUES ($1, $2, 'b', $3, 1, $4, $5)")
                .bind(Uuid::new_v4()).bind(key).bind(active).bind(workspace).bind(owner)
                .execute(&mut **tx.inner()).await.expect("override");
        }
        let scoped = bundle_overrides(&mut tx, workspace, Some(user))
            .await
            .expect("overrides");
        let mut keys: Vec<&str> = scoped.iter().map(|row| row.section_key.as_str()).collect();
        keys.sort_unstable();
        assert_eq!(keys, vec!["user", "ws"]);
        let anon = bundle_overrides(&mut tx, workspace, None)
            .await
            .expect("overrides");
        assert_eq!(anon.len(), 1);
        assert_eq!(anon[0].section_key, "ws");
        tx.rollback().await.expect("rollback");
    }

    #[tokio::test]
    async fn live_parent_counts_only_live_comments() {
        let Some(pool) = shadow_pool().await else {
            return;
        };
        let mut tx = Transaction::begin(&pool).await.expect("tx");
        shadow_tables(&mut tx, &["issues", "issue_comments"]).await;
        let parent = Uuid::new_v4();
        sqlx::query("INSERT INTO issues (id, name, sequence_id, project_id, state_id, git_work_branch, description_stripped, created_at, updated_at) VALUES ($1, 'parent', 7, $2, NULL, 'br', NULL, $3, $3)")
            .bind(parent).bind(Uuid::new_v4()).bind(now())
            .execute(&mut **tx.inner()).await.expect("parent issue");
        for (body, deleted) in [("live", None), ("gone", Some(now()))] {
            sqlx::query("INSERT INTO issue_comments (id, comment_stripped, speaker_type, speaker_label, issue_id, labels, created_at, updated_at, deleted_at) VALUES ($1, $2, 'human', '', $3, '{}', $4, $4, $5)")
                .bind(Uuid::new_v4()).bind(body).bind(parent).bind(now()).bind(deleted)
                .execute(&mut **tx.inner()).await.expect("comment");
        }
        let ancestors = vec![
            ChainNode {
                id: Uuid::new_v4(),
                project_identifier: "T".to_owned(),
                title: "leaf".to_owned(),
            },
            ChainNode {
                id: parent,
                project_identifier: "T".to_owned(),
                title: "parent".to_owned(),
            },
        ];
        let (view, _lineage) = bundle_parent_and_lineage(&mut tx, &ancestors)
            .await
            .expect("parent");
        let view = view.expect("parent view");
        assert_eq!(view.comments_count, 1);
        tx.rollback().await.expect("rollback");
    }
}
