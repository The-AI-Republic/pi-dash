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
//! - `_was_cut_off` / `_mark_cut_off` (`:369-372,502-530`) →
//!   [`TurnSeam::was_cut_off`] / [`TurnSeam::mark_cut_off`], called between
//!   usage extraction and completion exactly like Python.
//! - `_complete_turn` / `_fail_turn` / `_cancel_turn` (`:158-213`) →
//!   [`TurnSeam::complete_turn`] / [`fail_turn`] / [`cancel_turn`]; the exact
//!   `UPDATE`/`INSERT`/`DELETE` shapes are the `*_sql` builders here, pinned
//!   against the fixture's `updates` entries.
//! - `_finalize_open_rows` (`:173-178`) → `domain::assistant::finalize_open_messages`.
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
//! - `_classify_error` (`:447-486` on `origin/main`, past #1430) → the
//!   [`TurnSeam::turn_error_for_user`] hook with the
//!   [`classify_provider_error`] fallback: status-first wording (credit,
//!   auth, rate-limit, model, unreachable), text markers only for
//!   status-less failures.
//!   The typed-first branch (`:453-457`) is the [`AgentError::Typed`] arm:
//!   a typed failure crosses the seam already classified and is used
//!   verbatim, never text-guessed. The `_hit_output_limit` branch
//!   (`:489-499`) is the [`AgentError::OutputTruncated`] arm, mapping to
//!   `response_truncated` past the provider classifier. (Only this bullet
//!   tracks `origin/main`; the rest of this map tracks the frozen
//!   `tasks.py` until PIDASHCONV-1055 merges `main`.)
//! - `run_assistant_turn` (`:476-491`, `name="assistant.run_turn"`,
//!   `acks_late=False`, `max_retries=0`, soft/hard limits from settings) →
//!   [`RUN_TURN_TASK`] + [`register_turn_handler`]. Celery retries are
//!   disabled for the turn task, so the handler returns [`Verdict::Ack`] or
//!   [`Verdict::Fail`] — never `Retry`. The `SoftTimeLimitExceeded` arm
//!   (`:486-489`) is the stream-phase deadline in [`drive_turn`]: past the
//!   configured soft limit (300 at defaults) the stream drops (aborting the
//!   agent), the open row fails, and the turn fails with `turn_timeout`.
//!   There is no hard-kill equivalent — a timed-out turn ends in its fail
//!   write, and anything still `RUNNING` past the sweep cutoff is recovered
//!   by the sweep like any other crashed turn.
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
use std::time::Duration;

use sea_query::{Expr, JoinType, Order, PostgresQueryBuilder, Query};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

use crate::assistant::provider_errors::{classify_provider_error, BYOK};
use crate::queue::JobRow;
use crate::worker::{HandlerError, Registry, Verdict};

/// Celery task name (`tasks.py:477`, `@shared_task(name=...)`).
pub const RUN_TURN_TASK: &str = "assistant.run_turn";

/// Soft time limit default, seconds (`tasks.py:45`,
/// `ASSISTANT_TURN_SOFT_LIMIT` when unset).
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
// Error classification (`tasks.py:447-486` on `origin/main`, past #1430)
// --------------------------------------------------------------------------- //

/// Unrecognised-failure code: `_classify_error` returns `internal` when the
/// seam leaves a failure unclassified (`tasks.py:464-465`).
pub const CODE_INTERNAL: &str = "internal";

/// Iteration-limit code (`tasks.py:354`, `UsageLimitExceeded` branch).
pub const CODE_ITERATION_LIMIT: &str = "iteration_limit";
/// Soft-time-limit code (`tasks.py:489`, `SoftTimeLimitExceeded` branch).
pub const CODE_TURN_TIMEOUT: &str = "turn_timeout";
/// Output-limit code (`tasks.py:463`, `_hit_output_limit` branch).
pub const CODE_RESPONSE_TRUNCATED: &str = "response_truncated";

/// Unrecognised-failure detail, verbatim (`tasks.py:465`).
pub const DETAIL_INTERNAL: &str = "The assistant hit an unexpected error.";
/// Soft-time-limit detail, verbatim (`tasks.py:489`).
pub const DETAIL_TURN_TIMEOUT: &str = "The assistant took too long to respond.";
/// Output-limit detail, verbatim (`tasks.py:463`).
pub const DETAIL_RESPONSE_TRUNCATED: &str =
    "The reply was cut off because it reached the model's output limit.";

/// Word an untyped provider failure for the turn's user: the
/// [`TurnSeam::turn_error_for_user`] hook with the default-classifier
/// fallback (`tasks.py:447-486`).
///
/// A seam failure (an overlay hook raising) never fails the failure path:
/// like `_turn_error` it logs and falls back to
/// [`classify_provider_error`] with the BYOK connection. `None` from both
/// is the generic `internal` bucket. Typed failures never reach here:
/// `_classify_error` returns an `AssistantError`'s own code/detail first
/// (`tasks.py:453-457`), and the Rust equivalent is the
/// [`AgentError::Typed`] arm, used verbatim.
async fn turn_error_for_user<S: TurnSeam>(
    seam: &S,
    ctx: &TurnContext,
    message: &str,
    status_code: Option<u16>,
    body_detail: &str,
    model_name: &str,
) -> (String, String) {
    match seam
        .turn_error_for_user(ctx, message, status_code, body_detail, model_name)
        .await
    {
        Ok(Some((code, detail))) => (code, detail),
        Ok(None) => (CODE_INTERNAL.to_owned(), DETAIL_INTERNAL.to_owned()),
        Err(error) => {
            tracing::error!(
                error = %error.0,
                "turn_error_for_user failed; using the default classifier"
            );
            classify_provider_error(status_code, message, body_detail, BYOK, model_name)
                .unwrap_or((CODE_INTERNAL.to_owned(), DETAIL_INTERNAL.to_owned()))
        }
    }
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
    // Aliases go through `from_as`/`join_as`: a `(table, alias)` tuple is a
    // schema.table ref in sea-query, not an alias (it rendered
    // `FROM "assistant_turn"."t"`, an invalid relation live).
    Query::select()
        .from_as(TABLE_TURN, "t")
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
        .join_as(
            JoinType::InnerJoin,
            TABLE_THREAD,
            "th",
            Expr::col(("t", "thread_id")).equals(("th", "id")),
        )
        .join_as(
            JoinType::InnerJoin,
            "workspaces",
            "w",
            Expr::col(("th", "workspace_id")).equals(("w", "id")),
        )
        .join_as(
            JoinType::LeftJoin,
            TABLE_MESSAGE,
            "m",
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

/// Clear the thread's in-flight pointer when it still names this turn
/// (`tasks.py:166,190,211`).
pub fn clear_active_turn_update() -> String {
    format!(
        "UPDATE {TABLE_THREAD} SET active_turn_id = NULL \
         WHERE id = $1 AND active_turn_id = $2"
    )
}

/// Terminal cancel write (`tasks.py:204-209`): status + completed_at only —
/// no error columns, unlike the fail path. Guarded on `running` like
/// the complete/fail writes: only the first terminal writer owns the row.
pub fn cancel_turn_update() -> String {
    format!(
        "UPDATE {TABLE_TURN} SET status = '{TURN_CANCELLED}', completed_at = NOW() WHERE id = $1 \
         AND status = '{TURN_RUNNING}'"
    )
}

/// Thread lock for message/event writes (`events.py:92-93,112-113`):
/// `SELECT … FOR UPDATE` on the thread before allocating seq.
pub fn thread_lock_select() -> String {
    format!("SELECT id FROM {TABLE_THREAD} WHERE id = $1 FOR UPDATE")
}

// (complete/fail/finalize/error/event/prune builders moved to
// `pidash_services::domain::assistant`; cancel/clear/locks stay.)

// --------------------------------------------------------------------------- //
// Seam boundary + turn driver (`tasks.py:299-367`)
// --------------------------------------------------------------------------- //

/// Infra failure inside the seam (DB/Redis/transport). Outside the stream
/// loop the handler maps it to [`Verdict::Fail`]: Celery retries are disabled
/// for the turn task (`max_retries=0`, `tasks.py:479`), so a crashed turn
/// parks and the sweep recovers it — write tools are never re-executed by
/// redelivery (`tasks.py:8-10`). Inside the stream loop the driver fails the
/// turn first (report, fail the open row, classify, fail the turn), like the
/// `except Exception` arm (`tasks.py:356-361`); only a failure of that
/// failure path still parks for the sweep.
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
/// (`tasks.py:346-361`) plus the output-limit check inside
/// `_classify_error` (`tasks.py:489-499`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentError {
    /// `UsageLimitExceeded` → `iteration_limit` (`tasks.py:351-355`).
    UsageLimit(String),
    /// A typed `AssistantError` raised inside the run — by a model adapter
    /// or an ee provider — already carrying its stable code and user-facing
    /// detail. Used verbatim, never text-guessed: the typed-first branch of
    /// `_classify_error` (`tasks.py:453-457`, #1126, kept by #1430).
    ///
    /// Python walks `exc.__cause__` because the agent runtime may re-raise a
    /// provider failure wrapped in its own type. Rust has no exception chain:
    /// the seam implementor performs that walk when converting its native
    /// error into an [`AgentError`] at the boundary, emitting `Typed` when
    /// any cause is an `AssistantError` (see [`ModelError`] for the same
    /// already-classified crossing at model-resolution time).
    Typed { code: String, detail: String },
    /// The model ran out of output tokens (`IncompleteToolCall`, or the
    /// length-exhaustion `UnexpectedModelBehavior`) → `response_truncated`
    /// with the constant cut-off detail: the `_hit_output_limit` branch
    /// (`tasks.py:489-499`), which Django checks after the provider
    /// classifier and before `internal`. The variant bypasses
    /// [`classify_provider_error`] outright — neither pydantic-ai message matches
    /// any provider marker, so the bypass classifies identically.
    OutputTruncated,
    /// Any other provider failure → [`TurnSeam::turn_error_for_user`]
    /// (`tasks.py:356-361`). `message` is `str(exc)`; `status_code` the
    /// exception's HTTP status when it carries one; `body_detail` the
    /// provider's own one-line explanation from the structured response
    /// body (`""` when it gave none); `model_name` the model the failing
    /// call used (`""` when unknown).
    Provider {
        message: String,
        status_code: Option<u16>,
        body_detail: String,
        model_name: String,
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
    /// `_was_cut_off` (`tasks.py:369,502-507`): true when the final
    /// reply ended because the output limit was reached.
    fn was_cut_off(
        &self,
        stream: &Self::Stream,
    ) -> impl std::future::Future<Output = Result<bool, SeamError>> + Send;
    /// `_mark_cut_off` (`tasks.py:370-371,510-530`): flag the turn's last
    /// reply as cut off — `payload.truncated` on the row, then the
    /// envelope re-sent as a second `message_completed`.
    fn mark_cut_off(
        &self,
        ctx: &TurnContext,
    ) -> impl std::future::Future<Output = Result<(), SeamError>> + Send;
    /// `_complete_turn` (`tasks.py:158-172`), including the delta prune
    /// (`:172`). `truncated` adds the `"truncated": true` key to the
    /// `turn_completed` payload — only when true (`:168-169`).
    fn complete_turn(
        &self,
        ctx: &TurnContext,
        model_messages: Value,
        usage: Value,
        model_used: String,
        truncated: bool,
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
    /// `turn_error_for_user` (`ee/assistant/model_provider.py:72-87`): the
    /// error to show the turn's user for a failure that is not already a
    /// typed `AssistantError`. The returned pair supplies the stable code
    /// and the user-facing detail; `None` leaves the failure as a generic
    /// internal error. CE: every connection is BYOK, so the wording
    /// assumes a key the user supplied, and `model_name` falls back to
    /// the user's configured model when the failure carries none. An
    /// overlay seam picks the connection kind here; an `Err` (a hook
    /// that itself fails) falls back to the default classifier, never
    /// stranding the turn.
    fn turn_error_for_user(
        &self,
        ctx: &TurnContext,
        message: &str,
        status_code: Option<u16>,
        body_detail: &str,
        model_name: &str,
    ) -> impl std::future::Future<Output = Result<Option<(String, String)>, SeamError>> + Send;
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
///
/// The stream phase runs under the soft-limit deadline
/// (`soft_limit_secs` from drive start, like Celery counting from task
/// start): past it the stream drops (aborting the agent), the open row
/// fails, and the turn fails with `turn_timeout` — the
/// `SoftTimeLimitExceeded` arm (`tasks.py:486-489`).
///
/// `soft_limit_secs` is the configured `ASSISTANT_TURN_SOFT_LIMIT`
/// (`settings.assistant.turn_soft_limit`), not the
/// [`TURN_SOFT_LIMIT_SECS`] default: Django reads the same setting at
/// `tasks.py:45`, so a hard-coded 300 here would kill healthy long turns
/// for any operator who raised the limit (PIDASHCONV-1458).
pub async fn drive_turn<S: TurnSeam>(
    seam: &S,
    turn_id: Uuid,
    soft_limit_secs: i64,
) -> Result<DriveResult, SeamError> {
    drive_turn_with_deadline(
        seam,
        turn_id,
        tokio::time::Instant::now() + Duration::from_secs(soft_limit_secs.max(0) as u64),
    )
    .await
}

/// [`drive_turn`] with an explicit stream-phase deadline (the production
/// deadline is `now + soft_limit_secs`; tests pass a near one).
pub async fn drive_turn_with_deadline<S: TurnSeam>(
    seam: &S,
    turn_id: Uuid,
    deadline: tokio::time::Instant,
) -> Result<DriveResult, SeamError> {
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
    let mut sink = StreamSink::new();
    let mut stream = match tokio::time::timeout_at(
        deadline,
        seam.open_stream(&ctx, &history, &toolsets),
    )
    .await
    {
        Ok(Ok(stream)) => stream,
        Ok(Err(err)) => {
            return stream_failed(seam, &ctx, &toolsets, None, &mut open_row, err).await;
        }
        // No stream exists yet, so there is nothing to drop; the sink is
        // fresh, so no row is open — just fail the turn.
        Err(_) => return soft_limit_expired(seam, &ctx, &mut sink, &mut open_row).await,
    };

    loop {
        let item = match tokio::time::timeout_at(deadline, seam.next_event(&mut stream)).await {
            Ok(Ok(Some(item))) => item,
            Ok(Ok(None)) => break,
            Ok(Err(err)) => {
                // `streamer.fail_open_row(FAILED)` (`tasks.py:353,358`):
                // close the open row through the tracked id, then fail.
                let fail_action = sink.fail_open(MSG_FAILED);
                return stream_failed(seam, &ctx, &toolsets, fail_action, &mut open_row, err).await;
            }
            Err(_) => {
                // Soft limit (`tasks.py:486-489`): drop the stream — for the
                // live seam this aborts the agent task — then fail the open
                // row and the turn. No runtime-failure report: the Python
                // arm raises out of `_run_turn` past it.
                drop(stream);
                return soft_limit_expired(seam, &ctx, &mut sink, &mut open_row).await;
            }
        };
        // The cancel check runs only when an event is delivered, never on
        // stream end (`tasks.py:231-233`: the `async for` body). A cancel
        // that lands after the last event — or before a stream that yields
        // nothing — is ignored and the turn completes.
        if !matches!(item, StreamItem::End) {
            let cancelled = match seam.is_cancelled(ctx.turn_id).await {
                Ok(cancelled) => cancelled,
                // A seam failure inside the stream loop is a turn failure,
                // not a crash (`tasks.py:356-361`): the handler raised out
                // of `assistant.run`, so the run fails — report, fail the
                // open row, classify, fail the turn.
                Err(err) => {
                    let fail_action = sink.fail_open(MSG_FAILED);
                    return seam_failed(seam, &ctx, &toolsets, fail_action, &mut open_row, &err)
                        .await;
                }
            };
            if cancelled {
                report_runtime_failures(seam, &ctx, &toolsets).await?;
                if let Some(action) = sink.fail_open(MSG_CANCELLED) {
                    perform(seam, &ctx, &mut open_row, &action).await?;
                }
                seam.cancel_turn(&ctx).await?;
                return Ok(DriveResult::Cancelled);
            }
        }
        let actions = sink.handle(&item, seam.now_ms());
        for (index, action) in actions.iter().enumerate() {
            if let Err(err) = perform(seam, &ctx, &mut open_row, action).await {
                // Same failure path as a provider error (`tasks.py:356-361`).
                // A failed close is still open — Django clears `self.message`
                // only after success (`tasks.py:281-286`), so `fail_open_row`
                // retries the close as failed — hence the text of the failed
                // close is reused for the failed one. Likewise a failed flush
                // keeps its row text: `finalize_if_open` moves the text into
                // the close pair up front while Django keeps `self.text`
                // until the close succeeds, so an unperformed sibling close
                // carries the text `fail_open_row` would close with.
                let fail_action = match action {
                    SinkAction::FinalizeRow { text, .. } => Some(SinkAction::FinalizeRow {
                        text: text.clone(),
                        status: MSG_FAILED,
                    }),
                    _ => actions[index + 1..]
                        .iter()
                        .find_map(|sibling| match sibling {
                            SinkAction::FinalizeRow { text, .. } => Some(SinkAction::FinalizeRow {
                                text: text.clone(),
                                status: MSG_FAILED,
                            }),
                            _ => None,
                        })
                        .or_else(|| sink.fail_open(MSG_FAILED)),
                };
                return seam_failed(seam, &ctx, &toolsets, fail_action, &mut open_row, &err).await;
            }
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
    let truncated = seam.was_cut_off(&stream).await?;
    if truncated {
        seam.mark_cut_off(&ctx).await?;
    }
    seam.complete_turn(&ctx, model_messages, usage, model_label, truncated)
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
    let (code, detail) = match err {
        AgentError::UsageLimit(detail) => (CODE_ITERATION_LIMIT.to_owned(), detail),
        // Typed-first (`tasks.py:453-457`): the seam already resolved the
        // cause chain, so the code/detail cross verbatim — re-wording
        // here would erase both into the generic bucket.
        AgentError::Typed { code, detail } => (code, detail),
        // Output-limit (`tasks.py:489-499`): the constant code/detail —
        // `_hit_output_limit` sits after the provider classifier, and
        // neither pydantic-ai message matches a provider marker, so the
        // variant bypasses `classify_provider_error` outright.
        AgentError::OutputTruncated => (
            CODE_RESPONSE_TRUNCATED.to_owned(),
            DETAIL_RESPONSE_TRUNCATED.to_owned(),
        ),
        AgentError::Provider {
            message,
            status_code,
            body_detail,
            model_name,
        } => turn_error_for_user(seam, ctx, &message, status_code, &body_detail, &model_name).await,
    };
    fail_open_row_and_turn(seam, ctx, toolsets, fail_action, open_row, &code, &detail).await?;
    Ok(DriveResult::Failed { code, detail })
}

/// Seam-failure exit: a DB/Redis/transport error from inside the stream loop
/// (`_start_assistant_row` / `_emit_delta` / `_finalize_row` raising out of
/// `streamer.handle`, `tasks.py:118-155 + 336-361`) takes the same path as a
/// provider error — report, fail the open row, classify the seam text, fail
/// the turn — instead of leaving the turn RUNNING for the sweep.
async fn seam_failed<S: TurnSeam>(
    seam: &S,
    ctx: &TurnContext,
    toolsets: &[ToolsetHandle],
    fail_action: Option<SinkAction>,
    open_row: &mut Option<Uuid>,
    err: &SeamError,
) -> Result<DriveResult, SeamError> {
    let (code, detail) = turn_error_for_user(seam, ctx, &err.0, None, "", "").await;
    fail_open_row_and_turn(seam, ctx, toolsets, fail_action, open_row, &code, &detail).await?;
    Ok(DriveResult::Failed { code, detail })
}

/// Shared failure tail: report runtime tool failures, fail the open row
/// (pre-computed by the caller), fail the turn with the given code/detail.
async fn fail_open_row_and_turn<S: TurnSeam>(
    seam: &S,
    ctx: &TurnContext,
    toolsets: &[ToolsetHandle],
    fail_action: Option<SinkAction>,
    open_row: &mut Option<Uuid>,
    code: &str,
    detail: &str,
) -> Result<(), SeamError> {
    report_runtime_failures(seam, ctx, toolsets).await?;
    if let Some(action) = fail_action {
        perform(seam, ctx, open_row, &action).await?;
    }
    seam.fail_turn(ctx, code, detail).await
}

/// Soft-limit exit (`tasks.py:486-489`, `SoftTimeLimitExceeded`): fail the
/// open row when one is open, then fail the turn with `turn_timeout`. The
/// caller already dropped the stream. Deliberately no runtime-failure
/// report — the Python arm bypasses `_report_runtime_tool_failures`.
async fn soft_limit_expired<S: TurnSeam>(
    seam: &S,
    ctx: &TurnContext,
    sink: &mut StreamSink,
    open_row: &mut Option<Uuid>,
) -> Result<DriveResult, SeamError> {
    if let Some(action) = sink.fail_open(MSG_FAILED) {
        perform(seam, ctx, open_row, &action).await?;
    }
    seam.fail_turn(ctx, CODE_TURN_TIMEOUT, DETAIL_TURN_TIMEOUT)
        .await?;
    Ok(DriveResult::Failed {
        code: CODE_TURN_TIMEOUT.to_owned(),
        detail: DETAIL_TURN_TIMEOUT.to_owned(),
    })
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
            // The id is released only after success: a failed close leaves
            // the row open (Django clears `self.message` only after success,
            // `tasks.py:281-286`), so the failure path can retry it as failed.
            if let Some(id) = *open_row {
                seam.finalize_row(ctx, id, text, status).await?;
                *open_row = None;
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
///
/// `soft_limit_secs` is the configured `ASSISTANT_TURN_SOFT_LIMIT`
/// (`settings.assistant.turn_soft_limit`); the handler drives
/// [`drive_turn`] under `now + soft` like Celery's `soft_time_limit`.
pub fn register_turn_handler<S: TurnSeam + 'static>(
    registry: &mut Registry,
    _pool: PgPool,
    seam: Arc<S>,
    soft_limit_secs: i64,
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
                    match drive_turn(seam.as_ref(), turn_id, soft_limit_secs).await {
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
    fn classify_provider_error_matches_fixture_vectors() {
        use crate::assistant::provider_errors::{classify_provider_error, BYOK, GATEWAY};
        let fx = tasks_fixture();
        let vectors = fx["tasks"]["classify_error"]["vectors"]
            .as_object()
            .expect("vectors map");
        // Every fixture vector pins its (code, detail) pair.
        for (name, pair) in vectors {
            let (status, text, body_detail, connection, model) = match name.as_str() {
                "rate_429" => (
                    Some(429),
                    "status_code: 429, model_name: m, body: {'message': 'Rate limit exceeded'}",
                    "",
                    BYOK,
                    "",
                ),
                // Django parametrizes `test_429_asks_the_user_to_retry`
                // over both connections: a 429 reads as rate-limited on
                // the gateway too.
                "gateway_429" => (
                    Some(429),
                    "status_code: 429, model_name: m, body: {'message': 'Rate limit exceeded'}",
                    "",
                    GATEWAY,
                    "",
                ),
                "credit_402_body" => (
                    Some(402),
                    "status_code: 402, model_name: m, body: {'message': 'Insufficient credits.'}",
                    "Insufficient credits. Top up at https://example.test/billing.",
                    BYOK,
                    "",
                ),
                "credit_402_bare" => (Some(402), "anything at all", "", BYOK, ""),
                "credit_429_quota" => (
                    Some(429),
                    "You exceeded your current quota. code: insufficient_quota",
                    "",
                    BYOK,
                    "",
                ),
                "auth_401" => (Some(401), "status_code: 401, model_name: m", "", BYOK, ""),
                "auth_400_text" => (
                    Some(400),
                    "API key not valid. Please pass a valid API key.",
                    "",
                    BYOK,
                    "",
                ),
                "model_400_named" => (Some(400), "unsupported model: foo", "", BYOK, "oracle-stub"),
                "model_noname" => (None, "model_not_found: no such model", "", BYOK, ""),
                "unreachable_500" => (Some(500), "connection reset by upstream", "", BYOK, ""),
                "unreachable_text" => (None, "Connection timed out", "", BYOK, ""),
                "gateway_401" => (
                    Some(401),
                    "status_code: 401, model_name: m",
                    "",
                    GATEWAY,
                    "",
                ),
                "gateway_402_body" => (
                    Some(402),
                    "status_code: 402, model_name: m",
                    "Insufficient credits. Top up at https://example.test/billing.",
                    GATEWAY,
                    "",
                ),
                "gateway_model" => (Some(400), "Unsupported model", "", GATEWAY, "acme/gone-1"),
                other => panic!("unknown fixture vector {other}"),
            };
            let got = classify_provider_error(status, text, body_detail, connection, model)
                .unwrap_or_else(|| panic!("vector {name} classified"));
            assert_eq!(got.0, pair[0].as_str().unwrap(), "code for {name}");
            assert_eq!(got.1, pair[1].as_str().unwrap(), "detail for {name}");
        }
        // The status is read from the failure, never substring-matched: a
        // request id containing "401"/"402"/"429" classifies nothing on its
        // own (`test_a_status_like_number_in_the_text_is_not_a_status`).
        for request_id in ["req_401abc", "req_402abc", "req_429abc"] {
            let text = format!("upstream failed, request id {request_id}");
            assert_eq!(
                classify_provider_error(Some(500), &text, "", BYOK, ""),
                None,
                "{request_id} is not a status"
            );
            assert_eq!(
                classify_provider_error(None, &text, "", BYOK, ""),
                None,
                "{request_id} is not a status"
            );
        }
        let classify =
            |status, text: &str| classify_provider_error(status, text, "", BYOK, "").map(|p| p.0);
        // A 5xx never counts as auth, and "quota exceeded" alone on a 429
        // is a per-minute request quota, not an empty balance.
        assert_eq!(
            classify(Some(500), "authentication service returned an error"),
            None
        );
        assert_eq!(
            classify(Some(429), "Rate limit quota exceeded, retry in 30s").as_deref(),
            Some("provider_rate_limited")
        );
        // A 400 that is not about the model is left unclassified.
        assert_eq!(
            classify(
                Some(400),
                "This model's maximum context length is 128000 tokens."
            ),
            None
        );
        // Credit beats auth when both markers are present (branch order).
        assert_eq!(
            classify(None, "unauthorized: payment required").as_deref(),
            Some("provider_out_of_credit")
        );
        // Status-less failures still classify by text
        // (`test_statusless_failures_are_still_classified_by_text`).
        assert_eq!(
            classify(None, "Incorrect API key provided").as_deref(),
            Some("provider_auth_failed")
        );
        assert_eq!(classify(None, "something else entirely"), None);
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
        // Aliased refs (`AS`), never schema.table misreads: `(table, alias)`
        // tuples render `FROM "assistant_turn"."t"`, an invalid relation.
        for aliased in [
            "FROM \"assistant_turn\" AS \"t\"",
            "INNER JOIN \"assistant_thread\" AS \"th\"",
            "INNER JOIN \"workspaces\" AS \"w\"",
            "LEFT JOIN \"assistant_message\" AS \"m\"",
        ] {
            assert!(load.contains(aliased), "load renders {aliased}: {load}");
        }
        for misread in [
            "\"assistant_turn\".\"t\"",
            "\"assistant_thread\".\"th\"",
            "\"workspaces\".\"w\"",
            "\"assistant_message\".\"m\"",
        ] {
            assert!(!load.contains(misread), "schema.table misread {misread}");
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

        // (complete/fail/finalize/error/event/prune pins moved to
        // `pidash_services::domain::assistant` with their SQL.)
        let clear = clear_active_turn_update();
        assert!(clear.contains("active_turn_id = NULL"));
        assert!(
            clear.contains("active_turn_id = $2"),
            "clears only when matching"
        );

        // Cancel writes status only — no error columns (`tasks.py:204-209`).
        let cancel = cancel_turn_update();
        assert!(cancel.contains("status = 'cancelled'"));
        assert!(
            !cancel.contains("error_code"),
            "cancel keeps error columns: {cancel}"
        );

        assert!(thread_lock_select().contains("FOR UPDATE"));
    }

    /// Terminal writes are guarded on `running` (PIDASHCONV-1253): a swept
    /// turn can never flip back. Python overwrites unconditionally — safe
    /// there because its hard kill precedes the sweep — so the guard is a
    /// no-op on every Python-reachable path.
    #[test]
    fn terminal_updates_guard_running() {
        // (complete/fail guards pinned in full text by
        // `pidash_services::domain::assistant`.)
        let sql = cancel_turn_update();
        assert!(
            sql.contains("AND status = 'running'"),
            "guarded terminal write: {sql}"
        );
    }

    /// The soft-limit (code, detail) pair is Django's verbatim
    /// (`tasks.py:489`); the contract test pins it live on both backends.
    #[test]
    fn turn_timeout_code_and_detail_verbatim() {
        assert_eq!(CODE_TURN_TIMEOUT, "turn_timeout");
        assert_eq!(
            DETAIL_TURN_TIMEOUT,
            "The assistant took too long to respond."
        );
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
    /// successful `next_event` pulls. `open_delay` stalls `open_stream`;
    /// `stall_at_pull` stalls that 1-based `next_event` pull for `stall_for`
    /// (soft-limit tests).
    struct FakeSeam {
        ctx: Option<TurnContext>,
        claim: bool,
        model: Result<String, ModelError>,
        toolsets: Result<(Vec<ToolsetHandle>, Vec<SkippedServer>), SeamError>,
        runtime_failed: Vec<SkippedServer>,
        script: Mutex<VecDeque<Result<Option<StreamItem>, AgentError>>>,
        open_err: Option<AgentError>,
        open_delay: Option<Duration>,
        stall_at_pull: Option<usize>,
        stall_for: Duration,
        cancel_after: Option<usize>,
        pulls: Mutex<usize>,
        usage: Option<TurnUsage>,
        /// Scripted `was_cut_off` answer (default false).
        cut_off: bool,
        fail_emit: bool,
        /// 1-based row-op index to fail once (`start_row`/`emit_delta`/
        /// `finalize_row` in perform order); the failure-path writes that
        /// follow succeed, like a transient fault that already cleared.
        fail_row_op_at: Option<usize>,
        row_ops: Mutex<usize>,
        /// `is_cancelled` raises instead of answering.
        fail_cancel_check: bool,
        /// `fail_turn` raises (a failure path that cannot write).
        fail_turn_write: bool,
        /// `turn_error_for_user` raises (an overlay hook bug): the drive
        /// must fall back to the default classifier, not strand the turn.
        fail_turn_error: bool,
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
                open_delay: None,
                stall_at_pull: None,
                stall_for: Duration::from_secs(5),
                cancel_after: None,
                pulls: Mutex::new(0),
                usage: Some(TurnUsage {
                    input_tokens: Some(1),
                    output_tokens: Some(2),
                    total_tokens: Some(3),
                    requests: Some(1),
                    tool_calls: Some(0),
                }),
                cut_off: false,
                fail_emit: false,
                fail_row_op_at: None,
                row_ops: Mutex::new(0),
                fail_cancel_check: false,
                fail_turn_write: false,
                fail_turn_error: false,
                calls: Mutex::new(Vec::new()),
            }
        }

        /// Next 1-based row-op index, for `fail_row_op_at` scripts.
        fn next_row_op(&self) -> usize {
            let mut ops = self.row_ops.lock().unwrap();
            *ops += 1;
            *ops
        }

        fn row_fault(&self, n: usize) -> Result<(), SeamError> {
            if self.fail_row_op_at == Some(n) {
                return Err(SeamError("boom: row write failed".to_owned()));
            }
            Ok(())
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
            if self.fail_cancel_check {
                return Err(SeamError("boom: cancel read failed".to_owned()));
            }
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
            if let Some(delay) = self.open_delay {
                tokio::time::sleep(delay).await;
            }
            match &self.open_err {
                Some(e) => Err(e.clone()),
                None => Ok(FakeStream),
            }
        }
        async fn next_event(&self, _s: &mut FakeStream) -> Result<Option<StreamItem>, AgentError> {
            *self.pulls.lock().unwrap() += 1;
            let pulls = *self.pulls.lock().unwrap();
            if self.stall_at_pull == Some(pulls) {
                tokio::time::sleep(self.stall_for).await;
            }
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
            self.row_fault(self.next_row_op())?;
            Ok(id)
        }
        async fn emit_delta(
            &self,
            _c: &TurnContext,
            _m: Uuid,
            chunk: &str,
        ) -> Result<(), SeamError> {
            self.log(format!("emit_delta:{chunk}"));
            self.row_fault(self.next_row_op())?;
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
            self.row_fault(self.next_row_op())?;
            Ok(())
        }
        async fn dump_messages(&self, _s: &FakeStream) -> Result<Value, SeamError> {
            Ok(json!([{"role": "assistant"}]))
        }
        async fn extract_usage(&self, _s: &FakeStream) -> Result<Option<TurnUsage>, SeamError> {
            Ok(self.usage.clone())
        }
        async fn was_cut_off(&self, _s: &FakeStream) -> Result<bool, SeamError> {
            self.log("was_cut_off".to_owned());
            Ok(self.cut_off)
        }
        async fn mark_cut_off(&self, _c: &TurnContext) -> Result<(), SeamError> {
            self.log("mark_cut_off".to_owned());
            Ok(())
        }
        async fn complete_turn(
            &self,
            _c: &TurnContext,
            _m: Value,
            usage: Value,
            model_used: String,
            truncated: bool,
        ) -> Result<(), SeamError> {
            self.log(format!(
                "complete_turn:{model_used}:{}:{truncated}",
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
            if self.fail_turn_write {
                return Err(SeamError("boom: fail write failed".to_owned()));
            }
            Ok(())
        }
        async fn cancel_turn(&self, _c: &TurnContext) -> Result<(), SeamError> {
            self.log("cancel_turn".to_owned());
            Ok(())
        }

        async fn turn_error_for_user(
            &self,
            _c: &TurnContext,
            message: &str,
            status_code: Option<u16>,
            body_detail: &str,
            model_name: &str,
        ) -> Result<Option<(String, String)>, SeamError> {
            use crate::assistant::provider_errors::{classify_provider_error, BYOK};
            if self.fail_turn_error {
                return Err(SeamError("overlay bug".to_owned()));
            }
            Ok(classify_provider_error(
                status_code,
                message,
                body_detail,
                BYOK,
                model_name,
            ))
        }
    }

    /// Soft limit mid-stream (`tasks.py:486-489`, PIDASHCONV-1253): the
    /// second pull stalls past the deadline, so the open row fails and the
    /// turn fails with `turn_timeout` — no completion, and no
    /// runtime-failure report (the Python arm bypasses it).
    #[tokio::test]
    async fn drive_soft_limit_mid_stream_fails_turn_timeout() {
        let mut seam = FakeSeam::ok(vec![StreamItem::TextDelta {
            chunk: "hi".to_owned(),
        }]);
        seam.stall_at_pull = Some(2);
        let deadline = tokio::time::Instant::now() + Duration::from_millis(100);
        let out = drive_turn_with_deadline(&seam, Uuid::new_v4(), deadline)
            .await
            .unwrap();
        assert_eq!(
            out,
            DriveResult::Failed {
                code: CODE_TURN_TIMEOUT.to_owned(),
                detail: DETAIL_TURN_TIMEOUT.to_owned(),
            }
        );
        let calls = seam.take_calls();
        assert!(
            calls.iter().any(|c| c.starts_with("start_row:")),
            "the streamed row opened: {calls:?}"
        );
        assert!(
            calls.iter().any(|c| c == "finalize_row:failed:hi"),
            "the open row failed: {calls:?}"
        );
        assert!(
            calls
                .iter()
                .any(|c| c == "fail_turn:turn_timeout:The assistant took too long to respond."),
            "the turn failed with turn_timeout: {calls:?}"
        );
        assert!(
            !calls.iter().any(|c| c.starts_with("complete_turn")),
            "never completes: {calls:?}"
        );
        assert!(
            !calls.iter().any(|c| c == "runtime_failures"),
            "no runtime-failure report on the timeout path: {calls:?}"
        );
        assert!(
            !calls.iter().any(|c| c.starts_with("emit_skipped")),
            "no skipped-servers emit on the timeout path: {calls:?}"
        );
    }

    /// Soft limit while the stream is still opening: no row can be open, so
    /// only the turn fails.
    #[tokio::test]
    async fn drive_soft_limit_during_open_fails_without_row() {
        let mut seam = FakeSeam::ok(vec![]);
        seam.open_delay = Some(Duration::from_secs(5));
        let deadline = tokio::time::Instant::now() + Duration::from_millis(100);
        let out = drive_turn_with_deadline(&seam, Uuid::new_v4(), deadline)
            .await
            .unwrap();
        assert_eq!(
            out,
            DriveResult::Failed {
                code: CODE_TURN_TIMEOUT.to_owned(),
                detail: DETAIL_TURN_TIMEOUT.to_owned(),
            }
        );
        let calls = seam.take_calls();
        assert!(calls.iter().any(|c| c == "open_stream"));
        assert!(
            !calls.iter().any(|c| c.starts_with("start_row")),
            "no row opened: {calls:?}"
        );
        assert_eq!(
            calls.iter().filter(|c| c.starts_with("fail_turn:")).count(),
            1,
            "exactly one fail: {calls:?}"
        );
    }

    /// PIDASHCONV-1458: [`drive_turn`] honors the configured soft limit,
    /// not the 300 default — with a 1s limit a stream stalled past it
    /// fails with `turn_timeout` (against a hard-coded 300 the same
    /// stream would ride out the 5s stall and complete).
    #[tokio::test]
    async fn drive_turn_honors_configured_soft_limit() {
        let mut seam = FakeSeam::ok(vec![]);
        seam.stall_at_pull = Some(1);
        let out = drive_turn(&seam, Uuid::new_v4(), 1).await.unwrap();
        assert_eq!(
            out,
            DriveResult::Failed {
                code: CODE_TURN_TIMEOUT.to_owned(),
                detail: DETAIL_TURN_TIMEOUT.to_owned(),
            }
        );
        let calls = seam.take_calls();
        assert!(
            calls
                .iter()
                .any(|c| c == "fail_turn:turn_timeout:The assistant took too long to respond."),
            "the turn failed with turn_timeout: {calls:?}"
        );
        assert!(
            !calls.iter().any(|c| c.starts_with("complete_turn")),
            "never completes: {calls:?}"
        );
    }

    #[tokio::test]
    async fn drive_noop_paths() {
        // Missing/ineligible context: silent return, nothing else runs.
        let mut seam = FakeSeam::ok(vec![]);
        seam.ctx = None;
        let out = drive_turn(&seam, Uuid::new_v4(), TURN_SOFT_LIMIT_SECS as i64)
            .await
            .unwrap();
        assert_eq!(out, DriveResult::Noop);
        assert_eq!(seam.take_calls().as_slice(), ["load_context"]);

        // Lost claim: same silence after mark_running.
        let mut seam = FakeSeam::ok(vec![]);
        seam.claim = false;
        let out = drive_turn(&seam, Uuid::new_v4(), TURN_SOFT_LIMIT_SECS as i64)
            .await
            .unwrap();
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
        let out = drive_turn(&seam, Uuid::new_v4(), TURN_SOFT_LIMIT_SECS as i64)
            .await
            .unwrap();
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
        let out = drive_turn(&seam, Uuid::new_v4(), TURN_SOFT_LIMIT_SECS as i64)
            .await
            .unwrap();
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
        let out = drive_turn(&seam, Uuid::new_v4(), TURN_SOFT_LIMIT_SECS as i64)
            .await
            .unwrap();
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
        let out = drive_turn(&seam, Uuid::new_v4(), TURN_SOFT_LIMIT_SECS as i64)
            .await
            .unwrap();
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
        let out = drive_turn(&seam, Uuid::new_v4(), TURN_SOFT_LIMIT_SECS as i64)
            .await
            .unwrap();
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
        let out = drive_turn(&seam, Uuid::new_v4(), TURN_SOFT_LIMIT_SECS as i64)
            .await
            .unwrap();
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
        let out = drive_turn(&seam, Uuid::new_v4(), TURN_SOFT_LIMIT_SECS as i64)
            .await
            .unwrap();
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
                body_detail: String::new(),
                model_name: String::new(),
            }));
        let out = drive_turn(&seam, Uuid::new_v4(), TURN_SOFT_LIMIT_SECS as i64)
            .await
            .unwrap();
        assert_eq!(
            out,
            DriveResult::Failed {
                code: "provider_unreachable".to_owned(),
                detail: "Could not reach the configured provider endpoint.".to_owned(),
            }
        );
        assert!(seam.saw("finalize_row:failed:hi").len() == 1);
        assert!(seam.saw("fail_turn:provider_unreachable").len() == 1);
    }

    #[tokio::test]
    async fn drive_output_truncated_fails_response_truncated() {
        // `_hit_output_limit` (`tasks.py:489-499`): an output-limit
        // failure carries the constant code/detail, never
        // text-classified — the pydantic-ai messages match no
        // provider marker, so the bypass equals Django's order.
        let mut seam = FakeSeam::ok(vec![]);
        seam.open_err = Some(AgentError::OutputTruncated);
        let out = drive_turn(&seam, Uuid::new_v4(), TURN_SOFT_LIMIT_SECS as i64)
            .await
            .unwrap();
        assert_eq!(
            out,
            DriveResult::Failed {
                code: CODE_RESPONSE_TRUNCATED.to_owned(),
                detail: DETAIL_RESPONSE_TRUNCATED.to_owned(),
            }
        );
        assert_eq!(seam.saw("fail_turn:response_truncated:").len(), 1);
        assert!(seam.saw("complete_turn").is_empty());
    }

    #[tokio::test]
    async fn drive_cut_off_marks_and_completes_truncated() {
        // `_was_cut_off` / `_mark_cut_off` (`tasks.py:369-372`): a
        // cut-off success marks the row before completing, and the
        // completion carries the flag.
        let mut seam = FakeSeam::ok(vec![StreamItem::End]);
        seam.cut_off = true;
        let out = drive_turn(&seam, Uuid::new_v4(), TURN_SOFT_LIMIT_SECS as i64)
            .await
            .unwrap();
        assert_eq!(out, DriveResult::Completed);
        assert_eq!(seam.saw("was_cut_off").len(), 1);
        assert_eq!(seam.saw("mark_cut_off").len(), 1);
        let done = seam.saw("complete_turn");
        assert_eq!(done.len(), 1);
        assert!(done[0].ends_with(":true"), "truncated flag: {done:?}");
        let calls = seam.take_calls();
        let marked = calls.iter().position(|c| c == "mark_cut_off").unwrap();
        let completed = calls
            .iter()
            .position(|c| c.starts_with("complete_turn"))
            .unwrap();
        assert!(marked < completed, "mark precedes complete: {calls:?}");
    }

    #[tokio::test]
    async fn drive_clean_finish_skips_mark_and_flag() {
        // The normal-finish control: no mark call, and the completion
        // carries no flag.
        let seam = FakeSeam::ok(vec![StreamItem::End]);
        let out = drive_turn(&seam, Uuid::new_v4(), TURN_SOFT_LIMIT_SECS as i64)
            .await
            .unwrap();
        assert_eq!(out, DriveResult::Completed);
        assert_eq!(seam.saw("was_cut_off").len(), 1);
        assert!(seam.saw("mark_cut_off").is_empty());
        let done = seam.saw("complete_turn");
        assert_eq!(done.len(), 1);
        assert!(done[0].ends_with(":false"), "no flag: {done:?}");
    }

    #[tokio::test]
    async fn drive_failing_turn_error_hook_falls_back_to_default_classifier() {
        // Port of `test_a_failing_hook_does_not_strand_the_turn`: the seam
        // hook raises, so the drive falls back to the default classifier
        // and the turn still fails with the classified code.
        let mut seam = FakeSeam::ok(vec![]);
        seam.fail_turn_error = true;
        seam.open_err = Some(AgentError::Provider {
            message: "slow down".to_owned(),
            status_code: Some(429),
            body_detail: String::new(),
            model_name: String::new(),
        });
        let out = drive_turn(&seam, Uuid::new_v4(), TURN_SOFT_LIMIT_SECS as i64)
            .await
            .unwrap();
        assert_eq!(
            out,
            DriveResult::Failed {
                code: "provider_rate_limited".to_owned(),
                detail: "Too many requests to the model provider. Wait a moment and try again."
                    .to_owned(),
            }
        );
        assert!(seam.saw("fail_turn:provider_rate_limited").len() == 1);
    }

    #[tokio::test]
    async fn drive_typed_errors_keep_code_and_detail() {
        // Port of `test_turn_errors.py::test_typed_errors_keep_their_code_and_detail`
        // (bb3d99c6): the fixture pins Django's exact (code, detail) pairs,
        // and a `Typed` failure mid-stream fails the turn with them verbatim.
        let fx = tasks_fixture();
        let vectors = fx["tasks"]["classify_error"]["typed_vectors"]["vectors"]
            .as_object()
            .expect("typed vectors map");
        assert_eq!(vectors.len(), 2, "fixture typed vectors changed?");
        for (name, pair) in vectors {
            let code = pair[0].as_str().unwrap().to_owned();
            let detail = pair[1].as_str().unwrap().to_owned();
            let seam = FakeSeam::ok(vec![StreamItem::TextStart {
                initial: "Hello".to_owned(),
            }]);
            seam.script
                .lock()
                .unwrap()
                .push_back(Err(AgentError::Typed {
                    code: code.clone(),
                    detail: detail.clone(),
                }));
            let out = drive_turn(&seam, Uuid::new_v4(), TURN_SOFT_LIMIT_SECS as i64)
                .await
                .unwrap();
            assert_eq!(
                out,
                DriveResult::Failed {
                    code: code.clone(),
                    detail: detail.clone(),
                },
                "typed vector {name} must survive verbatim"
            );
            // The streamed text is closed out as failed, like any other
            // mid-stream failure (`tasks.py:353,358`).
            assert!(seam.saw("finalize_row:failed:Hello").len() == 1);
            assert!(seam.saw(&format!("fail_turn:{code}:")).len() == 1);
        }
        // The typed arm is load-bearing, not dead: neither detail
        // classifies to its own code, so without `Typed` both vectors
        // would land in the generic bucket. ("Stream ended early." matches
        // no marker at all — Django's own comment on the unreachable case.)
        use crate::assistant::provider_errors::{classify_provider_error, BYOK};
        assert_eq!(
            classify_provider_error(None, "Stream ended early.", "", BYOK, ""),
            None
        );
        assert_eq!(
            classify_provider_error(
                None,
                "Plan usage is unavailable. Check Settings → Usage.",
                "",
                BYOK,
                ""
            ),
            None
        );

        // Port of `test_a_typed_error_wrapped_by_the_runtime_is_still_recognised`:
        // Django walks `__cause__` past the wrapper; the Rust seam
        // implementor performs that walk at the boundary and emits `Typed`.
        // The wrapper's own text would classify to nothing, proving the
        // typed arm did the work. `open_stream` takes the same path with no
        // row work (nothing streamed yet).
        assert_eq!(
            classify_provider_error(None, "model request failed", "", BYOK, ""),
            None
        );
        let quota = &fx["tasks"]["classify_error"]["typed_vectors"]["vectors"]["quota_direct"];
        let mut seam = FakeSeam::ok(vec![]);
        seam.open_err = Some(AgentError::Typed {
            code: quota[0].as_str().unwrap().to_owned(),
            detail: quota[1].as_str().unwrap().to_owned(),
        });
        let out = drive_turn(&seam, Uuid::new_v4(), TURN_SOFT_LIMIT_SECS as i64)
            .await
            .unwrap();
        assert_eq!(
            out,
            DriveResult::Failed {
                code: "quota_exceeded".to_owned(),
                detail: "Plan usage is unavailable. Check Settings → Usage.".to_owned(),
            }
        );
        assert!(seam.saw("finalize_row:").is_empty());
        assert!(seam.saw("fail_turn:quota_exceeded:").len() == 1);
    }

    #[tokio::test]
    async fn drive_runtime_failures_reported_on_failure() {
        let mut seam = FakeSeam::ok(vec![]);
        seam.open_err = Some(AgentError::Provider {
            message: "x".to_owned(),
            status_code: None,
            body_detail: String::new(),
            model_name: String::new(),
        });
        seam.runtime_failed = vec![SkippedServer {
            name: "dead".to_owned(),
            reason: "boom".to_owned(),
        }];
        let out = drive_turn(&seam, Uuid::new_v4(), TURN_SOFT_LIMIT_SECS as i64)
            .await
            .unwrap();
        assert!(matches!(out, DriveResult::Failed { .. }));
        assert_eq!(seam.saw("emit_skipped:dead").len(), 1);
    }

    /// A seam DB error from a mid-stream row write is a turn failure, not a
    /// crash (`tasks.py:356-361`): the open row fails, the seam text
    /// classifies, the turn fails — the sweep never sees it (PIDASHCONV-1254).
    #[tokio::test]
    async fn drive_row_write_failure_fails_turn_at_once() {
        let mut seam = FakeSeam::ok(vec![
            StreamItem::TextDelta {
                chunk: "hi".to_owned(),
            },
            StreamItem::TextDelta {
                chunk: "yo".to_owned(),
            },
            StreamItem::End,
        ]);
        seam.fail_row_op_at = Some(2); // the delta flush after the row opened
        let out = drive_turn(&seam, Uuid::new_v4(), TURN_SOFT_LIMIT_SECS as i64)
            .await
            .unwrap();
        assert_eq!(
            out,
            DriveResult::Failed {
                code: CODE_INTERNAL.to_owned(),
                detail: DETAIL_INTERNAL.to_owned(),
            }
        );
        assert!(seam.saw("finalize_row:failed:hiyo").len() == 1);
        assert!(seam.saw("fail_turn:internal:").len() == 1);
        assert!(seam.saw("complete_turn").is_empty());
        assert!(seam.saw("cancel_turn").is_empty());
    }

    /// The first row never opened, so there is nothing to close — Django's
    /// `fail_open_row` is a no-op with `self.message is None`
    /// (`tasks.py:289-292`).
    #[tokio::test]
    async fn drive_start_row_failure_fails_without_row_close() {
        let mut seam = FakeSeam::ok(vec![StreamItem::TextDelta {
            chunk: "hi".to_owned(),
        }]);
        seam.fail_row_op_at = Some(1);
        let out = drive_turn(&seam, Uuid::new_v4(), TURN_SOFT_LIMIT_SECS as i64)
            .await
            .unwrap();
        assert!(matches!(out, DriveResult::Failed { .. }));
        assert!(seam.saw("finalize_row:").is_empty());
        assert_eq!(seam.saw("fail_turn:").len(), 1);
    }

    /// A failed flush keeps its row text: the close pair's unperformed
    /// sibling close carries the accumulated text (`tasks.py:289-292` closes
    /// with `self.text`, which a failed `_flush` leaves intact).
    #[tokio::test]
    async fn drive_close_pair_flush_failure_keeps_row_text() {
        let mut seam = FakeSeam::ok(vec![
            StreamItem::TextDelta {
                chunk: "hi".to_owned(),
            },
            StreamItem::End,
        ]);
        seam.fail_row_op_at = Some(2); // the close pair's flush
        let out = drive_turn(&seam, Uuid::new_v4(), TURN_SOFT_LIMIT_SECS as i64)
            .await
            .unwrap();
        assert!(matches!(out, DriveResult::Failed { .. }));
        assert!(seam.saw("finalize_row:failed:hi").len() == 1);
        assert!(
            seam.saw("finalize_row:completed:").is_empty(),
            "the sibling close never ran"
        );
        assert_eq!(seam.saw("fail_turn:").len(), 1);
    }

    /// A failed close is still open (`tasks.py:281-286` clears `self.message`
    /// only after success), so the failure path retries it as failed.
    #[tokio::test]
    async fn drive_finalize_failure_retries_close_as_failed() {
        let mut seam = FakeSeam::ok(vec![
            StreamItem::TextDelta {
                chunk: "hi".to_owned(),
            },
            StreamItem::End,
        ]);
        seam.fail_row_op_at = Some(3); // the completed close at stream end
        let out = drive_turn(&seam, Uuid::new_v4(), TURN_SOFT_LIMIT_SECS as i64)
            .await
            .unwrap();
        assert!(matches!(out, DriveResult::Failed { .. }));
        assert!(seam.saw("finalize_row:completed:hi").len() == 1);
        assert!(seam.saw("finalize_row:failed:hi").len() == 1);
        assert_eq!(seam.saw("fail_turn:").len(), 1);
    }

    /// The live seam swallows cancel-read errors as false; a seam that
    /// surfaces one takes the same failure path as any other stream-loop
    /// seam error (nothing streamed yet, so no row closes).
    #[tokio::test]
    async fn drive_cancel_check_failure_fails_turn() {
        let mut seam = FakeSeam::ok(vec![StreamItem::TextDelta {
            chunk: "hi".to_owned(),
        }]);
        seam.fail_cancel_check = true;
        let out = drive_turn(&seam, Uuid::new_v4(), TURN_SOFT_LIMIT_SECS as i64)
            .await
            .unwrap();
        assert!(matches!(out, DriveResult::Failed { .. }));
        assert!(seam.saw("start_row").is_empty());
        assert_eq!(seam.saw("fail_turn:").len(), 1);
    }

    /// A failure path that cannot write is a crash, like Django's
    /// `fail_open_row`/`_fail_turn` raising out of the `except` arm: the
    /// driver returns the seam error and the sweep recovers the turn.
    #[tokio::test]
    async fn drive_seam_outage_still_parks_for_sweep() {
        let mut seam = FakeSeam::ok(vec![StreamItem::TextDelta {
            chunk: "hi".to_owned(),
        }]);
        seam.fail_row_op_at = Some(1);
        seam.fail_turn_write = true;
        let err = drive_turn(&seam, Uuid::new_v4(), TURN_SOFT_LIMIT_SECS as i64)
            .await
            .unwrap_err();
        assert_eq!(err, SeamError("boom: fail write failed".to_owned()));
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

    // -- live scratch-DB tests (env-gated) -------------------------------

    /// Scratch Postgres for the Done-when verification. Unset (plain
    /// `cargo test`) skips these; CI sets no database either, so the suite
    /// stays green offline. Run with e.g.
    /// `export DATABASE_URL=postgresql://user@host/db` for the real check.
    async fn scratch_pool() -> Option<sqlx::PgPool> {
        match std::env::var("DATABASE_URL") {
            Ok(url) => Some(
                sqlx::PgPool::connect(&url)
                    .await
                    .expect("connect to scratch DATABASE_URL"),
            ),
            Err(_) => {
                eprintln!("skipping live-db test: DATABASE_URL is not set");
                None
            }
        }
    }

    const LIVE_DDL: &[&str] = &[
        "CREATE TEMPORARY TABLE assistant_turn (id UUID PRIMARY KEY, thread_id UUID NOT NULL, user_message_id UUID NULL, status VARCHAR(16) NOT NULL, model_messages JSONB NULL, usage JSONB NULL, model_used VARCHAR(255) NOT NULL, error_code VARCHAR(64) NOT NULL, error_detail TEXT NOT NULL, created_at TIMESTAMPTZ NOT NULL, started_at TIMESTAMPTZ NULL, completed_at TIMESTAMPTZ NULL)",
        "CREATE TEMPORARY TABLE assistant_thread (id UUID PRIMARY KEY, workspace_id UUID NOT NULL, user_id UUID NOT NULL, title VARCHAR(255) NOT NULL, kind VARCHAR(16) NOT NULL, is_archived BOOLEAN NOT NULL, active_turn_id UUID NULL, created_at TIMESTAMPTZ NOT NULL, updated_at TIMESTAMPTZ NOT NULL)",
        "CREATE TEMPORARY TABLE workspaces (id UUID PRIMARY KEY, slug VARCHAR(48) NOT NULL, name VARCHAR(80) NOT NULL)",
        "CREATE TEMPORARY TABLE assistant_message (id UUID PRIMARY KEY, thread_id UUID NOT NULL, display_content TEXT NOT NULL)",
    ];

    async fn live_tx(pool: &sqlx::PgPool) -> sqlx::Transaction<'_, sqlx::Postgres> {
        let mut tx = pool.begin().await.expect("begin scratch tx");
        for ddl in LIVE_DDL {
            sqlx::query(ddl)
                .execute(&mut *tx)
                .await
                .expect("create temp table");
        }
        tx
    }

    fn live_uuid(suffix: u8) -> Uuid {
        Uuid::parse_str(&format!("44444444-4444-4444-4444-4444444444{suffix:02}"))
            .expect("fixed test uuid")
    }

    #[tokio::test]
    async fn live_load_context_select() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let mut tx = live_tx(&pool).await;
        let workspace = live_uuid(1);
        let user = live_uuid(2);
        let thread = live_uuid(3);
        let message = live_uuid(4);
        let turn = live_uuid(5);
        let orphan = live_uuid(6);
        sqlx::query("INSERT INTO workspaces (id, slug, name) VALUES ($1, 'acme', 'Acme')")
            .bind(workspace)
            .execute(&mut *tx)
            .await
            .expect("seed workspace");
        sqlx::query(
            "INSERT INTO assistant_thread (id, workspace_id, user_id, title, kind, is_archived, active_turn_id, created_at, updated_at) \
             VALUES ($1, $2, $3, 'T', 'chat', FALSE, NULL, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
        )
        .bind(thread)
        .bind(workspace)
        .bind(user)
        .execute(&mut *tx)
        .await
        .expect("seed thread");
        sqlx::query(
            "INSERT INTO assistant_message (id, thread_id, display_content) VALUES ($1, $2, 'hello')",
        )
        .bind(message)
        .bind(thread)
        .execute(&mut *tx)
        .await
        .expect("seed message");
        for (id, user_message, status) in [
            (turn, Some(message), TURN_QUEUED),
            (orphan, None, TURN_RUNNING),
        ] {
            sqlx::query(
                "INSERT INTO assistant_turn (id, thread_id, user_message_id, status, model_messages, usage, model_used, error_code, error_detail, created_at, started_at, completed_at) \
                 VALUES ($1, $2, $3, $4, NULL, NULL, '', '', '', '2026-01-01T00:00:00Z', NULL, NULL)",
            )
            .bind(id)
            .bind(thread)
            .bind(user_message)
            .bind(status)
            .execute(&mut *tx)
            .await
            .expect("seed turn");
        }
        // Select order is 12 turn + 9 thread + 3 workspace + 2 message
        // columns; several names repeat (`id`), so read by index.
        use sqlx::Row;
        let row = sqlx::query(&load_context_sql())
            .bind(turn)
            .fetch_one(&mut *tx)
            .await
            .expect("context select runs");
        assert_eq!(row.try_get::<Uuid, _>(0).expect("t.id"), turn);
        assert_eq!(row.try_get::<Uuid, _>(1).expect("t.thread_id"), thread);
        assert_eq!(
            row.try_get::<String, _>(22).expect("w.slug"),
            "acme".to_owned()
        );
        assert_eq!(
            row.try_get::<Option<String>, _>(25)
                .expect("m.display_content"),
            Some("hello".to_owned())
        );
        // The user-message join is a LEFT JOIN: turns without one load.
        let row = sqlx::query(&load_context_sql())
            .bind(orphan)
            .fetch_one(&mut *tx)
            .await
            .expect("context select runs without a user message");
        assert_eq!(row.try_get::<Uuid, _>(0).expect("t.id"), orphan);
        assert_eq!(
            row.try_get::<Option<String>, _>(25)
                .expect("m.display_content"),
            None
        );
    }
}
