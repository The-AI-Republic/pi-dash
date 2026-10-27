#![forbid(unsafe_code)]

//! Live issue-save orchestration seam (stage 8 bug fix PIDASHCONV-1216),
//! as the api binary's binding over the public transition surface.
//!
//! The Django `Issue.post_save` receiver (`orchestration/signals.py:61-108`)
//! fires on every issue save and reconciles the ticker + creates the entry
//! run. The orchestration itself moved out of this crate (ORM-02
//! PIDASHCONV-1520) so a `WriteTxn` after-commit step can call it:
//!
//! * the pre-save snapshot is
//!   [`pidash_services::domain::transition::capture_prior_state_pool`],
//! * the completion comment and the transition fire core are
//!   [`pidash_jobs::issue_transition`].
//!
//! This module keeps the save handlers' entry points at their existing
//! paths: two re-exports plus [`fire_issue_transition`], which builds the
//! two [`StoreDeps`][pidash_jobs::orchestration::StoreDeps] from the
//! [`AppState`] and delegates to the jobs core with the api drains.
//! [`AppTransitionHook`] is the same binding as a
//! [`IssueTransitionHook`][pidash_services::domain::transition::IssueTransitionHook]
//! for binaries that register it on a [`WriteTxn`][pidash_services::domain::WriteTxn]
//! hook registry.

use std::future::Future;
use std::pin::Pin;

use pidash_jobs::issue_transition::{PodDrain, PublishEffectsDrain};
use pidash_services::domain::transition::IssueTransitionHook;
use pidash_services::orchestration::entries::{FireOutcome, FireRequest};
use pidash_services::runner_runs::LifecycleEffect;
use sqlx::PgPool;
use uuid::Uuid;

use crate::state::AppState;
use crate::v1_work_items::handlers_actions::dispatch_deps_for_ids;

/// The post-save completion comment-back, re-exported from the jobs layer.
pub(crate) use pidash_jobs::issue_transition::enqueue_completion_comment;
/// The pre-save `state_id` snapshot, re-exported from the domain layer.
pub(crate) use pidash_services::domain::transition::capture_prior_state_pool;

/// The api binary's post-commit drains for the jobs transition core:
/// the `project_move_handoff` publish drain and the `runner_poll` pod
/// drain. Both clones are cheap (`PgPool` and `AppState` are `Arc`
/// handles).
struct TransitionDrains {
    pool: PgPool,
    state: AppState,
    actor_id: Uuid,
}

impl PublishEffectsDrain for TransitionDrains {
    fn drain(
        &self,
        effects: Vec<LifecycleEffect>,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        let pool = self.pool.clone();
        let state = self.state.clone();
        let actor_id = self.actor_id;
        Box::pin(async move {
            // Web request: the crum user is the requester.
            crate::project_move_handoff::drain_publish_effects(
                &pool,
                &state,
                effects,
                Some(actor_id),
            )
            .await;
        })
    }
}

impl PodDrain for TransitionDrains {
    fn drain_pod(&self, pod_id: Uuid) -> Pin<Box<dyn Future<Output = Result<(), ()>> + Send + '_>> {
        let pool = self.pool.clone();
        let state = self.state.clone();
        Box::pin(async move {
            crate::runner_sessions::runner_poll::drain_pod_by_id(&pool, &state, pod_id).await
        })
    }
}

/// Run the post-save transition fire for one issue against live SQL.
///
/// Same signature and behaviour as before the ORM-02 move: builds the two
/// dependency bundles from the [`AppState`] and delegates to the jobs
/// core with the api drains. Public so overlay binaries (which link this
/// crate) and [`AppTransitionHook`] registrants can call it.
// The `()` error is the transition-driver convention, kept verbatim. Each
// erasure logs its cause first (PIDASHCONV-1563), as does the jobs core.
#[allow(clippy::result_unit_err)]
pub async fn fire_issue_transition(
    state: &AppState,
    pool: &PgPool,
    actor_id: Uuid,
    request: FireRequest,
) -> Result<FireOutcome, ()> {
    if request.prev_state_id == request.current_state_id {
        return Ok(FireOutcome::NoTransition);
    }
    let now_unix_secs = request.now.timestamp();
    let (store_deps, _matcher) =
        dispatch_deps_for_ids(state, pool, actor_id, request.issue_id, now_unix_secs)
            .await
            .map_err(|denial| {
                // The deps builders warn-log their own storage cause; this
                // carries the fire it 500s (PIDASHCONV-1563).
                tracing::error!(
                    denial = ?denial,
                    site = "fire-dispatch-deps",
                    issue_id = %request.issue_id,
                    "issue transition fire failure"
                );
            })?;
    let (preflight_deps, matcher) =
        dispatch_deps_for_ids(state, pool, actor_id, request.issue_id, now_unix_secs)
            .await
            .map_err(|denial| {
                tracing::error!(
                    denial = ?denial,
                    site = "fire-preflight-deps",
                    issue_id = %request.issue_id,
                    "issue transition fire failure"
                );
            })?;
    let cloud = state.settings().cloud_agent.clone();
    let drains = TransitionDrains {
        pool: pool.clone(),
        state: state.clone(),
        actor_id,
    };
    pidash_jobs::issue_transition::fire_transition(
        pool,
        store_deps,
        preflight_deps,
        matcher,
        &cloud,
        request,
        &drains,
        &drains,
    )
    .await
}

/// The api binary's [`IssueTransitionHook`]: the public transition entry
/// points bound to this binary's state and pool. Handlers that drive
/// domain writes register it on the [`WriteTxn`][pidash_services::domain::WriteTxn]
/// hook registry so after-commit steps can fire the orchestration.
pub struct AppTransitionHook {
    state: AppState,
    pool: PgPool,
}

impl AppTransitionHook {
    pub fn new(state: &AppState, pool: &PgPool) -> Self {
        Self {
            state: state.clone(),
            pool: pool.clone(),
        }
    }
}

impl IssueTransitionHook for AppTransitionHook {
    fn enqueue_completion_comment(
        &self,
        issue_id: Uuid,
        prev_state_id: Option<Uuid>,
        current_state_id: Option<Uuid>,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        let pool = self.pool.clone();
        Box::pin(async move {
            enqueue_completion_comment(
                &pool,
                &issue_id,
                prev_state_id.as_ref(),
                current_state_id.as_ref(),
            )
            .await;
        })
    }

    fn fire(
        &self,
        actor_id: Uuid,
        request: FireRequest,
    ) -> Pin<Box<dyn Future<Output = Result<FireOutcome, ()>> + Send + '_>> {
        let state = self.state.clone();
        let pool = self.pool.clone();
        Box::pin(async move { fire_issue_transition(&state, &pool, actor_id, request).await })
    }
}

/// Shared test doubles for the driver-level fire tests (`v1_work_items`
/// signal tests and the `app_issues` archive tests): only the pre-save
/// snapshot and the transition's state lookup run live against a lazy pool;
/// every other arm panics so a driver that reaches further fails loudly on
/// the no-transition / lookup-failure paths those tests pin.
#[cfg(test)]
pub(crate) mod test_support {
    use pidash_db::tasks_ticker::models::issue_agent_ticker::IssueAgentTicker;
    use pidash_services::dispatch::policy::UserFlags;
    use pidash_services::domain::identity::AgentUserCollisionError;
    use pidash_services::orchestration::clock::{ClockWrite, ProjectClockPolicy};
    use pidash_services::orchestration::creation::{
        AdmissionError, CreationError, CreationSeam, ExecutionError, ExecutionFields,
        ExecutionRequest, FinalizeAgentRunSeam, IssueView, LockedIssue, NewAgentRun, PodView,
        ProjectView, RenderBundle, RenderedTurn, RunView, RunnerView, StateView, STATE_SELECT_SQL,
    };
    use pidash_services::orchestration::entries::{
        BindingView, EntriesSeam, NewSchedulerRun, PreflightSeam, ProjectRoleFacts, SchedulerView,
        WorkspaceView, PRIOR_STATE_SELECT_SQL,
    };
    use pidash_services::prompting::composer::{OverrideIndex, OverrideRow};
    use pidash_types::dispatch::AgentExecutorKind;
    use serde_json::Value;
    use sqlx::PgPool;
    use uuid::Uuid;

    /// Test double for the fire wrapper: only `state` (the transition's state
    /// lookups) and `prior_state_id` (the pre-save snapshot) run live against
    /// the lazy pool; every other method is unreachable on these two cases and
    /// panics if a driver ever needs it.
    pub(crate) struct LazyStateSeam<'a> {
        pub(crate) pool: &'a PgPool,
    }

    fn unreached<T>() -> Result<T, CreationError> {
        unreachable!("lazy seam method is unreachable on this path")
    }

    impl CreationSeam for LazyStateSeam<'_> {
        async fn issue(&mut self, _id: Uuid) -> Result<IssueView, CreationError> {
            unreached()
        }
        async fn project(&mut self, _id: Uuid) -> Result<ProjectView, CreationError> {
            unreached()
        }
        async fn state(
            &mut self,
            state_id: Option<Uuid>,
        ) -> Result<Option<StateView>, CreationError> {
            let Some(state_id) = state_id else {
                return Ok(None);
            };
            let row: Option<(Uuid, String, String)> = sqlx::query_as(STATE_SELECT_SQL)
                .bind(state_id)
                .fetch_optional(self.pool)
                .await
                .map_err(|error| CreationError::Db(error.to_string()))?;
            Ok(row.map(|(id, name, group)| StateView { id, name, group }))
        }
        async fn latest_prior_run(&mut self, _id: Uuid) -> Result<Option<RunView>, CreationError> {
            unreached()
        }
        async fn active_run_for(&mut self, _id: Uuid) -> Result<Option<RunView>, CreationError> {
            unreached()
        }
        async fn run(&mut self, _id: Uuid) -> Result<Option<RunView>, CreationError> {
            unreached()
        }
        async fn runner(&mut self, _id: Uuid) -> Result<Option<RunnerView>, CreationError> {
            unreached()
        }
        async fn assigned_pod(&mut self, _id: Uuid) -> Result<Option<PodView>, CreationError> {
            unreached()
        }
        async fn default_pod_for_project(
            &mut self,
            _id: Uuid,
        ) -> Result<Option<PodView>, CreationError> {
            unreached()
        }
        async fn resume_parent_run_id(&mut self, _id: Uuid) -> Result<Option<Uuid>, CreationError> {
            unreached()
        }
        async fn work_item_id_for_run(&mut self, _id: Uuid) -> Result<Option<Uuid>, CreationError> {
            unreached()
        }
        async fn lock_issue_for_handoff(
            &mut self,
            _id: Uuid,
        ) -> Result<Option<LockedIssue>, CreationError> {
            unreached()
        }
        async fn lock_run_for_handoff(
            &mut self,
            _id: Uuid,
        ) -> Result<Option<RunView>, CreationError> {
            unreached()
        }
        async fn user_flags(&mut self, _id: Uuid) -> Result<UserFlags, CreationError> {
            unreached()
        }
        async fn insert_run(&mut self, _row: &NewAgentRun) -> Result<RunView, CreationError> {
            unreached()
        }
        async fn save_prompt(
            &mut self,
            _id: Uuid,
            _p: &str,
            _m: &Value,
        ) -> Result<(), CreationError> {
            unreached()
        }
        async fn save_run_config(&mut self, _id: Uuid, _c: &Value) -> Result<(), CreationError> {
            unreached()
        }
        async fn execution_fields(
            &mut self,
            _req: &ExecutionRequest,
        ) -> Result<ExecutionFields, ExecutionError> {
            unreachable!("lazy seam method is unreachable on this path")
        }
        async fn lock_cloud_creation_capacity(
            &mut self,
            _w: Uuid,
            _k: AgentExecutorKind,
            _a: bool,
        ) -> Result<Option<AdmissionError>, CreationError> {
            unreached()
        }
        fn dispatch_after_commit(&mut self, _id: Uuid) {
            unreachable!("lazy seam method is unreachable on this path")
        }
        async fn render_bundle(
            &mut self,
            _i: Uuid,
            _r: Uuid,
            _p: Option<Uuid>,
            _t: &str,
            _c: Uuid,
        ) -> Result<RenderBundle, CreationError> {
            unreached()
        }
        fn extra_toolsets_schema_tool(&self) -> String {
            String::new()
        }
    }

    impl FinalizeAgentRunSeam for LazyStateSeam<'_> {
        async fn finalize_failed_run(
            &mut self,
            _id: Uuid,
            _c: &str,
            _e: &str,
            _n: chrono::DateTime<chrono::Utc>,
        ) -> Result<RunView, CreationError> {
            unreached()
        }
    }

    impl EntriesSeam for LazyStateSeam<'_> {
        async fn prior_state_id(&mut self, issue_id: Uuid) -> Result<Option<Uuid>, CreationError> {
            let row: Option<(Uuid, Option<Uuid>)> = sqlx::query_as(PRIOR_STATE_SELECT_SQL)
                .bind(issue_id)
                .fetch_optional(self.pool)
                .await
                .map_err(|error| CreationError::Db(error.to_string()))?;
            Ok(row.and_then(|(_, state_id)| state_id))
        }
        async fn queued_follow_up(&mut self, _id: Uuid) -> Result<Option<RunView>, CreationError> {
            unreached()
        }
        async fn lock_ticker(
            &mut self,
            _id: Uuid,
        ) -> Result<Option<IssueAgentTicker>, CreationError> {
            unreached()
        }
        async fn save_ticker(
            &mut self,
            _row: &IssueAgentTicker,
            _w: ClockWrite,
        ) -> Result<(), CreationError> {
            unreached()
        }
        fn set_rollback(&mut self) {
            unreachable!("lazy seam method is unreachable on this path")
        }
        async fn clock_policy(&mut self, _id: Uuid) -> Result<ProjectClockPolicy, CreationError> {
            unreached()
        }
        async fn binding(&mut self, _id: Uuid) -> Result<BindingView, CreationError> {
            unreached()
        }
        async fn scheduler_override_pod(
            &mut self,
            _p: Uuid,
            _j: Option<Uuid>,
        ) -> Result<Option<PodView>, CreationError> {
            unreached()
        }
        async fn workspace(&mut self, _id: Uuid) -> Result<WorkspaceView, CreationError> {
            unreached()
        }
        async fn scheduler_row(&mut self, _id: Uuid) -> Result<SchedulerView, CreationError> {
            unreached()
        }
        async fn scheduler_override_rows(
            &mut self,
            _id: Uuid,
        ) -> Result<Vec<OverrideRow>, CreationError> {
            unreached()
        }
        async fn project_role_facts(
            &mut self,
            _u: Uuid,
            _s: &str,
            _p: Uuid,
        ) -> Result<ProjectRoleFacts, CreationError> {
            unreached()
        }
        async fn has_usable_llm_config(&mut self, _id: Uuid) -> Result<bool, CreationError> {
            unreached()
        }
        async fn agent_system_user(
            &mut self,
        ) -> Result<Result<Uuid, AgentUserCollisionError>, CreationError> {
            unreached()
        }
        async fn insert_scheduler_run(
            &mut self,
            _row: &NewSchedulerRun,
        ) -> Result<RunView, CreationError> {
            unreached()
        }
        fn compose_scheduler_turn(
            &mut self,
            _c: &Value,
            _i: &OverrideIndex,
            _w: Option<&str>,
            _e: Option<&str>,
            _t: i64,
        ) -> Result<RenderedTurn, String> {
            unreachable!("lazy seam method is unreachable on this path")
        }
    }

    /// Preflight is never consulted on these paths (the driver
    /// short-circuits, or the state lookup fails first): fail loudly
    /// if that stops being true.
    pub(crate) struct UnreachedPreflight;

    impl PreflightSeam for UnreachedPreflight {
        async fn preflight_eligibility_or_bounce(
            &mut self,
            _issue_id: Uuid,
            _creator_id: Uuid,
            _pod_id: Uuid,
            _triggered_by: &str,
        ) -> Result<bool, CreationError> {
            Err(CreationError::Db(
                "signal test: preflight is unreachable on this path".to_owned(),
            ))
        }
    }
}
