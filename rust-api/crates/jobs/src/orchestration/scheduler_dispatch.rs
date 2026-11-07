#![forbid(unsafe_code)]

//! Live scheduler-run dispatch for the worker-owned
//! `fire_scheduler_binding` (PIDASHCONV-861).
//!
//! Three pieces, all live:
//!
//! * `impl EntriesSeam for LiveCreationStore`: the 16 entries methods
//!   over the insertion transaction. Reads and writes run on the
//!   driver's transaction (the `DispatchSeam` precedent), so the
//!   capacity lock, the INSERT, and the FAILED finalization share one
//!   transaction exactly as the services driver's contract requires.
//!   SQL text comes from the services consts, executed verbatim.
//! * [`dispatch_scheduler_run`]: the jobs transaction driver — open
//!   the transaction, run the services driver, roll back when the
//!   outcome says so, commit, then drain post-commit dispatch. The
//!   `dispatch_driver!` shape, written out because the services call
//!   takes `(seam, req)` instead of the continuation argument shape.
//! * [`live_scheduler_dispatch`]: the [`SchedulerDispatch`] value
//!   `worker()` registers. Each call prefetches presence for the
//!   binding's creator chain (actor, project lead, default assignee),
//!   runs the driver, and maps the outcome onto the
//!   `(run, fail_reason)` contract.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, FromQueryResult, QueryFilter, QueryOrder,
    QuerySelect, Set,
};
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

use pidash_db::config::{CloudAgentSettings, ManagedRunnerSettings, RunnerSettings};
use pidash_db::dispatch::status::AgentRunStatus;
use pidash_db::entity::{
    agent_run, issue_agent_ticker, issues, pod, scheduler_bindings, schedulers, workspaces,
};
use pidash_db::orchestration::querysets::{
    fetch_project_has_allowed_role, fetch_project_is_member, fetch_workspace_admin_by_slug,
};
use pidash_db::tasks_ticker::models::issue_agent_ticker::IssueAgentTicker;
use pidash_db::tx::Transaction;
use pidash_services::dispatch::admission::AdmissionCache;
use pidash_services::dispatch::LlmProfile;
use pidash_services::domain::clock::apply_clock_write;
use pidash_services::domain::identity::{get_agent_system_user, GetAgentUserError};
use pidash_services::extensions::{AssistantModelSeam, CloudAgentToolsetsSeam};
use pidash_services::orchestration::clock::{ClockWrite, ProjectClockPolicy};
use pidash_services::orchestration::creation::{CreationError, PodView, RenderedTurn, RunView};
use pidash_services::orchestration::dispatch::DispatchError;
use pidash_services::orchestration::entries::{
    dispatch_scheduler_run as services_dispatch_scheduler_run, BindingView, EntriesError,
    EntriesSeam, NewSchedulerRun, ProjectRoleFacts, SchedulerOutcome, SchedulerRequest,
    SchedulerView, WorkspaceView, SCHEDULER_ALLOWED_ROLES,
};
use pidash_services::prompting::{composer, context};
use pidash_services::runner_sessions::live_drains::drain_pod_by_id;
use pidash_types::dispatch::AgentExecutorKind;

use super::creation_jobs::{bundle_overrides, LiveCreationStore, StoreDeps};
use super::fire_tick_seam::{drain_dispatch, DispatchJobsError, DispatchOutcome};
use super::live_inputs::{
    fetch_presence, gate_fn, gate_verdicts, order_universe, profile_fn, project_leads,
};
use crate::admission::RedisAdmissionCache;
use crate::dispatch::DispatchDecision;
use crate::tasks_ticker::fire_binding::{
    BindingForDispatch, DispatchRun, SchedulerDispatch, SchedulerDispatchResult,
};

fn db(error: sqlx::Error) -> CreationError {
    CreationError::Db(error.to_string())
}

fn db_err(error: sea_orm::DbErr) -> CreationError {
    CreationError::Db(error.to_string())
}

/// One [`RunView`] over the services `RUN_VIEW_COLUMNS` projection:
/// the single-table agent-run row the scheduler seam consumes.
/// `status` / `executor_kind` stay strings until [`run_view`]
/// applies the same strict decode as the services `map_run_view`.
#[derive(Clone, Debug, PartialEq, FromQueryResult)]
struct RunViewRow {
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

/// Map one [`RunViewRow`] onto the services view. The trigger
/// rides through unparsed (Django's `TextChoices` are
/// choices-only, so the stored value may sit outside the trigger
/// enum); `status` / `executor_kind` decode strictly, exactly as
/// the services `map_run_view` does.
fn run_view(row: RunViewRow) -> Result<RunView, CreationError> {
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
        ended_at: row.ended_at.map(|ended| ended.with_timezone(&Utc)),
    })
}

fn run_view_row_from_model(model: agent_run::Model) -> RunViewRow {
    RunViewRow {
        id: model.id,
        workspace_id: model.workspace_id,
        created_by_id: model.created_by_id,
        pod_id: model.pod_id,
        runner_id: model.runner_id,
        pinned_runner_id: model.pinned_runner_id,
        parent_run_id: model.parent_run_id,
        work_item_id: model.work_item_id,
        status: model.status,
        trigger: model.trigger,
        executor_kind: model.executor_kind,
        phase_kind: model.phase_kind,
        run_config: model.run_config,
        tool_plan: model.tool_plan,
        error_details: model.error_details,
        prompt: model.prompt,
        prompt_manifest: model.prompt_manifest,
        ended_at: model.ended_at,
    }
}

impl<C, H, P> EntriesSeam for LiveCreationStore<'_, C, H, P>
where
    C: AdmissionCache,
    H: Fn(Uuid) -> bool + Clone + Send + Sync,
    P: Fn(Option<Uuid>) -> LlmProfile + Clone + Send + Sync,
{
    async fn prior_state_id(&mut self, issue_id: Uuid) -> Result<Option<Uuid>, CreationError> {
        // `Issue.all_objects.only("state_id").get(pk)`: soft-deleted
        // rows included (no `deleted_at` guard), pk plus `state_id`
        // only.
        let row = {
            let conn = pidash_db::orm_tx::TxConn::new(self.dispatch_tx().inner());
            issues::Entity::find_by_id(issue_id)
                .select_only()
                .columns([issues::Column::Id, issues::Column::StateId])
                .into_tuple::<(Uuid, Option<Uuid>)>()
                .one(&conn)
                .await
                .map_err(db_err)?
        };
        Ok(row.and_then(|(_, state_id)| state_id))
    }

    async fn queued_follow_up(&mut self, issue_id: Uuid) -> Result<Option<RunView>, CreationError> {
        // `AgentRun.objects.filter(work_item=issue,
        // status=QUEUED).order_by("-created_at").first()` over the
        // 18-column `RUN_VIEW_COLUMNS` projection, newest first.
        let row = {
            let conn = pidash_db::orm_tx::TxConn::new(self.dispatch_tx().inner());
            agent_run::Entity::find()
                .select_only()
                .columns([
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
                .filter(agent_run::Column::WorkItemId.eq(issue_id))
                .filter(agent_run::Column::Status.eq(AgentRunStatus::Queued.value()))
                .order_by_desc(agent_run::Column::CreatedAt)
                .limit(1)
                .into_model::<RunViewRow>()
                .one(&conn)
                .await
                .map_err(db_err)?
        };
        row.map(run_view).transpose()
    }

    async fn lock_ticker(
        &mut self,
        issue_id: Uuid,
    ) -> Result<Option<IssueAgentTicker>, CreationError> {
        // The clock `_lock_ticker` read half (`scheduling.py:236`):
        // the default-manager soft-delete scope (`deleted_at IS
        // NULL`) plus the pk ordering `.first()` adds (id asc), then
        // `FOR UPDATE` — kept though unobservable (the issue FK is
        // one-to-one).
        let model = {
            let conn = pidash_db::orm_tx::TxConn::new(self.dispatch_tx().inner());
            issue_agent_ticker::Entity::find()
                .filter(issue_agent_ticker::Column::IssueId.eq(issue_id))
                .filter(issue_agent_ticker::Column::DeletedAt.is_null())
                .order_by_asc(issue_agent_ticker::Column::Id)
                .lock_exclusive()
                .one(&conn)
                .await
                .map_err(db_err)?
        };
        Ok(model.map(IssueAgentTicker::from))
    }

    async fn save_ticker(
        &mut self,
        row: &IssueAgentTicker,
        write: ClockWrite,
    ) -> Result<(), CreationError> {
        // `updated_at` is now-at-write: reconcile leaves the locked
        // row's stamp untouched, and Python's `save()` stamps
        // `auto_now` at write time (the pre-consolidation bind).
        apply_clock_write(
            &pidash_db::orm_tx::TxConn::new(self.dispatch_tx().inner()),
            &write,
            row,
            Utc::now(),
        )
        .await
        .map_err(db)
    }

    fn set_rollback(&mut self) {
        self.dispatch_request_rollback();
    }

    async fn clock_policy(
        &mut self,
        project_id: Uuid,
    ) -> Result<ProjectClockPolicy, CreationError> {
        // The shared entries `clock_policy` read (no soft-delete
        // guard): the project's ticking flag, pool, and
        // impl/review/test cadence columns.
        let row = {
            let conn = pidash_db::orm_tx::TxConn::new(self.dispatch_tx().inner());
            pidash_services::orchestration::entries::clock_policy(&conn, project_id)
                .await
                .map_err(db_err)?
        };
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
        // A plain pk read (no deleted guard): the Beat loop passes
        // the live binding object, and the read must return that
        // same row.
        let row = {
            let conn = pidash_db::orm_tx::TxConn::new(self.dispatch_tx().inner());
            scheduler_bindings::Entity::find_by_id(binding_id)
                .select_only()
                .columns([
                    scheduler_bindings::Column::Id,
                    scheduler_bindings::Column::WorkspaceId,
                    scheduler_bindings::Column::ProjectId,
                    scheduler_bindings::Column::SchedulerId,
                    scheduler_bindings::Column::OutcomeMode,
                    scheduler_bindings::Column::ExtraContext,
                    scheduler_bindings::Column::ActorId,
                    scheduler_bindings::Column::PodId,
                ])
                .into_tuple::<(
                    Uuid,
                    Option<Uuid>,
                    Option<Uuid>,
                    Uuid,
                    String,
                    String,
                    Option<Uuid>,
                    Option<Uuid>,
                )>()
                .one(&conn)
                .await
                .map_err(db_err)?
        };
        let Some((
            id,
            workspace_id,
            project_id,
            scheduler_id,
            outcome_mode,
            extra_context,
            actor_id,
            pod_id,
        )) = row
        else {
            return Err(CreationError::MissingRow(format!(
                "no binding {binding_id}"
            )));
        };
        // The old read decoded `workspace_id` as non-null; a NULL
        // workspace was a database error, never a silent miss.
        let Some(workspace_id) = workspace_id else {
            return Err(CreationError::Db(format!(
                "binding {binding_id} workspace_id is null"
            )));
        };
        Ok(BindingView {
            id,
            workspace_id,
            project_id,
            scheduler_id,
            outcome_mode,
            extra_context,
            actor_id,
            pod_id,
        })
    }

    async fn scheduler_override_pod(
        &mut self,
        pod_id: Uuid,
        project_id: Option<Uuid>,
    ) -> Result<Option<PodView>, CreationError> {
        // `Pod.objects.filter(pk, deleted_at__isnull=True,
        // project_id).first()` under the default manager.
        // A `None` project never matches — mirroring
        // `filter(project_id=None)`.
        let Some(project_id) = project_id else {
            return Ok(None);
        };
        let row = {
            let conn = pidash_db::orm_tx::TxConn::new(self.dispatch_tx().inner());
            pod::Entity::find_by_id(pod_id)
                .filter(pod::Column::DeletedAt.is_null())
                .filter(pod::Column::ProjectId.eq(project_id))
                .select_only()
                .columns([pod::Column::Id, pod::Column::ProjectId])
                .into_tuple::<(Uuid, Uuid)>()
                .one(&conn)
                .await
                .map_err(db_err)?
        };
        Ok(row.map(|(id, project_id)| PodView { id, project_id }))
    }

    async fn workspace(&mut self, workspace_id: Uuid) -> Result<WorkspaceView, CreationError> {
        let row = {
            let conn = pidash_db::orm_tx::TxConn::new(self.dispatch_tx().inner());
            workspaces::Entity::find_by_id(workspace_id)
                .select_only()
                .columns([workspaces::Column::Slug, workspaces::Column::Name])
                .into_tuple::<(String, String)>()
                .one(&conn)
                .await
                .map_err(db_err)?
        };
        let Some((slug, name)) = row else {
            return Err(CreationError::MissingRow(format!(
                "no workspace {workspace_id}"
            )));
        };
        Ok(WorkspaceView { slug, name })
    }

    async fn scheduler_row(&mut self, scheduler_id: Uuid) -> Result<SchedulerView, CreationError> {
        // The default manager scope applies: a soft-deleted
        // scheduler misses (the driver raises, as the FK follow
        // would).
        let row = {
            let conn = pidash_db::orm_tx::TxConn::new(self.dispatch_tx().inner());
            schedulers::Entity::find_by_id(scheduler_id)
                .filter(schedulers::Column::DeletedAt.is_null())
                .select_only()
                .columns([
                    schedulers::Column::Id,
                    schedulers::Column::Slug,
                    schedulers::Column::Name,
                    schedulers::Column::Description,
                    schedulers::Column::Prompt,
                ])
                .into_tuple::<(Uuid, String, String, String, String)>()
                .one(&conn)
                .await
                .map_err(db_err)?
        };
        let Some((id, slug, name, description, prompt)) = row else {
            return Err(CreationError::MissingRow(format!(
                "no scheduler {scheduler_id}"
            )));
        };
        Ok(SchedulerView {
            id,
            slug,
            name,
            description,
            prompt,
        })
    }

    async fn scheduler_override_rows(
        &mut self,
        workspace_id: Uuid,
    ) -> Result<Vec<composer::OverrideRow>, CreationError> {
        // `$2 = NULL`: workspace rows only — scheduler runs are
        // always automatic.
        bundle_overrides(self.dispatch_tx(), workspace_id, None).await
    }

    async fn project_role_facts(
        &mut self,
        user_id: Uuid,
        workspace_slug: &str,
        project_id: Uuid,
    ) -> Result<ProjectRoleFacts, CreationError> {
        let tx = self.dispatch_tx().inner();
        let has_allowed_role = fetch_project_has_allowed_role(
            &mut **tx,
            Some(user_id),
            workspace_slug,
            project_id,
            SCHEDULER_ALLOWED_ROLES,
        )
        .await
        .map_err(db)?;
        let tx = self.dispatch_tx().inner();
        let is_project_member =
            fetch_project_is_member(&mut **tx, Some(user_id), workspace_slug, project_id)
                .await
                .map_err(db)?;
        let tx = self.dispatch_tx().inner();
        let is_workspace_admin =
            fetch_workspace_admin_by_slug(&mut **tx, Some(user_id), workspace_slug)
                .await
                .map_err(db)?;
        Ok(ProjectRoleFacts {
            authenticated: true,
            has_allowed_role,
            is_project_member,
            is_workspace_admin,
        })
    }

    async fn has_usable_llm_config(&mut self, user_id: Uuid) -> Result<bool, CreationError> {
        Ok(self.dispatch_has_usable_llm_config(user_id))
    }

    async fn agent_system_user(
        &mut self,
    ) -> Result<
        Result<Uuid, pidash_services::domain::identity::AgentUserCollisionError>,
        CreationError,
    > {
        match get_agent_system_user(self.dispatch_tx().inner(), Utc::now()).await {
            Ok((user, _)) => Ok(Ok(user.id)),
            Err(GetAgentUserError::Collision(collision)) => Ok(Err(collision)),
            Err(GetAgentUserError::Db(error)) => Err(db(error)),
        }
    }

    async fn insert_scheduler_run(
        &mut self,
        row: &NewSchedulerRun,
    ) -> Result<RunView, CreationError> {
        // The scheduler full-row INSERT (`AgentRun.objects.create`,
        // `service.py:953-965`): the same 35 non-generated columns
        // in model order as the services `SCHEDULER_RUN_INSERT_SQL`,
        // with the scheduler VALUES shape — `work_item` NULL,
        // `scheduler_binding` bound, `parent_run` NULL, the
        // `'scheduler'` trigger and `'{}'` run config as literals
        // (fixed by the call path, like `'queued'`), no `phase_kind`
        // stamp (the model default `""`). Every column is set
        // explicitly, exactly as Django's `objects.create` does.
        let model = {
            let conn = pidash_db::orm_tx::TxConn::new(self.dispatch_tx().inner());
            agent_run::ActiveModel {
                id: Set(row.id),
                workspace_id: Set(row.workspace_id),
                owner_id: Set(None),
                created_by_id: Set(row.created_by_id),
                pod_id: Set(row.pod_id),
                runner_id: Set(None),
                pinned_runner_id: Set(row.pinned_runner_id),
                work_item_id: Set(None),
                scheduler_binding_id: Set(Some(row.binding_id)),
                parent_run_id: Set(None),
                status: Set("queued".to_owned()),
                executor_kind: Set(row.executor_kind.value().to_owned()),
                dispatch_attempts: Set(0),
                cancel_requested_at: Set(None),
                cancel_reason: Set(String::new()),
                tool_plan: Set(row.tool_plan.clone()),
                terminal_hooks_applied_at: Set(None),
                terminal_capacity_released_at: Set(None),
                prompt: Set(String::new()),
                trigger: Set("scheduler".to_owned()),
                prompt_manifest: Set(None),
                phase_kind: Set(String::new()),
                run_config: Set(serde_json::json!({})),
                required_capabilities: Set(serde_json::json!([])),
                thread_id: Set(String::new()),
                agent_metadata: Set(serde_json::json!({})),
                lease_expires_at: Set(None),
                done_payload: Set(None),
                error_details: Set(row.error_details.clone()),
                llm_model: Set(String::new()),
                usage: Set(serde_json::json!({})),
                created_at: Set(row
                    .now
                    .with_timezone(&chrono::FixedOffset::east_opt(0).expect("zero offset exists"))),
                assigned_at: Set(None),
                started_at: Set(None),
                ended_at: Set(None),
                // The rest (`input_tokens`, `output_tokens`,
                // `total_tokens`, `refusal_category`) stays `NotSet`
                // so the insert omits those columns, exactly as
                // the services `SCHEDULER_RUN_INSERT_SQL` does.
                ..Default::default()
            }
            .insert(&conn)
            .await
            .map_err(db_err)?
        };
        run_view(run_view_row_from_model(model))
    }

    fn compose_scheduler_turn(
        &mut self,
        ctx: &Value,
        index: &composer::OverrideIndex,
        workspace_id: Option<&str>,
        executor_kind: Option<&str>,
        tool_catalog_version: i64,
    ) -> Result<RenderedTurn, String> {
        // The live store delegates to the merged composer and maps
        // the manifest (`to_json`, or `Null` when absent).
        match context::build_scheduler_turn(
            ctx,
            index,
            workspace_id,
            executor_kind,
            tool_catalog_version,
        ) {
            Ok(turn) => Ok(RenderedTurn {
                text: turn.text,
                manifest: turn
                    .manifest
                    .map(|manifest| manifest.to_json())
                    .unwrap_or(Value::Null),
            }),
            Err(error) => Err(error.message().to_owned()),
        }
    }
}

// ---------------------------------------------------------------------------
// Jobs transaction driver
// ---------------------------------------------------------------------------

/// Map an entries failure onto the jobs error (message-preserving:
/// every variant stringifies without loss).
fn map_entries(error: EntriesError) -> DispatchJobsError {
    match error {
        EntriesError::Store(store) => DispatchJobsError::Store(DispatchError::from(store)),
        EntriesError::Clock(clock) => {
            DispatchJobsError::Store(DispatchError::Db(clock.to_string()))
        }
        EntriesError::AgentUserCollision(collision) => {
            DispatchJobsError::Store(DispatchError::Db(collision.to_string()))
        }
    }
}

/// Open the insertion transaction, run the services
/// `dispatch_scheduler_run`, roll back when the outcome says so,
/// commit, then drain post-commit dispatch. The `dispatch_driver!`
/// discipline, written out because the services call takes
/// `(seam, req)` instead of the continuation argument shape.
pub async fn dispatch_scheduler_run<C, H, P>(
    pool: &PgPool,
    deps: StoreDeps<C, H, P>,
    binding_id: Uuid,
    now: DateTime<Utc>,
) -> Result<DispatchOutcome<SchedulerOutcome>, DispatchJobsError>
where
    C: AdmissionCache,
    H: Fn(Uuid) -> bool + Clone + Send + Sync,
    P: Fn(Option<Uuid>) -> LlmProfile + Clone + Send + Sync,
{
    let cloud = deps.cloud.clone();
    let tx = Transaction::begin(pool).await?;
    let mut store = LiveCreationStore::new(tx, pool, deps);
    let outcome =
        services_dispatch_scheduler_run(&mut store, &SchedulerRequest { binding_id, now })
            .await
            .map_err(map_entries)?;
    if store.dispatch_rollback_requested() {
        let (tx, _, _, _, _, _) = store.into_parts();
        tx.rollback().await?;
        return Ok(DispatchOutcome {
            outcome,
            dispatched: Vec::new(),
            dispatch_decisions: Vec::new(),
            terminal_effects: Vec::new(),
        });
    }
    if let Some(run_id) = outcome.run {
        tracing::info!(%run_id, %binding_id, "scheduler.dispatch: created run");
    }
    let (tx, pool, cache, dispatches, deferred, terminal_effects) = store.into_parts();
    tx.commit().await?;
    let dispatch_decisions =
        drain_dispatch(pool, &cloud, &cache, dispatches, deferred, now).await?;
    let dispatched = dispatch_decisions.iter().map(|(id, _)| *id).collect();
    Ok(DispatchOutcome {
        outcome,
        dispatched,
        dispatch_decisions,
        terminal_effects,
    })
}

// ---------------------------------------------------------------------------
// Live SchedulerDispatch
// ---------------------------------------------------------------------------

/// The produced run's status line for the `(run, fail_reason)`
/// contract: `status` + `error` off the committed row (`run.error`,
/// the `message` key of `error_details` — PDASHOSS01-187).
/// Mirrors `COALESCE(error_details ->> 'message', '')`: a string
/// message is the text itself, other non-null JSON values render
/// as JSON text, and a missing key or JSON null is `""`.
fn run_status_error(details: &Value) -> String {
    match details.get("message") {
        Some(Value::String(text)) => text.clone(),
        Some(other) if !other.is_null() => other.to_string(),
        _ => String::new(),
    }
}

/// Map a services outcome onto the `(run, fail_reason)` contract
/// (`service.py:848`): a produced run (even FAILED) carries
/// `fail_reason: None`; only the short-circuits return `(None,
/// reason)`. A produced run whose row then misses is `Err` (task
/// failure — never a fabricated status). Pure so the offline tests
/// pin it.
fn to_dispatch_result(
    outcome: &SchedulerOutcome,
    fetched: Option<(String, String)>,
) -> Result<SchedulerDispatchResult, String> {
    match (outcome.run, outcome.error.clone(), fetched) {
        (Some(id), _, Some((status, error))) => Ok(SchedulerDispatchResult {
            run: Some(DispatchRun { id, status, error }),
            fail_reason: None,
        }),
        (Some(_), _, None) => Err("run row missing after dispatch".to_owned()),
        (None, error, _) => Ok(SchedulerDispatchResult {
            run: None,
            fail_reason: error,
        }),
    }
}

/// Build the live [`SchedulerDispatch`]: each call prefetches
/// presence for the binding's creator chain (actor, project lead,
/// default assignee), consults the installed assistant seam for the
/// llm-gate verdicts, runs the jobs driver, drains the produced
/// run's pod, and maps the outcome. A transport-level failure
/// outside the `(run, fail_reason)` contract is `Err` (task failure).
pub fn live_scheduler_dispatch(
    pool: PgPool,
    cloud: CloudAgentSettings,
    managed: ManagedRunnerSettings,
    redis: Option<redis::Client>,
    runner: RunnerSettings,
    toolsets: Arc<dyn CloudAgentToolsetsSeam>,
    assistant_seam: Arc<dyn AssistantModelSeam>,
) -> SchedulerDispatch {
    Arc::new(move |binding: BindingForDispatch| {
        let pool = pool.clone();
        let cloud = cloud.clone();
        let managed = managed.clone();
        let redis = redis.clone();
        let runner = runner.clone();
        let toolsets = toolsets.clone();
        let assistant_seam = assistant_seam.clone();
        Box::pin(async move {
            let now = Utc::now();
            let (lead, default) = match binding.project_id {
                Some(project_id) => project_leads(&pool, project_id)
                    .await
                    .map_err(|error| error.to_string())?,
                None => (None, None),
            };
            let universe = order_universe(binding.actor_id, None, lead, default, &[]);
            let presence = Arc::new(
                fetch_presence(&pool, &universe)
                    .await
                    .map_err(|error| error.to_string())?,
            );
            let verdicts = Arc::new(gate_verdicts(&assistant_seam, &presence).await);
            let deps = StoreDeps {
                cloud,
                managed,
                cache: RedisAdmissionCache::from_env(),
                has_usable_llm_config: gate_fn(verdicts),
                toolsets,
                llm_profile: profile_fn(presence),
                now_unix_secs: now.timestamp(),
            };
            let outcome = dispatch_scheduler_run(&pool, deps, binding.binding_id, now)
                .await
                .map_err(|error| error.to_string())?;
            // The driver's `dispatch_agent_run` only *returns* the
            // decision (`dispatch.py:61-70` drains the pod inline), so a
            // `PodDrain` must be acted on here or the run stays QUEUED
            // (PIDASHCONV-1494). The status readback below then sees the
            // drained row; its only consumer checks `FAILED`.
            for (_, decision) in &outcome.dispatch_decisions {
                if let DispatchDecision::PodDrain { pod_id } = decision {
                    drain_pod_by_id(&pool, redis.as_ref(), &runner, *pod_id)
                        .await
                        .map_err(|error| error.to_string())?;
                }
            }
            let fetched = match outcome.outcome.run {
                Some(id) => {
                    let db = pidash_db::pool::sea_orm_connection(&pool);
                    let row: Option<(String, Value)> = agent_run::Entity::find_by_id(id)
                        .select_only()
                        .columns([agent_run::Column::Status])
                        .column_as(agent_run::Column::ErrorDetails, "error_details")
                        .into_tuple::<(String, Value)>()
                        .one(&db)
                        .await
                        .map_err(|error| error.to_string())?;
                    row.map(|(status, details)| (status, run_status_error(&details)))
                }
                None => None,
            };
            to_dispatch_result(&outcome.outcome, fetched)
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pidash_services::orchestration::entries::SCHEDULER_RUN_INSERT_SQL;

    #[test]
    fn dispatch_result_mapping_pins_contract() {
        let id = Uuid::from_u128(42);
        // Produced run: fail_reason unset, status line carried.
        let ok = to_dispatch_result(
            &SchedulerOutcome {
                run: Some(id),
                error: None,
            },
            Some(("queued".to_owned(), String::new())),
        )
        .expect("produced run maps");
        assert_eq!(
            ok.run,
            Some(DispatchRun {
                id,
                status: "queued".to_owned(),
                error: String::new(),
            })
        );
        assert_eq!(ok.fail_reason, None);
        // A FAILED run still answers (run, None) — the binding pins
        // last_run and writes no last_error.
        let failed = to_dispatch_result(
            &SchedulerOutcome {
                run: Some(id),
                error: None,
            },
            Some(("failed".to_owned(), "prompt build failed: boom".to_owned())),
        )
        .expect("failed run maps");
        assert_eq!(failed.fail_reason, None);
        assert_eq!(
            failed.run.as_ref().map(|run| run.status.as_str()),
            Some("failed")
        );
        // Short-circuit: (None, reason).
        let declined = to_dispatch_result(
            &SchedulerOutcome {
                run: None,
                error: Some("no-creator".to_owned()),
            },
            None,
        )
        .expect("short-circuit maps");
        assert_eq!(declined.run, None);
        assert_eq!(declined.fail_reason.as_deref(), Some("no-creator"));
        // Produced run with a missing row: task failure, never a
        // fabricated status.
        assert!(to_dispatch_result(
            &SchedulerOutcome {
                run: Some(id),
                error: None,
            },
            None,
        )
        .is_err());
    }

    #[test]
    fn scheduler_insert_binds_match_new_scheduler_run_order() {
        // The impl binds $1..$10 in NewSchedulerRun field order; the
        // statement must carry exactly those placeholders.
        for n in 1..=10 {
            assert!(
                SCHEDULER_RUN_INSERT_SQL.contains(&format!("${n}")),
                "missing ${n}"
            );
        }
        assert!(!SCHEDULER_RUN_INSERT_SQL.contains("$11"));
    }

    #[test]
    fn run_status_error_mirrors_coalesce_message() {
        // `COALESCE(error_details ->> 'message', '')`: a string is
        // the text itself, other non-null JSON values render as JSON
        // text, and a missing key or JSON null is `""`.
        assert_eq!(
            run_status_error(&serde_json::json!({"message": "boom"})),
            "boom"
        );
        assert_eq!(run_status_error(&serde_json::json!({"message": 42})), "42");
        assert_eq!(
            run_status_error(&serde_json::json!({"message": true})),
            "true"
        );
        assert_eq!(
            run_status_error(&serde_json::json!({"message": [1, 2]})),
            "[1,2]"
        );
        assert_eq!(run_status_error(&serde_json::json!({"message": null})), "");
        assert_eq!(run_status_error(&serde_json::json!({})), "");
    }

    // -- live tests ------------------------------------------------------
    //
    // Temp copies `ON COMMIT DROP` inside one transaction over a
    // one-connection pool: the seam store reads and writes them
    // through `TxConn`, exactly as production does. Skipped when
    // `DATABASE_URL` is unset.

    use pidash_services::dispatch::admission::AdmissionCache;
    use pidash_services::dispatch::LlmProfile;
    use pidash_services::extensions::NoExtraToolsets;

    #[derive(Debug)]
    struct StubCacheError(String);

    impl std::fmt::Display for StubCacheError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.0)
        }
    }

    impl std::error::Error for StubCacheError {}

    #[derive(Debug, Default)]
    struct StubCache;

    impl AdmissionCache for StubCache {
        type Error = StubCacheError;

        fn bucket_count(&self, _key: &str) -> Result<Option<i64>, Self::Error> {
            Ok(None)
        }

        fn add_or_incr(&self, _key: &str, _timeout_secs: i64) -> Result<(), Self::Error> {
            Ok(())
        }
    }

    #[allow(clippy::type_complexity)]
    fn test_deps() -> StoreDeps<StubCache, fn(Uuid) -> bool, fn(Option<Uuid>) -> LlmProfile> {
        let settings = pidash_db::config::Settings::test_defaults();
        StoreDeps {
            cloud: settings.cloud_agent,
            managed: settings.managed_runner,
            cache: StubCache,
            has_usable_llm_config: |_| false,
            toolsets: Arc::new(NoExtraToolsets),
            llm_profile: |_| LlmProfile {
                available: false,
                reason_code: "test".to_owned(),
            },
            now_unix_secs: 0,
        }
    }

    const TEMP_TABLES: &[&str] = &[
        "workspaces",
        "projects",
        "issues",
        "pod",
        "schedulers",
        "scheduler_bindings",
        "agent_run",
        "issue_agent_ticker",
    ];

    /// Temp-copy the seam tables into the live transaction. The
    /// one-connection pool keeps every statement on one session, so
    /// the temp tables are visible to `TxConn`.
    async fn temp_tables(tx: &mut Transaction<'_>) {
        for table in TEMP_TABLES {
            sqlx::query(&format!(
                "CREATE TEMP TABLE {table} ON COMMIT DROP AS \
                 SELECT * FROM public.{table} WITH NO DATA"
            ))
            .execute(&mut **tx.inner())
            .await
            .expect("temp table copy");
        }
    }

    #[tokio::test]
    async fn live_scheduler_seam_reads() {
        let Ok(url) = std::env::var("DATABASE_URL") else {
            return;
        };
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .expect("pool connects");
        let mut tx = Transaction::begin(&pool).await.expect("begin");
        temp_tables(&mut tx).await;

        let now = Utc::now();
        let workspace_id = Uuid::new_v4();
        let project_id = Uuid::new_v4();
        let issue_id = Uuid::new_v4();
        let state_id = Uuid::new_v4();
        let pod_id = Uuid::new_v4();
        let scheduler_id = Uuid::new_v4();
        let binding_id = Uuid::new_v4();
        let creator_id = Uuid::new_v4();

        sqlx::query("INSERT INTO workspaces (id, slug, name) VALUES ($1, $2, $3)")
            .bind(workspace_id)
            .bind("live-ws")
            .bind("Live Workspace")
            .execute(&mut **tx.inner())
            .await
            .expect("seed workspace");
        sqlx::query(
            "INSERT INTO projects (id, name, agent_ticking_enabled, \
             agent_default_max_ticks, agent_default_interval_seconds, \
             agent_review_default_interval_seconds, agent_test_default_interval_seconds) \
             VALUES ($1, 'p', true, 5, 60, 120, 180)",
        )
        .bind(project_id)
        .execute(&mut **tx.inner())
        .await
        .expect("seed project");
        sqlx::query("INSERT INTO issues (id, state_id) VALUES ($1, $2)")
            .bind(issue_id)
            .bind(state_id)
            .execute(&mut **tx.inner())
            .await
            .expect("seed issue");
        sqlx::query("INSERT INTO pod (id, project_id) VALUES ($1, $2)")
            .bind(pod_id)
            .bind(project_id)
            .execute(&mut **tx.inner())
            .await
            .expect("seed pod");
        sqlx::query(
            "INSERT INTO schedulers (id, slug, name, description, prompt) \
             VALUES ($1, 'sched', 'Scheduler', 'desc', 'prompt')",
        )
        .bind(scheduler_id)
        .execute(&mut **tx.inner())
        .await
        .expect("seed scheduler");
        sqlx::query(
            "INSERT INTO scheduler_bindings \
             (id, workspace_id, project_id, scheduler_id, outcome_mode, extra_context) \
             VALUES ($1, $2, $3, $4, 'mode', '{}')",
        )
        .bind(binding_id)
        .bind(workspace_id)
        .bind(project_id)
        .bind(scheduler_id)
        .execute(&mut **tx.inner())
        .await
        .expect("seed binding");
        let run_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO agent_run \
             (id, workspace_id, created_by_id, pod_id, work_item_id, status, trigger, \
             executor_kind, phase_kind, run_config, tool_plan, error_details, prompt, \
             created_at) \
             VALUES ($1, $2, $3, $4, $5, 'queued', 'scheduler', 'cloud_agent', '', '{}', '[]', \
             '{}', '', $6)",
        )
        .bind(run_id)
        .bind(workspace_id)
        .bind(creator_id)
        .bind(pod_id)
        .bind(issue_id)
        .bind(now)
        .execute(&mut **tx.inner())
        .await
        .expect("seed queued run");
        sqlx::query(
            "INSERT INTO agent_run \
             (id, workspace_id, created_by_id, pod_id, work_item_id, status, trigger, \
             executor_kind, phase_kind, run_config, tool_plan, error_details, prompt, \
             created_at) \
             VALUES ($1, $2, $3, $4, $5, 'running', 'scheduler', 'cloud_agent', '', '{}', '[]', \
             '{}', '', $6)",
        )
        .bind(Uuid::new_v4())
        .bind(workspace_id)
        .bind(creator_id)
        .bind(pod_id)
        .bind(issue_id)
        .bind(now)
        .execute(&mut **tx.inner())
        .await
        .expect("seed running run");
        let ticker_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO issue_agent_ticker \
             (id, created_at, updated_at, deleted_at, issue_id, used, granted, waited, \
             user_disabled, next_run_at, last_tick_at, enabled, disarm_reason, pending_entry, \
             pending_entry_free, pending_entry_actor_id, pending_entry_trigger, \
             resume_parent_run_id) \
             VALUES ($1, $2, $2, NULL, $3, 0, 0, 0, false, NULL, NULL, true, '', false, false, \
             NULL, '', NULL)",
        )
        .bind(ticker_id)
        .bind(now)
        .bind(issue_id)
        .execute(&mut **tx.inner())
        .await
        .expect("seed ticker");

        let mut store = LiveCreationStore::new(tx, &pool, test_deps());

        let workspace = store.workspace(workspace_id).await.expect("workspace");
        assert_eq!(workspace.slug, "live-ws");
        assert_eq!(workspace.name, "Live Workspace");

        let policy = store.clock_policy(project_id).await.expect("clock policy");
        assert_eq!(policy.agent_ticking_enabled, Some(true));
        assert_eq!(policy.agent_default_max_ticks, Some(5));
        assert_eq!(policy.agent_default_interval_seconds, Some(60));
        assert_eq!(policy.agent_review_default_interval_seconds, Some(120));
        assert_eq!(policy.agent_test_default_interval_seconds, Some(180));

        let binding = store.binding(binding_id).await.expect("binding");
        assert_eq!(binding.workspace_id, workspace_id);
        assert_eq!(binding.project_id, Some(project_id));
        assert_eq!(binding.scheduler_id, scheduler_id);
        assert_eq!(binding.outcome_mode, "mode");

        let scheduler = store.scheduler_row(scheduler_id).await.expect("scheduler");
        assert_eq!(scheduler.slug, "sched");
        assert_eq!(scheduler.prompt, "prompt");

        let pod = store
            .scheduler_override_pod(pod_id, Some(project_id))
            .await
            .expect("override pod")
            .expect("live pod");
        assert_eq!(pod.id, pod_id);
        assert_eq!(pod.project_id, project_id);

        assert_eq!(
            store.prior_state_id(issue_id).await.expect("prior"),
            Some(state_id)
        );

        let follow_up = store
            .queued_follow_up(issue_id)
            .await
            .expect("follow-up")
            .expect("queued row");
        assert_eq!(follow_up.id, run_id);
        assert_eq!(follow_up.status, AgentRunStatus::Queued);
        assert_eq!(follow_up.trigger, "scheduler");
        assert_eq!(follow_up.executor_kind, AgentExecutorKind::CloudAgent);

        let ticker = store
            .lock_ticker(issue_id)
            .await
            .expect("lock ticker")
            .expect("live ticker");
        assert_eq!(ticker.id, ticker_id);
        assert_eq!(ticker.issue_id, issue_id);
        assert!(ticker.enabled);

        // The scheduler-run write: literals, the binding, and the
        // caller-supplied fields land, and the same transaction's
        // queued read then sees the new run.
        let new_run = NewSchedulerRun {
            id: Uuid::new_v4(),
            workspace_id,
            created_by_id: creator_id,
            pod_id,
            pinned_runner_id: None,
            binding_id,
            executor_kind: AgentExecutorKind::CloudAgent,
            error_details: serde_json::json!({}),
            tool_plan: serde_json::json!([]),
            now,
        };
        let new_run_id = new_run.id;
        let view = store
            .insert_scheduler_run(&new_run)
            .await
            .expect("insert run");
        assert_eq!(view.id, new_run_id);
        assert_eq!(view.status, AgentRunStatus::Queued);
        assert_eq!(view.trigger, "scheduler");
        assert_eq!(view.workspace_id, workspace_id);
        assert_eq!(view.pod_id, pod_id);
        assert_eq!(view.created_by_id, creator_id);
        assert_eq!(view.runner_id, None);
        assert_eq!(view.pinned_runner_id, None);
        assert_eq!(view.parent_run_id, None);
        assert_eq!(view.work_item_id, None);
        assert_eq!(view.run_config, serde_json::json!({}));
        assert_eq!(view.tool_plan, serde_json::json!([]));
        assert_eq!(view.error_details, serde_json::json!({}));
        assert_eq!(view.prompt, "");
        assert_eq!(view.prompt_manifest, None);
        assert_eq!(view.phase_kind, "");
        assert_eq!(view.ended_at, None);

        // The inserted run has `work_item_id = NULL`, so the queued
        // lookup by issue still answers the seeded run.
        assert_eq!(
            store
                .queued_follow_up(issue_id)
                .await
                .expect("follow-up again")
                .expect("seeded row")
                .id,
            run_id
        );

        // Missing rows answer the seam contract (no fabricated rows).
        assert!(matches!(
            store.binding(Uuid::new_v4()).await,
            Err(CreationError::MissingRow(_))
        ));
        assert!(matches!(
            store.clock_policy(Uuid::new_v4()).await,
            Err(CreationError::MissingRow(_))
        ));
        assert!(matches!(
            store.workspace(Uuid::new_v4()).await,
            Err(CreationError::MissingRow(_))
        ));
        assert!(matches!(
            store.scheduler_row(Uuid::new_v4()).await,
            Err(CreationError::MissingRow(_))
        ));
        assert_eq!(
            store.prior_state_id(Uuid::new_v4()).await.expect("none"),
            None
        );
        assert_eq!(store.lock_ticker(Uuid::new_v4()).await.expect("none"), None);
        assert_eq!(
            store.queued_follow_up(Uuid::new_v4()).await.expect("none"),
            None
        );
        assert_eq!(
            store
                .scheduler_override_pod(pod_id, None)
                .await
                .expect("no project"),
            None
        );
        assert_eq!(
            store
                .scheduler_override_pod(pod_id, Some(Uuid::new_v4()))
                .await
                .expect("wrong project"),
            None
        );
    }
}
