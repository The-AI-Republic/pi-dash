//! Cursor Agent bridge. Drives the `cursor-agent --print --output-format
//! stream-json` subprocess and translates its stream-json events into
//! agent-agnostic [`crate::agent::BridgeEvent`]s.
//!
//! The public surface mirrors `claude_code::bridge::Bridge` so the agent
//! dispatch layer can treat the three agents uniformly:
//!
//! - [`Bridge::spawn`] — prepare the bridge (does **not** launch a subprocess)
//! - [`Bridge::warm`] — return the known resume session id, if any
//! - [`Bridge::run`] — spawn `cursor-agent` for one turn, return a per-run cursor
//! - [`Bridge::next_events`] — pump translated events until the run ends
//! - [`Bridge::send_approval`] — stub for MVP (`--force` is on)
//! - [`Bridge::interrupt`] — cancel the run (SIGINT the child)
//! - [`Bridge::shutdown`] — drain and exit
//!
//! Structural difference from Claude Code: cursor-agent print mode takes the
//! prompt as a positional CLI argument and runs the turn to completion, so the
//! subprocess is one-shot and is **spawned lazily inside `run`** (the prompt
//! isn't known at `spawn` time). The exit-watch channel and stderr ring are
//! created up front so `process_handle()` — which the supervisor captures
//! before the first `run` — observes the real child once it starts. The PID,
//! captured at actual spawn time, is therefore `None` until the first `run`;
//! this only affects the opt-in `agent_observability_v1` snapshot.

use anyhow::{Context, Result};
use chrono::Utc;
use std::collections::VecDeque;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::process::Command;
use tokio::sync::{Mutex, watch};
use uuid::Uuid;

use crate::agent::{
    AgentProcessHandle, BridgeEvent, ExitSnapshot, RunPayload, STDERR_RING_LINES, StderrBuffer,
    StderrRing, StderrSnapshot,
};
use crate::cloud::protocol::{ApprovalDecision, FailureReason};
use crate::cursor_agent::process::CursorProcess;
use crate::cursor_agent::schema::StreamEvent;
use crate::util::shell::login_shell_command;

/// How long to wait for cursor-agent's `system/init` frame before giving up on
/// the run setup. Generous: the CLI can take several seconds to authenticate
/// and emit init when starting cold.
const INIT_TIMEOUT: Duration = Duration::from_secs(30);

pub struct Bridge {
    binary: String,
    model: Option<String>,
    /// `--force` (allow commands unless explicitly denied). Always on for MVP.
    force: bool,
    /// Cursor chat/session id captured from `system/init`, or seeded from
    /// `--resume`. Reused as the `--resume` argument on follow-up turns.
    session_id: Option<String>,
    /// Single exit-watch channel owned by the bridge. Reset to `None` at the
    /// start of each `run` and republished by the spawned process's wait task,
    /// so both `process_handle()` (assignment path) and `bridge_has_exited()`
    /// (chat path) read a consistent liveness signal.
    exit_tx: watch::Sender<Option<ExitSnapshot>>,
    exit_rx: watch::Receiver<Option<ExitSnapshot>>,
    stderr_ring: StderrRing,
    /// The currently-spawned one-shot subprocess, if a `run` is in flight (or
    /// just finished). `None` before the first `run`.
    proc: Option<CursorProcess>,
    /// Events pulled off stdout while waiting synchronously for `system/init`.
    /// Drained by `next_events` before touching the mpsc so no frame is lost.
    pending: VecDeque<StreamEvent>,
}

impl Bridge {
    pub async fn spawn(binary: &str, cwd: &Path, model_default: Option<String>) -> Result<Self> {
        Self::spawn_with_resume(binary, cwd, model_default, None).await
    }

    pub async fn spawn_with_resume(
        binary: &str,
        _cwd: &Path,
        model_default: Option<String>,
        resume_session_id: Option<&str>,
    ) -> Result<Self> {
        let (exit_tx, exit_rx) = watch::channel::<Option<ExitSnapshot>>(None);
        let stderr_ring: StderrRing = Arc::new(Mutex::new(StderrBuffer::new(STDERR_RING_LINES)));
        Ok(Self {
            binary: binary.to_string(),
            model: model_default.filter(|s| !s.is_empty()),
            force: true,
            session_id: resume_session_id
                .filter(|s| !s.is_empty())
                .map(ToOwned::to_owned),
            exit_tx,
            exit_rx,
            stderr_ring,
            proc: None,
            pending: VecDeque::new(),
        })
    }

    /// cursor-agent emits `system/init` only after it has a prompt, so warm
    /// cannot pre-spawn a useful process. Return the known resume session id (if
    /// any) so the cloud can keep its local-session pointer stable until the
    /// first turn lands.
    pub async fn warm(&mut self, _cwd: &Path) -> Result<Option<String>> {
        Ok(self.session_id.clone())
    }

    /// Build the production `cursor-agent` command for a turn.
    fn build_command(&self, prompt: &str, cwd: &Path) -> Command {
        let mut argv: Vec<&str> = vec!["--print", "--output-format", "stream-json"];
        if self.force {
            argv.push("--force");
        }
        if let Some(model) = self.model.as_deref() {
            argv.extend(["--model", model]);
        }
        if let Some(session_id) = self.session_id.as_deref().filter(|s| !s.is_empty()) {
            argv.extend(["--resume", session_id]);
        }
        argv.push(prompt);
        login_shell_command(&self.binary, &argv, Some(cwd))
    }

    /// Spawn the turn's subprocess and wait for `system/init`. Production `run`
    /// builds the command from config; tests inject a fake command directly.
    pub async fn run_with_command(&mut self, cmd: Command, run_id: Uuid) -> Result<BridgeCursor> {
        // Reset the exit signal before the new process can publish into it, so a
        // prior turn's exit snapshot doesn't make this turn look already-dead.
        let _ = self.exit_tx.send(None);
        self.pending.clear();
        let proc =
            CursorProcess::spawn_command(cmd, self.exit_tx.clone(), self.stderr_ring.clone())
                .await?;
        self.proc = Some(proc);
        let thread_id = self.wait_for_init(run_id).await?;
        Ok(BridgeCursor {
            run_id,
            thread_id,
            model: self.model.clone(),
            terminal: false,
            seq: 0,
        })
    }

    pub async fn run(&mut self, payload: &RunPayload, cwd: &Path) -> Result<BridgeCursor> {
        // Prefer a per-run model override if the supervisor supplied one.
        if let Some(m) = payload.model.as_deref().filter(|s| !s.is_empty()) {
            self.model = Some(m.to_string());
        }
        let cmd = self.build_command(&payload.prompt, cwd);
        self.run_with_command(cmd, payload.run_id).await
    }

    /// cursor-agent print mode is inherently one-shot: the prompt rides in argv
    /// and the process emits `result` then exits on its own — there is no stdin
    /// to close. So one-shot and chat `run` are identical at the process level.
    pub async fn run_one_shot(&mut self, payload: &RunPayload, cwd: &Path) -> Result<BridgeCursor> {
        self.run(payload, cwd).await
    }

    async fn wait_for_init(&mut self, run_id: Uuid) -> Result<String> {
        let proc = self
            .proc
            .as_mut()
            .context("cursor-agent process not spawned")?;
        let deadline = tokio::time::Instant::now() + INIT_TIMEOUT;
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            let recv = tokio::time::timeout(remaining, proc.inbound.recv())
                .await
                .context("timed out waiting for cursor-agent system/init")?;
            match recv {
                Some(StreamEvent::System(ref sys)) if sys.subtype == "init" => {
                    let thread_id = sys
                        .session_id
                        .clone()
                        .unwrap_or_else(|| format!("cursor-{run_id}"));
                    self.model = sys
                        .rest
                        .get("model")
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.is_empty())
                        .map(ToOwned::to_owned)
                        .or_else(|| self.model.clone());
                    self.session_id = Some(thread_id.clone());
                    return Ok(thread_id);
                }
                Some(other) => self.pending.push_back(other),
                None => {
                    anyhow::bail!("cursor-agent stdout closed before emitting system/init")
                }
            }
        }
    }

    /// Pull the next event off the subprocess (or the pre-init buffer) and
    /// translate it. Returns `None` once the stdout stream closes for good;
    /// callers treat that as EOF and exit their pump loop.
    pub async fn next_events(&mut self, cursor: &mut BridgeCursor) -> Option<Vec<BridgeEvent>> {
        loop {
            let ev = if let Some(buffered) = self.pending.pop_front() {
                buffered
            } else {
                self.proc.as_mut()?.inbound.recv().await?
            };
            let translated = cursor.translate(ev);
            if !translated.is_empty() {
                return Some(translated);
            }
        }
    }

    /// Approvals aren't wired for cursor-agent in the MVP (`--force` is set, so
    /// the subprocess never asks). Reaching this is a programmer error; fail
    /// fast so the supervisor surfaces the bug instead of silently dropping the
    /// operator's decision.
    pub async fn send_approval(
        &mut self,
        approval_id: &str,
        _decision: ApprovalDecision,
    ) -> Result<()> {
        tracing::error!(
            approval_id,
            "cursor_agent bridge received an approval decision but approvals are \
             not wired (--force is on); refusing to silently drop it"
        );
        anyhow::bail!(
            "cursor_agent bridge received approval {approval_id} but approvals are \
             not wired in MVP"
        );
    }

    pub async fn interrupt(&mut self) -> Result<()> {
        match self.proc.as_mut() {
            Some(proc) => proc.interrupt().await,
            None => Ok(()),
        }
    }

    pub async fn shutdown(self, grace: Duration) -> Result<()> {
        if let Some(proc) = self.proc {
            proc.shutdown(grace, self.exit_rx.clone()).await
        } else {
            Ok(())
        }
    }

    pub fn process_handle(&self) -> AgentProcessHandle {
        AgentProcessHandle {
            pid: self.proc.as_ref().and_then(|p| p.pid()),
            exit_rx: self.exit_rx.clone(),
        }
    }

    pub async fn recent_stderr(&self) -> StderrSnapshot {
        self.stderr_ring.lock().await.snapshot()
    }
}

/// Per-run translation state. The session-level `system/init` frame is consumed
/// by the bridge before the cursor is returned, so every cursor has a populated
/// `thread_id` matching the Codex / Claude bridge contract.
pub struct BridgeCursor {
    pub run_id: Uuid,
    pub thread_id: String,
    pub model: Option<String>,
    /// Flipped once we see a terminal `result` frame. Suppresses any trailing
    /// frames a stubborn subprocess might emit after completion.
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
                // `init` was already consumed during run setup; drop any repeat.
                if sys.subtype == "init" {
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
                method: "user/message".into(),
                params: u.message,
            }],
            StreamEvent::ToolCall(t) => {
                let params = serde_json::to_value(&t.rest).unwrap_or(serde_json::Value::Null);
                vec![BridgeEvent::Raw {
                    run_id: self.run_id,
                    method: format!("tool_call/{}", t.subtype),
                    params,
                }]
            }
            StreamEvent::Result(r) => {
                self.terminal = true;
                let is_err = r.is_error.unwrap_or(false) || r.subtype.starts_with("error");
                if is_err {
                    let detail = r
                        .result
                        .clone()
                        .or_else(|| Some(format!("cursor-agent result subtype: {}", r.subtype)));
                    vec![BridgeEvent::Failed {
                        run_id: self.run_id,
                        reason: classify_failure(&r.subtype),
                        detail,
                    }]
                } else {
                    let done_payload = serde_json::json!({
                        "conclusion": r.subtype,
                        "result": r.result,
                        "duration_ms": r.duration_ms,
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

/// Best-effort mapping from cursor-agent's `result.subtype` to our
/// `FailureReason`. cursor-agent has no documented turn-budget subtype, so all
/// error results map to the generic `AgentCrash` (kept distinct from
/// `CodexCrash` so cursor failures don't pollute Codex telemetry).
fn classify_failure(_subtype: &str) -> FailureReason {
    FailureReason::AgentCrash
}
