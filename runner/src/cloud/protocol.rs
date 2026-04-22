use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;

/// Wire version — bump on incompatible shape changes.
pub const WIRE_VERSION: u32 = 1;

/// All frames carry `v`, `type`, `mid` (message id for dedupe).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope<T> {
    #[serde(rename = "v")]
    pub version: u32,
    #[serde(rename = "mid")]
    pub message_id: Uuid,
    #[serde(flatten)]
    pub body: T,
}

impl<T> Envelope<T> {
    pub fn new(body: T) -> Self {
        Self {
            version: WIRE_VERSION,
            message_id: Uuid::new_v4(),
            body,
        }
    }
}

/// Messages the runner sends to the cloud.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMsg {
    Hello {
        runner_id: Uuid,
        version: String,
        os: String,
        arch: String,
        status: RunnerStatus,
        in_flight_run: Option<Uuid>,
        protocol_version: u32,
    },
    Heartbeat {
        ts: DateTime<Utc>,
        status: RunnerStatus,
        in_flight_run: Option<Uuid>,
    },
    Accept {
        run_id: Uuid,
        workspace_state: WorkspaceState,
    },
    RunStarted {
        run_id: Uuid,
        thread_id: String,
        started_at: DateTime<Utc>,
    },
    RunEvent {
        run_id: Uuid,
        seq: u64,
        kind: String,
        payload: serde_json::Value,
    },
    ApprovalRequest {
        run_id: Uuid,
        approval_id: Uuid,
        kind: ApprovalKind,
        payload: serde_json::Value,
        reason: Option<String>,
        expires_at: Option<DateTime<Utc>>,
    },
    RunAwaitingReauth {
        run_id: Uuid,
        detail: Option<String>,
    },
    RunCompleted {
        run_id: Uuid,
        done_payload: serde_json::Value,
        ended_at: DateTime<Utc>,
    },
    RunFailed {
        run_id: Uuid,
        reason: FailureReason,
        detail: Option<String>,
        ended_at: DateTime<Utc>,
    },
    RunCancelled {
        run_id: Uuid,
        cancelled_at: DateTime<Utc>,
    },
    RunResumed {
        run_id: Uuid,
        thread_id: String,
        elapsed_ms: u64,
    },
    Bye {
        reason: String,
    },
}

/// Messages the cloud sends to the runner.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMsg {
    Welcome {
        server_time: DateTime<Utc>,
        heartbeat_interval_secs: u64,
        protocol_version: u32,
    },
    Assign {
        run_id: Uuid,
        work_item_id: Option<Uuid>,
        prompt: String,
        repo_url: Option<String>,
        repo_ref: Option<String>,
        /// Existing branch the agent must check out and commit onto. When
        /// `None`, the runner creates a fresh feature branch off `repo_ref`
        /// (or the remote default if `repo_ref` is also `None`).
        #[serde(default)]
        git_work_branch: Option<String>,
        expected_codex_model: Option<String>,
        approval_policy_overrides: Option<BTreeMap<String, serde_json::Value>>,
        deadline: Option<DateTime<Utc>>,
    },
    Cancel {
        run_id: Uuid,
        reason: Option<String>,
    },
    Decide {
        run_id: Uuid,
        approval_id: Uuid,
        decision: ApprovalDecision,
        decided_by: Option<String>,
    },
    ConfigPush {
        approval_policy: Option<serde_json::Value>,
    },
    Ping {
        ts: DateTime<Utc>,
    },
    Revoke {
        reason: String,
    },
    ResumeAck {
        run_id: Uuid,
        last_seq: Option<u64>,
        status: String,
        thread_id: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunnerStatus {
    Idle,
    Busy,
    Reconnecting,
    AwaitingReauth,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceState {
    pub branch: Option<String>,
    pub dirty: bool,
    pub head: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalKind {
    CommandExecution,
    FileChange,
    NetworkAccess,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalDecision {
    Accept,
    Decline,
    AcceptForSession,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureReason {
    WorkspaceSetup,
    GitAuth,
    Network,
    /// Codex subprocess crashed or exited abnormally. Kept agent-specific so
    /// pre-existing dashboards filtering on `"codex_crash"` keep matching.
    CodexCrash,
    /// Non-Codex agent subprocess crashed or exited abnormally. Introduced
    /// with the Claude Code agent so its failures don't conflate with Codex
    /// failures in telemetry.
    AgentCrash,
    /// Agent hit its turn/step budget (e.g. Claude `error_max_turns`). Not
    /// the same as an internal bug: the run was deliberately bounded and
    /// ran out of room to complete.
    MaxTurns,
    Timeout,
    Internal,
    Cancelled,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrips_client_hello() {
        let msg = ClientMsg::Hello {
            runner_id: Uuid::new_v4(),
            version: "0.1.0".into(),
            os: "linux".into(),
            arch: "x86_64".into(),
            status: RunnerStatus::Idle,
            in_flight_run: None,
            protocol_version: WIRE_VERSION,
        };
        let env = Envelope::new(msg);
        let s = serde_json::to_string(&env).unwrap();
        let back: Envelope<ClientMsg> = serde_json::from_str(&s).unwrap();
        assert_eq!(back.version, WIRE_VERSION);
    }

    #[test]
    fn roundtrips_server_assign() {
        let msg = ServerMsg::Assign {
            run_id: Uuid::new_v4(),
            work_item_id: Some(Uuid::new_v4()),
            prompt: "do the thing".into(),
            repo_url: Some("https://example.invalid/x.git".into()),
            repo_ref: Some("main".into()),
            git_work_branch: Some("feat/existing-branch".into()),
            expected_codex_model: None,
            approval_policy_overrides: None,
            deadline: None,
        };
        let env = Envelope::new(msg);
        let s = serde_json::to_string(&env).unwrap();
        let back: Envelope<ServerMsg> = serde_json::from_str(&s).unwrap();
        assert_eq!(back.version, WIRE_VERSION);
    }
}
