//! Assistant comment tool (D-06, stage 5).
//!
//! Ports `apps/api/pi_dash/assistant/tools/comments.py:1-77`: the
//! `create_comment` tool — its guest gate, empty-body retry, comment-row
//! shape, activity summary, result shape, and input schema. Fixture id
//! F-A6-10 (`rust-api/fixtures/assistant/tools-tasks.json`, `tools.comments`).
//!
//! Shape notes:
//!
//! * ORM execution (`IssueComment.objects.create` under `impersonate`,
//!   `comments.py:51-65`) and the `record_write` persistence stay with the
//!   handler layer. This module ports everything around them byte for
//!   byte: the guest rule (`comments.py:39-46`), the empty-body retry
//!   (`comments.py:48-49`), the row the handler must write, the summary
//!   string (`comments.py:67-71`), and the returned dict
//!   (`comments.py:72-77`).

use serde_json::{json, Value};

use super::tools_scoping::{ToolScopeError, ROLE_GUEST};

/// Tool name as registered on the shared agent (`comments.py:21-22`).
pub const CREATE_COMMENT_TOOL: &str = "create_comment";

/// Guest denial when commenting outside the guest rule
/// (`comments.py:44-46`).
pub const GUEST_COMMENT_DENIED: &str = "Guests can only comment on issues they created.";

/// Empty-body retry (`comments.py:48-49`, a `ModelRetry`).
pub const EMPTY_BODY_MESSAGE: &str = "Comment body cannot be empty.";

/// Speaker marker written on agent-created comments (`comments.py:63-64`).
pub const SPEAKER_TYPE: &str = "agent";
/// Speaker label written on agent-created comments (`comments.py:63-64`).
pub const SPEAKER_LABEL: &str = "Pi Dash AI";

/// Guest comment gate (`comments.py:39-46`, mirroring
/// `app/views/issue/comment.py`): a guest (`workspace_role <= ROLE_GUEST`)
/// may comment only when the project enables `guest_view_all_features` or
/// they created the issue.
pub fn guest_may_comment(
    workspace_role: i64,
    guest_view_all_features: bool,
    created_by_id: &str,
    user_id: &str,
) -> bool {
    if workspace_role <= ROLE_GUEST && !guest_view_all_features && created_by_id != user_id {
        return false;
    }
    true
}

/// Guest-gate denial as the Python `raise` produces it.
pub fn guest_denial() -> ToolScopeError {
    ToolScopeError::Permission(GUEST_COMMENT_DENIED.to_string())
}

/// Empty-body check (`comments.py:48-49`): `None`, `""`, and
/// whitespace-only bodies are retried with the exact message.
pub fn validate_body(body_md: Option<&str>) -> Result<&str, ToolScopeError> {
    match body_md {
        Some(body) if !body.trim().is_empty() => Ok(body),
        _ => Err(ToolScopeError::Permission(EMPTY_BODY_MESSAGE.to_string())),
    }
}

/// Labels written on the comment row (`comments.py:62`): `["fold"]` for
/// low-value status/noop updates, otherwise `[]`.
pub fn comment_labels(fold: bool) -> Vec<String> {
    if fold {
        vec!["fold".to_string()]
    } else {
        vec![]
    }
}

/// Activity summary (`comments.py:67-71`).
pub fn comment_summary(project_identifier: &str, sequence_id: i64) -> String {
    format!("Commented on issue {project_identifier}-{sequence_id}")
}

/// Tool result (`comments.py:72-77`).
pub fn comment_result(comment_id: &str, issue_id: &str, fold: bool) -> Value {
    json!({
        "created": true,
        "comment_id": comment_id,
        "issue_id": issue_id,
        "folded": fold,
    })
}

/// Input schema for `create_comment` (parameters `issue_id`, `body_md`,
/// `fold=False` per the fixture; docstring documents the `fold` flag).
pub fn create_comment_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "issue_id": { "type": "string" },
            "body_md": { "type": "string" },
            "fold": { "type": "boolean", "default": false },
        },
        "required": ["issue_id", "body_md"],
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
    fn fixture_names_this_tool_with_its_params() {
        let comments = fixture()["tools"]["comments"].as_array().unwrap().clone();
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0]["name"], "create_comment");
        let params = comments[0]["params"].as_array().unwrap();
        assert_eq!(params[0], "issue_id");
        assert_eq!(params[1], "body_md");
        assert_eq!(params[2], "fold=False");
    }

    #[test]
    fn guest_gate_matches_comment_endpoint() {
        // Guest, no flag, not the creator: denied.
        assert!(!guest_may_comment(5, false, "other", "me"));
        // Guest but created the issue: allowed.
        assert!(guest_may_comment(5, false, "me", "me"));
        // Guest with the project flag: allowed.
        assert!(guest_may_comment(5, true, "other", "me"));
        // Member and above: allowed regardless.
        assert!(guest_may_comment(15, false, "other", "me"));
        assert!(guest_may_comment(20, false, "other", "me"));
        assert_eq!(
            guest_denial().to_string(),
            "Guests can only comment on issues they created."
        );
    }

    #[test]
    fn empty_body_is_retried() {
        assert!(validate_body(Some("hello")).is_ok());
        for bad in [None, Some(""), Some("   "), Some("\n\t ")] {
            assert_eq!(
                validate_body(bad).unwrap_err().to_string(),
                "Comment body cannot be empty."
            );
        }
    }

    #[test]
    fn row_shapes_and_result_match_python() {
        assert_eq!(comment_labels(true), vec!["fold".to_string()]);
        assert!(comment_labels(false).is_empty());
        assert_eq!(SPEAKER_TYPE, "agent");
        assert_eq!(SPEAKER_LABEL, "Pi Dash AI");
        assert_eq!(comment_summary("ABC", 42), "Commented on issue ABC-42");
        assert_eq!(
            comment_result("c1", "i1", true),
            json!({
                "created": true,
                "comment_id": "c1",
                "issue_id": "i1",
                "folded": true,
            })
        );
        let schema = create_comment_schema();
        assert_eq!(schema["required"], json!(["issue_id", "body_md"]));
        assert_eq!(schema["properties"]["fold"]["default"], json!(false));
    }
}
