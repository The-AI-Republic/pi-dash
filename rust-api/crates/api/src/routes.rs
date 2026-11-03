//! Router assembly.
//!
//! [`build_router`] is the extension seam: domain routers built by later
//! issues merge into it, and tests pass throwaway routers through
//! `with_routes` to prove the seam holds. Both delegate to
//! [`build_router_with_overlay`](crate::overlay::build_router_with_overlay)
//! with an additive-only [`Overlay`](crate::overlay::Overlay); named
//! group replacement lives in [`crate::overlay`].

use axum::Router;

use crate::overlay::{build_router_with_overlay, Overlay};
use crate::state::AppState;

/// Assemble the application router. `/` and `/robots.txt` are served by the
/// canonical D-00 handlers in [`crate::web`], Rust-owned only while the web
/// flag is on (otherwise they proxy like everything else); unmatched paths
/// proxy to Django, whose own 404 is the contract.
pub fn build_router(state: AppState) -> Router {
    with_routes(state, Router::new())
}

/// Assemble the router with extra (usually per-domain) routes merged in.
/// This is the seam domain port issues build against.
pub fn with_routes(state: AppState, extra: Router<AppState>) -> Router {
    build_router_with_overlay(state, Overlay::new().add_routes(extra))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edge;
    use axum::http::StatusCode;
    use axum::{routing::get, Json};
    use tower::ServiceExt;

    fn app() -> Router {
        build_router(AppState::new("0.1.0"))
    }

    #[tokio::test]
    async fn healthz_reports_ok_with_version() {
        let response = app()
            .oneshot(
                axum::http::Request::get("/healthz")
                    .body(axum::body::Body::empty())
                    .expect("request"),
            )
            .await
            .expect("serve");
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .expect("body");
        let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(
            json,
            serde_json::json!({"status": "ok", "version": "0.1.0"})
        );
    }

    #[tokio::test]
    async fn unmatched_paths_proxy_and_fail_closed_without_upstream() {
        // Port 1 is never bound, so the proxy must fail closed with a 502 in
        // the shared error shape — never a panic, never a Rust 404 that
        // would mask Django's own 404 page. (The default upstream port 8000
        // is not used here: a dev server may or may not listen on it.)
        let app = build_router(AppState::with_edge(
            "0.1.0",
            edge::EdgeHandle::for_tests("http://127.0.0.1:1"),
        ));
        let response = app
            .oneshot(
                axum::http::Request::get("/nope")
                    .body(axum::body::Body::empty())
                    .expect("request"),
            )
            .await
            .expect("serve");
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        let body = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .expect("body");
        let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(json["error"]["code"], "bad_gateway");
    }

    #[tokio::test]
    async fn extension_seam_merges_domain_routes() {
        let extra: Router<AppState> = Router::new().route(
            "/api/ping",
            get(|| async { Json(serde_json::json!({"pong": true})) }),
        );
        let app = with_routes(AppState::new("0.1.0"), extra);
        let response = app
            .oneshot(
                axum::http::Request::get("/api/ping")
                    .body(axum::body::Body::empty())
                    .expect("request"),
            )
            .await
            .expect("serve");
        assert_eq!(response.status(), StatusCode::OK);
    }
}
