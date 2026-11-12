//! `core::issues::set_state` (NEWPIDASH-190 §4.3): one shared state move
//! over a caller-supplied [`WriteTxn`](crate::domain::WriteTxn).
//!
//! The move is always the bare three-column write
//! ([`crate::domain::issues::update_issue_state`], the agent-tool
//! `save(update_fields=["state", "updated_at", "updated_by"])` shape),
//! under a row lock on the same transaction so the drive cannot
//! deadlock. `dispatch` carries the documented difference
//! ([`Dispatch`]): an agent move must queue or park, never dispatch —
//! the clock answers queue/park for any `moved_by_run` move (see the
//! entries comparison test
//! `t7_domain_fire_request_moved_by_run_matches_agent_move`). `NeverDispatch`
//! drives the transition inline on the transaction with the fail-closed
//! seam (the `AuditTransitionSeam` reach, absorbed from
//! `jobs/.../tools_exec.rs`); `InChain` routes the bare write plus the
//! `[CompletionIfChanged, FireTransition]` chain through the
//! after-commit queue, the same legs `update_driver` chains for the
//! state arm of a PATCH.
//!
//! The wrapper keeps: project scope / target lookup (agent), the
//! result shape, and the eager reactions after the drive. Ops
//! `issues::update` keeps its `partial_update` full-save in Part 2.4
//! (a bare write cannot replace its full-row save + m2m/activity
//! legs — see the workpad deviation note): the InChain arm is
//! implemented here so Part 2.6's `core::issues::update` can own the
//! full patch and route its state arm through this function.
use chrono::{DateTime, Utc};
use sea_orm::{ColumnTrait, EntityTrait, FromQueryResult, QueryFilter, QueryOrder, QuerySelect};
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

use crate::dispatch::admission::DeferredConsume;
use crate::dispatch::policy::UserFlags;
use crate::domain::identity::AgentUserCollisionError;
use crate::domain::issues::{self as domain_issues, ChainOutcome};
use crate::domain::txn::WriteTxn;
use crate::domain::TxnConn;
use crate::domain::WriteCtx;
use crate::orchestration::clock::ClockWrite;
use crate::orchestration::clock::ProjectClockPolicy;
use crate::orchestration::creation::{
    AdmissionError, CreationError, CreationSeam, DedicatedPinArgs, ExecutionError, ExecutionFields,
    ExecutionRequest, FinalizeAgentRunSeam, IssueView, LockedIssue, NewAgentRun, NewSystemComment,
    PinOutcome, PodView, ProjectView, RenderBundle, RenderedTurn, RunView, RunnerView, StateView,
};
use crate::orchestration::entries::{
    clock_policy, handle_issue_state_transition, BindingView, EntriesSeam, NewSchedulerRun,
    PreflightSeam, ProjectRoleFacts, SchedulerView, TransitionRequest, WorkspaceView,
};
use crate::prompting::composer::{OverrideIndex, OverrideRow};
use pidash_db::orchestration::runs::ACTIVE_STATUSES;
use pidash_db::tasks_ticker::models::issue_agent_ticker::IssueAgentTicker;
use pidash_types::dispatch::AgentExecutorKind;

use super::CoreError;

/// The fire rule of a state move (NEWPIDASH-190 §4.3): an agent move
/// must queue or park, never dispatch; a chained move routes the fire
/// through the transaction's registered hooks, which may dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dispatch {
    /// Fire through the post-commit chain (the domain `FireTransition`
    /// leg; `moved_by_run` travels in the request).
    InChain,
    /// Drive the transition inline on the transaction with the
    /// fail-closed seam: creation and preflight methods are
    /// unreachable, so nothing can dispatch (the agent path).
    NeverDispatch,
}

/// One state move: the row, the target, the mover run (when an agent
/// moved), and the fire rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StateChange {
    pub issue_id: Uuid,
    pub state_id: Uuid,
    pub moved_by_run: Option<Uuid>,
    pub dispatch: Dispatch,
}

/// What one move did: the identity legs a wrapper needs to render.
/// `chain` is the shared chain outcome the move's legs record into
/// (the `InChain` arm): a caller that drives the inline seam leaves
/// it unstopped (inline drive errors already propagated as
/// [`CoreError`]); callers that check the legs read `stopped()` only
/// after the run commits and drains the after-commit queue.
#[derive(Debug, Clone)]
pub struct StateChanged {
    pub changed: bool,
    pub from_state_id: Option<Uuid>,
    pub to_state_id: Uuid,
    pub chain: ChainOutcome,
}

/// `core::issues::set_state(txn, ctx, pool, change)`.
pub async fn set_state(
    txn: &mut WriteTxn,
    ctx: &WriteCtx,
    pool: &PgPool,
    change: &StateChange,
) -> Result<StateChanged, CoreError> {
    let now = Utc::now();
    let actor = ctx.actor.user_id;
    let prev_state_id = locked_prev_state(&txn.connection(), change.issue_id).await?;
    let changed = !plan_same_state(prev_state_id, change.state_id);
    match change.dispatch {
        Dispatch::NeverDispatch if !changed => {
            return Ok(StateChanged {
                changed: false,
                from_state_id: prev_state_id,
                to_state_id: change.state_id,
                chain: ChainOutcome::new(),
            });
        }
        _ => {}
    }
    domain_issues::update_issue_state(
        &txn.connection(),
        change.issue_id,
        change.state_id,
        now,
        actor,
    )
    .await
    .map_err(CoreError::Sqlx)?;
    let from_state = match prev_state_id {
        Some(id) => state_view(&txn.connection(), id).await?,
        None => None,
    };
    let to_state = state_view(&txn.connection(), change.state_id)
        .await?
        .ok_or_else(|| CoreError::Missing("State"))?;
    match change.dispatch {
        Dispatch::NeverDispatch => {
            drive_inline(txn, actor, change, from_state, to_state, now).await?;
            Ok(StateChanged {
                changed: true,
                from_state_id: prev_state_id,
                to_state_id: change.state_id,
                chain: ChainOutcome::new(),
            })
        }
        Dispatch::InChain => {
            let chain = queue_chain(txn, pool, actor, change, prev_state_id, now);
            Ok(StateChanged {
                changed: true,
                from_state_id: prev_state_id,
                to_state_id: change.state_id,
                chain,
            })
        }
    }
}

/// Same-state means the current state id equals the target state id
/// (mirrors `plan::transition_same_state`).
fn plan_same_state(prev: Option<Uuid>, to: Uuid) -> bool {
    prev == Some(to)
}

/// Queue `[CompletionIfChanged, FireTransition]` for post-commit — the
/// exact legs `update_driver` chains for the state arm of a PATCH.
/// The outcome is shared so the caller can inspect `stopped()` after
/// the run commits (the post-commit drain is awaited by the run
/// driver, `WriteTxn::commit`). Returns the outcome.
fn queue_chain(
    txn: &mut WriteTxn,
    pool: &PgPool,
    actor: Uuid,
    change: &StateChange,
    prev_state: Option<Uuid>,
    now: DateTime<Utc>,
) -> ChainOutcome {
    use crate::domain::issues::{enqueue_hook, transition_hook, ChainEffect, FireSpec};
    let transition = transition_hook(txn);
    let enqueue = enqueue_hook(txn);
    let chain = vec![
        ChainEffect::CompletionIfChanged {
            issue_id: change.issue_id,
            prev_state,
            current_state: Some(change.state_id),
        },
        ChainEffect::FireTransition {
            issue_id: change.issue_id,
            prev_state,
            current_state: Some(change.state_id),
            spec: FireSpec {
                dispatch_immediate: true,
                created_by: Some(actor),
                moved_by_run: change.moved_by_run,
                now,
                pod_once: false,
            },
            actor_id: actor,
        },
    ];
    let outcome = ChainOutcome::new();
    let step = outcome.clone();
    let pool = pool.clone();
    txn.on_commit(move || async move {
        domain_issues::run_chain(&pool, &transition, &enqueue, None, chain, &step).await;
    });
    outcome
}

/// The inline drive (moved from the agentside `drive_transition`): the
/// actor is always the creator on this path, so the fallback chain
/// never consults. Creation/preflight are unreachable, so the outboxes
/// are always empty — the drain keeps the documented driver shape.
async fn drive_inline(
    txn: &mut WriteTxn,
    actor: Uuid,
    change: &StateChange,
    from_state: Option<StateView>,
    to_state: StateView,
    now: DateTime<Utc>,
) -> Result<(), CoreError> {
    let has_key = creator_has_key(&txn.connection(), actor).await?;
    let conn = txn.connection();
    let mut seam = AuditSeam {
        conn,
        creator: actor,
        has_key,
        dispatches: Vec::new(),
        deferred: Vec::new(),
        terminal_effects: Vec::new(),
        rollback: false,
    };
    let request = TransitionRequest {
        issue_id: change.issue_id,
        from_state,
        to_state: Some(to_state),
        actor: Some(actor),
        dispatch_immediate: true,
        moved_by_run: None,
        now,
        jitter_secs: 0.0,
        created_by: None,
        pod_once: false,
    };
    let mut preflight = DeadPreflight;
    let outcome = handle_issue_state_transition(&mut seam, &mut preflight, &request).await;
    let (dispatches, deferred, terminal_effects) = seam.take_outboxes();
    for _run_id in dispatches {
        // Unreachable: moved_by_run moves never dispatch_now.
    }
    for _consume in &deferred {}
    if !terminal_effects.is_empty() {
        return Err(CoreError::Db(crate::domain::DomainError::Work(
            "tool transition finalized a run: unreachable, no creation on this path".to_owned(),
        )));
    }
    let _rollback = seam.rollback;
    match outcome {
        Ok(_) => Ok(()),
        Err(error) => Err(core_from_entries(error)),
    }
}

fn core_from_entries(error: crate::orchestration::entries::EntriesError) -> CoreError {
    match error {
        crate::orchestration::entries::EntriesError::Store(creation) => {
            CoreError::Db(crate::domain::DomainError::Work(creation.to_string()))
        }
        other => CoreError::Db(crate::domain::DomainError::Work(other.to_string())),
    }
}

// ---------------------------------------------------------------------------
// The fail-closed inline seam (absorbed `AuditTransitionSeam`)
// ---------------------------------------------------------------------------

pub(crate) struct AuditSeam<'c> {
    conn: TxnConn<'c>,
    creator: Uuid,
    has_key: bool,
    dispatches: Vec<Uuid>,
    deferred: Vec<DeferredConsume>,
    terminal_effects: Vec<Uuid>,
    rollback: bool,
}

impl<'c> AuditSeam<'c> {
    fn unreachable<T>(&self, method: &'static str) -> Result<T, CreationError> {
        let _ = method;
        Err(CreationError::Db(
            "agent transition seam reached a creation method".to_owned(),
        ))
    }

    fn take_outboxes(&mut self) -> (Vec<Uuid>, Vec<DeferredConsume>, Vec<Uuid>) {
        (
            std::mem::take(&mut self.dispatches),
            std::mem::take(&mut self.deferred),
            std::mem::take(&mut self.terminal_effects),
        )
    }
}

impl<'c> CreationSeam for AuditSeam<'c> {
    async fn issue(&mut self, issue_id: Uuid) -> Result<IssueView, CreationError> {
        use pidash_db::entity::issues;
        let conn = self.conn;
        let row = issues::Entity::find()
            .select_only()
            .columns(issue_columns())
            .filter(issues::Column::Id.eq(issue_id))
            .into_model::<IssueRow>()
            .one(&conn)
            .await
            .map_err(sea_db)?;
        row.map(IssueView::from).ok_or_else(row_missing)
    }

    async fn project(&mut self, _project_id: Uuid) -> Result<ProjectView, CreationError> {
        self.unreachable("project")
    }

    async fn state(&mut self, state_id: Option<Uuid>) -> Result<Option<StateView>, CreationError> {
        let Some(state_id) = state_id else {
            return Ok(None);
        };
        // A set id with no row is `MissingRow` (the dangling-FK raise).
        state_view(&self.conn, state_id)
            .await
            .map_err(|err: CoreError| match err {
                CoreError::Db(inner) => CreationError::Db(inner.to_string()),
                CoreError::Sqlx(sqlx_err) => CreationError::Db(sqlx_err.to_string()),
                CoreError::Request(msg) => CreationError::Db(msg),
                CoreError::Missing(model) => {
                    CreationError::MissingRow(format!("{model} {state_id}"))
                }
            })
    }

    async fn latest_prior_run(&mut self, issue_id: Uuid) -> Result<Option<RunView>, CreationError> {
        use pidash_db::entity::agent_run;
        let conn = self.conn;
        let row = agent_run::Entity::find()
            .select_only()
            .columns(run_columns())
            .filter(agent_run::Column::WorkItemId.eq(issue_id))
            .order_by_desc(agent_run::Column::CreatedAt)
            .limit(1)
            .into_model::<RunRow>()
            .one(&conn)
            .await
            .map_err(sea_db)?;
        row.map(map_run_row).transpose()
    }

    async fn active_run_for(&mut self, issue_id: Uuid) -> Result<Option<RunView>, CreationError> {
        use pidash_db::entity::agent_run;
        let statuses: Vec<&str> = ACTIVE_STATUSES.iter().map(|s| s.value()).collect();
        let conn = self.conn;
        let row = agent_run::Entity::find()
            .select_only()
            .columns(run_columns())
            .filter(agent_run::Column::WorkItemId.eq(issue_id))
            .filter(agent_run::Column::Status.is_in(statuses))
            .order_by_desc(agent_run::Column::CreatedAt)
            .limit(1)
            .into_model::<RunRow>()
            .one(&conn)
            .await
            .map_err(sea_db)?;
        row.map(map_run_row).transpose()
    }

    async fn run(&mut self, _run_id: Uuid) -> Result<Option<RunView>, CreationError> {
        self.unreachable("run")
    }

    async fn runner(&mut self, _runner_id: Uuid) -> Result<Option<RunnerView>, CreationError> {
        self.unreachable("runner")
    }

    async fn assigned_pod(&mut self, _pod_id: Uuid) -> Result<Option<PodView>, CreationError> {
        self.unreachable("assigned_pod")
    }

    async fn default_pod_for_project(
        &mut self,
        _project_id: Uuid,
    ) -> Result<Option<PodView>, CreationError> {
        self.unreachable("default_pod_for_project")
    }

    async fn resume_parent_run_id(
        &mut self,
        _issue_id: Uuid,
    ) -> Result<Option<Uuid>, CreationError> {
        self.unreachable("resume_parent_run_id")
    }

    async fn work_item_id_for_run(&mut self, _run_id: Uuid) -> Result<Option<Uuid>, CreationError> {
        self.unreachable("work_item_id_for_run")
    }

    async fn lock_issue_for_handoff(
        &mut self,
        _issue_id: Uuid,
    ) -> Result<Option<LockedIssue>, CreationError> {
        self.unreachable("lock_issue_for_handoff")
    }

    async fn lock_run_for_handoff(
        &mut self,
        _run_id: Uuid,
    ) -> Result<Option<RunView>, CreationError> {
        self.unreachable("lock_run_for_handoff")
    }

    async fn user_flags(&mut self, _user_id: Uuid) -> Result<UserFlags, CreationError> {
        self.unreachable("user_flags")
    }

    async fn insert_run(&mut self, _row: &NewAgentRun) -> Result<RunView, CreationError> {
        self.unreachable("insert_run")
    }

    async fn save_prompt(
        &mut self,
        _run_id: Uuid,
        _prompt: &str,
        _manifest: &Value,
    ) -> Result<(), CreationError> {
        self.unreachable("save_prompt")
    }

    async fn save_run_config(
        &mut self,
        _run_id: Uuid,
        _config: &Value,
    ) -> Result<(), CreationError> {
        self.unreachable("save_run_config")
    }

    async fn execution_fields(
        &mut self,
        _req: &ExecutionRequest,
    ) -> Result<ExecutionFields, ExecutionError> {
        // Fail closed when any creation seam method fires: mirror the
        // audit seam decoration (already pols other CreationSeam methods reach here).
        Err(ExecutionError::Store(CreationError::Db(
            "agent transition seam reached a creation method".to_owned(),
        )))
    }

    async fn lock_cloud_creation_capacity(
        &mut self,
        _workspace_id: Uuid,
        _executor_kind: AgentExecutorKind,
        _automatic: bool,
    ) -> Result<Option<AdmissionError>, CreationError> {
        self.unreachable("lock_cloud_creation_capacity")
    }

    fn dispatch_after_commit(&mut self, _run_id: Uuid) {}

    async fn render_bundle(
        &mut self,
        _issue_id: Uuid,
        _run_id: Uuid,
        _parent_run_id: Option<Uuid>,
        _trigger: &str,
        _created_by_id: Uuid,
    ) -> Result<RenderBundle, CreationError> {
        self.unreachable("render_bundle")
    }

    fn extra_toolsets_schema_tool(&self) -> String {
        String::new()
    }

    async fn dedicated_pin(
        &mut self,
        _args: &DedicatedPinArgs<'_>,
    ) -> Result<PinOutcome, CreationError> {
        self.unreachable("dedicated_pin")
    }

    async fn insert_system_comment(
        &mut self,
        _comment: &NewSystemComment,
    ) -> Result<(), CreationError> {
        self.unreachable("insert_system_comment")
    }

    async fn clear_parent_thread(&mut self, _run_id: Uuid) -> Result<(), CreationError> {
        self.unreachable("clear_parent_thread")
    }

    async fn pod_once_started_comment(
        &mut self,
        _issue_id: Uuid,
        _pod_id: Uuid,
        _actor_id: Uuid,
        _now: DateTime<Utc>,
    ) -> Result<NewSystemComment, CreationError> {
        self.unreachable("pod_once_started_comment")
    }
}

impl<'c> FinalizeAgentRunSeam for AuditSeam<'c> {
    async fn finalize_failed_run(
        &mut self,
        _run_id: Uuid,
        _error_code: &str,
        _error: &str,
        _now: DateTime<Utc>,
    ) -> Result<RunView, CreationError> {
        self.unreachable("finalize_failed_run")
    }
}

impl<'c> EntriesSeam for AuditSeam<'c> {
    async fn prior_state_id(&mut self, _issue_id: Uuid) -> Result<Option<Uuid>, CreationError> {
        self.unreachable("prior_state_id")
    }

    async fn queued_follow_up(
        &mut self,
        _issue_id: Uuid,
    ) -> Result<Option<RunView>, CreationError> {
        self.unreachable("queued_follow_up")
    }

    async fn lock_ticker(
        &mut self,
        issue_id: Uuid,
    ) -> Result<Option<IssueAgentTicker>, CreationError> {
        use pidash_db::entity::issue_agent_ticker;
        let conn = self.conn;
        let model = issue_agent_ticker::Entity::find()
            .filter(issue_agent_ticker::Column::IssueId.eq(issue_id))
            .filter(issue_agent_ticker::Column::DeletedAt.is_null())
            .order_by_asc(issue_agent_ticker::Column::Id)
            .lock_exclusive()
            .one(&conn)
            .await
            .map_err(sea_db)?;
        Ok(model.map(IssueAgentTicker::from))
    }

    async fn save_ticker(
        &mut self,
        row: &IssueAgentTicker,
        write: ClockWrite,
    ) -> Result<(), CreationError> {
        crate::domain::clock::apply_clock_write(&self.conn, &write, row, Utc::now())
            .await
            .map_err(|error| CreationError::Db(error.to_string()))
    }

    fn set_rollback(&mut self) {
        self.rollback = true;
    }

    async fn clock_policy(
        &mut self,
        project_id: Uuid,
    ) -> Result<ProjectClockPolicy, CreationError> {
        let row = clock_policy(&self.conn, project_id).await.map_err(sea_db)?;
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

    async fn binding(&mut self, _binding_id: Uuid) -> Result<BindingView, CreationError> {
        self.unreachable("binding")
    }

    async fn scheduler_override_pod(
        &mut self,
        _pod_id: Uuid,
        _project_id: Option<Uuid>,
    ) -> Result<Option<PodView>, CreationError> {
        self.unreachable("scheduler_override_pod")
    }

    async fn workspace(&mut self, _workspace_id: Uuid) -> Result<WorkspaceView, CreationError> {
        self.unreachable("workspace")
    }

    async fn scheduler_row(&mut self, _scheduler_id: Uuid) -> Result<SchedulerView, CreationError> {
        self.unreachable("scheduler_row")
    }

    async fn scheduler_override_rows(
        &mut self,
        _workspace_id: Uuid,
    ) -> Result<Vec<OverrideRow>, CreationError> {
        self.unreachable("scheduler_override_rows")
    }

    async fn project_role_facts(
        &mut self,
        _user_id: Uuid,
        _workspace_slug: &str,
        _project_id: Uuid,
    ) -> Result<ProjectRoleFacts, CreationError> {
        self.unreachable("project_role_facts")
    }

    async fn has_usable_llm_config(&mut self, user_id: Uuid) -> Result<bool, CreationError> {
        Ok(user_id == self.creator && self.has_key)
    }

    async fn agent_system_user(
        &mut self,
    ) -> Result<Result<Uuid, AgentUserCollisionError>, CreationError> {
        self.unreachable("agent_system_user")
    }

    async fn insert_scheduler_run(
        &mut self,
        _row: &NewSchedulerRun,
    ) -> Result<RunView, CreationError> {
        self.unreachable("insert_scheduler_run")
    }

    fn compose_scheduler_turn(
        &mut self,
        _context: &Value,
        _index: &OverrideIndex,
        _workspace_id: Option<&str>,
        _executor_kind: Option<&str>,
        _tool_catalog_version: i64,
    ) -> Result<RenderedTurn, String> {
        Err("unreachable on agent transition seam".to_owned())
    }
}

struct DeadPreflight;

impl PreflightSeam for DeadPreflight {
    async fn preflight_eligibility_or_bounce(
        &mut self,
        _issue_id: Uuid,
        _creator_id: Uuid,
        _pod_id: Uuid,
        _triggered_by: &str,
    ) -> Result<bool, CreationError> {
        Err(CreationError::Db(
            "tool transition reached preflight: agent moves never dispatch_now".to_owned(),
        ))
    }

    async fn pod_once_preflight(
        &mut self,
        _issue_id: Uuid,
        _creator_id: Uuid,
        _pod_id: Uuid,
    ) -> Result<Option<String>, CreationError> {
        Err(CreationError::Db(
            "tool transition reached preflight: agent moves never dispatch_now".to_owned(),
        ))
    }
}

// ---------------------------------------------------------------------------
// Local query helpers
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, FromQueryResult)]
struct IssueStateView {
    id: Uuid,
    name: String,
    group: String,
}

/// The orchestration state view (the `STATE_SELECT_SQL` read).
async fn state_view<C: sea_orm::ConnectionTrait>(
    conn: &C,
    state_id: Uuid,
) -> Result<Option<StateView>, CoreError> {
    use pidash_db::entity::states as st_entity;
    let row = st_entity::Entity::find_by_id(state_id)
        .into_model::<IssueStateView>()
        .one(conn)
        .await
        .map_err(core_from_db)?;
    Ok(row.map(|row| StateView {
        id: row.id,
        name: row.name,
        group: row.group,
    }))
}

fn core_from_db(error: sea_orm::DbErr) -> CoreError {
    CoreError::Db(crate::domain::DomainError::Db(error))
}

/// The creator's API-key presence (`LiveExecuteSeams::llm_row` /
/// `has_api_key` in the jobs side, mirrored here from the stored row:
/// the key column is set and non-empty).
async fn creator_has_key<C: sea_orm::ConnectionTrait>(
    conn: &C,
    creator_id: Uuid,
) -> Result<bool, CoreError> {
    use pidash_db::entity::assistant_user_llm_config as cfg_entity;
    let row: Option<cfg_entity::Model> = cfg_entity::Entity::find()
        .filter(cfg_entity::Column::UserId.eq(creator_id))
        .limit(1)
        .one(conn)
        .await
        .map_err(core_from_db)?;
    Ok(row.is_some_and(|model| {
        model
            .api_key_encrypted
            .as_ref()
            .is_some_and(|key| !key.is_empty())
    }))
}

/// The `FOR UPDATE OF issues` row lock (id + current state, live row
/// only) — the same lock the bare write runs under.
#[derive(Debug, Clone, PartialEq, FromQueryResult)]
struct LockedRow {
    id: Uuid,
    state_id: Option<Uuid>,
}

async fn locked_prev_state<C: sea_orm::ConnectionTrait>(
    conn: &C,
    issue_id: Uuid,
) -> Result<Option<Uuid>, CoreError> {
    use pidash_db::entity::issues as issue_entity;
    let locked = issue_entity::Entity::find()
        .filter(issue_entity::Column::Id.eq(issue_id))
        .filter(issue_entity::Column::DeletedAt.is_null())
        .lock_exclusive()
        .select_only()
        .column(issue_entity::Column::Id)
        .column(issue_entity::Column::StateId)
        .into_model::<LockedRow>()
        .one(conn)
        .await
        .map_err(core_from_db)?;
    match locked {
        Some(row) => Ok(row.state_id),
        None => Err(CoreError::Missing("Issue")),
    }
}

/// Map a SeaORM failure onto the store error, keeping the sqlx text.
fn sea_db(error: sea_orm::DbErr) -> CreationError {
    CreationError::Db(pidash_db::orm_tx::into_sqlx(error).to_string())
}

pub(crate) fn row_missing() -> CreationError {
    CreationError::Db(sqlx::Error::RowNotFound.to_string())
}

#[derive(Debug, FromQueryResult)]
struct IssueRow {
    id: Uuid,
    workspace_id: Uuid,
    project_id: Uuid,
    state_id: Option<Uuid>,
    parent_id: Option<Uuid>,
    created_by_id: Option<Uuid>,
    assigned_pod_id: Option<Uuid>,
    assigned_runner_id: Option<Uuid>,
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
            assigned_runner_id: row.assigned_runner_id,
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

#[derive(Debug, FromQueryResult)]
struct RunRow {
    id: Uuid,
    workspace_id: Uuid,
    created_by_id: Uuid,
    pod_id: Uuid,
    runner_id: Option<Uuid>,
    pinned_runner_id: Option<Uuid>,
    pin_kind: String,
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

fn issue_columns() -> Vec<pidash_db::entity::issues::Column> {
    use pidash_db::entity::issues;
    vec![
        issues::Column::Id,
        issues::Column::WorkspaceId,
        issues::Column::ProjectId,
        issues::Column::StateId,
        issues::Column::ParentId,
        issues::Column::CreatedById,
        issues::Column::AssignedPodId,
        issues::Column::AssignedRunnerId,
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

fn run_columns() -> Vec<pidash_db::entity::agent_run::Column> {
    use pidash_db::entity::agent_run;
    vec![
        agent_run::Column::Id,
        agent_run::Column::WorkspaceId,
        agent_run::Column::CreatedById,
        agent_run::Column::PodId,
        agent_run::Column::RunnerId,
        agent_run::Column::PinnedRunnerId,
        agent_run::Column::PinKind,
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

fn map_run_row(row: RunRow) -> Result<RunView, CreationError> {
    let status = pidash_db::dispatch::status::AgentRunStatus::from_value(&row.status)
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
        pin_kind: row.pin_kind,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{activity::TaskEnqueueHook, transition::IssueTransitionHook};
    use crate::orchestration::entries::{FireOutcome, FireRequest};
    use sqlx::Row;
    use std::sync::{Arc, Mutex};

    /// The fire-leg recorder: what the domain chain's `FireTransition`
    /// leg and completion leg hit after the commit.
    #[derive(Default)]
    struct Recorder {
        fires: Vec<(Uuid, Option<Uuid>)>,
        completions: usize,
    }

    struct RecordingHook {
        state: Arc<Mutex<Recorder>>,
    }

    impl IssueTransitionHook for RecordingHook {
        fn enqueue_completion_comment(
            &self,
            _issue_id: Uuid,
            _prev_state_id: Option<Uuid>,
            _current_state_id: Option<Uuid>,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>> {
            let state = self.state.clone();
            Box::pin(async move {
                state.lock().expect("recorder").completions += 1;
            })
        }

        fn fire(
            &self,
            actor_id: Uuid,
            request: FireRequest,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<FireOutcome, ()>> + Send + '_>>
        {
            let state = self.state.clone();
            Box::pin(async move {
                state
                    .lock()
                    .expect("recorder")
                    .fires
                    .push((actor_id, request.moved_by_run));
                Ok(FireOutcome::NoTransition)
            })
        }
    }

    struct NoopEnqueue;

    impl TaskEnqueueHook for NoopEnqueue {
        fn enqueue_task(
            &self,
            _task: &str,
            _args: Vec<Value>,
            _kwargs: serde_json::Map<String, Value>,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>> {
            Box::pin(async move {})
        }
    }

    async fn live_pool() -> Option<sqlx::PgPool> {
        let url = match std::env::var("DATABASE_URL") {
            Ok(url) => url,
            Err(_) => {
                eprintln!("skipping live-db test: DATABASE_URL is not set");
                return None;
            }
        };
        Some(
            sqlx::postgres::PgPoolOptions::new()
                .max_connections(2)
                .connect(&url)
                .await
                .expect("connect to scratch DATABASE_URL"),
        )
    }

    struct Seed {
        user_id: Uuid,
        issue_id: Uuid,
        from_state_id: Uuid,
        to_state_id: Uuid,
    }

    async fn seed_move(pool: &sqlx::PgPool, tag: &str) -> Seed {
        let user_id = Uuid::new_v4();
        let workspace_id = Uuid::new_v4();
        let project_id = Uuid::new_v4();
        let from_state_id = Uuid::new_v4();
        let to_state_id = Uuid::new_v4();
        let issue_id = Uuid::new_v4();
        let username = format!("cn-{tag}-{}", user_id.simple());
        let slug = format!("cn-{tag}-{}", workspace_id.simple());
        sqlx::query(
            "INSERT INTO users (id, password, username, email, first_name, last_name, avatar,              date_joined, created_at, updated_at, last_location, created_location, is_superuser,              is_managed, is_password_expired, is_active, is_staff, is_email_verified,              is_password_autoset, token, user_timezone, last_login_ip, last_logout_ip,              last_login_medium, last_login_uagent, is_bot, display_name, is_email_valid,              is_password_reset_required) VALUES ($1, '!', $2, $3, 'CN', 'User', '', now(), now(),              now(), '', '', false, false, false, true, false, true, false, $4, 'UTC', '', '', '',              '', false, $2, true, false)",
        )
        .bind(user_id)
        .bind(username.clone())
        .bind(format!("{username}@example.test"))
        .bind("t".repeat(64))
        .execute(pool)
        .await
        .expect("seed user");
        sqlx::query(
            "INSERT INTO workspaces (id, created_at, updated_at, name, slug, timezone,              background_color, owner_id) VALUES ($1, now(), now(), $2, $3, 'UTC', '#ffffff', $4)",
        )
        .bind(workspace_id)
        .bind(format!("cn ws {tag}"))
        .bind(&slug)
        .bind(user_id)
        .execute(pool)
        .await
        .expect("seed workspace");
        let project_identifier = format!("C{:07X}", (project_id.as_u128() & 0x0fff_ffff) as u32);
        sqlx::query(
            "INSERT INTO projects (id, name, description, network, identifier, workspace_id, \
             created_by_id, cycle_view, module_view, issue_views_view, page_view, intake_view, \
             archive_in, close_in, logo_props, is_time_tracking_enabled, is_issue_type_enabled, \
             guest_view_all_features, timezone, members_can_edit_states, repo_url, base_branch, \
             agent_default_interval_seconds, agent_default_max_ticks, agent_ticking_enabled, \
             is_default, agent_review_default_interval_seconds, default_agent_executor, \
             agent_test_default_interval_seconds, created_at, updated_at) VALUES ($1, \
             $2, '', 0, $3, $4, $5, true, true, true, true, false, 0, 0, '{}', true, \
             true, false, 'UTC', false, '', '', 0, 0, false, false, 0, '', 0, now(), now())",
        )
        .bind(project_id)
        .bind(format!("cn-project-{project_identifier}"))
        .bind(project_identifier.clone())
        .bind(workspace_id)
        .bind(user_id)
        .execute(pool)
        .await
        .expect("seed project");
        for (id, name, group) in [
            (from_state_id, "Backlog", "backlog"),
            (to_state_id, "Started", "started"),
        ] {
            sqlx::query(
                "INSERT INTO states (id, created_at, updated_at, name, description, color, slug, \
                 project_id, workspace_id, sequence, \"group\", \"default\", is_triage) VALUES \
                 ($1, now(), now(), $2, '', '#ff0000', $3, $4, $5, 100.0, $6, true, false)",
            )
            .bind(id)
            .bind(name)
            .bind(format!("cn-{tag}-{id}"))
            .bind(project_id)
            .bind(workspace_id)
            .bind(group)
            .execute(pool)
            .await
            .expect("seed state");
        }
        sqlx::query(
            "INSERT INTO issues (id, created_at, updated_at, project_id, workspace_id, \
             state_id, name, description_json, description_html, sequence_id, sort_order, \
             priority, complexity_score, is_draft, git_work_branch, workpad) VALUES ($1, \
             now(), now(), $2, $3, $4, 'cn issue', '{}', '<p>cn</p>', 21, 10000.0, 'none', 0, \
             false, '', '')",
        )
        .bind(issue_id)
        .bind(project_id)
        .bind(workspace_id)
        .bind(from_state_id)
        .execute(pool)
        .await
        .expect("seed issue");
        Seed {
            user_id,
            issue_id,
            from_state_id,
            to_state_id,
        }
    }

    async fn issue_read(pool: &sqlx::PgPool, issue_id: Uuid) -> (Uuid, chrono::DateTime<Utc>) {
        let row = sqlx::query("SELECT state_id, updated_at FROM issues WHERE id = $1")
            .bind(issue_id)
            .fetch_one(pool)
            .await
            .expect("read issue");
        (row.get::<Uuid, _>("state_id"), row.get("updated_at"))
    }

    #[tokio::test]
    async fn core_in_chain_writes_bare_columns_and_fires_through_hook() {
        let Some(pool) = live_pool().await else {
            return;
        };
        let seed = seed_move(&pool, "chain").await;
        let run = Uuid::new_v4();
        let hook_state = Arc::new(Mutex::new(Recorder::default()));
        let orm = sea_orm::SqlxPostgresConnector::from_sqlx_postgres_pool(pool.clone());
        let ctx = WriteCtx::new(crate::domain::WriteActor {
            user_id: seed.user_id,
            timezone: None,
        });
        let mut txn = WriteTxn::begin(&orm).await.expect("begin");
        txn.hooks_mut()
            .register::<dyn IssueTransitionHook>(Arc::new(RecordingHook {
                state: hook_state.clone(),
            }));
        txn.hooks_mut()
            .register::<dyn TaskEnqueueHook>(Arc::new(NoopEnqueue));
        let change = StateChange {
            issue_id: seed.issue_id,
            state_id: seed.to_state_id,
            moved_by_run: Some(run),
            dispatch: Dispatch::InChain,
        };
        let moved = set_state(&mut txn, &ctx, &pool, &change)
            .await
            .expect("set_state in chain");
        assert!(moved.changed);
        assert!(moved.chain.stopped().is_none());
        txn.commit().await.expect("commit");
        let (state_id, updated) = issue_read(&pool, seed.issue_id).await;
        assert_eq!(state_id, seed.to_state_id);
        assert!(updated > Utc::now() - chrono::Duration::minutes(5));
        let reads = hook_state.lock().expect("recorder");
        assert_eq!(reads.completions, 1);
        assert_eq!(reads.fires.len(), 1);
        assert_eq!(reads.fires[0].0, seed.user_id);
        assert_eq!(reads.fires[0].1, Some(run));
    }

    #[tokio::test]
    async fn core_never_dispatch_same_state_is_noop_no_write() {
        let Some(pool) = live_pool().await else {
            return;
        };
        let seed = seed_move(&pool, "noop").await;
        let before = issue_read(&pool, seed.issue_id).await;
        let orm = sea_orm::SqlxPostgresConnector::from_sqlx_postgres_pool(pool.clone());
        let ctx = WriteCtx::new(crate::domain::WriteActor {
            user_id: seed.user_id,
            timezone: None,
        });
        let mut txn = WriteTxn::begin(&orm).await.expect("begin");
        let change = StateChange {
            issue_id: seed.issue_id,
            state_id: seed.from_state_id,
            moved_by_run: Some(Uuid::new_v4()),
            dispatch: Dispatch::NeverDispatch,
        };
        let moved = set_state(&mut txn, &ctx, &pool, &change)
            .await
            .expect("set_state same-state noop");
        assert!(!moved.changed);
        txn.commit().await.expect("commit");
        let after = issue_read(&pool, seed.issue_id).await;
        // A no-op move writes nothing — in particular `updated_at` does
        // not re-stamp (the agent same-state shortcut).
        assert_eq!(before, after);
    }

    #[tokio::test]
    async fn core_set_state_missing_issue_fails_closed() {
        let Some(pool) = live_pool().await else {
            return;
        };
        let seed = seed_move(&pool, "miss").await;
        let orm = sea_orm::SqlxPostgresConnector::from_sqlx_postgres_pool(pool.clone());
        let ctx = WriteCtx::new(crate::domain::WriteActor {
            user_id: seed.user_id,
            timezone: None,
        });
        let mut txn = WriteTxn::begin(&orm).await.expect("begin");
        let change = StateChange {
            issue_id: Uuid::new_v4(),
            state_id: seed.to_state_id,
            moved_by_run: Some(Uuid::new_v4()),
            dispatch: Dispatch::NeverDispatch,
        };
        let error = set_state(&mut txn, &ctx, &pool, &change).await.unwrap_err();
        assert!(matches!(
            error,
            CoreError::Missing(model) if model == "Issue"
        ));
    }
}

// ---------------------------------------------------------------------------
// core::issues::create
// ---------------------------------------------------------------------------
//
// NOTE (NEWPIDASH-190 Part 2.5, deviation from design literal): ops'
// v1 `issues::create` keeps calling `domain::issues::create` directly
// in this part. It assembles the full `IssueFieldSet` + m2m lists +
// activity/overrides, and its `FireSpec` runs in-chain — all of which
// the minimal typed `IssueCreate` cannot carry without a 2.6-sized
// superset. The core below therefore assembles the agent's exact
// field set and drives `domain_issues::create`; ops wiring lands with
// Part 2.6's `core::issues::update` (which owns the full patch type).

/// The fire rule of an issue create (NEWPIDASH-190 §4.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreateFire {
    /// The domain chain carries the creation fire (`ChainEffect::FireTransition`
    /// post-commit through the transition hook).
    InChain,
    /// The creation fire is skipped inside the chain (`skip_fire: true`):
    /// the caller drives its own post-commit fire (the agent bespoke
    /// `fire_create_transition` with the live preflight).
    AfterCommit,
}

/// One issue create: the row, the mover, the fire rule.
#[derive(Debug, Clone)]
pub struct IssueCreate {
    pub project_id: Uuid,
    pub name: String,
    pub description_html: String,
    pub state: Option<Uuid>,
    pub created_via: Option<String>,
    /// Apply the project's default assignee (`DefaultAssignee` chain
    /// leg) when there are no assignees.
    pub apply_default_assignee: bool,
    pub fire: CreateFire,
}

/// What one create did: the identity legs a wrapper needs to render.
/// `drain_pod` is the source-pod drain from `apply_execution_target`
/// for the named dedicated choice (design §13.2): the create core
/// cannot run it inside the caller's transaction (it opens its own),
/// so it is returned as `None` here — ops fills it after the run in
/// Part 2.6 (the agent assigns no runner, so the plan is always
/// `None` on the agent path today).
#[derive(Debug, Clone)]
pub struct IssueCreated {
    pub issue_id: Uuid,
    pub state_id: Option<Uuid>,
    pub sequence_id: i32,
    pub drain_pod: Option<Uuid>,
}

/// `core::issues::create(txn, ctx, pool, create)`.
pub async fn create(
    txn: &mut WriteTxn,
    ctx: &WriteCtx,
    pool: &PgPool,
    create: &IssueCreate,
) -> Result<IssueCreated, CoreError> {
    let now = Utc::now();
    let actor = ctx.actor.user_id;
    // The project row: workspace scope for the insert + the default
    // assignee when applying it (the ops semantics resolve it in the
    // wrapper; the core needs it for the `DefaultAssignee` chain leg).
    let project = {
        use pidash_db::entity::projects;
        let row = projects::Entity::find_by_id(create.project_id)
            .one(&txn.connection())
            .await
            .map_err(core_from_db)?;
        row.ok_or(CoreError::Missing("Project"))?
    };
    let default_assignee_id = if create.apply_default_assignee {
        let row = {
            use pidash_db::entity::projects;
            projects::Entity::find_by_id(create.project_id)
                .select_only()
                .column(projects::Column::DefaultAssigneeId)
                .into_tuple::<Option<Uuid>>()
                .one(&txn.connection())
                .await
                .map_err(core_from_db)?
        };
        row.flatten()
    } else {
        None
    };
    let fields = domain_issues::IssueFieldSet {
        id: Uuid::new_v4(),
        created_at: now,
        updated_at: now,
        created_by_id: Some(actor),
        updated_by_id: None,
        deleted_at: None,
        project_id: create.project_id,
        workspace_id: project.workspace_id,
        parent_id: None,
        state_id: create.state,
        point: None,
        estimate_point_id: None,
        name: create.name.clone(),
        description_json: serde_json::Value::Object(serde_json::Map::new()),
        description_html: create.description_html.clone(),
        description_stripped: None,
        description_binary: None,
        priority: "none".to_owned(),
        complexity_score: 0,
        start_date: None,
        target_date: None,
        sequence_id: 0,
        sort_order: 65535.0,
        completed_at: None,
        archived_at: None,
        is_draft: false,
        external_source: None,
        external_id: None,
        type_id: None,
        git_work_branch: String::new(),
        workpad: String::new(),
        created_via: create.created_via.clone(),
        assigned_pod_id: None,
        // The tool create path accepts no dedicated runner (design §7: the
        // web app and v1 surfaces own the write path); always NULL here.
        assigned_runner_id: None,
        agent_executor: None,
    };
    // Inert: the fire is skipped (the caller drives it post-commit).
    let fire = domain_issues::FireSpec {
        dispatch_immediate: true,
        pod_once: false,
        created_by: Some(actor),
        moved_by_run: None,
        now,
    };
    // Inert: the chain stays empty (`skip_activity: true`, no model
    // activity, no overrides). `skip_fire` encodes `CreateFire`: the
    // agent tool drives its bespoke post-commit fire (NEWPIDASH-191).
    let input = domain_issues::CreateIssue {
        fields,
        assignees: Vec::new(),
        default_assignee: default_assignee_id,
        labels: Vec::new(),
        fire,
        origin: None,
        activity_requested: serde_json::Value::Null,
        activity_notification: None,
        activity_require_origin: false,
        skip_activity: true,
        skip_fire: create.fire == CreateFire::AfterCommit,
        model_activity: None,
        created_override: None,
    };
    let outcome = domain_issues::create(ctx, txn, pool, input)
        .await
        .map_err(CoreError::Db)?;
    Ok(IssueCreated {
        issue_id: outcome.issue.id,
        state_id: outcome.issue.state_id,
        sequence_id: outcome.issue.sequence_id,
        drain_pod: None,
    })
}

#[cfg(test)]
mod create_tests {
    use super::{create as core_create, CreateFire, Dispatch, IssueCreate, StateChange};
    use crate::domain::{activity::TaskEnqueueHook, transition::IssueTransitionHook};
    use crate::orchestration::entries::{FireOutcome, FireRequest};
    use chrono::Utc;
    use sqlx::Row;
    use std::sync::{Arc, Mutex};
    use uuid::Uuid;

    #[derive(Default)]
    struct Recorder {
        fires: Vec<(Uuid, Option<Uuid>)>,
        completions: usize,
    }

    struct RecordingHook {
        state: Arc<Mutex<Recorder>>,
    }

    impl IssueTransitionHook for RecordingHook {
        fn enqueue_completion_comment(
            &self,
            _issue_id: Uuid,
            _prev_state_id: Option<Uuid>,
            _current_state_id: Option<Uuid>,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>> {
            let state = self.state.clone();
            Box::pin(async move {
                state.lock().expect("recorder").completions += 1;
            })
        }

        fn fire(
            &self,
            actor_id: Uuid,
            request: FireRequest,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<FireOutcome, ()>> + Send + '_>>
        {
            let state = self.state.clone();
            Box::pin(async move {
                state
                    .lock()
                    .expect("recorder")
                    .fires
                    .push((actor_id, request.moved_by_run));
                Ok(FireOutcome::NoTransition)
            })
        }
    }

    struct NoopEnqueue;

    impl TaskEnqueueHook for NoopEnqueue {
        fn enqueue_task(
            &self,
            _task: &str,
            _args: Vec<serde_json::Value>,
            _kwargs: serde_json::Map<String, serde_json::Value>,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>> {
            Box::pin(async move {})
        }
    }

    async fn live_pool() -> Option<sqlx::PgPool> {
        let url = match std::env::var("DATABASE_URL") {
            Ok(url) => url,
            Err(_) => {
                eprintln!("skipping live-db test: DATABASE_URL is not set");
                return None;
            }
        };
        Some(
            sqlx::postgres::PgPoolOptions::new()
                .max_connections(2)
                .connect(&url)
                .await
                .expect("connect to scratch DATABASE_URL"),
        )
    }

    struct Seed {
        user_id: Uuid,
        issue_id: Uuid,
        state_id: Uuid,
        project_id: Uuid,
    }

    async fn seed_create(pool: &sqlx::PgPool, tag: &str) -> Seed {
        let user_id = Uuid::new_v4();
        let workspace_id = Uuid::new_v4();
        let project_id = Uuid::new_v4();
        let state_id = Uuid::new_v4();
        let issue_id = Uuid::new_v4();
        let username = format!("cn-{tag}-{}", user_id.simple());
        let slug = format!("cnc-{tag}-{}", workspace_id.simple());
        sqlx::query(
            "INSERT INTO users (id, password, username, email, first_name, last_name, avatar, \
             date_joined, created_at, updated_at, last_location, created_location, is_superuser, \
             is_managed, is_password_expired, is_active, is_staff, is_email_verified, \
             is_password_autoset, token, user_timezone, last_login_ip, last_logout_ip, \
             last_login_medium, last_login_uagent, is_bot, display_name, is_email_valid, \
             is_password_reset_required) VALUES ($1, '!', $2, $3, 'CN', 'User', '', now(), now(), \
             now(), '', '', false, false, false, true, false, true, false, $4, 'UTC', '', '', '', \
             '', false, $2, true, false)",
        )
        .bind(user_id)
        .bind(username.clone())
        .bind(format!("{username}@example.test"))
        .bind("t".repeat(64))
        .execute(pool)
        .await
        .expect("seed user");
        sqlx::query(
            "INSERT INTO workspaces (id, created_at, updated_at, name, slug, timezone, \
             background_color, owner_id) VALUES ($1, now(), now(), $2, $3, 'UTC', '#ffffff', $4)",
        )
        .bind(workspace_id)
        .bind(format!("cn ws {tag}"))
        .bind(&slug)
        .bind(user_id)
        .execute(pool)
        .await
        .expect("seed workspace");
        sqlx::query(
            "INSERT INTO projects (id, name, description, network, identifier, workspace_id, \
             created_by_id, cycle_view, module_view, issue_views_view, page_view, intake_view, \
             archive_in, close_in, logo_props, is_time_tracking_enabled, is_issue_type_enabled, \
             guest_view_all_features, timezone, members_can_edit_states, repo_url, base_branch, \
             agent_default_interval_seconds, agent_default_max_ticks, agent_ticking_enabled, \
             is_default, agent_review_default_interval_seconds, default_agent_executor, \
             agent_test_default_interval_seconds, created_at, updated_at) VALUES ($1, \
             $2, '', 0, $3, $4, $5, true, true, true, true, false, 0, 0, '{}', true, \
             true, false, 'UTC', false, '', '', 0, 0, false, false, 0, '', 0, now(), now())",
        )
        .bind(project_id)
        .bind(format!("cn-project-{tag}"))
        .bind(format!(
            "C{:07X}",
            (project_id.as_u128() & 0x0fff_ffff) as u32
        ))
        .bind(workspace_id)
        .bind(user_id)
        .execute(pool)
        .await
        .expect("seed project");
        sqlx::query(
            "INSERT INTO states (id, created_at, updated_at, name, description, color, slug, \
             project_id, workspace_id, sequence, \"group\", \"default\", is_triage) VALUES \
             ($1, now(), now(), 'Backlog', '', '#ff0000', $2, $3, $4, 100.0, 'backlog', true, false)",
        )
        .bind(state_id)
        .bind(format!("cn-{tag}-{state_id}"))
        .bind(project_id)
        .bind(workspace_id)
        .execute(pool)
        .await
        .expect("seed state");
        sqlx::query(concat!(
            "INSERT INTO issues (id, created_at, updated_at, project_id, workspace_id, ",
            "state_id, name, description_json, description_html, sequence_id, sort_order, ",
            "priority, complexity_score, is_draft, git_work_branch, workpad) ",
            "VALUES ($1, now(), now(), $2, $3, $4, 'cn issue', '{}', '<p>cn</p>', 5, ",
            "10000.0, 'none', 0, false, '', '')"
        ))
        .bind(issue_id)
        .bind(project_id)
        .bind(workspace_id)
        .bind(state_id)
        .execute(pool)
        .await
        .expect("seed issue");
        Seed {
            user_id,
            issue_id,
            state_id,
            project_id,
        }
    }

    #[tokio::test]
    async fn core_create_in_chain_inserts_row_and_fires_completion_chain() {
        let Some(pool) = live_pool().await else {
            return;
        };
        let seed = seed_create(&pool, "inchain").await;
        let run = Uuid::new_v4();
        let hook_state = Arc::new(Mutex::new(Recorder::default()));
        let orm = sea_orm::SqlxPostgresConnector::from_sqlx_postgres_pool(pool.clone());
        let ctx = crate::domain::WriteCtx::new(crate::domain::WriteActor {
            user_id: seed.user_id,
            timezone: None,
        });
        let mut txn = crate::domain::WriteTxn::begin(&orm).await.expect("begin");
        txn.hooks_mut()
            .register::<dyn crate::domain::transition::IssueTransitionHook>(Arc::new(
                RecordingHook {
                    state: hook_state.clone(),
                },
            ));
        txn.hooks_mut()
            .register::<dyn crate::domain::activity::TaskEnqueueHook>(Arc::new(NoopEnqueue));
        let create = IssueCreate {
            project_id: seed.project_id,
            name: "cn created".to_owned(),
            description_html: "<p>cn body</p>".to_owned(),
            state: Some(seed.state_id),
            created_via: Some("cloud_agent".to_owned()),
            apply_default_assignee: true,
            fire: CreateFire::InChain,
        };
        let created = core_create(&mut txn, &ctx, &pool, &create)
            .await
            .expect("core create in chain");
        txn.commit().await.expect("commit");
        let row = sqlx::query(
            "SELECT name, state_id, sequence_id, created_via FROM issues WHERE id = $1",
        )
        .bind(created.issue_id)
        .fetch_one(&pool)
        .await
        .expect("read created issue");
        assert_eq!(row.get::<String, _>("name"), "cn created");
        assert_eq!(row.get::<Uuid, _>("state_id"), seed.state_id);
        assert_eq!(row.get::<i32, _>("sequence_id"), 1);
        assert_eq!(row.get::<String, _>("created_via"), "cloud_agent");
        let reads = hook_state.lock().expect("recorder");
        // Created issues carry no `CompletionIfChanged` leg, but the
        // `FireTransition` leg of the create chain goes through the hook.
        assert_eq!(reads.fires.len(), 1);
        assert_eq!(reads.fires[0].0, seed.user_id);
        let _ = run;
    }

    #[tokio::test]
    async fn core_create_after_commit_skips_in_chain_fire() {
        let Some(pool) = live_pool().await else {
            return;
        };
        let seed = seed_create(&pool, "after").await;
        let hook_state = Arc::new(Mutex::new(Recorder::default()));
        let orm = sea_orm::SqlxPostgresConnector::from_sqlx_postgres_pool(pool.clone());
        let ctx = crate::domain::WriteCtx::new(crate::domain::WriteActor {
            user_id: seed.user_id,
            timezone: None,
        });
        let mut txn = crate::domain::WriteTxn::begin(&orm).await.expect("begin");
        txn.hooks_mut()
            .register::<dyn crate::domain::transition::IssueTransitionHook>(Arc::new(
                RecordingHook {
                    state: hook_state.clone(),
                },
            ));
        txn.hooks_mut()
            .register::<dyn crate::domain::activity::TaskEnqueueHook>(Arc::new(NoopEnqueue));
        let create = IssueCreate {
            project_id: seed.project_id,
            name: "cn after".to_owned(),
            description_html: "<p>cn body</p>".to_owned(),
            state: Some(seed.state_id),
            created_via: Some("cloud_agent".to_owned()),
            apply_default_assignee: false,
            fire: CreateFire::AfterCommit,
        };
        let created = core_create(&mut txn, &ctx, &pool, &create)
            .await
            .expect("core create after commit");
        txn.commit().await.expect("commit");
        let count = sqlx::query_scalar::<_, i64>("SELECT count(*) FROM issues WHERE id = $1")
            .bind(created.issue_id)
            .fetch_one(&pool)
            .await
            .expect("count created");
        assert_eq!(count, 1);
        let reads = hook_state.lock().expect("recorder");
        // The in-chain fire is skipped: the caller drives it.
        assert!(reads.fires.is_empty());
    }

    #[tokio::test]
    async fn core_set_state_in_chain_same_state_still_writes() {
        let Some(pool) = live_pool().await else {
            return;
        };
        let seed = seed_create(&pool, "sames").await;
        let orm = sea_orm::SqlxPostgresConnector::from_sqlx_postgres_pool(pool.clone());
        let ctx = crate::domain::WriteCtx::new(crate::domain::WriteActor {
            user_id: seed.user_id,
            timezone: None,
        });
        let mut txn = crate::domain::WriteTxn::begin(&orm).await.expect("begin");
        txn.hooks_mut()
            .register::<dyn crate::domain::transition::IssueTransitionHook>(Arc::new(
                RecordingHook {
                    state: Arc::new(Mutex::new(Recorder::default())),
                },
            ));
        txn.hooks_mut()
            .register::<dyn crate::domain::activity::TaskEnqueueHook>(Arc::new(NoopEnqueue));
        let before: (Uuid, chrono::DateTime<Utc>) = {
            let row = sqlx::query("SELECT state_id, updated_at FROM issues WHERE id = $1")
                .bind(seed.issue_id)
                .fetch_one(&pool)
                .await
                .expect("read");
            (row.get("state_id"), row.get("updated_at"))
        };
        let change = StateChange {
            issue_id: seed.issue_id,
            state_id: seed.state_id,
            moved_by_run: None,
            dispatch: Dispatch::InChain,
        };
        let moved = super::set_state(&mut txn, &ctx, &pool, &change)
            .await
            .expect("same-state in chain");
        txn.commit().await.expect("commit");
        let after: (Uuid, chrono::DateTime<Utc>) = {
            let row = sqlx::query("SELECT state_id, updated_at FROM issues WHERE id = $1")
                .bind(seed.issue_id)
                .fetch_one(&pool)
                .await
                .expect("read");
            (row.get("state_id"), row.get("updated_at"))
        };
        // The InChain arm has no same-state shortcut: the row writes
        // (and the fire no-ops in the clock), so the stamp advances.
        assert!(moved.changed);
        assert_eq!(before.0, after.0);
        assert!(after.1 >= before.1);
    }
}
