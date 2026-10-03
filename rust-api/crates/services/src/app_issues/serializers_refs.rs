#![forbid(unsafe_code)]

//! Cycle / module / state / intake reference serializers for app:issues (D-26).
//!
//! Port of `apps/api/pi_dash/app/serializers/issue.py`:
//!
//! * `:678-691` (`CycleBaseSerializer`, `fields = "__all__"`)
//! * `:708-721` (`ModuleBaseSerializer`, `fields = "__all__"`)
//! * `:1004-1020` (`IssueStateSerializer`, `exclude = ["workpad"]` + 7 declares)
//! * `:1021-1038` (`IssueIntakeSerializer`, explicit 8-key list)
//!
//! Pure output kernels: each `to_representation` takes a row borrowed from
//! the caller and returns a `serde::Serialize` view whose fields are the
//! live DRF wire fields. Fixture: `FX-ISS-06.refs.json` (`TRACE.md`:
//! serializers/FX-ISS-06). Key orders and byte vectors below were confirmed
//! by a live-DRF probe (pinned Django 4.2.30 / DRF 3.15.2, real rows on a
//! scratch database), which the tests replay byte-for-byte.
//!
//! DRF order rule (`__all__` / `exclude`): `[pk] + declared(base-first) +
//! model_info.fields + model_info.forward_relations` — forward relations
//! (FKs *and* M2Ms) trail after every concrete column
//! (`ModelSerializer.get_default_field_names`). Explicit `Meta.fields`
//! lists render verbatim. The `IssueIntakeSerializer` order is its listed
//! order; the other three shapes follow the default rule (probed).
//!
//! * UUID and FK primary keys render as strings (`PrimaryKeyRelatedField`,
//!   read-only); a null FK renders `null`. `IssueIntake.project_id` is the
//!   raw attname: DRF resolves it through the pk-only optimization, so it
//!   renders exactly like the FK name (probed).
//! * Datetimes and dates cross this boundary already rendered as DRF
//!   strings (`iso-8601` with `Z`, `YYYY-MM-DD`) — formatting owns to the
//!   DB edge, so rendering here is a byte-exact passthrough.
//! * `JSONField` columns cross as parsed `serde_json::Value`. Module
//!   `description_text` / `description_html` are nullable (`null=True`),
//!   hence `Option` (probed `null`, object, and bare-string arms).
//! * `Issue.description_binary` (`BinaryField`, no serializer mapping) falls
//!   back to DRF `ModelField`, which renders
//!   `BinaryField.value_to_string` — standard base64 ASCII
//!   (`django/db/models/fields: b64encode(...).decode("ascii")`, probed
//!   `b"\x00\x01bin"` -> `"AAFiaW4="`). The caller base64-encodes; the view
//!   passes the ASCII through. `None` renders `null`.
//! * The annotated counts (`sub_issues_count`, `attachment_count`,
//!   `link_count`) and `label_ids` have no model attribute: when the
//!   queryset does not annotate them DRF raises `SkipField` and the key is
//!   *omitted*, not `null` (probed) — hence `Option` with
//!   `skip_serializing_if`. An annotated empty `label_ids` renders `[]`.
//! * A null nest source short-circuits to a present `null` key (no skip):
//!   `state_detail` with `state=None` renders `"state_detail": null`
//!   (probed). Empty `many=True` nests render `[]`.
//! * M2M id arrays (`members`, `assignees`, `labels`) render in caller query
//!   order (probed `[u2, u1]`, the through-models' `-created_at`); the port
//!   preserves `Vec` order verbatim.
//! * `avatar_url` / `cover_image_url` are model `@property`s (asset URL, else
//!   the raw text when truthy, else `None`) resolved by the caller
//!   (`db/models/user.py:143-151`, `db/models/project.py:176-185`); the
//!   views pass them through verbatim. Empty-string `avatar` yields a null
//!   `avatar_url` (probed).
//!
//! Shape-only no-ops preserved as documentation, not code:
//! `read_only_fields` (`:682-689`, `:712-719`, `:1036`) constrain writes, of
//! which this port has none. One quirk is render-neutral: `label_ids` is a
//! *declared* field, and `read_only_fields` only applies to auto-generated
//! fields (`get_extra_kwargs`), so it stays `read_only=False` (probed) —
//! reads render identically either way.
//!
//! Out of scope here (not in the four units): the `IssueCycleDetail` /
//! `IssueModuleDetail` / `IssueStateFlat` variants, reused from the merged
//! space `issue_graph` port per the fixture; and the
//! `DynamicBaseSerializer` expand machinery (`serializers/base.py:12-201`)
//! — no call site passes `expand` to these shapes (`cycle.py:93`,
//! `intake.py:28,95` construct them plain), so default construction renders
//! the plain body (probed).
//!
//! Diverged twins (not a ported bug — ours to fix, tracked): the space
//! `taxonomy.rs` `CYCLE_ALL_FIELDS` / `MODULE_ALL_FIELDS` consts use
//! model-definition order, contradicting the live-DRF trailing-relations
//! rule above (their tests compare sorted keys, so the order slipped
//! through). This module ports the probed order; the merged twins are NOT
//! reused here. Fix: PIDASHCONV-687.
//!
//! Ported bugs (translate, don't redesign): none in these four units —
//! they are pure reads with no validation, write, or fallible paths.

use super::serializers_links::{user_lite_to_representation, UserLiteRow, UserLiteView};
use serde::Serialize;

// The app `UserLiteSerializer` nest (`user.py:141-153`) is the identical
// port already merged in `serializers_links` (PIDASHCONV-641); it is
// imported here, not duplicated. `USER_LITE_FIELDS` is imported by the
// test module below, its only user here.

/// App `StateLiteSerializer` wire keys (`state.py:37-41`), in
/// `Meta.fields` order: the `state_detail` nest.
pub const STATE_LITE_FIELDS: [&str; 4] = ["id", "name", "color", "group"];

/// A `State` row for nested lite rendering (`db/models/state.py:93-111`):
/// all four columns are non-null.
#[derive(Debug, Clone, PartialEq)]
pub struct StateLiteRow<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub color: &'a str,
    pub group: &'a str,
}

/// Nested app `StateLiteSerializer.to_representation` output
/// (`state.py:37-41`), in wire order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StateLiteView<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub color: &'a str,
    pub group: &'a str,
}

/// Port of the nested app `StateLiteSerializer` (`state.py:37-41`).
pub fn state_lite_to_representation<'a>(row: &'a StateLiteRow<'a>) -> StateLiteView<'a> {
    StateLiteView {
        id: row.id,
        name: row.name,
        color: row.color,
        group: row.group,
    }
}

/// App `ProjectLiteSerializer` wire keys (`project.py:120-133`), in
/// `Meta.fields` order: the `project_detail` nest. This is the app twin —
/// it differs from the space `ProjectLiteSerializer` (`cover_image_url` /
/// `logo_props` / `is_default` vs `icon_prop` / `emoji`).
pub const PROJECT_LITE_FIELDS: [&str; 8] = [
    "id",
    "identifier",
    "name",
    "cover_image",
    "cover_image_url",
    "logo_props",
    "description",
    "is_default",
];

/// A `Project` row for nested lite rendering (`db/models/project.py:72+`):
/// `id` UUID string, `identifier`, `name`, nullable `cover_image` text
/// (`project.py:107`, `null=True`), the resolved `cover_image_url`
/// (property, `project.py:176-185`), non-null `logo_props` JSON,
/// non-null `description` text, `is_default` flag.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectLiteRow<'a> {
    pub id: &'a str,
    pub identifier: &'a str,
    pub name: &'a str,
    pub cover_image: Option<&'a str>,
    pub cover_image_url: Option<&'a str>,
    pub logo_props: &'a serde_json::Value,
    pub description: &'a str,
    pub is_default: bool,
}

/// Nested app `ProjectLiteSerializer.to_representation` output
/// (`project.py:120-133`), in wire order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProjectLiteView<'a> {
    pub id: &'a str,
    pub identifier: &'a str,
    pub name: &'a str,
    pub cover_image: Option<&'a str>,
    pub cover_image_url: Option<&'a str>,
    pub logo_props: &'a serde_json::Value,
    pub description: &'a str,
    pub is_default: bool,
}

/// Port of the nested app `ProjectLiteSerializer` (`project.py:120-133`).
pub fn project_lite_to_representation<'a>(row: &'a ProjectLiteRow<'a>) -> ProjectLiteView<'a> {
    ProjectLiteView {
        id: row.id,
        identifier: row.identifier,
        name: row.name,
        cover_image: row.cover_image,
        cover_image_url: row.cover_image_url,
        logo_props: row.logo_props,
        description: row.description,
        is_default: row.is_default,
    }
}

/// App `LabelLiteSerializer` wire keys (`issue.py:577-580`), in
/// `Meta.fields` order: the `label_details` nest.
pub const LABEL_LITE_FIELDS: [&str; 3] = ["id", "name", "color"];

/// A `Label` row for nested lite rendering (`db/models/label.py:11-25`):
/// all three columns are non-null (`color` may be `""`).
#[derive(Debug, Clone, PartialEq)]
pub struct LabelLiteRow<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub color: &'a str,
}

/// Nested app `LabelLiteSerializer.to_representation` output
/// (`issue.py:577-580`), in wire order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LabelLiteView<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub color: &'a str,
}

/// Port of the nested app `LabelLiteSerializer` (`issue.py:577-580`).
pub fn label_lite_to_representation<'a>(row: &'a LabelLiteRow<'a>) -> LabelLiteView<'a> {
    LabelLiteView {
        id: row.id,
        name: row.name,
        color: row.color,
    }
}

/// The app `CycleBaseSerializer` `fields = "__all__"` key set
/// (`issue.py:678-691`), in live DRF order: `id`, the concrete columns
/// (`created_at`, `updated_at`, `deleted_at`, then `Cycle`'s own
/// non-relational columns in definition order,
/// `db/models/cycle.py:60-80`), then the forward relations trailing
/// (`created_by`, `updated_by`, `project`, `workspace`, `owned_by`).
pub const CYCLE_BASE_FIELDS: [&str; 22] = [
    "id",
    "created_at",
    "updated_at",
    "deleted_at",
    "name",
    "description",
    "start_date",
    "end_date",
    "view_props",
    "sort_order",
    "external_source",
    "external_id",
    "progress_snapshot",
    "archived_at",
    "logo_props",
    "timezone",
    "version",
    "created_by",
    "updated_by",
    "project",
    "workspace",
    "owned_by",
];

/// A database row for `Cycle` rendering. Datetimes are pre-rendered DRF
/// strings; `project`, `workspace` and `owned_by` are UUID strings
/// (`owned_by` is a required FK, `db/models/cycle.py:65-69`); JSON columns
/// are borrowed values.
#[derive(Debug, Clone, PartialEq)]
pub struct CycleBaseRow<'a> {
    pub id: &'a str,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub deleted_at: Option<&'a str>,
    pub name: &'a str,
    pub description: &'a str,
    pub start_date: Option<&'a str>,
    pub end_date: Option<&'a str>,
    pub view_props: &'a serde_json::Value,
    pub sort_order: f64,
    pub external_source: Option<&'a str>,
    pub external_id: Option<&'a str>,
    pub progress_snapshot: &'a serde_json::Value,
    pub archived_at: Option<&'a str>,
    pub logo_props: &'a serde_json::Value,
    pub timezone: &'a str,
    pub version: i32,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub project: &'a str,
    pub workspace: &'a str,
    pub owned_by: &'a str,
}

/// `CycleBaseSerializer.to_representation` output (`issue.py:678-691`,
/// `fields = "__all__"`), in wire order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CycleBaseView<'a> {
    pub id: &'a str,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub deleted_at: Option<&'a str>,
    pub name: &'a str,
    pub description: &'a str,
    pub start_date: Option<&'a str>,
    pub end_date: Option<&'a str>,
    pub view_props: &'a serde_json::Value,
    pub sort_order: f64,
    pub external_source: Option<&'a str>,
    pub external_id: Option<&'a str>,
    pub progress_snapshot: &'a serde_json::Value,
    pub archived_at: Option<&'a str>,
    pub logo_props: &'a serde_json::Value,
    pub timezone: &'a str,
    pub version: i32,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub project: &'a str,
    pub workspace: &'a str,
    pub owned_by: &'a str,
}

/// Port of `CycleBaseSerializer` (`issue.py:678-691`). Field-for-field copy.
pub fn cycle_base_to_representation<'a>(row: &'a CycleBaseRow<'a>) -> CycleBaseView<'a> {
    CycleBaseView {
        id: row.id,
        created_at: row.created_at,
        updated_at: row.updated_at,
        deleted_at: row.deleted_at,
        name: row.name,
        description: row.description,
        start_date: row.start_date,
        end_date: row.end_date,
        view_props: row.view_props,
        sort_order: row.sort_order,
        external_source: row.external_source,
        external_id: row.external_id,
        progress_snapshot: row.progress_snapshot,
        archived_at: row.archived_at,
        logo_props: row.logo_props,
        timezone: row.timezone,
        version: row.version,
        created_by: row.created_by,
        updated_by: row.updated_by,
        project: row.project,
        workspace: row.workspace,
        owned_by: row.owned_by,
    }
}

/// The app `ModuleBaseSerializer` `fields = "__all__"` key set
/// (`issue.py:708-721`), in live DRF order: `id`, the concrete columns
/// (`created_at`, `updated_at`, `deleted_at`, then `Module`'s own
/// non-relational columns in definition order,
/// `db/models/module.py:67-99`), then the forward relations trailing —
/// `created_by`, `updated_by`, `project`, `workspace`, the `lead` FK, and
/// the `members` M2M last.
pub const MODULE_BASE_FIELDS: [&str; 23] = [
    "id",
    "created_at",
    "updated_at",
    "deleted_at",
    "name",
    "description",
    "description_text",
    "description_html",
    "start_date",
    "target_date",
    "status",
    "view_props",
    "sort_order",
    "external_source",
    "external_id",
    "archived_at",
    "logo_props",
    "created_by",
    "updated_by",
    "project",
    "workspace",
    "lead",
    "members",
];

/// A database row for `Module` rendering. Dates/datetimes are pre-rendered
/// DRF strings; `project` and `workspace` are UUID strings; `lead` is a
/// nullable FK (`db/models/module.py:86`); `members` is the M2M id array in
/// caller query order; the two `description_*` JSON columns are nullable.
#[derive(Debug, Clone, PartialEq)]
pub struct ModuleBaseRow<'a> {
    pub id: &'a str,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub deleted_at: Option<&'a str>,
    pub name: &'a str,
    pub description: &'a str,
    pub description_text: Option<&'a serde_json::Value>,
    pub description_html: Option<&'a serde_json::Value>,
    pub start_date: Option<&'a str>,
    pub target_date: Option<&'a str>,
    pub status: &'a str,
    pub view_props: &'a serde_json::Value,
    pub sort_order: f64,
    pub external_source: Option<&'a str>,
    pub external_id: Option<&'a str>,
    pub archived_at: Option<&'a str>,
    pub logo_props: &'a serde_json::Value,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub project: &'a str,
    pub workspace: &'a str,
    pub lead: Option<&'a str>,
    pub members: Vec<&'a str>,
}

/// `ModuleBaseSerializer.to_representation` output (`issue.py:708-721`,
/// `fields = "__all__"`), in wire order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ModuleBaseView<'a> {
    pub id: &'a str,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub deleted_at: Option<&'a str>,
    pub name: &'a str,
    pub description: &'a str,
    pub description_text: Option<&'a serde_json::Value>,
    pub description_html: Option<&'a serde_json::Value>,
    pub start_date: Option<&'a str>,
    pub target_date: Option<&'a str>,
    pub status: &'a str,
    pub view_props: &'a serde_json::Value,
    pub sort_order: f64,
    pub external_source: Option<&'a str>,
    pub external_id: Option<&'a str>,
    pub archived_at: Option<&'a str>,
    pub logo_props: &'a serde_json::Value,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub project: &'a str,
    pub workspace: &'a str,
    pub lead: Option<&'a str>,
    pub members: Vec<&'a str>,
}

/// Port of `ModuleBaseSerializer` (`issue.py:708-721`). Field-for-field copy.
pub fn module_base_to_representation<'a>(row: &'a ModuleBaseRow<'a>) -> ModuleBaseView<'a> {
    ModuleBaseView {
        id: row.id,
        created_at: row.created_at,
        updated_at: row.updated_at,
        deleted_at: row.deleted_at,
        name: row.name,
        description: row.description,
        description_text: row.description_text,
        description_html: row.description_html,
        start_date: row.start_date,
        target_date: row.target_date,
        status: row.status,
        view_props: row.view_props,
        sort_order: row.sort_order,
        external_source: row.external_source,
        external_id: row.external_id,
        archived_at: row.archived_at,
        logo_props: row.logo_props,
        created_by: row.created_by,
        updated_by: row.updated_by,
        project: row.project,
        workspace: row.workspace,
        lead: row.lead,
        members: row.members.clone(),
    }
}

/// The app `IssueStateSerializer` key set (`issue.py:1004-1020`), in live
/// DRF order: `id`, the 7 declared nests/counters (in declaration order),
/// then every `Issue` column except `workpad` — concrete columns first
/// (`created_at` … `agent_executor`, `db/models/issue.py:115-228`), then
/// the forward relations trailing (`created_by` … `labels`, M2M last).
pub const ISSUE_STATE_FIELDS: [&str; 42] = [
    "id",
    "label_details",
    "state_detail",
    "project_detail",
    "assignee_details",
    "sub_issues_count",
    "attachment_count",
    "link_count",
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

/// A database row for `IssueStateSerializer` rendering: the resolved lite
/// nests (a null `state` resolves to `state_detail: None`), the annotated
/// counters (`None` when the queryset did not annotate them — the key is
/// then omitted), and the `Issue` columns (`db/models/issue.py:107-228`).
/// Datetimes/dates are pre-rendered DRF strings; `description_binary` is
/// the caller-encoded base64 ASCII; `assignees` / `labels` are M2M id
/// arrays in caller query order.
#[derive(Debug, Clone, PartialEq)]
pub struct IssueStateRow<'a> {
    pub id: &'a str,
    pub label_details: Vec<LabelLiteRow<'a>>,
    pub state_detail: Option<StateLiteRow<'a>>,
    pub project_detail: ProjectLiteRow<'a>,
    pub assignee_details: Vec<UserLiteRow<'a>>,
    pub sub_issues_count: Option<i64>,
    pub attachment_count: Option<i64>,
    pub link_count: Option<i64>,
    pub created_at: &'a str,
    pub updated_at: &'a str,
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
    pub issue_type: Option<&'a str>,
    pub assigned_pod: Option<&'a str>,
    pub assignees: Vec<&'a str>,
    pub labels: Vec<&'a str>,
}

/// `IssueStateSerializer.to_representation` output (`issue.py:1004-1020`),
/// in wire order. `issue_type` serializes as `type` (`type` is a Rust
/// keyword); the three counters carry `skip_serializing_if` for the
/// unannotated `SkipField` arm.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct IssueStateView<'a> {
    pub id: &'a str,
    pub label_details: Vec<LabelLiteView<'a>>,
    pub state_detail: Option<StateLiteView<'a>>,
    pub project_detail: ProjectLiteView<'a>,
    pub assignee_details: Vec<UserLiteView<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sub_issues_count: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attachment_count: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link_count: Option<i64>,
    pub created_at: &'a str,
    pub updated_at: &'a str,
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
    #[serde(rename = "type")]
    pub issue_type: Option<&'a str>,
    pub assigned_pod: Option<&'a str>,
    pub assignees: Vec<&'a str>,
    pub labels: Vec<&'a str>,
}

/// Port of `IssueStateSerializer` (`issue.py:1004-1020`). The `workpad`
/// exclusion is structural (no such field exists); the nests render
/// through the app lite ports above.
pub fn issue_state_to_representation<'a>(row: &'a IssueStateRow<'a>) -> IssueStateView<'a> {
    IssueStateView {
        id: row.id,
        label_details: row
            .label_details
            .iter()
            .map(label_lite_to_representation)
            .collect(),
        state_detail: row.state_detail.as_ref().map(state_lite_to_representation),
        project_detail: project_lite_to_representation(&row.project_detail),
        assignee_details: row
            .assignee_details
            .iter()
            .map(user_lite_to_representation)
            .collect(),
        sub_issues_count: row.sub_issues_count,
        attachment_count: row.attachment_count,
        link_count: row.link_count,
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
        issue_type: row.issue_type,
        assigned_pod: row.assigned_pod,
        assignees: row.assignees.clone(),
        labels: row.labels.clone(),
    }
}

/// The app `IssueIntakeSerializer` key set (`issue.py:1021-1038`), in
/// `Meta.fields` order (explicit list, all read-only).
pub const ISSUE_INTAKE_FIELDS: [&str; 8] = [
    "id",
    "name",
    "priority",
    "sequence_id",
    "project_id",
    "created_at",
    "label_ids",
    "created_by",
];

/// A database row for `IssueIntakeSerializer` rendering: `project_id` is
/// the raw attname UUID string (required FK); `label_ids` is the annotated
/// id array (`None` when not annotated — the key is then omitted).
#[derive(Debug, Clone, PartialEq)]
pub struct IssueIntakeRow<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub priority: &'a str,
    pub sequence_id: i32,
    pub project_id: &'a str,
    pub created_at: &'a str,
    pub label_ids: Option<Vec<&'a str>>,
    pub created_by: Option<&'a str>,
}

/// `IssueIntakeSerializer.to_representation` output (`issue.py:1021-1038`),
/// in wire order. `label_ids` carries `skip_serializing_if` for the
/// unannotated `SkipField` arm.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct IssueIntakeView<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub priority: &'a str,
    pub sequence_id: i32,
    pub project_id: &'a str,
    pub created_at: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label_ids: Option<Vec<&'a str>>,
    pub created_by: Option<&'a str>,
}

/// Port of `IssueIntakeSerializer` (`issue.py:1021-1038`). Field-for-field
/// copy; the intake caller (`intake.py:28,95`) copies the annotated
/// `label_ids` onto the issue instance before rendering.
pub fn issue_intake_to_representation<'a>(row: &'a IssueIntakeRow<'a>) -> IssueIntakeView<'a> {
    IssueIntakeView {
        id: row.id,
        name: row.name,
        priority: row.priority,
        sequence_id: row.sequence_id,
        project_id: row.project_id,
        created_at: row.created_at,
        label_ids: row.label_ids.clone(),
        created_by: row.created_by,
    }
}

#[cfg(test)]
mod tests {
    use super::super::serializers_links::USER_LITE_FIELDS;
    use super::*;
    use serde_json::Value;

    fn fixture() -> Value {
        let path = format!(
            "{}/../../fixtures/app_issues/serializers/FX-ISS-06.refs.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(&path).expect("golden exists"))
            .expect("golden parses")
    }

    fn fields_in_order(fixture: &Value, case: &str) -> Vec<String> {
        fixture
            .get(case)
            .unwrap_or_else(|| panic!("golden lacks {case}"))
            .get("fields_in_order")
            .and_then(Value::as_array)
            .expect("fields_in_order array")
            .iter()
            .map(|key| key.as_str().expect("key str").to_string())
            .collect()
    }

    /// Top-level JSON key order of a struct's serialization.
    ///
    /// Read off the serialized string, not a `serde_json::Value`: struct
    /// serialization always emits declaration order, while `Value` objects
    /// iterate alphabetically.
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
        fields.iter().map(|name| name.to_string()).collect()
    }

    const U1: &str = "919f5c25-56d5-44a8-9ef5-ff72e1e29cf2";
    const U2: &str = "7a62ed71-155e-4e65-a7eb-6853cf7b6261";
    const PROJ: &str = "22e00942-19b1-45b1-b388-438f37942205";
    const WS: &str = "994cc638-306d-4305-ad86-c4e87f829cb4";
    const ST: &str = "539afd47-4ad4-4cdc-bf26-8d463178faf8";
    const L_BUG: &str = "0e1cb691-7d20-4f47-804c-4c17711db39d";
    const L_URGENT: &str = "81e4506d-fe09-4bcd-8746-94ea4e17643d";
    const T1: &str = "2026-01-02T03:04:05.123000Z";
    const T2: &str = "2026-05-06T07:08:09.456000Z";
    const POD: &str = "32f13029-e8bf-4f39-a97e-dca9ab90b635";
    const ISS: &str = "e612ce6c-953c-4418-b943-55e50f416aa8";
    const EP: &str = "ac462fe7-dd34-4b65-8a41-6ab7c90feba9";
    const IT: &str = "5396e0eb-8629-4b0a-afd2-ca59f4b1dbf4";

    fn user_row_ada() -> UserLiteRow<'static> {
        UserLiteRow {
            id: U1,
            first_name: "Ada",
            last_name: "Lovelace",
            avatar: "https://cdn/x/a1.png",
            avatar_url: Some("https://cdn/x/a1.png"),
            is_bot: false,
            display_name: "probe-u1-643",
        }
    }

    fn user_row_alan() -> UserLiteRow<'static> {
        UserLiteRow {
            id: U2,
            first_name: "Alan",
            last_name: "Turing",
            avatar: "",
            avatar_url: None,
            is_bot: false,
            display_name: "probe-u2-643",
        }
    }

    fn state_row() -> StateLiteRow<'static> {
        StateLiteRow {
            id: ST,
            name: "In Progress",
            color: "#ff0000",
            group: "started",
        }
    }

    fn project_row<'a>(logo_props: &'a Value) -> ProjectLiteRow<'a> {
        ProjectLiteRow {
            id: PROJ,
            identifier: "PRB",
            name: "Probe Proj",
            cover_image: Some("https://cdn/x/cover.png"),
            cover_image_url: Some("https://cdn/x/cover.png"),
            logo_props,
            description: "probe project",
            is_default: true,
        }
    }

    #[test]
    fn field_consts_match_fx_iss_06() {
        let golden = fixture();
        // The intake case pins the full key array in the fixture.
        assert_eq!(
            const_keys(&ISSUE_INTAKE_FIELDS),
            fields_in_order(&golden, "issue_intake"),
        );
        // The other three cases pin the class + field-spec; the exact
        // orders below come from the live-DRF probe (the fixture carries
        // no key arrays for `__all__` / `exclude` shapes).
        let cycle = golden.get("cycle_base").expect("cycle_base case");
        assert_eq!(
            cycle.get("class").and_then(Value::as_str),
            Some("CycleBaseSerializer :678-691")
        );
        assert_eq!(
            cycle.get("fields").and_then(Value::as_str),
            Some("__all__ (Cycle model columns)")
        );
        let module = golden.get("module_base").expect("module_base case");
        assert_eq!(
            module.get("class").and_then(Value::as_str),
            Some("ModuleBaseSerializer :708-721")
        );
        assert_eq!(
            module.get("fields").and_then(Value::as_str),
            Some("__all__ (Module model columns)")
        );
        let state = golden.get("issue_state").expect("issue_state case");
        assert_eq!(
            state.get("class").and_then(Value::as_str),
            Some("IssueStateSerializer(DynamicBaseSerializer) :1004-1020")
        );
        let mut declared: Vec<String> = state
            .get("declared")
            .and_then(Value::as_object)
            .expect("declared object")
            .keys()
            .cloned()
            .collect();
        declared.sort();
        assert_eq!(
            declared,
            [
                "assignee_details",
                "attachment_count",
                "label_details",
                "link_count",
                "project_detail",
                "state_detail",
                "sub_issues_count",
            ]
        );
        assert!(
            state
                .get("note")
                .and_then(Value::as_str)
                .expect("note")
                .contains("workpad"),
            "state note pins the workpad exclusion"
        );
    }

    #[test]
    fn user_lite_replays_probe_bytes() {
        let row = user_row_ada();
        let view = user_lite_to_representation(&row);
        assert_eq!(serialized_keys(&view), const_keys(&USER_LITE_FIELDS));
        assert_eq!(
            serde_json::to_string(&view).expect("serializes"),
            "{\"id\":\"919f5c25-56d5-44a8-9ef5-ff72e1e29cf2\",\
             \"first_name\":\"Ada\",\"last_name\":\"Lovelace\",\
             \"avatar\":\"https://cdn/x/a1.png\",\"avatar_url\":\"https://cdn/x/a1.png\",\
             \"is_bot\":false,\"display_name\":\"probe-u1-643\"}",
        );
        // Empty-string avatar yields a null avatar_url (property arm).
        let row = user_row_alan();
        let view = user_lite_to_representation(&row);
        assert_eq!(
            serde_json::to_string(&view).expect("serializes"),
            "{\"id\":\"7a62ed71-155e-4e65-a7eb-6853cf7b6261\",\
             \"first_name\":\"Alan\",\"last_name\":\"Turing\",\
             \"avatar\":\"\",\"avatar_url\":null,\
             \"is_bot\":false,\"display_name\":\"probe-u2-643\"}",
        );
    }

    #[test]
    fn state_lite_replays_probe_bytes() {
        let row = state_row();
        let view = state_lite_to_representation(&row);
        assert_eq!(serialized_keys(&view), const_keys(&STATE_LITE_FIELDS));
        assert_eq!(
            serde_json::to_string(&view).expect("serializes"),
            "{\"id\":\"539afd47-4ad4-4cdc-bf26-8d463178faf8\",\
             \"name\":\"In Progress\",\"color\":\"#ff0000\",\"group\":\"started\"}",
        );
    }

    #[test]
    fn project_lite_replays_probe_bytes() {
        let logo = serde_json::json!({"theme": "dark"});
        let row = project_row(&logo);
        let view = project_lite_to_representation(&row);
        assert_eq!(serialized_keys(&view), const_keys(&PROJECT_LITE_FIELDS));
        assert_eq!(
            serde_json::to_string(&view).expect("serializes"),
            "{\"id\":\"22e00942-19b1-45b1-b388-438f37942205\",\
             \"identifier\":\"PRB\",\"name\":\"Probe Proj\",\
             \"cover_image\":\"https://cdn/x/cover.png\",\
             \"cover_image_url\":\"https://cdn/x/cover.png\",\
             \"logo_props\":{\"theme\":\"dark\"},\"description\":\"probe project\",\
             \"is_default\":true}",
        );
        // Null cover arms.
        let empty = serde_json::json!({});
        let row = ProjectLiteRow {
            id: "1a96c4ce-d6d2-4de4-b22d-8afd282a09d6",
            identifier: "NC",
            name: "NoCover",
            cover_image: None,
            cover_image_url: None,
            logo_props: &empty,
            description: "",
            is_default: false,
        };
        let view = project_lite_to_representation(&row);
        assert_eq!(
            serde_json::to_string(&view).expect("serializes"),
            "{\"id\":\"1a96c4ce-d6d2-4de4-b22d-8afd282a09d6\",\
             \"identifier\":\"NC\",\"name\":\"NoCover\",\
             \"cover_image\":null,\"cover_image_url\":null,\
             \"logo_props\":{},\"description\":\"\",\"is_default\":false}",
        );
    }

    #[test]
    fn label_lite_replays_probe_bytes() {
        let row = LabelLiteRow {
            id: L_BUG,
            name: "bug",
            color: "#00ff00",
        };
        let view = label_lite_to_representation(&row);
        assert_eq!(serialized_keys(&view), const_keys(&LABEL_LITE_FIELDS));
        assert_eq!(
            serde_json::to_string(&view).expect("serializes"),
            "{\"id\":\"0e1cb691-7d20-4f47-804c-4c17711db39d\",\
             \"name\":\"bug\",\"color\":\"#00ff00\"}",
        );
    }

    #[test]
    fn cycle_base_replays_probe_bytes() {
        let view_props = serde_json::json!({"layout": "list"});
        let progress = serde_json::json!({"done": 3});
        let logo = serde_json::json!({"icon": "bolt"});
        let row = CycleBaseRow {
            id: "4be08458-2c0c-4d43-861c-347225b0ded4",
            created_at: T1,
            updated_at: T2,
            deleted_at: None,
            name: "C1",
            description: "first cycle",
            start_date: Some(T1),
            end_date: Some(T2),
            view_props: &view_props,
            sort_order: 100.5,
            external_source: Some("gh"),
            external_id: Some("c-1"),
            progress_snapshot: &progress,
            archived_at: None,
            logo_props: &logo,
            timezone: "UTC",
            version: 2,
            created_by: Some(U1),
            updated_by: Some(U2),
            project: PROJ,
            workspace: WS,
            owned_by: U1,
        };
        let view = cycle_base_to_representation(&row);
        assert_eq!(serialized_keys(&view), const_keys(&CYCLE_BASE_FIELDS));
        assert_eq!(
            serde_json::to_string(&view).expect("serializes"),
            "{\"id\":\"4be08458-2c0c-4d43-861c-347225b0ded4\",\
             \"created_at\":\"2026-01-02T03:04:05.123000Z\",\
             \"updated_at\":\"2026-05-06T07:08:09.456000Z\",\"deleted_at\":null,\
             \"name\":\"C1\",\"description\":\"first cycle\",\
             \"start_date\":\"2026-01-02T03:04:05.123000Z\",\
             \"end_date\":\"2026-05-06T07:08:09.456000Z\",\
             \"view_props\":{\"layout\":\"list\"},\"sort_order\":100.5,\
             \"external_source\":\"gh\",\"external_id\":\"c-1\",\
             \"progress_snapshot\":{\"done\":3},\"archived_at\":null,\
             \"logo_props\":{\"icon\":\"bolt\"},\"timezone\":\"UTC\",\"version\":2,\
             \"created_by\":\"919f5c25-56d5-44a8-9ef5-ff72e1e29cf2\",\
             \"updated_by\":\"7a62ed71-155e-4e65-a7eb-6853cf7b6261\",\
             \"project\":\"22e00942-19b1-45b1-b388-438f37942205\",\
             \"workspace\":\"994cc638-306d-4305-ad86-c4e87f829cb4\",\
             \"owned_by\":\"919f5c25-56d5-44a8-9ef5-ff72e1e29cf2\"}",
        );
    }

    #[test]
    fn module_base_replays_probe_bytes() {
        let text = serde_json::json!({"rt": true});
        let html = serde_json::json!("<p>m</p>");
        let view_props = serde_json::json!({"layout": "board"});
        let logo = serde_json::json!({});
        let row = ModuleBaseRow {
            id: "44aa8fc5-3b94-4030-902d-df376fd746ae",
            created_at: T1,
            updated_at: T2,
            deleted_at: None,
            name: "M1",
            description: "first module",
            description_text: Some(&text),
            description_html: Some(&html),
            start_date: Some("2026-02-01"),
            target_date: Some("2026-03-01"),
            status: "in-progress",
            view_props: &view_props,
            sort_order: 200.25,
            external_source: Some("gl"),
            external_id: Some("m-1"),
            archived_at: None,
            logo_props: &logo,
            created_by: Some(U1),
            updated_by: Some(U2),
            project: PROJ,
            workspace: WS,
            lead: Some(U1),
            members: vec![U2, U1],
        };
        let view = module_base_to_representation(&row);
        assert_eq!(serialized_keys(&view), const_keys(&MODULE_BASE_FIELDS));
        assert_eq!(
            serde_json::to_string(&view).expect("serializes"),
            "{\"id\":\"44aa8fc5-3b94-4030-902d-df376fd746ae\",\
             \"created_at\":\"2026-01-02T03:04:05.123000Z\",\
             \"updated_at\":\"2026-05-06T07:08:09.456000Z\",\"deleted_at\":null,\
             \"name\":\"M1\",\"description\":\"first module\",\
             \"description_text\":{\"rt\":true},\"description_html\":\"<p>m</p>\",\
             \"start_date\":\"2026-02-01\",\"target_date\":\"2026-03-01\",\
             \"status\":\"in-progress\",\"view_props\":{\"layout\":\"board\"},\
             \"sort_order\":200.25,\"external_source\":\"gl\",\"external_id\":\"m-1\",\
             \"archived_at\":null,\"logo_props\":{},\
             \"created_by\":\"919f5c25-56d5-44a8-9ef5-ff72e1e29cf2\",\
             \"updated_by\":\"7a62ed71-155e-4e65-a7eb-6853cf7b6261\",\
             \"project\":\"22e00942-19b1-45b1-b388-438f37942205\",\
             \"workspace\":\"994cc638-306d-4305-ad86-c4e87f829cb4\",\
             \"lead\":\"919f5c25-56d5-44a8-9ef5-ff72e1e29cf2\",\
             \"members\":[\"7a62ed71-155e-4e65-a7eb-6853cf7b6261\",\
             \"919f5c25-56d5-44a8-9ef5-ff72e1e29cf2\"]}",
        );
        // Null arms: nullable dates/texts/FKs null, empty members, default status.
        let empty = serde_json::json!({});
        let row = ModuleBaseRow {
            id: "e9e0a9e6-21a2-482c-8252-8951ac72e1da",
            created_at: "2026-10-03T01:19:04.968152Z",
            updated_at: "2026-10-03T01:19:04.968165Z",
            deleted_at: None,
            name: "M-null",
            description: "",
            description_text: None,
            description_html: None,
            start_date: None,
            target_date: None,
            status: "planned",
            view_props: &empty,
            sort_order: -9799.75,
            external_source: None,
            external_id: None,
            archived_at: None,
            logo_props: &empty,
            created_by: None,
            updated_by: None,
            project: PROJ,
            workspace: WS,
            lead: None,
            members: vec![],
        };
        let view = module_base_to_representation(&row);
        assert_eq!(
            serde_json::to_string(&view).expect("serializes"),
            "{\"id\":\"e9e0a9e6-21a2-482c-8252-8951ac72e1da\",\
             \"created_at\":\"2026-10-03T01:19:04.968152Z\",\
             \"updated_at\":\"2026-10-03T01:19:04.968165Z\",\"deleted_at\":null,\
             \"name\":\"M-null\",\"description\":\"\",\
             \"description_text\":null,\"description_html\":null,\
             \"start_date\":null,\"target_date\":null,\"status\":\"planned\",\
             \"view_props\":{},\"sort_order\":-9799.75,\
             \"external_source\":null,\"external_id\":null,\
             \"archived_at\":null,\"logo_props\":{},\
             \"created_by\":null,\"updated_by\":null,\
             \"project\":\"22e00942-19b1-45b1-b388-438f37942205\",\
             \"workspace\":\"994cc638-306d-4305-ad86-c4e87f829cb4\",\
             \"lead\":null,\"members\":[]}",
        );
    }

    #[test]
    fn issue_intake_replays_probe_bytes() {
        // Annotated label_ids render as a UUID-string array.
        let row = IssueIntakeRow {
            id: "e612ce6c-953c-4418-b943-55e50f416aa8",
            name: "Probe issue",
            priority: "high",
            sequence_id: 1,
            project_id: PROJ,
            created_at: T1,
            label_ids: Some(vec![L_BUG, L_URGENT]),
            created_by: Some(U1),
        };
        let view = issue_intake_to_representation(&row);
        assert_eq!(serialized_keys(&view), const_keys(&ISSUE_INTAKE_FIELDS));
        assert_eq!(
            serde_json::to_string(&view).expect("serializes"),
            "{\"id\":\"e612ce6c-953c-4418-b943-55e50f416aa8\",\
             \"name\":\"Probe issue\",\"priority\":\"high\",\"sequence_id\":1,\
             \"project_id\":\"22e00942-19b1-45b1-b388-438f37942205\",\
             \"created_at\":\"2026-01-02T03:04:05.123000Z\",\
             \"label_ids\":[\"0e1cb691-7d20-4f47-804c-4c17711db39d\",\
             \"81e4506d-fe09-4bcd-8746-94ea4e17643d\"],\
             \"created_by\":\"919f5c25-56d5-44a8-9ef5-ff72e1e29cf2\"}",
        );
        // Missing annotation omits the key (SkipField); null creator stays.
        let row = IssueIntakeRow {
            id: "5c29298a-177a-4813-b838-74ed40570cc3",
            name: "Bare",
            priority: "none",
            sequence_id: 2,
            project_id: PROJ,
            created_at: "2026-10-03T01:19:05.068552Z",
            label_ids: None,
            created_by: None,
        };
        let view = issue_intake_to_representation(&row);
        let rendered = serde_json::to_string(&view).expect("serializes");
        assert_eq!(
            rendered,
            "{\"id\":\"5c29298a-177a-4813-b838-74ed40570cc3\",\
             \"name\":\"Bare\",\"priority\":\"none\",\"sequence_id\":2,\
             \"project_id\":\"22e00942-19b1-45b1-b388-438f37942205\",\
             \"created_at\":\"2026-10-03T01:19:05.068552Z\",\"created_by\":null}",
        );
        assert!(
            !rendered.contains("label_ids"),
            "unannotated arm omits the key"
        );
        // Annotated-but-empty renders a present [].
        let row = IssueIntakeRow {
            id: "e612ce6c-953c-4418-b943-55e50f416aa8",
            name: "Probe issue",
            priority: "high",
            sequence_id: 1,
            project_id: PROJ,
            created_at: T1,
            label_ids: Some(vec![]),
            created_by: Some(U1),
        };
        let view = issue_intake_to_representation(&row);
        assert_eq!(
            serde_json::to_string(&view).expect("serializes"),
            "{\"id\":\"e612ce6c-953c-4418-b943-55e50f416aa8\",\
             \"name\":\"Probe issue\",\"priority\":\"high\",\"sequence_id\":1,\
             \"project_id\":\"22e00942-19b1-45b1-b388-438f37942205\",\
             \"created_at\":\"2026-01-02T03:04:05.123000Z\",\
             \"label_ids\":[],\
             \"created_by\":\"919f5c25-56d5-44a8-9ef5-ff72e1e29cf2\"}",
        );
    }

    fn full_state_row<'a>(logo: &'a Value, description_json: &'a Value) -> IssueStateRow<'a> {
        IssueStateRow {
            id: ISS,
            label_details: vec![
                LabelLiteRow {
                    id: L_URGENT,
                    name: "urgent",
                    color: "#0000ff",
                },
                LabelLiteRow {
                    id: L_BUG,
                    name: "bug",
                    color: "#00ff00",
                },
            ],
            state_detail: Some(state_row()),
            project_detail: project_row(logo),
            assignee_details: vec![user_row_alan(), user_row_ada()],
            sub_issues_count: Some(4),
            attachment_count: Some(1),
            link_count: Some(9),
            created_at: T1,
            updated_at: T2,
            deleted_at: None,
            point: Some(5),
            name: "Probe issue",
            description_json,
            description_html: "<p>hi</p>",
            description_stripped: Some("hi"),
            description_binary: None,
            priority: "high",
            complexity_score: 7,
            start_date: Some("2026-02-02"),
            target_date: Some("2026-04-04"),
            sequence_id: 1,
            sort_order: 65535.0,
            completed_at: Some(T2),
            archived_at: None,
            is_draft: false,
            external_source: Some("gh"),
            external_id: Some("i-9"),
            git_work_branch: "pi-dash/probe",
            created_via: Some("assistant"),
            agent_executor: Some("local_runner"),
            created_by: Some(U1),
            updated_by: Some(U2),
            project: PROJ,
            workspace: WS,
            parent: None,
            state: Some(ST),
            estimate_point: None,
            issue_type: None,
            assigned_pod: Some(POD),
            assignees: vec![U2, U1],
            labels: vec![L_URGENT, L_BUG],
        }
    }

    #[test]
    fn issue_state_replays_probe_bytes() {
        let logo = serde_json::json!({"theme": "dark"});
        let description_json = serde_json::json!({"doc": [1]});
        let row = full_state_row(&logo, &description_json);
        let view = issue_state_to_representation(&row);
        assert_eq!(serialized_keys(&view), const_keys(&ISSUE_STATE_FIELDS));
        assert_eq!(
            serialized_keys(&view.label_details[0]),
            const_keys(&LABEL_LITE_FIELDS),
            "nested keys follow app LabelLiteSerializer order",
        );
        assert_eq!(
            serialized_keys(view.state_detail.as_ref().expect("state nest present")),
            const_keys(&STATE_LITE_FIELDS),
            "nested keys follow app StateLiteSerializer order",
        );
        assert_eq!(
            serialized_keys(&view.project_detail),
            const_keys(&PROJECT_LITE_FIELDS),
            "nested keys follow app ProjectLiteSerializer order",
        );
        assert_eq!(
            serialized_keys(&view.assignee_details[0]),
            const_keys(&USER_LITE_FIELDS),
            "nested keys follow app UserLiteSerializer order",
        );
        assert_eq!(
            serde_json::to_string(&view).expect("serializes"),
            "{\"id\":\"e612ce6c-953c-4418-b943-55e50f416aa8\",\
             \"label_details\":[{\"id\":\"81e4506d-fe09-4bcd-8746-94ea4e17643d\",\
             \"name\":\"urgent\",\"color\":\"#0000ff\"},\
             {\"id\":\"0e1cb691-7d20-4f47-804c-4c17711db39d\",\
             \"name\":\"bug\",\"color\":\"#00ff00\"}],\
             \"state_detail\":{\"id\":\"539afd47-4ad4-4cdc-bf26-8d463178faf8\",\
             \"name\":\"In Progress\",\"color\":\"#ff0000\",\"group\":\"started\"},\
             \"project_detail\":{\"id\":\"22e00942-19b1-45b1-b388-438f37942205\",\
             \"identifier\":\"PRB\",\"name\":\"Probe Proj\",\
             \"cover_image\":\"https://cdn/x/cover.png\",\
             \"cover_image_url\":\"https://cdn/x/cover.png\",\
             \"logo_props\":{\"theme\":\"dark\"},\"description\":\"probe project\",\
             \"is_default\":true},\
             \"assignee_details\":[{\"id\":\"7a62ed71-155e-4e65-a7eb-6853cf7b6261\",\
             \"first_name\":\"Alan\",\"last_name\":\"Turing\",\"avatar\":\"\",\
             \"avatar_url\":null,\"is_bot\":false,\"display_name\":\"probe-u2-643\"},\
             {\"id\":\"919f5c25-56d5-44a8-9ef5-ff72e1e29cf2\",\
             \"first_name\":\"Ada\",\"last_name\":\"Lovelace\",\
             \"avatar\":\"https://cdn/x/a1.png\",\"avatar_url\":\"https://cdn/x/a1.png\",\
             \"is_bot\":false,\"display_name\":\"probe-u1-643\"}],\
             \"sub_issues_count\":4,\"attachment_count\":1,\"link_count\":9,\
             \"created_at\":\"2026-01-02T03:04:05.123000Z\",\
             \"updated_at\":\"2026-05-06T07:08:09.456000Z\",\"deleted_at\":null,\
             \"point\":5,\"name\":\"Probe issue\",\"description_json\":{\"doc\":[1]},\
             \"description_html\":\"<p>hi</p>\",\"description_stripped\":\"hi\",\
             \"description_binary\":null,\"priority\":\"high\",\"complexity_score\":7,\
             \"start_date\":\"2026-02-02\",\"target_date\":\"2026-04-04\",\
             \"sequence_id\":1,\"sort_order\":65535.0,\
             \"completed_at\":\"2026-05-06T07:08:09.456000Z\",\"archived_at\":null,\
             \"is_draft\":false,\"external_source\":\"gh\",\"external_id\":\"i-9\",\
             \"git_work_branch\":\"pi-dash/probe\",\"created_via\":\"assistant\",\
             \"agent_executor\":\"local_runner\",\
             \"created_by\":\"919f5c25-56d5-44a8-9ef5-ff72e1e29cf2\",\
             \"updated_by\":\"7a62ed71-155e-4e65-a7eb-6853cf7b6261\",\
             \"project\":\"22e00942-19b1-45b1-b388-438f37942205\",\
             \"workspace\":\"994cc638-306d-4305-ad86-c4e87f829cb4\",\
             \"parent\":null,\"state\":\"539afd47-4ad4-4cdc-bf26-8d463178faf8\",\
             \"estimate_point\":null,\"type\":null,\
             \"assigned_pod\":\"32f13029-e8bf-4f39-a97e-dca9ab90b635\",\
             \"assignees\":[\"7a62ed71-155e-4e65-a7eb-6853cf7b6261\",\
             \"919f5c25-56d5-44a8-9ef5-ff72e1e29cf2\"],\
             \"labels\":[\"81e4506d-fe09-4bcd-8746-94ea4e17643d\",\
             \"0e1cb691-7d20-4f47-804c-4c17711db39d\"]}",
        );
    }

    #[test]
    fn issue_state_bare_omits_unannotated_counts() {
        let logo = serde_json::json!({"theme": "dark"});
        let empty = serde_json::json!({});
        let row = IssueStateRow {
            id: "5c29298a-177a-4813-b838-74ed40570cc3",
            label_details: vec![],
            state_detail: Some(state_row()),
            project_detail: project_row(&logo),
            assignee_details: vec![],
            sub_issues_count: None,
            attachment_count: None,
            link_count: None,
            created_at: "2026-10-03T01:19:05.068552Z",
            updated_at: "2026-10-03T01:19:05.068568Z",
            deleted_at: None,
            point: None,
            name: "Bare",
            description_json: &empty,
            description_html: "<p></p>",
            description_stripped: Some(""),
            description_binary: None,
            priority: "none",
            complexity_score: 0,
            start_date: None,
            target_date: None,
            sequence_id: 2,
            sort_order: 75535.0,
            completed_at: None,
            archived_at: None,
            is_draft: false,
            external_source: None,
            external_id: None,
            git_work_branch: "",
            created_via: None,
            agent_executor: None,
            created_by: None,
            updated_by: None,
            project: PROJ,
            workspace: WS,
            parent: None,
            state: Some(ST),
            estimate_point: None,
            issue_type: None,
            assigned_pod: Some(POD),
            assignees: vec![],
            labels: vec![],
        };
        let view = issue_state_to_representation(&row);
        let rendered = serde_json::to_string(&view).expect("serializes");
        assert_eq!(
            rendered,
            "{\"id\":\"5c29298a-177a-4813-b838-74ed40570cc3\",\
             \"label_details\":[],\
             \"state_detail\":{\"id\":\"539afd47-4ad4-4cdc-bf26-8d463178faf8\",\
             \"name\":\"In Progress\",\"color\":\"#ff0000\",\"group\":\"started\"},\
             \"project_detail\":{\"id\":\"22e00942-19b1-45b1-b388-438f37942205\",\
             \"identifier\":\"PRB\",\"name\":\"Probe Proj\",\
             \"cover_image\":\"https://cdn/x/cover.png\",\
             \"cover_image_url\":\"https://cdn/x/cover.png\",\
             \"logo_props\":{\"theme\":\"dark\"},\"description\":\"probe project\",\
             \"is_default\":true},\"assignee_details\":[],\
             \"created_at\":\"2026-10-03T01:19:05.068552Z\",\
             \"updated_at\":\"2026-10-03T01:19:05.068568Z\",\"deleted_at\":null,\
             \"point\":null,\"name\":\"Bare\",\"description_json\":{},\
             \"description_html\":\"<p></p>\",\"description_stripped\":\"\",\
             \"description_binary\":null,\"priority\":\"none\",\"complexity_score\":0,\
             \"start_date\":null,\"target_date\":null,\
             \"sequence_id\":2,\"sort_order\":75535.0,\
             \"completed_at\":null,\"archived_at\":null,\"is_draft\":false,\
             \"external_source\":null,\"external_id\":null,\"git_work_branch\":\"\",\
             \"created_via\":null,\"agent_executor\":null,\
             \"created_by\":null,\"updated_by\":null,\
             \"project\":\"22e00942-19b1-45b1-b388-438f37942205\",\
             \"workspace\":\"994cc638-306d-4305-ad86-c4e87f829cb4\",\
             \"parent\":null,\"state\":\"539afd47-4ad4-4cdc-bf26-8d463178faf8\",\
             \"estimate_point\":null,\"type\":null,\
             \"assigned_pod\":\"32f13029-e8bf-4f39-a97e-dca9ab90b635\",\
             \"assignees\":[],\"labels\":[]}",
        );
        assert!(
            !rendered.contains("sub_issues_count")
                && !rendered.contains("attachment_count")
                && !rendered.contains("link_count"),
            "unannotated counters are omitted, not null"
        );
    }

    #[test]
    fn issue_state_binary_renders_base64() {
        // Non-null BinaryField renders standard base64 ASCII
        // (BinaryField.value_to_string); the caller encodes.
        let logo = serde_json::json!({"theme": "dark"});
        let description_json = serde_json::json!({"doc": [1]});
        let mut row = full_state_row(&logo, &description_json);
        row.description_binary = Some("AAFiaW4=");
        row.sub_issues_count = Some(0);
        row.attachment_count = Some(0);
        row.link_count = Some(0);
        let view = issue_state_to_representation(&row);
        let rendered = serde_json::to_string(&view).expect("serializes");
        assert!(
            rendered.contains("\"description_binary\":\"AAFiaW4=\""),
            "base64 payload passes through verbatim"
        );
        assert!(
            rendered.contains("\"sub_issues_count\":0,\"attachment_count\":0,\"link_count\":0"),
            "annotated zero counters render present"
        );
    }

    #[test]
    fn issue_state_null_state_renders_null_nest() {
        // state=None short-circuits the nest to a present null (no skip);
        // set parent/estimate_point/type arms included.
        let logo = serde_json::json!({"theme": "dark"});
        let empty = serde_json::json!({});
        let row = IssueStateRow {
            id: "6497b675-a005-4395-89ce-f9d095fa419f",
            label_details: vec![],
            state_detail: None,
            project_detail: project_row(&logo),
            assignee_details: vec![],
            sub_issues_count: None,
            attachment_count: None,
            link_count: None,
            created_at: "2026-10-03T01:20:41.330105Z",
            updated_at: "2026-10-03T01:20:41.330119Z",
            deleted_at: None,
            point: None,
            name: "Third",
            description_json: &empty,
            description_html: "<p></p>",
            description_stripped: Some(""),
            description_binary: None,
            priority: "none",
            complexity_score: 0,
            start_date: None,
            target_date: None,
            sequence_id: 3,
            sort_order: 85535.0,
            completed_at: None,
            archived_at: Some("2026-07-08"),
            is_draft: false,
            external_source: None,
            external_id: None,
            git_work_branch: "",
            created_via: None,
            agent_executor: None,
            created_by: None,
            updated_by: None,
            project: PROJ,
            workspace: WS,
            parent: Some(ISS),
            state: None,
            estimate_point: Some(EP),
            issue_type: Some(IT),
            assigned_pod: None,
            assignees: vec![],
            labels: vec![],
        };
        let view = issue_state_to_representation(&row);
        assert_eq!(
            serde_json::to_string(&view).expect("serializes"),
            "{\"id\":\"6497b675-a005-4395-89ce-f9d095fa419f\",\
             \"label_details\":[],\"state_detail\":null,\
             \"project_detail\":{\"id\":\"22e00942-19b1-45b1-b388-438f37942205\",\
             \"identifier\":\"PRB\",\"name\":\"Probe Proj\",\
             \"cover_image\":\"https://cdn/x/cover.png\",\
             \"cover_image_url\":\"https://cdn/x/cover.png\",\
             \"logo_props\":{\"theme\":\"dark\"},\"description\":\"probe project\",\
             \"is_default\":true},\"assignee_details\":[],\
             \"created_at\":\"2026-10-03T01:20:41.330105Z\",\
             \"updated_at\":\"2026-10-03T01:20:41.330119Z\",\"deleted_at\":null,\
             \"point\":null,\"name\":\"Third\",\"description_json\":{},\
             \"description_html\":\"<p></p>\",\"description_stripped\":\"\",\
             \"description_binary\":null,\"priority\":\"none\",\"complexity_score\":0,\
             \"start_date\":null,\"target_date\":null,\
             \"sequence_id\":3,\"sort_order\":85535.0,\
             \"completed_at\":null,\"archived_at\":\"2026-07-08\",\"is_draft\":false,\
             \"external_source\":null,\"external_id\":null,\"git_work_branch\":\"\",\
             \"created_via\":null,\"agent_executor\":null,\
             \"created_by\":null,\"updated_by\":null,\
             \"project\":\"22e00942-19b1-45b1-b388-438f37942205\",\
             \"workspace\":\"994cc638-306d-4305-ad86-c4e87f829cb4\",\
             \"parent\":\"e612ce6c-953c-4418-b943-55e50f416aa8\",\"state\":null,\
             \"estimate_point\":\"ac462fe7-dd34-4b65-8a41-6ab7c90feba9\",\
             \"type\":\"5396e0eb-8629-4b0a-afd2-ca59f4b1dbf4\",\
             \"assigned_pod\":null,\"assignees\":[],\"labels\":[]}",
        );
    }
}
