#![forbid(unsafe_code)]

//! HTTP layer: axum routers, extractors, and middleware.
//!
//! Routers here mirror Django's URL paths exactly; per-domain routers live in
//! `src/<domain>/` and are merged into [`build_router`].
//!
//! - [`paginator`]: cursor paginator kernel (F-07).
//! - [`serializer`]: DRF-compatible JSON kernel (F-07).
//!
//! Middleware arrives under F-08.
//!
//! [`build_app`] is the composition point: the OSS binary and a private
//! overlay crate's own `main.rs` both call it with an [`AppState`] built
//! from their resolved [`Settings`](pidash_db::config::Settings) plus
//! their extra routes. Named route-group replacement arrives under F-10;
//! until then `extra` merges whole routers.

pub mod edge;
pub mod paginator;
pub mod routes;
pub mod serializer;
pub mod state;

pub use edge::{EdgeFlags, EdgeHandle, Prefix, DEFAULT_UPSTREAM};
pub use routes::{build_router, with_routes};
pub use state::AppState;

/// Assemble the full application: foundation routes plus `extra`
/// (domain routers from later issues, overlay routes from the private
/// crate). `None` serves the foundation routes only.
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
    with_routes(state, extra.unwrap_or_default())
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
