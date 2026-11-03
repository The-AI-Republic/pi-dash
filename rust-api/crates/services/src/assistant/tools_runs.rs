//! Assistant coding-run tools (D-06, stage 5).
//!
//! Ports `apps/api/pi_dash/assistant/tools/runs.py:1-96`: `get_run_status`
//! and `dispatch_coding_run` — the run-row shape, the recent-five cap, the
//! write gate, the ticking-state decision tree, and the input schemas.
//! Fixture id F-A6-10 (`rust-api/fixtures/assistant/tools-tasks.json`,
//! `tools.runs`).
//!
//! Shape notes:
//!
//! * ORM reads (`AgentRun` query, `runs.py:24`), the state lookup, and the
//!   `handle_issue_state_transition` side effect stay with the handler
//!   layer. This module ports everything around them byte for byte: the
//!   `[:5]` cap with `-created_at` ordering, the row projection (with the
//!   `None` `created_at` passthrough), the `PHASES` ticking table from
//!   `orchestration/agent_phases.py:117-147`, the exact retry messages, the
//!   no-delegation/already-in-state payloads, and the success payload keys.
//! * `is_ticking_state` (`agent_phases.py:149-162`) is `True` when the
//!   state's group is registered in `PHASES` **and** its name equals the
//!   registered state name — group alone is not enough.

use serde_json::{json, Value};

use super::tools_scoping::ToolScopeError;

/// Tool names as registered on the shared agent (`runs.py:18,38`).
pub const GET_RUN_STATUS_TOOL: &str = "get_run_status";
/// Tool names as registered on the shared agent (`runs.py:37-38`).
pub const DISPATCH_CODING_RUN_TOOL: &str = "dispatch_coding_run";

/// Recent-run cap (`runs.py:24`: `order_by("-created_at")[:5]`).
pub const RUN_HISTORY_LIMIT: usize = 5;

/// Retry when the requested state id is not in the project
/// (`runs.py:53-56`).
pub const INVALID_STATE_MESSAGE: &str = "That state is not valid for this project.";

/// Retry when the requested state does not start a run (`runs.py:57-61`).
pub const NON_TICKING_STATE_MESSAGE: &str =
    "That state does not start a coding run. Pick a delegated state such as 'In Progress' or 'In Review'.";

/// Payload when the project has no delegated state (`runs.py:66-71`).
pub const NO_DELEGATION_STATE_ERROR: &str = "no_delegation_state";
/// Payload detail when the project has no delegated state (`runs.py:70`).
pub const NO_DELEGATION_STATE_DETAIL: &str =
    "This project has no state that triggers a coding run.";

/// Payload error when the issue is already in the target state
/// (`runs.py:74-79`).
pub const ALREADY_IN_STATE_ERROR: &str = "already_in_state";

/// A `PHASES` entry (`agent_phases.py:107-147`): the ticking state name
/// for one state group. Only `state_name` drives `is_ticking_state`; the
/// rest is recorded so the table matches Python row for row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhaseConfig {
    /// State group value (`StateGroup.*.value`).
    pub group: &'static str,
    /// The registered ticking state name for the group.
    pub state_name: &'static str,
    /// Run template fired on entry (`template_name`).
    pub template_name: &'static str,
}

/// Delegated (ticking) phases (`PHASES`, `agent_phases.py:117-147`).
pub const PHASES: [PhaseConfig; 3] = [
    PhaseConfig {
        group: "started",
        state_name: "In Progress",
        template_name: "coding-task",
    },
    PhaseConfig {
        group: "review",
        state_name: "In Review",
        template_name: "review",
    },
    PhaseConfig {
        group: "test",
        state_name: "In Test",
        template_name: "test",
    },
];

/// Ticking-state check (`is_ticking_state`, `agent_phases.py:149-162`).
pub fn is_ticking_state(group: Option<&str>, name: Option<&str>) -> bool {
    match (group, name) {
        (Some(group), Some(name)) => PHASES
            .iter()
            .any(|phase| phase.group == group && phase.state_name == name),
        _ => false,
    }
}

/// Run-status row (`runs.py:27-33`); `created_at` passes `None` through
/// instead of calling `isoformat` on it.
pub fn run_row(id: &str, status: &str, created_at_iso: Option<&str>) -> Value {
    json!({ "id": id, "status": status, "created_at": created_at_iso })
}

/// `get_run_status` result (`runs.py:34`).
pub fn run_status_result(issue_id: &str, runs: Vec<Value>) -> Value {
    json!({ "issue_id": issue_id, "runs": runs })
}

/// No-delegation payload (`runs.py:67-71`).
pub fn no_delegation_state_result() -> Value {
    json!({
        "dispatched": false,
        "error": NO_DELEGATION_STATE_ERROR,
        "detail": NO_DELEGATION_STATE_DETAIL,
    })
}

/// Already-in-state payload (`runs.py:75-79`).
pub fn already_in_state_result(target_name: &str) -> Value {
    json!({
        "dispatched": false,
        "error": ALREADY_IN_STATE_ERROR,
        "detail": format!("Issue is already in '{target_name}'."),
    })
}

/// Success payload (`runs.py:91-96`): `dispatched` is whether the
/// transition created a run; `run_id` is `None` otherwise.
pub fn dispatch_success_result(run_id: Option<&str>, new_state: &str, reason: &str) -> Value {
    json!({
        "dispatched": run_id.is_some(),
        "run_id": run_id,
        "new_state": new_state,
        "reason": reason,
    })
}

/// Retry errors `dispatch_coding_run` raises as `ModelRetry`.
pub fn invalid_state_error() -> ToolScopeError {
    ToolScopeError::Permission(INVALID_STATE_MESSAGE.to_string())
}

/// Retry errors `dispatch_coding_run` raises as `ModelRetry`.
pub fn non_ticking_state_error() -> ToolScopeError {
    ToolScopeError::Permission(NON_TICKING_STATE_MESSAGE.to_string())
}

/// Activity summary for a dispatched run (`runs.py:85-90`).
pub fn dispatch_summary(project_identifier: &str, sequence_id: i64, target_name: &str) -> String {
    format!("Started a coding run on {project_identifier}-{sequence_id} (moved to '{target_name}')")
}

/// Input schema for `get_run_status` (parameter `issue_id`).
pub fn get_run_status_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "issue_id": { "type": "string" },
        },
        "required": ["issue_id"],
        "additionalProperties": false,
    })
}

/// Input schema for `dispatch_coding_run` (parameters `issue_id`,
/// optional `target_state_id`).
pub fn dispatch_coding_run_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "issue_id": { "type": "string" },
            "target_state_id": { "type": ["string", "null"] },
        },
        "required": ["issue_id"],
        "additionalProperties": false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn fixture() -> Value {
        let path = format!(
            "{}/../../fixtures/assistant/tools-tasks.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(&path).expect("fixture exists"))
            .expect("fixture parses")
    }

    #[test]
    fn fixture_names_both_run_tools_with_params() {
        let runs = fixture()["tools"]["runs"].as_array().unwrap().clone();
        assert_eq!(runs[0]["name"], "get_run_status");
        assert_eq!(runs[0]["params"], json!(["issue_id"]));
        assert_eq!(runs[1]["name"], "dispatch_coding_run");
        assert_eq!(runs[1]["params"], json!(["issue_id", "target_state_id?"]));
        assert!(runs[1]["shape"].as_str().unwrap().contains("ticking"));
    }

    #[test]
    fn ticking_table_matches_phases() {
        assert_eq!(PHASES.len(), 3);
        // Group alone is not enough: the name must equal the registered one.
        assert!(is_ticking_state(Some("started"), Some("In Progress")));
        assert!(is_ticking_state(Some("review"), Some("In Review")));
        assert!(is_ticking_state(Some("test"), Some("In Test")));
        assert!(!is_ticking_state(Some("started"), Some("In Review")));
        assert!(!is_ticking_state(Some("backlog"), Some("In Progress")));
        assert!(!is_ticking_state(Some("completed"), Some("Done")));
        assert!(!is_ticking_state(None, Some("In Progress")));
        assert!(!is_ticking_state(Some("started"), None));
        assert!(!is_ticking_state(None, None));
    }

    #[test]
    fn run_row_and_status_shapes_match_python() {
        assert_eq!(RUN_HISTORY_LIMIT, 5);
        assert_eq!(
            run_row("r1", "running", Some("2026-01-01T00:00:00+00:00")),
            json!({
                "id": "r1",
                "status": "running",
                "created_at": "2026-01-01T00:00:00+00:00",
            })
        );
        // Missing timestamp passes None through.
        assert_eq!(run_row("r1", "queued", None)["created_at"], Value::Null);
        assert_eq!(
            run_status_result("i1", vec![]),
            json!({ "issue_id": "i1", "runs": [] })
        );
    }

    #[test]
    fn dispatch_messages_and_payloads_match_python() {
        assert_eq!(
            invalid_state_error().to_string(),
            "That state is not valid for this project."
        );
        assert_eq!(
            non_ticking_state_error().to_string(),
            "That state does not start a coding run. Pick a delegated state such as 'In Progress' or 'In Review'."
        );
        assert_eq!(
            no_delegation_state_result(),
            json!({
                "dispatched": false,
                "error": "no_delegation_state",
                "detail": "This project has no state that triggers a coding run.",
            })
        );
        assert_eq!(
            already_in_state_result("In Progress"),
            json!({
                "dispatched": false,
                "error": "already_in_state",
                "detail": "Issue is already in 'In Progress'.",
            })
        );
        assert_eq!(
            dispatch_success_result(Some("run-1"), "In Progress", "entered bucket"),
            json!({
                "dispatched": true,
                "run_id": "run-1",
                "new_state": "In Progress",
                "reason": "entered bucket",
            })
        );
        // No run created: dispatched false, run_id null.
        let no_run = dispatch_success_result(None, "In Review", "queued");
        assert_eq!(no_run["dispatched"], json!(false));
        assert_eq!(no_run["run_id"], Value::Null);
        assert_eq!(
            dispatch_summary("ABC", 7, "In Progress"),
            "Started a coding run on ABC-7 (moved to 'In Progress')"
        );
    }

    #[test]
    fn schemas_match_tool_params() {
        assert_eq!(get_run_status_schema()["required"], json!(["issue_id"]));
        let schema = dispatch_coding_run_schema();
        assert_eq!(schema["required"], json!(["issue_id"]));
        assert!(schema["properties"]["target_state_id"]["type"]
            .as_array()
            .unwrap()
            .contains(&json!("null")));
    }
}
