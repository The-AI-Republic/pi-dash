//! A work item's children with a progress rollup (PDASHOSS01-405).
//!
//! Ports `apps/api/pi_dash/orchestration/issue_tree.py` (`issue_tree`,
//! `rollup`, `_header`) behind `GET .../work-items/<id>/tree/`
//! (`IssueTreeAPIEndpoint`, `api/views/issue.py`). Output, key order and
//! ordering match Django:
//!
//! * `{issue, recursive, rollup, children, truncated}`; `issue` is
//!   `{id, identifier, name, state, state_group, priority}`; each child adds
//!   `parent` (identifier), `depth`, `assignees: [{id, display_name}]` and
//!   `active_run: {id, status} | null`.
//! * Descendants come from one recursive query over the caller's visible
//!   pool (`member_project_issues`), ranked `(depth, sequence_id, id)`,
//!   then listed depth-first pre-order. Each item is visited once; at most
//!   [`NODE_LIMIT`] children, past that `truncated`.
//! * `rollup.done` counts the closed groups (`completed`, `cancelled`);
//!   `open = total - done`; `by_state_group` carries every group in
//!   lifecycle order.
//! * Assignees are the live `issue_assignees` rows ordered by
//!   `(created_at, id)`; `active_run` is the newest (`created_at DESC, id
//!   DESC`) run in `ACTIVE_RUN_STATUSES` (Django's matcher
//!   `NON_TERMINAL_STATUSES`; not the `jobs` set of that name).

use std::collections::HashMap;

use sea_orm::{
    ColumnTrait, ConnectionTrait, EntityTrait, FromQueryResult, JoinType, QueryFilter, QueryOrder,
    QuerySelect, QueryTrait, RelationTrait,
};
use serde_json::{Map, Value};
use sqlx::PgPool;
use uuid::Uuid;

use pidash_db::entity::{agent_run, issue_assignees, issues, projects, states, users, workspaces};

use crate::app_issues::serializers_create::ACTIVE_RUN_STATUSES;

use super::common::db_error;
use super::error::Error;
use super::relations::{live_manager_scope, visible_pool_query};

/// Cap on listed children, all depths together (`issue_tree.NODE_LIMIT`).
pub const NODE_LIMIT: usize = 1000;

/// `STATE_GROUP_ORDER` (`utils/constants.py`).
pub const STATE_GROUP_ORDER: [&str; 7] = [
    "backlog",
    "unstarted",
    "started",
    "review",
    "test",
    "completed",
    "cancelled",
];

/// `CLOSED_STATE_GROUPS`.
const CLOSED_STATE_GROUPS: [&str; 2] = ["completed", "cancelled"];

#[derive(Debug, Clone, FromQueryResult)]
struct NodeRow {
    id: Uuid,
    parent_id: Option<Uuid>,
    name: String,
    priority: String,
    sequence_id: i32,
    project_identifier: String,
    state_name: Option<String>,
    state_group: Option<String>,
}

impl NodeRow {
    fn identifier(&self) -> String {
        format!("{}-{}", self.project_identifier, self.sequence_id)
    }

    /// `_header` (`issue_tree.py`).
    fn header(&self) -> Map<String, Value> {
        let mut out = Map::with_capacity(10);
        out.insert("id".to_owned(), Value::String(self.id.to_string()));
        out.insert("identifier".to_owned(), Value::String(self.identifier()));
        out.insert("name".to_owned(), Value::String(self.name.clone()));
        out.insert("state".to_owned(), opt_string(self.state_name.as_deref()));
        out.insert(
            "state_group".to_owned(),
            opt_string(self.state_group.as_deref()),
        );
        out.insert("priority".to_owned(), Value::String(self.priority.clone()));
        out
    }
}

fn opt_string(value: Option<&str>) -> Value {
    value.map_or(Value::Null, |text| Value::String(text.to_owned()))
}

fn node_select(select: sea_orm::Select<issues::Entity>) -> sea_orm::Select<issues::Entity> {
    select
        .select_only()
        .columns([
            issues::Column::Id,
            issues::Column::ParentId,
            issues::Column::Name,
            issues::Column::Priority,
            issues::Column::SequenceId,
        ])
        .column_as(projects::Column::Identifier, "project_identifier")
        .column_as(states::Column::Name, "state_name")
        .column_as(states::Column::Group, "state_group")
}

/// `rollup` (`issue_tree.py`): counts over the listed children's groups.
pub fn rollup<'a>(groups: impl IntoIterator<Item = Option<&'a str>>) -> Map<String, Value> {
    let mut counts = [0u64; STATE_GROUP_ORDER.len()];
    let mut total = 0u64;
    for group in groups {
        total += 1;
        if let Some(index) = group.and_then(|g| STATE_GROUP_ORDER.iter().position(|o| *o == g)) {
            counts[index] += 1;
        }
    }
    let done: u64 = STATE_GROUP_ORDER
        .iter()
        .zip(counts)
        .filter(|(group, _)| CLOSED_STATE_GROUPS.contains(group))
        .map(|(_, count)| count)
        .sum();
    let mut by_group = Map::with_capacity(STATE_GROUP_ORDER.len());
    for (group, count) in STATE_GROUP_ORDER.iter().zip(counts) {
        by_group.insert((*group).to_owned(), Value::from(count));
    }
    let mut out = Map::with_capacity(4);
    out.insert("total".to_owned(), Value::from(total));
    out.insert("done".to_owned(), Value::from(done));
    out.insert("open".to_owned(), Value::from(total - done));
    out.insert("by_state_group".to_owned(), Value::Object(by_group));
    out
}

/// Parse the `recursive` query value as the view does:
/// `(value or "false").strip().lower()` in `true|false|1|0`, else `None`
/// (the 400).
pub fn parse_recursive(raw: Option<&str>) -> Option<bool> {
    let raw = raw.filter(|text| !text.is_empty()).unwrap_or("false");
    match raw.trim().to_lowercase().as_str() {
        "true" | "1" => Some(true),
        "false" | "0" => Some(false),
        _ => None,
    }
}

/// `_source` (`views/issue.py`, `_IssueRelationAgentBase`): the live
/// work item by slug, project and id; a miss is the 404.
async fn fetch_header(
    db: &sea_orm::DatabaseConnection,
    slug: &str,
    project_id: &Uuid,
    issue_id: &Uuid,
) -> Result<NodeRow, Error> {
    node_select(
        issues::Entity::find()
            .join(JoinType::LeftJoin, issues::Relation::States.def())
            .join(JoinType::InnerJoin, issues::Relation::Projects.def())
            .join(JoinType::InnerJoin, issues::Relation::Workspaces.def()),
    )
    .filter(live_manager_scope())
    .filter(workspaces::Column::Slug.eq(slug.to_owned()))
    .filter(issues::Column::ProjectId.eq(*project_id))
    .filter(issues::Column::Id.eq(*issue_id))
    .into_model::<NodeRow>()
    .one(db)
    .await
    .map_err(|error| db_error(error, "tree-source"))?
    .ok_or(Error::NotFound)
}

/// `issue_tree(issue, member_project_issues(user, slug), recursive=…)`.
pub async fn issue_tree(
    pool: &PgPool,
    slug: &str,
    user_id: &Uuid,
    project_id: &Uuid,
    issue_id: &Uuid,
    recursive: bool,
) -> Result<Map<String, Value>, Error> {
    let db = pidash_db::pool::sea_orm_connection(pool);
    let source = fetch_header(&db, slug, project_id, issue_id).await?;

    let (rows, truncated) = fetch_descendants(&db, slug, user_id, source.id, recursive).await?;
    // `rows` is in (depth, sequence_id, id) order, so each sibling list
    // comes out by sequence_id.
    let mut children_of: HashMap<Uuid, Vec<NodeRow>> = HashMap::new();
    for row in rows {
        if let Some(parent) = row.parent_id {
            children_of.entry(parent).or_default().push(row);
        }
    }

    // Depth-first pre-order, siblings in fetch order.
    let mut nodes: Vec<(NodeRow, String, u64)> = Vec::new();
    walk(&children_of, source.id, &source.identifier(), 1, &mut nodes);

    let ids: Vec<Uuid> = nodes.iter().map(|(node, _, _)| node.id).collect();
    let assignees = fetch_assignees(&db, &ids).await?;
    let active = fetch_active_runs(&db, &ids).await?;

    let mut children = Vec::with_capacity(nodes.len());
    for (node, parent_ident, depth) in &nodes {
        let mut item = node.header();
        item.insert("parent".to_owned(), Value::String(parent_ident.clone()));
        item.insert("depth".to_owned(), Value::from(*depth));
        item.insert(
            "assignees".to_owned(),
            Value::Array(assignees.get(&node.id).cloned().unwrap_or_default()),
        );
        item.insert(
            "active_run".to_owned(),
            active.get(&node.id).cloned().unwrap_or(Value::Null),
        );
        children.push(Value::Object(item));
    }

    let mut out = Map::with_capacity(5);
    out.insert("issue".to_owned(), Value::Object(source.header()));
    out.insert("recursive".to_owned(), Value::Bool(recursive));
    out.insert(
        "rollup".to_owned(),
        Value::Object(rollup(
            nodes.iter().map(|(node, _, _)| node.state_group.as_deref()),
        )),
    );
    out.insert("children".to_owned(), Value::Array(children));
    out.insert("truncated".to_owned(), Value::Bool(truncated));
    Ok(out)
}

/// `_descendants` (`issue_tree.py`): the listed children in
/// `(depth, sequence_id, id)` order, and whether [`NODE_LIMIT`] cut them
/// short, from one `WITH RECURSIVE` query instead of one per level.
///
/// Only rows in the caller's visible pool are followed (a hidden child
/// stops the walk below it); the source is never re-entered, which is the
/// only way a parent cycle can be reached from it; depth is capped at
/// `NODE_LIMIT + 1`, since a deeper row cannot rank inside the first
/// `NODE_LIMIT + 1` rows.
async fn fetch_descendants(
    db: &sea_orm::DatabaseConnection,
    slug: &str,
    user_id: &Uuid,
    source_id: Uuid,
    recursive: bool,
) -> Result<(Vec<NodeRow>, bool), Error> {
    let stmt = descendants_query(slug, user_id, source_id, recursive);
    let mut rows = db
        .query_all(db.get_database_backend().build(&stmt))
        .await
        .map_err(|error| db_error(error, "tree-descendants"))?
        .iter()
        .map(|row| NodeRow::from_query_result(row, ""))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| db_error(error, "tree-descendants"))?;
    let truncated = rows.len() > NODE_LIMIT;
    rows.truncate(NODE_LIMIT);
    Ok((rows, truncated))
}

/// The statement behind [`fetch_descendants`].
fn descendants_query(
    slug: &str,
    user_id: &Uuid,
    source_id: Uuid,
    recursive: bool,
) -> sea_query::WithQuery {
    use sea_query::{Alias, CommonTableExpression, Expr, Query, UnionType, WithClause};

    let tree = Alias::new("tree");
    let depth = Alias::new("depth");
    let pool = || {
        visible_pool_query(slug, user_id)
            .select_only()
            .column(issues::Column::Id)
            .into_query()
    };
    let id = (issues::Entity, issues::Column::Id);

    let mut seed = Query::select();
    seed.column(id)
        .expr_as(Expr::val(1i32), depth.clone())
        .from(issues::Entity)
        .and_where(Expr::col((issues::Entity, issues::Column::ParentId)).eq(source_id))
        .and_where(Expr::col(id).ne(source_id))
        .and_where(Expr::col(id).in_subquery(pool()));
    let mut step = Query::select();
    step.column(id)
        .expr(Expr::col((tree.clone(), depth.clone())).add(1i32))
        .from(issues::Entity)
        .inner_join(
            tree.clone(),
            Expr::col((issues::Entity, issues::Column::ParentId))
                .equals((tree.clone(), Alias::new("id"))),
        )
        .and_where(Expr::val(recursive).into())
        .and_where(Expr::col((tree.clone(), depth.clone())).lte(NODE_LIMIT as i32))
        .and_where(Expr::col(id).ne(source_id))
        .and_where(Expr::col(id).in_subquery(pool()));
    seed.union(UnionType::All, step);

    let mut cte = CommonTableExpression::new();
    cte.table_name(tree.clone())
        .columns([Alias::new("id"), depth.clone()])
        .query(seed);
    let mut with = WithClause::new();
    with.recursive(true).cte(cte);

    node_select(
        issues::Entity::find()
            .join(JoinType::LeftJoin, issues::Relation::States.def())
            .join(JoinType::InnerJoin, issues::Relation::Projects.def()),
    )
    .into_query()
    .inner_join(
        tree.clone(),
        Expr::col(id).equals((tree.clone(), Alias::new("id"))),
    )
    .order_by((tree, depth), sea_query::Order::Asc)
    .order_by(
        (issues::Entity, issues::Column::SequenceId),
        sea_query::Order::Asc,
    )
    .order_by(id, sea_query::Order::Asc)
    .limit(NODE_LIMIT as u64 + 1)
    .to_owned()
    .with(with)
}

/// Depth-first pre-order over `children_of` with an explicit stack (a
/// chain can be [`NODE_LIMIT`] deep).
fn walk(
    children_of: &HashMap<Uuid, Vec<NodeRow>>,
    parent_id: Uuid,
    parent_ident: &str,
    depth: u64,
    nodes: &mut Vec<(NodeRow, String, u64)>,
) {
    let mut stack: Vec<(&NodeRow, String, u64)> = children_of
        .get(&parent_id)
        .into_iter()
        .flatten()
        .rev()
        .map(|child| (child, parent_ident.to_owned(), depth))
        .collect();
    while let Some((child, ident, level)) = stack.pop() {
        let child_ident = child.identifier();
        for grandchild in children_of.get(&child.id).into_iter().flatten().rev() {
            stack.push((grandchild, child_ident.clone(), level + 1));
        }
        nodes.push((child.clone(), ident, level));
    }
}

async fn fetch_assignees(
    db: &sea_orm::DatabaseConnection,
    ids: &[Uuid],
) -> Result<HashMap<Uuid, Vec<Value>>, Error> {
    let mut out: HashMap<Uuid, Vec<Value>> = HashMap::new();
    if ids.is_empty() {
        return Ok(out);
    }
    #[derive(Debug, FromQueryResult)]
    struct AssigneeRow {
        issue_id: Uuid,
        assignee_id: Uuid,
        display_name: String,
    }
    let rows = issue_assignees::Entity::find()
        .join(JoinType::InnerJoin, issue_assignees::Relation::Users3.def())
        .select_only()
        .columns([
            issue_assignees::Column::IssueId,
            issue_assignees::Column::AssigneeId,
        ])
        .column_as(users::Column::DisplayName, "display_name")
        .filter(issue_assignees::Column::IssueId.is_in(ids.to_vec()))
        .filter(issue_assignees::Column::DeletedAt.is_null())
        .order_by_asc(issue_assignees::Column::CreatedAt)
        .order_by_asc(issue_assignees::Column::Id)
        .into_model::<AssigneeRow>()
        .all(db)
        .await
        .map_err(|error| db_error(error, "tree-assignees"))?;
    for row in rows {
        let mut item = Map::with_capacity(2);
        item.insert("id".to_owned(), Value::String(row.assignee_id.to_string()));
        item.insert("display_name".to_owned(), Value::String(row.display_name));
        out.entry(row.issue_id)
            .or_default()
            .push(Value::Object(item));
    }
    Ok(out)
}

async fn fetch_active_runs(
    db: &sea_orm::DatabaseConnection,
    ids: &[Uuid],
) -> Result<HashMap<Uuid, Value>, Error> {
    let mut out: HashMap<Uuid, Value> = HashMap::new();
    if ids.is_empty() {
        return Ok(out);
    }
    #[derive(Debug, FromQueryResult)]
    struct RunRow {
        id: Uuid,
        status: String,
        work_item_id: Option<Uuid>,
    }
    let rows = agent_run::Entity::find()
        .select_only()
        .columns([
            agent_run::Column::Id,
            agent_run::Column::Status,
            agent_run::Column::WorkItemId,
        ])
        .filter(agent_run::Column::WorkItemId.is_in(ids.to_vec()))
        .filter(agent_run::Column::Status.is_in(ACTIVE_RUN_STATUSES))
        .order_by_desc(agent_run::Column::CreatedAt)
        .order_by_desc(agent_run::Column::Id)
        .into_model::<RunRow>()
        .all(db)
        .await
        .map_err(|error| db_error(error, "tree-active-runs"))?;
    for row in rows {
        let Some(work_item) = row.work_item_id else {
            continue;
        };
        out.entry(work_item).or_insert_with(|| {
            let mut item = Map::with_capacity(2);
            item.insert("id".to_owned(), Value::String(row.id.to_string()));
            item.insert("status".to_owned(), Value::String(row.status));
            Value::Object(item)
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(seq: i32, parent: Option<Uuid>) -> NodeRow {
        NodeRow {
            id: Uuid::new_v4(),
            parent_id: parent,
            name: format!("n{seq}"),
            priority: "none".to_owned(),
            sequence_id: seq,
            project_identifier: "T".to_owned(),
            state_name: None,
            state_group: None,
        }
    }

    #[test]
    fn walk_is_depth_first_pre_order_with_sibling_order_kept() {
        let root = node(1, None);
        let a = node(2, Some(root.id));
        let b = node(3, Some(root.id));
        let a1 = node(4, Some(a.id));
        let mut children_of: HashMap<Uuid, Vec<NodeRow>> = HashMap::new();
        children_of.insert(root.id, vec![a.clone(), b.clone()]);
        children_of.insert(a.id, vec![a1.clone()]);
        let mut nodes = Vec::new();
        walk(&children_of, root.id, &root.identifier(), 1, &mut nodes);
        let got: Vec<(String, String, u64)> = nodes
            .iter()
            .map(|(n, p, d)| (n.name.clone(), p.clone(), *d))
            .collect();
        assert_eq!(
            got,
            vec![
                ("n2".to_owned(), "T-1".to_owned(), 1),
                ("n4".to_owned(), "T-2".to_owned(), 2),
                ("n3".to_owned(), "T-1".to_owned(), 1),
            ]
        );
    }

    #[test]
    fn walk_handles_a_chain_at_the_node_limit() {
        let root = node(0, None);
        let mut children_of: HashMap<Uuid, Vec<NodeRow>> = HashMap::new();
        let mut parent = root.id;
        for seq in 1..=i32::try_from(NODE_LIMIT).unwrap() {
            let child = node(seq, Some(parent));
            parent = child.id;
            children_of.insert(child.parent_id.unwrap(), vec![child]);
        }
        let mut nodes = Vec::new();
        walk(&children_of, root.id, &root.identifier(), 1, &mut nodes);
        assert_eq!(nodes.len(), NODE_LIMIT);
        assert_eq!(nodes.last().unwrap().2, NODE_LIMIT as u64);
    }

    #[test]
    fn descendants_are_one_recursive_statement() {
        let source = Uuid::nil();
        let sql = descendants_query("ws", &Uuid::nil(), source, true)
            .to_string(sea_query::PostgresQueryBuilder);
        assert!(
            sql.starts_with(r#"WITH RECURSIVE "tree" ("id", "depth") AS "#),
            "{sql}"
        );
        assert!(sql.contains("UNION ALL"), "{sql}");
        // The visible pool gates both the seed and the step.
        assert_eq!(
            sql.matches(r#""project_members"."is_active" = TRUE"#)
                .count(),
            2,
            "{sql}"
        );
        // The source is never re-entered, depth is capped.
        assert_eq!(
            sql.matches(r#""issues"."id" <> '00000000-0000-0000-0000-000000000000'"#)
                .count(),
            2,
            "{sql}"
        );
        assert!(sql.contains(r#""tree"."depth" <= 1000"#), "{sql}");
        assert!(
            sql.ends_with(
                r#"ORDER BY "tree"."depth" ASC, "issues"."sequence_id" ASC, "issues"."id" ASC LIMIT 1001"#
            ),
            "{sql}"
        );
    }

    #[test]
    fn rollup_counts_cancelled_as_done_and_keeps_every_group() {
        let out = rollup([
            Some("backlog"),
            Some("unstarted"),
            Some("started"),
            Some("completed"),
            Some("completed"),
            Some("cancelled"),
            None,
        ]);
        assert_eq!(
            serde_json::to_string(&out).unwrap(),
            r#"{"total":7,"done":3,"open":4,"by_state_group":{"backlog":1,"unstarted":1,"started":1,"review":0,"test":0,"completed":2,"cancelled":1}}"#
        );
    }

    #[test]
    fn rollup_of_nothing_is_zeroes() {
        let out = rollup(std::iter::empty());
        assert_eq!(out["total"], 0);
        assert_eq!(out["done"], 0);
        assert_eq!(out["open"], 0);
    }

    #[test]
    fn parse_recursive_matches_the_view() {
        assert_eq!(parse_recursive(None), Some(false));
        assert_eq!(parse_recursive(Some("")), Some(false));
        assert_eq!(parse_recursive(Some("TRUE")), Some(true));
        assert_eq!(parse_recursive(Some(" 1 ")), Some(true));
        assert_eq!(parse_recursive(Some("0")), Some(false));
        assert_eq!(parse_recursive(Some("False")), Some(false));
        assert_eq!(parse_recursive(Some("maybe")), None);
        assert_eq!(parse_recursive(Some("yes")), None);
    }
}
