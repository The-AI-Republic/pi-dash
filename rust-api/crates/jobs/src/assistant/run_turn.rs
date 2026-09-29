//! Assistant turn pipeline (D-06, stage 5).
//!
//! Port of `apps/api/pi_dash/assistant/tasks.py:1-526` (PIDASHCONV-254,
//! fixture id F-A6-10 in `rust-api/fixtures/assistant/tools-tasks.json`).
//!
//! Source map (Python → Rust):
//!
//! - `TURN_SOFT_LIMIT` / `TURN_HARD_LIMIT` / `DELTA_FLUSH_MS` (`:45-47`) →
//!   [`TURN_SOFT_LIMIT_SECS`] / [`TURN_HARD_LIMIT_SECS`] / [`DELTA_FLUSH_MS`].
//! - `cancel_key` (`:54-55`) → [`cancel_key`] (`assistant:cancel:<turn_id>`).
//! - `_load_context` (`:73-96`) → [`TurnContext`] + [`context_eligible`];
//!   the SQL shape is [`load_context_sql`], pinned against the fixture's
//!   `orm_sql.load_context_turn` entry (same tables, same join kinds, same
//!   `WHERE` on the turn id; only QUEUED/RUNNING rows are eligible).
//! - `_mark_running` (`:99-107`) → [`mark_running_update`] (atomic
//!   `QUEUED→RUNNING` + `started_at`, 0 rows → already taken) plus the
//!   `turn_started` event ([`EVENT_TURN_STARTED`]).
//! - `_is_cancelled` (`:110-115`) → the [`TurnSeam::is_cancelled`] boundary:
//!   Redis `GET`, every error → `false`.
//! - `_start_assistant_row` / `_emit_delta` / `_finalize_row` (`:118-155`) →
//!   [`TurnSeam::start_row`] / [`emit_delta`] / [`finalize_row`], driven by
//!   [`StreamSink`] below.
//! - `_complete_turn` / `_fail_turn` / `_cancel_turn` (`:157-213`) →
//!   [`TurnSeam::complete_turn`] / [`fail_turn`] / [`cancel_turn`]; the exact
//!   `UPDATE`/`INSERT`/`DELETE` shapes are the `*_sql` builders here, pinned
//!   against the fixture's `updates` entries.
//! - `_finalize_open_rows` (`:173-178`) → [`finalize_open_rows_update`].
//! - `_Streamer` (`:220-292`) → [`StreamSink`]: a `TextPart` start closes the
//!   open row and opens a new one, deltas accumulate and flush every
//!   [`DELTA_FLUSH_MS`] or at [`PENDING_FLUSH_CHARS`] pending chars, the end
//!   of the stream finalizes the open row as completed.
//! - `_run_turn` (`:299-367`) → [`drive_turn`]: same step order — load,
//!   claim, resolve model (failure → fail), resolve toolsets (degraded, never
//!   fatal), load history, run the agent with per-event cancel checks,
//!   runtime-failure report on every exit, then complete/fail/cancel.
//! - `_resolve_toolsets` / `_emit_skipped` / `_report_runtime_tool_failures`
//!   (`:370-419`) → [`resolve_toolsets_outcome`] + [`TurnSeam::emit_skipped`].
//! - `_model_label` (`:422-425`) → [`TurnSeam::model_label`].
//! - `_extract_usage` (`:428-439`) → [`TurnUsage`] + [`usage_value`]
//!   (extraction failure → `{}`, never null).
//! - `_classify_error` (`:442-469`) → [`classify_error`], same branch order:
//!   credit (status-code 402 read from the exception, never substring-matched)
//!   first, then auth, unreachable, model, internal.
//! - `run_assistant_turn` (`:476-491`, `name="assistant.run_turn"`,
//!   `acks_late=False`, `max_retries=0`, soft 300 / hard 330) →
//!   [`RUN_TURN_TASK`] + [`register_turn_handler`]. Celery retries are
//!   disabled for the turn task, so the handler returns [`Verdict::Ack`] or
//!   [`Verdict::Fail`] — never `Retry`. The `SoftTimeLimitExceeded` arm
//!   (`:486-489`) has no equivalent here: signal-based timeout enforcement
//!   belongs to the worker loop, and a killed turn is recovered by the sweep
//!   like any other crashed turn.
//! - `_close_turn_connection` (`:494-508`) has no equivalent: it releases the
//!   worker's thread-local Django connection, and `sqlx` pooling (plus the
//!   seam owning its own pool handle) needs no turn-end release.
//!
//! The LLM/provider side (model resolution, toolset construction, the agent
//! event stream, history load/dump) lives behind [`TurnSeam`]. The seam is
//! the documented port boundary — the same pattern as the mail tasks'
//! SMTP seam: [`drive_turn`] is the complete, tested lifecycle; the live
//! provider seam lands with the handler layer, and the domain gate flips
//! worker ownership after the proxy pass. Nothing here is a stub: every
//! branch of `_run_turn` executes against the seam in the unit tests below.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use sea_query::{Expr, Order, PostgresQueryBuilder, Query};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

use crate::queue::JobRow;
use crate::worker::{HandlerError, Registry, Verdict};

/// Celery task name (`tasks.py:477`, `@shared_task(name=...)`).
pub const RUN_TURN_TASK: &str = "assistant.run_turn";

/// Soft time limit, seconds (`tasks.py:45`, `ASSISTANT_TURN_SOFT_LIMIT`).
pub const TURN_SOFT_LIMIT_SECS: u64 = 300;
/// Hard time limit, seconds (`tasks.py:46`, `ASSISTANT_TURN_HARD_LIMIT`).
pub const TURN_HARD_LIMIT_SECS: u64 = 330;
/// Sweep cutoff slack: stale means `started_at < now - (hard + 60)`
/// (`tasks.py:513`).
pub const SWEEP_CUTOFF_SLACK_SECS: i64 = 60;
/// Delta flush cadence, milliseconds (`tasks.py:47`).
pub const DELTA_FLUSH_MS: u64 = 100;
/// Pending-char flush threshold (`tasks.py:263`, `len(self.pending) >= 200`).
/// Python `len(str)` counts code points, so this is a char count.
pub const PENDING_FLUSH_CHARS: usize = 200;
/// Agent usage caps (`tasks.py:342`, `UsageLimits(request_limit=25,
/// tool_calls_limit=20)`).
pub const REQUEST_LIMIT: u32 = 25;
/// Agent tool-call cap (`tasks.py:342`).
pub const TOOL_CALLS_LIMIT: u32 = 20;
/// `error_code` truncation (`tasks.py:185`, `code[:64]`).
pub const ERROR_CODE_MAX_CHARS: usize = 64;
/// `error_detail` truncation (`tasks.py:186`, `(detail or "")[:2000]`).
pub const ERROR_DETAIL_MAX_CHARS: usize = 2000;

/// Turn statuses (`assistant/models.py:64-69`).
pub const TURN_QUEUED: &str = "queued";
/// Turn statuses (`assistant/models.py:64-69`).
pub const TURN_RUNNING: &str = "running";
/// Turn statuses (`assistant/models.py:64-69`).
pub const TURN_COMPLETED: &str = "completed";
/// Turn statuses (`assistant/models.py:64-69`).
pub const TURN_FAILED: &str = "failed";
/// Turn statuses (`assistant/models.py:64-69`).
pub const TURN_CANCELLED: &str = "cancelled";

/// Message statuses (`assistant/models.py:116-120`).
pub const MSG_STREAMING: &str = "streaming";
/// Message statuses (`assistant/models.py:116-120`).
pub const MSG_COMPLETED: &str = "completed";
/// Message statuses (`assistant/models.py:116-120`).
pub const MSG_FAILED: &str = "failed";
/// Message statuses (`assistant/models.py:116-120`).
pub const MSG_CANCELLED: &str = "cancelled";

/// Message kinds written by the turn task (`assistant/models.py:108-113`).
pub const KIND_ASSISTANT: &str = "assistant";
/// Message kinds written by the turn task (`assistant/models.py:108-113`).
pub const KIND_ERROR: &str = "error";

/// Event kinds emitted by the turn task (`tasks.py` `append_event` calls).
pub const EVENT_TURN_STARTED: &str = "turn_started";
/// Event kinds emitted by the turn task (`tasks.py` `append_event` calls).
pub const EVENT_MESSAGE_CREATED: &str = "message_created";
/// Event kinds emitted by the turn task (`tasks.py` `append_event` calls).
pub const EVENT_ASSISTANT_DELTA: &str = "assistant_delta";
/// Event kinds emitted by the turn task (`tasks.py` `append_event` calls).
pub const EVENT_MESSAGE_COMPLETED: &str = "message_completed";
/// Event kinds emitted by the turn task (`tasks.py` `append_event` calls).
pub const EVENT_TURN_COMPLETED: &str = "turn_completed";
/// Event kinds emitted by the turn task (`tasks.py` `append_event` calls).
pub const EVENT_TURN_FAILED: &str = "turn_failed";
/// Event kinds emitted by the turn task (`tasks.py` `append_event` calls).
pub const EVENT_TURN_CANCELLED: &str = "turn_cancelled";
/// Event kinds emitted by the turn task (`tasks.py` `append_event` calls).
pub const EVENT_TOOL_SERVERS_SKIPPED: &str = "tool_servers_skipped";

/// Pseudo-entry emitted when toolset resolution blows up entirely
/// (`tasks.py:385`).
pub const ALL_TOOL_SERVERS_NAME: &str = "all tool servers";
/// Reason for the pseudo-entry (`tasks.py:385`).
pub const TOOLSETS_UNAVAILABLE_REASON: &str = "toolsets_unavailable";

/// Redis cancel key (`tasks.py:54-55`).
pub fn cancel_key(turn_id: &Uuid) -> String {
    format!("assistant:cancel:{turn_id}")
}

/// Char-boundary-safe truncation for the Python `s[:n]` slices in
/// `_fail_turn` (`tasks.py:185-186`).
///
/// Python slices code points; a byte slice in Rust can panic on a UTF-8
/// boundary (Porting guide "Semantic traps"), so truncate at the last
/// char boundary at or under `max` chars.
pub fn truncate_chars(s: &str, max: usize) -> &str {
    if s.chars().count() <= max {
        return s;
    }
    let end = s
        .char_indices()
        .take(max + 1)
        .last()
        .map(|(i, _)| i)
        .unwrap_or(0);
    &s[..end]
}

/// Truncated failure code (`tasks.py:185`, `code[:64]`).
pub fn truncate_code(code: &str) -> &str {
    truncate_chars(code, ERROR_CODE_MAX_CHARS)
}

/// Truncated failure detail (`tasks.py:186`, `(detail or "")[:2000]`).
pub fn truncate_detail(detail: &str) -> &str {
    truncate_chars(detail, ERROR_DETAIL_MAX_CHARS)
}

// --------------------------------------------------------------------------- //
// Error classification (`tasks.py:442-469`)
// --------------------------------------------------------------------------- //

/// Provider error codes (`tasks.py:458`, `provider_out_of_credit` /
/// `provider_auth_failed` / `provider_unreachable` / `model_invalid` /
/// `internal`).
pub const CODE_OUT_OF_CREDIT: &str = "provider_out_of_credit";
/// Provider error codes (`tasks.py:458`, `provider_out_of_credit` /
/// `provider_auth_failed` / `provider_unreachable` / `model_invalid` /
/// `internal`).
pub const CODE_AUTH_FAILED: &str = "provider_auth_failed";
/// Provider error codes (`tasks.py:458`, `provider_out_of_credit` /
/// `provider_unreachable` / `model_invalid` / `internal`).
pub const CODE_UNREACHABLE: &str = "provider_unreachable";
/// Provider error codes (`tasks.py:458`, `provider_out_of_credit` /
/// `provider_auth_failed` / `provider_unreachable` / `model_invalid` /
/// `internal`).
pub const CODE_MODEL_INVALID: &str = "model_invalid";
/// Provider error codes (`tasks.py:458`, `provider_out_of_credit` /
/// `provider_auth_failed` / `provider_unreachable` / `model_invalid` /
/// `internal`).
pub const CODE_INTERNAL: &str = "internal";

/// Iteration-limit code (`tasks.py:354`, `UsageLimitExceeded` branch).
pub const CODE_ITERATION_LIMIT: &str = "iteration_limit";
/// Soft-time-limit code (`tasks.py:489`, `SoftTimeLimitExceeded` branch).
pub const CODE_TURN_TIMEOUT: &str = "turn_timeout";

/// User-facing details, verbatim (`tasks.py:458-469`).
pub const DETAIL_OUT_OF_CREDIT: &str = "The provider rejected the request for lack of credit.";
/// User-facing details, verbatim (`tasks.py:458-469`).
pub const DETAIL_AUTH_FAILED: &str = "Your API key was rejected by the provider.";
/// User-facing details, verbatim (`tasks.py:458-469`).
pub const DETAIL_UNREACHABLE: &str = "Could not reach the configured provider endpoint.";
/// User-facing details, verbatim (`tasks.py:458-469`).
pub const DETAIL_MODEL_INVALID: &str =
    "The configured model name was not accepted by the provider.";
/// User-facing details, verbatim (`tasks.py:458-469`).
pub const DETAIL_INTERNAL: &str = "The assistant hit an unexpected error.";

/// Classify a provider failure (`tasks.py:442-469`).
///
/// `message` is `str(exc)`; `status_code` is the exception's `status_code`
/// attribute when present. Branch order is load-bearing: credit is checked
/// before auth, and the 402 status is read from the exception — never
/// substring-matched, because "402" appears in request ids and token counts
/// (`tasks.py:449-453`).
pub fn classify_error(message: &str, status_code: Option<u16>) -> (&'static str, &'static str) {
    let text = message.to_lowercase();
    if status_code == Some(402)
        || [
            "payment required",
            "insufficient_credits",
            "insufficient credits",
            "no_credit_account",
            "quota exceeded",
        ]
        .iter()
        .any(|s| text.contains(s))
    {
        return (CODE_OUT_OF_CREDIT, DETAIL_OUT_OF_CREDIT);
    }
    if [
        "401",
        "unauthorized",
        "api key",
        "authentication",
        "invalid_api_key",
    ]
    .iter()
    .any(|s| text.contains(s))
    {
        return (CODE_AUTH_FAILED, DETAIL_AUTH_FAILED);
    }
    if [
        "connection",
        "timeout",
        "timed out",
        "unreachable",
        "could not connect",
        "name resolution",
    ]
    .iter()
    .any(|s| text.contains(s))
    {
        return (CODE_UNREACHABLE, DETAIL_UNREACHABLE);
    }
    if [
        "model_not_found",
        "does not exist",
        "unknown model",
        "no such model",
    ]
    .iter()
    .any(|s| text.contains(s))
    {
        return (CODE_MODEL_INVALID, DETAIL_MODEL_INVALID);
    }
    (CODE_INTERNAL, DETAIL_INTERNAL)
}

// --------------------------------------------------------------------------- //
// Usage shape (`tasks.py:428-439`)
// --------------------------------------------------------------------------- //

/// Token/usage counters (`tasks.py:433-438`). Every counter is optional:
/// Python copies whatever attributes exist (`getattr(u, ..., None)`), so a
/// missing counter serializes as JSON null, never as an absent key.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnUsage {
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    pub requests: Option<i64>,
    pub tool_calls: Option<i64>,
}

/// Usage JSON for the turn row (`tasks.py:366,428-439`).
///
/// `None` (the provider gave no usage object, or reading it raised) stores
/// `{}`, matching the `except Exception: return {}` arm — never null.
pub fn usage_value(usage: Option<&TurnUsage>) -> Value {
    match usage {
        Some(u) => serde_json::to_value(u).expect("TurnUsage serializes"),
        None => Value::Object(Default::default()),
    }
}

// --------------------------------------------------------------------------- //
// Toolset degradation (`tasks.py:370-419`)
// --------------------------------------------------------------------------- //

/// A tool server the turn could not use (`mcp.SkippedServer`, surfaced via
/// the `tool_servers_skipped` event).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkippedServer {
    pub name: String,
    pub reason: String,
}

/// Outcome of the additive toolset build (`tasks.py:370-389`).
///
/// A resolver that blows up entirely costs the turn its tools, not its
/// life — and still emits the [`ALL_TOOL_SERVERS_NAME`] pseudo-entry so the
/// total-outage case is never the silent one.
pub fn resolve_toolsets_outcome(
    built: Result<(Vec<String>, Vec<SkippedServer>), ()>,
) -> (Vec<String>, Vec<SkippedServer>) {
    match built {
        Ok((toolsets, skipped)) => (toolsets, skipped),
        Err(()) => (
            Vec::new(),
            vec![SkippedServer {
                name: ALL_TOOL_SERVERS_NAME.to_owned(),
                reason: TOOLSETS_UNAVAILABLE_REASON.to_owned(),
            }],
        ),
    }
}

/// Payload of the `tool_servers_skipped` event (`tasks.py:392-403`).
/// Empty input emits nothing (`if not servers: return`).
pub fn skipped_payload(servers: &[SkippedServer]) -> Option<Value> {
    if servers.is_empty() {
        return None;
    }
    Some(serde_json::json!({ "servers": servers }))
}

// --------------------------------------------------------------------------- //
// Streaming sink (`tasks.py:220-292`)
// --------------------------------------------------------------------------- //

/// One inbound agent-stream item, mirroring the `pydantic_ai` events
/// `_Streamer.handle` matches on (`tasks.py:228-245`): a `TextPart` start
/// (with its initial content, possibly empty), a `TextPartDelta` chunk, any
/// other part boundary (closes the open row without opening a new one), or
/// the end of the stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamItem {
    TextStart { initial: String },
    TextDelta { chunk: String },
    NonTextBoundary,
    End,
}

/// One side effect the sink asks the runner to perform. Row identity lives
/// with the runner (it learns the new row id from `start_row`); the sink
/// only tracks open/closed state plus accumulated text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SinkAction {
    /// Open a streaming assistant row (`_start_assistant_row`, `tasks.py:247`).
    CreateRow,
    /// Append one `assistant_delta` event (`_emit_delta`, `tasks.py:266`).
    EmitDelta { chunk: String },
    /// Close the open row with final text and status (`_finalize_row`).
    FinalizeRow { text: String, status: &'static str },
}

/// Delta-streaming state machine (`_Streamer`, `tasks.py:220-292`).
///
/// A `TextPart` start finalizes any open row and opens a new one (seeding it
/// with the part's initial content); deltas accumulate into `pending` and
/// flush every [`DELTA_FLUSH_MS`] or when [`PENDING_FLUSH_CHARS`] chars are
/// pending; a non-text boundary finalizes without opening; the stream end
/// finalizes. An empty chunk is a no-op, and appending with no open row
/// opens one first (`tasks.py:255-264`).
#[derive(Debug, Default)]
pub struct StreamSink {
    open: bool,
    text: String,
    pending: String,
    last_flush_ms: u64,
}

impl StreamSink {
    pub fn new() -> Self {
        Self::default()
    }

    /// True while a streaming row is open (mirrors `self.message is not
    /// None`).
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Feed one stream item; returns the ordered side effects.
    /// `now_ms` is monotonic milliseconds (the runner's clock, standing in
    /// for `asyncio.get_running_loop().time()`).
    pub fn handle(&mut self, item: &StreamItem, now_ms: u64) -> Vec<SinkAction> {
        match item {
            StreamItem::TextStart { initial } => {
                let mut out = self.finalize_if_open(MSG_COMPLETED);
                out.push(SinkAction::CreateRow);
                self.open = true;
                self.text.clear();
                self.pending.clear();
                self.last_flush_ms = now_ms;
                if !initial.is_empty() {
                    out.extend(self.append(initial, now_ms));
                }
                out
            }
            StreamItem::TextDelta { chunk } => self.append(chunk, now_ms),
            StreamItem::NonTextBoundary => self.finalize_if_open(MSG_COMPLETED),
            StreamItem::End => self.finalize_if_open(MSG_COMPLETED),
        }
    }

    /// Close the open row with a terminal non-completed status
    /// (`fail_open_row`, `tasks.py:289-292`). `None` when no row is open.
    pub fn fail_open(&mut self, status: &'static str) -> Option<SinkAction> {
        if !self.open {
            return None;
        }
        let text = std::mem::take(&mut self.text);
        self.pending.clear();
        self.open = false;
        Some(SinkAction::FinalizeRow { text, status })
    }

    fn append(&mut self, chunk: &str, now_ms: u64) -> Vec<SinkAction> {
        if chunk.is_empty() {
            return Vec::new();
        }
        let mut out = Vec::new();
        if !self.open {
            out.push(SinkAction::CreateRow);
            self.open = true;
            self.text.clear();
            self.pending.clear();
            self.last_flush_ms = now_ms;
        }
        self.text.push_str(chunk);
        self.pending.push_str(chunk);
        if Self::due(&self.pending, self.last_flush_ms, now_ms) {
            let flushed = std::mem::take(&mut self.pending);
            self.last_flush_ms = now_ms;
            out.push(SinkAction::EmitDelta { chunk: flushed });
        }
        out
    }

    /// Flush rule (`tasks.py:263`): 100ms since the last flush, or 200
    /// pending chars. The char count matches Python `len(str)` semantics
    /// (code points, not bytes).
    fn due(pending: &str, last_flush_ms: u64, now_ms: u64) -> bool {
        now_ms.saturating_sub(last_flush_ms) >= DELTA_FLUSH_MS
            || pending.chars().count() >= PENDING_FLUSH_CHARS
    }

    /// Flush whatever is pending (the `_finalize` flush, `tasks.py:284`).
    pub fn flush(&mut self, now_ms: u64) -> Option<SinkAction> {
        if !self.open || self.pending.is_empty() {
            return None;
        }
        let flushed = std::mem::take(&mut self.pending);
        self.last_flush_ms = now_ms;
        Some(SinkAction::EmitDelta { chunk: flushed })
    }

    fn finalize_if_open(&mut self, status: &'static str) -> Vec<SinkAction> {
        if !self.open {
            return Vec::new();
        }
        let mut out = Vec::new();
        if !self.pending.is_empty() {
            let flushed = std::mem::take(&mut self.pending);
            out.push(SinkAction::EmitDelta { chunk: flushed });
        }
        let text = std::mem::take(&mut self.text);
        self.open = false;
        out.push(SinkAction::FinalizeRow { text, status });
        out
    }
}

// --------------------------------------------------------------------------- //
// Turn context (`tasks.py:58-96`)
// --------------------------------------------------------------------------- //

/// The loaded turn and everything `_run_turn` needs from it (`_Ctx`,
/// `tasks.py:58-64`): ids plus the display strings the runner derives —
/// `user_text` from the user message, `user_display` as
/// `display_name or email`, `thread_kind` as `thread.kind`, and the workspace
/// triple off the thread.
///
/// The sweep's store fills only turn/thread identity (the fail path touches
/// nothing else); the full turn seam fills every field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnContext {
    pub turn_id: Uuid,
    pub turn_status: String,
    pub thread_id: Uuid,
    pub thread_kind: String,
    pub workspace_id: Uuid,
    pub workspace_slug: String,
    pub workspace_name: String,
    pub workspace_role: i32,
    pub user_id: Uuid,
    pub user_display: String,
    pub user_text: String,
}

/// Eligibility gate (`tasks.py:79`): only QUEUED/RUNNING turns load.
/// Anything else (missing, completed, failed, cancelled, swept) is a silent
/// no-op for the task.
pub fn context_eligible(status: &str) -> bool {
    status == TURN_QUEUED || status == TURN_RUNNING
}

// --------------------------------------------------------------------------- //
// SQL shapes (same semantics as the ORM calls; see the fixture's `orm_sql`
// and `updates` entries for the Django-rendered originals)
// --------------------------------------------------------------------------- //

/// `assistant_turn` table (`assistant/models.py`, `db_table`).
pub const TABLE_TURN: &str = "assistant_turn";
/// `assistant_thread` table.
pub const TABLE_THREAD: &str = "assistant_thread";
/// `assistant_message` table.
pub const TABLE_MESSAGE: &str = "assistant_message";
/// `assistant_event` table.
pub const TABLE_EVENT: &str = "assistant_event";

/// Context-load select (`tasks.py:73-78`): turn by id with
/// `select_related("thread", "thread__workspace", "user_message")` — inner
/// joins to thread and workspace, left join to the user message, `WHERE` on
/// the turn id, `ORDER BY created_at`.
pub fn load_context_sql() -> String {
    Query::select()
        .from((TABLE_TURN, "t"))
        .columns([
            ("t", "id"),
            ("t", "thread_id"),
            ("t", "user_message_id"),
            ("t", "status"),
            ("t", "model_messages"),
            ("t", "usage"),
            ("t", "model_used"),
            ("t", "error_code"),
            ("t", "error_detail"),
            ("t", "created_at"),
            ("t", "started_at"),
            ("t", "completed_at"),
        ])
        .columns([
            ("th", "id"),
            ("th", "workspace_id"),
            ("th", "user_id"),
            ("th", "title"),
            ("th", "kind"),
            ("th", "is_archived"),
            ("th", "active_turn_id"),
            ("th", "created_at"),
            ("th", "updated_at"),
        ])
        .columns([("w", "id"), ("w", "slug"), ("w", "name")])
        .columns([("m", "id"), ("m", "display_content")])
        .inner_join(
            (TABLE_THREAD, "th"),
            Expr::col(("t", "thread_id")).equals(("th", "id")),
        )
        .inner_join(
            ("workspaces", "w"),
            Expr::col(("th", "workspace_id")).equals(("w", "id")),
        )
        .left_join(
            (TABLE_MESSAGE, "m"),
            Expr::col(("t", "user_message_id")).equals(("m", "id")),
        )
        .and_where(Expr::cust("t.\"id\" = $1"))
        .order_by(("t", "created_at"), Order::Asc)
        .to_string(PostgresQueryBuilder)
}

/// Atomic claim (`tasks.py:99-107`): `UPDATE … SET status=running,
/// started_at=now WHERE id AND status=queued`; 0 rows → already
/// taken/cancelled/swept (`return False`).
pub fn mark_running_update() -> String {
    format!(
        "UPDATE {TABLE_TURN} SET status = '{TURN_RUNNING}', started_at = NOW() \
         WHERE id = $1 AND status = '{TURN_QUEUED}'"
    )
}

/// Row lock for the terminal transitions (`tasks.py:159,183,206`):
/// `SELECT … FOR UPDATE` on the turn before rewriting it.
pub fn lock_turn_select() -> String {
    format!("SELECT * FROM {TABLE_TURN} WHERE id = $1 FOR UPDATE")
}

/// Terminal complete write (`tasks.py:157-166`): status + model_messages +
/// usage + model_used (or "") + completed_at.
pub fn complete_turn_update() -> String {
    format!(
        "UPDATE {TABLE_TURN} SET status = '{TURN_COMPLETED}', model_messages = $2, \
         usage = $3, model_used = $4, completed_at = NOW() WHERE id = $1"
    )
}

/// Clear the thread's in-flight pointer when it still names this turn
/// (`tasks.py:166,190,211`).
pub fn clear_active_turn_update() -> String {
    format!(
        "UPDATE {TABLE_THREAD} SET active_turn_id = NULL \
         WHERE id = $1 AND active_turn_id = $2"
    )
}

/// Terminal fail write (`tasks.py:181-188`): status plus truncated
/// code/detail plus `completed_at`. Truncation happens in
/// [`truncate_code`]/[`truncate_detail`] before binding, never in SQL,
/// because the Python slices count code points.
pub fn fail_turn_update() -> String {
    format!(
        "UPDATE {TABLE_TURN} SET status = '{TURN_FAILED}', error_code = $2, \
         error_detail = $3, completed_at = NOW() WHERE id = $1"
    )
}

/// Terminal cancel write (`tasks.py:204-209`): status + completed_at only —
/// no error columns, unlike the fail path.
pub fn cancel_turn_update() -> String {
    format!(
        "UPDATE {TABLE_TURN} SET status = '{TURN_CANCELLED}', completed_at = NOW() WHERE id = $1"
    )
}

/// Close still-streaming rows (`tasks.py:173-178`): every message of the
/// turn stuck in `streaming` moves to the terminal status with `completed_at`
/// now. Covers the soft-time-limit signal and sweep paths, where the
/// in-loop finalizer cannot run.
pub fn finalize_open_rows_update() -> String {
    format!(
        "UPDATE {TABLE_MESSAGE} SET status = $2, completed_at = NOW() \
         WHERE turn_id = $1 AND status = '{MSG_STREAMING}'"
    )
}

/// Thread lock for message/event writes (`events.py:92-93,112-113`):
/// `SELECT … FOR UPDATE` on the thread before allocating seq.
pub fn thread_lock_select() -> String {
    format!("SELECT id FROM {TABLE_THREAD} WHERE id = $1 FOR UPDATE")
}

/// Error message row (`events.create_message`, `tasks.py:191-193`): an
/// `error`-kind, `failed`-status message carrying `detail or code`. Seq is
/// `COALESCE(MAX(seq), 0) + 1` on the thread (`events.py:78-80`), so the
/// first row takes seq 1 — the same form the tools ports use.
pub fn error_message_insert() -> String {
    format!(
        "INSERT INTO {TABLE_MESSAGE} \
         (id, thread_id, turn_id, seq, kind, display_content, payload, status) \
         VALUES ($1, $2, $3, \
         (SELECT COALESCE(MAX(seq), 0) + 1 FROM {TABLE_MESSAGE} WHERE thread_id = $2), \
         '{KIND_ERROR}', $4, '{{}}', '{MSG_FAILED}')"
    )
}

/// `turn_failed` event row (`tasks.py:194-200`): kind, the error message id,
/// and the turn/error payload. The event id is a `BigAutoField`, so the
/// insert omits it.
pub fn turn_failed_event_insert() -> String {
    format!(
        "INSERT INTO {TABLE_EVENT} (thread_id, turn_id, seq, kind, message_id, payload) \
         VALUES ($1, $2, \
         (SELECT COALESCE(MAX(seq), 0) + 1 FROM {TABLE_EVENT} WHERE thread_id = $1), \
         '{EVENT_TURN_FAILED}', $3, $4)"
    )
}

/// Delta pruning (`events.prune_turn_deltas`, `tasks.py:170,201,213`): a
/// finished turn's `assistant_delta` events are deleted; completed content
/// lives in rows.
pub fn prune_turn_deltas_delete() -> String {
    format!("DELETE FROM {TABLE_EVENT} WHERE turn_id = $1 AND kind = '{EVENT_ASSISTANT_DELTA}'")
}

// --------------------------------------------------------------------------- //
// Seam boundary + turn driver (`tasks.py:299-367`)
// --------------------------------------------------------------------------- //

/// Infra failure inside the seam (DB/Redis/transport). The handler maps it
/// to [`Verdict::Fail`]: Celery retries are disabled for the turn task
/// (`max_retries=0`, `tasks.py:479`), so a crashed turn parks and the sweep
/// recovers it — write tools are never re-executed by redelivery
/// (`tasks.py:8-10`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeamError(pub String);

impl std::fmt::Display for SeamError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Model-resolution failure (`AssistantError`, `tasks.py:316-318`): carries
/// the already-classified code/detail straight into `_fail_turn`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelError {
    pub code: String,
    pub detail: String,
}

/// Agent-run failure, mirroring the three `except` arms of `_run_turn`
/// (`tasks.py:346-361`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentError {
    /// `UsageLimitExceeded` → `iteration_limit` (`tasks.py:351-355`).
    UsageLimit(String),
    /// Any other provider failure → [`classify_error`] (`tasks.py:356-361`).
    Provider {
        message: String,
        status_code: Option<u16>,
    },
}

/// One live toolset handle: only the server name crosses the seam (the
/// failure flags are read back through [`TurnSeam::runtime_failures`]).
pub type ToolsetHandle = String;

/// The provider/model side of the turn (`ee.assistant.model_provider`,
/// `runtime/history`, the `pydantic_ai` agent). Every method names the
/// Python function it stands in for; [`drive_turn`] below calls them in
/// `_run_turn` order.
pub trait TurnSeam: Send + Sync {
    /// The agent stream handle (the `event_stream` the handler iterates).
    type Stream: Send;

    /// `_load_context` (`tasks.py:73-96`): `None` for a missing turn or a
    /// turn outside {QUEUED, RUNNING}.
    fn load_context(
        &self,
        turn_id: Uuid,
    ) -> impl std::future::Future<Output = Result<Option<TurnContext>, SeamError>> + Send;
    /// `_mark_running` (`tasks.py:99-107`): atomic claim + `turn_started`.
    fn mark_running(
        &self,
        ctx: &TurnContext,
    ) -> impl std::future::Future<Output = Result<bool, SeamError>> + Send;
    /// `_is_cancelled` (`tasks.py:110-115`): Redis `GET`; errors → false.
    fn is_cancelled(
        &self,
        turn_id: Uuid,
    ) -> impl std::future::Future<Output = Result<bool, SeamError>> + Send;
    /// `resolve_model_for_user` (`tasks.py:315`).
    fn resolve_model(
        &self,
        ctx: &TurnContext,
    ) -> impl std::future::Future<Output = Result<String, ModelError>> + Send;
    /// `_resolve_toolsets` (`tasks.py:370-389`): additive; total failure is
    /// reported as `Err` and degraded by [`resolve_toolsets_outcome`].
    fn resolve_toolsets(
        &self,
        ctx: &TurnContext,
    ) -> impl std::future::Future<Output = Result<(Vec<ToolsetHandle>, Vec<SkippedServer>), SeamError>>
           + Send;
    /// `_emit_skipped` (`tasks.py:392-403`): no-op for an empty list.
    fn emit_skipped(
        &self,
        ctx: &TurnContext,
        servers: &[SkippedServer],
    ) -> impl std::future::Future<Output = Result<(), SeamError>> + Send;
    /// `_report_runtime_tool_failures` (`tasks.py:406-419`): servers that
    /// died mid-run (connection happens on toolset entry).
    fn runtime_failures(
        &self,
        toolsets: &[ToolsetHandle],
    ) -> impl std::future::Future<Output = Result<Vec<SkippedServer>, SeamError>> + Send;
    /// `history.load_history` (`tasks.py:327`).
    fn load_history(
        &self,
        ctx: &TurnContext,
    ) -> impl std::future::Future<Output = Result<Value, SeamError>> + Send;
    /// `_model_label` (`tasks.py:329`).
    fn model_label(
        &self,
        ctx: &TurnContext,
    ) -> impl std::future::Future<Output = Result<String, SeamError>> + Send;
    /// Open the agent stream (`assistant.run(...)`, `tasks.py:337-345`).
    /// An [`AgentError::UsageLimit`] here takes the same `iteration_limit`
    /// path as one raised mid-stream.
    fn open_stream(
        &self,
        ctx: &TurnContext,
        history: &Value,
        toolsets: &[ToolsetHandle],
    ) -> impl std::future::Future<Output = Result<Self::Stream, AgentError>> + Send;
    /// Next stream item; `None` ends the stream. May raise
    /// [`AgentError`] like the `async for` body (`tasks.py:346-361`).
    fn next_event(
        &self,
        stream: &mut Self::Stream,
    ) -> impl std::future::Future<Output = Result<Option<StreamItem>, AgentError>> + Send;
    /// Monotonic milliseconds for the flush rule (stands in for
    /// `asyncio.get_running_loop().time()`).
    fn now_ms(&self) -> u64;
    /// `_start_assistant_row` (`tasks.py:118-129`): returns the new row id.
    fn start_row(
        &self,
        ctx: &TurnContext,
    ) -> impl std::future::Future<Output = Result<Uuid, SeamError>> + Send;
    /// `_emit_delta` (`tasks.py:132-139`).
    fn emit_delta(
        &self,
        ctx: &TurnContext,
        message_id: Uuid,
        chunk: &str,
    ) -> impl std::future::Future<Output = Result<(), SeamError>> + Send;
    /// `_finalize_row` (`tasks.py:142-154`).
    fn finalize_row(
        &self,
        ctx: &TurnContext,
        message_id: Uuid,
        text: &str,
        status: &'static str,
    ) -> impl std::future::Future<Output = Result<(), SeamError>> + Send;
    /// `history.dump_new_messages` (`tasks.py:365`).
    fn dump_messages(
        &self,
        stream: &Self::Stream,
    ) -> impl std::future::Future<Output = Result<Value, SeamError>> + Send;
    /// Usage extraction (`tasks.py:366`, `_extract_usage`). Return `None`
    /// only when reading raises (stores `{}`); when the provider yields no
    /// usage object at all, return `Some(TurnUsage::default())` so the row
    /// keeps the null-counters dict Python's `getattr(u, ..., None)` copy
    /// produces.
    fn extract_usage(
        &self,
        stream: &Self::Stream,
    ) -> impl std::future::Future<Output = Result<Option<TurnUsage>, SeamError>> + Send;
    /// `_complete_turn` (`tasks.py:157-170`), including the delta prune
    /// (`:170`).
    fn complete_turn(
        &self,
        ctx: &TurnContext,
        model_messages: Value,
        usage: Value,
        model_used: String,
    ) -> impl std::future::Future<Output = Result<(), SeamError>> + Send;
    /// `_fail_turn` (`tasks.py:181-201`): truncates code/detail first,
    /// including the error row, the event and the delta prune (`:191-201`).
    fn fail_turn(
        &self,
        ctx: &TurnContext,
        code: &str,
        detail: &str,
    ) -> impl std::future::Future<Output = Result<(), SeamError>> + Send;
    /// `_cancel_turn` (`tasks.py:204-213`), including the delta prune
    /// (`:213`).
    fn cancel_turn(
        &self,
        ctx: &TurnContext,
    ) -> impl std::future::Future<Output = Result<(), SeamError>> + Send;
}

/// How one turn ended. Variants mirror the `_run_turn` exits
/// (`tasks.py:308-367`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DriveResult {
    /// No eligible context (`tasks.py:308-310`), or the claim lost
    /// (`tasks.py:311-312`): silent return.
    Noop,
    /// Model resolution raised `AssistantError` (`tasks.py:316-318`).
    ModelFailed { code: String, detail: String },
    /// Stream cancelled (`tasks.py:346-350`).
    Cancelled,
    /// Stream finished (`tasks.py:363-367`).
    Completed,
    /// `UsageLimitExceeded` or provider error (`tasks.py:351-361`).
    Failed { code: String, detail: String },
}

/// Run one turn to a terminal state (`_run_turn`, `tasks.py:299-367`).
///
/// Step order is the Python one: load → claim → model → toolsets (degraded)
/// → history → label → stream with per-event cancel checks → runtime-failure
/// report on every exit → terminal write. The runtime-failure report runs on
/// the success path too (`tasks.py:363`): a turn that survived a dying tool
/// server is exactly when the user must hear about it.
pub async fn drive_turn<S: TurnSeam>(seam: &S, turn_id: Uuid) -> Result<DriveResult, SeamError> {
    let ctx = match seam.load_context(turn_id).await? {
        Some(ctx) => ctx,
        None => return Ok(DriveResult::Noop),
    };
    if !seam.mark_running(&ctx).await? {
        return Ok(DriveResult::Noop);
    }

    if let Err(err) = seam.resolve_model(&ctx).await {
        seam.fail_turn(&ctx, &err.code, &err.detail).await?;
        return Ok(DriveResult::ModelFailed {
            code: err.code,
            detail: err.detail,
        });
    }

    let toolsets_outcome = seam.resolve_toolsets(&ctx).await;
    let (toolsets, skipped) = match toolsets_outcome {
        Ok((t, s)) => (t, s),
        Err(_) => resolve_toolsets_outcome(Err(())),
    };
    if skipped_payload(&skipped).is_some() {
        emit_best_effort(seam, &ctx, &skipped).await;
    }

    let history = seam.load_history(&ctx).await?;
    let model_label = seam.model_label(&ctx).await?;

    let mut open_row: Option<Uuid> = None;
    let mut stream = match seam.open_stream(&ctx, &history, &toolsets).await {
        Ok(stream) => stream,
        Err(err) => {
            return stream_failed(seam, &ctx, &toolsets, None, &mut open_row, err).await;
        }
    };

    let mut sink = StreamSink::new();
    loop {
        let item = match seam.next_event(&mut stream).await {
            Ok(Some(item)) => item,
            Ok(None) => break,
            Err(err) => {
                // `streamer.fail_open_row(FAILED)` (`tasks.py:353,358`):
                // close the open row through the tracked id, then fail.
                let fail_action = sink.fail_open(MSG_FAILED);
                return stream_failed(seam, &ctx, &toolsets, fail_action, &mut open_row, err).await;
            }
        };
        // The cancel check runs only when an event is delivered, never on
        // stream end (`tasks.py:231-233`: the `async for` body). A cancel
        // that lands after the last event — or before a stream that yields
        // nothing — is ignored and the turn completes.
        if !matches!(item, StreamItem::End) && seam.is_cancelled(ctx.turn_id).await? {
            report_runtime_failures(seam, &ctx, &toolsets).await?;
            if let Some(action) = sink.fail_open(MSG_CANCELLED) {
                perform(seam, &ctx, &mut open_row, &action).await?;
            }
            seam.cancel_turn(&ctx).await?;
            return Ok(DriveResult::Cancelled);
        }
        for action in sink.handle(&item, seam.now_ms()) {
            perform(seam, &ctx, &mut open_row, &action).await?;
        }
        if matches!(item, StreamItem::End) {
            break;
        }
    }

    let failed = seam.runtime_failures(&toolsets).await?;
    if skipped_payload(&failed).is_some() {
        seam.emit_skipped(&ctx, &failed).await?;
    }

    let model_messages = seam.dump_messages(&stream).await?;
    let usage = usage_value(seam.extract_usage(&stream).await?.as_ref());
    seam.complete_turn(&ctx, model_messages, usage, model_label)
        .await?;
    Ok(DriveResult::Completed)
}

/// Shared failure exit (`tasks.py:351-361`): report runtime tool failures,
/// fail the open row (pre-computed by the caller through its tracked row
/// id), classify, fail the turn. `fail_action` is `None` when the stream
/// never opened (nothing to fail) or no row was open.
async fn stream_failed<S: TurnSeam>(
    seam: &S,
    ctx: &TurnContext,
    toolsets: &[ToolsetHandle],
    fail_action: Option<SinkAction>,
    open_row: &mut Option<Uuid>,
    err: AgentError,
) -> Result<DriveResult, SeamError> {
    report_runtime_failures(seam, ctx, toolsets).await?;
    if let Some(action) = fail_action {
        perform(seam, ctx, open_row, &action).await?;
    }
    let (code, detail) = match err {
        AgentError::UsageLimit(detail) => (CODE_ITERATION_LIMIT.to_owned(), detail),
        AgentError::Provider {
            message,
            status_code,
        } => {
            let (code, detail) = classify_error(&message, status_code);
            (code.to_owned(), detail.to_owned())
        }
    };
    seam.fail_turn(ctx, &code, &detail).await?;
    Ok(DriveResult::Failed { code, detail })
}

/// Report runtime tool failures, emitting only when non-empty
/// (`tasks.py:347,352,357,363` via `_report_runtime_tool_failures`).
///
/// Best-effort like `_emit_skipped` (`tasks.py:402-403`): a notification
/// failure must not fail the turn, so read/emit errors are logged and the
/// turn continues. (The Python read is infallible in-memory attr access; a
/// seam that must read across a boundary degrades to "nothing failed".)
async fn report_runtime_failures<S: TurnSeam>(
    seam: &S,
    ctx: &TurnContext,
    toolsets: &[ToolsetHandle],
) -> Result<(), SeamError> {
    let failed = match seam.runtime_failures(toolsets).await {
        Ok(failed) => failed,
        Err(error) => {
            tracing::warn!("assistant turn: runtime toolset read failed: {error}");
            Vec::new()
        }
    };
    if skipped_payload(&failed).is_some() {
        emit_best_effort(seam, ctx, &failed).await;
    }
    Ok(())
}

/// Best-effort skipped-servers emit (`_emit_skipped`, `tasks.py:392-403`).
async fn emit_best_effort<S: TurnSeam>(seam: &S, ctx: &TurnContext, servers: &[SkippedServer]) {
    if let Err(error) = seam.emit_skipped(ctx, servers).await {
        tracing::warn!("assistant turn: tool_servers_skipped emit failed: {error}");
    }
}

/// Execute one sink action, tracking the open row id for later `fail_open`.
async fn perform<S: TurnSeam>(
    seam: &S,
    ctx: &TurnContext,
    open_row: &mut Option<Uuid>,
    action: &SinkAction,
) -> Result<(), SeamError> {
    match action {
        SinkAction::CreateRow => {
            let id = seam.start_row(ctx).await?;
            *open_row = Some(id);
            Ok(())
        }
        SinkAction::EmitDelta { chunk } => {
            if let Some(id) = *open_row {
                seam.emit_delta(ctx, id, chunk).await?;
            }
            Ok(())
        }
        SinkAction::FinalizeRow { text, status } => {
            if let Some(id) = open_row.take() {
                seam.finalize_row(ctx, id, text, status).await?;
            }
            Ok(())
        }
    }
}

// --------------------------------------------------------------------------- //
// Wire parsing + registration
// --------------------------------------------------------------------------- //

/// Parse the `run_assistant_turn.delay(str(turn_id))` payload
/// (`tasks.py:483`, `args=[str(turn_id)]`): `args[0]` must be a UUID string.
/// Anything else is a poison message — park it, never retry it (retries are
/// disabled for this task).
pub fn parse_run_turn_arg(job: &JobRow) -> Result<Uuid, String> {
    let args = job
        .args
        .as_array()
        .ok_or_else(|| format!("{RUN_TURN_TASK}: args is not an array: {}", job.args))?;
    let first = args
        .first()
        .ok_or_else(|| format!("{RUN_TURN_TASK}: args is empty"))?;
    let raw = first
        .as_str()
        .ok_or_else(|| format!("{RUN_TURN_TASK}: args[0] is not a string: {first}"))?;
    Uuid::parse_str(raw).map_err(|e| format!("{RUN_TURN_TASK}: args[0] is not a UUID ({raw}): {e}"))
}

/// Register the local `assistant.run_turn` handler.
///
/// The handler drives [`drive_turn`] with the given seam and pool; the pool
/// is captured for the seam implementor's own queries (the handler future
/// receives only the claimed row). Infra failures park the row
/// ([`Verdict::Fail`]) — never `Retry`: Celery retries are disabled for the
/// turn task, and a crashed turn is recovered by the sweep.
///
/// Like the mail tasks, this builds the handler table without flipping the
/// live worker: the domain gate flips ownership after the proxy pass, so
/// until then `assistant.run_turn` still routes to `PythonOwned` (see
/// [`crate::worker::route_for`]).
pub fn register_turn_handler<S: TurnSeam + 'static>(
    registry: &mut Registry,
    _pool: PgPool,
    seam: Arc<S>,
) {
    registry.register(
        RUN_TURN_TASK,
        Arc::new(move |job: JobRow| {
            let seam = seam.clone();
            let fut: Pin<Box<dyn Future<Output = Result<Verdict, HandlerError>> + Send>> =
                Box::pin(async move {
                    let turn_id = match parse_run_turn_arg(&job) {
                        Ok(id) => id,
                        Err(error) => return Ok(Verdict::Fail { error }),
                    };
                    match drive_turn(seam.as_ref(), turn_id).await {
                        Ok(_) => Ok(Verdict::Ack),
                        Err(error) => Ok(Verdict::Fail {
                            error: error.to_string(),
                        }),
                    }
                });
            fut
        }),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    /// F-A6-10 (`rust-api/fixtures/assistant/tools-tasks.json`, `tasks` key).
    fn tasks_fixture() -> Value {
        let path = format!(
            "{}/../../fixtures/assistant/tools-tasks.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(&path).expect("fixture exists"))
            .expect("fixture parses")
    }

    #[test]
    fn task_name_and_limits_match_python() {
        let fx = tasks_fixture();
        let celery = &fx["tasks"]["celery"];
        assert_eq!(RUN_TURN_TASK, "assistant.run_turn");
        assert!(celery["run_assistant_turn"]
            .as_str()
            .unwrap()
            .contains("assistant.run_turn"));
        assert_eq!(TURN_SOFT_LIMIT_SECS, 300);
        assert_eq!(TURN_HARD_LIMIT_SECS, 330);
        assert_eq!(fx["tasks"]["turn_limits"]["soft"], json!(300));
        assert_eq!(fx["tasks"]["turn_limits"]["hard"], json!(330));
        assert_eq!(DELTA_FLUSH_MS, 100);
        assert_eq!(fx["tasks"]["turn_limits"]["delta_flush_ms"], json!(100));
        assert_eq!(REQUEST_LIMIT, 25);
        assert_eq!(TOOL_CALLS_LIMIT, 20);
    }

    #[test]
    fn cancel_key_format() {
        let id = Uuid::parse_str("12345678-1234-5678-1234-567812345678").unwrap();
        assert_eq!(
            cancel_key(&id),
            "assistant:cancel:12345678-1234-5678-1234-567812345678"
        );
        // Fixture vector (`tasks.cancel_key`): `assistant:cancel:abc`.
        let fx = tasks_fixture();
        assert_eq!(
            fx["tasks"]["cancel_key"]["vector"],
            json!("assistant:cancel:abc")
        );
    }

    #[test]
    fn truncation_is_char_based_never_panics() {
        assert_eq!(
            truncate_code("provider_auth_failed"),
            "provider_auth_failed"
        );
        let long = "e".repeat(100);
        assert_eq!(truncate_code(&long).chars().count(), 64);
        let detail = "d".repeat(3000);
        assert_eq!(truncate_detail(&detail).chars().count(), 2000);
        // Multi-byte: must not panic and must not split a char (the
        // semantic-trap case Python `[:64]` handles by code points).
        let emoji = "é".repeat(100);
        let cut = truncate_code(&emoji);
        assert!(cut.chars().count() <= 64);
        assert!(cut.is_char_boundary(cut.len()));
        assert_eq!(truncate_chars("abc", 64), "abc");
    }

    #[test]
    fn classify_error_matches_fixture_vectors() {
        let fx = tasks_fixture();
        let vectors = fx["tasks"]["classify_error"]["vectors"]
            .as_object()
            .expect("vectors map");
        // Every fixture vector pins its (code, detail) pair.
        for (name, pair) in vectors {
            let (code, detail) = match name.as_str() {
                "payment_required" => classify_error("Payment Required: top up please", None),
                "status_402" => classify_error("anything at all", Some(402)),
                "auth_401" => classify_error("401 Unauthorized", None),
                "connection" => classify_error("could not connect: timed out", None),
                "model_not_found" => classify_error("model_not_found: no such model", None),
                "other" => classify_error("weird boom", None),
                other => panic!("unknown fixture vector {other}"),
            };
            assert_eq!(code, pair[0].as_str().unwrap(), "code for {name}");
            assert_eq!(detail, pair[1].as_str().unwrap(), "detail for {name}");
        }
        // The 402 status wins even when the message is unrelated: the code
        // is read from the exception, never substring-matched
        // (`tasks.py:449-453`). A bare "402" in text alone must NOT route
        // to credit.
        assert_eq!(
            classify_error("request id 402 token count 402", None).0,
            CODE_INTERNAL
        );
        assert_eq!(
            classify_error("request id 402 token count 402", Some(402)).0,
            CODE_OUT_OF_CREDIT
        );
        // Credit beats auth when both markers are present (branch order).
        assert_eq!(
            classify_error("401 payment required", None).0,
            CODE_OUT_OF_CREDIT
        );
    }

    #[test]
    fn usage_shape_nulls_and_empty() {
        let u = TurnUsage {
            input_tokens: Some(10),
            output_tokens: None,
            total_tokens: Some(10),
            requests: Some(1),
            tool_calls: None,
        };
        let v = usage_value(Some(&u));
        assert_eq!(
            v,
            json!({
                "input_tokens": 10,
                "output_tokens": null,
                "total_tokens": 10,
                "requests": 1,
                "tool_calls": null,
            })
        );
        // Extraction failure stores `{}`, never null (`tasks.py:431-432`).
        assert_eq!(usage_value(None), json!({}));
    }

    #[test]
    fn toolset_degradation_and_skipped_payload() {
        // Total resolver failure → empty tools + all-servers entry.
        let (tools, skipped) = resolve_toolsets_outcome(Err(()));
        assert!(tools.is_empty());
        assert_eq!(
            skipped,
            vec![SkippedServer {
                name: ALL_TOOL_SERVERS_NAME.to_owned(),
                reason: TOOLSETS_UNAVAILABLE_REASON.to_owned(),
            }]
        );
        // Partial failure passes through untouched.
        let partial = vec![SkippedServer {
            name: "s".to_owned(),
            reason: "r".to_owned(),
        }];
        let (tools, skipped) =
            resolve_toolsets_outcome(Ok((vec!["t".to_owned()], partial.clone())));
        assert_eq!(tools, vec!["t".to_owned()]);
        assert_eq!(skipped, partial);
        // Empty skipped list emits nothing (`tasks.py:393-394`).
        assert_eq!(skipped_payload(&[]), None);
        assert_eq!(
            skipped_payload(&partial),
            Some(json!({ "servers": [{"name": "s", "reason": "r"}] }))
        );
    }

    #[test]
    fn eligibility_gate() {
        assert!(context_eligible(TURN_QUEUED));
        assert!(context_eligible(TURN_RUNNING));
        for s in [TURN_COMPLETED, TURN_FAILED, TURN_CANCELLED, "bogus", ""] {
            assert!(!context_eligible(s), "{s} must not load");
        }
    }

    #[test]
    fn sql_shapes_match_python_semantics() {
        let load = load_context_sql();
        for table in [
            "assistant_turn",
            "assistant_thread",
            "workspaces",
            "assistant_message",
        ] {
            assert!(load.contains(table), "load selects {table}");
        }
        assert!(
            load.contains("INNER JOIN"),
            "thread+workspace are inner joins"
        );
        assert!(load.contains("LEFT JOIN"), "user_message is a left join");
        assert!(load.contains("t.\"id\" = $1"), "where on turn id");
        assert!(load.contains("ORDER BY"), "created_at ordering");

        let mark = mark_running_update();
        assert!(
            mark.contains("status = 'running'"),
            "mark sets running: {mark}"
        );
        assert!(
            mark.contains("started_at = NOW()"),
            "mark stamps start: {mark}"
        );
        assert!(
            mark.contains("status = 'queued'"),
            "mark is conditional: {mark}"
        );

        assert!(lock_turn_select().contains("FOR UPDATE"));

        let complete = complete_turn_update();
        for col in ["model_messages", "usage", "model_used", "completed_at"] {
            assert!(complete.contains(col), "complete sets {col}");
        }
        let clear = clear_active_turn_update();
        assert!(clear.contains("active_turn_id = NULL"));
        assert!(
            clear.contains("active_turn_id = $2"),
            "clears only when matching"
        );

        let fail = fail_turn_update();
        for col in ["error_code", "error_detail", "completed_at"] {
            assert!(fail.contains(col), "fail sets {col}");
        }
        // Cancel writes status only — no error columns (`tasks.py:204-209`).
        let cancel = cancel_turn_update();
        assert!(cancel.contains("status = 'cancelled'"));
        assert!(
            !cancel.contains("error_code"),
            "cancel keeps error columns: {cancel}"
        );

        let fin = finalize_open_rows_update();
        assert!(fin.contains("status = $2"));
        assert!(fin.contains("status = 'streaming'"));

        let err = error_message_insert();
        assert!(err.contains("'error'") && err.contains("'failed'"));
        assert!(
            err.contains("COALESCE(MAX(seq), 0) + 1"),
            "seq alloc: {err}"
        );

        let ev = turn_failed_event_insert();
        assert!(ev.contains("'turn_failed'"));
        assert!(ev.contains("COALESCE(MAX(seq), 0) + 1"), "seq alloc: {ev}");

        assert!(thread_lock_select().contains("FOR UPDATE"));

        let prune = prune_turn_deltas_delete();
        assert!(prune.contains("assistant_event") && prune.contains("'assistant_delta'"));
    }

    #[test]
    fn wire_arg_parsing() {
        let good = Uuid::new_v4();
        let job = JobRow {
            id: 1,
            celery_id: "c".to_owned(),
            task: RUN_TURN_TASK.to_owned(),
            args: json!([good.to_string()]),
            kwargs: json!({}),
            queue: "celery".to_owned(),
            status: "queued".to_owned(),
            attempts: 0,
            max_retries: 0,
            visible_at: chrono::Utc::now(),
            claimed_at: None,
            claimed_by: None,
            created_at: chrono::Utc::now(),
            last_error: None,
        };
        assert_eq!(parse_run_turn_arg(&job), Ok(good));

        let mut bad = job.clone();
        bad.args = json!(["not-a-uuid"]);
        assert!(parse_run_turn_arg(&bad).is_err());
        bad.args = json!([]);
        assert!(parse_run_turn_arg(&bad).is_err());
        bad.args = json!({});
        assert!(parse_run_turn_arg(&bad).is_err());
    }

    // -- drive_turn ------------------------------------------------------- //

    use std::collections::VecDeque;
    use std::sync::Mutex;

    fn test_ctx() -> TurnContext {
        TurnContext {
            turn_id: Uuid::new_v4(),
            turn_status: TURN_QUEUED.to_owned(),
            thread_id: Uuid::new_v4(),
            thread_kind: "chat".to_owned(),
            workspace_id: Uuid::new_v4(),
            workspace_slug: "ws".to_owned(),
            workspace_name: "WS".to_owned(),
            workspace_role: 20,
            user_id: Uuid::new_v4(),
            user_display: "u".to_owned(),
            user_text: "hi".to_owned(),
        }
    }

    /// Scripted seam: canned answers, recorded calls. The script drives the
    /// agent stream; `cancel_after` flips `is_cancelled` after that many
    /// successful `next_event` pulls.
    struct FakeSeam {
        ctx: Option<TurnContext>,
        claim: bool,
        model: Result<String, ModelError>,
        toolsets: Result<(Vec<ToolsetHandle>, Vec<SkippedServer>), SeamError>,
        runtime_failed: Vec<SkippedServer>,
        script: Mutex<VecDeque<Result<Option<StreamItem>, AgentError>>>,
        open_err: Option<AgentError>,
        cancel_after: Option<usize>,
        pulls: Mutex<usize>,
        usage: Option<TurnUsage>,
        fail_emit: bool,
        calls: Mutex<Vec<String>>,
    }

    impl FakeSeam {
        fn ok(script: Vec<StreamItem>) -> Self {
            Self {
                ctx: Some(test_ctx()),
                claim: true,
                model: Ok("m".to_owned()),
                toolsets: Ok((Vec::new(), Vec::new())),
                runtime_failed: Vec::new(),
                script: Mutex::new(script.into_iter().map(Ok).map(|r| r.map(Some)).collect()),
                open_err: None,
                cancel_after: None,
                pulls: Mutex::new(0),
                usage: Some(TurnUsage {
                    input_tokens: Some(1),
                    output_tokens: Some(2),
                    total_tokens: Some(3),
                    requests: Some(1),
                    tool_calls: Some(0),
                }),
                fail_emit: false,
                calls: Mutex::new(Vec::new()),
            }
        }

        fn log(&self, entry: String) {
            self.calls.lock().unwrap().push(entry);
        }

        fn saw(&self, prefix: &str) -> Vec<String> {
            self.calls
                .lock()
                .unwrap()
                .iter()
                .filter(|c| c.starts_with(prefix))
                .cloned()
                .collect()
        }

        fn take_calls(&self) -> Vec<String> {
            std::mem::take(&mut *self.calls.lock().unwrap())
        }
    }

    struct FakeStream;

    impl TurnSeam for FakeSeam {
        type Stream = FakeStream;

        async fn load_context(&self, _t: Uuid) -> Result<Option<TurnContext>, SeamError> {
            self.log("load_context".to_owned());
            Ok(self.ctx.clone())
        }
        async fn mark_running(&self, _c: &TurnContext) -> Result<bool, SeamError> {
            self.log("mark_running".to_owned());
            Ok(self.claim)
        }
        async fn is_cancelled(&self, _t: Uuid) -> Result<bool, SeamError> {
            match self.cancel_after {
                Some(n) => Ok(*self.pulls.lock().unwrap() >= n),
                None => Ok(false),
            }
        }
        async fn resolve_model(&self, _c: &TurnContext) -> Result<String, ModelError> {
            self.log("resolve_model".to_owned());
            self.model.clone()
        }
        async fn resolve_toolsets(
            &self,
            _c: &TurnContext,
        ) -> Result<(Vec<ToolsetHandle>, Vec<SkippedServer>), SeamError> {
            self.log("resolve_toolsets".to_owned());
            self.toolsets.clone()
        }
        async fn emit_skipped(
            &self,
            _c: &TurnContext,
            servers: &[SkippedServer],
        ) -> Result<(), SeamError> {
            self.log(format!(
                "emit_skipped:{}",
                servers
                    .iter()
                    .map(|s| s.name.clone())
                    .collect::<Vec<_>>()
                    .join(",")
            ));
            if self.fail_emit {
                return Err(SeamError("emit down".to_owned()));
            }
            Ok(())
        }
        async fn runtime_failures(
            &self,
            _t: &[ToolsetHandle],
        ) -> Result<Vec<SkippedServer>, SeamError> {
            self.log("runtime_failures".to_owned());
            Ok(self.runtime_failed.clone())
        }
        async fn load_history(&self, _c: &TurnContext) -> Result<Value, SeamError> {
            Ok(json!([]))
        }
        async fn model_label(&self, _c: &TurnContext) -> Result<String, SeamError> {
            Ok("label".to_owned())
        }
        async fn open_stream(
            &self,
            _c: &TurnContext,
            _h: &Value,
            _t: &[ToolsetHandle],
        ) -> Result<FakeStream, AgentError> {
            self.log("open_stream".to_owned());
            match &self.open_err {
                Some(e) => Err(e.clone()),
                None => Ok(FakeStream),
            }
        }
        async fn next_event(&self, _s: &mut FakeStream) -> Result<Option<StreamItem>, AgentError> {
            *self.pulls.lock().unwrap() += 1;
            self.script.lock().unwrap().pop_front().unwrap_or(Ok(None))
        }
        fn now_ms(&self) -> u64 {
            // Each pull advances the clock past the flush cadence so every
            // delta flushes deterministically.
            (*self.pulls.lock().unwrap() as u64) * (DELTA_FLUSH_MS + 1)
        }
        async fn start_row(&self, _c: &TurnContext) -> Result<Uuid, SeamError> {
            let id = Uuid::new_v4();
            self.log(format!("start_row:{id}"));
            Ok(id)
        }
        async fn emit_delta(
            &self,
            _c: &TurnContext,
            _m: Uuid,
            chunk: &str,
        ) -> Result<(), SeamError> {
            self.log(format!("emit_delta:{chunk}"));
            Ok(())
        }
        async fn finalize_row(
            &self,
            _c: &TurnContext,
            _m: Uuid,
            text: &str,
            status: &'static str,
        ) -> Result<(), SeamError> {
            self.log(format!("finalize_row:{status}:{text}"));
            Ok(())
        }
        async fn dump_messages(&self, _s: &FakeStream) -> Result<Value, SeamError> {
            Ok(json!([{"role": "assistant"}]))
        }
        async fn extract_usage(&self, _s: &FakeStream) -> Result<Option<TurnUsage>, SeamError> {
            Ok(self.usage.clone())
        }
        async fn complete_turn(
            &self,
            _c: &TurnContext,
            _m: Value,
            usage: Value,
            model_used: String,
        ) -> Result<(), SeamError> {
            self.log(format!(
                "complete_turn:{model_used}:{}",
                serde_json::to_string(&usage).unwrap()
            ));
            Ok(())
        }
        async fn fail_turn(
            &self,
            _c: &TurnContext,
            code: &str,
            detail: &str,
        ) -> Result<(), SeamError> {
            self.log(format!("fail_turn:{code}:{detail}"));
            Ok(())
        }
        async fn cancel_turn(&self, _c: &TurnContext) -> Result<(), SeamError> {
            self.log("cancel_turn".to_owned());
            Ok(())
        }
    }

    #[tokio::test]
    async fn drive_noop_paths() {
        // Missing/ineligible context: silent return, nothing else runs.
        let mut seam = FakeSeam::ok(vec![]);
        seam.ctx = None;
        let out = drive_turn(&seam, Uuid::new_v4()).await.unwrap();
        assert_eq!(out, DriveResult::Noop);
        assert_eq!(seam.take_calls().as_slice(), ["load_context"]);

        // Lost claim: same silence after mark_running.
        let mut seam = FakeSeam::ok(vec![]);
        seam.claim = false;
        let out = drive_turn(&seam, Uuid::new_v4()).await.unwrap();
        assert_eq!(out, DriveResult::Noop);
        assert_eq!(
            seam.take_calls().as_slice(),
            ["load_context", "mark_running"]
        );
    }

    #[tokio::test]
    async fn drive_model_failure() {
        let mut seam = FakeSeam::ok(vec![]);
        seam.model = Err(ModelError {
            code: "llm_config_missing".to_owned(),
            detail: "Configure your AI provider in Settings.".to_owned(),
        });
        let out = drive_turn(&seam, Uuid::new_v4()).await.unwrap();
        assert_eq!(
            out,
            DriveResult::ModelFailed {
                code: "llm_config_missing".to_owned(),
                detail: "Configure your AI provider in Settings.".to_owned(),
            }
        );
        assert!(seam.saw("fail_turn:llm_config_missing").len() == 1);
        assert!(seam.saw("open_stream").is_empty(), "never streams");
    }

    #[tokio::test]
    async fn drive_success_streams_and_completes() {
        let seam = FakeSeam::ok(vec![
            StreamItem::TextStart {
                initial: "Hel".to_owned(),
            },
            StreamItem::TextDelta {
                chunk: "lo".to_owned(),
            },
            StreamItem::End,
        ]);
        let out = drive_turn(&seam, Uuid::new_v4()).await.unwrap();
        assert_eq!(out, DriveResult::Completed);
        assert_eq!(seam.saw("start_row").len(), 1);
        let deltas = seam.saw("emit_delta");
        assert!(
            deltas.iter().any(|d| d.contains("Hel")),
            "initial flushes: {deltas:?}"
        );
        assert!(seam.saw("finalize_row:completed:Hello").len() == 1);
        let done = seam.saw("complete_turn");
        assert_eq!(done.len(), 1);
        assert!(
            done[0].contains("\"input_tokens\":1"),
            "usage stored: {}",
            done[0]
        );
        assert!(
            done[0].starts_with("complete_turn:label:"),
            "label stored: {}",
            done[0]
        );
        // Runtime-failure report runs on the success path too.
        assert_eq!(seam.saw("runtime_failures").len(), 1);
        assert!(
            seam.saw("emit_skipped").is_empty(),
            "nothing failed, nothing emitted"
        );
    }

    #[tokio::test]
    async fn drive_degraded_toolsets_still_completes() {
        let mut seam = FakeSeam::ok(vec![StreamItem::End]);
        seam.toolsets = Err(SeamError("boom".to_owned()));
        let out = drive_turn(&seam, Uuid::new_v4()).await.unwrap();
        assert_eq!(out, DriveResult::Completed);
        assert_eq!(seam.saw("emit_skipped:all tool servers").len(), 1);
    }

    #[tokio::test]
    async fn drive_skipped_emit_failure_does_not_fail_turn() {
        // `_emit_skipped` swallows notification failures (`tasks.py:402-403`):
        // the turn still completes.
        let mut seam = FakeSeam::ok(vec![StreamItem::End]);
        seam.toolsets = Err(SeamError("boom".to_owned()));
        seam.fail_emit = true;
        let out = drive_turn(&seam, Uuid::new_v4()).await.unwrap();
        assert_eq!(out, DriveResult::Completed);
        assert_eq!(seam.saw("emit_skipped:all tool servers").len(), 1);
        assert_eq!(seam.saw("complete_turn").len(), 1);
    }

    #[tokio::test]
    async fn drive_preset_cancel_with_empty_stream_completes() {
        // Python checks cancel only on delivered events (`tasks.py:231-233`):
        // a pre-set key with an eventless stream still completes.
        let mut seam = FakeSeam::ok(vec![StreamItem::End]);
        seam.cancel_after = Some(0);
        let out = drive_turn(&seam, Uuid::new_v4()).await.unwrap();
        assert_eq!(out, DriveResult::Completed);
        assert!(seam.saw("cancel_turn").is_empty());
    }

    #[tokio::test]
    async fn drive_cancel_closes_row_cancelled() {
        let mut seam = FakeSeam::ok(vec![
            StreamItem::TextStart {
                initial: "part".to_owned(),
            },
            StreamItem::TextDelta {
                chunk: "ial".to_owned(),
            },
            StreamItem::End,
        ]);
        // Cancel lands after the first event is processed (the check runs on
        // the delivered-but-unprocessed event, `tasks.py:231-233`): the open
        // row fails as cancelled, then the turn cancels.
        seam.cancel_after = Some(2);
        let out = drive_turn(&seam, Uuid::new_v4()).await.unwrap();
        assert_eq!(out, DriveResult::Cancelled);
        assert!(seam.saw("finalize_row:cancelled:").len() == 1);
        assert_eq!(seam.saw("cancel_turn").len(), 1);
        assert!(seam.saw("complete_turn").is_empty());
    }

    #[tokio::test]
    async fn drive_usage_limit_and_provider_errors() {
        // UsageLimit on open → iteration_limit, no row work.
        let mut seam = FakeSeam::ok(vec![]);
        seam.open_err = Some(AgentError::UsageLimit("25 requests".to_owned()));
        let out = drive_turn(&seam, Uuid::new_v4()).await.unwrap();
        assert_eq!(
            out,
            DriveResult::Failed {
                code: CODE_ITERATION_LIMIT.to_owned(),
                detail: "25 requests".to_owned(),
            }
        );
        assert!(seam.saw("fail_turn:iteration_limit:25 requests").len() == 1);

        // Provider error mid-stream → classified, open row failed.
        let seam = FakeSeam::ok(vec![StreamItem::TextStart {
            initial: "hi".to_owned(),
        }]);
        seam.script
            .lock()
            .unwrap()
            .push_back(Err(AgentError::Provider {
                message: "could not connect".to_owned(),
                status_code: None,
            }));
        let out = drive_turn(&seam, Uuid::new_v4()).await.unwrap();
        assert_eq!(
            out,
            DriveResult::Failed {
                code: CODE_UNREACHABLE.to_owned(),
                detail: DETAIL_UNREACHABLE.to_owned(),
            }
        );
        assert!(seam.saw("finalize_row:failed:hi").len() == 1);
        assert!(seam.saw("fail_turn:provider_unreachable").len() == 1);
    }

    #[tokio::test]
    async fn drive_runtime_failures_reported_on_failure() {
        let mut seam = FakeSeam::ok(vec![]);
        seam.open_err = Some(AgentError::Provider {
            message: "x".to_owned(),
            status_code: None,
        });
        seam.runtime_failed = vec![SkippedServer {
            name: "dead".to_owned(),
            reason: "boom".to_owned(),
        }];
        let out = drive_turn(&seam, Uuid::new_v4()).await.unwrap();
        assert!(matches!(out, DriveResult::Failed { .. }));
        assert_eq!(seam.saw("emit_skipped:dead").len(), 1);
    }

    #[test]
    fn sink_flush_rule_and_boundaries() {
        let mut sink = StreamSink::new();
        // Empty chunk: no-op, no row.
        assert!(sink
            .handle(
                &StreamItem::TextDelta {
                    chunk: "".to_owned()
                },
                0
            )
            .is_empty());
        assert!(!sink.is_open());
        // First chunk opens the row but does not flush yet (t=0).
        let out = sink.handle(
            &StreamItem::TextDelta {
                chunk: "a".to_owned(),
            },
            0,
        );
        assert_eq!(out, vec![SinkAction::CreateRow]);
        // 100ms later the pending char flushes.
        let out = sink.handle(
            &StreamItem::TextDelta {
                chunk: "b".to_owned(),
            },
            100,
        );
        assert_eq!(
            out,
            vec![SinkAction::EmitDelta {
                chunk: "ab".to_owned()
            }]
        );
        // 200 pending chars flush immediately regardless of the clock.
        let big = "x".repeat(200);
        let out = sink.handle(&StreamItem::TextDelta { chunk: big.clone() }, 101);
        assert_eq!(out, vec![SinkAction::EmitDelta { chunk: big.clone() }]);
        // Non-text boundary finalizes completed without opening a new row.
        let out = sink.handle(&StreamItem::NonTextBoundary, 101);
        assert_eq!(
            out,
            vec![SinkAction::FinalizeRow {
                text: "ab".to_owned() + &big,
                status: MSG_COMPLETED
            }]
        );
        assert!(!sink.is_open());
        // End with nothing open: silent.
        assert!(sink.handle(&StreamItem::End, 200).is_empty());
        assert!(sink.fail_open(MSG_FAILED).is_none());
    }
}
