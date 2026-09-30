#![forbid(unsafe_code)]
// Fallible helpers bottom out in the uncaught-exception 500, so they carry
// `Response` as the error (same precedent as the assistant handlers).
#![allow(clippy::result_large_err)]

//! Magic-link handlers (stage 5, PIDASHCONV-431).
//!
//! Python: `apps/api/pi_dash/authentication/views/app/magic.py`
//! (`MagicGenerateEndpoint`, `MagicSignInEndpoint`, `MagicSignUpEndpoint`),
//! `views/space/magic.py` (the space twins), `urls.py` (`magic-generate/`,
//! `magic-sign-in/`, `magic-sign-up/` + `spaces/` variants).
//!
//! Six owned routes, POST only; every other method proxies to Django:
//!
//! * `POST /auth/magic-generate/` and `POST /auth/spaces/magic-generate/`
//!   (DRF `APIView`, JSON `{"email"}` → 200 `{"key"}`).
//! * `POST /auth/magic-sign-in/`, `/auth/magic-sign-up/` and the `spaces/`
//!   twins (plain Django `View`, form `email`/`code`/`next_path` → 302).
//!
//! The provider decisions live in the services kernel
//! (`pidash_services::auth_session::magic`, PIDASHCONV-431) over the redis
//! builders in `pidash_services::auth_session::tasks` (PIDASHCONV-405);
//! error codes, redirect bytes, `base_host`, and the redirection-path
//! selector in `pidash_services::auth_session::shapes` (PIDASHCONV-340);
//! the throttle math in `...::guards` (PIDASHCONV-393); SQL shapes in
//! `...::queries` (PIDASHCONV-382). This module is the wiring: HTTP parsing,
//! Redis/DB I/O, session issuance, and publishing.
//!
//! Fixture oracle: `rust-api/fixtures/auth_session/FX-AUTH-08
//! .handlers_magic_password.json` `magic_generate` / `magic_signin` /
//! `magic_signup` (+ `handler_quirks[0]`), and FX-AUTH-09 `magic_link` for
//! the publish. The `#[cfg(test)]` suite replays the pure vectors
//! (redirect bytes, throttle keys, form parsing); the live vectors run in
//! `rust-api/contract-tests/auth_magic/`.
//!
//! # Ported quirks (translate, don't redesign)
//!
//! * QUIRK-generate-500 (`magic.py:45-51`, FX-AUTH-08 `handler_quirks[0]`):
//!   `validate_email` raising `ValidationError` is *not* caught (the
//!   `except` covers `AuthenticationException` only), so an empty or
//!   malformed email answers 500, not 400. Non-string payloads
//!   (`None.strip()` / `(123).strip()`) fail the same way.
//! * QUIRK-strip-noop + QUIRK-attempt-gate + QUIRK-signup-flag: see the
//!   services kernel docs; each is kept.
//! * QUIRK-login-ip (`utils/ip_address.py:8-14`): the first
//!   `X-Forwarded-For` entry is taken unstripped, else `REMOTE_ADDR` (which
//!   may be absent → NULL stamp columns → 500 on NOT NULL, same as Django's
//!   `IntegrityError`).
//! * QUIRK-no-transaction: no `transaction.atomic` anywhere, so the
//!   project-invite `IntegrityError` (missing `project_id`, kept) still
//!   500s *after* the workspace-member writes commit.
//! * QUIRK-csrf-rotate: `login()` rotates the CSRF secret, so success sets
//!   a fresh masked `csrftoken` cookie (Django defaults: 1-year age,
//!   `/`, `Lax`; secure/domain/httponly from settings).

use axum::body::Body;
use axum::extract::{ConnectInfo, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::Router;
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

use pidash_services::auth_session::guards::{
    allow_request, parse_rate, throttle_cache_key, throttle_denied_json,
    AUTHENTICATION_THROTTLE_RATE, AUTHENTICATION_THROTTLE_SCOPE, DEFAULT_ANON_RATE,
    DEFAULT_ANON_SCOPE,
};
use pidash_services::auth_session::magic::{
    exhausted_error, gate_error, init_gate, initiate_decision, parse_stored, payload_email,
    strip_magic_prefix, token_from_u32, verify_decision, verify_error, InitGate, InitiateOutcome,
    VerifyOutcome, TOKEN_REJECTION_LIMIT,
};

/// Unwrap a `Result`, answering the uncaught-exception 500 on any error
/// (every fallible I/O on these paths propagates in Python).
macro_rules! or_500 {
    ($expr:expr) => {
        match $expr {
            Ok(value) => value,
            Err(_) => return server_error(),
        }
    };
}
use pidash_db::config::encryption::Keyring;
use pidash_services::auth_session::queries::{
    user_by_email_sql, user_email_exists_sql, USER_TABLE,
};
use pidash_services::auth_session::shapes::{
    allowed_hosts_for, base_host, error_code, error_dict_json, error_pairs, get_safe_redirect_url,
    redirection_path_str, select_redirection_path, url_has_allowed_host_and_scheme,
    validate_next_path, HostSettings, ParamValue,
};
use pidash_services::auth_session::tasks::{
    empty_kwargs, magic_redis_key, magic_redis_value_first, magic_redis_value_retry,
    MAGIC_REDIS_EXPIRY_SECS,
};

use crate::license::handlers_auth_forms::{email_is_valid, LOGIN_BACKEND};
use crate::middleware::SessionHandle;
use crate::state::AppState;

/// `POST /auth/magic-generate/`.
pub const APP_GENERATE_PATH: &str = "/auth/magic-generate/";
/// `POST /auth/magic-sign-in/`.
pub const APP_SIGN_IN_PATH: &str = "/auth/magic-sign-in/";
/// `POST /auth/magic-sign-up/`.
pub const APP_SIGN_UP_PATH: &str = "/auth/magic-sign-up/";
/// `POST /auth/spaces/magic-generate/`.
pub const SPACE_GENERATE_PATH: &str = "/auth/spaces/magic-generate/";
/// `POST /auth/spaces/magic-sign-in/`.
pub const SPACE_SIGN_IN_PATH: &str = "/auth/spaces/magic-sign-in/";
/// `POST /auth/spaces/magic-sign-up/`.
pub const SPACE_SIGN_UP_PATH: &str = "/auth/spaces/magic-sign-up/";

/// Register the six owned magic routes. Sibling D-16 handler issues (422
/// email, 434 password/CSRF) extend `auth_session::routes` with their own
/// routers; merges keep both sides.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route(APP_GENERATE_PATH, post(generate_app))
        .route(APP_SIGN_IN_PATH, post(sign_in_app))
        .route(APP_SIGN_UP_PATH, post(sign_up_app))
        .route(SPACE_GENERATE_PATH, post(generate_space))
        .route(SPACE_SIGN_IN_PATH, post(sign_in_space))
        .route(SPACE_SIGN_UP_PATH, post(sign_up_space))
}

// ---------------------------------------------------------------------------
// Responses
// ---------------------------------------------------------------------------

const JSON_CONTENT_TYPE: &str = "application/json";
const REDIRECT_CONTENT_TYPE: &str = "text/html; charset=utf-8";

/// `Response({"key": ...}, 200)`: compact JSON, DRF renderer bytes.
fn ok_key(key: &str) -> Response {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, JSON_CONTENT_TYPE)],
        serde_json::json!({"key": key}).to_string(),
    )
        .into_response()
}

/// `Response(exc.get_error_dict(), 400)`: byte-exact error dict.
fn json_400(code: i32, message: &str, payload: &[(&str, ParamValue)]) -> Response {
    (
        StatusCode::BAD_REQUEST,
        [(header::CONTENT_TYPE, JSON_CONTENT_TYPE)],
        error_dict_json(&error_pairs(code, message, payload)),
    )
        .into_response()
}

/// `HttpResponseRedirect(url)`: 302 with `Location`, empty body.
fn redirect_302(location: String) -> Response {
    Response::builder()
        .status(StatusCode::FOUND)
        .header(header::LOCATION, location)
        .header(header::CONTENT_TYPE, REDIRECT_CONTENT_TYPE)
        .body(Body::empty())
        .expect("static 302 response")
}

/// Uncaught-exception 500 (`ValidationError`, redis/SQL failures outside
/// the bramched handling): empty body, like the F-08 session layer's
/// `server_error`.
fn server_error() -> Response {
    Response::builder()
        .status(StatusCode::INTERNAL_SERVER_ERROR)
        .body(Body::empty())
        .expect("static 500 response")
}

/// Throttle denial: the `auth_exception_handler` rewrite of DRF's
/// `Throttled` (`exception.py:26-31`), 429 + the 5900 dict.
fn throttled() -> Response {
    (
        StatusCode::TOO_MANY_REQUESTS,
        [(header::CONTENT_TYPE, JSON_CONTENT_TYPE)],
        throttle_denied_json(),
    )
        .into_response()
}

/// Malformed JSON body on a JSON post: DRF `ParseError`
/// (`{"detail": "JSON parse error - ..."}`).
fn malformed_json(error: serde_json::Error) -> Response {
    (
        StatusCode::BAD_REQUEST,
        [(header::CONTENT_TYPE, JSON_CONTENT_TYPE)],
        serde_json::json!({"detail": format!("JSON parse error - {error}")}).to_string(),
    )
        .into_response()
}

// ---------------------------------------------------------------------------
// Request parsing
// ---------------------------------------------------------------------------

/// `request.data.get("email", "")` for the generate views, which accept
/// both JSON and form posts (`request.data` covers both). Returns the
/// normalised email, or `None` when the payload cannot supply a string —
/// every `None` case 500s in Python (`AttributeError` on `.strip()` or the
/// uncaught `ValidationError`).
fn parse_generate_email(body: &[u8], is_json: bool) -> Option<String> {
    if is_json {
        if body.is_empty() {
            return None;
        }
        let value: serde_json::Value = serde_json::from_slice(body).ok()?;
        let email = value.as_object()?.get("email")?.as_str()?;
        Some(normalise_email(email))
    } else {
        let pairs: Vec<(String, String)> = serde_urlencoded::from_bytes(body).unwrap_or_default();
        let email = pairs.iter().rev().find(|(k, _)| k == "email")?;
        Some(normalise_email(&email.1))
    }
}

/// `request.data.get("email", "").strip().lower()`: Python `strip` trims
/// Unicode whitespace and `lower` is the simple case mapping — `trim` +
/// `to_lowercase` (same as the license kernel's `normalize_email`).
fn normalise_email(raw: &str) -> String {
    raw.trim().to_lowercase()
}

/// One form-POST field: `request.POST.get(key, "")`, last value wins (like
/// Django's `QueryDict.get`).
fn form_field(pairs: &[(String, String)], key: &str) -> String {
    pairs
        .iter()
        .rev()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.clone())
        .unwrap_or_default()
}

/// The sign-in/up form triple: `code`/`email` stripped (+ lowered for
/// email), `next_path` raw-or-absent (`request.POST.get("next_path")` is
/// `None` when missing — Python passes that `None` straight into
/// `get_safe_redirect_url` / `validate_next_path`).
#[derive(Debug, Clone, PartialEq, Eq)]
struct SignForm {
    code: String,
    email: String,
    next_path: Option<String>,
}

fn parse_sign_form(body: &[u8]) -> SignForm {
    let pairs: Vec<(String, String)> = serde_urlencoded::from_bytes(body).unwrap_or_default();
    SignForm {
        code: form_field(&pairs, "code").trim().to_owned(),
        email: normalise_email(&form_field(&pairs, "email")),
        next_path: pairs
            .iter()
            .rev()
            .find(|(k, _)| k == "next_path")
            .map(|(_, v)| v.clone()),
    }
}

// ---------------------------------------------------------------------------
// Redis (the `redis_instance()` side of the provider)
// ---------------------------------------------------------------------------

/// Minimal command surface the magic closure needs: `GET`, `SET .. EX`,
/// `DEL`, and `KEYS`-then-`DEL` (the cache-invalidate shape). The shared
/// foundation handle (`RedisHandle`) exposes only `SET .. EX` / `GET` /
/// `SUBSCRIBE`, so this module holds its own short-lived multiplexed
/// client off the same `REDIS_URL` — `None` when the URL is unset, in
/// which case every magic flow 500s exactly like a dead `redis_instance()`.
#[derive(Debug, Clone)]
struct Cache {
    client: redis::Client,
}

impl Cache {
    fn from_state(state: &AppState) -> Option<Self> {
        let url = state
            .settings()
            .redis
            .url
            .as_deref()
            .filter(|u| !u.is_empty())?;
        redis::Client::open(url).ok().map(|client| Self { client })
    }

    async fn get(&self, key: &str) -> Result<Option<String>, redis::RedisError> {
        let mut conn = self.client.get_multiplexed_async_connection().await?;
        redis::AsyncCommands::get(&mut conn, key).await
    }

    async fn set_ex(
        &self,
        key: &str,
        value: &str,
        expiry_secs: u64,
    ) -> Result<(), redis::RedisError> {
        let mut conn = self.client.get_multiplexed_async_connection().await?;
        redis::AsyncCommands::set_ex::<_, _, ()>(&mut conn, key, value, expiry_secs).await
    }

    async fn del(&self, key: &str) -> Result<(), redis::RedisError> {
        let mut conn = self.client.get_multiplexed_async_connection().await?;
        redis::AsyncCommands::del::<_, ()>(&mut conn, key).await
    }

    /// `cache.keys(pattern)` + `delete_many`: Django's `cache.keys` is the
    /// Redis `KEYS` command (django-redis), so the pattern match is one
    /// `KEYS` here too — never a scan-then-filter subset.
    async fn del_pattern(&self, pattern: &str) -> Result<(), redis::RedisError> {
        let mut conn = self.client.get_multiplexed_async_connection().await?;
        let keys: Vec<String> = redis::AsyncCommands::keys(&mut conn, pattern).await?;
        if !keys.is_empty() {
            redis::AsyncCommands::del::<_, ()>(&mut conn, keys).await?;
        }
        Ok(())
    }
}

/// DRF `AnonRateThrottle` ident (`throttling.py` `get_ident`): the
/// `X-Forwarded-For` value with all whitespace stripped when present, else
/// `REMOTE_ADDR` (which may itself be absent → the `"None"` ident Python's
/// `%`-formatting produces).
fn throttle_ident(headers: &axum::http::HeaderMap, peer: Option<String>) -> String {
    if let Some(xff) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()) {
        if !xff.is_empty() {
            return xff.split_whitespace().collect();
        }
    }
    peer.unwrap_or_else(|| "None".to_owned())
}

/// Evaluate the generate throttle *before* the handler body (DRF
/// `initial()` order: `check_throttles` precedes `post()`). App uses the
/// `AuthenticationThrottle` scope, space the default `Anon` scope (both
/// 30/minute). Histories are compact-JSON float arrays; a miss, an
/// unreadable value, or a failed re-cache fails open to allow (the merged
/// assistant-throttle precedent — cutover traffic owns these keys, and a
/// blip must never 500 an auth endpoint).
async fn check_generate_throttle(
    cache: Option<&Cache>,
    headers: &axum::http::HeaderMap,
    peer: Option<String>,
    app: bool,
) -> bool {
    let (rate, scope) = if app {
        (AUTHENTICATION_THROTTLE_RATE, AUTHENTICATION_THROTTLE_SCOPE)
    } else {
        (DEFAULT_ANON_RATE, DEFAULT_ANON_SCOPE)
    };
    let (num_requests, duration_secs) = match parse_rate(Some(rate)) {
        Some(parsed) => parsed,
        None => return true,
    };
    let Some(cache) = cache else { return true };
    let key = throttle_cache_key(scope, &throttle_ident(headers, peer));
    let now = now_secs_f64();
    let history: Vec<f64> = match cache.get(&key).await {
        Ok(Some(raw)) => serde_json::from_str(&raw).unwrap_or_default(),
        _ => Vec::new(),
    };
    let decision = allow_request(&history, num_requests, duration_secs, now);
    if !decision.allowed {
        return false;
    }
    if let Err(error) = cache
        .set_ex(
            &key,
            &serde_json::to_string(&decision.history).expect("float vec serializes"),
            duration_secs,
        )
        .await
    {
        tracing::debug!(%error, key = key.as_str(), "magic.throttle: re-cache failed; allowance stands");
    }
    true
}

fn now_secs_f64() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// Whether the request is already authenticated as an existing active
/// user: `AnonRateThrottle.get_cache_key` returns `None` then
/// (`throttling.py:173-179`), so the generate throttle does not apply.
/// `AuthenticationMiddleware.get_user` resolves `_auth_user_id` to the row
/// with no hash check and no soft-delete filter (`UserManager` is plain);
/// anything else counts as anonymous and stays throttled.
async fn generate_session_user_active(
    pool: &sqlx::PgPool,
    extension: &Option<axum::Extension<SessionHandle>>,
) -> bool {
    let uid: Option<String> = extension.as_ref().and_then(|e| {
        e.0.lock()
            .get("_auth_user_id")
            .and_then(|v| v.as_str())
            .map(str::to_owned)
    });
    let Some(uid) = uid else { return false };
    let Ok(id) = uuid::Uuid::parse_str(&uid) else {
        return false;
    };
    matches!(
        sqlx::query_as::<_, (bool,)>(
            "SELECT \"is_active\" FROM \"users\" WHERE \"users\".\"id\" = $1"
        )
        .bind(id)
        .fetch_optional(pool)
        .await,
        Ok(Some((true,)))
    )
}

// ---------------------------------------------------------------------------
// Instance gate + configuration values
// ---------------------------------------------------------------------------

/// `Instance.objects.first()` + `is_setup_done` gate
/// (`views/app/magic.py:40-46`): `None` or not-setup-done answers
/// `INSTANCE_NOT_CONFIGURED`. The projection carries the gate column of
/// [`pidash_services::auth_session::queries::instance_first_sql`].
async fn instance_setup_done(pool: &sqlx::PgPool) -> Result<bool, sqlx::Error> {
    let row: Option<(bool,)> = sqlx::query_as(
        "SELECT \"instances\".\"is_setup_done\" FROM \"instances\" \
         WHERE \"instances\".\"deleted_at\" IS NULL ORDER BY \"instances\".\"created_at\" DESC LIMIT 1",
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|(done,)| done).unwrap_or(false))
}

/// One `instance_configurations` read with the caller's env default — the
/// `get_configuration_value` shape for a db-sourced key
/// (`license/utils/instance_value.py:28-54`): the stored row wins when
/// present (decrypted when `is_encrypted`), otherwise the default (the
/// `os.environ.get(...)` the view passes in). Soft-deleted rows are
/// invisible (the default manager).
async fn config_value(
    pool: &sqlx::PgPool,
    secret_key: &str,
    key: &str,
    env_default: Option<String>,
) -> Result<Option<String>, sqlx::Error> {
    let row: Option<(Option<String>, bool)> = sqlx::query_as(
        "SELECT value, is_encrypted FROM instance_configurations \
         WHERE key = $1 AND deleted_at IS NULL",
    )
    .bind(key)
    .fetch_optional(pool)
    .await?;
    match row {
        Some((Some(value), true)) => Ok(Some(Keyring::from_secret(secret_key).decrypt(&value))),
        Some((Some(value), false)) => Ok(Some(value)),
        Some((None, _)) => Ok(env_default),
        None => Ok(env_default),
    }
}

fn env_default(name: &str, fallback: Option<&str>) -> Option<String> {
    match std::env::var(name) {
        Ok(value) => Some(value),
        Err(_) => fallback.map(str::to_owned),
    }
}

/// Resolve the two `MagicCodeProvider.__init__` inputs: `EMAIL_HOST`
/// (caller default `os.environ.get("EMAIL_HOST")`) and
/// `ENABLE_MAGIC_LINK_LOGIN` (caller default
/// `os.environ.get("ENABLE_MAGIC_LINK_LOGIN", "1")`).
async fn provider_config(
    pool: &sqlx::PgPool,
    secret_key: &str,
) -> Result<(Option<String>, String), sqlx::Error> {
    let email_host = config_value(
        pool,
        secret_key,
        "EMAIL_HOST",
        env_default("EMAIL_HOST", None),
    )
    .await?;
    let magic_enabled = config_value(
        pool,
        secret_key,
        "ENABLE_MAGIC_LINK_LOGIN",
        env_default("ENABLE_MAGIC_LINK_LOGIN", Some("1")),
    )
    .await?
    .unwrap_or_else(|| "1".to_owned());
    Ok((email_host, magic_enabled))
}

// ---------------------------------------------------------------------------
// Hosts + error redirects
// ---------------------------------------------------------------------------

/// `HostSettings` over the resolved F-03 URLs (`WEB_URL`, `APP_*`,
/// `ADMIN_*`, `SPACE_*`).
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

/// Error redirect: `HttpResponseRedirect(get_safe_redirect_url(base,
/// next_path, exc.get_error_dict()))` — the error dict in insertion order
/// (`error_code`, `error_message`, then the `email` payload).
fn redirect_error(
    base: &str,
    next_path: Option<&str>,
    code: i32,
    slug: &str,
    email: Option<&str>,
    allowed_hosts: &[&str],
) -> Response {
    let mut params: Vec<(&str, ParamValue)> = vec![
        ("error_code", ParamValue::Int(i64::from(code))),
        ("error_message", ParamValue::Str(slug.to_owned())),
    ];
    if let Some(address) = email {
        params.push(("email", ParamValue::Str(address.to_owned())));
    }
    redirect_302(get_safe_redirect_url(
        base,
        next_path.unwrap_or(""),
        &params,
        allowed_hosts,
    ))
}

/// Space success redirect (`space/magic.py:98-103,156-161`):
/// `validate_next_path` joined onto the stripped base, falling back to the
/// base when the composed URL is not allowed.
fn space_success(base: &str, next_path: Option<&str>, allowed_hosts: &[&str]) -> Response {
    let validated = validate_next_path(next_path.unwrap_or(""));
    let url = format!("{}{validated}", base.trim_end_matches('/'));
    if url_has_allowed_host_and_scheme(&url, allowed_hosts) {
        redirect_302(url)
    } else {
        redirect_302(base.to_owned())
    }
}

// ---------------------------------------------------------------------------
// Publishing
// ---------------------------------------------------------------------------

/// Enqueue a worker message for the worker to forward to the broker
/// (Python-owned task), exactly like the intake handlers: a propagation
/// failure 500s, mirroring the uncaught `.delay()` at the call site.
async fn enqueue_message(
    pool: &sqlx::PgPool,
    message: pidash_jobs::celery::CeleryTaskMessage,
) -> Result<(), sqlx::Error> {
    let job = pidash_jobs::queue::NewJob::new(
        message.task.clone(),
        serde_json::Value::Array(message.args.clone()),
        serde_json::Value::Object(message.kwargs.clone()),
    );
    pidash_jobs::queue::enqueue(pool, &job).await.map(|_| ())
}

/// `magic_link.delay(email, key, token)` (`views/app/magic.py:54`,
/// `views/space/magic.py:50`; FX-AUTH-09).
async fn enqueue_magic_link(pool: &sqlx::PgPool, email: &str, key: &str, token: &str) -> Response {
    let emit = pidash_services::auth_session::tasks::MagicLinkEmit {
        email: email.to_owned(),
        key: key.to_owned(),
        token: token.to_owned(),
    };
    let message =
        pidash_jobs::celery::CeleryTaskMessage::new(emit.task_name(), emit.args(), empty_kwargs());
    match enqueue_message(pool, message).await {
        Ok(()) => ok_key(key),
        Err(_) => server_error(),
    }
}

// ---------------------------------------------------------------------------
// Users
// ---------------------------------------------------------------------------

/// The `users` columns the magic closure reads and writes.
struct DbUser {
    id: uuid::Uuid,
    email: String,
    password: String,
    is_password_autoset: bool,
    is_active: bool,
}

/// `User.objects.filter(email=).first()` (`adapter/base.py:297`, the
/// views): exact match, `Meta.ordering = ("-created_at",)` — the full
/// select from [`user_by_email_sql`], decoded by column name.
async fn fetch_user(pool: &sqlx::PgPool, email: &str) -> Result<Option<DbUser>, sqlx::Error> {
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(&user_by_email_sql(USER_TABLE, "$1"))
        .bind(email)
        .fetch_optional(pool)
        .await?;
    let Some(row) = row else { return Ok(None) };
    use sqlx::Row;
    Ok(Some(DbUser {
        id: row.try_get("id")?,
        email: row
            .try_get::<Option<String>, _>("email")?
            .unwrap_or_default(),
        password: row.try_get("password")?,
        is_password_autoset: row.try_get("is_password_autoset")?,
        is_active: row.try_get("is_active")?,
    }))
}

/// `User.objects.filter(email=).exists()` (the magic-code paths
/// `magic_code.py:70,122,136`): same WHERE arm under `SELECT (1)`.
async fn user_exists(pool: &sqlx::PgPool, email: &str) -> Result<bool, sqlx::Error> {
    let row: Option<(i32,)> = sqlx::query_as(&user_email_exists_sql(USER_TABLE, "$1"))
        .bind(email)
        .fetch_optional(pool)
        .await?;
    Ok(row.is_some())
}

/// `user.get_session_auth_hash()`: `salted_hmac(...get_session_auth_hash,
/// password).hexdigest()` (`django/contrib/auth/base_user.py`). The salt
/// is shared with the license plumbing ([`crate::license::SESSION_AUTH_HASH_SALT`]).
fn session_auth_hash(password_field: &str, secret_key: &[u8]) -> String {
    let key = Sha256::digest(
        [
            crate::license::SESSION_AUTH_HASH_SALT.as_bytes(),
            secret_key,
        ]
        .concat(),
    );
    let mut mac = Hmac::<Sha256>::new_from_slice(&key).expect("HMAC-SHA256 accepts any key length");
    mac.update(password_field.as_bytes());
    hex_encode(&mac.finalize().into_bytes())
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

/// Draw `n` characters from Django's `get_random_string` alphabet
/// (`ascii_letters + digits`, 62 symbols) off UUID4 bytes with rejection
/// sampling (256 = 4 × 62 + 8: bytes ≥ 248 redraw, so the map is uniform).
fn django_random_string(n: usize) -> String {
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut out = String::with_capacity(n);
    let mut rand = uuid::Uuid::new_v4().into_bytes();
    let mut at = 0;
    while out.len() < n {
        if at == rand.len() {
            rand = uuid::Uuid::new_v4().into_bytes();
            at = 0;
        }
        let byte = rand[at];
        at += 1;
        if byte < 248 {
            out.push(ALPHABET[(byte % 62) as usize] as char);
        }
    }
    out
}

/// `secrets.randbelow(900000) + 100000` off UUID4 bytes, through the
/// kernel's unbiased map ([`token_from_u32`]).
fn generate_token() -> String {
    loop {
        let bytes = uuid::Uuid::new_v4().into_bytes();
        let draw = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        if draw >= TOKEN_REJECTION_LIMIT {
            continue;
        }
        if let Some(token) = token_from_u32(draw) {
            return token;
        }
    }
}

/// `set_password(uuid4().hex)` (`adapter/base.py:308-310`): PBKDF2-SHA256
/// with a fresh 22-alphanumeric salt (`BasePasswordHasher.salt()`) at the
/// fixture iteration count. Returns `(password_hash, raw_password)` — the
/// raw value is discarded by the caller (it is never stored).
fn autoset_password() -> (String, String) {
    let raw = uuid::Uuid::new_v4().simple().to_string();
    let salt = django_random_string(22);
    let hash = pidash_auth::password::hash_password(
        &raw,
        &salt,
        pidash_services::auth_session::models::PASSWORD_HASH_ITERATIONS,
    );
    (hash, raw)
}

/// New-user INSERT (`adapter/base.py:301-342` + `User.save()` side
/// effects `user.py:167-187`): every NOT NULL column is supplied (Django
/// emits no DB-level defaults); the password hash lands before the insert
/// and `token` stays `""` (`token_updated_at` NULL → no rotation).
#[allow(clippy::too_many_arguments)]
async fn insert_user(
    pool: &sqlx::PgPool,
    now: chrono::DateTime<chrono::Utc>,
    email: &str,
    username: &str,
    password_hash: &str,
    display_name: &str,
    login_ip: Option<&str>,
    login_uagent: Option<&str>,
) -> Result<uuid::Uuid, sqlx::Error> {
    let id = uuid::Uuid::new_v4();
    sqlx::query(
        "INSERT INTO \"users\" (\"id\", \"password\", \"last_login\", \"username\", \"mobile_number\", \
         \"email\", \"display_name\", \"first_name\", \"last_name\", \"avatar\", \"avatar_asset_id\", \
         \"cover_image\", \"cover_image_asset_id\", \"date_joined\", \"created_at\", \"updated_at\", \
         \"last_location\", \"created_location\", \"is_superuser\", \"is_managed\", \
         \"is_password_expired\", \"is_active\", \"is_staff\", \"is_email_verified\", \
         \"is_password_autoset\", \"is_password_reset_required\", \"token\", \"last_active\", \
         \"last_login_time\", \"last_logout_time\", \"last_login_ip\", \"last_logout_ip\", \
         \"last_login_medium\", \"last_login_uagent\", \"token_updated_at\", \"is_bot\", \"bot_type\", \
         \"user_timezone\", \"is_email_valid\", \"masked_at\") \
         VALUES ($1, $2, NULL, $3, NULL, $4, $5, '', '', '', NULL, NULL, NULL, $6, $6, $6, \
         '', '', FALSE, FALSE, FALSE, TRUE, FALSE, TRUE, TRUE, FALSE, '', $6, NULL, NULL, $7, '', \
         'email', $8, NULL, FALSE, NULL, 'UTC', FALSE, NULL)",
    )
    .bind(id)
    .bind(password_hash)
    .bind(username)
    .bind(email)
    .bind(display_name)
    .bind(now)
    .bind(login_ip)
    .bind(login_uagent)
    .execute(pool)
    .await?;
    Ok(id)
}

/// `post_save` receiver `create_user_notification` (`user.py:327-340`):
/// a preferences row on every non-bot create.
async fn insert_notification_prefs(
    pool: &sqlx::PgPool,
    now: chrono::DateTime<chrono::Utc>,
    user_id: &uuid::Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO \"user_notification_preferences\" (\"id\", \"created_at\", \"updated_at\", \
         \"created_by_id\", \"updated_by_id\", \"deleted_at\", \"user_id\", \"workspace_id\", \
         \"project_id\", \"property_change\", \"state_change\", \"comment\", \"mention\", \
         \"issue_completed\") \
         VALUES ($1, $2, $2, NULL, NULL, NULL, $3, NULL, NULL, TRUE, TRUE, TRUE, TRUE, TRUE)",
    )
    .bind(uuid::Uuid::new_v4())
    .bind(now)
    .bind(user_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Profile row (`Profile.objects.create(user=user)`, `base.py:342`) with
/// the model defaults (`user.py:198-288` + `color.py:9-14`).
async fn insert_profile(
    pool: &sqlx::PgPool,
    now: chrono::DateTime<chrono::Utc>,
    user_id: &uuid::Uuid,
) -> Result<(), sqlx::Error> {
    // `get_random_color` (`utils/color.py:9`): `#` + six hex digits.
    let color = format!("#{}", hex_encode(&uuid::Uuid::new_v4().into_bytes()[..3]));
    sqlx::query(
        "INSERT INTO \"profiles\" (\"id\", \"created_at\", \"updated_at\", \
         \"user_id\", \"theme\", \"is_app_rail_docked\", \
         \"is_tour_completed\", \"onboarding_step\", \"use_case\", \"role\", \"is_onboarded\", \
         \"last_workspace_id\", \"billing_address_country\", \"billing_address\", \
         \"has_billing_address\", \"company_name\", \"notification_view_mode\", \
         \"is_smooth_cursor_enabled\", \"is_mobile_onboarded\", \"mobile_onboarding_step\", \
         \"mobile_timezone_auto_set\", \"language\", \"start_of_the_week\", \"goals\", \
         \"background_color\", \"is_navigation_tour_completed\", \"has_marketing_email_consent\", \
         \"is_subscribed_to_changelog\", \"product_tour\", \"settings\") \
         VALUES ($1, $2, $2, $3, '{}', TRUE, FALSE, \
         '{\"profile_complete\": false, \"workspace_create\": false, \
           \"workspace_invite\": false, \"workspace_join\": false}', \
         NULL, NULL, FALSE, NULL, 'INDIA', NULL, FALSE, '', 'full', FALSE, FALSE, \
         '{\"profile_complete\": false, \"workspace_create\": false, \"workspace_join\": false}', \
         FALSE, 'en', 0, '{}', $4, FALSE, FALSE, FALSE, \
         '{\"work_items\": false, \"cycles\": false, \"modules\": false, \"intake\": false, \
           \"pages\": false}', '{}')",
    )
    .bind(uuid::Uuid::new_v4())
    .bind(now)
    .bind(user_id)
    .bind(color)
    .execute(pool)
    .await?;
    Ok(())
}

/// `save_user_data` (`adapter/base.py:220-234`): the login stamp UPDATE
/// plus the `user_activation_email.delay(base_host, user.id)` branch,
/// which fires *before* the save while the user is still inactive
/// (QUIRK-activation-order). The `token_updated_at` write rotates
/// `users.token` (`User.save`, `user.py:169-171`).
async fn save_user_data(
    pool: &sqlx::PgPool,
    now: chrono::DateTime<chrono::Utc>,
    user: &DbUser,
    login_ip: Option<&str>,
    login_uagent: Option<&str>,
    base_plain: &str,
) -> Result<(), Response> {
    if !user.is_active {
        let emit = pidash_services::auth_session::tasks::UserActivationEmailEmit {
            current_site: base_plain.to_owned(),
            user_id: user.id.to_string(),
        };
        let message = pidash_jobs::celery::CeleryTaskMessage::new(
            emit.task_name(),
            emit.args(),
            empty_kwargs(),
        );
        if enqueue_message(pool, message).await.is_err() {
            return Err(server_error());
        }
    }
    let rotated = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    let updated: Result<_, sqlx::Error> = sqlx::query(
        "UPDATE \"users\" SET \"last_login_medium\" = 'magic-code', \"last_active\" = $1, \
         \"last_login_time\" = $1, \"last_login_ip\" = $2, \"last_login_uagent\" = $3, \
         \"token_updated_at\" = $1, \"token\" = $4, \"is_active\" = TRUE, \"updated_at\" = $1 \
         WHERE \"users\".\"id\" = $5",
    )
    .bind(now)
    .bind(login_ip)
    .bind(login_uagent)
    .bind(rotated)
    .bind(user.id)
    .execute(pool)
    .await;
    match updated {
        Ok(_) => Ok(()),
        Err(_) => Err(server_error()),
    }
}

// ---------------------------------------------------------------------------
// Profiles + redirection path
// ---------------------------------------------------------------------------

/// `Profile.objects.get_or_create(user=user)`: the live row's
/// `is_onboarded` + `last_workspace_id`, creating the default row first
/// when missing.
async fn get_or_create_profile(
    pool: &sqlx::PgPool,
    now: chrono::DateTime<chrono::Utc>,
    user_id: &uuid::Uuid,
) -> Result<(bool, Option<uuid::Uuid>), sqlx::Error> {
    // `Profile` extends `TimeAuditModel` (`user.py:200`), not the full
    // `BaseModel`: no `deleted_at` soft-delete column, so no filter here.
    let row: Option<(bool, Option<uuid::Uuid>)> = sqlx::query_as(
        "SELECT \"is_onboarded\", \"last_workspace_id\" FROM \"profiles\" \
         WHERE \"user_id\" = $1 LIMIT 1",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await?;
    if let Some(found) = row {
        return Ok(found);
    }
    insert_profile(pool, now, user_id).await?;
    Ok((false, None))
}

/// `get_redirection_path` (`utils/redirection_path.py:8-46`): the pure
/// branch order lives in [`select_redirection_path`]; this runs the reads.
/// Short-circuits on a missing/unonboarded profile — no workspace query
/// fires then, exactly like the Python early return.
async fn redirection_path(
    pool: &sqlx::PgPool,
    user: &DbUser,
    is_onboarded: bool,
    last_workspace_id: Option<uuid::Uuid>,
) -> Result<String, sqlx::Error> {
    if !is_onboarded {
        return Ok(
            redirection_path_str(&select_redirection_path(false, None, None, false)).to_owned(),
        );
    }
    let mut last_slug: Option<String> = None;
    if let Some(workspace_id) = last_workspace_id {
        last_slug = sqlx::query_scalar(
            "SELECT \"workspaces\".\"slug\" FROM \"workspaces\" \
             WHERE \"workspaces\".\"id\" = $1 AND \"workspaces\".\"deleted_at\" IS NULL \
             AND EXISTS (SELECT 1 FROM \"workspace_members\" WHERE \"workspace_members\".\"workspace_id\" = \
             \"workspaces\".\"id\" AND \"workspace_members\".\"member_id\" = $2 \
             AND \"workspace_members\".\"is_active\" = TRUE \
             AND \"workspace_members\".\"deleted_at\" IS NULL) LIMIT 1",
        )
        .bind(workspace_id)
        .bind(user.id)
        .fetch_optional(pool)
        .await?;
    }
    let fallback_slug: Option<String> = sqlx::query_scalar(
        "SELECT \"workspaces\".\"slug\" FROM \"workspaces\" \
         JOIN \"workspace_members\" ON \"workspace_members\".\"workspace_id\" = \"workspaces\".\"id\" \
         WHERE \"workspace_members\".\"member_id\" = $1 \
         AND \"workspace_members\".\"is_active\" = TRUE \
         AND \"workspaces\".\"deleted_at\" IS NULL AND \"workspace_members\".\"deleted_at\" IS NULL \
         ORDER BY \"workspaces\".\"created_at\" ASC LIMIT 1",
    )
    .bind(user.id)
    .fetch_optional(pool)
    .await?;
    let invite_count: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM \"workspace_member_invites\" \
         WHERE \"email\" = $1 AND \"accepted\" = TRUE AND \"deleted_at\" IS NULL",
    )
    .bind(&user.email)
    .fetch_one(pool)
    .await?;
    let target = select_redirection_path(
        true,
        last_slug.as_deref(),
        fallback_slug.as_deref(),
        invite_count.0 > 0,
    );
    Ok(redirection_path_str(&target).to_owned())
}

// ---------------------------------------------------------------------------
// `post_user_auth_workflow` (app success only)
// ---------------------------------------------------------------------------

/// Invite row for the workspace arm: id/role plus the workspace slug for
/// the invalidate call and the track event.
struct WorkspaceInvite {
    workspace_id: uuid::Uuid,
    role: i16,
    slug: String,
}

/// Invite row for the project arm.
struct ProjectInvite {
    workspace_id: uuid::Uuid,
    role: i16,
    created_by_id: Option<uuid::Uuid>,
}

/// `process_workspace_project_invitations`
/// (`utils/workspace_project_join.py:20-91`): workspace joins +
/// per-workspace cache invalidation and `track_event`, then the project
/// arm (workspace membership with the mapped role, then the `ProjectMember`
/// write *as written* — without `project_id`, so an accepted project
/// invite raises `IntegrityError`, kept), then both invite sets are
/// soft-deleted. No transaction anywhere: earlier writes stand when a
/// later one raises (QUIRK-no-transaction).
async fn process_workspace_project_invitations(
    pool: &sqlx::PgPool,
    cache: &Cache,
    now: chrono::DateTime<chrono::Utc>,
    user: &DbUser,
) -> Result<(), Response> {
    let map_err = |_: sqlx::Error| server_error();
    let invites: Vec<(uuid::Uuid, i16, String)> = sqlx::query_as(
        "SELECT \"workspace_member_invites\".\"workspace_id\", \
         \"workspace_member_invites\".\"role\", \"workspaces\".\"slug\" \
         FROM \"workspace_member_invites\" JOIN \"workspaces\" \
         ON \"workspaces\".\"id\" = \"workspace_member_invites\".\"workspace_id\" \
         WHERE \"workspace_member_invites\".\"email\" = $1 \
         AND \"workspace_member_invites\".\"accepted\" = TRUE \
         AND \"workspace_member_invites\".\"deleted_at\" IS NULL",
    )
    .bind(&user.email)
    .fetch_all(pool)
    .await
    .map_err(map_err)?;
    let invites: Vec<WorkspaceInvite> = invites
        .into_iter()
        .map(|(workspace_id, role, slug)| WorkspaceInvite {
            workspace_id,
            role,
            slug,
        })
        .collect();

    if !invites.is_empty() {
        let moment = now.to_rfc3339();
        let mut values: Vec<String> = Vec::with_capacity(invites.len());
        for invite in &invites {
            values.push(format!(
                "('{}', '{moment}', '{moment}', '{}', '{}', {}, NULL, NULL, NULL, \
                 '{props}', '{props}', '{issue}', TRUE, '{{}}', '{{}}', '{{}}')",
                uuid::Uuid::new_v4(),
                invite.workspace_id,
                user.id,
                invite.role,
                props = workspace_default_props(),
                issue = workspace_issue_props(),
            ));
        }
        let sql = format!(
            "INSERT INTO \"workspace_members\" (\"id\", \"created_at\", \"updated_at\", \
             \"workspace_id\", \"member_id\", \"role\", \"created_by_id\", \"updated_by_id\", \
             \"deleted_at\", \"view_props\", \"default_props\", \"issue_props\", \"is_active\", \
             \"getting_started_checklist\", \"tips\", \"explored_features\") \
             VALUES {} ON CONFLICT DO NOTHING",
            values.join(", ")
        );
        sqlx::query(&sql).execute(pool).await.map_err(map_err)?;

        for invite in &invites {
            // `invalidate_cache_directly(path, url_params=False, user=False,
            // multiple=True)`: `cache.keys("*<path>*")` + delete.
            let pattern = format!("*/api/workspaces/{}/members/*", invite.slug);
            cache
                .del_pattern(&pattern)
                .await
                .map_err(|_| server_error())?;
            // `track_event.delay(user_id=, event_name=, slug=,
            // event_properties=)` — keyword args (kwargs, not positional).
            let joined_at = if now.timestamp_subsec_micros() == 0 {
                now.format("%Y-%m-%dT%H:%M:%S%:z").to_string()
            } else {
                now.format("%Y-%m-%dT%H:%M:%S%.6f%:z").to_string()
            };
            let call = pidash_services::auth_session::queries::workspace_join_track_event_call(
                &user.id.to_string(),
                &invite.workspace_id.to_string(),
                &invite.slug,
                i32::from(invite.role),
                &joined_at,
            );
            let kwargs = call.as_object().cloned().unwrap_or_default();
            let message = pidash_jobs::celery::CeleryTaskMessage::new(
                "pi_dash.bgtasks.event_tracking_task.track_event",
                Vec::new(),
                kwargs,
            );
            enqueue_message(pool, message)
                .await
                .map_err(|_| server_error())?;
        }
    }

    let project_invites: Vec<(uuid::Uuid, i16, Option<uuid::Uuid>)> = sqlx::query_as(
        "SELECT \"workspace_id\", \"role\", \"created_by_id\" FROM \"project_member_invites\" \
         WHERE \"email\" = $1 AND \"accepted\" = TRUE AND \"deleted_at\" IS NULL",
    )
    .bind(&user.email)
    .fetch_all(pool)
    .await
    .map_err(map_err)?;
    let project_invites: Vec<ProjectInvite> = project_invites
        .into_iter()
        .map(|(workspace_id, role, created_by_id)| ProjectInvite {
            workspace_id,
            role,
            created_by_id,
        })
        .collect();

    if !project_invites.is_empty() {
        let moment = now.to_rfc3339();
        let mut values: Vec<String> = Vec::with_capacity(project_invites.len());
        for invite in &project_invites {
            let created_by = invite
                .created_by_id
                .map(|id| format!("'{id}'"))
                .unwrap_or_else(|| "NULL".to_owned());
            values.push(format!(
                "('{}', '{moment}', '{moment}', {created_by}, NULL, NULL, '{}', '{}', {}, \
                 '{props}', '{props}', '{issue}', TRUE, '{{}}', '{{}}', '{{}}')",
                uuid::Uuid::new_v4(),
                invite.workspace_id,
                user.id,
                map_invite_role(invite.role),
                props = workspace_default_props(),
                issue = workspace_issue_props(),
            ));
        }
        let sql = format!(
            "INSERT INTO \"workspace_members\" (\"id\", \"created_at\", \"updated_at\", \
             \"created_by_id\", \"updated_by_id\", \"deleted_at\", \"workspace_id\", \"member_id\", \
             \"role\", \"view_props\", \"default_props\", \"issue_props\", \"is_active\", \
             \"getting_started_checklist\", \"tips\", \"explored_features\") \
             VALUES {} ON CONFLICT DO NOTHING",
            values.join(", ")
        );
        sqlx::query(&sql).execute(pool).await.map_err(map_err)?;

        // Ported bug kept (`workspace_project_join.py:76-87`): the
        // `ProjectMember` write omits `project_id` although the column is
        // NOT NULL — Postgres raises `IntegrityError` despite
        // `ignore_conflicts=True`.
        let mut values: Vec<String> = Vec::with_capacity(project_invites.len());
        for invite in &project_invites {
            let created_by = invite
                .created_by_id
                .map(|id| format!("'{id}'"))
                .unwrap_or_else(|| "NULL".to_owned());
            values.push(format!(
                "('{}', '{moment}', '{moment}', {created_by}, NULL, NULL, '{}', '{}', NULL, {}, \
                 '{props}', '{props}', '{prefs}', 65535.0, TRUE)",
                uuid::Uuid::new_v4(),
                invite.workspace_id,
                user.id,
                map_invite_role(invite.role),
                props = project_default_props(),
                prefs = project_default_preferences(),
            ));
        }
        let sql = format!(
            "INSERT INTO \"project_members\" (\"id\", \"created_at\", \"updated_at\", \
             \"created_by_id\", \"updated_by_id\", \"deleted_at\", \"workspace_id\", \"member_id\", \
             \"comment\", \"role\", \"view_props\", \"default_props\", \"preferences\", \
             \"sort_order\", \"is_active\") \
             VALUES {} ON CONFLICT DO NOTHING",
            values.join(", ")
        );
        // The missing `project_id` raises here when the arm fires — the
        // earlier writes above already committed (no transaction).
        sqlx::query(&sql).execute(pool).await.map_err(map_err)?;
    }

    // Both querysets are (soft-)deleted after the joins (`:90-91`): the
    // default manager's `delete()` stamps `deleted_at`.
    sqlx::query(
        "UPDATE \"workspace_member_invites\" SET \"deleted_at\" = $1 \
         WHERE \"email\" = $2 AND \"accepted\" = TRUE AND \"deleted_at\" IS NULL",
    )
    .bind(now)
    .bind(&user.email)
    .execute(pool)
    .await
    .map_err(map_err)?;
    sqlx::query(
        "UPDATE \"project_member_invites\" SET \"deleted_at\" = $1 \
         WHERE \"email\" = $2 AND \"accepted\" = TRUE AND \"deleted_at\" IS NULL",
    )
    .bind(now)
    .bind(&user.email)
    .execute(pool)
    .await
    .map_err(map_err)?;
    Ok(())
}

/// Role mapping for the project-invite arm (`:66,80`):
/// `role if role in [5, 15] else 15`.
fn map_invite_role(role: i16) -> i16 {
    if role == 5 || role == 15 {
        role
    } else {
        15
    }
}

/// `get_default_props` (`workspace.py:22-52`, with `display_properties`).
fn workspace_default_props() -> &'static str {
    "{\"filters\": {\"priority\": null, \"state\": null, \"state_group\": null, \
      \"assignees\": null, \"created_by\": null, \"labels\": null, \"start_date\": null, \
      \"target_date\": null, \"subscriber\": null}, \
      \"display_filters\": {\"group_by\": null, \"order_by\": \"-created_at\", \"type\": null, \
      \"sub_issue\": true, \"show_empty_groups\": true, \"layout\": \"list\", \
      \"calendar_date_range\": \"\"}, \
      \"display_properties\": {\"assignee\": true, \"attachment_count\": true, \
      \"created_on\": true, \"due_date\": true, \"estimate\": true, \"key\": true, \
      \"labels\": true, \"link\": true, \"priority\": true, \"start_date\": true, \
      \"state\": true, \"sub_issue_count\": true, \"updated_on\": true}}"
}

/// `get_issue_props` (`workspace.py:110-111`).
fn workspace_issue_props() -> &'static str {
    "{\"subscribed\": true, \"assigned\": true, \"created\": true, \"all_issues\": true}"
}

/// `get_default_props` (`project.py:43-62`, without `display_properties`).
fn project_default_props() -> &'static str {
    "{\"filters\": {\"priority\": null, \"state\": null, \"state_group\": null, \
      \"assignees\": null, \"created_by\": null, \"labels\": null, \"start_date\": null, \
      \"target_date\": null, \"subscriber\": null}, \
      \"display_filters\": {\"group_by\": null, \"order_by\": \"-created_at\", \"type\": null, \
      \"sub_issue\": true, \"show_empty_groups\": true, \"layout\": \"list\", \
      \"calendar_date_range\": \"\"}}"
}

/// `get_default_preferences` (`project.py:68-69`).
fn project_default_preferences() -> &'static str {
    "{\"pages\": {\"block_display\": true}, \
      \"navigation\": {\"default_tab\": \"work_items\", \"hide_in_more_menu\": []}}"
}

// ---------------------------------------------------------------------------
// Session issuance (`user_login` + `django.contrib.auth.login`)
// ---------------------------------------------------------------------------

/// `csrftoken` cookie name (no project override → Django default).
const CSRF_COOKIE_NAME: &str = "csrftoken";
/// `CSRF_COOKIE_AGE` (no project override → Django default, 1 year).
const CSRF_COOKIE_MAX_AGE: i64 = 31_449_600;

/// Inputs to [`login_user`], bundled so the helper stays under the
/// argument-count lint.
struct LoginRequest<'a> {
    pool: &'a sqlx::PgPool,
    extension: &'a Option<axum::Extension<SessionHandle>>,
    settings: &'a pidash_db::config::Settings,
    user: &'a DbUser,
    user_agent: &'a str,
    login_ip: Option<String>,
    domain: &'a str,
    now: chrono::DateTime<chrono::Utc>,
}

/// `user_login(request, user, ...)` (`utils/login.py:14-28`):
/// `login()` writes `_auth_user_id` / `_auth_user_backend` /
/// `_auth_user_hash` (cycling the key when another user — or nobody — was
/// on the session, i.e. `flush()` + `cycle_key()`), the `user_logged_in`
/// signal stamps `users.last_login`, and `device_info` lands in the
/// session. The tower layer persists the mutated session and sets the
/// `session-id` cookie; the fresh masked `csrftoken` cookie below mirrors
/// the `rotate_token` half of `login()`.
async fn login_user(request: LoginRequest<'_>) -> Result<String, Response> {
    let LoginRequest {
        pool,
        extension,
        settings,
        user,
        user_agent,
        login_ip,
        domain,
        now,
    } = request;
    let Some(handle) = extension.as_ref().map(|e| e.0.clone()) else {
        return Err(server_error());
    };
    let session_hash = session_auth_hash(&user.password, settings.secret_key.as_bytes());
    let user_id = user.id.to_string();
    // Every lock below is statement-scoped: the guard never crosses an
    // `.await`, per the session layer's contract ("neither the layer nor
    // handlers await while locked") — holding it across the delete below
    // would also poison the future's `Send` bound.
    let current: Option<String> = handle
        .lock()
        .get("_auth_user_id")
        .and_then(|v| v.as_str())
        .map(str::to_owned);
    if current.as_deref() != Some(user_id.as_str()) {
        // `flush()` when the session belonged to somebody else (or
        // nobody): drop the key (the layer issues a fresh one, like
        // `cycle_key`) and delete the abandoned row when there was one.
        let old: Option<String> = handle.lock().key.clone();
        handle.lock().clear();
        if let Some(old) = old {
            let deleted: Result<_, sqlx::Error> =
                sqlx::query("DELETE FROM \"sessions\" WHERE \"session_key\" = $1")
                    .bind(&old)
                    .execute(pool)
                    .await;
            if deleted.is_err() {
                return Err(server_error());
            }
        }
    }
    {
        let mut session = handle.lock();
        session.set(
            "_auth_user_id".to_owned(),
            serde_json::Value::String(user.id.to_string()),
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
            "device_info".to_owned(),
            serde_json::json!({
                "user_agent": user_agent,
                "ip_address": login_ip,
                "domain": domain,
            }),
        );
    }
    // `user_logged_in` → `update_last_login` (`auth/models.py`): the
    // signal save, inside `login()` before the session persists.
    if sqlx::query("UPDATE \"users\" SET \"last_login\" = $1 WHERE \"users\".\"id\" = $2")
        .bind(now)
        .bind(user.id)
        .execute(pool)
        .await
        .is_err()
    {
        return Err(server_error());
    }
    // `rotate_token` half: a fresh secret, masked, with Django's cookie
    // attributes (secure/domain from settings, httponly per the project
    // override, `/` + `Lax` defaults).
    let secret = pidash_auth::csrf::new_secret();
    let masked = pidash_auth::csrf::mask_secret(&secret).map_err(|_| server_error())?;
    let set_cookie = pidash_auth::session::render_set_cookie(&pidash_auth::session::SetCookie {
        name: CSRF_COOKIE_NAME.to_owned(),
        value: masked,
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
    Ok(set_cookie)
}

/// Push one `Set-Cookie` without disturbing the layer's own (Django's
/// login response carries both the session and the rotated CSRF cookie).
fn push_cookie(response: &mut Response, value: String) {
    if let Ok(value) = axum::http::HeaderValue::from_str(&value) {
        response.headers_mut().append(header::SET_COOKIE, value);
    }
}

// ---------------------------------------------------------------------------
// Generate
// ---------------------------------------------------------------------------

/// `MagicGenerateEndpoint.post` / `MagicGenerateSpaceEndpoint.post`: the
/// throttle runs first (DRF `initial()` order), then the instance gate,
/// then email validation (uncaught → 500), then the provider gates, then
/// `initiate()` + `magic_link.delay`.
async fn generate(
    state: AppState,
    headers: axum::http::HeaderMap,
    peer: ConnectInfo<std::net::SocketAddr>,
    extension: Option<axum::Extension<SessionHandle>>,
    body: Vec<u8>,
    app: bool,
) -> Response {
    let Some(pool) = state.pools().map(|pools| pools.primary().clone()) else {
        return server_error();
    };
    let cache = Cache::from_state(&state);
    if !generate_session_user_active(&pool, &extension).await
        && !check_generate_throttle(cache.as_ref(), &headers, Some(peer.0.ip().to_string()), app)
            .await
    {
        return throttled();
    }
    if !or_500!(instance_setup_done(&pool).await) {
        let slug = "INSTANCE_NOT_CONFIGURED";
        let code = error_code(slug).unwrap_or(5000);
        return json_400(code, slug, &[]);
    }
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let is_json = content_type.contains("json");
    let email = if is_json && !body.is_empty() {
        match serde_json::from_slice::<serde_json::Value>(&body) {
            Ok(value) => match value.as_object().and_then(|o| o.get("email")) {
                Some(serde_json::Value::String(s)) => Some(normalise_email(s)),
                _ => return server_error(),
            },
            Err(error) => return malformed_json(error),
        }
    } else if is_json {
        // DRF: empty JSON body parses to `{}` → `.get("email", "")` → 500.
        return server_error();
    } else {
        match parse_generate_email(&body, false) {
            Some(email) => Some(email),
            None => return server_error(),
        }
    };
    let Some(email) = email else {
        return server_error();
    };
    if !email_is_valid(&email) {
        // QUIRK-generate-500: `validate_email` raises `ValidationError`,
        // uncaught by the view.
        return server_error();
    }
    let secret = state.settings().secret_key.clone();
    let (email_host, magic_enabled) = match provider_config(&pool, &secret).await {
        Ok(config) => config,
        Err(_) => return server_error(),
    };
    match init_gate(email_host.as_deref(), &magic_enabled) {
        InitGate::Ok => {}
        gate => {
            let (slug, code) = gate_error(gate).expect("gate carries an error");
            return json_400(code, slug, &[("email", ParamValue::Str(email.clone()))]);
        }
    }
    let Some(cache) = cache else {
        return server_error();
    };
    let key = magic_redis_key(&email);
    let stored = match cache.get(&key).await {
        Ok(None) => None,
        Ok(Some(raw)) => match parse_stored(&raw) {
            Some(stored) => Some(stored),
            None => return server_error(),
        },
        Err(_) => return server_error(),
    };
    let exists = match user_exists(&pool, strip_magic_prefix(&key)).await {
        Ok(exists) => exists,
        Err(_) => return server_error(),
    };
    let attempt = match initiate_decision(stored.as_ref(), exists) {
        InitiateOutcome::Emit { attempt } => attempt,
        InitiateOutcome::Exhausted { branch } => {
            let (slug, code) = exhausted_error(branch);
            return json_400(
                code,
                slug,
                &[(
                    "email",
                    ParamValue::Str(payload_email(branch, &key).to_owned()),
                )],
            );
        }
    };
    let token = generate_token();
    let value = if attempt == 0 {
        magic_redis_value_first(&email, &token)
    } else {
        let stored_attempt = stored.map(|s| s.current_attempt).unwrap_or(0);
        magic_redis_value_retry(&email, &token, stored_attempt)
    };
    if cache
        .set_ex(&key, &value, MAGIC_REDIS_EXPIRY_SECS)
        .await
        .is_err()
    {
        return server_error();
    }
    enqueue_magic_link(&pool, &email, &key, &token).await
}

async fn generate_app(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    peer: ConnectInfo<std::net::SocketAddr>,
    extension: Option<axum::Extension<SessionHandle>>,
    body: axum::body::Bytes,
) -> Response {
    generate(state, headers, peer, extension, body.to_vec(), true).await
}

async fn generate_space(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    peer: ConnectInfo<std::net::SocketAddr>,
    extension: Option<axum::Extension<SessionHandle>>,
    body: axum::body::Bytes,
) -> Response {
    generate(state, headers, peer, extension, body.to_vec(), false).await
}

// ---------------------------------------------------------------------------
// Sign-in / sign-up
// ---------------------------------------------------------------------------

/// Which closure runs: the pre-login user check and the success redirect
/// differ (`USER_DOES_NOT_EXIST` vs `USER_ALREADY_EXIST`; the sign-up
/// variant creates the user).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Flow {
    SignIn,
    SignUp,
}

/// `MagicSignInEndpoint.post` / `MagicSignUpEndpoint.post` and the space
/// twins: form POST → 302 always. Plain Django `View`s: no DRF permission
/// or throttle layer. The provider `__init__` gates and every
/// `AuthenticationException` become error redirects; only success logs in.
async fn sign(
    state: AppState,
    headers: axum::http::HeaderMap,
    peer: ConnectInfo<std::net::SocketAddr>,
    extension: Option<axum::Extension<SessionHandle>>,
    body: Vec<u8>,
    app: bool,
    flow: Flow,
) -> Response {
    let settings = state.settings().clone();
    let hosts = host_settings(&settings);
    let base = if app {
        base_host(&hosts, false, false, true)
    } else {
        base_host(&hosts, false, true, false)
    };
    let allowed: Vec<&str> = allowed_hosts_for(
        hosts.web_url,
        hosts.app_base_url,
        hosts.admin_base_url,
        hosts.space_base_url,
    );
    let form = parse_sign_form(&body);
    let required_slug = match flow {
        Flow::SignIn => "MAGIC_SIGN_IN_EMAIL_CODE_REQUIRED",
        Flow::SignUp => "MAGIC_SIGN_UP_EMAIL_CODE_REQUIRED",
    };
    if form.code.is_empty() || form.email.is_empty() {
        let code = error_code(required_slug).expect("required slug has a code");
        return redirect_error(
            &base,
            form.next_path.as_deref(),
            code,
            required_slug,
            None,
            &allowed,
        );
    }
    let Some(pool) = state.pools().map(|pools| pools.primary().clone()) else {
        return server_error();
    };
    let Some(cache) = Cache::from_state(&state) else {
        return server_error();
    };
    let existing = or_500!(fetch_user(&pool, &form.email).await);
    match (flow, existing.as_ref()) {
        (Flow::SignIn, None) => {
            let slug = "USER_DOES_NOT_EXIST";
            let code = error_code(slug).expect("slug has a code");
            return redirect_error(&base, form.next_path.as_deref(), code, slug, None, &allowed);
        }
        (Flow::SignUp, Some(_)) => {
            let slug = "USER_ALREADY_EXIST";
            let code = error_code(slug).expect("slug has a code");
            return redirect_error(&base, form.next_path.as_deref(), code, slug, None, &allowed);
        }
        _ => {}
    }
    // `MagicCodeProvider(request, key, code, callback)` gates: SMTP then
    // disabled — as redirects here, not 400s.
    let secret = settings.secret_key.clone();
    let (email_host, magic_enabled) = match provider_config(&pool, &secret).await {
        Ok(config) => config,
        Err(_) => return server_error(),
    };
    if let Some((slug, code)) = gate_error(init_gate(email_host.as_deref(), &magic_enabled)) {
        return redirect_error(
            &base,
            form.next_path.as_deref(),
            code,
            slug,
            Some(&form.email),
            &allowed,
        );
    }
    let key = magic_redis_key(&form.email);
    let stored = match cache.get(&key).await {
        Ok(None) => None,
        Ok(Some(raw)) => match parse_stored(&raw) {
            Some(stored) => Some(stored),
            None => return server_error(),
        },
        Err(_) => return server_error(),
    };
    let user_exists_bit = existing.is_some();
    match verify_decision(stored.as_ref(), &form.code, user_exists_bit) {
        VerifyOutcome::Ok { .. } => {}
        outcome => {
            let (slug, code) = verify_error(&outcome).expect("non-ok carries an error");
            return redirect_error(
                &base,
                form.next_path.as_deref(),
                code,
                slug,
                Some(strip_magic_prefix(&key)),
                &allowed,
            );
        }
    }
    // Code match: the token is deleted before the login continues.
    if cache.del(&key).await.is_err() {
        return server_error();
    }
    let peer_ip = Some(peer.0.ip().to_string());
    let user_agent = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_owned();
    let login_ip = crate::license::handlers_auth_forms::client_ip(
        headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()),
        peer_ip.as_deref(),
    );
    let now = chrono::Utc::now();
    // `complete_login_or_signup`: the sign-up variant creates the user
    // (after the `__check_signup` gate); the sign-in variant keeps the row.
    let user = match (flow, existing) {
        (Flow::SignIn, Some(user)) => user,
        (Flow::SignUp, None) => {
            if or_500!(signup_disabled(&pool, &secret, &form.email).await) {
                let slug = "SIGNUP_DISABLED";
                let code = error_code(slug).expect("slug has a code");
                return redirect_error(
                    &base,
                    form.next_path.as_deref(),
                    code,
                    slug,
                    Some(&form.email),
                    &allowed,
                );
            }
            let username = uuid::Uuid::new_v4().simple().to_string();
            let (password_hash, _) = autoset_password();
            let display_name = form
                .email
                .split('@')
                .next()
                .unwrap_or(&form.email)
                .to_owned();
            let id = insert_user(
                &pool,
                now,
                &form.email,
                &username,
                &password_hash,
                &display_name,
                login_ip.as_deref(),
                headers
                    .get(header::USER_AGENT)
                    .and_then(|v| v.to_str().ok()),
            )
            .await;
            let id = or_500!(id);
            or_500!(insert_notification_prefs(&pool, now, &id).await);
            or_500!(insert_profile(&pool, now, &id).await);
            DbUser {
                id,
                email: form.email.clone(),
                password: password_hash,
                is_password_autoset: true,
                is_active: true,
            }
        }
        _ => return server_error(),
    };
    let base_plain = base_host(&hosts, false, false, false);
    match save_user_data(
        &pool,
        now,
        &user,
        login_ip.as_deref(),
        headers
            .get(header::USER_AGENT)
            .and_then(|v| v.to_str().ok()),
        &base_plain,
    )
    .await
    {
        Ok(()) => {}
        Err(response) => return response,
    }
    // `callback=post_user_auth_workflow` on the app variants only (the
    // space providers are constructed without one).
    if app {
        match process_workspace_project_invitations(&pool, &cache, now, &user).await {
            Ok(()) => {}
            Err(response) => return response,
        }
    }
    let domain = if app {
        base_host(&hosts, false, false, true)
    } else {
        base_host(&hosts, false, true, false)
    };
    let csrf_cookie = match login_user(LoginRequest {
        pool: &pool,
        extension: &extension,
        settings: &settings,
        user: &user,
        user_agent: &user_agent,
        login_ip,
        domain: &domain,
        now,
    })
    .await
    {
        Ok(cookie) => cookie,
        Err(response) => return response,
    };
    let mut response = if app {
        let path = match flow {
            Flow::SignIn => {
                let (is_onboarded, last_workspace_id) =
                    or_500!(get_or_create_profile(&pool, now, &user.id).await);
                if user.is_password_autoset && is_onboarded {
                    "/".to_owned()
                } else if let Some(next) = form.next_path.filter(|n| !n.is_empty()) {
                    next
                } else {
                    or_500!(redirection_path(&pool, &user, is_onboarded, last_workspace_id).await)
                }
            }
            Flow::SignUp => match form.next_path.filter(|n| !n.is_empty()) {
                Some(next) => next,
                None => {
                    // Fresh profile, never onboarded: short-circuits to
                    // `onboarding` with no workspace reads.
                    or_500!(redirection_path(&pool, &user, false, None).await)
                }
            },
        };
        redirect_302(get_safe_redirect_url(&base, &path, &[], &allowed))
    } else {
        space_success(&base, form.next_path.as_deref(), &allowed)
    };
    push_cookie(&mut response, csrf_cookie);
    response
}

/// `__check_signup` (`adapter/base.py:102-120`): `SIGNUP_DISABLED` unless
/// an invite exists. `ENABLE_SIGNUP` resolves like the other db-sourced
/// keys (caller default `os.environ.get("ENABLE_SIGNUP", "1")`).
async fn signup_disabled(
    pool: &sqlx::PgPool,
    secret_key: &str,
    email: &str,
) -> Result<bool, sqlx::Error> {
    let enabled = config_value(
        pool,
        secret_key,
        "ENABLE_SIGNUP",
        env_default("ENABLE_SIGNUP", Some("1")),
    )
    .await?
    .unwrap_or_else(|| "1".to_owned());
    if enabled != "0" {
        return Ok(false);
    }
    let invite: Option<(i32,)> = sqlx::query_as(
        "SELECT (1) FROM \"workspace_member_invites\" WHERE \"email\" = $1 \
         AND \"deleted_at\" IS NULL LIMIT 1",
    )
    .bind(email)
    .fetch_optional(pool)
    .await?;
    Ok(invite.is_none())
}

async fn sign_in_app(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    peer: ConnectInfo<std::net::SocketAddr>,
    extension: Option<axum::Extension<SessionHandle>>,
    body: axum::body::Bytes,
) -> Response {
    sign(
        state,
        headers,
        peer,
        extension,
        body.to_vec(),
        true,
        Flow::SignIn,
    )
    .await
}

async fn sign_up_app(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    peer: ConnectInfo<std::net::SocketAddr>,
    extension: Option<axum::Extension<SessionHandle>>,
    body: axum::body::Bytes,
) -> Response {
    sign(
        state,
        headers,
        peer,
        extension,
        body.to_vec(),
        true,
        Flow::SignUp,
    )
    .await
}

async fn sign_in_space(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    peer: ConnectInfo<std::net::SocketAddr>,
    extension: Option<axum::Extension<SessionHandle>>,
    body: axum::body::Bytes,
) -> Response {
    sign(
        state,
        headers,
        peer,
        extension,
        body.to_vec(),
        false,
        Flow::SignIn,
    )
    .await
}

async fn sign_up_space(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    peer: ConnectInfo<std::net::SocketAddr>,
    extension: Option<axum::Extension<SessionHandle>>,
    body: axum::body::Bytes,
) -> Response {
    sign(
        state,
        headers,
        peer,
        extension,
        body.to_vec(),
        false,
        Flow::SignUp,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_hosts() -> (String, String, Vec<String>) {
        // Contract layout: app base http://localhost:3000, spaces base
        // http://localhost:8000/spaces/ (conftest + FX-AUTH-08 vectors).
        (
            "http://localhost:3000".to_owned(),
            "http://localhost:8000/spaces/".to_owned(),
            vec!["localhost:3000".to_owned(), "localhost:8000".to_owned()],
        )
    }

    #[test]
    fn route_paths_match_django_urls() {
        // `authentication/urls.py:61-78`.
        assert_eq!(APP_GENERATE_PATH, "/auth/magic-generate/");
        assert_eq!(APP_SIGN_IN_PATH, "/auth/magic-sign-in/");
        assert_eq!(APP_SIGN_UP_PATH, "/auth/magic-sign-up/");
        assert_eq!(SPACE_GENERATE_PATH, "/auth/spaces/magic-generate/");
        assert_eq!(SPACE_SIGN_IN_PATH, "/auth/spaces/magic-sign-in/");
        assert_eq!(SPACE_SIGN_UP_PATH, "/auth/spaces/magic-sign-up/");
    }

    #[test]
    fn generate_key_body_matches_fixture() {
        // FX-AUTH-08 `magic_generate.app_ok`: 200 `{"key": "magic_h@x.com"}`.
        let body = serde_json::json!({"key": "magic_h@x.com"}).to_string();
        assert_eq!(body, "{\"key\":\"magic_h@x.com\"}");
    }

    #[test]
    fn signin_error_redirects_match_fixture() {
        // FX-AUTH-08 `magic_signin`: required (5085), no-user (5060).
        let (app_base, _, allowed) = test_hosts();
        let allowed: Vec<&str> = allowed.iter().map(String::as_str).collect();
        let required = get_safe_redirect_url(
            &app_base,
            "",
            &[
                ("error_code", ParamValue::Int(5085)),
                (
                    "error_message",
                    ParamValue::Str("MAGIC_SIGN_IN_EMAIL_CODE_REQUIRED".to_owned()),
                ),
            ],
            &allowed,
        );
        assert_eq!(
            required,
            "http://localhost:3000/?error_code=5085&error_message=MAGIC_SIGN_IN_EMAIL_CODE_REQUIRED"
        );
        let no_user = get_safe_redirect_url(
            &app_base,
            "",
            &[
                ("error_code", ParamValue::Int(5060)),
                (
                    "error_message",
                    ParamValue::Str("USER_DOES_NOT_EXIST".to_owned()),
                ),
            ],
            &allowed,
        );
        assert_eq!(
            no_user,
            "http://localhost:3000/?error_code=5060&error_message=USER_DOES_NOT_EXIST"
        );
    }

    #[test]
    fn signup_error_redirect_matches_fixture() {
        // FX-AUTH-08 `magic_signup.app_exists`: 5030 USER_ALREADY_EXIST.
        let (app_base, _, allowed) = test_hosts();
        let allowed: Vec<&str> = allowed.iter().map(String::as_str).collect();
        let exists = get_safe_redirect_url(
            &app_base,
            "",
            &[
                ("error_code", ParamValue::Int(5030)),
                (
                    "error_message",
                    ParamValue::Str("USER_ALREADY_EXIST".to_owned()),
                ),
            ],
            &allowed,
        );
        assert_eq!(
            exists,
            "http://localhost:3000/?error_code=5030&error_message=USER_ALREADY_EXIST"
        );
    }

    #[test]
    fn space_required_redirect_matches_fixture() {
        // FX-AUTH-08 `magic_signin.space_required`: spaces base + params.
        let (_, space_base, allowed) = test_hosts();
        let allowed: Vec<&str> = allowed.iter().map(String::as_str).collect();
        let required = get_safe_redirect_url(
            &space_base,
            "",
            &[
                ("error_code", ParamValue::Int(5085)),
                (
                    "error_message",
                    ParamValue::Str("MAGIC_SIGN_IN_EMAIL_CODE_REQUIRED".to_owned()),
                ),
            ],
            &allowed,
        );
        assert_eq!(
            required,
            "http://localhost:8000/spaces/?error_code=5085&error_message=MAGIC_SIGN_IN_EMAIL_CODE_REQUIRED"
        );
    }

    #[test]
    fn space_success_joins_next_path() {
        // Oracle `test_next_path_handling_differs`: spaces joins
        // `/workspaces` onto the base path.
        let (_, space_base, allowed) = test_hosts();
        let allowed: Vec<&str> = allowed.iter().map(String::as_str).collect();
        let validated = validate_next_path("/workspaces");
        let url = format!("{}{validated}", space_base.trim_end_matches('/'));
        assert!(url_has_allowed_host_and_scheme(&url, &allowed));
        assert_eq!(url, "http://localhost:8000/spaces/workspaces");
    }

    #[test]
    fn app_success_echoes_next_path_param() {
        // Oracle `test_success_with_next_path`: app echoes `next_path` as a
        // query parameter with no `error_code`.
        let (app_base, _, allowed) = test_hosts();
        let allowed: Vec<&str> = allowed.iter().map(String::as_str).collect();
        let url = get_safe_redirect_url(&app_base, "/workspaces", &[], &allowed);
        assert_eq!(url, "http://localhost:3000/?next_path=/workspaces");
    }

    #[test]
    fn generate_400_bodies_match_fixture() {
        // FX-AUTH-08 `app_no_user` shape + the exhausted payloads: exact
        // key order `error_code, error_message, email`.
        let body = error_dict_json(&error_pairs(5060, "USER_DOES_NOT_EXIST", &[]));
        assert_eq!(
            body,
            "{\"error_code\":5060,\"error_message\":\"USER_DOES_NOT_EXIST\"}"
        );
        let body = error_dict_json(&error_pairs(
            5102,
            "EMAIL_CODE_ATTEMPT_EXHAUSTED_SIGN_UP",
            &[("email", ParamValue::Str("brandnew@x.com".to_owned()))],
        ));
        assert_eq!(
            body,
            "{\"error_code\":5102,\"error_message\":\"EMAIL_CODE_ATTEMPT_EXHAUSTED_SIGN_UP\",\"email\":\"brandnew@x.com\"}"
        );
        let body = error_dict_json(&error_pairs(5000, "INSTANCE_NOT_CONFIGURED", &[]));
        assert_eq!(
            body,
            "{\"error_code\":5000,\"error_message\":\"INSTANCE_NOT_CONFIGURED\"}"
        );
    }

    #[test]
    fn form_parsing_matches_django_post() {
        // `request.POST.get(...)`: last value wins, missing → `""`,
        // email stripped + lowered, code stripped, `next_path` raw.
        let form =
            parse_sign_form(b"email=U%40X.com&code=+123456+&next_path=%2Fworkspaces&code=999");
        assert_eq!(form.email, "u@x.com");
        assert_eq!(form.code, "999");
        assert_eq!(form.next_path.as_deref(), Some("/workspaces"));
        let empty = parse_sign_form(b"");
        assert_eq!(
            empty,
            SignForm {
                code: String::new(),
                email: String::new(),
                next_path: None
            }
        );
    }

    #[test]
    fn generate_email_parsing_matches_request_data() {
        // JSON object with a string email; anything else supplies no email
        // (→ 500 downstream, QUIRK-generate-500).
        assert_eq!(
            parse_generate_email(br#"{"email": " H@x.com "}"#, true),
            Some("h@x.com".to_owned())
        );
        assert_eq!(parse_generate_email(br#"{"email": null}"#, true), None);
        assert_eq!(parse_generate_email(br#"{"email": 123}"#, true), None);
        assert_eq!(parse_generate_email(br#"{}"#, true), None);
        assert_eq!(parse_generate_email(b"not json", true), None);
        assert_eq!(
            parse_generate_email(b"email=f%40x.com", false),
            Some("f@x.com".to_owned())
        );
        assert_eq!(parse_generate_email(b"", false), None);
    }

    #[test]
    fn throttle_idents_match_drf() {
        // `get_ident`: XFF with all whitespace stripped, else the peer.
        let mut headers = axum::http::HeaderMap::new();
        headers.insert("x-forwarded-for", "1.2.3.4, 5.6.7.8".parse().unwrap());
        assert_eq!(
            throttle_ident(&headers, Some("9.9.9.9".to_owned())),
            "1.2.3.4,5.6.7.8"
        );
        let headers = axum::http::HeaderMap::new();
        assert_eq!(
            throttle_ident(&headers, Some("9.9.9.9".to_owned())),
            "9.9.9.9"
        );
        assert_eq!(throttle_ident(&headers, None), "None");
        // Scopes: app `authentication`, space `anon` (guards table).
        assert_eq!(AUTHENTICATION_THROTTLE_SCOPE, "authentication");
        assert_eq!(DEFAULT_ANON_SCOPE, "anon");
        assert_eq!(
            throttle_cache_key(AUTHENTICATION_THROTTLE_SCOPE, "9.9.9.9"),
            "throttle_authentication_9.9.9.9"
        );
    }

    #[test]
    fn generated_tokens_have_provider_shape() {
        for _ in 0..25 {
            let token = generate_token();
            assert_eq!(token.len(), 6);
            assert!(token.bytes().all(|b| b.is_ascii_digit()));
            assert!(pidash_services::auth_session::magic::token_shape_ok(&token));
        }
        let salt = django_random_string(22);
        assert_eq!(salt.len(), 22);
        assert!(salt.bytes().all(|b| b.is_ascii_alphanumeric()));
    }

    #[test]
    fn session_auth_hash_matches_django_vectors() {
        // `AbstractBaseUser._get_session_auth_hash` (Django 4.2.30
        // `contrib/auth/base_user.py:145-152`): `salted_hmac(key_salt,
        // password, secret, algorithm="sha256").hexdigest()`, verified
        // against the installed framework with the explicit algorithm:
        // password `pbkdf2_sha256$1500000 testsalt$abc`, secret `topsecret`.
        assert_eq!(
            session_auth_hash("pbkdf2_sha256$1500000 testsalt$abc", b"topsecret"),
            "bb53f993bcd2ae8f77ad3394548a134c57d07047c08bc40bfd8f960b64302d7b"
        );
        let hash = session_auth_hash("pbkdf2_sha256$1500000$salt$hash", b"secret");
        assert_eq!(hash.len(), 64);
        assert!(hash.bytes().all(|b| b.is_ascii_hexdigit()));
        assert_ne!(
            hash,
            session_auth_hash("pbkdf2_sha256$1500000$salt$hash", b"other")
        );
    }

    #[test]
    fn invite_role_mapping_matches_python() {
        assert_eq!(map_invite_role(5), 5);
        assert_eq!(map_invite_role(15), 15);
        assert_eq!(map_invite_role(20), 15);
    }
}
