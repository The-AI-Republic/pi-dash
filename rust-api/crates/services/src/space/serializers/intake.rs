//! Space intake serializers: intake-issue and inbox shapes.
//!
//! Port of `apps/api/pi_dash/space/serializer/intake.py`:
//!
//! * `intake.py:17-24` (`IntakeIssueSerializer`, `fields = "__all__"` +
//!   `issue_detail`/`project_detail` nests)
//! * `intake.py:27-31` (`IntakeIssueLiteSerializer`,
//!   `[id, status, duplicate_to, snoozed_till, source]`)
//! * `intake.py:34-47` (`IssueStateIntakeSerializer`,
//!   `Meta.exclude = ["workpad"]` + inbox nests)
//!
//! These are pure output shapes: each `to_representation` takes a row borrowed
//! from the caller and returns a `serde::Serialize` view whose fields are the
//! live DRF wire fields in live-DRF order: `[pk] + declared(base-first) +
//! concrete columns + forward relations` (`ModelSerializer.
//! get_default_field_names`, DRF 3.15.2) — every FK and M2M trails after the
//! last concrete column. UUID and FK primary keys render as strings
//! (`PrimaryKeyRelatedField`, read-only); a null FK renders `null`. Datetimes
//! and dates cross this boundary already rendered as DRF `iso-8601` strings —
//! formatting owns to the DB edge, so rendering here is a byte-exact
//! passthrough.
//!
//! The flat issue nest (`IssueFlatSerializer`, `space/serializer/issue.py:
//! 204-220`) is owned canonically by PIDASHCONV-165 (issue graph); the shape
//! below is that serializer's exact 10-key contract, kept here because
//! `IntakeIssueSerializer.issue_detail` needs it now.
//!
//! Shape-only no-ops preserved as documentation, not code:
//! `read_only_fields` (`intake.py:24`) constrain writes, of which this port
//! has none. `IntakeIssueSerializer` carries no `validate`/`update`/
//! `to_representation` override (`intake.py:17-24`) — contrast the app twin,
//! which validates accept-against-triage-default, moves state on accept and
//! stamps label ids (`app/serializers/intake.py:43-90`).
//!
//! No ported bugs in these three serializers: straight field mappings with no
//! custom write path.

use serde::Serialize;

use super::lite::{ProjectLiteView, StateLiteView, UserLiteView};
use super::taxonomy::LabelLiteView;

/// The `IntakeIssue` `fields = "__all__"` wire keys (`intake.py:17-24`), in
/// live-DRF order (probed `IntakeIssueSerializer().fields` minus the two
/// declared nests): `id`, the concrete columns (`created_at`, `updated_at`,
/// `deleted_at`, `status`, `snoozed_till`, `source`, `source_email`,
/// `external_source`, `external_id`, `extra`), then the forward relations
/// trailing (`created_by`, `updated_by`, `project`, `workspace`, `intake`,
/// `issue`, `duplicate_to`).
pub const INTAKE_ISSUE_ALL_FIELDS: [&str; 18] = [
    "id",
    "created_at",
    "updated_at",
    "deleted_at",
    "status",
    "snoozed_till",
    "source",
    "source_email",
    "external_source",
    "external_id",
    "extra",
    "created_by",
    "updated_by",
    "project",
    "workspace",
    "intake",
    "issue",
    "duplicate_to",
];

/// `IssueFlatSerializer` field list (`space/serializer/issue.py:204-220`):
/// flat issue columns only — notably no `complexity_score` (the app twin
/// adds it). Canonical owner: PIDASHCONV-165.
pub const ISSUE_FLAT_FIELDS: [&str; 10] = [
    "id",
    "name",
    "description_json",
    "description_html",
    "priority",
    "start_date",
    "target_date",
    "sequence_id",
    "sort_order",
    "is_draft",
];

/// A database row for the flat issue nest. Dates are pre-rendered strings;
/// `description_json` is a borrowed JSON value.
#[derive(Debug, Clone, PartialEq)]
pub struct IssueFlatRow<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub description_json: &'a serde_json::Value,
    pub description_html: &'a str,
    pub priority: &'a str,
    pub start_date: Option<&'a str>,
    pub target_date: Option<&'a str>,
    pub sequence_id: i32,
    pub sort_order: f64,
    pub is_draft: bool,
}

/// `IssueFlatSerializer.to_representation` output
/// (`space/serializer/issue.py:204-220`), in `Meta.fields` order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct IssueFlatView<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub description_json: &'a serde_json::Value,
    pub description_html: &'a str,
    pub priority: &'a str,
    pub start_date: Option<&'a str>,
    pub target_date: Option<&'a str>,
    pub sequence_id: i32,
    pub sort_order: f64,
    pub is_draft: bool,
}

/// Port of `IssueFlatSerializer` (`space/serializer/issue.py:204-220`).
pub fn issue_flat_to_representation<'a>(row: &'a IssueFlatRow<'a>) -> IssueFlatView<'a> {
    IssueFlatView {
        id: row.id,
        name: row.name,
        description_json: row.description_json,
        description_html: row.description_html,
        priority: row.priority,
        start_date: row.start_date,
        target_date: row.target_date,
        sequence_id: row.sequence_id,
        sort_order: row.sort_order,
        is_draft: row.is_draft,
    }
}

/// A database row for `IntakeIssue` rendering. `intake` and `issue` are
/// required FKs (`intake.py:51-52`); `duplicate_to` is a nullable FK
/// (`intake.py:64-69`); `status` is the `-2..2` intake status
/// (`intake.py:42-62`); `snoozed_till` is nullable (`intake.py:63`).
#[derive(Debug, Clone, PartialEq)]
pub struct IntakeIssueRow<'a> {
    pub id: &'a str,
    pub issue_detail: IssueFlatView<'a>,
    pub project_detail: ProjectLiteView<'a>,
    pub created_at: Option<&'a str>,
    pub updated_at: Option<&'a str>,
    pub deleted_at: Option<&'a str>,
    pub status: i32,
    pub snoozed_till: Option<&'a str>,
    pub source: Option<&'a str>,
    pub source_email: Option<&'a str>,
    pub external_source: Option<&'a str>,
    pub external_id: Option<&'a str>,
    pub extra: &'a serde_json::Value,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub project: &'a str,
    pub workspace: &'a str,
    pub intake: &'a str,
    pub issue: &'a str,
    pub duplicate_to: Option<&'a str>,
}

/// `IntakeIssueSerializer.to_representation` output (`intake.py:17-24`), in
/// live-DRF wire order (probed): `id`, the two declared nests (DRF `[pk] +
/// declared + fields + relations`), then the concrete columns, then the
/// trailing relations.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct IntakeIssueView<'a> {
    pub id: &'a str,
    pub issue_detail: IssueFlatView<'a>,
    pub project_detail: ProjectLiteView<'a>,
    pub created_at: Option<&'a str>,
    pub updated_at: Option<&'a str>,
    pub deleted_at: Option<&'a str>,
    pub status: i32,
    pub snoozed_till: Option<&'a str>,
    pub source: Option<&'a str>,
    pub source_email: Option<&'a str>,
    pub external_source: Option<&'a str>,
    pub external_id: Option<&'a str>,
    pub extra: &'a serde_json::Value,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub project: &'a str,
    pub workspace: &'a str,
    pub intake: &'a str,
    pub issue: &'a str,
    pub duplicate_to: Option<&'a str>,
}

/// Port of `IntakeIssueSerializer` (`intake.py:17-24`). Field-for-field copy.
pub fn intake_issue_to_representation<'a>(row: &'a IntakeIssueRow<'a>) -> IntakeIssueView<'a> {
    IntakeIssueView {
        id: row.id,
        issue_detail: row.issue_detail.clone(),
        project_detail: row.project_detail.clone(),
        created_at: row.created_at,
        updated_at: row.updated_at,
        deleted_at: row.deleted_at,
        status: row.status,
        snoozed_till: row.snoozed_till,
        source: row.source,
        source_email: row.source_email,
        external_source: row.external_source,
        external_id: row.external_id,
        extra: row.extra,
        created_by: row.created_by,
        updated_by: row.updated_by,
        project: row.project,
        workspace: row.workspace,
        intake: row.intake,
        issue: row.issue,
        duplicate_to: row.duplicate_to,
    }
}

/// A database row for `IntakeIssue` lite rendering (`intake.py:27-31`).
#[derive(Debug, Clone, PartialEq)]
pub struct IntakeIssueLiteRow<'a> {
    pub id: &'a str,
    pub status: i32,
    pub duplicate_to: Option<&'a str>,
    pub snoozed_till: Option<&'a str>,
    pub source: Option<&'a str>,
}

/// `IntakeIssueLiteSerializer.to_representation` output (`intake.py:27-31`),
/// in `Meta.fields` order. Every field is read-only (`:31`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct IntakeIssueLiteView<'a> {
    pub id: &'a str,
    pub status: i32,
    pub duplicate_to: Option<&'a str>,
    pub snoozed_till: Option<&'a str>,
    pub source: Option<&'a str>,
}

/// Port of `IntakeIssueLiteSerializer` (`intake.py:27-31`).
pub fn intake_issue_lite_to_representation<'a>(
    row: &'a IntakeIssueLiteRow<'a>,
) -> IntakeIssueLiteView<'a> {
    IntakeIssueLiteView {
        id: row.id,
        status: row.status,
        duplicate_to: row.duplicate_to,
        snoozed_till: row.snoozed_till,
        source: row.source,
    }
}

/// The `Issue` model `exclude = ["workpad"]` wire keys
/// (`db/models/issue.py:107-227` over `ProjectBaseModel`), in live-DRF
/// order (probed `IssueStateIntakeSerializer().fields` minus the declared
/// nests): `id`, the concrete columns, then the forward relations trailing
/// (`created_by`, `updated_by`, `project`, `workspace`, `parent`, `state`,
/// `estimate_point`, `type`, `assigned_pod`, the `assignees`/`labels` M2Ms).
/// `workpad` (the agent's per-issue scratchpad, `issue.py:198`) must never
/// leak into the public/guest Space app (`intake.py:45-46`), so it has no
/// view field.
pub const ISSUE_STATE_INTAKE_MODEL_FIELDS: [&str; 35] = [
    "id",
    "created_at",
    "updated_at",
    "deleted_at",
    "point",
    "name",
    "description_json",
    "description_html",
    "description_stripped",
    "description_binary",
    "priority",
    "complexity_score",
    "start_date",
    "target_date",
    "sequence_id",
    "sort_order",
    "completed_at",
    "archived_at",
    "is_draft",
    "external_source",
    "external_id",
    "git_work_branch",
    "created_via",
    "agent_executor",
    "created_by",
    "updated_by",
    "project",
    "workspace",
    "parent",
    "state",
    "estimate_point",
    "type",
    "assigned_pod",
    "assignees",
    "labels",
];

/// A database row for the inbox serializer. `sub_issues_count` is an
/// annotated read-only integer (`intake.py:39`); `bridge_id` is the
/// annotated `F(issue_intake__id)` UUID (`intake.py:40`,
/// `space/views/intake.py:72`); `issue_intake` is the reverse relation
/// rendered through `IntakeIssueLiteSerializer` (`intake.py:41`);
/// `assignees`/`labels` are the raw M2M PK lists (DRF `exclude` still
/// renders every other model field); `description_binary` (`BinaryField`,
/// `db/models/issue.py:141`) has no DRF JSON mapping, so it crosses this
/// boundary caller-resolved as an opaque string, like datetimes.
#[derive(Debug, Clone, PartialEq)]
pub struct IssueStateIntakeRow<'a> {
    pub id: &'a str,
    pub state_detail: Option<StateLiteView<'a>>,
    pub project_detail: ProjectLiteView<'a>,
    pub label_details: Vec<LabelLiteView<'a>>,
    pub assignee_details: Vec<UserLiteView<'a>>,
    pub sub_issues_count: i64,
    pub bridge_id: &'a str,
    pub issue_intake: Vec<IntakeIssueLiteView<'a>>,
    pub created_at: Option<&'a str>,
    pub updated_at: Option<&'a str>,
    pub deleted_at: Option<&'a str>,
    pub point: Option<i32>,
    pub name: &'a str,
    pub description_json: &'a serde_json::Value,
    pub description_html: &'a str,
    pub description_stripped: Option<&'a str>,
    pub description_binary: Option<&'a str>,
    pub priority: &'a str,
    pub complexity_score: i32,
    pub start_date: Option<&'a str>,
    pub target_date: Option<&'a str>,
    pub sequence_id: i32,
    pub sort_order: f64,
    pub completed_at: Option<&'a str>,
    pub archived_at: Option<&'a str>,
    pub is_draft: bool,
    pub external_source: Option<&'a str>,
    pub external_id: Option<&'a str>,
    pub git_work_branch: &'a str,
    pub created_via: Option<&'a str>,
    pub agent_executor: Option<&'a str>,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub project: &'a str,
    pub workspace: &'a str,
    pub parent: Option<&'a str>,
    pub state: Option<&'a str>,
    pub estimate_point: Option<&'a str>,
    pub r#type: Option<&'a str>,
    pub assigned_pod: Option<&'a str>,
    pub assignees: Vec<&'a str>,
    pub labels: Vec<&'a str>,
}

/// `IssueStateIntakeSerializer.to_representation` output
/// (`intake.py:34-47`), in live-DRF wire order (probed): `id`, the seven
/// declared fields (DRF `[pk] + declared + fields + relations`), then the
/// concrete `Issue` columns, then the trailing relations. `state` is
/// nullable (`issue.py:119-125`), so `state_detail` is `None` when it is —
/// DRF renders `None` for a null nest source.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct IssueStateIntakeView<'a> {
    pub id: &'a str,
    pub state_detail: Option<StateLiteView<'a>>,
    pub project_detail: ProjectLiteView<'a>,
    pub label_details: Vec<LabelLiteView<'a>>,
    pub assignee_details: Vec<UserLiteView<'a>>,
    pub sub_issues_count: i64,
    pub bridge_id: &'a str,
    pub issue_intake: Vec<IntakeIssueLiteView<'a>>,
    pub created_at: Option<&'a str>,
    pub updated_at: Option<&'a str>,
    pub deleted_at: Option<&'a str>,
    pub point: Option<i32>,
    pub name: &'a str,
    pub description_json: &'a serde_json::Value,
    pub description_html: &'a str,
    pub description_stripped: Option<&'a str>,
    pub description_binary: Option<&'a str>,
    pub priority: &'a str,
    pub complexity_score: i32,
    pub start_date: Option<&'a str>,
    pub target_date: Option<&'a str>,
    pub sequence_id: i32,
    pub sort_order: f64,
    pub completed_at: Option<&'a str>,
    pub archived_at: Option<&'a str>,
    pub is_draft: bool,
    pub external_source: Option<&'a str>,
    pub external_id: Option<&'a str>,
    pub git_work_branch: &'a str,
    pub created_via: Option<&'a str>,
    pub agent_executor: Option<&'a str>,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub project: &'a str,
    pub workspace: &'a str,
    pub parent: Option<&'a str>,
    pub state: Option<&'a str>,
    pub estimate_point: Option<&'a str>,
    pub r#type: Option<&'a str>,
    pub assigned_pod: Option<&'a str>,
    pub assignees: Vec<&'a str>,
    pub labels: Vec<&'a str>,
}

/// Port of `IssueStateIntakeSerializer` (`intake.py:34-47`).
/// Field-for-field copy; `workpad` is absent by construction.
pub fn issue_state_intake_to_representation<'a>(
    row: &'a IssueStateIntakeRow<'a>,
) -> IssueStateIntakeView<'a> {
    IssueStateIntakeView {
        id: row.id,
        state_detail: row.state_detail.clone(),
        project_detail: row.project_detail.clone(),
        label_details: row.label_details.clone(),
        assignee_details: row.assignee_details.clone(),
        sub_issues_count: row.sub_issues_count,
        bridge_id: row.bridge_id,
        issue_intake: row.issue_intake.clone(),
        created_at: row.created_at,
        updated_at: row.updated_at,
        deleted_at: row.deleted_at,
        point: row.point,
        name: row.name,
        description_json: row.description_json,
        description_html: row.description_html,
        description_stripped: row.description_stripped,
        description_binary: row.description_binary,
        priority: row.priority,
        complexity_score: row.complexity_score,
        start_date: row.start_date,
        target_date: row.target_date,
        sequence_id: row.sequence_id,
        sort_order: row.sort_order,
        completed_at: row.completed_at,
        archived_at: row.archived_at,
        is_draft: row.is_draft,
        external_source: row.external_source,
        external_id: row.external_id,
        git_work_branch: row.git_work_branch,
        created_via: row.created_via,
        agent_executor: row.agent_executor,
        created_by: row.created_by,
        updated_by: row.updated_by,
        project: row.project,
        workspace: row.workspace,
        parent: row.parent,
        state: row.state,
        estimate_point: row.estimate_point,
        r#type: row.r#type,
        assigned_pod: row.assigned_pod,
        assignees: row.assignees.clone(),
        labels: row.labels.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn intake_golden() -> Value {
        let path = format!(
            "{}/../../fixtures/space/serializers/intake.golden.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(&path).expect("golden exists"))
            .expect("golden parses")
    }

    fn case<'a>(golden: &'a Value, serializer: &str) -> &'a Value {
        golden
            .get("cases")
            .and_then(Value::as_array)
            .expect("cases array")
            .iter()
            .find(|case| case.get("serializer").and_then(Value::as_str) == Some(serializer))
            .unwrap_or_else(|| panic!("golden lacks {serializer} case"))
    }

    fn opt(obj: &Value, key: &str) -> Option<String> {
        match obj.get(key) {
            None | Some(Value::Null) => None,
            Some(Value::String(s)) => Some(s.clone()),
            Some(other) => panic!("golden key {key} is not a string/null: {other}"),
        }
    }

    /// Top-level JSON key order of a view's serialization, read off the
    /// serialized string: struct serialization always emits declaration
    /// order, while `Value` objects iterate alphabetically.
    fn serialized_keys<T: serde::Serialize>(value: &T) -> Vec<String> {
        let rendered = serde_json::to_string(value).expect("serializes");
        let mut keys = Vec::new();
        let mut depth = 0usize;
        let mut chars = rendered.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '{' => {
                    depth += 1;
                }
                '}' => {
                    depth -= 1;
                }
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

    fn const_keys<const N: usize>(fields: &[&str; N]) -> Vec<String> {
        fields.iter().map(|key| key.to_string()).collect()
    }

    /// Wire order for an `__all__`/`exclude` view: `id`, the declared
    /// nests, then the model body after `id`.
    fn wire_order<const N: usize>(nests: &[&str], body: &[&str; N]) -> Vec<String> {
        let mut expected = vec!["id".to_owned()];
        expected.extend(nests.iter().map(|key| key.to_string()));
        expected.extend(body[1..].iter().map(|key| key.to_string()));
        expected
    }

    /// Canonical form: objects with recursively sorted keys (same kernel as
    /// the lite leaves: workspace feature unification makes raw `to_string`
    /// order uncomparable).
    fn canonical(value: &Value) -> Value {
        match value {
            Value::Object(map) => {
                let mut keys: Vec<&String> = map.keys().collect();
                keys.sort();
                let mut sorted = serde_json::Map::new();
                for key in keys {
                    sorted.insert(key.clone(), canonical(&map[key]));
                }
                Value::Object(sorted)
            }
            Value::Array(items) => Value::Array(items.iter().map(canonical).collect()),
            _ => value.clone(),
        }
    }

    fn assert_replay(produced: &Value, expected: &Value) {
        assert_eq!(
            produced, expected,
            "field-for-field mismatch against golden output"
        );
        assert_eq!(
            serde_json::to_string(&canonical(produced)).expect("serializes"),
            serde_json::to_string(&canonical(expected)).expect("serializes"),
            "byte-identical replay mismatch"
        );
    }

    fn project_detail_view<'a>(id: &'a str, icon: &'a Value) -> ProjectLiteView<'a> {
        ProjectLiteView {
            id,
            identifier: "WEB",
            name: "Web",
            cover_image: None,
            icon_prop: icon,
            emoji: Some("🚀"),
            description: "Ship it",
        }
    }

    #[test]
    fn intake_lite_replays_golden() {
        // Fixture serializers/intake.golden.json: IntakeIssueLiteSerializer
        // (intake.py:27-31, keys id/status/duplicate_to/snoozed_till/source,
        // all read-only). In==out: the golden records the output object, so
        // the row is built from it and must render back byte-exact.
        let golden = intake_golden();
        let output = &case(&golden, "IntakeIssueLiteSerializer")["output"];
        let id = output.get("id").and_then(Value::as_str).expect("id");
        let status = output
            .get("status")
            .and_then(Value::as_i64)
            .expect("status");
        let duplicate_to = opt(output, "duplicate_to");
        let snoozed_till = opt(output, "snoozed_till");
        let source = opt(output, "source");
        let row = IntakeIssueLiteRow {
            id,
            status: status as i32,
            duplicate_to: duplicate_to.as_deref(),
            snoozed_till: snoozed_till.as_deref(),
            source: source.as_deref(),
        };
        let view = intake_issue_lite_to_representation(&row);
        assert_eq!(
            serialized_keys(&view),
            vec!["id", "status", "duplicate_to", "snoozed_till", "source"]
        );
        let produced = serde_json::to_value(&view).expect("serializes");
        assert_replay(&produced, output);
    }

    #[test]
    fn intake_view_carries_all_columns_plus_nests() {
        // intake.py:17-24: fields=__all__ (INTAKE_ISSUE_ALL_FIELDS, 18 keys)
        // with id first, then the declared issue_detail/project_detail
        // nests (live-DRF [pk]+declared+fields+relations, probed);
        // read_only_fields names project/workspace (:24). The golden pins
        // the output_keys/output_shape (case 1), so this pins the full
        // 20-key wire order plus the nested 10-key flat / 7-key lite.
        let icon = serde_json::json!({"color": "#fff"});
        let description = serde_json::json!({});
        let extra = serde_json::json!({});
        let row = IntakeIssueRow {
            id: "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb",
            created_at: None,
            updated_at: None,
            created_by: None,
            updated_by: None,
            deleted_at: None,
            project: "33333333-3333-3333-3333-333333333333",
            workspace: "22222222-2222-2222-2222-222222222222",
            intake: "cccccccc-cccc-cccc-cccc-cccccccccccc",
            issue: "dddddddd-dddd-dddd-dddd-dddddddddddd",
            status: -2,
            snoozed_till: None,
            duplicate_to: None,
            source: Some("IN_APP"),
            source_email: None,
            external_source: None,
            external_id: None,
            extra: &extra,
            issue_detail: IssueFlatView {
                id: "dddddddd-dddd-dddd-dddd-dddddddddddd",
                name: "Intake idea",
                description_json: &description,
                description_html: "<p>why</p>",
                priority: "low",
                start_date: None,
                target_date: None,
                sequence_id: 1,
                sort_order: 65535.0,
                is_draft: false,
            },
            project_detail: project_detail_view("33333333-3333-3333-3333-333333333333", &icon),
        };
        let view = intake_issue_to_representation(&row);
        assert_eq!(
            serialized_keys(&view),
            wire_order(
                &["issue_detail", "project_detail"],
                &INTAKE_ISSUE_ALL_FIELDS
            )
        );
        assert_eq!(
            serialized_keys(&view.issue_detail),
            const_keys(&ISSUE_FLAT_FIELDS)
        );
        let produced = serde_json::to_value(&view).expect("serializes");
        assert_eq!(produced["issue_detail"]["id"], produced["issue"]);
        assert_eq!(produced["project_detail"]["id"], produced["project"]);
    }

    #[test]
    fn issue_state_intake_excludes_workpad() {
        // intake.py:47 Meta.exclude=["workpad"]: the agent workpad must never
        // leak to guest/public Space readers. The view has no workpad field
        // by construction; this pins the full 42-key wire order and the
        // absence.
        let row = sample_issue_state_intake_row();
        let view = issue_state_intake_to_representation(&row);
        assert_eq!(
            serialized_keys(&view),
            wire_order(
                &[
                    "state_detail",
                    "project_detail",
                    "label_details",
                    "assignee_details",
                    "sub_issues_count",
                    "bridge_id",
                    "issue_intake",
                ],
                &ISSUE_STATE_INTAKE_MODEL_FIELDS
            )
        );
        let produced = serde_json::to_value(&view).expect("serializes");
        assert_eq!(produced.get("workpad"), None, "workpad MUST NOT render");
    }

    #[test]
    fn issue_state_intake_renders_bridge_count_and_details() {
        // intake.py:39-41: sub_issues_count (annotated int), bridge_id
        // (annotated F(issue_intake__id) UUID, views/intake.py:72),
        // issue_intake ([IntakeIssueLite] reverse relation); :35-38 the
        // state/project/label/assignee detail nests.
        let row = sample_issue_state_intake_row();
        let view = issue_state_intake_to_representation(&row);
        assert_eq!(
            serialized_keys(view.state_detail.as_ref().expect("state detail")),
            vec!["id", "name", "color", "group"]
        );
        assert_eq!(view.label_details.len(), 1);
        assert_eq!(
            serialized_keys(&view.label_details[0]),
            vec!["id", "name", "color"]
        );
        assert_eq!(view.assignee_details.len(), 1);
        assert_eq!(
            serialized_keys(&view.assignee_details[0]),
            vec![
                "id",
                "first_name",
                "last_name",
                "avatar",
                "avatar_url",
                "is_bot",
                "display_name"
            ]
        );
        assert_eq!(view.issue_intake.len(), 1);
        assert_eq!(
            serialized_keys(&view.issue_intake[0]),
            vec!["id", "status", "duplicate_to", "snoozed_till", "source"]
        );
        let produced = serde_json::to_value(&view).expect("serializes");
        assert_eq!(
            produced.get("bridge_id").and_then(Value::as_str),
            Some("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb")
        );
        assert_eq!(
            produced.get("sub_issues_count").and_then(Value::as_i64),
            Some(2)
        );
        // Raw M2M PK lists still render (exclude drops only workpad).
        assert_eq!(
            produced.get("assignees"),
            Some(&serde_json::json!(["11111111-1111-1111-1111-111111111111"]))
        );
        assert_eq!(
            produced.get("labels"),
            Some(&serde_json::json!(["77777777-7777-7777-7777-777777777777"]))
        );
    }

    #[test]
    fn issue_state_intake_renders_null_state_detail() {
        // `Issue.state` is nullable (`issue.py:119-125`); DRF renders
        // `None` for the null `state_detail` source instead of a lite
        // object.
        let mut row = sample_issue_state_intake_row();
        row.state = None;
        row.state_detail = None;
        let produced =
            serde_json::to_value(issue_state_intake_to_representation(&row)).expect("serializes");
        assert_eq!(produced.get("state"), Some(&Value::Null));
        assert_eq!(produced.get("state_detail"), Some(&Value::Null));
    }

    // Shared representative inbox row. The row borrows its JSON literals, so
    // the helper leaks them to 'static (test-only; production callers borrow
    // live data).
    fn sample_issue_state_intake_row() -> IssueStateIntakeRow<'static> {
        fn leak(value: Value) -> &'static Value {
            Box::leak(Box::new(value))
        }
        IssueStateIntakeRow {
            id: "dddddddd-dddd-dddd-dddd-dddddddddddd",
            created_at: None,
            updated_at: None,
            created_by: None,
            updated_by: None,
            deleted_at: None,
            project: "33333333-3333-3333-3333-333333333333",
            workspace: "22222222-2222-2222-2222-222222222222",
            parent: None,
            state: Some("44444444-4444-4444-4444-444444444444"),
            point: None,
            estimate_point: None,
            name: "Intake idea",
            description_json: leak(serde_json::json!({})),
            description_html: "<p>why</p>",
            description_stripped: None,
            description_binary: None,
            priority: "low",
            complexity_score: 0,
            start_date: None,
            target_date: None,
            assignees: vec!["11111111-1111-1111-1111-111111111111"],
            sequence_id: 1,
            labels: vec!["77777777-7777-7777-7777-777777777777"],
            sort_order: 65535.0,
            completed_at: None,
            archived_at: None,
            is_draft: false,
            external_source: None,
            external_id: None,
            r#type: None,
            git_work_branch: "",
            created_via: None,
            assigned_pod: None,
            agent_executor: None,
            state_detail: Some(StateLiteView {
                id: "44444444-4444-4444-4444-444444444444",
                name: "In Progress",
                color: "#ff0000",
                group: "started",
            }),
            project_detail: ProjectLiteView {
                id: "33333333-3333-3333-3333-333333333333",
                identifier: "WEB",
                name: "Web",
                cover_image: None,
                icon_prop: leak(serde_json::json!({"color": "#fff"})),
                emoji: Some("🚀"),
                description: "Ship it",
            },
            label_details: vec![LabelLiteView {
                id: "77777777-7777-7777-7777-777777777777",
                name: "Bug",
                color: "#ff0000",
            }],
            assignee_details: vec![UserLiteView {
                id: "11111111-1111-1111-1111-111111111111",
                first_name: "Ada",
                last_name: "L",
                avatar: "",
                avatar_url: None,
                is_bot: false,
                display_name: "Ada L",
            }],
            sub_issues_count: 2,
            bridge_id: "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb",
            issue_intake: vec![IntakeIssueLiteView {
                id: "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb",
                status: -2,
                duplicate_to: None,
                snoozed_till: None,
                source: Some("IN_APP"),
            }],
        }
    }
}
