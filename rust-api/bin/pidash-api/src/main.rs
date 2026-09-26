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
//! runbook); named route-group replacement arrives under F-10.
//!
//! [`build_app`] is the application seam (F-10): tests and later issues wrap
//! it with extra routes or layers without touching `main`.

use clap::{Parser, Subcommand};
use pidash_api::{with_routes, AppState, EdgeHandle};
use pidash_db::config::Settings;
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
}

/// Assemble the axum application. `extra` merges additional routes (domain
/// routers from later issues); `None` serves the foundation routes only.
/// State is test-defaults; use [`build_app_with_edge`] for a live edge or
/// [`build_app_from_env`] for the `serve` path (settings + edge from env).
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
/// the application (F-03 + F-02). `serve` uses this; tests that need a
/// fixed state use [`build_app`] / [`build_app_with_edge`].
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

async fn serve(bind: &str) -> MainResult {
    let addr: SocketAddr = bind.parse()?;
    let edge = EdgeHandle::from_env()?;
    tracing::info!(
        %addr,
        upstream = edge.upstream(),
        flags = ?edge.flags(),
        "serving HTTP"
    );
    let app = build_app_from_env(env!("CARGO_PKG_VERSION"), None)?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;
    Ok(())
}

async fn worker(concurrency: u32) -> MainResult {
    let db = pidash_db::DbConfig::from_env()?;
    let pools = pidash_db::Pools::connect(&db, None).await?;
    tracing::info!(target = %db.redacted_url(), "connected postgres");
    pidash_jobs::queue::ensure_schema(pools.primary()).await?;
    // No broker in some environments (local dev without RabbitMQ): the
    // worker still runs, and Python-owned jobs requeue with a delay
    // instead of dropping. See `worker::forward`.
    let publisher = match pidash_jobs::AmqpConfig::from_env() {
        Ok(config) => match pidash_jobs::Publisher::connect(&config).await {
            Ok(publisher) => Some(publisher),
            Err(error) => {
                tracing::warn!(%error, "broker unreachable; Python-owned jobs will requeue");
                None
            }
        },
        Err(error) => {
            tracing::warn!(%error, "no broker configured; Python-owned jobs will requeue");
            None
        }
    };
    // Empty registry: no task group has a Rust handler yet (the D-07…D-10
    // ports register theirs), so every claimed job forwards to Python.
    let registry = pidash_jobs::Registry::new();
    let worker_config = pidash_jobs::WorkerConfig {
        concurrency: concurrency.max(1) as usize,
        ..Default::default()
    };
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    tracing::info!(concurrency, "worker + scheduler loops started");
    let worker_fut = pidash_jobs::worker::run_worker(
        pools.primary().clone(),
        registry,
        publisher,
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
            // scheduler to release its lock.
            tokio::select! {
                _ = &mut worker_fut => {}
                _ = &mut scheduler_fut => {}
                _ = tokio::time::sleep(Duration::from_secs(30)) => {
                    tracing::warn!("shutdown grace period expired");
                }
            }
        }
    }
    Ok(())
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}

type MainResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn main() -> MainResult {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    match cli.mode {
        Mode::Serve { bind } => runtime.block_on(serve(&bind)),
        Mode::Worker { concurrency } => runtime.block_on(worker(concurrency)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower::ServiceExt;

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
            Mode::Serve { .. } => panic!("wrong mode"),
        }
    }
}
