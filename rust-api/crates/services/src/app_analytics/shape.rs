#![forbid(unsafe_code)]

//! D-35 analytic / exporter / importer + porter issue serializers.
//!
//! Port of these four units (all over `BaseSerializer`,
//! `app/serializers/base.py:8-9`):
//!
//! * `app/serializers/analytic.py:10-31` (`AnalyticViewSerializer`,
//!   FX-A-SER-01)
//! * `app/serializers/exporter.py:11-30` (`ExporterHistorySerializer`,
//!   FX-A-SER-02)
//! * `app/serializers/importer.py:13-20` (`ImporterSerializer`,
//!   FX-A-SER-03)
//! * `utils/porters/serializers/issue.py:12-146`
//!   (`IssueExportSerializer` + every `get_*`, FX-A-SER-04)
//!
//! Wire rules (same conventions as the D-29 port in
//! `app_views_search/serializers.rs`):
//!
//! * Key order is DRF order. For `fields = "__all__"` the installed
//!   DRF 3.18.1 computes
//!   `get_default_field_names = [pk] + declared + concrete + forward
//!   relations`, so declared extras lead, then concrete columns in Django
//!   `_meta` order (abstract parents first), then FKs in `_meta` order.
//!   For explicit `Meta.fields` lists the list order is the wire order.
//! * UUID and FK primary keys render as strings (`PrimaryKeyRelatedField`,
//!   read-only); a null FK renders `null`.
//! * Datetimes cross this boundary already rendered as DRF `iso-8601`
//!   strings — formatting owns to the DB edge, so rendering here is a
//!   byte-exact passthrough. JSON columns (`query`, `query_dict`,
//!   `metadata`, `config`, `data`) pass through verbatim. The one
//!   exception is the porter comment timestamp, which Python formats with
//!   `strftime("%Y-%m-%d %H:%M:%S")`, not DRF iso-8601; that kernel takes
//!   a `chrono` instant and formats it (Django stores UTC, and `strftime`
//!   on an aware datetime uses its own tzinfo).
//! * The `issue_filters` computation itself belongs to the queries layer
//!   (F-07); the write kernels below port only the branch decisions and
//!   take the mapping as a callback, exactly like D-29's
//!   `resolve_create_query` / `resolve_update_query`.
//!
//! Ported bugs (translate, don't redesign):
//!
//! * BUG-1 (`analytic.py:17` vs `:25`): `create` reads
//!   `validated_data["query_dict"]` but `update` reads
//!   `validated_data["query_data"]`; a PATCH body carrying `query_dict`
//!   is ignored by `update`. The two kernels take differently named
//!   inputs on purpose.
//! * BUG-2 (`analytic.py:30`): `update` assigns
//!   `validated_data["query"]` a third time unconditionally, so the
//!   `if/else` on lines 26-29 is dead — the effective query is always
//!   `issue_filters(query_params, "PATCH")`. [`resolve_analytic_update_query`]
//!   takes only the PATCH computation; the POST line is not represented.
//!
//! Shape-only no-ops preserved as documentation, not code:
//! `read_only_fields` constrain writes, of which the shape ports have none
//! (the constants are kept so handlers can enforce them).

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;

// ---------------------------------------------------------------------------
// FX-A-SER-01: AnalyticViewSerializer (analytic.py:10-31)
// ---------------------------------------------------------------------------

/// `AnalyticViewSerializer` wire keys (`Meta.fields = "__all__"`), in DRF
/// order: `[pk] + declared ([id]) + concrete + forward relations`.
/// Concrete `_meta` order is `created_at`, `updated_at` (`TimeAuditModel`),
/// `deleted_at` (`SoftDeleteModel`; the `UserAuditModel` FKs sort into
/// forward relations), then the model's own `name`, `description`,
/// `query`, `query_dict`; forward relations in `_meta` order are
/// `created_by`, `updated_by`, `workspace`.
pub const ANALYTIC_VIEW_FIELDS: [&str; 11] = [
    "id",
    "created_at",
    "updated_at",
    "deleted_at",
    "name",
    "description",
    "query",
    "query_dict",
    "created_by",
    "updated_by",
    "workspace",
];

/// `read_only_fields` (`analytic.py:13`).
pub const ANALYTIC_VIEW_READ_ONLY_FIELDS: [&str; 2] = ["workspace", "query"];

/// The `validated_data` key `create` reads (`analytic.py:17`).
pub const ANALYTIC_CREATE_QUERY_KEY: &str = "query_dict";

/// The `validated_data` key `update` reads (`analytic.py:25`, BUG-1: it is
/// `query_data`, not `query_dict`).
pub const ANALYTIC_UPDATE_QUERY_KEY: &str = "query_data";

/// `AnalyticViewSerializer.create` query kernel (`analytic.py:17-21`): a
/// truthy `query_dict` maps through `issue_filters(query_params, "POST")`,
/// otherwise the query is `{}`. `compute_post` is only invoked on the
/// non-empty path.
pub fn resolve_analytic_create_query(
    query_dict: Option<&Value>,
    compute_post: impl FnOnce(&Value) -> Value,
) -> Value {
    match query_dict {
        Some(value) if is_truthy_json(value) => compute_post(value),
        _ => empty_query(),
    }
}

/// `AnalyticViewSerializer.update` query kernel (`analytic.py:25-30`,
/// BUG-2): the `issue_filters(query_params, "PATCH")` assignment on line 30
/// runs unconditionally and overwrites the POST line, so the effective query
/// is always the PATCH mapping — even when `query_data` is empty or missing.
/// The POST computation never survives; it is not invoked here at all.
///
/// `query_data` reproduces `validated_data.get("query_data", {})`: a missing
/// key arrives as `None` and maps to the empty query input (`{}`), exactly
/// what Python hands `issue_filters`.
pub fn resolve_analytic_update_query(
    query_data: Option<&Value>,
    compute_patch: impl FnOnce(&Value) -> Value,
) -> Value {
    match query_data {
        Some(params) => compute_patch(params),
        None => compute_patch(&empty_query()),
    }
}

/// The empty-query literal the write kernels fall back to (`{}`).
pub fn empty_query() -> Value {
    Value::Object(serde_json::Map::new())
}

/// Python `bool()` over JSON values, for the `if bool(...)` branches:
/// only a non-empty container / nonzero number / non-empty string / `true`
/// takes the mapping path.
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

/// A database row for the analytic-view shape. `query` / `query_dict` borrow
/// the caller's parsed JSON and pass through verbatim; datetimes borrow
/// pre-rendered DRF strings; FKs borrow UUID strings.
#[derive(Debug, Clone, PartialEq)]
pub struct AnalyticViewRow<'a> {
    pub id: &'a str,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub deleted_at: Option<&'a str>,
    pub name: &'a str,
    pub description: &'a str,
    pub query: &'a Value,
    pub query_dict: &'a Value,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub workspace: Option<&'a str>,
}

/// `AnalyticViewSerializer.to_representation` output, in DRF wire order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AnalyticViewView<'a> {
    pub id: &'a str,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub deleted_at: Option<&'a str>,
    pub name: &'a str,
    pub description: &'a str,
    pub query: &'a Value,
    pub query_dict: &'a Value,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub workspace: Option<&'a str>,
}

/// Build the analytic-view representation from a row.
pub fn analytic_view_to_representation<'a>(row: &'a AnalyticViewRow<'a>) -> AnalyticViewView<'a> {
    AnalyticViewView {
        id: row.id,
        created_at: row.created_at,
        updated_at: row.updated_at,
        deleted_at: row.deleted_at,
        name: row.name,
        description: row.description,
        query: row.query,
        query_dict: row.query_dict,
        created_by: row.created_by,
        updated_by: row.updated_by,
        workspace: row.workspace,
    }
}

// ---------------------------------------------------------------------------
// FX-A-SER-02: ExporterHistorySerializer (exporter.py:11-30)
// ---------------------------------------------------------------------------

/// `ExporterHistorySerializer` wire keys (`Meta.fields` order,
/// `exporter.py:15-28`). The whole serializer is read-only
/// (`read_only_fields = fields`, `:30`).
pub const EXPORTER_HISTORY_FIELDS: [&str; 12] = [
    "id",
    "created_at",
    "updated_at",
    "project",
    "provider",
    "status",
    "url",
    "initiated_by",
    "initiated_by_detail",
    "token",
    "created_by",
    "updated_by",
];

/// The nested `initiated_by_detail` mirrors `UserLiteSerializer`
/// (`app/serializers/user.py`), whose 7-key wire order is pinned by the
/// space port (`space/serializers/lite.rs`).
#[derive(Debug, Clone, PartialEq)]
pub struct InitiatedByDetailRow<'a> {
    pub id: &'a str,
    pub first_name: &'a str,
    pub last_name: &'a str,
    pub avatar: &'a str,
    pub avatar_url: Option<&'a str>,
    pub is_bot: bool,
    pub display_name: &'a str,
}

/// `initiated_by_detail` representation, in `UserLiteSerializer` order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct InitiatedByDetailView<'a> {
    pub id: &'a str,
    pub first_name: &'a str,
    pub last_name: &'a str,
    pub avatar: &'a str,
    pub avatar_url: Option<&'a str>,
    pub is_bot: bool,
    pub display_name: &'a str,
}

/// Build the `initiated_by_detail` representation from a row.
pub fn initiated_by_detail_to_representation<'a>(
    row: &'a InitiatedByDetailRow<'a>,
) -> InitiatedByDetailView<'a> {
    InitiatedByDetailView {
        id: row.id,
        first_name: row.first_name,
        last_name: row.last_name,
        avatar: row.avatar,
        avatar_url: row.avatar_url,
        is_bot: row.is_bot,
        display_name: row.display_name,
    }
}

/// A database row for the exporter-history shape. `project` is the
/// `ArrayField` of project UUIDs (`db/models/exporter.py:24`); `url`
/// renders `null` until `upload_to_s3` sets the presigned URL;
/// `created_by` / `updated_by` render `null` on task-created rows.
#[derive(Debug, Clone, PartialEq)]
pub struct ExporterHistoryRow<'a> {
    pub id: &'a str,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub project: Vec<&'a str>,
    pub provider: &'a str,
    pub status: &'a str,
    pub url: Option<&'a str>,
    pub initiated_by: &'a str,
    pub initiated_by_detail: InitiatedByDetailRow<'a>,
    pub token: &'a str,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
}

/// `ExporterHistorySerializer.to_representation` output, in `Meta.fields`
/// order. `name` / `type` / `reason` / `key` / `filters` / `rich_filters`
/// are model columns but NOT serializer fields — never rendered.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ExporterHistoryView<'a> {
    pub id: &'a str,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub project: Vec<&'a str>,
    pub provider: &'a str,
    pub status: &'a str,
    pub url: Option<&'a str>,
    pub initiated_by: &'a str,
    pub initiated_by_detail: InitiatedByDetailView<'a>,
    pub token: &'a str,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
}

/// Build the exporter-history representation from a row.
pub fn exporter_history_to_representation<'a>(
    row: &'a ExporterHistoryRow<'a>,
) -> ExporterHistoryView<'a> {
    ExporterHistoryView {
        id: row.id,
        created_at: row.created_at,
        updated_at: row.updated_at,
        project: row.project.clone(),
        provider: row.provider,
        status: row.status,
        url: row.url,
        initiated_by: row.initiated_by,
        initiated_by_detail: initiated_by_detail_to_representation(&row.initiated_by_detail),
        token: row.token,
        created_by: row.created_by,
        updated_by: row.updated_by,
    }
}

// ---------------------------------------------------------------------------
// FX-A-SER-03: ImporterSerializer (importer.py:13-20)
// ---------------------------------------------------------------------------

/// `ImporterSerializer` wire keys (`Meta.fields = "__all__"`), in DRF
/// order: `[pk] + declared ([initiated_by_detail, project_detail,
/// workspace_detail]) + concrete + forward relations`. Concrete `_meta`
/// order is `created_at`, `updated_at`, `deleted_at`, then the model's own
/// `service`, `status`, `metadata`, `config`, `data`, `imported_data`;
/// forward relations in `_meta` order are `created_by`, `updated_by`,
/// `project`, `workspace` (`ProjectBaseModel`), `initiated_by`, `token`.
/// No handler serves this serializer (model only) — no route renders it.
pub const IMPORTER_FIELDS: [&str; 19] = [
    "id",
    "initiated_by_detail",
    "project_detail",
    "workspace_detail",
    "created_at",
    "updated_at",
    "deleted_at",
    "service",
    "status",
    "metadata",
    "config",
    "data",
    "imported_data",
    "created_by",
    "updated_by",
    "project",
    "workspace",
    "initiated_by",
    "token",
];

/// `project_detail` mirrors `ProjectLiteSerializer` (7 keys, same order as
/// the space port).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ImporterProjectDetailView<'a> {
    pub id: &'a str,
    pub identifier: &'a str,
    pub name: &'a str,
    pub cover_image: Option<&'a str>,
    pub icon_prop: &'a Value,
    pub emoji: Option<&'a str>,
    pub description: &'a str,
}

/// `workspace_detail` mirrors `WorkspaceLiteSerializer` (3 keys, same order
/// as the space port).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ImporterWorkspaceDetailView<'a> {
    pub name: &'a str,
    pub slug: &'a str,
    pub id: &'a str,
}

/// A database row for the importer shape. Defaults: `status = "queued"`,
/// `metadata` / `config` / `data` = `{}`, `imported_data = null`.
/// `service` is `github | jira`; `status` is
/// `queued | processing | completed | failed`.
#[derive(Debug, Clone, PartialEq)]
pub struct ImporterRow<'a> {
    pub id: &'a str,
    pub initiated_by_detail: InitiatedByDetailRow<'a>,
    pub project_detail: ImporterProjectDetailView<'a>,
    pub workspace_detail: ImporterWorkspaceDetailView<'a>,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub deleted_at: Option<&'a str>,
    pub service: &'a str,
    pub status: &'a str,
    pub metadata: &'a Value,
    pub config: &'a Value,
    pub data: &'a Value,
    pub imported_data: Option<&'a Value>,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub project: Option<&'a str>,
    pub workspace: Option<&'a str>,
    pub initiated_by: &'a str,
    pub token: &'a str,
}

/// `ImporterSerializer.to_representation` output, in DRF wire order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ImporterView<'a> {
    pub id: &'a str,
    pub initiated_by_detail: InitiatedByDetailView<'a>,
    pub project_detail: &'a ImporterProjectDetailView<'a>,
    pub workspace_detail: &'a ImporterWorkspaceDetailView<'a>,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub deleted_at: Option<&'a str>,
    pub service: &'a str,
    pub status: &'a str,
    pub metadata: &'a Value,
    pub config: &'a Value,
    pub data: &'a Value,
    pub imported_data: Option<&'a Value>,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub project: Option<&'a str>,
    pub workspace: Option<&'a str>,
    pub initiated_by: &'a str,
    pub token: &'a str,
}

/// Build the importer representation from a row.
pub fn importer_to_representation<'a>(row: &'a ImporterRow<'a>) -> ImporterView<'a> {
    ImporterView {
        id: row.id,
        initiated_by_detail: initiated_by_detail_to_representation(&row.initiated_by_detail),
        project_detail: &row.project_detail,
        workspace_detail: &row.workspace_detail,
        created_at: row.created_at,
        updated_at: row.updated_at,
        deleted_at: row.deleted_at,
        service: row.service,
        status: row.status,
        metadata: row.metadata,
        config: row.config,
        data: row.data,
        imported_data: row.imported_data,
        created_by: row.created_by,
        updated_by: row.updated_by,
        project: row.project,
        workspace: row.workspace,
        initiated_by: row.initiated_by,
        token: row.token,
    }
}

// ---------------------------------------------------------------------------
// FX-A-SER-04: IssueExportSerializer (porters issue.py:12-146)
// ---------------------------------------------------------------------------

/// `IssueExportSerializer` wire keys (`Meta.fields` order, `issue.py:37-66`).
pub const ISSUE_EXPORT_FIELDS: [&str; 29] = [
    "project_name",
    "project_identifier",
    "parent",
    "identifier",
    "sequence_id",
    "name",
    "state_name",
    "priority",
    "complexity_score",
    "assignees",
    "subscribers",
    "created_by_name",
    "start_date",
    "target_date",
    "completed_at",
    "created_at",
    "updated_at",
    "archived_at",
    "estimate",
    "labels",
    "cycles",
    "modules",
    "links",
    "relations",
    "comments",
    "sub_issues_count",
    "link_count",
    "attachment_count",
    "is_draft",
];

/// `get_identifier` (`issue.py:69-70`): `{project.identifier}-{sequence_id}`.
pub fn export_identifier(project_identifier: &str, sequence_id: i64) -> String {
    format!("{project_identifier}-{sequence_id}")
}

/// `get_assignees` (`issue.py:72-73`): full names of active assignees only;
/// inactive users are dropped.
pub fn active_assignee_names<'a>(assignees: &[(&'a str, bool)]) -> Vec<&'a str> {
    assignees
        .iter()
        .filter(|(_, is_active)| *is_active)
        .map(|(full_name, _)| *full_name)
        .collect()
}

/// `get_subscribers` (`issue.py:75-77`): subscriber full names; rows whose
/// subscriber is null are dropped.
pub fn subscriber_names<'a>(subscribers: &[Option<&'a str>]) -> Vec<&'a str> {
    subscribers.iter().flatten().copied().collect()
}

/// `get_parent` (`issue.py:79-82`): `""` when there is no parent, else the
/// parent's `{identifier}-{sequence_id}`.
pub fn parent_identifier(parent: Option<(&str, i64)>) -> String {
    match parent {
        Some((identifier, sequence_id)) => export_identifier(identifier, sequence_id),
        None => String::new(),
    }
}

/// `get_labels` (`issue.py:84-89`): label names through the `label_issue`
/// through rows, skipping soft-deleted links (`deleted_at is None`).
/// NOTE: this reads the through table, not the `labels` m2m — the schema
/// exporter variant (`utils/exporters/schemas/issue.py:prepare_labels`)
/// reads `labels.all()` unguarded instead; each is ported as-is by its own
/// layer, and the two exporters differ on soft-deleted labels.
pub fn visible_label_names<'a>(links: &[(&'a str, Option<&'a str>)]) -> Vec<&'a str> {
    links
        .iter()
        .filter(|(_, deleted_at)| deleted_at.is_none())
        .map(|(name, _)| *name)
        .collect()
}

/// `get_cycles` (`issue.py:91-92`): cycle names, no deleted guard.
pub fn cycle_names<'a>(cycles: &[&'a str]) -> Vec<&'a str> {
    cycles.to_vec()
}

/// `get_modules` (`issue.py:94-95`): module names, no deleted guard.
pub fn module_names<'a>(modules: &[&'a str]) -> Vec<&'a str> {
    modules.to_vec()
}

/// Input for `get_estimate` (`issue.py:97-101`): `None` when
/// `obj.estimate_point` is null; otherwise the point, which renders its
/// `.value` when the attribute exists and `str(point)` when it does not.
#[derive(Debug, Clone, PartialEq)]
pub enum EstimatePoint<'a> {
    /// `estimate_point.value` — rendered verbatim (usually a number).
    WithValue(&'a Value),
    /// No `.value` attribute — rendered as its string form.
    WithoutValue(&'a str),
}

/// `get_estimate`: `""` when null, else the value or its string form.
pub fn estimate_output(estimate: Option<EstimatePoint<'_>>) -> Value {
    match estimate {
        None => Value::String(String::new()),
        Some(EstimatePoint::WithValue(value)) => value.clone(),
        Some(EstimatePoint::WithoutValue(text)) => Value::String(text.to_owned()),
    }
}

/// One `issue_link` row for `get_links` (`issue.py:103-111`): empty titles
/// fall back to the URL.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ExportLinkView<'a> {
    pub url: &'a str,
    pub title: &'a str,
}

/// `get_links`: `{url, title}` pairs with the empty-title-to-URL fallback.
pub fn export_links<'a>(links: &[(&'a str, Option<&'a str>)]) -> Vec<ExportLinkView<'a>> {
    links
        .iter()
        .map(|(url, title)| ExportLinkView {
            url,
            title: title.filter(|text| !text.is_empty()).unwrap_or(url),
        })
        .collect()
}

/// One side of `get_relations` (`issue.py:113-135`).
#[derive(Debug, Clone, PartialEq)]
pub struct RelationInput<'a> {
    /// `rel.relation_type`; `None` reproduces the `getattr` fallback
    /// `"related"` (dead in practice — the model column always exists —
    /// but ported as-is).
    pub relation_type: Option<&'a str>,
    /// The related issue's `(project identifier, sequence_id)`; `None`
    /// rows are skipped (`if rel.related_issue` / `if rel.issue`).
    pub issue: Option<(&'a str, i64)>,
}

/// One `get_relations` entry.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ExportRelationView {
    #[serde(rename = "type")]
    pub relation_type: String,
    pub issue: String,
    pub direction: &'static str,
}

/// `get_relations`: outgoing from `issue_relation` then incoming from
/// `issue_related`, each `{type, issue, direction}`; null ends skipped.
pub fn export_relations(
    outgoing: &[RelationInput<'_>],
    incoming: &[RelationInput<'_>],
) -> Vec<ExportRelationView> {
    let mut relations = Vec::with_capacity(outgoing.len() + incoming.len());
    for (inputs, direction) in [(outgoing, "outgoing"), (incoming, "incoming")] {
        for input in inputs {
            if let Some((identifier, sequence_id)) = input.issue {
                relations.push(ExportRelationView {
                    relation_type: input.relation_type.unwrap_or("related").to_owned(),
                    issue: export_identifier(identifier, sequence_id),
                    direction,
                });
            }
        }
    }
    relations
}

/// One `issue_comments` row for `get_comments` (`issue.py:137-146`).
#[derive(Debug, Clone, PartialEq)]
pub struct CommentInput<'a> {
    /// `comment_stripped` when the attribute exists, else `comment_html`.
    pub stripped: Option<&'a str>,
    pub html: &'a str,
    /// `actor.full_name`, or `""` when there is no actor.
    pub author_full_name: Option<&'a str>,
    /// `created_at`, or `""` when null. Django stores UTC and `strftime`
    /// on an aware datetime uses its own tzinfo, so the UTC instant is
    /// what Python formats.
    pub created_at: Option<DateTime<Utc>>,
}

/// One `get_comments` entry.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ExportCommentView<'a> {
    pub comment: &'a str,
    pub created_by: &'a str,
    pub created_at: String,
}

/// `get_comments`: `{comment, created_by, created_at}` with the
/// stripped/html fallback, the missing-actor `""`, and the
/// `"%Y-%m-%d %H:%M:%S"` timestamp (or `""` when null).
pub fn export_comments<'a>(comments: &[CommentInput<'a>]) -> Vec<ExportCommentView<'a>> {
    comments
        .iter()
        .map(|comment| ExportCommentView {
            comment: comment.stripped.unwrap_or(comment.html),
            created_by: comment.author_full_name.unwrap_or(""),
            created_at: comment
                .created_at
                .map(|instant| instant.format("%Y-%m-%d %H:%M:%S").to_string())
                .unwrap_or_default(),
        })
        .collect()
}

/// A database row for the issue-export shape. Relation inputs arrive
/// pre-fetched (assignees with `is_active`, subscriber names with nulls,
/// label through rows with `deleted_at`, links, both relation sides,
/// comments ordered by `created_at` via the task prefetch); datetimes
/// borrow pre-rendered DRF strings.
#[derive(Debug, Clone, PartialEq)]
pub struct IssueExportRow<'a> {
    pub project_name: &'a str,
    pub project_identifier: &'a str,
    pub parent: Option<(&'a str, i64)>,
    pub sequence_id: i64,
    pub name: &'a str,
    pub state_name: &'a str,
    pub priority: &'a str,
    pub complexity_score: Option<i64>,
    pub assignees: &'a [(&'a str, bool)],
    pub subscribers: &'a [Option<&'a str>],
    pub created_by_name: &'a str,
    pub start_date: Option<&'a str>,
    pub target_date: Option<&'a str>,
    pub completed_at: Option<&'a str>,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub archived_at: Option<&'a str>,
    pub estimate: Option<EstimatePoint<'a>>,
    pub labels: &'a [(&'a str, Option<&'a str>)],
    pub cycles: &'a [&'a str],
    pub modules: &'a [&'a str],
    pub links: &'a [(&'a str, Option<&'a str>)],
    pub outgoing_relations: &'a [RelationInput<'a>],
    pub incoming_relations: &'a [RelationInput<'a>],
    pub comments: &'a [CommentInput<'a>],
    pub sub_issues_count: i64,
    pub link_count: i64,
    pub attachment_count: i64,
    pub is_draft: bool,
}

/// `IssueExportSerializer.to_representation` output, in `Meta.fields` order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct IssueExportView<'a> {
    pub project_name: &'a str,
    pub project_identifier: &'a str,
    pub parent: String,
    pub identifier: String,
    pub sequence_id: i64,
    pub name: &'a str,
    pub state_name: &'a str,
    pub priority: &'a str,
    pub complexity_score: Option<i64>,
    pub assignees: Vec<&'a str>,
    pub subscribers: Vec<&'a str>,
    pub created_by_name: &'a str,
    pub start_date: Option<&'a str>,
    pub target_date: Option<&'a str>,
    pub completed_at: Option<&'a str>,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub archived_at: Option<&'a str>,
    pub estimate: Value,
    pub labels: Vec<&'a str>,
    pub cycles: Vec<&'a str>,
    pub modules: Vec<&'a str>,
    pub links: Vec<ExportLinkView<'a>>,
    pub relations: Vec<ExportRelationView>,
    pub comments: Vec<ExportCommentView<'a>>,
    pub sub_issues_count: i64,
    pub link_count: i64,
    pub attachment_count: i64,
    pub is_draft: bool,
}

/// Build the issue-export representation from a row.
pub fn issue_export_to_representation<'a>(row: &'a IssueExportRow<'a>) -> IssueExportView<'a> {
    IssueExportView {
        project_name: row.project_name,
        project_identifier: row.project_identifier,
        parent: parent_identifier(row.parent),
        identifier: export_identifier(row.project_identifier, row.sequence_id),
        sequence_id: row.sequence_id,
        name: row.name,
        state_name: row.state_name,
        priority: row.priority,
        complexity_score: row.complexity_score,
        assignees: active_assignee_names(row.assignees),
        subscribers: subscriber_names(row.subscribers),
        created_by_name: row.created_by_name,
        start_date: row.start_date,
        target_date: row.target_date,
        completed_at: row.completed_at,
        created_at: row.created_at,
        updated_at: row.updated_at,
        archived_at: row.archived_at,
        estimate: estimate_output(row.estimate.clone()),
        labels: visible_label_names(row.labels),
        cycles: cycle_names(row.cycles),
        modules: module_names(row.modules),
        links: export_links(row.links),
        relations: export_relations(row.outgoing_relations, row.incoming_relations),
        comments: export_comments(row.comments),
        sub_issues_count: row.sub_issues_count,
        link_count: row.link_count,
        attachment_count: row.attachment_count,
        is_draft: row.is_draft,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use serde_json::json;

    /// Collect the serialized key order of a view.
    fn keys(value: &Value) -> Vec<String> {
        value.as_object().expect("object").keys().cloned().collect()
    }

    // -- FX-A-SER-01 ------------------------------------------------------

    #[test]
    fn analytic_view_fields_are_drf_all_order() {
        assert_eq!(
            ANALYTIC_VIEW_FIELDS,
            [
                "id",
                "created_at",
                "updated_at",
                "deleted_at",
                "name",
                "description",
                "query",
                "query_dict",
                "created_by",
                "updated_by",
                "workspace",
            ]
        );
        assert_eq!(ANALYTIC_VIEW_READ_ONLY_FIELDS, ["workspace", "query"]);
        // BUG-1: the two write paths read different keys.
        assert_eq!(ANALYTIC_CREATE_QUERY_KEY, "query_dict");
        assert_eq!(ANALYTIC_UPDATE_QUERY_KEY, "query_data");
        assert_ne!(ANALYTIC_CREATE_QUERY_KEY, ANALYTIC_UPDATE_QUERY_KEY);
    }

    #[test]
    fn analytic_create_truthy_query_dict_maps_post() {
        // Fixture: {"priority": "high"} -> issue_filters POST mapping.
        let input = json!({"priority": "high"});
        let out = resolve_analytic_create_query(Some(&input), |params| {
            assert_eq!(*params, input);
            json!({"priority__in": "high"})
        });
        assert_eq!(out, json!({"priority__in": "high"}));
    }

    #[test]
    fn analytic_create_empty_query_dict_stores_empty_query() {
        // Fixture: bool({}) is False so the else branch stores {} verbatim.
        let input = json!({});
        let out = resolve_analytic_create_query(Some(&input), |_| panic!("must not map"));
        assert_eq!(out, json!({}));
        let out = resolve_analytic_create_query(None, |_| panic!("must not map"));
        assert_eq!(out, json!({}));
    }

    #[test]
    fn analytic_update_name_only_patch_recomputes_from_empty() {
        // Fixture: name-only patch still recomputes query from empty
        // query_data -> issue_filters({}, "PATCH").
        let empty = empty_query();
        let out = resolve_analytic_update_query(None, |params| {
            assert_eq!(*params, empty);
            json!({"__patch_of": {}})
        });
        assert_eq!(out, json!({"__patch_of": {}}));
    }

    #[test]
    fn analytic_update_always_uses_patch_mapping_bug_2() {
        // BUG-2: the line-30 assignment is unconditional, so even a truthy
        // query_data takes the PATCH mapping; there is no POST computation
        // to consult (the kernel takes only compute_patch).
        let input = json!({"priority": "high"});
        let out = resolve_analytic_update_query(Some(&input), |params| {
            assert_eq!(*params, input);
            json!({"priority__in_patch": "high"})
        });
        assert_eq!(out, json!({"priority__in_patch": "high"}));
    }

    #[test]
    fn analytic_view_passthrough_and_key_order() {
        let query = json!({"priority__in": "high"});
        let query_dict = json!({"priority": "high"});
        let row = AnalyticViewRow {
            id: "av-1",
            created_at: "2026-09-01T10:00:00Z",
            updated_at: "2026-09-01T10:00:00Z",
            deleted_at: None,
            name: "AV-CRUD",
            description: "roundtrip",
            query: &query,
            query_dict: &query_dict,
            created_by: None,
            updated_by: None,
            workspace: Some("ws-1"),
        };
        let produced =
            serde_json::to_value(analytic_view_to_representation(&row)).expect("serializes");
        // query / query_dict render byte-identical to the stored JSON.
        assert_eq!(produced["query"], query);
        assert_eq!(produced["query_dict"], query_dict);
        assert_eq!(produced["workspace"], json!("ws-1"));
        assert_eq!(
            keys(&produced),
            ANALYTIC_VIEW_FIELDS
                .iter()
                .map(|name| (*name).to_owned())
                .collect::<Vec<_>>()
        );
    }

    // -- FX-A-SER-02 ------------------------------------------------------

    #[test]
    fn exporter_history_fields_are_meta_order() {
        assert_eq!(
            EXPORTER_HISTORY_FIELDS,
            [
                "id",
                "created_at",
                "updated_at",
                "project",
                "provider",
                "status",
                "url",
                "initiated_by",
                "initiated_by_detail",
                "token",
                "created_by",
                "updated_by",
            ]
        );
    }

    #[test]
    fn exporter_history_replays_golden_shape() {
        let detail = InitiatedByDetailRow {
            id: "user-1",
            first_name: "an_admin",
            last_name: "User",
            avatar: "",
            avatar_url: None,
            is_bot: false,
            display_name: "an_admin",
        };
        let row = ExporterHistoryRow {
            id: "exp-1",
            created_at: "2026-09-01T10:00:00Z",
            updated_at: "2026-09-01T10:00:00Z",
            project: vec!["proj-1"],
            provider: "csv",
            status: "completed",
            url: None,
            initiated_by: "user-1",
            initiated_by_detail: detail,
            token: "abcdef1234567890abcdef1234567890",
            created_by: None,
            updated_by: None,
        };
        let produced =
            serde_json::to_value(exporter_history_to_representation(&row)).expect("serializes");
        assert_eq!(
            produced,
            json!({
                "id": "exp-1",
                "created_at": "2026-09-01T10:00:00Z",
                "updated_at": "2026-09-01T10:00:00Z",
                "project": ["proj-1"],
                "provider": "csv",
                "status": "completed",
                "url": null,
                "initiated_by": "user-1",
                "initiated_by_detail": {
                    "id": "user-1",
                    "first_name": "an_admin",
                    "last_name": "User",
                    "avatar": "",
                    "avatar_url": null,
                    "is_bot": false,
                    "display_name": "an_admin",
                },
                "token": "abcdef1234567890abcdef1234567890",
                "created_by": null,
                "updated_by": null,
            })
        );
        assert_eq!(
            keys(&produced),
            EXPORTER_HISTORY_FIELDS
                .iter()
                .map(|name| (*name).to_owned())
                .collect::<Vec<_>>()
        );
    }

    // -- FX-A-SER-03 ------------------------------------------------------

    #[test]
    fn importer_fields_are_drf_all_order() {
        assert_eq!(
            IMPORTER_FIELDS,
            [
                "id",
                "initiated_by_detail",
                "project_detail",
                "workspace_detail",
                "created_at",
                "updated_at",
                "deleted_at",
                "service",
                "status",
                "metadata",
                "config",
                "data",
                "imported_data",
                "created_by",
                "updated_by",
                "project",
                "workspace",
                "initiated_by",
                "token",
            ]
        );
    }

    #[test]
    fn importer_replays_golden_defaults() {
        let empty = json!({});
        let icon = json!({});
        let detail = InitiatedByDetailRow {
            id: "user-1",
            first_name: "an_admin",
            last_name: "User",
            avatar: "",
            avatar_url: None,
            is_bot: false,
            display_name: "an_admin",
        };
        let row = ImporterRow {
            id: "imp-1",
            initiated_by_detail: detail,
            project_detail: ImporterProjectDetailView {
                id: "proj-1",
                identifier: "AN",
                name: "Analytics Project",
                cover_image: None,
                icon_prop: &icon,
                emoji: None,
                description: "",
            },
            workspace_detail: ImporterWorkspaceDetailView {
                name: "ws",
                slug: "ws",
                id: "ws-1",
            },
            created_at: "2026-09-01T10:00:00Z",
            updated_at: "2026-09-01T10:00:00Z",
            deleted_at: None,
            service: "github",
            status: "queued",
            metadata: &empty,
            config: &empty,
            data: &empty,
            imported_data: None,
            created_by: None,
            updated_by: None,
            project: Some("proj-1"),
            workspace: Some("ws-1"),
            initiated_by: "user-1",
            token: "tok-1",
        };
        let produced = serde_json::to_value(importer_to_representation(&row)).expect("serializes");
        assert_eq!(produced["service"], json!("github"));
        assert_eq!(produced["status"], json!("queued"));
        assert_eq!(produced["metadata"], json!({}));
        assert_eq!(produced["config"], json!({}));
        assert_eq!(produced["data"], json!({}));
        assert_eq!(produced["imported_data"], Value::Null);
        assert_eq!(produced["token"], json!("tok-1"));
        assert_eq!(
            keys(&produced),
            IMPORTER_FIELDS
                .iter()
                .map(|name| (*name).to_owned())
                .collect::<Vec<_>>()
        );
    }

    // -- FX-A-SER-04 ------------------------------------------------------

    #[test]
    fn issue_export_fields_are_meta_order() {
        assert_eq!(ISSUE_EXPORT_FIELDS.len(), 29);
        assert_eq!(
            &ISSUE_EXPORT_FIELDS[..5],
            [
                "project_name",
                "project_identifier",
                "parent",
                "identifier",
                "sequence_id",
            ]
        );
        assert_eq!(
            &ISSUE_EXPORT_FIELDS[25..],
            [
                "sub_issues_count",
                "link_count",
                "attachment_count",
                "is_draft",
            ]
        );
    }

    #[test]
    fn export_get_kernels_replay_fixture_edges() {
        assert_eq!(export_identifier("AN", 3), "AN-3");
        // Inactive assignees dropped.
        assert_eq!(
            active_assignee_names(&[("Ada", true), ("Ghost", false)]),
            vec!["Ada"]
        );
        // Null subscribers dropped.
        assert_eq!(subscriber_names(&[Some("Bo"), None]), vec!["Bo"]);
        // Parent "" vs IDENT-seq.
        assert_eq!(parent_identifier(None), "");
        assert_eq!(parent_identifier(Some(("AN", 1))), "AN-1");
        // Labels read the through table: soft-deleted links skipped.
        assert_eq!(
            visible_label_names(&[("bug", None), ("old", Some("2026-01-01T00:00:00Z"))]),
            vec!["bug"]
        );
        // Estimate: null -> "", value verbatim, else string form.
        assert_eq!(estimate_output(None), json!(""));
        assert_eq!(
            estimate_output(Some(EstimatePoint::WithValue(&json!(8)))),
            json!(8)
        );
        assert_eq!(
            estimate_output(Some(EstimatePoint::WithoutValue("P3"))),
            json!("P3")
        );
        // Links: empty title falls back to URL.
        let links = export_links(&[
            ("https://x", Some("X")),
            ("https://y", Some("")),
            ("https://z", None),
        ]);
        assert_eq!(links[0].title, "X");
        assert_eq!(links[1].title, "https://y");
        assert_eq!(links[2].title, "https://z");
        // Relations: outgoing then incoming, null ends skipped, getattr
        // fallback "related".
        let relations = export_relations(
            &[
                RelationInput {
                    relation_type: Some("blocks"),
                    issue: Some(("AN", 4)),
                },
                RelationInput {
                    relation_type: Some("blocks"),
                    issue: None,
                },
            ],
            &[RelationInput {
                relation_type: None,
                issue: Some(("AN", 2)),
            }],
        );
        assert_eq!(relations.len(), 2);
        assert_eq!(relations[0].direction, "outgoing");
        assert_eq!(relations[0].relation_type, "blocks");
        assert_eq!(relations[0].issue, "AN-4");
        assert_eq!(relations[1].direction, "incoming");
        assert_eq!(relations[1].relation_type, "related");
        assert_eq!(relations[1].issue, "AN-2");
        // Comments: stripped/html fallback, missing actor "", strftime.
        let instant = Utc.with_ymd_and_hms(2026, 9, 1, 10, 0, 0).unwrap();
        let comments = export_comments(&[
            CommentInput {
                stripped: Some("plain"),
                html: "<p>plain</p>",
                author_full_name: Some("an_admin User"),
                created_at: Some(instant),
            },
            CommentInput {
                stripped: None,
                html: "<p>hi</p>",
                author_full_name: None,
                created_at: None,
            },
        ]);
        assert_eq!(comments[0].comment, "plain");
        assert_eq!(comments[0].created_by, "an_admin User");
        assert_eq!(comments[0].created_at, "2026-09-01 10:00:00");
        assert_eq!(comments[1].comment, "<p>hi</p>");
        assert_eq!(comments[1].created_by, "");
        assert_eq!(comments[1].created_at, "");
    }

    #[test]
    fn issue_export_replays_golden_object() {
        let estimate = json!(8);
        let row = IssueExportRow {
            project_name: "Analytics Project",
            project_identifier: "AN",
            parent: None,
            sequence_id: 3,
            name: "I3",
            state_name: "Done",
            priority: "medium",
            complexity_score: None,
            assignees: &[],
            subscribers: &[],
            created_by_name: "an_admin User",
            start_date: None,
            target_date: None,
            completed_at: Some("2026-09-01T10:00:00Z"),
            created_at: "2026-09-01T10:00:00Z",
            updated_at: "2026-09-01T10:00:00Z",
            archived_at: None,
            estimate: Some(EstimatePoint::WithValue(&estimate)),
            labels: &[],
            cycles: &[],
            modules: &[],
            links: &[],
            outgoing_relations: &[],
            incoming_relations: &[],
            comments: &[],
            sub_issues_count: 0,
            link_count: 0,
            attachment_count: 0,
            is_draft: false,
        };
        let produced =
            serde_json::to_value(issue_export_to_representation(&row)).expect("serializes");
        assert_eq!(
            produced,
            json!({
                "project_name": "Analytics Project",
                "project_identifier": "AN",
                "parent": "",
                "identifier": "AN-3",
                "sequence_id": 3,
                "name": "I3",
                "state_name": "Done",
                "priority": "medium",
                "complexity_score": null,
                "assignees": [],
                "subscribers": [],
                "created_by_name": "an_admin User",
                "start_date": null,
                "target_date": null,
                "completed_at": "2026-09-01T10:00:00Z",
                "created_at": "2026-09-01T10:00:00Z",
                "updated_at": "2026-09-01T10:00:00Z",
                "archived_at": null,
                "estimate": 8,
                "labels": [],
                "cycles": [],
                "modules": [],
                "links": [],
                "relations": [],
                "comments": [],
                "sub_issues_count": 0,
                "link_count": 0,
                "attachment_count": 0,
                "is_draft": false,
            })
        );
        assert_eq!(
            keys(&produced),
            ISSUE_EXPORT_FIELDS
                .iter()
                .map(|name| (*name).to_owned())
                .collect::<Vec<_>>()
        );
    }
}
