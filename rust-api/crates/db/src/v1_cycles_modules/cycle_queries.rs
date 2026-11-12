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
//! Each builder returns a complete sea-query [`SelectStatement`] with its
//! values bound (`workspaces.slug`, the project id, the cycle id, the
//! acting user, the `cycle_view` instant), so the handlers run it through
//! SeaORM (`DbBackend::Postgres.build(&stmt)` → `query_all`) and decode
//! the rows by name. No SQL text is written here: the aggregate
//! annotations are `COUNT(CASE WHEN <filter> THEN <col> END)` /
//! `SUM(CASE ...)` (sea-query 0.32 has no aggregate `FILTER` clause; the
//! `CASE` form is the same count — `COUNT` and `SUM` skip the `NULL`
//! the unmatched rows produce), the correlated subqueries are nested
//! selects, and the `avatar_url` case is a `CaseStatement`. The group
//! literals (`'completed'`, …) and the `'ISSUE_ATTACHMENT'` / `'points'`
//! / avatar-URL literals bind as values — the same values Django sends as
//! parameters at execution.
//!
//! The Q6 transfer reads, static text before (`pub const ... _SQL`), are
//! builders now too, sharing the annotation pieces with Q1-Q5; the tests
//! pin each statement's joins, scope, grouping and output names.
//!
//! Spelling notes (all semantics-preserving, each pinned as-rendered by
//! the tests): sea-query renders `LEFT JOIN` where Django renders `LEFT
//! OUTER JOIN`; the bare `"project_members"."is_active"` predicate
//! renders as `= TRUE` (same convention as the sibling module port);
//! Django's incidental `users T9`/`T10` aliases render as the plain
//! `"users"` table; Django's positional `GROUP BY 1, 2, 3 ORDER BY 1` on
//! the transfer distributions renders as the grouped expressions
//! themselves.
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

use chrono::{DateTime, Utc};
use sea_query::{
    Alias, Asterisk, Condition, Expr, Func, IntoIden, JoinType, Order, Query, SelectStatement,
    SimpleExpr, TableRef,
};
use uuid::Uuid;

use super::cycle::{self, cycle_issue};
use crate::v1_projects::models::{estimate, estimate_point, project, project_member, state};

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
/// `labels` table behind bare-M2M `order_by=labels` (Q4 GET,
/// PIDASHCONV-522) and the transfer label distributions.
const LABEL_TABLE: &str = "labels";
/// `issue_assignees` through table behind `order_by=assignees` and the
/// transfer assignee distributions.
const ISSUE_ASSIGNEE_TABLE: &str = "issue_assignees";
/// `issue_labels` through table behind `order_by=labels` and the transfer
/// label distributions.
const ISSUE_LABEL_TABLE: &str = "issue_labels";
/// `pod` table behind bare-FK `order_by=assigned_pod` (Q4 GET,
/// PIDASHCONV-522; `runner.Pod`, `db_table = "pod"`, singular).
const POD_TABLE: &str = "pod";
/// Alias of the parent self-join (`select_related("parent")`, Q4 GET);
/// `order_by=parent__<col>` renders onto it (PIDASHCONV-522).
pub const PARENT_ALIAS: &str = "T7";

/// The tenant scope every cycle read binds: `workspaces.slug` and the
/// project id (`cycle.py:91-92` et al.).
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

/// Where an [`OrderBy`] renders its term (PIDASHCONV-522: the Q4
/// GET `?order_by=` resolves Django-side to more than base-table
/// columns — every shape below was read off live Django 4.2
/// `str(queryset.query)` output for the Q4 GET chain; the module-port
/// precedent is PIDASHCONV-510/511).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderTarget {
    /// `ORDER BY "<base>"."<column>"` — plain columns, FK `<name>_id`
    /// columns, and unknown text (which fails at the database exactly
    /// like Django's `FieldError`-at-evaluation). Every pre-522 shape.
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
    /// `Meta.ordering` terms, with the extra `LEFT JOIN` when the Q4
    /// GET does not already join the table ([`RelatedOrder`]).
    Related(RelatedOrder),
    /// A single-level FK traversal needing a fresh to-one `LEFT JOIN`
    /// ([`TraversalOrder`]): the builder adds the join and orders by the
    /// tail column on the joined table.
    Traversal(TraversalOrder),
    /// A single-level M2M traversal (`assignees__<tail>`,
    /// `labels__<tail>`): the builder adds the through + target `LEFT
    /// JOIN`s and orders by the tail column on the target table.
    M2MTraversal(M2MOrder),
}

/// The bare-M2M names the Q4 GET resolves (`assignees`, `labels`).
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

/// The bare-FK (and reverse-FK) names the Q4 GET resolves to the
/// related model's `Meta.ordering` (PIDASHCONV-522): Django orders
/// `order_by=<fk>` by the related ordering, *not* by `<name>_id` —
/// every shape below was read off live Django 4.2 `str(queryset.query)`
/// output for the Q4 GET chain. The request `-` inverts each term.
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
    /// `ORDER BY "cycle_issues"."created_at" DESC` (bare reverse FK
    /// onto the already-joined bridge table;
    /// `CycleIssue.Meta.ordering`, `db/models/cycle.py:124`).
    IssueCycle,
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
            RelatedOrder::IssueCycle => "issue_cycle",
        }
    }

    /// The extra `LEFT JOIN` the terms need, as `(table,
    /// issues_fk_column)` — `None` when the Q4 GET already joins the
    /// table (`state`/`project`/`workspace`/`parent` via
    /// `select_related`, `issue_cycle` via the filter traversal).
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
            | RelatedOrder::IssueCycle => None,
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
            RelatedOrder::IssueCycle => &[(cycle_issue::TABLE, "created_at", true)],
        }
    }
}

/// The single-level FK-traversal heads needing a join the Q4 GET lacks
/// (PIDASHCONV-522): `created_by`, `updated_by`, `estimate_point`,
/// `assigned_pod`. Each renders one to-one `LEFT JOIN` plus
/// `ORDER BY "<table>"."<tail>"` (the tail passes through raw — bad
/// tails 500 at the database like Django's `FieldError`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraversalOrder {
    /// `LEFT JOIN "users"` on `created_by_id`.
    CreatedBy,
    /// `LEFT JOIN "users"` on `updated_by_id`.
    UpdatedBy,
    /// `LEFT JOIN "estimate_points"` on `estimate_point_id`.
    EstimatePoint,
    /// `LEFT JOIN "pod"` on `assigned_pod_id`.
    AssignedPod,
}

impl TraversalOrder {
    /// The extra `LEFT JOIN` as `(table, issues_fk_column)` (same
    /// joins as the matching [`RelatedOrder::join`]).
    fn join(self) -> (&'static str, &'static str) {
        match self {
            TraversalOrder::CreatedBy => (USER_TABLE, "created_by_id"),
            TraversalOrder::UpdatedBy => (USER_TABLE, "updated_by_id"),
            TraversalOrder::EstimatePoint => (estimate_point::TABLE, "estimate_point_id"),
            TraversalOrder::AssignedPod => (POD_TABLE, "assigned_pod_id"),
        }
    }
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
/// (`services::v1_cycles_modules::cycle_queries`, PIDASHCONV-307) and,
/// for the Q4 GET `?order_by=`, in the handler's
/// `resolve_cycle_issue_order` (PIDASHCONV-362, extended PIDASHCONV-522);
/// [`OrderTarget::Base`] quotes the column onto the base table while the
/// other targets render per [`apply_order`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderBy {
    /// Raw column text (e.g. `created_at`); the raw `?` token for
    /// [`OrderTarget::Random`], the raw M2M name for
    /// [`OrderTarget::M2M`], and the raw tail for traversal targets.
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
    /// model's `Meta.ordering` terms (PIDASHCONV-522).
    pub fn related(which: RelatedOrder, descending: bool) -> Self {
        Self {
            column: which.name().to_owned(),
            descending,
            target: OrderTarget::Related(which),
        }
    }

    /// `OrderBy` for a single-level FK traversal needing a new join.
    pub fn traversal(which: TraversalOrder, tail: impl Into<String>, descending: bool) -> Self {
        Self {
            column: tail.into(),
            descending,
            target: OrderTarget::Traversal(which),
        }
    }

    /// `OrderBy` for a single-level M2M traversal.
    pub fn m2m_traversal(which: M2MOrder, tail: impl Into<String>, descending: bool) -> Self {
        Self {
            column: tail.into(),
            descending,
            target: OrderTarget::M2MTraversal(which),
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

/// Map a single-level `?order_by=` traversal head to its already-joined
/// table or alias in the Q4 GET builder (`None` = not a supported
/// head; the caller tries the join-needing heads next, then passes the
/// name through to 500 like Django's `FieldError`). Deeper paths
/// (`state__project__name`) render their full tail onto the head's
/// table and 500 at the database, exactly like Django.
pub fn issue_traversal_table(head: &str) -> Option<&'static str> {
    match head {
        "state" => Some(state::TABLE),
        "project" => Some(project::TABLE),
        "workspace" => Some(WORKSPACE_TABLE),
        "parent" => Some(PARENT_ALIAS),
        "issue_cycle" => Some(cycle_issue::TABLE),
        _ => None,
    }
}

/// Map a bare-M2M `?order_by=` name (or M2M traversal head) to its
/// ordering (`None` = not an M2M name).
pub fn issue_m2m_order(head: &str) -> Option<M2MOrder> {
    match head {
        "assignees" => Some(M2MOrder::Assignees),
        "labels" => Some(M2MOrder::Labels),
        _ => None,
    }
}

/// Map a single-level `?order_by=` traversal head needing a new to-one
/// join to its ordering (`None` = already-joined head or not a
/// traversal head; see [`issue_traversal_table`]).
pub fn issue_join_traversal(head: &str) -> Option<TraversalOrder> {
    match head {
        "created_by" => Some(TraversalOrder::CreatedBy),
        "updated_by" => Some(TraversalOrder::UpdatedBy),
        "estimate_point" => Some(TraversalOrder::EstimatePoint),
        "assigned_pod" => Some(TraversalOrder::AssignedPod),
        _ => None,
    }
}

/// Append the `ORDER BY` term for `order` onto `sel`, whose base table
/// is `base` — the one rendering helper every builder calls, so exotic
/// targets render identically wherever they appear. Non-`Base` targets
/// only reach the Q4 GET builder (kwargs chains always carry the
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
        // Related `Meta.ordering` terms (PIDASHCONV-522): the request
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
        // FK traversal with a fresh join: the tail renders onto the
        // joined table, request direction as-is.
        OrderTarget::Traversal(which) => {
            let (table, _) = which.join();
            sel.order_by(
                (
                    Alias::new(table.to_owned()),
                    Alias::new(order.column.clone()),
                ),
                order.order(),
            );
        }
        // M2M traversal: the tail renders onto the M2M target table.
        OrderTarget::M2MTraversal(which) => {
            let (_, _, _, target) = which.tables();
            sel.order_by(
                (
                    Alias::new(target.to_owned()),
                    Alias::new(order.column.clone()),
                ),
                order.order(),
            );
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

impl CycleView {
    /// Whether the predicate compares against the `timezone.now()` instant
    /// (the `All` and `Draft` arms take none).
    pub fn binds_now(self) -> bool {
        !matches!(self, CycleView::All | CycleView::Draft)
    }
}

/// The `cycle_view` predicate for the Q1 list `GET`
/// (`cycle.py:202-259`). `now` is the `timezone.now()` instant; the
/// `Draft` arm ignores it. Every comparison is on the `cycles` base
/// table; `NULL` dates compare `NULL` (never match) except in the arms
/// that test `IS NULL` explicitly — exactly like Django.
pub fn cycle_view_condition(view: CycleView, now: DateTime<Utc>) -> Condition {
    let start = || col(cycle::TABLE, "start_date");
    let end = || col(cycle::TABLE, "end_date");
    match view {
        CycleView::All => Condition::all(),
        CycleView::Current => Condition::all().add(start().lte(now)).add(end().gte(now)),
        CycleView::Upcoming => Condition::all().add(start().gt(now)),
        CycleView::Completed => Condition::all().add(end().lt(now)),
        CycleView::Draft => Condition::all().add(end().is_null()).add(start().is_null()),
        CycleView::Incomplete => Condition::any().add(end().gte(now)).add(end().is_null()),
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
/// (every `select_related("workspace")` / `workspace__slug` traversal).
fn join_workspace(sel: &mut SelectStatement, from: &str) {
    sel.join(
        JoinType::InnerJoin,
        table(WORKSPACE_TABLE),
        col(from, "workspace_id").equals((Alias::new(WORKSPACE_TABLE), Alias::new("id"))),
    );
}

/// `INNER JOIN "project_members" ON ("projects"."id" =
/// "project_members"."project_id")` (every
/// `project__project_projectmember` traversal, `cycle.py:93-96` et al.).
fn join_member(sel: &mut SelectStatement) {
    sel.join(
        JoinType::InnerJoin,
        table(project_member::TABLE),
        col(project::TABLE, "id")
            .equals((Alias::new(project_member::TABLE), Alias::new("project_id"))),
    );
}

/// `LEFT JOIN "<table>" ON ("issues"."<fk>" = "<table>"."id")` — one
/// ordering-only to-one join (bare-FK related orderings and FK
/// traversals, PIDASHCONV-522), appended after the parent `T7` join
/// like Django.
fn join_order_fk(sel: &mut SelectStatement, target: &str, fk: &str) {
    sel.join(
        JoinType::LeftJoin,
        table(target),
        col(ISSUE_TABLE, fk).equals((Alias::new(target), Alias::new("id"))),
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

/// Tenant scope shared by the slug/project reads: this workspace slug,
/// this project (`cycle.py:91-92` et al.). Conjunct order follows Django
/// (slug before project). Soft-delete scoping (`deleted_at IS NULL`)
/// comes from each model's manager and is applied per table at the call
/// site — `projects` / `workspaces` carry NO deleted predicate (verified
/// against live Django SQL: only the base table's manager scope renders).
fn tenant_condition(base: &str, scope: &TenantScope<'_>) -> Condition {
    Condition::all()
        .add(col(WORKSPACE_TABLE, "slug").eq(scope.slug))
        .add(col(base, "project_id").eq(scope.project_id))
}

/// Member-visibility conjuncts (`cycle.py:93-96` et al.): the acting user
/// is an active member of the project. Inlined at each call site (flat
/// `AND` chain, like Django) rather than nested: Django renders the
/// `is_active=True` conjunct bare; sea-query renders `= TRUE` —
/// identical semantics, pinned as-rendered.
fn add_member_scope(scope: Condition, actor_id: Uuid) -> Condition {
    scope
        .add(col(project_member::TABLE, "is_active").eq(true))
        .add(col(project_member::TABLE, "member_id").eq(actor_id))
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

/// The `(group, alias)` pairs of the five grouped counts, in source order.
const GROUPED_COUNTS: &[(&str, &str)] = &[
    ("completed", "completed_issues"),
    ("cancelled", "cancelled_issues"),
    ("started", "started_issues"),
    ("unstarted", "unstarted_issues"),
    ("backlog", "backlog_issues"),
];

/// Explicit issue-liveness filter shared by all six counts
/// (`cycle.py:103-107` and each group `Q`): the bridge is live, the
/// issue is not archived and not a draft. There is deliberately NO
/// triage / project-archived / issue soft-delete guard — relation
/// traversals use the base manager (ported bug 5).
fn live_issue_filter() -> Condition {
    Condition::all()
        .add(col(cycle_issue::TABLE, "deleted_at").is_null())
        .add(col(ISSUE_TABLE, "archived_at").is_null())
        .add(col(ISSUE_TABLE, "is_draft").not())
}

/// `COUNT(CASE WHEN <filter> THEN <counted> END)`: Django's
/// `Count(..., filter=Q(...))` (`COUNT(x) FILTER (WHERE ...)`) — the same
/// row count, since `COUNT` skips the `NULL` the unmatched rows yield.
fn count_where(filter: Condition, counted: Expr) -> SimpleExpr {
    Func::count(Expr::case(filter, counted)).into()
}

/// `SUM(CASE WHEN <filter> THEN <summed> END)`: Django's
/// `Sum(..., filter=Q(...))`; `NULL` when no row matches, like `FILTER`.
fn sum_where(filter: Condition, summed: SimpleExpr) -> SimpleExpr {
    Func::sum(Expr::case(filter, summed)).into()
}

/// One `Count(...)` annotation (`cycle.py:100-164`) — with **no
/// `DISTINCT`** (ported bug 1, the reverse of the module shape):
///
/// * total (`GroupFilter::All`): counts `"cycle_issues"."id"` over the
///   live filter — a real row count.
/// * grouped: counts `"states"."group"` over the live filter plus
///   `"states"."group" = '<group>'` — a real row count (`COUNT` skips
///   `NULL`, and every row passing the single-group filter is non-null).
pub fn count_issues_expr(filter: GroupFilter) -> SimpleExpr {
    match filter {
        GroupFilter::All => count_where(live_issue_filter(), col(cycle_issue::TABLE, "id")),
        GroupFilter::Eq(group) => count_where(
            live_issue_filter().add(col(state::TABLE, "group").eq(group)),
            col(state::TABLE, "group"),
        ),
    }
}

/// All six count annotations in source order, onto the Q1/Q2/Q3 select
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
fn join_cycle_reads(sel: &mut SelectStatement, with_estimates: bool) {
    join_workspace(sel, cycle::TABLE);
    join_project(sel, cycle::TABLE);
    join_member(sel);
    left_join(sel, cycle::TABLE, "id", cycle_issue::TABLE, "cycle_id");
    left_join(sel, cycle_issue::TABLE, "issue_id", ISSUE_TABLE, "id");
    left_join(sel, ISSUE_TABLE, "state_id", state::TABLE, "id");
    if with_estimates {
        left_join(
            sel,
            ISSUE_TABLE,
            "estimate_point_id",
            estimate_point::TABLE,
            "id",
        );
    }
    // Django aliases the owner join `T9`/`T10`; the alias is incidental
    // join-order numbering with identical semantics, so the plain table
    // renders.
    inner_join(sel, cycle::TABLE, "owned_by_id", USER_TABLE, "id");
}

/// Shared Q1/Q2/Q3 `WHERE` head: the base-table manager scope
/// (`cycles.deleted_at IS NULL`) plus the tenant scope plus the
/// member-visibility scope. The `projects` / `workspaces` /
/// `project_members` joins carry no deleted predicate (verified against
/// live Django SQL).
fn cycle_list_where(scope: &TenantScope<'_>, actor_id: Uuid) -> Condition {
    let head = Condition::all()
        .add(col(cycle::TABLE, "deleted_at").is_null())
        .add(tenant_condition(cycle::TABLE, scope));
    add_member_scope(head, actor_id)
}

/// The Q1/Q2/Q3 select head: `SELECT DISTINCT` base columns in
/// [`cycle::COLUMNS`] order plus the six counts, `FROM "cycles"` with
/// the shared joins.
fn cycle_read_head(with_estimates: bool) -> SelectStatement {
    let mut sel = Query::select();
    sel.distinct();
    select_table_columns(&mut sel, cycle::TABLE, cycle::COLUMNS);
    add_count_annotations(&mut sel);
    if with_estimates {
        add_estimate_sums(&mut sel);
    }
    sel.from(table(cycle::TABLE));
    join_cycle_reads(&mut sel, with_estimates);
    sel
}

/// Q1 cycle list (`CycleListCreateAPIEndpoint.get_queryset`,
/// `cycle.py:89-167`, + the list `GET` archived predicate at `:197`):
/// `SELECT DISTINCT` base columns in [`cycle::COLUMNS`] order, the six
/// count annotations, member-visibility scope, `GROUP BY "cycles"."id"`,
/// kwargs order.
///
/// Django groups by all four joined PKs; the tenant/member/owner joins
/// are N:1 off `cycles.id`, so grouping by `cycles.id` alone yields
/// identical groups (ported bug 12 — same documented narrowing as the
/// sibling module port).
pub fn cycle_list(
    order: &OrderBy,
    archived: ArchivedFilter,
    scope: &TenantScope<'_>,
    actor_id: Uuid,
) -> SelectStatement {
    cycle_list_inner(order, archived, CycleView::All, None, scope, actor_id)
}

/// Q1 list `GET` with a `cycle_view` date filter (`cycle.py:198-270`):
/// the live Q1 list plus [`cycle_view_condition`] against `now`, the
/// `timezone.now()` instant. `CycleView::All` renders no extra predicate
/// (identical to [`cycle_list`] with [`ArchivedFilter::Live`]).
pub fn cycle_list_filtered(
    order: &OrderBy,
    view: CycleView,
    now: DateTime<Utc>,
    scope: &TenantScope<'_>,
    actor_id: Uuid,
) -> SelectStatement {
    cycle_list_inner(
        order,
        ArchivedFilter::Live,
        view,
        Some(now),
        scope,
        actor_id,
    )
}

fn cycle_list_inner(
    order: &OrderBy,
    archived: ArchivedFilter,
    view: CycleView,
    now: Option<DateTime<Utc>>,
    scope: &TenantScope<'_>,
    actor_id: Uuid,
) -> SelectStatement {
    let mut sel = cycle_read_head(false);
    let mut where_ = cycle_list_where(scope, actor_id);
    match archived {
        ArchivedFilter::Live => {
            where_ = where_.add(col(cycle::TABLE, "archived_at").is_null());
        }
        ArchivedFilter::Archived => {
            where_ = where_.add(col(cycle::TABLE, "archived_at").is_not_null());
        }
        ArchivedFilter::Any => {}
    }
    if view != CycleView::All {
        where_ = where_.add(cycle_view_condition(view, now.unwrap_or_else(Utc::now)));
    }
    sel.cond_where(where_);
    sel.group_by_col((Alias::new(cycle::TABLE), Alias::new("id")));
    apply_order(&mut sel, cycle::TABLE, order);
    sel
}

/// Q2 cycle detail (`CycleDetailAPIEndpoint.get_queryset`,
/// `cycle.py:370-448`, + `.filter(archived_at__isnull=True).get(pk=pk)`
/// from the detail `GET` at `:469`): identical to the live Q1 plus the
/// pk predicate. The chain is identical to Q1's; only the GET wrapper
/// differs.
pub fn cycle_detail(
    order: &OrderBy,
    scope: &TenantScope<'_>,
    cycle_id: Uuid,
    actor_id: Uuid,
) -> SelectStatement {
    let mut sel = cycle_read_head(false);
    sel.cond_where(
        cycle_list_where(scope, actor_id)
            .add(col(cycle::TABLE, "archived_at").is_null())
            .add(col(cycle::TABLE, "id").eq(cycle_id)),
    );
    sel.group_by_col((Alias::new(cycle::TABLE), Alias::new("id")));
    apply_order(&mut sel, cycle::TABLE, order);
    sel
}

/// The three Q3 estimate-sum aliases in source order (`cycle.py:699-721`).
pub const ESTIMATE_ALIASES: &[&str] = &[
    "total_estimates",
    "completed_estimates",
    "started_estimates",
];

/// One Q3 `Sum("issue_cycle__issue__estimate_point__key")` annotation
/// (`cycle.py:699-721`). The total (`GroupFilter::All`) carries **no
/// filter** (ported bug 7); the grouped sums carry the same liveness
/// guard plus the group equality. `key` is the `IntegerField`, so the
/// sums are integer sums (unlike the transfer's `value`-cast doubles).
pub fn estimate_sum_expr(filter: GroupFilter) -> SimpleExpr {
    let key = col(estimate_point::TABLE, "key");
    match filter {
        GroupFilter::All => Func::sum(key).into(),
        GroupFilter::Eq(group) => sum_where(
            live_issue_filter().add(col(state::TABLE, "group").eq(group)),
            key.into(),
        ),
    }
}

/// All three estimate sums in source order, onto the Q3 select list.
fn add_estimate_sums(sel: &mut SelectStatement) {
    sel.expr_as(
        estimate_sum_expr(GroupFilter::All),
        Alias::new("total_estimates"),
    );
    sel.expr_as(
        estimate_sum_expr(GroupFilter::Eq("completed")),
        Alias::new("completed_estimates"),
    );
    sel.expr_as(
        estimate_sum_expr(GroupFilter::Eq("started")),
        Alias::new("started_estimates"),
    );
}

/// Q3 archived-cycle list (`CycleArchiveUnarchiveAPIEndpoint.get_queryset`,
/// `cycle.py:622-724`): same chain as Q1 with
/// `archived_at__isnull=False` (`:630`) **plus** the three estimate sums
/// (`:699-721`) and the `estimate_points` left join — the asymmetry vs
/// the archived-module list, which has no estimates.
pub fn archived_cycle_list(
    order: &OrderBy,
    scope: &TenantScope<'_>,
    actor_id: Uuid,
) -> SelectStatement {
    let mut sel = cycle_read_head(true);
    sel.cond_where(
        cycle_list_where(scope, actor_id).add(col(cycle::TABLE, "archived_at").is_not_null()),
    );
    sel.group_by_col((Alias::new(cycle::TABLE), Alias::new("id")));
    apply_order(&mut sel, cycle::TABLE, order);
    sel
}

// ---------------------------------------------------------------------------
// Q4/Q5 issue querysets (CycleIssue-based)
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

/// Outer `IssueManager` guards for the Q4 GET shape and the transfer
/// distributions (`Issue.issue_objects` entry points — never relation
/// traversals, ported bug 5), on the plain `issues` / `states` /
/// `projects` tables.
pub fn issue_manager_outer_guards() -> Condition {
    issue_manager_guards(ISSUE_TABLE, state::TABLE, project::TABLE)
}

/// `sub_issues_count` correlated subquery (`cycle.py:817-822,864-869,
/// 1029-1034`): `Issue.issue_objects.filter(parent=OuterRef(...))` — so
/// the FULL `IssueManager` guards render inside (triage / archived issue
/// / archived project / draft excluded, soft-delete excluded), with the
/// `states` join nullable (`LEFT OUTER JOIN`, hence the NULL-safe triage
/// guard). `Count` is NOT distinct. The subquery restricts `parent_id`
/// ONLY — no project or cycle scope (ported bug 6).
///
/// `outer` is the correlated parent column: `"cycle_issues"."issue_id"`
/// for the Q4/Q5 querysets, `"issues"."id"` for the Q4 GET shape.
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

/// Q4/Q5 queryset (`CycleIssueListCreateAPIEndpoint.get_queryset`,
/// `cycle.py:815-837`; Q5's `cycle.py:1027-1049` is chain-identical):
/// `SELECT DISTINCT` bridge columns in [`cycle_issue::COLUMNS`] order +
/// [`sub_issues_count_expr`] correlated to the bridge's issue.
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
pub fn cycle_issue_queryset(
    order: &OrderBy,
    scope: &TenantScope<'_>,
    cycle_id: Uuid,
    actor_id: Uuid,
) -> SelectStatement {
    let mut sel = Query::select();
    sel.distinct();
    select_table_columns(&mut sel, cycle_issue::TABLE, cycle_issue::COLUMNS);
    sel.expr_as(
        sub_issues_count_expr(col(cycle_issue::TABLE, "issue_id")),
        Alias::new("sub_issues_count"),
    );
    sel.from(table(cycle_issue::TABLE));
    inner_join(&mut sel, cycle_issue::TABLE, "issue_id", ISSUE_TABLE, "id");
    join_workspace(&mut sel, cycle_issue::TABLE);
    join_project(&mut sel, cycle_issue::TABLE);
    join_member(&mut sel);
    inner_join(&mut sel, cycle_issue::TABLE, "cycle_id", cycle::TABLE, "id");
    // INNER JOIN "projects" AS "T8" ON ("issues"."project_id" = T8."id")
    // (select_related("issue__project")).
    sel.join(
        JoinType::InnerJoin,
        TableRef::Table(table(project::TABLE).into_iden()).alias(Alias::new("T8")),
        col(ISSUE_TABLE, "project_id").equals((Alias::new("T8"), Alias::new("id"))),
    );
    left_join(&mut sel, ISSUE_TABLE, "state_id", state::TABLE, "id");
    let where_ = Condition::all()
        .add(col(cycle_issue::TABLE, "deleted_at").is_null())
        .add(tenant_condition(cycle_issue::TABLE, scope));
    let where_ =
        add_member_scope(where_, actor_id).add(col(cycle_issue::TABLE, "cycle_id").eq(cycle_id));
    sel.cond_where(where_);
    apply_order(&mut sel, cycle_issue::TABLE, order);
    sel
}

/// `link_count` scalar annotation (Q4 GET, `cycle.py:880-885`):
/// `IssueLink.objects` uses the soft-delete default manager, so
/// `deleted_at IS NULL` renders.
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

/// `attachment_count` scalar annotation (Q4 GET, `cycle.py:886-894`):
/// same soft-delete manager scope plus the
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

/// `bridge_id` annotation (Q4 GET, `cycle.py:870`):
/// `.annotate(bridge_id=F("issue_cycle__id"))` — the live bridge's pk
/// via the `INNER JOIN "cycle_issues"`. One row per live bridge: an
/// issue bridged twice (deleted-then-recreated rows excepted by the
/// `deleted_at` guard) fans out into two rows — ported as observed.
pub fn bridge_id_expr() -> SimpleExpr {
    col(cycle_issue::TABLE, "id").into()
}

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
pub fn cycle_issue_list_get(
    order: &OrderBy,
    scope: &TenantScope<'_>,
    cycle_id: Uuid,
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
    inner_join(&mut sel, ISSUE_TABLE, "id", cycle_issue::TABLE, "issue_id");
    join_workspace(&mut sel, ISSUE_TABLE);
    // LEFT OUTER JOIN "issues" AS "T7" ON ("issues"."parent_id" = T7."id")
    // (select_related("parent")).
    sel.join(
        JoinType::LeftJoin,
        TableRef::Table(table(ISSUE_TABLE).into_iden()).alias(Alias::new(PARENT_ALIAS)),
        col(ISSUE_TABLE, "parent_id").equals((Alias::new(PARENT_ALIAS), Alias::new("id"))),
    );
    // Bare-M2M ordering joins (PIDASHCONV-522), after T7 like Django:
    // `LEFT JOIN "<through>"` + `LEFT JOIN "<target>"`, no DISTINCT —
    // rows multiply per through-row exactly like the Django queryset.
    // M2M traversals (`assignees__<tail>`) share the same two joins.
    if let OrderTarget::M2M(which) | OrderTarget::M2MTraversal(which) = order.target {
        let (through, issue_fk, target_fk, target) = which.tables();
        left_join(&mut sel, ISSUE_TABLE, "id", through, issue_fk);
        left_join(&mut sel, through, target_fk, target, "id");
    }
    // Bare-FK related-ordering joins (PIDASHCONV-522), after T7 like
    // Django: one `LEFT JOIN "<table>" ON ("issues"."<fk>" =
    // "<table>"."id")`. To-one, so rows never multiply (unlike M2M).
    if let OrderTarget::Related(which) = order.target {
        if let Some((target, fk)) = which.join() {
            join_order_fk(&mut sel, target, fk);
        }
    }
    // FK-traversal ordering joins (PIDASHCONV-522): same to-one shape
    // as the bare-FK joins, for `created_by__<tail>` et al.
    if let OrderTarget::Traversal(which) = order.target {
        let (target, fk) = which.join();
        join_order_fk(&mut sel, target, fk);
    }
    sel.cond_where(
        issue_manager_outer_guards()
            .add(col(cycle_issue::TABLE, "cycle_id").eq(cycle_id))
            .add(col(cycle_issue::TABLE, "deleted_at").is_null())
            .add(col(ISSUE_TABLE, "project_id").eq(scope.project_id))
            .add(col(WORKSPACE_TABLE, "slug").eq(scope.slug)),
    );
    apply_order(&mut sel, ISSUE_TABLE, order);
    sel
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
pub fn cycle_issue_detail_lookup(
    scope: &TenantScope<'_>,
    cycle_id: Uuid,
    issue_id: Uuid,
) -> SelectStatement {
    let mut sel = Query::select();
    select_table_columns(&mut sel, cycle_issue::TABLE, cycle_issue::COLUMNS);
    sel.from(table(cycle_issue::TABLE));
    join_workspace(&mut sel, cycle_issue::TABLE);
    sel.cond_where(
        Condition::all()
            .add(col(cycle_issue::TABLE, "deleted_at").is_null())
            .add(col(cycle_issue::TABLE, "cycle_id").eq(cycle_id))
            .add(col(cycle_issue::TABLE, "issue_id").eq(issue_id))
            .add(col(cycle_issue::TABLE, "project_id").eq(scope.project_id))
            .add(col(WORKSPACE_TABLE, "slug").eq(scope.slug)),
    );
    sel
}

// ---------------------------------------------------------------------------
// Q6 transfer reads
// ---------------------------------------------------------------------------

/// Explicit issue-liveness filter of the transfer old-cycle recounts
/// (transfer file `:74-79` and each group `Q`): the Q1 filter plus the
/// issue soft-delete guard (ported bug 8). Condition order follows
/// Django, not the `Q()` kwarg order.
fn transfer_live_issue_filter() -> Condition {
    Condition::all()
        .add(col(cycle_issue::TABLE, "deleted_at").is_null())
        .add(col(ISSUE_TABLE, "archived_at").is_null())
        .add(col(ISSUE_TABLE, "deleted_at").is_null())
        .add(col(ISSUE_TABLE, "is_draft").not())
}

/// One old-cycle recount annotation (transfer file `:71-141`): the
/// [`count_issues_expr`] shape with the extra issue soft-delete guard.
/// No `DISTINCT`, like Q1.
pub fn transfer_count_issues_expr(filter: GroupFilter) -> SimpleExpr {
    match filter {
        GroupFilter::All => {
            count_where(transfer_live_issue_filter(), col(cycle_issue::TABLE, "id"))
        }
        GroupFilter::Eq(group) => count_where(
            transfer_live_issue_filter().add(col(state::TABLE, "group").eq(group)),
            col(state::TABLE, "group"),
        ),
    }
}

/// The transfer's open-state gate (`utils/constants.py:86`):
/// `STATE_GROUP_ORDER[:-2]` — every group except `completed` and
/// `cancelled`. Only issues in these states move
/// (`issue__state__group__in`, transfer file `:442`).
pub const OPEN_STATE_GROUPS: &[&str] = &["backlog", "unstarted", "started", "review", "test"];

/// `avatar_url` select item shared by the assignee distributions
/// (transfer file `:175-193,:296-312`): the asset URL when
/// `avatar_asset` is set, the raw `avatar` text when it is null, `NULL`
/// otherwise (unreachable — the two `WHEN` arms partition
/// null/non-null — ported as observed as the implicit `ELSE NULL`).
/// Django renders the nested `Concat` as nested `CONCAT(...::text)`
/// casts; the `Value` literals bind as text here.
pub fn avatar_url_expr() -> SimpleExpr {
    let asset = || col(USER_TABLE, "avatar_asset_id");
    let text = |value: &str| Expr::val(value).cast_as(Alias::new("text"));
    let url = Func::cust(Alias::new("CONCAT")).args([
        text("/api/assets/v2/static/"),
        asset().cast_as(Alias::new("text")),
        text("/"),
    ]);
    Expr::case(asset().is_not_null(), url)
        .case(asset().is_null(), col(USER_TABLE, "avatar"))
        .into()
}

/// `"estimate_points"."value"` cast to double (transfer estimate math,
/// `Cast("estimate_point__value", FloatField())`, ported bug 10): a
/// non-numeric value fails at the database.
fn estimate_value_double() -> SimpleExpr {
    col(estimate_point::TABLE, "value").cast_as(Alias::new("double precision"))
}

/// The transfer distributions' completed / pending issue predicates
/// (transfer file `:200-215,:320-334`).
fn dist_alive() -> Condition {
    Condition::all()
        .add(col(ISSUE_TABLE, "archived_at").is_null())
        .add(col(ISSUE_TABLE, "is_draft").not())
}

fn dist_completed() -> Condition {
    Condition::all()
        .add(col(ISSUE_TABLE, "archived_at").is_null())
        .add(col(ISSUE_TABLE, "completed_at").is_not_null())
        .add(col(ISSUE_TABLE, "is_draft").not())
}

fn dist_pending() -> Condition {
    Condition::all()
        .add(col(ISSUE_TABLE, "archived_at").is_null())
        .add(col(ISSUE_TABLE, "completed_at").is_null())
        .add(col(ISSUE_TABLE, "is_draft").not())
}

/// The three estimate-distribution sums in source order
/// (`total_estimates`, `completed_estimates`, `pending_estimates`,
/// transfer file `:195-215,:242-262`): `SUM` over the `value` CharField
/// cast to double (ported bug 10), unfiltered total plus the
/// completed/pending filters. `NULL` when a group has no valued rows.
fn add_estimate_dist_sums(sel: &mut SelectStatement) {
    sel.expr_as(
        SimpleExpr::from(Func::sum(estimate_value_double())),
        Alias::new("total_estimates"),
    );
    sel.expr_as(
        sum_where(dist_completed(), estimate_value_double()),
        Alias::new("completed_estimates"),
    );
    sel.expr_as(
        sum_where(dist_pending(), estimate_value_double()),
        Alias::new("pending_estimates"),
    );
}

/// The three issue-distribution counts in source order
/// (`total_issues`, `completed_issues`, `pending_issues`, transfer file
/// `:314-334,:362-382`): real row counts, unfiltered-alive total plus
/// the completed/pending filters.
fn add_issue_dist_counts(sel: &mut SelectStatement) {
    sel.expr_as(
        count_where(dist_alive(), col(ISSUE_TABLE, "id")),
        Alias::new("total_issues"),
    );
    sel.expr_as(
        count_where(dist_completed(), col(ISSUE_TABLE, "id")),
        Alias::new("completed_issues"),
    );
    sel.expr_as(
        count_where(dist_pending(), col(ISSUE_TABLE, "id")),
        Alias::new("pending_issues"),
    );
}

/// `WHERE` tail shared by the four transfer distributions (transfer file
/// `:166-171` et al.): the [`issue_manager_outer_guards`] head (the
/// entry point is `Issue.issue_objects`), then the cycle-bridge scope,
/// then the tenant scope. Condition order follows Django.
fn transfer_dist_where(scope: &TenantScope<'_>, cycle_id: Uuid) -> Condition {
    issue_manager_outer_guards()
        .add(col(cycle_issue::TABLE, "cycle_id").eq(cycle_id))
        .add(col(cycle_issue::TABLE, "deleted_at").is_null())
        .add(col(ISSUE_TABLE, "project_id").eq(scope.project_id))
        .add(col(WORKSPACE_TABLE, "slug").eq(scope.slug))
}

/// Which dimension a transfer distribution groups by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Distribution {
    /// `issue_assignees` → `users`: `display_name`, `assignee_id`,
    /// `avatar_url` (transfer file `:165-217,:287-336`).
    Assignee,
    /// `issue_labels` → `labels`: `label_name`, `color`, `label_id`
    /// (transfer file `:231-264,:351-384`).
    Label,
}

/// What a transfer distribution measures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Measure {
    /// The three estimate sums (the `estimate_type` branch).
    Estimates,
    /// The three issue counts (always loaded).
    Issues,
}

/// New-cycle guard lookup (transfer file `:59`) — and the identical
/// `current_cycle` re-read (`:409`): `Cycle.objects.filter(
/// workspace__slug, project_id, pk).first()`. The queryset keeps the
/// model's default `-created_at` ordering (Django applies
/// `Meta.ordering` to ungrouped selects); `.first()` adds `LIMIT 1`. No
/// member join, no archived predicate — the guard reads any cycle in
/// the tenant, live or archived.
pub fn transfer_cycle_lookup(scope: &TenantScope<'_>, cycle_id: Uuid) -> SelectStatement {
    let mut sel = Query::select();
    select_table_columns(&mut sel, cycle::TABLE, cycle::COLUMNS);
    sel.from(table(cycle::TABLE));
    join_workspace(&mut sel, cycle::TABLE);
    sel.cond_where(
        Condition::all()
            .add(col(cycle::TABLE, "deleted_at").is_null())
            .add(col(cycle::TABLE, "id").eq(cycle_id))
            .add(tenant_condition(cycle::TABLE, scope)),
    );
    sel.order_by(
        (Alias::new(cycle::TABLE), Alias::new("created_at")),
        Order::Desc,
    );
    sel.limit(1);
    sel
}

/// Old-cycle recounts (transfer file `:69-143`): the source cycle row
/// plus the six counts with the extra issue soft-delete guard
/// ([`transfer_count_issues_expr`]). No member join, no `projects` join
/// (the chain filters `project_id` directly on `cycles`), no archived
/// predicate. `.first()` at `:143` adds `ORDER BY "cycles"."id" ASC
/// LIMIT 1` (Django orders an unordered grouped queryset by pk).
pub fn transfer_old_cycle(scope: &TenantScope<'_>, cycle_id: Uuid) -> SelectStatement {
    let mut sel = Query::select();
    select_table_columns(&mut sel, cycle::TABLE, cycle::COLUMNS);
    sel.expr_as(
        transfer_count_issues_expr(GroupFilter::All),
        Alias::new("total_issues"),
    );
    for (group, alias) in GROUPED_COUNTS {
        sel.expr_as(
            transfer_count_issues_expr(GroupFilter::Eq(group)),
            Alias::new(*alias),
        );
    }
    sel.from(table(cycle::TABLE));
    join_workspace(&mut sel, cycle::TABLE);
    left_join(&mut sel, cycle::TABLE, "id", cycle_issue::TABLE, "cycle_id");
    left_join(&mut sel, cycle_issue::TABLE, "issue_id", ISSUE_TABLE, "id");
    left_join(&mut sel, ISSUE_TABLE, "state_id", state::TABLE, "id");
    sel.cond_where(
        Condition::all()
            .add(col(cycle::TABLE, "deleted_at").is_null())
            .add(col(cycle::TABLE, "id").eq(cycle_id))
            .add(tenant_condition(cycle::TABLE, scope)),
    );
    sel.group_by_col((Alias::new(cycle::TABLE), Alias::new("id")));
    sel.order_by((Alias::new(cycle::TABLE), Alias::new("id")), Order::Asc);
    sel.limit(1);
    sel
}

/// `estimate_type` check (transfer file `:152-157`, and the identical
/// re-check inside `burndown_plot`, `analytics_plot.py:127-132`):
/// `Project.objects.filter(workspace__slug, pk, estimate__isnull=False,
/// estimate__type="points").exists()` — Django's `.exists()` shape
/// (one column, `LIMIT 1`, no ordering); the caller tests for a row.
pub fn transfer_estimate_type(scope: &TenantScope<'_>) -> SelectStatement {
    let mut sel = Query::select();
    sel.column((Alias::new(project::TABLE), Alias::new("id")));
    sel.from(table(project::TABLE));
    inner_join(
        &mut sel,
        project::TABLE,
        "estimate_id",
        estimate::TABLE,
        "id",
    );
    join_workspace(&mut sel, project::TABLE);
    sel.cond_where(
        Condition::all()
            .add(col(project::TABLE, "deleted_at").is_null())
            .add(col(project::TABLE, "estimate_id").is_not_null())
            .add(col(estimate::TABLE, "type").eq("points"))
            .add(col(project::TABLE, "id").eq(scope.project_id))
            .add(col(WORKSPACE_TABLE, "slug").eq(scope.slug)),
    );
    sel.limit(1);
    sel
}

/// One transfer distribution (transfer file `:165-399`): one row per
/// group with the three measures. `assignee_id` / `label_id` resolve to
/// the through-table column (`issue_assignees.assignee_id`,
/// `issue_labels.label_id`), while `display_name` / `avatar_url` read
/// `users` and `label_name` / `color` read `labels`. Django's
/// positional `GROUP BY 1, 2, 3 ORDER BY 1 ASC` groups by the three
/// dimension items (the assignee `avatar_url` case by its two `users`
/// source columns, same groups) and orders by the first (`display_name`
/// / `label_name`) ascending — the order feeds the serialized list, so
/// it is load-bearing.
///
/// Joins (verified order): `states` left outer, `projects` inner,
/// `cycle_issues` inner, `workspaces` inner, the through table left
/// outer, its target left outer, then `estimate_points` left outer for
/// [`Measure::Estimates`].
pub fn transfer_distribution(
    scope: &TenantScope<'_>,
    cycle_id: Uuid,
    dimension: Distribution,
    measure: Measure,
) -> SelectStatement {
    let mut sel = Query::select();
    // The select items, and the columns the groups are made of. Django
    // groups positionally (`GROUP BY 1, 2, 3`); a bound `CASE` cannot be
    // matched by text (its literals are separate parameters), so the
    // avatar group is its two source columns — `users` is N:1 off
    // `assignee_id`, so the groups are identical.
    let (items, group_cols): (Vec<(SimpleExpr, &str)>, Vec<Expr>) = match dimension {
        Distribution::Assignee => (
            vec![
                (col(USER_TABLE, "display_name").into(), "display_name"),
                (
                    col(ISSUE_ASSIGNEE_TABLE, "assignee_id").into(),
                    "assignee_id",
                ),
                (avatar_url_expr(), "avatar_url"),
            ],
            vec![
                col(USER_TABLE, "display_name"),
                col(ISSUE_ASSIGNEE_TABLE, "assignee_id"),
                col(USER_TABLE, "avatar_asset_id"),
                col(USER_TABLE, "avatar"),
            ],
        ),
        Distribution::Label => (
            vec![
                (col(LABEL_TABLE, "name").into(), "label_name"),
                (col(LABEL_TABLE, "color").into(), "color"),
                (col(ISSUE_LABEL_TABLE, "label_id").into(), "label_id"),
            ],
            vec![
                col(LABEL_TABLE, "name"),
                col(LABEL_TABLE, "color"),
                col(ISSUE_LABEL_TABLE, "label_id"),
            ],
        ),
    };
    for (expr, alias) in &items {
        sel.expr_as(expr.clone(), Alias::new(*alias));
    }
    match measure {
        Measure::Estimates => add_estimate_dist_sums(&mut sel),
        Measure::Issues => add_issue_dist_counts(&mut sel),
    }
    sel.from(table(ISSUE_TABLE));
    left_join(&mut sel, ISSUE_TABLE, "state_id", state::TABLE, "id");
    join_project(&mut sel, ISSUE_TABLE);
    inner_join(&mut sel, ISSUE_TABLE, "id", cycle_issue::TABLE, "issue_id");
    join_workspace(&mut sel, ISSUE_TABLE);
    match dimension {
        Distribution::Assignee => {
            left_join(
                &mut sel,
                ISSUE_TABLE,
                "id",
                ISSUE_ASSIGNEE_TABLE,
                "issue_id",
            );
            left_join(
                &mut sel,
                ISSUE_ASSIGNEE_TABLE,
                "assignee_id",
                USER_TABLE,
                "id",
            );
        }
        Distribution::Label => {
            left_join(&mut sel, ISSUE_TABLE, "id", ISSUE_LABEL_TABLE, "issue_id");
            left_join(&mut sel, ISSUE_LABEL_TABLE, "label_id", LABEL_TABLE, "id");
        }
    }
    if measure == Measure::Estimates {
        left_join(
            &mut sel,
            ISSUE_TABLE,
            "estimate_point_id",
            estimate_point::TABLE,
            "id",
        );
    }
    sel.cond_where(transfer_dist_where(scope, cycle_id));
    for column in group_cols {
        sel.add_group_by([SimpleExpr::from(column)]);
    }
    sel.order_by_expr(items[0].0.clone(), Order::Asc);
    sel
}

/// Move selection (transfer file `:436-443`): the `CycleIssue` rows
/// whose issues transfer — live bridges in this cycle whose issue is
/// live, not a draft, and in one of [`OPEN_STATE_GROUPS`]. The `states`
/// join is **inner** (ported bug 9). The model's default `-created_at`
/// ordering is load-bearing: the iteration order feeds the
/// `update_cycle_issue_activity` payload (ported bug 11). The
/// per-row `cycle_id` rewrite + `bulk_update` and the `issue_activity`
/// payload assembly are handler/tasks-owned; this statement is the
/// exact row set they consume.
pub fn transfer_move_select(scope: &TenantScope<'_>, cycle_id: Uuid) -> SelectStatement {
    let mut sel = Query::select();
    select_table_columns(&mut sel, cycle_issue::TABLE, cycle_issue::COLUMNS);
    sel.from(table(cycle_issue::TABLE));
    inner_join(&mut sel, cycle_issue::TABLE, "issue_id", ISSUE_TABLE, "id");
    inner_join(&mut sel, ISSUE_TABLE, "state_id", state::TABLE, "id");
    join_workspace(&mut sel, cycle_issue::TABLE);
    sel.cond_where(
        Condition::all()
            .add(col(cycle_issue::TABLE, "deleted_at").is_null())
            .add(col(cycle_issue::TABLE, "cycle_id").eq(cycle_id))
            .add(col(ISSUE_TABLE, "archived_at").is_null())
            .add(col(ISSUE_TABLE, "is_draft").not())
            .add(col(state::TABLE, "group").is_in(OPEN_STATE_GROUPS.iter().copied()))
            .add(col(cycle_issue::TABLE, "project_id").eq(scope.project_id))
            .add(col(WORKSPACE_TABLE, "slug").eq(scope.slug)),
    );
    sel.order_by(
        (Alias::new(cycle_issue::TABLE), Alias::new("created_at")),
        Order::Desc,
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
    fn cycle_id() -> Uuid {
        Uuid::from_u128(0xC)
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

    /// The literal `CASE` filter every Q1 count carries.
    const LIVE_FILTER: &str = r#""cycle_issues"."deleted_at" IS NULL AND "issues"."archived_at" IS NULL AND (NOT "issues"."is_draft")"#;

    #[test]
    fn q1_list_live_scope_and_joins() {
        // FX-CYCMOD-04 Q1 (`cycle.py:89-167` + `:197`).
        let sql = render(&cycle_list(
            &OrderBy::default_cycle(),
            ArchivedFilter::Live,
            &scope(),
            actor_id(),
        ));
        assert!(
            sql.starts_with(r#"SELECT DISTINCT "cycles"."id", "#),
            "{sql}"
        );
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
        // sea-query renders `LEFT JOIN`; Django renders the semantically
        // identical `LEFT OUTER JOIN`.
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
        // Tenant scope + member scope + live filter, values bound.
        assert!(
            sql.contains(r#"WHERE "cycles"."deleted_at" IS NULL AND"#),
            "{sql}"
        );
        assert!(
            sql.contains(&format!(
                r#""workspaces"."slug" = '{SLUG}' AND "cycles"."project_id" = {}"#,
                lit(project_id())
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
        assert!(sql.contains(r#""cycles"."archived_at" IS NULL"#), "{sql}");
        // Only the base table carries a deleted predicate.
        assert!(!sql.contains(r#""projects"."deleted_at""#), "{sql}");
        assert!(!sql.contains(r#""workspaces"."deleted_at""#), "{sql}");
        assert!(!sql.contains(r#""project_members"."deleted_at""#), "{sql}");
        // No IssueManager guards ride the annotation joins.
        assert!(!sql.contains("triage"), "{sql}");
        assert!(sql.contains(r#"GROUP BY "cycles"."id""#), "{sql}");
        assert!(
            sql.ends_with(r#"ORDER BY "cycles"."created_at" DESC"#),
            "{sql}"
        );
    }

    #[test]
    fn q1_annotations_count_rows_with_no_distinct() {
        // FX-CYCMOD-04 Q1 (`cycle.py:100-164`): no DISTINCT anywhere
        // (ported bug 1 — the reverse of the module shape). Django's
        // `FILTER (WHERE ...)` is the `COUNT(CASE WHEN ... THEN x END)`
        // form here — the same row count.
        let sql = render(&cycle_list(
            &OrderBy::default_cycle(),
            ArchivedFilter::Live,
            &scope(),
            actor_id(),
        ));
        assert!(
            sql.contains(&format!(
                r#"COUNT((CASE WHEN ({LIVE_FILTER}) THEN "cycle_issues"."id" END)) AS "total_issues""#
            )),
            "{sql}"
        );
        for (group, alias) in GROUPED_COUNTS {
            assert!(
                sql.contains(&format!(
                    r#"COUNT((CASE WHEN ({LIVE_FILTER} AND "states"."group" = '{group}') THEN "states"."group" END)) AS "{alias}""#
                )),
                "{sql}"
            );
        }
        assert!(!sql.contains("COUNT(DISTINCT"), "{sql}");
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
    fn q1_bare_queryset_carries_no_archived_predicate() {
        let sql = render(&cycle_list(
            &OrderBy::default_cycle(),
            ArchivedFilter::Any,
            &scope(),
            actor_id(),
        ));
        // The projected `"cycles"."archived_at"` column stays (it is a
        // real column); only the WHERE predicate is absent.
        assert!(!sql.contains(r#""cycles"."archived_at" IS"#), "{sql}");
    }

    #[test]
    fn q1_order_passthrough() {
        // kwargs `.order_by(...)` passes through untouched.
        let sql = render(&cycle_list(
            &OrderBy::new("name", false),
            ArchivedFilter::Live,
            &scope(),
            actor_id(),
        ));
        assert!(sql.ends_with(r#"ORDER BY "cycles"."name" ASC"#), "{sql}");
    }

    #[test]
    fn q1_cycle_view_predicates() {
        // `cycle.py:198-270`; the instant binds as a value.
        let now = DateTime::<Utc>::from_timestamp(1_700_000_000, 0).expect("instant");
        let now_lit = "'2023-11-14 22:13:20.000000 +00:00'";
        let order = OrderBy::default_cycle();
        let all = render(&cycle_list_filtered(
            &order,
            CycleView::All,
            now,
            &scope(),
            actor_id(),
        ));
        let live = render(&cycle_list(
            &order,
            ArchivedFilter::Live,
            &scope(),
            actor_id(),
        ));
        assert_eq!(all, live, "All adds nothing");
        assert!(!CycleView::All.binds_now());
        assert!(!CycleView::Draft.binds_now());
        assert!(CycleView::Current.binds_now());
        let current = render(&cycle_list_filtered(
            &order,
            CycleView::Current,
            now,
            &scope(),
            actor_id(),
        ));
        assert!(
            current.contains(&format!(
                r#"AND ("cycles"."start_date" <= {now_lit} AND "cycles"."end_date" >= {now_lit}) GROUP BY"#
            )),
            "{current}"
        );
        let upcoming = render(&cycle_list_filtered(
            &order,
            CycleView::Upcoming,
            now,
            &scope(),
            actor_id(),
        ));
        assert!(
            upcoming.contains(&format!(
                r#"AND "cycles"."start_date" > {now_lit} GROUP BY"#
            )),
            "{upcoming}"
        );
        let completed = render(&cycle_list_filtered(
            &order,
            CycleView::Completed,
            now,
            &scope(),
            actor_id(),
        ));
        assert!(
            completed.contains(&format!(r#"AND "cycles"."end_date" < {now_lit} GROUP BY"#)),
            "{completed}"
        );
        let draft = render(&cycle_list_filtered(
            &order,
            CycleView::Draft,
            now,
            &scope(),
            actor_id(),
        ));
        assert!(
            draft.contains(
                r#"AND ("cycles"."end_date" IS NULL AND "cycles"."start_date" IS NULL) GROUP BY"#
            ),
            "{draft}"
        );
        assert!(!draft.contains(now_lit), "{draft}");
        let incomplete = render(&cycle_list_filtered(
            &order,
            CycleView::Incomplete,
            now,
            &scope(),
            actor_id(),
        ));
        assert!(
            incomplete.contains(&format!(
                r#"AND ("cycles"."end_date" >= {now_lit} OR "cycles"."end_date" IS NULL) GROUP BY"#
            )),
            "{incomplete}"
        );
        // Every filtered variant keeps the live predicate.
        for sql in [&current, &upcoming, &completed, &draft, &incomplete] {
            assert!(sql.contains(r#""cycles"."archived_at" IS NULL"#), "{sql}");
        }
    }

    #[test]
    fn q2_detail_adds_live_plus_pk() {
        let sql = render(&cycle_detail(
            &OrderBy::default_cycle(),
            &scope(),
            cycle_id(),
            actor_id(),
        ));
        let live = render(&cycle_list(
            &OrderBy::default_cycle(),
            ArchivedFilter::Live,
            &scope(),
            actor_id(),
        ));
        assert!(
            sql.contains(&format!(
                r#""cycles"."archived_at" IS NULL AND "cycles"."id" = {} GROUP BY"#,
                lit(cycle_id())
            )),
            "{sql}"
        );
        // Same head as Q1: select list and joins are identical.
        let head = |s: &str| s.split(" WHERE ").next().expect("head").to_owned();
        assert_eq!(head(&sql), head(&live));
    }

    #[test]
    fn q3_archived_adds_estimates() {
        let sql = render(&archived_cycle_list(
            &OrderBy::default_cycle(),
            &scope(),
            actor_id(),
        ));
        assert!(
            sql.contains(r#""cycles"."archived_at" IS NOT NULL"#),
            "{sql}"
        );
        assert!(!sql.contains(r#""cycles"."archived_at" IS NULL"#), "{sql}");
        assert!(
            sql.contains(
                r#"LEFT JOIN "estimate_points" ON "issues"."estimate_point_id" = "estimate_points"."id" INNER JOIN "users""#
            ),
            "{sql}"
        );
        // Ported bug 7: the total has no filter.
        assert!(
            sql.contains(r#"SUM("estimate_points"."key") AS "total_estimates""#),
            "{sql}"
        );
        assert!(
            sql.contains(&format!(
                r#"SUM((CASE WHEN ({LIVE_FILTER} AND "states"."group" = 'completed') THEN "estimate_points"."key" END)) AS "completed_estimates""#
            )),
            "{sql}"
        );
        assert!(
            sql.contains(&format!(
                r#"SUM((CASE WHEN ({LIVE_FILTER} AND "states"."group" = 'started') THEN "estimate_points"."key" END)) AS "started_estimates""#
            )),
            "{sql}"
        );
        assert_eq!(ESTIMATE_ALIASES.len(), 3);
        for alias in COUNT_ALIASES.iter().chain(ESTIMATE_ALIASES) {
            assert_eq!(
                sql.matches(&format!(r#" AS "{alias}""#)).count(),
                1,
                "{alias}"
            );
        }
        // Q1 has no estimate join.
        let live = render(&cycle_list(
            &OrderBy::default_cycle(),
            ArchivedFilter::Live,
            &scope(),
            actor_id(),
        ));
        assert!(!live.contains("estimate_points"), "{live}");
    }

    const SUB_ISSUES: &str = r#"(SELECT COUNT("U0"."id") FROM "issues" AS "U0" LEFT JOIN "states" AS "U1" ON "U0"."state_id" = "U1"."id" INNER JOIN "projects" AS "U2" ON "U0"."project_id" = "U2"."id" WHERE "U0"."deleted_at" IS NULL AND (NOT ("U1"."group" = 'triage' AND "U1"."group" IS NOT NULL)) AND "U0"."archived_at" IS NULL AND "U2"."archived_at" IS NULL AND (NOT "U0"."is_draft") AND "U0"."parent_id" = "#;

    #[test]
    fn q4_queryset_distinct_member_scope_and_subquery() {
        let sql = render(&cycle_issue_queryset(
            &OrderBy::default_cycle(),
            &scope(),
            cycle_id(),
            actor_id(),
        ));
        assert!(
            sql.starts_with(r#"SELECT DISTINCT "cycle_issues"."id", "#),
            "{sql}"
        );
        // The subquery is correlated to the bridge's issue; the full
        // manager guards render inside (ported bugs 5/6).
        assert!(
            sql.contains(&format!(
                r#"{SUB_ISSUES}"cycle_issues"."issue_id") AS "sub_issues_count" FROM "cycle_issues""#
            )),
            "{sql}"
        );
        // Join order: issues, workspaces, projects, project_members,
        // cycles, projects T8, states.
        let joins = [
            r#"INNER JOIN "issues" ON "cycle_issues"."issue_id" = "issues"."id""#,
            r#"INNER JOIN "workspaces" ON "cycle_issues"."workspace_id" = "workspaces"."id""#,
            r#"INNER JOIN "projects" ON "cycle_issues"."project_id" = "projects"."id""#,
            r#"INNER JOIN "project_members" ON "projects"."id" = "project_members"."project_id""#,
            r#"INNER JOIN "cycles" ON "cycle_issues"."cycle_id" = "cycles"."id""#,
            r#"INNER JOIN "projects" AS "T8" ON "issues"."project_id" = "T8"."id""#,
            r#"LEFT JOIN "states" ON "issues"."state_id" = "states"."id""#,
        ];
        let mut at = 0;
        for join in joins {
            let found = sql[at..]
                .find(join)
                .unwrap_or_else(|| panic!("{join}: {sql}"));
            at += found + join.len();
        }
        // No manager guards on the outer join; no project/cycle archived
        // predicates (ported bugs 3/5).
        let where_ = sql.split(" WHERE ").nth(2).expect("outer where");
        assert!(!where_.contains("triage"), "{where_}");
        assert!(!where_.contains("archived_at"), "{where_}");
        assert!(
            where_.starts_with(&format!(
                r#""cycle_issues"."deleted_at" IS NULL AND ("workspaces"."slug" = '{SLUG}' AND "cycle_issues"."project_id" = {}) AND "project_members"."is_active" = TRUE AND "project_members"."member_id" = {} AND "cycle_issues"."cycle_id" = {} ORDER BY "cycle_issues"."created_at" DESC"#,
                lit(project_id()),
                lit(actor_id()),
                lit(cycle_id())
            )),
            "{where_}"
        );
    }

    #[test]
    fn q4_get_issue_shape_annotations_and_guards() {
        let sql = render(&cycle_issue_list_get(
            &OrderBy::default_issue(),
            &scope(),
            cycle_id(),
        ));
        assert!(sql.starts_with(r#"SELECT "issues".*, "#), "{sql}");
        assert!(!sql.contains("DISTINCT"), "{sql}");
        assert!(
            sql.contains(&format!(
                r#"{SUB_ISSUES}"issues"."id") AS "sub_issues_count", "cycle_issues"."id" AS "bridge_id", (SELECT COUNT("U0"."id") FROM "issue_links" AS "U0" WHERE "U0"."deleted_at" IS NULL AND "U0"."issue_id" = "issues"."id") AS "link_count", (SELECT COUNT("U0"."id") FROM "file_assets" AS "U0" WHERE "U0"."deleted_at" IS NULL AND "U0"."entity_type" = 'ISSUE_ATTACHMENT' AND "U0"."issue_id" = "issues"."id") AS "attachment_count" FROM "issues""#
            )),
            "{sql}"
        );
        assert!(
            sql.contains(
                r#"FROM "issues" LEFT JOIN "states" ON "issues"."state_id" = "states"."id" INNER JOIN "projects" ON "issues"."project_id" = "projects"."id" INNER JOIN "cycle_issues" ON "issues"."id" = "cycle_issues"."issue_id" INNER JOIN "workspaces" ON "issues"."workspace_id" = "workspaces"."id" LEFT JOIN "issues" AS "T7" ON "issues"."parent_id" = "T7"."id" WHERE"#
            ),
            "{sql}"
        );
        // The outer manager guards, then bridge scope, then tenant scope.
        assert!(
            sql.contains(&format!(
                r#"WHERE "issues"."deleted_at" IS NULL AND (NOT ("states"."group" = 'triage' AND "states"."group" IS NOT NULL)) AND "issues"."archived_at" IS NULL AND "projects"."archived_at" IS NULL AND (NOT "issues"."is_draft") AND "cycle_issues"."cycle_id" = {} AND "cycle_issues"."deleted_at" IS NULL AND "issues"."project_id" = {} AND "workspaces"."slug" = '{SLUG}' ORDER BY "issues"."created_at" ASC"#,
                lit(cycle_id()),
                lit(project_id())
            )),
            "{sql}"
        );
        assert!(!sql.contains("project_members"), "{sql}");
        for alias in [
            "sub_issues_count",
            "bridge_id",
            "link_count",
            "attachment_count",
        ] {
            assert_eq!(
                sql.matches(&format!(r#" AS "{alias}""#)).count(),
                1,
                "{alias}"
            );
        }
    }

    #[test]
    fn q4_get_order_random_renders_random_asc() {
        let sql = render(&cycle_issue_list_get(
            &OrderBy::random(),
            &scope(),
            cycle_id(),
        ));
        assert!(sql.ends_with("ORDER BY RANDOM() ASC"), "{sql}");
    }

    #[test]
    fn q4_get_order_traversal_renders_qualified() {
        let sql = render(&cycle_issue_list_get(
            &OrderBy::table(state::TABLE, "sequence", true),
            &scope(),
            cycle_id(),
        ));
        assert!(
            sql.ends_with(r#"ORDER BY "states"."sequence" DESC"#),
            "{sql}"
        );
        let sql = render(&cycle_issue_list_get(
            &OrderBy::table(PARENT_ALIAS, "name", false),
            &scope(),
            cycle_id(),
        ));
        assert!(sql.ends_with(r#"ORDER BY "T7"."name" ASC"#), "{sql}");
    }

    #[test]
    fn q4_get_order_alias_renders_bare() {
        let sql = render(&cycle_issue_list_get(
            &OrderBy::alias("sub_issues_count", true),
            &scope(),
            cycle_id(),
        ));
        assert!(
            sql.ends_with(r#"ORDER BY "sub_issues_count" DESC"#),
            "{sql}"
        );
    }

    #[test]
    fn q4_get_order_m2m_joins_and_inverts_direction() {
        let sql = render(&cycle_issue_list_get(
            &OrderBy::m2m(M2MOrder::Assignees, false),
            &scope(),
            cycle_id(),
        ));
        assert!(
            sql.contains(
                r#"LEFT JOIN "issues" AS "T7" ON "issues"."parent_id" = "T7"."id" LEFT JOIN "issue_assignees" ON "issues"."id" = "issue_assignees"."issue_id" LEFT JOIN "users" ON "issue_assignees"."assignee_id" = "users"."id" WHERE"#
            ),
            "{sql}"
        );
        // `User.Meta.ordering = ('-created_at',)`: plain renders DESC.
        assert!(
            sql.ends_with(r#"ORDER BY "users"."created_at" DESC"#),
            "{sql}"
        );
        let sql = render(&cycle_issue_list_get(
            &OrderBy::m2m(M2MOrder::Labels, true),
            &scope(),
            cycle_id(),
        ));
        assert!(
            sql.contains(
                r#"LEFT JOIN "issue_labels" ON "issues"."id" = "issue_labels"."issue_id" LEFT JOIN "labels" ON "issue_labels"."label_id" = "labels"."id" WHERE"#
            ),
            "{sql}"
        );
        assert!(
            sql.ends_with(r#"ORDER BY "labels"."created_at" ASC"#),
            "{sql}"
        );
        // The default shape adds neither join.
        let plain = render(&cycle_issue_list_get(
            &OrderBy::default_issue(),
            &scope(),
            cycle_id(),
        ));
        assert!(!plain.contains("issue_assignees"), "{plain}");
        assert!(!plain.contains("issue_labels"), "{plain}");
    }

    #[test]
    fn q4_get_order_related_terms_and_joins() {
        let cases: &[(RelatedOrder, bool, Option<&str>, &str)] = &[
            (
                RelatedOrder::State,
                false,
                None,
                r#"ORDER BY "states"."sequence" ASC"#,
            ),
            (
                RelatedOrder::State,
                true,
                None,
                r#"ORDER BY "states"."sequence" DESC"#,
            ),
            (
                RelatedOrder::Project,
                false,
                None,
                r#"ORDER BY "projects"."created_at" DESC"#,
            ),
            (
                RelatedOrder::Project,
                true,
                None,
                r#"ORDER BY "projects"."created_at" ASC"#,
            ),
            (
                RelatedOrder::Workspace,
                false,
                None,
                r#"ORDER BY "workspaces"."created_at" DESC"#,
            ),
            (
                RelatedOrder::Parent,
                false,
                None,
                r#"ORDER BY "T7"."created_at" DESC"#,
            ),
            (
                RelatedOrder::CreatedBy,
                false,
                Some(r#"LEFT JOIN "users" ON "issues"."created_by_id" = "users"."id" WHERE"#),
                r#"ORDER BY "users"."created_at" DESC"#,
            ),
            (
                RelatedOrder::UpdatedBy,
                true,
                Some(r#"LEFT JOIN "users" ON "issues"."updated_by_id" = "users"."id" WHERE"#),
                r#"ORDER BY "users"."created_at" ASC"#,
            ),
            (
                RelatedOrder::EstimatePoint,
                false,
                Some(
                    r#"LEFT JOIN "estimate_points" ON "issues"."estimate_point_id" = "estimate_points"."id" WHERE"#,
                ),
                r#"ORDER BY "estimate_points"."value" ASC"#,
            ),
            (
                RelatedOrder::AssignedPod,
                false,
                Some(r#"LEFT JOIN "pod" ON "issues"."assigned_pod_id" = "pod"."id" WHERE"#),
                r#"ORDER BY "pod"."is_default" DESC, "pod"."created_at" ASC"#,
            ),
            (
                RelatedOrder::AssignedPod,
                true,
                Some(r#"LEFT JOIN "pod" ON "issues"."assigned_pod_id" = "pod"."id" WHERE"#),
                r#"ORDER BY "pod"."is_default" ASC, "pod"."created_at" DESC"#,
            ),
            (
                RelatedOrder::IssueCycle,
                false,
                None,
                r#"ORDER BY "cycle_issues"."created_at" DESC"#,
            ),
        ];
        for (which, descending, join, order) in cases {
            let sql = render(&cycle_issue_list_get(
                &OrderBy::related(*which, *descending),
                &scope(),
                cycle_id(),
            ));
            assert!(sql.ends_with(order), "{which:?}/{descending}: {sql}");
            match join {
                Some(join) => assert!(
                    sql.contains(&format!(r#""T7"."id" {join}"#)),
                    "{which:?}: {sql}"
                ),
                None => assert!(
                    sql.contains(r#""T7"."id" WHERE"#),
                    "{which:?} adds no join: {sql}"
                ),
            }
        }
    }

    #[test]
    fn q4_get_order_fk_traversal_joins_then_orders_tail() {
        let sql = render(&cycle_issue_list_get(
            &OrderBy::traversal(TraversalOrder::CreatedBy, "email", false),
            &scope(),
            cycle_id(),
        ));
        assert!(
            sql.contains(r#"LEFT JOIN "users" ON "issues"."created_by_id" = "users"."id" WHERE"#),
            "{sql}"
        );
        assert!(sql.ends_with(r#"ORDER BY "users"."email" ASC"#), "{sql}");
        let sql = render(&cycle_issue_list_get(
            &OrderBy::traversal(TraversalOrder::AssignedPod, "name", true),
            &scope(),
            cycle_id(),
        ));
        assert!(
            sql.contains(r#"LEFT JOIN "pod" ON "issues"."assigned_pod_id" = "pod"."id" WHERE"#),
            "{sql}"
        );
        assert!(sql.ends_with(r#"ORDER BY "pod"."name" DESC"#), "{sql}");
    }

    #[test]
    fn q4_get_order_m2m_traversal_joins_then_orders_tail() {
        let sql = render(&cycle_issue_list_get(
            &OrderBy::m2m_traversal(M2MOrder::Labels, "name", false),
            &scope(),
            cycle_id(),
        ));
        assert!(
            sql.contains(
                r#"LEFT JOIN "issue_labels" ON "issues"."id" = "issue_labels"."issue_id" LEFT JOIN "labels" ON "issue_labels"."label_id" = "labels"."id" WHERE"#
            ),
            "{sql}"
        );
        assert!(sql.ends_with(r#"ORDER BY "labels"."name" ASC"#), "{sql}");
    }

    #[test]
    fn traversal_head_mapping() {
        assert_eq!(issue_traversal_table("state"), Some(state::TABLE));
        assert_eq!(issue_traversal_table("project"), Some(project::TABLE));
        assert_eq!(issue_traversal_table("workspace"), Some(WORKSPACE_TABLE));
        assert_eq!(issue_traversal_table("parent"), Some(PARENT_ALIAS));
        assert_eq!(
            issue_traversal_table("issue_cycle"),
            Some(cycle_issue::TABLE)
        );
        assert_eq!(issue_traversal_table("nope"), None);
        assert_eq!(issue_m2m_order("assignees"), Some(M2MOrder::Assignees));
        assert_eq!(issue_m2m_order("labels"), Some(M2MOrder::Labels));
        assert_eq!(issue_m2m_order("state"), None);
        assert_eq!(
            issue_join_traversal("created_by"),
            Some(TraversalOrder::CreatedBy)
        );
        assert_eq!(
            issue_join_traversal("updated_by"),
            Some(TraversalOrder::UpdatedBy)
        );
        assert_eq!(
            issue_join_traversal("estimate_point"),
            Some(TraversalOrder::EstimatePoint)
        );
        assert_eq!(
            issue_join_traversal("assigned_pod"),
            Some(TraversalOrder::AssignedPod)
        );
        assert_eq!(issue_join_traversal("state"), None);
    }

    #[test]
    fn q5_lookup_scope() {
        let sql = render(&cycle_issue_detail_lookup(&scope(), cycle_id(), issue_id()));
        assert!(
            sql.starts_with(r#"SELECT "cycle_issues"."id", "cycle_issues"."created_at""#),
            "{sql}"
        );
        assert!(
            sql.ends_with(&format!(
                r#"FROM "cycle_issues" INNER JOIN "workspaces" ON "cycle_issues"."workspace_id" = "workspaces"."id" WHERE "cycle_issues"."deleted_at" IS NULL AND "cycle_issues"."cycle_id" = {} AND "cycle_issues"."issue_id" = {} AND "cycle_issues"."project_id" = {} AND "workspaces"."slug" = '{SLUG}'"#,
                lit(cycle_id()),
                lit(issue_id()),
                lit(project_id())
            )),
            "{sql}"
        );
        assert!(!sql.contains("LIMIT"), "{sql}");
        assert!(!sql.contains("ORDER BY"), "{sql}");
    }

    #[test]
    fn order_defaults_match_django() {
        assert_eq!(OrderBy::default_cycle(), OrderBy::new("created_at", true));
        assert_eq!(OrderBy::default_issue(), OrderBy::new("created_at", false));
    }

    #[test]
    fn table_literals_match_ports() {
        assert_eq!(cycle::TABLE, "cycles");
        assert_eq!(cycle_issue::TABLE, "cycle_issues");
        assert_eq!(project::TABLE, "projects");
        assert_eq!(project_member::TABLE, "project_members");
        assert_eq!(state::TABLE, "states");
        assert_eq!(estimate::TABLE, "estimates");
        assert_eq!(estimate_point::TABLE, "estimate_points");
        assert_eq!(WORKSPACE_TABLE, "workspaces");
        assert_eq!(ISSUE_TABLE, "issues");
        assert_eq!(USER_TABLE, "users");
    }

    #[test]
    fn q6_cycle_lookup_is_first_shaped() {
        let sql = render(&transfer_cycle_lookup(&scope(), cycle_id()));
        assert!(
            sql.starts_with(r#"SELECT "cycles"."id", "cycles"."created_at""#),
            "{sql}"
        );
        assert!(
            sql.ends_with(&format!(
                r#"FROM "cycles" INNER JOIN "workspaces" ON "cycles"."workspace_id" = "workspaces"."id" WHERE "cycles"."deleted_at" IS NULL AND "cycles"."id" = {} AND ("workspaces"."slug" = '{SLUG}' AND "cycles"."project_id" = {}) ORDER BY "cycles"."created_at" DESC LIMIT 1"#,
                lit(cycle_id()),
                lit(project_id())
            )),
            "{sql}"
        );
        assert!(!sql.contains("archived_at IS"), "{sql}");
        assert!(!sql.contains("project_members"), "{sql}");
    }

    #[test]
    fn q6_old_cycle_recounts_carry_issue_deleted_guard() {
        let sql = render(&transfer_old_cycle(&scope(), cycle_id()));
        let filter = r#""cycle_issues"."deleted_at" IS NULL AND "issues"."archived_at" IS NULL AND "issues"."deleted_at" IS NULL AND (NOT "issues"."is_draft")"#;
        assert!(
            sql.contains(&format!(
                r#"COUNT((CASE WHEN ({filter}) THEN "cycle_issues"."id" END)) AS "total_issues""#
            )),
            "{sql}"
        );
        for (group, alias) in GROUPED_COUNTS {
            assert!(
                sql.contains(&format!(
                    r#"COUNT((CASE WHEN ({filter} AND "states"."group" = '{group}') THEN "states"."group" END)) AS "{alias}""#
                )),
                "{sql}"
            );
        }
        assert!(
            sql.contains(
                r#"FROM "cycles" INNER JOIN "workspaces" ON "cycles"."workspace_id" = "workspaces"."id" LEFT JOIN "cycle_issues" ON "cycles"."id" = "cycle_issues"."cycle_id" LEFT JOIN "issues" ON "cycle_issues"."issue_id" = "issues"."id" LEFT JOIN "states" ON "issues"."state_id" = "states"."id" WHERE"#
            ),
            "{sql}"
        );
        assert!(
            sql.ends_with(r#"GROUP BY "cycles"."id" ORDER BY "cycles"."id" ASC LIMIT 1"#),
            "{sql}"
        );
        assert!(!sql.contains("project_members"), "{sql}");
        assert!(!sql.contains(r#"INNER JOIN "projects""#), "{sql}");
    }

    #[test]
    fn q6_estimate_type_is_exists_shaped() {
        let sql = render(&transfer_estimate_type(&scope()));
        assert_eq!(
            sql,
            format!(
                r#"SELECT "projects"."id" FROM "projects" INNER JOIN "estimates" ON "projects"."estimate_id" = "estimates"."id" INNER JOIN "workspaces" ON "projects"."workspace_id" = "workspaces"."id" WHERE "projects"."deleted_at" IS NULL AND "projects"."estimate_id" IS NOT NULL AND "estimates"."type" = 'points' AND "projects"."id" = {} AND "workspaces"."slug" = '{SLUG}' LIMIT 1"#,
                lit(project_id())
            )
        );
    }

    #[test]
    fn q6_distributions_share_fragments_and_where() {
        let where_ = format!(
            r#"WHERE "issues"."deleted_at" IS NULL AND (NOT ("states"."group" = 'triage' AND "states"."group" IS NOT NULL)) AND "issues"."archived_at" IS NULL AND "projects"."archived_at" IS NULL AND (NOT "issues"."is_draft") AND "cycle_issues"."cycle_id" = {} AND "cycle_issues"."deleted_at" IS NULL AND "issues"."project_id" = {} AND "workspaces"."slug" = '{SLUG}' GROUP BY"#,
            lit(cycle_id()),
            lit(project_id())
        );
        let avatar = r#"(CASE WHEN ("users"."avatar_asset_id" IS NOT NULL) THEN CONCAT(CAST('/api/assets/v2/static/' AS text), CAST("users"."avatar_asset_id" AS text), CAST('/' AS text)) WHEN ("users"."avatar_asset_id" IS NULL) THEN "users"."avatar" END)"#;
        let value = r#"CAST("estimate_points"."value" AS double precision)"#;
        let estimate_sums = format!(
            r#"SUM({value}) AS "total_estimates", SUM((CASE WHEN ("issues"."archived_at" IS NULL AND "issues"."completed_at" IS NOT NULL AND (NOT "issues"."is_draft")) THEN {value} END)) AS "completed_estimates", SUM((CASE WHEN ("issues"."archived_at" IS NULL AND "issues"."completed_at" IS NULL AND (NOT "issues"."is_draft")) THEN {value} END)) AS "pending_estimates""#
        );
        let issue_counts = r#"COUNT((CASE WHEN ("issues"."archived_at" IS NULL AND (NOT "issues"."is_draft")) THEN "issues"."id" END)) AS "total_issues", COUNT((CASE WHEN ("issues"."archived_at" IS NULL AND "issues"."completed_at" IS NOT NULL AND (NOT "issues"."is_draft")) THEN "issues"."id" END)) AS "completed_issues", COUNT((CASE WHEN ("issues"."archived_at" IS NULL AND "issues"."completed_at" IS NULL AND (NOT "issues"."is_draft")) THEN "issues"."id" END)) AS "pending_issues""#;
        let base_joins = r#"FROM "issues" LEFT JOIN "states" ON "issues"."state_id" = "states"."id" INNER JOIN "projects" ON "issues"."project_id" = "projects"."id" INNER JOIN "cycle_issues" ON "issues"."id" = "cycle_issues"."issue_id" INNER JOIN "workspaces" ON "issues"."workspace_id" = "workspaces"."id""#;
        let assignee_joins = r#"LEFT JOIN "issue_assignees" ON "issues"."id" = "issue_assignees"."issue_id" LEFT JOIN "users" ON "issue_assignees"."assignee_id" = "users"."id""#;
        let label_joins = r#"LEFT JOIN "issue_labels" ON "issues"."id" = "issue_labels"."issue_id" LEFT JOIN "labels" ON "issue_labels"."label_id" = "labels"."id""#;
        let estimate_join = r#"LEFT JOIN "estimate_points" ON "issues"."estimate_point_id" = "estimate_points"."id""#;

        let sql = render(&transfer_distribution(
            &scope(),
            cycle_id(),
            Distribution::Assignee,
            Measure::Estimates,
        ));
        assert_eq!(
            sql,
            format!(
                r#"SELECT "users"."display_name" AS "display_name", "issue_assignees"."assignee_id" AS "assignee_id", {avatar} AS "avatar_url", {estimate_sums} {base_joins} {assignee_joins} {estimate_join} {where_} "users"."display_name", "issue_assignees"."assignee_id", "users"."avatar_asset_id", "users"."avatar" ORDER BY "users"."display_name" ASC"#
            )
        );
        let sql = render(&transfer_distribution(
            &scope(),
            cycle_id(),
            Distribution::Label,
            Measure::Estimates,
        ));
        assert_eq!(
            sql,
            format!(
                r#"SELECT "labels"."name" AS "label_name", "labels"."color" AS "color", "issue_labels"."label_id" AS "label_id", {estimate_sums} {base_joins} {label_joins} {estimate_join} {where_} "labels"."name", "labels"."color", "issue_labels"."label_id" ORDER BY "labels"."name" ASC"#
            )
        );
        let sql = render(&transfer_distribution(
            &scope(),
            cycle_id(),
            Distribution::Assignee,
            Measure::Issues,
        ));
        assert_eq!(
            sql,
            format!(
                r#"SELECT "users"."display_name" AS "display_name", "issue_assignees"."assignee_id" AS "assignee_id", {avatar} AS "avatar_url", {issue_counts} {base_joins} {assignee_joins} {where_} "users"."display_name", "issue_assignees"."assignee_id", "users"."avatar_asset_id", "users"."avatar" ORDER BY "users"."display_name" ASC"#
            )
        );
        let sql = render(&transfer_distribution(
            &scope(),
            cycle_id(),
            Distribution::Label,
            Measure::Issues,
        ));
        assert_eq!(
            sql,
            format!(
                r#"SELECT "labels"."name" AS "label_name", "labels"."color" AS "color", "issue_labels"."label_id" AS "label_id", {issue_counts} {base_joins} {label_joins} {where_} "labels"."name", "labels"."color", "issue_labels"."label_id" ORDER BY "labels"."name" ASC"#
            )
        );
    }

    #[test]
    fn q6_move_select_scope_and_order() {
        let sql = render(&transfer_move_select(&scope(), cycle_id()));
        assert!(sql.starts_with(r#"SELECT "cycle_issues"."id", "#), "{sql}");
        // Ported bug 9: the states join is INNER.
        assert!(
            sql.contains(
                r#"FROM "cycle_issues" INNER JOIN "issues" ON "cycle_issues"."issue_id" = "issues"."id" INNER JOIN "states" ON "issues"."state_id" = "states"."id" INNER JOIN "workspaces" ON "cycle_issues"."workspace_id" = "workspaces"."id" WHERE"#
            ),
            "{sql}"
        );
        assert!(
            sql.ends_with(&format!(
                r#"WHERE "cycle_issues"."deleted_at" IS NULL AND "cycle_issues"."cycle_id" = {} AND "issues"."archived_at" IS NULL AND (NOT "issues"."is_draft") AND "states"."group" IN ('backlog', 'unstarted', 'started', 'review', 'test') AND "cycle_issues"."project_id" = {} AND "workspaces"."slug" = '{SLUG}' ORDER BY "cycle_issues"."created_at" DESC"#,
                lit(cycle_id()),
                lit(project_id())
            )),
            "{sql}"
        );
        assert_eq!(OPEN_STATE_GROUPS.len(), 5);
    }

    #[test]
    fn no_sql_text_in_builders() {
        // Every statement binds its values: rendering with the Postgres
        // builder yields parameters, never inlined ids or slugs.
        let (sql, values) = cycle_issue_list_get(&OrderBy::default_issue(), &scope(), cycle_id())
            .build(PostgresQueryBuilder);
        assert!(!sql.contains(SLUG), "{sql}");
        assert!(!sql.contains(&cycle_id().to_string()), "{sql}");
        // triage (subquery + outer), ISSUE_ATTACHMENT, cycle id, project
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
            sqlx::query_scalar("SELECT to_regclass('public.cycle_issues')::text")
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

    /// Every builder runs and counts over a seeded tenant: a live cycle
    /// with a completed, a started, a draft and an archived bridged
    /// issue, plus an archived cycle; the counts follow the ported
    /// Django filters (drafts and archived issues never count, triage
    /// counts, the transfer move skips completed and draft).
    #[tokio::test]
    async fn live_cycle_reads() {
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
            "cycles",
            "cycle_issues",
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
        // `GROUP BY "cycles"."id"` selects the other cycle columns through
        // the primary key's functional dependence, which the copy must
        // carry too.
        exec(&mut tx, "ALTER TABLE cycles ADD PRIMARY KEY (id)").await;
        let ws = Uuid::new_v4();
        let project = Uuid::new_v4();
        let other_project = Uuid::new_v4();
        let actor = Uuid::new_v4();
        let outsider = Uuid::new_v4();
        let estimate = Uuid::new_v4();
        let point = Uuid::new_v4();
        let label = Uuid::new_v4();
        let (done, started, backlog) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        let (live_cycle, archived_cycle) = (Uuid::new_v4(), Uuid::new_v4());
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
        let bridge = |cycle: Uuid, issue: Uuid, age: i32| {
            format!(
                "INSERT INTO cycle_issues (id, created_at, updated_at, project_id, workspace_id, cycle_id, issue_id) \
                 VALUES ('{}', now() - interval '{age} min', now(), '{project}', '{ws}', '{cycle}', '{issue}')",
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
            format!("INSERT INTO cycles (id, created_at, updated_at, project_id, workspace_id, name, description, owned_by_id, view_props, sort_order, progress_snapshot, logo_props, timezone, version, start_date, end_date, archived_at) VALUES \
                     ('{live_cycle}', now() - interval '2 min', now(), '{project}', '{ws}', 'Sprint 1', '', '{actor}', '{{}}', 65535, '{{}}', '{{}}', 'UTC', 1, now() - interval '1 day', now() + interval '1 day', NULL), \
                     ('{archived_cycle}', now() - interval '1 min', now(), '{project}', '{ws}', 'Sprint 0', '', '{actor}', '{{}}', 55535, '{{}}', '{{}}', 'UTC', 1, now() - interval '20 day', now() - interval '10 day', now())"),
            issue(i_done, done, false, false, true, true, 5),
            issue(i_started, started, false, false, false, false, 4),
            issue(i_draft, backlog, true, false, false, false, 3),
            issue(i_archived, backlog, false, true, false, false, 2),
            issue(i_old, done, false, false, true, true, 1),
            bridge(live_cycle, i_done, 4),
            bridge(live_cycle, i_started, 3),
            bridge(live_cycle, i_draft, 2),
            bridge(live_cycle, i_archived, 1),
            bridge(archived_cycle, i_old, 1),
            format!("INSERT INTO issue_assignees (id, issue_id, assignee_id, project_id, workspace_id, created_at, updated_at) VALUES ('{}', '{i_done}', '{actor}', '{project}', '{ws}', now(), now())", Uuid::new_v4()),
            format!("INSERT INTO issue_labels (id, issue_id, label_id, project_id, workspace_id, created_at, updated_at) VALUES ('{}', '{i_done}', '{label}', '{project}', '{ws}', now(), now())", Uuid::new_v4()),
            format!("INSERT INTO issue_links (id, issue_id, url, project_id, workspace_id, created_at, updated_at, metadata) VALUES ('{}', '{i_started}', 'http://x', '{project}', '{ws}', now(), now(), '{{}}')", Uuid::new_v4()),
        ];
        for sql in &seeds {
            exec(&mut tx, sql).await;
        }
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
        let now = Utc::now();

        // Q1: the live cycle only, two counted issues (draft + archived skip).
        let rows = run(cycle_list(
            &OrderBy::default_cycle(),
            ArchivedFilter::Live,
            &scope,
            actor,
        ))
        .await;
        assert_eq!(rows.len(), 1);
        assert_eq!(get::<Uuid>(&rows[0], "id"), live_cycle);
        assert_eq!(get::<i64>(&rows[0], "total_issues"), 2);
        assert_eq!(get::<i64>(&rows[0], "completed_issues"), 1);
        assert_eq!(get::<i64>(&rows[0], "started_issues"), 1);
        assert_eq!(get::<i64>(&rows[0], "backlog_issues"), 0);
        assert_eq!(get::<String>(&rows[0], "name"), "Sprint 1");
        // Member scope: an inactive member sees nothing.
        let rows = run(cycle_list(
            &OrderBy::default_cycle(),
            ArchivedFilter::Live,
            &scope,
            outsider,
        ))
        .await;
        assert!(rows.is_empty());
        // Both cycles without the archived predicate, `-created_at` order.
        let rows = run(cycle_list(
            &OrderBy::default_cycle(),
            ArchivedFilter::Any,
            &scope,
            actor,
        ))
        .await;
        assert_eq!(
            rows.iter()
                .map(|row| get::<Uuid>(row, "id"))
                .collect::<Vec<_>>(),
            vec![archived_cycle, live_cycle]
        );
        // cycle_view: current matches, upcoming/completed/draft do not,
        // incomplete matches.
        for (view, expected) in [
            (CycleView::Current, 1),
            (CycleView::Upcoming, 0),
            (CycleView::Completed, 0),
            (CycleView::Draft, 0),
            (CycleView::Incomplete, 1),
        ] {
            let rows = run(cycle_list_filtered(
                &OrderBy::default_cycle(),
                view,
                now,
                &scope,
                actor,
            ))
            .await;
            assert_eq!(rows.len(), expected, "{view:?}");
        }
        // Q2: live pk hits, archived pk misses.
        let rows = run(cycle_detail(
            &OrderBy::default_cycle(),
            &scope,
            live_cycle,
            actor,
        ))
        .await;
        assert_eq!(rows.len(), 1);
        assert_eq!(get::<i64>(&rows[0], "total_issues"), 2);
        let rows = run(cycle_detail(
            &OrderBy::default_cycle(),
            &scope,
            archived_cycle,
            actor,
        ))
        .await;
        assert!(rows.is_empty());
        // Q3: the archived cycle with its estimate sums (ported bug 7: the
        // unfiltered total over every fanned row).
        let rows = run(archived_cycle_list(
            &OrderBy::default_cycle(),
            &scope,
            actor,
        ))
        .await;
        assert_eq!(rows.len(), 1);
        assert_eq!(get::<Uuid>(&rows[0], "id"), archived_cycle);
        assert_eq!(get::<i64>(&rows[0], "total_issues"), 1);
        assert_eq!(get::<i64>(&rows[0], "completed_issues"), 1);
        assert_eq!(get::<Option<i64>>(&rows[0], "total_estimates"), Some(3));
        assert_eq!(get::<Option<i64>>(&rows[0], "completed_estimates"), Some(3));
        assert_eq!(get::<Option<i64>>(&rows[0], "started_estimates"), None);
        // Q4 queryset: every live bridge (ported bug 5), newest first.
        let rows = run(cycle_issue_queryset(
            &OrderBy::default_cycle(),
            &scope,
            live_cycle,
            actor,
        ))
        .await;
        assert_eq!(
            rows.iter()
                .map(|row| get::<Uuid>(row, "issue_id"))
                .collect::<Vec<_>>(),
            vec![i_archived, i_draft, i_started, i_done]
        );
        assert!(rows
            .iter()
            .all(|row| get::<i64>(row, "sub_issues_count") == 0));
        // Q4 GET: the manager guards drop the draft and archived issues;
        // ascending created_at; the annotations decode by name.
        let rows = run(cycle_issue_list_get(
            &OrderBy::default_issue(),
            &scope,
            live_cycle,
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
        assert_eq!(get::<i64>(&rows[0], "sub_issues_count"), 0);
        assert_eq!(get::<String>(&rows[0], "name"), "n");
        assert_eq!(get::<i32>(&rows[0], "sequence_id"), 1);
        assert!(get::<Option<Uuid>>(&rows[0], "bridge_id").is_some());
        // M2M ordering multiplies nothing here (one assignee) and orders
        // by the user's created_at.
        let rows = run(cycle_issue_list_get(
            &OrderBy::m2m(M2MOrder::Assignees, false),
            &scope,
            live_cycle,
        ))
        .await;
        assert_eq!(rows.len(), 2);
        let rows = run(cycle_issue_list_get(&OrderBy::random(), &scope, live_cycle)).await;
        assert_eq!(rows.len(), 2);
        // Q5 lookup: slug + project + cycle + issue.
        let rows = run(cycle_issue_detail_lookup(&scope, live_cycle, i_done)).await;
        assert_eq!(rows.len(), 1);
        assert_eq!(get::<Uuid>(&rows[0], "cycle_id"), live_cycle);
        let rows = run(cycle_issue_detail_lookup(&scope, archived_cycle, i_done)).await;
        assert!(rows.is_empty());
        // Q6: the guard lookup finds archived cycles too.
        let rows = run(transfer_cycle_lookup(&scope, archived_cycle)).await;
        assert_eq!(rows.len(), 1);
        assert!(get::<Option<DateTime<Utc>>>(&rows[0], "end_date").is_some());
        let rows = run(transfer_cycle_lookup(
            &TenantScope {
                slug: "acme",
                project_id: other_project,
            },
            archived_cycle,
        ))
        .await;
        assert!(rows.is_empty());
        let rows = run(transfer_old_cycle(&scope, live_cycle)).await;
        assert_eq!(rows.len(), 1);
        assert_eq!(get::<i64>(&rows[0], "total_issues"), 2);
        assert_eq!(get::<i64>(&rows[0], "completed_issues"), 1);
        assert_eq!(get::<i64>(&rows[0], "started_issues"), 1);
        assert_eq!(get::<i64>(&rows[0], "cancelled_issues"), 0);
        assert!(get::<Option<DateTime<Utc>>>(&rows[0], "start_date").is_some());
        // The estimate gate: points estimate on P, none on Q.
        assert_eq!(run(transfer_estimate_type(&scope)).await.len(), 1);
        assert!(run(transfer_estimate_type(&TenantScope {
            slug: "acme",
            project_id: other_project
        }))
        .await
        .is_empty());
        // Distributions: the unassigned started issue groups under NULLs
        // first? No — `ORDER BY display_name ASC` puts NULL last in
        // Postgres, so Ada's row comes first.
        let rows = run(transfer_distribution(
            &scope,
            live_cycle,
            Distribution::Assignee,
            Measure::Estimates,
        ))
        .await;
        assert_eq!(rows.len(), 2);
        assert_eq!(
            get::<Option<String>>(&rows[0], "display_name"),
            Some("Ada".to_owned())
        );
        assert_eq!(get::<Option<Uuid>>(&rows[0], "assignee_id"), Some(actor));
        assert_eq!(
            get::<Option<String>>(&rows[0], "avatar_url"),
            Some("http://a/x.png".to_owned())
        );
        assert_eq!(get::<Option<f64>>(&rows[0], "total_estimates"), Some(3.0));
        assert_eq!(
            get::<Option<f64>>(&rows[0], "completed_estimates"),
            Some(3.0)
        );
        assert_eq!(get::<Option<f64>>(&rows[0], "pending_estimates"), None);
        assert_eq!(get::<Option<String>>(&rows[1], "display_name"), None);
        assert_eq!(get::<Option<f64>>(&rows[1], "total_estimates"), None);
        let rows = run(transfer_distribution(
            &scope,
            live_cycle,
            Distribution::Assignee,
            Measure::Issues,
        ))
        .await;
        assert_eq!(rows.len(), 2);
        assert_eq!(get::<i64>(&rows[0], "total_issues"), 1);
        assert_eq!(get::<i64>(&rows[0], "completed_issues"), 1);
        assert_eq!(get::<i64>(&rows[0], "pending_issues"), 0);
        assert_eq!(get::<i64>(&rows[1], "pending_issues"), 1);
        let rows = run(transfer_distribution(
            &scope,
            live_cycle,
            Distribution::Label,
            Measure::Issues,
        ))
        .await;
        assert_eq!(rows.len(), 2);
        assert_eq!(
            get::<Option<String>>(&rows[0], "label_name"),
            Some("bug".to_owned())
        );
        assert_eq!(
            get::<Option<String>>(&rows[0], "color"),
            Some("#f00".to_owned())
        );
        assert_eq!(get::<Option<Uuid>>(&rows[0], "label_id"), Some(label));
        assert_eq!(get::<i64>(&rows[0], "completed_issues"), 1);
        let rows = run(transfer_distribution(
            &scope,
            live_cycle,
            Distribution::Label,
            Measure::Estimates,
        ))
        .await;
        assert_eq!(get::<Option<f64>>(&rows[0], "total_estimates"), Some(3.0));
        // The move: only the started issue (completed, draft and archived
        // stay).
        let rows = run(transfer_move_select(&scope, live_cycle)).await;
        assert_eq!(
            rows.iter()
                .map(|row| get::<Uuid>(row, "issue_id"))
                .collect::<Vec<_>>(),
            vec![i_started]
        );
        tx.rollback().await.expect("rollback");
    }
}
