//! Local tool schemas: the exact `ToolDefinition`s pydantic-ai 1.107
//! generates from `assistant/tools/*.py` (PIDASHCONV-1113).
//!
//! Each entry is the triple the model sees — `name`, `description`
//! (docstring via `cleandoc`), `parameters` — captured live from the
//! pinned pydantic-ai (fixture `assistant/tools-local-schemas.json`)
//! and transcribed with identical key order. Order of the list is
//! registration order (`tools/__init__.py:11` imports `comments`,
//! `github`, `issues`, `projects`, `runs`).
//!
//! Provider envelopes (OpenAI function wrapper, Anthropic `input_schema`,
//! strict inference) are PIDASHCONV-1111's layer, not this one: at the
//! `ToolDefinition` level `strict` and `include_return_schema` are both
//! `None` (probed).
//!
//! The hand-written schemas beside the services ports (`tools_issues::
//! tool_schemas`, `*_schema()` in `tools_projects`/`tools_comments`/
//! `tools_runs`/`tools_github`) are aspirational placeholders with
//! invented per-parameter descriptions and a `type: [...]` Optional form;
//! they do not match pydantic-ai output (which has no per-parameter
//! descriptions and uses `anyOf`). The turn loop must use this module.

use serde_json::{json, Value};

/// One model-facing tool definition (`tools.py:687`, `ToolDefinition`).
#[derive(Debug, Clone, PartialEq)]
pub struct ToolSchema {
    pub name: &'static str,
    pub description: &'static str,
    pub parameters: Value,
}

/// All 17 local tools in registration order.
pub fn all_tool_schemas() -> Vec<ToolSchema> {
    vec![
        ToolSchema {
            name: "create_comment",
            description: r#"Add a comment to an issue.

Set ``fold`` for low-value status/noop updates that should stay available
to humans but remain collapsed and be omitted from future agent prompts."#,
            parameters: json!({
                "additionalProperties": false,
                "properties": {
                    "issue_id": { "type": "string" },
                    "body_md": { "type": "string" },
                    "fold": {
                        "default": false,
                        "type": "boolean"
                    }
                },
                "required": ["issue_id", "body_md"],
                "type": "object"
            }),
        },
        ToolSchema {
            name: "get_pull_request_status",
            description: r#"Check whether a GitHub pull request or GitLab merge request is merged.

Pass a full PR/MR URL. Returns ``{"state": "merged"|"open"|"closed"|"unknown", ...}``.
An unsupported URL, rate limit, or network error returns ``unknown``."#,
            parameters: json!({
                "additionalProperties": false,
                "properties": {
                    "url": { "type": "string" }
                },
                "required": ["url"],
                "type": "object"
            }),
        },
        ToolSchema {
            name: "search_issues",
            description:
                "Full-text search issues you can access. Returns up to 20 results per page.",
            parameters: json!({
                "additionalProperties": false,
                "properties": {
                    "query": { "type": "string" },
                    "project_id": {
                        "anyOf": [
                            { "type": "string" },
                            { "type": "null" }
                        ],
                        "default": Value::Null
                    },
                    "limit": {
                        "default": 20,
                        "type": "integer"
                    },
                    "offset": {
                        "default": 0,
                        "type": "integer"
                    }
                },
                "required": ["query"],
                "type": "object"
            }),
        },
        ToolSchema {
            name: "list_issues",
            description: r#"List issues in one project, newest activity first, up to 20 per page.
Optional filters, each comma-separated (values OR together, filters AND
together): state (names or ids), state_group (backlog, unstarted, started,
review, test, completed, cancelled), parent_issue_id (issue id or PROJ-123
identifier, or "null" for top-level issues only), labels (names or ids),
priority (urgent, high, medium, low, none)."#,
            parameters: json!({
                "additionalProperties": false,
                "properties": {
                    "project_id": { "type": "string" },
                    "state": {
                        "anyOf": [
                            { "type": "string" },
                            { "type": "null" }
                        ],
                        "default": Value::Null
                    },
                    "state_group": {
                        "anyOf": [
                            { "type": "string" },
                            { "type": "null" }
                        ],
                        "default": Value::Null
                    },
                    "parent_issue_id": {
                        "anyOf": [
                            { "type": "string" },
                            { "type": "null" }
                        ],
                        "default": Value::Null
                    },
                    "labels": {
                        "anyOf": [
                            { "type": "string" },
                            { "type": "null" }
                        ],
                        "default": Value::Null
                    },
                    "priority": {
                        "anyOf": [
                            { "type": "string" },
                            { "type": "null" }
                        ],
                        "default": Value::Null
                    },
                    "limit": {
                        "default": 20,
                        "type": "integer"
                    },
                    "offset": {
                        "default": 0,
                        "type": "integer"
                    }
                },
                "required": ["project_id"],
                "type": "object"
            }),
        },
        ToolSchema {
            name: "list_my_issues",
            description: "List issues you're involved in. scope: 'all', 'assigned', or 'created'.",
            parameters: json!({
                "additionalProperties": false,
                "properties": {
                    "scope": {
                        "default": "all",
                        "type": "string"
                    },
                    "limit": {
                        "default": 20,
                        "type": "integer"
                    },
                    "offset": {
                        "default": 0,
                        "type": "integer"
                    }
                },
                "type": "object"
            }),
        },
        ToolSchema {
            name: "get_issue",
            description: "Get one issue in detail, including its most recent comments.",
            parameters: json!({
                "additionalProperties": false,
                "properties": {
                    "issue_id": { "type": "string" }
                },
                "required": ["issue_id"],
                "type": "object"
            }),
        },
        ToolSchema {
            name: "create_issue",
            description: r#"Create an issue. Requires Member or Admin role. Pass parent_issue_id to
link it as a sub-issue of another issue in the same project. (Assignees/labels:
set them afterwards in the UI — not yet supported by this tool.)"#,
            parameters: json!({
                "additionalProperties": false,
                "properties": {
                    "project_id": { "type": "string" },
                    "name": { "type": "string" },
                    "description_md": {
                        "anyOf": [
                            { "type": "string" },
                            { "type": "null" }
                        ],
                        "default": Value::Null
                    },
                    "state_id": {
                        "anyOf": [
                            { "type": "string" },
                            { "type": "null" }
                        ],
                        "default": Value::Null
                    },
                    "priority": {
                        "anyOf": [
                            { "type": "string" },
                            { "type": "null" }
                        ],
                        "default": Value::Null
                    },
                    "parent_issue_id": {
                        "anyOf": [
                            { "type": "string" },
                            { "type": "null" }
                        ],
                        "default": Value::Null
                    }
                },
                "required": ["project_id", "name"],
                "type": "object"
            }),
        },
        ToolSchema {
            name: "update_issue",
            description: r#"Update an issue's name, description, state, or priority. Requires write access.
Changing the state may dispatch a coding run if it moves the issue into a
delegated/ticking state (same as moving it in the UI). Pass parent_issue_id to
re-parent the issue (must be another issue in the same project); pass null to
unlink it from its current parent; omit it to leave the parent unchanged."#,
            parameters: json!({
                "additionalProperties": false,
                "properties": {
                    "issue_id": { "type": "string" },
                    "name": {
                        "anyOf": [
                            { "type": "string" },
                            { "type": "null" }
                        ],
                        "default": Value::Null
                    },
                    "description_md": {
                        "anyOf": [
                            { "type": "string" },
                            { "type": "null" }
                        ],
                        "default": Value::Null
                    },
                    "state_id": {
                        "anyOf": [
                            { "type": "string" },
                            { "type": "null" }
                        ],
                        "default": Value::Null
                    },
                    "priority": {
                        "anyOf": [
                            { "type": "string" },
                            { "type": "null" }
                        ],
                        "default": Value::Null
                    },
                    "parent_issue_id": {
                        "anyOf": [
                            { "type": "string" },
                            { "type": "null" }
                        ],
                        "default": "__unset__"
                    }
                },
                "required": ["issue_id"],
                "type": "object"
            }),
        },
        ToolSchema {
            name: "relate_issues",
            description: r#"Record a relation from ``issue`` to each of ``related_issues`` (identifiers
like PROJ-12, or UUIDs). relation_type is read from ``issue``'s side: one of
blocked_by, blocking, relates_to, duplicate, start_before, start_after,
finish_before, finish_after, implemented_by, implements. Use blocked_by when
``issue`` cannot finish until the related issues do. Idempotent: pairs that
already have this relation come back under ``unchanged``; pairs that already
have a different relation come back under ``conflicts`` and are not changed.
Requires write access to ``issue``'s project."#,
            parameters: json!({
                "additionalProperties": false,
                "properties": {
                    "issue": { "type": "string" },
                    "relation_type": { "type": "string" },
                    "related_issues": {
                        "items": { "type": "string" },
                        "type": "array"
                    }
                },
                "required": ["issue", "relation_type", "related_issues"],
                "type": "object"
            }),
        },
        ToolSchema {
            name: "unrelate_issues",
            description: r#"Remove the ``relation_type`` relation between ``issue`` and each of
``related_issues``. Only that exact relation is removed; pairs without it come
back under ``not_related`` (not an error). Requires write access."#,
            parameters: json!({
                "additionalProperties": false,
                "properties": {
                    "issue": { "type": "string" },
                    "relation_type": { "type": "string" },
                    "related_issues": {
                        "items": { "type": "string" },
                        "type": "array"
                    }
                },
                "required": ["issue", "relation_type", "related_issues"],
                "type": "object"
            }),
        },
        ToolSchema {
            name: "list_issue_relations",
            description: r#"List every relation of an issue (identifier or UUID), grouped by type from
its side (blocked_by, blocking, relates_to, ...), each with identifier, name
and state. Check ``blocked_by`` states before starting dependent work."#,
            parameters: json!({
                "additionalProperties": false,
                "properties": {
                    "issue": { "type": "string" }
                },
                "required": ["issue"],
                "type": "object"
            }),
        },
        ToolSchema {
            name: "list_projects",
            description: "List the projects in the current workspace that you can access.",
            parameters: json!({
                "additionalProperties": false,
                "properties": {},
                "type": "object"
            }),
        },
        ToolSchema {
            name: "list_states",
            description:
                "List the workflow states (e.g. Backlog, In Progress, Done) for a project.",
            parameters: json!({
                "additionalProperties": false,
                "properties": {
                    "project_id": { "type": "string" }
                },
                "required": ["project_id"],
                "type": "object"
            }),
        },
        ToolSchema {
            name: "list_labels",
            description: "List the labels available in a project.",
            parameters: json!({
                "additionalProperties": false,
                "properties": {
                    "project_id": { "type": "string" }
                },
                "required": ["project_id"],
                "type": "object"
            }),
        },
        ToolSchema {
            name: "list_project_members",
            description: "List the members of a project (id, name, role).",
            parameters: json!({
                "additionalProperties": false,
                "properties": {
                    "project_id": { "type": "string" }
                },
                "required": ["project_id"],
                "type": "object"
            }),
        },
        ToolSchema {
            name: "get_run_status",
            description: "Get the status of coding agent runs for an issue.",
            parameters: json!({
                "additionalProperties": false,
                "properties": {
                    "issue_id": { "type": "string" }
                },
                "required": ["issue_id"],
                "type": "object"
            }),
        },
        ToolSchema {
            name: "dispatch_coding_run",
            description: r#"Start a coding agent run on an issue by moving it into a delegated state.
Requires write access. Identical to dragging the issue into that state in the UI."#,
            parameters: json!({
                "additionalProperties": false,
                "properties": {
                    "issue_id": { "type": "string" },
                    "target_state_id": {
                        "anyOf": [
                            { "type": "string" },
                            { "type": "null" }
                        ],
                        "default": Value::Null
                    }
                },
                "required": ["issue_id"],
                "type": "object"
            }),
        },
    ]
}

/// Look up one tool's schema by name.
#[allow(dead_code)] // lookup used by the tests; `all_tool_schemas` is what the seam exposes
pub fn tool_schema(name: &str) -> Option<ToolSchema> {
    all_tool_schemas()
        .into_iter()
        .find(|schema| schema.name == name)
}

/// Tool names in the sorted order pydantic-ai lists them in the
/// unknown-tool retry (`tool_manager.py:364-365`).
pub fn tool_names_sorted() -> Vec<&'static str> {
    let mut names: Vec<&'static str> = all_tool_schemas()
        .iter()
        .map(|schema| schema.name)
        .collect();
    names.sort_unstable();
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Value {
        let path = format!(
            "{}/../../fixtures/assistant/tools-local-schemas.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(&path).expect("fixture exists"))
            .expect("fixture parses")
    }

    /// The ported schemas are byte-identical to the captured
    /// pydantic-ai output: same names, same descriptions, same parameter
    /// documents with the same key order (`serde_json` runs with
    /// `preserve_order`, so `to_string` comparison is order-sensitive).
    #[test]
    fn schemas_match_captured_pydantic_ai_output() {
        let captured: Vec<Value> = fixture().as_array().expect("array").clone();
        let ported = all_tool_schemas();
        assert_eq!(ported.len(), 17, "all 17 local tools");
        assert_eq!(captured.len(), 17, "fixture holds all 17 tools");
        for schema in &ported {
            let expected = captured
                .iter()
                .find(|entry| entry["name"] == schema.name)
                .unwrap_or_else(|| panic!("fixture holds {}", schema.name));
            assert_eq!(
                schema.description,
                expected["description"].as_str().unwrap(),
                "description of {}",
                schema.name
            );
            assert_eq!(
                serde_json::to_string(&schema.parameters).unwrap(),
                serde_json::to_string(&expected["parameters_json_schema"]).unwrap(),
                "parameters of {}",
                schema.name
            );
        }
    }

    /// Registration order follows the package import order
    /// (`tools/__init__.py:11`).
    #[test]
    fn schemas_follow_registration_order() {
        let names: Vec<&str> = all_tool_schemas()
            .iter()
            .map(|schema| schema.name)
            .collect();
        assert_eq!(
            names,
            vec![
                "create_comment",
                "get_pull_request_status",
                "search_issues",
                "list_issues",
                "list_my_issues",
                "get_issue",
                "create_issue",
                "update_issue",
                "relate_issues",
                "unrelate_issues",
                "list_issue_relations",
                "list_projects",
                "list_states",
                "list_labels",
                "list_project_members",
                "get_run_status",
                "dispatch_coding_run",
            ]
        );
    }
}
