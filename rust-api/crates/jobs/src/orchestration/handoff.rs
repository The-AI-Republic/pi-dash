//! Request-path project-move handoff executor (OVL-06, PIDASHCONV-1544).
//!
//! [`complete_project_move_handoff_inline`] is the api crate's handoff
//! executor, transplanted here verbatim: own transaction over
//! [`HandoffStore`], committed, then the handoff tx's own post-commit
//! effects — the terminal pairs (queued copy plus the inline apply
//! under the crum user) and the pod drains. It differs from
//! [`super::runner_seams::complete_project_move_handoff_live`] (worker
//! path, enqueue-only) by design (PIDASHCONV-1514): a replacement-run
//! failure comment keeps `created_by_id` NULL until the worker posts
//! it without the inline apply, so the request path applies inline.
//!
//! Isolated in Python (the terminal transition already committed), so
//! callers log failures. Re-exported through `pidash-sdk::ops`.

use std::future::Future;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use sea_orm::sea_query::{Expr, Func, SimpleExpr};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, Condition, EntityTrait, FromQueryResult, JoinType,
    PaginatorTrait, QueryFilter, QueryOrder, QuerySelect, RelationTrait, Set,
};
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

use pidash_compat::datetime::render_isoformat as twin_render_isoformat;
use pidash_db::config::{CloudAgentSettings, ManagedRunnerSettings, RunnerSettings};
use pidash_db::dispatch::status::AgentRunStatus as DbAgentRunStatus;
use pidash_db::entity::{
    agent_run, agent_run_event, assistant_user_llm_config, dev_machine, git_code_review_links,
    git_repositories, git_repository_bindings, issue_agent_ticker, issue_assignees, issue_comments,
    issue_labels, issue_relations, issues, labels, pod, projects, prompt_section_override, runner,
    states, users, workspaces,
};
use pidash_db::orm_tx::TxConn;
use pidash_db::pool::sea_orm_connection;
use pidash_db::runner_runs::agent_run::merge_error_details;
use pidash_db::tx::Transaction as DbTransaction;
use pidash_services::assistant::seams as assistant_seams;
use pidash_services::dispatch::admission::RedisAdmissionCache;
use pidash_services::dispatch::{
    consume_admission_token, DeferredConsume, LlmProfile, UserFlags as SvcUserFlags,
};
use pidash_services::extensions::{AssistantModelSeam, CloudAgentToolsetsSeam};
use pidash_services::orchestration::creation::{self as creation_kernel, user_id_for_run};
use pidash_services::orchestration::creation::{
    finalize_error_details_patch, AdmissionError as ServiceAdmissionError, CreationError,
    CreationSeam, ExecutionError, ExecutionFields as ServiceExecutionFields, ExecutionRequest,
    FinalizeAgentRunSeam, IssueView, LockedIssue, NewAgentRun, PodView, ProjectView, RenderBundle,
    RunView, RunnerView, StateView, TickerBudget,
};
use pidash_services::prompting::{composer, context};
use pidash_services::runner_runs::finalization as finalize_kernel;
use pidash_services::runner_runs::LifecycleEffect;
use pidash_services::runner_sessions::live_drains::drain_pod_by_id;
use pidash_types::dispatch::AgentExecutorKind;

use crate::dispatch::{
    dispatch_after_commit as collect_dispatch, dispatch_agent_run,
    execution_fields as resolve_execution_fields, lock_cloud_creation_capacity as lock_capacity,
    ActorScope, DeferredAdmissionError, DispatchDecision, ExecutionFieldsError, ExecutionInputs,
    ExecutionSeams, ProjectScope,
};

/// `timezone.now()` truncated to microseconds (Django datetimes are
/// microsecond-exact; Postgres would round stored nanos).
fn now_micros() -> DateTime<Utc> {
    let now = Utc::now();
    DateTime::from_timestamp_micros(now.timestamp_micros()).expect("micros in range")
}

/// `int(time.time())` for the admission bucket clock.
fn unix_now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0)
}

/// The request-path handoff facts a [`HandoffStore`] is built from:
/// executor settings, redis/runner planes, the seams, and the clock.
/// The api caller builds this from the request state; the overlay
/// builds it from its own.
#[derive(Clone)]
pub struct HandoffInlineDeps {
    /// Cloud-agent executor settings.
    pub cloud: CloudAgentSettings,
    /// Managed-runner executor settings.
    pub managed: ManagedRunnerSettings,
    /// Shared Redis client for admission buckets and pod drains.
    pub redis: Option<redis::Client>,
    /// Runner-plane settings for pod drains.
    pub runner: RunnerSettings,
    /// Installed toolsets seam.
    pub toolsets: Arc<dyn CloudAgentToolsetsSeam>,
    /// Installed assistant-model seam.
    pub assistant_seam: Arc<dyn AssistantModelSeam>,
}

/// Drain one publish effect, isolated with its failure log
/// (`agent_run_finalization.py` `_publish_effects`: the emit and the
/// inline apply sit in separate `try` blocks). The drain itself is a
/// caller-supplied callback (the api caller drains through its ports;
/// the callback returns the drain `Result`).
pub async fn drain_publish_effects<F, Fut, T, E>(effects: Vec<LifecycleEffect>, mut drain_one: F)
where
    F: FnMut(LifecycleEffect) -> Fut,
    Fut: Future<Output = Result<T, E>>,
{
    if effects.is_empty() {
        return;
    }
    for effect in effects {
        let label = match &effect {
            LifecycleEffect::PublishTerminalEffects { run_id } => {
                format!("failed to publish terminal effects for run {run_id}")
            }
            LifecycleEffect::ApplyTerminalEffectsInline { run_id } => {
                format!("failed to apply terminal effects for run {run_id}")
            }
            unexpected => format!("failed to drain unexpected terminal effect: {unexpected:?}"),
        };
        if drain_one(effect).await.is_err() {
            tracing::error!("{label}");
        }
    }
}

/// Pool-backed [`CreationSeam`] + [`FinalizeAgentRunSeam`]: the api-side
/// twin of `jobs::LiveCreationStore` (same builders, same order, same
/// decodes — cited per method). The jobs store cannot serve api
/// requests (its `StoreDeps` closures are sync and have no production
/// factory). The api span factory (`api::orchestration`) lends it a
/// caller-owned span transaction; the D-12 decisions stay in the
/// services driver in both.
pub struct HandoffStore<'t, 'p> {
    tx: DbTransaction<'t>,
    pool: &'p PgPool,
    cloud: CloudAgentSettings,
    managed: ManagedRunnerSettings,
    cache: RedisAdmissionCache,
    now_unix_secs: i64,
    toolsets: Arc<dyn CloudAgentToolsetsSeam>,
    assistant_seam: Arc<dyn AssistantModelSeam>,
    dispatches: Vec<Uuid>,
    deferred: Vec<DeferredConsume>,
    terminal_effects: Vec<Uuid>,
}

impl<'t, 'p> HandoffStore<'t, 'p> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        tx: DbTransaction<'t>,
        pool: &'p PgPool,
        cloud: CloudAgentSettings,
        managed: ManagedRunnerSettings,
        cache: RedisAdmissionCache,
        now_unix_secs: i64,
        toolsets: Arc<dyn CloudAgentToolsetsSeam>,
        assistant_seam: Arc<dyn AssistantModelSeam>,
    ) -> Self {
        Self {
            tx,
            pool,
            cloud,
            managed,
            cache,
            now_unix_secs,
            toolsets,
            assistant_seam,
            dispatches: Vec::new(),
            deferred: Vec::new(),
            terminal_effects: Vec::new(),
        }
    }

    fn db(error: sea_orm::DbErr) -> CreationError {
        CreationError::Db(error.to_string())
    }

    pub fn into_parts(
        self,
    ) -> (
        DbTransaction<'t>,
        Vec<Uuid>,
        Vec<DeferredConsume>,
        Vec<Uuid>,
        RedisAdmissionCache,
    ) {
        (
            self.tx,
            self.dispatches,
            self.deferred,
            self.terminal_effects,
            self.cache,
        )
    }
}

type Moment = sea_orm::prelude::DateTimeWithTimeZone;

fn twin_decode_error(column: &str, value: &str) -> CreationError {
    CreationError::Db(format!("unknown agent_run.{column} {value:?}"))
}

fn twin_map_status(value: String) -> Result<DbAgentRunStatus, CreationError> {
    DbAgentRunStatus::from_value(&value).ok_or_else(|| twin_decode_error("status", &value))
}

fn twin_map_executor(value: String) -> Result<AgentExecutorKind, CreationError> {
    // Strictness is safe here (unlike the trigger): the
    // `agent_run_cloud_has_no_local_assignment` check constraint
    // (`runner/models.py:1065-1075`, migration 0024) admits only the
    // three members, so an unknown stored value is unreachable.
    AgentExecutorKind::from_value(&value).ok_or_else(|| twin_decode_error("executor_kind", &value))
}

#[derive(Debug, FromQueryResult)]
struct IssueRow {
    id: Uuid,
    workspace_id: Uuid,
    project_id: Option<Uuid>,
    state_id: Option<Uuid>,
    parent_id: Option<Uuid>,
    created_by_id: Option<Uuid>,
    assigned_pod_id: Option<Uuid>,
    agent_executor: Option<String>,
    git_work_branch: Option<String>,
    workpad: Option<String>,
    name: Option<String>,
    description_stripped: Option<String>,
    priority: Option<String>,
    sequence_id: i32,
    target_date: Option<chrono::NaiveDate>,
}

#[derive(Debug, FromQueryResult)]
struct ProjectRow {
    id: Uuid,
    workspace_id: Uuid,
    identifier: String,
    name: String,
    description: Option<String>,
    repo_url: Option<String>,
    base_branch: Option<String>,
    default_agent_executor: String,
    project_lead_id: Option<Uuid>,
    default_assignee_id: Option<Uuid>,
    agent_default_max_ticks: i32,
    agent_default_interval_seconds: i32,
    agent_review_default_interval_seconds: i32,
    agent_test_default_interval_seconds: i32,
}

#[derive(Debug, FromQueryResult)]
struct StateRow {
    id: Uuid,
    name: String,
    group: String,
}

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
    ended_at: Option<Moment>,
}

fn run_view_from_row(row: RunRow) -> Result<RunView, CreationError> {
    let status = twin_map_status(row.status)?;
    let executor_kind = twin_map_executor(row.executor_kind)?;
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
        ended_at: row.ended_at.map(|m| m.with_timezone(&Utc)),
    })
}

fn run_view_from_model(m: agent_run::Model) -> Result<RunView, CreationError> {
    let status = twin_map_status(m.status)?;
    let executor_kind = twin_map_executor(m.executor_kind)?;
    Ok(RunView {
        id: m.id,
        workspace_id: m.workspace_id,
        created_by_id: m.created_by_id,
        pod_id: m.pod_id,
        runner_id: m.runner_id,
        pinned_runner_id: m.pinned_runner_id,
        parent_run_id: m.parent_run_id,
        work_item_id: m.work_item_id,
        status,
        trigger: m.trigger,
        executor_kind,
        phase_kind: m.phase_kind,
        run_config: m.run_config,
        tool_plan: m.tool_plan,
        error_details: m.error_details,
        prompt: m.prompt,
        prompt_manifest: m.prompt_manifest,
        ended_at: m.ended_at.map(|m| m.with_timezone(&Utc)),
    })
}

fn issue_view_from_row(row: IssueRow) -> IssueView {
    IssueView {
        id: row.id,
        workspace_id: row.workspace_id,
        project_id: row.project_id,
        state_id: row.state_id,
        parent_id: row.parent_id,
        created_by_id: row.created_by_id,
        assigned_pod_id: row.assigned_pod_id,
        agent_executor: row.agent_executor,
        git_work_branch: row.git_work_branch,
        workpad: row.workpad,
        name: row.name,
        description_stripped: row.description_stripped,
        priority: row.priority,
        sequence_id: row.sequence_id,
        target_date: row.target_date,
    }
}

fn project_view_from_row(row: ProjectRow) -> ProjectView {
    ProjectView {
        id: row.id,
        workspace_id: row.workspace_id,
        identifier: row.identifier,
        name: row.name,
        description: row.description,
        repo_url: row.repo_url,
        base_branch: row.base_branch,
        default_agent_executor: row.default_agent_executor,
        project_lead_id: row.project_lead_id,
        default_assignee_id: row.default_assignee_id,
        pool: i64::from(row.agent_default_max_ticks),
        interval_impl: i64::from(row.agent_default_interval_seconds),
        interval_review: i64::from(row.agent_review_default_interval_seconds),
        interval_test: i64::from(row.agent_test_default_interval_seconds),
    }
}

fn issue_select_query(issue_id: Uuid) -> sea_orm::Select<issues::Entity> {
    issues::Entity::find_by_id(issue_id).select_only().columns([
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
    ])
}

fn project_select_query(project_id: Uuid) -> sea_orm::Select<projects::Entity> {
    projects::Entity::find_by_id(project_id)
        .select_only()
        .columns([
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
        ])
}

fn state_select_query(state_id: Uuid) -> sea_orm::Select<states::Entity> {
    states::Entity::find_by_id(state_id).select_only().columns([
        states::Column::Id,
        states::Column::Name,
        states::Column::Group,
    ])
}

fn run_columns_select(q: sea_orm::Select<agent_run::Entity>) -> sea_orm::Select<agent_run::Entity> {
    q.select_only().columns([
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
    ])
}

fn run_select_query(run_id: Uuid) -> sea_orm::Select<agent_run::Entity> {
    run_columns_select(agent_run::Entity::find_by_id(run_id))
}

fn active_status_values() -> Vec<String> {
    pidash_db::orchestration::runs::ACTIVE_STATUSES
        .iter()
        .map(|s| s.value().to_owned())
        .collect()
}

fn terminal_status_values() -> Vec<String> {
    [
        DbAgentRunStatus::Completed,
        DbAgentRunStatus::Failed,
        DbAgentRunStatus::Cancelled,
        DbAgentRunStatus::Blocked,
        DbAgentRunStatus::Refused,
    ]
    .iter()
    .map(|s| s.value().to_owned())
    .collect()
}

impl CreationSeam for HandoffStore<'_, '_> {
    async fn issue(&mut self, issue_id: Uuid) -> Result<IssueView, CreationError> {
        let conn = TxConn::new(self.tx.inner());
        let row = issue_select_query(issue_id)
            .into_model::<IssueRow>()
            .one(&conn)
            .await
            .map_err(Self::db)?
            .ok_or_else(|| CreationError::Db("row not found".to_owned()))?;
        Ok(issue_view_from_row(row))
    }

    async fn project(&mut self, project_id: Uuid) -> Result<ProjectView, CreationError> {
        let conn = TxConn::new(self.tx.inner());
        let row = project_select_query(project_id)
            .into_model::<ProjectRow>()
            .one(&conn)
            .await
            .map_err(Self::db)?
            .ok_or_else(|| CreationError::Db("row not found".to_owned()))?;
        Ok(project_view_from_row(row))
    }

    async fn state(&mut self, state_id: Option<Uuid>) -> Result<Option<StateView>, CreationError> {
        let Some(state_id) = state_id else {
            return Ok(None);
        };
        let conn = TxConn::new(self.tx.inner());
        let row = state_select_query(state_id)
            .into_model::<StateRow>()
            .one(&conn)
            .await
            .map_err(Self::db)?;
        match row {
            Some(row) => Ok(Some(StateView {
                id: row.id,
                name: row.name,
                group: row.group,
            })),
            None => Err(CreationError::MissingRow(format!("no state {state_id}"))),
        }
    }

    async fn latest_prior_run(&mut self, issue_id: Uuid) -> Result<Option<RunView>, CreationError> {
        let conn = TxConn::new(self.tx.inner());
        let row = run_columns_select(
            agent_run::Entity::find()
                .filter(agent_run::Column::WorkItemId.eq(issue_id))
                .order_by_desc(agent_run::Column::CreatedAt)
                .limit(1),
        )
        .into_model::<RunRow>()
        .one(&conn)
        .await
        .map_err(Self::db)?;
        row.map(run_view_from_row).transpose()
    }

    async fn active_run_for(&mut self, issue_id: Uuid) -> Result<Option<RunView>, CreationError> {
        let conn = TxConn::new(self.tx.inner());
        let row = run_columns_select(
            agent_run::Entity::find()
                .filter(agent_run::Column::WorkItemId.eq(issue_id))
                .filter(agent_run::Column::Status.is_in(active_status_values()))
                .order_by_desc(agent_run::Column::CreatedAt)
                .limit(1),
        )
        .into_model::<RunRow>()
        .one(&conn)
        .await
        .map_err(Self::db)?;
        row.map(run_view_from_row).transpose()
    }

    async fn run(&mut self, run_id: Uuid) -> Result<Option<RunView>, CreationError> {
        let conn = TxConn::new(self.tx.inner());
        let row = run_select_query(run_id)
            .into_model::<RunRow>()
            .one(&conn)
            .await
            .map_err(Self::db)?;
        row.map(run_view_from_row).transpose()
    }

    async fn runner(&mut self, runner_id: Uuid) -> Result<Option<RunnerView>, CreationError> {
        #[derive(Debug, FromQueryResult)]
        struct RunnerRow {
            id: Uuid,
            pod_id: Uuid,
            status: String,
            last_heartbeat_at: Option<Moment>,
            paused_at: Option<Moment>,
            provisioning: String,
            pod_dispatch: String,
            dev_machine_id: Option<Uuid>,
        }
        let conn = TxConn::new(self.tx.inner());
        let row = runner::Entity::find_by_id(runner_id)
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
            .into_model::<RunnerRow>()
            .one(&conn)
            .await
            .map_err(Self::db)?;
        let engine_paused = match row.as_ref().and_then(|r| r.dev_machine_id) {
            Some(machine_id) => {
                let machine = dev_machine::Entity::find_by_id(machine_id)
                    .one(&conn)
                    .await
                    .map_err(Self::db)?;
                machine.is_some_and(|m| {
                    m.engine_paused_by_owner || !m.engine_auto_pause_reason.is_empty()
                })
            }
            None => false,
        };
        Ok(row.map(|row| RunnerView {
            id: row.id,
            pod_id: row.pod_id,
            status: row.status,
            last_heartbeat_at: row.last_heartbeat_at.map(|m| m.with_timezone(&Utc)),
            paused_at: row.paused_at.map(|m| m.with_timezone(&Utc)),
            provisioning: row.provisioning,
            pod_dispatch: row.pod_dispatch,
            engine_paused,
        }))
    }

    async fn assigned_pod(&mut self, pod_id: Uuid) -> Result<Option<PodView>, CreationError> {
        #[derive(Debug, FromQueryResult)]
        struct PodRow {
            id: Uuid,
            project_id: Uuid,
        }
        let conn = TxConn::new(self.tx.inner());
        let row = pod::Entity::find_by_id(pod_id)
            .select_only()
            .columns([pod::Column::Id, pod::Column::ProjectId])
            .filter(pod::Column::DeletedAt.is_null())
            .into_model::<PodRow>()
            .one(&conn)
            .await
            .map_err(Self::db)?;
        Ok(row.map(|row| PodView {
            id: row.id,
            project_id: row.project_id,
        }))
    }

    async fn default_pod_for_project(
        &mut self,
        project_id: Uuid,
    ) -> Result<Option<PodView>, CreationError> {
        #[derive(Debug, FromQueryResult)]
        struct PodRow {
            id: Uuid,
            project_id: Uuid,
        }
        let conn = TxConn::new(self.tx.inner());
        let row = pod::Entity::find()
            .select_only()
            .columns([pod::Column::Id, pod::Column::ProjectId])
            .filter(pod::Column::ProjectId.eq(project_id))
            .filter(pod::Column::IsDefault.eq(true))
            .filter(pod::Column::DeletedAt.is_null())
            .order_by_asc(pod::Column::Id)
            .into_model::<PodRow>()
            .one(&conn)
            .await
            .map_err(Self::db)?;
        Ok(row.map(|row| PodView {
            id: row.id,
            project_id: row.project_id,
        }))
    }

    async fn resume_parent_run_id(
        &mut self,
        issue_id: Uuid,
    ) -> Result<Option<Uuid>, CreationError> {
        #[derive(Debug, FromQueryResult)]
        struct TickerRow {
            resume_parent_run_id: Option<Uuid>,
        }
        let conn = TxConn::new(self.tx.inner());
        let row = issue_agent_ticker::Entity::find()
            .select_only()
            .columns([issue_agent_ticker::Column::ResumeParentRunId])
            .filter(issue_agent_ticker::Column::IssueId.eq(issue_id))
            .into_model::<TickerRow>()
            .one(&conn)
            .await
            .map_err(Self::db)?;
        Ok(row.and_then(|row| row.resume_parent_run_id))
    }

    async fn work_item_id_for_run(&mut self, run_id: Uuid) -> Result<Option<Uuid>, CreationError> {
        #[derive(Debug, FromQueryResult)]
        struct WorkItemRow {
            work_item_id: Option<Uuid>,
        }
        let conn = TxConn::new(self.tx.inner());
        let row = agent_run::Entity::find_by_id(run_id)
            .select_only()
            .columns([agent_run::Column::WorkItemId])
            .into_model::<WorkItemRow>()
            .one(&conn)
            .await
            .map_err(Self::db)?;
        Ok(row.and_then(|row| row.work_item_id))
    }

    async fn lock_issue_for_handoff(
        &mut self,
        issue_id: Uuid,
    ) -> Result<Option<LockedIssue>, CreationError> {
        #[allow(dead_code)]
        #[derive(Debug, FromQueryResult)]
        struct LockRow {
            id: Uuid,
        }
        let locked = {
            let conn = TxConn::new(self.tx.inner());
            issues::Entity::find_by_id(issue_id)
                .select_only()
                .column(issues::Column::Id)
                .lock_exclusive()
                .into_model::<LockRow>()
                .one(&conn)
                .await
                .map_err(Self::db)?
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
        let conn = TxConn::new(self.tx.inner());
        let row = run_columns_select(agent_run::Entity::find_by_id(run_id))
            .lock_exclusive()
            .into_model::<RunRow>()
            .one(&conn)
            .await
            .map_err(Self::db)?;
        row.map(run_view_from_row).transpose()
    }

    async fn user_flags(&mut self, user_id: Uuid) -> Result<SvcUserFlags, CreationError> {
        #[derive(Debug, FromQueryResult)]
        struct FlagsRow {
            is_active: bool,
            is_bot: bool,
        }
        let conn = TxConn::new(self.tx.inner());
        let row = users::Entity::find_by_id(user_id)
            .select_only()
            .columns([users::Column::IsActive, users::Column::IsBot])
            .into_model::<FlagsRow>()
            .one(&conn)
            .await
            .map_err(Self::db)?
            .ok_or_else(|| CreationError::Db("row not found".to_owned()))?;
        Ok(SvcUserFlags {
            is_active: row.is_active,
            is_bot: row.is_bot,
        })
    }

    async fn insert_run(&mut self, row: &NewAgentRun) -> Result<RunView, CreationError> {
        let conn = TxConn::new(self.tx.inner());
        let model = agent_run::ActiveModel {
            id: Set(row.id),
            workspace_id: Set(row.workspace_id),
            owner_id: Set(None),
            created_by_id: Set(row.created_by_id),
            pod_id: Set(row.pod_id),
            runner_id: Set(None),
            pinned_runner_id: Set(row.pinned_runner_id),
            work_item_id: Set(Some(row.work_item_id)),
            scheduler_binding_id: Set(None),
            parent_run_id: Set(row.parent_run_id),
            status: Set("queued".to_owned()),
            executor_kind: Set(row.executor_kind.value().to_owned()),
            dispatch_attempts: Set(0),
            cancel_requested_at: Set(None),
            cancel_reason: Set(String::new()),
            tool_plan: Set(row.tool_plan.clone()),
            terminal_hooks_applied_at: Set(None),
            terminal_capacity_released_at: Set(None),
            prompt: Set(String::new()),
            trigger: Set(row.trigger.clone()),
            prompt_manifest: Set(None),
            phase_kind: Set(row.phase_kind.clone()),
            run_config: Set(row.run_config.clone()),
            required_capabilities: Set(serde_json::json!([])),
            thread_id: Set(String::new()),
            agent_metadata: Set(serde_json::json!({})),
            lease_expires_at: Set(None),
            done_payload: Set(None),
            error_details: Set(row.error_details.clone()),
            llm_model: Set(String::new()),
            usage: Set(serde_json::json!({})),
            created_at: Set(row.now.into()),
            assigned_at: Set(None),
            started_at: Set(None),
            ended_at: Set(None),
            ..Default::default()
        };
        let inserted = model.insert(&conn).await.map_err(Self::db)?;
        run_view_from_model(inserted)
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
            .map_err(Self::db)?;
        Ok(())
    }

    async fn save_run_config(&mut self, run_id: Uuid, config: &Value) -> Result<(), CreationError> {
        let conn = TxConn::new(self.tx.inner());
        agent_run::Entity::update_many()
            .col_expr(agent_run::Column::RunConfig, Expr::value(config.clone()))
            .filter(agent_run::Column::Id.eq(run_id))
            .exec(&conn)
            .await
            .map_err(Self::db)?;
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
        // The jobs store's H/E/P closures have no production factory, so
        // the twin prefetches their facts asynchronously and moves the
        // values into the `FnOnce` seams (identical verdicts).
        let has_key = match actor_id {
            Some(id) => twin_has_api_key(self.pool, id)
                .await
                .map_err(|detail| ExecutionError::Store(CreationError::Db(detail)))?,
            None => false,
        };
        let profile = {
            let mapped = assistant_seams::agent_model_profile_for_user(has_key);
            LlmProfile {
                available: mapped.available,
                reason_code: mapped.reason_code,
            }
        };
        // The driver short-circuits a missing actor before consulting
        // the closure, so only a present actor consults the seam (and a
        // missing one never pays for the overlay's reads). Clone the seam
        // out: the store is `!Sync`, so the consult must not borrow
        // `self` across the await.
        let assistant_seam = self.assistant_seam.clone();
        let has_llm = match actor_id {
            Some(id) => twin_gate_allows(&assistant_seam, id, has_key).await,
            None => false,
        };
        let Self {
            pool,
            cache,
            cloud,
            managed,
            now_unix_secs,
            toolsets,
            deferred,
            ..
        } = self;
        let mut short = DbTransaction::begin(pool)
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
        let mut collect = |consume: DeferredConsume| deferred.push(consume);
        let fields = resolve_execution_fields(
            &mut short,
            &inputs,
            cloud,
            managed,
            cache,
            &mut collect,
            ExecutionSeams {
                has_usable_llm_config: move || has_llm,
                toolsets: toolsets.clone(),
                llm_profile: move || profile,
            },
        )
        .await;
        short
            .commit()
            .await
            .map_err(|error| ExecutionError::Store(CreationError::Db(error.to_string())))?;
        fields
            .map(twin_map_execution_fields)
            .map_err(twin_map_execution_error)
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
            Ok(deferred) => Ok(deferred.map(twin_map_admission_error)),
            Err(ExecutionFieldsError::Admission(exc)) => {
                Err(CreationError::CapacityRefused(exc.detail().to_owned()))
            }
            Err(ExecutionFieldsError::Db(error)) => Err(CreationError::Db(error.to_string())),
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
        twin_load_render_bundle(
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

fn twin_map_execution_fields(fields: crate::dispatch::ExecutionFields) -> ServiceExecutionFields {
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
        cloud_admission_error: fields.cloud_admission_error.map(twin_map_admission_error),
    }
}

fn twin_map_execution_error(error: ExecutionFieldsError) -> ExecutionError {
    match error {
        ExecutionFieldsError::Db(error) => {
            ExecutionError::Store(CreationError::Db(error.to_string()))
        }
        ExecutionFieldsError::Admission(exc) => ExecutionError::Refused(exc.detail().to_owned()),
        other => ExecutionError::Refused(other.to_string()),
    }
}

fn twin_map_admission_error(error: DeferredAdmissionError) -> ServiceAdmissionError {
    ServiceAdmissionError {
        code: error.code,
        detail: error.detail,
    }
}

/// `get_config(user)` (`llm.py:67-68`):
/// `UserLLMConfig.objects.filter(user).first()` — full row, unordered
/// (`LIMIT 1`, no `ORDER BY`). SeaORM form: `user_id` filtered,
/// `LIMIT 1`.
/// `get_config(user)` + `has_api_key` (`models.py:260-262`): key present
/// and non-empty.
async fn twin_has_api_key(pool: &PgPool, user_id: Uuid) -> Result<bool, String> {
    #[derive(Debug, FromQueryResult)]
    struct KeyRow {
        api_key_encrypted: Option<Vec<u8>>,
    }
    let db = sea_orm_connection(pool);
    let row = assistant_user_llm_config::Entity::find()
        .select_only()
        .column(assistant_user_llm_config::Column::ApiKeyEncrypted)
        .filter(assistant_user_llm_config::Column::UserId.eq(user_id))
        .limit(1)
        .into_model::<KeyRow>()
        .one(&db)
        .await
        .map_err(|error| error.to_string())?;
    let Some(row) = row else {
        return Ok(false);
    };
    Ok(row.api_key_encrypted.is_some_and(|key| !key.is_empty()))
}

/// The handoff llm gate through the installed assistant seam
/// (`user_has_llm_config`, `agent_execution.py:69-80`): the CE default
/// answers the BYOK key bit; the overlay routes OpenHub/Codex/BYOK
/// through the same single switch point Pi Dash AI uses (PIDASHCONV-1051).
/// A free function over the cloned seam: the store is `!Sync` (it owns
/// the span transaction), so the consult must not borrow `self` across
/// the await, or the handlers' futures stop being `Send`.
async fn twin_gate_allows(
    assistant_seam: &Arc<dyn AssistantModelSeam>,
    actor_id: Uuid,
    has_key: bool,
) -> bool {
    assistant_seam
        .has_usable_llm_config(actor_id, has_key)
        .await
}

fn twin_db(error: sea_orm::DbErr) -> CreationError {
    CreationError::Db(error.to_string())
}

fn bundle_labels_query(issue_id: Uuid) -> sea_orm::Select<issue_labels::Entity> {
    issue_labels::Entity::find()
        .select_only()
        .column_as(labels::Column::Name, "name")
        .join(JoinType::InnerJoin, issue_labels::Relation::Labels.def())
        .filter(issue_labels::Column::IssueId.eq(issue_id))
        .filter(labels::Column::DeletedAt.is_null())
        .order_by_desc(labels::Column::CreatedAt)
}

async fn twin_bundle_labels(
    tx: &mut DbTransaction<'_>,
    issue_id: Uuid,
) -> Result<Vec<String>, CreationError> {
    #[derive(Debug, FromQueryResult)]
    struct LabelRow {
        name: String,
    }
    let conn = TxConn::new(tx.inner());
    let rows = bundle_labels_query(issue_id)
        .into_model::<LabelRow>()
        .all(&conn)
        .await
        .map_err(twin_db)?;
    Ok(rows.into_iter().map(|row| row.name).collect())
}

fn bundle_assignees_query(issue_id: Uuid) -> sea_orm::Select<issue_assignees::Entity> {
    issue_assignees::Entity::find()
        .select_only()
        .column_as(users::Column::DisplayName, "display_name")
        .column_as(users::Column::Email, "email")
        .join(
            JoinType::InnerJoin,
            issue_assignees::Entity::belongs_to(users::Entity)
                .from(issue_assignees::Column::AssigneeId)
                .to(users::Column::Id)
                .into(),
        )
        .filter(issue_assignees::Column::IssueId.eq(issue_id))
        .order_by_desc(users::Column::CreatedAt)
}

async fn twin_bundle_assignees(
    tx: &mut DbTransaction<'_>,
    issue_id: Uuid,
) -> Result<Vec<String>, CreationError> {
    #[derive(Debug, FromQueryResult)]
    struct AssigneeRow {
        display_name: Option<String>,
        email: Option<String>,
    }
    let conn = TxConn::new(tx.inner());
    let rows = bundle_assignees_query(issue_id)
        .into_model::<AssigneeRow>()
        .all(&conn)
        .await
        .map_err(twin_db)?;
    Ok(rows
        .into_iter()
        .map(|row| context::assignee_display(row.display_name.as_deref(), row.email.as_deref()))
        .collect())
}

fn bundle_project_states_query(project_id: Uuid) -> sea_orm::Select<states::Entity> {
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
}

async fn twin_bundle_project_states(
    tx: &mut DbTransaction<'_>,
    project_id: Uuid,
) -> Result<Vec<context::ProjectStateView>, CreationError> {
    #[derive(Debug, FromQueryResult)]
    struct ProjectStateRow {
        name: String,
        group: String,
        description: Option<String>,
    }
    let conn = TxConn::new(tx.inner());
    let rows = bundle_project_states_query(project_id)
        .into_model::<ProjectStateRow>()
        .all(&conn)
        .await
        .map_err(twin_db)?;
    Ok(rows
        .into_iter()
        .map(|row| context::ProjectStateView {
            name: row.name,
            group: row.group,
            description: row.description,
        })
        .collect())
}

fn bundle_children_query(issue_id: Uuid) -> sea_orm::Select<issues::Entity> {
    issues::Entity::find()
        .select_only()
        .column(issues::Column::Id)
        .column(issues::Column::Name)
        .column(issues::Column::SequenceId)
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

async fn twin_bundle_children(
    tx: &mut DbTransaction<'_>,
    issue_id: Uuid,
) -> Result<Vec<context::IssueRef>, CreationError> {
    #[allow(dead_code)]
    #[derive(Debug, FromQueryResult)]
    struct ChildRow {
        id: Uuid,
        name: Option<String>,
        sequence_id: i32,
        project_identifier: String,
        state_name: Option<String>,
    }
    let conn = TxConn::new(tx.inner());
    let rows = bundle_children_query(issue_id)
        .into_model::<ChildRow>()
        .all(&conn)
        .await
        .map_err(twin_db)?;
    Ok(rows
        .into_iter()
        .map(|row| context::IssueRef {
            identifier: context::issue_identifier(
                &row.project_identifier,
                i64::from(row.sequence_id),
            ),
            title: row.name.unwrap_or_default(),
            state: row.state_name.unwrap_or_default(),
        })
        .collect())
}

fn bundle_relations_query(issue_id: Uuid) -> sea_orm::Select<issue_relations::Entity> {
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
}

fn bundle_relation_targets_query(other_ids: Vec<Uuid>) -> sea_orm::Select<issues::Entity> {
    issues::Entity::find()
        .select_only()
        .column(issues::Column::Id)
        .column(issues::Column::Name)
        .column(issues::Column::SequenceId)
        .column_as(projects::Column::Identifier, "project_identifier")
        .column_as(states::Column::Name, "state_name")
        .column_as(states::Column::Group, "state_group")
        .join(JoinType::InnerJoin, issues::Relation::Projects.def())
        .join(JoinType::LeftJoin, issues::Relation::States.def())
        .filter(issues::Column::Id.is_in(other_ids))
        .filter(issues::Column::DeletedAt.is_null())
}

async fn twin_bundle_relations(
    tx: &mut DbTransaction<'_>,
    issue_id: Uuid,
) -> Result<
    (
        Vec<context::RelationRow>,
        std::collections::HashMap<String, context::IssueRef>,
        std::collections::HashMap<String, context::DirectionalRef>,
    ),
    CreationError,
> {
    #[derive(Debug, FromQueryResult)]
    struct RelationRow {
        issue_id: Uuid,
        related_issue_id: Uuid,
        relation_type: String,
    }
    #[derive(Debug, FromQueryResult)]
    struct TargetRow {
        id: Uuid,
        name: Option<String>,
        sequence_id: i32,
        project_identifier: String,
        state_name: Option<String>,
        state_group: Option<String>,
    }
    let rows = {
        let conn = TxConn::new(tx.inner());
        bundle_relations_query(issue_id)
            .into_model::<RelationRow>()
            .all(&conn)
            .await
            .map_err(twin_db)?
    };
    let mut other_ids: Vec<Uuid> = Vec::new();
    let mut seen_ids = std::collections::HashSet::new();
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
    let targets = if other_ids.is_empty() {
        Vec::new()
    } else {
        let conn = TxConn::new(tx.inner());
        bundle_relation_targets_query(other_ids)
            .into_model::<TargetRow>()
            .all(&conn)
            .await
            .map_err(twin_db)?
    };
    let mut refs = std::collections::HashMap::new();
    let mut directional_refs = std::collections::HashMap::new();
    for target in targets {
        let key = target.id.to_string();
        let title = target.name.unwrap_or_default();
        let state_name = target.state_name.unwrap_or_default();
        refs.insert(
            key.clone(),
            context::IssueRef {
                identifier: context::issue_identifier(
                    &target.project_identifier,
                    i64::from(target.sequence_id),
                ),
                title: title.clone(),
                state: state_name.clone(),
            },
        );
        directional_refs.insert(
            key,
            context::DirectionalRef {
                identifier: context::issue_identifier(
                    &target.project_identifier,
                    i64::from(target.sequence_id),
                ),
                title,
                state: state_name,
                state_group: target.state_group.unwrap_or_default(),
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

fn bundle_comments_query(issue_id: Uuid) -> sea_orm::Select<issue_comments::Entity> {
    // The `fold` exclusion rows are filtered in memory (see
    // `twin_bundle_comments`): `labels` is selected alongside, and the
    // caller drops rows whose labels contain `fold`. Selecting the
    // column keeps the result identical to `NOT (labels @> …)` without
    // a custom operator.
    issue_comments::Entity::find()
        .select_only()
        .column(issue_comments::Column::CommentStripped)
        .column(issue_comments::Column::SpeakerType)
        .column(issue_comments::Column::SpeakerLabel)
        .column(issue_comments::Column::SpeakerAgentRunId)
        .column(issue_comments::Column::CreatedAt)
        .column(issue_comments::Column::Labels)
        .column_as(users::Column::DisplayName, "display_name")
        .column_as(users::Column::Email, "email")
        .column_as(users::Column::Username, "username")
        .column_as(users::Column::IsBot, "is_bot")
        .join(
            JoinType::LeftJoin,
            issue_comments::Entity::belongs_to(users::Entity)
                .from(issue_comments::Column::ActorId)
                .to(users::Column::Id)
                .into(),
        )
        .filter(issue_comments::Column::IssueId.eq(issue_id))
        .filter(issue_comments::Column::DeletedAt.is_null())
        .order_by_asc(issue_comments::Column::CreatedAt)
}

async fn twin_bundle_comments(
    tx: &mut DbTransaction<'_>,
    issue_id: Uuid,
) -> Result<Vec<context::CommentView>, CreationError> {
    #[derive(Debug, FromQueryResult)]
    struct CommentRow {
        comment_stripped: Option<String>,
        speaker_type: Option<String>,
        speaker_label: Option<String>,
        speaker_agent_run_id: Option<Uuid>,
        created_at: Option<Moment>,
        labels: Vec<String>,
        display_name: Option<String>,
        email: Option<String>,
        username: Option<String>,
        is_bot: Option<bool>,
    }
    let conn = TxConn::new(tx.inner());
    let rows = bundle_comments_query(issue_id)
        .into_model::<CommentRow>()
        .all(&conn)
        .await
        .map_err(twin_db)?;
    Ok(rows
        .into_iter()
        .filter(|row| !row.labels.contains(&"fold".to_owned()))
        .map(|row| {
            let display = row.display_name;
            let email = row.email;
            let username = row.username;
            let is_bot = row.is_bot;
            let actor = match (&display, &email, &username, &is_bot) {
                (None, None, None, None) => None,
                _ => Some(context::ActorView {
                    display_name: display,
                    email,
                    username,
                    is_bot: is_bot.unwrap_or(false),
                }),
            };
            context::CommentView {
                body: row.comment_stripped.unwrap_or_default(),
                speaker_type: row.speaker_type.unwrap_or_default(),
                speaker_label: row.speaker_label,
                actor,
                created_at_iso: row
                    .created_at
                    .map(|m| twin_render_isoformat(m.with_timezone(&Utc))),
                run_id: row.speaker_agent_run_id.map(|id| id.to_string()),
            }
        })
        .collect())
}

fn bundle_reviews_query(issue_id: Uuid) -> sea_orm::Select<git_code_review_links::Entity> {
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
}

async fn twin_bundle_reviews(
    tx: &mut DbTransaction<'_>,
    issue_id: Uuid,
) -> Result<Vec<context::CodeReviewView>, CreationError> {
    #[derive(Debug, FromQueryResult)]
    struct ReviewRow {
        url: String,
        title: Option<String>,
        state: String,
        merged: bool,
        draft: bool,
        provider: String,
        external_iid: String,
    }
    let conn = TxConn::new(tx.inner());
    let rows = bundle_reviews_query(issue_id)
        .into_model::<ReviewRow>()
        .all(&conn)
        .await
        .map_err(twin_db)?;
    Ok(rows
        .into_iter()
        .map(|row| context::CodeReviewView {
            url: row.url,
            title: row.title,
            state: row.state,
            merged: row.merged,
            draft: row.draft,
            provider: row.provider,
            external_iid: row.external_iid,
        })
        .collect())
}

fn bundle_remote_query(project_id: Uuid) -> sea_orm::Select<git_repository_bindings::Entity> {
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
}

async fn twin_bundle_remote(
    tx: &mut DbTransaction<'_>,
    project_id: Uuid,
) -> Result<(Option<context::RemoteView>, Option<context::AdapterNames>), CreationError> {
    #[derive(Debug, FromQueryResult)]
    struct RemoteRow {
        provider: String,
        host_url: String,
        full_name: String,
    }
    let row = {
        let conn = TxConn::new(tx.inner());
        bundle_remote_query(project_id)
            .limit(1)
            .into_model::<RemoteRow>()
            .one(&conn)
            .await
            .map_err(twin_db)?
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

struct TwinChainNode {
    id: Uuid,
    project_identifier: String,
    title: String,
}

fn ancestor_hop_query(id: Uuid) -> sea_orm::Select<issues::Entity> {
    issues::Entity::find_by_id(id).select_only().columns([
        issues::Column::Name,
        issues::Column::ProjectId,
        issues::Column::ParentId,
    ])
}

fn project_identifier_query(project_id: Uuid) -> sea_orm::Select<projects::Entity> {
    projects::Entity::find_by_id(project_id)
        .select_only()
        .column(projects::Column::Identifier)
}

fn sequence_query(issue_id: Uuid) -> sea_orm::Select<issues::Entity> {
    issues::Entity::find_by_id(issue_id)
        .select_only()
        .column(issues::Column::SequenceId)
}

fn parent_description_query(issue_id: Uuid) -> sea_orm::Select<issues::Entity> {
    issues::Entity::find_by_id(issue_id)
        .select_only()
        .column(issues::Column::DescriptionStripped)
}

fn parent_cols_query(issue_id: Uuid) -> sea_orm::Select<issues::Entity> {
    // `(SELECT COUNT(*) FROM issue_comments WHERE issue_id = i.id AND
    // deleted_at IS NULL)` as a correlated scalar subquery, so the
    // statement stays one query.
    let outer_id: SimpleExpr = Expr::col((issues::Entity, issues::Column::Id)).into();
    let count_sub = {
        let stmt = sea_orm::sea_query::Query::select()
            .from(issue_comments::Entity)
            .expr(Func::count(Expr::col(sea_orm::sea_query::Asterisk)))
            .and_where(
                Expr::col((issue_comments::Entity, issue_comments::Column::IssueId)).eq(outer_id),
            )
            .and_where(
                Expr::col((issue_comments::Entity, issue_comments::Column::DeletedAt)).is_null(),
            )
            .to_owned();
        stmt
    };
    let count_expr = SimpleExpr::SubQuery(None, Box::new(count_sub.into_sub_query_statement()));
    issues::Entity::find_by_id(issue_id)
        .select_only()
        .columns([
            issues::Column::Name,
            issues::Column::StateId,
            issues::Column::GitWorkBranch,
        ])
        .expr_as(count_expr, "comments_count")
}

async fn twin_bundle_ancestors(
    tx: &mut DbTransaction<'_>,
    issue: &IssueView,
    project_identifier: &str,
) -> Result<Vec<TwinChainNode>, CreationError> {
    #[derive(Debug, FromQueryResult)]
    struct HopRow {
        name: Option<String>,
        project_id: Option<Uuid>,
        parent_id: Option<Uuid>,
    }
    #[derive(Debug, FromQueryResult)]
    struct IdentifierRow {
        identifier: String,
    }
    let mut chain = vec![TwinChainNode {
        id: issue.id,
        project_identifier: project_identifier.to_owned(),
        title: issue.name.clone().unwrap_or_default(),
    }];
    let mut seen = std::collections::HashSet::from([issue.id]);
    let mut next = issue.parent_id;
    while let Some(id) = next {
        if chain.len() >= 50 || !seen.insert(id) {
            break;
        }
        let row = {
            let conn = TxConn::new(tx.inner());
            ancestor_hop_query(id)
                .into_model::<HopRow>()
                .one(&conn)
                .await
                .map_err(twin_db)?
        };
        let Some(row) = row else {
            break;
        };
        let (name, project_id, parent_id) = (row.name, row.project_id, row.parent_id);
        let project_identifier = match project_id {
            Some(pid) => {
                let conn = TxConn::new(tx.inner());
                let found = project_identifier_query(pid)
                    .into_model::<IdentifierRow>()
                    .one(&conn)
                    .await
                    .map_err(twin_db)?;
                found.map(|row| row.identifier).unwrap_or_default()
            }
            None => String::new(),
        };
        chain.push(TwinChainNode {
            id,
            project_identifier,
            title: name.unwrap_or_default(),
        });
        next = parent_id;
    }
    Ok(chain)
}

async fn load_sequence(tx: &mut DbTransaction<'_>, issue_id: Uuid) -> Result<i64, CreationError> {
    #[derive(Debug, FromQueryResult)]
    struct SequenceRow {
        sequence_id: i32,
    }
    let conn = TxConn::new(tx.inner());
    let row = sequence_query(issue_id)
        .into_model::<SequenceRow>()
        .one(&conn)
        .await
        .map_err(twin_db)?;
    Ok(row
        .map(|row| i64::from(row.sequence_id))
        .unwrap_or_default())
}

async fn twin_bundle_parent_and_lineage(
    tx: &mut DbTransaction<'_>,
    ancestors: &[TwinChainNode],
) -> Result<(Option<context::ParentView>, Vec<context::LineageNode>), CreationError> {
    #[derive(Debug, FromQueryResult)]
    struct ParentColsRow {
        name: Option<String>,
        state_id: Option<Uuid>,
        git_work_branch: Option<String>,
        comments_count: i64,
    }
    #[derive(Debug, FromQueryResult)]
    struct DescriptionRow {
        description_stripped: Option<String>,
    }
    let parent = match ancestors.get(1) {
        None => None,
        Some(node) => {
            let row = {
                let conn = TxConn::new(tx.inner());
                parent_cols_query(node.id)
                    .into_model::<ParentColsRow>()
                    .one(&conn)
                    .await
                    .map_err(twin_db)?
            };
            match row {
                None => None,
                Some(row) => {
                    let state_name = match row.state_id {
                        Some(id) => {
                            let conn = TxConn::new(tx.inner());
                            state_select_query(id)
                                .into_model::<StateRow>()
                                .one(&conn)
                                .await
                                .map_err(twin_db)?
                                .map(|state| state.name)
                        }
                        None => None,
                    };
                    let description = {
                        let conn = TxConn::new(tx.inner());
                        parent_description_query(node.id)
                            .into_model::<DescriptionRow>()
                            .one(&conn)
                            .await
                            .map_err(twin_db)?
                            .and_then(|row| row.description_stripped)
                    };
                    let sequence = load_sequence(tx, node.id).await?;
                    Some(context::ParentView {
                        identifier: context::issue_identifier(&node.project_identifier, sequence),
                        title: row.name,
                        state_name,
                        work_branch: row.git_work_branch,
                        description_stripped: description,
                        comments_count: row.comments_count,
                    })
                }
            }
        }
    };
    let lineage = if ancestors.len() > 2 {
        let mut nodes = Vec::new();
        for node in ancestors {
            let sequence = load_sequence(tx, node.id).await?;
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

fn done_payload_query(run_id: Uuid) -> sea_orm::Select<agent_run::Entity> {
    agent_run::Entity::find_by_id(run_id)
        .select_only()
        .column(agent_run::Column::DonePayload)
}

async fn twin_bundle_done_payload(
    tx: &mut DbTransaction<'_>,
    run_id: Uuid,
) -> Result<Option<Value>, CreationError> {
    #[derive(Debug, FromQueryResult)]
    struct DonePayloadRow {
        done_payload: Option<Value>,
    }
    let conn = TxConn::new(tx.inner());
    let row = done_payload_query(run_id)
        .into_model::<DonePayloadRow>()
        .one(&conn)
        .await
        .map_err(twin_db)?;
    Ok(row.and_then(|row| row.done_payload))
}

fn bundle_overrides_query(
    workspace_id: Uuid,
    user_id: Option<Uuid>,
) -> sea_orm::Select<prompt_section_override::Entity> {
    let q = prompt_section_override::Entity::find()
        .select_only()
        .columns([
            prompt_section_override::Column::SectionKey,
            prompt_section_override::Column::Body,
            prompt_section_override::Column::Version,
            prompt_section_override::Column::UserId,
        ])
        .filter(prompt_section_override::Column::IsActive.eq(true))
        .filter(prompt_section_override::Column::WorkspaceId.eq(workspace_id));
    match user_id {
        Some(uid) => q.filter(
            Condition::any()
                .add(prompt_section_override::Column::UserId.is_null())
                .add(prompt_section_override::Column::UserId.eq(uid)),
        ),
        None => q.filter(prompt_section_override::Column::UserId.is_null()),
    }
}

async fn twin_bundle_overrides(
    tx: &mut DbTransaction<'_>,
    workspace_id: Uuid,
    user_id: Option<Uuid>,
) -> Result<Vec<composer::OverrideRow>, CreationError> {
    #[derive(Debug, FromQueryResult)]
    struct OverrideRow {
        section_key: String,
        body: String,
        version: i32,
        user_id: Option<Uuid>,
    }
    let conn = TxConn::new(tx.inner());
    let rows = bundle_overrides_query(workspace_id, user_id)
        .into_model::<OverrideRow>()
        .all(&conn)
        .await
        .map_err(twin_db)?;
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

fn workspace_query(workspace_id: Uuid) -> sea_orm::Select<workspaces::Entity> {
    workspaces::Entity::find_by_id(workspace_id)
        .select_only()
        .columns([workspaces::Column::Slug, workspaces::Column::Name])
}

fn ticker_query(issue_id: Uuid) -> sea_orm::Select<issue_agent_ticker::Entity> {
    issue_agent_ticker::Entity::find()
        .select_only()
        .columns([
            issue_agent_ticker::Column::Used,
            issue_agent_ticker::Column::Waited,
            issue_agent_ticker::Column::Granted,
            issue_agent_ticker::Column::Enabled,
        ])
        .filter(issue_agent_ticker::Column::IssueId.eq(issue_id))
}

fn ticker_resume_query(issue_id: Uuid) -> sea_orm::Select<issue_agent_ticker::Entity> {
    issue_agent_ticker::Entity::find()
        .select_only()
        .column(issue_agent_ticker::Column::ResumeParentRunId)
        .filter(issue_agent_ticker::Column::IssueId.eq(issue_id))
}

async fn twin_load_render_bundle(
    tx: &mut DbTransaction<'_>,
    issue_id: Uuid,
    run_id: Uuid,
    parent_run_id: Option<Uuid>,
    trigger: &str,
    created_by_id: Uuid,
) -> Result<RenderBundle, CreationError> {
    let issue = {
        let conn = TxConn::new(tx.inner());
        let row = issue_select_query(issue_id)
            .into_model::<IssueRow>()
            .one(&conn)
            .await
            .map_err(twin_db)?
            .ok_or_else(|| CreationError::Db("row not found".to_owned()))?;
        issue_view_from_row(row)
    };
    let project_id = issue
        .project_id
        .ok_or_else(|| CreationError::MissingRow("issue has no project".to_owned()))?;
    let project = {
        let conn = TxConn::new(tx.inner());
        let row = project_select_query(project_id)
            .into_model::<ProjectRow>()
            .one(&conn)
            .await
            .map_err(twin_db)?
            .ok_or_else(|| CreationError::Db("row not found".to_owned()))?;
        project_view_from_row(row)
    };
    let state = match issue.state_id {
        Some(id) => {
            let conn = TxConn::new(tx.inner());
            let row = state_select_query(id)
                .into_model::<StateRow>()
                .one(&conn)
                .await
                .map_err(twin_db)?;
            row.map(|state| StateView {
                id: state.id,
                name: state.name,
                group: state.group,
            })
        }
        None => None,
    };
    let workspace = {
        #[derive(Debug, FromQueryResult)]
        struct WorkspaceRow {
            slug: String,
            name: String,
        }
        let conn = TxConn::new(tx.inner());
        let row = workspace_query(issue.workspace_id)
            .into_model::<WorkspaceRow>()
            .one(&conn)
            .await
            .map_err(twin_db)?
            .ok_or_else(|| CreationError::Db("row not found".to_owned()))?;
        (row.slug, row.name)
    };
    let labels = twin_bundle_labels(tx, issue_id).await?;
    let assignees = twin_bundle_assignees(tx, issue_id).await?;
    let project_states = twin_bundle_project_states(tx, project_id).await?;
    let children = twin_bundle_children(tx, issue_id).await?;
    let (relation_rows, refs, directional_refs) = twin_bundle_relations(tx, issue_id).await?;
    let comments = twin_bundle_comments(tx, issue_id).await?;
    let reviews = twin_bundle_reviews(tx, issue_id).await?;
    let (remote, adapter) = twin_bundle_remote(tx, project_id).await?;
    let ancestors = twin_bundle_ancestors(tx, &issue, &project.identifier).await?;
    let (parent, lineage) = twin_bundle_parent_and_lineage(tx, &ancestors).await?;
    let prior_run_count: i64 = {
        let conn = TxConn::new(tx.inner());
        let count = agent_run::Entity::find()
            .filter(agent_run::Column::WorkItemId.eq(issue_id))
            .filter(agent_run::Column::Id.ne(run_id))
            .count(&conn)
            .await
            .map_err(twin_db)?;
        i64::try_from(count).unwrap_or(i64::MAX)
    };
    let ticker = {
        #[derive(Debug, FromQueryResult)]
        struct TickerRow {
            used: i32,
            waited: i32,
            granted: i32,
            enabled: bool,
        }
        let conn = TxConn::new(tx.inner());
        ticker_query(issue_id)
            .into_model::<TickerRow>()
            .one(&conn)
            .await
            .map_err(twin_db)?
            .map(|row| (row.used, row.waited, row.granted, row.enabled))
    };
    let direct_parent_payload = match parent_run_id {
        Some(id) => twin_bundle_done_payload(tx, id).await?,
        None => None,
    };
    // A parent run with a NULL payload must render the fallback, never
    // the ticker payload — so its existence travels separately from the
    // payload itself (PIDASHCONV-1232).
    let has_parent_run = parent_run_id.is_some();
    let ticker_parent_payload = {
        #[derive(Debug, FromQueryResult)]
        struct ResumeRow {
            resume_parent_run_id: Option<Uuid>,
        }
        let resume = {
            let conn = TxConn::new(tx.inner());
            ticker_resume_query(issue_id)
                .into_model::<ResumeRow>()
                .one(&conn)
                .await
                .map_err(twin_db)?
                .and_then(|row| row.resume_parent_run_id)
        };
        match resume {
            Some(id) => twin_bundle_done_payload(tx, id).await?,
            None => None,
        }
    };
    let user_id = user_id_for_run(trigger, created_by_id)
        .map(|id| id.parse::<Uuid>())
        .transpose()
        .map_err(|_| CreationError::MissingRow("bad user id".to_owned()))?;
    let override_rows = twin_bundle_overrides(tx, issue.workspace_id, user_id).await?;
    Ok(RenderBundle {
        issue,
        project,
        workspace_slug: workspace.0,
        workspace_name: workspace.1,
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
        prior_run_count,
        ticker: ticker.map(|(used, waited, granted, enabled)| TickerBudget {
            used: i64::from(used),
            waited: i64::from(waited),
            granted: i64::from(granted),
            enabled,
        }),
        direct_parent_payload,
        has_parent_run,
        ticker_parent_payload,
        override_rows,
    })
}

fn finalize_lock_query(run_id: Uuid) -> sea_orm::Select<agent_run::Entity> {
    run_columns_select(agent_run::Entity::find_by_id(run_id))
        .filter(agent_run::Column::Status.is_not_in(terminal_status_values()))
        .lock_exclusive()
}

impl FinalizeAgentRunSeam for HandoffStore<'_, '_> {
    async fn finalize_failed_run(
        &mut self,
        run_id: Uuid,
        error_code: &str,
        error: &str,
        now: DateTime<Utc>,
    ) -> Result<RunView, CreationError> {
        let locked_row = {
            let conn = TxConn::new(self.tx.inner());
            finalize_lock_query(run_id)
                .into_model::<RunRow>()
                .one(&conn)
                .await
                .map_err(twin_db)?
        };
        if locked_row.is_none() {
            let conn = TxConn::new(self.tx.inner());
            let row = run_select_query(run_id)
                .into_model::<RunRow>()
                .one(&conn)
                .await
                .map_err(twin_db)?
                .ok_or_else(|| CreationError::Db("row not found".to_owned()))?;
            return run_view_from_row(row);
        }
        let locked = locked_row.map(run_view_from_row).transpose()?;
        let Some(locked) = locked else {
            unreachable!("locked_row was Some")
        };
        // Merge in Rust from the locked bag: equivalent to the SQL
        // `jsonb` merge under the held row lock, with no extra statement.
        let patch = finalize_error_details_patch(error_code, error);
        let code = patch.get("code").cloned().unwrap_or(Value::Null);
        let message = patch.get("message").cloned().unwrap_or(Value::Null);
        let merged = merge_error_details(
            &locked.error_details,
            &[("error_code", code), ("error", message)],
        );
        {
            let conn = TxConn::new(self.tx.inner());
            agent_run::Entity::update_many()
                .col_expr(agent_run::Column::Status, Expr::value("failed"))
                .col_expr(agent_run::Column::EndedAt, Expr::value(now))
                .col_expr(
                    agent_run::Column::TerminalHooksAppliedAt,
                    Expr::value(Option::<Moment>::None),
                )
                .col_expr(
                    agent_run::Column::TerminalCapacityReleasedAt,
                    Expr::value(Option::<Moment>::None),
                )
                .col_expr(agent_run::Column::ErrorDetails, Expr::value(merged))
                .filter(agent_run::Column::Id.eq(run_id))
                .exec(&conn)
                .await
                .map_err(twin_db)?;
        }
        if locked.executor_kind == AgentExecutorKind::CloudAgent {
            let exists_count = {
                let conn = TxConn::new(self.tx.inner());
                agent_run_event::Entity::find()
                    .filter(agent_run_event::Column::AgentRunId.eq(run_id))
                    .filter(agent_run_event::Column::Kind.eq("terminal"))
                    .count(&conn)
                    .await
                    .map_err(twin_db)?
            };
            if exists_count == 0 {
                let seq = {
                    #[derive(Debug, FromQueryResult)]
                    struct SeqRow {
                        max_seq: Option<i32>,
                    }
                    let conn = TxConn::new(self.tx.inner());
                    agent_run_event::Entity::find()
                        .select_only()
                        .expr_as(
                            Func::max(Expr::col((
                                agent_run_event::Entity,
                                agent_run_event::Column::Seq,
                            ))),
                            "max_seq",
                        )
                        .filter(agent_run_event::Column::AgentRunId.eq(run_id))
                        .into_model::<SeqRow>()
                        .one(&conn)
                        .await
                        .map_err(twin_db)?
                        .and_then(|row| row.max_seq)
                        .map(|max| max + 1)
                        .unwrap_or(1)
                };
                let payload = serde_json::json!({"status": "failed", "error_code": error_code});
                {
                    let conn = TxConn::new(self.tx.inner());
                    agent_run_event::ActiveModel {
                        seq: Set(seq),
                        kind: Set("terminal".to_owned()),
                        payload: Set(payload),
                        created_at: Set(now.into()),
                        agent_run_id: Set(run_id),
                        ..Default::default()
                    }
                    .insert(&conn)
                    .await
                    .map_err(twin_db)?;
                }
            }
        }
        // The `on_commit` lambda (`_publish_effects`): captured for the
        // handoff drain below (the jobs twin returns it pending for D-15;
        // the api twin can drain it directly).
        self.terminal_effects.push(run_id);
        let conn = TxConn::new(self.tx.inner());
        let row = run_select_query(run_id)
            .into_model::<RunRow>()
            .one(&conn)
            .await
            .map_err(twin_db)?
            .ok_or_else(|| CreationError::Db("row not found".to_owned()))?;
        run_view_from_row(row)
    }
}

/// `complete_project_move_handoff(run_id)` (`service.py:644-732`):
/// own transaction over [`HandoffStore`], committed, then the handoff
/// tx's own post-commit effects (the terminal pairs — queued copy plus
/// the inline apply — then the pod drains). The publish-effect drain
/// is a caller-supplied callback (it captures the ambient crum
/// request user for the inline terminal effects). Isolated (the
/// terminal transition already committed): failures are `Err(())` for
/// the caller to log. Moved from the api handoff module (OVL-06
/// PIDASHCONV-1544); the api entry is now a thin caller.
/// Re-exported through `pidash-sdk::ops`.
// The `()` error is the moved OSS signature: the caller logs.
#[allow(clippy::result_unit_err)]
pub async fn complete_project_move_handoff_inline<F, Fut, T, E>(
    pool: &PgPool,
    deps: &HandoffInlineDeps,
    run_id: Uuid,
    drain_one: F,
) -> Result<Option<Uuid>, ()>
where
    F: FnMut(LifecycleEffect) -> Fut,
    Fut: Future<Output = Result<T, E>>,
{
    let cache = RedisAdmissionCache::new(deps.redis.clone());
    let tx = DbTransaction::begin(pool).await.map_err(|_| ())?;
    let mut store = HandoffStore::new(
        tx,
        pool,
        deps.cloud.clone(),
        deps.managed.clone(),
        cache,
        unix_now_secs(),
        deps.toolsets.clone(),
        deps.assistant_seam.clone(),
    );
    let outcome = creation_kernel::complete_project_move_handoff(&mut store, run_id, now_micros())
        .await
        .map_err(|_| ())?;
    let (tx, dispatches, deferred, terminal_effects, cache) = store.into_parts();
    tx.commit().await.map_err(|_| ())?;
    let mut pairs = Vec::with_capacity(terminal_effects.len() * 2);
    for id in terminal_effects {
        pairs.extend(finalize_kernel::plan_publish_effects(id));
    }
    drain_publish_effects(pairs, drain_one).await;
    let now = now_micros();
    for id in dispatches {
        // `dispatch_agent_run` leaves the runner-plane drain to the caller
        // (`dispatch.py:61-70` drains the pod inline). Act on the decision
        // with the same recipe the revoke path uses so the replacement run
        // is matched to the target pod's idle runner.
        if let DispatchDecision::PodDrain { pod_id } =
            dispatch_agent_run(pool, &deps.cloud, id, now)
                .await
                .map_err(|_| ())?
        {
            drain_pod_by_id(pool, deps.redis.as_ref(), &deps.runner, pod_id)
                .await
                .map_err(|_| ())?;
        }
    }
    for consume in &deferred {
        consume_admission_token(&cache, consume);
    }
    Ok(outcome.map(|run| run.id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pidash_db::config::settings::{CloudAgentSettings, ManagedRunnerSettings};
    use std::sync::Arc;

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

    /// `int(time.time())` for the admission bucket clock.
    fn unix_now_secs() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_secs() as i64)
            .unwrap_or(0)
    }

    /// A replacement seam naming its schema tool: the store answers
    /// the installed name, not the CE default.
    #[derive(Debug)]
    struct AcmeToolsets;

    impl pidash_services::extensions::CloudAgentToolsetsSeam for AcmeToolsets {
        fn schema_tool_name(&self) -> &'static str {
            "acme_get_tool_schema"
        }
    }

    /// The trait defaults with a canned title: the CE behaviour lives
    /// in the defaults (the api crate's `CeAssistantSeam` only adds
    /// the title transport).
    #[derive(Debug)]
    struct DefaultSeam;

    impl pidash_services::extensions::AssistantModelSeam for DefaultSeam {
        fn generate_title(
            &self,
            request: pidash_services::extensions::TitleRequest,
        ) -> pidash_services::extensions::SeamFuture<
            '_,
            Result<String, pidash_services::extensions::TitleError>,
        > {
            let _ = request;
            Box::pin(async move { Ok("canned".to_owned()) })
        }
    }

    /// Overlay-shaped seam: a keyless user still counts as configured
    /// (the gateway lane, not BYOK).
    #[derive(Debug)]
    struct GatewaySeam;

    impl pidash_services::extensions::AssistantModelSeam for GatewaySeam {
        fn has_usable_llm_config(
            &self,
            user_id: Uuid,
            has_api_key: bool,
        ) -> pidash_services::extensions::SeamFuture<'_, bool> {
            let _ = (user_id, has_api_key);
            Box::pin(async move { true })
        }

        fn generate_title(
            &self,
            request: pidash_services::extensions::TitleRequest,
        ) -> pidash_services::extensions::SeamFuture<
            '_,
            Result<String, pidash_services::extensions::TitleError>,
        > {
            let _ = request;
            Box::pin(async move { Ok("seam title".to_owned()) })
        }
    }

    #[tokio::test]
    async fn live_twin_schema_tool_name_flows_from_installed_seam() {
        let Ok(url) = std::env::var("DATABASE_URL") else {
            eprintln!("skipping live-db test: DATABASE_URL is not set");
            return;
        };
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .expect("connect to scratch DATABASE_URL");
        let tx = DbTransaction::begin(&pool).await.expect("begin");
        let store = HandoffStore::new(
            tx,
            &pool,
            django_cloud_settings(),
            django_managed_settings(),
            RedisAdmissionCache { client: None },
            unix_now_secs(),
            Arc::new(AcmeToolsets),
            Arc::new(DefaultSeam),
        );
        assert_eq!(
            store.extra_toolsets_schema_tool(),
            "acme_get_tool_schema".to_owned()
        );
        let (tx, _, _, _, _) = store.into_parts();
        tx.rollback().await.expect("rollback");
    }

    /// The gate consults the installed seam, not the key bit: these two
    /// never touched the database (the store construction was only a
    /// way to reach the seam), so they run without a live pool.
    #[tokio::test]
    async fn live_twin_gate_uses_the_installed_seam() {
        let seam: Arc<dyn pidash_services::extensions::AssistantModelSeam> = Arc::new(GatewaySeam);
        assert!(
            twin_gate_allows(&seam, Uuid::new_v4(), false).await,
            "an installed seam that accepts the user must open the twin gate without a key"
        );
    }

    #[tokio::test]
    async fn live_twin_gate_keeps_the_ce_default() {
        let seam: Arc<dyn pidash_services::extensions::AssistantModelSeam> = Arc::new(DefaultSeam);
        assert!(
            twin_gate_allows(&seam, Uuid::new_v4(), true).await,
            "CE still admits a keyed handoff actor"
        );
        assert!(
            !twin_gate_allows(&seam, Uuid::new_v4(), false).await,
            "CE still refuses a keyless handoff actor"
        );
    }

    #[test]
    fn seaorm_statement_shapes_pin_joins_filters_and_locks() {
        use sea_orm::{DbBackend, QueryTrait};
        let id = Uuid::new_v4();
        let issue_sql = issue_select_query(id)
            .build(DbBackend::Postgres)
            .to_string();
        assert!(issue_sql.contains(r#"FROM "issues""#), "{issue_sql}");
        assert!(!issue_sql.contains("deleted_at"), "{issue_sql}");
        let labels_sql = bundle_labels_query(id)
            .build(DbBackend::Postgres)
            .to_string();
        assert!(
            labels_sql.contains(r#"INNER JOIN "labels""#),
            "{labels_sql}"
        );
        assert!(
            labels_sql.contains(r#""labels"."deleted_at" IS NULL"#),
            "{labels_sql}"
        );
        assert!(
            labels_sql.contains(r#"ORDER BY "labels"."created_at" DESC"#),
            "{labels_sql}"
        );
        let children_sql = bundle_children_query(id)
            .build(DbBackend::Postgres)
            .to_string();
        assert!(
            children_sql.contains(r#"INNER JOIN "projects""#),
            "{children_sql}"
        );
        assert!(
            children_sql.contains(r#"LEFT JOIN "states""#),
            "{children_sql}"
        );
        assert!(
            children_sql.contains(r#""states"."group" <> 'triage'"#)
                || children_sql.contains(r#""states"."group" != 'triage'"#),
            "{children_sql}"
        );
        let comments_sql = bundle_comments_query(id)
            .build(DbBackend::Postgres)
            .to_string();
        assert!(
            comments_sql.contains(r#"LEFT JOIN "users""#),
            "{comments_sql}"
        );
        assert!(
            comments_sql.contains(r#"ORDER BY "issue_comments"."created_at" ASC"#),
            "{comments_sql}"
        );
        let remote_sql = bundle_remote_query(id)
            .build(DbBackend::Postgres)
            .to_string();
        assert!(
            remote_sql.contains(r#"INNER JOIN "git_repositories""#),
            "{remote_sql}"
        );
        assert!(
            remote_sql.contains(r#""git_repositories"."deleted_at" IS NULL"#),
            "{remote_sql}"
        );
        let parent_sql = parent_cols_query(id).build(DbBackend::Postgres).to_string();
        assert!(parent_sql.contains("COUNT(*)"), "{parent_sql}");
        assert!(
            parent_sql.contains(r#""issue_comments"."deleted_at" IS NULL"#),
            "{parent_sql}"
        );
        let finalize_sql = finalize_lock_query(id)
            .build(DbBackend::Postgres)
            .to_string();
        assert!(finalize_sql.contains("NOT IN"), "{finalize_sql}");
        assert!(finalize_sql.contains("FOR UPDATE"), "{finalize_sql}");
    }

    async fn live_pool() -> Option<sqlx::PgPool> {
        let Ok(url) = std::env::var("DATABASE_URL") else {
            eprintln!("skipping live-db test: DATABASE_URL is not set");
            return None;
        };
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .expect("connect to scratch DATABASE_URL");
        Some(pool)
    }

    async fn create_temp_table(tx: &mut DbTransaction<'_>, table: &str) {
        sqlx::query(&format!(
            "CREATE TEMP TABLE {table} ON COMMIT DROP AS SELECT * FROM public.{table} WITH NO DATA"
        ))
        .execute(&mut **tx.inner())
        .await
        .expect("temp table");
    }

    /// Core `HandoffStore` reads + writes on temp copies, exercised
    /// through `TxConn` (one connection, temp tables shadow the real
    /// tables for the transaction).
    #[tokio::test]
    async fn live_handoff_core_seaorm() {
        let Some(pool) = live_pool().await else {
            return;
        };
        let mut tx = DbTransaction::begin(&pool).await.expect("begin");
        for table in [
            "issues",
            "projects",
            "states",
            "agent_run",
            "runner",
            "pod",
            "issue_agent_ticker",
            "users",
        ] {
            create_temp_table(&mut tx, table).await;
        }
        let ws = Uuid::new_v4();
        let project = Uuid::new_v4();
        let state = Uuid::new_v4();
        let issue = Uuid::new_v4();
        let runner = Uuid::new_v4();
        let pod = Uuid::new_v4();
        let run = Uuid::new_v4();
        let run2 = Uuid::new_v4();
        let user = Uuid::new_v4();
        sqlx::query(&format!(
            "INSERT INTO projects (id, workspace_id, identifier, name, description, repo_url, base_branch, default_agent_executor, project_lead_id, default_assignee_id, agent_default_max_ticks, agent_default_interval_seconds, agent_review_default_interval_seconds, agent_test_default_interval_seconds) VALUES ('{project}', '{ws}', 'PROJ', 'Proj', 'desc', 'https://r', 'main', 'cloud_agent', NULL, NULL, 10, 5, 6, 7)"
        ))
        .execute(&mut **tx.inner())
        .await
        .expect("project");
        sqlx::query(&format!(
            "INSERT INTO states (id, name, \"group\") VALUES ('{state}', 'Todo', 'todo')"
        ))
        .execute(&mut **tx.inner())
        .await
        .expect("state");
        sqlx::query(&format!(
            "INSERT INTO issues (id, workspace_id, project_id, state_id, parent_id, created_by_id, assigned_pod_id, agent_executor, git_work_branch, workpad, name, description_stripped, priority, sequence_id, target_date) VALUES ('{issue}', '{ws}', '{project}', '{state}', NULL, '{user}', NULL, NULL, 'br', 'pad', 'Title', 'stripped', 'medium', 3, NULL)"
        ))
        .execute(&mut **tx.inner())
        .await
        .expect("issue");
        sqlx::query(&format!(
            "INSERT INTO users (id, is_active, is_bot) VALUES ('{user}', true, false)"
        ))
        .execute(&mut **tx.inner())
        .await
        .expect("user");
        sqlx::query(&format!(
            "INSERT INTO pod (id, project_id, is_default, deleted_at) VALUES ('{pod}', '{project}', true, NULL)"
        ))
        .execute(&mut **tx.inner())
        .await
        .expect("pod");
        sqlx::query(&format!(
            "INSERT INTO runner (id, pod_id, status, last_heartbeat_at, paused_at) VALUES ('{runner}', '{pod}', 'online', now(), NULL)"
        ))
        .execute(&mut **tx.inner())
        .await
        .expect("runner");
        sqlx::query(&format!(
            "INSERT INTO agent_run (id, workspace_id, created_by_id, pod_id, runner_id, pinned_runner_id, parent_run_id, work_item_id, status, trigger, executor_kind, phase_kind, run_config, tool_plan, error_details, prompt, prompt_manifest, ended_at, created_at) VALUES ('{run}', '{ws}', '{user}', '{pod}', NULL, NULL, NULL, '{issue}', 'running', 'direct', 'cloud_agent', 'coding', '{{}}', '{{}}', '{{}}', '', NULL, NULL, now()), ('{run2}', '{ws}', '{user}', '{pod}', NULL, NULL, NULL, '{issue}', 'queued', 'direct', 'cloud_agent', 'coding', '{{}}', '{{}}', '{{}}', '', NULL, NULL, now() + interval '1 minute')"
        ))
        .execute(&mut **tx.inner())
        .await
        .expect("runs");
        sqlx::query(&format!(
            "INSERT INTO issue_agent_ticker (issue_id, resume_parent_run_id, used, waited, granted, enabled) VALUES ('{issue}', '{run}', 1, 2, 3, true)"
        ))
        .execute(&mut **tx.inner())
        .await
        .expect("ticker");
        let mut store = HandoffStore::new(
            tx,
            &pool,
            django_cloud_settings(),
            django_managed_settings(),
            RedisAdmissionCache { client: None },
            unix_now_secs(),
            Arc::new(AcmeToolsets),
            Arc::new(DefaultSeam),
        );
        let got_issue = store.issue(issue).await.expect("issue");
        assert_eq!(got_issue.id, issue);
        assert_eq!(got_issue.name.as_deref(), Some("Title"));
        let got_project = store.project(project).await.expect("project");
        assert_eq!(got_project.identifier, "PROJ");
        assert_eq!(got_project.pool, 10);
        let got_state = store
            .state(Some(state))
            .await
            .expect("state")
            .expect("state row");
        assert_eq!(got_state.name, "Todo");
        let latest = store
            .latest_prior_run(issue)
            .await
            .expect("latest")
            .expect("latest row");
        assert_eq!(latest.id, run2);
        let active = store
            .active_run_for(issue)
            .await
            .expect("active")
            .expect("active row");
        assert_eq!(active.id, run2);
        let run_row = store.run(run).await.expect("run").expect("run row");
        assert_eq!(run_row.id, run);
        let got_runner = store
            .runner(runner)
            .await
            .expect("runner")
            .expect("runner row");
        assert_eq!(got_runner.status, "online");
        let got_pod = store
            .assigned_pod(pod)
            .await
            .expect("pod")
            .expect("pod row");
        assert_eq!(got_pod.id, pod);
        let default_pod = store
            .default_pod_for_project(project)
            .await
            .expect("default pod")
            .expect("default pod row");
        assert_eq!(default_pod.id, pod);
        assert_eq!(
            store.resume_parent_run_id(issue).await.expect("resume"),
            Some(run)
        );
        assert_eq!(
            store.work_item_id_for_run(run).await.expect("work item"),
            Some(issue)
        );
        let locked = store
            .lock_issue_for_handoff(issue)
            .await
            .expect("lock issue")
            .expect("locked");
        assert_eq!(locked.issue.id, issue);
        let locked_run = store
            .lock_run_for_handoff(run)
            .await
            .expect("lock run")
            .expect("locked run");
        assert_eq!(locked_run.id, run);
        let flags = store.user_flags(user).await.expect("flags");
        assert!(flags.is_active);
        assert!(!flags.is_bot);
        // Insert a new run through the store and read it back.
        let new_id = Uuid::new_v4();
        let new_row = NewAgentRun {
            id: new_id,
            workspace_id: ws,
            created_by_id: user,
            pod_id: pod,
            pinned_runner_id: None,
            work_item_id: issue,
            parent_run_id: None,
            executor_kind: AgentExecutorKind::CloudAgent,
            error_details: serde_json::json!({}),
            tool_plan: serde_json::json!({}),
            trigger: "direct".to_owned(),
            phase_kind: "".to_owned(),
            run_config: serde_json::json!({}),
            now: chrono::Utc::now(),
        };
        let inserted = store.insert_run(&new_row).await.expect("insert");
        assert_eq!(inserted.id, new_id);
        store
            .save_prompt(new_id, "hello", &serde_json::json!({"a": 1}))
            .await
            .expect("save prompt");
        store
            .save_run_config(new_id, &serde_json::json!({"b": 2}))
            .await
            .expect("save config");
        let reread = store
            .run(new_id)
            .await
            .expect("reread")
            .expect("reread row");
        assert_eq!(reread.prompt, "hello");
        assert_eq!(reread.run_config, serde_json::json!({"b": 2}));
        // Finalize merges into error_details and stamps failed.
        let finalized = store
            .finalize_failed_run(run, "boom", "bad", chrono::Utc::now())
            .await
            .expect("finalize");
        assert_eq!(format!("{:?}", finalized.status), "Failed");
        assert!(finalized.ended_at.is_some());
        assert_eq!(finalized.error_code(), "boom");
        let (tx, _, _, _, _) = store.into_parts();
        tx.rollback().await.expect("rollback");
    }

    /// Bundle helpers on temp copies: labels/auth, children, relations,
    /// comments (with the `fold` drop), reviews, remote, workspace,
    /// overrides, ticker, prior count and done payload.
    #[tokio::test]
    async fn live_handoff_bundle_seaorm() {
        let Some(pool) = live_pool().await else {
            return;
        };
        let mut tx = DbTransaction::begin(&pool).await.expect("begin");
        for table in [
            "issues",
            "projects",
            "states",
            "agent_run",
            "agent_run_event",
            "users",
            "labels",
            "issue_labels",
            "issue_assignees",
            "issue_comments",
            "issue_relations",
            "issue_agent_ticker",
            "git_code_review_links",
            "git_repositories",
            "git_repository_bindings",
            "workspaces",
            "prompt_section_override",
        ] {
            create_temp_table(&mut tx, table).await;
        }
        let ws = Uuid::new_v4();
        let project = Uuid::new_v4();
        let state = Uuid::new_v4();
        let issue = Uuid::new_v4();
        let child = Uuid::new_v4();
        let other = Uuid::new_v4();
        let label = Uuid::new_v4();
        let assignee = Uuid::new_v4();
        let run = Uuid::new_v4();
        let repo = Uuid::new_v4();
        sqlx::query(&format!(
            "INSERT INTO workspaces (id, slug, name) VALUES ('{ws}', 'ws-slug', 'WS Name')"
        ))
        .execute(&mut **tx.inner())
        .await
        .expect("workspace");
        sqlx::query(&format!(
            "INSERT INTO projects (id, workspace_id, identifier, name, description, repo_url, base_branch, default_agent_executor, agent_default_max_ticks, agent_default_interval_seconds, agent_review_default_interval_seconds, agent_test_default_interval_seconds, archived_at) VALUES ('{project}', '{ws}', 'PRJ', 'P', 'd', 'r', 'main', 'local_runner', 1, 1, 1, 1, NULL)"
        ))
        .execute(&mut **tx.inner())
        .await
        .expect("project");
        sqlx::query(&format!(
            "INSERT INTO states (id, name, \"group\", project_id, deleted_at, sequence, description) VALUES ('{state}', 'Work', 'todo', '{project}', NULL, 1, 'desc')"
        ))
        .execute(&mut **tx.inner())
        .await
        .expect("state");
        sqlx::query(&format!(
            "INSERT INTO issues (id, workspace_id, project_id, state_id, parent_id, name, sequence_id, deleted_at, archived_at, is_draft, created_at) VALUES ('{issue}', '{ws}', '{project}', '{state}', NULL, 'Parent', 1, NULL, NULL, false, now()), ('{child}', '{ws}', '{project}', '{state}', '{issue}', 'Child', 2, NULL, NULL, false, now()), ('{other}', '{ws}', '{project}', '{state}', NULL, 'Other', 3, NULL, NULL, false, now())"
        ))
        .execute(&mut **tx.inner())
        .await
        .expect("issues");
        sqlx::query(&format!(
            "INSERT INTO users (id, display_name, email, username, is_bot, created_at) VALUES ('{assignee}', 'Alice', 'alice@example.com', 'alice', false, now())"
        ))
        .execute(&mut **tx.inner())
        .await
        .expect("user");
        sqlx::query(&format!(
            "INSERT INTO labels (id, name, deleted_at, created_at) VALUES ('{label}', 'bug', NULL, now())"
        ))
        .execute(&mut **tx.inner())
        .await
        .expect("label");
        sqlx::query(&format!(
            "INSERT INTO issue_labels (id, issue_id, label_id) VALUES (gen_random_uuid(), '{issue}', '{label}')"
        ))
        .execute(&mut **tx.inner())
        .await
        .expect("issue_labels");
        sqlx::query(&format!(
            "INSERT INTO issue_assignees (id, issue_id, assignee_id) VALUES (gen_random_uuid(), '{issue}', '{assignee}')"
        ))
        .execute(&mut **tx.inner())
        .await
        .expect("assignees");
        sqlx::query(&format!(
            "INSERT INTO issue_comments (id, issue_id, comment_stripped, speaker_type, speaker_label, speaker_agent_run_id, created_at, deleted_at, labels, actor_id) VALUES (gen_random_uuid(), '{issue}', 'Keep me', 'human', '', NULL, now(), NULL, '{{}}', '{assignee}'), (gen_random_uuid(), '{issue}', 'Fold me', 'human', '', NULL, now() + interval '1 minute', NULL, '{{fold}}', '{assignee}')"
        ))
        .execute(&mut **tx.inner())
        .await
        .expect("comments");
        sqlx::query(&format!(
            "INSERT INTO issue_relations (id, issue_id, related_issue_id, relation_type, deleted_at, created_at) VALUES (gen_random_uuid(), '{issue}', '{other}', 'blocks', NULL, now())"
        ))
        .execute(&mut **tx.inner())
        .await
        .expect("relations");
        sqlx::query(&format!(
            "INSERT INTO git_code_review_links (id, issue_id, url, title, state, merged, draft, provider, external_iid, deleted_at, created_at) VALUES (gen_random_uuid(), '{issue}', 'https://pr/1', 'T', 'open', false, false, 'github', '1', NULL, now())"
        ))
        .execute(&mut **tx.inner())
        .await
        .expect("reviews");
        sqlx::query(&format!(
            "INSERT INTO git_repositories (id, provider, host_url, full_name, deleted_at) VALUES ('{repo}', 'github', 'https://gh', 'o/r', NULL)"
        ))
        .execute(&mut **tx.inner())
        .await
        .expect("repos");
        sqlx::query(&format!(
            "INSERT INTO git_repository_bindings (id, project_id, repository_id, deleted_at, created_at) VALUES (gen_random_uuid(), '{project}', '{repo}', NULL, now())"
        ))
        .execute(&mut **tx.inner())
        .await
        .expect("bindings");
        sqlx::query(&format!(
            "INSERT INTO agent_run (id, workspace_id, created_by_id, pod_id, work_item_id, status, trigger, executor_kind, phase_kind, run_config, tool_plan, error_details, prompt, created_at, done_payload) VALUES ('{run}', '{ws}', '{assignee}', gen_random_uuid(), '{issue}', 'completed', 'direct', 'cloud_agent', '', '{{}}', '{{}}', '{{}}', '', now(), '{{\"done\": true}}')"
        ))
        .execute(&mut **tx.inner())
        .await
        .expect("run");
        sqlx::query(&format!(
            "INSERT INTO issue_agent_ticker (issue_id, resume_parent_run_id, used, waited, granted, enabled) VALUES ('{issue}', '{run}', 1, 0, 1, true)"
        ))
        .execute(&mut **tx.inner())
        .await
        .expect("ticker");
        sqlx::query(&format!(
            "INSERT INTO prompt_section_override (id, section_key, body, version, user_id, workspace_id, is_active) VALUES (gen_random_uuid(), 'sec', 'body', 1, NULL, '{ws}', true), (gen_random_uuid(), 'usec', 'ubody', 1, '{assignee}', '{ws}', true)"
        ))
        .execute(&mut **tx.inner())
        .await
        .expect("overrides");
        assert_eq!(
            twin_bundle_labels(&mut tx, issue).await.expect("labels"),
            vec!["bug".to_owned()]
        );
        let assignees = twin_bundle_assignees(&mut tx, issue)
            .await
            .expect("assignees");
        assert_eq!(assignees.len(), 1);
        let states = twin_bundle_project_states(&mut tx, project)
            .await
            .expect("states");
        assert_eq!(states.len(), 1);
        let children = twin_bundle_children(&mut tx, issue)
            .await
            .expect("children");
        assert_eq!(children.len(), 1);
        let (relations, refs, _) = twin_bundle_relations(&mut tx, issue)
            .await
            .expect("relations");
        assert_eq!(relations.len(), 1);
        assert!(refs.contains_key(&other.to_string()));
        let comments = twin_bundle_comments(&mut tx, issue)
            .await
            .expect("comments");
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].body, "Keep me");
        let reviews = twin_bundle_reviews(&mut tx, issue).await.expect("reviews");
        assert_eq!(reviews.len(), 1);
        let (remote, _) = twin_bundle_remote(&mut tx, project).await.expect("remote");
        assert!(remote.is_some());
        let payload = twin_bundle_done_payload(&mut tx, run)
            .await
            .expect("payload");
        assert_eq!(payload, Some(serde_json::json!({"done": true})));
        let overrides = twin_bundle_overrides(&mut tx, ws, None)
            .await
            .expect("overrides");
        assert_eq!(overrides.len(), 1);
        let overrides_user = twin_bundle_overrides(&mut tx, ws, Some(assignee))
            .await
            .expect("overrides user");
        assert_eq!(overrides_user.len(), 2);
        // Render bundle exercises the full loader.
        let ws2 = ws;
        let _ = ws2;
        let parent_payload_run = run;
        let bundle = twin_load_render_bundle(
            &mut tx,
            issue,
            run,
            Some(parent_payload_run),
            "direct",
            assignee,
        )
        .await
        .expect("bundle");
        assert_eq!(bundle.issue.id, issue);
        assert_eq!(bundle.prior_run_count, 0);
        assert!(bundle.ticker.is_some());
        assert_eq!(
            bundle.direct_parent_payload,
            Some(serde_json::json!({"done": true}))
        );
        tx.rollback().await.expect("rollback");
    }
}
