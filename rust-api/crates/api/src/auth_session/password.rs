#![forbid(unsafe_code)]

//! D-16 password-management + CSRF handlers (stage 5, PIDASHCONV-434).
//!
//! Ports the password/CSRF closure onto the D-16 kernels:
//!
//! * `GET /auth/get-csrf-token/` (`CSRFTokenEndpoint`,
//!   `authentication/views/common.py:28-35`): `AllowAny`, 200
//!   `{"csrf_token"}` with F-05 CSRF semantics (`pidash_auth::csrf`).
//! * `POST /auth/change-password/` (`ChangePasswordEndpoint`,
//!   `common.py:47-96`) and `POST /auth/set-password/`
//!   (`SetUserPasswordEndpoint`, `common.py:99-138`): session-authenticated
//!   DRF views (production default `IsAuthenticated`,
//!   `settings/common.py:116`) with DRF `SessionAuthentication` CSRF and
//!   default-Anon throttling.
//! * `POST /auth/forgot-password/` + `POST /auth/spaces/forgot-password/`
//!   (`ForgotPasswordEndpoint`, `views/app/password_management.py:45-96`,
//!   space twin `views/space/password_management.py:45-108`): `AllowAny`
//!   with `AuthenticationThrottle`, publishing `forgot_password.delay` on
//!   the user-exists branch.
//! * `POST /auth/reset-password/<uidb64>/<token>/` +
//!   `POST /auth/spaces/reset-password/<uidb64>/<token>/`
//!   (`ResetPasswordEndpoint`, `app/password_management.py:99-176`, space
//!   twin `space/...:111-160`): plain Django views answering 302s, with
//!   `CsrfViewMiddleware` semantics and the custom `csrf_failure` page.
//!
//! Only these seven paths with their owned methods are registered, so the
//! edge serves exactly this closure from Rust while every sibling path
//! (sign-in/up, magic, OAuth, CLI) keeps proxying to Django — route
//! registration is the cutover granularity, no flag needed.
//!
//! Layering: branch decisions, the Django 4.2 reset-token recipe, redirect
//! builders and the `UserSerializer` renderer live in
//! `pidash_services::auth_session::password`; error bodies/redirect
//! composition in `pidash_services::auth_session::{shapes, guards,
//! tasks}`; session-CSRF-password primitives in `pidash_auth`. This module
//! owns the HTTP shell (routes, session auth, CSRF/throttle gates), the
//! SQL text, and the AMQP publish.
//!
//! Ported bugs (also listed in the PR; translated, not fixed):
//! - reset-app EXPIRED branch is dead (bad-UTF-8 uids answer 5125, never
//!   5130); reset-space raises 500 for unknown ids and non-UUID uids.
//! - reset-app failure targets have no trailing slash, space targets keep
//!   a double slash; app success is `sign-in?success=True` (capital T),
//!   space success is the bare space base.
//! - set-password answers `INVALID_PASSWORD` (5020) for weak passwords
//!   where change-password answers `PASSWORD_TOO_WEAK` (5021).
//! - `invalidate_cache("/api/users/me/")` on set-password deletes a cache
//!   key no reader ever sets (`cache_response` never wraps a users/me
//!   view), so the observable port performs no cache I/O.
//! - `PasswordResetTokenGenerator` tokens are single-use (`set_password`
//!   invalidates outstanding tokens).
//! - multipart form bodies on the reset views parse as empty (no file
//!   field exists on these forms; urlencoded and JSON-on-DRF cover every
//!   product and contract caller).

use std::time::{SystemTime, UNIX_EPOCH};

use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use serde_json::Value;

use pidash_services::auth_session::{password as pw, tasks as t};

use crate::state::AppState;

/// Register the seven password/CSRF routes. Nothing else: sibling paths
/// stay unmatched and proxy to Django.
///
/// Non-owned methods proxy too. DRF authenticates before it checks the
/// method, and the plain views answer Django's own 405-after-CSRF —
///
/// answering 405 in Rust would break both, so every non-owned method
/// falls through to Django with no per-method logic.
pub fn password_routes() -> Router<AppState> {
    Router::new()
        .route("/auth/get-csrf-token/", owned_get(get(get_csrf_token)))
        .route("/auth/change-password/", owned_post(post(change_password)))
        .route("/auth/set-password/", owned_post(post(set_password)))
        .route(
            "/auth/forgot-password/",
            owned_post(post(forgot_password_app)),
        )
        .route(
            "/auth/spaces/forgot-password/",
            owned_post(post(forgot_password_space)),
        )
        .route(
            "/auth/reset-password/{uidb64}/{token}/",
            owned_post(post(reset_password_app)),
        )
        .route(
            "/auth/spaces/reset-password/{uidb64}/{token}/",
            owned_post(post(reset_password_space)),
        )
}

/// An owned GET path: reads serve from Rust, everything else falls
/// through to Django (its 405/401s live there). `HEAD` rides axum's
/// `get` handling like Django's `GET`-backed `HEAD`.
fn owned_get(
    get_handler: axum::routing::MethodRouter<AppState>,
) -> axum::routing::MethodRouter<AppState> {
    get_handler
        .post(crate::edge::proxy)
        .put(crate::edge::proxy)
        .patch(crate::edge::proxy)
        .delete(crate::edge::proxy)
        .options(crate::edge::proxy)
}

/// An owned POST path: writes serve from Rust, everything else falls
/// through to Django. OPTIONS proxies too: DRF answers metadata (401
/// anon / 200 authed) where axum would 405.
fn owned_post(
    post_handler: axum::routing::MethodRouter<AppState>,
) -> axum::routing::MethodRouter<AppState> {
    post_handler
        .get(crate::edge::proxy)
        .put(crate::edge::proxy)
        .patch(crate::edge::proxy)
        .delete(crate::edge::proxy)
        .options(crate::edge::proxy)
}

// ---------------------------------------------------------------------------
// Responses (exact bytes)
// ---------------------------------------------------------------------------

fn json_response(status: StatusCode, body: String) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body))
        .expect("password response")
}

fn json_400(pairs: &[(String, Value)]) -> Response {
    json_response(
        StatusCode::BAD_REQUEST,
        pidash_services::auth_session::error_dict_json(pairs),
    )
}

fn redirect_302(location: String) -> Response {
    Response::builder()
        .status(StatusCode::FOUND)
        .header(header::LOCATION, location)
        .body(axum::body::Body::empty())
        .expect("redirect response")
}

/// DRF `NotAuthenticated` default, passed through untouched by
/// `auth_exception_handler` (`adapter/exception.py:22-24`).
fn unauthorized() -> Response {
    json_response(
        StatusCode::UNAUTHORIZED,
        serde_json::to_string(&pidash_services::auth_session::unauthenticated_body())
            .expect("401 body serializes"),
    )
}

/// DRF `SessionAuthentication` CSRF denial: `PermissionDenied`
/// (`authentication.py:148`), passed through untouched (403).
fn csrf_denied(reason: &str) -> Response {
    json_response(
        StatusCode::FORBIDDEN,
        serde_json::to_string(&serde_json::json!({
            "detail": format!("CSRF Failed: {reason}")
        }))
        .expect("403 body serializes"),
    )
}

/// Live 429: `auth_exception_handler` rewrites DRF's default throttled
/// response to the 5900 error dict (`adapter/exception.py:26-31`); the
/// `Retry-After` header from `exception_handler` survives the rewrite
/// (`views.py:90-91`, `'%d' % wait`, truncated).
fn throttled(wait: Option<f64>) -> Response {
    let mut response = json_response(
        StatusCode::TOO_MANY_REQUESTS,
        pidash_services::auth_session::throttle_denied_json(),
    );
    if let Some(wait) = wait {
        if let Ok(value) = header::HeaderValue::from_str(&format!("{}", wait.trunc() as u64)) {
            response.headers_mut().insert(header::RETRY_AFTER, value);
        }
    }
    response
}

/// Empty 500. Django's production 500 page is deployment-specific HTML;
/// only the status is contractual here (the 500 vectors assert status).
fn server_error() -> Response {
    Response::builder()
        .status(StatusCode::INTERNAL_SERVER_ERROR)
        .body(axum::body::Body::empty())
        .expect("static 500 response")
}

/// Custom `csrf_failure` page (`common.py:38-44`): `render()` with the
/// default status, i.e. 200, carrying `{reason, root_url}` of which the
/// template only reads `root_url`.
fn csrf_failure_page(root_url: &str) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
        .body(axum::body::Body::from(
            crate::auth_session::render_csrf_failure(root_url),
        ))
        .expect("csrf failure response")
}

/// DRF `UnsupportedMediaType` (`{"detail": "Unsupported media type
/// \"<ct>\" in request."}`, 415).
fn unsupported_media_type(content_type: &str) -> Response {
    json_response(
        StatusCode::UNSUPPORTED_MEDIA_TYPE,
        serde_json::to_string(&serde_json::json!({
            "detail": format!("Unsupported media type \"{content_type}\" in request.")
        }))
        .expect("415 body serializes"),
    )
}

// ---------------------------------------------------------------------------
// Request parsing (`request.data` / `request.POST`)
// ---------------------------------------------------------------------------

/// A parsed request body: DRF `request.data` merges JSON and form;
/// plain views read `request.POST` (form only — a JSON body is ignored
/// there and reads as missing).
#[derive(Debug, Clone, Default)]
struct BodyData {
    json: Option<Value>,
    form: Vec<(String, String)>,
}

fn content_type_of(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
}

/// Percent-decode plus `+`-as-space (`urlencoded` form field decoding).
fn form_decode(field: &str) -> String {
    let mut out = Vec::with_capacity(field.len());
    let bytes = field.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() + 1 => {
                let hex = &field[i + 1..(i + 3).min(field.len())];
                match u8::from_str_radix(hex, 16) {
                    Ok(byte) => {
                        out.push(byte);
                        i += 3;
                    }
                    Err(_) => {
                        out.push(b'%');
                        i += 1;
                    }
                }
            }
            _ => {
                out.push(bytes[i]);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn parse_form(body: &[u8]) -> Vec<(String, String)> {
    if body.is_empty() {
        return Vec::new();
    }
    String::from_utf8_lossy(body)
        .split('&')
        .map(|pair| {
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            (form_decode(k), form_decode(v))
        })
        .collect()
}

/// Why a body could not be parsed (both answer before the view runs).
#[derive(Debug, Clone, PartialEq)]
enum BodyError {
    /// DRF `JSONParser` `ParseError` (400 `{"detail": ...}`).
    InvalidJson(String),
    /// No parser handles the media type (415).
    UnsupportedMediaType(String),
}

impl IntoResponse for BodyError {
    fn into_response(self) -> Response {
        match self {
            // `ParseError.detail` is parser-specific text; the status and
            // shape are contractual, the message echoes this parser.
            BodyError::InvalidJson(message) => json_response(
                StatusCode::BAD_REQUEST,
                serde_json::to_string(&serde_json::json!({
                    "detail": format!("JSON parse error - {message}")
                }))
                .expect("400 body serializes"),
            ),
            BodyError::UnsupportedMediaType(content_type) => unsupported_media_type(&content_type),
        }
    }
}

/// Parse the body per DRF content negotiation.
fn read_body(headers: &HeaderMap, body: &[u8]) -> Result<BodyData, BodyError> {
    if body.is_empty() {
        return Ok(BodyData::default());
    }
    let content_type = content_type_of(headers).unwrap_or_default();
    let mime = content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_lowercase();
    match mime.as_str() {
        // DRF `JSONParser` (empty content-type falls through to the
        // first parser, JSON, in this project's parser order).
        "application/json" | "" => match serde_json::from_slice::<Value>(body) {
            Ok(json) => Ok(BodyData {
                json: Some(json),
                form: Vec::new(),
            }),
            Err(error) => Err(BodyError::InvalidJson(error.to_string())),
        },
        "application/x-www-form-urlencoded" => Ok(BodyData {
            json: None,
            form: parse_form(body),
        }),
        // No file field exists on these forms; multipart (which Django
        // would parse into `request.POST`) reads as empty here.
        _ => Err(BodyError::UnsupportedMediaType(content_type)),
    }
}

/// One form/JSON field: absent (missing, null, `""`, `false`), text, or a
/// present non-string (whose downstream use raises `TypeError` in
/// Python, i.e. HTTP 500, except where the view `str()`s it first).
#[derive(Debug, Clone, PartialEq)]
enum Field {
    Absent,
    Text(String),
    Opaque,
}

fn json_field(json: &Option<Value>, key: &str) -> Option<Field> {
    let value = json.as_ref()?.get(key)?;
    // Python truthiness of `request.data.get(key, False)`: every falsy
    // JSON value reads as missing; truthy non-strings crash downstream
    // (`TypeError`, i.e. 500).
    Some(match value {
        Value::Null => Field::Absent,
        Value::Bool(value) => {
            if *value {
                Field::Opaque
            } else {
                Field::Absent
            }
        }
        Value::Number(value) => {
            if value.as_f64() == Some(0.0) {
                Field::Absent
            } else {
                Field::Opaque
            }
        }
        Value::String(s) if s.is_empty() => Field::Absent,
        Value::String(s) => Field::Text(s.clone()),
        Value::Array(items) if items.is_empty() => Field::Absent,
        Value::Object(map) if map.is_empty() => Field::Absent,
        _ => Field::Opaque,
    })
}

/// Whether `request.data.get` itself would raise (`AttributeError` on a
/// non-object JSON body, i.e. 500 before any branch runs).
fn data_get_raises(data: &BodyData) -> bool {
    !matches!(&data.json, None | Some(Value::Object(_)))
}

fn form_field(form: &[(String, String)], key: &str) -> Option<Field> {
    // `QueryDict.get` returns the LAST value for repeated keys.
    let value = form.iter().rev().find(|(k, _)| k == key).map(|(_, v)| v)?;
    if value.is_empty() {
        return Some(Field::Absent);
    }
    Some(Field::Text(value.clone()))
}

/// DRF `request.data.get(key)`: JSON wins when the body parsed as JSON,
/// else the form field (last value wins, like `QueryDict`).
fn data_field(data: &BodyData, key: &str) -> Field {
    if data.json.is_some() {
        json_field(&data.json, key).unwrap_or(Field::Absent)
    } else {
        form_field(&data.form, key).unwrap_or(Field::Absent)
    }
}

/// Plain-view `request.POST.get(key, False)`: form only, never JSON.
fn post_field(data: &BodyData, key: &str) -> Field {
    form_field(&data.form, key).unwrap_or(Field::Absent)
}

// ---------------------------------------------------------------------------
// Shared request plumbing
// ---------------------------------------------------------------------------

fn pool_of(state: &AppState) -> Option<sqlx::PgPool> {
    state.pools().map(|pools| pools.primary().clone())
}

/// Peer address from the serve-installed `ConnectInfo` extension, when
/// present (`Option<ConnectInfo>` is not an extractor — only types
/// implementing `OptionalFromRequestParts` wrap in `Option`).
struct PeerAddr(Option<std::net::SocketAddr>);

impl<S> axum::extract::FromRequestParts<S> for PeerAddr
where
    S: Send + Sync,
{
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        Ok(PeerAddr(
            parts
                .extensions
                .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
                .map(|info| info.0),
        ))
    }
}

/// `request.user`: the hash-verified session actor, or the 401
/// (`resolve_actor` mirrors `get_user` + DRF `SessionAuthentication`;
/// anonymous stays anonymous and the `IsAuthenticated` default answers).
/// Why session authentication failed: anonymous (401) or broken (500).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActorFail {
    Anonymous,
    Broken,
}

async fn authed_actor(
    state: &AppState,
    pool: &sqlx::PgPool,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Result<crate::license::Actor, ActorFail> {
    match crate::license::resolve_actor(pool, state.settings().secret_key.as_bytes(), extension)
        .await
    {
        Ok(Some(actor)) => Ok(actor),
        Ok(None) => Err(ActorFail::Anonymous),
        Err(_) => Err(ActorFail::Broken),
    }
}

fn actor_fail(fail: ActorFail) -> Response {
    match fail {
        ActorFail::Anonymous => unauthorized(),
        ActorFail::Broken => server_error(),
    }
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn now_float() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// DRF `get_ident` (`throttling.py:23-40`, `NUM_PROXIES` unset): the
/// `X-Forwarded-For` header with all whitespace stripped, else the peer
/// address.
fn throttle_ident(headers: &HeaderMap, peer: Option<std::net::SocketAddr>) -> String {
    if let Some(xff) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()) {
        if !xff.is_empty() {
            return xff.split_whitespace().collect();
        }
    }
    peer.map(|p| p.ip().to_string()).unwrap_or_default()
}

/// `get_client_ip` (`utils/ip_address.py`): first `X-Forwarded-For`
/// token (no stripping), else the peer address.
fn client_ip(headers: &HeaderMap, peer: Option<std::net::SocketAddr>) -> Option<String> {
    let xff = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok());
    let remote = peer.map(|p| p.ip().to_string());
    crate::license::handlers_auth_forms::client_ip(xff, remote.as_deref())
}

/// DRF throttle gate for the `AnonRateThrottle`-family guards
/// (`AuthenticationThrottle` on forgot-password, default `Anon` on
/// CSRF/change/set). Authenticated callers are never throttled
/// (`get_cache_key` returns `None`); anonymous callers share the
/// redis-backed history with Django, so the suite's counter flush
/// resets both. No redis handle (or an unreadable/unwritable cache)
/// degrades to allow, per the `AppState` convention.
async fn throttle_gate(
    state: &AppState,
    scope: &str,
    rate: &str,
    authenticated: bool,
    ident: &str,
) -> Result<(), Option<f64>> {
    if authenticated {
        return Ok(());
    }
    let (num_requests, duration) =
        pidash_services::auth_session::parse_rate(Some(rate)).ok_or(None)?;
    let key = pidash_services::auth_session::throttle_cache_key(scope, ident);
    let redis = match state.redis() {
        Some(redis) => redis,
        None => return Ok(()),
    };
    let history: Vec<f64> = match redis.get_string(&key).await {
        Ok(Some(raw)) => serde_json::from_str(&raw).unwrap_or_default(),
        _ => Vec::new(),
    };
    let now = now_float();
    let decision =
        pidash_services::auth_session::allow_request(&history, num_requests, duration, now);
    if !decision.allowed {
        return Err(pidash_services::auth_session::throttle_wait(
            &decision.history,
            num_requests,
            duration,
            now,
        ));
    }
    let raw = serde_json::to_string(&decision.history).unwrap_or_else(|_| "[]".to_owned());
    if redis.set_ex(&key, &raw, duration).await.is_err() {
        return Ok(());
    }
    Ok(())
}

/// CSRF secret from the `csrftoken` cookie (`_get_secret` with
/// `CSRF_USE_SESSIONS` off): missing is `None`, a 64-char masked value
/// is unmasked, anything else passes through for the format gate.
fn csrf_cookie_secret(headers: &HeaderMap) -> Option<Result<String, pidash_auth::csrf::CsrfError>> {
    let header = headers.get(header::COOKIE).and_then(|v| v.to_str().ok())?;
    let raw = pidash_auth::session::cookie_value(header, "csrftoken")?;
    if raw.len() == pidash_auth::csrf::CSRF_TOKEN_LENGTH {
        Some(pidash_auth::csrf::unmask_token(&raw))
    } else {
        Some(Ok(raw))
    }
}

/// Request token: the POST field first (POST only), then the
/// `X-CSRFToken` header (`_check_token`, Django 4.2).
fn csrf_request_token(
    data: &BodyData,
    headers: &HeaderMap,
    is_post: bool,
) -> (Option<String>, &'static str) {
    if is_post {
        if let Field::Text(value) = post_field(data, "csrfmiddlewaretoken") {
            if !value.is_empty() {
                return (Some(value), "POST");
            }
        }
    }
    match headers
        .get("x-csrftoken")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
    {
        Some(token) => (Some(token), "header"),
        None => (None, "header"),
    }
}

fn header_source_name(source: &str) -> String {
    if source == "POST" {
        "POST".to_owned()
    } else {
        // `HttpHeaders.parse_header_name("HTTP_X_CSRFTOKEN")`.
        "the 'X-Csrftoken' HTTP header".to_owned()
    }
}

/// `CsrfViewMiddleware._check_token` (Django 4.2): `Ok(())` or the
/// rejection reason. Origin/Referer checks only apply to HTTPS; the
/// reason never reaches the response body here (the failure page only
/// reads `root_url`), but the DRF denial echoes it.
fn check_csrf_token(data: &BodyData, headers: &HeaderMap, is_post: bool) -> Result<(), String> {
    let secret = match csrf_cookie_secret(headers) {
        None => return Err("CSRF cookie not set.".to_owned()),
        Some(Err(error)) => return Err(format!("CSRF cookie {}.", error.reason())),
        Some(Ok(secret)) => secret,
    };
    if let Err(error) = pidash_auth::csrf::check_token_format(&secret) {
        return Err(format!("CSRF cookie {}.", error.reason()));
    }
    let (token, source) = csrf_request_token(data, headers, is_post);
    let Some(token) = token else {
        return Err("CSRF token missing.".to_owned());
    };
    if token.is_empty() {
        return Err("CSRF token missing.".to_owned());
    }
    if let Err(error) = pidash_auth::csrf::check_token_format(&token) {
        return Err(format!(
            "CSRF token from {} {}.",
            header_source_name(source),
            error.reason()
        ));
    }
    if !pidash_auth::csrf::tokens_match(&token, &secret) {
        return Err(format!(
            "CSRF token from {} incorrect.",
            header_source_name(source)
        ));
    }
    Ok(())
}

/// `GET /auth/get-csrf-token/` (`CSRFTokenEndpoint`, `common.py:28-35`):
/// `AllowAny`, default-Anon throttle, 200 `{"csrf_token"}`. Reuses the
/// cookie secret when present (renewing the cookie, like `get_token`),
/// else mints a fresh one.
async fn get_csrf_token(
    State(state): State<AppState>,
    headers: HeaderMap,
    peer: PeerAddr,
) -> Response {
    let Some(pool) = pool_of(&state) else {
        return server_error();
    };
    // DRF order: authentication (session user, if any), then throttles.
    // `AllowAny` passes regardless; the throttle only counts anonymous
    // callers (`AnonRateThrottle.get_cache_key`).
    let actor =
        match crate::license::resolve_actor(&pool, state.settings().secret_key.as_bytes(), None)
            .await
        {
            Ok(actor) => actor,
            Err(_) => return server_error(),
        };
    let peer_addr = peer.0;
    if let Err(wait) = throttle_gate(
        &state,
        pidash_services::auth_session::DEFAULT_ANON_SCOPE,
        pidash_services::auth_session::DEFAULT_ANON_RATE,
        actor.is_some(),
        &throttle_ident(&headers, peer_addr),
    )
    .await
    {
        return throttled(wait);
    }
    let secret = match csrf_cookie_secret(&headers) {
        // No cookie: mint a fresh secret (`get_token`).
        None => pidash_auth::csrf::new_secret(),
        // A masked (64-char) cookie value unmasks first (`_get_secret`);
        // anything else reuses verbatim and the mask step fails closed.
        Some(Ok(secret)) => secret,
        Some(Err(_)) => return server_error(),
    };
    let masked = match pidash_auth::csrf::mask_secret(&secret) {
        Ok(masked) => masked,
        Err(_) => return server_error(),
    };
    let now = now_unix();
    let secure = state.settings().session.cookie_secure;
    let cookie = pw::csrf_set_cookie_value(
        &secret,
        &pidash_auth::session::http_date(now + pw::CSRF_COOKIE_AGE_SECS),
        secure,
    );
    let mut response = json_response(
        StatusCode::OK,
        serde_json::to_string(&pidash_auth::csrf::csrf_token_body(&masked))
            .expect("csrf body serializes"),
    );
    if let Ok(value) = header::HeaderValue::from_str(&cookie) {
        response.headers_mut().append(header::SET_COOKIE, value);
    }
    response
}

/// Session-auth hash for `login()`: `salted_hmac(
/// "...AbstractBaseUser.get_session_auth_hash", password_field,
/// SECRET_KEY).hexdigest()` — the same construction `resolve_actor`
/// verifies.
fn session_auth_hash(password_field: &str, secret_key: &[u8]) -> String {
    use hmac::{Hmac, Mac};
    use sha2::{Digest, Sha256};
    let key = {
        let mut hasher = Sha256::new();
        hasher.update(b"django.contrib.auth.models.AbstractBaseUser.get_session_auth_hash");
        hasher.update(secret_key);
        hasher.finalize()
    };
    let mut mac = Hmac::<Sha256>::new_from_slice(&key).expect("HMAC-SHA256 accepts any key length");
    mac.update(password_field.as_bytes());
    let digest = mac.finalize().into_bytes();
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(digest.len() * 2);
    for b in digest {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

/// `user_login(user, request, is_app=True)` (`utils/login.py:14-28`) over
/// the request session: Django `login()` cycles the session key, writes
/// `_auth_user_id`/`_auth_user_backend`/`_auth_user_hash` plus the
/// `device_info` dict, and rotates the CSRF secret. The middleware
/// persists the mutated session and re-sets the session cookie; the
/// fresh `csrftoken` cookie is appended here. Returns the new CSRF
/// secret for the `Set-Cookie` value.
fn relogin(
    handle: &crate::middleware::SessionHandle,
    user_id: &str,
    password_field: &str,
    secret_key: &[u8],
    user_agent: &str,
    ip: Option<&str>,
    domain: &str,
) -> String {
    let hash = session_auth_hash(password_field, secret_key);
    {
        let mut session = handle.lock();
        session.set(
            "_auth_user_id".to_owned(),
            Value::String(user_id.to_owned()),
        );
        session.set(
            "_auth_user_backend".to_owned(),
            Value::String(crate::license::MODEL_BACKEND.to_owned()),
        );
        session.set("_auth_user_hash".to_owned(), Value::String(hash));
        session.set(
            "device_info".to_owned(),
            crate::license::handlers_auth_forms::device_info(user_agent, ip, domain),
        );
        // `login()` cycles the key: drop it so the save issues a fresh
        // 128-char one (the stale row keeps its old hash and reads as
        // anonymous, like Django's deleted row).
        session.key = None;
    }
    pidash_auth::csrf::new_secret()
}

/// Append the rotated `csrftoken` cookie (`login()` calls
/// `rotate_token()`, so the browser's secret changes on every re-login).
fn push_csrf_cookie(response: &mut Response, secret: &str, secure: bool, now: i64) {
    let cookie = pw::csrf_set_cookie_value(
        secret,
        &pidash_auth::session::http_date(now + pw::CSRF_COOKIE_AGE_SECS),
        secure,
    );
    if let Ok(value) = header::HeaderValue::from_str(&cookie) {
        response.headers_mut().append(header::SET_COOKIE, value);
    }
}

fn user_agent_of(headers: &HeaderMap) -> String {
    headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_owned()
}

// ---------------------------------------------------------------------------
// Change-password / set-password
// ---------------------------------------------------------------------------

/// The `users` row a password write needs: `(password, is_password_autoset)`.
async fn password_row(
    pool: &sqlx::PgPool,
    user_id: uuid::Uuid,
) -> Result<Option<(String, bool)>, sqlx::Error> {
    use sqlx::Row;
    let row: Option<sqlx::postgres::PgRow> =
        sqlx::query("SELECT password, is_password_autoset FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_optional(pool)
            .await?;
    row.map(|row| {
        Ok::<_, sqlx::Error>((
            row.try_get("password")?,
            row.try_get("is_password_autoset")?,
        ))
    })
    .transpose()
}

/// `POST /auth/change-password/` (`ChangePasswordEndpoint`,
/// `common.py:47-96`).
async fn change_password(
    State(state): State<AppState>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    headers: HeaderMap,
    peer: PeerAddr,
    body: axum::body::Bytes,
) -> Response {
    let Some(pool) = pool_of(&state) else {
        return server_error();
    };
    // DRF order: authentication, permission (`IsAuthenticated`),
    // throttles (default Anon: authenticated callers skip).
    let actor = match authed_actor(&state, &pool, extension.clone()).await {
        Ok(actor) => actor,
        Err(fail) => return actor_fail(fail),
    };
    let data = match read_body(&headers, &body) {
        Ok(data) => data,
        Err(error) => return error.into_response(),
    };
    if data_get_raises(&data) {
        return server_error();
    }
    // `SessionAuthentication` CSRF, after authentication like DRF.
    if let Err(reason) = check_csrf_token(&data, &headers, true) {
        return csrf_denied(&reason);
    }
    let peer_addr = peer.0;
    if let Err(wait) = throttle_gate(
        &state,
        pidash_services::auth_session::DEFAULT_ANON_SCOPE,
        pidash_services::auth_session::DEFAULT_ANON_RATE,
        true,
        &throttle_ident(&headers, peer_addr),
    )
    .await
    {
        return throttled(wait);
    }
    let old = data_field(&data, "old_password");
    let new = data_field(&data, "new_password");
    // Non-string values crash `zxcvbn`/`check_password` (`TypeError`,
    // i.e. 500) everywhere except the falsy-missing branches.
    if matches!(old, Field::Opaque) || matches!(new, Field::Opaque) {
        return server_error();
    }
    let old_present = matches!(old, Field::Text(_));
    let new_text = match &new {
        Field::Text(text) => Some(text.clone()),
        _ => None,
    };
    let row = match password_row(&pool, actor.id).await {
        Ok(Some(row)) => row,
        _ => return server_error(),
    };
    let (stored, is_autoset) = row;
    let old_matches = match &old {
        Field::Text(candidate) => {
            match pidash_auth::password::verify_password(candidate, &stored) {
                Ok(matches) => matches,
                Err(_) => return server_error(),
            }
        }
        _ => false,
    };
    let new_weak = new_text
        .as_deref()
        .map(pidash_services::auth_session::password::password_is_weak)
        .unwrap_or(false);
    let outcome = pw::decide_change_password(
        is_autoset,
        old_present,
        new_text.is_some(),
        old_matches,
        new_weak,
    );
    if outcome != pw::ChangePasswordOutcome::Ok {
        return json_400(&pw::change_password_error_pairs(outcome));
    }
    let new_password = new_text.expect("decided Ok");
    let salt = pw::generate_password_salt();
    let encoded = pidash_auth::password::hash_password(
        &new_password,
        &salt,
        pidash_auth::password::PBKDF2_DEFAULT_ITERATIONS,
    );
    // `set_password` + `is_password_autoset = False` + `save()`; the
    // `user_logged_in` signal bumps `last_login` (`update_last_login`).
    if sqlx::query(
        "UPDATE users SET password = $1, is_password_autoset = false, last_login = now() WHERE id = $2",
    )
    .bind(&encoded)
    .bind(actor.id)
    .execute(&pool)
    .await
    .is_err()
    {
        return server_error();
    }
    // `user_login(user, request, is_app=True)`: re-login over the
    // request session plus a rotated CSRF secret.
    let now = now_unix();
    let secure = state.settings().session.cookie_secure;
    let mut response = json_response(StatusCode::OK, pw::CHANGE_PASSWORD_SUCCESS_BODY.to_owned());
    if let Some(axum::Extension(handle)) = extension {
        let domain = base_host_for(&state, true, false);
        let ip = client_ip(&headers, peer_addr);
        let csrf_secret = relogin(
            &handle,
            &actor.id.to_string(),
            &encoded,
            state.settings().secret_key.as_bytes(),
            &user_agent_of(&headers),
            ip.as_deref(),
            &domain,
        );
        push_csrf_cookie(&mut response, &csrf_secret, secure, now);
    }
    response
}

/// `POST /auth/set-password/` (`SetUserPasswordEndpoint`,
/// `common.py:99-138`).
async fn set_password(
    State(state): State<AppState>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    headers: HeaderMap,
    peer: PeerAddr,
    body: axum::body::Bytes,
) -> Response {
    let Some(pool) = pool_of(&state) else {
        return server_error();
    };
    let actor = match authed_actor(&state, &pool, extension.clone()).await {
        Ok(actor) => actor,
        Err(fail) => return actor_fail(fail),
    };
    let data = match read_body(&headers, &body) {
        Ok(data) => data,
        Err(error) => return error.into_response(),
    };
    if data_get_raises(&data) {
        return server_error();
    }
    if let Err(reason) = check_csrf_token(&data, &headers, true) {
        return csrf_denied(&reason);
    }
    let peer_addr = peer.0;
    if let Err(wait) = throttle_gate(
        &state,
        pidash_services::auth_session::DEFAULT_ANON_SCOPE,
        pidash_services::auth_session::DEFAULT_ANON_RATE,
        true,
        &throttle_ident(&headers, peer_addr),
    )
    .await
    {
        return throttled(wait);
    }
    let password = data_field(&data, "password");
    if matches!(password, Field::Opaque) {
        return server_error();
    }
    let password_text = match &password {
        Field::Text(text) => Some(text.clone()),
        _ => None,
    };
    let row = match password_row(&pool, actor.id).await {
        Ok(Some(row)) => row,
        _ => return server_error(),
    };
    let (_, is_autoset) = row;
    let password_weak = password_text
        .as_deref()
        .map(pidash_services::auth_session::password::password_is_weak)
        .unwrap_or(false);
    let outcome = pw::decide_set_password(is_autoset, password_text.is_some(), password_weak);
    if outcome != pw::SetPasswordOutcome::Ok {
        return json_400(&pw::set_password_error_pairs(outcome));
    }
    let password_value = password_text.expect("decided Ok");
    let salt = pw::generate_password_salt();
    let encoded = pidash_auth::password::hash_password(
        &password_value,
        &salt,
        pidash_auth::password::PBKDF2_DEFAULT_ITERATIONS,
    );
    if sqlx::query(
        "UPDATE users SET password = $1, is_password_autoset = false, last_login = now() WHERE id = $2",
    )
    .bind(&encoded)
    .bind(actor.id)
    .execute(&pool)
    .await
    .is_err()
    {
        return server_error();
    }
    // `invalidate_cache("/api/users/me/")` deletes a key no reader ever
    // sets (`cache_response` never wraps a users/me view), so there is no
    // cache I/O to port — the observable behavior is identical.
    let snapshot = match user_snapshot(&pool, actor.id).await {
        Ok(Some(snapshot)) => snapshot,
        _ => return server_error(),
    };
    let pairs = pw::user_serializer_pairs(&snapshot);
    let body = crate::auth_session::json_error_string(&pairs);
    let now = now_unix();
    let secure = state.settings().session.cookie_secure;
    let mut response = json_response(StatusCode::OK, body);
    if let Some(axum::Extension(handle)) = extension {
        let domain = base_host_for(&state, true, false);
        let ip = client_ip(&headers, peer_addr);
        let csrf_secret = relogin(
            &handle,
            &actor.id.to_string(),
            &encoded,
            state.settings().secret_key.as_bytes(),
            &user_agent_of(&headers),
            ip.as_deref(),
            &domain,
        );
        push_csrf_cookie(&mut response, &csrf_secret, secure, now);
    }
    response
}

// ---------------------------------------------------------------------------
// Forgot-password (app + space)
// ---------------------------------------------------------------------------

/// The `users` row the forgot-password branch needs: `(id, first_name,
/// email, password, last_login)`.
struct ForgotUser {
    id: uuid::Uuid,
    first_name: String,
    email: String,
    password: String,
    last_login_unix: Option<i64>,
}

async fn forgot_user_by_email(
    pool: &sqlx::PgPool,
    email: &str,
) -> Result<Option<ForgotUser>, sqlx::Error> {
    // `User.objects.filter(email=email).first()` (exact match, default
    // `-created_at` ordering).
    use sqlx::Row;
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(
        "SELECT id, first_name, email, password, last_login FROM users WHERE email = $1 ORDER BY created_at DESC LIMIT 1",
    )
    .bind(email)
    .fetch_optional(pool)
    .await?;
    row.map(|row| {
        Ok::<_, sqlx::Error>(ForgotUser {
            id: row.try_get("id")?,
            first_name: row
                .try_get::<Option<String>, _>("first_name")?
                .unwrap_or_default(),
            email: row
                .try_get::<Option<String>, _>("email")?
                .unwrap_or_default(),
            password: row.try_get("password")?,
            last_login_unix: row
                .try_get::<Option<chrono::DateTime<chrono::Utc>>, _>("last_login")?
                .map(|dt| dt.timestamp()),
        })
    })
    .transpose()
}

/// `EMAIL_HOST` through the legacy resolver
/// (`get_configuration_value([{"key": "EMAIL_HOST", "default":
/// os.environ.get("EMAIL_HOST")}])`, source `db` per the registry).
async fn smtp_configured(pool: &sqlx::PgPool, secret_key: &str) -> Result<bool, ()> {
    use pidash_db::config::accessor::PgConfigStore;
    use pidash_db::config::legacy::{get_configuration_values, LegacyItem};
    use pidash_db::config::value::ConfigValue;
    let default = match std::env::var("EMAIL_HOST") {
        Ok(value) => ConfigValue::Str(value),
        Err(_) => ConfigValue::Null,
    };
    let store = PgConfigStore::new(pool.clone());
    let keyring = pidash_services::license::encryption::Keyring::from_secret(secret_key);
    let values = get_configuration_values(
        pidash_db::config::registry::global(),
        &store,
        &keyring,
        &[LegacyItem::new("EMAIL_HOST", default)],
    )
    .await
    .map_err(|_| ())?;
    Ok(match values.first() {
        Some(ConfigValue::Str(value)) => !value.is_empty(),
        _ => false,
    })
}

/// Publish `forgot_password.delay(first_name, email, uidb64, token,
/// current_site)` in Celery v2 wire format. A refused publish is the
/// `.delay()` raising, i.e. HTTP 500.
async fn publish_forgot_password(
    first_name: &str,
    email: &str,
    uidb64: &str,
    token: &str,
    current_site: &str,
) -> Result<(), ()> {
    let message = pidash_jobs::CeleryTaskMessage::new(
        t::FORGOT_PASSWORD_TASK,
        vec![
            Value::String(first_name.to_owned()),
            Value::String(email.to_owned()),
            Value::String(uidb64.to_owned()),
            Value::String(token.to_owned()),
            Value::String(current_site.to_owned()),
        ],
        serde_json::Map::new(),
    );
    let config = pidash_jobs::AmqpConfig::from_env().map_err(|_| ())?;
    let publisher = pidash_jobs::Publisher::connect(&config)
        .await
        .map_err(|_| ())?;
    let result = publisher.publish(&message).await;
    let _ = publisher.close().await;
    result.map_err(|_| ())
}

/// Shared forgot-password flow; `is_space` selects the view twin (the
/// space twin additionally reads `EMAIL_HOST_USER`/`EMAIL_HOST_PASSWORD`
/// but gates on `EMAIL_HOST` only — ported quirk).
async fn forgot_password(
    state: AppState,
    headers: HeaderMap,
    peer: Option<std::net::SocketAddr>,
    body: axum::body::Bytes,
    is_space: bool,
) -> Response {
    let Some(pool) = pool_of(&state) else {
        return server_error();
    };
    let data = match read_body(&headers, &body) {
        Ok(data) => data,
        Err(error) => return error.into_response(),
    };
    if data_get_raises(&data) {
        return server_error();
    }
    // DRF order: authentication (nobody here — callers are anonymous),
    // permission (`AllowAny`), then `AuthenticationThrottle`.
    if let Err(wait) = throttle_gate(
        &state,
        pidash_services::auth_session::AUTHENTICATION_THROTTLE_SCOPE,
        pidash_services::auth_session::AUTHENTICATION_THROTTLE_RATE,
        false,
        &throttle_ident(&headers, peer),
    )
    .await
    {
        return throttled(wait);
    }
    // Instance gate (`Instance.objects.first()`, `is_setup_done`): a
    // missing row or an unset flag answers INSTANCE_NOT_CONFIGURED.
    let instance_row = match pidash_db::license::queries::fetch_instance_first(&pool).await {
        Ok(row) => row,
        Err(_) => return server_error(),
    };
    let instance_setup_done = instance_row.map(|row| row.is_setup_done).unwrap_or(false);
    // SMTP gate (`EMAIL_HOST` truthiness).
    let Ok(smtp) = smtp_configured(&pool, &state.settings().secret_key).await else {
        return server_error();
    };
    // `validate_email`: the value is `str()`-coerced first, so every
    // JSON scalar degrades to an invalid email rather than a 500.
    let email = match data_field(&data, "email") {
        Field::Text(value) => value,
        Field::Absent => String::new(),
        Field::Opaque => String::new(),
    };
    let email_valid = crate::license::handlers_auth_forms::email_is_valid(&email);
    let user = match forgot_user_by_email(&pool, &email).await {
        Ok(user) => user,
        Err(_) => return server_error(),
    };
    let decision =
        t::forgot_password_decision(instance_setup_done, smtp, email_valid, user.is_some());
    if decision != t::ForgotPasswordDecision::Emit {
        return json_400(&pw::forgot_password_error_pairs(decision));
    }
    let user = user.expect("decided Emit");
    let current_site = base_host_for(&state, !is_space, is_space);
    let now = now_unix();
    let (uidb64, token) = pw::make_password_token(
        &state.settings().secret_key,
        &user.id.to_string(),
        &user.password,
        user.last_login_unix,
        &user.email,
        now,
    );
    if publish_forgot_password(
        &user.first_name,
        &user.email,
        &uidb64,
        &token,
        &current_site,
    )
    .await
    .is_err()
    {
        // `.delay()` raising answers 500 in Django; same here.
        return server_error();
    }
    json_response(StatusCode::OK, pw::FORGOT_PASSWORD_SUCCESS_BODY.to_owned())
}

/// `POST /auth/forgot-password/` (`ForgotPasswordEndpoint`).
async fn forgot_password_app(
    State(state): State<AppState>,
    headers: HeaderMap,
    peer: PeerAddr,
    body: axum::body::Bytes,
) -> Response {
    forgot_password(state, headers, peer.0, body, false).await
}

/// `POST /auth/spaces/forgot-password/` (`ForgotPasswordSpaceEndpoint`).
async fn forgot_password_space(
    State(state): State<AppState>,
    headers: HeaderMap,
    peer: PeerAddr,
    body: axum::body::Bytes,
) -> Response {
    forgot_password(state, headers, peer.0, body, true).await
}

// ---------------------------------------------------------------------------
// Reset-password (app + space)
// ---------------------------------------------------------------------------

/// The `users` row the reset flow needs: `(password, last_login, email)`.
struct ResetUser {
    password: String,
    last_login_unix: Option<i64>,
    email: String,
}

async fn reset_user_by_id(
    pool: &sqlx::PgPool,
    user_id: uuid::Uuid,
) -> Result<Option<ResetUser>, sqlx::Error> {
    // `User.objects.get(id=id)`.
    use sqlx::Row;
    let row: Option<sqlx::postgres::PgRow> =
        sqlx::query("SELECT password, last_login, email FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_optional(pool)
            .await?;
    row.map(|row| {
        Ok::<_, sqlx::Error>(ResetUser {
            password: row.try_get("password")?,
            last_login_unix: row
                .try_get::<Option<chrono::DateTime<chrono::Utc>>, _>("last_login")?
                .map(|dt| dt.timestamp()),
            email: row
                .try_get::<Option<String>, _>("email")?
                .unwrap_or_default(),
        })
    })
    .transpose()
}

/// Shared reset-password flow; `is_space` selects the view twin. Plain
/// Django views: no DRF layer, no throttle — but `CsrfViewMiddleware`
/// rejects unsafe requests with the custom `csrf_failure` page (200).
async fn reset_password(
    state: AppState,
    headers: HeaderMap,
    body: axum::body::Bytes,
    uidb64: String,
    token: String,
    is_space: bool,
) -> Response {
    let Some(pool) = pool_of(&state) else {
        return server_error();
    };
    // Form only: a JSON body is invisible to `request.POST` and reads as
    // a missing password below (multipart likewise reads as empty).
    let data = BodyData {
        json: None,
        form: match content_type_of(&headers)
            .unwrap_or_default()
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .to_lowercase()
            .as_str()
        {
            "application/x-www-form-urlencoded" | "" => parse_form(&body),
            _ => Vec::new(),
        },
    };
    if let Err(reason) = check_csrf_token(&data, &headers, true) {
        let _ = reason;
        return csrf_failure_page(&base_host_for(&state, false, false));
    }
    let secret = state.settings().secret_key.clone();
    // Decode (`smart_str(urlsafe_base64_decode(uidb64))`).
    let pk = match pw::uidb64_decode(&uidb64) {
        Ok(pk) => pk,
        Err(error) => {
            // App: the inner `except (ValueError, User.DoesNotExist)`
            // catches every decode error, including the
            // `DjangoUnicodeDecodeError` subclass — the outer EXPIRED
            // branch is dead (`:103-116` vs `:167-176`).
            // Space: only `DjangoUnicodeDecodeError` maps to a redirect
            // (EXPIRED, `:154-160`); a `binascii.Error` escapes (500).
            if !is_space {
                let (code, message) =
                    pw::reset_app_failure_params(pw::ResetAppFailure::InvalidToken);
                let base = base_host_for(&state, true, false);
                return redirect_302(pw::reset_app_error_location(&base, code, message));
            }
            match error {
                pw::UidDecodeError::BadUtf8 => {
                    let (code, message) =
                        pw::reset_space_failure_params(pw::ResetSpaceFailure::ExpiredToken);
                    let base = base_host_for(&state, false, true);
                    return redirect_302(pw::reset_space_error_location(&base, code, message));
                }
                pw::UidDecodeError::BadEncoding => return server_error(),
            }
        }
    };
    // Lookup (`User.objects.get(id=id)`): a non-UUID pk raises
    // `ValidationError` (500 on both twins); an unknown id raises
    // `DoesNotExist` (INVALID on app, 500 on space).
    let user_id = match pk.parse::<uuid::Uuid>() {
        Ok(id) => id,
        Err(_) => return server_error(),
    };
    let user = match reset_user_by_id(&pool, user_id).await {
        Ok(user) => user,
        Err(_) => return server_error(),
    };
    let Some(user) = user else {
        if !is_space {
            let (code, message) = pw::reset_app_failure_params(pw::ResetAppFailure::InvalidToken);
            let base = base_host_for(&state, true, false);
            return redirect_302(pw::reset_app_error_location(&base, code, message));
        }
        return server_error();
    };
    // Token check: false answers INVALID_PASSWORD_TOKEN on both twins.
    if !pw::check_password_token(
        &secret,
        &user_id.to_string(),
        &user.password,
        user.last_login_unix,
        &user.email,
        &token,
        now_unix(),
    ) {
        return reset_invalid_redirect(&state, is_space);
    }
    // Password gate: missing/empty answers INVALID_PASSWORD,
    // `zxcvbn < 3` answers PASSWORD_TOO_WEAK.
    let new_password = match post_field(&data, "password") {
        Field::Text(password) => password,
        _ => {
            return reset_failure_redirect(&state, is_space, GateFailure::Missing);
        }
    };
    if pw::password_is_weak(&new_password) {
        return reset_failure_redirect(&state, is_space, GateFailure::Weak);
    }
    // `set_password` + `is_password_autoset = False` + `save()` — no
    // `login()` call, so no session or CSRF rotation and no `last_login`
    // bump.
    let salt = pw::generate_password_salt();
    let encoded = pidash_auth::password::hash_password(
        &new_password,
        &salt,
        pidash_auth::password::PBKDF2_DEFAULT_ITERATIONS,
    );
    if sqlx::query("UPDATE users SET password = $1, is_password_autoset = false WHERE id = $2")
        .bind(&encoded)
        .bind(user_id)
        .execute(&pool)
        .await
        .is_err()
    {
        return server_error();
    }
    if !is_space {
        let base = base_host_for(&state, true, false);
        redirect_302(pw::reset_app_success_location(&base))
    } else {
        let base = base_host_for(&state, false, true);
        redirect_302(pw::reset_space_success_location(&base))
    }
}

/// INVALID_PASSWORD_TOKEN redirect on the calling twin.
fn reset_invalid_redirect(state: &AppState, is_space: bool) -> Response {
    if !is_space {
        let (code, message) = pw::reset_app_failure_params(pw::ResetAppFailure::InvalidToken);
        let base = base_host_for(state, true, false);
        redirect_302(pw::reset_app_error_location(&base, code, message))
    } else {
        let (code, message) = pw::reset_space_failure_params(pw::ResetSpaceFailure::InvalidToken);
        let base = base_host_for(state, false, true);
        redirect_302(pw::reset_space_error_location(&base, code, message))
    }
}

/// Which password-gate branch fired (INVALID_PASSWORD vs
/// PASSWORD_TOO_WEAK).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GateFailure {
    Missing,
    Weak,
}

fn reset_failure_redirect(state: &AppState, is_space: bool, failure: GateFailure) -> Response {
    if !is_space {
        let failure = match failure {
            GateFailure::Missing => pw::ResetAppFailure::MissingPassword,
            GateFailure::Weak => pw::ResetAppFailure::TooWeak,
        };
        let (code, message) = pw::reset_app_failure_params(failure);
        let base = base_host_for(state, true, false);
        redirect_302(pw::reset_app_error_location(&base, code, message))
    } else {
        let failure = match failure {
            GateFailure::Missing => pw::ResetSpaceFailure::MissingPassword,
            GateFailure::Weak => pw::ResetSpaceFailure::TooWeak,
        };
        let (code, message) = pw::reset_space_failure_params(failure);
        let base = base_host_for(state, false, true);
        redirect_302(pw::reset_space_error_location(&base, code, message))
    }
}

/// `POST /auth/reset-password/<uidb64>/<token>/`
/// (`ResetPasswordEndpoint`).
async fn reset_password_app(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((uidb64, token)): Path<(String, String)>,
    body: axum::body::Bytes,
) -> Response {
    reset_password(state, headers, body, uidb64, token, false).await
}

/// `POST /auth/spaces/reset-password/<uidb64>/<token>/`
/// (`ResetPasswordSpaceEndpoint`).
async fn reset_password_space(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((uidb64, token)): Path<(String, String)>,
    body: axum::body::Bytes,
) -> Response {
    reset_password(state, headers, body, uidb64, token, true).await
}

// ---------------------------------------------------------------------------
// Set-password `UserSerializer` row
// ---------------------------------------------------------------------------

/// Read the full `UserSerializer` row in `USER_SERIALIZER_FIELDS` order
/// (`User._meta` field order minus `password`). Column-by-column decode:
/// 39 columns exceed sqlx's tuple `FromRow` arity.
async fn user_snapshot(
    pool: &sqlx::PgPool,
    user_id: uuid::Uuid,
) -> Result<Option<pw::UserSnapshot>, sqlx::Error> {
    use sqlx::Row;
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(
        "SELECT last_login, id, username, mobile_number, email, display_name, first_name, \
         last_name, avatar, avatar_asset_id, cover_image, cover_image_asset_id, date_joined, \
         created_at, updated_at, last_location, created_location, is_superuser, is_managed, \
         is_password_expired, is_active, is_staff, is_email_verified, is_password_autoset, \
         is_password_reset_required, token, last_active, last_login_time, last_logout_time, \
         last_login_ip, last_logout_ip, last_login_medium, last_login_uagent, token_updated_at, \
         is_bot, bot_type, user_timezone, is_email_valid, masked_at \
         FROM users WHERE id = $1",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await?;
    row.map(|row| {
        let dt = |value: chrono::DateTime<chrono::Utc>| {
            (value.timestamp(), value.timestamp_subsec_micros())
        };
        Ok::<_, sqlx::Error>(pw::UserSnapshot {
            last_login: row
                .try_get::<Option<chrono::DateTime<chrono::Utc>>, _>("last_login")?
                .map(dt),
            id: row.try_get::<uuid::Uuid, _>("id")?.to_string(),
            username: row.try_get("username")?,
            mobile_number: row.try_get("mobile_number")?,
            email: row.try_get("email")?,
            display_name: row
                .try_get::<Option<String>, _>("display_name")?
                .unwrap_or_default(),
            first_name: row
                .try_get::<Option<String>, _>("first_name")?
                .unwrap_or_default(),
            last_name: row
                .try_get::<Option<String>, _>("last_name")?
                .unwrap_or_default(),
            avatar: row
                .try_get::<Option<String>, _>("avatar")?
                .unwrap_or_default(),
            avatar_asset: row
                .try_get::<Option<uuid::Uuid>, _>("avatar_asset_id")?
                .map(|id| id.to_string()),
            cover_image: row.try_get("cover_image")?,
            cover_image_asset: row
                .try_get::<Option<uuid::Uuid>, _>("cover_image_asset_id")?
                .map(|id| id.to_string()),
            date_joined: dt(row.try_get("date_joined")?),
            created_at: dt(row.try_get("created_at")?),
            updated_at: dt(row.try_get("updated_at")?),
            last_location: row
                .try_get::<Option<String>, _>("last_location")?
                .unwrap_or_default(),
            created_location: row
                .try_get::<Option<String>, _>("created_location")?
                .unwrap_or_default(),
            is_superuser: row.try_get("is_superuser")?,
            is_managed: row.try_get("is_managed")?,
            is_password_expired: row.try_get("is_password_expired")?,
            is_active: row.try_get("is_active")?,
            is_staff: row.try_get("is_staff")?,
            is_email_verified: row.try_get("is_email_verified")?,
            is_password_autoset: row.try_get("is_password_autoset")?,
            is_password_reset_required: row.try_get("is_password_reset_required")?,
            token: row
                .try_get::<Option<String>, _>("token")?
                .unwrap_or_default(),
            last_active: row
                .try_get::<Option<chrono::DateTime<chrono::Utc>>, _>("last_active")?
                .map(dt),
            last_login_time: row
                .try_get::<Option<chrono::DateTime<chrono::Utc>>, _>("last_login_time")?
                .map(dt),
            last_logout_time: row
                .try_get::<Option<chrono::DateTime<chrono::Utc>>, _>("last_logout_time")?
                .map(dt),
            last_login_ip: row
                .try_get::<Option<String>, _>("last_login_ip")?
                .unwrap_or_default(),
            last_logout_ip: row
                .try_get::<Option<String>, _>("last_logout_ip")?
                .unwrap_or_default(),
            last_login_medium: row
                .try_get::<Option<String>, _>("last_login_medium")?
                .unwrap_or_else(|| "email".to_owned()),
            last_login_uagent: row
                .try_get::<Option<String>, _>("last_login_uagent")?
                .unwrap_or_default(),
            token_updated_at: row
                .try_get::<Option<chrono::DateTime<chrono::Utc>>, _>("token_updated_at")?
                .map(dt),
            is_bot: row.try_get("is_bot")?,
            bot_type: row.try_get("bot_type")?,
            user_timezone: row
                .try_get::<Option<String>, _>("user_timezone")?
                .unwrap_or_else(|| "UTC".to_owned()),
            is_email_valid: row.try_get("is_email_valid")?,
            masked_at: row
                .try_get::<Option<chrono::DateTime<chrono::Utc>>, _>("masked_at")?
                .map(dt),
        })
    })
    .transpose()
}

/// `base_host(request, ...)` over the process settings.
fn base_host_for(state: &AppState, is_app: bool, is_space: bool) -> String {
    let urls = &state.settings().urls;
    let host = pidash_services::auth_session::HostSettings {
        web_url: urls.web_url.as_deref(),
        app_base_url: urls.app_base_url.as_deref(),
        admin_base_url: urls.admin_base_url.as_deref(),
        space_base_url: urls.space_base_url.as_deref(),
        admin_base_path: Some(urls.admin_base_path.as_str()),
        space_base_path: Some(urls.space_base_path.as_str()),
    };
    pidash_services::auth_session::base_host(&host, false, is_space, is_app)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use tower::ServiceExt;

    fn state() -> AppState {
        // No pools: every handler answers 500 through `pool_of`, which
        // proves the failure is closed, never a panic.
        AppState::new("0.1.0")
    }

    #[test]
    fn form_decoding_matches_querydict() {
        assert_eq!(
            parse_form(b"password=New-Pass-1!x&csrfmiddlewaretoken=abc"),
            vec![
                ("password".to_owned(), "New-Pass-1!x".to_owned()),
                ("csrfmiddlewaretoken".to_owned(), "abc".to_owned()),
            ]
        );
        // `+` is a space, `%XX` decodes, last value wins.
        assert_eq!(
            parse_form(b"a=x+y&a=z%20w&empty=&flag"),
            vec![
                ("a".to_owned(), "x y".to_owned()),
                ("a".to_owned(), "z w".to_owned()),
                ("empty".to_owned(), String::new()),
                ("flag".to_owned(), String::new()),
            ]
        );
        let data = BodyData {
            json: None,
            form: parse_form(b"a=x&a=y"),
        };
        assert_eq!(post_field(&data, "a"), Field::Text("y".to_owned()));
        assert_eq!(post_field(&data, "missing"), Field::Absent);
        assert_eq!(post_field(&data, "empty"), Field::Absent);
    }

    #[test]
    fn data_field_merges_json_and_form() {
        let json = BodyData {
            json: Some(serde_json::json!({
                "old_password": "x",
                "new_password": "",
                "n": 5,
                "f": false,
                "t": true,
                "nil": null,
            })),
            form: Vec::new(),
        };
        assert_eq!(
            data_field(&json, "old_password"),
            Field::Text("x".to_owned())
        );
        assert_eq!(data_field(&json, "new_password"), Field::Absent);
        assert_eq!(data_field(&json, "n"), Field::Opaque);
        assert_eq!(data_field(&json, "f"), Field::Absent);
        assert_eq!(data_field(&json, "t"), Field::Opaque);
        assert_eq!(data_field(&json, "nil"), Field::Absent);
        assert_eq!(data_field(&json, "missing"), Field::Absent);
        assert!(!data_get_raises(&json));
        // Python-falsy scalars and empties read as missing.
        let falsy = BodyData {
            json: Some(serde_json::json!({
                "zero": 0,
                "zero_float": 0.0,
                "empty_list": [],
                "empty_dict": {},
            })),
            form: Vec::new(),
        };
        assert_eq!(data_field(&falsy, "zero"), Field::Absent);
        assert_eq!(data_field(&falsy, "zero_float"), Field::Absent);
        assert_eq!(data_field(&falsy, "empty_list"), Field::Absent);
        assert_eq!(data_field(&falsy, "empty_dict"), Field::Absent);
        // A non-object JSON body has no `.get` (`AttributeError`, 500).
        for raw in ["[1]", "5", "\"x\"", "true", "null"] {
            let data = BodyData {
                json: Some(serde_json::from_str(raw).expect("json")),
                form: Vec::new(),
            };
            assert!(data_get_raises(&data), "{raw}");
        }
        assert!(!data_get_raises(&BodyData::default()));
    }

    #[test]
    fn csrf_reasons_match_django_strings() {
        let empty = HeaderMap::new();
        let data = BodyData::default();
        assert_eq!(
            check_csrf_token(&data, &empty, true),
            Err("CSRF cookie not set.".to_owned())
        );
        // Malformed cookie secret fails closed at the format gate.
        let mut bad_cookie = HeaderMap::new();
        bad_cookie.insert(
            header::COOKIE,
            header::HeaderValue::from_static("csrftoken=short"),
        );
        assert_eq!(
            check_csrf_token(&data, &bad_cookie, true),
            Err("CSRF cookie has incorrect length.".to_owned())
        );
        // Valid secret round-trips through the masked token form.
        let secret = pidash_auth::csrf::new_secret();
        let masked = pidash_auth::csrf::mask_secret(&secret).expect("masks");
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            header::HeaderValue::from_str(&format!("csrftoken={secret}")).expect("cookie"),
        );
        headers.insert(
            "x-csrftoken",
            header::HeaderValue::from_str(&masked).expect("token"),
        );
        assert_eq!(check_csrf_token(&data, &headers, true), Ok(()));
        // A wrong token names the header source like Django.
        headers.insert(
            "x-csrftoken",
            header::HeaderValue::from_static(
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            ),
        );
        assert_eq!(
            check_csrf_token(&data, &headers, true),
            Err("CSRF token from the 'X-Csrftoken' HTTP header incorrect.".to_owned())
        );
        // A missing token (header and POST field both empty) is reported.
        let mut bare = HeaderMap::new();
        bare.insert(
            header::COOKIE,
            header::HeaderValue::from_str(&format!("csrftoken={secret}")).expect("cookie"),
        );
        assert_eq!(
            check_csrf_token(&data, &bare, true),
            Err("CSRF token missing.".to_owned())
        );
    }

    #[tokio::test]
    async fn pool_less_handlers_fail_closed() {
        // Seven owned routes, all 500 without pools (never a panic, never
        // a Rust 404 that would mask Django's).
        let app = password_routes().with_state(state());
        for (method, path) in [
            ("GET", "/auth/get-csrf-token/"),
            ("POST", "/auth/change-password/"),
            ("POST", "/auth/set-password/"),
            ("POST", "/auth/forgot-password/"),
            ("POST", "/auth/spaces/forgot-password/"),
            ("POST", "/auth/reset-password/dWlk/bogus-token/"),
            ("POST", "/auth/spaces/reset-password/dWlk/bogus-token/"),
        ] {
            let request = axum::http::Request::builder()
                .method(method)
                .uri(path)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{}"))
                .expect("request");
            let response = app.clone().oneshot(request).await.expect("serve");
            assert_eq!(
                response.status(),
                StatusCode::INTERNAL_SERVER_ERROR,
                "{method} {path}"
            );
        }
        // Unowned methods proxy (502 without an upstream here, never a
        // Rust 405).
        let request = axum::http::Request::builder()
            .method("GET")
            .uri("/auth/change-password/")
            .body(Body::empty())
            .expect("request");
        let edge_state = AppState::with_edge(
            "0.1.0",
            crate::edge::EdgeHandle::for_tests("http://127.0.0.1:1"),
        );
        let app = password_routes().with_state(edge_state);
        let response = app.oneshot(request).await.expect("serve");
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    }
}
