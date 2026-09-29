#![forbid(unsafe_code)]

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
//!
//! Intake serializers B: detail + lite + state (D-32, PIDASHCONV-282).
//!
//! Ports `apps/api/pi_dash/app/serializers/intake.py:93-139`:
//! - `IntakeIssueDetailSerializer` (`:93-117`): explicit 7-key `Meta.fields`
//!   with `issue` (`IssueDetailSerializer`, read-only) and
//!   `duplicate_issue_detail` (`IssueIntakeSerializer`, read-only,
//!   `source="duplicate_to"`); `to_representation` copies the annotated
//!   `assignee_ids` / `label_ids` onto the nested issue object first, each
//!   guarded by `hasattr` so absent annotations are skipped, not defaulted.
//! - `IntakeIssueLiteSerializer` (`:120-124`): 5 read-only fields, rendered
//!   in `Meta.fields` order. Consumed as a nested `many=True` read shape by
//!   `IssueStateIntakeSerializer.issue_intake` (`:133`).
//! - `IssueStateIntakeSerializer` (`:127-139`): `Meta.exclude = ["workpad"]`
//!   over the `Issue` model (`:136-139` — the agent workpad is served only
//!   by the dedicated workpad endpoint, never as part of an intake
//!   payload), with `state_detail` / `project_detail` / `label_details` /
//!   `assignee_details` nested details, the annotated `sub_issues_count`,
//!   and the reverse-FK `issue_intake` (`IntakeIssueLiteSerializer`,
//!   `many=True`).
//!
//! The nested detail shapes (`IssueDetailSerializer`,
//! `IssueIntakeSerializer`, `StateLiteSerializer`, `ProjectLiteSerializer`,
//! `LabelLiteSerializer`, `UserLiteSerializer`) belong to other domains:
//! they are referenced here by name only, never redefined.
//!
//! Fixtures (PIDASHCONV-278):
//! `rust-api/fixtures/app_intake/serializers/intake_issue_detail.golden.json`,
//! `serializers/intake_issue_lite.golden.json`,
//! `serializers/issue_state_intake.golden.json`.
//! Serializers A (`IntakeSerializer`, `IntakeIssueSerializer`,
//! `intake.py:17-90`, PIDASHCONV-281) live above in this same file;
//! the B section below must not move or rename anything above.
//! already-rendered strings supplied by the query layer, and statuses as
//! `i64` (`IntakeIssueStatus`: PENDING=-2, REJECTED=-1, SNOOZED=0,
//! ACCEPTED=1, DUPLICATE=2; `db/models/intake.py:42-47`). None of these
//! three serializers validates input — every field is read-only output —
//! so there are no error bodies to carry.
//!
//! JSON note (Porting guide DRF rows): explicit `Meta.fields` renders in
//! list order, which the builders below reproduce by inserting into the
//! `serde_json::Map` in field order (workspace `preserve_order`
//! unification keeps that order in the serialized bytes). The replay tests
//! assert both canonical (key-sorted) equality with the goldens and exact
//! key order against each fixture's `key_order`.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

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

// ---------------------------------------------------------------------------
// IntakeIssueDetailSerializer (intake.py:93-117)
// ---------------------------------------------------------------------------

/// `Meta.fields` (`intake.py:99-107`), the render order.
pub const DETAIL_FIELDS: &[&str] = &[
    "id",
    "status",
    "duplicate_to",
    "snoozed_till",
    "duplicate_issue_detail",
    "source",
    "issue",
];

/// `Meta.read_only_fields` (`intake.py:108`). Neither name is in `fields`,
/// so both entries are inert (DRF only applies `read_only_fields` to known
/// fields); carried here so the declaration stays complete.
pub const DETAIL_READ_ONLY_FIELDS: &[&str] = &["project", "workspace"];

/// Nested `issue` shape (`intake.py:94`): `IssueDetailSerializer`,
/// read-only. Owned by the issues domain; referenced only.
pub const DETAIL_NESTED_ISSUE_TYPE: &str = "IssueDetailSerializer";

/// Nested `duplicate_issue_detail` shape (`intake.py:95`):
/// `IssueIntakeSerializer`, read-only, `source="duplicate_to"` — null
/// whenever `duplicate_to` is null. Owned by the issues domain; referenced
/// only.
pub const DETAIL_NESTED_DUPLICATE_TYPE: &str = "IssueIntakeSerializer";

/// Source attribute of `duplicate_issue_detail` (`intake.py:95`).
pub const DETAIL_DUPLICATE_SOURCE: &str = "duplicate_to";

/// One `IntakeIssue` row plus its already-rendered nested shapes, as the
/// query layer supplies them. `snoozed_till` is the DRF-rendered datetime
/// string (null when unset); `duplicate_issue_detail` is the rendered
/// nested issue for `duplicate_to`. Because the field reads through
/// `source="duplicate_to"` (`intake.py:95`), a null `duplicate_to` always
/// renders a null detail — any supplied detail is ignored in that case.
pub struct DetailRow<'a> {
    pub id: &'a str,
    pub status: i64,
    pub duplicate_to: Option<&'a str>,
    pub snoozed_till: Option<&'a str>,
    pub duplicate_issue_detail: Option<Value>,
    pub source: Option<&'a str>,
    pub issue: Value,
}

/// The query annotations `to_representation` (`intake.py:110-117`) looks
/// for. Each side is `Some` exactly when the instance carries that
/// annotation (`hasattr` true); `None` means absent, never an empty list.
pub struct DetailAnnotations<'a> {
    pub assignee_ids: Option<&'a Value>,
    pub label_ids: Option<&'a Value>,
}

/// Ports `IntakeIssueDetailSerializer.to_representation`
/// (`intake.py:110-117`): copy each present annotation onto the nested
/// issue object. Absent annotations leave the object untouched — Python
/// checks `hasattr` per key, so one annotation present never implies the
/// other.
pub fn apply_detail_annotations(
    issue: &mut Map<String, Value>,
    annotations: DetailAnnotations<'_>,
) {
    if let Some(ids) = annotations.assignee_ids {
        issue.insert("assignee_ids".to_owned(), ids.clone());
    }
    if let Some(ids) = annotations.label_ids {
        issue.insert("label_ids".to_owned(), ids.clone());
    }
}

/// Renders a detail body in `DETAIL_FIELDS` order. The nested `issue` value
/// is carried through untouched: callers apply [`apply_detail_annotations`]
/// to it first, exactly as Python mutates `instance.issue` before
/// `super().to_representation(instance)`.
pub fn render_detail(row: &DetailRow<'_>) -> Value {
    let mut body = Map::with_capacity(DETAIL_FIELDS.len());
    body.insert("id".to_owned(), Value::String(row.id.to_owned()));
    body.insert("status".to_owned(), Value::from(row.status));
    body.insert(
        "duplicate_to".to_owned(),
        row.duplicate_to
            .map_or(Value::Null, |id| Value::String(id.to_owned())),
    );
    body.insert(
        "snoozed_till".to_owned(),
        row.snoozed_till
            .map_or(Value::Null, |ts| Value::String(ts.to_owned())),
    );
    // `source="duplicate_to"` (`intake.py:95`): the detail renders the
    // `duplicate_to` relation itself, so it is null exactly when the FK is
    // null — never a caller-supplied object for a null FK.
    let duplicate_issue_detail = match row.duplicate_to {
        None => Value::Null,
        Some(_) => row.duplicate_issue_detail.clone().unwrap_or(Value::Null),
    };
    body.insert("duplicate_issue_detail".to_owned(), duplicate_issue_detail);
    body.insert(
        "source".to_owned(),
        row.source
            .map_or(Value::Null, |s| Value::String(s.to_owned())),
    );
    body.insert("issue".to_owned(), row.issue.clone());
    Value::Object(body)
}

// ---------------------------------------------------------------------------
// IntakeIssueLiteSerializer (intake.py:120-124)
// ---------------------------------------------------------------------------

/// `Meta.fields` (`intake.py:123`), the render order.
pub const LITE_FIELDS: &[&str] = &["id", "status", "duplicate_to", "snoozed_till", "source"];

/// `Meta.read_only_fields = fields` (`intake.py:124`): every field is
/// read-only, so this serializer never validates input.
pub const LITE_READ_ONLY_FIELDS: &[&str] = LITE_FIELDS;

/// One `IntakeIssue` row for the lite shape. Same pass-through conventions
/// as [`DetailRow`].
pub struct LiteRow<'a> {
    pub id: &'a str,
    pub status: i64,
    pub duplicate_to: Option<&'a str>,
    pub snoozed_till: Option<&'a str>,
    pub source: Option<&'a str>,
}

/// Renders a lite body in `LITE_FIELDS` order.
pub fn render_lite(row: &LiteRow<'_>) -> Value {
    let mut body = Map::with_capacity(LITE_FIELDS.len());
    body.insert("id".to_owned(), Value::String(row.id.to_owned()));
    body.insert("status".to_owned(), Value::from(row.status));
    body.insert(
        "duplicate_to".to_owned(),
        row.duplicate_to
            .map_or(Value::Null, |id| Value::String(id.to_owned())),
    );
    body.insert(
        "snoozed_till".to_owned(),
        row.snoozed_till
            .map_or(Value::Null, |ts| Value::String(ts.to_owned())),
    );
    body.insert(
        "source".to_owned(),
        row.source
            .map_or(Value::Null, |s| Value::String(s.to_owned())),
    );
    Value::Object(body)
}

// ---------------------------------------------------------------------------
// IssueStateIntakeSerializer (intake.py:127-139)
// ---------------------------------------------------------------------------

/// Declared nested fields in source order (`intake.py:128-133`):
/// (field name, source attribute, serializer type). The `many=True` shapes
/// render arrays; the single-source shapes render one object each. All
/// serializers below are owned by other domains; referenced only.
pub const STATE_DECLARED: &[(&str, &str, &str)] = &[
    ("state_detail", "state", "StateLiteSerializer"),
    ("project_detail", "project", "ProjectLiteSerializer"),
    ("label_details", "labels", "LabelLiteSerializer"),
    ("assignee_details", "assignees", "UserLiteSerializer"),
];

/// Annotated integer field (`intake.py:132`): the query layer supplies the
/// counted value; the serializer only renders it.
pub const STATE_ANNOTATED_COUNT: &str = "sub_issues_count";

/// Reverse-FK nested field (`intake.py:133`): `issue_intake` renders the
/// related `IntakeIssue` rows through `IntakeIssueLiteSerializer`
/// (`many=True`).
pub const STATE_REVERSE_NESTED: &str = "issue_intake";
/// Element shape of [`STATE_REVERSE_NESTED`].
pub const STATE_REVERSE_NESTED_TYPE: &str = "IntakeIssueLiteSerializer";

/// `Meta.exclude` (`intake.py:136-139`): the agent workpad is never part of
/// an intake payload — it is served only by the dedicated workpad endpoint.
pub const STATE_EXCLUDED: &[&str] = &["workpad"];

/// Rejects a response body that carries the excluded key. The Rust render
/// path is typed, so `workpad` can only leak in through an untyped merge;
/// every such merge must pass through this guard before the body leaves
/// the intake domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("workpad is never part of an intake payload")]
pub struct WorkpadLeak;

pub fn reject_workpad_key(body: &Map<String, Value>) -> Result<(), WorkpadLeak> {
    if body.contains_key("workpad") {
        return Err(WorkpadLeak);
    }
    Ok(())
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

    fn fixture(name: &str) -> Value {
        let path = format!(
            "{}/../../fixtures/app_intake/serializers/{name}.golden.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(&path).expect("golden exists"))
            .expect("golden parses")
    }

    /// Canonical form: objects with recursively sorted keys. `Value` key
    /// order follows workspace feature unification (`preserve_order`), so
    /// raw `to_string` is only comparable at fixed insertion order; sorting
    /// first makes the equality order-insensitive.
    fn canonical(value: &Value) -> Value {
        match value {
            Value::Object(map) => {
                let mut keys: Vec<&String> = map.keys().collect();
                keys.sort();
                let mut sorted = Map::new();
                for key in keys {
                    sorted.insert(key.clone(), canonical(&map[key]));
                }
                Value::Object(sorted)
            }
            Value::Array(items) => Value::Array(items.iter().map(canonical).collect()),
            _ => value.clone(),
        }
    }

    /// Byte-identical replay: same content in canonical form plus the exact
    /// DRF key order from the fixture's `key_order`.
    fn assert_replay(produced: &Value, expected: &Value, key_order: &[&str]) {
        assert_eq!(
            serde_json::to_string(&canonical(produced)).expect("serializes"),
            serde_json::to_string(&canonical(expected)).expect("serializes"),
            "byte-identical replay mismatch"
        );
        let order: Vec<&str> = produced
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(order, key_order, "DRF key-order mismatch");
    }

    fn str_list(value: &Value) -> Vec<&str> {
        value
            .as_array()
            .expect("string array")
            .iter()
            .map(|item| item.as_str().expect("string item"))
            .collect()
    }

    #[test]
    fn detail_meta_matches_golden() {
        // Fixture serializers/intake_issue_detail.golden.json: Meta.fields,
        // read_only_fields, and both nested descriptors.
        let golden = fixture("intake_issue_detail");
        let meta = golden.get("meta").expect("meta");
        assert_eq!(
            DETAIL_FIELDS,
            str_list(meta.get("fields").expect("fields")).as_slice()
        );
        assert_eq!(
            DETAIL_READ_ONLY_FIELDS,
            str_list(meta.get("read_only_fields").expect("read_only_fields")).as_slice()
        );
        let nested = golden
            .get("nested")
            .expect("nested")
            .as_array()
            .expect("array");
        assert_eq!(nested.len(), 2);
        assert_eq!(nested[0].get("name").expect("name"), "issue");
        assert_eq!(
            nested[0].get("type").expect("type"),
            DETAIL_NESTED_ISSUE_TYPE
        );
        assert_eq!(
            nested[1].get("name").expect("name"),
            "duplicate_issue_detail"
        );
        assert_eq!(
            nested[1].get("type").expect("type"),
            DETAIL_NESTED_DUPLICATE_TYPE
        );
        assert_eq!(
            nested[1].get("source").expect("source"),
            DETAIL_DUPLICATE_SOURCE
        );
    }

    #[test]
    fn detail_replays_golden() {
        // Fixture example_output: annotated ids already copied onto the
        // nested issue; duplicate_to null so the duplicate detail is null.
        let golden = fixture("intake_issue_detail");
        let expected = golden.get("example_output").expect("example_output");
        let row = DetailRow {
            id: "55555555-5555-4555-8555-555555555555",
            status: -2,
            duplicate_to: None,
            snoozed_till: None,
            duplicate_issue_detail: None,
            source: Some("IN_APP"),
            issue: expected.get("issue").expect("issue").clone(),
        };
        let produced = render_detail(&row);
        let key_order = str_list(golden.get("key_order").expect("key_order"));
        assert_replay(&produced, expected, &key_order);
    }

    #[test]
    fn detail_duplicate_detail_null_exactly_when_fk_null() {
        // intake.py:95 `source="duplicate_to"`: the detail renders the
        // relation itself, so a null FK always renders null — even if a
        // caller supplies a detail object for it.
        let issue = Value::Object(Map::new());
        let stray = Value::String("stray".to_owned());
        let row = DetailRow {
            id: "55555555-5555-4555-8555-555555555555",
            status: -2,
            duplicate_to: None,
            snoozed_till: None,
            duplicate_issue_detail: Some(stray),
            source: Some("IN_APP"),
            issue: issue.clone(),
        };
        let produced = render_detail(&row);
        assert_eq!(produced.get("duplicate_issue_detail"), Some(&Value::Null));

        // A set FK renders the supplied detail object untouched.
        let detail = serde_json::json!({"id": "1"});
        let row = DetailRow {
            id: "55555555-5555-4555-8555-555555555555",
            status: 2,
            duplicate_to: Some("11111111-1111-4111-8111-111111111111"),
            snoozed_till: None,
            duplicate_issue_detail: Some(detail.clone()),
            source: Some("IN_APP"),
            issue,
        };
        let produced = render_detail(&row);
        assert_eq!(produced.get("duplicate_issue_detail"), Some(&detail));
    }

    #[test]
    fn detail_annotations_copied_only_when_present() {
        // intake.py:110-117: each hasattr guard is independent — a present
        // annotation is copied, an absent one leaves the object untouched
        // (never defaulted to an empty list).
        let assignee_ids = Value::Array(vec![]);
        let label_ids = Value::Array(vec![Value::String(
            "77777777-7777-4777-8777-777777777777".to_owned(),
        )]);

        let mut bare = Map::new();
        bare.insert("id".to_owned(), Value::String("x".to_owned()));
        let mut untouched = bare.clone();
        apply_detail_annotations(
            &mut untouched,
            DetailAnnotations {
                assignee_ids: None,
                label_ids: None,
            },
        );
        assert_eq!(Value::Object(untouched), Value::Object(bare.clone()));

        // One side present never implies the other.
        let mut half = bare.clone();
        apply_detail_annotations(
            &mut half,
            DetailAnnotations {
                assignee_ids: None,
                label_ids: Some(&label_ids),
            },
        );
        assert!(!half.contains_key("assignee_ids"));
        assert_eq!(half.get("label_ids"), Some(&label_ids));

        let mut full = bare;
        apply_detail_annotations(
            &mut full,
            DetailAnnotations {
                assignee_ids: Some(&assignee_ids),
                label_ids: Some(&label_ids),
            },
        );
        assert_eq!(full.get("assignee_ids"), Some(&assignee_ids));
        assert_eq!(full.get("label_ids"), Some(&label_ids));
    }

    #[test]
    fn lite_meta_matches_golden() {
        // Fixture serializers/intake_issue_lite.golden.json: five fields,
        // all read-only.
        let golden = fixture("intake_issue_lite");
        let meta = golden.get("meta").expect("meta");
        assert_eq!(
            LITE_FIELDS,
            str_list(meta.get("fields").expect("fields")).as_slice()
        );
        assert_eq!(
            LITE_READ_ONLY_FIELDS,
            str_list(meta.get("read_only_fields").expect("read_only_fields")).as_slice()
        );
        assert_eq!(LITE_READ_ONLY_FIELDS, LITE_FIELDS);
        assert_eq!(
            golden
                .get("used_by")
                .expect("used_by")
                .as_str()
                .expect("str"),
            "IssueStateIntakeSerializer.issue_intake (many=True), app/serializers/intake.py:133",
        );
    }

    #[test]
    fn lite_replays_golden() {
        // Fixture example_output (SNOOZED=0 row, nulls for the unset FK and
        // datetime).
        let golden = fixture("intake_issue_lite");
        let expected = golden.get("example_output").expect("example_output");
        let row = LiteRow {
            id: "55555555-5555-4555-8555-555555555555",
            status: 0,
            duplicate_to: None,
            snoozed_till: None,
            source: Some("IN_APP"),
        };
        let produced = render_lite(&row);
        let key_order = str_list(golden.get("key_order").expect("key_order"));
        assert_replay(&produced, expected, &key_order);
    }

    #[test]
    fn state_meta_matches_golden() {
        // Fixture serializers/issue_state_intake.golden.json: declared
        // fields in source order, excluded workpad with its trace.
        let golden = fixture("issue_state_intake");
        let declared = golden
            .get("declared")
            .expect("declared")
            .as_array()
            .expect("array");
        // Six declared entries in source order: the four nested details,
        // then the annotated count, then the reverse-FK nested list.
        let expected_names = [
            "state_detail",
            "project_detail",
            "label_details",
            "assignee_details",
            STATE_ANNOTATED_COUNT,
            STATE_REVERSE_NESTED,
        ];
        assert_eq!(declared.len(), expected_names.len());
        for (entry, name) in declared.iter().zip(expected_names.iter()) {
            assert_eq!(
                entry.get("name").expect("name").as_str().expect("str"),
                *name
            );
        }
        for (entry, (name, source, kind)) in declared.iter().zip(STATE_DECLARED.iter()) {
            assert_eq!(
                entry.get("name").expect("name").as_str().expect("str"),
                *name
            );
            assert_eq!(
                entry.get("source").expect("source").as_str().expect("str"),
                *source
            );
            let rendered = entry.get("type").expect("type").as_str().expect("str");
            assert!(
                rendered == *kind || rendered == format!("{kind} (many)"),
                "type mismatch for {name}: {rendered}"
            );
        }
        assert_eq!(
            declared[4]
                .get("type")
                .expect("type")
                .as_str()
                .expect("str"),
            "IntegerField (annotated)"
        );
        assert_eq!(
            declared[5]
                .get("type")
                .expect("type")
                .as_str()
                .expect("str"),
            "IntakeIssueLiteSerializer (many)"
        );
        assert_eq!(STATE_REVERSE_NESTED_TYPE, "IntakeIssueLiteSerializer");
        let meta = golden.get("meta").expect("meta");
        assert_eq!(
            STATE_EXCLUDED,
            str_list(meta.get("excluded").expect("excluded")).as_slice()
        );
        assert_eq!(
            golden
                .get("workpad_exclusion")
                .expect("workpad_exclusion")
                .get("trace")
                .expect("trace")
                .as_str()
                .expect("str"),
            "app/serializers/intake.py:136-139",
        );
    }

    #[test]
    fn state_workpad_key_fails() {
        // Done-when: a response containing a workpad key fails — the agent
        // workpad is never part of an intake payload (intake.py:136-139).
        let mut clean = Map::new();
        clean.insert("id".to_owned(), Value::String("x".to_owned()));
        clean.insert(STATE_ANNOTATED_COUNT.to_owned(), Value::from(0));
        assert_eq!(reject_workpad_key(&clean), Ok(()));

        let mut leaked = clean;
        leaked.insert(
            "workpad".to_owned(),
            Value::String("agent notes".to_owned()),
        );
        assert_eq!(reject_workpad_key(&leaked), Err(WorkpadLeak));
        assert_eq!(
            WorkpadLeak.to_string(),
            "workpad is never part of an intake payload"
        );
    }
}
