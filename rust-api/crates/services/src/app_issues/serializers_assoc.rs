//! App issue assoc/version serializers: assignees, labels, user properties, versions.
//!
//! Port of five classes in `apps/api/pi_dash/app/serializers/issue.py`:
//!
//! * `IssueAssigneeSerializer` (`:670-677`, `fields = "__all__"` + nest)
//! * `IssueLabelSerializer` (`:583-589`, `fields = "__all__"`)
//! * `ProjectUserPropertySerializer` (`:542-548`, `fields = "__all__"`)
//! * `IssueVersionDetailSerializer` (`:1448-1486`, explicit 31-entry list)
//! * `IssueDescriptionVersionDetailSerializer` (`:1487-1506`, explicit list)
//!
//! These are pure output shapes: each `to_representation` takes a row borrowed
//! from the caller and returns a `serde::Serialize` view whose fields are the
//! live DRF wire fields in DRF order. UUID and FK primary keys render as strings
//! (`PrimaryKeyRelatedField`, read-only); a null FK renders `null`. Datetimes
//! and dates cross this boundary already rendered as DRF `iso-8601` strings —
//! formatting owns to the DB edge, so rendering here is a byte-exact
//! passthrough. (The version detail views return `serializer.data` directly,
//! `app/views/issue/version.py:43,115`; only the paginated list path applies
//! `user_timezone_converter`.)
//!
//! The `__all__` wire order is DRF's, not model-definition order:
//! `[pk] + declared fields + non-relational columns + FK columns`
//! (`ModelSerializer.get_default_field_names`, DRF `serializers.py`; verified on
//! the pinned Django 4.2.30 + DRF 3.15.2). So `deleted_at` sorts with the plain
//! columns ahead of the `created_by`/`updated_by` FKs, the declared
//! `assignee_details` nest sits second behind `id`, and the user-property JSON
//! columns precede every FK. The `*_FIELDS` consts pin that exact order.
//!
//! Shape-only no-ops preserved as documentation, not code:
//! `read_only_fields` (`issue.py:546`, `:587`, `:1484`, `:1506`) constrain
//! writes, of which this port has none. `IssueAssigneeSerializer` declares no
//! `read_only_fields` at all (`:670-677`) — writable through default
//! `ModelSerializer` behavior, though no D-26 view writes through it (m2m rows
//! are managed by `IssueCreateSerializer`). The version detail list omits the
//! `properties` and `activity` model columns (`db/models/issue.py:836,841-846`)
//! — ported as the same omission.
//!
//! Not ported here: `LabelSerializer` / `LabelLiteSerializer`
//! (`issue.py:549-581`) stay owned by the merged space taxonomy port
//! (`crate::space::serializers::taxonomy`); no D-26 serializer edge nests
//! them. `assignee_details` reuses the merged
//! [`user_lite_to_representation`](crate::space::serializers::lite::user_lite_to_representation).
//!
//! Ported bugs (translate, don't redesign):
//!
//! * BUG-dup-name (`issue.py:1459` + `:1476`): `name` is listed twice in
//!   `IssueVersionDetailSerializer.Meta.fields`. DRF builds `fields` as a dict,
//!   so the wire object carries ONE `name` key at the FIRST position (verified
//!   on the pinned DRF 3.15.2). [`ISSUE_VERSION_DETAIL_FIELDS`] carries `name`
//!   once, at index 7.
//!
//! No other ported bugs: the five classes declare no custom
//! `create`/`update`/`to_representation`/`validate`, so the port is straight
//! field mappings plus the `description_binary` base64 rule below.

use base64::Engine as _;
use serde::Serialize;

use crate::space::serializers::lite::{user_lite_to_representation, UserLiteRow, UserLiteView};

/// The `IssueAssigneeSerializer` `fields = "__all__"` key set
/// (`issue.py:670-677`) in DRF wire order: `id` (`BaseModel`,
/// `db/models/base.py:17-18`), the declared `assignee_details` nest
/// (`UserLiteSerializer(source="assignee")`, `:671`), the non-relational
/// audit columns (`created_at`, `updated_at`, `deleted_at`,
/// `db/mixins.py:16-70`), then the FK columns in definition order
/// (`created_by`, `updated_by`, `project`, `workspace`,
/// `db/models/project.py:302-304`, `issue`, `assignee`,
/// `db/models/issue.py:446-451`).
pub const ISSUE_ASSIGNEE_FIELDS: [&str; 11] = [
    "id",
    "assignee_details",
    "created_at",
    "updated_at",
    "deleted_at",
    "created_by",
    "updated_by",
    "project",
    "workspace",
    "issue",
    "assignee",
];

/// A database row for `IssueAssignee` rendering. The `assignee` FK is required
/// (`db/models/issue.py:447-451`), so `assignee_details` always renders an
/// object, never `null`.
#[derive(Debug, Clone, PartialEq)]
pub struct IssueAssigneeRow<'a> {
    pub id: &'a str,
    pub assignee_details: &'a UserLiteRow<'a>,
    pub created_at: Option<&'a str>,
    pub updated_at: Option<&'a str>,
    pub deleted_at: Option<&'a str>,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub project: &'a str,
    pub workspace: &'a str,
    pub issue: &'a str,
    pub assignee: &'a str,
}

/// `IssueAssigneeSerializer.to_representation` output (`issue.py:670-677`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct IssueAssigneeView<'a> {
    pub id: &'a str,
    pub assignee_details: UserLiteView<'a>,
    pub created_at: Option<&'a str>,
    pub updated_at: Option<&'a str>,
    pub deleted_at: Option<&'a str>,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub project: &'a str,
    pub workspace: &'a str,
    pub issue: &'a str,
    pub assignee: &'a str,
}

/// Port of `IssueAssigneeSerializer` (`issue.py:670-677`). Field-for-field copy.
pub fn issue_assignee_to_representation<'a>(
    row: &'a IssueAssigneeRow<'a>,
) -> IssueAssigneeView<'a> {
    IssueAssigneeView {
        id: row.id,
        assignee_details: user_lite_to_representation(row.assignee_details),
        created_at: row.created_at,
        updated_at: row.updated_at,
        deleted_at: row.deleted_at,
        created_by: row.created_by,
        updated_by: row.updated_by,
        project: row.project,
        workspace: row.workspace,
        issue: row.issue,
        assignee: row.assignee,
    }
}

/// The `IssueLabelSerializer` `fields = "__all__"` key set
/// (`issue.py:583-589`) in DRF wire order: `id`, the non-relational audit
/// columns, then the FK columns in definition order (`created_by`,
/// `updated_by`, `project`, `workspace`, `issue`, `label`,
/// `db/models/issue.py:669-671`).
pub const ISSUE_LABEL_FIELDS: [&str; 10] = [
    "id",
    "created_at",
    "updated_at",
    "deleted_at",
    "created_by",
    "updated_by",
    "project",
    "workspace",
    "issue",
    "label",
];

/// A database row for `IssueLabel` rendering.
#[derive(Debug, Clone, PartialEq)]
pub struct IssueLabelRow<'a> {
    pub id: &'a str,
    pub created_at: Option<&'a str>,
    pub updated_at: Option<&'a str>,
    pub deleted_at: Option<&'a str>,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub project: &'a str,
    pub workspace: &'a str,
    pub issue: &'a str,
    pub label: &'a str,
}

/// `IssueLabelSerializer.to_representation` output (`issue.py:583-589`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct IssueLabelView<'a> {
    pub id: &'a str,
    pub created_at: Option<&'a str>,
    pub updated_at: Option<&'a str>,
    pub deleted_at: Option<&'a str>,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub project: &'a str,
    pub workspace: &'a str,
    pub issue: &'a str,
    pub label: &'a str,
}

/// Port of `IssueLabelSerializer` (`issue.py:583-589`). Field-for-field copy.
pub fn issue_label_to_representation<'a>(row: &'a IssueLabelRow<'a>) -> IssueLabelView<'a> {
    IssueLabelView {
        id: row.id,
        created_at: row.created_at,
        updated_at: row.updated_at,
        deleted_at: row.deleted_at,
        created_by: row.created_by,
        updated_by: row.updated_by,
        project: row.project,
        workspace: row.workspace,
        issue: row.issue,
        label: row.label,
    }
}

/// The `ProjectUserPropertySerializer` `fields = "__all__"` key set
/// (`issue.py:542-548`) in DRF wire order: `id`, the non-relational columns
/// (`created_at`, `updated_at`, `deleted_at`, then the JSON columns `filters`,
/// `display_filters`, `display_properties`, `rich_filters`, `preferences` and
/// the `sort_order` float, `db/models/project.py:472-477`), then the FK columns
/// in definition order (`created_by`, `updated_by`, `project`, `workspace`,
/// `user`, `db/models/project.py:467-471`).
pub const PROJECT_USER_PROPERTY_FIELDS: [&str; 15] = [
    "id",
    "created_at",
    "updated_at",
    "deleted_at",
    "filters",
    "display_filters",
    "display_properties",
    "rich_filters",
    "preferences",
    "sort_order",
    "created_by",
    "updated_by",
    "project",
    "workspace",
    "user",
];

/// A database row for `ProjectUserProperty` rendering. The JSON columns carry
/// `default=` callables (`db/models/project.py:472-476`), so they always render
/// objects, never `null`; `sort_order` is a required float (`:477`).
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectUserPropertyRow<'a> {
    pub id: &'a str,
    pub created_at: Option<&'a str>,
    pub updated_at: Option<&'a str>,
    pub deleted_at: Option<&'a str>,
    pub filters: &'a serde_json::Value,
    pub display_filters: &'a serde_json::Value,
    pub display_properties: &'a serde_json::Value,
    pub rich_filters: &'a serde_json::Value,
    pub preferences: &'a serde_json::Value,
    pub sort_order: f64,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub project: &'a str,
    pub workspace: &'a str,
    pub user: &'a str,
}

/// `ProjectUserPropertySerializer.to_representation` output (`issue.py:542-548`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProjectUserPropertyView<'a> {
    pub id: &'a str,
    pub created_at: Option<&'a str>,
    pub updated_at: Option<&'a str>,
    pub deleted_at: Option<&'a str>,
    pub filters: &'a serde_json::Value,
    pub display_filters: &'a serde_json::Value,
    pub display_properties: &'a serde_json::Value,
    pub rich_filters: &'a serde_json::Value,
    pub preferences: &'a serde_json::Value,
    pub sort_order: f64,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub project: &'a str,
    pub workspace: &'a str,
    pub user: &'a str,
}

/// Port of `ProjectUserPropertySerializer` (`issue.py:542-548`). Field-for-field
/// copy. Whole floats render with `.0` on both sides (`65535.0`), so `f64`
/// needs no custom rendering.
pub fn project_user_property_to_representation<'a>(
    row: &'a ProjectUserPropertyRow<'a>,
) -> ProjectUserPropertyView<'a> {
    ProjectUserPropertyView {
        id: row.id,
        created_at: row.created_at,
        updated_at: row.updated_at,
        deleted_at: row.deleted_at,
        filters: row.filters,
        display_filters: row.display_filters,
        display_properties: row.display_properties,
        rich_filters: row.rich_filters,
        preferences: row.preferences,
        sort_order: row.sort_order,
        created_by: row.created_by,
        updated_by: row.updated_by,
        project: row.project,
        workspace: row.workspace,
        user: row.user,
    }
}

/// The `IssueVersionDetailSerializer` key set (`issue.py:1448-1486`) in
/// `Meta.fields` order, with BUG-dup-name collapsed: the second `name`
/// (`:1476`) is dropped, the first (`:1459`) keeps its position, leaving 30
/// keys. `parent`, `state`, `estimate_point`, `type` and `cycle` are plain
/// nullable UUID columns (`db/models/issue.py:812-814,833-834`), not FKs;
/// `assignees`, `labels` and `modules` are UUID arrays (`:824,826,835`).
pub const ISSUE_VERSION_DETAIL_FIELDS: [&str; 30] = [
    "id",
    "workspace",
    "project",
    "issue",
    "parent",
    "state",
    "estimate_point",
    "name",
    "priority",
    "start_date",
    "target_date",
    "assignees",
    "sequence_id",
    "labels",
    "sort_order",
    "completed_at",
    "archived_at",
    "is_draft",
    "external_source",
    "external_id",
    "type",
    "cycle",
    "modules",
    "meta",
    "last_saved_at",
    "owned_by",
    "created_at",
    "updated_at",
    "created_by",
    "updated_by",
];

/// A database row for `IssueVersion` detail rendering. The UUID arrays default
/// to empty (`blank=True, default=list`) and are never `null`; `meta` defaults
/// to `{}` (`db/models/issue.py:837`); `name` is required (`:815`), `priority`
/// defaults to `"none"` (`:816-821`), `last_saved_at` defaults to now (`:838`),
/// and `owned_by` is a required FK (`:847-851`).
#[derive(Debug, Clone, PartialEq)]
pub struct IssueVersionDetailRow<'a> {
    pub id: &'a str,
    pub workspace: &'a str,
    pub project: &'a str,
    pub issue: &'a str,
    pub parent: Option<&'a str>,
    pub state: Option<&'a str>,
    pub estimate_point: Option<&'a str>,
    pub name: &'a str,
    pub priority: &'a str,
    pub start_date: Option<&'a str>,
    pub target_date: Option<&'a str>,
    pub assignees: &'a [&'a str],
    pub sequence_id: i32,
    pub labels: &'a [&'a str],
    pub sort_order: f64,
    pub completed_at: Option<&'a str>,
    pub archived_at: Option<&'a str>,
    pub is_draft: bool,
    pub external_source: Option<&'a str>,
    pub external_id: Option<&'a str>,
    pub r#type: Option<&'a str>,
    pub cycle: Option<&'a str>,
    pub modules: &'a [&'a str],
    pub meta: &'a serde_json::Value,
    pub last_saved_at: &'a str,
    pub owned_by: &'a str,
    pub created_at: Option<&'a str>,
    pub updated_at: Option<&'a str>,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
}

/// `IssueVersionDetailSerializer.to_representation` output (`issue.py:1448-1486`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct IssueVersionDetailView<'a> {
    pub id: &'a str,
    pub workspace: &'a str,
    pub project: &'a str,
    pub issue: &'a str,
    pub parent: Option<&'a str>,
    pub state: Option<&'a str>,
    pub estimate_point: Option<&'a str>,
    pub name: &'a str,
    pub priority: &'a str,
    pub start_date: Option<&'a str>,
    pub target_date: Option<&'a str>,
    pub assignees: &'a [&'a str],
    pub sequence_id: i32,
    pub labels: &'a [&'a str],
    pub sort_order: f64,
    pub completed_at: Option<&'a str>,
    pub archived_at: Option<&'a str>,
    pub is_draft: bool,
    pub external_source: Option<&'a str>,
    pub external_id: Option<&'a str>,
    pub r#type: Option<&'a str>,
    pub cycle: Option<&'a str>,
    pub modules: &'a [&'a str],
    pub meta: &'a serde_json::Value,
    pub last_saved_at: &'a str,
    pub owned_by: &'a str,
    pub created_at: Option<&'a str>,
    pub updated_at: Option<&'a str>,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
}

/// Port of `IssueVersionDetailSerializer` (`issue.py:1448-1486`).
/// Field-for-field copy with BUG-dup-name collapsed to one `name`.
pub fn issue_version_detail_to_representation<'a>(
    row: &'a IssueVersionDetailRow<'a>,
) -> IssueVersionDetailView<'a> {
    IssueVersionDetailView {
        id: row.id,
        workspace: row.workspace,
        project: row.project,
        issue: row.issue,
        parent: row.parent,
        state: row.state,
        estimate_point: row.estimate_point,
        name: row.name,
        priority: row.priority,
        start_date: row.start_date,
        target_date: row.target_date,
        assignees: row.assignees,
        sequence_id: row.sequence_id,
        labels: row.labels,
        sort_order: row.sort_order,
        completed_at: row.completed_at,
        archived_at: row.archived_at,
        is_draft: row.is_draft,
        external_source: row.external_source,
        external_id: row.external_id,
        r#type: row.r#type,
        cycle: row.cycle,
        modules: row.modules,
        meta: row.meta,
        last_saved_at: row.last_saved_at,
        owned_by: row.owned_by,
        created_at: row.created_at,
        updated_at: row.updated_at,
        created_by: row.created_by,
        updated_by: row.updated_by,
    }
}

/// The `IssueDescriptionVersionDetailSerializer` key set (`issue.py:1487-1506`)
/// in `Meta.fields` order.
pub const ISSUE_DESCRIPTION_VERSION_DETAIL_FIELDS: [&str; 14] = [
    "id",
    "workspace",
    "project",
    "issue",
    "description_binary",
    "description_html",
    "description_stripped",
    "description_json",
    "last_saved_at",
    "owned_by",
    "created_at",
    "updated_at",
    "created_by",
    "updated_by",
];

/// A database row for `IssueDescriptionVersion` detail rendering.
/// `description_html` is non-nullable (`blank=True, default="<p></p>"`,
/// `db/models/issue.py:911`); `description_json` defaults to `{}` (`:913`).
#[derive(Debug, Clone, PartialEq)]
pub struct IssueDescriptionVersionDetailRow<'a> {
    pub id: &'a str,
    pub workspace: &'a str,
    pub project: &'a str,
    pub issue: &'a str,
    pub description_binary: Option<&'a [u8]>,
    pub description_html: &'a str,
    pub description_stripped: Option<&'a str>,
    pub description_json: &'a serde_json::Value,
    pub last_saved_at: &'a str,
    pub owned_by: &'a str,
    pub created_at: Option<&'a str>,
    pub updated_at: Option<&'a str>,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
}

/// `IssueDescriptionVersionDetailSerializer.to_representation` output
/// (`issue.py:1487-1506`). `description_binary` is the base64 ASCII string —
/// DRF maps `BinaryField` to the generic `ModelField`, whose rendering calls
/// `BinaryField.value_to_string`, i.e. base64 (verified on the pinned
/// Django 4.2.30 + DRF 3.15.2: `b"<p>hi</p>"` renders `"PHA+aGk8L3A+"`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct IssueDescriptionVersionDetailView {
    pub id: String,
    pub workspace: String,
    pub project: String,
    pub issue: String,
    pub description_binary: Option<String>,
    pub description_html: String,
    pub description_stripped: Option<String>,
    pub description_json: serde_json::Value,
    pub last_saved_at: String,
    pub owned_by: String,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    pub created_by: Option<String>,
    pub updated_by: Option<String>,
}

/// Port of `IssueDescriptionVersionDetailSerializer` (`issue.py:1487-1506`).
/// The view owns its strings because `description_binary` must be re-encoded;
/// every other field is a verbatim copy.
pub fn issue_description_version_detail_to_representation(
    row: &IssueDescriptionVersionDetailRow<'_>,
) -> IssueDescriptionVersionDetailView {
    IssueDescriptionVersionDetailView {
        id: row.id.to_owned(),
        workspace: row.workspace.to_owned(),
        project: row.project.to_owned(),
        issue: row.issue.to_owned(),
        description_binary: row
            .description_binary
            .map(|bytes| base64::engine::general_purpose::STANDARD.encode(bytes)),
        description_html: row.description_html.to_owned(),
        description_stripped: row.description_stripped.map(str::to_owned),
        description_json: row.description_json.clone(),
        last_saved_at: row.last_saved_at.to_owned(),
        owned_by: row.owned_by.to_owned(),
        created_at: row.created_at.map(str::to_owned),
        updated_at: row.updated_at.map(str::to_owned),
        created_by: row.created_by.map(str::to_owned),
        updated_by: row.updated_by.map(str::to_owned),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use std::collections::BTreeSet;

    const TS: &str = "2026-10-02T03:04:05.006007Z";

    fn user_row() -> UserLiteRow<'static> {
        UserLiteRow {
            id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa1",
            first_name: "Ada",
            last_name: "Lovelace",
            avatar: "ada.png",
            avatar_url: None,
            is_bot: false,
            display_name: "Ada Lovelace",
        }
    }

    fn fx_iss_03() -> Value {
        let path = format!(
            "{}/../../fixtures/app_issues/serializers/FX-ISS-03.assoc.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(&path).expect("FX-ISS-03 exists"))
            .expect("FX-ISS-03 parses")
    }

    /// The parenthetical model-column list inside a `"__all__ (model ...: ...)"`
    /// fixture string, e.g. `issue_assignee.fields`.
    fn model_columns(entry: &Value) -> Vec<String> {
        let fields = entry
            .get("fields")
            .and_then(Value::as_str)
            .expect("fields string");
        let start = fields.find(": ").expect("colon") + 2;
        let end = fields.rfind(')').expect("paren");
        fields[start..end].split(", ").map(str::to_owned).collect()
    }

    #[test]
    fn assignee_renders_nest_second_and_fks_last() {
        let user = user_row();
        let row = IssueAssigneeRow {
            id: "11111111-1111-4111-8111-111111111111",
            assignee_details: &user,
            created_at: Some(TS),
            updated_at: Some(TS),
            deleted_at: None,
            created_by: None,
            updated_by: None,
            project: "22222222-2222-4222-8222-222222222222",
            workspace: "33333333-3333-4333-8333-333333333333",
            issue: "44444444-4444-4444-8444-444444444444",
            assignee: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa1",
        };
        let body = serde_json::to_string(&issue_assignee_to_representation(&row)).unwrap();
        assert_eq!(
            body,
            r#"{"id":"11111111-1111-4111-8111-111111111111","assignee_details":{"id":"aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa1","first_name":"Ada","last_name":"Lovelace","avatar":"ada.png","avatar_url":null,"is_bot":false,"display_name":"Ada Lovelace"},"created_at":"2026-10-02T03:04:05.006007Z","updated_at":"2026-10-02T03:04:05.006007Z","deleted_at":null,"created_by":null,"updated_by":null,"project":"22222222-2222-4222-8222-222222222222","workspace":"33333333-3333-4333-8333-333333333333","issue":"44444444-4444-4444-8444-444444444444","assignee":"aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa1"}"#
        );
    }

    #[test]
    fn label_renders_deleted_at_before_fk_block() {
        let row = IssueLabelRow {
            id: "11111111-1111-4111-8111-111111111111",
            created_at: Some(TS),
            updated_at: Some(TS),
            deleted_at: None,
            created_by: Some("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa1"),
            updated_by: None,
            project: "22222222-2222-4222-8222-222222222222",
            workspace: "33333333-3333-4333-8333-333333333333",
            issue: "44444444-4444-4444-8444-444444444444",
            label: "55555555-5555-4555-8555-555555555555",
        };
        let body = serde_json::to_string(&issue_label_to_representation(&row)).unwrap();
        assert_eq!(
            body,
            r#"{"id":"11111111-1111-4111-8111-111111111111","created_at":"2026-10-02T03:04:05.006007Z","updated_at":"2026-10-02T03:04:05.006007Z","deleted_at":null,"created_by":"aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa1","updated_by":null,"project":"22222222-2222-4222-8222-222222222222","workspace":"33333333-3333-4333-8333-333333333333","issue":"44444444-4444-4444-8444-444444444444","label":"55555555-5555-4555-8555-555555555555"}"#
        );
    }

    #[test]
    fn user_property_renders_json_block_before_fks() {
        let filters = json!({"priority": ["high"]});
        let display_filters = json!({});
        let display_properties = json!({"assignee": true});
        let rich_filters = json!({});
        let preferences = json!({"theme": "dark"});
        let row = ProjectUserPropertyRow {
            id: "11111111-1111-4111-8111-111111111111",
            created_at: Some(TS),
            updated_at: Some(TS),
            deleted_at: None,
            filters: &filters,
            display_filters: &display_filters,
            display_properties: &display_properties,
            rich_filters: &rich_filters,
            preferences: &preferences,
            sort_order: 65535.0,
            created_by: None,
            updated_by: None,
            project: "22222222-2222-4222-8222-222222222222",
            workspace: "33333333-3333-4333-8333-333333333333",
            user: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa1",
        };
        let body = serde_json::to_string(&project_user_property_to_representation(&row)).unwrap();
        assert_eq!(
            body,
            r#"{"id":"11111111-1111-4111-8111-111111111111","created_at":"2026-10-02T03:04:05.006007Z","updated_at":"2026-10-02T03:04:05.006007Z","deleted_at":null,"filters":{"priority":["high"]},"display_filters":{},"display_properties":{"assignee":true},"rich_filters":{},"preferences":{"theme":"dark"},"sort_order":65535.0,"created_by":null,"updated_by":null,"project":"22222222-2222-4222-8222-222222222222","workspace":"33333333-3333-4333-8333-333333333333","user":"aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa1"}"#
        );
    }

    #[test]
    fn version_detail_collapses_dup_name_at_first_position() {
        let meta = json!({});
        let empty: [&str; 0] = [];
        let row = IssueVersionDetailRow {
            id: "11111111-1111-4111-8111-111111111111",
            workspace: "33333333-3333-4333-8333-333333333333",
            project: "22222222-2222-4222-8222-222222222222",
            issue: "44444444-4444-4444-8444-444444444444",
            parent: None,
            state: Some("66666666-6666-4666-8666-666666666666"),
            estimate_point: None,
            name: "I1 parent",
            priority: "high",
            start_date: Some("2026-10-01"),
            target_date: None,
            assignees: &empty,
            sequence_id: 1,
            labels: &empty,
            sort_order: 65535.0,
            completed_at: None,
            archived_at: None,
            is_draft: false,
            external_source: None,
            external_id: None,
            r#type: None,
            cycle: None,
            modules: &empty,
            meta: &meta,
            last_saved_at: TS,
            owned_by: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa1",
            created_at: Some(TS),
            updated_at: Some(TS),
            created_by: Some("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa1"),
            updated_by: None,
        };
        let body = serde_json::to_string(&issue_version_detail_to_representation(&row)).unwrap();
        // BUG-dup-name: exactly one "name" key, at the first listing position.
        assert_eq!(body.matches("\"name\"").count(), 1);
        assert_eq!(
            body,
            r#"{"id":"11111111-1111-4111-8111-111111111111","workspace":"33333333-3333-4333-8333-333333333333","project":"22222222-2222-4222-8222-222222222222","issue":"44444444-4444-4444-8444-444444444444","parent":null,"state":"66666666-6666-4666-8666-666666666666","estimate_point":null,"name":"I1 parent","priority":"high","start_date":"2026-10-01","target_date":null,"assignees":[],"sequence_id":1,"labels":[],"sort_order":65535.0,"completed_at":null,"archived_at":null,"is_draft":false,"external_source":null,"external_id":null,"type":null,"cycle":null,"modules":[],"meta":{},"last_saved_at":"2026-10-02T03:04:05.006007Z","owned_by":"aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa1","created_at":"2026-10-02T03:04:05.006007Z","updated_at":"2026-10-02T03:04:05.006007Z","created_by":"aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa1","updated_by":null}"#
        );
    }

    #[test]
    fn description_version_renders_binary_as_base64() {
        let json = json!({"type": "doc"});
        let row = IssueDescriptionVersionDetailRow {
            id: "11111111-1111-4111-8111-111111111111",
            workspace: "33333333-3333-4333-8333-333333333333",
            project: "22222222-2222-4222-8222-222222222222",
            issue: "44444444-4444-4444-8444-444444444444",
            description_binary: Some(b"<p>hi</p>".as_slice()),
            description_html: "<p>hi</p>",
            description_stripped: Some("hi"),
            description_json: &json,
            last_saved_at: TS,
            owned_by: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa1",
            created_at: Some(TS),
            updated_at: Some(TS),
            created_by: Some("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa1"),
            updated_by: None,
        };
        let body = serde_json::to_string(&issue_description_version_detail_to_representation(&row))
            .unwrap();
        assert_eq!(
            body,
            r#"{"id":"11111111-1111-4111-8111-111111111111","workspace":"33333333-3333-4333-8333-333333333333","project":"22222222-2222-4222-8222-222222222222","issue":"44444444-4444-4444-8444-444444444444","description_binary":"PHA+aGk8L3A+","description_html":"<p>hi</p>","description_stripped":"hi","description_json":{"type":"doc"},"last_saved_at":"2026-10-02T03:04:05.006007Z","owned_by":"aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa1","created_at":"2026-10-02T03:04:05.006007Z","updated_at":"2026-10-02T03:04:05.006007Z","created_by":"aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa1","updated_by":null}"#
        );
    }

    #[test]
    fn description_version_renders_null_binary_as_null() {
        let json = json!({});
        let row = IssueDescriptionVersionDetailRow {
            id: "11111111-1111-4111-8111-111111111111",
            workspace: "33333333-3333-4333-8333-333333333333",
            project: "22222222-2222-4222-8222-222222222222",
            issue: "44444444-4444-4444-8444-444444444444",
            description_binary: None,
            description_html: "<p></p>",
            description_stripped: None,
            description_json: &json,
            last_saved_at: TS,
            owned_by: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa1",
            created_at: Some(TS),
            updated_at: Some(TS),
            created_by: None,
            updated_by: None,
        };
        let view = issue_description_version_detail_to_representation(&row);
        assert_eq!(view.description_binary, None);
        let body = serde_json::to_string(&view).unwrap();
        assert!(body.contains("\"description_binary\":null"));
    }

    #[test]
    fn fields_consts_replay_fx_iss_03() {
        let golden = fx_iss_03();

        let version_order: Vec<String> = golden["issue_version_detail"]["fields_in_order"]
            .as_array()
            .expect("version fields_in_order")
            .iter()
            .map(|v| v.as_str().expect("str").to_owned())
            .collect();
        // BUG-dup-name: the fixture lists `name` twice; DRF collapses it.
        let mut deduped: Vec<String> = Vec::with_capacity(version_order.len());
        for key in &version_order {
            if !deduped.contains(key) {
                deduped.push(key.clone());
            }
        }
        assert_eq!(version_order.len(), 31);
        let const_order: Vec<String> = ISSUE_VERSION_DETAIL_FIELDS
            .iter()
            .map(|key| key.to_string())
            .collect();
        assert_eq!(deduped, const_order);

        let desc_order: Vec<String> = golden["issue_description_version_detail"]["fields_in_order"]
            .as_array()
            .expect("desc fields_in_order")
            .iter()
            .map(|v| v.as_str().expect("str").to_owned())
            .collect();
        let desc_const: Vec<String> = ISSUE_DESCRIPTION_VERSION_DETAIL_FIELDS
            .iter()
            .map(|key| key.to_string())
            .collect();
        assert_eq!(desc_order, desc_const);

        // The `__all__` fixtures list model columns; the wire order on top of
        // them is DRF's pk + declared + non-relational + FK rule, pinned by
        // the byte goldens above. Replay compares key sets here.
        let assignee_model: BTreeSet<String> = model_columns(&golden["issue_assignee"])
            .into_iter()
            .collect();
        let assignee_wire: BTreeSet<String> = ISSUE_ASSIGNEE_FIELDS
            .iter()
            .copied()
            .filter(|key| *key != "assignee_details")
            .map(str::to_owned)
            .collect();
        assert_eq!(assignee_model, assignee_wire);

        let label_model: BTreeSet<String> =
            model_columns(&golden["issue_label"]).into_iter().collect();
        let label_wire: BTreeSet<String> = ISSUE_LABEL_FIELDS
            .iter()
            .map(|key| key.to_string())
            .collect();
        assert_eq!(label_model, label_wire);

        let prop_model: BTreeSet<String> = model_columns(&golden["project_user_property"])
            .into_iter()
            .collect();
        let prop_wire: BTreeSet<String> = PROJECT_USER_PROPERTY_FIELDS
            .iter()
            .map(|key| key.to_string())
            .collect();
        assert_eq!(prop_model, prop_wire);
    }
}
