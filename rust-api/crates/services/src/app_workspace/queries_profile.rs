#![forbid(unsafe_code)]

//! Workspace user-profile reads (D-24, stage 5): profile, stats bundle,
//! user issues, user activity, activity/completed graphs.
//!
//! Ports `apps/api/pi_dash/app/views/workspace/user.py:99-559`
//! (`WorkspaceUserProfileIssuesEndpoint`, `WorkspaceUserPropertiesEndpoint`,
//! `WorkspaceUserProfileEndpoint`, `WorkspaceUserActivityEndpoint`,
//! `WorkspaceUserProfileStatsEndpoint`, `UserActivityGraphEndpoint`,
//! `UserIssueCompletedGraphEndpoint`) plus the same-shape me-activities
//! route (`app/views/user/base.py:392-404`) and the dashboard `month`
//! cross-reference (`app/views/workspace/base.py:276,282`). The last-visited
//! endpoint (`user.py:69-96`) has no query here: it 500s on
//! `user.last_workspace_id` before any read (recorded in F-W24-15).
//!
//! SQL conventions (Porting guide data rules, same split as
//! `app_issues::ordering`): builders emit SQL text with Postgres `$n`
//! placeholders; the handlers issue (PIDASHCONV-618/620, api crate,
//! which owns sea-query/sqlx — this crate depends on neither) splices
//! the fragments and binds the values. Django spells placeholders `%s`;
//! `$n` is the driver-level translation, the predicates are unchanged.
//!
//! Join-shape rule (verified against the pinned Django 4.2.30 source,
//! `requirements/base.txt:4`, not just the ORM calls): `ON` clauses carry
//! only FK equalities and every condition sits in `WHERE`, exactly like
//! Django emits. Related managers NEVER scope a `filter()` join
//! (`Join.as_sql` renders `join_cols` only —
//! `django/db/models/sql/datastructures.py`; pilot-2 records the same rule
//! in `api/src/app_issues/mod.rs`): joined tables match live-or-deleted
//! rows unless an explicit condition says otherwise. Only the ROOT
//! queryset's manager applies (`Issue.issue_objects` scope,
//! `SoftDeletionManager`'s `deleted_at IS NULL`), plus explicit
//! conditions, plus the direct-manager subqueries in annotations.
//! Join fanout is load-bearing (counts/GROUPs over multi-valued joins
//! overcount exactly like Django); builders mirror Django's join shape
//! and never "optimize" a join into an `EXISTS`.
//!
//! Reuse map (merged code only — nothing is re-ported here):
//! - `crate::app_issues::order_sql` / `OrderSpec` via
//!   [`profile_issues_order`] (pins the `-created_at` default,
//!   `user.py:139`).
//! - `crate::app_issues::raw_group_mismatch` / `ParamError` via
//!   [`profile_group_mismatch`] (same guard as pilot-2, `user.py:176-182`).
//! - `crate::app_issues::{PRIORITY_VALUES, STATE_GROUP_VALUES}` for the
//!   static `issue_group_values` branches ([`profile_group_values`]).
//! - `crate::app_issues::ordering::PRIORITY_ORDER` for the stats priority
//!   `CASE` ([`priority_order_case_sql`]).
//! - `crate::app_workspace::models_prefs::workspace_user_properties` (`TABLE`, `COLUMNS`,
//!   `default_*`, scalar defaults) for the user-props get-or-create
//!   builders ([`user_props_lookup_sql`], [`user_props_insert_sql`]).
//! - `pidash_db::{filter, filterset, issue_filters}` (F-07): the
//!   `ComplexFilterBackend` + `IssueFilterSet` + legacy `issue_filters`
//!   layers. They compile in the handlers (api crate); every builder that
//!   sits under `**filters` takes the compiled fragment as `legacy_sql`
//!   and splices it as `AND ({legacy_sql})` (handler renumbers binds,
//!   Binder precedent). The pipeline order each builder assumes is
//!   documented on [`profile_issues_from_where`].
//! - Grouper + grouped/sub-grouped/flat pagination execute in the api
//!   crate (F-07 paginator kernels via the handlers issue); this module
//!   provides the fragments they need: [`profile_issues_annotations_sql`],
//!   [`grouped_count_filter_sql`] + [`grouped_count_filter_join`],
//!   [`profile_group_values`].
//!
//! Fixture: F-W24-11 (`rust-api/fixtures/app_workspace/queries/profile.sql`
//! and `profile.rows.json`, PIDASHCONV-599). Unit tests replay it: table
//! consts, WHERE fragments, GROUP/ORDER/HAVING shapes, response-key sets
//! (cross-checked against the contract suites
//! `contract-tests/app_workspace/test_workspace_extras.py::STATS_KEYS`,
//! `USER_DATA_KEYS`), and every body/status below.
//!
//! Ported bugs (translate, don't redesign — recorded here, fixed nowhere):
//! - No `permission_classes` on the profile/stats/graph endpoints
//!   (`user.py:281,397,524,541` — any authenticated caller; siblings are
//!   gated). Handler-layer; the contract suite pins it
//!   (`test_user_stats_non_member_open`, `test_dashboard_unknown_slug_*`).
//! - R3 project counts are ONE query with four `FILTER` aggregates over a
//!   shared multi-valued join (Django never splits annotations):
//!   `created_issues` overcounts by assignee-link fanout, deleted and
//!   triage issues count (joins carry no manager guard), duplicate live
//!   links double-count. [`profile_projects_sql`].
//! - Q2 `HAVING COUNT(*) >= 1` (`:427`) is always true (grouped counts are
//!   `>= 1` by construction) — emitted verbatim.
//! - R5 cycle queries take NO legacy filters and NO requester scope
//!   (`:496-507`); Q9's variable is singular `present_cycle` while the
//!   response key is plural `present_cycles` (`:502` vs `:518`).
//! - R6 reads SIX months (`:530`) while the dashboard twin reads three
//!   (`base.py:268`); R7 buckets `week % 4` (`:553`, buckets 0-3) while
//!   the dashboard uses `WeekInMonth` (`base.py:257-259`, buckets 1-5).
//! - `?month=` defaults to int `1` but arrives as `str` when present
//!   (`:543`, `base.py:276`); garbage months 500 via `ValueError`
//!   (not `ValidationError`, so not the 400 branch).
//!   [`parse_month_param`].
//! - `?project=` values on the activity route are raw strings (`:387`);
//!   non-UUID values 400 via `ValidationError`.
//!   [`validate_project_uuids`].
//! - `target_date`/`start_date`/`created_by` group values come from
//!   `SELECT DISTINCT col, created_at ... ORDER BY created_at DESC`
//!   (Django's `get_extra_select` appends the default-ordering column to
//!   satisfy Postgres, so values DUPLICATE and NULLs are kept):
//!   [`profile_group_values`].
//! - `assignees__id` group values (workspace scope, no `project_id`, no
//!   `"None"` sentinel) vs the `labels__id`/`issue_module__module_id`/
//!   `cycle_id` branches (which append `"None"`): [`profile_group_values`].
//!
//! Fixture deviation (Python + live contract test overrule the fixture):
//! - F-W24-11 R3 claims `User.objects.get` on a missing user 500s. The
//!   endpoint's `handle_exception` maps `ObjectDoesNotExist` (which
//!   `User.DoesNotExist` subclasses) to 404 (`app/views/base.py:132-136`),
//!   and the live contract test `test_user_profile_non_member_404` proves
//!   the `DoesNotExist` → 404 path on this same endpoint. The module
//!   encodes [`USER_GET_MISS_STATUS`] = 404.
//! - F-W24-11's "counts NULL when zero" row is likewise wrong: the R1
//!   `Func(..., function="Count")` annotations are scalar aggregates with
//!   no `GROUP BY`, so they yield 0 when empty (plain `COUNT` in
//!   [`profile_issues_annotations_sql`]). Fixture correction tracked by
//!   PIDASHCONV-700.
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.
//!
//! Pages read: Porting guide `4496e321-dd24-40f7-bfdf-f771e45fac0c`
//! (updated_at 2026-09-28T03:51:35.921141Z); PIDASHCONV-1 rulebook.

use chrono::Datelike;

use super::models_prefs::workspace_user_properties as wup;
use crate::app_issues::ordering::PRIORITY_ORDER;
use crate::app_issues::{
    order_sql, raw_group_mismatch, OrderSpec, ParamError, PRIORITY_VALUES, STATE_GROUP_VALUES,
};

// ---------------------------------------------------------------------------
// Table names (`Meta.db_table` per model; Django field spellings).
// ---------------------------------------------------------------------------

/// Physical tables this module reads. Each cites its `db_table` line;
/// column spellings are Django attnames (`state_id`, `assignee_id`, ...).
pub mod tables {
    /// `issues` (`db/models/issue.py:253`).
    pub const ISSUES: &str = "issues";
    /// `issue_assignees` (`db/models/issue.py:464`).
    pub const ISSUE_ASSIGNEES: &str = "issue_assignees";
    /// `issue_subscribers` (`db/models/issue.py:719`).
    pub const ISSUE_SUBSCRIBERS: &str = "issue_subscribers";
    /// `issue_activities` (`db/models/issue.py:542`).
    pub const ISSUE_ACTIVITIES: &str = "issue_activities";
    /// `issue_links` (`db/models/issue.py:480`).
    pub const ISSUE_LINKS: &str = "issue_links";
    /// `file_assets` (`db/models/asset.py`, `db_table`).
    pub const FILE_ASSETS: &str = "file_assets";
    /// `cycle_issues` (`db/models/cycle.py:123`).
    pub const CYCLE_ISSUES: &str = "cycle_issues";
    /// `cycles` (`db/models/cycle.py`, `db_table`).
    pub const CYCLES: &str = "cycles";
    /// `projects` (`db/models/project.py:252`).
    pub const PROJECTS: &str = "projects";
    /// `project_members` (`db/models/project.py:377`).
    pub const PROJECT_MEMBERS: &str = "project_members";
    /// `states` (`db/models/state.py:128`).
    pub const STATES: &str = "states";
    /// `workspaces` (`db/models/workspace.py`, `db_table`).
    pub const WORKSPACES: &str = "workspaces";
    /// `workspace_members` (`db/models/workspace.py`, `db_table`).
    pub const WORKSPACE_MEMBERS: &str = "workspace_members";
    /// `users` (`db/models/user.py`, `db_table`).
    pub const USERS: &str = "users";
    /// `intake_issues` (`db/models/intake.py`, `db_table`).
    pub const INTAKE_ISSUES: &str = "intake_issues";
    /// `labels` (`db/models/label.py:43`).
    pub const LABELS: &str = "labels";
    /// `modules` (`db/models/module.py:112`).
    pub const MODULES: &str = "modules";
}

use tables::*;

// ---------------------------------------------------------------------------
// Shared fragments.
// ---------------------------------------------------------------------------

/// `Issue.issue_objects` manager scope (`db/models/issue.py:95-104`):
/// `SoftDeletionManager` (`deleted_at IS NULL`) plus the triage, archived
/// (issue + project) and draft exclusions. `issue`/`state`/`project` are
/// the caller aliases for the `issues`, `states` (LEFT-joined) and
/// `projects` tables.
///
/// The triage exclusion keeps NULL-state rows: `state` is a nullable FK
/// and Django's `exclude(state__group=TRIAGE)` retains them via
/// `split_exclude`'s `IS NULL` disjunct; a bare
/// `NOT ("group" = 'triage')` over the left join would drop them
/// (three-valued logic). Same shape as pilot-2's `base_where`.
pub fn issue_manager_scope(issue: &str, state: &str, project: &str) -> String {
    format!(
        "{issue}.deleted_at IS NULL \
         AND ({state}.\"group\" IS NULL OR NOT ({state}.\"group\" = 'triage')) \
         AND {issue}.archived_at IS NULL \
         AND {project}.archived_at IS NULL \
         AND {issue}.is_draft = FALSE"
    )
}

/// Requester project-membership predicate
/// (`project__project_projectmember__member=request.user, ...__is_active=True`,
/// e.g. `user.py:146-147`). `members` is the caller's `project_members`
/// alias (INNER-joined on `project_id`), `viewer` the `$n` holder.
///
/// NO `deleted_at` guard: `filter()` joins never apply a related manager
/// (verified in Django 4.2.30 `Join.as_sql`, which renders `join_cols`
/// only). A deleted membership row still matches — ported as-is.
pub fn requester_membership_predicate(members: &str, viewer: &str) -> String {
    format!("{members}.member_id = {viewer} AND {members}.is_active = TRUE")
}

/// Splice an optional compiled legacy-fragment (`issue_filters`,
/// F-07 `pidash_db::issue_filters`) into a `WHERE` clause. `None`
/// renders nothing; `Some` renders `AND ({legacy_sql})`. The handler
/// renumbers the fragment's binds (Binder precedent).
fn and_legacy(legacy_sql: Option<&str>) -> String {
    match legacy_sql {
        Some(fragment) => format!(" AND ({fragment})"),
        None => String::new(),
    }
}

// ---------------------------------------------------------------------------
// Unit 1 — user-stats 9-query bundle (`user.py:397-522`).
// ---------------------------------------------------------------------------

/// Response keys in emission order (`user.py:510-520`). Note the scalar-name
/// asymmetry (`created_issues` vs `assigned_issues`) and the Q9 plural key
/// `present_cycles` (the variable is singular `present_cycle`, `:502`).
/// Matches `contract-tests/.../test_workspace_extras.py::STATS_KEYS` as a set.
pub const STATS_RESPONSE_KEYS: &[&str] = &[
    "state_distribution",
    "priority_distribution",
    "created_issues",
    "assigned_issues",
    "completed_issues",
    "pending_issues",
    "subscribed_issues",
    "present_cycles",
    "upcoming_cycles",
];

/// `CLOSED_STATE_GROUPS` (`utils/constants.py:88` = `STATE_GROUP_ORDER[-2:]`).
pub const CLOSED_STATE_GROUPS: &[&str] = &["completed", "cancelled"];

/// Q1/Q2/Q4-Q6 assignee scope, `user.py:403`:
/// `(Q(assignees__in=[user_id]) & Q(issue_assignee__deleted_at__isnull=True))`.
///
/// ONE join to `issue_assignees`: `assignees` is
/// `through="IssueAssignee"` (`db/models/issue.py:161-166`), so the M2M hop
/// and the explicit reverse-FK hop are the same table + same ON and Django
/// reuses the join (verified with `str(qs.query)` on the pinned Django
/// 4.2.30 — a single `INNER JOIN "issue_assignees"` carrying both
/// `"assignee_id" IN (...)` and `"deleted_at" IS NULL`). One row must
/// satisfy BOTH conditions. The M2M target (`users`) hop is folded into
/// `link.assignee_id = $uid` (Django never emits it — the `__in` lookup
/// targets the through FK directly). Returns the `(joins_sql,
/// predicates_sql)` pair; the caller supplies the alias and the `$uid`
/// holder.
pub fn stats_assignee_scope_sql(link: &str, uid: &str) -> (String, String) {
    (
        format!("JOIN {ISSUE_ASSIGNEES} {link} ON {link}.issue_id = i.id"),
        format!("{link}.assignee_id = {uid} AND {link}.deleted_at IS NULL"),
    )
}

/// Shared FROM/WHERE preamble for the issue-rooted stats queries
/// (Q1-Q6): root `issue_objects` scope, slug, requester membership.
/// `$1` = target uid, `$2` = slug, `$3` = viewer. `extra_joins` carries
/// path-specific joins (e.g. [`stats_assignee_scope_sql`]); `extra_where`
/// path-specific predicates; `legacy_sql` the compiled `**filters`
/// (`user.py:408,424,445,456,468,480` — 7 of 9 queries; Q8/Q9 take none).
fn stats_issue_preamble(extra_joins: &str, extra_where: &str, legacy_sql: Option<&str>) -> String {
    format!(
        "FROM {ISSUES} i \
         {extra_joins} \
         JOIN {WORKSPACES} w ON w.id = i.workspace_id \
         JOIN {PROJECTS} p ON p.id = i.project_id \
         LEFT JOIN {STATES} s ON s.id = i.state_id \
         JOIN {PROJECT_MEMBERS} rpm ON rpm.project_id = p.id \
         WHERE w.slug = $2 AND {membership} AND {scope}{extra} AND {legacy}",
        membership = requester_membership_predicate("rpm", "$3"),
        scope = issue_manager_scope("i", "s", "p"),
        extra = if extra_where.is_empty() {
            String::new()
        } else {
            format!(" AND ({extra_where})")
        },
        legacy = match legacy_sql {
            Some(fragment) => format!("({fragment})"),
            None => "TRUE".to_owned(),
        },
    )
}

/// Q1 state distribution (`user.py:401-413`): `GROUP BY state__group`,
/// `COUNT(state_group)` (counts NON-NULL groups only — a NULL-state row
/// lands in a NULL group with count 0), `ORDER BY state_group` (Postgres
/// `ASC` default, NULLS LAST — same default Django gets).
/// Params: `$1` uid, `$2` slug, `$3` viewer.
pub fn stats_state_distribution_sql(legacy_sql: Option<&str>) -> String {
    let (joins, predicates) = stats_assignee_scope_sql("ia", "$1");
    format!(
        "SELECT s.\"group\" AS state_group, COUNT(s.\"group\") AS state_count \
         {preamble} \
         GROUP BY s.\"group\" ORDER BY s.\"group\"",
        preamble = stats_issue_preamble(&joins, &predicates, legacy_sql),
    )
}

/// The priority `CASE` annotation shared by Q2 (`user.py:428-434`):
/// `urgent=0 .. none=4`, anything else → 5. Order values come from the
/// merged [`PRIORITY_ORDER`] kernel; the `ELSE 5` default is Q2's own
/// (`default=Value(len(priority_order))`). `column` is the SQL for the
/// issue's priority (e.g. `i.priority`).
pub fn priority_order_case_sql(column: &str) -> String {
    let mut cases = String::new();
    for (index, priority) in PRIORITY_ORDER.iter().enumerate() {
        cases.push_str(&format!("WHEN {column} = '{priority}' THEN {index} "));
    }
    format!("CASE {cases}ELSE {} END", PRIORITY_ORDER.len())
}

/// Q2 priority distribution (`user.py:417-436`): `GROUP BY priority`,
/// `HAVING COUNT(*) >= 1` (always true — grouped counts are `>= 1` by
/// construction; ported verbatim), priority-`CASE` order.
/// Params: `$1` uid, `$2` slug, `$3` viewer.
pub fn stats_priority_distribution_sql(legacy_sql: Option<&str>) -> String {
    let (joins, predicates) = stats_assignee_scope_sql("ia", "$1");
    format!(
        "SELECT i.priority AS priority, COUNT(i.priority) AS priority_count, \
         {case} AS priority_order \
         {preamble} \
         GROUP BY i.priority HAVING COUNT(i.priority) >= 1 ORDER BY priority_order",
        case = priority_order_case_sql("i.priority"),
        preamble = stats_issue_preamble(&joins, &predicates, legacy_sql),
    )
}

/// Q3 created count (`user.py:438-447`): `created_by_id = uid` (a plain
/// column — no assignee joins, no fanout).
/// Params: `$1` uid, `$2` slug, `$3` viewer.
pub fn stats_created_count_sql(legacy_sql: Option<&str>) -> String {
    format!(
        "SELECT COUNT(*) {}",
        stats_issue_preamble("", "i.created_by_id = $1", legacy_sql)
    )
}

/// Q4 assigned count (`user.py:449-458`): the single-join assignee scope;
/// fanout overcounts exactly like Django (no `DISTINCT`).
/// Params: `$1` uid, `$2` slug, `$3` viewer.
pub fn stats_assigned_count_sql(legacy_sql: Option<&str>) -> String {
    let (joins, predicates) = stats_assignee_scope_sql("ia", "$1");
    format!(
        "SELECT COUNT(*) {}",
        stats_issue_preamble(&joins, &predicates, legacy_sql)
    )
}

/// Q5 pending count (`user.py:460-470`): `~Q(state__group__in=CLOSED)`
/// keeps NULL-state rows via the `IS NULL` disjunct (same
/// `split_exclude` shape as the triage exclusion).
/// Params: `$1` uid, `$2` slug, `$3` viewer.
pub fn stats_pending_count_sql(legacy_sql: Option<&str>) -> String {
    let (joins, predicates) = stats_assignee_scope_sql("ia", "$1");
    let pending = format!(
        "({predicates}) AND (s.\"group\" IS NULL OR NOT (s.\"group\" IN ('completed', 'cancelled')))"
    );
    format!(
        "SELECT COUNT(*) {}",
        stats_issue_preamble(&joins, &pending, legacy_sql)
    )
}

/// Q6 completed count (`user.py:472-482`): literal `"completed"`
/// (not the `CLOSED` pair — `cancelled` rows are excluded).
/// Params: `$1` uid, `$2` slug, `$3` viewer.
pub fn stats_completed_count_sql(legacy_sql: Option<&str>) -> String {
    let (joins, predicates) = stats_assignee_scope_sql("ia", "$1");
    let completed = format!("({predicates}) AND s.\"group\" = 'completed'");
    format!(
        "SELECT COUNT(*) {}",
        stats_issue_preamble(&joins, &completed, legacy_sql)
    )
}

/// Q7 subscribed count (`user.py:484-494`): root
/// `IssueSubscriber.objects` (own `deleted_at` guard) + slug + requester
/// scope + the ONLY project-archived filter in the bundle (`:490`) +
/// legacy `**filters` — compiled for issues but applied to the subscriber
/// queryset, spliced verbatim (unknown keys are a Python-side 500).
/// Params: `$1` uid, `$2` slug, `$3` viewer.
pub fn stats_subscribed_count_sql(legacy_sql: Option<&str>) -> String {
    format!(
        "SELECT COUNT(*) FROM {ISSUE_SUBSCRIBERS} sub \
         JOIN {WORKSPACES} w ON w.id = sub.workspace_id \
         JOIN {PROJECTS} p ON p.id = sub.project_id \
         JOIN {PROJECT_MEMBERS} rpm ON rpm.project_id = p.id \
         WHERE sub.subscriber_id = $1 AND w.slug = $2 \
         AND {membership} AND p.archived_at IS NULL AND sub.deleted_at IS NULL{legacy}",
        membership = requester_membership_predicate("rpm", "$3"),
        legacy = and_legacy(legacy_sql),
    )
}

/// `.values()` keys for Q8/Q9 (`user.py:500,507`).
pub const CYCLE_VALUES_KEYS: &[&str] = &["cycle__name", "cycle__id", "cycle__project_id"];

/// Q8 upcoming cycles (`user.py:496-500`): `cycle.start_date > now`.
/// NO legacy filters, NO requester scope (ported as-is). The M2M hop is a
/// single through-join with NO deleted condition (unlike Q1-Q6, there is
/// no second conjunct — live-or-deleted links match); the `users` target
/// hop is folded into `ia.assignee_id` (`assignee_id` is a non-null
/// `CASCADE` FK, so the extra join filters nothing). Default ordering
/// applies (`CycleIssue.Meta.ordering = ("-created_at",)`,
/// `db/models/cycle.py:124`). Params: `$1` slug, `$2` now, `$3` uid.
pub fn stats_upcoming_cycles_sql() -> String {
    format!(
        "SELECT c.name AS cycle__name, c.id AS cycle__id, c.project_id AS cycle__project_id \
         FROM {CYCLE_ISSUES} ci \
         JOIN {CYCLES} c ON c.id = ci.cycle_id \
         JOIN {WORKSPACES} w ON w.id = ci.workspace_id \
         JOIN {ISSUES} i ON i.id = ci.issue_id \
         JOIN {ISSUE_ASSIGNEES} ia ON ia.issue_id = i.id \
         WHERE w.slug = $1 AND c.start_date > $2 AND ia.assignee_id = $3 \
         AND ci.deleted_at IS NULL \
         ORDER BY ci.created_at DESC"
    )
}

/// Q9 present cycle (`user.py:502-507`): `start < now < end`. Same
/// no-filters/no-scope shape as Q8; response key is plural
/// `present_cycles` (see [`STATS_RESPONSE_KEYS`]).
/// Params: `$1` slug, `$2` now, `$3` uid.
pub fn stats_present_cycles_sql() -> String {
    format!(
        "SELECT c.name AS cycle__name, c.id AS cycle__id, c.project_id AS cycle__project_id \
         FROM {CYCLE_ISSUES} ci \
         JOIN {CYCLES} c ON c.id = ci.cycle_id \
         JOIN {WORKSPACES} w ON w.id = ci.workspace_id \
         JOIN {ISSUES} i ON i.id = ci.issue_id \
         JOIN {ISSUE_ASSIGNEES} ia ON ia.issue_id = i.id \
         WHERE w.slug = $1 AND c.start_date < $2 AND c.end_date > $2 AND ia.assignee_id = $3 \
         AND ci.deleted_at IS NULL \
         ORDER BY ci.created_at DESC"
    )
}

// ---------------------------------------------------------------------------
// Unit 2 — user-issues pipeline (`user.py:99-251`).
// ---------------------------------------------------------------------------

/// Base filtered set, `user.py:140-154` (before annotations/ordering):
/// `id IN (assignee-OR-creator-OR-subscriber id-set, slug-scoped)` AND slug
/// AND the requester's OWN active project memberships (`:146-147` — the
/// viewer sees only issues in THEIR OWN projects, not the target's;
/// ported as-is).
///
/// Pipeline order the handlers follow (`:136-172`): legacy
/// `issue_filters` compile → `order_by` default read → this base →
/// `ComplexFilterBackend` + `IssueFilterSet` (`filter_queryset`, F-07
/// `pidash_db::{filter, filterset}`) → legacy `**filters` (F-07
/// `pidash_db::issue_filters`) → deepcopy the total-count queryset
/// (filters applied, annotations NOT) → [`profile_issues_annotations_sql`]
/// → [`profile_issues_order`] → `issue_queryset_grouper` (api crate) →
/// [`profile_group_mismatch`] → grouped/sub-grouped/flat paginate with
/// [`grouped_count_filter_sql`].
///
/// Both the outer query and the `id__in` subquery are rooted in
/// `Issue.issue_objects`, so the manager scope applies TWICE (triaged,
/// archived and draft issues are excluded from the id-set itself). The
/// OR legs LEFT-join (Django demotes joins under `OR`); none carries a
/// deleted condition — unlike the stats bundle, R2 has NO second
/// deleted conjunct, so live-or-deleted assignee/subscriber links match.
/// Params: `$1` target uid, `$2` slug, `$3` viewer.
pub fn profile_issues_from_where() -> String {
    format!(
        "FROM {ISSUES} i \
         JOIN {WORKSPACES} w ON w.id = i.workspace_id \
         JOIN {PROJECTS} p ON p.id = i.project_id \
         LEFT JOIN {STATES} s ON s.id = i.state_id \
         JOIN {PROJECT_MEMBERS} rpm ON rpm.project_id = p.id \
         WHERE i.id IN ( \
           SELECT i2.id FROM {ISSUES} i2 \
           JOIN {WORKSPACES} w2 ON w2.id = i2.workspace_id \
           JOIN {PROJECTS} p2 ON p2.id = i2.project_id \
           LEFT JOIN {STATES} s2 ON s2.id = i2.state_id \
           LEFT JOIN {ISSUE_ASSIGNEES} ia ON ia.issue_id = i2.id \
           LEFT JOIN {ISSUE_SUBSCRIBERS} sub ON sub.issue_id = i2.id \
           WHERE (ia.assignee_id = $1 OR i2.created_by_id = $1 OR sub.subscriber_id = $1) \
           AND w2.slug = $2 AND {inner_scope} \
         ) \
         AND w.slug = $2 AND {membership} AND {outer_scope}",
        inner_scope = issue_manager_scope("i2", "s2", "p2"),
        membership = requester_membership_predicate("rpm", "$3"),
        outer_scope = issue_manager_scope("i", "s", "p"),
    )
}

/// The four `apply_annotations` SELECT expressions, `user.py:105-134`
/// (alias `i`): `cycle_id` subquery (`:108-110`), `link_count` (`:113-117`),
/// `attachment_count` (`:119-126`, `entity_type = 'ISSUE_ATTACHMENT'`),
/// `sub_issues_count` (`:128-132`). Same shape as pilot-2's
/// `annotation_selects` (no array selects here — those belong to the
/// grouper's `on_results`, api crate): every annotation queries its
/// model's manager directly, so the subqueries DO carry the
/// `deleted_at IS NULL` guard (unlike filter joins). The three counts are
/// scalar aggregates (`Func(F("id"), function="Count")` is not an
/// Aggregate, so Django emits NO `GROUP BY`): they return 0 when empty,
/// never NULL (plain `COUNT` — verified with `str(qs.query)` on the
/// pinned Django 4.2.30). The `cycle_id` subquery keeps Django's inner
/// `ORDER BY ci.created_at DESC` (`CycleIssue.Meta.ordering`); without it
/// the `LIMIT 1` pick is nondeterministic over several live links.
/// `prefetch_related("assignees", "labels", "issue_module__module")`
/// (`:133`) is serializer-layer and emits no SQL here.
pub fn profile_issues_annotations_sql() -> String {
    format!(
        "(SELECT ci.cycle_id FROM {CYCLE_ISSUES} ci \
          WHERE ci.issue_id = i.id AND ci.deleted_at IS NULL \
          ORDER BY ci.created_at DESC LIMIT 1) AS cycle_id, \
         (SELECT COUNT(*) FROM {ISSUE_LINKS} il \
          WHERE il.issue_id = i.id AND il.deleted_at IS NULL) AS link_count, \
         (SELECT COUNT(*) FROM {FILE_ASSETS} fa \
          WHERE fa.issue_id = i.id AND fa.entity_type = 'ISSUE_ATTACHMENT' \
            AND fa.deleted_at IS NULL) AS attachment_count, \
         (SELECT COUNT(*) FROM {ISSUES} c \
           LEFT JOIN {STATES} cs ON cs.id = c.state_id \
           JOIN {PROJECTS} cp ON cp.id = c.project_id \
          WHERE c.parent_id = i.id AND {sub_scope}) AS sub_issues_count",
        sub_scope = issue_manager_scope("c", "cs", "cp"),
    )
}

/// Ordering for the issues pipeline: default `order_by` is `-created_at`
/// (`user.py:139`), resolved by the MERGED `order_issue_queryset` port
/// (`crate::app_issues::order_sql` — priority/state/min/default branches
/// with their ported quirks, NOT re-ported here). `state_column` is the
/// SQL for the issue's state group; `min_column` maps a min-aggregation
/// relation key to its pre-aggregated `MIN(...)` alias.
pub fn profile_issues_order(
    order_by_param: Option<&str>,
    state_column: &str,
    min_column: impl Fn(&str) -> String,
) -> OrderSpec {
    order_sql(
        order_by_param.unwrap_or("-created_at"),
        state_column,
        min_column,
    )
}

/// The `group_by == sub_group_by` 400 guard, `user.py:176-182`
/// (`{"error": "Group by and sub group by cannot have same parameters"}`).
/// Delegates to the MERGED pilot-2 kernel
/// (`crate::app_issues::raw_group_mismatch`): checked in-view only when
/// both params are present and non-empty, before pagination. Handlers map
/// `Some(ParamError)` to the 400 body via [`ParamError::body`].
pub fn profile_group_mismatch(
    group_by: Option<&str>,
    sub_group_by: Option<&str>,
) -> Option<ParamError> {
    raw_group_mismatch(group_by, sub_group_by)
}

/// `LEFT JOIN` for the grouped/sub-grouped `count_filter`
/// (`user.py:207-214,234-241`): the reverse `issue_intake` hop. NO
/// `deleted_at` guard (filter join). `issue` is the caller's `issues`
/// alias; the join alias is fixed as `ii` (see
/// [`grouped_count_filter_sql`]).
pub fn grouped_count_filter_join(issue: &str) -> String {
    format!("LEFT JOIN {INTAKE_ISSUES} ii ON ii.issue_id = {issue}.id")
}

/// The `count_filter` predicate (`Count("id", filter=..., distinct=True)`,
/// `utils/paginator.py:283,301`): intake status accepted (1), rejected
/// (-1) or duplicate (2) — pending (-2) and snoozed (0) excluded
/// (`IntakeIssueStatus`, `db/models/intake.py:42-47`) — OR no intake row,
/// AND unarchived AND not a draft. Requires
/// [`grouped_count_filter_join`] (`ii` alias); `issue` is the caller's
/// `issues` alias.
pub fn grouped_count_filter_sql(issue: &str) -> String {
    format!(
        "((ii.status IN (1, -1, 2) OR ii.id IS NULL) \
         AND {issue}.archived_at IS NULL AND {issue}.is_draft = FALSE)"
    )
}

/// How a `group_by`/`sub_group_by` field's values are produced
/// (`utils/grouper.py::issue_group_values`, called WITHOUT `project_id`
/// at `user.py:193-204` — the workspace-scoped branches).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GroupValuesSource {
    /// Static list, no query (`priority`, `state__group`).
    Static(&'static [&'static str]),
    /// `SELECT` returning one id column; `$1` = slug. Whether `"None"` is
    /// appended afterwards is [`group_values_appends_none`].
    Sql(String),
    /// `SELECT DISTINCT` over the FILTERED total queryset (post-filter,
    /// pre-annotation/ordering — filters applied, `order_by` NOT yet;
    /// `user.py:157` deepcopy). The `.0` column is read from the issue
    /// alias `i`; the handler wraps [`profile_issues_from_where`] plus the
    /// compiled filter fragments. Django appends the default-ordering
    /// column to satisfy Postgres (`get_extra_select`), so values
    /// DUPLICATE and NULLs are kept (ported quirk — see builder).
    DistinctOverFilteredSet(&'static str),
}

/// `"None"`-sentinel rule for [`GroupValuesSource::Sql`] branches: the
/// `labels__id`, `issue_module__module_id` and `cycle_id` branches append
/// the STRING `"None"` (`grouper.py:163,178,185`); `state_id`,
/// `assignees__id` and `project_id` do NOT (ported asymmetry).
pub fn group_values_appends_none(field: &str) -> bool {
    matches!(field, "labels__id" | "issue_module__module_id" | "cycle_id")
}

/// `issue_group_values(field, slug, filters, queryset)` for the profile
/// path (`grouper.py:146-224`, no `project_id`). `state_id` carries BOTH
/// the explicit `is_triage = FALSE` AND `StateManager`'s
/// `NOT ("group" = 'triage')` (`db/models/state.py:79-83`).
/// `assignees__id` reads ACTIVE `workspace_members` (`:173-176` — the
/// no-project branch, unlike pilot-2's project-scoped member list).
/// `priority`/`state__group` reuse the merged
/// [`PRIORITY_VALUES`]/[`STATE_GROUP_VALUES`] kernels (identical lists).
/// Every `Sql` branch keeps its model default ordering (`-created_at`,
/// except `state_id` → `sequence ASC`, `db/models/state.py` Meta).
/// Unknown fields yield no values (`:224`, `return []`) → `None`.
pub fn profile_group_values(field: &str) -> Option<GroupValuesSource> {
    let source = match field {
        "state_id" => GroupValuesSource::Sql(format!(
            "SELECT s.id FROM {STATES} s JOIN {WORKSPACES} w ON w.id = s.workspace_id \
             WHERE s.is_triage = FALSE AND NOT (s.\"group\" = 'triage') \
             AND w.slug = $1 AND s.deleted_at IS NULL \
             ORDER BY s.sequence"
        )),
        "labels__id" => GroupValuesSource::Sql(format!(
            "SELECT l.id FROM {LABELS} l JOIN {WORKSPACES} w ON w.id = l.workspace_id \
             WHERE w.slug = $1 AND l.deleted_at IS NULL \
             ORDER BY l.created_at DESC"
        )),
        "assignees__id" => GroupValuesSource::Sql(format!(
            "SELECT wm.member_id FROM {WORKSPACE_MEMBERS} wm \
             JOIN {WORKSPACES} w ON w.id = wm.workspace_id \
             WHERE w.slug = $1 AND wm.is_active = TRUE AND wm.deleted_at IS NULL \
             ORDER BY wm.created_at DESC"
        )),
        "issue_module__module_id" => GroupValuesSource::Sql(format!(
            "SELECT m.id FROM {MODULES} m JOIN {WORKSPACES} w ON w.id = m.workspace_id \
             WHERE w.slug = $1 AND m.deleted_at IS NULL \
             ORDER BY m.created_at DESC"
        )),
        "cycle_id" => GroupValuesSource::Sql(format!(
            "SELECT c.id FROM {CYCLES} c JOIN {WORKSPACES} w ON w.id = c.workspace_id \
             WHERE w.slug = $1 AND c.deleted_at IS NULL \
             ORDER BY c.created_at DESC"
        )),
        "project_id" => GroupValuesSource::Sql(format!(
            "SELECT p.id FROM {PROJECTS} p JOIN {WORKSPACES} w ON w.id = p.workspace_id \
             WHERE w.slug = $1 AND p.deleted_at IS NULL \
             ORDER BY p.created_at DESC"
        )),
        "priority" => GroupValuesSource::Static(PRIORITY_VALUES),
        "state__group" => GroupValuesSource::Static(STATE_GROUP_VALUES),
        "target_date" => GroupValuesSource::DistinctOverFilteredSet("i.target_date"),
        "start_date" => GroupValuesSource::DistinctOverFilteredSet("i.start_date"),
        "created_by" => GroupValuesSource::DistinctOverFilteredSet("i.created_by_id"),
        _ => return None,
    };
    Some(source)
}

/// `SELECT` for [`GroupValuesSource::DistinctOverFilteredSet`]:
/// `SELECT DISTINCT {column}, i.created_at ... ORDER BY i.created_at DESC`.
/// The extra `created_at` select is Django's `get_extra_select`
/// (`db/models/sql/compiler.py`): plain `DISTINCT` over `target_date`
/// with the default `-created_at` ordering would be rejected by Postgres
/// ("ORDER BY expressions must appear in select list"), so Django selects
/// the ordering column too — `DISTINCT` then applies to the PAIR, values
/// duplicate, and NULLs are kept (no `IS NOT NULL`). Handlers read column
/// 0 (`values_list(flat=True)` yields `row[0]`). `filtered_from_where` is
/// the filtered-set fragment (from/joins/where, binds first).
pub fn group_values_distinct_sql(column: &str, filtered_from_where: &str) -> String {
    format!(
        "SELECT DISTINCT {column}, i.created_at AS ordering_created_at \
         {filtered_from_where} \
         ORDER BY i.created_at DESC"
    )
}

// ---------------------------------------------------------------------------
// Unit 3 — user-activity query (`user.py:371-395`, `user/base.py:392-404`).
// ---------------------------------------------------------------------------

/// Fields excluded from the activity route (`~Q(field__in=[...])`,
/// `user.py:378` — same exclusion list as the export-CSV query).
pub const ACTIVITY_EXCLUDED_FIELDS: &[&str] = &["comment", "vote", "reaction", "draft"];

/// `?project=` values that fail UUID validation: Django's `UUIDField`
/// raises `ValidationError`, mapped to 400 (`app/views/base.py:126-130`).
pub const PROJECT_UUID_ERROR_BODY: &str = r#"{"error":"Please provide valid detail"}"#;
/// Status for [`PROJECT_UUID_ERROR_BODY`].
pub const PROJECT_UUID_ERROR_STATUS: u16 = 400;

/// Validate the raw `?project=` values (`user.py:375,386-387` —
/// `request.query_params.getlist("project", [])`, filtered RAW with no
/// UUID check in the view). Django validates at lookup-prep time
/// (`UUIDField` → `uuid.UUID(value)`), so this mirrors CPython's
/// `uuid.UUID.__init__` preprocessing verbatim (`Lib/uuid.py`):
/// global `replace('urn:', '')` + `replace('uuid:', '')` (bare `uuid:`
/// prefix accepted, `URN:UUID:` rejected — case-sensitive), `strip('{}')`
/// (multi/mismatched braces accepted), drop ALL `-` (free-placed
/// hyphens, only the 32-hex count matters), length `== 32`, then
/// hex-parse. Surrounding whitespace is rejected via the length check
/// (all verified against CPython's `uuid` module); anything else raises
/// `ValidationError` → handlers answer [`PROJECT_UUID_ERROR_BODY`] /
/// [`PROJECT_UUID_ERROR_STATUS`]. Recorded residual edge (NOT ported —
/// pathological, beyond review scope): CPython's final `int(hex, 16)`
/// would also accept a leading sign, inter-digit underscores, or padding
/// whitespace that still totals 32 chars; the hex-parse below rejects
/// those (400 where Django 200s).
pub fn validate_project_uuids(raw: &[&str]) -> Result<Vec<uuid::Uuid>, ProjectFilterError> {
    raw.iter()
        .map(|value| {
            // CPython Lib/uuid.py, verbatim:
            //   hex = hex.replace('urn:', '').replace('uuid:', '')
            //   hex = hex.strip('{}').replace('-', '')
            //   if len(hex) != 32: raise ValueError(...)
            let hex = value.replace("urn:", "").replace("uuid:", "");
            let hex = hex.trim_matches(['{', '}']).replace('-', "");
            if hex.len() != 32 {
                return Err(ProjectFilterError {
                    value: (*value).to_owned(),
                });
            }
            uuid::Uuid::parse_str(&hex).map_err(|_| ProjectFilterError {
                value: (*value).to_owned(),
            })
        })
        .collect()
}

/// A `?project=` value that is not a valid UUID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectFilterError {
    /// The offending raw value (for logs; never echoed to clients —
    /// Django's 400 body carries no value).
    pub value: String,
}

impl std::fmt::Display for ProjectFilterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid project UUID: {}", self.value)
    }
}

impl std::error::Error for ProjectFilterError {}

/// Workspace user-activity FROM/WHERE, `user.py:377-387`: field exclusion,
/// slug, requester project scope, project-unarchived
/// (`:382` — ADDS an archived filter the export-CSV twin LACKS; ported),
/// and `actor = uid` (`:383` spells `actor=`, not `actor_id=` — same column).
/// Root `IssueActivity.objects` guard applies (`a.deleted_at IS NULL`).
/// `select_related("actor", "workspace", "issue", "project")` (`:384`)
/// reuses the `w`/`p` joins and adds `actor_u` (INNER — Django
/// promotes the `select_related("actor")` LEFT join under the
/// `actor_id =` equality, verified with `str(qs.query)` on the pinned
/// Django 4.2.30; result-neutral here since the baked-in
/// `a.actor_id = $1` excludes NULL actors either way) and `i` (LEFT —
/// nullable FK, no equality). `~Q(field__in)` compiles to
/// `NOT (field IN (...) AND field IS NOT NULL)`, so NULL-`field` rows
/// are INCLUDED (`field` is `null=True` — ported exactly, not bare
/// `NOT IN`). Ordering (`-created_at` default, `:390`) is the
/// paginator's (api crate).
/// Params: `$1` target uid, `$2` slug, `$3` viewer.
/// `projects_in_sql` is the optional `project__in` fragment
/// (`p.id IN (...)`, values pre-validated by [`validate_project_uuids`]);
/// `None` renders nothing (`if projects:`, `:386`).
pub fn user_activity_from_where(projects_in_sql: Option<&str>) -> String {
    format!(
        "FROM {ISSUE_ACTIVITIES} a \
         JOIN {WORKSPACES} w ON w.id = a.workspace_id \
         JOIN {PROJECTS} p ON p.id = a.project_id \
         JOIN {PROJECT_MEMBERS} rpm ON rpm.project_id = p.id \
         JOIN {USERS} actor_u ON actor_u.id = a.actor_id \
         LEFT JOIN {ISSUES} i ON i.id = a.issue_id \
         WHERE NOT (a.field IN ('comment', 'vote', 'reaction', 'draft') \
           AND a.field IS NOT NULL) \
         AND w.slug = $2 AND {membership} \
         AND p.archived_at IS NULL AND a.actor_id = $1 AND a.deleted_at IS NULL{projects}",
        membership = requester_membership_predicate("rpm", "$3"),
        projects = match projects_in_sql {
            Some(fragment) => format!(" AND ({fragment})"),
            None => String::new(),
        },
    )
}

/// Me-activities FROM/WHERE (`app/views/user/base.py:392-404`): same
/// `select_related` four, but scoped ONLY by `actor = request.user` — no
/// slug, no project scope, no field exclusion (ported as-is).
/// `actor_u` is INNER for the same promotion reason as
/// [`user_activity_from_where`] (the `a.actor_id = $1` equality).
/// Params: `$1` viewer.
pub fn me_activities_from_where() -> String {
    format!(
        "FROM {ISSUE_ACTIVITIES} a \
         JOIN {WORKSPACES} w ON w.id = a.workspace_id \
         JOIN {PROJECTS} p ON p.id = a.project_id \
         JOIN {USERS} actor_u ON actor_u.id = a.actor_id \
         LEFT JOIN {ISSUES} i ON i.id = a.issue_id \
         WHERE a.actor_id = $1 AND a.deleted_at IS NULL"
    )
}

// ---------------------------------------------------------------------------
// Unit 4 — graphs (`user.py:524-559`).
// ---------------------------------------------------------------------------

/// R6 window: SIX months (`user.py:530`) vs the dashboard twin's three
/// (`base.py:268` — ported asymmetry, dashboard owned by PIDASHCONV-608).
pub const ACTIVITY_GRAPH_MONTHS: u32 = 6;

/// R6 activity graph (`user.py:524-538`): per-day activity counts for the
/// caller. Port the `__date` asymmetry EXACTLY (verified with
/// `str(qs.query)` on the pinned Django 4.2.30, `USE_TZ=True`,
/// `TIME_ZONE="UTC"`): the FILTER converts —
/// `("issue_activities"."created_at" AT TIME ZONE UTC)::date >= ...` —
/// while the `Cast("created_at", DateField())` annotation does NOT
/// (`("issue_activities"."created_at")::date`). The cutoff is server-local
/// `date.today() - 6 months` (NOT UTC — Django's `date.today()` uses the
/// system zone; the handler computes it via [`months_ago`] from the local
/// date and binds it).
/// Params: `$1` user, `$2` slug, `$3` cutoff `DATE`.
pub fn activity_graph_sql() -> String {
    format!(
        "SELECT CAST(a.created_at AS DATE) AS created_date, \
         COUNT(CAST(a.created_at AS DATE)) AS activity_count \
         FROM {ISSUE_ACTIVITIES} a \
         JOIN {WORKSPACES} w ON w.id = a.workspace_id \
         WHERE a.actor_id = $1 AND w.slug = $2 \
         AND (a.created_at AT TIME ZONE UTC)::date >= $3 AND a.deleted_at IS NULL \
         GROUP BY CAST(a.created_at AS DATE) ORDER BY created_date"
    )
}

/// R7 completed graph (`user.py:541-559`): per-week-bucket completed counts
/// for the caller. `completed_week = EXTRACT(WEEK ...)` then
/// `week = completed_week % 4` (`:553` — mod-4 buckets 0-3, vs the
/// dashboard's `WeekInMonth` 1-5, `base.py:257-259` — port each exactly).
/// `ExtractWeek` declares `IntegerField` output, so Django converts each
/// `week` to int server-side; the `::INT` cast reproduces that in SQL
/// (bare `EXTRACT` yields `NUMERIC`, which would serialize as `1.0`).
/// Every `EXTRACT` converts `AT TIME ZONE UTC` (`USE_TZ=True`,
/// `TIME_ZONE="UTC"` — verified with `str(qs.query)` on the pinned Django
/// 4.2.30; bare `EXTRACT` follows the session TimeZone and misbuckets
/// boundary rows). `COUNT(completed_week)` counts non-null weeks = rows
/// (`completed_at IS NOT NULL` is explicit). Root `issue_objects` scope
/// applies; the single `assignees__in` M2M hop carries NO deleted condition
/// (live-or-deleted links match), with the `users` target hop folded into
/// `ia.assignee_id` (Django never emits it — the `__in` lookup targets the
/// through FK directly). No project/requester scope.
/// Params: `$1` user, `$2` slug, `$3` month (see [`parse_month_param`]).
pub fn completed_graph_sql() -> String {
    format!(
        "SELECT (EXTRACT(WEEK FROM i.completed_at AT TIME ZONE UTC)::INT % 4) AS week, \
         COUNT(EXTRACT(WEEK FROM i.completed_at AT TIME ZONE UTC)) AS completed_count \
         FROM {ISSUES} i \
         JOIN {ISSUE_ASSIGNEES} ia ON ia.issue_id = i.id \
         JOIN {WORKSPACES} w ON w.id = i.workspace_id \
         JOIN {PROJECTS} p ON p.id = i.project_id \
         LEFT JOIN {STATES} s ON s.id = i.state_id \
         WHERE ia.assignee_id = $1 AND w.slug = $2 \
         AND EXTRACT(MONTH FROM i.completed_at AT TIME ZONE UTC) = $3 AND i.completed_at IS NOT NULL \
         AND {scope} \
         GROUP BY (EXTRACT(WEEK FROM i.completed_at AT TIME ZONE UTC)::INT % 4) ORDER BY week",
        scope = issue_manager_scope("i", "s", "p"),
    )
}

/// `date.today() + relativedelta(months=-months)`: month arithmetic with
/// end-of-month clamping (e.g. 2026-08-31 minus 6 → 2026-02-28, 2026 is
/// not a leap year). The caller passes the SERVER-LOCAL today
/// (`date.today()` is zone-naive local, not UTC).
pub fn months_ago(today: chrono::NaiveDate, months: u32) -> chrono::NaiveDate {
    let total = today.year() * 12 + (today.month() as i32 - 1) - months as i32;
    let year = total.div_euclid(12);
    let month = (total.rem_euclid(12) + 1) as u32;
    let last_day = if month == 12 {
        chrono::NaiveDate::from_ymd_opt(year + 1, 1, 1)
    } else {
        chrono::NaiveDate::from_ymd_opt(year, month + 1, 1)
    }
    .expect("month arithmetic stays in range")
    .pred_opt()
    .expect("first-of-month has a predecessor")
    .day();
    let day = today.day().min(last_day);
    chrono::NaiveDate::from_ymd_opt(year, month, day).expect("clamped day is valid")
}

/// `?month=` default (`user.py:543`, `base.py:276`): `request.GET.get("month", 1)`
/// yields INT `1` when absent but `str` when present (ported quirk —
/// shared with the dashboard Q2, PIDASHCONV-608).
pub const MONTH_DEFAULT: i64 = 1;

/// Garbage `?month=` values: `completed_at__month=<str>` goes through
/// integer prep (`int(value)`), which raises `ValueError` — NOT
/// `ValidationError`, so `handle_exception` falls through to the 500
/// branch (`app/views/base.py:145-149`), not the 400 one.
pub const MONTH_INVALID_STATUS: u16 = 500;
/// 500 body (`app/views/base.py:146-149`).
pub const SERVER_ERROR_BODY: &str = r#"{"error":"Something went wrong please try again later"}"#;

/// Parse `?month=` with Python `int(str)` semantics: surrounding
/// whitespace stripped, optional `+`/`-` sign, ASCII digits with single
/// interior underscores (`"1_2"` → 12, like `parse_per_page`'s `int()`
/// parity). `None` → [`MONTH_DEFAULT`]. Out-of-range months (0, 13, ...)
/// parse fine and match no rows; unparseable input → [`MonthError`]
/// (handlers answer [`MONTH_INVALID_STATUS`] / [`SERVER_ERROR_BODY`]).
/// Values beyond `i64` SATURATE (`parse_per_page` precedent: parse via
/// `i128`, clamp to `i64`) — Python's unbounded `int` would return `[]`,
/// and a saturated out-of-range month matches no rows either.
pub fn parse_month_param(raw: Option<&str>) -> Result<i64, MonthError> {
    let Some(text) = raw else {
        return Ok(MONTH_DEFAULT);
    };
    let trimmed = text.trim();
    let (sign, digits) = match trimmed.strip_prefix(['+', '-']) {
        Some(rest) => (trimmed.starts_with('-'), rest),
        None => (false, trimmed),
    };
    if digits.is_empty() {
        return Err(MonthError {
            value: text.to_owned(),
        });
    }
    let mut canonical = String::with_capacity(digits.len());
    let mut prev_underscore = true; // leading '_' is invalid, like int()
    for ch in digits.chars() {
        if ch == '_' {
            if prev_underscore {
                return Err(MonthError {
                    value: text.to_owned(),
                });
            }
            prev_underscore = true;
        } else if ch.is_ascii_digit() {
            canonical.push(ch);
            prev_underscore = false;
        } else {
            return Err(MonthError {
                value: text.to_owned(),
            });
        }
    }
    if prev_underscore {
        return Err(MonthError {
            value: text.to_owned(),
        });
    }
    // `canonical` is validated all-digits here, so a parse failure is
    // magnitude overflow (40+ digits): saturate by sign, like Python's
    // unbounded int matching no rows.
    let magnitude: i128 = canonical.parse().unwrap_or(i128::MAX);
    let value = if sign { -magnitude } else { magnitude };
    Ok(value.clamp(i64::MIN as i128, i64::MAX as i128) as i64)
}

/// A `?month=` value Python's `int()` rejects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonthError {
    /// The offending raw value (for logs; never echoed — the 500 body is fixed).
    pub value: String,
}

impl std::fmt::Display for MonthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid month parameter: {}", self.value)
    }
}

impl std::error::Error for MonthError {}

// ---------------------------------------------------------------------------
// Unit 5 — user-props + profile + dashboard reads (`user.py:253-369`).
// ---------------------------------------------------------------------------

/// `.get()` miss body: `ObjectDoesNotExist` → 404
/// (`app/views/base.py:132-136`).
pub const OBJECT_NOT_FOUND_BODY: &str = r#"{"error":"The required object does not exist."}"#;
/// Status for [`OBJECT_NOT_FOUND_BODY`].
pub const OBJECT_NOT_FOUND_STATUS: u16 = 404;

/// `Workspace.objects.get(slug=slug)` (`user.py:257,271` — shared by the
/// props get/patch). Root-manager scope; miss → 404. Only `id` is read
/// downstream. Params: `$1` slug.
pub fn workspace_lookup_sql() -> String {
    format!("SELECT w.id FROM {WORKSPACES} w WHERE w.slug = $1 AND w.deleted_at IS NULL LIMIT 1")
}

/// `WorkspaceUserProperties.objects.get_or_create(user, workspace)`
/// lookup half (`user.py:259-261,273-275`; patch spells `workspace_id=`,
/// get spells `workspace=` — same column). Full column list in
/// `COLUMNS` order (Django selects every concrete field); the serializer
/// then renders the row. Root-manager scope.
/// Params: `$1` workspace id, `$2` user id.
pub fn user_props_lookup_sql() -> String {
    let columns = wup::COLUMNS
        .iter()
        .map(|column| format!("\"{column}\""))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "SELECT {columns} FROM \"{table}\" \
         WHERE workspace_id = $1 AND user_id = $2 AND deleted_at IS NULL LIMIT 1",
        table = wup::TABLE,
    )
}

/// `get_or_create` insert half: one `$n` holder per [`wup::COLUMNS`] entry
/// (14 holders, same order). VALUES mapping: `id` = fresh v4 UUID,
/// `created_at`/`updated_at` = now, `created_by_id` = the user (crum
/// current user on create), `updated_by_id` = NULL, `deleted_at` = NULL,
/// `workspace_id`/`user_id` = the lookup pair, `filters` =
/// [`wup::default_filters`], `display_filters` =
/// [`wup::default_display_filters`], `display_properties` =
/// [`wup::default_display_properties`], `rich_filters` =
/// [`wup::DEFAULT_RICH_FILTERS`], `navigation_project_limit` =
/// [`wup::DEFAULT_NAVIGATION_PROJECT_LIMIT`],
/// `navigation_control_preference` =
/// [`wup::DEFAULT_NAVIGATION_CONTROL_PREFERENCE`]. On unique-violation
/// (`workspace, user` where `deleted_at IS NULL`,
/// `db/models/workspace.py:399-406`) Django retries the lookup — the
/// handler repeats [`user_props_lookup_sql`].
pub fn user_props_insert_sql() -> String {
    let columns = wup::COLUMNS
        .iter()
        .map(|column| format!("\"{column}\""))
        .collect::<Vec<_>>()
        .join(", ");
    let holders = (1..=wup::COLUMNS.len())
        .map(|n| format!("${n}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "INSERT INTO \"{table}\" ({columns}) VALUES ({holders})",
        table = wup::TABLE,
    )
}

/// `User.objects.get(pk=user_id)` (`user.py:283`): the 8 `user_data`
/// columns (`:356-365`) plus the `avatar`/`cover_image` source columns
/// the `avatar_url`/`cover_image_url` properties resolve through
/// (`db/models/user.py:143-165`, FK-first, text fallback). No
/// soft-delete scope — `User.objects` is Django's stock `UserManager`.
/// Miss → [`USER_GET_MISS_STATUS`] (see the fixture-deviation note in the
/// module docs). Params: `$1` uid.
pub fn profile_user_sql() -> String {
    format!(
        "SELECT email, first_name, last_name, avatar, avatar_asset_id, \
         cover_image, cover_image_asset_id, date_joined, user_timezone, display_name \
         FROM {USERS} WHERE id = $1 LIMIT 1"
    )
}

/// `User.objects.get` miss status: 404 (NOT the fixture's 500 —
/// `User.DoesNotExist` subclasses `ObjectDoesNotExist`, mapped at
/// `app/views/base.py:132-136`; the live contract test
/// `test_user_profile_non_member_404` proves the path on this endpoint).
/// Body: [`OBJECT_NOT_FOUND_BODY`].
pub const USER_GET_MISS_STATUS: u16 = 404;

/// `user_data` keys in emission order (`user.py:356-365`). Matches
/// `contract-tests/.../test_workspace_extras.py::USER_DATA_KEYS` as a set.
pub const USER_DATA_KEYS: &[&str] = &[
    "email",
    "first_name",
    "last_name",
    "avatar_url",
    "cover_image_url",
    "date_joined",
    "user_timezone",
    "display_name",
];

/// Top-level profile response keys (`user.py:353-368`).
pub const PROFILE_RESPONSE_KEYS: &[&str] = &["project_data", "user_data"];

/// Requester lookup (`user.py:285-287`):
/// `WorkspaceMember.objects.get(workspace__slug, member=request.user, is_active)`.
/// Root-manager scope on the member row; the workspace hop is unguarded.
/// Miss → 404 ([`OBJECT_NOT_FOUND_BODY`], contract-pinned). Only `role`
/// is read downstream. Params: `$1` slug, `$2` requester.
pub fn requester_role_sql() -> String {
    format!(
        "SELECT wm.role FROM {WORKSPACE_MEMBERS} wm \
         JOIN {WORKSPACES} w ON w.id = wm.workspace_id \
         WHERE w.slug = $1 AND wm.member_id = $2 AND wm.is_active = TRUE \
         AND wm.deleted_at IS NULL LIMIT 1"
    )
}

/// The `role >= 15` branch literal (`user.py:289`): spelled as the NUMBER
/// `15` in Python (the `MEMBER` value, `ROLE_CHOICES` in
/// `db/models/workspace.py:19`, but NOT referenced symbolically — ported
/// as the literal). Guests keep `project_data = []` (`:288`).
pub const ROLE_MEMBER_MIN: i32 = 15;

/// Whether the requester role opens the `project_data` branch
/// ([`ROLE_MEMBER_MIN`]).
pub fn requester_sees_projects(role: i32) -> bool {
    role >= ROLE_MEMBER_MIN
}

/// `project_data`: slug + requester's active memberships + unarchived
/// projects (`user.py:291-296`), annotated with 4 `Count(..., filter=...)`
/// aggregates (`:297-342`) and `.values("id", "logo_props", ...)`
/// (`:343-351`). NO `ORDER BY` (verified with `str(qs.query)` on the
/// pinned Django 4.2.30): grouped queries without an explicit
/// `.order_by()` drop `Meta.ordering` (`Project.Meta.ordering =
/// ("-created_at",)` — `django/db/models/sql/compiler.py`,
/// `if self._meta_ordering: order_by = None`). `GROUP BY p.id,
/// p.logo_props` vs Django's `GROUP BY p.id` keeps the same groups
/// (`logo_props` is functionally dependent on the PK — ported as-is).
///
/// ONE query (ported fanout bug): Django chains the four annotations on a
/// single queryset and never splits multi-valued aggregates, so all four
/// `FILTER`s share ONE `LEFT JOIN issues` + ONE `LEFT JOIN
/// issue_assignees` + ONE `LEFT JOIN states`. Consequences, all ported:
/// `created_issues` overcounts by assignee-link fanout (its own `FILTER`
/// never mentions assignees, but the shared join still multiplies rows);
/// joined rows carry NO manager guard (deleted and triage issues count —
/// only `archived_at`/`is_draft` are explicit); duplicate live assignee
/// links double-count (the partial unique index permits them).
/// `pending_issues` uses the LITERAL trio
/// `('backlog','unstarted','started')` (`:332-336`) — review/test rows
/// excluded, NOT the `CLOSED` complement (ported as-is).
/// Params: `$1` target uid, `$2` slug, `$3` viewer.
pub fn profile_projects_sql() -> String {
    format!(
        "SELECT p.id, p.logo_props, \
         COUNT(pi.id) FILTER (WHERE pi.created_by_id = $1 \
           AND pi.archived_at IS NULL AND pi.is_draft = FALSE) AS created_issues, \
         COUNT(pi.id) FILTER (WHERE ia.assignee_id = $1 \
           AND pi.archived_at IS NULL AND pi.is_draft = FALSE) AS assigned_issues, \
         COUNT(pi.id) FILTER (WHERE pi.completed_at IS NOT NULL AND ia.assignee_id = $1 \
           AND pi.archived_at IS NULL AND pi.is_draft = FALSE) AS completed_issues, \
         COUNT(pi.id) FILTER (WHERE s.\"group\" IN ('backlog', 'unstarted', 'started') \
           AND ia.assignee_id = $1 \
           AND pi.archived_at IS NULL AND pi.is_draft = FALSE) AS pending_issues \
         FROM {PROJECTS} p \
         JOIN {WORKSPACES} w ON w.id = p.workspace_id \
         JOIN {PROJECT_MEMBERS} rpm ON rpm.project_id = p.id \
         LEFT JOIN {ISSUES} pi ON pi.project_id = p.id \
         LEFT JOIN {ISSUE_ASSIGNEES} ia ON ia.issue_id = pi.id \
         LEFT JOIN {STATES} s ON s.id = pi.state_id \
         WHERE w.slug = $2 AND {membership} \
         AND p.archived_at IS NULL AND p.deleted_at IS NULL \
         GROUP BY p.id, p.logo_props",
        membership = requester_membership_predicate("rpm", "$3"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    // -- shared fragments ----------------------------------------------------

    #[test]
    fn manager_scope_has_all_five_conjuncts() {
        let scope = issue_manager_scope("i", "s", "p");
        assert!(scope.contains("i.deleted_at IS NULL"), "{scope}");
        assert!(
            scope.contains("(s.\"group\" IS NULL OR NOT (s.\"group\" = 'triage'))"),
            "{scope}"
        );
        assert!(scope.contains("i.archived_at IS NULL"), "{scope}");
        assert!(scope.contains("p.archived_at IS NULL"), "{scope}");
        assert!(scope.contains("i.is_draft = FALSE"), "{scope}");
    }

    #[test]
    fn requester_scope_has_no_deleted_guard() {
        // Filter joins never apply the related manager (Django 4.2.30
        // Join.as_sql renders join_cols only) — a deleted membership
        // still matches. Ported as-is.
        let predicate = requester_membership_predicate("rpm", "$3");
        assert!(predicate.contains("rpm.member_id = $3"), "{predicate}");
        assert!(predicate.contains("rpm.is_active = TRUE"), "{predicate}");
        assert!(!predicate.contains("deleted_at"), "{predicate}");
    }

    #[test]
    fn table_names_match_django_meta() {
        assert_eq!(ISSUES, "issues");
        assert_eq!(ISSUE_ASSIGNEES, "issue_assignees");
        assert_eq!(ISSUE_SUBSCRIBERS, "issue_subscribers");
        assert_eq!(ISSUE_ACTIVITIES, "issue_activities");
        assert_eq!(ISSUE_LINKS, "issue_links");
        assert_eq!(FILE_ASSETS, "file_assets");
        assert_eq!(CYCLE_ISSUES, "cycle_issues");
        assert_eq!(CYCLES, "cycles");
        assert_eq!(PROJECTS, "projects");
        assert_eq!(PROJECT_MEMBERS, "project_members");
        assert_eq!(STATES, "states");
        assert_eq!(WORKSPACES, "workspaces");
        assert_eq!(WORKSPACE_MEMBERS, "workspace_members");
        assert_eq!(USERS, "users");
        assert_eq!(INTAKE_ISSUES, "intake_issues");
        assert_eq!(LABELS, "labels");
        assert_eq!(MODULES, "modules");
    }

    // -- unit 1: stats -------------------------------------------------------

    #[test]
    fn stats_keys_match_contract_set() {
        // contract-tests/app_workspace/test_workspace_extras.py::STATS_KEYS.
        let expected: HashSet<&str> = [
            "assigned_issues",
            "completed_issues",
            "created_issues",
            "pending_issues",
            "present_cycles",
            "priority_distribution",
            "state_distribution",
            "subscribed_issues",
            "upcoming_cycles",
        ]
        .into_iter()
        .collect();
        let got: HashSet<&str> = STATS_RESPONSE_KEYS.iter().copied().collect();
        assert_eq!(got, expected);
        // Emission order is the user.py:510-520 order.
        assert_eq!(STATS_RESPONSE_KEYS[0], "state_distribution");
        assert_eq!(STATS_RESPONSE_KEYS[7], "present_cycles");
        assert_eq!(STATS_RESPONSE_KEYS[8], "upcoming_cycles");
    }

    #[test]
    fn closed_groups_are_completed_and_cancelled() {
        assert_eq!(CLOSED_STATE_GROUPS, &["completed", "cancelled"]);
    }

    #[test]
    fn assignee_scope_is_one_join() {
        // through="IssueAssignee": the M2M hop and the explicit reverse-FK
        // hop collapse onto one join; one row satisfies both conditions
        // (verified with str(qs.query) on Django 4.2.30).
        let (joins, predicates) = stats_assignee_scope_sql("ia", "$1");
        assert_eq!(joins.matches("issue_assignees").count(), 1, "{joins}");
        assert!(
            joins.contains("issue_assignees ia ON ia.issue_id = i.id"),
            "{joins}"
        );
        assert!(
            predicates.contains("ia.assignee_id = $1 AND ia.deleted_at IS NULL"),
            "{predicates}"
        );
    }

    #[test]
    fn state_distribution_groups_and_orders_by_group() {
        let sql = stats_state_distribution_sql(None);
        assert!(sql.contains("COUNT(s.\"group\") AS state_count"), "{sql}");
        assert!(
            sql.contains("GROUP BY s.\"group\" ORDER BY s.\"group\""),
            "{sql}"
        );
        assert!(sql.contains("w.slug = $2"), "{sql}");
        assert!(sql.contains("i.deleted_at IS NULL"), "{sql}");
    }

    #[test]
    fn priority_case_orders_urgent_first_unknown_last() {
        let case = priority_order_case_sql("i.priority");
        let urgent = case.find("'urgent' THEN 0").expect("urgent first");
        let high = case.find("'high' THEN 1").expect("high");
        let medium = case.find("'medium' THEN 2").expect("medium");
        let low = case.find("'low' THEN 3").expect("low");
        let none = case.find("'none' THEN 4").expect("none");
        assert!(
            urgent < high && high < medium && medium < low && low < none,
            "{case}"
        );
        assert!(case.contains("ELSE 5 END"), "{case}");
    }

    #[test]
    fn priority_distribution_has_always_true_having() {
        // Ported bug user.py:427 — grouped counts are >= 1 by construction.
        let sql = stats_priority_distribution_sql(None);
        assert!(sql.contains("HAVING COUNT(i.priority) >= 1"), "{sql}");
        assert!(sql.contains("ORDER BY priority_order"), "{sql}");
    }

    #[test]
    fn legacy_filters_splice_into_seven_queries() {
        let fragment = "i.priority = 'high'";
        for sql in [
            stats_state_distribution_sql(Some(fragment)),
            stats_priority_distribution_sql(Some(fragment)),
            stats_created_count_sql(Some(fragment)),
            stats_assigned_count_sql(Some(fragment)),
            stats_pending_count_sql(Some(fragment)),
            stats_completed_count_sql(Some(fragment)),
            stats_subscribed_count_sql(Some(fragment)),
        ] {
            assert!(sql.contains(fragment), "{sql}");
        }
        // Q8/Q9 take no filters and no requester scope.
        for sql in [stats_upcoming_cycles_sql(), stats_present_cycles_sql()] {
            assert!(!sql.contains("project_members"), "{sql}");
            assert!(!sql.contains("rpm"), "{sql}");
        }
    }

    #[test]
    fn pending_keeps_null_states_completed_is_literal() {
        let pending = stats_pending_count_sql(None);
        assert!(
            pending.contains(
                "(s.\"group\" IS NULL OR NOT (s.\"group\" IN ('completed', 'cancelled')))"
            ),
            "{pending}"
        );
        let completed = stats_completed_count_sql(None);
        assert!(
            completed.contains("s.\"group\" = 'completed'"),
            "{completed}"
        );
        assert!(!completed.contains("cancelled"), "{completed}");
    }

    #[test]
    fn subscribed_is_only_archived_filtered_count() {
        let sql = stats_subscribed_count_sql(None);
        assert!(sql.contains("issue_subscribers sub"), "{sql}");
        assert!(sql.contains("sub.subscriber_id = $1"), "{sql}");
        assert!(sql.contains("p.archived_at IS NULL"), "{sql}");
        assert!(sql.contains("sub.deleted_at IS NULL"), "{sql}");
    }

    #[test]
    fn cycle_queries_cover_present_and_upcoming() {
        let upcoming = stats_upcoming_cycles_sql();
        assert!(upcoming.contains("c.start_date > $2"), "{upcoming}");
        assert!(!upcoming.contains("c.end_date"), "{upcoming}");
        let present = stats_present_cycles_sql();
        assert!(
            present.contains("c.start_date < $2 AND c.end_date > $2"),
            "{present}"
        );
        for sql in [&upcoming, &present] {
            assert!(sql.contains("c.name AS cycle__name"), "{sql}");
            assert!(sql.contains("c.id AS cycle__id"), "{sql}");
            assert!(sql.contains("c.project_id AS cycle__project_id"), "{sql}");
            assert!(sql.contains("ORDER BY ci.created_at DESC"), "{sql}");
            assert!(sql.contains("ci.deleted_at IS NULL"), "{sql}");
        }
        assert_eq!(
            CYCLE_VALUES_KEYS,
            &["cycle__name", "cycle__id", "cycle__project_id"]
        );
    }

    // -- unit 2: issues pipeline ----------------------------------------------

    #[test]
    fn base_applies_manager_scope_twice_and_viewer_scope() {
        let sql = profile_issues_from_where();
        // Outer scope (i/s/p) + inner id-set scope (i2/s2/p2).
        assert!(sql.contains("i2.deleted_at IS NULL"), "{sql}");
        assert!(sql.contains("i.deleted_at IS NULL"), "{sql}");
        assert!(
            sql.contains(
                "(ia.assignee_id = $1 OR i2.created_by_id = $1 OR sub.subscriber_id = $1)"
            ),
            "{sql}"
        );
        // Django never joins `users` here (the __in lookup targets the
        // through FK directly).
        assert!(!sql.contains("JOIN users"), "{sql}");
        // Requester's OWN memberships — :146-147, ported as-is.
        assert!(
            sql.contains("rpm.member_id = $3 AND rpm.is_active = TRUE"),
            "{sql}"
        );
        // OR legs LEFT-join; no deleted condition anywhere on them.
        assert!(
            sql.contains("LEFT JOIN issue_assignees ia ON ia.issue_id = i2.id"),
            "{sql}"
        );
        assert!(
            sql.contains("LEFT JOIN issue_subscribers sub ON sub.issue_id = i2.id"),
            "{sql}"
        );
    }

    #[test]
    fn annotations_count_zero_when_empty() {
        // Func(Count) is not an Aggregate: no GROUP BY, scalar aggregate
        // yields 0 when empty — never NULL (str(qs.query), Django 4.2.30).
        let sql = profile_issues_annotations_sql();
        assert!(sql.contains("AS cycle_id"), "{sql}");
        assert!(
            sql.contains("ORDER BY ci.created_at DESC LIMIT 1) AS cycle_id"),
            "{sql}"
        );
        assert!(!sql.contains("NULLIF"), "{sql}");
        assert_eq!(sql.matches("SELECT COUNT(*)").count(), 3, "{sql}");
        assert!(sql.contains("fa.entity_type = 'ISSUE_ATTACHMENT'"), "{sql}");
        assert!(sql.contains("AS link_count"), "{sql}");
        assert!(sql.contains("AS attachment_count"), "{sql}");
        assert!(sql.contains("AS sub_issues_count"), "{sql}");
        assert!(sql.contains("c.parent_id = i.id"), "{sql}");
        // Direct-manager subqueries DO carry the deleted guard.
        assert!(sql.contains("il.deleted_at IS NULL"), "{sql}");
        assert!(sql.contains("fa.deleted_at IS NULL"), "{sql}");
        assert!(sql.contains("ci.deleted_at IS NULL"), "{sql}");
    }

    #[test]
    fn pipeline_order_defaults_to_created_at_desc() {
        let spec = profile_issues_order(None, "s.\"group\"", |name| format!("min_{name}"));
        assert_eq!(spec.order_by_sql, "-created_at");
        assert_eq!(spec.out_param, "-created_at");
        // Delegates to the merged kernel (priority quirk intact).
        let spec = profile_issues_order(Some("-priority"), "s.\"group\"", |name| {
            format!("min_{name}")
        });
        assert_eq!(spec.out_param, "priority_order");
        assert!(spec
            .order_by_sql
            .contains("WHEN issue.priority = 'urgent' THEN 0"));
    }

    #[test]
    fn group_mismatch_matches_pilot2_guard() {
        let hit = profile_group_mismatch(Some("priority"), Some("priority")).expect("mismatch");
        assert_eq!(hit.key, "error");
        assert_eq!(
            hit.body(),
            r#"{"error":"Group by and sub group by cannot have same parameters"}"#
        );
        assert!(profile_group_mismatch(None, Some("priority")).is_none());
        assert!(profile_group_mismatch(Some("priority"), Some("")).is_none());
        assert!(profile_group_mismatch(Some("priority"), Some("state__group")).is_none());
    }

    #[test]
    fn count_filter_pins_intake_statuses() {
        let join = grouped_count_filter_join("i");
        assert!(
            join.contains("LEFT JOIN intake_issues ii ON ii.issue_id = i.id"),
            "{join}"
        );
        assert!(!join.contains("deleted_at"), "{join}");
        let predicate = grouped_count_filter_sql("i");
        assert!(
            predicate.contains("ii.status IN (1, -1, 2) OR ii.id IS NULL"),
            "{predicate}"
        );
        assert!(
            predicate.contains("i.archived_at IS NULL AND i.is_draft = FALSE"),
            "{predicate}"
        );
    }

    #[test]
    fn group_values_static_branches_reuse_kernels() {
        assert_eq!(
            profile_group_values("priority"),
            Some(GroupValuesSource::Static(PRIORITY_VALUES))
        );
        assert_eq!(
            PRIORITY_VALUES,
            &["low", "medium", "high", "urgent", "none"]
        );
        assert_eq!(
            profile_group_values("state__group"),
            Some(GroupValuesSource::Static(STATE_GROUP_VALUES))
        );
        assert_eq!(
            STATE_GROUP_VALUES,
            &[
                "backlog",
                "unstarted",
                "started",
                "review",
                "test",
                "completed",
                "cancelled"
            ]
        );
    }

    #[test]
    fn group_values_db_branches_are_workspace_scoped() {
        // state_id keeps sequence ordering (State Meta).
        let GroupValuesSource::Sql(state) = profile_group_values("state_id").expect("state") else {
            panic!("state_id must be Sql");
        };
        assert!(state.contains("s.is_triage = FALSE"), "{state}");
        // StateManager excludes group='triage' on top (db/models/state.py).
        assert!(state.contains("NOT (s.\"group\" = 'triage')"), "{state}");
        assert!(state.contains("ORDER BY s.sequence"), "{state}");
        // assignees__id reads workspace members (no-project branch).
        let GroupValuesSource::Sql(members) =
            profile_group_values("assignees__id").expect("assignees")
        else {
            panic!("assignees__id must be Sql");
        };
        assert!(members.contains("FROM workspace_members wm"), "{members}");
        assert!(members.contains("wm.is_active = TRUE"), "{members}");
        for field in [
            "labels__id",
            "issue_module__module_id",
            "cycle_id",
            "project_id",
        ] {
            let GroupValuesSource::Sql(sql) = profile_group_values(field).expect(field) else {
                panic!("{field} must be Sql");
            };
            assert!(sql.contains("w.slug = $1"), "{field}: {sql}");
            assert!(sql.contains("ORDER BY"), "{field}: {sql}");
        }
        // "None"-sentinel asymmetry.
        assert!(group_values_appends_none("labels__id"));
        assert!(group_values_appends_none("issue_module__module_id"));
        assert!(group_values_appends_none("cycle_id"));
        assert!(!group_values_appends_none("state_id"));
        assert!(!group_values_appends_none("assignees__id"));
        assert!(!group_values_appends_none("project_id"));
        // Unknown fields → no values.
        assert_eq!(profile_group_values("bogus"), None);
    }

    #[test]
    fn distinct_branches_carry_extra_ordering_select() {
        assert_eq!(
            profile_group_values("target_date"),
            Some(GroupValuesSource::DistinctOverFilteredSet("i.target_date"))
        );
        assert_eq!(
            profile_group_values("start_date"),
            Some(GroupValuesSource::DistinctOverFilteredSet("i.start_date"))
        );
        assert_eq!(
            profile_group_values("created_by"),
            Some(GroupValuesSource::DistinctOverFilteredSet(
                "i.created_by_id"
            ))
        );
        let sql = group_values_distinct_sql("i.target_date", "FROM issues i WHERE TRUE");
        assert!(
            sql.contains("SELECT DISTINCT i.target_date, i.created_at"),
            "{sql}"
        );
        assert!(sql.contains("ORDER BY i.created_at DESC"), "{sql}");
        assert!(!sql.contains("IS NOT NULL"), "{sql}");
    }

    // -- unit 3: activity -----------------------------------------------------

    #[test]
    fn activity_excludes_four_fields_and_archived_projects() {
        assert_eq!(
            ACTIVITY_EXCLUDED_FIELDS,
            &["comment", "vote", "reaction", "draft"]
        );
        let sql = user_activity_from_where(None);
        // ~Q(field__in) is NULL-inclusive: Django's exact form.
        assert!(
            sql.contains(
                "NOT (a.field IN ('comment', 'vote', 'reaction', 'draft') \
                 AND a.field IS NOT NULL)"
            ),
            "{sql}"
        );
        // :382 adds the archived filter the export-CSV twin lacks.
        assert!(sql.contains("p.archived_at IS NULL"), "{sql}");
        assert!(sql.contains("a.actor_id = $1"), "{sql}");
        assert!(sql.contains("a.deleted_at IS NULL"), "{sql}");
        // select_related four: actor INNER (promoted under actor_id=),
        // issue LEFT, workspace/project reused.
        assert!(
            sql.contains("JOIN users actor_u ON actor_u.id = a.actor_id"),
            "{sql}"
        );
        assert!(!sql.contains("LEFT JOIN users actor_u"), "{sql}");
        assert!(
            sql.contains("LEFT JOIN issues i ON i.id = a.issue_id"),
            "{sql}"
        );
        assert!(!sql.contains("project__in"), "{sql}");
    }

    #[test]
    fn activity_project_filter_is_optional() {
        let sql = user_activity_from_where(Some("p.id IN ($4)"));
        assert!(sql.contains("AND (p.id IN ($4))"), "{sql}");
    }

    #[test]
    fn me_activities_scope_by_actor_only() {
        let sql = me_activities_from_where();
        assert!(sql.contains("a.actor_id = $1"), "{sql}");
        assert!(!sql.contains("slug"), "{sql}");
        assert!(!sql.contains("project_members"), "{sql}");
        assert!(!sql.contains("NOT IN"), "{sql}");
        // Same INNER promotion as the workspace route (actor_id= equality).
        assert!(
            sql.contains("JOIN users actor_u ON actor_u.id = a.actor_id"),
            "{sql}"
        );
        assert!(!sql.contains("LEFT JOIN users actor_u"), "{sql}");
    }

    #[test]
    fn project_uuids_accept_django_forms() {
        let hyphenated = "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa";
        let simple = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let out = validate_project_uuids(&[hyphenated, simple]).expect("valid");
        assert_eq!(out[0], out[1]);
        // CPython accepts these (verified against Lib/uuid.py): braced,
        // bare uuid: prefix, urn:uuid: prefix, multi/mismatched braces,
        // free-placed hyphens (only the 32-hex count matters).
        for good in [
            "{aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa}",
            "uuid:aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
            "urn:uuid:aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
            "{{aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa}}",
            "{aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
            "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa}",
            "aaaa--aaaaaaaa--aaaaaaaa--aaaaaaaa--aaaa",
            "-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-",
        ] {
            let parsed = validate_project_uuids(&[good]).expect(good);
            assert_eq!(parsed[0], out[0], "{good}");
        }
        // CPython rejects these (verified): non-hex, whitespace padding
        // (length check), and URN:UUID: (case-sensitive replace).
        for bad in [
            "not-a-uuid",
            " aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa ",
            "URN:UUID:aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
            "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaa",
        ] {
            validate_project_uuids(&[bad]).expect_err(bad);
        }
        let err = validate_project_uuids(&["not-a-uuid"]).expect_err("invalid");
        assert_eq!(err.value, "not-a-uuid");
        assert_eq!(PROJECT_UUID_ERROR_STATUS, 400);
        assert_eq!(
            PROJECT_UUID_ERROR_BODY,
            r#"{"error":"Please provide valid detail"}"#
        );
    }

    // -- unit 4: graphs -------------------------------------------------------

    #[test]
    fn activity_graph_casts_and_groups_by_date() {
        assert_eq!(ACTIVITY_GRAPH_MONTHS, 6);
        let sql = activity_graph_sql();
        assert!(
            sql.contains("CAST(a.created_at AS DATE) AS created_date"),
            "{sql}"
        );
        assert!(
            sql.contains("COUNT(CAST(a.created_at AS DATE)) AS activity_count"),
            "{sql}"
        );
        // __date asymmetry: the filter converts, the Cast does not.
        assert!(
            sql.contains("(a.created_at AT TIME ZONE UTC)::date >= $3"),
            "{sql}"
        );
        assert!(
            sql.contains("GROUP BY CAST(a.created_at AS DATE) ORDER BY created_date"),
            "{sql}"
        );
    }

    #[test]
    fn completed_graph_buckets_mod_four() {
        let sql = completed_graph_sql();
        // All three EXTRACTs convert AT TIME ZONE UTC (USE_TZ, TIME_ZONE=UTC).
        assert!(
            sql.contains("(EXTRACT(WEEK FROM i.completed_at AT TIME ZONE UTC)::INT % 4) AS week"),
            "{sql}"
        );
        assert!(
            sql.contains(
                "COUNT(EXTRACT(WEEK FROM i.completed_at AT TIME ZONE UTC)) AS completed_count"
            ),
            "{sql}"
        );
        assert!(
            sql.contains("EXTRACT(MONTH FROM i.completed_at AT TIME ZONE UTC) = $3"),
            "{sql}"
        );
        assert!(sql.contains("i.completed_at IS NOT NULL"), "{sql}");
        assert!(sql.contains("ORDER BY week"), "{sql}");
    }

    #[test]
    fn months_ago_clamps_end_of_month() {
        use chrono::NaiveDate;
        let date = |y, m, d| NaiveDate::from_ymd_opt(y, m, d).expect("valid test date");
        assert_eq!(months_ago(date(2026, 8, 15), 6), date(2026, 2, 15));
        assert_eq!(months_ago(date(2026, 8, 31), 6), date(2026, 2, 28));
        assert_eq!(months_ago(date(2026, 3, 31), 1), date(2026, 2, 28));
        assert_eq!(months_ago(date(2024, 3, 31), 1), date(2024, 2, 29));
        assert_eq!(months_ago(date(2026, 1, 15), 3), date(2025, 10, 15));
        assert_eq!(months_ago(date(2026, 5, 15), 3), date(2026, 2, 15));
    }

    #[test]
    fn month_param_defaults_to_one_and_parses_ints() {
        assert_eq!(MONTH_DEFAULT, 1);
        assert_eq!(parse_month_param(None), Ok(1));
        assert_eq!(parse_month_param(Some("3")), Ok(3));
        assert_eq!(parse_month_param(Some(" 4 ")), Ok(4));
        assert_eq!(parse_month_param(Some("+5")), Ok(5));
        assert_eq!(parse_month_param(Some("0")), Ok(0));
        assert_eq!(parse_month_param(Some("13")), Ok(13));
        assert_eq!(parse_month_param(Some("1_2")), Ok(12));
        // Overflow saturates (parse_per_page precedent), matching no rows
        // like Python's unbounded int — never a 500.
        assert_eq!(
            parse_month_param(Some("99999999999999999999999")),
            Ok(i64::MAX)
        );
        assert_eq!(
            parse_month_param(Some("-99999999999999999999999")),
            Ok(i64::MIN)
        );
        assert_eq!(
            parse_month_param(Some("99999999999999999999999999999999999999999999")),
            Ok(i64::MAX)
        );
        for bad in ["", "abc", "3.0", "1__2", "_1", "1_", "-", "+", "3 months"] {
            assert!(parse_month_param(Some(bad)).is_err(), "{bad:?} must 500");
        }
        assert_eq!(MONTH_INVALID_STATUS, 500);
        assert_eq!(
            SERVER_ERROR_BODY,
            r#"{"error":"Something went wrong please try again later"}"#
        );
    }

    // -- unit 5: user-props + profile ------------------------------------------

    #[test]
    fn not_found_body_matches_handle_exception() {
        assert_eq!(OBJECT_NOT_FOUND_STATUS, 404);
        assert_eq!(
            OBJECT_NOT_FOUND_BODY,
            r#"{"error":"The required object does not exist."}"#
        );
        // Fixture deviation: missing users 404 (DoesNotExist path), not 500.
        assert_eq!(USER_GET_MISS_STATUS, 404);
    }

    #[test]
    fn workspace_lookup_is_slug_scoped_get() {
        let sql = workspace_lookup_sql();
        assert!(
            sql.contains("w.slug = $1 AND w.deleted_at IS NULL LIMIT 1"),
            "{sql}"
        );
    }

    #[test]
    fn user_props_lookup_and_insert_cover_all_columns() {
        let lookup = user_props_lookup_sql();
        assert!(lookup.contains(wup::TABLE), "{lookup}");
        assert!(
            lookup.contains("workspace_id = $1 AND user_id = $2 AND deleted_at IS NULL LIMIT 1"),
            "{lookup}"
        );
        for column in wup::COLUMNS {
            assert!(lookup.contains(column), "{column} missing: {lookup}");
        }
        let insert = user_props_insert_sql();
        assert!(
            insert.starts_with(&format!("INSERT INTO \"{}\"", wup::TABLE)),
            "{insert}"
        );
        for (index, column) in wup::COLUMNS.iter().enumerate() {
            assert!(insert.contains(&format!("\"{column}\"")), "{insert}");
            assert!(insert.contains(&format!("${}", index + 1)), "{insert}");
        }
        // Insert defaults come from the merged models module.
        assert!(wup::default_filters().is_object());
        assert_eq!(wup::DEFAULT_RICH_FILTERS, "{}");
        assert_eq!(wup::DEFAULT_NAVIGATION_PROJECT_LIMIT, 10);
        assert_eq!(wup::DEFAULT_NAVIGATION_CONTROL_PREFERENCE, "ACCORDION");
    }

    #[test]
    fn user_data_keys_match_contract_set() {
        // contract-tests/app_workspace/test_workspace_extras.py::USER_DATA_KEYS.
        let expected: HashSet<&str> = [
            "email",
            "first_name",
            "last_name",
            "avatar_url",
            "cover_image_url",
            "date_joined",
            "user_timezone",
            "display_name",
        ]
        .into_iter()
        .collect();
        let got: HashSet<&str> = USER_DATA_KEYS.iter().copied().collect();
        assert_eq!(got, expected);
        assert_eq!(PROFILE_RESPONSE_KEYS, &["project_data", "user_data"]);
        let sql = profile_user_sql();
        for column in [
            "email",
            "first_name",
            "last_name",
            "avatar_asset_id",
            "cover_image_asset_id",
            "date_joined",
            "user_timezone",
            "display_name",
        ] {
            assert!(sql.contains(column), "{column} missing: {sql}");
        }
        // No soft-delete scope: stock UserManager.
        assert!(!sql.contains("deleted_at"), "{sql}");
    }

    #[test]
    fn role_branch_uses_literal_fifteen() {
        assert_eq!(ROLE_MEMBER_MIN, 15);
        assert!(!requester_sees_projects(5));
        assert!(!requester_sees_projects(14));
        assert!(requester_sees_projects(15));
        assert!(requester_sees_projects(20));
        let sql = requester_role_sql();
        assert!(
            sql.contains("wm.member_id = $2 AND wm.is_active = TRUE"),
            "{sql}"
        );
        assert!(sql.contains("wm.deleted_at IS NULL LIMIT 1"), "{sql}");
    }

    #[test]
    fn projects_query_is_single_query_four_filters() {
        let sql = profile_projects_sql();
        assert_eq!(sql.matches("COUNT(pi.id) FILTER").count(), 4, "{sql}");
        // Pending trio is the literal backlog/unstarted/started — not CLOSED.
        assert!(
            sql.contains("s.\"group\" IN ('backlog', 'unstarted', 'started')"),
            "{sql}"
        );
        assert!(!sql.contains("'cancelled'"), "{sql}");
        assert!(!sql.contains("'review'"), "{sql}");
        // Shared multi-valued joins (fanout ported): exactly one of each.
        assert_eq!(sql.matches("LEFT JOIN issues pi ON").count(), 1, "{sql}");
        assert_eq!(
            sql.matches("LEFT JOIN issue_assignees ia ON").count(),
            1,
            "{sql}"
        );
        assert_eq!(sql.matches("LEFT JOIN states s ON").count(), 1, "{sql}");
        // Joined rows unguarded (deleted/triage count); project root guarded.
        assert!(
            sql.contains("p.archived_at IS NULL AND p.deleted_at IS NULL"),
            "{sql}"
        );
        assert!(!sql.contains("pi.deleted_at"), "{sql}");
        assert!(!sql.contains("ia.deleted_at"), "{sql}");
        // Grouped queries drop Meta ordering: GROUP BY but NO ORDER BY.
        assert!(sql.contains("GROUP BY p.id, p.logo_props"), "{sql}");
        assert!(!sql.contains("ORDER BY"), "{sql}");
    }
}
