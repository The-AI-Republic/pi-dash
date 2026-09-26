#![forbid(unsafe_code)]

//! The `pidash-api` binary: one binary, two modes.
//!
//! - `pidash-api serve` runs the axum HTTP server.
//! - `pidash-api worker` runs the background job loop (F-09 fills in the
//!   queue polling; until then it ticks so the mode is exercisable).
//!
//! [`build_app`] is the application seam (F-10): tests and later issues wrap
//! it with extra routes or layers without touching `main`.

use clap::{Parser, Subcommand};
use pidash_api::{with_routes, AppState, EdgeHandle};
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
/// Flags default off; use [`build_app_with_edge`] for a live edge.
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

async fn serve(bind: &str) -> MainResult {
    let addr: SocketAddr = bind.parse()?;
    let edge = EdgeHandle::from_env()?;
    tracing::info!(
        %addr,
        upstream = edge.upstream(),
        flags = ?edge.flags(),
        "serving HTTP"
    );
    let app = build_app_with_edge(env!("CARGO_PKG_VERSION"), None, edge);
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
    tracing::info!(
        concurrency,
        "worker loop started (queue polling lands in F-09)"
    );
    loop {
        tokio::select! {
            _ = shutdown_signal() => {
                tracing::info!("worker shutting down");
                return Ok(());
            }
            _ = tokio::time::sleep(Duration::from_secs(5)) => {
                tracing::debug!("worker tick");
            }
        }
    }
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}

type MainResult = Result<(), Box<dyn std::error::Error>>;

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
        let app = build_app("test", None);
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
        let app = build_app("test", Some(extra));
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
