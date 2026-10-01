//! Cycle read querysets (PIDASHCONV-307, D-20 stage 5).
//!
//! Ports the five `get_queryset` chains in
//! `apps/api/pi_dash/api/views/cycle.py` — plus the `GET` inline `Issue`
//! queryset that shadows the issue-list queryset on the wire, the
//! `cycle_view` date filters, and the `transfer_cycle_issues` read shapes —
//! to executable SQL text (drift baseline `01a93e17`):
//!
//! * Q1 `CycleListCreateAPIEndpoint.get_queryset` (`cycle.py:89-167`),
//!   the list `GET` archived filter (`:197`) and the `cycle_view`
//!   filters (`:198-270`).
//! * Q2 `CycleDetailAPIEndpoint.get_queryset` (`cycle.py:370-448`)
//!   + the detail `GET` archived + pk lookup (`:469`).
//! * Q3 `CycleArchiveUnarchiveAPIEndpoint.get_queryset`
//!   (`cycle.py:622-724`, `archived_at__isnull=False` at `:630`, estimate
//!   sums at `:699-721`).
//! * Q4 `CycleIssueListCreateAPIEndpoint.get_queryset` (`cycle.py:815-837`)
//!   vs the list `GET` inline queryset (`:862-895`).
//! * Q5 `CycleIssueDetailAPIEndpoint.get_queryset` (`cycle.py:1027-1049`,
//!   chain-identical to Q4, dead on the wire) vs the detail `GET`/`DELETE`
//!   `.get()` lookups (`:1069-1074`, `:1092-1097`).
//! * Q6 `transfer_cycle_issues` reads
//!   (`utils/cycle_transfer_issues.py:36-479`): the new-cycle guard
//!   (`:59`), the old-cycle recounts (`:69-143`, `.first()` at `:143`),
//!   the `estimate_type` exists check (`:152-157`), the four assignee/label
//!   distributions (`:165-399`), and the move selection (`:436-443`).
//!
//! Fixture oracle: FX-CYCMOD-04 (`fixtures/v1_cycles_modules/queries/`
//! `cycle.sql` Q1-Q6 + `cycle.rows.json`). Every SQL shape below was
//! verified token-by-token against live Django 4.2 `str(queryset.query)`
//! output (test settings, fixed UUIDs); the unit tests pin the fragments
//! so transcription drift fails the build.
//!
//! # Builder contract
//!
//! Each `*_sql` function returns a complete `SELECT` statement with
//! symbolic bind parameters, following the merged `v1_projects`
//! precedent (`queries_stateest.rs`) and the sibling D-20 port
//! (`module_queries.rs`, PIDASHCONV-308): `$1` is the workspace slug
//! (`workspaces.slug`, text), `$2` the project id (uuid), `$3` the cycle
//! id (uuid, Q2 detail / Q4 / Q5 / Q6), `$4` the acting-user id (uuid,
//! member-scoped reads) or the issue id (uuid, Q5 lookup). Handlers bind
//! them in that order; group literals (`'completed'`, …) and the
//! `'ISSUE_ATTACHMENT'` / `'points'` / avatar-URL literals are embedded
//! because they are fixed values, the same values Django sends as
//! parameters at execution. The `cycle_view` now instant rides `$5` so
//! `$4` stays the acting user on every builder.
//!
//! Dynamic statements (Q1-Q5: kwargs order, archived variants,
//! `cycle_view`) are sea-query builders; the Q6 transfer reads are fully
//! static, so they are `pub const` full-statement strings with `$`
//! placeholders (the merged `license/queries` precedent — there is no
//! build-time database, same as the `license/queries` and
//! `app_integrations/queries_git` precedents). The fixed aggregate
//! fragments are shared `pub fn`/`pub const` pieces spliced into the
//! builders with `Expr::cust`, and the Q6 consts are cross-pinned
//! against those same pieces by the tests, so neither side can drift.
//!
//! Spelling notes (all semantics-preserving, each pinned as-rendered by
//! the tests): sea-query renders `LEFT JOIN` where Django renders `LEFT
//! OUTER JOIN`; the bare `"project_members"."is_active"` predicate
//! renders as `= TRUE` (same convention as the sibling module port);
//! `Count(U0."id")` (capital C, from `Func(..., function="Count")`)
//! renders as `COUNT`; Django's incidental `users T9`/`T10` aliases
//! render as the plain `"users"` table.
//!
//! # Reads scope tables, not views
//!
//! Like `module_queries`, the builders read the physical tables with
//! explicit `deleted_at IS NULL` conjuncts (matching Django's SQL
//! exactly) rather than the `<table>_active` views: the views bake in the
//! same predicate, but the Django text is the contract under test and the
//! `LEFT OUTER JOIN` annotation fanout needs the join conditions visible.
//! Writes still hit the tables (`super::cycle` docs).
//!
//! # Ported bugs and asymmetries (translate, don't redesign)
//!
//! 1. Cycle `Count` annotations carry **no `DISTINCT`** (unlike the
//!    module counts, `module.py:107` et al.): `total_issues` is
//!    `COUNT("cycle_issues"."id")` and each grouped count is
//!    `COUNT("states"."group")` — real row counts, never the module
//!    0/1 shape. Ported as observed via [`count_issues_sql`].
//! 2. Q1/Q2/Q3 join `project_members` (member-active visibility,
//!    `cycle.py:93-96`) and keep the trailing `.distinct()` — both
//!    opposite to the module reads. Ported as observed.
//! 3. The Q4/Q5 querysets have **no** `project__archived_at` guard,
//!    unlike module M4/M5 (`module.py:560`): cycle issue lists include
//!    issues of archived projects. Ported as observed.
//! 4. The Q5 `get_queryset` (`cycle.py:1027-1049`) is **dead on the
//!    wire**: both the detail `GET` and `DELETE` use a plain
//!    `CycleIssue.objects.get(...)` instead. [`cycle_issue_queryset_sql`]
//!    serves it anyway (the chain is identical to Q4's; the fixture Q5
//!    covers it); handlers wire [`cycle_issue_detail_lookup_sql`].
//!    (The reverse of the module asymmetry, where the M5 *GET* exists
//!    but is unrouted.)
//! 5. Relation traversals use the model's **base** manager, so no
//!    `IssueManager` guards (triage / archived issue / archived project /
//!    draft) ride the Q1/Q2/Q3 annotation joins or the Q4/Q5-queryset
//!    outer `issues` join: the explicit `Q` (bridge `deleted_at`, issue
//!    `archived_at`, `is_draft`) is the only issue filter there, and
//!    triage-group issues **count toward the totals**.
//!    `Issue.issue_objects` paths (Q4 GET outer + both
//!    `sub_issues_count` subqueries + all four transfer distributions) DO
//!    carry the full manager guards. Both shapes are kept, never unified.
//! 6. `sub_issues_count` counts children in **every project**: the
//!    correlated subquery restricts `parent_id` only (no project/cycle
//!    scope). Ported as observed.
//! 7. Q3 `total_estimates` has **no `FILTER`** (`cycle.py:699`): it sums
//!    `estimate_points.key` over every fanned-out row, including
//!    archived, draft and soft-deleted-bridge rows. `SUM` over no rows
//!    is `NULL`, not `0` (unlike `COUNT`). Ported as observed.
//! 8. The Q6 old-cycle recounts add an `issue__deleted_at` guard
//!    (`transfer file :78`) the Q1 counts lack, so the transfer
//!    snapshot counts can differ from the list counts when soft-deleted
//!    issues exist. Ported as observed via
//!    [`transfer_count_issues_sql`].
//! 9. The transfer move selection joins `states` with an **inner** join
//!    (`issue__state__group__in`, transfer file `:442`): issues with a
//!    `NULL` `state_id` never move, silently. Ported as observed.
//! 10. Transfer estimate math casts the `value` **CharField** to double
//!     (`Cast("estimate_point__value", FloatField())`): a non-numeric
//!     value fails at the database. Ported as observed (`::double
//!     precision`).
//! 11. The move selection keeps the model's default `-created_at`
//!     ordering: the iteration order feeds the
//!     `update_cycle_issue_activity` list inside the `issue_activity`
//!     payload, so the `ORDER BY` is load-bearing for byte parity.
//!     Ported as observed.
//! 12. `GROUP BY` narrows to the base-table id (Django groups by all
//!     joined PKs; the tenant/owner joins are N:1 off the base id, so
//!     the groups are identical — same documented narrowing as the
//!     sibling module port).
//!
//! Out of scope (sibling D-20 issues): response envelopes and serializer
//! field selection (handlers, PIDASHCONV-362), `ProjectEntityPermission`
//! gates (PIDASHCONV-309), activity enqueues (PIDASHCONV-310), the
//! `progress_snapshot` save and the `bulk_update` writes (handlers),
//! `burndown_plot` computation (the D-27 `burndown_*_sql` precedent in
//! `pidash_api::app_cycles::handlers_archive`; the transfer flow takes
//! its outputs as caller-supplied facts), prefetch round trips
//! (`assignees`, `labels` arrive via separate queries — no SQL fanout —
//! and stay handler-owned).

use sea_query::{Alias, Condition, Expr, IntoIden, JoinType, Order, Query, TableRef};

use super::cycle::{self, cycle_issue};
use crate::v1_projects::models::{estimate_point, project, project_member, state};

/// `workspaces` table (no db-layer port owns it yet; literal matches the
/// Django table name, same as `queries_stateest::WORKSPACE_TABLE`).
const WORKSPACE_TABLE: &str = "workspaces";
/// `issues` table (owned by the issues-domain port; literal matches the
/// Django table name — the Q4 GET projection owner).
const ISSUE_TABLE: &str = "issues";
/// `users` table behind `select_related("owned_by")` (Q1/Q2/Q3) and the
/// transfer assignee distributions.
const USER_TABLE: &str = "users";
/// `issue_links` table behind `link_count` (Q4 GET).
const ISSUE_LINK_TABLE: &str = "issue_links";
/// `file_assets` table behind `attachment_count` (Q4 GET).
const FILE_ASSET_TABLE: &str = "file_assets";
// NOTE: the transfer distributions join `issue_assignees`,
// `issue_labels` and `labels` too, but those statements are static
// `pub const` text (below), so the table names live inline there,
// pinned by the distribution tests — no builder const needed.

// ---------------------------------------------------------------------------
// Shared scope inputs
// ---------------------------------------------------------------------------

/// Which `archived_at` predicate a cycle read applies
/// (`cycle.py:197` live, `:469` live + pk, `:630` archived).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchivedFilter {
    /// `archived_at IS NULL` (Q1 list GET, Q2 detail GET).
    Live,
    /// `archived_at IS NOT NULL` (Q3 archived list).
    Archived,
    /// No archived predicate (bare querysets; Q4/Q5 issue paths, which
    /// never filter on the cycle's `archived_at` at all).
    Any,
}

/// A parsed `.order_by(...)` argument: the raw column plus direction.
///
/// Django passes `self.kwargs.get("order_by", <default>)` through
/// untouched (`cycle.py:165,446,722,835,1047`); the Q4 `GET` inline
/// chain instead reads `request.GET.get("order_by", "created_at")`
/// (`:861`) — a real query param defaulting to **ascending**. The
/// leading `-` selects descending; anything else is ascending, including
/// Django's verbatim behavior for unknown columns (database error at
/// evaluation, like `FieldError`). Parsing lives in the services layer
/// (`services::v1_cycles_modules::cycle_queries`, PIDASHCONV-307);
/// builders quote the column onto their base table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderBy {
    /// Raw column text (e.g. `created_at`).
    pub column: String,
    /// True when the raw text starts with `-`.
    pub descending: bool,
}

impl OrderBy {
    /// `OrderBy` for a literal `.order_by(...)` argument.
    pub fn new(column: impl Into<String>, descending: bool) -> Self {
        Self {
            column: column.into(),
            descending,
        }
    }

    /// The Q1/Q2/Q3/Q4-queryset/Q5-queryset default (`"-created_at"`).
    pub fn default_cycle() -> Self {
        Self::new("created_at", true)
    }

    /// The Q4 GET default (`"created_at"`, ascending).
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

/// The `cycle_view` query param on the Q1 list `GET`
/// (`cycle.py:198-270`, default `"all"`). Unknown values fall through the
/// `if` chain to the plain unfiltered list — i.e. they behave as
/// [`CycleView::All`]. Parsing lives in the services layer;
/// [`cycle_view_condition`] compiles the predicate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CycleView {
    /// No date predicate (explicit `"all"` or anything unrecognized).
    All,
    /// `start_date <= now AND end_date >= now` (`:202`).
    Current,
    /// `start_date > now` (`:214`).
    Upcoming,
    /// `end_date < now` (`:229`).
    Completed,
    /// `end_date IS NULL AND start_date IS NULL` (`:244`).
    Draft,
    /// `end_date >= now OR end_date IS NULL` (`:259`).
    Incomplete,
}

/// The `cycle_view` predicate for the Q1 list `GET`
/// (`cycle.py:202-259`). `now_bind` is the bind placeholder carrying the
/// `timezone.now()` instant (the composed builder passes `"$5"`); the
/// `Draft` arm takes no bind. Every comparison is on the `cycles` base
/// table; `NULL` dates compare `NULL` (never match) except in the arms
/// that test `IS NULL` explicitly — exactly like Django.
pub fn cycle_view_condition(view: CycleView, now_bind: &str) -> Condition {
    let cycles = || Alias::new(cycle::TABLE.to_owned());
    let now = Expr::cust(now_bind);
    match view {
        CycleView::All => Condition::all(),
        CycleView::Current => Condition::all()
            .add(Expr::col((cycles(), Alias::new("start_date"))).lte(now.clone()))
            .add(Expr::col((cycles(), Alias::new("end_date"))).gte(now)),
        CycleView::Upcoming => {
            Condition::all().add(Expr::col((cycles(), Alias::new("start_date"))).gt(now))
        }
        CycleView::Completed => {
            Condition::all().add(Expr::col((cycles(), Alias::new("end_date"))).lt(now))
        }
        CycleView::Draft => Condition::all()
            .add(Expr::col((cycles(), Alias::new("end_date"))).is_null())
            .add(Expr::col((cycles(), Alias::new("start_date"))).is_null()),
        CycleView::Incomplete => Condition::any()
            .add(Expr::col((cycles(), Alias::new("end_date"))).gte(now))
            .add(Expr::col((cycles(), Alias::new("end_date"))).is_null()),
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
/// (every `select_related("workspace")` / `workspace__slug` traversal).
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

/// `INNER JOIN "project_members" ON ("projects"."id" =
/// "project_members"."project_id")` (every
/// `project__project_projectmember` traversal, `cycle.py:93-96` et al.).
fn join_member(sel: &mut sea_query::SelectStatement) {
    sel.join(
        JoinType::InnerJoin,
        Alias::new(project_member::TABLE.to_owned()),
        Condition::all().add(
            Expr::col((Alias::new(project::TABLE), Alias::new("id")))
                .equals((Alias::new(project_member::TABLE), Alias::new("project_id"))),
        ),
    );
}

/// Tenant scope shared by the slug/project reads: this workspace slug,
/// this project (`cycle.py:91-92` et al.). Conjunct order follows Django
/// (slug before project). Soft-delete scoping (`deleted_at IS NULL`)
/// comes from each model's manager and is applied per table at the call
/// site — `projects` / `workspaces` carry NO deleted predicate (verified
/// against live Django SQL: only the base table's manager scope renders).
fn tenant_condition(table: &str) -> Condition {
    Condition::all()
        .add(
            Expr::col((Alias::new(WORKSPACE_TABLE.to_owned()), Alias::new("slug")))
                .eq(Expr::cust("$1")),
        )
        .add(
            Expr::col((Alias::new(table.to_owned()), Alias::new("project_id")))
                .eq(Expr::cust("$2")),
        )
}

/// Member-visibility conjuncts (`cycle.py:93-96` et al.): the acting user
/// (`$4`) is an active member of the project. Inlined at each call site
/// (flat `AND` chain, like Django) rather than nested: Django renders the
/// `is_active=True` conjunct bare; sea-query renders `= TRUE` —
/// identical semantics, pinned as-rendered.
fn add_member_scope(scope: Condition) -> Condition {
    scope
        .add(Expr::col((Alias::new(project_member::TABLE), Alias::new("is_active"))).eq(true))
        .add(
            Expr::col((Alias::new(project_member::TABLE), Alias::new("member_id")))
                .eq(Expr::cust("$4")),
        )
}

// ---------------------------------------------------------------------------
// Q1/Q2/Q3 count annotations
// ---------------------------------------------------------------------------

/// State-group restriction of a count annotation (`cycle.py:110-164`).
/// `All` is the total (no group filter).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupFilter {
    /// No group restriction (`total_issues`).
    All,
    /// `issue_cycle__issue__state__group = '<group>'`.
    Eq(&'static str),
}

/// The five state groups in annotation order (`cycle.py:111-163`);
/// `None` is the total (no group filter).
pub const COUNT_GROUPS: &[Option<&str>] = &[
    Some("completed"),
    Some("cancelled"),
    Some("started"),
    Some("unstarted"),
    Some("backlog"),
    None,
];

/// The six count annotation aliases in source order (`cycle.py:100-164`).
pub const COUNT_ALIASES: &[&str] = &[
    "total_issues",
    "completed_issues",
    "cancelled_issues",
    "started_issues",
    "unstarted_issues",
    "backlog_issues",
];

/// Explicit issue-liveness filter shared by all six counts
/// (`cycle.py:103-107` and each group `Q`): the bridge is live, the
/// issue is not archived and not a draft. There is deliberately NO
/// triage / project-archived / issue soft-delete guard — relation
/// traversals use the base manager (ported bug 5).
const LIVE_ISSUE_FILTER: &str = r#""cycle_issues"."deleted_at" IS NULL AND "issues"."archived_at" IS NULL AND NOT "issues"."is_draft""#;

/// One `Count(...)` annotation as Django renders it
/// (`cycle.py:100-164`, verified against live `str(query)` output) —
/// with **no `DISTINCT`** (ported bug 1, the reverse of the module
/// shape):
///
/// * total (`GroupFilter::All`): `COUNT("cycle_issues"."id")` — a real
///   row count.
/// * grouped: `COUNT("states"."group")` — a real row count (`COUNT`
///   skips `NULL`, and every row passing the single-group `FILTER` is
///   non-null). The `FILTER` carries the same liveness guard plus
///   `"states"."group" = '<group>'`.
pub fn count_issues_sql(filter: GroupFilter, alias: &str) -> String {
    match filter {
        GroupFilter::All => format!(
            r#"COUNT("cycle_issues"."id") FILTER (WHERE ({LIVE_ISSUE_FILTER})) AS "{alias}""#
        ),
        GroupFilter::Eq(group) => format!(
            r#"COUNT("states"."group") FILTER (WHERE ({LIVE_ISSUE_FILTER} AND "states"."group" = '{group}')) AS "{alias}""#
        ),
    }
}

/// All six count annotations in source order, for the Q1/Q2/Q3 select
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
// Q1/Q2/Q3 cycle reads
// ---------------------------------------------------------------------------

/// Shared Q1/Q2/Q3 `FROM` + `JOIN` block (`cycle.py:91-99,372-380,624-633`):
/// tenant tables inner-joined (`select_related("project")`,
/// `select_related("workspace")`), the member-visibility traversal
/// inner-joined (`project__project_projectmember`), annotation sources
/// left-joined (`issue_cycle` reverse FK, `issue`, `state`), owner
/// inner-joined (`select_related("owned_by")` — `owned_by` is a
/// non-nullable FK, so Django uses `INNER JOIN`). `with_estimates`
/// adds the Q3 `estimate_points` left join
/// (`issue_cycle__issue__estimate_point`, `:699`).
fn join_cycle_reads(sel: &mut sea_query::SelectStatement, with_estimates: bool) {
    let cycles = Alias::new(cycle::TABLE.to_owned());
    join_workspace(sel, cycle::TABLE);
    join_project(sel, cycle::TABLE);
    join_member(sel);
    // LEFT OUTER JOIN "cycle_issues" ON ("cycles"."id" = "cycle_issues"."cycle_id")
    sel.join(
        JoinType::LeftJoin,
        Alias::new(cycle_issue::TABLE.to_owned()),
        Condition::all().add(
            Expr::col((cycles.clone(), Alias::new("id")))
                .equals((Alias::new(cycle_issue::TABLE), Alias::new("cycle_id"))),
        ),
    );
    // LEFT OUTER JOIN "issues" ON ("cycle_issues"."issue_id" = "issues"."id")
    sel.join(
        JoinType::LeftJoin,
        Alias::new(ISSUE_TABLE.to_owned()),
        Condition::all().add(
            Expr::col((Alias::new(cycle_issue::TABLE), Alias::new("issue_id")))
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
    if with_estimates {
        // LEFT OUTER JOIN "estimate_points" ON ("issues"."estimate_point_id" = "estimate_points"."id")
        sel.join(
            JoinType::LeftJoin,
            Alias::new(estimate_point::TABLE.to_owned()),
            Condition::all().add(
                Expr::col((Alias::new(ISSUE_TABLE), Alias::new("estimate_point_id")))
                    .equals((Alias::new(estimate_point::TABLE), Alias::new("id"))),
            ),
        );
    }
    // INNER JOIN "users" ON ("cycles"."owned_by_id" = "users"."id")
    // (Django aliases this `T9`/`T10`; the alias is incidental join-order
    // numbering with identical semantics, so the plain table renders).
    sel.join(
        JoinType::InnerJoin,
        Alias::new(USER_TABLE.to_owned()),
        Condition::all().add(
            Expr::col((cycles, Alias::new("owned_by_id")))
                .equals((Alias::new(USER_TABLE), Alias::new("id"))),
        ),
    );
}

/// Shared Q1/Q2/Q3 `WHERE` head: the base-table manager scope
/// (`cycles.deleted_at IS NULL`) plus the tenant scope plus the
/// member-visibility scope. The `projects` / `workspaces` /
/// `project_members` joins carry no deleted predicate (verified against
/// live Django SQL).
fn cycle_list_where() -> Condition {
    let scope = Condition::all()
        .add(Expr::col((Alias::new(cycle::TABLE), Alias::new("deleted_at"))).is_null())
        .add(tenant_condition(cycle::TABLE));
    add_member_scope(scope)
}

/// Q1 cycle list (`CycleListCreateAPIEndpoint.get_queryset`,
/// `cycle.py:89-167`, + `archived_at IS NULL` from the list `GET` at
/// `:197`): `SELECT DISTINCT` base columns in [`cycle::COLUMNS`] order,
/// the six count annotations, member-visibility scope, `GROUP BY
/// "cycles"."id"`, kwargs order.
///
/// Django groups by all four joined PKs; the tenant/member/owner joins
/// are N:1 off `cycles.id`, so grouping by `cycles.id` alone yields
/// identical groups (ported bug 12 — same documented narrowing as the
/// sibling module port).
///
/// Binds: `$1` slug, `$2` project id, `$4` acting-user id.
pub fn cycle_list_sql(order: &OrderBy, archived: ArchivedFilter) -> String {
    cycle_list_sql_inner(order, archived, CycleView::All, "$5")
}

/// Q1 list `GET` with a `cycle_view` date filter (`cycle.py:198-270`):
/// the live Q1 list plus [`cycle_view_condition`]. `CycleView::All`
/// renders no extra predicate (identical text to
/// [`cycle_list_sql`] with [`ArchivedFilter::Live`]).
///
/// Binds: `$1` slug, `$2` project id, `$4` acting-user id, `$5` the
/// `timezone.now()` instant (timestamptz; unused by the `All`/`Draft`
/// arms).
pub fn cycle_list_filtered_sql(order: &OrderBy, view: CycleView) -> String {
    cycle_list_sql_inner(order, ArchivedFilter::Live, view, "$5")
}

fn cycle_list_sql_inner(
    order: &OrderBy,
    archived: ArchivedFilter,
    view: CycleView,
    now_bind: &str,
) -> String {
    use sea_query::PostgresQueryBuilder;
    let mut sel = Query::select();
    sel.distinct();
    select_table_columns(&mut sel, cycle::TABLE, cycle::COLUMNS);
    for annotation in all_count_annotations_sql() {
        sel.expr(Expr::cust(annotation));
    }
    sel.from(Alias::new(cycle::TABLE.to_owned()));
    join_cycle_reads(&mut sel, false);
    let mut scope = cycle_list_where();
    match archived {
        ArchivedFilter::Live => {
            scope = scope
                .add(Expr::col((Alias::new(cycle::TABLE), Alias::new("archived_at"))).is_null());
        }
        ArchivedFilter::Archived => {
            scope = scope.add(
                Expr::col((Alias::new(cycle::TABLE), Alias::new("archived_at"))).is_not_null(),
            );
        }
        ArchivedFilter::Any => {}
    }
    if view != CycleView::All {
        scope = scope.add(cycle_view_condition(view, now_bind));
    }
    sel.cond_where(scope);
    sel.group_by_col((Alias::new(cycle::TABLE), Alias::new("id")));
    sel.order_by(
        (
            Alias::new(cycle::TABLE.to_owned()),
            Alias::new(order.column.clone()),
        ),
        order.order(),
    );
    sel.to_string(PostgresQueryBuilder)
}

/// Q2 cycle detail (`CycleDetailAPIEndpoint.get_queryset`,
/// `cycle.py:370-448`, + `.filter(archived_at__isnull=True).get(pk=pk)`
/// from the detail `GET` at `:469`): identical to the live Q1 plus the
/// pk predicate. The chain text is byte-identical to Q1's; only the GET
/// wrapper differs.
///
/// Binds: `$1` slug, `$2` project id, `$3` cycle id, `$4` acting-user id.
pub fn cycle_detail_sql(order: &OrderBy) -> String {
    use sea_query::PostgresQueryBuilder;
    let mut sel = Query::select();
    sel.distinct();
    select_table_columns(&mut sel, cycle::TABLE, cycle::COLUMNS);
    for annotation in all_count_annotations_sql() {
        sel.expr(Expr::cust(annotation));
    }
    sel.from(Alias::new(cycle::TABLE.to_owned()));
    join_cycle_reads(&mut sel, false);
    sel.cond_where(
        cycle_list_where()
            .add(Expr::col((Alias::new(cycle::TABLE), Alias::new("archived_at"))).is_null())
            .add(Expr::col((Alias::new(cycle::TABLE), Alias::new("id"))).eq(Expr::cust("$3"))),
    );
    sel.group_by_col((Alias::new(cycle::TABLE), Alias::new("id")));
    sel.order_by(
        (
            Alias::new(cycle::TABLE.to_owned()),
            Alias::new(order.column.clone()),
        ),
        order.order(),
    );
    sel.to_string(PostgresQueryBuilder)
}

/// The three Q3 estimate-sum aliases in source order (`cycle.py:699-721`).
pub const ESTIMATE_ALIASES: &[&str] = &[
    "total_estimates",
    "completed_estimates",
    "started_estimates",
];

/// One Q3 `Sum("issue_cycle__issue__estimate_point__key")` annotation as
/// Django renders it (`cycle.py:699-721`, verified against live
/// `str(query)` output). The total (`GroupFilter::All`) carries **no
/// `FILTER`** (ported bug 7); the grouped sums carry the same liveness
/// guard plus the group equality. `key` is the `IntegerField`, so the
/// sums are integer sums (unlike the transfer's `value`-cast doubles).
pub fn estimate_sum_sql(filter: GroupFilter, alias: &str) -> String {
    match filter {
        GroupFilter::All => format!(
            r#"SUM("{ep}"."key") AS "{alias}""#,
            ep = estimate_point::TABLE
        ),
        GroupFilter::Eq(group) => format!(
            r#"SUM("{ep}"."key") FILTER (WHERE ({LIVE_ISSUE_FILTER} AND "states"."group" = '{group}')) AS "{alias}""#,
            ep = estimate_point::TABLE,
        ),
    }
}

/// All three estimate sums in source order, for the Q3 select list.
pub fn all_estimate_sums_sql() -> Vec<String> {
    vec![
        estimate_sum_sql(GroupFilter::All, "total_estimates"),
        estimate_sum_sql(GroupFilter::Eq("completed"), "completed_estimates"),
        estimate_sum_sql(GroupFilter::Eq("started"), "started_estimates"),
    ]
}

/// Q3 archived-cycle list (`CycleArchiveUnarchiveAPIEndpoint.get_queryset`,
/// `cycle.py:622-724`): same chain as Q1 with
/// `archived_at__isnull=False` (`:630`) **plus** the three estimate sums
/// (`:699-721`) and the `estimate_points` left join — the asymmetry vs
/// the archived-module list, which has no estimates.
///
/// Binds: `$1` slug, `$2` project id, `$4` acting-user id.
pub fn archived_cycle_list_sql(order: &OrderBy) -> String {
    use sea_query::PostgresQueryBuilder;
    let mut sel = Query::select();
    sel.distinct();
    select_table_columns(&mut sel, cycle::TABLE, cycle::COLUMNS);
    for annotation in all_count_annotations_sql() {
        sel.expr(Expr::cust(annotation));
    }
    for annotation in all_estimate_sums_sql() {
        sel.expr(Expr::cust(annotation));
    }
    sel.from(Alias::new(cycle::TABLE.to_owned()));
    join_cycle_reads(&mut sel, true);
    sel.cond_where(
        cycle_list_where()
            .add(Expr::col((Alias::new(cycle::TABLE), Alias::new("archived_at"))).is_not_null()),
    );
    sel.group_by_col((Alias::new(cycle::TABLE), Alias::new("id")));
    sel.order_by(
        (
            Alias::new(cycle::TABLE.to_owned()),
            Alias::new(order.column.clone()),
        ),
        order.order(),
    );
    sel.to_string(PostgresQueryBuilder)
}

// ---------------------------------------------------------------------------
// Q4/Q5 issue querysets (CycleIssue-based)
// ---------------------------------------------------------------------------

/// `sub_issues_count` correlated subquery (`cycle.py:817-822,864-869,
/// 1029-1034`, verified against live `str(query)` output):
/// `Issue.issue_objects.filter(parent=OuterRef(...))` — so the FULL
/// `IssueManager` guards render inside (triage / archived issue /
/// archived project / draft excluded, soft-delete excluded), with the
/// `states` join nullable (`LEFT OUTER JOIN`, hence the NULL-safe triage
/// guard). `Count` is NOT distinct (`Func(F("id"), function="Count")`
/// renders `COUNT`, capital-C spelling normalized). The subquery
/// restricts `parent_id` ONLY — no project or cycle scope (ported bug 6).
///
/// `outer` is the correlated parent column: `"cycle_issues"."issue_id"`
/// for the Q4/Q5 querysets, `"issues"."id"` for the Q4 GET shape.
pub fn sub_issues_count_sql(outer: &str) -> String {
    format!(
        r#"(SELECT COUNT(U0."id") AS "count" FROM "issues" U0 LEFT OUTER JOIN "states" U1 ON (U0."state_id" = U1."id") INNER JOIN "projects" U2 ON (U0."project_id" = U2."id") WHERE (U0."deleted_at" IS NULL AND NOT (U1."group" = 'triage' AND U1."group" IS NOT NULL) AND NOT (U0."archived_at" IS NOT NULL) AND NOT (U2."archived_at" IS NOT NULL) AND NOT (U0."is_draft") AND U0."parent_id" = ({outer}))) AS "sub_issues_count""#
    )
}

/// Q4/Q5 queryset (`CycleIssueListCreateAPIEndpoint.get_queryset`,
/// `cycle.py:815-837`; Q5's `cycle.py:1027-1049` is chain-identical):
/// `SELECT DISTINCT` bridge columns in [`cycle_issue::COLUMNS`] order +
/// [`sub_issues_count_sql`] correlated to the bridge's issue.
///
/// Joins (verified order): `issues` inner (`select_related("issue",
/// "issue__state", "issue__project")` — the issue's own project arrives
/// as the `T8` self-join), `workspaces` inner, `projects` inner,
/// `project_members` inner (the member-active visibility traversal),
/// `cycles` inner (`select_related("cycle")`), `projects T8` inner (the
/// issue's project), `states` left outer. The outer `issues` join
/// carries NO `IssueManager` guards — the traversal uses the base
/// manager (ported bug 5): archived / draft / triage issues list while
/// their bridge is live. There is NO `projects.archived_at` predicate
/// (ported bug 3 — the asymmetry vs the module querysets) and NO
/// cycle-`archived_at` predicate — issues of archived cycles still list.
///
/// `WHERE` order follows Django: bridge deleted scope, slug, project,
/// member `is_active` + member id, bridge cycle id.
///
/// The Q5 detail `GET`/`DELETE` do NOT use this shape (ported bug 4);
/// the Q4 list `POST` re-read (`:1008`) does.
///
/// Binds: `$1` slug, `$2` project id, `$3` cycle id, `$4` acting-user id.
pub fn cycle_issue_queryset_sql(order: &OrderBy) -> String {
    use sea_query::PostgresQueryBuilder;
    let bridges = Alias::new(cycle_issue::TABLE.to_owned());
    let mut sel = Query::select();
    sel.distinct();
    select_table_columns(&mut sel, cycle_issue::TABLE, cycle_issue::COLUMNS);
    sel.expr(Expr::cust(sub_issues_count_sql(
        r#""cycle_issues"."issue_id""#,
    )));
    sel.from(Alias::new(cycle_issue::TABLE.to_owned()));
    // INNER JOIN "issues" ON ("cycle_issues"."issue_id" = "issues"."id")
    sel.join(
        JoinType::InnerJoin,
        Alias::new(ISSUE_TABLE.to_owned()),
        Condition::all().add(
            Expr::col((bridges.clone(), Alias::new("issue_id")))
                .equals((Alias::new(ISSUE_TABLE), Alias::new("id"))),
        ),
    );
    join_workspace(&mut sel, cycle_issue::TABLE);
    join_project(&mut sel, cycle_issue::TABLE);
    join_member(&mut sel);
    // INNER JOIN "cycles" ON ("cycle_issues"."cycle_id" = "cycles"."id")
    sel.join(
        JoinType::InnerJoin,
        Alias::new(cycle::TABLE.to_owned()),
        Condition::all().add(
            Expr::col((bridges.clone(), Alias::new("cycle_id")))
                .equals((Alias::new(cycle::TABLE), Alias::new("id"))),
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
    let scope = Condition::all()
        .add(Expr::col((bridges.clone(), Alias::new("deleted_at"))).is_null())
        .add(
            Expr::col((Alias::new(WORKSPACE_TABLE.to_owned()), Alias::new("slug")))
                .eq(Expr::cust("$1")),
        )
        .add(Expr::col((bridges.clone(), Alias::new("project_id"))).eq(Expr::cust("$2")));
    let scope = add_member_scope(scope);
    let scope = scope.add(Expr::col((bridges, Alias::new("cycle_id"))).eq(Expr::cust("$3")));
    sel.cond_where(scope);
    sel.order_by(
        (
            Alias::new(cycle_issue::TABLE.to_owned()),
            Alias::new(order.column.clone()),
        ),
        order.order(),
    );
    sel.to_string(PostgresQueryBuilder)
}

/// `link_count` scalar annotation (Q4 GET, `cycle.py:880-885`, verified
/// against live `str(query)` output): `IssueLink.objects` uses the
/// soft-delete default manager, so `deleted_at IS NULL` renders.
pub fn link_count_sql() -> String {
    format!(
        r#"(SELECT COUNT(U0."id") AS "count" FROM "{ISSUE_LINK_TABLE}" U0 WHERE (U0."deleted_at" IS NULL AND U0."issue_id" = ("issues"."id"))) AS "link_count""#
    )
}

/// `attachment_count` scalar annotation (Q4 GET, `cycle.py:886-894`):
/// same soft-delete manager scope plus the
/// `entity_type = 'ISSUE_ATTACHMENT'` literal
/// (`FileAsset.EntityTypeContext.ISSUE_ATTACHMENT`).
pub fn attachment_count_sql() -> String {
    format!(
        r#"(SELECT COUNT(U0."id") AS "count" FROM "{FILE_ASSET_TABLE}" U0 WHERE (U0."deleted_at" IS NULL AND U0."entity_type" = 'ISSUE_ATTACHMENT' AND U0."issue_id" = ("issues"."id"))) AS "attachment_count""#
    )
}

/// `bridge_id` annotation (Q4 GET, `cycle.py:870`):
/// `.annotate(bridge_id=F("issue_cycle__id"))` — the live bridge's pk
/// via the `INNER JOIN "cycle_issues"`. One row per live bridge: an
/// issue bridged twice (deleted-then-recreated rows excepted by the
/// `deleted_at` guard) fans out into two rows — ported as observed.
pub fn bridge_id_sql() -> String {
    r#""cycle_issues"."id" AS "bridge_id""#.to_owned()
}

/// Outer `IssueManager` guards for the Q4 GET shape
/// (`db/models/issue.py:95-104`, verified against live `str(query)`
/// output): soft-delete, non-triage states (NULL-safe — the `states`
/// join is a `LEFT OUTER JOIN`), live issues, live projects, non-drafts.
/// These apply ONLY to `Issue.issue_objects` entry points (the GET outer
/// query, both `sub_issues_count` subqueries and the four transfer
/// distributions), never to relation traversals (ported bug 5).
pub const ISSUE_MANAGER_OUTER_GUARDS: &str = r#""issues"."deleted_at" IS NULL AND NOT ("states"."group" = 'triage' AND "states"."group" IS NOT NULL) AND NOT ("issues"."archived_at" IS NOT NULL) AND NOT ("projects"."archived_at" IS NOT NULL) AND NOT ("issues"."is_draft")"#;

/// Q4 issue list `GET` inline queryset (`cycle.py:862-895`): the shape
/// the wire actually serves (the `post` re-read at `:1008` uses the Q4
/// queryset instead). `Issue.issue_objects` outer with the four
/// annotations (`sub_issues_count`, `bridge_id`, `link_count`,
/// `attachment_count`), tenant scope, `ORDER BY "issues"."created_at"
/// ASC` default (`request.GET.get("order_by", "created_at")` at `:861` —
/// note the ASC default vs the queryset's `-created_at`).
///
/// Joins (verified order): `states` left outer, `projects` inner,
/// `cycle_issues` inner (the `issue_cycle__cycle_id` traversal),
/// `workspaces` inner, parent `T7` left outer (`select_related("parent")`).
/// No `DISTINCT`, no member join, no cycle-`archived_at` predicate.
///
/// The `issues` projection is `"issues".*`: Django projects every issue
/// column explicitly; same row content. (The issues-domain port owns the
/// typed column list; this layer ports the scope, joins and annotations.)
///
/// Binds: `$1` slug, `$2` project id, `$3` cycle id.
pub fn cycle_issue_list_get_sql(order: &OrderBy) -> String {
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
    // INNER JOIN "cycle_issues" ON ("issues"."id" = "cycle_issues"."issue_id")
    sel.join(
        JoinType::InnerJoin,
        Alias::new(cycle_issue::TABLE.to_owned()),
        Condition::all().add(
            Expr::col((Alias::new(ISSUE_TABLE), Alias::new("id")))
                .equals((Alias::new(cycle_issue::TABLE), Alias::new("issue_id"))),
        ),
    );
    join_workspace(&mut sel, ISSUE_TABLE);
    // LEFT OUTER JOIN "issues" AS "T7" ON ("issues"."parent_id" = T7."id")
    // (select_related("parent")).
    sel.join(
        JoinType::LeftJoin,
        TableRef::Table(Alias::new(ISSUE_TABLE.to_owned()).into_iden()).alias(Alias::new("T7")),
        Condition::all().add(
            Expr::col((Alias::new(ISSUE_TABLE), Alias::new("parent_id")))
                .equals((Alias::new("T7"), Alias::new("id"))),
        ),
    );
    sel.cond_where(
        Condition::all()
            .add(Expr::cust(ISSUE_MANAGER_OUTER_GUARDS))
            .add(
                Expr::col((Alias::new(cycle_issue::TABLE), Alias::new("cycle_id")))
                    .eq(Expr::cust("$3")),
            )
            .add(Expr::col((Alias::new(cycle_issue::TABLE), Alias::new("deleted_at"))).is_null())
            .add(
                Expr::col((Alias::new(ISSUE_TABLE), Alias::new("project_id"))).eq(Expr::cust("$2")),
            )
            .add(
                Expr::col((Alias::new(WORKSPACE_TABLE.to_owned()), Alias::new("slug")))
                    .eq(Expr::cust("$1")),
            ),
    );
    sel.order_by(
        (
            Alias::new(ISSUE_TABLE.to_owned()),
            Alias::new(order.column.clone()),
        ),
        order.order(),
    );
    sel.to_string(PostgresQueryBuilder)
}

/// Q5 detail `GET`/`DELETE` lookup (`CycleIssueDetailAPIEndpoint.get`,
/// `cycle.py:1069-1074`, and `delete`, `cycle.py:1092-1097` — both spell
/// the same `CycleIssue.objects.get(workspace__slug, project_id,
/// cycle_id, issue_id)` and Django renders both identically regardless
/// of kwarg order, verified against live output): default-manager scope
/// (`deleted_at IS NULL`) plus the four equality predicates, in Django's
/// deterministic conjunct order (base-table cycle/issue/project, then
/// the joined slug). Django's `.get()` adds no `LIMIT` (0 rows →
/// `DoesNotExist` → 404, 2+ → `MultipleObjectsReturned` → 500) and its
/// `Meta.ordering` `ORDER BY` is order-irrelevant for a single-row
/// fetch, so neither renders here; handlers own the exception mapping.
///
/// Binds: `$1` slug, `$2` project id, `$3` cycle id, `$4` issue id.
pub fn cycle_issue_detail_lookup_sql() -> String {
    use sea_query::PostgresQueryBuilder;
    let bridges = Alias::new(cycle_issue::TABLE.to_owned());
    let mut sel = Query::select();
    select_table_columns(&mut sel, cycle_issue::TABLE, cycle_issue::COLUMNS);
    sel.from(Alias::new(cycle_issue::TABLE.to_owned()));
    join_workspace(&mut sel, cycle_issue::TABLE);
    sel.cond_where(
        Condition::all()
            .add(Expr::col((bridges.clone(), Alias::new("deleted_at"))).is_null())
            .add(Expr::col((bridges.clone(), Alias::new("cycle_id"))).eq(Expr::cust("$3")))
            .add(Expr::col((bridges.clone(), Alias::new("issue_id"))).eq(Expr::cust("$4")))
            .add(Expr::col((bridges.clone(), Alias::new("project_id"))).eq(Expr::cust("$2")))
            .add(
                Expr::col((Alias::new(WORKSPACE_TABLE.to_owned()), Alias::new("slug")))
                    .eq(Expr::cust("$1")),
            ),
    );
    sel.to_string(PostgresQueryBuilder)
}

// ---------------------------------------------------------------------------
// Q6 transfer reads (static statements)
// ---------------------------------------------------------------------------

/// Explicit issue-liveness filter of the transfer old-cycle recounts
/// (transfer file `:74-79` and each group `Q`): the Q1 filter plus the
/// issue soft-delete guard (ported bug 8). Condition order follows
/// Django, not the `Q()` kwarg order.
const TRANSFER_LIVE_ISSUE_FILTER: &str = r#""cycle_issues"."deleted_at" IS NULL AND "issues"."archived_at" IS NULL AND "issues"."deleted_at" IS NULL AND NOT "issues"."is_draft""#;

/// One old-cycle recount annotation as Django renders it (transfer file
/// `:71-141`, verified against live `str(query)` output): the
/// [`count_issues_sql`] shape with the extra issue soft-delete guard.
/// No `DISTINCT`, like Q1.
pub fn transfer_count_issues_sql(filter: GroupFilter, alias: &str) -> String {
    match filter {
        GroupFilter::All => format!(
            r#"COUNT("cycle_issues"."id") FILTER (WHERE ({TRANSFER_LIVE_ISSUE_FILTER})) AS "{alias}""#
        ),
        GroupFilter::Eq(group) => format!(
            r#"COUNT("states"."group") FILTER (WHERE ({TRANSFER_LIVE_ISSUE_FILTER} AND "states"."group" = '{group}')) AS "{alias}""#
        ),
    }
}

/// All six old-cycle recounts in source order (transfer file `:71-141`).
pub fn all_transfer_counts_sql() -> Vec<String> {
    let mut out = vec![transfer_count_issues_sql(GroupFilter::All, "total_issues")];
    for (group, alias) in [
        ("completed", "completed_issues"),
        ("cancelled", "cancelled_issues"),
        ("started", "started_issues"),
        ("unstarted", "unstarted_issues"),
        ("backlog", "backlog_issues"),
    ] {
        out.push(transfer_count_issues_sql(GroupFilter::Eq(group), alias));
    }
    out
}

/// The transfer's open-state gate (`utils/constants.py:86`):
/// `STATE_GROUP_ORDER[:-2]` — every group except `completed` and
/// `cancelled`. Only issues in these states move
/// (`issue__state__group__in`, transfer file `:442`).
pub const OPEN_STATE_GROUPS: &[&str] = &["backlog", "unstarted", "started", "review", "test"];

/// `avatar_url` select item shared by the assignee distributions
/// (transfer file `:175-193,:296-312`, verified against live `str(query)`
/// output): asset URL when `avatar_asset` is set, the raw `avatar`
/// text when it is null, `NULL` otherwise (unreachable — the two `WHEN`
/// arms partition null/non-null — ported as observed). Django renders
/// the nested `Concat` as nested `CONCAT(...::text)` casts; the
/// `Value` literals below are the executed parameter values.
pub const AVATAR_URL_CASE: &str = r#"CASE WHEN "users"."avatar_asset_id" IS NOT NULL THEN CONCAT(('/api/assets/v2/static/')::text, (CONCAT(("users"."avatar_asset_id")::text, ('/')::text))::text) WHEN "users"."avatar_asset_id" IS NULL THEN "users"."avatar" ELSE NULL END AS "avatar_url""#;

/// The three estimate-distribution sums in source order
/// (`total_estimates`, `completed_estimates`, `pending_estimates`,
/// transfer file `:195-215,:242-262`): `SUM` over the `value`
/// CharField cast to double (ported bug 10), unfiltered total plus the
/// completed/pending `FILTER`s. `NULL` when a group has no valued rows.
pub const ESTIMATE_DIST_SUMS: &str = r#"SUM(("estimate_points"."value")::double precision) AS "total_estimates", SUM(("estimate_points"."value")::double precision) FILTER (WHERE ("issues"."archived_at" IS NULL AND "issues"."completed_at" IS NOT NULL AND NOT "issues"."is_draft")) AS "completed_estimates", SUM(("estimate_points"."value")::double precision) FILTER (WHERE ("issues"."archived_at" IS NULL AND "issues"."completed_at" IS NULL AND NOT "issues"."is_draft")) AS "pending_estimates""#;

/// The three issue-distribution counts in source order
/// (`total_issues`, `completed_issues`, `pending_issues`, transfer file
/// `:314-334,:362-382`): real row counts, unfiltered-alive total plus
/// the completed/pending `FILTER`s.
pub const ISSUE_DIST_COUNTS: &str = r#"COUNT("issues"."id") FILTER (WHERE ("issues"."archived_at" IS NULL AND NOT "issues"."is_draft")) AS "total_issues", COUNT("issues"."id") FILTER (WHERE ("issues"."archived_at" IS NULL AND "issues"."completed_at" IS NOT NULL AND NOT "issues"."is_draft")) AS "completed_issues", COUNT("issues"."id") FILTER (WHERE ("issues"."archived_at" IS NULL AND "issues"."completed_at" IS NULL AND NOT "issues"."is_draft")) AS "pending_issues""#;

/// `WHERE` tail shared by the four transfer distributions (transfer file
/// `:166-171` et al.): the [`ISSUE_MANAGER_OUTER_GUARDS`] head (the
/// entry point is `Issue.issue_objects`), then the cycle-bridge scope,
/// then the tenant scope. Condition order follows Django. The static
/// distribution statements embed this text verbatim (cross-pinned by
/// the tests — Rust `const` cannot concatenate).
pub const TRANSFER_DIST_WHERE: &str = r#""issues"."deleted_at" IS NULL AND NOT ("states"."group" = 'triage' AND "states"."group" IS NOT NULL) AND NOT ("issues"."archived_at" IS NOT NULL) AND NOT ("projects"."archived_at" IS NOT NULL) AND NOT ("issues"."is_draft") AND "cycle_issues"."cycle_id" = ($3) AND "cycle_issues"."deleted_at" IS NULL AND "issues"."project_id" = ($2) AND "workspaces"."slug" = ($1)"#;

/// New-cycle guard lookup (transfer file `:59`) — and the identical
/// `current_cycle` re-read (`:409`): `Cycle.objects.filter(
/// workspace__slug, project_id, pk).first()`. The queryset keeps the
/// model's default `-created_at` ordering (verified against live
/// `str(query)` output — Django applies `Meta.ordering` to ungrouped
/// selects); `.first()` adds `LIMIT 1`. No member join, no archived
/// predicate — the guard reads any cycle in the tenant, live or
/// archived.
///
/// Binds: `$1` slug, `$2` project id, `$3` the new (or current) cycle id.
pub const TRANSFER_CYCLE_LOOKUP_SQL: &str = r#"SELECT "cycles"."id", "cycles"."created_at", "cycles"."updated_at", "cycles"."created_by_id", "cycles"."updated_by_id", "cycles"."deleted_at", "cycles"."project_id", "cycles"."workspace_id", "cycles"."name", "cycles"."description", "cycles"."start_date", "cycles"."end_date", "cycles"."owned_by_id", "cycles"."view_props", "cycles"."sort_order", "cycles"."external_source", "cycles"."external_id", "cycles"."progress_snapshot", "cycles"."archived_at", "cycles"."logo_props", "cycles"."timezone", "cycles"."version" FROM "cycles" INNER JOIN "workspaces" ON ("cycles"."workspace_id" = "workspaces"."id") WHERE ("cycles"."deleted_at" IS NULL AND "cycles"."id" = ($3) AND "cycles"."project_id" = ($2) AND "workspaces"."slug" = ($1)) ORDER BY "cycles"."created_at" DESC LIMIT 1"#;

/// Old-cycle recounts (transfer file `:69-143`): the source cycle row
/// plus the six counts with the extra issue soft-delete guard
/// ([`transfer_count_issues_sql`]). No member join, no `projects` join
/// (the chain filters `project_id` directly on `cycles`), no archived
/// predicate. `.first()` at `:143` adds `ORDER BY "cycles"."id" ASC
/// LIMIT 1` (Django orders an unordered grouped queryset by pk).
///
/// Binds: `$1` slug, `$2` project id, `$3` the source cycle id.
pub const TRANSFER_OLD_CYCLE_SQL: &str = r#"SELECT "cycles"."id", "cycles"."created_at", "cycles"."updated_at", "cycles"."created_by_id", "cycles"."updated_by_id", "cycles"."deleted_at", "cycles"."project_id", "cycles"."workspace_id", "cycles"."name", "cycles"."description", "cycles"."start_date", "cycles"."end_date", "cycles"."owned_by_id", "cycles"."view_props", "cycles"."sort_order", "cycles"."external_source", "cycles"."external_id", "cycles"."progress_snapshot", "cycles"."archived_at", "cycles"."logo_props", "cycles"."timezone", "cycles"."version", COUNT("cycle_issues"."id") FILTER (WHERE ("cycle_issues"."deleted_at" IS NULL AND "issues"."archived_at" IS NULL AND "issues"."deleted_at" IS NULL AND NOT "issues"."is_draft")) AS "total_issues", COUNT("states"."group") FILTER (WHERE ("cycle_issues"."deleted_at" IS NULL AND "issues"."archived_at" IS NULL AND "issues"."deleted_at" IS NULL AND NOT "issues"."is_draft" AND "states"."group" = 'completed')) AS "completed_issues", COUNT("states"."group") FILTER (WHERE ("cycle_issues"."deleted_at" IS NULL AND "issues"."archived_at" IS NULL AND "issues"."deleted_at" IS NULL AND NOT "issues"."is_draft" AND "states"."group" = 'cancelled')) AS "cancelled_issues", COUNT("states"."group") FILTER (WHERE ("cycle_issues"."deleted_at" IS NULL AND "issues"."archived_at" IS NULL AND "issues"."deleted_at" IS NULL AND NOT "issues"."is_draft" AND "states"."group" = 'started')) AS "started_issues", COUNT("states"."group") FILTER (WHERE ("cycle_issues"."deleted_at" IS NULL AND "issues"."archived_at" IS NULL AND "issues"."deleted_at" IS NULL AND NOT "issues"."is_draft" AND "states"."group" = 'unstarted')) AS "unstarted_issues", COUNT("states"."group") FILTER (WHERE ("cycle_issues"."deleted_at" IS NULL AND "issues"."archived_at" IS NULL AND "issues"."deleted_at" IS NULL AND NOT "issues"."is_draft" AND "states"."group" = 'backlog')) AS "backlog_issues" FROM "cycles" INNER JOIN "workspaces" ON ("cycles"."workspace_id" = "workspaces"."id") LEFT OUTER JOIN "cycle_issues" ON ("cycles"."id" = "cycle_issues"."cycle_id") LEFT OUTER JOIN "issues" ON ("cycle_issues"."issue_id" = "issues"."id") LEFT OUTER JOIN "states" ON ("issues"."state_id" = "states"."id") WHERE ("cycles"."deleted_at" IS NULL AND "cycles"."id" = ($3) AND "cycles"."project_id" = ($2) AND "workspaces"."slug" = ($1)) GROUP BY "cycles"."id" ORDER BY "cycles"."id" ASC LIMIT 1"#;

/// `estimate_type` check (transfer file `:152-157`, and the identical
/// re-check inside `burndown_plot`, `analytics_plot.py:127-132`):
/// `Project.objects.filter(workspace__slug, pk, estimate__isnull=False,
/// estimate__type="points").exists()` — rendered as Django's
/// `.exists()` shape (`SELECT 1 AS "a" ... LIMIT 1`, verified against
/// live output), which drops the model's default ordering.
///
/// Binds: `$1` slug, `$2` project id.
pub const TRANSFER_ESTIMATE_TYPE_SQL: &str = r#"SELECT 1 AS "a" FROM "projects" INNER JOIN "estimates" ON ("projects"."estimate_id" = "estimates"."id") INNER JOIN "workspaces" ON ("projects"."workspace_id" = "workspaces"."id") WHERE ("projects"."deleted_at" IS NULL AND "projects"."estimate_id" IS NOT NULL AND "estimates"."type" = 'points' AND "projects"."id" = ($2) AND "workspaces"."slug" = ($1)) LIMIT 1"#;

/// Assignee estimate distribution (transfer file `:165-217`): one row
/// per (`display_name`, `assignee_id`, `avatar_url`) group with the
/// three estimate sums ([`ESTIMATE_DIST_SUMS`]). `assignee_id` resolves
/// to the through-table column (`issue_assignees.assignee_id`), while
/// `display_name`/`avatar_url` read `users` (verified against live
/// output). `GROUP BY 1, 2, 3 ORDER BY 1 ASC` is positional in Django's
/// text and stays positional here; the ascending `display_name` order
/// feeds the serialized list order, so it is load-bearing.
///
/// Binds: `$1` slug, `$2` project id, `$3` the source cycle id.
pub const TRANSFER_ASSIGNEE_ESTIMATE_SQL: &str = r#"SELECT "users"."display_name" AS "display_name", "issue_assignees"."assignee_id" AS "assignee_id", CASE WHEN "users"."avatar_asset_id" IS NOT NULL THEN CONCAT(('/api/assets/v2/static/')::text, (CONCAT(("users"."avatar_asset_id")::text, ('/')::text))::text) WHEN "users"."avatar_asset_id" IS NULL THEN "users"."avatar" ELSE NULL END AS "avatar_url", SUM(("estimate_points"."value")::double precision) AS "total_estimates", SUM(("estimate_points"."value")::double precision) FILTER (WHERE ("issues"."archived_at" IS NULL AND "issues"."completed_at" IS NOT NULL AND NOT "issues"."is_draft")) AS "completed_estimates", SUM(("estimate_points"."value")::double precision) FILTER (WHERE ("issues"."archived_at" IS NULL AND "issues"."completed_at" IS NULL AND NOT "issues"."is_draft")) AS "pending_estimates" FROM "issues" LEFT OUTER JOIN "states" ON ("issues"."state_id" = "states"."id") INNER JOIN "projects" ON ("issues"."project_id" = "projects"."id") INNER JOIN "cycle_issues" ON ("issues"."id" = "cycle_issues"."issue_id") INNER JOIN "workspaces" ON ("issues"."workspace_id" = "workspaces"."id") LEFT OUTER JOIN "issue_assignees" ON ("issues"."id" = "issue_assignees"."issue_id") LEFT OUTER JOIN "users" ON ("issue_assignees"."assignee_id" = "users"."id") LEFT OUTER JOIN "estimate_points" ON ("issues"."estimate_point_id" = "estimate_points"."id") WHERE ("issues"."deleted_at" IS NULL AND NOT ("states"."group" = 'triage' AND "states"."group" IS NOT NULL) AND NOT ("issues"."archived_at" IS NOT NULL) AND NOT ("projects"."archived_at" IS NOT NULL) AND NOT ("issues"."is_draft") AND "cycle_issues"."cycle_id" = ($3) AND "cycle_issues"."deleted_at" IS NULL AND "issues"."project_id" = ($2) AND "workspaces"."slug" = ($1)) GROUP BY 1, 2, 3 ORDER BY 1 ASC"#;

/// Label estimate distribution (transfer file `:231-264`): one row per
/// (`label_name`, `color`, `label_id`) group with the three estimate
/// sums ([`ESTIMATE_DIST_SUMS`]). `label_id` resolves to the
/// through-table column (`issue_labels.label_id`), while `label_name` /
/// `color` read `labels`. Ascending `label_name` order feeds the
/// serialized list order.
///
/// Binds: `$1` slug, `$2` project id, `$3` the source cycle id.
pub const TRANSFER_LABEL_ESTIMATE_SQL: &str = r#"SELECT "labels"."name" AS "label_name", "labels"."color" AS "color", "issue_labels"."label_id" AS "label_id", SUM(("estimate_points"."value")::double precision) AS "total_estimates", SUM(("estimate_points"."value")::double precision) FILTER (WHERE ("issues"."archived_at" IS NULL AND "issues"."completed_at" IS NOT NULL AND NOT "issues"."is_draft")) AS "completed_estimates", SUM(("estimate_points"."value")::double precision) FILTER (WHERE ("issues"."archived_at" IS NULL AND "issues"."completed_at" IS NULL AND NOT "issues"."is_draft")) AS "pending_estimates" FROM "issues" LEFT OUTER JOIN "states" ON ("issues"."state_id" = "states"."id") INNER JOIN "projects" ON ("issues"."project_id" = "projects"."id") INNER JOIN "cycle_issues" ON ("issues"."id" = "cycle_issues"."issue_id") INNER JOIN "workspaces" ON ("issues"."workspace_id" = "workspaces"."id") LEFT OUTER JOIN "issue_labels" ON ("issues"."id" = "issue_labels"."issue_id") LEFT OUTER JOIN "labels" ON ("issue_labels"."label_id" = "labels"."id") LEFT OUTER JOIN "estimate_points" ON ("issues"."estimate_point_id" = "estimate_points"."id") WHERE ("issues"."deleted_at" IS NULL AND NOT ("states"."group" = 'triage' AND "states"."group" IS NOT NULL) AND NOT ("issues"."archived_at" IS NOT NULL) AND NOT ("projects"."archived_at" IS NOT NULL) AND NOT ("issues"."is_draft") AND "cycle_issues"."cycle_id" = ($3) AND "cycle_issues"."deleted_at" IS NULL AND "issues"."project_id" = ($2) AND "workspaces"."slug" = ($1)) GROUP BY 1, 2, 3 ORDER BY 1 ASC"#;

/// Assignee issue distribution (transfer file `:287-336`): one row per
/// (`display_name`, `assignee_id`, `avatar_url`) group with the three
/// issue counts ([`ISSUE_DIST_COUNTS`]). Same grouping columns as the
/// estimate twin, no `estimate_points` join. Ascending `display_name`
/// order feeds the serialized list order.
///
/// Binds: `$1` slug, `$2` project id, `$3` the source cycle id.
pub const TRANSFER_ASSIGNEE_ISSUE_SQL: &str = r#"SELECT "users"."display_name" AS "display_name", "issue_assignees"."assignee_id" AS "assignee_id", CASE WHEN "users"."avatar_asset_id" IS NOT NULL THEN CONCAT(('/api/assets/v2/static/')::text, (CONCAT(("users"."avatar_asset_id")::text, ('/')::text))::text) WHEN "users"."avatar_asset_id" IS NULL THEN "users"."avatar" ELSE NULL END AS "avatar_url", COUNT("issues"."id") FILTER (WHERE ("issues"."archived_at" IS NULL AND NOT "issues"."is_draft")) AS "total_issues", COUNT("issues"."id") FILTER (WHERE ("issues"."archived_at" IS NULL AND "issues"."completed_at" IS NOT NULL AND NOT "issues"."is_draft")) AS "completed_issues", COUNT("issues"."id") FILTER (WHERE ("issues"."archived_at" IS NULL AND "issues"."completed_at" IS NULL AND NOT "issues"."is_draft")) AS "pending_issues" FROM "issues" LEFT OUTER JOIN "states" ON ("issues"."state_id" = "states"."id") INNER JOIN "projects" ON ("issues"."project_id" = "projects"."id") INNER JOIN "cycle_issues" ON ("issues"."id" = "cycle_issues"."issue_id") INNER JOIN "workspaces" ON ("issues"."workspace_id" = "workspaces"."id") LEFT OUTER JOIN "issue_assignees" ON ("issues"."id" = "issue_assignees"."issue_id") LEFT OUTER JOIN "users" ON ("issue_assignees"."assignee_id" = "users"."id") WHERE ("issues"."deleted_at" IS NULL AND NOT ("states"."group" = 'triage' AND "states"."group" IS NOT NULL) AND NOT ("issues"."archived_at" IS NOT NULL) AND NOT ("projects"."archived_at" IS NOT NULL) AND NOT ("issues"."is_draft") AND "cycle_issues"."cycle_id" = ($3) AND "cycle_issues"."deleted_at" IS NULL AND "issues"."project_id" = ($2) AND "workspaces"."slug" = ($1)) GROUP BY 1, 2, 3 ORDER BY 1 ASC"#;

/// Label issue distribution (transfer file `:351-384`, verified against
/// live `str(query)` output): one row per (`label_name`, `color`,
/// `label_id`) group with the three issue counts
/// ([`ISSUE_DIST_COUNTS`]). Ascending `label_name` order feeds the
/// serialized list order.
///
/// Binds: `$1` slug, `$2` project id, `$3` the source cycle id.
pub const TRANSFER_LABEL_ISSUE_SQL: &str = r#"SELECT "labels"."name" AS "label_name", "labels"."color" AS "color", "issue_labels"."label_id" AS "label_id", COUNT("issues"."id") FILTER (WHERE ("issues"."archived_at" IS NULL AND NOT "issues"."is_draft")) AS "total_issues", COUNT("issues"."id") FILTER (WHERE ("issues"."archived_at" IS NULL AND "issues"."completed_at" IS NOT NULL AND NOT "issues"."is_draft")) AS "completed_issues", COUNT("issues"."id") FILTER (WHERE ("issues"."archived_at" IS NULL AND "issues"."completed_at" IS NULL AND NOT "issues"."is_draft")) AS "pending_issues" FROM "issues" LEFT OUTER JOIN "states" ON ("issues"."state_id" = "states"."id") INNER JOIN "projects" ON ("issues"."project_id" = "projects"."id") INNER JOIN "cycle_issues" ON ("issues"."id" = "cycle_issues"."issue_id") INNER JOIN "workspaces" ON ("issues"."workspace_id" = "workspaces"."id") LEFT OUTER JOIN "issue_labels" ON ("issues"."id" = "issue_labels"."issue_id") LEFT OUTER JOIN "labels" ON ("issue_labels"."label_id" = "labels"."id") WHERE ("issues"."deleted_at" IS NULL AND NOT ("states"."group" = 'triage' AND "states"."group" IS NOT NULL) AND NOT ("issues"."archived_at" IS NOT NULL) AND NOT ("projects"."archived_at" IS NOT NULL) AND NOT ("issues"."is_draft") AND "cycle_issues"."cycle_id" = ($3) AND "cycle_issues"."deleted_at" IS NULL AND "issues"."project_id" = ($2) AND "workspaces"."slug" = ($1)) GROUP BY 1, 2, 3 ORDER BY 1 ASC"#;

/// Move selection (transfer file `:436-443`): the `CycleIssue` rows
/// whose issues transfer — live bridges in this cycle whose issue is
/// live, not a draft, and in one of [`OPEN_STATE_GROUPS`]. The `states`
/// join is **inner** (ported bug 9). The model's default `-created_at`
/// ordering is load-bearing: the iteration order feeds the
/// `update_cycle_issue_activity` payload (ported bug 11). The
/// per-row `cycle_id` rewrite + `bulk_update` and the `issue_activity`
/// payload assembly are handler/tasks-owned; this statement is the
/// exact row set they consume.
///
/// Binds: `$1` slug, `$2` project id, `$3` the source cycle id.
pub const TRANSFER_MOVE_SELECT_SQL: &str = r#"SELECT "cycle_issues"."id", "cycle_issues"."created_at", "cycle_issues"."updated_at", "cycle_issues"."created_by_id", "cycle_issues"."updated_by_id", "cycle_issues"."deleted_at", "cycle_issues"."project_id", "cycle_issues"."workspace_id", "cycle_issues"."issue_id", "cycle_issues"."cycle_id" FROM "cycle_issues" INNER JOIN "issues" ON ("cycle_issues"."issue_id" = "issues"."id") INNER JOIN "states" ON ("issues"."state_id" = "states"."id") INNER JOIN "workspaces" ON ("cycle_issues"."workspace_id" = "workspaces"."id") WHERE ("cycle_issues"."deleted_at" IS NULL AND "cycle_issues"."cycle_id" = ($3) AND "issues"."archived_at" IS NULL AND NOT "issues"."is_draft" AND "states"."group" IN ('backlog', 'unstarted', 'started', 'review', 'test') AND "cycle_issues"."project_id" = ($2) AND "workspaces"."slug" = ($1)) ORDER BY "cycle_issues"."created_at" DESC"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn q1_list_live_scope_and_joins() {
        // FX-CYCMOD-04 Q1 (`cycle.py:89-167` + `:197`).
        let sql = cycle_list_sql(&OrderBy::default_cycle(), ArchivedFilter::Live);
        assert!(sql.contains("SELECT DISTINCT"), "{sql}");
        assert!(sql.contains(r#"FROM "cycles""#), "{sql}");
        assert!(
            sql.contains(
                r#"INNER JOIN "workspaces" ON "cycles"."workspace_id" = "workspaces"."id""#
            ),
            "{sql}"
        );
        assert!(
            sql.contains(r#"INNER JOIN "projects" ON "cycles"."project_id" = "projects"."id""#),
            "{sql}"
        );
        // Ported bug 2: member-visibility join (unlike module M1).
        assert!(
            sql.contains(
                r#"INNER JOIN "project_members" ON "projects"."id" = "project_members"."project_id""#
            ),
            "{sql}"
        );
        // NOTE: sea-query renders `LEFT JOIN`; Django renders the
        // semantically identical `LEFT OUTER JOIN`.
        assert!(
            sql.contains(
                r#"LEFT JOIN "cycle_issues" ON "cycles"."id" = "cycle_issues"."cycle_id""#
            ),
            "{sql}"
        );
        assert!(
            sql.contains(r#"LEFT JOIN "issues" ON "cycle_issues"."issue_id" = "issues"."id""#),
            "{sql}"
        );
        assert!(
            sql.contains(r#"LEFT JOIN "states" ON "issues"."state_id" = "states"."id""#),
            "{sql}"
        );
        // owned_by is a non-nullable FK: INNER, not LEFT.
        assert!(
            sql.contains(r#"INNER JOIN "users" ON "cycles"."owned_by_id" = "users"."id""#),
            "{sql}"
        );
        // Tenant scope + member scope + live filter + binds.
        assert!(sql.contains(r#""cycles"."deleted_at" IS NULL"#), "{sql}");
        assert!(sql.contains(r#""cycles"."project_id" = ($2)"#), "{sql}");
        assert!(sql.contains(r#""workspaces"."slug" = ($1)"#), "{sql}");
        assert!(
            sql.contains(r#""project_members"."is_active" = TRUE"#),
            "{sql}"
        );
        assert!(
            sql.contains(r#""project_members"."member_id" = ($4)"#),
            "{sql}"
        );
        assert!(sql.contains(r#""cycles"."archived_at" IS NULL"#), "{sql}");
        // Only the base table carries a deleted predicate.
        assert!(!sql.contains(r#""projects"."deleted_at""#), "{sql}");
        assert!(!sql.contains(r#""workspaces"."deleted_at""#), "{sql}");
        assert!(!sql.contains(r#""project_members"."deleted_at""#), "{sql}");
        // No IssueManager guards ride the annotation joins.
        assert!(!sql.contains("triage"), "{sql}");
        assert!(sql.contains(r#"GROUP BY "cycles"."id""#), "{sql}");
        assert!(
            sql.contains(r#"ORDER BY "cycles"."created_at" DESC"#),
            "{sql}"
        );
    }

    #[test]
    fn q1_annotations_count_rows_with_no_distinct() {
        // FX-CYCMOD-04 Q1 (`cycle.py:100-164`): no DISTINCT anywhere
        // (ported bug 1 — the reverse of the module shape).
        let sql = cycle_list_sql(&OrderBy::default_cycle(), ArchivedFilter::Live);
        assert!(
            sql.contains(
                r#"COUNT("cycle_issues"."id") FILTER (WHERE ("cycle_issues"."deleted_at" IS NULL AND "issues"."archived_at" IS NULL AND NOT "issues"."is_draft")) AS "total_issues""#
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
                    r#"COUNT("states"."group") FILTER (WHERE ("cycle_issues"."deleted_at" IS NULL AND "issues"."archived_at" IS NULL AND NOT "issues"."is_draft" AND "states"."group" = '{group}')) AS "{alias}""#
                )),
                "{sql}"
            );
        }
        assert!(!sql.contains("COUNT(DISTINCT"), "{sql}");
        assert_eq!(
            COUNT_ALIASES.len(),
            6,
            "six annotations serve the fixture row keys"
        );
    }

    #[test]
    fn q1_bare_queryset_carries_no_archived_predicate() {
        let sql = cycle_list_sql(&OrderBy::default_cycle(), ArchivedFilter::Any);
        // The projected `"cycles"."archived_at"` column stays (it is a
        // real column); only the WHERE predicate is absent.
        assert!(!sql.contains(r#""cycles"."archived_at" IS"#), "{sql}");
    }

    #[test]
    fn q1_order_passthrough() {
        // kwargs `.order_by(...)` passes through untouched.
        let sql = cycle_list_sql(&OrderBy::new("name", false), ArchivedFilter::Live);
        assert!(sql.contains(r#"ORDER BY "cycles"."name" ASC"#), "{sql}");
    }

    #[test]
    fn q1_cycle_view_predicates() {
        // `cycle.py:198-270`; now rides $5.
        let all = cycle_list_filtered_sql(&OrderBy::default_cycle(), CycleView::All);
        let live = cycle_list_sql(&OrderBy::default_cycle(), ArchivedFilter::Live);
        assert_eq!(all, live, "All renders no extra predicate");
        assert!(!all.contains("($5)"), "{all}");

        let current = cycle_list_filtered_sql(&OrderBy::default_cycle(), CycleView::Current);
        assert!(
            current.contains(r#""cycles"."start_date" <= ($5)"#),
            "{current}"
        );
        assert!(
            current.contains(r#""cycles"."end_date" >= ($5)"#),
            "{current}"
        );
        let upcoming = cycle_list_filtered_sql(&OrderBy::default_cycle(), CycleView::Upcoming);
        assert!(
            upcoming.contains(r#""cycles"."start_date" > ($5)"#),
            "{upcoming}"
        );
        let completed = cycle_list_filtered_sql(&OrderBy::default_cycle(), CycleView::Completed);
        assert!(
            completed.contains(r#""cycles"."end_date" < ($5)"#),
            "{completed}"
        );
        let draft = cycle_list_filtered_sql(&OrderBy::default_cycle(), CycleView::Draft);
        assert!(draft.contains(r#""cycles"."end_date" IS NULL"#), "{draft}");
        assert!(
            draft.contains(r#""cycles"."start_date" IS NULL"#),
            "{draft}"
        );
        assert!(!draft.contains("($5)"), "{draft}");
        let incomplete = cycle_list_filtered_sql(&OrderBy::default_cycle(), CycleView::Incomplete);
        assert!(
            incomplete.contains(r#""cycles"."end_date" >= ($5)"#),
            "{incomplete}"
        );
        assert!(
            incomplete.contains(r#""cycles"."end_date" IS NULL"#),
            "{incomplete}"
        );
    }

    #[test]
    fn q2_detail_adds_live_plus_pk() {
        // FX-CYCMOD-04 Q2 (`cycle.py:370-448` + `:469`).
        let sql = cycle_detail_sql(&OrderBy::default_cycle());
        assert!(sql.contains("SELECT DISTINCT"), "{sql}");
        assert!(sql.contains(r#""cycles"."archived_at" IS NULL"#), "{sql}");
        assert!(sql.contains(r#""cycles"."id" = ($3)"#), "{sql}");
        assert!(
            sql.contains(r#""project_members"."member_id" = ($4)"#),
            "{sql}"
        );
        assert!(sql.contains(r#"AS "total_issues""#), "{sql}");
    }

    #[test]
    fn q3_archived_adds_estimates() {
        // FX-CYCMOD-04 Q3 (`cycle.py:622-724`, estimates `:699-721`).
        let sql = archived_cycle_list_sql(&OrderBy::default_cycle());
        assert!(sql.contains("SELECT DISTINCT"), "{sql}");
        assert!(
            sql.contains(r#""cycles"."archived_at" IS NOT NULL"#),
            "{sql}"
        );
        assert!(!sql.contains(r#""cycles"."archived_at" IS NULL"#), "{sql}");
        assert!(
            sql.contains(
                r#"LEFT JOIN "estimate_points" ON "issues"."estimate_point_id" = "estimate_points"."id""#
            ),
            "{sql}"
        );
        // Total has NO filter (ported bug 7); grouped sums do.
        assert!(
            sql.contains(r#"SUM("estimate_points"."key") AS "total_estimates""#),
            "{sql}"
        );
        for (group, alias) in [
            ("completed", "completed_estimates"),
            ("started", "started_estimates"),
        ] {
            assert!(
                sql.contains(&format!(
                    r#"SUM("estimate_points"."key") FILTER (WHERE ("cycle_issues"."deleted_at" IS NULL AND "issues"."archived_at" IS NULL AND NOT "issues"."is_draft" AND "states"."group" = '{group}')) AS "{alias}""#
                )),
                "{sql}"
            );
        }
        assert!(sql.contains(r#"AS "total_issues""#), "{sql}");
        assert_eq!(ESTIMATE_ALIASES.len(), 3);
        // Q1/Q2 carry no estimate join.
        let q1 = cycle_list_sql(&OrderBy::default_cycle(), ArchivedFilter::Live);
        assert!(!q1.contains("estimate_points"), "{q1}");
    }

    #[test]
    fn q4_queryset_distinct_member_scope_and_subquery() {
        // FX-CYCMOD-04 Q4 queryset (`cycle.py:815-837`; Q5's `:1027-1049`
        // is chain-identical and shares this builder).
        let sql = cycle_issue_queryset_sql(&OrderBy::default_cycle());
        assert!(sql.contains("SELECT DISTINCT"), "{sql}");
        assert!(sql.contains(r#"FROM "cycle_issues""#), "{sql}");
        assert!(
            sql.contains(r#"INNER JOIN "issues" ON "cycle_issues"."issue_id" = "issues"."id""#),
            "{sql}"
        );
        assert!(
            sql.contains(r#"INNER JOIN "cycles" ON "cycle_issues"."cycle_id" = "cycles"."id""#),
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
            sql.contains(r#""cycle_issues"."deleted_at" IS NULL"#),
            "{sql}"
        );
        assert!(
            sql.contains(r#""cycle_issues"."project_id" = ($2)"#),
            "{sql}"
        );
        assert!(sql.contains(r#""cycle_issues"."cycle_id" = ($3)"#), "{sql}");
        assert!(
            sql.contains(r#""project_members"."is_active" = TRUE"#),
            "{sql}"
        );
        assert!(
            sql.contains(r#""project_members"."member_id" = ($4)"#),
            "{sql}"
        );
        // Outer issues join carries no manager guards (ported bug 5): the
        // outer WHERE has no triage / archived-issue / draft guard...
        let outer_where = sql.rsplit("WHERE ").next().unwrap_or("");
        assert!(!outer_where.contains("triage"), "{sql}");
        assert!(!outer_where.contains(r#""issues"."archived_at""#), "{sql}");
        assert!(!outer_where.contains("is_draft"), "{sql}");
        // ...and no projects-archived guard either (ported bug 3 — the
        // asymmetry vs the module querysets).
        assert!(
            !outer_where.contains(r#""projects"."archived_at""#),
            "{sql}"
        );
        // ...but the correlated subquery does (IssueManager inside).
        assert!(
            sql.contains(r#"U0."parent_id" = ("cycle_issues"."issue_id")"#),
            "{sql}"
        );
        assert!(
            sql.contains(r#"NOT (U1."group" = 'triage' AND U1."group" IS NOT NULL)"#),
            "{sql}"
        );
        // No cycle-archived predicate: archived cycles' issues list.
        assert!(!outer_where.contains(r#""cycles"."archived_at""#), "{sql}");
        assert!(
            sql.contains(r#"ORDER BY "cycle_issues"."created_at" DESC"#),
            "{sql}"
        );
    }

    #[test]
    fn q4_get_issue_shape_annotations_and_guards() {
        // FX-CYCMOD-04 Q4 GET (`cycle.py:862-895`).
        let sql = cycle_issue_list_get_sql(&OrderBy::default_issue());
        assert!(!sql.contains("SELECT DISTINCT"), "{sql}");
        assert!(sql.contains(r#"FROM "issues""#), "{sql}");
        assert!(sql.contains(ISSUE_MANAGER_OUTER_GUARDS), "{sql}");
        assert!(
            sql.contains(r#""cycle_issues"."deleted_at" IS NULL"#),
            "{sql}"
        );
        assert!(sql.contains(r#""cycle_issues"."cycle_id" = ($3)"#), "{sql}");
        assert!(sql.contains(r#""issues"."project_id" = ($2)"#), "{sql}");
        assert!(sql.contains(r#""workspaces"."slug" = ($1)"#), "{sql}");
        assert!(
            sql.contains(r#""cycle_issues"."id" AS "bridge_id""#),
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
    fn q5_lookup_scope() {
        // FX-CYCMOD-04 Q5 (`cycle.py:1069-1074` get, `:1092-1097` delete).
        let sql = cycle_issue_detail_lookup_sql();
        assert!(sql.contains(r#"FROM "cycle_issues""#), "{sql}");
        assert!(
            sql.contains(r#""cycle_issues"."deleted_at" IS NULL"#),
            "{sql}"
        );
        assert!(sql.contains(r#""workspaces"."slug" = ($1)"#), "{sql}");
        assert!(
            sql.contains(r#""cycle_issues"."project_id" = ($2)"#),
            "{sql}"
        );
        assert!(sql.contains(r#""cycle_issues"."cycle_id" = ($3)"#), "{sql}");
        assert!(sql.contains(r#""cycle_issues"."issue_id" = ($4)"#), "{sql}");
        assert!(!sql.contains("LIMIT"), "{sql}");
        assert!(!sql.contains("ORDER BY"), "{sql}");
        assert!(!sql.contains("project_members"), "{sql}");
    }

    #[test]
    fn order_defaults_match_django() {
        assert_eq!(OrderBy::default_cycle(), OrderBy::new("created_at", true));
        assert_eq!(OrderBy::default_issue(), OrderBy::new("created_at", false));
    }

    #[test]
    fn table_literals_match_ports() {
        // Literals for tables no db-layer port owns stay in sync with the
        // Django table names; the v1_projects ports are referenced so a
        // rename fails the build here too.
        assert_eq!(WORKSPACE_TABLE, "workspaces");
        assert_eq!(ISSUE_TABLE, "issues");
        assert_eq!(USER_TABLE, "users");
        assert_eq!(ISSUE_LINK_TABLE, "issue_links");
        assert_eq!(FILE_ASSET_TABLE, "file_assets");
        assert_eq!(project::TABLE, "projects");
        assert_eq!(project_member::TABLE, "project_members");
        assert_eq!(state::TABLE, "states");
        assert_eq!(estimate_point::TABLE, "estimate_points");
        assert_eq!(cycle::TABLE, "cycles");
        assert_eq!(cycle_issue::TABLE, "cycle_issues");
    }

    /// Every `COLUMNS` entry appears in the hand-written const, in order.
    fn assert_columns_in_order(sql: &str, table: &str, columns: &[&str]) {
        let mut cursor = 0;
        for col in columns {
            let needle = format!(r#""{table}"."{col}""#);
            let at = sql[cursor..]
                .find(&needle)
                .unwrap_or_else(|| panic!("{needle} missing after offset {cursor} in:\n{sql}"));
            cursor += at + needle.len();
        }
    }

    #[test]
    fn q6_cycle_lookup_is_first_shaped() {
        // Transfer `:59` (+ `:409` re-read): Meta ordering + LIMIT 1, no
        // member join, no archived predicate.
        let sql = TRANSFER_CYCLE_LOOKUP_SQL;
        assert_columns_in_order(sql, cycle::TABLE, cycle::COLUMNS);
        assert!(
            sql.contains(
                r#"INNER JOIN "workspaces" ON ("cycles"."workspace_id" = "workspaces"."id")"#
            ),
            "{sql}"
        );
        assert!(sql.contains(r#""cycles"."deleted_at" IS NULL"#), "{sql}");
        assert!(sql.contains(r#""cycles"."id" = ($3)"#), "{sql}");
        assert!(sql.contains(r#""cycles"."project_id" = ($2)"#), "{sql}");
        assert!(sql.contains(r#""workspaces"."slug" = ($1)"#), "{sql}");
        assert!(!sql.contains("project_members"), "{sql}");
        assert!(!sql.contains("archived_at IS"), "{sql}");
        assert!(
            sql.ends_with(r#"ORDER BY "cycles"."created_at" DESC LIMIT 1"#),
            "{sql}"
        );
    }

    #[test]
    fn q6_old_cycle_recounts_carry_issue_deleted_guard() {
        // Transfer `:69-143`: six recounts cross-pinned against the
        // fragment builders, pk-ordered `.first()`, no member/projects join.
        let sql = TRANSFER_OLD_CYCLE_SQL;
        assert_columns_in_order(sql, cycle::TABLE, cycle::COLUMNS);
        for annotation in all_transfer_counts_sql() {
            assert!(sql.contains(&annotation), "{sql}\nmissing: {annotation}");
        }
        // The extra guard vs Q1 (ported bug 8).
        assert!(sql.contains(r#""issues"."deleted_at" IS NULL"#), "{sql}");
        assert!(!sql.contains("project_members"), "{sql}");
        assert!(!sql.contains(r#"JOIN "projects""#), "{sql}");
        assert!(sql.contains(r#"GROUP BY "cycles"."id""#), "{sql}");
        assert!(
            sql.ends_with(r#"ORDER BY "cycles"."id" ASC LIMIT 1"#),
            "{sql}"
        );
    }

    #[test]
    fn q6_estimate_type_is_exists_shaped() {
        // Transfer `:152-157`: `SELECT 1 AS "a" ... LIMIT 1`.
        let sql = TRANSFER_ESTIMATE_TYPE_SQL;
        assert!(
            sql.starts_with(r#"SELECT 1 AS "a" FROM "projects""#),
            "{sql}"
        );
        assert!(
            sql.contains(
                r#"INNER JOIN "estimates" ON ("projects"."estimate_id" = "estimates"."id")"#
            ),
            "{sql}"
        );
        assert!(sql.contains(r#""projects"."deleted_at" IS NULL"#), "{sql}");
        assert!(
            sql.contains(r#""projects"."estimate_id" IS NOT NULL"#),
            "{sql}"
        );
        assert!(sql.contains(r#""estimates"."type" = 'points'"#), "{sql}");
        assert!(sql.contains(r#""projects"."id" = ($2)"#), "{sql}");
        assert!(sql.contains(r#""workspaces"."slug" = ($1)"#), "{sql}");
        assert!(sql.ends_with("LIMIT 1"), "{sql}");
        assert!(!sql.contains("ORDER BY"), "{sql}");
    }

    #[test]
    fn q6_distributions_share_fragments_and_where() {
        // All four distributions embed the shared fragments verbatim.
        for sql in [
            TRANSFER_ASSIGNEE_ESTIMATE_SQL,
            TRANSFER_LABEL_ESTIMATE_SQL,
            TRANSFER_ASSIGNEE_ISSUE_SQL,
            TRANSFER_LABEL_ISSUE_SQL,
        ] {
            assert!(sql.contains(TRANSFER_DIST_WHERE), "{sql}");
            assert!(sql.contains(ISSUE_MANAGER_OUTER_GUARDS), "{sql}");
            assert!(sql.ends_with("GROUP BY 1, 2, 3 ORDER BY 1 ASC"), "{sql}");
        }
        for sql in [TRANSFER_ASSIGNEE_ESTIMATE_SQL, TRANSFER_ASSIGNEE_ISSUE_SQL] {
            assert!(sql.contains(AVATAR_URL_CASE), "{sql}");
            assert!(
                sql.contains(
                    r#"LEFT OUTER JOIN "issue_assignees" ON ("issues"."id" = "issue_assignees"."issue_id")"#
                ),
                "{sql}"
            );
            assert!(
                sql.contains(
                    r#"LEFT OUTER JOIN "users" ON ("issue_assignees"."assignee_id" = "users"."id")"#
                ),
                "{sql}"
            );
            assert!(
                sql.contains(r#""users"."display_name" AS "display_name""#),
                "{sql}"
            );
            // assignee_id resolves to the through-table column.
            assert!(
                sql.contains(r#""issue_assignees"."assignee_id" AS "assignee_id""#),
                "{sql}"
            );
            assert!(!sql.contains("issue_labels"), "{sql}");
        }
        for sql in [TRANSFER_LABEL_ESTIMATE_SQL, TRANSFER_LABEL_ISSUE_SQL] {
            assert!(
                sql.contains(
                    r#"LEFT OUTER JOIN "issue_labels" ON ("issues"."id" = "issue_labels"."issue_id")"#
                ),
                "{sql}"
            );
            assert!(
                sql.contains(
                    r#"LEFT OUTER JOIN "labels" ON ("issue_labels"."label_id" = "labels"."id")"#
                ),
                "{sql}"
            );
            assert!(sql.contains(r#""labels"."name" AS "label_name""#), "{sql}");
            // label_id resolves to the through-table column.
            assert!(
                sql.contains(r#""issue_labels"."label_id" AS "label_id""#),
                "{sql}"
            );
            assert!(!sql.contains("issue_assignees"), "{sql}");
            assert!(!sql.contains("avatar_url"), "{sql}");
        }
        for sql in [TRANSFER_ASSIGNEE_ESTIMATE_SQL, TRANSFER_LABEL_ESTIMATE_SQL] {
            assert!(sql.contains(ESTIMATE_DIST_SUMS), "{sql}");
            assert!(sql.contains(r#"JOIN "estimate_points""#), "{sql}");
        }
        for sql in [TRANSFER_ASSIGNEE_ISSUE_SQL, TRANSFER_LABEL_ISSUE_SQL] {
            assert!(sql.contains(ISSUE_DIST_COUNTS), "{sql}");
            assert!(!sql.contains("estimate_points"), "{sql}");
        }
    }

    #[test]
    fn q6_move_select_scope_and_order() {
        // Transfer `:436-443`: open states only, inner states join,
        // load-bearing default ordering.
        let sql = TRANSFER_MOVE_SELECT_SQL;
        assert_columns_in_order(sql, cycle_issue::TABLE, cycle_issue::COLUMNS);
        assert!(
            sql.contains(r#"INNER JOIN "issues" ON ("cycle_issues"."issue_id" = "issues"."id")"#),
            "{sql}"
        );
        assert!(
            sql.contains(r#"INNER JOIN "states" ON ("issues"."state_id" = "states"."id")"#),
            "{sql}"
        );
        assert!(
            sql.contains(
                r#""states"."group" IN ('backlog', 'unstarted', 'started', 'review', 'test')"#
            ),
            "{sql}"
        );
        for group in OPEN_STATE_GROUPS {
            assert!(sql.contains(&format!("'{group}'")), "{sql}");
        }
        assert_eq!(OPEN_STATE_GROUPS.len(), 5, "all but completed/cancelled");
        assert!(!sql.contains("'completed'"), "{sql}");
        assert!(!sql.contains("'cancelled'"), "{sql}");
        assert!(sql.contains(r#""cycle_issues"."cycle_id" = ($3)"#), "{sql}");
        assert!(
            sql.ends_with(r#"ORDER BY "cycle_issues"."created_at" DESC"#),
            "{sql}"
        );
        assert!(!sql.contains("LIMIT"), "{sql}");
    }
}
