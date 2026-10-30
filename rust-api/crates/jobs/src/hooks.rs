#![forbid(unsafe_code)]

//! Public domain-write hooks: OSS's own side effects for any caller (OVL-16, PIDASHCONV-1660).
//!
//! The domain writes (`pidash_services::domain::*`) and the serializer-step
//! operations (`pidash_services::operations::*`) take their side effects as
//! caller-supplied hooks ([`IssueTransitionHook`], [`TaskEnqueueHook`],
//! [`MovePostCommitHook`]). OSS's own implementations lived private to the
//! api crate (`AppTransitionHook` in `api/src/issue_transition.rs`,
//! `AppTaskEnqueueHook` / `AppMovePostCommitHook` in
//! `api/src/domain_hooks.rs`), so the cloud MCP overlay wrote its own
//! (`McpTransitionHook`, `McpEnqueueHook`, `McpMoveDrainHook` plus a
//! re-implemented transition fire, ticker reconcile, clock write, clock
//! policy, completion comment and dispatch/creation deps). That is Pi Dash
//! business logic outside OSS; in Python, MCP gets all of it from OSS
//! signals and services through `serializer.save()`.
//!
//! This module is the moved home: the same bindings over explicit
//! dependencies, constructible by any caller that holds a pool, the
//! settings, the installed seams, the drains and a queue sink. The api
//! crate's `App*` types and the worker's `Worker*` types are thin
//! same-signature wrappers over these, so every existing call site keeps
//! working and the OSS binary behaves exactly as before.
//!
//! [`HookDeps`] is the one bundle: build it once from the binary's pieces,
//! then [`HookDeps::transition_hook`], [`HookDeps::enqueue_hook`],
//! [`HookDeps::move_hook`] and [`HookDeps::operation_deps`] hand out the
//! hook implementations and the [`operations::Deps`][pidash_services::operations::Deps]
//! that installs the first two. A caller of `operations::*` gets exactly
//! the side effects the v1 endpoint gets with no hook code of its own:
//!
//! ```rust,ignore
//! let deps = pidash_sdk::hooks::HookDeps {
//!     pool, cloud, managed, cache, redis, runner,
//!     toolsets, assistant_seam,
//!     publish: std::sync::Arc::new(pidash_sdk::hooks::QueuePublishDrain::new(&pool)),
//!     pods: std::sync::Arc::new(pidash_sdk::hooks::LivePodDrain::new(&pool, redis.clone(), runner.clone())),
//!     terminal: std::sync::Arc::new(pidash_sdk::hooks::QueueTerminalEffectsDrain::new(&pool)),
//!     queue: std::sync::Arc::new(pidash_sdk::hooks::OutboxTaskSink),
//! };
//! let op_deps = deps.operation_deps(orm);
//! let outcome = pidash_sdk::operations::issues::create(&op_deps, &ctx, &write, options).await?;
//! ```
//!
//! Behaviours that genuinely differ per caller stay explicit parameters,
//! never caller-written hooks:
//!
//! * The task transport is the [`TaskSink`] seam. OSS binaries pass
//!   [`OutboxTaskSink`] (the `rust_job_queue` row, exactly what the v1
//!   endpoint enqueues). The overlay passes its own two-lane sink (eager
//!   Celery-broker publish where a broker is configured, else the outbox)
//!   so its broker-list contract assertions keep passing.
//! * The terminal-effects and pod drains are the `publish` / `pods` /
//!   `terminal` fields. The api binary passes its inline drains (the
//!   request applies terminal effects synchronously with the ambient
//!   user); worker and overlay contexts pass [`QueuePublishDrain`] +
//!   [`LivePodDrain`] + [`QueueTerminalEffectsDrain`] (the effects task
//!   covers the hooks idempotently, PIDASHCONV-859).
//! * MCP-shaped activity legs are already options on the operations
//!   (`CreateOptions::emit_activity` / `emit_model_activity`,
//!   PIDASHCONV-1652), and the MCP move shape (dual guards, the
//!   `updated_by` stamp, no handoff) is already
//!   `MoveOptions` (PIDASHCONV-1657) — no new option was needed here.
//!
//! [`IssueTransitionHook`]: pidash_services::domain::transition::IssueTransitionHook
//! [`TaskEnqueueHook`]: pidash_services::domain::activity::TaskEnqueueHook
//! [`MovePostCommitHook`]: pidash_services::domain::issues_move::MovePostCommitHook

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use pidash_db::config::{CloudAgentSettings, ManagedRunnerSettings, RunnerSettings};
use pidash_services::domain::activity::TaskEnqueueHook;
use pidash_services::domain::issues_move::{
    CreationDeps, MoveAdmissionCache, MoveCreationOutboxes, MoveDrainError, MovePostCommitHook,
};
use pidash_services::domain::transition::IssueTransitionHook;
use pidash_services::extensions::{AssistantModelSeam, CloudAgentToolsetsSeam};
use pidash_services::orchestration::entries::{FireOutcome, FireRequest};
use serde_json::{Map, Value};
use sqlx::PgPool;
use uuid::Uuid;

use crate::admission::RedisAdmissionCache;
use crate::dispatch::{dispatch_agent_run, DispatchDecision};
use crate::issue_transition::{fire_transition, PodDrain, PublishEffectsDrain};
use crate::orchestration::live_inputs::{
    creator_universe, fetch_presence, gate_fn, gate_verdicts, profile_fn, JobsMatcher,
};
use crate::orchestration::StoreDeps;
use crate::queue::{self, NewJob};
use crate::runner_runs::sweeps_chat::terminal_effects_job;
use crate::TaskMessage;

// ---------------------------------------------------------------------------
// Task sink: the enqueue transport as an object-safe seam
// ---------------------------------------------------------------------------

/// The task transport as an object-safe seam (the
/// [`IssueTransitionHook`] precedent: boxed futures, no generics).
/// OSS binaries use [`OutboxTaskSink`]; the overlay passes its own
/// (eager broker publish where configured, else the outbox) so its
/// broker-list assertions keep passing. Infallible like the
/// handler-local `enqueue_task` twins: failures warn inside the
/// implementation and the write stands.
pub trait TaskSink: Send + Sync {
    fn send(
        &self,
        pool: &PgPool,
        message: TaskMessage,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + '_>>;
}

/// `task.delay(*args, **kwargs)` through the jobs kernel: the
/// `rust_job_queue` row, exactly what the v1 endpoint enqueues.
/// Enqueue failures warn and the write stands (Django's broker publish
/// is equally fire-and-forget from the view's perspective).
pub struct OutboxTaskSink;

impl TaskSink for OutboxTaskSink {
    fn send(
        &self,
        pool: &PgPool,
        message: TaskMessage,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        let pool = pool.clone();
        Box::pin(async move {
            let job = NewJob::new(
                message.task.clone(),
                Value::Array(message.args.clone()),
                Value::Object(message.kwargs.clone()),
            );
            if let Err(error) = queue::enqueue(&pool, &job).await {
                tracing::warn!(%error, task = message.task.as_str(), "task enqueue failed; write stands");
            }
        })
    }
}

// ---------------------------------------------------------------------------
// Queue/live drains over explicit inputs (the worker shapes, public)
// ---------------------------------------------------------------------------

/// The [`PublishEffectsDrain`] over the queue: each finalized run's
/// publish leg enqueues its terminal-effects task and the inline leg is
/// skipped (no live `RunnerRunsSeams` here — the queued task covers the
/// hooks idempotently, PIDASHCONV-859). Moved verbatim from the worker's
/// `WorkerTransitionDrains` publish arm.
pub struct QueuePublishDrain {
    pool: PgPool,
}

impl QueuePublishDrain {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }
}

impl PublishEffectsDrain for QueuePublishDrain {
    fn drain(
        &self,
        effects: Vec<pidash_services::runner_runs::LifecycleEffect>,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        use pidash_services::runner_runs::LifecycleEffect;
        let pool = self.pool.clone();
        Box::pin(async move {
            for effect in effects {
                match effect {
                    LifecycleEffect::PublishTerminalEffects { run_id } => {
                        let job = terminal_effects_job(&run_id);
                        if let Err(error) = crate::queue::enqueue(&pool, &job).await {
                            tracing::error!(%error, "failed to publish terminal effects for run {run_id}");
                        }
                    }
                    LifecycleEffect::ApplyTerminalEffectsInline { run_id } => {
                        tracing::warn!(
                            "skipping inline terminal effects for run {run_id}: no live RunnerRunsSeams \
                             (PIDASHCONV-859); the queued effects task covers the hooks idempotently"
                        );
                    }
                    // Unreachable: `plan_publish_effects` emits exactly these two.
                    _ => {}
                }
            }
        })
    }
}

/// The [`PodDrain`] over the public live executor: the D-14 recipe
/// (`matcher.py:242-251`) with no binary state. Moved verbatim from the
/// worker's `WorkerTransitionDrains` pod arm.
pub struct LivePodDrain {
    pool: PgPool,
    redis: Option<redis::Client>,
    runner: RunnerSettings,
}

impl LivePodDrain {
    pub fn new(pool: &PgPool, redis: Option<redis::Client>, runner: RunnerSettings) -> Self {
        Self {
            pool: pool.clone(),
            redis,
            runner,
        }
    }
}

impl PodDrain for LivePodDrain {
    fn drain_pod(&self, pod_id: Uuid) -> Pin<Box<dyn Future<Output = Result<(), ()>> + Send + '_>> {
        let pool = self.pool.clone();
        let redis = self.redis.clone();
        let runner = self.runner.clone();
        Box::pin(async move {
            pidash_services::runner_sessions::live_drains::drain_pod_by_id(
                &pool,
                redis.as_ref(),
                &runner,
                pod_id,
            )
            .await
            .map(|_| ())
            .map_err(|_| ())
        })
    }
}

/// Drain finalized runs' terminal publish pairs by run id, with the
/// per-call ambient user for the inline leg. The move's post-commit
/// creation drain calls this (the api `drain_creation_outboxes`
/// terminal leg); unlike [`PublishEffectsDrain`] the actor arrives per
/// call, because the move chain learns it from its own span. Infallible
/// like the source legs: enqueue failures error-log, inline failures
/// error-log, the move stands either way.
pub trait TerminalEffectsDrain: Send + Sync {
    fn drain(
        &self,
        run_ids: &[Uuid],
        created_by: Option<Uuid>,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + '_>>;
}

/// The [`TerminalEffectsDrain`] over the queue: one terminal-effects
/// task per finalized run. The api binary passes its inline drain
/// instead (synchronous apply with the ambient user).
pub struct QueueTerminalEffectsDrain {
    pool: PgPool,
}

impl QueueTerminalEffectsDrain {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }
}

impl TerminalEffectsDrain for QueueTerminalEffectsDrain {
    fn drain(
        &self,
        run_ids: &[Uuid],
        _created_by: Option<Uuid>,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        let pool = self.pool.clone();
        let run_ids = run_ids.to_vec();
        Box::pin(async move {
            for run_id in run_ids {
                let job = terminal_effects_job(&run_id);
                if let Err(error) = crate::queue::enqueue(&pool, &job).await {
                    tracing::error!(%error, "failed to publish terminal effects for run {run_id}");
                }
            }
        })
    }
}

// ---------------------------------------------------------------------------
// The bundle + the three hooks
// ---------------------------------------------------------------------------

/// The explicit dependencies behind the three hooks: the pool, the
/// executor settings, the admission cache, the pubsub inputs, the
/// installed seams, and the three drains plus the queue sink. Build it
/// once from the binary's pieces; the hook constructors and the
/// [`operations::Deps`][pidash_services::operations::Deps] wiring below
/// clone what they need (pools, clients and seams are `Arc` handles).
#[derive(Clone)]
pub struct HookDeps {
    pub pool: PgPool,
    pub cloud: CloudAgentSettings,
    pub managed: ManagedRunnerSettings,
    pub cache: RedisAdmissionCache,
    pub redis: Option<redis::Client>,
    pub runner: RunnerSettings,
    pub toolsets: Arc<dyn CloudAgentToolsetsSeam>,
    pub assistant_seam: Arc<dyn AssistantModelSeam>,
    /// The transition fire's terminal-effects drain (per-fire actor is
    /// bound by the implementation: the api drain binds the fire's actor
    /// at construction, the queue drain ignores it).
    pub publish: Arc<dyn PublishEffectsDrain>,
    /// The pod drain for transition dispatches and move legs.
    pub pods: Arc<dyn PodDrain>,
    /// The move creation drain's terminal leg (per-call actor).
    pub terminal: Arc<dyn TerminalEffectsDrain>,
    /// The task transport for activity legs and completion comments.
    pub queue: Arc<dyn TaskSink>,
}

impl HookDeps {
    /// The post-save orchestration hook over these deps.
    pub fn transition_hook(&self) -> TransitionHook {
        TransitionHook::new(self)
    }

    /// The best-effort task-enqueue hook over these deps.
    pub fn enqueue_hook(&self) -> EnqueueHook {
        EnqueueHook::new(&self.pool, Arc::clone(&self.queue))
    }

    /// The move post-commit hook over these deps.
    pub fn move_hook(&self) -> MoveHook {
        MoveHook::new(
            &self.pool,
            &self.cloud,
            self.cache.clone(),
            self.redis.clone(),
            &self.runner,
            Arc::clone(&self.terminal),
            Arc::clone(&self.pods),
        )
    }

    /// The admission cache over these deps, for the move's creation facts.
    pub fn admission_cache(&self) -> LiveMoveAdmissionCache {
        LiveMoveAdmissionCache::new(self.cache.clone())
    }

    /// The shared operation dependencies with the OSS hooks installed:
    /// `transition` + `enqueue` over these deps, through
    /// [`Deps::live`][pidash_services::operations::Deps::live]. A caller
    /// of `operations::*` gets exactly the side effects the v1 endpoint
    /// gets with no hook code of its own.
    pub fn operation_deps(
        &self,
        orm: sea_orm::DatabaseConnection,
    ) -> pidash_services::operations::Deps {
        pidash_services::operations::Deps::live(
            self.pool.clone(),
            orm,
            Arc::new(self.transition_hook()),
            Arc::new(self.enqueue_hook()),
        )
    }

    /// The move's handoff-creation runtime facts over these deps, for
    /// the `creation` parameter of
    /// [`move_to_project`][pidash_services::operations::issues::move_to_project].
    pub fn creation_deps(&self, now_unix_secs: i64) -> CreationDeps {
        CreationDeps {
            pool: self.pool.clone(),
            cloud: self.cloud.clone(),
            managed: self.managed.clone(),
            now_unix_secs,
            toolsets: Arc::clone(&self.toolsets),
            assistant_seam: Arc::clone(&self.assistant_seam),
            admission: Arc::new(self.admission_cache()),
        }
    }
}

/// The post-save orchestration as an [`IssueTransitionHook`]: the
/// completion comment plus the transition fire, over explicit deps.
/// Moved from the api `AppTransitionHook` / worker
/// `WorkerTransitionHook` pair, which were the same binding over
/// different dep sources and drains: the universe, presence and
/// verdict prefetch is the shared shape (the api id-based and worker
/// builders read the same rows), built once per fire, and the drains
/// arrive on [`HookDeps`].
#[derive(Clone)]
pub struct TransitionHook {
    deps: HookDeps,
}

impl TransitionHook {
    pub fn new(deps: &HookDeps) -> Self {
        Self { deps: deps.clone() }
    }
}

impl IssueTransitionHook for TransitionHook {
    fn enqueue_completion_comment(
        &self,
        issue_id: Uuid,
        prev_state_id: Option<Uuid>,
        current_state_id: Option<Uuid>,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        let deps = self.deps.clone();
        Box::pin(async move {
            let Some(job) = crate::issue_transition::completion_comment_job(
                &deps.pool,
                &issue_id,
                prev_state_id.as_ref(),
                current_state_id.as_ref(),
            )
            .await
            else {
                return;
            };
            let task = job.task.clone();
            let (args, kwargs) = job.into_message_parts();
            let message = TaskMessage::new(task, args, kwargs);
            deps.queue.send(&deps.pool, message).await;
        })
    }

    fn fire(
        &self,
        actor_id: Uuid,
        request: FireRequest,
    ) -> Pin<Box<dyn Future<Output = Result<FireOutcome, ()>> + Send + '_>> {
        let deps = self.deps.clone();
        Box::pin(async move {
            // No state change: no I/O, like `fire_state_transition`.
            if request.prev_state_id == request.current_state_id {
                return Ok(FireOutcome::NoTransition);
            }
            // Per-fire inputs (the worker fire shape, which subsumes the
            // api `dispatch_deps_for_ids` pair: both read the same
            // universe, presence and verdicts, so one prefetch serves the
            // creation drive and the preflight). Each erasure logs its
            // cause first (PIDASHCONV-1563).
            let universe = creator_universe(&deps.pool, Some(actor_id), request.issue_id)
                .await
                .map_err(|error| {
                    tracing::error!(
                        %error,
                        site = "hook-creator-universe",
                        issue_id = %request.issue_id,
                        "issue transition fire failure"
                    );
                })?;
            let presence = Arc::new(fetch_presence(&deps.pool, &universe).await.map_err(
                |error| {
                    tracing::error!(
                        %error,
                        site = "hook-presence",
                        issue_id = %request.issue_id,
                        "issue transition fire failure"
                    );
                },
            )?);
            let verdicts = Arc::new(gate_verdicts(&deps.assistant_seam, &presence).await);
            let build_deps = || StoreDeps {
                cloud: deps.cloud.clone(),
                managed: deps.managed.clone(),
                cache: deps.cache.clone(),
                has_usable_llm_config: gate_fn(Arc::clone(&verdicts)),
                toolsets: Arc::clone(&deps.toolsets),
                llm_profile: profile_fn(Arc::clone(&presence)),
                now_unix_secs: request.now.timestamp(),
            };
            let matcher = JobsMatcher::new(deps.pool.clone());
            fire_transition(
                &deps.pool,
                build_deps(),
                build_deps(),
                matcher,
                &deps.cloud,
                request,
                &*deps.publish,
                &*deps.pods,
            )
            .await
        })
    }
}

/// `task.delay(*args, **kwargs)` through the configured [`TaskSink`].
/// Moved from the api `AppTaskEnqueueHook` / worker
/// `WorkerTaskEnqueueHook` pair, which were the same outbox enqueue.
#[derive(Clone)]
pub struct EnqueueHook {
    pool: PgPool,
    queue: Arc<dyn TaskSink>,
}

impl EnqueueHook {
    pub fn new(pool: &PgPool, queue: Arc<dyn TaskSink>) -> Self {
        Self {
            pool: pool.clone(),
            queue,
        }
    }
}

impl TaskEnqueueHook for EnqueueHook {
    fn enqueue_task(
        &self,
        task: &str,
        args: Vec<Value>,
        kwargs: Map<String, Value>,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        let pool = self.pool.clone();
        let queue = Arc::clone(&self.queue);
        let message = TaskMessage::new(task, args, kwargs);
        Box::pin(async move {
            queue.send(&pool, message).await;
        })
    }
}

/// The move's post-commit drains over explicit deps. Moved from the api
/// `AppMovePostCommitHook`: the same three legs in the same order —
/// the handoff-creation drain (terminal pairs, one `dispatch_agent_run`
/// per id with its pod drain, deferred admission consumes), the source
/// pod drain, and the cancel send.
#[derive(Clone)]
pub struct MoveHook {
    pool: PgPool,
    cloud: CloudAgentSettings,
    cache: RedisAdmissionCache,
    redis: Option<redis::Client>,
    runner: RunnerSettings,
    terminal: Arc<dyn TerminalEffectsDrain>,
    pods: Arc<dyn PodDrain>,
}

impl MoveHook {
    pub fn new(
        pool: &PgPool,
        cloud: &CloudAgentSettings,
        cache: RedisAdmissionCache,
        redis: Option<redis::Client>,
        runner: &RunnerSettings,
        terminal: Arc<dyn TerminalEffectsDrain>,
        pods: Arc<dyn PodDrain>,
    ) -> Self {
        Self {
            pool: pool.clone(),
            cloud: cloud.clone(),
            cache,
            redis,
            runner: runner.clone(),
            terminal,
            pods,
        }
    }
}

/// `timezone.now()` truncated to microseconds (the api
/// `project_move_handoff` `now_micros` position: Django datetimes are
/// microsecond-exact; Postgres would round stored nanos).
fn now_micros() -> DateTime<Utc> {
    let now = Utc::now();
    DateTime::from_timestamp_micros(now.timestamp_micros()).expect("micros in range")
}

impl MovePostCommitHook for MoveHook {
    fn drain_creation(
        &self,
        outboxes: MoveCreationOutboxes,
        terminal_created_by: Option<Uuid>,
    ) -> Pin<Box<dyn Future<Output = Result<(), MoveDrainError>> + Send + '_>> {
        let this = self.clone();
        Box::pin(async move {
            // The `complete_project_move_handoff` order: terminal publish
            // pairs (isolated each, infallible), dispatches
            // (`dispatch_agent_run` per id, failures propagate), deferred
            // admission consumes (best-effort).
            if !outboxes.terminal_effects.is_empty() {
                this.terminal
                    .drain(&outboxes.terminal_effects, terminal_created_by)
                    .await;
            }
            let now = now_micros();
            for id in outboxes.dispatches {
                match dispatch_agent_run(&this.pool, &this.cloud, id, now).await {
                    Ok(DispatchDecision::PodDrain { pod_id }) => {
                        this.pods.drain_pod(pod_id).await.map_err(|()| {
                            MoveDrainError("move creation pod drain failed".to_owned())
                        })?;
                    }
                    Ok(_) => {}
                    Err(error) => {
                        return Err(MoveDrainError(format!(
                            "move creation dispatch failed: {error}"
                        )));
                    }
                }
            }
            for consume in &outboxes.deferred {
                pidash_services::dispatch::admission::consume_admission_token(&this.cache, consume);
            }
            Ok(())
        })
    }

    fn drain_pod(
        &self,
        pod_id: Uuid,
    ) -> Pin<Box<dyn Future<Output = Result<(), MoveDrainError>> + Send + '_>> {
        let this = self.clone();
        Box::pin(async move {
            this.pods
                .drain_pod(pod_id)
                .await
                .map_err(|()| MoveDrainError("move pod drain failed".to_owned()))
        })
    }

    fn send_cancel(
        &self,
        runner_id: Uuid,
        run_id: Uuid,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        let this = self.clone();
        Box::pin(async move {
            let store = pidash_services::runner_sessions::live_drains::PoolPubsubStore::new(
                this.pool.clone(),
                this.redis.clone(),
                this.runner.clone(),
            );
            let outcome =
                pidash_services::app_issues::send_project_move_cancel(&store, runner_id, run_id)
                    .await;
            for warning in outcome.warnings {
                tracing::warn!("{warning}");
            }
        })
    }
}

/// The admission cache over an explicit [`RedisAdmissionCache`]: the two
/// reads the execution resolve needs, with string errors. Moved
/// verbatim from the api `AppMoveAdmissionCache`.
#[derive(Clone)]
pub struct LiveMoveAdmissionCache {
    cache: RedisAdmissionCache,
}

impl LiveMoveAdmissionCache {
    pub fn new(cache: RedisAdmissionCache) -> Self {
        Self { cache }
    }
}

impl MoveAdmissionCache for LiveMoveAdmissionCache {
    fn bucket_count(&self, key: &str) -> Result<Option<i64>, String> {
        use pidash_services::dispatch::AdmissionCache;
        self.cache
            .bucket_count(key)
            .map_err(|error| error.to_string())
    }

    fn add_or_incr(&self, key: &str, timeout_secs: i64) -> Result<(), String> {
        use pidash_services::dispatch::AdmissionCache;
        self.cache
            .add_or_incr(key, timeout_secs)
            .map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pidash_services::extensions::{NoExtraToolsets, SeamFuture, TitleError, TitleRequest};

    #[derive(Debug)]
    struct StubAssistantSeam;

    impl AssistantModelSeam for StubAssistantSeam {
        fn generate_title(
            &self,
            _request: TitleRequest,
        ) -> SeamFuture<'_, Result<String, TitleError>> {
            Box::pin(async move { Err(TitleError::ServerError) })
        }
    }

    fn test_deps() -> HookDeps {
        let pool =
            PgPool::connect_lazy("postgres://localhost/pidash_hooks_test").expect("lazy pool");
        let settings = pidash_db::config::Settings::test_defaults();
        HookDeps {
            pool: pool.clone(),
            cloud: settings.cloud_agent.clone(),
            managed: settings.managed_runner.clone(),
            cache: RedisAdmissionCache::new(None),
            redis: None,
            runner: settings.runner.clone(),
            toolsets: Arc::new(NoExtraToolsets),
            assistant_seam: Arc::new(StubAssistantSeam),
            publish: Arc::new(QueuePublishDrain::new(&pool)),
            pods: Arc::new(LivePodDrain::new(&pool, None, settings.runner.clone())),
            terminal: Arc::new(QueueTerminalEffectsDrain::new(&pool)),
            queue: Arc::new(OutboxTaskSink),
        }
    }

    /// The bundle wires the three hooks plus the operation deps: every
    /// hook registers on the domain registry under its trait, and
    /// `operation_deps` installs the transition + enqueue pair. Offline:
    /// the lazy pool and the SeaORM connector open no connections (the
    /// Tokio context is only for the lazy pool constructor).
    #[tokio::test]
    async fn hook_deps_wires_hooks_and_operation_deps() {
        let deps = test_deps();
        let mut registry = pidash_services::domain::HookRegistry::new();
        registry.register(Arc::new(deps.transition_hook()) as Arc<dyn IssueTransitionHook>);
        registry.register(Arc::new(deps.enqueue_hook()) as Arc<dyn TaskEnqueueHook>);
        registry.register(Arc::new(deps.move_hook()) as Arc<dyn MovePostCommitHook>);
        assert!(registry.get::<dyn IssueTransitionHook>().is_some());
        assert!(registry.get::<dyn TaskEnqueueHook>().is_some());
        assert!(registry.get::<dyn MovePostCommitHook>().is_some());
        let orm = sea_orm::SqlxPostgresConnector::from_sqlx_postgres_pool(deps.pool.clone());
        let op_deps = deps.operation_deps(orm);
        let mut op_registry = pidash_services::domain::HookRegistry::new();
        op_registry.register(op_deps.transition);
        op_registry.register(op_deps.enqueue);
        assert!(op_registry.get::<dyn IssueTransitionHook>().is_some());
        assert!(op_registry.get::<dyn TaskEnqueueHook>().is_some());
    }
}
