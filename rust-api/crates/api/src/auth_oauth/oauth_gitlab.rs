#![forbid(unsafe_code)]

//! GitLab OAuth handlers (D-17, stage 5, PIDASHCONV-339).
//!
//! Ports `apps/api/pi_dash/authentication/views/app/gitlab.py` (initiate
//! GET + callback GET) and `apps/api/pi_dash/authentication/views/space/`
//! `gitlab.py` (the space twins) onto the merged D-17 foundation:
//!
//! - routes: `authentication/urls.py:105-120` (`gitlab/`,
//!   `gitlab/callback/`, `spaces/gitlab/`, `spaces/gitlab/callback/`
//!   under the `auth/` prefix);
//! - provider shape: `provider/oauth/gitlab.py:25-124` (already ported as
//!   `pidash_services::auth_oauth::providers`, PIDASHCONV-325);
//! - error codes: `adapter/error.py:23-54` (already ported as
//!   `pidash_services::auth_oauth::error`, PIDASHCONV-325);
//! - redirect builder + `base_host` + `validate_next_path`: already ported
//!   as `pidash_services::auth_session` (PIDASHCONV-340);
//! - account upsert SQL: already ported as
//!   `pidash_db::auth_oauth::queries::account` (PIDASHCONV-327);
//! - task emits: `user_activation_email` + `track_event` wire shapes in
//!   `pidash_services::auth_session::tasks` (PIDASHCONV-405); the handlers
//!   wrap them in `CeleryTaskMessage` + `queue::enqueue` exactly like
//!   `api/src/space/intake.rs`.
//!
//! Vectors replay `rust-api/fixtures/auth_oauth/F10_initiate.golden.json`
//! (AUTHOAUTH-F10) and `F11_callback.golden.json` (AUTHOAUTH-F11).
//!
//! Only the four GitLab GETs are registered, so the edge serves exactly
//! this family from Rust while every sibling path keeps proxying to
//! Django — route registration is the cutover granularity, no flag
//! needed. Non-GET methods proxy so Django's own 405s survive.
//!
//! Ported bugs (translate, don't redesign; also listed in the PR):
//!
//! - B1 (`views/space/gitlab.py:66-69`): `base_host =
//!   request.session.get("host")` shadows the imported `base_host()`
//!   helper for the whole function body, so every later
//!   `base_host(request=..., is_space=True)` call raises `TypeError` and
//!   Django answers 500 on EVERY input — mismatch, missing code, and the
//!   valid path alike (the valid path runs `authenticate()` +
//!   `user_login()` first: F11 `space_google_github_gitlab_PORT_BUG`).
//!   The port resolves the same branch conditions, runs the same side
//!   effects on the valid path, and answers 500 at each redirect-build
//!   site instead of redirecting.
//! - B2 (F11 `order`): the callback checks state BEFORE code — a request
//!   with both wrong answers the state-mismatch branch.
//! - B3 (F10 `next_path_session`): space google/github/gitlab initiates
//!   never store `next_path` (only app + space-gitea do); ported as-is.
//! - B4 (`adapter/base.py:299`, also in `PORTED_BUGS`): `is_signup =
//!   bool(user)` is inverted as written (existing user reports `True`);
//!   kept verbatim, including the `check_sync_enabled() and not
//!   is_signup` gate that therefore syncs existing users only.
//! - B5 (`workspace_project_join.py:76-87`, also in `PORTED_BUGS`):
//!   `ProjectMember` bulk_create omits the non-nullable `project_id`, so
//!   any accepted project invite turns the callback into a 500.
//! - B6 (`adapter/oauth.py:134-140`): `create_update_account` swallows
//!   `DatabaseError`/`IntegrityError` (logs only); ported as
//!   `UpsertOutcome::ErrorSwallowed`.
//!
//! Deferred to follow-up issues (this issue's units are the four view
//! functions; these need transports other domains own):
//!
//! - avatar S3 upload + `FileAsset` create (`adapter/base.py:145-217`):
//!   download + content-type/size validation are ported; the SigV4 PUT
//!   belongs to the asset domain (no S3 client exists in `rust-api/`).
//!   Until it lands the port keeps Python's own upload-failure
//!   behaviour (avatar URL fallback, no asset row).
//! - invite-join cache invalidation (`workspace_project_join.py:38-44`):
//!   `RedisHandle` exposes no key-delete API; the deletes are recorded
//!   in the invite-join spec for the owning issue.
//!
//! Sibling provider issues (PIDASHCONV-335/336/341) merge their routers
//! in `super::routes`; merges keep both sides.

use axum::extract::{Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Router;
use chrono::Utc;
use sqlx::PgPool;

use crate::middleware::SessionHandle;
use crate::state::AppState;

/// App initiate path (`authentication/urls.py:106`).
pub const APP_INITIATE_PATH: &str = "/auth/gitlab/";
/// App callback path (`authentication/urls.py:107`).
pub const APP_CALLBACK_PATH: &str = "/auth/gitlab/callback/";
/// Space initiate path (`authentication/urls.py:109`).
pub const SPACE_INITIATE_PATH: &str = "/auth/spaces/gitlab/";
/// Space callback path (`authentication/urls.py:113`).
pub const SPACE_CALLBACK_PATH: &str = "/auth/spaces/gitlab/callback/";

/// Provider slug (`GitLabOAuthProvider.provider`, `gitlab.py:26`).
pub const PROVIDER: &str = "gitlab";
/// `GITLAB_NOT_CONFIGURED` (`adapter/error.py:47`).
pub const NOT_CONFIGURED_CODE: i32 = 5111;
/// `GITLAB_NOT_CONFIGURED` name.
pub const NOT_CONFIGURED_NAME: &str = "GITLAB_NOT_CONFIGURED";
/// `GITLAB_OAUTH_PROVIDER_ERROR` (`adapter/error.py:52`).
pub const PROVIDER_ERROR_CODE: i32 = 5121;
/// `GITLAB_OAUTH_PROVIDER_ERROR` name.
pub const PROVIDER_ERROR_NAME: &str = "GITLAB_OAUTH_PROVIDER_ERROR";
/// `INSTANCE_NOT_CONFIGURED` (`adapter/error.py:24`).
pub const INSTANCE_NOT_CONFIGURED_CODE: i32 = 5000;
/// `INSTANCE_NOT_CONFIGURED` name.
pub const INSTANCE_NOT_CONFIGURED_NAME: &str = "INSTANCE_NOT_CONFIGURED";

/// The four owned paths, in `authentication/urls.py` order.
pub const OWNED_PATHS: [&str; 4] = [
    APP_INITIATE_PATH,
    APP_CALLBACK_PATH,
    SPACE_INITIATE_PATH,
    SPACE_CALLBACK_PATH,
];

/// Merge the four GitLab GET routes. Sibling provider issues merge
/// theirs in `super::routes`; merges keep both sides.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            APP_INITIATE_PATH,
            owned(axum::routing::get(app_initiate), &["GET"]),
        )
        .route(
            APP_CALLBACK_PATH,
            owned(axum::routing::get(app_callback), &["GET"]),
        )
        .route(
            SPACE_INITIATE_PATH,
            owned(axum::routing::get(space_initiate), &["GET"]),
        )
        .route(
            SPACE_CALLBACK_PATH,
            owned(axum::routing::get(space_callback), &["GET"]),
        )
}

/// Optional peer address (`REMOTE_ADDR` for `get_client_ip`):
/// `ConnectInfo` implements `FromRequestParts` but not axum's
/// `OptionalFromRequestParts`, so `Option<ConnectInfo<..>>` is not a
/// valid extractor — this local extractor reads the extension when
/// the server installed it (`into_make_service_with_connect_info`)
/// and yields `None` otherwise (unit tests, pool-less states).
struct MaybePeer(Option<std::net::SocketAddr>);

impl<S> axum::extract::FromRequestParts<S> for MaybePeer
where
    S: Send + Sync,
{
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        Ok(MaybePeer(
            parts
                .extensions
                .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
                .map(|info| info.0),
        ))
    }
}

/// A GitLab OAuth path: the owned methods serve from Rust while every
/// other method falls through to Django (a plain `View` answers 405
/// there), following the pilot pattern: registration is the cutover
/// granularity.
fn owned(
    methods: axum::routing::MethodRouter<AppState>,
    owned: &[&str],
) -> axum::routing::MethodRouter<AppState> {
    let mut router = methods;
    for other in ["GET", "POST", "PUT", "PATCH", "DELETE", "OPTIONS"] {
        if owned.contains(&other) {
            continue;
        }
        router = match other {
            "GET" => router.get(crate::edge::proxy),
            "POST" => router.post(crate::edge::proxy),
            "PUT" => router.put(crate::edge::proxy),
            "PATCH" => router.patch(crate::edge::proxy),
            "DELETE" => router.delete(crate::edge::proxy),
            _ => router.options(crate::edge::proxy),
        };
    }
    router
}

/// `handle_exception`'s generic 500 branch (same body the sibling
/// handler ports use; the contract pins the status, not the page).
pub const SERVER_ERROR_BODY: &str = r#"{"error":"Something went wrong please try again later"}"#;

/// Handler failure with its exact status + body.
#[derive(Debug)]
enum Denial {
    ServerError,
}

impl IntoResponse for Denial {
    fn into_response(self) -> Response {
        match self {
            Denial::ServerError => (
                StatusCode::INTERNAL_SERVER_ERROR,
                [(header::CONTENT_TYPE, "application/json")],
                SERVER_ERROR_BODY,
            )
                .into_response(),
        }
    }
}

fn pool_of(state: &AppState) -> Result<&PgPool, Denial> {
    state
        .pools()
        .map(|pools| pools.primary())
        .ok_or(Denial::ServerError)
}

/// `HttpResponseRedirect(url)`: 302 with a `Location` header and an
/// empty body.
fn redirect_response(location: String) -> Response {
    Response::builder()
        .status(StatusCode::FOUND)
        .header(header::LOCATION, location)
        .body(axum::body::Body::empty())
        .expect("redirect response")
}

/// Django `QueryDict.get` (last wins) over the raw query pairs.
/// `?code=a&code=b` reads `b`, exactly like `request.GET.get("code")`.
fn query_last(pairs: &[(String, String)], key: &str) -> Option<String> {
    pairs
        .iter()
        .rev()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.clone())
}

/// Request host: the `Host` header Django's `request.get_host()`
/// reads (proxy deployments set it; the edge preserves it).
fn request_host(headers: &HeaderMap) -> String {
    headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_owned()
}

/// `request.is_secure()`: https scheme, or `X-Forwarded-Proto: https`
/// when the deployment trusts the proxy header (`production.py`
/// `SECURE_PROXY_SSL_HEADER`, surfaced as
/// `secure_proxy_ssl_header`).
fn request_is_secure(headers: &HeaderMap, trust_forwarded_proto: bool) -> bool {
    trust_forwarded_proto
        && headers
            .get("x-forwarded-proto")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.eq_ignore_ascii_case("https"))
}

/// `base_host` for the OAuth surface (`host.py:19-67`), over the
/// runtime URL settings.
fn app_base(settings: &pidash_db::config::Settings) -> String {
    pidash_services::auth_session::base_host(
        &pidash_services::auth_session::HostSettings {
            web_url: settings.urls.web_url.as_deref(),
            app_base_url: settings.urls.app_base_url.as_deref(),
            admin_base_url: settings.urls.admin_base_url.as_deref(),
            space_base_url: settings.urls.space_base_url.as_deref(),
            admin_base_path: Some(settings.urls.admin_base_path.as_str()),
            space_base_path: Some(settings.urls.space_base_path.as_str()),
        },
        false,
        false,
        true,
    )
}

/// `base_host(..., is_space=True)` (`host.py:40-53`).
fn space_base(settings: &pidash_db::config::Settings) -> String {
    pidash_services::auth_session::base_host(
        &pidash_services::auth_session::HostSettings {
            web_url: settings.urls.web_url.as_deref(),
            app_base_url: settings.urls.app_base_url.as_deref(),
            admin_base_url: settings.urls.admin_base_url.as_deref(),
            space_base_url: settings.urls.space_base_url.as_deref(),
            admin_base_path: Some(settings.urls.admin_base_path.as_str()),
            space_base_path: Some(settings.urls.space_base_path.as_str()),
        },
        false,
        true,
        false,
    )
}

/// `get_allowed_hosts()` (`path_validator.py:60-78`).
fn allowed_hosts(settings: &pidash_db::config::Settings) -> Vec<String> {
    pidash_services::auth_session::allowed_hosts_for(
        settings.urls.web_url.as_deref(),
        settings.urls.app_base_url.as_deref(),
        settings.urls.admin_base_url.as_deref(),
        settings.urls.space_base_url.as_deref(),
    )
    .into_iter()
    .map(str::to_owned)
    .collect()
}

/// Error-redirect `Location`: `get_safe_redirect_url(base_url,
/// next_path, params)` (`path_validator.py:106-145`) over the full
/// `get_error_dict()` pairs in order (`error_code`, `error_message`,
/// then payload — e.g. `INVALID_EMAIL` / `SIGNUP_DISABLED` carry the
/// raw `email` value).
fn error_location(
    base_url: &str,
    next_path: Option<&str>,
    pairs: &[(&str, pidash_services::auth_session::ParamValue)],
    allowed_hosts: &[&str],
) -> String {
    pidash_services::auth_session::get_safe_redirect_url(
        base_url,
        next_path.unwrap_or(""),
        pairs,
        allowed_hosts,
    )
}

/// `exc.get_error_dict()` for the payload-less errors
/// (`INSTANCE_NOT_CONFIGURED`, `GITLAB_NOT_CONFIGURED`,
/// `GITLAB_OAUTH_PROVIDER_ERROR`): exactly `error_code` +
/// `error_message`, in order (`adapter/error.py:88-92`).
fn plain_error_location(
    base_url: &str,
    next_path: Option<&str>,
    error_code: i32,
    error_message: &str,
    allowed_hosts: &[&str],
) -> String {
    let params = [
        ("error_code", param_int(error_code)),
        ("error_message", param_str(error_message)),
    ];
    error_location(base_url, next_path, &params, allowed_hosts)
}

/// `exc.get_error_dict()` with a JSON payload (`INVALID_EMAIL`,
/// `SIGNUP_DISABLED`: `payload={"email": email}`): base pairs plus
/// the payload entries in order, overwriting on key collision
/// (`adapter/error.py:88-92`).
fn payload_error_location(
    base_url: &str,
    next_path: Option<&str>,
    error_code: i32,
    error_message: &str,
    payload: &[(&str, serde_json::Value)],
    allowed_hosts: &[&str],
) -> String {
    let mut params: Vec<(&str, pidash_services::auth_session::ParamValue)> = vec![
        ("error_code", param_int(error_code)),
        ("error_message", param_str(error_message)),
    ];
    let mapped: Vec<pidash_services::auth_session::ParamValue> =
        payload.iter().map(|(_, v)| json_param(v)).collect();
    for ((key, _), value) in payload.iter().zip(mapped) {
        match params.iter_mut().find(|(k, _)| k == key) {
            Some(slot) => slot.1 = value,
            None => params.push((key, value)),
        }
    }
    error_location(base_url, next_path, &params, allowed_hosts)
}

/// JSON value → redirect param (`urlencode` renders `True`/`False` /
/// `None` Python-style; `ParamValue` already does).
fn json_param(value: &serde_json::Value) -> pidash_services::auth_session::ParamValue {
    match value {
        serde_json::Value::Null => pidash_services::auth_session::ParamValue::Null,
        serde_json::Value::Bool(b) => pidash_services::auth_session::ParamValue::Bool(*b),
        serde_json::Value::Number(n) => match n.as_i64() {
            Some(i) => pidash_services::auth_session::ParamValue::Int(i),
            None => param_str(&n.to_string()),
        },
        serde_json::Value::String(s) => param_str(s),
        // Arrays/objects never occur in these payloads; compact JSON
        // is the closest rendering.
        other => param_str(&other.to_string()),
    }
}

fn param_int(value: i32) -> pidash_services::auth_session::ParamValue {
    pidash_services::auth_session::ParamValue::Int(i64::from(value))
}

fn param_str(value: &str) -> pidash_services::auth_session::ParamValue {
    pidash_services::auth_session::ParamValue::Str(value.to_owned())
}

// ---------------------------------------------------------------------------
// Provider configuration
// ---------------------------------------------------------------------------

/// Resolved GitLab provider configuration: `get_configuration_value`
/// (`gitlab.py:30-46`) with the caller defaults evaluated exactly as
/// written — `os.environ.get("GITLAB_CLIENT_ID")`,
/// `os.environ.get("GITLAB_CLIENT_SECRET")`, and
/// `os.environ.get("GITLAB_HOST", "https://gitlab.com")`.
///
/// All three keys are db-sourced in the registry, so the legacy shim
/// (`pidash_db::config::legacy`, the port of `instance_value.py`) is
/// used: present rows (decrypted for the secret) win, absent rows fall
/// back to the caller default — registry defaults are ignored, exactly
/// like Python.
struct GitlabConfig {
    client_id: Option<String>,
    client_secret: Option<String>,
    host: Option<String>,
}

/// Empty-string-is-falsy (`gitlab.py:48`): `None` and `""` both read
/// as missing, the Semantic-traps item.
fn config_value(value: &pidash_db::config::value::ConfigValue) -> Option<String> {
    match value {
        pidash_db::config::value::ConfigValue::Str(s) if !s.is_empty() => Some(s.clone()),
        pidash_db::config::value::ConfigValue::Int(v) => Some(v.to_string()),
        pidash_db::config::value::ConfigValue::Float(v) => Some(v.to_string()),
        pidash_db::config::value::ConfigValue::Bool(true) => Some("True".to_owned()),
        pidash_db::config::value::ConfigValue::Bool(false) => Some("False".to_owned()),
        _ => None,
    }
}

fn env_or_null(name: &str) -> pidash_db::config::value::ConfigValue {
    match std::env::var(name) {
        Ok(v) => pidash_db::config::value::ConfigValue::Str(v),
        Err(_) => pidash_db::config::value::ConfigValue::Null,
    }
}

async fn gitlab_config(pool: &PgPool, secret_key: &str) -> Result<GitlabConfig, Denial> {
    use pidash_db::config::legacy::{get_configuration_values, LegacyItem};
    let store = pidash_db::config::accessor::PgConfigStore::new(pool.clone());
    let registry = pidash_db::config::registry::global();
    let keyring = pidash_services::license::encryption::Keyring::from_secret(secret_key);
    let host_default = std::env::var("GITLAB_HOST")
        .unwrap_or_else(|_| pidash_services::auth_oauth::providers::GITLAB_HOST_DEFAULT.to_owned());
    let values = get_configuration_values(
        registry,
        &store,
        &keyring,
        &[
            LegacyItem::new("GITLAB_CLIENT_ID", env_or_null("GITLAB_CLIENT_ID")),
            LegacyItem::new("GITLAB_CLIENT_SECRET", env_or_null("GITLAB_CLIENT_SECRET")),
            LegacyItem::new(
                "GITLAB_HOST",
                pidash_db::config::value::ConfigValue::Str(host_default),
            ),
        ],
    )
    .await
    .map_err(|_| Denial::ServerError)?;
    let [client_id, client_secret, host] = values.as_slice() else {
        return Err(Denial::ServerError);
    };
    Ok(GitlabConfig {
        client_id: config_value(client_id),
        client_secret: config_value(client_secret),
        host: config_value(host),
    })
}

/// `if not (GITLAB_CLIENT_ID and GITLAB_CLIENT_SECRET and GITLAB_HOST)`
/// (`gitlab.py:48-52`).
fn gitlab_configured(config: &GitlabConfig) -> bool {
    pidash_services::auth_oauth::providers::gitlab_configured(
        config.client_id.as_deref(),
        config.client_secret.as_deref(),
        config.host.as_deref(),
    )
}

// ---------------------------------------------------------------------------
// Initiate
// ---------------------------------------------------------------------------

/// Shared initiate core (`views/app/gitlab.py:30-65`,
/// `views/space/gitlab.py:30-63`): every branch answers 302.
///
/// `is_space` selects the base (`is_app` / `is_space`) and the
/// `next_path` session write (B3): the app view stores the raw query
/// value when present (`if next_path:` — empty reads as absent); the
/// space view never stores it.
async fn initiate(
    state: &AppState,
    headers: &HeaderMap,
    query: &[(String, String)],
    session: Option<axum::Extension<SessionHandle>>,
    is_space: bool,
) -> Result<Response, Denial> {
    let pool = pool_of(state)?;
    let settings = state.settings();
    let base = if is_space {
        space_base(settings)
    } else {
        app_base(settings)
    };
    let allowed_owned = allowed_hosts(settings);
    let allowed: Vec<&str> = allowed_owned.iter().map(String::as_str).collect();
    // `request.session["host"] = base_host(...)` runs first, on every
    // branch (F10 `session_host`).
    if let Some(axum::Extension(handle)) = session.as_ref() {
        handle
            .lock()
            .set("host".to_owned(), serde_json::Value::String(base.clone()));
    }
    let next_path = query_last(query, "next_path");
    if !is_space {
        if let Some(ref path) = next_path {
            if !path.is_empty() {
                if let Some(axum::Extension(handle)) = session.as_ref() {
                    handle.lock().set(
                        "next_path".to_owned(),
                        serde_json::Value::String(path.clone()),
                    );
                }
            }
        }
    }

    // `Instance.objects.first()`; missing or not set up answers the
    // `INSTANCE_NOT_CONFIGURED` redirect (5000).
    let instance = pidash_db::license::queries::fetch_instance_first(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let setup_done = instance.as_ref().is_some_and(|row| row.is_setup_done);
    if !setup_done {
        return Ok(redirect_response(plain_error_location(
            &base,
            next_path.as_deref(),
            INSTANCE_NOT_CONFIGURED_CODE,
            INSTANCE_NOT_CONFIGURED_NAME,
            &allowed,
        )));
    }

    // Provider construction: unconfigured answers the
    // `GITLAB_NOT_CONFIGURED` redirect (5111) before any outbound HTTP.
    let config = gitlab_config(pool, state.settings().secret_key.as_str()).await?;
    if !gitlab_configured(&config) {
        return Ok(redirect_response(plain_error_location(
            &base,
            next_path.as_deref(),
            NOT_CONFIGURED_CODE,
            NOT_CONFIGURED_NAME,
            &allowed,
        )));
    }

    // Success: `state = uuid.uuid4().hex`, stored, then 302 to the
    // provider authorization URL.
    let hex = uuid::Uuid::new_v4().simple().to_string();
    if let Some(axum::Extension(handle)) = session.as_ref() {
        handle
            .lock()
            .set("state".to_owned(), serde_json::Value::String(hex.clone()));
    }
    let host = request_host(headers);
    let is_secure = request_is_secure(headers, settings.secure_proxy_ssl_header);
    let auth_url = pidash_services::auth_oauth::providers::gitlab_auth_url(
        config.client_id.as_deref().unwrap_or(""),
        is_secure,
        &host,
        &hex,
        config.host.as_deref().unwrap_or(""),
    );
    Ok(redirect_response(auth_url))
}

/// `GitLabOauthInitiateEndpoint.get` (`views/app/gitlab.py:31-65`).
async fn app_initiate(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<Vec<(String, String)>>,
    session: Option<axum::Extension<SessionHandle>>,
) -> Result<Response, Denial> {
    initiate(&state, &headers, &query, session, false).await
}

/// `GitLabOauthInitiateSpaceEndpoint.get`
/// (`views/space/gitlab.py:31-63`).
async fn space_initiate(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<Vec<(String, String)>>,
    session: Option<axum::Extension<SessionHandle>>,
) -> Result<Response, Denial> {
    initiate(&state, &headers, &query, session, true).await
}

// ---------------------------------------------------------------------------
// Callback
// ---------------------------------------------------------------------------

/// An `AuthenticationException` equivalent: carries the exact
/// `error_code` + `error_message` the except-branches redirect with.
/// Anything else (DB errors, `TypeError`-class bugs) is
/// [`Denial::ServerError`], mirroring Python's `except
/// AuthenticationException` (which lets everything else escape to the
/// generic 500).
#[derive(Debug)]
struct ProviderError {
    code: i32,
    message: String,
    /// Raw `payload` entries (`{"email": ...}` for `INVALID_EMAIL` /
    /// `SIGNUP_DISABLED`); rendered after the base pairs, in order.
    payload: Vec<(String, serde_json::Value)>,
}

impl ProviderError {
    fn gitlab_provider_error() -> Self {
        Self {
            code: PROVIDER_ERROR_CODE,
            message: PROVIDER_ERROR_NAME.to_owned(),
            payload: Vec::new(),
        }
    }

    fn from_services(exc: pidash_services::auth_oauth::error::AuthenticationException) -> Self {
        let dict = exc.get_error_dict();
        let mut out = Self {
            code: exc.error_code,
            message: exc.error_message.clone(),
            payload: Vec::new(),
        };
        for (key, value) in dict.into_iter().skip(2) {
            out.payload.push((key, value));
        }
        out
    }

    fn with_payload(code: i32, message: &str, payload: Vec<(String, serde_json::Value)>) -> Self {
        Self {
            code,
            message: message.to_owned(),
            payload,
        }
    }
}

/// Success-redirect `Location`: `get_safe_redirect_url(base_url,
/// next_path=path, params={})` (`views/app/gitlab.py:101-104`).
fn success_location(base_url: &str, path: &str, allowed_hosts: &[&str]) -> String {
    pidash_services::auth_session::get_safe_redirect_url(base_url, path, &[], allowed_hosts)
}

/// Snapshot of the callback's session reads (`views/app/gitlab.py:69-71`,
/// `views/space/gitlab.py:67-69`).
struct CallbackSession {
    /// `request.session.get("state", "")`.
    state: String,
    /// `request.session.get("next_path")` (absent → `None`).
    next_path: Option<String>,
}

fn read_callback_session(session: &Option<axum::Extension<SessionHandle>>) -> CallbackSession {
    let mut snapshot = session
        .as_ref()
        .map(|axum::Extension(handle)| handle.snapshot())
        .unwrap_or_else(crate::middleware::RequestSession::empty);
    CallbackSession {
        state: snapshot
            .get("state")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_owned(),
        next_path: snapshot
            .get("next_path")
            .and_then(|v| v.as_str())
            .map(str::to_owned),
    }
}

/// `GitLabCallbackEndpoint.get` (`views/app/gitlab.py:68-107`).
///
/// Order is the fixture order (B2): state check first, then the
/// missing-code check, then the provider chain. Every
/// `AuthenticationException` answers the `GITLAB_OAUTH_PROVIDER_ERROR`
/// redirect; anything else is the generic 500.
async fn app_callback(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<Vec<(String, String)>>,
    session: Option<axum::Extension<SessionHandle>>,
    peer: MaybePeer,
) -> Result<Response, Denial> {
    let settings = state.settings();
    let base = app_base(settings);
    let allowed_owned = allowed_hosts(settings);
    let allowed: Vec<&str> = allowed_owned.iter().map(String::as_str).collect();
    let callback = read_callback_session(&session);
    let code = query_last(&query, "code");
    let query_state = query_last(&query, "state").unwrap_or_default();

    if query_state != callback.state {
        return Ok(redirect_response(plain_error_location(
            &base,
            callback.next_path.as_deref(),
            PROVIDER_ERROR_CODE,
            PROVIDER_ERROR_NAME,
            &allowed,
        )));
    }
    if code.as_deref().unwrap_or("").is_empty() {
        return Ok(redirect_response(plain_error_location(
            &base,
            callback.next_path.as_deref(),
            PROVIDER_ERROR_CODE,
            PROVIDER_ERROR_NAME,
            &allowed,
        )));
    }

    let handle = session
        .as_ref()
        .map(|axum::Extension(handle)| handle.clone());
    // The except-branch echoes the session `next_path`
    // (`views/app/gitlab.py:105-107`).
    let session_next = callback.next_path.clone();
    let peer_addr = peer.0;
    match callback_success(
        &state,
        &headers,
        code.unwrap_or_default(),
        callback.next_path,
        handle,
        peer_addr,
        false,
    )
    .await
    {
        Ok(path) => Ok(redirect_response(success_location(&base, &path, &allowed))),
        Err(CallbackOutcome::Provider(error)) => {
            let payload: Vec<(&str, serde_json::Value)> = error
                .payload
                .iter()
                .map(|(k, v)| (k.as_str(), v.clone()))
                .collect();
            Ok(redirect_response(payload_error_location(
                &base,
                session_next.as_deref(),
                error.code,
                &error.message,
                &payload,
                &allowed,
            )))
        }
        Err(CallbackOutcome::Server) => Err(Denial::ServerError),
    }
}

/// The two terminal outcomes of the success chain: an
/// `AuthenticationException` (redirect) or anything else (500).
enum CallbackOutcome {
    Provider(ProviderError),
    Server,
}

impl From<Denial> for CallbackOutcome {
    fn from(_: Denial) -> Self {
        CallbackOutcome::Server
    }
}

impl From<ProviderError> for CallbackOutcome {
    fn from(error: ProviderError) -> Self {
        CallbackOutcome::Provider(error)
    }
}

/// `GitLabCallbackSpaceEndpoint.get` (`views/space/gitlab.py:66-105`),
/// B1 ported as-is: the `base_host` local shadows the helper, so
/// every redirect-build site raises `TypeError` and Django answers
/// 500 on every input.
///
/// The port resolves the same branch conditions and runs the same
/// side effects: mismatch / missing-code paths touch nothing and
/// answer 500; the valid path runs the space success chain first
/// (`authenticate()` + `user_login(is_space=True)` side effects, no
/// invitation processing since the space view passes no `callback`)
/// and then answers 500 at the f-string site. A provider
/// `AuthenticationException` likewise ends in 500 here (Python
/// catches it, then raises `TypeError` while building the error
/// redirect).
async fn space_callback(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<Vec<(String, String)>>,
    session: Option<axum::Extension<SessionHandle>>,
    peer: MaybePeer,
) -> Result<Response, Denial> {
    let callback = read_callback_session(&session);
    let code = query_last(&query, "code");
    let query_state = query_last(&query, "state").unwrap_or_default();

    if query_state == callback.state && !code.as_deref().unwrap_or("").is_empty() {
        let handle = session
            .as_ref()
            .map(|axum::Extension(handle)| handle.clone());
        // Side effects as-is; the outcome is discarded — Python raises
        // `TypeError` while building the success URL next.
        let peer_addr = peer.0;
        let _ = callback_success(
            &state,
            &headers,
            code.unwrap_or_default(),
            callback.next_path,
            handle,
            peer_addr,
            true,
        )
        .await;
    }
    Err(Denial::ServerError)
}

// ---------------------------------------------------------------------------
// Token exchange (`OauthAdapter.get_user_token/get_user_response`,
// `adapter/oauth.py:75-100`, gitlab `set_token_data/set_user_data`)
// ---------------------------------------------------------------------------

/// POST the token URL and parse the JSON body
/// (`get_user_token`, `oauth.py:75-84`): transport errors, HTTP error
/// statuses (`raise_for_status`) and JSON decode failures all raise
/// the provider error — `requests`' `JSONDecodeError` subclasses
/// `RequestException`, so it is caught too.
async fn fetch_token_json(
    token_url: &str,
    form: &[(&str, &str)],
) -> Result<serde_json::Value, ProviderError> {
    // Every failure below maps through the ported exchange kernel
    // (`exchange::map_exchange_error("gitlab")` → 5121).
    let exchange_error = || {
        ProviderError::from_services(pidash_services::auth_oauth::exchange::map_exchange_error(
            PROVIDER,
        ))
    };
    let client = reqwest::Client::builder()
        .build()
        .map_err(|_| exchange_error())?;
    let response = client
        .post(token_url)
        .header(reqwest::header::ACCEPT, "application/json")
        .form(form)
        .send()
        .await
        .map_err(|_| exchange_error())?;
    if !response.status().is_success() {
        return Err(exchange_error());
    }
    let body = response
        .bytes()
        .await
        .map_err(|_| ProviderError::gitlab_provider_error())?;
    // `response.json()` raises on empty/invalid bodies too
    // (`JSONDecodeError` subclasses `RequestException`).
    serde_json::from_slice::<serde_json::Value>(&body)
        .map_err(|_| ProviderError::gitlab_provider_error())
}

/// GET the userinfo URL with the `Bearer` header
/// (`get_user_response`, `oauth.py:86-100`). A missing access token
/// renders `Bearer None`, exactly like the f-string.
async fn fetch_userinfo_json(
    userinfo_url: &str,
    access_token: Option<&str>,
) -> Result<serde_json::Value, ProviderError> {
    let bearer = match access_token {
        Some(token) => format!("Bearer {token}"),
        None => "Bearer None".to_owned(),
    };
    let exchange_error = || {
        ProviderError::from_services(pidash_services::auth_oauth::exchange::map_exchange_error(
            PROVIDER,
        ))
    };
    let client = reqwest::Client::builder()
        .build()
        .map_err(|_| exchange_error())?;
    let response = client
        .get(userinfo_url)
        .header(reqwest::header::AUTHORIZATION, bearer)
        .send()
        .await
        .map_err(|_| exchange_error())?;
    if !response.status().is_success() {
        return Err(exchange_error());
    }
    let body = response.bytes().await.map_err(|_| exchange_error())?;
    serde_json::from_slice::<serde_json::Value>(&body).map_err(|_| exchange_error())
}

/// TextField prep for the account columns: `None` stays `None`
/// (violates the non-null columns → swallowed by B6); anything else
/// renders Python-`str` style.
fn text_or_none(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::Null => None,
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Bool(true) => Some("True".to_owned()),
        serde_json::Value::Bool(false) => Some("False".to_owned()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        other => Some(other.to_string()),
    }
}

// ---------------------------------------------------------------------------
// Email (`Adapter.sanitize_email`, `adapter/base.py:60-84`)
// ---------------------------------------------------------------------------

/// `INVALID_EMAIL` code + name (`adapter/error.py:25`).
pub const INVALID_EMAIL_CODE: i32 = 5005;
/// `INVALID_EMAIL` name.
pub const INVALID_EMAIL_NAME: &str = "INVALID_EMAIL";

/// Missing email: `if not email:` (`base.py:63-69`) carries the RAW
/// value in the payload.
fn invalid_email_raw(raw: &serde_json::Value) -> ProviderError {
    ProviderError::with_payload(
        INVALID_EMAIL_CODE,
        INVALID_EMAIL_NAME,
        vec![("email".to_owned(), raw.clone())],
    )
}

/// Rejected email: `validate_email` failure (`base.py:74-82`) carries
/// the SANITIZED value (`email` was reassigned before validation).
fn invalid_email_sanitized(sanitized: &str) -> ProviderError {
    ProviderError::with_payload(
        INVALID_EMAIL_CODE,
        INVALID_EMAIL_NAME,
        vec![(
            "email".to_owned(),
            serde_json::Value::String(sanitized.to_owned()),
        )],
    )
}

/// `sanitize_email` (`base.py:60-84`): falsy → `INVALID_EMAIL` (raw
/// payload); else `str().lower().strip()` + Django `validate_email`
/// → `INVALID_EMAIL` (sanitized payload).
fn sanitize_email(raw: &serde_json::Value) -> Result<String, ProviderError> {
    if !pidash_services::auth_oauth::providers::json_truthy(raw) {
        return Err(invalid_email_raw(raw));
    }
    let sanitized = match raw {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Bool(true) => "True".to_owned(),
        serde_json::Value::Bool(false) => "False".to_owned(),
        // `str()` of the remaining JSON shapes never occurs on this
        // flow (only null/bool/number/string are truthy-capable
        // here); compact JSON is the closest rendering.
        other => other.to_string(),
    };
    let sanitized = sanitized.to_lowercase();
    let sanitized = sanitized.trim().to_owned();
    if !validate_email_address(&sanitized) {
        return Err(invalid_email_sanitized(&sanitized));
    }
    Ok(sanitized)
}

/// Django `EmailValidator` (`django.core.validators`, Django 6.0):
/// 320-char cap, `@` split from the right, dot-atom/quoted-string
/// user part, hostname + TLD domain part (no trailing dot), IP
/// literal, `localhost` allowlist. Hand-rolled: the domain patterns
/// use lookarounds (`(?!-)`, `(?<!-)`) the `regex` crate cannot
/// express.
fn validate_email_address(value: &str) -> bool {
    // `len(value) > 320` counts characters, not bytes (Semantic trap).
    if value.is_empty() || !value.contains('@') || value.chars().count() > 320 {
        return false;
    }
    let (user_part, domain_part) = match value.rsplit_once('@') {
        Some(pair) => pair,
        None => return false,
    };
    if !valid_email_user_part(user_part) {
        return false;
    }
    if domain_part == "localhost" {
        return true;
    }
    valid_email_domain_part(domain_part)
}

/// `user_regex`, `re.IGNORECASE`: dot-atom or quoted-string, full
/// match (`\Z`-anchored).
fn valid_email_user_part(user: &str) -> bool {
    if user.is_empty() {
        return false;
    }
    if user.starts_with('"') && user.ends_with('"') && user.len() >= 2 {
        return valid_quoted_string(&user[1..user.len() - 1]);
    }
    valid_dot_atom(user)
}

fn is_atom_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || "-!#$%&'*+/=?^_`{}|~".contains(c)
}

/// dot-atom: `atom(.atom)*`, ASCII case-insensitive.
fn valid_dot_atom(user: &str) -> bool {
    if user.is_empty() {
        return false;
    }
    for atom in user.split('.') {
        if atom.is_empty() || !atom.chars().all(is_atom_char) {
            return false;
        }
    }
    true
}

/// quoted-string inner: `([\001-\010\013\014\016-\037!#-\[\]-\177] |
/// \\[\001-\011\013\014\016-\177])*`.
fn valid_quoted_char(c: char) -> bool {
    ('\u{1}'..='\u{8}').contains(&c)
        || c == '\u{b}'
        || c == '\u{c}'
        || ('\u{e}'..='\u{1f}').contains(&c)
        || ('!'..='[').contains(&c)
        || (']'..='\u{7f}').contains(&c)
}

fn valid_quoted_escape(c: char) -> bool {
    ('\u{1}'..='\u{9}').contains(&c)
        || c == '\u{b}'
        || c == '\u{c}'
        || ('\u{e}'..='\u{7f}').contains(&c)
}

fn valid_quoted_string(inner: &str) -> bool {
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some(escaped) if valid_quoted_escape(escaped) => continue,
                _ => return false,
            }
        } else if valid_quoted_char(c) {
            continue;
        } else {
            return false;
        }
    }
    true
}

/// Unicode-letter range in the hostname patterns
/// (`ul = "\u00a1-\uffff"`).
fn is_hostname_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || ('\u{a1}'..='\u{10ffff}').contains(&c)
}

/// One domain label, 1-63 chars, no leading/trailing hyphen
/// (`hostname_re` for the first label; `domain_re` interiors share
/// the shape).
fn valid_domain_label(label: &str) -> bool {
    let count = label.chars().count();
    if count == 0 || count > 63 {
        return false;
    }
    let mut chars = label.chars();
    let first = chars.next().unwrap_or('\0');
    let last = label.chars().next_back().unwrap_or('\0');
    if !is_hostname_char(first) || !is_hostname_char(last) {
        return false;
    }
    if first == '-' || last == '-' {
        return false;
    }
    label.chars().all(|c| is_hostname_char(c) || c == '-')
}

/// TLD label: `[a-z\ul-]{2,63}` (no digits) or `xn--[a-z0-9]{1,59}`,
/// no leading/trailing hyphen (`tld_no_fqdn_re`, IGNORECASE — the
/// `xn--` test lowercases first).
fn valid_tld_label(label: &str) -> bool {
    let lower = label.to_lowercase();
    if let Some(rest) = lower.strip_prefix("xn--") {
        let count = rest.chars().count();
        return (1..=59).contains(&count) && rest.chars().all(|c| c.is_ascii_alphanumeric());
    }
    let count = label.chars().count();
    if !(2..=63).contains(&count) {
        return false;
    }
    let first = label.chars().next().unwrap_or('\0');
    let last = label.chars().next_back().unwrap_or('\0');
    if first == '-' || last == '-' {
        return false;
    }
    label
        .chars()
        .all(|c| c.is_ascii_alphabetic() || ('\u{a1}'..='\u{10ffff}').contains(&c) || c == '-')
}

/// `domain_regex` + IP literal (`validate_domain_part`).
fn valid_email_domain_part(domain: &str) -> bool {
    if domain.is_empty() {
        return false;
    }
    if let Some(inner) = domain
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
    {
        return valid_ipv46_literal(inner);
    }
    let labels: Vec<&str> = domain.split('.').collect();
    if labels.len() < 2 {
        return false;
    }
    let (tld, host) = labels.split_last().expect("at least two labels");
    if !valid_tld_label(tld) {
        // Case-insensitivity: the patterns compile with IGNORECASE;
        // the char checks above are case-agnostic except ASCII
        // alpha, which accepts both cases already.
        return false;
    }
    if host.is_empty() {
        return false;
    }
    if !valid_domain_label(host[0]) {
        return false;
    }
    host[1..].iter().all(|label| valid_domain_label(label))
}

/// `validate_ipv46_address` for `[...]` literals: dotted quad (each
/// octet `25[0-5]|2[0-4]\d|[0-1]?\d?\d`, leading zeros accepted) or
/// an IPv6 address.
fn valid_ipv46_literal(literal: &str) -> bool {
    if valid_ipv4_literal(literal) {
        return true;
    }
    literal.parse::<std::net::Ipv6Addr>().is_ok()
}

fn valid_ipv4_octet(octet: &str) -> bool {
    if octet.is_empty() || octet.len() > 3 || !octet.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    match octet.parse::<u32>() {
        Ok(v) => v <= 255,
        Err(_) => false,
    }
}

fn valid_ipv4_literal(literal: &str) -> bool {
    let parts: Vec<&str> = literal.split('.').collect();
    parts.len() == 4 && parts.iter().all(|part| valid_ipv4_octet(part))
}

// ---------------------------------------------------------------------------
// User pipeline (`Adapter.complete_login_or_signup`, `base.py:289-360`)
// ---------------------------------------------------------------------------

/// The user row the pipeline reads (`User.objects.filter(email)`
/// + the login/save/sync touches).
type UserRow = (
    uuid::Uuid,
    Option<String>,
    String,
    bool,
    bool,
    bool,
    String,
    String,
);

/// The user columns the pipeline reads (`User.objects.filter(email)`
/// + the login/save/sync touches).
struct DbUser {
    id: uuid::Uuid,
    email: String,
    password: String,
    is_active: bool,
    is_superuser: bool,
    is_staff: bool,
    display_name: String,
    avatar: String,
}

/// `User.objects.filter(email=email).first()` (`base.py:295-297`):
/// exact match, newest first (`Meta.ordering = ("-created_at",)`).
async fn find_user_by_email(pool: &PgPool, email: &str) -> Result<Option<DbUser>, Denial> {
    let row: Option<UserRow> = sqlx::query_as(
        r#"SELECT id, email, password, is_active, is_superuser, is_staff,
                  display_name, avatar
           FROM "users" WHERE email = $1 ORDER BY "created_at" DESC LIMIT 1"#,
    )
    .bind(email)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(row.map(
        |(id, email, password, is_active, is_superuser, is_staff, display_name, avatar)| DbUser {
            id,
            email: email.unwrap_or_default(),
            password,
            is_active,
            is_superuser,
            is_staff,
            display_name,
            avatar,
        },
    ))
}

/// `SIGNUP_DISABLED` code + name (`adapter/error.py:28`).
pub const SIGNUP_DISABLED_CODE: i32 = 5015;
/// `SIGNUP_DISABLED` name.
pub const SIGNUP_DISABLED_NAME: &str = "SIGNUP_DISABLED";

/// `__check_signup` (`base.py:103-122`): with signups disabled
/// (`ENABLE_SIGNUP == "0"`, legacy default `"1"`) a missing workspace
/// invite raises `SIGNUP_DISABLED` (payload: the sanitized email). A
/// config-store or DB failure escapes (Django 500), it is not a
/// provider error.
async fn check_signup(pool: &PgPool, secret_key: &str, email: &str) -> Result<(), CallbackOutcome> {
    use pidash_db::config::legacy::{get_configuration_values, LegacyItem};
    use pidash_db::config::value::ConfigValue;
    let store = pidash_db::config::accessor::PgConfigStore::new(pool.clone());
    let registry = pidash_db::config::registry::global();
    let keyring = pidash_services::license::encryption::Keyring::from_secret(secret_key);
    let default = std::env::var("ENABLE_SIGNUP").unwrap_or_else(|_| "1".to_owned());
    let values = get_configuration_values(
        registry,
        &store,
        &keyring,
        &[LegacyItem::new("ENABLE_SIGNUP", ConfigValue::Str(default))],
    )
    .await
    .map_err(|_| CallbackOutcome::Server)?;
    let enabled = values
        .first()
        .and_then(|v| match v {
            ConfigValue::Str(s) => Some(s.clone()),
            ConfigValue::Int(v) => Some(v.to_string()),
            ConfigValue::Bool(true) => Some("True".to_owned()),
            ConfigValue::Bool(false) => Some("False".to_owned()),
            _ => None,
        })
        .unwrap_or_else(|| "1".to_owned());
    if enabled != "0" {
        return Ok(());
    }
    let invited: Option<(i32,)> =
        sqlx::query_as(r#"SELECT 1 FROM "workspace_member_invites" WHERE email = $1 LIMIT 1"#)
            .bind(email)
            .fetch_optional(pool)
            .await
            .map_err(|_| CallbackOutcome::Server)?;
    if invited.is_none() {
        return Err(CallbackOutcome::Provider(ProviderError::with_payload(
            SIGNUP_DISABLED_CODE,
            SIGNUP_DISABLED_NAME,
            vec![(
                "email".to_owned(),
                serde_json::Value::String(email.to_owned()),
            )],
        )));
    }
    Ok(())
}

/// `User.get_display_name` (`user.py:192-199`): local part when the
/// address splits in two, else six random ASCII letters.
fn get_display_name(email: &str) -> String {
    if email.is_empty() {
        return random_letters(6);
    }
    let parts: Vec<&str> = email.split('@').collect();
    if parts.len() == 2 {
        parts[0].to_owned()
    } else {
        random_letters(6)
    }
}

/// Six random ASCII letters (`random.choice(string.ascii_letters)`;
/// the generator is unobservable, UUID-derived bytes feed it).
fn random_letters(count: usize) -> String {
    const LETTERS: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";
    let bytes = uuid::Uuid::new_v4().into_bytes();
    bytes
        .iter()
        .cycle()
        .take(count)
        .map(|b| LETTERS[(usize::from(*b)) % LETTERS.len()] as char)
        .collect()
}

/// Twelve alphanumeric salt chars (`get_random_string()` default
/// length/alphabet subset; unobservable, UUID-hex-derived).
fn random_salt() -> String {
    uuid::Uuid::new_v4().simple().to_string()[..12].to_owned()
}

/// `"#" + random hexdigits` (`utils/color.py:get_random_color`).
fn random_color() -> String {
    format!("#{}", &uuid::Uuid::new_v4().simple().to_string()[..6])
}

/// New-user row (`base.py:299-328` + `User.save()`, `user.py:167-187`):
/// identity + names + password hash; `display_name` is backfilled
/// from the local part BEFORE the insert (the `else` arm is dead —
/// `len(split)` is never 0); `is_active` defaults `True`;
/// `last_login_medium` keeps the `"email"` default until
/// `save_user_data` overwrites it.
#[allow(clippy::too_many_arguments)]
async fn insert_user(
    pool: &PgPool,
    id: uuid::Uuid,
    email: &str,
    username: &str,
    password_hash: &str,
    first_name: &str,
    last_name: &str,
    now: chrono::DateTime<Utc>,
) -> Result<(), Denial> {
    let display_name = email.split('@').next().unwrap_or_default();
    sqlx::query(
        r#"INSERT INTO "users"
           ("id", "password", "last_login", "username", "mobile_number", "email",
            "display_name", "first_name", "last_name", "avatar", "avatar_asset_id",
            "cover_image", "cover_image_asset_id", "date_joined", "created_at", "updated_at",
            "last_location", "created_location", "is_superuser", "is_managed",
            "is_password_expired", "is_active", "is_staff", "is_email_verified",
            "is_password_autoset", "is_password_reset_required", "token", "last_active",
            "last_login_time", "last_logout_time", "last_login_ip", "last_logout_ip",
            "last_login_medium", "last_login_uagent", "token_updated_at", "is_bot",
            "bot_type", "user_timezone", "is_email_valid", "masked_at")
           VALUES ($1,$2,NULL,$3,NULL,$4,$5,$6,$7,'',NULL,
                   NULL,NULL,$8,$8,$8,
                   '','',FALSE,FALSE,
                   FALSE,TRUE,FALSE,TRUE,
                   TRUE,FALSE,'',$8,
                   NULL,NULL,'','',
                   'email','',NULL,FALSE,
                   NULL,'UTC',FALSE,NULL)"#,
    )
    .bind(id)
    .bind(password_hash)
    .bind(username)
    .bind(email)
    .bind(display_name)
    .bind(first_name)
    .bind(last_name)
    .bind(now)
    .execute(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(())
}

/// `Profile.objects.create(user=user)` (`base.py:342`): every column
/// with its Python-side default.
async fn insert_profile(
    pool: &PgPool,
    id: uuid::Uuid,
    user_id: uuid::Uuid,
    now: chrono::DateTime<Utc>,
) -> Result<(), Denial> {
    sqlx::query(
        r#"INSERT INTO "profiles"
           ("id", "user_id", "theme", "is_app_rail_docked", "is_tour_completed",
            "onboarding_step", "use_case", "role", "is_onboarded", "last_workspace_id",
            "billing_address_country", "billing_address", "has_billing_address",
            "company_name", "notification_view_mode", "is_smooth_cursor_enabled",
            "is_mobile_onboarded", "mobile_onboarding_step", "mobile_timezone_auto_set",
            "language", "start_of_the_week", "goals", "background_color",
            "is_navigation_tour_completed", "has_marketing_email_consent",
            "is_subscribed_to_changelog", "product_tour", "settings",
            "created_at", "updated_at")
           VALUES ($1,$2,'{}',TRUE,FALSE,
                   $3,NULL,NULL,FALSE,NULL,
                   'INDIA',NULL,FALSE,
                   '','full',FALSE,
                   FALSE,$4,FALSE,
                   'en',0,'{}',$5,
                   FALSE,FALSE,
                   FALSE,$6,'{}',
                   $7,$7)"#,
    )
    .bind(id)
    .bind(user_id)
    .bind(serde_json::json!({
        "profile_complete": false,
        "workspace_create": false,
        "workspace_invite": false,
        "workspace_join": false,
    }))
    .bind(serde_json::json!({
        "profile_complete": false,
        "workspace_create": false,
        "workspace_join": false,
    }))
    .bind(random_color())
    .bind(serde_json::json!({
        "work_items": false,
        "cycles": false,
        "modules": false,
        "intake": false,
        "pages": false,
    }))
    .bind(now)
    .execute(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(())
}

/// `create_user_notification` post_save receiver
/// (`user.py:306-317`): fires on user create for non-bots.
async fn insert_notification_preference(
    pool: &PgPool,
    id: uuid::Uuid,
    user_id: uuid::Uuid,
    now: chrono::DateTime<Utc>,
) -> Result<(), Denial> {
    sqlx::query(
        r#"INSERT INTO "user_notification_preferences"
           ("id", "created_at", "updated_at", "created_by_id", "updated_by_id", "deleted_at",
            "user_id", "workspace_id", "project_id",
            "property_change", "state_change", "comment", "mention", "issue_completed")
           VALUES ($1,$2,$2,NULL,NULL,NULL,$3,NULL,NULL,TRUE,TRUE,TRUE,TRUE,TRUE)"#,
    )
    .bind(id)
    .bind(now)
    .bind(user_id)
    .execute(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(())
}

/// Avatar content types (`base.py:170-176`).
fn avatar_extension(content_type: &str) -> Option<&'static str> {
    match content_type {
        "image/jpeg" | "image/jpg" => Some("jpg"),
        "image/png" => Some("png"),
        "image/gif" => Some("gif"),
        "image/webp" => Some("webp"),
        _ => None,
    }
}

/// `download_and_upload_avatar` (`base.py:145-217`): downloads (10s
/// timeout), enforces the content-length / content-type / total-size
/// gates, then uploads to storage. The SigV4 PUT + `FileAsset` create
/// need the asset domain's S3 transport (follow-up issue); until it
/// lands the port keeps Python's own upload-failure behaviour — the
/// avatar URL fallback with no asset row.
async fn download_avatar_bytes(avatar_url: &str, max_size: u64) -> Option<(Vec<u8>, String)> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .ok()?;
    let response = client.get(avatar_url).send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    if let Some(length) = response
        .headers()
        .get(reqwest::header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
    {
        if length > max_size {
            return None;
        }
    }
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("image/jpeg")
        .split(';')
        .next()
        .unwrap_or("image/jpeg")
        .trim()
        .to_owned();
    avatar_extension(&content_type)?;
    let bytes = response.bytes().await.ok()?.to_vec();
    if bytes.len() as u64 > max_size {
        return None;
    }
    Some((bytes, content_type))
}

/// `get_default_props` (`workspace.py:22-50`): the JSON default for
/// member `view_props` / `default_props`.
fn default_props_json() -> serde_json::Value {
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

/// `get_issue_props` (`workspace.py:110-111`): member `issue_props`.
fn issue_props_json() -> serde_json::Value {
    serde_json::json!({
        "subscribed": true, "assigned": true, "created": true, "all_issues": true,
    })
}

/// `get_default_preferences` (`project.py:68-69`): project-member
/// `preferences`.
fn default_preferences_json() -> serde_json::Value {
    serde_json::json!({
        "pages": {"block_display": true},
        "navigation": {"default_tab": "work_items", "hide_in_more_menu": []},
    })
}

/// `check_sync_enabled` (`base.py:126-143`): `ENABLE_GITLAB_SYNC ==
/// "1"` (legacy default `"0"`).
async fn gitlab_sync_enabled(pool: &PgPool, secret_key: &str) -> Result<bool, Denial> {
    use pidash_db::config::legacy::{get_configuration_values, LegacyItem};
    use pidash_db::config::value::ConfigValue;
    let store = pidash_db::config::accessor::PgConfigStore::new(pool.clone());
    let registry = pidash_db::config::registry::global();
    let keyring = pidash_services::license::encryption::Keyring::from_secret(secret_key);
    let default = std::env::var("ENABLE_GITLAB_SYNC").unwrap_or_else(|_| "0".to_owned());
    let values = get_configuration_values(
        registry,
        &store,
        &keyring,
        &[LegacyItem::new(
            "ENABLE_GITLAB_SYNC",
            ConfigValue::Str(default),
        )],
    )
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(values.first().is_some_and(|v| match v {
        ConfigValue::Str(s) => s == "1",
        ConfigValue::Int(v) => *v == 1,
        ConfigValue::Bool(true) => false,
        _ => false,
    }))
}

/// `sync_user_data` (`base.py:256-287`, existing user + sync
/// enabled): names, `display_name` (stored or derived), avatar URL
/// fallback (S3 upload is the follow-up gap — old-asset delete is
/// skipped with it), then a full-row save with its side effects.
async fn sync_user_data(
    pool: &PgPool,
    user: &DbUser,
    first_name: &str,
    last_name: &str,
    display_name: &str,
    avatar: &str,
    now: chrono::DateTime<Utc>,
) -> Result<(), Denial> {
    let is_staff = user.is_staff || user.is_superuser;
    sqlx::query(
        r#"UPDATE "users" SET "first_name" = $1, "last_name" = $2, "display_name" = $3,
                  "avatar" = $4, "is_staff" = $5, "updated_at" = $6 WHERE "id" = $7"#,
    )
    .bind(first_name)
    .bind(last_name)
    .bind(display_name)
    .bind(avatar)
    .bind(is_staff)
    .bind(now)
    .bind(user.id)
    .execute(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(())
}

/// `save_user_data` (`base.py:220-234`): login stamps + activation
/// branch + `is_active = True`, then the save with token rotation
/// (`token_updated_at` is always set here, so the token always
/// rotates), display backfill for blank names, and
/// `is_staff`-from-`is_superuser`.
#[allow(clippy::too_many_arguments)]
async fn save_user_data(
    pool: &PgPool,
    user: &DbUser,
    avatar: &str,
    user_agent: Option<&str>,
    ip: Option<&str>,
    site: &str,
    now: chrono::DateTime<Utc>,
) -> Result<(), Denial> {
    // QUIRK-activation-order (`tasks.rs`): the mail is enqueued BEFORE
    // `is_active = True` is saved, so a crash between the two
    // re-sends on the next login. The call order is kept.
    if !user.is_active {
        enqueue_activation_mail(pool, site, &user.id.to_string()).await?;
    }
    let token = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    let display_name = if user.display_name.is_empty() {
        user.email.split('@').next().unwrap_or_default().to_owned()
    } else {
        user.display_name.clone()
    };
    let is_staff = user.is_staff || user.is_superuser;
    sqlx::query(
        r#"UPDATE "users" SET "last_login_medium" = 'gitlab', "last_active" = $1,
                  "last_login_time" = $1, "last_login_ip" = $2, "last_login_uagent" = $3,
                  "token_updated_at" = $1, "token" = $4, "is_active" = TRUE,
                  "avatar" = $5, "display_name" = $6, "is_staff" = $7, "updated_at" = $1
           WHERE "id" = $8"#,
    )
    .bind(now)
    .bind(ip)
    .bind(user_agent)
    .bind(token)
    .bind(avatar)
    .bind(display_name)
    .bind(is_staff)
    .bind(user.id)
    .execute(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(())
}

/// `user_activation_email.delay(current_site, user_id)`
/// (`base.py:230`): positional args, empty kwargs; a publish failure
/// propagates (Django `.delay` raises), it does not warn-and-stand.
async fn enqueue_activation_mail(
    pool: &PgPool,
    current_site: &str,
    user_id: &str,
) -> Result<(), Denial> {
    let message = pidash_jobs::celery::CeleryTaskMessage::new(
        pidash_services::auth_session::tasks::USER_ACTIVATION_EMAIL_TASK,
        vec![
            serde_json::Value::String(current_site.to_owned()),
            serde_json::Value::String(user_id.to_owned()),
        ],
        Default::default(),
    );
    let job = pidash_jobs::queue::NewJob::new(
        message.task.clone(),
        serde_json::Value::Array(message.args.clone()),
        serde_json::Value::Object(message.kwargs.clone()),
    );
    pidash_jobs::queue::enqueue(pool, &job)
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(())
}

/// `track_event.delay(user_id=..., event_name=...,
/// slug=..., event_properties=...)`
/// (`workspace_project_join.py:45-56`): keyword args (kombu renders
/// the UUIDs as strings).
async fn enqueue_track_event(
    pool: &PgPool,
    user_id: &str,
    slug: &str,
    workspace_id: &str,
    role: i16,
    joined_at: &str,
) -> Result<(), Denial> {
    let mut kwargs = serde_json::Map::new();
    kwargs.insert(
        "user_id".to_owned(),
        serde_json::Value::String(user_id.to_owned()),
    );
    kwargs.insert(
        "event_name".to_owned(),
        serde_json::Value::String("user_joined_workspace".to_owned()),
    );
    kwargs.insert(
        "slug".to_owned(),
        serde_json::Value::String(slug.to_owned()),
    );
    let mut props = serde_json::Map::new();
    props.insert(
        "user_id".to_owned(),
        serde_json::Value::String(user_id.to_owned()),
    );
    props.insert(
        "workspace_id".to_owned(),
        serde_json::Value::String(workspace_id.to_owned()),
    );
    props.insert(
        "workspace_slug".to_owned(),
        serde_json::Value::String(slug.to_owned()),
    );
    props.insert("role".to_owned(), serde_json::Value::from(role));
    props.insert(
        "joined_at".to_owned(),
        serde_json::Value::String(joined_at.to_owned()),
    );
    kwargs.insert(
        "event_properties".to_owned(),
        serde_json::Value::Object(props),
    );
    let message = pidash_jobs::celery::CeleryTaskMessage::new(
        "pi_dash.bgtasks.event_tracking_task.track_event",
        Vec::new(),
        kwargs,
    );
    let job = pidash_jobs::queue::NewJob::new(
        message.task.clone(),
        serde_json::Value::Array(message.args.clone()),
        serde_json::Value::Object(message.kwargs.clone()),
    );
    pidash_jobs::queue::enqueue(pool, &job)
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(())
}

/// Accepted project invites (`workspace_project_join.py:59`).
struct ProjectInvite {
    workspace_id: uuid::Uuid,
    role: i16,
    created_by_id: Option<uuid::Uuid>,
}

async fn accepted_project_invites(
    pool: &PgPool,
    email: &str,
) -> Result<Vec<ProjectInvite>, Denial> {
    let rows: Vec<(uuid::Uuid, i16, Option<uuid::Uuid>)> = sqlx::query_as(
        r#"SELECT "workspace_id", "role", "created_by_id"
           FROM "project_member_invites"
           WHERE "email" = $1 AND "accepted" = TRUE AND "deleted_at" IS NULL"#,
    )
    .bind(email)
    .fetch_all(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(rows
        .into_iter()
        .map(|(workspace_id, role, created_by_id)| ProjectInvite {
            workspace_id,
            role,
            created_by_id,
        })
        .collect())
}

/// `process_workspace_project_invitations`
/// (`workspace_project_join.py:20-91`): workspace joins + per-invite
/// events, project-arm joins (B5), then both invite deletes.
///
/// The per-invite cache invalidation (`invalidate_cache_directly`,
/// `:38-44`) needs a key-delete API `RedisHandle` does not expose —
/// recorded in the invite-join spec for the owning issue; the
/// `track_event` emits below are real publishes.
async fn process_invitations(
    pool: &PgPool,
    email: &str,
    user_id: uuid::Uuid,
    now: chrono::DateTime<Utc>,
) -> Result<(), Denial> {
    let user_id_str = user_id.to_string();
    let joined_at = now.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, false);

    let ws_invites = accepted_workspace_invites(pool, email).await?;
    bulk_insert_workspace_members(
        pool,
        &ws_invites
            .iter()
            .map(|invite| MemberRow {
                workspace_id: invite.workspace_id,
                member_id: user_id,
                role: invite.role,
                created_by_id: None,
            })
            .collect::<Vec<_>>(),
        now,
    )
    .await?;
    for invite in &ws_invites {
        enqueue_track_event(
            pool,
            &user_id_str,
            &invite.slug,
            &invite.workspace_id.to_string(),
            invite.role,
            &joined_at,
        )
        .await?;
    }

    let proj_invites = accepted_project_invites(pool, email).await?;
    bulk_insert_workspace_members(
        pool,
        &proj_invites
            .iter()
            .map(|invite| MemberRow {
                workspace_id: invite.workspace_id,
                member_id: user_id,
                role: pidash_services::auth_session::queries::map_invite_role(i32::from(
                    invite.role,
                )) as i16,
                created_by_id: invite.created_by_id,
            })
            .collect::<Vec<_>>(),
        now,
    )
    .await?;
    // B5: `project_id` is missing (non-nullable) — this raises
    // `IntegrityError` whenever project invites exist, and the
    // callback answers 500.
    bulk_insert_project_members(
        pool,
        &proj_invites
            .iter()
            .map(|invite| MemberRow {
                workspace_id: invite.workspace_id,
                member_id: user_id,
                role: pidash_services::auth_session::queries::map_invite_role(i32::from(
                    invite.role,
                )) as i16,
                created_by_id: invite.created_by_id,
            })
            .collect::<Vec<_>>(),
        now,
    )
    .await?;

    // Soft-delete (`SoftDeletionQuerySet.delete()`, `db/mixins.py:48-53`):
    // stamp `deleted_at` instead of removing rows.
    sqlx::query(
        r#"UPDATE "workspace_member_invites" SET "deleted_at" = $1
           WHERE "email" = $2 AND "accepted" = TRUE AND "deleted_at" IS NULL"#,
    )
    .bind(now)
    .bind(email)
    .execute(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    sqlx::query(
        r#"UPDATE "project_member_invites" SET "deleted_at" = $1
           WHERE "email" = $2 AND "accepted" = TRUE AND "deleted_at" IS NULL"#,
    )
    .bind(now)
    .bind(email)
    .execute(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(())
}

/// Accepted workspace invites with their workspace slugs
/// (`workspace_project_join.py:24`).
struct WorkspaceInvite {
    workspace_id: uuid::Uuid,
    role: i16,
    slug: String,
}

async fn accepted_workspace_invites(
    pool: &PgPool,
    email: &str,
) -> Result<Vec<WorkspaceInvite>, Denial> {
    let rows: Vec<(uuid::Uuid, i16, String)> = sqlx::query_as(
        r#"SELECT i."workspace_id", i."role", w."slug"
           FROM "workspace_member_invites" i
           JOIN "workspaces" w ON w."id" = i."workspace_id"
           WHERE i."email" = $1 AND i."accepted" = TRUE
             AND i."deleted_at" IS NULL AND w."deleted_at" IS NULL"#,
    )
    .bind(email)
    .fetch_all(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(rows
        .into_iter()
        .map(|(workspace_id, role, slug)| WorkspaceInvite {
            workspace_id,
            role,
            slug,
        })
        .collect())
}

/// One workspace-membership row for the bulk creates below.
struct MemberRow {
    workspace_id: uuid::Uuid,
    member_id: uuid::Uuid,
    role: i16,
    created_by_id: Option<uuid::Uuid>,
}

/// `WorkspaceMember.objects.bulk_create(..., ignore_conflicts=True)`
/// (`workspace_project_join.py:26-36,62-73`) with Django's full column
/// set (every column the ORM sends; `bulk_create` skips `save()`, so
/// `updated_by`/`deleted_at` stay `NULL` and `created_by` is only set
/// where the constructor passes it). One multi-row `INSERT ... ON
/// CONFLICT DO NOTHING`; empty input sends no statement
/// (`bulk_create([])` is a no-op).
async fn bulk_insert_workspace_members(
    pool: &PgPool,
    rows: &[MemberRow],
    now: chrono::DateTime<Utc>,
) -> Result<(), Denial> {
    if rows.is_empty() {
        return Ok(());
    }
    let mut builder: sqlx::QueryBuilder<sqlx::Postgres> = sqlx::QueryBuilder::new(
        r#"INSERT INTO "workspace_members" ("id", "created_at", "updated_at",
           "created_by_id", "updated_by_id", "deleted_at",
           "workspace_id", "member_id", "role", "company_role",
           "view_props", "default_props", "issue_props", "is_active",
           "getting_started_checklist", "tips", "explored_features") "#,
    );
    let view_props = default_props_json();
    let issue_props = issue_props_json();
    builder.push_values(rows, |mut row, member| {
        row.push_bind(uuid::Uuid::new_v4())
            .push_bind(now)
            .push_bind(now)
            .push_bind(member.created_by_id)
            .push_bind(Option::<uuid::Uuid>::None)
            .push_bind(Option::<chrono::DateTime<Utc>>::None)
            .push_bind(member.workspace_id)
            .push_bind(member.member_id)
            .push_bind(member.role)
            .push_bind(Option::<String>::None)
            .push_bind(view_props.clone())
            .push_bind(view_props.clone())
            .push_bind(issue_props.clone())
            .push_bind(true)
            .push_bind(serde_json::json!({}))
            .push_bind(serde_json::json!({}))
            .push_bind(serde_json::json!({}));
    });
    builder.push(" ON CONFLICT DO NOTHING");
    builder
        .build()
        .execute(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(())
}

/// `ProjectMember.objects.bulk_create(..., ignore_conflicts=True)`
/// (`workspace_project_join.py:76-87`): the same full column set
/// MINUS `project_id` (B5 — the constructor never sets `project`, so
/// the non-nullable column raises `IntegrityError` on Postgres
/// despite `ignore_conflicts=True`, and the callback answers 500).
async fn bulk_insert_project_members(
    pool: &PgPool,
    rows: &[MemberRow],
    now: chrono::DateTime<Utc>,
) -> Result<(), Denial> {
    if rows.is_empty() {
        return Ok(());
    }
    let mut builder: sqlx::QueryBuilder<sqlx::Postgres> = sqlx::QueryBuilder::new(
        r#"INSERT INTO "project_members" ("id", "created_at", "updated_at",
           "created_by_id", "updated_by_id", "deleted_at",
           "workspace_id", "member_id", "comment", "role",
           "view_props", "default_props", "preferences", "sort_order", "is_active") "#,
    );
    let view_props = default_props_json();
    builder.push_values(rows, |mut row, member| {
        row.push_bind(uuid::Uuid::new_v4())
            .push_bind(now)
            .push_bind(now)
            .push_bind(member.created_by_id)
            .push_bind(Option::<uuid::Uuid>::None)
            .push_bind(Option::<chrono::DateTime<Utc>>::None)
            .push_bind(member.workspace_id)
            .push_bind(member.member_id)
            .push_bind(Option::<String>::None)
            .push_bind(member.role)
            .push_bind(view_props.clone())
            .push_bind(view_props.clone())
            .push_bind(default_preferences_json())
            .push_bind(65535.0_f64)
            .push_bind(true);
    });
    builder.push(" ON CONFLICT DO NOTHING");
    builder
        .build()
        .execute(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Login + redirection
// ---------------------------------------------------------------------------

/// `user.get_session_auth_hash()`: the make side of
/// `verify_session_hash` (`license/mod.rs`): HMAC-SHA256 over the
/// password field, keyed by SHA256(salt + secret), hex-encoded.
fn make_session_auth_hash(password_field: &str, secret_key: &[u8]) -> String {
    use hmac::{Hmac, Mac};
    use sha2::{Digest, Sha256};
    let key = Sha256::digest(
        [
            crate::license::SESSION_AUTH_HASH_SALT.as_bytes(),
            secret_key,
        ]
        .concat(),
    );
    let mut mac = Hmac::<Sha256>::new_from_slice(&key).expect("HMAC-SHA256 accepts any key length");
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

/// `user_login` (`authentication/utils/login.py:13-28`) with
/// `is_app=True`: Django `login()` keys plus the `device_info` dict.
/// (Django also cycles the session key; the session layer exposes no
/// cycle operation, so the keys land on the current session — the
/// cookie behaviour is unchanged: a mutated session is persisted.)
fn user_login(
    handle: &SessionHandle,
    user: &DbUser,
    user_agent: Option<&str>,
    ip: Option<&str>,
    domain: &str,
    secret_key: &[u8],
) {
    let mut session = handle.lock();
    session.set(
        "_auth_user_id".to_owned(),
        serde_json::Value::String(user.id.to_string()),
    );
    session.set(
        "_auth_user_hash".to_owned(),
        serde_json::Value::String(make_session_auth_hash(&user.password, secret_key)),
    );
    session.set(
        "_auth_user_backend".to_owned(),
        serde_json::Value::String(crate::license::MODEL_BACKEND.to_owned()),
    );
    session.set(
        "device_info".to_owned(),
        crate::license::handlers_auth_forms::device_info(user_agent.unwrap_or(""), ip, domain),
    );
}

/// `get_redirection_path` (`redirection_path.py:8-46`): onboarding,
/// last workspace, earliest active membership, invites, else
/// create-workspace. The branch order lives in
/// `select_redirection_path`; the reads below feed it.
async fn redirection_path(
    pool: &PgPool,
    user_id: uuid::Uuid,
    email: &str,
    now: chrono::DateTime<Utc>,
) -> Result<String, Denial> {
    // `Profile.objects.get_or_create(user=user)`.
    let profile: Option<(bool, Option<uuid::Uuid>)> = sqlx::query_as(
        r#"SELECT "is_onboarded", "last_workspace_id" FROM "profiles" WHERE "user_id" = $1"#,
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let (is_onboarded, last_workspace_id) = match profile {
        Some(found) => found,
        None => {
            insert_profile(pool, uuid::Uuid::new_v4(), user_id, now).await?;
            (false, None)
        }
    };
    if !is_onboarded {
        return Ok("onboarding".to_owned());
    }
    if let Some(last_id) = last_workspace_id {
        let slug: Option<(String,)> = sqlx::query_as(
            r#"SELECT w."slug" FROM "workspaces" w
               JOIN "workspace_members" m ON m."workspace_id" = w."id"
               WHERE w."id" = $1 AND m."member_id" = $2 AND m."is_active" = TRUE
                 AND m."deleted_at" IS NULL AND w."deleted_at" IS NULL"#,
        )
        .bind(last_id)
        .bind(user_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
        if let Some((slug,)) = slug {
            return Ok(slug);
        }
    }
    let fallback: Option<(String,)> = sqlx::query_as(
        r#"SELECT w."slug" FROM "workspaces" w
           JOIN "workspace_members" m ON m."workspace_id" = w."id"
           WHERE m."member_id" = $1 AND m."is_active" = TRUE
             AND m."deleted_at" IS NULL AND w."deleted_at" IS NULL
           ORDER BY w."created_at" ASC LIMIT 1"#,
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    if let Some((slug,)) = fallback {
        return Ok(slug);
    }
    let invites: (i64,) = sqlx::query_as(
        r#"SELECT COUNT(*) FROM "workspace_member_invites"
           WHERE "email" = $1 AND "deleted_at" IS NULL"#,
    )
    .bind(email)
    .fetch_one(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    if invites.0 > 0 {
        return Ok("invitations".to_owned());
    }
    Ok("create-workspace".to_owned())
}

// ---------------------------------------------------------------------------
// Success chain (`provider.authenticate()` .. 302)
// ---------------------------------------------------------------------------

/// Falsy-or-string name field (`first_name if first_name else ""`,
/// `base.py:322-325`).
fn name_or_empty(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        other if pidash_services::auth_oauth::providers::json_truthy(other) => match other {
            serde_json::Value::Number(n) => n.to_string(),
            serde_json::Value::Bool(true) => "True".to_owned(),
            serde_json::Value::Bool(false) => "False".to_owned(),
            _ => String::new(),
        },
        _ => String::new(),
    }
}

/// The callback success chain shared by the app callback and the
/// space callback's valid path (`GitLabOAuthProvider.authenticate()`
/// → `user_login` → redirect path, `views/app/gitlab.py:92-104`):
/// token exchange, user fetch, login-or-signup, account upsert,
/// session login. Returns the redirect `path` (`next_path or
/// get_redirection_path(user)`).
#[allow(clippy::too_many_arguments)]
async fn callback_success(
    state: &AppState,
    headers: &HeaderMap,
    code: String,
    session_next_path: Option<String>,
    handle: Option<SessionHandle>,
    peer: Option<std::net::SocketAddr>,
    is_space: bool,
) -> Result<String, CallbackOutcome> {
    let pool = pool_of(state)?;
    let settings = state.settings();
    let secret = settings.secret_key.clone();
    let now = Utc::now();
    let host = request_host(headers);
    let is_secure = request_is_secure(headers, settings.secure_proxy_ssl_header);

    // `GitLabOAuthProvider(request, code, callback)` construction:
    // unconfigured answers `GITLAB_NOT_CONFIGURED` (caught below).
    let config = gitlab_config(pool, secret.as_str()).await?;
    if !gitlab_configured(&config) {
        return Err(CallbackOutcome::Provider(ProviderError::with_payload(
            NOT_CONFIGURED_CODE,
            NOT_CONFIGURED_NAME,
            Vec::new(),
        )));
    }
    let client_id = config.client_id.clone().unwrap_or_default();
    let client_secret = config.client_secret.clone().unwrap_or_default();
    let gitlab_host = config.host.clone().unwrap_or_default();
    let redirect_uri =
        pidash_services::auth_oauth::providers::redirect_uri(is_secure, &host, PROVIDER);

    // `authenticate()`: `set_token_data()` then `set_user_data()`.
    let form = pidash_services::auth_oauth::providers::authorization_code_post_data(
        &code,
        &client_id,
        &client_secret,
        &redirect_uri,
    );
    let form_refs: Vec<(&str, &str)> = form.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let token_response = fetch_token_json(
        &pidash_services::auth_oauth::providers::gitlab_token_url(&gitlab_host),
        &form_refs,
    )
    .await?;
    // BUG-3 (`providers.rs`): a truthy `expires_in` with a
    // missing/non-numeric `created_at` is a `TypeError` in Python —
    // the generic 500 here.
    let token_data = pidash_services::auth_oauth::providers::gitlab_token_data(&token_response)
        .map_err(|_| CallbackOutcome::Server)?;
    let access_token = text_or_none(&token_data.access_token);
    let userinfo = fetch_userinfo_json(
        &pidash_services::auth_oauth::providers::gitlab_userinfo_url(&gitlab_host),
        access_token.as_deref(),
    )
    .await?;
    let user_data = pidash_services::auth_oauth::providers::gitlab_user_data(&userinfo);

    // `complete_login_or_signup()`: sanitize, lookup, signup-or-login.
    let email = sanitize_email(&user_data.email).map_err(CallbackOutcome::Provider)?;
    let found = find_user_by_email(pool, &email).await?;
    // B4: `is_signup = bool(user)` — inverted as written.
    let _is_signup = found.is_some();

    let user_agent = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let ip = headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|forwarded| {
            if forwarded.is_empty() {
                None
            } else {
                Some(forwarded.split(',').next().unwrap_or("").to_owned())
            }
        })
        .or_else(|| peer.map(|addr| addr.ip().to_string()));
    let max_avatar = settings.file_size_limit.max(0) as u64;

    let user: DbUser = match found {
        None => {
            check_signup(pool, secret.as_str(), &email).await?;
            let id = uuid::Uuid::new_v4();
            let username = uuid::Uuid::new_v4().simple().to_string();
            let raw_password = uuid::Uuid::new_v4().simple().to_string();
            let password_hash = pidash_auth::password::hash_password(
                &raw_password,
                &random_salt(),
                pidash_auth::password::PBKDF2_DEFAULT_ITERATIONS,
            );
            let first_name = name_or_empty(&user_data.user.first_name);
            let last_name = name_or_empty(&user_data.user.last_name);
            insert_user(
                pool,
                id,
                &email,
                &username,
                &password_hash,
                &first_name,
                &last_name,
                now,
            )
            .await?;
            // Avatar: attempt the download for side-effect parity,
            // keep the URL fallback (S3 upload is the follow-up gap).
            let avatar = match &user_data.user.avatar {
                serde_json::Value::String(url) if !url.is_empty() => {
                    let _ = download_avatar_bytes(url, max_avatar).await;
                    url.clone()
                }
                _ => String::new(),
            };
            insert_profile(pool, uuid::Uuid::new_v4(), id, now).await?;
            insert_notification_preference(pool, uuid::Uuid::new_v4(), id, now).await?;
            DbUser {
                id,
                email: email.clone(),
                password: password_hash,
                is_active: true,
                is_superuser: false,
                is_staff: false,
                display_name: email.split('@').next().unwrap_or_default().to_owned(),
                avatar,
            }
        }
        Some(existing) => {
            if gitlab_sync_enabled(pool, secret.as_str()).await? {
                let first_name = name_or_empty(&user_data.user.first_name);
                let last_name = name_or_empty(&user_data.user.last_name);
                let display_name = get_display_name(&email);
                let avatar = match &user_data.user.avatar {
                    serde_json::Value::String(url) if !url.is_empty() => {
                        let _ = download_avatar_bytes(url, max_avatar).await;
                        url.clone()
                    }
                    _ => String::new(),
                };
                sync_user_data(
                    pool,
                    &existing,
                    &first_name,
                    &last_name,
                    &display_name,
                    &avatar,
                    now,
                )
                .await?;
                DbUser {
                    display_name,
                    avatar,
                    ..existing
                }
            } else {
                existing
            }
        }
    };

    // `save_user_data()`: stamps (+ activation mail) and the save.
    let site = pidash_services::auth_session::base_host(
        &pidash_services::auth_session::HostSettings {
            web_url: settings.urls.web_url.as_deref(),
            app_base_url: settings.urls.app_base_url.as_deref(),
            admin_base_url: settings.urls.admin_base_url.as_deref(),
            space_base_url: settings.urls.space_base_url.as_deref(),
            admin_base_path: Some(settings.urls.admin_base_path.as_str()),
            space_base_path: Some(settings.urls.space_base_path.as_str()),
        },
        false,
        false,
        false,
    );
    save_user_data(
        pool,
        &user,
        &user.avatar.clone(),
        user_agent.as_deref(),
        ip.as_deref(),
        &site,
        now,
    )
    .await?;

    // `callback(user, is_signup, request)` → workspace/project joins.
    // The space view constructs the provider without `callback`
    // (`views/space/gitlab.py:88`; `base.py:352-353` guards on
    // `if self.callback`), so invitations run on the app path only.
    if !is_space {
        process_invitations(pool, &email, user.id, now).await?;
    }

    // `create_update_account(user)` (B6: DB errors are swallowed).
    create_update_account(pool, &user.id, &user_data, &token_data, now).await;

    // `user_login(request, user, is_app=True)` on the app path,
    // `user_login(request, user, is_space=True)` on the space path
    // (`utils/login.py:21-25` records the matching base as the
    // `device_info` domain).
    let handle = handle.ok_or(CallbackOutcome::Server)?;
    let domain = if is_space {
        space_base(settings)
    } else {
        app_base(settings)
    };
    user_login(
        &handle,
        &user,
        user_agent.as_deref(),
        ip.as_deref(),
        &domain,
        secret.as_bytes(),
    );

    // `path = next_path or get_redirection_path(user)`.
    if let Some(next) = session_next_path {
        if !next.is_empty() {
            return Ok(next);
        }
    }
    Ok(redirection_path(pool, user.id, &email, now).await?)
}

/// `create_update_account` (`adapter/oauth.py:107-140`, gitlab
/// field shape): filter on user + provider + `provider_account_id`
/// (lookup side uses strict `.get("user").get(...)` — our payload
/// always carries `user`); update the six token columns plus
/// `last_connected_at`, else insert. B6: `DatabaseError` /
/// `IntegrityError` are logged and swallowed — every fallible step
/// below degrades to `ErrorSwallowed`, never to a redirect or 500.
async fn create_update_account(
    pool: &PgPool,
    user_id: &uuid::Uuid,
    user_data: &pidash_services::auth_oauth::providers::ProviderUserData,
    token_data: &pidash_services::auth_oauth::providers::TokenData,
    now: chrono::DateTime<Utc>,
) {
    use pidash_db::auth_oauth::queries::account::{
        create_account, find_account_id, update_account, NewAccount, TokenFields,
    };
    let tokens = TokenFields {
        access_token: text_or_none(&token_data.access_token),
        refresh_token: text_or_none(&token_data.refresh_token),
        access_token_expired_at: token_data.access_token_expired_at,
        refresh_token_expired_at: token_data.refresh_token_expired_at,
        id_token: text_or_none(&token_data.id_token),
    };
    let provider_account_id = text_or_none(&user_data.user.provider_id);
    let Some(provider_account_id) = provider_account_id else {
        // `provider_account_id=None` matches no `CharField` row and
        // violates non-null on insert — swallowed either way (B6).
        let params = NewAccount {
            id: uuid::Uuid::new_v4(),
            user_id: *user_id,
            provider: PROVIDER,
            provider_account_id: "",
            tokens: &tokens,
            now,
            metadata: None,
        };
        let _ = create_account(pool, &params).await;
        return;
    };
    let account_id = find_account_id(pool, *user_id, PROVIDER, &provider_account_id).await;
    match account_id {
        Ok(Some(id)) => {
            let loaded: Result<Option<(chrono::DateTime<Utc>, serde_json::Value)>, sqlx::Error> =
                sqlx::query_as(
                    r#"SELECT "created_at", "metadata" FROM "accounts" WHERE "id" = $1"#,
                )
                .bind(id)
                .fetch_optional(pool)
                .await;
            if let Ok(Some((created_at, metadata))) = loaded {
                let _ = update_account(
                    pool,
                    id,
                    created_at,
                    *user_id,
                    &provider_account_id,
                    PROVIDER,
                    &tokens,
                    now,
                    metadata,
                )
                .await;
            }
        }
        Ok(None) => {
            let params = NewAccount {
                id: uuid::Uuid::new_v4(),
                user_id: *user_id,
                provider: PROVIDER,
                provider_account_id: &provider_account_id,
                tokens: &tokens,
                now,
                metadata: None,
            };
            let _ = create_account(pool, &params).await;
        }
        // B6: the filter itself failing is swallowed too.
        Err(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixtures_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/auth_oauth")
    }

    fn fixture(name: &str) -> serde_json::Value {
        let body = std::fs::read_to_string(fixtures_dir().join(name)).expect("fixture exists");
        serde_json::from_str(&body).expect("fixture parses")
    }

    // -- routes ----------------------------------------------------------

    #[test]
    fn owned_paths_match_django_urls() {
        // `authentication/urls.py:105-120` under the `auth/` prefix.
        assert_eq!(
            OWNED_PATHS,
            [
                "/auth/gitlab/",
                "/auth/gitlab/callback/",
                "/auth/spaces/gitlab/",
                "/auth/spaces/gitlab/callback/",
            ]
        );
        let router = routes();
        // Four GET routes registered; anything else on those paths
        // falls through to the proxy (asserted structurally: the
        // router builds without duplicate-path panics).
        let _ = router;
    }

    // -- codes -----------------------------------------------------------

    #[test]
    fn error_codes_match_adapter() {
        assert_eq!(NOT_CONFIGURED_CODE, 5111);
        assert_eq!(NOT_CONFIGURED_NAME, "GITLAB_NOT_CONFIGURED");
        assert_eq!(PROVIDER_ERROR_CODE, 5121);
        assert_eq!(PROVIDER_ERROR_NAME, "GITLAB_OAUTH_PROVIDER_ERROR");
        assert_eq!(INSTANCE_NOT_CONFIGURED_CODE, 5000);
        assert_eq!(INVALID_EMAIL_CODE, 5005);
        assert_eq!(SIGNUP_DISABLED_CODE, 5015);
        let error = pidash_services::auth_oauth::error::error_code_by_name;
        assert_eq!(error("GITLAB_NOT_CONFIGURED"), Some(5111));
        assert_eq!(error("GITLAB_OAUTH_PROVIDER_ERROR"), Some(5121));
        assert_eq!(
            pidash_services::auth_oauth::error::authentication_error_code(PROVIDER),
            "GITLAB_OAUTH_PROVIDER_ERROR"
        );
        assert_eq!(
            pidash_services::auth_oauth::error::not_configured_name(PROVIDER),
            Some("GITLAB_NOT_CONFIGURED")
        );
    }

    // -- query last-wins ---------------------------------------------------

    #[test]
    fn query_last_reads_like_querydict_get() {
        let pairs = vec![
            ("code".to_owned(), "first".to_owned()),
            ("code".to_owned(), "second".to_owned()),
        ];
        assert_eq!(query_last(&pairs, "code").as_deref(), Some("second"));
        assert_eq!(query_last(&pairs, "state"), None);
    }

    // -- error locations ---------------------------------------------------

    fn app_allowed() -> Vec<String> {
        vec!["app.example.com".to_owned()]
    }

    #[test]
    fn not_configured_location_matches_f10() {
        let allowed_owned = app_allowed();
        let allowed: Vec<&str> = allowed_owned.iter().map(String::as_str).collect();
        // F10 `app_google_github_gitlab.golden_not_configured`: base +
        // `/?`, raw next_path, then urlencoded params in dict order.
        assert_eq!(
            plain_error_location(
                "https://app.example.com",
                Some("/x"),
                NOT_CONFIGURED_CODE,
                NOT_CONFIGURED_NAME,
                &allowed,
            ),
            "https://app.example.com/?next_path=/x&error_code=5111&error_message=GITLAB_NOT_CONFIGURED"
        );
        assert_eq!(
            plain_error_location(
                "https://app.example.com",
                None,
                INSTANCE_NOT_CONFIGURED_CODE,
                INSTANCE_NOT_CONFIGURED_NAME,
                &allowed,
            ),
            "https://app.example.com/?error_code=5000&error_message=INSTANCE_NOT_CONFIGURED"
        );
    }

    #[test]
    fn provider_error_location_matches_f11() {
        let allowed_owned = app_allowed();
        let allowed: Vec<&str> = allowed_owned.iter().map(String::as_str).collect();
        // F11 `app_google_github_gitlab.golden_missing_code`: the
        // session next_path echoes beside the provider error.
        assert_eq!(
            plain_error_location(
                "https://app.example.com",
                Some("/x"),
                PROVIDER_ERROR_CODE,
                PROVIDER_ERROR_NAME,
                &allowed,
            ),
            "https://app.example.com/?next_path=/x&error_code=5121&error_message=GITLAB_OAUTH_PROVIDER_ERROR"
        );
        assert_eq!(
            plain_error_location(
                "https://app.example.com",
                None,
                PROVIDER_ERROR_CODE,
                PROVIDER_ERROR_NAME,
                &allowed,
            ),
            "https://app.example.com/?error_code=5121&error_message=GITLAB_OAUTH_PROVIDER_ERROR"
        );
    }

    #[test]
    fn payload_errors_carry_email_after_base_pairs() {
        let allowed_owned = app_allowed();
        let allowed: Vec<&str> = allowed_owned.iter().map(String::as_str).collect();
        // `INVALID_EMAIL` with a raw `None` payload renders
        // `email=None` after the base pairs (`urlencode` order).
        assert_eq!(
            payload_error_location(
                "https://app.example.com",
                None,
                INVALID_EMAIL_CODE,
                INVALID_EMAIL_NAME,
                &[("email", serde_json::Value::Null)],
                &allowed,
            ),
            "https://app.example.com/?error_code=5005&error_message=INVALID_EMAIL&email=None"
        );
        assert_eq!(
            payload_error_location(
                "https://app.example.com",
                None,
                SIGNUP_DISABLED_CODE,
                SIGNUP_DISABLED_NAME,
                &[(
                    "email",
                    serde_json::Value::String("a@b.com".to_owned())
                )],
                &allowed,
            ),
            "https://app.example.com/?error_code=5015&error_message=SIGNUP_DISABLED&email=a%40b.com"
        );
    }

    #[test]
    fn success_location_drops_bare_paths() {
        let allowed_owned = app_allowed();
        let allowed: Vec<&str> = allowed_owned.iter().map(String::as_str).collect();
        // `get_redirection_path` outputs (`onboarding`, slugs) fail
        // `validate_next_path` downstream (no leading slash) and are
        // dropped — the redirect is the bare base.
        assert_eq!(
            success_location("https://app.example.com", "onboarding", &allowed),
            "https://app.example.com"
        );
        assert_eq!(
            success_location("https://app.example.com", "/dash", &allowed),
            "https://app.example.com/?next_path=/dash"
        );
    }

    // -- sanitize_email ----------------------------------------------------

    #[test]
    fn sanitize_missing_email_carries_raw_payload() {
        for raw in [
            serde_json::Value::Null,
            serde_json::json!(""),
            serde_json::json!(0),
            serde_json::json!(false),
        ] {
            let error = sanitize_email(&raw).expect_err("must reject");
            assert_eq!(error.code, INVALID_EMAIL_CODE);
            assert_eq!(error.message, INVALID_EMAIL_NAME);
            assert_eq!(error.payload, vec![("email".to_owned(), raw)]);
        }
    }

    #[test]
    fn sanitize_rejected_email_carries_sanitized_payload() {
        // `str().lower().strip()` runs before validation: the payload
        // is the sanitized form, not the raw input.
        let error =
            sanitize_email(&serde_json::json!("  NOT-AN-EMAIL  ")).expect_err("must reject");
        assert_eq!(error.code, INVALID_EMAIL_CODE);
        assert_eq!(
            error.payload,
            vec![("email".to_owned(), serde_json::json!("not-an-email"))]
        );
    }

    #[test]
    fn sanitize_accepts_lowered_stripped() {
        assert_eq!(
            sanitize_email(&serde_json::json!("  Ada@Example.COM  ")).expect("valid"),
            "ada@example.com"
        );
    }

    // -- email validator (vectors verified against Django 6.0.5) -----------

    #[test]
    fn email_validator_matches_django() {
        let valid = [
            "a@b.com",
            "A@B.COM",
            "a.b+c@sub.example.co",
            "x@localhost",
            "\"a\\\"b\"@c.com",
            "a@[1.2.3.4]",
            "a@[::1]",
            "x@[127.0.0.1]",
            "a@xn--nxasmq6b.example",
            "a@XN--NXASMQ6B.example",
            "test@münchen.de",
            "x@y.zz",
        ];
        for address in valid {
            assert!(validate_email_address(address), "valid: {address}");
        }
        let invalid = [
            "LOCALHOST@LOCALHOST",
            "a@b",
            "a@b.c",
            "a@-b.com",
            "a@b-.com",
            "a@b..com",
            ".a@b.com",
            "a.@b.com",
            "a..b@c.com",
            "\"a b\"@c.com",
            "a b@c.com",
            "@c.com",
            "a@",
            "a@[999.1.1.1]",
            "a@[not-an-ip]",
            "plainaddress",
            "a@b@c.com",
            "user@tld",
            "üñîçødé@example.com",
            "x@y.z1",
            "x@1.2.3.4",
        ];
        for address in invalid {
            assert!(!validate_email_address(address), "invalid: {address}");
        }
        // 64-char labels and the 320-char cap.
        assert!(!validate_email_address(&format!(
            "a@{}",
            "b".repeat(64) + ".com"
        )));
        assert!(!validate_email_address(&format!("a@b.{}", "c".repeat(64))));
        assert!(!validate_email_address(
            "u@toolonglabeltoolonglabeltoolonglabeltoolonglabeltoolonglabeltoolonglabel.com"
        ));
    }

    // -- session hash ------------------------------------------------------

    #[test]
    fn session_auth_hash_matches_django_salted_hmac() {
        // Oracle: Django 6.0 `salted_hmac(...get_session_auth_hash,
        // "pbkdf2_sha256$1200000$abc$def", secret="topsecret",
        // algorithm="sha256")` (the default `algorithm="sha1"` is a
        // trap — `get_session_auth_hash` passes sha256 explicitly).
        assert_eq!(
            make_session_auth_hash("pbkdf2_sha256$1200000$abc$def", b"topsecret"),
            "9c9a3cd895ca244f0c1fc8a4d24cd83d65287bb3353f39d69ab926a0d240b079"
        );
    }

    // -- misc pure helpers ---------------------------------------------------

    #[test]
    fn helpers_match_python_shapes() {
        assert_eq!(avatar_extension("image/jpeg"), Some("jpg"));
        assert_eq!(avatar_extension("image/png"), Some("png"));
        assert_eq!(avatar_extension("image/gif"), Some("gif"));
        assert_eq!(avatar_extension("image/webp"), Some("webp"));
        assert_eq!(avatar_extension("image/svg+xml"), None);
        assert_eq!(get_display_name("ada@example.com"), "ada");
        assert_eq!(get_display_name("no-at-sign").len(), 6);
        assert_eq!(name_or_empty(&serde_json::json!("Ada")), "Ada");
        assert_eq!(name_or_empty(&serde_json::Value::Null), "");
        assert_eq!(text_or_none(&serde_json::Value::Null), None);
        assert_eq!(text_or_none(&serde_json::json!("x")), Some("x".to_owned()));
        assert_eq!(
            json_param(&serde_json::Value::Null),
            pidash_services::auth_session::ParamValue::Null
        );
        // `gitlab_configured`: empty-string-is-falsy.
        assert!(gitlab_configured(&GitlabConfig {
            client_id: Some("id".to_owned()),
            client_secret: Some("secret".to_owned()),
            host: Some("https://gitlab.com".to_owned()),
        }));
        for config in [
            GitlabConfig {
                client_id: None,
                client_secret: Some("s".to_owned()),
                host: Some("h".to_owned()),
            },
            GitlabConfig {
                client_id: Some(String::new()),
                client_secret: Some("s".to_owned()),
                host: Some("h".to_owned()),
            },
            GitlabConfig {
                client_id: Some("i".to_owned()),
                client_secret: Some("s".to_owned()),
                host: Some(String::new()),
            },
        ] {
            assert!(!gitlab_configured(&config));
        }
    }

    #[test]
    fn provider_kernels_match_fixtures() {
        let f3 = fixture("F3_provider_auth_url.golden.json");
        let gitlab = &f3["gitlab"];
        assert!(gitlab["gitlab_host_default"]
            .as_str()
            .unwrap()
            .contains("https://gitlab.com"));
        assert_eq!(
            pidash_services::auth_oauth::providers::GITLAB_HOST_DEFAULT,
            "https://gitlab.com"
        );
        assert_eq!(
            gitlab["scope"].as_str().unwrap(),
            pidash_services::auth_oauth::providers::GITLAB_SCOPE
        );
        // `redirect_uri` shape: scheme + host + provider callback path.
        assert_eq!(
            pidash_services::auth_oauth::providers::redirect_uri(
                false,
                "app.example.com",
                "gitlab"
            ),
            "http://app.example.com/auth/gitlab/callback/"
        );
        assert_eq!(
            pidash_services::auth_oauth::providers::redirect_uri(true, "app.example.com", "gitlab"),
            "https://app.example.com/auth/gitlab/callback/"
        );
        // Token/userinfo URLs hang off the configured host.
        assert_eq!(
            pidash_services::auth_oauth::providers::gitlab_token_url("https://git.example.com"),
            "https://git.example.com/oauth/token"
        );
        assert_eq!(
            pidash_services::auth_oauth::providers::gitlab_userinfo_url("https://git.example.com"),
            "https://git.example.com/api/v4/user"
        );
        // F10/F11 trace lines name these source files.
        assert!(f3["trace"].as_str().unwrap().contains("gitlab"));
        let f10 = fixture("F10_initiate.golden.json");
        assert_eq!(
            f10["app_google_github_gitlab"]["provider_error_codes"]["gitlab"]
                .as_u64()
                .unwrap(),
            u64::from(NOT_CONFIGURED_CODE as u32)
        );
        let f11 = fixture("F11_callback.golden.json");
        assert_eq!(
            f11["app_google_github_gitlab"]["provider_error_codes"]["gitlab"]
                .as_u64()
                .unwrap(),
            u64::from(PROVIDER_ERROR_CODE as u32)
        );
        assert!(f11["space_google_github_gitlab_PORT_BUG"]["mechanism"]
            .as_str()
            .unwrap()
            .contains("shadows"));
    }

    #[test]
    fn oauth_view_shape_matches_guards() {
        // The space gitlab initiate writes host+state only (B3); the
        // guards kernel pins the same matrix row.
        assert_eq!(
            super::super::initiate_session_keys(true, super::super::OauthProvider::Gitlab),
            &super::super::OAUTH_SPACE_INITIATE_SESSION_KEYS
        );
        assert_eq!(
            super::super::initiate_session_keys(false, super::super::OauthProvider::Gitlab),
            &super::super::OAUTH_APP_INITIATE_SESSION_KEYS
        );
    }
}
