//! The built-in engine's daemon HTTP surface (#2910, NEWPIDASH-50).
//!
//! Three endpoints, all Django `APIView`s on `runner/views/engine.py`:
//!
//! * `GET /api/v1/runner/engine/profile/` (`EngineProfileEndpoint`)
//! * `POST /api/v1/runner/engine/model-token/`
//!   (`EngineModelTokenEndpoint`, `engine_model_token` throttle 30/min)
//! * `POST /api/v1/runner/engine/credential/revoke/`
//!   (`EngineCredentialRevokeEndpoint`)
//!
//! Rust owns all three; dispatch lands through the DB pool and SeaORM
//! writers in [`pidash_services::domain`], the model-profile and
//! model-credential reads through the assistant seam, and the engine
//! profile's `MANAGED_RUNNER_ENABLED` read through `AppState` settings.
//!
//! The web `engine-sign-out` route lives with the machine teardown
//! handlers (`runner_enroll/teardown.rs`), next to the machine revoke it
//! mirrors.

pub mod endpoints;

use axum::Router;

use crate::state::AppState;

/// Stamp DRF's `Allow` value on every response the engine handlers
/// return (`APIView.default_response_headers`, applied to errors too via
/// `finalize_response`).
fn with_allow(
    route: axum::routing::MethodRouter<AppState>,
    value: &'static str,
) -> axum::routing::MethodRouter<AppState> {
    use axum::http::header;
    route.layer(axum::middleware::map_response(
        move |mut response: axum::http::Response<axum::body::Body>| async move {
            response
                .headers_mut()
                .insert(header::ALLOW, header::HeaderValue::from_static(value));
            response
        },
    ))
}

/// Register the daemon engine endpoints (profile is GET-only; the
/// other two are POST-only). Non-owned verbs proxy to Django.
pub fn routes() -> Router<AppState> {
    use axum::routing::{get, post};

    Router::new()
        .route(
            "/api/v1/runner/engine/profile/",
            with_allow(
                crate::edge::owned(get(endpoints::engine_profile), &["GET"]),
                crate::runner_enroll::auth::ALLOW_GET,
            ),
        )
        .route(
            "/api/v1/runner/engine/model-token/",
            with_allow(
                crate::edge::owned(post(endpoints::engine_model_token), &["POST"]),
                crate::runner_enroll::auth::ALLOW_POST,
            ),
        )
        .route(
            "/api/v1/runner/engine/credential/revoke/",
            with_allow(
                crate::edge::owned(post(endpoints::engine_credential_revoke), &["POST"]),
                crate::runner_enroll::auth::ALLOW_POST,
            ),
        )
}
