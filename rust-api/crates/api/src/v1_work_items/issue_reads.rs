//! The v1 core work-item list and lookup statements on sea-query
//! (`handlers_core`): the `IssueManager` scope, the `select_related`
//! joins, the four list annotations, the compiled `work_item_list_filters`
//! predicates, the ordering chain (`views/issue.py:404-437`) and the
//! inline `.get()` lookups that
//! [`pidash_services::v1_work_items::queries_core`] documents as SQL text.
//! Same statements, built with the query builder (no SQL text, no
//! `:named` splicing) and decoded by name into [`DecodedIssue`].
//!
//! One request runs the same number of statements as before: the page is
//! one `SELECT DISTINCT` with correlated subqueries, the total one
//! `COUNT`, each lookup one `SELECT`, and the filter resolution one
//! `states` read, one `labels` read and one `issues` read per parent token.

use chrono::{DateTime, NaiveDate, Utc};
use chrono_tz::Tz;
use sea_orm::sea_query::{
    Alias, CaseStatement, Condition, Expr, Func, IntoIden, JoinType, Keyword, Order, Query,
    SelectStatement, SimpleExpr, SubQueryStatement,
};
use sea_orm::{
    ColumnTrait, ConnectionTrait, DbBackend, DbErr, EntityTrait, FromQueryResult, QueryFilter,
    QueryOrder, QuerySelect,
};
use uuid::Uuid;

use pidash_compat::datetime::render_datetime_in;
use pidash_db::entity::{
    cycle_issues, file_assets, issue_assignees, issue_labels, issue_links, issues, labels,
    projects, states, users, workspaces,
};
use pidash_db::issue_filters::{FilterValue, IssueFilter};
use pidash_services::app_issues::ordering::{PRIORITY_ORDER, STATE_ORDER};
use pidash_services::read::DecodedIssue;
use pidash_services::v1_work_items::queries_core as core_queries;

// ---------------------------------------------------------------------------
// Builder helpers
// ---------------------------------------------------------------------------

fn a(name: &str) -> Alias {
    Alias::new(name)
}

/// `"table"."column"` for an aliased table.
fn col<C: IntoIden + 'static>(table: &str, column: C) -> Expr {
    Expr::col((a(table), column))
}

/// `"issues"."column"` (the base alias is the table name).
fn issue(column: issues::Column) -> Expr {
    Expr::col((issues::Entity, column))
}

fn sub(stmt: SelectStatement) -> SimpleExpr {
    SimpleExpr::SubQuery(None, Box::new(SubQueryStatement::SelectStatement(stmt)))
}

/// The `issues.*` columns every read path selects
/// (`read::work_items::ISSUE_SELECT_COLS`, F18-05 column order).
const ISSUE_COLUMNS: [issues::Column; 31] = [
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
];

/// Select [`ISSUE_COLUMNS`].
fn select_issue_columns(stmt: &mut SelectStatement) {
    for column in ISSUE_COLUMNS {
        stmt.column((issues::Entity, column));
    }
}

/// The joined extras (`ISSUE_EXTRA_COLS`): the URL parts and the state
/// group.
fn select_extras(stmt: &mut SelectStatement) {
    stmt.expr_as(
        Expr::col((projects::Entity, projects::Column::Identifier)),
        a("project_identifier"),
    )
    .expr_as(
        Expr::col((workspaces::Entity, workspaces::Column::Slug)),
        a("workspace_slug"),
    )
    .expr_as(
        Expr::col((states::Entity, states::Column::Group)),
        a("state_group"),
    );
}

/// `FROM "issues"` plus the `select_related("project", "workspace",
/// "state"[, "parent"])` joins (`core_queries::base_joins_sql`): nullable
/// FKs LEFT, non-null FKs INNER; the parent self-join is aliased
/// `parent_issue`.
fn from_issues(stmt: &mut SelectStatement, with_parent: bool) {
    stmt.from(issues::Entity)
        .join(
            JoinType::LeftJoin,
            states::Entity,
            issue(issues::Column::StateId).equals((states::Entity, states::Column::Id)),
        )
        .join(
            JoinType::InnerJoin,
            projects::Entity,
            issue(issues::Column::ProjectId).equals((projects::Entity, projects::Column::Id)),
        )
        .join(
            JoinType::InnerJoin,
            workspaces::Entity,
            issue(issues::Column::WorkspaceId).equals((workspaces::Entity, workspaces::Column::Id)),
        );
    if with_parent {
        stmt.join_as(
            JoinType::LeftJoin,
            issues::Entity,
            a("parent_issue"),
            issue(issues::Column::ParentId).equals((a("parent_issue"), issues::Column::Id)),
        );
    }
}

/// `IssueManager.get_queryset` (`db/models/issue.py:95-103`,
/// `core_queries::ISSUE_MANAGER_WHERE`) over the given aliases: live rows,
/// non-triage with Django's `NULL` guard (stateless issues are kept),
/// unarchived, unarchived project, non-draft.
fn manager_scope(issue_tbl: &str, state_tbl: &str, project_tbl: &str) -> Condition {
    let group = || col(state_tbl, states::Column::Group);
    Condition::all()
        .add(col(issue_tbl, issues::Column::DeletedAt).is_null())
        .add(group().eq("triage").and(group().is_not_null()).not())
        .add(
            col(issue_tbl, issues::Column::ArchivedAt)
                .is_not_null()
                .not(),
        )
        .add(
            col(project_tbl, projects::Column::ArchivedAt)
                .is_not_null()
                .not(),
        )
        .add(col(issue_tbl, issues::Column::IsDraft).not())
}

/// The base-alias manager scope.
fn base_manager_scope() -> Condition {
    manager_scope("issues", "states", "projects")
}

/// Project scoping of the list/detail `get_queryset`s
/// (`core_queries::project_scope_sql`).
fn project_scope(project_id: Uuid, slug: &str) -> Condition {
    Condition::all()
        .add(issue(issues::Column::ProjectId).eq(project_id))
        .add(Expr::col((workspaces::Entity, workspaces::Column::Slug)).eq(slug))
}

// ---------------------------------------------------------------------------
// List annotations (correlated scalar subqueries)
// ---------------------------------------------------------------------------

/// `sub_issues_count`: a `Count` of live child issues under the manager
/// scope, correlated on the outer row (`core_queries::sub_issues_count_sql`).
fn sub_issues_count() -> SimpleExpr {
    let mut stmt = Query::select();
    stmt.expr(Func::count(col("sub_issue", issues::Column::Id)))
        .from_as(issues::Entity, a("sub_issue"))
        .join_as(
            JoinType::LeftJoin,
            states::Entity,
            a("sub_state"),
            col("sub_issue", issues::Column::StateId).equals((a("sub_state"), states::Column::Id)),
        )
        .join_as(
            JoinType::InnerJoin,
            projects::Entity,
            a("sub_project"),
            col("sub_issue", issues::Column::ProjectId)
                .equals((a("sub_project"), projects::Column::Id)),
        )
        .cond_where(manager_scope("sub_issue", "sub_state", "sub_project").add(
            col("sub_issue", issues::Column::ParentId).equals((issues::Entity, issues::Column::Id)),
        ));
    sub(stmt)
}

/// `cycle_id`: the newest live `cycle_issues` row for this issue
/// (`core_queries::cycle_id_sql`).
fn cycle_id() -> SimpleExpr {
    let mut stmt = Query::select();
    stmt.column((a("cycle_issue"), cycle_issues::Column::CycleId))
        .from_as(cycle_issues::Entity, a("cycle_issue"))
        .and_where(col("cycle_issue", cycle_issues::Column::DeletedAt).is_null())
        .and_where(
            col("cycle_issue", cycle_issues::Column::IssueId)
                .equals((issues::Entity, issues::Column::Id)),
        )
        .order_by(
            (a("cycle_issue"), cycle_issues::Column::CreatedAt),
            Order::Desc,
        )
        .limit(1);
    sub(stmt)
}

/// `link_count`: live `issue_links` rows for this issue
/// (`core_queries::link_count_sql`).
fn link_count() -> SimpleExpr {
    let mut stmt = Query::select();
    stmt.expr(Func::count(col("issue_link", issue_links::Column::Id)))
        .from_as(issue_links::Entity, a("issue_link"))
        .and_where(col("issue_link", issue_links::Column::DeletedAt).is_null())
        .and_where(
            col("issue_link", issue_links::Column::IssueId)
                .equals((issues::Entity, issues::Column::Id)),
        );
    sub(stmt)
}

/// `attachment_count`: live `ISSUE_ATTACHMENT` file assets for this issue
/// (`core_queries::attachment_count_sql`).
fn attachment_count() -> SimpleExpr {
    let mut stmt = Query::select();
    stmt.expr(Func::count(col("file_asset", file_assets::Column::Id)))
        .from_as(file_assets::Entity, a("file_asset"))
        .and_where(col("file_asset", file_assets::Column::DeletedAt).is_null())
        .and_where(col("file_asset", file_assets::Column::EntityType).eq("ISSUE_ATTACHMENT"))
        .and_where(
            col("file_asset", file_assets::Column::IssueId)
                .equals((issues::Entity, issues::Column::Id)),
        );
    sub(stmt)
}

// ---------------------------------------------------------------------------
// Compiled filters (`core_queries::filter_joins_sql` / `filter_where_sql`)
// ---------------------------------------------------------------------------

/// A compiled filter carried a label/assignee id that is not a UUID. The
/// compiler only emits resolved ids and `is_uuid`-passing tokens, so this
/// is the handler's 500 (as before).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BadFilterId;

/// Which through-table `INNER JOIN`s the compiled predicates need.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct FilterJoins {
    assignees: bool,
    labels: bool,
}

fn filter_joins(filters: &IssueFilter) -> FilterJoins {
    let keys = filters.predicates().iter().map(|(key, _)| key.as_str());
    let mut joins = FilterJoins::default();
    for key in keys {
        if key.starts_with("issue_assignee__") {
            joins.assignees = true;
        }
        if key.starts_with("label_issue__") {
            joins.labels = true;
        }
    }
    joins
}

/// Aliased `filter_assignees` / `filter_labels` INNER JOINs: they never
/// collide with the ordering `MAX` joins.
fn add_filter_joins(stmt: &mut SelectStatement, joins: FilterJoins) {
    if joins.assignees {
        stmt.join_as(
            JoinType::InnerJoin,
            issue_assignees::Entity,
            a("filter_assignees"),
            issue(issues::Column::Id)
                .equals((a("filter_assignees"), issue_assignees::Column::IssueId)),
        );
    }
    if joins.labels {
        stmt.join_as(
            JoinType::InnerJoin,
            issue_labels::Entity,
            a("filter_labels"),
            issue(issues::Column::Id).equals((a("filter_labels"), issue_labels::Column::IssueId)),
        );
    }
}

fn parse_ids(ids: &[String]) -> Result<Vec<Uuid>, BadFilterId> {
    ids.iter()
        .map(|id| id.parse::<Uuid>().map_err(|_| BadFilterId))
        .collect()
}

/// The `WHERE` conjunction for a compiled [`IssueFilter`], predicates in
/// key order (Django sorts single-call kwargs). Empty list values and
/// unknown keys render nothing (the compiler is closed over
/// `COMPILED_PREDICATE_KEYS`). `None` when no predicate renders.
fn filter_condition(filters: &IssueFilter) -> Result<Option<Condition>, BadFilterId> {
    let mut predicates: Vec<&(String, FilterValue)> = filters.predicates().iter().collect();
    predicates.sort_by(|left, right| left.0.cmp(&right.0));
    let mut cond = Condition::all();
    let mut any = false;
    for (key, value) in predicates {
        let part: Option<SimpleExpr> = match (key.as_str(), value) {
            ("state__in", FilterValue::Uuids(ids)) if !ids.is_empty() => {
                Some(Expr::col((states::Entity, states::Column::Id)).is_in(ids.iter().copied()))
            }
            ("state__group__in", FilterValue::Strings(groups)) if !groups.is_empty() => Some(
                Expr::col((states::Entity, states::Column::Group)).is_in(groups.iter().cloned()),
            ),
            ("priority__in", FilterValue::Strings(priorities)) if !priorities.is_empty() => {
                Some(issue(issues::Column::Priority).is_in(priorities.iter().cloned()))
            }
            ("parent__isnull", FilterValue::Flag(true)) => {
                Some(issue(issues::Column::ParentId).is_null())
            }
            ("parent__isnull", FilterValue::Flag(false)) => {
                Some(issue(issues::Column::ParentId).is_not_null())
            }
            ("parent__in", FilterValue::Uuids(ids)) if !ids.is_empty() => {
                Some(issue(issues::Column::ParentId).is_in(ids.iter().copied()))
            }
            ("label_issue__label_id__in", FilterValue::Strings(ids)) if !ids.is_empty() => {
                Some(col("filter_labels", issue_labels::Column::LabelId).is_in(parse_ids(ids)?))
            }
            ("label_issue__deleted_at__isnull", FilterValue::Flag(true)) => {
                Some(col("filter_labels", issue_labels::Column::DeletedAt).is_null())
            }
            ("label_issue__deleted_at__isnull", FilterValue::Flag(false)) => {
                Some(col("filter_labels", issue_labels::Column::DeletedAt).is_not_null())
            }
            ("issue_assignee__assignee_id__in", FilterValue::Strings(ids)) if !ids.is_empty() => {
                Some(
                    col("filter_assignees", issue_assignees::Column::AssigneeId)
                        .is_in(parse_ids(ids)?),
                )
            }
            ("issue_assignee__deleted_at__isnull", FilterValue::Flag(true)) => {
                Some(col("filter_assignees", issue_assignees::Column::DeletedAt).is_null())
            }
            ("issue_assignee__deleted_at__isnull", FilterValue::Flag(false)) => {
                Some(col("filter_assignees", issue_assignees::Column::DeletedAt).is_not_null())
            }
            _ => None,
        };
        if let Some(part) = part {
            cond = cond.add(part);
            any = true;
        }
    }
    Ok(any.then_some(cond))
}

// ---------------------------------------------------------------------------
// List ordering (`views/issue.py:361-437`, `core_queries::order_spec`)
// ---------------------------------------------------------------------------

/// The `MAX` branch's two LEFT joins: the through table and its target.
#[derive(Debug, Clone)]
struct MaxJoin {
    through: &'static str,
    through_alias: &'static str,
    target_alias: &'static str,
}

/// A renderable list ordering: the optional select annotation, the
/// `MAX` joins, the `ORDER BY` expression and whether Django's
/// `SELECT DISTINCT` needs the ordered expression added to the select
/// list (`order_extra`) or a `GROUP BY` (the `MAX` branch).
#[derive(Debug, Clone)]
struct OrderClause {
    annotation: Option<(SimpleExpr, &'static str)>,
    max_join: Option<MaxJoin>,
    order_expr: SimpleExpr,
    direction: Order,
    extra_select: bool,
    requires_grouping: bool,
}

/// The `?order_by=` value needs joins Django would fan out (multi-level
/// `__` paths): the handler answers 500 (ported quirk 12).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnsupportedOrder;

fn plain(order_expr: SimpleExpr, direction: Order, extra_select: bool) -> OrderClause {
    OrderClause {
        annotation: None,
        max_join: None,
        order_expr,
        direction,
        extra_select,
        requires_grouping: false,
    }
}

/// `CASE WHEN <column> = '<v>' THEN <i> ... ELSE <otherwise> END` over a
/// static value list (the priority / state-group annotations).
fn rank_case(column: Expr, order: &[&str], otherwise: SimpleExpr) -> SimpleExpr {
    let mut case = CaseStatement::new();
    for (index, value) in order.iter().enumerate() {
        case = case.case(
            column.clone().eq(*value),
            Expr::val(i32::try_from(index).unwrap_or(i32::MAX)),
        );
    }
    case.finally(otherwise).into()
}

fn reversed<'a>(order: &[&'a str]) -> Vec<&'a str> {
    order.iter().rev().copied().collect()
}

/// A syntactically valid identifier segment (Django field path); anything
/// else is not a field path at all.
fn is_identifier_segment(segment: &str) -> bool {
    let mut chars = segment.chars();
    match chars.next() {
        Some(first) if first.is_ascii_alphabetic() || first == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Default-branch ordering (`views/issue.py:436-437`): annotation names,
/// `?` (random), bare columns/fields (an unknown bare name renders the
/// qualified identifier so the database errors like Django's
/// `FieldError`), and one-level `select_related` traversals. A traversed
/// column outside the select list is added to it (`order_extra`) unless
/// it is one of the three joined extras.
fn default_order_clause(param: &str) -> Result<OrderClause, UnsupportedOrder> {
    let (descending, column) = match param.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, param),
    };
    let direction = if descending { Order::Desc } else { Order::Asc };
    if column == "?" && !descending {
        return Ok(plain(Func::random().into(), Order::Asc, true));
    }
    if core_queries::ORDERABLE_ANNOTATIONS.contains(&column) {
        return Ok(plain(Expr::col(a(column)).into(), direction, false));
    }
    if !column.contains("__") {
        let target = core_queries::ORDERABLE_FIELD_ALIASES
            .iter()
            .find(|(name, _)| *name == column)
            .map(|(_, target)| *target)
            .unwrap_or(column);
        if !core_queries::ORDERABLE_ISSUE_COLUMNS.contains(&target)
            && !is_identifier_segment(target)
        {
            return Err(UnsupportedOrder);
        }
        return Ok(plain(col("issues", a(target)).into(), direction, false));
    }
    let mut parts = column.split("__");
    let (Some(relation), Some(field), None) = (parts.next(), parts.next(), parts.next()) else {
        return Err(UnsupportedOrder);
    };
    match core_queries::ORDERABLE_RELATIONS
        .iter()
        .find(|(name, _)| *name == relation)
    {
        Some((_, alias)) if is_identifier_segment(field) => {
            let selected = matches!(
                (*alias, field),
                ("projects", "identifier") | ("workspaces", "slug") | ("states", "group")
            );
            Ok(plain(col(alias, a(field)).into(), direction, !selected))
        }
        _ => Err(UnsupportedOrder),
    }
}

/// Port of the list ordering chain (`views/issue.py:404-437`). Every
/// branch replaces the queryset order (ported quirk 7: no tiebreak).
fn order_clause(order_param: &str) -> Result<OrderClause, UnsupportedOrder> {
    if order_param == "priority" || order_param == "-priority" {
        let order: Vec<&str> = if order_param == "priority" {
            PRIORITY_ORDER.to_vec()
        } else {
            reversed(PRIORITY_ORDER)
        };
        let case = rank_case(
            issue(issues::Column::Priority),
            &order,
            SimpleExpr::Keyword(Keyword::Null),
        );
        // Postgres matches a `SELECT DISTINCT` order expression against the
        // select list structurally; with bound `WHEN` values the two `CASE`s
        // would not match, so the order names the annotation alias (same
        // ranking, same rows).
        return Ok(OrderClause {
            annotation: Some((case, "priority_order")),
            max_join: None,
            order_expr: Expr::col(a("priority_order")).into(),
            direction: Order::Asc,
            extra_select: false,
            requires_grouping: false,
        });
    }
    if matches!(
        order_param,
        "state__name" | "state__group" | "-state__name" | "-state__group"
    ) {
        // Ported bug 1: `state__name` takes this branch too and orders by
        // group, never by name.
        let ascending = order_param == "state__name" || order_param == "state__group";
        let order: Vec<&str> = if ascending {
            STATE_ORDER.to_vec()
        } else {
            reversed(STATE_ORDER)
        };
        let otherwise = Expr::val(i32::try_from(order.len()).unwrap_or(i32::MAX));
        let case = rank_case(
            Expr::col((states::Entity, states::Column::Group)),
            &order,
            otherwise.into(),
        );
        return Ok(OrderClause {
            annotation: Some((case, "state_order")),
            max_join: None,
            order_expr: Expr::col(a("state_order")).into(),
            direction: Order::Asc,
            extra_select: false,
            requires_grouping: false,
        });
    }
    // `Max(order_by_param[1::] if startswith("-") else ...)` strips exactly
    // one leading `-` (ported quirk 8); Django inlines the aggregate into
    // ORDER BY and groups by the select list. Both joins are LEFT.
    let stripped = order_param.strip_prefix('-').unwrap_or(order_param);
    let max_target: Option<(MaxJoin, Expr)> = match stripped {
        "labels__name" => Some((
            MaxJoin {
                through: "issue_labels",
                through_alias: "order_issue_labels",
                target_alias: "order_labels",
            },
            col("order_labels", labels::Column::Name),
        )),
        "assignees__first_name" => Some((
            MaxJoin {
                through: "issue_assignees",
                through_alias: "order_issue_assignees",
                target_alias: "order_users",
            },
            col("order_users", users::Column::FirstName),
        )),
        _ => None,
    };
    if let Some((max_join, column)) = max_target {
        let direction = if order_param.starts_with('-') {
            Order::Desc
        } else {
            Order::Asc
        };
        let max: SimpleExpr = Func::max(column).into();
        return Ok(OrderClause {
            annotation: Some((max.clone(), "max_values")),
            max_join: Some(max_join),
            order_expr: max,
            direction,
            extra_select: false,
            requires_grouping: true,
        });
    }
    default_order_clause(order_param)
}

/// The `MAX` branch joins (`core_queries::MAX_ORDER_FIELDS`).
fn add_max_joins(stmt: &mut SelectStatement, max_join: &MaxJoin) {
    match max_join.through {
        "issue_labels" => {
            stmt.join_as(
                JoinType::LeftJoin,
                issue_labels::Entity,
                a(max_join.through_alias),
                issue(issues::Column::Id)
                    .equals((a(max_join.through_alias), issue_labels::Column::IssueId)),
            )
            .join_as(
                JoinType::LeftJoin,
                labels::Entity,
                a(max_join.target_alias),
                col(max_join.through_alias, issue_labels::Column::LabelId)
                    .equals((a(max_join.target_alias), labels::Column::Id)),
            );
        }
        _ => {
            stmt.join_as(
                JoinType::LeftJoin,
                issue_assignees::Entity,
                a(max_join.through_alias),
                issue(issues::Column::Id)
                    .equals((a(max_join.through_alias), issue_assignees::Column::IssueId)),
            )
            .join_as(
                JoinType::LeftJoin,
                users::Entity,
                a(max_join.target_alias),
                col(max_join.through_alias, issue_assignees::Column::AssigneeId)
                    .equals((a(max_join.target_alias), users::Column::Id)),
            );
        }
    }
}

// ---------------------------------------------------------------------------
// The list page and its total
// ---------------------------------------------------------------------------

/// Why a list statement cannot be built: both answer the handler's 500,
/// exactly where the text builder did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListError {
    UnsupportedOrder,
    BadFilterId,
    BadWindow,
}

impl From<UnsupportedOrder> for ListError {
    fn from(_: UnsupportedOrder) -> Self {
        ListError::UnsupportedOrder
    }
}

impl From<BadFilterId> for ListError {
    fn from(_: BadFilterId) -> Self {
        ListError::BadFilterId
    }
}

/// One list page (`IssueListCreateAPIEndpoint.get_queryset` + the
/// ordering chain + the paginator window): `SELECT DISTINCT` of the issue
/// columns, the four annotations, the ordering annotation, the joined
/// extras; manager scope, project scope and compiled filters; the
/// `[offset, stop)` window.
pub fn page_statement(
    slug: &str,
    project_id: Uuid,
    filters: &IssueFilter,
    order_param: &str,
    offset: i64,
    stop: i64,
) -> Result<SelectStatement, ListError> {
    let order = order_clause(order_param)?;
    let limit = u64::try_from(stop - offset).map_err(|_| ListError::BadWindow)?;
    let offset = u64::try_from(offset).map_err(|_| ListError::BadWindow)?;
    let filter_cond = filter_condition(filters)?;
    let mut stmt = Query::select();
    stmt.distinct();
    select_issue_columns(&mut stmt);
    stmt.expr_as(sub_issues_count(), a("sub_issues_count"))
        .expr_as(cycle_id(), a("cycle_id"))
        .expr_as(link_count(), a("link_count"))
        .expr_as(attachment_count(), a("attachment_count"));
    if let Some((annotation, alias)) = &order.annotation {
        stmt.expr_as(annotation.clone(), a(alias));
    }
    if order.extra_select {
        stmt.expr_as(order.order_expr.clone(), a("order_extra"));
    }
    select_extras(&mut stmt);
    from_issues(&mut stmt, true);
    add_filter_joins(&mut stmt, filter_joins(filters));
    if let Some(max_join) = &order.max_join {
        add_max_joins(&mut stmt, max_join);
    }
    let mut cond = base_manager_scope().add(project_scope(project_id, slug));
    if let Some(filter_cond) = filter_cond {
        cond = cond.add(filter_cond);
    }
    stmt.cond_where(cond);
    if order.requires_grouping {
        // Django groups by the select list; the row key plus the joined
        // extras yields the same one-group-per-issue (scalar subqueries
        // need no `GROUP BY` membership).
        stmt.group_by_columns([
            (issues::Entity.into_iden(), issues::Column::Id.into_iden()),
            (
                projects::Entity.into_iden(),
                projects::Column::Identifier.into_iden(),
            ),
            (
                workspaces::Entity.into_iden(),
                workspaces::Column::Slug.into_iden(),
            ),
            (
                states::Entity.into_iden(),
                states::Column::Group.into_iden(),
            ),
        ]);
    }
    stmt.order_by_expr(order.order_expr, order.direction)
        .limit(limit)
        .offset(offset);
    Ok(stmt)
}

/// `total_count_queryset` (`views/issue.py:397-399`,
/// `core_queries::total_count_query`): the manager scope plus the compiled
/// filters, `COUNT(DISTINCT "issues"."id")` only when filters are present
/// (ported quirk 4), `COUNT(*)` otherwise; the result column is `n`.
pub fn count_statement(
    slug: &str,
    project_id: Uuid,
    filters: &IssueFilter,
) -> Result<SelectStatement, ListError> {
    let mut stmt = Query::select();
    let count: SimpleExpr = if filters.is_empty() {
        Func::count(Expr::col(sea_orm::sea_query::Asterisk)).into()
    } else {
        Func::count_distinct(issue(issues::Column::Id)).into()
    };
    stmt.expr_as(count, a("n"));
    from_issues(&mut stmt, true);
    add_filter_joins(&mut stmt, filter_joins(filters));
    let mut cond = base_manager_scope().add(project_scope(project_id, slug));
    if !filters.is_empty() {
        if let Some(filter_cond) = filter_condition(filters)? {
            cond = cond.add(filter_cond);
        }
    }
    stmt.cond_where(cond);
    Ok(stmt)
}

// ---------------------------------------------------------------------------
// Inline `.get()` lookups
// ---------------------------------------------------------------------------

/// The single-row lookups (`core_queries::*_lookup_where`). The caller
/// applies the `.get()` cardinality.
#[derive(Debug, Clone, Copy)]
pub enum Lookup<'a> {
    /// `WorkspaceIssueAPIEndpoint.get` (`:250-260`): manager scope plus
    /// the identifier triple.
    WorkspaceGet {
        project_identifier: &'a str,
        sequence: i64,
        slug: &'a str,
    },
    /// The external-id early return (`:344-359`): plain `objects` scope
    /// (soft-delete only) on the external key pair, project and slug.
    ExternalId {
        external_id: &'a str,
        external_source: &'a str,
        project_id: Uuid,
        slug: &'a str,
    },
    /// `IssueDetailAPIEndpoint.get` (`:600-605`): manager scope plus the
    /// pk triple.
    DetailGet {
        pk: Uuid,
        project_id: Uuid,
        slug: &'a str,
    },
}

/// The lookup `SELECT`: the issue columns plus the joined extras, the
/// manager-scope joins (`states` LEFT, `projects` and `workspaces` INNER;
/// the extras need all three), `Meta.ordering` (`-created_at`).
pub fn lookup_statement(lookup: Lookup<'_>) -> SelectStatement {
    let slug_is = |slug: &str| Expr::col((workspaces::Entity, workspaces::Column::Slug)).eq(slug);
    let cond = match lookup {
        Lookup::WorkspaceGet {
            project_identifier,
            sequence,
            slug,
        } => base_manager_scope()
            .add(Expr::col((projects::Entity, projects::Column::Identifier)).eq(project_identifier))
            .add(issue(issues::Column::SequenceId).eq(sequence))
            .add(slug_is(slug)),
        Lookup::ExternalId {
            external_id,
            external_source,
            project_id,
            slug,
        } => Condition::all()
            .add(issue(issues::Column::DeletedAt).is_null())
            .add(issue(issues::Column::ExternalId).eq(external_id))
            .add(issue(issues::Column::ExternalSource).eq(external_source))
            .add(issue(issues::Column::ProjectId).eq(project_id))
            .add(slug_is(slug)),
        Lookup::DetailGet {
            pk,
            project_id,
            slug,
        } => base_manager_scope()
            .add(issue(issues::Column::Id).eq(pk))
            .add(issue(issues::Column::ProjectId).eq(project_id))
            .add(slug_is(slug)),
    };
    let mut stmt = Query::select();
    select_issue_columns(&mut stmt);
    select_extras(&mut stmt);
    from_issues(&mut stmt, false);
    stmt.cond_where(cond)
        .order_by((issues::Entity, issues::Column::CreatedAt), Order::Desc);
    stmt
}

// ---------------------------------------------------------------------------
// Filter resolution reads (`_resolve_states` / `_labels` / `_parents`)
// ---------------------------------------------------------------------------

/// `State.objects.filter(project_id=...).values_list("id", "name")`
/// (`core_queries::states_lookup_sql`): live, non-triage states in
/// `sequence` order. `exclude(group="triage")` hits a concrete non-null
/// field, so no `NULL` guard.
pub async fn filter_states<C: ConnectionTrait>(
    db: &C,
    project_id: Uuid,
) -> Result<Vec<(Uuid, String)>, DbErr> {
    states::Entity::find()
        .select_only()
        .columns([states::Column::Id, states::Column::Name])
        .filter(states::Column::DeletedAt.is_null())
        .filter(
            Expr::col((states::Entity, states::Column::Group))
                .eq("triage")
                .not(),
        )
        .filter(states::Column::ProjectId.eq(project_id))
        .order_by_asc(states::Column::Sequence)
        .into_tuple()
        .all(db)
        .await
}

/// `Label.objects.filter(Q(project_id=...) | Q(project__isnull=True,
/// workspace__slug=...)).values_list("id", "name")`
/// (`core_queries::labels_lookup_sql`): project labels plus workspace-level
/// labels, INNER `workspaces` join, newest first.
pub async fn filter_labels<C: ConnectionTrait>(
    db: &C,
    project_id: Uuid,
    slug: &str,
) -> Result<Vec<(Uuid, String)>, DbErr> {
    labels::Entity::find()
        .select_only()
        .columns([labels::Column::Id, labels::Column::Name])
        .join(
            JoinType::InnerJoin,
            labels::Entity::belongs_to(workspaces::Entity)
                .from(labels::Column::WorkspaceId)
                .to(workspaces::Column::Id)
                .into(),
        )
        .filter(labels::Column::DeletedAt.is_null())
        .filter(
            Condition::any()
                .add(labels::Column::ProjectId.eq(project_id))
                .add(
                    Condition::all()
                        .add(labels::Column::ProjectId.is_null())
                        .add(Expr::col((workspaces::Entity, workspaces::Column::Slug)).eq(slug)),
                ),
        )
        .order_by_desc(labels::Column::CreatedAt)
        .into_tuple()
        .all(db)
        .await
}

/// `Issue.issue_objects.filter(workspace__slug=..., project__identifier
/// __iexact=..., sequence_id=...).values_list("id", flat=True).first()`
/// (`core_queries::parent_lookup_sql`): full manager scope,
/// case-insensitive identifier match, newest row on ties.
pub fn parent_statement(slug: &str, project_identifier: &str, sequence: i64) -> SelectStatement {
    let identifier_text =
        Expr::col((projects::Entity, projects::Column::Identifier)).cast_as(a("text"));
    let mut stmt = Query::select();
    stmt.column((issues::Entity, issues::Column::Id));
    from_issues(&mut stmt, false);
    stmt.cond_where(
        base_manager_scope()
            .add(
                Expr::expr(Func::upper(identifier_text))
                    .eq(Func::upper(Expr::val(project_identifier))),
            )
            .add(issue(issues::Column::SequenceId).eq(sequence))
            .add(Expr::col((workspaces::Entity, workspaces::Column::Slug)).eq(slug)),
    )
    .order_by((issues::Entity, issues::Column::CreatedAt), Order::Desc)
    .limit(1);
    stmt
}

/// Run [`parent_statement`].
pub async fn resolve_parent<C: ConnectionTrait>(
    db: &C,
    slug: &str,
    project_identifier: &str,
    sequence: i64,
) -> Result<Option<Uuid>, DbErr> {
    let stmt = parent_statement(slug, project_identifier, sequence);
    let row = db.query_one(DbBackend::Postgres.build(&stmt)).await?;
    row.map(|row| row.try_get::<Uuid>("", "id")).transpose()
}

// ---------------------------------------------------------------------------
// Execution and decode
// ---------------------------------------------------------------------------

/// One list/lookup row, decoded by name ([`ISSUE_COLUMNS`] plus the
/// extras) with the database types.
#[derive(Debug, Clone, FromQueryResult)]
struct IssueRow {
    id: Uuid,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    deleted_at: Option<DateTime<Utc>>,
    point: Option<i32>,
    name: String,
    description_html: String,
    description_binary: Option<Vec<u8>>,
    priority: String,
    complexity_score: i32,
    start_date: Option<NaiveDate>,
    target_date: Option<NaiveDate>,
    sequence_id: i32,
    sort_order: f64,
    completed_at: Option<DateTime<Utc>>,
    archived_at: Option<NaiveDate>,
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
    project_identifier: String,
    workspace_slug: String,
    state_group: Option<String>,
}

impl IssueRow {
    /// The [`DecodedIssue`] the renderers read: datetimes rendered as DRF
    /// strings in the request zone (`read::work_items::decode_issue`).
    fn into_decoded(self, tz: &Tz) -> DecodedIssue {
        let render = |dt: &DateTime<Utc>| render_datetime_in(dt, tz);
        DecodedIssue {
            id: self.id.to_string(),
            created_at: render(&self.created_at),
            created_at_dt: self.created_at,
            updated_at: render(&self.updated_at),
            deleted_at: self.deleted_at.as_ref().map(render),
            point: self.point.map(i64::from),
            name: self.name,
            description_html: self.description_html,
            description_binary: self.description_binary,
            priority: self.priority,
            complexity_score: i64::from(self.complexity_score),
            start_date: self.start_date.map(|d| d.to_string()),
            target_date: self.target_date.map(|d| d.to_string()),
            sequence_id: i64::from(self.sequence_id),
            sort_order: self.sort_order,
            completed_at: self.completed_at.as_ref().map(render),
            archived_at: self.archived_at.map(|d| d.to_string()),
            is_draft: self.is_draft,
            external_source: self.external_source,
            external_id: self.external_id,
            git_work_branch: self.git_work_branch,
            created_via: self.created_via,
            agent_executor: self.agent_executor,
            created_by: self.created_by_id.map(|id| id.to_string()),
            updated_by: self.updated_by_id.map(|id| id.to_string()),
            project: self.project_id.to_string(),
            workspace: self.workspace_id.to_string(),
            parent: self.parent_id.map(|id| id.to_string()),
            state: self.state_id.map(|id| id.to_string()),
            estimate_point: self.estimate_point_id.map(|id| id.to_string()),
            type_id: self.type_id.map(|id| id.to_string()),
            assigned_pod: self.assigned_pod_id.map(|id| id.to_string()),
            project_identifier: self.project_identifier,
            workspace_slug: self.workspace_slug,
            state_group: self.state_group,
        }
    }
}

/// Run a page / lookup statement into decoded issues.
pub async fn fetch_issues<C: ConnectionTrait>(
    db: &C,
    stmt: &SelectStatement,
    tz: &Tz,
) -> Result<Vec<DecodedIssue>, DbErr> {
    let rows = db.query_all(DbBackend::Postgres.build(stmt)).await?;
    rows.iter()
        .map(|row| IssueRow::from_query_result(row, "").map(|row| row.into_decoded(tz)))
        .collect()
}

/// Run [`count_statement`]: its single `n`.
pub async fn fetch_count<C: ConnectionTrait>(db: &C, stmt: &SelectStatement) -> Result<i64, DbErr> {
    let row = db.query_one(DbBackend::Postgres.build(stmt)).await?;
    row.map(|row| row.try_get::<i64>("", "n"))
        .transpose()
        .map(|n| n.unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::sea_query::PostgresQueryBuilder;

    fn sql(stmt: &SelectStatement) -> String {
        stmt.to_string(PostgresQueryBuilder)
    }

    fn filter_set(pairs: &[(&str, FilterValue)]) -> IssueFilter {
        let mut filters = IssueFilter::default();
        for (key, value) in pairs {
            filters.set((*key).to_owned(), value.clone());
        }
        filters
    }

    const SLUG: &str = "acme";

    #[test]
    fn page_default_order_has_scope_joins_annotations_and_window() {
        let project = Uuid::nil();
        let stmt = page_statement(
            SLUG,
            project,
            &IssueFilter::default(),
            "-created_at",
            10,
            35,
        )
        .expect("ordered");
        let text = sql(&stmt);
        assert!(
            text.starts_with(r#"SELECT DISTINCT "issues"."id", "issues"."created_at""#),
            "{text}"
        );
        for alias in [
            "sub_issues_count",
            "cycle_id",
            "link_count",
            "attachment_count",
        ] {
            assert!(
                text.contains(&format!(r#" AS "{alias}""#)),
                "{alias}: {text}"
            );
        }
        assert!(text.contains(r#""projects"."identifier" AS "project_identifier", "workspaces"."slug" AS "workspace_slug", "states"."group" AS "state_group" FROM "issues""#), "{text}");
        assert!(text.contains(r#"LEFT JOIN "states" ON "issues"."state_id" = "states"."id" INNER JOIN "projects" ON "issues"."project_id" = "projects"."id" INNER JOIN "workspaces" ON "issues"."workspace_id" = "workspaces"."id" LEFT JOIN "issues" AS "parent_issue" ON "issues"."parent_id" = "parent_issue"."id" WHERE"#), "{text}");
        assert!(text.contains(r#""issues"."deleted_at" IS NULL AND (NOT ("states"."group" = 'triage' AND "states"."group" IS NOT NULL)) AND (NOT "issues"."archived_at" IS NOT NULL) AND (NOT "projects"."archived_at" IS NOT NULL) AND (NOT "issues"."is_draft")"#), "{text}");
        assert!(
            text.contains(&format!(
                r#""issues"."project_id" = '{project}' AND "workspaces"."slug" = 'acme'"#
            )),
            "{text}"
        );
        assert!(
            text.ends_with(r#"ORDER BY "issues"."created_at" DESC LIMIT 25 OFFSET 10"#),
            "{text}"
        );
        assert!(!text.contains("GROUP BY"), "{text}");
        assert!(!text.contains("order_extra"), "{text}");
        // The correlated sub-issue count carries its own manager scope.
        assert!(text.contains(r#"(SELECT COUNT("sub_issue"."id") FROM "issues" AS "sub_issue" LEFT JOIN "states" AS "sub_state" ON "sub_issue"."state_id" = "sub_state"."id" INNER JOIN "projects" AS "sub_project" ON "sub_issue"."project_id" = "sub_project"."id" WHERE "sub_issue"."deleted_at" IS NULL"#), "{text}");
        assert!(
            text.contains(r#""sub_issue"."parent_id" = "issues"."id") AS "sub_issues_count""#),
            "{text}"
        );
        assert!(text.contains(r#"(SELECT "cycle_issue"."cycle_id" FROM "cycle_issues" AS "cycle_issue" WHERE "cycle_issue"."deleted_at" IS NULL AND "cycle_issue"."issue_id" = "issues"."id" ORDER BY "cycle_issue"."created_at" DESC LIMIT 1) AS "cycle_id""#), "{text}");
        assert!(
            text.contains(r#""file_asset"."entity_type" = 'ISSUE_ATTACHMENT'"#),
            "{text}"
        );
    }

    #[test]
    fn page_filters_join_through_tables_and_render_key_sorted_predicates() {
        let state = Uuid::from_u128(1);
        let label = Uuid::from_u128(2);
        let assignee = Uuid::from_u128(3);
        let filters = filter_set(&[
            ("state__in", FilterValue::Uuids(vec![state])),
            (
                "label_issue__label_id__in",
                FilterValue::Strings(vec![label.to_string()]),
            ),
            ("label_issue__deleted_at__isnull", FilterValue::Flag(true)),
            (
                "issue_assignee__assignee_id__in",
                FilterValue::Strings(vec![assignee.to_string()]),
            ),
            (
                "issue_assignee__deleted_at__isnull",
                FilterValue::Flag(true),
            ),
            ("priority__in", FilterValue::Strings(vec!["urgent".into()])),
            ("parent__isnull", FilterValue::Flag(false)),
            ("state__group__in", FilterValue::Strings(vec![])),
        ]);
        let stmt =
            page_statement(SLUG, Uuid::nil(), &filters, "-created_at", 0, 1000).expect("ordered");
        let text = sql(&stmt);
        assert!(text.contains(r#"INNER JOIN "issue_assignees" AS "filter_assignees" ON "issues"."id" = "filter_assignees"."issue_id" INNER JOIN "issue_labels" AS "filter_labels" ON "issues"."id" = "filter_labels"."issue_id""#), "{text}");
        let predicates = format!(
            r#""filter_assignees"."assignee_id" IN ('{assignee}') AND "filter_assignees"."deleted_at" IS NULL AND "filter_labels"."deleted_at" IS NULL AND "filter_labels"."label_id" IN ('{label}') AND "issues"."parent_id" IS NOT NULL AND "issues"."priority" IN ('urgent') AND "states"."id" IN ('{state}')"#
        );
        assert!(text.contains(&predicates), "{text}");
        // The empty list renders nothing (the compiler never emits one).
        assert!(!text.contains(r#""states"."group" IN"#), "{text}");
    }

    #[test]
    fn page_rejects_a_non_uuid_label_id() {
        let filters = filter_set(&[(
            "label_issue__label_id__in",
            FilterValue::Strings(vec!["nope".into()]),
        )]);
        assert_eq!(
            page_statement(SLUG, Uuid::nil(), &filters, "-created_at", 0, 10).err(),
            Some(ListError::BadFilterId)
        );
        assert_eq!(
            count_statement(SLUG, Uuid::nil(), &filters).err(),
            Some(ListError::BadFilterId)
        );
    }

    #[test]
    fn priority_and_state_orders_annotate_a_case() {
        let page = |param: &str| {
            sql(
                &page_statement(SLUG, Uuid::nil(), &IssueFilter::default(), param, 0, 10)
                    .expect("ordered"),
            )
        };
        let text = page("-priority");
        assert!(
            text.contains(r#"(CASE WHEN ("issues"."priority" = 'none') THEN 0 WHEN ("issues"."priority" = 'low') THEN 1 WHEN ("issues"."priority" = 'medium') THEN 2 WHEN ("issues"."priority" = 'high') THEN 3 WHEN ("issues"."priority" = 'urgent') THEN 4 ELSE NULL END) AS "priority_order""#),
            "{text}"
        );
        assert!(
            text.ends_with(r#"ORDER BY "priority_order" ASC LIMIT 10 OFFSET 0"#),
            "{text}"
        );
        let text = page("priority");
        assert!(
            text.contains(r#"WHEN ("issues"."priority" = 'urgent') THEN 0"#),
            "{text}"
        );
        let text = page("state__name");
        assert!(
            text.contains(r#"WHEN ("states"."group" = 'backlog') THEN 0"#),
            "{text}"
        );
        assert!(
            text.contains(&format!(
                r#"ELSE {} END) AS "state_order""#,
                STATE_ORDER.len()
            )),
            "{text}"
        );
        assert!(
            text.ends_with(r#"ORDER BY "state_order" ASC LIMIT 10 OFFSET 0"#),
            "{text}"
        );
        let desc = page("-state__group");
        let last = STATE_ORDER[STATE_ORDER.len() - 1];
        assert!(
            desc.contains(&format!(r#"WHEN ("states"."group" = '{last}') THEN 0"#)),
            "{desc}"
        );
    }

    #[test]
    fn max_order_left_joins_and_groups() {
        let text = sql(&page_statement(
            SLUG,
            Uuid::nil(),
            &IssueFilter::default(),
            "-labels__name",
            0,
            10,
        )
        .expect("ordered"));
        assert!(
            text.contains(r#"MAX("order_labels"."name") AS "max_values""#),
            "{text}"
        );
        assert!(text.contains(r#"LEFT JOIN "issue_labels" AS "order_issue_labels" ON "issues"."id" = "order_issue_labels"."issue_id" LEFT JOIN "labels" AS "order_labels" ON "order_issue_labels"."label_id" = "order_labels"."id""#), "{text}");
        assert!(text.contains(r#"GROUP BY "issues"."id", "projects"."identifier", "workspaces"."slug", "states"."group" ORDER BY MAX("order_labels"."name") DESC LIMIT 10"#), "{text}");
        let text = sql(&page_statement(
            SLUG,
            Uuid::nil(),
            &IssueFilter::default(),
            "assignees__first_name",
            0,
            10,
        )
        .expect("ordered"));
        assert!(text.contains(r#"LEFT JOIN "users" AS "order_users" ON "order_issue_assignees"."assignee_id" = "order_users"."id""#), "{text}");
        assert!(
            text.contains(r#"ORDER BY MAX("order_users"."first_name") ASC"#),
            "{text}"
        );
        // Exactly one `-` is stripped: `--labels__name` becomes the relation
        // path `-labels__name`, which Django would fan out (unsupported).
        assert_eq!(
            page_statement(
                SLUG,
                Uuid::nil(),
                &IssueFilter::default(),
                "--labels__name",
                0,
                10
            )
            .err(),
            Some(ListError::UnsupportedOrder)
        );
    }

    #[test]
    fn default_order_branches() {
        let page =
            |param: &str| page_statement(SLUG, Uuid::nil(), &IssueFilter::default(), param, 0, 10);
        // Annotation names order by alias, no extra select.
        let text = sql(&page("sub_issues_count").expect("ordered"));
        assert!(
            text.ends_with(r#"ORDER BY "sub_issues_count" ASC LIMIT 10 OFFSET 0"#),
            "{text}"
        );
        assert!(!text.contains("order_extra"), "{text}");
        // Field aliases map to their columns.
        let text = sql(&page("-state").expect("ordered"));
        assert!(
            text.ends_with(r#"ORDER BY "issues"."state_id" DESC LIMIT 10 OFFSET 0"#),
            "{text}"
        );
        // Unknown bare identifiers render qualified (the database errors).
        let text = sql(&page("bogus_col").expect("ordered"));
        assert!(
            text.ends_with(r#"ORDER BY "issues"."bogus_col" ASC LIMIT 10 OFFSET 0"#),
            "{text}"
        );
        // Random keeps Django's `ASC` and is added to the DISTINCT select.
        let text = sql(&page("?").expect("ordered"));
        assert!(text.contains(r#"RANDOM() AS "order_extra""#), "{text}");
        assert!(
            text.ends_with(r#"ORDER BY RANDOM() ASC LIMIT 10 OFFSET 0"#),
            "{text}"
        );
        // One-level relations: unselected columns join the select list.
        let text = sql(&page("-parent__name").expect("ordered"));
        assert!(
            text.contains(r#""parent_issue"."name" AS "order_extra""#),
            "{text}"
        );
        assert!(
            text.ends_with(r#"ORDER BY "parent_issue"."name" DESC LIMIT 10 OFFSET 0"#),
            "{text}"
        );
        let text = sql(&page("project__identifier").expect("ordered"));
        assert!(!text.contains("order_extra"), "{text}");
        assert!(
            text.ends_with(r#"ORDER BY "projects"."identifier" ASC LIMIT 10 OFFSET 0"#),
            "{text}"
        );
        // Unsupported: `-?`, non-identifiers, deeper paths, unknown relations.
        for param in ["-?", "1abc", "state__name__x", "labels__color", "a__b__c"] {
            assert_eq!(
                page(param).err(),
                Some(ListError::UnsupportedOrder),
                "{param}"
            );
        }
        assert_eq!(
            page_statement(
                SLUG,
                Uuid::nil(),
                &IssueFilter::default(),
                "-created_at",
                10,
                5
            )
            .err(),
            Some(ListError::BadWindow)
        );
    }

    #[test]
    fn count_is_plain_without_filters_and_distinct_with() {
        let project = Uuid::from_u128(7);
        let text = sql(&count_statement(SLUG, project, &IssueFilter::default()).expect("count"));
        assert!(
            text.starts_with(r#"SELECT COUNT(*) AS "n" FROM "issues" LEFT JOIN "states""#),
            "{text}"
        );
        assert!(
            text.contains(r#"LEFT JOIN "issues" AS "parent_issue""#),
            "{text}"
        );
        assert!(
            text.contains(&format!(
                r#""issues"."project_id" = '{project}' AND "workspaces"."slug" = 'acme'"#
            )),
            "{text}"
        );
        let filters = filter_set(&[("parent__isnull", FilterValue::Flag(true))]);
        let text = sql(&count_statement(SLUG, project, &filters).expect("count"));
        assert!(
            text.starts_with(r#"SELECT COUNT(DISTINCT "issues"."id") AS "n" FROM "issues""#),
            "{text}"
        );
        assert!(text.contains(r#""issues"."parent_id" IS NULL"#), "{text}");
        // A non-empty filter whose predicates all skip still counts DISTINCT.
        let filters = filter_set(&[("state__in", FilterValue::Uuids(vec![]))]);
        let text = sql(&count_statement(SLUG, project, &filters).expect("count"));
        assert!(
            text.starts_with(r#"SELECT COUNT(DISTINCT "issues"."id")"#),
            "{text}"
        );
        assert!(!text.contains(r#""states"."id" IN ("#), "{text}");
    }

    #[test]
    fn lookups_join_the_extras_and_keep_meta_ordering() {
        let project = Uuid::from_u128(1);
        let pk = Uuid::from_u128(2);
        let text = sql(&lookup_statement(Lookup::WorkspaceGet {
            project_identifier: "ACME",
            sequence: 42,
            slug: SLUG,
        }));
        assert!(text.contains(r#"FROM "issues" LEFT JOIN "states" ON "issues"."state_id" = "states"."id" INNER JOIN "projects" ON "issues"."project_id" = "projects"."id" INNER JOIN "workspaces" ON "issues"."workspace_id" = "workspaces"."id" WHERE "issues"."deleted_at" IS NULL AND (NOT ("states"."group" = 'triage'"#), "{text}");
        assert!(!text.contains("parent_issue"), "{text}");
        assert!(text.ends_with(r#""projects"."identifier" = 'ACME' AND "issues"."sequence_id" = 42 AND "workspaces"."slug" = 'acme' ORDER BY "issues"."created_at" DESC"#), "{text}");
        let text = sql(&lookup_statement(Lookup::ExternalId {
            external_id: "ext-1",
            external_source: "github",
            project_id: project,
            slug: SLUG,
        }));
        assert!(text.contains(&format!(r#"WHERE "issues"."deleted_at" IS NULL AND "issues"."external_id" = 'ext-1' AND "issues"."external_source" = 'github' AND "issues"."project_id" = '{project}' AND "workspaces"."slug" = 'acme' ORDER BY"#)), "{text}");
        assert!(!text.contains("triage"), "{text}");
        let text = sql(&lookup_statement(Lookup::DetailGet {
            pk,
            project_id: project,
            slug: SLUG,
        }));
        assert!(text.contains(r#"(NOT "issues"."is_draft")"#), "{text}");
        assert!(text.ends_with(&format!(r#""issues"."id" = '{pk}' AND "issues"."project_id" = '{project}' AND "workspaces"."slug" = 'acme' ORDER BY "issues"."created_at" DESC"#)), "{text}");
    }

    #[test]
    fn parent_lookup_is_case_insensitive_newest_first() {
        let text = sql(&parent_statement(SLUG, "acme", 7));
        assert!(
            text.starts_with(r#"SELECT "issues"."id" FROM "issues" LEFT JOIN "states""#),
            "{text}"
        );
        assert!(text.contains(r#"UPPER(CAST("projects"."identifier" AS text)) = UPPER('acme') AND "issues"."sequence_id" = 7 AND "workspaces"."slug" = 'acme' ORDER BY "issues"."created_at" DESC LIMIT 1"#), "{text}");
    }

    /// The whole engine against constraint-free temp copies on one connection.
    #[tokio::test]
    async fn live_issue_reads() {
        let Ok(url) = std::env::var("DATABASE_URL") else {
            eprintln!("skipping live-db test: DATABASE_URL is not set");
            return;
        };
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .expect("connect");
        for table in [
            "issues",
            "states",
            "projects",
            "workspaces",
            "cycle_issues",
            "issue_links",
            "file_assets",
            "issue_assignees",
            "issue_labels",
            "labels",
            "users",
        ] {
            sqlx::query(&format!(
                "CREATE TEMP TABLE {table} AS SELECT * FROM public.{table} WITH NO DATA"
            ))
            .execute(&pool)
            .await
            .expect("temp table");
        }
        // The `MAX` branch groups by the row key and selects the other
        // issue columns through the primary-key functional dependency.
        sqlx::query("ALTER TABLE issues ADD PRIMARY KEY (id)")
            .execute(&pool)
            .await
            .expect("issues pk");
        let ws = Uuid::new_v4();
        let project = Uuid::new_v4();
        let other_project = Uuid::new_v4();
        let (todo, triage, done) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        let (parent, child, triaged, archived, foreign, dead) = (
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
        );
        let (label, ws_label, dead_label, zeta) = (
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
        );
        let (alice, bob) = (Uuid::new_v4(), Uuid::new_v4());
        let cycle = Uuid::new_v4();
        let issue_values = |id: Uuid,
                            project_id: Uuid,
                            name: &str,
                            seq: i32,
                            state: Option<Uuid>,
                            parent_id: Option<Uuid>,
                            created: &str,
                            priority: &str| {
            let state = state.map_or("NULL".to_owned(), |s| format!("'{s}'"));
            let parent_id = parent_id.map_or("NULL".to_owned(), |p| format!("'{p}'"));
            format!(
                "('{id}', '{created}', '{created}', NULL, NULL, '{name}', '<p>{name}</p>', NULL, \
                 '{priority}', 0, NULL, '2026-01-31', {seq}, 65535, NULL, NULL, false, \
                 NULL, NULL, '', NULL, NULL, '{alice}', NULL, '{project_id}', '{ws}', {parent_id}, {state}, \
                 NULL, NULL, NULL)"
            )
        };
        let issue_cols = "(id, created_at, updated_at, deleted_at, point, name, description_html, \
                          description_binary, priority, complexity_score, start_date, target_date, \
                          sequence_id, sort_order, completed_at, archived_at, is_draft, external_source, \
                          external_id, git_work_branch, created_via, agent_executor, created_by_id, \
                          updated_by_id, project_id, workspace_id, parent_id, state_id, \
                          estimate_point_id, type_id, assigned_pod_id";
        let rows = [
            issue_values(
                parent,
                project,
                "Parent",
                1,
                Some(todo),
                None,
                "2026-01-01T00:00:00Z",
                "high",
            ),
            issue_values(
                child,
                project,
                "Child",
                2,
                Some(done),
                Some(parent),
                "2026-01-02T00:00:00.123456Z",
                "urgent",
            ),
            issue_values(
                triaged,
                project,
                "Triaged",
                3,
                Some(triage),
                Some(parent),
                "2026-01-03T00:00:00Z",
                "none",
            ),
            issue_values(
                archived,
                project,
                "Archived",
                4,
                Some(todo),
                None,
                "2026-01-04T00:00:00Z",
                "none",
            ),
            issue_values(
                foreign,
                other_project,
                "Foreign",
                5,
                Some(todo),
                None,
                "2026-01-05T00:00:00Z",
                "none",
            ),
            issue_values(
                dead,
                project,
                "Dead",
                6,
                Some(todo),
                None,
                "2026-01-06T00:00:00Z",
                "none",
            ),
        ];
        for sql in [
            format!("INSERT INTO workspaces (id, slug, name) VALUES ('{ws}', 'acme', 'Acme')"),
            format!(
                "INSERT INTO projects (id, identifier, name, workspace_id, archived_at) VALUES \
                 ('{project}', 'ACME', 'Acme', '{ws}', NULL), ('{other_project}', 'OTH', 'Other', '{ws}', NULL)"
            ),
            format!(
                "INSERT INTO states (id, name, \"group\", project_id, workspace_id, sequence, deleted_at) VALUES \
                 ('{todo}', 'Todo', 'unstarted', '{project}', '{ws}', 20000, NULL), \
                 ('{triage}', 'Triage', 'triage', '{project}', '{ws}', 10000, NULL), \
                 ('{done}', 'Done', 'completed', '{project}', '{ws}', 30000, NULL)"
            ),
            format!("INSERT INTO issues {issue_cols}) VALUES {}", rows.join(", ")),
            format!("UPDATE issues SET external_source = 'github', external_id = 'ext-1' WHERE id = '{parent}'"),
            format!("UPDATE issues SET archived_at = '2026-02-01' WHERE id = '{archived}'"),
            format!("UPDATE issues SET deleted_at = now() WHERE id = '{dead}'"),
            format!(
                "INSERT INTO labels (id, name, project_id, workspace_id, created_at, deleted_at) VALUES \
                 ('{label}', 'bug', '{project}', '{ws}', '2026-01-01T00:00:00Z', NULL), \
                 ('{ws_label}', 'global', NULL, '{ws}', '2026-01-02T00:00:00Z', NULL), \
                 ('{dead_label}', 'gone', '{project}', '{ws}', '2026-01-03T00:00:00Z', now()), \
                 ('{zeta}', 'zeta', '{project}', '{ws}', '2025-12-31T00:00:00Z', NULL)"
            ),
            format!(
                "INSERT INTO issue_labels (id, issue_id, label_id, deleted_at) VALUES \
                 (gen_random_uuid(), '{child}', '{label}', NULL), (gen_random_uuid(), '{parent}', '{label}', now()), \
                 (gen_random_uuid(), '{child}', '{zeta}', NULL)"
            ),
            format!("INSERT INTO users (id, first_name) VALUES ('{alice}', 'Alice'), ('{bob}', 'Bob')"),
            format!(
                "INSERT INTO issue_assignees (id, issue_id, assignee_id, deleted_at) VALUES \
                 (gen_random_uuid(), '{parent}', '{alice}', NULL), (gen_random_uuid(), '{parent}', '{bob}', NULL), \
                 (gen_random_uuid(), '{child}', '{alice}', NULL)"
            ),
            format!(
                "INSERT INTO cycle_issues (id, issue_id, cycle_id, created_at, deleted_at) VALUES \
                 (gen_random_uuid(), '{parent}', '{cycle}', '2026-01-02T00:00:00Z', NULL), \
                 (gen_random_uuid(), '{parent}', gen_random_uuid(), '2026-01-01T00:00:00Z', NULL), \
                 (gen_random_uuid(), '{child}', gen_random_uuid(), '2026-01-01T00:00:00Z', now())"
            ),
            format!(
                "INSERT INTO issue_links (id, issue_id, deleted_at) VALUES \
                 (gen_random_uuid(), '{parent}', NULL), (gen_random_uuid(), '{parent}', now())"
            ),
            format!(
                "INSERT INTO file_assets (id, issue_id, entity_type, deleted_at) VALUES \
                 (gen_random_uuid(), '{parent}', 'ISSUE_ATTACHMENT', NULL), \
                 (gen_random_uuid(), '{parent}', 'ISSUE_DESCRIPTION', NULL)"
            ),
        ] {
            sqlx::query(&sql).execute(&pool).await.expect(&sql);
        }
        let db = pidash_db::pool::sea_orm_connection(&pool);
        let tz: Tz = "Asia/Tokyo".parse().expect("tz");

        // Default page: manager scope keeps parent + child only (triage,
        // archived, foreign and deleted rows drop), newest first.
        let page = page_statement(SLUG, project, &IssueFilter::default(), "-created_at", 0, 10)
            .expect("page");
        let rows = fetch_issues(&db, &page, &tz).await.expect("rows");
        assert_eq!(
            rows.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
            ["Child", "Parent"]
        );
        let first = &rows[0];
        assert_eq!(first.id, child.to_string());
        assert_eq!(first.created_at, "2026-01-02T09:00:00.123456+09:00");
        assert_eq!(first.updated_at, "2026-01-02T09:00:00.123456+09:00");
        assert_eq!(first.parent.as_deref(), Some(parent.to_string()).as_deref());
        assert_eq!(first.state_group.as_deref(), Some("completed"));
        assert_eq!(
            (
                first.project_identifier.as_str(),
                first.workspace_slug.as_str()
            ),
            ("ACME", "acme")
        );
        assert_eq!(
            (first.sequence_id, first.sort_order, first.is_draft),
            (2, 65535.0, false)
        );
        assert_eq!(first.target_date.as_deref(), Some("2026-01-31"));
        assert_eq!(
            first.created_by.as_deref(),
            Some(alice.to_string()).as_deref()
        );
        assert_eq!(rows[1].created_at, "2026-01-01T09:00:00+09:00");
        assert_eq!(rows[1].external_id.as_deref(), Some("ext-1"));
        let total = fetch_count(
            &db,
            &count_statement(SLUG, project, &IssueFilter::default()).expect("count"),
        )
        .await
        .expect("n");
        assert_eq!(total, 2);

        // Window and the raw annotations on the parent row.
        let page = page_statement(SLUG, project, &IssueFilter::default(), "created_at", 1, 2)
            .expect("page");
        let rows = fetch_issues(&db, &page, &tz).await.expect("rows");
        assert_eq!(
            rows.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
            ["Child"]
        );
        let page = page_statement(SLUG, project, &IssueFilter::default(), "created_at", 0, 1)
            .expect("page");
        let annotated = db
            .query_one(DbBackend::Postgres.build(&page))
            .await
            .expect("row")
            .expect("one");
        assert_eq!(
            annotated
                .try_get::<i64>("", "sub_issues_count")
                .expect("count"),
            1
        );
        assert_eq!(
            annotated
                .try_get::<Option<Uuid>>("", "cycle_id")
                .expect("cycle"),
            Some(cycle)
        );
        assert_eq!(
            annotated.try_get::<i64>("", "link_count").expect("links"),
            1
        );
        assert_eq!(
            annotated
                .try_get::<i64>("", "attachment_count")
                .expect("attachments"),
            1
        );

        // Filters: a live label narrows to the child; the assignee filter
        // with the DISTINCT page never duplicates the parent (two assignees).
        let by_label = filter_set(&[
            (
                "label_issue__label_id__in",
                FilterValue::Strings(vec![label.to_string()]),
            ),
            ("label_issue__deleted_at__isnull", FilterValue::Flag(true)),
        ]);
        let page = page_statement(SLUG, project, &by_label, "-created_at", 0, 10).expect("page");
        let rows = fetch_issues(&db, &page, &tz).await.expect("rows");
        assert_eq!(
            rows.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
            ["Child"]
        );
        assert_eq!(
            fetch_count(
                &db,
                &count_statement(SLUG, project, &by_label).expect("count")
            )
            .await
            .expect("n"),
            1
        );
        let by_assignee = filter_set(&[
            (
                "issue_assignee__assignee_id__in",
                FilterValue::Strings(vec![alice.to_string(), bob.to_string()]),
            ),
            (
                "issue_assignee__deleted_at__isnull",
                FilterValue::Flag(true),
            ),
        ]);
        let page = page_statement(SLUG, project, &by_assignee, "-created_at", 0, 10).expect("page");
        let rows = fetch_issues(&db, &page, &tz).await.expect("rows");
        assert_eq!(
            rows.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
            ["Child", "Parent"]
        );
        assert_eq!(
            fetch_count(
                &db,
                &count_statement(SLUG, project, &by_assignee).expect("count")
            )
            .await
            .expect("n"),
            2
        );
        let roots = filter_set(&[
            ("parent__isnull", FilterValue::Flag(true)),
            ("priority__in", FilterValue::Strings(vec!["high".into()])),
        ]);
        let page = page_statement(SLUG, project, &roots, "-created_at", 0, 10).expect("page");
        assert_eq!(fetch_issues(&db, &page, &tz).await.expect("rows").len(), 1);

        // Ordering branches run: priority, state group, MAX label/assignee, random, relation.
        for (param, expected) in [
            ("priority", ["Child", "Parent"]),
            ("-priority", ["Parent", "Child"]),
            ("state__group", ["Parent", "Child"]),
            ("-state__group", ["Child", "Parent"]),
            ("labels__name", ["Parent", "Child"]),
            ("-labels__name", ["Child", "Parent"]),
            ("-assignees__first_name", ["Parent", "Child"]),
            ("assignees__first_name", ["Child", "Parent"]),
            ("-sequence_id", ["Child", "Parent"]),
            ("parent__name", ["Child", "Parent"]),
            ("-sub_issues_count", ["Parent", "Child"]),
        ] {
            let page =
                page_statement(SLUG, project, &IssueFilter::default(), param, 0, 10).expect(param);
            let rows = fetch_issues(&db, &page, &tz).await.expect(param);
            assert_eq!(
                rows.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
                expected,
                "{param}"
            );
        }
        let random =
            page_statement(SLUG, project, &IssueFilter::default(), "?", 0, 10).expect("random");
        assert_eq!(
            fetch_issues(&db, &random, &tz).await.expect("random").len(),
            2
        );
        let bogus = page_statement(SLUG, project, &IssueFilter::default(), "bogus_col", 0, 10)
            .expect("bogus");
        assert!(fetch_issues(&db, &bogus, &tz).await.is_err());

        // Lookups.
        let rows = fetch_issues(
            &db,
            &lookup_statement(Lookup::WorkspaceGet {
                project_identifier: "ACME",
                sequence: 2,
                slug: SLUG,
            }),
            &tz,
        )
        .await
        .expect("ws get");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, child.to_string());
        let rows = fetch_issues(
            &db,
            &lookup_statement(Lookup::WorkspaceGet {
                project_identifier: "ACME",
                sequence: 3,
                slug: SLUG,
            }),
            &tz,
        )
        .await
        .expect("ws get triaged");
        assert!(rows.is_empty());
        let rows = fetch_issues(
            &db,
            &lookup_statement(Lookup::ExternalId {
                external_id: "ext-1",
                external_source: "github",
                project_id: project,
                slug: SLUG,
            }),
            &tz,
        )
        .await
        .expect("external");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, parent.to_string());
        let rows = fetch_issues(
            &db,
            &lookup_statement(Lookup::DetailGet {
                pk: archived,
                project_id: project,
                slug: SLUG,
            }),
            &tz,
        )
        .await
        .expect("detail archived");
        assert!(rows.is_empty());
        let rows = fetch_issues(
            &db,
            &lookup_statement(Lookup::DetailGet {
                pk: parent,
                project_id: project,
                slug: SLUG,
            }),
            &tz,
        )
        .await
        .expect("detail");
        assert_eq!(rows.len(), 1);

        // Filter resolution reads.
        let states_found = filter_states(&db, project).await.expect("states");
        assert_eq!(
            states_found,
            vec![(todo, "Todo".to_owned()), (done, "Done".to_owned())]
        );
        let labels_found = filter_labels(&db, project, SLUG).await.expect("labels");
        assert_eq!(
            labels_found,
            vec![
                (ws_label, "global".to_owned()),
                (label, "bug".to_owned()),
                (zeta, "zeta".to_owned())
            ]
        );
        assert_eq!(
            resolve_parent(&db, SLUG, "acme", 1).await.expect("parent"),
            Some(parent)
        );
        assert_eq!(
            resolve_parent(&db, SLUG, "ACME", 3).await.expect("parent"),
            None
        );
        assert_eq!(
            resolve_parent(&db, SLUG, "OTH", 5).await.expect("parent"),
            Some(foreign)
        );
        assert_eq!(
            resolve_parent(&db, "nope", "ACME", 1)
                .await
                .expect("parent"),
            None
        );
    }
}
