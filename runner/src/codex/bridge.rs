use anyhow::{Context, Result};
use chrono::Utc;
use serde_json::json;
use std::path::Path;
use std::time::Duration;
use uuid::Uuid;

use crate::cloud::protocol::{ApprovalDecision, ApprovalKind, FailureReason};
use crate::codex::app_server::AppServer;
use crate::codex::jsonrpc::{self, Incoming};
use crate::codex::schema::{
    ApprovalResponseParams, ClientInfo, InitializeParams, NotificationKind, ThreadResumeParams,
    ThreadStartParams, TurnInputItem, TurnStartParams,
};

/// Events the bridge surfaces to the daemon's state machine.
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
        self.start_turn(payload).await?;
        Ok(BridgeCursor {
            run_id: payload.run_id,
            thread_id,
            seq: 0,
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
        let _ = self.await_response(id, Duration::from_secs(30)).await?;
        Ok(thread_id.to_string())
    }

    async fn start_turn(&mut self, payload: &RunPayload) -> Result<()> {
        let id = self.server.alloc_id();
        let line = jsonrpc::request(
            id,
            "turn/start",
            &TurnStartParams {
                input: vec![TurnInputItem {
                    role: "user".into(),
                    content: payload.prompt.clone(),
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
                    let conclusion = params
                        .get("conclusion")
                        .and_then(|v| v.as_str())
                        .unwrap_or("success");
                    if conclusion == "failed" {
                        vec![BridgeEvent::Failed {
                            run_id: self.run_id,
                            reason: FailureReason::Internal,
                            detail: params
                                .get("error")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string()),
                        }]
                    } else {
                        vec![BridgeEvent::Completed {
                            run_id: self.run_id,
                            done_payload: params.get("done").cloned().unwrap_or_else(|| {
                                serde_json::json!({
                                    "conclusion": conclusion,
                                    "ended_at": Utc::now().to_rfc3339(),
                                })
                            }),
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
            Incoming::Response { .. } => Vec::new(),
        }
    }
}
