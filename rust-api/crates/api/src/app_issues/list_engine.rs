#![forbid(unsafe_code)]

//! The issue-list engine on sea-query (Batch L).
//!
//! Every statement the project issue lists run (rows, counts, group and
//! sub-group totals, distinct group buckets, deleted ids) is built here as
//! a sea-query [`SelectStatement`] over the same table aliases the filter
//! kernels compile against: `issue` (issues), `project`, `workspaces`,
//! `state`, and the relation aliases `issue_assignee`, `issue_cycle`,
//! `issue_module`, `issue_mention`, `label_issue`, `issue_subscribers`,
//! `issue_intake`.
//!
//! Behaviour is the text engine's, statement for statement (the
//! differential live tests in `list_oracle.rs` run both against one seeded
//! dataset and compare the JSON byte for byte):
//! - the `issue_objects` preamble, guest scoping and the detail
//!   permission `EXISTS`;
//! - rich (`filters`) and legacy (`issue_filters`) predicates;
//! - the relation joins: INNER when a compiled rich leaf filters through
//!   the relation or the m2m group needs it, LEFT otherwise. The decision
//!   is structural (which declared relation leaf compiled to a predicate),
//!   not a text search. Legacy predicates never upgrade a join, exactly as
//!   before (a positive predicate over a LEFT join keeps the same rows);
//! - the annotation subqueries, order keys, group expressions, windows and
//!   counts.
//!
//! `COUNT(DISTINCT issue.id) FILTER (WHERE c)` is spelled
//! `COUNT(DISTINCT CASE WHEN c THEN issue.id END)`: the builder has no
//! `FILTER` clause, and both count the distinct ids of the rows where `c`
//! holds.

use std::collections::{BTreeSet, HashMap};

use chrono_tz::Tz;
use pidash_db::entity::{
    cycle_issues, cycles, estimate_points, file_assets, git_issue_syncs, github_issue_syncs,
    intake_issues, issue_assignees, issue_labels, issue_links, issue_mentions, issue_subscribers,
    issue_types, issues, labels, module_issues, modules, project_members, projects, states, users,
    workspaces,
};
use pidash_db::filterset::{FilterKind, ISSUE_FILTERSET};
use pidash_db::issue_filters::FilterValue;
use sea_orm::{ConnectionTrait, DbBackend, FromQueryResult};
use sea_query::extension::postgres::{PgExpr, PgFunc};
use sea_query::{
    Alias, Asterisk, BinOper, CaseStatement, Condition, Expr, ExprTrait, Func, IntoIden, JoinType,
    NullOrdering, Order, OverStatement, Query, SelectStatement, SimpleExpr, SubQueryStatement,
    WindowStatement,
};
use serde_json::{Map, Value};

use super::{query_last, Denial, FilterLayers, Gate, QueryMap, INVALID_DETAIL_BODY_MSG};

// ---- names -------------------------------------------------------------------

pub(crate) fn a(name: &'static str) -> Alias {
    Alias::new(name)
}

/// `alias.column`.
pub(crate) fn col<C: IntoIden + 'static>(alias: &'static str, column: C) -> Expr {
    Expr::col((a(alias), column))
}

/// `issue.column`.
pub(crate) fn issue<C: IntoIden + 'static>(column: C) -> Expr {
    col("issue", column)
}

/// A scalar subquery expression.
pub(crate) fn sub(stmt: SelectStatement) -> SimpleExpr {
    SimpleExpr::SubQuery(None, Box::new(SubQueryStatement::SelectStatement(stmt)))
}

pub(crate) fn text<T: Into<String>>(value: T) -> SimpleExpr {
    Expr::val(value.into()).into()
}

fn uuid_val(id: uuid::Uuid) -> SimpleExpr {
    Expr::val(id).into()
}

/// `COUNT(*)`, or `NULLIF(COUNT(*), 0)` for the grouped `Subquery` count
/// form (zero related rows → no group → `NULL`).
fn count_star(null_when_empty: bool) -> SimpleExpr {
    let count: SimpleExpr = Func::count(Expr::col(Asterisk)).into();
    if null_when_empty {
        Func::cust(a("NULLIF")).arg(count).arg(0).into()
    } else {
        count
    }
}

pub(crate) fn select_one_from<T: IntoIden + 'static>(
    table: T,
    alias: &'static str,
) -> SelectStatement {
    let mut stmt = Query::select();
    stmt.expr(Expr::val(1)).from_as(table, a(alias));
    stmt
}

// ---- filter set --------------------------------------------------------------

/// The relation joins every filtered list carries (table, kernel alias).
const RELATION_JOINS: &[(RelationTable, &str)] = &[
    (RelationTable::Assignees, "issue_assignee"),
    (RelationTable::Cycles, "issue_cycle"),
    (RelationTable::Modules, "issue_module"),
    (RelationTable::Mentions, "issue_mention"),
    (RelationTable::Labels, "label_issue"),
    (RelationTable::Subscribers, "issue_subscribers"),
];

#[derive(Debug, Clone, Copy)]
enum RelationTable {
    Assignees,
    Cycles,
    Modules,
    Mentions,
    Labels,
    Subscribers,
}

impl RelationTable {
    /// `JOIN <table> AS <alias> ON <alias>.issue_id = issue.id`.
    fn join(self, stmt: &mut SelectStatement, kind: JoinType, alias: &'static str) {
        let on = |issue_id: Expr| issue_id.equals((a("issue"), issues::Column::Id));
        match self {
            Self::Assignees => stmt.join_as(
                kind,
                issue_assignees::Entity,
                a(alias),
                on(col(alias, issue_assignees::Column::IssueId)),
            ),
            Self::Cycles => stmt.join_as(
                kind,
                cycle_issues::Entity,
                a(alias),
                on(col(alias, cycle_issues::Column::IssueId)),
            ),
            Self::Modules => stmt.join_as(
                kind,
                module_issues::Entity,
                a(alias),
                on(col(alias, module_issues::Column::IssueId)),
            ),
            Self::Mentions => stmt.join_as(
                kind,
                issue_mentions::Entity,
                a(alias),
                on(col(alias, issue_mentions::Column::IssueId)),
            ),
            Self::Labels => stmt.join_as(
                kind,
                issue_labels::Entity,
                a(alias),
                on(col(alias, issue_labels::Column::IssueId)),
            ),
            Self::Subscribers => stmt.join_as(
                kind,
                issue_subscribers::Entity,
                a(alias),
                on(col(alias, issue_subscribers::Column::IssueId)),
            ),
        };
    }
}

/// M2M group filters (`GROUP_FILTER_MAPPER`): grouping by this raw field
/// forces an INNER join on the relation.
fn group_join_alias(group_by: Option<&str>, sub_group_by: Option<&str>, alias: &str) -> bool {
    let raw = match alias {
        "issue_assignee" => "assignees__id",
        "label_issue" => "labels__id",
        "issue_module" => "issue_module__module_id",
        _ => return false,
    };
    group_by == Some(raw) || sub_group_by == Some(raw)
}

/// The filtered issue set: `FROM`, joins and `WHERE`, no select list.
/// Callers clone [`IssueSet::select`] and add columns, order and windows.
#[derive(Debug, Clone)]
pub struct IssueSet {
    base: SelectStatement,
    /// The relation aliases a rich leaf or the m2m group joins INNER.
    pub referenced: Vec<&'static str>,
}

impl IssueSet {
    /// A fresh copy of the `FROM ... WHERE ...` statement.
    pub fn select(&self) -> SelectStatement {
        self.base.clone()
    }
}

/// The rich `filters` param compiled like [`super::complex_filter`],
/// additionally reporting which relation aliases a leaf filters through
/// (a declared relation leaf that compiled to a predicate, not to the
/// match-all / match-none constants).
fn rich_condition(
    query: &QueryMap,
    tz: &Tz,
    used: &mut BTreeSet<&'static str>,
) -> Result<Option<Condition>, Denial> {
    use pidash_db::filter::FilterTree;
    fn walk(
        tree: &FilterTree,
        tz: &Tz,
        used: &mut BTreeSet<&'static str>,
    ) -> Result<Condition, pidash_db::filter::FilterError> {
        match tree {
            FilterTree::And(children) => {
                let mut cond = Condition::all();
                for child in children {
                    cond = cond.add(walk(child, tz, used)?);
                }
                Ok(cond)
            }
            FilterTree::Or(children) => {
                let mut cond = Condition::any();
                for child in children {
                    cond = cond.add(walk(child, tz, used)?);
                }
                Ok(cond)
            }
            FilterTree::Not(child) => Ok(walk(child, tz, used)?.not()),
            FilterTree::Leaf(leaves) => {
                // `build_combined`, leaf by leaf, in the caller's order.
                let mut combined = Condition::all();
                for leaf in leaves {
                    let name = super::leaf_name(&leaf.field, leaf.lookup);
                    let expr = pidash_db::filterset::compile_leaf(&name, &leaf.value, tz)?;
                    if !matches!(expr, SimpleExpr::Custom(_)) {
                        if let Some(alias) = relation_alias(&name) {
                            used.insert(alias);
                        }
                    }
                    combined = combined.add(expr);
                }
                Ok(combined)
            }
        }
    }
    let raw = query_last(query, "filters").unwrap_or_default();
    if raw.is_empty() {
        return Ok(None);
    }
    match FilterTree::parse_param_opt(&raw).map_err(super::filter_denial)? {
        Some(tree) => walk(&tree, tz, used)
            .map(Some)
            .map_err(super::filter_denial),
        None => Ok(None),
    }
}

/// The relation alias a declared rich filter joins through, if any.
fn relation_alias(name: &str) -> Option<&'static str> {
    let decl = ISSUE_FILTERSET.iter().find(|decl| decl.name == name)?;
    match decl.kind {
        FilterKind::Relation { join_table, .. } => Some(join_table),
        _ => None,
    }
}

/// Map a legacy `issue_filters` predicate name to the relation alias it
/// references (for conditional JOIN promotion). Returns the alias if the
/// predicate filters through that relation; `is_null` predicates are
/// nullable references (LEFT join), `__in` predicates are positive
/// references (LEFT in the current port, kept LEFT for parity with the
/// legacy-never-upgrades rule, but still a join).
fn legacy_alias_for_predicate(name: &str) -> Option<&'static str> {
    if let Some(path) = name.strip_suffix("__isnull") {
        return Some(match path {
            "labels" | "label_issue__deleted_at" => "label_issue",
            "assignees" | "issue_assignee__deleted_at" => "issue_assignee",
            "issue_cycle__cycle_id" | "issue_cycle__deleted_at" => "issue_cycle",
            "issue_module__module_id" | "issue_module__deleted_at" => "issue_module",
            "issue_subscribers__deleted_at" => "issue_subscribers",
            "issue_mention__mention__id" => "issue_mention",
            _ => return None,
        });
    }
    Some(match name {
        "labels__in" => "label_issue",
        "assignees__in" => "issue_assignee",
        "issue_mention__mention__id__in" => "issue_mention",
        "issue_cycle__cycle_id__in" => "issue_cycle",
        "issue_module__module_id__in" => "issue_module",
        "issue_subscribers__subscriber_id__in" => "issue_subscribers",
        "issue_intake__status__in" => "issue_intake",
        _ => return None,
    })
}

/// Compute which relation aliases legacy predicates reference (for
/// conditional joins). Uses the same `issue_filters_get` dispatcher the
/// condition builder uses.
fn legacy_referenced_aliases(query: &QueryMap) -> BTreeSet<&'static str> {
    let flat: HashMap<String, String> = query
        .keys()
        .filter_map(|key| query_last(query, key).map(|last| (key.clone(), last)))
        .collect();
    let today = chrono::Utc::now().date_naive();
    let Ok(legacy) = pidash_db::issue_filters::issue_filters_get(&flat, "", today) else {
        return BTreeSet::new();
    };
    let mut out = BTreeSet::new();
    for (name, _) in legacy.predicates() {
        if let Some(alias) = legacy_alias_for_predicate(name) {
            out.insert(alias);
        }
    }
    out
}

/// The legacy `issue_filters(query_params, "GET")` predicates, ANDed in
/// order. Relative dates resolve against the UTC date
/// (`timezone.now().date()`); the `__date` truncations use `tz`.
fn legacy_condition(query: &QueryMap, tz: &Tz) -> Result<Option<Condition>, Denial> {
    let flat: HashMap<String, String> = query
        .keys()
        .filter_map(|key| query_last(query, key).map(|last| (key.clone(), last)))
        .collect();
    let today = chrono::Utc::now().date_naive();
    let legacy = pidash_db::issue_filters::issue_filters_get(&flat, "", today)
        .map_err(|_| Denial::ServerError)?;
    let mut cond = Condition::all();
    let mut any = false;
    for (name, value) in legacy.predicates() {
        cond = cond.add(legacy_expr(name, value, tz)?);
        any = true;
    }
    Ok(any.then_some(cond))
}

/// Shared tail of every filtered set: rich, legacy, extra, then the
/// relation joins (rendered whenever a filter stack runs).
#[allow(clippy::too_many_arguments)]
fn finish_set(
    mut stmt: SelectStatement,
    mut cond: Condition,
    query: &QueryMap,
    tz: &Tz,
    group_by: Option<&str>,
    sub_group_by: Option<&str>,
    extra: Option<SimpleExpr>,
    layers: FilterLayers,
) -> Result<IssueSet, Denial> {
    let mut used = BTreeSet::new();
    if layers.rich {
        if let Some(rich) = rich_condition(query, tz, &mut used)? {
            cond = cond.add(rich);
        }
    }
    if layers.legacy {
        if let Some(legacy) = legacy_condition(query, tz)? {
            cond = cond.add(legacy);
        }
    }
    if let Some(extra) = extra {
        cond = cond.add(extra);
    }
    let mut referenced = Vec::new();
    if layers.rich || layers.legacy {
        let legacy_used = if layers.legacy {
            legacy_referenced_aliases(query)
        } else {
            BTreeSet::new()
        };
        for (table, alias) in RELATION_JOINS {
            let grouped = group_join_alias(group_by, sub_group_by, alias);
            let rich = used.contains(alias);
            let legacy_need = legacy_used.contains(alias);
            let needs_join = rich || grouped || legacy_need;
            // `issue_cycle` must always be joined: cycle-name ordering
            // (`order_expr` with `Some("issue_cycle")`) resolves to
            // `OrderJoins::Bridge("issue_cycle")`, which references
            // `issue_cycle` in FROM. Skipping it breaks that invariant
            // (`missing FROM-clause entry`, 500 on
            // `order_by=issue_cycle__cycle__name`).
            let always_join = *alias == "issue_cycle";
            if !needs_join && !always_join {
                continue;
            }
            let inner = rich || grouped;
            if inner {
                referenced.push(*alias);
            }
            let kind = if inner {
                JoinType::InnerJoin
            } else {
                JoinType::LeftJoin
            };
            table.join(&mut stmt, kind, alias);
        }
        // No rich leaf filters through the intake row: always LEFT.
        stmt.join_as(
            JoinType::LeftJoin,
            intake_issues::Entity,
            a("issue_intake"),
            col("issue_intake", intake_issues::Column::IssueId)
                .equals((a("issue"), issues::Column::Id)),
        );
    }
    stmt.cond_where(cond);
    Ok(IssueSet {
        base: stmt,
        referenced,
    })
}

/// `state."group" IS NULL OR NOT (state."group" = 'triage')`: the
/// manager's triage exclusion, keeping NULL-state rows.
pub(crate) fn not_triage(state_alias: &'static str) -> SimpleExpr {
    let group = || col(state_alias, states::Column::Group);
    group().is_null().or(group().eq("triage").not())
}

/// The main filtered set ([`super::FilterLayers`] semantics): tenant
/// scope, the `issue_objects` manager, guest scoping (not on `detail` and
/// not when `layers.guest_scope` is off), rich + legacy filters, `extra`.
#[allow(clippy::too_many_arguments)]
pub fn issue_set(
    gate: &Gate,
    slug: &str,
    query: &QueryMap,
    group_by: Option<&str>,
    sub_group_by: Option<&str>,
    extra: Option<SimpleExpr>,
    detail: bool,
    layers: FilterLayers,
) -> Result<IssueSet, Denial> {
    let mut stmt = Query::select();
    stmt.from_as(issues::Entity, a("issue"))
        .join_as(
            JoinType::InnerJoin,
            projects::Entity,
            a("project"),
            col("project", projects::Column::Id).equals((a("issue"), issues::Column::ProjectId)),
        )
        .join(
            JoinType::InnerJoin,
            workspaces::Entity,
            Expr::col((workspaces::Entity, workspaces::Column::Id))
                .equals((a("issue"), issues::Column::WorkspaceId)),
        )
        .join_as(
            JoinType::LeftJoin,
            states::Entity,
            a("state"),
            Condition::all()
                .add(col("state", states::Column::Id).equals((a("issue"), issues::Column::StateId)))
                .add(col("state", states::Column::DeletedAt).is_null()),
        );
    let mut cond = Condition::all()
        .add(Expr::col((workspaces::Entity, workspaces::Column::Slug)).eq(slug))
        .add(issue(issues::Column::ProjectId).eq(gate.project_id))
        .add(issue(issues::Column::DeletedAt).is_null())
        .add(not_triage("state"))
        .add(issue(issues::Column::ArchivedAt).is_null())
        .add(col("project", projects::Column::ArchivedAt).is_null())
        .add(issue(issues::Column::IsDraft).eq(false));
    if gate.guest_scoped && !detail && layers.guest_scope {
        cond = cond.add(issue(issues::Column::CreatedById).eq(gate.user_id));
    }
    finish_set(
        stmt,
        cond,
        query,
        &gate.timezone,
        group_by,
        sub_group_by,
        extra,
        layers,
    )
}

/// The archived set (`archive.py:98-116`): non-epic types, archived rows,
/// this project and workspace, optional top-level filter, then the same
/// rich + legacy stacks and relation joins. The state join carries no
/// soft-delete guard and there is no projects join.
pub fn archive_issue_set(
    gate: &Gate,
    slug: &str,
    query: &QueryMap,
    group_by: Option<&str>,
    sub_group_by: Option<&str>,
    show_sub_issues: bool,
) -> Result<IssueSet, Denial> {
    let mut stmt = Query::select();
    stmt.from_as(issues::Entity, a("issue"))
        .join(
            JoinType::LeftJoin,
            issue_types::Entity,
            issue(issues::Column::TypeId).equals((issue_types::Entity, issue_types::Column::Id)),
        )
        .join(
            JoinType::InnerJoin,
            workspaces::Entity,
            Expr::col((workspaces::Entity, workspaces::Column::Id))
                .equals((a("issue"), issues::Column::WorkspaceId)),
        )
        .join_as(
            JoinType::LeftJoin,
            states::Entity,
            a("state"),
            col("state", states::Column::Id).equals((a("issue"), issues::Column::StateId)),
        );
    let mut cond = Condition::all()
        .add(issue(issues::Column::DeletedAt).is_null())
        .add(
            issue(issues::Column::TypeId).is_null().or(Expr::col((
                issue_types::Entity,
                issue_types::Column::IsEpic,
            ))
            .not()),
        )
        .add(issue(issues::Column::ArchivedAt).is_not_null())
        .add(issue(issues::Column::ProjectId).eq(gate.project_id))
        .add(Expr::col((workspaces::Entity, workspaces::Column::Slug)).eq(slug));
    if !show_sub_issues {
        cond = cond.add(issue(issues::Column::ParentId).is_null());
    }
    finish_set(
        stmt,
        cond,
        query,
        &gate.timezone,
        group_by,
        sub_group_by,
        None,
        FilterLayers::FULL,
    )
}

// ---- extra predicates --------------------------------------------------------

/// The epic boundary (`epics.py:65-66`): Django's inner join on
/// `issue_types` with no related-manager guard, as an `EXISTS`.
pub fn epic_only() -> SimpleExpr {
    let mut stmt = select_one_from(issue_types::Entity, "t");
    stmt.and_where(col("t", issue_types::Column::Id).equals((a("issue"), issues::Column::TypeId)))
        .and_where(col("t", issue_types::Column::IsEpic).into());
    Expr::exists(stmt)
}

/// `issue.id IN (...)`.
pub fn id_in(ids: Vec<uuid::Uuid>) -> SimpleExpr {
    issue(issues::Column::Id).is_in(ids)
}

/// `updated_at__gt`: `issue.updated_at > $n::timestamptz`, or `None` when
/// absent. Garbage is a `ValidationError` (400 invalid detail).
pub fn updated_at_gt(query: &QueryMap) -> Result<Option<SimpleExpr>, Denial> {
    let Some(raw) = query_last(query, "updated_at__gt") else {
        return Ok(None);
    };
    let parsed = super::parse_datetime_param(&raw)
        .ok_or_else(|| Denial::BadError(INVALID_DETAIL_BODY_MSG.to_owned()))?;
    Ok(Some(issue(issues::Column::UpdatedAt).gt(
        Expr::val(parsed.to_rfc3339()).cast_as(a("timestamptz")),
    )))
}

/// The `IssueDetailEndpoint.get` permission `EXISTS` (see the text
/// engine's `permission_exists_fragment`): an active membership above
/// guest, an active guest on a view-all project, or an active guest on
/// a restricted project for issues they created.
pub fn permission_exists(gate: &Gate) -> SimpleExpr {
    let branch = |role: SimpleExpr, view_all: bool, own_only: bool| {
        let mut stmt = select_one_from(project_members::Entity, "_pm");
        stmt.join_as(
            JoinType::InnerJoin,
            projects::Entity,
            a("_pp"),
            col("_pp", projects::Column::Id).equals((a("_pm"), project_members::Column::ProjectId)),
        )
        .and_where(
            col("_pm", project_members::Column::ProjectId)
                .equals((a("_perm"), issues::Column::ProjectId)),
        )
        .and_where(col("_pm", project_members::Column::MemberId).eq(gate.user_id))
        .and_where(col("_pm", project_members::Column::IsActive).into())
        .and_where(col("_pm", project_members::Column::DeletedAt).is_null())
        .and_where(role)
        .and_where(col("_pp", projects::Column::GuestViewAllFeatures).eq(view_all));
        if own_only {
            stmt.and_where(col("_perm", issues::Column::CreatedById).eq(gate.user_id));
        }
        Expr::exists(stmt)
    };
    let role = || col("_pm", project_members::Column::Role);
    let mut outer = select_one_from(issues::Entity, "_perm");
    outer
        .and_where(col("_perm", issues::Column::Id).equals((a("issue"), issues::Column::Id)))
        .and_where(col("_perm", issues::Column::ProjectId).eq(gate.project_id))
        .cond_where(
            Condition::any()
                .add(branch(role().gt(5), true, false))
                .add(branch(role().eq(5), true, false))
                .add(branch(role().eq(5), false, true)),
        );
    Expr::exists(outer)
}

// ---- legacy predicates ---------------------------------------------------------

/// `(alias, column)` for `__isnull` predicates (and bare `Null` values).
fn isnull_column(path: &str) -> Option<Expr> {
    Some(match path {
        "parent" => issue(issues::Column::ParentId),
        "labels" => col("label_issue", issue_labels::Column::LabelId),
        "assignees" => col("issue_assignee", issue_assignees::Column::AssigneeId),
        "created_by" => issue(issues::Column::CreatedById),
        "issue_cycle__cycle_id" => col("issue_cycle", cycle_issues::Column::CycleId),
        "issue_module__module_id" => col("issue_module", module_issues::Column::ModuleId),
        "label_issue__deleted_at" => col("label_issue", issue_labels::Column::DeletedAt),
        "issue_assignee__deleted_at" => col("issue_assignee", issue_assignees::Column::DeletedAt),
        "issue_cycle__deleted_at" => col("issue_cycle", cycle_issues::Column::DeletedAt),
        "issue_module__deleted_at" => col("issue_module", module_issues::Column::DeletedAt),
        "issue_subscribers__deleted_at" => {
            col("issue_subscribers", issue_subscribers::Column::DeletedAt)
        }
        "target_date" => issue(issues::Column::TargetDate),
        "start_date" => issue(issues::Column::StartDate),
        _ => return None,
    })
}

/// Columns for UUID `__in` predicates. `logged_by` has no model field
/// (Django's `FieldError`, a generic 500).
fn uuid_in_column(name: &str) -> Option<Expr> {
    Some(match name {
        "state__in" => issue(issues::Column::StateId),
        "parent__in" => issue(issues::Column::ParentId),
        "labels__in" => col("label_issue", issue_labels::Column::LabelId),
        "assignees__in" => col("issue_assignee", issue_assignees::Column::AssigneeId),
        "issue_mention__mention__id__in" => col("issue_mention", issue_mentions::Column::MentionId),
        "created_by__in" => issue(issues::Column::CreatedById),
        "project__in" => issue(issues::Column::ProjectId),
        "issue_cycle__cycle_id__in" => col("issue_cycle", cycle_issues::Column::CycleId),
        "issue_module__module_id__in" => col("issue_module", module_issues::Column::ModuleId),
        "issue_subscribers__subscriber_id__in" => {
            col("issue_subscribers", issue_subscribers::Column::SubscriberId)
        }
        _ => return None,
    })
}

/// Columns for string `__in` predicates.
fn strings_in_column(name: &str) -> Option<Expr> {
    Some(match name {
        "state__group__in" => col("state", states::Column::Group),
        "estimate_point__in" => issue(issues::Column::EstimatePointId),
        "priority__in" => issue(issues::Column::Priority),
        "issue_intake__status__in" => col("issue_intake", intake_issues::Column::Status),
        _ => return None,
    })
}

/// A `__date` term truncated in `tz`: `(issue.<column> AT TIME ZONE
/// $n)::date` (Django's `TruncDate` under the activated actor zone).
fn zoned_date(column: issues::Column, tz: &Tz) -> SimpleExpr {
    issue(column)
        .binary(BinOper::Custom("AT TIME ZONE"), text(tz.name()))
        .cast_as(a("date"))
}

/// The left side of a `__gte` / `__lte` date term.
fn date_term(term: &str, tz: &Tz) -> Option<SimpleExpr> {
    Some(match term {
        "created_at__date" => zoned_date(issues::Column::CreatedAt, tz),
        "completed_at__date" => zoned_date(issues::Column::CompletedAt, tz),
        "start_date" => issue(issues::Column::StartDate).into(),
        "target_date" => issue(issues::Column::TargetDate).into(),
        _ => return None,
    })
}

/// `>=` / `<=` against `CAST($n AS date)`.
fn date_bound(left: SimpleExpr, operator: &str, value: String) -> Option<SimpleExpr> {
    let right = Expr::val(value).cast_as(a("date"));
    match operator {
        "gte" => Some(left.gte(right)),
        "lte" => Some(left.lte(right)),
        _ => None,
    }
}

/// Django `contains` / `icontains`: LIKE with `\`, `%`, `_` escaped.
fn like_pattern(text: &str) -> String {
    let escaped = text
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    format!("%{escaped}%")
}

/// One legacy predicate (the text engine's `legacy_sql`), same columns,
/// same errors in the same order.
pub(crate) fn legacy_expr(name: &str, value: &FilterValue, tz: &Tz) -> Result<SimpleExpr, Denial> {
    if let Some(path) = name.strip_suffix("__isnull") {
        let column = isnull_column(path).ok_or(Denial::ServerError)?;
        let flag = match value {
            FilterValue::Flag(flag) => *flag,
            _ => return Err(Denial::ServerError),
        };
        return Ok(if flag {
            column.is_null()
        } else {
            column.is_not_null()
        });
    }
    match value {
        FilterValue::Uuids(ids) => {
            let column = uuid_in_column(name).ok_or(Denial::ServerError)?;
            if ids.is_empty() {
                return Ok(Expr::val(false).into());
            }
            Ok(column.is_in(ids.iter().copied()))
        }
        FilterValue::Strings(items) => {
            let column = strings_in_column(name).ok_or(Denial::ServerError)?;
            if items.is_empty() {
                return Ok(Expr::val(false).into());
            }
            if name == "issue_intake__status__in" {
                let mut numbers = Vec::with_capacity(items.len());
                for item in items {
                    numbers.push(
                        item.parse::<i32>()
                            .map_err(|_| Denial::BadError(INVALID_DETAIL_BODY_MSG.to_owned()))?,
                    );
                }
                return Ok(column.is_in(numbers));
            }
            if name == "estimate_point__in" {
                // UUID PKs compared to strings: Django coerces via
                // `UUIDField.get_prep_value`; garbage is a ValidationError.
                let mut ids = Vec::with_capacity(items.len());
                for item in items {
                    ids.push(
                        item.parse::<uuid::Uuid>()
                            .map_err(|_| Denial::BadError(INVALID_DETAIL_BODY_MSG.to_owned()))?,
                    );
                }
                return Ok(column.is_in(ids));
            }
            Ok(column.is_in(items.iter().cloned()))
        }
        FilterValue::Text(raw) => legacy_text_expr(name, raw, tz),
        FilterValue::Flag(_) => Err(Denial::ServerError),
        FilterValue::Day(day) => {
            let (term, operator) = name.rsplit_once("__").ok_or(Denial::ServerError)?;
            let left = date_term(term, tz).ok_or(Denial::ServerError)?;
            date_bound(left, operator, day.to_string()).ok_or(Denial::ServerError)
        }
        FilterValue::Null => {
            let column = isnull_column(name).ok_or(Denial::ServerError)?;
            Ok(column.is_null())
        }
    }
}

/// Text predicates: `name__icontains`, explicit date bounds and the
/// single-value `__contains` form on a date term.
fn legacy_text_expr(name: &str, raw: &str, tz: &Tz) -> Result<SimpleExpr, Denial> {
    if name == "name__icontains" {
        return Ok(issue(issues::Column::Name).ilike(like_pattern(raw)));
    }
    let Some((term, operator)) = name.rsplit_once("__") else {
        return Err(Denial::ServerError);
    };
    if !matches!(operator, "gte" | "lte" | "contains") {
        return Err(Denial::ServerError);
    }
    let left = date_term(term, tz).ok_or(Denial::ServerError)?;
    // Explicit bounds must parse as dates: Django's `get_prep_value`
    // raises `ValidationError` (400 invalid detail) on garbage.
    if raw.parse::<chrono::NaiveDate>().is_err() && super::parse_datetime_param(raw).is_none() {
        return Err(Denial::BadError(INVALID_DETAIL_BODY_MSG.to_owned()));
    }
    if operator == "contains" {
        return Ok(left.cast_as(a("text")).like(like_pattern(raw)));
    }
    date_bound(left, operator, raw.to_owned()).ok_or(Denial::ServerError)
}

// ---- annotations ---------------------------------------------------------------

/// Which annotation shape a path selects (the text engine's
/// `annotation_selects` flags, documented there).
#[derive(Debug, Clone, Copy)]
pub struct Annotations {
    pub arrays: bool,
    pub skip_array: Option<&'static str>,
    pub assignee_active_member: bool,
    pub module_archived_guard: bool,
    pub scalar_counts: bool,
}

/// `(SELECT ci.cycle_id FROM cycle_issues ci WHERE ci.issue_id = issue.id
/// AND ci.deleted_at IS NULL LIMIT 1)`.
pub fn cycle_id_subquery() -> SimpleExpr {
    let mut stmt = Query::select();
    stmt.column((a("ci"), cycle_issues::Column::CycleId))
        .from_as(cycle_issues::Entity, a("ci"))
        .and_where(
            col("ci", cycle_issues::Column::IssueId).equals((a("issue"), issues::Column::Id)),
        )
        .and_where(col("ci", cycle_issues::Column::DeletedAt).is_null())
        .limit(1);
    sub(stmt)
}

pub(crate) fn link_count(null_when_empty: bool) -> SimpleExpr {
    let mut stmt = Query::select();
    stmt.expr(count_star(null_when_empty))
        .from_as(issue_links::Entity, a("il"))
        .and_where(col("il", issue_links::Column::IssueId).equals((a("issue"), issues::Column::Id)))
        .and_where(col("il", issue_links::Column::DeletedAt).is_null());
    sub(stmt)
}

pub(crate) fn attachment_count(null_when_empty: bool) -> SimpleExpr {
    let mut stmt = Query::select();
    stmt.expr(count_star(null_when_empty))
        .from_as(file_assets::Entity, a("fa"))
        .and_where(col("fa", file_assets::Column::IssueId).equals((a("issue"), issues::Column::Id)))
        .and_where(col("fa", file_assets::Column::EntityType).eq("ISSUE_ATTACHMENT"))
        .and_where(col("fa", file_assets::Column::DeletedAt).is_null());
    sub(stmt)
}

/// The sub-issue count over the `IssueManager` scope. `annotation` is the
/// select-list form (state join guarded, null-safe triage test); the
/// order-key form joins states unguarded and negates
/// `group = 'triage' AND group IS NOT NULL` (pinned live: a soft-deleted
/// triage state still excludes its children there).
pub(crate) fn sub_issues_count(null_when_empty: bool, annotation: bool) -> SimpleExpr {
    let mut stmt = Query::select();
    let state_on = col("cs", states::Column::Id).equals((a("c"), issues::Column::StateId));
    stmt.expr(count_star(null_when_empty))
        .from_as(issues::Entity, a("c"))
        .join_as(
            JoinType::LeftJoin,
            states::Entity,
            a("cs"),
            if annotation {
                Condition::all()
                    .add(state_on)
                    .add(col("cs", states::Column::DeletedAt).is_null())
            } else {
                Condition::all().add(state_on)
            },
        )
        .join_as(
            JoinType::InnerJoin,
            projects::Entity,
            a("cp"),
            col("cp", projects::Column::Id).equals((a("c"), issues::Column::ProjectId)),
        )
        .and_where(col("c", issues::Column::ParentId).equals((a("issue"), issues::Column::Id)))
        .and_where(col("c", issues::Column::DeletedAt).is_null());
    if annotation {
        stmt.and_where(not_triage("cs"));
    } else {
        stmt.and_where(
            col("cs", states::Column::Group)
                .eq("triage")
                .and(col("cs", states::Column::Group).is_not_null())
                .not(),
        );
    }
    stmt.and_where(col("c", issues::Column::ArchivedAt).is_null())
        .and_where(col("cp", projects::Column::ArchivedAt).is_null())
        .and_where(col("c", issues::Column::IsDraft).eq(false));
    sub(stmt)
}

/// `COALESCE(ARRAY_AGG(DISTINCT x), '{}'::uuid[])`.
pub(crate) fn id_array(x: Expr) -> SimpleExpr {
    Func::coalesce([
        PgFunc::array_agg_distinct(x).into(),
        Expr::val("{}").cast_as(a("uuid[]")),
    ])
    .into()
}

pub(crate) fn label_ids() -> SimpleExpr {
    let mut stmt = Query::select();
    stmt.expr(id_array(col("il", issue_labels::Column::LabelId)))
        .from_as(issue_labels::Entity, a("il"))
        .and_where(
            col("il", issue_labels::Column::IssueId).equals((a("issue"), issues::Column::Id)),
        )
        .and_where(col("il", issue_labels::Column::DeletedAt).is_null());
    sub(stmt)
}

fn assignee_ids(active_member: bool) -> SimpleExpr {
    let mut stmt = Query::select();
    stmt.expr(id_array(col("ia", issue_assignees::Column::AssigneeId)))
        .from_as(issue_assignees::Entity, a("ia"))
        .and_where(
            col("ia", issue_assignees::Column::IssueId).equals((a("issue"), issues::Column::Id)),
        )
        .and_where(col("ia", issue_assignees::Column::DeletedAt).is_null());
    if active_member {
        let mut member = select_one_from(project_members::Entity, "pm");
        member
            .and_where(
                col("pm", project_members::Column::MemberId)
                    .equals((a("ia"), issue_assignees::Column::AssigneeId)),
            )
            .and_where(col("pm", project_members::Column::IsActive).into())
            .and_where(col("pm", project_members::Column::DeletedAt).is_null());
        stmt.and_where(Expr::exists(member));
    }
    sub(stmt)
}

fn module_ids(archived_guard: bool) -> SimpleExpr {
    let mut stmt = Query::select();
    stmt.expr(id_array(col("mi", module_issues::Column::ModuleId)))
        .from_as(module_issues::Entity, a("mi"));
    if archived_guard {
        stmt.join_as(
            JoinType::InnerJoin,
            modules::Entity,
            a("m"),
            col("m", modules::Column::Id).equals((a("mi"), module_issues::Column::ModuleId)),
        );
    }
    stmt.and_where(
        col("mi", module_issues::Column::IssueId).equals((a("issue"), issues::Column::Id)),
    )
    .and_where(col("mi", module_issues::Column::DeletedAt).is_null());
    if archived_guard {
        stmt.and_where(col("m", modules::Column::ArchivedAt).is_null())
            .and_where(col("m", modules::Column::DeletedAt).is_null());
    }
    sub(stmt)
}

/// The shared list select: issue columns, the count and array annotations
/// and `state__group`, in the text engine's column order.
pub fn annotate(stmt: &mut SelectStatement, ann: Annotations) {
    let null_counts = !ann.scalar_counts;
    stmt.column((a("issue"), issues::Column::Id))
        .column((a("issue"), issues::Column::Name))
        .column((a("issue"), issues::Column::StateId))
        .column((a("issue"), issues::Column::SortOrder))
        .column((a("issue"), issues::Column::CompletedAt))
        .expr_as(issue(issues::Column::EstimatePointId), a("estimate_point"))
        .column((a("issue"), issues::Column::Priority))
        .column((a("issue"), issues::Column::StartDate))
        .column((a("issue"), issues::Column::TargetDate))
        .column((a("issue"), issues::Column::SequenceId))
        .column((a("issue"), issues::Column::ProjectId))
        .column((a("issue"), issues::Column::ParentId))
        .expr_as(cycle_id_subquery(), a("cycle_id"))
        .expr_as(link_count(null_counts), a("link_count"))
        .expr_as(attachment_count(null_counts), a("attachment_count"))
        .expr_as(sub_issues_count(null_counts, true), a("sub_issues_count"))
        .column((a("issue"), issues::Column::CreatedAt))
        .column((a("issue"), issues::Column::UpdatedAt))
        .expr_as(issue(issues::Column::CreatedById), a("created_by"))
        .expr_as(issue(issues::Column::UpdatedById), a("updated_by"))
        .column((a("issue"), issues::Column::IsDraft))
        .column((a("issue"), issues::Column::ArchivedAt))
        .column((a("issue"), issues::Column::DeletedAt))
        .expr_as(col("state", states::Column::Group), a("state__group"));
    if ann.arrays {
        if ann.skip_array != Some("label_ids") {
            stmt.expr_as(label_ids(), a("label_ids"));
        }
        if ann.skip_array != Some("assignee_ids") {
            stmt.expr_as(assignee_ids(ann.assignee_active_member), a("assignee_ids"));
        }
        if ann.skip_array != Some("module_ids") {
            stmt.expr_as(module_ids(ann.module_archived_guard), a("module_ids"));
        }
    }
}

/// The flat `fields=`/`expand=` branch's `IssueSerializer` columns, with
/// `is_synced` (empty `external_source` short-circuits false; otherwise a
/// live git or GitHub sync row).
pub fn serializer_columns(stmt: &mut SelectStatement) {
    let synced = |table: SelectStatement| Expr::exists(table);
    let mut git = select_one_from(git_issue_syncs::Entity, "g");
    git.and_where(
        col("g", git_issue_syncs::Column::IssueId).equals((a("issue"), issues::Column::Id)),
    )
    .and_where(col("g", git_issue_syncs::Column::DeletedAt).is_null());
    let mut github = select_one_from(github_issue_syncs::Entity, "gh");
    github
        .and_where(
            col("gh", github_issue_syncs::Column::IssueId).equals((a("issue"), issues::Column::Id)),
        )
        .and_where(col("gh", github_issue_syncs::Column::DeletedAt).is_null());
    let is_synced = CaseStatement::new()
        .case(
            issue(issues::Column::ExternalSource)
                .is_null()
                .or(issue(issues::Column::ExternalSource).eq("")),
            Expr::val(false),
        )
        .finally(synced(git).or(synced(github)));
    stmt.column((a("issue"), issues::Column::Id))
        .column((a("issue"), issues::Column::WorkspaceId))
        .column((a("issue"), issues::Column::Name))
        .column((a("issue"), issues::Column::StateId))
        .column((a("issue"), issues::Column::SortOrder))
        .column((a("issue"), issues::Column::CompletedAt))
        .expr_as(issue(issues::Column::EstimatePointId), a("estimate_point"))
        .column((a("issue"), issues::Column::Priority))
        .column((a("issue"), issues::Column::ComplexityScore))
        .column((a("issue"), issues::Column::StartDate))
        .column((a("issue"), issues::Column::TargetDate))
        .column((a("issue"), issues::Column::SequenceId))
        .column((a("issue"), issues::Column::ProjectId))
        .column((a("issue"), issues::Column::ParentId))
        .column((a("issue"), issues::Column::TypeId))
        .column((a("issue"), issues::Column::AssignedPodId))
        .column((a("issue"), issues::Column::AgentExecutor))
        .column((a("issue"), issues::Column::CreatedAt))
        .column((a("issue"), issues::Column::UpdatedAt))
        .expr_as(issue(issues::Column::CreatedById), a("created_by"))
        .expr_as(issue(issues::Column::UpdatedById), a("updated_by"))
        .column((a("issue"), issues::Column::IsDraft))
        .column((a("issue"), issues::Column::ArchivedAt))
        .expr_as(is_synced, a("is_synced"));
}

// ---- ordering ------------------------------------------------------------------

/// The priority `CASE` (`priority_order`): no `ELSE`.
pub(crate) fn priority_case() -> SimpleExpr {
    let mut case = CaseStatement::new();
    for (index, priority) in pidash_services::app_issues::ordering::PRIORITY_ORDER
        .iter()
        .enumerate()
    {
        case = case.case(
            issue(issues::Column::Priority).eq(*priority),
            Expr::val(index as i32),
        );
    }
    case.into()
}

/// The state-group `CASE` (`state_order`), `ELSE` after the last group.
pub(crate) fn state_case() -> SimpleExpr {
    let order = pidash_services::app_issues::ordering::STATE_ORDER;
    let mut case = CaseStatement::new();
    for (index, group) in order.iter().enumerate() {
        case = case.case(
            col("state", states::Column::Group).eq(*group),
            Expr::val(index as i32),
        );
    }
    case.finally(Expr::val(order.len() as i32)).into()
}

/// The `Min(...)` annotation for the assignee/label/module branch, as a
/// scalar subquery (NULL when empty).
pub(crate) fn min_order_subquery(relation: &str) -> Result<SimpleExpr, Denial> {
    let mut stmt = Query::select();
    match relation {
        "labels__name" => {
            stmt.expr(Func::min(col("l", labels::Column::Name)))
                .from_as(issue_labels::Entity, a("il"))
                .join_as(
                    JoinType::InnerJoin,
                    labels::Entity,
                    a("l"),
                    Condition::all()
                        .add(
                            col("l", labels::Column::Id)
                                .equals((a("il"), issue_labels::Column::LabelId)),
                        )
                        .add(col("l", labels::Column::DeletedAt).is_null()),
                )
                .and_where(
                    col("il", issue_labels::Column::IssueId)
                        .equals((a("issue"), issues::Column::Id)),
                )
                .and_where(col("il", issue_labels::Column::DeletedAt).is_null());
        }
        "assignees__first_name" => {
            stmt.expr(Func::min(col("u", users::Column::FirstName)))
                .from_as(issue_assignees::Entity, a("ia"))
                .join_as(
                    JoinType::InnerJoin,
                    users::Entity,
                    a("u"),
                    col("u", users::Column::Id)
                        .equals((a("ia"), issue_assignees::Column::AssigneeId)),
                )
                .and_where(
                    col("ia", issue_assignees::Column::IssueId)
                        .equals((a("issue"), issues::Column::Id)),
                )
                .and_where(col("ia", issue_assignees::Column::DeletedAt).is_null());
        }
        "issue_module__module__name" => {
            stmt.expr(Func::min(col("m", modules::Column::Name)))
                .from_as(module_issues::Entity, a("mi"))
                .join_as(
                    JoinType::InnerJoin,
                    modules::Entity,
                    a("m"),
                    Condition::all()
                        .add(
                            col("m", modules::Column::Id)
                                .equals((a("mi"), module_issues::Column::ModuleId)),
                        )
                        .add(col("m", modules::Column::DeletedAt).is_null()),
                )
                .and_where(
                    col("mi", module_issues::Column::IssueId)
                        .equals((a("issue"), issues::Column::Id)),
                )
                .and_where(col("mi", module_issues::Column::DeletedAt).is_null());
        }
        _ => return Err(Denial::ServerError),
    }
    Ok(sub(stmt))
}

/// Extra `FROM` joins an order key needs (only the cycle-name traversal).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderJoins {
    None,
    /// `LEFT JOIN cycles AS __order_c` over an already-joined bridge.
    Bridge(&'static str),
    /// `LEFT JOIN cycle_issues AS __order_ci` + `cycles AS __order_c`.
    Fresh,
}

impl OrderJoins {
    pub fn is_empty(self) -> bool {
        self == Self::None
    }

    /// Add the joins to a row (or fan-out count) statement.
    pub fn apply(self, stmt: &mut SelectStatement) {
        match self {
            Self::None => {}
            Self::Bridge(alias) => {
                stmt.join_as(
                    JoinType::LeftJoin,
                    cycles::Entity,
                    a("__order_c"),
                    col("__order_c", cycles::Column::Id)
                        .equals((a(alias), cycle_issues::Column::CycleId)),
                );
            }
            Self::Fresh => {
                stmt.join_as(
                    JoinType::LeftJoin,
                    cycle_issues::Entity,
                    a("__order_ci"),
                    col("__order_ci", cycle_issues::Column::IssueId)
                        .equals((a("issue"), issues::Column::Id)),
                )
                .join_as(
                    JoinType::LeftJoin,
                    cycles::Entity,
                    a("__order_c"),
                    col("__order_c", cycles::Column::Id)
                        .equals((a("__order_ci"), cycle_issues::Column::CycleId)),
                );
            }
        }
    }
}

/// The resolved paginator order key (the text engine's `order_key`).
#[derive(Debug, Clone)]
pub struct OrderExpr {
    pub expr: SimpleExpr,
    pub descending: bool,
    pub joins: OrderJoins,
}

impl OrderExpr {
    pub fn order(&self) -> Order {
        if self.descending {
            Order::Desc
        } else {
            Order::Asc
        }
    }
}

/// [`super::order_key`] as a builder expression: same keys, same 500 for
/// unknown paths, same count forms and cycle-name join reuse.
pub fn order_expr(
    out_param: &str,
    orig_param: &str,
    cycle_bridge: Option<&'static str>,
    null_counts: bool,
) -> Result<OrderExpr, Denial> {
    let descending = out_param.starts_with('-');
    let key = out_param.trim_start_matches('-');
    let mut joins = OrderJoins::None;
    let expr: SimpleExpr = match key {
        "priority_order" => priority_case(),
        "state_order" => state_case(),
        "min_values" => min_order_subquery(orig_param.trim_start_matches('-'))?,
        "cycle_id" => cycle_id_subquery(),
        "created_at" => issue(issues::Column::CreatedAt).into(),
        "updated_at" => issue(issues::Column::UpdatedAt).into(),
        "sort_order" => issue(issues::Column::SortOrder).into(),
        "sequence_id" => issue(issues::Column::SequenceId).into(),
        "name" => issue(issues::Column::Name).into(),
        "start_date" => issue(issues::Column::StartDate).into(),
        "target_date" => issue(issues::Column::TargetDate).into(),
        "completed_at" => issue(issues::Column::CompletedAt).into(),
        "id" => issue(issues::Column::Id).into(),
        "is_draft" => issue(issues::Column::IsDraft).into(),
        "archived_at" => issue(issues::Column::ArchivedAt).into(),
        "deleted_at" => issue(issues::Column::DeletedAt).into(),
        "estimate_point" => issue(issues::Column::EstimatePointId).into(),
        "project" => issue(issues::Column::ProjectId).into(),
        "parent" => issue(issues::Column::ParentId).into(),
        "state" => issue(issues::Column::StateId).into(),
        "state__name" => col("state", states::Column::Name).into(),
        "created_by" => issue(issues::Column::CreatedById).into(),
        "updated_by" => issue(issues::Column::UpdatedById).into(),
        "link_count" => link_count(null_counts),
        "attachment_count" => attachment_count(null_counts),
        "sub_issues_count" => sub_issues_count(null_counts, false),
        "estimate_point__key" => {
            let mut stmt = Query::select();
            stmt.column((a("ep"), estimate_points::Column::Key))
                .from_as(estimate_points::Entity, a("ep"))
                .and_where(
                    col("ep", estimate_points::Column::Id)
                        .equals((a("issue"), issues::Column::EstimatePointId)),
                );
            sub(stmt)
        }
        "issue_cycle__cycle__name" => {
            joins = match cycle_bridge {
                Some(alias) => OrderJoins::Bridge(alias),
                None => OrderJoins::Fresh,
            };
            col("__order_c", cycles::Column::Name).into()
        }
        _ => return Err(Denial::ServerError),
    };
    Ok(OrderExpr {
        expr,
        descending,
        joins,
    })
}

/// `ORDER BY` for the unpaginated flat endpoint (the text engine's
/// `flat_order_sql`): the rewritten priority/state branches sort their
/// `CASE` ascending then `created_at DESC`; the min branch its subquery
/// then `created_at DESC`; the default branch resolves the raw param with
/// a `created_at DESC` tiebreak unless the param mentions `created_at`.
/// No `NULLS LAST` anywhere. `created_at` here is the output column.
/// The flat order as an expression: `(expr, order, created_at tiebreak,
/// joins)`. Shared by [`flat_order`] (raw `ORDER BY` expr, for the text
/// engine's unpaginated probe) and [`flat_list_rows`] (selected
/// `__order_key`, required under `DISTINCT`).
fn flat_order_key(
    out_param: &str,
    orig_param: &str,
) -> Result<(SimpleExpr, Order, bool, OrderJoins), Denial> {
    match out_param {
        "priority_order" | "-priority_order" => {
            Ok((priority_case(), Order::Asc, true, OrderJoins::None))
        }
        "state_order" | "-state_order" => Ok((state_case(), Order::Asc, true, OrderJoins::None)),
        "min_values" | "-min_values" => {
            let sub = min_order_subquery(orig_param.trim_start_matches('-'))?;
            let order = if out_param.starts_with('-') {
                Order::Desc
            } else {
                Order::Asc
            };
            Ok((sub, order, true, OrderJoins::None))
        }
        _ => {
            let key = order_expr(out_param, orig_param, Some("issue_cycle"), false)?;
            Ok((
                key.expr.clone(),
                key.order(),
                !orig_param.contains("created_at"),
                key.joins,
            ))
        }
    }
}

pub fn flat_order(
    stmt: &mut SelectStatement,
    out_param: &str,
    orig_param: &str,
) -> Result<OrderJoins, Denial> {
    let (expr, order, tiebreak, joins) = flat_order_key(out_param, orig_param)?;
    stmt.order_by_expr(expr, order);
    if tiebreak {
        stmt.order_by(a("created_at"), Order::Desc);
    }
    Ok(joins)
}

// ---- grouping ------------------------------------------------------------------

/// The expression a group field partitions by.
pub fn group_expr(field: &str) -> Result<SimpleExpr, Denial> {
    Ok(match field {
        "labels__id" => col("label_issue", issue_labels::Column::LabelId).into(),
        "assignees__id" => col("issue_assignee", issue_assignees::Column::AssigneeId).into(),
        "issue_module__module_id" => col("issue_module", module_issues::Column::ModuleId).into(),
        "state_id" => issue(issues::Column::StateId).into(),
        "priority" => issue(issues::Column::Priority).into(),
        "cycle_id" => cycle_id_subquery(),
        "project_id" => issue(issues::Column::ProjectId).into(),
        "state__group" => col("state", states::Column::Group).into(),
        "target_date" => issue(issues::Column::TargetDate).into(),
        "start_date" => issue(issues::Column::StartDate).into(),
        "created_by" => issue(issues::Column::CreatedById).into(),
        _ => return Err(Denial::ServerError),
    })
}

/// The raw m2m group key per joined row, aliased by its field name.
fn group_member(stmt: &mut SelectStatement, group_by: &str) {
    match group_by {
        "labels__id" => {
            stmt.expr_as(
                col("label_issue", issue_labels::Column::LabelId),
                a("labels__id"),
            );
        }
        "assignees__id" => {
            stmt.expr_as(
                col("issue_assignee", issue_assignees::Column::AssigneeId),
                a("assignees__id"),
            );
        }
        "issue_module__module_id" => {
            stmt.expr_as(
                col("issue_module", module_issues::Column::ModuleId),
                a("issue_module__module_id"),
            );
        }
        _ => {}
    }
}

/// The grouped totals' count filter: an intake row in (1, -1, 2) or none,
/// not archived, not a draft.
fn count_filter() -> SimpleExpr {
    col("issue_intake", intake_issues::Column::Status)
        .is_in([1, -1, 2])
        .or(col("issue_intake", intake_issues::Column::Id).is_null())
        .and(issue(issues::Column::ArchivedAt).is_null())
        .and(issue(issues::Column::IsDraft).eq(false))
}

/// `COUNT(DISTINCT CASE WHEN <count filter> THEN issue.id END)`.
fn filtered_distinct_count() -> SimpleExpr {
    let case = CaseStatement::new().case(count_filter(), issue(issues::Column::Id));
    Func::count_distinct(SimpleExpr::Case(Box::new(case))).into()
}

/// `COALESCE(<expr>::text, 'None')`: a group bucket's key.
fn bucket(expr: SimpleExpr) -> SimpleExpr {
    Func::coalesce([expr.cast_as(a("text")), text("None")]).into()
}

// ---- statements ------------------------------------------------------------------

/// `SELECT COUNT(DISTINCT issue.id) AS n FROM ...`.
pub fn count_distinct(set: &IssueSet) -> SelectStatement {
    let mut stmt = set.select();
    stmt.expr_as(Func::count_distinct(issue(issues::Column::Id)), a("n"));
    stmt
}

/// `SELECT COUNT(*) AS n` over the order-joined rows (grouped totals with
/// the cycle-name fan-out).
pub fn count_rows(set: &IssueSet, joins: OrderJoins) -> SelectStatement {
    let mut stmt = set.select();
    joins.apply(&mut stmt);
    stmt.expr_as(Func::count(Expr::col(Asterisk)), a("n"));
    stmt
}

/// The flat paginated page: `SELECT DISTINCT <selects>, (key) AS
/// __order_key ... ORDER BY __order_key <dir> NULLS LAST,
/// issue.created_at DESC LIMIT .. OFFSET ..`.
pub fn flat_page(
    set: &IssueSet,
    ann: Annotations,
    key: &OrderExpr,
    limit: i64,
    offset: i64,
) -> Result<SelectStatement, Denial> {
    let mut stmt = set.select();
    key.joins.apply(&mut stmt);
    stmt.distinct();
    annotate(&mut stmt, ann);
    stmt.expr_as(key.expr.clone(), a("__order_key"))
        .order_by_with_nulls(a("__order_key"), key.order(), NullOrdering::Last)
        .order_by((a("issue"), issues::Column::CreatedAt), Order::Desc)
        .limit(non_negative(limit)?)
        .offset(non_negative(offset)?);
    Ok(stmt)
}

/// The grouped window page: rows numbered per (sub-)group partition,
/// kept in `(offset, stop]`, ordered globally like the window.
pub fn grouped_page(
    set: &IssueSet,
    group_by: &str,
    sub_group_by: Option<&str>,
    ann: Annotations,
    key: &OrderExpr,
    offset: i64,
    stop: i64,
) -> Result<SelectStatement, Denial> {
    // G01: DISTINCT before window. Fan-out rows from unconditional
    // relation joins share identical output columns; dedup must happen
    // before `ROW_NUMBER()` or the window assigns different numbers
    // to duplicates and inflates `count`. We build a DISTINCT
    // subselect (inner) with `__order_key`, then window over it so
    // one row per issue per group bucket is numbered.
    let mut inner = set.select();
    key.joins.apply(&mut inner);
    annotate(&mut inner, ann);
    group_member(&mut inner, group_by);
    if let Some(sub) = sub_group_by {
        group_member(&mut inner, sub);
    }
    inner.expr_as(key.expr.clone(), a("__order_key")).distinct();
    // Partition/order on the deduped output columns (`__dedup` alias):
    // each group field's output column name equals the field name for
    // every group_by the engine accepts (labels__id, assignees__id,
    // issue_module__module_id, state_id, priority, cycle_id,
    // project_id, state__group, target_date, start_date, created_by).
    // `__order_key` and `created_at` carry the global order.
    let mut window = WindowStatement::new();
    if group_expr(group_by).is_err() {
        return Err(Denial::ServerError);
    }
    if let Some(sub) = sub_group_by {
        if group_expr(sub).is_err() {
            return Err(Denial::ServerError);
        }
    }
    window.add_partition_by(Expr::col((Alias::new("__dedup"), Alias::new(group_by))).into());
    if let Some(sub) = sub_group_by {
        window.add_partition_by(Expr::col((Alias::new("__dedup"), Alias::new(sub))).into());
    }
    window
        .order_by_expr_with_nulls(
            Expr::col((Alias::new("__dedup"), Alias::new("__order_key"))).into(),
            key.order(),
            NullOrdering::Last,
        )
        .order_by(
            (Alias::new("__dedup"), Alias::new("created_at")),
            Order::Desc,
        );
    let mut windowed = Query::select();
    windowed
        .column(Asterisk)
        .expr_window_as(Func::cust(a("ROW_NUMBER")), window, a("__rn"))
        .from_subquery(inner, a("__dedup"));
    let mut outer = Query::select();
    outer
        .column(Asterisk)
        .from_subquery(windowed, a("__w"))
        .and_where(col("__w", a("__rn")).gt(offset))
        .and_where(col("__w", a("__rn")).lte(stop))
        .order_by_with_nulls(
            (a("__w"), a("__order_key")),
            key.order(),
            NullOrdering::Last,
        )
        .order_by((a("__w"), a("created_at")), Order::Desc);
    Ok(outer)
}

/// `(bucket, n)` per group: the filtered distinct count.
pub fn group_totals(set: &IssueSet, group_by: &str) -> Result<SelectStatement, Denial> {
    let mut stmt = set.select();
    stmt.expr_as(bucket(group_expr(group_by)?), a("bucket"))
        .expr_as(filtered_distinct_count(), a("n"))
        .group_by_col(a("bucket"));
    Ok(stmt)
}

/// `(bucket, sub, n)` per group pair.
pub fn sub_totals(
    set: &IssueSet,
    group_by: &str,
    sub_group_by: &str,
) -> Result<SelectStatement, Denial> {
    let mut stmt = set.select();
    stmt.expr_as(bucket(group_expr(group_by)?), a("bucket"))
        .expr_as(bucket(group_expr(sub_group_by)?), a("sub"))
        .expr_as(filtered_distinct_count(), a("n"))
        .group_by_col(a("bucket"))
        .group_by_col(a("sub"));
    Ok(stmt)
}

/// `SELECT DISTINCT <column>::text AS bucket, issue.created_at AS
/// ordering_created_at ... ORDER BY issue.created_at DESC` (Django's
/// `get_extra_select`: DISTINCT over the pair, NULLs kept).
pub fn distinct_bucket_values(set: &IssueSet, field: &str) -> Option<SelectStatement> {
    let column = match field {
        "target_date" => issues::Column::TargetDate,
        "start_date" => issues::Column::StartDate,
        "created_by" => issues::Column::CreatedById,
        _ => return None,
    };
    let mut stmt = set.select();
    stmt.distinct()
        .expr_as(issue(column).cast_as(a("text")), a("bucket"))
        .expr_as(issue(issues::Column::CreatedAt), a("ordering_created_at"))
        .order_by((a("issue"), issues::Column::CreatedAt), Order::Desc);
    Some(stmt)
}

/// `issues/list/` `.values()` rows: scalar counts, unpaginated, the flat
/// order (cycle-name joins on the row query only).
pub fn flat_list_rows(
    set: &IssueSet,
    out_param: &str,
    orig_param: &str,
) -> Result<SelectStatement, Denial> {
    let mut stmt = set.select();
    stmt.distinct();
    // `flat_page` pattern: the order expr must be selected as
    // `__order_key` so `ORDER BY` on the alias is valid under `DISTINCT`
    // (raw `ORDER BY <expr>` 500s: "for SELECT DISTINCT, ORDER BY
    // expressions must appear in select list").
    let (expr, order, tiebreak, joins) = flat_order_key(out_param, orig_param)?;
    stmt.expr_as(expr, a("__order_key"));
    stmt.order_by(a("__order_key"), order);
    if tiebreak {
        stmt.order_by(a("created_at"), Order::Desc);
    }
    joins.apply(&mut stmt);
    annotate(
        &mut stmt,
        Annotations {
            arrays: true,
            skip_array: None,
            assignee_active_member: false,
            module_archived_guard: true,
            scalar_counts: true,
        },
    );
    Ok(stmt)
}

/// The flat `fields=`/`expand=` branch rows, `-created_at`.
pub fn serializer_rows(set: &IssueSet) -> SelectStatement {
    let mut stmt = set.select();
    stmt.distinct();
    serializer_columns(&mut stmt);
    stmt.order_by((a("issue"), issues::Column::CreatedAt), Order::Desc);
    stmt
}

/// The v2 cursor page: grouped-count annotations with the active-member
/// assignee filter, plus `description_html`, by `updated_at`.
pub fn v2_rows(set: &IssueSet, limit: i64, offset: i64) -> Result<SelectStatement, Denial> {
    let mut stmt = set.select();
    stmt.distinct();
    annotate(
        &mut stmt,
        Annotations {
            arrays: true,
            skip_array: None,
            assignee_active_member: true,
            module_archived_guard: true,
            scalar_counts: false,
        },
    );
    stmt.column((a("issue"), issues::Column::DescriptionHtml))
        .order_by((a("issue"), issues::Column::UpdatedAt), Order::Asc)
        .limit(non_negative(limit)?)
        .offset(non_negative(offset)?);
    Ok(stmt)
}

/// `deleted-issues/`: `all_objects` ids, archived or deleted, newest first.
/// No guest scoping, no manager exclusions.
pub fn deleted_ids(gate: &Gate, slug: &str, updated_gt: Option<SimpleExpr>) -> SelectStatement {
    let mut stmt = Query::select();
    stmt.column((a("issue"), issues::Column::Id))
        .from_as(issues::Entity, a("issue"))
        .join_as(
            JoinType::InnerJoin,
            projects::Entity,
            a("project"),
            col("project", projects::Column::Id).equals((a("issue"), issues::Column::ProjectId)),
        )
        .join(
            JoinType::InnerJoin,
            workspaces::Entity,
            Expr::col((workspaces::Entity, workspaces::Column::Id))
                .equals((a("issue"), issues::Column::WorkspaceId)),
        )
        .and_where(Expr::col((workspaces::Entity, workspaces::Column::Slug)).eq(slug))
        .and_where(issue(issues::Column::ProjectId).eq(uuid_val(gate.project_id)))
        .and_where(
            issue(issues::Column::ArchivedAt)
                .is_not_null()
                .or(issue(issues::Column::DeletedAt).is_not_null()),
        );
    if let Some(extra) = updated_gt {
        stmt.and_where(extra);
    }
    stmt.order_by((a("issue"), issues::Column::CreatedAt), Order::Desc);
    stmt
}

fn non_negative(value: i64) -> Result<u64, Denial> {
    u64::try_from(value).map_err(|_| Denial::ServerError)
}

// ---- execution -------------------------------------------------------------------

/// Every row of a built statement as a JSON map, via `row_to_json`: the
/// shaping layer (field order, datetime re-rendering, the groupers) works
/// on these maps, and Postgres renders uuids, arrays, dates and numerics
/// exactly as the handlers' byte contract expects.
pub async fn fetch_rows(
    pool: &sqlx::PgPool,
    inner: SelectStatement,
) -> Result<Vec<Map<String, Value>>, Denial> {
    // raw-sql: the `row_to_json` list wrapper — built by sea-query over a
    // sea-query select (no SQL text); the rows stay JSON maps because the
    // shaping layer and the groupers consume maps.
    let mut wrapper = Query::select();
    wrapper
        .expr_as(
            SimpleExpr::from(Func::cust(a("row_to_json")).arg(Expr::col(a("__r"))))
                .cast_as(a("text")),
            a("__row"),
        )
        .from_subquery(inner, a("__r"));
    #[derive(FromQueryResult)]
    struct JsonRow {
        __row: String,
    }
    let db = pidash_db::pool::sea_orm_connection(pool);
    let rows = query_built::<JsonRow>(&db, &wrapper).await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        match serde_json::from_str(&row.__row).map_err(|_| Denial::ServerError)? {
            Value::Object(map) => out.push(map),
            _ => return Err(Denial::ServerError),
        }
    }
    Ok(out)
}

/// Run a built statement into typed rows, decoded by name.
pub(crate) async fn query_built<T: FromQueryResult>(
    db: &sea_orm::DatabaseConnection,
    stmt: &SelectStatement,
) -> Result<Vec<T>, Denial> {
    let rows = db
        .query_all(DbBackend::Postgres.build(stmt))
        .await
        .map_err(|_| Denial::ServerError)?;
    rows.iter()
        .map(|row| T::from_query_result(row, "").map_err(|_| Denial::ServerError))
        .collect()
}

#[derive(Debug, FromQueryResult)]
struct CountRow {
    n: i64,
}

/// A single `n` count.
pub async fn fetch_n(pool: &sqlx::PgPool, stmt: &SelectStatement) -> Result<i64, Denial> {
    let db = pidash_db::pool::sea_orm_connection(pool);
    let rows = query_built::<CountRow>(&db, stmt).await?;
    rows.into_iter()
        .next()
        .map(|row| row.n)
        .ok_or(Denial::ServerError)
}

#[derive(Debug, FromQueryResult)]
struct TotalRow {
    bucket: String,
    n: i64,
}

#[derive(Debug, FromQueryResult)]
struct SubTotalRow {
    bucket: String,
    sub: String,
    n: i64,
}

#[derive(Debug, FromQueryResult)]
struct BucketRow {
    bucket: Option<String>,
}

#[derive(Debug, FromQueryResult)]
struct IdRow {
    id: uuid::Uuid,
}

/// `(group, filtered count)` pairs.
pub async fn fetch_group_totals(
    pool: &sqlx::PgPool,
    stmt: &SelectStatement,
) -> Result<Vec<(String, i64)>, Denial> {
    let db = pidash_db::pool::sea_orm_connection(pool);
    Ok(query_built::<TotalRow>(&db, stmt)
        .await?
        .into_iter()
        .map(|row| (row.bucket, row.n))
        .collect())
}

/// `(group, sub, filtered count)` triples.
pub async fn fetch_sub_totals(
    pool: &sqlx::PgPool,
    stmt: &SelectStatement,
) -> Result<Vec<(String, String, i64)>, Denial> {
    let db = pidash_db::pool::sea_orm_connection(pool);
    Ok(query_built::<SubTotalRow>(&db, stmt)
        .await?
        .into_iter()
        .map(|row| (row.bucket, row.sub, row.n))
        .collect())
}

/// Distinct bucket values; NULL reads as `"None"` (`str(None)`).
pub async fn fetch_buckets(
    pool: &sqlx::PgPool,
    stmt: &SelectStatement,
) -> Result<Vec<String>, Denial> {
    let db = pidash_db::pool::sea_orm_connection(pool);
    Ok(query_built::<BucketRow>(&db, stmt)
        .await?
        .into_iter()
        .map(|row| row.bucket.unwrap_or_else(|| "None".to_owned()))
        .collect())
}

/// Issue ids, rendered as canonical hyphenated text.
pub async fn fetch_ids(pool: &sqlx::PgPool, stmt: &SelectStatement) -> Result<Vec<String>, Denial> {
    let db = pidash_db::pool::sea_orm_connection(pool);
    Ok(query_built::<IdRow>(&db, stmt)
        .await?
        .into_iter()
        .map(|row| row.id.to_string())
        .collect())
}

/// The known group buckets per field (`issue_group_values`), the three
/// queryset-derived fields over this set.
pub async fn group_values(
    pool: &sqlx::PgPool,
    field: &str,
    slug: &str,
    project_id: &uuid::Uuid,
    set: &IssueSet,
) -> Result<Vec<String>, Denial> {
    match distinct_bucket_values(set, field) {
        Some(stmt) => fetch_buckets(pool, &stmt).await,
        None => super::known_group_values(pool, field, slug, project_id).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_query::PostgresQueryBuilder;

    fn gate(guest_scoped: bool) -> Gate {
        Gate {
            user_id: uuid::Uuid::nil(),
            timezone: chrono_tz::UTC,
            workspace_id: uuid::Uuid::nil(),
            project_id: uuid::Uuid::nil(),
            guest_scoped,
        }
    }

    fn sql(stmt: &SelectStatement) -> String {
        stmt.to_string(PostgresQueryBuilder)
    }

    fn query(pairs: &[(&str, &str)]) -> QueryMap {
        let mut out = QueryMap::default();
        for (key, value) in pairs {
            out.insert(
                (*key).to_owned(),
                super::super::OneOrMany::One((*value).to_owned()),
            );
        }
        out
    }

    #[test]
    fn base_set_scopes_and_keeps_null_states() {
        let set = issue_set(
            &gate(true),
            "w",
            &query(&[]),
            None,
            None,
            None,
            false,
            FilterLayers::FULL,
        )
        .expect("set");
        let text = sql(&count_distinct(&set));
        assert!(text
            .starts_with(r#"SELECT COUNT(DISTINCT "issue"."id") AS "n" FROM "issues" AS "issue""#));
        assert!(text.contains(r#"INNER JOIN "projects" AS "project""#));
        assert!(text.contains(r#"LEFT JOIN "states" AS "state" ON "state"."id" = "issue"."state_id" AND "state"."deleted_at" IS NULL"#));
        assert!(
            text.contains(r#"("state"."group" IS NULL OR (NOT "state"."group" = 'triage'))"#),
            "{text}"
        );
        assert!(
            text.contains(r#""issue"."created_by_id" = '00000000-0000-0000-0000-000000000000'"#)
        );
        // Conditional joins: with no rich/legacy filter and no group,
        // no relation joins are rendered (intake stays for count_filter).
        // Exception: `issue_cycle` is always joined (LEFT) — the cycle-name
        // order bridge requires it in FROM (review defect 2).
        for (table, alias) in [
            ("issue_assignees", "issue_assignee"),
            ("module_issues", "issue_module"),
            ("issue_mentions", "issue_mention"),
            ("issue_labels", "label_issue"),
            ("issue_subscribers", "issue_subscribers"),
        ] {
            assert!(
                !text.contains(&format!(r#""{table}" AS "{alias}""#)),
                "unexpected join for {alias} in {text}"
            );
        }
        assert!(
            text.contains(r#"LEFT JOIN "cycle_issues" AS "issue_cycle""#),
            "cycle must always be joined for the order bridge: {text}"
        );
        assert!(text.contains(r#"LEFT JOIN "intake_issues" AS "issue_intake""#));
        assert!(set.referenced.is_empty());
        // Detail and NO_GUEST skip the guest scope; V2 renders no joins.
        let detail = issue_set(
            &gate(true),
            "w",
            &query(&[]),
            None,
            None,
            None,
            true,
            FilterLayers::FULL,
        )
        .expect("set");
        assert!(!sql(&count_distinct(&detail)).contains("created_by_id"));
        let v2 = issue_set(
            &gate(true),
            "w",
            &query(&[]),
            None,
            None,
            None,
            false,
            FilterLayers::V2,
        )
        .expect("set");
        let v2 = sql(&count_distinct(&v2));
        assert!(
            v2.contains("created_by_id")
                && !v2.contains("issue_labels")
                && !v2.contains("intake_issues")
        );
    }

    #[test]
    fn joins_are_inner_for_rich_relation_leaves_and_m2m_groups() {
        let rich = query(&[(
            "filters",
            r#"{"or":[{"label_id":"00000000-0000-0000-0000-000000000001"},{"assignee_id":""}]}"#,
        )]);
        let set = issue_set(
            &gate(false),
            "w",
            &rich,
            Some("assignees__id"),
            None,
            None,
            false,
            FilterLayers::FULL,
        )
        .expect("set");
        // The label leaf filters (INNER); the empty assignee leaf is
        // match-all but the assignee group forces INNER anyway.
        assert_eq!(set.referenced, vec!["issue_assignee", "label_issue"]);
        let text = sql(&count_distinct(&set));
        assert!(text.contains(r#"INNER JOIN "issue_labels" AS "label_issue""#));
        assert!(text.contains(r#"INNER JOIN "issue_assignees" AS "issue_assignee""#));
        // Unreferenced relations are skipped (G01 conditional joins).
        assert!(!text.contains(r#""issue_assignees" AS "issue_assignee" AND"#));
        // Cycle join is always present (LEFT) for the order bridge.
        assert!(text.contains(r#"LEFT JOIN "cycle_issues" AS "issue_cycle""#));
        // Legacy predicates keep LEFT joins but only for the referenced alias.
        let legacy = query(&[("labels", "00000000-0000-0000-0000-000000000001")]);
        let set = issue_set(
            &gate(false),
            "w",
            &legacy,
            None,
            None,
            None,
            false,
            FilterLayers::FULL,
        )
        .expect("set");
        assert!(set.referenced.is_empty());
        let legacy_text = sql(&count_distinct(&set));
        assert!(legacy_text.contains(r#"LEFT JOIN "issue_labels" AS "label_issue""#));
        // Other relations are skipped for a labels-only legacy filter
        // (cycle stays: always-joined order bridge).
        assert!(!legacy_text.contains(r#""issue_assignees" AS "issue_assignee""#));
        assert!(legacy_text.contains(r#"LEFT JOIN "cycle_issues" AS "issue_cycle""#));
    }

    #[test]
    fn annotations_pick_count_forms_and_guards() {
        let set = issue_set(
            &gate(false),
            "w",
            &query(&[]),
            None,
            None,
            None,
            false,
            FilterLayers::V2,
        )
        .expect("set");
        let grouped = sql(&v2_rows(&set, 10, 0).expect("v2"));
        assert_eq!(grouped.matches("NULLIF(COUNT(*), 0)").count(), 3);
        assert!(grouped
            .contains(r#"COALESCE(ARRAY_AGG(DISTINCT "il"."label_id"), CAST('{}' AS uuid[]))"#));
        assert!(grouped.contains(r#"EXISTS(SELECT 1 FROM "project_members" AS "pm""#));
        assert!(grouped.contains(r#"INNER JOIN "modules" AS "m""#));
        assert!(grouped.contains(r#"ORDER BY "issue"."updated_at" ASC LIMIT 10 OFFSET 0"#));
        let mut scalar = set.select();
        annotate(
            &mut scalar,
            Annotations {
                arrays: true,
                skip_array: Some("module_ids"),
                assignee_active_member: false,
                module_archived_guard: false,
                scalar_counts: true,
            },
        );
        let scalar = sql(&scalar);
        assert!(
            !scalar.contains("NULLIF")
                && !scalar.contains("module_ids")
                && !scalar.contains("project_members")
        );
    }

    #[test]
    fn order_keys_resolve_like_the_text_engine() {
        assert!(order_expr("estimate_point", "estimate_point", None, false).is_ok());
        assert!(matches!(
            order_expr("labels__name", "labels__name", None, false),
            Err(Denial::ServerError)
        ));
        assert!(matches!(
            order_expr("bogus", "bogus", None, false),
            Err(Denial::ServerError)
        ));
        let key = order_expr(
            "-issue_cycle__cycle__name",
            "-issue_cycle__cycle__name",
            None,
            false,
        )
        .expect("key");
        assert!(key.descending);
        assert_eq!(key.joins, OrderJoins::Fresh);
        let key =
            order_expr("issue_cycle__cycle__name", "x", Some("issue_cycle"), false).expect("key");
        assert_eq!(key.joins, OrderJoins::Bridge("issue_cycle"));
    }

    #[test]
    fn flat_list_rows_orders_by_selected_order_key() {
        // Review defect 1: `SELECT DISTINCT ... ORDER BY <raw expr>` is a
        // Postgres 500. The flat rows must select the order expr as
        // `__order_key` and order by the alias (the `flat_page` pattern).
        let set = issue_set(
            &gate(false),
            "w",
            &query(&[]),
            None,
            None,
            None,
            false,
            FilterLayers::FULL,
        )
        .expect("set");
        for (out_param, orig_param) in [
            ("priority_order", "priority"),
            ("-priority_order", "-priority"),
            ("state_order", "state__group"),
            ("min_values", "labels__name"),
            ("-min_values", "-labels__name"),
            ("-created_at", "-created_at"),
            ("issue_cycle__cycle__name", "issue_cycle__cycle__name"),
        ] {
            let text = sql(&flat_list_rows(&set, out_param, orig_param).expect("flat rows"));
            assert!(
                text.contains("SELECT DISTINCT"),
                "distinct missing for {out_param}: {text}"
            );
            assert!(
                text.contains(r#"AS "__order_key""#),
                "order key not selected for {out_param}: {text}"
            );
            let order_at = text.find("ORDER BY").expect("order by");
            let order_clause = &text[order_at..];
            assert!(
                order_clause.contains(r#""__order_key""#),
                "order by must reference __order_key for {out_param}: {order_clause}"
            );
            assert!(
                !order_clause.contains("CASE WHEN"),
                "raw CASE in ORDER BY under DISTINCT for {out_param}: {order_clause}"
            );
        }
    }

    #[test]
    fn cycle_bridge_renders_over_always_joined_from() {
        // Review defect 2: with conditional joins, `Bridge("issue_cycle")`
        // referenced a table that was not in FROM (500). The bridge joins
        // `__order_c` over `issue_cycle`, which must always be joined.
        let set = issue_set(
            &gate(false),
            "w",
            &query(&[]),
            None,
            None,
            None,
            false,
            FilterLayers::FULL,
        )
        .expect("set");
        let key = order_expr(
            "issue_cycle__cycle__name",
            "issue_cycle__cycle__name",
            Some("issue_cycle"),
            false,
        )
        .expect("key");
        let text = sql(&flat_page(
            &set,
            Annotations {
                arrays: true,
                skip_array: None,
                assignee_active_member: false,
                module_archived_guard: true,
                scalar_counts: true,
            },
            &key,
            10,
            0,
        )
        .expect("page"));
        assert!(
            text.contains(r#"LEFT JOIN "cycle_issues" AS "issue_cycle""#),
            "bridge source missing from FROM: {text}"
        );
        assert!(
            text.contains(r#"LEFT JOIN "cycles" AS "__order_c""#),
            "bridge join missing: {text}"
        );
        assert!(
            text.contains(r#""__order_c"."name" AS "__order_key""#),
            "order key must select the cycle name: {text}"
        );
        let flat =
            sql(
                &flat_list_rows(&set, "issue_cycle__cycle__name", "issue_cycle__cycle__name")
                    .expect("flat rows"),
            );
        assert!(
            flat.contains(r#"LEFT JOIN "cycle_issues" AS "issue_cycle""#),
            "flat rows bridge source missing: {flat}"
        );
        assert!(
            flat.contains(r#"LEFT JOIN "cycles" AS "__order_c""#),
            "flat rows bridge join missing: {flat}"
        );
    }

    #[test]
    fn grouped_page_windows_and_totals_render() {
        let set = issue_set(
            &gate(false),
            "w",
            &query(&[]),
            Some("labels__id"),
            Some("priority"),
            None,
            false,
            FilterLayers::FULL,
        )
        .expect("set");
        let key = order_expr("-created_at", "-created_at", Some("issue_cycle"), true).expect("key");
        let ann = Annotations {
            arrays: true,
            skip_array: Some("label_ids"),
            assignee_active_member: false,
            module_archived_guard: true,
            scalar_counts: false,
        };
        let page = sql(
            &grouped_page(&set, "labels__id", Some("priority"), ann, &key, 0, 3).expect("page"),
        );
        // G01: window over deduped subquery, partitions on output columns.
        assert!(
            page.contains(
                r#"ROW_NUMBER() OVER ( PARTITION BY "__dedup"."labels__id", "__dedup"."priority"#
            ),
            "{page}"
        );
        assert!(page.contains(r#""label_issue"."label_id" AS "labels__id""#));
        assert!(page.contains(r#"WHERE "__w"."__rn" > 0 AND "__w"."__rn" <= 3"#));
        assert!(page.contains("FROM (SELECT DISTINCT"), "{page}");
        let totals = sql(&sub_totals(&set, "labels__id", "priority").expect("totals"));
        assert!(totals.contains(r#"COUNT(DISTINCT (CASE WHEN"#), "{totals}");
        assert!(totals.ends_with(r#"GROUP BY "bucket", "sub""#));
    }

    #[test]
    fn legacy_predicates_bind_and_cast() {
        let tz: Tz = "Asia/Kolkata".parse().expect("tz");
        let render = |expr: SimpleExpr| {
            let mut stmt = Query::select();
            stmt.expr(Expr::val(1)).and_where(expr);
            sql(&stmt)
        };
        let text = render(
            legacy_expr(
                "created_at__date__gte",
                &FilterValue::Day(chrono::NaiveDate::from_ymd_opt(2026, 1, 2).expect("day")),
                &tz,
            )
            .expect("expr"),
        );
        assert!(text.contains(r#"CAST(("issue"."created_at" AT TIME ZONE 'Asia/Kolkata') AS date) >= CAST('2026-01-02' AS date)"#), "{text}");
        let text = render(
            legacy_expr(
                "name__icontains",
                &FilterValue::Text("50%_".to_owned()),
                &tz,
            )
            .expect("expr"),
        );
        assert!(
            text.contains(r#""issue"."name" ILIKE E'%50\\%\\_%'"#),
            "{text}"
        );
        assert!(matches!(
            legacy_expr("logged_by__in", &FilterValue::Uuids(vec![]), &tz),
            Err(Denial::ServerError)
        ));
        assert_eq!(
            render(legacy_expr("state__in", &FilterValue::Uuids(vec![]), &tz).expect("expr")),
            "SELECT 1 WHERE FALSE"
        );
    }
}
