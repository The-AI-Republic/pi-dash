//! Intake serializers A: `IntakeSerializer` + `IntakeIssueSerializer`.
//!
//! Ports `apps/api/pi_dash/app/serializers/intake.py:17-90`.
//!
//! JSON contract: both serializers emit keys in exact Python order —
//! `IntakeSerializer` follows the DRF `__all__` order recorded in the
//! fixture (`[pk]` + declared base-first + concrete model fields in model
//! order + forward relations), `IntakeIssueSerializer` follows its explicit
//! 7-field `Meta.fields` list order. Struct field declaration order is the
//! emission order (`serde` emits fields in declaration order), so the
//! structs below declare fields in fixture `key_order` and the tests assert
//! byte-identical replay of both golden `example_output` bodies.
//!
//! Nested shapes are `serde_json::Value` passthrough: `project_detail` is
//! `ProjectLiteSerializer` (`project.py:120-132`, owned by the project
//! domain) and `issue` is `IssueIntakeSerializer` (`issue.py:1021-1036`,
//! owned by the issues domain). This module owns neither shape; it owns
//! the key order, the read-only sets, and the three method rules.
//!
//! Fixtures:
//! `rust-api/fixtures/app_intake/serializers/intake.golden.json`,
//! `rust-api/fixtures/app_intake/serializers/intake_issue.golden.json`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// `IntakeSerializer` output keys in DRF `__all__` render order
/// (`intake.py:17-24`; order verified in the fixture via the installed DRF).
pub const INTAKE_KEY_ORDER: &[&str] = &[
    "id",
    "project_detail",
    "pending_issue_count",
    "created_at",
    "updated_at",
    "deleted_at",
    "name",
    "description",
    "is_default",
    "view_props",
    "logo_props",
    "created_by",
    "updated_by",
    "project",
    "workspace",
];

/// `IntakeSerializer.Meta.read_only_fields` (`intake.py:24`).
pub const INTAKE_READ_ONLY_FIELDS: &[&str] = &["project", "workspace"];

/// `IntakeIssueSerializer.Meta.fields`, in list order (`intake.py:30-38`).
pub const INTAKE_ISSUE_FIELDS: &[&str] = &[
    "id",
    "status",
    "duplicate_to",
    "snoozed_till",
    "source",
    "issue",
    "created_by",
];

/// `IntakeIssueSerializer.Meta.read_only_fields` (`intake.py:39`).
pub const INTAKE_ISSUE_READ_ONLY_FIELDS: &[&str] = &["project", "workspace"];

/// `IntakeIssueStatus` values (`db/models/intake.py:42-47`, via the fixture
/// enums block; repeated here because `validate`/`update` branch on them).
pub const INTAKE_ISSUE_STATUS_PENDING: i32 = -2;
/// `IntakeIssueStatus.REJECTED` (`db/models/intake.py:43`).
pub const INTAKE_ISSUE_STATUS_REJECTED: i32 = -1;
/// `IntakeIssueStatus.SNOOZED` (`db/models/intake.py:44`).
pub const INTAKE_ISSUE_STATUS_SNOOZED: i32 = 0;
/// `IntakeIssueStatus.ACCEPTED` (`db/models/intake.py:45`).
pub const INTAKE_ISSUE_STATUS_ACCEPTED: i32 = 1;
/// `IntakeIssueStatus.DUPLICATE` (`db/models/intake.py:46`).
pub const INTAKE_ISSUE_STATUS_DUPLICATE: i32 = 2;

/// `StateGroup.TRIAGE.value` (`db/models/state.py:22`).
pub const ISSUE_STATE_GROUP_TRIAGE: &str = "triage";

/// `validate` rejection message (`intake.py:62`).
pub const NO_DEFAULT_STATE_MESSAGE: &str =
    "Cannot accept intake issue: No default state found for the project";

/// `IntakeSerializer` output (`intake.py:17-24`).
///
/// Fields are declared in [`INTAKE_KEY_ORDER`] so serialization emits the
/// DRF `__all__` order. `project_detail` is the nested `ProjectLiteSerializer`
/// output and `pending_issue_count` is the `get_queryset` annotation
/// (`base.py:68`); both are read-only. Datetimes are carried as the
/// DRF-rendered strings, matching the fixture `example_output`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IntakeRecord {
    pub id: String,
    pub project_detail: Value,
    pub pending_issue_count: i64,
    pub created_at: String,
    pub updated_at: String,
    pub deleted_at: Option<String>,
    pub name: String,
    pub description: String,
    pub is_default: bool,
    pub view_props: Value,
    pub logo_props: Value,
    pub created_by: Option<String>,
    pub updated_by: Option<String>,
    pub project: String,
    pub workspace: String,
}

/// `IntakeIssueSerializer` output (`intake.py:27-39`).
///
/// Fields are declared in [`INTAKE_ISSUE_FIELDS`] (explicit `Meta.fields`
/// order). `issue` is the nested `IssueIntakeSerializer` output, carried
/// through verbatim; `to_representation` injects the annotated `label_ids`
/// into it via [`apply_label_ids_annotation`] before render.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IntakeIssueRecord {
    pub id: String,
    pub status: i32,
    pub duplicate_to: Option<String>,
    pub snoozed_till: Option<String>,
    pub source: Option<String>,
    pub issue: Value,
    pub created_by: Option<String>,
}

/// The exact HTTP 400 body for the no-default-state rejection.
///
/// DRF renders `ValidationError({"status": "..."})` as
/// `{"status":"<message>"}` — the dict-of-string detail keeps a single
/// `ErrorDetail` (a `str` subclass, no list wrap), verified against the
/// installed DRF 3.18.1 JSON renderer. Byte-exact with the fixture
/// `output_error`.
pub fn no_default_state_error_body() -> String {
    serde_json::to_string(&serde_json::json!({"status": NO_DEFAULT_STATE_MESSAGE}))
        .expect("error body serializes")
}

/// `IntakeIssueSerializer.validate` (`intake.py:43-66`).
///
/// Mirrors the Python exactly: only a transition to accepted (`status == 1`,
/// i.e. `attrs.get("status") == 1` — absent status never triggers) while the
/// linked issue sits in a TRIAGE state (`issue.state` set and
/// `group == "triage"`) requires a project default state
/// (`State.objects.filter(workspace, project, default=True).first()`).
/// Without one the update is rejected with the 400 body from
/// [`no_default_state_error_body`].
///
/// `issue_state_group` is `None` when the linked issue has no state
/// (Python `issue.state` falsy skips the check); `has_default_state` is
/// whether the default-state lookup returned a row.
pub fn validate_status_transition(
    new_status: Option<i32>,
    issue_state_group: Option<&str>,
    has_default_state: bool,
) -> Result<(), String> {
    if new_status == Some(INTAKE_ISSUE_STATUS_ACCEPTED)
        && issue_state_group == Some(ISSUE_STATE_GROUP_TRIAGE)
        && !has_default_state
    {
        return Err(no_default_state_error_body());
    }
    Ok(())
}

/// `IntakeIssueSerializer.update` TRIAGE-to-default move (`intake.py:68-84`).
///
/// After the row update, when the validated status is accepted and the
/// linked issue is in TRIAGE, the handler sets the issue's state to the
/// project default and saves it. Returns the default state id to assign, or
/// `None` when no move applies. Like the Python, a missing default row is a
/// silent no-op here (unreachable via DRF anyway: `validate` rejects that
/// case first, so `update` only runs when the row exists).
pub fn accepted_issue_transition(
    new_status: Option<i32>,
    issue_state_group: Option<&str>,
    default_state_id: Option<&str>,
) -> Option<String> {
    if new_status == Some(INTAKE_ISSUE_STATUS_ACCEPTED)
        && issue_state_group == Some(ISSUE_STATE_GROUP_TRIAGE)
    {
        return default_state_id.map(str::to_owned);
    }
    None
}

/// `IntakeIssueSerializer.to_representation` (`intake.py:86-90`).
///
/// When the row carries the list-query `label_ids` annotation, it is copied
/// onto the nested issue object before rendering
/// (`instance.issue.label_ids = instance.label_ids`); otherwise the nested
/// issue renders untouched. Operates on the rendered `issue` value so the
/// caller needs no typed nested shape.
pub fn apply_label_ids_annotation(issue: &mut Value, label_ids: Option<&Value>) {
    if let Some(ids) = label_ids {
        if let Some(obj) = issue.as_object_mut() {
            obj.insert("label_ids".to_owned(), ids.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn golden(name: &str) -> Value {
        let path = format!(
            "{}/../../fixtures/app_intake/serializers/{name}",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(&path).expect("golden exists"))
            .expect("golden parses")
    }

    fn str_keys(value: &Value) -> Vec<String> {
        value
            .as_object()
            .expect("object")
            .keys()
            .map(|k| k.to_owned())
            .collect()
    }

    /// Top-level JSON key order of a struct's serialization, read off the
    /// rendered string: without the workspace `preserve_order` feature a
    /// `serde_json::Value` object iterates alphabetically, while struct
    /// serialization always emits declaration order.
    fn serialized_keys<T: serde::Serialize>(value: &T) -> Vec<String> {
        let rendered = serde_json::to_string(value).expect("serializes");
        let mut keys = Vec::new();
        let mut depth = 0usize;
        let mut chars = rendered.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '{' => depth += 1,
                '}' => depth -= 1,
                '"' if depth == 1 => {
                    let mut key = String::new();
                    while let Some(&next) = chars.peek() {
                        chars.next();
                        if next == '"' {
                            break;
                        }
                        key.push(next);
                    }
                    if chars.peek() == Some(&':') {
                        keys.push(key);
                    }
                }
                _ => {}
            }
        }
        keys
    }

    /// Re-render a golden `example_output` object with keys in `order`,
    /// preserving each value's exact bytes.
    fn ordered_body(example: &Value, order: &[&str]) -> String {
        let obj = example.as_object().expect("example is an object");
        let parts: Vec<String> = order
            .iter()
            .map(|k| {
                let v = obj.get(*k).unwrap_or(&Value::Null);
                format!(
                    "\"{}\":{}",
                    k,
                    serde_json::to_string(v).expect("value serializes")
                )
            })
            .collect();
        format!("{{{}}}", parts.join(","))
    }

    fn string_field(example: &Value, key: &str) -> String {
        example
            .get(key)
            .and_then(Value::as_str)
            .expect("string field")
            .to_owned()
    }

    fn opt_string_field(example: &Value, key: &str) -> Option<String> {
        match example.get(key) {
            None | Some(Value::Null) => None,
            Some(Value::String(s)) => Some(s.clone()),
            Some(other) => panic!("expected string|null for {key}, got {other}"),
        }
    }

    #[test]
    fn intake_key_order_matches_golden() {
        let fixture = golden("intake.golden.json");
        let order: Vec<String> = fixture
            .get("key_order")
            .expect("key_order")
            .as_array()
            .expect("array")
            .iter()
            .map(|v| v.as_str().expect("str").to_owned())
            .collect();
        let expected: Vec<String> = INTAKE_KEY_ORDER.iter().map(|s| s.to_string()).collect();
        assert_eq!(expected, order);
        assert_eq!(INTAKE_READ_ONLY_FIELDS, &["project", "workspace"]);
    }

    #[test]
    fn intake_example_replays_byte_identical() {
        let fixture = golden("intake.golden.json");
        let example = fixture.get("example_output").expect("example_output");
        let record = IntakeRecord {
            id: string_field(example, "id"),
            project_detail: example
                .get("project_detail")
                .expect("project_detail")
                .clone(),
            pending_issue_count: example
                .get("pending_issue_count")
                .and_then(Value::as_i64)
                .expect("pending_issue_count"),
            created_at: string_field(example, "created_at"),
            updated_at: string_field(example, "updated_at"),
            deleted_at: opt_string_field(example, "deleted_at"),
            name: string_field(example, "name"),
            description: string_field(example, "description"),
            is_default: example
                .get("is_default")
                .and_then(Value::as_bool)
                .expect("bool"),
            view_props: example.get("view_props").expect("view_props").clone(),
            logo_props: example.get("logo_props").expect("logo_props").clone(),
            created_by: opt_string_field(example, "created_by"),
            updated_by: opt_string_field(example, "updated_by"),
            project: string_field(example, "project"),
            workspace: string_field(example, "workspace"),
        };
        let order: Vec<String> = fixture
            .get("key_order")
            .expect("key_order")
            .as_array()
            .expect("array")
            .iter()
            .map(|v| v.as_str().expect("str").to_owned())
            .collect();
        assert_eq!(serialized_keys(&record), order);
        let actual = serde_json::to_string(&record).expect("serializes");
        assert_eq!(actual, ordered_body(example, INTAKE_KEY_ORDER));
    }

    #[test]
    fn intake_issue_keys_match_golden() {
        let fixture = golden("intake_issue.golden.json");
        let meta = fixture.get("meta").expect("meta");
        assert_eq!(
            meta.get("model").and_then(Value::as_str),
            Some("IntakeIssue")
        );
        let fields: Vec<String> = meta
            .get("fields")
            .expect("fields")
            .as_array()
            .expect("array")
            .iter()
            .map(|v| v.as_str().expect("str").to_owned())
            .collect();
        let expected: Vec<String> = INTAKE_ISSUE_FIELDS.iter().map(|s| s.to_string()).collect();
        assert_eq!(expected, fields);
        let order: Vec<String> = fixture
            .get("key_order")
            .expect("key_order")
            .as_array()
            .expect("array")
            .iter()
            .map(|v| v.as_str().expect("str").to_owned())
            .collect();
        assert_eq!(expected, order);
        assert_eq!(INTAKE_ISSUE_READ_ONLY_FIELDS, &["project", "workspace"]);
    }

    #[test]
    fn intake_issue_example_replays_byte_identical() {
        let fixture = golden("intake_issue.golden.json");
        let example = fixture.get("example_output").expect("example_output");
        // to_representation passes the annotated label_ids onto the nested
        // issue; the golden example already carries them, so strip + reapply
        // must round-trip to the golden nested value.
        let issue = example.get("issue").expect("issue").clone();
        let annotated = issue.get("label_ids").cloned();
        let mut bare = issue.clone();
        bare.as_object_mut().expect("object").remove("label_ids");
        apply_label_ids_annotation(&mut bare, annotated.as_ref());
        assert_eq!(bare, issue);
        let record = IntakeIssueRecord {
            id: string_field(example, "id"),
            status: example
                .get("status")
                .and_then(Value::as_i64)
                .expect("status") as i32,
            duplicate_to: opt_string_field(example, "duplicate_to"),
            snoozed_till: opt_string_field(example, "snoozed_till"),
            source: opt_string_field(example, "source"),
            issue: example.get("issue").expect("issue").clone(),
            created_by: opt_string_field(example, "created_by"),
        };
        assert_eq!(
            serialized_keys(&record),
            INTAKE_ISSUE_FIELDS
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
        );
        let actual = serde_json::to_string(&record).expect("serializes");
        assert_eq!(actual, ordered_body(example, INTAKE_ISSUE_FIELDS));
    }

    #[test]
    fn validate_accept_without_default_state_is_400() {
        // Fixture case 1 (intake.py:43-66): status->1 in TRIAGE, no default.
        let fixture = golden("intake_issue.golden.json");
        let case = fixture
            .get("cases")
            .expect("cases")
            .as_array()
            .expect("array")
            .iter()
            .find(|c| c.get("status").and_then(Value::as_i64) == Some(400))
            .expect("400 case");
        let err = validate_status_transition(
            Some(INTAKE_ISSUE_STATUS_ACCEPTED),
            Some(ISSUE_STATE_GROUP_TRIAGE),
            false,
        )
        .expect_err("must reject");
        assert_eq!(err, no_default_state_error_body());
        let expected = serde_json::to_string(case.get("output_error").expect("output_error"))
            .expect("serializes");
        assert_eq!(err, expected);
        assert_eq!(
            err,
            r#"{"status":"Cannot accept intake issue: No default state found for the project"}"#
        );
    }

    #[test]
    fn validate_passes_outside_accept_in_triage() {
        // Fixture case 2: any status other than 1 passes through unchanged.
        assert!(validate_status_transition(
            Some(INTAKE_ISSUE_STATUS_REJECTED),
            Some("triage"),
            false
        )
        .is_ok());
        assert!(validate_status_transition(
            Some(INTAKE_ISSUE_STATUS_PENDING),
            Some("triage"),
            false
        )
        .is_ok());
        // Absent status (attrs.get -> None) never triggers the check.
        assert!(validate_status_transition(None, Some("triage"), false).is_ok());
        // Linked issue without a state skips the check (Python falsy state).
        assert!(
            validate_status_transition(Some(INTAKE_ISSUE_STATUS_ACCEPTED), None, false).is_ok()
        );
        // Non-triage groups skip the check.
        assert!(validate_status_transition(
            Some(INTAKE_ISSUE_STATUS_ACCEPTED),
            Some("backlog"),
            false
        )
        .is_ok());
        // Default state present: accept proceeds.
        assert!(validate_status_transition(
            Some(INTAKE_ISSUE_STATUS_ACCEPTED),
            Some("triage"),
            true
        )
        .is_ok());
    }

    #[test]
    fn update_moves_triage_issue_to_default() {
        // Fixture case 3 (intake.py:68-84): status->1 with a default present
        // moves the linked issue TRIAGE->default.
        let moved = accepted_issue_transition(
            Some(INTAKE_ISSUE_STATUS_ACCEPTED),
            Some(ISSUE_STATE_GROUP_TRIAGE),
            Some("default-state-id"),
        );
        assert_eq!(moved.as_deref(), Some("default-state-id"));
        // Missing default row: silent no-op (Python `if default_state:`).
        assert_eq!(
            accepted_issue_transition(Some(INTAKE_ISSUE_STATUS_ACCEPTED), Some("triage"), None),
            None
        );
        // Non-accept transitions and non-triage issues: no move.
        assert_eq!(
            accepted_issue_transition(
                Some(INTAKE_ISSUE_STATUS_REJECTED),
                Some("triage"),
                Some("d")
            ),
            None
        );
        assert_eq!(
            accepted_issue_transition(
                Some(INTAKE_ISSUE_STATUS_ACCEPTED),
                Some("backlog"),
                Some("d")
            ),
            None
        );
        assert_eq!(
            accepted_issue_transition(None, Some("triage"), Some("d")),
            None
        );
    }

    #[test]
    fn to_representation_copies_label_ids_only_when_annotated() {
        let mut issue = serde_json::json!({"id": "x", "name": "n"});
        apply_label_ids_annotation(&mut issue, Some(&serde_json::json!(["a"])));
        assert_eq!(issue.get("label_ids"), Some(&serde_json::json!(["a"])));
        let mut untouched = serde_json::json!({"id": "x"});
        apply_label_ids_annotation(&mut untouched, None);
        assert_eq!(str_keys(&untouched), vec!["id".to_owned()]);
    }
}
