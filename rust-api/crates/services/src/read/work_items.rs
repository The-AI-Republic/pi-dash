//! v1 work-item list/detail reads and renders.
//!
//! Verbatim move of the `v1_work_items::handlers_core` read/render closure
//! (OVL-05, PIDASHCONV-1543): row decode, the gated fetchers, the `expand=`
//! leaves, and the list/detail/write renderers. The handlers call these
//! and map [`Error`](super::Error) onto their denial type.

use std::collections::HashMap;

use chrono::{DateTime, NaiveDate, Utc};
use chrono_tz::Tz;
use serde_json::{Map, Value};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use super::{DbError, Error};
use crate::v1_work_items::queries_core as core_queries;
use crate::v1_work_items::shape_issue as shape;
use crate::v1_work_items::{filter_fields, FieldSpec};

/// The `issues.*` columns every read path selects (F18-05 column order).
/// `description_json` / `description_stripped` / `workpad` are unrendered
/// and unread here, so they stay out of the select list.
pub const ISSUE_SELECT_COLS: &str = r#""issues"."id", "issues"."created_at", "issues"."updated_at", "issues"."deleted_at", "issues"."point", "issues"."name", "issues"."description_html", "issues"."description_binary", "issues"."priority", "issues"."complexity_score", "issues"."start_date", "issues"."target_date", "issues"."sequence_id", "issues"."sort_order", "issues"."completed_at", "issues"."archived_at", "issues"."is_draft", "issues"."external_source", "issues"."external_id", "issues"."git_work_branch", "issues"."created_via", "issues"."agent_executor", "issues"."created_by_id", "issues"."updated_by_id", "issues"."project_id", "issues"."workspace_id", "issues"."parent_id", "issues"."state_id", "issues"."estimate_point_id", "issues"."type_id", "issues"."assigned_pod_id""#;

/// The joined extras every read path selects: the URL parts and the state
/// group (the completed-arm check on writes).
pub const ISSUE_EXTRA_COLS: &str = r#""projects"."identifier" AS "project_identifier", "workspaces"."slug" AS "workspace_slug", "states"."group" AS "state_group""#;

/// A decoded `issues` row: every column the renderer or the write paths
/// read, datetimes already rendered as DRF strings in the request timezone
/// (the `app_issues` precedent — rendering here is a byte-exact
/// passthrough into [`shape::IssueRow`]).
#[derive(Debug, Clone)]
pub struct DecodedIssue {
    pub id: String,
    pub created_at: String,
    pub created_at_dt: DateTime<Utc>,
    pub updated_at: String,
    pub deleted_at: Option<String>,
    pub point: Option<i64>,
    pub name: String,
    pub description_html: String,
    pub description_binary: Option<Vec<u8>>,
    pub priority: String,
    pub complexity_score: i64,
    pub start_date: Option<String>,
    pub target_date: Option<String>,
    pub sequence_id: i64,
    pub sort_order: f64,
    pub completed_at: Option<String>,
    pub archived_at: Option<String>,
    pub is_draft: bool,
    pub external_source: Option<String>,
    pub external_id: Option<String>,
    pub git_work_branch: String,
    pub created_via: Option<String>,
    pub agent_executor: Option<String>,
    pub created_by: Option<String>,
    pub updated_by: Option<String>,
    pub project: String,
    pub workspace: String,
    pub parent: Option<String>,
    pub state: Option<String>,
    pub estimate_point: Option<String>,
    pub type_id: Option<String>,
    pub assigned_pod: Option<String>,
    pub project_identifier: String,
    pub workspace_slug: String,
    pub state_group: Option<String>,
}

/// Owned `relations_summary` rows: `(identifier, state, state_group)`
/// per direction plus the uncapped open flag. The shape borrows these;
/// the caller holds them across the render.
pub struct BlockerRows {
    pub blocked_by: Vec<(String, Option<String>, Option<String>)>,
    pub blocking: Vec<(String, Option<String>, Option<String>)>,
    pub has_open_blockers: bool,
}

/// What one `render_decoded` call renders: the field selection plus the
/// payload kind. `viewer` is the relations viewer (`Some` on the three
/// single-item reads that pass `RELATIONS_VIEWER_CONTEXT`, `None` on
/// lists and the external-id branch).
pub struct RenderRequest<'a> {
    pub field_specs: Option<&'a [FieldSpec]>,
    pub kept: &'a [String],
    pub expand: &'a [&'a str],
    pub is_list: bool,
    pub viewer: Option<Uuid>,
    pub web_base: Option<&'a str>,
}

/// One owned grouped-relation target: the `_item` fields plus the sort
/// key (`project.identifier`, `sequence_id`).
pub struct OwnedRelationItem {
    pub id: String,
    pub name: String,
    pub state: Option<String>,
    pub state_group: Option<String>,
    pub project_identifier: String,
    pub sequence_id: i64,
}

/// One owned assignee row for `expand=assignees`: the `UserLite` inputs
/// plus the resolved avatar URL text. Fetched in
/// `User.objects.filter(pk__in=...)` order (`-created_at`,
/// `db/models/user.py:137`).
pub struct OwnedAssigneeRow {
    pub id: String,
    pub first_name: String,
    pub last_name: String,
    pub email: Option<String>,
    pub avatar: String,
    pub avatar_url: Option<String>,
    pub display_name: String,
}

/// Decoded blocker-target row: issue id, project identifier, sequence,
/// state name, state group.
type BlockerTargetRow = (Uuid, String, i32, Option<String>, Option<String>);

/// Sort key per `_summary_list`: resolved flag, identifier, sequence,
/// then the rendered triple.
type SummarySortKey = (i64, String, i64, Option<String>, Option<String>);

/// Decoded `file_assets` row for [`file_asset_url`]: entity type plus the
/// nullable scope ids.
type AssetUrlLookup = (Option<String>, Option<Uuid>, Option<Uuid>, Option<Uuid>);

/// Decoded user row for [`expand_user`]: names, nullable `email`,
/// `COALESCE`d avatar, optional avatar asset, display name.
type ExpandUserLookup = (
    String,
    String,
    Option<String>,
    String,
    Option<Uuid>,
    Option<String>,
);

/// Row decode helper (made public by OVL-14, PIDASHCONV-1649).
pub fn row_uuid(row: &sqlx::postgres::PgRow, column: &str) -> Result<Uuid, Error> {
    row.try_get(column).map_err(|_| Error::Failed("row-uuid"))
}

/// Row decode helper (made public by OVL-14, PIDASHCONV-1649).
pub fn row_uuid_opt(row: &sqlx::postgres::PgRow, column: &str) -> Result<Option<Uuid>, Error> {
    row.try_get(column)
        .map_err(|_| Error::Failed("row-uuid-opt"))
}

/// Row decode helper (made public by OVL-14, PIDASHCONV-1649).
pub fn row_string(row: &sqlx::postgres::PgRow, column: &str) -> Result<String, Error> {
    row.try_get(column).map_err(|_| Error::Failed("row-string"))
}

/// Row decode helper (made public by OVL-14, PIDASHCONV-1649).
pub fn row_string_opt(row: &sqlx::postgres::PgRow, column: &str) -> Result<Option<String>, Error> {
    row.try_get(column)
        .map_err(|_| Error::Failed("row-string-opt"))
}

/// Row decode helper (made public by OVL-14, PIDASHCONV-1649).
pub fn row_datetime(row: &sqlx::postgres::PgRow, column: &str) -> Result<DateTime<Utc>, Error> {
    row.try_get(column)
        .map_err(|_| Error::Failed("row-datetime"))
}

/// Row decode helper (made public by OVL-14, PIDASHCONV-1649).
pub fn row_datetime_opt(
    row: &sqlx::postgres::PgRow,
    column: &str,
) -> Result<Option<DateTime<Utc>>, Error> {
    row.try_get(column)
        .map_err(|_| Error::Failed("row-datetime-opt"))
}

fn row_date_opt(row: &sqlx::postgres::PgRow, column: &str) -> Result<Option<NaiveDate>, Error> {
    row.try_get(column)
        .map_err(|_| Error::Failed("row-date-opt"))
}

fn row_i32(row: &sqlx::postgres::PgRow, column: &str) -> Result<i32, Error> {
    row.try_get(column).map_err(|_| Error::Failed("row-i32"))
}

fn row_i32_opt(row: &sqlx::postgres::PgRow, column: &str) -> Result<Option<i32>, Error> {
    row.try_get(column)
        .map_err(|_| Error::Failed("row-i32-opt"))
}

fn row_f64(row: &sqlx::postgres::PgRow, column: &str) -> Result<f64, Error> {
    row.try_get(column).map_err(|_| Error::Failed("row-f64"))
}

fn row_bool(row: &sqlx::postgres::PgRow, column: &str) -> Result<bool, Error> {
    row.try_get(column).map_err(|_| Error::Failed("row-bool"))
}

fn row_bytes_opt(row: &sqlx::postgres::PgRow, column: &str) -> Result<Option<Vec<u8>>, Error> {
    row.try_get(column)
        .map_err(|_| Error::Failed("row-bytes-opt"))
}

fn render_dt(dt: &DateTime<Utc>, tz: &Tz) -> String {
    super::render_datetime_in(dt, tz)
}

fn render_dt_opt(dt: Option<DateTime<Utc>>, tz: &Tz) -> Option<String> {
    dt.map(|dt| render_dt(&dt, tz))
}

/// `get_actual_relation` (`utils/issue_relation_mapper.py`): the type
/// stored in the database for a forward-named relation.
fn stored_relation(relation_type: &str) -> &str {
    match relation_type {
        "start_after" => "start_before",
        "finish_after" => "finish_before",
        "blocking" => "blocked_by",
        "implements" => "implemented_by",
        _ => relation_type,
    }
}

/// `get_inverse_relation` (`utils/issue_relation_mapper.py`): the same
/// edge named from the other end.
fn inverse_relation(relation_type: &str) -> &str {
    match relation_type {
        "start_after" => "start_before",
        "finish_after" => "finish_before",
        "blocked_by" => "blocking",
        "blocking" => "blocked_by",
        "start_before" => "start_after",
        "finish_before" => "finish_after",
        "implemented_by" => "implements",
        "implements" => "implemented_by",
        _ => relation_type,
    }
}

/// `_type_from` (`orchestration/relations.py:100-117`): the relation a
/// stored row expresses, named from the viewpoint's side. Rows stored
/// under a reverse name (which the UI never writes but older data may
/// hold) normalize like `blockers` does.
fn type_from_viewpoint(
    stored: &str,
    issue_id: &Uuid,
    related_issue_id: &Uuid,
    viewpoint_id: &Uuid,
) -> String {
    // `REVERSE_TYPES` (`relations.py:67`).
    let (mut relation, mut anchor, mut _other) = (stored, issue_id, related_issue_id);
    if matches!(
        relation,
        "blocking" | "start_after" | "finish_after" | "implements"
    ) {
        relation = stored_relation(relation);
        (anchor, _other) = (related_issue_id, issue_id);
    }
    if anchor == viewpoint_id {
        relation.to_owned()
    } else {
        inverse_relation(relation).to_owned()
    }
}

/// `FileAsset.asset_url` (`db/models/asset.py:80-100`) for an asset id:
/// static types render `/api/assets/v2/static/<id>/`, attachments and
/// description assets join their workspace slug. A missing row (or slug)
/// renders null like `getattr` on a dead FK (the `v1_projects`
/// `file_asset_url` precedent, shared with `handlers_social`).
async fn file_asset_url(pool: &PgPool, asset_id: &Uuid) -> Result<Option<String>, Error> {
    let row: Option<AssetUrlLookup> = sqlx::query_as(
        r#"SELECT fa."entity_type", fa."workspace_id", fa."project_id", fa."issue_id" FROM "file_assets" fa WHERE fa."id" = $1"#,
    )
    .bind(asset_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| DbError::new("file-asset-url", error))?;
    let Some((entity_type, workspace_id, project_id, issue_id)) = row else {
        return Ok(None);
    };
    match entity_type.as_deref() {
        Some("WORKSPACE_LOGO")
        | Some("USER_AVATAR")
        | Some("USER_COVER")
        | Some("PROJECT_COVER") => Ok(Some(format!("/api/assets/v2/static/{asset_id}/"))),
        Some("ISSUE_ATTACHMENT") => {
            let slug = super::workspace::workspace_slug(pool, workspace_id).await?;
            Ok(slug.map(|s| {
                format!(
                    "/api/assets/v2/workspaces/{s}/projects/{}/issues/{}/attachments/{asset_id}/",
                    project_id.map(|v| v.to_string()).unwrap_or_default(),
                    issue_id.map(|v| v.to_string()).unwrap_or_default(),
                )
            }))
        }
        Some("ISSUE_DESCRIPTION")
        | Some("COMMENT_DESCRIPTION")
        | Some("PAGE_DESCRIPTION")
        | Some("DRAFT_ISSUE_DESCRIPTION") => {
            let slug = super::workspace::workspace_slug(pool, workspace_id).await?;
            Ok(slug.map(|s| {
                format!(
                    "/api/assets/v2/workspaces/{s}/projects/{}/{asset_id}/",
                    project_id.map(|v| v.to_string()).unwrap_or_default(),
                )
            }))
        }
        _ => Ok(None),
    }
}

/// Decode one list/detail row selected with [`ISSUE_SELECT_COLS`] plus the
/// `project_identifier` / `workspace_slug` / `state_group` extras.
pub fn decode_issue(row: &sqlx::postgres::PgRow, tz: &Tz) -> Result<DecodedIssue, Error> {
    let created_at_dt = row_datetime(row, "created_at")?;
    Ok(DecodedIssue {
        id: row_uuid(row, "id")?.to_string(),
        created_at: render_dt(&created_at_dt, tz),
        created_at_dt,
        updated_at: render_dt(&row_datetime(row, "updated_at")?, tz),
        deleted_at: render_dt_opt(row_datetime_opt(row, "deleted_at")?, tz),
        point: row_i32_opt(row, "point")?.map(i64::from),
        name: row_string(row, "name")?,
        description_html: row_string(row, "description_html")?,
        description_binary: row_bytes_opt(row, "description_binary")?,
        priority: row_string(row, "priority")?,
        complexity_score: i64::from(row_i32(row, "complexity_score")?),
        start_date: row_date_opt(row, "start_date")?.map(|d| d.to_string()),
        target_date: row_date_opt(row, "target_date")?.map(|d| d.to_string()),
        sequence_id: i64::from(row_i32(row, "sequence_id")?),
        sort_order: row_f64(row, "sort_order")?,
        completed_at: render_dt_opt(row_datetime_opt(row, "completed_at")?, tz),
        archived_at: row_date_opt(row, "archived_at")?.map(|d| d.to_string()),
        is_draft: row_bool(row, "is_draft")?,
        external_source: row_string_opt(row, "external_source")?,
        external_id: row_string_opt(row, "external_id")?,
        git_work_branch: row_string(row, "git_work_branch")?,
        created_via: row_string_opt(row, "created_via")?,
        agent_executor: row_string_opt(row, "agent_executor")?,
        created_by: row_uuid_opt(row, "created_by_id")?.map(|id| id.to_string()),
        updated_by: row_uuid_opt(row, "updated_by_id")?.map(|id| id.to_string()),
        project: row_uuid(row, "project_id")?.to_string(),
        workspace: row_uuid(row, "workspace_id")?.to_string(),
        parent: row_uuid_opt(row, "parent_id")?.map(|id| id.to_string()),
        state: row_uuid_opt(row, "state_id")?.map(|id| id.to_string()),
        estimate_point: row_uuid_opt(row, "estimate_point_id")?.map(|id| id.to_string()),
        type_id: row_uuid_opt(row, "type_id")?.map(|id| id.to_string()),
        assigned_pod: row_uuid_opt(row, "assigned_pod_id")?.map(|id| id.to_string()),
        project_identifier: row_string(row, "project_identifier")?,
        workspace_slug: row_string(row, "workspace_slug")?,
        state_group: row_string_opt(row, "state_group")?,
    })
}

/// The `prefetch_related("assignees")` ids
/// (`IssueAssignee.objects.filter(issue=...)`, `-created_at` order).
pub async fn fetch_assignee_ids(pool: &PgPool, issue_id: &Uuid) -> Result<Vec<String>, Error> {
    let ids: Vec<Uuid> = sqlx::query_scalar(
        r#"SELECT "assignee_id" FROM "issue_assignees" WHERE "issue_id" = $1 AND "deleted_at" IS NULL ORDER BY "created_at" DESC"#,
    )
    .bind(issue_id)
    .fetch_all(pool)
    .await
    .map_err(|error| DbError::new("issue-assignees", error))?;
    Ok(ids.iter().map(ToString::to_string).collect())
}

/// The `prefetch_related("labels")` ids (`IssueLabel.objects.filter(issue=...)`,
/// `-created_at` order).
pub async fn fetch_label_ids(pool: &PgPool, issue_id: &Uuid) -> Result<Vec<String>, Error> {
    let ids: Vec<Uuid> = sqlx::query_scalar(
        r#"SELECT "label_id" FROM "issue_labels" WHERE "issue_id" = $1 AND "deleted_at" IS NULL ORDER BY "created_at" DESC"#,
    )
    .bind(issue_id)
    .fetch_all(pool)
    .await
    .map_err(|error| DbError::new("issue-labels", error))?;
    Ok(ids.iter().map(ToString::to_string).collect())
}

/// The `_requested_fields` plain names (`serializers/issue.py:172-176`):
/// every `Include` name, including non-field names like
/// `relations_summary`. Empty (no `fields=`) shows the summary keys.
pub fn requested_names(specs: Option<&[FieldSpec]>) -> Vec<&str> {
    specs
        .unwrap_or(&[])
        .iter()
        .filter_map(|spec| match spec {
            FieldSpec::Include(name) => Some(name.as_str()),
            FieldSpec::Nested(_, _) => None,
        })
        .collect()
}

/// `relations_summary` (`orchestration/blockers.py:186-198`): per-direction
/// `{identifier, state, state_group}` lists, open first, capped at 100
/// (`SUMMARY_LIMIT`), plus the uncapped `has_open_blockers`. Mirrors the
/// `handlers_social` fetch (same source, same rows).
pub async fn fetch_blocker_rows(pool: &PgPool, issue_id: &Uuid) -> Result<BlockerRows, Error> {
    async fn direction(
        pool: &PgPool,
        issue_id: &Uuid,
        blocked_by: bool,
    ) -> Result<Vec<(String, Option<String>, Option<String>)>, Error> {
        let mut seen = std::collections::HashSet::new();
        let mut items: Vec<SummarySortKey> = Vec::new();
        for relation_type in ["blocked_by", "blocking"] {
            let (anchor_col, target_col) = if blocked_by {
                match relation_type {
                    "blocked_by" => ("issue_id", "related_issue_id"),
                    _ => ("related_issue_id", "issue_id"),
                }
            } else {
                match relation_type {
                    "blocked_by" => ("related_issue_id", "issue_id"),
                    _ => ("issue_id", "related_issue_id"),
                }
            };
            let rows: Vec<BlockerTargetRow> = sqlx::query_as(&format!(
                r#"SELECT t."id", p."identifier", t."sequence_id", s."name", s."group"
                   FROM "issue_relations" r
                   INNER JOIN "issues" t ON t."id" = r."{target_col}"
                   INNER JOIN "states" s ON s."id" = t."state_id"
                   INNER JOIN "projects" p ON p."id" = t."project_id"
                   WHERE r."deleted_at" IS NULL AND r."issue_id" <> r."related_issue_id"
                     AND r."{anchor_col}" = $1 AND r."relation_type" = $2
                     AND t."deleted_at" IS NULL AND t."archived_at" IS NULL AND t."is_draft" = FALSE
                     AND t."workspace_id" = r."workspace_id"
                     AND s."group" <> 'triage' AND p."archived_at" IS NULL"#,
            ))
            .bind(issue_id)
            .bind(relation_type)
            .fetch_all(pool)
            .await
            .map_err(|error| DbError::new("expand-blockers", error))?;
            for (id, identifier, sequence_id, state, group) in rows {
                if seen.insert(id) {
                    let resolved =
                        i64::from(matches!(group.as_deref(), Some("completed" | "cancelled")));
                    items.push((resolved, identifier, i64::from(sequence_id), state, group));
                }
            }
        }
        items.sort_by(|a, b| (a.0, &a.1, a.2).cmp(&(b.0, &b.1, b.2)));
        items.truncate(100);
        Ok(items
            .into_iter()
            .map(|(_, identifier, sequence_id, state, group)| {
                (format!("{identifier}-{sequence_id}"), state, group)
            })
            .collect())
    }
    let blocked_by = direction(pool, issue_id, true).await?;
    let blocking = direction(pool, issue_id, false).await?;
    let has_open_blockers: bool = sqlx::query_scalar(
        r#"SELECT EXISTS(
             SELECT 1 FROM "issue_relations" r
             INNER JOIN "issues" t ON t."id" = CASE WHEN r."relation_type" = 'blocked_by' THEN r."related_issue_id" ELSE r."issue_id" END
             INNER JOIN "states" s ON s."id" = t."state_id"
             INNER JOIN "projects" p ON p."id" = t."project_id"
             WHERE r."deleted_at" IS NULL AND r."issue_id" <> r."related_issue_id"
               AND t."deleted_at" IS NULL AND t."archived_at" IS NULL AND t."is_draft" = FALSE
               AND t."workspace_id" = r."workspace_id"
               AND s."group" <> 'triage' AND p."archived_at" IS NULL
               AND (
                 (r."issue_id" = $1 AND r."relation_type" = 'blocked_by')
                 OR (r."related_issue_id" = $1 AND r."relation_type" = 'blocking')
               )
               AND s."group" NOT IN ('completed', 'cancelled')
           )"#,
    )
    .bind(issue_id)
    .fetch_one(pool)
    .await
    .map_err(|error| DbError::new("expand-open-blockers", error))?;
    Ok(BlockerRows {
        blocked_by,
        blocking,
        has_open_blockers,
    })
}

/// `grouped_relations(issue, member_project_issues(viewer, slug))`
/// (`relations.py:280-316` + `core/querysets.py:19-30`): every live
/// relation of the issue, grouped by type from its side, the other ends
/// narrowed to the viewer's member projects. Returns the ten
/// [`shape::RELATION_TYPES`] groups in order, each sorted by
/// `(project.identifier, sequence_id)` and capped at
/// [`shape::GROUP_LIMIT`]. Unknown stored types drop (the `in
/// other_by_type` guard).
#[allow(clippy::type_complexity)]
pub async fn fetch_grouped_rows(
    pool: &PgPool,
    issue_id: &Uuid,
    workspace_id: &Uuid,
    viewer_id: &Uuid,
) -> Result<Vec<(String, Vec<OwnedRelationItem>)>, Error> {
    // `IssueRelation.objects.filter(Q(issue|related) =
    // issue, workspace).exclude(self-loop)` — the default manager is the
    // soft-deletion scope.
    let edges: Vec<(Uuid, Uuid, String)> = sqlx::query_as(
        r#"SELECT "issue_id", "related_issue_id", "relation_type" FROM "issue_relations"
           WHERE ("issue_id" = $1 OR "related_issue_id" = $1) AND "workspace_id" = $2
             AND "deleted_at" IS NULL AND NOT ("issue_id" = $1 AND "related_issue_id" = $1)"#,
    )
    .bind(issue_id)
    .bind(workspace_id)
    .fetch_all(pool)
    .await
    .map_err(|error| DbError::new("relations-edges", error))?;
    let mut other_by_type: Vec<(String, std::collections::HashSet<Uuid>)> = shape::RELATION_TYPES
        .iter()
        .map(|name| (name.to_string(), std::collections::HashSet::new()))
        .collect();
    for (anchor, related, stored) in &edges {
        let other = if anchor == issue_id {
            *related
        } else {
            *anchor
        };
        let relation = type_from_viewpoint(stored, anchor, related, issue_id);
        if let Some(group) = other_by_type.iter_mut().find(|(name, _)| *name == relation) {
            group.1.insert(other);
        }
    }
    let wanted: Vec<Uuid> = other_by_type
        .iter()
        .flat_map(|(_, ids)| ids.iter().copied())
        .collect();
    // The member-project pool (`member_project_issues` + the
    // `workspace_id` narrow + the manager scope), keyed by id. Ordering
    // is cleared (`.order_by()`) — the Rust sort below applies.
    let mut others: HashMap<Uuid, OwnedRelationItem> = HashMap::new();
    if !wanted.is_empty() {
        let rows: Vec<(Uuid, String, i32, Option<String>, Option<String>, String)> = sqlx::query_as(
            format!(
                r#"SELECT "issues"."id", "issues"."name", "issues"."sequence_id", "states"."name", "states"."group", "projects"."identifier"
                   FROM "issues"
                   LEFT OUTER JOIN "states" ON ("issues"."state_id" = "states"."id")
                   INNER JOIN "projects" ON ("issues"."project_id" = "projects"."id")
                   WHERE "issues"."id" = ANY($1) AND "issues"."workspace_id" = $2 AND {} AND EXISTS(
                     SELECT 1 FROM "project_members" pm
                     WHERE pm."project_id" = "issues"."project_id" AND pm."member_id" = $3
                       AND pm."is_active" AND pm."deleted_at" IS NULL
                   )"#,
                core_queries::ISSUE_MANAGER_WHERE
            )
            .as_str(),
        )
        .bind(&wanted)
        .bind(workspace_id)
        .bind(viewer_id)
        .fetch_all(pool)
        .await
        .map_err(|error| DbError::new("relations-pool", error))?;
        for (id, name, sequence_id, state, state_group, project_identifier) in rows {
            others.insert(
                id,
                OwnedRelationItem {
                    id: id.to_string(),
                    name,
                    state,
                    state_group,
                    project_identifier,
                    sequence_id: i64::from(sequence_id),
                },
            );
        }
    }
    let mut groups = Vec::with_capacity(other_by_type.len());
    for (name, ids) in &other_by_type {
        let mut items: Vec<&OwnedRelationItem> =
            ids.iter().filter_map(|id| others.get(id)).collect();
        items.sort_by(|a, b| {
            (&a.project_identifier, a.sequence_id).cmp(&(&b.project_identifier, b.sequence_id))
        });
        items.truncate(shape::GROUP_LIMIT);
        let owned: Vec<OwnedRelationItem> = items
            .into_iter()
            .map(|item| OwnedRelationItem {
                id: item.id.clone(),
                name: item.name.clone(),
                state: item.state.clone(),
                state_group: item.state_group.clone(),
                project_identifier: item.project_identifier.clone(),
                sequence_id: item.sequence_id,
            })
            .collect();
        groups.push((name.clone(), owned));
    }
    Ok(groups)
}

/// `expand=state`: `StateLiteSerializer` (`state.py:44-55`) via the D-19
/// kernel. `State.objects` is the triage-excluding `StateManager`
/// (`db/models/state.py:79-83`); the triage arm is unreachable here (the
/// manager scope never yields triage-group issues), so the fetch carries
/// the soft-delete scope only. A missing row renders `{}` via `None`
/// (null FKs never reach here — the caller maps them to `None`).
pub async fn expand_state(pool: &PgPool, state_id: &Uuid) -> Result<Option<Value>, Error> {
    use crate::v1_projects::ser_workflow as workflow;
    let row: Option<(String, String, String)> = sqlx::query_as(
        r#"SELECT "name", "color", "group" FROM "states" WHERE "id" = $1 AND "deleted_at" IS NULL"#,
    )
    .bind(state_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| DbError::new("expand-state", error))?;
    let Some((name, color, group)) = row else {
        return Ok(None);
    };
    let id = state_id.to_string();
    let lite = workflow::StateLiteRow {
        id: &id,
        name: &name,
        color: &color,
        group: &group,
    };
    let view = workflow::state_lite_to_representation(&lite);
    serde_json::to_value(&view)
        .map(Some)
        .map_err(|_| Error::Failed("expand-state"))
}

/// `expand=project`: `ProjectLiteSerializer` (`project.py:354-377`):
/// `id`, `identifier`, `name`, `cover_image`, `icon_prop`, `emoji`,
/// `description`, `is_default`, `cover_image_url` in field order.
/// `Project.objects` is the plain manager — no liveness scope, so
/// soft-deleted rows still render (Django `getattr` mechanics). A
/// hard-missing row renders `{}` via `None`. Mirrors the
/// `handlers_social` fetch.
pub async fn expand_project(pool: &PgPool, project_id: &Uuid) -> Result<Option<Value>, Error> {
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(
        r#"SELECT "id", "identifier", "name", "cover_image", "icon_prop", "emoji", "description", "is_default", "cover_image_asset_id" FROM "projects" WHERE "id" = $1"#,
    )
    .bind(project_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| DbError::new("expand-project", error))?;
    let Some(row) = row else {
        return Ok(None);
    };
    let cover_asset: Option<Uuid> = row_uuid_opt(&row, "cover_image_asset_id")?;
    let cover_text: Option<String> = row_string_opt(&row, "cover_image")?;
    // `Project.cover_image_url` (`db/models/project.py:176-185`): the
    // asset URL wins, then the stored text, else null.
    let cover_url = match cover_asset {
        Some(id) => file_asset_url(pool, &id).await?,
        None => None,
    };
    let cover_url = cover_url.or_else(|| cover_text.clone().filter(|text| !text.is_empty()));
    let icon_prop: Option<Value> = row
        .try_get("icon_prop")
        .map_err(|_| Error::Failed("expand-project"))?;
    let mut map = Map::with_capacity(9);
    map.insert(
        "id".to_owned(),
        Value::String(row_uuid(&row, "id")?.to_string()),
    );
    map.insert(
        "identifier".to_owned(),
        Value::String(row_string(&row, "identifier")?),
    );
    map.insert("name".to_owned(), Value::String(row_string(&row, "name")?));
    map.insert(
        "cover_image".to_owned(),
        cover_text.map(Value::String).unwrap_or(Value::Null),
    );
    map.insert("icon_prop".to_owned(), icon_prop.unwrap_or(Value::Null));
    map.insert(
        "emoji".to_owned(),
        row_string_opt(&row, "emoji")?
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    map.insert(
        "description".to_owned(),
        Value::String(row_string(&row, "description")?),
    );
    map.insert(
        "is_default".to_owned(),
        Value::Bool(
            row.try_get("is_default")
                .map_err(|_| Error::Failed("expand-project"))?,
        ),
    );
    map.insert(
        "cover_image_url".to_owned(),
        cover_url.map(Value::String).unwrap_or(Value::Null),
    );
    Ok(Some(Value::Object(map)))
}

/// `expand=workspace`: `WorkspaceLiteSerializer`
/// (`workspace.py:10-21`): `name`, `slug`, `id` in field order. Plain
/// manager — no liveness scope. A hard-missing row renders `{}` via
/// `None`. Mirrors the `handlers_social` fetch.
pub async fn expand_workspace(pool: &PgPool, workspace_id: &Uuid) -> Result<Option<Value>, Error> {
    let row: Option<(String, String)> =
        sqlx::query_as(r#"SELECT "name", "slug" FROM "workspaces" WHERE "id" = $1"#)
            .bind(workspace_id)
            .fetch_optional(pool)
            .await
            .map_err(|error| DbError::new("expand-workspace", error))?;
    Ok(match row {
        Some((name, slug)) => {
            let mut map = Map::with_capacity(3);
            map.insert("name".to_owned(), Value::String(name));
            map.insert("slug".to_owned(), Value::String(slug));
            map.insert("id".to_owned(), Value::String(workspace_id.to_string()));
            Some(Value::Object(map))
        }
        None => None,
    })
}

/// `expand=created_by` / `expand=updated_by`: `UserLiteSerializer`
/// (`user.py:13-38`) via the D-19 kernel. `User.objects` is the plain
/// `UserManager` — no liveness scope. A hard-missing row renders `{}`
/// via `None`. Mirrors the `handlers_social` fetch.
pub async fn expand_user(pool: &PgPool, user_id: &Uuid) -> Result<Option<Value>, Error> {
    use crate::v1_projects::ser_collab as collab;
    let row: Option<ExpandUserLookup> = sqlx::query_as(
        r#"SELECT "first_name", "last_name", "email", COALESCE("avatar", ''), "avatar_asset_id", "display_name" FROM "users" WHERE "id" = $1"#,
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| DbError::new("expand-user", error))?;
    let Some((first_name, last_name, email, avatar, avatar_asset, display_name)) = row else {
        return Ok(None);
    };
    let asset_url = match avatar_asset {
        Some(id) => file_asset_url(pool, &id).await?,
        None => None,
    };
    let avatar_url =
        collab::resolve_avatar_url(avatar_asset.is_some(), asset_url.as_deref(), &avatar);
    let id = user_id.to_string();
    let display_name = display_name.unwrap_or_default();
    let lite = collab::UserLiteRow {
        id: &id,
        first_name: &first_name,
        last_name: &last_name,
        email: email.as_deref(),
        avatar: &avatar,
        avatar_url,
        display_name: &display_name,
    };
    let view = collab::user_lite_to_representation(&lite);
    serde_json::to_value(&view)
        .map(Some)
        .map_err(|_| Error::Failed("expand-user"))
}

/// `expand=parent`: `IssueLiteSerializer` over the parent issue
/// (`serializers/issue.py:497-508`): `id`, `sequence_id`, `project_id`
/// in field order. `Issue.objects` (the forward-FK manager) is plain —
/// soft-deleted parents still render. A hard-missing row is unreachable
/// (`CASCADE`) and renders `{}` via `None`.
pub async fn expand_parent(pool: &PgPool, parent_id: &Uuid) -> Result<Option<Value>, Error> {
    use crate::v1_work_items::shape_labels as labels;
    let row: Option<(Uuid, i32, Uuid)> =
        sqlx::query_as(r#"SELECT "id", "sequence_id", "project_id" FROM "issues" WHERE "id" = $1"#)
            .bind(parent_id)
            .fetch_optional(pool)
            .await
            .map_err(|error| DbError::new("expand-parent", error))?;
    let Some((id, sequence_id, project_id)) = row else {
        return Ok(None);
    };
    let id_text = id.to_string();
    let project_text = project_id.to_string();
    let rendered = labels::render_issue_lite(&labels::IssueLiteRow {
        id: &id_text,
        sequence_id: Some(i64::from(sequence_id)),
        project_id: &project_text,
    });
    Ok(Some(Value::Object(rendered)))
}

/// `expand=estimate_point`: the API `EstimatePointSerializer` read shape
/// (`estimate.py:25-37`, `fields = "__all__"`) via the D-19 kernel, in
/// DRF wire order. Plain manager — no liveness scope. A hard-missing
/// row renders `{}` via `None`.
pub async fn expand_estimate_point(
    pool: &PgPool,
    estimate_point_id: &Uuid,
    tz: &Tz,
) -> Result<Option<Value>, Error> {
    use crate::v1_projects::ser_workflow as workflow;
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(
        r#"SELECT "id", "created_at", "updated_at", "created_by_id", "updated_by_id", "deleted_at", "project_id", "workspace_id", "estimate_id", "key", "description", "value" FROM "estimate_points" WHERE "id" = $1"#,
    )
    .bind(estimate_point_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| DbError::new("expand-estimate-point", error))?;
    let Some(row) = row else {
        return Ok(None);
    };
    let id = row_uuid(&row, "id")?.to_string();
    let created_at = render_dt(&row_datetime(&row, "created_at")?, tz);
    let updated_at = render_dt(&row_datetime(&row, "updated_at")?, tz);
    let created_by = row_uuid_opt(&row, "created_by_id")?.map(|id| id.to_string());
    let updated_by = row_uuid_opt(&row, "updated_by_id")?.map(|id| id.to_string());
    let deleted_at = render_dt_opt(row_datetime_opt(&row, "deleted_at")?, tz);
    let project = row_uuid(&row, "project_id")?.to_string();
    let workspace = row_uuid(&row, "workspace_id")?.to_string();
    let estimate = row_uuid(&row, "estimate_id")?.to_string();
    let key: i32 = row
        .try_get("key")
        .map_err(|_| Error::Failed("expand-estimate-point"))?;
    let description = row_string(&row, "description")?;
    let value = row_string(&row, "value")?;
    let point_row = workflow::EstimatePointRow {
        id: &id,
        created_at: &created_at,
        updated_at: &updated_at,
        created_by: created_by.as_deref(),
        updated_by: updated_by.as_deref(),
        deleted_at: deleted_at.as_deref(),
        project: &project,
        workspace: &workspace,
        estimate: &estimate,
        key,
        description: &description,
        value: &value,
    };
    let view = workflow::estimate_point_to_representation(&point_row);
    serde_json::to_value(&view)
        .map(Some)
        .map_err(|_| Error::Failed("expand-estimate-point"))
}

/// `expand=assignees`: `UserLiteSerializer(User.objects.filter(pk__in=...),
/// many=True)` (`serializers/issue.py:445-452`). Unscoped, `-created_at`
/// order; deactivated users still render.
#[allow(clippy::type_complexity)]
pub async fn fetch_assignee_rows(
    pool: &PgPool,
    issue_id: &Uuid,
) -> Result<Vec<OwnedAssigneeRow>, Error> {
    let rows: Vec<(Uuid, String, String, Option<String>, String, Option<Uuid>, Option<String>)> =
        sqlx::query_as(
            r#"SELECT u."id", u."first_name", u."last_name", u."email", COALESCE(u."avatar", ''), u."avatar_asset_id", u."display_name"
               FROM "users" u WHERE u."id" IN (
                 SELECT "assignee_id" FROM "issue_assignees" WHERE "issue_id" = $1 AND "deleted_at" IS NULL
               ) ORDER BY u."created_at" DESC"#,
        )
        .bind(issue_id)
        .fetch_all(pool)
        .await
        .map_err(|error| DbError::new("expand-assignees", error))?;
    let mut out = Vec::with_capacity(rows.len());
    for (id, first_name, last_name, email, avatar, avatar_asset, display_name) in rows {
        let asset_url = match avatar_asset {
            Some(asset) => file_asset_url(pool, &asset).await?,
            None => None,
        };
        let resolved = crate::v1_projects::ser_collab::resolve_avatar_url(
            avatar_asset.is_some(),
            asset_url.as_deref(),
            &avatar,
        );
        let avatar_url = resolved.map(str::to_owned);
        out.push(OwnedAssigneeRow {
            id: id.to_string(),
            first_name,
            last_name,
            email,
            avatar,
            avatar_url,
            display_name: display_name.unwrap_or_default(),
        });
    }
    Ok(out)
}

/// `expand=labels`: `LabelSerializer(Label.objects.filter(pk__in=...),
/// many=True)` (`serializers/issue.py:459-466`) — the full label read
/// shape per row, nested construction (no `fields=`, no `expand=`).
/// Unscoped, `-created_at` order (`db/models/label.py:44`).
pub async fn fetch_expanded_labels(
    pool: &PgPool,
    issue_id: &Uuid,
    tz: &Tz,
) -> Result<Vec<Value>, Error> {
    use crate::v1_work_items::shape_labels as labels;
    let rows: Vec<sqlx::postgres::PgRow> = sqlx::query(
        r#"SELECT l."id", l."created_at", l."updated_at", l."deleted_at", l."name", l."description", l."color", l."sort_order", l."external_source", l."external_id", l."created_by_id", l."updated_by_id", l."workspace_id", l."project_id", l."parent_id"
           FROM "labels" l WHERE l."id" IN (
             SELECT "label_id" FROM "issue_labels" WHERE "issue_id" = $1 AND "deleted_at" IS NULL
           ) ORDER BY l."created_at" DESC"#,
    )
    .bind(issue_id)
    .fetch_all(pool)
    .await
    .map_err(|error| DbError::new("expand-labels", error))?;
    let mut out = Vec::with_capacity(rows.len());
    for row in &rows {
        let id = row_uuid(row, "id")?.to_string();
        let created_at = render_dt(&row_datetime(row, "created_at")?, tz);
        let updated_at = render_dt(&row_datetime(row, "updated_at")?, tz);
        let deleted_at = render_dt_opt(row_datetime_opt(row, "deleted_at")?, tz);
        let name = row_string(row, "name")?;
        let description = row_string(row, "description")?;
        let color = row_string(row, "color")?;
        let sort_order = row_f64(row, "sort_order")?;
        let external_source = row_string_opt(row, "external_source")?;
        let external_id = row_string_opt(row, "external_id")?;
        let created_by = row_uuid_opt(row, "created_by_id")?.map(|id| id.to_string());
        let updated_by = row_uuid_opt(row, "updated_by_id")?.map(|id| id.to_string());
        let workspace = row_uuid(row, "workspace_id")?.to_string();
        let project = row_uuid_opt(row, "project_id")?.map(|id| id.to_string());
        let parent = row_uuid_opt(row, "parent_id")?.map(|id| id.to_string());
        let label_row = labels::LabelRow {
            id: &id,
            created_at: &created_at,
            updated_at: &updated_at,
            deleted_at: deleted_at.as_deref(),
            name: &name,
            description: &description,
            color: &color,
            sort_order,
            external_source: external_source.as_deref(),
            external_id: external_id.as_deref(),
            created_by: created_by.as_deref(),
            updated_by: updated_by.as_deref(),
            workspace: &workspace,
            project: project.as_deref(),
            parent: parent.as_deref(),
        };
        let no_expand: &[&str] = &[];
        let no_expansions: &[(&str, Option<Value>)] = &[];
        let rendered = labels::render_label(&labels::LabelRepresentationInput {
            row: &label_row,
            fields: None,
            expand: no_expand,
            expansions: no_expansions,
        })
        .map_err(|_| Error::Failed("fetch-expanded-labels"))?;
        out.push(Value::Object(rendered));
    }
    Ok(out)
}

/// Render one decoded row through [`shape::render_issue`]: the id lists,
/// the `?fields=`-gated blocker summary and viewer relations, and the
/// kept `expand` values. Every fetch below mirrors a Python query that
/// only runs when its gate passes, so gated-out keys cost no queries.
pub async fn render_decoded(
    pool: &PgPool,
    decoded: &DecodedIssue,
    tz: &Tz,
    req: &RenderRequest<'_>,
) -> Result<Value, Error> {
    let kept = |name: &str| req.kept.iter().any(|kept| kept == name);
    let expand = |name: &str| req.expand.contains(&name);
    let requested = requested_names(req.field_specs);

    let url = shape::issue_url(
        req.web_base,
        Some(decoded.workspace_slug.as_str()),
        Some(decoded.project_identifier.as_str()),
        Some(decoded.sequence_id),
    );
    let row = shape::IssueRow {
        id: &decoded.id,
        type_id: decoded.type_id.as_deref(),
        url,
        created_at: &decoded.created_at,
        updated_at: &decoded.updated_at,
        deleted_at: decoded.deleted_at.as_deref(),
        point: decoded.point,
        name: &decoded.name,
        description_html: &decoded.description_html,
        description_binary: decoded.description_binary.as_deref(),
        priority: &decoded.priority,
        complexity_score: decoded.complexity_score,
        start_date: decoded.start_date.as_deref(),
        target_date: decoded.target_date.as_deref(),
        sequence_id: decoded.sequence_id,
        sort_order: decoded.sort_order,
        completed_at: decoded.completed_at.as_deref(),
        archived_at: decoded.archived_at.as_deref(),
        is_draft: decoded.is_draft,
        external_source: decoded.external_source.as_deref(),
        external_id: decoded.external_id.as_deref(),
        git_work_branch: &decoded.git_work_branch,
        created_via: decoded.created_via.as_deref(),
        agent_executor: decoded.agent_executor.as_deref(),
        created_by: decoded.created_by.as_deref(),
        updated_by: decoded.updated_by.as_deref(),
        project: &decoded.project,
        workspace: &decoded.workspace,
        parent: decoded.parent.as_deref(),
        state: decoded.state.as_deref(),
        estimate_point: decoded.estimate_point.as_deref(),
        assigned_pod: decoded.assigned_pod.as_deref(),
    };

    let issue_id = decoded
        .id
        .parse::<Uuid>()
        .map_err(|_| Error::Failed("render-decoded"))?;

    // `assignees` / `labels` (`issue.py:442-468`): ids always when kept,
    // objects under `expand=`.
    let mut assignee_ids: Vec<String> = Vec::new();
    let mut label_ids: Vec<String> = Vec::new();
    let mut owned_assignees: Vec<OwnedAssigneeRow> = Vec::new();
    let mut expanded_labels: Vec<Value> = Vec::new();
    if kept("assignees") {
        if expand("assignees") {
            owned_assignees = fetch_assignee_rows(pool, &issue_id).await?;
        } else {
            assignee_ids = fetch_assignee_ids(pool, &issue_id).await?;
        }
    }
    if kept("labels") {
        if expand("labels") {
            expanded_labels = fetch_expanded_labels(pool, &issue_id, tz).await?;
        } else {
            label_ids = fetch_label_ids(pool, &issue_id).await?;
        }
    }
    let assignee_refs: Vec<&str> = assignee_ids.iter().map(String::as_str).collect();
    let label_refs: Vec<&str> = label_ids.iter().map(String::as_str).collect();
    let user_rows: Vec<crate::v1_projects::ser_collab::UserLiteRow<'_>> = owned_assignees
        .iter()
        .map(|row| crate::v1_projects::ser_collab::UserLiteRow {
            id: &row.id,
            first_name: &row.first_name,
            last_name: &row.last_name,
            email: row.email.as_deref(),
            avatar: &row.avatar,
            avatar_url: row.avatar_url.as_deref(),
            display_name: &row.display_name,
        })
        .collect();

    // Blocker summary (`issue.py:470-481`): single payloads only,
    // `?fields=`-gated.
    let summary_gate = !req.is_list
        && (requested.is_empty()
            || requested
                .iter()
                .any(|name| shape::RELATIONS_SUMMARY_KEYS.contains(name)));
    let mut blocker_rows: Option<BlockerRows> = None;
    if summary_gate {
        blocker_rows = Some(fetch_blocker_rows(pool, &issue_id).await?);
    }
    let blockers = blocker_rows.as_ref().map(|rows| shape::BlockerSummary {
        blocked_by: rows
            .blocked_by
            .iter()
            .map(|(identifier, state, group)| shape::SummaryItem {
                identifier: identifier.clone(),
                state: state.as_deref(),
                state_group: group.as_deref(),
            })
            .collect(),
        blocking: rows
            .blocking
            .iter()
            .map(|(identifier, state, group)| shape::SummaryItem {
                identifier: identifier.clone(),
                state: state.as_deref(),
                state_group: group.as_deref(),
            })
            .collect(),
        has_open_blockers: rows.has_open_blockers,
    });

    // Viewer relations (`issue.py:483-492`): viewer set, single payload,
    // `?fields=`-gated.
    let relations_gate = req.viewer.is_some()
        && !req.is_list
        && (requested.is_empty() || requested.contains(&"relations"));
    let mut grouped: Vec<(String, Vec<OwnedRelationItem>)> = Vec::new();
    if relations_gate {
        let workspace_id = decoded
            .workspace
            .parse::<Uuid>()
            .map_err(|_| Error::Failed("render-decoded"))?;
        grouped =
            fetch_grouped_rows(pool, &issue_id, &workspace_id, &req.viewer.expect("gated")).await?;
    }
    let mut relation_lists: Vec<Vec<shape::RelationItem<'_>>> = Vec::with_capacity(grouped.len());
    for (_, items) in &grouped {
        relation_lists.push(
            items
                .iter()
                .map(|item| shape::RelationItem {
                    id: &item.id,
                    identifier: format!("{}-{}", item.project_identifier, item.sequence_id),
                    name: &item.name,
                    state: item.state.as_deref(),
                    state_group: item.state_group.as_deref(),
                })
                .collect(),
        );
    }
    // The ten lists in `RELATION_TYPES` order (the fetch returns them in
    // that order); absent groups (never — the fetch always returns ten)
    // read as empty.
    let at = |index: usize| relation_lists.get(index).map(Vec::as_slice).unwrap_or(&[]);
    let relations = if relations_gate {
        Some(shape::GroupedRelations {
            blocked_by: at(0).to_vec(),
            blocking: at(1).to_vec(),
            relates_to: at(2).to_vec(),
            duplicate: at(3).to_vec(),
            start_before: at(4).to_vec(),
            start_after: at(5).to_vec(),
            finish_before: at(6).to_vec(),
            finish_after: at(7).to_vec(),
            implemented_by: at(8).to_vec(),
            implements: at(9).to_vec(),
        })
    } else {
        None
    };

    // Base expansions (`base.py:76-116`): kept map-hit names only. Null
    // FKs map to `None` (`{}`); the fetch fns map hard-missing rows the
    // same way.
    let mut expansions: Vec<(&str, Option<Value>)> = Vec::new();
    let parse_id = |text: &str| {
        text.parse::<Uuid>()
            .map_err(|_| Error::Failed("render-decoded"))
    };
    for name in shape::BASE_EXPANSION_NAMES {
        if !(req.expand.contains(name) && kept(name)) {
            continue;
        }
        let value: Option<Value> = match *name {
            "state" => match decoded.state.as_deref() {
                Some(id) => expand_state(pool, &parse_id(id)?).await?,
                None => None,
            },
            "project" => expand_project(pool, &parse_id(&decoded.project)?).await?,
            "workspace" => expand_workspace(pool, &parse_id(&decoded.workspace)?).await?,
            "created_by" => match decoded.created_by.as_deref() {
                Some(id) => expand_user(pool, &parse_id(id)?).await?,
                None => None,
            },
            "updated_by" => match decoded.updated_by.as_deref() {
                Some(id) => expand_user(pool, &parse_id(id)?).await?,
                None => None,
            },
            "parent" => match decoded.parent.as_deref() {
                Some(id) => expand_parent(pool, &parse_id(id)?).await?,
                None => None,
            },
            "estimate_point" => match decoded.estimate_point.as_deref() {
                Some(id) => expand_estimate_point(pool, &parse_id(id)?, tz).await?,
                None => None,
            },
            // Other map names (`user`, `issue`, `actor`, ...) are not
            // issue fields, so they are never kept — unreachable.
            _ => return Err(Error::Failed("render-decoded")),
        };
        expansions.push((name, value));
    }

    // Every `RenderError` arm is 500-class: `Fields` is unreachable
    // (query strings only build `Include` specs), the binary/NaN arms
    // reproduce Django 500s, and the `Missing*` arms are unreachable by
    // construction (the gates above fetch what the branch needs).
    let rendered = shape::render_issue(&shape::RepresentationInput {
        row: &row,
        fields: req.field_specs,
        expand: req.expand,
        is_list: req.is_list,
        assignee_ids: &assignee_refs,
        assignee_rows: &user_rows,
        label_ids: &label_refs,
        expanded_labels: &expanded_labels,
        blockers: blockers.as_ref(),
        relations: relations.as_ref(),
        expansions: &expansions,
    })
    .map_err(|_| Error::Failed("render-decoded"))?;
    Ok(Value::Object(rendered))
}

/// Fetch one row for the write paths (`Issue.objects.get(workspace__slug,
/// project_id, pk)`): the DEFAULT manager scope (`deleted_at IS NULL`
/// only — drafts, archived and triage rows are visible, unlike the GET
/// paths). Miss → the 404 `handle_exception` body.
pub async fn fetch_issue_for_write(
    pool: &PgPool,
    slug: &str,
    project_id: &Uuid,
    pk: &Uuid,
    tz: &Tz,
) -> Result<DecodedIssue, Error> {
    let sql = format!(
        "SELECT {}, {} FROM \"issues\" {} WHERE \"issues\".\"deleted_at\" IS NULL AND \"issues\".\"id\" = $1 AND \"issues\".\"project_id\" = $2 AND \"workspaces\".\"slug\" = $3 LIMIT 1",
        ISSUE_SELECT_COLS,
        ISSUE_EXTRA_COLS,
        core_queries::inline_lookup_joins_sql()
    );
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(&sql)
        .bind(pk)
        .bind(project_id)
        .bind(slug)
        .fetch_optional(pool)
        .await
        .map_err(|error| DbError::new("write-fetch", error))?;
    match row {
        Some(row) => decode_issue(&row, tz),
        None => Err(Error::NotFound),
    }
}

/// The project row the issue-write paths need: the stored workspace id
/// plus the default assignee (`Project.objects.get`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WriteProject {
    pub workspace_id: Uuid,
    pub default_assignee_id: Option<Uuid>,
}

/// Fetch the project row for an issue write: a miss is [`Error::NotFound`].
/// (Moved from the v1 work-items handlers by OVL-14, PIDASHCONV-1649.)
pub async fn fetch_project_for_write(
    pool: &PgPool,
    project_id: &Uuid,
) -> Result<WriteProject, Error> {
    let row: Option<(Uuid, Option<Uuid>)> =
        sqlx::query_as(r#"SELECT "workspace_id", "default_assignee_id" FROM "projects" WHERE "id" = $1 AND "deleted_at" IS NULL"#)
            .bind(project_id)
            .fetch_optional(pool)
            .await
            .map_err(|error| DbError::new("write-project", error))?;
    match row {
        Some((workspace_id, default_assignee_id)) => Ok(WriteProject {
            workspace_id,
            default_assignee_id,
        }),
        None => Err(Error::NotFound),
    }
}

/// Render a write response / `current_instance` image: the full
/// representation (`serializer.data` with no `fields`/`expand`, no viewer
/// — relations omitted, blocker keys present).
pub async fn render_write_response(
    pool: &PgPool,
    decoded: &DecodedIssue,
    tz: &Tz,
    web_base: Option<&str>,
) -> Result<Value, Error> {
    let kept = filter_fields(shape::FIELDS_IN_ORDER, None)
        .map_err(|_| Error::Failed("render-write-response"))?;
    let expand_refs: Vec<&str> = Vec::new();
    render_decoded(
        pool,
        decoded,
        tz,
        &RenderRequest {
            field_specs: None,
            kept: &kept,
            expand: &expand_refs,
            is_list: false,
            viewer: None,
            web_base,
        },
    )
    .await
}

// ---------------------------------------------------------------------------
// Comment rows (moved from `pidash_api::v1_work_items::handlers_social`
// by OVL-14, PIDASHCONV-1649).
// ---------------------------------------------------------------------------

/// Comment patch/delete direct lookup (`:2040`, `:2109`): live + slug +
/// project + issue + pk. (Moved from the v1 work-items handlers by
/// OVL-14, PIDASHCONV-1649.)
pub async fn fetch_comment_direct(
    pool: &PgPool,
    slug: &str,
    project_id: &Uuid,
    issue_id: &Uuid,
    pk: &Uuid,
) -> Result<Option<sqlx::postgres::PgRow>, Error> {
    let row = sqlx::query(
        r#"SELECT c.* FROM "issue_comments" c
           WHERE c."deleted_at" IS NULL
             AND c."workspace_id" = (SELECT "id" FROM "workspaces" WHERE "slug" = $1)
             AND c."project_id" = $2 AND c."issue_id" = $3 AND c."id" = $4"#,
    )
    .bind(slug)
    .bind(project_id)
    .bind(issue_id)
    .bind(pk)
    .fetch_optional(pool)
    .await
    .map_err(|error| DbError::new("comment-direct", error))?;
    Ok(row)
}

/// One decoded comment row: owned strings the [`CommentRow`] borrows.
pub struct DecodedComment {
    pub id: String,
    pub is_member: Option<bool>,
    pub created_at: String,
    pub updated_at: String,
    pub deleted_at: Option<String>,
    pub comment_json: Value,
    pub comment_html: String,
    pub attachments: Vec<Option<String>>,
    pub labels: Vec<Option<String>>,
    pub access: String,
    pub external_source: Option<String>,
    pub external_id: Option<String>,
    pub speaker_type: String,
    pub speaker_label: String,
    pub speaker_agent_run_id: Option<String>,
    pub edited_at: Option<String>,
    pub created_by: Option<String>,
    pub updated_by: Option<String>,
    pub project: String,
    pub workspace: String,
    pub description: Option<String>,
    pub issue: String,
    pub actor: Option<String>,
    pub parent: Option<String>,
}

pub fn decode_comment(row: &sqlx::postgres::PgRow, tz: &Tz) -> Result<DecodedComment, Error> {
    let attachments: Vec<Option<String>> = row
        .try_get("attachments")
        .map_err(|_| Error::Failed("row-attachments"))?;
    let labels: Vec<Option<String>> = row
        .try_get("labels")
        .map_err(|_| Error::Failed("row-labels"))?;
    let comment_json: Value = row
        .try_get("comment_json")
        .map_err(|_| Error::Failed("row-comment-json"))?;
    let is_member: Option<bool> = row.try_get("is_member").unwrap_or(None);
    Ok(DecodedComment {
        id: row_uuid(row, "id")?.to_string(),
        is_member,
        created_at: super::render_datetime_in(&row_datetime(row, "created_at")?, tz),
        updated_at: super::render_datetime_in(&row_datetime(row, "updated_at")?, tz),
        deleted_at: row_datetime_opt(row, "deleted_at")?
            .map(|dt| super::render_datetime_in(&dt, tz)),
        comment_json,
        comment_html: row_string(row, "comment_html")?,
        attachments,
        labels,
        access: row_string(row, "access")?,
        external_source: row_string_opt(row, "external_source")?,
        external_id: row_string_opt(row, "external_id")?,
        speaker_type: row_string(row, "speaker_type")?,
        speaker_label: row_string(row, "speaker_label")?,
        speaker_agent_run_id: row_uuid_opt(row, "speaker_agent_run_id")?.map(|id| id.to_string()),
        edited_at: row_datetime_opt(row, "edited_at")?.map(|dt| super::render_datetime_in(&dt, tz)),
        created_by: row_uuid_opt(row, "created_by_id")?.map(|id| id.to_string()),
        updated_by: row_uuid_opt(row, "updated_by_id")?.map(|id| id.to_string()),
        project: row_uuid(row, "project_id")?.to_string(),
        workspace: row_uuid(row, "workspace_id")?.to_string(),
        description: row_uuid_opt(row, "description_id")?.map(|id| id.to_string()),
        issue: row_uuid(row, "issue_id")?.to_string(),
        actor: row_uuid_opt(row, "actor_id")?.map(|id| id.to_string()),
        parent: row_uuid_opt(row, "parent_id")?.map(|id| id.to_string()),
    })
}

#[cfg(test)]
mod live {
    use chrono_tz::Tz;
    use uuid::Uuid;

    use super::*;
    use crate::read::test_support::{pool, seed_world};

    #[tokio::test]
    async fn decode_and_write_render_roundtrip() {
        let Some(db) = pool().await else { return };
        let world = seed_world(&db).await;
        let tz: Tz = chrono_tz::UTC;
        let decoded = fetch_issue_for_write(
            &db,
            &world.workspace_slug,
            &world.project_id,
            &world.issue_id,
            &tz,
        )
        .await
        .expect("fetches");
        assert_eq!(decoded.id, world.issue_id.to_string());
        assert_eq!(decoded.name, "Seed issue");
        assert_eq!(decoded.sequence_id, 101);
        assert_eq!(decoded.project_identifier, "SEED");
        assert!(decoded.created_at.ends_with('Z'));
        let rendered = render_write_response(&db, &decoded, &tz, None)
            .await
            .expect("renders");
        assert_eq!(rendered["id"], Value::String(world.issue_id.to_string()));
        assert_eq!(rendered["name"], Value::String("Seed issue".to_owned()));
        let err = fetch_issue_for_write(
            &db,
            &world.workspace_slug,
            &world.project_id,
            &Uuid::new_v4(),
            &tz,
        )
        .await
        .expect_err("miss");
        assert!(matches!(err, Error::NotFound));
    }

    #[tokio::test]
    async fn render_decoded_list_smoke() {
        let Some(db) = pool().await else { return };
        let world = seed_world(&db).await;
        let tz: Tz = chrono_tz::UTC;
        let decoded = fetch_issue_for_write(
            &db,
            &world.workspace_slug,
            &world.project_id,
            &world.issue_id,
            &tz,
        )
        .await
        .expect("fetches");
        let kept: Vec<String> = shape::FIELDS_IN_ORDER
            .iter()
            .map(|name| (*name).to_owned())
            .collect();
        let rendered = render_decoded(
            &db,
            &decoded,
            &tz,
            &RenderRequest {
                field_specs: None,
                kept: &kept,
                expand: &["project"],
                is_list: true,
                viewer: None,
                web_base: None,
            },
        )
        .await
        .expect("renders");
        assert_eq!(
            rendered["sequence_id"],
            Value::Number(world.sequence_id.into())
        );
        assert_eq!(
            rendered["project"]["identifier"],
            Value::String("SEED".to_owned())
        );
    }
}
