//! Issue post-save orchestration callable from outside the api crate.
//!
//! Moved here from `pidash-api::issue_transition` (stage 8 bug fix
//! PIDASHCONV-1216; ORM-02 PIDASHCONV-1520 makes it public): the Django
//! `Issue.post_save` receiver (`orchestration/signals.py:61-108`) fires on
//! every issue save and reconciles the ticker + creates the entry run.
//!
//! [`fire_transition`] is the wrapped core of the api
//! `fire_issue_transition`. Everything it needs from the api binary — the
//! built [`StoreDeps`], the [`PodRunnerMatcher`], the cloud settings, and
//! the two post-commit drains — arrives as explicit parameters, because
//! neither this crate nor `pidash-services` can name `AppState` (the crate
//! graph is `types` → `db` → `services` → `api`, with jobs beside api).
//! The api module keeps a same-signature wrapper that builds those from
//! its state; a `WriteTxn` after-commit step in a jobs context calls this
//! core directly, and a services-side step calls it through the
//! [`IssueTransitionHook`][pidash_services::domain::transition::IssueTransitionHook]
//! the binary registered.
//!
//! The ticker/clock split is load-bearing (kept verbatim from the api
//! module): the reconcile `clock` lock (`SELECT … FOR UPDATE`) must not be
//! held across `preflight_eligibility_or_bounce`, whose bounce opens its
//! own transaction and locks the same ticker row — a held lock would
//! deadlock the request. Autocommit per ticker statement mirrors Django's
//! own separate `reconcile` atomic (which commits before preflight) and
//! the shipped v1 ticker seam (PIDASHCONV-856).
//!
//! Post-commit work mirrors `_create_and_dispatch_run`'s `on_commit` drain
//! (`creation.py:148-152`): dispatch each created run, drain a pod for a
//! machine executor, publish finalized-run terminal effects, and consume
//! any deferred admission tokens.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use pidash_db::config::{CloudAgentSettings, ManagedRunnerSettings, RunnerSettings};
use pidash_db::tasks_ticker::models::issue_agent_ticker::IssueAgentTicker;
use pidash_db::tx::Transaction;
use pidash_services::dispatch::admission::AdmissionCache;
use pidash_services::dispatch::policy::UserFlags;
use pidash_services::dispatch::LlmProfile;
use pidash_services::domain::activity::TaskEnqueueHook;
use pidash_services::domain::clock::apply_clock_write;
use pidash_services::domain::identity::AgentUserCollisionError;
use pidash_services::domain::transition::IssueTransitionHook;
use pidash_services::extensions::{AssistantModelSeam, CloudAgentToolsetsSeam};
use pidash_services::orchestration::clock::{ClockWrite, ProjectClockPolicy};
use pidash_services::orchestration::creation::{
    AdmissionError, CreationError, CreationSeam, DedicatedPinArgs, ExecutionError, ExecutionFields,
    ExecutionRequest, FinalizeAgentRunSeam, IssueView, LockedIssue, NewAgentRun, NewSystemComment,
    PinOutcome, PodView, ProjectView, RenderBundle, RenderedTurn, RunView, RunnerView, StateView,
};
use pidash_services::orchestration::dispatch::PodRunnerMatcher;
use pidash_services::orchestration::entries::{
    fire_state_transition, BindingView, EntriesSeam, FireOutcome, FireRequest, NewSchedulerRun,
    PreflightSeam, ProjectRoleFacts, SchedulerView, WorkspaceView,
};
use pidash_services::prompting::composer::{OverrideIndex, OverrideRow};
use pidash_services::runner_runs::LifecycleEffect;
use pidash_types::dispatch::AgentExecutorKind;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect};
use serde_json::{Map, Value};
use sqlx::PgPool;
use uuid::Uuid;

use crate::dispatch::{dispatch_agent_run, DispatchDecision};
use crate::orchestration::{LiveCreationStore, StoreDeps};

/// Drain finalized runs' publish pairs in order. The api binary implements
/// this over its `project_move_handoff` drain; a jobs-context caller passes
/// its own.
pub trait PublishEffectsDrain: Send + Sync {
    fn drain(&self, effects: Vec<LifecycleEffect>)
        -> Pin<Box<dyn Future<Output = ()> + Send + '_>>;
}

/// Drain one pod by id (`matcher.py:242-251`). The api binary implements
/// this over its `runner_poll` drain; a jobs-context caller passes its own.
pub trait PodDrain: Send + Sync {
    fn drain_pod(&self, pod_id: Uuid) -> Pin<Box<dyn Future<Output = Result<(), ()>> + Send + '_>>;
}

/// Run the post-save transition fire for one issue.
///
/// The wrapped core of the api `fire_issue_transition` (same body, the
/// `AppState`-derived inputs as explicit parameters): `store_deps` and
/// `preflight_deps` are the two [`StoreDeps`] builds (creation drive and
/// preflight each take theirs by value), `matcher` the pod/runner matcher,
/// `cloud` the cloud-agent settings for dispatch, and the two drains the
/// post-commit effects. The actor id stays with the caller: it builds the
/// deps and binds the drains.
///
/// A non-transition answers [`FireOutcome::NoTransition`] without opening a
/// transaction. A handler failure rolls the driver's creation transaction
/// back and answers [`FireOutcome::Failed`] (the caller logs the verbatim
/// line; the save itself stands). A successful drive commits and drains the
/// creation outboxes.
// The `()` error is the transition-driver convention (callers log the
// verbatim line; the save stands either way), kept from the api original.
// Every erasure below logs its cause first (PIDASHCONV-1563): the web
// handlers map `()` to a 500 with no other trace, so without these logs
// the next fire failure needs temporary logging to diagnose.
fn fire_erased<E: std::fmt::Display>(error: E, site: &str, request: &FireRequest) {
    tracing::error!(
        %error,
        site,
        issue_id = %request.issue_id,
        "issue transition fire failure"
    );
}
#[allow(clippy::too_many_arguments, clippy::result_unit_err)]
pub async fn fire_transition<C, H, P, M>(
    pool: &PgPool,
    store_deps: StoreDeps<C, H, P>,
    preflight_deps: StoreDeps<C, H, P>,
    matcher: M,
    cloud: &CloudAgentSettings,
    request: FireRequest,
    publish_drain: &dyn PublishEffectsDrain,
    pod_drain: &dyn PodDrain,
) -> Result<FireOutcome, ()>
where
    C: AdmissionCache,
    H: Fn(Uuid) -> bool + Clone + Send + Sync,
    P: Fn(Option<Uuid>) -> LlmProfile + Clone + Send + Sync,
    M: PodRunnerMatcher,
{
    if request.prev_state_id == request.current_state_id {
        return Ok(FireOutcome::NoTransition);
    }
    let tx = Transaction::begin(pool)
        .await
        .map_err(|error| fire_erased(error, "fire-tx-begin", &request))?;
    let mut seam = LiveTransitionSeam {
        pool,
        store: LiveCreationStore::new(tx, pool, store_deps),
    };
    let mut preflight = LivePreflight {
        pool: pool.clone(),
        deps: Some(preflight_deps),
        matcher,
        now: request.now,
        jitter_secs: request.jitter_secs,
    };

    let outcome = fire_state_transition(&mut seam, &mut preflight, &request).await;
    let (tx, _pool, cache, dispatches, deferred, terminal_effects) = seam.store.into_parts();

    let outcome = match outcome {
        Ok(outcome) => outcome,
        Err(error) => {
            fire_erased(&error, "fire-drive", &request);
            let _ = tx.rollback().await;
            return Err(());
        }
    };
    if !matches!(outcome, FireOutcome::Called(_)) {
        // NoTransition / Failed: nothing this drive committed is kept.
        let _ = tx.rollback().await;
        return Ok(outcome);
    }
    tx.commit()
        .await
        .map_err(|error| fire_erased(&error, "fire-tx-commit", &request))?;

    if !terminal_effects.is_empty() {
        let mut pairs = Vec::with_capacity(terminal_effects.len() * 2);
        for id in terminal_effects {
            pairs.extend(pidash_services::runner_runs::finalization::plan_publish_effects(id));
        }
        publish_drain.drain(pairs).await;
    }
    for run_id in dispatches {
        match dispatch_agent_run(pool, cloud, run_id, request.now).await {
            Ok(DispatchDecision::PodDrain { pod_id }) => {
                if let Err(()) = pod_drain.drain_pod(pod_id).await {
                    // The drain logs its own cause where the storage error
                    // is still in hand; this breadcrumb carries the fire.
                    tracing::error!(
                        site = "fire-pod-drain",
                        issue_id = %request.issue_id,
                        %run_id,
                        %pod_id,
                        "issue transition fire failure"
                    );
                    return Err(());
                }
            }
            Ok(_) => {}
            Err(error) => {
                tracing::error!(
                    %error,
                    site = "fire-dispatch",
                    issue_id = %request.issue_id,
                    %run_id,
                    "issue transition fire failure"
                );
                return Err(());
            }
        }
    }
    for consume in &deferred {
        pidash_services::dispatch::admission::consume_admission_token(&cache, consume);
    }
    Ok(outcome)
}

/// Post-commit completion comment-back for a state-changing save
/// (`github_signals.py:44-75`, via the jobs-layer port; the
/// `tools_local_store::enqueue_completion_comment` shape from
/// PIDASHCONV-1252, shared by every app_issues/v1 write path —
/// PIDASHCONV-1460). The caller gates on its own transition check
/// (`prev != current`); this resolves the dispatch (group, mirror,
/// idempotency guards) and enqueues the provider's
/// `post_completion_comment` job. Best-effort: failures only warn, the
/// save stands.
pub async fn enqueue_completion_comment(
    pool: &PgPool,
    issue_id: &Uuid,
    prev_state_id: Option<&Uuid>,
    current_state_id: Option<&Uuid>,
) {
    let Some(job) = completion_comment_job(pool, issue_id, prev_state_id, current_state_id).await
    else {
        return;
    };
    if let Err(error) = crate::queue::enqueue(pool, &job).await {
        tracing::warn!(%error, task = %job.task, "task enqueue failed; op stands");
    }
}

/// The completion-comment dispatch lookup half of
/// [`enqueue_completion_comment`]: resolve the dispatch (group, mirror,
/// idempotency guards) and build the provider's `post_completion_comment`
/// job, or `None` when the leg skips. Best-effort: failures only warn.
/// Shared with the public [`hooks::TransitionHook`][crate::hooks::TransitionHook],
/// which sends the job through its queue sink instead of the outbox.
pub(crate) async fn completion_comment_job(
    pool: &PgPool,
    issue_id: &Uuid,
    prev_state_id: Option<&Uuid>,
    current_state_id: Option<&Uuid>,
) -> Option<crate::queue::NewJob> {
    use crate::integrations::github_signals::{
        dispatch_completion_comment, dispatch_job, DispatchOutcome,
    };
    let outcome =
        dispatch_completion_comment(pool, issue_id, current_state_id, false, prev_state_id).await;
    let target = match outcome {
        Ok(DispatchOutcome::Dispatch(target)) => target,
        Ok(DispatchOutcome::Skip(_)) => return None,
        Err(error) => {
            tracing::warn!(%error, "completion comment dispatch lookup failed; op stands");
            return None;
        }
    };
    Some(dispatch_job(&target))
}

/// The ticker/clock seam methods run pool-direct (autocommit); everything
/// else delegates to the tx-backed [`LiveCreationStore`].
struct LiveTransitionSeam<'a, C, H, P> {
    pool: &'a PgPool,
    store: LiveCreationStore<'a, C, H, P>,
}

impl<C, H, P> LiveTransitionSeam<'_, C, H, P> {
    fn db(error: sqlx::Error) -> CreationError {
        CreationError::Db(error.to_string())
    }

    fn db_orm(error: sea_orm::DbErr) -> CreationError {
        CreationError::Db(orm_error_text(error))
    }
}

/// A SeaORM failure as the text the old sqlx read raised: the wrapped sqlx
/// error when there is one (so Postgres codes still classify), otherwise its
/// message (cf. `tools_exec::orm_error`).
fn orm_error_text(error: sea_orm::DbErr) -> String {
    match error {
        sea_orm::DbErr::Query(sea_orm::RuntimeErr::SqlxError(inner))
        | sea_orm::DbErr::Exec(sea_orm::RuntimeErr::SqlxError(inner))
        | sea_orm::DbErr::Conn(sea_orm::RuntimeErr::SqlxError(inner)) => inner.to_string(),
        sea_orm::DbErr::ConnectionAcquire(sea_orm::ConnAcquireErr::Timeout) => {
            sqlx::Error::PoolTimedOut.to_string()
        }
        sea_orm::DbErr::ConnectionAcquire(sea_orm::ConnAcquireErr::ConnectionClosed) => {
            sqlx::Error::PoolClosed.to_string()
        }
        other => other.to_string(),
    }
}

// ---------------------------------------------------------------------------
// Preflight seam: the jobs `preflight_eligibility_or_bounce` driver
// ---------------------------------------------------------------------------

/// The eligibility preflight over the request pool (its own transaction and
/// commit, like Django's bounce atomic). The deps travel by `Option` because
/// the driver takes them by value while the seam only lends `&mut self`; the
/// driver consults preflight at most once per fire, so the `None` arm is
/// defensive.
struct LivePreflight<C, H, P, M> {
    pool: PgPool,
    deps: Option<StoreDeps<C, H, P>>,
    matcher: M,
    now: chrono::DateTime<chrono::Utc>,
    jitter_secs: f64,
}

impl<C, H, P, M> PreflightSeam for LivePreflight<C, H, P, M>
where
    C: AdmissionCache,
    H: Fn(Uuid) -> bool + Clone + Send + Sync,
    P: Fn(Option<Uuid>) -> LlmProfile + Clone + Send + Sync,
    M: PodRunnerMatcher,
{
    async fn preflight_eligibility_or_bounce(
        &mut self,
        issue_id: Uuid,
        creator_id: Uuid,
        pod_id: Uuid,
        triggered_by: &str,
    ) -> Result<bool, CreationError> {
        let Some(deps) = self.deps.take() else {
            return Err(CreationError::Db(
                "issue transition preflight: deps already consumed".to_owned(),
            ));
        };
        let out = crate::orchestration::preflight_eligibility_or_bounce(
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
        .map_err(|error| CreationError::Db(error.to_string()))?;
        for line in &out.outcome.logs {
            if line.level == "WARNING" {
                tracing::warn!("{}", line.message);
            } else {
                tracing::info!("{}", line.message);
            }
        }
        Ok(out.outcome.proceed)
    }

    async fn pod_once_preflight(
        &mut self,
        issue_id: Uuid,
        creator_id: Uuid,
        pod_id: Uuid,
    ) -> Result<Option<String>, CreationError> {
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
        let has = self
            .matcher
            .pod_has_runner_for_issue_principal(pod_id, issue_id, Some(creator_id), true, true)
            .await
            .map_err(|error| CreationError::Db(error.to_string()))?;
        Ok(if has {
            None
        } else {
            Some("no_eligible_runner_in_pod".to_owned())
        })
    }
}

// ---------------------------------------------------------------------------
// Ticker / clock reads (SeaORM; the L5 consts' shapes, rebuilt)
// ---------------------------------------------------------------------------
//
// Before TICKER-ORCH-1 these read the L5 consts verbatim
// (`PRIOR_STATE_SELECT_SQL`, `PROJECT_CLOCK_POLICY_SQL`, `lock_ticker_sql()`).
// The builders below reproduce those statements exactly — same tables,
// filters, ordering, limit and lock — without editing the shared consts
// outside this batch's exclusive scope.

/// `PRIOR_STATE_SELECT_SQL` (`SELECT id, state_id FROM issues WHERE id = $1`,
/// `entries.rs:195`): pk plus `state_id` only, soft-deleted rows included.
/// Narrow read via `select_only` + `FromQueryResult`.
#[derive(Debug, Clone, PartialEq, Eq, sea_orm::FromQueryResult)]
struct PriorStateRow {
    id: Uuid,
    state_id: Option<Uuid>,
}

fn prior_state_select(issue_id: Uuid) -> sea_orm::Select<pidash_db::entity::issues::Entity> {
    use pidash_db::entity::issues;
    issues::Entity::find_by_id(issue_id)
        .select_only()
        .columns([issues::Column::Id, issues::Column::StateId])
}

/// `lock_ticker_sql()` (`clock.rs:136`): the issue's ticker row, locked —
/// `WHERE issue_id = $1 AND deleted_at IS NULL ORDER BY id ASC LIMIT 1
/// FOR UPDATE` over the full 20-column ticker shape.
fn lock_ticker_select(
    issue_id: Uuid,
) -> sea_orm::Select<pidash_db::entity::issue_agent_ticker::Entity> {
    use pidash_db::entity::issue_agent_ticker;
    issue_agent_ticker::Entity::find()
        .filter(issue_agent_ticker::Column::IssueId.eq(issue_id))
        .filter(issue_agent_ticker::Column::DeletedAt.is_null())
        .order_by_asc(issue_agent_ticker::Column::Id)
        .limit(1)
        .lock_exclusive()
}

/// `PROJECT_CLOCK_POLICY_SQL`
/// (`SELECT agent_ticking_enabled, agent_default_max_ticks,
/// agent_default_interval_seconds, agent_review_default_interval_seconds,
/// agent_test_default_interval_seconds FROM projects WHERE id = $1`,
/// `dispatch.rs:183`). `Option` decode keeps the `getattr`-default rows
/// the old `map_policy` read; the cadence columns widen `i32 → i64` there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, sea_orm::FromQueryResult)]
struct ClockPolicyRow {
    agent_ticking_enabled: Option<bool>,
    agent_default_max_ticks: Option<i32>,
    agent_default_interval_seconds: Option<i32>,
    agent_review_default_interval_seconds: Option<i32>,
    agent_test_default_interval_seconds: Option<i32>,
}

impl From<ClockPolicyRow> for ProjectClockPolicy {
    fn from(row: ClockPolicyRow) -> Self {
        Self {
            agent_ticking_enabled: row.agent_ticking_enabled,
            agent_default_max_ticks: row.agent_default_max_ticks,
            agent_default_interval_seconds: row.agent_default_interval_seconds.map(i64::from),
            agent_review_default_interval_seconds: row
                .agent_review_default_interval_seconds
                .map(i64::from),
            agent_test_default_interval_seconds: row
                .agent_test_default_interval_seconds
                .map(i64::from),
        }
    }
}

fn clock_policy_select(project_id: Uuid) -> sea_orm::Select<pidash_db::entity::projects::Entity> {
    use pidash_db::entity::projects;
    projects::Entity::find()
        .filter(projects::Column::Id.eq(project_id))
        .select_only()
        .columns([
            projects::Column::AgentTickingEnabled,
            projects::Column::AgentDefaultMaxTicks,
            projects::Column::AgentDefaultIntervalSeconds,
            projects::Column::AgentReviewDefaultIntervalSeconds,
            projects::Column::AgentTestDefaultIntervalSeconds,
        ])
}

// ---------------------------------------------------------------------------
// Seam impls
// ---------------------------------------------------------------------------

impl<C, H, P> CreationSeam for LiveTransitionSeam<'_, C, H, P>
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
        now: chrono::DateTime<chrono::Utc>,
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

impl<C, H, P> FinalizeAgentRunSeam for LiveTransitionSeam<'_, C, H, P>
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

impl<C, H, P> EntriesSeam for LiveTransitionSeam<'_, C, H, P>
where
    C: AdmissionCache,
    H: Fn(Uuid) -> bool + Clone + Send + Sync,
    P: Fn(Option<Uuid>) -> LlmProfile + Clone + Send + Sync,
{
    async fn prior_state_id(&mut self, issue_id: Uuid) -> Result<Option<Uuid>, CreationError> {
        let db = pidash_db::pool::sea_orm_connection(self.pool);
        let row = prior_state_select(issue_id)
            .into_model::<PriorStateRow>()
            .one(&db)
            .await
            .map_err(Self::db_orm)?;
        Ok(row.and_then(|row| row.state_id))
    }

    async fn queued_follow_up(&mut self, issue_id: Uuid) -> Result<Option<RunView>, CreationError> {
        self.store.queued_follow_up(issue_id).await
    }

    async fn lock_ticker(
        &mut self,
        issue_id: Uuid,
    ) -> Result<Option<IssueAgentTicker>, CreationError> {
        let db = pidash_db::pool::sea_orm_connection(self.pool);
        let row = lock_ticker_select(issue_id)
            .one(&db)
            .await
            .map_err(Self::db_orm)?;
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
            chrono::Utc::now(),
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
        let row = clock_policy_select(project_id)
            .into_model::<ClockPolicyRow>()
            .one(&db)
            .await
            .map_err(Self::db_orm)?;
        row.map(ProjectClockPolicy::from)
            .ok_or_else(|| CreationError::Db(sqlx::Error::RowNotFound.to_string()))
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

// ---------------------------------------------------------------------------
// Worker hook: the jobs binding over the transition entry points
// ---------------------------------------------------------------------------

/// The worker's [`IssueTransitionHook`]: the public
/// [`hooks::TransitionHook`][crate::hooks::TransitionHook] over worker
/// settings + seams and the queue/live drains. The worker installs one
/// over its pools/settings/seams and hands it to the sync drivers, so
/// sync-created/adopted mirrors run the same post-save legs as every
/// other issue write (PIDASHCONV-1576).
pub struct WorkerTransitionHook {
    hook: crate::hooks::TransitionHook,
}

impl WorkerTransitionHook {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        pool: PgPool,
        cloud: CloudAgentSettings,
        managed: ManagedRunnerSettings,
        redis: Option<redis::Client>,
        runner: RunnerSettings,
        toolsets: Arc<dyn CloudAgentToolsetsSeam>,
        assistant_seam: Arc<dyn AssistantModelSeam>,
    ) -> Self {
        use crate::hooks::{
            HookDeps, LivePodDrain, OutboxTaskSink, QueuePublishDrain, QueueTerminalEffectsDrain,
            TransitionHook,
        };
        let publish = Arc::new(QueuePublishDrain::new(&pool));
        let pods = Arc::new(LivePodDrain::new(&pool, redis.clone(), runner.clone()));
        let terminal = Arc::new(QueueTerminalEffectsDrain::new(&pool));
        let deps = HookDeps {
            pool,
            cloud,
            managed,
            cache: crate::admission::RedisAdmissionCache::from_env(),
            redis,
            runner,
            toolsets,
            assistant_seam,
            publish,
            pods,
            terminal,
            queue: Arc::new(OutboxTaskSink),
        };
        Self {
            hook: TransitionHook::new(&deps),
        }
    }
}

impl IssueTransitionHook for WorkerTransitionHook {
    fn enqueue_completion_comment(
        &self,
        issue_id: Uuid,
        prev_state_id: Option<Uuid>,
        current_state_id: Option<Uuid>,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        self.hook
            .enqueue_completion_comment(issue_id, prev_state_id, current_state_id)
    }

    fn fire(
        &self,
        actor_id: Uuid,
        request: FireRequest,
    ) -> Pin<Box<dyn Future<Output = Result<FireOutcome, ()>> + Send + '_>> {
        self.hook.fire(actor_id, request)
    }
}

/// The worker's [`TaskEnqueueHook`]: the public
/// [`hooks::EnqueueHook`][crate::hooks::EnqueueHook] over the outbox
/// sink. Worker drivers that drive domain writes (the cloud-agent
/// tools, PIDASHCONV-1596) register one on the
/// [`WriteTxn`][pidash_services::domain::WriteTxn] hook registry next
/// to the [`WorkerTransitionHook`]. Enqueue failures log and the save
/// stands.
pub struct WorkerTaskEnqueueHook {
    hook: crate::hooks::EnqueueHook,
}

impl WorkerTaskEnqueueHook {
    pub fn new(pool: &PgPool) -> Self {
        use crate::hooks::{EnqueueHook, OutboxTaskSink};
        Self {
            hook: EnqueueHook::new(pool, Arc::new(OutboxTaskSink)),
        }
    }
}

impl TaskEnqueueHook for WorkerTaskEnqueueHook {
    fn enqueue_task(
        &self,
        task: &str,
        args: Vec<Value>,
        kwargs: Map<String, Value>,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        self.hook.enqueue_task(task, args, kwargs)
    }
}

/// Post-save completion comment-back for a sync write (the
/// `trigger_completion_comment` receiver leg,
/// `github_signals.py:44-75`). The hook resolves the dispatch (group,
/// mirror, idempotency guards) and enqueues the provider's
/// `post_completion_comment` job; best-effort, like the receiver. No
/// hook installed (direct unit-test calls) or no state change means no
/// dispatch evaluation at all.
pub async fn comment_sync_transition(
    hook: &Option<Arc<dyn IssueTransitionHook>>,
    issue_id: Uuid,
    prev_state_id: Option<Uuid>,
    current_state_id: Option<Uuid>,
) {
    let Some(hook) = hook else {
        return;
    };
    if prev_state_id == current_state_id {
        return;
    }
    hook.enqueue_completion_comment(issue_id, prev_state_id, current_state_id)
        .await;
}

/// Post-save orchestration fire for a sync write (the
/// `fire_state_transition` receiver leg, `signals.py:74-107`).
/// Best-effort — Django swallows orchestration errors into its error
/// counter, so a failed fire never fails the sync. No hook installed
/// (direct unit-test calls) or no state change means no fire at all.
pub async fn fire_sync_transition(
    hook: &Option<Arc<dyn IssueTransitionHook>>,
    actor_id: Uuid,
    issue_id: Uuid,
    prev_state_id: Option<Uuid>,
    current_state_id: Option<Uuid>,
    now: chrono::DateTime<chrono::Utc>,
) {
    let Some(hook) = hook else {
        return;
    };
    if prev_state_id == current_state_id {
        return;
    }
    let request = FireRequest {
        issue_id,
        prev_state_id,
        current_state_id,
        // The sync path never stamps `_DISPATCH_IMMEDIATE_ATTR`, so the
        // receiver default (`True`) always applies.
        dispatch_immediate: true,
        pod_once: false,

        // A sync write is never an agent's own move.
        moved_by_run: None,
        now,
        // Caller-drawn jitter (L5 quirk-5): the worker path draws
        // none, like the api fires.
        jitter_secs: 0.0,
        // System path: no crum user in the sync worker.
        created_by: None,
    };
    match hook.fire(actor_id, request).await {
        Ok(FireOutcome::Failed { log_line, .. }) => {
            tracing::error!("{log_line}");
        }
        Ok(_) => {}
        Err(()) => {
            tracing::warn!(%issue_id, "sync transition fire failed; mirror stands");
        }
    }
}

/// Shared test doubles for the sync-driver leg tests: a recording
/// [`IssueTransitionHook`] (the services `RecordingHook` shape) that
/// answers every fire with `NoTransition`, so driver tests pin the
/// call sites without driving the orchestration.
#[cfg(test)]
pub(crate) mod test_support {
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::Mutex;

    use pidash_services::domain::transition::IssueTransitionHook;
    use pidash_services::orchestration::entries::{FireOutcome, FireRequest};
    use uuid::Uuid;

    /// One recorded fire: the actor plus the request's signal fields.
    pub(crate) type FireCall = (Uuid, Uuid, Option<Uuid>, Option<Uuid>, bool, Option<Uuid>);
    /// One recorded completion dispatch: issue, previous, current.
    pub(crate) type CommentCall = (Uuid, Option<Uuid>, Option<Uuid>);

    pub(crate) struct RecordingHook {
        pub(crate) fires: Mutex<Vec<FireCall>>,
        pub(crate) comments: Mutex<Vec<CommentCall>>,
    }

    impl RecordingHook {
        pub(crate) fn new() -> Self {
            Self {
                fires: Mutex::new(Vec::new()),
                comments: Mutex::new(Vec::new()),
            }
        }
    }

    impl Default for RecordingHook {
        fn default() -> Self {
            Self::new()
        }
    }

    impl IssueTransitionHook for RecordingHook {
        fn enqueue_completion_comment(
            &self,
            issue_id: Uuid,
            prev_state_id: Option<Uuid>,
            current_state_id: Option<Uuid>,
        ) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
            self.comments
                .lock()
                .expect("lock")
                .push((issue_id, prev_state_id, current_state_id));
            Box::pin(async move {})
        }

        fn fire(
            &self,
            actor_id: Uuid,
            request: FireRequest,
        ) -> Pin<Box<dyn Future<Output = Result<FireOutcome, ()>> + Send + '_>> {
            self.fires.lock().expect("lock").push((
                actor_id,
                request.issue_id,
                request.prev_state_id,
                request.current_state_id,
                request.dispatch_immediate,
                request.created_by,
            ));
            Box::pin(async move { Ok(FireOutcome::NoTransition) })
        }
    }
}

#[cfg(test)]
mod ticker_orch_orm_tests {
    use super::*;
    use sea_orm::{DbBackend, QueryTrait};

    fn sql_of<E>(select: sea_orm::Select<E>) -> String
    where
        E: sea_orm::EntityTrait,
    {
        select.build(DbBackend::Postgres).to_string()
    }

    #[test]
    fn prior_state_select_is_narrow_pk_read() {
        let id = Uuid::parse_str("123e4567-e89b-12d3-a456-426614174000").unwrap();
        let sql = sql_of(prior_state_select(id));
        assert!(sql.contains("SELECT"), "select:\n{sql}");
        assert!(sql.contains("FROM"), "from:\n{sql}");
        assert!(sql.contains("\"id\""), "id col:\n{sql}");
        assert!(sql.contains("\"state_id\""), "state col:\n{sql}");
        assert!(sql.contains("WHERE"), "where:\n{sql}");
        assert!(sql.contains("issues"), "table:\n{sql}");
        assert!(sql.contains("123e4567"), "bind:\n{sql}");
        // The L5 const scopes no soft-delete and no join.
        assert!(!sql.contains("deleted_at"), "no deleted_at guard:\n{sql}");
        assert!(!sql.contains("JOIN"), "no join:\n{sql}");
    }

    #[test]
    fn lock_ticker_select_pins_guard_order_limit_and_lock() {
        let id = Uuid::parse_str("123e4567-e89b-12d3-a456-426614174000").unwrap();
        let sql = lock_ticker_select(id)
            .build(DbBackend::Postgres)
            .to_string();
        for fragment in [
            "issue_agent_ticker",
            "issue_id",
            "deleted_at",
            "IS NULL",
            "ORDER BY",
            "ASC",
            "LIMIT 1",
            "FOR UPDATE",
        ] {
            assert!(sql.contains(fragment), "missing {fragment} in:\n{sql}");
        }
    }

    #[test]
    fn clock_policy_select_reads_five_columns_by_pk() {
        let id = Uuid::parse_str("123e4567-e89b-12d3-a456-426614174000").unwrap();
        let sql = sql_of(clock_policy_select(id));
        for fragment in [
            "agent_ticking_enabled",
            "agent_default_max_ticks",
            "agent_default_interval_seconds",
            "agent_review_default_interval_seconds",
            "agent_test_default_interval_seconds",
            "FROM",
            "projects",
            "WHERE",
            "123e4567",
        ] {
            assert!(sql.contains(fragment), "missing {fragment} in:\n{sql}");
        }
        assert!(!sql.contains("JOIN"), "no join:\n{sql}");
    }

    #[test]
    fn clock_policy_row_widens_intervals() {
        let row = ClockPolicyRow {
            agent_ticking_enabled: Some(true),
            agent_default_max_ticks: Some(-1),
            agent_default_interval_seconds: Some(10800),
            agent_review_default_interval_seconds: Some(3600),
            agent_test_default_interval_seconds: Some(7200),
        };
        let policy: ProjectClockPolicy = row.into();
        assert_eq!(policy.agent_ticking_enabled, Some(true));
        assert_eq!(policy.agent_default_max_ticks, Some(-1));
        assert_eq!(policy.agent_default_interval_seconds, Some(10800));
        assert_eq!(policy.agent_review_default_interval_seconds, Some(3600));
        assert_eq!(policy.agent_test_default_interval_seconds, Some(7200));
    }

    #[test]
    fn orm_error_unwraps_sqlx_text() {
        let inner = sqlx::Error::RowNotFound;
        let err = sea_orm::DbErr::Query(sea_orm::RuntimeErr::SqlxError(inner));
        assert_eq!(orm_error_text(err), sqlx::Error::RowNotFound.to_string());
    }
}
