#![forbid(unsafe_code)]

//! Project read querysets + identifier routing (D-25, stage 5).
//!
//! Ports the annotated read querysets behind the project / member / invite /
//! favorites / state / estimate endpoints to SQL text, following the D-27
//! precedent (`app_cycles/queries.rs`): each builder returns a fragment the
//! caller splices into the statement it executes. Placeholders stay symbolic
//! — `:slug`, `:project_id`, `:user`, `:user_email`, `:value`,
//! `:identifier`, `:term` — exactly the notation the fixtures use; handlers
//! bind them.
//!
//! Sources (drift baseline `01a93e17`):
//! - `app/views/project/base.py:52-98` — `ProjectViewSet.get_queryset`
//!   (`is_favorite` `Exists` :64-72, `member_role` :73-79, `anchor` :80-86,
//!   `sort_order` :53-57+87, `members_list` `Prefetch` :88-96, `distinct`
//!   :97).
//! - `app/views/project/base.py:101-142` — `list_detail` (guest/member
//!   scoping :104-127, paginate-vs-serializer split :129-142).
//! - `app/views/project/base.py:144-223` — `list` (annotations :155-172,
//!   `.values()` :174-197, scoping :199-222).
//! - `app/views/project/base.py:506-514` — favorites `get_queryset`.
//! - `app/views/project/member.py:33-44` — member `get_queryset`
//!   (`search_fields` :31); `:159-166` — `list` queryset; `:346-356` —
//!   roles `.values` dict.
//! - `app/views/project/invite.py:43-51` — invite `get_queryset`;
//!   `:120-126` — user-invite `get_queryset`.
//! - `app/views/state/base.py:31-46` — state `get_queryset`.
//! - `app/views/estimate/base.py:54-61` — estimate `list`.
//! - `app/views/base.py:49-81` — `_rewrite_project_kwarg` (identifier→UUID
//!   rewrite); `:103-108` — `BaseViewSet.get_queryset`
//!   (`model.objects.all()`); `:89-95` — default filter backends.
//! - `db/models/project.py:191-224` — `Project.resolve` / `resolve_id` SQL
//!   paths (UUID pk vs stripped-upper identifier equality).
//! - `utils/paginator.py:639-760` — `BasePaginator.paginate` envelope +
//!   `OffsetPaginator.get_result` ordering (the `list_detail` paginate path).
//!
//! Fixture oracle: FX-APROJ-06
//! (`rust-api/fixtures/app_project/FX-APROJ-06.queries.json` +
//! `TRACE.md`). The unit tests below pin the builders against that file so
//! transcription drift fails the build.
//!
//! Existing bugs ported as-is (translation, don't redesign):
//! 1. `list` `.values()` selects 22 keys (`base.py:174-197`), not 26 — and
//!    the wire order is NOT the call order: concrete fields render in call
//!    order, then the four annotations append in `annotate()` order
//!    (`member_role`, `intake_count`, `inbox_view`, `sort_order`).
//!    [`LIST_VALUES_COLUMNS`] is the wire order.
//! 2. `list_detail` `.order_by("sort_order", "name")` (`:103`) is dead on
//!    the paginate path — `paginate(order_by=...)` re-orders by the request
//!    param (default `-created_at`, `NULLS LAST`) plus a secondary
//!    `-created_at` (`paginator.py:133-139`). Both orders are ported.
//! 3. `ProjectMemberViewSet.get_queryset` chains a no-op `.filter()`
//!    (`member.py:40`) — ported as no predicate.
//! 4. Member `get_queryset` omits `is_active=True` and the
//!    `member__member_workspace` guards that `list()` (`:159-166`) applies —
//!    the default DRF actions and the custom list see different rows. Both
//!    scopes are ported, never unified.
//! 5. State queryset excludes triage twice: the `StateManager`
//!    (`group != triage`) underneath plus the explicit `is_triage=False`
//!    (`state/base.py:42`). The doubled guard is ported.
//! 6. `Project.resolve` catches `TypeError` although `str(value)` cannot
//!    raise it (`project.py:197-200`, dead branch — L5 notes it too). Every
//!    parse failure is an identifier lookup.
//! 7. The `member_role` / `anchor` / `sort_order` scalar subqueries carry
//!    `ORDER BY created_at DESC` with no `LIMIT` (Django renders no limit
//!    for unsliced subqueries); single-row-ness comes from the partial
//!    unique constraints, not SQL.
//! 8. `list` `.select_related(...)` (`:154`) emits no join — `.values()`
//!    only joins what it references (`workspaces` for the slug filter,
//!    `intake_issues` for the count). The dead call is a docs note.
//! 9. Related-table soft-delete guards are absent across forward-FK
//!    traversal (Django applies only the base model's manager): the roles
//!    and state querysets carry no `workspace_members.deleted_at` /
//!    `project_members.deleted_at` guard on the joined rows. Ported as
//!    observed.
//! 10. Forward-FK filters inner-join: `member__is_bot=False` turns the
//!     nullable `member` FK into an `INNER JOIN`, silently dropping
//!     null-member rows from the member queryset.
//!
//! Out of scope (owned by sibling handler/guard issues): the response
//! envelopes, `create`/`update`/`destroy`, permission gates, Celery
//! enqueues, `handle_exception`. Queryset-adjacent constants handlers need
//! (paginate envelope keys, serializer field lists, the 404 body) ARE in
//! scope here and marked.

// ---------------------------------------------------------------------------
// Project get_queryset: scope + annotations + prefetch
// ---------------------------------------------------------------------------

/// Tenant scope of `ProjectViewSet.get_queryset` (`base.py:58-61`):
/// live rows in this workspace slug. The soft-delete predicate comes from
/// the default manager; the slug filter inner-joins `workspaces`.
pub fn project_scope_where() -> String {
    "projects.deleted_at IS NULL AND workspaces.slug = :slug".to_owned()
}

/// `select_related` chain of `get_queryset` (`base.py:62`): workspace,
/// workspace owner, default assignee, project lead. The first two render
/// `INNER JOIN`s (non-null FKs); the assignee/lead render `LEFT OUTER
/// JOIN`s (nullable FKs) — see the fixture `FROM` clause.
pub const PROJECT_SELECT_RELATED: &[&str] = &[
    "workspace",
    "workspace__owner",
    "default_assignee",
    "project_lead",
];

/// Join shape of [`PROJECT_SELECT_RELATED`], in fixture order.
pub fn project_select_joins_sql() -> String {
    [
        "JOIN workspaces ON (projects.workspace_id = workspaces.id)",
        "JOIN users AS workspace_owner ON (workspaces.owner_id = workspace_owner.id)",
        "LEFT OUTER JOIN users AS default_assignee ON (projects.default_assignee_id = default_assignee.id)",
        "LEFT OUTER JOIN users AS project_lead ON (projects.project_lead_id = project_lead.id)",
    ]
    .join(" ")
}

/// Annotation aliases of `get_queryset` (`base.py:63-87`), in source order.
pub const PROJECT_ANNOTATIONS: &[&str] = &["is_favorite", "member_role", "anchor", "sort_order"];

/// `is_favorite = Exists(UserFavorite...)` (`base.py:64-72`): a live
/// favorite of the caller bridging this project id twice
/// (`entity_identifier` AND `project_id`) with `entity_type='project'`.
pub fn favorite_exists_sql() -> String {
    "EXISTS (SELECT 1 FROM user_favorites uf WHERE uf.deleted_at IS NULL AND uf.entity_identifier = projects.id AND uf.entity_type = 'project' AND uf.project_id = projects.id AND uf.user_id = :user LIMIT 1)".to_owned()
}

/// `member_role` subquery (`base.py:73-79`): the caller's live membership
/// role on this project, newest row first, no `LIMIT` (ported bug 7).
pub fn member_role_sql() -> String {
    "(SELECT pm.role FROM project_members pm WHERE pm.deleted_at IS NULL AND pm.is_active AND pm.member_id = :user AND pm.project_id = projects.id ORDER BY pm.created_at DESC)".to_owned()
}

/// `anchor` subquery (`base.py:80-86`): the deploy-board anchor for this
/// project in this workspace, newest row first, no `LIMIT` (ported bug 7).
pub fn anchor_sql() -> String {
    "(SELECT db.anchor FROM deploy_boards db JOIN workspaces w ON (db.workspace_id = w.id) WHERE db.deleted_at IS NULL AND db.entity_identifier = projects.id AND db.entity_name = 'project' AND w.slug = :slug ORDER BY db.created_at DESC)".to_owned()
}

/// `sort_order` subquery (`base.py:53-57,87`): the caller's
/// `ProjectUserProperty.sort_order` for this project in this workspace,
/// newest row first, no `LIMIT` (ported bug 7).
pub fn sort_order_sql() -> String {
    "(SELECT pup.sort_order FROM project_user_properties pup JOIN workspaces w ON (pup.workspace_id = w.id) WHERE pup.deleted_at IS NULL AND pup.project_id = projects.id AND pup.user_id = :user AND w.slug = :slug ORDER BY pup.created_at DESC)".to_owned()
}

/// `get_queryset` ends in `.distinct()` (`base.py:97`) — required once the
/// scoping joins fan out (guest/member filters duplicate rows).
pub const PROJECT_GET_QUERYSET_DISTINCT: bool = true;

/// Default row order under `get_queryset`: no `order_by` call, so the
/// `Project.Meta.ordering = ("-created_at",)` applies (fixture `ORDER BY`).
pub const PROJECT_DEFAULT_ORDER_SQL: &str = "projects.created_at DESC";

/// `members_list` prefetch (`base.py:88-96`): the `project_projectmember`
/// reverse relation, live memberships in this workspace with the member
/// row joined, stored under `to_attr="members_list"`.
pub const MEMBERS_PREFETCH_RELATED: &str = "project_projectmember";
/// Attribute the prefetch stores under (`to_attr`, `:94`).
pub const MEMBERS_PREFETCH_TO_ATTR: &str = "members_list";
/// `select_related` inside the prefetch queryset (`:93`).
pub const MEMBERS_PREFETCH_SELECT_RELATED: &[&str] = &["member"];

/// Filter inside the `members_list` prefetch queryset (`base.py:91-93`):
/// this workspace slug, active memberships. The manager adds the
/// soft-delete guard.
pub fn members_prefetch_where() -> String {
    "project_members.deleted_at IS NULL AND workspaces.slug = :slug AND project_members.is_active"
        .to_owned()
}

// ---------------------------------------------------------------------------
// Guest / member scoping (list_detail + list)
// ---------------------------------------------------------------------------

/// Workspace role values gating the scoping branches (`ROLE.GUEST=5`,
/// `ROLE.MEMBER=15`; admins fall through unscoped).
pub const GUEST_ROLE: i32 = 5;
/// See [`GUEST_ROLE`].
pub const MEMBER_ROLE: i32 = 15;
/// `ProjectNetwork.PUBLIC` — members also see public projects (`:126,221`).
pub const PUBLIC_NETWORK: i32 = 2;

/// The workspace-role probe each scoping branch runs first
/// (`base.py:104-109`, `:115-120`, `:199-204`, `:210-215`): an active
/// membership of the caller in this workspace with the given role.
/// Django renders the `.exists()` call as `SELECT EXISTS(SELECT 1 ...
/// INNER JOIN workspaces ...)`; the `EXISTS (...)` below is that inner
/// query, which handlers run standalone or splice (same D-27 convention).
pub fn workspace_role_probe_sql(role: i32) -> String {
    format!(
        "EXISTS (SELECT 1 FROM workspace_members wm JOIN workspaces w ON (wm.workspace_id = w.id) WHERE wm.member_id = :user AND w.slug = :slug AND wm.is_active AND wm.role = {role})"
    )
}

/// Guest branch (`base.py:110-113`, `:205-208`): only projects where the
/// caller is an active member. Django renders the reverse-FK filter as an
/// `INNER JOIN`; with `DISTINCT` already on the queryset that is exactly
/// the `EXISTS` below (same D-27 convention).
pub fn guest_scope_where() -> String {
    "EXISTS (SELECT 1 FROM project_members pm WHERE pm.project_id = projects.id AND pm.member_id = :user AND pm.is_active AND pm.deleted_at IS NULL)".to_owned()
}

/// Member branch (`base.py:121-127`, `:216-222`): member projects OR public
/// projects. The two role probes are independent `if`s — a caller holding
/// both a guest and a member row gets both filters conjunctively; ported
/// as written, not simplified.
pub fn member_scope_where() -> String {
    format!(
        "({} OR projects.network = {PUBLIC_NETWORK})",
        guest_scope_where()
    )
}

// ---------------------------------------------------------------------------
// Project list shapes: .values() + list_detail paths
// ---------------------------------------------------------------------------

/// `list` `.values()` wire keys (`base.py:174-197`) in wire order: the 18
/// concrete fields in call order, then the 4 annotations in `annotate()`
/// order (ported bug 1 — 22 keys, not 26). FK attnames render under the
/// name given in the call (`workspace`, `project_lead`, `created_by`,
/// `updated_by`), matching the fixture row key order exactly.
pub const LIST_VALUES_COLUMNS: &[&str] = &[
    "id",
    "name",
    "identifier",
    "logo_props",
    "archived_at",
    "workspace",
    "cycle_view",
    "issue_views_view",
    "module_view",
    "page_view",
    "is_default",
    "guest_view_all_features",
    "project_lead",
    "network",
    "created_at",
    "updated_at",
    "created_by",
    "updated_by",
    "member_role",
    "intake_count",
    "inbox_view",
    "sort_order",
];

/// `IntakeIssueStatus.PENDING` — the `intake_count` filter value
/// (`base.py:166`, fixture renders `status = -2`).
pub const INTAKE_PENDING_STATUS: i32 = -2;

/// `intake_count = Count("project_intakeissue", filter=...)`
/// (`base.py:162-170`): live pending intake issues per project. Django
/// renders a `LEFT OUTER JOIN intake_issues` plus `GROUP BY projects.id`
/// (ported bug 9 shape — a join, not a correlated subquery).
pub fn intake_count_sql() -> String {
    format!(
        "COUNT(intake_issues.id) FILTER (WHERE intake_issues.deleted_at IS NULL AND intake_issues.status = {INTAKE_PENDING_STATUS})"
    )
}

/// Join the `intake_count` aggregate requires (`base.py:163`).
pub const INTAKE_COUNT_JOIN_SQL: &str =
    "LEFT OUTER JOIN intake_issues ON (projects.id = intake_issues.project_id)";

/// Grouping the `list` queryset carries once the count joins
/// (`GROUP BY "projects"."id"` in the fixture).
pub const LIST_GROUP_BY_SQL: &str = "projects.id";

/// `inbox_view = F("intake_view")` (`base.py:171`): a pure column alias,
/// no computation.
pub const INBOX_VIEW_SOURCE_COLUMN: &str = "intake_view";

/// `list` keeps `.distinct()` (`base.py:173`).
pub const LIST_DISTINCT: bool = true;

/// `list_detail` row order (`.order_by("sort_order", "name")`, `:103`):
/// ascending sort order, then name. No `NULLS` handling — Postgres puts
/// the `NULL` sort orders (callers without a property row) last by
/// default. Dead on the paginate path (ported bug 2).
pub const LIST_DETAIL_ORDER_SQL: &str = "sort_order ASC, projects.name ASC";

/// Gate for the paginate path (`base.py:129`): BOTH `per_page` and `cursor`
/// must be present and truthy; otherwise the serializer path runs.
pub const PAGINATE_GATE_PARAMS: &[&str] = &["per_page", "cursor"];

/// Default `order_by` the paginate call passes
/// (`request.GET.get("order_by", "-created_at")`, `:131`).
pub const PAGINATE_DEFAULT_ORDER_PARAM: &str = "-created_at";

/// Order the paginator applies (`OffsetPaginator.get_result`,
/// `paginator.py:133-139`): the requested key (descending when prefixed
/// with `-`, always `NULLS LAST`) plus a secondary `-created_at`.
/// `key` is the param with a leading `-` stripped.
pub fn paginate_order_sql(desc: bool, key: &str) -> String {
    let direction = if desc { "DESC" } else { "ASC" };
    format!("{key} {direction} NULLS LAST, projects.created_at DESC")
}

/// Query param carrying the serializer field allowlist on the
/// non-paginate path (`base.py:102`): comma-split, empties dropped,
/// empty list means no filtering (`fields=... if fields else None`).
pub const FIELDS_PARAM: &str = "fields";

/// Split `?fields=` per `base.py:102`.
pub fn split_fields_param(raw: &str) -> Vec<String> {
    raw.split(',')
        .filter(|field| !field.is_empty())
        .map(str::to_owned)
        .collect()
}

/// `BasePaginator.paginate` envelope keys (`paginator.py:717-732`), in
/// response order. `on_results` here is the `ProjectListSerializer`
/// (`base.py:134-137`).
pub const PAGINATE_ENVELOPE_KEYS: &[&str] = &[
    "grouped_by",
    "sub_grouped_by",
    "total_count",
    "next_cursor",
    "prev_cursor",
    "next_page_results",
    "prev_page_results",
    "count",
    "total_pages",
    "total_results",
    "extra_stats",
    "results",
];

/// Cursor wire format (`Cursor.__str__`, `paginator.py:32-33`):
/// `value:offset:is_prev`.
pub const CURSOR_FORMAT: &str = "value:offset:is_prev";

/// Cursor query param name (`BasePaginator.cursor_name`, `:639`).
pub const CURSOR_PARAM: &str = "cursor";

/// `paginate` defaults (`:660-661`): 1000/1000. A non-integer `per_page`
/// is a 400 (`"Invalid per_page parameter."`); over max is a 400
/// (`"Invalid per_page value. Cannot exceed 1000."`); a malformed cursor
/// is a 400 (`"Invalid cursor parameter."`); a bad offset is a 400
/// (`"Error in parsing"`).
pub const PAGINATE_DEFAULT_PER_PAGE: i64 = 1000;
/// See [`PAGINATE_DEFAULT_PER_PAGE`].
pub const PAGINATE_MAX_PER_PAGE: i64 = 1000;

// ---------------------------------------------------------------------------
// Member reads
// ---------------------------------------------------------------------------

/// Scope of `ProjectMemberViewSet.get_queryset` (`member.py:33-44`): live
/// memberships in this workspace slug on this project whose member is not
/// a bot. No `is_active` predicate here (ported bug 4); the stray
/// `.filter()` (`:40`) contributes nothing (ported bug 3).
pub fn member_queryset_scope_where() -> String {
    "project_members.deleted_at IS NULL AND workspaces.slug = :slug AND project_members.project_id = :project_id AND NOT users.is_bot".to_owned()
}

/// `select_related` chain of the member queryset (`member.py:41-43`).
/// `member` renders an `INNER JOIN` (the `is_bot` filter forces it —
/// ported bug 10); `project` and `workspace` are inner (non-null FKs),
/// `workspace__owner` inner.
pub const MEMBER_SELECT_RELATED: &[&str] = &["project", "member", "workspace", "workspace__owner"];

/// Row order under the member queryset: no `order_by` call, so
/// `ProjectMember.Meta.ordering = ("-created_at",)` applies.
pub const MEMBER_ORDER_SQL: &str = "project_members.created_at DESC";

/// `search_fields` of the member viewset (`member.py:31`).
pub const MEMBER_SEARCH_FIELDS: &[&str] = &["member__display_name", "member__first_name"];

/// Filter backends of the member viewset: inherited unchanged from
/// `BaseViewSet` (`views/base.py:89`). `filterset_fields` is empty, so
/// the `DjangoFilterBackend` is a practical no-op; the `SearchFilter`
/// applies [`member_search_sql`].
pub const MEMBER_FILTER_BACKENDS: &[&str] = &["DjangoFilterBackend", "SearchFilter"];

/// `SearchFilter` rendering for one term over [`MEMBER_SEARCH_FIELDS`]:
/// case-insensitive contains on the member's display and first names.
/// Django renders `UPPER(col::text) LIKE UPPER(%term%)` (fixture
/// `member_search_sql`); the caller wraps `term` in `%...%`.
pub fn member_search_sql() -> String {
    "(UPPER(users.display_name::text) LIKE UPPER(:term) OR UPPER(users.first_name::text) LIKE UPPER(:term))".to_owned()
}

/// Scope of the custom `list()` queryset (`member.py:159-166`): the
/// `get_queryset` scope PLUS `is_active` and live workspace membership of
/// the member in this slug (ported bug 4 — both scopes kept).
pub fn member_list_scope_where() -> String {
    "project_members.deleted_at IS NULL AND workspaces.slug = :slug AND project_members.project_id = :project_id AND NOT users.is_bot AND project_members.is_active AND member_workspace_members.is_active AND member_workspaces.slug = :slug".to_owned()
}

/// `select_related` of the custom `list()` (`member.py:166`): project,
/// member, workspace — no owner join, unlike `get_queryset`.
pub const MEMBER_LIST_SELECT_RELATED: &[&str] = &["project", "member", "workspace"];

/// Serializer field allowlist of the custom `list()`
/// (`fields=("id", "member", "role")`, `:168`).
pub const MEMBER_LIST_FIELDS: &[&str] = &["id", "member", "role"];

// ---------------------------------------------------------------------------
// Invite reads
// ---------------------------------------------------------------------------

/// Scope of `ProjectInvitationsViewset.get_queryset` (`invite.py:43-51`):
/// live invites in this workspace slug on this project.
pub fn invite_scope_where() -> String {
    "project_member_invites.deleted_at IS NULL AND workspaces.slug = :slug AND project_member_invites.project_id = :project_id".to_owned()
}

/// `select_related` of the invite queryset (`invite.py:49-50`).
pub const INVITE_SELECT_RELATED: &[&str] = &["project", "workspace", "workspace__owner"];

/// Row order under both invite querysets:
/// `ProjectMemberInvite.Meta.ordering = ("-created_at",)`.
pub const INVITE_ORDER_SQL: &str = "project_member_invites.created_at DESC";

/// Scope of `UserProjectInvitationsViewset.get_queryset`
/// (`invite.py:120-126`): live invites addressed to the caller's email,
/// across all workspaces (no slug predicate).
pub fn user_invite_scope_where() -> String {
    "project_member_invites.deleted_at IS NULL AND project_member_invites.email = :user_email"
        .to_owned()
}

/// `select_related` of the user-invite queryset (`invite.py:125`).
pub const USER_INVITE_SELECT_RELATED: &[&str] = &["workspace", "workspace__owner", "project"];

// ---------------------------------------------------------------------------
// Favorites reads
// ---------------------------------------------------------------------------

/// Scope of `ProjectFavoritesViewSet.get_queryset`
/// (`project/base.py:506-514`): the caller's live favorites in this
/// workspace slug.
pub fn favorite_scope_where() -> String {
    "user_favorites.deleted_at IS NULL AND workspaces.slug = :slug AND user_favorites.user_id = :user".to_owned()
}

/// `select_related` of the favorites queryset (`:512-513`): project plus
/// its lead/assignee, workspace plus its owner. `project` renders `LEFT
/// OUTER JOIN` (nullable FK on `WorkspaceBaseModel`); the rest inner.
pub const FAVORITE_SELECT_RELATED: &[&str] = &[
    "project",
    "project__project_lead",
    "project__default_assignee",
    "workspace",
    "workspace__owner",
];

/// Row order under the favorites queryset:
/// `UserFavorite.Meta.ordering = ("-created_at",)`.
pub const FAVORITE_ORDER_SQL: &str = "user_favorites.created_at DESC";

/// `serializer_class` of `ProjectFavoritesViewSet` is `None` (inherited
/// from `ModelViewSet`): the DRF-default list action raises
/// `AssertionError` → generic 500 (fixture `favorites_serializer_class`;
/// the 500 body is owned by FX-APROJ-08 / L8).
pub const FAVORITE_SERIALIZER_CLASS: Option<&str> = None;

// ---------------------------------------------------------------------------
// Roles dict
// ---------------------------------------------------------------------------

/// Scope of `UserProjectRolesEndpoint.get` (`member.py:346-353`): the
/// caller's live memberships in this workspace slug where the member also
/// holds a live workspace membership in the same slug. Both workspace
/// joins carry the slug predicate (fixture `roles_values` SQL), and the
/// joined `workspace_members` rows carry NO soft-delete guard (ported
/// bug 9).
pub fn roles_scope_where() -> String {
    "project_members.deleted_at IS NULL AND project_members.is_active AND workspace_members.is_active AND workspaces.slug = :slug AND member_workspaces.slug = :slug AND project_members.member_id = :user".to_owned()
}

/// `.values("project_id", "role")` projection (`member.py:353`).
pub const ROLES_VALUES: &[&str] = &["project_id", "role"];

/// Row order under the roles queryset (`Meta.ordering`, newest first).
pub const ROLES_ORDER_SQL: &str = "project_members.created_at DESC";

/// Fold one roles row into the response dict (`member.py:355`):
/// `{str(project_id): role}`. Iteration is newest-first and later rows
/// overwrite earlier ones per key — moot under the partial unique
/// constraint (one live row per project+member), where join fan-out
/// carries identical values anyway.
pub fn roles_fold(rows: &[(String, i32)]) -> std::collections::BTreeMap<String, i32> {
    rows.iter().cloned().collect()
}

// ---------------------------------------------------------------------------
// State reads
// ---------------------------------------------------------------------------

/// Scope of `StateViewSet.get_queryset` (`state/base.py:31-46`): live
/// non-triage states on this project in this workspace slug where the
/// caller is an active project member and the project is not archived.
/// Triage is excluded twice — the `StateManager` (`group != triage`)
/// underneath plus the explicit `is_triage=False` (`:42`, ported bug 5).
/// The joined `project_members` rows carry NO soft-delete guard (ported
/// bug 9).
pub fn state_scope_where() -> String {
    "states.deleted_at IS NULL AND NOT (states.group = 'triage') AND workspaces.slug = :slug AND states.project_id = :project_id AND project_members.member_id = :user AND project_members.is_active AND projects.archived_at IS NULL AND NOT states.is_triage".to_owned()
}

/// `select_related` of the state queryset (`state/base.py:43-44`).
pub const STATE_SELECT_RELATED: &[&str] = &["project", "workspace"];

/// The state queryset ends in `.distinct()` (`state/base.py:45`).
pub const STATE_DISTINCT: bool = true;

/// Row order under the state queryset: `State.Meta.ordering =
/// ("sequence",)`.
pub const STATE_ORDER_SQL: &str = "states.sequence ASC";

/// `State.objects` manager scope (`db/models/state.py:79-83`): live rows
/// outside the triage group.
pub fn state_objects_where() -> String {
    "states.deleted_at IS NULL AND NOT (states.group = 'triage')".to_owned()
}

/// `State.triage_objects` manager scope (`state.py:86-90`): live rows in
/// the triage group.
pub fn state_triage_objects_where() -> String {
    "states.deleted_at IS NULL AND states.group = 'triage'".to_owned()
}

/// `State.all_state_objects` is a plain `models.Manager()`
/// (`state.py:110`): NO soft-delete and NO group predicate — callers see
/// every row including soft-deleted ones.
pub const STATE_ALL_OBJECTS_WHERE: &str = "";

// ---------------------------------------------------------------------------
// Estimate reads
// ---------------------------------------------------------------------------

/// Scope of `BulkEstimatePointEndpoint.list` (`estimate/base.py:54-61`):
/// live estimates on this project in this workspace slug.
pub fn estimate_scope_where() -> String {
    "estimates.deleted_at IS NULL AND estimates.project_id = :project_id AND workspaces.slug = :slug".to_owned()
}

/// Prefetch of the estimate list (`:57`): the `points` reverse relation
/// (`EstimatePoint.estimate`, `related_name="points"`).
pub const ESTIMATE_PREFETCH: &[&str] = &["points"];

/// `select_related` of the estimate list (`:58`). Both joins render
/// `INNER JOIN` (non-null FKs).
pub const ESTIMATE_SELECT_RELATED: &[&str] = &["workspace", "project"];

/// Row order under the estimate list: `Estimate.Meta.ordering =
/// ("name",)`.
pub const ESTIMATE_ORDER_SQL: &str = "estimates.name ASC";

/// Scope of the project-estimate-points `get`
/// (`ProjectEstimatePointEndpoint.get`, `estimate/base.py:39-43`,
/// handler-adjacent): live points of the project's estimate on this
/// project in this slug. The `estimate_id is not None` branch returns
/// `[]` without a query.
pub fn estimate_point_scope_where() -> String {
    "estimate_points.deleted_at IS NULL AND estimate_points.estimate_id = :estimate_id AND estimate_points.project_id = :project_id AND workspaces.slug = :slug".to_owned()
}

// ---------------------------------------------------------------------------
// Identifier routing: _rewrite_project_kwarg + Project.resolve
// ---------------------------------------------------------------------------

/// Which URL kwarg `_rewrite_project_kwarg` targets
/// (`views/base.py:64-70`): `project_id` whenever present and not `None`;
/// otherwise `pk` — but ONLY when the resolved route name is exactly
/// `"project"` (the three core project routes share that name,
/// `app/urls/project.py:28-45`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RewriteTarget {
    ProjectId,
    Pk,
}

/// Route name gating the `pk` rewrite (`views/base.py:67`).
pub const REWRITE_PK_URL_NAME: &str = "project";

/// Pick the rewrite target per `views/base.py:64-70`. `has_project_id` is
/// `"project_id" in kwargs and kwargs["project_id"] is not None`;
/// `pk_is_set` is `kwargs.get("pk") is not None`; `url_name` is the
/// resolved route name. Returns `None` when no kwarg qualifies.
pub fn rewrite_target(
    has_project_id: bool,
    pk_is_set: bool,
    url_name: &str,
) -> Option<RewriteTarget> {
    if has_project_id {
        return Some(RewriteTarget::ProjectId);
    }
    if pk_is_set && url_name == REWRITE_PK_URL_NAME {
        return Some(RewriteTarget::Pk);
    }
    None
}

/// Gate for attempting the rewrite at all (`views/base.py:55-62`):
/// unauthenticated requests skip it (closing the slug-existence oracle),
/// as do requests without a workspace slug. Handlers must still run the
/// `IsAuthenticated` 401 and the permission gates afterwards.
pub fn should_attempt_rewrite(is_authenticated: bool, slug: Option<&str>) -> bool {
    is_authenticated && slug.is_some_and(|slug| !slug.is_empty())
}

/// UUID passthrough (`views/base.py:73-78`): inputs that parse as a UUID
/// are returned unrewritten — no database hit. `Uuid::parse_str` accepts
/// the same spellings Python's `uuid.UUID()` does, so the classifier
/// matches exactly (L5 ports the same rule for `resolve` itself).
pub fn is_uuid_like(raw: &str) -> bool {
    uuid::Uuid::parse_str(raw).is_ok()
}

/// Python `str.strip()` membership (`db/models/project.py:210`): Rust
/// `White_Space` plus U+001C-U+001F (verified by exhaustively diffing
/// `str.strip` against `char::is_whitespace` over all code points —
/// those four are the only differences).
fn is_py_strip_ws(ch: char) -> bool {
    ch.is_whitespace() || matches!(ch, '\u{1c}'..='\u{1f}')
}

/// Normalize a non-UUID identifier for the equality lookup
/// (`db/models/project.py:210`): `str(value).strip().upper()`. Mirrors
/// the L5 `normalize_identifier`; the test pins them equal.
pub fn normalize_resolve_identifier(raw: &str) -> String {
    raw.trim_matches(is_py_strip_ws).to_uppercase()
}

/// Partial unique btree the identifier path uses
/// (`project_unique_identifier_workspace_when_deleted_at_null`,
/// `project.py:234-238`): `(identifier, workspace)` where `deleted_at IS
/// NULL`. The lookup is plain equality on the already-normalized value —
/// never `__iexact`, which would expand to `UPPER(identifier)` and force
/// a sequential scan (`project.py:205-207`). Same index usage is part of
/// Done-when.
pub const RESOLVE_IDENTIFIER_INDEX: &str =
    "project_unique_identifier_workspace_when_deleted_at_null";

/// `Project.resolve` UUID path (`project.py:197-199`): pk equality in this
/// workspace slug. The `deleted_at IS NULL` guard appears TWICE (default
/// manager plus the explicit `deleted_at__isnull=True`), the slug filter
/// inner-joins `workspaces`, rows order `-created_at`, and `.first()`
/// takes one. `:value` binds the UUID.
pub fn resolve_uuid_sql() -> String {
    "SELECT projects.* FROM projects JOIN workspaces ON (projects.workspace_id = workspaces.id) WHERE projects.deleted_at IS NULL AND projects.deleted_at IS NULL AND projects.id = :value AND workspaces.slug = :slug ORDER BY projects.created_at DESC LIMIT 1".to_owned()
}

/// `Project.resolve` identifier path (`project.py:200-212`): equality on
/// the stripped-upper identifier in this slug. Same doubled guard, join,
/// order, and limit as [`resolve_uuid_sql`]; `:identifier` binds the
/// [`normalize_resolve_identifier`] output. `resolve_id` (`:221-224`) is
/// this same statement projecting `projects.id`.
pub fn resolve_identifier_sql() -> String {
    "SELECT projects.* FROM projects JOIN workspaces ON (projects.workspace_id = workspaces.id) WHERE projects.deleted_at IS NULL AND projects.deleted_at IS NULL AND projects.identifier = :identifier AND workspaces.slug = :slug ORDER BY projects.created_at DESC LIMIT 1".to_owned()
}

/// Generic 404 detail `Project.resolve` raises (`project.py:214-218`) —
/// the input value is never echoed. Mirrors the L5 `NOT_FOUND_DETAIL`;
/// the test pins them equal.
pub const RESOLVE_NOT_FOUND_DETAIL: &str = "Project not found";

/// Wire body of the resolve miss: DRF's `exception_handler` propagates
/// `Http404` args via `NotFound(*exc.args)`, rendering
/// `{"Detail": "Project not found"}` (capital `D` —
/// `rest_framework/views.py:97`, pinned by
/// `contract-tests/app_project/test_permissions.py:20`) with status 404.
pub const RESOLVE_NOT_FOUND_BODY: &str = "{\"Detail\": \"Project not found\"}";

/// Rewrite write-back (`views/base.py:80-81`): the resolved UUID string
/// replaces the raw kwarg in BOTH `kwargs` and `view.kwargs` (the same
/// dict in practice — belt-and-braces, ported as a sequencing note for
/// handlers, not a helper).
pub const REWRITE_WRITES_VIEW_KWARGS: bool = true;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn fixture_text() -> String {
        let path = format!(
            "{}/../../fixtures/app_project/FX-APROJ-06.queries.json",
            env!("CARGO_MANIFEST_DIR")
        );
        std::fs::read_to_string(&path).expect("fixture exists")
    }

    fn fixture() -> Value {
        serde_json::from_str(&fixture_text()).expect("fixture parses")
    }

    fn section<'a>(fixture: &'a Value, name: &str) -> &'a Value {
        fixture
            .get(name)
            .unwrap_or_else(|| panic!("fixture lacks {name}"))
    }

    fn section_sql<'a>(fixture: &'a Value, name: &str) -> &'a str {
        section(fixture, name)["sql"]
            .as_str()
            .unwrap_or_else(|| panic!("{name} lacks sql"))
    }

    /// Top-level keys of a `{...}` JSON object span, in byte order.
    /// Depth- and string-aware; proves wire order without `preserve_order`
    /// (same idiom as `ser_project::tests::object_keys`).
    fn object_keys(span: &str) -> Vec<String> {
        let chars: Vec<char> = span.chars().collect();
        let mut keys = Vec::new();
        let mut depth = 0usize;
        let mut i = 0;
        while i < chars.len() {
            match chars[i] {
                '"' => {
                    let mut j = i + 1;
                    let mut s = String::new();
                    while j < chars.len() && chars[j] != '"' {
                        if chars[j] == '\\' {
                            j += 1;
                            if j < chars.len() {
                                s.push(chars[j]);
                                j += 1;
                            }
                        } else {
                            s.push(chars[j]);
                            j += 1;
                        }
                    }
                    let mut k = j + 1;
                    while k < chars.len() && chars[k].is_whitespace() {
                        k += 1;
                    }
                    if depth == 1 && k < chars.len() && chars[k] == ':' {
                        keys.push(s);
                    }
                    i = j + 1;
                }
                '{' | '[' => {
                    depth += 1;
                    i += 1;
                }
                '}' | ']' => {
                    depth = depth.saturating_sub(1);
                    i += 1;
                }
                _ => {
                    i += 1;
                }
            }
        }
        keys
    }

    /// Span of the first object in a `"rows": [...]` array under the named
    /// section of the raw fixture text.
    fn first_row_span<'a>(raw: &'a str, section: &str) -> &'a str {
        let at = raw
            .find(&format!("\"{section}\""))
            .unwrap_or_else(|| panic!("raw lacks {section}"));
        let rows = raw[at..]
            .find("\"rows\"")
            .map(|i| at + i)
            .unwrap_or_else(|| panic!("{section} lacks rows"));
        let open = raw[rows..]
            .find('{')
            .map(|i| rows + i)
            .expect("row object opens");
        let chars: Vec<char> = raw.chars().collect();
        let mut i = raw[..open].chars().count();
        let mut depth = 0usize;
        let mut in_string = false;
        let mut end = open;
        while i < chars.len() {
            let c = chars[i];
            if in_string {
                if c == '\\' {
                    i += 1;
                } else if c == '"' {
                    in_string = false;
                }
            } else if c == '"' {
                in_string = true;
            } else if c == '{' {
                depth += 1;
            } else if c == '}' {
                depth -= 1;
                if depth == 0 {
                    end = raw
                        .char_indices()
                        .nth(i)
                        .map(|(b, _)| b + 1)
                        .expect("byte end");
                    break;
                }
            }
            i += 1;
        }
        &raw[open..end]
    }

    #[test]
    fn project_annotations_match_fixture() {
        let fx = fixture();
        let mut names = PROJECT_ANNOTATIONS.to_vec();
        names.sort_unstable();
        let mut pinned: Vec<&str> = section(&fx, "project_get_queryset_annotations")
            .as_array()
            .expect("annotations array")
            .iter()
            .map(|v| v.as_str().expect("str"))
            .collect();
        pinned.sort_unstable();
        assert_eq!(names, pinned);

        let sql = section_sql(&fx, "project_get_queryset");
        assert!(sql.starts_with("SELECT DISTINCT"), "distinct kept");
        for alias in ["is_favorite", "member_role", "anchor", "sort_order"] {
            assert!(
                sql.contains(&format!("AS \"{alias}\"")),
                "missing annotation {alias}"
            );
        }
        // is_favorite Exists shape.
        assert!(sql.contains("EXISTS(SELECT 1"));
        assert!(sql.contains("FROM \"user_favorites\""));
        assert!(sql.contains("entity_type"));
        assert!(sql.contains("LIMIT 1"));
        // Scalar subqueries: newest-first, no LIMIT.
        assert!(sql.contains("ORDER BY U0.\"created_at\" DESC)"));
        assert!(!sql.contains("LIMIT 1) AS \"member_role\""));
        // Tenant scope + default order.
        assert!(sql.contains("\"workspaces\".\"slug\" = fx06w-6d98814a"));
        assert!(sql.contains("ORDER BY \"projects\".\"created_at\" DESC"));
        // select_related joins: workspace + owner inner, assignee/lead left.
        assert!(sql.contains("INNER JOIN \"workspaces\""));
        assert!(sql.contains("INNER JOIN \"users\" ON (\"workspaces\".\"owner_id\""));
        assert!(sql.contains("LEFT OUTER JOIN \"users\" T4"));
        assert!(sql.contains("LEFT OUTER JOIN \"users\" T5"));

        // Our builders carry the same predicates with symbolic placeholders.
        let fav = favorite_exists_sql();
        for needle in [
            "EXISTS (SELECT 1 FROM user_favorites",
            "uf.deleted_at IS NULL",
            "uf.entity_identifier = projects.id",
            "uf.entity_type = 'project'",
            "uf.project_id = projects.id",
            "uf.user_id = :user",
            "LIMIT 1",
        ] {
            assert!(fav.contains(needle), "favorite missing {needle}");
        }
        let role = member_role_sql();
        assert!(role.contains("pm.member_id = :user"));
        assert!(role.contains("pm.project_id = projects.id"));
        assert!(role.contains("ORDER BY pm.created_at DESC"));
        assert!(!role.contains("LIMIT"), "no limit (ported bug 7)");
        let anchor = anchor_sql();
        for needle in [
            "db.entity_name = 'project'",
            "db.entity_identifier = projects.id",
            "w.slug = :slug",
            "ORDER BY db.created_at DESC",
        ] {
            assert!(anchor.contains(needle), "anchor missing {needle}");
        }
        let sort = sort_order_sql();
        for needle in [
            "pup.project_id = projects.id",
            "pup.user_id = :user",
            "w.slug = :slug",
            "ORDER BY pup.created_at DESC",
        ] {
            assert!(sort.contains(needle), "sort_order missing {needle}");
        }
        assert!(project_scope_where().contains("workspaces.slug = :slug"));
        const { assert!(PROJECT_GET_QUERYSET_DISTINCT) };
        assert_eq!(PROJECT_SELECT_RELATED.len(), 4);
        assert_eq!(MEMBERS_PREFETCH_TO_ATTR, "members_list");
        assert!(members_prefetch_where().contains("project_members.is_active"));
    }

    #[test]
    fn project_row_shape() {
        let raw = fixture_text();
        let fx = fixture();
        let row = &section(&fx, "project_get_queryset")["rows"][0];
        assert_eq!(row["is_favorite"], Value::Bool(false));
        assert_eq!(row["member_role"], Value::from(20));
        assert!(row["anchor"].is_null());
        assert_eq!(row["sort_order"], Value::from(65535.0));
        assert_eq!(row["members_list"].as_array().expect("list").len(), 1);
        // Wire order: model fields, then the four annotations in source
        // order, then the prefetched attr.
        let keys = object_keys(first_row_span(&raw, "project_get_queryset"));
        let tail = &keys[keys.len() - 5..];
        assert_eq!(
            tail,
            [
                "is_favorite",
                "member_role",
                "anchor",
                "sort_order",
                "members_list"
            ]
        );
    }

    #[test]
    fn list_values_wire_order_and_sql() {
        let raw = fixture_text();
        let fx = fixture();
        assert_eq!(section(&fx, "list_values_key_count"), &Value::from(22));
        let keys = object_keys(first_row_span(&raw, "list_values"));
        let expected: Vec<String> = LIST_VALUES_COLUMNS.iter().map(|s| s.to_string()).collect();
        assert_eq!(keys, expected);

        let sql = section_sql(&fx, "list_values");
        assert!(sql.starts_with("SELECT DISTINCT"));
        assert!(sql.contains("COUNT(\"intake_issues\".\"id\") FILTER"));
        assert!(sql.contains("\"intake_issues\".\"status\" = -2"));
        assert!(sql.contains("AS \"intake_count\""));
        assert!(sql.contains("\"projects\".\"intake_view\" AS \"inbox_view\""));
        assert!(sql.contains("AS \"member_role\""));
        assert!(sql.contains("AS \"sort_order\""));
        assert!(sql.contains("GROUP BY \"projects\".\"id\""));
        assert!(sql.contains("LEFT OUTER JOIN \"intake_issues\""));
        // select_related is dead on list: only workspaces + intake join.
        assert!(!sql.contains("LEFT OUTER JOIN \"users\""));

        assert_eq!(INTAKE_PENDING_STATUS, -2);
        let count = intake_count_sql();
        assert!(count.contains("COUNT(intake_issues.id) FILTER"));
        assert!(count.contains("intake_issues.status = -2"));
        assert_eq!(INBOX_VIEW_SOURCE_COLUMN, "intake_view");
        const { assert!(LIST_DISTINCT) };
    }

    #[test]
    fn member_queryset_and_search() {
        let fx = fixture();
        let attrs = section(&fx, "member_view_attrs");
        let fields: Vec<&str> = attrs["search_fields"]
            .as_array()
            .expect("array")
            .iter()
            .map(|v| v.as_str().expect("str"))
            .collect();
        assert_eq!(fields, MEMBER_SEARCH_FIELDS);
        let backends: Vec<&str> = attrs["filter_backends"]
            .as_array()
            .expect("array")
            .iter()
            .map(|v| v.as_str().expect("str"))
            .collect();
        assert_eq!(backends, MEMBER_FILTER_BACKENDS);

        let sql = section_sql(&fx, "member_get_queryset");
        assert!(sql.contains("NOT \"users\".\"is_bot\""));
        assert!(sql.contains("\"project_members\".\"project_id\" = "));
        assert!(sql.contains("\"workspaces\".\"slug\" = fx06w-6d98814a"));
        assert!(sql.contains("ORDER BY \"project_members\".\"created_at\" DESC"));
        // No is_active predicate on the get_queryset path (ported bug 4):
        // scope the check to the WHERE clause (the SELECT list names the
        // column).
        let where_clause = &sql[sql.find("WHERE").expect("where")..];
        assert!(!where_clause.contains("\"project_members\".\"is_active\""));

        let search = section(&fx, "member_search_sql").as_str().expect("str");
        assert!(search.contains("UPPER(\"users\".\"display_name\"::text) LIKE UPPER("));
        assert!(search.contains("OR UPPER(\"users\".\"first_name\"::text) LIKE UPPER("));

        let scope = member_queryset_scope_where();
        assert!(scope.contains("NOT users.is_bot"));
        assert!(!scope.contains("is_active"), "get_queryset omits it");
        let list_scope = member_list_scope_where();
        assert!(list_scope.contains("project_members.is_active"));
        assert!(list_scope.contains("member_workspace_members.is_active"));
        assert!(list_scope.contains("member_workspaces.slug = :slug"));
        let search_sql = member_search_sql();
        assert!(search_sql.contains("UPPER(users.display_name::text) LIKE UPPER(:term)"));
        assert!(search_sql.contains("OR UPPER(users.first_name::text) LIKE UPPER(:term)"));
        assert_eq!(MEMBER_LIST_FIELDS, &["id", "member", "role"]);
    }

    #[test]
    fn invite_querysets() {
        let fx = fixture();
        let sql = section_sql(&fx, "invite_get_queryset");
        assert!(sql.contains("\"project_member_invites\".\"project_id\" = "));
        assert!(sql.contains("\"workspaces\".\"slug\" = fx06w-6d98814a"));
        assert!(sql.contains("ORDER BY \"project_member_invites\".\"created_at\" DESC"));

        let user_sql = section_sql(&fx, "user_invite_get_queryset");
        assert!(user_sql.contains("\"project_member_invites\".\"email\" = "));
        assert!(
            !user_sql.contains("\"workspaces\".\"slug\" = "),
            "no slug guard"
        );
        assert!(user_invite_scope_where().contains(":user_email"));
        assert!(!user_invite_scope_where().contains(":slug"));
        assert_eq!(INVITE_SELECT_RELATED.len(), 3);
        assert_eq!(USER_INVITE_SELECT_RELATED.len(), 3);
    }

    #[test]
    fn favorites_queryset() {
        let fx = fixture();
        assert!(section(&fx, "favorites_serializer_class").is_null());
        assert!(FAVORITE_SERIALIZER_CLASS.is_none());
        let sql = section_sql(&fx, "favorites_get_queryset");
        assert!(sql.contains("\"user_favorites\".\"user_id\" = "));
        assert!(sql.contains("\"workspaces\".\"slug\" = fx06w-6d98814a"));
        assert!(sql.contains("LEFT OUTER JOIN \"projects\""));
        assert!(sql.contains("ORDER BY \"user_favorites\".\"created_at\" DESC"));
        let scope = favorite_scope_where();
        assert!(scope.contains("user_favorites.user_id = :user"));
        assert_eq!(FAVORITE_SELECT_RELATED.len(), 5);
    }

    #[test]
    fn roles_values_and_fold() {
        let fx = fixture();
        let sql = section_sql(&fx, "roles_values");
        assert!(sql.contains("\"project_members\".\"project_id\""));
        assert!(sql.contains("\"project_members\".\"role\""));
        // Both workspace joins carry the slug predicate ...
        assert!(sql.contains("\"workspaces\".\"slug\" = fx06w-6d98814a"));
        assert!(sql.contains("T5.\"slug\" = fx06w-6d98814a"));
        // ... and the joined workspace_members rows carry no soft-delete
        // guard (ported bug 9).
        assert!(!sql.contains("\"workspace_members\".\"deleted_at\""));
        assert!(sql.contains("ORDER BY \"project_members\".\"created_at\" DESC"));

        assert_eq!(ROLES_VALUES, &["project_id", "role"]);
        let rows: Vec<(String, i32)> = section(&fx, "roles_values")["rows"]
            .as_array()
            .expect("rows")
            .iter()
            .map(|r| {
                (
                    r["project_id"].as_str().expect("id").to_owned(),
                    r["role"].as_i64().expect("role") as i32,
                )
            })
            .collect();
        let folded = roles_fold(&rows);
        assert_eq!(folded.len(), 1);
        assert_eq!(folded[rows[0].0.as_str()], 20);
    }

    #[test]
    fn state_queryset_and_managers() {
        let fx = fixture();
        let sql = section_sql(&fx, "state_get_queryset");
        assert!(sql.starts_with("SELECT DISTINCT"));
        assert!(sql.contains("NOT (\"states\".\"group\" = triage)"));
        assert!(sql.contains("NOT \"states\".\"is_triage\""));
        assert!(sql.contains("\"projects\".\"archived_at\" IS NULL"));
        assert!(sql.contains("\"project_members\".\"member_id\" = "));
        assert!(sql.contains("\"states\".\"project_id\" = "));
        assert!(sql.contains("ORDER BY \"states\".\"sequence\" ASC"));
        // Joined membership rows carry no soft-delete guard (ported bug 9).
        assert!(!sql.contains("\"project_members\".\"deleted_at\""));
        const { assert!(STATE_DISTINCT) };

        let managers = section(&fx, "state_managers");
        assert_eq!(managers["objects_count"], Value::from(1));
        assert_eq!(managers["all_count"], Value::from(2));
        assert_eq!(managers["triage_count"], Value::from(1));
        assert!(managers["objects_sql"]
            .as_str()
            .expect("str")
            .contains("NOT (\"states\".\"group\" = triage)"));
        assert!(managers["triage_sql"]
            .as_str()
            .expect("str")
            .contains("\"states\".\"group\" = triage"));
        assert!(state_scope_where().contains("NOT states.is_triage"));
        assert!(state_objects_where().contains("NOT (states.group = 'triage')"));
        assert!(state_triage_objects_where().contains("states.group = 'triage'"));
        assert!(STATE_ALL_OBJECTS_WHERE.is_empty());
    }

    #[test]
    fn estimate_list() {
        let fx = fixture();
        let est = section(&fx, "estimate_list");
        let lookups: Vec<&str> = est["prefetch_lookups"]
            .as_array()
            .expect("array")
            .iter()
            .map(|v| v.as_str().expect("str"))
            .collect();
        assert_eq!(lookups, ESTIMATE_PREFETCH);
        let sql = est["sql"].as_str().expect("sql");
        assert!(sql.contains("\"estimates\".\"project_id\" = "));
        assert!(sql.contains("\"workspaces\".\"slug\" = fx06w-6d98814a"));
        assert!(sql.contains("ORDER BY \"estimates\".\"name\" ASC"));
        assert_eq!(ESTIMATE_SELECT_RELATED, &["workspace", "project"]);
        let point_scope = estimate_point_scope_where();
        for needle in [
            "estimate_points.estimate_id = :estimate_id",
            "estimate_points.project_id = :project_id",
            "workspaces.slug = :slug",
        ] {
            assert!(point_scope.contains(needle), "missing {needle}");
        }
    }

    #[test]
    fn resolve_paths_and_index_usage() {
        let fx = fixture();
        let uuid_sql = section(&fx, "resolve_sql_uuid_path").as_str().expect("str");
        assert!(uuid_sql.contains("\"projects\".\"id\" = "));
        assert!(uuid_sql.contains("::uuid"));
        assert_eq!(
            uuid_sql
                .matches("\"projects\".\"deleted_at\" IS NULL")
                .count(),
            2,
            "doubled guard: manager + explicit filter"
        );
        assert!(uuid_sql.contains("JOIN \"workspaces\""));
        assert!(uuid_sql.contains("ORDER BY \"projects\".\"created_at\" DESC"));

        let ident_sql = section(&fx, "resolve_sql_identifier_path")
            .as_str()
            .expect("str");
        // The bound literal is already upper-cased ...
        assert!(ident_sql.contains("\"projects\".\"identifier\" = 'P6D98814'"));
        // ... and the column side carries no UPPER(), so the composite
        // btree serves the lookup (same index usage is Done-when).
        assert!(!ident_sql.contains("UPPER(\"projects\".\"identifier\")"));
        assert!(!ident_sql.contains("UPPER(identifier)"));
        assert_eq!(
            ident_sql
                .matches("\"projects\".\"deleted_at\" IS NULL")
                .count(),
            2
        );

        let uuid_built = resolve_uuid_sql();
        assert!(uuid_built.contains("projects.id = :value"));
        assert_eq!(uuid_built.matches("projects.deleted_at IS NULL").count(), 2);
        assert!(uuid_built.ends_with("LIMIT 1"));
        let ident_built = resolve_identifier_sql();
        assert!(ident_built.contains("projects.identifier = :identifier"));
        assert!(!ident_built.contains("UPPER("));
        assert_eq!(
            RESOLVE_IDENTIFIER_INDEX,
            "project_unique_identifier_workspace_when_deleted_at_null"
        );
    }

    #[test]
    fn rewrite_routing_table() {
        // project_id wins whenever present and not None.
        assert_eq!(
            rewrite_target(true, false, "anything"),
            Some(RewriteTarget::ProjectId)
        );
        assert_eq!(
            rewrite_target(true, true, "project"),
            Some(RewriteTarget::ProjectId)
        );
        // pk only under url_name == "project".
        assert_eq!(
            rewrite_target(false, true, "project"),
            Some(RewriteTarget::Pk)
        );
        assert_eq!(rewrite_target(false, true, "project-member"), None);
        assert_eq!(rewrite_target(false, false, "project"), None);

        assert!(should_attempt_rewrite(true, Some("ws")));
        assert!(!should_attempt_rewrite(false, Some("ws")));
        assert!(!should_attempt_rewrite(true, None));
        assert!(!should_attempt_rewrite(true, Some("")));

        assert!(is_uuid_like("c6637f9a-fdbe-4e49-960b-6d06d2b6d26e"));
        assert!(is_uuid_like("c6637f9afdbe4e49960b6d06d2b6d26e"));
        assert!(!is_uuid_like("ENG"));
        assert!(!is_uuid_like(""));
        assert!(!is_uuid_like("not-a-uuid"));

        assert_eq!(normalize_resolve_identifier("  eng "), "ENG");
        const { assert!(REWRITE_WRITES_VIEW_KWARGS) };
        assert_eq!(REWRITE_PK_URL_NAME, "project");
    }

    #[test]
    fn resolve_detail_matches_l5() {
        // Single source of truth stays in L5; this layer mirrors it.
        assert_eq!(
            RESOLVE_NOT_FOUND_DETAIL,
            pidash_db::app_project::models::project::NOT_FOUND_DETAIL
        );
        assert_eq!(
            normalize_resolve_identifier("  p6d98814 "),
            pidash_db::app_project::models::project::normalize_identifier("  p6d98814 ")
        );
        let body: Value = serde_json::from_str(RESOLVE_NOT_FOUND_BODY).expect("body is JSON");
        assert_eq!(body["Detail"], Value::from("Project not found"));
    }

    #[test]
    fn scoping_pagination_helpers() {
        assert_eq!(GUEST_ROLE, 5);
        assert_eq!(MEMBER_ROLE, 15);
        assert_eq!(PUBLIC_NETWORK, 2);
        let probe = workspace_role_probe_sql(GUEST_ROLE);
        assert!(probe.contains("wm.role = 5"));
        assert!(probe.contains("w.slug = :slug"));
        let guest = guest_scope_where();
        assert!(guest.contains("pm.member_id = :user"));
        assert!(guest.contains("pm.is_active"));
        let member = member_scope_where();
        assert!(member.contains(&guest));
        assert!(member.contains("OR projects.network = 2"));

        assert_eq!(PAGINATE_GATE_PARAMS, &["per_page", "cursor"]);
        assert_eq!(PAGINATE_DEFAULT_ORDER_PARAM, "-created_at");
        assert_eq!(
            paginate_order_sql(true, "created_at"),
            "created_at DESC NULLS LAST, projects.created_at DESC"
        );
        assert_eq!(
            paginate_order_sql(false, "name"),
            "name ASC NULLS LAST, projects.created_at DESC"
        );
        assert_eq!(LIST_DETAIL_ORDER_SQL, "sort_order ASC, projects.name ASC");
        assert_eq!(split_fields_param("a,,b"), vec!["a", "b"]);
        assert!(split_fields_param("").is_empty());
        assert_eq!(PAGINATE_ENVELOPE_KEYS.len(), 12);
        assert_eq!(PAGINATE_ENVELOPE_KEYS[11], "results");
        assert_eq!(CURSOR_FORMAT, "value:offset:is_prev");
        assert_eq!(CURSOR_PARAM, "cursor");
        assert_eq!(PAGINATE_DEFAULT_PER_PAGE, 1000);
        assert_eq!(PAGINATE_MAX_PER_PAGE, 1000);
    }
}

#[cfg(test)]
mod pidashconv_736_tests {
    use super::normalize_resolve_identifier;

    #[test]
    fn resolve_identifier_strips_py_whitespace() {
        assert_eq!(normalize_resolve_identifier("  eng "), "ENG");
        // Python `str.strip()` also strips U+001C-U+001F (PIDASHCONV-736).
        for sep in ['\u{1c}', '\u{1d}', '\u{1e}', '\u{1f}'] {
            let padded = format!("{sep}eng{sep}");
            assert_eq!(
                normalize_resolve_identifier(&padded),
                "ENG",
                "U+{:04X} padding must strip like Python",
                sep as u32
            );
        }
        // TAB and U+0085 padding already matched Django; pin the behavior.
        assert_eq!(normalize_resolve_identifier("\teng\t"), "ENG");
        assert_eq!(normalize_resolve_identifier("\u{85}eng\u{85}"), "ENG");
        // Same expression as the db helper; pin them equal.
        assert_eq!(
            normalize_resolve_identifier("\u{1c}p6d98814\u{1c}"),
            pidash_db::app_project::models::project::normalize_identifier("\u{1c}p6d98814\u{1c}")
        );
    }
}
