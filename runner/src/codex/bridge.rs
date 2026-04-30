use anyhow::{Context, Result};
use chrono::Utc;
use serde_json::json;
use std::path::Path;
use std::time::Duration;
use uuid::Uuid;

// Re-export the shared types under their legacy `codex::bridge::` path so
// existing integration tests and any downstream callers that imported them
// from here keep compiling. The canonical definitions live in `agent::`.
pub use crate::agent::{BridgeEvent, RunPayload};
use crate::cloud::protocol::{ApprovalDecision, ApprovalKind, FailureReason};
use crate::codex::app_server::AppServer;
use crate::codex::jsonrpc::{self, Incoming};
use crate::codex::schema::{
    ApprovalResponseParams, ClientInfo, InitializeParams, NotificationKind, ThreadResumeParams,
    ThreadStartParams, TurnInputItem, TurnStartParams,
};

pub struct Bridge {
    pub server: AppServer,
    pub model_default: Option<String>,
    /// Notifications that arrived while we were waiting for an RPC response
    /// (e.g. an early `account/reauthRequired` during `initialize`). Drained
    /// by [`Bridge::next_frame`] before reading from the live stream so the
    /// supervisor's translator sees them.
    pending: std::collections::VecDeque<Incoming>,
}

impl Bridge {
    pub async fn spawn(binary: &str, cwd: &Path, model_default: Option<String>) -> Result<Self> {
        let server = AppServer::spawn(binary, cwd).await?;
        Ok(Self {
            server,
            model_default,
            pending: std::collections::VecDeque::new(),
        })
    }

    /// Bridge that speaks to an already-constructed AppServer. Used in
    /// integration tests to substitute a fake process.
    pub fn from_server(server: AppServer, model_default: Option<String>) -> Self {
        Self {
            server,
            model_default,
            pending: std::collections::VecDeque::new(),
        }
    }

    /// Returns the next frame from Codex, consulting the pending buffer first
    /// so notifications stashed during `await_response` are not lost.
    pub async fn next_frame(&mut self) -> Option<Incoming> {
        if let Some(f) = self.pending.pop_front() {
            return Some(f);
        }
        self.server.inbound.recv().await
    }

    pub async fn run(&mut self, payload: &RunPayload, cwd: &Path) -> Result<BridgeCursor> {
        self.initialize().await?;
        let thread_id = if let Some(existing) = &payload.resume_thread_id {
            self.resume_thread(existing).await?
        } else {
            self.start_thread(cwd).await?
        };
        self.start_turn(&thread_id, payload).await?;
        Ok(BridgeCursor {
            run_id: payload.run_id,
            thread_id,
            seq: 0,
            pending_command: None,
            pending_command_approval_id: None,
        })
    }

    async fn initialize(&mut self) -> Result<()> {
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
        let _ = self.await_response(id, Duration::from_secs(15)).await?;
        let line = jsonrpc::notification("initialized", &serde_json::Value::Null)?;
        self.server.send_raw(&line).await?;
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
                sandbox_policy: "workspace-write".into(),
                approval_policy: "on-request".into(),
            },
        )?;
        self.server.send_raw(&line).await?;
        let resp = self.await_response(id, Duration::from_secs(30)).await?;
        resp.get("threadId")
            .or_else(|| resp.get("thread").and_then(|t| t.get("id")))
            .or_else(|| resp.get("thread_id"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .context("thread/start missing threadId")
    }

    async fn resume_thread(&mut self, thread_id: &str) -> Result<String> {
        let id = self.server.alloc_id();
        let line = jsonrpc::request(
            id,
            "thread/resume",
            &ThreadResumeParams {
                thread_id: thread_id.to_string(),
            },
        )?;
        self.server.send_raw(&line).await?;
        match self.await_response(id, Duration::from_secs(30)).await {
            Ok(_) => Ok(thread_id.to_string()),
            Err(e) => {
                // The Codex app-server returns an error when the thread id
                // is unknown locally (session store wiped, runner reinstalled,
                // id stale). Surface as a typed error so the supervisor can
                // emit FailureReason::ResumeUnavailable rather than the
                // generic CodexCrash, and the cloud can drop the pin.
                Err(anyhow::Error::new(crate::agent::ResumeUnavailable {
                    thread_id: thread_id.to_string(),
                    detail: format!("{e:#}"),
                }))
            }
        }
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
                effort: None,
            },
        )?;
        self.server.send_raw(&line).await
    }

    pub async fn send_approval(
        &mut self,
        approval_id: &str,
        decision: ApprovalDecision,
    ) -> Result<()> {
        let line = jsonrpc::notification(
            "approval/response",
            &ApprovalResponseParams {
                approval_id: approval_id.to_string(),
                decision: match decision {
                    ApprovalDecision::Accept => "accept".into(),
                    ApprovalDecision::Decline => "decline".into(),
                    ApprovalDecision::AcceptForSession => "accept_for_session".into(),
                },
            },
        )?;
        self.server.send_raw(&line).await
    }

    pub async fn interrupt(&mut self) -> Result<()> {
        let id = self.server.alloc_id();
        let line = jsonrpc::request(id, "turn/interrupt", &json!({}))?;
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
                Some(other @ Incoming::Notification { .. }) => {
                    // Buffer notifications received while awaiting an RPC
                    // response so the translator sees them later.
                    self.pending.push_back(other);
                }
                Some(_) => continue,
                None => anyhow::bail!("codex stdout closed while awaiting response"),
            }
        }
    }
}

pub struct BridgeCursor {
    pub run_id: Uuid,
    pub thread_id: String,
    pub seq: u64,
    /// Most recent in-progress `commandExecution` item observed on
    /// `item/started`. Captured so we can synthesize an `ApprovalRequest`
    /// when codex sets `thread/status/changed → activeFlags:
    /// ["waitingOnApproval"]` (codex 0.125.0 stopped firing the legacy
    /// `item/commandExecution/requestApproval` notification, leaving the
    /// in-progress item + the flag as the only signal we get).
    pending_command: Option<PendingCommand>,
    /// Approval id last opened for `pending_command`. Stored so a stray
    /// re-fire of the `waitingOnApproval` flag (e.g. status notifications
    /// sent more than once for the same item) doesn't open duplicate
    /// approval rows.
    pending_command_approval_id: Option<String>,
}

#[derive(Debug, Clone)]
struct PendingCommand {
    item_id: String,
    command: String,
    cwd: Option<String>,
    raw: serde_json::Value,
}

impl BridgeCursor {
    /// Translate one inbound Codex frame into daemon-level events.
    pub fn translate(&mut self, frame: Incoming) -> Vec<BridgeEvent> {
        self.seq = self.seq.saturating_add(1);
        match frame {
            Incoming::Notification { method, params, .. } => {
                let kind = NotificationKind::from_method(&method);
                if kind.is_approval_request() {
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
                    let ak = match kind {
                        NotificationKind::CommandExecutionRequestApproval => {
                            ApprovalKind::CommandExecution
                        }
                        NotificationKind::FileChangeRequestApproval => ApprovalKind::FileChange,
                        _ => ApprovalKind::Other,
                    };
                    vec![BridgeEvent::ApprovalRequest {
                        run_id: self.run_id,
                        approval_id,
                        kind: ak,
                        payload: params,
                        reason,
                    }]
                } else if matches!(kind, NotificationKind::AccountReauthRequired) {
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
                    let conclusion = params
                        .get("conclusion")
                        .and_then(|v| v.as_str());
                    if conclusion == Some("success") {
                        vec![BridgeEvent::Completed {
                            run_id: self.run_id,
                            done_payload: params.get("done").cloned().unwrap_or_else(|| {
                                serde_json::json!({
                                    "conclusion": "success",
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
                } else if method == "thread/status/changed" {
                    // Codex 0.125.0+ signals "I'm parked waiting for the
                    // user to approve a command" by setting this flag,
                    // *without* firing the legacy
                    // `item/commandExecution/requestApproval` notification.
                    // If we don't translate it into an ApprovalRequest, the
                    // run deadlocks: codex blocks on stdin reading the
                    // approval response, the runner blocks on stdout
                    // reading frames that will never arrive.
                    let waiting = params
                        .get("status")
                        .and_then(|s| s.get("activeFlags"))
                        .and_then(|f| f.as_array())
                        .map(|arr| {
                            arr.iter().any(|v| v.as_str() == Some("waitingOnApproval"))
                        })
                        .unwrap_or(false);
                    if waiting && let Some(pending) = self.pending_command.clone() {
                        // Reuse the open approval id when the same flag
                        // re-fires so we don't open duplicate rows.
                        let approval_id = self
                            .pending_command_approval_id
                            .clone()
                            .unwrap_or_else(|| Uuid::new_v4().to_string());
                        self.pending_command_approval_id = Some(approval_id.clone());
                        let reason = Some(format!(
                            "codex requesting approval to run: {}",
                            pending.command
                        ));
                        // Synthesise a payload that downstream consumers
                        // (cloud-side ApprovalRequest serializer, TUI
                        // approval card) can render the same way as a real
                        // codex-fired approval.
                        let payload = serde_json::json!({
                            "command": pending.command,
                            "cwd": pending.cwd,
                            "item_id": pending.item_id,
                            "synthesized": true,
                            "raw": pending.raw,
                        });
                        return vec![BridgeEvent::ApprovalRequest {
                            run_id: self.run_id,
                            approval_id,
                            kind: ApprovalKind::CommandExecution,
                            payload,
                            reason,
                        }];
                    }
                    vec![BridgeEvent::Raw {
                        run_id: self.run_id,
                        method,
                        params,
                    }]
                } else if method == "item/started" {
                    // Cache the most recent in-progress commandExecution so
                    // a later `waitingOnApproval` flag can refer to it.
                    if let Some(item) = params.get("item")
                        && item.get("type").and_then(|v| v.as_str())
                            == Some("commandExecution")
                        && item.get("status").and_then(|v| v.as_str())
                            == Some("inProgress")
                    {
                        let item_id = item
                            .get("id")
                            .and_then(|v| v.as_str())
                            .unwrap_or_default()
                            .to_string();
                        let command = item
                            .get("command")
                            .and_then(|v| v.as_str())
                            .unwrap_or_default()
                            .to_string();
                        let cwd = item
                            .get("cwd")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string());
                        self.pending_command = Some(PendingCommand {
                            item_id,
                            command,
                            cwd,
                            raw: item.clone(),
                        });
                        // New item-started → previous waitingOnApproval (if any)
                        // is no longer relevant; allow a fresh approval id next
                        // time codex reports waiting.
                        self.pending_command_approval_id = None;
                    }
                    vec![BridgeEvent::Raw {
                        run_id: self.run_id,
                        method,
                        params,
                    }]
                } else if method == "item/completed" {
                    // The cached command finished; drop the tracking so a
                    // late waitingOnApproval doesn't re-open it.
                    if let Some(item) = params.get("item")
                        && item.get("type").and_then(|v| v.as_str())
                            == Some("commandExecution")
                    {
                        let same = self
                            .pending_command
                            .as_ref()
                            .and_then(|pc| {
                                item.get("id")
                                    .and_then(|v| v.as_str())
                                    .map(|id| id == pc.item_id)
                            })
                            .unwrap_or(false);
                        if same {
                            self.pending_command = None;
                            self.pending_command_approval_id = None;
                        }
                    }
                    vec![BridgeEvent::Raw {
                        run_id: self.run_id,
                        method,
                        params,
                    }]
                } else {
                    vec![BridgeEvent::Raw {
                        run_id: self.run_id,
                        method,
                        params,
                    }]
                }
            }
            Incoming::Response { .. } => Vec::new(),
        }
    }
}
