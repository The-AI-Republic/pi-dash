//! Agent abstraction layer. Both the Codex and Claude Code bridges expose
//! the same behavioural surface (`spawn`, `run`, `next_events`, `send_approval`,
//! `interrupt`, `shutdown`) through [`AgentBridge`], so the supervisor does
//! not have to know which underlying CLI is driving a run.

use anyhow::Result;
use chrono::{DateTime, Utc};
use std::collections::VecDeque;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, watch};
use uuid::Uuid;

use crate::cloud::protocol::{ApprovalDecision, ApprovalKind, FailureReason};
use crate::config::schema::{AgentKind, RunnerConfig};

/// Per-line cap, **in bytes**, for stderr lines retained in the ring.
/// Compared against `str::len()` which is byte length, not char count —
/// 512 bytes accommodates most error lines (and a 200-char line of CJK
/// would be ~600 bytes and get truncated). Without a per-line cap a
/// single 1 MB line could defeat the line-count cap.
pub const STDERR_LINE_CAP_BYTES: usize = 512;

/// How many stderr lines to retain in the per-process ring. 30 lines ×
/// 512 bytes ≈ 15 KB upper bound; comfortably fits in a `RunFailed.detail`
/// payload after truncation while still surfacing enough recent context
/// to diagnose most failures (auth errors, panics, segfaults).
pub const STDERR_RING_LINES: usize = 30;

/// Bounded ring buffer of recent stderr lines from an agent subprocess.
/// Each agent's `drain_stderr` task pushes here; the supervisor reads
/// when it needs to enrich a `RunFailed` detail.
#[derive(Debug)]
pub struct StderrBuffer {
    cap: usize,
    lines: VecDeque<String>,
}

impl StderrBuffer {
    /// Construct a buffer with capacity `cap`. A `cap` of zero is
    /// silently clamped to 1 — the eviction logic in `push` only fires
    /// on `len() == cap`, which would never hold for `cap == 0` after
    /// the first push, leaving the buffer to grow unboundedly.
    pub fn new(cap: usize) -> Self {
        let cap = cap.max(1);
        Self {
            cap,
            lines: VecDeque::with_capacity(cap),
        }
    }

    pub fn push(&mut self, line: &str) {
        let Some(sanitized) = sanitize_stderr_line(line) else {
            return;
        };
        let truncated = if sanitized.len() > STDERR_LINE_CAP_BYTES {
            // Truncate on a char boundary so we never split a multi-byte
            // sequence; append a single `…` so consumers can tell the
            // line was clipped.
            let mut end = STDERR_LINE_CAP_BYTES;
            while !sanitized.is_char_boundary(end) && end > 0 {
                end -= 1;
            }
            format!("{}…", &sanitized[..end])
        } else {
            sanitized
        };
        if self.lines.len() == self.cap {
            self.lines.pop_front();
        }
        self.lines.push_back(truncated);
    }

    pub fn snapshot(&self) -> Vec<String> {
        self.lines.iter().cloned().collect()
    }
}

pub type StderrRing = Arc<Mutex<StderrBuffer>>;

/// Clean an incoming stderr line for inclusion in the failure-detail
/// payload that ends up in an issue activity comment. Two responsibilities:
///
/// 1. Strip ANSI escape sequences. Codex / claude colorize their tracing
///    output via `tracing-subscriber`'s default formatter; the escape
///    bytes (e.g. `\x1b[2m`) render as literal glyphs (`␛[2m`) in the
///    issue comment HTML, making the detail unreadable.
/// 2. Drop low-level (`INFO` / `DEBUG` / `TRACE`) codex tracing log
///    lines. These are codex's internal observability stream, not
///    actual program errors. Surfacing them to the user is pure noise —
///    every codex turn emits dozens. `WARN` and `ERROR` lines, plus any
///    unstructured stderr (panics, child-process traces, raw error
///    messages), pass through unchanged.
///
/// Returns `None` to mean "drop this line entirely."
pub fn sanitize_stderr_line(line: &str) -> Option<String> {
    let stripped = strip_ansi_codes(line);
    let trimmed = stripped.trim_end();
    if trimmed.is_empty() {
        return None;
    }
    if is_low_level_tracing(trimmed) {
        return None;
    }
    Some(trimmed.to_string())
}

/// Remove ANSI CSI escape sequences (`\x1b[…<letter>`) from `s`. Other
/// escape forms (OSC, charset selection, etc.) are left alone — they're
/// vanishingly rare in dev-tool stderr and CSI is what tracing-subscriber
/// emits.
fn strip_ansi_codes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' && chars.peek() == Some(&'[') {
            chars.next(); // consume '['
            // Skip parameters / intermediate bytes, then the final
            // alphabetic byte that terminates the CSI sequence.
            while let Some(&p) = chars.peek() {
                chars.next();
                if p.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// True for lines that match `tracing-subscriber`'s default text
/// formatter at INFO / DEBUG / TRACE level. Format is roughly
/// `<RFC3339 timestamp ending in 'Z'>  <LEVEL>  <rest>`. The two-space
/// gap between timestamp and level + the right-aligned LEVEL field
/// (`" INFO"`, `"DEBUG"`, `"TRACE"`, `" WARN"`, `"ERROR"`) is what we
/// match against. Lines without the trailing-`Z` timestamp prefix are
/// considered real stderr and pass through.
fn is_low_level_tracing(line: &str) -> bool {
    // Look for "...Z " somewhere near the start; tracing's default
    // format places the level immediately after the timestamp + 2 spaces.
    let Some((_ts, rest)) = line.split_once("Z ") else {
        return false;
    };
    let rest = rest.trim_start();
    // tracing-subscriber pads the level to width 5: " INFO" / "DEBUG" /
    // "TRACE" / " WARN" / "ERROR". After trim_start, that becomes the
    // bare level name + space.
    rest.starts_with("INFO ")
        || rest.starts_with("DEBUG ")
        || rest.starts_with("TRACE ")
}

/// Volatile process-handle the bridge surfaces to the supervisor for
/// observability. PID is captured at spawn time, `exit_rx` fires once the
/// child wait task observes termination. See `.ai_design/runner_agent_bridge`
/// §4.4. The handle carries no ownership of the child — the process wrapper
/// retains exclusive ownership inside its wait task.
#[derive(Debug, Clone)]
pub struct AgentProcessHandle {
    pub pid: Option<u32>,
    pub exit_rx: watch::Receiver<Option<ExitSnapshot>>,
}

#[derive(Debug, Clone)]
pub struct ExitSnapshot {
    pub status_code: Option<i32>,
    pub signal: Option<i32>,
    pub observed_at: DateTime<Utc>,
}

/// Events the bridge surfaces to the daemon's state machine. Agent-agnostic:
/// Codex and Claude both translate their native protocols into this shape.
#[derive(Debug, Clone)]
pub enum BridgeEvent {
    RunStarted {
        run_id: Uuid,
        thread_id: String,
    },
    Raw {
        run_id: Uuid,
        method: String,
        params: serde_json::Value,
    },
    ApprovalRequest {
        run_id: Uuid,
        approval_id: String,
        kind: ApprovalKind,
        payload: serde_json::Value,
        reason: Option<String>,
    },
    AwaitingReauth {
        run_id: Uuid,
        detail: Option<String>,
    },
    Completed {
        run_id: Uuid,
        done_payload: serde_json::Value,
    },
    Failed {
        run_id: Uuid,
        reason: FailureReason,
        detail: Option<String>,
    },
}

#[derive(Debug, Clone)]
pub struct RunPayload {
    pub run_id: Uuid,
    pub prompt: String,
    pub model: Option<String>,
    pub resume_thread_id: Option<String>,
}

/// Agent-CLI native resume failed because the session id we were given
/// is not present on this runner's local session store. The supervisor
/// downcasts to this type to emit `FailureReason::ResumeUnavailable`
/// rather than the generic agent-crash reason; cloud's reaction is to
/// drop the pin and re-queue without the resume hint.
#[derive(Debug, thiserror::Error)]
#[error("agent CLI could not resume session {thread_id}: {detail}")]
pub struct ResumeUnavailable {
    pub thread_id: String,
    pub detail: String,
}

/// Enum dispatch over the concrete bridges. Each variant owns the agent's
/// subprocess; the supervisor treats them uniformly.
pub enum AgentBridge {
    Codex(crate::codex::bridge::Bridge),
    ClaudeCode(crate::claude_code::bridge::Bridge),
}

/// Per-run cursor, paired with an `AgentBridge`. Holds agent-specific frame
/// translation state (sequence numbers, session ids).
pub enum AgentCursor {
    Codex(crate::codex::bridge::BridgeCursor),
    ClaudeCode(crate::claude_code::bridge::BridgeCursor),
}

impl AgentCursor {
    pub fn run_id(&self) -> Uuid {
        match self {
            AgentCursor::Codex(c) => c.run_id,
            AgentCursor::ClaudeCode(c) => c.run_id,
        }
    }

    pub fn thread_id(&self) -> &str {
        match self {
            AgentCursor::Codex(c) => &c.thread_id,
            AgentCursor::ClaudeCode(c) => &c.thread_id,
        }
    }
}

impl AgentBridge {
    /// Spawn the agent subprocess selected by the runner's config.
    ///
    /// `resume_thread_id` is used at spawn time only by Claude Code (it
    /// goes to `claude --resume <id>` on the command line). Codex consumes
    /// the resume hint inside `bridge.run()` via the `RunPayload` field
    /// instead.
    pub async fn spawn_from_config(
        runner: &RunnerConfig,
        cwd: &Path,
        model_override: Option<String>,
        resume_thread_id: Option<&str>,
    ) -> Result<Self> {
        match runner.agent.kind {
            AgentKind::Codex => {
                let b = crate::codex::bridge::Bridge::spawn(
                    &runner.codex.binary,
                    cwd,
                    selected_model(model_override, runner.codex.model_default.clone()),
                )
                .await?;
                Ok(AgentBridge::Codex(b))
            }
            AgentKind::ClaudeCode => {
                let b = crate::claude_code::bridge::Bridge::spawn(
                    &runner.claude_code.binary,
                    cwd,
                    selected_model(model_override, runner.claude_code.model_default.clone()),
                    resume_thread_id,
                )
                .await?;
                Ok(AgentBridge::ClaudeCode(b))
            }
        }
    }

    pub async fn run(&mut self, payload: &RunPayload, cwd: &Path) -> Result<AgentCursor> {
        match self {
            AgentBridge::Codex(b) => Ok(AgentCursor::Codex(b.run(payload, cwd).await?)),
            AgentBridge::ClaudeCode(b) => Ok(AgentCursor::ClaudeCode(b.run(payload, cwd).await?)),
        }
    }

    /// Wait for the next batch of events from the agent. Returns `None` when
    /// the subprocess has closed its output stream (no more events will ever
    /// arrive); callers should surface that as a run failure.
    pub async fn next_events(&mut self, cursor: &mut AgentCursor) -> Option<Vec<BridgeEvent>> {
        match (self, cursor) {
            (AgentBridge::Codex(b), AgentCursor::Codex(c)) => {
                let frame = b.next_frame().await?;
                Some(c.translate(frame))
            }
            (AgentBridge::ClaudeCode(b), AgentCursor::ClaudeCode(c)) => b.next_events(c).await,
            // These pairings are constructed together by `run`, so a mismatch
            // is a programmer error — fail loudly.
            _ => panic!("agent bridge and cursor variants mismatched"),
        }
    }

    pub async fn send_approval(
        &mut self,
        approval_id: &str,
        decision: ApprovalDecision,
    ) -> Result<()> {
        match self {
            AgentBridge::Codex(b) => b.send_approval(approval_id, decision).await,
            AgentBridge::ClaudeCode(b) => b.send_approval(approval_id, decision).await,
        }
    }

    pub async fn interrupt(&mut self) -> Result<()> {
        match self {
            AgentBridge::Codex(b) => b.interrupt().await,
            AgentBridge::ClaudeCode(b) => b.interrupt().await,
        }
    }

    pub async fn shutdown(self, grace: Duration) -> Result<()> {
        match self {
            AgentBridge::Codex(b) => b.server.shutdown(grace).await,
            AgentBridge::ClaudeCode(b) => b.shutdown(grace).await,
        }
    }

    /// Bridge-owned observability handle: the agent subprocess's PID and a
    /// watch receiver that yields `Some(ExitSnapshot)` once the wait task
    /// observes termination. Supervisor uses this to drive
    /// `state.set_agent_pid` / `set_agent_alive`.
    pub fn process_handle(&self) -> AgentProcessHandle {
        match self {
            AgentBridge::Codex(b) => b.server.process_handle(),
            AgentBridge::ClaudeCode(b) => b.process_handle(),
        }
    }

    /// Snapshot the last N lines of agent stderr. Returns `vec![]` if the
    /// process has emitted nothing on stderr (the common case for healthy
    /// runs). Used by the supervisor to enrich `RunFailed` details so the
    /// cloud / UI can tell the user *why* the agent died beyond just
    /// "agent stdout closed" or "no agent frames for 5 minutes".
    pub async fn recent_stderr(&self) -> Vec<String> {
        match self {
            AgentBridge::Codex(b) => b.server.recent_stderr().await,
            AgentBridge::ClaudeCode(b) => b.recent_stderr().await,
        }
    }
}

fn selected_model(
    model_override: Option<String>,
    configured_default: Option<String>,
) -> Option<String> {
    model_override.or(configured_default)
}

#[cfg(test)]
mod tests {
    use super::{
        STDERR_LINE_CAP_BYTES, StderrBuffer, sanitize_stderr_line, selected_model,
        strip_ansi_codes,
    };

    #[test]
    fn stderr_buffer_evicts_oldest_when_full() {
        let mut buf = StderrBuffer::new(3);
        buf.push("a");
        buf.push("b");
        buf.push("c");
        buf.push("d");
        assert_eq!(buf.snapshot(), vec!["b", "c", "d"]);
    }

    #[test]
    fn strip_ansi_codes_removes_csi_escapes() {
        // Real codex tracing output: timestamp + level wrapped in
        // color CSI sequences. Strip should leave plain text intact.
        let input = "\x1b[2m2026-05-03T07:27:06.636239Z\x1b[0m \x1b[32m INFO\x1b[0m hello";
        assert_eq!(
            strip_ansi_codes(input),
            "2026-05-03T07:27:06.636239Z  INFO hello"
        );
    }

    #[test]
    fn strip_ansi_codes_passes_plain_text_unchanged() {
        let input = "permission denied (publickey)";
        assert_eq!(strip_ansi_codes(input), input);
    }

    #[test]
    fn sanitize_drops_codex_info_tracing_lines() {
        // The exact pattern that flooded the failure-comment field —
        // codex's per-event INFO log, ANSI-colored. Should be dropped.
        let input =
            "\x1b[2m2026-05-03T07:27:06.636239Z\x1b[0m \x1b[32m INFO\x1b[0m \
             session_loop{thread_id=abc}: codex.sse_event";
        assert_eq!(sanitize_stderr_line(input), None);
    }

    #[test]
    fn sanitize_drops_debug_and_trace() {
        for level in ["DEBUG", "TRACE"] {
            let input = format!("2026-05-03T07:27:06.636239Z {level} something");
            assert_eq!(
                sanitize_stderr_line(&input),
                None,
                "should drop {level}",
            );
        }
    }

    #[test]
    fn sanitize_keeps_warn_and_error_tracing_lines() {
        // Real diagnostic content — keep it. (`tracing-subscriber` pads
        // the level to width 5, so WARN gets a leading space.)
        let warn = "\x1b[2m2026-05-03T07:27:06.636239Z\x1b[0m  WARN \
                    something: deprecated foo@1.0.0";
        let err = "2026-05-03T07:27:06.636239Z ERROR network: timed out";
        assert!(sanitize_stderr_line(warn).is_some());
        assert!(sanitize_stderr_line(err).is_some());
    }

    #[test]
    fn sanitize_keeps_unstructured_stderr() {
        // Panics, raw error messages, gh CLI output — anything that
        // doesn't match the tracing format passes through.
        let panic = "thread 'main' panicked at 'oh no', src/main.rs:42";
        let gh = "gh: error: HTTP 401: Bad credentials";
        assert!(sanitize_stderr_line(panic).is_some());
        assert!(sanitize_stderr_line(gh).is_some());
    }

    #[test]
    fn sanitize_drops_empty_lines() {
        assert_eq!(sanitize_stderr_line(""), None);
        assert_eq!(sanitize_stderr_line("   "), None);
        assert_eq!(sanitize_stderr_line("\x1b[0m\x1b[2m"), None);
    }

    #[test]
    fn buffer_skips_filtered_lines() {
        // Integration: if the drain task pushes a noise line, the buffer
        // shouldn't grow.
        let mut buf = StderrBuffer::new(5);
        buf.push("\x1b[2m2026-05-03T00:00:00Z\x1b[0m  INFO noise");
        buf.push("real error: ENOENT /tmp/missing");
        buf.push("\x1b[2m2026-05-03T00:00:01Z\x1b[0m DEBUG noise");
        assert_eq!(buf.snapshot(), vec!["real error: ENOENT /tmp/missing"]);
    }

    #[test]
    fn stderr_buffer_truncates_long_lines() {
        let mut buf = StderrBuffer::new(2);
        let long = "x".repeat(STDERR_LINE_CAP_BYTES * 2);
        buf.push(&long);
        let snap = buf.snapshot();
        assert_eq!(snap.len(), 1);
        // Truncated line ends with a one-char ellipsis sentinel.
        assert!(snap[0].ends_with('…'), "expected ellipsis sentinel: {snap:?}");
        // Truncation respects char-boundary semantics: result fits within cap +
        // the multi-byte ellipsis.
        assert!(snap[0].len() <= STDERR_LINE_CAP_BYTES + 4);
    }

    #[test]
    fn stderr_buffer_handles_multibyte_input() {
        let mut buf = StderrBuffer::new(1);
        // "ä" is 2 bytes; build a string that crosses the cap mid-character.
        let line: String = std::iter::repeat('ä')
            .take(STDERR_LINE_CAP_BYTES)
            .collect();
        buf.push(&line);
        // Should not panic on truncation (regression: naive byte slicing
        // would split a multi-byte sequence).
        assert_eq!(buf.snapshot().len(), 1);
    }

    #[test]
    fn selected_model_prefers_run_override() {
        assert_eq!(
            selected_model(
                Some("claude-override".into()),
                Some("claude-default".into())
            ),
            Some("claude-override".into())
        );
    }

    #[test]
    fn selected_model_falls_back_to_config_default() {
        assert_eq!(
            selected_model(None, Some("claude-default".into())),
            Some("claude-default".into())
        );
    }

    #[test]
    fn selected_model_returns_none_when_unset() {
        assert_eq!(selected_model(None, None), None);
    }
}
