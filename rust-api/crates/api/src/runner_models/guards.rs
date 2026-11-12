#![forbid(unsafe_code)]

//! Validation for the runner LLM model catalog (NEWPIDASH-169).
//!
//! Pure helpers over `serde_json` bodies: no I/O, so the decision shapes
//! are unit-testable without a database. Handlers layer the row fetching,
//! duplicate checks against the database, and the always-in-transaction
//! preselect clearing on top of these.
//!
//! Validation codes (each answered as `400 {"error":"<code>"}`):
//! `not_an_object`, `unknown_agent`, `empty_model`, `empty_label`,
//! `invalid_effort`, `duplicate_effort`, `invalid_type`, `invalid_body`.

use serde_json::Value;

/// Agents the runner CLI accepts on `--agent` (kept in sync with
/// `apps/web_new/src/features/runners/addRunner/catalog.ts` `RUNNER_AGENTS`;
/// the list itself stays in code per NEWPIDASH-169).
pub const KNOWN_AGENTS: [&str; 6] = [
    "claude-code",
    "codex",
    "cursor-agent",
    "open-claw",
    "grok",
    "muse-code",
];

/// Reasoning-effort values offered by Codex models (the picker crosses
/// each Codex row's efforts with the model slug).
pub const KNOWN_EFFORTS: [&str; 4] = ["low", "medium", "high", "xhigh"];

pub fn is_known_agent(agent: &str) -> bool {
    KNOWN_AGENTS.contains(&agent)
}

pub fn is_known_effort(effort: &str) -> bool {
    KNOWN_EFFORTS.contains(&effort)
}

fn trimmed_string(body: &Value, key: &str) -> Result<Option<String>, &'static str> {
    match body.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => Ok(Some(text.trim().to_owned())),
        Some(_) => Err("invalid_type"),
    }
}

fn optional_bool(body: &Value, key: &str) -> Result<Option<bool>, &'static str> {
    match body.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(value)) => Ok(Some(*value)),
        Some(_) => Err("invalid_type"),
    }
}

fn optional_sort_order(body: &Value) -> Result<Option<i32>, &'static str> {
    match body.get("sort_order") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(number)) => {
            let Some(value) = number.as_i64() else {
                return Err("invalid_type");
            };
            let Ok(value) = i32::try_from(value) else {
                return Err("invalid_type");
            };
            Ok(Some(value))
        }
        Some(_) => Err("invalid_type"),
    }
}

fn optional_efforts(body: &Value) -> Result<Option<Vec<String>>, &'static str> {
    match body.get("reasoning_efforts") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Array(items)) => {
            let mut efforts = Vec::with_capacity(items.len());
            for item in items {
                let Value::String(text) = item else {
                    return Err("invalid_type");
                };
                let value = text.trim();
                if value.is_empty() {
                    return Err("invalid_type");
                }
                if !is_known_effort(value) {
                    return Err("invalid_effort");
                }
                if efforts.iter().any(|seen: &String| seen == value) {
                    return Err("duplicate_effort");
                }
                efforts.push(value.to_owned());
            }
            Ok(Some(efforts))
        }
        Some(_) => Err("invalid_type"),
    }
}

/// A validated create body. `reasoning_efforts`, `is_enabled` and
/// `is_preselected` default to empty / true / false; `sort_order` is
/// resolved by the handler (appends after the agent's last row) when
/// absent.
pub struct NewModel {
    pub agent: String,
    pub model: String,
    pub label: String,
    pub reasoning_efforts: Vec<String>,
    pub sort_order: Option<i32>,
    pub is_enabled: Option<bool>,
    pub is_preselected: Option<bool>,
}

/// A validated partial update. Every field is optional; an empty patch
/// leaves the row unchanged.
pub struct PatchModel {
    pub model: Option<String>,
    pub label: Option<String>,
    pub reasoning_efforts: Option<Vec<String>>,
    pub sort_order: Option<i32>,
    pub is_enabled: Option<bool>,
    pub is_preselected: Option<bool>,
}

/// Reject non-object bodies the way the catalog endpoints do: anything
/// but a JSON object counts as `invalid_body`.
pub fn is_empty_object(value: &Value) -> bool {
    value.as_object().is_some_and(|map| map.is_empty())
}

/// Validate a `POST` body. Err is the `{"error": …}` code.
pub fn validate_create(body: &Value) -> Result<NewModel, &'static str> {
    if !body.is_object() {
        return Err("invalid_body");
    }
    let agent = trimmed_string(body, "agent")?;
    let Some(agent) = agent else {
        return Err("unknown_agent");
    };
    if !is_known_agent(&agent) {
        return Err("unknown_agent");
    }
    let model = trimmed_string(body, "model")?;
    let Some(model) = model else {
        return Err("empty_model");
    };
    if model.is_empty() {
        return Err("empty_model");
    }
    let label = trimmed_string(body, "label")?;
    let Some(label) = label else {
        return Err("empty_label");
    };
    if label.is_empty() {
        return Err("empty_label");
    }
    Ok(NewModel {
        agent,
        model,
        label,
        reasoning_efforts: optional_efforts(body)?.unwrap_or_default(),
        sort_order: optional_sort_order(body)?,
        is_enabled: optional_bool(body, "is_enabled")?,
        is_preselected: optional_bool(body, "is_preselected")?,
    })
}

/// Validate a `PATCH` body. `agent` is never writable (the list stays in
/// code; a model belongs to one agent for its whole lifetime) — a
/// different agent in the body is ignored, like other unknown keys.
pub fn validate_patch(body: &Value) -> Result<PatchModel, &'static str> {
    if !body.is_object() {
        return Err("invalid_body");
    }
    if let Some(model) = trimmed_string(body, "model")? {
        if model.is_empty() {
            return Err("empty_model");
        }
    }
    if let Some(label) = trimmed_string(body, "label")? {
        if label.is_empty() {
            return Err("empty_label");
        }
    }
    Ok(PatchModel {
        model: trimmed_string(body, "model")?,
        label: trimmed_string(body, "label")?,
        reasoning_efforts: optional_efforts(body)?,
        sort_order: optional_sort_order(body)?,
        is_enabled: optional_bool(body, "is_enabled")?,
        is_preselected: optional_bool(body, "is_preselected")?,
    })
}

/// Duplicate decision: `(agent, model)` must be unique among the rows the
/// caller passes (its own agent's rows, excluding the row being patched).
/// Pure, so the handler keeps the query and the tests pin the shape.
pub fn is_duplicate(rows: &[(&str, &str)], agent: &str, model: &str) -> bool {
    rows.iter()
        .any(|(row_agent, row_model)| *row_agent == agent && *row_model == model)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn create_accepts_a_minimal_body() {
        let parsed = validate_create(&json!({
            "agent": "codex", "model": "gpt-5.5", "label": "GPT-5.5",
        }))
        .expect("valid");
        assert_eq!(parsed.agent, "codex");
        assert_eq!(parsed.model, "gpt-5.5");
        assert_eq!(parsed.label, "GPT-5.5");
        assert!(parsed.reasoning_efforts.is_empty());
        assert_eq!(parsed.is_enabled, None);
        assert_eq!(parsed.is_preselected, None);
    }

    #[test]
    fn create_rejects_an_unknown_agent() {
        assert_eq!(
            validate_create(&json!({
                "agent": "nope", "model": "m", "label": "M",
            }))
            .err(),
            Some("unknown_agent")
        );
        assert_eq!(
            validate_create(&json!({"model": "m", "label": "M"})).err(),
            Some("unknown_agent")
        );
    }

    #[test]
    fn create_rejects_empty_model_and_label() {
        assert_eq!(
            validate_create(&json!({
                "agent": "codex", "model": "   ", "label": "M",
            }))
            .err(),
            Some("empty_model")
        );
        assert_eq!(
            validate_create(&json!({
                "agent": "codex", "model": "m", "label": "",
            }))
            .err(),
            Some("empty_label")
        );
    }

    #[test]
    fn create_rejects_efforts_outside_the_allowed_set() {
        assert_eq!(
            validate_create(&json!({
                "agent": "codex", "model": "m", "label": "M",
                "reasoning_efforts": ["low", "ultra"],
            }))
            .err(),
            Some("invalid_effort")
        );
    }

    #[test]
    fn create_rejects_duplicate_efforts() {
        assert_eq!(
            validate_create(&json!({
                "agent": "codex", "model": "m", "label": "M",
                "reasoning_efforts": ["low", "low"],
            }))
            .err(),
            Some("duplicate_effort")
        );
    }

    #[test]
    fn create_rejects_wrong_types_and_non_objects() {
        assert_eq!(
            validate_create(&json!({"agent": 3, "model": "m", "label": "M"})).err(),
            Some("invalid_type")
        );
        assert_eq!(
            validate_create(&json!({
                "agent": "codex", "model": "m", "label": "M", "sort_order": 1.5,
            }))
            .err(),
            Some("invalid_type")
        );
        assert_eq!(validate_create(&json!([])).err(), Some("invalid_body"));
        assert_eq!(validate_create(&json!("x")).err(), Some("invalid_body"));
    }

    #[test]
    fn patch_accepts_empty_and_partial_bodies() {
        let parsed = validate_patch(&json!({})).expect("empty is a no-op");
        assert_eq!(parsed.model, None);
        let parsed = validate_patch(&json!({
            "label": "New", "is_enabled": false, "reasoning_efforts": ["high"],
        }))
        .expect("partial");
        assert_eq!(parsed.label, Some("New".to_owned()));
        assert_eq!(parsed.is_enabled, Some(false));
        assert_eq!(parsed.reasoning_efforts, Some(vec!["high".to_owned()]));
    }

    #[test]
    fn patch_ignores_agent() {
        let parsed = validate_patch(&json!({"agent": "grok", "label": "X"})).expect("valid");
        assert_eq!(parsed.label, Some("X".to_owned()));
    }

    #[test]
    fn duplicate_matches_agent_and_model_only() {
        let rows = vec![("codex", "gpt-5.5"), ("claude-code", "opus")];
        assert!(is_duplicate(&rows, "codex", "gpt-5.5"));
        assert!(!is_duplicate(&rows, "codex", "gpt-5.4"));
        assert!(!is_duplicate(&rows, "grok", "gpt-5.5"));
    }
}
