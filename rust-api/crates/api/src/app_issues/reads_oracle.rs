#![forbid(unsafe_code)]

//! The pre-Batch-L2 text statements of the issue reads (sub-issues,
//! identifier, meta, archive retrieve, user properties, bulk dates, the
//! `?expand=` nests), kept only as the differential oracle for the
//! sea-query builders that replaced them (`queries_core`,
//! `queries_engage`, `handlers_reads`, `expand`). Every builder below is
//! the old production text, moved verbatim; the live test at the bottom
//! seeds one dataset and asserts the built statements return
//! byte-identical `row_to_json` rows. The archive scope also backs the
//! list oracle (`list_oracle.rs`). Delete this module once nothing needs
//! the comparison.

use pidash_services::app_issues::order_sql;

use super::{order_key, with_order_joins, Binder, Denial};

// ---------------------------------------------------------------------------
// Shared column lists (Django concrete-field order, live-compiled)
// ---------------------------------------------------------------------------

/// All 35 concrete `issues` columns in Django's select order, qualified with
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
    issue.assigned_pod_id, issue.assigned_runner_id, issue.agent_executor";

/// All 18 concrete `states` columns (`select_related("state")` order).
/// `group`/`default` stay quoted: reserved words.
pub const STATE_COLUMNS: &str = "state.created_at, state.updated_at, state.created_by_id, \
    state.updated_by_id, state.deleted_at, state.id, state.project_id, state.workspace_id, \
    state.name, state.description, state.color, state.slug, state.sequence, state.\"group\", \
    state.is_triage, state.\"default\", state.external_source, state.external_id";

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

// ---------------------------------------------------------------------------
// Scalar annotations shared by retrieve / sub-issues / archive-list
// ---------------------------------------------------------------------------

/// `cycle_id` (`base.py:496`, `sub_issue.py:40-44`, `archive.py:63-67`).
/// Same text as pilot-2's: Django's `ORDER BY created_at DESC` (model
/// ordering) is dropped. The cycle write paths move links instead of stacking
/// them (`app/views/cycle/issue.py` and `api/views/cycle.py` re-point existing
/// links; drafts link brand-new issues), so at most one live row per issue
/// exists and the order cannot change the result. (The partial unique covers
/// `(cycle, issue)`, not `(issue)` alone — the single-row property comes from
/// the move semantics, shared with pilot-2's merged list paths.)
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

/// Sub-issue `.values(27)` select list in Django's RENDER order: concrete
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
        issue.target_date, issue.sequence_id, issue.project_id, issue.parent_id, issue.type_id, \
        issue.created_at, issue.updated_at, issue.created_by_id AS created_by, \
        issue.updated_by_id AS updated_by, issue.is_draft, issue.archived_at, {CYCLE_ID_SELECT}, \
        {link}, {attachment}, {sub}, {LABEL_IDS_SELECT}, {ASSIGNEE_IDS_ACTIVE_SELECT}, \
        {MODULE_IDS_SELECT}, state.\"group\" AS state_group"
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

/// All 14 concrete `workspaces` columns (`select_related("workspace")` order).
pub const WORKSPACE_COLUMNS: &str = "workspaces.created_at, workspaces.updated_at, \
    workspaces.created_by_id, workspaces.updated_by_id, workspaces.deleted_at, workspaces.id, \
    workspaces.name, workspaces.logo, workspaces.logo_asset_id, workspaces.owner_id, \
    workspaces.slug, workspaces.organization_size, workspaces.timezone, \
    workspaces.background_color";

/// All 20 concrete `issue_agent_ticker` columns (`select_related("agent_ticker")`
/// order; matches D-10 `tasks_ticker::models::issue_agent_ticker::COLUMNS`).
pub const TICKER_COLUMNS: &str = "issue_agent_ticker.created_at, issue_agent_ticker.updated_at, \
    issue_agent_ticker.created_by_id, issue_agent_ticker.updated_by_id, \
    issue_agent_ticker.deleted_at, issue_agent_ticker.id, issue_agent_ticker.issue_id, \
    issue_agent_ticker.used, issue_agent_ticker.granted, issue_agent_ticker.waited, \
    issue_agent_ticker.user_disabled, issue_agent_ticker.next_run_at, \
    issue_agent_ticker.last_tick_at, issue_agent_ticker.enabled, \
    issue_agent_ticker.disarm_reason, issue_agent_ticker.pending_entry, \
    issue_agent_ticker.pending_entry_free, issue_agent_ticker.pending_entry_actor_id, \
    issue_agent_ticker.pending_entry_trigger, issue_agent_ticker.resume_parent_run_id";

/// All 35 concrete `issues` columns under the `parent` self-join alias
/// (identifier-full `select_related("parent")`; Django renders `T4`).
pub const PARENT_ISSUE_COLUMNS: &str = "parent.created_at, parent.updated_at, \
    parent.created_by_id, parent.updated_by_id, parent.deleted_at, parent.id, \
    parent.project_id, parent.workspace_id, parent.parent_id, parent.state_id, parent.point, \
    parent.estimate_point_id, parent.name, parent.description_json, parent.description_html, \
    parent.description_stripped, parent.description_binary, parent.priority, \
    parent.complexity_score, parent.start_date, parent.target_date, parent.sequence_id, \
    parent.sort_order, parent.completed_at, parent.archived_at, parent.is_draft, \
    parent.external_source, parent.external_id, parent.type_id, parent.git_work_branch, \
    parent.workpad, parent.created_via, parent.assigned_pod_id, parent.assigned_runner_id, parent.agent_executor";

/// Identifier-lite `.only()` select list in Django's RENDER order: `.only()`
/// honors model field order, not the call order. `external_source` rides
/// along for the `_issue_is_actively_synced` short-circuit (serializers
/// own the predicate; the queries layer keeps its inputs loaded).
pub const LITE_ISSUE_COLUMNS: &str = "issue.created_at, issue.updated_at, issue.created_by_id, \
    issue.updated_by_id, issue.id, issue.project_id, issue.workspace_id, issue.name, \
    issue.description_html, issue.sequence_id, issue.sort_order, issue.archived_at, \
    issue.is_draft, issue.external_source";

// ---------------------------------------------------------------------------
// Meta (base.py:1199-1211)
// ---------------------------------------------------------------------------

/// Full meta statement (`base.py:1201-1211`): the issue's `sequence_id`
/// plus its project's `identifier`, over the `issue_objects` manager
/// scope (triage/archived/draft rows are NOT visible here — the only
/// lookup in this module on the scoped manager).
///
/// Django fires two queries — the `.only("sequence_id",
/// "project__identifier")` main select plus a lazy `project.identifier`
/// load, because a spanning `.only()` without `select_related` never
/// joins — and this builder is their single-statement consolidation over
/// the join the scope already requires. Same two values, one round trip.
/// Binds, in order: issue id, project id, workspace slug.
pub fn meta_sql(
    binder: &mut Binder,
    slug: &str,
    project_id: uuid::Uuid,
    issue_id: uuid::Uuid,
) -> String {
    let issue = binder.bind_uuid(issue_id);
    let project = binder.bind_uuid(project_id);
    let slug_holder = binder.bind_string(slug.to_owned());
    format!(
        "SELECT issues.sequence_id, projects.identifier FROM issues \
        LEFT OUTER JOIN states ON issues.state_id = states.id \
        INNER JOIN projects ON issues.project_id = projects.id \
        INNER JOIN workspaces ON issues.workspace_id = workspaces.id \
        WHERE issues.deleted_at IS NULL \
        AND NOT (states.\"group\" = 'triage' AND states.\"group\" IS NOT NULL) \
        AND issues.archived_at IS NULL AND projects.archived_at IS NULL \
        AND issues.is_draft = FALSE \
        AND issues.id = {issue} AND issues.project_id = {project} \
        AND workspaces.slug = {slug_holder} \
        ORDER BY issues.created_at DESC LIMIT 1"
    )
}

// ---------------------------------------------------------------------------
// Identifier (base.py:1214-1489)
// ---------------------------------------------------------------------------

/// Project lookup by workspace-scoped identifier (`base.py:1253`):
/// `identifier__iexact` renders `UPPER(identifier::text) = UPPER($n)`.
/// Unique per `(identifier, workspace)` among live rows (partial unique
/// plus `save()` upper-normalization), so `LIMIT 1` matches `.get()`.
/// Binds, in order: project identifier, workspace slug.
pub fn identifier_project_sql(binder: &mut Binder, slug: &str, project_identifier: &str) -> String {
    let identifier = binder.bind_string(project_identifier.to_owned());
    let slug_holder = binder.bind_string(slug.to_owned());
    format!(
        "SELECT {PROJECT_COLUMNS} FROM projects \
        INNER JOIN workspaces ON projects.workspace_id = workspaces.id \
        WHERE projects.deleted_at IS NULL AND UPPER(projects.identifier::text) = UPPER({identifier}) \
        AND workspaces.slug = {slug_holder} \
        ORDER BY projects.created_at DESC LIMIT 1"
    )
}

/// Identifier member gate `Exists` (`base.py:1256-1265`): ANY active
/// membership (no role filter — unlike the guest-view gate, which is
/// `queries_core::guest_view_exists_sql`). Django's `.exists()` renders
/// `SELECT 1 ... LIMIT 1` with no `ORDER BY`. Binds, in order: user id,
/// project id, workspace slug.
pub fn identifier_member_exists_sql(
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
        AND workspaces.slug = {slug_holder} LIMIT 1"
    )
}

/// `is_intake` `Exists`, shared by the lite (`base.py:1294-1301`) and
/// full (`base.py:1436-1445`) lookups: a pending (`-2`) or snoozed (`0`)
/// intake row for this issue. Correlated to the `issue` alias both
/// statements use. Binds, in order: project id, workspace slug.
pub fn is_intake_exists_select(binder: &mut Binder, slug: &str, project_id: uuid::Uuid) -> String {
    let project = binder.bind_uuid(project_id);
    let slug_holder = binder.bind_string(slug.to_owned());
    format!(
        "EXISTS(SELECT 1 AS a FROM intake_issues ii \
        INNER JOIN workspaces iiw ON ii.workspace_id = iiw.id \
        WHERE ii.deleted_at IS NULL AND ii.issue_id = issue.id AND ii.project_id = {project} \
        AND ii.status IN (-2, 0) AND iiw.slug = {slug_holder} LIMIT 1) AS is_intake"
    )
}

/// Lite `is_synced` `Exists` (`base.py:1303-1309`): bug 2 — git syncs
/// only, unscoped. No binds, so a plain const.
pub const LITE_IS_SYNCED_SELECT: &str = "EXISTS(SELECT 1 AS a FROM git_issue_syncs gs \
    WHERE gs.deleted_at IS NULL AND gs.issue_id = issue.id LIMIT 1) AS is_synced";

/// Lite `is_epic` (`base.py:1288-1293`): the nullable type flag coalesced
/// to `FALSE`, over the `issue_types` left join. No binds.
pub const LITE_IS_EPIC_SELECT: &str = "COALESCE(issue_types.is_epic, FALSE) AS is_epic";

/// Bind a 32-bit integer (issue `sequence_id`) for a `$n` placeholder.
fn bind_sequence_id(binder: &mut Binder, sequence_id: i32) -> String {
    binder.bind(sea_query::Value::Int(Some(sequence_id)))
}

/// Full lite statement (`base.py:1267-1310`): the deferred 14-column row
/// plus `is_epic` / `is_intake` / `is_synced`, over plain
/// `Issue.objects` (triage/archived/draft rows visible). Binds, in order:
/// project id, workspace slug, sequence id (outer `WHERE`), then the
/// `is_intake` binds (project id, slug).
pub fn identifier_lite_sql(
    binder: &mut Binder,
    slug: &str,
    project_id: uuid::Uuid,
    sequence_id: i32,
) -> String {
    let project = binder.bind_uuid(project_id);
    let slug_holder = binder.bind_string(slug.to_owned());
    let sequence = bind_sequence_id(binder, sequence_id);
    let is_intake = is_intake_exists_select(binder, slug, project_id);
    format!(
        "SELECT {LITE_ISSUE_COLUMNS}, {LITE_IS_EPIC_SELECT}, {is_intake}, {LITE_IS_SYNCED_SELECT} \
        FROM issues AS issue \
        INNER JOIN workspaces ON issue.workspace_id = workspaces.id \
        LEFT OUTER JOIN issue_types ON issue.type_id = issue_types.id \
        WHERE issue.deleted_at IS NULL AND issue.project_id = {project} \
        AND workspaces.slug = {slug_holder} AND issue.sequence_id = {sequence} \
        ORDER BY issue.created_at DESC LIMIT 1"
    )
}

/// Full identifier statement (`base.py:1312-1446`): all issue columns, the
/// seven annotations in `annotate()` order (counts coalesced to `0`,
/// arrays over the doubled-guard single-port form), `is_subscribed` and
/// `is_intake`, then the `select_related("workspace", "project",
/// "state", "parent", "agent_ticker")` columns in Django's field-order
/// rendering (projects, workspaces, parent, states, ticker) — Django's
/// exact select order. The `assignees` / `labels` / `issue_module__module`
/// M2M prefetches are separate queries, handler-owned; the
/// reaction/link prefetches reuse
/// `queries_core::{reaction_prefetch_sql, link_prefetch_sql}`. Binds, in
/// order: project id, workspace slug, sequence id (outer `WHERE`), then
/// the `is_subscribed` binds (project id, subscriber id, slug), then the
/// `is_intake` binds (project id, slug).
pub fn identifier_full_sql(
    binder: &mut Binder,
    slug: &str,
    project_id: uuid::Uuid,
    sequence_id: i32,
    user_id: uuid::Uuid,
) -> String {
    let project = binder.bind_uuid(project_id);
    let slug_holder = binder.bind_string(slug.to_owned());
    let sequence = bind_sequence_id(binder, sequence_id);
    let subscribed = is_subscribed_select(binder, slug, project_id, user_id);
    let is_intake = is_intake_exists_select(binder, slug, project_id);
    let link = link_count_select(true);
    let attachment = attachment_count_select(true);
    let sub = sub_issues_count_select(true);
    format!(
        "SELECT {ISSUE_COLUMNS}, {CYCLE_ID_SELECT}, {link}, {attachment}, {sub}, \
        {LABEL_IDS_SELECT}, {ASSIGNEE_IDS_ACTIVE_SELECT}, {MODULE_IDS_SELECT}, \
        {subscribed}, {is_intake}, \
        {PROJECT_COLUMNS}, {WORKSPACE_COLUMNS}, {PARENT_ISSUE_COLUMNS}, {STATE_COLUMNS}, \
        {TICKER_COLUMNS} \
        FROM issues AS issue \
        INNER JOIN projects ON issue.project_id = projects.id \
        INNER JOIN workspaces ON issue.workspace_id = workspaces.id \
        LEFT OUTER JOIN issues AS parent ON issue.parent_id = parent.id \
        LEFT OUTER JOIN states AS state ON issue.state_id = state.id \
        LEFT OUTER JOIN issue_agent_ticker ON issue.id = issue_agent_ticker.issue_id \
        WHERE issue.deleted_at IS NULL AND issue.project_id = {project} \
        AND workspaces.slug = {slug_holder} AND issue.sequence_id = {sequence} \
        ORDER BY issue.created_at DESC LIMIT 1"
    )
}

// ---------------------------------------------------------------------------
// Handler-local statements (pre-L2 `handlers_reads` / `expand` text)
// ---------------------------------------------------------------------------

type Stmt = (String, Vec<sea_query::Value>);

const PROPERTY_COLUMNS: &str = "id, created_at, updated_at, deleted_at, filters, display_filters, \
    display_properties, rich_filters, preferences, sort_order, created_by_id, updated_by_id, \
    project_id, workspace_id, user_id";

pub fn property_sql(user_id: uuid::Uuid, project_id: uuid::Uuid) -> Stmt {
    let mut binder = Binder::new();
    let user = binder.bind_uuid(user_id);
    let project = binder.bind_uuid(project_id);
    let inner = format!(
        "SELECT {PROPERTY_COLUMNS} FROM project_user_properties \
        WHERE user_id = {user} AND project_id = {project} AND deleted_at IS NULL \
        ORDER BY created_at DESC LIMIT 1"
    );
    (inner, binder.values())
}

pub fn bulk_issues_sql(slug: &str, project_id: uuid::Uuid, ids: &[uuid::Uuid]) -> Stmt {
    let mut binder = Binder::new();
    let mut holders = Vec::with_capacity(ids.len());
    for id in ids {
        holders.push(binder.bind_uuid(*id));
    }
    let project = binder.bind_uuid(project_id);
    let slug_holder = binder.bind_string(slug.to_owned());
    let inner = format!(
        "SELECT issue.id, issue.start_date, issue.target_date FROM issues AS issue \
        JOIN workspaces ON workspaces.id = issue.workspace_id \
        WHERE issue.deleted_at IS NULL AND issue.id IN ({}) \
        AND issue.project_id = {project} AND workspaces.slug = {slug_holder}",
        holders.join(",")
    );
    (inner, binder.values())
}

const SCOPED_ISSUE_FROM: &str = "FROM issues AS issue \
    LEFT JOIN states AS state ON state.id = issue.state_id \
    JOIN projects AS project ON project.id = issue.project_id";
const SCOPED_ISSUE_SCOPE: &str = "issue.deleted_at IS NULL \
    AND NOT (state.\"group\" = 'triage' AND state.\"group\" IS NOT NULL) \
    AND issue.archived_at IS NULL AND project.archived_at IS NULL AND issue.is_draft = FALSE";

pub fn scoped_issue_sql(id: uuid::Uuid) -> Stmt {
    let mut binder = Binder::new();
    let holder = binder.bind_uuid(id);
    let inner = format!(
        "SELECT issue.id {SCOPED_ISSUE_FROM} WHERE {SCOPED_ISSUE_SCOPE} AND issue.id = {holder} LIMIT 1"
    );
    (inner, binder.values())
}

pub fn scoped_issue_ids_sql(ids: &[uuid::Uuid]) -> Stmt {
    let mut binder = Binder::new();
    let mut holders = Vec::with_capacity(ids.len());
    for id in ids {
        holders.push(binder.bind_uuid(*id));
    }
    let inner = format!(
        "SELECT issue.id {SCOPED_ISSUE_FROM} WHERE {SCOPED_ISSUE_SCOPE} AND issue.id IN ({})",
        holders.join(",")
    );
    (inner, binder.values())
}

const POST_ROW_COLUMNS: &str = "issue.id, issue.name, issue.state_id, issue.sort_order, \
    issue.completed_at, issue.estimate_point_id AS estimate_point, issue.priority, \
    issue.complexity_score, issue.start_date, issue.target_date, issue.sequence_id, \
    issue.project_id, issue.parent_id, issue.type_id, issue.assigned_pod_id, issue.assigned_runner_id, issue.agent_executor, \
    issue.created_at, issue.updated_at, issue.created_by_id AS created_by, \
    issue.updated_by_id AS updated_by, issue.is_draft, issue.archived_at, \
    issue.external_source, state.\"group\" AS state_group";

pub fn post_rows_sql(found: &[uuid::Uuid]) -> Stmt {
    let mut binder = Binder::new();
    let mut holders = Vec::with_capacity(found.len());
    for id in found {
        holders.push(binder.bind_uuid(*id));
    }
    let inner = format!(
        "SELECT {POST_ROW_COLUMNS} {SCOPED_ISSUE_FROM} WHERE {SCOPED_ISSUE_SCOPE} \
        AND issue.id IN ({}) ORDER BY issue.created_at DESC",
        holders.join(",")
    );
    (inner, binder.values())
}

/// The pre-L2 sub-issue `ORDER BY` and its extra joins.
pub fn sub_issues_order(order_by_param: &str) -> Result<(String, String), Denial> {
    if order_by_param.is_empty() {
        return Ok((SUB_ISSUES_DEFAULT_ORDER.to_owned(), String::new()));
    }
    let spec = order_sql(order_by_param, "state.\"group\"", |name| {
        format!("min_{}", name.replace("__", "_"))
    });
    let out = spec.out_param.as_str();
    if out == "priority_order"
        || out == "-priority_order"
        || out == "state_order"
        || out == "-state_order"
    {
        let sql = spec
            .order_by_sql
            .replace("created_at DESC", "issue.created_at DESC");
        return Ok((format!("ORDER BY {sql}"), String::new()));
    }
    if out == "min_values" || out == "-min_values" {
        let key = order_key(out, order_by_param, None, false)?;
        return Ok((
            format!(
                "ORDER BY {expr} {}, issue.created_at DESC",
                if key.descending { "DESC" } else { "ASC" },
                expr = key.expr,
            ),
            key.joins,
        ));
    }
    let key = order_key(out, order_by_param, None, false)?;
    let mut sql = format!(
        "ORDER BY {expr} {}",
        if key.descending { "DESC" } else { "ASC" },
        expr = key.expr,
    );
    if !order_by_param.contains("created_at") {
        sql.push_str(", issue.created_at DESC");
    }
    Ok((sql, key.joins))
}

/// The pre-L2 sub-issue list statement for one `order_by`.
pub fn sub_issues_list_sql(
    slug: &str,
    parent_id: uuid::Uuid,
    order_param: &str,
) -> Result<Stmt, Denial> {
    let (order, order_joins) = sub_issues_order(order_param)?;
    let mut binder = Binder::new();
    let selects = sub_issues_selects();
    let from_where = sub_issues_from_where(&mut binder, slug, parent_id);
    let row_where = with_order_joins(&from_where, &order_joins);
    Ok((
        format!("SELECT {selects} {row_where} {order}"),
        binder.values(),
    ))
}

fn one_uuid(sql: &str, id: uuid::Uuid) -> Stmt {
    (
        sql.to_owned(),
        vec![sea_query::Value::Uuid(Some(Box::new(id)))],
    )
}

pub fn state_lite_sql(id: uuid::Uuid) -> Stmt {
    one_uuid(
        "SELECT id, name, color, \"group\" FROM states WHERE id = $1",
        id,
    )
}

pub fn issue_lite_sql(id: uuid::Uuid) -> Stmt {
    one_uuid(
        "SELECT id, sequence_id, project_id FROM issues WHERE id = $1",
        id,
    )
}

pub fn assignees_sql(issue_id: uuid::Uuid) -> Stmt {
    one_uuid(
        "SELECT u.id FROM issue_assignees ia JOIN users u ON u.id = ia.assignee_id \
         WHERE ia.issue_id = $1 \
         ORDER BY u.created_at DESC",
        issue_id,
    )
}

pub fn labels_sql(issue_id: uuid::Uuid) -> Stmt {
    one_uuid(
        "SELECT l.parent_id, l.name, l.color, l.id, l.project_id, l.workspace_id, l.sort_order \
         FROM labels l JOIN issue_labels il ON il.label_id = l.id \
         WHERE il.issue_id = $1 AND l.deleted_at IS NULL \
         ORDER BY l.created_at DESC",
        issue_id,
    )
}

/// `outgoing` picks the link column; `counterpart` the audit columns'
/// table (the inline `issues-detail/` dicts read the counterpart issue's).
pub fn relations_sql(issue_id: uuid::Uuid, outgoing: bool, counterpart: bool) -> Stmt {
    let (side_col, other_col) = if outgoing {
        ("r.issue_id", "r.related_issue_id")
    } else {
        ("r.related_issue_id", "r.issue_id")
    };
    let audit = if counterpart {
        "o.created_by_id, o.created_at, o.updated_at, o.updated_by_id"
    } else {
        "r.created_by_id, r.created_at, r.updated_at, r.updated_by_id"
    };
    let sql = format!(
        "SELECT o.id, o.project_id, o.sequence_id, o.name, r.relation_type, o.state_id, \
                o.priority, {audit} \
         FROM issue_relations r JOIN issues o ON o.id = {other_col} \
         WHERE {side_col} = $1 AND r.deleted_at IS NULL \
         ORDER BY r.created_at DESC"
    );
    one_uuid(&sql, issue_id)
}

pub fn intake_lites_sql(issue_id: uuid::Uuid) -> Stmt {
    one_uuid(
        "SELECT id, status, duplicate_to_id, snoozed_till, source FROM intake_issues \
         WHERE issue_id = $1 AND deleted_at IS NULL ORDER BY created_at DESC",
        issue_id,
    )
}

pub fn reactions_sql(issue_id: uuid::Uuid) -> Stmt {
    one_uuid(
        "SELECT r.id, r.actor_id, r.issue_id, r.reaction, u.display_name AS display_name \
         FROM issue_reactions r JOIN users u ON u.id = r.actor_id \
         WHERE r.issue_id = $1 AND r.deleted_at IS NULL \
         ORDER BY r.created_at DESC",
        issue_id,
    )
}

pub fn links_sql(issue_id: uuid::Uuid) -> Stmt {
    one_uuid(
        "SELECT l.id, l.issue_id, l.title, l.url, l.metadata, l.created_by_id, l.created_at \
         FROM issue_links l WHERE l.issue_id = $1 AND l.deleted_at IS NULL ORDER BY l.created_at DESC",
        issue_id,
    )
}

pub fn attachments_sql(issue_id: uuid::Uuid) -> Stmt {
    one_uuid(
        "SELECT fa.id, fa.asset, fa.attributes, fa.created_by_id AS created_by, \
                fa.updated_at, fa.updated_by_id AS updated_by, fa.entity_type, \
                fa.project_id, fa.issue_id, w.slug AS workspace_slug \
         FROM file_assets fa JOIN workspaces w ON w.id = fa.workspace_id \
         WHERE fa.issue_id = $1 AND fa.entity_type = 'ISSUE_ATTACHMENT' AND fa.deleted_at IS NULL \
         ORDER BY fa.created_at DESC",
        issue_id,
    )
}

/// The pre-L2 identifier-full output names: each fragment's columns
/// stripped of the table qualifier and prefixed (the old `__f(...)`
/// column-alias list).
pub fn full_row_aliases() -> Vec<String> {
    fn stems(columns: &str, table: &str, prefix: &str) -> Vec<String> {
        let qualifier = format!("{table}.");
        columns
            .split(',')
            .map(|column| {
                let bare = column
                    .trim()
                    .strip_prefix(qualifier.as_str())
                    .expect("qualified");
                format!("{prefix}{}", bare.replace('"', ""))
            })
            .collect()
    }
    let mut aliases = stems(ISSUE_COLUMNS, "issue", "i_");
    aliases.extend(
        [
            "cycle_id",
            "link_count",
            "attachment_count",
            "sub_issues_count",
            "label_ids",
            "assignee_ids",
            "module_ids",
            "is_subscribed",
            "is_intake",
        ]
        .iter()
        .map(|name| (*name).to_owned()),
    );
    aliases.extend(stems(PROJECT_COLUMNS, "projects", "p_"));
    aliases.extend(stems(WORKSPACE_COLUMNS, "workspaces", "w_"));
    aliases.extend(stems(PARENT_ISSUE_COLUMNS, "parent", "par_"));
    aliases.extend(stems(STATE_COLUMNS, "state", "s_"));
    aliases.extend(stems(TICKER_COLUMNS, "issue_agent_ticker", "t_"));
    aliases
}

/// The pre-L2 identifier-full statement with its column-alias list.
pub fn identifier_full_aliased_sql(
    slug: &str,
    project_id: uuid::Uuid,
    sequence_id: i32,
    user_id: uuid::Uuid,
) -> Stmt {
    let mut binder = Binder::new();
    let full = identifier_full_sql(&mut binder, slug, project_id, sequence_id, user_id);
    let inner = format!(
        "SELECT * FROM ({full}) AS __f({})",
        full_row_aliases().join(",")
    );
    (inner, binder.values())
}

#[cfg(test)]
mod pins {
    use super::super::queries_core::{ISSUE_FIELDS, PROJECT_FIELDS, STATE_FIELDS};
    use super::super::queries_engage::{LITE_ISSUE_FIELDS, TICKER_FIELDS, WORKSPACE_FIELDS};
    use super::*;
    use sea_orm::IdenStatic;

    fn stems(columns: &str, table: &str) -> Vec<String> {
        let qualifier = format!("{table}.");
        columns
            .split(',')
            .map(|column| {
                column
                    .trim()
                    .strip_prefix(qualifier.as_str())
                    .expect("qualified")
                    .replace('"', "")
            })
            .collect()
    }

    fn names<C: IdenStatic>(columns: &[C]) -> Vec<String> {
        columns.iter().map(|c| c.as_str().to_owned()).collect()
    }

    /// The typed column lists are the old fragment consts, column for
    /// column, in the same (Django) order.
    #[test]
    fn typed_column_lists_match_the_text_fragments() {
        assert_eq!(names(&ISSUE_FIELDS), stems(ISSUE_COLUMNS, "issue"));
        assert_eq!(names(&ISSUE_FIELDS), stems(PARENT_ISSUE_COLUMNS, "parent"));
        assert_eq!(names(&STATE_FIELDS), stems(STATE_COLUMNS, "state"));
        assert_eq!(names(&PROJECT_FIELDS), stems(PROJECT_COLUMNS, "projects"));
        assert_eq!(
            names(&WORKSPACE_FIELDS),
            stems(WORKSPACE_COLUMNS, "workspaces")
        );
        assert_eq!(
            names(&TICKER_FIELDS),
            stems(TICKER_COLUMNS, "issue_agent_ticker")
        );
        assert_eq!(
            names(&LITE_ISSUE_FIELDS),
            stems(LITE_ISSUE_COLUMNS, "issue")
        );
        assert_eq!(full_row_aliases().len(), 177);
    }
}

#[cfg(test)]
mod live {
    use super::super::fetch_json_rows;
    use super::super::list_oracle::live::{
        seed, uid, C_ONE, L_ALPHA, L_BETA, PROJECT, S_BACKLOG, S_DONE, S_GONE, S_STARTED, S_TRIAGE,
        U_ANN, U_BOB, U_CID, WS,
    };
    use super::super::{
        expand, handlers_reads, list_engine as engine, queries_core, queries_engage,
    };
    use super::*;
    use sea_orm::QueryTrait;
    use sea_query::SelectStatement;
    use serde_json::{Map, Value};

    fn q(id: u128) -> String {
        format!("'{}'", uid(id))
    }

    /// Tables the list seed does not create, plus the extra columns the
    /// reads render (the list seed leaves them NULL).
    async fn seed_reads(pool: &sqlx::PgPool) {
        for table in [
            "project_user_properties",
            "issue_reactions",
            "issue_relations",
            "issue_agent_ticker",
        ] {
            sqlx::query(&format!(
                "CREATE TEMP TABLE {table} AS SELECT * FROM public.{table} WITH NO DATA"
            ))
            .execute(pool)
            .await
            .expect("temp table");
        }
        let at = |minutes: u32| {
            format!("timestamptz '2026-10-02 09:00:00.654321+00' + interval '{minutes} min'")
        };
        let statements = vec![
            format!(
                "UPDATE projects SET identifier = CASE WHEN id = {} THEN 'ACME' ELSE 'OTH' END, \
                 name = 'P', created_at = {}, network = 2",
                q(PROJECT),
                at(0)
            ),
            format!("UPDATE workspaces SET name = 'Acme', created_at = {}", at(0)),
            "UPDATE states SET color = '#abc', slug = name, sequence = 1".to_owned(),
            format!(
                "UPDATE users SET display_name = first_name || '_d', \
                 created_at = CASE first_name WHEN 'Ann' THEN {} WHEN 'Bob' THEN {} ELSE {} END",
                at(1),
                at(2),
                at(3)
            ),
            format!(
                "UPDATE labels SET color = '#123', sort_order = 1.5, parent_id = \
                 CASE WHEN id = {} THEN {}::uuid END, \
                 created_at = CASE WHEN id = {} THEN {} ELSE {} END",
                q(L_BETA),
                q(L_ALPHA),
                q(L_ALPHA),
                at(4),
                at(5)
            ),
            format!(
                "UPDATE issue_links SET url = 'https://x.test/' || id, title = 't', \
                 metadata = '{{\"b\": 1, \"a\": [2, 3]}}', created_by_id = {}, \
                 created_at = {} + (random() * interval '10 min')",
                q(U_ANN),
                at(6)
            ),
            format!(
                "UPDATE file_assets SET workspace_id = {}, project_id = {}, asset = 'a/b.png', \
                 attributes = '{{\"name\": \"b.png\", \"size\": 10}}', created_by_id = {}, \
                 updated_at = {u}, created_at = {u} + (random() * interval '10 min')",
                q(WS),
                q(PROJECT),
                q(U_BOB),
                u = at(7)
            ),
            format!(
                "UPDATE intake_issues SET project_id = {}, workspace_id = {}, source = 'in_app', \
                 snoozed_till = CASE status WHEN 0 THEN {} END, created_at = {} + (random() * interval '10 min')",
                q(PROJECT),
                q(WS),
                at(8),
                at(9)
            ),
            format!(
                "INSERT INTO intake_issues (id, issue_id, status, project_id, workspace_id, created_at) \
                 VALUES (gen_random_uuid(), {}, -2, {}, {}, {})",
                q(100),
                q(PROJECT),
                q(WS),
                at(30)
            ),
            format!(
                "INSERT INTO git_issue_syncs (id, issue_id, deleted_at) VALUES (gen_random_uuid(), {}, NULL)",
                q(102)
            ),
            format!(
                "INSERT INTO project_user_properties (id, user_id, project_id, workspace_id, created_at, \
                 updated_at, deleted_at, filters, display_filters, display_properties, rich_filters, \
                 preferences, sort_order) VALUES \
                 (gen_random_uuid(), {ann}, {p}, {w}, {a1}, {a1}, NULL, '{{\"x\": null}}', '{{\"layout\": \"list\"}}', \
                  '{{\"key\": true}}', '{{}}', '{{\"pages\": {{\"block_display\": true}}}}', 65535), \
                 (gen_random_uuid(), {ann}, {p}, {w}, {a2}, {a2}, NULL, '{{}}', '{{}}', '{{}}', '{{}}', '{{}}', 100.25), \
                 (gen_random_uuid(), {ann}, {p}, {w}, {a3}, {a3}, now(), '{{}}', '{{}}', '{{}}', '{{}}', '{{}}', 1)",
                ann = q(U_ANN),
                p = q(PROJECT),
                w = q(WS),
                a1 = at(10),
                a2 = at(11),
                a3 = at(12)
            ),
            format!(
                "INSERT INTO issue_reactions (id, issue_id, actor_id, reaction, project_id, workspace_id, \
                 created_at, deleted_at) VALUES \
                 (gen_random_uuid(), {i}, {ann}, '👍', {p}, {w}, {a1}, NULL), \
                 (gen_random_uuid(), {i}, {bob}, 'tada', {p}, {w}, {a2}, NULL), \
                 (gen_random_uuid(), {i}, {cid}, 'gone', {p}, {w}, {a2}, now())",
                i = q(100),
                ann = q(U_ANN),
                bob = q(U_BOB),
                cid = q(U_CID),
                p = q(PROJECT),
                w = q(WS),
                a1 = at(13),
                a2 = at(14)
            ),
            format!(
                "INSERT INTO issue_relations (id, issue_id, related_issue_id, relation_type, project_id, \
                 workspace_id, created_at, updated_at, created_by_id, updated_by_id, deleted_at) VALUES \
                 (gen_random_uuid(), {i100}, {i101}, 'blocked_by', {p}, {w}, {a1}, {a1}, {ann}, NULL, NULL), \
                 (gen_random_uuid(), {i102}, {i100}, 'relates_to', {p}, {w}, {a2}, {a3}, NULL, {bob}, NULL), \
                 (gen_random_uuid(), {i100}, {i110}, 'duplicate', {p}, {w}, {a3}, {a3}, NULL, NULL, NULL), \
                 (gen_random_uuid(), {i100}, {i103}, 'start_before', {p}, {w}, {a4}, {a4}, NULL, NULL, now())",
                i100 = q(100),
                i101 = q(101),
                i102 = q(102),
                i103 = q(103),
                i110 = q(110),
                ann = q(U_ANN),
                bob = q(U_BOB),
                p = q(PROJECT),
                w = q(WS),
                a1 = at(15),
                a2 = at(16),
                a3 = at(17),
                a4 = at(18)
            ),
            format!(
                "INSERT INTO issue_agent_ticker (id, issue_id, created_at, updated_at, used, granted, \
                 waited, user_disabled, enabled, next_run_at, pending_entry, pending_entry_free) VALUES \
                 (gen_random_uuid(), {}, {a}, {a}, 2, 5, 1, false, true, {a}, false, true)",
                q(100),
                a = at(19)
            ),
        ];
        for statement in statements {
            sqlx::query(&statement)
                .execute(pool)
                .await
                .unwrap_or_else(|error| panic!("{statement}: {error}"));
        }
    }

    async fn old_rows(pool: &sqlx::PgPool, (sql, values): Stmt) -> Result<String, Denial> {
        let rows = fetch_json_rows(pool, &sql, values).await?;
        Ok(serde_json::to_string(&rows).expect("json"))
    }

    async fn new_rows(pool: &sqlx::PgPool, stmt: SelectStatement) -> Result<String, Denial> {
        let rows = engine::fetch_rows(pool, stmt).await?;
        Ok(serde_json::to_string(&rows).expect("json"))
    }

    /// Old text vs new built statement: same success, same rows, same
    /// order, byte for byte. Returns the row count.
    async fn same(pool: &sqlx::PgPool, label: &str, old: Stmt, new: SelectStatement) -> usize {
        let sql = old.0.clone();
        let built = new.to_string(sea_query::PostgresQueryBuilder);
        let old = old_rows(pool, old).await;
        let new = new_rows(pool, new).await;
        match (old, new) {
            (Ok(old), Ok(new)) => {
                assert_eq!(old, new, "{label}\nold: {sql}\nnew: {built}");
                serde_json::from_str::<Vec<Value>>(&new)
                    .expect("rows")
                    .len()
            }
            (old, new) => panic!(
                "{label}: {:?} vs {:?}\nold: {sql}\nnew: {built}",
                old.err(),
                new.err()
            ),
        }
    }

    /// Unordered statements compare as sorted rows.
    async fn same_set(pool: &sqlx::PgPool, label: &str, old: Stmt, new: SelectStatement) -> usize {
        let old: Vec<Map<String, Value>> =
            serde_json::from_str(&old_rows(pool, old).await.expect("old")).expect("rows");
        let new: Vec<Map<String, Value>> =
            serde_json::from_str(&new_rows(pool, new).await.expect("new")).expect("rows");
        let sorted = |rows: Vec<Map<String, Value>>| {
            let mut out: Vec<String> = rows
                .iter()
                .map(|r| serde_json::to_string(r).expect("row"))
                .collect();
            out.sort();
            out
        };
        assert_eq!(sorted(old), sorted(new.clone()), "{label}");
        new.len()
    }

    const SUB_ORDERS: &[&str] = &[
        "",
        "-created_at",
        "created_at",
        "-updated_at",
        "sort_order",
        "priority",
        "-priority",
        "state__group",
        "-state__group",
        "labels__name",
        "-assignees__first_name",
        "issue_module__module__name",
        "start_date",
        "-target_date",
        "name",
        "estimate_point__key",
        "state__name",
        "-cycle_id",
        "link_count",
        "-attachment_count",
        "sub_issues_count",
        "issue_cycle__cycle__name",
        "-issue_cycle__cycle__name",
        "created_by",
        "-id",
    ];

    #[tokio::test]
    async fn live_reads_match_text_statements() {
        let Ok(url) = std::env::var("DATABASE_URL") else {
            eprintln!("skipping live-db test: DATABASE_URL is not set");
            return;
        };
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .expect("connect");
        seed(&pool).await;
        seed_reads(&pool).await;
        let db = pidash_db::pool::sea_orm_connection(&pool);
        let issue_ids: Vec<uuid::Uuid> = (100..=114).chain([999]).map(uid).collect();
        let mut rows = 0usize;
        let mut checks = 0usize;
        let mut covered: std::collections::BTreeMap<&str, usize> = Default::default();
        // Every read returns rows somewhere in the matrix (not only misses).
        macro_rules! cover {
            ($label:expr, $n:expr $(,)?) => {{
                let n = $n;
                *covered.entry($label).or_default() += n;
                rows += n;
                checks += 1;
            }};
        }

        // User properties: newest live row; none for another user.
        for user in [U_ANN, U_BOB] {
            cover!(
                "property",
                same(
                    &pool,
                    "property",
                    property_sql(uid(user), uid(PROJECT)),
                    handlers_reads::property_select(uid(user), uid(PROJECT)),
                )
                .await,
            );
        }

        // Bulk dates, sub-issue POST scope and rows.
        for (slug, ids) in [
            ("acme", issue_ids.clone()),
            ("acme", vec![uid(100), uid(104), uid(110)]),
            ("other", issue_ids.clone()),
        ] {
            cover!(
                "bulk",
                same_set(
                    &pool,
                    "bulk",
                    bulk_issues_sql(slug, uid(PROJECT), &ids),
                    handlers_reads::bulk_issues_select(slug, &uid(PROJECT), &ids),
                )
                .await,
            );
        }
        cover!(
            "scoped ids",
            same_set(
                &pool,
                "scoped ids",
                scoped_issue_ids_sql(&issue_ids),
                handlers_reads::scoped_issue_ids_select(&issue_ids),
            )
            .await,
        );
        cover!(
            "post rows",
            same(
                &pool,
                "post rows",
                post_rows_sql(&issue_ids),
                handlers_reads::post_rows_select(&issue_ids),
            )
            .await,
        );
        for id in &issue_ids {
            let mut one = handlers_reads::scoped_issue_ids_select(&[*id]);
            one.limit(1);
            cover!(
                "scoped issue",
                same(&pool, "scoped issue", scoped_issue_sql(*id), one).await
            );
        }

        // Identifier project, member and guest gates.
        for (slug, identifier) in [
            ("acme", "acme"),
            ("acme", "ACME"),
            ("acme", "oTh"),
            ("acme", "zzz"),
            ("other", "ACME"),
        ] {
            let mut binder = Binder::new();
            let old = identifier_project_sql(&mut binder, slug, identifier);
            cover!(
                "identifier project",
                same(
                    &pool,
                    "identifier project",
                    (old, binder.values()),
                    queries_engage::identifier_project_select(slug, identifier),
                )
                .await,
            );
        }
        for user in [U_ANN, U_BOB, U_CID, 999] {
            for slug in ["acme", "other"] {
                let mut binder = Binder::new();
                let old = identifier_member_exists_sql(&mut binder, slug, uid(PROJECT), uid(user));
                let old_hit = !fetch_json_rows(&pool, &old, binder.values())
                    .await
                    .expect("old")
                    .is_empty();
                let new_hit =
                    queries_engage::identifier_member_select(slug, uid(PROJECT), uid(user))
                        .into_tuple::<uuid::Uuid>()
                        .one(&db)
                        .await
                        .expect("new")
                        .is_some();
                assert_eq!(old_hit, new_hit, "member {user} {slug}");
                let mut binder = Binder::new();
                let old = guest_view_exists_sql(&mut binder, slug, uid(PROJECT), uid(user));
                let old_hit = !fetch_json_rows(&pool, &old, binder.values())
                    .await
                    .expect("old")
                    .is_empty();
                let new_hit = queries_core::guest_view_select(slug, uid(PROJECT), uid(user))
                    .into_tuple::<uuid::Uuid>()
                    .one(&db)
                    .await
                    .expect("new")
                    .is_some();
                assert_eq!(old_hit, new_hit, "guest {user} {slug}");
                checks += 2;
            }
        }

        // Per-issue reads: meta, lite, full, archive retrieve, expand nests.
        for (index, id) in issue_ids.iter().enumerate() {
            let sequence = 100 + index as i32;
            for slug in ["acme", "other"] {
                let mut binder = Binder::new();
                let old = meta_sql(&mut binder, slug, uid(PROJECT), *id);
                let stmt = queries_engage::meta_select(slug, uid(PROJECT), *id);
                cover!(
                    "meta",
                    same(&pool, "meta", (old, binder.values()), stmt.clone()).await
                );
                engine::query_built::<queries_engage::MetaRow>(&db, &stmt)
                    .await
                    .expect("meta decodes");
                let mut binder = Binder::new();
                let old = identifier_lite_sql(&mut binder, slug, uid(PROJECT), sequence);
                cover!(
                    "lite",
                    same(
                        &pool,
                        "lite",
                        (old, binder.values()),
                        queries_engage::identifier_lite_select(slug, uid(PROJECT), sequence),
                    )
                    .await,
                );
                for user in [U_ANN, U_BOB] {
                    let label = format!("full {sequence} {slug} {user}");
                    let n = same(
                        &pool,
                        &label,
                        identifier_full_aliased_sql(slug, uid(PROJECT), sequence, uid(user)),
                        queries_engage::identifier_full_select(
                            slug,
                            uid(PROJECT),
                            sequence,
                            uid(user),
                        ),
                    )
                    .await;
                    cover!("full", n);
                    let mut binder = Binder::new();
                    let old = archive_retrieve_sql(&mut binder, slug, uid(PROJECT), *id, uid(user));
                    cover!(
                        "archive retrieve",
                        same(
                            &pool,
                            "archive retrieve",
                            (old, binder.values()),
                            queries_core::archive_retrieve_select(
                                slug,
                                uid(PROJECT),
                                *id,
                                uid(user)
                            ),
                        )
                        .await,
                    );
                }
            }
            cover!(
                "issue lite",
                same(
                    &pool,
                    "issue lite",
                    issue_lite_sql(*id),
                    expand::issue_lite_select(*id).into_query()
                )
                .await
            );
            cover!(
                "assignees",
                same(
                    &pool,
                    "assignees",
                    assignees_sql(*id),
                    expand::assignees_select(*id).into_query()
                )
                .await
            );
            cover!(
                "labels",
                same(
                    &pool,
                    "labels",
                    labels_sql(*id),
                    expand::labels_select(*id).into_query()
                )
                .await
            );
            for (outgoing, side) in [
                (true, expand::RelationSide::Outgoing),
                (false, expand::RelationSide::Incoming),
            ] {
                for (counterpart, audit) in [
                    (false, expand::RelationAudit::Relation),
                    (true, expand::RelationAudit::Counterpart),
                ] {
                    cover!(
                        "relations",
                        same(
                            &pool,
                            "relations",
                            relations_sql(*id, outgoing, counterpart),
                            expand::relations_select(*id, side, audit),
                        )
                        .await,
                    );
                }
            }
            cover!(
                "intake",
                same(
                    &pool,
                    "intake",
                    intake_lites_sql(*id),
                    expand::intake_lites_select(*id)
                )
                .await
            );
            cover!(
                "reactions",
                same(
                    &pool,
                    "reactions",
                    reactions_sql(*id),
                    expand::reactions_select(*id).into_query()
                )
                .await
            );
            cover!(
                "links",
                same(&pool, "links", links_sql(*id), expand::links_select(*id)).await
            );
            cover!(
                "attachments",
                same(
                    &pool,
                    "attachments",
                    attachments_sql(*id),
                    expand::attachments_select(*id)
                )
                .await
            );
        }
        for state in [S_BACKLOG, S_STARTED, S_DONE, S_TRIAGE, S_GONE, 999] {
            cover!(
                "state lite",
                same(
                    &pool,
                    "state lite",
                    state_lite_sql(uid(state)),
                    expand::state_lite_select(uid(state)).into_query(),
                )
                .await,
            );
        }

        // Sub-issue lists: every order, three parents, two slugs.
        for parent in [100, 102, 101] {
            for slug in ["acme", "other"] {
                for param in SUB_ORDERS {
                    let label = format!("sub-issues {parent} {slug} {param:?}");
                    let old = sub_issues_list_sql(slug, uid(parent), param);
                    let mut stmt = queries_core::sub_issues_select(slug, uid(parent));
                    let new = handlers_reads::sub_issues_order(&mut stmt, param);
                    match (old, new) {
                        (Ok(old), Ok(())) => {
                            cover!("sub-issues", same(&pool, &label, old, stmt).await)
                        }
                        (Err(_), Err(_)) => checks += 1,
                        (old, new) => panic!("{label}: {:?} vs {:?}", old.err(), new.err()),
                    }
                }
                assert!(sub_issues_order("nope").is_err());
            }
        }
        let _ = C_ONE;
        eprintln!("live reads: {checks} checks, {rows} rows compared: {covered:?}");
        for (label, n) in &covered {
            assert!(*n > 0, "{label}: no rows compared");
        }
        assert!(rows > 200, "too few rows compared: {rows}");
    }
}
