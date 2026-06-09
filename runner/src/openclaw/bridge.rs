//! OpenClaw bridge. Drives the OpenClaw coding agent over the Agent Client
//! Protocol (ACP) by shelling out to the headless ACP client `acpx`:
//!
//! ```text
//! acpx --cwd <dir> --format json --approve-all openclaw exec -- "<prompt>"
//! ```
//!
//! `acpx` performs the ACP handshake with OpenClaw (`initialize` →
//! `session/new {cwd}` → `session/prompt`), streams the agent's work back as
//! raw ACP JSON-RPC NDJSON on stdout, and exits when the turn ends. We consume
//! that NDJSON and translate it into agent-agnostic
//! [`crate::agent::BridgeEvent`]s.
//!
//! The public surface mirrors `cursor_agent::bridge::Bridge` so the agent
//! dispatch layer can treat all four agents uniformly. Like cursor-agent (and
//! unlike Codex / Claude), the prompt rides in argv and the process is
//! **one-shot per turn**, spawned lazily inside `run` once the prompt is known.
//!
//! MVP limitations (tracked as follow-ups):
//! - **Approvals bypass**: runs with `--approve-all`, mirroring the cursor and
//!   Claude bridges' bypass posture. Wiring per-tool approval requires driving
//!   `openclaw acp` natively (a persistent bidirectional ACP session against a
//!   running OpenClaw Gateway, with `session/request_permission` answered from
//!   `runner/src/approval`) rather than shelling out to `acpx`; that is the
//!   documented next step.
//! - **Stateless turns**: `acpx ... exec` creates a temporary ACP session and
//!   does not persist a session record, so each turn is independent (no
//!   `--resume` continuity). A known session id is reused only as the run's
//!   thread-id seed.

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
use crate::openclaw::process::OpenClawProcess;
use crate::openclaw::schema::AcpMessage;
use crate::util::shell::login_shell_command;

/// How long to wait for `acpx` to emit its first ACP frame carrying a
/// `sessionId` before giving up on run setup. Generous: acpx has to perform
/// the ACP handshake and OpenClaw may take several seconds to spin a session.
const SESSION_TIMEOUT: Duration = Duration::from_secs(30);

pub struct Bridge {
    binary: String,
    model: Option<String>,
    /// `--approve-all` (auto-approve every ACP permission request). Always on
    /// for the MVP, mirroring the cursor bridge's `--force` posture.
    approve_all: bool,
    /// ACP session id captured from the first frame that carries one, or
    /// seeded from `--resume`. Surfaced as the run's thread id.
    session_id: Option<String>,
    /// Single exit-watch channel owned by the bridge. Reset to `None` at the
    /// start of each `run` and republished by the spawned process's wait task,
    /// so both `process_handle()` and liveness checks read a consistent signal.
    exit_tx: watch::Sender<Option<ExitSnapshot>>,
    exit_rx: watch::Receiver<Option<ExitSnapshot>>,
    stderr_ring: StderrRing,
    /// The currently-spawned one-shot subprocess, if a `run` is in flight (or
    /// just finished). `None` before the first `run`.
    proc: Option<OpenClawProcess>,
    /// Frames pulled off stdout while waiting synchronously for the first
    /// session-bearing frame. Drained by `next_events` before touching the
    /// mpsc so no frame is lost.
    pending: VecDeque<AcpMessage>,
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
            approve_all: true,
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

    /// acpx emits ACP frames only after it has a prompt, so warm cannot
    /// pre-spawn a useful process. Return the known resume session id (if any)
    /// so the cloud can keep its local-session pointer stable until the first
    /// turn lands.
    pub async fn warm(&mut self, _cwd: &Path) -> Result<Option<String>> {
        Ok(self.session_id.clone())
    }

    /// Build the production `acpx` command for a turn. Global options precede
    /// the `openclaw exec` subcommand (per the acpx CLI); the prompt is the
    /// final positional argument.
    fn build_command(&self, prompt: &str, cwd: &Path) -> Command {
        let cwd_str = cwd.to_string_lossy();
        let mut argv: Vec<&str> = vec!["--cwd", &cwd_str, "--format", "json"];
        if self.approve_all {
            argv.push("--approve-all");
        }
        if let Some(model) = self.model.as_deref() {
            argv.extend(["--model", model]);
        }
        argv.extend(["openclaw", "exec"]);
        // `--` terminates option parsing so a prompt that legitimately starts
        // with a dash (a markdown rule `---`, a leading bullet `- ...`, or
        // pasted CLI/diff text) is taken as the positional prompt instead of
        // being misread as flags. Like cursor-agent, acpx takes the prompt in
        // argv, so this guard is load-bearing.
        argv.push("--");
        argv.push(prompt);
        login_shell_command(&self.binary, &argv, Some(cwd))
    }

    /// Spawn the turn's subprocess and wait for the first session-bearing
    /// frame. Production `run` builds the command from config; tests inject a
    /// fake command directly.
    pub async fn run_with_command(&mut self, cmd: Command, run_id: Uuid) -> Result<BridgeCursor> {
        // Reset the exit signal before the new process can publish into it, so
        // a prior turn's exit snapshot doesn't make this turn look already-dead.
        // Use `send_if_modified` so a no-op reset (a fresh bridge whose value
        // is already `None`) does NOT fire a watch notification that the
        // supervisor (which subscribed before calling `run`) would misread as
        // an immediate exit. Only a genuine `Some -> None` clear publishes.
        self.exit_tx.send_if_modified(|v| {
            if v.is_some() {
                *v = None;
                true
            } else {
                false
            }
        });
        self.pending.clear();
        let proc =
            OpenClawProcess::spawn_command(cmd, self.exit_tx.clone(), self.stderr_ring.clone())
                .await?;
        self.proc = Some(proc);
        let thread_id = self.wait_for_session(run_id).await?;
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

    /// acpx exec is inherently one-shot: the prompt rides in argv and the
    /// process emits its terminal `session/prompt` response then exits on its
    /// own. So one-shot and chat `run` are identical at the process level.
    pub async fn run_one_shot(&mut self, payload: &RunPayload, cwd: &Path) -> Result<BridgeCursor> {
        self.run(payload, cwd).await
    }

    /// Pull frames until one carries an ACP `sessionId` (the
    /// `session/prompt` request, the first `session/update`, or the
    /// `session/new` response all do). Buffer everything seen along the way so
    /// `next_events` re-emits it. Mirrors cursor-agent's `wait_for_init`.
    async fn wait_for_session(&mut self, run_id: Uuid) -> Result<String> {
        let proc = self.proc.as_mut().context("acpx process not spawned")?;
        let deadline = tokio::time::Instant::now() + SESSION_TIMEOUT;
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            let recv = tokio::time::timeout(remaining, proc.inbound.recv())
                .await
                .context("timed out waiting for acpx ACP session frame")?;
            match recv {
                Some(frame) => {
                    if let Some(sid) = frame.session_id() {
                        self.session_id = Some(sid.clone());
                        self.pending.push_back(frame);
                        return Ok(sid);
                    }
                    // acpx can fail before any session is established (auth,
                    // gateway, or quota error): it skips straight to a terminal
                    // error / stop frame. Buffer it, synthesize a thread id, and
                    // return so the normal pump translates it into a `Failed` /
                    // `Completed` event with the real detail intact.
                    let is_terminal = matches!(&frame, AcpMessage::Error { .. })
                        || matches!(&frame, AcpMessage::Response { result }
                            if result.get("stopReason").is_some());
                    self.pending.push_back(frame);
                    if is_terminal {
                        let thread_id = format!("openclaw-{run_id}");
                        self.session_id = Some(thread_id.clone());
                        return Ok(thread_id);
                    }
                }
                None => anyhow::bail!("acpx stdout closed before emitting an ACP session frame"),
            }
        }
    }

    /// Pull the next event off the subprocess (or the pre-session buffer) and
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

    /// Approvals aren't wired for OpenClaw in the MVP (`--approve-all` is set,
    /// so acpx auto-answers every ACP `session/request_permission` and never
    /// relays one to us). Reaching this is a programmer error; fail fast so the
    /// supervisor surfaces the bug instead of silently dropping the decision.
    pub async fn send_approval(
        &mut self,
        approval_id: &str,
        _decision: ApprovalDecision,
    ) -> Result<()> {
        tracing::error!(
            approval_id,
            "openclaw bridge received an approval decision but approvals are \
             not wired (--approve-all is on); refusing to silently drop it"
        );
        anyhow::bail!(
            "openclaw bridge received approval {approval_id} but approvals are \
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

/// Per-run translation state. The first session-bearing frame is consumed by
/// the bridge before the cursor is returned, so every cursor has a populated
/// `thread_id` matching the Codex / Claude / Cursor bridge contract.
pub struct BridgeCursor {
    pub run_id: Uuid,
    pub thread_id: String,
    pub model: Option<String>,
    /// Flipped once we see a terminal frame (the `session/prompt` response or a
    /// JSON-RPC error). Suppresses any trailing frames acpx might emit after.
    terminal: bool,
    pub seq: u64,
}

impl BridgeCursor {
    pub fn translate(&mut self, ev: AcpMessage) -> Vec<BridgeEvent> {
        if self.terminal {
            return Vec::new();
        }
        self.seq = self.seq.saturating_add(1);

        match ev {
            AcpMessage::Method { method, params } => {
                // Label `session/update` frames with their `sessionUpdate`
                // discriminator so history shows the kind without re-parsing.
                let label = if method == "session/update" {
                    match AcpMessage::session_update_kind(&params) {
                        Some(kind) => format!("session/update/{kind}"),
                        None => method,
                    }
                } else {
                    method
                };
                vec![BridgeEvent::Raw {
                    run_id: self.run_id,
                    method: label,
                    params,
                }]
            }
            AcpMessage::Response { result } => {
                // The `session/prompt` response carries `stopReason`; that's the
                // end of the turn. Other responses (initialize, session/new) are
                // preserved as Raw for history.
                match result.get("stopReason").and_then(|s| s.as_str()) {
                    Some(stop_reason) => {
                        self.terminal = true;
                        if is_failure_stop_reason(stop_reason) {
                            vec![BridgeEvent::Failed {
                                run_id: self.run_id,
                                reason: FailureReason::AgentCrash,
                                detail: Some(format!("openclaw stopReason: {stop_reason}")),
                            }]
                        } else {
                            let done_payload = serde_json::json!({
                                "conclusion": stop_reason,
                                "stop_reason": stop_reason,
                                "ended_at": Utc::now().to_rfc3339(),
                            });
                            vec![BridgeEvent::Completed {
                                run_id: self.run_id,
                                done_payload,
                            }]
                        }
                    }
                    None => vec![BridgeEvent::Raw {
                        run_id: self.run_id,
                        method: "response".into(),
                        params: result,
                    }],
                }
            }
            AcpMessage::Error { error } => {
                self.terminal = true;
                let detail = error
                    .get("message")
                    .and_then(|m| m.as_str())
                    .map(ToOwned::to_owned)
                    .or_else(|| Some(format!("openclaw ACP error: {error}")));
                vec![BridgeEvent::Failed {
                    run_id: self.run_id,
                    reason: FailureReason::AgentCrash,
                    detail,
                }]
            }
            AcpMessage::Unknown(v) => vec![BridgeEvent::Raw {
                run_id: self.run_id,
                method: "unknown".into(),
                params: v,
            }],
        }
    }
}

/// ACP `stopReason` values that mean the turn did not complete its work:
/// `cancelled` (interrupted) and `refusal` (the model declined). Everything
/// else (`end_turn`, `max_tokens`, `max_turn_requests`) is a natural end and
/// maps to `Completed` with the reason recorded as the conclusion.
fn is_failure_stop_reason(stop_reason: &str) -> bool {
    matches!(stop_reason, "cancelled" | "refusal")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv_of(cmd: &Command) -> Vec<String> {
        cmd.as_std()
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    #[tokio::test]
    async fn build_command_terminates_options_before_prompt() {
        let bridge = Bridge::spawn("acpx", Path::new("/tmp"), None)
            .await
            .expect("bridge setup");
        // A prompt that begins with a dash must not be parsed as a flag.
        let cmd = bridge.build_command("--help me refactor", Path::new("/tmp"));
        let argv = argv_of(&cmd);
        let sep = argv
            .iter()
            .position(|a| a == "--")
            .expect("expected a `--` option terminator in argv");
        assert_eq!(argv.last().map(String::as_str), Some("--help me refactor"));
        assert_eq!(sep, argv.len() - 2, "`--` must directly precede the prompt");
        // The acpx invocation drives the `openclaw exec` subcommand in json mode.
        assert!(argv.iter().any(|a| a == "openclaw"));
        assert!(argv.iter().any(|a| a == "exec"));
        assert!(argv.iter().any(|a| a == "--format"));
        assert!(argv.iter().any(|a| a == "--approve-all"));
    }

    #[tokio::test]
    async fn build_command_includes_model_when_set() {
        let bridge = Bridge::spawn("acpx", Path::new("/tmp"), Some("claude-x".into()))
            .await
            .expect("bridge setup");
        let cmd = bridge.build_command("hi", Path::new("/tmp"));
        let argv = argv_of(&cmd);
        let i = argv
            .iter()
            .position(|a| a == "--model")
            .expect("--model present");
        assert_eq!(argv.get(i + 1).map(String::as_str), Some("claude-x"));
    }

    fn cursor() -> BridgeCursor {
        BridgeCursor {
            run_id: Uuid::nil(),
            thread_id: "s1".into(),
            model: None,
            terminal: false,
            seq: 0,
        }
    }

    fn msg(line: &str) -> AcpMessage {
        serde_json::from_str(line).unwrap()
    }

    #[test]
    fn translates_session_update_to_raw_with_kind_label() {
        let mut c = cursor();
        let ev = msg(
            r#"{"method":"session/update","params":{"sessionId":"s1","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"hi"}}}}"#,
        );
        let out = c.translate(ev);
        match &out[0] {
            BridgeEvent::Raw { method, .. } => {
                assert_eq!(method, "session/update/agent_message_chunk")
            }
            other => panic!("expected Raw, got {other:?}"),
        }
    }

    #[test]
    fn stop_reason_end_turn_completes() {
        let mut c = cursor();
        let out = c.translate(msg(r#"{"id":3,"result":{"stopReason":"end_turn"}}"#));
        assert!(matches!(out[0], BridgeEvent::Completed { .. }));
        // Terminal: subsequent frames are suppressed.
        assert!(
            c.translate(msg(r#"{"method":"session/update","params":{}}"#))
                .is_empty()
        );
    }

    #[test]
    fn stop_reason_cancelled_fails() {
        let mut c = cursor();
        let out = c.translate(msg(r#"{"id":3,"result":{"stopReason":"cancelled"}}"#));
        assert!(matches!(out[0], BridgeEvent::Failed { .. }));
    }

    #[test]
    fn jsonrpc_error_fails_with_detail() {
        let mut c = cursor();
        let out = c.translate(msg(
            r#"{"id":2,"error":{"code":-32603,"message":"gateway down"}}"#,
        ));
        match &out[0] {
            BridgeEvent::Failed { detail, .. } => {
                assert_eq!(detail.as_deref(), Some("gateway down"))
            }
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[test]
    fn non_terminal_response_is_raw() {
        // session/new response (sessionId, no stopReason) → preserved as Raw.
        let mut c = cursor();
        let out = c.translate(msg(r#"{"id":1,"result":{"sessionId":"s1"}}"#));
        assert!(matches!(out[0], BridgeEvent::Raw { .. }));
    }
}
