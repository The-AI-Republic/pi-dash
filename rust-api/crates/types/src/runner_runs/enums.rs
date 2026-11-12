//! Runner run/chat enums + status sets (D-15, stage 5).
//!
//! Port of the `TextChoices` behind the run/chat columns in
//! `apps/api/pi_dash/runner/models.py`:
//!
//! * `AgentRunStatus` (`:207-236`) → [`AgentRunStatus`].
//! * `AgentRunTrigger` (`:237-253`) → [`AgentRunTrigger`].
//! * `HUMAN_TRIGGERS` (`:259-266`) → [`HUMAN_TRIGGERS`].
//! * `AUTOMATIC_ISSUE_TRIGGERS` (`:273`) → [`AUTOMATIC_ISSUE_TRIGGERS`].
//! * `run_is_human_triggered` (`:276-278`) → [`run_is_human_triggered`].
//! * `RefusalCategory` (`:281-294`) → [`RefusalCategory`].
//! * `ApprovalStatus` (`:295-299`) → [`ApprovalStatus`].
//! * `ApprovalKind` (`:302-306`) → [`ApprovalKind`].
//! * `AgentChatSessionStatus` (`:309-312`) → [`AgentChatSessionStatus`].
//! * `AgentChatMessageRole` (`:315-319`) → [`AgentChatMessageRole`].
//! * `AgentChatMessageStatus` (`:322-330`) → [`AgentChatMessageStatus`].
//! * `ToolCallStatus` (`:1178-1186`) → [`ToolCallStatus`].
//! * `is_terminal` (`:1137-1144`) → [`AgentRunStatus::is_terminal`].
//! * `is_active` (`:1147-1159`) → [`AgentRunStatus::is_active`].
//! * `TERMINAL_RUN_STATUSES` (`services/run_lifecycle.py:39-45`) →
//!   [`TERMINAL_RUN_STATUSES`].
//!
//! Translation notes:
//!
//! * Same enum API as the D-11 ports (PIDASHCONV-482, `db::dispatch::status`):
//!   `Display` renders the value as `str(member)` does in Django, serde
//!   reads/writes the value, and unknown values are rejected as Django
//!   choice validation would. D-11 keeps its own `db`-local copy of
//!   `AgentRunStatus`/`AgentRunTrigger` and explicitly deferred
//!   `is_terminal`/`is_active`/`HUMAN_TRIGGERS` to the layer that needs
//!   them — this module is that layer (L2 models and D-14 reuse it).
//! * `run_is_human_triggered` takes the trigger value: Python takes the run
//!   row and reads `.trigger` off it, but this crate holds values, not rows.
//! * The metrics in-flight set (`views/metrics.py:40-46`) is a different
//!   5-set (no `QUEUED`/`WAITING_FOR_WORKTREE`); it lives with the other
//!   view consts in [`crate::runner_runs::consts::ACTIVE_RUN_STATUSES`],
//!   not here, so the two sets cannot be confused.
//!
//! Fixture: `rust-api/fixtures/runner_runs/fx-run-01-types-pure.golden.json`
//! (FX-RUN-01 `enums`).
//!
//! Ported bugs: none found in these units on read-through.

use serde::{Deserialize, Serialize};

/// Run lifecycle states (`models.py:207-236`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentRunStatus {
    Queued,
    Assigned,
    WaitingForWorktree,
    Running,
    CancelRequested,
    AwaitingApproval,
    AwaitingReauth,
    PausedAwaitingInput,
    Blocked,
    Completed,
    Failed,
    Cancelled,
    Refused,
}

impl AgentRunStatus {
    /// The `TextChoices` value (`queued`, …).
    pub fn value(&self) -> &'static str {
        match self {
            AgentRunStatus::Queued => "queued",
            AgentRunStatus::Assigned => "assigned",
            AgentRunStatus::WaitingForWorktree => "waiting_for_worktree",
            AgentRunStatus::Running => "running",
            AgentRunStatus::CancelRequested => "cancel_requested",
            AgentRunStatus::AwaitingApproval => "awaiting_approval",
            AgentRunStatus::AwaitingReauth => "awaiting_reauth",
            AgentRunStatus::PausedAwaitingInput => "paused_awaiting_input",
            AgentRunStatus::Blocked => "blocked",
            AgentRunStatus::Completed => "completed",
            AgentRunStatus::Failed => "failed",
            AgentRunStatus::Cancelled => "cancelled",
            AgentRunStatus::Refused => "refused",
        }
    }

    /// The human label (`models.py:208-236`).
    pub fn label(&self) -> &'static str {
        match self {
            AgentRunStatus::Queued => "Queued",
            AgentRunStatus::Assigned => "Assigned",
            AgentRunStatus::WaitingForWorktree => "Waiting for Worktree",
            AgentRunStatus::Running => "Running",
            AgentRunStatus::CancelRequested => "Cancellation Requested",
            AgentRunStatus::AwaitingApproval => "Awaiting Approval",
            AgentRunStatus::AwaitingReauth => "Awaiting Reauth",
            AgentRunStatus::PausedAwaitingInput => "Paused — Awaiting Input",
            AgentRunStatus::Blocked => "Blocked",
            AgentRunStatus::Completed => "Completed",
            AgentRunStatus::Failed => "Failed",
            AgentRunStatus::Cancelled => "Cancelled",
            AgentRunStatus::Refused => "Refused",
        }
    }

    /// Parse a stored value (`AgentRunStatus.values` membership).
    pub fn from_value(value: &str) -> Option<Self> {
        match value {
            "queued" => Some(AgentRunStatus::Queued),
            "assigned" => Some(AgentRunStatus::Assigned),
            "waiting_for_worktree" => Some(AgentRunStatus::WaitingForWorktree),
            "running" => Some(AgentRunStatus::Running),
            "cancel_requested" => Some(AgentRunStatus::CancelRequested),
            "awaiting_approval" => Some(AgentRunStatus::AwaitingApproval),
            "awaiting_reauth" => Some(AgentRunStatus::AwaitingReauth),
            "paused_awaiting_input" => Some(AgentRunStatus::PausedAwaitingInput),
            "blocked" => Some(AgentRunStatus::Blocked),
            "completed" => Some(AgentRunStatus::Completed),
            "failed" => Some(AgentRunStatus::Failed),
            "cancelled" => Some(AgentRunStatus::Cancelled),
            "refused" => Some(AgentRunStatus::Refused),
            _ => None,
        }
    }

    /// `AgentRun.is_terminal` (`models.py:1137-1144`).
    pub fn is_terminal(&self) -> bool {
        TERMINAL_RUN_STATUSES.contains(self)
    }

    /// `AgentRun.is_active` (`models.py:1147-1159`): active runs occupy the
    /// single-active-run slot per issue (mirrors the
    /// `agent_run_one_active_per_work_item` constraint).
    pub fn is_active(&self) -> bool {
        matches!(
            self,
            AgentRunStatus::Queued
                | AgentRunStatus::Assigned
                | AgentRunStatus::WaitingForWorktree
                | AgentRunStatus::Running
                | AgentRunStatus::CancelRequested
                | AgentRunStatus::AwaitingApproval
                | AgentRunStatus::AwaitingReauth
        )
    }
}

impl std::fmt::Display for AgentRunStatus {
    /// `str(member)`: the value.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.value())
    }
}

/// Terminal run states (`services/run_lifecycle.py:39-45`), in tuple order.
/// Same members as `is_terminal` and as finalization's `TERMINAL_STATUSES`
/// set; this is the one L4 reuses.
pub const TERMINAL_RUN_STATUSES: [AgentRunStatus; 5] = [
    AgentRunStatus::Completed,
    AgentRunStatus::Failed,
    AgentRunStatus::Cancelled,
    AgentRunStatus::Blocked,
    AgentRunStatus::Refused,
];

/// How a run came to be created (`models.py:237-253`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentRunTrigger {
    StateTransition,
    RunAi,
    CommentAndRun,
    Tick,
    Scheduler,
    Direct,
}

impl AgentRunTrigger {
    /// The `TextChoices` value (`state_transition`, …).
    pub fn value(&self) -> &'static str {
        match self {
            AgentRunTrigger::StateTransition => "state_transition",
            AgentRunTrigger::RunAi => "run_ai",
            AgentRunTrigger::CommentAndRun => "comment_and_run",
            AgentRunTrigger::Tick => "tick",
            AgentRunTrigger::Scheduler => "scheduler",
            AgentRunTrigger::Direct => "direct",
        }
    }

    /// The human label (`models.py:248-253`).
    pub fn label(&self) -> &'static str {
        match self {
            AgentRunTrigger::StateTransition => "State transition",
            AgentRunTrigger::RunAi => "Run AI button",
            AgentRunTrigger::CommentAndRun => "Comment & Run",
            AgentRunTrigger::Tick => "Automatic tick",
            AgentRunTrigger::Scheduler => "Scheduler beat",
            AgentRunTrigger::Direct => "Direct",
        }
    }

    /// Parse a stored value (`AgentRunTrigger.values` membership).
    pub fn from_value(value: &str) -> Option<Self> {
        match value {
            "state_transition" => Some(AgentRunTrigger::StateTransition),
            "run_ai" => Some(AgentRunTrigger::RunAi),
            "comment_and_run" => Some(AgentRunTrigger::CommentAndRun),
            "tick" => Some(AgentRunTrigger::Tick),
            "scheduler" => Some(AgentRunTrigger::Scheduler),
            "direct" => Some(AgentRunTrigger::Direct),
            _ => None,
        }
    }
}

impl std::fmt::Display for AgentRunTrigger {
    /// `str(member)`: the value.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.value())
    }
}

/// Triggers that count as a human directly initiating the run
/// (`models.py:259-266`): only these resolve per-user prompt overrides.
pub const HUMAN_TRIGGERS: [AgentRunTrigger; 4] = [
    AgentRunTrigger::StateTransition,
    AgentRunTrigger::RunAi,
    AgentRunTrigger::CommentAndRun,
    AgentRunTrigger::Direct,
];

/// Issue-run triggers the ticking clock starts on its own
/// (`models.py:273`): only the cadence tick qualifies.
pub const AUTOMATIC_ISSUE_TRIGGERS: [AgentRunTrigger; 1] = [AgentRunTrigger::Tick];

/// True when the trigger means a human directly initiated the run
/// (`run_is_human_triggered`, `models.py:276-278`).
pub fn run_is_human_triggered(trigger: AgentRunTrigger) -> bool {
    HUMAN_TRIGGERS.contains(&trigger)
}

/// Safety-classifier decline category (`models.py:281-294`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RefusalCategory {
    Cyber,
    Bio,
    ReasoningExtraction,
    Unknown,
}

impl RefusalCategory {
    /// The `TextChoices` value (`cyber`, …).
    pub fn value(&self) -> &'static str {
        match self {
            RefusalCategory::Cyber => "cyber",
            RefusalCategory::Bio => "bio",
            RefusalCategory::ReasoningExtraction => "reasoning_extraction",
            RefusalCategory::Unknown => "unknown",
        }
    }

    /// The human label (`models.py:288-292`).
    pub fn label(&self) -> &'static str {
        match self {
            RefusalCategory::Cyber => "Cyber",
            RefusalCategory::Bio => "Bio",
            RefusalCategory::ReasoningExtraction => "Reasoning Extraction",
            RefusalCategory::Unknown => "Unknown",
        }
    }

    /// Parse a stored value (`RefusalCategory.values` membership).
    pub fn from_value(value: &str) -> Option<Self> {
        match value {
            "cyber" => Some(RefusalCategory::Cyber),
            "bio" => Some(RefusalCategory::Bio),
            "reasoning_extraction" => Some(RefusalCategory::ReasoningExtraction),
            "unknown" => Some(RefusalCategory::Unknown),
            _ => None,
        }
    }
}

impl std::fmt::Display for RefusalCategory {
    /// `str(member)`: the value.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.value())
    }
}

/// Tool-approval decision state (`models.py:295-299`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalStatus {
    Pending,
    Accepted,
    Declined,
    Expired,
}

impl ApprovalStatus {
    /// The `TextChoices` value (`pending`, …).
    pub fn value(&self) -> &'static str {
        match self {
            ApprovalStatus::Pending => "pending",
            ApprovalStatus::Accepted => "accepted",
            ApprovalStatus::Declined => "declined",
            ApprovalStatus::Expired => "expired",
        }
    }

    /// The human label (`models.py:296-299`).
    pub fn label(&self) -> &'static str {
        match self {
            ApprovalStatus::Pending => "Pending",
            ApprovalStatus::Accepted => "Accepted",
            ApprovalStatus::Declined => "Declined",
            ApprovalStatus::Expired => "Expired",
        }
    }

    /// Parse a stored value (`ApprovalStatus.values` membership).
    pub fn from_value(value: &str) -> Option<Self> {
        match value {
            "pending" => Some(ApprovalStatus::Pending),
            "accepted" => Some(ApprovalStatus::Accepted),
            "declined" => Some(ApprovalStatus::Declined),
            "expired" => Some(ApprovalStatus::Expired),
            _ => None,
        }
    }
}

impl std::fmt::Display for ApprovalStatus {
    /// `str(member)`: the value.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.value())
    }
}

/// What a tool approval gates (`models.py:302-306`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalKind {
    CommandExecution,
    FileChange,
    NetworkAccess,
    Other,
}

impl ApprovalKind {
    /// The `TextChoices` value (`command_execution`, …).
    pub fn value(&self) -> &'static str {
        match self {
            ApprovalKind::CommandExecution => "command_execution",
            ApprovalKind::FileChange => "file_change",
            ApprovalKind::NetworkAccess => "network_access",
            ApprovalKind::Other => "other",
        }
    }

    /// The human label (`models.py:303-306`).
    pub fn label(&self) -> &'static str {
        match self {
            ApprovalKind::CommandExecution => "Command Execution",
            ApprovalKind::FileChange => "File Change",
            ApprovalKind::NetworkAccess => "Network Access",
            ApprovalKind::Other => "Other",
        }
    }

    /// Parse a stored value (`ApprovalKind.values` membership).
    pub fn from_value(value: &str) -> Option<Self> {
        match value {
            "command_execution" => Some(ApprovalKind::CommandExecution),
            "file_change" => Some(ApprovalKind::FileChange),
            "network_access" => Some(ApprovalKind::NetworkAccess),
            "other" => Some(ApprovalKind::Other),
            _ => None,
        }
    }
}

impl std::fmt::Display for ApprovalKind {
    /// `str(member)`: the value.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.value())
    }
}

/// Direct-chat session state (`models.py:309-312`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentChatSessionStatus {
    Open,
    Closed,
    Failed,
}

impl AgentChatSessionStatus {
    /// The `TextChoices` value (`open`, …).
    pub fn value(&self) -> &'static str {
        match self {
            AgentChatSessionStatus::Open => "open",
            AgentChatSessionStatus::Closed => "closed",
            AgentChatSessionStatus::Failed => "failed",
        }
    }

    /// The human label (`models.py:310-312`).
    pub fn label(&self) -> &'static str {
        match self {
            AgentChatSessionStatus::Open => "Open",
            AgentChatSessionStatus::Closed => "Closed",
            AgentChatSessionStatus::Failed => "Failed",
        }
    }

    /// Parse a stored value (`AgentChatSessionStatus.values` membership).
    pub fn from_value(value: &str) -> Option<Self> {
        match value {
            "open" => Some(AgentChatSessionStatus::Open),
            "closed" => Some(AgentChatSessionStatus::Closed),
            "failed" => Some(AgentChatSessionStatus::Failed),
            _ => None,
        }
    }
}

impl std::fmt::Display for AgentChatSessionStatus {
    /// `str(member)`: the value.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.value())
    }
}

/// Direct-chat message author (`models.py:315-319`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentChatMessageRole {
    User,
    Assistant,
    Tool,
    System,
}

impl AgentChatMessageRole {
    /// The `TextChoices` value (`user`, …).
    pub fn value(&self) -> &'static str {
        match self {
            AgentChatMessageRole::User => "user",
            AgentChatMessageRole::Assistant => "assistant",
            AgentChatMessageRole::Tool => "tool",
            AgentChatMessageRole::System => "system",
        }
    }

    /// The human label (`models.py:316-319`).
    pub fn label(&self) -> &'static str {
        match self {
            AgentChatMessageRole::User => "User",
            AgentChatMessageRole::Assistant => "Assistant",
            AgentChatMessageRole::Tool => "Tool",
            AgentChatMessageRole::System => "System",
        }
    }

    /// Parse a stored value (`AgentChatMessageRole.values` membership).
    pub fn from_value(value: &str) -> Option<Self> {
        match value {
            "user" => Some(AgentChatMessageRole::User),
            "assistant" => Some(AgentChatMessageRole::Assistant),
            "tool" => Some(AgentChatMessageRole::Tool),
            "system" => Some(AgentChatMessageRole::System),
            _ => None,
        }
    }
}

impl std::fmt::Display for AgentChatMessageRole {
    /// `str(member)`: the value.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.value())
    }
}

/// Direct-chat message delivery state (`models.py:322-330`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentChatMessageStatus {
    Queued,
    Sent,
    Streaming,
    Completed,
    Failed,
    Cancelled,
}

impl AgentChatMessageStatus {
    /// The `TextChoices` value (`queued`, …).
    pub fn value(&self) -> &'static str {
        match self {
            AgentChatMessageStatus::Queued => "queued",
            AgentChatMessageStatus::Sent => "sent",
            AgentChatMessageStatus::Streaming => "streaming",
            AgentChatMessageStatus::Completed => "completed",
            AgentChatMessageStatus::Failed => "failed",
            AgentChatMessageStatus::Cancelled => "cancelled",
        }
    }

    /// The human label (`models.py:323-328`).
    pub fn label(&self) -> &'static str {
        match self {
            AgentChatMessageStatus::Queued => "Queued",
            AgentChatMessageStatus::Sent => "Sent",
            AgentChatMessageStatus::Streaming => "Streaming",
            AgentChatMessageStatus::Completed => "Completed",
            AgentChatMessageStatus::Failed => "Failed",
            AgentChatMessageStatus::Cancelled => "Cancelled",
        }
    }

    /// Parse a stored value (`AgentChatMessageStatus.values` membership).
    pub fn from_value(value: &str) -> Option<Self> {
        match value {
            "queued" => Some(AgentChatMessageStatus::Queued),
            "sent" => Some(AgentChatMessageStatus::Sent),
            "streaming" => Some(AgentChatMessageStatus::Streaming),
            "completed" => Some(AgentChatMessageStatus::Completed),
            "failed" => Some(AgentChatMessageStatus::Failed),
            "cancelled" => Some(AgentChatMessageStatus::Cancelled),
            _ => None,
        }
    }
}

impl std::fmt::Display for AgentChatMessageStatus {
    /// `str(member)`: the value.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.value())
    }
}

/// Cloud tool-call ledger state (`models.py:1178-1186`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolCallStatus {
    Prepared,
    Submitted,
    Succeeded,
    Failed,
    Unknown,
    Denied,
}

impl ToolCallStatus {
    /// The `TextChoices` value (`prepared`, …).
    pub fn value(&self) -> &'static str {
        match self {
            ToolCallStatus::Prepared => "prepared",
            ToolCallStatus::Submitted => "submitted",
            ToolCallStatus::Succeeded => "succeeded",
            ToolCallStatus::Failed => "failed",
            ToolCallStatus::Unknown => "unknown",
            ToolCallStatus::Denied => "denied",
        }
    }

    /// The human label (`models.py:1179-1184`).
    pub fn label(&self) -> &'static str {
        match self {
            ToolCallStatus::Prepared => "Prepared",
            ToolCallStatus::Submitted => "Submitted",
            ToolCallStatus::Succeeded => "Succeeded",
            ToolCallStatus::Failed => "Failed",
            ToolCallStatus::Unknown => "Outcome unknown",
            ToolCallStatus::Denied => "Denied",
        }
    }

    /// Parse a stored value (`ToolCallStatus.values` membership).
    pub fn from_value(value: &str) -> Option<Self> {
        match value {
            "prepared" => Some(ToolCallStatus::Prepared),
            "submitted" => Some(ToolCallStatus::Submitted),
            "succeeded" => Some(ToolCallStatus::Succeeded),
            "failed" => Some(ToolCallStatus::Failed),
            "unknown" => Some(ToolCallStatus::Unknown),
            "denied" => Some(ToolCallStatus::Denied),
            _ => None,
        }
    }
}

impl std::fmt::Display for ToolCallStatus {
    /// `str(member)`: the value.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.value())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    static FIXTURE: &str =
        include_str!("../../../../fixtures/runner_runs/fx-run-01-types-pure.golden.json");

    fn fixture() -> Value {
        serde_json::from_str(FIXTURE).expect("fixture parses")
    }

    fn members(name: &Value, key: &str) -> Vec<Value> {
        name.get("enums")
            .and_then(|e| e.get(key))
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default()
    }

    macro_rules! check_enum {
        ($ty:ty, $key:literal, [$($variant:expr),+ $(,)?]) => {{
            let fx = fixture();
            let expected = members(&fx, $key);
            let variants: &[$ty] = &[$($variant),+];
            assert_eq!(expected.len(), variants.len(), concat!($key, " member count"));
            for (entry, variant) in expected.iter().zip(variants.iter()) {
                let value = entry.get("value").and_then(Value::as_str).unwrap_or_default();
                let label = entry.get("label").and_then(Value::as_str).unwrap_or_default();
                assert_eq!(variant.value(), value, concat!($key, " value"));
                assert_eq!(variant.label(), label, concat!($key, " label"));
                assert_eq!(variant.to_string(), value, concat!($key, " Display"));
                assert_eq!(<$ty>::from_value(value), Some(*variant), concat!($key, " from_value"));
                assert_eq!(
                    serde_json::to_value(variant).expect("serializes"),
                    Value::String(value.to_string()),
                    concat!($key, " serde value"),
                );
                assert_eq!(
                    serde_json::from_value::<$ty>(Value::String(value.to_string())).expect("parses"),
                    *variant,
                );
            }
            assert_eq!(<$ty>::from_value("no_such_value"), None, concat!($key, " unknown rejected"));
            assert!(
                serde_json::from_value::<$ty>(Value::String("no_such_value".to_string())).is_err(),
                concat!($key, " serde unknown rejected"),
            );
        }};
    }

    #[test]
    fn agent_run_status_matches_fixture_verbatim_in_order() {
        check_enum!(
            AgentRunStatus,
            "AgentRunStatus",
            [
                AgentRunStatus::Queued,
                AgentRunStatus::Assigned,
                AgentRunStatus::WaitingForWorktree,
                AgentRunStatus::Running,
                AgentRunStatus::CancelRequested,
                AgentRunStatus::AwaitingApproval,
                AgentRunStatus::AwaitingReauth,
                AgentRunStatus::PausedAwaitingInput,
                AgentRunStatus::Blocked,
                AgentRunStatus::Completed,
                AgentRunStatus::Failed,
                AgentRunStatus::Cancelled,
                AgentRunStatus::Refused,
            ]
        );
    }

    #[test]
    fn agent_run_trigger_matches_fixture_verbatim_in_order() {
        check_enum!(
            AgentRunTrigger,
            "AgentRunTrigger",
            [
                AgentRunTrigger::StateTransition,
                AgentRunTrigger::RunAi,
                AgentRunTrigger::CommentAndRun,
                AgentRunTrigger::Tick,
                AgentRunTrigger::Scheduler,
                AgentRunTrigger::Direct,
            ]
        );
    }

    #[test]
    fn refusal_category_matches_fixture_verbatim_in_order() {
        check_enum!(
            RefusalCategory,
            "RefusalCategory",
            [
                RefusalCategory::Cyber,
                RefusalCategory::Bio,
                RefusalCategory::ReasoningExtraction,
                RefusalCategory::Unknown,
            ]
        );
    }

    #[test]
    fn approval_enums_match_fixture_verbatim_in_order() {
        check_enum!(
            ApprovalStatus,
            "ApprovalStatus",
            [
                ApprovalStatus::Pending,
                ApprovalStatus::Accepted,
                ApprovalStatus::Declined,
                ApprovalStatus::Expired,
            ]
        );
        check_enum!(
            ApprovalKind,
            "ApprovalKind",
            [
                ApprovalKind::CommandExecution,
                ApprovalKind::FileChange,
                ApprovalKind::NetworkAccess,
                ApprovalKind::Other,
            ]
        );
    }

    #[test]
    fn chat_enums_match_fixture_verbatim_in_order() {
        check_enum!(
            AgentChatSessionStatus,
            "AgentChatSessionStatus",
            [
                AgentChatSessionStatus::Open,
                AgentChatSessionStatus::Closed,
                AgentChatSessionStatus::Failed,
            ]
        );
        check_enum!(
            AgentChatMessageRole,
            "AgentChatMessageRole",
            [
                AgentChatMessageRole::User,
                AgentChatMessageRole::Assistant,
                AgentChatMessageRole::Tool,
                AgentChatMessageRole::System,
            ]
        );
        check_enum!(
            AgentChatMessageStatus,
            "AgentChatMessageStatus",
            [
                AgentChatMessageStatus::Queued,
                AgentChatMessageStatus::Sent,
                AgentChatMessageStatus::Streaming,
                AgentChatMessageStatus::Completed,
                AgentChatMessageStatus::Failed,
                AgentChatMessageStatus::Cancelled,
            ]
        );
    }

    #[test]
    fn tool_call_status_matches_fixture_verbatim_in_order() {
        check_enum!(
            ToolCallStatus,
            "ToolCallStatus",
            [
                ToolCallStatus::Prepared,
                ToolCallStatus::Submitted,
                ToolCallStatus::Succeeded,
                ToolCallStatus::Failed,
                ToolCallStatus::Unknown,
                ToolCallStatus::Denied,
            ]
        );
    }

    #[test]
    fn terminal_set_matches_lifecycle_tuple_order() {
        let values: Vec<&str> = TERMINAL_RUN_STATUSES.iter().map(|s| s.value()).collect();
        assert_eq!(
            values,
            ["completed", "failed", "cancelled", "blocked", "refused"]
        );
    }

    #[test]
    fn terminal_and_active_partition_all_statuses() {
        // is_terminal: COMPLETED/FAILED/CANCELLED/BLOCKED/REFUSED only.
        // is_active: QUEUED..AWAITING_REAUTH (7). PAUSED_AWAITING_INPUT is
        // neither (non-terminal but the runner is free for other work).
        let cases = [
            (AgentRunStatus::Queued, false, true),
            (AgentRunStatus::Assigned, false, true),
            (AgentRunStatus::WaitingForWorktree, false, true),
            (AgentRunStatus::Running, false, true),
            (AgentRunStatus::CancelRequested, false, true),
            (AgentRunStatus::AwaitingApproval, false, true),
            (AgentRunStatus::AwaitingReauth, false, true),
            (AgentRunStatus::PausedAwaitingInput, false, false),
            (AgentRunStatus::Blocked, true, false),
            (AgentRunStatus::Completed, true, false),
            (AgentRunStatus::Failed, true, false),
            (AgentRunStatus::Cancelled, true, false),
            (AgentRunStatus::Refused, true, false),
        ];
        for (status, terminal, active) in cases {
            assert_eq!(status.is_terminal(), terminal, "{status} is_terminal");
            assert_eq!(status.is_active(), active, "{status} is_active");
        }
    }

    #[test]
    fn trigger_sets_match_models() {
        let human: Vec<&str> = HUMAN_TRIGGERS.iter().map(|t| t.value()).collect();
        assert_eq!(
            human,
            ["state_transition", "run_ai", "comment_and_run", "direct"]
        );
        let automatic: Vec<&str> = AUTOMATIC_ISSUE_TRIGGERS.iter().map(|t| t.value()).collect();
        assert_eq!(automatic, ["tick"]);
        let cases = [
            (AgentRunTrigger::StateTransition, true),
            (AgentRunTrigger::RunAi, true),
            (AgentRunTrigger::CommentAndRun, true),
            (AgentRunTrigger::Tick, false),
            (AgentRunTrigger::Scheduler, false),
            (AgentRunTrigger::Direct, true),
        ];
        for (trigger, expected) in cases {
            assert_eq!(run_is_human_triggered(trigger), expected, "{trigger}");
        }
    }
}
