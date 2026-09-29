//! D-29 view + favorite serializers: JSON shapes and write-path kernels.
//!
//! Port of `apps/api/pi_dash/app/serializers/view.py` (86 lines) and
//! `apps/api/pi_dash/app/serializers/favorite.py` (`ViewFavoriteSerializer`
//! `:40-43`, the entity map `:46-56`, `UserFavoriteSerializer` `:59-89`),
//! over `DynamicBaseSerializer` (`app/serializers/base.py:12-24`).
//!
//! * `view.py:14-53` (`ViewIssueListSerializer`) — plain `Serializer` with a
//!   hand-written `to_representation`: the 26-key row shape of the
//!   view-issues list.
//! * `view.py:56-86` (`IssueViewSerializer`) — `ModelSerializer`,
//!   `fields = "__all__"` plus the declared `is_favorite` boolean, with
//!   `create` / `update` query kernels.
//! * `favorite.py:40-43` (`ViewFavoriteSerializer`) — 4-key `IssueView` lite.
//! * `favorite.py:46-89` (entity map + `UserFavoriteSerializer`) — 10-key
//!   shape whose `entity_data` resolves through the entity map.
//!
//! Wire rules (all verified against live DRF 3.18.1 through the repo venv,
//! not assumed from the fixture prose):
//!
//! * Key order is DRF order. For `IssueViewSerializer` that is `id`, then the
//!   declared `is_favorite`, then the model fields in Django `_meta` order:
//!   `created_at`, `updated_at`, `deleted_at`, `name`, `description`,
//!   `query`, `filters`, `display_filters`, `display_properties`,
//!   `rich_filters`, `access`, `sort_order`, `logo_props`, `is_locked`,
//!   `archived_at`, `created_by`, `updated_by`, `workspace`, `project`,
//!   `owned_by`. (Audit FKs trail the view's own columns; `id` leads.)
//! * `is_favorite` is a read-only `BooleanField` fed by the
//!   `Exists(UserFavorite …)` annotation on project-view querysets. When the
//!   queryset does not annotate it, DRF raises `SkipField` and the key is
//!   absent (workspace views, create responses) — not `null`. Here that is
//!   `Option<bool>` with `skip_serializing_if`, sitting at index 1 when
//!   present.
//! * UUID and FK primary keys render as strings (`PrimaryKeyRelatedField`,
//!   read-only); a null FK renders `null`. `estimate_point` renders
//!   `estimate_point_id`; `created_by` / `updated_by` render `*_id`.
//! * Datetimes and dates cross this boundary already rendered as DRF
//!   `iso-8601` strings — formatting owns to the DB edge, so rendering here
//!   is a byte-exact passthrough. The live rule (probed): `+00:00` becomes
//!   `Z`, microseconds print only when nonzero (`2026-09-01T10:00:00Z` vs
//!   `2026-09-02T10:00:00.123456Z`). FX-SER.json's `output_example` shows an
//!   illustrative `.000000Z`; live DRF never emits a zero fraction, and this
//!   port follows live DRF.
//! * Floats (`sort_order`, `sequence`) render as JSON numbers (`65535.0`);
//!   `access` renders as an int; JSON columns pass through verbatim.
//!
//! Ported bugs (translate, don't redesign):
//!
//! * B2 (`base.py:12-18`): `DynamicBaseSerializer.__init__` pops the
//!   `fields` kwarg and immediately overwrites it with `self.expand`, so
//!   `?fields=` on both view list views (`app/views/view/base.py:74-77,
//!   :304-305`, the only call sites — `expand=` is never passed) is dead and
//!   full objects always render. [`effective_selection`] ports the overwrite.
//! * B3 (`view.py:79-86`): `IssueViewSerializer.update` assigns
//!   `validated_data["query"]` twice — first `issue_filters(filters,
//!   "POST")`, then unconditionally `issue_filters(query_params, "PATCH")`.
//!   The POST line is dead; the effective query is always the PATCH mapping,
//!   even when `filters` is empty or missing. [`resolve_update_query`] ports
//!   the final value only.
//!
//! Observed quirks, ported as documented behavior (all probed live):
//!
//! * An `expand` name that is neither a serializer field nor in the
//!   expansion map is silently ignored — no key is added.
//! * An `expand` name that IS a serializer field but has no expansion entry
//!   (e.g. `name`) overwrites the rendered value with
//!   `getattr(instance, "<name>_id", None)` — `None` for `IssueView`, so
//!   `expand=name` nulls the name. [`expand_outcome`] encodes the decision.
//! * Expanding a relation whose object is missing raises in Python
//!   (`RelatedObjectDoesNotExist`, a 500); callers resolve the object before
//!   building the row, like the Python attribute access does.
//!
//! Shape-only no-ops preserved as documentation, not code:
//! `read_only_fields` (`view.py:62-69`, `favorite.py:72`) constrain writes,
//! of which the shape ports have none. `IssueView.save()` recomputes `query`
//! the same way `create` does (`db/models/view.py:79-81`); the save path
//! belongs to the models layer (PIDASHCONV-270).

use serde::Serialize;
use serde_json::Value;

/// `ViewIssueListSerializer.to_representation` keys (`view.py:30-55`), in
/// source order. `estimate_point` renders `estimate_point_id`,
/// `created_by` / `updated_by` render `*_id`, `state__group` is None-guarded
/// (`instance.state.group if instance.state else None`), and the three id
/// arrays come from the prefetched relations.
pub const VIEW_ISSUE_LIST_FIELDS: [&str; 26] = [
    "id",
    "name",
    "state_id",
    "sort_order",
    "completed_at",
    "estimate_point",
    "priority",
    "start_date",
    "target_date",
    "sequence_id",
    "project_id",
    "parent_id",
    "cycle_id",
    "sub_issues_count",
    "created_at",
    "updated_at",
    "created_by",
    "updated_by",
    "attachment_count",
    "link_count",
    "is_draft",
    "archived_at",
    "state__group",
    "assignee_ids",
    "label_ids",
    "module_ids",
];

/// `IssueViewSerializer` wire keys (`view.py:56-69`, `fields = "__all__"`),
/// in live DRF order (probed, not inferred): `id` first, then the declared
/// `is_favorite` when annotated, then Django `_meta` order. This is the
/// 21-key set pinned by `GLOBAL_VIEW_KEYS` in
/// `contract-tests/app_views_search/test_global_views.py`.
pub const ISSUE_VIEW_FIELDS: [&str; 21] = [
    "id",
    "created_at",
    "updated_at",
    "deleted_at",
    "name",
    "description",
    "query",
    "filters",
    "display_filters",
    "display_properties",
    "rich_filters",
    "access",
    "sort_order",
    "logo_props",
    "is_locked",
    "archived_at",
    "created_by",
    "updated_by",
    "workspace",
    "project",
    "owned_by",
];

/// The declared extra field on `IssueViewSerializer` (`view.py:57`).
pub const IS_FAVORITE_FIELD: &str = "is_favorite";

/// Wire index of `is_favorite` when the annotation is present (probed: it
/// renders immediately after `id`).
pub const IS_FAVORITE_INDEX: usize = 1;

/// `ViewFavoriteSerializer` keys (`favorite.py:40-43`, `Meta.fields` order).
pub const VIEW_FAVORITE_FIELDS: [&str; 4] = ["id", "name", "logo_props", "project_id"];

/// `UserFavoriteSerializer` keys (`favorite.py:62-72`, `Meta.fields` order).
pub const USER_FAVORITE_FIELDS: [&str; 10] = [
    "id",
    "entity_type",
    "entity_identifier",
    "entity_data",
    "name",
    "is_folder",
    "sequence",
    "parent",
    "workspace_id",
    "project_id",
];

/// Expansion-map names that serialize `many=True` (`base.py`, both the
/// `_filter_fields` and the `to_representation` maps).
pub const EXPANSION_MANY_FIELDS: &[&str] = &[
    "members",
    "assignees",
    "labels",
    "issue_cycle",
    "issue_relation",
    "issue_intake",
    "issue_reactions",
    "issue_attachment",
    "issue_link",
    "sub_issues",
    "issue_related",
];

/// Every key of the `_filter_fields` / `to_representation` expansion maps
/// (`base.py`), for reference: a selected name outside this map falls back
/// to the `<name>_id` attribute (or is ignored when it is not a field).
pub const EXPANSION_FIELDS: &[&str] = &[
    "user",
    "workspace",
    "project",
    "default_assignee",
    "project_lead",
    "state",
    "created_by",
    "issue",
    "actor",
    "owned_by",
    "members",
    "assignees",
    "labels",
    "issue_cycle",
    "parent",
    "issue_relation",
    "issue_intake",
    "issue_related",
    "issue_reactions",
    "issue_attachment",
    "issue_link",
    "sub_issues",
];

/// A database row for the view-issues list shape. Datetimes cross already
/// rendered as DRF `iso-8601` strings; id arrays come from the prefetched
/// `issue_assignee` / `label_issue` / `issue_module` relations
/// (`view.py:14-27`).
#[derive(Debug, Clone, PartialEq)]
pub struct ViewIssueListRow<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub state_id: Option<&'a str>,
    pub sort_order: f64,
    pub completed_at: Option<&'a str>,
    pub estimate_point: Option<&'a str>,
    pub priority: &'a str,
    pub start_date: Option<&'a str>,
    pub target_date: Option<&'a str>,
    pub sequence_id: i64,
    pub project_id: Option<&'a str>,
    pub parent_id: Option<&'a str>,
    pub cycle_id: Option<&'a str>,
    pub sub_issues_count: i64,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub attachment_count: i64,
    pub link_count: i64,
    pub is_draft: bool,
    pub archived_at: Option<&'a str>,
    pub state_group: Option<&'a str>,
    pub assignee_ids: Vec<&'a str>,
    pub label_ids: Vec<&'a str>,
    pub module_ids: Vec<&'a str>,
}

/// `ViewIssueListSerializer.to_representation` output (`view.py:28-55`):
/// the 26 keys in source order. `state__group` is the None-guarded
/// `instance.state.group`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ViewIssueListView<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub state_id: Option<&'a str>,
    pub sort_order: f64,
    pub completed_at: Option<&'a str>,
    pub estimate_point: Option<&'a str>,
    pub priority: &'a str,
    pub start_date: Option<&'a str>,
    pub target_date: Option<&'a str>,
    pub sequence_id: i64,
    pub project_id: Option<&'a str>,
    pub parent_id: Option<&'a str>,
    pub cycle_id: Option<&'a str>,
    pub sub_issues_count: i64,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub attachment_count: i64,
    pub link_count: i64,
    pub is_draft: bool,
    pub archived_at: Option<&'a str>,
    #[serde(rename = "state__group")]
    pub state_group: Option<&'a str>,
    pub assignee_ids: Vec<&'a str>,
    pub label_ids: Vec<&'a str>,
    pub module_ids: Vec<&'a str>,
}

/// Build the view-issues list representation from a row.
pub fn view_issue_list_to_representation<'a>(
    row: &'a ViewIssueListRow<'a>,
) -> ViewIssueListView<'a> {
    ViewIssueListView {
        id: row.id,
        name: row.name,
        state_id: row.state_id,
        sort_order: row.sort_order,
        completed_at: row.completed_at,
        estimate_point: row.estimate_point,
        priority: row.priority,
        start_date: row.start_date,
        target_date: row.target_date,
        sequence_id: row.sequence_id,
        project_id: row.project_id,
        parent_id: row.parent_id,
        cycle_id: row.cycle_id,
        sub_issues_count: row.sub_issues_count,
        created_at: row.created_at,
        updated_at: row.updated_at,
        created_by: row.created_by,
        updated_by: row.updated_by,
        attachment_count: row.attachment_count,
        link_count: row.link_count,
        is_draft: row.is_draft,
        archived_at: row.archived_at,
        state_group: row.state_group,
        assignee_ids: row.assignee_ids.clone(),
        label_ids: row.label_ids.clone(),
        module_ids: row.module_ids.clone(),
    }
}

/// A database row for the full view shape. JSON columns borrow the caller's
/// parsed values and pass through verbatim; datetimes borrow pre-rendered
/// DRF strings; `is_favorite` borrows the `Exists` annotation when the
/// queryset provides it (`None` on workspace views and create responses).
#[derive(Debug, Clone, PartialEq)]
pub struct IssueViewRow<'a> {
    pub id: &'a str,
    pub is_favorite: Option<bool>,
    pub created_at: Option<&'a str>,
    pub updated_at: Option<&'a str>,
    pub deleted_at: Option<&'a str>,
    pub name: &'a str,
    pub description: &'a str,
    pub query: &'a Value,
    pub filters: &'a Value,
    pub display_filters: &'a Value,
    pub display_properties: &'a Value,
    pub rich_filters: &'a Value,
    pub access: i32,
    pub sort_order: f64,
    pub logo_props: &'a Value,
    pub is_locked: bool,
    pub archived_at: Option<&'a str>,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub workspace: Option<&'a str>,
    pub project: Option<&'a str>,
    pub owned_by: Option<&'a str>,
}

/// `IssueViewSerializer.to_representation` output: the 21 model keys in live
/// DRF order with `is_favorite` skipped when `None` (DRF `SkipField` → the
/// key is absent, not `null`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct IssueViewView<'a> {
    pub id: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_favorite: Option<bool>,
    pub created_at: Option<&'a str>,
    pub updated_at: Option<&'a str>,
    pub deleted_at: Option<&'a str>,
    pub name: &'a str,
    pub description: &'a str,
    pub query: &'a Value,
    pub filters: &'a Value,
    pub display_filters: &'a Value,
    pub display_properties: &'a Value,
    pub rich_filters: &'a Value,
    pub access: i32,
    pub sort_order: f64,
    pub logo_props: &'a Value,
    pub is_locked: bool,
    pub archived_at: Option<&'a str>,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub workspace: Option<&'a str>,
    pub project: Option<&'a str>,
    pub owned_by: Option<&'a str>,
}

/// Build the full view representation from a row.
pub fn issue_view_to_representation<'a>(row: &'a IssueViewRow<'a>) -> IssueViewView<'a> {
    IssueViewView {
        id: row.id,
        is_favorite: row.is_favorite,
        created_at: row.created_at,
        updated_at: row.updated_at,
        deleted_at: row.deleted_at,
        name: row.name,
        description: row.description,
        query: row.query,
        filters: row.filters,
        display_filters: row.display_filters,
        display_properties: row.display_properties,
        rich_filters: row.rich_filters,
        access: row.access,
        sort_order: row.sort_order,
        logo_props: row.logo_props,
        is_locked: row.is_locked,
        archived_at: row.archived_at,
        created_by: row.created_by,
        updated_by: row.updated_by,
        workspace: row.workspace,
        project: row.project,
        owned_by: row.owned_by,
    }
}

/// A database row for the view-favorite lite shape.
#[derive(Debug, Clone, PartialEq)]
pub struct ViewFavoriteRow<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub logo_props: &'a Value,
    pub project_id: Option<&'a str>,
}

/// `ViewFavoriteSerializer` output (`favorite.py:40-43`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ViewFavoriteView<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub logo_props: &'a Value,
    pub project_id: Option<&'a str>,
}

/// Build the view-favorite lite representation from a row.
pub fn view_favorite_to_representation<'a>(row: &'a ViewFavoriteRow<'a>) -> ViewFavoriteView<'a> {
    ViewFavoriteView {
        id: row.id,
        name: row.name,
        logo_props: row.logo_props,
        project_id: row.project_id,
    }
}

/// A database row for the user-favorite shape. `entity_data` carries the
/// already-resolved nest (or `None`); resolution itself is
/// [`resolve_entity_data`].
#[derive(Debug, Clone, PartialEq)]
pub struct UserFavoriteRow<'a> {
    pub id: &'a str,
    pub entity_type: &'a str,
    pub entity_identifier: Option<&'a str>,
    pub entity_data: Option<&'a Value>,
    pub name: Option<&'a str>,
    pub is_folder: bool,
    pub sequence: f64,
    pub parent: Option<&'a str>,
    pub workspace_id: Option<&'a str>,
    pub project_id: Option<&'a str>,
}

/// `UserFavoriteSerializer` output (`favorite.py:59-89`). `entity_data` is a
/// `SerializerMethodField`: the key is always present, `None` renders
/// `null`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct UserFavoriteView<'a> {
    pub id: &'a str,
    pub entity_type: &'a str,
    pub entity_identifier: Option<&'a str>,
    pub entity_data: Option<&'a Value>,
    pub name: Option<&'a str>,
    pub is_folder: bool,
    pub sequence: f64,
    pub parent: Option<&'a str>,
    pub workspace_id: Option<&'a str>,
    pub project_id: Option<&'a str>,
}

/// Build the user-favorite representation from a row.
pub fn user_favorite_to_representation<'a>(row: &'a UserFavoriteRow<'a>) -> UserFavoriteView<'a> {
    UserFavoriteView {
        id: row.id,
        entity_type: row.entity_type,
        entity_identifier: row.entity_identifier,
        entity_data: row.entity_data,
        name: row.name,
        is_folder: row.is_folder,
        sequence: row.sequence,
        parent: row.parent,
        workspace_id: row.workspace_id,
        project_id: row.project_id,
    }
}

/// The B2 selection rule (`base.py:12-18`): the `fields` constructor kwarg is
/// accepted and then discarded — `fields = self.expand`. The effective
/// selection is the `expand` list; `None` (or empty) means the full shape.
pub fn effective_selection(
    _fields: Option<Vec<String>>,
    expand: Option<Vec<String>>,
) -> Option<Vec<String>> {
    expand
}

/// `IssueViewSerializer.create` query kernel (`view.py:71-77`): a non-empty
/// `filters` maps through `issue_filters(filters, "POST")`, otherwise the
/// query is `{}`. The `issue_filters` computation itself belongs to the
/// queries layer; this ports the branch. `compute_post` is only invoked on
/// the non-empty path.
pub fn resolve_create_query(
    filters: Option<&Value>,
    compute_post: impl FnOnce(&Value) -> Value,
) -> Value {
    match filters {
        Some(value) if is_truthy_json(value) => compute_post(value),
        _ => empty_query(),
    }
}

/// `IssueViewSerializer.update` query kernel (`view.py:79-86`, B3): the
/// `issue_filters(query_params, "PATCH")` assignment runs unconditionally
/// and overwrites the POST line, so the effective query is always the PATCH
/// mapping — even when `filters` is empty or missing. The POST computation
/// never survives; it is not invoked here at all.
///
/// `query_params` reproduces `validated_data.get("filters", {})`: a missing
/// `filters` key arrives as `None` and maps to the empty query input (`{}`),
/// exactly what Python hands `issue_filters`. Only an explicitly present
/// value — including JSON `null`, which makes Python's `key in query_params`
/// raise — is forwarded to the PATCH computation as-is.
pub fn resolve_update_query(
    query_params: Option<&Value>,
    compute_patch: impl FnOnce(&Value) -> Value,
) -> Value {
    match query_params {
        Some(params) => compute_patch(params),
        None => compute_patch(&Value::Object(serde_json::Map::new())),
    }
}

/// The empty-query literal both write kernels fall back to (`{}`).
pub fn empty_query() -> Value {
    Value::Object(serde_json::Map::new())
}

/// Python `bool()` over JSON values, for the `if bool(query_params)` branch
/// in `create` (`view.py:73`): only a non-empty container / nonzero number /
/// non-empty string / `true` takes the mapping path.
pub fn is_truthy_json(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => number.as_f64().is_some_and(|float| float != 0.0),
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(fields) => !fields.is_empty(),
    }
}

/// Which side of the entity map (`favorite.py:46-56`) an `entity_type` falls
/// on. `cycle`, `module`, `view`, `page` and `project` resolve a model plus
/// a lite serializer; `issue` maps to `(Issue, None)`, `folder` to
/// `(None, None)`, and anything unknown misses the map — all three render
/// `entity_data: null` unconditionally.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityKind {
    Cycle,
    Module,
    View,
    Page,
    Project,
    AlwaysNone,
}

/// Classify an `entity_type` per the entity map.
pub fn entity_kind(entity_type: &str) -> EntityKind {
    match entity_type {
        "cycle" => EntityKind::Cycle,
        "module" => EntityKind::Module,
        "view" => EntityKind::View,
        "page" => EntityKind::Page,
        "project" => EntityKind::Project,
        _ => EntityKind::AlwaysNone,
    }
}

/// `UserFavoriteSerializer.get_entity_data` (`favorite.py:78-89`): resolvable
/// kinds render the fetched entity's lite shape; a `DoesNotExist` miss
/// renders `None`. `AlwaysNone` kinds (`issue`, `folder`, unknown) render
/// `None` without touching the database. `found` is the caller's fetched
/// lite value (`None` when the row does not exist).
pub fn resolve_entity_data(kind: EntityKind, found: Option<Value>) -> Option<Value> {
    match kind {
        EntityKind::AlwaysNone => None,
        _ => found,
    }
}

/// What `to_representation` (`base.py`) does with one `expand` name:
/// [`ExpandOutcome::Nested`] replaces the value with its nested serialization
/// (`many=True` for [`EXPANSION_MANY_FIELDS`]); [`ExpandOutcome::Fallback`]
/// overwrites it with the `<name>_id` attribute (probed: `expand=name`
/// nulls the name on `IssueView`); [`ExpandOutcome::Ignored`] leaves the
/// response untouched (the name is not a serializer field, so the
/// `if expand in self.fields` gate skips it — probed with `expand=bogus`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpandOutcome {
    Nested,
    Fallback,
    Ignored,
}

/// Classify one `expand` name for a serializer declaring `fields`.
pub fn expand_outcome(name: &str, fields: &[&str]) -> ExpandOutcome {
    if !fields.contains(&name) {
        return ExpandOutcome::Ignored;
    }
    if EXPANSION_FIELDS.contains(&name) {
        return ExpandOutcome::Nested;
    }
    ExpandOutcome::Fallback
}

/// The `<expand>_id` attribute name used by the fallback branch.
pub fn expand_fallback_key(name: &str) -> String {
    format!("{name}_id")
}

/// Whether the `issue_attachments` special case fires (`base.py`): it is in
/// the declared fields or in the expand selection.
pub fn needs_attachments(fields: &[&str], expand: &[String]) -> bool {
    fields.contains(&"issue_attachments") || expand.iter().any(|name| name == "issue_attachments")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::HashSet;

    fn fixture_view_issue_row() -> ViewIssueListRow<'static> {
        // Values mirror FX-SER.json `view_issue_list_serializer.output_example`.
        ViewIssueListRow {
            id: "11111111-1111-1111-1111-111111111111",
            name: "Login fails on SSO",
            state_id: Some("22222222-2222-2222-2222-222222222222"),
            sort_order: 65535.0,
            completed_at: None,
            estimate_point: None,
            priority: "high",
            start_date: None,
            target_date: None,
            sequence_id: 42,
            project_id: Some("33333333-3333-3333-3333-333333333333"),
            parent_id: None,
            cycle_id: None,
            sub_issues_count: 0,
            created_at: "2026-09-01T10:00:00Z",
            updated_at: "2026-09-02T10:00:00Z",
            created_by: Some("44444444-4444-4444-4444-444444444444"),
            updated_by: Some("44444444-4444-4444-4444-444444444444"),
            attachment_count: 0,
            link_count: 0,
            is_draft: false,
            archived_at: None,
            state_group: Some("unstarted"),
            assignee_ids: vec![],
            label_ids: vec![],
            module_ids: vec![],
        }
    }

    #[test]
    fn view_issue_list_keys_follow_source_order() {
        // view.py:30-55 order, pinned by VIEW_ISSUE_KEYS in the contract suite.
        assert_eq!(VIEW_ISSUE_LIST_FIELDS.len(), 26);
        assert_eq!(VIEW_ISSUE_LIST_FIELDS[0], "id");
        assert_eq!(VIEW_ISSUE_LIST_FIELDS[22], "state__group");
        assert_eq!(
            &VIEW_ISSUE_LIST_FIELDS[23..],
            &["assignee_ids", "label_ids", "module_ids"]
        );
        let contract: HashSet<&str> = [
            "id",
            "name",
            "state_id",
            "sort_order",
            "completed_at",
            "estimate_point",
            "priority",
            "start_date",
            "target_date",
            "sequence_id",
            "project_id",
            "parent_id",
            "cycle_id",
            "sub_issues_count",
            "created_at",
            "updated_at",
            "created_by",
            "updated_by",
            "attachment_count",
            "link_count",
            "is_draft",
            "archived_at",
            "state__group",
            "assignee_ids",
            "label_ids",
            "module_ids",
        ]
        .into_iter()
        .collect();
        let ours: HashSet<&str> = VIEW_ISSUE_LIST_FIELDS.into_iter().collect();
        assert_eq!(ours, contract);
    }

    #[test]
    fn view_issue_list_wire_matches_fixture_example() {
        let row = fixture_view_issue_row();
        let body = serde_json::to_string(&view_issue_list_to_representation(&row)).unwrap();
        // Key order is the view.py source order; values are the FX-SER.json
        // output_example. Datetimes use the live-DRF form (no zero fraction;
        // the fixture's illustrative `.000000Z` never appears on the wire).
        assert_eq!(
            body,
            r#"{"id":"11111111-1111-1111-1111-111111111111","name":"Login fails on SSO","state_id":"22222222-2222-2222-2222-222222222222","sort_order":65535.0,"completed_at":null,"estimate_point":null,"priority":"high","start_date":null,"target_date":null,"sequence_id":42,"project_id":"33333333-3333-3333-3333-333333333333","parent_id":null,"cycle_id":null,"sub_issues_count":0,"created_at":"2026-09-01T10:00:00Z","updated_at":"2026-09-02T10:00:00Z","created_by":"44444444-4444-4444-4444-444444444444","updated_by":"44444444-4444-4444-4444-444444444444","attachment_count":0,"link_count":0,"is_draft":false,"archived_at":null,"state__group":"unstarted","assignee_ids":[],"label_ids":[],"module_ids":[]}"#
        );
    }

    #[test]
    fn view_issue_list_state_guard_and_arrays() {
        // `instance.state.group if instance.state else None`: a null state
        // renders a null group, and the *_id columns render through.
        let mut row = fixture_view_issue_row();
        row.state_id = None;
        row.state_group = None;
        row.estimate_point = Some("7e8b0b1e-allow-null-fk-renders-string");
        row.assignee_ids = vec!["44444444-4444-4444-4444-444444444444"];
        let body = serde_json::to_string(&view_issue_list_to_representation(&row)).unwrap();
        assert!(body.contains(r#""state_id":null,"sort_order""#));
        assert!(body.contains(r#""state__group":null"#));
        assert!(body.contains(r#""assignee_ids":["44444444-4444-4444-4444-444444444444"]"#));
    }

    #[test]
    fn issue_view_keys_follow_live_drf_order() {
        // Probed order (DRF 3.18.1): id, [is_favorite], created_at … owned_by.
        assert_eq!(ISSUE_VIEW_FIELDS.len(), 21);
        assert_eq!(
            &ISSUE_VIEW_FIELDS[..5],
            &["id", "created_at", "updated_at", "deleted_at", "name"]
        );
        assert_eq!(
            &ISSUE_VIEW_FIELDS[16..],
            &[
                "created_by",
                "updated_by",
                "workspace",
                "project",
                "owned_by"
            ]
        );
        // The contract suite's GLOBAL_VIEW_KEYS is exactly this set.
        let contract: HashSet<&str> = [
            "access",
            "archived_at",
            "created_at",
            "created_by",
            "deleted_at",
            "description",
            "display_filters",
            "display_properties",
            "filters",
            "id",
            "is_locked",
            "logo_props",
            "name",
            "owned_by",
            "project",
            "query",
            "rich_filters",
            "sort_order",
            "updated_at",
            "updated_by",
            "workspace",
        ]
        .into_iter()
        .collect();
        let ours: HashSet<&str> = ISSUE_VIEW_FIELDS.into_iter().collect();
        assert_eq!(ours, contract);
        assert_eq!(IS_FAVORITE_FIELD, "is_favorite");
        assert_eq!(IS_FAVORITE_INDEX, 1);
    }

    fn issue_view_probe_row() -> IssueViewRow<'static> {
        // Values mirror the live-DRF probe (leaked so the row can borrow).
        let query: &'static Value = Box::leak(Box::new(json!({"a": 1})));
        let filters: &'static Value = Box::leak(Box::new(json!({"priority": ["high"]})));
        let display_filters: &'static Value = Box::leak(Box::new(json!({"layout": "list"})));
        let display_properties: &'static Value = Box::leak(Box::new(json!({"key": true})));
        let rich: &'static Value = Box::leak(Box::new(json!({})));
        let logo: &'static Value = Box::leak(Box::new(json!({})));
        IssueViewRow {
            id: "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
            is_favorite: None,
            created_at: None,
            updated_at: None,
            deleted_at: None,
            name: "My board",
            description: "",
            query,
            filters,
            display_filters,
            display_properties,
            rich_filters: rich,
            access: 1,
            sort_order: 65535.0,
            logo_props: logo,
            is_locked: false,
            archived_at: None,
            created_by: None,
            updated_by: None,
            workspace: None,
            project: None,
            owned_by: Some("44444444-4444-4444-4444-444444444444"),
        }
    }

    #[test]
    fn issue_view_wire_matches_live_drf_byte_for_byte() {
        // The expected string is the live-DRF probe output (ReturnDict run
        // through DRF's JSONEncoder): is_favorite absent without the
        // annotation, null FKs as null, float kept as 65535.0.
        let row = issue_view_probe_row();
        let body = serde_json::to_string(&issue_view_to_representation(&row)).unwrap();
        assert_eq!(
            body,
            r#"{"id":"aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa","created_at":null,"updated_at":null,"deleted_at":null,"name":"My board","description":"","query":{"a":1},"filters":{"priority":["high"]},"display_filters":{"layout":"list"},"display_properties":{"key":true},"rich_filters":{},"access":1,"sort_order":65535.0,"logo_props":{},"is_locked":false,"archived_at":null,"created_by":null,"updated_by":null,"workspace":null,"project":null,"owned_by":"44444444-4444-4444-4444-444444444444"}"#
        );
    }

    #[test]
    fn issue_view_favorite_present_at_index_one_when_annotated() {
        let mut row = issue_view_probe_row();
        row.is_favorite = Some(true);
        let body = serde_json::to_string(&issue_view_to_representation(&row)).unwrap();
        assert!(body.starts_with(
            r#"{"id":"aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa","is_favorite":true,"created_at""#
        ));
        let id_at = body.find("\"id\"").unwrap();
        let fav_at = body.find("\"is_favorite\"").unwrap();
        let created_at = body.find("\"created_at\"").unwrap();
        assert!(id_at < fav_at && fav_at < created_at);
    }

    #[test]
    fn issue_view_datetimes_pass_through_verbatim() {
        // Rendering owns to the DB edge; both live-DRF forms (bare Z and
        // microsecond fraction) cross unchanged.
        let mut row = issue_view_probe_row();
        row.created_at = Some("2026-09-01T10:00:00Z");
        row.updated_at = Some("2026-09-02T10:00:00.123456Z");
        row.archived_at = Some("2026-09-03T10:00:00.123456789Z");
        let body = serde_json::to_string(&issue_view_to_representation(&row)).unwrap();
        assert!(body.contains(r#""created_at":"2026-09-01T10:00:00Z""#));
        assert!(body.contains(r#""updated_at":"2026-09-02T10:00:00.123456Z""#));
        assert!(body.contains(r#""archived_at":"2026-09-03T10:00:00.123456789Z""#));
    }

    #[test]
    fn effective_selection_discards_fields_bug_b2() {
        // `fields = self.expand`: whatever `fields` carries is ignored.
        assert_eq!(
            effective_selection(Some(vec!["id".to_owned(), "name".to_owned()]), None),
            None
        );
        assert_eq!(
            effective_selection(
                Some(vec!["id".to_owned(), "name".to_owned()]),
                Some(vec!["owned_by".to_owned()])
            ),
            Some(vec!["owned_by".to_owned()])
        );
        assert_eq!(effective_selection(None, None), None);
    }

    #[test]
    fn create_query_takes_post_path_only_when_filters_nonempty() {
        let filters = json!({"priority": ["high"]});
        let out = resolve_create_query(Some(&filters), |value| {
            assert_eq!(value, &filters);
            json!({"post": true})
        });
        assert_eq!(out, json!({"post": true}));
        // Empty object, empty string, null and missing all fall to {} — and
        // the POST computation is never invoked on those paths.
        for probe in [json!({}), json!(""), json!(0), json!(false), json!(null)] {
            let out = resolve_create_query(Some(&probe), |_| panic!("must not compute"));
            assert_eq!(out, json!({}));
        }
        let out = resolve_create_query(None, |_| panic!("must not compute"));
        assert_eq!(out, json!({}));
    }

    #[test]
    fn update_query_is_always_the_patch_mapping_bug_b3() {
        // The PATCH line overwrites the POST line unconditionally — even for
        // empty or missing filters the PATCH computation runs.
        let filters = json!({"priority": ["high"]});
        let out = resolve_update_query(Some(&filters), |value| {
            assert_eq!(value, &filters);
            json!({"patch": true})
        });
        assert_eq!(out, json!({"patch": true}));
        let out = resolve_update_query(Some(&json!({})), |value| {
            assert_eq!(value, &json!({}));
            json!({"patch-empty": true})
        });
        assert_eq!(out, json!({"patch-empty": true}));
        // Missing `filters` reproduces `validated_data.get("filters", {})`: the
        // PATCH computation receives `{}`, not null.
        let out = resolve_update_query(None, |value| {
            assert_eq!(value, &json!({}));
            json!({"patch-missing": true})
        });
        assert_eq!(out, json!({"patch-missing": true}));
        // An explicitly present null is forwarded as-is (Python hands `None`
        // to `issue_filters`, whose `key in query_params` then raises).
        let out = resolve_update_query(Some(&Value::Null), |value| {
            assert_eq!(value, &Value::Null);
            json!({"patch-null": true})
        });
        assert_eq!(out, json!({"patch-null": true}));
    }

    #[test]
    fn entity_map_classifies_all_types() {
        assert_eq!(entity_kind("cycle"), EntityKind::Cycle);
        assert_eq!(entity_kind("module"), EntityKind::Module);
        assert_eq!(entity_kind("view"), EntityKind::View);
        assert_eq!(entity_kind("page"), EntityKind::Page);
        assert_eq!(entity_kind("project"), EntityKind::Project);
        // (Issue, None), (None, None), and unknown misses all render null.
        assert_eq!(entity_kind("issue"), EntityKind::AlwaysNone);
        assert_eq!(entity_kind("folder"), EntityKind::AlwaysNone);
        assert_eq!(entity_kind("nope"), EntityKind::AlwaysNone);
    }

    #[test]
    fn entity_data_none_without_fetch_or_row() {
        // AlwaysNone kinds never touch the database, even when a value exists.
        assert_eq!(
            resolve_entity_data(EntityKind::AlwaysNone, Some(json!({"id": "x"}))),
            None
        );
        // DoesNotExist on a resolvable kind renders None.
        assert_eq!(resolve_entity_data(EntityKind::View, None), None);
        let nest = json!({"id": "a", "name": "My board"});
        assert_eq!(
            resolve_entity_data(EntityKind::View, Some(nest.clone())),
            Some(nest)
        );
    }

    #[test]
    fn view_favorite_wire_matches_meta_order() {
        assert_eq!(
            VIEW_FAVORITE_FIELDS,
            ["id", "name", "logo_props", "project_id"]
        );
        let logo: &'static Value = Box::leak(Box::new(json!({})));
        let row = ViewFavoriteRow {
            id: "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
            name: "My board",
            logo_props: logo,
            project_id: None,
        };
        let body = serde_json::to_string(&view_favorite_to_representation(&row)).unwrap();
        assert_eq!(
            body,
            r#"{"id":"aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa","name":"My board","logo_props":{},"project_id":null}"#
        );
    }

    #[test]
    fn user_favorite_keys_and_wire() {
        assert_eq!(
            USER_FAVORITE_FIELDS,
            [
                "id",
                "entity_type",
                "entity_identifier",
                "entity_data",
                "name",
                "is_folder",
                "sequence",
                "parent",
                "workspace_id",
                "project_id"
            ]
        );
        // FX-SER.json user_favorite_serializer.output_example: a view
        // favorite whose entity nest is the 4-key ViewFavorite shape.
        let nest: &'static Value = Box::leak(Box::new(json!({
            "id": "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
            "logo_props": {},
            "name": "My board",
            "project_id": null,
        })));
        let row = UserFavoriteRow {
            id: "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb",
            entity_type: "view",
            entity_identifier: Some("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa"),
            entity_data: Some(nest),
            name: None,
            is_folder: false,
            sequence: 65535.0,
            parent: None,
            workspace_id: Some("cccccccc-cccc-cccc-cccc-cccccccccccc"),
            project_id: Some("33333333-3333-3333-3333-333333333333"),
        };
        let body = serde_json::to_string(&user_favorite_to_representation(&row)).unwrap();
        assert_eq!(
            body,
            r#"{"id":"bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb","entity_type":"view","entity_identifier":"aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa","entity_data":{"id":"aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa","logo_props":{},"name":"My board","project_id":null},"name":null,"is_folder":false,"sequence":65535.0,"parent":null,"workspace_id":"cccccccc-cccc-cccc-cccc-cccccccccccc","project_id":"33333333-3333-3333-3333-333333333333"}"#
        );
        // The probed issue-type favorite: entity_data null, key present.
        let probed = UserFavoriteRow {
            id: "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb",
            entity_type: "issue",
            entity_identifier: Some("427538ba-299c-411f-b959-d284dcd4cf33"),
            entity_data: None,
            name: None,
            is_folder: false,
            sequence: 65535.0,
            parent: None,
            workspace_id: None,
            project_id: None,
        };
        let body = serde_json::to_string(&user_favorite_to_representation(&probed)).unwrap();
        assert!(body.contains(r#""entity_data":null"#));
        assert!(body.contains(r#""parent":null"#));
    }

    #[test]
    fn expand_outcome_table_matches_probed_behavior() {
        // Unknown names are ignored entirely (no key added).
        assert_eq!(
            expand_outcome("bogus", &ISSUE_VIEW_FIELDS),
            ExpandOutcome::Ignored
        );
        // Serializer fields in the expansion map nest.
        assert_eq!(
            expand_outcome("owned_by", &ISSUE_VIEW_FIELDS),
            ExpandOutcome::Nested
        );
        assert_eq!(
            expand_outcome("workspace", &ISSUE_VIEW_FIELDS),
            ExpandOutcome::Nested
        );
        // Serializer fields outside the map fall back to <name>_id —
        // probed: expand=name overwrites the name with None.
        assert_eq!(
            expand_outcome("name", &ISSUE_VIEW_FIELDS),
            ExpandOutcome::Fallback
        );
        assert_eq!(expand_fallback_key("name"), "name_id");
        // many=True membership follows the base.py lists.
        assert!(EXPANSION_MANY_FIELDS.contains(&"assignees"));
        assert!(!EXPANSION_MANY_FIELDS.contains(&"owned_by"));
    }

    #[test]
    fn attachments_case_needs_fields_or_expand() {
        assert!(needs_attachments(&["issue_attachments"], &[]));
        assert!(needs_attachments(
            &ISSUE_VIEW_FIELDS,
            &["issue_attachments".to_owned()]
        ));
        assert!(!needs_attachments(&ISSUE_VIEW_FIELDS, &[]));
    }
}
