#![forbid(unsafe_code)]

//! D-17 Google OAuth handlers: app + space initiate/callback (stage 5).
//!
//! Ports `apps/api/pi_dash/authentication/views/app/google.py` (initiate
//! `GET :28-58`, callback `GET :62-104`) and
//! `apps/api/pi_dash/authentication/views/space/google.py` (space twins
//! `:26-54`, `:58-102`). `provider/oauth/google.py` is a read-only
//! reference: its pure parts already live in
//! `pidash_services::auth_oauth::providers` (PIDASHCONV-325).
//!
//! Fixture ids: AUTHOAUTH-F10 (initiate goldens) and AUTHOAUTH-F11
//! (callback goldens) in `rust-api/fixtures/auth_oauth/` (PIDASHCONV-324).
//! The `#[cfg(test)]` suite replays the google rows of both fixtures.
//!
//! Out of scope (sibling issues): github/gitlab/gitea twins
//! (PIDASHCONV-336/339/341), device endpoints (PIDASHCONV-342/343,
//! AUTHOAUTH-F12), account-upsert SQL (PIDASHCONV-327, reused here, not
//! redefined), guards (PIDASHCONV-331, `super::guards`).
//!
//! # Branch map (translate-only; every branch below mirrors its Python site)
//!
//! * Initiate (app + space): write `session["host"]`, store `next_path`
//!   only on the app side (raw, unvalidated —
//!   `views/app/google.py:30-32`), gate on `Instance` setup (5000), then
//!   build the provider auth URL (5105 when unconfigured). Space initiates
//!   never store `next_path` (`views/space/google.py` has no such write).
//! * App callback: state-mismatch then missing-code (both 5115), then the
//!   exchange → provision → login → redirect pipeline; any
//!   `AuthenticationException` redirects with its own code.
//! * Space callback: `base_host = request.session.get("host")` at the top
//!   shadows the imported helper for the whole function body, so every
//!   later `base_host(request=...)` call raises `TypeError` and Django
//!   answers 500 on every input (`test_callback.py` pins this; F11
//!   `space_google_github_gitlab_PORT_BUG`). The port runs the same
//!   side-effecting pipeline on the valid path, then answers 500 instead
//!   of building the shadowed URL.
//!
//! # Ported bugs (translate, don't redesign; also listed in the PR)
//!
//! * `BUG-SPACE-SHADOW`: the space-callback `base_host` shadowing above —
//!   500 on every input, including valid code+state (side effects still
//!   run first on that path, as in Python).
//! * `BUG-SIGNUP-FLAG` (`adapter/base.py:299,353-354`): `is_signup =
//!   bool(existing_user)` is inverted, and the sync gate consumes the
//!   inversion (`check_sync_enabled() and not is_signup`), so IDP sync
//!   runs for newly created users only — never for existing ones. The
//!   flag is also passed to the post-auth callback, which ignores it
//!   (`workspace_project_join.py` takes only `user`).
//! * `BUG-PROJECT-INVITE-COLUMNS` (`workspace_project_join.py:76-87`):
//!   the `ProjectMember` bulk insert omits `project_id` (NOT NULL), so any
//!   signup carrying an accepted project invite raises `IntegrityError`
//!   even with `ON CONFLICT DO NOTHING` — reproduced as-is.
//! * `BUG-REDIRECT-SLASHLESS` (`redirection_path.py:8-46`): success paths
//!   without a session `next_path` (`onboarding`, `<slug>`,
//!   `invitations`, `create-workspace`) fail `validate_next_path` and the
//!   redirect lands on the bare base — inherited from the shared kernel.
//! * NULL `last_login_ip` / `last_login_uagent` (`login.py:21-25`): absent
//!   headers bind NULL into NOT NULL columns, so the stamps update raises
//!   (500), exactly like Django's `save()`.
//!
//! # Wired success effects (PIDASHCONV-475; via PIDASHCONV-479/480)
//!
//! * Avatar persistence (`download_and_upload_avatar`, `delete_old_avatar`,
//!   `base.py:139-254,285-287`): the provider bytes download over reqwest
//!   (10s timeout, `DATA_UPLOAD_MAX_MEMORY_SIZE` guards) and upload through
//!   the assets/storage-owned signer (`pidash_storage::sign_put`,
//!   `sign_head`, `sign_delete` — PIDASHCONV-480; this domain never signs
//!   SigV4 itself), then a `FileAsset` row (`USER_AVATAR`, uploaded) is
//!   inserted and the FK set. Every S3 failure takes Python's own fallback
//!   — the provider URL is stored directly, as `base.py:335-345` does —
//!   and nothing here 500s.
//! * Cache invalidation (`invalidate_cache_directly`, one Redis `KEYS` +
//!   `DEL` per joined workspace): `RedisHandle::invalidate_matching`
//!   (PIDASHCONV-479) runs per joined workspace slug with the exact
//!   `*{path}*` pattern; Redis failures propagate like Python.
//! * `BUG-content-type-exact-match` (via the shared helper,
//!   `base.py:165-175`): the extension allowlist keys on the raw
//!   `Content-Type` header value, so `"image/png; charset=binary"` misses
//!   and falls back to the provider URL, exactly like Python.
//!
//! # Remaining documented divergences
//!
//! * Celery `.delay()` publishes (`user_activation_email`, `track_event`)
//!   enqueue into the Postgres job queue (`pidash_jobs::queue::enqueue`,
//!   warn-and-continue on failure) per the `handlers_webhook` precedent.
//!   Python raises to 500 when the broker is down; the Rust port lets the
//!   response stand, like every other merged handler.
//! * The space-callback valid path writes the login session, then answers
//!   500: the session middleware's 5xx status gate does not persist it, so
//!   no `session-id` cookie is set. Django's own 500 path would persist
//!   the session; that branch is unreachable without live provider
//!   credentials (pinned oracle limits), so the difference is untestable.

use std::collections::HashMap;
use std::net::SocketAddr;

use axum::extract::{Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Extension, Router};
use serde_json::Value;

use crate::auth_session::render::disallowed_redirect;
use crate::middleware::SessionHandle;
use crate::state::AppState;
use pidash_db::redis::RedisHandle;

use pidash_services::auth_oauth::error::AuthenticationException;
use pidash_services::auth_oauth::exchange::map_exchange_error;
use pidash_services::auth_oauth::providers::{
    google_auth_url, google_configured, google_token_data, google_token_post_data,
    google_user_data, redirect_uri, GOOGLE_USERINFO_URL,
};
use pidash_services::auth_session::{
    allowed_hosts_for, base_host, get_safe_redirect_url, redirection_path_str,
    select_redirection_path, HostSettings, ParamValue, MAX_URL_REDIRECT_LENGTH,
};

// ---------------------------------------------------------------------------
// Route registration: the cutover granularity is the route. Only these four
// GETs are owned; every other method proxies to Django (plain `View`
// semantics: Django's own 405/OPTIONS bodies stay Django's).
// ---------------------------------------------------------------------------

/// Google OAuth app + space initiate/callback routes.
///
/// Registration is the cutover granularity (Porting guide, pilot-2 row):
/// sibling paths have no Rust route and keep proxying to Django through
/// the fallback, so no per-path flag is needed.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/auth/google/", owned(get(app_initiate)))
        .route("/auth/google/callback/", owned(get(app_callback)))
        .route("/auth/spaces/google/", owned(get(space_initiate)))
        .route("/auth/spaces/google/callback/", owned(get(space_callback)))
}

/// A Google OAuth path: the GET handler owns reads, everything else falls
/// through to Django (its 405/OPTIONS bodies live there). `HEAD` proxies
/// explicitly: axum would auto-serve it from `get`, but Django defines no
/// `head` and 405s after auth.
fn owned(
    get_handler: axum::routing::MethodRouter<AppState>,
) -> axum::routing::MethodRouter<AppState> {
    get_handler
        .post(crate::edge::proxy)
        .put(crate::edge::proxy)
        .patch(crate::edge::proxy)
        .delete(crate::edge::proxy)
        .head(crate::edge::proxy)
        .options(crate::edge::proxy)
}

// ---------------------------------------------------------------------------
// Constants: provider identity, error codes, config keys, task names.
// ---------------------------------------------------------------------------

/// `GoogleOAuthProvider.provider` (`provider/oauth/google.py:25`).
pub const PROVIDER: &str = "google";
/// `GoogleOAuthProvider.token_url` (`:22`).
pub const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";

/// `AUTHENTICATION_ERROR_CODES["INSTANCE_NOT_CONFIGURED"]` (`error.py:8`).
pub const INSTANCE_NOT_CONFIGURED_CODE: i32 = 5000;
/// `AUTHENTICATION_ERROR_CODES["GOOGLE_NOT_CONFIGURED"]` (`error.py:43`).
pub const GOOGLE_NOT_CONFIGURED_CODE: i32 = 5105;
/// `AUTHENTICATION_ERROR_CODES["GOOGLE_OAUTH_PROVIDER_ERROR"]`
/// (`error.py:48`).
pub const GOOGLE_PROVIDER_ERROR_CODE: i32 = 5115;
/// `AUTHENTICATION_ERROR_CODES["INVALID_EMAIL"]` (`error.py:9`).
pub const INVALID_EMAIL_CODE: i32 = 5005;
/// `AUTHENTICATION_ERROR_CODES["SIGNUP_DISABLED"]` (`error.py:12`).
pub const SIGNUP_DISABLED_CODE: i32 = 5035;

/// `get_configuration_value` keys the google provider reads
/// (`provider/oauth/google.py:28-39`).
pub const GOOGLE_CLIENT_ID_KEY: &str = "GOOGLE_CLIENT_ID";
/// `get_configuration_value` keys the google provider reads (`:34-37`).
pub const GOOGLE_CLIENT_SECRET_KEY: &str = "GOOGLE_CLIENT_SECRET";

/// `user_activation_email.delay` wire name
/// (`bgtasks/user_activation_email_task.py:23`; also pinned at
/// `services/src/auth_session/tasks.rs:92-93`).
pub const USER_ACTIVATION_EMAIL_TASK: &str =
    "pi_dash.bgtasks.user_activation_email_task.user_activation_email";
/// `track_event.delay` wire name (`bgtasks/event_tracking_task.py:61-62`).
pub const TRACK_EVENT_TASK: &str = "pi_dash.bgtasks.event_tracking_task.track_event";
/// `track_event` event for a joined workspace
/// (`utils/analytics_events.py`, `USER_JOINED_WORKSPACE`).
pub const USER_JOINED_WORKSPACE_EVENT: &str = "user_joined_workspace";

/// `make_password` iteration count under the pinned Django 4.2.30
/// (`PBKDF2PasswordHasher.iterations`, verified against the installed
/// Django; the admin port pins the same value).
pub const PASSWORD_HASH_ITERATIONS: u32 = 600_000;

// ---------------------------------------------------------------------------
// Small pure helpers (branch logic shared by the handlers; unit-tested
// against AUTHOAUTH-F10/F11 below).
// ---------------------------------------------------------------------------

/// 302 with a `Location`, mirroring `HttpResponseRedirect(url)`.
/// Django percent-encodes the URL first (`iri_to_uri`,
/// `django/http/response.py:620`). Past `MAX_URL_REDIRECT_LENGTH`
/// characters the final `Location` answers 400 (`DisallowedRedirect`).
pub fn redirect_response(location: &str) -> Response {
    let too_long = location.chars().count() > MAX_URL_REDIRECT_LENGTH;
    let encoded = crate::response_headers::iri_to_uri(location);
    let Some(value) = crate::response_headers::header_value(&encoded) else {
        return server_error();
    };
    if too_long {
        return disallowed_redirect();
    }
    let mut response = StatusCode::FOUND.into_response();
    response.headers_mut().insert(header::LOCATION, value);
    response
}

/// Uncaught-exception answer. Plain Django `View`s have no
/// `handle_exception` matrix: anything escaping `get()` becomes Django's
/// technical 500 page. The contract pins the status only, so the body is
/// empty.
pub fn server_error() -> Response {
    StatusCode::INTERNAL_SERVER_ERROR.into_response()
}

/// `params = exc.get_error_dict()` rendered for `get_safe_redirect_url`:
/// `error_code` (int, never quote_plus-encoded) then `error_message`,
/// then payload entries in order, preserving `get_error_dict` order
/// (`INVALID_EMAIL` / `SIGNUP_DISABLED` redirects carry `email=`).
pub fn error_params(exception: &AuthenticationException) -> Vec<(&str, ParamValue)> {
    let mut out = vec![
        ("error_code", ParamValue::Int(exception.error_code as i64)),
        (
            "error_message",
            ParamValue::Str(exception.error_message.clone()),
        ),
    ];
    for (key, value) in &exception.payload {
        out.push((key.as_str(), json_param_value(value)));
    }
    out
}

/// `urlencode` rendering of one payload value. `str` / `int` / `bool` /
/// `None` match CPython exactly; composite values fall back to their JSON
/// rendering (unreachable for the string-only payloads this module emits).
pub fn json_param_value(value: &Value) -> ParamValue {
    match value {
        Value::String(text) => ParamValue::Str(text.clone()),
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                ParamValue::Int(int)
            } else {
                ParamValue::Str(number.to_string())
            }
        }
        Value::Bool(flag) => ParamValue::Bool(*flag),
        Value::Null => ParamValue::Null,
        other => ParamValue::Str(serde_json::to_string(other).unwrap_or_default()),
    }
}

/// Error redirect shared by every initiate/callback error branch:
/// `get_safe_redirect_url(base_url=..., next_path=..., params=...)`.
pub fn error_redirect(
    base_url: &str,
    next_path: Option<&str>,
    exception: &AuthenticationException,
    settings: &HostSettings<'_>,
) -> String {
    get_safe_redirect_url(
        base_url,
        next_path.unwrap_or(""),
        &error_params(exception),
        &allowed_hosts(settings),
    )
}

/// `state != request.session.get("state", "")`
/// (`views/app/google.py:67`, space `:64`): a missing GET `state`
/// (`None`) never equals the `""` default, so a fresh session always
/// mismatches — even before any provider traffic.
pub fn states_match(get_state: Option<&str>, session_state: Option<&str>) -> bool {
    get_state == Some(session_state.unwrap_or(""))
}

/// App-initiate `next_path` session rule (`views/app/google.py:30-32`):
/// stored raw (`str(next_path)`, unvalidated) when truthy, untouched
/// otherwise. Space initiates never store it.
pub fn app_initiate_session_next(get_next_path: Option<&str>) -> Option<String> {
    get_next_path
        .filter(|next| !next.is_empty())
        .map(str::to_owned)
}

/// `HostSettings` over the resolved `Settings` URL fields.
pub fn host_settings(settings: &pidash_db::config::Settings) -> HostSettings<'_> {
    HostSettings {
        web_url: settings.urls.web_url.as_deref(),
        app_base_url: settings.urls.app_base_url.as_deref(),
        admin_base_url: settings.urls.admin_base_url.as_deref(),
        space_base_url: settings.urls.space_base_url.as_deref(),
        admin_base_path: None,
        space_base_path: None,
    }
}

/// `get_allowed_hosts()` (`path_validator.py:70-86`) over settings.
pub fn allowed_hosts<'a>(settings: &'a HostSettings<'a>) -> Vec<&'a str> {
    allowed_hosts_for(
        settings.web_url,
        settings.app_base_url,
        settings.admin_base_url,
        settings.space_base_url,
    )
}

/// Resolve one legacy config value (`instance_value.py:30-72`): the
/// caller-supplied default is the only fallback; unregistered keys read
/// the environment. Returns the string when non-empty (Python truthiness
/// for the `configured` checks).
pub fn config_string(value: &pidash_db::config::value::ConfigValue) -> Option<String> {
    match value {
        pidash_db::config::value::ConfigValue::Str(text) if !text.is_empty() => Some(text.clone()),
        _ => None,
    }
}

/// `request.is_secure()`: `X-Forwarded-Proto: https` exactly when
/// `SECURE_PROXY_SSL_HEADER` is configured (production), else the local
/// (http) scheme — mirroring `HttpRequest.is_secure()`.
pub fn is_secure_request(headers: &HeaderMap, settings: &pidash_db::config::Settings) -> bool {
    if !settings.secure_proxy_ssl_header {
        return false;
    }
    headers
        .get("x-forwarded-proto")
        .and_then(|value| value.to_str().ok())
        == Some("https")
}

/// `get_client_ip` (`utils/ip_address.py:8-14`): first `X-Forwarded-For`
/// entry, unstripped, else `REMOTE_ADDR`, else `None` (which binds NULL
/// downstream — the documented 500 hazard). Handlers pass `None` for the
/// peer: axum handlers do not see it without `ConnectInfo` (same call as
/// the admin port's sign-out stamps).
pub fn client_ip(headers: &HeaderMap, peer: Option<SocketAddr>) -> Option<String> {
    if let Some(forwarded) = headers
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
    {
        if !forwarded.is_empty() {
            return Some(forwarded.split(',').next().unwrap_or("").to_owned());
        }
    }
    peer.map(|addr| addr.ip().to_string())
}

/// Query-string read mirroring `request.GET.get(key)`: the last value wins
/// (Django `QueryDict.get`), `None` when absent.
pub fn query_get(query: &HashMap<String, String>, key: &str) -> Option<String> {
    query.get(key).cloned()
}

// ---------------------------------------------------------------------------
// Session access. `request.session[k]` reads mark `accessed` (a `Vary:
// Cookie` at most); writes mark `modified` (save + `Set-Cookie`). The
// callback error branches only read, so they never set a session cookie —
// the permissions suite pins exactly that.
// ---------------------------------------------------------------------------

/// Read one session key as a string (`request.session.get(key)`).
pub fn session_get(handle: &SessionHandle, key: &str) -> Option<String> {
    handle
        .lock()
        .get(key)
        .and_then(|value| value.as_str())
        .map(str::to_owned)
}

/// Write one session key (`request.session[key] = value`).
pub fn session_set(handle: &SessionHandle, key: &str, value: String) {
    handle.lock().set(key.to_owned(), Value::String(value));
}

/// The live request context every handler builds first: query params,
/// headers, peer address, scheme/host inputs, settings views, and the DB
/// pool. Anything missing here is a server misconfiguration (Django would
/// always have a session and a database), so the handlers answer 500.
pub struct RequestContext<'a> {
    pub query: HashMap<String, String>,
    pub headers: HeaderMap,
    pub host: String,
    pub secure: bool,
    /// `META.get("HTTP_USER_AGENT")` (`adapter/base.py:227`): `None` when
    /// absent — the NULL hazard. `login.py:22` defaults the same header to
    /// `""` for `device_info`; see [`RequestContext::device_user_agent`].
    pub user_agent: Option<String>,
    pub ip: Option<String>,
    pub settings_view: HostSettings<'a>,
    pub app_base: String,
    pub space_base: String,
    pub pool: sqlx::PgPool,
    pub session: SessionHandle,
    /// Shared Redis handle for the join-path cache invalidation
    /// (`invalidate_cache_directly`, PIDASHCONV-479). `None` when
    /// `REDIS_URL` is unset — the join path then 500s like Python does
    /// when its cache backend is down.
    pub redis: Option<RedisHandle>,
    /// S3 endpoint/credentials for the avatar upload + delete
    /// (`pidash_storage`, PIDASHCONV-480).
    pub storage: pidash_db::config::StorageSettings,
}

impl<'a> RequestContext<'a> {
    // `Response` is axum's handle type, so boxing it buys no runtime win;
    // the crate-wide `Result<_, Response>` helper shape stays as-is
    // (same rationale as PIDASHCONV-468).
    #[allow(clippy::result_large_err)]
    pub fn build(
        state: &'a AppState,
        query: HashMap<String, String>,
        headers: HeaderMap,
        session: Option<Extension<SessionHandle>>,
    ) -> Result<Self, Response> {
        // `request.get_host()`: a missing or non-ASCII `Host` header is
        // `DisallowedHost` (400) in Django.
        let host = headers
            .get(header::HOST)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned)
            .ok_or_else(|| StatusCode::BAD_REQUEST.into_response())?;
        let pool = state
            .pools()
            .map(|pools| pools.primary().clone())
            .ok_or_else(server_error)?;
        let settings = state.settings();
        let settings_view = host_settings(settings);
        let app_base = base_host(&settings_view, false, false, true);
        let space_base = base_host(&settings_view, false, true, false);
        Ok(Self {
            query,
            user_agent: headers
                .get("user-agent")
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned),
            ip: client_ip(&headers, None),
            headers,
            host,
            secure: false,
            settings_view,
            app_base,
            space_base,
            pool,
            session: session
                .map(|extension| extension.0)
                .ok_or_else(server_error)?,
            redis: state.redis().cloned(),
            storage: state.settings().storage.clone(),
        })
    }

    pub fn finish_scheme(mut self, settings: &pidash_db::config::Settings) -> Self {
        self.secure = is_secure_request(&self.headers, settings);
        self
    }

    /// `request.META.get("HTTP_USER_AGENT", "")` (`login.py:22`).
    pub fn device_user_agent(&self) -> &str {
        self.user_agent.as_deref().unwrap_or("")
    }
}

/// `Instance.objects.first()` gate (`views/app/google.py:35-45`,
/// space `:31-41`): missing row or `is_setup_done == false` redirects
/// with `INSTANCE_NOT_CONFIGURED` (5000). Returns `Ok(true)` when setup
/// is done.
pub async fn instance_setup_done(pool: &sqlx::PgPool) -> Result<bool, sqlx::Error> {
    let row = pidash_db::license::queries::fetch_instance_first(pool).await?;
    Ok(row.is_some_and(|instance| instance.is_setup_done))
}

/// Google provider config (`provider/oauth/google.py:28-45`):
/// `get_configuration_value` with `os.environ` defaults, then
/// `if not (GOOGLE_CLIENT_ID and GOOGLE_CLIENT_SECRET)` raises
/// `GOOGLE_NOT_CONFIGURED` (5105). A failing config read is not an
/// `AuthenticationException` in Python (it escapes to the technical
/// 500), so it is [`CallbackError::Internal`] here.
pub async fn google_config(
    pool: &sqlx::PgPool,
    secret: &str,
) -> Result<(String, String), CallbackError> {
    use pidash_db::config::accessor::PgConfigStore;
    use pidash_db::config::encryption::Keyring;
    use pidash_db::config::legacy::{get_configuration_values, LegacyItem};
    use pidash_db::config::{registry, value::ConfigValue};

    let store = PgConfigStore::new(pool.clone());
    let keyring = Keyring::from_secret(secret);
    let items = vec![
        LegacyItem::new(
            GOOGLE_CLIENT_ID_KEY,
            std::env::var(GOOGLE_CLIENT_ID_KEY)
                .map(ConfigValue::Str)
                .unwrap_or(ConfigValue::Null),
        ),
        LegacyItem::new(
            GOOGLE_CLIENT_SECRET_KEY,
            std::env::var(GOOGLE_CLIENT_SECRET_KEY)
                .map(ConfigValue::Str)
                .unwrap_or(ConfigValue::Null),
        ),
    ];
    let values = get_configuration_values(registry::global(), &store, &keyring, &items)
        .await
        .map_err(|_| CallbackError::Internal)?;
    let client_id = config_string(&values[0]);
    let client_secret = config_string(&values[1]);
    if google_configured(client_id.as_deref(), client_secret.as_deref()) {
        Ok((
            client_id.unwrap_or_default(),
            client_secret.unwrap_or_default(),
        ))
    } else {
        Err(CallbackError::Auth(AuthenticationException::new(
            GOOGLE_NOT_CONFIGURED_CODE,
            "GOOGLE_NOT_CONFIGURED",
            Vec::new(),
        )))
    }
}

/// `uuid.uuid4().hex` (`views/app/google.py:48`, space `:44`).
pub fn new_state() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

// ---------------------------------------------------------------------------
// Initiate handlers.
// ---------------------------------------------------------------------------

/// `GoogleOauthInitiateEndpoint.get` (`views/app/google.py:28-58`).
pub async fn app_initiate(
    State(state): State<AppState>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
    session: Option<Extension<SessionHandle>>,
) -> Response {
    let ctx = match RequestContext::build(&state, query, headers, session) {
        Ok(ctx) => ctx.finish_scheme(state.settings()),
        Err(response) => return response,
    };
    // `request.session["host"] = base_host(request=request, is_app=True)`
    session_set(&ctx.session, "host", ctx.app_base.clone());
    // `if next_path: request.session["next_path"] = str(next_path)`
    let next_path = query_get(&ctx.query, "next_path");
    if let Some(store) = app_initiate_session_next(next_path.as_deref()) {
        session_set(&ctx.session, "next_path", store);
    }
    match instance_setup_done(&ctx.pool).await {
        Err(_) => return server_error(),
        Ok(false) => {
            let exception = AuthenticationException::new(
                INSTANCE_NOT_CONFIGURED_CODE,
                "INSTANCE_NOT_CONFIGURED",
                Vec::new(),
            );
            let url = error_redirect(
                &ctx.app_base,
                next_path.as_deref(),
                &exception,
                &ctx.settings_view,
            );
            return redirect_response(&url);
        }
        Ok(true) => {}
    }
    match google_config(&ctx.pool, state.settings().secret_key.as_str()).await {
        Ok((client_id, _)) => {
            let hex = new_state();
            session_set(&ctx.session, "state", hex.clone());
            let url = google_auth_url(&client_id, ctx.secure, &ctx.host, &hex);
            redirect_response(&url)
        }
        Err(CallbackError::Auth(exception)) => {
            let url = error_redirect(
                &ctx.app_base,
                next_path.as_deref(),
                &exception,
                &ctx.settings_view,
            );
            redirect_response(&url)
        }
        Err(CallbackError::Internal) => server_error(),
    }
}

/// `GoogleOauthInitiateSpaceEndpoint.get` (`views/space/google.py:26-54`):
/// the app twin minus the `next_path` session write.
pub async fn space_initiate(
    State(state): State<AppState>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
    session: Option<Extension<SessionHandle>>,
) -> Response {
    let ctx = match RequestContext::build(&state, query, headers, session) {
        Ok(ctx) => ctx.finish_scheme(state.settings()),
        Err(response) => return response,
    };
    // `request.session["host"] = base_host(request=request, is_space=True)`
    session_set(&ctx.session, "host", ctx.space_base.clone());
    let next_path = query_get(&ctx.query, "next_path");
    match instance_setup_done(&ctx.pool).await {
        Err(_) => return server_error(),
        Ok(false) => {
            let exception = AuthenticationException::new(
                INSTANCE_NOT_CONFIGURED_CODE,
                "INSTANCE_NOT_CONFIGURED",
                Vec::new(),
            );
            let url = error_redirect(
                &ctx.space_base,
                next_path.as_deref(),
                &exception,
                &ctx.settings_view,
            );
            return redirect_response(&url);
        }
        Ok(true) => {}
    }
    match google_config(&ctx.pool, state.settings().secret_key.as_str()).await {
        Ok((client_id, _)) => {
            let hex = new_state();
            session_set(&ctx.session, "state", hex.clone());
            let url = google_auth_url(&client_id, ctx.secure, &ctx.host, &hex);
            redirect_response(&url)
        }
        Err(CallbackError::Auth(exception)) => {
            let url = error_redirect(
                &ctx.space_base,
                next_path.as_deref(),
                &exception,
                &ctx.settings_view,
            );
            redirect_response(&url)
        }
        Err(CallbackError::Internal) => server_error(),
    }
}

// ---------------------------------------------------------------------------
// Callback success pipeline: exchange, provisioning, login, redirect.
// `OauthAdapter.authenticate` (`adapter/oauth.py:70-73`) is
// `set_token_data` → `set_user_data` → `complete_login_or_signup`
// (`adapter/base.py:289-360`); the view then calls `user_login` and
// builds the success redirect. Anything that is not an
// `AuthenticationException` is an internal failure (Django's technical
// 500): SQL errors, clock/parse failures, missing pool rows.
// ---------------------------------------------------------------------------

/// Pipeline failure: an `AuthenticationException` redirects with its own
/// code; anything else is Django's technical 500.
#[derive(Debug)]
pub enum CallbackError {
    Auth(AuthenticationException),
    Internal,
}

impl From<AuthenticationException> for CallbackError {
    fn from(exception: AuthenticationException) -> Self {
        CallbackError::Auth(exception)
    }
}

/// `requests.post(token_url, data=data, headers={})` + `raise_for_status`
/// (`adapter/oauth.py:75-84`): google posts with no headers
/// (`providers.TOKEN_JSON_ACCEPT_HEADERS` is for the other providers).
/// Transport failures, HTTP errors, and non-JSON bodies all map to
/// `GOOGLE_OAUTH_PROVIDER_ERROR` — `requests`' `JSONDecodeError` is a
/// `RequestException` subclass, so it is caught too.
pub async fn fetch_user_token(
    client: &reqwest::Client,
    code: &str,
    client_id: &str,
    client_secret: &str,
    redirect_uri: &str,
) -> Result<Value, CallbackError> {
    let pairs = google_token_post_data(code, client_id, client_secret, redirect_uri);
    let response = client
        .post(TOKEN_URL)
        .form(&pairs)
        .send()
        .await
        .map_err(|_| CallbackError::Auth(map_exchange_error(PROVIDER)))?;
    let response = response
        .error_for_status()
        .map_err(|_| CallbackError::Auth(map_exchange_error(PROVIDER)))?;
    let body = response
        .text()
        .await
        .map_err(|_| CallbackError::Auth(map_exchange_error(PROVIDER)))?;
    serde_json::from_str(&body).map_err(|_| CallbackError::Auth(map_exchange_error(PROVIDER)))
}

/// `requests.get(userinfo_url, headers={"Authorization": Bearer ...})`
/// (`adapter/oauth.py:86-100`).
pub async fn fetch_user_response(
    client: &reqwest::Client,
    userinfo_url: &str,
    access_token: &str,
) -> Result<Value, CallbackError> {
    let response = client
        .get(userinfo_url)
        .header("Authorization", format!("Bearer {access_token}"))
        .send()
        .await
        .map_err(|_| CallbackError::Auth(map_exchange_error(PROVIDER)))?;
    let response = response
        .error_for_status()
        .map_err(|_| CallbackError::Auth(map_exchange_error(PROVIDER)))?;
    let body = response
        .text()
        .await
        .map_err(|_| CallbackError::Auth(map_exchange_error(PROVIDER)))?;
    serde_json::from_str(&body).map_err(|_| CallbackError::Auth(map_exchange_error(PROVIDER)))
}

/// `datetime.fromtimestamp(value, tz=utc)` for a truthy `expires_in`
/// (`provider/oauth/google.py:89-98`): numbers are epoch seconds, falsy
/// values (`None`, `0`) map to `None`. A truthy non-number (or an
/// out-of-range epoch) raises outside `RequestException` in Python, so it
/// is [`CallbackError::Internal`] here.
pub fn expires_at(value: &Value) -> Result<Option<chrono::DateTime<chrono::Utc>>, CallbackError> {
    let seconds = match value {
        Value::Null | Value::Bool(false) => return Ok(None),
        Value::String(text) if text.is_empty() => return Ok(None),
        Value::Number(number) => number.as_f64().ok_or(CallbackError::Internal)?,
        Value::Bool(true) => 1.0,
        _ => return Err(CallbackError::Internal),
    };
    if seconds == 0.0 {
        return Ok(None);
    }
    let whole = seconds.trunc() as i64;
    let nanos = ((seconds - seconds.trunc()) * 1_000_000_000.0).round() as u32;
    chrono::DateTime::from_timestamp(whole, nanos)
        .map(Some)
        .ok_or(CallbackError::Internal)
}

/// Stored token fields for the upsert (`google.py:85-101` via
/// `providers::google_token_data`): expiry through [`expires_at`], and
/// `refresh_token_expired_at` is always absent in google responses, so the
/// mapper's `None` stands.
pub fn token_fields(
    token_response: &Value,
) -> Result<pidash_db::auth_oauth::queries::account::TokenFields, CallbackError> {
    use pidash_db::auth_oauth::queries::account::TokenFields;
    let mapped = google_token_data(token_response);
    Ok(TokenFields {
        access_token: mapped.access_token.as_str().map(str::to_owned),
        refresh_token: mapped.refresh_token.as_str().map(str::to_owned),
        access_token_expired_at: expires_at(
            token_response.get("expires_in").unwrap_or(&Value::Null),
        )?,
        refresh_token_expired_at: None,
        id_token: mapped.id_token.as_str().map(str::to_owned),
    })
}

/// Stored user payload (`google.py:103-115` via
/// `providers::google_user_data`), serialized in Python dict order for the
/// downstream `.get()` reads and the account lookup.
pub fn user_payload(userinfo: &Value) -> Value {
    let mapped = google_user_data(userinfo);
    serde_json::to_value(&mapped).unwrap_or(Value::Null)
}

/// `Adapter.sanitize_email` (`adapter/base.py:62-89`): missing email
/// raises `INVALID_EMAIL` with the raw value as payload; otherwise the
/// value is lowered + stripped (`str(email)` first, so numbers render —
/// then fail validation) and `validate_email` decides, raising the same
/// code with the cleaned value as payload on failure.
pub fn sanitize_email(raw: &Value) -> Result<String, AuthenticationException> {
    let invalid = |email: Value| {
        AuthenticationException::new(
            INVALID_EMAIL_CODE,
            "INVALID_EMAIL",
            vec![("email".to_owned(), email)],
        )
    };
    let text = match raw {
        Value::Null => return Err(invalid(Value::Null)),
        Value::String(text) if text.is_empty() => return Err(invalid(raw.clone())),
        Value::String(text) => text.clone(),
        Value::Number(number) => number.to_string(),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => return Err(invalid(raw.clone())),
        other => serde_json::to_string(other).unwrap_or_default(),
    };
    let cleaned = text.to_lowercase();
    let cleaned = cleaned.trim().to_owned();
    if crate::license::handlers_auth_forms::email_is_valid(&cleaned) {
        Ok(cleaned)
    } else {
        Err(invalid(Value::String(cleaned)))
    }
}

/// `make_password` salt: 22 chars from Django's `RANDOM_STRING_CHARS`
/// (128 bits of entropy; `BasePasswordHasher.salt`). Randomness comes
/// from two v4 UUIDs (244 bits) reduced mod 62 — uniform enough for a
/// salt, whose only requirement is uniqueness.
pub fn new_password_salt() -> String {
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let a = uuid::Uuid::new_v4();
    let b = uuid::Uuid::new_v4();
    let mut bytes = [0u8; 32];
    bytes[..16].copy_from_slice(a.as_bytes());
    bytes[16..].copy_from_slice(b.as_bytes());
    bytes
        .iter()
        .take(22)
        .map(|byte| ALPHABET[(usize::from(*byte)) % 62] as char)
        .collect()
}

/// `user.set_password(raw)` + `make_password` (`user.py` via
/// `AbstractBaseUser`): PBKDF2-SHA256 at [`PASSWORD_HASH_ITERATIONS`].
pub fn encode_password(password: &str) -> String {
    crate::license::handlers_auth_forms::encode_password(
        password,
        &new_password_salt(),
        PASSWORD_HASH_ITERATIONS,
    )
}

// ---------------------------------------------------------------------------
// Provisioning writes (`complete_login_or_signup`, `adapter/base.py:289-360,
// `save_user_data` `:220-234`, `sync_user_data` `:256-287`). Every statement
// is its own autocommit write — Python runs no transaction here — so the
// port issues one statement at a time over the pool.
// ---------------------------------------------------------------------------

/// Columns read for the login/signup decision
/// (`User.objects.filter(email=).first()`, D-16 `USER_SELECT_COLUMNS`
/// order via `user_by_email_sql`).
#[derive(Debug, Clone)]
pub struct UserRow {
    pub id: uuid::Uuid,
    pub password: String,
    pub email: Option<String>,
    pub is_active: bool,
    pub is_superuser: bool,
    pub first_name: String,
    pub last_name: String,
    pub display_name: String,
    pub avatar: String,
    pub avatar_asset_id: Option<uuid::Uuid>,
}

/// `User.objects.filter(email=email).first()` (`adapter/base.py:297`):
/// exact match, latest-created first (`Meta.ordering =
/// ("-created_at",)`), one row.
pub async fn find_user_by_email(
    pool: &sqlx::PgPool,
    email: &str,
) -> Result<Option<UserRow>, CallbackError> {
    use pidash_services::auth_session::queries::user_by_email_sql;
    let sql = format!("{} LIMIT 1", user_by_email_sql("users", "$1"));
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(&sql)
        .bind(email)
        .fetch_optional(pool)
        .await
        .map_err(|_| CallbackError::Internal)?;
    row.map(|row| {
        use sqlx::Row;
        Ok(UserRow {
            id: row.try_get("id").map_err(|_| CallbackError::Internal)?,
            password: row
                .try_get("password")
                .map_err(|_| CallbackError::Internal)?,
            email: row.try_get("email").map_err(|_| CallbackError::Internal)?,
            is_active: row
                .try_get("is_active")
                .map_err(|_| CallbackError::Internal)?,
            is_superuser: row
                .try_get("is_superuser")
                .map_err(|_| CallbackError::Internal)?,
            first_name: row
                .try_get("first_name")
                .map_err(|_| CallbackError::Internal)?,
            last_name: row
                .try_get("last_name")
                .map_err(|_| CallbackError::Internal)?,
            display_name: row
                .try_get("display_name")
                .map_err(|_| CallbackError::Internal)?,
            avatar: row.try_get("avatar").map_err(|_| CallbackError::Internal)?,
            avatar_asset_id: row
                .try_get("avatar_asset_id")
                .map_err(|_| CallbackError::Internal)?,
        })
    })
    .transpose()
}

/// `Adapter.__check_signup` (`adapter/base.py:95-119`): when
/// `ENABLE_SIGNUP == "0"` (string compare) and no
/// `WorkspaceMemberInvite` row carries the email, signup raises
/// `SIGNUP_DISABLED` (5035) with the email as payload. Any invite row
/// counts — accepted or not.
pub async fn check_signup(
    pool: &sqlx::PgPool,
    secret: &str,
    email: &str,
) -> Result<(), CallbackError> {
    use pidash_db::config::accessor::PgConfigStore;
    use pidash_db::config::encryption::Keyring;
    use pidash_db::config::legacy::{get_configuration_values, LegacyItem};
    use pidash_db::config::{registry, value::ConfigValue};

    let store = PgConfigStore::new(pool.clone());
    let keyring = Keyring::from_secret(secret);
    let items = vec![LegacyItem::new(
        "ENABLE_SIGNUP",
        std::env::var("ENABLE_SIGNUP")
            .map(ConfigValue::Str)
            .unwrap_or(ConfigValue::Str("1".to_owned())),
    )];
    let values = get_configuration_values(registry::global(), &store, &keyring, &items)
        .await
        .map_err(|_| CallbackError::Internal)?;
    let enabled = match &values[0] {
        ConfigValue::Str(text) => text.clone(),
        other => config_display(other),
    };
    if enabled != "0" {
        return Ok(());
    }
    let invited: Option<(i32,)> = sqlx::query_as(
        "SELECT 1 FROM \"workspace_member_invites\" WHERE \"email\" = $1 AND \"deleted_at\" IS NULL LIMIT 1",
    )
    .bind(email)
    .fetch_optional(pool)
    .await
    .map_err(|_| CallbackError::Internal)?;
    if invited.is_some() {
        return Ok(());
    }
    Err(AuthenticationException::new(
        SIGNUP_DISABLED_CODE,
        "SIGNUP_DISABLED",
        vec![("email".to_owned(), Value::String(email.to_owned()))],
    )
    .into())
}

/// String rendering of a non-string config value for the `== "0"` /
/// `== "1"` compares (`py_str` in the license domain).
pub fn config_display(value: &pidash_db::config::value::ConfigValue) -> String {
    use pidash_db::config::value::ConfigValue;
    match value {
        ConfigValue::Null => "None".to_owned(),
        ConfigValue::Str(text) => text.clone(),
        ConfigValue::Int(number) => number.to_string(),
        ConfigValue::Float(number) => number.to_string(),
        ConfigValue::Bool(true) => "True".to_owned(),
        ConfigValue::Bool(false) => "False".to_owned(),
    }
}

/// `User.save()` display-name backfill (`db/models/user.py:178-183`):
/// `email.split("@")[0]` (`len(split)` is never 0, so the prefix always
/// wins).
pub fn backfill_display_name(email: &str) -> String {
    email.split('@').next().unwrap_or("").to_owned()
}

/// `User.get_display_name(email)` (`user.py:190-197`): the local part
/// when exactly one `@` is present, else six random ASCII letters.
pub fn get_display_name(email: &str) -> String {
    if !email.is_empty() && email.split('@').count() == 2 {
        return email.split('@').next().unwrap_or("").to_owned();
    }
    const LETTERS: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";
    let seed = uuid::Uuid::new_v4();
    seed.as_bytes()
        .iter()
        .take(6)
        .map(|byte| LETTERS[(usize::from(*byte)) % 52] as char)
        .collect()
}

/// New-user row (`adapter/base.py:300-328` + `User.save()` side effects
/// `user.py:167-187`): `User(email, username=uuid4hex)`,
/// `set_password(uuid4hex)` + autoset/verified flags, names (each `""`
/// when falsy), then the save backfills `display_name` and leaves
/// `token` empty (`token_updated_at` is `None`, so no rotation).
pub async fn create_user(
    pool: &sqlx::PgPool,
    email: &str,
    first_name: &str,
    last_name: &str,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<UserRow, CallbackError> {
    let id = uuid::Uuid::new_v4();
    let username = new_state();
    let password = encode_password(&new_state());
    let display_name = backfill_display_name(email);
    let first_name = if first_name.is_empty() {
        String::new()
    } else {
        first_name.to_owned()
    };
    let last_name = if last_name.is_empty() {
        String::new()
    } else {
        last_name.to_owned()
    };
    sqlx::query(
        "INSERT INTO \"users\" (\"id\", \"password\", \"last_login\", \"username\", \
         \"mobile_number\", \"email\", \"display_name\", \"first_name\", \
         \"last_name\", \"avatar\", \"avatar_asset_id\", \"date_joined\", \
         \"created_at\", \"updated_at\", \"last_location\", \"created_location\", \
         \"is_superuser\", \"is_managed\", \"is_password_expired\", \
         \"is_active\", \"is_staff\", \"is_email_verified\", \
         \"is_password_autoset\", \"is_password_reset_required\", \"token\", \
         \"last_active\", \"last_login_time\", \"last_logout_time\", \
         \"last_login_ip\", \"last_logout_ip\", \"last_login_medium\", \
         \"last_login_uagent\", \"token_updated_at\", \"is_bot\", \"bot_type\", \
         \"user_timezone\", \"is_email_valid\", \"masked_at\") \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, \
         $15, $16, $17, $18, $19, $20, $21, $22, $23, $24, $25, $26, $27, $28, \
         $29, $30, $31, $32, $33, $34, $35, $36, $37, $38)",
    )
    .bind(id)
    .bind(password.clone())
    .bind(None::<chrono::DateTime<chrono::Utc>>)
    .bind(username)
    .bind(None::<String>)
    .bind(email)
    .bind(display_name.clone())
    .bind(first_name.clone())
    .bind(last_name.clone())
    .bind(String::new())
    .bind(None::<uuid::Uuid>)
    .bind(now)
    .bind(now)
    .bind(now)
    .bind(String::new())
    .bind(String::new())
    .bind(false)
    .bind(false)
    .bind(false)
    .bind(true)
    .bind(false)
    .bind(true)
    .bind(true)
    .bind(false)
    .bind(String::new())
    .bind(now)
    .bind(None::<chrono::DateTime<chrono::Utc>>)
    .bind(None::<chrono::DateTime<chrono::Utc>>)
    .bind(String::new())
    .bind(String::new())
    .bind("email")
    .bind(String::new())
    .bind(None::<chrono::DateTime<chrono::Utc>>)
    .bind(false)
    .bind(None::<String>)
    .bind("UTC")
    .bind(false)
    .bind(None::<chrono::DateTime<chrono::Utc>>)
    .execute(pool)
    .await
    .map_err(|_| CallbackError::Internal)?;
    Ok(UserRow {
        id,
        password,
        email: Some(email.to_owned()),
        is_active: true,
        is_superuser: false,
        first_name,
        last_name,
        display_name,
        avatar: String::new(),
        avatar_asset_id: None,
    })
}

// ---------------------------------------------------------------------------
// Profile, sync, stamps, task fan-out.
// ---------------------------------------------------------------------------

/// `get_random_color` (`utils/color.py:9-14`): `"#"` + six hexdigits.
/// Python draws from `string.hexdigits` (mixed case); the port draws from
/// a v4 UUID (lowercase only) — random either way, and no test can pin a
/// random color.
pub fn random_color() -> String {
    format!("#{}", &new_state()[..6])
}

/// The `Profile.objects.get_or_create(user=user)` read half: the
/// onboarding flag and last-workspace pointer `get_redirection_path`
/// branches on. `None` when the row is missing (the caller creates it).
pub struct ProfileState {
    pub is_onboarded: bool,
    pub last_workspace_id: Option<uuid::Uuid>,
}

/// `Profile.objects.get_or_create(user=user)` (`redirection_path.py:11`,
/// `adapter/base.py:342`): select, else insert with every model default
/// (`user.py:222-271`, `get_default_onboarding`,
/// `get_mobile_default_onboarding`, `get_default_product_tour`,
/// `get_random_color`).
pub async fn get_or_create_profile(
    pool: &sqlx::PgPool,
    user_id: uuid::Uuid,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<ProfileState, CallbackError> {
    let row: Option<(bool, Option<uuid::Uuid>)> = sqlx::query_as(
        "SELECT \"is_onboarded\", \"last_workspace_id\" FROM \"profiles\" WHERE \"user_id\" = $1 LIMIT 1",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| CallbackError::Internal)?;
    if let Some((is_onboarded, last_workspace_id)) = row {
        return Ok(ProfileState {
            is_onboarded,
            last_workspace_id,
        });
    }
    let onboarding = serde_json::json!({
        "profile_complete": false,
        "workspace_create": false,
        "workspace_invite": false,
        "workspace_join": false,
    });
    let mobile_onboarding = serde_json::json!({
        "profile_complete": false,
        "workspace_create": false,
        "workspace_join": false,
    });
    let product_tour = serde_json::json!({
        "work_items": false,
        "cycles": false,
        "modules": false,
        "intake": false,
        "pages": false,
    });
    sqlx::query(
        "INSERT INTO \"profiles\" (\"id\", \"created_at\", \"updated_at\", \
         \"user_id\", \"theme\", \"is_app_rail_docked\", \"is_tour_completed\", \
         \"onboarding_step\", \"use_case\", \"role\", \"is_onboarded\", \
         \"last_workspace_id\", \"billing_address_country\", \
         \"billing_address\", \"has_billing_address\", \"company_name\", \
         \"notification_view_mode\", \"is_smooth_cursor_enabled\", \
         \"is_mobile_onboarded\", \"mobile_onboarding_step\", \
         \"mobile_timezone_auto_set\", \"language\", \"start_of_the_week\", \
         \"goals\", \"background_color\", \
         \"is_navigation_tour_completed\", \"has_marketing_email_consent\", \
         \"is_subscribed_to_changelog\", \"product_tour\", \"settings\") \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, \
         $15, $16, $17, $18, $19, $20, $21, $22, $23, $24, $25, $26, $27, $28, \
         $29, $30)",
    )
    .bind(uuid::Uuid::new_v4())
    .bind(now)
    .bind(now)
    .bind(user_id)
    .bind(serde_json::json!({}))
    .bind(true)
    .bind(false)
    .bind(onboarding)
    .bind(None::<String>)
    .bind(None::<String>)
    .bind(false)
    .bind(None::<uuid::Uuid>)
    .bind("INDIA")
    .bind(None::<serde_json::Value>)
    .bind(false)
    .bind(String::new())
    .bind("full")
    .bind(false)
    .bind(false)
    .bind(mobile_onboarding)
    .bind(false)
    .bind("en")
    .bind(0i16)
    .bind(serde_json::json!({}))
    .bind(random_color())
    .bind(false)
    .bind(false)
    .bind(false)
    .bind(product_tour)
    .bind(serde_json::json!({}))
    .execute(pool)
    .await
    .map_err(|_| CallbackError::Internal)?;
    Ok(ProfileState {
        is_onboarded: false,
        last_workspace_id: None,
    })
}

/// `Adapter.check_sync_enabled` (`adapter/base.py:128-137`): the
/// `ENABLE_GOOGLE_SYNC` flag (default `"0"`, string compare). A failing
/// config read escapes to the technical 500 in Python.
pub async fn google_sync_enabled(pool: &sqlx::PgPool, secret: &str) -> Result<bool, CallbackError> {
    use pidash_db::config::accessor::PgConfigStore;
    use pidash_db::config::encryption::Keyring;
    use pidash_db::config::legacy::{get_configuration_values, LegacyItem};
    use pidash_db::config::{registry, value::ConfigValue};

    let store = PgConfigStore::new(pool.clone());
    let keyring = Keyring::from_secret(secret);
    let items = vec![LegacyItem::new(
        "ENABLE_GOOGLE_SYNC",
        std::env::var("ENABLE_GOOGLE_SYNC")
            .map(ConfigValue::Str)
            .unwrap_or(ConfigValue::Str("0".to_owned())),
    )];
    let values = get_configuration_values(registry::global(), &store, &keyring, &items)
        .await
        .map_err(|_| CallbackError::Internal)?;
    Ok(config_display(&values[0]) == "1")
}

/// `sync_user_data` (`adapter/base.py:256-287`): names, display name
/// (`get_display_name` — google payloads carry no `display_name`, so the
/// email prefix always wins), then the avatar pair: `delete_old_avatar`
/// first (S3 object + row + FK/field clear), then `download_and_upload_avatar`.
/// On success only the new FK is set (the avatar string keeps the `""` the
/// delete wrote); on failure the provider URL is stored — `base.py:278-284`
/// verbatim. The FK stays untouched when no asset was linked.
/// `max_bytes` is `settings.DATA_UPLOAD_MAX_MEMORY_SIZE`.
#[allow(clippy::too_many_arguments)]
pub async fn sync_user_data(
    client: &reqwest::Client,
    pool: &sqlx::PgPool,
    storage: &pidash_db::config::StorageSettings,
    scheme: &str,
    host: &str,
    user: &UserRow,
    email: &str,
    first_name: &str,
    last_name: &str,
    avatar_url: &str,
    max_bytes: u64,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<(), CallbackError> {
    sqlx::query(
        "UPDATE \"users\" SET \"first_name\" = $1, \"last_name\" = $2, \
         \"display_name\" = $3 WHERE \"id\" = $4",
    )
    .bind(if first_name.is_empty() {
        String::new()
    } else {
        first_name.to_owned()
    })
    .bind(if last_name.is_empty() {
        String::new()
    } else {
        last_name.to_owned()
    })
    .bind(get_display_name(email))
    .bind(user.id)
    .execute(pool)
    .await
    .map_err(|_| CallbackError::Internal)?;
    delete_old_avatar(
        client,
        pool,
        storage,
        scheme,
        host,
        user.id,
        user.avatar_asset_id,
        now,
    )
    .await;
    match download_and_upload_avatar(
        client, pool, storage, scheme, host, avatar_url, user.id, max_bytes, now,
    )
    .await
    {
        Some(asset_id) => {
            sqlx::query("UPDATE \"users\" SET \"avatar_asset_id\" = $1 WHERE \"id\" = $2")
                .bind(asset_id)
                .bind(user.id)
                .execute(pool)
                .await
                .map_err(|_| CallbackError::Internal)?;
        }
        None => {
            sqlx::query("UPDATE \"users\" SET \"avatar\" = $1 WHERE \"id\" = $2")
                .bind(avatar_url)
                .bind(user.id)
                .execute(pool)
                .await
                .map_err(|_| CallbackError::Internal)?;
        }
    }
    Ok(())
}

/// `download_and_upload_avatar` (`adapter/base.py:139-218`): provider
/// download (10s timeout, `Content-Length` pre-check, exact-match
/// content-type allowlist, streaming size guard), SigV4 PUT through the
/// shared signer, HEAD metadata, `FileAsset` row. ANY failure returns
/// `None` so the caller falls back to the provider URL
/// (`except Exception: log_exception; return None`). `storage` /
/// `scheme` / `host` mirror the `S3Storage(request)` server-side call the
/// avatar flow makes (`base.py:198`, `is_server=False`). `max_bytes` is
/// `settings.DATA_UPLOAD_MAX_MEMORY_SIZE`.
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
    // `get_avatar_download_headers` (`base.py:122-123`): empty for the base
    // adapter — no provider overrides it for google.
    let response = match client
        .get(avatar_url)
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
    {
        Ok(response) => response,
        Err(error) => {
            tracing::warn!(%error, "avatar download failed; falling back to provider URL");
            return None;
        }
    };
    let mut response = match response.error_for_status() {
        Ok(response) => response,
        Err(error) => {
            tracing::warn!(%error, "avatar download status failed; falling back to provider URL");
            return None;
        }
    };
    let headers = response.headers().clone();
    // `if content_length and int(content_length) > max_size: return None`
    // (`base.py:155-158`): a missing (or empty) header imposes no limit; an
    // unparseable one raises in Python, i.e. the same `None` outcome. A
    // present-but-undecodable value never parses either — requests would
    // hand Python a raw string that `int()` rejects.
    match headers
        .get(reqwest::header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
    {
        None if headers.contains_key(reqwest::header::CONTENT_LENGTH) => {
            tracing::warn!("avatar undecodable length; falling back to provider URL");
            return None;
        }
        None | Some("") => {}
        Some(raw) => match raw.parse::<i64>() {
            Ok(length)
                if pidash_storage::content_length_allowed(Some(length), max_bytes as i64) => {}
            _ => {
                tracing::warn!(
                    "avatar over size limit or bad length; falling back to provider URL"
                );
                return None;
            }
        },
    }
    // Raw header value, defaulting exactly like Python (`base.py:160`);
    // the exact-match allowlist (and its bug) lives in the shared helper.
    // A present-but-undecodable value misses the allowlist, like Python's
    // raw-string lookup miss — only a truly missing header defaults.
    let content_type = match headers
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
    {
        Some(value) => value.to_owned(),
        None if headers.contains_key(reqwest::header::CONTENT_TYPE) => {
            tracing::warn!("avatar undecodable content type; falling back to provider URL");
            return None;
        }
        None => "image/jpeg".to_owned(),
    };
    let extension = match pidash_storage::avatar_extension(Some(&content_type)) {
        Some(extension) => extension,
        None => {
            tracing::warn!("avatar content type not allowed; falling back to provider URL");
            return None;
        }
    };
    let mut content = Vec::new();
    loop {
        let chunk = match response.chunk().await {
            Ok(Some(chunk)) => chunk,
            Ok(None) => break,
            Err(error) => {
                tracing::warn!(%error, "avatar stream failed; falling back to provider URL");
                return None;
            }
        };
        if !pidash_storage::chunk_fits(content.len() as u64, chunk.len() as u64, max_bytes) {
            tracing::warn!("avatar over size limit; falling back to provider URL");
            return None;
        }
        content.extend_from_slice(&chunk);
    }
    let file_size = content.len() as i64;
    // `f"{uuid4hex}-user-avatar.{extension}"` (`base.py:188`): the S3 key
    // and, verbatim, the `asset` column (`objects.create` stores the raw
    // string — `upload_to` never applies).
    let filename =
        pidash_storage::avatar_object_key(&uuid::Uuid::new_v4().simple().to_string(), extension);
    if pidash_storage::validate_storage(storage).is_err() {
        tracing::warn!("avatar storage misconfigured; falling back to provider URL");
        return None;
    }
    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let date_stamp = now.format("%Y%m%d").to_string();
    let Ok(signed) = pidash_storage::sign_put(
        storage,
        scheme,
        host,
        &filename,
        &content_type,
        &content,
        &amz_date,
        &date_stamp,
    ) else {
        tracing::warn!("avatar storage endpoint invalid; falling back to provider URL");
        return None;
    };
    let put = match client
        .put(signed.url)
        .header("Authorization", signed.authorization)
        .header("x-amz-date", signed.amz_date)
        .header("x-amz-content-sha256", signed.content_sha256)
        .header(reqwest::header::CONTENT_TYPE, content_type.clone())
        .body(content)
        .send()
        .await
    {
        Ok(put) => put,
        Err(error) => {
            tracing::warn!(%error, "avatar S3 PUT failed; falling back to provider URL");
            return None;
        }
    };
    if !put.status().is_success() {
        tracing::warn!(status = %put.status(), "avatar S3 PUT rejected; falling back to provider URL");
        return None;
    }
    // `get_object_metadata` (`storage.py:156-174`): HEAD the stored key; a
    // HEAD failure records `None` (NULL metadata) while the row is still
    // created.
    let metadata = head_storage_metadata(client, storage, scheme, host, &filename, &now).await;
    let asset_id = uuid::Uuid::new_v4();
    if insert_file_asset(
        pool,
        asset_id,
        &filename,
        extension,
        &content_type,
        file_size,
        user_id,
        metadata,
        now,
    )
    .await
    .is_err()
    {
        tracing::warn!("avatar FileAsset insert failed; falling back to provider URL");
        return None;
    }
    Some(asset_id)
}

/// `S3Storage.get_object_metadata` (`storage.py:156-174`) executed over a
/// shared-signed HEAD: the response headers projected through the shared
/// mapper (`LastModified` HTTP-date → ISO-8601, `Metadata` the
/// `x-amz-meta-*` map — `{}` for avatar uploads). `None` on any failure,
/// exactly Python's logged-and-`None` contract.
pub async fn head_storage_metadata(
    client: &reqwest::Client,
    storage: &pidash_db::config::StorageSettings,
    scheme: &str,
    host: &str,
    object_key: &str,
    now: &chrono::DateTime<chrono::Utc>,
) -> Option<serde_json::Value> {
    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let date_stamp = now.format("%Y%m%d").to_string();
    let Ok(signed) =
        pidash_storage::sign_head(storage, scheme, host, object_key, &amz_date, &date_stamp)
    else {
        return None;
    };
    let head = client
        .head(signed.url)
        .header("Authorization", signed.authorization)
        .header("x-amz-date", signed.amz_date)
        .header("x-amz-content-sha256", signed.content_sha256)
        .send()
        .await
        .ok()?;
    if !head.status().is_success() {
        return None;
    }
    let headers = head.headers();
    let header_str = |name: &str| {
        headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned)
    };
    let content_length = headers
        .get(reqwest::header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<i64>().ok());
    let last_modified = header_str("last-modified")
        .and_then(|value| chrono::DateTime::parse_from_rfc2822(&value).ok())
        .map(|dt| dt.to_rfc3339());
    Some(pidash_storage::head_to_storage_metadata(
        header_str("content-type").as_deref(),
        content_length,
        last_modified.as_deref(),
        header_str("etag").as_deref(),
        None,
    ))
}

/// New `FileAsset` row (`base.py:197-206`): `attributes={name, type,
/// size}` (`{provider}-avatar.{extension}`), `asset` the bare filename,
/// `user` + `created_by` set, entity `USER_AVATAR`, uploaded flag, S3
/// head metadata (`None` → NULL when the HEAD failed, like boto3's
/// `None`). Every other column rides its Django default (the reviewed
/// column list mirrors the D-17 gitea port of the same table).
#[allow(clippy::too_many_arguments)]
pub async fn insert_file_asset(
    pool: &sqlx::PgPool,
    id: uuid::Uuid,
    stored_key: &str,
    extension: &str,
    content_type: &str,
    file_size: i64,
    user_id: uuid::Uuid,
    storage_metadata: Option<serde_json::Value>,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<(), sqlx::Error> {
    let attributes =
        pidash_storage::avatar_attributes(PROVIDER, extension, content_type, file_size);
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

/// `delete_old_avatar` (`adapter/base.py:236-254`): when the user carries
/// a live avatar asset, delete the S3 object, soft-delete the row and
/// clear both columns. `FileAsset.DoesNotExist` (no row, or a
/// soft-deleted one — the manager filters `deleted_at IS NULL`) passes
/// silently; any other failure is logged and the flow continues
/// (`except Exception: log_exception; return`) — so this returns `()`
/// always and never fails the pipeline.
///
/// Row-delete policy mirrors `delete_files` exactly: the S3 multi-delete
/// is idempotent and its `ClientError` collapses to `False`, after which
/// Python still deletes the row — so any *answered* HTTP response (even
/// an error status) soft-deletes the row + clears the FK. Only a
/// *transport* failure (Python's non-`ClientError` raise, e.g. connection
/// loss) or a misconfigured backend (Python fails lazily on first use)
/// keeps the old avatar intact. The row write is a soft delete, never a
/// hard `DELETE`: the user's FK still points at the row, so a hard
/// delete would die on the deferred FK in autocommit (PIDASHCONV-1321).
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
    let row: Option<(String,)> = match sqlx::query_as(
        "SELECT \"asset\" FROM \"file_assets\" WHERE \"id\" = $1 AND \"deleted_at\" IS NULL",
    )
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
    if pidash_storage::validate_storage(storage).is_err() {
        tracing::warn!("delete_old_avatar storage misconfigured; keeping old avatar");
        return;
    }
    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let date_stamp = now.format("%Y%m%d").to_string();
    let Ok(signed) =
        pidash_storage::sign_delete(storage, scheme, host, &key, &amz_date, &date_stamp)
    else {
        tracing::warn!("delete_old_avatar storage endpoint invalid; keeping old avatar");
        return;
    };
    match client
        .delete(signed.url)
        .header("Authorization", signed.authorization)
        .header("x-amz-date", signed.amz_date)
        .header("x-amz-content-sha256", signed.content_sha256)
        .send()
        .await
    {
        Ok(_) => {}
        Err(error) => {
            tracing::warn!(%error, "delete_old_avatar S3 delete failed; keeping old avatar");
            return;
        }
    }
    crate::auth_oauth::soft_delete_avatar_asset(pool, user_id, asset_id, now).await;
}

/// Best-effort `.delay()` through the Postgres job queue
/// (`pidash_jobs::queue::enqueue`, `handlers_webhook` precedent):
/// enqueue failures warn and the response stands — Python would raise to
/// 500 when the broker is down (see the module docs).
pub async fn enqueue_task(
    pool: &sqlx::PgPool,
    task: &str,
    args: Vec<Value>,
    kwargs: serde_json::Map<String, Value>,
) {
    let message = pidash_jobs::TaskMessage::new(task, args, kwargs);
    let job = pidash_jobs::queue::NewJob::new(
        message.task.clone(),
        Value::Array(message.args.clone()),
        Value::Object(message.kwargs.clone()),
    );
    if let Err(error) = pidash_jobs::queue::enqueue(pool, &job).await {
        tracing::warn!(%error, task = message.task.as_str(), "task enqueue failed; response stands");
    }
}

/// `save_user_data` (`adapter/base.py:220-234`): login enrichment plus the
/// activation branch. `user_activation_email.delay(site, user.id)` fires
/// while the row still shows inactive; the update then stamps the token
/// rotation (`User.save`: `token_updated_at` set ⇒ `token = 64 hex`),
/// refreshes `updated_at`, backfills a blank `display_name` from the
/// email local part, syncs `is_staff` from `is_superuser`
/// (`user.py:169-187`; the github twin is PIDASHCONV-1081), and marks the
/// user active. `ip` / `user_agent` bind NULL when absent — the
/// documented NOT NULL 500 hazard.
pub async fn save_user_data(
    pool: &sqlx::PgPool,
    user: &UserRow,
    origin_site: &str,
    ip: Option<&str>,
    user_agent: Option<&str>,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<(), CallbackError> {
    if !user.is_active {
        enqueue_task(
            pool,
            USER_ACTIVATION_EMAIL_TASK,
            vec![
                Value::String(origin_site.to_owned()),
                Value::String(user.id.to_string()),
            ],
            serde_json::Map::new(),
        )
        .await;
    }
    let token = format!("{}{}", new_state(), new_state());
    sqlx::query(
        "UPDATE \"users\" SET \"last_login_medium\" = $1, \"last_active\" = $2, \
         \"last_login_time\" = $3, \"last_login_ip\" = $4, \
         \"last_login_uagent\" = $5, \"token_updated_at\" = $6, \
         \"is_active\" = TRUE, \"token\" = $7, \"updated_at\" = $2, \
         \"display_name\" = CASE WHEN \"display_name\" = '' THEN split_part(\"email\", '@', 1) ELSE \"display_name\" END, \
         \"is_staff\" = \"is_staff\" OR \"is_superuser\" WHERE \"id\" = $8",
    )
    .bind(PROVIDER)
    .bind(now)
    .bind(now)
    .bind(ip)
    .bind(user_agent)
    .bind(now)
    .bind(token)
    .bind(user.id)
    .execute(pool)
    .await
    .map_err(|_| CallbackError::Internal)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Invitations (`post_user_auth_workflow` → `process_workspace_project_
// invitations`, `utils/workspace_project_join.py:20-91`). Runs only for
// the app callback: space providers are constructed without `callback`.
// ---------------------------------------------------------------------------

/// One accepted workspace invite with its join targets.
pub struct WorkspaceInvite {
    pub workspace_id: uuid::Uuid,
    pub role: i16,
    pub slug: String,
    pub db_workspace_id: uuid::Uuid,
}

/// One accepted project invite with its join targets.
pub struct ProjectInvite {
    pub workspace_id: uuid::Uuid,
    pub role: i16,
    pub created_by_id: Option<uuid::Uuid>,
}

/// Accepted workspace invites for the email, oldest first (queryset
/// order, `Meta.ordering = ("-created_at",)` — newest first; the port
/// keeps that order for the per-invite loop).
pub async fn fetch_workspace_invites(
    pool: &sqlx::PgPool,
    email: &str,
) -> Result<Vec<WorkspaceInvite>, CallbackError> {
    let rows: Vec<(uuid::Uuid, i16, String, uuid::Uuid)> = sqlx::query_as(
        "SELECT i.\"workspace_id\", i.\"role\", w.\"slug\", w.\"id\" \
         FROM \"workspace_member_invites\" i \
         JOIN \"workspaces\" w ON w.\"id\" = i.\"workspace_id\" \
         WHERE i.\"email\" = $1 AND i.\"accepted\" = TRUE AND i.\"deleted_at\" IS NULL \
         ORDER BY i.\"created_at\" DESC",
    )
    .bind(email)
    .fetch_all(pool)
    .await
    .map_err(|_| CallbackError::Internal)?;
    Ok(rows
        .into_iter()
        .map(
            |(workspace_id, role, slug, db_workspace_id)| WorkspaceInvite {
                workspace_id,
                role,
                slug,
                db_workspace_id,
            },
        )
        .collect())
}

/// Accepted project invites for the email, newest first.
pub async fn fetch_project_invites(
    pool: &sqlx::PgPool,
    email: &str,
) -> Result<Vec<ProjectInvite>, CallbackError> {
    let rows: Vec<(uuid::Uuid, i16, Option<uuid::Uuid>)> = sqlx::query_as(
        "SELECT \"workspace_id\", \"role\", \"created_by_id\" \
         FROM \"project_member_invites\" \
         WHERE \"email\" = $1 AND \"accepted\" = TRUE AND \"deleted_at\" IS NULL \
         ORDER BY \"created_at\" DESC",
    )
    .bind(email)
    .fetch_all(pool)
    .await
    .map_err(|_| CallbackError::Internal)?;
    Ok(rows
        .into_iter()
        .map(|(workspace_id, role, created_by_id)| ProjectInvite {
            workspace_id,
            role,
            created_by_id,
        })
        .collect())
}

/// `view_props` / `default_props` default (`workspace.py:22-60`).
pub fn default_props() -> Value {
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
            "due_date": true, "estimate": true, "key": true, "labels": true,
            "link": true, "priority": true, "start_date": true, "state": true,
            "sub_issue_count": true, "updated_on": true,
        },
    })
}

/// `get_issue_props` (`workspace.py:110-111`).
pub fn issue_props() -> Value {
    serde_json::json!({"subscribed": true, "assigned": true, "created": true, "all_issues": true})
}

/// `get_default_preferences` (`project.py:68-69`).
pub fn default_preferences() -> Value {
    serde_json::json!({"pages": {"block_display": true}, "navigation": {"default_tab": "work_items", "hide_in_more_menu": []}})
}

/// `WorkspaceMember.objects.bulk_create(..., ignore_conflicts=True)`
/// (`workspace_project_join.py:26-36,62-73`): one multi-row `INSERT`
/// with `ON CONFLICT DO NOTHING`, full audit + default columns (the
/// current user is anonymous here, so `created_by`/`updated_by` are
/// `None`; the project-invite arm carries the invite's `created_by_id`).
pub async fn insert_workspace_members(
    pool: &sqlx::PgPool,
    rows: &[(uuid::Uuid, uuid::Uuid, i16, Option<uuid::Uuid>)],
    now: chrono::DateTime<chrono::Utc>,
) -> Result<(), CallbackError> {
    if rows.is_empty() {
        return Ok(());
    }
    let props = default_props();
    let issues = issue_props();
    let empty = serde_json::json!({});
    let mut builder = sqlx::QueryBuilder::new(
        "INSERT INTO \"workspace_members\" (\"id\", \"created_at\", \
         \"updated_at\", \"created_by_id\", \"updated_by_id\", \
         \"deleted_at\", \"workspace_id\", \"member_id\", \"role\", \
         \"company_role\", \"view_props\", \"default_props\", \
         \"issue_props\", \"is_active\", \"getting_started_checklist\", \
         \"tips\", \"explored_features\") ",
    );
    builder.push_values(rows, |mut bind, row| {
        let (workspace_id, member_id, role, created_by) = row;
        bind.push_bind(uuid::Uuid::new_v4())
            .push_bind(now)
            .push_bind(now)
            .push_bind(*created_by)
            .push_bind(None::<uuid::Uuid>)
            .push_bind(None::<chrono::DateTime<chrono::Utc>>)
            .push_bind(*workspace_id)
            .push_bind(*member_id)
            .push_bind(*role)
            .push_bind(None::<String>)
            .push_bind(props.clone())
            .push_bind(props.clone())
            .push_bind(issues.clone())
            .push_bind(true)
            .push_bind(empty.clone())
            .push_bind(empty.clone())
            .push_bind(empty.clone());
    });
    builder.push(" ON CONFLICT DO NOTHING");
    builder
        .build()
        .execute(pool)
        .await
        .map_err(|_| CallbackError::Internal)?;
    Ok(())
}

/// `ProjectMember.objects.bulk_create(..., ignore_conflicts=True)` as
/// written (`workspace_project_join.py:76-87`): `project_id` is missing
/// although the column is NOT NULL, so any accepted project invite raises
/// `IntegrityError` even with `ON CONFLICT DO NOTHING` — reproduced
/// as-is (`BUG-PROJECT-INVITE-COLUMNS`).
pub async fn insert_project_members(
    pool: &sqlx::PgPool,
    rows: &[(uuid::Uuid, uuid::Uuid, i16, Option<uuid::Uuid>)],
    now: chrono::DateTime<chrono::Utc>,
) -> Result<(), CallbackError> {
    if rows.is_empty() {
        return Ok(());
    }
    let props = default_props();
    let prefs = default_preferences();
    let mut builder = sqlx::QueryBuilder::new(
        "INSERT INTO \"project_members\" (\"id\", \"created_at\", \
         \"updated_at\", \"created_by_id\", \"updated_by_id\", \
         \"deleted_at\", \"workspace_id\", \"member_id\", \"comment\", \
         \"role\", \"view_props\", \"default_props\", \"preferences\", \
         \"sort_order\", \"is_active\") ",
    );
    builder.push_values(rows, |mut bind, row| {
        let (workspace_id, member_id, role, created_by) = row;
        bind.push_bind(uuid::Uuid::new_v4())
            .push_bind(now)
            .push_bind(now)
            .push_bind(*created_by)
            .push_bind(None::<uuid::Uuid>)
            .push_bind(None::<chrono::DateTime<chrono::Utc>>)
            .push_bind(*workspace_id)
            .push_bind(*member_id)
            .push_bind(None::<String>)
            .push_bind(*role)
            .push_bind(props.clone())
            .push_bind(props.clone())
            .push_bind(prefs.clone())
            .push_bind(65535.0f64)
            .push_bind(true);
    });
    builder.push(" ON CONFLICT DO NOTHING");
    builder
        .build()
        .execute(pool)
        .await
        .map_err(|_| CallbackError::Internal)?;
    Ok(())
}

/// `role if role in [5, 15] else 15` (`workspace_project_join.py:66,80`,
/// D-16 `map_invite_role`).
pub fn map_invite_role(role: i16) -> i16 {
    if role == 5 || role == 15 {
        role
    } else {
        15
    }
}

/// `invalidate_cache_directly(path, url_params=False, user=False,
/// multiple=True)` with `request=None` (`utils/cache.py:54-75` +
/// `workspace_project_join.py:39-44`): `user=False` forces
/// `auth_header=None`, so `generate_cache_key` returns the bare path and
/// `multiple=True` deletes every key matching `*{path}*` (`KEYS` + `DEL`).
/// Any Redis failure raises in Python, so it surfaces as a 500 here too.
pub fn workspace_members_cache_pattern(workspace_slug: &str) -> String {
    format!("*/api/workspaces/{workspace_slug}/members/*")
}

/// `process_workspace_project_invitations`
/// (`workspace_project_join.py:20-91`): workspace joins (membership +
/// per-invite cache invalidation + `track_event`; invalidation runs
/// before the event publish, `join.py:39-45` order),
/// project-invite workspace joins (mapped role), the buggy project joins,
/// then both invite soft-deletes.
pub async fn process_invitations(
    pool: &sqlx::PgPool,
    redis: Option<&RedisHandle>,
    user_id: uuid::Uuid,
    email: &str,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<(), CallbackError> {
    let workspace_invites = fetch_workspace_invites(pool, email).await?;
    let workspace_rows: Vec<(uuid::Uuid, uuid::Uuid, i16, Option<uuid::Uuid>)> = workspace_invites
        .iter()
        .map(|invite| (invite.workspace_id, user_id, invite.role, None))
        .collect();
    insert_workspace_members(pool, &workspace_rows, now).await?;
    for invite in &workspace_invites {
        // `invalidate_cache_directly(path=f"/api/workspaces/{slug}/members/",
        // user=False, multiple=True)` — a missing handle 500s, like Python
        // with its cache backend down.
        let handle = redis.ok_or(CallbackError::Internal)?;
        handle
            .invalidate_matching(&workspace_members_cache_pattern(&invite.slug))
            .await
            .map_err(|_| CallbackError::Internal)?;
        let mut properties = serde_json::Map::new();
        properties.insert("user_id".to_owned(), Value::String(user_id.to_string()));
        properties.insert(
            "workspace_id".to_owned(),
            Value::String(invite.db_workspace_id.to_string()),
        );
        properties.insert(
            "workspace_slug".to_owned(),
            Value::String(invite.slug.clone()),
        );
        properties.insert("role".to_owned(), Value::from(invite.role));
        properties.insert(
            "joined_at".to_owned(),
            Value::String(now.to_rfc3339_opts(chrono::SecondsFormat::Micros, false)),
        );
        let mut kwargs = serde_json::Map::new();
        kwargs.insert("user_id".to_owned(), Value::String(user_id.to_string()));
        kwargs.insert(
            "event_name".to_owned(),
            Value::String(USER_JOINED_WORKSPACE_EVENT.to_owned()),
        );
        kwargs.insert("slug".to_owned(), Value::String(invite.slug.clone()));
        kwargs.insert("event_properties".to_owned(), Value::Object(properties));
        enqueue_task(pool, TRACK_EVENT_TASK, Vec::new(), kwargs).await;
    }
    let project_invites = fetch_project_invites(pool, email).await?;
    let project_rows: Vec<(uuid::Uuid, uuid::Uuid, i16, Option<uuid::Uuid>)> = project_invites
        .iter()
        .map(|invite| {
            (
                invite.workspace_id,
                user_id,
                map_invite_role(invite.role),
                invite.created_by_id,
            )
        })
        .collect();
    insert_workspace_members(pool, &project_rows, now).await?;
    insert_project_members(pool, &project_rows, now).await?;
    // Both invite sets soft-delete (queryset `.delete()` → `UPDATE
    // deleted_at`, `mixins.py:SoftDeletionQuerySet`).
    sqlx::query(
        r#"UPDATE "workspace_member_invites" SET "deleted_at" = $1
           WHERE "email" = $2 AND "accepted" = TRUE AND "deleted_at" IS NULL"#,
    )
    .bind(now)
    .bind(email)
    .execute(pool)
    .await
    .map_err(|_| CallbackError::Internal)?;
    sqlx::query(
        r#"UPDATE "project_member_invites" SET "deleted_at" = $1
           WHERE "email" = $2 AND "accepted" = TRUE AND "deleted_at" IS NULL"#,
    )
    .bind(now)
    .bind(email)
    .execute(pool)
    .await
    .map_err(|_| CallbackError::Internal)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Account upsert, session login, redirection, callback handlers.
// ---------------------------------------------------------------------------

/// `create_update_account` (`adapter/oauth.py:105-136`, PIDASHCONV-327):
/// the looked-up row's `created_at`/`metadata` ride into the update side
/// (Django's `.save()` rewrites them unchanged); absent, the ported
/// defaults apply. Database failures are swallowed inside (BUG-6).
pub async fn upsert_account(
    pool: &sqlx::PgPool,
    user_id: uuid::Uuid,
    user_data: &Value,
    tokens: &pidash_db::auth_oauth::queries::account::TokenFields,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<(), CallbackError> {
    use pidash_db::auth_oauth::queries::account::{create_update_account, ExistingAccount};
    let provider_id =
        pidash_db::auth_oauth::queries::account::lookup_provider_id(user_data).unwrap_or("");
    let prior: Option<(chrono::DateTime<chrono::Utc>, Value)> = if provider_id.is_empty() {
        None
    } else {
        sqlx::query_as(
            "SELECT \"created_at\", \"metadata\" FROM \"accounts\" \
             WHERE \"user_id\" = $1 AND \"provider\" = $2 AND \"provider_account_id\" = $3 LIMIT 1",
        )
        .bind(user_id)
        .bind(PROVIDER)
        .bind(provider_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| CallbackError::Internal)?
    };
    let mut conn = pool.acquire().await.map_err(|_| CallbackError::Internal)?;
    let existing = prior.map(|(created_at, metadata)| ExistingAccount {
        created_at,
        metadata,
    });
    create_update_account(
        &mut conn,
        user_id,
        PROVIDER,
        user_data,
        tokens,
        now,
        uuid::Uuid::new_v4(),
        existing,
    )
    .await
    .map_err(|_| CallbackError::Internal)?;
    Ok(())
}

/// `django.contrib.auth.login` + `user_login`
/// (`authentication/utils/login.py:14-29`): the shared `login()` half
/// first (rotation, the `_auth_*` triple, the `update_last_login` signal
/// write, the CSRF cookie — see
/// [`super::login::login_session_keys`]), then `device_info`. Returns the
/// rotated `csrftoken` `Set-Cookie` value for the success redirect. The
/// middleware persists + issues the `session-id` cookie on success; a 500
/// answer never persists (status gate).
pub async fn login_user(
    ctx: &RequestContext<'_>,
    settings: &pidash_db::config::Settings,
    user: &UserRow,
    secret: &[u8],
    domain: &str,
) -> Result<String, CallbackError> {
    let csrf_cookie = super::login::login_session_keys(
        &ctx.session,
        &ctx.pool,
        user.id,
        &user.password,
        secret,
        settings,
        chrono::Utc::now(),
    )
    .await
    .map_err(|_| CallbackError::Internal)?;
    let mut info = serde_json::Map::new();
    info.insert(
        "user_agent".to_owned(),
        Value::String(ctx.device_user_agent().to_owned()),
    );
    info.insert(
        "ip_address".to_owned(),
        ctx.ip.clone().map(Value::String).unwrap_or(Value::Null),
    );
    info.insert("domain".to_owned(), Value::String(domain.to_owned()));
    ctx.session
        .lock()
        .set("device_info".to_owned(), Value::Object(info));
    Ok(csrf_cookie)
}

/// `get_redirection_path` (`utils/redirection_path.py:8-46`): profile
/// (created when missing), active last workspace, earliest active
/// membership, unaccepted-invite check, else create-workspace. Branch
/// order via the shared [`select_redirection_path`] kernel.
pub async fn redirection_path(
    pool: &sqlx::PgPool,
    user_id: uuid::Uuid,
    email: &str,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<String, CallbackError> {
    let profile = get_or_create_profile(pool, user_id, now).await?;
    if !profile.is_onboarded {
        return Ok("onboarding".to_owned());
    }
    let mut last_slug: Option<String> = None;
    if let Some(last_id) = profile.last_workspace_id {
        last_slug = sqlx::query_scalar(
            "SELECT w.\"slug\" FROM \"workspaces\" w \
             JOIN \"workspace_members\" m ON m.\"workspace_id\" = w.\"id\" \
             WHERE w.\"id\" = $1 AND m.\"member_id\" = $2 AND m.\"is_active\" = TRUE \
             AND m.\"deleted_at\" IS NULL AND w.\"deleted_at\" IS NULL LIMIT 1",
        )
        .bind(last_id)
        .bind(user_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| CallbackError::Internal)?;
    }
    let fallback_slug: Option<String> = sqlx::query_scalar(
        "SELECT w.\"slug\" FROM \"workspaces\" w \
         JOIN \"workspace_members\" m ON m.\"workspace_id\" = w.\"id\" \
         WHERE m.\"member_id\" = $1 AND m.\"is_active\" = TRUE \
         AND m.\"deleted_at\" IS NULL AND w.\"deleted_at\" IS NULL \
         ORDER BY w.\"created_at\" ASC LIMIT 1",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| CallbackError::Internal)?;
    let invited: Option<(i32,)> = sqlx::query_as(
        "SELECT 1 FROM \"workspace_member_invites\" WHERE \"email\" = $1 AND \"deleted_at\" IS NULL LIMIT 1",
    )
    .bind(email)
    .fetch_optional(pool)
    .await
    .map_err(|_| CallbackError::Internal)?;
    let target = select_redirection_path(
        profile.is_onboarded,
        last_slug.as_deref(),
        fallback_slug.as_deref(),
        invited.is_some(),
    );
    Ok(redirection_path_str(&target).to_owned())
}

/// The authenticated user: `provider.authenticate()`
/// (`adapter/oauth.py:70-73`) — exchange, user mapping, provisioning,
/// account upsert — minus the view-level login/redirect.
pub struct AuthedUser {
    pub row: UserRow,
    pub email: String,
}

/// Full success pipeline for one `code`: config (5105), token exchange +
/// userinfo (5115), sanitize (5005/5035), lookup, signup (profile +
/// avatar upload attempt with provider-URL fallback, then IDP sync for
/// the new user when enabled — `not is_signup`, never for existing
/// users), stamps, post-auth workflow on the app side (`with_callback`),
/// account upsert. `file_size_max` is `settings.file_size_limit.max(0)`
/// for the avatar size guards.
pub async fn authenticate_google(
    ctx: &RequestContext<'_>,
    secret: &str,
    code: &str,
    with_callback: bool,
    file_size_max: u64,
) -> Result<AuthedUser, CallbackError> {
    let now = chrono::Utc::now();
    let (client_id, client_secret) = google_config(&ctx.pool, secret).await?;
    let callback_uri = redirect_uri(ctx.secure, &ctx.host, PROVIDER);
    let client = reqwest::Client::builder()
        .build()
        .map_err(|_| CallbackError::Internal)?;
    // `request.scheme` for the server-side S3 calls (`S3Storage(request)`,
    // `base.py:198` — `is_server=False` signs the public host).
    let scheme = if ctx.secure { "https" } else { "http" };
    let token_response =
        fetch_user_token(&client, code, &client_id, &client_secret, &callback_uri).await?;
    let tokens = token_fields(&token_response)?;
    // `f"Bearer {self.token_data.get('access_token')}"`: a missing token
    // renders `None`, not an empty string.
    let bearer = tokens
        .access_token
        .clone()
        .unwrap_or_else(|| "None".to_owned());
    let userinfo = fetch_user_response(&client, GOOGLE_USERINFO_URL, &bearer).await?;
    let user_data = user_payload(&userinfo);
    let email = sanitize_email(user_data.get("email").unwrap_or(&Value::Null))?;
    let inner = user_data.get("user").unwrap_or(&Value::Null);
    let first_name = inner
        .get("first_name")
        .and_then(Value::as_str)
        .unwrap_or("");
    let last_name = inner.get("last_name").and_then(Value::as_str).unwrap_or("");
    let avatar_url = inner.get("avatar").and_then(Value::as_str).unwrap_or("");
    let existing = find_user_by_email(&ctx.pool, &email).await?;
    // `is_signup = bool(user)` — inverted as written (`BUG-SIGNUP-FLAG`):
    // true when the user already exists. The sync gate consumes the
    // inversion (`check_sync_enabled() and not is_signup`,
    // `adapter/base.py:353-354`), so IDP sync runs for newly created
    // users only, never for existing ones.
    let is_signup = existing.is_some();
    let user = match existing {
        Some(row) => row,
        None => {
            check_signup(&ctx.pool, secret, &email).await?;
            let mut row = create_user(&ctx.pool, &email, first_name, last_name, now).await?;
            if !avatar_url.is_empty() {
                sqlx::query("UPDATE \"users\" SET \"avatar\" = $1 WHERE \"id\" = $2")
                    .bind(avatar_url)
                    .bind(row.id)
                    .execute(&ctx.pool)
                    .await
                    .map_err(|_| CallbackError::Internal)?;
                row.avatar = avatar_url.to_owned();
                // `avatar_asset = download_and_upload_avatar(...)`;
                // success sets the FK too, failure keeps the provider URL
                // (`base.py:330-345`). No old avatar exists to delete.
                if let Some(asset_id) = download_and_upload_avatar(
                    &client,
                    &ctx.pool,
                    &ctx.storage,
                    scheme,
                    &ctx.host,
                    avatar_url,
                    row.id,
                    file_size_max,
                    now,
                )
                .await
                {
                    sqlx::query("UPDATE \"users\" SET \"avatar_asset_id\" = $1 WHERE \"id\" = $2")
                        .bind(asset_id)
                        .bind(row.id)
                        .execute(&ctx.pool)
                        .await
                        .map_err(|_| CallbackError::Internal)?;
                    row.avatar_asset_id = Some(asset_id);
                }
            }
            get_or_create_profile(&ctx.pool, row.id, now).await?;
            if !is_signup && google_sync_enabled(&ctx.pool, secret).await? {
                sync_user_data(
                    &client,
                    &ctx.pool,
                    &ctx.storage,
                    scheme,
                    &ctx.host,
                    &row,
                    &email,
                    first_name,
                    last_name,
                    avatar_url,
                    file_size_max,
                    now,
                )
                .await?;
            }
            row
        }
    };
    let origin = base_host(&ctx.settings_view, false, false, false);
    save_user_data(
        &ctx.pool,
        &user,
        &origin,
        ctx.ip.as_deref(),
        ctx.user_agent.as_deref(),
        now,
    )
    .await?;
    if with_callback {
        process_invitations(&ctx.pool, ctx.redis.as_ref(), user.id, &email, now).await?;
    }
    upsert_account(&ctx.pool, user.id, &user_data, &tokens, now).await?;
    Ok(AuthedUser { row: user, email })
}

/// `GOOGLE_OAUTH_PROVIDER_ERROR` constructor (5115, `error.py:48`).
pub fn provider_error() -> AuthenticationException {
    AuthenticationException::new(
        GOOGLE_PROVIDER_ERROR_CODE,
        "GOOGLE_OAUTH_PROVIDER_ERROR",
        Vec::new(),
    )
}

/// `GoogleCallbackEndpoint.get` (`views/app/google.py:62-104`).
pub async fn app_callback(
    State(state): State<AppState>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
    session: Option<Extension<SessionHandle>>,
) -> Response {
    let ctx = match RequestContext::build(&state, query, headers, session) {
        Ok(ctx) => ctx.finish_scheme(state.settings()),
        Err(response) => return response,
    };
    // `code = request.GET.get("code")`, `state = request.GET.get("state")`,
    // `next_path = request.session.get("next_path")`
    let code = query_get(&ctx.query, "code");
    let get_state = query_get(&ctx.query, "state");
    let session_state = session_get(&ctx.session, "state");
    let session_next = session_get(&ctx.session, "next_path");
    // `if state != request.session.get("state", ""):`
    if !states_match(get_state.as_deref(), session_state.as_deref()) {
        let exception = provider_error();
        let url = error_redirect(
            &ctx.app_base,
            session_next.as_deref(),
            &exception,
            &ctx.settings_view,
        );
        return redirect_response(&url);
    }
    // `if not code:`
    let code = match code {
        Some(code) if !code.is_empty() => code,
        _ => {
            let exception = provider_error();
            let url = error_redirect(
                &ctx.app_base,
                session_next.as_deref(),
                &exception,
                &ctx.settings_view,
            );
            return redirect_response(&url);
        }
    };
    let secret = state.settings().secret_key.clone();
    let file_size_max = state.settings().file_size_limit.max(0) as u64;
    match authenticate_google(&ctx, &secret, &code, true, file_size_max).await {
        Err(CallbackError::Auth(exception)) => {
            let url = error_redirect(
                &ctx.app_base,
                session_next.as_deref(),
                &exception,
                &ctx.settings_view,
            );
            redirect_response(&url)
        }
        Err(CallbackError::Internal) => server_error(),
        Ok(authed) => {
            let csrf_cookie = match login_user(
                &ctx,
                state.settings(),
                &authed.row,
                secret.as_bytes(),
                &ctx.app_base,
            )
            .await
            {
                Ok(cookie) => cookie,
                Err(_) => return server_error(),
            };
            let path = match session_next {
                Some(next) if !next.is_empty() => next,
                _ => match redirection_path(
                    &ctx.pool,
                    authed.row.id,
                    &authed.email,
                    chrono::Utc::now(),
                )
                .await
                {
                    Ok(path) => path,
                    Err(_) => return server_error(),
                },
            };
            let url = get_safe_redirect_url(
                &ctx.app_base,
                &path,
                &[],
                &allowed_hosts(&ctx.settings_view),
            );
            let mut response = redirect_response(&url);
            super::login::push_cookie(&mut response, csrf_cookie);
            response
        }
    }
}

/// `GoogleCallbackSpaceEndpoint.get` (`views/space/google.py:58-102`).
///
/// `BUG-SPACE-SHADOW`: `base_host = request.session.get("host")` binds
/// the name for the whole function body, so every `base_host(request=...)`
/// call below raises `TypeError` and Django answers 500 on every input.
/// The port reads the same session keys, runs the same side-effecting
/// pipeline on the valid path (provider constructed *without* `callback`,
/// like Python), then answers 500 instead of building the shadowed URL.
pub async fn space_callback(
    State(state): State<AppState>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
    session: Option<Extension<SessionHandle>>,
) -> Response {
    let ctx = match RequestContext::build(&state, query, headers, session) {
        Ok(ctx) => ctx.finish_scheme(state.settings()),
        Err(response) => return response,
    };
    // `base_host = request.session.get("host")` — the shadowing bind. The
    // value is never callable; every branch below ends in 500.
    let _shadowed_host = session_get(&ctx.session, "host");
    let _shadowed_next = session_get(&ctx.session, "next_path");
    let code = query_get(&ctx.query, "code");
    let get_state = query_get(&ctx.query, "state");
    let session_state = session_get(&ctx.session, "state");
    if !states_match(get_state.as_deref(), session_state.as_deref()) {
        return server_error();
    }
    let code = match code {
        Some(code) if !code.is_empty() => code,
        _ => return server_error(),
    };
    let secret = state.settings().secret_key.clone();
    // Valid path: `authenticate()` + `user_login()` side effects run
    // first (Python `:85-90`), then the f-string `base_host(...)` raises.
    let file_size_max = state.settings().file_size_limit.max(0) as u64;
    if let Ok(authed) = authenticate_google(&ctx, &secret, &code, false, file_size_max).await {
        let _ = login_user(
            &ctx,
            state.settings(),
            &authed.row,
            secret.as_bytes(),
            &ctx.space_base,
        )
        .await;
    }
    server_error()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlong_redirect_location_answers_400() {
        let long = format!("http://app.test/?{}", "a".repeat(17_000));
        assert_eq!(redirect_response(&long).status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            redirect_response("http://app.test/?next_path=/x").status(),
            StatusCode::FOUND
        );
    }

    fn fixtures_dir() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/auth_oauth")
    }

    fn fixture(name: &str) -> Value {
        let body = std::fs::read_to_string(fixtures_dir().join(name))
            .unwrap_or_else(|_| panic!("read {name}"));
        serde_json::from_str(&body).unwrap_or_else(|_| panic!("{name} is valid JSON"))
    }

    fn test_settings() -> HostSettings<'static> {
        HostSettings {
            web_url: None,
            app_base_url: Some("http://app.example.com"),
            admin_base_url: None,
            // `SPACE_BASE_URL` is the origin; `base_host` appends the
            // space path (`host.py:44-55`).
            space_base_url: Some("http://app.example.com"),
            admin_base_path: None,
            space_base_path: None,
        }
    }

    fn app_base(settings: &HostSettings<'_>) -> String {
        base_host(settings, false, false, true)
    }

    fn space_base(settings: &HostSettings<'_>) -> String {
        base_host(settings, false, true, false)
    }

    // -- F10: initiate -----------------------------------------------------

    #[test]
    fn f10_app_google_not_configured_redirect() {
        // `F10_initiate app_google_github_gitlab.golden_not_configured`:
        // session host + raw next_path stored, 302 to the app base with
        // `next_path` first, then the 5000 pair.
        let fixture = fixture("F10_initiate.golden.json");
        let golden = &fixture["app_google_github_gitlab"]["golden_not_configured"];
        assert_eq!(
            golden["out"]["status"].as_u64().unwrap(),
            302,
            "F10 app initiate is a 302"
        );
        let settings = test_settings();
        let base = app_base(&settings);
        assert_eq!(base, "http://app.example.com");
        let next_path = golden["in"]["GET_next_path"].as_str();
        // Session rule: raw store when truthy.
        assert_eq!(
            app_initiate_session_next(next_path),
            Some("/x".to_owned()),
            "F10 app google stores next_path raw"
        );
        let exception = AuthenticationException::new(
            INSTANCE_NOT_CONFIGURED_CODE,
            "INSTANCE_NOT_CONFIGURED",
            Vec::new(),
        );
        let url = error_redirect(&base, next_path, &exception, &settings);
        assert_eq!(
            url,
            "http://app.example.com/?next_path=/x&error_code=5000&error_message=INSTANCE_NOT_CONFIGURED",
            "F10 app google not-configured Location"
        );
    }

    #[test]
    fn f10_app_google_provider_error_redirect() {
        // `F10_initiate app_google_github_gitlab.golden_provider_error`:
        // unconfigured provider (5105) with no next_path.
        let settings = test_settings();
        let base = app_base(&settings);
        let exception = AuthenticationException::new(
            GOOGLE_NOT_CONFIGURED_CODE,
            "GOOGLE_NOT_CONFIGURED",
            Vec::new(),
        );
        let url = error_redirect(&base, None, &exception, &settings);
        assert_eq!(
            url, "http://app.example.com/?error_code=5105&error_message=GOOGLE_NOT_CONFIGURED",
            "F10 app google provider-error Location"
        );
    }

    #[test]
    fn f10_space_google_never_stores_next_path() {
        // `F10_initiate space_google_github_gitlab`: no
        // `session["next_path"]` write exists in `views/space/google.py`;
        // the error redirect still echoes the GET value, and the base
        // carries `/spaces/`.
        let fixture = fixture("F10_initiate.golden.json");
        let golden = &fixture["space_google_github_gitlab"]["golden_not_configured"];
        assert_eq!(
            golden["out"]["session_next_path"].as_str().unwrap(),
            "ABSENT (never stored)"
        );
        let settings = test_settings();
        let base = space_base(&settings);
        assert_eq!(base, "http://app.example.com/spaces/");
        let exception = AuthenticationException::new(
            INSTANCE_NOT_CONFIGURED_CODE,
            "INSTANCE_NOT_CONFIGURED",
            Vec::new(),
        );
        let url = error_redirect(&base, Some("/x"), &exception, &settings);
        assert_eq!(
            url,
            "http://app.example.com/spaces/?next_path=/x&error_code=5000&error_message=INSTANCE_NOT_CONFIGURED",
            "F10 space google not-configured Location"
        );
    }

    #[test]
    fn initiate_next_path_session_rule() {
        // Raw, unvalidated store on the app side (gitea validates; google
        // does not — `views/app/google.py:30-32` vs `gitea.py`).
        assert_eq!(app_initiate_session_next(None), None);
        assert_eq!(app_initiate_session_next(Some("")), None);
        assert_eq!(app_initiate_session_next(Some("/x")), Some("/x".to_owned()));
        assert_eq!(
            app_initiate_session_next(Some("https://evil.example/x")),
            Some("https://evil.example/x".to_owned()),
            "stored raw; the redirect kernel validates, the session does not"
        );
    }

    // -- F11: callback -----------------------------------------------------

    #[test]
    fn f11_app_google_state_mismatch_redirect() {
        // `F11_callback app_google_github_gitlab.golden_state_mismatch`:
        // 5115, no next_path echo (the session carried none).
        let settings = test_settings();
        let base = app_base(&settings);
        assert!(!states_match(Some("WRONG"), Some("S")));
        assert!(!states_match(None, None), "fresh session always mismatches");
        assert!(states_match(Some("S"), Some("S")));
        assert!(
            states_match(Some(""), None),
            "empty state equals the default"
        );
        let url = error_redirect(&base, None, &provider_error(), &settings);
        assert_eq!(
            url,
            "http://app.example.com/?error_code=5115&error_message=GOOGLE_OAUTH_PROVIDER_ERROR",
            "F11 app google state-mismatch Location"
        );
    }

    #[test]
    fn f11_app_google_missing_code_redirect() {
        // `F11_callback app_google_github_gitlab.golden_missing_code`:
        // matching state, no code — 5115 with the session next_path.
        let settings = test_settings();
        let base = app_base(&settings);
        let url = error_redirect(&base, Some("/x"), &provider_error(), &settings);
        assert_eq!(
            url,
            "http://app.example.com/?next_path=/x&error_code=5115&error_message=GOOGLE_OAUTH_PROVIDER_ERROR",
            "F11 app google missing-code Location"
        );
    }

    #[test]
    fn f11_space_google_is_500_everywhere() {
        // `F11_callback space_google_github_gitlab_PORT_BUG`: the
        // shadowed `base_host` makes every branch a TypeError, so the
        // handler answers 500 with a code, without one, with or without
        // a session. The response shape is asserted here; the branch
        // coverage (mismatch / missing / valid) is the contract suite's
        // `SPACE_500_CALLBACKS` table.
        let response = server_error();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let redirect = redirect_response("http://app.example.com/?error_code=5115");
        assert_eq!(redirect.status(), StatusCode::FOUND);
        assert_eq!(
            redirect.headers()[header::LOCATION],
            "http://app.example.com/?error_code=5115"
        );
    }

    // -- pure helpers ------------------------------------------------------

    #[test]
    fn sanitize_email_cases() {
        assert_eq!(
            sanitize_email(&Value::String("User@Example.COM ".to_owned())).unwrap(),
            "user@example.com"
        );
        let err = sanitize_email(&Value::Null).unwrap_err();
        assert_eq!(err.error_code, INVALID_EMAIL_CODE);
        assert_eq!(
            err.payload,
            vec![("email".to_owned(), Value::Null)],
            "raw value rides as payload"
        );
        let err = sanitize_email(&Value::String("not-an-email".to_owned())).unwrap_err();
        assert_eq!(err.error_code, INVALID_EMAIL_CODE);
        assert_eq!(
            err.payload,
            vec![("email".to_owned(), Value::String("not-an-email".to_owned()))]
        );
        // Payload ordering: code, message, then email.
        let params = error_params(&err);
        let keys: Vec<&str> = params.iter().map(|(key, _)| *key).collect();
        assert_eq!(keys, vec!["error_code", "error_message", "email"]);
    }

    #[test]
    fn expires_at_cases() {
        assert_eq!(expires_at(&Value::Null).unwrap(), None);
        assert_eq!(
            expires_at(&serde_json::json!(0)).unwrap(),
            None,
            "falsy epoch maps to None"
        );
        let at = expires_at(&serde_json::json!(3600)).unwrap().unwrap();
        assert_eq!(
            at,
            chrono::DateTime::from_timestamp(3600, 0).unwrap(),
            "absolute epoch basis like fromtimestamp"
        );
        assert!(expires_at(&serde_json::json!("soon")).is_err());
    }

    #[test]
    fn display_name_rules() {
        assert_eq!(backfill_display_name("ada@example.com"), "ada");
        assert_eq!(get_display_name("ada@example.com"), "ada");
        assert_eq!(get_display_name("").len(), 6);
        assert_eq!(map_invite_role(5), 5);
        assert_eq!(map_invite_role(15), 15);
        assert_eq!(map_invite_role(20), 15);
    }

    #[test]
    fn workspace_members_cache_pattern_matches_python() {
        // `workspace_project_join.py:39-44`: `path` is the bare
        // `/api/workspaces/{slug}/members/` (`user=False`, `request=None`)
        // and `multiple=True` wraps it as `*{key}*` (`cache.py:66-67`).
        assert_eq!(
            workspace_members_cache_pattern("acme"),
            "*/api/workspaces/acme/members/*"
        );
        assert_eq!(
            workspace_members_cache_pattern("ai-republic"),
            "*/api/workspaces/ai-republic/members/*"
        );
    }

    // -- live kernel tests (env-gated, PIDASHCONV-1089) ------------------
    // Same convention as the github kernel harness (PIDASHCONV-1083):
    // unset DATABASE_URL (plain `cargo test`) skips these. REDIS_URL
    // defaults to the shared local server's db 6 scratch lane. The HTTP
    // success path is sealed in tests (the exchange posts to hardcoded
    // https://oauth2.googleapis.com URLs on both backends), so these
    // drive `process_invitations` directly — the kernel-level harness.

    /// Serializes the queue DDL: parallel tests racing
    /// `CREATE TABLE IF NOT EXISTS` can collide on the sequence.
    static SCHEMA_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    async fn kernel_pool() -> Option<sqlx::PgPool> {
        match std::env::var("DATABASE_URL") {
            Ok(url) => {
                let pool = sqlx::PgPool::connect(&url)
                    .await
                    .expect("connect to scratch DATABASE_URL");
                let _guard = SCHEMA_LOCK.lock().await;
                pidash_jobs::queue::ensure_schema(&pool)
                    .await
                    .expect("queue schema");
                Some(pool)
            }
            Err(_) => {
                eprintln!("skipping live-kernel test: DATABASE_URL is not set");
                None
            }
        }
    }

    fn kernel_redis_url() -> String {
        std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379/6".to_owned())
    }

    /// Seed an active user + workspace + accepted invite for the email,
    /// plus an unaccepted decoy in a second workspace (live invites are
    /// unique per (email, workspace); column lists mirror
    /// `_harness.db.Database.make_user/make_workspace/make_invite`).
    async fn seed_invited_user(
        pool: &sqlx::PgPool,
        email: &str,
        slug: &str,
    ) -> (uuid::Uuid, uuid::Uuid, uuid::Uuid) {
        let user_id = uuid::Uuid::new_v4();
        let workspace_id = uuid::Uuid::new_v4();
        let decoy_workspace_id = uuid::Uuid::new_v4();
        let decoy_slug = format!("{slug}-u");
        let now = chrono::Utc::now();
        sqlx::query(
            r#"INSERT INTO "users" ("password", "id", "username", "email", "first_name", "last_name",
                "avatar", "date_joined", "created_at", "updated_at", "last_location",
                "created_location", "is_superuser", "is_managed", "is_password_expired",
                "is_active", "is_staff", "is_email_verified", "is_password_autoset",
                "token", "user_timezone", "last_login_ip", "last_logout_ip",
                "last_login_medium", "last_login_uagent", "is_bot", "display_name",
                "is_email_valid", "is_password_reset_required")
               VALUES ('pbkdf2_sha256$600000$kernelseed$kernelhash', $1, $2, $3, 'Kernel', 'User',
                '', $4, $4, $4, '', '', FALSE, FALSE, FALSE, TRUE, FALSE, TRUE, FALSE,
                '0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef',
                'UTC', '', '', 'email', '', FALSE, 'Kernel User', TRUE, FALSE)"#,
        )
        .bind(user_id)
        .bind(format!("u-{}", &user_id.simple().to_string()[..8]))
        .bind(email)
        .bind(now)
        .execute(pool)
        .await
        .expect("seed user");
        sqlx::query(
            r#"INSERT INTO "workspaces" ("created_at", "updated_at", "id", "name", "slug",
                "owner_id", "timezone", "background_color")
               VALUES ($1, $1, $2, $3, $4, $5, 'UTC', '#ffffff')"#,
        )
        .bind(now)
        .bind(workspace_id)
        .bind(format!("kernel ws {slug}"))
        .bind(slug)
        .bind(user_id)
        .execute(pool)
        .await
        .expect("seed workspace");
        sqlx::query(
            r#"INSERT INTO "workspaces" ("created_at", "updated_at", "id", "name", "slug",
                "owner_id", "timezone", "background_color")
               VALUES ($1, $1, $2, $3, $4, $5, 'UTC', '#ffffff')"#,
        )
        .bind(now)
        .bind(decoy_workspace_id)
        .bind(format!("kernel ws {decoy_slug}"))
        .bind(&decoy_slug)
        .bind(user_id)
        .execute(pool)
        .await
        .expect("seed decoy workspace");
        for (accepted, target, suffix) in
            [(true, workspace_id, ""), (false, decoy_workspace_id, "-u")]
        {
            sqlx::query(
                r#"INSERT INTO "workspace_member_invites" ("created_at", "updated_at", "id", "email",
                    "accepted", "token", "role", "workspace_id", "created_by_id", "responded_at")
                   VALUES ($1, $1, $2, $3, $4, $5, 15, $6, NULL, NULL)"#,
            )
            .bind(now)
            .bind(uuid::Uuid::new_v4())
            .bind(email)
            .bind(accepted)
            .bind(format!("tok-1089-{slug}{suffix}"))
            .bind(target)
            .execute(pool)
            .await
            .expect("seed invite");
        }
        (user_id, workspace_id, decoy_workspace_id)
    }

    async fn cleanup_kernel_world(pool: &sqlx::PgPool, email: &str, workspace_ids: &[uuid::Uuid]) {
        let user_ids: Vec<uuid::Uuid> =
            sqlx::query_scalar(r#"SELECT "id" FROM "users" WHERE "email" = $1"#)
                .bind(email)
                .fetch_all(pool)
                .await
                .unwrap_or_default();
        for user_id in &user_ids {
            sqlx::query(r#"DELETE FROM "workspace_members" WHERE "member_id" = $1"#)
                .bind(user_id)
                .execute(pool)
                .await
                .ok();
        }
        sqlx::query(r#"DELETE FROM "workspace_member_invites" WHERE "email" = $1"#)
            .bind(email)
            .execute(pool)
            .await
            .ok();
        for workspace_id in workspace_ids {
            sqlx::query(r#"DELETE FROM "workspaces" WHERE "id" = $1"#)
                .bind(workspace_id)
                .execute(pool)
                .await
                .ok();
        }
        for user_id in &user_ids {
            sqlx::query(r#"DELETE FROM "users" WHERE "id" = $1"#)
                .bind(user_id)
                .execute(pool)
                .await
                .ok();
            // Best-effort queue sweep for this run's track_event emits
            // (the user id rides in kwargs).
            sqlx::query(
                r#"DELETE FROM "rust_job_queue"
                   WHERE "args"::text LIKE $1 OR "kwargs"::text LIKE $1"#,
            )
            .bind(format!("%{user_id}%"))
            .execute(pool)
            .await
            .ok();
        }
    }

    /// Consumed workspace invites soft-delete: the accepted row survives
    /// with `deleted_at` set (Django `.delete()` semantics), the
    /// membership lands, and the unaccepted decoy stays live.
    #[tokio::test]
    async fn google_kernel_consumed_invites_soft_delete() {
        let Some(pool) = kernel_pool().await else {
            return;
        };
        let redis =
            pidash_db::redis::RedisHandle::from_url(&kernel_redis_url()).expect("redis url parses");
        let tag = uuid::Uuid::new_v4().simple().to_string()[..8].to_owned();
        let email = format!("go1089-{tag}@example.test");
        let slug = format!("go1089-{tag}");
        let (user_id, workspace_id, decoy_workspace_id) =
            seed_invited_user(&pool, &email, &slug).await;

        let outcome =
            process_invitations(&pool, Some(&redis), user_id, &email, chrono::Utc::now()).await;

        // Observe everything before cleanup so a failure still sweeps.
        let members: i64 = sqlx::query_scalar(
            r#"SELECT COUNT(*) FROM "workspace_members"
               WHERE "workspace_id" = $1 AND "member_id" = $2"#,
        )
        .bind(workspace_id)
        .bind(user_id)
        .fetch_one(&pool)
        .await
        .expect("count members");
        let invite_rows: Vec<(bool, Option<chrono::DateTime<chrono::Utc>>)> = sqlx::query_as(
            r#"SELECT "accepted", "deleted_at" FROM "workspace_member_invites"
               WHERE "email" = $1"#,
        )
        .bind(&email)
        .fetch_all(&pool)
        .await
        .expect("read invites");
        let project_rows: i64 = sqlx::query_scalar(
            r#"SELECT COUNT(*) FROM "project_member_invites" WHERE "email" = $1"#,
        )
        .bind(&email)
        .fetch_one(&pool)
        .await
        .expect("count project invites");
        cleanup_kernel_world(&pool, &email, &[workspace_id, decoy_workspace_id]).await;

        assert!(outcome.is_ok(), "process_invitations succeeds");
        assert_eq!(members, 1, "membership row lands");
        assert_eq!(invite_rows.len(), 2, "both invite rows survive");
        for (accepted, deleted_at) in &invite_rows {
            if *accepted {
                assert!(deleted_at.is_some(), "consumed invite soft-deletes");
            } else {
                assert!(deleted_at.is_none(), "unaccepted invite stays live");
            }
        }
        assert_eq!(project_rows, 0, "no project invites seeded");
    }

    /// Returning-user `save_user_data` applies the `User.save()` effects
    /// (PIDASHCONV-1364): a blank `display_name` backfills from the email
    /// local part, `is_staff` syncs from `is_superuser`, and `updated_at`
    /// refreshes — the `user.py:169-187` semantics behind `base.py:233`.
    /// Fails on the pre-fix UPDATE (login stamps only).
    #[tokio::test]
    async fn google_kernel_save_user_data_applies_save_effects() {
        let Some(pool) = kernel_pool().await else {
            return;
        };
        let tag = uuid::Uuid::new_v4().simple().to_string()[..8].to_owned();
        let email = format!("go1364-{tag}@example.test");
        let user_id = uuid::Uuid::new_v4();
        let stale = chrono::Utc::now() - chrono::Duration::days(7);
        sqlx::query(
            r#"INSERT INTO "users" ("password", "id", "username", "email", "first_name", "last_name",
                "avatar", "date_joined", "created_at", "updated_at", "last_location",
                "created_location", "is_superuser", "is_managed", "is_password_expired",
                "is_active", "is_staff", "is_email_verified", "is_password_autoset",
                "token", "user_timezone", "last_login_ip", "last_logout_ip",
                "last_login_medium", "last_login_uagent", "is_bot", "display_name",
                "is_email_valid", "is_password_reset_required")
               VALUES ('pbkdf2_sha256$600000$kernelseed$kernelhash', $1, $2, $3, 'Kernel', 'User',
                '', $4, $4, $5, '', '', TRUE, FALSE, FALSE, TRUE, FALSE, TRUE, FALSE,
                '0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef',
                'UTC', '', '', 'email', '', FALSE, '', TRUE, FALSE)"#,
        )
        .bind(user_id)
        .bind(format!("u-{}", &user_id.simple().to_string()[..8]))
        .bind(&email)
        .bind(chrono::Utc::now())
        .bind(stale)
        .execute(&pool)
        .await
        .expect("seed returning user");
        let user = UserRow {
            id: user_id,
            password: "pbkdf2_sha256$600000$kernelseed$kernelhash".to_owned(),
            email: Some(email.clone()),
            is_active: true,
            is_superuser: true,
            first_name: "Kernel".to_owned(),
            last_name: "User".to_owned(),
            display_name: String::new(),
            avatar: String::new(),
            avatar_asset_id: None,
        };
        save_user_data(
            &pool,
            &user,
            "http://app.example.com",
            Some("127.0.0.1"),
            Some("kernel-agent"),
            chrono::Utc::now(),
        )
        .await
        .expect("save_user_data succeeds");
        let row: (String, bool, chrono::DateTime<chrono::Utc>) = sqlx::query_as(
            r#"SELECT "display_name", "is_staff", "updated_at" FROM "users" WHERE "id" = $1"#,
        )
        .bind(user_id)
        .fetch_one(&pool)
        .await
        .expect("read back user");
        sqlx::query(r#"DELETE FROM "users" WHERE "id" = $1"#)
            .bind(user_id)
            .execute(&pool)
            .await
            .ok();
        assert_eq!(
            row.0,
            email.split('@').next().unwrap_or_default(),
            "display_name backfills from the email local part"
        );
        assert!(row.1, "is_staff syncs from is_superuser");
        assert!(row.2 > stale, "updated_at refreshes on the login save");
    }

    // -- live kernel tests: delete_old_avatar (PIDASHCONV-1321) -----------
    // The HTTP success path is sealed (hardcoded https googleapis URLs),
    // so these drive `delete_old_avatar` directly against the scratch DB
    // with a loopback stub answering 200 to every S3 request.

    /// Loopback S3 stub: answers `200` with an empty body to every
    /// request. Returns the base URL plus a hit counter.
    fn s3_stub_200() -> (String, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
        use std::io::{Read, Write};
        let hits = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind s3 stub");
        let port = listener.local_addr().expect("stub addr").port();
        let counted = std::sync::Arc::clone(&hits);
        std::thread::spawn(move || {
            for stream in listener.incoming().take(8) {
                let Ok(mut socket) = stream else {
                    break;
                };
                counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let _ = socket.set_read_timeout(Some(std::time::Duration::from_secs(5)));
                let mut buf = [0u8; 4096];
                let _ = socket.read(&mut buf);
                let _ = socket.write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                );
            }
        });
        (format!("http://127.0.0.1:{port}"), hits)
    }

    /// Seed a user carrying a live `USER_AVATAR` asset (column lists mirror
    /// `insert_file_asset` and `_harness.db.Database.make_user`), with the
    /// user's FK + avatar string pointed at it. `created_by` is seeded set
    /// (like `insert_file_asset`) so the test pins Django's anonymous-save
    /// NULLing (`BaseModel.save`, `db/models/base.py:31-33`).
    async fn seed_avatar_user(
        pool: &sqlx::PgPool,
        email: &str,
        key: &str,
    ) -> (uuid::Uuid, uuid::Uuid) {
        let user_id = uuid::Uuid::new_v4();
        let asset_id = uuid::Uuid::new_v4();
        let now = chrono::Utc::now();
        sqlx::query(
            r#"INSERT INTO "users" ("password", "id", "username", "email", "first_name", "last_name",
                "avatar", "date_joined", "created_at", "updated_at", "last_location",
                "created_location", "is_superuser", "is_managed", "is_password_expired",
                "is_active", "is_staff", "is_email_verified", "is_password_autoset",
                "token", "user_timezone", "last_login_ip", "last_logout_ip",
                "last_login_medium", "last_login_uagent", "is_bot", "display_name",
                "is_email_valid", "is_password_reset_required")
               VALUES ('pbkdf2_sha256$600000$kernelseed$kernelhash', $1, $2, $3, 'Kernel', 'User',
                '', $4, $4, $4, '', '', FALSE, FALSE, FALSE, TRUE, FALSE, TRUE, FALSE,
                '0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef',
                'UTC', '', '', 'email', '', FALSE, 'Kernel User', TRUE, FALSE)"#,
        )
        .bind(user_id)
        .bind(format!("u-{}", &user_id.simple().to_string()[..8]))
        .bind(email)
        .bind(now)
        .execute(pool)
        .await
        .expect("seed user");
        sqlx::query(
            r#"INSERT INTO "file_assets"
               ("id", "created_at", "updated_at", "attributes", "asset",
                "created_by_id", "updated_by_id", "workspace_id",
                "is_deleted", "deleted_at", "is_archived",
                "comment_id", "entity_type", "external_id", "external_source",
                "is_uploaded", "issue_id", "page_id", "project_id",
                "size", "storage_metadata", "user_id", "draft_issue_id", "entity_identifier")
               VALUES ($1, $2, $2, '{"name": "google-avatar.png"}', $3, $4, NULL, NULL,
                       FALSE, NULL, FALSE,
                       NULL, 'USER_AVATAR', NULL, NULL,
                       TRUE, NULL, NULL, NULL,
                       42, NULL, $4, NULL, NULL)"#,
        )
        .bind(asset_id)
        .bind(now)
        .bind(key)
        .bind(user_id)
        .execute(pool)
        .await
        .expect("seed avatar asset");
        sqlx::query(r#"UPDATE "users" SET "avatar_asset_id" = $1, "avatar" = $2 WHERE "id" = $3"#)
            .bind(asset_id)
            .bind("http://avatar.test/old.png")
            .bind(user_id)
            .execute(pool)
            .await
            .expect("point user at asset");
        (user_id, asset_id)
    }

    async fn cleanup_avatar_world(pool: &sqlx::PgPool, user_id: uuid::Uuid) {
        sqlx::query(r#"DELETE FROM "rust_job_queue" WHERE "args"::text LIKE $1"#)
            .bind(format!("%{user_id}%"))
            .execute(pool)
            .await
            .ok();
        sqlx::query(r#"DELETE FROM "file_assets" WHERE "user_id" = $1"#)
            .bind(user_id)
            .execute(pool)
            .await
            .ok();
        sqlx::query(r#"DELETE FROM "users" WHERE "id" = $1"#)
            .bind(user_id)
            .execute(pool)
            .await
            .ok();
    }

    /// `delete_old_avatar` soft-deletes the asset row (Django
    /// `SoftDeleteModel.delete`: `deleted_at` + save + the
    /// `soft_delete_related_objects` enqueue), clears the user's FK +
    /// avatar string, and deletes the S3 object — instead of hard-deleting
    /// the still-referenced row (23503, early return, live row + FK left
    /// pointing at the deleted object).
    #[tokio::test]
    async fn google_kernel_delete_old_avatar_soft_deletes() {
        let Some(pool) = kernel_pool().await else {
            return;
        };
        let (s3_base, hits) = s3_stub_200();
        let storage = pidash_db::config::StorageSettings {
            use_minio: false,
            minio_endpoint_ssl: false,
            access_key_id: "kernel-access".to_owned(),
            secret_access_key: "kernel-secret".to_owned(),
            bucket_name: "uploads".to_owned(),
            region: "us-east-1".to_owned(),
            endpoint_url: Some(s3_base),
            signed_url_expiration_secs: 3600,
        };
        let tag = uuid::Uuid::new_v4().simple().to_string()[..8].to_owned();
        let email = format!("go1321-{tag}@example.test");
        let (user_id, asset_id) =
            seed_avatar_user(&pool, &email, &format!("kernel-{tag}-user-avatar.png")).await;
        let client = reqwest::Client::new();

        delete_old_avatar(
            &client,
            &pool,
            &storage,
            "http",
            "127.0.0.1",
            user_id,
            Some(asset_id),
            chrono::Utc::now(),
        )
        .await;

        let row: (
            Option<chrono::DateTime<chrono::Utc>>,
            Option<uuid::Uuid>,
            Option<uuid::Uuid>,
        ) = sqlx::query_as(
            r#"SELECT "deleted_at", "created_by_id", "updated_by_id"
               FROM "file_assets" WHERE "id" = $1"#,
        )
        .bind(asset_id)
        .fetch_one(&pool)
        .await
        .expect("old asset row survives as a soft delete");
        let user: (Option<uuid::Uuid>, String) =
            sqlx::query_as(r#"SELECT "avatar_asset_id", "avatar" FROM "users" WHERE "id" = $1"#)
                .bind(user_id)
                .fetch_one(&pool)
                .await
                .expect("read user");
        let jobs: Vec<(String, serde_json::Value, serde_json::Value)> = sqlx::query_as(
            r#"SELECT "task", "args", "kwargs" FROM "rust_job_queue"
               WHERE "args"::text LIKE $1"#,
        )
        .bind(format!("%{asset_id}%"))
        .fetch_all(&pool)
        .await
        .expect("read queue");
        cleanup_avatar_world(&pool, user_id).await;

        assert_eq!(
            hits.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "the S3 object is deleted exactly once"
        );
        assert!(row.0.is_some(), "old asset row soft-deletes");
        assert_eq!(row.1, None, "anonymous save NULLs created_by");
        assert_eq!(row.2, None, "anonymous save NULLs updated_by");
        assert_eq!(user.0, None, "user FK cleared");
        assert_eq!(user.1, "", "user avatar string cleared");
        assert_eq!(jobs.len(), 1, "one soft-delete enqueue, {jobs:?}");
        assert_eq!(
            jobs[0].0,
            "pi_dash.bgtasks.deletion_task.soft_delete_related_objects"
        );
        assert_eq!(
            jobs[0].1,
            serde_json::json!(["db", "fileasset", asset_id.to_string()])
        );
        assert_eq!(jobs[0].2, serde_json::json!({"using": null}));
    }

    /// A soft-deleted asset reads as missing (Django's `objects.get`
    /// `deleted_at IS NULL` manager raises `DoesNotExist`): no S3 I/O, no
    /// user write, no enqueue.
    #[tokio::test]
    async fn google_kernel_delete_old_avatar_ignores_soft_deleted() {
        let Some(pool) = kernel_pool().await else {
            return;
        };
        let (s3_base, hits) = s3_stub_200();
        let storage = pidash_db::config::StorageSettings {
            use_minio: false,
            minio_endpoint_ssl: false,
            access_key_id: "kernel-access".to_owned(),
            secret_access_key: "kernel-secret".to_owned(),
            bucket_name: "uploads".to_owned(),
            region: "us-east-1".to_owned(),
            endpoint_url: Some(s3_base),
            signed_url_expiration_secs: 3600,
        };
        let tag = uuid::Uuid::new_v4().simple().to_string()[..8].to_owned();
        let email = format!("go1321d-{tag}@example.test");
        let (user_id, asset_id) =
            seed_avatar_user(&pool, &email, &format!("kernel-{tag}-gone.png")).await;
        sqlx::query(r#"UPDATE "file_assets" SET "deleted_at" = $1 WHERE "id" = $2"#)
            .bind(chrono::Utc::now())
            .bind(asset_id)
            .execute(&pool)
            .await
            .expect("pre-soft-delete the asset");
        let client = reqwest::Client::new();

        delete_old_avatar(
            &client,
            &pool,
            &storage,
            "http",
            "127.0.0.1",
            user_id,
            Some(asset_id),
            chrono::Utc::now(),
        )
        .await;

        let user: (Option<uuid::Uuid>, String) =
            sqlx::query_as(r#"SELECT "avatar_asset_id", "avatar" FROM "users" WHERE "id" = $1"#)
                .bind(user_id)
                .fetch_one(&pool)
                .await
                .expect("read user");
        let jobs: i64 = sqlx::query_scalar(
            r#"SELECT COUNT(*) FROM "rust_job_queue" WHERE "args"::text LIKE $1"#,
        )
        .bind(format!("%{asset_id}%"))
        .fetch_one(&pool)
        .await
        .expect("count queue");
        cleanup_avatar_world(&pool, user_id).await;

        assert_eq!(
            hits.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "no S3 I/O for a soft-deleted asset"
        );
        assert_eq!(user.0, Some(asset_id), "user FK untouched");
        assert_eq!(jobs, 0, "no enqueue for a soft-deleted asset");
    }

    // -- live kernel tests: download_and_upload_avatar (PIDASHCONV-1324) --
    // The HTTP success path is sealed (hardcoded https googleapis URLs),
    // so these drive `download_and_upload_avatar` directly against the
    // scratch DB with a loopback avatar stub + a draining S3 stub.
    // Loopback avatar stub: serves `body_len` bytes as image/png, with an
    // explicit Content-Length unless `with_length` is false (then the
    // streaming guard alone decides, like a chunked provider response).
    fn avatar_stub_bytes(body_len: usize, with_length: bool) -> String {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind avatar stub");
        let port = listener.local_addr().expect("stub addr").port();
        std::thread::spawn(move || {
            let mut body = vec![0u8; body_len];
            body[..8].copy_from_slice(&[137, 80, 78, 71, 13, 10, 26, 10]);
            for stream in listener.incoming().take(8) {
                let Ok(mut socket) = stream else {
                    break;
                };
                let _ = socket.set_read_timeout(Some(std::time::Duration::from_secs(10)));
                let mut req = Vec::new();
                let mut buf = [0u8; 4096];
                loop {
                    match socket.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            req.extend_from_slice(&buf[..n]);
                            if req.windows(4).any(|w| w == b"\r\n\r\n") || req.len() > 65536 {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
                let head = if with_length {
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    )
                } else {
                    "HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nConnection: close\r\n\r\n"
                        .to_owned()
                };
                let _ = socket.write_all(head.as_bytes());
                let _ = socket.write_all(&body);
            }
        });
        format!("http://127.0.0.1:{port}/avatar.png")
    }

    /// Loopback S3 stub that drains request bodies (the avatar PUT ships
    /// megabytes; an early-close stub would RST the upload): answers
    /// `200` with an empty body to every request.
    fn s3_stub_drain() -> String {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind s3 stub");
        let port = listener.local_addr().expect("stub addr").port();
        std::thread::spawn(move || {
            for stream in listener.incoming().take(16) {
                let Ok(mut socket) = stream else {
                    break;
                };
                let _ = socket.set_read_timeout(Some(std::time::Duration::from_secs(30)));
                let mut req = Vec::new();
                let mut buf = [0u8; 65536];
                let mut content_length: usize = 0;
                let mut header_end: Option<usize> = None;
                loop {
                    match socket.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            req.extend_from_slice(&buf[..n]);
                            if header_end.is_none() {
                                if let Some(pos) = req.windows(4).position(|w| w == b"\r\n\r\n") {
                                    header_end = Some(pos + 4);
                                    let head = String::from_utf8_lossy(&req[..pos + 4]);
                                    for line in head.lines() {
                                        if let Some(v) = line
                                            .strip_prefix("Content-Length:")
                                            .or_else(|| line.strip_prefix("content-length:"))
                                        {
                                            content_length = v.trim().parse::<usize>().unwrap_or(0);
                                        }
                                    }
                                }
                            }
                            if let Some(end) = header_end {
                                if req.len() >= end + content_length {
                                    break;
                                }
                            }
                            if req.len() > 64 * 1024 * 1024 {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
                let _ = socket.write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                );
            }
        });
        format!("http://127.0.0.1:{port}")
    }

    async fn seed_download_user(pool: &sqlx::PgPool, email: &str) -> uuid::Uuid {
        let user_id = uuid::Uuid::new_v4();
        let now = chrono::Utc::now();
        sqlx::query(
            r#"INSERT INTO "users" ("password", "id", "username", "email", "first_name", "last_name",
                "avatar", "date_joined", "created_at", "updated_at", "last_location",
                "created_location", "is_superuser", "is_managed", "is_password_expired",
                "is_active", "is_staff", "is_email_verified", "is_password_autoset",
                "token", "user_timezone", "last_login_ip", "last_logout_ip",
                "last_login_medium", "last_login_uagent", "is_bot", "display_name",
                "is_email_valid", "is_password_reset_required")
               VALUES ('pbkdf2_sha256$600000$kernelseed$kernelhash', $1, $2, $3, 'Kernel', 'User',
                '', $4, $4, $4, '', '', FALSE, FALSE, FALSE, TRUE, FALSE, TRUE, FALSE,
                '0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef',
                'UTC', '', '', 'email', '', FALSE, 'Kernel User', TRUE, FALSE)"#,
        )
        .bind(user_id)
        .bind(format!("u-{}", &user_id.simple().to_string()[..8]))
        .bind(email)
        .bind(now)
        .execute(pool)
        .await
        .expect("seed user");
        user_id
    }

    /// The avatar size guards follow the configured limit
    /// (`settings.DATA_UPLOAD_MAX_MEMORY_SIZE`, i.e. `FILE_SIZE_LIMIT`),
    /// not a hard-coded 5 MiB: a 6 MB avatar uploads at a 10 MB limit and
    /// a 3 MB avatar falls back at a 1 MB limit — the Django decisions
    /// (`base.py:155-158,177-182`). The boundary pins Python's strict
    /// `>`: exactly-at-limit uploads via the pre-check, one-over falls
    /// back via the streaming guard (no Content-Length served). Fails on
    /// the pre-fix code (constant 5 MiB: the 6 MB case falls back).
    #[tokio::test]
    async fn google_kernel_download_avatar_follows_file_size_limit() {
        let Some(pool) = kernel_pool().await else {
            return;
        };
        let s3_base = s3_stub_drain();
        let storage = pidash_db::config::StorageSettings {
            use_minio: false,
            minio_endpoint_ssl: false,
            access_key_id: "kernel-access".to_owned(),
            secret_access_key: "kernel-secret".to_owned(),
            bucket_name: "uploads".to_owned(),
            region: "us-east-1".to_owned(),
            endpoint_url: Some(s3_base),
            signed_url_expiration_secs: 3600,
        };
        let client = reqwest::Client::new();
        // (`max_bytes`, body bytes, Content-Length served?, uploads?)
        let cases: [(u64, usize, bool, bool); 4] = [
            (10 * 1024 * 1024, 6 * 1024 * 1024, true, true),
            (1024 * 1024, 3 * 1024 * 1024, true, false),
            (5 * 1024 * 1024, 5 * 1024 * 1024, true, true),
            (5 * 1024 * 1024, 5 * 1024 * 1024 + 1, false, false),
        ];
        for (case_no, (max_bytes, body_len, with_length, uploads)) in cases.into_iter().enumerate()
        {
            let tag = uuid::Uuid::new_v4().simple().to_string()[..8].to_owned();
            let email = format!("go1324-{case_no}-{tag}@example.test");
            let user_id = seed_download_user(&pool, &email).await;
            let outcome = download_and_upload_avatar(
                &client,
                &pool,
                &storage,
                "http",
                "127.0.0.1",
                &avatar_stub_bytes(body_len, with_length),
                user_id,
                max_bytes,
                chrono::Utc::now(),
            )
            .await;
            let rows: Vec<(uuid::Uuid, f64)> =
                sqlx::query_as(r#"SELECT "id", "size" FROM "file_assets" WHERE "user_id" = $1"#)
                    .bind(user_id)
                    .fetch_all(&pool)
                    .await
                    .expect("read assets");
            cleanup_avatar_world(&pool, user_id).await;

            assert_eq!(
                outcome.is_some(),
                uploads,
                "case {case_no}: {body_len} bytes at a {max_bytes} limit",
            );
            if uploads {
                assert_eq!(rows.len(), 1, "case {case_no}: one FileAsset row lands");
                assert_eq!(
                    rows[0].0,
                    outcome.unwrap(),
                    "case {case_no}: row id returned"
                );
                assert_eq!(
                    rows[0].1, body_len as f64,
                    "case {case_no}: stored size is the body length",
                );
            } else {
                assert!(
                    rows.is_empty(),
                    "case {case_no}: fallback leaves no FileAsset row",
                );
            }
        }
    }
}
