//! Shared ACP bridge. Acts as the ACP *client* for a native ACP server
//! (Grok's `grok agent stdio`, Antigravity's `agy acp`): it performs the
//! JSON-RPC handshake
//!
//! ```text
//! initialize → session/new {cwd} → session/prompt {sessionId, prompt}
//! ```
//!
//! then streams the server's `session/update` notifications as
//! [`crate::agent::BridgeEvent::Raw`] and ends the turn on the
//! `session/prompt` response's `stopReason`. Inbound frames are parsed with
//! OpenClaw's shared [`AcpMessage`]; outbound requests are built in
//! [`crate::acp::schema`].
//!
//! The public surface mirrors the other bridges so the agent dispatch layer
//! (`crate::agent::AgentBridge`) can treat all agents uniformly. Unlike the
//! one-shot cursor / acpx bridges, an ACP server process is persistent, so
//! `initialize` + `session/new` happen once (in `warm`) and multiple turns can
//! reuse the same session.
//!
//! Per-agent differences are carried by [`AcpProfile`]; behaviour notes:
//! - **Approvals bypass**: a `session/request_permission` request is answered
//!   automatically by selecting an "allow" option (see
//!   [`schema::select_allow_option`]). Real per-tool approval — routing the
//!   request through `runner/src/approval` — is the documented next step.
//! - **Resume**: a known resume session id is revived through
//!   `session/resume` (no history replay) when the server advertises
//!   `sessionCapabilities.resume`, else `session/load` when it advertises
//!   `loadSession`, else the id is kept as the run's thread-id seed only.
//! - **Run-time auth**: a turn that fails with the ACP authentication-required
//!   error (code `-32000`) surfaces as `AgentAuthMissing` with the agent's
//!   fix hint, not a plain agent crash. That catches expired or revoked
//!   sign-ins, which the pre-pickup presence check cannot see. A handshake
//!   that fails the same way (e.g. `session/new` answering `-32000` before
//!   any turn exists) surfaces a typed [`HandshakeAuthError`] so the
//!   supervisor reports the same `agent_auth_missing` + fix hint.
//! - **Token usage**: all `session/update` frames (including any future
//!   `usage_update`) stream as `Raw` history events; the supervisor's usage
//!   parser picks up the shapes it knows.

use anyhow::{Context, Result};
use chrono::Utc;
use std::collections::VecDeque;
use std::path::Path;
use std::time::Duration;
use uuid::Uuid;

use crate::acp::process::AcpProcess;
use crate::acp::schema::{self, ConfigOption, ResumeSupport};
use crate::agent::{
    AgentProcessHandle, BridgeEvent, ENGINE_STARTUP_TIMEOUT, RunPayload, StderrSnapshot,
};
use crate::cloud::protocol::{ApprovalDecision, FailureReason};
use crate::openclaw::schema::AcpMessage;

/// A handshake that failed with the ACP authentication-required error
/// (code `-32000`). Produced by [`Bridge::await_response`] when a handshake
/// request (`initialize`, `session/new`, `session/resume`, `session/load`,
/// or `session/set_config_option`) is answered with that error — e.g. `agy
/// acp` answering `session/new` when the cached sign-in expired or was
/// revoked. Handshake callers propagate it unwrapped (see
/// [`Bridge::await_handshake_response`]) so the supervisor's
/// handshake-failure arm can match it by type via [`Self::find`] and report
/// `agent_auth_missing` with the agent's fix hint — the same mapping
/// [`BridgeCursor::translate`] applies to turn-time errors.
#[derive(Debug)]
pub struct HandshakeAuthError {
    /// Agent label for failure details (`"grok"`, `"antigravity"`).
    pub label: &'static str,
    /// The server's `error.message` (defaults to `"Authentication required"`).
    pub message: String,
    /// Agent's fix hint (the profile's `auth_fix_hint`).
    pub fix_hint: &'static str,
}

impl HandshakeAuthError {
    /// Locate one in an error chain. Matches only when it is the outermost
    /// error: handshake callers propagate it unwrapped, so a match means
    /// the handshake itself failed with authentication-required.
    pub fn find(err: &anyhow::Error) -> Option<&HandshakeAuthError> {
        err.downcast_ref::<HandshakeAuthError>()
    }

    /// `RunFailed` detail, in the same shape as the turn-time mapping in
    /// [`BridgeCursor::translate`]: `"<label>: <message>: <fix hint>"`.
    pub fn detail(&self) -> String {
        format!("{}: {}: {}", self.label, self.message, self.fix_hint)
    }
}

impl std::fmt::Display for HandshakeAuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.detail())
    }
}

impl std::error::Error for HandshakeAuthError {}

/// Per-agent ACP behaviour. Grok keeps its pre-shared posture (model on
/// `session/new`, interrupt by OS signal); Antigravity applies the configured
/// model through the advertised session config options and ends the turn
/// through the protocol-level `session/cancel` notification.
#[derive(Debug, Clone, Copy)]
pub struct AcpProfile {
    /// Agent label for logs and failure details (`"grok"`, `"antigravity"`).
    pub label: &'static str,
    /// Send the configured model as the extra `model` field on `session/new`.
    /// Grok reads it; Antigravity ignores unknown fields, so it stays off
    /// there (the model is applied via `session/set_config_option` instead).
    pub send_model_on_session_new: bool,
    /// Apply the configured model through the advertised `configOptions`
    /// after `session/new` (or resume / load). Off for Grok (unchanged
    /// behaviour); on for Antigravity.
    pub apply_model_via_config_option: bool,
    /// End the turn with the `session/cancel` notification before falling
    /// back to the OS signal. Off for Grok (SIGINT only, unchanged); on for
    /// Antigravity, whose server documents turn cancellation this way.
    pub cancel_via_notification: bool,
    /// Fix hint surfaced when a turn fails with the ACP
    /// authentication-required error (`agent_auth_missing` detail).
    pub auth_fix_hint: &'static str,
}

pub struct Bridge {
    proc: AcpProcess,
    profile: AcpProfile,
    model: Option<String>,
    /// `initialize` completed for this process.
    initialized: bool,
    /// A live ACP session exists on this process (via `session/new`, resume,
    /// or load).
    session_ready: bool,
    /// How a known resume id is revived; read off the `initialize` response.
    resume_support: ResumeSupport,
    /// ACP session id — captured from `session/new` (or resume / load), or
    /// seeded from a resume id when the server offers no revival path.
    /// Surfaced as the run's thread id.
    session_id: Option<String>,
    /// Session config options advertised by the latest `session/new` (or
    /// resume / load) response; used to apply the configured model.
    config_options: Vec<ConfigOption>,
    /// Frames pulled off stdout while awaiting a handshake response that were
    /// notifications rather than the response — replayed by `next_events` so
    /// no streamed output is lost.
    pending: VecDeque<serde_json::Value>,
}

impl Bridge {
    pub async fn spawn(
        profile: AcpProfile,
        binary: &str,
        argv: &[String],
        cwd: &Path,
        model_default: Option<String>,
    ) -> Result<Self> {
        Self::spawn_with_resume(profile, binary, argv, cwd, model_default, None).await
    }

    pub async fn spawn_with_resume(
        profile: AcpProfile,
        binary: &str,
        argv: &[String],
        cwd: &Path,
        model_default: Option<String>,
        resume_session_id: Option<&str>,
    ) -> Result<Self> {
        let refs: Vec<&str> = argv.iter().map(|s| s.as_str()).collect();
        let proc = AcpProcess::spawn(profile.label, binary, &refs, cwd).await?;
        Ok(Self::from_process(
            proc,
            profile,
            model_default,
            resume_session_id
                .filter(|s| !s.is_empty())
                .map(ToOwned::to_owned),
        ))
    }

    /// Build a bridge over an already-constructed process. Used by tests to
    /// substitute a fake ACP server subprocess.
    pub fn from_process(
        proc: AcpProcess,
        profile: AcpProfile,
        model_default: Option<String>,
        resume_session_id: Option<String>,
    ) -> Self {
        Self {
            proc,
            profile,
            model: model_default.filter(|s| !s.is_empty()),
            initialized: false,
            session_ready: false,
            resume_support: ResumeSupport::SeedOnly,
            session_id: resume_session_id,
            config_options: Vec::new(),
            pending: VecDeque::new(),
        }
    }

    /// Prepare the ACP server for a turn: `initialize` (once) then
    /// `session/new` — or `session/resume` / `session/load` when a resume id
    /// is known and the server supports it — (once). Applies the configured
    /// model where the profile says to. Returns the live session id so the
    /// cloud can anchor the run's thread on it before the first turn lands.
    pub async fn warm(&mut self, cwd: &Path) -> Result<Option<String>> {
        self.ensure_initialized().await?;
        self.ensure_session(cwd).await?;
        self.apply_model_option().await?;
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
            .with_context(|| format!("{} session id missing after warm", self.profile.label))?;
        let id = self.proc.alloc_id();
        let line = schema::session_prompt_request(id, &session_id, &payload.prompt);
        self.proc.send_raw(&line).await?;
        Ok(BridgeCursor {
            profile_label: self.profile.label,
            auth_fix_hint: self.profile.auth_fix_hint,
            run_id: payload.run_id,
            thread_id: session_id,
            model: self.model.clone(),
            terminal: false,
            seq: 0,
        })
    }

    /// An ACP session persists across turns, so one-shot and chat `run` are
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
        let result = self.await_handshake_response("initialize").await?;
        self.resume_support = schema::resume_support_from_initialize(&result);
        self.initialized = true;
        Ok(())
    }

    async fn ensure_session(&mut self, cwd: &Path) -> Result<()> {
        if self.session_ready {
            return Ok(());
        }
        let label = self.profile.label;
        let cwd_str = cwd.to_string_lossy();
        let model_on_new = self.profile.send_model_on_session_new;
        let resume_id = self.session_id.clone();
        let result = match (resume_id.filter(|s| !s.is_empty()), self.resume_support) {
            (Some(known), ResumeSupport::Resume) => {
                let id = self.proc.alloc_id();
                self.proc
                    .send_raw(&schema::session_resume_request(id, &known, &cwd_str))
                    .await?;
                let result = self.await_handshake_response("session/resume").await?;
                // The resumed session keeps its id; prefer one the server
                // mints, fall back to the id we asked to resume.
                Ok::<_, anyhow::Error>((
                    schema::session_id_from_result(&result).or(Some(known)),
                    result,
                ))
            }
            (Some(known), ResumeSupport::Load) => {
                let id = self.proc.alloc_id();
                self.proc
                    .send_raw(&schema::session_load_request(id, &known, &cwd_str))
                    .await?;
                let result = self.await_handshake_response("session/load").await?;
                Ok((
                    schema::session_id_from_result(&result).or(Some(known)),
                    result,
                ))
            }
            _ => {
                let id = self.proc.alloc_id();
                let line = schema::session_new_request(
                    id,
                    &cwd_str,
                    model_on_new.then_some(self.model.as_deref()).flatten(),
                );
                self.proc.send_raw(&line).await?;
                let result = self.await_handshake_response("session/new").await?;
                // Prefer the id the server minted; fall back to any seeded
                // resume id so we still have a stable thread id even if the
                // server omits it.
                Ok((schema::session_id_from_result(&result), result))
            }
        }?;
        let (session_id, result) = result;
        if let Some(sid) = session_id {
            self.session_id = Some(sid);
        }
        self.config_options = schema::config_options_from_result(&result);
        if self.session_id.is_none() {
            anyhow::bail!("{label} session/new returned no sessionId");
        }
        self.session_ready = true;
        Ok(())
    }

    /// Apply the configured model through the advertised session config
    /// options, where the profile says to. No model configured, no advertised
    /// model option, or no advertised value matching the configured slug:
    /// the server default stays and the miss is logged, never fatal — the
    /// run still proceeds.
    async fn apply_model_option(&mut self) -> Result<()> {
        if !self.profile.apply_model_via_config_option {
            return Ok(());
        }
        let Some(model) = self.model.clone() else {
            return Ok(());
        };
        let Some(session_id) = self.session_id.clone() else {
            return Ok(());
        };
        match schema::select_config_value(&self.config_options, &model) {
            Some((config_id, value)) => {
                let id = self.proc.alloc_id();
                let line =
                    schema::session_set_config_option_request(id, &session_id, &config_id, &value);
                self.proc.send_raw(&line).await?;
                self.await_handshake_response("session/set_config_option")
                    .await?;
            }
            None => {
                tracing::warn!(
                    label = self.profile.label,
                    model,
                    "configured model matches no advertised session option; using server default"
                );
            }
        }
        Ok(())
    }

    /// Await one handshake response, attaching `{label} {step} failed`
    /// context to a plain failure. A [`HandshakeAuthError`] propagates
    /// unwrapped so the supervisor's handshake-failure arm can match it by
    /// type and report `agent_auth_missing` instead of a plain crash.
    async fn await_handshake_response(&mut self, step: &str) -> Result<serde_json::Value> {
        let label = self.profile.label;
        self.await_response(ENGINE_STARTUP_TIMEOUT)
            .await
            .map_err(|e| {
                if e.is::<HandshakeAuthError>() {
                    e
                } else {
                    e.context(format!("{label} {step} failed"))
                }
            })
    }

    /// Pull frames until the next JSON-RPC *response* arrives, returning its
    /// `result` (or bailing on an `error`). Non-response frames (notifications,
    /// and any early permission request) are handled along the way — permission
    /// requests are auto-answered, other notifications are buffered so
    /// `next_events` replays them. Responses are correlated by arrival order:
    /// the handshake sends one request at a time and awaits it before the next,
    /// so the first response we see is the one we asked for.
    ///
    /// An authentication-required error (code `-32000`) fails with a typed
    /// [`HandshakeAuthError`] carrying the agent's fix hint, so callers can
    /// report it as `agent_auth_missing` rather than a plain crash.
    async fn await_response(&mut self, timeout: Duration) -> Result<serde_json::Value> {
        let label = self.profile.label;
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            let frame = tokio::time::timeout(remaining, self.proc.inbound.recv())
                .await
                .with_context(|| format!("{label} response timed out"))?;
            let Some(value) = frame else {
                anyhow::bail!("{label} stdout closed while awaiting response");
            };
            if self.try_auto_approve(&value).await? {
                continue;
            }
            if let Some(err) = value.get("error").filter(|e| !e.is_null()) {
                if schema::is_auth_required_error(err) {
                    let message = err
                        .get("message")
                        .and_then(|m| m.as_str())
                        .unwrap_or("Authentication required")
                        .to_owned();
                    return Err(HandshakeAuthError {
                        label,
                        message,
                        fix_hint: self.profile.auth_fix_hint,
                    }
                    .into());
                }
                let msg = err
                    .get("message")
                    .and_then(|m| m.as_str())
                    .map(ToOwned::to_owned)
                    .unwrap_or_else(|| err.to_string());
                anyhow::bail!("{label} rpc error: {msg}");
            }
            if value.get("result").is_some() {
                return Ok(value
                    .get("result")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null));
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
                    tracing::warn!("{} auto-approve failed: {e}", self.profile.label);
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
                    tracing::warn!(
                        "{} emitted unparsable ACP frame ({e}): {value}",
                        self.profile.label
                    );
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
        let params = value
            .get("params")
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        match schema::select_allow_option(&params) {
            Some(option_id) => {
                let line = schema::permission_selected_response(&id, &option_id);
                self.proc.send_raw(&line).await?;
            }
            None => {
                tracing::warn!(
                    "{} session/request_permission offered no options; cannot auto-approve",
                    self.profile.label
                );
            }
        }
        Ok(true)
    }

    /// Approvals aren't routed for ACP agents in the MVP — `next_events`
    /// auto-answers every `session/request_permission` inline, so the
    /// supervisor never gets an `ApprovalRequest` to decide. Reaching this is
    /// a programmer error.
    pub async fn send_approval(
        &mut self,
        approval_id: &str,
        _decision: ApprovalDecision,
    ) -> Result<()> {
        let label = self.profile.label;
        tracing::error!(
            approval_id,
            "{label} bridge received an approval decision but approvals are \
             auto-answered inline; refusing to silently drop it"
        );
        anyhow::bail!(
            "{label} bridge received approval {approval_id} but approvals are auto-answered"
        );
    }

    pub async fn interrupt(&mut self) -> Result<()> {
        // Protocol-level cancel first where the profile says to: the server
        // aborts the live turn and answers the in-flight `session/prompt`
        // with `stopReason: "cancelled"`. Best-effort — a wedged server still
        // dies by signal on shutdown.
        if self.profile.cancel_via_notification
            && let Some(session_id) = self.session_id.clone()
        {
            let line = schema::session_cancel_notification(&session_id);
            if let Err(e) = self.proc.send_raw(&line).await {
                tracing::warn!(
                    "{} session/cancel send failed ({e}); falling back to signal",
                    self.profile.label
                );
                return self.proc.interrupt().await;
            }
            return Ok(());
        }
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
    profile_label: &'static str,
    auth_fix_hint: &'static str,
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
                        if let Some(reason) = failure_stop_reason(stop_reason) {
                            vec![BridgeEvent::Failed {
                                run_id: self.run_id,
                                reason,
                                detail: Some(format!(
                                    "{} stopReason: {stop_reason}",
                                    self.profile_label
                                )),
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
                // Expired or revoked credentials surface here (e.g. `agy acp`
                // answers `session/new` with code -32000 "Authentication
                // required" when the cached sign-in is gone). The pre-pickup
                // presence check cannot see that, so report it as
                // `agent_auth_missing` with the agent's fix hint rather than a
                // plain crash.
                if schema::is_auth_required_error(&error) {
                    let message = error
                        .get("message")
                        .and_then(|m| m.as_str())
                        .unwrap_or("Authentication required");
                    return vec![BridgeEvent::Failed {
                        run_id: self.run_id,
                        reason: FailureReason::AgentAuthMissing,
                        detail: Some(format!(
                            "{}: {message}: {}",
                            self.profile_label, self.auth_fix_hint
                        )),
                    }];
                }
                let detail = error
                    .get("message")
                    .and_then(|m| m.as_str())
                    .map(ToOwned::to_owned)
                    .or_else(|| Some(format!("{} ACP error: {error}", self.profile_label)));
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

/// ACP `stopReason` values meaning the turn did not complete its work,
/// mapped to the specific `FailureReason` instead of a blanket
/// `AgentCrash` (PDASHOSS01-183): `cancelled` (interrupted) and `refusal`
/// (the model declined — current clouds record it as REFUSED). Everything
/// else (`end_turn`, `max_tokens`, `max_turn_requests`) is a natural end.
/// Mirrors the OpenClaw bridge.
fn failure_stop_reason(stop_reason: &str) -> Option<FailureReason> {
    match stop_reason {
        "cancelled" => Some(FailureReason::Cancelled),
        "refusal" => Some(FailureReason::Refusal),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const TEST_PROFILE: AcpProfile = AcpProfile {
        label: "test-acp",
        send_model_on_session_new: false,
        apply_model_via_config_option: true,
        cancel_via_notification: true,
        auth_fix_hint: "test fix hint",
    };

    fn cursor() -> BridgeCursor {
        BridgeCursor {
            profile_label: TEST_PROFILE.label,
            auth_fix_hint: TEST_PROFILE.auth_fix_hint,
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
    fn auth_required_error_maps_to_agent_auth_missing() {
        let mut c = cursor();
        let out = c.translate(msg(
            r#"{"id":2,"error":{"code":-32000,"message":"Authentication required"}}"#,
        ));
        match &out[0] {
            BridgeEvent::Failed { reason, detail, .. } => {
                assert!(matches!(reason, FailureReason::AgentAuthMissing));
                let detail = detail.as_deref().unwrap();
                assert!(detail.contains("Authentication required"));
                assert!(detail.contains("test fix hint"));
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

    /// Drive the real handshake + translate path against a fake ACP server
    /// that prints canned ACP frames in order and then drains stdin.
    /// This exercises `initialize` → `session/new` → `session/prompt` and the
    /// terminal `stopReason` without a real binary.
    #[tokio::test]
    async fn end_to_end_handshake_against_fake_server() {
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
        let proc = AcpProcess::spawn_command("test-acp", cmd)
            .await
            .expect("spawn fake ACP server");
        let mut bridge = Bridge::from_process(proc, TEST_PROFILE, None, None);

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
        assert!(
            saw_raw,
            "expected the streamed session/update as a Raw event"
        );
        assert!(completed, "expected the stopReason to complete the turn");
    }

    /// A configured model is applied through the advertised config options:
    /// the fake answers `session/new` with a Model option and expects a
    /// `session/set_config_option` selecting the configured value.
    #[tokio::test]
    async fn configured_model_applied_via_set_config_option() {
        // The fake pre-emits the initialize, session/new (advertising a
        // Model option) and set_config_option responses, then drains the
        // bridge's stdin writes into a file we inspect for the
        // `session/set_config_option` the bridge must send.
        let dir = tempfile::tempdir().expect("tempdir");
        let seen = dir.path().join("stdin.log");
        let script = format!(
            r#"
printf '%s\n' '{{"jsonrpc":"2.0","id":1,"result":{{"protocolVersion":1}}}}'
printf '%s\n' '{{"jsonrpc":"2.0","id":2,"result":{{"sessionId":"sess-m","configOptions":[{{"id":"model","name":"Model","category":"model","type":"select","currentValue":"model-a","options":[{{"value":"model-a"}},{{"value":"model-b"}}]}}]}}}}'
printf '%s\n' '{{"jsonrpc":"2.0","id":3,"result":{{"configOptions":[]}}}}'
cat > {}
exec cat >/dev/null
"#,
            seen.display()
        );
        let mut cmd = tokio::process::Command::new("bash");
        cmd.arg("-c").arg(&script);
        let proc = AcpProcess::spawn_command("test-acp", cmd)
            .await
            .expect("spawn fake ACP server");
        let mut bridge = Bridge::from_process(proc, TEST_PROFILE, Some("model-b".into()), None);

        let payload = RunPayload {
            run_id: Uuid::nil(),
            prompt: "do it".into(),
            model: None,
        };
        let cursor = bridge
            .run(&payload, Path::new("/tmp"))
            .await
            .expect("run drives the handshake");
        assert_eq!(cursor.thread_id, "sess-m");

        // Let the fake drain stdin, then shut down and inspect what the
        // bridge sent.
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        bridge
            .shutdown(std::time::Duration::from_secs(5))
            .await
            .ok();
        let logged = std::fs::read_to_string(&seen).unwrap_or_default();
        assert!(
            logged.contains("session/set_config_option"),
            "expected set_config_option in bridge stdin, got: {logged}"
        );
        assert!(
            logged.contains("model-b"),
            "expected the configured model value in stdin, got: {logged}"
        );
    }

    /// `session/resume` is used when the server advertises it and a resume
    /// id is known; the resumed id becomes the thread id.
    #[tokio::test]
    async fn resume_id_uses_session_resume_when_supported() {
        let dir = tempfile::tempdir().expect("tempdir");
        let seen = dir.path().join("stdin.log");
        let script = format!(
            r#"
printf '%s\n' '{{"jsonrpc":"2.0","id":1,"result":{{"protocolVersion":1,"agentCapabilities":{{"sessionCapabilities":{{"resume":{{}}}}}}}}}}'
printf '%s\n' '{{"jsonrpc":"2.0","id":2,"result":{{"sessionId":"sess-old"}}}}'
cat > {}
exec cat >/dev/null
"#,
            seen.display()
        );
        let mut cmd = tokio::process::Command::new("bash");
        cmd.arg("-c").arg(&script);
        let proc = AcpProcess::spawn_command("test-acp", cmd)
            .await
            .expect("spawn fake ACP server");
        let mut bridge = Bridge::from_process(proc, TEST_PROFILE, None, Some("sess-old".into()));

        let payload = RunPayload {
            run_id: Uuid::nil(),
            prompt: "continue".into(),
            model: None,
        };
        let cursor = bridge
            .run(&payload, Path::new("/tmp"))
            .await
            .expect("run drives resume");
        assert_eq!(cursor.thread_id, "sess-old");

        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        bridge
            .shutdown(std::time::Duration::from_secs(5))
            .await
            .ok();
        let logged = std::fs::read_to_string(&seen).unwrap_or_default();
        assert!(
            logged.contains("session/resume"),
            "expected session/resume in bridge stdin, got: {logged}"
        );
        assert!(
            !logged.contains("session/new"),
            "expected no fresh session/new, got: {logged}"
        );
    }

    /// The exact frame a real `agy acp` returns for `session/new` without
    /// cached credentials (Step 0 recording): the handshake fails with a
    /// typed `HandshakeAuthError` — not a plain string — so the supervisor
    /// reports `agent_auth_missing` with the fix hint instead of a crash.
    #[tokio::test]
    async fn handshake_session_new_auth_required_is_typed() {
        let script = r#"
printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1}}'
printf '%s\n' '{"jsonrpc":"2.0","error":{"code":-32000,"message":"Authentication required"},"id":2}'
exec cat >/dev/null
"#;
        let mut cmd = tokio::process::Command::new("bash");
        cmd.arg("-c").arg(script);
        let proc = AcpProcess::spawn_command("test-acp", cmd)
            .await
            .expect("spawn fake ACP server");
        let mut bridge = Bridge::from_process(proc, TEST_PROFILE, None, None);

        let payload = RunPayload {
            run_id: Uuid::nil(),
            prompt: "do it".into(),
            model: None,
        };
        let err = bridge
            .run(&payload, Path::new("/tmp"))
            .await
            .err()
            .expect("session/new -32000 must fail the handshake");
        let auth = HandshakeAuthError::find(&err).expect("handshake auth failure must be typed");
        assert_eq!(auth.label, TEST_PROFILE.label);
        let detail = auth.detail();
        assert!(
            detail.contains("Authentication required"),
            "detail must carry the server message, got: {detail}"
        );
        assert!(
            detail.contains(TEST_PROFILE.auth_fix_hint),
            "detail must carry the fix hint, got: {detail}"
        );
    }

    /// A non-auth handshake failure stays a plain (context-wrapped) error:
    /// the supervisor must keep reporting those as crashes, never as
    /// `agent_auth_missing`.
    #[tokio::test]
    async fn handshake_session_new_other_error_stays_untyped() {
        let script = r#"
printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1}}'
printf '%s\n' '{"jsonrpc":"2.0","id":2,"error":{"code":-32603,"message":"internal error"}}'
exec cat >/dev/null
"#;
        let mut cmd = tokio::process::Command::new("bash");
        cmd.arg("-c").arg(script);
        let proc = AcpProcess::spawn_command("test-acp", cmd)
            .await
            .expect("spawn fake ACP server");
        let mut bridge = Bridge::from_process(proc, TEST_PROFILE, None, None);

        let payload = RunPayload {
            run_id: Uuid::nil(),
            prompt: "do it".into(),
            model: None,
        };
        let err = bridge
            .run(&payload, Path::new("/tmp"))
            .await
            .err()
            .expect("session/new error must fail the handshake");
        assert!(
            HandshakeAuthError::find(&err).is_none(),
            "non-auth handshake failure must stay untyped, got: {err:#}"
        );
        assert!(
            format!("{err:#}").contains("session/new failed"),
            "non-auth failure must keep its step context, got: {err:#}"
        );
    }

    #[test]
    fn failure_stop_reasons_map_specifically() {
        assert!(matches!(
            failure_stop_reason("cancelled"),
            Some(FailureReason::Cancelled)
        ));
        assert!(matches!(
            failure_stop_reason("refusal"),
            Some(FailureReason::Refusal)
        ));
        assert!(failure_stop_reason("end_turn").is_none());
        assert!(failure_stop_reason("max_tokens").is_none());
    }
}
