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
//! # Documented divergences (no workspace precedent; follow-up owned)
//!
//! * Avatar persistence (`download_and_upload_avatar`, `delete_old_avatar`):
//!   no S3 client exists anywhere in `rust-api/`, so the port takes
//!   Python's own failure branch — the provider avatar URL is stored
//!   directly, as `base.py:335-345` does whenever the upload fails.
//!   Tracked by the follow-up issue in the PR body.
//! * Cache invalidation (`invalidate_cache_directly`, one Redis `KEYS` +
//!   `DEL` per joined workspace): `pidash_db::redis::RedisHandle` exposes
//!   no keys/del API and foundation crates are read-only, so the call is
//!   a documented no-op (same precedent as `recent_visited_task` in
//!   `app_issues`). Tracked by the same follow-up.
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

use crate::middleware::SessionHandle;
use crate::state::AppState;

use pidash_services::auth_oauth::error::AuthenticationException;
use pidash_services::auth_oauth::exchange::map_exchange_error;
use pidash_services::auth_oauth::providers::{
    google_auth_url, google_configured, google_token_data, google_token_post_data,
    google_user_data, redirect_uri, GOOGLE_USERINFO_URL,
};
use pidash_services::auth_session::{
    allowed_hosts_for, base_host, get_safe_redirect_url, redirection_path_str,
    select_redirection_path, HostSettings, ParamValue,
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
/// through to Django (its 405/OPTIONS bodies live there). `HEAD` rides
/// axum's `get` handling like Django's `GET`-backed `HEAD`.
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

/// `django.contrib.auth.login` backend (`settings/common.py:125`, single
/// `AUTHENTICATION_BACKENDS` entry; harness `SESSION_BACKEND`).
pub const LOGIN_BACKEND: &str = "django.contrib.auth.backends.ModelBackend";
/// Salt for `get_session_auth_hash`
/// (`AbstractBaseUser.get_session_auth_hash`).
pub const SESSION_AUTH_HASH_SALT: &str =
    "django.contrib.auth.models.AbstractBaseUser.get_session_auth_hash";

/// `make_password` iteration count under the pinned Django 4.2.30
/// (`PBKDF2PasswordHasher.iterations`, verified against the installed
/// Django; the admin port pins the same value).
pub const PASSWORD_HASH_ITERATIONS: u32 = 600_000;

// ---------------------------------------------------------------------------
// Small pure helpers (branch logic shared by the handlers; unit-tested
// against AUTHOAUTH-F10/F11 below).
// ---------------------------------------------------------------------------

/// 302 with a `Location`, mirroring `HttpResponseRedirect(url)`.
pub fn redirect_response(location: &str) -> Response {
    let mut response = StatusCode::FOUND.into_response();
    response.headers_mut().insert(
        header::LOCATION,
        header::HeaderValue::from_str(location).unwrap_or_else(|_| {
            // `HeaderValue` rejects non-ASCII bytes; every URL this module
            // builds is ASCII (urlencode output + validated paths), so this
            // arm is unreachable — fail closed rather than panic.
            header::HeaderValue::from_static("/")
        }),
    );
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
         \"last_name\", \"avatar\", \"avatar_asset_id\", \"cover_image\", \
         \"cover_image_asset_id\", \"date_joined\", \"created_at\", \
         \"updated_at\", \"last_location\", \"created_location\", \
         \"is_superuser\", \"is_managed\", \"is_password_expired\", \
         \"is_active\", \"is_staff\", \"is_email_verified\", \
         \"is_password_autoset\", \"is_password_reset_required\", \"token\", \
         \"last_active\", \"last_login_time\", \"last_logout_time\", \
         \"last_login_ip\", \"last_logout_ip\", \"last_login_medium\", \
         \"last_login_uagent\", \"token_updated_at\", \"is_bot\", \"bot_type\", \
         \"user_timezone\", \"is_email_valid\", \"masked_at\") \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, \
         $15, $16, $17, $18, $19, $20, $21, $22, $23, $24, $25, $26, $27, $28, \
         $29, $30, $31, $32, $33, $34, $35, $36, $37, $38, $39, $40)",
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
    .bind(None::<String>)
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

/// `user.get_session_auth_hash()`: `salted_hmac(salt, password)` hex
/// (`django.utils.crypto.salted_hmac`, SHA256) — same construction as the
/// license-domain verifier.
pub fn session_auth_hash(password_field: &str, secret_key: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let key = Sha256::digest([SESSION_AUTH_HASH_SALT.as_bytes(), secret_key].concat());
    use hmac::{Hmac, Mac};
    let mut mac = Hmac::<Sha256>::new_from_slice(&key).expect("HMAC-SHA256 accepts any key length");
    mac.update(password_field.as_bytes());
    let bytes = mac.finalize().into_bytes();
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
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
/// email prefix always wins), and the avatar URL. The old asset row is
/// deleted and the FK cleared (the S3 object delete and the re-upload
/// have no workspace precedent — see the module docs); the FK stays
/// untouched when no asset was linked.
pub async fn sync_user_data(
    pool: &sqlx::PgPool,
    user: &UserRow,
    email: &str,
    first_name: &str,
    last_name: &str,
    avatar_url: &str,
) -> Result<(), CallbackError> {
    if user.avatar_asset_id.is_some() {
        sqlx::query("DELETE FROM \"file_assets\" WHERE \"id\" = $1")
            .bind(user.avatar_asset_id)
            .execute(pool)
            .await
            .map_err(|_| CallbackError::Internal)?;
    }
    sqlx::query(
        "UPDATE \"users\" SET \"first_name\" = $1, \"last_name\" = $2, \
         \"display_name\" = $3, \"avatar\" = $4, \"avatar_asset_id\" = NULL \
         WHERE \"id\" = $5",
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
    .bind(avatar_url)
    .bind(user.id)
    .execute(pool)
    .await
    .map_err(|_| CallbackError::Internal)?;
    Ok(())
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
    let message = pidash_jobs::celery::CeleryTaskMessage::new(task, args, kwargs);
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
/// rotation (`User.save`: `token_updated_at` set ⇒ `token = 64 hex`) and
/// marks the user active. `ip` / `user_agent` bind NULL when absent —
/// the documented NOT NULL 500 hazard.
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
         \"is_active\" = TRUE, \"token\" = $7 WHERE \"id\" = $8",
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

/// `process_workspace_project_invitations`
/// (`workspace_project_join.py:20-91`): workspace joins (membership +
/// `track_event` per invite; cache invalidation is a documented no-op —
/// see the module docs), project-invite workspace joins (mapped role),
/// the buggy project joins, then both invite deletes.
pub async fn process_invitations(
    pool: &sqlx::PgPool,
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
    sqlx::query(
        "DELETE FROM \"workspace_member_invites\" WHERE \"email\" = $1 AND \"accepted\" = TRUE",
    )
    .bind(email)
    .execute(pool)
    .await
    .map_err(|_| CallbackError::Internal)?;
    sqlx::query(
        "DELETE FROM \"project_member_invites\" WHERE \"email\" = $1 AND \"accepted\" = TRUE",
    )
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
/// (`authentication/utils/login.py:14-29`): session-key rotation (flush
/// when a *different* user is logged in — data gone, fresh key; cycle
/// when anonymous — data kept, fresh key; untouched for the same user),
/// the `_auth_*` triple, the `update_last_login` signal write, then
/// `device_info`. The middleware persists + issues the `session-id`
/// cookie on success; a 500 answer never persists (status gate).
pub async fn login_user(
    ctx: &RequestContext<'_>,
    user: &UserRow,
    secret: &[u8],
    domain: &str,
) -> Result<(), CallbackError> {
    match session_get(&ctx.session, "_auth_user_id") {
        Some(current) if current != user.id.to_string() => ctx.session.lock().clear(),
        Some(_) => {}
        None => {
            ctx.session.lock().key = None;
        }
    }
    session_set(&ctx.session, "_auth_user_id", user.id.to_string());
    session_set(&ctx.session, "_auth_user_backend", LOGIN_BACKEND.to_owned());
    session_set(
        &ctx.session,
        "_auth_user_hash",
        session_auth_hash(&user.password, secret),
    );
    sqlx::query("UPDATE \"users\" SET \"last_login\" = $1 WHERE \"id\" = $2")
        .bind(chrono::Utc::now())
        .bind(user.id)
        .execute(&ctx.pool)
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
    Ok(())
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
/// avatar-URL fallback, then IDP sync for the new user when enabled —
/// `not is_signup`, never for existing users), stamps, post-auth
/// workflow on the app side (`with_callback`), account upsert.
pub async fn authenticate_google(
    ctx: &RequestContext<'_>,
    secret: &str,
    code: &str,
    with_callback: bool,
) -> Result<AuthedUser, CallbackError> {
    let now = chrono::Utc::now();
    let (client_id, client_secret) = google_config(&ctx.pool, secret).await?;
    let callback_uri = redirect_uri(ctx.secure, &ctx.host, PROVIDER);
    let client = reqwest::Client::builder()
        .build()
        .map_err(|_| CallbackError::Internal)?;
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
            }
            get_or_create_profile(&ctx.pool, row.id, now).await?;
            if !is_signup && google_sync_enabled(&ctx.pool, secret).await? {
                sync_user_data(&ctx.pool, &row, &email, first_name, last_name, avatar_url).await?;
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
        process_invitations(&ctx.pool, user.id, &email, now).await?;
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
    match authenticate_google(&ctx, &secret, &code, true).await {
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
            if login_user(&ctx, &authed.row, secret.as_bytes(), &ctx.app_base)
                .await
                .is_err()
            {
                return server_error();
            }
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
            redirect_response(&url)
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
    if let Ok(authed) = authenticate_google(&ctx, &secret, &code, false).await {
        let _ = login_user(&ctx, &authed.row, secret.as_bytes(), &ctx.space_base).await;
    }
    server_error()
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
