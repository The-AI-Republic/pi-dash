//! Desktop session handoff endpoints (desktop_unified_signin §5).
//!
//! Mirrors `runner/services/desktop_handoff.py` + `authentication/views/cli/handoff.py`:
//!
//! * `POST /api/v1/auth/desktop-handoff/` — the daemon asks for a single-use
//!   code with a workspace daemon credential and a `challenge`; the server
//!   stores it bound to the user, the machine and the challenge, and
//!   returns it with the account it belongs to.
//! * `POST /auth/desktop-handoff/redeem/` (the path the desktop app posts
//!   to, `authentication/urls.py`) and `POST
//!   /api/v1/auth/desktop-handoff/redeem/` (`api/urls/auth.py`) — the app
//!   redeems the code
//!   with its verifier (never a URL) and `expected_user_id`; on success the
//!   server creates the same kind of session a desktop browser sign-in
//!   creates (marked `client = "desktop"`). On any mismatch the code is
//!   consumed and no session is created.
//!
//! The verifier never leaves the app's process: issue takes only its hash
//! (the challenge), redeem recomputes the hash and compares. Neither the
//! code nor the verifier is logged; every issue and redeem is audited with
//! user, machine and outcome. The kill switch
//! (`DESKTOP_SESSION_HANDOFF_ENABLED`, default on) refuses both endpoints.

use axum::extract::{Extension, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Router;
use base64::Engine as _;
use chrono::{DateTime, Utc};
use sea_orm::EntityTrait;

use pidash_compat::response::json_response;
use pidash_services::auth_session::shapes::base_host;
use pidash_services::auth_session::shapes::HostSettings;
use pidash_services::domain::desktop_handoff;
use pidash_services::domain::identity;

use crate::assistant::common::{
    body_get, parse_body, pool_of, request_content_type, Failure, ParsedBody,
};
use crate::middleware::SessionHandle;
use crate::state::AppState;

use super::device_session::{authenticate, Caller};

pub const HANDOFF_PATH: &str = "/api/v1/auth/desktop-handoff/";
pub const HANDOFF_REDEEM_PATH: &str = "/api/v1/auth/desktop-handoff/redeem/";
/// The redeem path the desktop app posts to (`authentication/urls.py`,
/// design §5 step 5; the cloud serves it under `/api/auth/`).
pub const HANDOFF_AUTH_REDEEM_PATH: &str = "/auth/desktop-handoff/redeem/";

const CSRF_COOKIE_NAME: &str = "csrftoken";
const CSRF_COOKIE_MAX_AGE: i64 = 31_449_600;
const LOGIN_BACKEND: &str = "django.contrib.auth.backends.ModelBackend";
const DESKTOP_SESSION_KEY: &str = "pidash_client";
const DESKTOP_CLIENT_VALUE: &str = "desktop";

/// An owned POST path: the write serves from Rust, every other method
/// falls through to Django (which answers 405 / OPTIONS metadata).
fn owned_post(
    handler: axum::routing::MethodRouter<AppState>,
) -> axum::routing::MethodRouter<AppState> {
    handler
        .get(crate::edge::proxy)
        .put(crate::edge::proxy)
        .patch(crate::edge::proxy)
        .delete(crate::edge::proxy)
        .head(crate::edge::proxy)
        .options(crate::edge::proxy)
}

/// The `/api/v1/` pair (issue + redeem), mounted on the ApiV1 group.
pub fn routes() -> Router<AppState> {
    use axum::routing::post;
    Router::new()
        .route(HANDOFF_PATH, owned_post(post(post_issue)))
        .route(HANDOFF_REDEEM_PATH, owned_post(post(post_redeem)))
}

/// The `/auth/` redeem the desktop app posts to, mounted on the Auth group.
pub fn auth_routes() -> Router<AppState> {
    use axum::routing::post;
    Router::new().route(HANDOFF_AUTH_REDEEM_PATH, owned_post(post(post_redeem)))
}

fn host_settings(settings: &pidash_db::config::Settings) -> HostSettings<'_> {
    let urls = &settings.urls;
    HostSettings {
        web_url: urls.web_url.as_deref(),
        app_base_url: urls.app_base_url.as_deref(),
        admin_base_url: urls.admin_base_url.as_deref(),
        space_base_url: urls.space_base_url.as_deref(),
        admin_base_path: Some(urls.admin_base_path.as_str()),
        space_base_path: Some(urls.space_base_path.as_str()),
    }
}

fn refusal_body(error: &desktop_handoff::HandoffError) -> String {
    serde_json::to_string(&serde_json::json!({ "error": error.code }))
        .expect("refusal body serializes")
}

fn parse_uuid_field(raw: &str) -> Option<uuid::Uuid> {
    uuid::Uuid::parse_str(raw.trim()).ok()
}

/// POST the daemon asks for a code (authenticated with one workspace daemon
/// credential, `X-Api-Key`).
async fn post_issue(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(failure) => return failure.into_response(),
    };
    let caller = match authenticate(&pool, state.settings().secret_key.as_bytes(), &headers).await {
        Ok(caller) => caller,
        Err(denial) => return denial,
    };
    let Caller::Machine { user_id, token_id } = caller else {
        // `machine_token_required` (`handoff.py`): issue needs a daemon
        // credential, not an API token.
        return json_response(
            StatusCode::UNAUTHORIZED,
            serde_json::to_string(&serde_json::json!({
                "error": "machine_token_required",
                "error_description": "Issue a handoff with a daemon credential.",
            }))
            .expect("refusal body serializes"),
        );
    };
    let fields = match parse_body(&body, request_content_type(&headers)) {
        Ok(ParsedBody::Object(fields)) => fields,
        Ok(ParsedBody::Scalar(_) | ParsedBody::Null) => {
            return Failure::server_error().into_response()
        }
        Err(failure) => return failure.into_response(),
    };
    // Body reading goes through the shared assistant helpers; pull the
    // string field the same way `or_blank` would.
    let raw = match body_get(&fields, "challenge") {
        Some(crate::assistant::common::BodyField::Json(serde_json::Value::String(s))) => s.clone(),
        _ => String::new(),
    };
    let challenge = raw.trim().to_owned();
    if challenge.is_empty() || challenge.len() > 128 {
        return json_response(
            StatusCode::BAD_REQUEST,
            serde_json::to_string(&serde_json::json!({
                "error": "invalid_request",
                "error_description": "challenge is required.",
            }))
            .expect("refusal body serializes"),
        );
    }
    // The machine: the token's bound computer.
    let machine_id = {
        use pidash_db::entity::machine_token;
        use sea_orm::EntityTrait;
        let db = pidash_db::pool::sea_orm_connection(&pool);
        match machine_token::Entity::find_by_id(token_id).one(&db).await {
            Ok(Some(token)) => token.dev_machine_id,
            Ok(None) => return Failure::server_error().into_response(),
            Err(_) => return Failure::server_error().into_response(),
        }
    };
    let Some(machine_id) = machine_id else {
        return json_response(
            StatusCode::UNAUTHORIZED,
            serde_json::to_string(&serde_json::json!({
                "error": "machine_token_required",
                "error_description": "Issue a handoff with a daemon credential.",
            }))
            .expect("refusal body serializes"),
        );
    };
    let settings = state.settings().clone();
    let enabled = settings.desktop_session_handoff_enabled;
    let db = pidash_db::pool::sea_orm_connection(&pool);
    match desktop_handoff::issue_handoff(
        &db,
        settings.secret_key.as_bytes(),
        user_id,
        machine_id,
        token_id,
        &challenge,
        enabled,
        Utc::now(),
    )
    .await
    {
        Ok(issued) => json_response(
            StatusCode::OK,
            serde_json::to_string(&serde_json::json!({
                "code": issued.raw,
                "user_id": issued.user_id.to_string(),
                "email": issued.email,
                "expires_in": desktop_handoff::HANDOFF_TTL_SECS,
            }))
            .expect("issue body serializes"),
        ),
        Err(desktop_handoff::HandoffDbError::Refusal(error)) => json_response(
            StatusCode::from_u16(error.http_status).unwrap_or(StatusCode::BAD_REQUEST),
            refusal_body(&error),
        ),
        Err(_) => Failure::server_error().into_response(),
    }
}

/// Decode a verifier the app sends as base64url (padding optional).
fn decode_verifier(raw: &str) -> Option<Vec<u8>> {
    let mut padded = raw.to_owned();
    while !padded.len().is_multiple_of(4) {
        padded.push('=');
    }
    base64::engine::general_purpose::URL_SAFE
        .decode(padded.as_bytes())
        .or_else(|_| base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(raw.as_bytes()))
        .ok()
}

/// POST the app redeems its code with the verifier (allow-any): four
/// checks before any session (design §5 step 6).
async fn post_redeem(
    State(state): State<AppState>,
    extension: Option<Extension<SessionHandle>>,
    headers: HeaderMap,
    peer: axum::extract::ConnectInfo<std::net::SocketAddr>,
    body: axum::body::Bytes,
) -> Response {
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(failure) => return failure.into_response(),
    };
    let fields = match parse_body(&body, request_content_type(&headers)) {
        Ok(ParsedBody::Object(fields)) => fields,
        Ok(ParsedBody::Scalar(_) | ParsedBody::Null) => {
            return Failure::server_error().into_response()
        }
        Err(failure) => return failure.into_response(),
    };
    let field_str = |name: &str| -> String {
        match body_get(&fields, name) {
            Some(crate::assistant::common::BodyField::Json(serde_json::Value::String(s))) => {
                s.clone()
            }
            _ => String::new(),
        }
    };
    let code = field_str("code").trim().to_owned();
    let verifier_raw = field_str("verifier").trim().to_owned();
    let expected_user_id_raw = field_str("expected_user_id").trim().to_owned();
    let Some(verifier) = decode_verifier(&verifier_raw) else {
        // A missing or non-base64 verifier is a refused redemption, never
        // a session.
        return json_response(
            StatusCode::BAD_REQUEST,
            serde_json::to_string(&serde_json::json!({
                "error": "invalid_request",
                "error_description": "code, verifier and expected_user_id are required.",
            }))
            .expect("refusal body serializes"),
        );
    };
    let Some(expected_user_id) = parse_uuid_field(&expected_user_id_raw) else {
        return json_response(
            StatusCode::BAD_REQUEST,
            serde_json::to_string(&serde_json::json!({
                "error": "invalid_request",
                "error_description": "code, verifier and expected_user_id are required.",
            }))
            .expect("refusal body serializes"),
        );
    };
    if code.is_empty() || verifier.is_empty() || verifier.len() > 64 {
        return json_response(
            StatusCode::BAD_REQUEST,
            serde_json::to_string(&serde_json::json!({
                "error": "invalid_request",
                "error_description": "code, verifier and expected_user_id are required.",
            }))
            .expect("refusal body serializes"),
        );
    }
    let settings = state.settings().clone();
    if !settings.desktop_session_handoff_enabled {
        return json_response(
            StatusCode::GONE,
            serde_json::to_string(&serde_json::json!({
                "error": desktop_handoff::HANDOFF_DISABLED,
                "error_description": "The handoff is unavailable right now.",
            }))
            .expect("refusal body serializes"),
        );
    }
    let now = Utc::now();
    let mut tx = match pool.begin().await {
        Ok(tx) => tx,
        Err(_) => return Failure::server_error().into_response(),
    };
    let redeemed = {
        let tx_conn = pidash_db::orm_tx::TxConn::new(&mut tx);
        desktop_handoff::redeem_handoff(
            &tx_conn,
            settings.secret_key.as_bytes(),
            &code,
            &verifier,
            expected_user_id,
            now,
        )
        .await
    };
    let redeemed = match redeemed {
        Ok(redeemed) => redeemed,
        Err(desktop_handoff::HandoffDbError::Refusal(error)) => {
            // Commit so the failure consumes the code (single-use even on
            // mismatch, design §5 step 6), then refuse.
            let _ = tx.commit().await;
            return json_response(
                StatusCode::from_u16(error.http_status).unwrap_or(StatusCode::BAD_REQUEST),
                refusal_body(&error),
            );
        }
        Err(_) => return Failure::server_error().into_response(),
    };
    // Same session a desktop browser sign-in creates, marked desktop.
    let user = {
        use pidash_db::entity::users;
        use sea_orm::EntityTrait;
        let db = pidash_db::pool::sea_orm_connection(&pool);
        match users::Entity::find_by_id(redeemed.user_id).one(&db).await {
            Ok(Some(user)) => user,
            Ok(None) => {
                let _ = tx.commit().await;
                return Failure::server_error().into_response();
            }
            Err(_) => return Failure::server_error().into_response(),
        }
    };
    let csrf = match stamp_session(&pool, &settings, extension, &user, &headers, peer.0, now).await
    {
        Ok(csrf) => csrf,
        Err(()) => {
            let _ = tx.commit().await;
            return Failure::server_error().into_response();
        }
    };
    if tx.commit().await.is_err() {
        return Failure::server_error().into_response();
    }
    let mut response = json_response(
        StatusCode::OK,
        serde_json::to_string(&serde_json::json!({
            "user_id": redeemed.user_id.to_string(),
            "email": redeemed.email,
        }))
        .expect("redeem body serializes"),
    );
    if let Some(cookie) = csrf {
        if let Ok(value) = axum::http::header::HeaderValue::from_str(&cookie) {
            response
                .headers_mut()
                .append(axum::http::header::SET_COOKIE, value);
        }
    }
    response
}

/// `user_login()` + the desktop mark: `_auth_user_id` /
/// `_auth_user_backend` / `_auth_user_hash` plus `device_info`, the session
/// marked as a desktop client, and a fresh CSRF secret. The tower layer
/// persists the mutated session and sets the `session-id` cookie.
async fn stamp_session(
    pool: &sqlx::PgPool,
    settings: &pidash_db::config::Settings,
    extension: Option<Extension<SessionHandle>>,
    user: &pidash_db::entity::users::Model,
    headers: &HeaderMap,
    peer: std::net::SocketAddr,
    now: DateTime<Utc>,
) -> Result<Option<String>, ()> {
    let Some(handle) = extension.map(|e| e.0) else {
        return Err(());
    };
    let session_hash =
        pidash_compat::http::session_auth_hash(&user.password, settings.secret_key.as_bytes());
    let user_id = user.id.to_string();
    let current: Option<String> = handle
        .lock()
        .get("_auth_user_id")
        .and_then(|v| v.as_str())
        .map(str::to_owned);
    if current.as_deref() != Some(user_id.as_str()) {
        let old: Option<String> = handle.lock().key.clone();
        handle.lock().clear();
        if let Some(old) = old {
            use pidash_db::entity::sessions;
            let db = pidash_db::pool::sea_orm_connection(pool);
            let deleted = sessions::Entity::delete_by_id(old).exec(&db).await;
            let _ = deleted;
            if deleted.is_err() {
                return Err(());
            }
        }
    }
    let hosts = host_settings(settings);
    let domain = base_host(&hosts, false, false, false);
    let user_agent = headers
        .get(axum::http::header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_owned();
    let ip = Some(peer.to_string());
    {
        let mut session = handle.lock();
        session.set(
            "_auth_user_id".to_owned(),
            serde_json::Value::String(user_id),
        );
        session.set(
            "_auth_user_backend".to_owned(),
            serde_json::Value::String(LOGIN_BACKEND.to_owned()),
        );
        session.set(
            "_auth_user_hash".to_owned(),
            serde_json::Value::String(session_hash),
        );
        session.set(
            DESKTOP_SESSION_KEY.to_owned(),
            serde_json::Value::String(DESKTOP_CLIENT_VALUE.to_owned()),
        );
        session.set(
            "device_info".to_owned(),
            serde_json::json!({
                "user_agent": user_agent,
                "ip_address": ip,
                "domain": domain,
            }),
        );
    }
    if identity::stamp_last_login(&pidash_db::pool::sea_orm_connection(pool), user.id, now)
        .await
        .is_err()
    {
        return Err(());
    }
    let secret = pidash_auth::csrf::new_secret();
    let set_cookie = pidash_auth::session::render_set_cookie(&pidash_auth::session::SetCookie {
        name: CSRF_COOKIE_NAME.to_owned(),
        value: secret,
        expires: Some(pidash_auth::session::http_date(
            now.timestamp() + CSRF_COOKIE_MAX_AGE,
        )),
        max_age: Some(CSRF_COOKIE_MAX_AGE),
        domain: settings.session.cookie_domain.clone(),
        path: "/".to_owned(),
        secure: settings.session.cookie_secure,
        httponly: true,
        samesite: "Lax".to_owned(),
    });
    Ok(Some(set_cookie))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owned_paths_match_django_urls() {
        // `api/urls/auth.py`: issue + redeem under /api/v1/ (the daemon
        // issues on HANDOFF_PATH, runner/src/daemon/supervisor.rs).
        assert_eq!(HANDOFF_PATH, "/api/v1/auth/desktop-handoff/");
        assert_eq!(HANDOFF_REDEEM_PATH, "/api/v1/auth/desktop-handoff/redeem/");
        // `authentication/urls.py`: the redeem the desktop app posts to
        // (desktop/src-tauri/src/handoff.rs joins it onto the API base).
        assert_eq!(HANDOFF_AUTH_REDEEM_PATH, "/auth/desktop-handoff/redeem/");
    }

    #[test]
    fn both_routers_build() {
        // Registration is proven end to end by the inventory pin test
        // (`bin/pidash-api/src/inventory.rs`: the three POST rows are
        // owned); here, each router builds without a path conflict.
        let _ = routes();
        let _ = auth_routes();
    }

    #[test]
    fn verifier_decoding_accepts_padding_and_no_padding() {
        let verifier: Vec<u8> = (0u8..32).collect();
        let sent = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&verifier);
        let decoded = decode_verifier(&sent).expect("the app's base64url decodes");
        assert_eq!(
            decoded, verifier,
            "round-trips through the base64url the app sends"
        );
        // Padded input is accepted too.
        let mut padded = sent.clone();
        while !padded.len().is_multiple_of(4) {
            padded.push('=');
        }
        assert_eq!(decode_verifier(&padded), Some(verifier.clone()));
        // Garbage is refused, never a session.
        assert_eq!(decode_verifier("not-base64!"), None);
        assert_eq!(decode_verifier(""), Some(Vec::new()));
    }

    #[test]
    fn user_id_parsing_rejects_garbage() {
        assert!(parse_uuid_field("not-a-uuid").is_none());
        assert!(parse_uuid_field("  ").is_none());
        assert!(parse_uuid_field("550e8400-e29b-41d4-a716-446655440000").is_some());
    }
}
