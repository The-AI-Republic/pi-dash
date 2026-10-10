//! Desktop agent model profile + token handlers (D-06, stage 5).
//!
//! Ports `apps/api/pi_dash/assistant/views/agent_profile.py:1-126`
//! (`AgentModelProfileEndpoint`, `AgentModelTokenEndpoint`, routes
//! `users/me/ai-assistant/agent-profile/` and
//! `users/me/ai-assistant/agent-token/` from `assistant/urls.py:90-98`).
//!
//! The desktop app does not re-implement provider resolution — it asks
//! these two endpoints, which run the same CE seam the server-side
//! callers use. The split is deliberate: the profile is safe to poll
//! and carries no secret; the credential is minted on demand,
//! rate-limited and audited.
//!
//! Both endpoints are desktop-only (`IsDesktopSession`,
//! `managed_runner/permissions.py:11-30`): a browser session is refused
//! with `desktop_session_required` (403) rather than a bare 403 so the
//! client can tell "not signed in" (401) apart from "not for browsers".
//! Only the token endpoint is throttled (`agent_profile.py:67-82`); the
//! profile endpoint carries no throttle class.
//!
//! Registration is the cutover granularity (same rule as the `space`,
//! `loop` and `prompting` families): the owned methods serve from Rust
//! while every other method on those paths proxies to Django. `HEAD`
//! rides axum's `get` handling on the profile path like Django's
//! `GET`-backed `HEAD`; on the token path (no GET in Django) `HEAD`
//! proxies so Django's own 405 answers.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Router;
use serde_json::Value;

use crate::middleware::SessionHandle;
use crate::state::AppState;

use super::{error_response, json_body, pool_of, request_actor, Denial};

/// `users/me/ai-assistant/agent-profile/` (under the `api/` include).
pub const PROFILE_PATH: &str = "/api/users/me/ai-assistant/agent-profile/";
/// `users/me/ai-assistant/agent-token/` (under the `api/` include).
pub const TOKEN_PATH: &str = "/api/users/me/ai-assistant/agent-token/";

/// Register the two owned paths. Sibling methods proxy to Django.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            PROFILE_PATH,
            axum::routing::get(get_profile)
                .post(crate::edge::proxy)
                .put(crate::edge::proxy)
                .patch(crate::edge::proxy)
                .delete(crate::edge::proxy)
                .head(crate::edge::proxy)
                .options(crate::edge::proxy),
        )
        .route(
            TOKEN_PATH,
            axum::routing::post(post_token)
                .get(crate::edge::proxy)
                .head(crate::edge::proxy)
                .put(crate::edge::proxy)
                .patch(crate::edge::proxy)
                .delete(crate::edge::proxy)
                .options(crate::edge::proxy),
        )
}

/// Session key CE uses to remember a desktop session
/// (`ee/authentication/desktop.py:22-23`). Set at sign-in by the desktop
/// flow; absent for browser sessions.
pub const DESKTOP_SESSION_KEY: &str = "pidash_client";
/// The session value marking a desktop client
/// (`ee/authentication/desktop.py:24`).
pub const DESKTOP_CLIENT: &str = "desktop";

/// Exact `IsDesktopSession.message`
/// (`managed_runner/permissions.py:20`): dict messages render as-is
/// (DRF `permission_denied` raises with the dict as `detail`, and the
/// exception handler echoes it), so the wire body is the dict itself.
pub const DESKTOP_DENIED_BODY: &str = "{\"error\":\"desktop_session_required\",\"detail\":\"This endpoint is available to the Pi Dash desktop app.\"}";

/// `request_is_desktop` (`ee/authentication/desktop.py:27-37`): the
/// session holds the desktop marker. A missing/unreadable session is not
/// a desktop (the `except` in Python returns `False`).
pub fn is_desktop_session(session: Option<&Value>) -> bool {
    matches!(
        session
            .and_then(|data| data.get(DESKTOP_SESSION_KEY))
            .and_then(Value::as_str),
        Some(DESKTOP_CLIENT)
    )
}

/// The desktop-only denial: 403 with the exact `IsDesktopSession`
/// message body.
fn desktop_denied() -> Response {
    Response::builder()
        .status(StatusCode::FORBIDDEN)
        .header(axum::http::header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(DESKTOP_DENIED_BODY))
        .expect("static desktop denial")
}

/// Map a credential failure onto a stable code the desktop can act on
/// (`_classify_credential_error`, `agent_profile.py:115-126`): `401`
/// means "sign in again" (the app stops the daemon and prompts); `503`
/// means "retry later" (the app keeps the current token until it
/// expires). Anything unrecognised is treated as transient rather than
/// signing the user out on a bug. `name` is the Python exception class
/// name; only the cloud overlay ever raises the session trio — CE only
/// ever reaches the `ManagedRunnerUnavailable` branch in the handler.
pub fn classify_credential_error(name: &str) -> (&'static str, StatusCode) {
    match name {
        "OpenHubAuthError" | "SessionRevoked" | "NoAIRepublicSession" => {
            ("gateway_session_revoked", StatusCode::UNAUTHORIZED)
        }
        _ => ("gateway_unavailable", StatusCode::SERVICE_UNAVAILABLE),
    }
}

/// `GET` the endpoint the bundled engine should call, and whether it
/// may (`AgentModelProfileEndpoint.get`, `:44-64`). Never returns a
/// credential. `managed_runner_enabled` travels here rather than being
/// baked into the desktop bundle so the operator kill switch takes
/// effect without shipping a new app build.
async fn get_profile(
    State(state): State<AppState>,
    extension: Option<axum::Extension<SessionHandle>>,
) -> Response {
    // Borrow the session marker before `extension` moves into auth
    // (DRF runs authentication before permissions, so anon still 401s).
    let desktop = is_desktop_from_extension(&extension);
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let actor = match request_actor(&state, &pool, extension).await {
        Ok(actor) => actor,
        Err(denial) => return denial.into_response(),
    };
    if !desktop {
        return desktop_denied();
    }
    let has_key = match fetch_llm_has_key(&pool, &actor.id).await {
        Ok(has_key) => has_key,
        Err(denial) => return denial.into_response(),
    };
    let profile = pidash_services::assistant::seams::agent_model_profile_for_user(has_key);
    let enabled = state.settings().managed_runner.enabled;
    // The instance switch is part of "can I run here", so it folds in
    // here rather than making the client combine two flags and risk
    // showing an available engine on a disabled instance.
    let available = enabled && profile.available;
    // `""` exactly when available, else the seam's reason (or the
    // operator-switch code when the seam has none).
    let reason_code = if available {
        String::new()
    } else {
        profile
            .reason_code
            .clone()
            .if_empty("managed_runner_disabled")
    };
    json_body(
        StatusCode::OK,
        &serde_json::json!({
            "managed_runner_enabled": enabled,
            "lane": profile.lane,
            "base_url": profile.base_url,
            "model": profile.model,
            "available": available,
            "reason_code": reason_code,
            "graceful_stop_seconds": state.settings().managed_runner.graceful_stop_secs,
        }),
    )
}

/// `POST` a short-lived credential for the bundled engine
/// (`AgentModelTokenEndpoint.post`, `:84-112`). Nothing is persisted
/// server-side by this call; it is throttled and audited by user rather
/// than treated as a mutation.
async fn post_token(
    State(state): State<AppState>,
    extension: Option<axum::Extension<SessionHandle>>,
) -> Response {
    let desktop = is_desktop_from_extension(&extension);
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let actor = match request_actor(&state, &pool, extension).await {
        Ok(actor) => actor,
        Err(denial) => return denial.into_response(),
    };
    // DRF `initial()` order is permissions before throttles: a browser
    // session answers the desktop denial even when throttled.
    if !desktop {
        return desktop_denied();
    }
    if !super::governor::throttle_check(
        super::throttles::AGENT_TOKEN_THROTTLE,
        &actor.id.to_string(),
    ) {
        return Denial::throttled().into_response();
    }
    match pidash_services::assistant::seams::agent_model_credential_for_user() {
        Ok((token, expires_at)) => {
            // Cloud-overlay shape only; CE always takes the error branch
            // below. Audited by issuance, never the token.
            tracing::info!(
                user = %actor.id,
                expires_at = %expires_at,
                "managed_runner.token_issued"
            );
            json_body(
                StatusCode::OK,
                &serde_json::json!({"token": token, "expires_at": expires_at}),
            )
        }
        Err(unavailable) => {
            // The client should have consulted the profile first; answer
            // with the same reason code it would have seen there.
            error_response(
                StatusCode::CONFLICT,
                &serde_json::json!({
                    "error": unavailable.code,
                    "detail": unavailable.to_string(),
                }),
            )
        }
    }
}

/// Whether the request's session carries the CE desktop marker. The
/// marker lives in the Django session row
/// (`ee/authentication/desktop.py`), which the session layer exposes
/// through the request snapshot; a missing snapshot is not a desktop.
pub fn is_desktop_from_extension(extension: &Option<axum::Extension<SessionHandle>>) -> bool {
    let data = extension.as_ref().map(|handle| handle.snapshot());
    // `RequestSession::get` takes `&mut self` (Django-session API), so
    // work on the owned snapshot.
    let mut data = data.unwrap_or_else(crate::middleware::RequestSession::empty);
    is_desktop_session(data.get(DESKTOP_SESSION_KEY).cloned().as_ref())
}

/// `get_config(user)` reduced to its key bit
/// (`runtime/llm.py:67-68` + `models.py:260-262`): the row for the user,
/// whether its encrypted key is non-empty.
async fn fetch_llm_has_key(pool: &sqlx::PgPool, user_id: &uuid::Uuid) -> Result<bool, Denial> {
    let row = sqlx::query(
        "SELECT \"api_key_encrypted\" FROM \"assistant_user_llm_config\" WHERE \"user_id\" = $1",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    use sqlx::Row as _;
    Ok(row
        .map(|row| {
            let encrypted: Option<Vec<u8>> = row.try_get("api_key_encrypted").unwrap_or(None);
            pidash_db::assistant::models::has_secret(&encrypted)
        })
        .unwrap_or(false))
}

/// `String::is_empty` fallback helper: `or` with a default when empty.
trait IfEmpty {
    fn if_empty(self, fallback: &str) -> String;
}

impl IfEmpty for String {
    fn if_empty(self, fallback: &str) -> String {
        if self.is_empty() {
            fallback.to_string()
        } else {
            self
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_marker_matches_the_ce_seam() {
        assert!(is_desktop_session(Some(&serde_json::json!({
            "pidash_client": "desktop",
        }))));
        assert!(!is_desktop_session(Some(&serde_json::json!({
            "pidash_client": "browser",
        }))));
        assert!(!is_desktop_session(Some(&serde_json::json!({}))));
        assert!(!is_desktop_session(None));
    }

    #[test]
    fn credential_taxonomy_matches_python() {
        for name in ["OpenHubAuthError", "SessionRevoked", "NoAIRepublicSession"] {
            assert_eq!(
                classify_credential_error(name),
                ("gateway_session_revoked", StatusCode::UNAUTHORIZED)
            );
        }
        for name in ["ValueError", "TimeoutError", "AnythingElse"] {
            assert_eq!(
                classify_credential_error(name),
                ("gateway_unavailable", StatusCode::SERVICE_UNAVAILABLE)
            );
        }
    }

    #[test]
    fn denial_body_is_byte_exact() {
        assert_eq!(
            DESKTOP_DENIED_BODY,
            "{\"error\":\"desktop_session_required\",\"detail\":\"This endpoint is available to the Pi Dash desktop app.\"}"
        );
    }
}
