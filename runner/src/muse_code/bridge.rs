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
//! - Muse Code is closed-source and its JSONL schema is not publicly
//!   documented; the schema is modeled on captured real frames (see
//!   [`crate::muse_code::schema`]). Run setup is deliberately strict: it only
//!   succeeds once a recognized `run.lifecycle.started` (or terminal) record
//!   arrives, and bails on the first frame that is not a record envelope. A
//!   tolerant "any first frame starts the run" posture once let a wrong schema
//!   stream a whole run of unrecognized frames and surface only at EOF as a
//!   misleading "agent stdout closed" crash.

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
use crate::muse_code::schema::{Record, StreamEvent};
use crate::util::shell::login_shell_command;

/// How long to wait for `muse exec`'s `run.lifecycle.started` record before
/// giving up on the run setup. Generous: the CLI can take several seconds to
/// authenticate against the Meta Model API and emit its first event when
/// starting cold.
const INIT_TIMEOUT: Duration = Duration::from_secs(30);

/// Record that marks the run as started (preceded by bookkeeping records:
/// `runtime.command.accepted`, `session.run.linked`, `run.model.configured`,
/// `turn.input.user`).
const RUN_STARTED: &str = "run.lifecycle.started";
/// Prefix of the run-level terminal records: `run.terminal.completed`,
/// `run.terminal.failed`, ... Task-level `task.lifecycle.failed` is NOT
/// terminal — Muse reports a failed tool call (e.g. a non-zero bash exit) that
/// way and carries on.
const RUN_TERMINAL_PREFIX: &str = "run.terminal.";

pub struct Bridge {
    binary: String,
    model: Option<String>,
    /// `--yolo` (disable approval prompts and sandbox). Always on for MVP,
    /// mirroring the Cursor bridge's `--force` posture — a real approval loop
    /// is a follow-up.
    yolo: bool,
    /// Muse session id (the session stream UUID) captured during run setup, or
    /// seeded from a prior turn. Reused as the `--session-id` argument on
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
            // `muse exec --session-id` requires a UUID and exits immediately
            // (status 2) on anything else. Older runners stored a synthesized
            // `muse-<run_id>` pointer; drop such an id and start a fresh
            // session rather than killing the turn.
            if Uuid::parse_str(session_id).is_ok() {
                argv.extend(["--session-id", session_id]);
            } else {
                tracing::warn!(
                    session_id,
                    "ignoring non-UUID muse session id; starting a new session"
                );
            }
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
        Ok(BridgeCursor::new(run_id, thread_id, self.model.clone()))
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
    /// completion, emits `run.terminal.*`, then exits — there is no stdin to close. So
    /// one-shot and chat `run` are identical at the process level.
    pub async fn run_one_shot(&mut self, payload: &RunPayload, cwd: &Path) -> Result<BridgeCursor> {
        self.run(payload, cwd).await
    }

    /// Wait for the run to start so the returned cursor carries a populated
    /// `thread_id` (matching the Codex / Claude / Cursor contract).
    ///
    /// Records before `run.lifecycle.started` are buffered for the normal pump
    /// and mined for the session id (the session stream UUID) and the model
    /// (`run.model.configured`). A terminal record before the start (e.g. an
    /// early provider/config failure) also ends setup so the pump surfaces its
    /// real reason. Anything that is not a record envelope fails setup
    /// immediately, naming the schema — like Claude's `system/init` check.
    async fn wait_for_init(&mut self, run_id: Uuid) -> Result<String> {
        let proc = self
            .proc
            .as_mut()
            .context("muse exec process not spawned")?;
        let deadline = tokio::time::Instant::now() + INIT_TIMEOUT;
        let mut session_id: Option<String> = None;
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            let recv = tokio::time::timeout(remaining, proc.inbound.recv())
                .await
                .context("timed out waiting for muse exec's run.lifecycle.started record")?;
            let record = match recv {
                Some(StreamEvent::Record(r)) => r,
                Some(StreamEvent::Unknown(v)) => {
                    let mut frame = v.to_string();
                    if frame.len() > 200 {
                        let mut cut = 200;
                        while !frame.is_char_boundary(cut) {
                            cut -= 1;
                        }
                        frame.truncate(cut);
                        frame.push('…');
                    }
                    anyhow::bail!(
                        "muse exec emitted a frame that is not a record envelope \
                         (expected record_type/payload_type/payload); the muse \
                         --json schema may have changed: {frame}"
                    )
                }
                None => {
                    anyhow::bail!("muse exec stdout closed before emitting run.lifecycle.started")
                }
            };
            if session_id.is_none() {
                session_id = record.session_id();
            }
            if record.payload_type == "run.model.configured"
                && let Some(model) = record.payload_str("model_id").filter(|s| !s.is_empty())
            {
                self.model = Some(model.to_owned());
            }
            let started = record.payload_type == RUN_STARTED
                || record.payload_type.starts_with(RUN_TERMINAL_PREFIX);
            self.pending.push_back(StreamEvent::Record(record));
            if started {
                // The run id is itself a UUID, so the fallback is still a valid
                // `--session-id` for the next turn.
                let thread_id = session_id.unwrap_or_else(|| run_id.to_string());
                self.session_id = Some(thread_id.clone());
                return Ok(thread_id);
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

/// Per-run translation state. Mirrors the Cursor bridge cursor: a populated
/// `thread_id` established during run setup, plus a terminal latch and the
/// assistant text being streamed in `run.output.delta` fragments.
pub struct BridgeCursor {
    pub run_id: Uuid,
    pub thread_id: String,
    pub model: Option<String>,
    /// Flipped once we see a `run.terminal.*` record. Suppresses any trailing
    /// frames a stubborn subprocess might emit after completion.
    terminal: bool,
    pub seq: u64,
    /// `run.output.delta` text not yet emitted as an `assistant/message`.
    output: String,
}

impl BridgeCursor {
    fn new(run_id: Uuid, thread_id: String, model: Option<String>) -> Self {
        Self {
            run_id,
            thread_id,
            model,
            terminal: false,
            seq: 0,
            output: String::new(),
        }
    }

    /// Translate one Muse record into bridge events.
    ///
    /// Transcript-bearing records reuse the method names (and message shapes)
    /// the daemon already understands, so they reach the cloud as typed events:
    /// - `run.output.delta` fragments are joined into one `assistant/message`
    ///   (`{role, content: [{type: "text", text}]}`), emitted when the next
    ///   non-status record arrives;
    /// - `tool.result` → `user/toolResult` with a `tool_result` block;
    /// - `task.lifecycle.proposed` for a `tool.*` task → `tool_call/started`;
    /// - `run.terminal.completed` → `Completed`, other `run.terminal.*` →
    ///   `Failed`.
    ///
    /// Every other record (task bookkeeping, session linking, ...) passes
    /// through as `muse/<payload_type>` with the full record as params.
    pub fn translate(&mut self, ev: StreamEvent) -> Vec<BridgeEvent> {
        if self.terminal {
            return Vec::new();
        }
        self.seq = self.seq.saturating_add(1);

        let rec = match ev {
            StreamEvent::Record(rec) => rec,
            StreamEvent::Unknown(v) => {
                let mut out = self.flush_output();
                out.push(BridgeEvent::Raw {
                    run_id: self.run_id,
                    method: "unknown".into(),
                    params: v,
                });
                return out;
            }
        };

        match rec.payload_type.as_str() {
            "run.output.delta" => {
                if let Some(text) = rec.payload_str("text") {
                    self.output.push_str(text);
                }
                Vec::new()
            }
            // Model-stream progress notes arrive between deltas; don't let them
            // split one assistant message into several.
            "task.lifecycle.status" => vec![self.passthrough(rec)],
            pt if pt.starts_with(RUN_TERMINAL_PREFIX) => {
                let mut out = self.flush_output();
                self.terminal = true;
                out.push(self.terminal_event(&rec));
                out
            }
            "tool.result" => {
                let mut out = self.flush_output();
                out.push(self.tool_result(&rec));
                out
            }
            "task.lifecycle.proposed"
                if rec.task_kind().is_some_and(|k| k.starts_with("tool.")) =>
            {
                let mut out = self.flush_output();
                let task_kind = rec.task_kind().unwrap_or_default();
                out.push(BridgeEvent::Raw {
                    run_id: self.run_id,
                    method: "tool_call/started".into(),
                    params: serde_json::json!({
                        "task_id": rec.payload_str("task_id"),
                        "tool_name": task_kind.trim_start_matches("tool."),
                        "task_kind": task_kind,
                    }),
                });
                out
            }
            "run.model.configured" => {
                if let Some(model) = rec.payload_str("model_id").filter(|s| !s.is_empty()) {
                    self.model = Some(model.to_owned());
                }
                let mut out = self.flush_output();
                out.push(self.passthrough(rec));
                out
            }
            _ => {
                let mut out = self.flush_output();
                out.push(self.passthrough(rec));
                out
            }
        }
    }

    fn passthrough(&self, rec: Record) -> BridgeEvent {
        BridgeEvent::Raw {
            run_id: self.run_id,
            method: format!("muse/{}", rec.payload_type),
            params: rec.raw,
        }
    }

    /// Emit buffered `run.output.delta` text as one `assistant/message`.
    fn flush_output(&mut self) -> Vec<BridgeEvent> {
        if self.output.trim().is_empty() {
            self.output.clear();
            return Vec::new();
        }
        let text = std::mem::take(&mut self.output);
        vec![BridgeEvent::Raw {
            run_id: self.run_id,
            method: "assistant/message".into(),
            params: serde_json::json!({
                "role": "assistant",
                "content": [{"type": "text", "text": text}],
            }),
        }]
    }

    fn tool_result(&self, rec: &Record) -> BridgeEvent {
        let facts = rec.payload.get("correlation_facts");
        let fact = |k: &str| facts.and_then(|f| f.get(k)).and_then(|v| v.as_str());
        let outcome = fact("outcome");
        BridgeEvent::Raw {
            run_id: self.run_id,
            method: "user/toolResult".into(),
            params: serde_json::json!({
                "role": "user",
                "content": [{
                    "type": "tool_result",
                    "tool_use_id": rec.payload_str("call_id"),
                    "tool_name": fact("tool_name"),
                    "outcome": outcome,
                    "is_error": outcome.map(|o| o != "success"),
                    "content": rec.payload_str("text"),
                }],
            }),
        }
    }

    fn terminal_event(&self, rec: &Record) -> BridgeEvent {
        let terminal = rec
            .payload_str("terminal")
            .unwrap_or_else(|| rec.payload_type.trim_start_matches(RUN_TERMINAL_PREFIX));
        let text = rec.payload_str("text").filter(|s| !s.trim().is_empty());
        let reason = rec.payload_str("reason").filter(|s| !s.trim().is_empty());
        if terminal == "completed" {
            BridgeEvent::Completed {
                run_id: self.run_id,
                done_payload: serde_json::json!({
                    "conclusion": terminal,
                    "result": text,
                    "reason": reason,
                    "ended_at": Utc::now().to_rfc3339(),
                }),
            }
        } else {
            BridgeEvent::Failed {
                run_id: self.run_id,
                reason: classify_failure(terminal),
                detail: Some(format!(
                    "muse run {terminal}: {}",
                    reason.or(text).unwrap_or("no reason given")
                )),
            }
        }
    }
}

/// Best-effort mapping from a non-`completed` `run.terminal.*` kind to our
/// `FailureReason`. Muse has no documented turn-budget terminal, so all of them
/// map to the generic `AgentCrash` (shared with the other headless bridges;
/// kept distinct from `CodexCrash`).
fn classify_failure(_terminal: &str) -> FailureReason {
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

    const SESSION: &str = "01a0b6be-4608-7a63-92a0-ecd8ed7c4c7b";

    #[tokio::test]
    async fn build_command_includes_model_and_session_id_when_set() {
        let mut bridge = Bridge::spawn("muse", Path::new("/tmp"), Some("muse-spark".into()))
            .await
            .expect("bridge setup");
        bridge.session_id = Some(SESSION.into());
        let cmd = bridge.build_command(Path::new("/tmp/p.md"), Path::new("/tmp"));
        let argv = argv_of(&cmd);
        let m = argv.iter().position(|a| a == "--model").expect("--model");
        assert_eq!(argv.get(m + 1).map(String::as_str), Some("muse-spark"));
        let s = argv
            .iter()
            .position(|a| a == "--session-id")
            .expect("--session-id");
        assert_eq!(argv.get(s + 1).map(String::as_str), Some(SESSION));
    }

    #[tokio::test]
    async fn build_command_drops_non_uuid_session_id() {
        // `muse exec --session-id muse-<uuid>` exits 2 ("expected a UUID"); a
        // legacy synthesized id must not reach argv.
        let mut bridge = Bridge::spawn("muse", Path::new("/tmp"), None)
            .await
            .expect("bridge setup");
        bridge.session_id = Some("muse-977a6003-21cb-4e34-9d33-e231d8ab6c6b".into());
        let argv = argv_of(&bridge.build_command(Path::new("/tmp/p.md"), Path::new("/tmp")));
        assert!(!argv.iter().any(|a| a == "--session-id"), "argv: {argv:?}");
    }

    fn cursor() -> BridgeCursor {
        BridgeCursor::new(Uuid::new_v4(), SESSION.into(), None)
    }

    /// Replay a captured fixture through a cursor, returning every event.
    fn replay(fixture: &str) -> (BridgeCursor, Vec<BridgeEvent>) {
        let mut c = cursor();
        let mut out = Vec::new();
        for line in fixture.lines().filter(|l| !l.trim().is_empty()) {
            let ev = serde_json::from_str::<StreamEvent>(line).expect("fixture line parses");
            out.extend(c.translate(ev));
        }
        (c, out)
    }

    fn methods(events: &[BridgeEvent]) -> Vec<&str> {
        events
            .iter()
            .filter_map(|e| match e {
                BridgeEvent::Raw { method, .. } => Some(method.as_str()),
                _ => None,
            })
            .collect()
    }

    const META_COMPLETED: &str =
        include_str!("../../tests/fixtures/muse_code/meta_completed_run.jsonl");
    const ECHO_COMPLETED: &str =
        include_str!("../../tests/fixtures/muse_code/echo_completed_run.jsonl");
    const META_FAILED: &str =
        include_str!("../../tests/fixtures/muse_code/meta_transport_failed_run.jsonl");

    #[test]
    fn real_meta_run_completes_with_transcript() {
        let (c, out) = replay(META_COMPLETED);
        // Every captured frame is a recognized record: nothing is `unknown`.
        let methods = methods(&out);
        assert!(!methods.contains(&"unknown"), "{methods:?}");

        // Exactly one terminal event, and it is last.
        let terminal = out
            .iter()
            .filter(|e| {
                matches!(
                    e,
                    BridgeEvent::Completed { .. } | BridgeEvent::Failed { .. }
                )
            })
            .count();
        assert_eq!(terminal, 1);
        let BridgeEvent::Completed { done_payload, .. } = out.last().unwrap() else {
            panic!("expected Completed last, got {:?}", out.last());
        };
        let final_text = done_payload["result"].as_str().expect("result text");
        assert!(final_text.starts_with("Delta 1. Delta 2."), "{final_text}");

        // The streamed deltas arrive as one assistant message the daemon's
        // transcript extractor can read.
        let assistant: Vec<_> = out
            .iter()
            .filter_map(|e| match e {
                BridgeEvent::Raw { method, params, .. } if method == "assistant/message" => {
                    crate::daemon::observability::extract_agent_message_text(method, params)
                }
                _ => None,
            })
            .collect();
        assert_eq!(assistant, vec![final_text.trim().to_string()]);

        // Tool calls and results are typed; the fixture has 12 tool calls, one
        // of which failed.
        let started = methods
            .iter()
            .filter(|m| **m == "tool_call/started")
            .count();
        assert_eq!(started, 12);
        let results: Vec<_> = out
            .iter()
            .filter_map(|e| match e {
                BridgeEvent::Raw { method, params, .. } if method == "user/toolResult" => {
                    Some(params["content"][0].clone())
                }
                _ => None,
            })
            .collect();
        assert_eq!(results.len(), 12);
        assert!(results.iter().all(|r| r["tool_use_id"].is_string()));
        assert_eq!(results.iter().filter(|r| r["is_error"] == true).count(), 1);

        // The model is captured from `run.model.configured`.
        assert_eq!(c.model.as_deref(), Some("muse-spark-1.3-contributor"));
    }

    #[test]
    fn task_level_failure_does_not_fail_the_run() {
        // The echo fixture carries a `task.lifecycle.failed` (a reminder task)
        // followed by `run.terminal.completed`: the run still completes.
        assert!(ECHO_COMPLETED.contains("task.lifecycle.failed"));
        let (_, out) = replay(ECHO_COMPLETED);
        assert!(!out.iter().any(|e| matches!(e, BridgeEvent::Failed { .. })));
        let BridgeEvent::Completed { done_payload, .. } = out.last().unwrap() else {
            panic!("expected Completed");
        };
        assert_eq!(done_payload["result"], "echo: hello there");
        assert!(methods(&out).contains(&"muse/task.lifecycle.failed"));
    }

    #[test]
    fn real_terminal_failure_emits_failed_with_reason() {
        let (_, out) = replay(META_FAILED);
        match out.last().unwrap() {
            BridgeEvent::Failed { detail, reason, .. } => {
                assert!(matches!(reason, FailureReason::AgentCrash));
                let detail = detail.as_deref().unwrap();
                assert!(
                    detail.starts_with("muse run failed: transport error"),
                    "{detail}"
                );
            }
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[test]
    fn terminal_latch_suppresses_trailing_frames() {
        let mut c = cursor();
        let done =
            serde_json::from_str::<StreamEvent>(ECHO_COMPLETED.lines().last().unwrap()).unwrap();
        assert!(matches!(
            c.translate(done.clone()).as_slice(),
            [BridgeEvent::Completed { .. }]
        ));
        assert!(c.translate(done).is_empty());
    }

    #[test]
    fn unrecognized_terminal_kind_fails() {
        let mut c = cursor();
        let ev = serde_json::from_str::<StreamEvent>(
            r#"{"record_type":"event","payload_type":"run.terminal.cancelled","payload":{"kind":"run_terminal","terminal":"cancelled","reason":"interrupted","text":""}}"#,
        )
        .unwrap();
        match c.translate(ev).as_slice() {
            [BridgeEvent::Failed { detail, .. }] => {
                assert_eq!(detail.as_deref(), Some("muse run cancelled: interrupted"));
            }
            other => panic!("expected Failed, got {other:?}"),
        }
    }
}
