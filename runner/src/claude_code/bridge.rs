//! Claude Code bridge. Drives the `claude --print` subprocess for one run
//! and translates stream-json events into agent-agnostic
//! [`crate::agent::BridgeEvent`]s.
//!
//! The public surface intentionally mirrors `codex::bridge::Bridge` so the
//! agent dispatch layer can treat the two uniformly:
//!
//! - [`Bridge::spawn`] — launch the subprocess
//! - [`Bridge::run`] — feed the user prompt, return a per-run cursor
//! - [`Bridge::next_events`] — pump translated events until the run ends
//! - [`Bridge::send_approval`] — stub for MVP (bypassPermissions is on)
//! - [`Bridge::interrupt`] — cancel the run (SIGINT the child)
//! - [`Bridge::shutdown`] — drain and exit

use anyhow::{Context, Result};
use chrono::Utc;
use std::collections::VecDeque;
use std::path::Path;
use std::time::Duration;
use uuid::Uuid;

use crate::agent::{BridgeEvent, RunPayload};
use crate::claude_code::process::{ClaudeProcess, SpawnArgs};
use crate::claude_code::schema::{StreamEvent, UserInput};
use crate::cloud::protocol::{ApprovalDecision, FailureReason};

/// How long to wait for Claude's `system/init` frame before giving up on
/// the run setup. Chosen to be generous: Claude can take several seconds to
/// emit init when starting cold.
const INIT_TIMEOUT: Duration = Duration::from_secs(30);

pub struct Bridge {
    proc: ClaudeProcess,
    /// Events received while we were waiting synchronously for `system/init`
    /// inside `run()`. Drained by `next_events` before touching the mpsc so
    /// no frame is lost across the run-setup boundary.
    pending: VecDeque<StreamEvent>,
}

impl Bridge {
    pub async fn spawn(binary: &str, cwd: &Path, model_default: Option<String>) -> Result<Self> {
        let proc = ClaudeProcess::spawn(SpawnArgs {
            binary,
            cwd,
            model: model_default.as_deref(),
            bypass_permissions: true,
        })
        .await?;
        Ok(Self {
            proc,
            pending: VecDeque::new(),
        })
    }

    /// Test-friendly constructor that wraps an already-built `ClaudeProcess`
    /// (typically a shell-script fake).
    pub fn from_process(proc: ClaudeProcess, model_default: Option<String>) -> Self {
        let _ = model_default;
        Self {
            proc,
            pending: VecDeque::new(),
        }
    }

    /// Send the prompt and block until Claude emits its `system/init` frame
    /// so the returned cursor carries a populated `thread_id`, matching the
    /// Codex bridge's contract. Frames that arrive before init (rare but
    /// allowed by the protocol) are buffered and replayed by `next_events`.
    pub async fn run(&mut self, payload: &RunPayload, _cwd: &Path) -> Result<BridgeCursor> {
        let input = UserInput::user_text(&payload.prompt);
        let line = serde_json::to_string(&input)?;
        self.proc.send_line(&line).await?;
        // Half-close so Claude processes the turn and exits when done.
        self.proc.close_stdin();

        let deadline = tokio::time::Instant::now() + INIT_TIMEOUT;
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            let recv = tokio::time::timeout(remaining, self.proc.inbound.recv())
                .await
                .context("timed out waiting for claude system/init")?;
            match recv {
                Some(StreamEvent::System(ref sys)) if sys.subtype == "init" => {
                    let thread_id = sys
                        .session_id
                        .clone()
                        .unwrap_or_else(|| format!("claude-{}", payload.run_id));
                    return Ok(BridgeCursor {
                        run_id: payload.run_id,
                        thread_id,
                        init_consumed: true,
                        terminal: false,
                        seq: 0,
                    });
                }
                Some(other) => self.pending.push_back(other),
                None => anyhow::bail!("claude stdout closed before emitting system/init"),
            }
        }
    }

    /// Pull the next event off the subprocess (or the pre-init buffer) and
    /// translate it. Returns `None` once the stdout stream closes for good;
    /// callers should treat that as EOF and exit their pump loop.
    pub async fn next_events(&mut self, cursor: &mut BridgeCursor) -> Option<Vec<BridgeEvent>> {
        loop {
            let ev = if let Some(buffered) = self.pending.pop_front() {
                buffered
            } else {
                self.proc.inbound.recv().await?
            };
            let translated = cursor.translate(ev);
            // Skip empty translations (e.g. duplicate init frames that get
            // suppressed) so the supervisor isn't repeatedly woken for no
            // reason.
            if !translated.is_empty() {
                return Some(translated);
            }
        }
    }

    /// Approvals aren't wired for Claude in the MVP (bypassPermissions is
    /// set, so the subprocess never asks). Reaching this is a programmer
    /// error — either the bypass flag was flipped without wiring the
    /// permission-prompt MCP bridge, or the dispatch layer misrouted an
    /// approval. Fail fast so the supervisor surfaces the bug instead of
    /// silently dropping the operator's decision.
    pub async fn send_approval(
        &mut self,
        approval_id: &str,
        _decision: ApprovalDecision,
    ) -> Result<()> {
        tracing::error!(
            approval_id,
            "claude_code bridge received an approval decision but approvals are \
             not wired (bypassPermissions=true); refusing to silently drop it"
        );
        anyhow::bail!(
            "claude_code bridge received approval {approval_id} but approvals are \
             not wired in MVP"
        );
    }

    pub async fn interrupt(&mut self) -> Result<()> {
        self.proc.interrupt().await
    }

    pub async fn shutdown(self, grace: Duration) -> Result<()> {
        self.proc.shutdown(grace).await
    }
}

/// Per-run translation state. The `system/init` frame is consumed inside
/// `Bridge::run`, so by the time the cursor is handed to the supervisor the
/// `thread_id` is already populated (matching Codex's lifecycle order).
pub struct BridgeCursor {
    pub run_id: Uuid,
    pub thread_id: String,
    /// True once `Bridge::run` has eaten the leading `init` frame. A second
    /// `system/init` is silently dropped — Claude shouldn't emit one but we
    /// defend against it anyway.
    init_consumed: bool,
    /// Flipped once we see a terminal `result` frame. Used to suppress any
    /// trailing frames a stubborn subprocess might emit after completion.
    terminal: bool,
    pub seq: u64,
}

impl BridgeCursor {
    pub fn translate(&mut self, ev: StreamEvent) -> Vec<BridgeEvent> {
        if self.terminal {
            return Vec::new();
        }
        self.seq = self.seq.saturating_add(1);

        match ev {
            StreamEvent::System(sys) => {
                // Duplicate init is a no-op; the real init is handled in
                // `Bridge::run` so the cursor's `thread_id` is always set.
                if sys.subtype == "init" && self.init_consumed {
                    return Vec::new();
                }
                let params = serde_json::to_value(&sys.rest).unwrap_or(serde_json::Value::Null);
                vec![BridgeEvent::Raw {
                    run_id: self.run_id,
                    method: format!("system/{}", sys.subtype),
                    params,
                }]
            }
            StreamEvent::Assistant(a) => vec![BridgeEvent::Raw {
                run_id: self.run_id,
                method: "assistant/message".into(),
                params: a.message,
            }],
            StreamEvent::User(u) => vec![BridgeEvent::Raw {
                run_id: self.run_id,
                method: "user/toolResult".into(),
                params: u.message,
            }],
            StreamEvent::Result(r) => {
                self.terminal = true;
                let is_err = r.is_error.unwrap_or(false) || r.subtype.starts_with("error");
                if is_err {
                    let detail = r
                        .result
                        .clone()
                        .or_else(|| Some(format!("claude result subtype: {}", r.subtype)));
                    vec![BridgeEvent::Failed {
                        run_id: self.run_id,
                        reason: classify_failure(&r.subtype),
                        detail,
                    }]
                } else {
                    let done_payload = serde_json::json!({
                        "conclusion": r.subtype,
                        "result": r.result,
                        "total_cost_usd": r.total_cost_usd,
                        "usage": r.usage,
                        "ended_at": Utc::now().to_rfc3339(),
                    });
                    vec![BridgeEvent::Completed {
                        run_id: self.run_id,
                        done_payload,
                    }]
                }
            }
            StreamEvent::Unknown(v) => vec![BridgeEvent::Raw {
                run_id: self.run_id,
                method: "unknown".into(),
                params: v,
            }],
        }
    }
}

/// Best-effort mapping from Claude's `result.subtype` to our
/// `FailureReason`. `error_max_turns` is budget exhaustion (not an internal
/// bug) so it gets its own variant. Everything else — including
/// `error_during_execution` — is a generic agent crash; we use `AgentCrash`
/// rather than `CodexCrash` so Claude failures don't pollute Codex telemetry.
fn classify_failure(subtype: &str) -> FailureReason {
    match subtype {
        "error_max_turns" => FailureReason::MaxTurns,
        _ => FailureReason::AgentCrash,
    }
}
