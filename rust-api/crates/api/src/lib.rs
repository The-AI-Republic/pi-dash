#![forbid(unsafe_code)]

//! HTTP layer: axum routers, extractors, and middleware.
//!
//! Routers here mirror Django's URL paths exactly; per-domain routers live in
//! `src/<domain>/` and are merged into [`build_router`].
//!
//! - [`paginator`]: cursor paginator kernel (F-07).
//! - [`prompting`]: D-04 prompt-section handlers (guards + 4 routes).
//! - [`serializer`]: DRF-compatible JSON kernel (F-07).
//! - [`middleware`]: `MIDDLEWARE` equivalents (F-08), wrapping every app
//!   [`build_app`] builds.
//! - [`web`]: D-00 web edge handlers (`GET /`, `GET /robots.txt`).
//! - [`license`]: D-01 license / instance-console handlers (`api/instances/`).
//! - [`space`]: D-02 space public-API handlers (`api/public/`: project/meta/taxonomy/intake/issue + S3 assets).
//! - [`r#loop`]: D-03 loop auto-pm user handlers (`api/users/me/auto-pm/`).
//!
//! [`build_app`] is the composition point: the OSS binary and a private
//! overlay crate's own `main.rs` both call it with an [`AppState`] built
//! from their resolved [`Settings`](pidash_db::config::Settings) plus
//! their extra routes. Named route-group replacement lives in [`overlay`];
//! `extra` stays the additive-only path.

pub mod app_analytics;
pub mod app_assets;
pub mod app_cycles;
pub mod app_intake;
pub mod app_integrations;
pub mod app_issues;
pub mod app_modules;
pub mod app_notifications;
pub mod app_pages;
pub mod app_project;
pub mod app_scheduler;
pub mod app_views_search;
pub mod app_workspace;
pub mod assistant;
pub mod auth_oauth;
pub mod auth_session;
pub mod edge;
pub mod edge_shadow;
pub mod license;
pub mod r#loop;
pub mod middleware;
pub mod ops;
pub mod orchestration;
pub mod overlay;
pub mod paginator;
pub mod permissions;
pub mod project_move_handoff;
pub mod prompting;
pub mod routes;
pub mod runner_enroll;
pub mod runner_runs;
pub mod runner_sessions;
pub mod serializer;
pub mod space;
pub mod sse_body;
pub mod state;
pub mod v1_assets;
pub mod v1_cli_auth;
pub mod v1_cycles_modules;
pub mod v1_openapi;
pub mod v1_projects;
pub mod v1_work_items;
pub mod web;

pub use edge::{EdgeFlags, EdgeHandle, Prefix, DEFAULT_UPSTREAM};
pub use edge_shadow::{ShadowConfig, METRICS_PATH as SHADOW_METRICS_PATH};
pub use middleware::{
    stack, ApiTokenLogRecord, BodyLimitLayer, CorsConfig, CorsLayer, GzipLayer, LogSink,
    LoggerUserId, LoggingConfig, MemorySessionStore, PgSessionStore, RequestLogRecord,
    RequestLoggerLayer, RequestSession, SecurityConfig, SecurityLayer, SessionConfig,
    SessionExpiry, SessionHandle, SessionLayer, SessionRow, SessionStore, StoreError,
    StoredSession, TokenLogLayer, TracingSink,
};
pub use overlay::{build_router_with_overlay, Overlay, RouteGroup};
pub use routes::{build_router, with_routes};
pub use state::AppState;

/// Assemble the full application: foundation routes plus `extra`
/// (domain routers from later issues, overlay routes from the private
/// crate), wrapped in the F-08 middleware stack. `None` serves the
/// foundation routes only. For named group replacement (the private
/// crate claiming or dropping whole groups), build an [`Overlay`] and
/// call [`build_router_with_overlay`] instead, then wrap it in the same
/// [`stack`].
///
/// The stack reads its config from the state's F-03 `Settings`; the
/// session layer persists through `PgSessionStore` when the state carries
/// pools and stays transparent otherwise (`serve` connects pools at boot,
/// so a pool-less state only arises from non-`serve` constructors).
///
/// A private `main.rs` composes it like this (with `from_env`; the doctest
/// uses deterministic `test_defaults` so it runs hermetically):
///
/// ```
/// # fn private_routes() -> axum::Router<pidash_api::AppState> {
/// #     axum::Router::new()
/// # }
/// let settings = pidash_db::config::Settings::test_defaults();
/// let state = pidash_api::AppState::with_settings("0.1.0", settings);
/// let _app: axum::Router = pidash_api::build_app(state, Some(private_routes()));
/// ```
pub fn build_app(state: AppState, extra: Option<axum::Router<AppState>>) -> axum::Router {
    let settings = state.settings().clone();
    let store = state
        .pools()
        .map(|pools| PgSessionStore::new(pools.primary().clone()));
    let router = with_routes(state, extra.unwrap_or_default());
    stack(
        router,
        &CorsConfig::from_settings(&settings),
        &SecurityConfig::from_settings(&settings),
        settings.file_size_limit.max(0) as u64,
        &LoggingConfig::enabled(),
        SessionConfig::from_settings(&settings, store),
        TracingSink,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower::ServiceExt;

    /// The private-crate composition pattern: own settings, own routes,
    /// same builder. Guards the F-03 seam F-10 builds on.
    #[tokio::test]
    async fn private_style_composition_serves_overlay_routes() {
        let settings = pidash_db::config::Settings::test_defaults();
        let state = AppState::with_settings("overlay-1", settings);
        assert_eq!(state.version(), "overlay-1");
        assert!(!state.settings().debug);
        let extra: axum::Router<AppState> =
            axum::Router::new().route("/api/overlay-ping", axum::routing::get(|| async { "pong" }));
        let app = build_app(state, Some(extra));
        for path in ["/healthz", "/api/overlay-ping"] {
            let response = app
                .clone()
                .oneshot(
                    axum::http::Request::get(path)
                        .body(axum::body::Body::empty())
                        .expect("request"),
                )
                .await
                .expect("serve");
            assert_eq!(response.status(), axum::http::StatusCode::OK, "{path}");
        }
    }
}
