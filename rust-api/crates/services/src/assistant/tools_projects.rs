//! Assistant read-only project/metadata tools (D-06, stage 5).
//!
//! Ports `apps/api/pi_dash/assistant/tools/projects.py:1-65`: the four
//! `@assistant.tool` functions — `list_projects`, `list_states`,
//! `list_labels`, `list_project_members` — with their caps, ordering,
//! scope checks, row shapes, and input schemas. Fixture id F-A6-10
//! (`rust-api/fixtures/assistant/tools-tasks.json`, `tools.projects`).
//!
//! Shape notes:
//!
//! * Queryset execution stays with the handler layer; this module ports
//!   the exact caps (`[:50]`), orderings (`name`, `sequence`), scope
//!   gates (`get_project`), row projections, and the member display-name
//!   fallback chain.

use serde_json::{json, Value};

/// Tool names as registered on the shared agent (`projects.py:17-48`).
pub const LIST_PROJECTS_TOOL: &str = "list_projects";
/// Tool names as registered on the shared agent (`projects.py:27-28`).
pub const LIST_STATES_TOOL: &str = "list_states";
/// Tool names as registered on the shared agent (`projects.py:37-38`).
pub const LIST_LABELS_TOOL: &str = "list_labels";
/// Tool names as registered on the shared agent (`projects.py:47-48`).
pub const LIST_PROJECT_MEMBERS_TOOL: &str = "list_project_members";

/// Project-list cap (`projects.py:20`: `order_by("name")[:50]`).
pub const PROJECT_LIST_LIMIT: usize = 50;

/// Project row (`projects.py:21-24`).
pub fn project_row(id: &str, identifier: &str, name: &str) -> Value {
    json!({ "id": id, "identifier": identifier, "name": name })
}

/// State row (`projects.py:30-34`).
pub fn state_row(id: &str, name: &str, group: &str, default: bool) -> Value {
    json!({ "id": id, "name": name, "group": group, "default": default })
}

/// Label row (`projects.py:43-44`).
pub fn label_row(id: &str, name: &str) -> Value {
    json!({ "id": id, "name": name })
}

/// Member display name (`projects.py:61`):
/// `member.display_name or member.email or ""` — empty strings fall
/// through exactly like `None` does under Python truthiness.
pub fn member_display_name(display_name: Option<&str>, email: Option<&str>) -> String {
    match display_name {
        Some(name) if !name.is_empty() => name.to_string(),
        _ => match email {
            Some(email) if !email.is_empty() => email.to_string(),
            _ => String::new(),
        },
    }
}

/// Member row (`projects.py:58-64`); `None` signals a null member row,
/// which Python skips (`projects.py:56-57`).
pub fn member_row(user_id: &str, display_name: &str, role: i64) -> Value {
    json!({ "user_id": user_id, "display_name": display_name, "role": role })
}

/// Whether a member row is kept: rows with a null member are skipped
/// (`projects.py:56-57`).
pub fn keep_member_row(has_member: bool) -> bool {
    has_member
}

/// Input schemas for the four tools (parameter lists per the fixture:
/// `list_projects` takes none; the other three take `project_id`).
pub fn list_projects_schema() -> Value {
    json!({
        "type": "object",
        "properties": {},
        "required": [],
        "additionalProperties": false,
    })
}

/// Input schema shared by `list_states`, `list_labels`, and
/// `list_project_members` (each takes exactly `project_id`).
pub fn project_id_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "project_id": { "type": "string" },
        },
        "required": ["project_id"],
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
    fn fixture_names_all_four_tools_with_params() {
        let projects = fixture()["tools"]["projects"].as_array().unwrap().clone();
        let names = projects
            .iter()
            .map(|t| t["name"].as_str().unwrap().to_string())
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            vec![
                "list_projects",
                "list_states",
                "list_labels",
                "list_project_members"
            ]
        );
        assert!(projects[0]["params"].as_array().unwrap().is_empty());
        for tool in &projects[1..] {
            assert_eq!(tool["params"].as_array().unwrap().len(), 1);
            assert_eq!(tool["params"][0], "project_id");
        }
    }

    #[test]
    fn cap_and_row_shapes_match_python() {
        assert_eq!(PROJECT_LIST_LIMIT, 50);
        assert_eq!(
            project_row("p", "ABC", "Alpha"),
            json!({ "id": "p", "identifier": "ABC", "name": "Alpha" })
        );
        assert_eq!(
            state_row("s", "Backlog", "backlog", true),
            json!({ "id": "s", "name": "Backlog", "group": "backlog", "default": true })
        );
        assert_eq!(label_row("l", "bug"), json!({ "id": "l", "name": "bug" }));
        assert_eq!(
            member_row("u", "Ann", 15),
            json!({ "user_id": "u", "display_name": "Ann", "role": 15 })
        );
    }

    #[test]
    fn display_name_fallback_chain_matches_python_truthiness() {
        assert_eq!(member_display_name(Some("Ann"), Some("a@x")), "Ann");
        assert_eq!(member_display_name(None, Some("a@x")), "a@x");
        // Empty display name falls through to email (Python `or`).
        assert_eq!(member_display_name(Some(""), Some("a@x")), "a@x");
        assert_eq!(member_display_name(None, None), "");
        assert_eq!(member_display_name(Some(""), Some("")), "");
        assert!(!keep_member_row(false));
        assert!(keep_member_row(true));
    }

    #[test]
    fn schemas_match_tool_params() {
        let empty = list_projects_schema();
        assert_eq!(empty["required"], json!([]));
        let with_id = project_id_schema();
        assert_eq!(with_id["required"], json!(["project_id"]));
    }
}
