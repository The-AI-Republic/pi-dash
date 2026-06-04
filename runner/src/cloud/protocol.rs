use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;

/// Wire version — bump on incompatible shape changes.
///
/// v4 (current): per-runner HTTPS long-poll transport. The daemon presents
/// the shared dev-machine token and identifies the speaking runner by URL
/// or ``X-Runner-Id``. It opens one session per runner via
/// ``POST /runners/<rid>/sessions/`` and polls
/// ``POST /runners/<rid>/sessions/<sid>/poll`` for control-plane messages.
/// ``Hello``/``Heartbeat``/``Bye``/``Ping`` are folded into HTTP
/// request/response bodies; ``ForceRefresh`` is retained for legacy
/// refresh-token clients.
///
/// v3 (retired): always-on WebSocket per Connection.
///
/// v2 (retired): multi-runner Hello fan-out over WS.
pub const WIRE_VERSION: u32 = 4;

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunEventRecord {
    pub seq: u32,
    pub kind: String,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input: u64,
    pub output: u64,
    pub total: u64,
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<String>,
    },
    RunEvent {
        run_id: Uuid,
        seq: u64,
        kind: String,
        payload: serde_json::Value,
    },
    RunEvents {
        run_id: Uuid,
        events: Vec<RunEventRecord>,
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tokens: Option<TokenUsage>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<String>,
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tokens: Option<TokenUsage>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<String>,
    },
    RunFailed {
        run_id: Uuid,
        reason: FailureReason,
        detail: Option<String>,
        ended_at: DateTime<Utc>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tokens: Option<TokenUsage>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<String>,
    },
    RunCancelled {
        run_id: Uuid,
        cancelled_at: DateTime<Utc>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tokens: Option<TokenUsage>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<String>,
    },
    RunResumed {
        run_id: Uuid,
        thread_id: String,
        elapsed_ms: u64,
    },
    ChatStarted {
        chat_session_id: Uuid,
        local_thread_id: String,
        local_session_id: Option<String>,
        started_at: DateTime<Utc>,
    },
    ChatMessageStarted {
        chat_session_id: Uuid,
        message_id: Uuid,
        turn_id: Option<String>,
        started_at: DateTime<Utc>,
    },
    ChatEvent {
        chat_session_id: Uuid,
        bridge_seq: u64,
        kind: String,
        payload: serde_json::Value,
    },
    ChatApprovalRequest {
        chat_session_id: Uuid,
        local_approval_id: String,
        kind: ApprovalKind,
        payload: serde_json::Value,
        reason: Option<String>,
        expires_at: Option<DateTime<Utc>>,
    },
    ChatMessageCompleted {
        chat_session_id: Uuid,
        message_id: Uuid,
        turn_id: Option<String>,
        assistant_message: Option<String>,
        status: String,
        completed_at: DateTime<Utc>,
    },
    ChatFailed {
        chat_session_id: Uuid,
        code: String,
        detail: Option<String>,
        failed_at: DateTime<Utc>,
    },
    ChatClosed {
        chat_session_id: Uuid,
        closed_at: DateTime<Utc>,
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
        /// Latest available runner version the cloud is announcing.
        /// Daemons compare against their own `CARGO_PKG_VERSION` and
        /// either swap the on-disk binary (when `auto_update` is on) or
        /// surface a "restart to apply" / "update available" advisory.
        /// Optional + `skip_serializing_if = None` for forward-compat:
        /// older clouds that don't set this look the same on the wire.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        latest_runner_version: Option<String>,
        /// Minimum acceptable runner version. Advisory today; surfaced
        /// in TUI/status as a red banner so operators can act before
        /// the cloud bumps the wire-protocol floor.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        min_runner_version: Option<String>,
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
    /// Force the runner to perform an inline refresh before its next
    /// scheduled refresh window. ``min_rtg`` is the lowest acceptable
    /// refresh-token generation; the runner must have rotated past
    /// this value before any access token issued before the rotation
    /// is accepted server-side. See ``design.md`` §5.2 / §7.8.
    ForceRefresh {
        #[serde(default)]
        reason: Option<String>,
        #[serde(default)]
        min_rtg: Option<u64>,
    },
    ChatUserMessage {
        chat_session_id: Uuid,
        message_id: Uuid,
        content: String,
        #[serde(default)]
        content_parts: Vec<serde_json::Value>,
        #[serde(default)]
        local_thread_id: Option<String>,
        #[serde(default)]
        local_session_id: Option<String>,
        #[serde(default)]
        cwd: Option<String>,
        #[serde(default)]
        model: Option<String>,
    },
    ChatWarm {
        chat_session_id: Uuid,
        #[serde(default)]
        local_thread_id: Option<String>,
        #[serde(default)]
        local_session_id: Option<String>,
        #[serde(default)]
        cwd: Option<String>,
        #[serde(default)]
        model: Option<String>,
    },
    ChatCancel {
        chat_session_id: Uuid,
        reason: Option<String>,
    },
    ChatClose {
        chat_session_id: Uuid,
        reason: Option<String>,
    },
    ChatDecide {
        chat_session_id: Uuid,
        approval_id: Uuid,
        local_approval_id: String,
        decision: ApprovalDecision,
        decided_by: Option<String>,
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
    /// Legacy: native session resume failed. No longer emitted by current
    /// runners (resume support was removed; see
    /// `.ai_design/ticking_optimization/design.md`). Variant is kept so older
    /// runners can still serialize this reason to a current cloud without
    /// deserialization errors.
    ResumeUnavailable,
    /// The daemon process is shutting down (SIGTERM, e.g. `pidash restart`,
    /// systemd stop, host reboot). The run cannot continue past this point.
    /// Sent eagerly during the daemon's drain step so the cloud transitions
    /// the run to FAILED via a deliberate signal instead of inferring it
    /// from the heartbeat reaper after the next reconnect.
    DaemonRestart,
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
    fn roundtrips_server_welcome_with_version_advisory() {
        let msg = ServerMsg::Welcome {
            server_time: Utc::now(),
            heartbeat_interval_secs: 25,
            protocol_version: WIRE_VERSION,
            latest_runner_version: Some("0.1.3".into()),
            min_runner_version: Some("0.1.2".into()),
        };
        let env = Envelope::new(msg);
        let s = serde_json::to_string(&env).unwrap();
        let back: Envelope<ServerMsg> = serde_json::from_str(&s).unwrap();
        match back.body {
            ServerMsg::Welcome {
                latest_runner_version,
                min_runner_version,
                ..
            } => {
                assert_eq!(latest_runner_version.as_deref(), Some("0.1.3"));
                assert_eq!(min_runner_version.as_deref(), Some("0.1.2"));
            }
            other => panic!("expected Welcome, got {other:?}"),
        }
    }

    #[test]
    fn server_welcome_omits_version_advisory_when_absent() {
        // Older clouds that don't announce a version must produce the
        // exact same wire bytes they always have. `skip_serializing_if`
        // drops the optional fields entirely; no `null` on the wire.
        let msg = ServerMsg::Welcome {
            server_time: Utc::now(),
            heartbeat_interval_secs: 25,
            protocol_version: WIRE_VERSION,
            latest_runner_version: None,
            min_runner_version: None,
        };
        let env = Envelope::new(msg);
        let s = serde_json::to_string(&env).unwrap();
        assert!(
            !s.contains("latest_runner_version"),
            "expected latest_runner_version omitted: {s}",
        );
        assert!(
            !s.contains("min_runner_version"),
            "expected min_runner_version omitted: {s}",
        );
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
