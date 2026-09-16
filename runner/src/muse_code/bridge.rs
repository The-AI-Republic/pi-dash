//! Muse Code bridge. Drives the `muse exec --json --prompt-file <PATH>`
//! subprocess (Meta Muse Code's headless one-shot mode) and translates its
//! JSONL events into agent-agnostic [`crate::agent::BridgeEvent`]s.
//!
//! The public surface mirrors `cursor_agent::bridge::Bridge` so the agent
//! dispatch layer can treat every backend uniformly:
//!
//! - [`Bridge::spawn`] / [`Bridge::spawn_with_resume`] — prepare the bridge
//!   (does **not** launch a subprocess)
//! - [`Bridge::warm`] — return the known resume session id, if any
//! - [`Bridge::run`] — spawn `muse exec` for one turn, return a per-run cursor
//! - [`Bridge::next_events`] — pump translated events until the run ends
//! - [`Bridge::send_approval`] — stub for MVP (`--yolo` is on)
//! - [`Bridge::interrupt`] — cancel the run (SIGINT the child)
//! - [`Bridge::shutdown`] — drain and exit
//!
//! Structural notes (vs. Cursor):
//! - Muse's `exec` mode is one-shot: it runs one prompt to completion and
//!   exits, so — like Cursor — the subprocess is spawned lazily inside `run`,
//!   reusing the prior `--session-id` for continuity across turns.
//! - The prompt is delivered via a temp file (`--prompt-file`), not argv. See
//!   [`crate::muse_code::process::PromptFile`].
//! - Muse Code is closed-source and its JSONL schema is not fully documented,
//!   so the schema layer (and this translator) are deliberately tolerant: a
//!   `system/init` frame is used when present, but its absence does not break
//!   the run (the first frame of any kind starts it).

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
use crate::muse_code::process::{MuseProcess, PromptFile};
use crate::muse_code::schema::StreamEvent;
use crate::util::shell::login_shell_command;

/// How long to wait for `muse exec`'s first JSONL frame before giving up on
/// the run setup. Generous: the CLI can take several seconds to authenticate
/// against the Meta Model API and emit its first event when starting cold.
const INIT_TIMEOUT: Duration = Duration::from_secs(30);

pub struct Bridge {
    binary: String,
    model: Option<String>,
    /// `--yolo` (disable approval prompts and sandbox). Always on for MVP,
    /// mirroring the Cursor bridge's `--force` posture — a real approval loop
    /// is a follow-up.
    yolo: bool,
    /// Muse session id captured from the first frame, or seeded from a prior
    /// turn's `--session-id`. Reused as the `--session-id` argument on
    /// follow-up turns for conversational continuity.
    session_id: Option<String>,
    /// Single exit-watch channel owned by the bridge. Reset to `None` at the
    /// start of each `run` and republished by the spawned process's wait task,
    /// so both `process_handle()` and `shutdown()` read a consistent liveness
    /// signal. See the Cursor bridge for the full rationale.
    exit_tx: watch::Sender<Option<ExitSnapshot>>,
    exit_rx: watch::Receiver<Option<ExitSnapshot>>,
    stderr_ring: StderrRing,
    /// The currently-spawned one-shot subprocess, if a `run` is in flight (or
    /// just finished). `None` before the first `run`.
    proc: Option<MuseProcess>,
    /// Events pulled off stdout while waiting synchronously for the first
    /// frame. Drained by `next_events` before touching the mpsc so no frame is
    /// lost.
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
            yolo: true,
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

    /// `muse exec` only starts emitting once it has a prompt, so warm cannot
    /// pre-spawn a useful process. Return the known resume session id (if any)
    /// so the cloud can keep its local-session pointer stable until the first
    /// turn lands.
    pub async fn warm(&mut self, _cwd: &Path) -> Result<Option<String>> {
        Ok(self.session_id.clone())
    }

    /// Build the production `muse exec` command for a turn. The prompt lives in
    /// `prompt_path` (a temp file the caller owns for the process lifetime).
    fn build_command(&self, prompt_path: &Path, cwd: &Path) -> Command {
        let prompt_arg = prompt_path.to_string_lossy();
        let mut argv: Vec<&str> = vec!["exec", "--json", "--prompt-file", &prompt_arg];
        if self.yolo {
            argv.push("--yolo");
        }
        if let Some(model) = self.model.as_deref() {
            argv.extend(["--model", model]);
        }
        if let Some(session_id) = self.session_id.as_deref().filter(|s| !s.is_empty()) {
            argv.extend(["--session-id", session_id]);
        }
        login_shell_command(&self.binary, &argv, Some(cwd))
    }

    /// Spawn the turn's subprocess and wait for its first frame. Production
    /// `run` builds the command from config and supplies the prompt file;
    /// tests inject a fake command directly with `prompt_file: None`.
    pub async fn run_with_command(
        &mut self,
        cmd: Command,
        prompt_file: Option<PromptFile>,
        run_id: Uuid,
    ) -> Result<BridgeCursor> {
        // Reset the exit signal before the new process can publish into it, so a
        // prior turn's exit snapshot doesn't make this turn look already-dead.
        // Use `send_if_modified` so a no-op reset (a fresh bridge whose value is
        // already `None`) does NOT fire a watch notification that a subscriber
        // captured before `run` would misread as "already exited". Only a
        // genuine `Some -> None` clear (a reused bridge) publishes a change.
        self.exit_tx.send_if_modified(|v| {
            if v.is_some() {
                *v = None;
                true
            } else {
                false
            }
        });
        self.pending.clear();
        let proc = MuseProcess::spawn_command(
            cmd,
            prompt_file,
            self.exit_tx.clone(),
            self.stderr_ring.clone(),
        )
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
        let prompt_file =
            PromptFile::create(&payload.prompt).context("preparing muse prompt file")?;
        let cmd = self.build_command(prompt_file.path(), cwd);
        self.run_with_command(cmd, Some(prompt_file), payload.run_id)
            .await
    }

    /// `muse exec` is inherently one-shot: it reads the prompt file, runs to
    /// completion, emits `result`, then exits — there is no stdin to close. So
    /// one-shot and chat `run` are identical at the process level.
    pub async fn run_one_shot(&mut self, payload: &RunPayload, cwd: &Path) -> Result<BridgeCursor> {
        self.run(payload, cwd).await
    }

    /// Wait for the first JSONL frame so the returned cursor carries a
    /// populated `thread_id` (matching the Codex / Claude / Cursor contract).
    /// A `system/init` frame is preferred (it carries `session_id` + `model`),
    /// but Muse's schema is not guaranteed to emit one, so any first frame
    /// starts the run: we capture its `session_id` when present and otherwise
    /// synthesize a stable id.
    async fn wait_for_init(&mut self, run_id: Uuid) -> Result<String> {
        let proc = self
            .proc
            .as_mut()
            .context("muse exec process not spawned")?;
        let deadline = tokio::time::Instant::now() + INIT_TIMEOUT;
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        let recv = tokio::time::timeout(remaining, proc.inbound.recv())
            .await
            .context("timed out waiting for muse exec's first frame")?;
        match recv {
            Some(StreamEvent::System(ref sys)) if sys.subtype == "init" => {
                let thread_id = sys
                    .session_id
                    .clone()
                    .unwrap_or_else(|| format!("muse-{run_id}"));
                self.model = sys
                    .rest
                    .get("model")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .map(ToOwned::to_owned)
                    .or_else(|| self.model.clone());
                self.session_id = Some(thread_id.clone());
                Ok(thread_id)
            }
            Some(ev) => {
                // No init frame (or Muse skipped straight to output / a terminal
                // `result` on an auth or quota error). Adopt any session id the
                // frame carries, else synthesize one, then buffer the frame so
                // the normal pump translates it — keeping the real error /
                // content instead of dropping it.
                let sid = session_id_of(&ev);
                let thread_id = sid.unwrap_or_else(|| format!("muse-{run_id}"));
                self.session_id = Some(thread_id.clone());
                self.pending.push_back(ev);
                Ok(thread_id)
            }
            None => anyhow::bail!("muse exec stdout closed before emitting any frame"),
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

    /// Approvals aren't wired for muse exec in the MVP (`--yolo` is set, so the
    /// subprocess never asks). Reaching this is a programmer error; fail fast so
    /// the supervisor surfaces the bug instead of silently dropping the
    /// operator's decision.
    pub async fn send_approval(
        &mut self,
        approval_id: &str,
        _decision: ApprovalDecision,
    ) -> Result<()> {
        tracing::error!(
            approval_id,
            "muse_code bridge received an approval decision but approvals are \
             not wired (--yolo is on); refusing to silently drop it"
        );
        anyhow::bail!(
            "muse_code bridge received approval {approval_id} but approvals are \
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

/// The `session_id` carried by a frame, if any. Used to adopt a thread id when
/// Muse doesn't lead with a `system/init` frame.
fn session_id_of(ev: &StreamEvent) -> Option<String> {
    match ev {
        StreamEvent::System(s) => s.session_id.clone(),
        StreamEvent::Assistant(m) | StreamEvent::User(m) => m.session_id.clone(),
        StreamEvent::ToolCall(t) => t.session_id.clone(),
        StreamEvent::Result(r) => r.session_id.clone(),
        StreamEvent::Unknown(v) => v
            .get("session_id")
            .and_then(|x| x.as_str())
            .map(ToOwned::to_owned),
    }
}

/// Per-run translation state. Mirrors the Cursor bridge cursor: a populated
/// `thread_id` established during run setup, plus a terminal latch.
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
                        .or_else(|| Some(format!("muse exec result subtype: {}", r.subtype)));
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

/// Best-effort mapping from muse exec's `result.subtype` to our
/// `FailureReason`. Muse has no documented turn-budget subtype, so all error
/// results map to the generic `AgentCrash` (shared with the other headless
/// bridges; kept distinct from `CodexCrash`).
fn classify_failure(_subtype: &str) -> FailureReason {
    FailureReason::AgentCrash
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
    async fn build_command_uses_exec_json_and_prompt_file() {
        let bridge = Bridge::spawn("muse", Path::new("/tmp"), None)
            .await
            .expect("bridge setup");
        let cmd = bridge.build_command(Path::new("/tmp/prompt.md"), Path::new("/tmp"));
        let argv = argv_of(&cmd);
        // The subcommand and headless flags must be present and ordered so the
        // prompt file is read and JSONL is emitted.
        let exec = argv
            .iter()
            .position(|a| a == "exec")
            .expect("exec subcommand");
        assert!(argv.iter().any(|a| a == "--json"), "argv: {argv:?}");
        let pf = argv
            .iter()
            .position(|a| a == "--prompt-file")
            .expect("--prompt-file flag");
        assert!(pf > exec, "--prompt-file must follow exec: {argv:?}");
        assert_eq!(
            argv.get(pf + 1).map(String::as_str),
            Some("/tmp/prompt.md"),
            "prompt path must directly follow --prompt-file: {argv:?}"
        );
        // MVP approval posture.
        assert!(argv.iter().any(|a| a == "--yolo"), "argv: {argv:?}");
    }

    #[tokio::test]
    async fn build_command_includes_model_and_session_id_when_set() {
        let mut bridge = Bridge::spawn("muse", Path::new("/tmp"), Some("muse-spark".into()))
            .await
            .expect("bridge setup");
        bridge.session_id = Some("sess-123".into());
        let cmd = bridge.build_command(Path::new("/tmp/p.md"), Path::new("/tmp"));
        let argv = argv_of(&cmd);
        let m = argv.iter().position(|a| a == "--model").expect("--model");
        assert_eq!(argv.get(m + 1).map(String::as_str), Some("muse-spark"));
        let s = argv
            .iter()
            .position(|a| a == "--session-id")
            .expect("--session-id");
        assert_eq!(argv.get(s + 1).map(String::as_str), Some("sess-123"));
    }

    #[test]
    fn translate_result_success_emits_completed() {
        let mut cursor = BridgeCursor {
            run_id: Uuid::new_v4(),
            thread_id: "muse-x".into(),
            model: None,
            terminal: false,
            seq: 0,
        };
        let ev = serde_json::from_str::<StreamEvent>(
            r#"{"type":"result","subtype":"success","is_error":false,"result":"ok"}"#,
        )
        .unwrap();
        let out = cursor.translate(ev);
        assert!(matches!(out.as_slice(), [BridgeEvent::Completed { .. }]));
        // Terminal latch: any trailing frame is suppressed.
        let trailing = serde_json::from_str::<StreamEvent>(
            r#"{"type":"assistant","message":{"role":"assistant","content":[]}}"#,
        )
        .unwrap();
        assert!(cursor.translate(trailing).is_empty());
    }

    #[test]
    fn translate_result_error_emits_failed() {
        let mut cursor = BridgeCursor {
            run_id: Uuid::new_v4(),
            thread_id: "muse-x".into(),
            model: None,
            terminal: false,
            seq: 0,
        };
        let ev = serde_json::from_str::<StreamEvent>(
            r#"{"type":"result","subtype":"error_auth","is_error":true,"result":"unauthorized"}"#,
        )
        .unwrap();
        let out = cursor.translate(ev);
        match out.as_slice() {
            [BridgeEvent::Failed { detail, .. }] => {
                assert_eq!(detail.as_deref(), Some("unauthorized"));
            }
            other => panic!("expected Failed, got {other:?}"),
        }
    }
}
