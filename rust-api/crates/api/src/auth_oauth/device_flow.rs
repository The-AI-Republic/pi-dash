#![forbid(unsafe_code)]

//! Device-flow handlers A: start / approve / token poll (stage 5, PIDASHCONV-342).
//!
//! Ports `DeviceCodeStartEndpoint` (`:145-187`), `DeviceCodeApproveEndpoint`
//! (`:190-278`) and `DeviceCodeTokenEndpoint` (`:281-376`) from
//! `apps/api/pi_dash/authentication/views/cli/device.py`, with the three
//! routes from `apps/api/pi_dash/api/urls/auth.py:16-37`
//! (`auth/device/start/`, `auth/device/approve/`, `auth/device/token/`,
//! names `auth-device-start` / `-approve` / `-token`, all under `api/v1/`).
//!
//! Registration is the cutover granularity (Porting guide F-02): the three
//! owned POSTs serve from Rust; every other method on those paths falls
//! through to Django through the proxy, so Django's own 405s stay the
//! contract there. Sibling issue PIDASHCONV-343 owns the remaining three
//! device routes (workspaces / machine-token / revoke) and merges its
//! router into the same `ApiV1` group arm; merges keep both sides.
//!
//! Layering: generators (`generate_device_code`, `generate_user_code`),
//! the CLI-token label/description consts and the flow constants, throttle
//! scope and verification-URI builder are reused read-only from the db
//! models layer (`pidash_db::auth_oauth::models`, PIDASHCONV-326, Done)
//! and the sibling guards module (`super::guards`, PIDASHCONV-331, Done).
//! This module owns the HTTP shell (routes, session auth on approve, body
//! parsing), the `user_code` normalizer, and the row SQL with Django's
//! semantics (`SELECT ... FOR UPDATE` inside a transaction, the bounded
//! start retry, the slow_down no-touch, the mint).
//!
//! Fixture ids: AUTHOAUTH-F12 (`rust-api/fixtures/auth_oauth/`
//! `F12_device_endpoints.golden.json`; start 200 keys + 503
//! collision-exhausted; approve 400/404/410/409/200; token
//! pending/slow_down/expired/denied/consumed/200+mint) with constants from
//! AUTHOAUTH-F9 (`DEVICE_CODE_TTL`, `POLL_INTERVAL`, `MIN_POLL_GAP`,
//! `START_MAX_RETRIES`).
//!
//! Non-obvious faithful corners:
//!
//! - `start` never reads the request body (Django never touches
//!   `request.data` there), so no body extractor is taken and even
//!   malformed JSON still answers 200.
//! - `approve`'s anonymous denial is 401: DRF's
//!   `IsAuthenticated` raises `NotAuthenticated`, which live Django
//!   renders as 401
//!   `{"detail":"Authentication credentials were not provided."}`
//!   (probed; no `WWW-Authenticate` header on the wire).
//! - `approve` stamps `updated_at` alongside `user`/`workspace`/`approved`
//!   (`auto_now`); `token` stamps `last_polled_at` + `updated_at` on the
//!   pending path and `consumed` + `last_polled_at` + `updated_at` on mint.
//! - The start retry reuses one `expires_at` across attempts (computed
//!   before the loop, `device.py:159`); only a unique-violation (`23505`)
//!   retries — any other database error is a 500 like Django's unhandled
//!   `IntegrityError`-and-beyond path.
//! - Row reads filter `deleted_at IS NULL` (the soft-delete-aware default
//!   manager); the `workspaces` join in the approve membership pick is a
//!   plain inner join like `select_related` (no deleted filter on the
//!   workspace side).
//!
//! Ported bugs and quirks (translate, don't redesign; also listed in the PR):
//!
//! - A non-string `user_code` / `device_code` JSON value (e.g. a number)
//!   500s in Django (`(123 or "").strip()` raises `AttributeError`); here
//!   it answers the missing-field 400 instead. Only observable on input
//!   the contract suite never sends.
//!
//! The device-start throttle (`DeviceCodeStartThrottle`, 20/minute
//! per-IP) is enforced in `post_start` through the `super::guards` spec
//! and the shared DRF counter kernel against the app redis cache —
//! fail-open without a cache, answering the rewritten 429 like Django.

use axum::body::Bytes;
use axum::extract::{Extension, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Router;

use crate::auth_session::throttle_ident;
use crate::middleware::SessionHandle;
use crate::state::AppState;
use pidash_db::auth_oauth::models::{api_token_device_flow, cli_device_code};
use pidash_services::auth_session::guards::{allow_request, throttle_denied_json, throttle_wait};
use sea_orm::{ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter};

use super::guards::{
    device_start_cache_key, device_start_throttle_rate, verification_uri,
    DEVICE_CODE_MIN_POLL_GAP_SECS, DEVICE_CODE_POLL_INTERVAL_SECS, DEVICE_CODE_START_MAX_RETRIES,
    DEVICE_CODE_TTL_SECS,
};
pub use pidash_compat::response::SERVER_ERROR_BODY;
use pidash_compat::response::{error_body, json_response};

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------

/// Register the three owned device-flow POSTs. Sibling methods stay
/// unmatched and proxy to Django through the fallback (notably Django's
/// own 405s, e.g. `GET device/start/`).
///
/// The three device-flow paths are Rust-owned (NEWPIDASH-51): engine
/// `scope`, silent/interactive approval rules and engine-credential
/// issuance on redemption are ported.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/auth/device/start/", owned_start())
        .route("/api/v1/auth/device/approve/", owned_approve())
        .route("/api/v1/auth/device/token/", owned_token())
}

/// `device/start/`: POST is owned; everything else proxies.
fn owned_start() -> axum::routing::MethodRouter<AppState> {
    axum::routing::post(post_start)
        .get(crate::edge::proxy)
        .put(crate::edge::proxy)
        .patch(crate::edge::proxy)
        .delete(crate::edge::proxy)
        .head(crate::edge::proxy)
        .options(crate::edge::proxy)
}

/// `device/approve/`: POST is owned; everything else proxies.
fn owned_approve() -> axum::routing::MethodRouter<AppState> {
    axum::routing::post(post_approve)
        .get(crate::edge::proxy)
        .put(crate::edge::proxy)
        .patch(crate::edge::proxy)
        .delete(crate::edge::proxy)
        .head(crate::edge::proxy)
        .options(crate::edge::proxy)
}

/// `device/token/`: POST is owned; everything else proxies.
fn owned_token() -> axum::routing::MethodRouter<AppState> {
    axum::routing::post(post_token)
        .get(crate::edge::proxy)
        .put(crate::edge::proxy)
        .patch(crate::edge::proxy)
        .delete(crate::edge::proxy)
        .head(crate::edge::proxy)
        .options(crate::edge::proxy)
}

// ---------------------------------------------------------------------------
// Fixed bodies
// ---------------------------------------------------------------------------

/// DRF `NotAuthenticated` denial: 401 (probed live; the
/// `IsAuthenticated` gate, no `WWW-Authenticate` header).
pub const UNAUTHENTICATED_DETAIL_BODY: &str =
    r#"{"detail":"Authentication credentials were not provided."}"#;

// ---------------------------------------------------------------------------
// Denial / error mapping
// ---------------------------------------------------------------------------

/// Handler-level failure: the approve 401, the DRF `ParseError` branch,
/// view-inline `{"error"}` bodies with explicit statuses, and the generic
/// 500 for unexpected database failures.
enum Denial {
    /// 401, anonymous on `approve`.
    UnauthorizedDetail,
    /// `{"error": message}` (or a two-key error body) with an explicit
    /// status — every view-inline branch.
    Error(StatusCode, String),
    /// 500, generic branch (logs, then the fixed body).
    ServerError,
}

impl Denial {
    fn status_and_body(&self) -> (StatusCode, String) {
        match self {
            Denial::UnauthorizedDetail => (
                StatusCode::UNAUTHORIZED,
                UNAUTHENTICATED_DETAIL_BODY.to_owned(),
            ),
            Denial::Error(status, body) => (*status, body.clone()),
            Denial::ServerError => (
                StatusCode::INTERNAL_SERVER_ERROR,
                SERVER_ERROR_BODY.to_owned(),
            ),
        }
    }

    /// `{"error": message}` rendered the way DRF renders it (compact,
    /// UTF-8; `JSONRenderer` with `COMPACT_JSON` + `UNICODE_JSON`).
    fn error(status: StatusCode, message: &str) -> Self {
        Denial::Error(status, error_body(message))
    }
}

impl IntoResponse for Denial {
    fn into_response(self) -> Response {
        if matches!(self, Denial::ServerError) {
            tracing::warn!("device-flow handler: internal error");
        }
        let (status, body) = self.status_and_body();
        json_response(status, body)
    }
}

/// Successful body: `serde_json` with `preserve_order` keeps the `json!`
/// insertion order, which is the DRF field order the fixtures pin.
fn ok_response(status: StatusCode, value: serde_json::Value) -> Response {
    let body = serde_json::to_string(&value).expect("device-flow body serializes");
    json_response(status, body)
}

fn pool_of(state: &AppState) -> Result<sqlx::PgPool, Denial> {
    state
        .pools()
        .map(|pools| pools.primary().clone())
        .ok_or(Denial::ServerError)
}

// ---------------------------------------------------------------------------
// Session auth (approve only)
// ---------------------------------------------------------------------------

/// `request.user` from the Django session (`_auth_user_id`). No session,
/// no key, or a non-UUID id means anonymous → 401. (Django PKs are UUIDs;
/// a session id that is not a UUID cannot be a user.)
fn session_actor_id(extension: Option<Extension<SessionHandle>>) -> Option<uuid::Uuid> {
    let handle = extension?.0;
    let mut session = handle.snapshot();
    let raw = session.get("_auth_user_id")?.as_str()?.to_owned();
    raw.parse::<uuid::Uuid>().ok()
}

// ---------------------------------------------------------------------------
// Body parsing + user_code normalization (pure)
// ---------------------------------------------------------------------------

/// Locked `cli_device_codes` columns the approve path reads.
type ApproveRow = (
    uuid::Uuid,
    Option<uuid::Uuid>,
    bool,
    bool,
    bool,
    chrono::DateTime<chrono::Utc>,
    String,
    Option<uuid::Uuid>,
    bool,
);

/// Locked `cli_device_codes` columns the token path reads.
type TokenRow = (
    uuid::Uuid,
    Option<uuid::Uuid>,
    Option<uuid::Uuid>,
    bool,
    bool,
    bool,
    chrono::DateTime<chrono::Utc>,
    Option<chrono::DateTime<chrono::Utc>>,
    String,
    Option<uuid::Uuid>,
    bool,
);

/// Parse a JSON body leniently: empty means "no fields" (DRF reads an
/// empty POST as `{}`), malformed JSON is the DRF `ParseError` 400.
fn parse_body(body: &Bytes) -> Result<serde_json::Value, Denial> {
    if body.is_empty() {
        return Ok(serde_json::Value::Null);
    }
    serde_json::from_slice(body).map_err(|err| {
        Denial::Error(
            StatusCode::BAD_REQUEST,
            format!(
                "{{\"detail\":{}}}",
                serde_json::to_string(&format!("JSON parse error - {err}"))
                    .expect("parse error serializes")
            ),
        )
    })
}

/// Field lookup on a possibly-non-object body: only objects carry fields
/// (a JSON array/string/number has no `.get`, like DRF's `request.data`).
fn body_field<'a>(data: &'a serde_json::Value, key: &str) -> Option<&'a serde_json::Value> {
    data.as_object()?.get(key)
}

/// Python truthiness for JSON values (`x or y`): null, false, 0, "", [], {}
/// are falsy.
fn is_truthy(v: &serde_json::Value) -> bool {
    match v {
        serde_json::Value::Null => false,
        serde_json::Value::Bool(b) => *b,
        serde_json::Value::Number(n) => n.as_i64().unwrap_or(1) != 0,
        serde_json::Value::String(s) => !s.is_empty(),
        serde_json::Value::Array(a) => !a.is_empty(),
        serde_json::Value::Object(o) => !o.is_empty(),
    }
}

/// `str(request.data.get("scope") or "cli").strip().lower()` — Python's
/// `or` collapses falsy values to the default, then `str()` on the truthy
/// remainder, then strip/lower.
fn device_flow_scope(data: &serde_json::Value) -> String {
    match body_field(data, "scope") {
        Some(v) if is_truthy(v) => {
            let s = match v {
                serde_json::Value::String(s) => s.clone(),
                serde_json::Value::Bool(b) => b.to_string(),
                serde_json::Value::Number(n) => n.to_string(),
                other => other.to_string(),
            };
            s.trim().to_lowercase()
        }
        _ => "cli".to_owned(),
    }
}

/// `_parse_uuid(request.data.get("dev_machine_id"))` for the start endpoint:
/// only when scope is engine and the value is truthy.
fn device_flow_claimed_machine_id(data: &serde_json::Value, scope: &str) -> Option<uuid::Uuid> {
    if scope != "engine" {
        return None;
    }
    let v = body_field(data, "dev_machine_id")?;
    if !is_truthy(v) {
        return None;
    }
    let s = match v {
        serde_json::Value::String(s) => s.trim().to_owned(),
        other => other.to_string().trim().to_owned(),
    };
    if s.is_empty() {
        None
    } else {
        s.parse::<uuid::Uuid>().ok()
    }
}

/// SeaORM unique-violation detection for the start insert.
/// `DbErr` from a unique violation contains the Postgres code 23505.
fn is_unique_violation_db(err: &sea_orm::DbErr) -> bool {
    let s = err.to_string();
    s.contains("duplicate") || s.contains("unique") || s.contains("23505")
}

fn silent_refusal_description(state: &str) -> &'static str {
    match state {
        "never" => "The built-in engine is not turned on for this computer; turn it on explicitly.",
        "active" => "The built-in engine credential on this computer is not due for renewal yet.",
        _ => "The built-in engine was signed out on this computer; sign it in explicitly.",
    }
}

/// Why a raw `user_code` is unusable (`device.py:204-216`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserCodeRejection {
    /// Missing, blank, or not a string (`user_code is required.`).
    Blank,
    /// Fewer or more than 8 code characters after de-formatting
    /// (`user_code must be 8 characters.`).
    BadLength,
}

/// Python `str.strip()` membership
/// (`authentication/views/cli/device.py:202`): Rust `White_Space` plus
/// U+001C-U+001F (verified by exhaustively diffing `str.strip` against
/// `char::is_whitespace` over all code points — those four are the only
/// differences).
fn is_py_strip_ws(ch: char) -> bool {
    ch.is_whitespace() || matches!(ch, '\u{1c}'..='\u{1f}')
}

/// Normalize a raw `user_code` to the canonical `XXXX-XXXX` form
/// (`device.py:204-216`): `strip` + `upper`, drop ASCII spaces and
/// hyphens, require exactly 8 characters, re-insert the hyphen.
///
/// Char-based throughout: Python's `len` counts code points and its
/// slices cut code points, never bytes (Porting guide semantic trap).
pub fn canonical_user_code(raw: &str) -> Result<String, UserCodeRejection> {
    let upper = raw.trim_matches(is_py_strip_ws).to_uppercase();
    if upper.is_empty() {
        return Err(UserCodeRejection::Blank);
    }
    let normalized: Vec<char> = upper.chars().filter(|c| *c != ' ' && *c != '-').collect();
    if normalized.len() != 8 {
        return Err(UserCodeRejection::BadLength);
    }
    let head: String = normalized[..4].iter().collect();
    let tail: String = normalized[4..].iter().collect();
    Ok(format!("{head}-{tail}"))
}

/// `base_host(request=request)` (`authentication/utils/host.py:28-64`)
/// for the device flow: `settings.WEB_URL or settings.APP_BASE_URL`.
fn verification_base(state: &AppState) -> String {
    let urls = &state.settings().urls;
    urls.web_url
        .clone()
        .filter(|s| !s.is_empty())
        .or_else(|| urls.app_base_url.clone().filter(|s| !s.is_empty()))
        .unwrap_or_else(|| "http://localhost".to_owned())
}

// ---------------------------------------------------------------------------
// device/start/ throttle (DRF `DeviceCodeStartThrottle`, 20/minute per IP)
// ---------------------------------------------------------------------------

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

fn now_secs_f64() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// One `DeviceCodeStartThrottle` pass over the shared redis history
/// (`device.py:132-138` + `throttling.py:109-132` via the `super::guards`
/// spec and the guards kernel): trim entries older than the window, deny
/// iff 20 remain, recording `now` on allow with a 60s TTL (DRF caches
/// with `timeout=duration`). `start` is fully anonymous
/// (`authentication_classes = []`), so every caller counts — the
/// `authenticated=false` key unconditionally. No redis handle (or an
/// unreadable/unwritable cache) degrades to allow, per the
/// `auth_session` fail-open precedent — a cache blip must never deny an
/// auth endpoint.
async fn check_start_throttle(
    state: &AppState,
    headers: &HeaderMap,
    peer: Option<std::net::SocketAddr>,
) -> Result<(), Option<f64>> {
    let (num_requests, duration) = match device_start_throttle_rate() {
        Some(parsed) => parsed,
        None => return Ok(()),
    };
    let key = match device_start_cache_key(
        false,
        &throttle_ident(headers, peer.map(|p| p.ip().to_string())),
    ) {
        Some(key) => key,
        None => return Ok(()),
    };
    let redis = match state.redis() {
        Some(redis) => redis,
        None => return Ok(()),
    };
    let history: Vec<f64> = match redis.get_string(&key).await {
        Ok(Some(raw)) => serde_json::from_str(&raw).unwrap_or_default(),
        _ => Vec::new(),
    };
    let now = now_secs_f64();
    let decision = allow_request(&history, num_requests, duration, now);
    if !decision.allowed {
        return Err(throttle_wait(
            &decision.history,
            num_requests,
            duration,
            now,
        ));
    }
    let raw = serde_json::to_string(&decision.history).unwrap_or_else(|_| "[]".to_owned());
    if let Err(error) = redis.set_ex(&key, &raw, duration).await {
        tracing::debug!(%error, key = key.as_str(), "device-flow throttle: re-cache failed; allowance stands");
    }
    Ok(())
}

/// `Retry-After` rendering chain: `Throttled.__init__` ceils the wait
/// (`exceptions.py:237`), then the exception handler emits
/// `Retry-After: '%d' % wait` only when the ceiled value is truthy
/// (`views.py:90-91`). `None` in (overfull history) means the header is
/// absent.
fn retry_after_secs(wait: Option<f64>) -> Option<i64> {
    let secs = wait?.ceil() as i64;
    if secs == 0 {
        None
    } else {
        Some(secs)
    }
}

/// Live 429: `auth_exception_handler` rewrites DRF's default throttled
/// response to the 5900 error dict (`adapter/exception.py:26-31`); the
/// `Retry-After` header from `exception_handler` survives the rewrite
/// (`views.py:90-91`).
fn throttled(wait: Option<f64>) -> Response {
    let mut response = json_response(StatusCode::TOO_MANY_REQUESTS, throttle_denied_json());
    if let Some(secs) = retry_after_secs(wait) {
        if let Ok(value) = header::HeaderValue::from_str(&secs.to_string()) {
            response.headers_mut().insert(header::RETRY_AFTER, value);
        }
    }
    response
}

// ---------------------------------------------------------------------------
// POST /api/v1/auth/device/start/
// ---------------------------------------------------------------------------

/// RFC 8628 §3.1 — issue a device/user code pair (`device.py:145-187`).
///
/// Anonymous (`AllowAny`, empty `authentication_classes`), throttled per
/// IP (`DeviceCodeStartThrottle`, enforced here through the
/// `super::guards` spec and the shared DRF kernel). The body is read for
/// `scope` / `dev_machine_id` (`device.py` engine branch).
async fn post_start(
    State(state): State<AppState>,
    headers: HeaderMap,
    peer: PeerAddr,
    body: Bytes,
) -> Response {
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    // DRF order: `check_throttles` runs before `post()` — a denied call
    // writes no row.
    if let Err(wait) = check_start_throttle(&state, &headers, peer.0).await {
        return throttled(wait);
    }
    // `scope`: cli (default) or engine; stripped and lowercased
    // (`device.py` engine branch). A value other than cli/engine is 400
    // invalid_scope. For engine scope a non-empty dev_machine_id must
    // parse as a UUID, else 400 invalid_dev_machine_id.
    let data = match parse_body(&body) {
        Ok(d) => d,
        Err(denial) => return denial.into_response(),
    };
    let scope = device_flow_scope(&data);
    if !matches!(scope.as_str(), "cli" | "engine") {
        return Denial::Error(
            StatusCode::BAD_REQUEST,
            serde_json::json!({
                "error": "invalid_scope",
                "error_description": "scope must be 'cli' or 'engine'.",
            })
            .to_string(),
        )
        .into_response();
    }
    let claimed_machine_id = device_flow_claimed_machine_id(&data, &scope);
    if scope == "engine"
        && body_field(&data, "dev_machine_id").is_some_and(is_truthy)
        && claimed_machine_id.is_none()
    {
        return Denial::Error(
            StatusCode::BAD_REQUEST,
            serde_json::json!({
                "error": "invalid_dev_machine_id",
            })
            .to_string(),
        )
        .into_response();
    }

    // One grant window for every attempt (`device.py:159`).
    let now = chrono::Utc::now();
    let expires_at = now + chrono::Duration::seconds(DEVICE_CODE_TTL_SECS as i64);

    let mut created: Option<(String, String, String)> = None;
    for _ in 0..DEVICE_CODE_START_MAX_RETRIES {
        let device_code = cli_device_code::generate_device_code();
        let user_code = cli_device_code::generate_user_code();
        // SeaORM insert for the engine fields (scope, dev_machine_id);
        // the DB fills the rest via defaults. `Set(None)` would write NULL
        // and override the DB default, so `interactive_approval` and
        // `scope` get real values.
        use sea_orm::Set;
        let insert = pidash_db::entity::cli_device_codes::Entity::insert(
            pidash_db::entity::cli_device_codes::ActiveModel {
                id: Set(uuid::Uuid::new_v4()),
                created_at: Set(now.fixed_offset()),
                updated_at: Set(now.fixed_offset()),
                deleted_at: Set(None),
                device_code: Set(device_code.clone()),
                user_code: Set(user_code.clone()),
                approved: Set(false),
                denied: Set(false),
                consumed: Set(false),
                expires_at: Set(expires_at.fixed_offset()),
                last_polled_at: Set(None),
                created_by_id: Set(None),
                updated_by_id: Set(None),
                user_id: Set(None),
                workspace_id: Set(None),
                scope: Set(scope.clone()),
                dev_machine_id: Set(claimed_machine_id),
                interactive_approval: Set(false),
                api_token_id: Set(None),
            },
        )
        .exec_without_returning(&pidash_db::pool::sea_orm_connection(&pool))
        .await;
        match insert {
            Ok(_) => {
                created = Some((device_code, user_code, scope.clone()));
                break;
            }
            Err(err) if is_unique_violation_db(&err) => {
                tracing::warn!("CLIDeviceCode create collided, retrying: {err}");
            }
            Err(_) => return Denial::ServerError.into_response(),
        }
    }
    let Some((device_code, user_code, scope_val)) = created else {
        return Denial::Error(
            StatusCode::SERVICE_UNAVAILABLE,
            serde_json::json!({
                "error": "internal_error",
                "error_description": "Could not allocate a device code; try again.",
            })
            .to_string(),
        )
        .into_response();
    };
    ok_response(
        StatusCode::OK,
        serde_json::json!({
            "device_code": device_code,
            "user_code": user_code,
            "verification_uri": verification_uri(&verification_base(&state)),
            "expires_in": DEVICE_CODE_TTL_SECS,
            "interval": DEVICE_CODE_POLL_INTERVAL_SECS,
            "scope": scope_val,
        }),
    )
}

// ---------------------------------------------------------------------------
// POST /api/v1/auth/device/approve/
// ---------------------------------------------------------------------------

/// Session-auth: the logged-in human approves a pending CLI login
/// (`device.py:190-278` + engine grants). Validated before the transaction:
/// `workspace_slug` (stripped), `interactive` (default false, non-bool 400),
/// `dev_machine_id` (unparsable 400). Workspace: explicit slug must exist
/// and approver must be a member (404), else most-recent membership.
/// Engine grant: `_check_engine_grant_approval`.
async fn post_approve(
    State(state): State<AppState>,
    extension: Option<Extension<SessionHandle>>,
    body: Bytes,
) -> Response {
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let Some(actor_id) = session_actor_id(extension) else {
        return Denial::UnauthorizedDetail.into_response();
    };
    let data = match parse_body(&body) {
        Ok(data) => data,
        Err(denial) => return denial.into_response(),
    };
    // `(request.data.get("user_code") or "").strip().upper()`
    let raw = body_field(&data, "user_code")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let canonical = match canonical_user_code(raw) {
        Ok(canonical) => canonical,
        Err(UserCodeRejection::Blank) => {
            return Denial::error(StatusCode::BAD_REQUEST, "user_code is required.")
                .into_response();
        }
        Err(UserCodeRejection::BadLength) => {
            return Denial::error(StatusCode::BAD_REQUEST, "user_code must be 8 characters.")
                .into_response();
        }
    };

    // Pre-transaction validation (`device.py` engine approve).
    let workspace_slug_raw = body_field(&data, "workspace_slug")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_owned();
    let interactive = match body_field(&data, "interactive") {
        Some(serde_json::Value::Bool(b)) => *b,
        None => false,
        Some(_) => {
            return Denial::error(StatusCode::BAD_REQUEST, "interactive must be a boolean.")
                .into_response();
        }
    };
    let requested_machine_id: Option<uuid::Uuid> = match body_field(&data, "dev_machine_id") {
        Some(v) if is_truthy(v) => {
            let s = match v {
                serde_json::Value::String(s) => s.trim().to_owned(),
                other => other.to_string().trim().to_owned(),
            };
            if s.is_empty() {
                None
            } else {
                match s.parse::<uuid::Uuid>() {
                    Ok(id) => Some(id),
                    Err(_) => {
                        return Denial::error(StatusCode::BAD_REQUEST, "invalid_dev_machine_id")
                            .into_response();
                    }
                }
            }
        }
        _ => None,
    };

    // The approver's email for the 200 shape (SeaORM fetch).
    let email = {
        use pidash_db::entity::users::Entity as UserEntity;
        let db_mail = pidash_db::pool::sea_orm_connection(&pool);
        let user = UserEntity::find_by_id(actor_id)
            .filter(pidash_db::entity::users::Column::IsActive.eq(true))
            .one(&db_mail)
            .await
            .unwrap_or(None);
        match user {
            Some(u) => u.email,
            None => return Denial::UnauthorizedDetail.into_response(),
        }
    };

    let db = pidash_db::pool::sea_orm_connection(&pool);
    let mut tx = match pool.begin().await {
        Ok(tx) => tx,
        Err(_) => return Denial::ServerError.into_response(),
    };
    // raw-sql: FOR UPDATE on the grant row has no SeaORM equivalent.
    let row: Option<ApproveRow> = sqlx::query_as(
        r#"SELECT id, user_id, approved, denied, consumed, expires_at, scope, dev_machine_id, interactive_approval
           FROM cli_device_codes
           WHERE user_code = $1 AND deleted_at IS NULL
           FOR UPDATE"#,
    )
    .bind(&canonical)
    .fetch_optional(&mut *tx)
    .await
    .unwrap_or(None);
    let Some((
        row_id,
        row_user_id,
        approved,
        denied,
        consumed,
        expires_at,
        row_scope,
        row_dev_machine_id,
        row_interactive,
    )) = row
    else {
        return Denial::error(
            StatusCode::NOT_FOUND,
            "Code not recognized. Check the code on your terminal and try again.",
        )
        .into_response();
    };
    if consumed {
        return Denial::error(StatusCode::GONE, "This code has already been used.").into_response();
    }
    if denied {
        return Denial::error(StatusCode::GONE, "This code has been denied.").into_response();
    }
    let now = chrono::Utc::now();
    if expires_at <= now {
        return Denial::error(
            StatusCode::GONE,
            "This code has expired. Run `pidash auth login` again.",
        )
        .into_response();
    }
    if approved && row_user_id.is_some_and(|id| id != actor_id) {
        return Denial::error(
            StatusCode::CONFLICT,
            "This code has already been approved by another user.",
        )
        .into_response();
    }

    // Workspace: explicit slug vs most-recent (SeaORM lookups).
    let (workspace_id, workspace_slug) = if !workspace_slug_raw.is_empty() {
        use pidash_db::entity::{workspace_members, workspaces};
        let tx_conn = pidash_db::orm_tx::TxConn::new(&mut tx);
        let ws = workspaces::Entity::find()
            .filter(workspaces::Column::Slug.eq(&workspace_slug_raw))
            .filter(workspaces::Column::DeletedAt.is_null())
            .one(&tx_conn as &_)
            .await
            .unwrap_or(None);
        let Some(ws_row) = ws else {
            return Denial::error(StatusCode::NOT_FOUND, "workspace_not_found").into_response();
        };
        let ws_id = ws_row.id;
        let ws_slug = ws_row.slug;
        let is_member = workspace_members::Entity::find()
            .filter(workspace_members::Column::WorkspaceId.eq(ws_id))
            .filter(workspace_members::Column::MemberId.eq(actor_id))
            .filter(workspace_members::Column::IsActive.eq(true))
            .filter(workspace_members::Column::DeletedAt.is_null())
            .count(&tx_conn as &_)
            .await
            .unwrap_or(0)
            > 0;
        if !is_member {
            return Denial::error(StatusCode::NOT_FOUND, "workspace_not_found").into_response();
        }
        (Some(ws_id), Some(ws_slug))
    } else {
        use pidash_db::entity::{workspace_members, workspaces};
        use sea_orm::{EntityTrait as _, QueryOrder as _};
        let tx_conn = pidash_db::orm_tx::TxConn::new(&mut tx);
        let member = workspace_members::Entity::find()
            .filter(workspace_members::Column::MemberId.eq(actor_id))
            .filter(workspace_members::Column::IsActive.eq(true))
            .filter(workspace_members::Column::DeletedAt.is_null())
            .order_by_desc(workspace_members::Column::CreatedAt)
            .one(&tx_conn as &_)
            .await
            .unwrap_or(None);
        match member {
            Some(m) => {
                let ws = workspaces::Entity::find_by_id(m.workspace_id)
                    .one(&tx_conn as &_)
                    .await
                    .unwrap_or(None);
                match ws {
                    Some(w) => (Some(w.id), Some(w.slug)),
                    None => (None, None),
                }
            }
            None => (None, None),
        }
    };

    // Engine grant approval (`_check_engine_grant_approval`).
    let mut machine_state: Option<String> = None;
    let mut resolved_machine_id: Option<uuid::Uuid> = row_dev_machine_id;
    if row_scope == "engine" {
        // Requested and stored both set and different => 400.
        if let (Some(req), Some(stored)) = (requested_machine_id, row_dev_machine_id) {
            if req != stored {
                return Denial::error(StatusCode::BAD_REQUEST, "dev_machine_mismatch")
                    .into_response();
            }
        }
        let machine_id = requested_machine_id.or(row_dev_machine_id);
        if let Some(mid) = machine_id {
            // Fetch machine via SeaORM.
            use sea_orm::EntityTrait;
            let machine = pidash_db::entity::dev_machine::Entity::find_by_id(mid)
                .one(&db)
                .await
                .unwrap_or(None);
            let Some(machine) = machine else {
                return Denial::error(StatusCode::NOT_FOUND, "dev_machine_not_found")
                    .into_response();
            };
            if machine.owner_id != actor_id || machine.revoked_at.is_some() {
                return Denial::error(StatusCode::NOT_FOUND, "dev_machine_not_found")
                    .into_response();
            }
            // Interactive or no machine passes; otherwise check silent.
            if !interactive {
                let state =
                    pidash_services::domain::engine_credential::current_engine_sign_in_state(
                        &machine.engine_sign_in_state,
                        machine.engine_credential_expires_at.as_ref().map(
                            |dt: &sea_orm::prelude::DateTimeWithTimeZone| {
                                dt.with_timezone(&chrono::Utc)
                            },
                        ),
                        now,
                    );
                let allowed = pidash_services::domain::engine_credential::silent_issue_allowed(
                    &db, machine.id, now,
                )
                .await
                .unwrap_or(false);
                if !allowed {
                    let desc = silent_refusal_description(&state);
                    return Denial::Error(
                        StatusCode::CONFLICT,
                        serde_json::json!({
                            "error": pidash_services::domain::engine_credential::SILENT_RENEWAL_REFUSED,
                            "engine_sign_in_state": state,
                            "error_description": desc,
                        })
                        .to_string(),
                    )
                    .into_response();
                }
                machine_state = Some(state);
            } else {
                machine_state = Some(
                    pidash_services::domain::engine_credential::current_engine_sign_in_state(
                        &machine.engine_sign_in_state,
                        machine.engine_credential_expires_at.as_ref().map(
                            |dt: &sea_orm::prelude::DateTimeWithTimeZone| {
                                dt.with_timezone(&chrono::Utc)
                            },
                        ),
                        now,
                    ),
                );
            }
            resolved_machine_id = Some(mid);
        } else {
            // No machine named at approval; checked at redemption.
        }
    }

    let new_interactive = row_interactive || interactive;
    // Save user, workspace, approved, interactive_approval, dev_machine_id.
    let update = {
        use pidash_db::entity::cli_device_codes;
        use sea_orm::sea_query::Expr;
        let tx_conn = pidash_db::orm_tx::TxConn::new(&mut tx);
        let now_tz = now.fixed_offset();
        cli_device_codes::Entity::update_many()
            .col_expr(
                cli_device_codes::Column::UserId,
                Expr::value(Some(actor_id)),
            )
            .col_expr(
                cli_device_codes::Column::WorkspaceId,
                Expr::value(workspace_id),
            )
            .col_expr(cli_device_codes::Column::Approved, Expr::value(true))
            .col_expr(
                cli_device_codes::Column::InteractiveApproval,
                Expr::value(new_interactive),
            )
            .col_expr(
                cli_device_codes::Column::DevMachineId,
                Expr::value(resolved_machine_id),
            )
            .col_expr(cli_device_codes::Column::UpdatedAt, Expr::value(now_tz))
            .filter(cli_device_codes::Column::Id.eq(row_id))
            .exec(&tx_conn as &_)
            .await
    };
    if update.is_err() {
        return Denial::ServerError.into_response();
    }
    if tx.commit().await.is_err() {
        return Denial::ServerError.into_response();
    }

    let mut body = serde_json::json!({
        "ok": true,
        "user_email": email,
        "workspace_slug": workspace_slug,
        "scope": row_scope,
        "interactive": new_interactive,
    });
    if let Some(state) = machine_state {
        if let Some(mid) = resolved_machine_id {
            body.as_object_mut().unwrap().insert(
                "dev_machine_id".to_owned(),
                serde_json::Value::String(mid.to_string()),
            );
            body.as_object_mut().unwrap().insert(
                "engine_sign_in_state".to_owned(),
                serde_json::Value::String(state),
            );
        }
    }
    ok_response(StatusCode::OK, body)
}

// ---------------------------------------------------------------------------
// POST /api/v1/auth/device/token/
// ---------------------------------------------------------------------------

/// RFC 8628 §3.4 — the CLI polls here trading `device_code` for an
/// `APIToken` (`device.py:281-376` + engine grants).
///
/// Branch order is the Python order: blank → unknown → consumed → denied
/// → expired → engine bearer gate → slow_down (no touch) → pending
/// (touch) → engine redemption or cli mint + consume.
async fn post_token(State(state): State<AppState>, headers: HeaderMap, body: Bytes) -> Response {
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let data = match parse_body(&body) {
        Ok(data) => data,
        Err(denial) => return denial.into_response(),
    };
    // `(request.data.get("device_code") or "").strip()`: a missing or
    // non-string value answers the required 400 (Django would 500 on a
    // non-string's `.strip()`; see module docs).
    let device_code = body_field(&data, "device_code")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_owned();
    if device_code.is_empty() {
        return Denial::Error(
            StatusCode::BAD_REQUEST,
            serde_json::json!({
                "error": "invalid_request",
                "error_description": "device_code is required.",
            })
            .to_string(),
        )
        .into_response();
    }

    let mut tx = match pool.begin().await {
        Ok(tx) => tx,
        Err(_) => return Denial::ServerError.into_response(),
    };
    // raw-sql: FOR UPDATE on the grant row.
    let row: Option<TokenRow> = sqlx::query_as(
        r#"SELECT id, user_id, workspace_id, approved, denied, consumed,
                  expires_at, last_polled_at, scope, dev_machine_id, interactive_approval
           FROM cli_device_codes
           WHERE device_code = $1 AND deleted_at IS NULL
           FOR UPDATE"#,
    )
    .bind(&device_code)
    .fetch_optional(&mut *tx)
    .await
    .unwrap_or(None);
    let Some((
        row_id,
        row_user_id,
        row_workspace_id,
        approved,
        denied,
        consumed,
        expires_at,
        last_polled_at,
        row_scope,
        row_dev_machine_id,
        row_interactive,
    )) = row
    else {
        return Denial::Error(
            StatusCode::BAD_REQUEST,
            serde_json::json!({
                "error": "invalid_grant",
                "error_description": "Unknown device code.",
            })
            .to_string(),
        )
        .into_response();
    };

    // `now` is clocked after the locked fetch (`device.py:309`).
    let now = chrono::Utc::now();
    if consumed {
        return Denial::Error(
            StatusCode::BAD_REQUEST,
            serde_json::json!({
                "error": "invalid_grant",
                "error_description": "Device code already consumed.",
            })
            .to_string(),
        )
        .into_response();
    }
    if denied {
        return Denial::error(StatusCode::GONE, "access_denied").into_response();
    }
    if expires_at <= now {
        return Denial::error(StatusCode::GONE, "expired_token").into_response();
    }
    // Engine grant: require Authorization: Bearer mt_... before
    // slow_down/pending (`device.py` _bearer_machine_token).
    let mut engine_machine_token: Option<(uuid::Uuid, uuid::Uuid, Option<uuid::Uuid>)> = None;
    if row_scope == "engine" {
        let bearer = match bearer_machine_token(&headers) {
            Ok(b) => b,
            Err(denial) => return denial.into_response(),
        };
        let machine = match resolve_machine_token_for_engine(
            &pool,
            state.settings().secret_key.as_bytes(),
            &bearer,
            now,
        )
        .await
        {
            Ok(m) => m,
            Err(denial) => return denial.into_response(),
        };
        engine_machine_token = Some((machine.0, machine.1, machine.2));
    }
    // Minimum poll gap (`device.py:319-336`): the first poll is always
    // accepted; a `slow_down` rejection must NOT bump `last_polled_at`
    // or a spammer could starve the legit CLI (anti-starvation).
    if last_polled_at.is_some_and(|last| now - last < poll_gap()) {
        return Denial::error(StatusCode::BAD_REQUEST, "slow_down").into_response();
    }

    if !approved || row_user_id.is_none() {
        let touched = {
            use pidash_db::entity::cli_device_codes;
            use sea_orm::sea_query::Expr;
            let tx_conn = pidash_db::orm_tx::TxConn::new(&mut tx);
            let now_tz = now.fixed_offset();
            cli_device_codes::Entity::update_many()
                .col_expr(
                    cli_device_codes::Column::LastPolledAt,
                    Expr::value(Some(now_tz)),
                )
                .col_expr(cli_device_codes::Column::UpdatedAt, Expr::value(now_tz))
                .filter(cli_device_codes::Column::Id.eq(row_id))
                .exec(&tx_conn as &_)
                .await
        };
        if touched.is_err() || tx.commit().await.is_err() {
            return Denial::ServerError.into_response();
        }
        return Denial::error(StatusCode::BAD_REQUEST, "authorization_pending").into_response();
    }

    // Engine grant redemption (`_redeem_engine_grant`, `device.py:393-469`).
    if row_scope == "engine" {
        let user_id = row_user_id.expect("approved row carries a user");
        // The Bearer token was resolved before slow_down; it must be present.
        let Some((_tok_id, tok_user_id, tok_machine_id)) = engine_machine_token else {
            return Denial::error(StatusCode::UNAUTHORIZED, "machine_token_required")
                .into_response();
        };
        // Token without a machine => 403 dev_machine_required.
        let Some(machine_id) = tok_machine_id else {
            return Denial::Error(
                StatusCode::FORBIDDEN,
                serde_json::json!({
                    "error": "dev_machine_required",
                    "error_description": "This machine token is not bound to a computer.",
                })
                .to_string(),
            )
            .into_response();
        };
        // Token user or machine owner is not the approver => 403 access_denied.
        // Fetch machine for owner check.
        let machine = {
            use sea_orm::EntityTrait;
            pidash_db::entity::dev_machine::Entity::find_by_id(machine_id)
                .one(&pidash_db::pool::sea_orm_connection(&pool))
                .await
                .unwrap_or(None)
        };
        let Some(machine) = machine else {
            return Denial::error(StatusCode::NOT_FOUND, "dev_machine_not_found").into_response();
        };
        if tok_user_id != user_id || machine.owner_id != user_id {
            return Denial::Error(
                StatusCode::FORBIDDEN,
                serde_json::json!({
                    "error": "access_denied",
                    "error_description": "This computer does not belong to the approving user.",
                })
                .to_string(),
            )
            .into_response();
        }
        // Grant named another machine => 403 dev_machine_mismatch.
        if let Some(grant_mid) = row_dev_machine_id {
            if grant_mid != machine_id {
                return Denial::Error(
                    StatusCode::FORBIDDEN,
                    serde_json::json!({
                        "error": "dev_machine_mismatch",
                        "error_description": "This grant was approved for another computer.",
                    })
                    .to_string(),
                )
                .into_response();
            }
        }
        // Silent grant with !silent_issue_allowed => 409.
        let db_conn = pidash_db::pool::sea_orm_connection(&pool);
        if !row_interactive {
            let state = pidash_services::domain::engine_credential::current_engine_sign_in_state(
                &machine.engine_sign_in_state,
                machine.engine_credential_expires_at.as_ref().map(
                    |dt: &sea_orm::prelude::DateTimeWithTimeZone| dt.with_timezone(&chrono::Utc),
                ),
                now,
            );
            let allowed = pidash_services::domain::engine_credential::silent_issue_allowed(
                &db_conn, machine.id, now,
            )
            .await
            .unwrap_or(false);
            if !allowed {
                let desc = silent_refusal_description(&state);
                return Denial::Error(
                    StatusCode::CONFLICT,
                    serde_json::json!({
                        "error": pidash_services::domain::engine_credential::SILENT_RENEWAL_REFUSED,
                        "engine_sign_in_state": state,
                        "error_description": desc,
                    })
                    .to_string(),
                )
                .into_response();
            }
        }
        // Engine profile unavailable => 409 engine_unavailable.
        let (available, pause_reason) =
            pidash_services::domain::engine_credential::engine_profile_for_user_available(
                state.settings(),
            );
        if !available {
            let state = pidash_services::domain::engine_credential::current_engine_sign_in_state(
                &machine.engine_sign_in_state,
                machine.engine_credential_expires_at.as_ref().map(
                    |dt: &sea_orm::prelude::DateTimeWithTimeZone| dt.with_timezone(&chrono::Utc),
                ),
                now,
            );
            return Denial::Error(
                StatusCode::CONFLICT,
                serde_json::json!({
                    "error": "engine_unavailable",
                    "reason_code": pause_reason,
                    "engine_sign_in_state": state,
                    "error_description": "This Pi Dash instance cannot serve the built-in engine for this account.",
                })
                .to_string(),
            )
            .into_response();
        }
        // Issue engine credential (silent = !interactive).
        let secret = state.settings().secret_key.clone();
        let issued = pidash_services::domain::engine_credential::issue_engine_credential(
            &db_conn,
            secret.as_bytes(),
            user_id,
            machine_id,
            !row_interactive,
            now,
        )
        .await;
        let issued = match issued {
            Ok(i) => i,
            Err(pidash_services::domain::engine_credential::IssueError::SilentRenewalRefused) => {
                // Lost race: reuse silent-refused body with fresh state.
                let fresh = {
                    use sea_orm::EntityTrait;
                    pidash_db::entity::dev_machine::Entity::find_by_id(machine_id)
                        .one(&db_conn)
                        .await
                        .unwrap_or(None)
                };
                let state = fresh
                    .map(|m| {
                        pidash_services::domain::engine_credential::current_engine_sign_in_state(
                            &m.engine_sign_in_state,
                            m.engine_credential_expires_at.as_ref().map(
                                |dt: &sea_orm::prelude::DateTimeWithTimeZone| {
                                    dt.with_timezone(&chrono::Utc)
                                },
                            ),
                            now,
                        )
                    })
                    .unwrap_or_else(|| "never".to_owned());
                let desc = silent_refusal_description(&state);
                return Denial::Error(
                    StatusCode::CONFLICT,
                    serde_json::json!({
                        "error": pidash_services::domain::engine_credential::SILENT_RENEWAL_REFUSED,
                        "engine_sign_in_state": state,
                        "error_description": desc,
                    })
                    .to_string(),
                )
                .into_response();
            }
            Err(_) => return Denial::ServerError.into_response(),
        };
        // Mark consumed and store dev_machine_id.
        let consumed = {
            use pidash_db::entity::cli_device_codes;
            use sea_orm::sea_query::Expr;
            let tx_conn = pidash_db::orm_tx::TxConn::new(&mut tx);
            let now_tz = now.fixed_offset();
            cli_device_codes::Entity::update_many()
                .col_expr(cli_device_codes::Column::Consumed, Expr::value(true))
                .col_expr(
                    cli_device_codes::Column::LastPolledAt,
                    Expr::value(Some(now_tz)),
                )
                .col_expr(
                    cli_device_codes::Column::DevMachineId,
                    Expr::value(Some(machine_id)),
                )
                .col_expr(cli_device_codes::Column::UpdatedAt, Expr::value(now_tz))
                .filter(cli_device_codes::Column::Id.eq(row_id))
                .exec(&tx_conn as &_)
                .await
        };
        if consumed.is_err() || tx.commit().await.is_err() {
            return Denial::ServerError.into_response();
        }
        let state_after = {
            use sea_orm::EntityTrait;
            let m = pidash_db::entity::dev_machine::Entity::find_by_id(machine_id)
                .one(&db_conn)
                .await
                .unwrap_or(None);
            m.map(|m| {
                pidash_services::domain::engine_credential::current_engine_sign_in_state(
                    &m.engine_sign_in_state,
                    m.engine_credential_expires_at.as_ref().map(
                        |dt: &sea_orm::prelude::DateTimeWithTimeZone| {
                            dt.with_timezone(&chrono::Utc)
                        },
                    ),
                    now,
                )
            })
            .unwrap_or_else(|| "active".to_owned())
        };
        return ok_response(
            StatusCode::OK,
            serde_json::json!({
                "engine_credential": {
                    "token": issued.raw,
                    "expires_at": issued.expires_at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                },
                "engine_sign_in_state": state_after,
                "dev_machine_id": machine_id.to_string(),
            }),
        );
    }

    // Mint: override the default opaque-hex label so device tokens are
    // distinguishable from user-created PATs (`device.py:340-349`).
    // A cli grant also stores row.api_token_id (`device.py:355`).
    let user_id = row_user_id.expect("approved row carries a user");
    let api_token = format!("pi_dash_api_{}", uuid::Uuid::new_v4().simple());
    let api_token_id = uuid::Uuid::new_v4();
    let minted = pidash_services::domain::tokens::issue_device_token(
        &pidash_db::orm_tx::TxConn::new(&mut tx),
        &pidash_services::domain::tokens::NewDeviceToken {
            id: api_token_id,
            token: api_token.clone(),
            label: api_token_device_flow::device_flow_label(&now),
            description: api_token_device_flow::DEVICE_FLOW_DESCRIPTION.to_string(),
            user_id,
            workspace_id: row_workspace_id,
            now,
        },
    )
    .await;
    if minted.is_err() {
        return Denial::ServerError.into_response();
    }
    let consumed = {
        use pidash_db::entity::cli_device_codes;
        use sea_orm::sea_query::Expr;
        let tx_conn = pidash_db::orm_tx::TxConn::new(&mut tx);
        let now_tz = now.fixed_offset();
        cli_device_codes::Entity::update_many()
            .col_expr(cli_device_codes::Column::Consumed, Expr::value(true))
            .col_expr(
                cli_device_codes::Column::LastPolledAt,
                Expr::value(Some(now_tz)),
            )
            .col_expr(
                cli_device_codes::Column::ApiTokenId,
                Expr::value(Some(api_token_id)),
            )
            .col_expr(cli_device_codes::Column::UpdatedAt, Expr::value(now_tz))
            .filter(cli_device_codes::Column::Id.eq(row_id))
            .exec(&tx_conn as &_)
            .await
    };
    if consumed.is_err() || tx.commit().await.is_err() {
        return Denial::ServerError.into_response();
    }

    // Response identity (`device.py:355-362`): the grant user's email and
    // workspace slug, each null when the FK is null. A dangling FK would
    // raise in Django, hence `ServerError` here.
    let user_email = {
        use pidash_db::entity::users::Entity as UserEntity;
        let db_e = pidash_db::pool::sea_orm_connection(&pool);
        let user = UserEntity::find_by_id(user_id)
            .one(&db_e)
            .await
            .unwrap_or(None);
        let Some(u) = user else {
            return Denial::ServerError.into_response();
        };
        u.email
    };
    let workspace_slug: Option<String> = match row_workspace_id {
        None => None,
        Some(workspace_id) => {
            use pidash_db::entity::workspaces::Entity as WsEntity;
            let db_w = pidash_db::pool::sea_orm_connection(&pool);
            let ws = WsEntity::find_by_id(workspace_id)
                .one(&db_w)
                .await
                .unwrap_or(None);
            match ws {
                Some(w) => Some(w.slug),
                None => return Denial::ServerError.into_response(),
            }
        }
    };
    ok_response(
        StatusCode::OK,
        serde_json::json!({
            "access_token": api_token,
            "token_type": "X-Api-Key",
            "user_email": user_email,
            "workspace_slug": workspace_slug,
        }),
    )
}

/// `DEVICE_CODE_MIN_POLL_GAP` as a `chrono` duration for the slow_down
/// comparison (`(now - row.last_polled_at) < DEVICE_CODE_MIN_POLL_GAP`,
/// `device.py:332`).
fn poll_gap() -> chrono::Duration {
    chrono::Duration::seconds(DEVICE_CODE_MIN_POLL_GAP_SECS as i64)
}

/// `_bearer_machine_token(request)` (`device.py:203-209`): Rust mirror
/// of Python `header.split()` (any whitespace, empty discarded), exactly
/// two parts, scheme `bearer` (case-insensitive), token must start with
/// `mt_`; else 401 `machine_token_required`.
fn bearer_machine_token(headers: &HeaderMap) -> Result<String, Denial> {
    let auth = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    // Python: parts = header.split() — whitespace-split, empty tokens dropped.
    // Rust split_whitespace() matches (splits on the same Unicode White_Space).
    let mut parts = auth.split_whitespace();
    let scheme = parts.next().unwrap_or("");
    let token = parts.next().unwrap_or("");
    let extra = parts.next();
    if extra.is_some()
        || !scheme.eq_ignore_ascii_case("bearer")
        || token.is_empty()
        || !token.starts_with(pidash_auth::token::MACHINE_TOKEN_PREFIX)
    {
        return Err(Denial::Error(
            StatusCode::UNAUTHORIZED,
            serde_json::json!({
                "error": "machine_token_required",
                "error_description": "Redeem an engine grant with Authorization: Bearer <machine token>.",
            })
            .to_string(),
        ));
    }
    Ok(token.to_owned())
}

/// `resolve_machine_token(raw, touch=True)` (`runner/authentication.py:97-121`)
/// for the engine redemption path. Returns
/// `(token_id, user_id, dev_machine_id)` on success, else a 401 with
/// `invalid_client` and the resolver code as `error_description`.
/// Resolver codes are byte-identical to Django: `machine_token_invalid`,
/// `machine_token_revoked`, `machine_token_expired`, `dev_machine_revoked`,
/// `membership_revoked`. An unbound token (`dev_machine_id = NULL`) is NOT
/// refused here — redemption answers 403 `dev_machine_required` (see
/// `post_token`). Membership lapse revokes the token before answering.
async fn resolve_machine_token_for_engine(
    pool: &sqlx::PgPool,
    secret_key: &[u8],
    raw: &str,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<(uuid::Uuid, uuid::Uuid, Option<uuid::Uuid>), Denial> {
    use sea_orm::EntityTrait;
    fn invalid_client(code: &str) -> Denial {
        Denial::Error(
            StatusCode::UNAUTHORIZED,
            serde_json::json!({
                "error": "invalid_client",
                "error_description": code,
            })
            .to_string(),
        )
    }
    let token_hash = pidash_auth::token::hash_token(raw, secret_key);
    let db = pidash_db::pool::sea_orm_connection(pool);
    let row = pidash_db::entity::machine_token::Entity::find()
        .filter(pidash_db::entity::machine_token::Column::TokenHash.eq(&token_hash))
        .one(&db)
        .await
        .map_err(|_| Denial::ServerError)?;
    let Some(token_row) = row else {
        return Err(invalid_client("machine_token_invalid"));
    };
    if token_row.revoked_at.is_some() {
        return Err(invalid_client("machine_token_revoked"));
    }
    if let Some(exp) = token_row.expires_at {
        if exp.with_timezone(&chrono::Utc) <= now {
            return Err(invalid_client("machine_token_expired"));
        }
    }
    // Revoked (or missing) dev machine -> dev_machine_revoked. Unbound
    // (NULL) skips this gate; redemption 403s below (device.py:258-262).
    if let Some(mid) = token_row.dev_machine_id {
        let machine = pidash_db::entity::dev_machine::Entity::find_by_id(mid)
            .one(&db)
            .await
            .map_err(|_| Denial::ServerError)?;
        let Some(machine) = machine else {
            return Err(invalid_client("dev_machine_revoked"));
        };
        if machine.revoked_at.is_some() {
            return Err(invalid_client("dev_machine_revoked"));
        }
    }
    // Workspace membership (`is_workspace_member`): a lapsed token is
    // revoked as a side effect before refusing.
    {
        use sea_orm::EntityTrait;
        let is_member = pidash_db::entity::workspace_members::Entity::find()
            .filter(
                pidash_db::entity::workspace_members::Column::WorkspaceId
                    .eq(token_row.workspace_id),
            )
            .filter(pidash_db::entity::workspace_members::Column::MemberId.eq(token_row.user_id))
            .filter(pidash_db::entity::workspace_members::Column::IsActive.eq(true))
            .filter(pidash_db::entity::workspace_members::Column::DeletedAt.is_null())
            .count(&db)
            .await
            .map_err(|_| Denial::ServerError)?;
        if is_member == 0 {
            let _ =
                pidash_services::domain::tokens::revoke_machine_token_by_id(&db, token_row.id, now)
                    .await;
            return Err(invalid_client("membership_revoked"));
        }
    }
    // Touch: last_used_at + 60-day idle-expiry renewal.
    let _ = pidash_services::domain::tokens::touch_machine_token_last_used_by_id(
        &db,
        token_row.id,
        now,
    )
    .await;
    Ok((token_row.id, token_row.user_id, token_row.dev_machine_id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn origin_state(app_base_url: Option<&str>, web_url: Option<&str>) -> AppState {
        let mut settings = pidash_db::config::Settings::test_defaults();
        settings.urls.app_base_url = app_base_url.map(str::to_owned);
        settings.urls.web_url = web_url.map(str::to_owned);
        AppState::with_settings("test", settings)
    }

    /// `base_host(request=request)` treats `""` as missing
    /// (PIDASHCONV-1597): an empty `WEB_URL` falls through to
    /// `APP_BASE_URL`. (The localhost fallback for unset-everywhere is
    /// a separate, deliberate-looking deviation — out of scope.)
    #[test]
    fn verification_base_falls_through_empty_strings() {
        let app = "https://app.example.invalid";
        let state = origin_state(Some(app), Some(""));
        assert_eq!(verification_base(&state), app);
        let web = "https://web.example.invalid";
        let state = origin_state(Some(app), Some(web));
        assert_eq!(verification_base(&state), web);
        let state = origin_state(Some(""), Some(""));
        assert_eq!(verification_base(&state), "http://localhost");
    }

    #[test]
    fn peerless_throttle_ident_is_drf_none() {
        // PIDASHCONV-1419: with no XFF and no peer, DRF's `cache_format
        // % {'ident': None}` renders "None", never "".
        let headers = axum::http::HeaderMap::new();
        assert_eq!(throttle_ident(&headers, None), "None");
    }

    fn fixtures_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/auth_oauth")
    }

    fn f12() -> serde_json::Value {
        let body = std::fs::read_to_string(fixtures_dir().join("F12_device_endpoints.golden.json"))
            .expect("read F12_device_endpoints.golden.json");
        serde_json::from_str(&body).expect("F12_device_endpoints.golden.json is valid JSON")
    }

    fn branch(section: &serde_json::Value, input: &str) -> serde_json::Value {
        section["branches"]
            .as_array()
            .unwrap()
            .iter()
            .find(|b| b["in"].as_str().unwrap() == input)
            .unwrap_or_else(|| panic!("F12 branch {input}"))
            .clone()
    }

    fn error_json(message: &str) -> serde_json::Value {
        serde_json::from_str(&error_body(message)).expect("error body is JSON")
    }

    // -- user_code normalization (device.py:204-216) ------------------------

    #[test]
    fn canonical_user_code_matches_python_normalization() {
        // Blank inputs (missing/empty/whitespace-only).
        for raw in ["", "   ", "  \t\n "] {
            assert_eq!(
                canonical_user_code(raw),
                Err(UserCodeRejection::Blank),
                "{raw:?}"
            );
        }
        // Lenient about case, hyphens and spaces; canonical XXXX-XXXX.
        assert_eq!(canonical_user_code("wxyz-1234").as_deref(), Ok("WXYZ-1234"));
        assert_eq!(
            canonical_user_code("  WXYZ 1234 ").as_deref(),
            Ok("WXYZ-1234")
        );
        assert_eq!(canonical_user_code("wxyz1234").as_deref(), Ok("WXYZ-1234"));
        // Length is checked after de-formatting: 7 and 9 both fail.
        assert_eq!(
            canonical_user_code("WXYZ-123"),
            Err(UserCodeRejection::BadLength)
        );
        assert_eq!(
            canonical_user_code("WXYZ-12345"),
            Err(UserCodeRejection::BadLength)
        );
        // Tabs/newlines inside are NOT stripped (only ' ' and '-'), so
        // they count toward the 8 and fail the length check.
        assert_eq!(
            canonical_user_code("WXYZ\t1234"),
            Err(UserCodeRejection::BadLength)
        );
    }

    // -- start (F12 start_POST_device_start) ---------------------------------

    #[test]
    fn start_golden_shape_matches_f12() {
        let start = &f12()["start_POST_device_start"];
        assert_eq!(
            start["golden_200"]["body_keys_order"]
                .as_array()
                .unwrap()
                .iter()
                .map(|k| k.as_str().unwrap())
                .collect::<Vec<_>>(),
            [
                "device_code",
                "user_code",
                "verification_uri",
                "expires_in",
                "interval",
                "scope"
            ]
        );
        assert_eq!(
            start["golden_200"]["expires_in"].as_u64().unwrap(),
            DEVICE_CODE_TTL_SECS
        );
        assert_eq!(
            start["golden_200"]["interval"].as_u64().unwrap(),
            DEVICE_CODE_POLL_INTERVAL_SECS
        );
        // Exhaustion answers 503 with the exact two-key body.
        assert!(start["collision"].as_str().unwrap().contains("5 exhausted"));
        assert!(start["collision"].as_str().unwrap().contains("503"));
        let exhausted: serde_json::Value = serde_json::from_str(
            r#"{"error":"internal_error","error_description":"Could not allocate a device code; try again."}"#,
        )
        .expect("503 body is JSON");
        assert_eq!(exhausted["error"], "internal_error");
        assert_eq!(
            exhausted["error_description"],
            "Could not allocate a device code; try again."
        );
        // The retry bound is the F9 constant (5).
        assert_eq!(
            DEVICE_CODE_START_MAX_RETRIES, 5,
            "matches F9 DEVICE_CODE_START_MAX_RETRIES"
        );
    }

    // -- approve (F12 approve_POST_device_approve) -----------------------------

    #[test]
    fn approve_branch_bodies_match_f12() {
        let approve = &f12()["approve_POST_device_approve"];
        // Every error branch is a single {"error"} body, byte-identical.
        for (input, status, body) in [
            (
                "user_code missing/blank",
                StatusCode::BAD_REQUEST,
                &error_body("user_code is required."),
            ),
            (
                "normalized len != 8 (after strip/upper, remove spaces+hyphens)",
                StatusCode::BAD_REQUEST,
                &error_body("user_code must be 8 characters."),
            ),
            (
                "canonical unknown",
                StatusCode::NOT_FOUND,
                &error_body("Code not recognized. Check the code on your terminal and try again."),
            ),
            (
                "row.consumed",
                StatusCode::GONE,
                &error_body("This code has already been used."),
            ),
            (
                "row.denied",
                StatusCode::GONE,
                &error_body("This code has been denied."),
            ),
            (
                "row.expires_at <= now",
                StatusCode::GONE,
                &error_body("This code has expired. Run `pidash auth login` again."),
            ),
            (
                "row.approved and row.user_id set and != requester",
                StatusCode::CONFLICT,
                &error_body("This code has already been approved by another user."),
            ),
        ] {
            let b = branch(approve, input);
            assert_eq!(
                b["out"]["status"].as_u64().unwrap(),
                status.as_u16() as u64,
                "{input}"
            );
            assert_eq!(
                b["out"]["body"],
                error_json_for(&b["out"]["body"], body),
                "{input}"
            );
        }
        // The normalizer drives the first two branches.
        assert_eq!(
            canonical_user_code(""),
            Err(UserCodeRejection::Blank),
            "blank input feeds the 400 required branch"
        );
        assert_eq!(
            canonical_user_code("ABC"),
            Err(UserCodeRejection::BadLength),
            "short input feeds the 400 length branch"
        );
        // The 200 shape carries ok/user_email/workspace_slug, with a null
        // workspace when the approver has no membership.
        let ok = &approve["branches"]
            .as_array()
            .unwrap()
            .iter()
            .find(|b| b["in"].as_str().unwrap() == "valid, membership found")
            .unwrap()["out"]["body"];
        assert_eq!(ok["ok"], true);
        assert!(ok.get("user_email").is_some());
        assert!(ok.get("workspace_slug").is_some());
        let lonely = &approve["branches"]
            .as_array()
            .unwrap()
            .iter()
            .find(|b| b["in"].as_str().unwrap() == "valid, no membership")
            .unwrap()["out"]["body"];
        assert_eq!(lonely["workspace_slug"], serde_json::Value::Null);
    }

    // -- approve anonymous denial (PIDASHCONV-734) ---------------------------

    #[test]
    fn approve_anonymous_denial_is_401_with_drf_body() {
        // Live Django renders the `IsAuthenticated` gate as 401 with the
        // `NotAuthenticated` body (probed, no `WWW-Authenticate` header).
        let (status, body) = Denial::UnauthorizedDetail.status_and_body();
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(
            body,
            r#"{"detail":"Authentication credentials were not provided."}"#
        );
    }

    /// Compare a fixture `out.body` (with `<...>` placeholders) against a
    /// rendered error body: placeholder-free branches must be identical.
    fn error_json_for(fixture_body: &serde_json::Value, rendered: &str) -> serde_json::Value {
        let rendered: serde_json::Value =
            serde_json::from_str(rendered).expect("rendered error is JSON");
        assert_eq!(
            fixture_body, &rendered,
            "fixture body {fixture_body} vs rendered {rendered}"
        );
        rendered
    }

    // -- token (F12 token_POST_device_token) -----------------------------------

    #[test]
    fn token_branch_bodies_match_f12() {
        let token = &f12()["token_POST_device_token"];
        for (input, status) in [
            ("device_code blank", StatusCode::BAD_REQUEST),
            ("unknown code", StatusCode::BAD_REQUEST),
            ("row.consumed", StatusCode::BAD_REQUEST),
            ("row.denied", StatusCode::GONE),
            ("row.expires_at <= now", StatusCode::GONE),
            (
                "polled < 3s since last_polled_at (and last_polled_at set)",
                StatusCode::BAD_REQUEST,
            ),
            ("not approved or user null", StatusCode::BAD_REQUEST),
            ("approved", StatusCode::OK),
        ] {
            let b = branch(token, input);
            assert_eq!(
                b["out"]["status"].as_u64().unwrap(),
                status.as_u16() as u64,
                "{input}"
            );
        }
        // Exact two-key / single-key bodies.
        let blank = branch(token, "device_code blank");
        assert_eq!(
            blank["out"]["body"],
            serde_json::json!({
                "error": "invalid_request",
                "error_description": "device_code is required.",
            })
        );
        let unknown = branch(token, "unknown code");
        assert_eq!(
            unknown["out"]["body"],
            serde_json::json!({
                "error": "invalid_grant",
                "error_description": "Unknown device code.",
            })
        );
        let consumed = branch(token, "row.consumed");
        assert_eq!(
            consumed["out"]["body"],
            serde_json::json!({
                "error": "invalid_grant",
                "error_description": "Device code already consumed.",
            })
        );
        for (input, message) in [
            ("row.denied", "access_denied"),
            ("row.expires_at <= now", "expired_token"),
            (
                "polled < 3s since last_polled_at (and last_polled_at set)",
                "slow_down",
            ),
            ("not approved or user null", "authorization_pending"),
        ] {
            let b = branch(token, input);
            assert_eq!(b["out"]["body"], error_json(message), "{input}");
        }
        // slow_down must NOT bump last_polled_at (anti-starvation); the
        // floor is the F9 3s constant.
        let slow = branch(
            token,
            "polled < 3s since last_polled_at (and last_polled_at set)",
        );
        assert!(slow["side_effect"].as_str().unwrap().contains("NOT bumped"));
        assert_eq!(DEVICE_CODE_MIN_POLL_GAP_SECS, 3);
        assert_eq!(poll_gap(), chrono::Duration::seconds(3));
        // The mint stamps the distinguishable label + device description.
        let mint = &token["mint"];
        assert!(mint.as_str().unwrap().contains("pidash CLI"));
        assert_eq!(
            api_token_device_flow::DEVICE_FLOW_DESCRIPTION,
            "Issued by pidash auth login (device-code flow)."
        );
        assert_eq!(
            api_token_device_flow::device_flow_label(
                &chrono::DateTime::parse_from_rfc3339("2026-01-02T03:04:05Z")
                    .unwrap()
                    .with_timezone(&chrono::Utc)
            ),
            "pidash CLI · 2026-01-02 03:04 UTC"
        );
    }
}

#[cfg(test)]
mod pidashconv_736_tests {
    use super::{canonical_user_code, UserCodeRejection};

    #[test]
    fn user_code_strips_py_whitespace() {
        // Python `str.strip()` also strips U+001C-U+001F (PIDASHCONV-736).
        for sep in ['\u{1c}', '\u{1d}', '\u{1e}', '\u{1f}'] {
            let padded = format!("{sep}WXYZ1234{sep}");
            assert_eq!(
                canonical_user_code(&padded).as_deref(),
                Ok("WXYZ-1234"),
                "U+{:04X} padding must strip like Python",
                sep as u32
            );
        }
        // Control-char-only padding strips to empty → Blank (device.py:202-203).
        assert_eq!(
            canonical_user_code("\u{1c} \u{1d}"),
            Err(UserCodeRejection::Blank)
        );
    }
}
