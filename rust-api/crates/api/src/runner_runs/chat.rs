//! Web + daemon chat endpoints (D-15 L8, stage 5, PIDASHCONV-543).
//!
//! Ports `apps/api/pi_dash/runner/views/chat.py:1-780` (the SSE stream
//! lives in [`super::sse`]): 8 web endpoints under `/api/runners/chat/`
//! (session list/create/detail, warm, messages GET+POST, cancel, close,
//! approvals list/decide) and 7 daemon endpoints under
//! `/api/v1/runner/chat/` (started, message-started, event, approval,
//! message-complete, failed, closed).
//!
//! # Execution model
//!
//! * Web endpoints authenticate via [`crate::license::resolve_actor`]
//!   (401 `{"Detail": ...}` when anonymous); daemon endpoints via the
//!   shared [`super::authenticate_daemon`].
//! * Every `POST` runs one sqlx transaction, collects [`ChatEffect`]s
//!   (plus the endpoint-local [`DirectSend`]s), commits, then drains
//!   them before responding — Python's synchronous `on_commit` order.
//! * L5 plans execute here: [`pidash_services::runner_runs::chat`]
//!   planners decide, this module binds and runs. The L5 SQL builders
//!   interpolate literals (Django-compiler form, pinned by L5's own
//!   tests), so production statements are parameterized equivalents —
//!   same predicates, same locks, same order — never the literal text.
//! * Early-success answers (duplicates, noops, skip rules) commit
//!   before responding, exactly as the source's normal block exit.
//!
//! # Ported bugs (also listed in the PR)
//!
//! * BUG-chat-noauth-500 (`chat.py:522`): the daemon `_resolve`
//!   dereferences `auth_runner.id` with no `None` guard —
//!   unauthenticated daemon chat POSTs answer 500, not 401/403.
//! * BUG-offline-fanout-500 (`chat.py:380-425,457-510`): close/decide
//!   send inside `on_commit`; for an offline runner the 500 carries a
//!   persisted close/decision.
//!
//! # Documented gaps and approximations
//!
//! * `record_dedupe` has no savepoint in the source (`chat.py:180-190`,
//!   unlike `_record_dedupe`): a duplicate aborts the Postgres
//!   transaction, and the endpoint still answers 200 `duplicate` with
//!   nothing persisted. This port rolls back explicitly and answers
//!   the same bytes.
//! * No CSRF enforcement on the web POSTs (the `assistant`
//!   `post_message` precedent: session-auth JSON POSTs without a CSRF
//!   gate; `prompting` is the only module that enforces). Contract
//!   traffic always carries valid CSRF, so the gate cannot tell.
//! * Throttle histories live under the bare `throttle_<scope>_<pk>`
//!   key (the `assistant::redis` precedent), separate from Django's
//!   version-prefixed cache keys during cutover.
//! * Unknown enum values read from the database answer 500: the L2
//!   rows cannot hold them and no contract input covers them.
//! * Soft-deleted workspace memberships do not count (the
//!   `workspace_member_exists` precedent plus the approvals-list Q's
//!   explicit `deleted_at` clause).

// Every handler returns a fully-rendered `Response` by design (the
// intake `parse_body` precedent, which carries the same allow).
#![allow(clippy::result_large_err)]

use std::time::{SystemTime, UNIX_EPOCH};

use axum::extract::{Extension, Path, Query, State};
use axum::http::{HeaderMap, Request, StatusCode};
use axum::response::Response;
use axum::Router;
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

use pidash_db::runner_runs::{
    AgentChatApprovalRequest, AgentChatEvent, AgentChatMessage, AgentChatSession,
};
use pidash_services::runner_runs::chat as chat_kernel;
use pidash_services::runner_runs::chat::{ChatEffect, NewEventInputs};
use pidash_types::runner_runs::{
    AgentChatMessageRole, AgentChatMessageStatus, AgentChatSessionStatus, ApprovalStatus,
};

use super::run_endpoints::{idempotency_key as daemon_idempotency_key, FrameError};
use super::{
    authenticate_daemon, frame_text, json_response, pool_of, publish_chat_event, py_truthy,
    read_request_data, server_error, truncate_chars, DaemonRunner, LivePorts, RunnerPorts,
};
use crate::middleware::SessionHandle;
use crate::state::AppState;

// ---------------------------------------------------------------------------
// Pure frame helpers (unit-tested against FX-RUN-09)
// ---------------------------------------------------------------------------

/// `CHAT_EVENT_PAYLOAD_MAX_BYTES` (`chat.py:64`).
pub const CHAT_EVENT_PAYLOAD_MAX_BYTES: usize = 256 * 1024;

/// The message-POST throttle scope (`chat.py:60-61`): 60/minute
/// (`settings/common.py:97`).
pub const CHAT_SEND_SCOPE: &str = "runner_chat_send";
pub const CHAT_SEND_REQUESTS: u32 = 60;
pub const CHAT_SEND_WINDOW_SECS: u64 = 60;

/// 404 body for missing/forbidden chat objects (`chat.py` web views).
pub const CHAT_NOT_FOUND_BODY: &str = r#"{"error":"not found"}"#;

/// `_idempotency_key` (`chat.py:69-70`): the stripped header.
pub fn chat_idempotency_key(headers: &HeaderMap) -> String {
    daemon_idempotency_key(headers)
}

/// `_missing_idempotency_key_response` (`chat.py:73-74`).
pub fn missing_idempotency_key() -> Response {
    json_response(
        StatusCode::BAD_REQUEST,
        r#"{"error":"idempotency_key_required"}"#.to_owned(),
    )
}

/// `_payload_too_large` (`chat.py:77-81`): the spaced-`dumps` rendering
/// over 256KiB. The source's `except → True` arm is unreachable for
/// parsed-JSON values (dumps never fails on them).
pub fn payload_too_large(value: &Value) -> bool {
    crate::assistant::events::py_dumps(value).len() > CHAT_EVENT_PAYLOAD_MAX_BYTES
}

/// `_assistant_delta_text` (`chat.py:84-98`): the delta string carried
/// by an `assistant_delta` payload (`params.delta` as a string, or
/// `params.delta.text`, else `params.text`).
pub fn assistant_delta_text(payload: &Value) -> String {
    let Value::Object(top) = payload else {
        return String::new();
    };
    let Some(Value::Object(params)) = top.get("params") else {
        return String::new();
    };
    match params.get("delta") {
        Some(Value::String(delta)) => return delta.clone(),
        Some(Value::Object(delta)) => {
            if let Some(Value::String(text)) = delta.get("text") {
                return text.clone();
            }
        }
        _ => {}
    }
    match params.get("text") {
        Some(Value::String(text)) => text.clone(),
        _ => String::new(),
    }
}

/// `_approval_kind` (`chat.py:123-128`): the same 3-kind map as the
/// run approvals, defaulting to `other`. `Err` is the source's
/// `AttributeError` on a truthy non-string (500).
pub fn chat_approval_kind(raw: &Value) -> Result<&'static str, FrameError> {
    if !py_truthy(raw) {
        return Ok("other");
    }
    let Value::String(text) = raw else {
        return Err(FrameError);
    };
    Ok(match text.to_lowercase().as_str() {
        "command_execution" => "command_execution",
        "file_change" => "file_change",
        "network_access" => "network_access",
        _ => "other",
    })
}

/// `_runner_unavailable` (`chat.py:119-120`): `OFFLINE` or `REVOKED`.
pub fn runner_unavailable(status: &str) -> bool {
    status == "offline" || status == "revoked"
}

/// `(request.data.get("kind") or "raw")[:64]` for the daemon event
/// endpoint (`chat.py:599`): falsy reads `"raw"`, strings truncate,
/// truthy non-strings are the source's `TypeError` (500).
pub fn daemon_event_kind(raw: &Value) -> Result<String, FrameError> {
    if !py_truthy(raw) {
        return Ok("raw".to_owned());
    }
    match raw.as_str() {
        Some(text) => Ok(truncate_chars(text, 64)),
        None => Err(FrameError),
    }
}

/// The `bridge_seq` injection (`chat.py:601-604`): an explicit
/// `is not None` check (falsy-but-present values still inject),
/// wrapping non-dict payloads in `{"value": ...}` first.
pub fn inject_bridge_seq(payload: &Value, bridge_seq: &Value) -> Value {
    if bridge_seq.is_null() {
        return payload.clone();
    }
    let mut injected = match payload.as_object() {
        Some(map) => map.clone(),
        None => {
            let mut map = serde_json::Map::with_capacity(2);
            map.insert("value".to_owned(), payload.clone());
            map
        }
    };
    injected.insert("bridge_seq".to_owned(), bridge_seq.clone());
    Value::Object(injected)
}

/// `final` status coercion for message-complete (`chat.py:674-680`):
/// `data.status or COMPLETED`, reset to `COMPLETED` unless one of
/// the three final values. Never crashes (a set-membership test).
pub fn complete_final_status(raw: &Value) -> AgentChatMessageStatus {
    match raw.as_str().unwrap_or("") {
        "completed" => AgentChatMessageStatus::Completed,
        "cancelled" => AgentChatMessageStatus::Cancelled,
        "failed" => AgentChatMessageStatus::Failed,
        _ => AgentChatMessageStatus::Completed,
    }
}

/// Throttle-cache key (`SimpleRateThrottle.get_cache_key`):
/// `throttle_<scope>_<user pk>`.
pub fn chat_send_throttle_key(user_id: &Uuid) -> String {
    crate::assistant::throttles::cache_key(CHAT_SEND_SCOPE, &user_id.to_string())
}

/// Decode a cached throttle history: the JSON array of floats this
/// handler writes. Anything else decodes to the empty history —
/// fail-open to allow (the `assistant::redis` precedent).
pub fn decode_throttle_history(raw: &str) -> Vec<f64> {
    serde_json::from_str(raw).unwrap_or_default()
}

/// Encode a throttle history for the cache.
pub fn encode_throttle_history(history: &[f64]) -> String {
    serde_json::to_string(history).expect("float vec serializes")
}

/// The message-POST brake verdict: allow, or deny with DRF's `wait`
/// seconds for the `Retry-After` header (`views.py:92` — the
/// project's 429 rewrite keeps the default handler's headers).
pub enum ChatSendVerdict {
    Allow,
    Deny { retry_after_secs: String },
}

/// Check the message-POST brake (`chat.py:60-62,269-272`): the DRF
/// sliding window over the cached history, 60/minute. On allow the
/// caller records `now` at the front and re-caches with a 60s
/// timeout (`throttle_success`); on deny the caller answers 429 with
/// the rewritten body. A missing client, a cache miss, an unreadable
/// value, and a failed re-cache all fail open to allow.
pub async fn check_chat_send_throttle(state: &AppState, user_id: &Uuid) -> ChatSendVerdict {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs_f64())
        .unwrap_or(0.0);
    let verdict = evaluate_chat_send_throttle(&[], now);
    let key = chat_send_throttle_key(user_id);
    let Some(redis) = state.redis() else {
        return verdict;
    };
    let history: Vec<f64> = match redis.get_string(&key).await {
        Ok(Some(raw)) => decode_throttle_history(&raw),
        _ => Vec::new(),
    };
    let window = CHAT_SEND_WINDOW_SECS as f64;
    let live: Vec<f64> = history
        .into_iter()
        .filter(|&stamp| stamp > now - window)
        .collect();
    if live.len() >= CHAT_SEND_REQUESTS as usize {
        let wait = crate::assistant::throttles::throttle_wait(
            &live,
            now,
            CHAT_SEND_REQUESTS,
            CHAT_SEND_WINDOW_SECS,
        );
        // `'%d' % wait`: truncation toward zero; a `None` wait sends
        // no header (DRF skips falsy `exc.wait`).
        let retry_after_secs = wait
            .map(|wait| format!("{}", wait.trunc() as i64))
            .unwrap_or_default();
        return ChatSendVerdict::Deny { retry_after_secs };
    }
    let mut live = live;
    live.insert(0, now);
    if let Err(error) = redis
        .set_ex(&key, &encode_throttle_history(&live), CHAT_SEND_WINDOW_SECS)
        .await
    {
        tracing::debug!(%error, key = key.as_str(), "chat.throttle: re-cache failed; allowance stands");
    }
    ChatSendVerdict::Allow
}

/// The pure brake decision behind [`check_chat_send_throttle`], kept
/// so the algorithm stays unit-testable without a Redis server.
pub fn evaluate_chat_send_throttle(history: &[f64], now: f64) -> ChatSendVerdict {
    if crate::assistant::throttles::allow_request(
        history,
        now,
        CHAT_SEND_REQUESTS,
        CHAT_SEND_WINDOW_SECS,
    ) {
        ChatSendVerdict::Allow
    } else {
        ChatSendVerdict::Deny {
            retry_after_secs: String::new(),
        }
    }
}

/// Render the throttle denial: 429 with the project's rewritten
/// `RATE_LIMIT_EXCEEDED` body and DRF's `Retry-After` header (when
/// the wait computed one).
pub fn throttle_denied(retry_after_secs: &str) -> Response {
    let mut response = Response::builder()
        .status(StatusCode::TOO_MANY_REQUESTS)
        .header(axum::http::header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(
            crate::assistant::throttles::RATE_LIMIT_BODY,
        ))
        .expect("throttled response builds");
    if !retry_after_secs.is_empty() {
        response.headers_mut().insert(
            axum::http::header::RETRY_AFTER,
            retry_after_secs
                .parse()
                .expect("retry-after seconds render"),
        );
    }
    response
}

// ---------------------------------------------------------------------------
// Rows, fetches, guards, serialization
// ---------------------------------------------------------------------------

/// `agent_chat_session` columns in L2 order: id, workspace, runner,
/// creator, pod, status, agent_kind, local_thread, local_session,
/// cwd, model, active_turn, active_message, close_requested,
/// last_message_at, closed_at, error, created_at, updated_at.
/// Nineteen columns exceed sqlx's 16-tuple `FromRow`, so sessions
/// decode by position through [`session_from_pg`].
const SESSION_COLUMNS: &str = r#""id", "workspace_id", "runner_id", "created_by_id", "pod_id",
    "status", "agent_kind", "local_thread_id", "local_session_id", "cwd", "model",
    "active_turn_id", "active_message_id", "close_requested", "last_message_at",
    "closed_at", "error", "created_at", "updated_at""#;

/// The session columns qualified with the session table (for joins —
/// split on commas, since the constant spans lines).
fn qualified_session_columns() -> String {
    SESSION_COLUMNS
        .split(',')
        .map(|column| format!("agent_chat_session.{}", column.trim()))
        .collect::<Vec<_>>()
        .join(", ")
}

/// `agent_chat_message` columns in L2 order.
type MessageRow = (
    Uuid,
    Uuid,
    String,
    String,
    Value,
    String,
    String,
    String,
    i32,
    DateTime<Utc>,
    Option<DateTime<Utc>>,
);

/// `agent_chat_event` columns in L2 order.
type EventRow = (
    i64,
    Uuid,
    Option<Uuid>,
    i32,
    String,
    String,
    Value,
    DateTime<Utc>,
);

/// `agent_chat_approval` columns in L2 order.
type ChatApprovalRow = (
    Uuid,
    Uuid,
    String,
    String,
    Value,
    String,
    String,
    String,
    Option<Uuid>,
    DateTime<Utc>,
    Option<DateTime<Utc>>,
    Option<DateTime<Utc>>,
);

/// The runner columns the chat guards read: id, owner, workspace,
/// visibility, status. Unknown `visibility`/`status` values pass
/// through as-is (the guards compare, never parse).
pub struct RunnerGuard {
    pub id: Uuid,
    pub owner_id: Uuid,
    pub workspace_id: Uuid,
    pub visibility: i16,
    pub status: String,
}

type RunnerGuardRow = (Uuid, Uuid, Uuid, i16, String);

/// Build the L2 session from a row decoded by position. An
/// unknown status is a 500 (the L2 rows cannot hold it; no contract
/// input covers it).
fn session_from_pg(row: &sqlx::postgres::PgRow) -> Result<AgentChatSession, Response> {
    use sqlx::Row;
    let status_raw: String = row.try_get(5).map_err(|_| server_error())?;
    let status = pidash_types::runner_runs::AgentChatSessionStatus::from_value(&status_raw)
        .ok_or_else(server_error)?;
    Ok(AgentChatSession {
        id: row.try_get(0).map_err(|_| server_error())?,
        workspace_id: row.try_get(1).map_err(|_| server_error())?,
        runner_id: row.try_get(2).map_err(|_| server_error())?,
        created_by_id: row.try_get(3).map_err(|_| server_error())?,
        pod_id: row.try_get(4).map_err(|_| server_error())?,
        status,
        agent_kind: row.try_get(6).map_err(|_| server_error())?,
        local_thread_id: row.try_get(7).map_err(|_| server_error())?,
        local_session_id: row.try_get(8).map_err(|_| server_error())?,
        cwd: row.try_get(9).map_err(|_| server_error())?,
        model: row.try_get(10).map_err(|_| server_error())?,
        active_turn_id: row.try_get(11).map_err(|_| server_error())?,
        active_message_id: row.try_get(12).map_err(|_| server_error())?,
        close_requested: row.try_get(13).map_err(|_| server_error())?,
        last_message_at: row.try_get(14).map_err(|_| server_error())?,
        closed_at: row.try_get(15).map_err(|_| server_error())?,
        error: row.try_get(16).map_err(|_| server_error())?,
        created_at: row.try_get(17).map_err(|_| server_error())?,
        updated_at: row.try_get(18).map_err(|_| server_error())?,
    })
}

fn message_from_row(row: MessageRow) -> Result<AgentChatMessage, Response> {
    let role = AgentChatMessageRole::from_value(&row.2).ok_or_else(server_error)?;
    let status = AgentChatMessageStatus::from_value(&row.5).ok_or_else(server_error)?;
    Ok(AgentChatMessage {
        id: row.0,
        session_id: row.1,
        role,
        content: row.3,
        content_parts: row.4,
        status,
        local_item_id: row.6,
        local_turn_id: row.7,
        seq: row.8,
        created_at: row.9,
        completed_at: row.10,
    })
}

fn event_from_row(row: EventRow) -> Result<AgentChatEvent, Response> {
    Ok(AgentChatEvent {
        id: row.0,
        session_id: row.1,
        message_id: row.2,
        seq: row.3,
        source_key: row.4,
        kind: row.5,
        payload: row.6,
        created_at: row.7,
    })
}

fn chat_approval_from_row(row: ChatApprovalRow) -> Result<AgentChatApprovalRequest, Response> {
    let kind =
        pidash_types::runner_runs::ApprovalKind::from_value(&row.3).ok_or_else(server_error)?;
    let status = ApprovalStatus::from_value(&row.6).ok_or_else(server_error)?;
    Ok(AgentChatApprovalRequest {
        id: row.0,
        session_id: row.1,
        local_approval_id: row.2,
        kind,
        payload: row.4,
        reason: row.5,
        status,
        decision_source: row.7,
        decided_by_id: row.8,
        requested_at: row.9,
        expires_at: row.10,
        decided_at: row.11,
    })
}

/// The session plus its runner's guard columns, as the locked web
/// reads fetch them (`select_for_update` with `select_related`).
pub struct LockedChatSession {
    pub session: AgentChatSession,
    pub runner: RunnerGuard,
}

/// Lock one session row (no join — the runner locks separately
/// below, since sqlx cannot decode nested tuples).
async fn lock_session_row(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    session_id: Uuid,
) -> Result<Option<AgentChatSession>, Response> {
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(&format!(
        r#"SELECT {SESSION_COLUMNS} FROM "agent_chat_session"
           WHERE "id" = $1 LIMIT 1 FOR UPDATE"#
    ))
    .bind(session_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| server_error())?;
    row.map(|row| session_from_pg(&row)).transpose()
}

/// Lock one runner's guard columns.
async fn lock_runner_guard(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    runner_id: Uuid,
) -> Result<Option<RunnerGuard>, Response> {
    let row: Option<RunnerGuardRow> = sqlx::query_as(
        r#"SELECT "id", "owner_id", "workspace_id", "visibility", "status"
           FROM "runner" WHERE "id" = $1 LIMIT 1 FOR UPDATE"#,
    )
    .bind(runner_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| server_error())?;
    Ok(row.map(|row| RunnerGuard {
        id: row.0,
        owner_id: row.1,
        workspace_id: row.2,
        visibility: row.3,
        status: row.4,
    }))
}

/// Lock the session row with its runner's guard row — the same two
/// locks Django's `select_for_update` + `select_related` takes.
/// `None` is a missing session (the caller answers 404); a vanished
/// runner under a live session is the source's
/// `RelatedObjectDoesNotExist` (500).
async fn lock_chat_session(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    session_id: Uuid,
) -> Result<Option<LockedChatSession>, Response> {
    let Some(session) = lock_session_row(tx, session_id).await? else {
        return Ok(None);
    };
    let Some(runner) = lock_runner_guard(tx, session.runner_id).await? else {
        return Err(server_error());
    };
    Ok(Some(LockedChatSession { session, runner }))
}

/// Fetch the session with its runner's guard columns, unlocked (the
/// GET detail/messages shape). Shared with the SSE stream.
pub(crate) async fn fetch_session_with_runner(
    pool: &PgPool,
    session_id: Uuid,
) -> Result<Option<(AgentChatSession, RunnerGuard)>, Response> {
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(&format!(
        r#"SELECT {SESSION_COLUMNS} FROM "agent_chat_session" WHERE "id" = $1"#
    ))
    .bind(session_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| server_error())?;
    let Some(row) = row else {
        return Ok(None);
    };
    let session = session_from_pg(&row)?;
    let guard: Option<RunnerGuardRow> = sqlx::query_as(
        r#"SELECT "id", "owner_id", "workspace_id", "visibility", "status"
           FROM "runner" WHERE "id" = $1"#,
    )
    .bind(session.runner_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| server_error())?;
    let Some(guard) = guard else {
        return Err(server_error());
    };
    Ok(Some((
        session,
        RunnerGuard {
            id: guard.0,
            owner_id: guard.1,
            workspace_id: guard.2,
            visibility: guard.3,
            status: guard.4,
        },
    )))
}

/// The newest live workspace role (`core/permissions.py:39-51` plus
/// the soft-delete filter): `None` when the user holds no live
/// membership.
async fn workspace_role(
    executor: &PgPool,
    workspace_id: Uuid,
    user_id: Uuid,
) -> Result<Option<i32>, Response> {
    workspace_role_in(executor, workspace_id, user_id).await
}

async fn workspace_role_in(
    executor: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    workspace_id: Uuid,
    user_id: Uuid,
) -> Result<Option<i32>, Response> {
    let role: Option<i16> = sqlx::query_scalar(
        r#"SELECT "role" FROM "workspace_members"
           WHERE ("workspace_id" = $1 AND "member_id" = $2
             AND "is_active" AND "deleted_at" IS NULL)
           ORDER BY "created_at" DESC LIMIT 1"#,
    )
    .bind(workspace_id)
    .bind(user_id)
    .fetch_optional(executor)
    .await
    .map_err(|_| server_error())?;
    Ok(role.map(i32::from))
}

/// `can_view_runner` / `can_use_runner`
/// (`permissions.py:39-87`): private and owned by the requester.
fn runner_visible_to_user(runner: &RunnerGuard, user_id: Uuid) -> bool {
    use pidash_auth::permissions::runner::{can_view_runner, RunnerFacts};
    use pidash_types::WorkspaceId;
    can_view_runner(&RunnerFacts {
        workspace: WorkspaceId::from(runner.workspace_id.to_string()),
        authenticated: true,
        visibility: i32::from(runner.visibility),
        owned_by_requester: runner.owner_id == user_id,
    })
}

/// The daemon `_resolve` (`chat.py:513-531`): the session's identity
/// plus its runner binding. `runner` is `None` for anonymous
/// callers — and `None.id` is the ported 500 (`chat.py:522`), not a
/// 401.
pub struct ResolvedChatSession {
    pub id: Uuid,
    pub runner_id: Uuid,
}

/// Resolve a daemon chat session: 404 when missing, 500 when the
/// caller is anonymous (the ported `AttributeError`), 403 unless the
/// authenticated runner owns the session.
pub async fn resolve_chat_session(
    pool: &PgPool,
    session_id: Uuid,
    runner: Option<&DaemonRunner>,
) -> Result<ResolvedChatSession, Response> {
    let row: Option<(Uuid, Uuid)> =
        sqlx::query_as(r#"SELECT "id", "runner_id" FROM "agent_chat_session" WHERE "id" = $1"#)
            .bind(session_id)
            .fetch_optional(pool)
            .await
            .map_err(|_| server_error())?;
    let Some((id, runner_id)) = row else {
        return Err(json_response(
            StatusCode::NOT_FOUND,
            r#"{"error":"chat_session_not_found"}"#.to_owned(),
        ));
    };
    // `session.runner_id != request.auth_runner.id`: the `None`
    // dereference is the ported bug (500), reached before any 401.
    let Some(runner) = runner else {
        return Err(server_error());
    };
    if runner_id != runner.id {
        return Err(json_response(
            StatusCode::FORBIDDEN,
            r#"{"error":"chat_session_not_owned_by_runner"}"#.to_owned(),
        ));
    }
    Ok(ResolvedChatSession { id, runner_id })
}

/// `chat_session_not_found` for unparseable daemon ids (the
/// `assistant/events.rs` precedent: the endpoint's 404 body).
fn daemon_chat_not_found() -> Response {
    json_response(
        StatusCode::NOT_FOUND,
        r#"{"error":"chat_session_not_found"}"#.to_owned(),
    )
}

/// The project's deploy-admin role floor (`ROLE_ADMIN`, 20).
const ROLE_ADMIN: i32 = 20;

// ---------------------------------------------------------------------------
// `runner_detail` (the D-13 `RunnerSerializer` reuse)
// ---------------------------------------------------------------------------

/// Owned strings behind a [`RunnerRow`]: the runner row plus its pod
/// mini (with the project identifier), dev-machine mini, and
/// optional live state. One fetch per part, exactly as the source's
/// `select_related` + lazy nested serializers read them.
pub struct RunnerDetailOwned {
    pub id: String,
    pub name: String,
    pub status: String,
    pub host_label: String,
    pub provisioning: String,
    pub os: String,
    pub arch: String,
    pub runner_version: String,
    pub dev_metadata: Value,
    pub protocol_version: i64,
    pub capabilities: Value,
    pub last_heartbeat_at: Option<String>,
    pub owner: String,
    pub dev_machine: Option<String>,
    pub dev_machine_detail: Option<DevMachineMiniOwned>,
    pub visibility: i64,
    pub pod: String,
    pub pod_detail: PodMiniOwned,
    pub live_state: Option<LiveStateOwned>,
    pub enrolled_at: Option<String>,
    pub revoked_at: Option<String>,
    pub revoked_reason: String,
    pub created_at: String,
    pub updated_at: String,
}

pub struct PodMiniOwned {
    pub id: String,
    pub name: String,
    pub is_default: bool,
    pub project: String,
    pub project_identifier: String,
}

pub struct DevMachineMiniOwned {
    pub id: String,
    pub host_label: String,
    pub label: String,
}

pub struct LiveStateOwned {
    pub observed_run_id: Option<String>,
    pub last_event_at: Option<String>,
    pub last_event_kind: Option<String>,
    pub last_event_summary: Option<String>,
    pub agent_pid: Option<i64>,
    pub agent_subprocess_alive: Option<bool>,
    pub approvals_pending: Option<i64>,
    pub usage: Value,
    pub llm_model: Option<String>,
    pub turn_count: Option<i64>,
    pub updated_at: String,
}

fn drf_opt(moment: &Option<DateTime<Utc>>) -> Option<String> {
    use pidash_services::runner_runs::shape::drf_datetime;
    moment.as_ref().map(drf_datetime)
}

impl RunnerDetailOwned {
    fn pod_row(&self) -> pidash_services::runner_enroll::serializers::shapes::PodMiniRow<'_> {
        pidash_services::runner_enroll::serializers::shapes::PodMiniRow {
            id: &self.pod_detail.id,
            name: &self.pod_detail.name,
            is_default: self.pod_detail.is_default,
            project: &self.pod_detail.project,
            project_identifier: &self.pod_detail.project_identifier,
        }
    }

    fn dev_machine_row(
        &self,
    ) -> Option<pidash_services::runner_enroll::serializers::shapes::DevMachineMiniRow<'_>> {
        self.dev_machine_detail.as_ref().map(|detail| {
            pidash_services::runner_enroll::serializers::shapes::DevMachineMiniRow {
                id: &detail.id,
                host_label: &detail.host_label,
                label: &detail.label,
            }
        })
    }

    fn live_state_row(
        &self,
    ) -> Option<pidash_services::runner_enroll::serializers::shapes::LiveStateRow<'_>> {
        self.live_state.as_ref().map(|live| {
            pidash_services::runner_enroll::serializers::shapes::LiveStateRow {
                observed_run_id: live.observed_run_id.as_deref(),
                last_event_at: live.last_event_at.as_deref(),
                last_event_kind: live.last_event_kind.as_deref(),
                last_event_summary: live.last_event_summary.as_deref(),
                agent_pid: live.agent_pid,
                agent_subprocess_alive: live.agent_subprocess_alive,
                approvals_pending: live.approvals_pending,
                usage: &live.usage,
                llm_model: live.llm_model.as_deref(),
                turn_count: live.turn_count,
                updated_at: &live.updated_at,
            }
        })
    }

    /// Borrow the D-13 shape input (`PodMiniRow` is `Clone`, so the
    /// nested pod row moves in by value).
    fn row_with_pod<'a>(
        &'a self,
        pod: pidash_services::runner_enroll::serializers::shapes::PodMiniRow<'a>,
        dev_machine: Option<
            pidash_services::runner_enroll::serializers::shapes::DevMachineMiniRow<'a>,
        >,
        live_state: Option<pidash_services::runner_enroll::serializers::shapes::LiveStateRow<'a>>,
    ) -> pidash_services::runner_enroll::serializers::shapes::RunnerRow<'a> {
        pidash_services::runner_enroll::serializers::shapes::RunnerRow {
            id: &self.id,
            name: &self.name,
            status: &self.status,
            host_label: &self.host_label,
            provisioning: &self.provisioning,
            os: &self.os,
            arch: &self.arch,
            runner_version: &self.runner_version,
            dev_metadata: &self.dev_metadata,
            protocol_version: self.protocol_version,
            capabilities: &self.capabilities,
            last_heartbeat_at: self.last_heartbeat_at.as_deref(),
            owner: &self.owner,
            dev_machine: self.dev_machine.as_deref(),
            dev_machine_detail: dev_machine,
            visibility: self.visibility,
            pod: &self.pod,
            pod_detail: pod,
            live_state,
            enrolled_at: self.enrolled_at.as_deref(),
            revoked_at: self.revoked_at.as_deref(),
            revoked_reason: &self.revoked_reason,
            created_at: &self.created_at,
            updated_at: &self.updated_at,
        }
    }
}

/// Fetch the runner row with its pod mini, dev-machine mini, and live
/// state. A missing runner (or a dangling pod/dev-machine FK) is the
/// source's `DoesNotExist` on nested serialization (500); a missing
/// live state renders `null`.
pub async fn fetch_runner_detail(
    pool: &PgPool,
    runner_id: Uuid,
) -> Result<RunnerDetailOwned, Response> {
    use pidash_services::runner_runs::shape::drf_datetime;
    use sqlx::Row;
    // Twenty-one columns exceed sqlx's 16-tuple `FromRow`: decode by
    // position. Order: id, name, status, host_label, provisioning,
    // os, arch, runner_version, dev_metadata, protocol_version,
    // capabilities, last_heartbeat_at, owner, dev_machine,
    // visibility, pod, enrolled_at, revoked_at, revoked_reason,
    // created_at, updated_at.
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(
        r#"SELECT "id", "name", "status", "host_label", "provisioning", "os", "arch",
              "runner_version", "dev_metadata", "protocol_version", "capabilities",
              "last_heartbeat_at", "owner_id", "dev_machine_id", "visibility", "pod_id",
              "enrolled_at", "revoked_at", "revoked_reason", "created_at", "updated_at"
           FROM "runner" WHERE "id" = $1"#,
    )
    .bind(runner_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| server_error())?;
    let Some(row) = row else {
        return Err(server_error());
    };
    let get_uuid = |idx: usize| row.try_get::<Uuid, usize>(idx).map_err(|_| server_error());
    let get_string = |idx: usize| {
        row.try_get::<String, usize>(idx)
            .map_err(|_| server_error())
    };
    let get_opt_uuid = |idx: usize| {
        row.try_get::<Option<Uuid>, usize>(idx)
            .map_err(|_| server_error())
    };
    let get_opt_moment = |idx: usize| {
        row.try_get::<Option<DateTime<Utc>>, usize>(idx)
            .map_err(|_| server_error())
    };
    let get_moment = |idx: usize| {
        row.try_get::<DateTime<Utc>, usize>(idx)
            .map_err(|_| server_error())
    };
    let dev_machine_id = get_opt_uuid(13)?;
    let pod_id = get_uuid(15)?;
    let dev_machine_detail = match dev_machine_id {
        Some(dev_machine_id) => {
            let mini: Option<(Uuid, String, String)> = sqlx::query_as(
                r#"SELECT "id", "host_label", "label" FROM "dev_machine" WHERE "id" = $1"#,
            )
            .bind(dev_machine_id)
            .fetch_optional(pool)
            .await
            .map_err(|_| server_error())?;
            match mini {
                Some((id, host_label, label)) => Some(DevMachineMiniOwned {
                    id: id.to_string(),
                    host_label,
                    label,
                }),
                None => return Err(server_error()),
            }
        }
        None => None,
    };
    let pod: Option<(Uuid, String, bool, Uuid, String)> = sqlx::query_as(
        r#"SELECT "pod"."id", "pod"."name", "pod"."is_default",
              "pod"."project_id", "projects"."identifier"
           FROM "pod" INNER JOIN "projects" ON ("pod"."project_id" = "projects"."id")
           WHERE "pod"."id" = $1"#,
    )
    .bind(pod_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| server_error())?;
    let Some((pod_id, pod_name, pod_default, project_id, project_identifier)) = pod else {
        return Err(server_error());
    };
    type LiveRow = (
        Option<Uuid>,
        Option<DateTime<Utc>>,
        Option<String>,
        Option<String>,
        Option<i32>,
        Option<bool>,
        Option<i32>,
        Value,
        Option<String>,
        Option<i32>,
        DateTime<Utc>,
    );
    let live: Option<LiveRow> = sqlx::query_as(
        r#"SELECT "observed_run_id", "last_event_at", "last_event_kind",
              "last_event_summary", "agent_pid", "agent_subprocess_alive",
              "approvals_pending", "usage", "llm_model", "turn_count", "updated_at"
           FROM "runner_live_state" WHERE "runner_id" = $1"#,
    )
    .bind(runner_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| server_error())?;
    let id = get_uuid(0)?;
    let name = get_string(1)?;
    let status = get_string(2)?;
    let host_label = get_string(3)?;
    let provisioning = get_string(4)?;
    let os = get_string(5)?;
    let arch = get_string(6)?;
    let runner_version = get_string(7)?;
    let dev_metadata: Value = row.try_get(8).map_err(|_| server_error())?;
    let protocol_version: i32 = row.try_get(9).map_err(|_| server_error())?;
    let capabilities: Value = row.try_get(10).map_err(|_| server_error())?;
    let last_heartbeat_at = get_opt_moment(11)?;
    let owner = get_uuid(12)?;
    let visibility: i16 = row.try_get(14).map_err(|_| server_error())?;
    let enrolled_at = get_opt_moment(16)?;
    let revoked_at = get_opt_moment(17)?;
    let revoked_reason = get_string(18)?;
    let created_at = get_moment(19)?;
    let updated_at = get_moment(20)?;
    Ok(RunnerDetailOwned {
        id: id.to_string(),
        name,
        status,
        host_label,
        provisioning,
        os,
        arch,
        runner_version,
        dev_metadata,
        protocol_version: i64::from(protocol_version),
        capabilities,
        last_heartbeat_at: drf_opt(&last_heartbeat_at),
        owner: owner.to_string(),
        dev_machine: dev_machine_id.map(|id| id.to_string()),
        dev_machine_detail,
        visibility: i64::from(visibility),
        pod: pod_id.to_string(),
        pod_detail: PodMiniOwned {
            id: pod_id.to_string(),
            name: pod_name,
            is_default: pod_default,
            project: project_id.to_string(),
            project_identifier,
        },
        live_state: live.map(|live| LiveStateOwned {
            observed_run_id: live.0.map(|id| id.to_string()),
            last_event_at: drf_opt(&live.1),
            last_event_kind: live.2,
            last_event_summary: live.3,
            agent_pid: live.4.map(i64::from),
            agent_subprocess_alive: live.5,
            approvals_pending: live.6.map(i64::from),
            usage: live.7,
            llm_model: live.8,
            turn_count: live.9.map(i64::from),
            updated_at: drf_datetime(&live.10),
        }),
        enrolled_at: drf_opt(&enrolled_at),
        revoked_at: drf_opt(&revoked_at),
        revoked_reason,
        created_at: drf_datetime(&created_at),
        updated_at: drf_datetime(&updated_at),
    })
}

/// Serialize a session with its runner detail (the list/detail/create
/// shape).
pub fn render_session(session: &AgentChatSession, detail: &RunnerDetailOwned) -> String {
    use pidash_services::runner_runs::shape::chat_session_to_representation;
    let pod = detail.pod_row();
    let dev_machine = detail.dev_machine_row();
    let live_state = detail.live_state_row();
    let runner_row = detail.row_with_pod(pod, dev_machine, live_state);
    let view = chat_session_to_representation(session, &runner_row);
    serde_json::to_string(&view).expect("session view serializes")
}

/// Serialize a message, event, or chat approval (the detail shapes).
pub fn render_message(message: &AgentChatMessage) -> String {
    use pidash_services::runner_runs::shape::chat_message_to_representation;
    serde_json::to_string(&chat_message_to_representation(message)).expect("message serializes")
}

pub fn render_event(event: &AgentChatEvent) -> String {
    use pidash_services::runner_runs::shape::chat_event_to_representation;
    serde_json::to_string(&chat_event_to_representation(event)).expect("event serializes")
}

pub fn render_chat_approval(approval: &AgentChatApprovalRequest) -> String {
    use pidash_services::runner_runs::shape::chat_approval_to_representation;
    serde_json::to_string(&chat_approval_to_representation(approval)).expect("approval serializes")
}

// ---------------------------------------------------------------------------
// L5 executors (`services/chat.py`)
// ---------------------------------------------------------------------------

/// Commit the endpoint transaction, mapping a commit failure to the
/// 500 the source's `Atomic.__exit__` raises.
async fn commit_tx(tx: sqlx::Transaction<'_, sqlx::Postgres>) -> Result<(), Response> {
    tx.commit().await.map_err(|_| server_error())
}

/// `next_event_seq` (`chat.py:218-224`): one past the session max.
async fn next_event_seq(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    session_id: Uuid,
) -> Result<i32, Response> {
    let max: Option<i32> =
        sqlx::query_scalar(r#"SELECT MAX("seq") FROM "agent_chat_event" WHERE "session_id" = $1"#)
            .bind(session_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(|_| server_error())?
            .flatten();
    Ok(chat_kernel::next_seq_after_max(max))
}

/// `next_message_seq` (`chat.py:227-233`): one past the session max.
async fn next_message_seq(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    session_id: Uuid,
) -> Result<i32, Response> {
    let max: Option<i32> = sqlx::query_scalar(
        r#"SELECT MAX("seq") FROM "agent_chat_message" WHERE "session_id" = $1"#,
    )
    .bind(session_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| server_error())?
    .flatten();
    Ok(chat_kernel::next_seq_after_max(max))
}

/// `append_event_locked` (`chat.py:236-247`): the idempotent event
/// insert. An existing `source_key` returns the stored row without
/// queuing a publish; otherwise the row inserts with the next `seq`
/// and a [`ChatEffect::PublishEvent`] queues.
pub async fn append_event(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    session_id: Uuid,
    inputs: &NewEventInputs,
    effects: &mut Vec<ChatEffect>,
) -> Result<AgentChatEvent, Response> {
    if !inputs.source_key.is_empty() {
        let existing: Option<EventRow> = sqlx::query_as(
            r#"SELECT "id", "session_id", "message_id", "seq", "source_key", "kind",
                  "payload", "created_at"
               FROM "agent_chat_event"
               WHERE ("session_id" = $1 AND "source_key" = $2)"#,
        )
        .bind(session_id)
        .bind(&inputs.source_key)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|_| server_error())?;
        if let Some(existing) = existing {
            return event_from_row(existing);
        }
    }
    let seq = next_event_seq(tx, session_id).await?;
    let created_at = Utc::now();
    let id: i64 = sqlx::query_scalar(
        r#"INSERT INTO "agent_chat_event"
           ("session_id", "message_id", "seq", "source_key", "kind", "payload", "created_at")
           VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING "id""#,
    )
    .bind(session_id)
    .bind(inputs.message_id)
    .bind(seq)
    .bind(&inputs.source_key)
    .bind(&inputs.kind)
    .bind(&inputs.payload)
    .bind(created_at)
    .fetch_one(&mut **tx)
    .await
    .map_err(|_| server_error())?;
    effects.push(ChatEffect::PublishEvent { event_id: id });
    Ok(AgentChatEvent {
        id,
        session_id,
        message_id: inputs.message_id,
        seq,
        source_key: inputs.source_key.clone(),
        kind: inputs.kind.clone(),
        payload: inputs.payload.clone(),
        created_at,
    })
}

/// `record_dedupe` (`chat.py:180-190`): the chat idempotency insert.
/// A duplicate rolls back the caller's transaction — the source has
/// no savepoint here, so the unique violation aborts the Postgres
/// transaction and the endpoint still answers 200 `duplicate` with
/// nothing persisted. The caller rolls back and answers; `Ok(true)`
/// continues the endpoint.
pub async fn record_chat_dedupe(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    session_id: Uuid,
    key: &str,
) -> Result<bool, Response> {
    if key.is_empty() {
        return Ok(true);
    }
    let message_id = chat_kernel::record_dedupe_message_id(key);
    let inserted = sqlx::query(
        r#"INSERT INTO "chat_message_dedupe" ("session_id", "message_id", "created_at")
           VALUES ($1, $2, $3)"#,
    )
    .bind(session_id)
    .bind(&message_id)
    .bind(Utc::now())
    .execute(&mut **tx)
    .await;
    match inserted {
        Ok(_) => Ok(true),
        Err(error) => {
            let unique_violation = error
                .as_database_error()
                .and_then(|db| db.code())
                .is_some_and(|code| code == "23505");
            if unique_violation {
                Ok(false)
            } else {
                Err(server_error())
            }
        }
    }
}

/// `create_assistant_message_locked` (`chat.py:250-275`): the empty
/// streaming assistant placeholder for a turn.
pub async fn create_assistant(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    session_id: Uuid,
    local_turn_id: &str,
) -> Result<AgentChatMessage, Response> {
    let inputs =
        chat_kernel::create_assistant_inputs(local_turn_id, "", AgentChatMessageStatus::Streaming);
    let seq = next_message_seq(tx, session_id).await?;
    let id = Uuid::new_v4();
    let created_at = Utc::now();
    sqlx::query(
        r#"INSERT INTO "agent_chat_message"
           ("id", "session_id", "role", "content", "content_parts", "status",
            "local_item_id", "local_turn_id", "seq", "created_at", "completed_at")
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)"#,
    )
    .bind(id)
    .bind(session_id)
    .bind(AgentChatMessageRole::Assistant.value())
    .bind("")
    .bind(Value::Array(Vec::new()))
    .bind(inputs.status.value())
    .bind("")
    .bind(&inputs.local_turn_id)
    .bind(seq)
    .bind(created_at)
    .bind(None::<DateTime<Utc>>)
    .execute(&mut **tx)
    .await
    .map_err(|_| server_error())?;
    Ok(AgentChatMessage {
        id,
        session_id,
        role: AgentChatMessageRole::Assistant,
        content: String::new(),
        content_parts: Value::Array(Vec::new()),
        status: inputs.status,
        local_item_id: String::new(),
        local_turn_id: inputs.local_turn_id,
        seq,
        created_at,
        completed_at: None,
    })
}

/// `active_assistant_message_locked` (`chat.py:278-295`): the newest
/// streaming assistant for the active turn, else the newest streaming
/// assistant on the session.
pub async fn active_assistant(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    session: &AgentChatSession,
) -> Result<Option<AgentChatMessage>, Response> {
    let scoped: Option<MessageRow> = sqlx::query_as(
        r#"SELECT "id", "session_id", "role", "content", "content_parts", "status",
              "local_item_id", "local_turn_id", "seq", "created_at", "completed_at"
           FROM "agent_chat_message"
           WHERE ("session_id" = $1 AND "local_turn_id" = $2 AND "status" = $3)
           ORDER BY "created_at" DESC LIMIT 1"#,
    )
    .bind(session.id)
    .bind(&session.active_turn_id)
    .bind(AgentChatMessageStatus::Streaming.value())
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| server_error())?;
    if let Some(scoped) = scoped {
        return Ok(Some(message_from_row(scoped)?));
    }
    let fallback: Option<MessageRow> = sqlx::query_as(
        r#"SELECT "id", "session_id", "role", "content", "content_parts", "status",
              "local_item_id", "local_turn_id", "seq", "created_at", "completed_at"
           FROM "agent_chat_message"
           WHERE ("session_id" = $1 AND "role" = $2 AND "status" = $3)
           ORDER BY "created_at" DESC LIMIT 1"#,
    )
    .bind(session.id)
    .bind(AgentChatMessageRole::Assistant.value())
    .bind(AgentChatMessageStatus::Streaming.value())
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| server_error())?;
    fallback.map(message_from_row).transpose()
}

/// `finalize_active_messages_locked` (`chat.py:298-338`) with no
/// explicit message override (the daemon failed/closed shape):
/// finalize the active message plus the active assistant to
/// `status`, returning the finalized explicit id, if any.
pub async fn finalize_active(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    session: &AgentChatSession,
    status: AgentChatMessageStatus,
) -> Result<Option<Uuid>, Response> {
    let target = chat_kernel::finalize_target_message_id(None, session.active_message_id);
    let explicit = match target {
        Some(target) => {
            let row: Option<MessageRow> = sqlx::query_as(
                r#"SELECT "id", "session_id", "role", "content", "content_parts", "status",
                      "local_item_id", "local_turn_id", "seq", "created_at", "completed_at"
                   FROM "agent_chat_message"
                   WHERE ("session_id" = $1 AND "id" = $2)"#,
            )
            .bind(session.id)
            .bind(target)
            .fetch_optional(&mut **tx)
            .await
            .map_err(|_| server_error())?;
            row.map(message_from_row).transpose()?
        }
        None => None,
    };
    let assistant = active_assistant(tx, session).await?;
    let plan = chat_kernel::plan_finalize_active_messages(
        explicit.as_ref(),
        assistant.as_ref(),
        status,
        Utc::now(),
    );
    for update in &plan.updates {
        // `message_status_update_sql` order: status, completed_at, id.
        sqlx::query(
            r#"UPDATE "agent_chat_message" SET "status" = $1, "completed_at" = $2
               WHERE "id" = $3"#,
        )
        .bind(update.status.value())
        .bind(update.completed_at)
        .bind(update.id)
        .execute(&mut **tx)
        .await
        .map_err(|_| server_error())?;
    }
    Ok(plan.returned_message_id)
}

/// Apply message-status updates from any fail/complete plan.
async fn apply_message_updates(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    updates: &[chat_kernel::MessageStatusUpdate],
) -> Result<(), Response> {
    for update in updates {
        sqlx::query(
            r#"UPDATE "agent_chat_message" SET "status" = $1, "completed_at" = $2
               WHERE "id" = $3"#,
        )
        .bind(update.status.value())
        .bind(update.completed_at)
        .bind(update.id)
        .execute(&mut **tx)
        .await
        .map_err(|_| server_error())?;
    }
    Ok(())
}

/// `complete_active_turn_locked` (`chat.py:359-400`): the active
/// message moves to `final_status`, the turn clears, `turn_completed`
/// fires (plus `chat_closed` when `close_requested` closes), and the
/// drain queues iff the turn was active.
pub async fn complete_turn(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    session: &AgentChatSession,
    final_status: AgentChatMessageStatus,
    payload: &Value,
    effects: &mut Vec<ChatEffect>,
) -> Result<(), Response> {
    let now = Utc::now();
    let plan = chat_kernel::plan_complete_active_turn(session, final_status, Some(payload), now);
    if let Some(update) = &plan.message_update {
        // Unscoped by session (BUG-COMPLETE-UNSCOPED, ported as-is).
        sqlx::query(
            r#"UPDATE "agent_chat_message" SET "status" = $1, "completed_at" = $2
               WHERE "id" = $3"#,
        )
        .bind(update.status.value())
        .bind(update.completed_at)
        .bind(update.id)
        .execute(&mut **tx)
        .await
        .map_err(|_| server_error())?;
    }
    // `complete_session_update_sql`: six columns in model field
    // order; `status`/`closed_at` carry the session values when not
    // closing, `updated_at` is `auto_now`.
    let status = if plan.close_session {
        AgentChatSessionStatus::Closed.value()
    } else {
        session.status.value()
    };
    let closed_at = plan.closed_at.or(session.closed_at);
    sqlx::query(
        r#"UPDATE "agent_chat_session"
           SET "status" = $1, "active_turn_id" = $2, "active_message_id" = $3,
               "closed_at" = $4, "last_message_at" = $5, "updated_at" = $6
           WHERE "id" = $7"#,
    )
    .bind(status)
    .bind("")
    .bind(None::<Uuid>)
    .bind(closed_at)
    .bind(plan.last_message_at)
    .bind(Utc::now())
    .bind(session.id)
    .execute(&mut **tx)
    .await
    .map_err(|_| server_error())?;
    for event in &plan.events {
        append_event(tx, session.id, event, effects).await?;
    }
    if plan.queue_drain {
        effects.push(ChatEffect::DrainTasks {
            runner_id: session.runner_id,
            pod_id: Some(session.pod_id),
        });
    }
    Ok(())
}

/// The daemon failed flow (`chat.py:699-735`): finalize the active
/// messages to `FAILED`, store the truncated error, append
/// `chat_failed` (with the FULL detail — QUIRK-ERROR-TRUNCATION —
/// and the `Value`-verbatim code), close when `close_requested`,
/// and drain iff the turn was active.
pub async fn execute_fail_session(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    session: &AgentChatSession,
    code: &Value,
    detail: &str,
    effects: &mut Vec<ChatEffect>,
) -> Result<(), Response> {
    let finalized = finalize_active(tx, session, AgentChatMessageStatus::Failed).await?;
    let should_close = session.close_requested;
    // `FAIL_SESSION_UPDATE_FIELDS` + `FAIL_SESSION_CLOSE_FIELDS` in
    // model field order.
    if should_close {
        sqlx::query(
            r#"UPDATE "agent_chat_session"
               SET "status" = $1, "active_turn_id" = $2, "active_message_id" = $3,
                   "closed_at" = $4, "error" = $5, "updated_at" = $6
               WHERE "id" = $7"#,
        )
        .bind(AgentChatSessionStatus::Closed.value())
        .bind("")
        .bind(None::<Uuid>)
        .bind(Utc::now())
        .bind(truncate_chars(detail, 2000))
        .bind(Utc::now())
        .bind(session.id)
        .execute(&mut **tx)
        .await
        .map_err(|_| server_error())?;
    } else {
        sqlx::query(
            r#"UPDATE "agent_chat_session"
               SET "active_turn_id" = $1, "active_message_id" = $2,
                   "error" = $3, "updated_at" = $4
               WHERE "id" = $5"#,
        )
        .bind("")
        .bind(None::<Uuid>)
        .bind(truncate_chars(detail, 2000))
        .bind(Utc::now())
        .bind(session.id)
        .execute(&mut **tx)
        .await
        .map_err(|_| server_error())?;
    }
    let mut payload = serde_json::Map::with_capacity(2);
    payload.insert("code".to_owned(), code.clone());
    payload.insert("detail".to_owned(), Value::String(detail.to_owned()));
    let inputs = chat_kernel::append_event_inputs(
        "chat_failed",
        Some(&Value::Object(payload)),
        finalized,
        "",
    );
    append_event(tx, session.id, &inputs, effects).await?;
    if should_close {
        let inputs = chat_kernel::append_event_inputs(
            "chat_closed",
            Some(&serde_json::json!({"close_requested": true})),
            None,
            "",
        );
        append_event(tx, session.id, &inputs, effects).await?;
    }
    if session.active_message_id.is_some() {
        effects.push(ChatEffect::DrainTasks {
            runner_id: session.runner_id,
            pod_id: Some(session.pod_id),
        });
    }
    Ok(())
}

/// The daemon closed flow (`chat.py:754-780`): finalize the active
/// messages to `CANCELLED`, close the session, append `chat_closed`,
/// and drain iff the turn was active.
pub async fn execute_close_session(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    session: &AgentChatSession,
    reason: &Value,
    effects: &mut Vec<ChatEffect>,
) -> Result<(), Response> {
    let was_active = session.active_message_id.is_some();
    if was_active {
        finalize_active(tx, session, AgentChatMessageStatus::Cancelled).await?;
    }
    sqlx::query(
        r#"UPDATE "agent_chat_session"
           SET "status" = $1, "active_turn_id" = $2, "active_message_id" = $3,
               "closed_at" = $4, "updated_at" = $5
           WHERE "id" = $6"#,
    )
    .bind(AgentChatSessionStatus::Closed.value())
    .bind("")
    .bind(None::<Uuid>)
    .bind(Utc::now())
    .bind(Utc::now())
    .bind(session.id)
    .execute(&mut **tx)
    .await
    .map_err(|_| server_error())?;
    let inputs = chat_kernel::append_event_inputs(
        "chat_closed",
        Some(&serde_json::json!({"reason": reason})),
        None,
        "",
    );
    append_event(tx, session.id, &inputs, effects).await?;
    if was_active {
        effects.push(ChatEffect::DrainTasks {
            runner_id: session.runner_id,
            pod_id: Some(session.pod_id),
        });
    }
    Ok(())
}

/// `mark_message_dispatch_failed` (`chat.py:270-296`) in its own
/// transaction: the dispatch-failure backstop behind the message and
/// warm enqueue effects. Errors propagate — the backstop runs inside
/// the enqueue `on_commit`, unisolated there.
pub async fn execute_mark_message_dispatch_failed(
    pool: &PgPool,
    session_id: Uuid,
    message_id: Option<Uuid>,
    detail: &str,
    effects: &mut Vec<ChatEffect>,
) -> Result<(), Response> {
    let mut tx = pool.begin().await.map_err(|_| server_error())?;
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(&format!(
        r#"SELECT {SESSION_COLUMNS} FROM "agent_chat_session"
           WHERE "id" = $1 LIMIT 1 FOR UPDATE"#
    ))
    .bind(session_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| server_error())?;
    let Some(row) = row else {
        tx.rollback().await.map_err(|_| server_error())?;
        return Ok(());
    };
    let session = session_from_pg(&row)?;
    let target = chat_kernel::finalize_target_message_id(message_id, session.active_message_id);
    let explicit = match target {
        Some(target) => {
            let row: Option<MessageRow> = sqlx::query_as(
                r#"SELECT "id", "session_id", "role", "content", "content_parts", "status",
                      "local_item_id", "local_turn_id", "seq", "created_at", "completed_at"
                   FROM "agent_chat_message"
                   WHERE ("session_id" = $1 AND "id" = $2)"#,
            )
            .bind(session.id)
            .bind(target)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|_| server_error())?;
            row.map(message_from_row).transpose()?
        }
        None => None,
    };
    let assistant = active_assistant(&mut tx, &session).await?;
    let plan = chat_kernel::plan_mark_message_dispatch_failed(
        &session,
        explicit.as_ref(),
        assistant.as_ref(),
        message_id,
        detail,
        &mut Utc::now,
    );
    apply_message_updates(&mut tx, &plan.finalize.updates).await?;
    if plan.close_session {
        let closed_at = plan.closed_at.ok_or_else(server_error)?;
        sqlx::query(
            r#"UPDATE "agent_chat_session"
               SET "status" = $1, "active_turn_id" = $2, "active_message_id" = $3,
                   "closed_at" = $4, "error" = $5, "updated_at" = $6
               WHERE "id" = $7"#,
        )
        .bind(AgentChatSessionStatus::Closed.value())
        .bind("")
        .bind(None::<Uuid>)
        .bind(closed_at)
        .bind(&plan.session_error)
        .bind(Utc::now())
        .bind(session.id)
        .execute(&mut *tx)
        .await
        .map_err(|_| server_error())?;
    } else {
        sqlx::query(
            r#"UPDATE "agent_chat_session"
               SET "active_turn_id" = $1, "active_message_id" = $2,
                   "error" = $3, "updated_at" = $4
               WHERE "id" = $5"#,
        )
        .bind("")
        .bind(None::<Uuid>)
        .bind(&plan.session_error)
        .bind(Utc::now())
        .bind(session.id)
        .execute(&mut *tx)
        .await
        .map_err(|_| server_error())?;
    }
    for event in &plan.events {
        append_event(&mut tx, session.id, event, effects).await?;
    }
    if plan.queue_drain {
        effects.push(ChatEffect::DrainTasks {
            runner_id: session.runner_id,
            pod_id: Some(session.pod_id),
        });
    }
    commit_tx(tx).await
}

/// `mark_warm_dispatch_failed` (`chat.py:256-267`) in its own
/// transaction: append `chat_warm_failed` to a still-open session.
/// Errors propagate (same unisolated `on_commit` seat).
pub async fn execute_mark_warm_dispatch_failed(
    pool: &PgPool,
    session_id: Uuid,
    effects: &mut Vec<ChatEffect>,
) -> Result<(), Response> {
    let mut tx = pool.begin().await.map_err(|_| server_error())?;
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(&format!(
        r#"SELECT {SESSION_COLUMNS} FROM "agent_chat_session"
           WHERE ("id" = $1 AND "status" = $2) LIMIT 1 FOR UPDATE"#
    ))
    .bind(session_id)
    .bind(AgentChatSessionStatus::Open.value())
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| server_error())?;
    let Some(row) = row else {
        tx.rollback().await.map_err(|_| server_error())?;
        return Ok(());
    };
    let session = session_from_pg(&row)?;
    // `chat_warm_failed`, `{"message_id": None, "reason":
    // "warm_dispatch_failed"}` (`chat.py:256-267`).
    let inputs = chat_kernel::append_event_inputs(
        "chat_warm_failed",
        Some(&serde_json::json!({
            "message_id": Value::Null,
            "reason": "warm_dispatch_failed",
        })),
        None,
        "",
    );
    append_event(&mut tx, session.id, &inputs, effects).await?;
    commit_tx(tx).await
}

// ---------------------------------------------------------------------------
// Post-commit drain (`transaction.on_commit`, sync order)
// ---------------------------------------------------------------------------

/// Publish one event frame (`publish_event`, `chat.py:148-164`):
/// re-read the row post-commit, serialize, and publish. A vanished
/// row publishes nothing. A Redis failure propagates — the publish
/// sits in `on_commit` unisolated, so the source 500s after commit
/// too. A missing client returns silently (`redis_instance()`
/// returning `None`).
pub async fn publish_event_frame(
    pool: &PgPool,
    ports: &impl RunnerPorts,
    event_id: i64,
) -> Result<(), Response> {
    let row: Option<EventRow> = sqlx::query_as(
        r#"SELECT "id", "session_id", "message_id", "seq", "source_key", "kind",
              "payload", "created_at"
           FROM "agent_chat_event" WHERE "id" = $1"#,
    )
    .bind(event_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| server_error())?;
    let Some(row) = row else {
        return Ok(());
    };
    let event = event_from_row(row)?;
    let created_at = crate::assistant::common::py_iso(&event.created_at);
    let session_id = event.session_id.to_string();
    let message_id = event.message_id.map(|id| id.to_string());
    let payload_json = chat_kernel::dumps_value(&event.payload);
    let frame = chat_kernel::serialize_event_json(&chat_kernel::EventParts {
        id: event.id,
        session_id: session_id.as_str(),
        message_id: message_id.as_deref(),
        seq: event.seq,
        kind: event.kind.as_str(),
        payload_json: payload_json.as_str(),
        created_at: created_at.as_str(),
    });
    let channel = pidash_types::runner_runs::consts::event_channel(&session_id);
    publish_chat_event(ports.redis_client(), &channel, &frame)
        .await
        .map_err(|_| server_error())
}

/// Drain collected [`ChatEffect`]s after commit, in order, with
/// Python's per-site isolation:
/// * `PublishEvent` propagates Redis failures (500 with committed
///   rows — the publish is an unisolated `on_commit`).
/// * `DrainTasks` swallows-and-logs under one guard, skipping the pod
///   drain when the runner drain fails (`_drain`, `chat.py:196-207`).
/// * `SendChatMessage` / `SendChatWarm` run the mark-failed backstop
///   (in its own transaction) on send failure and swallow the
///   original error; backstop errors propagate.
pub async fn drain_chat_effects(
    pool: &PgPool,
    ports: &impl RunnerPorts,
    effects: Vec<ChatEffect>,
) -> Result<(), Response> {
    for effect in effects {
        match effect {
            ChatEffect::PublishEvent { event_id } => {
                publish_event_frame(pool, ports, event_id).await?;
            }
            ChatEffect::DrainTasks { runner_id, pod_id } => {
                if let Err(error) = ports.drain_for_runner_by_id(runner_id).await {
                    tracing::error!(
                        ?error,
                        runner_id = %runner_id,
                        "chat.drain: runner drain failed"
                    );
                } else if let Some(pod_id) = pod_id {
                    if let Err(error) = ports.drain_pod_by_id(pod_id).await {
                        tracing::error!(
                            ?error,
                            pod_id = %pod_id,
                            "chat.drain: pod drain failed"
                        );
                    }
                }
            }
            ChatEffect::SendChatMessage { runner_id, payload } => {
                let body = serde_json::to_value(&payload).map_err(|_| server_error())?;
                if let Err(error) = ports.send_to_runner(runner_id, body).await {
                    // The backstop ids ride the payload frame.
                    let session_id: Uuid = match payload.chat_session_id.parse() {
                        Ok(session_id) => session_id,
                        Err(_) => return Err(server_error()),
                    };
                    let message_id: Uuid = match payload.message_id.parse() {
                        Ok(message_id) => message_id,
                        Err(_) => return Err(server_error()),
                    };
                    let mut backstop = Vec::new();
                    execute_mark_message_dispatch_failed(
                        pool,
                        session_id,
                        Some(message_id),
                        &error.to_string(),
                        &mut backstop,
                    )
                    .await?;
                    Box::pin(drain_chat_effects(pool, ports, backstop)).await?;
                }
            }
            ChatEffect::SendChatWarm { runner_id, payload } => {
                let body = serde_json::to_value(&payload).map_err(|_| server_error())?;
                if let Err(error) = ports.send_to_runner(runner_id, body).await {
                    let session_id: Uuid = match payload.chat_session_id.parse() {
                        Ok(session_id) => session_id,
                        Err(_) => return Err(server_error()),
                    };
                    tracing::error!(
                        ?error,
                        session_id = %session_id,
                        "chat.warm: control dispatch failed; marking warm-dispatch failed"
                    );
                    let mut backstop = Vec::new();
                    execute_mark_warm_dispatch_failed(pool, session_id, &mut backstop).await?;
                    Box::pin(drain_chat_effects(pool, ports, backstop)).await?;
                }
            }
        }
    }
    Ok(())
}

/// The endpoint-local control sends that are not L5 service effects:
/// cancel, close, and decide each send one frame directly from
/// `on_commit` (`chat.py:415-425,493-510,705-720`). Every failure
/// propagates (BUG-offline-fanout-500: the 500 carries the persisted
/// write).
pub enum DirectSend {
    Cancel {
        runner_id: Uuid,
        session_id: Uuid,
        reason: Value,
    },
    Close {
        runner_id: Uuid,
        session_id: Uuid,
        reason: Value,
    },
    Decide {
        runner_id: Uuid,
        session_id: Uuid,
        approval_id: Uuid,
        local_approval_id: String,
        decision: String,
        decided_by: Uuid,
    },
}

impl DirectSend {
    fn frame(&self) -> Value {
        match self {
            DirectSend::Cancel {
                session_id, reason, ..
            } => serde_json::json!({
                "type": "chat_cancel",
                "chat_session_id": session_id.to_string(),
                "reason": reason,
            }),
            DirectSend::Close {
                session_id, reason, ..
            } => serde_json::json!({
                "type": "chat_close",
                "chat_session_id": session_id.to_string(),
                "reason": reason,
            }),
            DirectSend::Decide {
                session_id,
                approval_id,
                local_approval_id,
                decision,
                decided_by,
                ..
            } => serde_json::json!({
                "type": "chat_decide",
                "chat_session_id": session_id.to_string(),
                "approval_id": approval_id.to_string(),
                "local_approval_id": local_approval_id,
                "decision": decision,
                "decided_by": decided_by.to_string(),
            }),
        }
    }

    fn runner_id(&self) -> Uuid {
        match self {
            DirectSend::Cancel { runner_id, .. }
            | DirectSend::Close { runner_id, .. }
            | DirectSend::Decide { runner_id, .. } => *runner_id,
        }
    }
}

/// Drain [`DirectSend`]s after commit, in order, propagating every
/// failure to the 500 the source's `on_commit` raises.
pub async fn drain_direct_sends(
    ports: &impl RunnerPorts,
    sends: Vec<DirectSend>,
) -> Result<(), Response> {
    for send in sends {
        ports
            .send_to_runner(send.runner_id(), send.frame())
            .await
            .map_err(|_| server_error())?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Web handlers (8 chat endpoints)
// ---------------------------------------------------------------------------

/// Authenticate a web caller: the pool plus the user id, 401
/// `NotAuthenticated` when anonymous, 500 on any resolver failure
/// (the `app_notifications::actor` precedent).
async fn web_actor(
    state: &AppState,
    extension: Option<Extension<SessionHandle>>,
) -> Result<(PgPool, Uuid), Response> {
    let pool = pool_of(state)?.clone();
    let secret = state.settings().secret_key.clone();
    let actor = crate::license::resolve_actor(&pool, secret.as_bytes(), extension)
        .await
        .map_err(|_| server_error())?;
    let Some(actor) = actor else {
        return Err(json_response(
            StatusCode::UNAUTHORIZED,
            crate::license::UNAUTHENTICATED_BODY.to_owned(),
        ));
    };
    Ok((pool, actor.id))
}

fn chat_not_found() -> Response {
    json_response(StatusCode::NOT_FOUND, CHAT_NOT_FOUND_BODY.to_owned())
}

fn workspace_forbidden() -> Response {
    json_response(StatusCode::FORBIDDEN, r#"{"error":"forbidden"}"#.to_owned())
}

/// `GET /api/runners/chat/sessions/` (`chat.py:140-189`): the
/// workspace/runner/project filters, the visibility predicate, the
/// admin rule, the 100 cap. Query params parse as UUIDs or 500
/// (Django's `ValidationError` on filter).
pub async fn chat_sessions_list(
    State(state): State<AppState>,
    extension: Option<Extension<SessionHandle>>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Response {
    let (pool, user_id) = match web_actor(&state, extension).await {
        Ok(preamble) => preamble,
        Err(response) => return response,
    };
    let workspace_id: Option<Uuid> = match params.get("workspace") {
        Some(raw) => {
            let workspace_id: Uuid = match raw.parse() {
                Ok(workspace_id) => workspace_id,
                Err(_) => return server_error(),
            };
            let role = match workspace_role(&pool, workspace_id, user_id).await {
                Ok(role) => role,
                Err(response) => return response,
            };
            if role.is_none() {
                return workspace_forbidden();
            }
            Some(workspace_id)
        }
        None => None,
    };
    let mut force_empty = false;
    let runner_id: Option<Uuid> = match params.get("runner") {
        Some(raw) => {
            let runner_id: Uuid = match raw.parse() {
                Ok(runner_id) => runner_id,
                Err(_) => return server_error(),
            };
            let runner: Option<RunnerGuardRow> = match sqlx::query_as(
                r#"SELECT "id", "owner_id", "workspace_id", "visibility", "status"
                   FROM "runner" WHERE "id" = $1"#,
            )
            .bind(runner_id)
            .fetch_optional(&pool)
            .await
            {
                Ok(runner) => runner,
                Err(_) => return server_error(),
            };
            match runner {
                None => {
                    force_empty = true;
                }
                Some((_, owner_id, runner_workspace, visibility, _)) => {
                    if let Some(workspace_id) = workspace_id {
                        if runner_workspace != workspace_id {
                            force_empty = true;
                        }
                    } else {
                        let role = match workspace_role(&pool, runner_workspace, user_id).await {
                            Ok(role) => role,
                            Err(response) => return response,
                        };
                        if role.is_none() {
                            return workspace_forbidden();
                        }
                        if visibility != pidash_auth::permissions::runner::VISIBILITY_PRIVATE as i16
                            || owner_id != user_id
                        {
                            force_empty = true;
                        }
                    }
                }
            }
            Some(runner_id)
        }
        None => None,
    };
    let project_id: Option<Uuid> = match params.get("project") {
        Some(raw) => match raw.parse() {
            Ok(project_id) => Some(project_id),
            Err(_) => return server_error(),
        },
        None => None,
    };
    if force_empty {
        return json_response(StatusCode::OK, "[]".to_owned());
    }
    // The creator rule: a scoped admin sees the whole workspace, and
    // unscoped callers see only their own rows.
    let admin = match workspace_id {
        Some(workspace_id) => {
            let role = match workspace_role(&pool, workspace_id, user_id).await {
                Ok(role) => role,
                Err(response) => return response,
            };
            role.is_some_and(|role| role >= ROLE_ADMIN)
        }
        None => false,
    };
    let only_own = workspace_id.is_none() || !admin;
    // Sessions with the runner joined for the visibility predicate
    // (`runner_visible_to_user_q`, `guards.py:67-82`); the project
    // filter joins the session pod. Default ordering
    // `-last_message_at, -created_at`, capped at 100.
    let mut sql = format!(
        r#"SELECT {columns} FROM "agent_chat_session"
           INNER JOIN "runner" ON ("agent_chat_session"."runner_id" = "runner"."id")"#,
        columns = qualified_session_columns(),
    );
    if project_id.is_some() {
        sql.push_str(r#" INNER JOIN "pod" ON ("agent_chat_session"."pod_id" = "pod"."id")"#);
    }
    sql.push_str(r#" WHERE "runner"."owner_id" = $1 AND "runner"."visibility" = $2"#);
    let mut position = 3;
    if workspace_id.is_some() {
        sql.push_str(&format!(
            r#" AND "agent_chat_session"."workspace_id" = ${position}"#
        ));
        position += 1;
    }
    if runner_id.is_some() {
        sql.push_str(&format!(
            r#" AND "agent_chat_session"."runner_id" = ${position}"#
        ));
        position += 1;
    }
    if project_id.is_some() {
        sql.push_str(&format!(r#" AND "pod"."project_id" = ${position}"#));
        position += 1;
    }
    if only_own {
        sql.push_str(&format!(
            r#" AND "agent_chat_session"."created_by_id" = ${position}"#
        ));
    }
    sql.push_str(
        r#" ORDER BY "agent_chat_session"."last_message_at" DESC,
                    "agent_chat_session"."created_at" DESC LIMIT 100"#,
    );
    let mut query = sqlx::query(&sql)
        .bind(user_id)
        .bind(pidash_auth::permissions::runner::VISIBILITY_PRIVATE as i16);
    if let Some(workspace_id) = workspace_id {
        query = query.bind(workspace_id);
    }
    if let Some(runner_id) = runner_id {
        query = query.bind(runner_id);
    }
    if let Some(project_id) = project_id {
        query = query.bind(project_id);
    }
    if only_own {
        query = query.bind(user_id);
    }
    let rows: Vec<sqlx::postgres::PgRow> = match query.fetch_all(&pool).await {
        Ok(rows) => rows,
        Err(_) => return server_error(),
    };
    let mut rendered = Vec::with_capacity(rows.len());
    for row in rows {
        let session = match session_from_pg(&row) {
            Ok(session) => session,
            Err(response) => return response,
        };
        let detail = match fetch_runner_detail(&pool, session.runner_id).await {
            Ok(detail) => detail,
            Err(response) => return response,
        };
        rendered.push(render_session(&session, &detail));
    }
    json_response(StatusCode::OK, format!("[{}]", rendered.join(",")))
}

/// `POST /api/runners/chat/sessions/` (`chat.py:192-237`): scope to a
/// live membership, lock the runner, reuse an empty open session
/// (200) or create one (201).
pub async fn chat_session_create(
    State(state): State<AppState>,
    extension: Option<Extension<SessionHandle>>,
    req: Request<axum::body::Body>,
) -> Response {
    let (pool, user_id) = match web_actor(&state, extension).await {
        Ok(preamble) => preamble,
        Err(response) => return response,
    };
    let data = match read_request_data(&state, req).await {
        Ok(data) => data,
        Err(response) => return response,
    };
    // A non-dict body is the source's `AttributeError` on `.get`
    // (500).
    let Some(obj) = data.as_object() else {
        return server_error();
    };
    let workspace_raw = obj.get("workspace").filter(|value| py_truthy(value));
    let runner_raw = obj.get("runner").filter(|value| py_truthy(value));
    let (Some(workspace_raw), Some(runner_raw)) = (workspace_raw, runner_raw) else {
        return json_response(
            StatusCode::BAD_REQUEST,
            r#"{"error":"workspace and runner are required"}"#.to_owned(),
        );
    };
    // Truthy values parse as UUIDs; anything else is the source's
    // `ValidationError` (500).
    let workspace_id: Uuid = match workspace_raw.as_str().and_then(|raw| raw.parse().ok()) {
        Some(workspace_id) => workspace_id,
        None => return server_error(),
    };
    let runner_id: Uuid = match runner_raw.as_str().and_then(|raw| raw.parse().ok()) {
        Some(runner_id) => runner_id,
        None => return server_error(),
    };
    let role = match workspace_role(&pool, workspace_id, user_id).await {
        Ok(role) => role,
        Err(response) => return response,
    };
    if role.is_none() {
        return workspace_forbidden();
    }
    let mut tx = match pool.begin().await {
        Ok(tx) => tx,
        Err(_) => return server_error(),
    };
    let runner: Option<(Uuid, Uuid, i16, String, Uuid)> = match sqlx::query_as(
        r#"SELECT "id", "owner_id", "visibility", "status", "pod_id" FROM "runner"
           WHERE ("id" = $1 AND "workspace_id" = $2) LIMIT 1 FOR UPDATE"#,
    )
    .bind(runner_id)
    .bind(workspace_id)
    .fetch_optional(&mut *tx)
    .await
    {
        Ok(runner) => runner,
        Err(_) => return server_error(),
    };
    let not_found = || {
        json_response(
            StatusCode::NOT_FOUND,
            r#"{"error":"runner_not_found"}"#.to_owned(),
        )
    };
    let Some((_, owner_id, visibility, status, pod_id)) = runner else {
        return not_found();
    };
    if visibility != pidash_auth::permissions::runner::VISIBILITY_PRIVATE as i16
        || owner_id != user_id
    {
        return not_found();
    }
    if runner_unavailable(&status) {
        return json_response(
            StatusCode::CONFLICT,
            r#"{"error":"runner_unavailable"}"#.to_owned(),
        );
    }
    // Empty-session reuse: the newest open session by this user on
    // this runner with no messages at all.
    let existing: Option<sqlx::postgres::PgRow> = match sqlx::query(&format!(
        r#"SELECT {columns} FROM "agent_chat_session"
           WHERE ("created_by_id" = $1 AND "runner_id" = $2 AND "status" = $3
             AND NOT EXISTS (SELECT 1 FROM "agent_chat_message"
               WHERE "agent_chat_message"."session_id" = "agent_chat_session"."id"))
           ORDER BY "created_at" DESC LIMIT 1"#,
        columns = qualified_session_columns(),
    ))
    .bind(user_id)
    .bind(runner_id)
    .bind(AgentChatSessionStatus::Open.value())
    .fetch_optional(&mut *tx)
    .await
    {
        Ok(existing) => existing,
        Err(_) => return server_error(),
    };
    if let Some(existing) = existing {
        let session = match session_from_pg(&existing) {
            Ok(session) => session,
            Err(response) => return response,
        };
        if commit_tx(tx).await.is_err() {
            return server_error();
        }
        let detail = match fetch_runner_detail(&pool, session.runner_id).await {
            Ok(detail) => detail,
            Err(response) => return response,
        };
        return json_response(StatusCode::OK, render_session(&session, &detail));
    }
    let model = match frame_text(obj.get("model").unwrap_or(&Value::Null), 128) {
        Ok(model) => model,
        Err(response) => return response,
    };
    let id = Uuid::new_v4();
    let now = Utc::now();
    if sqlx::query(
        r#"INSERT INTO "agent_chat_session"
           ("id", "workspace_id", "runner_id", "created_by_id", "pod_id", "status",
            "agent_kind", "local_thread_id", "local_session_id", "cwd", "model",
            "active_turn_id", "active_message_id", "close_requested",
            "last_message_at", "closed_at", "error", "created_at", "updated_at")
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14,
                   $15, $16, $17, $18, $19)"#,
    )
    .bind(id)
    .bind(workspace_id)
    .bind(runner_id)
    .bind(user_id)
    .bind(pod_id)
    .bind(AgentChatSessionStatus::Open.value())
    .bind("")
    .bind("")
    .bind("")
    .bind("")
    .bind(&model)
    .bind("")
    .bind(None::<Uuid>)
    .bind(false)
    .bind(None::<DateTime<Utc>>)
    .bind(None::<DateTime<Utc>>)
    .bind("")
    .bind(now)
    .bind(now)
    .execute(&mut *tx)
    .await
    .is_err()
    {
        return server_error();
    }
    if commit_tx(tx).await.is_err() {
        return server_error();
    }
    let detail = match fetch_runner_detail(&pool, runner_id).await {
        Ok(detail) => detail,
        Err(response) => return response,
    };
    let session = AgentChatSession {
        id,
        workspace_id,
        runner_id,
        created_by_id: user_id,
        pod_id,
        status: AgentChatSessionStatus::Open,
        agent_kind: String::new(),
        local_thread_id: String::new(),
        local_session_id: String::new(),
        cwd: String::new(),
        model,
        active_turn_id: String::new(),
        active_message_id: None,
        close_requested: false,
        last_message_at: None,
        closed_at: None,
        error: String::new(),
        created_at: now,
        updated_at: now,
    };
    json_response(StatusCode::CREATED, render_session(&session, &detail))
}

/// `GET /api/runners/chat/sessions/<id>/` (`chat.py:240-249`):
/// the session or 404 (missing and forbidden share the body).
pub async fn chat_session_detail(
    State(state): State<AppState>,
    extension: Option<Extension<SessionHandle>>,
    Path(session_id): Path<String>,
) -> Response {
    let (pool, user_id) = match web_actor(&state, extension).await {
        Ok(preamble) => preamble,
        Err(response) => return response,
    };
    let session_id: Uuid = match session_id.parse() {
        Ok(session_id) => session_id,
        Err(_) => return chat_not_found(),
    };
    let fetched = match fetch_session_with_runner(&pool, session_id).await {
        Ok(fetched) => fetched,
        Err(response) => return response,
    };
    let Some((session, runner)) = fetched else {
        return chat_not_found();
    };
    if !can_read_session(&pool, user_id, &session, &runner).await {
        return chat_not_found();
    }
    let detail = match fetch_runner_detail(&pool, session.runner_id).await {
        Ok(detail) => detail,
        Err(response) => return response,
    };
    json_response(StatusCode::OK, render_session(&session, &detail))
}

/// `can_read_chat` over live reads: owner, member, or admin
/// (`chat.py:26-29` + `guards.py`). Shared with the SSE stream.
pub(crate) async fn can_read_session(
    pool: &PgPool,
    user_id: Uuid,
    session: &AgentChatSession,
    runner: &RunnerGuard,
) -> bool {
    // No creator shortcut: the kernel requires membership and runner
    // visibility even for the creator.
    let role = workspace_role(pool, session.workspace_id, user_id)
        .await
        .unwrap_or(None);
    let member = role.is_some();
    let admin = role.is_some_and(|role| role >= ROLE_ADMIN);
    let visible = runner_visible_to_user(runner, user_id);
    chat_kernel::can_read_chat(
        user_id,
        session.created_by_id,
        || member,
        || visible,
        || admin,
    )
}

/// `can_send_chat` over live reads (`chat.py:32-35`).
async fn can_send_session(
    pool: &PgPool,
    user_id: Uuid,
    session: &AgentChatSession,
    runner: &RunnerGuard,
) -> bool {
    // Member and creator-with-use (`chat.py:56-59`): admins cannot
    // send on sessions they did not create.
    let role = workspace_role(pool, session.workspace_id, user_id)
        .await
        .unwrap_or(None);
    let member = role.is_some();
    let visible = runner_visible_to_user(runner, user_id);
    chat_kernel::can_send_chat(user_id, session.created_by_id, || member, || visible)
}

/// `POST /api/runners/chat/sessions/<id>/warm/` (`chat.py:252-288`):
/// lock the session, then the status check, then the runner check,
/// then the active-turn skip — in that order — else enqueue warm
/// (202).
pub async fn chat_session_warm(
    State(state): State<AppState>,
    extension: Option<Extension<SessionHandle>>,
    Path(session_id): Path<String>,
) -> Response {
    let (pool, user_id) = match web_actor(&state, extension).await {
        Ok(preamble) => preamble,
        Err(response) => return response,
    };
    let session_id: Uuid = match session_id.parse() {
        Ok(session_id) => session_id,
        Err(_) => return chat_not_found(),
    };
    let mut tx = match pool.begin().await {
        Ok(tx) => tx,
        Err(_) => return server_error(),
    };
    let locked = match lock_chat_session(&mut tx, session_id).await {
        Ok(locked) => locked,
        Err(response) => return response,
    };
    let Some(locked) = locked else {
        return chat_not_found();
    };
    if !can_send_session(&pool, user_id, &locked.session, &locked.runner).await {
        return chat_not_found();
    }
    let runner_status: Option<String> = match sqlx::query_scalar(
        r#"SELECT "status" FROM "runner" WHERE "id" = $1 LIMIT 1 FOR UPDATE"#,
    )
    .bind(locked.session.runner_id)
    .fetch_optional(&mut *tx)
    .await
    {
        Ok(runner_status) => runner_status,
        Err(_) => return server_error(),
    };
    // The status check runs before the runner check, exactly as in
    // the source.
    if locked.session.status != AgentChatSessionStatus::Open {
        return json_response(
            StatusCode::CONFLICT,
            r#"{"error":"chat_session_closed"}"#.to_owned(),
        );
    }
    match runner_status {
        Some(status) if !runner_unavailable(&status) => {}
        _ => {
            return json_response(
                StatusCode::CONFLICT,
                r#"{"error":"runner_unavailable"}"#.to_owned(),
            );
        }
    }
    if chat_kernel::turn_was_active(&locked.session) {
        if commit_tx(tx).await.is_err() {
            return server_error();
        }
        return json_response(
            StatusCode::OK,
            r#"{"ok":true,"skipped":"chat_turn_active"}"#.to_owned(),
        );
    }
    let mut effects = Vec::new();
    chat_kernel::enqueue_chat_warm_after_commit(
        locked.session.runner_id,
        chat_kernel::EnqueueWarmParams {
            chat_session_id: locked.session.id,
            local_thread_id: locked.session.local_thread_id.clone(),
            local_session_id: locked.session.local_session_id.clone(),
            cwd: locked.session.cwd.clone(),
            model: locked.session.model.clone(),
        },
        &mut |effect| effects.push(effect),
    );
    if commit_tx(tx).await.is_err() {
        return server_error();
    }
    if !effects.is_empty() {
        let ports = LivePorts::new(pool.clone(), &state);
        if drain_chat_effects(&pool, &ports, effects).await.is_err() {
            return server_error();
        }
    }
    json_response(StatusCode::ACCEPTED, r#"{"ok":true}"#.to_owned())
}

/// `GET /api/runners/chat/sessions/<id>/messages/` (`chat.py:291-299`):
/// the session's messages in `seq` order, or 404.
pub async fn chat_messages_list(
    State(state): State<AppState>,
    extension: Option<Extension<SessionHandle>>,
    Path(session_id): Path<String>,
) -> Response {
    let (pool, user_id) = match web_actor(&state, extension).await {
        Ok(preamble) => preamble,
        Err(response) => return response,
    };
    let session_id: Uuid = match session_id.parse() {
        Ok(session_id) => session_id,
        Err(_) => return chat_not_found(),
    };
    let fetched = match fetch_session_with_runner(&pool, session_id).await {
        Ok(fetched) => fetched,
        Err(response) => return response,
    };
    let Some((session, runner)) = fetched else {
        return chat_not_found();
    };
    if !can_read_session(&pool, user_id, &session, &runner).await {
        return chat_not_found();
    }
    let rows: Vec<MessageRow> = match sqlx::query_as(
        r#"SELECT "id", "session_id", "role", "content", "content_parts", "status",
              "local_item_id", "local_turn_id", "seq", "created_at", "completed_at"
           FROM "agent_chat_message" WHERE "session_id" = $1 ORDER BY "seq" ASC"#,
    )
    .bind(session_id)
    .fetch_all(&pool)
    .await
    {
        Ok(rows) => rows,
        Err(_) => return server_error(),
    };
    let mut rendered = Vec::with_capacity(rows.len());
    for row in rows {
        let message = match message_from_row(row) {
            Ok(message) => message,
            Err(response) => return response,
        };
        rendered.push(render_message(&message));
    }
    json_response(StatusCode::OK, format!("[{}]", rendered.join(",")))
}

/// `POST /api/runners/chat/sessions/<id>/messages/` (`chat.py:302-377`):
/// throttle first (DRF order — even invalid bodies consume quota),
/// then validate, lock, create the user message, stamp the session,
/// append the timing event, and enqueue (201).
pub async fn chat_message_create(
    State(state): State<AppState>,
    extension: Option<Extension<SessionHandle>>,
    Path(session_id): Path<String>,
    req: Request<axum::body::Body>,
) -> Response {
    let (pool, user_id) = match web_actor(&state, extension).await {
        Ok(preamble) => preamble,
        Err(response) => return response,
    };
    let session_id: Uuid = match session_id.parse() {
        Ok(session_id) => session_id,
        Err(_) => return chat_not_found(),
    };
    // The brake runs before parsing and validation, exactly as
    // `check_throttles` does in `initial()`.
    if let ChatSendVerdict::Deny { retry_after_secs } =
        check_chat_send_throttle(&state, &user_id).await
    {
        return throttle_denied(&retry_after_secs);
    }
    let data = match read_request_data(&state, req).await {
        Ok(data) => data,
        Err(response) => return response,
    };
    let Some(obj) = data.as_object() else {
        return server_error();
    };
    let content_raw = obj.get("content").filter(|value| py_truthy(value));
    let parts_raw = obj.get("content_parts").filter(|value| py_truthy(value));
    if content_raw.is_none() && parts_raw.is_none() {
        return json_response(
            StatusCode::BAD_REQUEST,
            r#"{"error":"content is required"}"#.to_owned(),
        );
    }
    let content_value = content_raw.cloned().unwrap_or(Value::String(String::new()));
    let parts_value = parts_raw.cloned().unwrap_or(Value::Array(Vec::new()));
    if payload_too_large(&serde_json::json!({
        "content": content_value,
        "content_parts": parts_value,
    })) {
        return json_response(
            StatusCode::PAYLOAD_TOO_LARGE,
            r#"{"error":"payload_too_large"}"#.to_owned(),
        );
    }
    let mut tx = match pool.begin().await {
        Ok(tx) => tx,
        Err(_) => return server_error(),
    };
    let locked = match lock_chat_session(&mut tx, session_id).await {
        Ok(locked) => locked,
        Err(response) => return response,
    };
    let Some(locked) = locked else {
        return chat_not_found();
    };
    if !can_send_session(&pool, user_id, &locked.session, &locked.runner).await {
        return chat_not_found();
    }
    let runner_status: Option<String> = match sqlx::query_scalar(
        r#"SELECT "status" FROM "runner" WHERE "id" = $1 LIMIT 1 FOR UPDATE"#,
    )
    .bind(locked.session.runner_id)
    .fetch_optional(&mut *tx)
    .await
    {
        Ok(runner_status) => runner_status,
        Err(_) => return server_error(),
    };
    if locked.session.status != AgentChatSessionStatus::Open {
        return json_response(
            StatusCode::CONFLICT,
            r#"{"error":"chat_session_closed"}"#.to_owned(),
        );
    }
    match runner_status {
        Some(status) if !runner_unavailable(&status) => {}
        _ => {
            return json_response(
                StatusCode::CONFLICT,
                r#"{"error":"runner_unavailable"}"#.to_owned(),
            );
        }
    }
    if chat_kernel::turn_was_active(&locked.session) {
        return json_response(
            StatusCode::CONFLICT,
            r#"{"error":"chat_turn_active"}"#.to_owned(),
        );
    }
    // `content` stores `str()` (a `TextField`); `content_parts` stores
    // verbatim JSON.
    let content = match content_raw {
        Some(Value::String(content)) => content.clone(),
        Some(other) => super::run_endpoints::py_str_value(other),
        None => String::new(),
    };
    let seq = match next_message_seq(&mut tx, session_id).await {
        Ok(seq) => seq,
        Err(response) => return response,
    };
    let message_id = Uuid::new_v4();
    let message_created_at = Utc::now();
    if sqlx::query(
        r#"INSERT INTO "agent_chat_message"
           ("id", "session_id", "role", "content", "content_parts", "status",
            "local_item_id", "local_turn_id", "seq", "created_at", "completed_at")
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)"#,
    )
    .bind(message_id)
    .bind(session_id)
    .bind(AgentChatMessageRole::User.value())
    .bind(&content)
    .bind(&parts_value)
    .bind(AgentChatMessageStatus::Queued.value())
    .bind("")
    .bind("")
    .bind(seq)
    .bind(message_created_at)
    .bind(None::<DateTime<Utc>>)
    .execute(&mut *tx)
    .await
    .is_err()
    {
        return server_error();
    }
    let stamped_at = Utc::now();
    if sqlx::query(
        r#"UPDATE "agent_chat_session"
           SET "active_message_id" = $1, "last_message_at" = $2, "updated_at" = $3
           WHERE "id" = $4"#,
    )
    .bind(message_id)
    .bind(stamped_at)
    .bind(Utc::now())
    .bind(session_id)
    .execute(&mut *tx)
    .await
    .is_err()
    {
        return server_error();
    }
    let mut effects = Vec::new();
    // `chat_timing`, `{"stage": ..., "message_id": ..., "recorded_at":
    // ...}` (`chat.py:361-369`), linked to the new message.
    let timing = chat_kernel::append_event_inputs(
        "chat_timing",
        Some(&serde_json::json!({
            "stage": "web_send_accepted",
            "message_id": message_id.to_string(),
            "recorded_at": crate::assistant::common::py_iso(&Utc::now()),
        })),
        Some(message_id),
        "",
    );
    if append_event(&mut tx, session_id, &timing, &mut effects)
        .await
        .is_err()
    {
        return server_error();
    }
    let message = AgentChatMessage {
        id: message_id,
        session_id,
        role: AgentChatMessageRole::User,
        content: content.clone(),
        content_parts: parts_value.clone(),
        status: AgentChatMessageStatus::Queued,
        local_item_id: String::new(),
        local_turn_id: String::new(),
        seq,
        created_at: message_created_at,
        completed_at: None,
    };
    chat_kernel::enqueue_chat_message_after_commit(
        locked.session.runner_id,
        chat_kernel::EnqueueMessageParams {
            chat_session_id: locked.session.id,
            message_id,
            content: content.clone(),
            content_parts: parts_value.clone(),
            local_thread_id: locked.session.local_thread_id.clone(),
            local_session_id: locked.session.local_session_id.clone(),
            cwd: locked.session.cwd.clone(),
            model: locked.session.model.clone(),
        },
        &mut |effect| effects.push(effect),
    );
    if commit_tx(tx).await.is_err() {
        return server_error();
    }
    if !effects.is_empty() {
        let ports = LivePorts::new(pool.clone(), &state);
        if drain_chat_effects(&pool, &ports, effects).await.is_err() {
            return server_error();
        }
    }
    json_response(StatusCode::CREATED, render_message(&message))
}

/// `POST /api/runners/chat/sessions/<id>/cancel/` (`chat.py:380-397`):
/// an inactive session noops (without touching the body); an active
/// one sends `chat_cancel` after commit (unisolated — 500 on
/// offline, with nothing persisted either way).
pub async fn chat_session_cancel(
    State(state): State<AppState>,
    extension: Option<Extension<SessionHandle>>,
    Path(session_id): Path<String>,
    req: Request<axum::body::Body>,
) -> Response {
    let (pool, user_id) = match web_actor(&state, extension).await {
        Ok(preamble) => preamble,
        Err(response) => return response,
    };
    let session_id: Uuid = match session_id.parse() {
        Ok(session_id) => session_id,
        Err(_) => return chat_not_found(),
    };
    let data = match read_request_data(&state, req).await {
        Ok(data) => data,
        Err(response) => return response,
    };
    let mut tx = match pool.begin().await {
        Ok(tx) => tx,
        Err(_) => return server_error(),
    };
    let locked = match lock_chat_session(&mut tx, session_id).await {
        Ok(locked) => locked,
        Err(response) => return response,
    };
    let Some(locked) = locked else {
        return chat_not_found();
    };
    if !can_send_session(&pool, user_id, &locked.session, &locked.runner).await {
        return chat_not_found();
    }
    if !chat_kernel::turn_was_active(&locked.session) {
        if commit_tx(tx).await.is_err() {
            return server_error();
        }
        return json_response(StatusCode::OK, r#"{"ok":true,"noop":true}"#.to_owned());
    }
    // The reason reads here, after the noop check — and verbatim
    // (the frame carries JSON, never `str()`).
    let reason = match data.as_object() {
        Some(obj) => obj
            .get("reason")
            .filter(|value| py_truthy(value))
            .cloned()
            .unwrap_or(Value::String("user_cancelled".to_owned())),
        None => return server_error(),
    };
    if commit_tx(tx).await.is_err() {
        return server_error();
    }
    let ports = LivePorts::new(pool.clone(), &state);
    if drain_direct_sends(
        &ports,
        vec![DirectSend::Cancel {
            runner_id: locked.session.runner_id,
            session_id,
            reason,
        }],
    )
    .await
    .is_err()
    {
        return server_error();
    }
    json_response(StatusCode::OK, r#"{"ok":true}"#.to_owned())
}

/// `POST /api/runners/chat/sessions/<id>/close/` (`chat.py:400-425`):
/// an active session marks `close_requested` and sends
/// `chat_cancel` (the runner closes on turn end); an idle one closes
/// now with `chat_closed` and `chat_close`. Both sends are
/// unisolated (BUG-offline-fanout-500).
pub async fn chat_session_close(
    State(state): State<AppState>,
    extension: Option<Extension<SessionHandle>>,
    Path(session_id): Path<String>,
    req: Request<axum::body::Body>,
) -> Response {
    let (pool, user_id) = match web_actor(&state, extension).await {
        Ok(preamble) => preamble,
        Err(response) => return response,
    };
    let session_id: Uuid = match session_id.parse() {
        Ok(session_id) => session_id,
        Err(_) => return chat_not_found(),
    };
    let data = match read_request_data(&state, req).await {
        Ok(data) => data,
        Err(response) => return response,
    };
    let mut tx = match pool.begin().await {
        Ok(tx) => tx,
        Err(_) => return server_error(),
    };
    let locked = match lock_chat_session(&mut tx, session_id).await {
        Ok(locked) => locked,
        Err(response) => return response,
    };
    let Some(locked) = locked else {
        return chat_not_found();
    };
    if !can_send_session(&pool, user_id, &locked.session, &locked.runner).await {
        return chat_not_found();
    }
    if chat_kernel::turn_was_active(&locked.session) {
        // Active: flag only — never touches the body, so a garbage
        // body still closes-requests.
        if sqlx::query(
            r#"UPDATE "agent_chat_session" SET "close_requested" = $1, "updated_at" = $2
               WHERE "id" = $3"#,
        )
        .bind(true)
        .bind(Utc::now())
        .bind(session_id)
        .execute(&mut *tx)
        .await
        .is_err()
        {
            return server_error();
        }
        if commit_tx(tx).await.is_err() {
            return server_error();
        }
        let ports = LivePorts::new(pool.clone(), &state);
        if drain_direct_sends(
            &ports,
            vec![DirectSend::Cancel {
                runner_id: locked.session.runner_id,
                session_id,
                reason: Value::String("close_requested".to_owned()),
            }],
        )
        .await
        .is_err()
        {
            return server_error();
        }
        let mut session = locked.session;
        session.close_requested = true;
        let detail = match fetch_runner_detail(&pool, session.runner_id).await {
            Ok(detail) => detail,
            Err(response) => return response,
        };
        return json_response(StatusCode::OK, render_session(&session, &detail));
    }
    let reason = match data.as_object() {
        Some(obj) => obj
            .get("reason")
            .filter(|value| py_truthy(value))
            .cloned()
            .unwrap_or(Value::String("user_closed".to_owned())),
        None => return server_error(),
    };
    let closed_at = Utc::now();
    if sqlx::query(
        r#"UPDATE "agent_chat_session" SET "status" = $1, "closed_at" = $2, "updated_at" = $3
           WHERE "id" = $4"#,
    )
    .bind(AgentChatSessionStatus::Closed.value())
    .bind(closed_at)
    .bind(Utc::now())
    .bind(session_id)
    .execute(&mut *tx)
    .await
    .is_err()
    {
        return server_error();
    }
    let mut effects = Vec::new();
    let inputs = chat_kernel::append_event_inputs(
        "chat_closed",
        Some(&serde_json::json!({"reason": reason})),
        None,
        "",
    );
    if append_event(&mut tx, session_id, &inputs, &mut effects)
        .await
        .is_err()
    {
        return server_error();
    }
    if commit_tx(tx).await.is_err() {
        return server_error();
    }
    let ports = LivePorts::new(pool.clone(), &state);
    // The event publish registered first, the control send second.
    if drain_chat_effects(&pool, &ports, effects).await.is_err() {
        return server_error();
    }
    if drain_direct_sends(
        &ports,
        vec![DirectSend::Close {
            runner_id: locked.session.runner_id,
            session_id,
            reason,
        }],
    )
    .await
    .is_err()
    {
        return server_error();
    }
    let mut session = locked.session;
    session.status = AgentChatSessionStatus::Closed;
    session.closed_at = Some(closed_at);
    let detail = match fetch_runner_detail(&pool, session.runner_id).await {
        Ok(detail) => detail,
        Err(response) => return response,
    };
    json_response(StatusCode::OK, render_session(&session, &detail))
}

/// `GET /api/runners/chat/approvals/` (`chat.py:428-460`): pending
/// chat approvals — scoped to a workspace (members see their own,
/// admins see all) or unscoped (own sessions plus sessions whose
/// workspace the caller admins). Capped at 200, newest first.
pub async fn chat_approvals_list(
    State(state): State<AppState>,
    extension: Option<Extension<SessionHandle>>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Response {
    let (pool, user_id) = match web_actor(&state, extension).await {
        Ok(preamble) => preamble,
        Err(response) => return response,
    };
    let workspace_id: Option<Uuid> = match params.get("workspace") {
        Some(raw) => {
            let workspace_id: Uuid = match raw.parse() {
                Ok(workspace_id) => workspace_id,
                Err(_) => return server_error(),
            };
            let role = match workspace_role(&pool, workspace_id, user_id).await {
                Ok(role) => role,
                Err(response) => return response,
            };
            if role.is_none() {
                return workspace_forbidden();
            }
            Some(workspace_id)
        }
        None => None,
    };
    let admin = match workspace_id {
        Some(workspace_id) => {
            let role = match workspace_role(&pool, workspace_id, user_id).await {
                Ok(role) => role,
                Err(response) => return response,
            };
            role.is_some_and(|role| role >= ROLE_ADMIN)
        }
        None => false,
    };
    const APPROVAL_COLUMNS: &str = r#""agent_chat_approval"."id", "agent_chat_approval"."session_id",
        "agent_chat_approval"."local_approval_id", "agent_chat_approval"."kind",
        "agent_chat_approval"."payload", "agent_chat_approval"."reason",
        "agent_chat_approval"."status", "agent_chat_approval"."decision_source",
        "agent_chat_approval"."decided_by_id", "agent_chat_approval"."requested_at",
        "agent_chat_approval"."expires_at", "agent_chat_approval"."decided_at""#;
    let mut sql = format!(
        r#"SELECT {APPROVAL_COLUMNS} FROM "agent_chat_approval"
           INNER JOIN "agent_chat_session"
             ON ("agent_chat_approval"."session_id" = "agent_chat_session"."id")
           INNER JOIN "runner"
             ON ("agent_chat_session"."runner_id" = "runner"."id")
           WHERE "runner"."owner_id" = $1 AND "runner"."visibility" = $2"#,
    );
    let mut position = 3;
    if let Some(workspace_id) = workspace_id {
        sql.push_str(&format!(
            r#" AND "agent_chat_session"."workspace_id" = ${position}"#,
        ));
        position += 1;
        if !admin {
            sql.push_str(&format!(
                r#" AND "agent_chat_session"."created_by_id" = ${position}"#,
            ));
            position += 1;
        }
        let _ = workspace_id;
    } else {
        sql.push_str(&format!(
            r#" AND ("agent_chat_session"."created_by_id" = ${position} OR EXISTS (
                   SELECT 1 FROM "workspace_members"
                   WHERE ("workspace_members"."workspace_id"
                            = "agent_chat_session"."workspace_id"
                     AND "workspace_members"."member_id" = ${next}
                     AND "workspace_members"."role" >= {ROLE_ADMIN}
                     AND "workspace_members"."is_active"
                     AND "workspace_members"."deleted_at" IS NULL)))"#,
            next = position + 1,
        ));
        position += 2;
    }
    let _ = position;
    sql.push_str(r#" ORDER BY "agent_chat_approval"."requested_at" DESC LIMIT 200"#);
    let mut query = sqlx::query_as::<_, ChatApprovalRow>(&sql)
        .bind(user_id)
        .bind(pidash_auth::permissions::runner::VISIBILITY_PRIVATE as i16);
    if let Some(workspace_id) = workspace_id {
        query = query.bind(workspace_id);
        if !admin {
            query = query.bind(user_id);
        }
    } else {
        query = query.bind(user_id).bind(user_id);
    }
    let rows: Vec<ChatApprovalRow> = match query.fetch_all(&pool).await {
        Ok(rows) => rows,
        Err(_) => return server_error(),
    };
    let mut rendered = Vec::with_capacity(rows.len());
    for row in rows {
        let approval = match chat_approval_from_row(row) {
            Ok(approval) => approval,
            Err(response) => return response,
        };
        rendered.push(render_chat_approval(&approval));
    }
    json_response(StatusCode::OK, format!("[{}]", rendered.join(",")))
}

/// `POST /api/runners/chat/approvals/<id>/decide/` (`chat.py:463-510`):
/// validate the decision (400s before the transaction), lock the
/// approval, decide it, append `approval_decided`, and send
/// `chat_decide` after commit (unisolated — 500 on offline, with the
/// decision persisted).
pub async fn chat_approval_decide(
    State(state): State<AppState>,
    extension: Option<Extension<SessionHandle>>,
    Path(approval_id): Path<String>,
    req: Request<axum::body::Body>,
) -> Response {
    let (pool, user_id) = match web_actor(&state, extension).await {
        Ok(preamble) => preamble,
        Err(response) => return response,
    };
    let approval_id: Uuid = match approval_id.parse() {
        Ok(approval_id) => approval_id,
        Err(_) => return chat_not_found(),
    };
    let data = match read_request_data(&state, req).await {
        Ok(data) => data,
        Err(response) => return response,
    };
    let decision = match pidash_services::runner_runs::shape::validate_approval_decision(&data) {
        Ok(decision) => decision,
        Err(body) => {
            return json_response(StatusCode::BAD_REQUEST, body.to_string());
        }
    };
    let (status, decision_wire) = match decision {
        pidash_services::runner_runs::shape::ApprovalDecision::Accept => {
            (ApprovalStatus::Accepted, "accept")
        }
        pidash_services::runner_runs::shape::ApprovalDecision::Decline => {
            (ApprovalStatus::Declined, "decline")
        }
    };
    let mut tx = match pool.begin().await {
        Ok(tx) => tx,
        Err(_) => return server_error(),
    };
    // The approval, session, and runner each lock — the same three
    // locks Django's `select_for_update` + `select_related` takes.
    let approval_row: Option<ChatApprovalRow> = match sqlx::query_as(
        r#"SELECT "id", "session_id", "local_approval_id", "kind", "payload", "reason",
              "status", "decision_source", "decided_by_id", "requested_at",
              "expires_at", "decided_at"
           FROM "agent_chat_approval" WHERE "id" = $1 LIMIT 1 FOR UPDATE"#,
    )
    .bind(approval_id)
    .fetch_optional(&mut *tx)
    .await
    {
        Ok(approval_row) => approval_row,
        Err(_) => return server_error(),
    };
    let Some(approval_row) = approval_row else {
        return chat_not_found();
    };
    let approval = match chat_approval_from_row(approval_row) {
        Ok(approval) => approval,
        Err(response) => return response,
    };
    let locked = match lock_chat_session(&mut tx, approval.session_id).await {
        Ok(locked) => locked,
        Err(response) => return response,
    };
    // A dangling session FK under a live approval is the source's
    // `DoesNotExist` on nested access (500), not a 404.
    let Some(locked) = locked else {
        return server_error();
    };
    let session = locked.session;
    let runner = locked.runner;
    if !can_decide_session(&pool, user_id, &session, &runner).await {
        return chat_not_found();
    }
    if approval.status != ApprovalStatus::Pending {
        return json_response(
            StatusCode::CONFLICT,
            r#"{"error":"already decided"}"#.to_owned(),
        );
    }
    let decided_at = Utc::now();
    if sqlx::query(
        r#"UPDATE "agent_chat_approval"
           SET "status" = $1, "decision_source" = $2, "decided_by_id" = $3, "decided_at" = $4
           WHERE "id" = $5"#,
    )
    .bind(status.value())
    .bind("web")
    .bind(user_id)
    .bind(decided_at)
    .bind(approval_id)
    .execute(&mut *tx)
    .await
    .is_err()
    {
        return server_error();
    }
    let mut effects = Vec::new();
    let inputs = chat_kernel::append_event_inputs(
        "approval_decided",
        Some(&serde_json::json!({
            "approval_id": approval_id.to_string(),
            "decision": decision_wire,
        })),
        None,
        "",
    );
    if append_event(&mut tx, session.id, &inputs, &mut effects)
        .await
        .is_err()
    {
        return server_error();
    }
    if commit_tx(tx).await.is_err() {
        return server_error();
    }
    let ports = LivePorts::new(pool.clone(), &state);
    if drain_chat_effects(&pool, &ports, effects).await.is_err() {
        return server_error();
    }
    if drain_direct_sends(
        &ports,
        vec![DirectSend::Decide {
            runner_id: session.runner_id,
            session_id: session.id,
            approval_id,
            local_approval_id: approval.local_approval_id.clone(),
            decision: decision_wire.to_owned(),
            decided_by: user_id,
        }],
    )
    .await
    .is_err()
    {
        return server_error();
    }
    let approval = AgentChatApprovalRequest {
        status,
        decision_source: "web".to_owned(),
        decided_by_id: Some(user_id),
        decided_at: Some(decided_at),
        ..approval
    };
    json_response(StatusCode::OK, render_chat_approval(&approval))
}

/// `can_decide_chat` over live reads (`chat.py:38-41`).
async fn can_decide_session(
    pool: &PgPool,
    user_id: Uuid,
    session: &AgentChatSession,
    runner: &RunnerGuard,
) -> bool {
    // The `can_read_chat` predicate over the approval's session: no
    // creator shortcut, for the same reason as `can_read_session`.
    let role = workspace_role(pool, session.workspace_id, user_id)
        .await
        .unwrap_or(None);
    let member = role.is_some();
    let admin = role.is_some_and(|role| role >= ROLE_ADMIN);
    let visible = runner_visible_to_user(runner, user_id);
    chat_kernel::can_decide_chat_approval(
        user_id,
        session.created_by_id,
        || member,
        || visible,
        || admin,
    )
}

// ---------------------------------------------------------------------------
// Daemon handlers (7 chat endpoints)
// ---------------------------------------------------------------------------

/// The daemon chat request head: the resolved session, the body,
/// and the idempotency key. The authenticated runner is fully
/// consumed by `_resolve` (ownership check only — no daemon chat
/// handler reads a runner field past it).
struct DaemonChatRequest {
    session_id: Uuid,
    data: Value,
    key: String,
}

/// Authenticate, resolve the session (404/500/403), and read the
/// body — in that order, as DRF authenticates before the view reads
/// `request.data`.
async fn daemon_chat_preamble(
    state: &AppState,
    session_raw: &str,
    req: Request<axum::body::Body>,
) -> Result<(PgPool, DaemonChatRequest), Response> {
    let pool = pool_of(state)?.clone();
    let session_id: Uuid = session_raw.parse().map_err(|_| daemon_chat_not_found())?;
    let secret = state.settings().secret_key.clone();
    let runner = authenticate_daemon(&pool, secret.as_bytes(), req.headers()).await?;
    let resolved = resolve_chat_session(&pool, session_id, runner.as_ref()).await?;
    if runner.is_none() {
        // Unreachable: `_resolve` 500s anonymous callers first.
        return Err(server_error());
    }
    let key = chat_idempotency_key(req.headers());
    let data = read_request_data(state, req).await?;
    Ok((
        pool,
        DaemonChatRequest {
            session_id: resolved.id,
            data,
            key,
        },
    ))
}

/// Lock the daemon session row (`.get()` — a race miss is the
/// source's `DoesNotExist`, 500, never 404: `_resolve` already
/// answered the 404).
async fn lock_daemon_session(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    session_id: Uuid,
) -> Result<AgentChatSession, Response> {
    match lock_session_row(tx, session_id).await? {
        Some(session) => Ok(session),
        None => Err(server_error()),
    }
}

/// Roll a poisoned transaction back and answer the chat duplicate
/// (see [`record_chat_dedupe`]).
async fn answer_chat_duplicate(tx: sqlx::Transaction<'_, sqlx::Postgres>) -> Response {
    let _ = tx.rollback().await;
    json_response(StatusCode::OK, r#"{"ok":true,"duplicate":true}"#.to_owned())
}

/// `POST /api/v1/runner/chat/sessions/<id>/started/`
/// (`chat.py:534-564`): idempotency first (400 without a key), then
/// lock, dedupe, the thread stamp, and `chat_started`.
pub async fn daemon_chat_started(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    req: Request<axum::body::Body>,
) -> Response {
    let (pool, preamble) = match daemon_chat_preamble(&state, &session_id, req).await {
        Ok(preamble) => preamble,
        Err(response) => return response,
    };
    if preamble.key.is_empty() {
        return missing_idempotency_key();
    }
    let mut tx = match pool.begin().await {
        Ok(tx) => tx,
        Err(_) => return server_error(),
    };
    let session = match lock_daemon_session(&mut tx, preamble.session_id).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    match record_chat_dedupe(&mut tx, session.id, &preamble.key).await {
        Ok(true) => {}
        Ok(false) => return answer_chat_duplicate(tx).await,
        Err(response) => return response,
    }
    // Frame reads land here, after the dedupe, exactly as in the
    // source — a non-dict body records the key, then 500s (and the
    // 500 rolls the key back).
    let Some(obj) = preamble.data.as_object() else {
        return server_error();
    };
    let local_thread_id = match frame_text(obj.get("local_thread_id").unwrap_or(&Value::Null), 128)
    {
        Ok(local_thread_id) => local_thread_id,
        Err(response) => return response,
    };
    let local_session_id =
        match frame_text(obj.get("local_session_id").unwrap_or(&Value::Null), 128) {
            Ok(local_session_id) => local_session_id,
            Err(response) => return response,
        };
    let agent_kind = match frame_text(obj.get("agent_kind").unwrap_or(&Value::Null), 24) {
        Ok(agent_kind) => agent_kind,
        Err(response) => return response,
    };
    if sqlx::query(
        r#"UPDATE "agent_chat_session"
           SET "local_thread_id" = $1, "local_session_id" = $2, "agent_kind" = $3,
               "updated_at" = $4
           WHERE "id" = $5"#,
    )
    .bind(&local_thread_id)
    .bind(&local_session_id)
    .bind(&agent_kind)
    .bind(Utc::now())
    .bind(session.id)
    .execute(&mut *tx)
    .await
    .is_err()
    {
        return server_error();
    }
    let mut effects = Vec::new();
    let inputs = chat_kernel::append_event_inputs(
        "chat_started",
        Some(&serde_json::json!({
            "local_thread_id": local_thread_id,
            "local_session_id": local_session_id,
            "agent_kind": agent_kind,
        })),
        None,
        "",
    );
    if append_event(&mut tx, session.id, &inputs, &mut effects)
        .await
        .is_err()
    {
        return server_error();
    }
    if commit_tx(tx).await.is_err() {
        return server_error();
    }
    if !effects.is_empty() {
        let ports = LivePorts::new(pool.clone(), &state);
        if drain_chat_effects(&pool, &ports, effects).await.is_err() {
            return server_error();
        }
    }
    json_response(StatusCode::OK, r#"{"ok":true}"#.to_owned())
}

/// `POST .../messages/<message_id>/started/` (`chat.py:567-596`): the
/// turn-scoped idempotency key (`message_started:<id>`) dedupes
/// without the header; then the user message flips to `sent`, the
/// assistant placeholder opens, and `turn_started` fires.
pub async fn daemon_chat_message_started(
    State(state): State<AppState>,
    Path((session_id, message_id)): Path<(String, String)>,
    req: Request<axum::body::Body>,
) -> Response {
    let (pool, preamble) = match daemon_chat_preamble(&state, &session_id, req).await {
        Ok(preamble) => preamble,
        Err(response) => return response,
    };
    let message_id: Uuid = match message_id.parse() {
        Ok(message_id) => message_id,
        Err(_) => return daemon_chat_not_found(),
    };
    let mut tx = match pool.begin().await {
        Ok(tx) => tx,
        Err(_) => return server_error(),
    };
    let session = match lock_daemon_session(&mut tx, preamble.session_id).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    let source_key = format!("message_started:{message_id}");
    let existing: Option<i64> = match sqlx::query_scalar(
        r#"SELECT "id" FROM "agent_chat_event" WHERE ("session_id" = $1 AND "source_key" = $2)"#,
    )
    .bind(session.id)
    .bind(&source_key)
    .fetch_optional(&mut *tx)
    .await
    {
        Ok(existing) => existing,
        Err(_) => return server_error(),
    };
    if existing.is_some() {
        if commit_tx(tx).await.is_err() {
            return server_error();
        }
        return json_response(StatusCode::OK, r#"{"ok":true,"duplicate":true}"#.to_owned());
    }
    let Some(obj) = preamble.data.as_object() else {
        return server_error();
    };
    let turn_id = match frame_text(obj.get("turn_id").unwrap_or(&Value::Null), 128) {
        Ok(turn_id) => turn_id,
        Err(response) => return response,
    };
    // The user message flips to `sent` (a queryset `update` — zero
    // rows are fine).
    if sqlx::query(
        r#"UPDATE "agent_chat_message" SET "status" = $1, "local_turn_id" = $2
           WHERE ("id" = $3 AND "session_id" = $4)"#,
    )
    .bind(AgentChatMessageStatus::Sent.value())
    .bind(&turn_id)
    .bind(message_id)
    .bind(session.id)
    .execute(&mut *tx)
    .await
    .is_err()
    {
        return server_error();
    }
    let assistant = match create_assistant(&mut tx, session.id, &turn_id).await {
        Ok(assistant) => assistant,
        Err(response) => return response,
    };
    if sqlx::query(
        r#"UPDATE "agent_chat_session" SET "active_turn_id" = $1, "updated_at" = $2
           WHERE "id" = $3"#,
    )
    .bind(&turn_id)
    .bind(Utc::now())
    .bind(session.id)
    .execute(&mut *tx)
    .await
    .is_err()
    {
        return server_error();
    }
    let mut effects = Vec::new();
    let inputs = chat_kernel::append_event_inputs(
        "turn_started",
        Some(&serde_json::json!({
            "message_id": message_id.to_string(),
            "turn_id": turn_id,
        })),
        Some(assistant.id),
        &source_key,
    );
    if append_event(&mut tx, session.id, &inputs, &mut effects)
        .await
        .is_err()
    {
        return server_error();
    }
    if commit_tx(tx).await.is_err() {
        return server_error();
    }
    if !effects.is_empty() {
        let ports = LivePorts::new(pool.clone(), &state);
        if drain_chat_effects(&pool, &ports, effects).await.is_err() {
            return server_error();
        }
    }
    json_response(StatusCode::OK, r#"{"ok":true}"#.to_owned())
}

/// `POST .../messages/<message_id>/events/` (`chat.py:599-654`): the
/// header key is the `source_key` (a hit replays the stored event in
/// the duplicate answer); otherwise the frame appends, `assistant_delta`
/// payloads accumulate onto the assistant message, and the event
/// links to it.
pub async fn daemon_chat_event(
    State(state): State<AppState>,
    Path((session_id, message_id)): Path<(String, String)>,
    req: Request<axum::body::Body>,
) -> Response {
    let (pool, preamble) = match daemon_chat_preamble(&state, &session_id, req).await {
        Ok(preamble) => preamble,
        Err(response) => return response,
    };
    // The message id rides the URL for routing only (the handler
    // never reads it past the converter); an unparseable id is the
    // endpoint's 404, as Django's `<uuid:>` resolution 404s.
    if message_id.parse::<Uuid>().is_err() {
        return daemon_chat_not_found();
    }
    if preamble.key.is_empty() {
        return missing_idempotency_key();
    }
    let mut tx = match pool.begin().await {
        Ok(tx) => tx,
        Err(_) => return server_error(),
    };
    let session = match lock_daemon_session(&mut tx, preamble.session_id).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    let existing: Option<EventRow> = match sqlx::query_as(
        r#"SELECT "id", "session_id", "message_id", "seq", "source_key", "kind",
              "payload", "created_at"
           FROM "agent_chat_event" WHERE ("session_id" = $1 AND "source_key" = $2)"#,
    )
    .bind(session.id)
    .bind(truncate_chars(&preamble.key, 160))
    .fetch_optional(&mut *tx)
    .await
    {
        Ok(existing) => existing,
        Err(_) => return server_error(),
    };
    if let Some(existing) = existing {
        let event = match event_from_row(existing) {
            Ok(event) => event,
            Err(response) => return response,
        };
        if commit_tx(tx).await.is_err() {
            return server_error();
        }
        return json_response(
            StatusCode::OK,
            serde_json::json!({
                "ok": true,
                "duplicate": true,
                "event": serde_json::from_str::<Value>(&render_event(&event))
                    .unwrap_or(Value::Null),
            })
            .to_string(),
        );
    }
    let Some(obj) = preamble.data.as_object() else {
        return server_error();
    };
    let kind = match daemon_event_kind(obj.get("kind").unwrap_or(&Value::Null)) {
        Ok(kind) => kind,
        Err(_) => return server_error(),
    };
    let payload = match obj.get("payload") {
        Some(payload) if py_truthy(payload) => payload.clone(),
        _ => Value::Object(Default::default()),
    };
    let payload = inject_bridge_seq(&payload, obj.get("bridge_seq").unwrap_or(&Value::Null));
    if payload_too_large(&payload) {
        return json_response(
            StatusCode::PAYLOAD_TOO_LARGE,
            r#"{"error":"payload_too_large"}"#.to_owned(),
        );
    }
    let mut effects = Vec::new();
    let inputs = chat_kernel::append_event_inputs(
        kind.as_str(),
        Some(&payload),
        None,
        preamble.key.as_str(),
    );
    let mut event = match append_event(&mut tx, session.id, &inputs, &mut effects).await {
        Ok(event) => event,
        Err(response) => return response,
    };
    if kind == "assistant_delta" {
        let delta = assistant_delta_text(&payload);
        let mut assistant = match active_assistant(&mut tx, &session).await {
            Ok(assistant) => assistant,
            Err(response) => return response,
        };
        if !delta.is_empty() {
            if assistant.is_none() {
                assistant =
                    match create_assistant(&mut tx, session.id, &session.active_turn_id).await {
                        Ok(assistant) => Some(assistant),
                        Err(response) => return response,
                    };
            }
            let assistant = assistant.expect("created above");
            let content = format!("{}{delta}", assistant.content);
            if sqlx::query(r#"UPDATE "agent_chat_message" SET "content" = $1 WHERE "id" = $2"#)
                .bind(&content)
                .bind(assistant.id)
                .execute(&mut *tx)
                .await
                .is_err()
            {
                return server_error();
            }
            if sqlx::query(r#"UPDATE "agent_chat_event" SET "message_id" = $1 WHERE "id" = $2"#)
                .bind(assistant.id)
                .bind(event.id)
                .execute(&mut *tx)
                .await
                .is_err()
            {
                return server_error();
            }
            event.message_id = Some(assistant.id);
        } else if let Some(assistant) = assistant {
            if sqlx::query(r#"UPDATE "agent_chat_event" SET "message_id" = $1 WHERE "id" = $2"#)
                .bind(assistant.id)
                .bind(event.id)
                .execute(&mut *tx)
                .await
                .is_err()
            {
                return server_error();
            }
            event.message_id = Some(assistant.id);
        }
    } else {
        let assistant = match active_assistant(&mut tx, &session).await {
            Ok(assistant) => assistant,
            Err(response) => return response,
        };
        if let Some(assistant) = assistant {
            if sqlx::query(r#"UPDATE "agent_chat_event" SET "message_id" = $1 WHERE "id" = $2"#)
                .bind(assistant.id)
                .bind(event.id)
                .execute(&mut *tx)
                .await
                .is_err()
            {
                return server_error();
            }
            event.message_id = Some(assistant.id);
        }
    }
    if commit_tx(tx).await.is_err() {
        return server_error();
    }
    if !effects.is_empty() {
        let ports = LivePorts::new(pool.clone(), &state);
        if drain_chat_effects(&pool, &ports, effects).await.is_err() {
            return server_error();
        }
    }
    json_response(
        StatusCode::OK,
        serde_json::json!({
            "ok": true,
            "event": serde_json::from_str::<Value>(&render_event(&event)).unwrap_or(Value::Null),
        })
        .to_string(),
    )
}

/// `POST /api/v1/runner/chat/approvals/` (`chat.py:640-672`): the
/// local id is required (400) and the payload sized (413) before the
/// transaction; inside, the approval upserts (resetting to
/// `PENDING`) and `approval_requested` fires. No idempotency header
/// on this endpoint.
pub async fn daemon_chat_approval(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    req: Request<axum::body::Body>,
) -> Response {
    let (pool, preamble) = match daemon_chat_preamble(&state, &session_id, req).await {
        Ok(preamble) => preamble,
        Err(response) => return response,
    };
    let Some(obj) = preamble.data.as_object() else {
        return server_error();
    };
    let local_approval_id = match obj.get("local_approval_id") {
        Some(value) if py_truthy(value) => match value.as_str() {
            Some(local_approval_id) => truncate_chars(local_approval_id, 160),
            None => return server_error(),
        },
        _ => String::new(),
    };
    if local_approval_id.is_empty() {
        return json_response(
            StatusCode::BAD_REQUEST,
            r#"{"error":"local_approval_id_required"}"#.to_owned(),
        );
    }
    let payload = match obj.get("payload") {
        Some(payload) if py_truthy(payload) => payload.clone(),
        _ => Value::Object(Default::default()),
    };
    if payload_too_large(&payload) {
        return json_response(
            StatusCode::PAYLOAD_TOO_LARGE,
            r#"{"error":"payload_too_large"}"#.to_owned(),
        );
    }
    let kind = match chat_approval_kind(obj.get("kind").unwrap_or(&Value::Null)) {
        Ok(kind) => kind,
        Err(_) => return server_error(),
    };
    // `reason` stores `str()` (a `TextField`).
    let reason = match obj.get("reason") {
        Some(reason) if py_truthy(reason) => match reason.as_str() {
            Some(reason) => reason.to_owned(),
            None => super::run_endpoints::py_str_value(reason),
        },
        _ => String::new(),
    };
    let expires_at = match super::run_endpoints::expires_at_text(&preamble.data) {
        Ok(expires_at) => expires_at,
        Err(_) => return server_error(),
    };
    let mut tx = match pool.begin().await {
        Ok(tx) => tx,
        Err(_) => return server_error(),
    };
    let session = match lock_daemon_session(&mut tx, preamble.session_id).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    // `update_or_create(session=..., local_approval_id=...)`: the
    // get, then the update or the create.
    let existing: Option<Uuid> = match sqlx::query_scalar(
        r#"SELECT "id" FROM "agent_chat_approval"
           WHERE ("session_id" = $1 AND "local_approval_id" = $2)"#,
    )
    .bind(session.id)
    .bind(&local_approval_id)
    .fetch_optional(&mut *tx)
    .await
    {
        Ok(existing) => existing,
        Err(_) => return server_error(),
    };
    let approval_id = match existing {
        Some(approval_id) => {
            if sqlx::query(
                r#"UPDATE "agent_chat_approval"
                   SET "kind" = $1, "payload" = $2, "reason" = $3, "status" = $4,
                       "expires_at" = $5::timestamptz
                   WHERE "id" = $6"#,
            )
            .bind(kind)
            .bind(&payload)
            .bind(&reason)
            .bind(ApprovalStatus::Pending.value())
            .bind(expires_at.as_deref())
            .bind(approval_id)
            .execute(&mut *tx)
            .await
            .is_err()
            {
                return server_error();
            }
            approval_id
        }
        None => {
            let approval_id = Uuid::new_v4();
            if sqlx::query(
                r#"INSERT INTO "agent_chat_approval"
                   ("id", "session_id", "local_approval_id", "kind", "payload", "reason",
                    "status", "decision_source", "decided_by_id", "requested_at",
                    "expires_at", "decided_at")
                   VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11::timestamptz, $12)"#,
            )
            .bind(approval_id)
            .bind(session.id)
            .bind(&local_approval_id)
            .bind(kind)
            .bind(&payload)
            .bind(&reason)
            .bind(ApprovalStatus::Pending.value())
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
            approval_id
        }
    };
    let mut effects = Vec::new();
    let inputs = chat_kernel::append_event_inputs(
        "approval_requested",
        Some(&serde_json::json!({
            "approval_id": approval_id.to_string(),
            "local_approval_id": local_approval_id,
        })),
        None,
        &format!("approval_requested:{local_approval_id}"),
    );
    if append_event(&mut tx, session.id, &inputs, &mut effects)
        .await
        .is_err()
    {
        return server_error();
    }
    if commit_tx(tx).await.is_err() {
        return server_error();
    }
    if !effects.is_empty() {
        let ports = LivePorts::new(pool.clone(), &state);
        if drain_chat_effects(&pool, &ports, effects).await.is_err() {
            return server_error();
        }
    }
    let row: Option<ChatApprovalRow> = match sqlx::query_as(
        r#"SELECT "id", "session_id", "local_approval_id", "kind", "payload", "reason",
              "status", "decision_source", "decided_by_id", "requested_at",
              "expires_at", "decided_at"
           FROM "agent_chat_approval" WHERE "id" = $1"#,
    )
    .bind(approval_id)
    .fetch_optional(&pool)
    .await
    {
        Ok(row) => row,
        Err(_) => return server_error(),
    };
    let Some(row) = row else {
        return server_error();
    };
    let approval = match chat_approval_from_row(row) {
        Ok(approval) => approval,
        Err(response) => return response,
    };
    json_response(
        StatusCode::OK,
        serde_json::json!({
            "ok": true,
            "approval": serde_json::from_str::<Value>(&render_chat_approval(&approval))
                .unwrap_or(Value::Null),
        })
        .to_string(),
    )
}

/// `POST .../messages/<message_id>/complete/` (`chat.py:675-712`):
/// the `message_complete:<id>` key dedupes; the newest streaming
/// assistant (unscoped by turn) finalizes with the optional text;
/// the turn completes; the `raw` receipt appends.
pub async fn daemon_chat_message_complete(
    State(state): State<AppState>,
    Path((session_id, message_id)): Path<(String, String)>,
    req: Request<axum::body::Body>,
) -> Response {
    let (pool, preamble) = match daemon_chat_preamble(&state, &session_id, req).await {
        Ok(preamble) => preamble,
        Err(response) => return response,
    };
    let message_id: Uuid = match message_id.parse() {
        Ok(message_id) => message_id,
        Err(_) => return daemon_chat_not_found(),
    };
    let mut tx = match pool.begin().await {
        Ok(tx) => tx,
        Err(_) => return server_error(),
    };
    let session = match lock_daemon_session(&mut tx, preamble.session_id).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    let source_key = format!("message_complete:{message_id}");
    let existing: Option<i64> = match sqlx::query_scalar(
        r#"SELECT "id" FROM "agent_chat_event" WHERE ("session_id" = $1 AND "source_key" = $2)"#,
    )
    .bind(session.id)
    .bind(&source_key)
    .fetch_optional(&mut *tx)
    .await
    {
        Ok(existing) => existing,
        Err(_) => return server_error(),
    };
    if existing.is_some() {
        if commit_tx(tx).await.is_err() {
            return server_error();
        }
        return json_response(StatusCode::OK, r#"{"ok":true,"duplicate":true}"#.to_owned());
    }
    let Some(obj) = preamble.data.as_object() else {
        return server_error();
    };
    let final_status = complete_final_status(obj.get("status").unwrap_or(&Value::Null));
    let assistant_text = match obj.get("assistant_message") {
        Some(text) if py_truthy(text) => match text.as_str() {
            Some(text) => text.to_owned(),
            None => super::run_endpoints::py_str_value(text),
        },
        _ => String::new(),
    };
    // The newest streaming assistant, unscoped by turn (unlike the
    // delta path's `active_assistant_message_locked`).
    let assistant: Option<MessageRow> = match sqlx::query_as(
        r#"SELECT "id", "session_id", "role", "content", "content_parts", "status",
              "local_item_id", "local_turn_id", "seq", "created_at", "completed_at"
           FROM "agent_chat_message"
           WHERE ("session_id" = $1 AND "role" = $2 AND "status" = $3)
           ORDER BY "created_at" DESC LIMIT 1"#,
    )
    .bind(session.id)
    .bind(AgentChatMessageRole::Assistant.value())
    .bind(AgentChatMessageStatus::Streaming.value())
    .fetch_optional(&mut *tx)
    .await
    {
        Ok(assistant) => assistant,
        Err(_) => return server_error(),
    };
    let mut assistant = match assistant.map(message_from_row).transpose() {
        Ok(assistant) => assistant,
        Err(response) => return response,
    };
    if !assistant_text.is_empty() || assistant.is_some() {
        if assistant.is_none() {
            // `turn_id` verbatim through the create (which truncates
            // to 128); a truthy non-string is the source's
            // `TypeError` (500).
            let turn_id = match obj.get("turn_id") {
                Some(turn_id) if py_truthy(turn_id) => match turn_id.as_str() {
                    Some(turn_id) => turn_id.to_owned(),
                    None => return server_error(),
                },
                _ => String::new(),
            };
            assistant = match create_assistant(&mut tx, session.id, &turn_id).await {
                Ok(assistant) => Some(assistant),
                Err(response) => return response,
            };
        }
        let assistant = assistant.expect("present or created above");
        let completed_at = Utc::now();
        if assistant_text.is_empty() {
            if sqlx::query(
                r#"UPDATE "agent_chat_message" SET "status" = $1, "completed_at" = $2
                   WHERE "id" = $3"#,
            )
            .bind(final_status.value())
            .bind(completed_at)
            .bind(assistant.id)
            .execute(&mut *tx)
            .await
            .is_err()
            {
                return server_error();
            }
        } else if sqlx::query(
            r#"UPDATE "agent_chat_message" SET "content" = $1, "status" = $2, "completed_at" = $3
               WHERE "id" = $4"#,
        )
        .bind(&assistant_text)
        .bind(final_status.value())
        .bind(completed_at)
        .bind(assistant.id)
        .execute(&mut *tx)
        .await
        .is_err()
        {
            return server_error();
        }
    }
    let mut effects = Vec::new();
    let payload = serde_json::json!({
        "status": final_status.value(),
        "message_id": message_id.to_string(),
    });
    if complete_turn(&mut tx, &session, final_status, &payload, &mut effects)
        .await
        .is_err()
    {
        return server_error();
    }
    let inputs = chat_kernel::append_event_inputs(
        "raw",
        Some(&serde_json::json!({"kind": "message_complete"})),
        None,
        &source_key,
    );
    if append_event(&mut tx, session.id, &inputs, &mut effects)
        .await
        .is_err()
    {
        return server_error();
    }
    if commit_tx(tx).await.is_err() {
        return server_error();
    }
    if !effects.is_empty() {
        let ports = LivePorts::new(pool.clone(), &state);
        if drain_chat_effects(&pool, &ports, effects).await.is_err() {
            return server_error();
        }
    }
    json_response(StatusCode::OK, r#"{"ok":true}"#.to_owned())
}

/// `POST /api/v1/runner/chat/sessions/<id>/failed/`
/// (`chat.py:715-750`): idempotency first (400 without a key), then
/// lock, dedupe, and the fail flow (`code` verbatim, `detail`
/// `str`-gated by the `[:2000]` slice).
pub async fn daemon_chat_failed(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    req: Request<axum::body::Body>,
) -> Response {
    let (pool, preamble) = match daemon_chat_preamble(&state, &session_id, req).await {
        Ok(preamble) => preamble,
        Err(response) => return response,
    };
    if preamble.key.is_empty() {
        return missing_idempotency_key();
    }
    let mut tx = match pool.begin().await {
        Ok(tx) => tx,
        Err(_) => return server_error(),
    };
    let session = match lock_daemon_session(&mut tx, preamble.session_id).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    match record_chat_dedupe(&mut tx, session.id, &preamble.key).await {
        Ok(true) => {}
        Ok(false) => return answer_chat_duplicate(tx).await,
        Err(response) => return response,
    }
    let Some(obj) = preamble.data.as_object() else {
        return server_error();
    };
    let code = obj
        .get("code")
        .filter(|value| py_truthy(value))
        .cloned()
        .unwrap_or(Value::String("chat_failed".to_owned()));
    // `detail[:2000]`: falsy reads `""`, strings slice, truthy
    // non-strings are the source's `TypeError` (500).
    let detail = match obj.get("detail") {
        Some(detail) if py_truthy(detail) => match detail.as_str() {
            Some(detail) => detail.to_owned(),
            None => return server_error(),
        },
        _ => String::new(),
    };
    let mut effects = Vec::new();
    if execute_fail_session(&mut tx, &session, &code, &detail, &mut effects)
        .await
        .is_err()
    {
        return server_error();
    }
    if commit_tx(tx).await.is_err() {
        return server_error();
    }
    if !effects.is_empty() {
        let ports = LivePorts::new(pool.clone(), &state);
        if drain_chat_effects(&pool, &ports, effects).await.is_err() {
            return server_error();
        }
    }
    json_response(StatusCode::OK, r#"{"ok":true}"#.to_owned())
}

/// `POST /api/v1/runner/chat/sessions/<id>/closed/`
/// (`chat.py:753-780`): idempotency first (400 without a key), then
/// lock, dedupe, and the close flow.
pub async fn daemon_chat_closed(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    req: Request<axum::body::Body>,
) -> Response {
    let (pool, preamble) = match daemon_chat_preamble(&state, &session_id, req).await {
        Ok(preamble) => preamble,
        Err(response) => return response,
    };
    if preamble.key.is_empty() {
        return missing_idempotency_key();
    }
    let mut tx = match pool.begin().await {
        Ok(tx) => tx,
        Err(_) => return server_error(),
    };
    let session = match lock_daemon_session(&mut tx, preamble.session_id).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    match record_chat_dedupe(&mut tx, session.id, &preamble.key).await {
        Ok(true) => {}
        Ok(false) => return answer_chat_duplicate(tx).await,
        Err(response) => return response,
    }
    let Some(obj) = preamble.data.as_object() else {
        return server_error();
    };
    let reason = obj
        .get("reason")
        .filter(|value| py_truthy(value))
        .cloned()
        .unwrap_or(Value::String("runner_closed".to_owned()));
    let mut effects = Vec::new();
    if execute_close_session(&mut tx, &session, &reason, &mut effects)
        .await
        .is_err()
    {
        return server_error();
    }
    if commit_tx(tx).await.is_err() {
        return server_error();
    }
    if !effects.is_empty() {
        let ports = LivePorts::new(pool.clone(), &state);
        if drain_chat_effects(&pool, &ports, effects).await.is_err() {
            return server_error();
        }
    }
    json_response(StatusCode::OK, r#"{"ok":true}"#.to_owned())
}

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------

/// An owned path: the listed methods serve from Rust, every other
/// method falls through to Django (the `app_scheduler` precedent).
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

/// Register the web chat routes (`runner/web_urls.py` chat block:
/// sessions, messages, warm, cancel, close, approvals, the SSE
/// stream). Merged under `RouteGroup::RunnerWeb` at the F-10 seam;
/// sibling handler issues extend the merge, keeping both sides.
pub fn web_routes() -> Router<AppState> {
    use axum::routing::{get, post};
    const POST_ONLY: &[&str] = &["GET", "PUT", "PATCH", "DELETE", "OPTIONS"];
    const GET_ONLY: &[&str] = &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"];
    const GET_POST: &[&str] = &["PUT", "PATCH", "DELETE", "OPTIONS"];
    Router::new()
        .route(
            "/api/runners/chat/sessions/",
            owned(get(chat_sessions_list).post(chat_session_create), GET_POST),
        )
        .route(
            "/api/runners/chat/sessions/{session_id}/",
            owned(get(chat_session_detail), GET_ONLY),
        )
        .route(
            "/api/runners/chat/sessions/{session_id}/messages/",
            owned(get(chat_messages_list).post(chat_message_create), GET_POST),
        )
        .route(
            "/api/runners/chat/sessions/{session_id}/warm/",
            owned(post(chat_session_warm), POST_ONLY),
        )
        .route(
            "/api/runners/chat/sessions/{session_id}/cancel/",
            owned(post(chat_session_cancel), POST_ONLY),
        )
        .route(
            "/api/runners/chat/sessions/{session_id}/close/",
            owned(post(chat_session_close), POST_ONLY),
        )
        .route(
            "/api/runners/chat/approvals/",
            owned(get(chat_approvals_list), GET_ONLY),
        )
        .route(
            "/api/runners/chat/approvals/{approval_id}/decide/",
            owned(post(chat_approval_decide), POST_ONLY),
        )
        .route(
            "/api/runners/chat/sessions/{session_id}/events/",
            owned(get(super::sse::chat_event_stream), GET_ONLY),
        )
}

/// Register the daemon chat routes (`runner/urls.py:204-236`).
/// Merged under `RouteGroup::Runner` at the F-10 seam; sibling
/// handler issues extend the merge, keeping both sides.
pub fn daemon_routes() -> Router<AppState> {
    use axum::routing::post;
    const POST_ONLY: &[&str] = &["GET", "PUT", "PATCH", "DELETE", "OPTIONS"];
    Router::new()
        .route(
            "/api/v1/runner/chat/sessions/{session_id}/started/",
            owned(post(daemon_chat_started), POST_ONLY),
        )
        .route(
            "/api/v1/runner/chat/sessions/{session_id}/messages/{message_id}/started/",
            owned(post(daemon_chat_message_started), POST_ONLY),
        )
        .route(
            "/api/v1/runner/chat/sessions/{session_id}/messages/{message_id}/events/",
            owned(post(daemon_chat_event), POST_ONLY),
        )
        .route(
            "/api/v1/runner/chat/sessions/{session_id}/approvals/",
            owned(post(daemon_chat_approval), POST_ONLY),
        )
        .route(
            "/api/v1/runner/chat/sessions/{session_id}/messages/{message_id}/complete/",
            owned(post(daemon_chat_message_complete), POST_ONLY),
        )
        .route(
            "/api/v1/runner/chat/sessions/{session_id}/failed/",
            owned(post(daemon_chat_failed), POST_ONLY),
        )
        .route(
            "/api/v1/runner/chat/sessions/{session_id}/closed/",
            owned(post(daemon_chat_closed), POST_ONLY),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    static FIXTURE: &str =
        include_str!("../../../../fixtures/runner_runs/fx-run-09-handlers-daemon.golden.json");

    fn section(name: &str) -> Value {
        let fx: Value = serde_json::from_str(FIXTURE).expect("fixture parses");
        fx.get(name).unwrap_or_else(|| panic!("{name}")).clone()
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
    fn fx_daemon_chat_answer_bodies_match() {
        let chat = section("daemon_chat");
        for (case, expected) in [
            ("resolve_404", json!({"error": "chat_session_not_found"})),
            (
                "resolve_403",
                json!({"error": "chat_session_not_owned_by_runner"}),
            ),
            (
                "started_no_key",
                json!({"error": "idempotency_key_required"}),
            ),
            ("started_ok", json!({"ok": true})),
            ("started_dup", json!({"ok": true, "duplicate": true})),
            ("msgstarted_ok", json!({"ok": true})),
            ("msgstarted_dup", json!({"ok": true, "duplicate": true})),
            ("msgstarted_unknown_msg", json!({"ok": true})),
            ("event_no_key", json!({"error": "idempotency_key_required"})),
            ("event_too_large", json!({"error": "payload_too_large"})),
            (
                "approval_no_local_id",
                json!({"error": "local_approval_id_required"}),
            ),
            ("approval_big", json!({"error": "payload_too_large"})),
            ("complete_dup", json!({"ok": true, "duplicate": true})),
            (
                "failed_no_key",
                json!({"error": "idempotency_key_required"}),
            ),
            ("failed_dup", json!({"ok": true, "duplicate": true})),
            ("failed_default_code", json!({"ok": true})),
            (
                "closed_no_key",
                json!({"error": "idempotency_key_required"}),
            ),
            ("closed_dup", json!({"ok": true, "duplicate": true})),
        ] {
            assert_eq!(fx_body(&chat, case), expected, "{case}");
        }
        // The duplicate event replay nests the stored event third.
        let dup = fx_body(&chat, "event_dup");
        let keys: Vec<&str> = dup
            .as_object()
            .expect("obj")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(keys, vec!["ok", "duplicate", "event"]);
        assert_eq!(chat["started_no_key"]["status"], 400);
        assert_eq!(chat["event_too_large"]["status"], 413);
        assert_eq!(chat["resolve_404"]["status"], 404);
        assert_eq!(chat["resolve_403"]["status"], 403);
        // The ported no-auth bug: a live `AttributeError` (Django
        // renders the unhandled raise as a 500), recorded as
        // `resolve_noauth`.
        assert_eq!(
            chat["resolve_noauth"]["raised"],
            json!("builtins.AttributeError: 'NoneType' object has no attribute 'id'")
        );
    }

    #[test]
    fn fx_web_chat_answer_bodies_match() {
        let chat = section("web_chat");
        for (case, expected) in [
            ("sess_list_forbidden_ws", json!({"error": "forbidden"})),
            (
                "sess_create_missing",
                json!({"error": "workspace and runner are required"}),
            ),
            ("sess_create_forbidden", json!({"error": "forbidden"})),
            (
                "sess_create_runner_gone",
                json!({"error": "runner_not_found"}),
            ),
            (
                "sess_create_runner_unowned",
                json!({"error": "runner_not_found"}),
            ),
            (
                "sess_create_offline",
                json!({"error": "runner_unavailable"}),
            ),
            (
                "sess_create_revoked",
                json!({"error": "runner_unavailable"}),
            ),
            ("detail_gone", json!({"error": "not found"})),
            ("detail_stranger", json!({"error": "not found"})),
            ("warm_gone", json!({"error": "not found"})),
            ("warm_closed", json!({"error": "chat_session_closed"})),
            ("warm_offline", json!({"error": "runner_unavailable"})),
            (
                "warm_active_skip",
                json!({"ok": true, "skipped": "chat_turn_active"}),
            ),
            ("warm_ok", json!({"ok": true})),
            ("msg_list_stranger", json!({"error": "not found"})),
            ("msg_post_empty", json!({"error": "content is required"})),
            ("msg_post_big", json!({"error": "payload_too_large"})),
            ("msg_post_closed", json!({"error": "chat_session_closed"})),
            ("msg_post_offline", json!({"error": "runner_unavailable"})),
            ("msg_post_active", json!({"error": "chat_turn_active"})),
            ("msg_post_stranger", json!({"error": "not found"})),
            ("cancel_gone", json!({"error": "not found"})),
            ("cancel_noop", json!({"ok": true, "noop": true})),
            ("cancel_ok", json!({"ok": true})),
            ("cappr_list_ws_forbidden", json!({"error": "forbidden"})),
            (
                "cappr_decide_bad",
                json!({"decision": ["\"maybe\" is not a valid choice."]}),
            ),
            ("cappr_decide_gone", json!({"error": "not found"})),
            ("cappr_decide_stranger", json!({"error": "not found"})),
            ("cappr_decide_again", json!({"error": "already decided"})),
            ("sess_list_runner_stranger_noscope", json!([])),
        ] {
            assert_eq!(fx_body(&chat, case), expected, "{case}");
        }
        assert_eq!(chat["warm_ok"]["status"], 202);
        assert_eq!(chat["sess_create_new"]["status"], 201);
        assert_eq!(chat["sess_create_reuse"]["status"], 200);
        assert_eq!(chat["msg_post_ok"]["status"], 201);
        assert_eq!(chat["msg_post_big"]["status"], 413);
    }

    #[test]
    fn assistant_delta_text_follows_the_fallback_chain() {
        assert_eq!(
            assistant_delta_text(&json!({"params": {"delta": "hi"}})),
            "hi"
        );
        assert_eq!(
            assistant_delta_text(&json!({"params": {"delta": {"text": "yo"}}})),
            "yo"
        );
        // A non-string `delta` without `.text` falls through to
        // `params.text`.
        assert_eq!(
            assistant_delta_text(&json!({"params": {"delta": 5, "text": "t"}})),
            "t"
        );
        assert_eq!(assistant_delta_text(&json!({"params": {"text": "t"}})), "t");
        assert_eq!(assistant_delta_text(&json!({"params": {}})), "");
        assert_eq!(assistant_delta_text(&json!({})), "");
        assert_eq!(assistant_delta_text(&json!("x")), "");
        assert_eq!(
            assistant_delta_text(&json!({"params": {"delta": {"text": 5}}})),
            ""
        );
    }

    #[test]
    fn daemon_event_kind_defaults_and_truncates() {
        assert_eq!(daemon_event_kind(&Value::Null).expect("null"), "raw");
        assert_eq!(daemon_event_kind(&json!("")).expect("empty"), "raw");
        assert_eq!(
            daemon_event_kind(&json!("assistant_delta")).expect("kind"),
            "assistant_delta"
        );
        assert_eq!(
            daemon_event_kind(&json!("k".repeat(100)))
                .expect("long")
                .chars()
                .count(),
            64
        );
        assert!(daemon_event_kind(&json!(5)).is_err());
    }

    #[test]
    fn bridge_seq_injects_on_present_not_truthy() {
        // Explicit `is not None`: falsy-but-present values inject.
        assert_eq!(
            inject_bridge_seq(&json!({"a": 1}), &json!(0)),
            json!({"a": 1, "bridge_seq": 0})
        );
        assert_eq!(
            inject_bridge_seq(&json!({"a": 1}), &json!("")),
            json!({"a": 1, "bridge_seq": ""})
        );
        assert_eq!(
            inject_bridge_seq(&json!({"a": 1}), &Value::Null),
            json!({"a": 1})
        );
        // Non-dict payloads wrap in `{"value": ...}` first.
        assert_eq!(
            inject_bridge_seq(&json!("s"), &json!(3)),
            json!({"value": "s", "bridge_seq": 3})
        );
    }

    #[test]
    fn complete_final_status_coerces_bogus_to_completed() {
        use pidash_types::runner_runs::AgentChatMessageStatus as Status;
        assert_eq!(
            complete_final_status(&json!("completed")),
            Status::Completed
        );
        assert_eq!(
            complete_final_status(&json!("cancelled")),
            Status::Cancelled
        );
        assert_eq!(complete_final_status(&json!("failed")), Status::Failed);
        assert_eq!(complete_final_status(&json!("bogus")), Status::Completed);
        assert_eq!(complete_final_status(&json!(5)), Status::Completed);
        assert_eq!(complete_final_status(&Value::Null), Status::Completed);
    }

    #[test]
    fn chat_approval_kind_map_matches_run_approvals() {
        assert_eq!(
            chat_approval_kind(&json!("COMMAND_EXECUTION")).expect("kind"),
            "command_execution"
        );
        assert_eq!(chat_approval_kind(&json!("x")).expect("kind"), "other");
        assert_eq!(chat_approval_kind(&Value::Null).expect("null"), "other");
        assert!(chat_approval_kind(&json!(5)).is_err());
    }

    #[test]
    fn runner_unavailable_covers_offline_and_revoked() {
        assert!(runner_unavailable("offline"));
        assert!(runner_unavailable("revoked"));
        assert!(!runner_unavailable("online"));
        assert!(!runner_unavailable("busy"));
        assert!(!runner_unavailable(""));
    }

    #[test]
    fn throttle_brake_allows_under_quota_and_denies_at_it() {
        let now = 1_700_000_000.0;
        assert!(matches!(
            evaluate_chat_send_throttle(&[], now),
            ChatSendVerdict::Allow
        ));
        let full: Vec<f64> = (0..60).map(|i| now - i as f64).collect();
        assert!(matches!(
            evaluate_chat_send_throttle(&full, now),
            ChatSendVerdict::Deny { .. }
        ));
        // Entries at the window edge have passed out.
        let aged: Vec<f64> = (0..60).map(|i| now - 60.0 - i as f64).collect();
        assert!(matches!(
            evaluate_chat_send_throttle(&aged, now),
            ChatSendVerdict::Allow
        ));
        // Histories round-trip through the cache codec; garbage
        // decodes empty (fail-open).
        let encoded = encode_throttle_history(&[1.5, 2.5]);
        assert_eq!(decode_throttle_history(&encoded), vec![1.5, 2.5]);
        assert!(decode_throttle_history("not json").is_empty());
        assert!(decode_throttle_history("gASV").is_empty());
        let user: Uuid = "0192d3b4-8c1c-7a2e-9f4b-6d5c8b7a6e5d"
            .parse()
            .expect("uuid");
        assert_eq!(
            chat_send_throttle_key(&user),
            "throttle_runner_chat_send_0192d3b4-8c1c-7a2e-9f4b-6d5c8b7a6e5d"
        );
    }

    #[test]
    fn payload_cap_is_256kib() {
        assert!(!payload_too_large(&json!({"content": "hi"})));
        let big = json!({"content": "x".repeat(300 * 1024)});
        assert!(payload_too_large(&big));
    }
}
