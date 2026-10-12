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
    let profile = state
        .assistant_seam()
        .agent_model_profile(actor.id, has_key)
        .await;
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
    match state
        .assistant_seam()
        .agent_model_credential(actor.id)
        .await
    {
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
            // `str(exc)`: the detail, or the code when empty (the
            // `ManagedRunnerUnavailable` Display the ported seam renders).
            let reason = unavailable.reason;
            let detail = if unavailable.message.is_empty() {
                reason.clone()
            } else {
                unavailable.message
            };
            error_response(
                StatusCode::CONFLICT,
                &serde_json::json!({
                    "error": reason,
                    "detail": detail,
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
    // Compare the marker value itself: `is_desktop_session` takes the
    // whole session dict, so handing it one extracted value double-indexes
    // and is never a desktop (every desktop caller 403s).
    matches!(
        data.get(DESKTOP_SESSION_KEY).and_then(Value::as_str),
        Some(DESKTOP_CLIENT)
    )
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
    fn extension_check_reads_the_marker_value() {
        use crate::middleware::RequestSession;
        fn with_marker(value: Option<Value>) -> Option<axum::Extension<SessionHandle>> {
            let mut session = RequestSession::empty();
            if let Some(value) = value {
                session.set(DESKTOP_SESSION_KEY.to_owned(), value);
            }
            Some(axum::Extension(SessionHandle::new(session)))
        }
        assert!(is_desktop_from_extension(&with_marker(Some(
            Value::String(DESKTOP_CLIENT.to_owned())
        ))));
        assert!(!is_desktop_from_extension(&with_marker(Some(
            Value::String("browser".to_owned())
        ))));
        assert!(!is_desktop_from_extension(&with_marker(None)));
        assert!(!is_desktop_from_extension(&None));
    }

    #[test]
    fn denial_body_is_byte_exact() {
        assert_eq!(
            DESKTOP_DENIED_BODY,
            "{\"error\":\"desktop_session_required\",\"detail\":\"This endpoint is available to the Pi Dash desktop app.\"}"
        );
    }
}

/// Live scratch-DB proof that both handlers consult the state-carried seam
/// (PIDASHCONV-914), env-gated like the `handlers_members` live tests:
/// without `DATABASE_URL` these skip. A forged desktop session (the
/// `_auth_*` trio plus the CE desktop marker, no middleware) drives the
/// handlers directly; the fake seam's profile and token must reach the
/// wire, and the CE default must answer exactly like the ported seam.
#[cfg(test)]
mod live_tests {
    use std::sync::Arc;

    use super::*;
    use pidash_services::extensions::{
        AgentModelProfile, AssistantModelSeam, CredentialUnavailable, SeamFuture, TitleError,
        TitleRequest,
    };

    /// Overlay-shaped seam: the gateway lane CE never has.
    #[derive(Debug)]
    struct GatewaySeam;

    impl AssistantModelSeam for GatewaySeam {
        fn agent_model_profile(
            &self,
            user_id: uuid::Uuid,
            has_api_key: bool,
        ) -> SeamFuture<'_, AgentModelProfile> {
            let _ = (user_id, has_api_key);
            Box::pin(async move {
                AgentModelProfile {
                    available: true,
                    lane: "openhub".to_owned(),
                    base_url: "https://gateway.example.com".to_owned(),
                    model: "hub-default".to_owned(),
                    reason_code: String::new(),
                }
            })
        }

        fn agent_model_credential(
            &self,
            user_id: uuid::Uuid,
        ) -> SeamFuture<'_, Result<(String, String), CredentialUnavailable>> {
            let _ = user_id;
            Box::pin(async move {
                Ok((
                    "gateway-token".to_owned(),
                    "2030-01-01T00:00:00Z".to_owned(),
                ))
            })
        }

        fn generate_title(
            &self,
            request: TitleRequest,
        ) -> SeamFuture<'_, Result<String, TitleError>> {
            let _ = request;
            Box::pin(async move { Ok("seam title".to_owned()) })
        }
    }

    async fn live_state(seam: Option<Arc<dyn AssistantModelSeam>>) -> Option<(AppState, String)> {
        let url = std::env::var("DATABASE_URL").ok()?;
        let cfg = pidash_db::config::DbConfig::new(url, 5).expect("db config");
        let pools = pidash_db::Pools::connect(&cfg, None)
            .await
            .expect("connect pools");
        let mut settings = pidash_db::config::Settings::test_defaults();
        // The operator switch stays in the profile verdict, so the tests
        // run with it on and read the seam's own values through.
        settings.managed_runner.enabled = true;
        let mut state = AppState::with_settings("test", settings).with_pools(pools);
        if let Some(seam) = seam {
            state = state.with_assistant_seam(seam);
        }
        let secret = state.settings().secret_key.clone();
        Some((state, secret))
    }

    /// Seed one active user; returns its id. Column list mirrors the
    /// contract harness `create_user` (the `handlers_members` precedent).
    async fn seed_user(pool: &sqlx::PgPool) -> uuid::Uuid {
        let id = uuid::Uuid::new_v4();
        let email = format!("seam-{}@example.com", id.simple());
        sqlx::query(
            "INSERT INTO users (id, password, username, email, first_name, last_name,
                avatar, date_joined, created_at, updated_at, last_location,
                created_location, is_superuser, is_managed, is_password_expired,
                is_active, is_staff, is_email_verified, is_password_autoset, token,
                user_timezone, last_login_ip, last_logout_ip, last_login_medium,
                last_login_uagent, is_bot, display_name, is_email_valid,
                is_password_reset_required)
             VALUES ($1, '!', $2, $2, 'Seam', 'User',
                '', now(), now(), now(), '',
                '', false, false, false,
                true, false, true, false, '',
                'UTC', '', '', '',
                '', false, 'Seam User', true,
                false)",
        )
        .bind(id)
        .bind(&email)
        .execute(pool)
        .await
        .expect("seed user");
        id
    }

    fn hex_encode(bytes: &[u8]) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut out = String::with_capacity(bytes.len() * 2);
        for b in bytes {
            out.push(HEX[(b >> 4) as usize] as char);
            out.push(HEX[(b & 0x0f) as usize] as char);
        }
        out
    }

    /// `user.get_session_auth_hash()` for the seeded `'!'` password field
    /// (the `resolve_actor` verification, recomputed, not copied).
    fn session_hash(secret_key: &str) -> String {
        use hmac::{Hmac, Mac};
        use sha2::{Digest, Sha256};
        let key = Sha256::digest(
            [
                crate::license::SESSION_AUTH_HASH_SALT.as_bytes(),
                secret_key.as_bytes(),
            ]
            .concat(),
        );
        let mut mac =
            Hmac::<Sha256>::new_from_slice(&key).expect("HMAC-SHA256 accepts any key length");
        mac.update(b"!");
        hex_encode(&mac.finalize().into_bytes())
    }

    fn desktop_session(user_id: uuid::Uuid, secret_key: &str) -> axum::Extension<SessionHandle> {
        let mut session = crate::middleware::RequestSession::empty();
        session.set(
            "_auth_user_id".to_owned(),
            Value::String(user_id.to_string()),
        );
        session.set(
            "_auth_user_backend".to_owned(),
            Value::String(crate::license::MODEL_BACKEND.to_owned()),
        );
        session.set(
            "_auth_user_hash".to_owned(),
            Value::String(session_hash(secret_key)),
        );
        session.set(
            DESKTOP_SESSION_KEY.to_owned(),
            Value::String(DESKTOP_CLIENT.to_owned()),
        );
        axum::Extension(SessionHandle::new(session))
    }

    async fn body_json(response: Response) -> (StatusCode, Value) {
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("body");
        let body: Value = serde_json::from_slice(&bytes).expect("json body");
        (status, body)
    }

    #[tokio::test]
    async fn profile_serves_the_installed_seam() {
        let Some((state, secret)) = live_state(Some(Arc::new(GatewaySeam))).await else {
            eprintln!("skipping live-db test: DATABASE_URL is not set");
            return;
        };
        let pool = state.pools().expect("pools").primary().clone();
        let user_id = seed_user(&pool).await;
        let extension = desktop_session(user_id, &secret);
        let (status, body) = body_json(get_profile(State(state), Some(extension)).await).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["lane"], "openhub");
        assert_eq!(body["base_url"], "https://gateway.example.com");
        assert_eq!(body["model"], "hub-default");
        assert_eq!(body["available"], true);
        assert_eq!(body["reason_code"], "");
    }

    #[tokio::test]
    async fn profile_serves_the_ce_default() {
        // No `with_assistant_seam`: the default CE seam answers a keyless
        // user exactly like the ported `agent_model_profile_for_user`.
        let Some((state, secret)) = live_state(None).await else {
            eprintln!("skipping live-db test: DATABASE_URL is not set");
            return;
        };
        let pool = state.pools().expect("pools").primary().clone();
        let user_id = seed_user(&pool).await;
        let extension = desktop_session(user_id, &secret);
        let (status, body) = body_json(get_profile(State(state), Some(extension)).await).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["lane"], "");
        assert_eq!(body["available"], false);
        assert_eq!(body["reason_code"], "llm_config_missing");
    }

    #[tokio::test]
    async fn token_mints_from_the_installed_seam() {
        let Some((state, secret)) = live_state(Some(Arc::new(GatewaySeam))).await else {
            eprintln!("skipping live-db test: DATABASE_URL is not set");
            return;
        };
        let pool = state.pools().expect("pools").primary().clone();
        let user_id = seed_user(&pool).await;
        let extension = desktop_session(user_id, &secret);
        let (status, body) = body_json(post_token(State(state), Some(extension)).await).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["token"], "gateway-token");
        assert_eq!(body["expires_at"], "2030-01-01T00:00:00Z");
    }

    #[tokio::test]
    async fn token_refuses_like_ce() {
        let Some((state, secret)) = live_state(None).await else {
            eprintln!("skipping live-db test: DATABASE_URL is not set");
            return;
        };
        let pool = state.pools().expect("pools").primary().clone();
        let user_id = seed_user(&pool).await;
        let extension = desktop_session(user_id, &secret);
        let (status, body) = body_json(post_token(State(state), Some(extension)).await).await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body["error"], "byok_not_supported_on_desktop");
        assert_eq!(
            body["detail"],
            "This build has no model lane the desktop agent can use."
        );
    }
}
