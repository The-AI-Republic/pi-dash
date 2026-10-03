#![forbid(unsafe_code)]

//! Retrieve / sub-issue / relation+link / archive read querysets (D-26 queries-A).
//!
//! Ports the annotated read querysets behind the issue detail, sub-issue,
//! relation, link and archive endpoints to SQL text, extending pilot-2's
//! api-crate query pattern (`super::annotation_selects` / `super::base_where`,
//! [`Binder`] `$n` binds). Each builder returns a fragment or a complete
//! statement the handler executes; handlers own auth gates, filter stacks,
//! ordering variants, pagination and response shaping.
//!
//! Sources (drift baseline `01a93e17`):
//! - `app/views/issue/base.py:486-620` — `IssueViewSet.retrieve` (base
//!   `:489-495`, cycle/count/array annotations `:496-558`, reaction/link
//!   prefetches `:559-570`, `is_subscribed` `:571-580`, guest 403 `:593-607`).
//! - `app/views/issue/sub_issue.py:37-201` — `SubIssuesEndpoint.get`
//!   (annotations `:38-131`, ordering `:133-138`, `.values(26)` `:140-168`).
//! - `app/views/issue/relation.py:42-207` — `IssueRelationViewSet.list`
//!   (relation scope `:42-50`, ten id-lists `:52-100`, target queryset
//!   `:102-154`, eight buckets `:174-207`).
//! - `app/views/issue/link.py:32-46` — `IssueLinkViewSet.get_queryset`.
//! - `app/views/issue/archive.py:54-255` — `IssueArchiveViewSet`
//!   (`apply_annotations` `:61-96`, `get_queryset` `:98-104`, list `:108-219`,
//!   retrieve `:222-255`).
//!
//! Fixture oracle: FX-ISS-11
//! (`rust-api/fixtures/app_issues/queries/FX-ISS-11.core.json`). Every builder
//! below was verified against live-compiled Django 4.2.30 ORM SQL
//! (`str(qs.query)` — no database needed); the unit tests pin the verified
//! text so transcription drift fails the build.
//!
//! Reuse vs pilot-2 (deliberate, verified live):
//! - The `NULLIF(COUNT(*), 0)` count idiom and the `cycle_id` / `label_ids`
//!   subquery texts are pilot-2's, reused verbatim — Django renders the same
//!   predicates as a grouped `COUNT(id)` subquery, which returns no row (SQL
//!   `NULL`) exactly when `NULLIF` yields `NULL`. Pin tests assert the shared
//!   texts still match `super::annotation_selects`.
//! - Three pilot-2 guards are deliberately NOT inherited: the joined-row
//!   `project_members.deleted_at` guard on the active-member assignee array,
//!   the joined-row `modules.deleted_at` guard on the module array, and the
//!   `states.deleted_at` guard on joined state rows. Django applies only the
//!   base model's manager to a query — `filter()` joins and `select_related`
//!   never carry the related model's soft-delete guard (live SQL shows none
//!   of the three). The builders here match Django; pilot-2's merged list
//!   paths keep their disclosed forms (out of scope for this issue).
//!
//! Existing bugs ported as-is (translation, don't redesign):
//! 1. `relation.py`'s `duplicate` / `relates_to` `|` combinations compile to
//!    a single `SELECT` with OR-ed `WHERE` branches — not a SQL `UNION`
//!    (FX-ISS-11 says UNION; live SQL shows OR). Ported as OR.
//! 2. The relation target queryset's `cycle_id` / count annotations,
//!    `select_related` and prefetches never reach SQL: `.values(*fields)`
//!    selects only the 14 bucket keys, and Django renders joins solely for
//!    referenced relations. The dead chains are documented, not emitted.
//! 3. The relation id-lists `SELECT DISTINCT issue_id, created_at`: the
//!    ordering column rides along because `DISTINCT` + `ORDER BY` requires
//!    it. Ported with the extra column.
//! 4. The relation bucket queries carry no `ORDER BY`: the direct-`ArrayAgg`
//!    `GROUP BY` drops the model's default ordering. Ported without order.
//! 5. Sub-issue `.values(26)` renders concrete columns in call order, then
//!    annotations in `annotate()` order — so `module_ids` (called third)
//!    selects after `label_ids`/`assignee_ids`. [`sub_issues_selects`]
//!    keeps Django's render order.
//! 6. Explicit `deleted_at__isnull` guards on sub-issue/archive-cycle and
//!    sub-issue arrays double the manager's guard (`deleted_at IS NULL AND
//!    deleted_at IS NULL`). One guard is ported; the doubling is unobservable.
//! 7. Link visibility ignores soft-deleted memberships: the member join has
//!    no `deleted_at` guard, so a deleted membership still shows links.
//! 8. `NOT issue_types.is_epic` (not `is_epic = FALSE`): three-valued logic
//!    drops `NULL`-flag rows unless `type_id IS NULL`. Ported as `NOT`.
//! 9. The relation `Func(F('id'), function='Count')` counts would return `0`
//!    (scalar aggregate, no `GROUP BY`) where every sibling path yields
//!    `NULL` — unobservable, since the bucket `.values()` never selects them.
//!
//! Out of scope (owned by sibling handler/guard issues): the permission gates
//! (`resolve_gate`, `@allow_permission`), rich + legacy filter stacks
//! (`complex_filter`, `legacy_sql`), ordering variants (`ordering.rs`,
//! `order_key`), pagination envelopes (`render.rs`, groupers), the archive
//! grouped-count path (uses [`archive_count_filter_sql`] plus pilot-2's
//! grouped paginator), serializer shapes and `handle_exception` bodies.
//! Queryset-adjacent constants handlers need (`.values()` key orders, the
//! `IN`-list bucket table, default orders) ARE in scope and marked.

use super::Binder;

// ---------------------------------------------------------------------------
// Shared column lists (Django concrete-field order, live-compiled)
// ---------------------------------------------------------------------------

/// All 34 concrete `issues` columns in Django's select order, qualified with
/// the pilot-2 `issue` alias so these fragments compose with
/// `complex_filter` / `legacy_sql` / `order_key` output.
pub const ISSUE_COLUMNS: &str = "issue.created_at, issue.updated_at, issue.created_by_id, \
    issue.updated_by_id, issue.deleted_at, issue.id, issue.project_id, issue.workspace_id, \
    issue.parent_id, issue.state_id, issue.point, issue.estimate_point_id, issue.name, \
    issue.description_json, issue.description_html, issue.description_stripped, \
    issue.description_binary, issue.priority, issue.complexity_score, issue.start_date, \
    issue.target_date, issue.sequence_id, issue.sort_order, issue.completed_at, \
    issue.archived_at, issue.is_draft, issue.external_source, issue.external_id, \
    issue.type_id, issue.git_work_branch, issue.workpad, issue.created_via, \
    issue.assigned_pod_id, issue.agent_executor";

/// All 19 concrete `states` columns (`select_related("state")` order).
/// `group`/`default` stay quoted: reserved words.
pub const STATE_COLUMNS: &str = "state.created_at, state.updated_at, state.created_by_id, \
    state.updated_by_id, state.deleted_at, state.id, state.project_id, state.workspace_id, \
    state.name, state.description, state.color, state.slug, state.sequence, state.\"group\", \
    state.is_triage, state.\"default\", state.external_source, state.external_id";

/// All 11 concrete `issue_reactions` columns (prefetch base-table order).
pub const ISSUE_REACTION_COLUMNS: &str = "issue_reactions.created_at, issue_reactions.updated_at, \
    issue_reactions.created_by_id, issue_reactions.updated_by_id, issue_reactions.deleted_at, \
    issue_reactions.id, issue_reactions.project_id, issue_reactions.workspace_id, \
    issue_reactions.actor_id, issue_reactions.issue_id, issue_reactions.reaction";

/// All 12 concrete `issue_links` columns (prefetch / list base-table order).
pub const ISSUE_LINK_COLUMNS: &str = "issue_links.created_at, issue_links.updated_at, \
    issue_links.created_by_id, issue_links.updated_by_id, issue_links.deleted_at, \
    issue_links.id, issue_links.project_id, issue_links.workspace_id, issue_links.title, \
    issue_links.url, issue_links.issue_id, issue_links.metadata";

/// All 50 concrete `users` columns (`select_related("actor")` /
/// `select_related("created_by")` order). The auth user has no `deleted_at`.
pub const USER_COLUMNS: &str = "users.password, users.last_login, users.id, users.username, \
    users.mobile_number, users.email, users.display_name, users.first_name, users.last_name, \
    users.avatar, users.avatar_asset_id, users.cover_image, users.cover_image_asset_id, \
    users.date_joined, users.created_at, users.updated_at, users.last_location, \
    users.created_location, users.is_superuser, users.is_managed, users.is_password_expired, \
    users.is_active, users.is_staff, users.is_email_verified, users.is_password_autoset, \
    users.is_password_reset_required, users.token, users.last_active, users.last_login_time, \
    users.last_logout_time, users.last_login_ip, users.last_logout_ip, users.last_login_medium, \
    users.last_login_uagent, users.token_updated_at, users.is_bot, users.bot_type, \
    users.user_timezone, users.is_email_valid, users.masked_at";

/// All 46 concrete `projects` columns (`Project.objects.get` order).
pub const PROJECT_COLUMNS: &str = "projects.created_at, projects.updated_at, \
    projects.created_by_id, projects.updated_by_id, projects.deleted_at, projects.id, \
    projects.name, projects.description, projects.description_text, projects.description_html, \
    projects.network, projects.workspace_id, projects.identifier, projects.default_assignee_id, \
    projects.project_lead_id, projects.emoji, projects.icon_prop, projects.module_view, \
    projects.cycle_view, projects.issue_views_view, projects.page_view, projects.intake_view, \
    projects.is_time_tracking_enabled, projects.is_issue_type_enabled, projects.is_default, \
    projects.guest_view_all_features, projects.members_can_edit_states, projects.cover_image, \
    projects.cover_image_asset_id, projects.estimate_id, projects.archive_in, projects.close_in, \
    projects.logo_props, projects.default_state_id, projects.archived_at, projects.timezone, \
    projects.external_source, projects.external_id, projects.repo_url, projects.base_branch, \
    projects.agent_default_interval_seconds, projects.agent_default_max_ticks, \
    projects.agent_review_default_interval_seconds, projects.agent_test_default_interval_seconds, \
    projects.agent_ticking_enabled, projects.default_agent_executor";

/// `issues` columns prefixed for a self-join alias (reaction-prefetch
/// `select_related("issue")` renders the full issue row under the `issues`
/// table name).
pub const PREFETCH_ISSUE_COLUMNS: &str = "issues.created_at, issues.updated_at, \
    issues.created_by_id, issues.updated_by_id, issues.deleted_at, issues.id, \
    issues.project_id, issues.workspace_id, issues.parent_id, issues.state_id, issues.point, \
    issues.estimate_point_id, issues.name, issues.description_json, issues.description_html, \
    issues.description_stripped, issues.description_binary, issues.priority, \
    issues.complexity_score, issues.start_date, issues.target_date, issues.sequence_id, \
    issues.sort_order, issues.completed_at, issues.archived_at, issues.is_draft, \
    issues.external_source, issues.external_id, issues.type_id, issues.git_work_branch, \
    issues.workpad, issues.created_via, issues.assigned_pod_id, issues.agent_executor";

// ---------------------------------------------------------------------------
// Scalar annotations shared by retrieve / sub-issues / archive-list
// ---------------------------------------------------------------------------

/// `cycle_id` (`base.py:496`, `sub_issue.py:40-44`, `archive.py:63-67`).
/// Same text as pilot-2's: Django's `ORDER BY created_at DESC` (model
/// ordering) is dropped — the partial unique constraint leaves one live row,
/// so the order cannot change the result.
pub const CYCLE_ID_SELECT: &str = "(SELECT ci.cycle_id FROM cycle_issues ci \
    WHERE ci.issue_id = issue.id AND ci.deleted_at IS NULL LIMIT 1) AS cycle_id";

/// `link_count` (`base.py:497-504`, `sub_issue.py:45-57`, `archive.py:68-75`,
/// `relation.py:112-117` as dead code). `NULLIF(COUNT(*), 0)` reproduces
/// Django's grouped `COUNT(id)` subquery, which yields no row — SQL `NULL` —
/// when empty. `coalesce_zero` selects the sub-issue form (`COALESCE(..., 0)`).
pub fn link_count_select(coalesce_zero: bool) -> String {
    let inner = "(SELECT NULLIF(COUNT(*), 0) FROM issue_links il \
        WHERE il.issue_id = issue.id AND il.deleted_at IS NULL)";
    if coalesce_zero {
        format!("COALESCE({inner}, 0) AS link_count")
    } else {
        format!("{inner} AS link_count")
    }
}

/// `attachment_count` (`base.py:505-515`, `sub_issue.py:58-73`,
/// `archive.py:76-86`). Same `NULLIF` equivalence as [`link_count_select`].
pub fn attachment_count_select(coalesce_zero: bool) -> String {
    let inner = "(SELECT NULLIF(COUNT(*), 0) FROM file_assets fa \
        WHERE fa.issue_id = issue.id AND fa.entity_type = 'ISSUE_ATTACHMENT' \
        AND fa.deleted_at IS NULL)";
    if coalesce_zero {
        format!("COALESCE({inner}, 0) AS attachment_count")
    } else {
        format!("{inner} AS attachment_count")
    }
}

/// `sub_issues_count` over the `issue_objects` manager scope
/// (`base.py:516-523`, `sub_issue.py:74-86`, `archive.py:87-94`). Same shape
/// as pilot-2's, minus the joined-`states` soft-delete guard Django never
/// emits (see module docs): a soft-deleted triage state still excludes the
/// child here, exactly as in Django.
pub fn sub_issues_count_select(coalesce_zero: bool) -> String {
    let inner = "(SELECT NULLIF(COUNT(*), 0) FROM issues c \
        LEFT JOIN states cs ON cs.id = c.state_id \
        JOIN projects cp ON cp.id = c.project_id \
        WHERE c.parent_id = issue.id AND c.deleted_at IS NULL \
        AND NOT (cs.\"group\" = 'triage' AND cs.\"group\" IS NOT NULL) \
        AND c.archived_at IS NULL AND cp.archived_at IS NULL AND c.is_draft = FALSE)";
    if coalesce_zero {
        format!("COALESCE({inner}, 0) AS sub_issues_count")
    } else {
        format!("{inner} AS sub_issues_count")
    }
}

/// `label_ids` (`base.py:525-533`, `sub_issue.py:88-98`). Same text as
/// pilot-2's (pin-tested): manager soft-delete guard only.
pub const LABEL_IDS_SELECT: &str =
    "(SELECT COALESCE(ARRAY_AGG(DISTINCT il.label_id), '{}'::uuid[]) \
    FROM issue_labels il WHERE il.issue_id = issue.id AND il.deleted_at IS NULL) AS label_ids";

/// `assignee_ids` with the active-member join (`base.py:534-545`,
/// `sub_issue.py:99-113`). Faithful to Django: the `users` +
/// `project_members` joins carry NO soft-delete guard (`filter()` joins never
/// apply the related manager), unlike pilot-2's shared `EXISTS` form which
/// adds `pm.deleted_at IS NULL`.
pub const ASSIGNEE_IDS_ACTIVE_SELECT: &str =
    "(SELECT COALESCE(ARRAY_AGG(DISTINCT ia.assignee_id), '{}'::uuid[]) \
    FROM issue_assignees ia \
    JOIN users u ON u.id = ia.assignee_id \
    JOIN project_members pm ON pm.member_id = u.id \
    WHERE ia.issue_id = issue.id AND ia.deleted_at IS NULL AND pm.is_active) AS assignee_ids";

/// `module_ids` with the archived-module guard (`base.py:546-558`,
/// `sub_issue.py:114-129`). Faithful to Django: NO `modules.deleted_at`
/// guard (pilot-2's shared form adds one).
pub const MODULE_IDS_SELECT: &str =
    "(SELECT COALESCE(ARRAY_AGG(DISTINCT mi.module_id), '{}'::uuid[]) \
    FROM module_issues mi JOIN modules m ON m.id = mi.module_id \
    WHERE mi.issue_id = issue.id AND mi.deleted_at IS NULL \
    AND m.archived_at IS NULL) AS module_ids";

/// `is_subscribed` `Exists` (`base.py:571-580`, `archive.py:238-248`).
/// Binds, in order: project id, subscriber id, workspace slug.
pub fn is_subscribed_select(
    binder: &mut Binder,
    slug: &str,
    project_id: uuid::Uuid,
    user_id: uuid::Uuid,
) -> String {
    let project = binder.bind_uuid(project_id);
    let user = binder.bind_uuid(user_id);
    let slug_holder = binder.bind_string(slug.to_owned());
    format!(
        "EXISTS(SELECT 1 AS a FROM issue_subscribers s \
        JOIN workspaces sw ON sw.id = s.workspace_id \
        WHERE s.deleted_at IS NULL AND s.issue_id = issue.id AND s.project_id = {project} \
        AND s.subscriber_id = {user} AND sw.slug = {slug_holder} LIMIT 1) AS is_subscribed"
    )
}

// ---------------------------------------------------------------------------
// Retrieve (base.py:486-620)
// ---------------------------------------------------------------------------

/// Full retrieve statement (`base.py:489-581`, `.first()` included): all issue
/// columns, the seven annotations in `annotate()` order, `is_subscribed`,
/// then the `select_related("state")` columns — Django's exact select order.
/// `Issue.objects` (soft-delete only): triage/archived/draft rows ARE visible
/// here, unlike on the `issue_objects` paths. Binds, in order: issue id,
/// project id, workspace slug (outer `WHERE`), then the `is_subscribed`
/// binds (project id, subscriber id, slug).
pub fn retrieve_sql(
    binder: &mut Binder,
    slug: &str,
    project_id: uuid::Uuid,
    issue_id: uuid::Uuid,
    user_id: uuid::Uuid,
) -> String {
    let issue = binder.bind_uuid(issue_id);
    let project = binder.bind_uuid(project_id);
    let slug_holder = binder.bind_string(slug.to_owned());
    let subscribed = is_subscribed_select(binder, slug, project_id, user_id);
    let link = link_count_select(false);
    let attachment = attachment_count_select(false);
    let sub = sub_issues_count_select(false);
    format!(
        "SELECT {ISSUE_COLUMNS}, {CYCLE_ID_SELECT}, {link}, {attachment}, {sub}, \
        {LABEL_IDS_SELECT}, {ASSIGNEE_IDS_ACTIVE_SELECT}, {MODULE_IDS_SELECT}, {subscribed}, \
        {STATE_COLUMNS} \
        FROM issues AS issue \
        JOIN workspaces ON workspaces.id = issue.workspace_id \
        LEFT JOIN states AS state ON state.id = issue.state_id \
        WHERE issue.deleted_at IS NULL AND issue.id = {issue} \
        AND issue.project_id = {project} AND workspaces.slug = {slug_holder} \
        ORDER BY issue.created_at DESC LIMIT 1"
    )
}

/// Reaction prefetch (`Prefetch("issue_reactions",
/// IssueReaction.objects.select_related("issue", "actor"))`, `base.py:559-564`,
/// shared with archive retrieve `:226-231`). Column order follows the MODEL
/// field order (`actor` before `issue`), not the call order; both joins are
/// `INNER` (non-null FKs). One bind per parent id, in order; the single-parent
/// retrieve/archive calls pass exactly one.
pub fn reaction_prefetch_sql(binder: &mut Binder, issue_ids: &[uuid::Uuid]) -> String {
    let list = issue_ids
        .iter()
        .map(|id| binder.bind_uuid(*id))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "SELECT {ISSUE_REACTION_COLUMNS}, {USER_COLUMNS}, {PREFETCH_ISSUE_COLUMNS} \
        FROM issue_reactions \
        INNER JOIN issues ON issue_reactions.issue_id = issues.id \
        INNER JOIN users ON issue_reactions.actor_id = users.id \
        WHERE issue_reactions.deleted_at IS NULL AND issue_reactions.issue_id IN ({list}) \
        ORDER BY issue_reactions.created_at DESC"
    )
}

/// Link prefetch (`Prefetch("issue_link",
/// IssueLink.objects.select_related("created_by"))`, `base.py:565-570`,
/// shared with archive retrieve `:232-237`). `created_by` is nullable, so the
/// users join is `LEFT OUTER`.
pub fn link_prefetch_sql(binder: &mut Binder, issue_ids: &[uuid::Uuid]) -> String {
    let list = issue_ids
        .iter()
        .map(|id| binder.bind_uuid(*id))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "SELECT {ISSUE_LINK_COLUMNS}, {USER_COLUMNS} \
        FROM issue_links \
        LEFT OUTER JOIN users ON issue_links.created_by_id = users.id \
        WHERE issue_links.deleted_at IS NULL AND issue_links.issue_id IN ({list}) \
        ORDER BY issue_links.created_at DESC"
    )
}

/// `Project.objects.get(pk, workspace__slug)` (`base.py:487`). A pk lookup is
/// unique, so the handler's single-row fetch matches Django's `.get()`
/// (which over-fetches one row to detect multiples — unobservable here).
/// Binds: project id, workspace slug.
pub fn project_get_sql(binder: &mut Binder, slug: &str, project_id: uuid::Uuid) -> String {
    let project = binder.bind_uuid(project_id);
    let slug_holder = binder.bind_string(slug.to_owned());
    format!(
        "SELECT {PROJECT_COLUMNS} FROM projects \
        INNER JOIN workspaces ON projects.workspace_id = workspaces.id \
        WHERE projects.deleted_at IS NULL AND projects.id = {project} \
        AND workspaces.slug = {slug_holder} \
        ORDER BY projects.created_at DESC LIMIT 1"
    )
}

/// Guest view gate `Exists` (`base.py:593-601`): an active role-5 membership.
/// Django's `.exists()` renders `SELECT 1 ... LIMIT 1` with no `ORDER BY`.
/// Binds, in order: user id, project id, workspace slug.
pub fn guest_view_exists_sql(
    binder: &mut Binder,
    slug: &str,
    project_id: uuid::Uuid,
    user_id: uuid::Uuid,
) -> String {
    let user = binder.bind_uuid(user_id);
    let project = binder.bind_uuid(project_id);
    let slug_holder = binder.bind_string(slug.to_owned());
    format!(
        "SELECT 1 AS a FROM project_members \
        INNER JOIN workspaces ON project_members.workspace_id = workspaces.id \
        WHERE project_members.deleted_at IS NULL AND project_members.is_active \
        AND project_members.member_id = {user} AND project_members.project_id = {project} \
        AND project_members.role = 5 AND workspaces.slug = {slug_holder} LIMIT 1"
    )
}

// ---------------------------------------------------------------------------
// Sub-issues (sub_issue.py:37-201)
// ---------------------------------------------------------------------------

/// The 26 `.values()` keys in CALL order (`sub_issue.py:141-167`) — the wire
/// key order handlers render. `estimate_point` / `created_by` / `updated_by`
/// select the `*_id` columns (see [`sub_issues_selects`]).
pub const SUB_ISSUES_KEYS: &[&str] = &[
    "id",
    "name",
    "state_id",
    "sort_order",
    "completed_at",
    "estimate_point",
    "priority",
    "start_date",
    "target_date",
    "sequence_id",
    "project_id",
    "parent_id",
    "cycle_id",
    "module_ids",
    "label_ids",
    "assignee_ids",
    "sub_issues_count",
    "created_at",
    "updated_at",
    "created_by",
    "updated_by",
    "attachment_count",
    "link_count",
    "is_draft",
    "archived_at",
    "state_group",
];

/// Sub-issue `.values(26)` select list in Django's RENDER order: concrete
/// columns in call order (with `AS` aliases where the key differs), then
/// annotations in `annotate()` order (`cycle_id`, `link_count`,
/// `attachment_count`, `sub_issues_count`, `label_ids`, `assignee_ids`,
/// `module_ids`), then the `state_group` `F()` reference. Counts coalesce to
/// `0` on this path (unlike retrieve's `NULL`).
pub fn sub_issues_selects() -> String {
    let link = link_count_select(true);
    let attachment = attachment_count_select(true);
    let sub = sub_issues_count_select(true);
    format!(
        "issue.id, issue.name, issue.state_id, issue.sort_order, issue.completed_at, \
        issue.estimate_point_id AS estimate_point, issue.priority, issue.start_date, \
        issue.target_date, issue.sequence_id, issue.project_id, issue.parent_id, issue.created_at, \
        issue.updated_at, issue.created_by_id AS created_by, issue.updated_by_id AS updated_by, \
        issue.is_draft, issue.archived_at, {CYCLE_ID_SELECT}, {link}, {attachment}, {sub}, \
        {LABEL_IDS_SELECT}, {ASSIGNEE_IDS_ACTIVE_SELECT}, {MODULE_IDS_SELECT}, \
        state.\"group\" AS state_group"
    )
}

/// Sub-issue `FROM` + `WHERE`: the `issue_objects` manager scope plus
/// `parent_id` and the workspace slug (`sub_issue.py:38-39`). The manager's
/// triage exclusion shares the `state` join with `state_group`; the `project`
/// join serves the archived-project exclusion. Binds: parent issue id,
/// workspace slug.
pub fn sub_issues_from_where(binder: &mut Binder, slug: &str, parent_id: uuid::Uuid) -> String {
    let parent = binder.bind_uuid(parent_id);
    let slug_holder = binder.bind_string(slug.to_owned());
    format!(
        "FROM issues AS issue \
        LEFT JOIN states AS state ON state.id = issue.state_id \
        JOIN projects AS project ON project.id = issue.project_id \
        JOIN workspaces ON workspaces.id = issue.workspace_id \
        WHERE issue.deleted_at IS NULL \
        AND NOT (state.\"group\" = 'triage' AND state.\"group\" IS NOT NULL) \
        AND issue.archived_at IS NULL AND project.archived_at IS NULL AND issue.is_draft = FALSE \
        AND issue.parent_id = {parent} AND workspaces.slug = {slug_holder}"
    )
}

/// Default sub-issue order: `order_issue_queryset(sub_issues, "-created_at")`
/// echoes the param through (`ordering.rs`, pilot-2's).
pub const SUB_ISSUES_DEFAULT_ORDER: &str = "ORDER BY issue.created_at DESC";

/// Full default-order sub-issues statement (GET with no `order_by` override).
/// Ordering variants and the `state_distribution` / `group_by` shaping are
/// handler-owned (`ordering.rs`, `SUB_ISSUES_KEYS`).
pub fn sub_issues_sql(binder: &mut Binder, slug: &str, parent_id: uuid::Uuid) -> String {
    let selects = sub_issues_selects();
    let from_where = sub_issues_from_where(binder, slug, parent_id);
    format!("SELECT {selects} {from_where} {SUB_ISSUES_DEFAULT_ORDER}")
}

// ---------------------------------------------------------------------------
// Relations (relation.py:42-207)
// ---------------------------------------------------------------------------

/// One relation id-list: which column to return, which stored `relation_type`
/// to filter, and which side of the pair holds the current issue.
/// Django's ten `values_list` calls collapse to this triple; the eight
/// response buckets each name one (single) or two (OR-ed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RelationIds {
    /// Returned column: `issue_id` or `related_issue_id`.
    pub column: &'static str,
    /// Stored `relation_type` filter (`blocked_by`, `duplicate`, `relates_to`,
    /// `start_before`, `finish_before`).
    pub relation_type: &'static str,
    /// Side holding the current issue: `issue_id` or `related_issue_id`.
    pub side: &'static str,
}

/// The eight response buckets (`relation.py:174-207`) with their id-list(s).
/// `duplicate` and `relates_to` carry two lists, OR-ed into one statement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RelationBucket {
    /// Response key and `relation_type` literal.
    pub label: &'static str,
    /// One id-list, or two for the OR-ed buckets.
    pub ids: &'static [RelationIds],
}

pub const RELATION_BUCKETS: &[RelationBucket] = &[
    RelationBucket {
        label: "blocking",
        ids: &[RelationIds {
            column: "issue_id",
            relation_type: "blocked_by",
            side: "related_issue_id",
        }],
    },
    RelationBucket {
        label: "blocked_by",
        ids: &[RelationIds {
            column: "related_issue_id",
            relation_type: "blocked_by",
            side: "issue_id",
        }],
    },
    RelationBucket {
        label: "duplicate",
        ids: &[
            RelationIds {
                column: "related_issue_id",
                relation_type: "duplicate",
                side: "issue_id",
            },
            RelationIds {
                column: "issue_id",
                relation_type: "duplicate",
                side: "related_issue_id",
            },
        ],
    },
    RelationBucket {
        label: "relates_to",
        ids: &[
            RelationIds {
                column: "related_issue_id",
                relation_type: "relates_to",
                side: "issue_id",
            },
            RelationIds {
                column: "issue_id",
                relation_type: "relates_to",
                side: "related_issue_id",
            },
        ],
    },
    RelationBucket {
        label: "start_after",
        ids: &[RelationIds {
            column: "issue_id",
            relation_type: "start_before",
            side: "related_issue_id",
        }],
    },
    RelationBucket {
        label: "start_before",
        ids: &[RelationIds {
            column: "related_issue_id",
            relation_type: "start_before",
            side: "issue_id",
        }],
    },
    RelationBucket {
        label: "finish_after",
        ids: &[RelationIds {
            column: "issue_id",
            relation_type: "finish_before",
            side: "related_issue_id",
        }],
    },
    RelationBucket {
        label: "finish_before",
        ids: &[RelationIds {
            column: "related_issue_id",
            relation_type: "finish_before",
            side: "issue_id",
        }],
    },
];

/// One relation id-list statement (`relation.py:52-100`): `SELECT DISTINCT`
/// the id column plus the ordering column, over the `(issue OR related)`
/// scope with the workspace slug, side and type filters. The base
/// queryset's `select_related` joins never reach SQL (unreferenced by a
/// `values_list`). Binds, in order: issue id (OR left), issue id (OR right),
/// workspace slug, issue id (side), relation type.
pub fn relation_ids_sql(
    binder: &mut Binder,
    slug: &str,
    issue_id: uuid::Uuid,
    ids: RelationIds,
) -> String {
    let left = binder.bind_uuid(issue_id);
    let right = binder.bind_uuid(issue_id);
    let slug_holder = binder.bind_string(slug.to_owned());
    let side = binder.bind_uuid(issue_id);
    let relation_type = binder.bind_string(ids.relation_type.to_owned());
    format!(
        "SELECT DISTINCT issue_relations.{column}, issue_relations.created_at \
        FROM issue_relations \
        INNER JOIN workspaces ON issue_relations.workspace_id = workspaces.id \
        WHERE issue_relations.deleted_at IS NULL \
        AND (issue_relations.issue_id = {left} OR issue_relations.related_issue_id = {right}) \
        AND workspaces.slug = {slug_holder} \
        AND issue_relations.{side_col} = {side} \
        AND issue_relations.relation_type = {relation_type} \
        ORDER BY issue_relations.created_at DESC",
        column = ids.column,
        side_col = ids.side,
    )
}

/// The 14 bucket `.values()` keys in RENDER order (`relation.py:157-172`):
/// concrete columns in call order, the two direct-`ArrayAgg` annotations in
/// `annotate()` order, then the `relation_type` literal.
pub const RELATION_ROW_FIELDS: &[&str] = &[
    "id",
    "name",
    "state_id",
    "sort_order",
    "priority",
    "sequence_id",
    "project_id",
    "label_ids",
    "assignee_ids",
    "created_at",
    "updated_at",
    "created_by",
    "updated_by",
    "relation_type",
];

/// Bucket select list for one `relation_type` literal (target queryset
/// `relation.py:102-154`, keys `:157-172`). The label/assignee arrays aggregate
/// DIRECTLY over `LEFT JOIN`ed link rows with `FILTER` (not the subquery
/// form), which is why the bucket statement needs `GROUP BY issue.id`. The
/// target queryset's `cycle_id`/count annotations are dead here —
/// `.values(*fields)` never selects them — and are not emitted. Binds the
/// label literal.
pub fn relation_bucket_selects(binder: &mut Binder, label: &str) -> String {
    let literal = binder.bind_string(label.to_owned());
    format!(
        "DISTINCT issue.id, issue.name, issue.state_id, issue.sort_order, issue.priority, \
        issue.sequence_id, issue.project_id, issue.created_at, issue.updated_at, \
        issue.created_by_id, issue.updated_by_id, \
        COALESCE(ARRAY_AGG(DISTINCT issue_labels.label_id) \
        FILTER (WHERE (NOT (issue_labels.label_id IS NULL) AND issue_labels.deleted_at IS NULL)), \
        '{{}}'::uuid[]) AS label_ids, \
        COALESCE(ARRAY_AGG(DISTINCT issue_assignees.assignee_id) \
        FILTER (WHERE (NOT (issue_assignees.assignee_id IS NULL) AND project_members.is_active \
        AND issue_assignees.deleted_at IS NULL)), '{{}}'::uuid[]) AS assignee_ids, \
        {literal} AS relation_type"
    )
}

/// Bucket `FROM`: manager-scope joins plus the array-aggregation `LEFT JOIN`s
/// (`labels__id` / `assignees__id` traversal joins the link tables; the
/// active-member predicate left-joins `users` + `project_members`). None of
/// the joined tables carries a soft-delete guard — only the `FILTER` clauses
/// and the outer `WHERE` do.
pub const RELATION_BUCKET_FROM: &str = "FROM issues AS issue \
    LEFT JOIN states AS state ON state.id = issue.state_id \
    JOIN projects AS project ON project.id = issue.project_id \
    JOIN workspaces ON workspaces.id = issue.workspace_id \
    LEFT OUTER JOIN issue_labels ON issue.id = issue_labels.issue_id \
    LEFT OUTER JOIN issue_assignees ON issue.id = issue_assignees.issue_id \
    LEFT OUTER JOIN users ON issue_assignees.assignee_id = users.id \
    LEFT OUTER JOIN project_members ON users.id = project_members.member_id";

/// Bucket scope preamble: the `issue_objects` manager plus the workspace slug
/// (`relation.py:102-103`). Binds the slug.
pub fn relation_bucket_preamble(binder: &mut Binder, slug: &str) -> String {
    let slug_holder = binder.bind_string(slug.to_owned());
    format!(
        "issue.deleted_at IS NULL \
        AND NOT (state.\"group\" = 'triage' AND state.\"group\" IS NOT NULL) \
        AND issue.archived_at IS NULL AND project.archived_at IS NULL AND issue.is_draft = FALSE \
        AND workspaces.slug = {slug_holder}"
    )
}

/// Full single-list bucket statement: preamble plus `id IN (<id-list>)`,
/// grouped by the issue pk. No `ORDER BY` (bug 4). The id-list subquery drops
/// its outer `ORDER BY` (and with it the ordering column) — subqueries never
/// carry ordering.
pub fn relation_bucket_sql(
    binder: &mut Binder,
    slug: &str,
    issue_id: uuid::Uuid,
    bucket: RelationBucket,
) -> String {
    debug_assert_eq!(
        bucket.ids.len(),
        1,
        "OR-ed buckets use relation_bucket_union_sql"
    );
    relation_bucket_union_sql(binder, slug, issue_id, bucket)
}

/// Full bucket statement for one or two id-lists. Django's `|` renders the
/// two-branch form as `(preamble AND id IN A) OR (preamble AND id IN B)`;
/// the preamble is conjunctive, so the factored `preamble AND (IN A OR IN B)`
/// ported here matches row for row with half the text.
pub fn relation_bucket_union_sql(
    binder: &mut Binder,
    slug: &str,
    issue_id: uuid::Uuid,
    bucket: RelationBucket,
) -> String {
    let selects = relation_bucket_selects(binder, bucket.label);
    let preamble = relation_bucket_preamble(binder, slug);
    let mut branches = Vec::with_capacity(bucket.ids.len());
    for ids in bucket.ids {
        let left = binder.bind_uuid(issue_id);
        let right = binder.bind_uuid(issue_id);
        let slug_holder = binder.bind_string(slug.to_owned());
        let side = binder.bind_uuid(issue_id);
        let relation_type = binder.bind_string(ids.relation_type.to_owned());
        branches.push(format!(
            "issue.id IN (SELECT DISTINCT r.{column} FROM issue_relations r \
            INNER JOIN workspaces rw ON rw.id = r.workspace_id \
            WHERE r.deleted_at IS NULL AND (r.issue_id = {left} OR r.related_issue_id = {right}) \
            AND rw.slug = {slug_holder} AND r.{side_col} = {side} \
            AND r.relation_type = {relation_type})",
            column = ids.column,
            side_col = ids.side,
        ));
    }
    let membership = branches.join(" OR ");
    format!("SELECT {selects} {RELATION_BUCKET_FROM} WHERE {preamble} AND ({membership}) GROUP BY issue.id")
}

// ---------------------------------------------------------------------------
// Links (link.py:32-46)
// ---------------------------------------------------------------------------

/// Full link-list statement: all link columns, `DISTINCT` (the member join
/// can fan out), newest first. The member join is unguarded (bug 7). Binds,
/// in order: workspace slug, project id, issue id, user id.
pub fn link_list_sql(
    binder: &mut Binder,
    slug: &str,
    project_id: uuid::Uuid,
    issue_id: uuid::Uuid,
    user_id: uuid::Uuid,
) -> String {
    let slug_holder = binder.bind_string(slug.to_owned());
    let project = binder.bind_uuid(project_id);
    let issue = binder.bind_uuid(issue_id);
    let user = binder.bind_uuid(user_id);
    format!(
        "SELECT DISTINCT {ISSUE_LINK_COLUMNS} \
        FROM issue_links \
        INNER JOIN workspaces ON issue_links.workspace_id = workspaces.id \
        INNER JOIN projects ON issue_links.project_id = projects.id \
        INNER JOIN project_members ON projects.id = project_members.project_id \
        WHERE issue_links.deleted_at IS NULL AND workspaces.slug = {slug_holder} \
        AND issue_links.project_id = {project} AND issue_links.issue_id = {issue} \
        AND projects.archived_at IS NULL AND project_members.is_active \
        AND project_members.member_id = {user} \
        ORDER BY issue_links.created_at DESC"
    )
}

// ---------------------------------------------------------------------------
// Archive (archive.py:54-255)
// ---------------------------------------------------------------------------

/// Archive scope joins: the nullable `type` FK left-joins `issue_types`; the
/// slug filter inner-joins `workspaces`.
pub const ARCHIVE_JOINS: &str = "FROM issues AS issue \
    LEFT OUTER JOIN issue_types ON issue.type_id = issue_types.id \
    JOIN workspaces ON workspaces.id = issue.workspace_id";

/// Archive scope (`get_queryset`, `archive.py:98-104`): non-epic types
/// (`type_id IS NULL OR NOT is_epic`), archived rows, this project, this
/// workspace slug. `Issue.objects`, so triage states and drafts stay visible.
/// Binds: project id, workspace slug.
pub fn archive_scope_where(binder: &mut Binder, slug: &str, project_id: uuid::Uuid) -> String {
    let project = binder.bind_uuid(project_id);
    let slug_holder = binder.bind_string(slug.to_owned());
    format!(
        "issue.deleted_at IS NULL AND (issue.type_id IS NULL OR NOT issue_types.is_epic) \
        AND issue.archived_at IS NOT NULL AND issue.project_id = {project} \
        AND workspaces.slug = {slug_holder}"
    )
}

/// Archive-list select list: all issue columns plus the four `NULL`-count
/// annotations (`apply_annotations`, `archive.py:61-96`). The assignee/label/
/// module prefetches are separate queries (handler-owned, pilot-2 prefetch
/// pattern); the grouped path adds grouper arrays via pilot-2's grouper.
pub fn archive_list_selects() -> String {
    let link = link_count_select(false);
    let attachment = attachment_count_select(false);
    let sub = sub_issues_count_select(false);
    format!("{ISSUE_COLUMNS}, {CYCLE_ID_SELECT}, {link}, {attachment}, {sub}")
}

/// Archive-list default order (`order_issue_queryset(..., "-created_at")`
/// passthrough, `archive.py:110-114`).
pub const ARCHIVE_DEFAULT_ORDER: &str = "ORDER BY issue.created_at DESC";

/// Full default archive-list statement before filter stacks: scope, optional
/// `show_sub_issues=false` top-level filter (`archive.py:116`), default
/// order. Rich + legacy filters splice into the `WHERE` via pilot-2's
/// `complex_filter` / `legacy_sql`; grouping and pagination are handler-owned.
pub fn archive_list_sql(
    binder: &mut Binder,
    slug: &str,
    project_id: uuid::Uuid,
    show_sub_issues: bool,
) -> String {
    let selects = archive_list_selects();
    let scope = archive_scope_where(binder, slug, project_id);
    let top = if show_sub_issues {
        String::new()
    } else {
        " AND issue.parent_id IS NULL".to_owned()
    };
    format!("SELECT {selects} {ARCHIVE_JOINS} WHERE {scope}{top} {ARCHIVE_DEFAULT_ORDER}")
}

/// Archive grouped-count predicate (`count_filter`, `archive.py:174-184`):
/// intake rows in (1, -1, 2) or no intake row. Handlers join
/// `LEFT JOIN intake_issues AS issue_intake ON issue_intake.issue_id =
/// issue.id` (pilot-2's `filtered_set` intake join) and count with
/// `Count("id", filter=...)`.
pub const ARCHIVE_COUNT_FILTER: &str = "(issue_intake.status = 1 OR issue_intake.status = -1 \
    OR issue_intake.status = 2 OR issue_intake.id IS NULL) \
    AND issue.archived_at IS NULL AND issue.is_draft = FALSE";

/// Full archive-retrieve statement (`archive.py:222-255`): the archive scope
/// plus the pk, `is_subscribed`, and NO count/array annotations — unlike the
/// main retrieve. Same reaction/link prefetches ([`reaction_prefetch_sql`],
/// [`link_prefetch_sql`]). Binds, in order: project id, slug (scope), issue
/// id, then the `is_subscribed` binds (project id, subscriber id, slug).
pub fn archive_retrieve_sql(
    binder: &mut Binder,
    slug: &str,
    project_id: uuid::Uuid,
    issue_id: uuid::Uuid,
    user_id: uuid::Uuid,
) -> String {
    let scope = archive_scope_where(binder, slug, project_id);
    let issue = binder.bind_uuid(issue_id);
    let subscribed = is_subscribed_select(binder, slug, project_id, user_id);
    format!(
        "SELECT {ISSUE_COLUMNS}, {subscribed} {ARCHIVE_JOINS} \
        WHERE {scope} AND issue.id = {issue} \
        ORDER BY issue.created_at DESC LIMIT 1"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const SLUG: &str = "ws-slug";
    const PROJECT: uuid::Uuid = uuid::Uuid::from_u128(0x11111111_1111_1111_1111_111111111111);
    const ISSUE: uuid::Uuid = uuid::Uuid::from_u128(0x33333333_3333_3333_3333_333333333333);
    const USER: uuid::Uuid = uuid::Uuid::from_u128(0x22222222_2222_2222_2222_222222222222);

    /// Collapse runs of whitespace so multi-line builders compare cleanly.
    fn flat(sql: &str) -> String {
        sql.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    // -- pilot-2 reuse pins --------------------------------------------------

    #[test]
    fn shared_fragments_match_pilot2_helper() {
        // The reused texts stay byte-identical to pilot-2's shared helper;
        // any drift in either direction fails here, not in a gate run.
        let helper = super::super::annotation_selects(true, None, true, true);
        for fragment in [
            CYCLE_ID_SELECT,
            &link_count_select(false),
            &attachment_count_select(false),
            LABEL_IDS_SELECT,
        ] {
            let fragment = flat(fragment);
            assert!(
                flat(&helper).contains(&fragment),
                "pilot-2 helper drifted from shared fragment: {fragment}"
            );
        }
    }

    #[test]
    fn faithful_fragments_drop_pilot2_added_guards() {
        // Live Django SQL carries no soft-delete guard on the joined member,
        // module, or state rows; these builders match Django, not the helper.
        assert!(!ASSIGNEE_IDS_ACTIVE_SELECT.contains("pm.deleted_at"));
        assert!(!MODULE_IDS_SELECT.contains("m.deleted_at"));
        assert!(!sub_issues_count_select(false).contains("cs.deleted_at"));
        assert!(ASSIGNEE_IDS_ACTIVE_SELECT.contains("pm.is_active"));
        assert!(MODULE_IDS_SELECT.contains("m.archived_at IS NULL"));
    }

    // -- retrieve ------------------------------------------------------------

    #[test]
    fn retrieve_selects_full_row_plus_annotations_plus_state() {
        let mut binder = Binder::new();
        let sql = retrieve_sql(&mut binder, SLUG, PROJECT, ISSUE, USER);
        // Select order: issue columns, annotations in annotate() order,
        // is_subscribed, then the select_related state columns.
        let issue_pos = sql.find("issue.created_at").expect("issue cols");
        let cycle_pos = sql.find("AS cycle_id").expect("cycle");
        let link_pos = sql.find("AS link_count").expect("link");
        let attach_pos = sql.find("AS attachment_count").expect("attach");
        let sub_pos = sql.find("AS sub_issues_count").expect("sub");
        let label_pos = sql.find("AS label_ids").expect("labels");
        let assignee_pos = sql.find("AS assignee_ids").expect("assignees");
        let module_pos = sql.find("AS module_ids").expect("modules");
        let subscribed_pos = sql.find("AS is_subscribed").expect("subscribed");
        let state_pos = sql.find("state.created_at").expect("state cols");
        assert!(
            issue_pos < cycle_pos
                && cycle_pos < link_pos
                && link_pos < attach_pos
                && attach_pos < sub_pos
                && sub_pos < label_pos
                && label_pos < assignee_pos
                && assignee_pos < module_pos
                && module_pos < subscribed_pos
                && subscribed_pos < state_pos,
            "retrieve select order wrong: {sql}"
        );
        // Retrieve counts are NULL when zero (no Coalesce).
        assert!(!sql.contains("COALESCE((SELECT NULLIF"));
        // Issue.objects scope: soft-delete only, no triage/archived/draft guards
        // on the OUTER query (the sub_issues_count subquery legitimately
        // carries the child-scope triage guard).
        let outer = sql
            .split("FROM issues AS issue")
            .nth(1)
            .expect("outer query");
        assert!(outer.contains("issue.deleted_at IS NULL"));
        assert!(!outer.contains("triage"));
        assert!(!outer.contains("issue.archived_at IS NULL"));
        assert!(!outer.contains("issue.is_draft"));
        assert!(sql.contains("LEFT JOIN states AS state ON state.id = issue.state_id"));
        assert!(sql.ends_with("ORDER BY issue.created_at DESC LIMIT 1"));
        assert_eq!(binder.values().len(), 6);
    }

    #[test]
    fn retrieve_prefetches_match_django_shapes() {
        let mut binder = Binder::new();
        let reactions = reaction_prefetch_sql(&mut binder, &[ISSUE]);
        // Model field order (actor, then issue), INNER joins, IN-list, ordered.
        let users_pos = reactions.find("users.password").expect("actor cols");
        let issues_pos = reactions.find("issues.created_at").expect("issue cols");
        assert!(
            users_pos < issues_pos,
            "actor columns precede issue columns"
        );
        assert!(reactions.contains("INNER JOIN issues ON issue_reactions.issue_id = issues.id"));
        assert!(reactions.contains("INNER JOIN users ON issue_reactions.actor_id = users.id"));
        assert!(reactions.contains("issue_reactions.issue_id IN ($1)"));
        assert!(reactions.ends_with("ORDER BY issue_reactions.created_at DESC"));

        let mut binder = Binder::new();
        let links = link_prefetch_sql(&mut binder, &[ISSUE]);
        // Nullable created_by: LEFT join.
        assert!(links.contains("LEFT OUTER JOIN users ON issue_links.created_by_id = users.id"));
        assert!(links.contains("issue_links.issue_id IN ($1)"));
        assert!(links.ends_with("ORDER BY issue_links.created_at DESC"));
        assert_eq!(binder.values().len(), 1);
    }

    #[test]
    fn retrieve_gate_queries_are_scoped() {
        let mut binder = Binder::new();
        let project = project_get_sql(&mut binder, SLUG, PROJECT);
        assert!(project.contains("projects.deleted_at IS NULL"));
        assert!(project.contains("workspaces.slug = $2"));
        assert_eq!(binder.values().len(), 2);

        let mut binder = Binder::new();
        let guest = guest_view_exists_sql(&mut binder, SLUG, PROJECT, USER);
        assert!(guest.starts_with("SELECT 1 AS a"));
        assert!(guest.contains("project_members.role = 5"));
        assert!(guest.contains("project_members.is_active"));
        assert!(!guest.contains("ORDER BY"));
        assert!(guest.ends_with("LIMIT 1"));
        assert_eq!(binder.values().len(), 3);
    }

    // -- sub-issues ----------------------------------------------------------

    #[test]
    fn sub_issues_keys_cover_all_26() {
        assert_eq!(SUB_ISSUES_KEYS.len(), 26);
        for key in ["estimate_point", "created_by", "updated_by", "state_group"] {
            assert!(SUB_ISSUES_KEYS.contains(&key), "missing {key}");
        }
    }

    #[test]
    fn sub_issues_render_concrete_first_then_annotations() {
        let selects = flat(&sub_issues_selects());
        // Aliased keys.
        assert!(selects.contains("issue.estimate_point_id AS estimate_point"));
        assert!(selects.contains("issue.created_by_id AS created_by"));
        assert!(selects.contains("issue.updated_by_id AS updated_by"));
        // Render order: ... parent_id, created_at ... (concrete), then
        // annotations in annotate() order — module_ids AFTER label/assignee.
        let parent = selects.find("issue.parent_id,").expect("parent");
        let created = selects.find("issue.created_at,").expect("created");
        let cycle = selects.find("AS cycle_id").expect("cycle");
        let link = selects.find("AS link_count").expect("link");
        let label = selects.find("AS label_ids").expect("labels");
        let assignee = selects.find("AS assignee_ids").expect("assignees");
        let module = selects.find("AS module_ids").expect("modules");
        let group = selects.find("AS state_group").expect("group");
        assert!(parent < created && created < cycle);
        assert!(cycle < link && link < label && label < assignee && assignee < module);
        assert!(module < group);
        // Sub-issue counts coalesce to 0.
        assert!(selects.contains("COALESCE((SELECT NULLIF(COUNT(*), 0)"));
        assert_eq!(
            selects
                .matches("COALESCE((SELECT NULLIF(COUNT(*), 0)")
                .count(),
            3
        );
    }

    #[test]
    fn sub_issues_scope_is_manager_plus_parent_plus_slug() {
        let mut binder = Binder::new();
        let sql = sub_issues_sql(&mut binder, SLUG, ISSUE);
        assert!(sql.contains("NOT (state.\"group\" = 'triage' AND state.\"group\" IS NOT NULL)"));
        assert!(sql.contains("issue.archived_at IS NULL"));
        assert!(sql.contains("project.archived_at IS NULL"));
        assert!(sql.contains("issue.is_draft = FALSE"));
        assert!(sql.contains("issue.parent_id = $1"));
        assert!(sql.contains("workspaces.slug = $2"));
        assert!(sql.ends_with("ORDER BY issue.created_at DESC"));
        assert_eq!(binder.values().len(), 2);
    }

    // -- relations -----------------------------------------------------------

    #[test]
    fn relation_buckets_cover_all_eight_labels() {
        let labels: Vec<_> = RELATION_BUCKETS.iter().map(|bucket| bucket.label).collect();
        assert_eq!(
            labels,
            [
                "blocking",
                "blocked_by",
                "duplicate",
                "relates_to",
                "start_after",
                "start_before",
                "finish_after",
                "finish_before"
            ]
        );
        // Ten id-lists in total: six singles plus two OR-ed pairs.
        let total: usize = RELATION_BUCKETS.iter().map(|bucket| bucket.ids.len()).sum();
        assert_eq!(total, 10);
        assert_eq!(RELATION_ROW_FIELDS.len(), 14);
    }

    #[test]
    fn relation_ids_carry_ordering_column_and_scope() {
        let ids = RelationIds {
            column: "issue_id",
            relation_type: "blocked_by",
            side: "related_issue_id",
        };
        let mut binder = Binder::new();
        let sql = relation_ids_sql(&mut binder, SLUG, ISSUE, ids);
        assert!(
            sql.contains("SELECT DISTINCT issue_relations.issue_id, issue_relations.created_at")
        );
        assert!(sql
            .contains("(issue_relations.issue_id = $1 OR issue_relations.related_issue_id = $2)"));
        assert!(sql.contains("workspaces.slug = $3"));
        assert!(sql.contains("issue_relations.related_issue_id = $4"));
        assert!(sql.contains("issue_relations.relation_type = $5"));
        assert!(sql.ends_with("ORDER BY issue_relations.created_at DESC"));
        // Only the slug join survives a values_list; the base select_related
        // joins (project, issue) never render.
        assert!(!sql.contains("JOIN projects"));
        assert!(!sql.contains("JOIN issues"));
        assert_eq!(binder.values().len(), 5);
    }

    #[test]
    fn relation_bucket_aggregates_over_joins_without_order() {
        let bucket = RELATION_BUCKETS[0]; // blocking
        let mut binder = Binder::new();
        let sql = relation_bucket_sql(&mut binder, SLUG, ISSUE, bucket);
        // Direct FILTER aggregates over LEFT joins + GROUP BY, no ORDER BY.
        assert!(sql.contains("FILTER (WHERE (NOT (issue_labels.label_id IS NULL)"));
        assert!(sql.contains("LEFT OUTER JOIN issue_assignees"));
        assert!(sql.contains("LEFT OUTER JOIN project_members"));
        assert!(sql.ends_with("GROUP BY issue.id"));
        assert!(!sql.contains("ORDER BY"));
        assert!(sql.contains("issue.id IN (SELECT DISTINCT r.issue_id"));
        // Dead target annotations are not emitted.
        assert!(!sql.contains("AS cycle_id"));
        assert!(!sql.contains("AS link_count"));
        // created_by/updated_by select the raw id columns (dict keys renamed
        // in Python, aliased by handlers from RELATION_ROW_FIELDS).
        assert!(sql.contains("issue.created_by_id, issue.updated_by_id"));
    }

    #[test]
    fn relation_or_buckets_factor_the_preamble() {
        let bucket = RELATION_BUCKETS[2]; // duplicate
        let mut binder = Binder::new();
        let sql = relation_bucket_union_sql(&mut binder, SLUG, ISSUE, bucket);
        assert!(sql.contains("r.related_issue_id"));
        assert!(sql.contains(" OR issue.id IN (SELECT DISTINCT r.issue_id"));
        assert!(sql.contains("$1 AS relation_type"));
        // Single preamble, two membership branches (preamble binds
        // workspaces.slug; branches bind rw.slug).
        assert_eq!(sql.matches(".slug").count(), 3); // preamble + 2 branches
        assert!(sql.ends_with("GROUP BY issue.id"));
    }

    // -- links ---------------------------------------------------------------

    #[test]
    fn link_list_joins_member_without_deleted_guard() {
        let mut binder = Binder::new();
        let sql = link_list_sql(&mut binder, SLUG, PROJECT, ISSUE, USER);
        assert!(sql.starts_with("SELECT DISTINCT"));
        assert!(sql.contains("INNER JOIN projects ON issue_links.project_id = projects.id"));
        assert!(
            sql.contains("INNER JOIN project_members ON projects.id = project_members.project_id")
        );
        assert!(sql.contains("projects.archived_at IS NULL"));
        assert!(sql.contains("project_members.is_active"));
        assert!(sql.contains("project_members.member_id = $4"));
        // Bug 7: no project_members.deleted_at guard.
        assert!(!sql.contains("project_members.deleted_at"));
        assert!(sql.ends_with("ORDER BY issue_links.created_at DESC"));
        assert_eq!(binder.values().len(), 4);
    }

    // -- archive -------------------------------------------------------------

    #[test]
    fn archive_scope_keeps_triage_and_drafts() {
        let mut binder = Binder::new();
        let sql = archive_list_sql(&mut binder, SLUG, PROJECT, true);
        assert!(sql.contains("(issue.type_id IS NULL OR NOT issue_types.is_epic)"));
        assert!(sql.contains("LEFT OUTER JOIN issue_types ON issue.type_id = issue_types.id"));
        assert!(sql.contains("issue.archived_at IS NOT NULL"));
        let outer = sql
            .split("FROM issues AS issue")
            .nth(1)
            .expect("outer query");
        assert!(!outer.contains("triage"));
        assert!(!outer.contains("issue.is_draft"));
        assert!(!outer.contains("parent_id IS NULL"));
        assert!(sql.ends_with("ORDER BY issue.created_at DESC"));

        let mut binder = Binder::new();
        let top = archive_list_sql(&mut binder, SLUG, PROJECT, false);
        assert!(top.contains("AND issue.parent_id IS NULL"));

        assert!(ARCHIVE_COUNT_FILTER.contains("issue_intake.status = 1"));
        assert!(ARCHIVE_COUNT_FILTER.contains("issue_intake.id IS NULL"));
    }

    #[test]
    fn archive_retrieve_has_subscribed_but_no_counts() {
        let mut binder = Binder::new();
        let sql = archive_retrieve_sql(&mut binder, SLUG, PROJECT, ISSUE, USER);
        assert!(sql.contains("AS is_subscribed"));
        assert!(!sql.contains("AS cycle_id"));
        assert!(!sql.contains("AS link_count"));
        assert!(!sql.contains("AS label_ids"));
        assert!(sql.contains("issue.archived_at IS NOT NULL"));
        assert!(sql.ends_with("ORDER BY issue.created_at DESC LIMIT 1"));
        assert_eq!(binder.values().len(), 6);
    }

    // -- replay dump ---------------------------------------------------------

    /// Print every statement for the FX-ISS-11 replay script
    /// (`/tmp/replay-648/replay.py`): `@@ <name> @@`, the SQL, then `@@ binds:
    /// <n> @@`. Not an assertion — the replay diffs this against live Django.
    #[test]
    fn dump_statements_for_fixture_replay() {
        let mut statements: Vec<(&str, String, usize)> = Vec::new();
        macro_rules! dump {
            ($name:expr, $call:expr) => {{
                let mut binder = Binder::new();
                let sql = $call(&mut binder);
                statements.push(($name, sql, binder.values().len()));
            }};
        }
        dump!("retrieve_main", |binder: &mut Binder| retrieve_sql(
            binder, SLUG, PROJECT, ISSUE, USER
        ));
        dump!("retrieve_prefetch_reactions", |binder: &mut Binder| {
            reaction_prefetch_sql(binder, &[ISSUE])
        });
        dump!("retrieve_prefetch_links", |binder: &mut Binder| {
            link_prefetch_sql(binder, &[ISSUE])
        });
        dump!("retrieve_project_get", |binder: &mut Binder| {
            project_get_sql(binder, SLUG, PROJECT)
        });
        dump!("retrieve_guest_exists", |binder: &mut Binder| {
            guest_view_exists_sql(binder, SLUG, PROJECT, USER)
        });
        dump!("subissues_values", |binder: &mut Binder| {
            sub_issues_sql(binder, SLUG, ISSUE)
        });
        dump!("relation_ids_blocking", |binder: &mut Binder| {
            relation_ids_sql(binder, SLUG, ISSUE, RELATION_BUCKETS[0].ids[0])
        });
        dump!("relation_ids_blocked_by", |binder: &mut Binder| {
            relation_ids_sql(binder, SLUG, ISSUE, RELATION_BUCKETS[1].ids[0])
        });
        dump!("relation_ids_duplicate", |binder: &mut Binder| {
            relation_ids_sql(binder, SLUG, ISSUE, RELATION_BUCKETS[2].ids[0])
        });
        dump!("relation_bucket_blocking", |binder: &mut Binder| {
            relation_bucket_sql(binder, SLUG, ISSUE, RELATION_BUCKETS[0])
        });
        dump!("relation_bucket_duplicate_union", |binder: &mut Binder| {
            relation_bucket_union_sql(binder, SLUG, ISSUE, RELATION_BUCKETS[2])
        });
        dump!("link_list", |binder: &mut Binder| {
            link_list_sql(binder, SLUG, PROJECT, ISSUE, USER)
        });
        dump!("archive_base", |binder: &mut Binder| {
            archive_list_sql(binder, SLUG, PROJECT, true)
        });
        dump!("archive_nosub", |binder: &mut Binder| {
            archive_list_sql(binder, SLUG, PROJECT, false)
        });
        dump!("archive_retrieve", |binder: &mut Binder| {
            archive_retrieve_sql(binder, SLUG, PROJECT, ISSUE, USER)
        });
        for (name, sql, binds) in &statements {
            println!("@@ {name} @@\n{sql}\n@@ binds: {binds} @@");
        }
        println!("@@ count_filter @@\n{ARCHIVE_COUNT_FILTER}");
    }
}
