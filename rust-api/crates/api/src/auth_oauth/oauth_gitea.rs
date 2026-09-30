//! Gitea OAuth app + space initiate/callback handlers (D-17, PIDASHCONV-341).
//!
//! Port of `apps/api/pi_dash/authentication/views/app/gitea.py` (full;
//! `GiteaOauthInitiateEndpoint.get` + `GiteaCallbackEndpoint.get`) and
//! `apps/api/pi_dash/authentication/views/space/gitea.py` (full; the space
//! twins). Read-only reference: `provider/oauth/gitea.py` (already ported
//! as `pidash_services::auth_oauth::providers`, PIDASHCONV-325).
//!
//! Routes (`authentication/urls.py`):
//!
//! - `GET /auth/gitea/` — app initiate
//! - `GET /auth/gitea/callback/` — app callback
//! - `GET /auth/spaces/gitea/` — space initiate
//! - `GET /auth/spaces/gitea/callback/` — space callback
//!
//! Fixture ids: AUTHOAUTH-F10 (initiate redirect goldens) + AUTHOAUTH-F11
//! (callback branch goldens) under `rust-api/fixtures/auth_oauth/`.
//!
//! Layering: this module owns the HTTP shell (route registration, session
//! reads/writes, the 302 `Location` builders) plus the view-ordered
//! orchestration of the success path. Every pure decision (provider auth
//! URLs, host normalization, `validate_next_path`, error codes, exchange
//! error mapping, account-upsert SQL, user/invitation SQL builders) is
//! reused read-only from the merged foundation kernels; nothing here
//! re-derives them.
//!
//! Ported quirks (translate, don't redesign; also listed in the PR):
//!
//! * BUG-app-gitea-urljoin (`views/app/gitea.py`): the app views build
//!   error redirects with `urljoin(base, "?" + urlencode(params))`, which
//!   drops the base path — an app base without trailing slash yields an
//!   empty-path `Location`, and a missing session host yields a bare
//!   relative `?error_code=...` ref ([`urljoin`]).
//! * The space views use an f-string (`f"{base}?{params}"`), never
//!   `urljoin` — different wire shape for identical values.
//! * App state-mismatch echoes `next_path` raw (`str(next_path)`); the
//!   missing-code and provider-error branches echo it validated
//!   (`validate_next_path`). Space echoes validated on every branch.
//! * Space success never runs the invitation workflow: the space callback
//!   constructs its provider without `callback=post_user_auth_workflow`
//!   (despite the "Process workspace and project invitations" comment),
//!   so `complete_login_or_signup` skips the callback.
//! * `is_signup = bool(user)` is inverted as written (`base.py:297`): a
//!   brand-new user reports `False`, so IDP sync also runs for new users
//!   when `ENABLE_GITEA_SYNC` is set.
//! * New-user avatar assignments persist only via the later
//!   `save_user_data` save (no save follows the avatar block itself).
//!
//! Out of scope (sibling issues): google/github/gitlab twins
//! (PIDASHCONV-335/336/339), device endpoints (PIDASHCONV-342/343),
//! the domain gate (PIDASHCONV-344).
//!
//! # Wiring note
//!
//! [`routes`] registers exactly these four GETs. Like the pilot-2 list
//! family, unowned methods proxy to Django (plain `View`s sit behind
//! `CsrfViewMiddleware`, so e.g. POST answers Django's own responses,
//! and answering 405 in Rust would break the contract).
#![forbid(unsafe_code)]

use std::collections::HashMap;

use axum::extract::{Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Extension;
use axum::Router;

use crate::middleware::SessionHandle;
use crate::state::AppState;

// ---------------------------------------------------------------------------
// §1 Constants (authentication/adapter/error.py:41-50, the `# Oauth` block)
// ---------------------------------------------------------------------------

/// `AUTHENTICATION_ERROR_CODES["INSTANCE_NOT_CONFIGURED"]` (`error.py:14`).
pub const INSTANCE_NOT_CONFIGURED_CODE: i32 = 5000;
/// `AUTHENTICATION_ERROR_CODES["INSTANCE_NOT_CONFIGURED"]` name.
pub const INSTANCE_NOT_CONFIGURED_NAME: &str = "INSTANCE_NOT_CONFIGURED";
/// `AUTHENTICATION_ERROR_CODES["GITEA_NOT_CONFIGURED"]` (`error.py:46`).
pub const GITEA_NOT_CONFIGURED_CODE: i32 = 5112;
/// `AUTHENTICATION_ERROR_CODES["GITEA_NOT_CONFIGURED"]` name.
pub const GITEA_NOT_CONFIGURED_NAME: &str = "GITEA_NOT_CONFIGURED";
/// `AUTHENTICATION_ERROR_CODES["GITEA_OAUTH_PROVIDER_ERROR"]` (`error.py:50`).
pub const GITEA_PROVIDER_ERROR_CODE: i32 = 5123;
/// `AUTHENTICATION_ERROR_CODES["GITEA_OAUTH_PROVIDER_ERROR"]` name.
pub const GITEA_PROVIDER_ERROR_NAME: &str = "GITEA_OAUTH_PROVIDER_ERROR";

/// Provider key (`GiteaOAuthProvider.provider`, `gitea.py:22`).
pub const PROVIDER: &str = "gitea";

/// Session keys the four views read and write (`app/gitea.py`, `space/gitea.py`).
pub const SESSION_HOST: &str = "host";
pub const SESSION_NEXT_PATH: &str = "next_path";
pub const SESSION_STATE: &str = "state";

/// `HttpResponseRedirect.status_code`.
pub const REDIRECT_STATUS: u16 = 302;

// ---------------------------------------------------------------------------
// §2 Pure kernels: query encoding, urljoin, redirect builders
// ---------------------------------------------------------------------------

/// One query value, repeated or not. Mirrors the D-26 `app_issues` shape:
/// axum's `Query` backend does not coerce a lone `?key=value` into a
/// sequence, so callers read the last value like Django's `QueryDict.get`.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(untagged)]
pub enum OneOrMany {
    One(String),
    Many(Vec<String>),
}

/// The multi-value query map every handler extracts.
pub type QueryMap = HashMap<String, OneOrMany>;

/// Django `QueryDict.get`: the last value, or `None`.
pub fn query_last(query: &QueryMap, key: &str) -> Option<String> {
    query.get(key).and_then(|value| match value {
        OneOrMany::One(one) => Some(one.clone()),
        OneOrMany::Many(many) => many.last().cloned(),
    })
}

/// `urllib.parse.urlencode` over an ordered pair list (insertion order is
/// the `get_error_dict` order: `error_code`, `error_message`, then
/// `next_path`). Reuses the F3 kernel; the error-param order is what makes
/// the bytes identical.
pub fn urlencode_pairs(pairs: &[(String, String)]) -> String {
    let refs: Vec<(&str, &str)> = pairs
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    pidash_services::auth_oauth::providers::urlencode(&refs)
}

/// Ordered error params: `exc.get_error_dict()` (`error.py:88-92`) plus an
/// optional `next_path` echo appended last (`app/gitea.py`, `space/gitea.py`).
pub fn error_params(code: i32, name: &str, next_path: Option<&str>) -> Vec<(String, String)> {
    let mut out = vec![
        ("error_code".to_owned(), code.to_string()),
        ("error_message".to_owned(), name.to_owned()),
    ];
    if let Some(next) = next_path {
        out.push(("next_path".to_owned(), next.to_owned()));
    }
    out
}

/// Split `http://host/path` into `(scheme, authority, path)`. Returns
/// `(None, None, base)` when no `://` is present (relative/empty base).
fn split_base(base: &str) -> (Option<&str>, Option<&str>, &str) {
    match base.find("://") {
        Some(end) => {
            let scheme = &base[..end];
            let rest = &base[end + 3..];
            let path_start = rest.find('/').unwrap_or(rest.len());
            let authority = &rest[..path_start];
            let path = &rest[path_start..];
            (Some(scheme), Some(authority), path)
        }
        None => (None, None, base),
    }
}

/// CPython `urllib.parse.urljoin(base, reference)` for the reference shapes
/// these views use (`views/app/gitea.py`):
///
/// - query-only refs (`"?error_code=..."`): the base path is kept and any
///   base query/fragment is replaced — notably `urljoin("http://h", "?q")`
///   yields `"http://h?q"` (empty path), the F10 app-gitea golden;
/// - absolute-path refs (`"/x"`): the base path is replaced;
/// - relative refs (`"onboarding"`): merged onto the base directory.
///
/// A `None`/empty base behaves like CPython's falsy-to-`""` coercion
/// (`_decode_args` maps falsy to `""`): the ref returns unchanged, which is
/// the F11 relative-`Location` quirk (`urljoin(None, "?error...")`).
pub fn urljoin(base: Option<&str>, reference: &str) -> String {
    let base = base.unwrap_or("");
    if base.is_empty() {
        return reference.to_owned();
    }
    if reference.starts_with('?') || reference.starts_with('#') {
        let (scheme, authority, path) = split_base(base);
        match (scheme, authority) {
            (Some(s), Some(a)) => format!("{s}://{a}{path}{reference}"),
            _ => format!("{base}{reference}"),
        }
    } else if reference.starts_with('/') {
        let (scheme, authority, _) = split_base(base);
        match (scheme, authority) {
            (Some(s), Some(a)) => format!("{s}://{a}{reference}"),
            _ => reference.to_owned(),
        }
    } else if reference.is_empty() {
        base.to_owned()
    } else {
        let (scheme, authority, path) = split_base(base);
        match (scheme, authority) {
            (Some(s), Some(a)) => {
                let dir = path.rsplit_once('/').map_or("", |(d, _)| d);
                if dir.is_empty() {
                    format!("{s}://{a}/{reference}")
                } else {
                    format!("{s}://{a}{dir}/{reference}")
                }
            }
            _ => reference.to_owned(),
        }
    }
}

/// App error redirect (`app/gitea.py:47,60`):
/// `urljoin(base_host(is_app=True), "?" + urlencode(params))`.
pub fn app_error_location(app_base: &str, params: &[(String, String)]) -> String {
    urljoin(Some(app_base), &format!("?{}", urlencode_pairs(params)))
}

/// App callback error redirect (`app/gitea.py:82,95,106`):
/// `urljoin(request.session.get("host"), "?" + urlencode(params))` — the
/// session host shadows the `base_host` import but is never called here,
/// so no 500; a missing host yields the relative ref (quirk, F11).
pub fn app_callback_error_location(
    session_host: Option<&str>,
    params: &[(String, String)],
) -> String {
    urljoin(session_host, &format!("?{}", urlencode_pairs(params)))
}

/// Space error redirect (`space/gitea.py:48,64` + callback twins):
/// `f"{base_host(is_space=True)}?{urlencode(params)}"` — an f-string, never
/// `urljoin`, so the base path survives (differs from the app twin).
pub fn space_error_location(space_base: &str, params: &[(String, String)]) -> String {
    format!("{space_base}?{}", urlencode_pairs(params))
}

/// App success redirect (`app/gitea.py:102`):
/// `urljoin(base_host, validated_next_path or get_redirection_path(user))`.
pub fn app_success_location(session_host: Option<&str>, path: &str) -> String {
    urljoin(session_host, path)
}

/// Space success redirect (`space/gitea.py:109-111`):
/// `f"{base_host(is_space=True)}{validated_next_path or ''}"`.
pub fn space_success_location(space_base: &str, validated_next_path: &str) -> String {
    format!("{space_base}{validated_next_path}")
}

/// `redirect_uri` (`gitea.py:69`):
/// `f"{'https' if request.is_secure() else 'http'}://{request.get_host()}/auth/gitea/callback/"`.
pub fn redirect_uri_for(is_secure: bool, host: &str) -> String {
    let scheme = if is_secure { "https" } else { "http" };
    format!("{scheme}://{host}/auth/gitea/callback/")
}

/// First `X-Forwarded-For` entry, unstripped, else the peer address
/// (`utils/ip_address.py:get_client_ip`). Either leg may be absent.
pub fn client_ip(x_forwarded_for: Option<&str>, remote_addr: Option<&str>) -> Option<String> {
    if let Some(forwarded) = x_forwarded_for {
        if !forwarded.is_empty() {
            return Some(forwarded.split(',').next().unwrap_or("").to_owned());
        }
    }
    remote_addr.map(str::to_owned)
}

/// `device_info` dict `user_login` stores in the session
/// (`authentication/utils/login.py:21-25`). The user-agent default is `""`
/// when the header is absent; a missing IP stays JSON null.
pub fn device_info_value(
    user_agent: Option<&str>,
    ip: Option<&str>,
    domain: &str,
) -> serde_json::Value {
    serde_json::json!({
        "user_agent": user_agent.unwrap_or(""),
        "ip_address": ip,
        "domain": domain,
    })
}

// ---------------------------------------------------------------------------
// §3 Session + request plumbing
// ---------------------------------------------------------------------------

/// Snapshot of the OAuth session keys a callback reads
/// (`app/gitea.py:72-75`, `space/gitea.py:76-78`). Reads are tolerant:
/// a missing key is `None`, a non-string value reads as absent — the
/// session dict only ever holds strings here.
#[derive(Debug, Clone, Default)]
pub struct CallbackSession {
    pub host: Option<String>,
    pub state: Option<String>,
    pub next_path: Option<String>,
}

fn session_str(session: &mut crate::middleware::RequestSession, key: &str) -> Option<String> {
    session.get(key).and_then(|v| v.as_str()).map(str::to_owned)
}

/// Read the callback session keys off the live handle. `None` (no session
/// layer — transparent mode) reads as an empty session, matching Django
/// behind the proxy owning sessions meanwhile.
pub fn read_callback_session(handle: &Option<Extension<SessionHandle>>) -> CallbackSession {
    match handle {
        Some(Extension(handle)) => {
            let mut session = handle.snapshot();
            CallbackSession {
                host: session_str(&mut session, SESSION_HOST),
                state: session_str(&mut session, SESSION_STATE),
                next_path: session_str(&mut session, SESSION_NEXT_PATH),
            }
        }
        None => CallbackSession::default(),
    }
}

/// Write one session key on the live handle (initiate writes). No-op
/// without a session layer.
pub fn write_session_key(handle: &Option<Extension<SessionHandle>>, key: &str, value: String) {
    if let Some(Extension(handle)) = handle {
        handle
            .lock()
            .set(key.to_owned(), serde_json::Value::String(value));
    }
}

/// `request.get_host()` (`django/http/request.py`): the `Host` header.
/// Axum serves only what the listener received, so the header is the host.
pub fn request_host(headers: &HeaderMap) -> String {
    headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_owned()
}

/// `request.is_secure()` for this deployment: the proxy terminates TLS and
/// forwards `X-Forwarded-Proto`, which is what Django's
/// `SECURE_PROXY_SSL_HEADER` reads. Absent header means plain HTTP, exactly
/// like Django without the header configured.
pub fn request_is_secure(headers: &HeaderMap) -> bool {
    headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|proto| proto.split(',').next().unwrap_or("").trim() == "https")
}

/// App/space base hosts from settings (`authentication/utils/host.py:19-67`
/// via the D-16 `base_host` kernel). `is_app` returns `APP_BASE_URL` when
/// set, else the `WEB_URL or APP_BASE_URL` origin; `is_space` appends the
/// space base path to `SPACE_BASE_URL` when set, else to the origin.
pub fn app_base(settings: &pidash_db::config::Settings) -> String {
    let urls = &settings.urls;
    pidash_services::auth_session::shapes::base_host(
        &pidash_services::auth_session::shapes::HostSettings {
            web_url: urls.web_url.as_deref(),
            app_base_url: urls.app_base_url.as_deref(),
            admin_base_url: urls.admin_base_url.as_deref(),
            space_base_url: urls.space_base_url.as_deref(),
            admin_base_path: Some(urls.admin_base_path.as_str()),
            space_base_path: Some(urls.space_base_path.as_str()),
        },
        false,
        false,
        true,
    )
}

/// Space base host from settings (same kernel, `is_space=True`).
pub fn space_base(settings: &pidash_db::config::Settings) -> String {
    let urls = &settings.urls;
    pidash_services::auth_session::shapes::base_host(
        &pidash_services::auth_session::shapes::HostSettings {
            web_url: urls.web_url.as_deref(),
            app_base_url: urls.app_base_url.as_deref(),
            admin_base_url: urls.admin_base_url.as_deref(),
            space_base_url: urls.space_base_url.as_deref(),
            admin_base_path: Some(urls.admin_base_path.as_str()),
            space_base_path: Some(urls.space_base_path.as_str()),
        },
        false,
        true,
        false,
    )
}

/// Handler failure with its exact status + body.
#[derive(Debug)]
pub enum Denial {
    /// 500, `handle_exception` generic branch (`views/base.py:92-95`).
    ServerError,
    /// 302, error redirect (every `AuthenticationException` branch).
    Redirect(String),
}

impl Denial {
    fn status_and_body(&self) -> (StatusCode, Option<String>) {
        match self {
            Denial::ServerError => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Some(crate::license::SERVER_ERROR_BODY.to_owned()),
            ),
            Denial::Redirect(_) => (StatusCode::FOUND, None),
        }
    }
}

impl IntoResponse for Denial {
    fn into_response(self) -> Response {
        match self {
            Denial::Redirect(location) => {
                (StatusCode::FOUND, [(header::LOCATION, location)]).into_response()
            }
            denial => {
                let (status, body) = denial.status_and_body();
                (status, body.unwrap_or_default()).into_response()
            }
        }
    }
}

/// 302 `Location` response.
pub fn redirect(location: String) -> Response {
    (StatusCode::FOUND, [(header::LOCATION, location)]).into_response()
}

// ---------------------------------------------------------------------------
// §4 Instance gate + provider configuration (initiate + callback shared)
// ---------------------------------------------------------------------------

/// `Instance.objects.first()` projected to the setup gate: the
/// `SoftDeleteManager` adds `deleted_at IS NULL`, `Meta.ordering =
/// ("-created_at",)` picks the latest row (D-16 `instance_first_sql`
/// semantics over `instances`).
pub async fn instance_setup_done(pool: &sqlx::PgPool) -> Result<Option<bool>, sqlx::Error> {
    let row: Option<(bool,)> = sqlx::query_as(
        r#"SELECT "is_setup_done" FROM "instances"
           WHERE "deleted_at" IS NULL ORDER BY "created_at" DESC LIMIT 1"#,
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|(done,)| done))
}

/// One `instance_configurations` read with the caller's env default — the
/// `get_configuration_value` shape for a db-sourced key
/// (`license/utils/instance_value.py:28-54`, same as the D-33
/// `config_value`): the stored row wins when present (decrypted when
/// `is_encrypted`), otherwise the default (the `os.environ.get(...)` the
/// view passes in). Soft-deleted rows are invisible.
pub async fn config_value(
    pool: &sqlx::PgPool,
    secret_key: &str,
    key: &str,
    env_default: Option<String>,
) -> Result<Option<String>, sqlx::Error> {
    let row: Option<(Option<String>, bool)> = sqlx::query_as(
        r#"SELECT value, is_encrypted FROM instance_configurations
           WHERE key = $1 AND deleted_at IS NULL"#,
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

fn env_default(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

/// Resolved Gitea provider configuration
/// (`GiteaOAuthProvider.__init__`, `gitea.py:24-50`).
pub struct GiteaConfig {
    pub client_id: String,
    pub client_secret: String,
    /// `GITEA_HOST` after scheme enforcement + `rstrip("/")` (`:52-57`).
    pub host_normalized: String,
}

/// Read the three Gitea settings, raising `GITEA_NOT_CONFIGURED` exactly
/// where the provider constructor raises (`gitea.py:37-57`): missing/empty
/// id, secret, or host first, then a host whose scheme is not http/https
/// (no detail leak — same code and message).
pub async fn gitea_config(
    pool: &sqlx::PgPool,
    secret_key: &str,
) -> Result<GiteaConfig, pidash_services::auth_oauth::error::AuthenticationException> {
    use pidash_services::auth_oauth::error::AuthenticationException;
    use pidash_services::auth_oauth::providers::{gitea_configured, normalize_gitea_host};
    let not_configured = || {
        AuthenticationException::new(
            GITEA_NOT_CONFIGURED_CODE,
            GITEA_NOT_CONFIGURED_NAME,
            Vec::new(),
        )
    };
    let client_id = config_value(
        pool,
        secret_key,
        "GITEA_CLIENT_ID",
        env_default("GITEA_CLIENT_ID"),
    )
    .await
    .map_err(|_| not_configured())?;
    let client_secret = config_value(
        pool,
        secret_key,
        "GITEA_CLIENT_SECRET",
        env_default("GITEA_CLIENT_SECRET"),
    )
    .await
    .map_err(|_| not_configured())?;
    let host = config_value(pool, secret_key, "GITEA_HOST", env_default("GITEA_HOST"))
        .await
        .map_err(|_| not_configured())?;
    if !gitea_configured(
        client_id.as_deref(),
        client_secret.as_deref(),
        host.as_deref(),
    ) {
        return Err(not_configured());
    }
    let host_normalized =
        normalize_gitea_host(host.as_deref().unwrap_or("")).map_err(|_| not_configured())?;
    Ok(GiteaConfig {
        client_id: client_id.unwrap_or_default(),
        client_secret: client_secret.unwrap_or_default(),
        host_normalized,
    })
}

// ---------------------------------------------------------------------------
// §5 Token/userinfo exchange (OauthAdapter.get_user_token/get_user_response
// + GiteaOAuthProvider.set_token_data/set_user_data)
// ---------------------------------------------------------------------------

use pidash_services::auth_oauth::error::AuthenticationException;

fn provider_error() -> AuthenticationException {
    AuthenticationException::new(
        GITEA_PROVIDER_ERROR_CODE,
        GITEA_PROVIDER_ERROR_NAME,
        Vec::new(),
    )
}

/// `OauthAdapter.get_user_token` (`adapter/oauth.py:75-84`): form-POST the
/// token URL; any transport error or non-2xx maps to the provider error
/// (warn + raise, via the D-17 exchange kernel's code).
pub async fn post_token(
    client: &reqwest::Client,
    token_url: &str,
    data: &[(String, String)],
    headers: &[(String, String)],
) -> Result<serde_json::Value, AuthenticationException> {
    let mut request = client.post(token_url).form(data);
    for (name, value) in headers {
        request = request.header(name.as_str(), value.as_str());
    }
    let response = request.send().await.map_err(|_| provider_error())?;
    if !response.status().is_success() {
        return Err(pidash_services::auth_oauth::exchange::map_exchange_error(
            PROVIDER,
        ));
    }
    response_json(response).await
}

/// Read a JSON body without reqwest's `json` feature (kept off crate-wide
/// so relayed bytes are never transcoded): bytes + `serde_json`, with the
/// same provider-error mapping as a transport failure.
async fn response_json(
    response: reqwest::Response,
) -> Result<serde_json::Value, AuthenticationException> {
    let bytes = response.bytes().await.map_err(|_| provider_error())?;
    serde_json::from_slice(&bytes).map_err(|_| provider_error())
}

/// `OauthAdapter.get_user_response` (`adapter/oauth.py:86-100`): GET the
/// userinfo URL under `Bearer <access_token>`; same error mapping.
pub async fn get_userinfo(
    client: &reqwest::Client,
    userinfo_url: &str,
    access_token: &str,
) -> Result<serde_json::Value, AuthenticationException> {
    let response = client
        .get(userinfo_url)
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|_| provider_error())?;
    if !response.status().is_success() {
        return Err(pidash_services::auth_oauth::exchange::map_exchange_error(
            PROVIDER,
        ));
    }
    response_json(response).await
}

/// `GiteaOAuthProvider.set_token_data` (`gitea.py:88-114`): POST
/// `{code, client_id, client_secret, redirect_uri, grant_type:
/// authorization_code}` with `Accept: application/json`, then store the
/// mapped token fields. Pure mapping via the F4 kernel; `now` stamps the
/// `expires_in` expiry exactly like the Python `timedelta` sum.
pub async fn fetch_token_data(
    client: &reqwest::Client,
    config: &GiteaConfig,
    code: &str,
    redirect_uri: &str,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<pidash_services::auth_oauth::providers::TokenData, AuthenticationException> {
    let token_url =
        pidash_services::auth_oauth::providers::gitea_token_url(&config.host_normalized);
    let data: Vec<(String, String)> =
        pidash_services::auth_oauth::providers::gitea_token_post_data(
            code,
            &config.client_id,
            &config.client_secret,
            redirect_uri,
        )
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v))
        .collect();
    let headers: Vec<(String, String)> =
        pidash_services::auth_oauth::providers::TOKEN_JSON_ACCEPT_HEADERS
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
    let token_response = post_token(client, &token_url, &data, &headers).await?;
    Ok(pidash_services::auth_oauth::providers::gitea_token_data(
        &token_response,
        now,
    ))
}

/// `GiteaOAuthProvider.set_user_data` (`gitea.py:149-173`): GET the userinfo,
/// fall back to the emails endpoint when `email` is absent
/// (`__get_email`, `:116-147`: non-OK, empty list, or transport failure all
/// raise the provider error), then store the mapped user fields.
pub async fn fetch_user_data(
    client: &reqwest::Client,
    config: &GiteaConfig,
    access_token: &str,
) -> Result<pidash_services::auth_oauth::providers::ProviderUserData, AuthenticationException> {
    let userinfo_url =
        pidash_services::auth_oauth::providers::gitea_userinfo_url(&config.host_normalized);
    let userinfo = get_userinfo(client, &userinfo_url, access_token).await?;
    let email = match userinfo.get("email").and_then(|v| v.as_str()) {
        Some(email) if !email.is_empty() => serde_json::Value::String(email.to_owned()),
        _ => {
            let emails_url =
                pidash_services::auth_oauth::providers::gitea_emails_url(&userinfo_url);
            let response = client
                .get(emails_url)
                .header("Accept", "application/json")
                .bearer_auth(access_token)
                .send()
                .await
                .map_err(|_| provider_error())?;
            if !response.status().is_success() {
                return Err(provider_error());
            }
            let emails: serde_json::Value = response_json(response).await?;
            let list = emails.as_array().ok_or_else(provider_error)?;
            if list.is_empty() {
                return Err(provider_error());
            }
            pidash_services::auth_oauth::providers::gitea_preferred_email(list)
                .map_err(|_| provider_error())?
        }
    };
    Ok(pidash_services::auth_oauth::providers::gitea_user_data(
        &userinfo, email,
    ))
}

/// `Adapter.sanitize_email` (`adapter/base.py:62-89`): missing email raises
/// `INVALID_EMAIL` (5005) with the raw email as payload; the survivors are
/// lowered + stripped, then Django-`validate_email` rejects with the same
/// code. Reuses the D-01 email kernel (verified against Django 4.2.30).
pub fn sanitize_email(raw: Option<&serde_json::Value>) -> Result<String, AuthenticationException> {
    use crate::license::handlers_auth_forms::{email_is_valid, normalize_email};
    let invalid = |email: serde_json::Value| {
        AuthenticationException::new(5005, "INVALID_EMAIL", vec![("email".to_owned(), email)])
    };
    let text = match raw {
        Some(serde_json::Value::String(s)) if !s.is_empty() => s.clone(),
        Some(other) => return Err(invalid(other.clone())),
        None => return Err(invalid(serde_json::Value::Null)),
    };
    // `str(email).lower().strip()`: non-string payloads stringify first,
    // but every provider emits a string or null, so only those two arms
    // are reachable — same observable behavior.
    let cleaned = normalize_email(&text);
    if !email_is_valid(&cleaned) {
        return Err(invalid(serde_json::Value::String(cleaned)));
    }
    Ok(cleaned)
}

// ---------------------------------------------------------------------------
// §6 complete_login_or_signup: user lookup/create, sync, save
// (`adapter/base.py:289-360`)
// ---------------------------------------------------------------------------

/// Columns the signup/login flow reads from `users`
/// (`db/models/user.py`, table `users`).
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct UserRow {
    pub id: uuid::Uuid,
    pub email: Option<String>,
    pub password: String,
    pub is_active: bool,
    pub first_name: String,
    pub last_name: String,
    pub display_name: String,
    pub avatar: String,
    pub avatar_asset_id: Option<uuid::Uuid>,
}

/// `User.objects.filter(email=email).first()` (`base.py:295`).
pub async fn find_user_by_email(
    pool: &sqlx::PgPool,
    email: &str,
) -> Result<Option<UserRow>, sqlx::Error> {
    sqlx::query_as::<_, UserRow>(
        r#"SELECT id, email, password, is_active, first_name, last_name,
                  display_name, avatar, avatar_asset_id
           FROM users WHERE email = $1 LIMIT 1"#,
    )
    .bind(email)
    .fetch_optional(pool)
    .await
}

/// `Adapter.__check_signup` (`base.py:103-121`): when `ENABLE_SIGNUP` is
/// `"0"` and no `WorkspaceMemberInvite` row carries the email, signup
/// raises `SIGNUP_DISABLED` (5015) with the email as payload.
pub async fn check_signup(
    pool: &sqlx::PgPool,
    secret_key: &str,
    email: &str,
) -> Result<(), AuthenticationException> {
    let flag = config_value(
        pool,
        secret_key,
        "ENABLE_SIGNUP",
        env_default("ENABLE_SIGNUP"),
    )
    .await
    .map_err(|_| {
        AuthenticationException::new(
            5015,
            "SIGNUP_DISABLED",
            vec![(
                "email".to_owned(),
                serde_json::Value::String(email.to_owned()),
            )],
        )
    })?;
    let enabled = flag.as_deref().unwrap_or("1");
    if enabled != "0" {
        return Ok(());
    }
    let invited: Option<(uuid::Uuid,)> =
        sqlx::query_as(r#"SELECT id FROM workspace_member_invites WHERE email = $1 LIMIT 1"#)
            .bind(email)
            .fetch_optional(pool)
            .await
            .map_err(|_| {
                AuthenticationException::new(
                    5015,
                    "SIGNUP_DISABLED",
                    vec![(
                        "email".to_owned(),
                        serde_json::Value::String(email.to_owned()),
                    )],
                )
            })?;
    if invited.is_none() {
        return Err(AuthenticationException::new(
            5015,
            "SIGNUP_DISABLED",
            vec![(
                "email".to_owned(),
                serde_json::Value::String(email.to_owned()),
            )],
        ));
    }
    Ok(())
}

/// `User.get_display_name` (`db/models/user.py:190-197`): the email local
/// part when it splits into exactly two `@` parts, else six random ASCII
/// letters.
pub fn display_name_for<R: rand::Rng>(email: &str, rng: &mut R) -> String {
    if !email.is_empty() {
        let parts: Vec<&str> = email.split('@').collect();
        if parts.len() == 2 {
            return parts[0].to_owned();
        }
    }
    // `string.ascii_letters` (letters only — not alphanumerics).
    const LETTERS: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";
    let mut out = String::with_capacity(6);
    for _ in 0..6 {
        out.push(LETTERS[rng.random_range(0..LETTERS.len())] as char);
    }
    out
}

/// Django `BasePasswordHasher.salt()`: 22 alphanumeric characters
/// (128 bits of entropy).
pub fn random_salt<R: rand::Rng>(rng: &mut R) -> String {
    use rand::distr::{Alphanumeric, DistString};
    Alphanumeric.sample_string(rng, 22)
}

/// `make_password` at the project's iteration count (600000 under the
/// pinned Django 4.2.30; the hasher writes the count into the string).
pub fn encode_password(password: &str, salt: &str) -> String {
    crate::license::handlers_auth_forms::encode_password(password, salt, 600_000)
}

/// New-user row (`base.py:299-328` + `User.save()` side effects,
/// `user.py:167-187`): every NOT NULL `users` column is supplied —
/// Django field defaults apply client-side, so the INSERT carries them
/// verbatim. `display_name` is backfilled from the email local part by
/// `save()` (never empty at INSERT); `token` stays `""` until
/// `save_user_data` rotates it (rotation only fires when
/// `token_updated_at` is set).
#[allow(clippy::too_many_arguments)]
pub async fn insert_user(
    pool: &sqlx::PgPool,
    id: uuid::Uuid,
    email: &str,
    username: &str,
    password_hash: &str,
    first_name: &str,
    last_name: &str,
    display_name: &str,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"INSERT INTO users
           (id, username, mobile_number, email, display_name, first_name, last_name,
            avatar, avatar_asset_id, cover_image, cover_image_asset_id,
            date_joined, created_at, updated_at,
            last_location, created_location,
            is_superuser, is_managed, is_password_expired, is_active, is_staff,
            is_email_verified, is_password_autoset, is_password_reset_required,
            token, user_timezone, is_email_valid,
            last_active, last_login_time, last_logout_time,
            last_login_ip, last_logout_ip, last_login_medium, last_login_uagent,
            token_updated_at, is_bot, bot_type, masked_at,
            password)
           VALUES ($1,$2,NULL,$3,$4,$5,$6,'',NULL,NULL,NULL,
                   $7,$7,$7,'','',FALSE,FALSE,FALSE,TRUE,FALSE,
                   TRUE,TRUE,FALSE,'','UTC',FALSE,
                   NULL,NULL,NULL,'','',$8,'',NULL,FALSE,NULL,NULL,$9)"#,
    )
    .bind(id)
    .bind(username)
    .bind(email)
    .bind(display_name)
    .bind(first_name)
    .bind(last_name)
    .bind(now)
    .bind("email")
    .bind(password_hash)
    .execute(pool)
    .await?;
    Ok(())
}

/// `save_user_data` (`base.py:220-234`): login enrichment plus the
/// activation branch. The UPDATE writes the D-16 `SAVE_USER_DATA_FIELDS`
/// plus the avatar pair the signup block assigned in memory (persisted only
/// by this save) and the rotated `token` (`User.save()` rotates whenever
/// `token_updated_at` is set — always, here). Returns whether the
/// activation email must fire (`not user.is_active` before the save).
#[allow(clippy::too_many_arguments)]
pub async fn save_user_data(
    pool: &sqlx::PgPool,
    user_id: uuid::Uuid,
    was_active: bool,
    ip: Option<&str>,
    user_agent: Option<&str>,
    avatar: &str,
    avatar_asset_id: Option<uuid::Uuid>,
    rotated_token: &str,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<bool, sqlx::Error> {
    sqlx::query(
        r#"UPDATE users SET
             last_login_medium = 'gitea',
             last_active = $2, last_login_time = $2, token_updated_at = $2,
             token = $3,
             last_login_ip = $4, last_login_uagent = $5,
             avatar = $6, avatar_asset_id = $7,
             is_active = TRUE
           WHERE id = $1"#,
    )
    .bind(user_id)
    .bind(now)
    .bind(rotated_token)
    .bind(ip.unwrap_or(""))
    .bind(user_agent.unwrap_or(""))
    .bind(avatar)
    .bind(avatar_asset_id)
    .execute(pool)
    .await?;
    Ok(!was_active)
}

/// `user_activation_email.delay(current_site, user.id)`: the Celery message
/// for the Python worker, enqueued through the Postgres queue like every
/// other handler-side publish (the worker/forwarding plane delivers it;
/// cf. the D-33 soft-delete enqueue). A queue failure raises in Python
/// (`.delay` propagates), so it surfaces as a 500 here too.
pub async fn enqueue_activation_email(
    pool: &sqlx::PgPool,
    current_site: &str,
    user_id: uuid::Uuid,
) -> Result<(), sqlx::Error> {
    let message = pidash_jobs::celery::CeleryTaskMessage::new(
        "pi_dash.bgtasks.user_activation_email_task.user_activation_email",
        vec![
            serde_json::Value::String(current_site.to_owned()),
            serde_json::Value::String(user_id.to_string()),
        ],
        Default::default(),
    );
    let job = pidash_jobs::queue::NewJob::new(
        message.task.clone(),
        serde_json::Value::Array(message.args.clone()),
        serde_json::Value::Object(message.kwargs.clone()),
    );
    pidash_jobs::queue::enqueue(pool, &job).await?;
    Ok(())
}

/// `sync_user_data` (`base.py:256-287`): runs when IDP sync is enabled and
/// the (inverted) signup flag says so. Names + `display_name` (regenerated
/// when the provider ships none), then the avatar pair via
/// `delete_old_avatar` + `download_and_upload_avatar`.
pub async fn sync_user_data(
    pool: &sqlx::PgPool,
    user_id: uuid::Uuid,
    first_name: &str,
    last_name: &str,
    email: &str,
    avatar: &str,
    avatar_asset_id: Option<uuid::Uuid>,
) -> Result<(), sqlx::Error> {
    // Gitea ships no `display_name` key, so the provider value is always
    // absent and `User.get_display_name(email)` decides (`base.py:268-276`).
    let display_name = display_name_for(email, &mut rand::rng());
    sqlx::query(
        r#"UPDATE users SET first_name = $2, last_name = $3, display_name = $4,
                  avatar = $5, avatar_asset_id = $6
           WHERE id = $1"#,
    )
    .bind(user_id)
    .bind(first_name)
    .bind(last_name)
    .bind(display_name)
    .bind(avatar)
    .bind(avatar_asset_id)
    .execute(pool)
    .await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// §7 Avatar download + S3 upload (`base.py:139-218`)
// ---------------------------------------------------------------------------

/// `get_avatar_download_headers` (`base.py:122-123`): empty for the base
/// adapter (no provider overrides it for gitea).
pub fn avatar_download_headers() -> Vec<(String, String)> {
    Vec::new()
}

/// Content-Type → file extension (`base.py:166-174`). Anything else ends
/// the attempt (`None`), exactly like the Python early return.
pub fn avatar_extension(content_type: &str) -> Option<&'static str> {
    match content_type {
        "image/jpeg" | "image/jpg" => Some("jpg"),
        "image/png" => Some("png"),
        "image/gif" => Some("gif"),
        "image/webp" => Some("webp"),
        _ => None,
    }
}

/// SigV4 endpoint split, mirroring `S3Storage.__init__` with a request
/// (`is_server=False`) exactly like the D-32 asset presigning does
/// (`space/assets.rs::endpoint_parts`, same settings source): MinIO mode
/// signs `{scheme}://{host}` path-style; an explicit endpoint URL signs
/// path-style against it; otherwise the virtual-hosted AWS default.
/// Returns `(endpoint, signed_host, key_prefix)` where `key_prefix` is
/// `"/{bucket}"` for path-style and `""` for virtual-hosted.
pub fn s3_target(
    storage: &pidash_db::config::StorageSettings,
    scheme: &str,
    host: &str,
) -> (String, String, String) {
    if storage.use_minio {
        (
            format!("{scheme}://{host}"),
            host.to_owned(),
            format!("/{}", storage.bucket_name),
        )
    } else if let Some(endpoint) = storage.endpoint_url.as_deref().filter(|e| !e.is_empty()) {
        let endpoint = endpoint.trim_end_matches('/').to_owned();
        let signed_host = endpoint
            .rsplit("://")
            .next()
            .unwrap_or(endpoint.as_str())
            .split('/')
            .next()
            .unwrap_or(endpoint.as_str())
            .to_owned();
        (endpoint, signed_host, format!("/{}", storage.bucket_name))
    } else {
        let region = storage.region.as_str();
        let base = if region.is_empty() {
            "s3.amazonaws.com".to_owned()
        } else {
            format!("s3.{region}.amazonaws.com")
        };
        (
            format!("https://{}.{base}", storage.bucket_name),
            format!("{}.{base}", storage.bucket_name),
            String::new(),
        )
    }
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

fn sha256_hex(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(64);
    for b in Sha256::digest(data) {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

/// SigV4 `Authorization`-header signer for one S3 request (PUT/HEAD/DELETE
/// object), mirroring what boto3 signs for `upload_fileobj`, `head_object`,
/// and `delete_objects`. Any storage misconfiguration (empty keys/bucket)
/// degrades to `None` — boto3 raises there, and every caller here folds
/// that into the avatar-URL fallback, same observable outcome.
pub fn s3_authorization(
    storage: &pidash_db::config::StorageSettings,
    signed_host: &str,
    method: &str,
    canonical_uri: &str,
    payload_hash: &str,
    content_type: Option<&str>,
    now: &chrono::DateTime<chrono::Utc>,
) -> Option<(String, String)> {
    if storage.access_key_id.is_empty()
        || storage.secret_access_key.is_empty()
        || storage.bucket_name.is_empty()
    {
        return None;
    }
    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let date = now.format("%Y%m%d").to_string();
    let scope = format!("{date}/{}/s3/aws4_request", storage.region);
    let mut signed: Vec<(&str, String)> = vec![
        ("host", signed_host.to_owned()),
        ("x-amz-content-sha256", payload_hash.to_owned()),
        ("x-amz-date", amz_date.clone()),
    ];
    if let Some(ct) = content_type {
        signed.push(("content-type", ct.to_owned()));
    }
    signed.sort_by(|a, b| a.0.cmp(b.0));
    let canonical_headers = signed
        .iter()
        .map(|(k, v)| format!("{k}:{v}\n"))
        .collect::<String>();
    let signed_headers = signed.iter().map(|(k, _)| *k).collect::<Vec<_>>().join(";");
    let canonical = format!(
        "{method}\n{canonical_uri}\n\n{canonical_headers}\n{signed_headers}\n{payload_hash}"
    );
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{}",
        sha256_hex(canonical.as_bytes())
    );
    let signing_key = {
        let k_date = hmac_sha256(
            format!("AWS4{}", storage.secret_access_key).as_bytes(),
            date.as_bytes(),
        );
        let k_region = hmac_sha256(&k_date, storage.region.as_bytes());
        let k_service = hmac_sha256(&k_region, b"s3");
        hmac_sha256(&k_service, b"aws4_request")
    };
    let signature = sha256_hex(&hmac_sha256(&signing_key, string_to_sign.as_bytes()));
    let authorization = format!(
        "AWS4-HMAC-SHA256 Credential={}/{scope}, SignedHeaders={signed_headers}, Signature={signature}",
        storage.access_key_id
    );
    Some((amz_date, authorization))
}

/// New FileAsset row (`base.py:197-206`): `attributes={name, type, size}`,
/// `asset` under the avatar `upload_to` (`asset.py:17-20`, workspace-less →
/// `user-{hex}-{filename}`), `user` + `created_by` set, entity
/// `USER_AVATAR`, uploaded flag, S3 head metadata (NULL when the HEAD
/// fails, like boto3's `None`).
#[allow(clippy::too_many_arguments)]
pub async fn insert_file_asset(
    pool: &sqlx::PgPool,
    id: uuid::Uuid,
    stored_key: &str,
    content_type: &str,
    file_size: i64,
    user_id: uuid::Uuid,
    storage_metadata: Option<serde_json::Value>,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<(), sqlx::Error> {
    let attributes = serde_json::json!({
        "name": format!("gitea-avatar.{}", avatar_extension(content_type).unwrap_or("jpg")),
        "type": content_type,
        "size": file_size,
    });
    sqlx::query(
        r#"INSERT INTO file_assets
           (id, created_at, updated_at, attributes, asset,
            created_by_id, updated_by_id, workspace_id,
            is_deleted, deleted_at, is_archived,
            comment_id, entity_type, external_id, external_source,
            is_uploaded, issue_id, page_id, project_id,
            size, storage_metadata, user_id, draft_issue_id, entity_identifier)
           VALUES ($1,$2,$2,$3,$4,$5,NULL,NULL,
                   FALSE,NULL,FALSE,
                   NULL,'USER_AVATAR',NULL,NULL,
                   TRUE,NULL,NULL,NULL,
                   $6,$7,$5,NULL,NULL)"#,
    )
    .bind(id)
    .bind(now)
    .bind(attributes)
    .bind(stored_key)
    .bind(user_id)
    .bind(file_size as f64)
    .bind(storage_metadata)
    .execute(pool)
    .await?;
    Ok(())
}

/// `download_and_upload_avatar` (`base.py:139-205`): download (10s
/// timeout), size + content-type guards, S3 PUT, HEAD metadata, FileAsset
/// row. ANY failure returns `Ok(None)` so the caller falls back to the
/// provider URL (`except Exception: log_exception; return None`).
/// `max_bytes` is `settings.DATA_UPLOAD_MAX_MEMORY_SIZE`.
#[allow(clippy::too_many_arguments)]
pub async fn download_and_upload_avatar(
    client: &reqwest::Client,
    pool: &sqlx::PgPool,
    storage: &pidash_db::config::StorageSettings,
    scheme: &str,
    host: &str,
    avatar_url: &str,
    user_id: uuid::Uuid,
    max_bytes: u64,
    now: chrono::DateTime<chrono::Utc>,
) -> Option<uuid::Uuid> {
    if avatar_url.is_empty() {
        return None;
    }
    let outcome: Result<Option<uuid::Uuid>, ()> = async {
        let mut response = client
            .get(avatar_url)
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await
            .map_err(|_| ())?
            .error_for_status()
            .map_err(|_| ())?;
        let headers = response.headers().clone();
        if let Some(length) = headers
            .get(reqwest::header::CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok())
        {
            if length > max_bytes {
                return Err(());
            }
        }
        let content_type = headers
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("image/jpeg")
            .split(';')
            .next()
            .unwrap_or("image/jpeg")
            .trim()
            .to_owned();
        let extension = avatar_extension(&content_type).ok_or(())?;
        let mut content = Vec::new();
        loop {
            let chunk = response.chunk().await.map_err(|_| ())?;
            let Some(chunk) = chunk else {
                break;
            };
            if content.len() as u64 + chunk.len() as u64 > max_bytes {
                return Err(());
            }
            content.extend_from_slice(&chunk);
        }
        let file_size = content.len() as i64;
        let filename = format!("{}-user-avatar.{extension}", uuid::Uuid::new_v4().simple());
        // `upload_to` with no workspace (`asset.py:17-20`).
        let stored_key = format!("user-{}-{filename}", uuid::Uuid::new_v4().simple());
        let (endpoint, signed_host, prefix) = s3_target(storage, scheme, host);
        let payload_hash = sha256_hex(&content);
        let canonical_uri = format!("{prefix}/{stored_key}");
        let (amz_date, authorization) = s3_authorization(
            storage,
            &signed_host,
            "PUT",
            &canonical_uri,
            &payload_hash,
            Some(&content_type),
            &now,
        )
        .ok_or(())?;
        let url = format!("{endpoint}{canonical_uri}");
        let put = client
            .put(url)
            .header("Host", signed_host.clone())
            .header("x-amz-date", amz_date.clone())
            .header("x-amz-content-sha256", payload_hash.clone())
            .header(reqwest::header::CONTENT_TYPE, content_type.clone())
            .header("Authorization", authorization.clone())
            .body(content)
            .send()
            .await
            .map_err(|_| ())?;
        if !put.status().is_success() {
            return Err(());
        }
        // `get_object_metadata`: HEAD, same signing, no content type.
        let head_hash = sha256_hex(&[]);
        let (head_date, head_auth) = s3_authorization(
            storage,
            &signed_host,
            "HEAD",
            &canonical_uri,
            &head_hash,
            None,
            &now,
        )
        .ok_or(())?;
        let head_url = format!("{endpoint}{canonical_uri}");
        let metadata = client
            .head(head_url)
            .header("Host", signed_host)
            .header("x-amz-date", head_date)
            .header("x-amz-content-sha256", head_hash)
            .header("Authorization", head_auth)
            .send()
            .await
            .ok()
            .filter(|r| r.status().is_success())
            .map(|r| head_metadata_json(r.headers()));
        let asset_id = uuid::Uuid::new_v4();
        insert_file_asset(
            pool,
            asset_id,
            &stored_key,
            &content_type,
            file_size,
            user_id,
            metadata,
            now,
        )
        .await
        .map_err(|_| ())?;
        Ok(Some(asset_id))
    }
    .await;
    match outcome {
        Ok(found) => found,
        Err(()) => {
            tracing::warn!("download_and_upload_avatar failed; falling back to provider URL");
            None
        }
    }
}

/// `S3Storage.get_object_metadata` (`storage.py:156-170`): the HEAD
/// response projected to `{ContentType, ContentLength, LastModified
/// (ISO-8601), ETag, Metadata (x-amz-meta-* map)}`.
pub fn head_metadata_json(headers: &reqwest::header::HeaderMap) -> serde_json::Value {
    let str_header = |name: &str| {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned)
    };
    let content_length = headers
        .get(reqwest::header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<i64>().ok());
    let last_modified = str_header("last-modified")
        .and_then(|v| chrono::DateTime::parse_from_rfc2822(&v).ok())
        .map(|dt| dt.to_rfc3339());
    let mut meta = serde_json::Map::new();
    for (name, value) in headers {
        let name = name.as_str();
        if let Some(suffix) = name.strip_prefix("x-amz-meta-") {
            if let Ok(value) = value.to_str() {
                meta.insert(
                    suffix.to_owned(),
                    serde_json::Value::String(value.to_owned()),
                );
            }
        }
    }
    serde_json::json!({
        "ContentType": str_header("content-type"),
        "ContentLength": content_length,
        "LastModified": last_modified,
        "ETag": str_header("etag"),
        "Metadata": meta,
    })
}

/// `delete_old_avatar` (`base.py:236-253`): when the user carries an
/// avatar asset, delete the S3 object + the row and clear both columns.
/// `FileAsset.DoesNotExist` passes silently; any other failure is logged
/// and the flow continues with the old avatar intact (`except Exception:
/// log_exception; return`).
#[allow(clippy::too_many_arguments)]
pub async fn delete_old_avatar(
    client: &reqwest::Client,
    pool: &sqlx::PgPool,
    storage: &pidash_db::config::StorageSettings,
    scheme: &str,
    host: &str,
    user_id: uuid::Uuid,
    avatar_asset_id: Option<uuid::Uuid>,
    now: chrono::DateTime<chrono::Utc>,
) {
    let Some(asset_id) = avatar_asset_id else {
        return;
    };
    let row: Option<(String,)> =
        match sqlx::query_as(r#"SELECT asset FROM file_assets WHERE id = $1"#)
            .bind(asset_id)
            .fetch_optional(pool)
            .await
        {
            Ok(row) => row,
            Err(error) => {
                tracing::warn!(%error, "delete_old_avatar lookup failed");
                return;
            }
        };
    let Some((key,)) = row else {
        return;
    };
    let (endpoint, signed_host, prefix) = s3_target(storage, scheme, host);
    let payload_hash = sha256_hex(&[]);
    let canonical_uri = format!("{prefix}/{key}");
    if let Some((amz_date, authorization)) = s3_authorization(
        storage,
        &signed_host,
        "DELETE",
        &canonical_uri,
        &payload_hash,
        None,
        &now,
    ) {
        let url = format!("{endpoint}{canonical_uri}");
        match client
            .delete(url)
            .header("Host", signed_host)
            .header("x-amz-date", amz_date)
            .header("x-amz-content-sha256", payload_hash)
            .header("Authorization", authorization)
            .send()
            .await
        {
            Ok(response) if response.status().is_success() || response.status() == 404 => {
                if sqlx::query(r#"DELETE FROM file_assets WHERE id = $1"#)
                    .bind(asset_id)
                    .execute(pool)
                    .await
                    .is_err()
                {
                    return;
                }
                let _ = sqlx::query(
                    r#"UPDATE users SET avatar_asset_id = NULL, avatar = '' WHERE id = $1"#,
                )
                .bind(user_id)
                .execute(pool)
                .await;
            }
            other => {
                tracing::warn!(
                    "delete_old_avatar S3 delete failed: {:?}",
                    other.map(|r| r.status())
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// §8 Invitation workflow (post_user_auth_workflow →
// process_workspace_project_invitations, workspace_project_join.py)
// ---------------------------------------------------------------------------

/// `get_default_props` (`db/models/workspace.py:22-60`): the
/// `view_props`/`default_props` JSON for bulk-created memberships.
pub fn default_props_json() -> serde_json::Value {
    serde_json::json!({
        "filters": {
            "priority": null, "state": null, "state_group": null,
            "assignees": null, "created_by": null, "labels": null,
            "start_date": null, "target_date": null, "subscriber": null,
        },
        "display_filters": {
            "group_by": null, "order_by": "-created_at", "type": null,
            "sub_issue": true, "show_empty_groups": true, "layout": "list",
            "calendar_date_range": "",
        },
        "display_properties": {
            "assignee": true, "attachment_count": true, "created_on": true,
            "due_date": true, "estimate": true, "key": true,
            "labels": true, "link": true, "priority": true,
            "start_date": true, "state": true, "sub_issue_count": true,
            "updated_on": true,
        },
    })
}

/// `get_issue_props` (`workspace.py:110-111`).
pub fn default_issue_props_json() -> serde_json::Value {
    serde_json::json!({"subscribed": true, "assigned": true, "created": true, "all_issues": true})
}

/// `get_default_preferences` (`db/models/project.py:68-69`).
pub fn default_preferences_json() -> serde_json::Value {
    serde_json::json!({"pages": {"block_display": true}, "navigation": {"default_tab": "work_items", "hide_in_more_menu": []}})
}

/// One accepted workspace invite with its workspace slug
/// (`workspace_project_join.py:24` + the `:39-44` cache loop needing the
/// slug). Soft-deleted invite rows are invisible to the Django manager;
/// the `accepted` filter is verbatim.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct WorkspaceInviteRow {
    pub id: uuid::Uuid,
    pub workspace_id: uuid::Uuid,
    pub role: i16,
    pub slug: String,
}

/// One accepted project invite (`workspace_project_join.py:59-87`).
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ProjectInviteRow {
    pub id: uuid::Uuid,
    pub workspace_id: uuid::Uuid,
    pub role: i16,
    pub created_by_id: Option<uuid::Uuid>,
}

/// `track_event.delay(...)` for one joined workspace (`:45-56`): the
/// Celery message for the Python worker (`event_name
/// USER_JOINED_WORKSPACE`), enqueued through the Postgres queue. A queue
/// failure raises in Python, so it surfaces as a 500 here too.
pub async fn enqueue_join_event(
    pool: &sqlx::PgPool,
    user_id: uuid::Uuid,
    workspace_id: uuid::Uuid,
    workspace_slug: &str,
    role: i32,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<(), sqlx::Error> {
    let joined_at = now.to_rfc3339_opts(chrono::SecondsFormat::Micros, false);
    let message = pidash_jobs::celery::CeleryTaskMessage::new(
        "pi_dash.bgtasks.event_tracking_task.track_event",
        vec![
            serde_json::Value::String(user_id.to_string()),
            serde_json::Value::String("user_joined_workspace".to_owned()),
            serde_json::Value::String(workspace_slug.to_owned()),
            serde_json::json!({
                "user_id": user_id.to_string(),
                "workspace_id": workspace_id.to_string(),
                "workspace_slug": workspace_slug,
                "role": role,
                "joined_at": joined_at,
            }),
        ],
        Default::default(),
    );
    let job = pidash_jobs::queue::NewJob::new(
        message.task.clone(),
        serde_json::Value::Array(message.args.clone()),
        serde_json::Value::Object(message.kwargs.clone()),
    );
    pidash_jobs::queue::enqueue(pool, &job).await?;
    Ok(())
}

/// `invalidate_cache_directly(path, url_params=False, user=False,
/// multiple=True)` (`utils/cache.py:54-69` + `workspace_project_join.py:39-44`):
/// `user=False` forces `auth_header=None`, so the key is the bare
/// `/api/workspaces/{slug}/members/` path; `multiple=True` deletes every
/// key matching `*{key}*` (`KEYS` + `DEL`). Any Redis failure raises in
/// Python, so it surfaces as a 500 here too.
pub async fn invalidate_workspace_members_cache(
    redis_url: Option<&str>,
    workspace_slug: &str,
) -> Result<(), String> {
    let url = redis_url
        .filter(|u| !u.is_empty())
        .ok_or_else(|| "cache backend unavailable".to_owned())?;
    let client = redis::Client::open(url).map_err(|e| e.to_string())?;
    let mut conn = client
        .get_multiplexed_async_connection()
        .await
        .map_err(|e| e.to_string())?;
    let key = format!("/api/workspaces/{workspace_slug}/members/");
    let pattern = format!("*{key}*");
    let keys: Vec<String> = redis::cmd("KEYS")
        .arg(&pattern)
        .query_async(&mut conn)
        .await
        .map_err(|e| e.to_string())?;
    if !keys.is_empty() {
        let _: () = redis::cmd("DEL")
            .arg(&keys)
            .query_async(&mut conn)
            .await
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Full `process_workspace_project_invitations`
/// (`workspace_project_join.py:18-91`): workspace bulk-create
/// (`ignore_conflicts` → `ON CONFLICT DO NOTHING`, every concrete column
/// supplied with Django defaults), per-invite cache invalidation +
/// `track_event`, project-invite workspace rows (mapped role +
/// `created_by_id` carried over), then the project-member rows **without
/// `project_id`** — ported bug, raises on Postgres (fixture
/// `ported_bugs[0]`) — and finally both invite deletes. No transaction:
///
/// each statement commits on its own, so a later failure (notably the
/// project-member bug) leaves the earlier writes in place.
pub async fn process_invitations(
    pool: &sqlx::PgPool,
    redis_url: Option<&str>,
    user_id: uuid::Uuid,
    email: &str,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<(), sqlx::Error> {
    let workspace_invites: Vec<WorkspaceInviteRow> = sqlx::query_as::<_, WorkspaceInviteRow>(
        r#"SELECT i.id, i.workspace_id, i.role, w.slug
           FROM workspace_member_invites i
           JOIN workspaces w ON w.id = i.workspace_id
           WHERE i.email = $1 AND i.accepted = TRUE AND i.deleted_at IS NULL"#,
    )
    .bind(email)
    .fetch_all(pool)
    .await?;
    if !workspace_invites.is_empty() {
        let props = default_props_json();
        let issue_props = default_issue_props_json();
        for invite in &workspace_invites {
            sqlx::query(
                r#"INSERT INTO workspace_members
                   (id, created_at, updated_at, workspace_id, member_id, role,
                    company_role, view_props, default_props, issue_props,
                    is_active, getting_started_checklist, tips, explored_features,
                    created_by_id, updated_by_id, deleted_at)
                   VALUES ($1,$2,$2,$3,$4,$5,
                           NULL,$6,$6,$7,
                           TRUE,'{}','{}','{}',
                           NULL,NULL,NULL)
                   ON CONFLICT DO NOTHING"#,
            )
            .bind(uuid::Uuid::new_v4())
            .bind(now)
            .bind(invite.workspace_id)
            .bind(user_id)
            .bind(i32::from(invite.role))
            .bind(&props)
            .bind(&issue_props)
            .execute(pool)
            .await?;
        }
        for invite in &workspace_invites {
            invalidate_workspace_members_cache(redis_url, &invite.slug)
                .await
                .map_err(|_| sqlx::Error::PoolTimedOut)?;
            enqueue_join_event(
                pool,
                user_id,
                invite.workspace_id,
                &invite.slug,
                i32::from(invite.role),
                now,
            )
            .await?;
        }
    }
    let project_invites: Vec<ProjectInviteRow> = sqlx::query_as::<_, ProjectInviteRow>(
        r#"SELECT id, workspace_id, role, created_by_id
           FROM project_member_invites
           WHERE email = $1 AND accepted = TRUE AND deleted_at IS NULL"#,
    )
    .bind(email)
    .fetch_all(pool)
    .await?;
    if !project_invites.is_empty() {
        let props = default_props_json();
        let prefs = default_preferences_json();
        for invite in &project_invites {
            let role = if invite.role == 5 || invite.role == 15 {
                i32::from(invite.role)
            } else {
                15
            };
            sqlx::query(
                r#"INSERT INTO workspace_members
                   (id, created_at, updated_at, workspace_id, member_id, role,
                    company_role, view_props, default_props, issue_props,
                    is_active, getting_started_checklist, tips, explored_features,
                    created_by_id, updated_by_id, deleted_at)
                   VALUES ($1,$2,$2,$3,$4,$5,
                           NULL,$6,$6,$7,
                           TRUE,'{}','{}','{}',
                           $8,NULL,NULL)
                   ON CONFLICT DO NOTHING"#,
            )
            .bind(uuid::Uuid::new_v4())
            .bind(now)
            .bind(invite.workspace_id)
            .bind(user_id)
            .bind(role)
            .bind(&props)
            .bind(default_issue_props_json())
            .bind(invite.created_by_id)
            .execute(pool)
            .await?;
            // Ported bug: `project_id` is missing here, so this raises
            // `IntegrityError` on Postgres (`project_members.project_id`
            // is NOT NULL) — every app-callback success with accepted
            // project invites 500s after the rows above committed.
            sqlx::query(
                r#"INSERT INTO project_members
                   (id, created_at, updated_at, project_id, workspace_id, member_id,
                    comment, role, view_props, default_props, preferences,
                    sort_order, is_active, created_by_id, updated_by_id, deleted_at)
                   VALUES ($1,$2,$2,NULL,$3,$4,
                           NULL,$5,$6,$6,$7,
                           65535,TRUE,$8,NULL,NULL)
                   ON CONFLICT DO NOTHING"#,
            )
            .bind(uuid::Uuid::new_v4())
            .bind(now)
            .bind(invite.workspace_id)
            .bind(user_id)
            .bind(role)
            .bind(&props)
            .bind(&prefs)
            .bind(invite.created_by_id)
            .execute(pool)
            .await?;
        }
    }
    sqlx::query(r#"DELETE FROM workspace_member_invites WHERE email = $1 AND accepted = TRUE"#)
        .bind(email)
        .execute(pool)
        .await?;
    sqlx::query(r#"DELETE FROM project_member_invites WHERE email = $1 AND accepted = TRUE"#)
        .bind(email)
        .execute(pool)
        .await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// §9 Redirection path + profile (`utils/redirection_path.py:8-46`)
// ---------------------------------------------------------------------------

/// `Profile.objects.get_or_create(user=user)` projected to what the path
/// decision reads (`is_onboarded`, `last_workspace_id`).
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ProfilePathRow {
    pub id: uuid::Uuid,
    pub is_onboarded: bool,
    pub last_workspace_id: Option<uuid::Uuid>,
}

/// `get_default_onboarding` (`db/models/user.py:25-31`).
pub fn default_onboarding_json() -> serde_json::Value {
    serde_json::json!({
        "profile_complete": false, "workspace_create": false,
        "workspace_invite": false, "workspace_join": false,
    })
}

/// `get_mobile_default_onboarding` (`user.py:34-39`).
pub fn default_mobile_onboarding_json() -> serde_json::Value {
    serde_json::json!({
        "profile_complete": false, "workspace_create": false,
        "workspace_join": false,
    })
}

/// `get_random_color` (`utils/color.py:9-14`): `#` + six random hexdigits.
/// The draws are random either way; any six-hexdigit sample is a faithful
/// draw from the same alphabet (`string.hexdigits`).
pub fn random_color<R: rand::Rng>(rng: &mut R) -> String {
    const HEXDIGITS: &[u8] = b"0123456789abcdefABCDEF";
    let mut out = String::with_capacity(7);
    out.push('#');
    for _ in 0..6 {
        out.push(HEXDIGITS[rng.random_range(0..HEXDIGITS.len())] as char);
    }
    out
}

/// `get_default_product_tour` (`user.py:42-48`).
pub fn default_product_tour_json() -> serde_json::Value {
    serde_json::json!({
        "work_items": false, "cycles": false, "modules": false,
        "intake": false, "pages": false,
    })
}

/// `Profile.objects.create(user=user)` (`base.py:342`): every NOT NULL
/// `profiles` column with its Django field default. The background draw
/// happens before any await so no RNG lives across one.
pub async fn create_profile(
    pool: &sqlx::PgPool,
    user_id: uuid::Uuid,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<(), sqlx::Error> {
    let background = random_color(&mut rand::rng());
    sqlx::query(
        r#"INSERT INTO profiles
           (id, created_at, updated_at, theme, is_app_rail_docked,
            is_tour_completed, onboarding_step, use_case, role,
            is_onboarded, last_workspace_id,
            billing_address_country, billing_address, has_billing_address,
            company_name, notification_view_mode, is_smooth_cursor_enabled,
            is_mobile_onboarded, mobile_onboarding_step, mobile_timezone_auto_set,
            language, start_of_the_week, goals, background_color,
            is_navigation_tour_completed,
            has_marketing_email_consent, is_subscribed_to_changelog,
            product_tour, settings, user_id)
           VALUES ($1,$2,$2,'{}',TRUE,
                   FALSE,$3,NULL,NULL,
                   FALSE,NULL,
                   'INDIA',NULL,FALSE,
                   '','full',FALSE,
                   FALSE,$4,FALSE,
                   'en',0,'{}',$7,
                   FALSE,
                   FALSE,FALSE,
                   $5,'{}',$6)"#,
    )
    .bind(uuid::Uuid::new_v4())
    .bind(now)
    .bind(default_onboarding_json())
    .bind(default_mobile_onboarding_json())
    .bind(default_product_tour_json())
    .bind(user_id)
    .bind(background)
    .execute(pool)
    .await?;
    Ok(())
}

/// `get_redirection_path` (`redirection_path.py:8-46`): onboarding first,
/// then the last workspace (only when the membership is active), then the
/// earliest-created active membership, then invitations, else
/// create-workspace. The profile is created when missing
/// (`get_or_create`); the pure branch order reuses the D-16
/// `select_redirection_path` kernel shape.
pub async fn redirection_path(
    pool: &sqlx::PgPool,
    user_id: uuid::Uuid,
    email: &str,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<String, sqlx::Error> {
    let profile: Option<ProfilePathRow> = sqlx::query_as::<_, ProfilePathRow>(
        r#"SELECT id, is_onboarded, last_workspace_id FROM profiles WHERE user_id = $1 LIMIT 1"#,
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await?;
    let profile = match profile {
        Some(row) => row,
        None => {
            create_profile(pool, user_id, now).await?;
            sqlx::query_as::<_, ProfilePathRow>(
                r#"SELECT id, is_onboarded, last_workspace_id FROM profiles WHERE user_id = $1 LIMIT 1"#,
            )
            .bind(user_id)
            .fetch_optional(pool)
            .await?
            .ok_or(sqlx::Error::RowNotFound)?
        }
    };
    if !profile.is_onboarded {
        return Ok("onboarding".to_owned());
    }
    if let Some(last_id) = profile.last_workspace_id {
        let member: Option<(uuid::Uuid, String)> = sqlx::query_as(
            r#"SELECT w.id, w.slug FROM workspaces w
               JOIN workspace_members m ON m.workspace_id = w.id
               WHERE w.id = $1 AND m.member_id = $2 AND m.is_active = TRUE
                 AND m.deleted_at IS NULL AND w.deleted_at IS NULL LIMIT 1"#,
        )
        .bind(last_id)
        .bind(user_id)
        .fetch_optional(pool)
        .await?;
        if let Some((_, slug)) = member {
            return Ok(slug);
        }
    }
    let fallback: Option<(uuid::Uuid, String)> = sqlx::query_as(
        r#"SELECT w.id, w.slug FROM workspaces w
           JOIN workspace_members m ON m.workspace_id = w.id
           WHERE m.member_id = $1 AND m.is_active = TRUE
             AND m.deleted_at IS NULL AND w.deleted_at IS NULL
           ORDER BY w.created_at ASC LIMIT 1"#,
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await?;
    if let Some((_, slug)) = fallback {
        return Ok(slug);
    }
    let invites: Option<(i64,)> = sqlx::query_as(
        r#"SELECT COUNT(*) FROM workspace_member_invites WHERE email = $1 AND deleted_at IS NULL"#,
    )
    .bind(email)
    .fetch_optional(pool)
    .await?;
    if invites.is_some_and(|(count,)| count > 0) {
        return Ok("invitations".to_owned());
    }
    Ok("create-workspace".to_owned())
}

// ---------------------------------------------------------------------------
// §10 `user_login` session (`authentication/utils/login.py:10-27`)
// ---------------------------------------------------------------------------

/// `user.get_session_auth_hash()`: `salted_hmac` over the password field
/// with the Django session-auth salt (same construction the D-01
/// `verify_session_hash` checks against).
pub fn session_auth_hash(password_field: &str, secret_key: &[u8]) -> String {
    use hmac::{Hmac, Mac};
    use sha2::{Digest, Sha256};
    const SALT: &str = "django.contrib.auth.models.AbstractBaseUser.get_session_auth_hash";
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let key = Sha256::digest([SALT.as_bytes(), secret_key].concat());
    let mut mac = Hmac::<Sha256>::new_from_slice(&key).expect("HMAC accepts any key length");
    mac.update(password_field.as_bytes());
    let mut out = String::with_capacity(64);
    for b in mac.finalize().into_bytes() {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

/// `user_login` (`login.py:10-27`): Django `login()` (session `_auth_*`
/// trio) + `device_info` + save. One documented divergence: Django
/// `login()` cycles the session key (fresh key; old row deleted; data
/// preserved on cycle, wiped on flush), but the session middleware exposes
/// no cycle API and the crate is read-only for this issue, so the keys are
/// written onto the live session and the middleware persists them. Fresh
/// sessions — the only contract-relevant case — behave identically (fresh
/// key + `Set-Cookie`); a pre-existing key is reused instead of rotated.
pub fn user_login_session(
    handle: &Option<Extension<SessionHandle>>,
    user_id: &uuid::Uuid,
    password_field: &str,
    secret_key: &[u8],
    user_agent: Option<&str>,
    ip: Option<&str>,
    domain: &str,
) {
    let Some(Extension(handle)) = handle else {
        return;
    };
    let mut session = handle.lock();
    session.set(
        "_auth_user_id".to_owned(),
        serde_json::Value::String(user_id.to_string()),
    );
    session.set(
        "_auth_user_backend".to_owned(),
        serde_json::Value::String(crate::license::MODEL_BACKEND.to_owned()),
    );
    session.set(
        "_auth_user_hash".to_owned(),
        serde_json::Value::String(session_auth_hash(password_field, secret_key)),
    );
    session.set(
        "device_info".to_owned(),
        device_info_value(user_agent, ip, domain),
    );
}

// ---------------------------------------------------------------------------
// §11 The four handlers
// (`views/app/gitea.py`, `views/space/gitea.py`, full)
// ---------------------------------------------------------------------------

/// Request inputs every handler derives from headers + query.
pub struct RequestInputs {
    pub host: String,
    pub is_secure: bool,
    pub user_agent: Option<String>,
    pub ip: Option<String>,
}

pub fn request_inputs(headers: &HeaderMap) -> RequestInputs {
    // `REMOTE_ADDR` (the TCP peer) is invisible to axum handlers without a
    // `ConnectInfo` extractor, so it reads as absent (a missing IP stays
    // JSON null in `device_info` and `last_login_ip` stores NULL) — the
    // same convention the D-01 handlers use for their login stamps.
    let forwarded = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok());
    RequestInputs {
        host: request_host(headers),
        is_secure: request_is_secure(headers),
        user_agent: headers
            .get(header::USER_AGENT)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned),
        ip: client_ip(forwarded, None),
    }
}

/// Shared initiate prologue (`app/gitea.py:30-35`, `space/gitea.py:31-36`):
/// store `host` + validated `next_path`, returning the raw next_path for
/// the error echo. `request.GET.get("next_path")` truthiness gates the
/// session write; the echo rule differs per twin (validated vs raw) and is
/// applied by the caller.
pub fn initiate_prologue(
    handle: &Option<Extension<SessionHandle>>,
    query: &QueryMap,
    host: &str,
) -> Option<String> {
    write_session_key(handle, SESSION_HOST, host.to_owned());
    let raw = query_last(query, "next_path").filter(|v| !v.is_empty());
    if let Some(ref next) = raw {
        let validated = pidash_services::auth_session::shapes::validate_next_path(next);
        write_session_key(handle, SESSION_NEXT_PATH, validated);
    }
    raw
}

/// Shared initiate error params: `exc.get_error_dict()` plus the
/// `next_path` echo when the query carried one.
pub fn initiate_error_params(
    exc: &AuthenticationException,
    raw_next: Option<&str>,
    validated_echo: bool,
) -> Vec<(String, String)> {
    let mut params: Vec<(String, String)> = exc
        .get_error_dict()
        .into_iter()
        .map(|(k, v)| {
            (
                k,
                match v {
                    serde_json::Value::String(s) => s,
                    other => other.to_string(),
                },
            )
        })
        .collect();
    if let Some(raw) = raw_next {
        let echo = if validated_echo {
            pidash_services::auth_session::shapes::validate_next_path(raw)
        } else {
            raw.to_owned()
        };
        params.push(("next_path".to_owned(), echo));
    }
    params
}

/// Authenticated-user result of the success path.
pub struct AuthenticatedUser {
    pub id: uuid::Uuid,
    pub password_field: String,
}

/// Success-path failure: an `AuthenticationException` becomes the provider
/// redirect; any storage/queue failure becomes the generic 500 (uncaught
/// in a plain `View`, like Django's handler-500).
#[derive(Debug)]
pub enum AuthError {
    Provider(AuthenticationException),
    Server(Denial),
}

/// Full `GiteaOAuthProvider.authenticate()` +
/// `Adapter.complete_login_or_signup` for one callback: token exchange,
/// userinfo, sanitize, user lookup/create (+avatar, +profile), IDP sync,
/// `save_user_data` (+activation mail), the app-only invitation callback,
/// and the account upsert. `AuthenticationException` propagates for the
/// redirect branches; any `sqlx::Error` becomes the generic 500 (uncaught
/// in a plain `View`, like Django's handler-500).
#[allow(clippy::too_many_arguments)]
pub async fn authenticate_user(
    pool: &sqlx::PgPool,
    secret_key: &str,
    redis_url: Option<&str>,
    storage: &pidash_db::config::StorageSettings,
    file_size_max: u64,
    inputs: &RequestInputs,
    code: &str,
    redirect_uri: &str,
    with_callback: bool,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<AuthenticatedUser, AuthError> {
    let client = reqwest::Client::new();
    let config = gitea_config(pool, secret_key)
        .await
        .map_err(AuthError::Provider)?;
    let tokens = fetch_token_data(&client, &config, code, redirect_uri, now)
        .await
        .map_err(AuthError::Provider)?;
    let access_token = match &tokens.access_token {
        serde_json::Value::String(s) => s.clone(),
        _ => String::new(),
    };
    let user_data = fetch_user_data(&client, &config, &access_token)
        .await
        .map_err(AuthError::Provider)?;
    let user_json =
        serde_json::to_value(&user_data).map_err(|_| AuthError::Server(Denial::ServerError))?;
    let email = sanitize_email(user_json.get("email")).map_err(AuthError::Provider)?;
    let existing = find_user_by_email(pool, &email)
        .await
        .map_err(|_| AuthError::Server(Denial::ServerError))?;
    // `is_signup = bool(user)` — inverted as written (`base.py:297`).
    let is_signup = existing.is_some();
    let user_id;
    let password_field;
    if let Some(row) = existing {
        user_id = row.id;
        password_field = row.password.clone();
        let sync_enabled = config_value(
            pool,
            secret_key,
            "ENABLE_GITEA_SYNC",
            env_default("ENABLE_GITEA_SYNC"),
        )
        .await
        .map(|v| v.as_deref() == Some("1"))
        .unwrap_or(false);
        if sync_enabled && !is_signup {
            let first = user_json
                .get("user")
                .and_then(|u| u.get("first_name"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let last = user_json
                .get("user")
                .and_then(|u| u.get("last_name"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            delete_old_avatar(
                &client,
                pool,
                storage,
                if inputs.is_secure { "https" } else { "http" },
                &inputs.host,
                user_id,
                row.avatar_asset_id,
                now,
            )
            .await;
            let avatar_url = user_json
                .get("user")
                .and_then(|u| u.get("avatar"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let (avatar, asset) = match download_and_upload_avatar(
                &client,
                pool,
                storage,
                if inputs.is_secure { "https" } else { "http" },
                &inputs.host,
                avatar_url,
                user_id,
                file_size_max,
                now,
            )
            .await
            {
                Some(asset_id) => (avatar_url.to_owned(), Some(asset_id)),
                None => (avatar_url.to_owned(), None),
            };
            sync_user_data(pool, user_id, first, last, &email, &avatar, asset)
                .await
                .map_err(|_| AuthError::Server(Denial::ServerError))?;
        }
    } else {
        check_signup(pool, secret_key, &email)
            .await
            .map_err(AuthError::Provider)?;
        user_id = uuid::Uuid::new_v4();
        let username = uuid::Uuid::new_v4().simple().to_string();
        // Drawn before any await: the `rand::rng()` handle must not live
        // across one (the handler future has to stay `Send`).
        let (password, display) = {
            let mut rng = rand::rng();
            (
                encode_password(
                    &uuid::Uuid::new_v4().simple().to_string(),
                    &random_salt(&mut rng),
                ),
                display_name_for(&email, &mut rng),
            )
        };
        password_field = password;
        let first = user_json
            .get("user")
            .and_then(|u| u.get("first_name"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let last = user_json
            .get("user")
            .and_then(|u| u.get("last_name"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let first = if first.is_empty() { "" } else { first };
        let last = if last.is_empty() { "" } else { last };
        insert_user(
            pool,
            user_id,
            &email,
            &username,
            &password_field,
            first,
            last,
            &display,
            now,
        )
        .await
        .map_err(|_| AuthError::Server(Denial::ServerError))?;
        let avatar_url = user_json
            .get("user")
            .and_then(|u| u.get("avatar"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        // In-memory avatar assignment, persisted by `save_user_data`
        // below (`base.py:330-340` has no save of its own).
        let (avatar, asset) = if avatar_url.is_empty() {
            (String::new(), None)
        } else {
            match download_and_upload_avatar(
                &client,
                pool,
                storage,
                if inputs.is_secure { "https" } else { "http" },
                &inputs.host,
                avatar_url,
                user_id,
                file_size_max,
                now,
            )
            .await
            {
                Some(asset_id) => (avatar_url.to_owned(), Some(asset_id)),
                None => (avatar_url.to_owned(), None),
            }
        };
        create_profile(pool, user_id, now)
            .await
            .map_err(|_| AuthError::Server(Denial::ServerError))?;
        let rotated = format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        let fire_mail = save_user_data(
            pool,
            user_id,
            true,
            inputs.ip.as_deref(),
            inputs.user_agent.as_deref(),
            &avatar,
            asset,
            &rotated,
            now,
        )
        .await
        .map_err(|_| AuthError::Server(Denial::ServerError))?;
        let _ = fire_mail;
        return finish_authenticated(
            pool,
            redis_url,
            user_id,
            &password_field,
            &email,
            &user_json,
            &tokens,
            with_callback,
            now,
        )
        .await;
    }
    // Existing-user tail: `save_user_data` always runs (`base.py:347`).
    let row = find_user_by_email(pool, &email)
        .await
        .map_err(|_| AuthError::Server(Denial::ServerError))?
        .ok_or(AuthError::Server(Denial::ServerError))?;
    let rotated = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    save_user_data(
        pool,
        user_id,
        row.is_active,
        inputs.ip.as_deref(),
        inputs.user_agent.as_deref(),
        &row.avatar,
        row.avatar_asset_id,
        &rotated,
        now,
    )
    .await
    .map_err(|_| AuthError::Server(Denial::ServerError))?;
    if !row.is_active {
        let site = if inputs.is_secure {
            format!("https://{}", inputs.host)
        } else {
            format!("http://{}", inputs.host)
        };
        enqueue_activation_email(pool, &site, user_id)
            .await
            .map_err(|_| AuthError::Server(Denial::ServerError))?;
    }
    finish_authenticated(
        pool,
        redis_url,
        user_id,
        &password_field,
        &email,
        &user_json,
        &tokens,
        with_callback,
        now,
    )
    .await
}

/// `complete_login_or_signup` tail shared by both arms: the app-only
/// invitation callback, then the account upsert when token data is present
/// (`base.py:350-357`).
#[allow(clippy::too_many_arguments)]
async fn finish_authenticated(
    pool: &sqlx::PgPool,
    redis_url: Option<&str>,
    user_id: uuid::Uuid,
    password_field: &str,
    email: &str,
    user_json: &serde_json::Value,
    tokens: &pidash_services::auth_oauth::providers::TokenData,
    with_callback: bool,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<AuthenticatedUser, AuthError> {
    if with_callback {
        process_invitations(pool, redis_url, user_id, email, now)
            .await
            .map_err(|_| AuthError::Server(Denial::ServerError))?;
    }
    let to_opt_string = |value: &serde_json::Value| match value {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Null => None,
        other => Some(other.to_string()),
    };
    let fields = pidash_db::auth_oauth::queries::account::TokenFields {
        access_token: to_opt_string(&tokens.access_token),
        refresh_token: to_opt_string(&tokens.refresh_token),
        access_token_expired_at: tokens.access_token_expired_at,
        refresh_token_expired_at: tokens.refresh_token_expired_at,
        id_token: to_opt_string(&tokens.id_token),
    };
    let mut conn = pool
        .acquire()
        .await
        .map_err(|_| AuthError::Server(Denial::ServerError))?;
    pidash_db::auth_oauth::queries::account::create_update_account(
        &mut conn,
        user_id,
        PROVIDER,
        user_json,
        &fields,
        now,
        uuid::Uuid::new_v4(),
        None,
    )
    .await
    .map_err(|_| AuthError::Server(Denial::ServerError))?;
    Ok(AuthenticatedUser {
        id: user_id,
        password_field: password_field.to_owned(),
    })
}

// ---------------------------------------------------------------------------
// §12 The four handlers + routes
// ---------------------------------------------------------------------------

#[allow(clippy::result_large_err)]
fn pool_or_500(state: &AppState) -> Result<sqlx::PgPool, Response> {
    state
        .pools()
        .map(|pools| pools.primary().clone())
        .ok_or_else(|| Denial::ServerError.into_response())
}

#[allow(clippy::result_large_err)]
fn setup_blocked(gate: Result<Option<bool>, sqlx::Error>) -> Result<bool, Response> {
    match gate {
        Ok(Some(true)) => Ok(false),
        Ok(_) => Ok(true),
        Err(_) => Err(Denial::ServerError.into_response()),
    }
}

/// `GET /auth/gitea/` (`views/app/gitea.py:27-66`).
async fn app_initiate(
    State(state): State<AppState>,
    Query(query): Query<QueryMap>,
    headers: HeaderMap,
    extension: Option<Extension<SessionHandle>>,
) -> Response {
    let settings = state.settings().clone();
    let base = app_base(&settings);
    let raw_next = initiate_prologue(&extension, &query, &base);
    let pool = match pool_or_500(&state) {
        Ok(pool) => pool,
        Err(response) => return response,
    };
    let blocked = match setup_blocked(instance_setup_done(&pool).await) {
        Ok(blocked) => blocked,
        Err(response) => return response,
    };
    if blocked {
        let exc = AuthenticationException::new(
            INSTANCE_NOT_CONFIGURED_CODE,
            INSTANCE_NOT_CONFIGURED_NAME,
            Vec::new(),
        );
        let params = initiate_error_params(&exc, raw_next.as_deref(), true);
        return redirect(app_error_location(&base, &params));
    }
    let inputs = request_inputs(&headers);
    let state_hex = uuid::Uuid::new_v4().simple().to_string();
    match gitea_config(&pool, &settings.secret_key).await {
        Err(exc) => {
            let params = initiate_error_params(&exc, raw_next.as_deref(), true);
            redirect(app_error_location(&base, &params))
        }
        Ok(config) => {
            let url = pidash_services::auth_oauth::providers::gitea_auth_url(
                &config.client_id,
                inputs.is_secure,
                &inputs.host,
                &state_hex,
                &config.host_normalized,
            );
            write_session_key(&extension, SESSION_STATE, state_hex);
            redirect(url)
        }
    }
}

/// `GET /auth/gitea/callback/` (`views/app/gitea.py:69-107`).
async fn app_callback(
    State(state): State<AppState>,
    Query(query): Query<QueryMap>,
    headers: HeaderMap,
    extension: Option<Extension<SessionHandle>>,
) -> Response {
    let settings = state.settings().clone();
    let session = read_callback_session(&extension);
    let code = query_last(&query, "code").filter(|v| !v.is_empty());
    let req_state = query_last(&query, "state");
    // Echo iff the session carries a next_path (`if next_path:`).
    // State check first; the echo here is RAW (`str(next_path)`).
    let echo_raw = session.next_path.as_deref().filter(|v| !v.is_empty());
    if req_state.as_deref() != Some(session.state.as_deref().unwrap_or("")) {
        let exc = AuthenticationException::new(
            GITEA_PROVIDER_ERROR_CODE,
            GITEA_PROVIDER_ERROR_NAME,
            Vec::new(),
        );
        let params = error_params(exc.error_code, &exc.error_message, echo_raw);
        return redirect(app_callback_error_location(
            session.host.as_deref(),
            &params,
        ));
    }
    // Missing code; the echo is validated from here on.
    let validated_echo = echo_raw.map(pidash_services::auth_session::shapes::validate_next_path);
    let Some(code) = code else {
        let exc = AuthenticationException::new(
            GITEA_PROVIDER_ERROR_CODE,
            GITEA_PROVIDER_ERROR_NAME,
            Vec::new(),
        );
        let params = error_params(
            exc.error_code,
            &exc.error_message,
            validated_echo.as_deref(),
        );
        return redirect(app_callback_error_location(
            session.host.as_deref(),
            &params,
        ));
    };
    let pool = match pool_or_500(&state) {
        Ok(pool) => pool,
        Err(response) => return response,
    };
    let inputs = request_inputs(&headers);
    let redirect_uri = redirect_uri_for(inputs.is_secure, &inputs.host);
    let now = chrono::Utc::now();
    let authenticated = authenticate_user(
        &pool,
        &settings.secret_key,
        settings.redis.url.as_deref(),
        &settings.storage,
        settings.file_size_limit.max(0) as u64,
        &inputs,
        &code,
        &redirect_uri,
        true,
        now,
    )
    .await;
    let user = match authenticated {
        Ok(user) => user,
        Err(AuthError::Provider(exc)) => {
            let params = error_params(
                exc.error_code,
                &exc.error_message,
                validated_echo.as_deref(),
            );
            return redirect(app_callback_error_location(
                session.host.as_deref(),
                &params,
            ));
        }
        Err(AuthError::Server(denial)) => return denial.into_response(),
    };
    let base = app_base(&settings);
    user_login_session(
        &extension,
        &user.id,
        &user.password_field,
        settings.secret_key.as_bytes(),
        inputs.user_agent.as_deref(),
        inputs.ip.as_deref(),
        &base,
    );
    // `if next_path: validate else: get_redirection_path` — a present
    // but invalid next_path validates to `""` and stays `""` (it does NOT
    // fall back to the redirection path); only an absent/empty one does.
    let path = match session.next_path.as_deref() {
        Some(next) if !next.is_empty() => {
            pidash_services::auth_session::shapes::validate_next_path(next)
        }
        _ => {
            match redirection_path(&pool, user.id, &user_email(&pool, &user.id).await, now).await {
                Ok(path) => path,
                Err(_) => return Denial::ServerError.into_response(),
            }
        }
    };
    redirect(app_success_location(session.host.as_deref(), &path))
}

async fn user_email(pool: &sqlx::PgPool, user_id: &uuid::Uuid) -> String {
    sqlx::query_as::<_, (Option<String>,)>(r#"SELECT email FROM users WHERE id = $1"#)
        .bind(user_id)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten()
        .and_then(|(email,)| email)
        .unwrap_or_default()
}

/// `GET /auth/spaces/gitea/` (`views/space/gitea.py:28-70`): the space
/// twin — f-string error channel, raw echo on the provider-error branch.
async fn space_initiate(
    State(state): State<AppState>,
    Query(query): Query<QueryMap>,
    headers: HeaderMap,
    extension: Option<Extension<SessionHandle>>,
) -> Response {
    let settings = state.settings().clone();
    let base = space_base(&settings);
    let raw_next = initiate_prologue(&extension, &query, &base);
    let pool = match pool_or_500(&state) {
        Ok(pool) => pool,
        Err(response) => return response,
    };
    let blocked = match setup_blocked(instance_setup_done(&pool).await) {
        Ok(blocked) => blocked,
        Err(response) => return response,
    };
    if blocked {
        let exc = AuthenticationException::new(
            INSTANCE_NOT_CONFIGURED_CODE,
            INSTANCE_NOT_CONFIGURED_NAME,
            Vec::new(),
        );
        let params = initiate_error_params(&exc, raw_next.as_deref(), true);
        return redirect(space_error_location(&base, &params));
    }
    let inputs = request_inputs(&headers);
    let state_hex = uuid::Uuid::new_v4().simple().to_string();
    match gitea_config(&pool, &settings.secret_key).await {
        Err(exc) => {
            // Raw echo here (`str(next_path)`), unlike the app twin.
            let params = initiate_error_params(&exc, raw_next.as_deref(), false);
            redirect(space_error_location(&base, &params))
        }
        Ok(config) => {
            let url = pidash_services::auth_oauth::providers::gitea_auth_url(
                &config.client_id,
                inputs.is_secure,
                &inputs.host,
                &state_hex,
                &config.host_normalized,
            );
            write_session_key(&extension, SESSION_STATE, state_hex);
            redirect(url)
        }
    }
}

/// `GET /auth/spaces/gitea/callback/` (`views/space/gitea.py:73-117`):
/// validated echo on every branch, live `base_host` (never the session
/// host), provider without callback (no invitation workflow), f-string
/// success target.
async fn space_callback(
    State(state): State<AppState>,
    Query(query): Query<QueryMap>,
    headers: HeaderMap,
    extension: Option<Extension<SessionHandle>>,
) -> Response {
    let settings = state.settings().clone();
    let base = space_base(&settings);
    let session = read_callback_session(&extension);
    let code = query_last(&query, "code").filter(|v| !v.is_empty());
    let req_state = query_last(&query, "state");
    // Echo iff the session carries a next_path (`if next_path:`); the
    // value is validated on every space branch.
    let echo_raw = session.next_path.as_deref().filter(|v| !v.is_empty());
    let validated_next = echo_raw.map(pidash_services::auth_session::shapes::validate_next_path);
    if req_state.as_deref() != Some(session.state.as_deref().unwrap_or("")) {
        let exc = AuthenticationException::new(
            GITEA_PROVIDER_ERROR_CODE,
            GITEA_PROVIDER_ERROR_NAME,
            Vec::new(),
        );
        let params = error_params(
            exc.error_code,
            &exc.error_message,
            validated_next.as_deref(),
        );
        return redirect(space_error_location(&base, &params));
    }
    let Some(code) = code else {
        let exc = AuthenticationException::new(
            GITEA_PROVIDER_ERROR_CODE,
            GITEA_PROVIDER_ERROR_NAME,
            Vec::new(),
        );
        let params = error_params(
            exc.error_code,
            &exc.error_message,
            validated_next.as_deref(),
        );
        return redirect(space_error_location(&base, &params));
    };
    let pool = match pool_or_500(&state) {
        Ok(pool) => pool,
        Err(response) => return response,
    };
    let inputs = request_inputs(&headers);
    let redirect_uri = redirect_uri_for(inputs.is_secure, &inputs.host);
    let now = chrono::Utc::now();
    let authenticated = authenticate_user(
        &pool,
        &settings.secret_key,
        settings.redis.url.as_deref(),
        &settings.storage,
        settings.file_size_limit.max(0) as u64,
        &inputs,
        &code,
        &redirect_uri,
        false,
        now,
    )
    .await;
    let user = match authenticated {
        Ok(user) => user,
        Err(AuthError::Provider(exc)) => {
            let params = error_params(
                exc.error_code,
                &exc.error_message,
                validated_next.as_deref(),
            );
            return redirect(space_error_location(&base, &params));
        }
        Err(AuthError::Server(denial)) => return denial.into_response(),
    };
    user_login_session(
        &extension,
        &user.id,
        &user.password_field,
        settings.secret_key.as_bytes(),
        inputs.user_agent.as_deref(),
        inputs.ip.as_deref(),
        &base,
    );
    let next = validated_next.unwrap_or_default();
    redirect(space_success_location(&base, &next))
}

// ---------------------------------------------------------------------------
// §13 Routes
// ---------------------------------------------------------------------------

/// Register the four Gitea OAuth GET routes. Nothing else: sibling auth
/// paths stay unmatched and proxy to Django. Unowned methods on these
/// paths proxy too — plain `View`s sit behind `CsrfViewMiddleware`, and
/// DRF answers metadata/405s that Rust must not shadow; `HEAD` rides
/// axum's `get` handling like Django's `GET`-backed `HEAD`.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/auth/gitea/", owned(axum::routing::get(app_initiate)))
        .route(
            "/auth/gitea/callback/",
            owned(axum::routing::get(app_callback)),
        )
        .route(
            "/auth/spaces/gitea/",
            owned(axum::routing::get(space_initiate)),
        )
        .route(
            "/auth/spaces/gitea/callback/",
            owned(axum::routing::get(space_callback)),
        )
}

/// An owned path: GET serves from Rust, every other method falls through
/// to Django (its 405-after-auth and metadata responses live there).
fn owned(
    get_handler: axum::routing::MethodRouter<AppState>,
) -> axum::routing::MethodRouter<AppState> {
    get_handler
        .post(crate::edge::proxy)
        .put(crate::edge::proxy)
        .patch(crate::edge::proxy)
        .delete(crate::edge::proxy)
        .options(crate::edge::proxy)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> serde_json::Value {
        let path = format!(
            "{}/../../fixtures/auth_oauth/{name}",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(&path).expect("fixture exists"))
            .expect("fixture parses")
    }

    #[test]
    fn urljoin_missing_host_returns_relative_ref() {
        // F11 app-gitea quirk: `urljoin(None, "?error...")` is the bare ref.
        let location = app_callback_error_location(
            None,
            &error_params(5123, "GITEA_OAUTH_PROVIDER_ERROR", None),
        );
        assert!(location.starts_with("?error_code=5123"), "{location}");
        assert!(location.contains("error_message=GITEA_OAUTH_PROVIDER_ERROR"));
    }

    #[test]
    fn urljoin_drops_base_path_on_query_ref() {
        // F10 app-gitea quirk: `urljoin("http://host", "?...")` has an
        // empty path — the golden pins `""`, not `"/"`.
        let location = app_error_location(
            "http://app.example",
            &error_params(5112, "GITEA_NOT_CONFIGURED", Some("/x")),
        );
        assert_eq!(
            location,
            "http://app.example?error_code=5112&error_message=GITEA_NOT_CONFIGURED&next_path=%2Fx"
        );
    }

    #[test]
    fn urljoin_keeps_trailing_slash_base_path() {
        let location = app_error_location(
            "http://app.example/",
            &error_params(5000, "INSTANCE_NOT_CONFIGURED", None),
        );
        assert_eq!(
            location,
            "http://app.example/?error_code=5000&error_message=INSTANCE_NOT_CONFIGURED"
        );
    }

    #[test]
    fn urljoin_merges_relative_success_path() {
        // `urljoin(host, "onboarding")` — the redirection-path shape.
        assert_eq!(
            app_success_location(Some("http://app.example"), "onboarding"),
            "http://app.example/onboarding"
        );
        assert_eq!(
            app_success_location(Some("http://app.example/"), "/x"),
            "http://app.example/x"
        );
    }

    #[test]
    fn space_error_location_is_fstring_shape() {
        // Space twin never urljoins: the base path survives verbatim.
        let location = space_error_location(
            "http://space.example/spaces/",
            &error_params(5112, "GITEA_NOT_CONFIGURED", Some("/x")),
        );
        assert_eq!(
            location,
            "http://space.example/spaces/?error_code=5112&error_message=GITEA_NOT_CONFIGURED&next_path=%2Fx"
        );
    }

    #[test]
    fn space_success_location_appends_next() {
        assert_eq!(
            space_success_location("http://space.example/spaces/", "/x"),
            "http://space.example/spaces//x"
        );
        assert_eq!(
            space_success_location("http://space.example/spaces/", ""),
            "http://space.example/spaces/"
        );
    }

    #[test]
    fn error_params_order_matches_get_error_dict() {
        let params = error_params(5000, "INSTANCE_NOT_CONFIGURED", Some("/x"));
        let keys: Vec<&str> = params.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["error_code", "error_message", "next_path"]);
        assert_eq!(
            urlencode_pairs(&params),
            "error_code=5000&error_message=INSTANCE_NOT_CONFIGURED&next_path=%2Fx"
        );
    }

    #[test]
    fn f10_gitea_rows_replay() {
        let fx = fixture("F10_initiate.golden.json");
        assert_eq!(fx["app_gitea"]["provider_error_code"], 5112);
        assert_eq!(fx["app_gitea"]["not_configured_code"], 5000);
        assert_eq!(fx["space_gitea"]["provider_error_code"], 5112);
        // The fixture pins the channel difference this module ports.
        assert!(fx["app_gitea"]["error_channel"]
            .as_str()
            .unwrap()
            .contains("urljoin"));
        assert!(fx["space_gitea"]["error_channel"]
            .as_str()
            .unwrap()
            .contains("f-string"));
    }

    #[test]
    fn f11_gitea_rows_replay() {
        let fx = fixture("F11_callback.golden.json");
        assert_eq!(fx["app_gitea"]["provider_error_code"], 5123);
        assert_eq!(fx["space_gitea"]["provider_error_code"], 5123);
        assert!(fx["app_gitea"]["error_channel"]
            .as_str()
            .unwrap()
            .contains("RELATIVE"));
    }

    #[test]
    fn redirect_uri_scheme_follows_request() {
        assert_eq!(
            redirect_uri_for(false, "app.example"),
            "http://app.example/auth/gitea/callback/"
        );
        assert_eq!(
            redirect_uri_for(true, "app.example"),
            "https://app.example/auth/gitea/callback/"
        );
    }

    #[test]
    fn avatar_extension_map_matches_python() {
        assert_eq!(avatar_extension("image/jpeg"), Some("jpg"));
        assert_eq!(avatar_extension("image/jpg"), Some("jpg"));
        assert_eq!(avatar_extension("image/png"), Some("png"));
        assert_eq!(avatar_extension("image/gif"), Some("gif"));
        assert_eq!(avatar_extension("image/webp"), Some("webp"));
        assert_eq!(avatar_extension("image/svg+xml"), None);
        assert_eq!(avatar_extension(""), None);
    }

    #[test]
    fn sanitize_email_branches() {
        assert_eq!(
            sanitize_email(Some(&serde_json::json!("U@Example.COM"))).expect("valid"),
            "u@example.com"
        );
        let missing = sanitize_email(None).expect_err("missing");
        assert_eq!(missing.error_code, 5005);
        assert_eq!(missing.error_message, "INVALID_EMAIL");
        assert_eq!(
            missing.get_error_dict()[2],
            ("email".to_owned(), serde_json::Value::Null)
        );
        sanitize_email(Some(&serde_json::json!("not-an-email"))).expect_err("invalid");
    }

    #[test]
    fn display_name_shapes() {
        let mut rng = rand::rng();
        assert_eq!(display_name_for("u@example.com", &mut rng), "u");
        let fallback = display_name_for("no-at-sign", &mut rng);
        assert_eq!(fallback.len(), 6);
        assert!(fallback.chars().all(|c| c.is_ascii_alphabetic()));
        let color = random_color(&mut rng);
        assert_eq!(color.len(), 7);
        assert!(color.starts_with('#'));
        assert!(color[1..].chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn session_auth_hash_is_stable_hex() {
        let first = session_auth_hash("pbkdf2_sha256$600000$salt$hash", b"secret");
        let second = session_auth_hash("pbkdf2_sha256$600000$salt$hash", b"secret");
        assert_eq!(first, second);
        assert_eq!(first.len(), 64);
        assert!(first.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(first, session_auth_hash("other", b"secret"));
    }

    #[test]
    fn session_auth_hash_matches_django_oracle() {
        // Golden from the contract harness (`_harness/client.py::
        // _session_auth_hash`, Django 4.2 `salted_hmac` semantics).
        assert_eq!(
            session_auth_hash(
                "pbkdf2_sha256$600000$salt$hash",
                b"gate402-contract-secret-only"
            ),
            "47d27a6f5648b682973929db408d0fac49e45e06d7736d448b2d0149867aed10"
        );
    }

    #[test]
    fn query_last_reads_last_value() {
        let query: QueryMap = [
            ("code".to_owned(), OneOrMany::One("c1".to_owned())),
            (
                "state".to_owned(),
                OneOrMany::Many(vec!["s1".to_owned(), "s2".to_owned()]),
            ),
        ]
        .into_iter()
        .collect();
        assert_eq!(query_last(&query, "code").as_deref(), Some("c1"));
        assert_eq!(query_last(&query, "state").as_deref(), Some("s2"));
        assert_eq!(query_last(&query, "missing"), None);
    }

    #[test]
    fn default_json_shapes_match_python() {
        assert_eq!(
            default_onboarding_json(),
            serde_json::json!({
                "profile_complete": false, "workspace_create": false,
                "workspace_invite": false, "workspace_join": false,
            })
        );
        assert_eq!(
            default_issue_props_json(),
            serde_json::json!({"subscribed": true, "assigned": true, "created": true, "all_issues": true})
        );
    }
}
