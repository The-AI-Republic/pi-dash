//! Grok bridge. Acts as the ACP *client* for grok's native ACP server
//! (`grok agent stdio`): it performs the JSON-RPC handshake
//!
//! ```text
//! initialize → session/new {cwd} → session/prompt {sessionId, prompt}
//! ```
//!
//! then streams the server's `session/update` notifications as
//! [`crate::agent::BridgeEvent::Raw`] and ends the turn on the
//! `session/prompt` response's `stopReason`. Inbound frames are parsed with
//! OpenClaw's shared [`AcpMessage`]; outbound requests are built in
//! [`crate::grok::schema`].
//!
//! The public surface mirrors the other bridges so the agent dispatch layer
//! (`crate::agent::AgentBridge`) can treat all agents uniformly. Unlike the
//! one-shot cursor / acpx bridges, the grok process is a persistent server, so
//! `initialize` + `session/new` happen once (in `warm`) and multiple turns can
//! reuse the same session.
//!
//! MVP limitations (tracked as follow-ups):
//! - **Approvals bypass**: a `session/request_permission` request is answered
//!   automatically by selecting an "allow" option (see
//!   [`schema::select_allow_option`]). Real per-tool approval — routing the
//!   request through `runner/src/approval` — is the documented next step.
//! - **Stateless turns**: each fresh bridge does a new `session/new`; a resume
//!   session id only seeds the run's thread id, it does not `session/load`
//!   prior conversation state.

use anyhow::{Context, Result};
use chrono::Utc;
use std::collections::VecDeque;
use std::path::Path;
use std::time::Duration;
use uuid::Uuid;

use crate::agent::{AgentProcessHandle, BridgeEvent, RunPayload, StderrSnapshot};
use crate::cloud::protocol::{ApprovalDecision, FailureReason};
use crate::grok::process::GrokProcess;
use crate::grok::schema;
use crate::openclaw::schema::AcpMessage;

/// How long to wait for grok to answer a handshake request (`initialize` /
/// `session/new`) before giving up on run setup. Generous: grok has to boot
/// its ACP server and may authenticate against the xAI API first.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);

pub struct Bridge {
    proc: GrokProcess,
    model: Option<String>,
    /// `initialize` completed for this process.
    initialized: bool,
    /// A live ACP session exists on this process (via `session/new`).
    session_ready: bool,
    /// ACP session id — captured from `session/new`, or seeded from a resume id
    /// (MVP: seed only; see module docs). Surfaced as the run's thread id.
    session_id: Option<String>,
    /// Frames pulled off stdout while awaiting a handshake response that were
    /// notifications rather than the response — replayed by `next_events` so no
    /// streamed output is lost.
    pending: VecDeque<serde_json::Value>,
}

impl Bridge {
    pub async fn spawn(binary: &str, cwd: &Path, model_default: Option<String>) -> Result<Self> {
        Self::spawn_with_resume(binary, cwd, model_default, None).await
    }

    pub async fn spawn_with_resume(
        binary: &str,
        cwd: &Path,
        model_default: Option<String>,
        resume_session_id: Option<&str>,
    ) -> Result<Self> {
        let proc = GrokProcess::spawn(binary, cwd).await?;
        Ok(Self::from_process(
            proc,
            model_default,
            resume_session_id.filter(|s| !s.is_empty()).map(ToOwned::to_owned),
        ))
    }

    /// Build a bridge over an already-constructed process. Used by tests to
    /// substitute a fake `grok` subprocess.
    pub fn from_process(
        proc: GrokProcess,
        model_default: Option<String>,
        resume_session_id: Option<String>,
    ) -> Self {
        Self {
            proc,
            model: model_default.filter(|s| !s.is_empty()),
            initialized: false,
            session_ready: false,
            session_id: resume_session_id,
            pending: VecDeque::new(),
        }
    }

    /// Prepare the ACP server for a turn: `initialize` (once) then `session/new`
    /// (once). Returns the live session id so the cloud can anchor the run's
    /// thread on it before the first turn lands.
    pub async fn warm(&mut self, cwd: &Path) -> Result<Option<String>> {
        self.ensure_initialized().await?;
        self.ensure_session(cwd).await?;
        Ok(self.session_id.clone())
    }

    pub async fn run(&mut self, payload: &RunPayload, cwd: &Path) -> Result<BridgeCursor> {
        if let Some(m) = payload.model.as_deref().filter(|s| !s.is_empty()) {
            self.model = Some(m.to_string());
        }
        self.warm(cwd).await?;
        let session_id = self
            .session_id
            .clone()
            .context("grok session id missing after warm")?;
        let id = self.proc.alloc_id();
        let line = schema::session_prompt_request(id, &session_id, &payload.prompt);
        self.proc.send_raw(&line).await?;
        Ok(BridgeCursor {
            run_id: payload.run_id,
            thread_id: session_id,
            model: self.model.clone(),
            terminal: false,
            seq: 0,
        })
    }

    /// grok's ACP session persists across turns, so one-shot and chat `run` are
    /// identical at the process level.
    pub async fn run_one_shot(&mut self, payload: &RunPayload, cwd: &Path) -> Result<BridgeCursor> {
        self.run(payload, cwd).await
    }

    async fn ensure_initialized(&mut self) -> Result<()> {
        if self.initialized {
            return Ok(());
        }
        let id = self.proc.alloc_id();
        let line = schema::initialize_request(id);
        self.proc.send_raw(&line).await?;
        self.await_response(HANDSHAKE_TIMEOUT)
            .await
            .context("grok initialize failed")?;
        self.initialized = true;
        Ok(())
    }

    async fn ensure_session(&mut self, cwd: &Path) -> Result<()> {
        if self.session_ready {
            return Ok(());
        }
        let id = self.proc.alloc_id();
        let cwd_str = cwd.to_string_lossy();
        let line = schema::session_new_request(id, &cwd_str, self.model.as_deref());
        self.proc.send_raw(&line).await?;
        let result = self
            .await_response(HANDSHAKE_TIMEOUT)
            .await
            .context("grok session/new failed")?;
        // Prefer the id grok minted; fall back to any seeded resume id so we
        // still have a stable thread id even if the server omits it.
        if let Some(sid) = schema::session_id_from_result(&result) {
            self.session_id = Some(sid);
        }
        if self.session_id.is_none() {
            anyhow::bail!("grok session/new returned no sessionId");
        }
        self.session_ready = true;
        Ok(())
    }

    /// Pull frames until the next JSON-RPC *response* arrives, returning its
    /// `result` (or bailing on an `error`). Non-response frames (notifications,
    /// and any early permission request) are handled along the way — permission
    /// requests are auto-answered, other notifications are buffered so
    /// `next_events` replays them. Responses are correlated by arrival order:
    /// the handshake sends one request at a time and awaits it before the next,
    /// so the first response we see is the one we asked for.
    async fn await_response(&mut self, timeout: Duration) -> Result<serde_json::Value> {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            let frame = tokio::time::timeout(remaining, self.proc.inbound.recv())
                .await
                .context("grok response timed out")?;
            let Some(value) = frame else {
                anyhow::bail!("grok stdout closed while awaiting response");
            };
            if self.try_auto_approve(&value).await? {
                continue;
            }
            if let Some(err) = value.get("error").filter(|e| !e.is_null()) {
                let msg = err
                    .get("message")
                    .and_then(|m| m.as_str())
                    .map(ToOwned::to_owned)
                    .unwrap_or_else(|| err.to_string());
                anyhow::bail!("grok rpc error: {msg}");
            }
            if value.get("result").is_some() {
                return Ok(value.get("result").cloned().unwrap_or(serde_json::Value::Null));
            }
            // A notification (has `method`, no `result`) arrived mid-handshake;
            // stash it so the turn translator still sees it.
            self.pending.push_back(value);
        }
    }

    /// Pull the next event and translate it. Auto-answers permission requests
    /// (emitting them as `Raw` for history). Returns `None` once the stream
    /// closes for good.
    pub async fn next_events(&mut self, cursor: &mut BridgeCursor) -> Option<Vec<BridgeEvent>> {
        loop {
            let value = if let Some(buffered) = self.pending.pop_front() {
                buffered
            } else {
                self.proc.inbound.recv().await?
            };
            // A permission request must be answered or the turn deadlocks; do it
            // and surface the request as Raw so history records the ask.
            if is_permission_request(&value) {
                if let Err(e) = self.try_auto_approve(&value).await {
                    tracing::warn!("grok auto-approve failed: {e}");
                }
                let translated = cursor.translate_raw("session/request_permission", value);
                if !translated.is_empty() {
                    return Some(translated);
                }
                continue;
            }
            let msg: AcpMessage = match serde_json::from_value(value.clone()) {
                Ok(m) => m,
                Err(e) => {
                    tracing::warn!("grok emitted unparsable ACP frame ({e}): {value}");
                    AcpMessage::Unknown(value)
                }
            };
            let translated = cursor.translate(msg);
            if !translated.is_empty() {
                return Some(translated);
            }
        }
    }

    /// If `value` is a `session/request_permission` request, answer it by
    /// selecting an allow option and return `true`. Otherwise `false`.
    async fn try_auto_approve(&mut self, value: &serde_json::Value) -> Result<bool> {
        if !is_permission_request(value) {
            return Ok(false);
        }
        let id = value.get("id").cloned().unwrap_or(serde_json::Value::Null);
        let params = value.get("params").cloned().unwrap_or(serde_json::Value::Null);
        match schema::select_allow_option(&params) {
            Some(option_id) => {
                let line = schema::permission_selected_response(&id, &option_id);
                self.proc.send_raw(&line).await?;
            }
            None => {
                tracing::warn!(
                    "grok session/request_permission offered no options; cannot auto-approve"
                );
            }
        }
        Ok(true)
    }

    /// Approvals aren't routed for grok in the MVP — `next_events` auto-answers
    /// every `session/request_permission` inline, so the supervisor never gets
    /// an `ApprovalRequest` to decide. Reaching this is a programmer error.
    pub async fn send_approval(
        &mut self,
        approval_id: &str,
        _decision: ApprovalDecision,
    ) -> Result<()> {
        tracing::error!(
            approval_id,
            "grok bridge received an approval decision but approvals are \
             auto-answered inline; refusing to silently drop it"
        );
        anyhow::bail!("grok bridge received approval {approval_id} but approvals are auto-answered");
    }

    pub async fn interrupt(&mut self) -> Result<()> {
        self.proc.interrupt().await
    }

    pub async fn shutdown(self, grace: Duration) -> Result<()> {
        self.proc.shutdown(grace).await
    }

    pub fn process_handle(&self) -> AgentProcessHandle {
        self.proc.process_handle()
    }

    pub async fn recent_stderr(&self) -> StderrSnapshot {
        self.proc.recent_stderr().await
    }
}

/// True when `value` is a JSON-RPC *request* (carries an `id`) whose method is
/// `session/request_permission`.
fn is_permission_request(value: &serde_json::Value) -> bool {
    value.get("id").is_some()
        && value.get("method").and_then(|m| m.as_str()) == Some("session/request_permission")
}

/// Per-run translation state. Mirrors OpenClaw's cursor: the first turn output
/// streams as `Raw`, and the `session/prompt` response's `stopReason` (or a
/// JSON-RPC error) ends the turn.
pub struct BridgeCursor {
    pub run_id: Uuid,
    pub thread_id: String,
    pub model: Option<String>,
    /// Flipped once a terminal frame (the `stopReason` response or an error) is
    /// seen; suppresses any trailing frames.
    terminal: bool,
    pub seq: u64,
}

impl BridgeCursor {
    /// Emit a raw history event without consuming a translation branch — used
    /// for the auto-answered permission request.
    fn translate_raw(&mut self, method: &str, params: serde_json::Value) -> Vec<BridgeEvent> {
        if self.terminal {
            return Vec::new();
        }
        self.seq = self.seq.saturating_add(1);
        vec![BridgeEvent::Raw {
            run_id: self.run_id,
            method: method.to_string(),
            params,
        }]
    }

    pub fn translate(&mut self, ev: AcpMessage) -> Vec<BridgeEvent> {
        if self.terminal {
            return Vec::new();
        }
        self.seq = self.seq.saturating_add(1);

        match ev {
            AcpMessage::Method { method, params } => {
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
                match result.get("stopReason").and_then(|s| s.as_str()) {
                    Some(stop_reason) => {
                        self.terminal = true;
                        if is_failure_stop_reason(stop_reason) {
                            vec![BridgeEvent::Failed {
                                run_id: self.run_id,
                                reason: FailureReason::AgentCrash,
                                detail: Some(format!("grok stopReason: {stop_reason}")),
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
                    .or_else(|| Some(format!("grok ACP error: {error}")));
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

/// ACP `stopReason` values meaning the turn did not complete its work:
/// `cancelled` (interrupted) and `refusal` (the model declined). Everything
/// else (`end_turn`, `max_tokens`, `max_turn_requests`) is a natural end.
/// Mirrors the OpenClaw bridge.
fn is_failure_stop_reason(stop_reason: &str) -> bool {
    matches!(stop_reason, "cancelled" | "refusal")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
        let out = c.translate(msg(
            r#"{"method":"session/update","params":{"sessionId":"s1","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"hi"}}}}"#,
        ));
        match &out[0] {
            BridgeEvent::Raw { method, .. } => {
                assert_eq!(method, "session/update/agent_message_chunk")
            }
            other => panic!("expected Raw, got {other:?}"),
        }
    }

    #[test]
    fn stop_reason_end_turn_completes_and_is_terminal() {
        let mut c = cursor();
        let out = c.translate(msg(r#"{"id":3,"result":{"stopReason":"end_turn"}}"#));
        assert!(matches!(out[0], BridgeEvent::Completed { .. }));
        // Terminal: subsequent frames suppressed.
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
            r#"{"id":2,"error":{"code":-32603,"message":"invalid api key"}}"#,
        ));
        match &out[0] {
            BridgeEvent::Failed { detail, .. } => {
                assert_eq!(detail.as_deref(), Some("invalid api key"))
            }
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[test]
    fn non_terminal_response_is_raw() {
        let mut c = cursor();
        let out = c.translate(msg(r#"{"id":1,"result":{"sessionId":"s1"}}"#));
        assert!(matches!(out[0], BridgeEvent::Raw { .. }));
    }

    #[test]
    fn permission_request_is_detected() {
        assert!(is_permission_request(&json!({
            "id": 5, "method": "session/request_permission", "params": {}
        })));
        // A notification with the same method but no id is not a request.
        assert!(!is_permission_request(&json!({
            "method": "session/request_permission", "params": {}
        })));
        // A session/update is not a permission request.
        assert!(!is_permission_request(&json!({
            "id": 6, "method": "session/update", "params": {}
        })));
    }

    /// Drive the real handshake + translate path against a fake `grok agent
    /// stdio` that prints canned ACP frames in order and then drains stdin.
    /// This exercises `initialize` → `session/new` → `session/prompt` and the
    /// terminal `stopReason` without a real binary.
    #[tokio::test]
    async fn end_to_end_handshake_against_fake_grok() {
        // The fake ignores request ids and just emits, in order: the
        // initialize response, the session/new response (with a sessionId), a
        // streamed agent message, and the terminal stopReason. `exec cat` then
        // keeps the process alive draining our stdin writes so send_raw never
        // hits a broken pipe.
        let script = r#"
printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1}}'
printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"sessionId":"sess-xyz"}}'
printf '%s\n' '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"sess-xyz","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"working"}}}}'
printf '%s\n' '{"jsonrpc":"2.0","id":3,"result":{"stopReason":"end_turn"}}'
exec cat >/dev/null
"#;
        let mut cmd = tokio::process::Command::new("bash");
        cmd.arg("-c").arg(script);
        let proc = GrokProcess::spawn_command(cmd).await.expect("spawn fake grok");
        let mut bridge = Bridge::from_process(proc, None, None);

        let payload = RunPayload {
            run_id: Uuid::nil(),
            prompt: "do it".into(),
            model: None,
        };
        let mut cursor = bridge
            .run(&payload, Path::new("/tmp"))
            .await
            .expect("run drives the handshake");
        assert_eq!(cursor.thread_id, "sess-xyz");

        // Drain events to the terminal Completed.
        let mut saw_raw = false;
        let mut completed = false;
        while let Some(events) = bridge.next_events(&mut cursor).await {
            for ev in events {
                match ev {
                    BridgeEvent::Raw { .. } => saw_raw = true,
                    BridgeEvent::Completed { .. } => {
                        completed = true;
                    }
                    other => panic!("unexpected event: {other:?}"),
                }
            }
            if completed {
                break;
            }
        }
        assert!(saw_raw, "expected the streamed session/update as a Raw event");
        assert!(completed, "expected the stopReason to complete the turn");
    }
}
