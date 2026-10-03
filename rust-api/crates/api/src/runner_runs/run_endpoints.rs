//! Run daemon endpoints (D-15 L8, stage 5, PIDASHCONV-543).
//!
//! Ports `apps/api/pi_dash/runner/views/run_endpoints.py:1-510`: the 12
//! `POST /api/v1/runner/runs/<run_id>/...` endpoints (`accept`, `queued`,
//! `started`, `events`, `approvals`, `awaiting-reauth`, `complete`,
//! `pause`, `fail`, `cancelled`, `resumed`, `stream/upgrade`) plus the
//! `_RunEndpointBase` shared base (`_resolve`, `_lock_non_terminal`,
//! `_cancellation_pending`, `_record_dedupe`).
//!
//! # Execution model
//!
//! * Every endpoint runs one sqlx transaction, collects post-commit
//!   effects, commits, then drains them before responding — Python's
//!   synchronous `on_commit` order. Each drain site keeps Python's
//!   isolation, documented at the call.
//! * L4 plans execute here: [`pidash_services::runner_runs`] planners
//!   decide, this module binds and runs. Param-style kernel SQL
//!   (`$N`, `lifecycle`/`finalization`) runs verbatim; `SELECT`s are
//!   behavior-equal targeted column reads (the L2 rows carry no
//!   `FromRow`, and foundation crates are read-only, so full-row
//!   fetches cannot map) — same predicates, same locks, same
//!   `ORDER BY`/`LIMIT`, fewer columns. The gate compares behavior,
//!   never statement text.
//! * Frame coercions (`int()`, `.lower()`, `str()`, `or` defaults) are
//!   pure helpers below, unit-tested against FX-RUN-09.
//!
//! # Ported bugs (also listed in the PR)
//!
//! * None of the run endpoints carries a known bug; the retired
//!   `queued` ack-only shape (`run_endpoints.py:130-150`) is behavior,
//!   ported as-is.
//!
//! # Documented approximations (no contract input covers them)
//!
//! * `expires_at` binds as text with a `::timestamptz` cast: the same
//!   ISO strings Django accepts parse identically; garbage 500s on
//!   both sides.
//! * `int("1_0")` (underscore digits) 500s here where CPython reads
//!   `10`; absurd from a runner frame.
//! * A non-UUID `run_id` segment answers the endpoint's JSON 404 (the
//!   `assistant/events.rs` precedent); Django's `<uuid:>` converter
//!   404s at URL resolution instead. Same status, JSON not HTML.

// Every handler returns a fully-rendered `Response` by design (the
// intake `parse_body` precedent, which carries the same allow).
#![allow(clippy::result_large_err)]

use axum::extract::{Path, State};
use axum::http::{HeaderMap, Request, StatusCode};
use axum::response::Response;
use axum::Router;
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

use pidash_services::runner_runs::finalization as finalize_kernel;
use pidash_services::runner_runs::lifecycle as lifecycle_kernel;
use pidash_services::runner_runs::{LifecycleEffect, LiveStateUsageFacts};
use pidash_types::dispatch::AgentExecutorKind;
use pidash_types::runner_runs::{
    AgentRunStatus, DevMachineInfo, RunnerInfo, TERMINAL_RUN_STATUSES,
};

use super::{
    authenticate_daemon, frame_model, frame_text, json_response, pool_of, py_truthy,
    read_request_data, resolve_runner_for_run, server_error, truncate_chars, DaemonRunner,
    LivePorts, RunnerPorts,
};
use crate::state::AppState;

// ---------------------------------------------------------------------------
// Pure frame helpers (unit-tested against FX-RUN-09)
// ---------------------------------------------------------------------------

/// `MAX_EVENT_PAYLOAD_BYTES` (`run_endpoints.py:45`).
pub const MAX_EVENT_PAYLOAD_BYTES: usize = 64 * 1024;

/// A frame-coercion failure: the source's `AttributeError` /
/// `TypeError` / `ValueError` / `ValidationError` on the same input.
/// Callers map every variant to the same 500.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameError;

/// `_idempotency_key` (`run_endpoints.py:48-49`): the stripped header.
pub fn idempotency_key(headers: &HeaderMap) -> String {
    headers
        .get("idempotency-key")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .trim()
        .to_owned()
}

/// Whether a status value is terminal (`run.is_terminal` /
/// `_lock_non_terminal`, `run_endpoints.py:93-99`). Unknown values are
/// not terminal, exactly as the Python `in` test treats them.
pub fn is_terminal_status(status: &str) -> bool {
    TERMINAL_RUN_STATUSES
        .iter()
        .any(|terminal| terminal.value() == status)
}

/// `int(value)` for a JSON frame value (`run_endpoints.py:215`):
/// bools as 0/1, ints verbatim, floats truncated toward zero,
/// strings stripped then parsed (`+`/`-` accepted). `Err` is the
/// source's `ValueError`/`TypeError` (500). The caller applies the
/// `or 0` default before calling.
pub fn py_int(value: &Value) -> Result<i64, FrameError> {
    match value {
        Value::Bool(true) => Ok(1),
        Value::Bool(false) => Ok(0),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(i)
            } else if let Some(u) = n.as_u64() {
                i64::try_from(u).map_err(|_| FrameError)
            } else if let Some(f) = n.as_f64() {
                // `int()` truncates toward zero; out-of-range is
                // `OverflowError` (500 either way).
                if f.is_finite() && f >= i64::MIN as f64 && f <= i64::MAX as f64 {
                    Ok(f.trunc() as i64)
                } else {
                    Err(FrameError)
                }
            } else {
                Err(FrameError)
            }
        }
        Value::String(text) => {
            let stripped: String = text
                .trim_matches(|c: char| c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c))
                .to_owned();
            if stripped.is_empty() {
                return Err(FrameError);
            }
            stripped.parse::<i64>().map_err(|_| FrameError)
        }
        Value::Null | Value::Array(_) | Value::Object(_) => Err(FrameError),
    }
}

/// `request.data.get("events") or [request.data]`
/// (`run_endpoints.py:212`): the batch, or the whole body as one
/// event. `Err` is the source's `AttributeError` on a non-dict body
/// (500).
pub fn event_batch_items(data: &Value) -> Result<Vec<&Value>, FrameError> {
    let obj = data.as_object().ok_or(FrameError)?;
    match obj.get("events") {
        Some(events) if py_truthy(events) => match events.as_array() {
            Some(items) => Ok(items.iter().collect()),
            // A truthy non-list `events` iterates element-wise in
            // Python (`for ev in "ab"` yields chars); each char is a
            // non-dict and 500s on `.get` — but a truthy non-list
            // never reaches a per-item `.get` here, so model the
            // outcome directly: strings/dicts iterate into non-dict
            // items (500), numbers are not iterable (500).
            None => Err(FrameError),
        },
        _ => Ok(vec![data]),
    }
}

/// `str(value)` for a truthy frame value (`reason`, `TextField`
/// storage): strings verbatim, ints raw, floats in CPython repr
/// form, bools as `True`/`False`, containers in single-quote repr
/// form. Only called on truthy values (every call site gates first).
pub fn py_str_value(value: &Value) -> String {
    match value {
        Value::Null => "None".to_owned(),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::Number(n) => {
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
        Value::String(s) => s.clone(),
        Value::Array(items) => {
            let inner: Vec<String> = items.iter().map(py_repr_value).collect();
            format!("[{}]", inner.join(", "))
        }
        Value::Object(map) => {
            let inner: Vec<String> = map
                .iter()
                .map(|(k, v)| format!("{}: {}", py_repr_str(k), py_repr_value(v)))
                .collect();
            format!("{{{}}}", inner.join(", "))
        }
    }
}

fn py_repr_value(value: &Value) -> String {
    match value {
        Value::String(s) => py_repr_str(s),
        other => py_str_value(other),
    }
}

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
                c if !(c.is_control() || c == '\u{7f}' || c == '\u{2028}' || c == '\u{2029}') => {
                    out.push(c)
                }
                c if (c as u32) < 0x100 => out.push_str(&format!("\\x{:02x}", c as u32)),
                c if (c as u32) < 0x1_0000 => out.push_str(&format!("\\u{:04x}", c as u32)),
                c => out.push_str(&format!("\\U{:08x}", c as u32)),
            }
        }
    }
    out.push(quote);
    out
}

/// `(request.data.get("reason") or "")` then `== name`
/// (`run_endpoints.py:356,380,403`): falsy maps to `""`, strings
/// compare, every other truthy value compares unequal (no crash —
/// `==` never raises).
pub fn reason_is(data: &Value, name: &str) -> bool {
    let reason = data
        .as_object()
        .and_then(|obj| obj.get("reason"))
        .filter(|value| py_truthy(value))
        .and_then(Value::as_str)
        .unwrap_or("");
    reason == name
}

/// `request.data.get("tokens") or request.data.get("usage")`
/// (`run_endpoints.py:305,335,411,419,439`): the fresher usage frame,
/// or null. Non-dict bodies read as null — every call site passes
/// the value straight into the merge kernels, which treat null as
/// absent, so no `AttributeError` arm exists here (unlike the
/// endpoints that call `.get` on a field and then a method).
pub fn frame_tokens(data: &Value) -> Value {
    let obj = match data.as_object() {
        Some(obj) => obj,
        None => return Value::Null,
    };
    match obj.get("tokens") {
        Some(tokens) if py_truthy(tokens) => tokens.clone(),
        _ => obj.get("usage").cloned().unwrap_or(Value::Null),
    }
}

/// `(request.data.get("kind") or "").lower()` mapped through the
/// approval table (`run_endpoints.py:252-257`). `Err` is the source's
/// `AttributeError` on a truthy non-string (500).
pub fn run_approval_kind(data: &Value) -> Result<&'static str, FrameError> {
    let raw = data
        .as_object()
        .ok_or(FrameError)?
        .get("kind")
        .filter(|value| py_truthy(value));
    let text = match raw {
        None => return Ok("other"),
        Some(Value::String(text)) => text,
        Some(_) => return Err(FrameError),
    };
    Ok(match text.to_lowercase().as_str() {
        "command_execution" => "command_execution",
        "file_change" => "file_change",
        "network_access" => "network_access",
        _ => "other",
    })
}

/// `request.data.get("approval_id")` as a UUID for
/// `update_or_create(id=...)` (`run_endpoints.py:258-268`): missing
/// or null mints a fresh id (the field default), strings parse, and
/// everything else is the source's `ValidationError`/`DataError`
/// (500).
pub fn approval_id(data: &Value) -> Result<Uuid, FrameError> {
    let raw = data.as_object().ok_or(FrameError)?.get("approval_id");
    match raw {
        None | Some(Value::Null) => Ok(Uuid::new_v4()),
        Some(Value::String(text)) => text.parse::<Uuid>().map_err(|_| FrameError),
        Some(_) => Err(FrameError),
    }
}

/// `request.data.get("expires_at")` as an optional timestamptz input
/// (`run_endpoints.py:266`, `chat.py:652`): only a missing key or
/// `null` binds NULL. Both endpoints pass the raw JSON value into
/// `update_or_create` on a plain `DateTimeField(null=True)`
/// (`models.py:1240,1413`), whose `to_python` returns `None` solely
/// for `None` — `""` raises `ValidationError`, `[]`/`{}` raise
/// `TypeError` (both uncaught, 500). Strings bind as text for
/// Postgres to parse; everything else is the source's 500.
pub fn expires_at_text(data: &Value) -> Result<Option<String>, FrameError> {
    let raw = data.as_object().ok_or(FrameError)?.get("expires_at");
    match raw {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) if !text.is_empty() => Ok(Some(text.clone())),
        Some(_) => Err(FrameError),
    }
}

/// `(request.data.get("stream") or "events").lower()` for the stream
/// upgrade (`run_endpoints.py:473`): `None` is a non-dict body (the
/// source's `AttributeError`, 500); `Some(Err)` is an invalid stream
/// name (400 `invalid_stream`).
pub fn stream_param(data: &Value) -> Option<Result<String, FrameError>> {
    let obj = data.as_object()?;
    let raw = obj.get("stream").filter(|value| py_truthy(value));
    let text = match raw {
        None => return Some(Ok("events".to_owned())),
        Some(Value::String(text)) => text.to_lowercase(),
        Some(_) => return None,
    };
    if text == "log" || text == "events" {
        Some(Ok(text))
    } else {
        Some(Err(FrameError))
    }
}

/// The 64KiB event-payload rule (`run_endpoints.py:220-228`):
/// payloads whose `json.dumps` rendering exceeds
/// [`MAX_EVENT_PAYLOAD_BYTES`] are replaced by the truncation marker
/// (keys in source order: `_truncated`, `original_size_bytes`).
pub fn truncate_event_payload(payload: &Value) -> Value {
    let encoded = crate::assistant::events::py_dumps(payload);
    if encoded.len() > MAX_EVENT_PAYLOAD_BYTES {
        serde_json::json!({
            "_truncated": true,
            "original_size_bytes": encoded.len(),
        })
    } else {
        payload.clone()
    }
}

/// `ws_upgrade_ticket:{ticket}` (`run_endpoints.py:493`).
pub fn ticket_key(ticket: &str) -> String {
    format!("ws_upgrade_ticket:{ticket}")
}

/// The ticket payload (`run_endpoints.py:494-501`): `run_id`,
/// `stream`, `runner_id` (`""` when unauthenticated — unreachable
/// past `_resolve`, kept for exactness), `expires_at` as
/// `timezone.now() + 60s` in `isoformat()` (`+00:00`, microseconds).
/// Rendered with `json.dumps` separators (`", "` / `": "`) in source
/// key order — Redis bytes, never served, but the gate diffs them.
pub fn ticket_payload(
    run_id: &Uuid,
    stream: &str,
    runner_id: Option<&Uuid>,
    now: &DateTime<Utc>,
) -> String {
    let expires = (*now + chrono::Duration::seconds(60))
        .to_rfc3339_opts(chrono::SecondsFormat::Micros, false);
    crate::assistant::events::py_dumps(&serde_json::json!({
        "run_id": run_id.to_string(),
        "stream": stream,
        "runner_id": runner_id.map(ToString::to_string).unwrap_or_default(),
        "expires_at": expires,
    }))
}

// ---------------------------------------------------------------------------
// Shared base (`_RunEndpointBase`, `run_endpoints.py:75-105`)
// ---------------------------------------------------------------------------

/// The `_resolve` fetch (`run_endpoints.py:82-91`): the run's identity
/// plus the status the events endpoint's terminal check reads.
pub struct ResolvedRun {
    pub id: Uuid,
    pub runner_id: Option<Uuid>,
    pub status: String,
}

/// `_resolve`: the run row by id, 404 when missing, 403 unless the
/// authenticated runner owns it. `runner` is `None` for anonymous
/// callers, which always resolve "not owned" (403, never 401).
pub async fn resolve_run(
    pool: &PgPool,
    run_id: Uuid,
    runner: Option<&DaemonRunner>,
) -> Result<ResolvedRun, Response> {
    let row: Option<(Uuid, Option<Uuid>, String)> =
        sqlx::query_as(r#"SELECT "id", "runner_id", "status" FROM "agent_run" WHERE "id" = $1"#)
            .bind(run_id)
            .fetch_optional(pool)
            .await
            .map_err(|_| server_error())?;
    let Some((id, runner_id, status)) = row else {
        return Err(json_response(
            StatusCode::NOT_FOUND,
            r#"{"error":"run_not_found"}"#.to_owned(),
        ));
    };
    if !resolve_runner_for_run(runner_id, runner) {
        return Err(json_response(
            StatusCode::FORBIDDEN,
            r#"{"error":"run_not_owned_by_runner"}"#.to_owned(),
        ));
    }
    Ok(ResolvedRun {
        id,
        runner_id,
        status,
    })
}

type LockedRunRow = (Uuid, Option<Uuid>, Option<Uuid>, Option<Uuid>, String);

/// The effects-lock fetch: status, config, error, refusal category,
/// scheduler binding, hooks marker.
type EffectsLockRow = (
    String,
    Value,
    String,
    String,
    Option<Uuid>,
    Option<DateTime<Utc>>,
);

/// The capacity re-read: release marker, executor, runner, pod,
/// workspace.
type CapacityRow = (
    Option<DateTime<Utc>>,
    String,
    Option<Uuid>,
    Option<Uuid>,
    Uuid,
);

/// The `_lock_non_terminal` fetch (`run_endpoints.py:93-99`): the
/// locked row's identity, status, and the requeue facts.
pub struct LockedRun {
    pub id: Uuid,
    pub runner_id: Option<Uuid>,
    pub pod_id: Option<Uuid>,
    pub parent_run_id: Option<Uuid>,
    pub status: String,
}

/// `_lock_non_terminal`: re-read under `FOR UPDATE`, answering
/// `{"ok": true, "terminal": true}` (200, no dedupe rollback — the
/// caller already recorded it) when the row is gone or terminal.
pub async fn lock_non_terminal(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    run_id: Uuid,
) -> Result<LockedRun, Response> {
    let row: Option<LockedRunRow> = sqlx::query_as(
        r#"SELECT "id", "runner_id", "pod_id", "parent_run_id", "status"
           FROM "agent_run" WHERE "id" = $1 LIMIT 1 FOR UPDATE"#,
    )
    .bind(run_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| server_error())?;
    let Some((id, runner_id, pod_id, parent_run_id, status)) = row else {
        return Err(json_response(
            StatusCode::NOT_FOUND,
            r#"{"error":"run_not_found"}"#.to_owned(),
        ));
    };
    if is_terminal_status(&status) {
        return Err(json_response(
            StatusCode::OK,
            r#"{"ok":true,"terminal":true}"#.to_owned(),
        ));
    }
    Ok(LockedRun {
        id,
        runner_id,
        pod_id,
        parent_run_id,
        status,
    })
}

/// `_cancellation_pending` (`run_endpoints.py:101-105`).
pub fn cancellation_pending_response(locked: &LockedRun) -> Option<Response> {
    if locked.status == AgentRunStatus::CancelRequested.value() {
        Some(json_response(
            StatusCode::OK,
            r#"{"ok":true,"cancel_requested":true,"ignored":true}"#.to_owned(),
        ))
    } else {
        None
    }
}

/// `_record_dedupe` (`run_endpoints.py:52-72`): insert the
/// (`run`, `message_id[:128]`) row under a savepoint; a unique
/// violation rolls back to the savepoint and reports a duplicate,
/// keeping the outer transaction valid. An empty key records
/// nothing and reports fresh.
pub async fn record_dedupe(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    run_id: Uuid,
    message_id: &str,
) -> Result<bool, Response> {
    if message_id.is_empty() {
        return Ok(true);
    }
    let key = truncate_chars(message_id, 128);
    sqlx::query("SAVEPOINT run_message_dedupe")
        .execute(&mut **tx)
        .await
        .map_err(|_| server_error())?;
    let inserted = sqlx::query(
        r#"INSERT INTO "run_message_dedupe" ("run_id", "message_id", "created_at")
           VALUES ($1, $2, $3)"#,
    )
    .bind(run_id)
    .bind(&key)
    .bind(Utc::now())
    .execute(&mut **tx)
    .await;
    match inserted {
        Ok(_) => {
            sqlx::query("RELEASE SAVEPOINT run_message_dedupe")
                .execute(&mut **tx)
                .await
                .map_err(|_| server_error())?;
            Ok(true)
        }
        Err(error) => {
            let unique_violation = is_unique_violation(&error);
            sqlx::query("ROLLBACK TO SAVEPOINT run_message_dedupe")
                .execute(&mut **tx)
                .await
                .map_err(|_| server_error())?;
            sqlx::query("RELEASE SAVEPOINT run_message_dedupe")
                .execute(&mut **tx)
                .await
                .map_err(|_| server_error())?;
            if unique_violation {
                Ok(false)
            } else {
                Err(server_error())
            }
        }
    }
}

/// Whether a sqlx error is a unique violation (SQLSTATE 23505).
fn is_unique_violation(error: &sqlx::Error) -> bool {
    error
        .as_database_error()
        .and_then(|db| db.code())
        .is_some_and(|code| code == "23505")
}

// ---------------------------------------------------------------------------
// Comment writes (`IssueComment.objects.create`, both lifecycle paths)
// ---------------------------------------------------------------------------

/// Insert a planned comment: the comment row, its description row, and
/// the link-back (`comment_insert_sql`, `description_insert_sql`,
/// `comment_description_link_sql`). Runs inside the caller's
/// transaction (the pause path) or savepoint (the failure path).
pub async fn insert_comment(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    insert: &lifecycle_kernel::CommentInsert,
) -> Result<(), sqlx::Error> {
    let empty_json = Value::Object(Default::default());
    let empty_list = Value::Array(Vec::new());
    sqlx::query(&lifecycle_kernel::comment_insert_sql())
        .bind(Utc::now())
        .bind(Utc::now())
        .bind(insert.created_by_id)
        .bind(insert.updated_by_id)
        .bind(None::<DateTime<Utc>>)
        .bind(insert.comment_id)
        .bind(insert.project_id)
        .bind(insert.workspace_id)
        .bind(&insert.plan.comment_stripped)
        .bind(&empty_json)
        .bind(&insert.plan.comment_html)
        .bind(None::<Uuid>)
        .bind(&empty_list)
        .bind(&empty_list)
        .bind(insert.issue_id)
        .bind(insert.actor_id)
        .bind("INTERNAL")
        .bind(None::<String>)
        .bind(None::<String>)
        .bind(insert.plan.speaker.value())
        .bind(&insert.plan.speaker_label)
        .bind(insert.plan.speaker_agent_run_id)
        .bind(None::<DateTime<Utc>>)
        .bind(None::<Uuid>)
        .execute(&mut **tx)
        .await?;
    let description_id = Uuid::new_v4();
    sqlx::query(&lifecycle_kernel::description_insert_sql())
        .bind(Utc::now())
        .bind(Utc::now())
        .bind(insert.created_by_id)
        .bind(insert.updated_by_id)
        .bind(None::<DateTime<Utc>>)
        .bind(description_id)
        .bind(insert.workspace_id)
        .bind(insert.project_id)
        .bind(&empty_json)
        .bind(&insert.plan.comment_html)
        .bind(None::<Vec<u8>>)
        .bind(&insert.plan.description_stripped)
        .execute(&mut **tx)
        .await?;
    sqlx::query(lifecycle_kernel::comment_description_link_sql())
        .bind(description_id)
        .bind(insert.comment_id)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// L4 executors (`run_lifecycle.py` + `agent_run_finalization.py`)
// ---------------------------------------------------------------------------

/// Build the enrich signals (`RunnerInfo`) from the authenticated
/// daemon runner for `FAILED` terminal frames.
fn runner_info(runner: &DaemonRunner) -> RunnerInfo<'_> {
    let dev_machine = match (&runner.dev_label, &runner.dev_host_label) {
        (None, None) => None,
        (label, host_label) => Some(DevMachineInfo {
            label: label.as_deref(),
            host_label: host_label.as_deref(),
        }),
    };
    RunnerInfo {
        name: Some(runner.name.as_str()),
        host_label: Some(runner.host_label.as_str()),
        capabilities: Some(&runner.capabilities),
        dev_machine,
    }
}

/// `_matching_live_state` (`run_lifecycle.py:73-77`): the usage facts,
/// or `None` when the runner recorded no snapshot for this run.
async fn live_state_facts(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    runner_id: Uuid,
    run_id: Uuid,
) -> Result<Option<LiveStateUsageFacts>, Response> {
    let row: Option<(Value, Option<String>)> = sqlx::query_as(
        r#"SELECT "usage", "llm_model" FROM "runner_live_state"
           WHERE ("observed_run_id" = $1 AND "runner_id" = $2) LIMIT 1"#,
    )
    .bind(run_id)
    .bind(runner_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| server_error())?;
    Ok(row.map(|(usage, llm_model)| LiveStateUsageFacts { usage, llm_model }))
}

/// Bind one finalize `SET` clause, in values order. `Now` is the
/// statement's single timestamp; `Null` binds the column's typed
/// null (the only nulls the terminal path writes are the queue
/// position and the two markers).
fn bind_finalize_value<'q>(
    query: sqlx::query::Query<'q, sqlx::Postgres, sqlx::postgres::PgArguments>,
    column: &str,
    value: &pidash_services::runner_runs::SetValue,
    now: &DateTime<Utc>,
) -> sqlx::query::Query<'q, sqlx::Postgres, sqlx::postgres::PgArguments> {
    use pidash_services::runner_runs::SetValue;
    match (column, value) {
        (_, SetValue::Now) => query.bind(*now),
        (_, SetValue::Text(text)) => query.bind(text.clone()),
        (_, SetValue::Json(payload)) => query.bind(payload.clone()),
        ("queue_position", SetValue::Null) => query.bind(None::<i16>),
        (_, SetValue::Null) => query.bind(None::<DateTime<Utc>>),
    }
}

/// `finalize_run_terminal` (`run_lifecycle.py:402-463`) inside the
/// endpoint transaction: plan, first-writer-wins lock, merge,
/// update, cloud-only terminal event. Returns the post-commit
/// effects (`_publish_effects`); a lost race logs and returns none.
#[allow(clippy::too_many_arguments)]
pub async fn execute_finalize_terminal(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    runner: &DaemonRunner,
    run_id: Uuid,
    status: AgentRunStatus,
    done_payload: &Value,
    error_detail: &Value,
    refusal_category: &Value,
    tokens: &Value,
    model: &Value,
) -> Result<Vec<LifecycleEffect>, Response> {
    let live = live_state_facts(tx, runner.id, run_id).await?;
    let info = runner_info(runner);
    let inputs = lifecycle_kernel::TerminalUpdateInputs {
        status,
        done_payload,
        error_detail,
        refusal_category,
        tokens,
        model,
        live: live.as_ref(),
        runner: Some(&info),
    };
    let mut values =
        lifecycle_kernel::plan_terminal_finalize(&inputs).map_err(|_| server_error())?;
    // `lock_run_for_finalize_sql(true, false)`: same predicates and
    // lock, targeted columns (id, executor, stored payload).
    let locked: Option<(Uuid, String, Option<Value>)> = sqlx::query_as(
        r#"SELECT "id", "executor_kind", "done_payload" FROM "agent_run"
           WHERE ("agent_run"."id" = $1
             AND NOT ("agent_run"."status" IN ($2, $3, $4, $5, $6))
             AND "agent_run"."runner_id" = $7)
           ORDER BY "agent_run"."created_at" DESC LIMIT 1 FOR UPDATE"#,
    )
    .bind(run_id)
    .bind(TERMINAL_RUN_STATUSES[0].value())
    .bind(TERMINAL_RUN_STATUSES[1].value())
    .bind(TERMINAL_RUN_STATUSES[2].value())
    .bind(TERMINAL_RUN_STATUSES[3].value())
    .bind(TERMINAL_RUN_STATUSES[4].value())
    .bind(runner.id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| server_error())?;
    let Some((locked_id, executor_kind, stored_payload)) = locked else {
        tracing::info!(
            run_id = %run_id,
            "run_lifecycle: ignoring late terminal transition for closed run"
        );
        return Ok(Vec::new());
    };
    finalize_kernel::apply_done_payload_merge(
        &mut values,
        stored_payload.as_ref().unwrap_or(&Value::Null),
    );
    let now = Utc::now();
    let update_sql = finalize_kernel::finalize_update_sql(&values);
    let mut update = sqlx::query(&update_sql);
    for clause in &values.clauses {
        update = bind_finalize_value(update, clause.column, &clause.value, &now);
    }
    update
        .bind(locked_id)
        .execute(&mut **tx)
        .await
        .map_err(|_| server_error())?;
    if executor_kind == AgentExecutorKind::CloudAgent.value() {
        let exists: Option<i32> = sqlx::query_scalar(finalize_kernel::terminal_event_exists_sql())
            .bind(run_id)
            .bind("terminal")
            .fetch_optional(&mut **tx)
            .await
            .map_err(|_| server_error())?;
        if exists.is_none() {
            let max_seq: Option<i32> =
                sqlx::query_scalar(finalize_kernel::terminal_event_max_seq_sql())
                    .bind(run_id)
                    .fetch_optional(&mut **tx)
                    .await
                    .map_err(|_| server_error())?
                    .flatten();
            let event = finalize_kernel::plan_terminal_event(
                max_seq,
                status,
                &finalize_kernel::finalize_error_code(&values),
            );
            sqlx::query(&finalize_kernel::terminal_event_insert_sql())
                .bind(run_id)
                .bind(event.seq)
                .bind("terminal")
                .bind(&event.payload)
                .bind(Utc::now())
                .execute(&mut **tx)
                .await
                .map_err(|_| server_error())?;
        }
    }
    Ok(finalize_kernel::plan_publish_effects(run_id).to_vec())
}

/// `apply_run_paused` (`run_lifecycle.py:158-244`) inside the endpoint
/// transaction: live-state read, pause lock + phase-1 update, phase-2
/// re-read, pause comment. Returns the post-commit effects
/// (`_pause_and_drain`); a lost race or a vanished row returns none
/// (the drain is skipped too, `run_lifecycle.py:196-199`).
pub async fn execute_apply_paused(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ports: &impl RunnerPorts,
    runner: &DaemonRunner,
    run_id: Uuid,
    payload: &Value,
    tokens: &Value,
    model: &Value,
) -> Result<Vec<LifecycleEffect>, Response> {
    let live = live_state_facts(tx, runner.id, run_id).await?;
    // `lock_run_for_pause_sql`: same predicates and lock, targeted
    // columns (id, stored payload).
    let pausing: Option<(Uuid, Option<Value>)> = sqlx::query_as(
        r#"SELECT "id", "done_payload" FROM "agent_run"
           WHERE ("agent_run"."id" = $1 AND "agent_run"."runner_id" = $2
             AND NOT ("agent_run"."status" IN ($3, $4, $5, $6, $7))
             AND NOT ("agent_run"."status" = $8))
           ORDER BY "agent_run"."created_at" DESC LIMIT 1 FOR UPDATE"#,
    )
    .bind(run_id)
    .bind(runner.id)
    .bind(TERMINAL_RUN_STATUSES[0].value())
    .bind(TERMINAL_RUN_STATUSES[1].value())
    .bind(TERMINAL_RUN_STATUSES[2].value())
    .bind(TERMINAL_RUN_STATUSES[3].value())
    .bind(TERMINAL_RUN_STATUSES[4].value())
    .bind(AgentRunStatus::CancelRequested.value())
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| server_error())?;
    let Some((pausing_id, stored_payload)) = pausing else {
        return Ok(Vec::new());
    };
    let plan = lifecycle_kernel::plan_pause_update(
        stored_payload.as_ref().unwrap_or(&Value::Null),
        payload,
        tokens,
        model,
        live.as_ref(),
    );
    let pause_sql = lifecycle_kernel::pause_update_sql(&plan);
    let mut update = sqlx::query(&pause_sql)
        .bind(AgentRunStatus::PausedAwaitingInput.value())
        .bind(&plan.done_payload);
    if let Some(usage) = &plan.usage {
        update = update.bind(usage);
    }
    if let Some(model) = &plan.llm_model {
        update = update.bind(model);
    }
    update
        .bind(pausing_id)
        .execute(&mut **tx)
        .await
        .map_err(|_| server_error())?;
    // Phase-2 re-read (`pause_reread_sql`): a missing row returns
    // before the `on_commit` registration — no comment, no drain.
    let work_item: Option<Option<Uuid>> =
        sqlx::query_scalar(r#"SELECT "work_item_id" FROM "agent_run" WHERE "agent_run"."id" = $1"#)
            .bind(run_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(|_| server_error())?;
    let Some(work_item_id) = work_item else {
        return Ok(Vec::new());
    };
    if let Some(issue_id) = work_item_id {
        let comment = lifecycle_kernel::plan_pause_comment(payload).map_err(|_| server_error())?;
        if let Some(plan) = comment {
            // `run.work_item.project` / `.workspace` through the
            // `select_related` join: a dangling issue is the source's
            // `AttributeError` on `None` (500).
            let issue: Option<(Uuid, Uuid)> = sqlx::query_as(
                r#"SELECT "project_id", "workspace_id" FROM "issues" WHERE "id" = $1"#,
            )
            .bind(issue_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(|_| server_error())?;
            let Some((project_id, workspace_id)) = issue else {
                return Err(server_error());
            };
            let actor_id = ports
                .agent_system_user_id()
                .await
                .map_err(|_| server_error())?;
            insert_comment(
                tx,
                &lifecycle_kernel::CommentInsert {
                    comment_id: Uuid::new_v4(),
                    issue_id,
                    project_id,
                    workspace_id,
                    actor_id: Some(actor_id),
                    created_by_id: None,
                    updated_by_id: None,
                    plan,
                },
            )
            .await
            .map_err(|_| server_error())?;
        }
    }
    Ok(vec![LifecycleEffect::PauseAndDrain {
        run_id,
        runner_id: runner.id,
    }])
}

/// `apply_run_resume_unavailable` (`run_lifecycle.py:247-295`) inside
/// the endpoint transaction: the caller always passes its locked row,
/// so the fallback lock never runs here. Returns the post-commit pod
/// drain, if the run has a pod.
pub async fn execute_requeue(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    locked: &LockedRun,
) -> Result<Vec<LifecycleEffect>, Response> {
    let parent_thread_id = match locked.parent_run_id {
        Some(parent_id) => {
            // `run.parent_run.thread_id`: a dangling parent id is the
            // source's `DoesNotExist` (500); impossible under the FK.
            let thread: Option<Option<String>> =
                sqlx::query_scalar(r#"SELECT "thread_id" FROM "agent_run" WHERE "id" = $1"#)
                    .bind(parent_id)
                    .fetch_optional(&mut **tx)
                    .await
                    .map_err(|_| server_error())?;
            match thread {
                Some(thread) => thread,
                None => return Err(server_error()),
            }
        }
        None => None,
    };
    let plan = lifecycle_kernel::plan_requeue_from_locked(&lifecycle_kernel::RequeueRunFacts {
        id: locked.id,
        pod_id: locked.pod_id,
        parent_run_id: locked.parent_run_id,
        parent_thread_id,
    });
    // The parent thread clears *before* the run row, in the same
    // transaction (`run_lifecycle.py:279-290`).
    if let Some(parent_id) = plan.parent_thread_clear {
        let cleared = sqlx::query(lifecycle_kernel::parent_thread_clear_sql())
            .bind("")
            .bind(parent_id)
            .execute(&mut **tx)
            .await
            .map_err(|_| server_error())?;
        if cleared.rows_affected() == 0 {
            // `save()` on a vanished row is `DatabaseError` (500).
            return Err(server_error());
        }
    }
    let updated = sqlx::query(lifecycle_kernel::requeue_update_sql())
        .bind(AgentRunStatus::Queued.value())
        .bind(None::<Uuid>)
        .bind(None::<Uuid>)
        .bind(None::<DateTime<Utc>>)
        .bind(None::<i16>)
        .bind(locked.id)
        .execute(&mut **tx)
        .await
        .map_err(|_| server_error())?;
    if updated.rows_affected() == 0 {
        return Err(server_error());
    }
    Ok(plan.after_commit.into_iter().collect())
}

/// `apply_assign_rejected_busy` (`run_lifecycle.py:298-322`): flip the
/// runner to `BUSY` first (a queryset `update` — zero rows are fine),
/// then the shared requeue.
pub async fn execute_assign_rejected_busy(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    runner_id: Uuid,
    locked: &LockedRun,
) -> Result<Vec<LifecycleEffect>, Response> {
    sqlx::query(lifecycle_kernel::runner_busy_update_sql())
        .bind("busy")
        .bind(runner_id)
        .execute(&mut **tx)
        .await
        .map_err(|_| server_error())?;
    execute_requeue(tx, locked).await
}

// ---------------------------------------------------------------------------
// Terminal effects (`apply_terminal_effects`) + post-commit drain
// ---------------------------------------------------------------------------

/// How [`execute_terminal_effects`] failed. The caller
/// ([`drain_lifecycle_effects`]) logs every variant — the whole call
/// is isolated in `_publish_effects`.
#[derive(Debug)]
pub enum TerminalEffectsError {
    Database(String),
    Ports(String),
}

/// `apply_terminal_effects` (`agent_run_finalization.py:105-199`):
/// hooks once behind `terminal_hooks_applied_at` (failure comment
/// and scheduler hook each in their own savepoint), a pending
/// handoff after the hooks transaction, then capacity release
/// at-least-once. Returns `false` when the run is not terminal.
/// `Err` propagates to the `_publish_effects` log (isolated there),
/// except the capacity dispatch/drains, which propagate with the
/// marker unset for the reconciler — also via `Err`, since the
/// caller logs without writing anything.
pub async fn execute_terminal_effects(
    pool: &PgPool,
    ports: &impl RunnerPorts,
    run_id: Uuid,
) -> Result<bool, TerminalEffectsError> {
    let db = |error: sqlx::Error| TerminalEffectsError::Database(error.to_string());
    let mut hooks = pool.begin().await.map_err(db)?;
    // Lock order: issue → run (`finalization.py:115-125`).
    let work_item_id: Option<Uuid> =
        sqlx::query_scalar(finalize_kernel::select_run_work_item_id_sql())
            .bind(run_id)
            .fetch_optional(&mut *hooks)
            .await
            .map_err(db)?
            .flatten();
    if let Some(issue_id) = work_item_id {
        sqlx::query(
            r#"SELECT 1 AS "a" FROM "issues" WHERE "issues"."id" = $1
               ORDER BY "issues"."created_at" DESC LIMIT 1 FOR UPDATE OF "issues""#,
        )
        .bind(issue_id)
        .fetch_optional(&mut *hooks)
        .await
        .map_err(db)?;
    }
    // `lock_run_for_effects_sql`: same predicates and lock, targeted
    // columns (status, config, error, category, binding, marker).
    let locked: Option<EffectsLockRow> = sqlx::query_as(
        r#"SELECT "status", "run_config", "error", "refusal_category",
                  "scheduler_binding_id", "terminal_hooks_applied_at"
               FROM "agent_run"
               WHERE ("agent_run"."id" = $1 AND "agent_run"."status" IN ($2, $3, $4, $5, $6))
               ORDER BY "agent_run"."created_at" DESC LIMIT 1 FOR UPDATE OF "agent_run""#,
    )
    .bind(run_id)
    .bind(TERMINAL_RUN_STATUSES[0].value())
    .bind(TERMINAL_RUN_STATUSES[1].value())
    .bind(TERMINAL_RUN_STATUSES[2].value())
    .bind(TERMINAL_RUN_STATUSES[3].value())
    .bind(TERMINAL_RUN_STATUSES[4].value())
    .fetch_optional(&mut *hooks)
    .await
    .map_err(db)?;
    let Some((status, run_config, error, refusal_category, binding_id, hooks_applied_at)) = locked
    else {
        return Ok(false);
    };
    let status = AgentRunStatus::from_value(&status)
        .ok_or_else(|| TerminalEffectsError::Database("unknown terminal status".to_owned()))?;
    let mut pending_handoff = false;
    if hooks_applied_at.is_none() {
        let handoff = lifecycle_kernel::has_project_move_handoff(&run_config);
        if status == AgentRunStatus::Failed && !handoff {
            // Best-effort comment behind a savepoint: a comment
            // database failure must not poison the hooks
            // transaction or strand terminal capacity.
            sqlx::query("SAVEPOINT failure_comment")
                .execute(&mut *hooks)
                .await
                .map_err(db)?;
            let commented = post_failure_comment(&mut hooks, ports, run_id, &error).await;
            match commented {
                Ok(()) => {
                    sqlx::query("RELEASE SAVEPOINT failure_comment")
                        .execute(&mut *hooks)
                        .await
                        .map_err(db)?;
                }
                Err(error) => {
                    sqlx::query("ROLLBACK TO SAVEPOINT failure_comment")
                        .execute(&mut *hooks)
                        .await
                        .map_err(db)?;
                    sqlx::query("RELEASE SAVEPOINT failure_comment")
                        .execute(&mut *hooks)
                        .await
                        .map_err(db)?;
                    tracing::error!(
                        ?error,
                        run_id = %run_id,
                        "run_lifecycle: failed to post failure comment"
                    );
                }
            }
        }
        if handoff {
            pending_handoff = true;
        } else if let Err(error) = ports.post_run_orchestration(run_id).await {
            // Unisolated in Python (the isolation lives inside the
            // three sub-calls): abort the hooks transaction and
            // propagate to the `_publish_effects` log.
            return Err(TerminalEffectsError::Ports(format!("{error:?}")));
        }
        if let Some(binding_id) = binding_id {
            // The savepoint pair always runs around the hook call;
            // the `UPDATE` only when the hook rewrites `last_error`.
            sqlx::query("SAVEPOINT scheduler_hook")
                .execute(&mut *hooks)
                .await
                .map_err(db)?;
            let hooked =
                apply_scheduler_hook(&mut hooks, binding_id, status, &refusal_category, &error)
                    .await;
            match hooked {
                Ok(()) => {
                    sqlx::query("RELEASE SAVEPOINT scheduler_hook")
                        .execute(&mut *hooks)
                        .await
                        .map_err(db)?;
                }
                Err(error) => {
                    sqlx::query("ROLLBACK TO SAVEPOINT scheduler_hook")
                        .execute(&mut *hooks)
                        .await
                        .map_err(db)?;
                    sqlx::query("RELEASE SAVEPOINT scheduler_hook")
                        .execute(&mut *hooks)
                        .await
                        .map_err(db)?;
                    tracing::error!(
                        ?error,
                        run_id = %run_id,
                        "scheduler.terminate_hook: failed"
                    );
                }
            }
        }
        let marked = sqlx::query(finalize_kernel::update_hooks_marker_sql())
            .bind(Utc::now())
            .bind(run_id)
            .execute(&mut *hooks)
            .await
            .map_err(db)?;
        if marked.rows_affected() == 0 {
            // `save()` on a vanished row is `DatabaseError`.
            return Err(TerminalEffectsError::Database(
                "hooks marker matched no row".to_owned(),
            ));
        }
        hooks.commit().await.map_err(db)?;
    } else {
        hooks.rollback().await.map_err(db)?;
    }

    if pending_handoff {
        // After the hooks transaction (lock order), isolated: the
        // terminal transition already committed, so a handoff
        // failure must not strand capacity release.
        if let Err(error) = ports.complete_project_move_handoff(run_id).await {
            tracing::error!(
                ?error,
                run_id = %run_id,
                "failed to complete project-move handoff"
            );
        }
    }

    // Capacity re-read (`select_run_for_capacity_sql`, a `.get()` —
    // a missing row raises, unguarded in the source).
    let capacity: Option<CapacityRow> = sqlx::query_as(
        r#"SELECT "terminal_capacity_released_at", "executor_kind",
                  "runner_id", "pod_id", "workspace_id"
               FROM "agent_run" WHERE "agent_run"."id" = $1"#,
    )
    .bind(run_id)
    .fetch_optional(pool)
    .await
    .map_err(db)?;
    let Some((released_at, executor_kind, runner_id, pod_id, workspace_id)) = capacity else {
        return Err(TerminalEffectsError::Database(
            "capacity re-read found no row".to_owned(),
        ));
    };
    if released_at.is_none() {
        // Drains and dispatch run *before* the marker write,
        // unisolated — a failure leaves the marker unset for the
        // reconciler.
        if executor_kind == AgentExecutorKind::CloudAgent.value() {
            ports
                .dispatch_waiting(workspace_id)
                .await
                .map_err(|error| TerminalEffectsError::Ports(format!("{error:?}")))?;
        } else {
            if let Some(runner_id) = runner_id {
                ports
                    .drain_for_runner_by_id(runner_id)
                    .await
                    .map_err(|error| TerminalEffectsError::Ports(format!("{error:?}")))?;
            }
            if let Some(pod_id) = pod_id {
                ports
                    .drain_pod_by_id(pod_id)
                    .await
                    .map_err(|error| TerminalEffectsError::Ports(format!("{error:?}")))?;
            }
        }
        sqlx::query(finalize_kernel::update_capacity_marker_sql())
            .bind(Utc::now())
            .bind(run_id)
            .execute(pool)
            .await
            .map_err(db)?;
    }
    Ok(true)
}

/// `_post_failure_comment` (`run_lifecycle.py:341-399`) inside the
/// failure savepoint: suppressed for infra noise, silent without a
/// work item or on a dedupe hit, otherwise one `SYSTEM` comment from
/// the agent user. `Err` rolls back to the savepoint (logged by the
/// caller).
async fn post_failure_comment(
    hooks: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ports: &impl RunnerPorts,
    run_id: Uuid,
    error: &str,
) -> Result<(), TerminalEffectsError> {
    let db = |error: sqlx::Error| TerminalEffectsError::Database(error.to_string());
    // Suppression returns before any SQL (`finalization.py:136-147`).
    let Some(plan) = lifecycle_kernel::plan_failure_comment(error, run_id) else {
        return Ok(());
    };
    // `failure_reread_sql`: run + work item + project, first row.
    let reread: Option<(Option<Uuid>, Option<Uuid>, Option<Uuid>)> = sqlx::query_as(
        r#"SELECT "agent_run"."work_item_id", "issues"."project_id", "issues"."workspace_id"
           FROM "agent_run" LEFT OUTER JOIN "issues"
             ON ("agent_run"."work_item_id" = "issues"."id")
           WHERE "agent_run"."id" = $1
           ORDER BY "agent_run"."created_at" DESC LIMIT 1"#,
    )
    .bind(run_id)
    .fetch_optional(&mut **hooks)
    .await
    .map_err(db)?;
    let (work_item_id, project_id, workspace_id) = match reread {
        Some((Some(work_item), Some(project), Some(workspace))) => (work_item, project, workspace),
        _ => return Ok(()),
    };
    let dupe: Option<i32> = sqlx::query_scalar(lifecycle_kernel::comment_dedupe_exists_sql())
        .bind(work_item_id)
        .bind(run_id)
        .bind(lifecycle_kernel::CommentSpeaker::System.value())
        .fetch_optional(&mut **hooks)
        .await
        .map_err(db)?;
    if dupe.is_some() {
        return Ok(());
    }
    let actor_id = ports
        .agent_system_user_id()
        .await
        .map_err(|error| TerminalEffectsError::Ports(format!("{error:?}")))?;
    insert_comment(
        hooks,
        &lifecycle_kernel::CommentInsert {
            comment_id: Uuid::new_v4(),
            issue_id: work_item_id,
            project_id,
            workspace_id,
            actor_id: Some(actor_id),
            created_by_id: None,
            updated_by_id: None,
            plan,
        },
    )
    .await
    .map_err(db)?;
    Ok(())
}

/// `update_scheduler_binding_on_terminate` (`scheduler_hook.py:17-42`)
/// inside the scheduler savepoint: no write on a missing binding or
/// an unchanged value; zero rows on a planned write are
/// `DatabaseError` (logged by the caller).
async fn apply_scheduler_hook(
    hooks: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    binding_id: Uuid,
    status: AgentRunStatus,
    refusal_category: &str,
    error: &str,
) -> Result<(), TerminalEffectsError> {
    use pidash_services::runner_runs::SchedulerHookPlan;
    let db = |error: sqlx::Error| TerminalEffectsError::Database(error.to_string());
    let last_error: Option<String> =
        sqlx::query_scalar(r#"SELECT "last_error" FROM "scheduler_bindings" WHERE "id" = $1"#)
            .bind(binding_id)
            .fetch_optional(&mut **hooks)
            .await
            .map_err(db)?
            .flatten();
    let facts = last_error.map(|last_error| pidash_services::runner_runs::BindingFacts {
        id: binding_id,
        last_error,
    });
    let plan = pidash_services::runner_runs::plan_scheduler_hook(
        facts.as_ref(),
        status,
        refusal_category,
        error,
    );
    let message = match plan {
        SchedulerHookPlan::Noop => return Ok(()),
        SchedulerHookPlan::ClearError { .. } => String::new(),
        SchedulerHookPlan::SetError { message, .. } => message,
    };
    let updated = sqlx::query(pidash_services::runner_runs::scheduler_binding_update_sql())
        .bind(&message)
        .bind(Utc::now())
        .bind(binding_id)
        .execute(&mut **hooks)
        .await
        .map_err(db)?;
    if updated.rows_affected() == 0 {
        return Err(TerminalEffectsError::Database(
            "scheduler binding matched no row".to_owned(),
        ));
    }
    Ok(())
}

/// Drain collected [`LifecycleEffect`]s after commit, in order, with
/// Python's per-site isolation:
/// * `PauseAndDrain`'s re-read miss skips only the orchestration; the
///   runner drain runs unconditionally (`_pause_and_drain`,
///   `run_lifecycle.py:230-238`). Both propagate — a 500 with
///   committed rows.
/// * `PostRunOrchestration`, `FireTick`, `DrainRunner`, `DrainPod`,
///   `DispatchWaiting` propagate (direct `on_commit` registrations
///   or unisolated inline calls).
/// * `PublishTerminalEffects` and `ApplyTerminalEffectsInline` are
///   isolated with `_publish_effects`' log lines; `DrainTasks`-style
///   chat effects never appear here.
///
/// With [`LivePorts`] every port call succeeds, so the `Err` arms are
/// unreachable in production — they exist for the D-14/D-12/D-11
/// providers and for fakes in tests.
pub async fn drain_lifecycle_effects(
    pool: &PgPool,
    ports: &impl RunnerPorts,
    effects: Vec<LifecycleEffect>,
) -> Result<(), Response> {
    for effect in effects {
        match effect {
            LifecycleEffect::PauseAndDrain { run_id, runner_id } => {
                let present: Option<Uuid> =
                    sqlx::query_scalar(r#"SELECT "id" FROM "agent_run" WHERE "id" = $1"#)
                        .bind(run_id)
                        .fetch_optional(pool)
                        .await
                        .map_err(|_| server_error())?;
                if present.is_some() {
                    ports
                        .post_run_orchestration(run_id)
                        .await
                        .map_err(|_| server_error())?;
                }
                ports
                    .drain_for_runner_by_id(runner_id)
                    .await
                    .map_err(|_| server_error())?;
            }
            LifecycleEffect::PostRunOrchestration { run_id } => {
                ports
                    .post_run_orchestration(run_id)
                    .await
                    .map_err(|_| server_error())?;
            }
            LifecycleEffect::FireTick { ticker_id } => {
                ports
                    .emit_fire_tick(ticker_id)
                    .await
                    .map_err(|_| server_error())?;
            }
            LifecycleEffect::DrainRunner { runner_id } => {
                ports
                    .drain_for_runner_by_id(runner_id)
                    .await
                    .map_err(|_| server_error())?;
            }
            LifecycleEffect::DrainPod { pod_id } => {
                ports
                    .drain_pod_by_id(pod_id)
                    .await
                    .map_err(|_| server_error())?;
            }
            LifecycleEffect::DispatchWaiting { workspace_id } => {
                ports
                    .dispatch_waiting(workspace_id)
                    .await
                    .map_err(|_| server_error())?;
            }
            LifecycleEffect::CompleteProjectMoveHandoff { run_id } => {
                if let Err(error) = ports.complete_project_move_handoff(run_id).await {
                    tracing::error!(
                        ?error,
                        run_id = %run_id,
                        "failed to complete project-move handoff"
                    );
                }
            }
            LifecycleEffect::PublishTerminalEffects { run_id } => {
                if let Err(error) = ports.emit_terminal_effects(run_id).await {
                    tracing::error!(
                        ?error,
                        run_id = %run_id,
                        "failed to publish terminal effects"
                    );
                }
            }
            LifecycleEffect::ApplyTerminalEffectsInline { run_id } => {
                if let Err(error) = execute_terminal_effects(pool, ports, run_id).await {
                    tracing::error!(
                        ?error,
                        run_id = %run_id,
                        "failed to apply terminal effects"
                    );
                }
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Handlers (12 run endpoints)
// ---------------------------------------------------------------------------

/// The request head every run endpoint resolves: the authenticated
/// runner (`None` when anonymous), the parsed run id, the DRF
/// `request.data` body, and the idempotency key.
struct DaemonRequest {
    runner: Option<DaemonRunner>,
    run_id: Uuid,
    data: Value,
    key: String,
}

fn run_not_found() -> Response {
    json_response(
        StatusCode::NOT_FOUND,
        r#"{"error":"run_not_found"}"#.to_owned(),
    )
}

/// Authenticate (DRF `initial`, before parsing) and read
/// `request.data`. Used by the eight endpoints that touch the body;
/// the other four ([`daemon_preamble_nobody`]) never parse, so a
/// malformed body still acks there, exactly as in Python.
async fn daemon_preamble(
    state: &AppState,
    run_id_raw: &str,
    req: Request<axum::body::Body>,
) -> Result<(PgPool, DaemonRequest), Response> {
    let pool = pool_of(state)?.clone();
    let run_id: Uuid = run_id_raw.parse().map_err(|_| run_not_found())?;
    let secret = state.settings().secret_key.clone();
    let runner = authenticate_daemon(&pool, secret.as_bytes(), req.headers()).await?;
    let key = idempotency_key(req.headers());
    let data = read_request_data(state, req).await?;
    Ok((
        pool,
        DaemonRequest {
            runner,
            run_id,
            data,
            key,
        },
    ))
}

/// [`daemon_preamble`] without the body read, for `accept`, `queued`,
/// `awaiting-reauth` and `resumed`, which never touch `request.data`.
async fn daemon_preamble_nobody(
    state: &AppState,
    run_id_raw: &str,
    headers: &HeaderMap,
) -> Result<(PgPool, DaemonRunner, Uuid, String), Response> {
    let pool = pool_of(state)?.clone();
    let run_id: Uuid = run_id_raw.parse().map_err(|_| run_not_found())?;
    let secret = state.settings().secret_key.clone();
    let runner = authenticate_daemon(&pool, secret.as_bytes(), headers).await?;
    let key = idempotency_key(headers);
    // `_resolve` runs next and 403s anonymous callers; unwrap here
    // would mistranslate that into a 500, so resolve inline.
    let run = resolve_run(&pool, run_id, runner.as_ref()).await?;
    let Some(runner) = runner else {
        // Unreachable: `resolve_run` 403s when `runner` is `None`.
        return Err(server_error());
    };
    Ok((pool, runner, run.id, key))
}

/// Commit the endpoint transaction, mapping a commit failure to the
/// 500 the source's `Atomic.__exit__` raises.
async fn commit_tx(tx: sqlx::Transaction<'_, sqlx::Postgres>) -> Result<(), Response> {
    tx.commit().await.map_err(|_| server_error())
}

/// `POST runs/<run_id>/accept/` (`run_endpoints.py:108-127`): dedupe,
/// lock, cancel check, then `RUNNING` with the worktree position
/// cleared.
pub async fn run_accept(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let (pool, _runner, run_id, key) = match daemon_preamble_nobody(&state, &run_id, &headers).await
    {
        Ok(preamble) => preamble,
        Err(response) => return response,
    };
    let mut tx = match pool.begin().await {
        Ok(tx) => tx,
        Err(_) => return server_error(),
    };
    match record_dedupe(&mut tx, run_id, &key).await {
        Ok(true) => {}
        Ok(false) => {
            if commit_tx(tx).await.is_err() {
                return server_error();
            }
            return json_response(StatusCode::OK, r#"{"ok":true,"duplicate":true}"#.to_owned());
        }
        Err(response) => return response,
    }
    let locked = match lock_non_terminal(&mut tx, run_id).await {
        Ok(locked) => locked,
        Err(closed) => {
            // Normal block exit: the dedupe commits with the answer.
            if commit_tx(tx).await.is_err() {
                return server_error();
            }
            return closed;
        }
    };
    if let Some(pending) = cancellation_pending_response(&locked) {
        if commit_tx(tx).await.is_err() {
            return server_error();
        }
        return pending;
    }
    if sqlx::query(
        r#"UPDATE "agent_run" SET "status" = $1, "queue_position" = $2
           WHERE "agent_run"."id" = $3"#,
    )
    .bind(AgentRunStatus::Running.value())
    .bind(None::<i16>)
    .bind(locked.id)
    .execute(&mut *tx)
    .await
    .is_err()
    {
        return server_error();
    }
    if commit_tx(tx).await.is_err() {
        return server_error();
    }
    json_response(StatusCode::OK, r#"{"ok":true}"#.to_owned())
}

/// `POST runs/<run_id>/queued/` (`run_endpoints.py:130-150`): the
/// retired worktree-queue notice — acknowledge and drop, no side
/// effect, no dedupe, no lock, no body read.
pub async fn run_queued(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let (pool, _runner, _run_id, _key) =
        match daemon_preamble_nobody(&state, &run_id, &headers).await {
            Ok(preamble) => preamble,
            Err(response) => return response,
        };
    let _ = pool;
    json_response(StatusCode::OK, r#"{"ok":true,"ignored":true}"#.to_owned())
}

/// `POST runs/<run_id>/started/` (`run_endpoints.py:153-191`): dedupe,
/// lock, cancel check, then the `RUNNING` stamp with thread ids,
/// session metadata, and the optional model.
pub async fn run_started(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    req: Request<axum::body::Body>,
) -> Response {
    let (pool, preamble) = match daemon_preamble(&state, &run_id, req).await {
        Ok(preamble) => preamble,
        Err(response) => return response,
    };
    let run = match resolve_run(&pool, preamble.run_id, preamble.runner.as_ref()).await {
        Ok(run) => run,
        Err(response) => return response,
    };
    let mut tx = match pool.begin().await {
        Ok(tx) => tx,
        Err(_) => return server_error(),
    };
    match record_dedupe(&mut tx, run.id, &preamble.key).await {
        Ok(true) => {}
        Ok(false) => {
            if commit_tx(tx).await.is_err() {
                return server_error();
            }
            return json_response(StatusCode::OK, r#"{"ok":true,"duplicate":true}"#.to_owned());
        }
        Err(response) => return response,
    }
    let locked = match lock_non_terminal(&mut tx, run.id).await {
        Ok(locked) => locked,
        Err(closed) => {
            if commit_tx(tx).await.is_err() {
                return server_error();
            }
            return closed;
        }
    };
    if let Some(pending) = cancellation_pending_response(&locked) {
        if commit_tx(tx).await.is_err() {
            return server_error();
        }
        return pending;
    }
    let data = preamble.data.as_object();
    let get = |key: &str| data.and_then(|obj| obj.get(key)).unwrap_or(&Value::Null);
    // A non-dict body is the source's `AttributeError` on `.get`
    // (500) — but only *here*, after the cancel check.
    if preamble.data.as_object().is_none() {
        return server_error();
    }
    let thread_id = match frame_text(get("thread_id"), 128) {
        Ok(thread_id) => thread_id,
        Err(response) => return response,
    };
    let local_thread_id = match get("local_thread_id") {
        value if !py_truthy(value) => thread_id.clone(),
        Value::String(text) => truncate_chars(text, 128),
        _ => return server_error(),
    };
    let local_session_id = match frame_text(get("local_session_id"), 128) {
        Ok(local_session_id) => local_session_id,
        Err(response) => return response,
    };
    let agent_kind = match frame_text(get("agent_kind"), 24) {
        Ok(agent_kind) => agent_kind,
        Err(response) => return response,
    };
    let model = frame_model(get("model"));
    let mut metadata = serde_json::Map::with_capacity(3);
    metadata.insert(
        "local_session_id".to_owned(),
        Value::String(local_session_id),
    );
    metadata.insert("local_thread_id".to_owned(), Value::String(local_thread_id));
    metadata.insert("agent_kind".to_owned(), Value::String(agent_kind));
    let started_at = Utc::now();
    if model.is_empty() {
        if sqlx::query(
            r#"UPDATE "agent_run" SET "status" = $1, "thread_id" = $2,
                  "started_at" = $3, "agent_metadata" = $4
               WHERE "agent_run"."id" = $5"#,
        )
        .bind(AgentRunStatus::Running.value())
        .bind(&thread_id)
        .bind(started_at)
        .bind(Value::Object(metadata))
        .bind(locked.id)
        .execute(&mut *tx)
        .await
        .is_err()
        {
            return server_error();
        }
    } else {
        if sqlx::query(
            r#"UPDATE "agent_run" SET "status" = $1, "thread_id" = $2,
                  "started_at" = $3, "agent_metadata" = $4, "llm_model" = $5
               WHERE "agent_run"."id" = $6"#,
        )
        .bind(AgentRunStatus::Running.value())
        .bind(&thread_id)
        .bind(started_at)
        .bind(Value::Object(metadata))
        .bind(truncate_chars(&model, 128))
        .bind(locked.id)
        .execute(&mut *tx)
        .await
        .is_err()
        {
            return server_error();
        }
    }
    if commit_tx(tx).await.is_err() {
        return server_error();
    }
    json_response(StatusCode::OK, r#"{"ok":true}"#.to_owned())
}

/// `POST runs/<run_id>/events/` (`run_endpoints.py:194-235`): the
/// terminal check runs *before* the transaction (no dedupe recorded
/// for late batches); inside, the per-batch dedupe guards the
/// per-`seq` upserts.
pub async fn run_events(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    req: Request<axum::body::Body>,
) -> Response {
    let (pool, preamble) = match daemon_preamble(&state, &run_id, req).await {
        Ok(preamble) => preamble,
        Err(response) => return response,
    };
    let run = match resolve_run(&pool, preamble.run_id, preamble.runner.as_ref()).await {
        Ok(run) => run,
        Err(response) => return response,
    };
    if is_terminal_status(&run.status) {
        return json_response(
            StatusCode::OK,
            r#"{"ok":true,"terminal":true,"accepted":0}"#.to_owned(),
        );
    }
    let items = match event_batch_items(&preamble.data) {
        Ok(items) => items,
        Err(FrameError) => return server_error(),
    };
    let mut tx = match pool.begin().await {
        Ok(tx) => tx,
        Err(_) => return server_error(),
    };
    match record_dedupe(&mut tx, run.id, &preamble.key).await {
        Ok(true) => {}
        Ok(false) => {
            if commit_tx(tx).await.is_err() {
                return server_error();
            }
            return json_response(StatusCode::OK, r#"{"ok":true,"duplicate":true}"#.to_owned());
        }
        Err(response) => return response,
    }
    let mut accepted = 0;
    for ev in items {
        let Some(obj) = ev.as_object() else {
            return server_error();
        };
        // `int(ev.get("seq") or 0)`.
        let zero = Value::Number(0.into());
        let seq_base = obj
            .get("seq")
            .filter(|value| py_truthy(value))
            .unwrap_or(&zero);
        let seq = match py_int(seq_base) {
            Ok(seq) => seq,
            Err(FrameError) => return server_error(),
        };
        let kind = match frame_text(obj.get("kind").unwrap_or(&Value::Null), 64) {
            Ok(kind) => kind,
            Err(response) => return response,
        };
        if kind.is_empty() {
            continue;
        }
        let payload = match obj.get("payload") {
            Some(payload) if py_truthy(payload) => payload.clone(),
            _ => Value::Object(Default::default()),
        };
        let payload = truncate_event_payload(&payload);
        // `update_or_create(agent_run=run, seq=seq)`: the get, then
        // the update or the create.
        let existing: Option<i64> = match sqlx::query_scalar(
            r#"SELECT "id" FROM "agent_run_event"
               WHERE ("agent_run_id" = $1 AND "seq" = $2)"#,
        )
        .bind(run.id)
        .bind(seq)
        .fetch_optional(&mut *tx)
        .await
        {
            Ok(existing) => existing,
            Err(_) => return server_error(),
        };
        match existing {
            Some(id) => {
                if sqlx::query(
                    r#"UPDATE "agent_run_event" SET "kind" = $1, "payload" = $2
                       WHERE "agent_run_event"."id" = $3"#,
                )
                .bind(&kind)
                .bind(&payload)
                .bind(id)
                .execute(&mut *tx)
                .await
                .is_err()
                {
                    return server_error();
                }
            }
            None => {
                if sqlx::query(
                    r#"INSERT INTO "agent_run_event"
                       ("agent_run_id", "seq", "kind", "payload", "created_at")
                       VALUES ($1, $2, $3, $4, $5)"#,
                )
                .bind(run.id)
                .bind(seq)
                .bind(&kind)
                .bind(&payload)
                .bind(Utc::now())
                .execute(&mut *tx)
                .await
                .is_err()
                {
                    return server_error();
                }
            }
        }
        accepted += 1;
    }
    if commit_tx(tx).await.is_err() {
        return server_error();
    }
    json_response(
        StatusCode::OK,
        serde_json::json!({"ok": true, "accepted": accepted}).to_string(),
    )
}

/// `POST runs/<run_id>/approvals/` (`run_endpoints.py:238-270`):
/// dedupe, lock, cancel check, then the approval upsert (which
/// resets the row to `PENDING`) and the run to `AWAITING_APPROVAL`.
pub async fn run_approval(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    req: Request<axum::body::Body>,
) -> Response {
    let (pool, preamble) = match daemon_preamble(&state, &run_id, req).await {
        Ok(preamble) => preamble,
        Err(response) => return response,
    };
    let run = match resolve_run(&pool, preamble.run_id, preamble.runner.as_ref()).await {
        Ok(run) => run,
        Err(response) => return response,
    };
    let mut tx = match pool.begin().await {
        Ok(tx) => tx,
        Err(_) => return server_error(),
    };
    match record_dedupe(&mut tx, run.id, &preamble.key).await {
        Ok(true) => {}
        Ok(false) => {
            if commit_tx(tx).await.is_err() {
                return server_error();
            }
            return json_response(StatusCode::OK, r#"{"ok":true,"duplicate":true}"#.to_owned());
        }
        Err(response) => return response,
    }
    let locked = match lock_non_terminal(&mut tx, run.id).await {
        Ok(locked) => locked,
        Err(closed) => {
            if commit_tx(tx).await.is_err() {
                return server_error();
            }
            return closed;
        }
    };
    if let Some(pending) = cancellation_pending_response(&locked) {
        if commit_tx(tx).await.is_err() {
            return server_error();
        }
        return pending;
    }
    // Field reads land here, after the cancel check, exactly as in
    // the source — a cancel-pending run with a garbage body answers
    // `ignored`, not 500.
    let approval_uuid = match approval_id(&preamble.data) {
        Ok(approval_uuid) => approval_uuid,
        Err(FrameError) => return server_error(),
    };
    let kind = match run_approval_kind(&preamble.data) {
        Ok(kind) => kind,
        Err(FrameError) => return server_error(),
    };
    let payload = match preamble.data.get("payload") {
        Some(payload) if py_truthy(payload) => payload.clone(),
        _ => Value::Object(Default::default()),
    };
    let reason = match preamble.data.get("reason") {
        Some(reason) if py_truthy(reason) => py_str_value(reason),
        _ => String::new(),
    };
    let expires_at = match expires_at_text(&preamble.data) {
        Ok(expires_at) => expires_at,
        Err(FrameError) => return server_error(),
    };
    // `update_or_create(id=...)`: the get, then the update or the
    // create. The update rewrites the defaults wholesale — status
    // back to `pending` included.
    let existing: Option<Uuid> =
        match sqlx::query_scalar(r#"SELECT "id" FROM "agent_run_approval" WHERE "id" = $1"#)
            .bind(approval_uuid)
            .fetch_optional(&mut *tx)
            .await
        {
            Ok(existing) => existing,
            Err(_) => return server_error(),
        };
    match existing {
        Some(id) => {
            if sqlx::query(
                r#"UPDATE "agent_run_approval"
                   SET "agent_run_id" = $1, "kind" = $2, "payload" = $3,
                       "reason" = $4, "status" = $5, "expires_at" = $6::timestamptz
                   WHERE "agent_run_approval"."id" = $7"#,
            )
            .bind(locked.id)
            .bind(kind)
            .bind(&payload)
            .bind(&reason)
            .bind("pending")
            .bind(expires_at.as_deref())
            .bind(id)
            .execute(&mut *tx)
            .await
            .is_err()
            {
                return server_error();
            }
        }
        None => {
            if sqlx::query(
                r#"INSERT INTO "agent_run_approval"
                   ("id", "agent_run_id", "kind", "payload", "reason", "status",
                    "decision_source", "decided_by_id", "requested_at", "expires_at",
                    "decided_at")
                   VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10::timestamptz, $11)"#,
            )
            .bind(approval_uuid)
            .bind(locked.id)
            .bind(kind)
            .bind(&payload)
            .bind(&reason)
            .bind("pending")
            .bind("")
            .bind(None::<Uuid>)
            .bind(Utc::now())
            .bind(expires_at.as_deref())
            .bind(None::<DateTime<Utc>>)
            .execute(&mut *tx)
            .await
            .is_err()
            {
                return server_error();
            }
        }
    }
    if sqlx::query(r#"UPDATE "agent_run" SET "status" = $1 WHERE "agent_run"."id" = $2"#)
        .bind(AgentRunStatus::AwaitingApproval.value())
        .bind(locked.id)
        .execute(&mut *tx)
        .await
        .is_err()
    {
        return server_error();
    }
    if commit_tx(tx).await.is_err() {
        return server_error();
    }
    json_response(StatusCode::OK, r#"{"ok":true}"#.to_owned())
}

/// `POST runs/<run_id>/awaiting-reauth/` (`run_endpoints.py:273-287`):
/// dedupe, lock, cancel check, then `AWAITING_REAUTH`.
pub async fn run_awaiting_reauth(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let (pool, _runner, run_id, key) = match daemon_preamble_nobody(&state, &run_id, &headers).await
    {
        Ok(preamble) => preamble,
        Err(response) => return response,
    };
    let mut tx = match pool.begin().await {
        Ok(tx) => tx,
        Err(_) => return server_error(),
    };
    match record_dedupe(&mut tx, run_id, &key).await {
        Ok(true) => {}
        Ok(false) => {
            if commit_tx(tx).await.is_err() {
                return server_error();
            }
            return json_response(StatusCode::OK, r#"{"ok":true,"duplicate":true}"#.to_owned());
        }
        Err(response) => return response,
    }
    let locked = match lock_non_terminal(&mut tx, run_id).await {
        Ok(locked) => locked,
        Err(closed) => {
            if commit_tx(tx).await.is_err() {
                return server_error();
            }
            return closed;
        }
    };
    if let Some(pending) = cancellation_pending_response(&locked) {
        if commit_tx(tx).await.is_err() {
            return server_error();
        }
        return pending;
    }
    if sqlx::query(r#"UPDATE "agent_run" SET "status" = $1 WHERE "agent_run"."id" = $2"#)
        .bind(AgentRunStatus::AwaitingReauth.value())
        .bind(locked.id)
        .execute(&mut *tx)
        .await
        .is_err()
    {
        return server_error();
    }
    if commit_tx(tx).await.is_err() {
        return server_error();
    }
    json_response(StatusCode::OK, r#"{"ok":true}"#.to_owned())
}

/// `POST runs/<run_id>/complete/` (`run_endpoints.py:290-308`):
/// dedupe, then the terminal finalize (which carries its own
/// first-writer-wins lock — no `_lock_non_terminal` here).
pub async fn run_completed(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    req: Request<axum::body::Body>,
) -> Response {
    let (pool, preamble) = match daemon_preamble(&state, &run_id, req).await {
        Ok(preamble) => preamble,
        Err(response) => return response,
    };
    let run = match resolve_run(&pool, preamble.run_id, preamble.runner.as_ref()).await {
        Ok(run) => run,
        Err(response) => return response,
    };
    let mut tx = match pool.begin().await {
        Ok(tx) => tx,
        Err(_) => return server_error(),
    };
    match record_dedupe(&mut tx, run.id, &preamble.key).await {
        Ok(true) => {}
        Ok(false) => {
            if commit_tx(tx).await.is_err() {
                return server_error();
            }
            return json_response(StatusCode::OK, r#"{"ok":true,"duplicate":true}"#.to_owned());
        }
        Err(response) => return response,
    }
    let mut effects = Vec::new();
    if let Some(runner) = preamble.runner.as_ref() {
        // A non-dict body is the source's `AttributeError` on `.get`
        // (500) — but only *here*, after the dedupe and the runner
        // check (`run_endpoints.py:296-306`).
        if preamble.data.as_object().is_none() {
            return server_error();
        }
        let done_payload = preamble
            .data
            .get("done_payload")
            .cloned()
            .unwrap_or(Value::Null);
        let tokens = frame_tokens(&preamble.data);
        let model = preamble.data.get("model").cloned().unwrap_or(Value::Null);
        effects = match execute_finalize_terminal(
            &mut tx,
            runner,
            run.id,
            AgentRunStatus::Completed,
            &done_payload,
            &Value::Null,
            &Value::Null,
            &tokens,
            &model,
        )
        .await
        {
            Ok(effects) => effects,
            Err(response) => return response,
        };
    }
    if commit_tx(tx).await.is_err() {
        return server_error();
    }
    if !effects.is_empty() {
        let ports = LivePorts::new(pool.clone(), &state);
        if drain_lifecycle_effects(&pool, &ports, effects)
            .await
            .is_err()
        {
            return server_error();
        }
    }
    json_response(StatusCode::OK, r#"{"ok":true}"#.to_owned())
}

/// `POST runs/<run_id>/pause/` (`run_endpoints.py:311-338`): dedupe,
/// lock, cancel check, then the pause path (phase-1 update, phase-2
/// comment, post-commit drain).
pub async fn run_paused(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    req: Request<axum::body::Body>,
) -> Response {
    let (pool, preamble) = match daemon_preamble(&state, &run_id, req).await {
        Ok(preamble) => preamble,
        Err(response) => return response,
    };
    let run = match resolve_run(&pool, preamble.run_id, preamble.runner.as_ref()).await {
        Ok(run) => run,
        Err(response) => return response,
    };
    let mut tx = match pool.begin().await {
        Ok(tx) => tx,
        Err(_) => return server_error(),
    };
    match record_dedupe(&mut tx, run.id, &preamble.key).await {
        Ok(true) => {}
        Ok(false) => {
            if commit_tx(tx).await.is_err() {
                return server_error();
            }
            return json_response(StatusCode::OK, r#"{"ok":true,"duplicate":true}"#.to_owned());
        }
        Err(response) => return response,
    }
    let locked = match lock_non_terminal(&mut tx, run.id).await {
        Ok(locked) => locked,
        Err(closed) => {
            if commit_tx(tx).await.is_err() {
                return server_error();
            }
            return closed;
        }
    };
    if let Some(pending) = cancellation_pending_response(&locked) {
        if commit_tx(tx).await.is_err() {
            return server_error();
        }
        return pending;
    }
    let Some(runner) = preamble.runner.as_ref() else {
        // Unreachable past `_resolve`, kept for exactness.
        if commit_tx(tx).await.is_err() {
            return server_error();
        }
        return json_response(StatusCode::OK, r#"{"ok":true}"#.to_owned());
    };
    // `request.data.get("payload") or {}` — a non-dict body is the
    // source's `AttributeError` (500), after the cancel check.
    let payload = match preamble.data.as_object() {
        Some(obj) => obj
            .get("payload")
            .filter(|value| py_truthy(value))
            .cloned()
            .unwrap_or(Value::Object(Default::default())),
        None => return server_error(),
    };
    let tokens = frame_tokens(&preamble.data);
    let model = preamble.data.get("model").cloned().unwrap_or(Value::Null);
    let ports = LivePorts::new(pool.clone(), &state);
    let effects = match execute_apply_paused(
        &mut tx, &ports, runner, run.id, &payload, &tokens, &model,
    )
    .await
    {
        Ok(effects) => effects,
        Err(response) => return response,
    };
    if commit_tx(tx).await.is_err() {
        return server_error();
    }
    if !effects.is_empty()
        && drain_lifecycle_effects(&pool, &ports, effects)
            .await
            .is_err()
    {
        return server_error();
    }
    json_response(StatusCode::OK, r#"{"ok":true}"#.to_owned())
}

/// `POST runs/<run_id>/fail/` (`run_endpoints.py:341-422`): dedupe,
/// then the four reason branches — `resume_unavailable` and
/// `assign_rejected_busy` re-queue (or satisfy a pending cancel),
/// `refusal` finalizes `REFUSED`, anything else finalizes `FAILED`.
pub async fn run_failed(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    req: Request<axum::body::Body>,
) -> Response {
    let (pool, preamble) = match daemon_preamble(&state, &run_id, req).await {
        Ok(preamble) => preamble,
        Err(response) => return response,
    };
    let run = match resolve_run(&pool, preamble.run_id, preamble.runner.as_ref()).await {
        Ok(run) => run,
        Err(response) => return response,
    };
    let mut tx = match pool.begin().await {
        Ok(tx) => tx,
        Err(_) => return server_error(),
    };
    match record_dedupe(&mut tx, run.id, &preamble.key).await {
        Ok(true) => {}
        Ok(false) => {
            if commit_tx(tx).await.is_err() {
                return server_error();
            }
            return json_response(StatusCode::OK, r#"{"ok":true,"duplicate":true}"#.to_owned());
        }
        Err(response) => return response,
    }
    let Some(runner) = preamble.runner.as_ref() else {
        // Unreachable past `_resolve`, kept for exactness.
        if commit_tx(tx).await.is_err() {
            return server_error();
        }
        return json_response(StatusCode::OK, r#"{"ok":true}"#.to_owned());
    };
    // A non-dict body is the source's `AttributeError` on `.get`
    // (500) — but only *here*, after the dedupe and the runner check,
    // before the first `reason` read (`run_endpoints.py:352`).
    if preamble.data.as_object().is_none() {
        return server_error();
    }
    let ports = LivePorts::new(pool.clone(), &state);
    if reason_is(&preamble.data, "resume_unavailable") {
        return run_failed_requeue(&pool, &ports, tx, runner, run.id, false).await;
    }
    if reason_is(&preamble.data, "assign_rejected_busy") {
        return run_failed_requeue(&pool, &ports, tx, runner, run.id, true).await;
    }
    let detail = preamble.data.get("detail").cloned().unwrap_or(Value::Null);
    let tokens = frame_tokens(&preamble.data);
    let model = preamble.data.get("model").cloned().unwrap_or(Value::Null);
    if reason_is(&preamble.data, "refusal") {
        let category = preamble
            .data
            .get("category")
            .cloned()
            .unwrap_or(Value::Null);
        let effects = match execute_finalize_terminal(
            &mut tx,
            runner,
            run.id,
            AgentRunStatus::Refused,
            &Value::Null,
            &detail,
            &category,
            &tokens,
            &model,
        )
        .await
        {
            Ok(effects) => effects,
            Err(response) => return response,
        };
        if commit_tx(tx).await.is_err() {
            return server_error();
        }
        if !effects.is_empty()
            && drain_lifecycle_effects(&pool, &ports, effects)
                .await
                .is_err()
        {
            return server_error();
        }
        return json_response(StatusCode::OK, r#"{"ok":true,"refused":true}"#.to_owned());
    }
    let effects = match execute_finalize_terminal(
        &mut tx,
        runner,
        run.id,
        AgentRunStatus::Failed,
        &Value::Null,
        &detail,
        &Value::Null,
        &tokens,
        &model,
    )
    .await
    {
        Ok(effects) => effects,
        Err(response) => return response,
    };
    if commit_tx(tx).await.is_err() {
        return server_error();
    }
    if !effects.is_empty()
        && drain_lifecycle_effects(&pool, &ports, effects)
            .await
            .is_err()
    {
        return server_error();
    }
    json_response(StatusCode::OK, r#"{"ok":true}"#.to_owned())
}

/// The shared `resume_unavailable` / `assign_rejected_busy` branch
/// (`run_endpoints.py:356-398`): lock (terminal answers commit with
/// the answer), a pending cancel finalizes `CANCELLED`, otherwise
/// the requeue (with the `BUSY` flip for the NACK branch).
async fn run_failed_requeue(
    pool: &PgPool,
    ports: &LivePorts,
    mut tx: sqlx::Transaction<'_, sqlx::Postgres>,
    runner: &DaemonRunner,
    run_id: Uuid,
    busy_flip: bool,
) -> Response {
    let locked = match lock_non_terminal(&mut tx, run_id).await {
        Ok(locked) => locked,
        Err(closed) => {
            if commit_tx(tx).await.is_err() {
                return server_error();
            }
            return closed;
        }
    };
    if locked.status == AgentRunStatus::CancelRequested.value() {
        // The daemon proved it has no resumable process (or never
        // started one) for this run: the cancellation barrier is
        // satisfied.
        let effects = match execute_finalize_terminal(
            &mut tx,
            runner,
            run_id,
            AgentRunStatus::Cancelled,
            &Value::Null,
            &Value::Null,
            &Value::Null,
            &Value::Null,
            &Value::Null,
        )
        .await
        {
            Ok(effects) => effects,
            Err(response) => return response,
        };
        if commit_tx(tx).await.is_err() {
            return server_error();
        }
        if !effects.is_empty() && drain_lifecycle_effects(pool, ports, effects).await.is_err() {
            return server_error();
        }
        return json_response(StatusCode::OK, r#"{"ok":true,"cancelled":true}"#.to_owned());
    }
    let effects = if busy_flip {
        match execute_assign_rejected_busy(&mut tx, runner.id, &locked).await {
            Ok(effects) => effects,
            Err(response) => return response,
        }
    } else {
        match execute_requeue(&mut tx, &locked).await {
            Ok(effects) => effects,
            Err(response) => return response,
        }
    };
    if commit_tx(tx).await.is_err() {
        return server_error();
    }
    if !effects.is_empty() && drain_lifecycle_effects(pool, ports, effects).await.is_err() {
        return server_error();
    }
    json_response(
        StatusCode::OK,
        r#"{"ok":true,"rescheduled":true}"#.to_owned(),
    )
}

/// `POST runs/<run_id>/cancelled/` (`run_endpoints.py:425-442`):
/// dedupe, then the terminal finalize (own lock, no cancel check).
pub async fn run_cancelled(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    req: Request<axum::body::Body>,
) -> Response {
    let (pool, preamble) = match daemon_preamble(&state, &run_id, req).await {
        Ok(preamble) => preamble,
        Err(response) => return response,
    };
    let run = match resolve_run(&pool, preamble.run_id, preamble.runner.as_ref()).await {
        Ok(run) => run,
        Err(response) => return response,
    };
    let mut tx = match pool.begin().await {
        Ok(tx) => tx,
        Err(_) => return server_error(),
    };
    match record_dedupe(&mut tx, run.id, &preamble.key).await {
        Ok(true) => {}
        Ok(false) => {
            if commit_tx(tx).await.is_err() {
                return server_error();
            }
            return json_response(StatusCode::OK, r#"{"ok":true,"duplicate":true}"#.to_owned());
        }
        Err(response) => return response,
    }
    let mut effects = Vec::new();
    if let Some(runner) = preamble.runner.as_ref() {
        // A non-dict body is the source's `AttributeError` on `.get`
        // (500) — but only *here*, after the dedupe and the runner
        // check (`run_endpoints.py:431-441`).
        if preamble.data.as_object().is_none() {
            return server_error();
        }
        let tokens = frame_tokens(&preamble.data);
        let model = preamble.data.get("model").cloned().unwrap_or(Value::Null);
        effects = match execute_finalize_terminal(
            &mut tx,
            runner,
            run.id,
            AgentRunStatus::Cancelled,
            &Value::Null,
            &Value::Null,
            &Value::Null,
            &tokens,
            &model,
        )
        .await
        {
            Ok(effects) => effects,
            Err(response) => return response,
        };
    }
    if commit_tx(tx).await.is_err() {
        return server_error();
    }
    if !effects.is_empty() {
        let ports = LivePorts::new(pool.clone(), &state);
        if drain_lifecycle_effects(&pool, &ports, effects)
            .await
            .is_err()
        {
            return server_error();
        }
    }
    json_response(StatusCode::OK, r#"{"ok":true}"#.to_owned())
}

/// `POST runs/<run_id>/resumed/` (`run_endpoints.py:445-459`):
/// dedupe, lock, cancel check, then `RUNNING`.
pub async fn run_resumed(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let (pool, _runner, run_id, key) = match daemon_preamble_nobody(&state, &run_id, &headers).await
    {
        Ok(preamble) => preamble,
        Err(response) => return response,
    };
    let mut tx = match pool.begin().await {
        Ok(tx) => tx,
        Err(_) => return server_error(),
    };
    match record_dedupe(&mut tx, run_id, &key).await {
        Ok(true) => {}
        Ok(false) => {
            if commit_tx(tx).await.is_err() {
                return server_error();
            }
            return json_response(StatusCode::OK, r#"{"ok":true,"duplicate":true}"#.to_owned());
        }
        Err(response) => return response,
    }
    let locked = match lock_non_terminal(&mut tx, run_id).await {
        Ok(locked) => locked,
        Err(closed) => {
            if commit_tx(tx).await.is_err() {
                return server_error();
            }
            return closed;
        }
    };
    if let Some(pending) = cancellation_pending_response(&locked) {
        if commit_tx(tx).await.is_err() {
            return server_error();
        }
        return pending;
    }
    if sqlx::query(r#"UPDATE "agent_run" SET "status" = $1 WHERE "agent_run"."id" = $2"#)
        .bind(AgentRunStatus::Running.value())
        .bind(locked.id)
        .execute(&mut *tx)
        .await
        .is_err()
    {
        return server_error();
    }
    if commit_tx(tx).await.is_err() {
        return server_error();
    }
    json_response(StatusCode::OK, r#"{"ok":true}"#.to_owned())
}

/// `POST runs/<run_id>/stream/upgrade/` (`run_endpoints.py:462-510`):
/// resolve, validate the stream name, then mint a 60s ticket into
/// Redis. No transaction (nothing writes to Postgres); any Redis
/// failure — missing client included — is 503 `redis_unavailable`.
pub async fn run_stream_upgrade(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    req: Request<axum::body::Body>,
) -> Response {
    let (pool, preamble) = match daemon_preamble(&state, &run_id, req).await {
        Ok(preamble) => preamble,
        Err(response) => return response,
    };
    let run = match resolve_run(&pool, preamble.run_id, preamble.runner.as_ref()).await {
        Ok(run) => run,
        Err(response) => return response,
    };
    let stream = match stream_param(&preamble.data) {
        Some(Ok(stream)) => stream,
        Some(Err(FrameError)) => {
            return json_response(
                StatusCode::BAD_REQUEST,
                r#"{"error":"invalid_stream"}"#.to_owned(),
            );
        }
        None => return server_error(),
    };
    let ports = LivePorts::new(pool.clone(), &state);
    let Some(client) = ports.redis_client() else {
        return json_response(
            StatusCode::SERVICE_UNAVAILABLE,
            r#"{"error":"redis_unavailable"}"#.to_owned(),
        );
    };
    let ticket = Uuid::new_v4().simple().to_string();
    let payload = ticket_payload(
        &run.id,
        &stream,
        preamble.runner.as_ref().map(|runner| &runner.id),
        &Utc::now(),
    );
    let stored: Result<(), redis::RedisError> = async {
        use redis::AsyncCommands;
        let mut connection = client.get_multiplexed_async_connection().await?;
        connection
            .set_ex::<_, _, ()>(&ticket_key(&ticket), &payload, 60)
            .await
    }
    .await;
    if let Err(error) = stored {
        tracing::error!(%error, run_id = %run.id, "failed to mint ws upgrade ticket");
        return json_response(
            StatusCode::SERVICE_UNAVAILABLE,
            r#"{"error":"redis_unavailable"}"#.to_owned(),
        );
    }
    json_response(
        StatusCode::OK,
        serde_json::json!({"ticket": ticket, "expires_in_secs": 60}).to_string(),
    )
}

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------

/// An owned path: POST serves from Rust, every other method falls
/// through to Django (the `app_scheduler` precedent).
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
            "OPTIONS" => router.options(crate::edge::proxy),
            _ => router.get(crate::edge::proxy),
        };
    }
    router
}

/// Register the 12 run daemon routes (`runner/urls.py:142-201`).
/// Sibling handler issues merge their routers at the F-10 seam;
/// merges keep both sides.
pub fn routes() -> Router<AppState> {
    use axum::routing::post;
    const UNOWNED: &[&str] = &["GET", "PUT", "PATCH", "DELETE", "OPTIONS"];
    Router::new()
        .route(
            "/api/v1/runner/runs/{run_id}/accept/",
            owned(post(run_accept), UNOWNED),
        )
        .route(
            "/api/v1/runner/runs/{run_id}/queued/",
            owned(post(run_queued), UNOWNED),
        )
        .route(
            "/api/v1/runner/runs/{run_id}/started/",
            owned(post(run_started), UNOWNED),
        )
        .route(
            "/api/v1/runner/runs/{run_id}/events/",
            owned(post(run_events), UNOWNED),
        )
        .route(
            "/api/v1/runner/runs/{run_id}/approvals/",
            owned(post(run_approval), UNOWNED),
        )
        .route(
            "/api/v1/runner/runs/{run_id}/awaiting-reauth/",
            owned(post(run_awaiting_reauth), UNOWNED),
        )
        .route(
            "/api/v1/runner/runs/{run_id}/complete/",
            owned(post(run_completed), UNOWNED),
        )
        .route(
            "/api/v1/runner/runs/{run_id}/pause/",
            owned(post(run_paused), UNOWNED),
        )
        .route(
            "/api/v1/runner/runs/{run_id}/fail/",
            owned(post(run_failed), UNOWNED),
        )
        .route(
            "/api/v1/runner/runs/{run_id}/cancelled/",
            owned(post(run_cancelled), UNOWNED),
        )
        .route(
            "/api/v1/runner/runs/{run_id}/resumed/",
            owned(post(run_resumed), UNOWNED),
        )
        .route(
            "/api/v1/runner/runs/{run_id}/stream/upgrade/",
            owned(post(run_stream_upgrade), UNOWNED),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    static FIXTURE: &str =
        include_str!("../../../../fixtures/runner_runs/fx-run-09-handlers-daemon.golden.json");

    fn daemon_runs() -> Value {
        let fx: Value = serde_json::from_str(FIXTURE).expect("fixture parses");
        fx.get("daemon_runs").expect("daemon_runs").clone()
    }

    fn fx_body(section: &Value, case: &str) -> Value {
        section
            .get(case)
            .unwrap_or_else(|| panic!("{case}"))
            .get("body")
            .unwrap_or_else(|| panic!("{case}.body"))
            .clone()
    }

    #[test]
    fn fx_shared_answer_bodies_match() {
        let runs = daemon_runs();
        // `_resolve` denials (404/403 × mismatch/runnerless/no-auth).
        assert_eq!(
            fx_body(&runs, "resolve_404"),
            json!({"error": "run_not_found"})
        );
        for case in [
            "resolve_403_mismatch",
            "resolve_403_runnerless",
            "resolve_403_noauth",
        ] {
            assert_eq!(
                fx_body(&runs, case),
                json!({"error": "run_not_owned_by_runner"}),
                "{case}"
            );
        }
        // Dedupe / terminal / cancel-pending / retired-queue answers.
        assert_eq!(
            fx_body(&runs, "accept_dup"),
            json!({"ok": true, "duplicate": true})
        );
        assert_eq!(
            fx_body(&runs, "events_dup_batch"),
            json!({"ok": true, "duplicate": true})
        );
        assert_eq!(
            fx_body(&runs, "approval_dup"),
            json!({"ok": true, "duplicate": true})
        );
        assert_eq!(
            fx_body(&runs, "accept_terminal"),
            json!({"ok": true, "terminal": true})
        );
        assert_eq!(
            fx_body(&runs, "resumed_terminal"),
            json!({"ok": true, "terminal": true})
        );
        assert_eq!(
            fx_body(&runs, "accept_cancel_pending"),
            json!({"ok": true, "cancel_requested": true, "ignored": true})
        );
        assert_eq!(
            fx_body(&runs, "queued"),
            json!({"ok": true, "ignored": true})
        );
        // The terminal events batch acknowledges with `accepted: 0`.
        assert_eq!(
            fx_body(&runs, "events_terminal"),
            json!({"ok": true, "terminal": true, "accepted": 0})
        );
        // Failed-branch answers.
        assert_eq!(
            fx_body(&runs, "failed_resume"),
            json!({"ok": true, "rescheduled": true})
        );
        assert_eq!(
            fx_body(&runs, "failed_busy"),
            json!({"ok": true, "rescheduled": true})
        );
        assert_eq!(
            fx_body(&runs, "failed_resume_cancel"),
            json!({"ok": true, "cancelled": true})
        );
        assert_eq!(
            fx_body(&runs, "failed_busy_cancel"),
            json!({"ok": true, "cancelled": true})
        );
        assert_eq!(
            fx_body(&runs, "failed_refusal"),
            json!({"ok": true, "refused": true})
        );
        // Stream-upgrade denials.
        assert_eq!(
            fx_body(&runs, "stream_bad"),
            json!({"error": "invalid_stream"})
        );
        assert_eq!(
            fx_body(&runs, "stream_no_redis"),
            json!({"error": "redis_unavailable"})
        );
        assert_eq!(
            fx_body(&runs, "stream_blowup"),
            json!({"error": "redis_unavailable"})
        );
        // The literals above are the exact strings the handlers
        // render (serde_json compact == DRF compact); a drift here
        // fails loudly against the recorded goldens.
        assert_eq!(runs["stream_bad"]["status"], 400);
        assert_eq!(runs["stream_no_redis"]["status"], 503);
        assert_eq!(runs["resolve_404"]["status"], 404);
        assert_eq!(runs["resolve_403_mismatch"]["status"], 403);
    }

    #[test]
    fn py_int_matches_cpython() {
        assert_eq!(py_int(&json!(0)).expect("zero"), 0);
        assert_eq!(py_int(&json!(7)).expect("int"), 7);
        assert_eq!(py_int(&json!(-7)).expect("neg"), -7);
        assert_eq!(py_int(&json!(true)).expect("true"), 1);
        assert_eq!(py_int(&json!(false)).expect("false"), 0);
        assert_eq!(py_int(&json!(3.99)).expect("trunc"), 3);
        assert_eq!(py_int(&json!(-3.99)).expect("neg trunc"), -3);
        assert_eq!(py_int(&json!(" 42 ")).expect("padded"), 42);
        assert_eq!(py_int(&json!("+5")).expect("plus"), 5);
        assert!(py_int(&json!("3.5")).is_err());
        assert!(py_int(&json!("abc")).is_err());
        assert!(py_int(&json!("")).is_err());
        assert!(py_int(&json!(null)).is_err());
        assert!(py_int(&json!([1])).is_err());
        assert!(py_int(&json!({"n": 1})).is_err());
    }

    #[test]
    fn event_batch_shapes_match_python() {
        // An `events` batch passes through element-wise.
        let batch = json!({"events": [{"seq": 0}, {"seq": 1}]});
        let items = event_batch_items(&batch).expect("batch");
        assert_eq!(items.len(), 2);
        // A missing or falsy `events` reads the whole body as one event.
        let solo = json!({"seq": 9, "kind": "solo"});
        let items = event_batch_items(&solo).expect("solo");
        assert_eq!(items, vec![&solo]);
        let empty_batch = json!({"events": []});
        assert_eq!(event_batch_items(&empty_batch).expect("empty").len(), 1);
        // Non-dict bodies and non-list batches are the source's errors.
        assert!(event_batch_items(&json!([1])).is_err());
        assert!(event_batch_items(&json!("x")).is_err());
        assert!(event_batch_items(&json!({"events": "ab"})).is_err());
        assert!(event_batch_items(&json!({"events": 5})).is_err());
    }

    #[test]
    fn truncation_marker_matches_fixture_rule() {
        // Small payloads pass through untouched.
        let small = json!({"m": 1});
        assert_eq!(truncate_event_payload(&small), small);
        // The fixture pins `original_size_bytes: 65558` for its big
        // row; the marker shape (keys + order) is what replays here.
        let big = json!({"pad": "x".repeat(70_000)});
        let truncated = truncate_event_payload(&big);
        let marker = truncated.as_object().expect("marker");
        let keys: Vec<&str> = marker.keys().map(String::as_str).collect();
        assert_eq!(keys, vec!["_truncated", "original_size_bytes"]);
        assert_eq!(marker["_truncated"], json!(true));
        assert!(marker["original_size_bytes"].as_u64().expect("size") > 65_536);
        // The boundary is strict-greater-than.
        let boundary = format!("\"{}\"", "y".repeat(65_536 - 2));
        let at_limit: Value = serde_json::from_str(&boundary).expect("json");
        assert_eq!(truncate_event_payload(&at_limit), at_limit);
    }

    #[test]
    fn approval_kind_map_is_case_insensitive_with_other_default() {
        for (raw, expected) in [
            ("command_execution", "command_execution"),
            ("COMMAND_EXECUTION", "command_execution"),
            ("File_Change", "file_change"),
            ("network_access", "network_access"),
            ("shell", "other"),
            ("", "other"),
        ] {
            assert_eq!(
                run_approval_kind(&json!({"kind": raw})).expect("kind"),
                expected,
                "{raw}"
            );
        }
        assert_eq!(run_approval_kind(&json!({})).expect("missing"), "other");
        assert!(run_approval_kind(&json!({"kind": 5})).is_err());
        assert!(run_approval_kind(&json!([1])).is_err());
    }

    #[test]
    fn reason_matching_ignores_falsy_and_non_strings() {
        assert!(reason_is(
            &json!({"reason": "resume_unavailable"}),
            "resume_unavailable"
        ));
        assert!(!reason_is(
            &json!({"reason": "resume_unavailable"}),
            "refusal"
        ));
        assert!(!reason_is(&json!({}), "refusal"));
        assert!(!reason_is(&json!({"reason": null}), "refusal"));
        assert!(!reason_is(&json!({"reason": true}), "refusal"));
        assert!(!reason_is(&json!({"reason": 5}), "refusal"));
        assert!(!reason_is(&json!(["refusal"]), "refusal"));
    }

    #[test]
    fn frame_tokens_prefers_tokens_then_usage() {
        assert_eq!(
            frame_tokens(&json!({"tokens": {"a": 1}, "usage": {"b": 2}})),
            json!({"a": 1})
        );
        assert_eq!(
            frame_tokens(&json!({"tokens": null, "usage": {"b": 2}})),
            json!({"b": 2})
        );
        assert_eq!(frame_tokens(&json!({})), Value::Null);
        assert_eq!(frame_tokens(&json!([1])), Value::Null);
    }

    #[test]
    fn stream_param_defaults_validates_and_500s() {
        assert_eq!(
            stream_param(&json!({})).expect("dict"),
            Ok("events".to_owned())
        );
        assert_eq!(
            stream_param(&json!({"stream": "LOG"})).expect("dict"),
            Ok("log".to_owned())
        );
        assert_eq!(
            stream_param(&json!({"stream": "video"})).expect("dict"),
            Err(FrameError)
        );
        assert!(stream_param(&json!({"stream": 5})).is_none());
        assert!(stream_param(&json!([1])).is_none());
    }

    #[test]
    fn ticket_key_and_payload_match_source_shape() {
        assert_eq!(ticket_key("abc"), "ws_upgrade_ticket:abc");
        let run_id: Uuid = "0192d3b4-8c1c-7a2e-9f4b-6d5c8b7a6e5d"
            .parse()
            .expect("uuid");
        let runner_id: Uuid = "a11f797c-8430-44f9-8ae2-d3aef1c2cffc"
            .parse()
            .expect("uuid");
        let now: DateTime<Utc> = "2026-10-03T12:00:00Z".parse().expect("dt");
        let raw = ticket_payload(&run_id, "log", Some(&runner_id), &now);
        // Byte-exact `json.dumps`: spaced separators, source key order.
        assert_eq!(
            raw,
            r#"{"run_id": "0192d3b4-8c1c-7a2e-9f4b-6d5c8b7a6e5d", "stream": "log", "runner_id": "a11f797c-8430-44f9-8ae2-d3aef1c2cffc", "expires_at": "2026-10-03T12:01:00.000000+00:00"}"#
        );
        let payload: Value = serde_json::from_str(&raw).expect("json");
        // Python dict order: run_id, stream, runner_id, expires_at.
        let keys: Vec<&str> = payload
            .as_object()
            .expect("obj")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(keys, vec!["run_id", "stream", "runner_id", "expires_at"]);
        assert_eq!(
            payload["run_id"],
            json!("0192d3b4-8c1c-7a2e-9f4b-6d5c8b7a6e5d")
        );
        assert_eq!(payload["stream"], json!("log"));
        assert_eq!(
            payload["runner_id"],
            json!("a11f797c-8430-44f9-8ae2-d3aef1c2cffc")
        );
        assert_eq!(
            payload["expires_at"],
            json!("2026-10-03T12:01:00.000000+00:00")
        );
        // Anonymous minting renders `runner_id` empty (unreachable
        // past `_resolve`, kept for exactness).
        let anon: Value =
            serde_json::from_str(&ticket_payload(&run_id, "events", None, &now)).expect("json");
        assert_eq!(anon["runner_id"], json!(""));
        // Tickets are hex32 (`uuid4().hex`).
        let ticket = Uuid::new_v4().simple().to_string();
        assert_eq!(ticket.len(), 32);
        assert!(ticket.bytes().all(|byte| byte.is_ascii_hexdigit()));
    }

    #[test]
    fn approval_id_mints_parses_and_rejects() {
        let minted = approval_id(&json!({})).expect("minted");
        assert_eq!(minted.get_version(), Some(uuid::Version::Random));
        let minted_null = approval_id(&json!({"approval_id": null})).expect("null mints");
        assert_eq!(minted_null.get_version(), Some(uuid::Version::Random));
        let parsed = approval_id(&json!({"approval_id": "0192d3b4-8c1c-7a2e-9f4b-6d5c8b7a6e5d"}))
            .expect("parsed");
        assert_eq!(parsed.to_string(), "0192d3b4-8c1c-7a2e-9f4b-6d5c8b7a6e5d");
        assert!(approval_id(&json!({"approval_id": "nope"})).is_err());
        assert!(approval_id(&json!({"approval_id": 5})).is_err());
        assert!(approval_id(&json!(5)).is_err());
    }

    #[test]
    fn expires_at_text_nulls_and_rejects() {
        assert_eq!(expires_at_text(&json!({})).expect("missing"), None);
        assert_eq!(
            expires_at_text(&json!({"expires_at": null})).expect("null"),
            None
        );
        assert_eq!(
            expires_at_text(&json!({"expires_at": "2026-01-01T00:00:00Z"})).expect("iso"),
            Some("2026-01-01T00:00:00Z".to_owned())
        );
        // `DateTimeField.to_python` on the raw value: `""` is a
        // `ValidationError`, `[]`/`{}` a `TypeError` — all 500.
        assert!(expires_at_text(&json!({"expires_at": ""})).is_err());
        assert!(expires_at_text(&json!({"expires_at": []})).is_err());
        assert!(expires_at_text(&json!({"expires_at": {}})).is_err());
        assert!(expires_at_text(&json!({"expires_at": 5})).is_err());
        assert!(expires_at_text(&json!({"expires_at": [1]})).is_err());
        assert!(expires_at_text(&json!({"expires_at": {"a": 1}})).is_err());
    }

    #[test]
    fn py_str_value_matches_python_str() {
        assert_eq!(py_str_value(&json!("need it")), "need it");
        assert_eq!(py_str_value(&json!(5)), "5");
        assert_eq!(py_str_value(&json!(-5)), "-5");
        assert_eq!(py_str_value(&json!(true)), "True");
        assert_eq!(py_str_value(&json!(1.5)), "1.5");
        assert_eq!(py_str_value(&json!([1, "a"])), "[1, 'a']");
        assert_eq!(py_str_value(&json!({"a": 1})), "{'a': 1}");
    }

    #[test]
    fn started_truncation_lengths_match_fixture() {
        let runs = daemon_runs();
        let trunc = &runs["started_trunc"];
        assert_eq!(trunc["thread_len"], 128);
        assert_eq!(trunc["agent_kind_len"], 24);
        assert_eq!(trunc["model_len"], 128);
        // The helpers the `started` handler truncates through.
        let long = "é".repeat(200);
        assert_eq!(
            frame_text(&json!(long.clone()), 128)
                .expect("thread")
                .chars()
                .count(),
            128
        );
        assert_eq!(
            frame_text(&json!(long.clone()), 24)
                .expect("kind")
                .chars()
                .count(),
            24
        );
        assert_eq!(truncate_chars(&long, 128).chars().count(), 128);
    }

    #[test]
    fn terminal_matrix_matches_python_membership() {
        for status in ["completed", "failed", "cancelled", "blocked", "refused"] {
            assert!(is_terminal_status(status), "{status}");
        }
        for status in [
            "queued",
            "assigned",
            "waiting_for_worktree",
            "running",
            "cancel_requested",
            "awaiting_approval",
            "awaiting_reauth",
            "paused_awaiting_input",
            "",
            "bogus",
        ] {
            assert!(!is_terminal_status(status), "{status}");
        }
    }

    #[test]
    fn idempotency_key_strips_and_defaults_empty() {
        use axum::http::HeaderMap;
        let headers = HeaderMap::new();
        assert_eq!(idempotency_key(&headers), "");
        let mut headers = HeaderMap::new();
        headers.insert("idempotency-key", "  abc  ".parse().expect("header"));
        assert_eq!(idempotency_key(&headers), "abc");
    }
}
