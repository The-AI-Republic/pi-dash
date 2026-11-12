//! Shared read functions (NEWPIDASH-190 §4.6): the reads the cloud agent
//! tools and the MCP/overlay tools both issue through one typed function,
//! over a caller-supplied connection (a transaction, the SeaORM pool
//! connection, or a runner's pool turned into the former).
//!
//! Each function returns a typed view carrying exactly the fields both
//! wrappers need; the wrappers shape to their capped dicts (the agent
//! caps description/workpad, sorts and serializes; MCP renders the v1
//! shape). Differences between the surfaces travel as parameters, not
//! hidden logic: the agent's `bound_issue` lookup has no `deleted_at`
//! filter, the agent's search matches title+stripped text capped at
//! `limit` inside one project, MCP's `grouped_relations` query services
//! the sorted-truncated map without the member-pool guard.
//!
//! NOTE (NEWPIDASH-190 Part 3): the two read dialects in the tree
//! (`operations::relations::grouped_relations` (sqlx pool) and the agent's
//! `grouped_tx`) converge on [`grouped_relations`] below; the v1 handler
//! reads (`services::read::*`) keep riding their own functions — the MCP
//! overlay re-point lands in 3.4 (outside this checkout).

use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use crate::operations::core::CoreError;

/// One issue as the read wrappers render it: identity, project, state and
/// the text the read surfaces carry (capped by the wrapper).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssueView {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub project_id: Uuid,
    pub sequence_id: i32,
    pub name: String,
    pub priority: String,
    pub description_stripped: Option<String>,
    pub workpad: Option<String>,
    pub state_name: Option<String>,
    pub state_group: Option<String>,
    pub project_identifier: String,
}

/// The search hit: the brief `dispatch::tool_issues::ToolIssueBrief`
/// fields both wrappers carry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHit {
    pub id: Uuid,
    pub sequence_id: i32,
    pub name: String,
    pub priority: String,
    pub state_name: Option<String>,
    pub state_group: Option<String>,
    pub project_identifier: String,
}

/// One issue comment as the read wrappers carry it.
#[derive(Debug, Clone, PartialEq)]
pub struct CommentView {
    pub id: Uuid,
    pub issue_id: Uuid,
    pub body_html: String,
    pub comment_stripped: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// The comment ordering of a list (`created_at` asc/desc).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListOrder {
    Asc,
    Desc,
}

/// One state as the read wrappers carry it (`id`, `name`, `group` in
/// `sequence` order).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateView {
    pub id: Uuid,
    pub name: String,
    pub group: String,
}

/// One linked review as the read wrappers carry it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewView {
    pub id: Uuid,
    pub provider: String,
    pub url: String,
    pub title: String,
    pub state: String,
    pub number: String,
}

fn map_db(error: sea_orm::DbErr) -> CoreError {
    CoreError::Db(crate::domain::DomainError::Db(error))
}

/// The state join: issue identity + state name/group + project identifier
/// without scope filters beyond the caller's.
#[derive(Debug, Clone, sea_orm::FromQueryResult)]
struct IssueRow {
    id: Uuid,
    workspace_id: Uuid,
    project_id: Uuid,
    sequence_id: i32,
    name: String,
    priority: String,
    description_stripped: Option<String>,
    workpad: String,
    state_name: Option<String>,
    state_group: Option<String>,
    project_identifier: String,
}

impl From<IssueRow> for IssueView {
    fn from(row: IssueRow) -> Self {
        Self {
            id: row.id,
            workspace_id: row.workspace_id,
            project_id: row.project_id,
            sequence_id: row.sequence_id,
            name: row.name,
            priority: row.priority,
            description_stripped: row.description_stripped,
            workpad: Some(row.workpad),
            state_name: row.state_name,
            state_group: row.state_group,
            project_identifier: row.project_identifier,
        }
    }
}

/// `issue_by_id(conn, id, project)` (NEWPIDASH-190 §4.6): the read shared
/// by `pidash_get_current_issue`, `pidash_get_project_issue` and
/// `pidash_get_issue`. The union `IssueView` covers the fields both
/// wrappers shape; the wrapper caps the text. `live_only` carries the
/// documented difference: the agent's `bound_issue` lookup has no
/// `deleted_at` filter.
pub async fn issue_by_id<C: sea_orm::ConnectionTrait>(
    conn: &C,
    issue_id: Uuid,
    project_id: Option<Uuid>,
    live_only: bool,
) -> Result<Option<IssueView>, CoreError> {
    use pidash_db::entity::{issues, projects, states};
    use sea_orm::{ColumnTrait, EntityTrait, JoinType, QueryFilter, QuerySelect, RelationTrait};
    let mut query = issues::Entity::find()
        .select_only()
        .columns([
            issues::Column::Id,
            issues::Column::WorkspaceId,
            issues::Column::ProjectId,
            issues::Column::SequenceId,
            issues::Column::Name,
            issues::Column::Priority,
            issues::Column::DescriptionStripped,
            issues::Column::Workpad,
        ])
        .column_as(states::Column::Name, "state_name")
        .column_as(states::Column::Group, "state_group")
        .column_as(projects::Column::Identifier, "project_identifier")
        .join(JoinType::InnerJoin, issues::Relation::Projects.def())
        .join(JoinType::LeftJoin, issues::Relation::States.def())
        .filter(issues::Column::Id.eq(issue_id));
    if live_only {
        query = query.filter(issues::Column::DeletedAt.is_null());
    }
    if let Some(project_id) = project_id {
        query = query.filter(issues::Column::ProjectId.eq(project_id));
    }
    let row = query
        .into_model::<IssueRow>()
        .one(conn)
        .await
        .map_err(map_db)?;
    Ok(row.map(IssueView::from))
}

#[derive(Debug, Clone, sea_orm::FromQueryResult)]
struct BriefRow {
    id: Uuid,
    sequence_id: i32,
    name: String,
    priority: String,
    state_name: Option<String>,
    state_group: Option<String>,
    project_identifier: String,
}

impl From<BriefRow> for SearchHit {
    fn from(row: BriefRow) -> Self {
        Self {
            id: row.id,
            sequence_id: row.sequence_id,
            name: row.name,
            priority: row.priority,
            state_name: row.state_name,
            state_group: row.state_group,
            project_identifier: row.project_identifier,
        }
    }
}

/// `search_issues(conn, scope, query, limit)` (NEWPIDASH-190 §4.6):
/// name + description-stripped icontains matches, newest first. The
/// agent's search is inside one project with a row cap; MCP's is
/// workspace-wide, so `project_id` travels as a parameter.
pub async fn search_issues<C: sea_orm::ConnectionTrait>(
    conn: &C,
    project_id: Option<Uuid>,
    escaped: &str,
    limit: u64,
) -> Result<Vec<SearchHit>, CoreError> {
    use pidash_db::entity::{issues, projects, states};
    use sea_orm::sea_query::{BinOper, Expr, Func};
    use sea_orm::{
        ColumnTrait, Condition, EntityTrait, JoinType, QueryFilter, QueryOrder, QuerySelect,
        RelationTrait,
    };
    let pattern = format!("%{escaped}%");
    let contains = |column: issues::Column| {
        Expr::expr(Func::upper(Expr::col((issues::Entity, column))))
            .binary(BinOper::Like, Func::upper(Expr::val(pattern.clone())))
    };
    let mut query = issues::Entity::find()
        .select_only()
        .columns([
            issues::Column::Id,
            issues::Column::SequenceId,
            issues::Column::Name,
            issues::Column::Priority,
        ])
        .column_as(states::Column::Name, "state_name")
        .column_as(states::Column::Group, "state_group")
        .column_as(projects::Column::Identifier, "project_identifier")
        .join(JoinType::InnerJoin, issues::Relation::Projects.def())
        .join(JoinType::LeftJoin, issues::Relation::States.def())
        .filter(issues::Column::DeletedAt.is_null());
    if let Some(project_id) = project_id {
        query = query.filter(issues::Column::ProjectId.eq(project_id));
    }
    let rows = query
        .filter(
            Condition::any()
                .add(contains(issues::Column::Name))
                .add(contains(issues::Column::DescriptionStripped)),
        )
        .order_by_desc(issues::Column::CreatedAt)
        .limit(limit)
        .into_model::<BriefRow>()
        .all(conn)
        .await
        .map_err(map_db)?;
    Ok(rows.into_iter().map(SearchHit::from).collect())
}

#[derive(Debug, Clone, sea_orm::FromQueryResult)]
struct CommentRow {
    id: Uuid,
    issue_id: Uuid,
    body_html: String,
    comment_stripped: Option<String>,
    created_at: sea_orm::prelude::DateTimeWithTimeZone,
}

/// `list_comments(conn, issue, order, limit)` (NEWPIDASH-190 §4.6): live
/// comments of one issue; order and limit travel as parameters (the agent
/// lists newest-first, MCP lists ascending).
pub async fn list_comments<C: sea_orm::ConnectionTrait>(
    conn: &C,
    issue_id: Uuid,
    order: ListOrder,
    limit: u64,
) -> Result<Vec<CommentView>, CoreError> {
    use pidash_db::entity::issue_comments as ic_entity;
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect};
    let mut query = ic_entity::Entity::find()
        .filter(ic_entity::Column::IssueId.eq(issue_id))
        .filter(ic_entity::Column::DeletedAt.is_null());
    query = match order {
        ListOrder::Asc => query.order_by_asc(ic_entity::Column::CreatedAt),
        ListOrder::Desc => query.order_by_desc(ic_entity::Column::CreatedAt),
    };
    let rows = query
        .limit(limit)
        .into_model::<CommentRow>()
        .all(conn)
        .await
        .map_err(map_db)?;
    Ok(rows
        .into_iter()
        .map(|row| CommentView {
            id: row.id,
            issue_id: row.issue_id,
            body_html: row.body_html,
            comment_stripped: row.comment_stripped,
            created_at: row.created_at.with_timezone(&Utc),
        })
        .collect())
}

#[derive(Debug, Clone, sea_orm::FromQueryResult)]
struct StateRow {
    id: Uuid,
    name: String,
    group: String,
}

/// `list_states(conn, project)` (NEWPIDASH-190 §4.6): live non-triage
/// states of the project in `sequence` order. `include_triage` carries
/// the MCP/behind-scope list (the agent's list filters it out at shape).
pub async fn list_states<C: sea_orm::ConnectionTrait>(
    conn: &C,
    project_id: Uuid,
    include_triage: bool,
) -> Result<Vec<StateView>, CoreError> {
    use pidash_db::entity::states as st_entity;
    use sea_orm::sea_query::Expr;
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder};
    let mut query = st_entity::Entity::find()
        .filter(st_entity::Column::ProjectId.eq(project_id))
        .filter(st_entity::Column::DeletedAt.is_null());
    if !include_triage {
        query = query.filter(Expr::col(st_entity::Column::Group).ne("triage"));
    }
    let rows = query
        .order_by_asc(st_entity::Column::Sequence)
        .into_model::<StateRow>()
        .all(conn)
        .await
        .map_err(map_db)?;
    Ok(rows
        .into_iter()
        .map(|row| StateView {
            id: row.id,
            name: row.name,
            group: row.group,
        })
        .collect())
}

#[derive(Debug, Clone, sea_orm::FromQueryResult)]
struct ReviewRow {
    id: Uuid,
    provider: String,
    url: String,
    title: String,
    state: String,
    external_iid: String,
}

/// `linked_reviews(conn, issue, limit)` (NEWPIDASH-190 §4.6): live
/// code-review links for the issue, newest first.
pub async fn linked_reviews<C: sea_orm::ConnectionTrait>(
    conn: &C,
    issue_id: Uuid,
    limit: u64,
) -> Result<Vec<ReviewView>, CoreError> {
    use pidash_db::entity::git_code_review_links as gc_entity;
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect};
    let rows = gc_entity::Entity::find()
        .filter(gc_entity::Column::IssueId.eq(issue_id))
        .filter(gc_entity::Column::DeletedAt.is_null())
        .order_by_desc(gc_entity::Column::CreatedAt)
        .limit(limit)
        .into_model::<ReviewRow>()
        .all(conn)
        .await
        .map_err(map_db)?;
    Ok(rows
        .into_iter()
        .map(|row| ReviewView {
            id: row.id,
            provider: row.provider,
            url: row.url,
            title: row.title,
            state: row.state,
            number: row.external_iid,
        })
        .collect())
}

/// The visibility predicate of a pool-scoped grouped read: the visible
/// pool query the caller evaluated the viewer against (active project
/// membership of the workspace, per the run's scope).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Visibility {
    pub creator: Uuid,
    pub slug: String,
}

impl Visibility {
    fn member_pool_stmt(&self) -> sea_orm::sea_query::SelectStatement {
        use pidash_db::entity::{
            issues as i_entity, project_members as pm_entity, projects as proj_entity,
            states as st_entity, workspaces as ws_entity,
        };
        use sea_orm::sea_query::Expr;
        use sea_orm::JoinType;
        sea_orm::sea_query::Query::select()
            .expr_as(Expr::col((i_entity::Entity, i_entity::Column::Id)), "id")
            .from(i_entity::Entity)
            .join(
                JoinType::LeftJoin,
                st_entity::Entity,
                Expr::col((st_entity::Entity, st_entity::Column::Id))
                    .equals((i_entity::Entity, i_entity::Column::StateId)),
            )
            .join(
                JoinType::InnerJoin,
                proj_entity::Entity,
                Expr::col((proj_entity::Entity, proj_entity::Column::Id))
                    .equals((i_entity::Entity, i_entity::Column::ProjectId)),
            )
            .join(
                JoinType::InnerJoin,
                pm_entity::Entity,
                Expr::col((pm_entity::Entity, pm_entity::Column::ProjectId))
                    .equals((proj_entity::Entity, proj_entity::Column::Id)),
            )
            .join(
                JoinType::InnerJoin,
                ws_entity::Entity,
                Expr::col((ws_entity::Entity, ws_entity::Column::Id))
                    .equals((i_entity::Entity, i_entity::Column::WorkspaceId)),
            )
            .and_where(Expr::col((i_entity::Entity, i_entity::Column::DeletedAt)).is_null())
            .and_where(
                Expr::col((st_entity::Entity, st_entity::Column::Group))
                    .is_null()
                    .or(Expr::col((st_entity::Entity, st_entity::Column::Group)).ne("triage")),
            )
            .and_where(Expr::col((i_entity::Entity, i_entity::Column::ArchivedAt)).is_null())
            .and_where(Expr::col((proj_entity::Entity, proj_entity::Column::ArchivedAt)).is_null())
            .and_where(Expr::col((i_entity::Entity, i_entity::Column::IsDraft)).eq(false))
            .and_where(Expr::col((pm_entity::Entity, pm_entity::Column::IsActive)).eq(true))
            .and_where(Expr::col((pm_entity::Entity, pm_entity::Column::MemberId)).eq(self.creator))
            .and_where(
                Expr::col((ws_entity::Entity, ws_entity::Column::Slug)).eq(self.slug.clone()),
            )
            .to_owned()
    }
}

/// One grouped item of the map (union fields both wrappers carry).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GroupedItem {
    pub id: Uuid,
    pub project_identifier: String,
    pub sequence_id: i32,
    pub name: String,
    pub state_name: Option<String>,
    pub state_group: Option<String>,
}

/// The typed grouped map the wrappers shape (relation keyed, in
/// `RELATION_TYPES` order).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Grouped {
    pub map: Vec<(String, Vec<GroupedItem>)>,
}

#[derive(Debug, Clone, sea_orm::FromQueryResult)]
struct PoolRow {
    id: Uuid,
    name: String,
    sequence_id: i32,
    project_identifier: String,
    state_name: Option<String>,
    state_group: Option<String>,
}

/// `grouped_relations(conn, source, visible_to)` (NEWPIDASH-190 §4.6):
/// the relation map shared by `pidash_list_issue_relations` on both
/// surfaces. `visible=None` renders the map against the unguarded pool
/// (ops shape); `Visible` filters through the member pool (agent shape).
pub async fn grouped_relations<C: sea_orm::ConnectionTrait>(
    conn: &C,
    source: &crate::orchestration::relations::ResolvedIssue,
    visible: Option<&Visibility>,
) -> Result<Grouped, CoreError> {
    use pidash_db::entity::{issue_relations as rel_entity, issues, projects, states};
    use sea_orm::{
        ColumnTrait, Condition, EntityTrait, JoinType, QueryFilter, QuerySelect, RelationTrait,
    };
    use std::collections::{HashMap, HashSet};

    #[derive(Debug, Clone, sea_orm::FromQueryResult)]
    struct RawPair {
        id: Uuid,
        issue_id: Uuid,
        related_issue_id: Uuid,
        relation_type: String,
    }
    let rows = rel_entity::Entity::find()
        .filter(
            Condition::any()
                .add(rel_entity::Column::IssueId.eq(source.id))
                .add(rel_entity::Column::RelatedIssueId.eq(source.id)),
        )
        .filter(rel_entity::Column::WorkspaceId.eq(source.workspace_id))
        .filter(
            sea_orm::sea_query::Expr::col(rel_entity::Column::IssueId).ne(
                sea_orm::sea_query::Expr::col(rel_entity::Column::RelatedIssueId),
            ),
        )
        .filter(rel_entity::Column::DeletedAt.is_null())
        .into_model::<RawPair>()
        .all(conn)
        .await
        .map_err(map_db)?;
    let rows: Vec<crate::orchestration::relations::PairRow> = rows
        .into_iter()
        .map(|row| crate::orchestration::relations::PairRow {
            id: row.id,
            issue_id: row.issue_id,
            related_issue_id: row.related_issue_id,
            relation_type: row.relation_type,
        })
        .collect();
    let mut other_by_type: HashMap<&str, HashSet<Uuid>> = HashMap::new();
    for relation in crate::orchestration::relations::RELATION_TYPES {
        other_by_type.insert(*relation, HashSet::new());
    }
    for row in &rows {
        let issue_id = row.issue_id;
        let related_id = row.related_issue_id;
        let relation_type = row.relation_type.clone();
        let other = if issue_id == source.id {
            related_id
        } else {
            issue_id
        };
        let probe = crate::orchestration::relations::PairRow {
            id: Uuid::nil(),
            issue_id,
            related_issue_id: related_id,
            relation_type,
        };
        let relation = crate::orchestration::relations::type_from(&probe, &source.id);
        if let Some(bucket) = other_by_type.get_mut(relation.as_str()) {
            bucket.insert(other);
        }
    }
    let wanted: Vec<Uuid> = other_by_type
        .values()
        .flat_map(|bucket| bucket.iter().copied())
        .collect();
    let mut others: HashMap<Uuid, GroupedItem> = HashMap::new();
    if !wanted.is_empty() {
        let query = match visible {
            None => issues::Entity::find()
                .filter(issues::Column::Id.is_in(wanted.clone()))
                .filter(issues::Column::WorkspaceId.eq(source.workspace_id)),
            Some(visibility) => {
                let pool_query = visibility.member_pool_stmt();
                issues::Entity::find()
                    .filter(issues::Column::Id.is_in(wanted.clone()))
                    .filter(issues::Column::WorkspaceId.eq(source.workspace_id))
                    .filter(issues::Column::Id.in_subquery(pool_query))
            }
        };
        let pool_rows = query
            .join(JoinType::LeftJoin, issues::Relation::States.def())
            .join(JoinType::InnerJoin, issues::Relation::Projects.def())
            .select_only()
            .columns([
                issues::Column::Id,
                issues::Column::Name,
                issues::Column::SequenceId,
            ])
            .column_as(projects::Column::Identifier, "project_identifier")
            .column_as(states::Column::Name, "state_name")
            .column_as(states::Column::Group, "state_group")
            .into_model::<PoolRow>()
            .all(conn)
            .await
            .map_err(map_db)?;
        for row in pool_rows {
            let item = GroupedItem {
                id: row.id,
                project_identifier: row.project_identifier,
                sequence_id: row.sequence_id,
                name: row.name,
                state_name: row.state_name,
                state_group: row.state_group,
            };
            others.insert(item.id, item);
        }
    }
    let mut map = Vec::with_capacity(crate::orchestration::relations::RELATION_TYPES.len());
    for relation in crate::orchestration::relations::RELATION_TYPES {
        let bucket = other_by_type.get(*relation).expect("seeded types");
        let mut items: Vec<&GroupedItem> = bucket.iter().filter_map(|id| others.get(id)).collect();
        items.sort_by(|a, b| {
            (&a.project_identifier, a.sequence_id).cmp(&(&b.project_identifier, b.sequence_id))
        });
        items.truncate(100);
        map.push(((*relation).to_owned(), items.into_iter().cloned().collect()));
    }
    Ok(Grouped { map })
}
