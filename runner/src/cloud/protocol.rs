use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;

/// Wire version — bump on incompatible shape changes.
///
/// v3 (current): WS auth is per-Connection. The daemon presents
/// ``Authorization: Bearer <connection_secret>`` + ``X-Connection-Id``
/// on the upgrade. Runners come online individually via Hello frames
/// over that connection; runner_id stays purely a routing key.
///
/// v2 (retired): introduced multi-runner Hello fan-out + RunPaused +
/// optional `Envelope.runner_id`. Legacy per-runner-secret auth was
/// still permitted alongside token-based auth.
pub const WIRE_VERSION: u32 = 3;

/// All frames carry `v`, `type`, `mid` (message id for dedupe). Multi-runner
/// frames also carry `runner_id` so the cloud's demux can route to the right
/// runner record on a connection that authenticates as a token owning N
/// runners. The field is `Option` (with `skip_serializing_if`) so legacy
/// single-runner traffic stays byte-identical on the wire — only frames that
/// explicitly set a runner_id include the field.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope<T> {
    #[serde(rename = "v")]
    pub version: u32,
    #[serde(rename = "mid")]
    pub message_id: Uuid,
    /// Routing discriminator for multi-runner traffic. See `design.md` §4.2:
    /// connection-scoped frames (`Ping`, `Bye`, connection-wide `Revoke`)
    /// leave it `None`; everything else carries `Some(runner_id)` once
    /// multi-runner ships. Today's single-runner daemon may set it from the
    /// only configured runner's id (still backward-compatible — the cloud's
    /// connection-bound auth context resolves identity either way).
    ///
    /// Serialised as `rid` on the wire so it doesn't collide with the
    /// `runner_id` field that `Hello` carries in its body (the body's
    /// `runner_id` is part of the auth handshake; the envelope's is for
    /// routing once the connection is up).
    #[serde(rename = "rid", default, skip_serializing_if = "Option::is_none")]
    pub runner_id: Option<Uuid>,
    #[serde(flatten)]
    pub body: T,
}

impl<T> Envelope<T> {
    pub fn new(body: T) -> Self {
        Self {
            version: WIRE_VERSION,
            message_id: Uuid::new_v4(),
            runner_id: None,
            body,
        }
    }

    /// Build an envelope with the per-runner routing discriminator set.
    /// Use this for any frame that targets one runner: `Hello`, `Welcome`,
    /// `Heartbeat`, `Assign`, `RunStarted`, etc.
    pub fn for_runner(runner_id: Uuid, body: T) -> Self {
        Self {
            version: WIRE_VERSION,
            message_id: Uuid::new_v4(),
            runner_id: Some(runner_id),
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
        /// Project identifier this runner serves. Optional purely for
        /// back-compat with daemons enrolled before the project refactor;
        /// when present, the cloud cross-checks it against
        /// `runner.pod.project.identifier` and emits `RemoveRunner` on a
        /// mismatch. See
        /// `.ai_design/n_runners_in_same_machine/new_pod_project_relationship/design.md` §7.4.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        project_slug: Option<String>,
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
    /// Agent yielded with a question for the human (Codex tool-call path).
    /// Cloud transitions the run to PAUSED_AWAITING_INPUT and waits for a
    /// continuation trigger (typically a non-bot comment on the issue).
    /// Claude's yield uses the `pi-dash-done` fenced block instead and
    /// arrives via `RunCompleted` → cloud's done-signal parser.
    RunPaused {
        run_id: Uuid,
        payload: serde_json::Value,
        paused_at: DateTime<Utc>,
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
        /// Provider session id to resume. When set, the runner asks the
        /// agent CLI to reattach to that session (`thread/resume` for
        /// Codex, `--resume` for Claude). Field-only addition — backward
        /// compatible with older clouds that omit it.
        #[serde(default)]
        resume_thread_id: Option<String>,
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
    /// Cloud-initiated per-runner removal (UI's "Remove" action on a
    /// runner row, or `pidash token remove-runner` on a different
    /// machine). The daemon cancels that runner's in-flight run, drops
    /// it from `instances`, and deletes its data directory. Connection
    /// and other runners stay up. See `design.md` §11.4.
    ///
    /// Envelope's `runner_id` carries the target — the body's
    /// `runner_id` is included as a self-contained sanity check so a
    /// view fed only the body still has the id.
    RemoveRunner {
        runner_id: Uuid,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
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
    /// Native session resume was requested (Assign carried `resume_thread_id`)
    /// but the agent CLI couldn't find the session on disk — the runner was
    /// reinstalled, the session store was wiped, or the id is otherwise
    /// stale. Cloud's response is to drop the pin and re-queue with a fresh
    /// session.
    ResumeUnavailable,
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
            project_slug: None,
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
            resume_thread_id: Some("sess_xyz".into()),
        };
        let env = Envelope::new(msg);
        let s = serde_json::to_string(&env).unwrap();
        let back: Envelope<ServerMsg> = serde_json::from_str(&s).unwrap();
        assert_eq!(back.version, WIRE_VERSION);
    }

    #[test]
    fn envelope_runner_id_roundtrips_when_set() {
        let id = Uuid::new_v4();
        let msg = ClientMsg::Heartbeat {
            ts: Utc::now(),
            status: RunnerStatus::Idle,
            in_flight_run: None,
        };
        let env = Envelope::for_runner(id, msg);
        let s = serde_json::to_string(&env).unwrap();
        // Wire field is `rid`, not `runner_id`, so it doesn't collide with
        // body fields named `runner_id` (e.g. on `Hello`).
        assert!(s.contains("\"rid\""), "expected rid on the wire: {s}");
        let back: Envelope<ClientMsg> = serde_json::from_str(&s).unwrap();
        assert_eq!(back.runner_id, Some(id));
    }

    #[test]
    fn envelope_omits_runner_id_when_none() {
        // Connection-scoped frames (Ping/Bye/connection-Revoke equivalent)
        // leave `runner_id = None`. The wire bytes must not include the
        // `rid` field at all so legacy v1 cloud sees identical traffic.
        let msg = ClientMsg::Bye {
            reason: "shutdown".into(),
        };
        let env = Envelope::new(msg);
        let s = serde_json::to_string(&env).unwrap();
        assert!(!s.contains("\"rid\""), "expected no rid: {s}");
    }

    #[test]
    fn envelope_with_hello_body_does_not_collide_with_envelope_runner_id() {
        // Hello's body carries a `runner_id` (auth handshake); the envelope
        // carries its own routing discriminator under the `rid` wire name.
        // Both must round-trip independently.
        let body_id = Uuid::new_v4();
        let env_id = Uuid::new_v4();
        let msg = ClientMsg::Hello {
            runner_id: body_id,
            version: "0.1.0".into(),
            os: "linux".into(),
            arch: "x86_64".into(),
            status: RunnerStatus::Idle,
            in_flight_run: None,
            protocol_version: WIRE_VERSION,
            project_slug: None,
        };
        let env = Envelope::for_runner(env_id, msg);
        let s = serde_json::to_string(&env).unwrap();
        let back: Envelope<ClientMsg> = serde_json::from_str(&s).unwrap();
        assert_eq!(back.runner_id, Some(env_id));
        match back.body {
            ClientMsg::Hello { runner_id, .. } => assert_eq!(runner_id, body_id),
            other => panic!("expected Hello, got {other:?}"),
        }
    }
}
