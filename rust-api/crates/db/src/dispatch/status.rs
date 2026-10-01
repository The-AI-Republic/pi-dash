#![forbid(unsafe_code)]

//! Agent run status + trigger enums (D-11, stage 5).
//!
//! Port of the `TextChoices` behind the `AgentRun.status`
//! (`models.py:947-952`) and `AgentRun.trigger` (`models.py:972-980`)
//! columns:
//!
//! * `AgentRunStatus` (`models.py:207-235`) → [`AgentRunStatus`].
//! * `AgentRunTrigger` (`models.py:237-250`) → [`AgentRunTrigger`].
//!
//! Values are verbatim, in declaration order. `Display` renders the
//! value, as `str(member)` does in Django; serde reads/writes the
//! value and unknown values are rejected, as Django choice validation
//! would (same enum API as the L1 [`pidash_types::dispatch::AgentExecutorKind`]
//! port, PIDASHCONV-482).
//!
//! Out of scope here: `is_active` / `is_terminal`
//! (`models.py:1137-1160`) are model properties, not columns or
//! choices, and no FX-DISP-02 vector covers them, so they are left
//! for the layer that needs them; `HUMAN_TRIGGERS` (`models.py:259`)
//! is a prompt-composer concern, not dispatch.
//!
//! Fixture: `rust-api/fixtures/dispatch/fx-disp-02-models.golden.json`
//! (`agent_run_status_values`, `agent_run_trigger_values`).
//!
//! Ported bugs: none found in these units on read-through.

use serde::{Deserialize, Serialize};

/// Run lifecycle states (`models.py:207-235`).
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

    /// The human label (`models.py:208-235`).
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
}

impl std::fmt::Display for AgentRunStatus {
    /// `str(member)`: the value.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.value())
    }
}

/// How a run came to be created (`models.py:237-250`).
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

    /// The human label (`models.py:244-249`).
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    static FIXTURE: &str =
        include_str!("../../../../fixtures/dispatch/fx-disp-02-models.golden.json");

    fn fixture() -> Value {
        serde_json::from_str(FIXTURE).expect("fixture parses")
    }

    #[test]
    fn status_values_match_fixture_verbatim_in_order() {
        let statuses = [
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
        ];
        let values: Vec<&str> = statuses.iter().map(AgentRunStatus::value).collect();
        assert_eq!(
            serde_json::json!(values),
            fixture()["agent_run_status_values"]
        );
        // Labels come from the source (`models.py:208-235`); the
        // fixture records values only, so pin the full label set here.
        let labels: Vec<&str> = statuses.iter().map(AgentRunStatus::label).collect();
        assert_eq!(
            labels,
            [
                "Queued",
                "Assigned",
                "Waiting for Worktree",
                "Running",
                "Cancellation Requested",
                "Awaiting Approval",
                "Awaiting Reauth",
                "Paused — Awaiting Input",
                "Blocked",
                "Completed",
                "Failed",
                "Cancelled",
                "Refused",
            ]
        );
    }

    #[test]
    fn trigger_values_match_fixture_verbatim_in_order() {
        let triggers = [
            AgentRunTrigger::StateTransition,
            AgentRunTrigger::RunAi,
            AgentRunTrigger::CommentAndRun,
            AgentRunTrigger::Tick,
            AgentRunTrigger::Scheduler,
            AgentRunTrigger::Direct,
        ];
        let values: Vec<&str> = triggers.iter().map(AgentRunTrigger::value).collect();
        assert_eq!(
            serde_json::json!(values),
            fixture()["agent_run_trigger_values"]
        );
        let labels: Vec<&str> = triggers.iter().map(AgentRunTrigger::label).collect();
        assert_eq!(
            labels,
            [
                "State transition",
                "Run AI button",
                "Comment & Run",
                "Automatic tick",
                "Scheduler beat",
                "Direct",
            ]
        );
    }

    #[test]
    fn status_serde_round_trips_values_and_rejects_unknown() {
        for status in [
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
        ] {
            let rendered = serde_json::to_string(&status).expect("serializes");
            assert_eq!(rendered, format!("\"{}\"", status.value()));
            let back: AgentRunStatus = serde_json::from_str(&rendered).expect("parses");
            assert_eq!(back, status);
            assert_eq!(AgentRunStatus::from_value(status.value()), Some(status));
        }
        assert_eq!(AgentRunStatus::from_value("bogus"), None);
        assert!(serde_json::from_str::<AgentRunStatus>("\"bogus\"").is_err());
        assert_eq!(AgentRunStatus::Queued.to_string(), "queued");
    }

    #[test]
    fn trigger_serde_round_trips_values_and_rejects_unknown() {
        for trigger in [
            AgentRunTrigger::StateTransition,
            AgentRunTrigger::RunAi,
            AgentRunTrigger::CommentAndRun,
            AgentRunTrigger::Tick,
            AgentRunTrigger::Scheduler,
            AgentRunTrigger::Direct,
        ] {
            let rendered = serde_json::to_string(&trigger).expect("serializes");
            assert_eq!(rendered, format!("\"{}\"", trigger.value()));
            let back: AgentRunTrigger = serde_json::from_str(&rendered).expect("parses");
            assert_eq!(back, trigger);
            assert_eq!(AgentRunTrigger::from_value(trigger.value()), Some(trigger));
        }
        assert_eq!(AgentRunTrigger::from_value("bogus"), None);
        assert!(serde_json::from_str::<AgentRunTrigger>("\"bogus\"").is_err());
        assert_eq!(AgentRunTrigger::Direct.to_string(), "direct");
    }
}
