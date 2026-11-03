use anyhow::{Context, Result};
use chrono::Utc;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use uuid::Uuid;

// Re-export the shared types under their legacy `codex::bridge::` path so
// existing integration tests and any downstream callers that imported them
// from here keep compiling. The canonical definitions live in `agent::`.
pub use crate::agent::{BridgeEvent, RunPayload};
use crate::agent::ENGINE_STARTUP_TIMEOUT;
use crate::approval::EngineThreadSettings;
use crate::cloud::protocol::{ApprovalDecision, ApprovalKind, FailureReason};
use crate::codex::app_server::AppServer;
use crate::util::shell::AgentEnv;
use crate::codex::jsonrpc::{self, Incoming};
use crate::codex::schema::{
    ApprovalResponse, ClientInfo, InitializeParams, NotificationKind, ThreadStartParams,
    TurnInputItem, TurnInterruptParams, TurnStartParams,
};

/// How long a new turn waits for an interrupted one to report its end.
const INTERRUPT_SETTLE_TIMEOUT: Duration = Duration::from_secs(10);

pub struct Bridge {
    pub server: AppServer,
    pub model_default: Option<String>,
    /// Reasoning-effort tier sent on every `turn/start` (`low`/`medium`/
    /// `high`/`xhigh`). `None` omits the field so codex uses the model's
    /// default effort. Sourced from `[runner.codex].effort_default`.
    pub effort_default: Option<String>,
    /// The `sandbox` / `approval_policy` this bridge stamps on every thread it
    /// starts. Defaults to the historical full-access posture; the local chat
    /// lane overrides it per session from the user's chosen approval mode.
    pub engine_settings: EngineThreadSettings,
    initialized: bool,
    thread_id: Option<String>,
    /// Request id of a `turn/start` whose response (carrying the turn id) has
    /// not been read yet.
    turn_start_request: Option<u64>,
    /// Id of the turn in flight. `turn/interrupt` is rejected without it.
    turn_id: Option<String>,
    /// An interrupt was asked for; the turn has not reported its end yet. The
    /// next turn settles this first, so the interrupted turn's remaining
    /// frames are not read as its own.
    interrupting: bool,
    /// Notifications that arrived while we were waiting for an RPC response
    /// (e.g. an early `account/reauthRequired` during `initialize`). Drained
    /// by [`Bridge::next_frame`] before reading from the live stream so the
    /// supervisor's translator sees them.
    pending: std::collections::VecDeque<Incoming>,
    /// Approval requests the engine is waiting on, shared with the cursors
    /// that raise them.
    server_requests: ServerRequests,
}

/// Engine approval requests still unanswered: approval id → JSON-RPC request id.
type ServerRequests = Arc<Mutex<HashMap<String, serde_json::Value>>>;

impl Bridge {
    pub async fn spawn(
        binary: &str,
        cwd: &Path,
        model_default: Option<String>,
        effort_default: Option<String>,
    ) -> Result<Self> {
        Self::spawn_with_env(
            binary,
            cwd,
            model_default,
            effort_default,
            EngineThreadSettings::default(),
            &AgentEnv::default(),
        )
        .await
    }

    /// [`Bridge::spawn`] with a Pi Dash-controlled agent environment and the
    /// per-thread engine posture derived from the caller's approval mode.
    pub async fn spawn_with_env(
        binary: &str,
        cwd: &Path,
        model_default: Option<String>,
        effort_default: Option<String>,
        engine_settings: EngineThreadSettings,
        env: &AgentEnv,
    ) -> Result<Self> {
        let server = AppServer::spawn_with_env(binary, cwd, env).await?;
        Ok(Self {
            server,
            model_default,
            effort_default,
            engine_settings,
            initialized: false,
            thread_id: None,
            turn_start_request: None,
            turn_id: None,
            interrupting: false,
            pending: std::collections::VecDeque::new(),
            server_requests: ServerRequests::default(),
        })
    }

    /// Bridge that speaks to an already-constructed AppServer. Used in
    /// integration tests to substitute a fake process.
    pub fn from_server(server: AppServer, model_default: Option<String>) -> Self {
        Self {
            server,
            model_default,
            effort_default: None,
            engine_settings: EngineThreadSettings::default(),
            initialized: false,
            thread_id: None,
            turn_start_request: None,
            turn_id: None,
            interrupting: false,
            pending: std::collections::VecDeque::new(),
            server_requests: ServerRequests::default(),
        }
    }

    /// Returns the next frame from Codex, consulting the pending buffer first
    /// so notifications stashed during `await_response` are not lost.
    pub async fn next_frame(&mut self) -> Option<Incoming> {
        let frame = match self.pending.pop_front() {
            Some(f) => f,
            None => self.server.inbound.recv().await?,
        };
        self.track_turn(&frame);
        if let Incoming::Request { id, method, .. } = &frame
            && !NotificationKind::from_method(method).is_approval_request()
        {
            // Nothing here answers this request, and the engine holds the turn
            // until someone does.
            let refusal =
                jsonrpc::error_response(id, -32601, &format!("{method} is not supported"));
            match refusal {
                Ok(line) => {
                    if let Err(e) = self.server.send_raw(&line).await {
                        tracing::warn!("refusing codex request {method} failed: {e:#}");
                    }
                }
                Err(e) => tracing::warn!("refusing codex request {method} failed: {e:#}"),
            }
        }
        Some(frame)
    }

    /// Follow the turn in flight from the frames going past.
    fn track_turn(&mut self, frame: &Incoming) {
        match frame {
            Incoming::Response { id, result, .. } if self.turn_start_request == Some(*id) => {
                self.turn_start_request = None;
                self.turn_id = result.as_ref().and_then(turn_id_of);
                // The turn never started, so there is nothing to wait out.
                if self.turn_id.is_none() {
                    self.interrupting = false;
                }
            }
            Incoming::Notification { method, .. } if method == "turn/completed" => {
                self.turn_start_request = None;
                self.turn_id = None;
                self.interrupting = false;
                // The engine drops a turn's unanswered requests when it ends.
                self.server_requests.lock().unwrap().clear();
            }
            _ => {}
        }
    }

    /// Wait for an interrupted turn to end, discarding what is left of it.
    /// Without this the next turn would read the old turn's frames as its own.
    async fn settle_interrupted_turn(&mut self) -> Result<()> {
        let deadline = tokio::time::Instant::now() + INTERRUPT_SETTLE_TIMEOUT;
        while self.interrupting {
            // The interrupt could not be sent before the turn id was known.
            if self.turn_id.is_some() {
                self.send_interrupt().await?;
            }
            tokio::time::timeout_at(deadline, self.next_frame())
                .await
                .context("interrupted codex turn did not end")?
                .context("codex stdout closed while interrupting a turn")?;
        }
        Ok(())
    }

    /// Prepare the app-server for a chat turn without sending user input.
    ///
    /// A warm chat bridge keeps the same Codex app-server process alive for
    /// multiple turns. Codex accepts `initialize` once per process, and the
    /// conversation is anchored by the thread created after initialization.
    pub async fn warm(&mut self, cwd: &Path) -> Result<String> {
        self.ensure_initialized().await?;
        if let Some(thread_id) = self.thread_id.as_ref() {
            return Ok(thread_id.clone());
        }
        let thread_id = self.start_thread(cwd).await?;
        self.thread_id = Some(thread_id.clone());
        Ok(thread_id)
    }

    pub async fn run(&mut self, payload: &RunPayload, cwd: &Path) -> Result<BridgeCursor> {
        let thread_id = self.warm(cwd).await?;
        self.settle_interrupted_turn().await?;
        self.start_turn(&thread_id, payload).await?;
        Ok(BridgeCursor {
            run_id: payload.run_id,
            thread_id,
            model: payload.model.clone().or_else(|| self.model_default.clone()),
            seq: 0,
            server_requests: self.server_requests.clone(),
        })
    }

    async fn ensure_initialized(&mut self) -> Result<()> {
        if self.initialized {
            return Ok(());
        }
        let id = self.server.alloc_id();
        let line = jsonrpc::request(
            id,
            "initialize",
            &InitializeParams {
                client_info: ClientInfo {
                    name: "pidash".to_string(),
                    version: crate::RUNNER_VERSION.to_string(),
                },
            },
        )?;
        self.server.send_raw(&line).await?;
        // Wait for init response.
        let _ = self.await_response(id, ENGINE_STARTUP_TIMEOUT).await?;
        let line = jsonrpc::notification("initialized", &serde_json::Value::Null)?;
        self.server.send_raw(&line).await?;
        self.initialized = true;
        Ok(())
    }

    async fn start_thread(&mut self, cwd: &Path) -> Result<String> {
        let id = self.server.alloc_id();
        let line = jsonrpc::request(
            id,
            "thread/start",
            &ThreadStartParams {
                cwd: cwd.to_string_lossy().to_string(),
                model: self.model_default.clone(),
                // Per-thread posture. Defaults to the historical full-access
                // stance; the local chat lane overrides it from the user's
                // chosen approval mode (see `approval::mode`). The managed
                // config's own `sandbox_mode`/`approval_policy` are shadowed by
                // these per-thread values, which is why they must be set here.
                sandbox: self.engine_settings.sandbox.into(),
                approval_policy: self.engine_settings.approval_policy.into(),
            },
        )?;
        self.server.send_raw(&line).await?;
        let resp = self.await_response(id, ENGINE_STARTUP_TIMEOUT).await?;
        resp.get("threadId")
            .or_else(|| resp.get("thread").and_then(|t| t.get("id")))
            .or_else(|| resp.get("thread_id"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .context("thread/start missing threadId")
    }

    async fn start_turn(&mut self, thread_id: &str, payload: &RunPayload) -> Result<()> {
        let id = self.server.alloc_id();
        let line = jsonrpc::request(
            id,
            "turn/start",
            &TurnStartParams {
                thread_id: thread_id.to_string(),
                input: vec![TurnInputItem {
                    item_type: "text".into(),
                    text: payload.prompt.clone(),
                }],
                model: payload.model.clone().or_else(|| self.model_default.clone()),
                effort: self.effort_default.clone(),
            },
        )?;
        self.server.send_raw(&line).await?;
        self.turn_start_request = Some(id);
        Ok(())
    }

    pub async fn send_approval(
        &mut self,
        approval_id: &str,
        decision: ApprovalDecision,
    ) -> Result<()> {
        // The engine asked with a JSON-RPC request and resumes the turn only
        // on the response to that request id.
        let request_id = self
            .server_requests
            .lock()
            .unwrap()
            .remove(approval_id)
            .with_context(|| format!("codex is not waiting on approval {approval_id}"))?;
        let line = jsonrpc::response(
            &request_id,
            &ApprovalResponse {
                decision: match decision {
                    ApprovalDecision::Accept => "accept",
                    ApprovalDecision::Decline => "decline",
                    ApprovalDecision::AcceptForSession => "acceptForSession",
                },
            },
        )?;
        self.server.send_raw(&line).await
    }

    pub async fn interrupt(&mut self) -> Result<()> {
        if self.turn_start_request.is_none() && self.turn_id.is_none() {
            return Ok(());
        }
        self.interrupting = true;
        self.send_interrupt().await
    }

    /// Send `turn/interrupt` for the turn in flight. A no-op until its id is
    /// known; [`Bridge::settle_interrupted_turn`] retries once it is.
    async fn send_interrupt(&mut self) -> Result<()> {
        let (Some(thread_id), Some(turn_id)) = (self.thread_id.clone(), self.turn_id.take()) else {
            return Ok(());
        };
        let id = self.server.alloc_id();
        let line = jsonrpc::request(
            id,
            "turn/interrupt",
            &TurnInterruptParams { thread_id, turn_id },
        )?;
        self.server.send_raw(&line).await
    }

    async fn await_response(&mut self, id: u64, timeout: Duration) -> Result<serde_json::Value> {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            let frame = tokio::time::timeout(remaining, self.server.inbound.recv())
                .await
                .context("codex response timed out")?;
            match frame {
                Some(Incoming::Response {
                    id: got,
                    result,
                    error,
                    ..
                }) if got == id => {
                    if let Some(err) = error {
                        anyhow::bail!("codex rpc error {}: {}", err.code, err.message);
                    }
                    return Ok(result.unwrap_or(serde_json::Value::Null));
                }
                Some(Incoming::Response { .. }) => continue,
                Some(other) => {
                    // Buffer notifications and requests received while
                    // awaiting an RPC response so the translator sees them
                    // later.
                    self.pending.push_back(other);
                }
                None => anyhow::bail!("codex stdout closed while awaiting response"),
            }
        }
    }
}

fn turn_id_of(value: &serde_json::Value) -> Option<String> {
    value
        .get("turn")
        .and_then(|turn| turn.get("id"))
        .and_then(|id| id.as_str())
        .map(|id| id.to_string())
}

pub struct BridgeCursor {
    pub run_id: Uuid,
    pub thread_id: String,
    pub model: Option<String>,
    pub seq: u64,
    /// Approval requests raised by this cursor that the engine still waits on.
    server_requests: ServerRequests,
}

impl BridgeCursor {
    /// Translate one inbound Codex frame into daemon-level events.
    pub fn translate(&mut self, frame: Incoming) -> Vec<BridgeEvent> {
        self.seq = self.seq.saturating_add(1);
        match frame {
            Incoming::Notification { method, params, .. } => {
                let kind = NotificationKind::from_method(&method);
                if matches!(kind, NotificationKind::AccountReauthRequired) {
                    vec![BridgeEvent::AwaitingReauth {
                        run_id: self.run_id,
                        detail: params
                            .get("detail")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string()),
                    }]
                } else if matches!(kind, NotificationKind::TurnCompleted) {
                    // Treat success only when codex explicitly says so. Any
                    // other value (including a missing field) means the turn
                    // ended without a clean signal — the prior default of
                    // "success" silently masked codex errors that landed
                    // a `turn/completed` with no conclusion right after a
                    // systemError, so the runner reported "completed" on
                    // 400s from OpenAI.
                    let conclusion = params.get("conclusion").and_then(|v| v.as_str());
                    let turn_status = params
                        .get("turn")
                        .and_then(|v| v.get("status"))
                        .and_then(|v| v.as_str());
                    let turn_error = params
                        .get("turn")
                        .and_then(|v| v.get("error"))
                        .filter(|v| !v.is_null());
                    if conclusion == Some("success")
                        || (conclusion.is_none()
                            && turn_error.is_none()
                            && matches!(turn_status, Some("completed" | "succeeded")))
                    {
                        vec![BridgeEvent::Completed {
                            run_id: self.run_id,
                            done_payload: params.get("done").cloned().unwrap_or_else(|| {
                                serde_json::json!({
                                    "conclusion": conclusion.unwrap_or("success"),
                                    "turn": params.get("turn"),
                                    "ended_at": Utc::now().to_rfc3339(),
                                })
                            }),
                        }]
                    } else {
                        vec![BridgeEvent::Failed {
                            run_id: self.run_id,
                            reason: FailureReason::Internal,
                            detail: params
                                .get("error")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string())
                                .or_else(|| {
                                    conclusion.map(|c| format!("turn ended with conclusion={c:?}"))
                                })
                                .or_else(|| {
                                    turn_error.map(|e| format!("turn completed with error: {e}"))
                                })
                                .or_else(|| Some("turn/completed without conclusion".to_string())),
                        }]
                    }
                } else if method == "error" {
                    // Codex emits this for transport / API failures (e.g. a
                    // 400 from OpenAI saying the model isn't allowed). When
                    // `willRetry` is false it's terminal, so fail the run
                    // instead of silently logging it as a Raw event and
                    // waiting for a `turn/completed` that may or may not
                    // arrive.
                    let will_retry = params
                        .get("willRetry")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    if will_retry {
                        vec![BridgeEvent::Raw {
                            run_id: self.run_id,
                            method,
                            params,
                        }]
                    } else {
                        let detail = params
                            .get("error")
                            .and_then(|e| e.get("message"))
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string());
                        vec![BridgeEvent::Failed {
                            run_id: self.run_id,
                            reason: FailureReason::Internal,
                            detail,
                        }]
                    }
                } else {
                    vec![BridgeEvent::Raw {
                        run_id: self.run_id,
                        method,
                        params,
                    }]
                }
            }
            Incoming::Request {
                id, method, params, ..
            } => {
                let kind = match NotificationKind::from_method(&method) {
                    NotificationKind::CommandExecutionRequestApproval => {
                        ApprovalKind::CommandExecution
                    }
                    NotificationKind::FileChangeRequestApproval => ApprovalKind::FileChange,
                    _ => {
                        return vec![BridgeEvent::Raw {
                            run_id: self.run_id,
                            method,
                            params,
                        }];
                    }
                };
                let approval_id = params
                    .get("approval_id")
                    .or_else(|| params.get("approvalId"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| Uuid::new_v4().to_string());
                let reason = params
                    .get("reason")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                // The engine holds the turn until this request id is answered;
                // `Bridge::send_approval` looks it up by approval id.
                self.server_requests
                    .lock()
                    .unwrap()
                    .insert(approval_id.clone(), id);
                vec![BridgeEvent::ApprovalRequest {
                    run_id: self.run_id,
                    approval_id,
                    kind,
                    payload: params,
                    reason,
                }]
            }
            Incoming::Response { .. } => Vec::new(),
        }
    }
}

#[cfg(test)]
mod translate_tests {
    use super::*;
    use serde_json::json;

    fn cursor() -> BridgeCursor {
        BridgeCursor {
            run_id: Uuid::new_v4(),
            thread_id: "th_test".into(),
            model: None,
            seq: 0,
            server_requests: ServerRequests::default(),
        }
    }

    fn notif(method: &str, params: serde_json::Value) -> Incoming {
        Incoming::Notification {
            jsonrpc: Some("2.0".into()),
            method: method.into(),
            params,
        }
    }

    #[test]
    fn turn_completed_without_conclusion_fails_run() {
        // Codex sometimes emits a bare `turn/completed` after a systemError.
        // The previous default of "success" silently masked those failures.
        let mut c = cursor();
        let evs = c.translate(notif("turn/completed", json!({})));
        assert_eq!(evs.len(), 1);
        match &evs[0] {
            BridgeEvent::Failed { detail, .. } => {
                assert_eq!(detail.as_deref(), Some("turn/completed without conclusion"));
            }
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[test]
    fn turn_completed_with_explicit_success_completes_run() {
        let mut c = cursor();
        let evs = c.translate(notif("turn/completed", json!({"conclusion": "success"})));
        assert!(matches!(evs.as_slice(), [BridgeEvent::Completed { .. }]));
    }

    #[test]
    fn turn_completed_with_completed_turn_status_completes_run() {
        let mut c = cursor();
        let evs = c.translate(notif(
            "turn/completed",
            json!({"turn": {"status": "completed", "error": null}}),
        ));
        assert!(matches!(evs.as_slice(), [BridgeEvent::Completed { .. }]));
    }

    #[test]
    fn turn_completed_with_turn_error_fails_run() {
        let mut c = cursor();
        let evs = c.translate(notif(
            "turn/completed",
            json!({"turn": {"status": "completed", "error": {"message": "boom"}}}),
        ));
        match evs.as_slice() {
            [BridgeEvent::Failed { detail, .. }] => {
                assert!(detail.as_deref().unwrap_or("").contains("boom"));
            }
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[test]
    fn turn_completed_with_non_success_conclusion_fails_run() {
        let mut c = cursor();
        let evs = c.translate(notif("turn/completed", json!({"conclusion": "aborted"})));
        match evs.as_slice() {
            [BridgeEvent::Failed { detail, .. }] => {
                assert_eq!(
                    detail.as_deref(),
                    Some("turn ended with conclusion=\"aborted\"")
                );
            }
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[test]
    fn error_notification_with_will_retry_false_fails_run() {
        let mut c = cursor();
        let evs = c.translate(notif(
            "error",
            json!({
                "willRetry": false,
                "error": {"message": "model not allowed"}
            }),
        ));
        match evs.as_slice() {
            [BridgeEvent::Failed { detail, .. }] => {
                assert_eq!(detail.as_deref(), Some("model not allowed"));
            }
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[test]
    fn error_notification_with_will_retry_true_passes_through_as_raw() {
        let mut c = cursor();
        let evs = c.translate(notif(
            "error",
            json!({
                "willRetry": true,
                "error": {"message": "transient"}
            }),
        ));
        assert!(matches!(evs.as_slice(), [BridgeEvent::Raw { .. }]));
    }

    fn request(id: serde_json::Value, method: &str, params: serde_json::Value) -> Incoming {
        Incoming::Request {
            jsonrpc: Some("2.0".into()),
            id,
            method: method.into(),
            params,
        }
    }

    #[test]
    fn command_approval_request_becomes_an_approval_bound_to_its_request_id() {
        let mut c = cursor();
        let evs = c.translate(request(
            json!(0),
            "item/commandExecution/requestApproval",
            json!({"itemId": "it_1", "command": "rm -rf /tmp/x", "cwd": "/work", "reason": "why"}),
        ));
        let approval_id = match evs.as_slice() {
            [
                BridgeEvent::ApprovalRequest {
                    approval_id,
                    kind,
                    payload,
                    reason,
                    ..
                },
            ] => {
                assert!(matches!(kind, ApprovalKind::CommandExecution));
                assert_eq!(payload["command"], "rm -rf /tmp/x");
                assert_eq!(reason.as_deref(), Some("why"));
                approval_id.clone()
            }
            other => panic!("expected ApprovalRequest, got {other:?}"),
        };
        assert_eq!(
            c.server_requests.lock().unwrap().get(&approval_id),
            Some(&json!(0))
        );
    }

    #[test]
    fn file_change_approval_request_keeps_the_engine_approval_id() {
        let mut c = cursor();
        let evs = c.translate(request(
            json!("req-7"),
            "item/fileChange/requestApproval",
            json!({"itemId": "it_2", "approvalId": "a-9"}),
        ));
        match evs.as_slice() {
            [
                BridgeEvent::ApprovalRequest {
                    approval_id, kind, ..
                },
            ] => {
                assert_eq!(approval_id, "a-9");
                assert!(matches!(kind, ApprovalKind::FileChange));
            }
            other => panic!("expected ApprovalRequest, got {other:?}"),
        }
        assert_eq!(
            c.server_requests.lock().unwrap().get("a-9"),
            Some(&json!("req-7"))
        );
    }

    #[test]
    fn waiting_on_approval_status_alone_opens_no_approval() {
        // The flag only mirrors the approval request; a prompt raised from it
        // would have no request to answer.
        let mut c = cursor();
        let _ = c.translate(notif(
            "item/started",
            json!({"item": {"id": "it_1", "type": "commandExecution", "status": "inProgress", "command": "ls"}}),
        ));
        let evs = c.translate(notif(
            "thread/status/changed",
            json!({"status": {"activeFlags": ["waitingOnApproval"]}}),
        ));
        assert!(matches!(evs.as_slice(), [BridgeEvent::Raw { .. }]));
        assert!(c.server_requests.lock().unwrap().is_empty());
    }

    #[test]
    fn other_server_requests_pass_through_as_raw() {
        let mut c = cursor();
        let evs = c.translate(request(json!(3), "item/tool/requestUserInput", json!({})));
        assert!(matches!(evs.as_slice(), [BridgeEvent::Raw { .. }]));
        assert!(c.server_requests.lock().unwrap().is_empty());
    }

    #[test]
    fn thread_status_without_waiting_flag_is_raw() {
        let mut c = cursor();
        let evs = c.translate(notif(
            "thread/status/changed",
            json!({"status": {"activeFlags": ["running"]}}),
        ));
        assert!(matches!(evs.as_slice(), [BridgeEvent::Raw { .. }]));
    }
}
