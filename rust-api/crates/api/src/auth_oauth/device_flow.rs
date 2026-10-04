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
//! - The device-start throttle (`DeviceCodeStartThrottle`, 20/minute
//!   per-IP) is specified in `super::guards` but not enforced here: the
//!   Rust edge has no shared DRF-style counter store, matching the merged
//!   handler precedent of documenting the gate without re-implementing
//!   the counter.

use axum::body::{Body, Bytes};
use axum::extract::{Extension, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Router;

use crate::middleware::SessionHandle;
use crate::state::AppState;
use pidash_db::auth_oauth::models::{api_token_device_flow, cli_device_code};

use super::guards::{
    verification_uri, DEVICE_CODE_MIN_POLL_GAP_SECS, DEVICE_CODE_POLL_INTERVAL_SECS,
    DEVICE_CODE_START_MAX_RETRIES, DEVICE_CODE_TTL_SECS,
};

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------

/// Register the three owned device-flow POSTs. Sibling methods stay
/// unmatched and proxy to Django through the fallback (notably Django's
/// own 405s, e.g. `GET device/start/`).
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
/// `handle_exception`'s generic 500 branch.
pub const SERVER_ERROR_BODY: &str = r#"{"error":"Something went wrong please try again later"}"#;

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

/// `{"error": message}` with DRF-compact rendering.
fn error_body(message: &str) -> String {
    format!(
        "{{\"error\":{}}}",
        serde_json::to_string(message).expect("error message serializes")
    )
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

fn json_response(status: StatusCode, body: String) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body))
        .expect("device-flow json response")
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

/// `true` for a Postgres unique-violation (`23505`) — the concurrent
/// generator collision the start retry converts instead of 500ing.
fn is_unique_violation(err: &sqlx::Error) -> bool {
    matches!(err, sqlx::Error::Database(db) if db.code().as_deref() == Some("23505"))
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

/// Locked `cli_device_codes` columns the approve path reads
/// (`id, user_id, approved, denied, consumed, expires_at`).
type ApproveRow = (
    uuid::Uuid,
    Option<uuid::Uuid>,
    bool,
    bool,
    bool,
    chrono::DateTime<chrono::Utc>,
);

/// Locked `cli_device_codes` columns the token path reads
/// (`id, user_id, workspace_id, approved, denied, consumed, expires_at`,
/// `last_polled_at`).
type TokenRow = (
    uuid::Uuid,
    Option<uuid::Uuid>,
    Option<uuid::Uuid>,
    bool,
    bool,
    bool,
    chrono::DateTime<chrono::Utc>,
    Option<chrono::DateTime<chrono::Utc>>,
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

/// Why a raw `user_code` is unusable (`device.py:204-216`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserCodeRejection {
    /// Missing, blank, or not a string (`user_code is required.`).
    Blank,
    /// Fewer or more than 8 code characters after de-formatting
    /// (`user_code must be 8 characters.`).
    BadLength,
}

/// Normalize a raw `user_code` to the canonical `XXXX-XXXX` form
/// (`device.py:204-216`): `strip` + `upper`, drop ASCII spaces and
/// hyphens, require exactly 8 characters, re-insert the hyphen.
///
/// Char-based throughout: Python's `len` counts code points and its
/// slices cut code points, never bytes (Porting guide semantic trap).
pub fn canonical_user_code(raw: &str) -> Result<String, UserCodeRejection> {
    let upper = raw.trim().to_uppercase();
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
    state
        .settings()
        .urls
        .web_url
        .clone()
        .or_else(|| state.settings().urls.app_base_url.clone())
        .unwrap_or_else(|| "http://localhost".to_owned())
}

// ---------------------------------------------------------------------------
// POST /api/v1/auth/device/start/
// ---------------------------------------------------------------------------

/// RFC 8628 §3.1 — issue a device/user code pair (`device.py:145-187`).
///
/// Anonymous (`AllowAny`, empty `authentication_classes`), throttled per
/// IP in Django (`DeviceCodeStartThrottle`; spec in `super::guards`, not
/// enforced here — see module docs). The body is never read.
async fn post_start(State(state): State<AppState>) -> Response {
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    // One grant window for every attempt (`device.py:159`).
    let now = chrono::Utc::now();
    let expires_at = now + chrono::Duration::seconds(DEVICE_CODE_TTL_SECS as i64);

    let mut created: Option<(String, String)> = None;
    for _ in 0..DEVICE_CODE_START_MAX_RETRIES {
        let device_code = cli_device_code::generate_device_code();
        let user_code = cli_device_code::generate_user_code();
        let insert = sqlx::query(
            r#"INSERT INTO cli_device_codes
               (id, created_at, updated_at, device_code, user_code,
                approved, denied, consumed, expires_at)
               VALUES ($1, $2, $3, $4, $5, false, false, false, $6)"#,
        )
        .bind(uuid::Uuid::new_v4())
        .bind(now)
        .bind(now)
        .bind(&device_code)
        .bind(&user_code)
        .bind(expires_at)
        .execute(&pool)
        .await;
        match insert {
            Ok(_) => {
                created = Some((device_code, user_code));
                break;
            }
            Err(err) if is_unique_violation(&err) => {
                tracing::warn!("CLIDeviceCode create collided, retrying: {err}");
            }
            Err(_) => return Denial::ServerError.into_response(),
        }
    }
    let Some((device_code, user_code)) = created else {
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
        }),
    )
}

// ---------------------------------------------------------------------------
// POST /api/v1/auth/device/approve/
// ---------------------------------------------------------------------------

/// Session-auth: the logged-in human approves a pending CLI login
/// (`device.py:190-278`), stamping the row with their user, most-recent
/// workspace (or none) and `approved`.
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
    // `(request.data.get("user_code") or "").strip().upper()`: a missing
    // or non-string value answers the required 400 (Django would 500 on
    // a non-string's `.strip()`; see module docs).
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

    // The approver's email for the 200 shape; an unknown or inactive id
    // is anonymous in Django's session auth, hence the same 401.
    let email: Option<(Option<String>,)> =
        sqlx::query_as(r#"SELECT email FROM users WHERE id = $1 AND is_active"#)
            .bind(actor_id)
            .fetch_optional(&pool)
            .await
            .unwrap_or(None);
    let Some((email,)) = email else {
        return Denial::UnauthorizedDetail.into_response();
    };

    let mut tx = match pool.begin().await {
        Ok(tx) => tx,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let row: Option<ApproveRow> = sqlx::query_as(
        r#"SELECT id, user_id, approved, denied, consumed, expires_at
           FROM cli_device_codes
           WHERE user_code = $1 AND deleted_at IS NULL
           FOR UPDATE"#,
    )
    .bind(&canonical)
    .fetch_optional(&mut *tx)
    .await
    .unwrap_or(None);
    // Roll back on every early return below by dropping `tx`: nothing was
    // written yet, matching the `transaction.atomic()` no-write exits.
    let Some((row_id, row_user_id, approved, denied, consumed, expires_at)) = row else {
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

    // Most-recent active membership (`select_related("workspace")`,
    // `order_by("-created_at")`, `device.py:259-264`).
    let membership: Option<(uuid::Uuid, String)> = sqlx::query_as(
        r#"SELECT wm.workspace_id, w.slug
           FROM workspace_members wm
           JOIN workspaces w ON w.id = wm.workspace_id
           WHERE wm.member_id = $1 AND wm.is_active AND wm.deleted_at IS NULL
           ORDER BY wm.created_at DESC
           LIMIT 1"#,
    )
    .bind(actor_id)
    .fetch_optional(&mut *tx)
    .await
    .unwrap_or(None);
    let (workspace_id, workspace_slug) = match membership {
        Some((id, slug)) => (Some(id), Some(slug)),
        None => (None, None),
    };
    let update = sqlx::query(
        r#"UPDATE cli_device_codes
           SET user_id = $1, workspace_id = $2, approved = true, updated_at = $3
           WHERE id = $4"#,
    )
    .bind(actor_id)
    .bind(workspace_id)
    .bind(now)
    .bind(row_id)
    .execute(&mut *tx)
    .await;
    if update.is_err() {
        return Denial::ServerError.into_response();
    }
    if tx.commit().await.is_err() {
        return Denial::ServerError.into_response();
    }
    ok_response(
        StatusCode::OK,
        serde_json::json!({
            "ok": true,
            "user_email": email,
            "workspace_slug": workspace_slug,
        }),
    )
}

// ---------------------------------------------------------------------------
// POST /api/v1/auth/device/token/
// ---------------------------------------------------------------------------

/// RFC 8628 §3.4 — the CLI polls here trading `device_code` for an
/// `APIToken` (`device.py:281-376`).
///
/// Branch order is the Python order: blank → unknown → consumed → denied
/// → expired → slow_down (no touch) → pending (touch) → mint + consume.
async fn post_token(State(state): State<AppState>, body: Bytes) -> Response {
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
    let row: Option<TokenRow> = sqlx::query_as(
        r#"SELECT id, user_id, workspace_id, approved, denied, consumed,
                  expires_at, last_polled_at
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
    // Minimum poll gap (`device.py:319-336`): the first poll is always
    // accepted; a `slow_down` rejection must NOT bump `last_polled_at`
    // or a spammer could starve the legit CLI (anti-starvation).
    if last_polled_at.is_some_and(|last| now - last < poll_gap()) {
        return Denial::error(StatusCode::BAD_REQUEST, "slow_down").into_response();
    }

    if !approved || row_user_id.is_none() {
        let touched = sqlx::query(
            r#"UPDATE cli_device_codes
               SET last_polled_at = $1, updated_at = $2
               WHERE id = $3"#,
        )
        .bind(now)
        .bind(now)
        .bind(row_id)
        .execute(&mut *tx)
        .await;
        if touched.is_err() || tx.commit().await.is_err() {
            return Denial::ServerError.into_response();
        }
        return Denial::error(StatusCode::BAD_REQUEST, "authorization_pending").into_response();
    }

    // Mint: override the default opaque-hex label so device tokens are
    // distinguishable from user-created PATs (`device.py:340-349`).
    let user_id = row_user_id.expect("approved row carries a user");
    let api_token = format!("pi_dash_api_{}", uuid::Uuid::new_v4().simple());
    let minted = sqlx::query(
        r#"INSERT INTO api_tokens
           (id, created_at, updated_at, token, label, description,
            is_active, user_type, user_id, workspace_id,
            is_service, allowed_rate_limit)
           VALUES ($1, $2, $3, $4, $5, $6, true, 0, $7, $8, false, '60/min')"#,
    )
    .bind(uuid::Uuid::new_v4())
    .bind(now)
    .bind(now)
    .bind(&api_token)
    .bind(api_token_device_flow::device_flow_label(&now))
    .bind(api_token_device_flow::DEVICE_FLOW_DESCRIPTION)
    .bind(user_id)
    .bind(row_workspace_id)
    .execute(&mut *tx)
    .await;
    if minted.is_err() {
        return Denial::ServerError.into_response();
    }
    let consumed = sqlx::query(
        r#"UPDATE cli_device_codes
           SET consumed = true, last_polled_at = $1, updated_at = $2
           WHERE id = $3"#,
    )
    .bind(now)
    .bind(now)
    .bind(row_id)
    .execute(&mut *tx)
    .await;
    if consumed.is_err() || tx.commit().await.is_err() {
        return Denial::ServerError.into_response();
    }

    // Response identity (`device.py:355-362`): the grant user's email and
    // workspace slug, each null when the FK is null. A dangling FK would
    // raise in Django, hence `ServerError` here.
    let user_email: Option<(Option<String>,)> =
        sqlx::query_as(r#"SELECT email FROM users WHERE id = $1"#)
            .bind(user_id)
            .fetch_optional(&pool)
            .await
            .unwrap_or(None);
    let Some((user_email,)) = user_email else {
        return Denial::ServerError.into_response();
    };
    let workspace_slug: Option<String> = match row_workspace_id {
        None => None,
        Some(workspace_id) => {
            let slug: Option<(String,)> =
                sqlx::query_as(r#"SELECT slug FROM workspaces WHERE id = $1"#)
                    .bind(workspace_id)
                    .fetch_optional(&pool)
                    .await
                    .unwrap_or(None);
            match slug {
                Some((slug,)) => Some(slug),
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

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
                "interval"
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
