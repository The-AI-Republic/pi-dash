#![forbid(unsafe_code)]

//! Activity / comment / version / meta / identifier read querysets (D-26 queries-B).
//!
//! Ports the annotated read querysets behind the issue history, comment,
//! version, meta and identifier endpoints to SQL text, extending pilot-2's
//! api-crate query pattern (`super::queries_core` fragments, [`Binder`] `$n`
//! binds, `super::render::V2Page` page math). Each builder returns a fragment
//! or a complete statement the handler executes; handlers own auth gates,
//! filter stacks, ordering variants, pagination envelopes and response shaping.
//!
//! Sources (drift baseline `01a93e17`):
//! - `app/views/issue/activity.py:30-86` — `IssueActivityEndpoint.get`
//!   (activities `:35-46`, comments `:47-64`, `issue-property` branch
//!   `:66-75`, `issue-comment` branch `:77-79`, default branch `:81-85`).
//! - `app/views/issue/comment.py:43-69` — `IssueCommentViewSet.get_queryset`
//!   (selects `:55-57`, `is_member` `:58-67`, `distinct` `:68`) and
//!   `:186-200` — `CommentReactionViewSet.get_queryset`.
//! - `app/views/issue/version.py:36-144` — `IssueVersionEndpoint.get`
//!   (detail `:38-44`, list `:46-74`) and
//!   `WorkItemDescriptionVersionEndpoint.get` (detail `:107-116`, list
//!   `:118-144`), both paginated through `utils/global_paginator.py:33-87`.
//! - `app/views/issue/base.py:1199-1211` — `IssueMetaEndpoint.get`.
//! - `app/views/issue/base.py:1214-1489` — `IssueDetailIdentifierEndpoint.get`
//!   (project lookup `:1253`, member gate `:1256-1265`, lite `:1267-1310`,
//!   full `:1312-1446`, guest gate `:1460-1474`).
//!
//! Fixture oracle: FX-ISS-12
//! (`rust-api/fixtures/app_issues/queries/FX-ISS-12.engage.json`). Every
//! builder below was verified against live-compiled Django 4.2.30 ORM SQL
//! (`str(qs.query)` — no database needed, plus a replicated `get_count`
//! rendering for the two `paginate` counts); the unit tests pin the
//! verified text so transcription drift fails the build.
//!
//! Reuse (deliberate, verified live — nothing is re-ported):
//! - Column lists, the `cycle_id` / count / array annotation texts,
//!   `is_subscribed`, the reaction/link prefetch statements and the
//!   guest-view gate are `super::queries_core` fragments, referenced, not
//!   copied. Pin tests assert the shared texts still match.
//! - Page math is pilot-2's `super::render::v2_page` (the
//!   `global_paginator.paginate` port): handlers compute the [`V2Page`],
//!   version builders render its `LIMIT`/`OFFSET` only.
//! - The identifier reaction/link prefetches are
//!   `queries_core::{reaction_prefetch_sql, link_prefetch_sql}` verbatim
//!   (same `Prefetch` chains as retrieve); the guest-view gate is
//!   `queries_core::guest_view_exists_sql` verbatim.
//! - The identifier array subqueries carry Django's doubled `deleted_at`
//!   guard (explicit `deleted_at__isnull=True` plus the manager's); one
//!   guard is ported, the doubling is unobservable (queries-A bug 6).
//! - The `NULLIF(COUNT(*), 0)` count idiom reproduces Django's grouped
//!   `COUNT(id)` subquery, which yields no row (SQL `NULL`) when empty
//!   (queries-A CAGG class, shared text).
//!
//! `Issue.objects` vs `issue_objects` (load-bearing, verified live): the
//! identifier lite/full lookups use the plain `Issue.objects` manager, so
//! triage-state, archived and draft rows ARE visible there; only the meta
//! lookup uses the scoped `issue_objects` manager (triage/archived/draft
//! exclusions). [`meta_sql`] carries the scope, the identifier builders
//! carry none.
//!
//! Existing bugs ported as-is (translation, don't redesign):
//! 1. The history default branch subscripts model instances
//!    (`instance["created_at"]`, `activity.py:81-84`) and raises
//!    `TypeError` → generic 500 whenever `?activity_type` is absent or
//!    unknown. Handler behavior, not SQL: the two querysets below serve
//!    all three branches; the handler owns the 500.
//! 2. Lite `is_synced` is a git-only `Exists` over `git_issue_syncs`
//!    (unscoped — no project/workspace predicate) that the
//!    `_issue_is_actively_synced` predicate honors, so lite rows report
//!    Github-synced issues as unsynced. Ported as-is.
//! 3. Version `paginate`: a malformed cursor raises `ValueError` and a
//!    zero-size cursor raises `ZeroDivisionError`, both uncaught → generic
//!    500. `render::v2_page` already fails both inputs; the handler maps
//!    the failure to the 500.
//! 4. The history/comment member join carries no `deleted_at` guard
//!    (`filter()` joins never apply the related manager), so a deleted
//!    membership still authorizes — and, on the `DISTINCT`-less history
//!    paths, duplicates rows. Ported as-is (queries-A bug 7 class).
//!
//! Deliberate consolidations (same values, fewer round trips):
//! - Meta: Django's `.only("sequence_id", "project__identifier")` selects
//!   three issue columns and lazy-loads `project.identifier` in a second
//!   query (a spanning `.only()` without `select_related` never joins).
//!   [`meta_sql`] is one statement selecting both values over the join the
//!   manager scope already requires.
//! - `.get()` lookups port `LIMIT 21` as `LIMIT 1`: every one is over a
//!   unique key (version pk; project `(identifier, workspace)` partial
//!   unique plus `save()` upper-normalization), so `MultipleObjectsReturned`
//!   is unreachable through the app (queries-A `.get()` precedent).
//! - Django's generated table aliases (`T4`/`T7` actors and parents, `U0`
//!   subqueries) are normalized to readable aliases (`users`, `parent`,
//!   short subquery aliases); a semantic no-op.
//!
//! Out of scope (owned by sibling handler/guard/serializer/task issues):
//! the permission gates (`resolve_gate`, `@allow_permission`, the
//! `strict_str_to_int` / `is_lite_request` pure helpers), the comment
//! `filterset_fields` splice (`issue__id`, `workspace__id`), the
//! `paginate` envelopes (keys + `user_timezone_converter`, via [`V2Page`]),
//! the lite/full serializer shapes (the 15-key lite dict, `LITE_KEYS`),
//! the `recent_visited_task` / `issue_activity` enqueues, and the
//! identifier full `assignees` / `labels` / `issue_module__module` M2M
//! prefetches (handler-owned, pilot-2 prefetch pattern — queries-A
//! archive precedent). Queryset-adjacent constants handlers need (the
//! `.values()` key orders) ARE in scope and marked.

use super::queries_core::{
    attachment_count_select, is_subscribed_select, link_count_select, sub_issues_count_select,
    ASSIGNEE_IDS_ACTIVE_SELECT, CYCLE_ID_SELECT, ISSUE_COLUMNS, LABEL_IDS_SELECT,
    MODULE_IDS_SELECT, PREFETCH_ISSUE_COLUMNS, PROJECT_COLUMNS, STATE_COLUMNS, USER_COLUMNS,
};
use super::render::V2Page;
use super::Binder;

// ---------------------------------------------------------------------------
// Shared column lists (Django concrete-field order, live-compiled)
// ---------------------------------------------------------------------------

/// All 20 concrete `issue_activities` columns (history base-table order).
pub const ACTIVITY_COLUMNS: &str = "issue_activities.created_at, issue_activities.updated_at, \
    issue_activities.created_by_id, issue_activities.updated_by_id, issue_activities.deleted_at, \
    issue_activities.id, issue_activities.project_id, issue_activities.workspace_id, \
    issue_activities.issue_id, issue_activities.verb, issue_activities.field, \
    issue_activities.old_value, issue_activities.new_value, issue_activities.comment, \
    issue_activities.attachments, issue_activities.issue_comment_id, issue_activities.actor_id, \
    issue_activities.old_identifier, issue_activities.new_identifier, issue_activities.epoch";

/// All 24 concrete `issue_comments` columns (history / comment base order).
pub const COMMENT_COLUMNS: &str = "issue_comments.created_at, issue_comments.updated_at, \
    issue_comments.created_by_id, issue_comments.updated_by_id, issue_comments.deleted_at, \
    issue_comments.id, issue_comments.project_id, issue_comments.workspace_id, \
    issue_comments.comment_stripped, issue_comments.comment_json, issue_comments.comment_html, \
    issue_comments.description_id, issue_comments.attachments, issue_comments.labels, \
    issue_comments.issue_id, issue_comments.actor_id, issue_comments.access, \
    issue_comments.external_source, issue_comments.external_id, issue_comments.speaker_type, \
    issue_comments.speaker_label, issue_comments.speaker_agent_run_id, issue_comments.edited_at, \
    issue_comments.parent_id";

/// All 11 concrete `comment_reactions` columns (prefetch / list base order).
pub const COMMENT_REACTION_COLUMNS: &str = "comment_reactions.created_at, \
    comment_reactions.updated_at, comment_reactions.created_by_id, \
    comment_reactions.updated_by_id, comment_reactions.deleted_at, comment_reactions.id, \
    comment_reactions.project_id, comment_reactions.workspace_id, comment_reactions.actor_id, \
    comment_reactions.comment_id, comment_reactions.reaction";

/// All 33 concrete `issue_versions` columns (detail base-table order). The
/// snapshot references (`parent`, `state`, `estimate_point`, `type`, `cycle`)
/// are plain UUID columns without the `_id` suffix.
pub const VERSION_COLUMNS: &str = "issue_versions.created_at, issue_versions.updated_at, \
    issue_versions.created_by_id, issue_versions.updated_by_id, issue_versions.deleted_at, \
    issue_versions.id, issue_versions.project_id, issue_versions.workspace_id, \
    issue_versions.parent, issue_versions.state, issue_versions.estimate_point, \
    issue_versions.name, issue_versions.priority, issue_versions.start_date, \
    issue_versions.target_date, issue_versions.assignees, issue_versions.sequence_id, \
    issue_versions.labels, issue_versions.sort_order, issue_versions.completed_at, \
    issue_versions.archived_at, issue_versions.is_draft, issue_versions.external_source, \
    issue_versions.external_id, issue_versions.type, issue_versions.cycle, \
    issue_versions.modules, issue_versions.properties, issue_versions.meta, \
    issue_versions.last_saved_at, issue_versions.issue_id, issue_versions.activity_id, \
    issue_versions.owned_by_id";

/// All 15 concrete `issue_description_versions` columns (detail base order).
pub const DESCRIPTION_VERSION_COLUMNS: &str = "issue_description_versions.created_at, \
    issue_description_versions.updated_at, issue_description_versions.created_by_id, \
    issue_description_versions.updated_by_id, issue_description_versions.deleted_at, \
    issue_description_versions.id, issue_description_versions.project_id, \
    issue_description_versions.workspace_id, issue_description_versions.issue_id, \
    issue_description_versions.description_binary, issue_description_versions.description_html, \
    issue_description_versions.description_stripped, issue_description_versions.description_json, \
    issue_description_versions.last_saved_at, issue_description_versions.owned_by_id";

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

/// All 34 concrete `issues` columns under the `parent` self-join alias
/// (identifier-full `select_related("parent")`; Django renders `T4`).
pub const PARENT_ISSUE_COLUMNS: &str = "parent.created_at, parent.updated_at, \
    parent.created_by_id, parent.updated_by_id, parent.deleted_at, parent.id, \
    parent.project_id, parent.workspace_id, parent.parent_id, parent.state_id, parent.point, \
    parent.estimate_point_id, parent.name, parent.description_json, parent.description_html, \
    parent.description_stripped, parent.description_binary, parent.priority, \
    parent.complexity_score, parent.start_date, parent.target_date, parent.sequence_id, \
    parent.sort_order, parent.completed_at, parent.archived_at, parent.is_draft, \
    parent.external_source, parent.external_id, parent.type_id, parent.git_work_branch, \
    parent.workpad, parent.created_via, parent.assigned_pod_id, parent.agent_executor";

/// Identifier-lite `.only()` select list in Django's RENDER order: `.only()`
/// honors model field order, not the call order. `external_source` rides
/// along for the `_issue_is_actively_synced` short-circuit (serializers
/// own the predicate; the queries layer keeps its inputs loaded).
pub const LITE_ISSUE_COLUMNS: &str = "issue.created_at, issue.updated_at, issue.created_by_id, \
    issue.updated_by_id, issue.id, issue.project_id, issue.workspace_id, issue.name, \
    issue.description_html, issue.sequence_id, issue.sort_order, issue.archived_at, \
    issue.is_draft, issue.external_source";

/// History intake prefetch `.only("source_email", "source", "extra")` in
/// Django's RENDER order — model field order (`source` before
/// `source_email`), plus the always-selected pk.
pub const INTAKE_SOURCE_SELECTS: &str = "intake_issues.id, intake_issues.source, \
    intake_issues.source_email, intake_issues.extra";

// ---------------------------------------------------------------------------
// History (activity.py:30-86)
// ---------------------------------------------------------------------------

/// `select_related` column order for the history paths: Django renders
/// related tables in model field order (inherited `project`, `workspace`
/// first, then declared FKs), not in `select_related()` call order — so
/// `("actor", "workspace", "issue", "project")` and
/// `("actor", "issue", "project", "workspace")` both render projects,
/// workspaces, issues, users.
fn history_related_selects() -> String {
    format!("{PROJECT_COLUMNS}, {WORKSPACE_COLUMNS}, {PREFETCH_ISSUE_COLUMNS}, {USER_COLUMNS}")
}

/// History member-gate joins, shared by both history querysets
/// (`activity.py:35-55`): project membership (unguarded — bug 4), the
/// archived-project exclusion and the workspace slug.
fn history_joins(base_table: &str) -> String {
    format!(
        "FROM {base_table} \
        INNER JOIN issues ON {base_table}.issue_id = issues.id \
        INNER JOIN projects ON {base_table}.project_id = projects.id \
        INNER JOIN project_members ON projects.id = project_members.project_id \
        INNER JOIN workspaces ON {base_table}.workspace_id = workspaces.id \
        LEFT OUTER JOIN users ON {base_table}.actor_id = users.id"
    )
}

/// History member-gate predicates. Binds: issue id, user id, workspace slug.
fn history_where(
    binder: &mut Binder,
    base_table: &str,
    slug: &str,
    issue_id: uuid::Uuid,
    user_id: uuid::Uuid,
) -> String {
    let issue = binder.bind_uuid(issue_id);
    let user = binder.bind_uuid(user_id);
    let slug_holder = binder.bind_string(slug.to_owned());
    format!(
        "{base_table}.deleted_at IS NULL AND {base_table}.issue_id = {issue} \
        AND projects.archived_at IS NULL AND project_members.is_active \
        AND project_members.member_id = {user} AND workspaces.slug = {slug_holder}"
    )
}

/// Optional `?created_at__gt=` passthrough (`activity.py:31-33`): Django
/// parses the raw query string into a timestamptz comparison. The builder
/// takes the raw value and binds it as text; Postgres coerces
/// text→timestamptz in the comparison, yielding the same rows for valid
/// inputs. Invalid inputs raise in Django (generic 500); the handler
/// validates before calling.
fn created_after_predicate(
    binder: &mut Binder,
    base_table: &str,
    created_after: Option<&str>,
) -> String {
    match created_after {
        None => String::new(),
        Some(raw) => {
            let holder = binder.bind_string(raw.to_owned());
            format!(" AND {base_table}.created_at > {holder}")
        }
    }
}

/// Full history-activities statement (`activity.py:35-46`): the property
/// feed minus the `comment`/`vote`/`reaction`/`draft` fields, oldest
/// first. The negated `IN` keeps Django's exact null semantics — `NOT
/// (field IN (...) AND field IS NOT NULL)` INCLUDES null-field rows,
/// where a bare `NOT IN` would drop them. Binds, in order: issue id, user
/// id, workspace slug, then the optional `created_at__gt` value.
pub fn history_activities_sql(
    binder: &mut Binder,
    slug: &str,
    issue_id: uuid::Uuid,
    user_id: uuid::Uuid,
    created_after: Option<&str>,
) -> String {
    let related = history_related_selects();
    let joins = history_joins("issue_activities");
    let gate = history_where(binder, "issue_activities", slug, issue_id, user_id);
    let after = created_after_predicate(binder, "issue_activities", created_after);
    format!(
        "SELECT {ACTIVITY_COLUMNS}, {related} {joins} \
        WHERE {gate} \
        AND NOT (issue_activities.field IN ('comment', 'vote', 'reaction', 'draft') \
        AND issue_activities.field IS NOT NULL){after} \
        ORDER BY issue_activities.created_at ASC"
    )
}

/// Full history-comments statement (`activity.py:47-64`): same gate and
/// order as [`history_activities_sql`], no field exclusion. The
/// `comment_reactions` prefetch is a separate statement
/// ([`history_reactions_prefetch_sql`]). Same binds.
pub fn history_comments_sql(
    binder: &mut Binder,
    slug: &str,
    issue_id: uuid::Uuid,
    user_id: uuid::Uuid,
    created_after: Option<&str>,
) -> String {
    let related = history_related_selects();
    let joins = history_joins("issue_comments");
    let gate = history_where(binder, "issue_comments", slug, issue_id, user_id);
    let after = created_after_predicate(binder, "issue_comments", created_after);
    format!(
        "SELECT {COMMENT_COLUMNS}, {related} {joins} WHERE {gate}{after} \
        ORDER BY issue_comments.created_at ASC"
    )
}

/// History reaction prefetch (`Prefetch("comment_reactions",
/// CommentReaction.objects.select_related("actor"))`, `activity.py:58-63`).
/// The actor FK is non-null, so the users join is `INNER`. One bind per
/// parent id, in order; Django passes the page's comment ids.
pub fn history_reactions_prefetch_sql(binder: &mut Binder, comment_ids: &[uuid::Uuid]) -> String {
    let list = comment_ids
        .iter()
        .map(|id| binder.bind_uuid(*id))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "SELECT {COMMENT_REACTION_COLUMNS}, {USER_COLUMNS} \
        FROM comment_reactions \
        INNER JOIN users ON comment_reactions.actor_id = users.id \
        WHERE comment_reactions.deleted_at IS NULL AND comment_reactions.comment_id IN ({list}) \
        ORDER BY comment_reactions.created_at DESC"
    )
}

/// `issue-property` intake prefetch (`activity.py:67-73`):
/// `issue__issue_intake` deferred to the three source columns (`to_attr`
/// is fetch-shape only — no SQL). One bind per parent id, in order.
pub fn history_intake_prefetch_sql(binder: &mut Binder, issue_ids: &[uuid::Uuid]) -> String {
    let list = issue_ids
        .iter()
        .map(|id| binder.bind_uuid(*id))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "SELECT {INTAKE_SOURCE_SELECTS} FROM intake_issues \
        WHERE intake_issues.deleted_at IS NULL AND intake_issues.issue_id IN ({list}) \
        ORDER BY intake_issues.created_at DESC"
    )
}

// ---------------------------------------------------------------------------
// Comments (comment.py:43-69,186-200)
// ---------------------------------------------------------------------------

/// `is_member` `Exists` (`comment.py:58-67`): an active membership of the
/// caller in this project. Uncorrelated — every predicate is a literal.
/// Binds, in order: user id, project id, workspace slug.
pub fn comment_is_member_select(
    binder: &mut Binder,
    slug: &str,
    project_id: uuid::Uuid,
    user_id: uuid::Uuid,
) -> String {
    let user = binder.bind_uuid(user_id);
    let project = binder.bind_uuid(project_id);
    let slug_holder = binder.bind_string(slug.to_owned());
    format!(
        "EXISTS(SELECT 1 AS a FROM project_members pm \
        INNER JOIN workspaces pmw ON pm.workspace_id = pmw.id \
        WHERE pm.deleted_at IS NULL AND pm.is_active AND pm.member_id = {user} \
        AND pm.project_id = {project} AND pmw.slug = {slug_holder} LIMIT 1) AS is_member"
    )
}

/// Full comment-list statement (`comment.py:43-69`): `DISTINCT` (the member
/// join can fan out), model-ordered newest first. The annotation selects
/// between the base columns and the `select_related` columns — Django's
/// exact select order. `filter_queryset` splices the optional
/// `filterset_fields` (`issue__id`, `workspace__id`) into the `WHERE`;
/// handler-owned. Binds, in order: workspace slug, project id, issue id,
/// user id (outer `WHERE`), then the `is_member` binds (user id, project
/// id, slug).
pub fn comment_list_sql(
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
    let is_member = comment_is_member_select(binder, slug, project_id, user_id);
    format!(
        "SELECT DISTINCT {COMMENT_COLUMNS}, {is_member}, {PROJECT_COLUMNS}, {WORKSPACE_COLUMNS}, \
        {PREFETCH_ISSUE_COLUMNS} \
        FROM issue_comments \
        INNER JOIN workspaces ON issue_comments.workspace_id = workspaces.id \
        INNER JOIN projects ON issue_comments.project_id = projects.id \
        INNER JOIN issues ON issue_comments.issue_id = issues.id \
        INNER JOIN project_members ON projects.id = project_members.project_id \
        WHERE issue_comments.deleted_at IS NULL AND workspaces.slug = {slug_holder} \
        AND issue_comments.project_id = {project} AND issue_comments.issue_id = {issue} \
        AND projects.archived_at IS NULL AND project_members.is_active \
        AND project_members.member_id = {user} \
        ORDER BY issue_comments.created_at DESC"
    )
}

/// Full comment-reaction-list statement (`comment.py:186-200`): `DISTINCT`,
/// newest first, no `select_related`. Binds, in order: workspace slug,
/// project id, comment id, user id.
pub fn comment_reaction_list_sql(
    binder: &mut Binder,
    slug: &str,
    project_id: uuid::Uuid,
    comment_id: uuid::Uuid,
    user_id: uuid::Uuid,
) -> String {
    let slug_holder = binder.bind_string(slug.to_owned());
    let project = binder.bind_uuid(project_id);
    let comment = binder.bind_uuid(comment_id);
    let user = binder.bind_uuid(user_id);
    format!(
        "SELECT DISTINCT {COMMENT_REACTION_COLUMNS} \
        FROM comment_reactions \
        INNER JOIN workspaces ON comment_reactions.workspace_id = workspaces.id \
        INNER JOIN projects ON comment_reactions.project_id = projects.id \
        INNER JOIN project_members ON projects.id = project_members.project_id \
        WHERE comment_reactions.deleted_at IS NULL AND workspaces.slug = {slug_holder} \
        AND comment_reactions.project_id = {project} AND comment_reactions.comment_id = {comment} \
        AND projects.archived_at IS NULL AND project_members.is_active \
        AND project_members.member_id = {user} \
        ORDER BY comment_reactions.created_at DESC"
    )
}

// ---------------------------------------------------------------------------
// Versions (version.py:36-144)
// ---------------------------------------------------------------------------

/// The 10 `.values()` keys in RENDER order (`version.py:48-59`,
/// `:120-131`) — the wire key order handlers render. FK fields keep
/// their field names as keys (`workspace`, not `workspace_id`).
pub const VERSION_PAGE_KEYS: &[&str] = &[
    "id",
    "workspace",
    "project",
    "issue",
    "last_saved_at",
    "owned_by",
    "created_at",
    "updated_at",
    "created_by",
    "updated_by",
];

/// Version page select list for one version table: the
/// [`VERSION_PAGE_KEYS`] columns with FK attnames, in call order. Shared
/// by both version lists (same `required_fields`, both endpoints).
pub fn version_page_selects(table: &str) -> String {
    format!(
        "{table}.id, {table}.workspace_id, {table}.project_id, {table}.issue_id, \
        {table}.last_saved_at, {table}.owned_by_id, {table}.created_at, {table}.updated_at, \
        {table}.created_by_id, {table}.updated_by_id"
    )
}

/// Version scope predicates for one version table. Binds, in order: issue
/// id, project id, workspace slug.
fn version_scope_where(
    binder: &mut Binder,
    table: &str,
    slug: &str,
    project_id: uuid::Uuid,
    issue_id: uuid::Uuid,
) -> String {
    let issue = binder.bind_uuid(issue_id);
    let project = binder.bind_uuid(project_id);
    let slug_holder = binder.bind_string(slug.to_owned());
    format!(
        "{table}.deleted_at IS NULL AND {table}.issue_id = {issue} \
        AND {table}.project_id = {project} AND workspaces.slug = {slug_holder}"
    )
}

/// `LIMIT`/`OFFSET` tail for a [`V2Page`]: Django omits `OFFSET` when the
/// slice starts at zero (`[0:1000]` renders bare `LIMIT 1000`) and always
/// renders the length (`[20:30]` renders `LIMIT 10 OFFSET 20`). The
/// bounds are computed page math, never user input, so they inline.
fn page_limit_offset(page: &V2Page) -> String {
    let length = page.end - page.start;
    if page.start == 0 {
        format!("LIMIT {length}")
    } else {
        format!("LIMIT {length} OFFSET {}", page.start)
    }
}

/// Version-detail statement (`version.py:38-44`): full version row, model
/// ordering. The pk lookup is unique, so `LIMIT 1` matches Django's
/// `.get()` (which reads up to `LIMIT 21` to detect multiples —
/// unobservable on a unique pk). Binds, in order: issue id, version id,
/// project id, workspace slug.
pub fn version_detail_sql(
    binder: &mut Binder,
    slug: &str,
    project_id: uuid::Uuid,
    issue_id: uuid::Uuid,
    version_id: uuid::Uuid,
) -> String {
    let issue = binder.bind_uuid(issue_id);
    let version = binder.bind_uuid(version_id);
    let project = binder.bind_uuid(project_id);
    let slug_holder = binder.bind_string(slug.to_owned());
    format!(
        "SELECT {VERSION_COLUMNS} FROM issue_versions \
        INNER JOIN workspaces ON issue_versions.workspace_id = workspaces.id \
        WHERE issue_versions.deleted_at IS NULL AND issue_versions.issue_id = {issue} \
        AND issue_versions.id = {version} AND issue_versions.project_id = {project} \
        AND workspaces.slug = {slug_holder} \
        ORDER BY issue_versions.created_at DESC LIMIT 1"
    )
}

/// Version-list count (`paginate`'s `base_queryset.count()`): same
/// joins/`WHERE` as the list, no `ORDER BY` (aggregates clear ordering;
/// rendering replicated from `Query.get_count`). Binds: issue id, project
/// id, workspace slug.
pub fn version_count_sql(
    binder: &mut Binder,
    slug: &str,
    project_id: uuid::Uuid,
    issue_id: uuid::Uuid,
) -> String {
    let scope = version_scope_where(binder, "issue_versions", slug, project_id, issue_id);
    format!(
        "SELECT COUNT(*) AS \"__count\" FROM issue_versions \
        INNER JOIN workspaces ON issue_versions.workspace_id = workspaces.id \
        WHERE {scope}"
    )
}

/// Version-list page (`version.py:61-74`): the 10 `required_fields` over
/// the model's `-created_at` ordering (no explicit `order_by`), sliced
/// per [`V2Page`]. The handler computes the page with
/// `render::v2_page(cursor, total)` and renders the envelope; the
/// `user_timezone_converter` pass over `created_at`/`updated_at` is
/// render-shape, handler-owned. Binds: issue id, project id, slug.
pub fn version_list_sql(
    binder: &mut Binder,
    slug: &str,
    project_id: uuid::Uuid,
    issue_id: uuid::Uuid,
    page: &V2Page,
) -> String {
    let selects = version_page_selects("issue_versions");
    let scope = version_scope_where(binder, "issue_versions", slug, project_id, issue_id);
    let tail = page_limit_offset(page);
    format!(
        "SELECT {selects} FROM issue_versions \
        INNER JOIN workspaces ON issue_versions.workspace_id = workspaces.id \
        WHERE {scope} ORDER BY issue_versions.created_at DESC {tail}"
    )
}

/// Description-version-detail statement (`version.py:107-116`): full row.
/// `IssueDescriptionVersion` declares no `Meta.ordering` (unlike its
/// siblings), so there is no `ORDER BY` — ported as-is. Same binds as
/// [`version_detail_sql`].
pub fn description_detail_sql(
    binder: &mut Binder,
    slug: &str,
    project_id: uuid::Uuid,
    issue_id: uuid::Uuid,
    version_id: uuid::Uuid,
) -> String {
    let issue = binder.bind_uuid(issue_id);
    let version = binder.bind_uuid(version_id);
    let project = binder.bind_uuid(project_id);
    let slug_holder = binder.bind_string(slug.to_owned());
    format!(
        "SELECT {DESCRIPTION_VERSION_COLUMNS} FROM issue_description_versions \
        INNER JOIN workspaces ON issue_description_versions.workspace_id = workspaces.id \
        WHERE issue_description_versions.deleted_at IS NULL \
        AND issue_description_versions.issue_id = {issue} \
        AND issue_description_versions.id = {version} \
        AND issue_description_versions.project_id = {project} \
        AND workspaces.slug = {slug_holder} LIMIT 1"
    )
}

/// Description-version-list count: same shape as [`version_count_sql`].
/// Same binds.
pub fn description_count_sql(
    binder: &mut Binder,
    slug: &str,
    project_id: uuid::Uuid,
    issue_id: uuid::Uuid,
) -> String {
    let scope = version_scope_where(
        binder,
        "issue_description_versions",
        slug,
        project_id,
        issue_id,
    );
    format!(
        "SELECT COUNT(*) AS \"__count\" FROM issue_description_versions \
        INNER JOIN workspaces ON issue_description_versions.workspace_id = workspaces.id \
        WHERE {scope}"
    )
}

/// Description-version-list page (`version.py:133-144`): same 10 columns
/// as [`version_list_sql`], over the endpoint's explicit
/// `.order_by("-created_at")` (the model has no default ordering, so the
/// call is load-bearing). Same binds as [`version_list_sql`].
pub fn description_list_sql(
    binder: &mut Binder,
    slug: &str,
    project_id: uuid::Uuid,
    issue_id: uuid::Uuid,
    page: &V2Page,
) -> String {
    let selects = version_page_selects("issue_description_versions");
    let scope = version_scope_where(
        binder,
        "issue_description_versions",
        slug,
        project_id,
        issue_id,
    );
    let tail = page_limit_offset(page);
    format!(
        "SELECT {selects} FROM issue_description_versions \
        INNER JOIN workspaces ON issue_description_versions.workspace_id = workspaces.id \
        WHERE {scope} ORDER BY issue_description_versions.created_at DESC {tail}"
    )
}

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

/// The 15 lite wire keys in `serialize_lite_issue` order
/// (`base.py:1223-1240`) — the key order handlers render. `created_by` /
/// `updated_by` select the `*_id` columns; `is_synced` is the predicate's
/// answer over the `is_synced` annotation (bug 2).
pub const LITE_KEYS: &[&str] = &[
    "id",
    "sequence_id",
    "name",
    "description_html",
    "sort_order",
    "project_id",
    "created_at",
    "updated_at",
    "created_by",
    "updated_by",
    "is_draft",
    "is_epic",
    "is_intake",
    "is_synced",
    "archived_at",
];

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

#[cfg(test)]
mod tests {
    use super::super::render::v2_page;
    use super::*;

    const SLUG: &str = "ws-slug";
    const PROJECT: uuid::Uuid = uuid::Uuid::from_u128(0x11111111_1111_1111_1111_111111111111);
    const ISSUE: uuid::Uuid = uuid::Uuid::from_u128(0x33333333_3333_3333_3333_333333333333);
    const USER: uuid::Uuid = uuid::Uuid::from_u128(0x22222222_2222_2222_2222_222222222222);
    const COMMENT: uuid::Uuid = uuid::Uuid::from_u128(0x44444444_4444_4444_4444_444444444444);

    /// Collapse runs of whitespace so multi-line builders compare cleanly.
    fn flat(sql: &str) -> String {
        sql.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    // -- reuse pins ----------------------------------------------------------

    #[test]
    fn shared_fragments_are_queries_core_text() {
        // Referenced, not copied: the full statement embeds the shared
        // fragments byte-identically, so a fix in queries_core lands here
        // with no second edit.
        let mut binder = Binder::new();
        let sql = identifier_full_sql(&mut binder, SLUG, PROJECT, 42, USER);
        for fragment in [
            CYCLE_ID_SELECT,
            &link_count_select(true),
            &attachment_count_select(true),
            &sub_issues_count_select(true),
            LABEL_IDS_SELECT,
            ASSIGNEE_IDS_ACTIVE_SELECT,
            MODULE_IDS_SELECT,
            ISSUE_COLUMNS,
            STATE_COLUMNS,
            PROJECT_COLUMNS,
        ] {
            assert!(
                flat(&sql).contains(&flat(fragment)),
                "full statement drifted from shared fragment: {fragment}"
            );
        }
        // The actor/issue related selects ride the history paths instead
        // (the full statement has no actor select; its parent select is
        // `parent.`-qualified).
        let mut binder = Binder::new();
        let history = history_activities_sql(&mut binder, SLUG, ISSUE, USER, None);
        for fragment in [USER_COLUMNS, PREFETCH_ISSUE_COLUMNS, PROJECT_COLUMNS] {
            assert!(
                flat(&history).contains(&flat(fragment)),
                "history statement drifted from shared fragment: {fragment}"
            );
        }
    }

    #[test]
    fn shared_prefetches_and_guest_gate_are_reused() {
        // The identifier reaction/link prefetches and the guest-view gate
        // are queries_core builders — assert the reuse compiles to the
        // same text the retrieve path uses.
        let mut binder = Binder::new();
        let reactions = super::super::queries_core::reaction_prefetch_sql(&mut binder, &[ISSUE]);
        assert!(reactions.contains("issue_reactions.issue_id IN ($1)"));
        let mut binder = Binder::new();
        let links = super::super::queries_core::link_prefetch_sql(&mut binder, &[ISSUE]);
        assert!(links.contains("issue_links.issue_id IN ($1)"));
        let mut binder = Binder::new();
        let guest =
            super::super::queries_core::guest_view_exists_sql(&mut binder, SLUG, PROJECT, USER);
        assert!(guest.contains("project_members.role = 5"));
        assert_eq!(binder.values().len(), 3);
    }

    // -- history -------------------------------------------------------------

    #[test]
    fn history_activities_matches_django_shape() {
        let mut binder = Binder::new();
        let sql = history_activities_sql(&mut binder, SLUG, ISSUE, USER, None);
        // Select order: base columns, then related tables in model field
        // order (projects, workspaces, issues, users) — not the
        // select_related() call order.
        let base_pos = sql.find("issue_activities.created_at").expect("base");
        let proj_pos = sql.find("projects.created_at").expect("projects");
        let ws_pos = sql.find("workspaces.created_at").expect("workspaces");
        let issue_pos = sql.find("issues.created_at").expect("issues");
        let user_pos = sql.find("users.password").expect("users");
        assert!(
            base_pos < proj_pos && proj_pos < ws_pos && ws_pos < issue_pos && issue_pos < user_pos,
            "history select order wrong: {sql}"
        );
        // The negated IN keeps Django's null semantics verbatim.
        assert!(sql.contains(
            "NOT (issue_activities.field IN ('comment', 'vote', 'reaction', 'draft') \
            AND issue_activities.field IS NOT NULL)"
        ));
        // Joins: issues/projects/members/workspaces INNER, nullable actor LEFT.
        assert!(sql.contains("INNER JOIN issues ON issue_activities.issue_id = issues.id"));
        assert!(sql.contains("INNER JOIN projects ON issue_activities.project_id = projects.id"));
        assert!(
            sql.contains("INNER JOIN project_members ON projects.id = project_members.project_id")
        );
        assert!(
            sql.contains("INNER JOIN workspaces ON issue_activities.workspace_id = workspaces.id")
        );
        assert!(sql.contains("LEFT OUTER JOIN users ON issue_activities.actor_id = users.id"));
        // Gate: archived-project exclusion, active membership, slug. The
        // member join carries no deleted_at guard (bug 4).
        assert!(sql.contains("projects.archived_at IS NULL"));
        assert!(sql.contains("project_members.is_active"));
        assert!(!sql.contains("project_members.deleted_at"));
        assert!(sql.ends_with("ORDER BY issue_activities.created_at ASC"));
        assert!(!sql.contains("DISTINCT"));
        assert_eq!(binder.values().len(), 3);
    }

    #[test]
    fn history_created_after_appends_gt_bind() {
        let mut binder = Binder::new();
        let sql = history_activities_sql(
            &mut binder,
            SLUG,
            ISSUE,
            USER,
            Some("2026-01-02T03:04:05+00:00"),
        );
        assert!(sql.contains("AND issue_activities.created_at > $4"));
        assert_eq!(binder.values().len(), 4);

        let mut binder = Binder::new();
        let sql = history_comments_sql(&mut binder, SLUG, ISSUE, USER, None);
        assert!(!sql.contains("created_at >"));
        assert!(!sql.contains("issue_comments.field"));
        assert!(sql.ends_with("ORDER BY issue_comments.created_at ASC"));
        assert_eq!(binder.values().len(), 3);
    }

    #[test]
    fn history_prefetches_match_django_shapes() {
        let mut binder = Binder::new();
        let reactions = history_reactions_prefetch_sql(&mut binder, &[COMMENT]);
        // Non-null actor FK: INNER join; IN-list; model ordering.
        assert!(reactions.contains("INNER JOIN users ON comment_reactions.actor_id = users.id"));
        assert!(reactions.contains("comment_reactions.comment_id IN ($1)"));
        assert!(reactions.ends_with("ORDER BY comment_reactions.created_at DESC"));
        assert_eq!(binder.values().len(), 1);

        let mut binder = Binder::new();
        let intake = history_intake_prefetch_sql(&mut binder, &[ISSUE]);
        // .only() renders model field order (source before source_email).
        assert!(flat(&intake).contains(
            "SELECT intake_issues.id, intake_issues.source, intake_issues.source_email, \
            intake_issues.extra FROM"
        ));
        assert!(intake.contains("intake_issues.issue_id IN ($1)"));
        assert!(intake.ends_with("ORDER BY intake_issues.created_at DESC"));
        assert_eq!(binder.values().len(), 1);
    }

    // -- comments ------------------------------------------------------------

    #[test]
    fn comment_list_has_distinct_member_annotation_and_related() {
        let mut binder = Binder::new();
        let sql = comment_list_sql(&mut binder, SLUG, PROJECT, ISSUE, USER);
        assert!(sql.starts_with("SELECT DISTINCT"));
        // Annotation between base and select_related columns.
        let base_pos = sql.find("issue_comments.created_at").expect("base");
        let member_pos = sql.find("AS is_member").expect("is_member");
        let proj_pos = sql.find("projects.created_at").expect("projects");
        assert!(
            base_pos < member_pos && member_pos < proj_pos,
            "comment select order wrong: {sql}"
        );
        assert!(sql.contains("pm.member_id = $5"));
        assert!(sql.contains("pm.project_id = $6"));
        assert!(sql.contains("pmw.slug = $7"));
        assert!(sql.ends_with("ORDER BY issue_comments.created_at DESC"));
        assert_eq!(binder.values().len(), 7);
    }

    #[test]
    fn comment_reaction_list_is_distinct_newest_first() {
        let mut binder = Binder::new();
        let sql = comment_reaction_list_sql(&mut binder, SLUG, PROJECT, COMMENT, USER);
        assert!(sql.starts_with("SELECT DISTINCT"));
        assert!(sql.contains("comment_reactions.comment_id = $3"));
        assert!(sql.contains("project_members.member_id = $4"));
        assert!(!sql.contains("select_related"));
        assert!(sql.ends_with("ORDER BY comment_reactions.created_at DESC"));
        assert_eq!(binder.values().len(), 4);
    }

    // -- versions ------------------------------------------------------------

    #[test]
    fn version_page_keys_and_selects_render_in_call_order() {
        assert_eq!(
            VERSION_PAGE_KEYS,
            &[
                "id",
                "workspace",
                "project",
                "issue",
                "last_saved_at",
                "owned_by",
                "created_at",
                "updated_at",
                "created_by",
                "updated_by",
            ]
        );
        // FK keys select attnames; both tables share the shape.
        for table in ["issue_versions", "issue_description_versions"] {
            let selects = flat(&version_page_selects(table));
            assert_eq!(
                selects,
                format!(
                    "{table}.id, {table}.workspace_id, {table}.project_id, {table}.issue_id, \
                    {table}.last_saved_at, {table}.owned_by_id, {table}.created_at, \
                    {table}.updated_at, {table}.created_by_id, {table}.updated_by_id"
                )
            );
        }
    }

    #[test]
    fn version_detail_count_and_list_shapes() {
        let mut binder = Binder::new();
        let detail = version_detail_sql(&mut binder, SLUG, PROJECT, ISSUE, ISSUE);
        assert!(detail.contains("issue_versions.activity_id"));
        assert!(detail.contains("issue_versions.owned_by_id"));
        assert!(detail.ends_with("ORDER BY issue_versions.created_at DESC LIMIT 1"));
        assert_eq!(binder.values().len(), 4);

        let mut binder = Binder::new();
        let count = version_count_sql(&mut binder, SLUG, PROJECT, ISSUE);
        assert!(count.starts_with("SELECT COUNT(*) AS \"__count\""));
        assert!(!count.contains("ORDER BY"));
        assert_eq!(binder.values().len(), 3);

        // First page: bare LIMIT, no OFFSET. Later page: LIMIT + OFFSET.
        let mut binder = Binder::new();
        let first = v2_page(None, 2500).expect("page");
        let sql = version_list_sql(&mut binder, SLUG, PROJECT, ISSUE, &first);
        assert!(sql.ends_with("ORDER BY issue_versions.created_at DESC LIMIT 1000"));
        let mut binder = Binder::new();
        let later = v2_page(Some("10:2:0"), 100).expect("page");
        let sql = version_list_sql(&mut binder, SLUG, PROJECT, ISSUE, &later);
        assert!(sql.ends_with("ORDER BY issue_versions.created_at DESC LIMIT 10 OFFSET 20"));
        assert_eq!(binder.values().len(), 3);
    }

    #[test]
    fn description_version_ordering_differs_from_issue_version() {
        // No Meta.ordering: the detail has no ORDER BY at all...
        let mut binder = Binder::new();
        let detail = description_detail_sql(&mut binder, SLUG, PROJECT, ISSUE, ISSUE);
        assert!(!detail.contains("ORDER BY"));
        assert!(detail.ends_with("LIMIT 1"));

        // ...while the list orders explicitly (load-bearing call).
        let mut binder = Binder::new();
        let page = v2_page(None, 5).expect("page");
        let sql = description_list_sql(&mut binder, SLUG, PROJECT, ISSUE, &page);
        assert!(sql.ends_with("ORDER BY issue_description_versions.created_at DESC LIMIT 5"));

        let mut binder = Binder::new();
        let count = description_count_sql(&mut binder, SLUG, PROJECT, ISSUE);
        assert!(count.starts_with("SELECT COUNT(*) AS \"__count\""));
        assert!(!count.contains("ORDER BY"));
    }

    // -- meta ----------------------------------------------------------------

    #[test]
    fn meta_selects_both_values_over_manager_scope() {
        let mut binder = Binder::new();
        let sql = meta_sql(&mut binder, SLUG, PROJECT, ISSUE);
        // Single-statement consolidation of Django's two queries.
        assert!(flat(&sql).starts_with("SELECT issues.sequence_id, projects.identifier FROM"));
        // issue_objects scope: triage exclusion, both archived guards, drafts.
        assert!(sql.contains("NOT (states.\"group\" = 'triage' AND states.\"group\" IS NOT NULL)"));
        assert!(sql.contains("issues.archived_at IS NULL"));
        assert!(sql.contains("projects.archived_at IS NULL"));
        assert!(sql.contains("issues.is_draft = FALSE"));
        assert!(sql.ends_with("ORDER BY issues.created_at DESC LIMIT 1"));
        assert_eq!(binder.values().len(), 3);
    }

    // -- identifier ----------------------------------------------------------

    #[test]
    fn identifier_project_uses_iexact_and_limit_1() {
        let mut binder = Binder::new();
        let sql = identifier_project_sql(&mut binder, SLUG, "ENG");
        assert!(sql.contains("UPPER(projects.identifier::text) = UPPER($1)"));
        assert!(sql.contains("workspaces.slug = $2"));
        assert!(sql.ends_with("ORDER BY projects.created_at DESC LIMIT 1"));
        assert_eq!(binder.values().len(), 2);
    }

    #[test]
    fn identifier_member_gate_has_no_role_filter() {
        let mut binder = Binder::new();
        let sql = identifier_member_exists_sql(&mut binder, SLUG, PROJECT, USER);
        assert!(sql.starts_with("SELECT 1 AS a"));
        assert!(!sql.contains("role"));
        assert!(!sql.contains("ORDER BY"));
        assert!(sql.ends_with("LIMIT 1"));
        assert_eq!(binder.values().len(), 3);
    }

    #[test]
    fn identifier_lite_selects_deferred_row_plus_three_flags() {
        let mut binder = Binder::new();
        let sql = identifier_lite_sql(&mut binder, SLUG, PROJECT, 42);
        // Select order: deferred columns, then annotations in annotate() order.
        let base_pos = sql.find("issue.created_at").expect("base");
        let epic_pos = sql.find("AS is_epic").expect("epic");
        let intake_pos = sql.find("AS is_intake").expect("intake");
        let synced_pos = sql.find("AS is_synced").expect("synced");
        assert!(
            base_pos < epic_pos && epic_pos < intake_pos && intake_pos < synced_pos,
            "lite select order wrong: {sql}"
        );
        // Deferred shape: 14 columns incl. external_source (predicate input).
        assert!(sql.contains("issue.external_source, COALESCE"));
        assert!(!sql.contains("issue.priority"));
        // is_epic over the type left join; is_intake pending/snoozed only.
        assert!(sql.contains("COALESCE(issue_types.is_epic, FALSE) AS is_epic"));
        assert!(sql.contains("LEFT OUTER JOIN issue_types ON issue.type_id = issue_types.id"));
        assert!(sql.contains("ii.status IN (-2, 0)"));
        // Bug 2: git-only, unscoped — no Github table, no project predicate.
        assert!(sql.contains("FROM git_issue_syncs gs"));
        assert!(!flat(&sql).contains("github"));
        let synced_from = sql.find("FROM git_issue_syncs").expect("sync from");
        let synced_end = sql.find("AS is_synced").expect("sync end");
        assert!(!sql[synced_from..synced_end].contains("project_id"));
        // Plain manager: no triage/archived/draft guards on the outer query.
        let outer = sql.split("FROM issues AS issue").nth(1).expect("outer");
        assert!(!outer.contains("triage"));
        assert!(!outer.contains("archived_at"));
        assert!(!outer.contains("is_draft"));
        assert!(sql.ends_with("ORDER BY issue.created_at DESC LIMIT 1"));
        assert_eq!(binder.values().len(), 5);
    }

    #[test]
    fn lite_keys_follow_serializer_order() {
        assert_eq!(
            LITE_KEYS,
            &[
                "id",
                "sequence_id",
                "name",
                "description_html",
                "sort_order",
                "project_id",
                "created_at",
                "updated_at",
                "created_by",
                "updated_by",
                "is_draft",
                "is_epic",
                "is_intake",
                "is_synced",
                "archived_at",
            ]
        );
    }

    /// Dump every builder plus the column consts for the `/tmp/replay-649`
    /// fixture replay (live Django 4.2.30 SQL vs these builders). Prints
    /// `@@ name @@` / SQL / `@@ binds: N @@` per statement (queries-A
    /// precedent); the replay script, not this test, compares.
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
        dump!("history_activities", |binder: &mut Binder| {
            history_activities_sql(binder, SLUG, ISSUE, USER, None)
        });
        dump!("history_activities_gt", |binder: &mut Binder| {
            history_activities_sql(binder, SLUG, ISSUE, USER, Some("2026-01-02T03:04:05+00:00"))
        });
        dump!("history_comments", |binder: &mut Binder| {
            history_comments_sql(binder, SLUG, ISSUE, USER, None)
        });
        dump!("history_prefetch_reactions", |binder: &mut Binder| {
            history_reactions_prefetch_sql(binder, &[COMMENT])
        });
        dump!("history_prefetch_intake", |binder: &mut Binder| {
            history_intake_prefetch_sql(binder, &[ISSUE])
        });
        dump!("comment_list", |binder: &mut Binder| {
            comment_list_sql(binder, SLUG, PROJECT, ISSUE, USER)
        });
        dump!("comment_reaction_list", |binder: &mut Binder| {
            comment_reaction_list_sql(binder, SLUG, PROJECT, COMMENT, USER)
        });
        dump!("version_detail", |binder: &mut Binder| {
            version_detail_sql(binder, SLUG, PROJECT, ISSUE, ISSUE)
        });
        dump!("version_count", |binder: &mut Binder| {
            version_count_sql(binder, SLUG, PROJECT, ISSUE)
        });
        dump!("version_list_page", |binder: &mut Binder| {
            version_list_sql(
                binder,
                SLUG,
                PROJECT,
                ISSUE,
                &v2_page(None, 2500).expect("page"),
            )
        });
        dump!("description_detail", |binder: &mut Binder| {
            description_detail_sql(binder, SLUG, PROJECT, ISSUE, ISSUE)
        });
        dump!("description_count", |binder: &mut Binder| {
            description_count_sql(binder, SLUG, PROJECT, ISSUE)
        });
        dump!("description_list_page", |binder: &mut Binder| {
            description_list_sql(
                binder,
                SLUG,
                PROJECT,
                ISSUE,
                &v2_page(Some("10:2:0"), 100).expect("page"),
            )
        });
        dump!("meta", |binder: &mut Binder| {
            meta_sql(binder, SLUG, PROJECT, ISSUE)
        });
        dump!("identifier_project", |binder: &mut Binder| {
            identifier_project_sql(binder, SLUG, "ENG")
        });
        dump!("identifier_member_exists", |binder: &mut Binder| {
            identifier_member_exists_sql(binder, SLUG, PROJECT, USER)
        });
        dump!("identifier_lite", |binder: &mut Binder| {
            identifier_lite_sql(binder, SLUG, PROJECT, 42)
        });
        dump!("identifier_full", |binder: &mut Binder| {
            identifier_full_sql(binder, SLUG, PROJECT, 42, USER)
        });
        for (name, sql, binds) in &statements {
            println!("@@ {name} @@\n{sql}\n@@ binds: {binds} @@");
        }
        for (name, cols) in [
            ("cols_activity", ACTIVITY_COLUMNS),
            ("cols_comment", COMMENT_COLUMNS),
            ("cols_comment_reaction", COMMENT_REACTION_COLUMNS),
            ("cols_version", VERSION_COLUMNS),
            ("cols_description_version", DESCRIPTION_VERSION_COLUMNS),
            ("cols_workspace", WORKSPACE_COLUMNS),
            ("cols_ticker", TICKER_COLUMNS),
            ("cols_parent_issue", PARENT_ISSUE_COLUMNS),
            ("cols_lite_issue", LITE_ISSUE_COLUMNS),
            ("cols_intake_source", INTAKE_SOURCE_SELECTS),
        ] {
            println!("@@ {name} @@\n{cols}");
        }
    }

    #[test]
    fn identifier_full_selects_row_annotations_and_related_in_order() {
        let mut binder = Binder::new();
        let sql = identifier_full_sql(&mut binder, SLUG, PROJECT, 42, USER);
        let issue_pos = sql.find("issue.created_at").expect("issue");
        let cycle_pos = sql.find("AS cycle_id").expect("cycle");
        let link_pos = sql.find("AS link_count").expect("link");
        let attach_pos = sql.find("AS attachment_count").expect("attach");
        let sub_pos = sql.find("AS sub_issues_count").expect("sub");
        let label_pos = sql.find("AS label_ids").expect("labels");
        let assignee_pos = sql.find("AS assignee_ids").expect("assignees");
        let module_pos = sql.find("AS module_ids").expect("modules");
        let subscribed_pos = sql.find("AS is_subscribed").expect("subscribed");
        let intake_pos = sql.find("AS is_intake").expect("intake");
        let proj_pos = sql.find("projects.created_at").expect("projects");
        let ws_pos = sql.find("workspaces.created_at").expect("workspaces");
        let parent_pos = sql.find("parent.created_at").expect("parent");
        let state_pos = sql.find("state.created_at").expect("state");
        let ticker_pos = sql.find("issue_agent_ticker.created_at").expect("ticker");
        assert!(
            issue_pos < cycle_pos
                && cycle_pos < link_pos
                && link_pos < attach_pos
                && attach_pos < sub_pos
                && sub_pos < label_pos
                && label_pos < assignee_pos
                && assignee_pos < module_pos
                && module_pos < subscribed_pos
                && subscribed_pos < intake_pos
                && intake_pos < proj_pos
                && proj_pos < ws_pos
                && ws_pos < parent_pos
                && parent_pos < state_pos
                && state_pos < ticker_pos,
            "full select order wrong: {sql}"
        );
        // Counts coalesce to 0 on this path (unlike retrieve's NULL).
        assert!(sql.contains("COALESCE((SELECT NULLIF(COUNT(*), 0)"));
        // Joins: projects/workspaces INNER, parent/state/ticker LEFT.
        assert!(sql.contains("INNER JOIN projects ON issue.project_id = projects.id"));
        assert!(sql.contains("INNER JOIN workspaces ON issue.workspace_id = workspaces.id"));
        assert!(sql.contains("LEFT OUTER JOIN issues AS parent ON issue.parent_id = parent.id"));
        assert!(sql.contains("LEFT OUTER JOIN states AS state ON issue.state_id = state.id"));
        assert!(sql.contains(
            "LEFT OUTER JOIN issue_agent_ticker ON issue.id = issue_agent_ticker.issue_id"
        ));
        assert!(sql.ends_with("ORDER BY issue.created_at DESC LIMIT 1"));
        assert_eq!(binder.values().len(), 8);
    }
}
