//! Runner long-poll endpoint (D-14, stage 5).
//!
//! Port of `apps/api/pi_dash/runner/views/sessions.py:315-601`
//! (PIDASHCONV-558): `POST runners/<rid>/sessions/<sid>/poll` —
//! authenticate the runner outside DRF, parse the raw body, run the
//! sync bookkeeping (session checks, heartbeat transaction, reaper,
//! live-state upsert, acks, drain scheduling), then await messages
//! with eviction awareness and answer the 200 envelope.
//!
//! * [`runner_session_poll`]: the plain async view (`:541-601`) —
//!   405/401/403/400 handling, error-plan mapping, eviction→409,
//!   the PEL-drain mark, the 200 envelope.
//! * Heartbeat transaction (`_poll_bookkeeping`, `:330-459`): the
//!   prior-snapshot read, stale/available/drain-trigger computation,
//!   `runner_updates`, the default-path reaper, 503 on lock timeout.
//! * [`read_with_eviction_awareness`]
//!   (`_aread_with_eviction_awareness`, `:462-538`): the nonblocking
//!   PEL-replay path, the eviction-channel subscribe + deadline-sliced
//!   read loop, `_SessionEvictedDuringPoll` on a pubsub message.
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
//! The 401 re-renders the D-13 denial locally (the PIDASHCONV-590 /
//! machine.rs precedent): `auth.rs` still emits a capital `Detail`
//! key until PIDASHCONV-718 lands, while the hand-built poll
//! `JsonResponse` uses lowercase `detail`, spaced, with no
//! `WWW-Authenticate` challenge. Both casings are accepted back out
//! of the denial body so the 718 fix heals rather than breaks this
//! path. 500s pass through either way. The URL runner id passed to
//! auth is the canonical segment rendering (Django's `<uuid:>`
//! converter compares canonical `str()` too).
//!
//! # `on_commit` order
//!
//! `transaction.on_commit` fires registrations in order on commit —
//! or immediately when no transaction is active. The heartbeat commit
//! fires the reap registrations first (the cancel retry, then one
//! publish pair per reaped run in reap order), then
//! `_drain_after_commit` (handoffs, the runner drain, the pod
//! drains). The poll-ready drain registers with no active
//! transaction, so it fires synchronously at the end of bookkeeping.
//! The retry registers before the reap early-return gate, so it fires
//! even when nothing was reaped or barriered.
//!
//! # Ported bugs (translate, don't redesign; also listed in the PR)
//!
//! * BUG-poll-slice-never-block-0 (`:506-523`): the loop bails before
//!   calling Redis once the deadline expired — `BLOCK 0` with `>`
//!   would park the request indefinitely.
//! * BUG-poll-first-replay-vs-block (`:432-434, :458`): the first
//!   poll for a session replays the PEL (`0`, nonblocking) while
//!   later polls block on `>`; the PEL-drained marker selects.
//! * BUG-poll-single-trigger-drain (`:441-445`): one trigger —
//!   stale-heartbeat recovery, became-available, or first poll — so a
//!   first poll with no prior heartbeat does not double-dispatch.
//! * BUG-poll-free-worktrees-ignored (`:391-395`): the reported
//!   `free_worktrees` hint is never persisted (retired pool).
//! * BUG-poll-live-state-swallow (`:420-426`): the live-state upsert
//!   swallows every failure (log only) so a malformed snapshot never
//!   breaks the poll.
//! * BUG-poll-nondict-ignored (`:576-577`): a well-formed non-dict
//!   JSON body is silently treated as `{}`.
//! * BUG-poll-fraction-omitted: `server_time` drops the fractional
//!   part when the microsecond is 0 (plain `.isoformat()`); matched
//!   with `AutoSi` over microsecond-truncated stamps.
//! * BUG-reap-offline-drain-500 (TRACE bug 2): the reaper's
//!   `_drain_after_commit` is unguarded — an ONLINE-but-sessionless
//!   dispatch raises out of the commit (the row stays `ASSIGNED`)
//!   and the poll 500s. The poll-ready drain *is* wrapped.
//! * BUG-reap-pod-order-arbitrary: `pod_ids` is a `set` in Python;
//!   Rust drains first-seen order (reaped, then stopped cancels),
//!   deterministic for the same rows.
//!
//! # Approximations (no contract input covers them)
//!
//! * A 500 body is the shared runner JSON; Django renders its HTML
//!   error page on these paths. Contract tests pin the 500 status
//!   only.
//! * Integer/float ack ids render via `to_string` (CPython `repr()`
//!   spelling may differ for exotic magnitudes; the status never
//!   does). A `true` item and nested items 500 like the source's
//!   redis-py `DataError`; `false` items drop out via the `if sid`
//!   filter on both sides.
//! * A nested `NaN`/`Infinity` inside an otherwise-valid poll body
//!   400s; only a top-level one takes the source's ignore-as-non-dict
//!   path (`serde_json` has no lenient mode).
//! * Live-state text columns bind exotic JSON scalars (arrays,
//!   objects, non-integral floats on integer columns) in Rust
//!   spelling; only strings, integers, and booleans round-trip
//!   exactly (booleans via CPython truthiness, matching Django's
//!   `bool()` coercion).
//! * The per-`finalize_agent_run` nested `atomic()` (a savepoint) is
//!   elided into the heartbeat transaction: any finalize error aborts
//!   the outer transaction identically, so the savepoint is
//!   unobservable.
//!
//! Fixture: `rust-api/fixtures/runner_sessions/fx-rses-02-shapes.json`
//! (FX-RSES-02, poll sections); the `#[cfg(test)]` suites replay the
//! handler-owned branches (auth/body/plan/slices, the heartbeat
//! decision table, the 503 predicate, the poll 401 re-render, route
//! registration).

// Every handler returns a fully-rendered `Response` by design (the
// runner `run_endpoints` precedent, which carries the same allow).
#![allow(clippy::result_large_err)]

use std::time::{Duration, Instant};

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use chrono::{DateTime, Utc};
use http_body_util::BodyExt as _;
use serde_json::{Map, Value};
use uuid::Uuid;

use pidash_db::runner_enroll::columns::enums::{RUNNER_STATUS_BUSY, RUNNER_STATUS_ONLINE};
use pidash_db::runner_sessions::models::runner_session::{self, runner_session_from_row};
use pidash_db::runner_sessions::outbox as session_outbox;
use pidash_db::runner_sessions::outbox::OutboxError as SessionOutboxError;
use pidash_db::runner_sessions::RunnerSession as DbRunnerSession;
use pidash_services::runner_enroll::tokens::build_key_ring;
use pidash_services::runner_runs::finalization as finalize_kernel;
use pidash_services::runner_runs::{LifecycleEffect, SetValue};
use pidash_services::runner_sessions::drain as drain_kernel;
use pidash_services::runner_sessions::guards::{alive_threshold, heartbeat_grace};
use pidash_services::runner_sessions::pubsub as pubsub_kernel;
use pidash_services::runner_sessions::pubsub::PubsubStore;
use pidash_services::runner_sessions::session_service as session_kernel;
use pidash_types::dispatch::AgentExecutorKind;
use pidash_types::runner_runs::{AgentRunStatus, TERMINAL_RUN_STATUSES};
use pidash_types::runner_sessions::keys::runner as runner_keys;
use pidash_types::runner_sessions::{
    json_parse_error_poll, poll_200, runner_id_mismatch_poll, runner_state_locked_poll,
    session_evicted_poll, unauthorized_poll, DecodedMessage, HttpResponse,
};

use crate::runner_enroll::auth::{authenticate_access_token, ALLOW_POST};
use crate::runner_enroll::now_micros;
use crate::runner_runs::run_endpoints::drain_lifecycle_effects;
use crate::runner_runs::{json_response, pool_of, server_error, LivePorts};
use crate::state::AppState;

/// Long-poll wait slice (`_POLL_SLICE_MS`, `sessions.py:42`).
const POLL_SLICE_MS: u64 = 1000;
/// `XREADGROUP COUNT` on every poll read (`:483-488, :525-531`).
const POLL_READ_COUNT: i64 = 100;
/// Django's `custom_404_view` bytes (`app/views/error_404.py`):
/// `JsonResponse({"error": "Page not found."})`. The `<uuid:>`
/// converter 404s non-UUID segments before any view runs; axum matches
/// the segment, so the handlers answer those bytes on a bad id.
const PAGE_NOT_FOUND_BODY: &str = r#"{"error": "Page not found."}"#;
/// Lock/statement caps (`_bound_txn_waits`, `:50-71`). Neither
/// `RUNNER_TXN_*` setting exists in the settings modules, so the
/// `getattr` defaults always apply; the bytes replay the FX-RSES-02
/// `bound_txn_waits` capture verbatim.
const SET_LOCAL_LOCK_TIMEOUT_SQL: &str = "SET LOCAL lock_timeout = '5000ms'";
const SET_LOCAL_STATEMENT_TIMEOUT_SQL: &str = "SET LOCAL statement_timeout = '20000ms'";
/// Prior-heartbeat snapshot (`:375-377`):
/// `.select_for_update().filter(pk).values("last_heartbeat_at",
/// "status").get()` — the `.get()` carries `LIMIT 21`. No provider
/// const exists (D-13 owns the table, not this read), so the Django
/// queryset shape is spelled here, verified against the FX-RSES-02
/// `bookkeeping_happy.sql[5]` capture.
const HEARTBEAT_SNAPSHOT_SQL: &str = "SELECT \"runner\".\"last_heartbeat_at\", \"runner\".\"status\" FROM \"runner\" WHERE \"runner\".\"id\" = $1 LIMIT 21 FOR UPDATE";
/// Heartbeat write (`:387-396`): `.filter(pk).update(...)` preserves
/// call order (`last_heartbeat_at`, `status`); the enum literal binds
/// as a param (the planner interpolated-literals convention).
/// Verified against `bookkeeping_happy.sql[6]`.
const HEARTBEAT_UPDATE_SQL: &str =
    "UPDATE \"runner\" SET \"last_heartbeat_at\" = $1, \"status\" = $2 WHERE \"runner\".\"id\" = $3";

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
/// nonzero (the `handlers_git_repo` `AutoSi` precedent). Callers pass
/// microsecond-truncated stamps (Django renders at most 6 digits).
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

/// Python truthiness for JSON values (`None`/`False`/`0`/`""`/`[]`/`{}`
/// are falsy; everything else is truthy) — the `or []` half of
/// `body.get("ack") or []` (`:354`) and the `in_flight_run` gate
/// (`:444`).
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
/// source's redis-py `DataError` (unhandled → 500).
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

/// `list(body.get("ack") or [])` (`sessions.py:354`):
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

/// Top-level `json.loads` extensions (`:571-577`): CPython parses bare
/// `NaN`/`Infinity`/`-Infinity` (surrounding whitespace allowed) into
/// floats — non-dicts, so the poll ignores them — where `serde_json`
/// errors. Nested occurrences stay a 400 (documented approximation).
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

/// The poll body (`:570-577`): empty → `{}`; unparsable → 400;
/// well-formed non-dict → `{}` (silently ignored).
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

/// Read a D-13 denial body back into its code, returning the code plus
/// the rebuilt response. Accepts the lowercase `detail` and, tolerantly,
/// the pre-718 capital `Detail`; anything else yields `None` and the
/// caller passes the rebuilt response through untouched.
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

/// Re-render a D-13 access-token denial for the plain-Django poll
/// view: the 401 keeps its code but renders spaced without the
/// `WWW-Authenticate` challenge (the hand-built `JsonResponse`,
/// `:563-566`); 500s and unrecognized shapes pass through untouched.
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
// Heartbeat transaction (`_poll_bookkeeping`, `:362-412`)
// ---------------------------------------------------------------------------

/// The heartbeat decision (`:380-389`): staleness, availability,
/// drain eligibility, and the status the update stamps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct HeartbeatDecision {
    was_stale: bool,
    became_available: bool,
    status_allows_drain: bool,
    new_status: &'static str,
}

/// Decide from the prior snapshot (`:380-389`): stale when the prior
/// heartbeat is missing or older than [`heartbeat_grace`];
/// became-available on a `busy → idle/online` edge; the drain gate
/// opens on a reported available status or on an empty `status`
/// entry over a non-busy prior; anything but reported `busy` stamps
/// `online`.
fn heartbeat_decision(
    now: DateTime<Utc>,
    prior_hb: Option<DateTime<Utc>>,
    prior_status: &str,
    reported_status: Option<&str>,
    status_entry_empty: bool,
) -> HeartbeatDecision {
    let was_stale = prior_hb.is_none_or(|hb| now - hb > heartbeat_grace());
    let reports_busy = reported_status == Some(RUNNER_STATUS_BUSY);
    let reports_available = matches!(
        reported_status,
        Some(status) if status == "idle" || status == RUNNER_STATUS_ONLINE
    );
    HeartbeatDecision {
        was_stale,
        became_available: prior_status == RUNNER_STATUS_BUSY && reports_available,
        status_allows_drain: reports_available
            || (status_entry_empty && prior_status != RUNNER_STATUS_BUSY),
        new_status: if reports_busy {
            RUNNER_STATUS_BUSY
        } else {
            RUNNER_STATUS_ONLINE
        },
    }
}

/// Whether a heartbeat-transaction failure is the source's
/// `OperationalError` (`:399-412`, the convoy guard → 503):
/// every class psycopg raises as `OperationalError` that a live
/// backend can emit mid-transaction — connection (`08`), rollback
/// (`40`: deadlock, serialization), resources (`53`), limits (`54`),
/// prerequisite state (`55`: lock timeout), operator intervention
/// (`57`: statement timeout, shutdown), system (`58`) — plus
/// transport/pool failures. Anything else (constraint, syntax, data,
/// decode) is a plain 500.
fn is_operational_error(error: &sqlx::Error) -> bool {
    match error {
        sqlx::Error::Database(db) => db.code().is_some_and(|code| {
            const CLASSES: [&str; 7] = ["08", "40", "53", "54", "55", "57", "58"];
            CLASSES.iter().any(|class| code.as_ref().starts_with(class))
        }),
        sqlx::Error::Io(_) | sqlx::Error::PoolTimedOut | sqlx::Error::PoolClosed => true,
        _ => false,
    }
}

/// Map a heartbeat-transaction failure (`:399-412`): the convoy guard
/// answers 503 `runner_state_locked` (spaced poll bytes) so the
/// daemon backs off; anything else 500s.
fn heartbeat_error(error: sqlx::Error) -> Response {
    if is_operational_error(&error) {
        tracing::error!(%error, "runner poll bookkeeping timed out waiting on runner-scoped locks");
        render(runner_state_locked_poll())
    } else {
        server_error()
    }
}

/// Bind one finalize `SET` clause, in values order. `Now` is the
/// statement's single timestamp; `Null` binds the column's typed
/// null (the reap path nulls the queue position and the two
/// markers); `Text`/`Json` carry the bound value (the
/// `run_endpoints::bind_finalize_value` twin).
fn bind_finalize_value<'q>(
    query: sqlx::query::Query<'q, sqlx::Postgres, sqlx::postgres::PgArguments>,
    column: &str,
    value: &SetValue,
    now: &DateTime<Utc>,
) -> sqlx::query::Query<'q, sqlx::Postgres, sqlx::postgres::PgArguments> {
    match (column, value) {
        (_, SetValue::Now) => query.bind(*now),
        (_, SetValue::Text(text)) => query.bind(text.clone()),
        (_, SetValue::Json(payload)) => query.bind(payload.clone()),
        ("queue_position", SetValue::Null) => query.bind(None::<i16>),
        (_, SetValue::Null) => query.bind(None::<DateTime<Utc>>),
    }
}

/// One reaped run's `finalize_agent_run` (`reap_stale_busy_runs`
/// `:266-272` via `finalization.py:48-86`), inline in the heartbeat
/// transaction: the first-writer-wins lock (with runner, no status —
/// a miss finalizes nothing, like the source's `False`), the
/// conditional `UPDATE`, the cloud-only terminal event. Returns the
/// publish pair when the run finalized (each `finalize_agent_run`
/// registers its own `on_commit`, in reap order).
async fn finalize_reaped_run(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    run_id: Uuid,
    runner_id: Uuid,
    detail: &str,
) -> Result<Option<[LifecycleEffect; 2]>, Response> {
    let values = session_kernel::plan_reap_finalize(detail);
    let lock_sql = finalize_kernel::lock_run_for_finalize_sql(true, false);
    let mut lock = sqlx::query(&lock_sql).bind(run_id);
    for status in TERMINAL_RUN_STATUSES {
        lock = lock.bind(status.value());
    }
    let row: Option<sqlx::postgres::PgRow> = lock
        .bind(runner_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(heartbeat_error)?;
    let Some(row) = row else {
        return Ok(None);
    };
    let update_sql = finalize_kernel::finalize_update_sql(&values);
    let mut update = sqlx::query(&update_sql);
    let now = now_micros();
    for clause in &values.clauses {
        update = bind_finalize_value(update, clause.column, &clause.value, &now);
    }
    update
        .bind(run_id)
        .execute(&mut **tx)
        .await
        .map_err(heartbeat_error)?;
    // Cloud-only terminal event (`finalization.py:72-84`): the
    // executor kind reads tolerant (an unparseable stored value is
    // simply not cloud, as Django's `==` comparison is).
    let executor_kind: String =
        sqlx::Row::try_get(&row, "executor_kind").map_err(|_| server_error())?;
    if AgentExecutorKind::from_value(&executor_kind) == Some(AgentExecutorKind::CloudAgent) {
        let exists = sqlx::query_scalar::<_, i32>(finalize_kernel::terminal_event_exists_sql())
            .bind(run_id)
            .bind("terminal")
            .fetch_optional(&mut **tx)
            .await
            .map_err(heartbeat_error)?;
        if exists.is_none() {
            let max_seq =
                sqlx::query_scalar::<_, i32>(finalize_kernel::terminal_event_max_seq_sql())
                    .bind(run_id)
                    .fetch_optional(&mut **tx)
                    .await
                    .map_err(heartbeat_error)?;
            let plan = finalize_kernel::plan_terminal_event(
                max_seq,
                AgentRunStatus::Failed,
                &finalize_kernel::finalize_error_code(&values),
            );
            sqlx::query(&finalize_kernel::terminal_event_insert_sql())
                .bind(run_id)
                .bind(plan.seq)
                .bind("terminal")
                .bind(plan.payload)
                .bind(now_micros())
                .execute(&mut **tx)
                .await
                .map_err(heartbeat_error)?;
        }
    }
    Ok(Some(finalize_kernel::plan_publish_effects(run_id)))
}

/// The reap's deferred registrations, in `on_commit` order: the
/// cancel retry (registered before the early-return gate), one
/// publish pair per finalized run in reap order, then the
/// `_drain_after_commit` effects (handoffs, runner drain, pod
/// drains).
struct ReapOutcome {
    retry_cancel: Option<session_kernel::SessionEffect>,
    publish_pairs: Vec<[LifecycleEffect; 2]>,
    drain_effects: Vec<session_kernel::SessionEffect>,
}

/// `reap_stale_busy_runs(runner, status_entry)` on the poll path
/// (`session_service.py:150-290`, `exclude_redeliverable=False`),
/// executed inside the caller's heartbeat transaction: the clamp and
/// cutoff, the in-flight exclusion + cancel-retry probe, the
/// stopped-cancel barrier, the candidate scan, one inline finalize
/// per reaped run. Collects the `on_commit` registrations for the
/// caller to fire after commit.
async fn reap_stale_busy_runs(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    runner_id: Uuid,
    status_entry: &Map<String, Value>,
) -> Result<ReapOutcome, Response> {
    use session_kernel::SessionEffect;
    let now = now_micros();
    // `:172-179`: a naive `ts` raises `TypeError` in the clamp
    // comparison — not an `OperationalError`, so it 500s (the
    // transaction rolls back with the caller).
    let heartbeat_ts = match session_kernel::parse_heartbeat_ts(status_entry.get("ts"), now) {
        Ok(ts) => ts,
        Err(session_kernel::HeartbeatError::NaiveTimestamp) => return Err(server_error()),
    };
    let cutoff = session_kernel::effective_cutoff(now, heartbeat_ts);
    let in_flight = session_kernel::parse_in_flight_id(status_entry.get("in_flight_run"));
    let in_flight_id: Option<Uuid> = in_flight
        .as_deref()
        .map(str::parse)
        .transpose()
        .map_err(|_| server_error())?;
    let statuses = session_kernel::reapable_statuses(false);

    // `:211-241`: the cancel retry registers before the early-return
    // gate below, so it fires even when nothing reaps.
    let mut retry_cancel = None;
    if let Some(in_flight) = in_flight_id {
        let exists = sqlx::query_scalar::<_, i32>(session_kernel::CANCEL_REQUESTED_EXISTS_SQL)
            .bind(in_flight)
            .bind(runner_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(heartbeat_error)?;
        if session_kernel::should_schedule_cancel_retry(true, exists.is_some(), false) {
            retry_cancel = Some(SessionEffect::RetryCancelDelivery {
                runner_id,
                message: session_kernel::cancel_retry_message(&in_flight.to_string()),
            });
        }
    }

    // `:246-254`: the stopped-cancel barrier (ids + pod ids, then
    // the guarded write).
    let cancel_ids_sql = session_kernel::stale_cancel_ids_sql(&statuses, in_flight_id.is_some());
    let mut cancel_ids = sqlx::query_scalar::<_, Uuid>(&cancel_ids_sql)
        .bind(cutoff)
        .bind(runner_id);
    if let Some(in_flight) = in_flight_id {
        cancel_ids = cancel_ids.bind(in_flight);
    }
    let stopped_cancel_ids: Vec<Uuid> = cancel_ids
        .fetch_all(&mut **tx)
        .await
        .map_err(heartbeat_error)?;
    let cancel_pods_sql =
        session_kernel::stale_cancel_pod_ids_sql(&statuses, in_flight_id.is_some());
    let mut cancel_pods = sqlx::query_scalar::<_, Option<Uuid>>(&cancel_pods_sql)
        .bind(cutoff)
        .bind(runner_id);
    if let Some(in_flight) = in_flight_id {
        cancel_pods = cancel_pods.bind(in_flight);
    }
    let stopped_pod_ids: Vec<Option<Uuid>> = cancel_pods
        .fetch_all(&mut **tx)
        .await
        .map_err(heartbeat_error)?;
    if !stopped_cancel_ids.is_empty() {
        let barrier_sql = session_kernel::cancel_barrier_update_sql(stopped_cancel_ids.len());
        let mut barrier = sqlx::query(&barrier_sql).bind(now);
        for id in &stopped_cancel_ids {
            barrier = barrier.bind(*id);
        }
        barrier.execute(&mut **tx).await.map_err(heartbeat_error)?;
    }

    // `:256`: the reap candidate scan, minus barriered ids.
    let pairs_sql = session_kernel::stale_pairs_sql(
        &statuses,
        in_flight_id.is_some(),
        stopped_cancel_ids.len(),
    );
    let mut pairs = sqlx::query_as::<_, (Uuid, Option<Uuid>)>(&pairs_sql)
        .bind(cutoff)
        .bind(runner_id);
    if let Some(in_flight) = in_flight_id {
        pairs = pairs.bind(in_flight);
    }
    for id in &stopped_cancel_ids {
        pairs = pairs.bind(*id);
    }
    let reaped: Vec<(Uuid, Option<Uuid>)> =
        pairs.fetch_all(&mut **tx).await.map_err(heartbeat_error)?;
    if reaped.is_empty() && stopped_cancel_ids.is_empty() {
        return Ok(ReapOutcome {
            retry_cancel,
            publish_pairs: Vec::new(),
            drain_effects: Vec::new(),
        });
    }

    // `:260-272`: one inline finalize per reaped run.
    let detail = session_kernel::reap_error_detail(in_flight.as_deref());
    let mut publish_pairs = Vec::new();
    for (run_id, _) in &reaped {
        if let Some(pair) = finalize_reaped_run(tx, *run_id, runner_id, &detail).await? {
            publish_pairs.push(pair);
        }
    }
    let reaped_pods: Vec<Option<Uuid>> = reaped.iter().map(|(_, pod)| *pod).collect();
    let pod_ids = session_kernel::drain_pod_ids(&reaped_pods, &stopped_pod_ids);
    let drain_effects =
        session_kernel::plan_drain_after_commit(runner_id, &pod_ids, &stopped_cancel_ids);
    Ok(ReapOutcome {
        retry_cancel,
        publish_pairs,
        drain_effects,
    })
}

// ---------------------------------------------------------------------------
// Live-state upsert (`_poll_bookkeeping`, `:413-426`)
// ---------------------------------------------------------------------------

/// Whether an insert failure is a lost `get_or_create` race
/// (Django retries on `IntegrityError` only): SQLSTATE `23505`.
fn is_unique_violation(error: &sqlx::Error) -> bool {
    matches!(error, sqlx::Error::Database(db) if db.code().is_some_and(|code| code.as_ref() == "23505"))
}

/// Whether the entry carries any observability key (the `:353-357`
/// `has_snapshot` gate): `observed_run_id`, a snapshot field,
/// `tokens`, or `model`. Controls whether the `get_or_create` runs at
/// all — the planner re-derives the same gate, but the gate must run
/// before any SQL.
fn has_live_state_snapshot(status_entry: &Map<String, Value>) -> bool {
    status_entry.contains_key("observed_run_id")
        || session_kernel::SNAPSHOT_FIELDS
            .iter()
            .any(|field| status_entry.contains_key(*field))
        || status_entry.contains_key("tokens")
        || status_entry.contains_key("model")
}

/// CPython `int()` for live-state integer columns
/// (`IntegerField.to_python`): surrounding whitespace, a sign, and
/// singly-placed underscores are accepted; anything else fails the
/// upsert (as `ValueError`/`TypeError` fails the `save()`). Past
/// `i64` the parse fails, which fails the upsert — the same outcome
/// as Django's `save()` hitting the column overflow.
fn parse_live_int(text: &str) -> Option<i64> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Ok(int) = trimmed.parse::<i64>() {
        return Some(int);
    }
    let digits = trimmed.strip_prefix(['+', '-']).unwrap_or(trimmed);
    if digits.is_empty()
        || digits.starts_with('_')
        || digits.ends_with('_')
        || digits.contains("__")
        || !digits.bytes().all(|b| b.is_ascii_digit() || b == b'_')
    {
        return None;
    }
    let mut signed = String::with_capacity(digits.len() + 1);
    if trimmed.starts_with('-') {
        signed.push('-');
    }
    signed.push_str(&digits.replace('_', ""));
    signed.parse::<i64>().ok()
}

/// `BooleanField.to_python` for the nullable `agent_subprocess_alive`
/// column: `None` is invalid (fails the upsert, as `ValidationError`
/// fails the `save()`); `Some(None)` is `NULL` (`null=True`, so the
/// empty values are `NULL`, not `False`); `Some(Some(flag))` binds.
fn live_bool_from_text(text: &str) -> Option<Option<bool>> {
    match text {
        "" => Some(None),
        "t" | "True" | "1" => Some(Some(true)),
        "f" | "False" | "0" => Some(Some(false)),
        _ => None,
    }
}

/// `BooleanField.to_python` for JSON values: `1`/`1.0` are `True`
/// and `0`/`0.0` are `False` (`value in (True, False)` is `==`);
/// `[]`/`{}`/`null` are `NULL` (`empty_values`); anything else is
/// invalid.
fn live_bool_from_json(value: &Value) -> Option<Option<bool>> {
    match value {
        Value::Bool(flag) => Some(Some(*flag)),
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                return match int {
                    1 => Some(Some(true)),
                    0 => Some(Some(false)),
                    _ => None,
                };
            }
            if let Some(uint) = number.as_u64() {
                return match uint {
                    1 => Some(Some(true)),
                    0 => Some(Some(false)),
                    _ => None,
                };
            }
            match number.as_f64() {
                Some(1.0) => Some(Some(true)),
                Some(0.0) => Some(Some(false)),
                _ => None,
            }
        }
        Value::String(text) => live_bool_from_text(text),
        Value::Array(items) if items.is_empty() => Some(None),
        Value::Object(map) if map.is_empty() => Some(None),
        Value::Null => Some(None),
        _ => None,
    }
}

/// `DateTimeField.to_python` + the `USE_TZ` naive rule (`TIME_ZONE`
/// is `UTC`): ISO datetimes (`T` or space separator, `Z` or numeric
/// offset, seconds and fraction optional), naive values assumed UTC.
/// Garbage (and impossible dates) fails the upsert, as
/// `ValidationError` fails the `save()`. Truncated to microseconds
/// (`parse_datetime` keeps six fraction digits; the column is
/// `timestamptz`). The year is exactly four digits, as the source
/// regex requires — which also keeps the micros conversion in range.
fn parse_live_datetime(text: &str) -> Option<DateTime<Utc>> {
    fn truncate(dt: DateTime<Utc>) -> Option<DateTime<Utc>> {
        DateTime::from_timestamp_micros(dt.timestamp_micros())
    }
    let trimmed = text.trim();
    let head = trimmed.as_bytes();
    if head.len() < 5 || head[4] != b'-' || !head[..4].iter().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if let Ok(aware) = DateTime::parse_from_rfc3339(trimmed) {
        return truncate(aware.to_utc());
    }
    // `parse_datetime` also takes a space separator and `%z`-style
    // offsets (`+HHMM`, `+HH`): normalize to RFC 3339, retry once.
    let mut normalized = trimmed.replacen(' ', "T", 1);
    let bytes = normalized.as_bytes();
    let mut sign_at = None;
    for (i, b) in bytes.iter().enumerate().skip(11) {
        if *b == b'+' || *b == b'-' {
            sign_at = Some(i);
        }
    }
    if let Some(i) = sign_at {
        let tail = &normalized[i + 1..];
        if tail.len() == 4 && tail.bytes().all(|b| b.is_ascii_digit()) {
            normalized = format!("{}{}:{}", &normalized[..i + 1], &tail[..2], &tail[2..]);
        } else if tail.len() == 2 && tail.bytes().all(|b| b.is_ascii_digit()) {
            normalized = format!("{normalized}:00");
        }
        if let Ok(aware) = DateTime::parse_from_rfc3339(&normalized) {
            return truncate(aware.to_utc());
        }
    }
    const NAIVE: [&str; 4] = [
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%dT%H:%M",
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%d %H:%M",
    ];
    for format in NAIVE {
        if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(trimmed, format) {
            return truncate(naive.and_utc());
        }
    }
    None
}

/// Bind one live-state `SET` value (`:383-394` — `Null` renders
/// inline, so only `Text`/`Json` arrive here): Django field
/// adaptation per column. Text columns take `str()` (strings bind
/// verbatim; numbers/bools render; nested values render compact —
/// documented residual); integer columns take `int()`; the boolean
/// column takes `BooleanField.to_python` (nullable: `""` is `NULL`);
/// datetimes parse ISO (naive assumed UTC); uuids bind native;
/// `usage` binds JSON. `None` means Django raised
/// (`ValueError`/`TypeError`) — the caller swallows the upsert.
/// Typed columns must bind native values: Postgres has no `text`
/// assignment cast for `uuid`/`timestamptz`, so binding the raw
/// string fails the `UPDATE` where Django's `save()` stores it.
fn bind_live_state_value<'q>(
    query: sqlx::query::Query<'q, sqlx::Postgres, sqlx::postgres::PgArguments>,
    column: &str,
    value: &SetValue,
) -> Option<sqlx::query::Query<'q, sqlx::Postgres, sqlx::postgres::PgArguments>> {
    match column {
        "observed_run_id" => match value {
            SetValue::Text(text) | SetValue::Json(Value::String(text)) => {
                Uuid::parse_str(text).ok().map(|id| query.bind(id))
            }
            _ => None,
        },
        "last_event_at" => match value {
            SetValue::Text(text) | SetValue::Json(Value::String(text)) => {
                parse_live_datetime(text).map(|dt| query.bind(dt))
            }
            _ => None,
        },
        "agent_pid" | "approvals_pending" | "turn_count" => match value {
            SetValue::Text(text) | SetValue::Json(Value::String(text)) => {
                parse_live_int(text).map(|int| query.bind(int))
            }
            SetValue::Json(Value::Number(number)) => {
                if let Some(int) = number.as_i64() {
                    Some(query.bind(int))
                } else if let Some(uint) = number.as_u64() {
                    uint.try_into().ok().map(|int: i64| query.bind(int))
                } else {
                    number
                        .as_f64()
                        .map(|float| query.bind(float.trunc() as i64))
                }
            }
            SetValue::Json(Value::Bool(flag)) => Some(query.bind(i64::from(*flag))),
            _ => None,
        },
        "agent_subprocess_alive" => match value {
            SetValue::Text(text) => match live_bool_from_text(text) {
                Some(Some(flag)) => Some(query.bind(flag)),
                Some(None) => Some(query.bind(None::<bool>)),
                None => None,
            },
            SetValue::Json(payload) => match live_bool_from_json(payload) {
                Some(Some(flag)) => Some(query.bind(flag)),
                Some(None) => Some(query.bind(None::<bool>)),
                None => None,
            },
            _ => None,
        },
        "usage" => match value {
            SetValue::Json(payload) => Some(query.bind(payload.clone())),
            _ => None,
        },
        _ => match value {
            SetValue::Text(text) => Some(query.bind(text.clone())),
            SetValue::Json(payload) => match payload {
                Value::String(text) => Some(query.bind(text.clone())),
                Value::Number(number) => Some(query.bind(number.to_string())),
                Value::Bool(true) => Some(query.bind("True".to_owned())),
                Value::Bool(false) => Some(query.bind("False".to_owned())),
                // CPython `str()` of a nested value (single quotes,
                // `True`/`False`, `', '` separators) — the compact
                // JSON rendering is a documented residual.
                Value::Array(_) | Value::Object(_) => Some(query.bind(payload.to_string())),
                Value::Null => None,
            },
            SetValue::Null | SetValue::Now => None,
        },
    }
}

/// `upsert_runner_live_state(runner, status_entry)`
/// (`session_service.py:336-397`), fully swallowed: the source wraps
/// the whole call in `try/except Exception` (log only), so every
/// failure here logs and returns. The `get_or_create` selects, then
/// inserts on a miss (a lost insert race re-selects, as Django's
/// `get_or_create` does); the plan then updates, warns-and-skips, or
/// no-ops.
async fn upsert_runner_live_state(
    pool: &sqlx::postgres::PgPool,
    runner_id: Uuid,
    status_entry: &Map<String, Value>,
) {
    if status_entry.is_empty() || !has_live_state_snapshot(status_entry) {
        return;
    }
    let outcome: Result<(), ()> = async {
        let row: Option<sqlx::postgres::PgRow> = sqlx::query(session_kernel::LIVE_STATE_SELECT_SQL)
            .bind(runner_id)
            .fetch_optional(pool)
            .await
            .map_err(|_| ())?;
        let observed: Option<Uuid> = match row {
            Some(row) => sqlx::Row::try_get(&row, "observed_run_id").map_err(|_| ())?,
            None => {
                match sqlx::query(session_kernel::LIVE_STATE_INSERT_SQL)
                    .bind(runner_id)
                    .bind(now_micros())
                    .execute(pool)
                    .await
                {
                    Ok(_) => None,
                    Err(error) if is_unique_violation(&error) => {
                        // Lost `get_or_create` race: re-select the
                        // winner's row (a second miss swallows, as
                        // Django's `DoesNotExist` does).
                        let row: Option<sqlx::postgres::PgRow> =
                            sqlx::query(session_kernel::LIVE_STATE_SELECT_SQL)
                                .bind(runner_id)
                                .fetch_optional(pool)
                                .await
                                .map_err(|_| ())?;
                        let Some(row) = row else {
                            return Err(());
                        };
                        sqlx::Row::try_get(&row, "observed_run_id").map_err(|_| ())?
                    }
                    Err(_) => return Err(()),
                }
            }
        };
        let facts = session_kernel::LiveStateFacts {
            observed_run_id: observed,
        };
        match session_kernel::plan_live_state_upsert(&facts, Some(status_entry), &runner_id) {
            session_kernel::LiveStatePlan::Noop | session_kernel::LiveStatePlan::NoChange => {}
            session_kernel::LiveStatePlan::SkippedInvalidRunId { warning } => {
                tracing::warn!("{warning}");
            }
            session_kernel::LiveStatePlan::Update { set_clauses } => {
                let update_sql = session_kernel::live_state_update_sql(&set_clauses);
                let mut update = sqlx::query(&update_sql);
                for clause in &set_clauses {
                    if matches!(clause.value, SetValue::Null) {
                        continue;
                    }
                    update =
                        bind_live_state_value(update, clause.column, &clause.value).ok_or(())?;
                }
                update
                    .bind(now_micros())
                    .bind(runner_id)
                    .execute(pool)
                    .await
                    .map_err(|_| ())?;
            }
        }
        Ok(())
    }
    .await;
    if outcome.is_err() {
        tracing::error!("upsert_runner_live_state failed for runner {runner_id}");
    }
}

// ---------------------------------------------------------------------------
// Drains (`matcher.py:194-303`, the `drain.rs` recipes, verbatim)
// ---------------------------------------------------------------------------

/// Pool-backed [`PubsubStore`] (the `runner_enroll::teardown` twin):
/// Redis verbs through the api-crate-owned client, SQL verbs on the
/// pool. The poll path only drives `enqueue_for_runner`; the close
/// verbs ride the same provider calls so the seam stays complete.
struct PollPubsubStore<'p, 's> {
    pool: &'p sqlx::postgres::PgPool,
    state: &'s AppState,
}

impl<'p, 's> PollPubsubStore<'p, 's> {
    fn new(pool: &'p sqlx::postgres::PgPool, state: &'s AppState) -> Self {
        Self { pool, state }
    }
}

impl PubsubStore for PollPubsubStore<'_, '_> {
    async fn enqueue_for_runner(
        &self,
        runner_id: Uuid,
        message: &Map<String, Value>,
    ) -> Result<Option<String>, SessionOutboxError> {
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
    ) -> Result<Vec<DbRunnerSession>, SessionOutboxError> {
        let rows = sqlx::query(pubsub_kernel::CLOSE_ACTIVE_SESSIONS_SQL)
            .bind(runner_id)
            .fetch_all(self.pool)
            .await
            .map_err(SessionOutboxError::Db)?;
        rows.iter()
            .map(runner_session_from_row)
            .collect::<Result<Vec<_>, _>>()
            .map_err(SessionOutboxError::Db)
    }

    async fn revoke_runner_session(
        &self,
        session_id: Uuid,
        reason: &str,
    ) -> Result<(), SessionOutboxError> {
        sqlx::query(runner_session::REVOKE_SQL)
            .bind(now_micros())
            .bind(reason)
            .bind(session_id)
            .execute(self.pool)
            .await
            .map_err(SessionOutboxError::Db)?;
        Ok(())
    }

    async fn clear_session_marker(&self, session_id: Uuid) -> Result<(), SessionOutboxError> {
        let redis = redis_client(self.state);
        session_outbox::clear_session_marker(redis.as_ref(), &session_id.to_string()).await
    }

    async fn publish_session_eviction(
        &self,
        runner_id: Uuid,
        old_session_id: Uuid,
        new_session_id: &str,
    ) -> Result<(), SessionOutboxError> {
        let redis = redis_client(self.state);
        let runner_id = runner_id.to_string();
        let old = old_session_id.to_string();
        session_outbox::publish_session_eviction(
            redis.as_ref(),
            &runner_id,
            Some(&old),
            new_session_id,
        )
        .await
    }
}

/// Dispatch one [`drain_kernel::DrainEffect`] after commit, logging
/// the swallowed-failure lines. Only the offline error propagates —
/// the row stays `ASSIGNED` while the caller observes the failure,
/// exactly as Python (TRACE bug 2).
async fn dispatch_assign(
    pool: &sqlx::postgres::PgPool,
    state: &AppState,
    effect: &drain_kernel::DrainEffect,
) -> Result<(), ()> {
    let pubsub = PollPubsubStore::new(pool, state);
    let drain_kernel::DrainEffect::SendAssign { runner_id, message } = effect;
    let outcome = pubsub_kernel::send_to_runner(&pubsub, *runner_id, message)
        .await
        .map_err(|_| ())?;
    for warning in &outcome.warnings {
        tracing::warn!("{warning}");
    }
    Ok(())
}

/// `drain_for_runner_by_id` (`matcher.py:298-304`): the by-id lookup
/// (miss ⇒ `Ok(false)`), else the locked drain. Runner-row indices
/// follow the 30-column projection (`id` 0, `owner_id` 1, `pod_id` 4,
/// `provisioning` 7, `visibility` 8); run-row indices the 41-column
/// image (`id` 0, `work_item_id` 7, `prompt` 19, `run_config` 23).
async fn drain_for_runner_by_id(
    pool: &sqlx::postgres::PgPool,
    state: &AppState,
    runner_id: Uuid,
) -> Result<bool, ()> {
    use sqlx::Row;
    let found: Option<sqlx::postgres::PgRow> =
        sqlx::query(drain_kernel::DRAIN_FOR_RUNNER_BY_ID_LOOKUP_SQL)
            .bind(runner_id)
            .fetch_optional(pool)
            .await
            .map_err(|_| ())?;
    if found.is_none() {
        return Ok(false);
    }
    let mut tx = pool.begin().await.map_err(|_| ())?;
    let threshold = alive_threshold(now_micros());
    let locked: Option<sqlx::postgres::PgRow> =
        sqlx::query(drain_kernel::DRAIN_FOR_RUNNER_LOCK_SQL)
            .bind(threshold)
            .bind(runner_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|_| ())?;
    let Some(locked) = locked else {
        return Ok(false);
    };
    let locked_runner_id: Uuid = locked.try_get(0).map_err(|_| ())?;
    let owner_id: Uuid = locked.try_get(1).map_err(|_| ())?;
    let pod_id: Uuid = locked.try_get(4).map_err(|_| ())?;
    let provisioning: String = locked.try_get(7).map_err(|_| ())?;
    let visibility: i16 = locked.try_get(8).map_err(|_| ())?;
    // Non-private runners issue no query (the `qs.none()` arm).
    let Some(next_sql) = drain_kernel::next_for_runner_sql(&provisioning, i32::from(visibility))
    else {
        return Ok(false);
    };
    let run: Option<sqlx::postgres::PgRow> = sqlx::query(&next_sql)
        .bind(pod_id)
        .bind(locked_runner_id)
        .bind(owner_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|_| ())?;
    let Some(run) = run else {
        return Ok(false);
    };
    let run_id: Uuid = run.try_get(0).map_err(|_| ())?;
    let work_item_id: Option<Uuid> = run.try_get(7).map_err(|_| ())?;
    let prompt: String = run.try_get(19).map_err(|_| ())?;
    let run_config: Value = run.try_get(23).map_err(|_| ())?;
    let Value::Object(run_config) = run_config else {
        return Err(());
    };
    sqlx::query(drain_kernel::ASSIGN_RUN_UPDATE_SQL)
        .bind(owner_id)
        .bind(locked_runner_id)
        .bind(now_micros())
        .bind(run_id)
        .execute(&mut *tx)
        .await
        .map_err(|_| ())?;
    let plan = drain_kernel::plan_assignment(&drain_kernel::AssignmentFacts {
        run_id,
        runner_id: locked_runner_id,
        owner_id,
        work_item_id,
        prompt,
        run_config,
    });
    tx.commit().await.map_err(|_| ())?;
    // The `on_commit` send fires before the `:294` log line (the
    // registration runs immediately with no active transaction).
    dispatch_assign(pool, state, &plan.after_commit).await?;
    tracing::info!(
        "{}",
        drain_kernel::drain_for_runner_log(&locked_runner_id, &run_id)
    );
    Ok(true)
}

/// `drain_pod_by_id` (`matcher.py:242-251`): pod lookup (miss ⇒ `Ok`,
/// no tx), else the drain loop. A dispatch failure propagates — the
/// row stays `ASSIGNED` while the caller observes the failure.
async fn drain_pod_by_id(
    pool: &sqlx::postgres::PgPool,
    state: &AppState,
    pod_id: Uuid,
) -> Result<(), ()> {
    let pod: Option<sqlx::postgres::PgRow> = sqlx::query(drain_kernel::DRAIN_POD_BY_ID_LOOKUP_SQL)
        .bind(pod_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| ())?;
    if pod.is_none() {
        return Ok(());
    }
    drain_pod(pool, state, pod_id).await
}

/// `drain_pod` (`matcher.py:194-239`): lock the idle list, then per
/// runner in list order take the next queued run, write the
/// assignment, and plan the frame; commit once; then dispatch each
/// frame in assignment order. The `:237` log line runs after the
/// sends (the `on_commit` registrations fire immediately).
async fn drain_pod(
    pool: &sqlx::postgres::PgPool,
    state: &AppState,
    pod_id: Uuid,
) -> Result<(), ()> {
    use sqlx::Row;
    let mut tx = pool.begin().await.map_err(|_| ())?;
    let threshold = alive_threshold(now_micros());
    let runners = sqlx::query(drain_kernel::DRAIN_POD_IDLE_RUNNERS_SQL)
        .bind(threshold)
        .bind(pod_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(|_| ())?;
    let mut plans: Vec<drain_kernel::AssignmentPlan> = Vec::new();
    for runner in &runners {
        let runner_id: Uuid = runner.try_get(0).map_err(|_| ())?;
        let owner_id: Uuid = runner.try_get(1).map_err(|_| ())?;
        let provisioning: String = runner.try_get(7).map_err(|_| ())?;
        let visibility: i16 = runner.try_get(8).map_err(|_| ())?;
        // Non-private runners issue no query (the `qs.none()` arm).
        let Some(next_sql) =
            drain_kernel::next_for_runner_sql(&provisioning, i32::from(visibility))
        else {
            continue;
        };
        let run: Option<sqlx::postgres::PgRow> = sqlx::query(&next_sql)
            .bind(pod_id)
            .bind(runner_id)
            .bind(owner_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|_| ())?;
        let Some(run) = run else {
            continue;
        };
        let run_id: Uuid = run.try_get(0).map_err(|_| ())?;
        let work_item_id: Option<Uuid> = run.try_get(7).map_err(|_| ())?;
        let prompt: String = run.try_get(19).map_err(|_| ())?;
        let run_config: Value = run.try_get(23).map_err(|_| ())?;
        let Value::Object(run_config) = run_config else {
            return Err(());
        };
        // `$3` is a fresh `timezone.now()` per assignment (`:233`).
        sqlx::query(drain_kernel::ASSIGN_RUN_UPDATE_SQL)
            .bind(owner_id)
            .bind(runner_id)
            .bind(now_micros())
            .bind(run_id)
            .execute(&mut *tx)
            .await
            .map_err(|_| ())?;
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
    tx.commit().await.map_err(|_| ())?;
    for plan in &plans {
        dispatch_assign(pool, state, &plan.after_commit).await?;
    }
    if !plans.is_empty() {
        tracing::info!("{}", drain_kernel::drain_pod_log(&pod_id, plans.len()));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Post-commit firing (the `on_commit` registrations, in order)
// ---------------------------------------------------------------------------

/// Drain one finalized run's publish pair, each effect isolated with
/// its failure log (`agent_run_finalization.py` `_publish_effects`:
/// the emit and the inline apply sit in separate `try` blocks).
async fn drain_publish_pair(
    pool: &sqlx::postgres::PgPool,
    ports: &LivePorts,
    pair: [LifecycleEffect; 2],
) {
    for effect in pair {
        let label = match &effect {
            LifecycleEffect::PublishTerminalEffects { run_id } => {
                format!("failed to publish terminal effects for run {run_id}")
            }
            LifecycleEffect::ApplyTerminalEffectsInline { run_id } => {
                format!("failed to apply terminal effects for run {run_id}")
            }
            unexpected => format!("failed to drain unexpected terminal effect: {unexpected:?}"),
        };
        if drain_lifecycle_effects(pool, ports, vec![effect])
            .await
            .is_err()
        {
            tracing::error!("{label}");
        }
    }
}

/// Fire one `_drain_after_commit` effect (`:277-290`): handoffs run
/// through the D-12 provider, drains through [`drain_kernel`] — all
/// unguarded, so any failure propagates to the poll (TRACE bug 2).
/// The cancel retry is the isolated twin (`:223-241`, one log line).
async fn fire_session_effect(
    pool: &sqlx::postgres::PgPool,
    state: &AppState,
    effect: session_kernel::SessionEffect,
) -> Result<(), ()> {
    use session_kernel::SessionEffect;
    match effect {
        SessionEffect::RetryCancelDelivery { runner_id, message } => {
            let pubsub = PollPubsubStore::new(pool, state);
            let run_id = message
                .get("run_id")
                .and_then(Value::as_str)
                .unwrap_or_default();
            match pubsub_kernel::send_to_runner(&pubsub, runner_id, &message).await {
                Ok(outcome) => {
                    for warning in &outcome.warnings {
                        tracing::warn!("{warning}");
                    }
                }
                Err(error) => {
                    tracing::error!(
                        "{}",
                        session_kernel::cancel_retry_error_line(run_id, &error.to_string())
                    );
                }
            }
            Ok(())
        }
        SessionEffect::DrainRunner { runner_id } => drain_for_runner_by_id(pool, state, runner_id)
            .await
            .map(|_| ()),
        SessionEffect::DrainPod { pod_id } => drain_pod_by_id(pool, state, pod_id).await,
        SessionEffect::CompleteProjectMoveHandoff { run_id } => {
            crate::project_move_handoff::complete_project_move_handoff(pool, state, run_id)
                .await
                .map(|_| ())
        }
    }
}

// ---------------------------------------------------------------------------
// Wait (`_aread_with_eviction_awareness`, `:462-538`)
// ---------------------------------------------------------------------------

/// Bookkeeping plan (`_poll_bookkeeping`, `:432-459`): the wait
/// window plus whether this poll drains the PEL (`use_zero`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PollPlan {
    block_ms: i64,
    use_zero: bool,
}

/// Plan from the PEL marker (`:434, :458`): a fresh session replays
/// its PEL with a zero-block read; a drained one blocks up to the
/// poll interval (floored at 1ms).
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

/// Whether this poll schedules the poll-ready drain (`:441-445`): one
/// trigger — stale-heartbeat recovery, became-available, or first
/// poll — gated on drain eligibility and no reported in-flight run.
fn should_schedule_poll_drain(
    was_stale: bool,
    became_available: bool,
    use_zero: bool,
    status_allows_drain: bool,
    in_flight_reported: bool,
) -> bool {
    (was_stale || became_available || use_zero) && status_allows_drain && !in_flight_reported
}

/// Outcome of the eviction-aware wait: messages, or an eviction that
/// landed mid-wait (`_SessionEvictedDuringPoll`, `:535-536`).
enum PollWait {
    Messages(Vec<DecodedMessage>),
    Evicted,
}

/// Next wait slice against the deadline (`:507-524`): `None` once the
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
/// (`_aread_with_eviction_awareness`, `:462-538`).
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
    runner_id: &str,
    session_id: &str,
    plan: &PollPlan,
) -> Result<PollWait, Response> {
    if plan.block_ms <= 0 {
        if !plan.use_zero {
            return Ok(PollWait::Messages(Vec::new()));
        }
        return match session_outbox::read_for_session(
            redis,
            runner_id,
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
        // No Redis: the single blocking read (`:493-501`).
        return match session_outbox::read_for_session(
            redis,
            runner_id,
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
    let channel = runner_keys::session_eviction_channel(runner_id);
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
        let messages = match session_outbox::read_for_session(
            Some(redis),
            runner_id,
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
            // eviction publish (`:535-536`).
            Ok(Ok(_)) => return Ok(PollWait::Evicted),
            // A failing subscription check propagates like the
            // source's unguarded `get_message`.
            Ok(Err(_)) => return Err(server_error()),
            // Elapsed: nothing buffered; next slice.
            Err(_) => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Poll
// ---------------------------------------------------------------------------

/// `POST runners/<rid>/sessions/<sid>/poll` — long-poll
/// (`runner_session_poll`, `:541-601`).
///
/// A plain async view (not DRF): the auth class is invoked directly
/// (401s re-rendered spaced/challenge-free), the body parses as raw
/// JSON, and bookkeeping runs in short-lived checkouts — the wait
/// below holds NO pooled connection. After the wait, the original
/// plan's `use_zero` (not the loop-mutated one) decides the PEL-drain
/// mark; the 200 carries the envelope.
pub async fn runner_session_poll(
    State(state): State<AppState>,
    Path((raw_runner_id, raw_sid)): Path<(String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let pool = match pool_of(&state) {
        Ok(pool) => pool.clone(),
        Err(response) => return response,
    };
    let runner_id = match parse_uuid(&raw_runner_id) {
        Ok(id) => id,
        Err(response) => return response,
    };
    let sid = match parse_uuid(&raw_sid) {
        Ok(id) => id,
        Err(response) => return response,
    };
    // Stock settings carry no `RUNNER_ACCESS_TOKEN_KEYS`, so the ring
    // is always the derived dev key (the `enroll.rs` `&[]` position).
    // The canonical URL id renders like Django's `<uuid:>` converter
    // output, which is what the auth comparison reads.
    let secret = state.settings().secret_key.clone();
    let ring = build_key_ring(&[], &secret);
    let url_runner_id = runner_id.to_string();
    let auth = match authenticate_access_token(
        &pool,
        secret.as_bytes(),
        &ring,
        &headers,
        Some(&url_runner_id),
        // The poll route is POST-only; the value never reaches the
        // wire — `poll_denial` re-renders every 401 from scratch
        // (plain view: no `WWW-Authenticate`, no `Allow`).
        ALLOW_POST,
    )
    .await
    {
        Ok(auth) => auth,
        Err(response) => return poll_denial(response).await,
    };
    let Some(auth) = auth else {
        return render(runner_id_mismatch_poll());
    };
    if auth.runner.id != runner_id {
        return render(runner_id_mismatch_poll());
    }

    let rid = url_runner_id;
    let sid_str = sid.to_string();
    let body = match poll_body(&body) {
        Ok(body) => body,
        Err(response) => return response,
    };

    // Bookkeeping (`_poll_bookkeeping`, `:330-459`).
    let row = match sqlx::query(runner_session::POLL_GET_SQL)
        .bind(sid)
        .bind(runner_id)
        .fetch_optional(&pool)
        .await
    {
        Ok(row) => row,
        Err(_) => return server_error(),
    };
    let Some(row) = row else {
        return render(session_evicted_poll(None));
    };
    let session = match runner_session_from_row(&row) {
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
    let status_entry: Map<String, Value> = body
        .get("status")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    if sqlx::query(runner_session::TOUCH_SQL)
        .bind(now_micros())
        .bind(sid)
        .execute(&pool)
        .await
        .is_err()
    {
        return server_error();
    }

    // Heartbeat transaction (`:362-412`): caps, the prior snapshot,
    // the update, the reaper. The `now` samples before the txn, like
    // `:371`.
    let hb_now = now_micros();
    let mut tx = match pool.begin().await {
        Ok(tx) => tx,
        Err(error) => return heartbeat_error(error),
    };
    if let Err(error) = sqlx::query(SET_LOCAL_LOCK_TIMEOUT_SQL)
        .execute(&mut *tx)
        .await
    {
        return heartbeat_error(error);
    }
    if let Err(error) = sqlx::query(SET_LOCAL_STATEMENT_TIMEOUT_SQL)
        .execute(&mut *tx)
        .await
    {
        return heartbeat_error(error);
    }
    let snapshot: Option<(Option<DateTime<Utc>>, String)> =
        match sqlx::query_as(HEARTBEAT_SNAPSHOT_SQL)
            .bind(runner_id)
            .fetch_optional(&mut *tx)
            .await
        {
            Ok(snapshot) => snapshot,
            Err(error) => return heartbeat_error(error),
        };
    let Some((prior_hb, prior_status)) = snapshot else {
        // The runner vanished between auth and bookkeeping: the
        // source's `.get()` raises `DoesNotExist` (not an
        // `OperationalError`), so this 500s.
        return server_error();
    };
    let reported_status = status_entry.get("status").and_then(Value::as_str);
    let decision = heartbeat_decision(
        hb_now,
        prior_hb,
        &prior_status,
        reported_status,
        status_entry.is_empty(),
    );
    if let Err(error) = sqlx::query(HEARTBEAT_UPDATE_SQL)
        .bind(hb_now)
        .bind(decision.new_status)
        .bind(runner_id)
        .execute(&mut *tx)
        .await
    {
        return heartbeat_error(error);
    }
    let mut reap = ReapOutcome {
        retry_cancel: None,
        publish_pairs: Vec::new(),
        drain_effects: Vec::new(),
    };
    if !status_entry.is_empty() {
        reap = match reap_stale_busy_runs(&mut tx, runner_id, &status_entry).await {
            Ok(reap) => reap,
            Err(response) => return response,
        };
    }
    if let Err(error) = tx.commit().await {
        return heartbeat_error(error);
    }
    // The heartbeat commit fires the reap registrations in order:
    // the cancel retry, then one publish pair per finalized run in
    // reap order, then the drain effects. Only the drain effects
    // propagate (TRACE bug 2).
    if let Some(retry) = reap.retry_cancel {
        if fire_session_effect(&pool, &state, retry).await.is_err() {
            return server_error();
        }
    }
    if !reap.publish_pairs.is_empty() {
        let ports = LivePorts::new(pool.clone(), &state);
        for pair in reap.publish_pairs {
            drain_publish_pair(&pool, &ports, pair).await;
        }
    }
    for effect in reap.drain_effects {
        if fire_session_effect(&pool, &state, effect).await.is_err() {
            return server_error();
        }
    }

    // Volatile observability snapshot (`:413-426`): fully swallowed.
    if !status_entry.is_empty() {
        upsert_runner_live_state(&pool, runner_id, &status_entry).await;
    }

    // XACK explicit ids (`:428-430`).
    let redis = redis_client(&state);
    if !ack_ids.is_empty()
        && session_outbox::ack_for_session(redis.as_ref(), &rid, &ack_ids)
            .await
            .is_err()
    {
        return server_error();
    }

    // Plan + poll-ready drain (`:432-458`).
    let pel_drained = match session_outbox::is_pel_drained(redis.as_ref(), &sid_str).await {
        Ok(drained) => drained,
        Err(_) => return server_error(),
    };
    let plan = poll_plan(state.settings().runner.long_poll_interval_secs, pel_drained);
    let in_flight_reported = status_entry.get("in_flight_run").is_some_and(py_truthy);
    if should_schedule_poll_drain(
        decision.was_stale,
        decision.became_available,
        plan.use_zero,
        decision.status_allows_drain,
        in_flight_reported,
    ) {
        // The `on_commit` fires immediately (no active transaction);
        // the source wraps the drain (`:447-456`), so failures log
        // and the poll continues.
        if drain_for_runner_by_id(&pool, &state, runner_id)
            .await
            .is_err()
        {
            tracing::error!("drain_for_runner_by_id failed for poll-ready runner {runner_id}");
        }
    }

    let messages =
        match read_with_eviction_awareness(&state, redis.as_ref(), &rid, &sid_str, &plan).await {
            Ok(PollWait::Messages(messages)) => messages,
            Ok(PollWait::Evicted) => return render(session_evicted_poll(None)),
            Err(response) => return response,
        };
    if plan.use_zero
        && session_outbox::mark_pel_drained(redis.as_ref(), &state.settings().runner, &sid_str)
            .await
            .is_err()
    {
        return server_error();
    }

    render(poll_200(
        &messages,
        &django_isoformat(now_micros()),
        state.settings().runner.long_poll_interval_secs,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::header;
    use chrono::Duration;

    use crate::runner_enroll::auth::{bearer_failure_response, server_error as auth_server_error};

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
    /// (`sessions.py:354`).
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
        // `DataError`).
        assert_eq!(
            ack_ids_from_body(&body_with_ack(Value::Array(vec![Value::Bool(true)]))),
            Err(())
        );
        assert_eq!(
            ack_ids_from_body(&body_with_ack(Value::Array(vec![Value::Array(vec![])]))),
            Err(())
        );
    }

    /// Plan derivation (`:434, :458`): fresh sessions replay with a
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

    /// Slicing (`:507-524`): capped at 1s, and `None` on an expired
    /// deadline so `BLOCK 0` is never issued past it.
    #[test]
    fn next_slice_caps_and_expires() {
        let now = Instant::now();
        assert_eq!(
            next_slice_ms(now + std::time::Duration::from_millis(2500), now),
            Some(1000)
        );
        let short = next_slice_ms(now + std::time::Duration::from_millis(300), now)
            .expect("unexpired deadline slices");
        assert!((1..=300).contains(&short));
        assert_eq!(
            next_slice_ms(now, now + std::time::Duration::from_millis(1)),
            None
        );
    }

    /// `timezone.now().isoformat()`: `+00:00`, micros iff nonzero.
    #[test]
    fn django_isoformat_matches_python() {
        let with_micros = chrono::DateTime::parse_from_rfc3339("2026-10-02T23:58:25.738360+00:00")
            .expect("fixture time parses")
            .with_timezone(&Utc);
        assert_eq!(
            django_isoformat(with_micros),
            "2026-10-02T23:58:25.738360+00:00"
        );
        let whole = chrono::DateTime::parse_from_rfc3339("2026-10-02T23:58:25+00:00")
            .expect("whole-second time parses")
            .with_timezone(&Utc);
        assert_eq!(django_isoformat(whole), "2026-10-02T23:58:25+00:00");
    }

    /// Poll body handling (`:570-577`): empty → `{}`; garbage → the
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

    /// Heartbeat decision (`:380-389`): staleness, availability,
    /// drain eligibility, and the stamped status.
    #[test]
    fn heartbeat_decision_covers_snapshot_and_report() {
        let now = Utc::now();
        let fresh = now - Duration::seconds(10);
        let stale = now - Duration::seconds(91);
        // Fresh + idle report: available, drains, stamps online.
        assert_eq!(
            heartbeat_decision(now, Some(fresh), "online", Some("idle"), false),
            HeartbeatDecision {
                was_stale: false,
                became_available: false,
                status_allows_drain: true,
                new_status: "online",
            }
        );
        // Missing heartbeat is stale; empty entry over a non-busy
        // prior still allows the drain.
        assert_eq!(
            heartbeat_decision(now, None, "online", None, true),
            HeartbeatDecision {
                was_stale: true,
                became_available: false,
                status_allows_drain: true,
                new_status: "online",
            }
        );
        // Busy edge: became-available, drains, stamps online.
        assert_eq!(
            heartbeat_decision(now, Some(stale), "busy", Some("online"), false),
            HeartbeatDecision {
                was_stale: true,
                became_available: true,
                status_allows_drain: true,
                new_status: "online",
            }
        );
        // Reported busy: no availability, no drain, stamps busy —
        // even over a stale prior.
        assert_eq!(
            heartbeat_decision(now, Some(stale), "online", Some("busy"), false),
            HeartbeatDecision {
                was_stale: true,
                became_available: false,
                status_allows_drain: false,
                new_status: "busy",
            }
        );
        // Non-empty entry without a `status` key: the drain gate
        // reads the whole entry, not the missing key.
        assert_eq!(
            heartbeat_decision(now, Some(fresh), "online", None, false),
            HeartbeatDecision {
                was_stale: false,
                became_available: false,
                status_allows_drain: false,
                new_status: "online",
            }
        );
        // Empty entry over a busy prior: no drain.
        assert_eq!(
            heartbeat_decision(now, Some(fresh), "busy", None, true),
            HeartbeatDecision {
                was_stale: false,
                became_available: false,
                status_allows_drain: false,
                new_status: "online",
            }
        );
    }

    /// Poll-ready drain trigger (`:441-445`): one trigger (stale,
    /// became-available, or first poll), gated on eligibility and no
    /// reported in-flight run.
    #[test]
    fn poll_drain_trigger_is_single_gated() {
        // Each trigger fires alone …
        assert!(should_schedule_poll_drain(true, false, false, true, false));
        assert!(should_schedule_poll_drain(false, true, false, true, false));
        assert!(should_schedule_poll_drain(false, false, true, true, false));
        // … but never without a trigger, without eligibility, or
        // with an in-flight run reported.
        assert!(!should_schedule_poll_drain(
            false, false, false, true, false
        ));
        assert!(!should_schedule_poll_drain(true, true, true, false, false));
        assert!(!should_schedule_poll_drain(true, true, true, true, true));
    }

    /// Minimal [`sqlx::DatabaseError`] double for the 503 predicate.
    #[derive(Debug)]
    struct DbError {
        code: Option<String>,
    }

    impl std::fmt::Display for DbError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "db error")
        }
    }

    impl std::error::Error for DbError {}

    impl sqlx::error::DatabaseError for DbError {
        fn message(&self) -> &str {
            "db error"
        }

        fn code(&self) -> Option<std::borrow::Cow<'_, str>> {
            self.code.as_deref().map(std::borrow::Cow::Borrowed)
        }

        fn kind(&self) -> sqlx::error::ErrorKind {
            sqlx::error::ErrorKind::Other
        }

        fn is_transient_in_connect_phase(&self) -> bool {
            false
        }

        fn as_error(&self) -> &(dyn std::error::Error + Send + Sync + 'static) {
            self
        }

        fn as_error_mut(&mut self) -> &mut (dyn std::error::Error + Send + Sync + 'static) {
            self
        }

        fn into_error(self: Box<Self>) -> Box<dyn std::error::Error + Send + Sync + 'static> {
            self
        }
    }

    fn db_error(code: Option<&str>) -> sqlx::Error {
        sqlx::Error::Database(Box::new(DbError {
            code: code.map(str::to_owned),
        }))
    }

    /// 503 predicate (`:399-412`): every `OperationalError` class
    /// psycopg raises mid-transaction plus transport/pool failures
    /// are the convoy-guard 503; constraint/syntax/data errors 500.
    #[test]
    fn operational_error_maps_the_guard() {
        for code in [
            "55P03", "57014", "08006", "08000", "40P01", "40001", "40002", "53100", "53300",
            "54001", "55006", "57P01", "58000",
        ] {
            assert!(is_operational_error(&db_error(Some(code))), "{code}");
        }
        for code in ["23505", "23503", "22P02", "42601", "42P01"] {
            assert!(!is_operational_error(&db_error(Some(code))), "{code}");
        }
        assert!(!is_operational_error(&db_error(None)));
        assert!(is_operational_error(&sqlx::Error::Io(std::io::Error::new(
            std::io::ErrorKind::ConnectionReset,
            "reset"
        ))));
        assert!(is_operational_error(&sqlx::Error::PoolTimedOut));
        assert!(is_operational_error(&sqlx::Error::PoolClosed));
        assert!(!is_operational_error(&sqlx::Error::RowNotFound));
    }

    /// The heartbeat SQL this module spells keeps the Django
    /// queryset shapes (terms verified against the FX-RSES-02
    /// `bound_txn_waits` + `bookkeeping_happy.sql` captures).
    #[test]
    fn heartbeat_sql_keeps_queryset_shapes() {
        assert_eq!(
            SET_LOCAL_LOCK_TIMEOUT_SQL,
            "SET LOCAL lock_timeout = '5000ms'"
        );
        assert_eq!(
            SET_LOCAL_STATEMENT_TIMEOUT_SQL,
            "SET LOCAL statement_timeout = '20000ms'"
        );
        assert_eq!(
            HEARTBEAT_SNAPSHOT_SQL,
            "SELECT \"runner\".\"last_heartbeat_at\", \"runner\".\"status\" FROM \"runner\" WHERE \"runner\".\"id\" = $1 LIMIT 21 FOR UPDATE"
        );
        assert_eq!(
            HEARTBEAT_UPDATE_SQL,
            "UPDATE \"runner\" SET \"last_heartbeat_at\" = $1, \"status\" = $2 WHERE \"runner\".\"id\" = $3"
        );
    }

    /// A post-718-shaped denial (already lowercase-spaced without the
    /// challenge): the re-render must accept it, so the PIDASHCONV-718
    /// fix heals rather than breaks this path.
    fn post_718_denial() -> Response {
        Response::builder()
            .status(StatusCode::UNAUTHORIZED)
            .header(header::CONTENT_TYPE, "application/json")
            .body(axum::body::Body::from(
                r#"{"detail":"access_token_malformed"}"#,
            ))
            .expect("denial builds")
    }

    /// The poll 401 re-render: the D-13 denial keeps its code but
    /// renders lowercase-spaced with no challenge and no `Allow`
    /// header, exactly the hand-built `JsonResponse` (`:563-566` —
    /// the plain view never had DRF's `default_response_headers`);
    /// 500s pass through untouched.
    #[tokio::test]
    async fn poll_denial_rerenders_401_spaced_without_challenge() {
        for denial in [
            bearer_failure_response("access_token_malformed", ALLOW_POST),
            post_718_denial(),
        ] {
            let denied = poll_denial(denial).await;
            assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);
            assert!(denied.headers().get(header::WWW_AUTHENTICATE).is_none());
            assert!(denied.headers().get(header::ALLOW).is_none());
            assert_eq!(
                body_text(denied).await,
                r#"{"detail": "access_token_malformed"}"#
            );
        }
        let failed = poll_denial(auth_server_error()).await;
        assert_eq!(failed.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    /// Poll shape wiring: the 403/503/409/405 bodies render
    /// byte-identical to the FX-RSES-02 captures (spaced).
    #[tokio::test]
    async fn poll_bodies_match_fixture_bytes() {
        let mismatch = render(runner_id_mismatch_poll());
        assert_eq!(mismatch.status(), StatusCode::FORBIDDEN);
        assert_eq!(
            body_text(mismatch).await,
            r#"{"error": "runner_id_mismatch"}"#
        );
        let locked = render(runner_state_locked_poll());
        assert_eq!(locked.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            body_text(locked).await,
            r#"{"error": "runner_state_locked"}"#
        );
        let evicted = render(session_evicted_poll(Some("evicted_by_new_session")));
        assert_eq!(evicted.status(), StatusCode::CONFLICT);
        assert_eq!(
            body_text(evicted).await,
            r#"{"error": "session_evicted", "reason": "evicted_by_new_session"}"#
        );
        let denied = render(unauthorized_poll("runner_id_mismatch"));
        assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            body_text(denied).await,
            r#"{"detail": "runner_id_mismatch"}"#
        );
    }

    /// The live-state snapshot gate (`:353-357`): only observability
    /// keys run the `get_or_create` at all.
    #[test]
    fn live_state_gate_needs_snapshot_keys() {
        assert!(!has_live_state_snapshot(&Map::new()));
        let mut plain = Map::new();
        plain.insert("status".to_owned(), Value::String("idle".to_owned()));
        assert!(!has_live_state_snapshot(&plain));
        for key in [
            "observed_run_id",
            "last_event_at",
            "last_event_kind",
            "last_event_summary",
            "agent_pid",
            "agent_subprocess_alive",
            "approvals_pending",
            "llm_model",
            "turn_count",
            "tokens",
            "model",
        ] {
            let mut entry = Map::new();
            entry.insert(key.to_owned(), Value::Null);
            assert!(has_live_state_snapshot(&entry), "{key} gates");
        }
    }

    /// `parse_live_int` is CPython `int()`: whitespace, sign, and
    /// singly-placed underscores pass; fractions, empties, and junk
    /// fail the upsert (past `i64` fails too — same outcome as
    /// Django's `save()` hitting the column overflow).
    #[test]
    fn live_int_matches_cpython_int() {
        for (raw, expected) in [
            ("3", Some(3)),
            ("  -3 ", Some(-3)),
            ("+3", Some(3)),
            ("1_0", Some(10)),
            ("-1_0", Some(-10)),
            ("007", Some(7)),
            ("3.5", None),
            ("", None),
            ("   ", None),
            ("x", None),
            ("3x", None),
            ("1__0", None),
            ("_10", None),
            ("10_", None),
            ("+", None),
            ("99999999999999999999999", None),
        ] {
            assert_eq!(parse_live_int(raw), expected, "{raw:?}");
        }
    }

    /// `live_bool_from_text` is `BooleanField.to_python` on strings:
    /// the six literals bind, `""` is `NULL` (`null=True`), anything
    /// else (including `"TRUE"`/`"yes"`) fails the upsert.
    #[test]
    fn live_bool_text_matches_django_literals() {
        for raw in ["t", "True", "1"] {
            assert_eq!(live_bool_from_text(raw), Some(Some(true)), "{raw:?}");
        }
        for raw in ["f", "False", "0"] {
            assert_eq!(live_bool_from_text(raw), Some(Some(false)), "{raw:?}");
        }
        assert_eq!(live_bool_from_text(""), Some(None));
        for raw in ["TRUE", "FALSE", "yes", "no", " true", "2"] {
            assert_eq!(live_bool_from_text(raw), None, "{raw:?}");
        }
    }

    /// `live_bool_from_json` is `BooleanField.to_python` on JSON:
    /// `1`/`1.0` are true and `0`/`0.0` are false (`==` against
    /// `True`/`False`); `[]`/`{}`/`null` are `NULL`; other numbers
    /// and non-empty containers fail the upsert.
    #[test]
    fn live_bool_json_matches_django_equality() {
        assert_eq!(live_bool_from_json(&Value::Bool(true)), Some(Some(true)));
        assert_eq!(live_bool_from_json(&Value::Bool(false)), Some(Some(false)));
        for raw in [1, 0] {
            let json = Value::Number(serde_json::Number::from(raw));
            assert_eq!(live_bool_from_json(&json), Some(Some(raw == 1)));
        }
        for (raw, expected) in [(1.0, true), (0.0, false)] {
            let json = Value::Number(serde_json::Number::from_f64(raw).unwrap());
            assert_eq!(live_bool_from_json(&json), Some(Some(expected)));
        }
        for raw in [2, -1] {
            let json = Value::Number(serde_json::Number::from(raw));
            assert_eq!(live_bool_from_json(&json), None, "{raw}");
        }
        let json = Value::Number(serde_json::Number::from_f64(1.5).unwrap());
        assert_eq!(live_bool_from_json(&json), None);
        assert_eq!(live_bool_from_json(&Value::Array(vec![])), Some(None));
        assert_eq!(live_bool_from_json(&Value::Object(Map::new())), Some(None));
        assert_eq!(live_bool_from_json(&Value::Null), Some(None));
        assert_eq!(
            live_bool_from_json(&Value::Array(vec![Value::Bool(true)])),
            None
        );
        let mut object = Map::new();
        object.insert("a".to_owned(), Value::Bool(true));
        assert_eq!(live_bool_from_json(&Value::Object(object)), None);
    }

    /// `parse_live_datetime` is `DateTimeField.to_python` + the
    /// `USE_TZ` naive rule: offsets convert to UTC, naive values are
    /// assumed UTC, garbage and impossible dates fail the upsert,
    /// fraction digits past six truncate (not round).
    #[test]
    fn live_datetime_matches_django_parse() {
        let utc = |text: &str| {
            chrono::NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S%.f")
                .unwrap()
                .and_utc()
        };
        for (raw, expected) in [
            (
                "2026-10-09T06:55:56.748795+00:00",
                utc("2026-10-09T06:55:56.748795"),
            ),
            ("2026-10-09T06:55:56Z", utc("2026-10-09T06:55:56")),
            ("2026-10-09 06:55:56+00:00", utc("2026-10-09T06:55:56")),
            ("2026-10-09T08:55:56+02:00", utc("2026-10-09T06:55:56")),
            (
                "2026-10-09T06:55:56.7487959+00:00",
                utc("2026-10-09T06:55:56.748795"),
            ),
            ("2026-10-09T06:55:56", utc("2026-10-09T06:55:56")),
            ("2026-10-09 06:55", utc("2026-10-09T06:55:00")),
            ("2026-10-09T08:55:56+0200", utc("2026-10-09T06:55:56")),
            ("2026-10-09T08:55:56+02", utc("2026-10-09T06:55:56")),
        ] {
            assert_eq!(parse_live_datetime(raw), Some(expected), "{raw:?}");
        }
        for raw in [
            "",
            "not-a-date",
            "2026-13-09T06:55:56",
            "2026-10-32T06:55:56",
            "2026-10-09T25:55:56",
            "2026-10-09",
            "06:55:56",
            "99999-10-09T06:55:56",
        ] {
            assert_eq!(parse_live_datetime(raw), None, "{raw:?}");
        }
    }
}
