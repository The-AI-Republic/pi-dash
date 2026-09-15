//! Leaf data-transfer objects the IPC `Response` reaches into.
//!
//! These types were extracted from the runner (`cloud::protocol`,
//! `history::index`, `approval::router`, `daemon::state`,
//! `daemon::observability`, `cli::doctor`) so that the desktop app — and
//! any other client, like the direct-chat surface in PDASHOSS01-159 — can
//! speak the daemon's IPC without depending on the whole runner crate. The
//! runner re-exports every type here from its original module path, so no
//! runner-internal call site changed. There is **no wire-format change**:
//! the field shapes, `serde` attributes, and derives are byte-for-byte the
//! originals.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// cloud::protocol
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunnerStatus {
    Idle,
    Busy,
    Reconnecting,
    AwaitingReauth,
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

// ---------------------------------------------------------------------------
// history::index
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunSummary {
    pub run_id: Uuid,
    pub work_item_id: Option<Uuid>,
    pub status: String,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub title: Option<String>,
}

// ---------------------------------------------------------------------------
// approval::router
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionSource {
    Local,
    Cloud,
    Policy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalRecord {
    pub approval_id: String,
    /// Runner this approval belongs to. Stamped at `open()` time so
    /// the TUI can route a `Decide` to the right instance even if the
    /// user changes the runner picker between selection and decision.
    /// Defaulted to nil for back-compat with records minted before the
    /// field landed.
    #[serde(default)]
    pub runner_id: Uuid,
    pub run_id: Uuid,
    pub kind: ApprovalKind,
    pub payload: serde_json::Value,
    pub reason: Option<String>,
    pub requested_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub status: ApprovalStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalStatus {
    Pending,
    Resolved {
        decision: ApprovalDecision,
        source: DecisionSource,
        decided_at: DateTime<Utc>,
    },
    Expired,
}

// ---------------------------------------------------------------------------
// daemon::observability
// ---------------------------------------------------------------------------

/// Streaming token usage parsed opportunistically from a Codex
/// `codex/event/token_count` Raw frame. Claude does not emit equivalent
/// streaming counts during a run, so this is left `None` for Claude.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input: u64,
    pub output: u64,
    pub total: u64,
}

// ---------------------------------------------------------------------------
// daemon::state
// ---------------------------------------------------------------------------

/// Volatile observability fields that ride on `PollStatus`.
///
/// **Ownership (B2):** this snapshot is written **only by the assign (issue)
/// lane** — `set_agent_pid`, `set_tokens`, `set_model`, `incr_turn`,
/// `note_agent_event`, `note_exec_command*`. The chat lane MUST NOT write any of
/// these (it has no per-lane slot here); per-lane chat observability is deferred
/// (design `make_chat_issue_parallel_working` §3.3 B2 / Phase 3). With the two
/// lanes running concurrently, a single shared snapshot would otherwise
/// last-write-wins between two agents.
///
/// Doubles as
/// the in-memory storage shape (held under one `Mutex` inside `Inner`)
/// AND the wire-snapshot returned by `StateHandle::observability_snapshot()`.
/// Keeping them in one struct means `reset_run_snapshot()` is a single
/// `Default::default()` assignment and a new field added here is automatically
/// included in reset, snapshot read, and rid-change wipe — no enumeration to
/// keep in sync.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObservabilitySnapshot {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_event_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_event_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_event_summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_pid: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_subprocess_alive: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens: Option<TokenUsage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_count: Option<u32>,
    /// Last shell command tool the agent kicked off (for failure-detail
    /// enrichment), including whether a matching completion/result event was
    /// later observed. Reset on rid change like the other per-run scalars;
    /// never serialised onto the wire — only consumed locally to enrich
    /// `RunFailed.detail` when the watchdog or stdout-close path fires.
    #[serde(skip)]
    pub last_exec_command: Option<ExecCommandSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecCommandSnapshot {
    pub command: String,
    pub cwd: Option<String>,
    pub tool_call_id: Option<String>,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    /// `Some(true)` when the matching completion event reported a clean
    /// terminal status, `Some(false)` for a non-success terminal (codex
    /// `failed` status, Claude `is_error: true`). `None` when no
    /// completion has been observed yet or the protocol frame didn't
    /// surface an outcome.
    pub completed_success: Option<bool>,
}

// ---------------------------------------------------------------------------
// cli::doctor
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Report {
    pub checks: Vec<Check>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Check {
    pub name: String,
    pub ok: bool,
    pub detail: String,
    pub blocker: bool,
}

impl Report {
    pub fn has_blockers(&self) -> bool {
        self.checks.iter().any(|c| c.blocker && !c.ok)
    }

    pub fn print_compact(&self) {
        for c in &self.checks {
            let mark = if c.ok { "✓" } else { "✗" };
            println!(
                "  {mark} {name:<14} {detail}",
                mark = mark,
                name = c.name,
                detail = c.detail
            );
        }
    }
}
