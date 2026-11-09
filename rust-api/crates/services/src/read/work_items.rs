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

use pidash_db::entity::{
    estimate_points, file_assets, issue_assignees, issue_comments, issue_labels, issue_relations,
    issues, project_members, projects, states, users, workspaces,
};
use sea_orm::sea_query::{Alias, Expr, Func, SimpleExpr};
use sea_orm::{
    ColumnTrait, Condition, ConnectionTrait, DbBackend, EntityTrait, FromQueryResult, JoinType,
    QueryFilter, QueryOrder, QuerySelect, RelationTrait,
};

use super::{DbError, Error};
use crate::v1_work_items::shape_issue as shape;
use crate::v1_work_items::{filter_fields, FieldSpec};

/// The `issues.*` columns every read path selects (F18-05 column order).
/// `description_json` / `description_stripped` / `workpad` are unrendered
/// and unread here, so they stay out of the select list.
pub const ISSUE_SELECT_COLS: &str = r#""issues"."id", "issues"."created_at", "issues"."updated_at", "issues"."deleted_at", "issues"."point", "issues"."name", "issues"."description_html", "issues"."description_binary", "issues"."priority", "issues"."complexity_score", "issues"."start_date", "issues"."target_date", "issues"."sequence_id", "issues"."sort_order", "issues"."completed_at", "issues"."archived_at", "issues"."is_draft", "issues"."external_source", "issues"."external_id", "issues"."git_work_branch", "issues"."created_via", "issues"."agent_executor", "issues"."created_by_id", "issues"."updated_by_id", "issues"."project_id", "issues"."workspace_id", "issues"."parent_id", "issues"."state_id", "issues"."estimate_point_id", "issues"."type_id", "issues"."assigned_pod_id", "issues"."assigned_runner_id""#;

/// The joined extras every read path selects: the URL parts and the state
/// group (the completed-arm check on writes).
pub const ISSUE_EXTRA_COLS: &str = r#""projects"."identifier" AS "project_identifier", "workspaces"."slug" AS "workspace_slug", "states"."group" AS "state_group""#;

/// `assigned_runner_detail` by runner id (`issue_assignment.py`,
/// design §14.1): `None` when the issue is pod-assigned or the runner
/// is gone.
async fn fetch_assigned_runner_detail(
    pool: &PgPool,
    runner_id: Option<Uuid>,
) -> Result<Option<Value>, Error> {
    use sea_orm::EntityTrait;
    let Some(runner_id) = runner_id else {
        return Ok(None);
    };
    let orm = pidash_db::pool::sea_orm_connection(pool);
    let Some(runner) = pidash_db::entity::runner::Entity::find_by_id(runner_id)
        .one(&orm)
        .await
        .map_err(|_| Error::Failed("render-decoded"))?
    else {
        return Ok(None);
    };
    crate::domain::issue_assignment::assigned_runner_detail(&orm, &runner)
        .await
        .map(Some)
        .map_err(|_| Error::Failed("render-decoded"))
}

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
    pub assigned_runner: Option<String>,
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
#[allow(dead_code)]
type BlockerTargetRow = (Uuid, String, i32, Option<String>, Option<String>);

/// Sort key per `_summary_list`: resolved flag, identifier, sequence,
/// then the rendered triple.
type SummarySortKey = (i64, String, i64, Option<String>, Option<String>);

/// Decoded `file_assets` row for [`file_asset_url`]: entity type plus the
/// nullable scope ids.
type AssetUrlLookup = (Option<String>, Option<Uuid>, Option<Uuid>, Option<Uuid>);

/// Decoded user row for [`expand_user`]: names, nullable `email`,
/// `COALESCE`d avatar, optional avatar asset, display name.
#[allow(dead_code)]
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
    let db = pidash_db::pool::sea_orm_connection(pool);
    let row: Option<AssetUrlLookup> = file_assets::Entity::find_by_id(*asset_id)
        .select_only()
        .columns([
            file_assets::Column::EntityType,
            file_assets::Column::WorkspaceId,
            file_assets::Column::ProjectId,
            file_assets::Column::IssueId,
        ])
        .into_tuple::<AssetUrlLookup>()
        .one(&db)
        .await
        .map_err(|error| DbError::from_orm("file-asset-url", error))?;
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
        assigned_runner: row_uuid_opt(row, "assigned_runner_id")?.map(|id| id.to_string()),
        project_identifier: row_string(row, "project_identifier")?,
        workspace_slug: row_string(row, "workspace_slug")?,
        state_group: row_string_opt(row, "state_group")?,
    })
}

/// The `prefetch_related("assignees")` ids
/// (`IssueAssignee.objects.filter(issue=...)`, `-created_at` order).
pub async fn fetch_assignee_ids(pool: &PgPool, issue_id: &Uuid) -> Result<Vec<String>, Error> {
    let db = pidash_db::pool::sea_orm_connection(pool);
    let ids: Vec<Uuid> = issue_assignees::Entity::find()
        .filter(issue_assignees::Column::IssueId.eq(*issue_id))
        .filter(issue_assignees::Column::DeletedAt.is_null())
        .order_by_desc(issue_assignees::Column::CreatedAt)
        .select_only()
        .column(issue_assignees::Column::AssigneeId)
        .into_tuple::<Uuid>()
        .all(&db)
        .await
        .map_err(|error| DbError::from_orm("issue-assignees", error))?;
    Ok(ids.iter().map(ToString::to_string).collect())
}

/// The `prefetch_related("labels")` ids (`IssueLabel.objects.filter(issue=...)`,
/// `-created_at` order).
pub async fn fetch_label_ids(pool: &PgPool, issue_id: &Uuid) -> Result<Vec<String>, Error> {
    let db = pidash_db::pool::sea_orm_connection(pool);
    let ids: Vec<Uuid> = issue_labels::Entity::find()
        .filter(issue_labels::Column::IssueId.eq(*issue_id))
        .filter(issue_labels::Column::DeletedAt.is_null())
        .order_by_desc(issue_labels::Column::CreatedAt)
        .select_only()
        .column(issue_labels::Column::LabelId)
        .into_tuple::<Uuid>()
        .all(&db)
        .await
        .map_err(|error| DbError::from_orm("issue-labels", error))?;
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

/// The `has_open_blockers` arm of [`fetch_blocker_rows`]: a single
/// `SELECT 1 ... LIMIT 1` presence check over the open-blocker set
/// (orm-migration §3.3), kept as a builder so statement-shape tests
/// pin the join, scope and open-group predicates without a database.
/// The `CASE`-selected other end stays in the `ON` clause (`CASE WHEN
/// relation_type = 'blocked_by' THEN related_issue_id ELSE issue_id
/// END`) — the former raw shape, one statement, same row set.
fn has_open_blockers_statement(issue_id: &Uuid) -> sea_orm::sea_query::SelectStatement {
    use sea_orm::sea_query::{Alias, Cond, Expr, Query as SeaQuery};
    let r = Alias::new("r");
    let t = Alias::new("t");
    let s = Alias::new("s");
    let p = Alias::new("p");
    let mut select = SeaQuery::select();
    select
        .expr_as(Expr::val(1), Alias::new("a"))
        .from_as(issue_relations::Entity, r.clone())
        .join_as(
            JoinType::InnerJoin,
            issues::Entity,
            t.clone(),
            Expr::col((t.clone(), issues::Column::Id)).eq(Expr::case(
                Expr::col((r.clone(), issue_relations::Column::RelationType)).eq("blocked_by"),
                Expr::col((r.clone(), issue_relations::Column::RelatedIssueId)),
            )
            .finally(Expr::col((r.clone(), issue_relations::Column::IssueId)))),
        )
        .join_as(
            JoinType::InnerJoin,
            states::Entity,
            s.clone(),
            Expr::col((t.clone(), issues::Column::StateId)).equals((s.clone(), states::Column::Id)),
        )
        .join_as(
            JoinType::InnerJoin,
            projects::Entity,
            p.clone(),
            Expr::col((t.clone(), issues::Column::ProjectId))
                .equals((p.clone(), projects::Column::Id)),
        )
        .cond_where(
            Cond::all()
                .add(Expr::col((r.clone(), issue_relations::Column::DeletedAt)).is_null())
                .add(
                    Expr::col((r.clone(), issue_relations::Column::IssueId)).ne(Expr::col((
                        r.clone(),
                        issue_relations::Column::RelatedIssueId,
                    ))),
                )
                .add(Expr::col((t.clone(), issues::Column::DeletedAt)).is_null())
                .add(Expr::col((t.clone(), issues::Column::ArchivedAt)).is_null())
                .add(Expr::col((t.clone(), issues::Column::IsDraft)).eq(false))
                .add(
                    Expr::col((t.clone(), issues::Column::WorkspaceId))
                        .eq(Expr::col((r.clone(), issue_relations::Column::WorkspaceId))),
                )
                .add(Expr::col((s.clone(), states::Column::Group)).ne("triage"))
                .add(Expr::col((p.clone(), projects::Column::ArchivedAt)).is_null())
                .add(
                    Cond::any()
                        .add(
                            Cond::all()
                                .add(
                                    Expr::col((r.clone(), issue_relations::Column::IssueId))
                                        .eq(*issue_id),
                                )
                                .add(
                                    Expr::col((r.clone(), issue_relations::Column::RelationType))
                                        .eq("blocked_by"),
                                ),
                        )
                        .add(
                            Cond::all()
                                .add(
                                    Expr::col((r.clone(), issue_relations::Column::RelatedIssueId))
                                        .eq(*issue_id),
                                )
                                .add(
                                    Expr::col((r.clone(), issue_relations::Column::RelationType))
                                        .eq("blocking"),
                                ),
                        ),
                )
                .add(
                    Expr::col((s.clone(), states::Column::Group))
                        .is_in(["completed", "cancelled"])
                        .not(),
                ),
        )
        .limit(1);
    select
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
        use sea_orm::sea_query::Expr;
        let db = pidash_db::pool::sea_orm_connection(pool);
        for relation_type in ["blocked_by", "blocking"] {
            // Map the (blocked_by, relation_type) pair to (anchor, target)
            // columns exactly like the format!-built SQL did.
            let (anchor_eq_issue, target_is_related) = if blocked_by {
                match relation_type {
                    "blocked_by" => (true, true),
                    _ => (false, false),
                }
            } else {
                match relation_type {
                    "blocked_by" => (false, false),
                    _ => (true, true),
                }
            };
            #[derive(Debug, Clone, FromQueryResult)]
            struct DirRow {
                id: Uuid,
                identifier: String,
                sequence_id: i32,
                state: Option<String>,
                group: Option<String>,
            }
            let anchor = if anchor_eq_issue {
                issue_relations::Column::IssueId
            } else {
                issue_relations::Column::RelatedIssueId
            };
            let to_t = if target_is_related {
                issue_relations::Relation::Issues1.def()
            } else {
                issue_relations::Relation::Issues2.def()
            };
            let rows: Vec<DirRow> = issue_relations::Entity::find()
                .filter(issue_relations::Column::DeletedAt.is_null())
                .filter(
                    Expr::col((issue_relations::Entity, issue_relations::Column::IssueId)).ne(
                        Expr::col((
                            issue_relations::Entity,
                            issue_relations::Column::RelatedIssueId,
                        )),
                    ),
                )
                .filter(issue_relations::Column::RelationType.eq(relation_type))
                .filter(Expr::col((issue_relations::Entity, anchor)).eq(*issue_id))
                .filter(Expr::col((issues::Entity, issues::Column::DeletedAt)).is_null())
                .filter(Expr::col((issues::Entity, issues::Column::ArchivedAt)).is_null())
                .filter(Expr::col((issues::Entity, issues::Column::IsDraft)).eq(false))
                .filter(
                    Expr::col((issues::Entity, issues::Column::WorkspaceId)).eq(Expr::col((
                        issue_relations::Entity,
                        issue_relations::Column::WorkspaceId,
                    ))),
                )
                .filter(Expr::col((states::Entity, states::Column::Group)).ne("triage"))
                .filter(Expr::col((projects::Entity, projects::Column::ArchivedAt)).is_null())
                .join(JoinType::InnerJoin, to_t)
                .join(JoinType::InnerJoin, issues::Relation::States.def())
                .join(JoinType::InnerJoin, issues::Relation::Projects.def())
                .select_only()
                .column_as(issues::Column::Id, "id")
                .column_as(projects::Column::Identifier, "identifier")
                .column_as(issues::Column::SequenceId, "sequence_id")
                .column_as(states::Column::Name, "state")
                .column_as(states::Column::Group, "group")
                .into_model::<DirRow>()
                .all(&db)
                .await
                .map_err(|error| DbError::from_orm("expand-blockers", error))?;
            for row in rows {
                if seen.insert(row.id) {
                    let resolved = i64::from(matches!(
                        row.group.as_deref(),
                        Some("completed" | "cancelled")
                    ));
                    items.push((
                        resolved,
                        row.identifier,
                        i64::from(row.sequence_id),
                        row.state,
                        row.group,
                    ));
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
    let db = pidash_db::pool::sea_orm_connection(pool);
    let has_open_blockers = db
        .query_one(DbBackend::Postgres.build(&has_open_blockers_statement(issue_id)))
        .await
        .map_err(|error| DbError::from_orm("expand-open-blockers", error))?
        .is_some();
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
    let db = pidash_db::pool::sea_orm_connection(pool);
    let edges: Vec<(Uuid, Uuid, String)> = issue_relations::Entity::find()
        .filter(
            Condition::any()
                .add(issue_relations::Column::IssueId.eq(*issue_id))
                .add(issue_relations::Column::RelatedIssueId.eq(*issue_id)),
        )
        .filter(issue_relations::Column::WorkspaceId.eq(*workspace_id))
        .filter(issue_relations::Column::DeletedAt.is_null())
        .filter(
            sea_orm::sea_query::Expr::col(issue_relations::Column::IssueId)
                .ne(*issue_id)
                .or(
                    sea_orm::sea_query::Expr::col(issue_relations::Column::RelatedIssueId)
                        .ne(*issue_id),
                ),
        )
        .select_only()
        .columns([
            issue_relations::Column::IssueId,
            issue_relations::Column::RelatedIssueId,
            issue_relations::Column::RelationType,
        ])
        .into_tuple::<(Uuid, Uuid, String)>()
        .all(&db)
        .await
        .map_err(|error| DbError::from_orm("relations-edges", error))?;
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
        #[derive(Debug, Clone, FromQueryResult)]
        struct GroupedPoolRow {
            id: Uuid,
            name: String,
            sequence_id: i32,
            state: Option<String>,
            state_group: Option<String>,
            project_identifier: String,
        }
        let rows: Vec<GroupedPoolRow> = relations_pool_select(&wanted, workspace_id, viewer_id)
            .into_model::<GroupedPoolRow>()
            .all(&db)
            .await
            .map_err(|error| DbError::from_orm("relations-pool", error))?;
        for row in rows {
            let id = row.id;
            others.insert(
                id,
                OwnedRelationItem {
                    id: id.to_string(),
                    name: row.name,
                    state: row.state,
                    state_group: row.state_group,
                    project_identifier: row.project_identifier,
                    sequence_id: i64::from(row.sequence_id),
                },
            );
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
        return Ok(groups);
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
    let db = pidash_db::pool::sea_orm_connection(pool);
    let row: Option<(String, String, String)> = states::Entity::find_by_id(*state_id)
        .filter(states::Column::DeletedAt.is_null())
        .select_only()
        .columns([
            states::Column::Name,
            states::Column::Color,
            states::Column::Group,
        ])
        .into_tuple::<(String, String, String)>()
        .one(&db)
        .await
        .map_err(|error| DbError::from_orm("expand-state", error))?;
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
    let db = pidash_db::pool::sea_orm_connection(pool);
    let model = projects::Entity::find_by_id(*project_id)
        .one(&db)
        .await
        .map_err(|error| DbError::from_orm("expand-project", error))?;
    let Some(model) = model else {
        return Ok(None);
    };
    let cover_asset = model.cover_image_asset_id;
    let cover_text = model.cover_image.clone();
    // `Project.cover_image_url` (`db/models/project.py:176-185`): the
    // asset URL wins, then the stored text, else null.
    let cover_url = match cover_asset {
        Some(id) => file_asset_url(pool, &id).await?,
        None => None,
    };
    let cover_url = cover_url.or_else(|| cover_text.clone().filter(|text| !text.is_empty()));
    let mut map = Map::with_capacity(9);
    map.insert("id".to_owned(), Value::String(model.id.to_string()));
    map.insert("identifier".to_owned(), Value::String(model.identifier));
    map.insert("name".to_owned(), Value::String(model.name));
    map.insert(
        "cover_image".to_owned(),
        cover_text.map(Value::String).unwrap_or(Value::Null),
    );
    map.insert(
        "icon_prop".to_owned(),
        model.icon_prop.unwrap_or(Value::Null),
    );
    map.insert(
        "emoji".to_owned(),
        model.emoji.map(Value::String).unwrap_or(Value::Null),
    );
    map.insert("description".to_owned(), Value::String(model.description));
    map.insert("is_default".to_owned(), Value::Bool(model.is_default));
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
    let db = pidash_db::pool::sea_orm_connection(pool);
    let row: Option<(String, String)> = workspaces::Entity::find_by_id(*workspace_id)
        .select_only()
        .columns([workspaces::Column::Name, workspaces::Column::Slug])
        .into_tuple::<(String, String)>()
        .one(&db)
        .await
        .map_err(|error| DbError::from_orm("expand-workspace", error))?;
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
    let db = pidash_db::pool::sea_orm_connection(pool);
    let model = users::Entity::find_by_id(*user_id)
        .one(&db)
        .await
        .map_err(|error| DbError::from_orm("expand-user", error))?;
    let Some(model) = model else {
        return Ok(None);
    };
    let first_name = model.first_name;
    let last_name = model.last_name;
    let email = model.email;
    let avatar = model.avatar;
    let avatar_asset = model.avatar_asset_id;
    let asset_url = match avatar_asset {
        Some(id) => file_asset_url(pool, &id).await?,
        None => None,
    };
    let avatar_url =
        collab::resolve_avatar_url(avatar_asset.is_some(), asset_url.as_deref(), &avatar);
    let id = user_id.to_string();
    let display_name = model.display_name;
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
    let db = pidash_db::pool::sea_orm_connection(pool);
    let row: Option<(Uuid, i32, Uuid)> = issues::Entity::find_by_id(*parent_id)
        .select_only()
        .columns([
            issues::Column::Id,
            issues::Column::SequenceId,
            issues::Column::ProjectId,
        ])
        .into_tuple::<(Uuid, i32, Uuid)>()
        .one(&db)
        .await
        .map_err(|error| DbError::from_orm("expand-parent", error))?;
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
    let db = pidash_db::pool::sea_orm_connection(pool);
    let model = estimate_points::Entity::find_by_id(*estimate_point_id)
        .one(&db)
        .await
        .map_err(|error| DbError::from_orm("expand-estimate-point", error))?;
    let Some(model) = model else {
        return Ok(None);
    };
    let id = model.id.to_string();
    let created_at = super::render_datetime_in(&model.created_at, tz);
    let updated_at = super::render_datetime_in(&model.updated_at, tz);
    let created_by = model.created_by_id.map(|id| id.to_string());
    let updated_by = model.updated_by_id.map(|id| id.to_string());
    let deleted_at = model
        .deleted_at
        .map(|dt| super::render_datetime_in(&dt, tz));
    let project = model.project_id.to_string();
    let workspace = model.workspace_id.to_string();
    let estimate = model.estimate_id.to_string();
    let key = model.key;
    let description = model.description;
    let value = model.value;
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
    use sea_orm::sea_query::{Expr, Query as SeaQuery};
    let db = pidash_db::pool::sea_orm_connection(pool);
    let id_sub = SeaQuery::select()
        .column(issue_assignees::Column::AssigneeId)
        .from(issue_assignees::Entity)
        .and_where(Expr::col(issue_assignees::Column::IssueId).eq(*issue_id))
        .and_where(Expr::col(issue_assignees::Column::DeletedAt).is_null())
        .to_owned();
    let models = users::Entity::find()
        .filter(Expr::col(users::Column::Id).in_subquery(id_sub))
        .order_by_desc(users::Column::CreatedAt)
        .all(&db)
        .await
        .map_err(|error| DbError::from_orm("expand-assignees", error))?;
    let rows: Vec<(
        Uuid,
        String,
        String,
        Option<String>,
        String,
        Option<Uuid>,
        Option<String>,
    )> = models
        .into_iter()
        .map(|m| {
            (
                m.id,
                m.first_name,
                m.last_name,
                m.email,
                m.avatar,
                m.avatar_asset_id,
                Some(m.display_name),
            )
        })
        .collect();
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
/// `Label.objects` is the soft-deletion manager (`deleted_at IS NULL`),
/// `-created_at` order (`db/models/label.py:44`).
pub async fn fetch_expanded_labels(
    pool: &PgPool,
    issue_id: &Uuid,
    tz: &Tz,
) -> Result<Vec<Value>, Error> {
    use crate::v1_work_items::shape_labels as labels;
    use sea_orm::sea_query::{Expr, Query as SeaQuery};
    let db = pidash_db::pool::sea_orm_connection(pool);
    let id_sub = SeaQuery::select()
        .column(issue_labels::Column::LabelId)
        .from(issue_labels::Entity)
        .and_where(Expr::col(issue_labels::Column::IssueId).eq(*issue_id))
        .and_where(Expr::col(issue_labels::Column::DeletedAt).is_null())
        .to_owned();
    let models = pidash_db::entity::labels::Entity::find()
        .filter(Expr::col(pidash_db::entity::labels::Column::Id).in_subquery(id_sub))
        .filter(pidash_db::entity::labels::Column::DeletedAt.is_null())
        .order_by_desc(pidash_db::entity::labels::Column::CreatedAt)
        .all(&db)
        .await
        .map_err(|error| DbError::from_orm("expand-labels", error))?;
    let mut out = Vec::with_capacity(models.len());
    for model in &models {
        let id = model.id.to_string();
        let created_at = super::render_datetime_in(&model.created_at, tz);
        let updated_at = super::render_datetime_in(&model.updated_at, tz);
        let deleted_at = model
            .deleted_at
            .map(|dt| super::render_datetime_in(&dt, tz));
        let name = model.name.clone();
        let description = model.description.clone();
        let color = model.color.clone();
        let sort_order = model.sort_order;
        let external_source = model.external_source.clone();
        let external_id = model.external_id.clone();
        let created_by = model.created_by_id.map(|id| id.to_string());
        let updated_by = model.updated_by_id.map(|id| id.to_string());
        let workspace = model.workspace_id.to_string();
        let project = model.project_id.map(|id| id.to_string());
        let parent = model.parent_id.map(|id| id.to_string());
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
    // The dedicated-runner detail (design §14.1): fetched only when the
    // field is kept, the same value Django's serializer method field
    // renders either way.
    let assigned_runner_id = decoded
        .assigned_runner
        .as_deref()
        .map(|text| {
            text.parse::<Uuid>()
                .map_err(|_| Error::Failed("render-decoded"))
        })
        .transpose()?;
    let assigned_runner_detail = if kept("assigned_runner_detail") {
        fetch_assigned_runner_detail(pool, assigned_runner_id).await?
    } else {
        None
    };
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
        assigned_runner: decoded.assigned_runner.as_deref(),
        assigned_runner_detail: assigned_runner_detail.as_ref(),
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
    #[derive(Debug, Clone, FromQueryResult)]
    struct IssueWriteRow {
        id: Uuid,
        created_at: sea_orm::prelude::DateTimeWithTimeZone,
        updated_at: sea_orm::prelude::DateTimeWithTimeZone,
        deleted_at: Option<sea_orm::prelude::DateTimeWithTimeZone>,
        point: Option<i32>,
        name: String,
        description_html: String,
        description_binary: Option<Vec<u8>>,
        priority: String,
        complexity_score: i32,
        start_date: Option<chrono::NaiveDate>,
        target_date: Option<chrono::NaiveDate>,
        sequence_id: i32,
        sort_order: f64,
        completed_at: Option<sea_orm::prelude::DateTimeWithTimeZone>,
        archived_at: Option<chrono::NaiveDate>,
        is_draft: bool,
        external_source: Option<String>,
        external_id: Option<String>,
        git_work_branch: String,
        created_via: Option<String>,
        agent_executor: Option<String>,
        created_by_id: Option<Uuid>,
        updated_by_id: Option<Uuid>,
        project_id: Uuid,
        workspace_id: Uuid,
        parent_id: Option<Uuid>,
        state_id: Option<Uuid>,
        estimate_point_id: Option<Uuid>,
        type_id: Option<Uuid>,
        assigned_pod_id: Option<Uuid>,
        assigned_runner_id: Option<Uuid>,
        project_identifier: String,
        workspace_slug: String,
        state_group: Option<String>,
    }
    let db = pidash_db::pool::sea_orm_connection(pool);
    let row: Option<IssueWriteRow> = issues::Entity::find()
        .filter(issues::Column::DeletedAt.is_null())
        .filter(issues::Column::Id.eq(*pk))
        .filter(issues::Column::ProjectId.eq(*project_id))
        .filter(workspaces::Column::Slug.eq(slug.to_owned()))
        .join(JoinType::LeftJoin, issues::Relation::States.def())
        .join(JoinType::InnerJoin, issues::Relation::Projects.def())
        .join(JoinType::InnerJoin, issues::Relation::Workspaces.def())
        .select_only()
        .columns([
            issues::Column::Id,
            issues::Column::CreatedAt,
            issues::Column::UpdatedAt,
            issues::Column::DeletedAt,
            issues::Column::Point,
            issues::Column::Name,
            issues::Column::DescriptionHtml,
            issues::Column::DescriptionBinary,
            issues::Column::Priority,
            issues::Column::ComplexityScore,
            issues::Column::StartDate,
            issues::Column::TargetDate,
            issues::Column::SequenceId,
            issues::Column::SortOrder,
            issues::Column::CompletedAt,
            issues::Column::ArchivedAt,
            issues::Column::IsDraft,
            issues::Column::ExternalSource,
            issues::Column::ExternalId,
            issues::Column::GitWorkBranch,
            issues::Column::CreatedVia,
            issues::Column::AgentExecutor,
            issues::Column::CreatedById,
            issues::Column::UpdatedById,
            issues::Column::ProjectId,
            issues::Column::WorkspaceId,
            issues::Column::ParentId,
            issues::Column::StateId,
            issues::Column::EstimatePointId,
            issues::Column::TypeId,
            issues::Column::AssignedPodId,
            issues::Column::AssignedRunnerId,
        ])
        .column_as(projects::Column::Identifier, "project_identifier")
        .column_as(workspaces::Column::Slug, "workspace_slug")
        .column_as(states::Column::Group, "state_group")
        .limit(1)
        .into_model::<IssueWriteRow>()
        .one(&db)
        .await
        .map_err(|error| DbError::from_orm("write-fetch", error))?;
    let Some(row) = row else {
        return Err(Error::NotFound);
    };
    Ok(DecodedIssue {
        id: row.id.to_string(),
        created_at: super::render_datetime_in(&row.created_at, tz),
        created_at_dt: row.created_at.with_timezone(&Utc),
        updated_at: super::render_datetime_in(&row.updated_at, tz),
        created_by: row.created_by_id.map(|id| id.to_string()),
        updated_by: row.updated_by_id.map(|id| id.to_string()),
        deleted_at: row.deleted_at.map(|dt| super::render_datetime_in(&dt, tz)),
        point: row.point.map(i64::from),
        name: row.name,
        description_html: row.description_html,
        description_binary: row.description_binary,
        priority: row.priority,
        complexity_score: i64::from(row.complexity_score),
        start_date: row.start_date.map(|d| d.to_string()),
        target_date: row.target_date.map(|d| d.to_string()),
        sequence_id: i64::from(row.sequence_id),
        sort_order: row.sort_order,
        completed_at: row
            .completed_at
            .map(|dt| super::render_datetime_in(&dt, tz)),
        archived_at: row.archived_at.map(|d| d.to_string()),
        is_draft: row.is_draft,
        external_source: row.external_source,
        external_id: row.external_id,
        git_work_branch: row.git_work_branch,
        created_via: row.created_via,
        agent_executor: row.agent_executor,
        project: row.project_id.to_string(),
        workspace: row.workspace_id.to_string(),
        parent: row.parent_id.map(|id| id.to_string()),
        state: row.state_id.map(|id| id.to_string()),
        estimate_point: row.estimate_point_id.map(|id| id.to_string()),
        type_id: row.type_id.map(|id| id.to_string()),
        assigned_pod: row.assigned_pod_id.map(|id| id.to_string()),
        assigned_runner: row.assigned_runner_id.map(|id| id.to_string()),
        project_identifier: row.project_identifier,
        workspace_slug: row.workspace_slug,
        state_group: row.state_group,
    })
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
    let db = pidash_db::pool::sea_orm_connection(pool);
    let row: Option<(Uuid, Option<Uuid>)> = projects::Entity::find_by_id(*project_id)
        .filter(projects::Column::DeletedAt.is_null())
        .select_only()
        .columns([
            projects::Column::WorkspaceId,
            projects::Column::DefaultAssigneeId,
        ])
        .into_tuple::<(Uuid, Option<Uuid>)>()
        .one(&db)
        .await
        .map_err(|error| DbError::from_orm("write-project", error))?;
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

/// One `issue_comments` row: every column the comment renderers and
/// the patch/delete ops read, decoded by name, plus the list's
/// `is_member` annotation (`None` on the un-annotated direct
/// lookups). Datetimes keep the `DateTime<Utc>` instant the old
/// `PgRow` read produced. The array columns arrive as JSON arrays
/// ([`array_to_json`], so nullable elements survive as `null` —
/// sea-orm's `Vec<String>` decode errors on them); they decode to
/// `Vec<Option<String>>` in [`decode_comment`].
#[derive(Debug, Clone, PartialEq, FromQueryResult)]
pub struct CommentDbRow {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
    pub comment_stripped: String,
    pub attachments: Value,
    pub created_by_id: Option<Uuid>,
    pub issue_id: Uuid,
    pub project_id: Uuid,
    pub updated_by_id: Option<Uuid>,
    pub workspace_id: Uuid,
    pub actor_id: Option<Uuid>,
    pub comment_html: String,
    pub comment_json: Value,
    pub access: String,
    pub external_id: Option<String>,
    pub external_source: Option<String>,
    pub edited_at: Option<DateTime<Utc>>,
    pub description_id: Option<Uuid>,
    pub parent_id: Option<Uuid>,
    pub speaker_type: String,
    pub speaker_label: String,
    pub speaker_agent_run_id: Option<Uuid>,
    pub labels: Value,
    pub is_member: Option<bool>,
}

/// The `issue_comments` columns, in entity order: every non-array
/// column the renderers or the patch/delete ops read. The array
/// columns (`attachments`, `labels`) are picked as JSON alongside
/// (see [`comment_columns`]).
pub const COMMENT_COLUMNS: [issue_comments::Column; 22] = [
    issue_comments::Column::Id,
    issue_comments::Column::CreatedAt,
    issue_comments::Column::UpdatedAt,
    issue_comments::Column::DeletedAt,
    issue_comments::Column::CommentStripped,
    issue_comments::Column::CreatedById,
    issue_comments::Column::IssueId,
    issue_comments::Column::ProjectId,
    issue_comments::Column::UpdatedById,
    issue_comments::Column::WorkspaceId,
    issue_comments::Column::ActorId,
    issue_comments::Column::CommentHtml,
    issue_comments::Column::CommentJson,
    issue_comments::Column::Access,
    issue_comments::Column::ExternalId,
    issue_comments::Column::ExternalSource,
    issue_comments::Column::EditedAt,
    issue_comments::Column::DescriptionId,
    issue_comments::Column::ParentId,
    issue_comments::Column::SpeakerType,
    issue_comments::Column::SpeakerLabel,
    issue_comments::Column::SpeakerAgentRunId,
];

/// The comment columns with the array columns as JSON: `SELECT`
/// picks `array_to_json(attachments)` / `array_to_json(labels)` (the
/// `Func::cust` precedent in `app_views_search::queries_search` —
/// `PgFunc` has no array-to-JSON form) so nullable elements decode
/// as `null` instead of erroring, and every other column by name.
fn comment_columns(
    select: sea_orm::Select<issue_comments::Entity>,
) -> sea_orm::Select<issue_comments::Entity> {
    select
        .columns(COMMENT_COLUMNS)
        .column_as(
            SimpleExpr::from(Func::cust("array_to_json").arg(Expr::col((
                issue_comments::Entity,
                issue_comments::Column::Attachments,
            ))))
            .cast_as(Alias::new("jsonb")),
            "attachments",
        )
        .column_as(
            SimpleExpr::from(Func::cust("array_to_json").arg(Expr::col((
                issue_comments::Entity,
                issue_comments::Column::Labels,
            ))))
            .cast_as(Alias::new("jsonb")),
            "labels",
        )
}

/// `SELECT "id" FROM "workspaces" WHERE "slug" = $1`: the workspace
/// the comment scope resolves through (subquery, not a join — the
/// old SQL binds the slug as the subquery predicate).
fn workspace_id_subquery(slug: &str) -> SimpleExpr {
    use sea_orm::sea_query::Query as SeaQuery;
    SimpleExpr::SubQuery(
        None,
        Box::new(
            SeaQuery::select()
                .column(workspaces::Column::Id)
                .from(workspaces::Entity)
                .and_where(workspaces::Column::Slug.eq(slug))
                .take()
                .into_sub_query_statement(),
        ),
    )
}

/// The member-project pool behind [`fetch_grouped_rows`]: live, non-draft,
/// non-triage issues among `wanted`, in unarchived projects the viewer is an
/// active member of.
fn relations_pool_select(
    wanted: &[Uuid],
    workspace_id: &Uuid,
    viewer_id: &Uuid,
) -> sea_orm::Select<issues::Entity> {
    // Every column is table-qualified: issues, states and projects are
    // joined and share `deleted_at`/`archived_at`/`id`, so a bare column is
    // 42702 ambiguous on Postgres. Membership is the shared correlated
    // `member_exists` (Django's join span: no `deleted_at` scope).
    issues::Entity::find()
        .filter(issues::Column::Id.is_in(wanted.to_vec()))
        .filter(issues::Column::WorkspaceId.eq(*workspace_id))
        .filter(issues::Column::DeletedAt.is_null())
        .filter(issues::Column::ArchivedAt.is_null())
        .filter(issues::Column::IsDraft.eq(false))
        .filter(
            Expr::col((states::Entity, states::Column::Group))
                .is_null()
                .or(Expr::col((states::Entity, states::Column::Group)).ne("triage")),
        )
        .filter(Expr::col((projects::Entity, projects::Column::ArchivedAt)).is_null())
        .filter(member_exists(
            (issues::Entity, issues::Column::ProjectId),
            viewer_id,
        ))
        .join(JoinType::LeftJoin, issues::Relation::States.def())
        .join(JoinType::InnerJoin, issues::Relation::Projects.def())
        .select_only()
        .columns([
            issues::Column::Id,
            issues::Column::Name,
            issues::Column::SequenceId,
        ])
        .column_as(states::Column::Name, "state")
        .column_as(states::Column::Group, "state_group")
        .column_as(projects::Column::Identifier, "project_identifier")
}

/// `EXISTS (SELECT 1 FROM "project_members" pm WHERE pm."project_id" =
/// <project_col> AND pm."member_id" = $user AND pm."is_active")`: the
/// caller's active membership on the row's project, no `deleted_at`
/// scope on the join span (fixture-verbatim).
fn member_exists(project_col: (impl EntityTrait, impl ColumnTrait), user_id: &Uuid) -> SimpleExpr {
    use sea_orm::sea_query::Query as SeaQuery;
    let pm = || Alias::new("pm");
    Expr::exists(
        SeaQuery::select()
            .expr(Expr::val(1))
            .from_as(project_members::Entity, pm())
            .and_where(Expr::col((pm(), project_members::Column::ProjectId)).equals(project_col))
            .and_where(Expr::col((pm(), project_members::Column::MemberId)).eq(*user_id))
            .and_where(Expr::col((pm(), project_members::Column::IsActive)).eq(true))
            .take(),
    )
}

/// `EXISTS (SELECT 1 FROM "projects" p WHERE p."id" = <project_col> AND
/// p."archived_at" IS NULL)`: a live (unarchived) project.
fn project_unarchived_exists(project_col: (impl EntityTrait, impl ColumnTrait)) -> SimpleExpr {
    use sea_orm::sea_query::Query as SeaQuery;
    let p = || Alias::new("p");
    Expr::exists(
        SeaQuery::select()
            .expr(Expr::val(1))
            .from_as(projects::Entity, p())
            .and_where(Expr::col((p(), projects::Column::Id)).equals(project_col))
            .and_where(Expr::col((p(), projects::Column::ArchivedAt)).is_null())
            .take(),
    )
}

/// The comment direct statement (`:2040`, `:2109`): live + slug +
/// project + issue + pk, no member/archived guards and no
/// annotation (the `is_member` output is NULL, decoded to `None`).
fn comment_direct_select(
    slug: &str,
    project_id: &Uuid,
    issue_id: &Uuid,
    pk: &Uuid,
) -> sea_orm::Select<issue_comments::Entity> {
    comment_columns(issue_comments::Entity::find().select_only())
        .column_as(Expr::val(None::<bool>), "is_member")
        .filter(issue_comments::Column::DeletedAt.is_null())
        .filter(
            Expr::col((issue_comments::Entity, issue_comments::Column::WorkspaceId))
                .eq(workspace_id_subquery(slug)),
        )
        .filter(issue_comments::Column::ProjectId.eq(*project_id))
        .filter(issue_comments::Column::IssueId.eq(*issue_id))
        .filter(issue_comments::Column::Id.eq(*pk))
}

/// Comment patch/delete direct lookup (`:2040`, `:2109`): live + slug +
/// project + issue + pk. (Moved from the v1 work-items handlers by
/// OVL-14, PIDASHCONV-1649.)
pub async fn fetch_comment_direct(
    pool: &PgPool,
    slug: &str,
    project_id: &Uuid,
    issue_id: &Uuid,
    pk: &Uuid,
) -> Result<Option<CommentDbRow>, Error> {
    let db = pidash_db::pool::sea_orm_connection(pool);
    let row = comment_direct_select(slug, project_id, issue_id, pk)
        .into_model::<CommentDbRow>()
        .one(&db)
        .await
        .map_err(|error| DbError::from_orm("comment-direct", error))?;
    Ok(row)
}

/// The comment list statement (`views/issue.py:1805-1828`): the link
/// predicates plus the `is_member=Exists(...)` annotation (explicit
/// `ProjectMember.objects` filter — WITH the manager's `deleted_at IS
/// NULL`, unlike the guard) plus the unguarded member and
/// unarchived-project `EXISTS` scope guards. DISTINCT, newest first.
fn comment_list_select(
    slug: &str,
    project_id: &Uuid,
    issue_id: &Uuid,
    user_id: &Uuid,
) -> sea_orm::Select<issue_comments::Entity> {
    use sea_orm::sea_query::Query as SeaQuery;
    let pm = || Alias::new("pm");
    let is_member = Expr::exists(
        SeaQuery::select()
            .expr(Expr::val(1))
            .from_as(project_members::Entity, pm())
            .and_where(Expr::col((pm(), project_members::Column::DeletedAt)).is_null())
            .and_where(Expr::col((pm(), project_members::Column::IsActive)).eq(true))
            .and_where(Expr::col((pm(), project_members::Column::MemberId)).eq(*user_id))
            .and_where(Expr::col((pm(), project_members::Column::ProjectId)).eq(*project_id))
            .and_where(
                Expr::col((pm(), project_members::Column::WorkspaceId))
                    .eq(workspace_id_subquery(slug)),
            )
            .take(),
    );
    comment_columns(issue_comments::Entity::find().select_only())
        .column_as(is_member, "is_member")
        .distinct()
        .filter(issue_comments::Column::DeletedAt.is_null())
        .filter(
            Expr::col((issue_comments::Entity, issue_comments::Column::WorkspaceId))
                .eq(workspace_id_subquery(slug)),
        )
        .filter(issue_comments::Column::ProjectId.eq(*project_id))
        .filter(issue_comments::Column::IssueId.eq(*issue_id))
        .filter(member_exists(
            (issue_comments::Entity, issue_comments::Column::ProjectId),
            user_id,
        ))
        .filter(project_unarchived_exists((
            issue_comments::Entity,
            issue_comments::Column::ProjectId,
        )))
        .order_by_desc(issue_comments::Column::CreatedAt)
}

/// Comment list rows (`views/issue.py:1805-1828`): the link predicates
/// plus the `is_member=Exists(...)` annotation (explicit
/// `ProjectMember.objects` filter — WITH the manager's `deleted_at IS
/// NULL`, unlike the guard). Newest first. (Moved from the v1
/// work-items handlers by PIDASHCONV-1661.)
pub async fn fetch_comment_list_rows(
    pool: &PgPool,
    slug: &str,
    project_id: &Uuid,
    issue_id: &Uuid,
    user_id: &Uuid,
) -> Result<Vec<CommentDbRow>, Error> {
    let db = pidash_db::pool::sea_orm_connection(pool);
    let rows = comment_list_select(slug, project_id, issue_id, user_id)
        .into_model::<CommentDbRow>()
        .all(&db)
        .await
        .map_err(|error| DbError::from_orm("comment-list", error))?;
    Ok(rows)
}

/// `IssueComment.objects.filter(workspace__slug=..., project_id=...,
/// issue_id=...).order_by("created_at")[:limit]`
/// (`pi_dash_cloud/mcp/tools.py:1151-1158`): the default manager hides
/// soft-deleted rows; ascending `created_at`; the caller resolved the
/// issue and checked membership before calling, so the queryset carries
/// no member/archived guards of its own. (Moved from the MCP read
/// tools by PIDASHCONV-1661; Rule 5: queries on OSS tables live here.)
pub async fn fetch_comment_list_ascending(
    pool: &PgPool,
    workspace_slug: &str,
    project_id: Uuid,
    issue_id: Uuid,
    limit: i64,
) -> Result<Vec<CommentDbRow>, Error> {
    // Django slices `[:limit]` and raises on a negative limit (the old
    // driver also errored); keep the failure instead of clamping.
    if limit < 0 {
        return Err(Error::Failed("comment-list-ascending"));
    }
    let db = pidash_db::pool::sea_orm_connection(pool);
    let rows = comment_columns(issue_comments::Entity::find().select_only())
        .column_as(Expr::val(None::<bool>), "is_member")
        .join(
            JoinType::InnerJoin,
            issue_comments::Relation::Workspaces.def(),
        )
        .filter(workspaces::Column::Slug.eq(workspace_slug))
        .filter(issue_comments::Column::ProjectId.eq(project_id))
        .filter(issue_comments::Column::IssueId.eq(issue_id))
        .filter(issue_comments::Column::DeletedAt.is_null())
        .order_by_asc(issue_comments::Column::CreatedAt)
        .limit(u64::try_from(limit).unwrap_or(u64::MAX))
        .into_model::<CommentDbRow>()
        .all(&db)
        .await
        .map_err(|error| DbError::from_orm("comment-list-ascending", error))?;
    Ok(rows)
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

/// A `array_to_json` output into the old `Vec<Option<String>>`
/// decode: `null` elements stay `None`; anything else than a string
/// fails like the old by-name read did.
fn comment_strings(value: &Value, site: &'static str) -> Result<Vec<Option<String>>, Error> {
    let items = value.as_array().ok_or(Error::Failed(site))?;
    items
        .iter()
        .map(|item| match item {
            Value::Null => Ok(None),
            Value::String(text) => Ok(Some(text.clone())),
            _ => Err(Error::Failed(site)),
        })
        .collect()
}

pub fn decode_comment(row: &CommentDbRow, tz: &Tz) -> Result<DecodedComment, Error> {
    Ok(DecodedComment {
        id: row.id.to_string(),
        is_member: row.is_member,
        created_at: super::render_datetime_in(&row.created_at, tz),
        updated_at: super::render_datetime_in(&row.updated_at, tz),
        deleted_at: row
            .deleted_at
            .as_ref()
            .map(|dt| super::render_datetime_in(dt, tz)),
        comment_json: row.comment_json.clone(),
        comment_html: row.comment_html.clone(),
        attachments: comment_strings(&row.attachments, "row-attachments")?,
        labels: comment_strings(&row.labels, "row-labels")?,
        access: row.access.clone(),
        external_source: row.external_source.clone(),
        external_id: row.external_id.clone(),
        speaker_type: row.speaker_type.clone(),
        speaker_label: row.speaker_label.clone(),
        speaker_agent_run_id: row.speaker_agent_run_id.map(|id| id.to_string()),
        edited_at: row
            .edited_at
            .as_ref()
            .map(|dt| super::render_datetime_in(dt, tz)),
        created_by: row.created_by_id.map(|id| id.to_string()),
        updated_by: row.updated_by_id.map(|id| id.to_string()),
        project: row.project_id.to_string(),
        workspace: row.workspace_id.to_string(),
        description: row.description_id.map(|id| id.to_string()),
        issue: row.issue_id.to_string(),
        actor: row.actor_id.map(|id| id.to_string()),
        parent: row.parent_id.map(|id| id.to_string()),
    })
}

#[cfg(test)]
mod tests {
    use sea_orm::sea_query::Iden;
    use sea_orm::{
        ColumnTrait, DbBackend, EntityTrait, QueryFilter, QueryOrder, QuerySelect, QueryTrait,
    };

    use super::*;

    #[test]
    fn relations_pool_qualifies_columns_and_filters_membership_in_where() {
        let viewer = Uuid::from_u128(7);
        let sql = relations_pool_select(&[Uuid::nil()], &Uuid::nil(), &viewer)
            .build(DbBackend::Postgres)
            .to_string();
        // issues, states and projects are all joined: every shared column
        // must carry its table, or Postgres answers 42702 ambiguous.
        assert!(sql.contains(r#""projects"."archived_at" IS NULL"#), "{sql}");
        assert!(!sql.contains(r#"AND "archived_at""#), "{sql}");
        assert!(sql.contains(r#""states"."group" IS NULL"#), "{sql}");
        // Membership is a correlated WHERE on the viewer, not SELECT
        // expressions (behaviour is pinned by the live test).
        assert!(
            sql.contains(r#""pm"."project_id" = "issues"."project_id""#),
            "{sql}"
        );
        assert!(
            sql.contains(&format!(r#""pm"."member_id" = '{viewer}'"#)),
            "{sql}"
        );
        assert!(!sql.contains(r#""pm"."deleted_at""#), "{sql}");
    }

    #[test]
    fn comment_statements_keep_scope_and_annotations() {
        // Direct lookup: no DISTINCT, NULL annotation, array JSON.
        let sql = comment_direct_select("acme", &Uuid::nil(), &Uuid::nil(), &Uuid::nil())
            .build(DbBackend::Postgres)
            .to_string();
        assert!(sql.starts_with(r#"SELECT "issue_comments"."id""#), "{sql}");
        assert!(!sql.contains("DISTINCT"), "{sql}");
        assert!(
            sql.contains(r#"array_to_json("issue_comments"."attachments")"#),
            "{sql}"
        );
        assert!(
            sql.contains(r#"array_to_json("issue_comments"."labels")"#),
            "{sql}"
        );
        assert!(sql.contains(r#"AS "is_member""#), "{sql}");
        assert!(
            sql.contains(r#""issue_comments"."deleted_at" IS NULL"#),
            "{sql}"
        );
        assert!(sql.contains(r#""issue_comments"."project_id" = "#), "{sql}");
        assert!(sql.contains(r#""issue_comments"."issue_id" = "#), "{sql}");
        assert!(sql.contains(r#""issue_comments"."id" = "#), "{sql}");
        // List: DISTINCT + is_member EXISTS + member/archived EXISTS.
        let sql = comment_list_select("acme", &Uuid::nil(), &Uuid::nil(), &Uuid::nil())
            .build(DbBackend::Postgres)
            .to_string();
        assert!(
            sql.starts_with(r#"SELECT DISTINCT "issue_comments"."id""#),
            "{sql}"
        );
        assert!(
            sql.contains(r#"EXISTS(SELECT 1 FROM "project_members" AS "pm""#),
            "{sql}"
        );
        assert!(sql.contains(r#""pm"."deleted_at" IS NULL"#), "{sql}");
        assert!(
            sql.contains(r#"EXISTS(SELECT 1 FROM "projects" AS "p""#),
            "{sql}"
        );
        assert!(
            sql.ends_with(r#"ORDER BY "issue_comments"."created_at" DESC"#),
            "{sql}"
        );
        // Every output name is selected exactly once: the row decodes
        // by name.
        let select_list = sql
            .split(r#" FROM "issue_comments""#)
            .next()
            .expect("select");
        for column in COMMENT_COLUMNS {
            let name = format!(r#""issue_comments"."{}""#, column.to_string());
            assert_eq!(select_list.matches(&name).count(), 1, "{name} in {sql}");
        }
    }

    #[test]
    fn file_asset_and_id_queries_are_scoped() {
        let sql = file_assets::Entity::find_by_id(Uuid::nil())
            .select_only()
            .columns([
                file_assets::Column::EntityType,
                file_assets::Column::WorkspaceId,
                file_assets::Column::ProjectId,
                file_assets::Column::IssueId,
            ])
            .build(DbBackend::Postgres)
            .to_string();
        assert!(sql.contains(r#""file_assets"."entity_type""#), "{sql}");
        let sql = issue_assignees::Entity::find()
            .filter(issue_assignees::Column::IssueId.eq(Uuid::nil()))
            .filter(issue_assignees::Column::DeletedAt.is_null())
            .order_by_desc(issue_assignees::Column::CreatedAt)
            .select_only()
            .column(issue_assignees::Column::AssigneeId)
            .build(DbBackend::Postgres)
            .to_string();
        assert!(
            sql.contains(r#""issue_assignees"."deleted_at" IS NULL"#),
            "{sql}"
        );
        assert!(
            sql.contains(r#"ORDER BY "issue_assignees"."created_at" DESC"#),
            "{sql}"
        );
    }

    #[test]
    fn expand_queries_preserve_scopes() {
        let sql = states::Entity::find_by_id(Uuid::nil())
            .filter(states::Column::DeletedAt.is_null())
            .select_only()
            .columns([
                states::Column::Name,
                states::Column::Color,
                states::Column::Group,
            ])
            .build(DbBackend::Postgres)
            .to_string();
        assert!(sql.contains(r#""states"."deleted_at" IS NULL"#), "{sql}");
        let sql = workspaces::Entity::find_by_id(Uuid::nil())
            .select_only()
            .columns([workspaces::Column::Name, workspaces::Column::Slug])
            .build(DbBackend::Postgres)
            .to_string();
        assert!(!sql.contains("deleted_at"), "{sql}");
    }

    #[test]
    fn blocker_and_grouped_queries_preserve_scopes_and_order() {
        let sql = issue_relations::Entity::find()
            .filter(issue_relations::Column::DeletedAt.is_null())
            .filter(
                sea_orm::sea_query::Expr::col(issue_relations::Column::IssueId).ne(
                    sea_orm::sea_query::Expr::col(issue_relations::Column::RelatedIssueId),
                ),
            )
            .filter(issue_relations::Column::RelationType.eq("blocked_by"))
            .build(DbBackend::Postgres)
            .to_string();
        assert!(
            sql.contains(r#""issue_relations"."deleted_at" IS NULL"#),
            "{sql}"
        );
        let sql = issues::Entity::find()
            .filter(issues::Column::DeletedAt.is_null())
            .filter(issues::Column::ArchivedAt.is_null())
            .filter(issues::Column::IsDraft.eq(false))
            .build(DbBackend::Postgres)
            .to_string();
        assert!(sql.contains(r#""issues"."deleted_at" IS NULL"#), "{sql}");
        assert!(sql.contains(r#""issues"."archived_at" IS NULL"#), "{sql}");
        assert!(sql.contains(r#""issues"."is_draft" = FALSE"#), "{sql}");
    }

    #[test]
    fn open_blocker_presence_keeps_case_join_and_open_scope() {
        let sql = has_open_blockers_statement(&Uuid::nil())
            .to_string(sea_orm::sea_query::PostgresQueryBuilder);
        // The CASE-selected other end stays in the join target, and the
        // blockers' two directions plus the open-group exclusion and
        // the limit-1 presence shape are all in the statement.
        assert!(sql.contains("CASE WHEN"), "{sql}");
        assert!(
            sql.contains(r#""r"."relation_type" = 'blocked_by') THEN "r"."related_issue_id" ELSE "r"."issue_id""#),
            "{sql}"
        );
        assert!(
            sql.contains(r#"NOT "s"."group" IN ('completed', 'cancelled')"#),
            "{sql}"
        );
        assert!(sql.contains(r#""s"."group" <> 'triage'"#), "{sql}");
        assert!(sql.contains(r#""t"."deleted_at" IS NULL"#), "{sql}");
        assert!(sql.contains(r#""t"."archived_at" IS NULL"#), "{sql}");
        assert!(sql.contains(r#""t"."is_draft" = FALSE"#), "{sql}");
        assert!(sql.contains("LIMIT 1"), "{sql}");
    }

    #[test]
    fn assignee_and_label_subqueries_preserve_order() {
        let sql = users::Entity::find()
            .filter(
                sea_orm::sea_query::Expr::col(users::Column::Id).in_subquery(
                    sea_orm::sea_query::Query::select()
                        .column(issue_assignees::Column::AssigneeId)
                        .from(issue_assignees::Entity)
                        .to_owned(),
                ),
            )
            .order_by_desc(users::Column::CreatedAt)
            .build(DbBackend::Postgres)
            .to_string();
        assert!(
            sql.contains(r#"ORDER BY "users"."created_at" DESC"#),
            "{sql}"
        );
        assert!(sql.contains("IN"), "{sql}");
    }

    #[test]
    fn issue_write_query_joins_and_limits() {
        let sql = issues::Entity::find()
            .filter(issues::Column::DeletedAt.is_null())
            .filter(issues::Column::Id.eq(Uuid::nil()))
            .filter(issues::Column::ProjectId.eq(Uuid::nil()))
            .limit(1)
            .build(DbBackend::Postgres)
            .to_string();
        assert!(sql.contains(r#""issues"."deleted_at" IS NULL"#), "{sql}");
        assert!(sql.contains("LIMIT 1"), "{sql}");
    }
}

#[cfg(test)]
mod live {
    use chrono_tz::Tz;
    use uuid::Uuid;

    use super::*;
    use crate::read::test_support::{pool, seed_world};

    /// The converted expansion and relations-pool queries run on Postgres
    /// (their bugs were planner errors that SQL-text tests cannot see).
    #[tokio::test]
    async fn expansion_and_relation_queries_plan_on_postgres() {
        use sea_orm::QueryTrait;
        let Some(db) = pool().await else { return };
        let world = seed_world(&db).await;
        let issue = world.issue_id;
        sqlx::query(
            "INSERT INTO issue_assignees (id, created_at, updated_at, assignee_id, issue_id,
                project_id, workspace_id)
             VALUES ($1, now(), now(), $2, $3, $4, $5)",
        )
        .bind(Uuid::new_v4())
        .bind(world.user_id)
        .bind(issue)
        .bind(world.project_id)
        .bind(world.workspace_id)
        .execute(&db)
        .await
        .expect("seed assignee");
        let assignees = fetch_assignee_rows(&db, &issue).await.expect("assignees");
        assert_eq!(assignees.len(), 1);
        // One live and one soft-deleted label, both still linked: only the
        // live one renders (`Label.objects` is the soft-deletion manager).
        let (live_label, deleted_label) = (Uuid::new_v4(), Uuid::new_v4());
        for (label, deleted) in [(live_label, false), (deleted_label, true)] {
            sqlx::query(
                "INSERT INTO labels (id, created_at, updated_at, name, description, workspace_id,
                    color, sort_order, project_id, deleted_at)
                 VALUES ($1, now(), now(), $2, '', $3, '#000000', 65535, $4,
                    CASE WHEN $5 THEN now() END)",
            )
            .bind(label)
            .bind(format!("label-{}", label.simple()))
            .bind(world.workspace_id)
            .bind(world.project_id)
            .bind(deleted)
            .execute(&db)
            .await
            .expect("seed label");
            sqlx::query(
                "INSERT INTO issue_labels (id, created_at, updated_at, issue_id, label_id,
                    project_id, workspace_id)
                 VALUES ($1, now(), now(), $2, $3, $4, $5)",
            )
            .bind(Uuid::new_v4())
            .bind(issue)
            .bind(label)
            .bind(world.project_id)
            .bind(world.workspace_id)
            .execute(&db)
            .await
            .expect("seed issue label");
        }
        let labels = fetch_expanded_labels(&db, &issue, &chrono_tz::UTC)
            .await
            .expect("labels");
        let ids: Vec<&str> = labels.iter().filter_map(|l| l["id"].as_str()).collect();
        assert_eq!(ids, vec![live_label.to_string().as_str()]);
        let sql = relations_pool_select(&[issue], &world.workspace_id, &world.user_id)
            .build(sea_orm::DbBackend::Postgres);
        sqlx::query(&format!("EXPLAIN {sql}"))
            .execute(&db)
            .await
            .expect("relations pool plans");
    }

    /// The relations pool is narrowed to the viewer's active project
    /// memberships: a member sees the related issue, a non-member does not,
    /// and a soft-deleted but active membership still counts (Django's join
    /// span has no `deleted_at` scope).
    #[tokio::test]
    async fn grouped_relations_follow_project_membership() {
        let Some(db) = pool().await else { return };
        let world = seed_world(&db).await;
        let related = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO issues (id, created_at, updated_at, name, description_json, priority,
                sequence_id, project_id, state_id, workspace_id, description_html, sort_order,
                is_draft, git_work_branch, workpad, complexity_score)
             VALUES ($1, now(), now(), 'Related issue', '{}', 'high', 102, $2, $3, $4, '',
                65535, false, '', '', 0)",
        )
        .bind(related)
        .bind(world.project_id)
        .bind(world.state_id)
        .bind(world.workspace_id)
        .execute(&db)
        .await
        .expect("seed related issue");
        sqlx::query(
            "INSERT INTO issue_relations (id, created_at, updated_at, relation_type, issue_id,
                project_id, related_issue_id, workspace_id)
             VALUES ($1, now(), now(), 'relates_to', $2, $3, $4, $5)",
        )
        .bind(Uuid::new_v4())
        .bind(world.issue_id)
        .bind(world.project_id)
        .bind(related)
        .bind(world.workspace_id)
        .execute(&db)
        .await
        .expect("seed relation");
        let seen = |groups: Vec<(String, Vec<OwnedRelationItem>)>| -> Vec<String> {
            groups
                .into_iter()
                .flat_map(|(_, items)| items.into_iter().map(|item| item.id))
                .collect()
        };
        let fetch = |viewer: Uuid| {
            let db = db.clone();
            async move {
                fetch_grouped_rows(&db, &world.issue_id, &world.workspace_id, &viewer)
                    .await
                    .expect("grouped rows")
            }
        };
        // No membership row yet: the viewer sees nothing.
        assert!(seen(fetch(world.user_id).await).is_empty());
        let membership = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO project_members (id, created_at, updated_at, role, project_id,
                workspace_id, view_props, default_props, sort_order, preferences, is_active,
                member_id)
             VALUES ($1, now(), now(), 20, $2, $3, '{}', '{}', 65535, '{}', true, $4)",
        )
        .bind(membership)
        .bind(world.project_id)
        .bind(world.workspace_id)
        .bind(world.user_id)
        .execute(&db)
        .await
        .expect("seed membership");
        assert_eq!(seen(fetch(world.user_id).await), vec![related.to_string()]);
        // Another user is not a member of the project.
        assert!(seen(fetch(Uuid::new_v4()).await).is_empty());
        // Soft-deleted but active still counts; inactive does not.
        sqlx::query("UPDATE project_members SET deleted_at = now() WHERE id = $1")
            .bind(membership)
            .execute(&db)
            .await
            .expect("soft-delete membership");
        assert_eq!(seen(fetch(world.user_id).await), vec![related.to_string()]);
        sqlx::query("UPDATE project_members SET is_active = false WHERE id = $1")
            .bind(membership)
            .execute(&db)
            .await
            .expect("deactivate membership");
        assert!(seen(fetch(world.user_id).await).is_empty());
    }

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

    async fn seed_member(pool: &PgPool, world: &crate::read::test_support::World) {
        sqlx::query(
            "INSERT INTO project_members (id, role, project_id, workspace_id, member_id, \
             view_props, default_props, sort_order, preferences, is_active, created_at, updated_at) \
             VALUES ($1, 20, $2, $3, $4, '{}', '{}', 65535, '{}', true, now(), now())",
        )
        .bind(Uuid::new_v4())
        .bind(world.project_id)
        .bind(world.workspace_id)
        .bind(world.user_id)
        .execute(pool)
        .await
        .expect("seed member");
    }

    #[tokio::test]
    async fn comment_list_rows_hit_desc_with_member() {
        let Some(db) = pool().await else { return };
        let world = seed_world(&db).await;
        seed_member(&db, &world).await;
        let rows = fetch_comment_list_rows(
            &db,
            &world.workspace_slug,
            &world.project_id,
            &world.issue_id,
            &world.user_id,
        )
        .await
        .expect("hit");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, world.comment_id);
        assert_eq!(rows[0].is_member, Some(true));
    }

    #[tokio::test]
    async fn comment_list_ascending_order_and_limit() {
        let Some(db) = pool().await else { return };
        let world = seed_world(&db).await;
        let second = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO issue_comments (id, workspace_id, project_id, comment_stripped, \
             comment_json, comment_html, attachments, labels, issue_id, actor_id, access, \
             speaker_type, speaker_label, created_at, updated_at) \
             VALUES ($1,$2,$3,'second','{}','<p>second</p>','{}','{}',$4,$5, \
             'EXTERNAL','human','',now() + interval '1 second',now())",
        )
        .bind(second)
        .bind(world.workspace_id)
        .bind(world.project_id)
        .bind(world.issue_id)
        .bind(world.user_id)
        .execute(&db)
        .await
        .expect("seed second comment");
        let rows = fetch_comment_list_ascending(
            &db,
            &world.workspace_slug,
            world.project_id,
            world.issue_id,
            10,
        )
        .await
        .expect("hit");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].id, world.comment_id);
        assert_eq!(rows[1].id, second);
        let one = fetch_comment_list_ascending(
            &db,
            &world.workspace_slug,
            world.project_id,
            world.issue_id,
            1,
        )
        .await
        .expect("limited");
        assert_eq!(one.len(), 1);
        assert_eq!(one[0].id, world.comment_id);
    }

    #[tokio::test]
    async fn blocker_open_presence_tracks_open_blocker() {
        let Some(db) = pool().await else { return };
        let world = seed_world(&db).await;
        // No relations: nothing open.
        let rows = fetch_blocker_rows(&db, &world.issue_id)
            .await
            .expect("reads");
        assert!(!rows.has_open_blockers);
        // A blocked by B, B in the seeded open (`backlog`) state: open.
        let blocker_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO issues (id, created_at, updated_at, name, description_json, priority,\
             sequence_id, project_id, state_id, workspace_id, description_html, sort_order,\
             is_draft, git_work_branch, workpad, complexity_score) \
             VALUES ($1, now(), now(), 'Blocker', '{}', 'high', 102, $2, $3, $4, '', 65535, \
             false, '', '', 0)",
        )
        .bind(blocker_id)
        .bind(world.project_id)
        .bind(world.state_id)
        .bind(world.workspace_id)
        .execute(&db)
        .await
        .expect("seed blocker issue");
        sqlx::query(
            "INSERT INTO issue_relations (id, relation_type, issue_id, project_id, \
             related_issue_id, workspace_id, created_at, updated_at) \
             VALUES ($1, 'blocked_by', $2, $3, $4, $5, now(), now())",
        )
        .bind(Uuid::new_v4())
        .bind(world.issue_id)
        .bind(world.project_id)
        .bind(blocker_id)
        .bind(world.workspace_id)
        .execute(&db)
        .await
        .expect("seed blocked_by relation");
        let rows = fetch_blocker_rows(&db, &world.issue_id)
            .await
            .expect("reads");
        assert!(rows.has_open_blockers);
        // The blocker itself blocks nothing.
        let reverse = fetch_blocker_rows(&db, &blocker_id).await.expect("reads");
        assert!(!reverse.has_open_blockers);
    }
}
