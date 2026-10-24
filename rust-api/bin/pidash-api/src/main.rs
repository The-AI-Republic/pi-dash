#![forbid(unsafe_code)]

//! The `pidash-api` binary: one binary, two modes.
//!
//! - `pidash-api serve` runs the axum HTTP server.
//! - `pidash-api worker` runs the job worker loop plus the beat-equivalent
//!   scheduler loop (F-09: Postgres queue, Celery-format forwarding).
//!
//! This `main` is deliberately thin: it resolves [`Settings`] from the
//! environment, builds an [`AppState`], and delegates to
//! [`pidash_api::build_app`]. A private overlay crate's own `main.rs`
//! composes the same builder with its own settings and routes (see the
//! runbook), claiming or dropping whole [`RouteGroup`](pidash_api::RouteGroup)
//! groups through an [`Overlay`](pidash_api::Overlay) where it replaces
//! OSS behaviour.
//!
//! [`build_app`] is the application seam: tests and later issues wrap it
//! with extra routes or layers without touching `main`.

/// Re-export shims for the inventory mirror test (PIDASHCONV-918):
/// the providers moved to `pidash_jobs::oss_worker`, and only the
/// `#[cfg(test)]` mirror still addresses them through the binary.
#[cfg(test)]
mod execute_seams;
mod inventory;
#[cfg(test)]
mod model_runtime;
#[cfg(test)]
mod runner_seams;
/// Live assistant turn runtime: the provider-backed half of
/// `assistant.run_turn` behind the jobs-crate `TurnSeam`.
/// PIDASHCONV-1110…1113 shipped the codec, provider streaming, MCP client
/// and local tools as tested library modules; PIDASHCONV-863 adds the agent
/// loop and the live seam, and `worker()` registers the handler.
mod turn;

use clap::{Parser, Subcommand};
use pidash_api::{with_routes, AppState, EdgeHandle};
use pidash_db::config::{NoOverlay, Profile, Settings};
use std::net::SocketAddr;
use std::time::Duration;

#[derive(Debug, Parser)]
#[command(name = "pidash-api", about = "Pi Dash Rust backend")]
struct Cli {
    #[command(subcommand)]
    mode: Mode,
}

#[derive(Debug, Subcommand)]
enum Mode {
    /// Run the HTTP server.
    Serve {
        /// Address to listen on.
        #[arg(long, default_value = "0.0.0.0:8080")]
        bind: String,
    },
    /// Run the background job worker.
    Worker {
        /// Jobs to run side by side.
        #[arg(long, default_value_t = 4)]
        concurrency: u32,
    },
    /// Run a Django management-command port (D-37 ops).
    Ops {
        #[command(subcommand)]
        command: pidash_ops::OpsCommand,
    },
    /// Print the route inventory (parity tooling, PIDASHCONV-820).
    Routes {
        /// Emit the machine-readable inventory as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Print the job inventory (parity tooling, PIDASHCONV-820).
    Jobs {
        /// Emit the machine-readable inventory as JSON.
        #[arg(long)]
        json: bool,
    },
}

/// Assemble the axum application. `extra` merges additional routes (domain
/// routers from later issues); `None` serves the foundation routes only.
/// State is test-defaults; use [`build_app_with_edge`] for a live edge or
/// [`build_app_from_env`] for a pool-less env state (tests). `serve`
/// builds its own state with pools attached.
pub fn build_app(version: &'static str, extra: Option<axum::Router<AppState>>) -> axum::Router {
    build_router_with(version, extra.unwrap_or_default())
}

/// Assemble the application with explicit cutover state (F-02): the Django
/// upstream and per-prefix flags come from `PIDASH_DJANGO_UPSTREAM` and
/// `PIDASH_RUST_*` in `serve`, or from the caller in tests.
pub fn build_app_with_edge(
    version: &'static str,
    extra: Option<axum::Router<AppState>>,
    edge: EdgeHandle,
) -> axum::Router {
    with_routes(
        AppState::with_edge(version, edge),
        extra.unwrap_or_default(),
    )
}

fn build_router_with(version: &'static str, extra: axum::Router<AppState>) -> axum::Router {
    with_routes(AppState::new(version), extra)
}

/// Resolve settings plus the cutover edge from the environment and assemble
/// the application (F-03 + F-02). Tests that need a fixed state use this
/// or [`build_app`] / [`build_app_with_edge`]; `serve` builds its own
/// state (settings + edge + pools) so unit tests never need a database.
#[cfg(test)]
fn build_app_from_env(
    version: &'static str,
    extra: Option<axum::Router<AppState>>,
) -> MainResult<axum::Router> {
    let settings = Settings::from_env()?;
    let edge = EdgeHandle::from_env()?;
    Ok(pidash_api::build_app(
        AppState::with_settings_and_edge(version, settings, edge),
        extra,
    ))
}

/// Connect the pools `serve` attaches to its state. A missing or
/// unreachable database is a boot error, never a per-request 500: without
/// pools every DB-backed handler answers the generic 500 (PIDASHCONV-126),
/// so serving pool-less is strictly worse than refusing to start (same
/// fail-fast as `worker`).
///
/// `DATABASE_URL` must be a TCP `postgres://` URL; socket-dir URLs
/// (`?host=/tmp`) are rejected by the URL parser at connect time.
async fn connect_serve_pools() -> Result<pidash_db::Pools, Box<dyn std::error::Error>> {
    let db = pidash_db::DbConfig::from_env()?;
    let pools = pidash_db::Pools::connect(&db, None).await?;
    tracing::info!(db = %db.redacted_url(), "connected postgres");
    Ok(pools)
}

/// Resolve [`Settings`] the way Django resolves its settings module:
/// `DJANGO_SETTINGS_MODULE`'s last segment picks the [`Profile`]
/// (`test` / `local` / `production`); anything else, including unset,
/// keeps [`Profile::Common`]. `serve` and `worker` boot the same profile
/// as the Django server/worker they run beside, so profile-driven
/// settings — `EMAIL_BACKEND` first among them (PIDASHCONV-1126) — agree
/// instead of Rust always answering with the common (SMTP) resolution.
fn settings_from_env() -> MainResult<Settings> {
    Ok(Settings::from_env_with(
        profile_from_settings_module(),
        &NoOverlay,
    )?)
}

/// Pure mapping behind [`settings_from_env`], split out for unit tests.
fn profile_from_settings_module() -> Profile {
    let module = std::env::var("DJANGO_SETTINGS_MODULE").unwrap_or_default();
    match module.rsplit('.').next().unwrap_or("") {
        "test" => Profile::Test,
        "local" => Profile::Local,
        "production" => Profile::Production,
        _ => Profile::Common,
    }
}

async fn serve(bind: &str) -> MainResult {
    let addr: SocketAddr = bind.parse()?;
    let settings = settings_from_env()?;
    let edge = EdgeHandle::from_env()?;
    let pools = connect_serve_pools().await?;
    // Enqueue paths (magic-link mail, password mail, …) INSERT into the
    // Rust-owned queue tables; without them every such handler 500s.
    // `worker` runs the same DDL at its own boot, but `serve` also runs
    // workerless (the V-02 matrix starts no worker for the Rust run), so
    // `serve` must ensure the schema too (PIDASHCONV-854). Fail-fast like
    // `worker`: refusing to start beats per-request 500s.
    pidash_jobs::queue::ensure_schema(pools.primary()).await?;
    tracing::info!(
        %addr,
        upstream = edge.upstream(),
        flags = ?edge.flags(),
        "serving HTTP"
    );
    // Shared Redis client (PIDASHCONV-265): cancel signals, the throttle
    // cache, and the SSE live tail multiplex over it. Absent without a
    // `REDIS_URL` (handlers degrade per-site: swallow / allow / replay-only).
    let redis = pidash_db::redis::RedisHandle::from_settings(&settings);
    if redis.is_some() {
        tracing::info!("redis handle ready (client connects lazily per operation)");
    }
    let mut state = AppState::with_settings_and_edge(env!("CARGO_PKG_VERSION"), settings, edge)
        .with_pools(pools);
    if let Some(handle) = redis {
        state = state.with_redis(handle);
    }
    let app = pidash_api::build_app(state, None);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;
    Ok(())
}

/// Seed-data directory for `workspace_seed` (`settings.SEED_DIR`,
/// `apps/api/pi_dash/settings/common.py:743`): `SEED_DIR` env, else the
/// `seeds` checkout dir. Missing files only empty the seed.
fn seed_data_dir() -> std::path::PathBuf {
    std::env::var("SEED_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from("seeds"))
}

async fn worker(concurrency: u32) -> MainResult {
    let db = pidash_db::DbConfig::from_env()?;
    let pools = pidash_db::Pools::connect(&db, None).await?;
    tracing::info!(target = %db.redacted_url(), "connected postgres");
    pidash_jobs::queue::ensure_schema(pools.primary()).await?;
    // No broker: the job queue is Postgres and every task name has a
    // registered handler; an unknown name parks as failed
    // (`unknown task <name>`), never forwarded to Python (PIDASHCONV-1108,
    // the standalone rule).
    // Task handlers register through the shared OSS composition
    // (`pidash_jobs::register_oss_handlers`, PIDASHCONV-918). The async inputs —
    // the Mongo sink and the boot-loaded fire-tick states — load here,
    // exactly as before; registration itself is a pure function of
    // the context.
    let mut registry = pidash_jobs::Registry::new();
    let settings = settings_from_env()?;
    let mongo = pidash_db::tasks_cleanup::cleanup_queries::MongoSink::from_env().await;
    if mongo.is_none() {
        tracing::info!("MongoDB not configured; cleanup tasks will delete from Postgres only");
    }
    // The CE assistant seam: the same default `serve` installs on the
    // app state, so worker-side runs-creation gates answer exactly as
    // the serve-side gates do (PIDASHCONV-1054). The cloud binary
    // installs its overlay implementation here instead.
    let assistant_seam =
        std::sync::Arc::new(pidash_api::assistant::model_seam::CeAssistantSeam::new(
            std::sync::Arc::new(settings.clone()),
        ));
    // The live assistant turn seam (PIDASHCONV-863): the multiplexed Redis
    // client for the cancel key and the SSE publishes (`None` when
    // `REDIS_URL` is unset, exactly like Django's swallowed
    // `redis_instance()`), the worker pool, and settings. Construction does
    // no I/O.
    let turn_redis = match settings.redis.url.as_deref().filter(|url| !url.is_empty()) {
        Some(url) => match pidash_db::redis::open_client(url) {
            Ok(client) => Some(std::sync::Arc::new(client)),
            Err(error) => {
                tracing::warn!(%error, "assistant: REDIS_URL unparsable; cancel key disabled");
                None
            }
        },
        None => None,
    };
    let turn_seam = std::sync::Arc::new(
        turn::live::LiveTurnSeam::new(
            pools.primary().clone(),
            std::sync::Arc::new(settings.clone()),
            turn_redis,
        )
        .map_err(|error| format!("assistant turn seam: {error}"))?,
    );
    pidash_jobs::register_oss_handlers(
        &mut registry,
        &pidash_jobs::OssWorkerCtx {
            pools: pools.clone(),
            settings,
            mongo,
            seed_dir: seed_data_dir(),
            assistant_seam,
        },
    )?;
    // The turn handler (PIDASHCONV-863): `worker()` now owns
    // `assistant.run_turn` (the sweep above is registered by
    // `register_oss_handlers`). Registering makes the name locally
    // executed.
    pidash_jobs::assistant::register_turn_handler(
        &mut registry,
        pools.primary().clone(),
        turn_seam,
    );
    let worker_config = pidash_jobs::WorkerConfig {
        concurrency: concurrency.max(1) as usize,
        ..Default::default()
    };
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    tracing::info!(concurrency, "worker + scheduler loops started");
    let worker_fut = pidash_jobs::worker::run_worker(
        pools.primary().clone(),
        registry,
        worker_config,
        shutdown_rx.clone(),
    );
    let scheduler_fut = pidash_jobs::scheduler::run_scheduler(
        pools.primary().clone(),
        pidash_jobs::default_schedule(),
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

/// Graceful-shutdown trigger for `serve` and `worker`: SIGINT or SIGTERM.
///
/// `ctrl_c()` alone listens for SIGINT only, but containers stop with
/// SIGTERM — without a SIGTERM arm the drain paths never run on redeploy
/// (PIDASHCONV-1326): as PID 1 the kernel ignores the bare SIGTERM until
/// SIGKILL, otherwise the default action kills the process at once. Celery
/// (warm shutdown) and gunicorn (graceful timeout) both drain on SIGTERM;
/// this future matches them.
async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("SIGTERM handler installs");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = terminate.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

type MainResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn main() -> MainResult {
    // Both `ring` (via reqwest) and `aws-lc-rs` (via mongodb) are enabled
    // in this binary, so rustls cannot pick a process-level CryptoProvider
    // by itself and `ClientConfig::builder()` (which redis uses for
    // `rediss://`) panics. Install ring first, before any TLS use; a
    // previously installed provider (tests, embedding) is fine too.
    let _ = rustls::crypto::ring::default_provider().install_default();
    let cli = Cli::parse();
    // Ops commands mirror Django management output byte for byte, so
    // they emit no tracing events: the subscriber stays off and stderr
    // carries only failures. Same for the inventory commands: stdout
    // carries the inventory (JSON with --json) and nothing else.
    if !matches!(
        cli.mode,
        Mode::Ops { .. } | Mode::Routes { .. } | Mode::Jobs { .. }
    ) {
        tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
            )
            .init();
    }
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    match cli.mode {
        Mode::Serve { bind } => runtime.block_on(serve(&bind)),
        Mode::Worker { concurrency } => runtime.block_on(worker(concurrency)),
        Mode::Ops { command } => runtime.block_on(pidash_ops::run(command)),
        Mode::Routes { json } => {
            if json {
                println!("{}", inventory::routes_json());
            } else {
                print!("{}", inventory::routes_text());
            }
            Ok(())
        }
        Mode::Jobs { json } => {
            if json {
                println!("{}", inventory::jobs_json());
            } else {
                print!("{}", inventory::jobs_text());
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower::ServiceExt;

    /// Assistant-model seam double for registration-only tests: the
    /// trait defaults with a canned title (no test here drives
    /// dispatch).
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

    /// Serialise the env-mutating boot tests: they share the process
    /// environment with the other tests in this binary.
    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        static ENV_LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
        // Ignore poisoning: a failed sibling test must not cascade.
        ENV_LOCK
            .get_or_init(|| std::sync::Mutex::new(()))
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    /// PIDASHCONV-126: `serve` must fail fast at boot without a database
    /// instead of starting pool-less (every DB-backed handler 500s then).
    #[tokio::test]
    async fn serve_pools_fail_fast_without_database_url() {
        // Remove under the lock, restore under the lock, but never hold
        // it across the await (clippy::await_holding_lock); no other test
        // in this binary reads DATABASE_URL.
        let saved = {
            let _guard = env_lock();
            let saved = std::env::var("DATABASE_URL").ok();
            std::env::remove_var("DATABASE_URL");
            saved
        };
        let err = connect_serve_pools()
            .await
            .expect_err("serve boot without DATABASE_URL must fail");
        assert!(
            err.to_string().contains("DATABASE_URL"),
            "unexpected error: {err}"
        );
        {
            let _guard = env_lock();
            if let Some(value) = saved {
                std::env::set_var("DATABASE_URL", value);
            }
        }
    }

    /// PIDASHCONV-1126: `serve`/`worker` boot the profile matching
    /// `DJANGO_SETTINGS_MODULE`'s last segment, Common when unset or
    /// unknown — the same module Django itself boots from.
    #[test]
    fn settings_module_selects_profile() {
        let _guard = env_lock();
        let saved = std::env::var("DJANGO_SETTINGS_MODULE").ok();
        for (module, expected) in [
            ("pi_dash.settings.test", Profile::Test),
            ("pi_dash.settings.local", Profile::Local),
            ("pi_dash.settings.production", Profile::Production),
            ("pi_dash.settings.common", Profile::Common),
            ("something.else", Profile::Common),
            ("", Profile::Common),
        ] {
            std::env::set_var("DJANGO_SETTINGS_MODULE", module);
            assert_eq!(profile_from_settings_module(), expected, "module={module}");
        }
        std::env::remove_var("DJANGO_SETTINGS_MODULE");
        assert_eq!(profile_from_settings_module(), Profile::Common);
        if let Some(value) = saved {
            std::env::set_var("DJANGO_SETTINGS_MODULE", value);
        }
    }

    #[tokio::test]
    async fn built_app_serves_healthz() {
        let app = build_app_from_env("test", None).expect("settings resolve");
        let response = app
            .oneshot(
                axum::http::Request::get("/healthz")
                    .body(axum::body::Body::empty())
                    .expect("request"),
            )
            .await
            .expect("serve");
        assert_eq!(response.status(), axum::http::StatusCode::OK);
    }

    #[tokio::test]
    async fn built_app_accepts_extra_routes() {
        let extra: axum::Router<AppState> =
            axum::Router::new().route("/api/ping", axum::routing::get(|| async { "pong" }));
        let app = build_app_from_env("test", Some(extra)).expect("settings resolve");
        let response = app
            .oneshot(
                axum::http::Request::get("/api/ping")
                    .body(axum::body::Body::empty())
                    .expect("request"),
            )
            .await
            .expect("serve");
        assert_eq!(response.status(), axum::http::StatusCode::OK);
    }

    #[test]
    fn cli_parses_serve_and_worker_modes() {
        let serve = Cli::try_parse_from(["pidash-api", "serve"]).expect("serve");
        assert!(matches!(serve.mode, Mode::Serve { .. }));
        let worker =
            Cli::try_parse_from(["pidash-api", "worker", "--concurrency", "2"]).expect("worker");
        match worker.mode {
            Mode::Worker { concurrency } => assert_eq!(concurrency, 2),
            _ => panic!("wrong mode"),
        }
    }

    #[test]
    fn cli_parses_inventory_modes() {
        let routes = Cli::try_parse_from(["pidash-api", "routes", "--json"]).expect("routes");
        match routes.mode {
            Mode::Routes { json } => assert!(json),
            _ => panic!("wrong mode"),
        }
        let jobs = Cli::try_parse_from(["pidash-api", "jobs"]).expect("jobs");
        match jobs.mode {
            Mode::Jobs { json } => assert!(!json),
            _ => panic!("wrong mode"),
        }
    }

    #[test]
    fn cli_parses_ops_instance_commands() {
        let ops = Cli::try_parse_from(["pidash-api", "ops", "instance", "configure-instance"])
            .expect("ops instance");
        assert!(matches!(ops.mode, Mode::Ops { .. }));
        let ops = Cli::try_parse_from([
            "pidash-api",
            "ops",
            "instance",
            "dry-run-scheduler-migration",
            "--workspace",
            "acme",
            "--json",
        ])
        .expect("ops dry-run flags");
        match ops.mode {
            Mode::Ops {
                command:
                    pidash_ops::OpsCommand::Instance(
                        pidash_ops::instance::InstanceCommand::DryRunSchedulerMigration {
                            workspace,
                            json,
                        },
                    ),
            } => {
                assert_eq!(workspace.as_deref(), Some("acme"));
                assert!(json);
            }
            _ => panic!("wrong mode"),
        }
    }

    /// The `ops` argv shapes moved to `pidash-ops` with the modules
    /// (PIDASHCONV-919); they stay pinned here, against the real binary
    /// `Cli`, with the same assertions.
    fn parse_ops(argv: &[&str]) -> pidash_ops::OpsCommand {
        let mut full = vec!["pidash-api", "ops"];
        full.extend(argv.iter());
        match Cli::try_parse_from(full)
            .unwrap_or_else(|error| panic!("parses {argv:?}: {error}"))
            .mode
        {
            Mode::Ops { command } => command,
            _ => panic!("wrong mode for {argv:?}"),
        }
    }

    /// Command names are underscored Django names, not kebab-case.
    #[test]
    fn command_names_match_django() {
        for (argv, check) in [
            ("wait_for_db", "WaitForDb"),
            ("wait_for_migrations", "WaitForMigrations"),
            ("clear_cache", "ClearCache"),
            ("create_bucket", "CreateBucket"),
            ("update_bucket", "UpdateBucket"),
        ] {
            let command = parse_ops(&[argv]);
            assert!(
                format!("{command:?}").starts_with(check),
                "{argv} parses to {check}"
            );
        }
        assert!(
            Cli::try_parse_from(["pidash-api", "ops", "wait-for-db"]).is_err(),
            "kebab-case is not a Django command name"
        );
    }

    /// `--key` takes an optional value (`nargs="?"`): absent and bare
    /// both yield the full-clear path, like argparse's `None` const.
    #[test]
    fn clear_cache_key_arg_shapes() {
        assert!(matches!(
            parse_ops(&["clear_cache"]),
            pidash_ops::OpsCommand::ClearCache(pidash_ops::boot::ClearCacheArgs { key: None })
        ));
        assert!(matches!(
            parse_ops(&["clear_cache", "--key"]),
            pidash_ops::OpsCommand::ClearCache(pidash_ops::boot::ClearCacheArgs { key: Some(_) })
        ));
        match parse_ops(&["clear_cache", "--key", "k"]) {
            pidash_ops::OpsCommand::ClearCache(args) => {
                assert_eq!(args.key.as_deref(), Some("k"))
            }
            _ => panic!("wrong variant"),
        }
    }

    /// Unknown options fail with exit code 2, like argparse.
    #[test]
    fn unknown_option_exits_2() {
        let error = Cli::try_parse_from(["pidash-api", "ops", "clear_cache", "--bogus"])
            .expect_err("rejects");
        assert_eq!(error.exit_code(), 2);
    }

    #[test]
    fn cli_parses_ops_activate_user() {
        let ops = Cli::try_parse_from(["pidash-api", "ops", "activate_user", "a@b.com"])
            .expect("ops activate_user");
        assert!(matches!(
            ops.mode,
            Mode::Ops {
                command: pidash_ops::OpsCommand::ActivateUser(_)
            }
        ));
    }

    /// PIDASHCONV-224: the worker owns every local D-09 task name, while
    /// `restore_related_objects` and the three remaining export tasks
    /// still route to Python (`delete_old_s3_link` flipped to Rust in
    /// PIDASHCONV-858). Mirrors the `worker()` registration block: the
    /// `PgPool` groups register for real (lazy pool, no I/O); the `Pools`
    /// groups (cleanup, deletion) register stubs over the same name
    /// constants the real register fns consume — `Pools` cannot be built
    /// without a database (same precedent as `all_five_tasks_registered`).
    #[tokio::test]
    async fn worker_registry_owns_all_d09_local_tasks() {
        use pidash_jobs::tasks_cleanup;
        use pidash_jobs::worker::{Handler, Registry, Verdict};

        // `connect_lazy` never touches the network (it needs a Tokio
        // context to build the pool, hence `tokio::test`): registration
        // wiring stays testable with no database.
        let pool =
            sqlx::PgPool::connect_lazy("postgres://localhost:1/unused").expect("lazy pool builds");
        let mut registry = Registry::new();
        // Real registrations (the same calls `worker()` makes).
        tasks_cleanup::register_versions(&mut registry, pool.clone());
        tasks_cleanup::assets::register_assets(
            &mut registry,
            pool.clone(),
            std::sync::Arc::new(tasks_cleanup::assets::LiveS3ObjectStore::from_settings(
                &pidash_db::config::Settings::test_defaults(),
            )),
            std::sync::Arc::new(tasks_cleanup::assets::NoopLiveConvert),
        );
        let seed_settings = pidash_db::config::Settings::test_defaults();
        tasks_cleanup::register_workspace_seed(
            &mut registry,
            pool.clone(),
            std::env::temp_dir(),
            seed_settings.cloud_agent.clone(),
            seed_settings.managed_runner.clone(),
            std::sync::Arc::new(pidash_services::extensions::NoExtraToolsets),
            std::sync::Arc::new(DefaultSeam),
        );
        tasks_cleanup::dummy_data::register(&mut registry, pool.clone());
        tasks_cleanup::register_expiry(
            &mut registry,
            pool.clone(),
            std::sync::Arc::new(tasks_cleanup::expiry::LiveS3Deleter::from_settings(
                &pidash_db::config::Settings::test_defaults(),
            )),
        );
        // Export tasks (PIDASHCONV-1106): the same calls `worker()` makes.
        pidash_jobs::tasks_export::register_issue_export_task(&mut registry, pool.clone());
        pidash_jobs::tasks_export::register_analytic_task(&mut registry, pool.clone());
        // Stub-backed: the exact names the `Pools`-taking registers own.
        for spec in tasks_cleanup::cleanup::TASKS {
            let handler: Handler = std::sync::Arc::new(|_| Box::pin(async { Ok(Verdict::Ack) }));
            registry.register(spec.task, handler);
        }
        for name in [
            tasks_cleanup::SOFT_DELETE_TASK,
            tasks_cleanup::HARD_DELETE_TASK,
        ] {
            let handler: Handler = std::sync::Arc::new(|_| Box::pin(async { Ok(Verdict::Ack) }));
            registry.register(name, handler);
        }

        let mut local: Vec<&str> = tasks_cleanup::cleanup::TASKS
            .iter()
            .map(|spec| spec.task)
            .collect();
        local.extend([
            tasks_cleanup::SOFT_DELETE_TASK,
            tasks_cleanup::HARD_DELETE_TASK,
            tasks_cleanup::assets::TASK_DELETE_UNUPLOADED,
            tasks_cleanup::assets::TASK_GET_METADATA,
            tasks_cleanup::assets::TASK_COPY_S3_OBJECTS,
            tasks_cleanup::WORKSPACE_SEED_TASK_NAME,
            tasks_cleanup::dummy_data::TASK_NAME,
            tasks_cleanup::TASK_DELETE_OLD_S3_LINK,
        ]);
        local.extend(tasks_cleanup::versions::ALL_VERSION_TASKS);
        assert_eq!(local.len(), 20, "D-09 local task count drifted");
        for name in local {
            assert!(registry.owns(name), "{name} must be worker-owned");
            if name != tasks_cleanup::TASK_DELETE_OLD_S3_LINK {
                assert!(
                    !tasks_cleanup::is_export_task(name),
                    "{name} must not be an export task"
                );
            }
        }

        // Still Python-owned: restore (BUG-DEL-2) plus the dead CSV-mail
        // export name. `delete_old_s3_link` (PIDASHCONV-858) and the two
        // live exports (PIDASHCONV-1106) are worker-owned now.
        assert!(!registry.owns(tasks_cleanup::RESTORE_TASK_NAME));
        assert_eq!(tasks_cleanup::EXPORT_TASK_NAMES.len(), 4);
        for name in tasks_cleanup::EXPORT_TASK_NAMES {
            assert!(tasks_cleanup::is_export_task(name));
            if name == tasks_cleanup::TASK_DELETE_OLD_S3_LINK
                || name == pidash_jobs::tasks_export::ISSUE_EXPORT_TASK
                || name == pidash_jobs::tasks_export::ANALYTIC_EXPORT_TASK
            {
                assert!(registry.owns(name), "{name} must be worker-owned");
            } else {
                assert!(!registry.owns(name), "{name} must stay Python-owned");
            }
        }
    }

    /// PIDASHCONV-238: the worker owns all six D-05 task names with live
    /// providers. Mirrors the `worker()` registration block: the real
    /// register fns over a lazy pool (no I/O) plus `from_env` providers
    /// (`Keyring::from_env` falls back to an empty secret, so no env is
    /// needed). `github_signals` owns no names of its own — its dispatch
    /// targets the git `post_completion_comment` task.
    #[tokio::test]
    async fn worker_registry_owns_all_d05_local_tasks() {
        use pidash_jobs::integrations::{git_sync, github_sync};
        use pidash_jobs::worker::Registry;

        // `connect_lazy` never touches the network (it needs a Tokio
        // context to build the pool, hence `tokio::test`): registration
        // wiring stays testable with no database.
        let pool =
            sqlx::PgPool::connect_lazy("postgres://localhost:1/unused").expect("lazy pool builds");
        let mut registry = Registry::new();
        // Real registrations (the same calls `worker()` makes).
        git_sync::register_git_sync_tasks(
            &mut registry,
            pool.clone(),
            git_sync::LiveProviders::from_env(),
        );
        github_sync::register_github_sync_tasks(
            &mut registry,
            pool.clone(),
            github_sync::LiveTransports::from_env(),
        );

        let mut local: Vec<&str> = git_sync::TASK_NAMES.to_vec();
        local.extend(github_sync::TASK_NAMES);
        assert_eq!(local.len(), 6, "D-05 local task count drifted");
        for name in local {
            assert!(registry.owns(name), "{name} must be worker-owned");
        }
    }

    /// PIDASHCONV-260 (FX-LOG-01) + PIDASHCONV-1107: the worker owns
    /// both sink tasks — `PROCESS_LOGS_TASK` (EXECUTE parity per
    /// PIDASHCONV-242) and `TRACK_EVENT_TASK` (standalone-Rust rule:
    /// the Rust worker runs it; Django drops it in production per
    /// PDASHOSS01-292). Mirrors the `worker()` registration block:
    /// `register_sink_tasks` needs a `Pools`, which cannot be built
    /// without a database (same precedent as the `Pools`-taking groups
    /// in `worker_registry_owns_all_d09_local_tasks`), so the stubs
    /// below are registered over the exact name constants the real
    /// call uses — the live execution itself is pinned by the
    /// contract replays (`test_rust_process_logs_executes_locally`,
    /// `test_rust_track_event_executes_locally`).
    #[test]
    fn worker_registry_owns_sink_tasks() {
        use pidash_jobs::tasks_webhooks::{PROCESS_LOGS_TASK, TRACK_EVENT_TASK};
        use pidash_jobs::worker::{Handler, Registry, Verdict};

        let mut registry = Registry::new();
        // Same names + same handler constructors `worker()` uses (the
        // real `register_sink_tasks` needs live pools, so stubs stand
        // in for the handler bodies — never for the names).
        for name in [PROCESS_LOGS_TASK, TRACK_EVENT_TASK] {
            let handler: Handler = std::sync::Arc::new(|_| Box::pin(async { Ok(Verdict::Ack) }));
            registry.register(name, handler);
        }

        assert_eq!(
            PROCESS_LOGS_TASK,
            "pi_dash.bgtasks.logger_task.process_logs"
        );
        assert_eq!(
            TRACK_EVENT_TASK,
            "pi_dash.bgtasks.event_tracking_task.track_event"
        );
        for name in [PROCESS_LOGS_TASK, TRACK_EVENT_TASK] {
            assert!(registry.owns(name), "{name} must be worker-owned");
        }
    }
}
