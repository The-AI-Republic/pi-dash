#![forbid(unsafe_code)]

//! The `pidash-api` binary: one binary, two modes.
//!
//! - `pidash-api serve` runs the axum HTTP server.
//! - `pidash-api worker` runs the job worker loop plus the beat-equivalent
//!   scheduler loop (F-09: Postgres queue, Celery-format forwarding).
//!
//! This `main` is deliberately thin: `serve` and `worker` are thin
//! callers of [`ServeBuilder`](pidash_api::startup::ServeBuilder) and
//! [`WorkerBuilder`](pidash_api::startup::WorkerBuilder), which own the
//! full startup sequence. A private overlay crate's own `main.rs`
//! composes the same builders with its own settings, seams, routes and
//! jobs (see the runbook), so OSS and overlay boot through one code path.

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

use clap::{Parser, Subcommand};
use std::net::SocketAddr;

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

async fn serve(bind: &str) -> MainResult {
    let addr: SocketAddr = bind.parse()?;
    pidash_api::startup::ServeBuilder::from_env()?
        .with_version(env!("CARGO_PKG_VERSION"))
        .with_bind(addr)
        .run()
        .await
}

async fn worker(concurrency: u32) -> MainResult {
    pidash_api::startup::WorkerBuilder::from_env()
        .await?
        .with_concurrency(concurrency)
        .run()
        .await
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
    use pidash_api::AppState;
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
    /// Through the builder the binary serves with; `for_tests` keeps the
    /// failure on the pool connect (no settings env is read).
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
        let err = pidash_api::startup::ServeBuilder::for_tests()
            .run()
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

    #[tokio::test]
    async fn built_app_serves_healthz() {
        let app = pidash_api::startup::ServeBuilder::for_tests()
            .build()
            .router;
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
        let app = pidash_api::startup::ServeBuilder::for_tests()
            .merge_routes(extra)
            .build()
            .router;
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
    /// PIDASHCONV-858). Mirrors the `WorkerBuilder` registration: the
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
        // Real registrations (the same calls the worker builder makes).
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
        // Export tasks (PIDASHCONV-1106): the same calls the worker builder makes.
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
    /// providers. Mirrors the `WorkerBuilder` registration: the real
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
        // Real registrations (the same calls the worker builder makes;
        // no transition hook — leg coverage lives in the contract suite).
        git_sync::register_git_sync_tasks(
            &mut registry,
            pool.clone(),
            git_sync::LiveProviders::from_env(),
            None,
        );
        github_sync::register_github_sync_tasks(
            &mut registry,
            pool.clone(),
            github_sync::LiveTransports::from_env(),
            None,
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
    /// PDASHOSS01-292). Mirrors the `WorkerBuilder` registration:
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
        // Same names + same handler constructors the worker builder uses (the
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
