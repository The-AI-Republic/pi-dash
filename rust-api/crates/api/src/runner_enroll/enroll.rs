#![forbid(unsafe_code)]
#![allow(clippy::result_large_err)]

//! Daemon enrollment handlers (D-13 handlers-A, PIDASHCONV-590).
//!
//! Port of `runner/views/enrollment.py` (the daemon enrollment units) plus
//! `HealthEndpoint` (`runner/views/register.py:19-28`):
//!
//! * `POST /api/v1/runner/runners/enroll/` (`:258-377`) — one-time
//!   enrollment-token redeem.
//! * `POST /api/v1/runner/runners/` (`:545-787`) — CLI runner creation.
//! * `POST /api/runners/machine-tokens/<ws>/tickets/` (`:799-850`) —
//!   web ticket mint (Redis `SET EX 60`).
//! * `POST /api/v1/runner/machine-tokens/` (`:853-932`) — ticket redeem.
//! * `GET /api/v1/runner/health/` — AllowAny probe.
//! * `POST /api/runners/invites/` (`:206-224`) and `POST
//!   /api/runners/<rid>/revive/` (`:479-500`) — 410 deprecations.
//!
//! Fixture ids D13-F2 (request shapes), D13-F3 (mint formats), D13-F4
//! (auth), D13-F5 (enroll SQL) and D13-F7 (endpoint bodies); the
//! auto-name / cap / name-charset helpers also pin their D13-F6 goldens.
//! Shelf reuse (all in-domain, merged): `validate_enroll_request` +
//! `runner_name_is_valid` (serializers), the `enroll_reads` statement
//! builders, the `tokens` mints, the `auth` extractors, the `throttle`
//! math, the `columns` orders, and `deactivate_api_token`
//! (`db::auth_oauth::queries`). Denial bodies render LOCALLY (lowercase
//! `detail`, verified against live Django): the `auth` denial renderers
//! emit capital-`Detail` (PIDASHCONV-589 follow-up filed separately).
//! Everything else (session resolve, throttle wiring, ticket Redis, tx
//! bodies) is owned here — no cross-domain code dependency.
//!
//! Ported bugs (translate, don't redesign; also listed in the PR):
//!
//! * Ticket `SET EX` is skipped when Redis is `None`, but the endpoint
//!   still 201s an unredeemable ticket (`:832-850`).
//! * Rotate drops `user` from the dev-machine filter while bootstrap
//!   keeps it (`:181-190`; the `rotate_revoke_sql` machine branch).
//! * An unknown `?pod=` name falls through to the project default pod
//!   instead of 404ing (`:671-675`).
//! * The managed-cap 409 returns from inside the attempt `atomic()`
//!   block, so the dev-machine touch/create commits (`:723-725`).
//! * A rotate-insert race surfaces as `runner_name_taken` / an
//!   auto-name retry: the attempt `except IntegrityError` cannot tell it
//!   from a `(pod, name)` collision (`:736-759`).
//! * The enroll request label is sliced without stripping while the
//!   D-path strips first (`:281` vs `:78`).

use std::net::SocketAddr;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::extract::{ConnectInfo, Extension, Path, State};
use axum::http::{header, HeaderMap, Request, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Router;
use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use http_body_util::BodyExt as _;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use sqlx::postgres::{PgRow, Postgres};
use sqlx::{Row, Transaction};
use uuid::Uuid;

use pidash_db::auth_oauth::queries::cli_token::deactivate_api_token;
use pidash_db::runner_enroll::columns::{
    dev_machine as dm_cols, enums, machine_token as mt_cols, pod as pod_cols, runner as r_cols,
};
use pidash_services::runner_enroll::queries::enroll_reads as enroll_sql;
use pidash_services::runner_enroll::serializers::shapes as enroll_shapes;
use pidash_services::runner_enroll::tokens as enroll_tokens;

use super::auth as enroll_auth;
use super::throttle as enroll_throttle;
use crate::middleware::SessionHandle;
use crate::state::AppState;

// ---------------------------------------------------------------------------
// Wire consts (`enrollment.py`, `register.py`)
// ---------------------------------------------------------------------------

/// `HEALTH` body protocol version (`register.py:20`). The enroll/create
/// bodies report 4 (a literal in each view) — ported verbatim per F7.
pub const HEALTH_PROTOCOL_VERSION: i64 = 3;

/// Enroll/create body protocol version (`enrollment.py:372,782`).
pub const ENROLL_PROTOCOL_VERSION: i64 = 4;

/// Enroll/create `long_poll_interval_secs` (`:371,781`): a literal 25 in
/// both views, not the settings value.
pub const LONG_POLL_INTERVAL_SECS: i64 = 25;

/// Auto-name retry budget (`_MAX_AUTO_NAME_RETRIES`, `:542`).
pub const MAX_AUTO_NAME_RETRIES: u32 = 5;

/// Ticket TTL (`:846`; the 201 `expires_in_secs` mirrors it).
pub const TICKET_TTL_SECS: u64 = 60;

/// Ticket key prefix (`:837`).
pub const TICKET_KEY_PREFIX: &str = "machine_token_ticket:";

/// `BaseSessionAuthentication` backend gate: `AUTHENTICATION_BACKENDS` is
/// exactly `(ModelBackend,)` (`settings/common.py:125`).
const MODEL_BACKEND: &str = "django.contrib.auth.backends.ModelBackend";

/// Salt for `user.get_session_auth_hash()`
/// (`django.contrib.auth` `HASH_SESSION_KEY` verification).
const SESSION_AUTH_HASH_SALT: &str =
    "django.contrib.auth.models.AbstractBaseUser.get_session_auth_hash";

// ---------------------------------------------------------------------------
// Shared responses (the `runner_runs` shape, restated: no cross-domain dep)
// ---------------------------------------------------------------------------

/// Unhandled-failure body (500). Runner views have no custom
/// `handle_exception`, so Django renders its HTML error page here; only
/// the status is contract-pinned.
pub const SERVER_ERROR_BODY: &str = r#"{"error":"Something went wrong please try again later"}"#;

/// Render a JSON body with the DRF content type.
pub fn json_response(status: StatusCode, body: String) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body))
        .expect("runner-enroll response builds")
}

/// The request pool, or the 500 when the server runs pool-less
/// (unreachable in `serve`, which fail-fasts at boot).
pub fn pool_of(state: &AppState) -> Result<&sqlx::PgPool, Response> {
    match state.pools().map(|pools| pools.primary()) {
        Some(pool) => Ok(pool),
        None => Err(json_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            SERVER_ERROR_BODY.to_owned(),
        )),
    }
}

/// The 500 shortcut for fallible handler paths.
pub fn server_error() -> Response {
    json_response(
        StatusCode::INTERNAL_SERVER_ERROR,
        SERVER_ERROR_BODY.to_owned(),
    )
}

/// `{"error": name}` (view-inline 4xx bodies).
pub fn error_body(name: &str) -> String {
    serde_json::json!({ "error": name }).to_string()
}

/// `{"error": name, "error_description": desc}`.
pub fn error_desc_body(name: &str, desc: &str) -> String {
    serde_json::json!({ "error": name, "error_description": desc }).to_string()
}

/// Render a DRF `AuthenticationFailed(code)` denial: 401 + the
/// `WWW-Authenticate` challenge when the view's first authenticator
/// supplies one, else the DRF-default 403 — always with the lowercase
/// `detail` key stock DRF 3.15.2 emits (verified against live Django;
/// the `auth` renderers' capital-`Detail` is a PIDASHCONV-589 bug with
/// its own fix issue, so handlers-A renders locally).
pub fn auth_denied(code: &str, header: Option<&str>) -> Response {
    let builder = Response::builder()
        .status(if header.is_some() {
            StatusCode::UNAUTHORIZED
        } else {
            StatusCode::FORBIDDEN
        })
        .header(header::CONTENT_TYPE, "application/json");
    let builder = match header {
        Some(challenge) => builder.header(header::WWW_AUTHENTICATE, challenge),
        None => builder,
    };
    builder
        .body(axum::body::Body::from(
            serde_json::json!({ "detail": code }).to_string(),
        ))
        .expect("auth denial builds")
}

/// Render a `NotAuthenticated` denial: the project's
/// `auth_exception_handler` forces 401 with no challenge on these
/// views (verified against live Django).
pub fn not_authenticated() -> Response {
    json_response(
        StatusCode::UNAUTHORIZED,
        serde_json::json!({ "detail": "Authentication credentials were not provided." })
            .to_string(),
    )
}

/// Integrity errors (`IntegrityError`, Postgres class 23): unique / FK /
/// not-null violations. Django catches these around the D2/B2 savepoint
/// inserts and the create attempt body; anything else escapes to 500.
fn is_integrity_error(err: &sqlx::Error) -> bool {
    match err {
        sqlx::Error::Database(db) => db.code().is_some_and(|code| code.starts_with("23")),
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// DRF request-data reader (`Request._parse`, DRF 3.15.2; the `runner_runs`
// `read_request_data` precedent, restated: no cross-domain dep)
// ---------------------------------------------------------------------------

/// Read `request.data` for a POST body: content-length 0 validates as
/// `{}` with the body ignored; a non-JSON content type proxies to
/// Django (form posts stay on the Python plane); whitespace-only JSON
/// 400s with CPython's `Expecting value` position; unparsable JSON
/// 400s `{"detail": "JSON parse error"}` (lowercase key; the message
/// keeps the merged precedent's truncation — Django appends the full
/// CPython message, which no contract test pins).
pub async fn read_request_data(
    state: &AppState,
    req: Request<axum::body::Body>,
) -> Result<Value, Response> {
    let content_length = req
        .headers()
        .get(header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|raw| raw.trim().parse::<usize>().ok())
        .unwrap_or(0);
    if content_length == 0 {
        return Ok(Value::Object(Map::new()));
    }
    let raw_type = req
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    let main = raw_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_lowercase();
    if main != "application/json" {
        return Err(crate::edge::proxy(State(state.clone()), req).await);
    }
    let (parts, body) = req.into_parts();
    let _ = parts;
    let bytes = body
        .collect()
        .await
        .map(|collected| collected.to_bytes())
        .map_err(|_| server_error())?;
    if bytes.iter().all(|byte| byte.is_ascii_whitespace()) {
        let position = bytes.len();
        return Err(json_response(
            StatusCode::BAD_REQUEST,
            format!(
                "{{\"detail\":\"JSON parse error - Expecting value: line 1 column {} (char {})\"}}",
                position + 1,
                position
            ),
        ));
    }
    match serde_json::from_slice::<Value>(&bytes) {
        Ok(value) => Ok(value),
        Err(_) => Err(json_response(
            StatusCode::BAD_REQUEST,
            r#"{"detail":"JSON parse error"}"#.to_owned(),
        )),
    }
}

// ---------------------------------------------------------------------------
// Python scalar semantics (small twins of the `runner_runs` helpers)
// ---------------------------------------------------------------------------

/// Python truthiness for JSON frame values (`None`/`False`/`0`/`""`/
/// `[]`/`{}` are falsy; everything else is truthy).
pub fn py_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => {
            if n.is_i64() {
                n.as_i64() != Some(0)
            } else if n.is_u64() {
                n.as_u64() != Some(0)
            } else {
                n.as_f64().is_some_and(|f| f != 0.0)
            }
        }
        Value::String(s) => !s.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
    }
}

/// Python `str.strip()`: `str.isspace()` is Unicode `White_Space` plus
/// U+001C-U+001F and U+0085; Rust `trim()` strips `White_Space` only.
pub fn py_strip(s: &str) -> &str {
    s.trim_matches(|c: char| {
        c.is_whitespace() || c == '\u{85}' || ('\u{1c}'..='\u{1f}').contains(&c)
    })
}

/// First `max` Unicode code points (`text[:max]`).
pub fn truncate_chars(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    text.chars().take(max).collect()
}

/// `(request.data.get(key) or "")`: falsy values map to `""`, strings
/// pass through, truthy non-strings are the source's `AttributeError`
/// (500) — `int` has no `.strip()`. The body must be a dict: anything
/// else is the same 500 (`.get` on a list/str/int/`None`).
pub fn frame_raw(body: &Value, key: &str) -> Result<String, Response> {
    let Value::Object(obj) = body else {
        return Err(server_error());
    };
    let value = obj.get(key).unwrap_or(&Value::Null);
    if !py_truthy(value) {
        return Ok(String::new());
    }
    match value.as_str() {
        Some(text) => Ok(text.to_owned()),
        None => Err(server_error()),
    }
}

/// `(request.data.get(key) or "").strip()[:max]`.
pub fn frame_text(body: &Value, key: &str, max: usize) -> Result<String, Response> {
    Ok(truncate_chars(py_strip(&frame_raw(body, key)?), max))
}

/// `(request.data.get(key) or "").strip()` (no truncation).
pub fn frame_stripped(body: &Value, key: &str) -> Result<String, Response> {
    Ok(py_strip(&frame_raw(body, key)?).to_owned())
}

/// Python `datetime.isoformat()` for an aware UTC timestamp: `+00:00`
/// offset, six-digit microseconds only when nonzero (the `prompting`
/// `render_isoformat` precedent, restated).
pub fn render_isoformat(dt: DateTime<Utc>) -> String {
    if dt.timestamp_subsec_micros() == 0 {
        dt.to_rfc3339_opts(chrono::SecondsFormat::Secs, false)
    } else {
        dt.to_rfc3339_opts(chrono::SecondsFormat::Micros, false)
    }
}

/// `json.dumps(..., ensure_ascii=True)` over compact `serde_json` output
/// (the `tokens::ensure_ascii` twin, which is private there):
/// `serde_json` already escapes `"`, `\` and control chars identically
/// and emits raw UTF-8 otherwise, so only DEL and non-ASCII (lowercase
/// `\uXXXX`, surrogate pairs above U+FFFF) need rewriting.
pub fn ensure_ascii(compact_json: &str) -> String {
    let mut out = String::with_capacity(compact_json.len());
    for c in compact_json.chars() {
        if c.is_ascii() && c != '\u{7f}' {
            out.push(c);
        } else if c == '\u{7f}' {
            out.push_str("\\u007f");
        } else {
            let n = c as u32;
            if n < 0x1_0000 {
                out.push_str(&format!("\\u{n:04x}"));
            } else {
                let v = n - 0x1_0000;
                let (hi, lo) = (0xd800 + (v >> 10), 0xdc00 + (v & 0x3ff));
                out.push_str(&format!("\\u{hi:04x}\\u{lo:04x}"));
            }
        }
    }
    out
}

/// The ticket payload bytes (`:838-844`): `json.dumps` with default
/// separators (`', '`, `': '`), `ensure_ascii`, insertion order
/// `user_id`, `workspace_id`, `host_label`.
pub fn ticket_payload_bytes(user_id: &Uuid, workspace_id: &Uuid, host_label: &str) -> String {
    let host = ensure_ascii(&serde_json::to_string(host_label).expect("host label serializes"));
    format!("{{\"user_id\": \"{user_id}\", \"workspace_id\": \"{workspace_id}\", \"host_label\": {host}}}")
}

/// The ticket cache key (`:837`).
pub fn ticket_key(ticket: &str) -> String {
    format!("{TICKET_KEY_PREFIX}{ticket}")
}

/// `f"runner_{count + 1:03d}"` (`:796`; F6 `next_auto_runner_name`):
/// minimum width 3, overflows past 999 (`runner_1000`).
pub fn next_auto_runner_name(pod_runner_count: i64) -> String {
    format!("runner_{:03}", pod_runner_count + 1)
}

// ---------------------------------------------------------------------------
// Session resolve (`BaseSessionAuthentication` + `IsAuthenticated`)
// ---------------------------------------------------------------------------

/// `user.get_session_auth_hash()`: `salted_hmac(<salt>, password)` with
/// SHA256, compared constant-time (the `license::resolve_actor`
/// precedent, restated without the timezone arm: runner views render no
/// datetimes, so a bad `user_timezone` must not 500 here).
fn verify_session_hash(session_hash: &str, password_field: &str, secret_key: &[u8]) -> bool {
    let key = Sha256::digest([SESSION_AUTH_HASH_SALT.as_bytes(), secret_key].concat());
    let mut mac = Hmac::<Sha256>::new_from_slice(&key).expect("HMAC-SHA256 accepts any key length");
    mac.update(password_field.as_bytes());
    let expected = hex_encode(&mac.finalize().into_bytes());
    constant_time_eq(expected.as_bytes(), session_hash.as_bytes())
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

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for i in 0..a.len() {
        diff |= a[i] ^ b[i];
    }
    diff == 0
}

/// `request.user` from the Django session, or `None` for anonymous.
/// Mirrors `django.contrib.auth.get_user` + DRF `SessionAuthentication`
/// (CSRF is disabled on `BaseSessionAuthentication`): the session must
/// carry a UUID `_auth_user_id` for the `ModelBackend` with a matching
/// `_auth_user_hash`, and the user row must exist and be active.
pub async fn session_user_id(
    pool: &sqlx::PgPool,
    secret_key: &[u8],
    extension: Option<Extension<SessionHandle>>,
) -> Result<Option<Uuid>, Response> {
    let Some(Extension(handle)) = extension else {
        return Ok(None);
    };
    let mut session = handle.snapshot();
    let user_id_raw = session
        .get("_auth_user_id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_owned();
    let backend = session
        .get("_auth_user_backend")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_owned();
    let session_hash = session
        .get("_auth_user_hash")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_owned();
    if backend != MODEL_BACKEND {
        return Ok(None);
    }
    let Ok(user_id) = user_id_raw.parse::<Uuid>() else {
        return Ok(None);
    };
    let row: Option<(Uuid, String, bool)> =
        sqlx::query_as(r#"SELECT "id", "password", "is_active" FROM "users" WHERE "id" = $1"#)
            .bind(user_id)
            .fetch_optional(pool)
            .await
            .map_err(|_| server_error())?;
    let Some((id, password, is_active)) = row else {
        return Ok(None);
    };
    if !is_active {
        return Ok(None);
    }
    if session_hash.is_empty() || !verify_session_hash(&session_hash, &password, secret_key) {
        return Ok(None);
    }
    Ok(Some(id))
}

// ---------------------------------------------------------------------------
// Anon throttle wiring (the `assistant::redis` precedent, restated)
// ---------------------------------------------------------------------------

/// Decode a cached throttle history: the JSON array of `time.time()`
/// floats this handler writes. Anything else decodes to the empty
/// history, i.e. fail-open to allow.
pub fn decode_throttle_history(raw: &str) -> Vec<f64> {
    serde_json::from_str(raw).unwrap_or_default()
}

/// Enforce the inherited `AnonRateThrottle` (30/minute) on an `AllowAny`
/// endpoint: DRF's sliding window over the cached timestamp history at
/// `throttle_anon_<ident>`. A missing client, a cache miss, an
/// unreadable value, and a failed re-cache all fail open to allow;
/// quota exhaustion answers the rewritten 429 (`Throttled_anon`).
pub async fn check_anon_throttle(
    state: &AppState,
    headers: &HeaderMap,
    remote: Option<SocketAddr>,
) -> Result<(), Response> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs_f64())
        .unwrap_or(0.0);
    let Some(redis) = state.redis() else {
        return Ok(());
    };
    let xff = headers
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok());
    let remote = remote.map(|addr| addr.ip().to_string()).unwrap_or_default();
    let ident = enroll_throttle::get_ident(xff, &remote);
    let key = enroll_throttle::anon_cache_key(false, &ident).expect("anon key");
    let history: Vec<f64> = match redis.get_string(&key).await {
        Ok(Some(raw)) => decode_throttle_history(&raw),
        _ => Vec::new(),
    };
    let spec = enroll_throttle::ANON_THROTTLE;
    let trimmed: Vec<f64> = history
        .into_iter()
        .filter(|stamp| *stamp > now - spec.window_secs as f64)
        .collect();
    if !enroll_throttle::allow_request(&trimmed, now, spec.requests, spec.window_secs) {
        let wait = enroll_throttle::throttle_wait(&trimmed, now, spec.requests, spec.window_secs);
        return Err(enroll_throttle::AnonThrottled::new(wait).into_response());
    }
    let mut recached = Vec::with_capacity(trimmed.len() + 1);
    recached.push(now);
    recached.extend(trimmed);
    if let Err(error) = redis
        .set_ex(
            &key,
            &serde_json::to_string(&recached).unwrap_or_default(),
            spec.window_secs,
        )
        .await
    {
        tracing::debug!(%error, key = key.as_str(), "runner_enroll.throttle: re-cache failed; allowance stands");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Ticket Redis (raw client: the `LivePorts` / `auth_session::magic`
// precedent — `RedisHandle` exposes no GETDEL/DEL and the kernel is
// read-only, so callers build the client from settings)
// ---------------------------------------------------------------------------

/// An api-crate-owned Redis client, or `None` when `REDIS_URL` is unset,
/// empty or unparsable — mirroring `redis_instance()` returning `None`.
pub fn ticket_redis_client(state: &AppState) -> Option<redis::Client> {
    state
        .settings()
        .redis
        .url
        .as_deref()
        .filter(|url| !url.is_empty())
        .and_then(|url| redis::Client::open(url).ok())
}

/// Consume a ticket blob (`:876-884`): atomic `GETDEL`, falling back to
/// `GET` + `DELETE`-when-present on ANY `GETDEL` error (pre-6.2
/// servers). Transport failures outside the `GETDEL` attempt escape to
/// 500, exactly like the unguarded Python calls.
pub async fn redeem_ticket_blob(
    client: &redis::Client,
    key: &str,
) -> Result<Option<Vec<u8>>, Response> {
    let mut conn = client
        .get_multiplexed_async_connection()
        .await
        .map_err(|_| server_error())?;
    let getdel: Result<Option<Vec<u8>>, redis::RedisError> =
        redis::cmd("GETDEL").arg(key).query_async(&mut conn).await;
    match getdel {
        Ok(blob) => Ok(blob),
        Err(_) => {
            let blob: Option<Vec<u8>> = redis::cmd("GET")
                .arg(key)
                .query_async(&mut conn)
                .await
                .map_err(|_| server_error())?;
            if blob.is_some() {
                let _: i64 = redis::cmd("DEL")
                    .arg(key)
                    .query_async(&mut conn)
                    .await
                    .map_err(|_| server_error())?;
            }
            Ok(blob)
        }
    }
}

// ---------------------------------------------------------------------------
// Membership (the `auth::workspace_member_role` shape, restated: private)
// ---------------------------------------------------------------------------

/// `is_workspace_member(user, workspace_id)` (`core/permissions.py:28-34`):
/// an active, non-soft-deleted `WorkspaceMember` row.
pub async fn workspace_member_exists(
    pool: &sqlx::PgPool,
    workspace_id: Uuid,
    user_id: Uuid,
) -> Result<bool, Response> {
    let role: Option<i16> = sqlx::query_scalar(
        r#"SELECT "role" FROM "workspace_members"
           WHERE "workspace_id" = $1 AND "member_id" = $2
             AND "is_active" AND "deleted_at" IS NULL
           LIMIT 1"#,
    )
    .bind(workspace_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| server_error())?;
    Ok(role.is_some())
}

// ---------------------------------------------------------------------------
// Row decodes (positional over the `enroll_reads` full-row selects)
// ---------------------------------------------------------------------------

/// Positional index of `name` inside a `COLUMNS` order.
fn col_index(columns: &[&str], name: &str) -> usize {
    columns
        .iter()
        .position(|col| *col == name)
        .expect("fixture column exists")
}

/// The E1 locked row (`:284-290`): runner + workspace slug + pod name +
/// project identifier, decoded positionally.
pub struct EnrollRow {
    pub runner_id: Uuid,
    pub owner_id: Uuid,
    pub workspace_id: Uuid,
    pub pod_id: Uuid,
    pub name: String,
    pub host_label: String,
    pub enrolled_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub workspace_slug: String,
    pub pod_name: String,
    pub project_identifier: String,
}

fn decode_enroll_row(row: &PgRow) -> Result<EnrollRow, Response> {
    let ws_base = r_cols::COLUMNS.len();
    let pod_base = ws_base + enroll_sql::WORKSPACES_SELECT_COLUMNS.len();
    let proj_base = pod_base + pod_cols::COLUMNS.len();
    let get = |idx: usize| row.try_get::<String, _>(idx).map_err(|_| server_error());
    Ok(EnrollRow {
        runner_id: row.try_get(0).map_err(|_| server_error())?,
        owner_id: row.try_get(1).map_err(|_| server_error())?,
        workspace_id: row.try_get(2).map_err(|_| server_error())?,
        pod_id: row.try_get(4).map_err(|_| server_error())?,
        name: get(5)?,
        host_label: get(6)?,
        enrolled_at: row.try_get(16).map_err(|_| server_error())?,
        revoked_at: row.try_get(28).map_err(|_| server_error())?,
        workspace_slug: get(ws_base + col_index(enroll_sql::WORKSPACES_SELECT_COLUMNS, "slug"))?,
        pod_name: get(pod_base + col_index(pod_cols::COLUMNS, "name"))?,
        project_identifier: get(proj_base
            + col_index(
                pidash_db::app_project::models::project::COLUMNS,
                "identifier",
            ))?,
    })
}

/// A locked dev-machine row (D1/D3): thetouch inputs plus provisioning.
pub struct LockedDevMachine {
    pub id: Uuid,
    pub owner_id: Uuid,
    pub host_label: String,
    pub label: String,
    pub provisioning: String,
}

fn decode_dev_machine(row: &PgRow) -> Result<LockedDevMachine, Response> {
    Ok(LockedDevMachine {
        id: row.try_get(0).map_err(|_| server_error())?,
        owner_id: row.try_get(1).map_err(|_| server_error())?,
        host_label: row.try_get::<String, _>(2).map_err(|_| server_error())?,
        label: row.try_get::<String, _>(3).map_err(|_| server_error())?,
        provisioning: row.try_get::<String, _>(5).map_err(|_| server_error())?,
    })
}

/// A workspace hit (E6a/E6b): id + slug.
pub struct WorkspaceHit {
    pub id: Uuid,
    pub slug: String,
}

fn decode_workspace_at(row: &PgRow, base: usize) -> Result<WorkspaceHit, Response> {
    let cols = enroll_sql::WORKSPACES_SELECT_COLUMNS;
    Ok(WorkspaceHit {
        id: row
            .try_get(base + col_index(cols, "id"))
            .map_err(|_| server_error())?,
        slug: row
            .try_get::<String, _>(base + col_index(cols, "slug"))
            .map_err(|_| server_error())?,
    })
}

/// A pod hit (E6d): id + name + project.
pub struct PodHit {
    pub id: Uuid,
    pub name: String,
    pub project_id: Uuid,
}

fn decode_pod(row: &PgRow) -> Result<PodHit, Response> {
    Ok(PodHit {
        id: row.try_get(0).map_err(|_| server_error())?,
        name: row.try_get::<String, _>(3).map_err(|_| server_error())?,
        project_id: row.try_get(2).map_err(|_| server_error())?,
    })
}

/// A project hit (E6c): id + identifier.
pub struct ProjectHit {
    pub id: Uuid,
    pub identifier: String,
}

fn decode_project(row: &PgRow) -> Result<ProjectHit, Response> {
    let cols = pidash_db::app_project::models::project::COLUMNS;
    Ok(ProjectHit {
        id: row
            .try_get(col_index(cols, "id"))
            .map_err(|_| server_error())?,
        identifier: row
            .try_get::<String, _>(col_index(cols, "identifier"))
            .map_err(|_| server_error())?,
    })
}

/// A reused desktop runner row (E7 probe): id + name + generation.
pub struct ReuseHit {
    pub id: Uuid,
    pub name: String,
    pub refresh_token_generation: i32,
}

fn decode_reuse(row: &PgRow) -> Result<ReuseHit, Response> {
    Ok(ReuseHit {
        id: row.try_get(0).map_err(|_| server_error())?,
        name: row.try_get::<String, _>(5).map_err(|_| server_error())?,
        refresh_token_generation: row.try_get(11).map_err(|_| server_error())?,
    })
}

// ---------------------------------------------------------------------------
// E1: enroll locked read
// ---------------------------------------------------------------------------

/// E1 (`:284-290`): locked runner by one-time enrollment hash. Miss →
/// `None` (401); two rows → 500 (Django's `MultipleObjectsReturned`).
pub async fn fetch_enroll_runner(
    tx: &mut Transaction<'_, Postgres>,
    token_hash: &str,
) -> Result<Option<EnrollRow>, Response> {
    let rows: Vec<PgRow> = sqlx::query(&enroll_sql::enroll_locked_read_sql())
        .bind(token_hash)
        .fetch_all(&mut **tx)
        .await
        .map_err(|_| server_error())?;
    if rows.len() > 1 {
        return Err(server_error());
    }
    rows.into_iter()
        .next()
        .map(|row| decode_enroll_row(&row))
        .transpose()
}

/// E2 (`:325-348`): mark enrolled + rotate to refresh generation 1.
/// `updated_at` untouched (not in the field list).
#[allow(clippy::too_many_arguments)]
pub async fn mark_runner_enrolled(
    tx: &mut Transaction<'_, Postgres>,
    runner_id: Uuid,
    dev_machine_id: Option<Uuid>,
    host_label: &str,
    refresh_hash: &str,
    refresh_fingerprint: &str,
    body_name: &str,
    enrolled_at: DateTime<Utc>,
) -> Result<(), Response> {
    let with_name = !body_name.is_empty();
    let sql = enroll_sql::enroll_mark_enrolled_sql(with_name);
    let mut query = sqlx::query(&sql).bind(dev_machine_id);
    if with_name {
        query = query.bind(body_name);
    }
    query
        .bind(host_label)
        .bind(refresh_hash)
        .bind(refresh_fingerprint)
        .bind(1i32)
        .bind("")
        .bind("")
        .bind("")
        .bind(enrolled_at)
        .bind(runner_id)
        .execute(&mut **tx)
        .await
        .map_err(|_| server_error())?;
    Ok(())
}

// ---------------------------------------------------------------------------
// D-path: `_get_or_create_dev_machine` (`:72-128`) + `_touch_dev_machine`
// ---------------------------------------------------------------------------

/// `_touch_dev_machine` write (`:57-69`) for a locked row.
async fn touch_dev_machine(
    tx: &mut Transaction<'_, Postgres>,
    machine: &LockedDevMachine,
    request_label: &str,
) -> Result<(), Response> {
    let selection = enroll_sql::touch_selection(request_label, &machine.host_label, &machine.label);
    let normalized = enroll_sql::normalize_host_label(request_label);
    let sql =
        enroll_sql::touch_dev_machine_sql(selection.update_host_label, selection.update_label);
    let mut query = sqlx::query(&sql);
    if selection.update_host_label {
        query = query.bind(&normalized);
    }
    if selection.update_label {
        query = query.bind(enroll_sql::dev_machine_label(&normalized));
    }
    let now = Utc::now();
    query
        .bind(now)
        .bind(now)
        .bind(machine.id)
        .execute(&mut **tx)
        .await
        .map_err(|_| server_error())?;
    Ok(())
}

/// D2 insert binds (`:88-94,113-118`): id, owner, host, label,
/// visibility 0, provisioning manual, last_seen now, revoked NULL,
/// created/updated now.
async fn insert_dev_machine(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    owner_id: Uuid,
    host_label: &str,
) -> Result<(), sqlx::Error> {
    let now = Utc::now();
    sqlx::query(&enroll_sql::dev_machine_insert_sql())
        .bind(id)
        .bind(owner_id)
        .bind(host_label)
        .bind(enroll_sql::dev_machine_label(host_label))
        .bind(0i16)
        .bind(enums::RUNNER_PROVISIONING_MANUAL)
        .bind(now)
        .bind(None::<DateTime<Utc>>)
        .bind(now)
        .bind(now)
        .execute(&mut **tx)
        .await
        .map(|_| ())
}

/// D1 re-lock by id.
async fn lock_dev_machine_by_id(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
) -> Result<Option<LockedDevMachine>, Response> {
    let row: Option<PgRow> = sqlx::query(&enroll_sql::dev_machine_by_id_sql())
        .bind(id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|_| server_error())?;
    row.map(|row| decode_dev_machine(&row)).transpose()
}

/// How `_get_or_create_dev_machine` failed: an ownership 404 or a 500.
pub enum DevMachineFail {
    Ownership,
    Server,
}

/// The D-path result the callers need: row id + provisioning.
pub struct DevMachineTouch {
    pub id: Uuid,
    pub provisioning: String,
}

/// `_get_or_create_dev_machine` (`:72-128`): locked-by-id + ownership
/// error, create-with-id + race retry, locked-by-owner-host + legacy
/// race path, `_touch_dev_machine` field updates.
pub async fn get_or_create_dev_machine(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    dev_machine_id: Option<Uuid>,
    request_label: &str,
) -> Result<Option<DevMachineTouch>, DevMachineFail> {
    let err = |_: Response| DevMachineFail::Server;
    let host_label = enroll_sql::normalize_host_label(request_label);
    if let Some(id) = dev_machine_id {
        if let Some(locked) = lock_dev_machine_by_id(tx, id).await.map_err(err)? {
            if locked.owner_id != user_id {
                return Err(DevMachineFail::Ownership);
            }
            touch_dev_machine(tx, &locked, &host_label)
                .await
                .map_err(err)?;
            return Ok(Some(DevMachineTouch {
                id: locked.id,
                provisioning: locked.provisioning,
            }));
        }
        sqlx::query("SAVEPOINT enroll_dev_machine_create")
            .execute(&mut **tx)
            .await
            .map_err(|_| DevMachineFail::Server)?;
        match insert_dev_machine(tx, id, user_id, &host_label).await {
            Ok(()) => {
                sqlx::query("RELEASE SAVEPOINT enroll_dev_machine_create")
                    .execute(&mut **tx)
                    .await
                    .map_err(|_| DevMachineFail::Server)?;
                return Ok(Some(DevMachineTouch {
                    id,
                    provisioning: enums::RUNNER_PROVISIONING_MANUAL.to_owned(),
                }));
            }
            Err(error) if is_integrity_error(&error) => {
                sqlx::query("ROLLBACK TO SAVEPOINT enroll_dev_machine_create")
                    .execute(&mut **tx)
                    .await
                    .map_err(|_| DevMachineFail::Server)?;
            }
            Err(_) => return Err(DevMachineFail::Server),
        }
        let relocked = lock_dev_machine_by_id(tx, id).await.map_err(err)?;
        match relocked {
            Some(locked) if locked.owner_id == user_id => {
                touch_dev_machine(tx, &locked, &host_label)
                    .await
                    .map_err(err)?;
                return Ok(Some(DevMachineTouch {
                    id: locked.id,
                    provisioning: locked.provisioning,
                }));
            }
            _ => return Err(DevMachineFail::Ownership),
        }
    }

    if host_label.is_empty() {
        return Ok(None);
    }
    let row: Option<PgRow> = sqlx::query(&enroll_sql::dev_machine_by_owner_host_sql())
        .bind(&host_label)
        .bind(user_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|_| DevMachineFail::Server)?;
    if let Some(row) = row {
        let locked = decode_dev_machine(&row).map_err(err)?;
        touch_dev_machine(tx, &locked, &host_label)
            .await
            .map_err(err)?;
        return Ok(Some(DevMachineTouch {
            id: locked.id,
            provisioning: locked.provisioning,
        }));
    }
    sqlx::query("SAVEPOINT enroll_dev_machine_legacy")
        .execute(&mut **tx)
        .await
        .map_err(|_| DevMachineFail::Server)?;
    let fresh = Uuid::new_v4();
    match insert_dev_machine(tx, fresh, user_id, &host_label).await {
        Ok(()) => {
            sqlx::query("RELEASE SAVEPOINT enroll_dev_machine_legacy")
                .execute(&mut **tx)
                .await
                .map_err(|_| DevMachineFail::Server)?;
            return Ok(Some(DevMachineTouch {
                id: fresh,
                provisioning: enums::RUNNER_PROVISIONING_MANUAL.to_owned(),
            }));
        }
        Err(error) if is_integrity_error(&error) => {
            sqlx::query("ROLLBACK TO SAVEPOINT enroll_dev_machine_legacy")
                .execute(&mut **tx)
                .await
                .map_err(|_| DevMachineFail::Server)?;
        }
        Err(_) => return Err(DevMachineFail::Server),
    }
    // Legacy race path (`:119-127`): return whatever won, even `None`,
    // with NO touch.
    let row: Option<PgRow> = sqlx::query(&enroll_sql::dev_machine_by_owner_host_sql())
        .bind(&host_label)
        .bind(user_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|_| DevMachineFail::Server)?;
    match row {
        Some(row) => {
            let winner = decode_dev_machine(&row).map_err(err)?;
            Ok(Some(DevMachineTouch {
                id: winner.id,
                provisioning: winner.provisioning,
            }))
        }
        None => Ok(None),
    }
}

// ---------------------------------------------------------------------------
// B-path: `_maybe_mint_machine_token` (`:130-171`) + `_rotate_machine_token`
// ---------------------------------------------------------------------------

/// B2/B3 insert binds (`:159-168,193-202`): id, user, dev machine,
/// workspace, host, hash, fingerprint, label, is_service TRUE, created
/// now, last_used/revoked NULL.
async fn insert_machine_token(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    dev_machine_id: Option<Uuid>,
    workspace_id: Uuid,
    host_label: &str,
    minted: &enroll_tokens::MintedToken,
) -> Result<(), sqlx::Error> {
    sqlx::query(&enroll_sql::machine_token_insert_sql())
        .bind(Uuid::new_v4())
        .bind(user_id)
        .bind(dev_machine_id)
        .bind(workspace_id)
        .bind(host_label)
        .bind(&minted.hashed)
        .bind(&minted.fingerprint)
        .bind(enroll_sql::machine_token_label(host_label))
        .bind(true)
        .bind(Utc::now())
        .bind(None::<DateTime<Utc>>)
        .bind(None::<DateTime<Utc>>)
        .execute(&mut **tx)
        .await
        .map(|_| ())
}

/// `_maybe_mint_machine_token` (`:130-171`): locked-exists probe, mint,
/// savepoint insert (`IntegrityError` → `None`).
pub async fn maybe_mint_machine_token(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    workspace_id: Uuid,
    dev_machine_id: Option<Uuid>,
    host_label: &str,
    secret_key: &str,
) -> Result<Option<enroll_tokens::MintedToken>, Response> {
    let with_machine = dev_machine_id.is_some();
    let sql = enroll_sql::bootstrap_probe_sql(with_machine);
    let mut probe = sqlx::query(&sql);
    if with_machine {
        probe = probe.bind(dev_machine_id).bind(user_id).bind(workspace_id);
    } else {
        probe = probe.bind(host_label).bind(user_id).bind(workspace_id);
    }
    let locked: Option<PgRow> = probe
        .fetch_optional(&mut **tx)
        .await
        .map_err(|_| server_error())?;
    if locked.is_some() {
        return Ok(None);
    }
    let minted = enroll_tokens::mint_machine_token(secret_key);
    sqlx::query("SAVEPOINT enroll_machine_token_mint")
        .execute(&mut **tx)
        .await
        .map_err(|_| server_error())?;
    match insert_machine_token(
        tx,
        user_id,
        dev_machine_id,
        workspace_id,
        host_label,
        &minted,
    )
    .await
    {
        Ok(()) => {
            sqlx::query("RELEASE SAVEPOINT enroll_machine_token_mint")
                .execute(&mut **tx)
                .await
                .map_err(|_| server_error())?;
            Ok(Some(minted))
        }
        Err(error) if is_integrity_error(&error) => {
            sqlx::query("ROLLBACK TO SAVEPOINT enroll_machine_token_mint")
                .execute(&mut **tx)
                .await
                .map_err(|_| server_error())?;
            Ok(None)
        }
        Err(_) => Err(server_error()),
    }
}

/// What `_rotate_machine_token` returned: the minted token, a unique
/// collision (the caller folds it into the create name-collision path),
/// or a 500.
pub enum RotateOutcome {
    Collision,
    Server,
}

/// `_rotate_machine_token` (`:174-203`): revoke-then-insert in the
/// caller tx (NOT savepoint-guarded). The machine branch drops `user`
/// from the filter — ported as-is (BUG-rotate-user-omission).
pub async fn rotate_machine_token(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    workspace_id: Uuid,
    dev_machine_id: Option<Uuid>,
    host_label: &str,
    secret_key: &str,
) -> Result<enroll_tokens::MintedToken, RotateOutcome> {
    let with_machine = dev_machine_id.is_some();
    let sql = enroll_sql::rotate_revoke_sql(with_machine);
    let mut revoke = sqlx::query(&sql).bind(Utc::now());
    if with_machine {
        revoke = revoke.bind(dev_machine_id).bind(workspace_id);
    } else {
        revoke = revoke.bind(host_label).bind(user_id).bind(workspace_id);
    }
    revoke
        .execute(&mut **tx)
        .await
        .map_err(|_| RotateOutcome::Server)?;
    let minted = enroll_tokens::mint_machine_token(secret_key);
    match insert_machine_token(
        tx,
        user_id,
        dev_machine_id,
        workspace_id,
        host_label,
        &minted,
    )
    .await
    {
        Ok(()) => Ok(minted),
        Err(error) if is_integrity_error(&error) => Err(RotateOutcome::Collision),
        Err(_) => Err(RotateOutcome::Server),
    }
}

// ---------------------------------------------------------------------------
// E6/E7: create-endpoint reads + insert
// ---------------------------------------------------------------------------

/// E6a (`:629-637`): explicit workspace slug.
pub async fn workspace_by_slug(
    pool: &sqlx::PgPool,
    slug: &str,
) -> Result<Option<WorkspaceHit>, Response> {
    let row: Option<PgRow> = sqlx::query(&enroll_sql::workspace_by_slug_sql())
        .bind(slug)
        .fetch_optional(pool)
        .await
        .map_err(|_| server_error())?;
    row.map(|row| decode_workspace_at(&row, 0)).transpose()
}

/// E6b (`:639-643`): the caller's active memberships, oldest first,
/// probing 2 rows.
pub async fn memberships_for_inference(
    pool: &sqlx::PgPool,
    user_id: Uuid,
) -> Result<Vec<WorkspaceHit>, Response> {
    let rows: Vec<PgRow> = sqlx::query(&enroll_sql::memberships_for_inference_sql())
        .bind(user_id)
        .fetch_all(pool)
        .await
        .map_err(|_| server_error())?;
    let base = enroll_sql::WORKSPACE_MEMBERS_SELECT_COLUMNS.len();
    rows.iter()
        .map(|row| decode_workspace_at(row, base))
        .collect::<Result<Vec<_>, _>>()
}

/// E6c (`:664-669`): project by workspace + identifier.
pub async fn project_by_workspace_identifier(
    pool: &sqlx::PgPool,
    workspace_id: Uuid,
    identifier: &str,
) -> Result<Option<ProjectHit>, Response> {
    let row: Option<PgRow> = sqlx::query(&enroll_sql::project_by_workspace_identifier_sql())
        .bind(identifier)
        .bind(workspace_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| server_error())?;
    row.map(|row| decode_project(&row)).transpose()
}

/// E6d-explicit (`:672-673`): pod by name (miss falls through to the
/// default leg — QUIRK-pod-fallthrough).
pub async fn pod_by_name(
    pool: &sqlx::PgPool,
    project_id: Uuid,
    name: &str,
) -> Result<Option<PodHit>, Response> {
    let row: Option<PgRow> = sqlx::query(&enroll_sql::pod_by_name_sql())
        .bind(name)
        .bind(project_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| server_error())?;
    row.map(|row| decode_pod(&row)).transpose()
}

/// E6d-default (`Pod.default_for_project_id`, `models.py:173-176`).
pub async fn default_pod_for_project(
    pool: &sqlx::PgPool,
    project_id: Uuid,
) -> Result<Option<PodHit>, Response> {
    let row: Option<PgRow> =
        sqlx::query(pidash_db::runner_enroll::columns::pod::DEFAULT_FOR_PROJECT_ID_SQL)
            .bind(project_id)
            .fetch_optional(pool)
            .await
            .map_err(|_| server_error())?;
    row.map(|row| decode_pod(&row)).transpose()
}

/// `_next_auto_runner_name` count (`:790-796`): ALL runners in the pod,
/// revoked included. Django `.count()` rendering.
pub async fn count_pod_runners(pool: &sqlx::PgPool, pod_id: Uuid) -> Result<i64, Response> {
    sqlx::query_scalar::<_, i64>(
        r#"SELECT COUNT(*) AS "__count" FROM "runner" WHERE "runner"."pod_id" = $1"#,
    )
    .bind(pod_id)
    .fetch_one(pool)
    .await
    .map_err(|_| server_error())
}

/// E7 reuse probe (`:709-720`): a live desktop-bundled runner for this
/// owner/workspace/machine/pod.
pub async fn desktop_reuse_probe(
    tx: &mut Transaction<'_, Postgres>,
    dev_machine_id: Uuid,
    owner_id: Uuid,
    pod_id: Uuid,
    workspace_id: Uuid,
) -> Result<Option<ReuseHit>, Response> {
    let row: Option<PgRow> = sqlx::query(&enroll_sql::desktop_reuse_probe_sql())
        .bind(dev_machine_id)
        .bind(owner_id)
        .bind(pod_id)
        .bind(enums::RUNNER_PROVISIONING_DESKTOP_BUNDLED)
        .bind(workspace_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|_| server_error())?;
    row.map(|row| decode_reuse(&row)).transpose()
}

/// E7 cap count (`_managed_cap_error`, `:237-246`): non-revoked bundled
/// runners for this owner/pod/workspace.
pub async fn managed_cap_count(
    tx: &mut Transaction<'_, Postgres>,
    owner_id: Uuid,
    pod_id: Uuid,
    workspace_id: Uuid,
) -> Result<i64, Response> {
    sqlx::query_scalar::<_, i64>(&enroll_sql::managed_cap_count_sql())
        .bind(owner_id)
        .bind(pod_id)
        .bind(enums::RUNNER_PROVISIONING_DESKTOP_BUNDLED)
        .bind(workspace_id)
        .bind(enums::RUNNER_STATUS_REVOKED)
        .fetch_one(&mut **tx)
        .await
        .map_err(|_| server_error())
}

/// What the E7 runner insert returned: the new row id, a unique
/// collision (explicit name → 409, auto name → retry), or a 500.
pub enum RunnerInsertOutcome {
    Collision,
    Server,
}

/// E7 runner insert (`:726-735`): the 30-column `INSERT` in
/// `COLUMNS` order (`Runner.save()`'s auto-resolve is a no-op — `pod`
/// is always set).
#[allow(clippy::too_many_arguments)]
pub async fn insert_runner(
    tx: &mut Transaction<'_, Postgres>,
    owner_id: Uuid,
    workspace_id: Uuid,
    dev_machine_id: Option<Uuid>,
    pod_id: Uuid,
    name: &str,
    host_label: &str,
    provisioning: &str,
) -> Result<Uuid, RunnerInsertOutcome> {
    let id = Uuid::new_v4();
    let now = Utc::now();
    let inserted = sqlx::query(&enroll_sql::runner_insert_sql())
        .bind(id)
        .bind(owner_id)
        .bind(workspace_id)
        .bind(dev_machine_id)
        .bind(pod_id)
        .bind(name)
        .bind(host_label)
        .bind(provisioning)
        .bind(0i16)
        .bind("")
        .bind("")
        .bind(0i32)
        .bind("")
        .bind(1i32)
        .bind("")
        .bind("")
        .bind(now)
        .bind(serde_json::json!([]))
        .bind(enums::RUNNER_STATUS_OFFLINE)
        .bind("")
        .bind("")
        .bind("")
        .bind(serde_json::json!({}))
        .bind(1i32)
        .bind(None::<DateTime<Utc>>)
        .bind(None::<i32>)
        .bind(now)
        .bind(now)
        .bind(None::<DateTime<Utc>>)
        .bind("")
        .execute(&mut **tx)
        .await;
    match inserted {
        Ok(_) => Ok(id),
        Err(error) if is_integrity_error(&error) => Err(RunnerInsertOutcome::Collision),
        Err(_) => Err(RunnerInsertOutcome::Server),
    }
}

/// Silence dead-column warnings: every `COLUMNS` order this module
/// decodes positionally is referenced, so a schema drift breaks the
/// build instead of shifting indices silently.
#[allow(dead_code)]
fn column_orders_pin() {
    let _ = (
        r_cols::COLUMNS.len(),
        dm_cols::COLUMNS.len(),
        mt_cols::COLUMNS.len(),
        pod_cols::COLUMNS.len(),
        enroll_sql::WORKSPACES_SELECT_COLUMNS.len(),
        enroll_sql::WORKSPACE_MEMBERS_SELECT_COLUMNS.len(),
    );
}

// ---------------------------------------------------------------------------
// Response bodies (F7; key order is the wire order)
// ---------------------------------------------------------------------------

/// `GET /api/v1/runner/health/` 200 (`register.py:28`).
pub const HEALTH_BODY: &str = r#"{"ok":true,"protocol_version":3}"#;

/// `POST /api/runners/invites/` 410 (`:217-224`).
pub const INVITE_GONE_BODY: &str = r#"{"error":"legacy_enrollment_disabled","error_description":"Use `pidash auth login` and `pidash runner add` to register runners."}"#;

/// `POST /api/runners/<rid>/revive/` 410 (`:491-500`).
pub const REVIVE_GONE_BODY: &str = r#"{"error":"legacy_enrollment_disabled","error_description":"Delete this runner and run `pidash runner add` from the authenticated target machine."}"#;

/// Enroll/create 201 shared envelope fields (`:361-374,771-784`).
pub struct EnrollmentBody<'a> {
    pub runner_id: &'a str,
    pub runner_name: &'a str,
    pub refresh_token: &'a str,
    pub access_token: &'a str,
    pub access_token_expires_at: String,
    pub refresh_token_generation: i32,
    pub workspace_slug: &'a str,
    pub pod_slug: &'a str,
    pub project_identifier: &'a str,
    pub machine_token: Option<&'a str>,
}

/// The enroll 201 (`:361-377`) and create 201 (`:771-787`): identical
/// key order; `machine_token` present ONLY when minted (appended last).
pub fn enrollment_body(fields: &EnrollmentBody<'_>) -> String {
    let mut body = Map::with_capacity(13);
    body.insert(
        "runner_id".to_owned(),
        Value::String(fields.runner_id.to_owned()),
    );
    body.insert(
        "runner_name".to_owned(),
        Value::String(fields.runner_name.to_owned()),
    );
    body.insert(
        "refresh_token".to_owned(),
        Value::String(fields.refresh_token.to_owned()),
    );
    body.insert(
        "access_token".to_owned(),
        Value::String(fields.access_token.to_owned()),
    );
    body.insert(
        "access_token_expires_at".to_owned(),
        Value::String(fields.access_token_expires_at.clone()),
    );
    body.insert(
        "refresh_token_generation".to_owned(),
        Value::Number(fields.refresh_token_generation.into()),
    );
    body.insert(
        "workspace_slug".to_owned(),
        Value::String(fields.workspace_slug.to_owned()),
    );
    body.insert(
        "pod_slug".to_owned(),
        Value::String(fields.pod_slug.to_owned()),
    );
    body.insert(
        "project_identifier".to_owned(),
        Value::String(fields.project_identifier.to_owned()),
    );
    body.insert(
        "long_poll_interval_secs".to_owned(),
        Value::Number(LONG_POLL_INTERVAL_SECS.into()),
    );
    body.insert(
        "protocol_version".to_owned(),
        Value::Number(ENROLL_PROTOCOL_VERSION.into()),
    );
    body.insert(
        "machine_token_minted".to_owned(),
        Value::Bool(fields.machine_token.is_some()),
    );
    if let Some(token) = fields.machine_token {
        body.insert("machine_token".to_owned(), Value::String(token.to_owned()));
    }
    Value::Object(body).to_string()
}

/// The ticket 201 (`:847-850`): exactly `ticket` + `expires_in_secs`.
pub fn ticket_body(ticket: &str) -> String {
    serde_json::json!({ "ticket": ticket, "expires_in_secs": TICKET_TTL_SECS }).to_string()
}

/// The redeem 201 (`:925-931`).
pub fn redeem_body(machine_token: &str, host_label: &str, workspace_slug: &str) -> String {
    let mut body = Map::with_capacity(3);
    body.insert(
        "machine_token".to_owned(),
        Value::String(machine_token.to_owned()),
    );
    body.insert(
        "host_label".to_owned(),
        Value::String(host_label.to_owned()),
    );
    body.insert(
        "workspace_slug".to_owned(),
        Value::String(workspace_slug.to_owned()),
    );
    Value::Object(body).to_string()
}

/// `int(time.time())` for JWT `iat`/`exp` (`tokens.py:153`).
fn unix_now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// `GET /api/v1/runner/health/`
// ---------------------------------------------------------------------------

/// `HealthEndpoint.get` (`register.py:23-28`): AllowAny + the inherited
/// anon throttle, no DB touch.
pub async fn health(State(state): State<AppState>, req: Request<axum::body::Body>) -> Response {
    let remote_addr = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|info| info.0);
    if let Err(denied) = check_anon_throttle(&state, req.headers(), remote_addr).await {
        return denied;
    }
    json_response(StatusCode::OK, HEALTH_BODY.to_owned())
}

// ---------------------------------------------------------------------------
// `POST /api/v1/runner/runners/enroll/`
// ---------------------------------------------------------------------------

/// `RunnerEnrollEndpoint.post` (`:274-377`): AllowAny + anon throttle,
/// serializer gate, one-time-token redeem tx, refresh+access mint,
/// dev-machine attach, optional rename, machine-token bootstrap.
pub async fn enroll_runner(
    State(state): State<AppState>,
    req: Request<axum::body::Body>,
) -> Response {
    let remote_addr = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|info| info.0);
    if let Err(denied) = check_anon_throttle(&state, req.headers(), remote_addr).await {
        return denied;
    }
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(response) => return response,
    };
    let secret = state.settings().secret_key.clone();
    let data = match read_request_data(&state, req).await {
        Ok(data) => data,
        Err(response) => return response,
    };
    let validated = match enroll_shapes::validate_enroll_request(&data) {
        Ok(validated) => validated,
        Err(errors) => return json_response(StatusCode::BAD_REQUEST, errors.to_string()),
    };

    let token_hash = pidash_auth::token::hash_token(&validated.enrollment_token, secret.as_bytes());
    let dev_machine_id = match validated.dev_machine_id {
        Some(ref raw) => match raw.parse::<Uuid>() {
            Ok(id) => Some(id),
            Err(_) => return server_error(),
        },
        None => None,
    };
    // The request label is sliced WITHOUT stripping
    // (QUIRK-enroll-host-not-stripped); the D-path strips its own copy.
    let host_label = enroll_sql::slice_host_label(&validated.host_label);
    let body_name = enroll_sql::normalize_body_name(&validated.name);

    let mut tx = match pool.begin().await {
        Ok(tx) => tx,
        Err(_) => return server_error(),
    };
    let Some(runner) = (match fetch_enroll_runner(&mut tx, &token_hash).await {
        Ok(row) => row,
        Err(response) => return response,
    }) else {
        return json_response(
            StatusCode::UNAUTHORIZED,
            error_body("invalid_or_expired_enrollment_token"),
        );
    };
    if runner.revoked_at.is_some() {
        return json_response(StatusCode::CONFLICT, error_body("runner_revoked"));
    }
    if runner.enrolled_at.is_some() {
        return json_response(
            StatusCode::CONFLICT,
            error_body("enrollment_token_already_used"),
        );
    }

    let refresh = enroll_tokens::mint_refresh_token(&secret);
    let runner_id_str = runner.runner_id.to_string();
    let owner_id_str = runner.owner_id.to_string();
    let workspace_id_str = runner.workspace_id.to_string();
    let access = match enroll_tokens::mint_access_token_with_keys(
        &enroll_tokens::MintParams {
            runner_id: &runner_id_str,
            user_id: &owner_id_str,
            workspace_id: &workspace_id_str,
            rtg: 1,
            ttl_secs: None,
            default_ttl_secs: state.settings().runner.access_token_ttl_secs,
            now_unix: unix_now_secs(),
        },
        &[],
        &secret,
    ) {
        Ok(access) => access,
        Err(_) => return server_error(),
    };
    let dev_machine = match get_or_create_dev_machine(
        &mut tx,
        runner.owner_id,
        dev_machine_id,
        &host_label,
    )
    .await
    {
        Ok(dev_machine) => dev_machine,
        Err(DevMachineFail::Ownership) => {
            return json_response(StatusCode::NOT_FOUND, error_body("dev_machine_not_found"));
        }
        Err(DevMachineFail::Server) => return server_error(),
    };
    let stored_host = runner.host_label.clone();
    let enrolled_host = enroll_sql::enroll_host_label(&host_label, &stored_host).to_owned();
    if let Err(response) = mark_runner_enrolled(
        &mut tx,
        runner.runner_id,
        dev_machine.as_ref().map(|dev| dev.id),
        &enrolled_host,
        &refresh.hashed,
        &refresh.fingerprint,
        &body_name,
        Utc::now(),
    )
    .await
    {
        return response;
    }
    let machine_minted = if host_label.is_empty() {
        None
    } else {
        match maybe_mint_machine_token(
            &mut tx,
            runner.owner_id,
            runner.workspace_id,
            dev_machine.as_ref().map(|dev| dev.id),
            &host_label,
            &secret,
        )
        .await
        {
            Ok(minted) => minted,
            Err(response) => return response,
        }
    };
    if tx.commit().await.is_err() {
        return server_error();
    }

    let runner_name = if body_name.is_empty() {
        runner.name.clone()
    } else {
        body_name
    };
    json_response(
        StatusCode::CREATED,
        enrollment_body(&EnrollmentBody {
            runner_id: &runner_id_str,
            runner_name: &runner_name,
            refresh_token: &refresh.raw,
            access_token: &access.raw,
            access_token_expires_at: render_isoformat(access.expires_at),
            refresh_token_generation: 1,
            workspace_slug: &runner.workspace_slug,
            pod_slug: &runner.pod_name,
            project_identifier: &runner.project_identifier,
            machine_token: machine_minted.as_ref().map(|minted| minted.raw.as_str()),
        }),
    )
}

// ---------------------------------------------------------------------------
// `POST /api/v1/runner/runners/`
// ---------------------------------------------------------------------------

/// One create-attempt input (`:691-744`).
struct CreateAttempt<'a> {
    user_id: Uuid,
    workspace_id: Uuid,
    pod_id: Uuid,
    dev_machine_id: Option<Uuid>,
    host_label: &'a str,
    name: &'a str,
    secret_key: &'a str,
    has_machine_token: bool,
    auth_token: &'a str,
    managed_max: i64,
}

/// One create-attempt result: done (commit), a name collision (roll
/// back + retry/`runner_name_taken`), the managed cap (COMMIT +
/// 409 — the return-inside-`atomic()` quirk), an ownership 404, or a
/// 500.
enum Attempt {
    Done(AttemptDone),
    Collision,
    Cap,
    Ownership,
    Server,
}

/// A finished attempt: the row identity + an optional rotated token.
struct AttemptDone {
    runner_id: Uuid,
    runner_name: String,
    generation: i32,
    minted: Option<enroll_tokens::MintedToken>,
}

/// One attempt of the create retry loop (`:691-744`): dev-machine
/// attach, desktop-bundled recover-or-cap, runner insert, APIToken-path
/// rotate + `deactivate_api_token`.
async fn create_attempt(tx: &mut Transaction<'_, Postgres>, params: &CreateAttempt<'_>) -> Attempt {
    let dev_machine = match get_or_create_dev_machine(
        tx,
        params.user_id,
        params.dev_machine_id,
        params.host_label,
    )
    .await
    {
        Ok(dev_machine) => dev_machine,
        Err(DevMachineFail::Ownership) => return Attempt::Ownership,
        Err(DevMachineFail::Server) => return Attempt::Server,
    };
    let dev_machine_id = dev_machine.as_ref().map(|dev| dev.id);
    let provisioning = dev_machine
        .as_ref()
        .map(|dev| dev.provisioning.clone())
        .unwrap_or_else(|| enums::RUNNER_PROVISIONING_MANUAL.to_owned());
    if provisioning == enums::RUNNER_PROVISIONING_DESKTOP_BUNDLED {
        if let Some(machine_id) = dev_machine_id {
            match desktop_reuse_probe(
                tx,
                machine_id,
                params.user_id,
                params.pod_id,
                params.workspace_id,
            )
            .await
            {
                Ok(Some(hit)) => {
                    return Attempt::Done(AttemptDone {
                        runner_id: hit.id,
                        runner_name: hit.name,
                        generation: hit.refresh_token_generation,
                        minted: None,
                    });
                }
                Ok(None) => {}
                Err(_) => return Attempt::Server,
            }
            match managed_cap_count(tx, params.user_id, params.pod_id, params.workspace_id).await {
                Ok(count) => {
                    if count >= params.managed_max {
                        return Attempt::Cap;
                    }
                }
                Err(_) => return Attempt::Server,
            }
        }
    }
    let runner_id = match insert_runner(
        tx,
        params.user_id,
        params.workspace_id,
        dev_machine_id,
        params.pod_id,
        params.name,
        params.host_label,
        &provisioning,
    )
    .await
    {
        Ok(id) => id,
        Err(RunnerInsertOutcome::Collision) => return Attempt::Collision,
        Err(RunnerInsertOutcome::Server) => return Attempt::Server,
    };
    let mut minted = None;
    if !params.has_machine_token && !params.host_label.is_empty() {
        match rotate_machine_token(
            tx,
            params.user_id,
            params.workspace_id,
            dev_machine_id,
            params.host_label,
            params.secret_key,
        )
        .await
        {
            Ok(rotated) => minted = Some(rotated),
            Err(RotateOutcome::Collision) => return Attempt::Collision,
            Err(RotateOutcome::Server) => return Attempt::Server,
        }
        if deactivate_api_token(&mut **tx, Some(params.auth_token), true, Utc::now())
            .await
            .is_err()
        {
            return Attempt::Server;
        }
    }
    Attempt::Done(AttemptDone {
        runner_id,
        runner_name: params.name.to_owned(),
        generation: 0,
        minted,
    })
}

/// `RunnerCreateEndpoint.post` (`:571-787`): `X-Api-Key` auth, workspace
/// resolve (explicit / single-membership infer / 400s), project + pod
/// resolve, name validate-or-autoname with the 5-retry loop,
/// desktop-bundled recover-or-cap, APIToken-path rotate.
pub async fn create_runner(
    State(state): State<AppState>,
    req: Request<axum::body::Body>,
) -> Response {
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(response) => return response,
    };
    let secret = state.settings().secret_key.clone();
    let auth = match enroll_auth::authenticate_api_key(pool, secret.as_bytes(), req.headers()).await
    {
        Err(response) => return response,
        Ok(enroll_auth::ApiKeyOutcome::Missing) => {
            return not_authenticated();
        }
        Ok(enroll_auth::ApiKeyOutcome::Invalid) => {
            return auth_denied(enroll_auth::CODE_GIVEN_API_TOKEN_NOT_VALID, None);
        }
        Ok(enroll_auth::ApiKeyOutcome::Authenticated(auth)) => auth,
    };
    let user_id = auth.user_id;
    let data = match read_request_data(&state, req).await {
        Ok(data) => data,
        Err(response) => return response,
    };
    let mut workspace_slug = match frame_stripped(&data, "workspace_slug") {
        Ok(value) => value,
        Err(response) => return response,
    };
    let project_identifier = match frame_stripped(&data, "project") {
        Ok(value) => value,
        Err(response) => return response,
    };
    let dev_machine_id_raw = match frame_stripped(&data, "dev_machine_id") {
        Ok(value) => value,
        Err(response) => return response,
    };
    let mut host_label = match frame_text(&data, "host_label", 255) {
        Ok(value) => value,
        Err(response) => return response,
    };
    let body_name = match frame_text(&data, "name", 128) {
        Ok(value) => value,
        Err(response) => return response,
    };
    let pod_name = match frame_stripped(&data, "pod") {
        Ok(value) => value,
        Err(response) => return response,
    };

    if project_identifier.is_empty() {
        return json_response(StatusCode::BAD_REQUEST, error_body("project is required"));
    }
    let mut dev_machine_id: Option<Uuid> = None;
    if !dev_machine_id_raw.is_empty() {
        match dev_machine_id_raw.parse::<Uuid>() {
            Ok(id) => dev_machine_id = Some(id),
            Err(_) => {
                return json_response(
                    StatusCode::BAD_REQUEST,
                    error_body("invalid_dev_machine_id"),
                );
            }
        }
    }
    if let Some(token) = auth.machine_token.as_ref() {
        let Some(token_slug) = token.workspace_slug.as_deref() else {
            return server_error();
        };
        if !workspace_slug.is_empty() && workspace_slug != token_slug {
            return json_response(StatusCode::NOT_FOUND, error_body("workspace_not_found"));
        }
        workspace_slug = token_slug.to_owned();
        if let Some(bound) = token.dev_machine_id {
            if dev_machine_id.is_some_and(|id| id != bound) {
                return json_response(
                    StatusCode::FORBIDDEN,
                    error_body("dev_machine_token_mismatch"),
                );
            }
            dev_machine_id = Some(bound);
        }
        if host_label.is_empty() {
            host_label = token.host_label.clone();
        }
    }
    if !body_name.is_empty() && !enroll_shapes::runner_name_is_valid(&body_name) {
        return json_response(
            StatusCode::BAD_REQUEST,
            error_desc_body(
                "invalid_runner_name",
                enroll_shapes::RUNNER_NAME_VIEW_MESSAGE,
            ),
        );
    }

    let workspace = if !workspace_slug.is_empty() {
        let hit = match workspace_by_slug(pool, &workspace_slug).await {
            Ok(hit) => hit,
            Err(response) => return response,
        };
        let Some(hit) = hit else {
            return json_response(StatusCode::NOT_FOUND, error_body("workspace_not_found"));
        };
        match workspace_member_exists(pool, hit.id, user_id).await {
            Ok(true) => hit,
            Ok(false) => {
                return json_response(StatusCode::NOT_FOUND, error_body("workspace_not_found"));
            }
            Err(response) => return response,
        }
    } else {
        let memberships = match memberships_for_inference(pool, user_id).await {
            Ok(memberships) => memberships,
            Err(response) => return response,
        };
        if memberships.is_empty() {
            return json_response(
                StatusCode::BAD_REQUEST,
                error_desc_body(
                    "no_workspace_membership",
                    "Caller is not a member of any workspace.",
                ),
            );
        }
        if memberships.len() > 1 {
            return json_response(
                StatusCode::BAD_REQUEST,
                error_desc_body(
                    "workspace_slug_required",
                    "Caller belongs to multiple workspaces — pass workspace_slug to pick one.",
                ),
            );
        }
        memberships
            .into_iter()
            .next()
            .expect("exactly one membership")
    };

    let project =
        match project_by_workspace_identifier(pool, workspace.id, &project_identifier).await {
            Ok(Some(project)) => project,
            Ok(None) => {
                return json_response(StatusCode::NOT_FOUND, error_body("project_not_found"));
            }
            Err(response) => return response,
        };

    let mut pod = None;
    if !pod_name.is_empty() {
        match pod_by_name(pool, project.id, &pod_name).await {
            Ok(hit) => pod = hit,
            Err(response) => return response,
        }
    }
    if pod.is_none() {
        match default_pod_for_project(pool, project.id).await {
            Ok(hit) => pod = hit,
            Err(response) => return response,
        }
    }
    let Some(pod) = pod else {
        return json_response(
            StatusCode::CONFLICT,
            error_body("project_has_no_default_pod"),
        );
    };

    let mut attempts = 0u32;
    let mut finished: Option<AttemptDone> = None;
    while attempts < MAX_AUTO_NAME_RETRIES {
        attempts += 1;
        let name = if !body_name.is_empty() {
            body_name.clone()
        } else {
            match count_pod_runners(pool, pod.id).await {
                Ok(count) => next_auto_runner_name(count),
                Err(response) => return response,
            }
        };
        let mut tx = match pool.begin().await {
            Ok(tx) => tx,
            Err(_) => return server_error(),
        };
        let attempt = create_attempt(
            &mut tx,
            &CreateAttempt {
                user_id,
                workspace_id: workspace.id,
                pod_id: pod.id,
                dev_machine_id,
                host_label: &host_label,
                name: &name,
                secret_key: &secret,
                has_machine_token: auth.machine_token.is_some(),
                auth_token: &auth.auth_token,
                managed_max: state.settings().managed_runner.max_per_user_project,
            },
        )
        .await;
        match attempt {
            Attempt::Done(done) => {
                if tx.commit().await.is_err() {
                    return server_error();
                }
                finished = Some(done);
                break;
            }
            Attempt::Collision => {
                let _ = tx.rollback().await;
                if !body_name.is_empty() {
                    return json_response(StatusCode::CONFLICT, error_body("runner_name_taken"));
                }
            }
            Attempt::Cap => {
                // The 409 returns from inside the attempt `atomic()`
                // block: the dev-machine side effects COMMIT.
                if tx.commit().await.is_err() {
                    return server_error();
                }
                return json_response(
                    StatusCode::CONFLICT,
                    error_desc_body(
                        "managed_runner_limit",
                        "This project already has a Pi Dash Agent registered for you on a machine.",
                    ),
                );
            }
            Attempt::Ownership => {
                return json_response(StatusCode::NOT_FOUND, error_body("dev_machine_not_found"));
            }
            Attempt::Server => return server_error(),
        }
    }
    let Some(done) = finished else {
        tracing::warn!(
            attempts = MAX_AUTO_NAME_RETRIES,
            "RunnerCreateEndpoint: gave up after auto-name attempts"
        );
        return json_response(
            StatusCode::CONFLICT,
            error_body("could_not_allocate_runner_name"),
        );
    };

    let runner_id_str = done.runner_id.to_string();
    json_response(
        StatusCode::CREATED,
        enrollment_body(&EnrollmentBody {
            runner_id: &runner_id_str,
            runner_name: &done.runner_name,
            refresh_token: "",
            access_token: "",
            access_token_expires_at: render_isoformat(Utc::now()),
            refresh_token_generation: done.generation,
            workspace_slug: &workspace.slug,
            pod_slug: &pod.name,
            project_identifier: &project.identifier,
            machine_token: done.minted.as_ref().map(|minted| minted.raw.as_str()),
        }),
    )
}

// ---------------------------------------------------------------------------
// `POST /api/runners/machine-tokens/<ws>/tickets/`
// ---------------------------------------------------------------------------

/// Django's `<uuid:>` converter (`[0-9a-f]{8}-...`, lowercase-only):
/// the only segment form that reaches the view. `Uuid::parse_str`
/// alone also accepts uppercase/braced/simple forms, which Django
/// 404s before auth runs; comparing against the canonical lowercase
/// form reproduces the converter exactly (neither side checks
/// version bits).
fn strict_uuid(segment: &str) -> Option<Uuid> {
    match segment.parse::<Uuid>() {
        Ok(id) if id.hyphenated().to_string() == segment => Some(id),
        _ => None,
    }
}

/// `MachineTokenTicketEndpoint.post` (`:810-850`): workspace-uuid
/// gate, session auth, membership 404, host-label required, Redis
/// `SET EX 60`, 201 body. The `SET` is skipped when Redis is `None`
/// but the 201 still goes out (ported bug). A non-canonical segment
/// falls through to Django (which renders its own 404): the view's
/// defensive `invalid_workspace_id` 400 is unreachable in Django
/// (the converter 404s first), so Rust never emits it either.
pub async fn machine_token_ticket(
    State(state): State<AppState>,
    extension: Option<Extension<SessionHandle>>,
    Path(workspace_raw): Path<String>,
    req: Request<axum::body::Body>,
) -> Response {
    let Some(ws_uuid) = strict_uuid(&workspace_raw) else {
        return crate::edge::proxy(State(state.clone()), req).await;
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(response) => return response,
    };
    let secret = state.settings().secret_key.clone();
    let user_id = match session_user_id(pool, secret.as_bytes(), extension).await {
        Ok(Some(user_id)) => user_id,
        Ok(None) => return not_authenticated(),
        Err(response) => return response,
    };
    match workspace_member_exists(pool, ws_uuid, user_id).await {
        Ok(true) => {}
        Ok(false) => {
            return json_response(StatusCode::NOT_FOUND, error_body("workspace not found"));
        }
        Err(response) => return response,
    }
    let data = match read_request_data(&state, req).await {
        Ok(data) => data,
        Err(response) => return response,
    };
    let host_label = match frame_text(&data, "host_label", 255) {
        Ok(host_label) => host_label,
        Err(response) => return response,
    };
    if host_label.is_empty() {
        return json_response(
            StatusCode::BAD_REQUEST,
            error_body("host_label is required"),
        );
    }
    let ticket = Uuid::new_v4().simple().to_string();
    if let Some(redis) = state.redis() {
        let payload = ticket_payload_bytes(&user_id, &ws_uuid, &host_label);
        if redis
            .set_ex(&ticket_key(&ticket), &payload, TICKET_TTL_SECS)
            .await
            .is_err()
        {
            return server_error();
        }
    }
    json_response(StatusCode::CREATED, ticket_body(&ticket))
}

// ---------------------------------------------------------------------------
// `POST /api/v1/runner/machine-tokens/`
// ---------------------------------------------------------------------------

/// A valid-JSON non-object ticket blob (`:903-905`): Django indexes
/// `payload["user_id"]`, so a list/string/number/`null` raises
/// `TypeError` (500) — `Value::get` would wrongly 410. Runs before
/// the per-key lookups; object payloads are untouched.
fn ensure_object_payload(payload: &Value) -> Result<(), Response> {
    if payload.is_object() {
        Ok(())
    } else {
        Err(server_error())
    }
}

/// One ticket-payload UUID (`:903-910`): absent/`null` is the 410
/// (`KeyError`/`DoesNotExist`); a present non-string or an unparseable
/// string is the 500 (Django's `ValidationError`).
fn payload_uuid(payload: &Value, key: &str) -> Result<Uuid, Response> {
    let stale = || json_response(StatusCode::GONE, error_body("stale_ticket"));
    match payload.get(key) {
        None => Err(stale()),
        Some(value) if value.is_null() => Err(stale()),
        Some(Value::String(raw)) => match raw.parse::<Uuid>() {
            Ok(id) => Ok(id),
            Err(_) => Err(server_error()),
        },
        Some(_) => Err(server_error()),
    }
}

/// `MachineTokenRedeemEndpoint.post` (`:861-932`): AllowAny + anon
/// throttle, ticket required, Redis-`None` 503, atomic `GETDEL` +
/// get+delete fallback, payload/user/workspace errors
/// (400/401/410), `_maybe_mint` or 409, 201 body.
pub async fn machine_token_redeem(
    State(state): State<AppState>,
    req: Request<axum::body::Body>,
) -> Response {
    let remote_addr = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|info| info.0);
    if let Err(denied) = check_anon_throttle(&state, req.headers(), remote_addr).await {
        return denied;
    }
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(response) => return response,
    };
    let secret = state.settings().secret_key.clone();
    let data = match read_request_data(&state, req).await {
        Ok(data) => data,
        Err(response) => return response,
    };
    let ticket = match frame_stripped(&data, "ticket") {
        Ok(ticket) => ticket,
        Err(response) => return response,
    };
    if ticket.is_empty() {
        return json_response(StatusCode::BAD_REQUEST, error_body("ticket is required"));
    }
    let Some(client) = ticket_redis_client(&state) else {
        return json_response(
            StatusCode::SERVICE_UNAVAILABLE,
            error_body("redis_unavailable"),
        );
    };
    let blob = match redeem_ticket_blob(&client, &ticket_key(&ticket)).await {
        Ok(blob) => blob,
        Err(response) => return response,
    };
    let Some(blob) = blob.filter(|blob| !blob.is_empty()) else {
        return json_response(
            StatusCode::UNAUTHORIZED,
            error_body("invalid_or_expired_ticket"),
        );
    };
    let text = match String::from_utf8(blob) {
        Ok(text) => text,
        Err(_) => return server_error(),
    };
    let payload: Value = match serde_json::from_str(&text) {
        Ok(payload) => payload,
        Err(_) => {
            return json_response(
                StatusCode::BAD_REQUEST,
                error_body("invalid_ticket_payload"),
            );
        }
    };
    if let Err(response) = ensure_object_payload(&payload) {
        return response;
    }
    let user_id = match payload_uuid(&payload, "user_id") {
        Ok(user_id) => user_id,
        Err(response) => return response,
    };
    let workspace_id = match payload_uuid(&payload, "workspace_id") {
        Ok(workspace_id) => workspace_id,
        Err(response) => return response,
    };
    let user_exists: Option<Uuid> =
        match sqlx::query_scalar(r#"SELECT "id" FROM "users" WHERE "id" = $1"#)
            .bind(user_id)
            .fetch_optional(pool)
            .await
        {
            Ok(row) => row,
            Err(_) => return server_error(),
        };
    let workspace: Option<(Uuid, String)> = match sqlx::query_as(
        r#"SELECT "id", "slug" FROM "workspaces" WHERE "id" = $1 AND "deleted_at" IS NULL"#,
    )
    .bind(workspace_id)
    .fetch_optional(pool)
    .await
    {
        Ok(row) => row,
        Err(_) => return server_error(),
    };
    let (Some(_), Some((_, workspace_slug))) = (user_exists, workspace) else {
        return json_response(StatusCode::GONE, error_body("stale_ticket"));
    };
    // `(payload.get("host_label") or "")[:255]` — no strip. A truthy
    // non-string is the source's `TypeError` (500).
    let host_value = payload.get("host_label").unwrap_or(&Value::Null);
    let host_label = if !py_truthy(host_value) {
        String::new()
    } else {
        match host_value.as_str() {
            Some(label) => truncate_chars(label, 255),
            None => return server_error(),
        }
    };

    let mut tx = match pool.begin().await {
        Ok(tx) => tx,
        Err(_) => return server_error(),
    };
    let minted =
        match maybe_mint_machine_token(&mut tx, user_id, workspace_id, None, &host_label, &secret)
            .await
        {
            Ok(minted) => minted,
            Err(response) => return response,
        };
    if tx.commit().await.is_err() {
        return server_error();
    }
    let Some(minted) = minted else {
        return json_response(
            StatusCode::CONFLICT,
            error_body("machine_token_already_active"),
        );
    };
    json_response(
        StatusCode::CREATED,
        redeem_body(&minted.raw, &host_label, &workspace_slug),
    )
}

// ---------------------------------------------------------------------------
// 410s: `POST /api/runners/invites/`, `POST /api/runners/<rid>/revive/`
// ---------------------------------------------------------------------------

/// `RunnerInviteEndpoint.post` (`:217-224`): session-authed, always 410.
/// The view never touches the body, so no body is read.
pub async fn runner_invite(
    State(state): State<AppState>,
    extension: Option<Extension<SessionHandle>>,
) -> Response {
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(response) => return response,
    };
    let secret = state.settings().secret_key.clone();
    match session_user_id(pool, secret.as_bytes(), extension).await {
        Ok(Some(_)) => json_response(StatusCode::GONE, INVITE_GONE_BODY.to_owned()),
        Ok(None) => not_authenticated(),
        Err(response) => response,
    }
}

/// `RunnerReviveEndpoint.post` (`:491-500`): session-authed, always 410.
/// The view ignores `runner_id` entirely (never parsed), but the
/// `<uuid:runner_id>` converter 404s non-canonical segments before
/// auth runs — so the gate falls through to Django, which renders
/// its own 404.
pub async fn runner_revive(
    State(state): State<AppState>,
    extension: Option<Extension<SessionHandle>>,
    Path(runner_raw): Path<String>,
    req: Request<axum::body::Body>,
) -> Response {
    if strict_uuid(&runner_raw).is_none() {
        return crate::edge::proxy(State(state.clone()), req).await;
    }
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(response) => return response,
    };
    let secret = state.settings().secret_key.clone();
    match session_user_id(pool, secret.as_bytes(), extension).await {
        Ok(Some(_)) => json_response(StatusCode::GONE, REVIVE_GONE_BODY.to_owned()),
        Ok(None) => not_authenticated(),
        Err(response) => response,
    }
}

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------

/// An owned path: the listed methods serve from Rust, every other
/// method falls through to Django (the `runner_runs` precedent).
fn owned(
    handler: axum::routing::MethodRouter<AppState>,
    unowned: &[&str],
) -> axum::routing::MethodRouter<AppState> {
    let mut router = handler;
    for method in unowned {
        router = match *method {
            "POST" => router.post(crate::edge::proxy),
            "PUT" => router.put(crate::edge::proxy),
            "PATCH" => router.patch(crate::edge::proxy),
            "DELETE" => router.delete(crate::edge::proxy),
            "HEAD" => router.head(crate::edge::proxy),
            "OPTIONS" => router.options(crate::edge::proxy),
            _ => router.get(crate::edge::proxy),
        };
    }
    router
}

/// Register the daemon enrollment routes (`runner/urls.py`: `health/`,
/// `runners/enroll/`, `runners/`, `machine-tokens/`). Merged under
/// `RouteGroup::Runner` at the F-10 seam; sibling handler issues
/// extend the merge, keeping both sides.
pub fn daemon_routes() -> Router<AppState> {
    use axum::routing::{get, post};
    const POST_ONLY: &[&str] = &["GET", "PUT", "PATCH", "DELETE", "OPTIONS"];
    const GET_ONLY: &[&str] = &["POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];
    Router::new()
        .route("/api/v1/runner/health/", owned(get(health), GET_ONLY))
        .route(
            "/api/v1/runner/runners/enroll/",
            owned(post(enroll_runner), POST_ONLY),
        )
        .route(
            "/api/v1/runner/runners/",
            owned(post(create_runner), POST_ONLY),
        )
        .route(
            "/api/v1/runner/machine-tokens/",
            owned(post(machine_token_redeem), POST_ONLY),
        )
}

/// Register the web enrollment routes (`runner/web_urls.py`: `invites/`,
/// `machine-tokens/<ws>/tickets/`, `<rid>/revive/`). Merged under
/// `RouteGroup::RunnerWeb` at the F-10 seam; sibling handler issues
/// extend the merge, keeping both sides.
pub fn web_routes() -> Router<AppState> {
    use axum::routing::post;
    const POST_ONLY: &[&str] = &["GET", "PUT", "PATCH", "DELETE", "OPTIONS"];
    Router::new()
        .route(
            "/api/runners/invites/",
            owned(post(runner_invite), POST_ONLY),
        )
        .route(
            "/api/runners/machine-tokens/{workspace_id}/tickets/",
            owned(post(machine_token_ticket), POST_ONLY),
        )
        .route(
            "/api/runners/{runner_id}/revive/",
            owned(post(runner_revive), POST_ONLY),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use tower::ServiceExt;

    /// Read a response body into bytes (1MB cap).
    async fn body_bytes(response: Response) -> Vec<u8> {
        to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("body")
            .to_vec()
    }

    // -- F7: static bodies, byte for byte ------------------------------

    #[test]
    fn health_body_pins_f7() {
        assert_eq!(HEALTH_BODY, r#"{"ok":true,"protocol_version":3}"#);
        assert_eq!(HEALTH_PROTOCOL_VERSION, 3);
        assert_eq!(ENROLL_PROTOCOL_VERSION, 4);
        assert_eq!(LONG_POLL_INTERVAL_SECS, 25);
    }

    #[test]
    fn gone_bodies_pin_f7() {
        assert_eq!(
            INVITE_GONE_BODY,
            r#"{"error":"legacy_enrollment_disabled","error_description":"Use `pidash auth login` and `pidash runner add` to register runners."}"#
        );
        assert_eq!(
            REVIVE_GONE_BODY,
            r#"{"error":"legacy_enrollment_disabled","error_description":"Delete this runner and run `pidash runner add` from the authenticated target machine."}"#
        );
    }

    #[test]
    fn enrollment_body_key_order_pins_f7() {
        let without_token = enrollment_body(&EnrollmentBody {
            runner_id: "r",
            runner_name: "n",
            refresh_token: "rt_x",
            access_token: "jwt",
            access_token_expires_at: "2026-01-01T00:00:00+00:00".to_owned(),
            refresh_token_generation: 1,
            workspace_slug: "ws",
            pod_slug: "pod",
            project_identifier: "PRJ",
            machine_token: None,
        });
        assert_eq!(
            without_token,
            r#"{"runner_id":"r","runner_name":"n","refresh_token":"rt_x","access_token":"jwt","access_token_expires_at":"2026-01-01T00:00:00+00:00","refresh_token_generation":1,"workspace_slug":"ws","pod_slug":"pod","project_identifier":"PRJ","long_poll_interval_secs":25,"protocol_version":4,"machine_token_minted":false}"#
        );
        let with_token = enrollment_body(&EnrollmentBody {
            machine_token: Some("mt_x"),
            ..EnrollmentBody {
                runner_id: "r",
                runner_name: "n",
                refresh_token: "rt_x",
                access_token: "jwt",
                access_token_expires_at: "2026-01-01T00:00:00+00:00".to_owned(),
                refresh_token_generation: 1,
                workspace_slug: "ws",
                pod_slug: "pod",
                project_identifier: "PRJ",
                machine_token: None,
            }
        });
        assert!(with_token.ends_with(r#","machine_token":"mt_x"}"#));
        assert!(with_token.contains(r#""machine_token_minted":true"#));
    }

    #[test]
    fn ticket_and_redeem_bodies_pin_f7() {
        assert_eq!(
            ticket_body("abc"),
            r#"{"ticket":"abc","expires_in_secs":60}"#
        );
        assert_eq!(TICKET_TTL_SECS, 60);
        assert_eq!(ticket_key("abc"), "machine_token_ticket:abc");
        assert_eq!(
            redeem_body("mt_x", "h", "ws"),
            r#"{"machine_token":"mt_x","host_label":"h","workspace_slug":"ws"}"#
        );
    }

    #[test]
    fn error_bodies_pin_f7() {
        let cases = [
            "invalid_or_expired_enrollment_token",
            "runner_revoked",
            "enrollment_token_already_used",
            "dev_machine_not_found",
            "project is required",
            "invalid_dev_machine_id",
            "workspace_not_found",
            "dev_machine_token_mismatch",
            "project_not_found",
            "project_has_no_default_pod",
            "runner_name_taken",
            "could_not_allocate_runner_name",
            "managed_runner_limit",
            "invalid_workspace_id",
            "workspace not found",
            "host_label is required",
            "ticket is required",
            "redis_unavailable",
            "invalid_or_expired_ticket",
            "invalid_ticket_payload",
            "stale_ticket",
            "machine_token_already_active",
        ];
        for name in cases {
            assert_eq!(
                error_body(name),
                format!(r#"{{"error":"{name}"}}"#),
                "{name}"
            );
        }
        assert_eq!(
            error_desc_body(
                "no_workspace_membership",
                "Caller is not a member of any workspace."
            ),
            r#"{"error":"no_workspace_membership","error_description":"Caller is not a member of any workspace."}"#
        );
        assert_eq!(
            error_desc_body(
                "workspace_slug_required",
                "Caller belongs to multiple workspaces — pass workspace_slug to pick one."
            ),
            r#"{"error":"workspace_slug_required","error_description":"Caller belongs to multiple workspaces — pass workspace_slug to pick one."}"#
        );
        assert_eq!(
            error_desc_body(
                "managed_runner_limit",
                "This project already has a Pi Dash Agent registered for you on a machine."
            ),
            r#"{"error":"managed_runner_limit","error_description":"This project already has a Pi Dash Agent registered for you on a machine."}"#
        );
        assert_eq!(
            error_desc_body(
                "invalid_runner_name",
                enroll_shapes::RUNNER_NAME_VIEW_MESSAGE
            ),
            r#"{"error":"invalid_runner_name","error_description":"name must start with a letter, digit, or underscore and contain only letters, digits, underscore, dot, or dash"}"#
        );
    }

    // -- F6: auto-name / cap / charset helpers --------------------------

    #[test]
    fn auto_names_pin_f6() {
        assert_eq!(next_auto_runner_name(0), "runner_001");
        assert_eq!(next_auto_runner_name(1), "runner_002");
        assert_eq!(next_auto_runner_name(9), "runner_010");
        assert_eq!(next_auto_runner_name(999), "runner_1000");
        assert_eq!(MAX_AUTO_NAME_RETRIES, 5);
    }

    #[test]
    fn name_charset_pins_f6() {
        let cases = [
            ("a", true),
            ("_x", true),
            ("runner_001", true),
            ("9lives", true),
            (".hidden", false),
            ("-dash", false),
            ("has space", false),
            ("UPPER.ok-dash_under", true),
            (&"x".repeat(128), true),
            (&"x".repeat(129), false),
            ("", false),
            ("semi;colon", false),
            ("dot.", true),
        ];
        for (name, valid) in cases {
            assert_eq!(enroll_shapes::runner_name_is_valid(name), valid, "{name:?}");
        }
    }

    // -- F2: serializer gate rendering ----------------------------------

    #[test]
    fn enroll_empty_body_errors_pin_f2() {
        let Err(errors) = enroll_shapes::validate_enroll_request(&serde_json::json!({})) else {
            panic!("empty body must fail");
        };
        assert_eq!(
            errors.to_string(),
            r#"{"enrollment_token":["This field is required."],"host_label":["This field is required."]}"#
        );
    }

    // -- F3: mint wiring -------------------------------------------------

    #[test]
    fn refresh_mint_pins_f3_prefix() {
        let minted = enroll_tokens::mint_refresh_token("d13-fixture-secret-key");
        assert!(minted.raw.starts_with("rt_"));
        assert_eq!(minted.raw.len(), 3 + 43);
        assert_eq!(minted.hashed.len(), 64);
        assert_eq!(minted.fingerprint.len(), 12);
        let machine = enroll_tokens::mint_machine_token("d13-fixture-secret-key");
        assert!(machine.raw.starts_with("mt_"));
    }

    #[test]
    fn access_mint_pins_f3_claim_order() {
        use base64::Engine as _;
        let access = enroll_tokens::mint_access_token_with_keys(
            &enroll_tokens::MintParams {
                runner_id: "11111111-1111-1111-1111-111111111111",
                user_id: "22222222-2222-2222-2222-222222222222",
                workspace_id: "33333333-3333-3333-3333-333333333333",
                rtg: 1,
                ttl_secs: None,
                default_ttl_secs: 3600,
                now_unix: 1_700_000_000,
            },
            &[],
            "d13-fixture-secret-key",
        )
        .expect("derived key always active");
        assert_eq!(access.kid, "default");
        let payload_b64 = access.raw.split('.').nth(1).expect("jwt payload");
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(payload_b64)
            .expect("payload decodes");
        assert_eq!(
            String::from_utf8(payload).expect("utf8"),
            r#"{"iss":"pi-dash-cloud","sub":"11111111-1111-1111-1111-111111111111","uid":"22222222-2222-2222-2222-222222222222","wid":"33333333-3333-3333-3333-333333333333","iat":1700000000,"exp":1700003600,"rtg":1}"#
        );
    }

    // -- Scalars: strip / frames / isoformat / ascii ---------------------

    #[test]
    fn py_strip_covers_python_whitespace() {
        assert_eq!(py_strip("  x  "), "x");
        let padded: String = [0x85, 0x1c, 0x78, 0x85]
            .into_iter()
            .map(|c| char::from_u32(c).expect("scalar"))
            .collect();
        assert_eq!(py_strip(&padded), "x");
        assert_eq!(py_strip(""), "");
    }

    #[test]
    fn frame_readers_mirror_request_data_get() {
        let body = serde_json::json!({"s": " x ", "n": 1, "z": 0, "t": true, "f": false, "l": [1]});
        assert_eq!(frame_raw(&body, "s").expect("str"), " x ");
        assert_eq!(frame_stripped(&body, "s").expect("str"), "x");
        assert_eq!(frame_text(&body, "s", 1).expect("str"), "x");
        assert_eq!(frame_raw(&body, "missing").expect("absent"), "");
        assert_eq!(frame_raw(&body, "z").expect("zero"), "");
        assert_eq!(frame_raw(&body, "f").expect("false"), "");
        for key in ["n", "t", "l"] {
            assert!(frame_raw(&body, key).is_err(), "{key} 500s");
        }
        for bad in [serde_json::json!([1]), serde_json::json!("s"), Value::Null] {
            assert!(frame_raw(&bad, "s").is_err(), "{bad} 500s");
        }
    }

    #[test]
    fn py_truthy_matches_python() {
        assert!(!py_truthy(&serde_json::json!(0)));
        assert!(!py_truthy(&serde_json::json!(0.0)));
        assert!(py_truthy(&serde_json::json!(0.5)));
        assert!(!py_truthy(&serde_json::json!("")));
        assert!(!py_truthy(&serde_json::json!([])));
        assert!(!py_truthy(&serde_json::json!({})));
        assert!(py_truthy(&serde_json::json!([0])));
    }

    #[test]
    fn isoformat_matches_django() {
        let whole = DateTime::from_timestamp(1_700_000_000, 0).expect("ts");
        assert_eq!(render_isoformat(whole), "2023-11-14T22:13:20+00:00");
        let micros = DateTime::from_timestamp(1_700_000_000, 123_456_000).expect("ts");
        assert_eq!(render_isoformat(micros), "2023-11-14T22:13:20.123456+00:00");
        // Millis-exact micros keep six digits (the PIDASHCONV-716 trap).
        let millis = DateTime::from_timestamp(1_700_000_000, 123_000_000).expect("ts");
        assert_eq!(render_isoformat(millis), "2023-11-14T22:13:20.123000+00:00");
    }

    #[test]
    fn ticket_payload_matches_python_dumps() {
        let user: Uuid = "22222222-2222-2222-2222-222222222222"
            .parse()
            .expect("uuid");
        let ws: Uuid = "33333333-3333-3333-3333-333333333333"
            .parse()
            .expect("uuid");
        assert_eq!(
            ticket_payload_bytes(&user, &ws, "mac-mini"),
            r#"{"user_id": "22222222-2222-2222-2222-222222222222", "workspace_id": "33333333-3333-3333-3333-333333333333", "host_label": "mac-mini"}"#
        );
        // Escaping vector verified against CPython `json.dumps`:
        // short `\"`/`\\`/`\n`, lowercase `\uXXXX`, DEL escaped, surrogate
        // pair. The payload must both parse back and carry the Python
        // byte escapes.
        let tricky = ticket_payload_bytes(&user, &ws, "q\"\\\u{e9}\u{7f}\u{1f600}\n");
        let parsed: Value = serde_json::from_str(&tricky).expect("valid json");
        assert_eq!(
            parsed["host_label"],
            Value::String("q\"\\\u{e9}\u{7f}\u{1f600}\n".to_owned())
        );
        assert!(tricky.contains("\\u00e9"));
        assert!(tricky.contains("\\u007f"));
        assert!(tricky.contains("\\ud83d\\ude00"));
        assert!(tricky.ends_with("\\n\"}"));
    }

    // -- Denial bodies (lowercase `detail`, live-Django verified) -------

    #[tokio::test]
    async fn denial_bodies_use_lowercase_detail() {
        let denied = not_authenticated();
        assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);
        assert!(denied.headers().get(header::WWW_AUTHENTICATE).is_none());
        assert_eq!(
            body_bytes(denied).await,
            br#"{"detail":"Authentication credentials were not provided."}"#.as_slice()
        );
        let invalid = auth_denied(enroll_auth::CODE_GIVEN_API_TOKEN_NOT_VALID, None);
        assert_eq!(invalid.status(), StatusCode::FORBIDDEN);
        assert!(invalid.headers().get(header::WWW_AUTHENTICATE).is_none());
        assert_eq!(
            body_bytes(invalid).await,
            br#"{"detail":"Given API token is not valid"}"#.as_slice()
        );
        let challenged = auth_denied("session_expired", Some("Bearer"));
        assert_eq!(challenged.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(challenged.headers()[header::WWW_AUTHENTICATE], "Bearer");
        assert_eq!(
            body_bytes(challenged).await,
            br#"{"detail":"session_expired"}"#.as_slice()
        );
    }

    // -- Ticket payload UUIDs --------------------------------------------

    #[tokio::test]
    async fn payload_uuid_branches() {
        let id = payload_uuid(
            &serde_json::json!({"user_id": "22222222-2222-2222-2222-222222222222"}),
            "user_id",
        )
        .expect("uuid");
        assert_eq!(id.to_string(), "22222222-2222-2222-2222-222222222222");
        for missing in [serde_json::json!({}), serde_json::json!({"user_id": null})] {
            let err = payload_uuid(&missing, "user_id").expect_err("410");
            assert_eq!(err.status(), StatusCode::GONE);
            assert_eq!(
                body_bytes(err).await,
                br#"{"error":"stale_ticket"}"#.as_slice()
            );
        }
        for bad in [
            serde_json::json!({"user_id": "nope"}),
            serde_json::json!({"user_id": 7}),
        ] {
            let err = payload_uuid(&bad, "user_id").expect_err("500");
            assert_eq!(err.status(), StatusCode::INTERNAL_SERVER_ERROR);
        }
    }

    // -- Web-route UUID gating + non-object ticket blob (PIDASHCONV-791) --

    #[test]
    fn strict_uuid_pins_django_converter() {
        let lower = "12345678-1234-abcd-ef01-234567890abc";
        assert_eq!(
            strict_uuid(lower)
                .expect("lowercase")
                .hyphenated()
                .to_string(),
            lower
        );
        // Django's `<uuid:>` converter 404s every one of these
        // (several of which `Uuid::parse_str` would accept), so the
        // gate rejects them.
        for rejected in [
            lower.to_uppercase(),
            "not-a-uuid".to_owned(),
            "123".to_owned(),
            format!("{{{lower}}}"),
            lower.replace('-', ""),
            format!("urn:uuid:{lower}"),
            format!("{lower}/"),
        ] {
            assert!(strict_uuid(&rejected).is_none(), "{rejected}");
        }
    }

    #[test]
    fn non_object_ticket_payload_is_500() {
        let object: Value = serde_json::from_str(r#"{"user_id":"x"}"#).expect("object");
        assert!(ensure_object_payload(&object).is_ok());
        for raw in ["[1,2]", "\"just-a-string\"", "42", "true", "null"] {
            let payload: Value = serde_json::from_str(raw).expect("valid json");
            let err = ensure_object_payload(&payload).expect_err("500");
            assert_eq!(err.status(), StatusCode::INTERNAL_SERVER_ERROR, "{raw}");
        }
    }

    // -- Positional decode pins ------------------------------------------

    #[test]
    fn decode_indices_match_column_orders() {
        for (idx, name) in [
            (0, "id"),
            (1, "owner_id"),
            (2, "workspace_id"),
            (4, "pod_id"),
            (5, "name"),
            (6, "host_label"),
            (11, "refresh_token_generation"),
            (16, "enrolled_at"),
            (28, "revoked_at"),
        ] {
            assert_eq!(r_cols::COLUMNS[idx], name);
        }
        for (idx, name) in [
            (0, "id"),
            (1, "owner_id"),
            (2, "host_label"),
            (3, "label"),
            (5, "provisioning"),
        ] {
            assert_eq!(dm_cols::COLUMNS[idx], name);
        }
        for (idx, name) in [(0, "id"), (2, "project_id"), (3, "name")] {
            assert_eq!(pod_cols::COLUMNS[idx], name);
        }
        assert_eq!(col_index(enroll_sql::WORKSPACES_SELECT_COLUMNS, "id"), 5);
        assert_eq!(col_index(enroll_sql::WORKSPACES_SELECT_COLUMNS, "slug"), 10);
        let project_cols = pidash_db::app_project::models::project::COLUMNS;
        assert_eq!(col_index(project_cols, "id"), 5);
        assert_eq!(col_index(project_cols, "identifier"), 12);
        assert_eq!(mt_cols::COLUMNS.len(), 12);
    }

    // -- Routes ----------------------------------------------------------

    fn test_app() -> axum::Router {
        crate::routes::build_router(AppState::with_edge(
            "0.1.0",
            crate::edge::EdgeHandle::for_tests("http://127.0.0.1:1"),
        ))
    }

    #[tokio::test]
    async fn health_serves_without_credentials() {
        let response = test_app()
            .oneshot(
                Request::get("/api/v1/runner/health/")
                    .body(axum::body::Body::empty())
                    .expect("request"),
            )
            .await
            .expect("serve");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(body_bytes(response).await, HEALTH_BODY.as_bytes());
    }

    #[tokio::test]
    async fn unowned_methods_proxy() {
        // GET on the enroll path is not Rust-owned: it proxies (502
        // fail-closed here), never a Rust 405 masking Django's own.
        let response = test_app()
            .oneshot(
                Request::get("/api/v1/runner/runners/enroll/")
                    .body(axum::body::Body::empty())
                    .expect("request"),
            )
            .await
            .expect("serve");
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    }

    #[tokio::test]
    async fn enroll_rejects_empty_body_before_pool() {
        // The serializer gate runs past the pool check: pool-less states
        // 500 before validating (unreachable in `serve`).
        let response = test_app()
            .oneshot(
                Request::post("/api/v1/runner/runners/enroll/")
                    .header("content-type", "application/json")
                    .header("content-length", "2")
                    .body(axum::body::Body::from("{}"))
                    .expect("request"),
            )
            .await
            .expect("serve");
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }
}
