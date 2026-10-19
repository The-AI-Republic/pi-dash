//! The OSS worker composition as library APIs (PIDASHCONV-918).
//!
//! [`register_oss_handlers`] performs exactly the handler-registration
//! sequence `worker()` ran when this module was created: same task
//! names, same handlers, same forwards, same comments about what stays
//! Python-owned. The OSS binary's `worker()` builds an [`OssWorkerCtx`]
//! and calls it; the private cloud binary composes this helper plus its
//! own `register_cloud_jobs`, so an unextended cloud worker behaves
//! exactly like the OSS worker.
//!
//! The seam providers live here next to the tasks they serve (moved
//! from the binary, where `pidash-api-bin`'s lack of a lib target made
//! them unreachable from another crate):
//!
//! * [`runner_seams::RunnerSeams`] — live providers for the 11 runner
//!   tasks (PIDASHCONV-859), plus the chat-event publish it shares with
//!   the API chat handler.
//! * [`execute_seams::LiveExecuteSeams`] — live execute seams over
//!   [`model_runtime`] (PIDASHCONV-832 + PIDASHCONV-866).
//! * [`redis_origin::OriginRedis`] — live sync origin writer for
//!   `issue_activity` (PIDASHCONV-836).
//! * [`model_runtime`] — the live model runtime the execute seams
//!   invoke on.
//! * [`kms_transport::HttpKmsTransport`] — the production KMS transport
//!   (moved from `pidash-api`, which re-exports it under its old path):
//!   the key resolver's second caller after the assistant handlers.
//!
//! The helper is synchronous: everything it touches is a constructor
//! or a registration. The two inputs that need I/O or the environment
//! — the Mongo sink and the boot-loaded fire-tick states — are loaded
//! by the caller (as `worker()` always did) and carried on the
//! context, so registration itself stays a pure function of the
//! context. The one failure is the mail seams' fail-fast: a
//! missing/unparsable `REDIS_URL` or a drifted template aborts boot
//! (PIDASHCONV-865), exactly as `worker()` did. Mirror rule: any
//! registration change here must update the binary's
//! `mirror_registry_matches_tasks_table` test and the TASKS table in
//! step — the inventory suite, not this module, pins the
//! worker-owned set.

pub mod execute_seams;
pub mod kms_transport;
pub mod model_runtime;
pub mod redis_origin;
pub mod runner_seams;

use std::path::PathBuf;

use pidash_db::config::Settings;
use pidash_db::tasks_cleanup::cleanup_queries::MongoSink;
use pidash_db::Pools;
use pidash_services::extensions::{AssistantModelSeam, NoExtraToolsets};

use crate::worker::Registry;

/// The OSS worker's registration inputs: everything
/// [`register_oss_handlers`] needs beyond the registry.
///
/// The caller loads the async pieces (`mongo` via
/// `MongoSink::from_env`) exactly as `worker()` did before
/// PIDASHCONV-918; the helper only registers.
pub struct OssWorkerCtx {
    /// The worker's pools (handlers close over clones).
    pub pools: Pools,
    /// The resolved settings (dispatch + runner + execute slices).
    pub settings: Settings,
    /// The Mongo sink for cleanup tasks, if MongoDB is configured.
    pub mongo: Option<MongoSink>,
    /// Seed-data dir for `workspace_seed` (`SEED_DIR` env or `seeds/`).
    pub seed_dir: PathBuf,
    /// The assistant model-provider seam the worker-side
    /// runs-creation gates consult (tick, scheduler, handoff
    /// executor). The OSS binary installs the CE default; the cloud
    /// binary installs its overlay implementation.
    pub assistant_seam: std::sync::Arc<dyn AssistantModelSeam>,
}

/// Register every OSS-owned task handler on `registry`.
///
/// This is the `worker()` registration sequence, moved verbatim: the
/// D-07…D-10 ports' register calls plus the runner, execute and
/// activity groups, with the same live providers and the same
/// never-register comments for what stays Python-owned. Any name left
/// unregistered here forwards to Python (`worker::forward`).
///
/// The cloud binary calls this first, then its own
/// `register_cloud_jobs` — with no cloud domain registered the two
/// workers own identical registries:
///
/// ```rust
/// use pidash_db::config::Settings;
/// use pidash_jobs::oss_worker::{register_oss_handlers, OssWorkerCtx};
/// use pidash_jobs::worker::Registry;
///
/// // The trait defaults with a canned title: the CE behaviour lives
/// // in the defaults.
/// #[derive(Debug)]
/// struct DefaultSeam;
///
/// impl pidash_services::extensions::AssistantModelSeam for DefaultSeam {
///     fn generate_title(
///         &self,
///         request: pidash_services::extensions::TitleRequest,
///     ) -> pidash_services::extensions::SeamFuture<
///         '_,
///         Result<String, pidash_services::extensions::TitleError>,
///     > {
///         let _ = request;
///         Box::pin(async move { Ok("canned".to_owned()) })
///     }
/// }
///
/// # #[tokio::main]
/// # async fn main() {
/// // `connect_lazy` never touches the network (it needs a Tokio
/// // context to build the pool, hence the async main): registration
/// // wiring stays provable with no database.
/// let pool = sqlx::PgPool::connect_lazy("postgres://localhost:1/unused").unwrap();
/// let mut registry = Registry::new();
/// // The mail seams fail boot without a Redis URL (PIDASHCONV-865);
/// // the client only parses here — it never dials.
/// let mut settings = Settings::test_defaults();
/// settings.redis.url = Some("redis://localhost:1/".to_owned());
/// let ctx = OssWorkerCtx {
///     pools: pidash_db::Pools::primary_only(pool),
///     settings,
///     mongo: None,
///     seed_dir: std::env::temp_dir(),
///     assistant_seam: std::sync::Arc::new(DefaultSeam),
/// };
/// register_oss_handlers(&mut registry, &ctx).expect("mail seams build");
/// assert!(registry.owns(pidash_jobs::assistant::SWEEP_TASK));
/// # }
/// ```
pub fn register_oss_handlers(registry: &mut Registry, ctx: &OssWorkerCtx) -> Result<(), String> {
    // Task handlers register here (the D-07…D-10 ports register theirs);
    // any still-unregistered name forwards to Python.
    crate::tasks_cleanup::register_cleanup_handlers(registry, ctx.pools.clone(), ctx.mongo.clone());
    crate::tasks_cleanup::register_versions(registry, ctx.pools.primary().clone());
    // D-09 remaining groups (PIDASHCONV-224; kept in sync with
    // `tests::worker_registry_owns_all_d09_local_tasks`). The dead
    // `export_analytics_to_csv_email` stays Python-owned: never register it
    // here. `delete_old_s3_link` flipped to Rust in PIDASHCONV-858 (below):
    // the live S3 deleter fails the task on any S3 error, so an S3 outage
    // parks instead of ack-and-clearing.
    crate::tasks_cleanup::register_deletion_tasks(registry, ctx.pools.clone());
    // Export tasks (PIDASHCONV-1106; kept in sync with
    // `inventory::tests::mirror_registry_matches_tasks_table`). The
    // pool-only handlers exist with live mail/S3 seams, so the Rust
    // worker owns `issue_export_task` and `analytic_export_task` —
    // otherwise the exports are lost at switchover (parity gap
    // W-export). Django drops both (PDASHOSS01-292); Rust runs them.
    crate::tasks_export::register_issue_export_task(registry, ctx.pools.primary().clone());
    crate::tasks_export::register_analytic_task(registry, ctx.pools.primary().clone());
    // `register_assets` installs sweep + metadata + copy (assets.rs:692).
    crate::tasks_cleanup::assets::register_assets(
        registry,
        ctx.pools.primary().clone(),
        std::sync::Arc::new(crate::tasks_cleanup::assets::UnavailableObjectStore),
        std::sync::Arc::new(crate::tasks_cleanup::assets::NoopLiveConvert),
    );
    crate::tasks_cleanup::register_expiry(
        registry,
        ctx.pools.primary().clone(),
        std::sync::Arc::new(crate::tasks_cleanup::expiry::LiveS3Deleter::from_settings(
            &ctx.settings,
        )),
    );
    crate::tasks_cleanup::register_workspace_seed(
        registry,
        ctx.pools.primary().clone(),
        ctx.seed_dir.clone(),
    );
    crate::tasks_cleanup::dummy_data::register(registry, ctx.pools.primary().clone());
    // D-05 integrations groups (PIDASHCONV-238; kept in sync with
    // `tests::worker_registry_owns_all_d05_local_tasks`). All six task
    // names are worker-owned with live providers.
    crate::integrations::git_sync::register_git_sync_tasks(
        registry,
        ctx.pools.primary().clone(),
        crate::integrations::git_sync::LiveProviders::from_env(),
    );
    crate::integrations::github_sync::register_github_sync_tasks(
        registry,
        ctx.pools.primary().clone(),
        crate::integrations::github_sync::LiveTransports::from_env(),
    );
    // D-08 sink tasks (PIDASHCONV-260, FX-LOG-01 for `process_logs`;
    // PIDASHCONV-1107 for `track_event`; kept in sync with
    // `tests::worker_registry_owns_sink_tasks`). EXECUTE parity per
    // PIDASHCONV-242: canonical Django (settings.local, the CI
    // worker-plane env) executes `process_logs` at worker boot, so the
    // Rust worker owns it too — the existing `process_logs_handler`
    // (mongo-configured → mongo sink, else the Postgres fallback row;
    // `logger_task.py:97-100`). `track_event` is owned as well: with no
    // PostHog configured the handler early-returns exactly like Python
    // (`event_tracking_task.py:65-67`), and with PostHog configured it
    // POSTs the same `/batch/` capture — never a silent swallow.
    // Standalone-Rust rule (PIDASHCONV-1, 2026-10-16): the Rust worker
    // runs it; Django drops it in production (PDASHOSS01-292).
    crate::tasks_webhooks::register_sink_tasks(
        registry,
        ctx.pools.clone(),
        ctx.mongo.clone(),
        reqwest::Client::new(),
    );
    // D-08 visit/page + link-crawl tasks (PIDASHCONV-1107; kept in
    // sync with `inventory::tests::mirror_registry_matches_tasks_table`).
    // The `recent_visited_task` + `page_transaction` handlers
    // (PIDASHCONV-201) and the `crawl_work_item_link_title` handler
    // (PIDASHCONV-200) exist with fixtures and unit tests; this wires
    // them so the Rust worker owns all three names. Standalone-Rust
    // rule (PIDASHCONV-1, 2026-10-16): the Rust worker runs them;
    // Django drops them in production (PDASHOSS01-292).
    crate::tasks_webhooks::register_visit_page_handlers(registry, ctx.pools.clone());
    crate::tasks_webhooks::register_link_crawl_handler(registry, ctx.pools.clone());
    // License instance_traces (PIDASHCONV-839; kept in sync with
    // `mirror_registry_matches_tasks_table` in inventory.rs). The
    // beat entry fires it every 6 hours and the D-01 handler exists, so
    // the Rust worker owns it — otherwise the task is lost at
    // switchover (parity gap W-license).
    crate::license::tasks::register(registry, ctx.pools.primary().clone());
    // D-06 stale-turn sweep (PIDASHCONV-831; kept in sync with
    // `inventory::tests::mirror_registry_matches_tasks_table`). The live
    // store fails RUNNING turns older than 390s, like the beat in Django.
    // `assistant.run_turn` stays Python-owned: `register_turn_handler`
    // needs a live `TurnSeam` and none exists (only test fakes) — never
    // register it here until the seam lands.
    crate::assistant::register_sweep_handler(registry, ctx.pools.primary().clone());
    // Loop scan + fire (PIDASHCONV-838; kept in sync with
    // `inventory::tests::mirror_registry_matches_tasks_table`). The
    // beat entry fires the scanner every minute and the scanner fans
    // out one fire job per eligible due target, so the Rust worker
    // owns both — otherwise the loop is lost at switchover (parity
    // gap W-loop).
    crate::loop_scan::register_scanner(registry, ctx.pools.primary().clone());
    crate::loop_fire::register_fire(registry, ctx.pools.primary().clone());
    // D-08 archive-and-close (PIDASHCONV-837; kept in sync with
    // `mirror_registry_matches_tasks_table` in inventory.rs). The
    // daily beat entry fires it and the D-08 handler exists, so the
    // Rust worker owns it — otherwise the task is lost at switchover
    // (parity gap W-automation).
    crate::tasks_webhooks::register_automation_handlers(registry, ctx.pools.clone());
    // D-10 ticker + scheduler scans (PIDASHCONV-833; kept in sync with
    // `inventory::tests::mirror_registry_matches_tasks_table`). One
    // shared call wires both beat scans; each fan-out enqueues into
    // the Rust queue.
    crate::tasks_ticker::scan::register_scanners(registry, ctx.pools.primary().clone());
    // D-10 ticker + scheduler fires (PIDASHCONV-861; kept in sync with
    // `inventory::tests::mirror_registry_matches_tasks_table`). Live
    // seams: the tick seam prefetches creator presence per fire and
    // runs the jobs continuation driver; the scheduler dispatch runs
    // the jobs scheduler driver over a live EntriesSeam. Both need
    // dispatch settings.
    crate::tasks_ticker::fire_tick::register_fire_tick(
        registry,
        ctx.pools.primary().clone(),
        std::sync::Arc::new(crate::orchestration::LiveFireTickSeam::new(
            ctx.pools.primary().clone(),
            ctx.settings.cloud_agent.clone(),
            ctx.settings.managed_runner.clone(),
            std::sync::Arc::new(NoExtraToolsets),
            ctx.assistant_seam.clone(),
        )),
    );
    crate::tasks_ticker::fire_binding::register_fire_binding(
        registry,
        ctx.pools.primary().clone(),
        crate::orchestration::live_scheduler_dispatch(
            ctx.pools.primary().clone(),
            ctx.settings.cloud_agent.clone(),
            ctx.settings.managed_runner.clone(),
            std::sync::Arc::new(NoExtraToolsets),
            ctx.assistant_seam.clone(),
        ),
    );
    // D-07 send/stack (PIDASHCONV-865; kept in sync with
    // `inventory::tests::mirror_registry_matches_tasks_table`). The live
    // mail seams exist (`tasks_mail::live`), so the Rust worker owns both
    // names — otherwise the tasks are lost at switchover (parity gap
    // W-mail). A missing/unparsable `REDIS_URL` or a drifted template
    // fails boot (fail-fast, like the missing database in `worker()`):
    // registering without the seams would ACK-and-drop mail.
    let mail_redis = crate::tasks_mail::live::LiveRedisMail::from_settings(&ctx.settings)
        .map_err(|detail| format!("worker mail redis unavailable: {detail}"))?;
    let mail_renderer = crate::tasks_mail::live::IssueUpdatesRenderer::new()
        .map_err(|error| format!("worker mail renderer unavailable: {error}"))?;
    crate::tasks_mail::register_email_notification_tasks(
        registry,
        crate::tasks_mail::email_notification::stack_handler(
            std::sync::Arc::new(crate::tasks_mail::email_notification::SqlStackStore::new(
                ctx.pools.primary().clone(),
            )),
            std::sync::Arc::new(crate::tasks_mail::live::QueueStackOutbox::new(
                ctx.pools.primary().clone(),
            )),
        ),
        crate::tasks_mail::email_notification::send_handler(
            std::sync::Arc::new(mail_redis),
            std::sync::Arc::new(crate::tasks_mail::live::SqlMailDb::new(
                ctx.pools.primary().clone(),
            )),
            std::sync::Arc::new(mail_renderer),
            std::sync::Arc::new(crate::tasks_mail::live::SmtpMailer::new()),
        ),
    );
    // D-07 notifications (PIDASHCONV-834; kept in sync with
    // `inventory::tests::mirror_registry_matches_tasks_table`). The
    // pool-only handler exists, so the Rust worker owns it —
    // otherwise the task is lost at switchover (parity gap W-mail).
    crate::tasks_mail::register_notifications_task(registry, ctx.pools.primary().clone());
    // D-07 membership mail (PIDASHCONV-1104; kept in sync with
    // `inventory::tests::mirror_registry_matches_tasks_table`). The four
    // T6 handlers exist and the project invitation port lands in the same
    // module, so the Rust worker owns all five — otherwise the tasks are
    // lost at switchover (parity gap W-mail).
    crate::tasks_mail::register_membership_mail_tasks(registry, ctx.pools.primary().clone());
    crate::tasks_mail::register_project_invitation_task(registry, ctx.pools.primary().clone());
    // D-07 auth mail (PIDASHCONV-1103; kept in sync with
    // `inventory::tests::mirror_registry_matches_tasks_table`). The
    // four T5 handlers exist over the live send pipeline
    // (`tasks_mail::mail_send`), so the Rust worker owns all four —
    // otherwise the jobs never run on a Rust-only deployment (no
    // RabbitMQ on the Redis Celery broker; standalone-Rust rule).
    crate::tasks_mail::register_auth_mail_tasks(registry, ctx.pools.primary().clone());
    // Runner tasks (PIDASHCONV-859; kept in sync with
    // `inventory::tests::mirror_registry_matches_tasks_table`). The
    // beat entries fire the sweeps and the L6 handlers exist with
    // live seam providers (`oss_worker::runner_seams::RunnerSeams`
    // over the PIDASHCONV-862 executors), so the Rust worker owns
    // all thirteen — otherwise the runs are lost at switchover
    // (parity gap W-runner, plus the four dormant names the
    // registers also own).
    let redis = match ctx
        .settings
        .redis
        .url
        .as_deref()
        .filter(|url| !url.is_empty())
        .map(redis::Client::open)
    {
        None => None,
        Some(Ok(client)) => Some(client),
        Some(Err(error)) => {
            tracing::warn!(%error, "redis: REDIS_URL unparsable; continuing without cache");
            None
        }
    };
    let runner_seams = std::sync::Arc::new(runner_seams::RunnerSeams::new(
        ctx.pools.primary().clone(),
        redis,
        &ctx.settings,
        std::sync::Arc::new(NoExtraToolsets),
        ctx.assistant_seam.clone(),
    ));
    crate::runner_runs::register_runner_runs_tasks(
        registry,
        ctx.pools.primary().clone(),
        runner_seams.clone(),
    );
    crate::runner_runs::register_sweeps_runs_tasks(
        registry,
        ctx.pools.primary().clone(),
        runner_seams.clone(),
        runner_seams.clone(),
    );
    crate::runner_runs::register_offline_recovery_tasks(
        registry,
        ctx.pools.primary().clone(),
        runner_seams,
    );
    // D-11 execution tasks (PIDASHCONV-832; kept in sync with
    // `inventory::tests::mirror_registry_matches_tasks_table`). All
    // four handlers execute here on live seams
    // (`oss_worker::execute_seams::LiveExecuteSeams`), including the run
    // task's model runtime (PIDASHCONV-866 — no forward override). The
    // register call also owns `managed_runner.expire_waiting_runs`
    // (never published in OSS; the table follows the registry).
    let invoke = model_runtime::invoke::InvokeCtx {
        tools: model_runtime::tools_exec::ToolCtx {
            pool: ctx.pools.primary().clone(),
            cloud: ctx.settings.cloud_agent.clone(),
            managed: ctx.settings.managed_runner.clone(),
            http: reqwest::Client::new(),
            github: model_runtime::github::GithubAppContext::new(
                ctx.pools.primary().clone(),
                pidash_db::redis::RedisHandle::from_settings(&ctx.settings),
                ctx.settings.secret_key.clone(),
            ),
            toolsets: std::sync::Arc::new(NoExtraToolsets),
        },
        keys: model_runtime::keys::KeyResolver::from_env(
            ctx.settings.assistant.key_cache_ttl_secs,
            ctx.settings.assistant.key_cache_maxsize,
        ),
    };
    crate::dispatch_execute::register_execute_tasks(
        registry,
        ctx.pools.primary().clone(),
        ctx.settings.cloud_agent.clone(),
        ctx.settings.managed_runner.clone(),
        std::sync::Arc::new(execute_seams::LiveExecuteSeams::new(
            ctx.pools.primary().clone(),
            ctx.settings.cloud_agent.block_private_urls,
            invoke,
        )),
        std::sync::Arc::new(NoExtraToolsets),
    );
    // D-08 issue_activity (PIDASHCONV-836; kept in sync with
    // `inventory::tests::mirror_registry_matches_tasks_table`, where the
    // `Pools`-taking register is a stub over the same name const).
    // Ownership flips to Rust the moment this registers; the live
    // `OriginRedis` (REDIS_URL, sync SETEX) preserves the still-Python
    // mail task's `ri.get(issue_id)` read. Without a REDIS_URL the
    // origin write errors per call and the task aborts, exactly like
    // Django's `redis_instance()` RuntimeError under the broad-except.
    crate::tasks_webhooks::register_activity_task(
        registry,
        ctx.pools.clone(),
        redis_origin::OriginRedis::from_env(),
    );
    // D-08 webhook send + fan-out (PIDASHCONV-1105; kept in sync with
    // `inventory::tests::mirror_registry_matches_tasks_table`). The
    // four `webhook_task` jobs run locally now: the send task POSTs
    // through the raw requests-faithful client and logs to
    // mongo-else-postgres, the fan-out renders `get_model_data` live
    // and enqueues sends into the Rust queue, and the deactivation
    // email goes through the shared mail send.
    crate::tasks_webhooks::webhook_send::register_webhook_send_task(
        registry,
        ctx.pools.clone(),
        ctx.mongo.clone(),
    );
    crate::tasks_webhooks::webhook_send::register_deactivation_email_task(
        registry,
        ctx.pools.clone(),
    );
    crate::tasks_webhooks::webhook_fanout::register_webhook_activity_task(
        registry,
        ctx.pools.clone(),
        crate::tasks_webhooks::model_data::web_base_url(
            ctx.settings.urls.web_url.as_deref(),
            ctx.settings.urls.app_base_url.as_deref(),
        ),
    );
    crate::tasks_webhooks::webhook_fanout::register_model_activity_task(
        registry,
        ctx.pools.clone(),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worker::{route_for, Route};

    /// The trait defaults with a canned title: the CE behaviour lives
    /// in the defaults (the api crate's `CeAssistantSeam` only adds
    /// the title transport).
    #[derive(Debug)]
    struct DefaultSeam;

    impl AssistantModelSeam for DefaultSeam {
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

    /// PIDASHCONV-918: the helper owns exactly the OSS task set — the
    /// same const union the binary's `mirror_registry_matches_tasks_table`
    /// pins (70 names), while the Python-owned sentinels stay
    /// unregistered. `Pools::primary_only` over a lazy pool keeps the
    /// test hermetic: registration constructs handlers without I/O, and
    /// none of them runs here.
    #[tokio::test]
    async fn helper_owns_exactly_the_oss_task_set() {
        // `connect_lazy` never touches the network (it needs a Tokio
        // context to build the pool, hence `tokio::test`).
        let pool =
            sqlx::PgPool::connect_lazy("postgres://localhost:1/unused").expect("lazy pool builds");
        // The mail seams fail boot without a Redis URL (PIDASHCONV-865);
        // the client only parses here — it never dials.
        let mut settings = Settings::test_defaults();
        settings.redis.url = Some("redis://localhost:1/".to_owned());
        let ctx = OssWorkerCtx {
            pools: Pools::primary_only(pool),
            settings,
            mongo: None,
            seed_dir: std::env::temp_dir(),
            assistant_seam: std::sync::Arc::new(DefaultSeam),
        };
        let mut registry = Registry::new();
        register_oss_handlers(&mut registry, &ctx).expect("mail seams build");

        // The const union, independent of the binary's mirror.
        let mut expected = std::collections::BTreeSet::new();
        expected.extend(
            crate::tasks_cleanup::cleanup::TASKS
                .iter()
                .map(|spec| spec.task),
        );
        expected.extend(crate::tasks_cleanup::versions::ALL_VERSION_TASKS);
        expected.extend([
            crate::tasks_cleanup::assets::TASK_DELETE_UNUPLOADED,
            crate::tasks_cleanup::assets::TASK_GET_METADATA,
            crate::tasks_cleanup::assets::TASK_COPY_S3_OBJECTS,
            crate::tasks_cleanup::SOFT_DELETE_TASK,
            crate::tasks_cleanup::HARD_DELETE_TASK,
            crate::tasks_cleanup::WORKSPACE_SEED_TASK_NAME,
            crate::tasks_cleanup::dummy_data::TASK_NAME,
            crate::tasks_cleanup::TASK_DELETE_OLD_S3_LINK,
            crate::tasks_webhooks::PROCESS_LOGS_TASK,
            crate::tasks_webhooks::TRACK_EVENT_TASK,
            crate::tasks_webhooks::RECENT_VISITED_TASK_NAME,
            crate::tasks_webhooks::PAGE_TRANSACTION_TASK_NAME,
            crate::tasks_webhooks::ISSUE_ACTIVITY_TASK,
            crate::tasks_webhooks::webhook_send::WEBHOOK_SEND_TASK_NAME,
            crate::tasks_webhooks::webhook_send::DEACTIVATION_EMAIL_TASK_NAME,
            crate::tasks_webhooks::webhook_fanout::WEBHOOK_ACTIVITY_TASK_NAME,
            crate::tasks_webhooks::webhook_fanout::MODEL_ACTIVITY_TASK_NAME,
        ]);
        expected.extend(crate::tasks_webhooks::link_crawl::TASK_NAMES);
        expected.extend(crate::integrations::git_sync::TASK_NAMES);
        expected.extend(crate::integrations::github_sync::TASK_NAMES);
        expected.insert(crate::license::tasks::TASK_NAME);
        expected.extend([crate::assistant::SWEEP_TASK]);
        expected.extend(crate::loop_scan::TASK_NAMES);
        expected.insert(crate::tasks_webhooks::ARCHIVE_AND_CLOSE_TASK);
        expected.extend([
            crate::tasks_ticker::scan::SCAN_DUE_TICKERS_TASK,
            crate::tasks_ticker::scan::SCAN_DUE_BINDINGS_TASK,
            crate::tasks_ticker::scan::FIRE_TICK_TASK,
            crate::tasks_ticker::scan::FIRE_SCHEDULER_BINDING_TASK,
            crate::dispatch::RUN_CLOUD_AGENT_TASK,
            crate::dispatch_execute::SCAN_QUEUED_RUNS_TASK,
            crate::dispatch_execute::SWEEP_STALE_RUNS_TASK,
            crate::dispatch_execute::EXPIRE_WAITING_RUNS_TASK,
        ]);
        expected.insert(crate::tasks_mail::NOTIFICATIONS_TASK);
        expected.extend(crate::runner_runs::TASK_NAMES);
        expected.insert(crate::tasks_mail::SEND_EMAIL_NOTIFICATION_TASK);
        expected.insert(crate::tasks_mail::STACK_EMAIL_NOTIFICATION_TASK);
        // D-07 membership mail (PIDASHCONV-1104): the four T6 names plus
        // the project invitation task.
        expected.extend(crate::tasks_mail::membership_mail::MEMBERSHIP_MAIL_TASKS);
        expected.insert(crate::tasks_mail::membership_mail::PROJECT_INVITATION_TASK);
        // D-07 auth mail (PIDASHCONV-1103): the four T5 names.
        expected.extend(crate::tasks_mail::AUTH_MAIL_TASKS);
        assert_eq!(
            crate::tasks_mail::membership_mail::MEMBERSHIP_MAIL_TASKS.len(),
            4,
            "membership group drifted"
        );
        assert_eq!(
            crate::tasks_mail::AUTH_MAIL_TASKS.len(),
            4,
            "auth mail group drifted"
        );
        expected.extend([
            crate::tasks_export::ISSUE_EXPORT_TASK,
            crate::tasks_export::ANALYTIC_EXPORT_TASK,
        ]);
        assert_eq!(expected.len(), 76, "worker-owned count drifted");

        for name in &expected {
            assert!(registry.owns(name), "{name} must be worker-owned");
            assert_eq!(route_for(&registry, name), Route::Local);
        }

        // Still Python-owned: the never-register sentinels from the
        // helper's comments.
        for name in [
            crate::tasks_cleanup::RESTORE_TASK_NAME,
            crate::assistant::RUN_TURN_TASK,
        ] {
            assert!(!registry.owns(name), "{name} must stay Python-owned");
            assert_eq!(route_for(&registry, name), Route::Unknown);
        }
        // Three of the four export names stay Python-owned;
        // `delete_old_s3_link` flipped to Rust (PIDASHCONV-858).
        assert_eq!(crate::tasks_cleanup::EXPORT_TASK_NAMES.len(), 4);
        for name in crate::tasks_cleanup::EXPORT_TASK_NAMES {
            if name == crate::tasks_cleanup::TASK_DELETE_OLD_S3_LINK
                || name == crate::tasks_export::ISSUE_EXPORT_TASK
                || name == crate::tasks_export::ANALYTIC_EXPORT_TASK
            {
                assert!(registry.owns(name), "{name} must be worker-owned");
                assert_eq!(route_for(&registry, name), Route::Local);
            } else {
                assert!(!registry.owns(name), "{name} must stay Python-owned");
                assert_eq!(route_for(&registry, name), Route::Unknown);
            }
        }
    }
}
