//! The OSS job worker as a composable builder (PIDASHCONV-1541, OVL-03).
//!
//! [`WorkerBuilder::from_env`] owns the `worker` startup sequence the OSS
//! `main.rs` used to spell out: pools, the SeaORM handle, the queue
//! schema, the Redis client, the [`Registry`], the Mongo sink,
//! [`register_oss_handlers`], the turn handler (with the configured
//! `turn_soft_limit`), the worker and scheduler loops over
//! [`default_schedule`](pidash_jobs::default_schedule), and the
//! timeout-bounded shutdown drain — with the same fail-fast behaviour.
//! The OSS binary calls `from_env` plus [`WorkerBuilder::run`]; an
//! extension binary composes the same path with its own seams
//! ([`WorkerBuilder::with_assistant_seam`],
//! [`WorkerBuilder::with_turn_seam`]), extra jobs
//! ([`WorkerBuilder::register`]) and extra schedule entries
//! ([`WorkerBuilder::schedule`]).
//!
//! The builder is generic over the turn seam because [`TurnSeam`] is not
//! object-safe (associated stream type, `impl Future` methods); the
//! default is the live OSS seam. An extra registration over an
//! already-owned task name — OSS or an earlier extra — is a build error,
//! never a silent replace.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use pidash_db::config::Settings;
use pidash_jobs::assistant::{register_turn_handler, TurnSeam};
use pidash_jobs::{
    default_schedule, register_oss_handlers, BeatEntry, Handler, OssWorkerCtx, Registry,
    WorkerConfig,
};
use pidash_services::extensions::AssistantModelSeam;

use super::{queue_schema_pool, seed_data_dir, settings_from_env, shutdown_signal};
use crate::assistant::model_seam::CeAssistantSeam;
use crate::turn::live::LiveTurnSeam;
use crate::turn::tools_local_store::RedisClient;

/// One deferred extra registration, run in call order after the OSS and
/// turn registrations. Receives the registry plus the built worker
/// context (pools, settings, ORM handle) the handlers close over.
type ExtraReg = Box<dyn FnOnce(&mut Registry, &OssWorkerCtx) -> Result<(), String> + Send>;

/// The composed worker without the loops: the registry, the installed
/// turn seam, the context registration ran against, and the schedule.
/// [`WorkerBuilder::build`] returns this for tests and embedding;
/// [`WorkerBuilder::run`] builds the same value and drives the loops.
pub struct BuiltWorker<S = LiveTurnSeam> {
    registry: Registry,
    turn_seam: Arc<S>,
    ctx: OssWorkerCtx,
    schedule: Vec<BeatEntry>,
    concurrency: u32,
    turn_soft_limit: i64,
}

impl<S> std::fmt::Debug for BuiltWorker<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BuiltWorker")
            .field("tasks", &self.registry.task_names().len())
            .field("schedule", &self.schedule.len())
            .field("concurrency", &self.concurrency)
            .finish_non_exhaustive()
    }
}

impl<S> BuiltWorker<S> {
    /// The handler table: every OSS-owned name plus every extra job.
    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    /// The installed turn seam.
    pub fn turn_seam(&self) -> &Arc<S> {
        &self.turn_seam
    }

    /// The context registration ran against (pools, settings, ORM
    /// handle, Mongo sink, seed dir, assistant seam).
    pub fn context(&self) -> &OssWorkerCtx {
        &self.ctx
    }

    /// The beat schedule: [`default_schedule`](pidash_jobs::default_schedule)
    /// plus every [`WorkerBuilder::schedule`] entry, in order.
    pub fn schedule(&self) -> &[BeatEntry] {
        &self.schedule
    }

    /// The configured `assistant.run_turn` soft limit in seconds: the
    /// final `settings.assistant.turn_soft_limit` (after
    /// [`with_settings`](WorkerBuilder::with_settings)) the turn
    /// handler registered with — Celery's `soft_time_limit`
    /// (PIDASHCONV-1458). (PIDASHCONV-1558.)
    pub fn turn_soft_limit(&self) -> i64 {
        self.turn_soft_limit
    }
}

/// Owns the `worker` startup sequence; extensions plug in through the
/// `with_*` seams, [`WorkerBuilder::register`] and [`WorkerBuilder::schedule`].
///
/// The OSS binary runs with `from_env` plus [`WorkerBuilder::run`] (and
/// the CLI concurrency); every other method is an extension point the
/// cloud overlay uses instead of re-doing the sequence by hand.
pub struct WorkerBuilder<S = LiveTurnSeam> {
    settings: Settings,
    pools: pidash_db::Pools,
    mongo: Option<pidash_db::tasks_cleanup::cleanup_queries::MongoSink>,
    seed_dir: PathBuf,
    assistant_seam: Arc<dyn AssistantModelSeam>,
    assistant_seam_is_default: bool,
    turn_seam: Arc<S>,
    turn_seam_is_default: bool,
    turn_soft_limit: i64,
    concurrency: u32,
    extras: Vec<ExtraReg>,
    extra_schedule: Vec<BeatEntry>,
    queue_schema_owner: Option<sqlx::PgPool>,
}

impl<S> std::fmt::Debug for WorkerBuilder<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkerBuilder")
            .field("seed_dir", &self.seed_dir)
            .field("turn_soft_limit", &self.turn_soft_limit)
            .field("concurrency", &self.concurrency)
            .field("extras", &self.extras.len())
            .field("extra_schedule", &self.extra_schedule.len())
            .finish_non_exhaustive()
    }
}

impl WorkerBuilder<LiveTurnSeam> {
    /// Resolve settings, connect pools, and load the async inputs (the
    /// Mongo sink; the turn seam over the multiplexed Redis client) —
    /// everything `worker` did before registering.
    pub async fn from_env() -> Result<Self, Box<dyn std::error::Error>> {
        let db = pidash_db::DbConfig::from_env()?;
        let pools = pidash_db::Pools::connect(&db, None).await?;
        tracing::info!(target = %db.redacted_url(), "connected postgres");
        let settings = settings_from_env()?;
        // No broker: the job queue is Postgres and every task name has a
        // registered handler; an unknown name parks as failed
        // (`unknown task <name>`), never forwarded to Python (PIDASHCONV-1108,
        // the standalone rule).
        let mongo = pidash_db::tasks_cleanup::cleanup_queries::MongoSink::from_env().await;
        if mongo.is_none() {
            tracing::info!("MongoDB not configured; cleanup tasks will delete from Postgres only");
        }
        // The CE assistant seam: the same default `serve` installs on the
        // app state, so worker-side runs-creation gates answer exactly as
        // the serve-side gates do (PIDASHCONV-1054). The cloud binary
        // installs its overlay implementation here instead.
        let assistant_seam = Arc::new(CeAssistantSeam::new(Arc::new(settings.clone())));
        // The live assistant turn seam (PIDASHCONV-863): the multiplexed Redis
        // client for the cancel key and the SSE publishes, the worker
        // pool, and settings. Construction does no I/O.
        let turn_seam = Arc::new(
            LiveTurnSeam::new(
                pools.primary().clone(),
                Arc::new(settings.clone()),
                turn_redis_client(&settings),
            )
            .map_err(|error| format!("assistant turn seam: {error}"))?,
        );
        // The configured soft limit for the turn runner's stream deadline
        // (PIDASHCONV-1458), like Celery's `soft_time_limit`.
        let turn_soft_limit = settings.assistant.turn_soft_limit;
        Ok(Self {
            settings,
            pools,
            mongo,
            seed_dir: seed_data_dir(),
            assistant_seam,
            assistant_seam_is_default: true,
            turn_seam,
            turn_seam_is_default: true,
            turn_soft_limit,
            concurrency: 4,
            extras: Vec::new(),
            extra_schedule: Vec::new(),
            queue_schema_owner: None,
        })
    }

    /// Deterministic builder for tests and embedding over caller-supplied
    /// pools (lazy pools keep it hermetic: registration constructs
    /// handlers without I/O, and none of them runs here). No Mongo sink;
    /// the seed dir is the temp dir; a never-dialed Redis URL satisfies
    /// the mail seams' boot-time URL parse (PIDASHCONV-865, the 918-test
    /// precedent — the client only parses, it never dials).
    pub fn for_tests(pools: pidash_db::Pools, mut settings: Settings) -> Result<Self, String> {
        settings.redis.url = Some("redis://localhost:1/".to_owned());
        let turn_seam = Arc::new(
            LiveTurnSeam::new(pools.primary().clone(), Arc::new(settings.clone()), None)
                .map_err(|error| format!("assistant turn seam: {error}"))?,
        );
        let turn_soft_limit = settings.assistant.turn_soft_limit;
        Ok(Self {
            settings: settings.clone(),
            pools,
            mongo: None,
            seed_dir: std::env::temp_dir(),
            assistant_seam: Arc::new(CeAssistantSeam::new(Arc::new(settings))),
            assistant_seam_is_default: true,
            turn_seam,
            turn_seam_is_default: true,
            turn_soft_limit,
            concurrency: 4,
            extras: Vec::new(),
            extra_schedule: Vec::new(),
            queue_schema_owner: None,
        })
    }

    /// Tweak the resolved settings before the worker is built, mirroring
    /// [`ServeBuilder::with_settings`](super::serve::ServeBuilder::with_settings).
    /// The final settings feed `ctx.settings` plus every settings-derived
    /// capture: the turn soft limit is re-captured, and the assistant and
    /// turn seams are rebuilt from the mutated settings while still the
    /// defaults — both seams consult settings at runtime (the turn seam
    /// reads `assistant.block_private_urls`, one of the overlay's own
    /// mutations), so leaving the `from_env` captures in place would
    /// orphan the tweak. (PIDASHCONV-1558.)
    ///
    /// Returns `Result` (unlike serve's infallible setter) because the
    /// default turn-seam rebuild can fail. An explicitly installed seam
    /// is never rebuilt here, whichever order the calls run in. Call
    /// before [`with_turn_seam`](WorkerBuilder::with_turn_seam): a
    /// prebuilt seam is the caller's own construction from the final
    /// settings, so this setter only exists on the default-seam builder.
    pub fn with_settings(mut self, f: impl FnOnce(&mut Settings)) -> Result<Self, String> {
        f(&mut self.settings);
        self.turn_soft_limit = self.settings.assistant.turn_soft_limit;
        if self.assistant_seam_is_default {
            self.assistant_seam = Arc::new(CeAssistantSeam::new(Arc::new(self.settings.clone())));
        }
        if self.turn_seam_is_default {
            self.turn_seam = Arc::new(
                LiveTurnSeam::new(
                    self.pools.primary().clone(),
                    Arc::new(self.settings.clone()),
                    turn_redis_client(&self.settings),
                )
                .map_err(|error| format!("assistant turn seam: {error}"))?,
            );
        }
        Ok(self)
    }
}

impl<S: TurnSeam + 'static> WorkerBuilder<S> {
    /// Replace the assistant model-provider seam the worker-side
    /// runs-creation gates consult (tick, scheduler, handoff executor).
    /// CE default: [`CeAssistantSeam`], the same default `serve`
    /// installs, so both sides gate identically (PIDASHCONV-1054).
    /// The installed seam is never rebuilt by
    /// [`with_settings`](WorkerBuilder::with_settings), whichever order
    /// the calls run in.
    pub fn with_assistant_seam(mut self, seam: Arc<dyn AssistantModelSeam>) -> Self {
        self.assistant_seam = seam;
        self.assistant_seam_is_default = false;
        self
    }

    /// Replace the turn seam driving `assistant.run_turn` with a
    /// prebuilt one. The default is the live OSS seam, built from the
    /// pools, settings and Redis client in
    /// [`WorkerBuilder::from_env`]. Build the prebuilt seam from the
    /// final settings (call
    /// [`with_settings`](WorkerBuilder::with_settings) first): it is
    /// installed as-is and never rebuilt.
    pub fn with_turn_seam<T: TurnSeam + 'static>(self, seam: Arc<T>) -> WorkerBuilder<T> {
        let Self {
            settings,
            pools,
            mongo,
            seed_dir,
            assistant_seam,
            assistant_seam_is_default,
            turn_seam: _,
            turn_seam_is_default: _,
            turn_soft_limit,
            concurrency,
            extras,
            extra_schedule,
            queue_schema_owner,
        } = self;
        WorkerBuilder {
            settings,
            pools,
            mongo,
            seed_dir,
            assistant_seam,
            assistant_seam_is_default,
            turn_seam: seam,
            turn_seam_is_default: false,
            turn_soft_limit,
            concurrency,
            extras,
            extra_schedule,
            queue_schema_owner,
        }
    }

    /// Run `concurrency` jobs side by side (the `worker --concurrency`
    /// flag; default 4, clamped to at least 1 like the CLI path).
    pub fn with_concurrency(mut self, concurrency: u32) -> Self {
        self.concurrency = concurrency;
        self
    }

    /// Run the queue-schema DDL in [`WorkerBuilder::run`] on `pool`
    /// instead of the app pool. Split-role deploys pass the schema
    /// owner's pool (the migrator role; the app role is DML-only and
    /// Postgres checks `CREATE` on the schema even for
    /// `CREATE TABLE IF NOT EXISTS`). Unset, `run` ensures the schema
    /// on primary, exactly as before. (PIDASHCONV-1558.)
    pub fn with_queue_schema_owner(mut self, pool: sqlx::PgPool) -> Self {
        self.queue_schema_owner = Some(pool);
        self
    }

    /// Register extra jobs after the OSS and turn registrations. The
    /// closure receives the registry plus the built worker context;
    /// registering over an already-owned name fails the build instead
    /// of silently replacing the handler.
    pub fn register(
        mut self,
        register: impl FnOnce(&mut Registry, &OssWorkerCtx) -> Result<(), String> + Send + 'static,
    ) -> Self {
        self.extras.push(Box::new(register));
        self
    }

    /// Append extra beat entries after
    /// [`default_schedule`](pidash_jobs::default_schedule).
    pub fn schedule(mut self, entries: Vec<BeatEntry>) -> Self {
        self.extra_schedule.extend(entries);
        self
    }

    /// Register the OSS handlers, the turn handler, and every extra, and
    /// return the composed worker without driving the loops. Pure
    /// construction over the (possibly lazy) pools — no I/O.
    pub fn build(self) -> Result<BuiltWorker<S>, String> {
        let Self {
            settings,
            pools,
            mongo,
            seed_dir,
            assistant_seam,
            assistant_seam_is_default: _,
            turn_seam,
            turn_seam_is_default: _,
            turn_soft_limit,
            concurrency,
            extras,
            extra_schedule,
            queue_schema_owner: _,
        } = self;
        // Task handlers register through the shared OSS composition
        // (`pidash_jobs::register_oss_handlers`, PIDASHCONV-918).
        let ctx = OssWorkerCtx {
            orm: pidash_db::sea_orm_connection(pools.primary()),
            pools,
            settings,
            mongo,
            seed_dir,
            assistant_seam,
        };
        let mut registry = Registry::new();
        register_oss_handlers(&mut registry, &ctx)?;
        // The turn handler (PIDASHCONV-863): the builder owns
        // `assistant.run_turn` (the sweep above is registered by
        // `register_oss_handlers`). Registering makes the name locally
        // executed.
        register_turn_handler(
            &mut registry,
            ctx.pools.primary().clone(),
            turn_seam.clone(),
            turn_soft_limit,
        );
        // Each extra runs against the registry the OSS and turn
        // registrations plus the earlier extras built; a replaced
        // handler is a build error naming the task, never a silent
        // swap (the private-pi-dash #304 class).
        for extra in extras {
            let before = snapshot_handlers(&registry);
            extra(&mut registry, &ctx)?;
            let replaced: Vec<String> = before
                .iter()
                .filter(|(name, handler)| {
                    matches!(registry.get(name), Some(current) if !Arc::ptr_eq(handler, current))
                })
                .map(|(name, _)| name.clone())
                .collect();
            if !replaced.is_empty() {
                return Err(format!(
                    "WorkerBuilder: extra registration replaced already-owned task(s): {}",
                    replaced.join(", ")
                ));
            }
        }
        let mut schedule = default_schedule();
        schedule.extend(extra_schedule);
        Ok(BuiltWorker {
            registry,
            turn_seam,
            ctx,
            schedule,
            concurrency,
            turn_soft_limit,
        })
    }

    /// Ensure the queue schema, build the registry, and drive the worker
    /// plus scheduler loops until SIGINT/SIGTERM, then drain both loops
    /// inside the 30s grace period — the `worker` sequence.
    pub async fn run(self) -> Result<(), Box<dyn std::error::Error>> {
        pidash_jobs::queue::ensure_schema(queue_schema_pool(
            self.queue_schema_owner.as_ref(),
            self.pools.primary(),
        ))
        .await?;
        let built = self.build()?;
        let worker_config = WorkerConfig {
            concurrency: built.concurrency.max(1) as usize,
            ..Default::default()
        };
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        tracing::info!(
            concurrency = built.concurrency,
            "worker + scheduler loops started"
        );
        let worker_fut = pidash_jobs::worker::run_worker(
            built.ctx.pools.primary().clone(),
            built.registry,
            worker_config,
            shutdown_rx.clone(),
        );
        let scheduler_fut = pidash_jobs::scheduler::run_scheduler(
            built.ctx.pools.primary().clone(),
            built.schedule,
            shutdown_rx,
        );
        tokio::pin!(worker_fut);
        tokio::pin!(scheduler_fut);
        tokio::select! {
            _ = &mut worker_fut => tracing::warn!("worker loop exited unexpectedly"),
            _ = &mut scheduler_fut => tracing::warn!("scheduler loop exited unexpectedly"),
            _ = shutdown_signal() => {
                tracing::info!("worker shutting down");
                let _ = shutdown_tx.send(true);
                // Grace period for in-flight jobs to settle and the
                // scheduler to release its lock. Both loops must finish: a
                // select! here would resolve when the first loop stops (the
                // scheduler stops at once) and abort in-flight jobs with the
                // runtime (PIDASHCONV-1326).
                let drained = tokio::time::timeout(
                    Duration::from_secs(30),
                    async {
                        tokio::join!(&mut worker_fut, &mut scheduler_fut);
                    },
                )
                .await;
                if drained.is_err() {
                    tracing::warn!("shutdown grace period expired");
                }
            }
        }
        Ok(())
    }
}

/// The turn seam's multiplexed Redis client for the cancel key and the
/// SSE publishes (`None` when `REDIS_URL` is unset, exactly like
/// Django's swallowed `redis_instance()`). Construction parses only,
/// never dials; an unparsable URL warns and disables the cancel key.
/// Shared by [`WorkerBuilder::from_env`] and
/// [`WorkerBuilder::with_settings`](WorkerBuilder::with_settings) so a
/// settings tweak rebuilds the client from the final URL.
fn turn_redis_client(settings: &Settings) -> Option<RedisClient> {
    match settings.redis.url.as_deref().filter(|url| !url.is_empty()) {
        Some(url) => match pidash_db::redis::open_client(url) {
            Ok(client) => Some(Arc::new(client)),
            Err(error) => {
                tracing::warn!(%error, "assistant: REDIS_URL unparsable; cancel key disabled");
                None
            }
        },
        None => None,
    }
}

/// The registry's name-to-handler snapshot: handler identity is the
/// `Arc` pointer, so a re-registration over the same name compares
/// unequal while an untouched name compares equal.
fn snapshot_handlers(registry: &Registry) -> Vec<(String, Handler)> {
    registry
        .task_names()
        .into_iter()
        .filter_map(|name| registry.get(&name).map(|handler| (name, handler.clone())))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pidash_jobs::assistant::run_turn::{
        AgentError, ModelError, SeamError, SkippedServer, StreamItem, ToolsetHandle, TurnContext,
        TurnUsage,
    };
    use pidash_jobs::Verdict;

    /// Turn-seam double: never driven (the tests assert identity, not
    /// behaviour), so every method is unreachable.
    struct FakeTurnSeam;

    impl TurnSeam for FakeTurnSeam {
        type Stream = ();

        async fn load_context(
            &self,
            _turn_id: uuid::Uuid,
        ) -> Result<Option<TurnContext>, SeamError> {
            unreachable!("fake turn seam is never driven")
        }

        async fn mark_running(&self, _ctx: &TurnContext) -> Result<bool, SeamError> {
            unreachable!("fake turn seam is never driven")
        }

        async fn is_cancelled(&self, _turn_id: uuid::Uuid) -> Result<bool, SeamError> {
            unreachable!("fake turn seam is never driven")
        }

        async fn resolve_model(&self, _ctx: &TurnContext) -> Result<String, ModelError> {
            unreachable!("fake turn seam is never driven")
        }

        async fn resolve_toolsets(
            &self,
            _ctx: &TurnContext,
        ) -> Result<(Vec<ToolsetHandle>, Vec<SkippedServer>), SeamError> {
            unreachable!("fake turn seam is never driven")
        }

        async fn emit_skipped(
            &self,
            _ctx: &TurnContext,
            _servers: &[SkippedServer],
        ) -> Result<(), SeamError> {
            unreachable!("fake turn seam is never driven")
        }

        async fn runtime_failures(
            &self,
            _toolsets: &[ToolsetHandle],
        ) -> Result<Vec<SkippedServer>, SeamError> {
            unreachable!("fake turn seam is never driven")
        }

        async fn load_history(&self, _ctx: &TurnContext) -> Result<serde_json::Value, SeamError> {
            unreachable!("fake turn seam is never driven")
        }

        async fn model_label(&self, _ctx: &TurnContext) -> Result<String, SeamError> {
            unreachable!("fake turn seam is never driven")
        }

        async fn open_stream(
            &self,
            _ctx: &TurnContext,
            _history: &serde_json::Value,
            _toolsets: &[ToolsetHandle],
        ) -> Result<Self::Stream, AgentError> {
            unreachable!("fake turn seam is never driven")
        }

        async fn next_event(
            &self,
            _stream: &mut Self::Stream,
        ) -> Result<Option<StreamItem>, AgentError> {
            unreachable!("fake turn seam is never driven")
        }

        fn now_ms(&self) -> u64 {
            0
        }

        async fn start_row(&self, _ctx: &TurnContext) -> Result<uuid::Uuid, SeamError> {
            unreachable!("fake turn seam is never driven")
        }

        async fn emit_delta(
            &self,
            _ctx: &TurnContext,
            _message_id: uuid::Uuid,
            _chunk: &str,
        ) -> Result<(), SeamError> {
            unreachable!("fake turn seam is never driven")
        }

        async fn finalize_row(
            &self,
            _ctx: &TurnContext,
            _message_id: uuid::Uuid,
            _text: &str,
            _status: &'static str,
        ) -> Result<(), SeamError> {
            unreachable!("fake turn seam is never driven")
        }

        async fn dump_messages(
            &self,
            _stream: &Self::Stream,
        ) -> Result<serde_json::Value, SeamError> {
            unreachable!("fake turn seam is never driven")
        }

        async fn extract_usage(
            &self,
            _stream: &Self::Stream,
        ) -> Result<Option<TurnUsage>, SeamError> {
            unreachable!("fake turn seam is never driven")
        }

        async fn was_cut_off(&self, _stream: &Self::Stream) -> Result<bool, SeamError> {
            unreachable!("fake turn seam is never driven")
        }

        async fn mark_cut_off(&self, _ctx: &TurnContext) -> Result<(), SeamError> {
            unreachable!("fake turn seam is never driven")
        }

        async fn complete_turn(
            &self,
            _ctx: &TurnContext,
            _model_messages: serde_json::Value,
            _usage: serde_json::Value,
            _model_used: String,
            _truncated: bool,
        ) -> Result<(), SeamError> {
            unreachable!("fake turn seam is never driven")
        }

        async fn fail_turn(
            &self,
            _ctx: &TurnContext,
            _code: &str,
            _detail: &str,
        ) -> Result<(), SeamError> {
            unreachable!("fake turn seam is never driven")
        }

        async fn cancel_turn(&self, _ctx: &TurnContext) -> Result<(), SeamError> {
            unreachable!("fake turn seam is never driven")
        }
    }

    /// Assistant-seam double with a marker title.
    #[derive(Debug)]
    struct MarkerAssistantSeam;

    impl AssistantModelSeam for MarkerAssistantSeam {
        fn generate_title(
            &self,
            request: pidash_services::extensions::TitleRequest,
        ) -> pidash_services::extensions::SeamFuture<
            '_,
            Result<String, pidash_services::extensions::TitleError>,
        > {
            let _ = request;
            Box::pin(async move { Ok("marker-title".to_owned()) })
        }
    }

    /// `connect_lazy` never touches the network (it needs a Tokio
    /// context to build the pool, hence `tokio::test`): the builder
    /// stays hermetic over lazy pools.
    fn lazy_pools() -> pidash_db::Pools {
        let pool =
            sqlx::PgPool::connect_lazy("postgres://localhost:1/unused").expect("lazy pool builds");
        pidash_db::Pools::primary_only(pool)
    }

    fn stub_handler() -> Handler {
        Arc::new(|_| Box::pin(async { Ok(Verdict::Ack) }))
    }

    /// The builder registers OSS handlers across groups plus one extra
    /// job, over lazy pools with no I/O.
    #[tokio::test]
    async fn builder_registers_oss_and_extra_handlers() {
        let built = WorkerBuilder::for_tests(lazy_pools(), Settings::test_defaults())
            .expect("test builder builds")
            .register(|registry, ctx| {
                // The context carries the pools the handlers close over.
                let _ = ctx.pools.primary();
                registry.register("acme.extra", stub_handler());
                Ok(())
            })
            .build()
            .expect("build registers");
        for name in [
            pidash_jobs::assistant::RUN_TURN_TASK,
            pidash_jobs::assistant::SWEEP_TASK,
            pidash_jobs::tasks_webhooks::TRACK_EVENT_TASK,
            pidash_jobs::tasks_ticker::scan::FIRE_TICK_TASK,
            "acme.extra",
        ] {
            assert!(built.registry().owns(name), "{name} must be registered");
        }
        assert!(
            built.registry().task_names().len() > 50,
            "the OSS set must register wholesale"
        );
    }

    /// Re-registering an OSS task name fails the build, naming the task.
    #[tokio::test]
    async fn reregistering_an_oss_task_name_fails() {
        let err = WorkerBuilder::for_tests(lazy_pools(), Settings::test_defaults())
            .expect("test builder builds")
            .register(|registry, _ctx| {
                registry.register(pidash_jobs::assistant::RUN_TURN_TASK, stub_handler());
                Ok(())
            })
            .build()
            .expect_err("replacing the OSS turn handler must fail");
        assert!(
            err.contains(pidash_jobs::assistant::RUN_TURN_TASK),
            "unexpected error: {err}"
        );
    }

    /// The same guard covers extra-over-extra: the second registration
    /// over one name fails too.
    #[tokio::test]
    async fn extra_over_extra_fails() {
        let err = WorkerBuilder::for_tests(lazy_pools(), Settings::test_defaults())
            .expect("test builder builds")
            .register(|registry, _ctx| {
                registry.register("acme.extra", stub_handler());
                Ok(())
            })
            .register(|registry, _ctx| {
                registry.register("acme.extra", stub_handler());
                Ok(())
            })
            .build()
            .expect_err("double extra registration must fail");
        assert!(err.contains("acme.extra"), "unexpected error: {err}");
    }

    /// An extra closure's own error aborts the build.
    #[tokio::test]
    async fn extra_registration_error_aborts_build() {
        let err = WorkerBuilder::for_tests(lazy_pools(), Settings::test_defaults())
            .expect("test builder builds")
            .register(|_registry, _ctx| Err("boom".to_owned()))
            .build()
            .expect_err("extra error must abort");
        assert_eq!(err, "boom");
    }

    /// Swapping either seam is observable: the built worker carries the
    /// installed instances, not the defaults.
    #[tokio::test]
    async fn seam_swaps_are_observable_on_built_worker() {
        let assistant = Arc::new(MarkerAssistantSeam);
        let turn = Arc::new(FakeTurnSeam);
        let built = WorkerBuilder::for_tests(lazy_pools(), Settings::test_defaults())
            .expect("test builder builds")
            .with_assistant_seam(assistant.clone())
            .with_turn_seam(turn.clone())
            .build()
            .expect("build registers");
        assert!(Arc::ptr_eq(
            &built.context().assistant_seam,
            &(assistant as Arc<dyn AssistantModelSeam>)
        ));
        assert!(Arc::ptr_eq(built.turn_seam(), &turn));
    }

    /// `schedule` appends after the default schedule without disturbing it.
    #[tokio::test]
    async fn schedule_appends_after_defaults() {
        let defaults = default_schedule();
        let extra = BeatEntry {
            name: "acme-extra",
            task: "acme.extra",
            cadence: pidash_jobs::Cadence::IntervalSecs(60),
        };
        let built = WorkerBuilder::for_tests(lazy_pools(), Settings::test_defaults())
            .expect("test builder builds")
            .schedule(vec![extra.clone()])
            .build()
            .expect("build registers");
        assert_eq!(built.schedule().len(), defaults.len() + 1);
        assert_eq!(built.schedule()[..defaults.len()], defaults[..]);
        assert!(built.schedule().contains(&extra));
    }

    /// `with_settings` feeds the final settings into `ctx.settings` and
    /// re-captures the turn soft limit: a marker value lands in both.
    #[tokio::test]
    async fn with_settings_feeds_ctx_and_turn_limit() {
        let built = WorkerBuilder::for_tests(lazy_pools(), Settings::test_defaults())
            .expect("test builder builds")
            .with_settings(|settings| {
                settings.debug = true;
                settings.assistant.turn_soft_limit = 4242;
            })
            .expect("settings apply")
            .build()
            .expect("build registers");
        assert!(built.context().settings.debug);
        assert_eq!(built.context().settings.assistant.turn_soft_limit, 4242);
        assert_eq!(built.turn_soft_limit(), 4242);
    }

    /// Without `with_settings` the turn limit is the stock capture from
    /// the resolved settings — the OSS default path is untouched.
    #[tokio::test]
    async fn turn_limit_defaults_to_stock_capture() {
        let built = WorkerBuilder::for_tests(lazy_pools(), Settings::test_defaults())
            .expect("test builder builds")
            .build()
            .expect("build registers");
        assert_eq!(
            built.turn_soft_limit(),
            Settings::test_defaults().assistant.turn_soft_limit
        );
    }

    /// `with_settings` never rebuilds an explicitly installed assistant
    /// seam: the custom instance survives, while the settings tweak
    /// still lands in `ctx.settings`.
    #[tokio::test]
    async fn with_settings_preserves_explicit_assistant_seam() {
        let assistant = Arc::new(MarkerAssistantSeam);
        let built = WorkerBuilder::for_tests(lazy_pools(), Settings::test_defaults())
            .expect("test builder builds")
            .with_assistant_seam(assistant.clone())
            .with_settings(|settings| {
                settings.debug = true;
            })
            .expect("settings apply")
            .build()
            .expect("build registers");
        assert!(Arc::ptr_eq(
            &built.context().assistant_seam,
            &(assistant as Arc<dyn AssistantModelSeam>)
        ));
        assert!(built.context().settings.debug);
    }

    /// A prebuilt turn seam installed before `with_settings` survives
    /// it: only the still-default seam is rebuilt.
    #[tokio::test]
    async fn with_settings_preserves_prebuilt_turn_seam() {
        let pools = lazy_pools();
        let custom = Arc::new(
            LiveTurnSeam::new(
                pools.primary().clone(),
                Arc::new(Settings::test_defaults()),
                None,
            )
            .expect("prebuilt seam"),
        );
        let built = WorkerBuilder::for_tests(pools, Settings::test_defaults())
            .expect("test builder builds")
            .with_turn_seam(custom.clone())
            .with_settings(|settings| {
                settings.debug = true;
            })
            .expect("settings apply")
            .build()
            .expect("build registers");
        assert!(Arc::ptr_eq(built.turn_seam(), &custom));
        assert!(built.context().settings.debug);
    }
}
