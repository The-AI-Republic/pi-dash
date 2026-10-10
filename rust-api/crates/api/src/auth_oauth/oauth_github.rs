//! D-17 GitHub OAuth handlers: app + space initiate/callback (stage 5, PIDASHCONV-336).
//!
//! Port of `apps/api/pi_dash/authentication/views/app/github.py` (full:
//! `GitHubOauthInitiateEndpoint` + `GitHubCallbackEndpoint`) and
//! `apps/api/pi_dash/authentication/views/space/github.py` (full: the space
//! twins). Routes from `apps/api/pi_dash/authentication/urls.py:88-103`:
//! `GET /auth/github/`, `GET /auth/github/callback/`,
//! `GET /auth/spaces/github/`, `GET /auth/spaces/github/callback/`.
//!
//! The dependency layers are owned by sibling issues and reused here,
//! never re-derived: the provider constructor/mapping half (PIDASHCONV-325,
//! `pidash_services::auth_oauth::{providers, error, exchange}`), the
//! account upsert (PIDASHCONV-327,
//! `pidash_db::auth_oauth::queries::account`), the redirect/host kernels
//! (D-16, `pidash_services::auth_session::shapes`), the view auth shape
//! (PIDASHCONV-331, `super::guards`, AUTHOAUTH-F9), and Django
//! `validate_email` (D-16, `crate::license::handlers_auth_forms`).
//!
//! Fixture ids: AUTHOAUTH-F10 (initiate goldens) + AUTHOAUTH-F11 (callback
//! goldens), github rows
//! (`rust-api/fixtures/auth_oauth/{F10_initiate,F11_callback}.golden.json`,
//! recorded by PIDASHCONV-324).
//!
//! # Ported bugs and quirks (translate, don't redesign)
//!
//! * QUIRK-space-callback-500 (`views/space/github.py:65,74,83,96,99`):
//!   `base_host = request.session.get("host")` at the top of `get`
//!   shadows the imported `base_host()` helper for the whole function
//!   body, so every later `base_host(request=..., is_space=True)` call
//!   raises `TypeError` (`'NoneType'`/`'str'` object is not callable),
//!   uncaught by `except AuthenticationException`. Every GET to
//!   `/auth/spaces/github/callback/` answers Django's 500 — mismatch,
//!   missing-code, and success branches alike (the `authenticate()` /
//!   `user_login()` side effects still run first on the valid path). The
//!   handler below returns 500 for every GET to that path, as-is.
//! * QUIRK-app-initiate-raw-next-path (`views/app/github.py:35-37`): the
//!   app initiate stores `str(next_path)` RAW (unvalidated); validation
//!   happens later inside `get_safe_redirect_url`.
//! * QUIRK-space-initiate-no-next-path (`views/space/github.py:31-33`):
//!   the space initiates read `next_path` from the query for the error
//!   redirect but never store it in the session. Ported as-is.
//! * QUIRK-is-signup-inverted (`adapter/base.py:complete_login_or_signup`):
//!   `is_signup = bool(user)` is True when the user already EXISTS, so
//!   the IDP-sync branch (`check_sync_enabled() and not is_signup`) runs
//!   for newly created users, against its own comment. Ported as-is.
//! * QUIRK-callback-final-validate (`views/app/github.py:97`): the success
//!   redirect passes `session_next_path or get_redirection_path(user)`
//!   through `get_safe_redirect_url`, whose `validate_next_path` drops
//!   the slash-less redirection targets (`onboarding`, `<slug>`,
//!   `invitations`, `create-workspace`) — those users land on the bare
//!   app base. Ported as-is via the shared kernel.
//!
//! # Cross-domain transport boundaries (documented, no stubs)
//!
//! The success chain below performs every Python statement with real
//! I/O. Three transports belong to other domains and degrade exactly as
//! Python's own `except` fallbacks do: avatar S3 upload failure falls
//! back to the provider URL (`base.py:download_and_upload_avatar`
//! returns `None` on any exception); `user_activation_email.delay` and
//! `track_event.delay` are Celery tasks owned by other domains, so the
//! DB writes they guard (activation flag, membership rows) land while
//! the queue publish is a documented no-op like the D-33 install flow's
//! memory-broker note; cache invalidation (`invalidate_cache_directly`)
//! has no Rust cache population to evict from and is a documented no-op.

use std::collections::HashMap;

use std::net::SocketAddr;

use axum::{
    extract::{ConnectInfo, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Extension, Router,
};
use serde_json::Value;
use uuid::Uuid;

use pidash_db::auth_oauth::queries::account as account_queries;
use pidash_services::auth_oauth::{error as oauth_error, exchange, providers};
use pidash_services::auth_session::shapes::{self, HostSettings, ParamValue};

use crate::middleware::SessionHandle;
use crate::state::AppState;

/// `GITHUB_NOT_CONFIGURED` code (fixture F5 `OAUTH_ERROR_ROWS` order).
fn github_not_configured() -> oauth_error::AuthenticationException {
    oauth_error::AuthenticationException::new(
        oauth_error::error_code_by_name("GITHUB_NOT_CONFIGURED").unwrap_or(5110),
        "GITHUB_NOT_CONFIGURED".to_owned(),
        Vec::new(),
    )
}

/// `GITHUB_OAUTH_PROVIDER_ERROR` code (state-mismatch, missing-code, and
/// every exchange failure in these views).
fn github_provider_error() -> oauth_error::AuthenticationException {
    oauth_error::AuthenticationException::new(
        oauth_error::error_code_by_name("GITHUB_OAUTH_PROVIDER_ERROR").unwrap_or(5120),
        "GITHUB_OAUTH_PROVIDER_ERROR".to_owned(),
        Vec::new(),
    )
}

/// The four owned paths. Every other method on each path proxies to Django
/// (a plain `django.views.View` answers 405 there), following the
/// license-handler precedent: registration is the cutover granularity.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/auth/github/",
            get(app_initiate)
                .post(crate::edge::proxy)
                .put(crate::edge::proxy)
                .patch(crate::edge::proxy)
                .delete(crate::edge::proxy)
                .head(crate::edge::proxy)
                .options(crate::edge::proxy),
        )
        .route(
            "/auth/github/callback/",
            get(app_callback)
                .post(crate::edge::proxy)
                .put(crate::edge::proxy)
                .patch(crate::edge::proxy)
                .delete(crate::edge::proxy)
                .head(crate::edge::proxy)
                .options(crate::edge::proxy),
        )
        .route(
            "/auth/spaces/github/",
            get(space_initiate)
                .post(crate::edge::proxy)
                .put(crate::edge::proxy)
                .patch(crate::edge::proxy)
                .delete(crate::edge::proxy)
                .head(crate::edge::proxy)
                .options(crate::edge::proxy),
        )
        .route(
            "/auth/spaces/github/callback/",
            get(space_callback)
                .post(crate::edge::proxy)
                .put(crate::edge::proxy)
                .patch(crate::edge::proxy)
                .delete(crate::edge::proxy)
                .head(crate::edge::proxy)
                .options(crate::edge::proxy),
        )
}

/// `HttpResponseRedirect(url)`: 302 with a `Location` header.
fn redirect_response(location: String) -> Response {
    Response::builder()
        .status(StatusCode::FOUND)
        .header(header::LOCATION, location)
        .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
        .body(axum::body::Body::empty())
        .expect("redirect response")
}

/// Uncaught-exception answer: Django's 500 page. The contract suite pins
/// only the status on the space-callback path.
fn server_error() -> Response {
    (StatusCode::INTERNAL_SERVER_ERROR, "Internal Server Error").into_response()
}

// ---------------------------------------------------------------------------
// Request plumbing (host settings, session, headers)
// ---------------------------------------------------------------------------

/// `HostSettings` view over the boot-resolved `Settings`
/// (`authentication/utils/host.py:19-67` over D-16's kernel).
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

/// `base_host(request, is_app=True)` (`host.py:59-64`).
fn app_base(settings: &pidash_db::config::Settings) -> String {
    shapes::base_host(&host_settings(settings), false, false, true)
}

/// `base_host(request, is_space=True)` (`host.py:42-57`).
fn space_base(settings: &pidash_db::config::Settings) -> String {
    shapes::base_host(&host_settings(settings), false, true, false)
}

/// `get_allowed_hosts()` (`path_validator.py:70-86`) over the settings.
fn allowed_hosts(settings: &pidash_db::config::Settings) -> Vec<String> {
    let hs = host_settings(settings);
    shapes::allowed_hosts_for(
        hs.web_url,
        hs.app_base_url,
        hs.admin_base_url,
        hs.space_base_url,
    )
    .into_iter()
    .map(str::to_owned)
    .collect()
}

/// 302 `Location` for an error redirect: `get_safe_redirect_url` over the
/// `get_error_dict()` pairs in order (`path_validator.py:129-145`).
fn error_location(
    base_url: &str,
    next_path: Option<&str>,
    exc: &oauth_error::AuthenticationException,
    allowed: &[String],
) -> String {
    let allowed_refs: Vec<&str> = allowed.iter().map(String::as_str).collect();
    let mut owned: Vec<(String, ParamValue)> = vec![
        (
            "error_code".to_owned(),
            ParamValue::Int(exc.error_code as i64),
        ),
        (
            "error_message".to_owned(),
            ParamValue::Str(exc.error_message.clone()),
        ),
    ];
    for (key, value) in &exc.payload {
        let rendered = match value {
            Value::Bool(b) => ParamValue::Bool(*b),
            Value::Number(n) => n
                .as_i64()
                .map(ParamValue::Int)
                .unwrap_or_else(|| ParamValue::Str(n.to_string())),
            Value::Null => ParamValue::Null,
            Value::String(s) => ParamValue::Str(s.clone()),
            other => ParamValue::Str(other.to_string()),
        };
        match owned.iter_mut().find(|(k, _)| k == key) {
            Some(slot) => slot.1 = rendered,
            None => owned.push((key.clone(), rendered)),
        }
    }
    let param_refs: Vec<(&str, ParamValue)> =
        owned.iter().map(|(k, v)| (k.as_str(), v.clone())).collect();
    shapes::get_safe_redirect_url(
        base_url,
        next_path.unwrap_or(""),
        &param_refs,
        &allowed_refs,
    )
}

/// Session string read (`request.session.get(key)`); missing or non-string
/// reads as `None`.
fn session_get(handle: &SessionHandle, key: &str) -> Option<String> {
    handle
        .lock()
        .get(key)
        .and_then(|v| v.as_str())
        .map(str::to_owned)
}

/// Session write (`request.session[key] = value`).
fn session_set(handle: &SessionHandle, key: &str, value: Value) {
    handle.lock().set(key.to_owned(), value);
}

/// Live session, or a throwaway when the layer is absent (unit tests): the
/// views only ever read/write the session dict.
fn session_handle(extension: Option<Extension<SessionHandle>>) -> SessionHandle {
    match extension {
        Some(Extension(handle)) => handle,
        None => SessionHandle::new(crate::middleware::RequestSession::empty()),
    }
}

/// `request.get_host()`: the `Host` header.
fn request_host(headers: &HeaderMap) -> String {
    headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_owned()
}

/// `request.is_secure()`: `production.py` honors `X-Forwarded-Proto` when
/// `secure_proxy_ssl_header` is set, else the (http) scheme.
fn is_secure_request(headers: &HeaderMap, settings: &pidash_db::config::Settings) -> bool {
    settings.secure_proxy_ssl_header
        && headers
            .get("x-forwarded-proto")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.split(',').next().unwrap_or("").trim() == "https")
}

/// `request.META.get("HTTP_USER_AGENT")` (`login.py:22`, `base.py`): absent
/// stays absent — the stamp columns are NOT NULL, so Python raises into a
/// 500 downstream, which the `None` bind below reproduces exactly.
fn user_agent(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
}

/// `get_client_ip` (`utils/ip_address.py`): first `X-Forwarded-For` entry,
/// unstripped, else `REMOTE_ADDR` (the axum peer, which serve installs).
/// Absent on both legs stays `None` (same NOT NULL → 500 parity as above).
fn client_ip(headers: &HeaderMap, remote_addr: Option<&str>) -> Option<String> {
    if let Some(forwarded) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()) {
        if !forwarded.is_empty() {
            return Some(forwarded.split(',').next().unwrap_or("").to_owned());
        }
    }
    remote_addr.map(str::to_owned)
}

// ---------------------------------------------------------------------------
// Database reads (Instance gate + provider configuration)
// ---------------------------------------------------------------------------

/// `Instance.objects.first()` (`license/models/instance.py:50`,
/// `ordering = ("-created_at",)`): the latest non-deleted row decides
/// `is_setup_done`.
async fn instance_setup_done(pool: &sqlx::PgPool) -> Result<bool, sqlx::Error> {
    let row: Option<(bool,)> = sqlx::query_as(
        "SELECT is_setup_done FROM instances WHERE deleted_at IS NULL ORDER BY created_at DESC LIMIT 1",
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.is_some_and(|(done,)| done))
}

/// One `instance_configurations` read with the view's `os.environ` default
/// (`license/utils/instance_value.py:28-54`, the `handlers_external`
/// precedent): the stored row wins when present (decrypted when
/// `is_encrypted`), otherwise the environment default.
async fn config_value(
    pool: &sqlx::PgPool,
    secret_key: &str,
    key: &str,
    env_default: Option<String>,
) -> Result<Option<String>, sqlx::Error> {
    let row: Option<(Option<String>, bool)> = sqlx::query_as(
        "SELECT value, is_encrypted FROM instance_configurations WHERE key = $1 AND deleted_at IS NULL",
    )
    .bind(key)
    .fetch_optional(pool)
    .await?;
    match row {
        Some((Some(value), true)) => {
            let keyring = pidash_db::config::encryption::Keyring::from_secret(secret_key);
            Ok(Some(keyring.decrypt(&value)))
        }
        Some((Some(value), false)) => Ok(Some(value)),
        Some((None, _)) => Ok(env_default),
        None => Ok(env_default),
    }
}

/// The three `get_configuration_value` items the github provider reads
/// (`provider/oauth/github.py:33-50`): client id/secret default to
/// `os.environ.get(...)`, the organization id likewise.
struct GithubConfig {
    client_id: Option<String>,
    client_secret: Option<String>,
    organization_id: Option<String>,
}

async fn github_config(pool: &sqlx::PgPool, secret_key: &str) -> Result<GithubConfig, sqlx::Error> {
    let env = |name: &str| std::env::var(name).ok();
    Ok(GithubConfig {
        client_id: config_value(
            pool,
            secret_key,
            "GITHUB_CLIENT_ID",
            env("GITHUB_CLIENT_ID"),
        )
        .await?,
        client_secret: config_value(
            pool,
            secret_key,
            "GITHUB_CLIENT_SECRET",
            env("GITHUB_CLIENT_SECRET"),
        )
        .await?,
        organization_id: config_value(
            pool,
            secret_key,
            "GITHUB_ORGANIZATION_ID",
            env("GITHUB_ORGANIZATION_ID"),
        )
        .await?,
    })
}

fn pool_of(state: &AppState) -> Option<sqlx::PgPool> {
    state.pools().map(|pools| pools.primary().clone())
}

// ---------------------------------------------------------------------------
// Initiate (F10)
// ---------------------------------------------------------------------------

/// `GitHubOauthInitiateEndpoint.get` (`views/app/github.py:30-64`) and its
/// space twin (`views/space/github.py:30-62`).
///
/// Both write `session["host"]` on every GET; only the app surface stores
/// `next_path` (RAW — QUIRK-app-initiate-raw-next-path), and the space
/// twins never do (QUIRK-space-initiate-no-next-path). `INSTANCE_NOT_CONFIGURED`
/// (5000) fires before the provider constructor; the constructor's
/// `GITHUB_NOT_CONFIGURED` (5110) is caught into the same error channel.
/// Success is a 302 to the provider `auth_url` with `session["state"]`
/// set to fresh hex (`uuid.uuid4().hex`).
async fn initiate(
    state: &AppState,
    handle: &SessionHandle,
    headers: &HeaderMap,
    query: &HashMap<String, String>,
    is_space: bool,
) -> Response {
    let settings = state.settings();
    let base = if is_space {
        space_base(settings)
    } else {
        app_base(settings)
    };
    let allowed = allowed_hosts(settings);
    session_set(handle, "host", Value::String(base.clone()));

    // `next_path = request.GET.get("next_path")` (last value wins, like
    // Django's `QueryDict.get`).
    let next_path = query.get("next_path").cloned();
    if !is_space {
        if let Some(path) = next_path.as_deref() {
            if !path.is_empty() {
                session_set(handle, "next_path", Value::String(path.to_owned()));
            }
        }
    }

    let Some(pool) = pool_of(state) else {
        return server_error();
    };
    // `instance = Instance.objects.first()`; `None or not is_setup_done`
    // answers 5000. A database failure is uncaught in Python (500).
    match instance_setup_done(&pool).await {
        Ok(true) => {}
        Ok(false) => {
            let exc = oauth_error::AuthenticationException::new(
                oauth_error::error_code_by_name("INSTANCE_NOT_CONFIGURED").unwrap_or(5000),
                "INSTANCE_NOT_CONFIGURED".to_owned(),
                Vec::new(),
            );
            return redirect_response(error_location(&base, next_path.as_deref(), &exc, &allowed));
        }
        Err(_) => return server_error(),
    }

    let secret = settings.secret_key.clone();
    let config = match github_config(&pool, &secret).await {
        Ok(config) => config,
        Err(_) => return server_error(),
    };
    // `GitHubOAuthProvider(request, state)` raises
    // `GITHUB_NOT_CONFIGURED` when the id/secret pair is falsy
    // (`github.py:52-57`); the org id only widens the scope.
    if !providers::github_configured(config.client_id.as_deref(), config.client_secret.as_deref()) {
        let exc = github_not_configured();
        return redirect_response(error_location(&base, next_path.as_deref(), &exc, &allowed));
    }
    let state_hex = Uuid::new_v4().simple().to_string();
    session_set(handle, "state", Value::String(state_hex.clone()));
    let auth_url = providers::github_auth_url(
        config.client_id.as_deref().unwrap_or(""),
        is_secure_request(headers, settings),
        &request_host(headers),
        &state_hex,
        config.organization_id.as_deref(),
    );
    redirect_response(auth_url)
}

/// `GET /auth/github/` (`authentication/urls.py:92`).
async fn app_initiate(
    State(state): State<AppState>,
    extension: Option<Extension<SessionHandle>>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    initiate(&state, &session_handle(extension), &headers, &query, false).await
}

/// `GET /auth/spaces/github/` (`authentication/urls.py:96`).
async fn space_initiate(
    State(state): State<AppState>,
    extension: Option<Extension<SessionHandle>>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    initiate(&state, &session_handle(extension), &headers, &query, true).await
}

// ---------------------------------------------------------------------------
// Callback error branches (F11): state-mismatch, then missing-code
// ---------------------------------------------------------------------------

/// The two pre-provider guards both app and space callbacks share in
/// order (`views/app/github.py:67-89`): a `state` that does not equal
/// `session.get("state", "")` answers 5120, then a missing `code` answers
/// 5120 — each redirecting through `get_safe_redirect_url` with the
/// session `next_path`.
fn callback_guard_error(
    base_url: &str,
    session_next_path: Option<&str>,
    allowed: &[String],
) -> Response {
    redirect_response(error_location(
        base_url,
        session_next_path,
        &github_provider_error(),
        allowed,
    ))
}

// ---------------------------------------------------------------------------
// Callback success path: exchange (F4 shapes, transport like `requests`)
// ---------------------------------------------------------------------------

/// Callback failure: a rendered `AuthenticationException` (→ 302 with its
/// code) or an uncaught failure (→ Django's 500).
enum CallbackFailure {
    Auth(oauth_error::AuthenticationException),
    Server,
}

impl From<oauth_error::AuthenticationException> for CallbackFailure {
    fn from(exc: oauth_error::AuthenticationException) -> Self {
        CallbackFailure::Auth(exc)
    }
}

/// Python `str(value)` for the `Bearer {token}` / `f"{...}"` interpolations
/// (`oauth.py:88`, `github.py:138`): `None` renders as `"None"`.
fn python_str(value: &Value) -> String {
    match value {
        Value::Null => "None".to_owned(),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// The exchanged github credentials (`authenticate()` through
/// `set_user_data`, before `complete_login_or_signup`).
struct GithubAuth {
    tokens: providers::TokenData,
    user_data: providers::ProviderUserData,
}

/// `GitHubOAuthProvider.authenticate()` (`github.py:84-182` over
/// `adapter/oauth.py:66-100`).
///
/// Transport mirrors `requests` exactly: no timeout on the token/userinfo
/// calls, connection/status failures raise the provider error (5120),
/// while a non-JSON body escapes as a 500 (`response.json()` raising
/// `ValueError` is not a `RequestException`). The org-membership GET has
/// no `try` at all (`github.py:137-143`), so its transport failure is a
/// 500 too — only a non-200 *status* raises `GITHUB_USER_NOT_IN_ORG`
/// (5122).
async fn exchange_github(
    client_id: &str,
    client_secret: &str,
    organization_id: Option<&str>,
    code: &str,
    redirect_uri: &str,
) -> Result<GithubAuth, CallbackFailure> {
    let provider_error = || CallbackFailure::Auth(exchange::map_exchange_error("github"));
    let http = reqwest::Client::new();

    // `set_token_data` (`github.py:84-89`): form POST with
    // `{"Accept": "application/json"}`.
    let post_pairs =
        providers::github_token_post_data(code, client_id, client_secret, redirect_uri);
    let post_refs: Vec<(&str, &str)> = post_pairs.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let token_response = http
        .post(providers::GITHUB_TOKEN_URL)
        .header("Accept", "application/json")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(providers::urlencode(&post_refs))
        .send()
        .await
        .map_err(|_| provider_error())?;
    // `raise_for_status()` is inside the `try` (`oauth.py:75-84`).
    let token_response = token_response
        .error_for_status()
        .map_err(|_| provider_error())?;
    // `.json()` outside `RequestException` reach: decode failure is a 500.
    // (`reqwest` ships without its `json` feature here, so decode goes
    // through `bytes` + `serde_json`, with the same error mapping.)
    let token_bytes = token_response
        .bytes()
        .await
        .map_err(|_| CallbackFailure::Server)?;
    let token_json: Value =
        serde_json::from_slice(&token_bytes).map_err(|_| CallbackFailure::Server)?;
    let tokens = providers::github_token_data(&token_json);

    // `get_user_response` (`oauth.py:86-100`).
    let userinfo_response = http
        .get(providers::GITHUB_USERINFO_URL)
        .header(
            "Authorization",
            format!("Bearer {}", python_str(&tokens.access_token)),
        )
        .send()
        .await
        .map_err(|_| provider_error())?;
    let userinfo_response = userinfo_response
        .error_for_status()
        .map_err(|_| provider_error())?;
    let userinfo_bytes = userinfo_response
        .bytes()
        .await
        .map_err(|_| CallbackFailure::Server)?;
    let userinfo: Value =
        serde_json::from_slice(&userinfo_bytes).map_err(|_| CallbackFailure::Server)?;

    // Org gate (`github.py:145-160`): only when an organization id is set;
    // a non-200 membership status raises 5122, transport failure escapes.
    if organization_id.is_some_and(|org| !org.is_empty()) {
        let org = organization_id.unwrap_or("");
        let login = userinfo.get("login").and_then(|v| v.as_str()).unwrap_or("");
        let membership_url =
            providers::github_membership_url(providers::GITHUB_ORG_MEMBERSHIP_URL, org, login);
        let membership = http
            .get(membership_url)
            .header(
                "Authorization",
                format!("Bearer {}", python_str(&tokens.access_token)),
            )
            .send()
            .await
            .map_err(|_| CallbackFailure::Server)?;
        if !providers::is_org_member(membership.status().as_u16()) {
            return Err(CallbackFailure::Auth(
                oauth_error::AuthenticationException::new(
                    oauth_error::error_code_by_name("GITHUB_USER_NOT_IN_ORG").unwrap_or(5122),
                    "GITHUB_USER_NOT_IN_ORG".to_owned(),
                    Vec::new(),
                ),
            ));
        }
    }

    // `__get_email` (`github.py:108-135`): transport → 5120, non-list or
    // missing primary → 5120, decode failure → 500. The status is never
    // checked (`raise_for_status` is absent there, unlike the token and
    // userinfo legs), so a non-2xx body decodes like any other.
    let emails_response = http
        .get(providers::GITHUB_EMAILS_URL)
        .header(
            "Authorization",
            format!("Bearer {}", python_str(&tokens.access_token)),
        )
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|_| provider_error())?;
    let emails_bytes = emails_response
        .bytes()
        .await
        .map_err(|_| CallbackFailure::Server)?;
    let emails: Value =
        serde_json::from_slice(&emails_bytes).map_err(|_| CallbackFailure::Server)?;
    let email =
        providers::github_primary_email(&emails).map_err(|_| exchange::map_github_email_error())?;

    // `set_user_data` (`github.py:162-182`).
    let user_data = providers::github_user_data(&userinfo, email);
    Ok(GithubAuth { tokens, user_data })
}

/// `sanitize_email` (`adapter/base.py:sanitize_email`): falsy input fails
/// with 5005 `INVALID_EMAIL` carrying the RAW payload; otherwise
/// `str(email).lower().strip()` is validated and the reject payload
/// carries the NORMALIZED string. (`str()` rendering is exact for the
/// string/bool/number shapes the providers emit; exotic JSON shapes are
/// unreachable here.)
fn sanitize_email(email: &Value) -> Result<String, oauth_error::AuthenticationException> {
    let code = pidash_services::auth_session::shapes::error_code("INVALID_EMAIL").unwrap_or(5005);
    let invalid = |payload: Value| {
        oauth_error::AuthenticationException::new(
            code,
            "INVALID_EMAIL".to_owned(),
            vec![("email".to_owned(), payload)],
        )
    };
    if !providers::json_truthy(email) {
        return Err(invalid(email.clone()));
    }
    let normalized = python_str(email).to_lowercase();
    let normalized = normalized.trim().to_owned();
    if !crate::license::handlers_auth_forms::email_is_valid(&normalized) {
        return Err(invalid(Value::String(normalized.clone())));
    }
    Ok(normalized)
}

/// `User.get_display_name` (`db/models/user.py:190-197`): the local part
/// when the address splits into exactly two `@` parts, else six random
/// ASCII letters.
fn display_name_for_email(email: &str) -> String {
    let parts: Vec<&str> = email.split('@').collect();
    if parts.len() == 2 {
        return parts[0].to_owned();
    }
    random_letters(6)
}

/// Six random letters (`random.choice(string.ascii_letters)` × 6,
/// `user.py:192,196`): drawn from uuid4 hex here — still six random
/// ASCII alphanumerics, and the value is never observed (it only feeds
/// `display_name` when the email has no single `@`).
fn random_letters(n: usize) -> String {
    Uuid::new_v4()
        .simple()
        .to_string()
        .chars()
        .take(n)
        .collect()
}

// ---------------------------------------------------------------------------
// Callback success path: user resolution (`complete_login_or_signup`)
// ---------------------------------------------------------------------------

/// `make_password` salt (`BasePasswordHasher.salt()`, 128 bits): 22
/// alphanumerics drawn from uuid4 hex (the D-16 port documents the
/// alphabet as Django's).
fn password_salt() -> String {
    Uuid::new_v4()
        .simple()
        .to_string()
        .chars()
        .take(22)
        .collect()
}

/// `user.set_password(raw)` (`django/contrib/auth/hashers.py`,
/// PBKDF2-SHA256 at the project's iteration count — 600000 under the
/// pinned Django, per the D-16 port).
fn encode_password(password: &str) -> String {
    pidash_auth::password::hash_password(password, &password_salt(), 600_000)
}

/// `user.get_session_auth_hash()`: `salted_hmac` over the password field
/// (`django/contrib/auth/__init__.py`, salt
/// `AbstractBaseUser.get_session_auth_hash` — the construction the D-16
/// port verifies in `license/mod.rs`).
fn session_auth_hash(password_field: &str, secret_key: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    const SALT: &[u8] = b"django.contrib.auth.models.AbstractBaseUser.get_session_auth_hash";
    let key = Sha256::digest([SALT, secret_key].concat());
    use hmac::{Hmac, Mac};
    let mut mac = Hmac::<Sha256>::new_from_slice(&key).expect("HMAC accepts any key length");
    mac.update(password_field.as_bytes());
    let bytes = mac.finalize().into_bytes();
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

/// `onboarding_step` default (`db/models/user.py:get_default_onboarding`).
fn default_onboarding_step() -> Value {
    serde_json::json!({
        "profile_complete": false,
        "workspace_create": false,
        "workspace_invite": false,
        "workspace_join": false,
    })
}

/// `mobile_onboarding_step` default (`user.py:get_mobile_default_onboarding`).
fn default_mobile_onboarding_step() -> Value {
    serde_json::json!({
        "profile_complete": false,
        "workspace_create": false,
        "workspace_join": false,
    })
}

/// `product_tour` default (`user.py:get_default_product_tour`).
fn default_product_tour() -> Value {
    serde_json::json!({
        "work_items": false,
        "cycles": false,
        "modules": false,
        "intake": false,
        "pages": false,
    })
}

/// Member `view_props`/`default_props` (`workspace.py:get_default_props`,
/// identical in `project.py:get_default_props` minus `display_properties`).
fn workspace_default_props() -> Value {
    serde_json::json!({
        "filters": {
            "priority": null, "state": null, "state_group": null,
            "assignees": null, "created_by": null, "labels": null,
            "start_date": null, "target_date": null, "subscriber": null,
        },
        "display_filters": {
            "group_by": null, "order_by": "-created_at", "type": null,
            "sub_issue": true, "show_empty_groups": true,
            "layout": "list", "calendar_date_range": "",
        },
        "display_properties": {
            "assignee": true, "attachment_count": true, "created_on": true,
            "due_date": true, "estimate": true, "key": true, "labels": true,
            "link": true, "priority": true, "start_date": true, "state": true,
            "sub_issue_count": true, "updated_on": true,
        },
    })
}

/// Project-member `view_props`/`default_props` (`project.py:get_default_props`).
fn project_default_props() -> Value {
    serde_json::json!({
        "filters": {
            "priority": null, "state": null, "state_group": null,
            "assignees": null, "created_by": null, "labels": null,
            "start_date": null, "target_date": null, "subscriber": null,
        },
        "display_filters": {
            "group_by": null, "order_by": "-created_at", "type": null,
            "sub_issue": true, "show_empty_groups": true,
            "layout": "list", "calendar_date_range": "",
        },
    })
}

/// What the callback needs back from user resolution: the row id plus the
/// password field the session hash is computed over.
struct ResolvedUser {
    id: Uuid,
    password_field: String,
}

/// Django `CharField.get_prep_value` str()s non-string values on save
/// (`Account.provider_account_id` is a `CharField`, `user.py`): github
/// userinfo ids are JSON numbers, so the stored — and looked-up — id is
/// their decimal form. The F6 kernel reads the id as `&str` only, so the
/// coercion happens here at the call site, before lookup and create.
fn coerce_provider_id(user_data: &mut Value) {
    let coerced = match user_data.get("user").and_then(|u| u.get("provider_id")) {
        None | Some(Value::String(_)) => None,
        Some(other) => Some(Value::String(python_str(other))),
    };
    if let Some(replacement) = coerced {
        if let Some(user) = user_data.get_mut("user").and_then(|u| u.as_object_mut()) {
            user.insert("provider_id".to_owned(), replacement);
        }
    }
}

/// `complete_login_or_signup` (`adapter/base.py`) for the github callback:
/// sanitize → find-or-create (signup gate, profile, avatar URL) → optional
/// IDP sync → login-stamp save → invite callback → account upsert.
///
/// * The `validate_password` arm of user creation is unreachable for this
///   provider: `is_password_autoset` is hardcoded `true` in
///   `github_user_data` (`github.py:179`), so creation always takes the
///   autoset arm (`set_password(uuid4hex)`, verified + autoset flags).
/// * Avatar upload (`download_and_upload_avatar`, `base.py`) needs the S3
///   storage domain, which has no Rust facility in this crate: the
///   provider URL is stored as `avatar` with no `FileAsset` row — the
///   same value Python stores in `user.avatar` on every path, including
///   its upload-failure fallback.
/// * `user_activation_email.delay` and `track_event.delay` are Celery
///   tasks owned by other domains (documented no-op publishes, like the
///   D-33 flow); the guarded DB writes land. Cache invalidation has no
///   Rust population to evict from (documented no-op).
async fn complete_login_or_signup(
    pool: &sqlx::PgPool,
    secret_key: &str,
    headers: &HeaderMap,
    remote_addr: Option<&str>,
    auth: &GithubAuth,
    email: &str,
) -> Result<ResolvedUser, CallbackFailure> {
    let now = chrono::Utc::now();
    let user_data = &auth.user_data;
    let user_obj = &user_data.user;

    // `user = User.objects.filter(email=email).first()`;
    // `is_signup = bool(user)` — True when the user EXISTS
    // (QUIRK-is-signup-inverted, ported as-is).
    let existing: Option<(Uuid, bool, String)> =
        sqlx::query_as("SELECT id, is_active, password FROM users WHERE email = $1")
            .bind(email)
            .fetch_optional(pool)
            .await
            .map_err(|_| CallbackFailure::Server)?;
    let is_signup = existing.is_some();

    let (user_id, password_field) = if let Some((id, _, password)) = existing {
        (id, password)
    } else {
        // `__check_signup`: `ENABLE_SIGNUP == "0"` with no invite → 5030.
        let enable_signup = config_value(
            pool,
            secret_key,
            "ENABLE_SIGNUP",
            std::env::var("ENABLE_SIGNUP").ok(),
        )
        .await
        .map_err(|_| CallbackFailure::Server)?;
        if enable_signup.as_deref() == Some("0") {
            let invited: Option<(i64,)> = sqlx::query_as(
                "SELECT 1 FROM workspace_member_invites WHERE email = $1 AND deleted_at IS NULL LIMIT 1",
            )
            .bind(email)
            .fetch_optional(pool)
            .await
            .map_err(|_| CallbackFailure::Server)?;
            if invited.is_none() {
                return Err(CallbackFailure::Auth(
                    oauth_error::AuthenticationException::new(
                        pidash_services::auth_session::shapes::error_code("SIGNUP_DISABLED")
                            .unwrap_or(5035),
                        "SIGNUP_DISABLED".to_owned(),
                        vec![("email".to_owned(), Value::String(email.to_owned()))],
                    ),
                ));
            }
        }

        // Autoset arm (`base.py:complete_login_or_signup` new-user branch):
        // `username = uuid4().hex`, `set_password(uuid4().hex)`.
        let id = Uuid::new_v4();
        let username = Uuid::new_v4().simple().to_string();
        let password = encode_password(&Uuid::new_v4().simple().to_string());
        let first_name = user_obj
            .first_name
            .as_str()
            .map(str::to_owned)
            .unwrap_or_default();
        let last_name = user_obj
            .last_name
            .as_str()
            .map(str::to_owned)
            .unwrap_or_default();
        let avatar = user_obj.avatar.as_str().unwrap_or("").to_owned();
        // `display_name` stays the model default (`""`): the create branch
        // never sets it — only the sync path does below.
        sqlx::query(
            r#"INSERT INTO "users" ("id", "password", "last_login", "username", "mobile_number", "email",
                "display_name", "first_name", "last_name", "avatar", "avatar_asset_id",
                "cover_image", "cover_image_asset_id", "date_joined", "created_at", "updated_at",
                "last_location", "created_location", "is_superuser", "is_managed", "is_password_expired",
                "is_active", "is_staff", "is_email_verified", "is_password_autoset", "is_password_reset_required",
                "token", "last_active", "last_login_time", "last_logout_time", "last_login_ip", "last_logout_ip",
                "last_login_medium", "last_login_uagent", "token_updated_at", "is_bot", "bot_type",
                "user_timezone", "is_email_valid", "masked_at")
               VALUES ($1, $2, NULL, $3, NULL, $4, '', $5, $6, $7, NULL, NULL, NULL, $8, $8, $8,
                '', '', FALSE, FALSE, FALSE, TRUE, FALSE, TRUE, TRUE, FALSE,
                '', $8, NULL, NULL, '', '', 'email', '', NULL, FALSE, NULL, 'UTC', FALSE, NULL)"#,
        )
        .bind(id)
        .bind(&password)
        .bind(&username)
        .bind(email)
        .bind(&first_name)
        .bind(&last_name)
        .bind(&avatar)
        .bind(now)
        .execute(pool)
        .await
        .map_err(|_| CallbackFailure::Server)?;

        // `Profile.objects.create(user=user)` (`user.py:200-275` defaults).
        let background = format!(
            "#{}",
            Uuid::new_v4()
                .simple()
                .to_string()
                .chars()
                .take(6)
                .collect::<String>()
        );
        sqlx::query(
            r#"INSERT INTO "profiles" ("id", "created_at", "updated_at", "user_id", "theme",
                "is_app_rail_docked", "is_tour_completed", "onboarding_step", "use_case", "role",
                "is_onboarded", "last_workspace_id", "billing_address_country", "billing_address",
                "has_billing_address", "company_name", "notification_view_mode", "is_smooth_cursor_enabled",
                "is_mobile_onboarded", "mobile_onboarding_step", "mobile_timezone_auto_set", "language",
                "start_of_the_week", "goals", "background_color", "is_navigation_tour_completed",
                "has_marketing_email_consent", "is_subscribed_to_changelog", "product_tour", "settings")
               VALUES ($1, $2, $2, $3, '{}', TRUE, FALSE, $4, NULL, NULL, FALSE, NULL, 'INDIA', NULL,
                FALSE, '', 'full', FALSE, FALSE, $5, FALSE, 'en', 0, '{}', $6, FALSE, FALSE, FALSE, $7, '{}')"#,
        )
        .bind(Uuid::new_v4())
        .bind(now)
        .bind(id)
        .bind(default_onboarding_step())
        .bind(default_mobile_onboarding_step())
        .bind(&background)
        .bind(default_product_tour())
        .execute(pool)
        .await
        .map_err(|_| CallbackFailure::Server)?;
        (id, password)
    };

    // IDP sync (`base.py:sync_user_data`): `check_sync_enabled() and not
    // is_signup` — with the inverted flag this runs for NEW users.
    let sync_enabled = config_value(
        pool,
        secret_key,
        "ENABLE_GITHUB_SYNC",
        std::env::var("ENABLE_GITHUB_SYNC").ok(),
    )
    .await
    .map_err(|_| CallbackFailure::Server)?;
    if sync_enabled.as_deref() == Some("1") && !is_signup {
        let first_name = user_obj
            .first_name
            .as_str()
            .map(str::to_owned)
            .unwrap_or_default();
        let last_name = user_obj
            .last_name
            .as_str()
            .map(str::to_owned)
            .unwrap_or_default();
        let avatar = user_obj.avatar.as_str().unwrap_or("").to_owned();
        let display_name = display_name_for_email(email);
        sqlx::query(
            r#"UPDATE "users" SET "first_name" = $1, "last_name" = $2, "display_name" = $3,
               "avatar" = $4, "updated_at" = $5 WHERE "id" = $6"#,
        )
        .bind(&first_name)
        .bind(&last_name)
        .bind(&display_name)
        .bind(&avatar)
        .bind(now)
        .bind(user_id)
        .execute(pool)
        .await
        .map_err(|_| CallbackFailure::Server)?;
    }

    // `save_user_data` (`base.py`): login stamps; inactive users are
    // flipped active (the activation-email publish is a cross-domain
    // boundary — see module docs). `None` binds NULL into the NOT NULL
    // stamp columns, raising into the same 500 Python hits.
    let ip = client_ip(headers, remote_addr);
    let uagent = user_agent(headers);
    sqlx::query(
        r#"UPDATE "users" SET "last_login_medium" = 'github', "last_active" = $1, "last_login_time" = $1,
           "last_login_ip" = $2, "last_login_uagent" = $3, "token_updated_at" = $1,
           "is_active" = TRUE, "updated_at" = $1 WHERE "id" = $4"#,
    )
    .bind(now)
    .bind(ip.as_deref())
    .bind(uagent.as_deref())
    .bind(user_id)
    .execute(pool)
    .await
    .map_err(|_| CallbackFailure::Server)?;

    // `post_user_auth_workflow` → `process_workspace_project_invitations`
    // (`workspace_project_join.py`, full file): accepted workspace invites
    // become members (`bulk_create(ignore_conflicts=True)`), then accepted
    // project invites become workspace + project members (role clamped to
    // 5/15), then both invite sets soft-delete. Per-invite cache invalidation
    // and analytics publishes are cross-domain boundaries (see module docs).
    process_invitations(pool, email, user_id, now)
        .await
        .map_err(|_| CallbackFailure::Server)?;

    // `create_update_account` (F6 kernel, BUG-6 swallow preserved inside).
    let tokens = account_queries::TokenFields {
        access_token: auth.tokens.access_token.as_str().map(str::to_owned),
        refresh_token: auth.tokens.refresh_token.as_str().map(str::to_owned),
        access_token_expired_at: auth.tokens.access_token_expired_at,
        refresh_token_expired_at: auth.tokens.refresh_token_expired_at,
        id_token: auth.tokens.id_token.as_str().map(str::to_owned),
    };
    let mut user_data_value =
        serde_json::to_value(&auth.user_data).map_err(|_| CallbackFailure::Server)?;
    coerce_provider_id(&mut user_data_value);
    let mut conn = pool.acquire().await.map_err(|_| CallbackFailure::Server)?;
    account_queries::create_update_account(
        &mut conn,
        user_id,
        "github",
        &user_data_value,
        &tokens,
        now,
        Uuid::new_v4(),
        None,
    )
    .await
    .map_err(|_| CallbackFailure::Server)?;

    Ok(ResolvedUser {
        id: user_id,
        password_field,
    })
}

/// `process_workspace_project_invitations`
/// (`authentication/utils/workspace_project_join.py`, full file).
async fn process_invitations(
    pool: &sqlx::PgPool,
    email: &str,
    user_id: Uuid,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<(), sqlx::Error> {
    // Accepted workspace invites → members, `ignore_conflicts=True`.
    let workspace_invites: Vec<(Uuid, i16)> = sqlx::query_as(
        r#"SELECT workspace_id, role FROM workspace_member_invites
           WHERE email = $1 AND accepted AND deleted_at IS NULL"#,
    )
    .bind(email)
    .fetch_all(pool)
    .await?;
    for (workspace_id, role) in &workspace_invites {
        sqlx::query(
            r#"INSERT INTO "workspace_members" ("id", "created_at", "updated_at", "workspace_id",
               "member_id", "role", "view_props", "default_props", "issue_props",
               "is_active", "explored_features", "getting_started_checklist", "tips")
               VALUES ($1, $2, $2, $3, $4, $5, $6, $6, $7, TRUE, '{}', '{}', '{}') ON CONFLICT DO NOTHING"#,
        )
        .bind(Uuid::new_v4())
        .bind(now)
        .bind(workspace_id)
        .bind(user_id)
        .bind(role)
        .bind(workspace_default_props())
        .bind(serde_json::json!({"subscribed": true, "assigned": true, "created": true, "all_issues": true}))
        .execute(pool)
        .await?;
    }

    // Accepted project invites → workspace members (role clamped to 5/15)
    // + project members, `ignore_conflicts=True` on both.
    let project_invites: Vec<(Uuid, Uuid, i16, Option<Uuid>)> = sqlx::query_as(
        r#"SELECT workspace_id, project_id, role, created_by_id FROM project_member_invites
           WHERE email = $1 AND accepted AND deleted_at IS NULL"#,
    )
    .bind(email)
    .fetch_all(pool)
    .await?;
    for (workspace_id, project_id, role, created_by) in &project_invites {
        let clamped = if *role == 5 || *role == 15 { *role } else { 15 };
        sqlx::query(
            r#"INSERT INTO "workspace_members" ("id", "created_at", "updated_at", "workspace_id",
               "member_id", "role", "created_by_id", "view_props", "default_props", "issue_props",
               "is_active", "explored_features", "getting_started_checklist", "tips")
               VALUES ($1, $2, $2, $3, $4, $5, $6, $7, $7, $8, TRUE, '{}', '{}', '{}') ON CONFLICT DO NOTHING"#,
        )
        .bind(Uuid::new_v4())
        .bind(now)
        .bind(workspace_id)
        .bind(user_id)
        .bind(clamped)
        .bind(created_by)
        .bind(workspace_default_props())
        .bind(serde_json::json!({"subscribed": true, "assigned": true, "created": true, "all_issues": true}))
        .execute(pool)
        .await?;
        sqlx::query(
            r#"INSERT INTO "project_members" ("id", "created_at", "updated_at", "workspace_id",
               "project_id", "member_id", "role", "created_by_id", "view_props", "default_props", "preferences",
               "sort_order", "is_active")
               VALUES ($1, $2, $2, $3, $4, $5, $6, $7, $8, $8, $9, 65535, TRUE) ON CONFLICT DO NOTHING"#,
        )
        .bind(Uuid::new_v4())
        .bind(now)
        .bind(workspace_id)
        .bind(project_id)
        .bind(user_id)
        .bind(clamped)
        .bind(created_by)
        .bind(project_default_props())
        .bind(serde_json::json!({"pages": {"block_display": true},
                    "navigation": {"default_tab": "work_items", "hide_in_more_menu": []}}))
        .execute(pool)
        .await?;
    }

    // Both invite sets soft-delete (queryset `.delete()` → `UPDATE
    // deleted_at`, `mixins.py:SoftDeletionQuerySet`).
    sqlx::query(
        r#"UPDATE workspace_member_invites SET deleted_at = $1
           WHERE email = $2 AND accepted AND deleted_at IS NULL"#,
    )
    .bind(now)
    .bind(email)
    .execute(pool)
    .await?;
    sqlx::query(
        r#"UPDATE project_member_invites SET deleted_at = $1
           WHERE email = $2 AND accepted AND deleted_at IS NULL"#,
    )
    .bind(now)
    .bind(email)
    .execute(pool)
    .await?;
    Ok(())
}

/// `get_redirection_path` (`authentication/utils/redirection_path.py:8-46`)
/// over the branch kernel: onboarding → last workspace → earliest active
/// membership → invitations → create-workspace. The slash-less targets
/// degrade to the bare base downstream (QUIRK-callback-final-validate).
async fn redirection_path(
    pool: &sqlx::PgPool,
    user_id: Uuid,
    email: &str,
) -> Result<String, sqlx::Error> {
    let profile: Option<(bool, Option<Uuid>)> =
        sqlx::query_as("SELECT is_onboarded, last_workspace_id FROM profiles WHERE user_id = $1")
            .bind(user_id)
            .fetch_optional(pool)
            .await?;
    let (is_onboarded, last_workspace_id) = profile.unwrap_or((false, None));
    if !is_onboarded {
        return Ok("onboarding".to_owned());
    }
    if let Some(last_id) = last_workspace_id {
        let row: Option<(String,)> = sqlx::query_as(
            r#"SELECT w.slug FROM workspaces w
               JOIN workspace_members wm ON wm.workspace_id = w.id
                 AND wm.member_id = $1 AND wm.is_active AND wm.deleted_at IS NULL
               WHERE w.id = $2 AND w.deleted_at IS NULL LIMIT 1"#,
        )
        .bind(user_id)
        .bind(last_id)
        .fetch_optional(pool)
        .await?;
        if let Some((slug,)) = row {
            return Ok(slug);
        }
    }
    let fallback: Option<(String,)> = sqlx::query_as(
        r#"SELECT w.slug FROM workspaces w
           JOIN workspace_members wm ON wm.workspace_id = w.id
             AND wm.member_id = $1 AND wm.is_active AND wm.deleted_at IS NULL
           WHERE w.deleted_at IS NULL ORDER BY w.created_at LIMIT 1"#,
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await?;
    if let Some((slug,)) = fallback {
        return Ok(slug);
    }
    let invites: Option<(i64,)> = sqlx::query_as(
        "SELECT COUNT(*) FROM workspace_member_invites WHERE email = $1 AND deleted_at IS NULL",
    )
    .bind(email)
    .fetch_optional(pool)
    .await?;
    if invites.is_some_and(|(n,)| n > 0) {
        return Ok("invitations".to_owned());
    }
    Ok("create-workspace".to_owned())
}

/// `user_login` (`authentication/utils/login.py:15-27`): `login()` writes
/// the auth trio, then `device_info`, then the session saves (the layer
/// persists the mutated handle on response).
fn login_session(
    handle: &SessionHandle,
    settings: &pidash_db::config::Settings,
    headers: &HeaderMap,
    remote_addr: Option<&str>,
    user: &ResolvedUser,
    is_app: bool,
    is_space: bool,
) {
    session_set(handle, "_auth_user_id", Value::String(user.id.to_string()));
    session_set(
        handle,
        "_auth_user_backend",
        Value::String("django.contrib.auth.backends.ModelBackend".to_owned()),
    );
    session_set(
        handle,
        "_auth_user_hash",
        Value::String(session_auth_hash(
            &user.password_field,
            settings.secret_key.as_bytes(),
        )),
    );
    let domain = if is_space {
        space_base(settings)
    } else if is_app {
        app_base(settings)
    } else {
        shapes::base_host(&host_settings(settings), false, false, false)
    };
    // `user_login` (`login.py:21-25`, D-16 `device_info` precedent): the
    // user-agent default is `""` when the header is absent (the login-stamp
    // `None` above stays NULL — different site, different parity).
    session_set(
        handle,
        "device_info",
        serde_json::json!({
            "user_agent": user_agent(headers).as_deref().unwrap_or(""),
            "ip_address": client_ip(headers, remote_addr),
            "domain": domain,
        }),
    );
}

// ---------------------------------------------------------------------------
// Callbacks (F11)
// ---------------------------------------------------------------------------

/// `GitHubCallbackEndpoint.get` (`views/app/github.py:67-106`): state
/// check, then missing-code check (both 5120), then the provider exchange
/// with `callback=post_user_auth_workflow`, `user_login(is_app=True)`,
/// and the safe redirect over `session_next_path or
/// get_redirection_path(user)`.
async fn app_callback(
    State(state): State<AppState>,
    extension: Option<Extension<SessionHandle>>,
    peer: ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    let settings = state.settings();
    let base = app_base(settings);
    let allowed = allowed_hosts(settings);
    let handle = session_handle(extension);
    let session_next = session_get(&handle, "next_path");
    let stored_state = session_get(&handle, "state").unwrap_or_default();

    // State-check FIRST (`github.py:72-81`): `None != ""` fires it when
    // `?state` is absent, exactly like Python.
    if query.get("state").map(String::as_str) != Some(stored_state.as_str()) {
        return callback_guard_error(&base, session_next.as_deref(), &allowed);
    }
    if query
        .get("code")
        .map(String::as_str)
        .unwrap_or("")
        .is_empty()
    {
        return callback_guard_error(&base, session_next.as_deref(), &allowed);
    }
    let code = query.get("code").cloned().unwrap_or_default();

    let Some(pool) = pool_of(&state) else {
        return server_error();
    };
    let secret = settings.secret_key.clone();
    // `REMOTE_ADDR` (`get_client_ip` fallback; serve installs the peer via
    // `into_make_service_with_connect_info`, so this is always present live).
    let remote = peer.0.ip().to_string();
    let remote_addr = Some(remote.as_str());
    let config = match github_config(&pool, &secret).await {
        Ok(config) => config,
        Err(_) => return server_error(),
    };
    // `GitHubOAuthProvider(request, code, callback)` raises
    // `GITHUB_NOT_CONFIGURED` inside the `try` (`github.py:52-57,92`),
    // caught into the error channel below.
    let redirect_uri = providers::redirect_uri(
        is_secure_request(&headers, settings),
        &request_host(&headers),
        "github",
    );
    let outcome: Result<String, CallbackFailure> = async {
        if !providers::github_configured(
            config.client_id.as_deref(),
            config.client_secret.as_deref(),
        ) {
            return Err(github_not_configured().into());
        }
        let auth = exchange_github(
            config.client_id.as_deref().unwrap_or(""),
            config.client_secret.as_deref().unwrap_or(""),
            config.organization_id.as_deref(),
            &code,
            &redirect_uri,
        )
        .await?;
        let email = sanitize_email(&auth.user_data.email).map_err(CallbackFailure::Auth)?;
        let user =
            complete_login_or_signup(&pool, &secret, &headers, remote_addr, &auth, &email).await?;
        login_session(&handle, settings, &headers, remote_addr, &user, true, false);
        // `path = next_path or get_redirection_path(user)`
        // (`github.py:98-101`): the RAW session value (possibly "").
        let path = match session_next.as_deref() {
            Some(next) if !next.is_empty() => next.to_owned(),
            _ => redirection_path(&pool, user.id, &email)
                .await
                .map_err(|_| CallbackFailure::Server)?,
        };
        let allowed_refs: Vec<&str> = allowed.iter().map(String::as_str).collect();
        Ok(shapes::get_safe_redirect_url(
            &base,
            &path,
            &[],
            &allowed_refs,
        ))
    }
    .await;
    match outcome {
        Ok(url) => redirect_response(url),
        Err(CallbackFailure::Auth(exc)) => redirect_response(error_location(
            &base,
            session_next.as_deref(),
            &exc,
            &allowed,
        )),
        Err(CallbackFailure::Server) => server_error(),
    }
}

/// `GitHubCallbackSpaceEndpoint.get` (`views/space/github.py:65-104`):
/// QUIRK-space-callback-500 — the `base_host` local shadows the helper,
/// so every branch raises `TypeError` before responding. This handler
/// answers 500 for every GET, as-is. (Non-GET methods proxy to Django,
///
/// which owns the 405.)
async fn space_callback(
    State(_state): State<AppState>,
    extension: Option<Extension<SessionHandle>>,
    Query(_query): Query<HashMap<String, String>>,
) -> Response {
    // Touch the session/params shape for signature parity with the Python
    // view (`code`/`state` reads, `host`/`next_path` session reads), then
    // the shadowed `base_host(...)` call raises before any branch can
    // answer — Django's 500 on every input.
    let handle = session_handle(extension);
    let _ = session_get(&handle, "host");
    let _ = session_get(&handle, "next_path");
    let _ = session_get(&handle, "state");
    server_error()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_settings() -> pidash_db::config::Settings {
        // No `WEB_URL`: `get_allowed_hosts` allows only
        // `WEB_URL or APP_BASE_URL` plus the space/admin bases
        // (`path_validator.py:70-86`), so setting both would push the app
        // base out of the allowlist — exactly like Python.
        let mut vars = HashMap::new();
        vars.insert("APP_BASE_URL".to_owned(), "http://app.test".to_owned());
        vars.insert("SPACE_BASE_URL".to_owned(), "http://space.test".to_owned());
        pidash_db::config::Settings::from_map_with(
            &vars,
            pidash_db::config::Profile::Common,
            &pidash_db::config::NoOverlay,
        )
        .expect("test settings")
    }

    fn test_state() -> AppState {
        AppState::with_settings("0.1.0", test_settings())
    }

    fn location_of(resp: &Response) -> String {
        resp.headers()
            .get(header::LOCATION)
            .expect("Location")
            .to_str()
            .expect("ascii")
            .to_owned()
    }

    // F10: the github initiate error codes.
    #[test]
    fn github_error_codes_match_fixture() {
        assert_eq!(github_not_configured().error_code, 5110);
        assert_eq!(
            github_not_configured().error_message,
            "GITHUB_NOT_CONFIGURED"
        );
        assert_eq!(github_provider_error().error_code, 5120);
        assert_eq!(
            github_provider_error().error_message,
            "GITHUB_OAUTH_PROVIDER_ERROR"
        );
    }

    // F10: error redirect wire shape — validated `next_path` first, then
    // the error dict in order (`path_validator.py:129-145`).
    #[test]
    fn error_location_byte_shape() {
        let settings = test_settings();
        let allowed = allowed_hosts(&settings);
        let url = error_location(
            "http://app.test",
            Some("/x"),
            &github_not_configured(),
            &allowed,
        );
        assert_eq!(
            url,
            "http://app.test/?next_path=/x&error_code=5110&error_message=GITHUB_NOT_CONFIGURED"
        );
        // Absolute next_path degrades to its path (`validate_next_path`
        // strips scheme/netloc); the hostile host never survives.
        let url = error_location(
            "http://app.test",
            Some("https://evil.example/x"),
            &github_not_configured(),
            &allowed,
        );
        assert_eq!(
            url,
            "http://app.test/?next_path=/x&error_code=5110&error_message=GITHUB_NOT_CONFIGURED"
        );
        assert!(!url.contains("evil.example"));
    }

    // F10: app vs space bases (`host.py`).
    #[test]
    fn base_hosts_match_python() {
        let settings = test_settings();
        assert_eq!(app_base(&settings), "http://app.test");
        assert_eq!(space_base(&settings), "http://space.test/spaces/");
    }

    // F10: provider auth_url shape (`github.py` url_params order).
    #[test]
    fn github_auth_url_shape() {
        let url = providers::github_auth_url("cid", false, "h.test", "ST", Some("org1"));
        assert!(
            url.starts_with("https://github.com/login/oauth/authorize?"),
            "{url}"
        );
        assert!(url.contains("client_id=cid"), "{url}");
        assert!(
            url.contains("redirect_uri=http%3A%2F%2Fh.test%2Fauth%2Fgithub%2Fcallback%2F"),
            "{url}"
        );
        assert!(
            url.contains("scope=read%3Auser+user%3Aemail+read%3Aorg"),
            "{url}"
        );
        assert!(url.contains("state=ST"), "{url}");
        // No org → base scope only.
        let plain = providers::github_auth_url("cid", false, "h.test", "ST", None);
        assert!(plain.contains("scope=read%3Auser+user%3Aemail"), "{plain}");
        assert!(!plain.contains("read%3Aorg"), "{plain}");
    }

    // F9/F10: session-key matrix — app initiates write the triple, space
    // github writes host+state only.
    #[test]
    fn initiate_session_keys_match_guards() {
        use crate::auth_oauth::guards::{initiate_session_keys, OauthProvider};
        assert_eq!(
            initiate_session_keys(false, OauthProvider::Github),
            &["host", "next_path", "state"]
        );
        assert_eq!(
            initiate_session_keys(true, OauthProvider::Github),
            &["host", "state"]
        );
    }

    // App callback with no session state: state-mismatch fires before any
    // pool access (no DB needed), 302 with 5120 and no session cookie
    // written by the handler itself.
    #[tokio::test]
    async fn app_callback_state_mismatch_needs_no_db() {
        let state = test_state();
        let resp = app_callback(
            State(state),
            None,
            ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 0))),
            HeaderMap::new(),
            Query(HashMap::from([("code".to_owned(), "c".to_owned())])),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::FOUND);
        let location = location_of(&resp);
        assert!(location.contains("error_code=5120"), "{location}");
        assert!(
            location.contains("GITHUB_OAUTH_PROVIDER_ERROR"),
            "{location}"
        );
    }

    // Missing code with matching state: same 5120 redirect. Seed the
    // session through the handle so the state check passes.
    #[tokio::test]
    async fn app_callback_missing_code_redirects() {
        let state = test_state();
        let handle = SessionHandle::new(crate::middleware::RequestSession::empty());
        session_set(&handle, "state", Value::String("S".to_owned()));
        session_set(&handle, "next_path", Value::String("/x".to_owned()));
        let resp = app_callback(
            State(state),
            Some(Extension(handle)),
            ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 0))),
            HeaderMap::new(),
            Query(HashMap::from([("state".to_owned(), "S".to_owned())])),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::FOUND);
        let location = location_of(&resp);
        assert!(location.contains("error_code=5120"), "{location}");
        // `get_safe_redirect_url` echoes the validated path RAW.
        assert!(location.contains("next_path=/x"), "{location}");
    }

    // F11 QUIRK-space-callback-500: every GET answers 500 without touching
    // the database (no pools on the state).
    #[tokio::test]
    async fn space_callback_always_500() {
        let state = test_state();
        for params in [
            HashMap::from([
                ("code".to_owned(), "c".to_owned()),
                ("state".to_owned(), "s".to_owned()),
            ]),
            HashMap::from([("code".to_owned(), "c".to_owned())]),
            HashMap::new(),
        ] {
            let resp = space_callback(State(state.clone()), None, Query(params)).await;
            assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
        }
    }

    // `user_login` device_info (`login.py:21-25`, D-16 precedent): an
    // absent User-Agent stores `""`, an absent IP stays null.
    #[test]
    fn login_device_info_absent_header_defaults() {
        let settings = test_settings();
        let handle = SessionHandle::new(crate::middleware::RequestSession::empty());
        let user = ResolvedUser {
            id: Uuid::new_v4(),
            password_field: "pw".to_owned(),
        };
        login_session(
            &handle,
            &settings,
            &HeaderMap::new(),
            None,
            &user,
            true,
            false,
        );
        let info = handle
            .lock()
            .get("device_info")
            .expect("device_info")
            .clone();
        assert_eq!(info["user_agent"], Value::String(String::new()));
        assert!(info["ip_address"].is_null());
    }

    // `sanitize_email` goldens (`adapter/base.py`).
    #[test]
    fn sanitize_email_goldens() {
        assert_eq!(
            sanitize_email(&Value::String("A@Example.COM ".to_owned())).expect("valid"),
            "a@example.com"
        );
        assert!(sanitize_email(&Value::Null).is_err());
        assert!(sanitize_email(&Value::String(String::new())).is_err());
        assert!(sanitize_email(&Value::String("not-an-email".to_owned())).is_err());
        let err = sanitize_email(&Value::Null).expect_err("invalid");
        assert_eq!(err.error_code, 5005);
        assert_eq!(err.payload, vec![("email".to_owned(), Value::Null)]);
    }

    // `str(value)` interpolation goldens.
    #[test]
    fn python_str_goldens() {
        assert_eq!(python_str(&Value::Null), "None");
        assert_eq!(python_str(&Value::Bool(true)), "True");
        assert_eq!(python_str(&Value::String("t".to_owned())), "t");
    }

    // `get_display_name` goldens (`user.py:190-197`).
    #[test]
    fn display_name_goldens() {
        assert_eq!(display_name_for_email("octo@example.com"), "octo");
        assert_eq!(display_name_for_email("a@b@c").len(), 6);
    }

    // Numeric provider ids (github userinfo `id`) str-coerce like Django's
    // `CharField` prep, before the F6 lookup/create.
    #[test]
    fn provider_id_coercion_goldens() {
        let mut data = serde_json::json!({"user": {"provider_id": 12345}});
        coerce_provider_id(&mut data);
        assert_eq!(
            data["user"]["provider_id"],
            Value::String("12345".to_owned())
        );
        let mut already = serde_json::json!({"user": {"provider_id": "12345"}});
        coerce_provider_id(&mut already);
        assert_eq!(
            already["user"]["provider_id"],
            Value::String("12345".to_owned())
        );
    }
}
