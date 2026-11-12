//! Module read querysets (PIDASHCONV-308, D-20 stage 5).
//!
//! Ports the five `get_queryset` chains in
//! `apps/api/pi_dash/api/views/module.py` — plus the two `GET` inline
//! `Issue` querysets that shadow the issue querysets on the wire — to
//! executable SeaORM sea-query statements (drift baseline `01a93e17`):
//!
//! * M1 `ModuleListCreateAPIEndpoint.get_queryset` (`module.py:85-171`)
//!   + the list `GET` archived filter (`:272`).
//! * M2 `ModuleDetailAPIEndpoint.get_queryset` (`module.py:288-374`)
//!   + the detail `GET` archived + pk lookup (`:473`).
//! * M3 `ModuleArchiveUnarchiveAPIEndpoint.get_queryset`
//!   (`module.py:895-982`, `archived_at__isnull=False` at `:899`).
//! * M4 `ModuleIssueListCreateAPIEndpoint.get_queryset` (`module.py:545-569`)
//!   vs the list `GET` inline queryset (`:601-634`).
//! * M5 `ModuleIssueDetailAPIEndpoint.get_queryset` (`module.py:751-775`)
//!   vs the detail `GET` inline queryset (`:800-849`); the detail
//!   `DELETE` lookup (`:870-875`) shares the M4 scope plus the issue id.
//!
//! Fixture oracle: FX-CYCMOD-05 (`fixtures/v1_cycles_modules/queries/`
//! `module.sql` M1-M5 + `module.rows.json`). Every SQL shape below was
//! verified token-by-token against live Django 4.2 `str(queryset.query)`
//! output (test settings, fixed UUIDs); the unit tests pin the fragments
//! so transcription drift fails the build.
//!
//! # Builder contract
//!
//! Each builder returns a complete sea-query [`SelectStatement`] with its
//! values bound (the workspace slug, the project id, the module id, the
//! acting user, the issue id), so the handlers run it through SeaORM
//! (`DbBackend::Postgres.build(&stmt)` → `query_all`) and decode the rows
//! by name. No SQL text is written here: Django's
//! `Count(..., distinct=True, filter=...)` (`COUNT(DISTINCT x) FILTER
//! (WHERE ...)`) is the `COUNT(DISTINCT CASE WHEN <filter> THEN x END)`
//! form (sea-query 0.32 has no aggregate `FILTER` clause — the same
//! count, since `COUNT` skips the `NULL` the unmatched rows produce, and
//! `DISTINCT` still sees at most one value per group), the correlated
//! subqueries are nested selects, and the group literals (`'completed'`,
//! `'triage'`, `'ISSUE_ATTACHMENT'`) bind as values — the same values
//! Django sends as parameters at execution.
//!
//! `ORDER BY` is dynamic: `.order_by(self.kwargs.get("order_by", ...))`
//! passes the kwarg through untouched, so [`OrderBy`] carries the raw
//! column plus direction and the builder quotes it onto the base table —
//! an unknown column fails at the database exactly like Django's
//! `FieldError`-at-evaluation. (PIDASHCONV-510: the M4/M5 GET
//! `?order_by=` additionally resolves Django-side to `?` → `RANDOM()`,
//! single-level FK traversals onto the already-joined tables, bare
//! output aliases for the two annotations that exist at `.order_by()`
//! time, and bare-M2M default orderings with extra joins; PIDASHCONV-511:
//! bare-FK names resolve to the related model's `Meta.ordering` terms,
//! with extra joins where the M4 GET lacks them — [`OrderTarget`] carries
//! those shapes and [`apply_order`] renders them identically in every
//! builder.)
//!
//! # Reads scope tables, not views
//!
//! Like `queries_stateest`, the builders read the physical tables with
//! explicit `deleted_at IS NULL` conjuncts (matching Django's SQL
//! exactly) rather than the `<table>_active` views: the views bake in the
//! same predicate, but the Django text is the contract under test and the
//! `LEFT OUTER JOIN` annotation fanout needs the join conditions visible.
//! Writes still hit the tables (`super::module` docs).
//!
//! # Ported bugs and asymmetries (translate, don't redesign)
//!
//! 1. Grouped counts are `COUNT(DISTINCT "states"."group")`, so each of
//!    `completed/cancelled/started/unstarted/backlog_issues` is **0 or 1**,
//!    never a real count (`Count("issue_module__issue__state__group",
//!    distinct=True)` counts distinct group values, and the `FILTER`
//!    restricts to one group). Only `total_issues`
//!    (`COUNT(DISTINCT "module_issues"."id")`) counts rows. Ported as
//!    observed via [`count_issues_expr`]; the 0/1 shape is pinned by
//!    `grouped_counts_are_zero_or_one_distinct_group`.
//! 2. M1/M2/M3 carry **no member-visibility join** (unlike the cycle
//!    querysets, `cycle.py:93-96`): any caller in the project lists every
//!    module; membership is enforced by `ProjectEntityPermission` only.
//!    Ported as observed — [`module_list`] joins no `project_members`.
//! 3. M1/M2 omit the trailing `.distinct()` the cycle querysets keep;
//!    only the `DISTINCT` *inside* each `Count` remains. M4/M5 querysets
//!    keep `SELECT DISTINCT`. Ported as observed.
//! 4. M3 has **no estimate annotations**, unlike archived cycles Q3
//!    (`cycle.py:699-721`). Ported as observed: [`archived_module_list`]
//!    is M1 with the archived predicate and nothing else.
//! 5. Relation traversals use the model's **base** manager, so no
//!    `IssueManager` guards (triage / archived issue / archived project /
//!    draft) ride the M1 annotation joins or the M4/M5-queryset outer
//!    `issues` join: the explicit `Q` (bridge `deleted_at`, issue
//!    `archived_at`, `is_draft`) is the only issue filter there, and
//!    triage-group issues **count toward the totals**. `Issue.issue_objects`
//!    paths (M4/M5 GET outer + both `sub_issues_count` subqueries) DO carry
//!    the full manager guards. Both shapes are kept, never unified.
//! 6. The M5 detail `GET` (`module.py:800-849`) is **unreachable on the
//!    wire**: `api/urls/module.py:33-37` allows only `delete` on the
//!    `module-issues-detail` route (cycles allow `get` + `delete`).
//!    [`module_issue_detail_get`] ports the shape anyway (the fixture
//!    M5 covers it); handlers wire only `delete`.
//! 7. `sub_issues_count` counts children in **every project**: the
//!    correlated subquery restricts `parent_id` only (no project/module
//!    scope). Ported as observed.
//!
//! Out of scope (sibling D-20 issues): response envelopes and serializer
//! field selection (handlers, PIDASHCONV-406), `ProjectEntityPermission`
//! gates (PIDASHCONV-309), activity enqueues (PIDASHCONV-310), prefetch
//! round trips (`members`, `link_module`, `assignees`, `labels` arrive via
//! separate queries — no SQL fanout — and stay handler-owned).

use sea_query::{
    Alias, Asterisk, CaseStatement, Condition, Expr, Func, IntoIden, JoinType, Order, Query,
    SelectStatement, SimpleExpr, TableRef,
};
use uuid::Uuid;

use super::module::{self, module_issue};
use crate::v1_projects::models::{estimate_point, project, project_member, state};

/// `workspaces` table (no db-layer port owns it yet; literal matches the
/// Django table name, same as `queries_stateest::WORKSPACE_TABLE`).
const WORKSPACE_TABLE: &str = "workspaces";
/// `issues` table (owned by the issues-domain port; literal matches the
/// Django table name — the M4/M5 GET projection owner).
const ISSUE_TABLE: &str = "issues";
/// `users` table behind `select_related("lead")` (M1/M2/M3).
const USER_TABLE: &str = "users";
/// `issue_links` table behind `link_count` (M4/M5 GET).
const ISSUE_LINK_TABLE: &str = "issue_links";
/// `file_assets` table behind `attachment_count` (M4/M5 GET).
const FILE_ASSET_TABLE: &str = "file_assets";
/// `labels` table behind bare-M2M `order_by=labels` (M4/M5 GET,
/// PIDASHCONV-510).
const LABEL_TABLE: &str = "labels";
/// `issue_assignees` through table behind `order_by=assignees`.
const ISSUE_ASSIGNEE_TABLE: &str = "issue_assignees";
/// `issue_labels` through table behind `order_by=labels`.
const ISSUE_LABEL_TABLE: &str = "issue_labels";
/// `pod` table behind bare-FK `order_by=assigned_pod` (M4/M5 GET,
/// PIDASHCONV-511; `runner.Pod`, `db_table = "pod"`, singular).
const POD_TABLE: &str = "pod";
/// Alias of the parent self-join (`select_related("parent")`, M4/M5 GET);
/// `order_by=parent__<col>` renders onto it (PIDASHCONV-510).
pub const PARENT_ALIAS: &str = "T7";

/// The tenant scope every module read binds: `workspaces.slug` and the
/// project id (`module.py:87-88` et al.).
#[derive(Debug, Clone, Copy)]
pub struct TenantScope<'a> {
    pub slug: &'a str,
    pub project_id: Uuid,
}

/// `"<table>"."<column>"`.
fn col(table: &str, column: &str) -> Expr {
    Expr::col((Alias::new(table), Alias::new(column)))
}

/// A scalar subquery as a select item.
fn scalar(sub: SelectStatement) -> SimpleExpr {
    SimpleExpr::SubQuery(None, Box::new(sub.into_sub_query_statement()))
}

/// `"<table>"` as a `FROM`/`JOIN` target.
fn table(name: &str) -> Alias {
    Alias::new(name)
}

// ---------------------------------------------------------------------------
// Shared scope inputs
// ---------------------------------------------------------------------------

/// Which `archived_at` predicate a module read applies
/// (`module.py:272` live, `:473` live + pk, `:899` archived).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchivedFilter {
    /// `archived_at IS NULL` (M1 list GET, M2 detail GET).
    Live,
    /// `archived_at IS NOT NULL` (M3 archived list).
    Archived,
    /// No archived predicate (bare querysets; M4/M5 issue paths, which
    /// never filter on the module's `archived_at` at all).
    Any,
}

/// Where an [`OrderBy`] renders its term (PIDASHCONV-510: the M4/M5
/// GET `?order_by=` resolves Django-side to more than base-table
/// columns — every shape below was read off live Django 4.2
/// `str(queryset.query)` output for the M4 GET chain).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderTarget {
    /// `ORDER BY "<base>"."<column>"` — plain columns, FK `<name>_id`
    /// columns, and unknown text (which fails at the database exactly
    /// like Django's `FieldError`-at-evaluation). Every pre-510 shape.
    Base,
    /// `ORDER BY "<table>"."<column>"` onto an already-joined table or
    /// alias (see [`issue_traversal_table`]). The tail passes through
    /// raw, so an unresolvable tail fails at the database exactly like
    /// Django's `FieldError` — no column allowlist needed.
    Table(&'static str),
    /// `ORDER BY "<column>"` — a bare output alias. Django renders the
    /// early annotations positionally (`ORDER BY 35`); the alias is the
    /// same semantics without depending on the `issues.*` width. Only
    /// for annotations that exist at `.order_by()` time
    /// (`sub_issues_count`, `bridge_id`); `link_count` and
    /// `attachment_count` are annotated after and stay [`OrderTarget::Base`]
    /// pass-through 500s, like Django's `FieldError` for them.
    Alias,
    /// `ORDER BY RANDOM() ASC` — Django `?` (the compiler yields an
    /// ascending `Random()`; `-?` is a `FieldError`, never random, so
    /// only the exact `?` maps here and the flag always renders `ASC`).
    Random,
    /// A bare-M2M default ordering: the builder adds the two `LEFT JOIN`s
    /// and orders by the related model's `Meta.ordering` term.
    M2M(M2MOrder),
    /// A bare-FK (or reverse-FK) default ordering: the related model's
    /// `Meta.ordering` terms, with the extra `LEFT JOIN` when the M4
    /// GET does not already join the table ([`RelatedOrder`]).
    Related(RelatedOrder),
}

/// The bare-M2M names the M4 GET resolves (`assignees`, `labels`).
/// Both related models order `('-created_at',)` (`db/models/user.py:137`,
/// `db/models/label.py:44`), so plain `assignees` renders
/// `ORDER BY "users"."created_at" DESC` and the `-` prefix inverts it
/// to `ASC` (verified against live `str(query)` output).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum M2MOrder {
    /// `LEFT JOIN "issue_assignees"` + `LEFT JOIN "users"`.
    Assignees,
    /// `LEFT JOIN "issue_labels"` + `LEFT JOIN "labels"`.
    Labels,
}

impl M2MOrder {
    /// `(through_table, through_issue_fk, through_target_fk, target_table)`.
    fn tables(self) -> (&'static str, &'static str, &'static str, &'static str) {
        match self {
            M2MOrder::Assignees => (ISSUE_ASSIGNEE_TABLE, "issue_id", "assignee_id", USER_TABLE),
            M2MOrder::Labels => (ISSUE_LABEL_TABLE, "issue_id", "label_id", LABEL_TABLE),
        }
    }
}

/// The bare-FK (and reverse-FK) names the M4 GET resolves to the
/// related model's `Meta.ordering` (PIDASHCONV-511): Django orders
/// `order_by=<fk>` by the related ordering, *not* by `<name>_id` —
/// every shape below was read off live Django 4.2 `str(queryset.query)`
/// output for the M4 GET chain. The request `-` inverts each term.
/// `type` is deliberately NOT here: `IssueType` has no
/// `Meta.ordering`, so Django orders `type` by the local
/// `"issues"."type_id"` with no join at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelatedOrder {
    /// `ORDER BY "states"."sequence"` (`State.Meta.ordering`,
    /// `db/models/state.py:129`; no new join — `select_related`).
    State,
    /// `ORDER BY "projects"."created_at" DESC` (`Project.Meta.ordering
    /// = ('-created_at',)`, `db/models/project.py:253`; no new join).
    Project,
    /// `ORDER BY "workspaces"."created_at" DESC`
    /// (`Workspace.Meta.ordering`, `db/models/workspace.py:182`).
    Workspace,
    /// `ORDER BY "T7"."created_at" DESC` (`Issue.Meta.ordering`,
    /// `db/models/issue.py:254`, onto the parent self-join alias).
    Parent,
    /// `LEFT JOIN "users"` on `created_by_id` +
    /// `ORDER BY "users"."created_at" DESC` (`User.Meta.ordering`,
    /// `db/models/user.py:137`).
    CreatedBy,
    /// `LEFT JOIN "users"` on `updated_by_id` + the same term.
    UpdatedBy,
    /// `LEFT JOIN "estimate_points"` +
    /// `ORDER BY "estimate_points"."value" ASC`
    /// (`EstimatePoint.Meta.ordering`, `db/models/estimate.py:57`;
    /// `value` is a varchar — string sort, ported as-is).
    EstimatePoint,
    /// `LEFT JOIN "pod"` + TWO terms:
    /// `ORDER BY "pod"."is_default" DESC, "pod"."created_at" ASC`
    /// (`Pod.Meta.ordering = ('-is_default', 'created_at')`,
    /// `runner/models.py`; the `-` inverts both).
    AssignedPod,
    /// `ORDER BY "module_issues"."created_at" DESC` (bare reverse FK
    /// onto the already-joined bridge table;
    /// `ModuleIssue.Meta.ordering`, `db/models/module.py:168`).
    IssueModule,
}

impl RelatedOrder {
    /// The raw `?order_by=` name.
    fn name(self) -> &'static str {
        match self {
            RelatedOrder::State => "state",
            RelatedOrder::Project => "project",
            RelatedOrder::Workspace => "workspace",
            RelatedOrder::Parent => "parent",
            RelatedOrder::CreatedBy => "created_by",
            RelatedOrder::UpdatedBy => "updated_by",
            RelatedOrder::EstimatePoint => "estimate_point",
            RelatedOrder::AssignedPod => "assigned_pod",
            RelatedOrder::IssueModule => "issue_module",
        }
    }

    /// The extra `LEFT JOIN` the terms need, as `(table,
    /// issues_fk_column)` — `None` when the M4 GET already joins the
    /// table (`state`/`project`/`workspace`/`parent` via
    /// `select_related`, `issue_module` via the filter traversal).
    /// Django appends these joins after the parent `T7` join.
    fn join(self) -> Option<(&'static str, &'static str)> {
        match self {
            RelatedOrder::CreatedBy => Some((USER_TABLE, "created_by_id")),
            RelatedOrder::UpdatedBy => Some((USER_TABLE, "updated_by_id")),
            RelatedOrder::EstimatePoint => Some((estimate_point::TABLE, "estimate_point_id")),
            RelatedOrder::AssignedPod => Some((POD_TABLE, "assigned_pod_id")),
            RelatedOrder::State
            | RelatedOrder::Project
            | RelatedOrder::Workspace
            | RelatedOrder::Parent
            | RelatedOrder::IssueModule => None,
        }
    }

    /// The related `Meta.ordering` terms as `(table, column,
    /// model_descending)`; the request `-` XORs each term's direction
    /// (verified per name against live `str(query)` output).
    fn terms(self) -> &'static [(&'static str, &'static str, bool)] {
        match self {
            RelatedOrder::State => &[(state::TABLE, "sequence", false)],
            RelatedOrder::Project => &[(project::TABLE, "created_at", true)],
            RelatedOrder::Workspace => &[(WORKSPACE_TABLE, "created_at", true)],
            RelatedOrder::Parent => &[(PARENT_ALIAS, "created_at", true)],
            RelatedOrder::CreatedBy | RelatedOrder::UpdatedBy => {
                &[(USER_TABLE, "created_at", true)]
            }
            RelatedOrder::EstimatePoint => &[(estimate_point::TABLE, "value", false)],
            RelatedOrder::AssignedPod => &[
                (POD_TABLE, "is_default", true),
                (POD_TABLE, "created_at", false),
            ],
            RelatedOrder::IssueModule => &[(module_issue::TABLE, "created_at", true)],
        }
    }
}

/// A parsed `.order_by(...)` argument: the raw column plus direction.
///
/// Django passes `self.kwargs.get("order_by", <default>)` through
/// untouched (`module.py:170,373,567,773,981`; M4/M5 GET use
/// `request.GET.get("order_by", "created_at")` at `:600,806`). The
/// leading `-` selects descending; anything else is ascending, including
/// Django's verbatim behavior for unknown columns (database error at
/// evaluation, like `FieldError`). Parsing lives in the services layer
/// (`services::v1_cycles_modules::module_queries`, PIDASHCONV-308) and,
/// for the M4/M5 GET `?order_by=`, in the handler's `resolve_issue_order`
/// (PIDASHCONV-406); [`OrderTarget::Base`] quotes the column onto the
/// base table while the other targets render per [`apply_order`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderBy {
    /// Raw column text (e.g. `created_at`); the raw `?` token for
    /// [`OrderTarget::Random`] and the raw M2M name for
    /// [`OrderTarget::M2M`].
    pub column: String,
    /// True when the raw text starts with `-`.
    pub descending: bool,
    /// Where the term renders (default [`OrderTarget::Base`]).
    pub target: OrderTarget,
}

impl OrderBy {
    /// `OrderBy` for a literal `.order_by(...)` argument.
    pub fn new(column: impl Into<String>, descending: bool) -> Self {
        Self {
            column: column.into(),
            descending,
            target: OrderTarget::Base,
        }
    }

    /// `OrderBy` for a traversal onto an already-joined table or alias.
    pub fn table(table: &'static str, column: impl Into<String>, descending: bool) -> Self {
        Self {
            column: column.into(),
            descending,
            target: OrderTarget::Table(table),
        }
    }

    /// `OrderBy` for a bare output alias (early annotations).
    pub fn alias(column: impl Into<String>, descending: bool) -> Self {
        Self {
            column: column.into(),
            descending,
            target: OrderTarget::Alias,
        }
    }

    /// `OrderBy` for the exact `?` token (random).
    pub fn random() -> Self {
        Self {
            column: "?".to_owned(),
            descending: false,
            target: OrderTarget::Random,
        }
    }

    /// `OrderBy` for a bare-M2M name.
    pub fn m2m(which: M2MOrder, descending: bool) -> Self {
        let column = match which {
            M2MOrder::Assignees => "assignees",
            M2MOrder::Labels => "labels",
        };
        Self {
            column: column.to_owned(),
            descending,
            target: OrderTarget::M2M(which),
        }
    }

    /// `OrderBy` for a bare-FK (or reverse-FK) name: the related
    /// model's `Meta.ordering` terms (PIDASHCONV-511).
    pub fn related(which: RelatedOrder, descending: bool) -> Self {
        Self {
            column: which.name().to_owned(),
            descending,
            target: OrderTarget::Related(which),
        }
    }

    /// The M1/M2/M3/M4-queryset/M5-queryset default (`"-created_at"`).
    pub fn default_module() -> Self {
        Self::new("created_at", true)
    }

    /// The M4/M5 GET default (`"created_at"`, ascending).
    pub fn default_issue() -> Self {
        Self::new("created_at", false)
    }

    fn order(&self) -> Order {
        if self.descending {
            Order::Desc
        } else {
            Order::Asc
        }
    }
}

/// Map a single-level `?order_by=` traversal head to its already-joined
/// table or alias in the M4/M5 GET builders (`None` = not a supported
/// head; the caller passes the name through to 500 like Django's
/// `FieldError`). Deeper paths (`state__project__name`) and relations
/// needing new joins (`created_by__email`, `assignees__email`) resolve
/// Django-side to 200s the builder cannot express — recorded residual
/// divergences (PIDASHCONV-510), still 500 here.
pub fn issue_traversal_table(head: &str) -> Option<&'static str> {
    match head {
        "state" => Some(state::TABLE),
        "project" => Some(project::TABLE),
        "workspace" => Some(WORKSPACE_TABLE),
        "parent" => Some(PARENT_ALIAS),
        "issue_module" => Some(module_issue::TABLE),
        _ => None,
    }
}

/// Append the `ORDER BY` term for `order` onto `sel`, whose base table
/// is `base` — the one rendering helper every builder calls, so exotic
/// targets render identically wherever they appear. Non-`Base` targets
/// only reach the M4/M5 GET builders (kwargs chains always carry the
/// default); rendered elsewhere they fail at the database exactly like
/// Django's `FieldError` for the same name on those chains.
fn apply_order(sel: &mut SelectStatement, base: &str, order: &OrderBy) {
    match order.target {
        OrderTarget::Base => {
            sel.order_by(
                (
                    Alias::new(base.to_owned()),
                    Alias::new(order.column.clone()),
                ),
                order.order(),
            );
        }
        OrderTarget::Table(table) => {
            sel.order_by(
                (
                    Alias::new(table.to_owned()),
                    Alias::new(order.column.clone()),
                ),
                order.order(),
            );
        }
        OrderTarget::Alias => {
            sel.order_by(Alias::new(order.column.clone()), order.order());
        }
        // Django's compiler yields `OrderBy(Random())` ascending — the
        // `-` flag never survives onto `?` (`-?` is a FieldError).
        OrderTarget::Random => {
            sel.order_by_expr(Func::random().into(), Order::Asc);
        }
        // Related `Meta.ordering` is `('-created_at',)`: the request `-`
        // inverts it, so plain M2M renders `DESC`.
        OrderTarget::M2M(which) => {
            let (_, _, _, target) = which.tables();
            let direction = if order.descending {
                Order::Asc
            } else {
                Order::Desc
            };
            sel.order_by(
                (
                    Alias::new(target.to_owned()),
                    Alias::new("created_at".to_owned()),
                ),
                direction,
            );
        }
        // Related `Meta.ordering` terms (PIDASHCONV-511): the request
        // `-` XORs each term's model direction, so a `-created_at`
        // term renders `DESC` plain / `ASC` inverted while an
        // ascending term (`sequence`, `value`) renders the opposite.
        OrderTarget::Related(which) => {
            for (table, column, model_descending) in which.terms() {
                let direction = if order.descending != *model_descending {
                    Order::Desc
                } else {
                    Order::Asc
                };
                sel.order_by(
                    (
                        Alias::new((*table).to_owned()),
                        Alias::new((*column).to_owned()),
                    ),
                    direction,
                );
            }
        }
    }
}

/// Project one table's `columns` as `"table"."col"` selects, in order.
fn select_table_columns(sel: &mut SelectStatement, table_name: &str, columns: &[&str]) {
    for column in columns {
        sel.column((Alias::new(table_name), Alias::new(*column)));
    }
}

/// `INNER JOIN "projects" ON ("<table>"."project_id" = "projects"."id")`
/// (every `select_related("project")`).
fn join_project(sel: &mut SelectStatement, from: &str) {
    sel.join(
        JoinType::InnerJoin,
        table(project::TABLE),
        col(from, "project_id").equals((Alias::new(project::TABLE), Alias::new("id"))),
    );
}

/// `INNER JOIN "workspaces" ON ("<table>"."workspace_id" = "workspaces"."id")`
/// (every `select_related("workspace")`).
fn join_workspace(sel: &mut SelectStatement, from: &str) {
    sel.join(
        JoinType::InnerJoin,
        table(WORKSPACE_TABLE),
        col(from, "workspace_id").equals((Alias::new(WORKSPACE_TABLE), Alias::new("id"))),
    );
}

/// `LEFT JOIN "<to>" ON ("<from>"."<from_col>" = "<to>"."<to_col>")`.
fn left_join(sel: &mut SelectStatement, from: &str, from_col: &str, to: &str, to_col: &str) {
    sel.join(
        JoinType::LeftJoin,
        table(to),
        col(from, from_col).equals((Alias::new(to), Alias::new(to_col))),
    );
}

/// `INNER JOIN "<to>" ON ("<from>"."<from_col>" = "<to>"."<to_col>")`.
fn inner_join(sel: &mut SelectStatement, from: &str, from_col: &str, to: &str, to_col: &str) {
    sel.join(
        JoinType::InnerJoin,
        table(to),
        col(from, from_col).equals((Alias::new(to), Alias::new(to_col))),
    );
}

/// `LEFT JOIN "<table>" ON ("issues"."<fk>" = "<table>"."id")` — one
/// ordering-only to-one join (bare-FK related orderings), appended after
/// the parent `T7` join like Django.
fn join_order_fk(sel: &mut SelectStatement, target: &str, fk: &str) {
    sel.join(
        JoinType::LeftJoin,
        table(target),
        col(ISSUE_TABLE, fk).equals((Alias::new(target), Alias::new("id"))),
    );
}

/// Tenant scope shared by the module reads: this project, this workspace
/// slug (`module.py:87-88,290-291,553-554,760-761,897-898`). Conjunct
/// order follows Django (slug, then project). Soft-delete scoping
/// (`deleted_at IS NULL`) comes from each model's manager and is applied
/// per table at the call site — `projects` / `workspaces` carry NO
/// deleted predicate (verified against live Django SQL: only the base
/// table's manager scope renders).
fn tenant_condition(base: &str, scope: &TenantScope<'_>) -> Condition {
    Condition::all()
        .add(col(WORKSPACE_TABLE, "slug").eq(scope.slug))
        .add(col(base, "project_id").eq(scope.project_id))
}

/// Member-visibility conjuncts (`project__project_projectmember` traversal,
/// the M4/M5 querysets): the acting user is an active member of the
/// project. Inlined at each call site (flat `AND` chain, like Django)
/// rather than nested: Django renders the `is_active=True` conjunct
/// bare; sea-query renders `= TRUE` — identical semantics.
fn add_member_scope(scope: Condition, actor_id: Uuid) -> Condition {
    scope
        .add(col(project_member::TABLE, "is_active").eq(true))
        .add(col(project_member::TABLE, "member_id").eq(actor_id))
}

// ---------------------------------------------------------------------------
// M1/M2/M3 count annotations
// ---------------------------------------------------------------------------

/// State-group restriction of a count annotation (`module.py:110-169`).
/// `All` is the total (no group filter).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupFilter {
    /// No group restriction (`total_issues`).
    All,
    /// `issue_module__issue__state__group = '<group>'`.
    Eq(&'static str),
}

/// The five state groups in annotation order (`module.py:111-168`);
/// `None` is the total (no group filter).
pub const COUNT_GROUPS: &[Option<&str>] = &[
    Some("completed"),
    Some("cancelled"),
    Some("started"),
    Some("unstarted"),
    Some("backlog"),
    None,
];

/// The six count annotation aliases in source order (`module.py:99-169`).
pub const COUNT_ALIASES: &[&str] = &[
    "total_issues",
    "completed_issues",
    "cancelled_issues",
    "started_issues",
    "unstarted_issues",
    "backlog_issues",
];

/// The `(group, alias)` pairs of the five grouped counts, in source order.
const GROUPED_COUNTS: &[(&str, &str)] = &[
    ("completed", "completed_issues"),
    ("cancelled", "cancelled_issues"),
    ("started", "started_issues"),
    ("unstarted", "unstarted_issues"),
    ("backlog", "backlog_issues"),
];

/// Explicit issue-liveness filter shared by all six counts
/// (`module.py:102-106` and each group `Q`): the bridge is live, the
/// issue is not archived and not a draft. There is deliberately NO
/// triage / project-archived guard — relation traversals use the base
/// manager (ported bug 5).
fn live_issue_filter() -> Condition {
    Condition::all()
        .add(col(module_issue::TABLE, "deleted_at").is_null())
        .add(col(ISSUE_TABLE, "archived_at").is_null())
        .add(col(ISSUE_TABLE, "is_draft").not())
}

/// `COUNT(DISTINCT CASE WHEN <filter> THEN <counted> END)`: Django's
/// `Count(..., distinct=True, filter=Q(...))` (`COUNT(DISTINCT x) FILTER
/// (WHERE ...)`) — the same count, since `COUNT` skips the `NULL` the
/// unmatched rows yield and `DISTINCT` still sees only the counted
/// values (at most one distinct group value under a single-group filter,
/// i.e. 0/1 — ported bug 1).
fn count_distinct_where(filter: Condition, counted: Expr) -> SimpleExpr {
    let case = CaseStatement::new().case(filter, counted);
    Func::count_distinct(SimpleExpr::Case(Box::new(case))).into()
}

/// One `Count(...)` annotation (`module.py:99-169`):
///
/// * total (`GroupFilter::All`): `COUNT(DISTINCT "module_issues"."id")`
///   over the live filter — a real row count.
/// * grouped: `COUNT(DISTINCT "states"."group")` over the live filter
///   plus `"states"."group" = '<group>'` — the count of distinct group
///   values under a single-group filter, i.e. **0 or 1** (ported bug 1).
pub fn count_issues_expr(filter: GroupFilter) -> SimpleExpr {
    match filter {
        GroupFilter::All => {
            count_distinct_where(live_issue_filter(), col(module_issue::TABLE, "id"))
        }
        GroupFilter::Eq(group) => count_distinct_where(
            live_issue_filter().add(col(state::TABLE, "group").eq(group)),
            col(state::TABLE, "group"),
        ),
    }
}

/// All six count annotations in source order, onto the M1/M2/M3 select
/// list (`total_issues` first, then the five groups).
fn add_count_annotations(sel: &mut SelectStatement) {
    sel.expr_as(
        count_issues_expr(GroupFilter::All),
        Alias::new("total_issues"),
    );
    for (group, alias) in GROUPED_COUNTS {
        sel.expr_as(
            count_issues_expr(GroupFilter::Eq(group)),
            Alias::new(*alias),
        );
    }
}

// ---------------------------------------------------------------------------
// M1/M2/M3 module reads
// ---------------------------------------------------------------------------

/// Shared M1/M2/M3 `FROM` + `JOIN` block (`module.py:87-98,290-301,897-909`):
/// tenant tables inner-joined (`select_related("project")`,
/// `select_related("workspace")`), annotation sources left-joined
/// (`issue_module` reverse FK, `issue`, `state`), lead left-joined
/// (`select_related("lead")`). `members` and `link_module` are
/// `prefetch_related` — separate round trips, no join here (ported bug 2
/// documents the missing member-visibility join: there is none at all).
fn join_module_reads(sel: &mut SelectStatement) {
    join_project(sel, module::TABLE);
    join_workspace(sel, module::TABLE);
    left_join(sel, module::TABLE, "id", module_issue::TABLE, "module_id");
    left_join(sel, module_issue::TABLE, "issue_id", ISSUE_TABLE, "id");
    left_join(sel, ISSUE_TABLE, "state_id", state::TABLE, "id");
    left_join(sel, module::TABLE, "lead_id", USER_TABLE, "id");
}

/// Shared M1/M2/M3 `WHERE` head: the base-table manager scope
/// (`modules.deleted_at IS NULL`) plus the tenant scope. The
/// `projects` / `workspaces` joins carry no deleted predicate
/// (verified against live Django SQL).
fn module_list_where(scope: &TenantScope<'_>) -> Condition {
    Condition::all()
        .add(col(module::TABLE, "deleted_at").is_null())
        .add(tenant_condition(module::TABLE, scope))
}

/// The M1/M2/M3 select head: base columns in [`module::COLUMNS`] order
/// plus the six counts, `FROM "modules"` with the shared joins. No
/// `SELECT DISTINCT` (ported bug 3 — the asymmetry vs cycles).
fn module_read_head() -> SelectStatement {
    let mut sel = Query::select();
    select_table_columns(&mut sel, module::TABLE, module::COLUMNS);
    add_count_annotations(&mut sel);
    sel.from(table(module::TABLE));
    join_module_reads(&mut sel);
    sel
}

/// M1 module list (`ModuleListCreateAPIEndpoint.get_queryset`,
/// `module.py:85-171`, + `archived_at IS NULL` from the list `GET` at
/// `:272`): base columns in [`module::COLUMNS`] order, the six count
/// annotations, `GROUP BY "modules"."id"`, kwargs order.
///
/// Django groups by all four joined PKs; the tenant/lead joins are N:1
/// off `modules.id`, so grouping by `modules.id` alone yields identical
/// groups (documented projection narrowing: joined-table columns are
/// re-added by the handlers layer when serializing nested objects, per
/// the `queries_stateest` precedent — the row set is unchanged).
pub fn module_list(
    order: &OrderBy,
    archived: ArchivedFilter,
    scope: &TenantScope<'_>,
) -> SelectStatement {
    let mut sel = module_read_head();
    let mut where_ = module_list_where(scope);
    match archived {
        ArchivedFilter::Live => {
            where_ = where_.add(col(module::TABLE, "archived_at").is_null());
        }
        ArchivedFilter::Archived => {
            where_ = where_.add(col(module::TABLE, "archived_at").is_not_null());
        }
        ArchivedFilter::Any => {}
    }
    sel.cond_where(where_);
    sel.group_by_col((Alias::new(module::TABLE), Alias::new("id")));
    apply_order(&mut sel, module::TABLE, order);
    sel
}

/// M2 module detail (`ModuleDetailAPIEndpoint.get_queryset`,
/// `module.py:288-374`, + `.filter(archived_at__isnull=True).get(pk=pk)`
/// from the detail `GET` at `:473`): identical to M1 live plus the pk
/// predicate. The chain text is byte-identical to M1's; only the GET
/// wrapper differs.
pub fn module_detail(order: &OrderBy, scope: &TenantScope<'_>, module_id: Uuid) -> SelectStatement {
    let mut sel = module_read_head();
    sel.cond_where(
        module_list_where(scope)
            .add(col(module::TABLE, "archived_at").is_null())
            .add(col(module::TABLE, "id").eq(module_id)),
    );
    sel.group_by_col((Alias::new(module::TABLE), Alias::new("id")));
    apply_order(&mut sel, module::TABLE, order);
    sel
}

/// M3 archived-module list (`ModuleArchiveUnarchiveAPIEndpoint.get_queryset`,
/// `module.py:895-982`): same chain as M1 with
/// `archived_at__isnull=False` (`:899`) and NO estimate annotations
/// (ported bug 4 — the asymmetry vs archived cycles is intentional).
pub fn archived_module_list(order: &OrderBy, scope: &TenantScope<'_>) -> SelectStatement {
    module_list(order, ArchivedFilter::Archived, scope)
}

// ---------------------------------------------------------------------------
// M4/M5 issue querysets (ModuleIssue-based)
// ---------------------------------------------------------------------------

/// The `IssueManager` guards (`db/models/issue.py:95-104`) spelled onto
/// the given `issues` / `states` / `projects` identifiers: soft-delete,
/// non-triage states (NULL-safe — the `states` join is a `LEFT OUTER
/// JOIN`), live issues, live projects, non-drafts. Django's `NOT (x IS
/// NOT NULL)` arms render as the equivalent `x IS NULL`.
fn issue_manager_guards(issues: &str, states: &str, projects: &str) -> Condition {
    Condition::all()
        .add(col(issues, "deleted_at").is_null())
        .add(
            Condition::all()
                .add(col(states, "group").eq("triage"))
                .add(col(states, "group").is_not_null())
                .not(),
        )
        .add(col(issues, "archived_at").is_null())
        .add(col(projects, "archived_at").is_null())
        .add(col(issues, "is_draft").not())
}

/// Outer `IssueManager` guards for the M4/M5 GET shape
/// (`Issue.issue_objects` entry points — never relation traversals,
/// ported bug 5), on the plain `issues` / `states` / `projects` tables.
pub fn issue_manager_outer_guards() -> Condition {
    issue_manager_guards(ISSUE_TABLE, state::TABLE, project::TABLE)
}

/// `sub_issues_count` correlated subquery (`module.py:547-552,753-758`,
/// verified against live `str(query)` output):
/// `Issue.issue_objects.filter(parent=OuterRef(...))` — so the FULL
/// `IssueManager` guards render inside (triage / archived issue /
/// archived project / draft excluded, soft-delete excluded), with the
/// `states` join nullable (`LEFT OUTER JOIN`, hence the NULL-safe triage
/// guard). `Count` is NOT distinct (`Func(F("id"), function="Count")`).
/// The subquery restricts `parent_id` ONLY — no project or module scope
/// (ported bug 7).
///
/// `outer` is the correlated parent column: `"module_issues"."issue_id"`
/// for the M4/M5 querysets, `"issues"."id"` for the M4/M5 GET shapes.
pub fn sub_issues_count_expr(outer: Expr) -> SimpleExpr {
    let (u0, u1, u2) = ("U0", "U1", "U2");
    let mut sub = Query::select();
    sub.expr(Func::count(col(u0, "id")))
        .from_as(table(ISSUE_TABLE), Alias::new(u0))
        .join_as(
            JoinType::LeftJoin,
            table(state::TABLE),
            Alias::new(u1),
            col(u0, "state_id").equals((Alias::new(u1), Alias::new("id"))),
        )
        .join_as(
            JoinType::InnerJoin,
            table(project::TABLE),
            Alias::new(u2),
            col(u0, "project_id").equals((Alias::new(u2), Alias::new("id"))),
        )
        .cond_where(issue_manager_guards(u0, u1, u2).add(col(u0, "parent_id").eq(outer)));
    scalar(sub)
}

/// M4/M5 queryset (`ModuleIssueListCreateAPIEndpoint.get_queryset`,
/// `module.py:545-569`; M5's `module.py:751-775` is chain-identical):
/// `SELECT DISTINCT` bridge columns in [`module_issue::COLUMNS`] order +
/// [`sub_issues_count_expr`] correlated to the bridge's issue.
///
/// Joins (verified order): `issues` inner (`select_related("issue",
/// "issue__state", "issue__project")` — the issue's own project arrives
/// as the `T8` self-join), `workspaces` inner, `projects` inner,
/// `modules` inner (`select_related("module")`), `project_members`
/// inner (the member-active visibility traversal), `projects T8` inner
/// (the issue's project), `states` left outer. The outer `issues` join
/// carries NO `IssueManager` guards — the traversal uses the base
/// manager (ported bug 5): archived / draft / triage issues list while
/// their bridge is live. There is NO module-`archived_at` predicate —
/// issues of archived modules still list.
///
/// `WHERE` order follows Django: bridge deleted scope, slug, project,
/// module, member `is_active` + member id, `projects.archived_at IS NULL`.
pub fn module_issue_queryset(
    order: &OrderBy,
    scope: &TenantScope<'_>,
    module_id: Uuid,
    actor_id: Uuid,
) -> SelectStatement {
    let mut sel = Query::select();
    sel.distinct();
    select_table_columns(&mut sel, module_issue::TABLE, module_issue::COLUMNS);
    sel.expr_as(
        sub_issues_count_expr(col(module_issue::TABLE, "issue_id")),
        Alias::new("sub_issues_count"),
    );
    sel.from(table(module_issue::TABLE));
    inner_join(&mut sel, module_issue::TABLE, "issue_id", ISSUE_TABLE, "id");
    join_workspace(&mut sel, module_issue::TABLE);
    join_project(&mut sel, module_issue::TABLE);
    inner_join(
        &mut sel,
        module_issue::TABLE,
        "module_id",
        module::TABLE,
        "id",
    );
    inner_join(
        &mut sel,
        project::TABLE,
        "id",
        project_member::TABLE,
        "project_id",
    );
    // INNER JOIN "projects" AS "T8" ON ("issues"."project_id" = T8."id")
    // (select_related("issue__project")).
    sel.join(
        JoinType::InnerJoin,
        TableRef::Table(table(project::TABLE).into_iden()).alias(Alias::new("T8")),
        col(ISSUE_TABLE, "project_id").equals((Alias::new("T8"), Alias::new("id"))),
    );
    left_join(&mut sel, ISSUE_TABLE, "state_id", state::TABLE, "id");
    let where_ = Condition::all()
        .add(col(module_issue::TABLE, "deleted_at").is_null())
        .add(col(WORKSPACE_TABLE, "slug").eq(scope.slug))
        .add(col(module_issue::TABLE, "project_id").eq(scope.project_id))
        .add(col(module_issue::TABLE, "module_id").eq(module_id));
    let where_ =
        add_member_scope(where_, actor_id).add(col(project::TABLE, "archived_at").is_null());
    sel.cond_where(where_);
    apply_order(&mut sel, module_issue::TABLE, order);
    sel
}

/// `link_count` scalar annotation (M4/M5 GET, `module.py:619-624,829-834`,
/// verified against live `str(query)` output): `IssueLink.objects` uses
/// the soft-delete default manager, so `deleted_at IS NULL` renders
/// (unlike the unguarded traversal in `sub_issues_count`'s outer query).
pub fn link_count_expr() -> SimpleExpr {
    let u0 = "U0";
    let mut sub = Query::select();
    sub.expr(Func::count(col(u0, "id")))
        .from_as(table(ISSUE_LINK_TABLE), Alias::new(u0))
        .cond_where(
            Condition::all()
                .add(col(u0, "deleted_at").is_null())
                .add(col(u0, "issue_id").eq(col(ISSUE_TABLE, "id"))),
        );
    scalar(sub)
}

/// `attachment_count` scalar annotation (M4/M5 GET, `module.py:625-633,
/// 835-843`): same soft-delete manager scope plus the
/// `entity_type = 'ISSUE_ATTACHMENT'` literal
/// (`FileAsset.EntityTypeContext.ISSUE_ATTACHMENT`).
pub fn attachment_count_expr() -> SimpleExpr {
    let u0 = "U0";
    let mut sub = Query::select();
    sub.expr(Func::count(col(u0, "id")))
        .from_as(table(FILE_ASSET_TABLE), Alias::new(u0))
        .cond_where(
            Condition::all()
                .add(col(u0, "deleted_at").is_null())
                .add(col(u0, "entity_type").eq("ISSUE_ATTACHMENT"))
                .add(col(u0, "issue_id").eq(col(ISSUE_TABLE, "id"))),
        );
    scalar(sub)
}

/// `bridge_id` annotation (M4/M5 GET, `module.py:609,819`):
/// `.annotate(bridge_id=F("issue_module__id"))` — the live bridge's pk
/// via the `INNER JOIN "module_issues"`. One row per live bridge: an
/// issue bridged twice (deleted-then-recreated rows excepted by the
/// `deleted_at` guard) fans out into two rows — ported as observed.
pub fn bridge_id_expr() -> SimpleExpr {
    col(module_issue::TABLE, "id").into()
}

/// M4 issue list `GET` inline queryset (`module.py:601-634`): the shape
/// the wire actually serves (the `post` re-read at `:731` uses the M4
/// queryset instead). `Issue.issue_objects` outer with the four
/// annotations (`sub_issues_count`, `bridge_id`, `link_count`,
/// `attachment_count`), tenant scope, `ORDER BY "issues"."created_at"
/// ASC` default (`request.GET.get("order_by", "created_at")` at `:600` —
/// note the ASC default vs the queryset's `-created_at`).
///
/// Joins (verified order): `states` left outer, `projects` inner,
/// `module_issues` inner (the `issue_module__module_id` traversal),
/// `workspaces` inner, parent `T7` left outer (`select_related("parent")`).
/// No `DISTINCT`, no member join, no module-`archived_at` predicate.
///
/// The `issues` projection is `"issues".*`: Django projects every issue
/// column explicitly; same row content. (The issues-domain port owns the
/// typed column list; this layer ports the scope, joins and annotations.)
pub fn module_issue_list_get(
    order: &OrderBy,
    scope: &TenantScope<'_>,
    module_id: Uuid,
) -> SelectStatement {
    module_issue_get_inner(order, None, scope, module_id)
}

/// M5 issue detail `GET` inline queryset (`module.py:807-843`): M4 GET
/// plus `pk=issue_id`. Unreachable on the wire (ported bug 6 — the
/// `module-issues-detail` route allows `delete` only); ported because
/// fixture M5 covers it and the domain gate may exercise it.
pub fn module_issue_detail_get(
    order: &OrderBy,
    scope: &TenantScope<'_>,
    module_id: Uuid,
    issue_id: Uuid,
) -> SelectStatement {
    module_issue_get_inner(order, Some(issue_id), scope, module_id)
}

fn module_issue_get_inner(
    order: &OrderBy,
    issue_id: Option<Uuid>,
    scope: &TenantScope<'_>,
    module_id: Uuid,
) -> SelectStatement {
    let mut sel = Query::select();
    sel.column((Alias::new(ISSUE_TABLE), Asterisk));
    sel.expr_as(
        sub_issues_count_expr(col(ISSUE_TABLE, "id")),
        Alias::new("sub_issues_count"),
    );
    sel.expr_as(bridge_id_expr(), Alias::new("bridge_id"));
    sel.expr_as(link_count_expr(), Alias::new("link_count"));
    sel.expr_as(attachment_count_expr(), Alias::new("attachment_count"));
    sel.from(table(ISSUE_TABLE));
    left_join(&mut sel, ISSUE_TABLE, "state_id", state::TABLE, "id");
    join_project(&mut sel, ISSUE_TABLE);
    inner_join(&mut sel, ISSUE_TABLE, "id", module_issue::TABLE, "issue_id");
    join_workspace(&mut sel, ISSUE_TABLE);
    // LEFT OUTER JOIN "issues" AS "T7" ON ("issues"."parent_id" = T7."id")
    // (select_related("parent")).
    sel.join(
        JoinType::LeftJoin,
        TableRef::Table(table(ISSUE_TABLE).into_iden()).alias(Alias::new(PARENT_ALIAS)),
        col(ISSUE_TABLE, "parent_id").equals((Alias::new(PARENT_ALIAS), Alias::new("id"))),
    );
    // Bare-M2M ordering joins (PIDASHCONV-510), after T7 like Django:
    // `LEFT JOIN "<through>"` + `LEFT JOIN "<target>"`, no DISTINCT —
    // rows multiply per through-row exactly like the Django queryset.
    if let OrderTarget::M2M(which) = order.target {
        let (through, issue_fk, target_fk, target) = which.tables();
        left_join(&mut sel, ISSUE_TABLE, "id", through, issue_fk);
        left_join(&mut sel, through, target_fk, target, "id");
    }
    // Bare-FK related-ordering joins (PIDASHCONV-511), after T7 like
    // Django: one `LEFT JOIN "<table>" ON ("issues"."<fk>" =
    // "<table>"."id")`. To-one, so rows never multiply (unlike M2M).
    if let OrderTarget::Related(which) = order.target {
        if let Some((target, fk)) = which.join() {
            join_order_fk(&mut sel, target, fk);
        }
    }
    let mut scope_ = issue_manager_outer_guards()
        .add(col(module_issue::TABLE, "deleted_at").is_null())
        .add(col(module_issue::TABLE, "module_id").eq(module_id))
        .add(col(ISSUE_TABLE, "project_id").eq(scope.project_id))
        .add(col(WORKSPACE_TABLE, "slug").eq(scope.slug));
    if let Some(issue_id) = issue_id {
        scope_ = scope_.add(col(ISSUE_TABLE, "id").eq(issue_id));
    }
    sel.cond_where(scope_);
    apply_order(&mut sel, ISSUE_TABLE, order);
    sel
}

/// M5 detail `DELETE` lookup (`ModuleIssueDetailAPIEndpoint.delete`,
/// `module.py:870-875`): `ModuleIssue.objects.get(workspace__slug,
/// project_id, module_id, issue_id)` — default-manager scope
/// (`deleted_at IS NULL`) plus the four equality predicates. Django's
/// `.get()` adds no `LIMIT` (0 rows → `DoesNotExist` → 404, 2+ →
/// `MultipleObjectsReturned` → 500); handlers own that mapping.
pub fn module_issue_delete_lookup(
    scope: &TenantScope<'_>,
    module_id: Uuid,
    issue_id: Uuid,
) -> SelectStatement {
    let mut sel = Query::select();
    select_table_columns(&mut sel, module_issue::TABLE, module_issue::COLUMNS);
    sel.from(table(module_issue::TABLE));
    join_workspace(&mut sel, module_issue::TABLE);
    sel.cond_where(
        Condition::all()
            .add(col(module_issue::TABLE, "deleted_at").is_null())
            .add(col(WORKSPACE_TABLE, "slug").eq(scope.slug))
            .add(col(module_issue::TABLE, "project_id").eq(scope.project_id))
            .add(col(module_issue::TABLE, "module_id").eq(module_id))
            .add(col(module_issue::TABLE, "issue_id").eq(issue_id)),
    );
    sel
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_query::PostgresQueryBuilder;

    const SLUG: &str = "acme";
    fn project_id() -> Uuid {
        Uuid::from_u128(0xA)
    }
    fn module_id() -> Uuid {
        Uuid::from_u128(0xB)
    }
    fn actor_id() -> Uuid {
        Uuid::from_u128(0xD)
    }
    fn issue_id() -> Uuid {
        Uuid::from_u128(0xE)
    }
    fn scope() -> TenantScope<'static> {
        TenantScope {
            slug: SLUG,
            project_id: project_id(),
        }
    }
    fn render(sel: &SelectStatement) -> String {
        sel.to_string(PostgresQueryBuilder)
    }
    fn lit(id: Uuid) -> String {
        format!("'{id}'")
    }

    /// The literal `CASE` filter every M1 count carries.
    const LIVE_FILTER: &str = r#""module_issues"."deleted_at" IS NULL AND "issues"."archived_at" IS NULL AND (NOT "issues"."is_draft")"#;

    #[test]
    fn m1_list_live_scope_and_joins() {
        // FX-CYCMOD-05 M1 (`module.py:85-171` + `:272`).
        let sql = render(&module_list(
            &OrderBy::default_module(),
            ArchivedFilter::Live,
            &scope(),
        ));
        assert!(sql.contains(r#"FROM "modules""#), "{sql}");
        assert!(
            sql.contains(r#"INNER JOIN "projects" ON "modules"."project_id" = "projects"."id""#),
            "{sql}"
        );
        assert!(
            sql.contains(
                r#"INNER JOIN "workspaces" ON "modules"."workspace_id" = "workspaces"."id""#
            ),
            "{sql}"
        );
        // NOTE: sea-query renders `LEFT JOIN`; Django renders the
        // semantically identical `LEFT OUTER JOIN`.
        assert!(
            sql.contains(
                r#"LEFT JOIN "module_issues" ON "modules"."id" = "module_issues"."module_id""#
            ),
            "{sql}"
        );
        assert!(
            sql.contains(r#"LEFT JOIN "issues" ON "module_issues"."issue_id" = "issues"."id""#),
            "{sql}"
        );
        assert!(
            sql.contains(r#"LEFT JOIN "states" ON "issues"."state_id" = "states"."id""#),
            "{sql}"
        );
        assert!(
            sql.contains(r#"LEFT JOIN "users" ON "modules"."lead_id" = "users"."id""#),
            "{sql}"
        );
        // Ported bug 2: no member-visibility join (unlike cycle Q1).
        assert!(!sql.contains("project_members"), "{sql}");
        // No trailing DISTINCT (ported asymmetry vs cycles).
        assert!(!sql.contains("SELECT DISTINCT"), "{sql}");
        // Tenant scope + live filter, values bound.
        assert!(sql.contains(r#""modules"."deleted_at" IS NULL"#), "{sql}");
        assert!(
            sql.contains(&format!(
                r#""workspaces"."slug" = '{SLUG}' AND "modules"."project_id" = {}"#,
                lit(project_id())
            )),
            "{sql}"
        );
        assert!(sql.contains(r#""modules"."archived_at" IS NULL"#), "{sql}");
        // Only the base table carries a deleted predicate.
        assert!(!sql.contains(r#""projects"."deleted_at""#), "{sql}");
        assert!(!sql.contains(r#""workspaces"."deleted_at""#), "{sql}");
        // No IssueManager guards ride the annotation joins.
        assert!(!sql.contains("triage"), "{sql}");
        assert!(sql.contains(r#"GROUP BY "modules"."id""#), "{sql}");
        assert!(
            sql.ends_with(r#"ORDER BY "modules"."created_at" DESC"#),
            "{sql}"
        );
    }

    #[test]
    fn grouped_counts_are_zero_or_one_distinct_group() {
        // FX-CYCMOD-05 M1 (`module.py:99-169`): total counts bridge rows,
        // each grouped count tallies DISTINCT group values (0/1) as
        // `COUNT(DISTINCT CASE WHEN ...)`.
        let sql = render(&module_list(
            &OrderBy::default_module(),
            ArchivedFilter::Live,
            &scope(),
        ));
        assert!(
            sql.contains(&format!(
                r#"COUNT(DISTINCT (CASE WHEN ({LIVE_FILTER}) THEN "module_issues"."id" END)) AS "total_issues""#
            )),
            "{sql}"
        );
        for (group, alias) in GROUPED_COUNTS {
            assert!(
                sql.contains(&format!(
                    r#"COUNT(DISTINCT (CASE WHEN ({LIVE_FILTER} AND "states"."group" = '{group}') THEN "states"."group" END)) AS "{alias}""#
                )),
                "{sql}"
            );
        }
        assert!(!sql.contains("FILTER"), "{sql}");
        assert_eq!(
            COUNT_ALIASES.len(),
            6,
            "six annotations serve the fixture row keys"
        );
        assert_eq!(COUNT_GROUPS.len(), 6);
        for alias in COUNT_ALIASES {
            assert_eq!(
                sql.matches(&format!(r#" AS "{alias}""#)).count(),
                1,
                "{alias}"
            );
        }
    }

    #[test]
    fn m1_bare_queryset_carries_no_archived_predicate() {
        let sql = render(&module_list(
            &OrderBy::default_module(),
            ArchivedFilter::Any,
            &scope(),
        ));
        // The projected `"modules"."archived_at"` column stays (it is a
        // real column); only the WHERE predicate is absent.
        assert!(!sql.contains(r#""modules"."archived_at" IS"#), "{sql}");
    }

    #[test]
    fn m2_detail_adds_live_plus_pk() {
        // FX-CYCMOD-05 M2 (`module.py:288-374` + `:473`).
        let sql = render(&module_detail(
            &OrderBy::default_module(),
            &scope(),
            module_id(),
        ));
        assert!(sql.contains(r#""modules"."archived_at" IS NULL"#), "{sql}");
        assert!(
            sql.contains(&format!(
                r#""modules"."id" = {} GROUP BY"#,
                lit(module_id())
            )),
            "{sql}"
        );
        assert!(!sql.contains("project_members"), "{sql}");
        assert!(!sql.contains("SELECT DISTINCT"), "{sql}");
        // Same head as M1: select list and joins are identical.
        let live = render(&module_list(
            &OrderBy::default_module(),
            ArchivedFilter::Live,
            &scope(),
        ));
        let head = |s: &str| s.split(" WHERE ").next().expect("head").to_owned();
        assert_eq!(head(&sql), head(&live));
    }

    #[test]
    fn m3_archived_flips_predicate_and_adds_no_estimates() {
        // FX-CYCMOD-05 M3 (`module.py:895-982`): same chain, archived
        // predicate, no estimate annotations (ported bug 4).
        let sql = render(&archived_module_list(&OrderBy::default_module(), &scope()));
        assert!(
            sql.contains(r#""modules"."archived_at" IS NOT NULL"#),
            "{sql}"
        );
        assert!(!sql.contains(r#""modules"."archived_at" IS NULL"#), "{sql}");
        assert!(!sql.to_lowercase().contains("estimate"), "{sql}");
        assert!(sql.contains(r#"AS "total_issues""#), "{sql}");
    }

    #[test]
    fn m1_order_passthrough() {
        // kwargs `.order_by(...)` passes through untouched.
        let sql = render(&module_list(
            &OrderBy::new("name", false),
            ArchivedFilter::Live,
            &scope(),
        ));
        assert!(sql.ends_with(r#"ORDER BY "modules"."name" ASC"#), "{sql}");
    }

    #[test]
    fn m4_get_order_random_renders_random_asc() {
        // PIDASHCONV-510: `?order_by=?` → Django `ORDER BY RANDOM() ASC`,
        // no extra joins.
        let sql = render(&module_issue_list_get(
            &OrderBy::random(),
            &scope(),
            module_id(),
        ));
        assert!(sql.contains("ORDER BY RANDOM() ASC"), "{sql}");
        assert!(!sql.contains("issue_assignees"), "{sql}");
        assert!(!sql.contains("issue_labels"), "{sql}");
    }

    #[test]
    fn m4_get_order_traversal_renders_qualified() {
        // PIDASHCONV-510: `state__group` reuses the select_related join.
        let sql = render(&module_issue_list_get(
            &OrderBy::table("states", "group", false),
            &scope(),
            module_id(),
        ));
        assert!(sql.contains(r#"ORDER BY "states"."group" ASC"#), "{sql}");
        let sql = render(&module_issue_list_get(
            &OrderBy::table("states", "group", true),
            &scope(),
            module_id(),
        ));
        assert!(sql.contains(r#"ORDER BY "states"."group" DESC"#), "{sql}");
        // Parent traversals render onto the T7 self-join alias.
        let sql = render(&module_issue_list_get(
            &OrderBy::table(PARENT_ALIAS, "created_at", false),
            &scope(),
            module_id(),
        ));
        assert!(sql.contains(r#"ORDER BY "T7"."created_at" ASC"#), "{sql}");
    }

    #[test]
    fn m4_get_order_alias_renders_bare() {
        // PIDASHCONV-510: early annotations order by bare alias (Django
        // renders the positional `ORDER BY 35` — same semantics, without
        // depending on the `issues.*` width).
        let sql = render(&module_issue_list_get(
            &OrderBy::alias("sub_issues_count", false),
            &scope(),
            module_id(),
        ));
        assert!(sql.contains(r#"ORDER BY "sub_issues_count" ASC"#), "{sql}");
        assert!(!sql.contains(r#""issues"."sub_issues_count""#), "{sql}");
    }

    #[test]
    fn m4_get_order_m2m_joins_and_inverts_direction() {
        // PIDASHCONV-510: `assignees` — through + target LEFT JOINs after
        // T7, related Meta.ordering ('-created_at',) as-is → DESC ...
        let sql = render(&module_issue_list_get(
            &OrderBy::m2m(M2MOrder::Assignees, false),
            &scope(),
            module_id(),
        ));
        let t7 = sql.find(r#""T7""#).expect("T7 join");
        let through = sql
            .find(r#"LEFT JOIN "issue_assignees" ON "issues"."id" = "issue_assignees"."issue_id""#)
            .expect("through join");
        assert!(through > t7, "{sql}");
        assert!(
            sql.contains(r#"LEFT JOIN "users" ON "issue_assignees"."assignee_id" = "users"."id""#),
            "{sql}"
        );
        assert!(
            sql.contains(r#"ORDER BY "users"."created_at" DESC"#),
            "{sql}"
        );
        // ... and the `-` prefix inverts it to ASC.
        let sql = render(&module_issue_list_get(
            &OrderBy::m2m(M2MOrder::Assignees, true),
            &scope(),
            module_id(),
        ));
        assert!(
            sql.contains(r#"ORDER BY "users"."created_at" ASC"#),
            "{sql}"
        );
        // `labels` mirrors via issue_labels/labels.
        let sql = render(&module_issue_list_get(
            &OrderBy::m2m(M2MOrder::Labels, false),
            &scope(),
            module_id(),
        ));
        assert!(
            sql.contains(
                r#"LEFT JOIN "issue_labels" ON "issues"."id" = "issue_labels"."issue_id""#
            ),
            "{sql}"
        );
        assert!(
            sql.contains(r#"LEFT JOIN "labels" ON "issue_labels"."label_id" = "labels"."id""#),
            "{sql}"
        );
        assert!(
            sql.contains(r#"ORDER BY "labels"."created_at" DESC"#),
            "{sql}"
        );
    }

    #[test]
    fn m4_get_order_related_terms_and_joins() {
        // PIDASHCONV-511: bare FKs order by the related Meta.ordering,
        // read off live Django 4.2 str(query) output for the M4 GET.
        // Already-joined tables add no join: `state` renders the
        // ascending `sequence` term ...
        let sql = render(&module_issue_list_get(
            &OrderBy::related(RelatedOrder::State, false),
            &scope(),
            module_id(),
        ));
        assert!(sql.contains(r#"ORDER BY "states"."sequence" ASC"#), "{sql}");
        let sql = render(&module_issue_list_get(
            &OrderBy::related(RelatedOrder::State, true),
            &scope(),
            module_id(),
        ));
        assert!(
            sql.contains(r#"ORDER BY "states"."sequence" DESC"#),
            "{sql}"
        );
        // ... while `-created_at` orderings render DESC plain / ASC
        // inverted, reusing the select_related / bridge joins.
        for (which, term) in [
            (
                RelatedOrder::Project,
                r#"ORDER BY "projects"."created_at" DESC"#,
            ),
            (
                RelatedOrder::Workspace,
                r#"ORDER BY "workspaces"."created_at" DESC"#,
            ),
            (RelatedOrder::Parent, r#"ORDER BY "T7"."created_at" DESC"#),
            (
                RelatedOrder::IssueModule,
                r#"ORDER BY "module_issues"."created_at" DESC"#,
            ),
        ] {
            let sql = render(&module_issue_list_get(
                &OrderBy::related(which, false),
                &scope(),
                module_id(),
            ));
            assert!(sql.contains(term), "{sql}");
            let inverted = term.replace("DESC", "ASC");
            let sql = render(&module_issue_list_get(
                &OrderBy::related(which, true),
                &scope(),
                module_id(),
            ));
            assert!(sql.contains(&inverted), "{sql}");
        }
        // New joins render after T7, like Django: `created_by` and
        // `updated_by` each LEFT JOIN `users` on their own FK ...
        for (which, join) in [
            (
                RelatedOrder::CreatedBy,
                r#"LEFT JOIN "users" ON "issues"."created_by_id" = "users"."id""#,
            ),
            (
                RelatedOrder::UpdatedBy,
                r#"LEFT JOIN "users" ON "issues"."updated_by_id" = "users"."id""#,
            ),
        ] {
            let sql = render(&module_issue_list_get(
                &OrderBy::related(which, false),
                &scope(),
                module_id(),
            ));
            let t7 = sql.find(r#""T7""#).expect("T7 join");
            let extra = sql.find(join).expect("ordering join");
            assert!(extra > t7, "{sql}");
            assert!(
                sql.contains(r#"ORDER BY "users"."created_at" DESC"#),
                "{sql}"
            );
            let sql = render(&module_issue_list_get(
                &OrderBy::related(which, true),
                &scope(),
                module_id(),
            ));
            assert!(
                sql.contains(r#"ORDER BY "users"."created_at" ASC"#),
                "{sql}"
            );
        }
        // ... `estimate_point` joins `estimate_points` for the
        // ascending `value` term ...
        let sql = render(&module_issue_list_get(
            &OrderBy::related(RelatedOrder::EstimatePoint, false),
            &scope(),
            module_id(),
        ));
        assert!(
            sql.contains(
                r#"LEFT JOIN "estimate_points" ON "issues"."estimate_point_id" = "estimate_points"."id""#
            ),
            "{sql}"
        );
        assert!(
            sql.contains(r#"ORDER BY "estimate_points"."value" ASC"#),
            "{sql}"
        );
        let sql = render(&module_issue_list_get(
            &OrderBy::related(RelatedOrder::EstimatePoint, true),
            &scope(),
            module_id(),
        ));
        assert!(
            sql.contains(r#"ORDER BY "estimate_points"."value" DESC"#),
            "{sql}"
        );
        // ... and `assigned_pod` joins `pod` for TWO terms (the `-`
        // inverts both).
        let sql = render(&module_issue_list_get(
            &OrderBy::related(RelatedOrder::AssignedPod, false),
            &scope(),
            module_id(),
        ));
        assert!(
            sql.contains(r#"LEFT JOIN "pod" ON "issues"."assigned_pod_id" = "pod"."id""#),
            "{sql}"
        );
        let first = sql
            .find(r#"ORDER BY "pod"."is_default" DESC, "pod"."created_at" ASC"#)
            .expect("two-term ordering");
        assert!(first > 0, "{sql}");
        let sql = render(&module_issue_list_get(
            &OrderBy::related(RelatedOrder::AssignedPod, true),
            &scope(),
            module_id(),
        ));
        assert!(
            sql.contains(r#"ORDER BY "pod"."is_default" ASC, "pod"."created_at" DESC"#),
            "{sql}"
        );
    }

    #[test]
    fn m5_get_shares_exotic_order_targets() {
        // PIDASHCONV-510: M5 GET shares the inner builder — M2M joins
        // render there too, plus the pk predicate.
        let sql = render(&module_issue_detail_get(
            &OrderBy::m2m(M2MOrder::Assignees, false),
            &scope(),
            module_id(),
            issue_id(),
        ));
        assert!(sql.contains(r#"LEFT JOIN "issue_assignees""#), "{sql}");
        assert!(
            sql.contains(r#"ORDER BY "users"."created_at" DESC"#),
            "{sql}"
        );
        assert!(
            sql.contains(&format!(r#""issues"."id" = {} "#, lit(issue_id()))),
            "{sql}"
        );
        // PIDASHCONV-511: related-ordering joins render there too.
        let sql = render(&module_issue_detail_get(
            &OrderBy::related(RelatedOrder::AssignedPod, false),
            &scope(),
            module_id(),
            issue_id(),
        ));
        assert!(sql.contains(r#"LEFT JOIN "pod""#), "{sql}");
        assert!(
            sql.contains(r#"ORDER BY "pod"."is_default" DESC, "pod"."created_at" ASC"#),
            "{sql}"
        );
    }

    #[test]
    fn traversal_head_table_mapping() {
        // PIDASHCONV-510: only heads onto already-joined tables map;
        // relations needing new joins stay `None` (pass-through 500s).
        assert_eq!(issue_traversal_table("state"), Some("states"));
        assert_eq!(issue_traversal_table("project"), Some("projects"));
        assert_eq!(issue_traversal_table("workspace"), Some("workspaces"));
        assert_eq!(issue_traversal_table("parent"), Some(PARENT_ALIAS));
        assert_eq!(issue_traversal_table("issue_module"), Some("module_issues"));
        assert_eq!(issue_traversal_table("created_by"), None);
        assert_eq!(issue_traversal_table("assignees"), None);
    }

    #[test]
    fn m4_queryset_distinct_member_scope_and_subquery() {
        // FX-CYCMOD-05 M4 queryset (`module.py:545-569`).
        let sql = render(&module_issue_queryset(
            &OrderBy::default_module(),
            &scope(),
            module_id(),
            actor_id(),
        ));
        assert!(sql.contains("SELECT DISTINCT"), "{sql}");
        assert!(sql.contains(r#"FROM "module_issues""#), "{sql}");
        assert!(
            sql.contains(r#"INNER JOIN "issues" ON "module_issues"."issue_id" = "issues"."id""#),
            "{sql}"
        );
        assert!(
            sql.contains(r#"INNER JOIN "modules" ON "module_issues"."module_id" = "modules"."id""#),
            "{sql}"
        );
        assert!(
            sql.contains(r#"INNER JOIN "project_members" ON "projects"."id" = "project_members"."project_id""#),
            "{sql}"
        );
        assert!(
            sql.contains(r#"INNER JOIN "projects" AS "T8" ON "issues"."project_id" = "T8"."id""#),
            "{sql}"
        );
        assert!(
            sql.contains(r#"LEFT JOIN "states" ON "issues"."state_id" = "states"."id""#),
            "{sql}"
        );
        assert!(
            sql.contains(r#""module_issues"."deleted_at" IS NULL"#),
            "{sql}"
        );
        assert!(
            sql.contains(&format!(
                r#""module_issues"."project_id" = {}"#,
                lit(project_id())
            )),
            "{sql}"
        );
        assert!(
            sql.contains(&format!(
                r#""module_issues"."module_id" = {}"#,
                lit(module_id())
            )),
            "{sql}"
        );
        assert!(
            sql.contains(r#""project_members"."is_active" = TRUE"#),
            "{sql}"
        );
        assert!(
            sql.contains(&format!(
                r#""project_members"."member_id" = {}"#,
                lit(actor_id())
            )),
            "{sql}"
        );
        assert!(sql.contains(r#""projects"."archived_at" IS NULL"#), "{sql}");
        // Outer WHERE (the final WHERE) carries no manager guards
        // (ported bug 5)...
        let outer_where = sql.rsplit("WHERE ").next().unwrap_or("");
        // ...but the correlated subquery does (IssueManager inside).
        assert!(
            sql.contains(r#""U0"."parent_id" = "module_issues"."issue_id""#),
            "{sql}"
        );
        assert!(
            sql.contains(r#"NOT ("U1"."group" = 'triage' AND "U1"."group" IS NOT NULL)"#),
            "{sql}"
        );
        // No module-archived predicate: archived modules' issues list.
        assert!(!outer_where.contains(r#""modules"."archived_at""#), "{sql}");
        assert!(
            sql.contains(r#"ORDER BY "module_issues"."created_at" DESC"#),
            "{sql}"
        );
    }

    #[test]
    fn m4_get_issue_shape_annotations_and_guards() {
        // FX-CYCMOD-05 M4 GET (`module.py:601-634`).
        let sql = render(&module_issue_list_get(
            &OrderBy::default_issue(),
            &scope(),
            module_id(),
        ));
        assert!(!sql.contains("SELECT DISTINCT"), "{sql}");
        assert!(sql.contains(r#"FROM "issues""#), "{sql}");
        assert!(sql.contains(r#""issues"."deleted_at" IS NULL"#), "{sql}");
        assert!(
            sql.contains(r#"NOT ("states"."group" = 'triage' AND "states"."group" IS NOT NULL)"#),
            "{sql}"
        );
        assert!(
            sql.contains(r#""module_issues"."deleted_at" IS NULL"#),
            "{sql}"
        );
        assert!(
            sql.contains(&format!(
                r#""module_issues"."module_id" = {}"#,
                lit(module_id())
            )),
            "{sql}"
        );
        assert!(
            sql.contains(&format!(r#""issues"."project_id" = {}"#, lit(project_id()))),
            "{sql}"
        );
        assert!(
            sql.contains(&format!(r#""workspaces"."slug" = '{SLUG}'"#)),
            "{sql}"
        );
        assert!(
            sql.contains(r#""module_issues"."id" AS "bridge_id""#),
            "{sql}"
        );
        assert!(
            sql.contains(
                r#"FROM "issue_links" AS "U0" WHERE "U0"."deleted_at" IS NULL AND "U0"."issue_id" = "issues"."id""#
            ),
            "{sql}"
        );
        assert!(
            sql.contains(r#""U0"."entity_type" = 'ISSUE_ATTACHMENT'"#),
            "{sql}"
        );
        assert!(
            sql.contains(r#"LEFT JOIN "issues" AS "T7" ON "issues"."parent_id" = "T7"."id""#),
            "{sql}"
        );
        assert!(!sql.contains("project_members"), "{sql}");
        assert!(
            sql.contains(r#"ORDER BY "issues"."created_at" ASC"#),
            "{sql}"
        );
    }

    #[test]
    fn m5_get_is_m4_get_plus_issue_pk() {
        // FX-CYCMOD-05 M5 GET (`module.py:807-843`).
        let m4 = render(&module_issue_list_get(
            &OrderBy::default_issue(),
            &scope(),
            module_id(),
        ));
        let m5 = render(&module_issue_detail_get(
            &OrderBy::default_issue(),
            &scope(),
            module_id(),
            issue_id(),
        ));
        assert!(
            m5.contains(&format!(r#""issues"."id" = {}"#, lit(issue_id()))),
            "{m5}"
        );
        assert!(
            !m4.contains(&format!(r#""issues"."id" = {}"#, lit(issue_id()))),
            "{m4}"
        );
        // Every M4 fragment is in M5; the only extra predicate is the pk.
        let m5_without_pk = m5.replace(&format!(r#" AND "issues"."id" = {}"#, lit(issue_id())), "");
        assert_eq!(
            m5_without_pk, m4,
            "M5 GET differs from M4 GET only by the pk predicate"
        );
    }

    #[test]
    fn m5_delete_lookup_scope() {
        // FX-CYCMOD-05 M5 delete (`module.py:870-875`).
        let sql = render(&module_issue_delete_lookup(
            &scope(),
            module_id(),
            issue_id(),
        ));
        assert!(sql.contains(r#"FROM "module_issues""#), "{sql}");
        assert!(
            sql.contains(r#""module_issues"."deleted_at" IS NULL"#),
            "{sql}"
        );
        assert!(
            sql.contains(&format!(r#""workspaces"."slug" = '{SLUG}'"#)),
            "{sql}"
        );
        assert!(
            sql.contains(&format!(
                r#""module_issues"."project_id" = {}"#,
                lit(project_id())
            )),
            "{sql}"
        );
        assert!(
            sql.contains(&format!(
                r#""module_issues"."module_id" = {}"#,
                lit(module_id())
            )),
            "{sql}"
        );
        assert!(
            sql.contains(&format!(
                r#""module_issues"."issue_id" = {}"#,
                lit(issue_id())
            )),
            "{sql}"
        );
        assert!(!sql.contains("LIMIT"), "{sql}");
    }

    #[test]
    fn order_defaults_match_django() {
        assert_eq!(OrderBy::default_module(), OrderBy::new("created_at", true));
        assert_eq!(OrderBy::default_issue(), OrderBy::new("created_at", false));
    }

    #[test]
    fn no_sql_text_in_builders() {
        // Every statement binds its values: rendering with the Postgres
        // builder yields parameters, never inlined ids or slugs.
        let (sql, values) = module_issue_list_get(&OrderBy::default_issue(), &scope(), module_id())
            .build(PostgresQueryBuilder);
        assert!(!sql.contains(SLUG), "{sql}");
        assert!(!sql.contains(&module_id().to_string()), "{sql}");
        // triage (subquery + outer), ISSUE_ATTACHMENT, module id, project
        // id, slug.
        assert_eq!(values.0.len(), 6, "{sql}");
    }

    // -----------------------------------------------------------------------
    // Live reads against the scratch database (temp tables, one transaction)
    // -----------------------------------------------------------------------

    /// `DATABASE_URL` pointing at a migrated database, else skip.
    async fn pool() -> Option<sqlx::PgPool> {
        let Ok(url) = std::env::var("DATABASE_URL") else {
            eprintln!("skipping live-db test: DATABASE_URL is not set");
            return None;
        };
        let pool = sqlx::PgPool::connect(&url).await.expect("connect");
        let migrated: Option<String> =
            sqlx::query_scalar("SELECT to_regclass('public.module_issues')::text")
                .fetch_one(&pool)
                .await
                .expect("probe");
        if migrated.is_none() {
            eprintln!("skipping live-db test: the database is not migrated");
            return None;
        }
        Some(pool)
    }

    async fn exec(tx: &mut sqlx::Transaction<'_, sqlx::Postgres>, sql: &str) {
        sqlx::query(sql)
            .execute(&mut **tx)
            .await
            .unwrap_or_else(|error| panic!("{sql}: {error}"));
    }

    fn get<T: sea_orm::TryGetable>(row: &sea_orm::QueryResult, column: &str) -> T {
        row.try_get("", column)
            .unwrap_or_else(|error| panic!("{column}: {error}"))
    }

    /// Every builder runs and counts over a seeded tenant: a live module
    /// with a completed, a started, a draft and an archived bridged
    /// issue, plus an archived module; the grouped counts follow the
    /// ported 0/1 distinct-group shape (drafts and archived issues never
    /// count, triage counts).
    #[tokio::test]
    async fn live_module_reads() {
        use sea_orm::{ConnectionTrait, DbBackend};
        let Some(pool) = pool().await else {
            return;
        };
        let mut tx = pool.begin().await.expect("begin");
        for table in [
            "workspaces",
            "projects",
            "project_members",
            "users",
            "modules",
            "module_issues",
            "issues",
            "states",
            "estimates",
            "estimate_points",
            "issue_assignees",
            "issue_labels",
            "labels",
            "issue_links",
            "file_assets",
        ] {
            exec(
                &mut tx,
                &format!("CREATE TEMP TABLE {table} AS SELECT * FROM public.{table} WITH NO DATA"),
            )
            .await;
        }
        // `GROUP BY "modules"."id"` selects the other module columns
        // through the primary key's functional dependence, which the
        // copy must carry too.
        exec(&mut tx, "ALTER TABLE modules ADD PRIMARY KEY (id)").await;
        let ws = Uuid::new_v4();
        let project = Uuid::new_v4();
        let other_project = Uuid::new_v4();
        let actor = Uuid::new_v4();
        let outsider = Uuid::new_v4();
        let estimate = Uuid::new_v4();
        let point = Uuid::new_v4();
        let label = Uuid::new_v4();
        let (done, started, backlog) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        let (live_module, archived_module) = (Uuid::new_v4(), Uuid::new_v4());
        let (i_done, i_started, i_draft, i_archived, i_old) = (
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
        );
        let issue = |id: Uuid,
                     state: Uuid,
                     draft: bool,
                     archived: bool,
                     completed: bool,
                     with_point: bool,
                     age: i32| {
            format!(
                "INSERT INTO issues (id, name, workspace_id, project_id, state_id, sort_order, priority, complexity_score, sequence_id, \
                 created_at, updated_at, is_draft, archived_at, completed_at, estimate_point_id, description_html, description_json, git_work_branch, workpad) \
                 VALUES ('{id}', 'n', '{ws}', '{project}', '{state}', 1, 'none', 0, 1, now() - interval '{age} min', now(), {draft}, {}, {}, {}, '', '{{}}', '', '')",
                if archived { "current_date" } else { "NULL" },
                if completed { "now()" } else { "NULL" },
                if with_point { format!("'{point_id}'", point_id = point) } else { "NULL".to_owned() },
            )
        };
        let bridge = |module: Uuid, issue: Uuid, age: i32| {
            format!(
                "INSERT INTO module_issues (id, created_at, updated_at, project_id, workspace_id, module_id, issue_id) \
                 VALUES ('{}', now() - interval '{age} min', now(), '{project}', '{ws}', '{module}', '{issue}')",
                Uuid::new_v4()
            )
        };
        let seeds = [
            format!("INSERT INTO workspaces (id, slug, name, created_at, updated_at, owner_id, timezone, background_color) VALUES ('{ws}', 'acme', 'Acme', now(), now(), '{actor}', 'UTC', '')"),
            format!("INSERT INTO projects (id, workspace_id, name, identifier, created_at, updated_at, estimate_id, cycle_view, timezone) VALUES ('{project}', '{ws}', 'P', 'P', now(), now(), '{estimate}', true, 'UTC'), ('{other_project}', '{ws}', 'Q', 'Q', now(), now(), NULL, true, 'UTC')"),
            format!("INSERT INTO project_members (id, workspace_id, project_id, member_id, role, is_active, created_at, updated_at) VALUES ('{}', '{ws}', '{project}', '{actor}', 20, true, now(), now()), ('{}', '{ws}', '{project}', '{outsider}', 15, false, now(), now())", Uuid::new_v4(), Uuid::new_v4()),
            format!("INSERT INTO users (id, display_name, avatar, email, first_name, last_name, created_at, updated_at, username, password, date_joined, user_timezone) VALUES ('{actor}', 'Ada', 'http://a/x.png', 'ada@example.invalid', 'Ada', 'L', now(), now(), 'ada', '', now(), 'UTC'), ('{outsider}', 'Bob', '', NULL, 'Bob', 'B', now(), now(), 'bob', '', now(), 'UTC')"),
            format!("INSERT INTO estimates (id, project_id, workspace_id, name, type, created_at, updated_at) VALUES ('{estimate}', '{project}', '{ws}', 'E', 'points', now(), now())"),
            format!("INSERT INTO estimate_points (id, estimate_id, project_id, workspace_id, key, value, description, created_at, updated_at) VALUES ('{point}', '{estimate}', '{project}', '{ws}', 3, '3', '', now(), now())"),
            format!("INSERT INTO states (id, name, \"group\", created_at, updated_at, description, color, slug, project_id, workspace_id, sequence, \"default\", is_triage) VALUES \
                     ('{done}', 'Done', 'completed', now(), now(), '', '', 'done', '{project}', '{ws}', 1, false, false), \
                     ('{started}', 'Doing', 'started', now(), now(), '', '', 'doing', '{project}', '{ws}', 2, false, false), \
                     ('{backlog}', 'Backlog', 'backlog', now(), now(), '', '', 'backlog', '{project}', '{ws}', 3, true, false)"),
            format!("INSERT INTO labels (id, name, color, workspace_id, project_id, created_at, updated_at, description, sort_order) VALUES ('{label}', 'bug', '#f00', '{ws}', '{project}', now(), now(), '', 1)"),
            format!("INSERT INTO modules (id, created_at, updated_at, project_id, workspace_id, name, description, view_props, sort_order, logo_props, status, start_date, target_date) VALUES \
                     ('{live_module}', now() - interval '2 min', now(), '{project}', '{ws}', 'Epic 1', '', '{{}}', 65535, '{{}}', 'planned', '2026-03-01', '2026-03-31'), \
                     ('{archived_module}', now() - interval '1 min', now(), '{project}', '{ws}', 'Epic 0', '', '{{}}', 55535, '{{}}', 'completed', '2026-02-01', '2026-02-28')"),
            issue(i_done, done, false, false, true, true, 5),
            issue(i_started, started, false, false, false, false, 4),
            issue(i_draft, backlog, true, false, false, false, 3),
            issue(i_archived, backlog, false, true, false, false, 2),
            issue(i_old, done, false, false, true, true, 1),
            bridge(live_module, i_done, 4),
            bridge(live_module, i_started, 3),
            bridge(live_module, i_draft, 2),
            bridge(live_module, i_archived, 1),
            bridge(archived_module, i_old, 1),
            format!("INSERT INTO issue_assignees (id, issue_id, assignee_id, project_id, workspace_id, created_at, updated_at) VALUES ('{}', '{i_done}', '{actor}', '{project}', '{ws}', now(), now())", Uuid::new_v4()),
            format!("INSERT INTO issue_labels (id, issue_id, label_id, project_id, workspace_id, created_at, updated_at) VALUES ('{}', '{i_done}', '{label}', '{project}', '{ws}', now(), now())", Uuid::new_v4()),
            format!("INSERT INTO issue_links (id, issue_id, url, project_id, workspace_id, created_at, updated_at, metadata) VALUES ('{}', '{i_started}', 'http://x', '{project}', '{ws}', now(), now(), '{{}}')", Uuid::new_v4()),
        ];
        // Archive one module (M3) — the live/archived split is by
        // `archived_at`; the seeds leave both live, so stamp the old one.
        let mut with_archive = seeds.to_vec();
        with_archive.push(format!(
            "UPDATE modules SET archived_at = now() WHERE id = '{archived_module}'"
        ));
        for sql in &with_archive {
            exec(&mut tx, sql).await;
        }
        // The sub_issues_count shape: a child counts, a triage child does
        // not (the full IssueManager guards inside the subquery). Seeded
        // here, before the `TxConn` borrow, alongside the main seeds.
        let child = Uuid::new_v4();
        let triage_child = Uuid::new_v4();
        let triage = Uuid::new_v4();
        exec(
            &mut tx,
            &format!("INSERT INTO states (id, name, \"group\", created_at, updated_at, description, color, slug, project_id, workspace_id, sequence, \"default\", is_triage) VALUES ('{triage}', 'Triage', 'triage', now(), now(), '', '', 'triage', '{project}', '{ws}', 4, false, true)"),
        )
        .await;
        exec(
            &mut tx,
            &format!("INSERT INTO issues (id, name, parent_id, workspace_id, project_id, state_id, sort_order, priority, complexity_score, sequence_id, created_at, updated_at, is_draft, description_html, description_json, git_work_branch, workpad) VALUES ('{child}', 'c', '{i_done}', '{ws}', '{project}', '{started}', 1, 'none', 0, 1, now(), now(), false, '', '{{}}', '', ''), ('{triage_child}', 't', '{i_done}', '{ws}', '{project}', '{triage}', 1, 'none', 0, 1, now(), now(), false, '', '{{}}', '', '')"),
        )
        .await;
        let conn = crate::orm_tx::TxConn::new(&mut tx);
        let run = |sel: SelectStatement| {
            let conn = &conn;
            async move {
                conn.query_all(DbBackend::Postgres.build(&sel))
                    .await
                    .unwrap_or_else(|error| panic!("{error}: {}", render(&sel)))
            }
        };
        let scope = TenantScope {
            slug: "acme",
            project_id: project,
        };

        // M1: the live module only, two counted issues (draft + archived
        // skip); the grouped counts are 0/1.
        let rows = run(module_list(
            &OrderBy::default_module(),
            ArchivedFilter::Live,
            &scope,
        ))
        .await;
        assert_eq!(rows.len(), 1);
        assert_eq!(get::<Uuid>(&rows[0], "id"), live_module);
        assert_eq!(get::<i64>(&rows[0], "total_issues"), 2);
        assert_eq!(get::<i64>(&rows[0], "completed_issues"), 1);
        assert_eq!(get::<i64>(&rows[0], "started_issues"), 1);
        assert_eq!(get::<i64>(&rows[0], "backlog_issues"), 0);
        assert_eq!(get::<String>(&rows[0], "name"), "Epic 1");
        // M1 has no member scope: an inactive member still lists.
        let rows = run(module_list(
            &OrderBy::default_module(),
            ArchivedFilter::Live,
            &scope,
        ))
        .await;
        assert_eq!(rows.len(), 1);
        // Both modules without the archived predicate, `-created_at` order.
        let rows = run(module_list(
            &OrderBy::default_module(),
            ArchivedFilter::Any,
            &scope,
        ))
        .await;
        assert_eq!(
            rows.iter()
                .map(|row| get::<Uuid>(row, "id"))
                .collect::<Vec<_>>(),
            vec![archived_module, live_module]
        );
        // M2: live pk hits, archived pk misses (the live predicate).
        let rows = run(module_detail(
            &OrderBy::default_module(),
            &scope,
            live_module,
        ))
        .await;
        assert_eq!(rows.len(), 1);
        assert_eq!(get::<i64>(&rows[0], "total_issues"), 2);
        let rows = run(module_detail(
            &OrderBy::default_module(),
            &scope,
            archived_module,
        ))
        .await;
        assert!(rows.is_empty());
        // M3: the archived module only.
        let rows = run(archived_module_list(&OrderBy::default_module(), &scope)).await;
        assert_eq!(rows.len(), 1);
        assert_eq!(get::<Uuid>(&rows[0], "id"), archived_module);
        assert_eq!(get::<i64>(&rows[0], "total_issues"), 1);
        assert_eq!(get::<i64>(&rows[0], "completed_issues"), 1);
        // M4 queryset: every live bridge (ported bug 5), newest first;
        // member-scoped (the outsider sees nothing).
        let rows = run(module_issue_queryset(
            &OrderBy::default_module(),
            &scope,
            live_module,
            actor,
        ))
        .await;
        assert_eq!(
            rows.iter()
                .map(|row| get::<Uuid>(row, "issue_id"))
                .collect::<Vec<_>>(),
            vec![i_archived, i_draft, i_started, i_done]
        );
        // Only i_done has a (non-triage) child seeded above.
        let sub_by_issue: std::collections::HashMap<Uuid, i64> = rows
            .iter()
            .map(|row| {
                (
                    get::<Uuid>(row, "issue_id"),
                    get::<i64>(row, "sub_issues_count"),
                )
            })
            .collect();
        assert_eq!(sub_by_issue[&i_done], 1);
        assert_eq!(sub_by_issue[&i_started], 0);
        assert_eq!(sub_by_issue[&i_draft], 0);
        assert_eq!(sub_by_issue[&i_archived], 0);
        let rows = run(module_issue_queryset(
            &OrderBy::default_module(),
            &scope,
            live_module,
            outsider,
        ))
        .await;
        assert!(rows.is_empty());
        // M4 GET: the manager guards drop the draft and archived issues;
        // ascending created_at; the annotations decode by name.
        let rows = run(module_issue_list_get(
            &OrderBy::default_issue(),
            &scope,
            live_module,
        ))
        .await;
        assert_eq!(
            rows.iter()
                .map(|row| get::<Uuid>(row, "id"))
                .collect::<Vec<_>>(),
            vec![i_done, i_started]
        );
        assert_eq!(get::<i64>(&rows[1], "link_count"), 1);
        assert_eq!(get::<i64>(&rows[0], "link_count"), 0);
        assert_eq!(get::<i64>(&rows[0], "attachment_count"), 0);
        // M5 GET: the same row by pk; a wrong pk misses.
        let rows = run(module_issue_detail_get(
            &OrderBy::default_issue(),
            &scope,
            live_module,
            i_done,
        ))
        .await;
        assert_eq!(rows.len(), 1);
        assert_eq!(get::<Uuid>(&rows[0], "id"), i_done);
        let rows = run(module_issue_detail_get(
            &OrderBy::default_issue(),
            &scope,
            live_module,
            i_archived,
        ))
        .await;
        assert!(rows.is_empty());
        // M5 delete lookup: the live bridge by (module, issue).
        let rows = run(module_issue_delete_lookup(&scope, live_module, i_done)).await;
        assert_eq!(rows.len(), 1);
        let rows = run(module_issue_delete_lookup(&scope, live_module, i_old)).await;
        assert!(rows.is_empty());

        // The sub_issues_count shape (seeded above): a child counts, a
        // triage child does not.
        let rows = run(module_issue_list_get(
            &OrderBy::default_issue(),
            &scope,
            live_module,
        ))
        .await;
        let done_row = rows
            .iter()
            .find(|row| get::<Uuid>(row, "id") == i_done)
            .expect("done row");
        assert_eq!(get::<i64>(done_row, "sub_issues_count"), 1);
        let started_row = rows
            .iter()
            .find(|row| get::<Uuid>(row, "id") == i_started)
            .expect("started row");
        assert_eq!(get::<i64>(started_row, "sub_issues_count"), 0);
    }
}
