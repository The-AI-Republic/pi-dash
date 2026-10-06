//! Runner session open + delete endpoints (D-14, stage 5).
//!
//! Port of `apps/api/pi_dash/runner/views/sessions.py:50-313`
//! (PIDASHCONV-557): the per-runner session lifecycle.
//!
//! * [`runner_session_open`][]: `POST runners/<rid>/sessions/`
//!   (`:132-285`) — protocol-header 426, `auth_runner` 403,
//!   project-slug 409, the revoke-prior + insert + hello + reaper +
//!   chat-release transaction (503 on lock timeout), post-tx Redis
//!   side effects, resume_ack + redeliver, 201 welcome body.
//! * [`runner_session_delete`][]: `DELETE
//!   runners/<rid>/sessions/<sid>/` (`:288-313`) — 403 mismatch,
//!   missing row still clears the marker, revoke `clean_shutdown` +
//!   `mark_runner_offline` + marker clear, 204.
//!
//! `permission_classes=[]`, `throttle_classes=[]` on both endpoints
//! are preserved as-is: no permission or throttle checks run here.
//!
//! # Dispatch order (DRF-source-verified)
//!
//! Resolver 404 (non-UUID segment) → D-13 auth 401 → view body:
//! protocol check 426 → runner-id match 403 → body parse 400 →
//! project 409. Auth runs inside DRF `initial()`, *before* `post()`;
//! the body parses lazily at its first touch (`request.data`), so the
//! guards above all precede the 400.
//!
//! # Execution model
//!
//! * One sqlx transaction holds the prior revoke, the insert,
//!   `apply_hello` (hello update + the heartbeat reaper with
//!   `exclude_redeliverable`), and `release_active_chats_for_runner`.
//!   The nested `atomic()` blocks (per-chat-session savepoints, the
//!   per-finalize savepoint) are elided into the caller's transaction
//!   (the `runs.rs` `finalize_cancelled` precedent): every error
//!   propagates to the same 503/500 mapping, so the savepoints are
//!   unobservable.
//! * Post-commit effects collect in registration order (Django
//!   `on_commit` is FIFO — `pop(0)` in `base.py`) and fire after the
//!   commit: finalize publishes, the reaper drain (*unisolated* — a
//!   failure 500s with the rows committed, exactly as the source's
//!   bare `_drain_after_commit`), then the per-chat-session publish +
//!   drain pairs (each isolated per its Python site). The Redis side
//!   effects run after all of them, in view order.
//! * `_bound_txn_waits` (`:50-71`) runs first inside the transaction;
//!   Postgres 55P03/57014 anywhere in the transaction or commit maps
//!   to the 503 convoy guard, everything else to 500.
//!
//! # Reuse, not forks
//!
//! * Auth: D-13 [`authenticate_access_token`](crate::runner_enroll::auth::authenticate_access_token)
//!   (PIDASHCONV-589); the 401 re-renders lowercase-compact with the
//!   `Bearer` challenge through [`pidash_types::runner_sessions`]
//!   (the [`machine`](super::machine) precedent — `auth.rs` still
//!   emits capital-`Detail` until PIDASHCONV-718 lands).
//! * Body: [`crate::runner_enroll::read_request_data`] (CPython-exact
//!   JSON, lowercase 400s, non-JSON content proxied to Django), then
//!   [`json_cpython::to_serde_publish`](crate::v1_cycles_modules::json_cpython::to_serde_publish)
//!   for the kernel maps.
//! * Plans: [`session_service`](pidash_services::runner_sessions::session_service)
//!   (hello, reaper, slug, resume, redeliver),
//!   [`chat`](pidash_services::runner_runs::chat) (release rows),
//!   [`finalization`](pidash_services::runner_runs::finalization)
//!   (reap finalizes),
//!   [`drain`](pidash_services::runner_sessions::drain) +
//!   [`pubsub`](pidash_services::runner_sessions::pubsub) (drains,
//!   sends). Kernel SQL runs verbatim where already parameterized;
//!   the Django-literal chat builders execute as parameterized
//!   equivalents (same predicates, locks, order — the L8 `chat.rs`
//!   precedent).
//! * Post-commit drains: `drain_lifecycle_effects` +
//!   `drain_chat_effects` over a local `RunnerPorts` with real D-14
//!   drains; the D-12/D-11/D-10 arms delegate to `LivePorts` (the
//!   fleet-wide merged pattern — those providers land later).
//! * Bodies: [`pidash_types::runner_sessions`] response builders;
//!   shared [`crate::runner_runs`] response helpers.
//!
//! # Ported bugs (translate, don't redesign; also listed in the PR)
//!
//! * BUG-open-hello-reaps (`session_service.py:142-147`): `apply_hello`
//!   ends with `reap_stale_busy_runs(exclude_redeliverable=True)`,
//!   so every session open may fail stale `RUNNING` runs.
//! * BUG-open-barrier-always-empty (`:246-254` with the open-path
//!   exclusion): the stopped-cancel scans run but `CANCEL_REQUESTED`
//!   is never reapable here, so the barrier write and the project-move
//!   handoffs never fire — the scans still execute.
//! * BUG-open-nondict-500 (`sessions.py:143-147`): a truthy non-dict
//!   JSON body reaches `body.get` and 500s (`AttributeError`).
//! * BUG-open-naive-ts-500 (`session_service.py:175-179`): a naive
//!   heartbeat `ts` parses but raises `TypeError` in the clamp
//!   comparison (unhandled → 500).
//! * BUG-open-bad-resume-id-500 (`:482`): a malformed `in_flight_run`
//!   reaches `filter(id=…)` and 500s (`ValidationError`); the
//!   redeliver skip for the same value is silently ignored.
//! * BUG-open-drain-500-after-commit (`:277-290`): `_drain_after_commit`
//!   has no `try`, so a drain failure 500s with the session row (and
//!   any reaps) already committed.
//! * BUG-open-finalize-miss-silent (`agent_run_finalization.py:64-68`):
//!   a lost finalize lock returns `False`, ignored by the reaper.
//! * BUG-open-int-underscores (`sessions.py:101-102`): the protocol
//!   header parses via `int()`, so `4_0` reads as 40.
//!
//! # Approximations (no contract input covers them)
//!
//! * A 500 body is the shared runner JSON; Django renders its HTML
//!   error page on these paths. Contract tests pin the 500 status
//!   only.
//! * A non-ASCII-decimal-digit protocol header (fullwidth digits and
//!   friends, which CPython `int()` accepts) 426s; ASCII spellings,
//!   signs, single underscores, and bignums match exactly.
//! * Unknown enum values read from the database answer 500 (the L8
//!   `chat.rs` precedent); Django would keep them as strings.
//!
//! Fixture: `rust-api/fixtures/runner_sessions/fx-rses-02-shapes.json`
//! (FX-RSES-02); the `#[cfg(test)]` suites replay the handler-owned
//! branches (protocol matrix, 401 re-render, 503 mapping, route
//! registration in `mod.rs`).

// Every handler returns a fully-rendered `Response` by design (the
// runner `run_endpoints` precedent, which carries the same allow).
#![allow(clippy::result_large_err)]

use std::time::Instant;

use axum::extract::{Path, Request, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Utc};
use http_body_util::BodyExt as _;
use serde_json::{Map, Value};
use sqlx::PgPool;
use uuid::Uuid;

use pidash_db::runner_runs::{
    chat_event, chat_message, chat_session, AgentChatMessage, AgentChatSession,
};
use pidash_db::runner_sessions::models::runner_session::{self, runner_session_from_row};
use pidash_db::runner_sessions::outbox::{self as session_outbox, OutboxError};
use pidash_services::runner_enroll::tokens as enroll_tokens;
use pidash_services::runner_runs::chat as chat_kernel;
use pidash_services::runner_runs::chat::ChatEffect;
use pidash_services::runner_runs::finalization;
use pidash_services::runner_runs::LifecycleEffect;
use pidash_services::runner_sessions::drain as drain_kernel;
use pidash_services::runner_sessions::guards as session_guards;
use pidash_services::runner_sessions::pubsub as pubsub_kernel;
use pidash_services::runner_sessions::pubsub::PubsubStore;
use pidash_services::runner_sessions::session_service as session_kernel;
use pidash_types::dispatch::AgentExecutorKind;
use pidash_types::runner_runs::{
    AgentChatMessageRole, AgentChatMessageStatus, AgentRunStatus, TERMINAL_RUN_STATUSES,
};
use pidash_types::runner_sessions::keys::runner as runner_keys;
use pidash_types::runner_sessions::{
    project_mismatch_drf, protocol_version_unsupported_drf, runner_id_mismatch_drf,
    runner_open_201, runner_state_locked_drf, unauthorized_drf, HttpResponse,
};

use crate::runner_enroll::auth::{authenticate_access_token, AUTHENTICATE_HEADER_BEARER};
use crate::runner_enroll::read_request_data;
use crate::runner_runs::chat::drain_chat_effects;
use crate::runner_runs::run_endpoints::drain_lifecycle_effects;
use crate::runner_runs::{
    is_uuid_path_segment, json_response, pool_of, py_truthy, server_error, LivePorts, PortError,
    RunnerPorts,
};
use crate::state::AppState;
use crate::v1_cycles_modules::json_cpython;

/// Revoke reason stamped on the prior session at open (`:181`).
const REASON_EVICTED_BY_NEW_SESSION: &str = "evicted_by_new_session";
/// Revoke reason stamped at delete (`:308`).
const REASON_CLEAN_SHUTDOWN: &str = "clean_shutdown";
/// `RUNNER_TXN_LOCK_TIMEOUT_MS` (`:67`): a `getattr` default, not a
/// Django setting (absent from `settings/common.py`).
const TXN_LOCK_TIMEOUT_MS: i64 = 5000;
/// `RUNNER_TXN_STATEMENT_TIMEOUT_MS` (`:68`): likewise a default.
const TXN_STATEMENT_TIMEOUT_MS: i64 = 20000;
/// `RUNNER_SESSION_OPEN_REDIS_WARN_MS` (`:87`): likewise a default.
const REDIS_WARN_MS: u128 = 500;
/// The `detail` text `release_active_chats_for_runner` stores on
/// session open (`:192-195`).
const CHAT_RELEASE_DETAIL: &str =
    "runner opened a new session before the prior chat turn completed";
/// Minimum idle ms for the open-path claim (`outbox.py:286-291`
/// default): claim everything pending, however fresh.
const CLAIM_MIN_IDLE_MS: i64 = 0;
/// The reaper's finalize `SET` order (`status`, `ended_at`,
/// `queue_position`, the two markers, then `error`, `error_code` in
/// updates order): pinned so a kernel drift fails loudly instead of
/// mis-binding (the `runs.rs` `CANCEL_FINALIZE_COLUMNS` precedent).
const REAP_FINALIZE_COLUMNS: [&str; 7] = [
    "status",
    "ended_at",
    "queue_position",
    "terminal_hooks_applied_at",
    "terminal_capacity_released_at",
    "error",
    "error_code",
];
/// The hello `SET` order with a capabilities change (model
/// field-definition order, `capabilities` first).
const HELLO_COLUMNS_WITH_CAPS: [&str; 6] = [
    "capabilities",
    "os",
    "arch",
    "runner_version",
    "dev_metadata",
    "last_heartbeat_at",
];
/// The hello `SET` order without one.
const HELLO_COLUMNS_BARE: [&str; 5] = [
    "os",
    "arch",
    "runner_version",
    "dev_metadata",
    "last_heartbeat_at",
];

// ---------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------

/// Render a shape response: the builder's status plus its exact bytes.
fn render(response: HttpResponse) -> Response {
    let status = StatusCode::from_u16(response.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    json_response(status, response.body)
}

/// Parse a `<uuid:>` path segment. The converter regex is
/// lowercase-hex-only; anything else matches no Django route, so
/// callers proxy the request to Django, reproducing its 404
/// byte-for-byte in every settings flavor (the `v1_work_items`
/// `proxy_request` precedent).
fn parse_uuid(raw: &str) -> Result<Uuid, ()> {
    if !is_uuid_path_segment(raw) {
        return Err(());
    }
    raw.parse().map_err(|_| ())
}

/// `timezone.now().isoformat()`: `+00:00` suffix, microseconds iff
/// nonzero (the `handlers_git_repo` `AutoSi` precedent).
fn django_isoformat(now: DateTime<Utc>) -> String {
    now.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, false)
}

/// Api-crate-owned Redis client (the `LivePorts` precedent):
/// `None` when `REDIS_URL` is unset, empty, or unparsable, mirroring
/// `redis_instance()` returning `None`.
fn redis_client(state: &AppState) -> Option<redis::Client> {
    state
        .settings()
        .redis
        .url
        .as_deref()
        .filter(|url| !url.is_empty())
        .and_then(|url| redis::Client::open(url).ok())
}

/// Quote an L2 column list for a `SELECT` projection (`"a", "b"`),
/// so the executing reads cannot drift from the pinned lists.
fn quoted_columns(table: &str, columns: &[&str]) -> String {
    columns
        .iter()
        .map(|column| format!("\"{table}\".\"{column}\""))
        .collect::<Vec<_>>()
        .join(", ")
}

// ---------------------------------------------------------------------------
// Python `str()` for JSON values (the `session_service` twin: that
// crate is read-only for this issue, so the formula is restated here
// for the view-level `str(project_slug)` / `str(in_flight)` reads)
// ---------------------------------------------------------------------------

/// Python `str(value)` for JSON frame values: `None`/`True`/`False`
/// spellings, integers verbatim, floats in CPython repr form,
/// strings as-is, containers in single-quote repr form.
fn py_str(value: &Value) -> String {
    match value {
        Value::Null => "None".to_owned(),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::Number(n) => py_num_str(n),
        Value::String(s) => s.clone(),
        Value::Array(items) => {
            let inner: Vec<String> = items.iter().map(py_repr).collect();
            format!("[{}]", inner.join(", "))
        }
        Value::Object(map) => {
            let inner: Vec<String> = map
                .iter()
                .map(|(k, v)| format!("{}: {}", py_repr_str(k), py_repr(v)))
                .collect();
            format!("{{{}}}", inner.join(", "))
        }
    }
}

/// CPython `repr` of a number: integers verbatim, floats via
/// [`py_float_str`].
fn py_num_str(n: &serde_json::Number) -> String {
    if let Some(i) = n.as_i64() {
        return i.to_string();
    }
    if let Some(u) = n.as_u64() {
        return u.to_string();
    }
    n.as_f64()
        .map(py_float_str)
        .unwrap_or_else(|| n.to_string())
}

/// CPython `repr(float)`: Rust's `Debug` shortest round-trip with
/// the exponent normalized to Python's `e±XX` form.
fn py_float_str(f: f64) -> String {
    if f.is_nan() {
        return "nan".to_owned();
    }
    if f.is_infinite() {
        return if f > 0.0 {
            "inf".to_owned()
        } else {
            "-inf".to_owned()
        };
    }
    let rust = format!("{f:?}");
    let Some(pos) = rust.find('e') else {
        return rust;
    };
    let (mantissa, exp) = rust.split_at(pos);
    let exp = &exp[1..];
    let (sign, digits) = match exp.strip_prefix('-') {
        Some(digits) => ("-", digits),
        None => ("+", exp.strip_prefix('+').unwrap_or(exp)),
    };
    format!("{mantissa}e{sign}{digits:0>2}")
}

/// Python `repr(value)` for a value nested in a container: identical
/// to [`py_str`] except strings, which gain quotes.
fn py_repr(value: &Value) -> String {
    match value {
        Value::String(s) => py_repr_str(s),
        other => py_str(other),
    }
}

/// Python `repr(str)`: single quotes unless the string contains
/// `'` but not `"`, backslash escapes for quotes, backslashes,
/// whitespace, and non-printables (`\xNN` / `\uNNNN` /
/// `\U00NNNNNN`).
fn py_repr_str(s: &str) -> String {
    let use_double = s.contains('\'') && !s.contains('"');
    let quote = if use_double { '"' } else { '\'' };
    let mut out = String::with_capacity(s.len() + 2);
    out.push(quote);
    for c in s.chars() {
        if c == quote {
            out.push('\\');
            out.push(c);
        } else {
            match c {
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                c if py_printable(c) => out.push(c),
                c if (c as u32) < 0x100 => {
                    out.push_str(&format!("\\x{:02x}", c as u32));
                }
                c if (c as u32) < 0x1_0000 => {
                    out.push_str(&format!("\\u{:04x}", c as u32));
                }
                c => {
                    out.push_str(&format!("\\U{:08x}", c as u32));
                }
            }
        }
    }
    out.push(quote);
    out
}

fn py_printable(c: char) -> bool {
    !(c.is_control() || c == '\u{7f}' || c == '\u{2028}' || c == '\u{2029}')
}

// ---------------------------------------------------------------------------
// Protocol header (`_check_protocol_header`, `:97-119`)
// ---------------------------------------------------------------------------

/// Parse the stripped header per CPython `int()`: an optional sign,
/// ASCII digits, single underscores between digits. Bignums short-
/// circuit by sign (a >18-digit magnitude is out of `i64` range on
/// both sides of the minimum). Non-ASCII decimal digits are the one
/// documented gap (CPython accepts them; this 426s).
fn protocol_version(raw: &str) -> Option<i64> {
    let digits = raw.strip_prefix('+').or_else(|| raw.strip_prefix('-'));
    let negative = raw.starts_with('-');
    let digits = digits.unwrap_or(raw);
    if digits.is_empty() {
        return None;
    }
    let mut prev_underscore = false;
    let mut digit_count = 0usize;
    for c in digits.chars() {
        if c == '_' {
            if prev_underscore || digit_count == 0 {
                return None;
            }
            prev_underscore = true;
        } else if c.is_ascii_digit() {
            prev_underscore = false;
            digit_count += 1;
        } else {
            return None;
        }
    }
    if prev_underscore {
        return None;
    }
    if digit_count > 18 {
        // Below `i64` range iff negative; either way decided without
        // parsing (the minimum is small and positive).
        return if negative {
            Some(i64::MIN)
        } else {
            Some(i64::MAX)
        };
    }
    let compact: String = digits.chars().filter(|c| *c != '_').collect();
    let mut value: i64 = compact.parse().ok()?;
    if negative {
        value = -value;
    }
    Some(value)
}

/// `_check_protocol_header` as data: the 426 response when the
/// `X-Runner-Protocol-Version` header parses below the minimum (or
/// does not parse at all), else `None`. A missing or blank header
/// passes.
fn check_protocol_header(headers: &HeaderMap, minimum: i64) -> Option<Response> {
    let raw = headers
        .get("x-runner-protocol-version")
        .map(|value| String::from_utf8_lossy(value.as_bytes()).into_owned())
        .unwrap_or_default();
    let stripped = raw.trim();
    if stripped.is_empty() {
        return None;
    }
    match protocol_version(stripped) {
        Some(version) if version >= minimum => None,
        _ => Some(render(protocol_version_unsupported_drf(minimum))),
    }
}

// ---------------------------------------------------------------------------
// Auth denials (the `machine.rs` precedent: `auth.rs` still emits
// capital-`Detail` until PIDASHCONV-718 lands, so the code comes back
// out of the denial body and re-renders lowercase-compact)
// ---------------------------------------------------------------------------

/// Read a D-13 denial body back into its code, returning the code
/// plus the rebuilt response. Accepts the correct lowercase `detail`
/// and the pre-718 capital `Detail`; anything else yields `None` and
/// the caller passes the response through untouched.
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

/// Re-render a D-13 access-token denial for the DRF open/delete
/// endpoints: the 401 keeps its code and `Bearer` challenge but
/// renders lowercase-compact (DRF's `exception_handler`). 500s and
/// unrecognized shapes pass through untouched.
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

// ---------------------------------------------------------------------------
// Session-open side effects (`_session_open_side_effect`, `:74-94`)
// ---------------------------------------------------------------------------

/// Run one Redis side effect: time it, swallow Redis failures with
/// an error log, and warn when slow. A non-Redis provider error
/// (unreachable on these SQL-free verbs — only [`OutboxError::Redis`]
/// can occur) answers 500 instead, mirroring the Python `except
/// (RedisError, OSError)` narrowly.
async fn side_effect<T>(
    runner_id: Uuid,
    label: &'static str,
    fut: impl std::future::Future<Output = Result<T, OutboxError>>,
) -> Result<Option<T>, Response> {
    let started = Instant::now();
    let outcome = fut.await;
    let elapsed_ms = started.elapsed().as_millis();
    if elapsed_ms >= REDIS_WARN_MS {
        tracing::warn!(
            runner_id = %runner_id,
            step = label,
            duration_ms = elapsed_ms,
            "runner session-open Redis side effect slow",
        );
    }
    match outcome {
        Ok(value) => Ok(Some(value)),
        Err(OutboxError::Redis(error)) => {
            tracing::error!(
                %error,
                runner_id = %runner_id,
                step = label,
                "runner session-open Redis side effect failed",
            );
            Ok(None)
        }
        Err(error) => {
            tracing::error!(
                %error,
                runner_id = %runner_id,
                step = label,
                "runner session-open Redis side effect failed unexpectedly",
            );
            Err(server_error())
        }
    }
}

/// Time an infallible side effect (the claim reports `usize`, never
/// fails) for the slow-warn only.
async fn side_effect_timed<T>(
    runner_id: Uuid,
    label: &'static str,
    fut: impl std::future::Future<Output = T>,
) -> T {
    let started = Instant::now();
    let value = fut.await;
    let elapsed_ms = started.elapsed().as_millis();
    if elapsed_ms >= REDIS_WARN_MS {
        tracing::warn!(
            runner_id = %runner_id,
            step = label,
            duration_ms = elapsed_ms,
            "runner session-open Redis side effect slow",
        );
    }
    value
}

// ---------------------------------------------------------------------------
// Transaction failures (`except OperationalError` → 503, `:202-213`)
// ---------------------------------------------------------------------------

/// A failure inside the open transaction: lock/statement timeouts
/// (Postgres 55P03/57014 — Django's `OperationalError` from
/// `_bound_txn_waits`) answer 503 so the runner backs off; anything
/// else is the source's unhandled 500.
enum TxnFail {
    Timeout,
    Other,
}

/// Whether a Postgres error code is a `_bound_txn_waits` timeout:
/// `55P03` (`lock_timeout`) or `57014` (`statement_timeout`,
/// `query_canceled`).
fn is_timeout_code(code: Option<&str>) -> bool {
    matches!(code, Some("55P03" | "57014"))
}

impl From<sqlx::Error> for TxnFail {
    fn from(error: sqlx::Error) -> Self {
        let timeout = error
            .as_database_error()
            .and_then(|db| db.code())
            .is_some_and(|code| is_timeout_code(Some(code.as_ref())));
        if timeout {
            TxnFail::Timeout
        } else {
            TxnFail::Other
        }
    }
}

fn txn_fail_response(fail: TxnFail, runner_id: Uuid) -> Response {
    match fail {
        TxnFail::Timeout => {
            tracing::error!(
                runner_id = %runner_id,
                "runner session-open timed out waiting on runner-scoped locks",
            );
            render(runner_state_locked_drf())
        }
        TxnFail::Other => server_error(),
    }
}

// ---------------------------------------------------------------------------
// Cross-domain ports: real D-14 drains, LivePorts for the rest
// ---------------------------------------------------------------------------

/// Pool-backed [`PubsubStore`] for the drain sends (the
/// `teardown.rs` `PoolPubsubStore` precedent): each statement
/// autocommits, like the source paths that run outside any `atomic`.
struct SessionPubsub<'p, 's> {
    pool: &'p PgPool,
    state: &'s AppState,
}

impl PubsubStore for SessionPubsub<'_, '_> {
    async fn enqueue_for_runner(
        &self,
        runner_id: Uuid,
        message: &Map<String, Value>,
    ) -> Result<Option<String>, OutboxError> {
        let redis = redis_client(self.state);
        session_outbox::enqueue_for_runner(
            redis.as_ref(),
            self.pool,
            &self.state.settings().runner,
            runner_id,
            message,
        )
        .await
    }

    async fn enqueue_for_machine(
        &self,
        dev_machine_id: Uuid,
        message: &Map<String, Value>,
    ) -> Result<Option<String>, pidash_db::runner_sessions::machine_outbox::MachineOutboxError>
    {
        let redis = redis_client(self.state);
        pidash_db::runner_sessions::machine_outbox::enqueue_for_machine(
            redis.as_ref(),
            self.pool,
            &self.state.settings().runner,
            dev_machine_id,
            message,
        )
        .await
    }

    async fn active_runner_sessions(
        &self,
        runner_id: Uuid,
    ) -> Result<Vec<pidash_db::runner_sessions::RunnerSession>, OutboxError> {
        let rows = sqlx::query(pubsub_kernel::CLOSE_ACTIVE_SESSIONS_SQL)
            .bind(runner_id)
            .fetch_all(self.pool)
            .await
            .map_err(OutboxError::Db)?;
        rows.iter()
            .map(runner_session_from_row)
            .collect::<Result<Vec<_>, _>>()
            .map_err(OutboxError::Db)
    }

    async fn revoke_runner_session(
        &self,
        session_id: Uuid,
        reason: &str,
    ) -> Result<(), OutboxError> {
        sqlx::query(runner_session::REVOKE_SQL)
            .bind(Utc::now())
            .bind(reason)
            .bind(session_id)
            .execute(self.pool)
            .await
            .map_err(OutboxError::Db)?;
        Ok(())
    }

    async fn clear_session_marker(&self, session_id: Uuid) -> Result<(), OutboxError> {
        let redis = redis_client(self.state);
        session_outbox::clear_session_marker(redis.as_ref(), &session_id.to_string()).await
    }

    async fn publish_session_eviction(
        &self,
        runner_id: Uuid,
        old_session_id: Uuid,
        new_session_id: &str,
    ) -> Result<(), OutboxError> {
        let redis = redis_client(self.state);
        session_outbox::publish_session_eviction(
            redis.as_ref(),
            &runner_id.to_string(),
            Some(&old_session_id.to_string()),
            new_session_id,
        )
        .await
    }
}

/// [`RunnerPorts`] for the session-open post-commit effects: D-14
/// sends and drains execute for real through the merged providers;
/// the D-12/D-11/D-10 arms delegate to [`LivePorts`].
struct SessionPorts<'s> {
    live: LivePorts,
    state: &'s AppState,
}

impl<'s> SessionPorts<'s> {
    fn new(pool: &PgPool, state: &'s AppState) -> Self {
        Self {
            live: LivePorts::new(pool.clone(), state),
            state,
        }
    }

    fn pool(&self) -> &PgPool {
        self.live.pool()
    }
}

impl RunnerPorts for SessionPorts<'_> {
    async fn send_to_runner(&self, runner_id: Uuid, message: Value) -> Result<(), PortError> {
        let Value::Object(map) = message else {
            tracing::warn!(%runner_id, "send_to_runner: non-object frame; swallowed");
            return Ok(());
        };
        let store = SessionPubsub {
            pool: self.pool(),
            state: self.state,
        };
        match pubsub_kernel::send_to_runner(&store, runner_id, &map).await {
            Ok(outcome) => {
                for warning in &outcome.warnings {
                    tracing::warn!("{warning}");
                }
                Ok(())
            }
            Err(OutboxError::RunnerOffline { message_type, .. }) => Err(PortError::Offline {
                runner_id,
                message_type,
            }),
            Err(error) => {
                tracing::warn!(%error, %runner_id, "send_to_runner: enqueue failed; swallowed");
                Ok(())
            }
        }
    }

    async fn drain_for_runner_by_id(&self, runner_id: Uuid) -> Result<(), PortError> {
        drain_for_runner_by_id(self.pool(), self.state, runner_id)
            .await
            .map_err(PortError::Transport)
    }

    async fn drain_pod_by_id(&self, pod_id: Uuid) -> Result<(), PortError> {
        drain_pod_by_id(self.pool(), self.state, pod_id)
            .await
            .map_err(PortError::Transport)
    }

    async fn post_run_orchestration(&self, run_id: Uuid) -> Result<(), PortError> {
        self.live.post_run_orchestration(run_id).await
    }

    async fn complete_project_move_handoff(&self, run_id: Uuid) -> Result<(), PortError> {
        self.live.complete_project_move_handoff(run_id).await
    }

    async fn agent_system_user_id(&self) -> Result<Uuid, PortError> {
        self.live.agent_system_user_id().await
    }

    async fn dispatch_waiting(&self, workspace_id: Uuid) -> Result<(), PortError> {
        self.live.dispatch_waiting(workspace_id).await
    }

    async fn emit_terminal_effects(&self, run_id: Uuid) -> Result<(), PortError> {
        self.live.emit_terminal_effects(run_id).await
    }

    async fn emit_fire_tick(&self, ticker_id: Uuid) -> Result<(), PortError> {
        self.live.emit_fire_tick(ticker_id).await
    }

    fn redis_client(&self) -> Option<&redis::Client> {
        self.live.redis_client()
    }
}

// ---------------------------------------------------------------------------
// Drain executors (`matcher.py:194-306` via the `drain` kernel)
// ---------------------------------------------------------------------------

/// `drain_for_runner_by_id` (`:298-303`): runner lookup (miss ⇒ `Ok`,
/// no tx), else the single-assignment drain.
async fn drain_for_runner_by_id(
    pool: &PgPool,
    state: &AppState,
    runner_id: Uuid,
) -> Result<(), String> {
    let row = sqlx::query(drain_kernel::DRAIN_FOR_RUNNER_BY_ID_LOOKUP_SQL)
        .bind(runner_id)
        .fetch_optional(pool)
        .await
        .map_err(|error| error.to_string())?;
    if row.is_none() {
        return Ok(());
    }
    drain_for_runner(pool, state, runner_id).await
}

/// `drain_for_runner` (`:252-296`): re-select the runner under
/// `SKIP LOCKED`, take the next queued run, write the assignment,
/// commit, then dispatch the frame (the nested `on_commit` fires
/// immediately post-commit) and log.
async fn drain_for_runner(pool: &PgPool, state: &AppState, runner_id: Uuid) -> Result<(), String> {
    use sqlx::Row;
    let mut tx = pool.begin().await.map_err(|error| error.to_string())?;
    let threshold = session_guards::alive_threshold(Utc::now());
    let locked = sqlx::query(drain_kernel::DRAIN_FOR_RUNNER_LOCK_SQL)
        .bind(threshold)
        .bind(runner_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|error| error.to_string())?;
    let Some(locked) = locked else {
        tx.commit().await.map_err(|error| error.to_string())?;
        return Ok(());
    };
    let owner_id: Uuid = locked
        .try_get("owner_id")
        .map_err(|error| error.to_string())?;
    let pod_id: Uuid = locked
        .try_get("pod_id")
        .map_err(|error| error.to_string())?;
    let provisioning: String = locked
        .try_get("provisioning")
        .map_err(|error| error.to_string())?;
    let visibility: i16 = locked
        .try_get("visibility")
        .map_err(|error| error.to_string())?;
    // Non-private runners issue no query (the `qs.none()` arm).
    let Some(next_sql) = drain_kernel::next_for_runner_sql(&provisioning, i32::from(visibility))
    else {
        tx.commit().await.map_err(|error| error.to_string())?;
        return Ok(());
    };
    let run = sqlx::query(&next_sql)
        .bind(pod_id)
        .bind(runner_id)
        .bind(owner_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|error| error.to_string())?;
    let Some(run) = run else {
        tx.commit().await.map_err(|error| error.to_string())?;
        return Ok(());
    };
    let run_id: Uuid = run.try_get("id").map_err(|error| error.to_string())?;
    let work_item_id: Option<Uuid> = run
        .try_get("work_item_id")
        .map_err(|error| error.to_string())?;
    let prompt: String = run.try_get("prompt").map_err(|error| error.to_string())?;
    let run_config: Value = run
        .try_get("run_config")
        .map_err(|error| error.to_string())?;
    let Value::Object(run_config) = run_config else {
        return Err("drain_for_runner: run_config is not an object".to_owned());
    };
    // `$3` is a fresh `timezone.now()` per assignment (`:285-289`).
    sqlx::query(drain_kernel::ASSIGN_RUN_UPDATE_SQL)
        .bind(owner_id)
        .bind(runner_id)
        .bind(Utc::now())
        .bind(run_id)
        .execute(&mut *tx)
        .await
        .map_err(|error| error.to_string())?;
    let plan = drain_kernel::plan_assignment(&drain_kernel::AssignmentFacts {
        run_id,
        runner_id,
        owner_id,
        work_item_id,
        prompt,
        run_config,
    });
    tx.commit().await.map_err(|error| error.to_string())?;
    let drain_kernel::DrainEffect::SendAssign { runner_id, message } = &plan.after_commit;
    let store = SessionPubsub { pool, state };
    let outcome = pubsub_kernel::send_to_runner(&store, *runner_id, message)
        .await
        .map_err(|error| error.to_string())?;
    for warning in &outcome.warnings {
        tracing::warn!("{warning}");
    }
    tracing::info!("{}", drain_kernel::drain_for_runner_log(runner_id, &run_id));
    Ok(())
}

/// `drain_pod_by_id` (`:242-250`): pod lookup (miss ⇒ `Ok`, no tx),
/// else the drain loop.
async fn drain_pod_by_id(pool: &PgPool, state: &AppState, pod_id: Uuid) -> Result<(), String> {
    let pod = sqlx::query(drain_kernel::DRAIN_POD_BY_ID_LOOKUP_SQL)
        .bind(pod_id)
        .fetch_optional(pool)
        .await
        .map_err(|error| error.to_string())?;
    if pod.is_none() {
        return Ok(());
    }
    drain_pod(pool, state, pod_id).await
}

/// `drain_pod` (`:194-239`): lock the idle list, then per runner in
/// list order take the next queued run, write the assignment, and
/// plan the frame; commit once; then dispatch each frame in
/// assignment order (the `teardown.rs` recipe).
async fn drain_pod(pool: &PgPool, state: &AppState, pod_id: Uuid) -> Result<(), String> {
    use sqlx::Row;
    let mut tx = pool.begin().await.map_err(|error| error.to_string())?;
    let threshold = session_guards::alive_threshold(Utc::now());
    let runners = sqlx::query(drain_kernel::DRAIN_POD_IDLE_RUNNERS_SQL)
        .bind(threshold)
        .bind(pod_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(|error| error.to_string())?;
    let mut plans: Vec<drain_kernel::AssignmentPlan> = Vec::new();
    for runner in &runners {
        let runner_id: Uuid = runner.try_get("id").map_err(|error| error.to_string())?;
        let owner_id: Uuid = runner
            .try_get("owner_id")
            .map_err(|error| error.to_string())?;
        let provisioning: String = runner
            .try_get("provisioning")
            .map_err(|error| error.to_string())?;
        let visibility: i16 = runner
            .try_get("visibility")
            .map_err(|error| error.to_string())?;
        let Some(next_sql) =
            drain_kernel::next_for_runner_sql(&provisioning, i32::from(visibility))
        else {
            continue;
        };
        let run = sqlx::query(&next_sql)
            .bind(pod_id)
            .bind(runner_id)
            .bind(owner_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|error| error.to_string())?;
        let Some(run) = run else {
            continue;
        };
        let run_id: Uuid = run.try_get("id").map_err(|error| error.to_string())?;
        let work_item_id: Option<Uuid> = run
            .try_get("work_item_id")
            .map_err(|error| error.to_string())?;
        let prompt: String = run.try_get("prompt").map_err(|error| error.to_string())?;
        let run_config: Value = run
            .try_get("run_config")
            .map_err(|error| error.to_string())?;
        let Value::Object(run_config) = run_config else {
            return Err("drain_pod: run_config is not an object".to_owned());
        };
        sqlx::query(drain_kernel::ASSIGN_RUN_UPDATE_SQL)
            .bind(owner_id)
            .bind(runner_id)
            .bind(Utc::now())
            .bind(run_id)
            .execute(&mut *tx)
            .await
            .map_err(|error| error.to_string())?;
        plans.push(drain_kernel::plan_assignment(
            &drain_kernel::AssignmentFacts {
                run_id,
                runner_id,
                owner_id,
                work_item_id,
                prompt,
                run_config,
            },
        ));
    }
    tx.commit().await.map_err(|error| error.to_string())?;
    if !plans.is_empty() {
        tracing::info!("{}", drain_kernel::drain_pod_log(&pod_id, plans.len()));
    }
    let store = SessionPubsub { pool, state };
    for plan in &plans {
        let drain_kernel::DrainEffect::SendAssign { runner_id, message } = &plan.after_commit;
        let outcome = pubsub_kernel::send_to_runner(&store, *runner_id, message)
            .await
            .map_err(|error| error.to_string())?;
        for warning in &outcome.warnings {
            tracing::warn!("{warning}");
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// In-transaction executors (`apply_hello`, the reaper, chat release)
// ---------------------------------------------------------------------------

/// What the open transaction hands to the post-commit phase, in
/// `on_commit` registration order: the finalize publishes (reap
/// order), the reaper drain, then the per-chat-session publish +
/// drain pairs.
#[derive(Default)]
struct OpenEffects {
    lifecycle: Vec<LifecycleEffect>,
    session_drains: Vec<session_kernel::SessionEffect>,
    chat: Vec<ChatEffect>,
}

/// Read a [`SetClause`] text value (kernel-drift guard: any other
/// shape is a 500, like the `runs.rs` column pins).
fn set_text(
    clauses: &[pidash_services::runner_runs::SetClause],
    index: usize,
) -> Result<&str, TxnFail> {
    match clauses.get(index).map(|clause| &clause.value) {
        Some(pidash_services::runner_runs::SetValue::Text(text)) => Ok(text),
        _ => Err(TxnFail::Other),
    }
}

/// Read a [`SetClause`] JSON value.
fn set_json(
    clauses: &[pidash_services::runner_runs::SetClause],
    index: usize,
) -> Result<&Value, TxnFail> {
    match clauses.get(index).map(|clause| &clause.value) {
        Some(pidash_services::runner_runs::SetValue::Json(value)) => Ok(value),
        _ => Err(TxnFail::Other),
    }
}

/// The `apply_hello` metadata write (`session_service.py:108-141`):
/// plan from the authenticated runner row (no extra `SELECT` — the
/// auth load already fetched it, as `select_related` does), then the
/// capabilities or bare `UPDATE`.
async fn execute_hello(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    runner: &crate::runner_enroll::auth::RunnerAuthRow,
    body: &Map<String, Value>,
) -> Result<(), TxnFail> {
    let facts = session_kernel::HelloFacts {
        os: runner.os.clone(),
        arch: runner.arch.clone(),
        runner_version: runner.runner_version.clone(),
        dev_metadata: runner.dev_metadata.clone(),
        capabilities: runner.capabilities.clone(),
    };
    let plan = session_kernel::plan_hello_update(&facts, body);
    let columns: Vec<&str> = plan
        .set_clauses
        .iter()
        .map(|clause| clause.column)
        .collect();
    if plan.capabilities_changed {
        if columns.as_slice() != HELLO_COLUMNS_WITH_CAPS {
            return Err(TxnFail::Other);
        }
        sqlx::query(session_kernel::HELLO_UPDATE_SQL)
            .bind(set_json(&plan.set_clauses, 0)?)
            .bind(set_text(&plan.set_clauses, 1)?)
            .bind(set_text(&plan.set_clauses, 2)?)
            .bind(set_text(&plan.set_clauses, 3)?)
            .bind(set_json(&plan.set_clauses, 4)?)
            .bind(Utc::now())
            .bind(runner.id)
            .execute(&mut **tx)
            .await
            .map_err(TxnFail::from)?;
    } else {
        if columns.as_slice() != HELLO_COLUMNS_BARE {
            return Err(TxnFail::Other);
        }
        sqlx::query(session_kernel::HELLO_UPDATE_NO_CAPABILITIES_SQL)
            .bind(set_text(&plan.set_clauses, 0)?)
            .bind(set_text(&plan.set_clauses, 1)?)
            .bind(set_text(&plan.set_clauses, 2)?)
            .bind(set_json(&plan.set_clauses, 3)?)
            .bind(Utc::now())
            .bind(runner.id)
            .execute(&mut **tx)
            .await
            .map_err(TxnFail::from)?;
    }
    Ok(())
}

/// One reaped run's `finalize_agent_run` (`:266-272` +
/// `agent_run_finalization.py:48-86`): lock with the expected runner
/// (miss ⇒ `False`, ignored), the conditional `UPDATE`, the
/// cloud-only terminal event, and the publish pair queued for
/// post-commit. The nested `atomic()` is elided into the caller's
/// transaction (the `runs.rs` precedent).
async fn execute_reap_finalize(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    run_id: Uuid,
    runner_id: Uuid,
    detail: &str,
    effects: &mut Vec<LifecycleEffect>,
) -> Result<(), TxnFail> {
    use sqlx::Row;
    let values = session_kernel::plan_reap_finalize(detail);
    let columns: Vec<&str> = values.clauses.iter().map(|clause| clause.column).collect();
    if columns.as_slice() != REAP_FINALIZE_COLUMNS {
        return Err(TxnFail::Other);
    }
    let lock_sql = finalization::lock_run_for_finalize_sql(true, false);
    let mut lock = sqlx::query(&lock_sql).bind(run_id);
    for status in TERMINAL_RUN_STATUSES {
        lock = lock.bind(status.value());
    }
    let found = lock
        .bind(runner_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(TxnFail::from)?;
    let Some(row) = found else {
        return Ok(());
    };
    let error = set_text(&values.clauses, 5)?;
    let error_code = set_text(&values.clauses, 6)?;
    sqlx::query(&finalization::finalize_update_sql(&values))
        .bind(AgentRunStatus::Failed.value())
        .bind(Utc::now())
        .bind(None::<i16>)
        .bind(None::<DateTime<Utc>>)
        .bind(None::<DateTime<Utc>>)
        .bind(error)
        .bind(error_code)
        .bind(run_id)
        .execute(&mut **tx)
        .await
        .map_err(TxnFail::from)?;
    let executor_raw: String = row.try_get("executor_kind").map_err(TxnFail::from)?;
    if executor_raw == AgentExecutorKind::CloudAgent.value() {
        let exists = sqlx::query_scalar::<_, i32>(finalization::terminal_event_exists_sql())
            .bind(run_id)
            .bind("terminal")
            .fetch_optional(&mut **tx)
            .await
            .map_err(TxnFail::from)?;
        if exists.is_none() {
            let max_seq = sqlx::query_scalar::<_, i32>(finalization::terminal_event_max_seq_sql())
                .bind(run_id)
                .fetch_optional(&mut **tx)
                .await
                .map_err(TxnFail::from)?;
            let event =
                finalization::plan_terminal_event(max_seq, AgentRunStatus::Failed, error_code);
            sqlx::query(&finalization::terminal_event_insert_sql())
                .bind(run_id)
                .bind(event.seq)
                .bind("terminal")
                .bind(event.payload)
                .bind(Utc::now())
                .execute(&mut **tx)
                .await
                .map_err(TxnFail::from)?;
        }
    }
    effects.extend(finalization::plan_publish_effects(run_id));
    Ok(())
}

/// The session-open reaper (`reap_stale_busy_runs` with
/// `exclude_redeliverable`, `:150-290`): stale scan, barrier scans
/// (always empty here, still executed), per-run finalizes, and the
/// `_drain_after_commit` plan. The in-flight cancel probe and retry
/// never run on this path (short-circuited by the exclusion flag).
async fn execute_reap_open(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    runner_id: Uuid,
    body: &Map<String, Value>,
    effects: &mut OpenEffects,
) -> Result<(), TxnFail> {
    let now = Utc::now();
    let heartbeat_ts =
        session_kernel::parse_heartbeat_ts(body.get("ts"), now).map_err(|_| TxnFail::Other)?;
    let cutoff = session_kernel::effective_cutoff(now, heartbeat_ts);
    let in_flight = session_kernel::parse_in_flight_id(body.get("in_flight_run"));
    // Parser-validated, so the re-parse is infallible; the typed
    // bind matters because Postgres has no `uuid = text` operator.
    let in_flight_id: Option<Uuid> = in_flight
        .as_deref()
        .map(str::parse)
        .transpose()
        .map_err(|_| TxnFail::Other)?;
    let statuses = session_kernel::reapable_statuses(true);
    let exclude_in_flight = in_flight_id.is_some();

    let stopped_ids: Vec<Uuid> = {
        let sql = session_kernel::stale_cancel_ids_sql(&statuses, exclude_in_flight);
        let mut query = sqlx::query_scalar(&sql).bind(cutoff).bind(runner_id);
        if let Some(id) = in_flight_id {
            query = query.bind(id);
        }
        query.fetch_all(&mut **tx).await.map_err(TxnFail::from)?
    };
    let stopped_pods: Vec<Option<Uuid>> = {
        let sql = session_kernel::stale_cancel_pod_ids_sql(&statuses, exclude_in_flight);
        let mut query = sqlx::query_scalar(&sql).bind(cutoff).bind(runner_id);
        if let Some(id) = in_flight_id {
            query = query.bind(id);
        }
        query.fetch_all(&mut **tx).await.map_err(TxnFail::from)?
    };
    if !stopped_ids.is_empty() {
        // Dead on the open path (`CANCEL_REQUESTED` is never
        // reapable here); kept so the executor stays total.
        let sql = session_kernel::cancel_barrier_update_sql(stopped_ids.len());
        let mut update = sqlx::query(&sql).bind(now);
        for id in &stopped_ids {
            update = update.bind(*id);
        }
        update.execute(&mut **tx).await.map_err(TxnFail::from)?;
    }
    let reaped: Vec<(Uuid, Option<Uuid>)> = {
        let sql = session_kernel::stale_pairs_sql(&statuses, exclude_in_flight, stopped_ids.len());
        let mut query = sqlx::query_as(&sql).bind(cutoff).bind(runner_id);
        if let Some(id) = in_flight_id {
            query = query.bind(id);
        }
        for id in &stopped_ids {
            query = query.bind(*id);
        }
        query.fetch_all(&mut **tx).await.map_err(TxnFail::from)?
    };
    if !session_kernel::should_schedule_drain(reaped.len(), stopped_ids.len()) {
        return Ok(());
    }
    let detail = session_kernel::reap_error_detail(in_flight.as_deref());
    for (run_id, _) in &reaped {
        execute_reap_finalize(tx, *run_id, runner_id, &detail, &mut effects.lifecycle).await?;
    }
    let reaped_pods: Vec<Option<Uuid>> = reaped.iter().map(|(_, pod)| *pod).collect();
    let pod_ids = session_kernel::drain_pod_ids(&reaped_pods, &stopped_pods);
    effects
        .session_drains
        .extend(session_kernel::plan_drain_after_commit(
            runner_id,
            &pod_ids,
            &stopped_ids,
        ));
    Ok(())
}

// ---------------------------------------------------------------------------
// Chat release (`release_active_chats_for_runner`, `chat.py:451-480`)
// ---------------------------------------------------------------------------

/// Map a locked chat-session row (the `chat_session::COLUMNS`
/// projection) into the L2 struct. Unknown statuses are the fleet's
/// 500 (unreachable here — the lock already filters `open`).
fn chat_session_from_row(row: &sqlx::postgres::PgRow) -> Result<AgentChatSession, TxnFail> {
    use sqlx::Row;
    let status_raw: String = row.try_get("status").map_err(TxnFail::from)?;
    let status = pidash_types::runner_runs::AgentChatSessionStatus::from_value(&status_raw)
        .ok_or(TxnFail::Other)?;
    Ok(AgentChatSession {
        id: row.try_get("id").map_err(TxnFail::from)?,
        workspace_id: row.try_get("workspace_id").map_err(TxnFail::from)?,
        runner_id: row.try_get("runner_id").map_err(TxnFail::from)?,
        created_by_id: row.try_get("created_by_id").map_err(TxnFail::from)?,
        pod_id: row.try_get("pod_id").map_err(TxnFail::from)?,
        status,
        agent_kind: row.try_get("agent_kind").map_err(TxnFail::from)?,
        local_thread_id: row.try_get("local_thread_id").map_err(TxnFail::from)?,
        local_session_id: row.try_get("local_session_id").map_err(TxnFail::from)?,
        cwd: row.try_get("cwd").map_err(TxnFail::from)?,
        model: row.try_get("model").map_err(TxnFail::from)?,
        active_turn_id: row.try_get("active_turn_id").map_err(TxnFail::from)?,
        active_message_id: row.try_get("active_message_id").map_err(TxnFail::from)?,
        close_requested: row.try_get("close_requested").map_err(TxnFail::from)?,
        last_message_at: row.try_get("last_message_at").map_err(TxnFail::from)?,
        closed_at: row.try_get("closed_at").map_err(TxnFail::from)?,
        error: row.try_get("error").map_err(TxnFail::from)?,
        created_at: row.try_get("created_at").map_err(TxnFail::from)?,
        updated_at: row.try_get("updated_at").map_err(TxnFail::from)?,
    })
}

/// Map a chat-message row (the `chat_message::COLUMNS` projection).
fn chat_message_from_row(row: &sqlx::postgres::PgRow) -> Result<AgentChatMessage, TxnFail> {
    use sqlx::Row;
    let role_raw: String = row.try_get("role").map_err(TxnFail::from)?;
    let role = AgentChatMessageRole::from_value(&role_raw).ok_or(TxnFail::Other)?;
    let status_raw: String = row.try_get("status").map_err(TxnFail::from)?;
    let status = AgentChatMessageStatus::from_value(&status_raw).ok_or(TxnFail::Other)?;
    Ok(AgentChatMessage {
        id: row.try_get("id").map_err(TxnFail::from)?,
        session_id: row.try_get("session_id").map_err(TxnFail::from)?,
        role,
        content: row.try_get("content").map_err(TxnFail::from)?,
        content_parts: row.try_get("content_parts").map_err(TxnFail::from)?,
        status,
        local_item_id: row.try_get("local_item_id").map_err(TxnFail::from)?,
        local_turn_id: row.try_get("local_turn_id").map_err(TxnFail::from)?,
        seq: row.try_get("seq").map_err(TxnFail::from)?,
        created_at: row.try_get("created_at").map_err(TxnFail::from)?,
        completed_at: row.try_get("completed_at").map_err(TxnFail::from)?,
    })
}

/// One `release_active_chats_for_runner` row (`:458-479`): lock the
/// OPEN session, skip idle rows, finalize to FAILED, clear the turn,
/// emit `chat_failed`, queue the publish + drain. The per-session
/// `atomic()` is elided into the caller's transaction. Returns
/// whether the row counted.
async fn execute_release_row(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    session_id: Uuid,
    effects: &mut Vec<ChatEffect>,
) -> Result<bool, TxnFail> {
    let lock_sql = format!(
        "SELECT {} FROM \"{}\" WHERE (\"{}\".\"id\" = $1 AND \"{}\".\"status\" = 'open') ORDER BY \"{}\".\"last_message_at\" DESC, \"{}\".\"created_at\" DESC LIMIT 1 FOR UPDATE",
        quoted_columns(chat_session::TABLE, chat_session::COLUMNS),
        chat_session::TABLE,
        chat_session::TABLE,
        chat_session::TABLE,
        chat_session::TABLE,
        chat_session::TABLE,
    );
    let row = sqlx::query(&lock_sql)
        .bind(session_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(TxnFail::from)?;
    let Some(row) = row else {
        return Ok(false);
    };
    let session = chat_session_from_row(&row)?;
    if !chat_kernel::release_row_applies(&session) {
        return Ok(false);
    }

    // `finalize_active_messages_locked`: the explicit message scoped
    // to the session, then the streaming assistant (turn-scoped
    // probe first, lazy fallback).
    let target = chat_kernel::finalize_target_message_id(None, session.active_message_id);
    let explicit = match target {
        Some(id) => {
            let lookup_sql = format!(
                "SELECT {} FROM \"{}\" INNER JOIN \"{}\" ON (\"{}\".\"session_id\" = \"{}\".\"id\") WHERE (\"{}\".\"id\" = $1 AND \"{}\".\"session_id\" = $2) ORDER BY \"{}\".\"last_message_at\" DESC, \"{}\".\"created_at\" DESC, \"{}\".\"seq\" ASC LIMIT 1",
                quoted_columns(chat_message::TABLE, chat_message::COLUMNS),
                chat_message::TABLE,
                chat_session::TABLE,
                chat_message::TABLE,
                chat_session::TABLE,
                chat_message::TABLE,
                chat_message::TABLE,
                chat_session::TABLE,
                chat_session::TABLE,
                chat_message::TABLE,
            );
            sqlx::query(&lookup_sql)
                .bind(id)
                .bind(session.id)
                .fetch_optional(&mut **tx)
                .await
                .map_err(TxnFail::from)?
                .map(|row| chat_message_from_row(&row))
                .transpose()?
        }
        None => None,
    };
    // `active_assistant_message_locked`: the turn-scoped hit wins;
    // otherwise the newest streaming assistant (the fallback stays
    // lazy — it runs only when the scoped probe missed or the
    // session has no active turn).
    let mut assistant = None;
    if !session.active_turn_id.is_empty() {
        let scoped_sql = format!(
            "SELECT {} FROM \"{}\" WHERE (\"{}\".\"role\" = '{}' AND \"{}\".\"session_id\" = $1 AND \"{}\".\"status\" = '{}' AND \"{}\".\"local_turn_id\" = $2) ORDER BY \"{}\".\"created_at\" DESC LIMIT 1",
            quoted_columns(chat_message::TABLE, chat_message::COLUMNS),
            chat_message::TABLE,
            chat_message::TABLE,
            AgentChatMessageRole::Assistant.value(),
            chat_message::TABLE,
            chat_message::TABLE,
            AgentChatMessageStatus::Streaming.value(),
            chat_message::TABLE,
            chat_message::TABLE,
        );
        assistant = sqlx::query(&scoped_sql)
            .bind(session.id)
            .bind(&session.active_turn_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(TxnFail::from)?
            .map(|row| chat_message_from_row(&row))
            .transpose()?;
    }
    if assistant.is_none() {
        let fallback_sql = format!(
            "SELECT {} FROM \"{}\" WHERE (\"{}\".\"role\" = '{}' AND \"{}\".\"session_id\" = $1 AND \"{}\".\"status\" = '{}') ORDER BY \"{}\".\"created_at\" DESC LIMIT 1",
            quoted_columns(chat_message::TABLE, chat_message::COLUMNS),
            chat_message::TABLE,
            chat_message::TABLE,
            AgentChatMessageRole::Assistant.value(),
            chat_message::TABLE,
            chat_message::TABLE,
            AgentChatMessageStatus::Streaming.value(),
            chat_message::TABLE,
        );
        assistant = sqlx::query(&fallback_sql)
            .bind(session.id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(TxnFail::from)?
            .map(|row| chat_message_from_row(&row))
            .transpose()?;
    }

    let plan = chat_kernel::plan_release_row(
        &session,
        explicit.as_ref(),
        assistant.as_ref(),
        CHAT_RELEASE_DETAIL,
        &mut Utc::now,
    );
    debug_assert!(
        !plan.close_session,
        "release rows never close (chat.py:471 has no close arm)"
    );
    for update in &plan.finalize.updates {
        sqlx::query(&format!(
            "UPDATE \"{}\" SET \"status\" = $1, \"completed_at\" = $2 WHERE \"{}\".\"id\" = $3",
            chat_message::TABLE,
            chat_message::TABLE,
        ))
        .bind(update.status.value())
        .bind(update.completed_at)
        .bind(update.id)
        .execute(&mut **tx)
        .await
        .map_err(TxnFail::from)?;
    }
    sqlx::query(&format!(
        "UPDATE \"{}\" SET \"active_turn_id\" = '', \"active_message_id\" = NULL, \"error\" = $1, \"updated_at\" = $2 WHERE \"{}\".\"id\" = $3",
        chat_session::TABLE,
        chat_session::TABLE,
    ))
    .bind(&plan.session_error)
    .bind(Utc::now())
    .bind(session.id)
    .execute(&mut **tx)
    .await
    .map_err(TxnFail::from)?;
    for event in &plan.events {
        let max: Option<i32> = sqlx::query_scalar::<_, Option<i32>>(&format!(
            "SELECT MAX(\"{}\".\"seq\") AS \"seq__max\" FROM \"{}\" WHERE \"{}\".\"session_id\" = $1",
            chat_event::TABLE,
            chat_event::TABLE,
            chat_event::TABLE,
        ))
        .bind(session.id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(TxnFail::from)?
        .flatten();
        let seq = chat_kernel::next_seq_after_max(max);
        let id: i64 = sqlx::query_scalar(&format!(
            "INSERT INTO \"{}\" (\"session_id\", \"message_id\", \"seq\", \"source_key\", \"kind\", \"payload\", \"created_at\") VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING \"id\"",
            chat_event::TABLE,
        ))
        .bind(session.id)
        .bind(event.message_id)
        .bind(seq)
        .bind(&event.source_key)
        .bind(&event.kind)
        .bind(&event.payload)
        .bind(Utc::now())
        .fetch_one(&mut **tx)
        .await
        .map_err(TxnFail::from)?;
        effects.push(ChatEffect::PublishEvent { event_id: id });
    }
    if plan.queue_drain {
        chat_kernel::drain_tasks_after_chat_release(
            Some(session.runner_id),
            Some(session.pod_id),
            &mut |effect| effects.push(effect),
        );
    }
    Ok(true)
}

/// `release_active_chats_for_runner` (`:451-480`): the id select,
/// then one elided-savepoint row each. Returns the release count.
async fn execute_chat_release(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    runner_id: Uuid,
    effects: &mut Vec<ChatEffect>,
) -> Result<usize, TxnFail> {
    let ids: Vec<Uuid> = sqlx::query_scalar(&format!(
        "SELECT \"{table}\".\"id\" FROM \"{table}\" WHERE (\"{table}\".\"runner_id\" = $1 AND \"{table}\".\"status\" = 'open' AND ({})) ORDER BY \"{table}\".\"last_message_at\" DESC, \"{table}\".\"created_at\" DESC",
        chat_kernel::ACTIVE_TURN_PREDICATE_SQL,
        table = chat_session::TABLE,
    ))
    .bind(runner_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(TxnFail::from)?;
    let mut count = 0usize;
    for session_id in ids {
        if execute_release_row(tx, session_id, effects).await? {
            count += 1;
        }
    }
    if count > 0 {
        tracing::info!(
            "released {count} stale active chat session(s) for runner {runner_id} on session open",
        );
    }
    Ok(count)
}

// ---------------------------------------------------------------------------
// Open
// ---------------------------------------------------------------------------

/// What the open transaction returns: the evicted session id, if
/// the runner had a live one.
struct OpenTxnOutcome {
    old_session_id: Option<String>,
}

/// The session-open transaction (`:168-213`): bound waits, prior
/// revoke, insert, hello, reaper, chat release, commit.
async fn open_transaction(
    pool: &PgPool,
    runner: &crate::runner_enroll::auth::RunnerAuthRow,
    body: &Map<String, Value>,
    new_sid: Uuid,
) -> Result<(OpenTxnOutcome, OpenEffects), TxnFail> {
    let mut tx = pool.begin().await.map_err(TxnFail::from)?;
    sqlx::query(&format!(
        "SET LOCAL lock_timeout = '{TXN_LOCK_TIMEOUT_MS}ms'"
    ))
    .execute(&mut *tx)
    .await
    .map_err(TxnFail::from)?;
    sqlx::query(&format!(
        "SET LOCAL statement_timeout = '{TXN_STATEMENT_TIMEOUT_MS}ms'"
    ))
    .execute(&mut *tx)
    .await
    .map_err(TxnFail::from)?;
    let prior = sqlx::query(runner_session::OPEN_PRIOR_SQL)
        .bind(runner.id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(TxnFail::from)?;
    let mut old_session_id = None;
    if let Some(row) = prior {
        let prior = runner_session_from_row(&row).map_err(TxnFail::from)?;
        old_session_id = Some(prior.id.to_string());
        sqlx::query(runner_session::REVOKE_SQL)
            .bind(Utc::now())
            .bind(REASON_EVICTED_BY_NEW_SESSION)
            .bind(prior.id)
            .execute(&mut *tx)
            .await
            .map_err(TxnFail::from)?;
    }
    // `create(… last_seen_at=now())`: the kwarg `now()` evaluates
    // first, then `auto_now_add` stamps `created_at` — two calls, in
    // that order.
    let last_seen_at = Utc::now();
    let created_at = Utc::now();
    sqlx::query(runner_session::OPEN_INSERT_SQL)
        .bind(new_sid)
        .bind(runner.id)
        .bind(runner_session::DEFAULT_PROTOCOL_VERSION)
        .bind(created_at)
        .bind(last_seen_at)
        .bind(None::<DateTime<Utc>>)
        .bind(runner_session::DEFAULT_REVOKED_REASON)
        .execute(&mut *tx)
        .await
        .map_err(TxnFail::from)?;
    execute_hello(&mut tx, runner, body).await?;
    let mut effects = OpenEffects::default();
    execute_reap_open(&mut tx, runner.id, body, &mut effects).await?;
    execute_chat_release(&mut tx, runner.id, &mut effects.chat).await?;
    tx.commit().await.map_err(TxnFail::from)?;
    Ok((OpenTxnOutcome { old_session_id }, effects))
}

/// Fire the collected post-commit effects in registration order: the
/// finalize publishes (isolated, via
/// [`drain_lifecycle_effects`]), the reaper drain (*unisolated* — a
/// failure 500s with the rows committed), then the chat pairs (each
/// isolated per its Python site, via [`drain_chat_effects`]).
async fn fire_open_effects(
    pool: &PgPool,
    state: &AppState,
    effects: OpenEffects,
) -> Result<(), Response> {
    let ports = SessionPorts::new(pool, state);
    drain_lifecycle_effects(pool, &ports, effects.lifecycle).await?;
    for effect in effects.session_drains {
        match effect {
            session_kernel::SessionEffect::RetryCancelDelivery { runner_id, message } => {
                // Isolated per its site (unreachable on the open
                // path — polls schedule these, never opens).
                if let Err(error) = ports
                    .send_to_runner(runner_id, Value::Object(message))
                    .await
                {
                    tracing::error!(
                        "{}",
                        session_kernel::cancel_retry_error_line(
                            &runner_id.to_string(),
                            &error.to_string()
                        )
                    );
                }
            }
            session_kernel::SessionEffect::CompleteProjectMoveHandoff { run_id } => {
                ports
                    .complete_project_move_handoff(run_id)
                    .await
                    .map_err(|_| server_error())?;
            }
            session_kernel::SessionEffect::DrainRunner { runner_id } => {
                ports
                    .drain_for_runner_by_id(runner_id)
                    .await
                    .map_err(|_| server_error())?;
            }
            session_kernel::SessionEffect::DrainPod { pod_id } => {
                ports
                    .drain_pod_by_id(pod_id)
                    .await
                    .map_err(|_| server_error())?;
            }
        }
    }
    drain_chat_effects(pool, &ports, effects.chat).await?;
    Ok(())
}

/// `resolve_runner_project_slug` (`session_service.py:462-470`) as
/// data: `None` when the join finds no row, the runner has no pod,
/// or the project partner is missing.
async fn resolve_project_slug(pool: &PgPool, runner_id: Uuid) -> Result<Option<String>, Response> {
    use sqlx::Row;
    let row = sqlx::query(&session_kernel::resolve_runner_slug_sql())
        .bind(runner_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| server_error())?;
    let Some(row) = row else {
        return Ok(session_kernel::resolve_slug(&session_kernel::SlugFacts {
            row_found: false,
            pod_id: None,
            project_identifier: None,
        }));
    };
    let facts = session_kernel::SlugFacts {
        row_found: true,
        pod_id: row.try_get("pod_id").map_err(|_| server_error())?,
        project_identifier: row.try_get("identifier").map_err(|_| server_error())?,
    };
    Ok(session_kernel::resolve_slug(&facts))
}

/// `build_resume_ack` (`:473-508`) as data. `run_id` echoes the
/// passed value verbatim; the `last_seq` lookup runs only in the
/// resume arm, exactly as the source's early returns dictate.
async fn open_resume_ack(
    pool: &PgPool,
    runner_id: Uuid,
    in_flight: &Value,
) -> Result<Value, Response> {
    use sqlx::Row;
    let run_id = py_str(in_flight);
    // Django's `filter(id=…)` validates before issuing SQL
    // (`ValidationError` → 500, no query); the typed bind then needs
    // no `uuid = text` operator either.
    let run_uuid: Uuid = run_id.parse().map_err(|_| server_error())?;
    let row = sqlx::query(&session_kernel::resume_ack_lookup_sql())
        .bind(run_uuid)
        .bind(runner_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| server_error())?;
    let Some(row) = row else {
        return Ok(Value::Object(session_kernel::plan_resume_ack(
            &run_id, None, None,
        )));
    };
    let status_raw: String = row.try_get("status").map_err(|_| server_error())?;
    let status = AgentRunStatus::from_value(&status_raw).ok_or_else(server_error)?;
    let thread_id: String = row.try_get("thread_id").map_err(|_| server_error())?;
    let facts = session_kernel::ResumeAckFacts { status, thread_id };
    if facts.status == AgentRunStatus::CancelRequested || facts.status.is_terminal() {
        return Ok(Value::Object(session_kernel::plan_resume_ack(
            &run_id,
            Some(&facts),
            None,
        )));
    }
    let last_seq: Option<i32> = sqlx::query_scalar(session_kernel::RESUME_ACK_LAST_SEQ_SQL)
        .bind(run_uuid)
        .fetch_optional(pool)
        .await
        .map_err(|_| server_error())?;
    Ok(Value::Object(session_kernel::plan_resume_ack(
        &run_id,
        Some(&facts),
        last_seq.map(i64::from),
    )))
}

/// `build_session_open_redeliver` (`:400-451`) as data: the cancel
/// scan first, the assign scan only when it finds nothing.
async fn open_redeliver(
    pool: &PgPool,
    runner_id: Uuid,
    in_flight: Option<&Value>,
) -> Result<Option<Value>, Response> {
    use sqlx::Row;
    let skip = session_kernel::parse_skip_id(in_flight);
    // Parser-validated, so the re-parse is infallible; typed because
    // Postgres has no `uuid = text` operator.
    let skip_id: Option<Uuid> = skip
        .as_deref()
        .map(str::parse)
        .transpose()
        .map_err(|_| server_error())?;
    let cancel_sql = session_kernel::redeliver_cancel_sql(skip_id.is_some());
    let mut cancel = sqlx::query(&cancel_sql).bind(runner_id);
    if let Some(id) = skip_id {
        cancel = cancel.bind(id);
    }
    let cancel_row = cancel
        .fetch_optional(pool)
        .await
        .map_err(|_| server_error())?;
    let facts = if let Some(row) = cancel_row {
        session_kernel::RedeliverFacts {
            cancel_run_id: Some(row.try_get("id").map_err(|_| server_error())?),
            assign_run: None,
        }
    } else {
        let assign_sql = session_kernel::redeliver_assign_sql(skip_id.is_some());
        let mut assign = sqlx::query(&assign_sql).bind(runner_id);
        if let Some(id) = skip_id {
            assign = assign.bind(id);
        }
        let assign_row = assign
            .fetch_optional(pool)
            .await
            .map_err(|_| server_error())?;
        match assign_row {
            None => session_kernel::RedeliverFacts {
                cancel_run_id: None,
                assign_run: None,
            },
            Some(row) => {
                let run_config: Value = row.try_get("run_config").map_err(|_| server_error())?;
                let Value::Object(run_config) = run_config else {
                    return Err(server_error());
                };
                session_kernel::RedeliverFacts {
                    cancel_run_id: None,
                    assign_run: Some(session_kernel::RedeliverAssignFacts {
                        run_id: row.try_get("id").map_err(|_| server_error())?,
                        work_item_id: row.try_get("work_item_id").map_err(|_| server_error())?,
                        prompt: row.try_get("prompt").map_err(|_| server_error())?,
                        run_config,
                    }),
                }
            }
        }
    };
    Ok(session_kernel::plan_redeliver(&facts).map(Value::Object))
}

/// `POST runners/<rid>/sessions/` — open a session for one runner
/// (`sessions.py:132-285`).
///
/// Auth (D-13) precedes the view; then the protocol check, the
/// runner-id match, the body (409 before any row exists), the
/// pre-tx stream-group ensure, the transaction, the post-commit
/// effects, the Redis side effects, resume/redeliver, and the 201
/// welcome body.
pub async fn runner_session_open(
    State(state): State<AppState>,
    Path(raw_rid): Path<String>,
    req: Request,
) -> Response {
    // Resolver first (Django 404s non-converter segments before any
    // view code — and before touching the database).
    let runner_id = match parse_uuid(&raw_rid) {
        Ok(id) => id,
        Err(()) => return crate::edge::proxy(State(state), req).await,
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool.clone(),
        Err(response) => return response,
    };
    let headers = req.headers().clone();
    let secret = state.settings().secret_key.clone();
    let ring = enroll_tokens::build_key_ring(&[], &secret);
    let rid_str = runner_id.to_string();
    let auth = match authenticate_access_token(
        &pool,
        secret.as_bytes(),
        &ring,
        &headers,
        Some(rid_str.as_str()),
    )
    .await
    {
        Ok(auth) => auth,
        Err(response) => return open_denial(response).await,
    };
    let minimum = i64::from(runner_session::DEFAULT_PROTOCOL_VERSION);
    if let Some(denied) = check_protocol_header(&headers, minimum) {
        return denied;
    }
    let runner = match auth.as_ref().map(|auth| &auth.runner) {
        Some(runner) if runner.id == runner_id => runner,
        _ => return render(runner_id_mismatch_drf()),
    };
    let parsed = match read_request_data(&state, req).await {
        Ok(data) => data,
        Err(response) => return response,
    };
    // `body = request.data or {}`: a truthy non-dict reaches
    // `body.get` and 500s (`AttributeError`).
    let body = match json_cpython::to_serde_publish(&parsed) {
        Value::Object(map) => map,
        other if py_truthy(&other) => return server_error(),
        _ => Map::new(),
    };
    if let Some(slug) = body.get("project_slug") {
        if py_truthy(slug) {
            let expected = match resolve_project_slug(&pool, runner_id).await {
                Ok(expected) => expected,
                Err(response) => return response,
            };
            if let Some(expected) = expected {
                if py_str(slug) != expected {
                    return render(project_mismatch_drf(&expected));
                }
            }
        }
    }

    let redis = redis_client(&state);
    let rid_key = rid_str.clone();
    if let Err(response) = side_effect(
        runner_id,
        "ensure_stream_group",
        session_outbox::ensure_stream_group(redis.as_ref(), &rid_key),
    )
    .await
    {
        return response;
    }

    let new_sid = Uuid::new_v4();
    let (outcome, effects) = match open_transaction(&pool, runner, &body, new_sid).await {
        Ok(outcome) => outcome,
        Err(fail) => return txn_fail_response(fail, runner_id),
    };
    if let Err(response) = fire_open_effects(&pool, &state, effects).await {
        return response;
    }

    if let Some(old) = outcome.old_session_id.as_deref() {
        if let Err(response) = side_effect(
            runner_id,
            "clear_session_marker",
            session_outbox::clear_session_marker(redis.as_ref(), old),
        )
        .await
        {
            return response;
        }
    }
    let new_sid_str = new_sid.to_string();
    let old_consumer = outcome
        .old_session_id
        .as_deref()
        .map(runner_keys::consumer_name);
    let new_consumer = runner_keys::consumer_name(&new_sid_str);
    side_effect_timed(
        runner_id,
        "claim_pending_for_new_session",
        session_outbox::claim_pending_for_new_session(
            redis.as_ref(),
            &rid_key,
            old_consumer.as_deref(),
            &new_consumer,
            CLAIM_MIN_IDLE_MS,
        ),
    )
    .await;
    if let Err(response) = side_effect(
        runner_id,
        "publish_session_eviction",
        session_outbox::publish_session_eviction(
            redis.as_ref(),
            &rid_key,
            outcome.old_session_id.as_deref(),
            &new_sid_str,
        ),
    )
    .await
    {
        return response;
    }
    if let Err(response) = side_effect(
        runner_id,
        "drain_offline_into_live",
        session_outbox::drain_offline_into_live(redis.as_ref(), &rid_key),
    )
    .await
    {
        return response;
    }

    let in_flight = body.get("in_flight_run");
    let resume_ack = match in_flight {
        Some(value) if py_truthy(value) => match open_resume_ack(&pool, runner_id, value).await {
            Ok(ack) => Some(ack),
            Err(response) => return response,
        },
        _ => None,
    };
    let redeliver = match open_redeliver(&pool, runner_id, in_flight).await {
        Ok(redeliver) => redeliver,
        Err(response) => return response,
    };

    let latest = state
        .settings()
        .runner
        .latest_runner_version
        .as_deref()
        .filter(|version| !version.is_empty());
    let min = state
        .settings()
        .runner
        .min_runner_version
        .as_deref()
        .filter(|version| !version.is_empty());
    render(runner_open_201(
        &new_sid_str,
        &rid_str,
        &django_isoformat(Utc::now()),
        state.settings().runner.long_poll_interval_secs,
        minimum,
        latest,
        min,
        resume_ack.as_ref(),
        redeliver.as_ref(),
    ))
}

// ---------------------------------------------------------------------------
// Delete
// ---------------------------------------------------------------------------

/// `DELETE runners/<rid>/sessions/<sid>/` — clean shutdown
/// (`sessions.py:288-313`): 403 on mismatch; a missing row still
/// clears the marker and 204s; otherwise revoke `clean_shutdown`,
/// mark the runner offline, clear the marker, 204. Each statement
/// autocommits, exactly as the source's transactionless view. The
/// body is never read, so even malformed JSON still 204s.
pub async fn runner_session_delete(
    State(state): State<AppState>,
    Path((raw_rid, raw_sid)): Path<(String, String)>,
    req: Request,
) -> Response {
    // Either segment off-converter matches no Django route (proxied,
    // like the open path above).
    let (runner_id, sid) = match (parse_uuid(&raw_rid), parse_uuid(&raw_sid)) {
        (Ok(runner_id), Ok(sid)) => (runner_id, sid),
        _ => return crate::edge::proxy(State(state), req).await,
    };
    let headers = req.headers().clone();
    let pool = match pool_of(&state) {
        Ok(pool) => pool.clone(),
        Err(response) => return response,
    };
    let secret = state.settings().secret_key.clone();
    let ring = enroll_tokens::build_key_ring(&[], &secret);
    let rid_str = runner_id.to_string();
    let auth = match authenticate_access_token(
        &pool,
        secret.as_bytes(),
        &ring,
        &headers,
        Some(rid_str.as_str()),
    )
    .await
    {
        Ok(auth) => auth,
        Err(response) => return open_denial(response).await,
    };
    match auth.as_ref().map(|auth| &auth.runner) {
        Some(runner) if runner.id == runner_id => {}
        _ => return render(runner_id_mismatch_drf()),
    }

    let redis = redis_client(&state);
    // Canonical (lowercase) rendering: the marker key must match what
    // the open/poll paths write however the URL spelled the id.
    let sid_str = sid.to_string();
    let row = match sqlx::query(runner_session::DELETE_GET_SQL)
        .bind(sid)
        .bind(runner_id)
        .fetch_optional(&pool)
        .await
    {
        Ok(row) => row,
        Err(_) => return server_error(),
    };
    if row.is_none() {
        if session_outbox::clear_session_marker(redis.as_ref(), &sid_str)
            .await
            .is_err()
        {
            return server_error();
        }
        return StatusCode::NO_CONTENT.into_response();
    }
    if sqlx::query(runner_session::REVOKE_SQL)
        .bind(Utc::now())
        .bind(REASON_CLEAN_SHUTDOWN)
        .bind(sid)
        .execute(&pool)
        .await
        .is_err()
    {
        return server_error();
    }
    if sqlx::query(session_kernel::MARK_RUNNER_OFFLINE_SQL)
        .bind(runner_id)
        .execute(&pool)
        .await
        .is_err()
    {
        return server_error();
    }
    if session_outbox::clear_session_marker(redis.as_ref(), &sid_str)
        .await
        .is_err()
    {
        return server_error();
    }
    StatusCode::NO_CONTENT.into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    static FIXTURE: &str =
        include_str!("../../../../fixtures/runner_sessions/fx-rses-02-shapes.json");

    fn fixture() -> Value {
        serde_json::from_str(FIXTURE).expect("fixture parses")
    }

    fn headers_with_proto(value: Option<&str>) -> HeaderMap {
        let mut headers = HeaderMap::new();
        if let Some(value) = value {
            headers.insert(
                "x-runner-protocol-version",
                value.parse().expect("header value"),
            );
        }
        headers
    }

    async fn body_text(response: Response) -> (StatusCode, String) {
        let status = response.status();
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("body collects")
            .to_bytes();
        (status, String::from_utf8(bytes.to_vec()).expect("utf8"))
    }

    /// `protocol_version` follows CPython `int()`: signs, single
    /// underscores between digits, bignums by sign; anything else is
    /// not a version.
    #[test]
    fn protocol_version_matrix() {
        assert_eq!(protocol_version("4"), Some(4));
        assert_eq!(protocol_version("99"), Some(99));
        assert_eq!(protocol_version("1"), Some(1));
        assert_eq!(protocol_version("+4"), Some(4));
        assert_eq!(protocol_version("-1"), Some(-1));
        assert_eq!(protocol_version("0"), Some(0));
        // `int()` accepts single underscores between digits.
        assert_eq!(protocol_version("4_0"), Some(40));
        assert_eq!(protocol_version("1_2_3"), Some(123));
        assert_eq!(protocol_version("4__0"), None);
        assert_eq!(protocol_version("_4"), None);
        assert_eq!(protocol_version("4_"), None);
        assert_eq!(protocol_version("_"), None);
        assert_eq!(protocol_version("+"), None);
        assert_eq!(protocol_version("-"), None);
        assert_eq!(protocol_version(""), None);
        assert_eq!(protocol_version("abc"), None);
        assert_eq!(protocol_version("0x4"), None);
        assert_eq!(protocol_version("4.0"), None);
        assert_eq!(protocol_version(" 4"), None);
        assert_eq!(protocol_version("4 "), None);
        // Bignums decide by sign (out of `i64` range either way).
        assert_eq!(protocol_version(&"9".repeat(30)), Some(i64::MAX));
        assert_eq!(
            protocol_version(&format!("-{}", "9".repeat(30))),
            Some(i64::MIN)
        );
        assert_eq!(protocol_version(&"9".repeat(19)), Some(i64::MAX));
        assert_eq!(protocol_version("9223372036854775807"), Some(i64::MAX));
        // Just over `i64::MAX` still parses as bignum-positive.
        assert_eq!(protocol_version("9223372036854775808"), Some(i64::MAX));
    }

    /// `check_protocol_header` replays the fixture matrix: missing /
    /// blank / current / future pass, old and non-numeric 426 with
    /// the exact body.
    #[tokio::test]
    async fn protocol_header_replays_fixture() {
        let fx = fixture();
        let proto = fx.get("protocol_header").expect("protocol_header");
        assert!(proto.get("missing").expect("missing").is_null());
        assert!(proto.get("current_4").expect("current_4").is_null());
        assert!(proto.get("future_99").expect("future_99").is_null());
        assert!(proto.get("blank").expect("blank").is_null());

        assert!(check_protocol_header(&headers_with_proto(None), 4).is_none());
        assert!(check_protocol_header(&headers_with_proto(Some("")), 4).is_none());
        assert!(check_protocol_header(&headers_with_proto(Some("   ")), 4).is_none());
        assert!(check_protocol_header(&headers_with_proto(Some("4")), 4).is_none());
        assert!(check_protocol_header(&headers_with_proto(Some("99")), 4).is_none());

        for key in ["old_1", "non_numeric", "via_open_old"] {
            let case = proto.get(key).expect(key);
            let response = check_protocol_header(
                &headers_with_proto(Some(if key == "non_numeric" { "abc" } else { "1" })),
                4,
            )
            .expect("426s");
            let (status, body) = body_text(response).await;
            assert_eq!(
                status.as_u16(),
                case["status"].as_u64().expect("status") as u16
            );
            let expected: Value = serde_json::from_value(case["body"].clone()).expect("body");
            let actual: Value = serde_json::from_str(&body).expect("parses");
            assert_eq!(actual, expected, "{key}");
        }
    }

    /// `py_str` vectors: scalar spellings, float exponents, and
    /// single-quote container reprs.
    #[test]
    fn py_str_vectors() {
        assert_eq!(py_str(&Value::Null), "None");
        assert_eq!(py_str(&Value::Bool(true)), "True");
        assert_eq!(py_str(&Value::Bool(false)), "False");
        assert_eq!(py_str(&serde_json::json!(5)), "5");
        assert_eq!(py_str(&serde_json::json!(-42)), "-42");
        assert_eq!(py_str(&serde_json::json!("CT00003")), "CT00003");
        assert_eq!(py_str(&serde_json::json!(1.5)), "1.5");
        assert_eq!(py_str(&serde_json::json!(1e16)), "1e+16");
        assert_eq!(
            py_str(&serde_json::json!([1, "a", true, null])),
            "[1, 'a', True, None]"
        );
        assert_eq!(py_str(&serde_json::json!({"k": 1})), "{'k': 1}");
        assert_eq!(py_repr_str("it\"s"), "'it\"s'");
        assert_eq!(py_repr_str("it's"), "\"it's\"");
        assert_eq!(py_repr_str("a'b\"c"), "'a\\'b\"c'");
    }

    /// The 401 re-render: the pre-718 capital-`Detail` denial and the
    /// fixed lowercase one both come back lowercase-compact with the
    /// `Bearer` challenge; 500s and unknown shapes pass through.
    #[tokio::test]
    async fn open_denial_rerenders_lowercase() {
        let fx = fixture();
        // `open_401_dispatch.body_bytes` pins the exact wire bytes.
        let wire = fx["open_401_dispatch"]["body_bytes"]
            .as_str()
            .expect("bytes");
        for body in [
            r#"{"Detail":"access_token_malformed"}"#.to_owned(),
            r#"{"detail":"access_token_malformed"}"#.to_owned(),
        ] {
            let denial = Response::builder()
                .status(StatusCode::UNAUTHORIZED)
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::WWW_AUTHENTICATE, AUTHENTICATE_HEADER_BEARER)
                .body(axum::body::Body::from(body))
                .expect("denial builds");
            let rendered = open_denial(denial).await;
            assert_eq!(rendered.status(), StatusCode::UNAUTHORIZED);
            let (status, text) = body_text(rendered).await;
            assert_eq!(status, StatusCode::UNAUTHORIZED);
            assert_eq!(text, wire);
        }
        // Challenge preserved.
        let denial = Response::builder()
            .status(StatusCode::UNAUTHORIZED)
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::WWW_AUTHENTICATE, AUTHENTICATE_HEADER_BEARER)
            .body(axum::body::Body::from(r#"{"Detail":"runner_id_mismatch"}"#))
            .expect("denial builds");
        let rendered = open_denial(denial).await;
        assert_eq!(
            rendered
                .headers()
                .get(header::WWW_AUTHENTICATE)
                .expect("challenge")
                .to_str()
                .expect("ascii"),
            AUTHENTICATE_HEADER_BEARER
        );
        // 500s pass through untouched.
        let failure = server_error();
        let status = failure.status();
        let rendered = open_denial(failure).await;
        assert_eq!(rendered.status(), status);
        // Unknown shapes pass through untouched.
        let odd = Response::builder()
            .status(StatusCode::UNAUTHORIZED)
            .header(header::CONTENT_TYPE, "application/json")
            .body(axum::body::Body::from(r#"{"weird":true}"#))
            .expect("denial builds");
        let (_, text) = body_text(open_denial(odd).await).await;
        assert_eq!(text, r#"{"weird":true}"#);
    }

    /// The 503 classifier: only the two `_bound_txn_waits` codes map
    /// to the convoy guard; the 503 renders the fixture body.
    #[test]
    fn timeout_code_matrix() {
        assert!(is_timeout_code(Some("55P03")));
        assert!(is_timeout_code(Some("57014")));
        assert!(!is_timeout_code(Some("23505")));
        assert!(!is_timeout_code(Some("22P02")));
        assert!(!is_timeout_code(Some("40001")));
        assert!(!is_timeout_code(None));
    }

    #[tokio::test]
    async fn txn_fail_responses_replay_fixture() {
        let fx = fixture();
        let error_503 = fx.get("error_503_runner_state_locked").expect("503");
        let (status, body) = body_text(txn_fail_response(TxnFail::Timeout, Uuid::nil())).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        let expected: Value = serde_json::from_value(error_503["body"].clone()).expect("body");
        assert_eq!(
            serde_json::from_str::<Value>(&body).expect("parses"),
            expected
        );
        let (status, _) = body_text(txn_fail_response(TxnFail::Other, Uuid::nil())).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    }

    /// `parse_uuid` emulates the `<uuid:>` converter (lowercase hex
    /// only); anything else errs and the handlers proxy to Django.
    #[test]
    fn parse_uuid_rejects_non_converter() {
        let id = "28a83b35-9dd5-42bc-9d94-d1a4cf880fb1";
        assert_eq!(parse_uuid(id).expect("parses").to_string(), id);
        for bad in [
            "28A83B35-9DD5-42BC-9D94-D1A4CF880FB1",
            "nope",
            "urn:uuid:28a83b35-9dd5-42bc-9d94-d1a4cf880fb1",
            "{28a83b35-9dd5-42bc-9d94-d1a4cf880fb1}",
            "28a83b359dd542bc9d94d1a4cf880fb1",
            "",
        ] {
            assert_eq!(parse_uuid(bad), Err(()), "{bad}");
        }
    }

    /// The `SET LOCAL` statements pin the fixture bytes exactly.
    #[test]
    fn bound_txn_waits_sql_replays_fixture() {
        let fx = fixture();
        let waits = fx.get("bound_txn_waits").expect("bound_txn_waits");
        let sql = waits["sql"].as_array().expect("sql");
        assert_eq!(
            sql[0].as_str().expect("lock"),
            format!("SET LOCAL lock_timeout = '{TXN_LOCK_TIMEOUT_MS}ms'")
        );
        assert_eq!(
            sql[1].as_str().expect("stmt"),
            format!("SET LOCAL statement_timeout = '{TXN_STATEMENT_TIMEOUT_MS}ms'")
        );
    }

    /// `django_isoformat` drops the fraction iff zero (CPython
    /// `datetime.isoformat`).
    #[test]
    fn isoformat_autosi() {
        use chrono::TimeZone;
        let whole = Utc.with_ymd_and_hms(2026, 10, 2, 22, 22, 34).unwrap();
        assert_eq!(django_isoformat(whole), "2026-10-02T22:22:34+00:00");
        let frac = whole + chrono::Duration::microseconds(118_933);
        assert_eq!(django_isoformat(frac), "2026-10-02T22:22:34.118933+00:00");
    }
}
