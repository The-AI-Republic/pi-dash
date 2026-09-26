//! Router assembly.
//!
//! [`build_router`] is the extension seam (F-10): domain routers built by
//! later issues merge into it, and tests pass throwaway routers through
//! `with_routes` to prove the seam holds.

use axum::{http::StatusCode, response::IntoResponse, routing::get, Json, Router};
use pidash_services::health_report;
use pidash_types::Error;

use crate::state::AppState;

/// Assemble the application router. Unknown paths fall back to a JSON 404
/// with the same `error` shape every handler uses.
pub fn build_router(state: AppState) -> Router {
    with_routes(state, Router::new())
}

/// Assemble the router with extra (usually per-domain) routes merged in.
/// This is the seam domain port issues build against.
pub fn with_routes(state: AppState, extra: Router<AppState>) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .merge(extra)
        .fallback(not_found)
        .with_state(state)
}

async fn healthz(axum::extract::State(state): axum::extract::State<AppState>) -> impl IntoResponse {
    Json(health_report(state.version()))
}

async fn not_found() -> impl IntoResponse {
    let error = Error::NotFound("no route for this path".to_owned());
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({
            "error": {
                "code": error.error_key(),
                "message": error.to_string(),
            }
        })),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
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
    async fn unknown_paths_return_json_404() {
        let response = app()
            .oneshot(
                axum::http::Request::get("/nope")
                    .body(axum::body::Body::empty())
                    .expect("request"),
            )
            .await
            .expect("serve");
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .expect("body");
        let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(json["error"]["code"], "not_found");
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
