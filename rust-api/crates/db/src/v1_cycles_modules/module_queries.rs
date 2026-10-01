//! Module read querysets (PIDASHCONV-308, D-20 stage 5).
//!
//! Ports the five `get_queryset` chains in
//! `apps/api/pi_dash/api/views/module.py` — plus the two `GET` inline
//! `Issue` querysets that shadow the issue querysets on the wire — to
//! executable SQL text (drift baseline `01a93e17`):
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
//! Each `*_sql` function returns a complete `SELECT` statement with
//! symbolic bind parameters, following the merged `v1_projects`
//! precedent (`queries_stateest.rs`): `$1` is the workspace slug
//! (`workspaces.slug`, text), `$2` the project id (uuid), `$3` the module
//! id (uuid, M2 detail / M4 / M5), `$4` the acting-user id (uuid, M4/M5
//! querysets only) or the issue id (uuid, M5 GET / M5 delete). Handlers
//! bind them in that order; group literals (`'completed'`, `'triage'`,
//! `'ISSUE_ATTACHMENT'`) are embedded because they are fixed enums, the
//! same values Django sends as parameters at execution.
//!
//! Dynamic statements are sea-query builders; the fixed aggregate
//! fragments are string constants spliced in with `Expr::cust` (there is
//! no build-time database, same as the merged `license/queries` and
//! `app_integrations/queries_git` precedents). Spelling note: sea-query
//! renders `LEFT JOIN` where Django renders `LEFT OUTER JOIN` —
//! identical semantics, pinned as-rendered by the tests. `ORDER BY` is dynamic:
//! `.order_by(self.kwargs.get("order_by", ...))` passes the kwarg through
//! untouched, so [`OrderBy`] carries the raw column plus direction and
//! the builder quotes it onto the base table — an unknown column fails at
//! the database exactly like Django's `FieldError`-at-evaluation.
//! (PIDASHCONV-510: the M4/M5 GET `?order_by=` additionally resolves
//! Django-side to `?` → `RANDOM()`, single-level FK traversals onto the
//! already-joined tables, bare output aliases for the two annotations
//! that exist at `.order_by()` time, and bare-M2M default orderings with
//! extra joins — [`OrderTarget`] carries those shapes and [`apply_order`]
//! renders them identically in every builder.)
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
//!    observed via [`count_issues_sql`]; the 0/1 shape is pinned by
//!    `grouped_counts_are_zero_or_one_distinct_group`.
//! 2. M1/M2/M3 carry **no member-visibility join** (unlike the cycle
//!    querysets, `cycle.py:93-96`): any caller in the project lists every
//!    module; membership is enforced by `ProjectEntityPermission` only.
//!    Ported as observed — [`module_list_sql`] joins no
//!    `project_members`.
//! 3. M1/M2 omit the trailing `.distinct()` the cycle querysets keep;
//!    only the `DISTINCT` *inside* each `Count` remains. M4/M5 querysets
//!    keep `SELECT DISTINCT`. Ported as observed.
//! 4. M3 has **no estimate annotations**, unlike archived cycles Q3
//!    (`cycle.py:699-721`). Ported as observed: [`archived_module_list_sql`]
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
//!    [`module_issue_detail_get_sql`] ports the shape anyway (the fixture
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

use sea_query::{Alias, Condition, Expr, IntoIden, JoinType, Order, Query, TableRef};

use super::module::{self, module_issue};
use crate::v1_projects::models::{project, project_member, state};

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
/// Alias of the parent self-join (`select_related("parent")`, M4/M5 GET);
/// `order_by=parent__<col>` renders onto it (PIDASHCONV-510).
pub const PARENT_ALIAS: &str = "T7";

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
fn apply_order(sel: &mut sea_query::SelectStatement, base: &str, order: &OrderBy) {
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
            sel.order_by_expr(Expr::cust("RANDOM()"), Order::Asc);
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
    }
}

/// Project one table's `columns` as `"table"."col"` selects, in order.
fn select_table_columns(sel: &mut sea_query::SelectStatement, table: &str, columns: &[&str]) {
    for col in columns {
        sel.column((Alias::new(table.to_owned()), Alias::new((*col).to_owned())));
    }
}

/// `INNER JOIN "projects" ON ("<table>"."project_id" = "projects"."id")`
/// (every `select_related("project")`).
fn join_project(sel: &mut sea_query::SelectStatement, table: &str) {
    sel.join(
        JoinType::InnerJoin,
        Alias::new(project::TABLE.to_owned()),
        Condition::all().add(
            Expr::col((Alias::new(table.to_owned()), Alias::new("project_id")))
                .equals((Alias::new(project::TABLE), Alias::new("id"))),
        ),
    );
}

/// `INNER JOIN "workspaces" ON ("<table>"."workspace_id" = "workspaces"."id")`
/// (every `select_related("workspace")`).
fn join_workspace(sel: &mut sea_query::SelectStatement, table: &str) {
    sel.join(
        JoinType::InnerJoin,
        Alias::new(WORKSPACE_TABLE.to_owned()),
        Condition::all().add(
            Expr::col((Alias::new(table.to_owned()), Alias::new("workspace_id")))
                .equals((Alias::new(WORKSPACE_TABLE), Alias::new("id"))),
        ),
    );
}

/// Tenant scope shared by the module reads: this project, this workspace
/// slug (`module.py:87-88,290-291,553-554,760-761,897-898`). Soft-delete
/// scoping (`deleted_at IS NULL`) comes from each model's manager and is
/// applied per table at the call site — `projects` / `workspaces` carry
/// NO deleted predicate (verified against live Django SQL: only the base
/// table's manager scope renders).
fn tenant_condition(table: &str) -> Condition {
    Condition::all()
        .add(
            Expr::col((Alias::new(table.to_owned()), Alias::new("project_id")))
                .eq(Expr::cust("$2")),
        )
        .add(
            Expr::col((Alias::new(WORKSPACE_TABLE.to_owned()), Alias::new("slug")))
                .eq(Expr::cust("$1")),
        )
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

/// Explicit issue-liveness filter shared by all six counts
/// (`module.py:102-106` and each group `Q`): the bridge is live, the
/// issue is not archived and not a draft. There is deliberately NO
/// triage / project-archived guard — relation traversals use the base
/// manager (ported bug 5).
const LIVE_ISSUE_FILTER: &str = r#""module_issues"."deleted_at" IS NULL AND "issues"."archived_at" IS NULL AND NOT "issues"."is_draft""#;

/// One `Count(...)` annotation as Django renders it
/// (`module.py:99-169`, verified against live `str(query)` output):
///
/// * total (`GroupFilter::All`): `COUNT(DISTINCT "module_issues"."id")`
///   — a real row count.
/// * grouped: `COUNT(DISTINCT "states"."group")` — the count of distinct
///   group values under a single-group `FILTER`, i.e. **0 or 1**
///   (ported bug 1). The `FILTER` carries the same liveness guard plus
///   `"states"."group" = '<group>'`.
pub fn count_issues_sql(filter: GroupFilter, alias: &str) -> String {
    match filter {
        GroupFilter::All => format!(
            r#"COUNT(DISTINCT "module_issues"."id") FILTER (WHERE ({LIVE_ISSUE_FILTER})) AS "{alias}""#
        ),
        GroupFilter::Eq(group) => format!(
            r#"COUNT(DISTINCT "states"."group") FILTER (WHERE ({LIVE_ISSUE_FILTER} AND "states"."group" = '{group}')) AS "{alias}""#
        ),
    }
}

/// All six count annotations in source order, for the M1/M2/M3 select
/// list (`total_issues` first, then the five groups).
pub fn all_count_annotations_sql() -> Vec<String> {
    let mut out = vec![count_issues_sql(GroupFilter::All, "total_issues")];
    for (group, alias) in [
        ("completed", "completed_issues"),
        ("cancelled", "cancelled_issues"),
        ("started", "started_issues"),
        ("unstarted", "unstarted_issues"),
        ("backlog", "backlog_issues"),
    ] {
        out.push(count_issues_sql(GroupFilter::Eq(group), alias));
    }
    out
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
fn join_module_reads(sel: &mut sea_query::SelectStatement) {
    let modules = Alias::new(module::TABLE.to_owned());
    join_project(sel, module::TABLE);
    join_workspace(sel, module::TABLE);
    // LEFT OUTER JOIN "module_issues" ON ("modules"."id" = "module_issues"."module_id")
    sel.join(
        JoinType::LeftJoin,
        Alias::new(module_issue::TABLE.to_owned()),
        Condition::all().add(
            Expr::col((modules.clone(), Alias::new("id")))
                .equals((Alias::new(module_issue::TABLE), Alias::new("module_id"))),
        ),
    );
    // LEFT OUTER JOIN "issues" ON ("module_issues"."issue_id" = "issues"."id")
    sel.join(
        JoinType::LeftJoin,
        Alias::new(ISSUE_TABLE.to_owned()),
        Condition::all().add(
            Expr::col((Alias::new(module_issue::TABLE), Alias::new("issue_id")))
                .equals((Alias::new(ISSUE_TABLE), Alias::new("id"))),
        ),
    );
    // LEFT OUTER JOIN "states" ON ("issues"."state_id" = "states"."id")
    sel.join(
        JoinType::LeftJoin,
        Alias::new(state::TABLE.to_owned()),
        Condition::all().add(
            Expr::col((Alias::new(ISSUE_TABLE), Alias::new("state_id")))
                .equals((Alias::new(state::TABLE), Alias::new("id"))),
        ),
    );
    // LEFT OUTER JOIN "users" ON ("modules"."lead_id" = "users"."id")
    sel.join(
        JoinType::LeftJoin,
        Alias::new(USER_TABLE.to_owned()),
        Condition::all().add(
            Expr::col((modules, Alias::new("lead_id")))
                .equals((Alias::new(USER_TABLE), Alias::new("id"))),
        ),
    );
}

/// Shared M1/M2/M3 `WHERE` head: the base-table manager scope
/// (`modules.deleted_at IS NULL`) plus the tenant scope. The
/// `projects` / `workspaces` joins carry no deleted predicate
/// (verified against live Django SQL).
fn module_list_where() -> Condition {
    Condition::all()
        .add(Expr::col((Alias::new(module::TABLE), Alias::new("deleted_at"))).is_null())
        .add(tenant_condition(module::TABLE))
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
///
/// Binds: `$1` slug, `$2` project id.
pub fn module_list_sql(order: &OrderBy, archived: ArchivedFilter) -> String {
    use sea_query::PostgresQueryBuilder;
    let mut sel = Query::select();
    select_table_columns(&mut sel, module::TABLE, module::COLUMNS);
    for annotation in all_count_annotations_sql() {
        sel.expr(Expr::cust(annotation));
    }
    sel.from(Alias::new(module::TABLE.to_owned()));
    join_module_reads(&mut sel);
    let mut scope = module_list_where();
    match archived {
        ArchivedFilter::Live => {
            scope = scope
                .add(Expr::col((Alias::new(module::TABLE), Alias::new("archived_at"))).is_null());
        }
        ArchivedFilter::Archived => {
            scope = scope.add(
                Expr::col((Alias::new(module::TABLE), Alias::new("archived_at"))).is_not_null(),
            );
        }
        ArchivedFilter::Any => {}
    }
    sel.cond_where(scope);
    sel.group_by_col((Alias::new(module::TABLE), Alias::new("id")));
    apply_order(&mut sel, module::TABLE, order);
    sel.to_string(PostgresQueryBuilder)
}

/// M2 module detail (`ModuleDetailAPIEndpoint.get_queryset`,
/// `module.py:288-374`, + `.filter(archived_at__isnull=True).get(pk=pk)`
/// from the detail `GET` at `:473`): identical to M1 live plus the pk
/// predicate. The chain text is byte-identical to M1's; only the GET
/// wrapper differs.
///
/// Binds: `$1` slug, `$2` project id, `$3` module id.
pub fn module_detail_sql(order: &OrderBy) -> String {
    use sea_query::PostgresQueryBuilder;
    let mut sel = Query::select();
    select_table_columns(&mut sel, module::TABLE, module::COLUMNS);
    for annotation in all_count_annotations_sql() {
        sel.expr(Expr::cust(annotation));
    }
    sel.from(Alias::new(module::TABLE.to_owned()));
    join_module_reads(&mut sel);
    sel.cond_where(
        module_list_where()
            .add(Expr::col((Alias::new(module::TABLE), Alias::new("archived_at"))).is_null())
            .add(Expr::col((Alias::new(module::TABLE), Alias::new("id"))).eq(Expr::cust("$3"))),
    );
    sel.group_by_col((Alias::new(module::TABLE), Alias::new("id")));
    apply_order(&mut sel, module::TABLE, order);
    sel.to_string(PostgresQueryBuilder)
}

/// M3 archived-module list (`ModuleArchiveUnarchiveAPIEndpoint.get_queryset`,
/// `module.py:895-982`): same chain as M1 with
/// `archived_at__isnull=False` (`:899`) and NO estimate annotations
/// (ported bug 4 — the asymmetry vs archived cycles is intentional).
///
/// Binds: `$1` slug, `$2` project id.
pub fn archived_module_list_sql(order: &OrderBy) -> String {
    module_list_sql(order, ArchivedFilter::Archived)
}

// ---------------------------------------------------------------------------
// M4/M5 issue querysets (ModuleIssue-based)
// ---------------------------------------------------------------------------

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
pub fn sub_issues_count_sql(outer: &str) -> String {
    format!(
        r#"(SELECT COUNT(U0."id") AS "count" FROM "issues" U0 LEFT OUTER JOIN "states" U1 ON (U0."state_id" = U1."id") INNER JOIN "projects" U2 ON (U0."project_id" = U2."id") WHERE (U0."deleted_at" IS NULL AND NOT (U1."group" = 'triage' AND U1."group" IS NOT NULL) AND NOT (U0."archived_at" IS NOT NULL) AND NOT (U2."archived_at" IS NOT NULL) AND NOT (U0."is_draft") AND U0."parent_id" = ({outer}))) AS "sub_issues_count""#
    )
}

/// M4/M5 queryset (`ModuleIssueListCreateAPIEndpoint.get_queryset`,
/// `module.py:545-569`; M5's `module.py:751-775` is chain-identical):
/// `SELECT DISTINCT` bridge columns in [`module_issue::COLUMNS`] order +
/// [`sub_issues_count_sql`] correlated to the bridge's issue.
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
///
/// Binds: `$1` slug, `$2` project id, `$3` module id, `$4` acting-user id.
pub fn module_issue_queryset_sql(order: &OrderBy) -> String {
    use sea_query::PostgresQueryBuilder;
    let bridges = Alias::new(module_issue::TABLE.to_owned());
    let mut sel = Query::select();
    sel.distinct();
    select_table_columns(&mut sel, module_issue::TABLE, module_issue::COLUMNS);
    sel.expr(Expr::cust(sub_issues_count_sql(
        r#""module_issues"."issue_id""#,
    )));
    sel.from(Alias::new(module_issue::TABLE.to_owned()));
    // INNER JOIN "issues" ON ("module_issues"."issue_id" = "issues"."id")
    sel.join(
        JoinType::InnerJoin,
        Alias::new(ISSUE_TABLE.to_owned()),
        Condition::all().add(
            Expr::col((bridges.clone(), Alias::new("issue_id")))
                .equals((Alias::new(ISSUE_TABLE), Alias::new("id"))),
        ),
    );
    join_workspace(&mut sel, module_issue::TABLE);
    join_project(&mut sel, module_issue::TABLE);
    // INNER JOIN "modules" ON ("module_issues"."module_id" = "modules"."id")
    sel.join(
        JoinType::InnerJoin,
        Alias::new(module::TABLE.to_owned()),
        Condition::all().add(
            Expr::col((bridges.clone(), Alias::new("module_id")))
                .equals((Alias::new(module::TABLE), Alias::new("id"))),
        ),
    );
    // INNER JOIN "project_members" ON ("projects"."id" = "project_members"."project_id")
    sel.join(
        JoinType::InnerJoin,
        Alias::new(project_member::TABLE.to_owned()),
        Condition::all().add(
            Expr::col((Alias::new(project::TABLE), Alias::new("id")))
                .equals((Alias::new(project_member::TABLE), Alias::new("project_id"))),
        ),
    );
    // INNER JOIN "projects" AS "T8" ON ("issues"."project_id" = T8."id")
    // (select_related("issue__project")).
    sel.join(
        JoinType::InnerJoin,
        TableRef::Table(Alias::new(project::TABLE.to_owned()).into_iden()).alias(Alias::new("T8")),
        Condition::all().add(
            Expr::col((Alias::new(ISSUE_TABLE), Alias::new("project_id")))
                .equals((Alias::new("T8"), Alias::new("id"))),
        ),
    );
    // LEFT OUTER JOIN "states" ON ("issues"."state_id" = "states"."id")
    sel.join(
        JoinType::LeftJoin,
        Alias::new(state::TABLE.to_owned()),
        Condition::all().add(
            Expr::col((Alias::new(ISSUE_TABLE), Alias::new("state_id")))
                .equals((Alias::new(state::TABLE), Alias::new("id"))),
        ),
    );
    sel.cond_where(
        Condition::all()
            .add(Expr::col((bridges.clone(), Alias::new("deleted_at"))).is_null())
            .add(
                Expr::col((Alias::new(WORKSPACE_TABLE.to_owned()), Alias::new("slug")))
                    .eq(Expr::cust("$1")),
            )
            .add(Expr::col((bridges.clone(), Alias::new("project_id"))).eq(Expr::cust("$2")))
            .add(Expr::col((bridges.clone(), Alias::new("module_id"))).eq(Expr::cust("$3")))
            .add(Expr::col((Alias::new(project_member::TABLE), Alias::new("is_active"))).eq(true))
            .add(
                Expr::col((Alias::new(project_member::TABLE), Alias::new("member_id")))
                    .eq(Expr::cust("$4")),
            )
            .add(Expr::col((Alias::new(project::TABLE), Alias::new("archived_at"))).is_null()),
    );
    apply_order(&mut sel, module_issue::TABLE, order);
    sel.to_string(PostgresQueryBuilder)
}

/// `link_count` scalar annotation (M4/M5 GET, `module.py:619-624,829-834`,
/// verified against live `str(query)` output): `IssueLink.objects` uses
/// the soft-delete default manager, so `deleted_at IS NULL` renders
/// (unlike the unguarded traversal in `sub_issues_count`'s outer query).
pub fn link_count_sql() -> String {
    format!(
        r#"(SELECT COUNT(U0."id") AS "count" FROM "{ISSUE_LINK_TABLE}" U0 WHERE (U0."deleted_at" IS NULL AND U0."issue_id" = ("issues"."id"))) AS "link_count""#
    )
}

/// `attachment_count` scalar annotation (M4/M5 GET, `module.py:625-633,
/// 835-843`): same soft-delete manager scope plus the
/// `entity_type = 'ISSUE_ATTACHMENT'` literal
/// (`FileAsset.EntityTypeContext.ISSUE_ATTACHMENT`).
pub fn attachment_count_sql() -> String {
    format!(
        r#"(SELECT COUNT(U0."id") AS "count" FROM "{FILE_ASSET_TABLE}" U0 WHERE (U0."deleted_at" IS NULL AND U0."entity_type" = 'ISSUE_ATTACHMENT' AND U0."issue_id" = ("issues"."id"))) AS "attachment_count""#
    )
}

/// `bridge_id` annotation (M4/M5 GET, `module.py:609,819`):
/// `.annotate(bridge_id=F("issue_module__id"))` — the live bridge's pk
/// via the `INNER JOIN "module_issues"`. One row per live bridge: an
/// issue bridged twice (deleted-then-recreated rows excepted by the
/// `deleted_at` guard) fans out into two rows — ported as observed.
pub fn bridge_id_sql() -> String {
    r#""module_issues"."id" AS "bridge_id""#.to_owned()
}

/// Outer `IssueManager` guards for the M4/M5 GET shapes
/// (`db/models/issue.py:95-104`, verified against live `str(query)`
/// output): soft-delete, non-triage states (NULL-safe — the `states`
/// join is a `LEFT OUTER JOIN`), live issues, live projects, non-drafts.
/// These apply ONLY to `Issue.issue_objects` entry points (the GET outer
/// query and the `sub_issues_count` subqueries), never to relation
/// traversals (ported bug 5).
pub const ISSUE_MANAGER_OUTER_GUARDS: &str = r#""issues"."deleted_at" IS NULL AND NOT ("states"."group" = 'triage' AND "states"."group" IS NOT NULL) AND NOT ("issues"."archived_at" IS NOT NULL) AND NOT ("projects"."archived_at" IS NOT NULL) AND NOT ("issues"."is_draft")"#;

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
///
/// Binds: `$1` slug, `$2` project id, `$3` module id.
pub fn module_issue_list_get_sql(order: &OrderBy) -> String {
    module_issue_get_sql_inner(order, None)
}

/// M5 issue detail `GET` inline queryset (`module.py:807-843`): M4 GET
/// plus `pk=issue_id`. Unreachable on the wire (ported bug 6 — the
/// `module-issues-detail` route allows `delete` only); ported because
/// fixture M5 covers it and the domain gate may exercise it.
///
/// Binds: `$1` slug, `$2` project id, `$3` module id, `$4` issue id.
pub fn module_issue_detail_get_sql(order: &OrderBy) -> String {
    module_issue_get_sql_inner(order, Some("$4"))
}

fn module_issue_get_sql_inner(order: &OrderBy, issue_bind: Option<&str>) -> String {
    use sea_query::PostgresQueryBuilder;
    let mut sel = Query::select();
    sel.expr(Expr::cust(r#""issues".*"#));
    sel.expr(Expr::cust(sub_issues_count_sql(r#""issues"."id""#)));
    sel.expr(Expr::cust(bridge_id_sql()));
    sel.expr(Expr::cust(link_count_sql()));
    sel.expr(Expr::cust(attachment_count_sql()));
    sel.from(Alias::new(ISSUE_TABLE.to_owned()));
    // LEFT OUTER JOIN "states" ON ("issues"."state_id" = "states"."id")
    sel.join(
        JoinType::LeftJoin,
        Alias::new(state::TABLE.to_owned()),
        Condition::all().add(
            Expr::col((Alias::new(ISSUE_TABLE), Alias::new("state_id")))
                .equals((Alias::new(state::TABLE), Alias::new("id"))),
        ),
    );
    join_project(&mut sel, ISSUE_TABLE);
    // INNER JOIN "module_issues" ON ("issues"."id" = "module_issues"."issue_id")
    sel.join(
        JoinType::InnerJoin,
        Alias::new(module_issue::TABLE.to_owned()),
        Condition::all().add(
            Expr::col((Alias::new(ISSUE_TABLE), Alias::new("id")))
                .equals((Alias::new(module_issue::TABLE), Alias::new("issue_id"))),
        ),
    );
    join_workspace(&mut sel, ISSUE_TABLE);
    // LEFT OUTER JOIN "issues" AS "T7" ON ("issues"."parent_id" = T7."id")
    // (select_related("parent")).
    sel.join(
        JoinType::LeftJoin,
        TableRef::Table(Alias::new(ISSUE_TABLE.to_owned()).into_iden())
            .alias(Alias::new(PARENT_ALIAS)),
        Condition::all().add(
            Expr::col((Alias::new(ISSUE_TABLE), Alias::new("parent_id")))
                .equals((Alias::new(PARENT_ALIAS), Alias::new("id"))),
        ),
    );
    // Bare-M2M ordering joins (PIDASHCONV-510), after T7 like Django:
    // `LEFT JOIN "<through>"` + `LEFT JOIN "<target>"`, no DISTINCT —
    // rows multiply per through-row exactly like the Django queryset.
    if let OrderTarget::M2M(which) = order.target {
        let (through, issue_fk, target_fk, target) = which.tables();
        sel.join(
            JoinType::LeftJoin,
            Alias::new(through.to_owned()),
            Condition::all().add(
                Expr::col((Alias::new(ISSUE_TABLE), Alias::new("id")))
                    .equals((Alias::new(through), Alias::new(issue_fk))),
            ),
        );
        sel.join(
            JoinType::LeftJoin,
            Alias::new(target.to_owned()),
            Condition::all().add(
                Expr::col((Alias::new(through), Alias::new(target_fk)))
                    .equals((Alias::new(target), Alias::new("id"))),
            ),
        );
    }
    let mut scope = Condition::all()
        .add(Expr::cust(ISSUE_MANAGER_OUTER_GUARDS))
        .add(Expr::col((Alias::new(module_issue::TABLE), Alias::new("deleted_at"))).is_null())
        .add(
            Expr::col((Alias::new(module_issue::TABLE), Alias::new("module_id")))
                .eq(Expr::cust("$3")),
        )
        .add(Expr::col((Alias::new(ISSUE_TABLE), Alias::new("project_id"))).eq(Expr::cust("$2")))
        .add(
            Expr::col((Alias::new(WORKSPACE_TABLE.to_owned()), Alias::new("slug")))
                .eq(Expr::cust("$1")),
        );
    if let Some(bind) = issue_bind {
        scope =
            scope.add(Expr::col((Alias::new(ISSUE_TABLE), Alias::new("id"))).eq(Expr::cust(bind)));
    }
    sel.cond_where(scope);
    apply_order(&mut sel, ISSUE_TABLE, order);
    sel.to_string(PostgresQueryBuilder)
}

/// M5 detail `DELETE` lookup (`ModuleIssueDetailAPIEndpoint.delete`,
/// `module.py:870-875`): `ModuleIssue.objects.get(workspace__slug,
/// project_id, module_id, issue_id)` — default-manager scope
/// (`deleted_at IS NULL`) plus the four equality predicates. Django's
/// `.get()` adds no `LIMIT` (0 rows → `DoesNotExist` → 404, 2+ →
/// `MultipleObjectsReturned` → 500); handlers own that mapping.
///
/// Binds: `$1` slug, `$2` project id, `$3` module id, `$4` issue id.
pub fn module_issue_delete_lookup_sql() -> String {
    use sea_query::PostgresQueryBuilder;
    let bridges = Alias::new(module_issue::TABLE.to_owned());
    let mut sel = Query::select();
    select_table_columns(&mut sel, module_issue::TABLE, module_issue::COLUMNS);
    sel.from(Alias::new(module_issue::TABLE.to_owned()));
    join_workspace(&mut sel, module_issue::TABLE);
    sel.cond_where(
        Condition::all()
            .add(Expr::col((bridges.clone(), Alias::new("deleted_at"))).is_null())
            .add(
                Expr::col((Alias::new(WORKSPACE_TABLE.to_owned()), Alias::new("slug")))
                    .eq(Expr::cust("$1")),
            )
            .add(Expr::col((bridges.clone(), Alias::new("project_id"))).eq(Expr::cust("$2")))
            .add(Expr::col((bridges.clone(), Alias::new("module_id"))).eq(Expr::cust("$3")))
            .add(Expr::col((bridges, Alias::new("issue_id"))).eq(Expr::cust("$4"))),
    );
    sel.to_string(PostgresQueryBuilder)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn m1_list_live_scope_and_joins() {
        // FX-CYCMOD-05 M1 (`module.py:85-171` + `:272`).
        let sql = module_list_sql(&OrderBy::default_module(), ArchivedFilter::Live);
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
        // Tenant scope + live filter + binds.
        assert!(sql.contains(r#""modules"."deleted_at" IS NULL"#), "{sql}");
        assert!(sql.contains(r#""modules"."project_id" = ($2)"#), "{sql}");
        assert!(sql.contains(r#""workspaces"."slug" = ($1)"#), "{sql}");
        assert!(sql.contains(r#""modules"."archived_at" IS NULL"#), "{sql}");
        // Only the base table carries a deleted predicate.
        assert!(!sql.contains(r#""projects"."deleted_at""#), "{sql}");
        assert!(!sql.contains(r#""workspaces"."deleted_at""#), "{sql}");
        // No IssueManager guards ride the annotation joins.
        assert!(!sql.contains("triage"), "{sql}");
        assert!(sql.contains(r#"GROUP BY "modules"."id""#), "{sql}");
        assert!(
            sql.contains(r#"ORDER BY "modules"."created_at" DESC"#),
            "{sql}"
        );
    }

    #[test]
    fn m1_annotations_total_counts_rows_groups_are_zero_or_one() {
        // FX-CYCMOD-05 M1 (`module.py:99-169`): total counts bridge rows,
        // each grouped count tallies DISTINCT group values (0/1).
        let sql = module_list_sql(&OrderBy::default_module(), ArchivedFilter::Live);
        assert!(
            sql.contains(
                r#"COUNT(DISTINCT "module_issues"."id") FILTER (WHERE ("module_issues"."deleted_at" IS NULL AND "issues"."archived_at" IS NULL AND NOT "issues"."is_draft")) AS "total_issues""#
            ),
            "{sql}"
        );
        for (group, alias) in [
            ("completed", "completed_issues"),
            ("cancelled", "cancelled_issues"),
            ("started", "started_issues"),
            ("unstarted", "unstarted_issues"),
            ("backlog", "backlog_issues"),
        ] {
            assert!(
                sql.contains(&format!(
                    r#"COUNT(DISTINCT "states"."group") FILTER (WHERE ("module_issues"."deleted_at" IS NULL AND "issues"."archived_at" IS NULL AND NOT "issues"."is_draft" AND "states"."group" = '{group}')) AS "{alias}""#
                )),
                "{sql}"
            );
        }
        assert_eq!(
            COUNT_ALIASES.len(),
            6,
            "six annotations serve the fixture row keys"
        );
    }

    #[test]
    fn m1_bare_queryset_carries_no_archived_predicate() {
        let sql = module_list_sql(&OrderBy::default_module(), ArchivedFilter::Any);
        // The projected `"modules"."archived_at"` column stays (it is a
        // real column); only the WHERE predicate is absent.
        assert!(!sql.contains(r#""modules"."archived_at" IS"#), "{sql}");
    }

    #[test]
    fn m2_detail_adds_live_plus_pk() {
        // FX-CYCMOD-05 M2 (`module.py:288-374` + `:473`).
        let sql = module_detail_sql(&OrderBy::default_module());
        assert!(sql.contains(r#""modules"."archived_at" IS NULL"#), "{sql}");
        assert!(sql.contains(r#""modules"."id" = ($3)"#), "{sql}");
        assert!(!sql.contains("project_members"), "{sql}");
        assert!(!sql.contains("SELECT DISTINCT"), "{sql}");
    }

    #[test]
    fn m3_archived_flips_predicate_and_adds_no_estimates() {
        // FX-CYCMOD-05 M3 (`module.py:895-982`): same chain, archived
        // predicate, no estimate annotations (ported bug 4).
        let sql = archived_module_list_sql(&OrderBy::default_module());
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
        let sql = module_list_sql(&OrderBy::new("name", false), ArchivedFilter::Live);
        assert!(sql.contains(r#"ORDER BY "modules"."name" ASC"#), "{sql}");
    }

    #[test]
    fn m4_get_order_random_renders_random_asc() {
        // PIDASHCONV-510: `?order_by=?` → Django `ORDER BY RANDOM() ASC`,
        // no extra joins.
        let sql = module_issue_list_get_sql(&OrderBy::random());
        assert!(sql.contains("ORDER BY RANDOM() ASC"), "{sql}");
        assert!(!sql.contains("issue_assignees"), "{sql}");
        assert!(!sql.contains("issue_labels"), "{sql}");
    }

    #[test]
    fn m4_get_order_traversal_renders_qualified() {
        // PIDASHCONV-510: `state__group` reuses the select_related join.
        let sql = module_issue_list_get_sql(&OrderBy::table("states", "group", false));
        assert!(sql.contains(r#"ORDER BY "states"."group" ASC"#), "{sql}");
        let sql = module_issue_list_get_sql(&OrderBy::table("states", "group", true));
        assert!(sql.contains(r#"ORDER BY "states"."group" DESC"#), "{sql}");
        // Parent traversals render onto the T7 self-join alias.
        let sql = module_issue_list_get_sql(&OrderBy::table(PARENT_ALIAS, "created_at", false));
        assert!(sql.contains(r#"ORDER BY "T7"."created_at" ASC"#), "{sql}");
    }

    #[test]
    fn m4_get_order_alias_renders_bare() {
        // PIDASHCONV-510: early annotations order by bare alias (Django
        // renders the positional `ORDER BY 35` — same semantics, without
        // depending on the `issues.*` width).
        let sql = module_issue_list_get_sql(&OrderBy::alias("sub_issues_count", false));
        assert!(sql.contains(r#"ORDER BY "sub_issues_count" ASC"#), "{sql}");
        assert!(!sql.contains(r#""issues"."sub_issues_count""#), "{sql}");
    }

    #[test]
    fn m4_get_order_m2m_joins_and_inverts_direction() {
        // PIDASHCONV-510: `assignees` — through + target LEFT JOINs after
        // T7, related Meta.ordering ('-created_at',) as-is → DESC ...
        let sql = module_issue_list_get_sql(&OrderBy::m2m(M2MOrder::Assignees, false));
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
        let sql = module_issue_list_get_sql(&OrderBy::m2m(M2MOrder::Assignees, true));
        assert!(
            sql.contains(r#"ORDER BY "users"."created_at" ASC"#),
            "{sql}"
        );
        // `labels` mirrors via issue_labels/labels.
        let sql = module_issue_list_get_sql(&OrderBy::m2m(M2MOrder::Labels, false));
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
    fn m5_get_shares_exotic_order_targets() {
        // PIDASHCONV-510: M5 GET shares the inner builder — M2M joins
        // render there too, plus the pk predicate.
        let sql = module_issue_detail_get_sql(&OrderBy::m2m(M2MOrder::Assignees, false));
        assert!(sql.contains(r#"LEFT JOIN "issue_assignees""#), "{sql}");
        assert!(
            sql.contains(r#"ORDER BY "users"."created_at" DESC"#),
            "{sql}"
        );
        assert!(sql.contains(r#""issues"."id" = ($4)"#), "{sql}");
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
        let sql = module_issue_queryset_sql(&OrderBy::default_module());
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
        assert!(sql.contains(r#"INNER JOIN "project_members" ON "projects"."id" = "project_members"."project_id""#), "{sql}");
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
            sql.contains(r#""module_issues"."project_id" = ($2)"#),
            "{sql}"
        );
        assert!(
            sql.contains(r#""module_issues"."module_id" = ($3)"#),
            "{sql}"
        );
        assert!(
            sql.contains(r#""project_members"."is_active" = TRUE"#),
            "{sql}"
        );
        assert!(
            sql.contains(r#""project_members"."member_id" = ($4)"#),
            "{sql}"
        );
        assert!(sql.contains(r#""projects"."archived_at" IS NULL"#), "{sql}");
        // Outer issues join carries no manager guards (ported bug 5): the
        // outer WHERE (the final WHERE — sea-query renders top-level
        // conjuncts without wrapping parens) has no triage /
        // archived-issue / draft guard...
        let outer_where = sql.rsplit("WHERE ").next().unwrap_or("");
        assert!(!outer_where.contains("triage"), "{sql}");
        assert!(!outer_where.contains(r#""issues"."archived_at""#), "{sql}");
        assert!(!outer_where.contains("is_draft"), "{sql}");
        // ...but the correlated subquery does (IssueManager inside).
        assert!(
            sql.contains(r#"U0."parent_id" = ("module_issues"."issue_id")"#),
            "{sql}"
        );
        assert!(
            sql.contains(r#"NOT (U1."group" = 'triage' AND U1."group" IS NOT NULL)"#),
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
        let sql = module_issue_list_get_sql(&OrderBy::default_issue());
        assert!(!sql.contains("SELECT DISTINCT"), "{sql}");
        assert!(sql.contains(r#"FROM "issues""#), "{sql}");
        assert!(sql.contains(ISSUE_MANAGER_OUTER_GUARDS), "{sql}");
        assert!(
            sql.contains(r#""module_issues"."deleted_at" IS NULL"#),
            "{sql}"
        );
        assert!(
            sql.contains(r#""module_issues"."module_id" = ($3)"#),
            "{sql}"
        );
        assert!(sql.contains(r#""issues"."project_id" = ($2)"#), "{sql}");
        assert!(sql.contains(r#""workspaces"."slug" = ($1)"#), "{sql}");
        assert!(
            sql.contains(r#""module_issues"."id" AS "bridge_id""#),
            "{sql}"
        );
        assert!(
            sql.contains(r#"FROM "issue_links" U0 WHERE (U0."deleted_at" IS NULL AND U0."issue_id" = ("issues"."id"))"#),
            "{sql}"
        );
        assert!(
            sql.contains(r#"U0."entity_type" = 'ISSUE_ATTACHMENT'"#),
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
        let m4 = module_issue_list_get_sql(&OrderBy::default_issue());
        let m5 = module_issue_detail_get_sql(&OrderBy::default_issue());
        assert!(m5.contains(r#""issues"."id" = ($4)"#), "{m5}");
        assert!(!m4.contains(r#""issues"."id" = ($4)"#), "{m4}");
        let m5_without_pk = m5.replacen(r#" AND "issues"."id" = ($4)"#, "", 1);
        assert_eq!(
            m5_without_pk, m4,
            "M5 GET differs from M4 GET only by the pk predicate"
        );
    }

    #[test]
    fn m5_delete_lookup_scope() {
        // FX-CYCMOD-05 M5 delete (`module.py:870-875`).
        let sql = module_issue_delete_lookup_sql();
        assert!(sql.contains(r#"FROM "module_issues""#), "{sql}");
        assert!(
            sql.contains(r#""module_issues"."deleted_at" IS NULL"#),
            "{sql}"
        );
        assert!(sql.contains(r#""workspaces"."slug" = ($1)"#), "{sql}");
        assert!(
            sql.contains(r#""module_issues"."project_id" = ($2)"#),
            "{sql}"
        );
        assert!(
            sql.contains(r#""module_issues"."module_id" = ($3)"#),
            "{sql}"
        );
        assert!(
            sql.contains(r#""module_issues"."issue_id" = ($4)"#),
            "{sql}"
        );
        assert!(!sql.contains("LIMIT"), "{sql}");
    }

    #[test]
    fn order_defaults_match_django() {
        assert_eq!(OrderBy::default_module(), OrderBy::new("created_at", true));
        assert_eq!(OrderBy::default_issue(), OrderBy::new("created_at", false));
    }
}
