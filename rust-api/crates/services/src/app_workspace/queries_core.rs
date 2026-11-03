#![forbid(unsafe_code)]

//! Workspace core querysets: lists, dashboard bundle, themes, export-CSV
//! (D-24 queries A, PIDASHCONV-608).
//!
//! Ports the five query units behind `apps/api/pi_dash/app/views/workspace/base.py`
//! to SQL text, following the pilot-2 precedent (`app_cycles/queries.rs`):
//! each builder returns a fragment the handler splices into the statement
//! it executes. The services crate carries no `sea-query`/`sqlx`
//! dependency, so placeholders stay symbolic — `:user`, `:slug`, `:month`,
//! `:date`, `:user_id`, `:search`, `:owner`, `:iso_week`, `:today`,
//! `:from_date`, `:tzname` — exactly the notation the fixtures use;
//! handlers bind them.
//!
//! Sources (drift baseline `01a93e17`; verified no drift at `c948ceaf`):
//! - `app/views/workspace/base.py:60-81` — `WorkSpaceViewSet.get_queryset`
//!   (member-count subquery `:66-71`, `select_related("owner")`,
//!   `search_fields=["name"] :60`, `filterset_fields=["owner"] :61`,
//!   `order_by("name") :75`, membership scope `:76-79`,
//!   `total_members` annotation `:80`).
//! - `app/views/workspace/base.py:204-241` — `UserWorkSpacesEndpoint.get`
//!   (`fields=` parse `:210`, member count `:211-216`, role subquery
//!   `:218-220`, member prefetch `:223-228`, annotations `:229`, membership
//!   filter `:230`, `distinct() :231`, `filter_queryset` `:235`).
//! - `app/views/workspace/base.py:257-348` — dashboard bundle (`WeekInMonth`
//!   `:257-259`, activity series `:264-274`, completed-by-week `:276-290`,
//!   assigned/pending/completed counts `:292-302`, due-week count
//!   `:304-309`, state distribution `:311-317`, overdue/upcoming `.values()`
//!   lists `:319-333`, response keys `:336-346`).
//! - `app/views/workspace/base.py:351-365` — theme `get_queryset :356-357`
//!   + create lookup `Workspace.objects.get(slug=slug) :360`.
//! - `app/views/workspace/base.py:368-420` — export-CSV query (`?date`
//!   required `:380-381`, activity filter `:383-389`, `select_related` 4
//!   `:390`, `[:10000]` cap `:390`).
//!
//! Every fragment below was verified against SQL compiled live from the
//! Django ORM (Django 4.2, `USE_TZ=True`, `TIME_ZONE="UTC"`,
//! `settings/common.py:361-362`); where the live SQL contradicts the
//! fixture's prose, the live SQL wins and the gap is called out (bug 7).
//!
//! Fixture oracle: F-W24-09 (`rust-api/fixtures/app_workspace/queries/`
//! `core.sql` + `core.rows.json`). The unit tests below pin the builders
//! against that file so transcription drift fails the build.
//!
//! Existing bugs ported as-is (translation, don't redesign):
//! 1. Dashboard Q2 `month` defaults to the int `1` (`:276`) — with no
//!    `?month=` the buckets are always January's, whatever the current
//!    month is ([`DASHBOARD_MONTH_DEFAULT`]).
//! 2. Q5 completed count uses the literal `"completed"` (`:301`), not the
//!    complement of Q4's `~Q(state__group__in=CLOSED)` — `cancelled`
//!    issues sit in neither count. Both predicates are ported, not unified.
//! 3. `filter_queryset` runs BEFORE the membership scope in R1 (`:74`) but
//!    AFTER it (and after `distinct()`) in R2 (`:235`). Both predicates
//!    are plain conjuncts on `workspaces` columns, so the order is
//!    unobservable in the emitted SQL — ported as a doc note, one fragment
//!    each ([`search_name_sql`], [`filterset_owner_sql`]).
//! 4. The R2 `role` subquery keeps its dead `ORDER BY created_at DESC`
//!    (`:218-220` never clears the model default ordering, unlike the
//!    member-count subquery's `.order_by()`). Scalar-subquery `ORDER BY`
//!    without `LIMIT` is a no-op; ported verbatim.
//! 5. `fields=` is parsed (`:210`) but dead — the serializer renders the
//!    full shape regardless (DynamicBase quirk). The query shape never
//!    varies with it; there is no fragment to build.
//! 6. Q8/Q9 compare `DateField`s against `timezone.now()` (`:323`, `:329`):
//!    Django truncates the datetime to a date, so "overdue" means
//!    `target_date < <UTC date>` and "upcoming" means
//!    `start_date >= <UTC date>` — time-of-day never matters. Ported as
//!    date binds ([`OVERDUE_WHERE_SQL`] takes `:today`).
//! 7. Fixture gap (fixture wrong, live SQL wins): F-W24-09 claims the
//!    member-count subquery returns `NULL` for zero members. The compiled
//!    SQL is `SELECT Count(id) ...` with no `GROUP BY`, which always
//!    returns exactly one row — `0`, never `NULL`. Ported as `COUNT`
//!    (0-on-empty); see the PR description.
//!
//! Soft-delete scoping, read carefully — Django applies the default
//! manager's `deleted_at IS NULL` to the *base* table and to every fresh
//! `SomeModel.objects` subquery, but NEVER to tables reached through a
//! `filter()` join. So the membership/assignee/project-member JOINs below
//! carry no `deleted_at` guard (a soft-deleted-but-active membership still
//! grants visibility), while the base tables and the annotation subqueries
//! do. `users` has no `deleted_at` column at all (`User` is not a
//! `SoftDeleteModel`, `db/models/user.py:56`). Every guard below matches
//! the live-compiled SQL predicate for predicate.
//!
//! Time zones, read carefully — Django's `__date` / `Extract*` shift by the
//! ACTIVATED time zone (`get_current_timezone_name`), which `TimezoneMixin`
//! (`app/views/base.py`) sets to `request.user.user_timezone` on every
//! authenticated request. Production Q1/Q2/R5 SQL therefore carries the
//! user's zone; `settings.TIME_ZONE` (`"UTC"`) is only the default when
//! nothing is activated. The fragments bind it as `:tzname` (handlers bind
//! `request.user.user_timezone`, default `"UTC"`, `db/models/user.py:120`).
//! Unaffected: Q6 (`ExtractWeek` over a `DateField` — no shift), Q8/Q9 and
//! `:iso_week`/`:today` (Python-side UTC dates, verified live).
//!
//! Out of scope (owned by sibling handler issues PIDASHCONV-615/620): the
//! envelopes, response shaping, `fields=` parsing, the R2 member-prefetch
//! execution (a second query; its stable predicates are pinned in
//! [`member_prefetch_where_sql`]), CSV rendering, and `get()` 404/500
//! mapping. Their queryset-adjacent constants that ARE in scope here
//! (value lists, orderings, limits, `.values()` key lists) are marked.

use std::fmt::Write as _;

// ---------------------------------------------------------------------------
// Shared vocabulary
// ---------------------------------------------------------------------------

/// Closed state groups (`utils/constants.py:88`,
/// `STATE_GROUP_ORDER[-2:]`).
pub const CLOSED_STATE_GROUPS: &[&str] = &["completed", "cancelled"];

/// `Issue.issue_objects` manager triage exclusion
/// (`db/models/issue.py:95-104`): triage lives under its own manager.
pub const TRIAGE_GROUP: &str = "triage";

/// Default `AT TIME ZONE` name: `settings.TIME_ZONE`
/// (`settings/common.py:362`), which is also the `User.user_timezone`
/// default (`db/models/user.py:120`). Django shifts `__date` / `Extract*`
/// by the ACTIVATED zone (the request user's zone in production — see the
/// module docs), so fragments bind `:tzname` and handlers bind
/// `request.user.user_timezone`; this const is the bind value when no
/// request user applies. `USE_TZ` is `True` (`:361`); with it off Django
/// emits no shift at all.
pub const DJANGO_TIME_ZONE_SQL: &str = "UTC";

/// `UPPER("workspaces"."name"::text) LIKE UPPER(:search)` — DRF
/// `SearchFilter` over `search_fields = ["name"]` compiles `icontains`
/// to `UPPER`-`LIKE`, not `ILIKE` (verified live). The handler wraps the
/// raw `?search=` term in `%...%`; escaping (`\\`, `%`, `_`) is the
/// caller's job, exactly as DRF does it.
pub fn search_name_sql() -> String {
    "UPPER(workspaces.name::TEXT) LIKE UPPER(:search)".to_owned()
}

/// `owner_id = :owner` — `filterset_fields = ["owner"]` exact match
/// (`:61`, `:206`).
pub fn filterset_owner_sql() -> String {
    "workspaces.owner_id = :owner".to_owned()
}

/// The two list paths and the fields each filter backend consults, in
/// source order (`:60-61`, `:205-206`).
pub const LIST_SEARCH_FIELDS: &[&str] = &["name"];
/// Exact-match filterset fields shared by both list paths.
pub const LIST_FILTERSET_FIELDS: &[&str] = &["owner"];

// ---------------------------------------------------------------------------
// R1/R2 shared: member-count + membership scope
// ---------------------------------------------------------------------------

/// `total_members` scalar subquery (`:66-71`, `:211-216`): live,
/// active, non-bot members of the outer workspace. `.order_by()` clears
/// the `workspace_members` default ordering, so there is no `ORDER BY`.
/// Django spells the function `Count` (`Func(function="Count")`);
/// Postgres folds it to `count` — same function, conventional casing.
/// No `GROUP BY`: `COUNT` over zero rows is `0`, never `NULL` (bug 7).
pub fn member_count_sql() -> String {
    "SELECT COUNT(wm.id) FROM workspace_members wm \
     INNER JOIN users m ON (wm.member_id = m.id) \
     WHERE wm.deleted_at IS NULL AND wm.is_active AND NOT m.is_bot \
     AND wm.workspace_id = workspaces.id"
        .to_owned()
}

/// Membership scope join (`:76-79`, `:230`): the reverse-FK filter
/// becomes an `INNER JOIN`. No `deleted_at` guard — Django never scopes
/// join tables (see module docs).
pub const MEMBERSHIP_JOIN_SQL: &str =
    "INNER JOIN workspace_members ON (workspaces.id = workspace_members.workspace_id)";

/// Membership scope predicate: the caller's live membership row.
/// `:user` is the request user id (UUID).
pub fn membership_where_sql() -> String {
    "workspace_members.member_id = :user AND workspace_members.is_active".to_owned()
}

/// `select_related("owner")` (`:74`): `owner` is non-nullable, so Django
/// emits `INNER JOIN` (verified live). `users` has no `deleted_at`.
pub const OWNER_JOIN_SQL: &str = "INNER JOIN users ON (workspaces.owner_id = users.id)";

/// R1 effective ordering: explicit `order_by("name")` (`:75`) overrides
/// the model default (`-created_at`, `db/models/workspace.py:182`).
pub const WORKSPACE_NAME_ORDER_SQL: &str = "workspaces.name ASC";

/// `Workspace` model default ordering (`db/models/workspace.py:182`),
/// effective on R2, which sets no explicit order.
pub const WORKSPACE_DEFAULT_ORDER_SQL: &str = "workspaces.created_at DESC";

// ---------------------------------------------------------------------------
// R1: WorkSpaceViewSet.get_queryset (:65-81)
// ---------------------------------------------------------------------------

/// R1 `WHERE` assembly in Django predicate order: base scope, then the
/// `filter_queryset` conjuncts (`?search=` / `?owner=` — present only
/// when the corresponding query param is given), then the membership
/// scope. `filter_queryset` runs BEFORE the membership scope here (`:74`)
/// — unobservable in SQL (bug 3), but the assembly keeps source order.
pub fn workspace_list_where_sql(with_search: bool, with_owner: bool) -> String {
    let mut parts = vec!["workspaces.deleted_at IS NULL".to_owned()];
    if with_search {
        parts.push(search_name_sql());
    }
    if with_owner {
        parts.push(filterset_owner_sql());
    }
    parts.push(membership_where_sql());
    parts.join(" AND ")
}

// ---------------------------------------------------------------------------
// R2: UserWorkSpacesEndpoint.get (:209-240)
// ---------------------------------------------------------------------------

/// `role` scalar subquery (`:218-220`): the caller's role on the outer
/// workspace, `NULL` when they hold no live row (the `:230` membership
/// filter guarantees a row in practice). Default ordering NOT cleared —
/// the dead `ORDER BY` is ported verbatim (bug 4).
pub fn workspace_role_sql() -> String {
    "SELECT wm2.role FROM workspace_members wm2 \
     WHERE wm2.deleted_at IS NULL AND wm2.is_active \
     AND wm2.member_id = :user AND wm2.workspace_id = workspaces.id \
     ORDER BY wm2.created_at DESC"
        .to_owned()
}

/// R2 `WHERE` assembly: base scope + membership scope first, then the
/// `filter_queryset` conjuncts (`:235` runs AFTER `distinct()` — bug 3).
pub fn user_workspaces_where_sql(with_search: bool, with_owner: bool) -> String {
    let mut parts = vec![
        "workspaces.deleted_at IS NULL".to_owned(),
        membership_where_sql(),
    ];
    if with_search {
        parts.push(search_name_sql());
    }
    if with_owner {
        parts.push(filterset_owner_sql());
    }
    parts.join(" AND ")
}

/// R2 selects `DISTINCT` (`:231`); R1 does not. The join fanout is
/// one-live-row-per-member in practice (partial unique
/// `workspace_member_unique_workspace_member_when_deleted_at_null`), so
/// `DISTINCT` is nearly a no-op — ported, not judged.
pub const USER_WORKSPACES_DISTINCT: bool = true;

/// R2 annotations in source order (`:229`).
pub const USER_WORKSPACES_ANNOTATIONS: &[&str] = &["role", "total_members"];

/// R2 member-prefetch stable predicates (`:223-228`): the `Prefetch` runs
/// as a second query over `WorkspaceMember.objects` — manager-scoped
/// (`deleted_at IS NULL`, unlike the join scope), filtered to the
/// requester's live row, default-ordered (`-created_at`). Django appends
/// the `workspace_id IN (...)` batch key at execution (handler machinery,
/// not pinned).
pub fn member_prefetch_where_sql() -> String {
    "workspace_members.deleted_at IS NULL AND workspace_members.is_active \
     AND workspace_members.member_id = :user"
        .to_owned()
}

// ---------------------------------------------------------------------------
// R3: dashboard bundle (:262-348)
// ---------------------------------------------------------------------------

/// `Issue.issue_objects` manager exclusions (`db/models/issue.py:95-104`)
/// as Django renders them: `exclude(state__group=TRIAGE)` keeps
/// null-group rows via the `IS NOT NULL` guard; archived issues,
/// archived projects, and drafts are dropped. Every R3 issue query
/// carries all four (Q3-Q9); Q1 reads activities, not issues.
pub fn issue_manager_where_sql() -> String {
    "issues.deleted_at IS NULL \
     AND NOT (\"states\".\"group\" = 'triage' AND \"states\".\"group\" IS NOT NULL) \
     AND NOT (issues.archived_at IS NOT NULL) \
     AND NOT (projects.archived_at IS NOT NULL) \
     AND NOT (issues.is_draft)"
        .to_owned()
}

/// R3 issue joins in Django's join order: `states` is `LEFT OUTER`
/// (`state` nullable); `projects`, `issue_assignees` (M2M), `workspaces`
/// are `INNER`. No `deleted_at` guard on any joined table.
pub const ISSUE_TENANT_JOINS_SQL: &str = "LEFT OUTER JOIN states ON (issues.state_id = states.id) \
     INNER JOIN projects ON (issues.project_id = projects.id) \
     INNER JOIN issue_assignees ON (issues.id = issue_assignees.issue_id) \
     INNER JOIN workspaces ON (issues.workspace_id = workspaces.id)";

/// R3 tenant predicate: workspace slug + `assignees__in=[request.user]`
/// (`issue_assignees.assignee_id IN (:user)` over the M2M join).
pub fn issue_tenant_where_sql() -> String {
    "issue_assignees.assignee_id IN (:user) AND workspaces.slug = :slug".to_owned()
}

/// `Issue` model default ordering, effective on Q8/Q9 (no explicit
/// order there).
pub const ISSUE_DEFAULT_ORDER_SQL: &str = "issues.created_at DESC";

/// `IssueActivity` model default ordering, effective on R5 (no explicit
/// order there).
pub const ACTIVITY_DEFAULT_ORDER_SQL: &str = "issue_activities.created_at DESC";

/// Q1 select (`:270-272`): `Cast("created_at", DateField())` renders a
/// `::date` cast (no time-zone shift on the cast itself), and the count
/// is over the cast expression — `created_at` is non-nullable so it
/// equals `COUNT(*)` row for row.
pub const DASHBOARD_Q1_SELECT_SQL: &str =
    "(issue_activities.created_at)::DATE AS created_date, COUNT((issue_activities.created_at)::DATE) AS activity_count";

/// Q1 join (`:267`): `workspace__slug` over the non-nullable FK renders
/// `INNER JOIN` (verified live). The `actor` predicate needs no join.
pub const DASHBOARD_Q1_JOIN_SQL: &str =
    "INNER JOIN workspaces ON (issue_activities.workspace_id = workspaces.id)";

/// Q1 `WHERE`: actor + workspace slug + `created_at__date__gte`. The
/// `__date` lookup renders `(created_at AT TIME ZONE :tzname)::DATE`
/// (`USE_TZ`; `:tzname` is the request user's zone — see the module
/// docs). `:from_date` is bound to
/// `date.today() + relativedelta(months=-3)` — a NAIVE server-local
/// date, not UTC-derived; ported as computed, not as "three months ago".
pub fn dashboard_q1_where_sql() -> String {
    "issue_activities.deleted_at IS NULL AND issue_activities.actor_id = :user \
     AND (issue_activities.created_at AT TIME ZONE :tzname)::DATE >= :from_date \
     AND workspaces.slug = :slug"
        .to_owned()
}

/// Q1 grouping + ordering (`:271-273`).
pub const DASHBOARD_Q1_GROUP_ORDER_SQL: &str = "GROUP BY 1 ORDER BY 1 ASC";

/// `?month=` default (`:276`): the int `1`. With no `?month=` every
/// bucket is January's — ported (bug 1). When provided, Django coerces
/// the string to int for `EXTRACT(MONTH ...) = :month`.
pub const DASHBOARD_MONTH_DEFAULT: i32 = 1;

/// `WeekInMonth` (`:257-259`): `(((day - 1) / 7) + 1)::INTEGER`.
/// `EXTRACT(DAY ...)` is `numeric`, so `/ 7` is exact division and the
/// `::INTEGER` cast truncates — buckets 1-5 for days 1-31. The day
/// extraction is time-zone shifted (`AT TIME ZONE :tzname`, same as every
/// datetime part extraction under `USE_TZ`; `:tzname` is the request
/// user's zone — see the module docs).
pub fn week_in_month_sql() -> String {
    "(((EXTRACT(DAY FROM issues.completed_at AT TIME ZONE :tzname) - 1) / 7) + 1)::INTEGER"
        .to_owned()
}

/// Q2 select (`:285-288`): week bucket + `COUNT(issues.id)`.
pub fn dashboard_q2_select_sql() -> String {
    format!(
        "{} AS week_in_month, COUNT(issues.id) AS completed_count",
        week_in_month_sql()
    )
}

/// Q2 `WHERE`: tenant scope + `completed_at__month=:month` (time-zone
/// shifted like every datetime extraction; `:tzname` is the request
/// user's zone — see the module docs) + `completed_at NOT NULL`.
/// The month predicate binds `:month` (default [`DASHBOARD_MONTH_DEFAULT`]).
pub fn dashboard_q2_where_sql() -> String {
    format!(
        "{} AND {} AND issues.completed_at IS NOT NULL \
         AND EXTRACT(MONTH FROM issues.completed_at AT TIME ZONE :tzname) = :month",
        issue_manager_where_sql(),
        issue_tenant_where_sql()
    )
}

/// Q2 grouping + ordering (`:287-289`).
pub const DASHBOARD_Q2_GROUP_ORDER_SQL: &str = "GROUP BY 1 ORDER BY 1 ASC";

/// Q3 `WHERE` — assigned count (`:292`): tenant scope only.
pub fn dashboard_assigned_where_sql() -> String {
    format!(
        "{} AND {}",
        issue_manager_where_sql(),
        issue_tenant_where_sql()
    )
}

/// `NOT ("states"."group" IN ('completed','cancelled') AND
/// "states"."group" IS NOT NULL)` — `~Q(state__group__in=CLOSED)` keeps
/// null-group rows via the guard (same shape as the triage exclusion).
pub fn open_state_group_sql() -> String {
    "NOT (\"states\".\"group\" IN ('completed', 'cancelled') AND \"states\".\"group\" IS NOT NULL)"
        .to_owned()
}

/// Q4 `WHERE` — pending count (`:294-298`): tenant scope + open groups.
pub fn dashboard_pending_where_sql() -> String {
    format!(
        "{} AND {} AND {}",
        issue_manager_where_sql(),
        open_state_group_sql(),
        issue_tenant_where_sql()
    )
}

/// Q5 `WHERE` — completed count (`:300-302`): tenant scope + the literal
/// `state__group="completed"`. `cancelled` is in NEITHER Q4 nor Q5
/// (bug 2).
pub fn dashboard_completed_where_sql() -> String {
    format!(
        "{} AND \"states\".\"group\" = 'completed' AND {}",
        issue_manager_where_sql(),
        issue_tenant_where_sql()
    )
}

/// Q6 predicate (`:304-309`): `ExtractWeek("target_date")` — a plain
/// `EXTRACT(WEEK ...)` with no time-zone shift (`target_date` is a
/// `DateField`) — equals the CURRENT ISO week number. `:iso_week` binds
/// `timezone.now().date().isocalendar()[1]` (UTC). `NULL` target dates
/// drop out (`EXTRACT` of `NULL` is `NULL`, never `=`). Year is ignored
/// on both sides: week 52 matches week 52 of any year — ported as-is.
pub fn due_week_predicate_sql() -> String {
    "EXTRACT(WEEK FROM issues.target_date) = :iso_week".to_owned()
}

/// Q6 `WHERE` — due-this-week count: tenant scope + week predicate.
pub fn dashboard_due_week_where_sql() -> String {
    format!(
        "{} AND {} AND {}",
        issue_manager_where_sql(),
        issue_tenant_where_sql(),
        due_week_predicate_sql()
    )
}

/// Q7 select (`:313-315`): group key + `COUNT("states"."group")` — the
/// count is over the group column, exactly as Django emits it.
pub const DASHBOARD_Q7_SELECT_SQL: &str =
    "\"states\".\"group\" AS state_group, COUNT(\"states\".\"group\") AS state_count";

/// Q7 `WHERE` — state distribution (`:311-312`): tenant scope only.
pub fn dashboard_state_dist_where_sql() -> String {
    format!(
        "{} AND {}",
        issue_manager_where_sql(),
        issue_tenant_where_sql()
    )
}

/// Q7 grouping + ordering (`:314-316`).
pub const DASHBOARD_Q7_GROUP_ORDER_SQL: &str = "GROUP BY 1 ORDER BY 1 ASC";

/// Q8 `WHERE` — overdue (`:319-325`): open groups + tenant scope +
/// `completed_at IS NULL` + `target_date < :today`. `:today` binds the
/// UTC date (`timezone.now().date()` — the datetime is truncated to a
/// date by the `DateField` lookup, bug 6).
pub fn overdue_where_sql() -> String {
    format!(
        "{} AND {} AND {} AND issues.completed_at IS NULL AND issues.target_date < :today",
        issue_manager_where_sql(),
        open_state_group_sql(),
        issue_tenant_where_sql()
    )
}

/// Q8 `.values()` projection in source order (`:325`):
/// `workspace__slug` joins `workspaces` (already in [`ISSUE_TENANT_JOINS_SQL`]).
pub const OVERDUE_VALUES_FIELDS: &[&str] =
    &["id", "name", "workspace__slug", "project_id", "target_date"];

/// Q9 `WHERE` — upcoming (`:327-333`): open groups + tenant scope +
/// `completed_at IS NULL` + `start_date >= :today` (same truncation, bug 6).
pub fn upcoming_where_sql() -> String {
    format!(
        "{} AND {} AND {} AND issues.completed_at IS NULL AND issues.start_date >= :today",
        issue_manager_where_sql(),
        open_state_group_sql(),
        issue_tenant_where_sql()
    )
}

/// Q9 `.values()` projection in source order (`:333`).
pub const UPCOMING_VALUES_FIELDS: &[&str] =
    &["id", "name", "workspace__slug", "project_id", "start_date"];

/// Dashboard response keys in source order (`:336-346`). Q3-Q6 collapse
/// to `_count` scalars at the handler; the queryset layer only owns the
/// `COUNT(*)` shapes above (`.count()` clears ordering — no `ORDER BY`
/// on Q3-Q6).
pub const DASHBOARD_RESPONSE_KEYS: &[&str] = &[
    "issue_activities",
    "completed_issues",
    "assigned_issues_count",
    "pending_issues_count",
    "completed_issues_count",
    "issues_due_week_count",
    "state_distribution",
    "overdue_issues",
    "upcoming_issues",
];

// ---------------------------------------------------------------------------
// R4: themes (:351-365)
// ---------------------------------------------------------------------------

/// Theme list join + `WHERE`: slug scope over an `INNER JOIN` to
/// `workspaces` (`:356-357`). No `select_related`, no ordering override
/// — the model default (`-created_at`) applies.
pub const THEME_JOIN_SQL: &str =
    "INNER JOIN workspaces ON (workspace_themes.workspace_id = workspaces.id)";

/// Theme list predicate: live rows for the slug workspace.
pub fn theme_where_sql() -> String {
    "workspace_themes.deleted_at IS NULL AND workspaces.slug = :slug".to_owned()
}

/// `WorkspaceTheme` model default ordering (effective on the list path).
pub const THEME_DEFAULT_ORDER_SQL: &str = "workspace_themes.created_at DESC";

/// Create lookup (`:360`): `Workspace.objects.get(slug=slug)` — live
/// workspace by slug; single-row `.get()` semantics (missing slug raises
/// at the handler — queryset layer owns the predicate only).
pub fn workspace_by_slug_where_sql() -> String {
    "workspaces.deleted_at IS NULL AND workspaces.slug = :slug".to_owned()
}

// ---------------------------------------------------------------------------
// R5: export-CSV query (:379-390)
// ---------------------------------------------------------------------------

/// Fields excluded from the export (`:384`): `~Q(field__in=[...])`.
pub const EXPORT_EXCLUDED_FIELDS: &[&str] = &["comment", "vote", "reaction", "draft"];

/// `NOT (field IN (...) AND field IS NOT NULL)` — Django's `~Q` keeps
/// null-`field` rows via the guard (verified live); only the four named
/// values are dropped.
pub fn export_field_predicate_sql() -> String {
    "NOT (issue_activities.field IN ('comment', 'vote', 'reaction', 'draft') \
     AND issue_activities.field IS NOT NULL)"
        .to_owned()
}

/// R5 joins in Django's join order (`:387-390`): actor/project/members/
/// workspace `INNER`, issue `LEFT OUTER` (nullable, unconstrained).
/// The actor join is demoted from `LEFT` to `INNER` because the query
/// constrains `actor_id = :user_id` — equivalent here (null actors are
/// already excluded), ported exactly as compiled. No `deleted_at` guard
/// on any joined table.
pub const EXPORT_JOINS_SQL: &str = "INNER JOIN users ON (issue_activities.actor_id = users.id) \
     INNER JOIN projects ON (issue_activities.project_id = projects.id) \
     INNER JOIN project_members ON (projects.id = project_members.project_id) \
     INNER JOIN workspaces ON (issue_activities.workspace_id = workspaces.id) \
     LEFT OUTER JOIN issues ON (issue_activities.issue_id = issues.id)";

/// R5 `WHERE` in Django predicate order (`:383-389`): base scope, field
/// exclusion, actor, `created_at__date=:date` (time-zone shifted like
/// Q1; `:tzname` is the request user's zone — see the module docs),
/// requester's live project membership, workspace slug. NOTE: no
/// `projects.archived_at` filter here, unlike the user-activity query in
/// `profile.sql` R4 — ported as-is. `:user` is the REQUESTER (project
/// membership); `:user_id` is the export target (actor).
pub fn export_where_sql() -> String {
    format!(
        "issue_activities.deleted_at IS NULL AND {} \
         AND issue_activities.actor_id = :user_id \
         AND (issue_activities.created_at AT TIME ZONE :tzname)::DATE = :date \
         AND project_members.is_active AND project_members.member_id = :user \
         AND workspaces.slug = :slug",
        export_field_predicate_sql()
    )
}

/// `[:10000]` cap (`:390`): limit, no offset. Ordering is the model
/// default ([`ACTIVITY_DEFAULT_ORDER_SQL`]) — no explicit `order_by`.
pub const EXPORT_LIMIT: i64 = 10000;

/// Full export statement shape for the oracle: joins + where + default
/// order + cap. Column lists are the handler's (`select_related` pulls
/// full joined rows); the queryset layer pins everything else.
pub fn export_statement_shape_sql() -> String {
    let mut out = String::new();
    let _ = write!(
        out,
        "SELECT ... FROM issue_activities {} WHERE {} ORDER BY {} LIMIT {}",
        EXPORT_JOINS_SQL,
        export_where_sql(),
        ACTIVITY_DEFAULT_ORDER_SQL,
        EXPORT_LIMIT
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE_ROWS: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/app_workspace/queries/core.rows.json"
    );

    fn rows_fixture() -> serde_json::Value {
        let raw = std::fs::read_to_string(FIXTURE_ROWS).expect("fixture exists");
        serde_json::from_str(&raw).expect("fixture is valid JSON")
    }

    fn bugs(fixture: &serde_json::Value) -> Vec<String> {
        fixture["bugs"]
            .as_array()
            .expect("bugs array")
            .iter()
            .map(|b| b.as_str().expect("bug string").to_owned())
            .collect()
    }

    #[test]
    fn fixture_oracle_covers_all_five_units() {
        let fixture = rows_fixture();
        assert_eq!(
            fixture["source"].as_str().expect("source"),
            "app/views/workspace/base.py:65-81,:204-240,:257-390"
        );
        // 8 rows: list annotation, Q1, Q2, Q3-Q6 scalars, Q7, Q8, R4, R5.
        assert_eq!(fixture["rows"].as_array().expect("rows").len(), 8);
        assert_eq!(
            fixture["rows_excluded"].as_array().expect("excluded").len(),
            5
        );
        // Every rows_excluded guard maps to a predicate in this module.
        let excluded = fixture["rows_excluded"].as_array().expect("excluded");
        let whys: Vec<&str> = excluded
            .iter()
            .map(|r| r["why"].as_str().expect("why"))
            .collect();
        assert!(whys.iter().any(|w| w.contains("is_active")));
        assert!(whys.iter().any(|w| w.contains("is_bot")));
        assert!(whys.iter().any(|w| w.contains("completed")));
        assert!(whys.iter().any(|w| w.contains("ExtractWeek")));
        assert!(whys.iter().any(|w| w.contains("field__in")));
    }

    #[test]
    fn member_count_matches_django_shape() {
        let sql = member_count_sql();
        assert!(sql.contains("SELECT COUNT(wm.id) FROM workspace_members wm"));
        assert!(sql.contains("INNER JOIN users m ON (wm.member_id = m.id)"));
        assert!(sql.contains("wm.deleted_at IS NULL"));
        assert!(sql.contains("wm.is_active"));
        assert!(sql.contains("NOT m.is_bot"));
        assert!(sql.contains("wm.workspace_id = workspaces.id"));
        // No GROUP BY: COUNT over zero rows is 0, never NULL (bug 7 —
        // the fixture's "NULL when zero rows" note is wrong; live SQL wins).
        assert!(!sql.contains("GROUP BY"));
        assert!(!sql.to_uppercase().contains("ORDER BY"));
    }

    #[test]
    fn membership_scope_has_no_deleted_guard() {
        assert!(MEMBERSHIP_JOIN_SQL.contains("INNER JOIN workspace_members"));
        let where_sql = membership_where_sql();
        assert!(where_sql.contains("workspace_members.member_id = :user"));
        assert!(where_sql.contains("workspace_members.is_active"));
        // Ported Django behavior: filter() joins are never manager-scoped.
        assert!(!where_sql.contains("deleted_at"));
        assert!(!MEMBERSHIP_JOIN_SQL.contains("deleted_at"));
    }

    #[test]
    fn r1_where_orders_filter_before_membership() {
        let sql = workspace_list_where_sql(true, true);
        assert!(sql.starts_with("workspaces.deleted_at IS NULL"));
        let search = sql.find("UPPER(workspaces.name").expect("search");
        let member = sql.find("workspace_members.member_id").expect("membership");
        assert!(search < member, "filter_queryset before scope in R1: {sql}");
        assert!(sql.contains("workspaces.owner_id = :owner"));
        // Absent params drop their conjuncts.
        let bare = workspace_list_where_sql(false, false);
        assert!(!bare.contains("UPPER("));
        assert!(!bare.contains("owner_id"));
        assert_eq!(WORKSPACE_NAME_ORDER_SQL, "workspaces.name ASC");
        assert!(OWNER_JOIN_SQL.starts_with("INNER JOIN users"));
        assert!(!OWNER_JOIN_SQL.contains("deleted_at"));
    }

    #[test]
    fn search_renders_upper_like_not_ilike() {
        assert_eq!(
            search_name_sql(),
            "UPPER(workspaces.name::TEXT) LIKE UPPER(:search)"
        );
        assert_eq!(LIST_SEARCH_FIELDS, &["name"]);
        assert_eq!(LIST_FILTERSET_FIELDS, &["owner"]);
    }

    #[test]
    fn r2_where_orders_filter_after_membership() {
        let sql = user_workspaces_where_sql(true, true);
        let member = sql.find("workspace_members.member_id").expect("membership");
        let search = sql.find("UPPER(workspaces.name").expect("search");
        assert!(member < search, "filter_queryset after scope in R2: {sql}");
        const { assert!(USER_WORKSPACES_DISTINCT) };
        assert_eq!(USER_WORKSPACES_ANNOTATIONS, &["role", "total_members"]);
        assert_eq!(WORKSPACE_DEFAULT_ORDER_SQL, "workspaces.created_at DESC");
    }

    #[test]
    fn role_subquery_keeps_dead_order_by() {
        let sql = workspace_role_sql();
        assert!(sql.contains("SELECT wm2.role FROM workspace_members wm2"));
        assert!(sql.contains("wm2.deleted_at IS NULL"));
        assert!(sql.contains("wm2.member_id = :user"));
        assert!(sql.contains("ORDER BY wm2.created_at DESC"));
    }

    #[test]
    fn member_prefetch_where_is_manager_scoped() {
        // The Prefetch is a second query over WorkspaceMember.objects, so
        // unlike the join scope it carries the manager's deleted_at guard.
        let sql = member_prefetch_where_sql();
        assert!(sql.contains("workspace_members.deleted_at IS NULL"));
        assert!(sql.contains("workspace_members.is_active"));
        assert!(sql.contains("workspace_members.member_id = :user"));
    }

    #[test]
    fn issue_manager_exclusions_match_django() {
        let sql = issue_manager_where_sql();
        assert!(sql.contains("issues.deleted_at IS NULL"));
        assert!(sql.contains(
            "NOT (\"states\".\"group\" = 'triage' AND \"states\".\"group\" IS NOT NULL)"
        ));
        assert!(sql.contains("NOT (issues.archived_at IS NOT NULL)"));
        assert!(sql.contains("NOT (projects.archived_at IS NOT NULL)"));
        assert!(sql.contains("NOT (issues.is_draft)"));
        assert_eq!(TRIAGE_GROUP, "triage");
        assert_eq!(CLOSED_STATE_GROUPS, &["completed", "cancelled"]);
    }

    #[test]
    fn issue_tenant_joins_use_exact_join_types() {
        assert!(ISSUE_TENANT_JOINS_SQL
            .contains("LEFT OUTER JOIN states ON (issues.state_id = states.id)"));
        assert!(ISSUE_TENANT_JOINS_SQL
            .contains("INNER JOIN projects ON (issues.project_id = projects.id)"));
        assert!(ISSUE_TENANT_JOINS_SQL
            .contains("INNER JOIN issue_assignees ON (issues.id = issue_assignees.issue_id)"));
        assert!(ISSUE_TENANT_JOINS_SQL
            .contains("INNER JOIN workspaces ON (issues.workspace_id = workspaces.id)"));
        assert!(!ISSUE_TENANT_JOINS_SQL.contains("deleted_at"));
        let tenant = issue_tenant_where_sql();
        assert!(tenant.contains("issue_assignees.assignee_id IN (:user)"));
        assert!(tenant.contains("workspaces.slug = :slug"));
    }

    #[test]
    fn dashboard_q1_casts_and_shifts_like_django() {
        assert!(
            DASHBOARD_Q1_SELECT_SQL.contains("(issue_activities.created_at)::DATE AS created_date")
        );
        assert!(DASHBOARD_Q1_SELECT_SQL
            .contains("COUNT((issue_activities.created_at)::DATE) AS activity_count"));
        let where_sql = dashboard_q1_where_sql();
        assert!(where_sql.contains("issue_activities.deleted_at IS NULL"));
        assert!(where_sql.contains("issue_activities.actor_id = :user"));
        assert!(where_sql
            .contains("(issue_activities.created_at AT TIME ZONE :tzname)::DATE >= :from_date"));
        assert!(where_sql.contains("workspaces.slug = :slug"));
        assert_eq!(DASHBOARD_Q1_GROUP_ORDER_SQL, "GROUP BY 1 ORDER BY 1 ASC");
        assert_eq!(DJANGO_TIME_ZONE_SQL, "UTC");
        // Fixture Q1 row keys.
        let fixture = rows_fixture();
        let q1 = &fixture["rows"][1];
        assert!(q1.get("created_date").is_some());
        assert!(q1.get("activity_count").is_some());
    }

    #[test]
    fn dashboard_q1_join_scopes_workspace_slug() {
        // workspace__slug needs this INNER JOIN; the actor predicate does not.
        assert_eq!(
            DASHBOARD_Q1_JOIN_SQL,
            "INNER JOIN workspaces ON (issue_activities.workspace_id = workspaces.id)"
        );
        assert!(dashboard_q1_where_sql().contains("workspaces.slug = :slug"));
    }

    #[test]
    fn dashboard_q2_buckets_week_in_month() {
        assert_eq!(
            week_in_month_sql(),
            "(((EXTRACT(DAY FROM issues.completed_at AT TIME ZONE :tzname) - 1) / 7) + 1)::INTEGER"
        );
        let select = dashboard_q2_select_sql();
        assert!(select.contains("AS week_in_month"));
        assert!(select.contains("COUNT(issues.id) AS completed_count"));
        let where_sql = dashboard_q2_where_sql();
        assert!(where_sql.contains("issues.completed_at IS NOT NULL"));
        assert!(where_sql
            .contains("EXTRACT(MONTH FROM issues.completed_at AT TIME ZONE :tzname) = :month"));
        assert!(where_sql.contains("\"states\".\"group\" = 'triage'"));
        assert_eq!(DASHBOARD_MONTH_DEFAULT, 1);
        assert_eq!(DASHBOARD_Q2_GROUP_ORDER_SQL, "GROUP BY 1 ORDER BY 1 ASC");
        let fixture = rows_fixture();
        let q2 = &fixture["rows"][2];
        assert!(q2.get("week_in_month").is_some());
        assert!(q2.get("completed_count").is_some());
    }

    #[test]
    fn dashboard_counts_split_open_completed() {
        let assigned = dashboard_assigned_where_sql();
        assert!(!assigned.contains("completed"));
        assert!(!assigned.contains("cancelled"));
        let pending = dashboard_pending_where_sql();
        assert!(pending.contains(&open_state_group_sql()));
        assert!(pending.contains("'completed', 'cancelled'"));
        let completed = dashboard_completed_where_sql();
        // Literal equality — cancelled excluded (bug 2).
        assert!(completed.contains("\"states\".\"group\" = 'completed'"));
        assert!(!completed.contains("cancelled"));
        let fixture = rows_fixture();
        let scalars = &fixture["rows"][3];
        for key in [
            "assigned_issues_count",
            "pending_issues_count",
            "completed_issues_count",
            "issues_due_week_count",
        ] {
            assert!(scalars.get(key).is_some(), "scalar {key}");
        }
    }

    #[test]
    fn dashboard_due_week_uses_plain_extract_week() {
        assert_eq!(
            due_week_predicate_sql(),
            "EXTRACT(WEEK FROM issues.target_date) = :iso_week"
        );
        // No time-zone shift on a DateField extraction.
        assert!(!due_week_predicate_sql().contains("AT TIME ZONE"));
        let where_sql = dashboard_due_week_where_sql();
        assert!(where_sql.contains("issue_assignees.assignee_id IN (:user)"));
    }

    #[test]
    fn dashboard_q7_counts_group_column() {
        assert_eq!(
            DASHBOARD_Q7_SELECT_SQL,
            "\"states\".\"group\" AS state_group, COUNT(\"states\".\"group\") AS state_count"
        );
        let where_sql = dashboard_state_dist_where_sql();
        assert!(!where_sql.contains("completed"));
        assert_eq!(DASHBOARD_Q7_GROUP_ORDER_SQL, "GROUP BY 1 ORDER BY 1 ASC");
        let fixture = rows_fixture();
        let q7 = &fixture["rows"][4];
        assert!(q7.get("state_group").is_some());
        assert!(q7.get("state_count").is_some());
    }

    #[test]
    fn overdue_upcoming_use_truncated_today() {
        let overdue = overdue_where_sql();
        assert!(overdue.contains(&open_state_group_sql()));
        assert!(overdue.contains("issues.completed_at IS NULL"));
        assert!(overdue.contains("issues.target_date < :today"));
        assert_eq!(
            OVERDUE_VALUES_FIELDS,
            &["id", "name", "workspace__slug", "project_id", "target_date"]
        );
        let upcoming = upcoming_where_sql();
        assert!(upcoming.contains("issues.start_date >= :today"));
        assert_eq!(
            UPCOMING_VALUES_FIELDS,
            &["id", "name", "workspace__slug", "project_id", "start_date"]
        );
        assert_eq!(ISSUE_DEFAULT_ORDER_SQL, "issues.created_at DESC");
        let fixture = rows_fixture();
        let q8 = &fixture["rows"][5];
        for key in ["id", "name", "project_id", "target_date", "workspace__slug"] {
            assert!(q8.get(key).is_some(), "overdue key {key}");
        }
    }

    #[test]
    fn dashboard_response_keys_match_source_order() {
        assert_eq!(
            DASHBOARD_RESPONSE_KEYS,
            &[
                "issue_activities",
                "completed_issues",
                "assigned_issues_count",
                "pending_issues_count",
                "completed_issues_count",
                "issues_due_week_count",
                "state_distribution",
                "overdue_issues",
                "upcoming_issues",
            ]
        );
    }

    #[test]
    fn theme_queries_scope_slug_only() {
        assert!(THEME_JOIN_SQL
            .contains("INNER JOIN workspaces ON (workspace_themes.workspace_id = workspaces.id)"));
        assert_eq!(
            theme_where_sql(),
            "workspace_themes.deleted_at IS NULL AND workspaces.slug = :slug"
        );
        assert_eq!(THEME_DEFAULT_ORDER_SQL, "workspace_themes.created_at DESC");
        assert_eq!(
            workspace_by_slug_where_sql(),
            "workspaces.deleted_at IS NULL AND workspaces.slug = :slug"
        );
        let fixture = rows_fixture();
        let theme = &fixture["rows"][6];
        assert!(theme.get("name").is_some());
        assert!(theme.get("workspace__slug").is_some());
    }

    #[test]
    fn export_where_matches_django_predicate_order() {
        assert_eq!(
            EXPORT_EXCLUDED_FIELDS,
            &["comment", "vote", "reaction", "draft"]
        );
        let field = export_field_predicate_sql();
        assert!(field
            .contains("NOT (issue_activities.field IN ('comment', 'vote', 'reaction', 'draft')"));
        assert!(field.contains("issue_activities.field IS NOT NULL"));
        let where_sql = export_where_sql();
        // Requester (:user, project membership) vs export target (:user_id, actor).
        assert!(where_sql.contains("issue_activities.actor_id = :user_id"));
        assert!(where_sql.contains("project_members.member_id = :user"));
        assert!(where_sql.contains("project_members.is_active"));
        assert!(
            where_sql.contains("(issue_activities.created_at AT TIME ZONE :tzname)::DATE = :date")
        );
        assert!(where_sql.contains("workspaces.slug = :slug"));
        // No archived-project filter on this path (unlike profile R4).
        assert!(!where_sql.contains("archived_at"));
        assert_eq!(EXPORT_LIMIT, 10000);
        assert_eq!(
            ACTIVITY_DEFAULT_ORDER_SQL,
            "issue_activities.created_at DESC"
        );
    }

    #[test]
    fn export_joins_match_compiled_join_types() {
        assert!(
            EXPORT_JOINS_SQL.contains("INNER JOIN users ON (issue_activities.actor_id = users.id)")
        );
        assert!(EXPORT_JOINS_SQL
            .contains("INNER JOIN projects ON (issue_activities.project_id = projects.id)"));
        assert!(EXPORT_JOINS_SQL
            .contains("INNER JOIN project_members ON (projects.id = project_members.project_id)"));
        assert!(EXPORT_JOINS_SQL
            .contains("INNER JOIN workspaces ON (issue_activities.workspace_id = workspaces.id)"));
        assert!(EXPORT_JOINS_SQL
            .contains("LEFT OUTER JOIN issues ON (issue_activities.issue_id = issues.id)"));
        let shape = export_statement_shape_sql();
        assert!(shape.contains("ORDER BY issue_activities.created_at DESC"));
        assert!(shape.contains("LIMIT 10000"));
        let fixture = rows_fixture();
        let export = &fixture["rows"][7];
        for key in ["action", "actor", "field", "issue"] {
            assert!(export.get(key).is_some(), "export key {key}");
        }
        assert_ne!(export["field"].as_str(), Some("comment"));
    }

    #[test]
    fn list_annotation_row_keys_match_fixture() {
        let fixture = rows_fixture();
        let row = &fixture["rows"][0];
        assert_eq!(row["total_members"].as_i64(), Some(3));
        assert_eq!(row["role"].as_i64(), Some(20));
        assert!(row.get("slug").is_some());
        assert!(row.get("name").is_some());
    }

    #[test]
    fn ported_bugs_are_all_documented_in_fixture_or_here() {
        let fixture_bugs = bugs(&rows_fixture()).join("\n");
        // Bugs 1, 2, 3, 5 (month default, literal completed, filter order,
        // 10k cap) are recorded in the fixture; bugs 4, 6, 7 (dead ORDER
        // BY, date truncation, COUNT-vs-NULL) are grounded in live-compiled
        // SQL and documented in this module's header + PR.
        for needle in ["month default", "literal 'completed'", "BEFORE", "10000"] {
            assert!(fixture_bugs.contains(needle), "fixture notes {needle}");
        }
        assert!(workspace_role_sql().contains("ORDER BY"));
        assert!(overdue_where_sql().contains(":today"));
        assert!(!member_count_sql().contains("GROUP BY"));
    }
}
