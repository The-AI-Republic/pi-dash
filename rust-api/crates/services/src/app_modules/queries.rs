#![forbid(unsafe_code)]

//! Module queryset builders (D-28, stage 5).
//!
//! Ports the annotated querysets behind the module endpoints to SQL text,
//! following the D-27 precedent (`app_cycles/queries.rs`): each builder
//! returns a fragment the caller splices into the statement it executes.
//! The services crate carries no `sea-query` dependency (foundation crates
//! are read-only for port agents), so placeholders stay symbolic — `:slug`,
//! `:project_id`, `:user`, `:module_id` — exactly the notation the
//! fixtures use; handlers bind them.
//!
//! Sources (drift baseline `01a93e17`):
//! - `app/views/module/base.py:78-292` — `ModuleViewSet.get_queryset`
//!   (favorite `Exists` :79-85, five group `Count`s + total :86-144, six
//!   estimate `Sum`s :145-210, `member_ids` `ArrayAgg` :278-290,
//!   `-is_favorite,-created_at` ordering :291) + `list()` :353-393 +
//!   `retrieve()` :395-649 + `partial_update` values :673-705.
//! - `app/views/module/base.py:774-788` — `ModuleLinkViewSet.get_queryset`.
//! - `app/views/module/base.py:795-802` — `ModuleFavoriteViewSet.get_queryset`
//!   (raises `FieldError` on `select_related("module")` — ported bug 1).
//! - `app/views/module/archive.py:45-256` — archived `get_queryset`
//!   (`archived_at IS NOT NULL` :181, `select_related` lead :183,
//!   unguarded `member_ids` :246-254 — ported bug 2) + archived
//!   `get()` list/retrieve :258-565.
//! - `app/views/module/issue.py:53-94` — `ModuleIssueViewSet.get_queryset`
//!   + `apply_annotations` (`cycle_id` / `link_count` /
//!     `attachment_count` / `sub_issues_count` :53-82).
//! - `db/models/issue.py:95-104` — `IssueManager` base (excludes triage,
//!   archived issues, archived projects, drafts inside every issue
//!   subquery).
//! - `utils/timezone_converter.py:17-41` — `user_timezone_converter`
//!   (pytz shift of `created_at`/`updated_at` on read).
//! - `utils/analytics_plot.py:123-...` — `burndown_plot` behind the
//!   `completion_chart` view props on retrieve.
//!
//! Fixture oracle: FX-MOD-03 (`fixtures/app_modules/queries/`
//! `module_querysets.sql` Q1-Q6 + `module_querysets.rows.json`). The unit
//! tests below pin the builders against those files so transcription drift
//! fails the build.
//!
//! Existing bugs ported as-is (translation, don't redesign):
//! 1. `ModuleFavoriteViewSet.get_queryset` calls
//!    `select_related("module")` on `UserFavorite`, which has no `module`
//!    FK — Django raises `FieldError` before rendering, so the endpoint
//!    500s on two independent defects (bad `select_related` AND no
//!    serializer could render a `module` attr). Ported as
//!    [`FAVORITE_SELECT_RELATED_ERROR`]; [`favorite_scope_where`] is the
//!    scope the queryset *intended*.
//! 2. Archive `member_ids` (`archive.py:246-254`) filters only
//!    `~Q(members__id__isnull=True)` and omits the
//!    `modulemember__deleted_at__isnull=True` clause Q1 has
//!    (`base.py:283-286`) — soft-deleted memberships aggregate in the
//!    archive path. Ported via `member_deleted_guard: false`; both shapes
//!    are kept, never unified.
//! 3. Q6 `cycle_id` carries a doubled `deleted_at IS NULL` (manager +
//!    explicit `deleted_at__isnull=True`, `issue.py:57`) and inherits the
//!    bridge model's default `ORDER BY created_at DESC` before `LIMIT 1`.
//!    Both ported as observed.
//! 4. Estimate inner aliases use the singular for backlog / unstarted /
//!    started / cancelled (`backlog_estimate_point`, `unstarted_estimate_point`,
//!    `started_estimate_point`, `cancelled_estimate_point`) while the outer
//!    annotation aliases are plural (`..._points`); `completed` and `total`
//!    are plural on both sides (`completed_estimate_points`,
//!    `total_estimate_points`, Q1 SQL). Ported as observed.
//! 5. `retrieve()` evaluates the queryset twice (`queryset.first()` at
//!    `:424` and `:425`) and `partial_update` re-reads via `.values()`
//!    after `save()` — ported as handler sequencing notes, not deduped.
//!
//! Out of scope (owned by sibling handler/guard/task issues): the
//! envelopes, `create`/`update`/`destroy`, permission gates, Celery
//! enqueues, `burndown_plot` internals, `ModuleDetailSerializer` shape.
//! Their queryset-adjacent constants that ARE in scope here are marked.

// ---------------------------------------------------------------------------
// Shared scope
// ---------------------------------------------------------------------------

/// `IssueManager` base guards (`db/models/issue.py:95-104`), present inside
/// every issue subquery in Q1/Q4/Q5/Q6: exclude triage states, archived
/// issues, archived projects, drafts. Soft-delete (`deleted_at IS NULL`)
/// comes from `SoftDeletionManager` underneath.
pub fn issue_manager_guards_sql() -> String {
    [
        "issues.deleted_at IS NULL",
        "NOT (states.group = 'triage' AND states.group IS NOT NULL)",
        "NOT (issues.archived_at IS NOT NULL)",
        "NOT (projects.archived_at IS NOT NULL)",
        "NOT (issues.is_draft)",
    ]
    .join(" AND ")
}

/// Module tenant scope shared by Q1 and Q4 (`base.py:214-215`,
/// `archive.py:179-180`): this project, this workspace slug. Soft-delete
/// scoping (`deleted_at IS NULL`) comes from the manager underneath.
pub fn module_scope_where() -> String {
    "modules.project_id = :project_id AND workspaces.slug = :slug".to_owned()
}

/// Archived predicate: the list/retrieve path filters `archived_at IS NULL`
/// (`base.py:355,399`); the archive path filters `archived_at IS NOT NULL`
/// (`archive.py:181`).
pub fn archived_predicate(archived_only: bool) -> &'static str {
    if archived_only {
        "modules.archived_at IS NOT NULL"
    } else {
        "modules.archived_at IS NULL"
    }
}

/// `is_favorite = EXISTS(...)` over `user_favorites` (`base.py:79-85`,
/// `archive.py:46-52`): caller, `entity_type='module'`, bridge to this
/// module, project, workspace slug. Q1 SQL renders the soft-delete guard
/// (`user_favorites.deleted_at IS NULL`) from the manager.
pub fn favorite_exists_sql() -> String {
    "EXISTS (SELECT 1 FROM user_favorites uf JOIN workspaces w ON (uf.workspace_id = w.id) WHERE uf.deleted_at IS NULL AND uf.entity_identifier = modules.id AND uf.entity_type = 'module' AND uf.project_id = :project_id AND uf.user_id = :user AND w.slug = :slug LIMIT 1)".to_owned()
}

// ---------------------------------------------------------------------------
// Q1/Q4 count + estimate annotations
// ---------------------------------------------------------------------------

/// State-group restriction of a count/estimate annotation.
/// `All` is the total (no group filter).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupFilter {
    /// No group restriction (`total_issues`, total estimate subquery).
    All,
    /// `state_group = '<group>'`.
    Eq(&'static str),
}

impl GroupFilter {
    fn group(&self) -> Option<&'static str> {
        match self {
            GroupFilter::All => None,
            GroupFilter::Eq(group) => Some(group),
        }
    }
}

/// Per-group issue count subquery (`base.py:86-144`, `archive.py:53-111`):
/// correlated over the module, restricted to live bridges
/// (`module_issues.deleted_at IS NULL`), wrapped by handlers as
/// `COALESCE((<this> LIMIT 1), 0) AS <group>_issues`.
///
/// The total (`GroupFilter::All`) joins `states` with LEFT OUTER JOIN
/// (Q1 SQL) while every grouped count uses INNER JOIN — ported as
/// observed via `states_join`.
pub fn count_issues_sql(filter: GroupFilter) -> String {
    let group_predicate = match filter.group() {
        Some(group) => format!(" AND states.group = '{group}'"),
        None => String::new(),
    };
    // Ported Q1 shape: grouped counts INNER JOIN states; the total LEFT
    // OUTER JOINs it (its `NOT (group = triage ...)` guard is NULL-safe).
    let states_join = match filter.group() {
        Some(_) => "JOIN states ON (issues.state_id = states.id)",
        None => "LEFT OUTER JOIN states ON (issues.state_id = states.id)",
    };
    format!(
        "SELECT COUNT(issues.id) AS cnt FROM issues {states_join} JOIN projects ON (issues.project_id = projects.id) JOIN module_issues ON (issues.id = module_issues.issue_id) WHERE {} AND module_issues.deleted_at IS NULL AND module_issues.module_id = modules.id{group_predicate} GROUP BY module_issues.module_id",
        issue_manager_guards_sql()
    )
}

/// The six count annotation aliases in source order
/// (`base.py:224-244`, `archive.py:191-211`).
pub const COUNT_ALIASES: &[&str] = &[
    "completed_issues",
    "cancelled_issues",
    "started_issues",
    "unstarted_issues",
    "backlog_issues",
    "total_issues",
];

/// The five state groups feeding the count/estimate subqueries, in source
/// order (`base.py:86-135`); `None` is the total (no group filter).
pub const COUNT_GROUPS: &[Option<&str>] = &[
    Some("cancelled"),
    Some("completed"),
    Some("started"),
    Some("unstarted"),
    Some("backlog"),
    None,
];

/// `COALESCE((<subquery> LIMIT 1), 0) AS <alias>` wrapper
/// (`base.py:224-244`, `Value(0, IntegerField())` default).
pub fn count_coalesce_sql(filter: GroupFilter, alias: &str) -> String {
    format!(
        "COALESCE(({} LIMIT 1), 0) AS {alias}",
        count_issues_sql(filter)
    )
}

/// Per-group estimate subquery (`base.py:145-210`, `archive.py:112-177`):
/// correlated over the module, restricted to `estimates.type = 'points'`,
/// `SUM` of `CAST(estimate_point.value AS double precision)`.
/// `group = None` is the total (no group filter).
/// Handlers wrap it: `COALESCE((<this> LIMIT 1), 0.0) AS <alias>`.
pub fn estimate_subquery_sql(group: Option<&str>) -> String {
    let group_predicate = match group {
        Some(group) => format!(" AND states.group = '{group}'"),
        None => String::new(),
    };
    format!(
        "SELECT SUM(CAST(estimate_points.value AS double precision)) FROM issues JOIN states ON (issues.state_id = states.id) JOIN projects ON (issues.project_id = projects.id) JOIN estimate_points ON (issues.estimate_point_id = estimate_points.id) JOIN estimates ON (estimate_points.estimate_id = estimates.id) JOIN module_issues ON (issues.id = module_issues.issue_id) WHERE {} AND estimates.type = 'points' AND module_issues.deleted_at IS NULL AND module_issues.module_id = modules.id{group_predicate} GROUP BY module_issues.module_id",
        issue_manager_guards_sql()
    )
}

/// The six estimate annotation aliases in source order
/// (`base.py:245-277`, `archive.py:212-244`).
pub const ESTIMATE_POINT_ALIASES: &[&str] = &[
    "backlog_estimate_points",
    "unstarted_estimate_points",
    "started_estimate_points",
    "cancelled_estimate_points",
    "completed_estimate_points",
    "total_estimate_points",
];

/// `COALESCE((<subquery> LIMIT 1), 0.0) AS <alias>` wrapper
/// (`base.py:245-277`, `Value(0, FloatField())` default).
pub fn estimate_coalesce_sql(group: Option<&str>, alias: &str) -> String {
    format!(
        "COALESCE(({} LIMIT 1), 0.0) AS {alias}",
        estimate_subquery_sql(group)
    )
}

/// The six estimate groups in annotation order (`base.py:167-210`);
/// `None` is the total (no group filter).
pub const ESTIMATE_GROUPS: &[Option<&str>] = &[
    Some("backlog"),
    Some("unstarted"),
    Some("started"),
    Some("cancelled"),
    Some("completed"),
    None,
];

/// `member_ids = COALESCE(ARRAY_AGG(DISTINCT ...), '{}')`
/// (`base.py:278-290`, `archive.py:245-254`). Q1/Q4 SQL aggregate
/// `module_members.member_id` (the M2M through table, left-joined to
/// `modules`); the names below are the real columns, not ORM paths.
///
/// `member_deleted_guard = true` adds `module_members.deleted_at IS NULL`
/// (Q1); `false` omits it (Q4 archive path — ported bug 2, the asymmetry
/// is intentional and must survive refactors).
pub fn member_ids_sql(member_deleted_guard: bool) -> String {
    let bridge = if member_deleted_guard {
        " AND module_members.deleted_at IS NULL"
    } else {
        ""
    };
    format!(
        "COALESCE(ARRAY_AGG(DISTINCT module_members.member_id) FILTER (WHERE module_members.member_id IS NOT NULL{bridge}), '{{}}')"
    )
}

/// `get_queryset` tail order, shared by Q1 and Q4
/// (`base.py:291`, `archive.py:255`): `-is_favorite`, `-created_at`.
pub const MODULE_ORDER_SQL: &str = "is_favorite DESC, modules.created_at DESC";

/// `list()` `.values()` keys (`base.py:359-390`).
pub const MODULE_LIST_VALUES_FIELDS: &[&str] = &[
    "id",
    "workspace_id",
    "project_id",
    "name",
    "description",
    "description_text",
    "description_html",
    "start_date",
    "target_date",
    "status",
    "lead_id",
    "member_ids",
    "view_props",
    "sort_order",
    "external_source",
    "external_id",
    "logo_props",
    "completed_estimate_points",
    "total_estimate_points",
    "total_issues",
    "is_favorite",
    "cancelled_issues",
    "completed_issues",
    "started_issues",
    "unstarted_issues",
    "backlog_issues",
    "created_at",
    "updated_at",
];

/// Archived `get()` list `.values()` keys (`archive.py:261-290`): the Q1
/// list shape minus `logo_props` / `completed_estimate_points` /
/// `total_estimate_points`, plus `archived_at`.
pub const ARCHIVE_LIST_VALUES_FIELDS: &[&str] = &[
    "id",
    "workspace_id",
    "project_id",
    "name",
    "description",
    "description_text",
    "description_html",
    "start_date",
    "target_date",
    "status",
    "lead_id",
    "member_ids",
    "view_props",
    "sort_order",
    "external_source",
    "external_id",
    "total_issues",
    "is_favorite",
    "cancelled_issues",
    "completed_issues",
    "started_issues",
    "unstarted_issues",
    "backlog_issues",
    "created_at",
    "updated_at",
    "archived_at",
];

/// `partial_update` re-read `.values()` keys (`base.py:673-705`): the list
/// shape in a different order (same set — ported as observed).
pub const MODULE_PARTIAL_UPDATE_VALUES_FIELDS: &[&str] = &[
    "id",
    "workspace_id",
    "project_id",
    "name",
    "description",
    "description_text",
    "description_html",
    "start_date",
    "target_date",
    "status",
    "lead_id",
    "member_ids",
    "view_props",
    "sort_order",
    "external_source",
    "external_id",
    "logo_props",
    "completed_estimate_points",
    "total_estimate_points",
    "is_favorite",
    "cancelled_issues",
    "completed_issues",
    "started_issues",
    "total_issues",
    "unstarted_issues",
    "backlog_issues",
    "created_at",
    "updated_at",
];

/// Retrieve 404 body (`base.py:414-415`).
pub const MODULE_NOT_FOUND_BODY: &str = "{\"error\":\"Module not found\"}";

/// Archived-update refusal (`base.py:663-667`, HTTP 400).
pub const ARCHIVED_MODULE_UPDATE_BODY: &str = "{\"error\":\"Archived module cannot be updated\"}";

// ---------------------------------------------------------------------------
// Retrieve extras: sub_issues, distributions, burndown, timezone
// ---------------------------------------------------------------------------

/// `sub_issues` retrieve annotation (`base.py:401-411`,
/// `archive.py:298-308`): `COUNT` over live module bridges of child
/// issues (`parent IS NOT NULL`). The source queryset runs through
/// `Issue.issue_objects`, so the `IssueManager` base guards apply inside
/// this subquery too (triage / archived / draft exclusion); base clears
/// ordering (`.order_by()`), archive keeps the bridge default — both
/// render the same `COUNT` subquery; handlers keep the call-site spelling.
pub fn sub_issues_sql() -> String {
    format!(
        "SELECT COUNT(issues.id) FROM issues LEFT OUTER JOIN states ON (issues.state_id = states.id) JOIN projects ON (issues.project_id = projects.id) JOIN module_issues ON (issues.id = module_issues.issue_id) WHERE {} AND issues.project_id = :project_id AND issues.parent_id IS NOT NULL AND module_issues.module_id = :module_id AND module_issues.deleted_at IS NULL",
        issue_manager_guards_sql()
    )
}

/// `estimate_type` gate (`base.py:417-422`, `archive.py:311-316`):
/// an `estimates.type = 'points'` estimate exists for the project
/// (`Project.objects.filter(workspace__slug, pk, estimate__isnull=False,
/// estimate__type="points")`). The `workspace__slug` and `estimate__type`
/// lookups are joins — `projects` carries `workspace_id` / `estimate_id`,
/// not the slug or the type — so the fragment joins both tables; the
/// INNER JOIN on `estimates` also enforces `estimate__isnull=False`.
/// Guards the `estimate_distribution` block and the points
/// `completion_chart`.
pub fn estimate_type_exists_sql() -> String {
    "SELECT 1 FROM projects JOIN workspaces ON (projects.workspace_id = workspaces.id) JOIN estimates ON (projects.estimate_id = estimates.id) WHERE workspaces.slug = :slug AND projects.id = :project_id AND estimates.type = 'points'".to_owned()
}

/// `avatar_url` `Case` (`base.py:442-460`, repeated `:549-565` and the
/// archive twins): the asset URL when `avatar_asset` is set, else the raw
/// `avatar` field, else NULL.
pub fn avatar_url_case_sql() -> String {
    "CASE WHEN assignees.avatar_asset IS NOT NULL THEN CONCAT('/api/assets/v2/static/', assignees.avatar_asset, '/') WHEN assignees.avatar_asset IS NULL THEN assignees.avatar ELSE NULL END".to_owned()
}

/// Estimate-distribution sums (`base.py:468-488`, `:503-524`): total plus
/// completed (`completed_at IS NOT NULL`) / pending
/// (`completed_at IS NULL`) splits, each over live non-draft issues.
pub fn estimate_sum_sql(completed: Option<bool>) -> String {
    let split = match completed {
        Some(true) => " AND issues.completed_at IS NOT NULL",
        Some(false) => " AND issues.completed_at IS NULL",
        None => "",
    };
    format!(
        "SUM(CAST(estimate_points.value AS double precision)) FILTER (WHERE issues.archived_at IS NULL AND issues.is_draft = FALSE{split})"
    )
}

/// Issue-count distribution (`base.py:567-587`, `:602-623`): same splits
/// as [`estimate_sum_sql`] but `COUNT(id)`.
pub fn distribution_count_sql(completed: Option<bool>) -> String {
    let split = match completed {
        Some(true) => " AND issues.completed_at IS NOT NULL",
        Some(false) => " AND issues.completed_at IS NULL",
        None => "",
    };
    format!("COUNT(issues.id) FILTER (WHERE issues.archived_at IS NULL AND issues.is_draft = FALSE{split})")
}

/// Estimate-flavoured assignee row keys (`base.py:461-467`).
pub const ASSIGNEE_ESTIMATE_ROW_KEYS: &[&str] = &[
    "first_name",
    "last_name",
    "assignee_id",
    "avatar_url",
    "display_name",
    "total_estimates",
    "completed_estimates",
    "pending_estimates",
];

/// Estimate-flavoured label row keys (`base.py:498-503`).
pub const LABEL_ESTIMATE_ROW_KEYS: &[&str] = &[
    "label_name",
    "color",
    "label_id",
    "total_estimates",
    "completed_estimates",
    "pending_estimates",
];

/// Count-flavoured assignee row keys (`base.py:566`).
pub const ASSIGNEE_COUNT_ROW_KEYS: &[&str] = &[
    "first_name",
    "last_name",
    "assignee_id",
    "avatar_url",
    "display_name",
    "total_issues",
    "completed_issues",
    "pending_issues",
];

/// Count-flavoured label row keys (`base.py:598-602`).
pub const LABEL_COUNT_ROW_KEYS: &[&str] = &[
    "label_name",
    "color",
    "label_id",
    "total_issues",
    "completed_issues",
    "pending_issues",
];

/// Points `completion_chart` trigger (`base.py:529-536`): rendered when
/// the module has both dates; the plot itself (`burndown_plot`,
/// `plot_type="points"`) is handler-owned.
pub fn points_chart_trigger_sql() -> String {
    "modules.start_date IS NOT NULL AND modules.target_date IS NOT NULL".to_owned()
}

/// Issues `completion_chart` trigger (`base.py:632-639`): both dates AND
/// at least one issue; the plot (`plot_type="issues"`) is handler-owned.
pub fn issues_chart_trigger_sql() -> String {
    "modules.start_date IS NOT NULL AND modules.target_date IS NOT NULL AND modules.total_issues > 0"
        .to_owned()
}

/// Datetime fields shifted to the caller's timezone on read
/// (`base.py:391-392, :718-719`; `archive.py:291-292`):
/// `user_timezone_converter(rows, ["created_at", "updated_at"],
/// request.user.user_timezone)` — pytz shift, handler-owned; the DB
/// stores UTC.
pub const TIMEZONE_CONVERTED_FIELDS: &[&str] = &["created_at", "updated_at"];

// ---------------------------------------------------------------------------
// Q2 link queryset
// ---------------------------------------------------------------------------

/// `ModuleLinkViewSet.get_queryset` scope (`base.py:774-788`): this
/// workspace / project / module, caller an active member of a live project.
/// Renders `DISTINCT ... ORDER BY created_at DESC` (Q2 SQL).
pub fn module_link_scope_where() -> String {
    [
        "module_links.deleted_at IS NULL",
        "workspaces.slug = :slug",
        "module_links.project_id = :project_id",
        "module_links.module_id = :module_id",
        "projects.archived_at IS NULL",
        "EXISTS (SELECT 1 FROM project_members pm WHERE pm.project_id = module_links.project_id AND pm.member_id = :user AND pm.is_active = TRUE)",
    ]
    .join(" AND ")
}

/// Q2 order (`base.py:786`): `-created_at`, distinct rows.
pub const MODULE_LINK_ORDER_SQL: &str = "module_links.created_at DESC";

// ---------------------------------------------------------------------------
// Q3 favorite queryset (ported FieldError)
// ---------------------------------------------------------------------------

/// Live Django probe result (FX-MOD-03 Q3): the queryset raises before
/// rendering because `UserFavorite` has no `module` FK. Handlers must
/// reproduce the 500 — never silently serve the scope below.
pub const FAVORITE_SELECT_RELATED_ERROR: &str = "Invalid field name(s) given in select_related: 'module'. Choices are: created_by, updated_by, workspace, project, user, parent";

/// The scope the favorite queryset *intended*
/// (`base.py:795-802` minus the bad `select_related`): this workspace's
/// favorites belonging to the caller.
pub fn favorite_scope_where() -> String {
    "user_favorites.workspace_slug = :slug AND user_favorites.user_id = :user".to_owned()
}

// ---------------------------------------------------------------------------
// Q5 module-issue queryset + Q6 apply_annotations
// ---------------------------------------------------------------------------

/// `ModuleIssueViewSet.get_queryset` scope (`issue.py:84-92`): live
/// bridges of this module in this project/workspace, under the
/// `IssueManager` base. Renders `DISTINCT ... ORDER BY created_at DESC`
/// (Q5 SQL).
pub fn module_issue_scope_where() -> String {
    [
        "issues.project_id = :project_id",
        "workspaces.slug = :slug",
        "module_issues.module_id = :module_id",
        "module_issues.deleted_at IS NULL",
    ]
    .join(" AND ")
}

/// Q5 order (`issue.py` list path, Q5 SQL): `-created_at`, distinct rows.
pub const MODULE_ISSUE_ORDER_SQL: &str = "issues.created_at DESC";

/// Declared filter surface (`issue.py:50-51`): `ComplexFilterBackend` +
/// `IssueFilterSet` run on top of [`module_issue_scope_where`], then the
/// legacy `issue_filters(params, "GET")` dict filter (`issue.py:97-104`),
/// then `order_issue_queryset` (default `created_at`). Leaf resolution is
/// the shared F-04 `pidash_db::filter` / `filterset` kernels (pilot-2
/// `app_issues` pattern) — these consts are the parity surface.
pub const MODULE_ISSUE_FILTER_BACKENDS: &[&str] = &["ComplexFilterBackend"];
pub const MODULE_ISSUE_FILTERSET_CLASS: &str = "IssueFilterSet";
pub const MODULE_ISSUE_DEFAULT_ORDER_PARAM: &str = "created_at";

/// `cycle_id` in `apply_annotations` (`issue.py:56-58`): the live bridge
/// for the issue. The doubled `deleted_at IS NULL` (ported bug 3) and the
/// bridge default `ORDER BY created_at DESC` are kept verbatim (Q6 SQL).
pub fn annotated_cycle_id_sql() -> String {
    "SELECT cycle_issues.cycle_id FROM cycle_issues WHERE cycle_issues.deleted_at IS NULL AND cycle_issues.deleted_at IS NULL AND cycle_issues.issue_id = issues.id ORDER BY cycle_issues.created_at DESC LIMIT 1".to_owned()
}

/// `link_count` in `apply_annotations` (`issue.py:60-65`): `COUNT` over
/// `IssueLink` per issue; the soft-delete guard comes from the manager
/// (Q6 SQL keeps `deleted_at IS NULL`).
pub fn link_count_sql() -> String {
    "SELECT COUNT(issue_links.id) FROM issue_links WHERE issue_links.deleted_at IS NULL AND issue_links.issue_id = issues.id".to_owned()
}

/// `FileAsset.EntityTypeContext.ISSUE_ATTACHMENT` value used by the
/// `attachment_count` annotation (`issue.py:66-74`, Q6 SQL).
pub const ATTACHMENT_ENTITY_TYPE: &str = "ISSUE_ATTACHMENT";

/// `attachment_count` in `apply_annotations` (`issue.py:66-74`).
pub fn attachment_count_sql() -> String {
    format!(
        "SELECT COUNT(file_assets.id) FROM file_assets WHERE file_assets.deleted_at IS NULL AND file_assets.entity_type = '{ATTACHMENT_ENTITY_TYPE}' AND file_assets.issue_id = issues.id"
    )
}

/// `sub_issues_count` in `apply_annotations` (`issue.py:75-80`):
/// children of the issue itself under the `IssueManager` base (vs
/// [`sub_issues_sql`], children counted through this module's bridges —
/// same shape, different outer row). The manager guards are spelled out
/// with the `child` alias (Q6 SQL: `LEFT OUTER JOIN states`, triage /
/// archived-issue / archived-project / draft exclusion); `.order_by()`
/// clears ordering so no `ORDER BY` is rendered.
pub fn issue_sub_issues_count_sql() -> String {
    "SELECT COUNT(child.id) FROM issues child LEFT OUTER JOIN states ON (child.state_id = states.id) JOIN projects ON (child.project_id = projects.id) WHERE child.deleted_at IS NULL AND NOT (states.group = 'triage' AND states.group IS NOT NULL) AND NOT (child.archived_at IS NOT NULL) AND NOT (projects.archived_at IS NOT NULL) AND NOT (child.is_draft) AND child.parent_id = issues.id".to_owned()
}

/// Prefetch hints on the annotated queryset (`issue.py:81`):
/// `assignees`, `labels`, `issue_module__module` — same rows with or
/// without them.
pub const MODULE_ISSUE_PREFETCH: &[&str] = &["assignees", "labels", "issue_module__module"];

/// Link prefetch hint on Q1/Q4 (`base.py:218-223`, `archive.py:185-190`):
/// `link_module` with `select_related("module", "created_by")` — valid
/// here (`ModuleLink` HAS a `module` FK; contrast ported bug 1).
pub const LINK_PREFETCH_RELATED: &[&str] = &["module", "created_by"];

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE_SQL: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/app_modules/queries/module_querysets.sql"
    );
    const FIXTURE_ROWS: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/app_modules/queries/module_querysets.rows.json"
    );

    fn fixture_text(path: &str) -> String {
        std::fs::read_to_string(path).expect("fixture exists")
    }

    fn rows_fixture() -> serde_json::Value {
        let raw = fixture_text(FIXTURE_ROWS);
        serde_json::from_str(&raw).expect("fixture is valid JSON")
    }

    #[test]
    fn shared_scope_carries_tenant_and_manager_guards() {
        let scope = module_scope_where();
        assert!(scope.contains("modules.project_id = :project_id"));
        assert!(scope.contains("workspaces.slug = :slug"));
        let guards = issue_manager_guards_sql();
        for needle in [
            "issues.deleted_at IS NULL",
            "states.group = 'triage'",
            "NOT (issues.archived_at IS NOT NULL)",
            "NOT (projects.archived_at IS NOT NULL)",
            "NOT (issues.is_draft)",
        ] {
            assert!(guards.contains(needle), "missing {needle}");
        }
        assert_eq!(archived_predicate(false), "modules.archived_at IS NULL");
        assert_eq!(archived_predicate(true), "modules.archived_at IS NOT NULL");
    }

    #[test]
    fn favorite_exists_matches_django_lookup() {
        let sql = favorite_exists_sql();
        assert!(sql.starts_with("EXISTS (SELECT 1 FROM user_favorites"));
        for needle in [
            "uf.user_id = :user",
            "uf.entity_identifier = modules.id",
            "uf.entity_type = 'module'",
            "uf.project_id = :project_id",
            "w.slug = :slug",
            "uf.deleted_at IS NULL",
            "LIMIT 1",
        ] {
            assert!(sql.contains(needle), "missing {needle}");
        }
        // Q1 fixture renders the same EXISTS shape.
        let fixture = fixture_text(FIXTURE_SQL);
        assert!(fixture.contains("U0.\"entity_type\" = module"));
        assert!(fixture.contains("AS \"is_favorite\""));
    }

    #[test]
    fn count_subqueries_keep_bridges_and_group_filters() {
        for group in ["completed", "cancelled", "started", "unstarted", "backlog"] {
            let sub = count_issues_sql(GroupFilter::Eq(group));
            assert!(sub.contains("module_issues.deleted_at IS NULL"), "{group}");
            assert!(
                sub.contains("module_issues.module_id = modules.id"),
                "{group}"
            );
            assert!(
                sub.contains(&format!("states.group = '{group}'")),
                "{group}"
            );
            assert!(sub.contains("GROUP BY module_issues.module_id"), "{group}");
            let wrapped = count_coalesce_sql(GroupFilter::Eq(group), &format!("{group}_issues"));
            assert!(wrapped.starts_with("COALESCE(("), "{group}");
            assert!(
                wrapped.ends_with(&format!("LIMIT 1), 0) AS {group}_issues")),
                "{group}"
            );
        }
        let total = count_issues_sql(GroupFilter::All);
        // Only the triage guard mentions a group on the total path.
        assert_eq!(
            total.matches("states.group = '").count(),
            1,
            "total has no group filter beyond triage"
        );
        assert!(
            total.contains("LEFT OUTER JOIN states"),
            "total LEFT JOINs states (Q1)"
        );
        let grouped = count_issues_sql(GroupFilter::Eq("completed"));
        assert!(
            grouped.contains("JOIN states ON"),
            "grouped INNER JOINs states (Q1)"
        );
        assert_eq!(COUNT_ALIASES.len(), 6);
    }

    #[test]
    fn estimate_subqueries_use_points_cast_double() {
        for (group, alias) in ESTIMATE_GROUPS.iter().zip(ESTIMATE_POINT_ALIASES.iter()) {
            let sub = estimate_subquery_sql(*group);
            assert!(
                sub.contains("SUM(CAST(estimate_points.value AS double precision))"),
                "{alias}"
            );
            assert!(sub.contains("estimates.type = 'points'"), "{alias}");
            assert!(
                sub.contains("module_issues.module_id = modules.id"),
                "{alias}"
            );
            assert!(sub.contains("module_issues.deleted_at IS NULL"), "{alias}");
            match group {
                Some(group) => assert!(
                    sub.contains(&format!("states.group = '{group}'")),
                    "{alias}"
                ),
                // Only the triage guard mentions a group on the total path.
                None => assert_eq!(sub.matches("states.group = '").count(), 1, "{alias}"),
            }
            let wrapped = estimate_coalesce_sql(*group, alias);
            assert!(wrapped.starts_with("COALESCE(("), "{alias}");
            assert!(
                wrapped.ends_with(&format!("LIMIT 1), 0.0) AS {alias}")),
                "{alias}"
            );
        }
        assert_eq!(ESTIMATE_POINT_ALIASES.len(), 6);
        // Q1 fixture renders the same estimate shape.
        let fixture = fixture_text(FIXTURE_SQL);
        assert!(fixture.contains("::double precision"));
        assert!(fixture.contains("AS \"backlog_estimate_points\""));
        assert!(fixture.contains("AS \"total_estimate_points\""));
    }

    #[test]
    fn member_ids_guard_asymmetry_is_kept() {
        // Q1 (base.py:283-286): guarded; Q1/Q4 SQL aggregate the through
        // table's real columns (module_members.member_id).
        let base = member_ids_sql(true);
        assert!(base.contains("COALESCE(ARRAY_AGG(DISTINCT module_members.member_id)"));
        assert!(base.contains("module_members.member_id IS NOT NULL"));
        assert!(base.contains("module_members.deleted_at IS NULL"));
        assert!(base.ends_with(", '{}')"));
        // Q4 (archive.py:250): unguarded — ported bug 2.
        let archive = member_ids_sql(false);
        assert!(
            !archive.contains("module_members.deleted_at IS NULL"),
            "archive drops the guard"
        );
        assert!(archive.contains("module_members.member_id IS NOT NULL"));
        // Both shapes render the same columns the fixture does.
        let fixture = fixture_text(FIXTURE_SQL);
        assert!(fixture.contains("ARRAY_AGG(DISTINCT \"module_members\".\"member_id\""));
        assert!(fixture.contains("\"module_members\".\"deleted_at\" IS NULL"));
        assert_eq!(
            MODULE_ORDER_SQL,
            "is_favorite DESC, modules.created_at DESC"
        );
    }

    #[test]
    fn list_projections_match_fixture_contract() {
        let fixture = rows_fixture();
        let contract = fixture["q1_module_viewset_get_queryset_base_py_78_292"]["result_columns"]
            .as_str()
            .expect("Q1 result_columns contract");
        assert!(contract.contains("MODULE_ROW_KEYS"));
        assert!(contract.contains("member_ids uuid[]"));
        assert!(contract.contains("is_favorite bool"));
        assert!(contract.contains("6 counts"));
        // Base list shape: 28 keys in source order.
        assert_eq!(MODULE_LIST_VALUES_FIELDS.len(), 28);
        assert_eq!(
            &MODULE_LIST_VALUES_FIELDS[..5],
            &["id", "workspace_id", "project_id", "name", "description"]
        );
        assert!(MODULE_LIST_VALUES_FIELDS.contains(&"member_ids"));
        assert!(MODULE_LIST_VALUES_FIELDS.contains(&"logo_props"));
        // Archive list: drops logo/estimate-points, adds archived_at.
        assert!(!ARCHIVE_LIST_VALUES_FIELDS.contains(&"logo_props"));
        assert!(!ARCHIVE_LIST_VALUES_FIELDS.contains(&"completed_estimate_points"));
        assert!(!ARCHIVE_LIST_VALUES_FIELDS.contains(&"total_estimate_points"));
        assert!(ARCHIVE_LIST_VALUES_FIELDS.contains(&"archived_at"));
        assert!(ARCHIVE_LIST_VALUES_FIELDS.contains(&"total_issues"));
        // Partial-update re-read: same set as list, different order.
        let mut list_sorted = MODULE_LIST_VALUES_FIELDS.to_vec();
        let mut partial_sorted = MODULE_PARTIAL_UPDATE_VALUES_FIELDS.to_vec();
        list_sorted.sort_unstable();
        partial_sorted.sort_unstable();
        assert_eq!(
            list_sorted, partial_sorted,
            "partial re-read projects the same set"
        );
        assert_ne!(
            MODULE_LIST_VALUES_FIELDS, MODULE_PARTIAL_UPDATE_VALUES_FIELDS,
            "but in a different order"
        );
        assert_eq!(MODULE_NOT_FOUND_BODY, "{\"error\":\"Module not found\"}");
        assert_eq!(
            ARCHIVED_MODULE_UPDATE_BODY,
            "{\"error\":\"Archived module cannot be updated\"}"
        );
    }

    #[test]
    fn retrieve_extras_match_python_paths() {
        let sub = sub_issues_sql();
        assert!(sub.contains("issues.parent_id IS NOT NULL"));
        assert!(sub.contains("module_issues.module_id = :module_id"));
        assert!(sub.contains(":project_id"));
        // Retrieve runs through Issue.issue_objects: manager guards inside.
        for needle in [
            "issues.deleted_at IS NULL",
            "states.group = 'triage'",
            "NOT (issues.archived_at IS NOT NULL)",
            "NOT (projects.archived_at IS NOT NULL)",
            "NOT (issues.is_draft)",
        ] {
            assert!(sub.contains(needle), "sub_issues missing {needle}");
        }
        let gate = estimate_type_exists_sql();
        assert!(gate.contains("estimates.type = 'points'"));
        // workspace__slug / estimate__type are joins (projects carries only
        // workspace_id / estimate_id); the INNER JOIN doubles as the
        // estimate__isnull=False gate.
        assert!(gate.contains("JOIN workspaces ON (projects.workspace_id = workspaces.id)"));
        assert!(gate.contains("JOIN estimates ON (projects.estimate_id = estimates.id)"));
        assert!(gate.contains("workspaces.slug = :slug"));
        assert!(avatar_url_case_sql().contains("CONCAT('/api/assets/v2/static/'"));
        assert!(avatar_url_case_sql().contains("THEN assignees.avatar ELSE NULL END"));
        assert!(
            estimate_sum_sql(None).contains("SUM(CAST(estimate_points.value AS double precision))")
        );
        assert!(!estimate_sum_sql(None).contains("completed_at"));
        assert!(estimate_sum_sql(Some(true)).contains("issues.completed_at IS NOT NULL"));
        assert!(estimate_sum_sql(Some(false)).contains("issues.completed_at IS NULL"));
        assert!(distribution_count_sql(Some(true)).contains("COUNT(issues.id)"));
        assert_eq!(TIMEZONE_CONVERTED_FIELDS, &["created_at", "updated_at"]);
        assert!(points_chart_trigger_sql().contains("start_date IS NOT NULL"));
        assert!(issues_chart_trigger_sql().contains("modules.total_issues > 0"));
        assert_eq!(
            ASSIGNEE_ESTIMATE_ROW_KEYS,
            &[
                "first_name",
                "last_name",
                "assignee_id",
                "avatar_url",
                "display_name",
                "total_estimates",
                "completed_estimates",
                "pending_estimates",
            ]
        );
        assert_eq!(
            LABEL_COUNT_ROW_KEYS,
            &[
                "label_name",
                "color",
                "label_id",
                "total_issues",
                "completed_issues",
                "pending_issues",
            ]
        );
    }

    #[test]
    fn link_scope_matches_q2_fixture() {
        let scope = module_link_scope_where();
        for needle in [
            "module_links.deleted_at IS NULL",
            "workspaces.slug = :slug",
            "module_links.project_id = :project_id",
            "module_links.module_id = :module_id",
            "projects.archived_at IS NULL",
            "pm.member_id = :user",
            "pm.is_active = TRUE",
        ] {
            assert!(scope.contains(needle), "missing {needle}");
        }
        assert_eq!(MODULE_LINK_ORDER_SQL, "module_links.created_at DESC");
        // Q2 fixture renders DISTINCT with the same scope; the membership
        // table is project_members (Q2 SQL), not project_projectmembers.
        assert!(scope.contains("FROM project_members pm"));
        let fixture = fixture_text(FIXTURE_SQL);
        assert!(fixture.contains("SELECT DISTINCT \"module_links\""));
        assert!(fixture.contains("\"project_members\".\"is_active\""));
    }

    #[test]
    fn favorite_fielderror_is_ported_not_fixed() {
        // Ported bug 1: the exact Django error text is the contract.
        assert!(FAVORITE_SELECT_RELATED_ERROR.contains("select_related: 'module'"));
        let fixture = rows_fixture();
        let live = fixture["q3_module_favorite_get_queryset_base_py_795_802"]["live_probe"]
            .as_str()
            .expect("Q3 live_probe contract");
        assert!(
            live.contains("select_related: 'module'"),
            "fixture pins the same error"
        );
        let scope = favorite_scope_where();
        assert!(scope.contains(":slug"));
        assert!(scope.contains(":user"));
    }

    #[test]
    fn module_issue_scope_and_annotations_match_q5_q6() {
        let scope = module_issue_scope_where();
        for needle in [
            "issues.project_id = :project_id",
            "workspaces.slug = :slug",
            "module_issues.module_id = :module_id",
            "module_issues.deleted_at IS NULL",
        ] {
            assert!(scope.contains(needle), "missing {needle}");
        }
        assert_eq!(MODULE_ISSUE_ORDER_SQL, "issues.created_at DESC");
        assert_eq!(MODULE_ISSUE_FILTER_BACKENDS, &["ComplexFilterBackend"]);
        assert_eq!(MODULE_ISSUE_FILTERSET_CLASS, "IssueFilterSet");
        assert_eq!(MODULE_ISSUE_DEFAULT_ORDER_PARAM, "created_at");
        // Q6: doubled deleted_at guard + bridge default ordering (ported bug 3).
        let cycle = annotated_cycle_id_sql();
        assert_eq!(
            cycle.matches("deleted_at IS NULL").count(),
            2,
            "doubled guard kept"
        );
        assert!(cycle.contains("ORDER BY cycle_issues.created_at DESC LIMIT 1"));
        assert!(link_count_sql().contains("issue_links.issue_id = issues.id"));
        assert!(link_count_sql().contains("deleted_at IS NULL"));
        assert_eq!(ATTACHMENT_ENTITY_TYPE, "ISSUE_ATTACHMENT");
        assert!(attachment_count_sql().contains("entity_type = 'ISSUE_ATTACHMENT'"));
        assert!(attachment_count_sql().contains("file_assets.issue_id = issues.id"));
        // sub_issues_count runs through Issue.issue_objects (Q6 SQL carries
        // the full manager guard set under the child alias).
        let sub_count = issue_sub_issues_count_sql();
        assert!(sub_count.contains("child.parent_id = issues.id"));
        for needle in [
            "child.deleted_at IS NULL",
            "states.group = 'triage'",
            "NOT (child.archived_at IS NOT NULL)",
            "NOT (projects.archived_at IS NOT NULL)",
            "NOT (child.is_draft)",
            "LEFT OUTER JOIN states ON (child.state_id = states.id)",
            "JOIN projects ON (child.project_id = projects.id)",
        ] {
            assert!(
                sub_count.contains(needle),
                "sub_issues_count missing {needle}"
            );
        }
        assert_eq!(
            MODULE_ISSUE_PREFETCH,
            &["assignees", "labels", "issue_module__module"]
        );
        assert_eq!(LINK_PREFETCH_RELATED, &["module", "created_by"]);
        // Q5/Q6 fixtures render the same shapes.
        let fixture = fixture_text(FIXTURE_SQL);
        assert!(fixture.contains("SELECT DISTINCT \"issues\""));
        assert!(fixture.contains("\"module_issues\".\"module_id\" = 0403e4b8"));
        assert!(fixture.contains("AS \"cycle_id\""));
        assert!(fixture.contains("AS \"link_count\""));
        assert!(fixture.contains("AS \"attachment_count\""));
        assert!(fixture.contains("AS \"sub_issues_count\""));
    }
}
