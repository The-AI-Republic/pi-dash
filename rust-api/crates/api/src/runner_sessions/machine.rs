//! Machine session lifecycle + long-poll endpoints (D-14, stage 5).
//!
//! Port of `apps/api/pi_dash/runner/views/machine_sessions.py:54-352`
//! (PIDASHCONV-559): the machine-scoped twin of the per-runner
//! session, keyed on the `DevMachine` and authenticated by the shared
//! `mt_` token — the channel exists even with zero runners enrolled.
//!
//! * [`machine_session_open`][]: `POST dev-machines/<mid>/sessions/`
//!   (`:69-131`) — 403 mismatch, pre-tx `ensure_stream_group`, the
//!   revoke-prior + insert + machine-touch transaction, post-tx marker
//!   clear + eviction publish + offline drain, 201 welcome body.
//! * [`machine_session_delete`][]: `DELETE
//!   dev-machines/<mid>/sessions/<sid>/` (`:134-159`) — 403 mismatch,
//!   missing row still clears the marker, revoke `clean_shutdown`,
//!   204.
//! * [`machine_session_poll`][]: `POST
//!   dev-machines/<mid>/sessions/<sid>/poll` (`:162-345`) — a plain
//!   async view (not DRF): raw-body JSON parse, bookkeeping (409s,
//!   touches, XACK, plan), the sliced eviction-aware wait, PEL-marker
//!   discipline, 200 envelope.
//!
//! `permission_classes=[]`, `throttle_classes=[]` on both DRF endpoints
//! are preserved as-is: no permission or throttle checks run here.
//!
//! # Connection discipline
//!
//! The ~25s wait holds no worker thread (axum-native async) and no
//! pooled DB connection: bookkeeping finishes (and every checkout is
//! dropped) before the wait starts, exactly like the source's
//! `db_sync_to_async` scopes.
//!
//! # Auth rendering
//!
//! Both 401 flavors re-render the D-13 denial locally (the
//! PIDASHCONV-590 precedent): `auth.rs` renders the lowercase
//! `detail` DRF 401 (PIDASHCONV-718); the re-render pins the exact
//! compact/spaced flavor per endpoint. The open/delete
//! 401 renders lowercase-compact with the `Bearer` challenge (DRF's
//! `exception_handler`); the poll 401 renders lowercase-spaced with
//! no challenge (the hand-built `JsonResponse`); 500s pass through
//! either way.
//!
//! # Dropped ORM-lazy select
//!
//! The source fetches `token.dev_machine` (one `SELECT … LIMIT 21`)
//! after authenticating; only the id is ever used, and the FK forbids
//! a miss, so the handlers bind the token's `dev_machine_id` directly
//! (the drain-issue precedent for ORM-lazy artifacts).
//!
//! # Approximations (no contract input covers them)
//!
//! * A 500 body is the shared runner JSON; Django renders its HTML
//!   error page on these paths. Contract tests pin the 500 status
//!   only.
//! * Integer/float ack ids render via CPython `repr()` spelling
//!   (`XACK` arg bytes may differ for exotic magnitudes like `1e100`
//!   vs `1e+100`; the status never does — both sides `XACK` and 200).
//!   A `true` item and nested items 500 like the source's redis-py
//!   `DataError` (`redis==5.0.4` rejects `bool` explicitly); `false`
//!   items drop out via the `if sid` filter on both sides.
//! * A nested `NaN`/`Infinity` inside an otherwise-valid poll body
//!   400s; only a top-level one takes the source's ignore-as-non-dict
//!   path (`serde_json` has no lenient mode).

// Every handler returns a fully-rendered `Response` by design (the
// runner `run_endpoints` precedent, which carries the same allow).
#![allow(clippy::result_large_err)]

use std::time::{Duration, Instant};

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Utc};
use http_body_util::BodyExt as _;
use serde_json::{Map, Value};
use uuid::Uuid;

use pidash_db::runner_sessions::machine_outbox;
use pidash_db::runner_sessions::models::machine_session;
use pidash_db::runner_sessions::models::machine_session::machine_session_from_row;
use pidash_types::runner_sessions::keys::machine as machine_keys;
use pidash_types::runner_sessions::{
    dev_machine_mismatch_drf, dev_machine_mismatch_poll, json_parse_error_poll, machine_open_201,
    poll_200, session_evicted_poll, unauthorized_drf, unauthorized_poll, DecodedMessage,
    HttpResponse,
};

use crate::runner_enroll::auth::{
    authenticate_machine_token, MachineAuth, AUTHENTICATE_HEADER_BEARER,
};
use crate::runner_runs::{json_response, pool_of, server_error};
use crate::state::AppState;

/// Long-poll wait slice (`_POLL_SLICE_MS`, `machine_sessions.py:49`).
const POLL_SLICE_MS: u64 = 1000;
/// `XREADGROUP COUNT` on every poll read (`:230, :275-276`).
const POLL_READ_COUNT: i64 = 100;
/// `DevMachine.objects.filter(pk=…).update(last_seen_at=…)` (D-13 table;
/// no provider const exists, so the Django queryset shape is spelled
/// here — verified against the FX-RSES-02 machine SQL captures).
const DEV_MACHINE_TOUCH_SQL: &str =
    "UPDATE \"dev_machine\" SET \"last_seen_at\" = $1 WHERE \"dev_machine\".\"id\" = $2";
/// Django's `custom_404_view` bytes (`app/views/error_404.py`):
/// `JsonResponse({"error": "Page not found."})`. The `<uuid:>`
/// converter 404s non-UUID segments before any view runs; axum matches
/// the segment, so the handlers answer those bytes on a bad id.
const PAGE_NOT_FOUND_BODY: &str = r#"{"error": "Page not found."}"#;
/// Revoke reason stamped on the prior session at open (`:97`).
const REASON_EVICTED_BY_NEW_SESSION: &str = "evicted_by_new_session";
/// Revoke reason stamped at delete (`:156`).
const REASON_CLEAN_SHUTDOWN: &str = "clean_shutdown";

// ---------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------

/// Render a shape response: the builder's status plus its exact bytes.
fn render(response: HttpResponse) -> Response {
    let status = StatusCode::from_u16(response.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    json_response(status, response.body)
}

/// Resolver-404 for a non-UUID path id (see [`PAGE_NOT_FOUND_BODY`]).
fn page_not_found() -> Response {
    json_response(StatusCode::NOT_FOUND, PAGE_NOT_FOUND_BODY.to_owned())
}

/// Parse a `<uuid:>` path segment, or the resolver-404.
fn parse_uuid(raw: &str) -> Result<Uuid, Response> {
    raw.parse().map_err(|_| page_not_found())
}

/// `timezone.now().isoformat()`: `+00:00` suffix, microseconds iff
/// nonzero (the `handlers_git_repo` `AutoSi` precedent).
fn django_isoformat(now: DateTime<Utc>) -> String {
    now.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, false)
}

/// Api-crate-owned Redis client (the `LivePorts` precedent):
/// `None` when `REDIS_URL` is unset, empty, or unparsable, mirroring
/// `redis_instance()` / `async_redis_instance()` returning `None`.
fn redis_client(state: &AppState) -> Option<redis::Client> {
    state
        .settings()
        .redis
        .url
        .as_deref()
        .filter(|url| !url.is_empty())
        .and_then(|url| redis::Client::open(url).ok())
}

/// `_auth_dev_machine` (`machine_sessions.py:54-67`) as data: the URL
/// machine id iff the presented token is bound to it, else `None`.
fn authed_machine_id(auth: Option<&MachineAuth>, url_id: Uuid) -> Option<Uuid> {
    match auth {
        Some(auth) if auth.token.dev_machine_id == Some(url_id) => Some(url_id),
        _ => None,
    }
}

/// Python truthiness for JSON values (`None`/`False`/`0`/`""`/`[]`/`{}`
/// are falsy; everything else is truthy) — the `or []` half of
/// `body.get("ack") or []` (`:197`).
fn py_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                int != 0
            } else if let Some(uint) = number.as_u64() {
                uint != 0
            } else {
                number.as_f64().is_some_and(|float| float != 0.0)
            }
        }
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
    }
}

/// One ack-list item to its `XACK` id: strings verbatim, numbers via
/// `repr()`; `false`/`null`/empty items drop out (the `if sid` filter
/// in `ack_for_session`); `true` and nested values fail like the
/// source's redis-py `DataError` (unhandled → 500 — `redis==5.0.4`
/// rejects `bool` explicitly, and the truthy `true` survives the
/// `if sid` filter to reach the encoder).
fn ack_item(value: &Value) -> Result<Option<String>, ()> {
    match value {
        Value::String(text) => {
            if text.is_empty() {
                Ok(None)
            } else {
                Ok(Some(text.clone()))
            }
        }
        Value::Number(number) => Ok(Some(number.to_string())),
        Value::Bool(false) | Value::Null => Ok(None),
        Value::Bool(true) | Value::Array(_) | Value::Object(_) => Err(()),
    }
}

/// `list(body.get("ack") or [])` (`machine_sessions.py:197`):
/// missing/falsy → empty; a list maps item-wise; a truthy string
/// splits into chars and a truthy dict into keys (`list(…)`); a truthy
/// number/bool is the source's `TypeError` (unhandled → 500).
fn ack_ids_from_body(body: &Map<String, Value>) -> Result<Vec<String>, ()> {
    let Some(raw) = body.get("ack") else {
        return Ok(Vec::new());
    };
    if !py_truthy(raw) {
        return Ok(Vec::new());
    }
    match raw {
        Value::Array(items) => {
            let mut ids = Vec::with_capacity(items.len());
            for item in items {
                if let Some(id) = ack_item(item)? {
                    ids.push(id);
                }
            }
            Ok(ids)
        }
        Value::String(text) => Ok(text.chars().map(|char| char.to_string()).collect()),
        Value::Object(map) => Ok(map.keys().cloned().collect()),
        _ => Err(()),
    }
}

/// Top-level `json.loads` extensions (`machine_sessions.py:320-327`):
/// CPython parses bare `NaN`/`Infinity`/`-Infinity` (surrounding
/// whitespace allowed) into floats — non-dicts, so the poll ignores
/// them — where `serde_json` errors. Nested occurrences stay a 400
/// (documented approximation).
fn is_top_level_json_extension(bytes: &[u8]) -> bool {
    let trimmed = bytes
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .map_or(&[][..], |start| {
            let end = bytes
                .iter()
                .rposition(|byte| !byte.is_ascii_whitespace())
                .map_or(start, |end| end + 1);
            &bytes[start..end]
        });
    matches!(trimmed, b"NaN" | b"Infinity" | b"-Infinity")
}

/// The poll body (`machine_sessions.py:316-327`): empty → `{}`;
/// unparsable → 400; well-formed non-dict → `{}` (silently ignored).
fn poll_body(bytes: &[u8]) -> Result<Map<String, Value>, Response> {
    if bytes.is_empty() {
        return Ok(Map::new());
    }
    match serde_json::from_slice::<Value>(bytes) {
        Ok(Value::Object(map)) => Ok(map),
        Ok(_) => Ok(Map::new()),
        Err(_) => {
            if is_top_level_json_extension(bytes) {
                Ok(Map::new())
            } else {
                Err(render(json_parse_error_poll()))
            }
        }
    }
}

/// Read a machine-generated D-13 denial body back into its code,
/// returning the code plus the rebuilt response. Accepts lowercase
/// `detail` (what `auth.rs` renders since PIDASHCONV-718) plus the
/// legacy pre-718 capital `Detail` for tolerance; anything else yields
/// `None` and the caller passes the rebuilt response through
/// untouched. The auth lookups are private to `runner_enroll::auth`
/// (read-only for this issue), so the code comes back out of the
/// denial body — both shapes are pinned by the unit tests below.
async fn denial_code(response: Response) -> (Option<String>, Response) {
    let (parts, body) = response.into_parts();
    let bytes = match body.collect().await {
        Ok(collected) => collected.to_bytes(),
        Err(_) => return (None, server_error()),
    };
    let code: Option<String> = serde_json::from_slice::<Value>(&bytes)
        .ok()
        .and_then(|value| {
            value
                .get("detail")
                .or_else(|| value.get("Detail"))
                .and_then(Value::as_str)
                .map(str::to_owned)
        });
    (
        code,
        Response::from_parts(parts, axum::body::Body::from(bytes)),
    )
}

/// Re-render a D-13 machine-token denial for the DRF open/delete
/// endpoints: the 401 keeps its code and `Bearer` challenge but
/// renders lowercase-compact (DRF's `exception_handler`), like the
/// PIDASHCONV-590 handlers do locally. 500s and unrecognized shapes
/// pass through untouched.
async fn open_denial(response: Response) -> Response {
    if response.status() != StatusCode::UNAUTHORIZED {
        return response;
    }
    let (code, response) = denial_code(response).await;
    match code {
        Some(code) => {
            let rendered = unauthorized_drf(&code);
            Response::builder()
                .status(StatusCode::UNAUTHORIZED)
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::WWW_AUTHENTICATE, AUTHENTICATE_HEADER_BEARER)
                .body(axum::body::Body::from(rendered.body))
                .unwrap_or_else(|_| server_error())
        }
        None => response,
    }
}

/// Re-render a D-13 machine-token denial for the plain-Django poll
/// view: the 401 keeps its code but renders spaced without the
/// `WWW-Authenticate` challenge (the hand-built `JsonResponse`,
/// `:304-307`); 500s and unrecognized shapes pass through untouched.
async fn poll_denial(response: Response) -> Response {
    if response.status() != StatusCode::UNAUTHORIZED {
        return response;
    }
    let (code, response) = denial_code(response).await;
    match code {
        Some(code) => render(unauthorized_poll(&code)),
        None => response,
    }
}

// ---------------------------------------------------------------------------
// Open
// ---------------------------------------------------------------------------

/// `POST dev-machines/<mid>/sessions/` — open a machine session
/// (`machine_sessions.py:69-131`).
///
/// 403 unless the `mt_` token is bound to the URL machine; the stream
/// group is ensured pre-tx; the transaction revokes the prior session
/// (`evicted_by_new_session`), inserts the row, and touches the
/// machine — with NO `_bound_txn_waits` guard (the deliberate
/// runner-open asymmetry: lock failures are plain 500s). Post-tx, the
/// old marker clears, the eviction publishes, and the offline stream
/// drains — all unguarded (a failure 500s after commit).
pub async fn machine_session_open(
    State(state): State<AppState>,
    Path(raw_mid): Path<String>,
    headers: HeaderMap,
) -> Response {
    let pool = match pool_of(&state) {
        Ok(pool) => pool.clone(),
        Err(response) => return response,
    };
    let dev_machine_id = match parse_uuid(&raw_mid) {
        Ok(id) => id,
        Err(response) => return response,
    };
    let secret = state.settings().secret_key.clone();
    let auth = match authenticate_machine_token(&pool, secret.as_bytes(), &headers).await {
        Ok(auth) => auth,
        Err(response) => return open_denial(response).await,
    };
    if authed_machine_id(auth.as_ref(), dev_machine_id).is_none() {
        return render(dev_machine_mismatch_drf());
    }

    let redis = redis_client(&state);
    let mid = dev_machine_id.to_string();
    if machine_outbox::ensure_stream_group(redis.as_ref(), &mid)
        .await
        .is_err()
    {
        return server_error();
    }

    let mut tx = match pool.begin().await {
        Ok(tx) => tx,
        Err(_) => return server_error(),
    };
    let prior = match sqlx::query(machine_session::OPEN_PRIOR_SQL)
        .bind(dev_machine_id)
        .fetch_optional(&mut *tx)
        .await
    {
        Ok(prior) => prior,
        Err(_) => return server_error(),
    };
    let mut old_session_id: Option<String> = None;
    if let Some(row) = prior {
        let prior = match machine_session_from_row(&row) {
            Ok(prior) => prior,
            Err(_) => return server_error(),
        };
        old_session_id = Some(prior.id.to_string());
        if sqlx::query(machine_session::REVOKE_SQL)
            .bind(Utc::now())
            .bind(REASON_EVICTED_BY_NEW_SESSION)
            .bind(prior.id)
            .execute(&mut *tx)
            .await
            .is_err()
        {
            return server_error();
        }
    }
    // `create(… last_seen_at=now())`: the kwarg `now()` evaluates
    // first, then `auto_now_add` stamps `created_at` — two calls, in
    // that order (captured micros differ).
    let new_sid = Uuid::new_v4();
    let last_seen_at = Utc::now();
    let created_at = Utc::now();
    if sqlx::query(machine_session::OPEN_INSERT_SQL)
        .bind(new_sid)
        .bind(dev_machine_id)
        .bind(machine_session::DEFAULT_PROTOCOL_VERSION)
        .bind(created_at)
        .bind(last_seen_at)
        .bind(None::<DateTime<Utc>>)
        .bind(machine_session::DEFAULT_REVOKED_REASON)
        .execute(&mut *tx)
        .await
        .is_err()
    {
        return server_error();
    }
    if sqlx::query(DEV_MACHINE_TOUCH_SQL)
        .bind(Utc::now())
        .bind(dev_machine_id)
        .execute(&mut *tx)
        .await
        .is_err()
    {
        return server_error();
    }
    if tx.commit().await.is_err() {
        return server_error();
    }

    if let Some(old) = old_session_id.as_deref() {
        if machine_outbox::clear_session_marker(redis.as_ref(), old)
            .await
            .is_err()
        {
            return server_error();
        }
    }
    let new_sid = new_sid.to_string();
    if machine_outbox::publish_session_eviction(
        redis.as_ref(),
        &mid,
        old_session_id.as_deref(),
        &new_sid,
    )
    .await
    .is_err()
    {
        return server_error();
    }
    if machine_outbox::drain_offline_into_live(redis.as_ref(), &mid)
        .await
        .is_err()
    {
        return server_error();
    }

    render(machine_open_201(
        &new_sid,
        &mid,
        &django_isoformat(Utc::now()),
        state.settings().runner.long_poll_interval_secs,
        i64::from(machine_session::DEFAULT_PROTOCOL_VERSION),
    ))
}

// ---------------------------------------------------------------------------
// Delete
// ---------------------------------------------------------------------------

/// `DELETE dev-machines/<mid>/sessions/<sid>/` — clean shutdown
/// (`machine_sessions.py:134-159`): 403 on mismatch; a missing row
/// still clears the marker and 204s; otherwise revoke
/// `clean_shutdown`, clear the marker, 204.
pub async fn machine_session_delete(
    State(state): State<AppState>,
    Path((raw_mid, raw_sid)): Path<(String, String)>,
    headers: HeaderMap,
) -> Response {
    let pool = match pool_of(&state) {
        Ok(pool) => pool.clone(),
        Err(response) => return response,
    };
    let dev_machine_id = match parse_uuid(&raw_mid) {
        Ok(id) => id,
        Err(response) => return response,
    };
    let sid = match parse_uuid(&raw_sid) {
        Ok(id) => id,
        Err(response) => return response,
    };
    let secret = state.settings().secret_key.clone();
    let auth = match authenticate_machine_token(&pool, secret.as_bytes(), &headers).await {
        Ok(auth) => auth,
        Err(response) => return open_denial(response).await,
    };
    if authed_machine_id(auth.as_ref(), dev_machine_id).is_none() {
        return render(dev_machine_mismatch_drf());
    }

    let redis = redis_client(&state);
    // Canonical (lowercase) rendering: the marker key must match what
    // the open/poll paths write however the URL spelled the id.
    let sid_str = sid.to_string();
    let row = match sqlx::query(machine_session::DELETE_GET_SQL)
        .bind(dev_machine_id)
        .bind(sid)
        .fetch_optional(&pool)
        .await
    {
        Ok(row) => row,
        Err(_) => return server_error(),
    };
    if row.is_none() {
        if machine_outbox::clear_session_marker(redis.as_ref(), &sid_str)
            .await
            .is_err()
        {
            return server_error();
        }
        return StatusCode::NO_CONTENT.into_response();
    }
    if sqlx::query(machine_session::REVOKE_SQL)
        .bind(Utc::now())
        .bind(REASON_CLEAN_SHUTDOWN)
        .bind(sid)
        .execute(&pool)
        .await
        .is_err()
    {
        return server_error();
    }
    if machine_outbox::clear_session_marker(redis.as_ref(), &sid_str)
        .await
        .is_err()
    {
        return server_error();
    }
    StatusCode::NO_CONTENT.into_response()
}

// ---------------------------------------------------------------------------
// Poll
// ---------------------------------------------------------------------------

/// Bookkeeping plan (`_poll_bookkeeping`, `:175-216`): the wait window
/// plus whether this poll drains the PEL (`use_zero`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PollPlan {
    block_ms: i64,
    use_zero: bool,
}

/// Plan from the PEL marker (`:211-213`): a fresh session replays its
/// PEL with a zero-block read; a drained one blocks up to the poll
/// interval (floored at 1ms).
fn poll_plan(long_poll_interval_secs: i64, pel_drained: bool) -> PollPlan {
    if pel_drained {
        PollPlan {
            block_ms: (long_poll_interval_secs * 1000).max(1),
            use_zero: false,
        }
    } else {
        PollPlan {
            block_ms: 0,
            use_zero: true,
        }
    }
}

/// Outcome of the eviction-aware wait: messages, or an eviction that
/// landed mid-wait (`_SessionEvictedDuringPoll`, `:290-292`).
enum PollWait {
    Messages(Vec<DecodedMessage>),
    Evicted,
}

/// Next wait slice against the deadline (`:273-276`): `None` once the
/// deadline expired — `BLOCK 0` is never issued on an expired
/// deadline — else the slice capped at [`POLL_SLICE_MS`].
fn next_slice_ms(deadline: Instant, now: Instant) -> Option<u64> {
    let remaining = deadline.saturating_duration_since(now).as_millis();
    if remaining == 0 {
        None
    } else {
        Some(remaining.min(u128::from(POLL_SLICE_MS)) as u64)
    }
}

/// Await messages for the session, breaking early on eviction
/// (`_aread_with_eviction_awareness`, `:219-292`).
///
/// A zero-block plan reads once (PEL replay when `use_zero`, else an
/// immediate empty — both without subscribing). A blocking plan with
/// no Redis does one blocking read; otherwise it subscribes to the
/// session-eviction channel and loops deadline-capped 1s slices,
/// checking the subscription after each empty slice (`get_message`
/// with a zero timeout — any message means eviction) and expiring to
/// empty. Only the first slice replays the PEL (`use_zero` clears
/// after it). Dropping the subscription closes it (the source's
/// `finally: close()` — no explicit unsubscribe on this path).
async fn read_with_eviction_awareness(
    state: &AppState,
    redis: Option<&redis::Client>,
    dev_machine_id: &str,
    session_id: &str,
    plan: &PollPlan,
) -> Result<PollWait, Response> {
    if plan.block_ms <= 0 {
        if !plan.use_zero {
            return Ok(PollWait::Messages(Vec::new()));
        }
        return match machine_outbox::read_for_session(
            redis,
            dev_machine_id,
            session_id,
            0,
            POLL_READ_COUNT,
            true,
        )
        .await
        {
            Ok(messages) => Ok(PollWait::Messages(messages)),
            Err(_) => Err(server_error()),
        };
    }
    let (Some(redis), Some(handle)) = (redis, state.redis()) else {
        // No Redis: the single blocking read (`:257-264`).
        return match machine_outbox::read_for_session(
            redis,
            dev_machine_id,
            session_id,
            plan.block_ms,
            POLL_READ_COUNT,
            plan.use_zero,
        )
        .await
        {
            Ok(messages) => Ok(PollWait::Messages(messages)),
            Err(_) => Err(server_error()),
        };
    };
    let channel = machine_keys::session_eviction_channel(dev_machine_id);
    let mut pubsub = match handle.subscribe(&channel).await {
        Ok(pubsub) => pubsub,
        Err(_) => return Err(server_error()),
    };
    let deadline = Instant::now() + Duration::from_millis(plan.block_ms as u64);
    let mut use_zero = plan.use_zero;
    loop {
        let Some(slice_ms) = next_slice_ms(deadline, Instant::now()) else {
            return Ok(PollWait::Messages(Vec::new()));
        };
        let messages = match machine_outbox::read_for_session(
            Some(redis),
            dev_machine_id,
            session_id,
            slice_ms as i64,
            POLL_READ_COUNT,
            use_zero,
        )
        .await
        {
            Ok(messages) => messages,
            Err(_) => return Err(server_error()),
        };
        if !messages.is_empty() {
            return Ok(PollWait::Messages(messages));
        }
        use_zero = false;
        match tokio::time::timeout(Duration::ZERO, handle.next_payload(&mut pubsub)).await {
            // Any message on the single-channel subscription is the
            // eviction publish (`:290-292`).
            Ok(Ok(_)) => return Ok(PollWait::Evicted),
            // A failing subscription check propagates like the
            // source's unguarded `get_message`.
            Ok(Err(_)) => return Err(server_error()),
            // Elapsed: nothing buffered; next slice.
            Err(_) => {}
        }
    }
}

/// `POST dev-machines/<mid>/sessions/<sid>/poll` — long-poll
/// (`machine_session_poll`, `:295-345`).
///
/// A plain async view (not DRF): the auth class is invoked directly
/// (401s re-rendered spaced/challenge-free), the body parses as raw
/// JSON, and bookkeeping runs in short-lived checkouts — the wait
/// below holds NO pooled connection. After the wait, the original
/// plan's `use_zero` (not the loop-mutated one) decides the PEL-drain
/// mark; the 200 carries the envelope.
pub async fn machine_session_poll(
    State(state): State<AppState>,
    Path((raw_mid, raw_sid)): Path<(String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let pool = match pool_of(&state) {
        Ok(pool) => pool.clone(),
        Err(response) => return response,
    };
    let dev_machine_id = match parse_uuid(&raw_mid) {
        Ok(id) => id,
        Err(response) => return response,
    };
    let sid = match parse_uuid(&raw_sid) {
        Ok(id) => id,
        Err(response) => return response,
    };
    let secret = state.settings().secret_key.clone();
    let auth = match authenticate_machine_token(&pool, secret.as_bytes(), &headers).await {
        Ok(auth) => auth,
        Err(response) => return poll_denial(response).await,
    };
    if authed_machine_id(auth.as_ref(), dev_machine_id).is_none() {
        return render(dev_machine_mismatch_poll());
    }

    let mid = dev_machine_id.to_string();
    let sid_str = sid.to_string();
    let body = match poll_body(&body) {
        Ok(body) => body,
        Err(response) => return response,
    };

    // Bookkeeping (`_poll_bookkeeping`, `:175-216`): the checks and
    // the ack parse run before any write, the session and machine
    // touches share one timestamp, and the plan derives from the PEL
    // marker.
    let row = match sqlx::query(machine_session::POLL_GET_SQL)
        .bind(dev_machine_id)
        .bind(sid)
        .fetch_optional(&pool)
        .await
    {
        Ok(row) => row,
        Err(_) => return server_error(),
    };
    let Some(row) = row else {
        return render(session_evicted_poll(None));
    };
    let session = match machine_session_from_row(&row) {
        Ok(session) => session,
        Err(_) => return server_error(),
    };
    if session.revoked_at.is_some() {
        return render(session_evicted_poll(Some(&session.revoked_reason)));
    }
    let ack_ids = match ack_ids_from_body(&body) {
        Ok(ids) => ids,
        Err(()) => return server_error(),
    };
    let now = Utc::now();
    if sqlx::query(machine_session::TOUCH_SQL)
        .bind(now)
        .bind(sid)
        .execute(&pool)
        .await
        .is_err()
    {
        return server_error();
    }
    if sqlx::query(DEV_MACHINE_TOUCH_SQL)
        .bind(now)
        .bind(dev_machine_id)
        .execute(&pool)
        .await
        .is_err()
    {
        return server_error();
    }
    let redis = redis_client(&state);
    if !ack_ids.is_empty()
        && machine_outbox::ack_for_session(redis.as_ref(), &mid, &ack_ids)
            .await
            .is_err()
    {
        return server_error();
    }
    let pel_drained = match machine_outbox::is_pel_drained(redis.as_ref(), &sid_str).await {
        Ok(drained) => drained,
        Err(_) => return server_error(),
    };
    let plan = poll_plan(state.settings().runner.long_poll_interval_secs, pel_drained);

    let messages =
        match read_with_eviction_awareness(&state, redis.as_ref(), &mid, &sid_str, &plan).await {
            Ok(PollWait::Messages(messages)) => messages,
            Ok(PollWait::Evicted) => return render(session_evicted_poll(None)),
            Err(response) => return response,
        };
    if plan.use_zero
        && machine_outbox::mark_pel_drained(redis.as_ref(), &state.settings().runner, &sid_str)
            .await
            .is_err()
    {
        return server_error();
    }

    render(poll_200(
        &messages,
        &django_isoformat(Utc::now()),
        state.settings().runner.long_poll_interval_secs,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::runner_enroll::auth::{
        bearer_failure_response, server_error as auth_server_error, CODE_MACHINE_TOKEN_INVALID,
    };

    async fn body_text(response: Response) -> String {
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("body collects")
            .to_bytes();
        String::from_utf8(bytes.to_vec()).expect("body is utf-8")
    }

    fn body_with_ack(ack: Value) -> Map<String, Value> {
        let mut map = Map::new();
        map.insert("ack".to_owned(), ack);
        map
    }

    /// `list(body.get("ack") or [])`, every branch
    /// (`machine_sessions.py:197`).
    #[test]
    fn ack_ids_cover_every_body_shape() {
        // Missing / null / falsy → empty.
        assert_eq!(ack_ids_from_body(&Map::new()), Ok(Vec::new()));
        assert_eq!(
            ack_ids_from_body(&body_with_ack(Value::Null)),
            Ok(Vec::new())
        );
        for falsy in [
            Value::Bool(false),
            Value::from(0),
            Value::from(0.0),
            Value::String(String::new()),
            Value::Array(Vec::new()),
            Value::Object(Map::new()),
        ] {
            assert_eq!(ack_ids_from_body(&body_with_ack(falsy)), Ok(Vec::new()));
        }
        // Lists map item-wise; falsy items drop (the `if sid` filter).
        assert_eq!(
            ack_ids_from_body(&body_with_ack(Value::Array(vec![
                Value::String("1790985779555-0".to_owned()),
                Value::String(String::new()),
                Value::from(7),
                Value::Bool(false),
                Value::Null,
            ]))),
            Ok(vec!["1790985779555-0".to_owned(), "7".to_owned()])
        );
        // Truthy string splits into chars; truthy dict into keys.
        assert_eq!(
            ack_ids_from_body(&body_with_ack(Value::String("ab".to_owned()))),
            Ok(vec!["a".to_owned(), "b".to_owned()])
        );
        let mut dict = Map::new();
        dict.insert("k".to_owned(), Value::from(1));
        assert_eq!(
            ack_ids_from_body(&body_with_ack(Value::Object(dict))),
            Ok(vec!["k".to_owned()])
        );
        // Truthy numbers/bools are the source's `TypeError` → 500 …
        assert_eq!(ack_ids_from_body(&body_with_ack(Value::from(5))), Err(()));
        assert_eq!(
            ack_ids_from_body(&body_with_ack(Value::Bool(true))),
            Err(())
        );
        // … as are `true` items and nested ack items (the redis-py
        // `DataError`: `redis==5.0.4` rejects `bool`, and the truthy
        // `true` survives the `if sid` filter to reach the encoder).
        assert_eq!(
            ack_ids_from_body(&body_with_ack(Value::Array(vec![Value::Bool(true)]))),
            Err(())
        );
        assert_eq!(
            ack_ids_from_body(&body_with_ack(Value::Array(vec![Value::Array(vec![])]))),
            Err(())
        );
    }

    /// Plan derivation (`:211-213`): fresh sessions replay with a
    /// zero block, drained ones wait the interval floored at 1ms.
    #[test]
    fn poll_plan_derives_block_and_use_zero() {
        assert_eq!(
            poll_plan(25, false),
            PollPlan {
                block_ms: 0,
                use_zero: true
            }
        );
        assert_eq!(
            poll_plan(25, true),
            PollPlan {
                block_ms: 25_000,
                use_zero: false
            }
        );
        assert_eq!(poll_plan(0, true).block_ms, 1);
    }

    /// Slicing (`:273-276`): capped at 1s, and `None` on an expired
    /// deadline so `BLOCK 0` is never issued past it.
    #[test]
    fn next_slice_caps_and_expires() {
        let now = Instant::now();
        assert_eq!(
            next_slice_ms(now + Duration::from_millis(2500), now),
            Some(1000)
        );
        let short = next_slice_ms(now + Duration::from_millis(300), now)
            .expect("unexpired deadline slices");
        assert!((1..=300).contains(&short));
        assert_eq!(next_slice_ms(now, now + Duration::from_millis(1)), None);
    }

    /// `timezone.now().isoformat()`: `+00:00`, micros iff nonzero.
    #[test]
    fn django_isoformat_matches_python() {
        let with_micros = chrono::DateTime::parse_from_rfc3339("2026-10-02T22:22:34.218067+00:00")
            .expect("fixture time parses")
            .with_timezone(&Utc);
        assert_eq!(
            django_isoformat(with_micros),
            "2026-10-02T22:22:34.218067+00:00"
        );
        let whole = chrono::DateTime::parse_from_rfc3339("2026-10-02T22:22:34+00:00")
            .expect("whole-second time parses")
            .with_timezone(&Utc);
        assert_eq!(django_isoformat(whole), "2026-10-02T22:22:34+00:00");
    }

    /// Poll body handling (`:316-327`): empty → `{}`; garbage → the
    /// 400; well-formed non-dicts (incl. top-level `NaN`/`Infinity`,
    /// which CPython parses) → `{}`.
    #[test]
    fn poll_body_parses_like_the_plain_view() {
        assert_eq!(poll_body(b"").unwrap(), Map::new());
        assert_eq!(poll_body(b"null").unwrap(), Map::new());
        assert_eq!(poll_body(b"[1,2]").unwrap(), Map::new());
        assert_eq!(poll_body(b"NaN").unwrap(), Map::new());
        assert_eq!(poll_body(b"  -Infinity\t").unwrap(), Map::new());
        let mut expected = Map::new();
        expected.insert("ack".to_owned(), Value::Array(Vec::new()));
        assert_eq!(poll_body(br#"{"ack":[]}"#).unwrap(), expected);
        assert!(poll_body(b"{oops").is_err());
        assert!(poll_body(b"   ").is_err());
    }

    /// A post-718-shaped denial (already lowercase-compact with the
    /// challenge): the re-renders must accept it and render
    /// canonically, so the PIDASHCONV-718 fix heals rather than
    /// breaks these paths.
    fn post_718_denial() -> Response {
        Response::builder()
            .status(StatusCode::UNAUTHORIZED)
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::WWW_AUTHENTICATE, AUTHENTICATE_HEADER_BEARER)
            .body(axum::body::Body::from(
                r#"{"detail":"machine_token_invalid"}"#,
            ))
            .expect("denial builds")
    }

    /// The open/delete 401 re-render: the D-13 denial keeps its code
    /// and `Bearer` challenge but renders lowercase-compact, exactly
    /// DRF's `exception_handler`; 500s pass through untouched.
    #[tokio::test]
    async fn open_denial_rerenders_401_lowercase_compact_with_challenge() {
        for denial in [
            bearer_failure_response(CODE_MACHINE_TOKEN_INVALID),
            post_718_denial(),
        ] {
            let denied = open_denial(denial).await;
            assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);
            assert_eq!(
                denied
                    .headers()
                    .get(header::WWW_AUTHENTICATE)
                    .map(|value| value.to_str().expect("challenge is ascii")),
                Some("Bearer")
            );
            assert_eq!(
                body_text(denied).await,
                r#"{"detail":"machine_token_invalid"}"#
            );
        }
        let failed = open_denial(auth_server_error()).await;
        assert_eq!(failed.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    /// The poll 401 re-render: the D-13 denial keeps its code but
    /// renders lowercase-spaced with no challenge, exactly the
    /// hand-built `JsonResponse` (`:304-307`); 500s pass through
    /// untouched.
    #[tokio::test]
    async fn poll_denial_rerenders_401_spaced_without_challenge() {
        for denial in [
            bearer_failure_response(CODE_MACHINE_TOKEN_INVALID),
            post_718_denial(),
        ] {
            let denied = poll_denial(denial).await;
            assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);
            assert!(denied.headers().get(header::WWW_AUTHENTICATE).is_none());
            assert_eq!(
                body_text(denied).await,
                r#"{"detail": "machine_token_invalid"}"#
            );
        }
        let failed = poll_denial(auth_server_error()).await;
        assert_eq!(failed.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    /// Shape wiring: the DRF/poll 403s render byte-identical to the
    /// FX-RSES-02 captures (compact vs spaced).
    #[tokio::test]
    async fn mismatch_bodies_match_fixture_bytes() {
        let drf = render(dev_machine_mismatch_drf());
        assert_eq!(drf.status(), StatusCode::FORBIDDEN);
        assert_eq!(body_text(drf).await, r#"{"error":"dev_machine_mismatch"}"#);
        let poll = render(dev_machine_mismatch_poll());
        assert_eq!(poll.status(), StatusCode::FORBIDDEN);
        assert_eq!(
            body_text(poll).await,
            r#"{"error": "dev_machine_mismatch"}"#
        );
    }

    /// The `DevMachine` touch this module spells keeps the Django
    /// `.filter(pk=…).update(…)` shape (terms verified against the
    /// FX-RSES-02 machine SQL captures).
    #[test]
    fn dev_machine_touch_keeps_queryset_shape() {
        assert_eq!(
            DEV_MACHINE_TOUCH_SQL,
            "UPDATE \"dev_machine\" SET \"last_seen_at\" = $1 WHERE \"dev_machine\".\"id\" = $2"
        );
    }
}
