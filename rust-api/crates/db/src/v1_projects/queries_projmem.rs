//! Project / member / invite / user querysets (D-19, stage 5).
//!
//! Ports the read querysets behind the api-v1 project, member, invite and
//! user routes to SQL-identical builders and executors, recorded in
//! `rust-api/fixtures/v1_projects/queries/project_member.sql` (FX-Q-PROJMEM)
//! with result shapes in `project_member.rows.json`:
//!
//! * Q1 project list/detail base (`api/views/project.py:82-140` list,
//!   `:297-355` detail): workspace scoping + member scoping
//!   (`project_projectmember__member = user AND is_active OR network = 2`)
//!   with the six annotations `is_member`, `total_members`, `total_cycles`,
//!   `total_modules`, `member_role`, `is_deployed`. The detail base adds the
//!   `select_related("workspace", "workspace__owner", "default_assignee",
//!   "project_lead")` joins; the list base selects `project_lead` only.
//!   Both order by `kwargs order_by` default `-created_at` (always the
//!   default: `BaseAPIView` never sets the kwarg).
//! * Q2 list-GET variant (`:169-187`): the `sort_order` subquery annotation,
//!   the `project_projectmember` prefetch
//!   (`workspace__slug = slug, is_active`, `select_related("member")`),
//!   and re-ordering by `request.GET order_by` default `'sort_order'`.
//! * Q3 workspace members (`api/views/member.py:76-82`; 400 when the slug is
//!   unknown).
//! * Q4 project member ids + users (`:135-141`).
//! * Q5 project member detail + user (`:189-192,204,219`; `.get()` misses
//!   become 404 through the base exception handler).
//! * Q6 invite queryset + object (`api/views/invite.py:36-40`; `get_object`
//!   adds the pk predicate).
//! * Q7 current user (`api/views/user.py:40`): no query — the view
//!   serializes `request.user` from authentication. There is deliberately no
//!   builder for it here.
//!
//! Identifier-vs-uuid resolution (`Project.resolve`,
//! `db/models/project.py:191-224`, plus the `_rewrite_project_kwarg`
//! middleware, `api/views/base.py:52-104`) reuses the classification in
//! [`super::models::project`]: [`resolve_project_sql`] picks the pk or the
//! identifier branch, [`fetch_project_id`] runs it. A UUID-looking input is
//! never tried as an identifier and non-UUID input is never tried as a pk;
//! no row means the caller 404s with the generic
//! [`super::models::project::NOT_FOUND_DETAIL`] (the input is never echoed).
//!
//! Every builder and fetch takes a [`TenantScope`]: the workspace slug from
//! the URL kwargs plus the acting user id (`self.kwargs.get("slug")` and
//! `request.user`). There is no unscoped entry point — the write-path
//! [`crate::context::RequestContext`] counterpart carries the same tenant for
//! writes; reads scope by slug + user exactly as the Django views do.
//!
//! Static statements are string constants matching the fixture text with
//! Postgres `$N` binds where the fixture writes `%(name)s` (`$1` slug,
//! `$2` user, unless noted). Only the `IN (...)` arity (Q4) and the `ORDER
//! BY` key are builder functions. SQL execution uses runtime `sqlx::query`
//! (no `query!` macros: no build-time database, same as the merged
//! `license/queries` precedent).
//!
//! # Ported bugs and quirks (translate, don't redesign)
//!
//! * `member_role` / `sort_order` are bare scalar subqueries with no
//!   `LIMIT 1`: a second active membership row fails the query instead of
//!   picking a row, mirroring Django's `MultipleObjectsReturned` 500. The
//!   partial unique index
//!   (`project_member_unique_project_member_when_deleted_at_null`) is what
//!   keeps this to one row.
//! * `total_members` counts non-bot active memberships with no
//!   `users.deleted_at` predicate — exactly as the fixture records it.
//!   Django's stock `UserManager` adds no soft-delete scope anywhere, so the
//!   `u.deleted_at IS NULL` guards in Q3/Q4 below are fixture-pinned shapes
//!   (kept verbatim; flagged for the domain gate to confirm against live
//!   Django).
//! * Q4 with zero member ids issues no query and returns `[]`, mirroring
//!   Django's `id__in=[]` empty-result short-circuit.
//! * Q4 keeps the `JOIN workspaces` + slug predicate from the fixture;
//!   `member_id` NULLs (the FK is nullable) are dropped before the users
//!   query, mirroring `id__in` semantics.
//! * The Q2 prefetch scopes by `workspace__slug`, not by project: Django
//!   matches prefetched rows to projects in Python. Handler-layer ports must
//!   do the same grouping.
//! * The invite list carries no `ORDER BY` (fixture Q6); row order is the
//!   plan order, as in Django.
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

use sqlx::postgres::PgRow;
use sqlx::Row;

use super::models::project::{classify_lookup, ProjectLookup};

// ---------------------------------------------------------------------------
// Tenant scope
// ---------------------------------------------------------------------------

/// Explicit per-request tenant scope for every query in this module.
///
/// `workspace_slug` is the URL `slug` kwarg (`self.kwargs.get("slug")`);
/// `actor_id` is `request.user.id`. Binding both on every statement is what
/// keeps the member scoping (`vis` join), the `is_member` / `member_role` /
/// `sort_order` subqueries and the workspace predicates tenant-correct.
#[derive(Debug, Clone, Copy)]
pub struct TenantScope<'a> {
    pub workspace_slug: &'a str,
    pub actor_id: uuid::Uuid,
}

impl<'a> TenantScope<'a> {
    pub fn new(workspace_slug: &'a str, actor_id: uuid::Uuid) -> Self {
        Self {
            workspace_slug,
            actor_id,
        }
    }
}

// ---------------------------------------------------------------------------
// Q1 project list / detail base
// ---------------------------------------------------------------------------

/// Q1 list base, no `ORDER BY` (the builder appends it).
///
/// Fixture Q1 (`views/project.py:82-140`) with `$1` = slug, `$2` = user.
/// `select_related("project_lead")` is the `LEFT OUTER JOIN` (the FK is
/// nullable); it selects no extra columns here because the fixture pins
/// `SELECT p.*`.
pub const PROJECT_LIST_BASE_SQL: &str = "SELECT p.*,
  EXISTS(SELECT 1 FROM project_members pm
         WHERE pm.member_id = $2 AND pm.project_id = p.id
           AND pm.workspace_id = w.id AND pm.is_active AND pm.deleted_at IS NULL
        ) AS is_member,
  (SELECT COUNT(*) FROM project_members pm
    JOIN users u ON u.id = pm.member_id
    WHERE pm.project_id = p.id AND u.is_bot = FALSE
      AND pm.is_active AND pm.deleted_at IS NULL) AS total_members,
  (SELECT COUNT(*) FROM cycles c
    WHERE c.project_id = p.id AND c.deleted_at IS NULL) AS total_cycles,
  (SELECT COUNT(*) FROM modules m
    WHERE m.project_id = p.id AND m.deleted_at IS NULL) AS total_modules,
  (SELECT pm.role FROM project_members pm
    WHERE pm.project_id = p.id AND pm.member_id = $2
      AND pm.is_active AND pm.deleted_at IS NULL) AS member_role,
  EXISTS(SELECT 1 FROM deploy_boards d
         WHERE d.project_id = p.id AND d.workspace_id = w.id
           AND d.deleted_at IS NULL) AS is_deployed
FROM projects p
JOIN workspaces w ON w.id = p.workspace_id
LEFT OUTER JOIN users project_lead ON p.project_lead_id = project_lead.id
LEFT OUTER JOIN project_members vis
  ON (vis.project_id = p.id AND vis.member_id = $2 AND vis.is_active
      AND vis.deleted_at IS NULL)
WHERE w.slug = $1 AND p.deleted_at IS NULL
  AND (vis.id IS NOT NULL OR p.network = 2)
GROUP BY p.id, w.id";

/// Q1 detail base, no `ORDER BY` (`views/project.py:297-355`).
///
/// Same annotations and scoping as the list base; `select_related`
/// adds the workspace-owner (`INNER`, required FK), default-assignee and
/// project-lead (`LEFT OUTER`, nullable FKs) joins. The workspace join is
/// reused from the slug predicate.
pub const PROJECT_DETAIL_BASE_SQL: &str = "SELECT p.*,
  EXISTS(SELECT 1 FROM project_members pm
         WHERE pm.member_id = $2 AND pm.project_id = p.id
           AND pm.workspace_id = w.id AND pm.is_active AND pm.deleted_at IS NULL
        ) AS is_member,
  (SELECT COUNT(*) FROM project_members pm
    JOIN users u ON u.id = pm.member_id
    WHERE pm.project_id = p.id AND u.is_bot = FALSE
      AND pm.is_active AND pm.deleted_at IS NULL) AS total_members,
  (SELECT COUNT(*) FROM cycles c
    WHERE c.project_id = p.id AND c.deleted_at IS NULL) AS total_cycles,
  (SELECT COUNT(*) FROM modules m
    WHERE m.project_id = p.id AND m.deleted_at IS NULL) AS total_modules,
  (SELECT pm.role FROM project_members pm
    WHERE pm.project_id = p.id AND pm.member_id = $2
      AND pm.is_active AND pm.deleted_at IS NULL) AS member_role,
  EXISTS(SELECT 1 FROM deploy_boards d
         WHERE d.project_id = p.id AND d.workspace_id = w.id
           AND d.deleted_at IS NULL) AS is_deployed
FROM projects p
JOIN workspaces w ON w.id = p.workspace_id
INNER JOIN users workspace_owner ON w.owner_id = workspace_owner.id
LEFT OUTER JOIN users default_assignee ON p.default_assignee_id = default_assignee.id
LEFT OUTER JOIN users project_lead ON p.project_lead_id = project_lead.id
LEFT OUTER JOIN project_members vis
  ON (vis.project_id = p.id AND vis.member_id = $2 AND vis.is_active
      AND vis.deleted_at IS NULL)
WHERE w.slug = $1 AND p.deleted_at IS NULL
  AND (vis.id IS NOT NULL OR p.network = 2)
GROUP BY p.id, w.id";

/// Base `ORDER BY` (`kwargs order_by`, always the `-created_at` default).
pub const ORDER_BASE: &str = "ORDER BY p.created_at DESC";

/// List base with its default ordering (`views/project.py:138`).
pub fn project_list_sql() -> String {
    format!("{PROJECT_LIST_BASE_SQL}\n{ORDER_BASE}")
}

/// Detail base with its default ordering (`views/project.py:353`).
pub fn project_detail_sql() -> String {
    format!("{PROJECT_DETAIL_BASE_SQL}\n{ORDER_BASE}")
}

// ---------------------------------------------------------------------------
// Q2 list-GET sort_order variant + ordering
// ---------------------------------------------------------------------------

/// The `sort_order` annotation the list GET adds
/// (`views/project.py:169-174`): the caller's own active membership row in
/// this workspace, or NULL when the caller holds none (scalar subquery with
/// zero rows — never absent).
pub const SORT_ORDER_ANNOTATION_SQL: &str = "(SELECT pm.sort_order FROM project_members pm
              WHERE pm.member_id = $2 AND pm.project_id = p.id
                AND pm.workspace_id = w.id AND pm.is_active
                AND pm.deleted_at IS NULL) AS sort_order";

/// Default GET ordering (`request.GET order_by`, `:186`).
pub const ORDER_GET_DEFAULT: &str = "sort_order";

/// Map a GET `order_by` key to its `ORDER BY` fragment.
///
/// Django resolves field names against the queryset (annotations included);
/// unknown keys fail in the view, so the handlers validate there. Unknown
/// keys here fall back to the GET default rather than emitting SQL for an
/// unmapped identifier.
pub fn order_by_sql(key: &str) -> &'static str {
    match key {
        "sort_order" => "ORDER BY sort_order",
        "-sort_order" => "ORDER BY sort_order DESC",
        "created_at" => "ORDER BY p.created_at ASC",
        "-created_at" => "ORDER BY p.created_at DESC",
        "updated_at" => "ORDER BY p.updated_at ASC",
        "-updated_at" => "ORDER BY p.updated_at DESC",
        "name" => "ORDER BY p.name ASC",
        "-name" => "ORDER BY p.name DESC",
        _ => "ORDER BY sort_order",
    }
}

/// Full list-GET statement: list base + `sort_order` annotation + GET
/// ordering (`views/project.py:169-187`).
pub fn project_list_get_sql(order_by: &str) -> String {
    let select_from = "FROM projects p";
    let with_annotation = PROJECT_LIST_BASE_SQL.replacen(
        select_from,
        &format!(",\n  {SORT_ORDER_ANNOTATION_SQL}\n{select_from}"),
        1,
    );
    format!("{with_annotation}\n{}", order_by_sql(order_by))
}

/// Member prefetch for the projects on the page (`:178-185`).
///
/// `$1` is the workspace id. Django filters `workspace__slug = slug`; the
/// fixture pins the equivalent `workspace_id` predicate, so callers resolve
/// the id once ([`WORKSPACE_ID_FOR_SLUG_SQL`]) and reuse it.
pub const PROJECT_MEMBER_PREFETCH_SQL: &str = "SELECT pm.*, u.* FROM project_members pm
LEFT OUTER JOIN users u ON pm.member_id = u.id
WHERE pm.workspace_id = $1 AND pm.is_active AND pm.deleted_at IS NULL";

// ---------------------------------------------------------------------------
// Q3 workspace members
// ---------------------------------------------------------------------------

/// Slug existence check (`views/member.py:76`).
///
/// Django's `exists()` renders `SELECT 1 ... LIMIT 1`; the fixture pins the
/// unquoted shape. `$1` = slug.
pub const WORKSPACE_EXISTS_SQL: &str = "SELECT 1 FROM workspaces WHERE slug = $1 LIMIT 1";

/// Workspace id lookup for a slug.
///
/// Folds Django's `exists()` check and the `workspace__slug` join target
/// into one lookup: the fixture Q3 exists-check carries no `deleted_at`
/// predicate, while every manager read scopes live rows, so this filters
/// `deleted_at IS NULL` (a soft-deleted workspace reads as missing, exactly
/// what the 400 branch needs). No row means the caller returns
/// `{"error": "Provided workspace does not exist"}` with 400
/// (`member.py:76-80,128-132,182-186`).
pub const WORKSPACE_ID_FOR_SLUG_SQL: &str =
    "SELECT id FROM workspaces WHERE slug = $1 AND deleted_at IS NULL LIMIT 1";

/// Workspace members with their users (`views/member.py:82`).
///
/// `$1` is the workspace id (see [`WORKSPACE_ID_FOR_SLUG_SQL`]).
/// `select_related("member")` is the `JOIN users`.
pub const WORKSPACE_MEMBERS_SQL: &str = "SELECT wm.*, u.* FROM workspace_members wm
JOIN users u ON u.id = wm.member_id
WHERE wm.workspace_id = $1 AND wm.deleted_at IS NULL AND u.deleted_at IS NULL";

/// Whether the workspace slug exists (the `exists()` branch).
pub async fn fetch_workspace_exists<'e, E>(
    ex: E,
    scope: &TenantScope<'_>,
) -> Result<bool, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let row: Option<PgRow> = sqlx::query(WORKSPACE_EXISTS_SQL)
        .bind(scope.workspace_slug)
        .fetch_optional(ex)
        .await?;
    Ok(row.is_some())
}

/// Workspace id for the scope slug, or `None` (the 400 branch).
pub async fn fetch_workspace_id<'e, E>(
    ex: E,
    scope: &TenantScope<'_>,
) -> Result<Option<uuid::Uuid>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let row: Option<PgRow> = sqlx::query(WORKSPACE_ID_FOR_SLUG_SQL)
        .bind(scope.workspace_slug)
        .fetch_optional(ex)
        .await?;
    row.map(|r| r.try_get("id")).transpose()
}

// ---------------------------------------------------------------------------
// Q4 project member ids + users
// ---------------------------------------------------------------------------

/// Project member ids (`views/member.py:135-137`).
///
/// `values_list("member_id", flat=True)` selects the bare column; `$1` is
/// the project id, `$2` the workspace slug.
pub const PROJECT_MEMBER_IDS_SQL: &str = "SELECT pm.member_id FROM project_members pm
JOIN workspaces w ON w.id = pm.workspace_id
WHERE pm.project_id = $1 AND w.slug = $2
  AND pm.deleted_at IS NULL";

/// Users for an id list (`views/member.py:140`).
///
/// `n` is the number of `$N` placeholders. Use [`project_users_sql`].
pub fn project_users_sql(n: usize) -> String {
    let list: Vec<String> = (1..=n).map(|i| format!("${i}")).collect();
    format!(
        "SELECT u.* FROM users u WHERE u.id IN ({}) AND u.deleted_at IS NULL",
        list.join(", ")
    )
}

/// Member ids for a project, NULLs dropped.
///
/// `member` is nullable; `id__in` never matches NULL, so NULLs are filtered
/// before the users query.
pub async fn fetch_project_member_ids<'e, E>(
    ex: E,
    scope: &TenantScope<'_>,
    project_id: uuid::Uuid,
) -> Result<Vec<uuid::Uuid>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let rows: Vec<PgRow> = sqlx::query(PROJECT_MEMBER_IDS_SQL)
        .bind(project_id)
        .bind(scope.workspace_slug)
        .fetch_all(ex)
        .await?;
    rows.iter()
        .map(|r| r.try_get::<Option<uuid::Uuid>, _>("member_id"))
        .filter_map(|id| id.transpose())
        .collect()
}

/// Users for a member-id list; empty in, empty out with no query.
///
/// Mirrors Django's `id__in=[]` empty-result short-circuit: no statement is
/// issued for an empty list.
pub async fn fetch_project_users<'e, E>(
    ex: E,
    ids: &[uuid::Uuid],
) -> Result<Vec<MemberUserLite>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let sql = project_users_sql(ids.len());
    let mut q = sqlx::query(&sql);
    for id in ids {
        q = q.bind(*id);
    }
    let rows = q.fetch_all(ex).await?;
    rows.iter().map(map_member_user).collect()
}

// ---------------------------------------------------------------------------
// Q5 project member detail
// ---------------------------------------------------------------------------

/// Project member detail (`.get(project_id, workspace__slug, pk)`,
/// `views/member.py:189,204,219`).
///
/// `$1` project id, `$2` workspace slug, `$3` membership pk. A miss raises
/// `DoesNotExist` in Django and becomes 404 through the base exception
/// handler; here it is `None` and the caller 404s. The fixture writes
/// `SELECT *`; Django enumerates the driving table's columns, so this
/// selects `pm.*` — a bare star would also return `w.*` and duplicate
/// `id`, mapping the workspace id over the membership id.
pub const PROJECT_MEMBER_GET_SQL: &str =
    "SELECT pm.* FROM project_members pm JOIN workspaces w ON w.id = pm.workspace_id
WHERE pm.project_id = $1 AND w.slug = $2 AND pm.id = $3
  AND pm.deleted_at IS NULL";

/// Member user (`User.objects.get(id=...)`, `member.py:190`).
pub const USER_GET_SQL: &str = "SELECT * FROM users WHERE id = $1 AND deleted_at IS NULL";

/// Membership reference for a detail lookup, or `None` (the 404 branch).
pub async fn fetch_project_member<'e, E>(
    ex: E,
    scope: &TenantScope<'_>,
    project_id: uuid::Uuid,
    pk: uuid::Uuid,
) -> Result<Option<ProjectMemberRef>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let row: Option<PgRow> = sqlx::query(PROJECT_MEMBER_GET_SQL)
        .bind(project_id)
        .bind(scope.workspace_slug)
        .bind(pk)
        .fetch_optional(ex)
        .await?;
    row.map(|r| map_project_member_ref(&r)).transpose()
}

/// User by id, or `None` (the 404 branch).
pub async fn fetch_user_by_id<'e, E>(
    ex: E,
    member_id: uuid::Uuid,
) -> Result<Option<MemberUserLite>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let row: Option<PgRow> = sqlx::query(USER_GET_SQL)
        .bind(member_id)
        .fetch_optional(ex)
        .await?;
    row.map(|r| map_member_user(&r)).transpose()
}

// ---------------------------------------------------------------------------
// Q6 invites
// ---------------------------------------------------------------------------

/// Invite list (`views/invite.py:36-37`).
///
/// `BaseViewSet.get_queryset` (default manager scope) filtered on
/// `workspace__slug`; `$1` = slug. Pagination / filter backends apply above
/// this layer. The fixture writes `SELECT *`; Django enumerates the driving
/// table's columns, so this selects `i.*` — a bare star would also return
/// `w.*` and duplicate `id`, mapping the workspace id over the invite id
/// (caught by the live test).
pub const INVITES_LIST_SQL: &str = "SELECT i.* FROM workspace_member_invites i
JOIN workspaces w ON w.id = i.workspace_id
WHERE w.slug = $1 AND i.deleted_at IS NULL";

/// Invite object (`.get(pk)`, `invite.py:40`); a miss is `None` → 404.
pub const INVITE_GET_SQL: &str = "SELECT i.* FROM workspace_member_invites i
JOIN workspaces w ON w.id = i.workspace_id
WHERE w.slug = $1 AND i.deleted_at IS NULL AND i.id = $2";

/// All invites for the scope workspace.
pub async fn fetch_invites<'e, E>(
    ex: E,
    scope: &TenantScope<'_>,
) -> Result<Vec<InviteRow>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let rows: Vec<PgRow> = sqlx::query(INVITES_LIST_SQL)
        .bind(scope.workspace_slug)
        .fetch_all(ex)
        .await?;
    rows.iter().map(map_invite_row).collect()
}

/// One invite by pk, or `None` (the 404 branch).
pub async fn fetch_invite<'e, E>(
    ex: E,
    scope: &TenantScope<'_>,
    pk: uuid::Uuid,
) -> Result<Option<InviteRow>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let row: Option<PgRow> = sqlx::query(INVITE_GET_SQL)
        .bind(scope.workspace_slug)
        .bind(pk)
        .fetch_optional(ex)
        .await?;
    row.map(|r| map_invite_row(&r)).transpose()
}

// ---------------------------------------------------------------------------
// Identifier-vs-uuid resolution
// ---------------------------------------------------------------------------

/// `Project.resolve` pk branch (`db/models/project.py:199`): `$1` slug,
/// `$2` uuid. UUID-looking input is never tried as an identifier.
pub const RESOLVE_BY_PK_SQL: &str = "SELECT p.id FROM projects p
JOIN workspaces w ON w.id = p.workspace_id
WHERE w.slug = $1 AND p.id = $2 AND p.deleted_at IS NULL LIMIT 1";

/// `Project.resolve` identifier branch (`:204-212`): `$1` slug,
/// `$2` stripped upper-cased identifier with exact equality (never
/// `iexact`, so the composite btree is used).
pub const RESOLVE_BY_IDENTIFIER_SQL: &str = "SELECT p.id FROM projects p
JOIN workspaces w ON w.id = p.workspace_id
WHERE w.slug = $1 AND p.identifier = $2 AND p.deleted_at IS NULL LIMIT 1";

/// Pick the resolve statement for raw input, classifying exactly like
/// `Project.resolve` (`uuid.UUID(str(value))` → pk, `ValueError` /
/// `AttributeError` / `TypeError` → identifier).
pub fn resolve_project_sql(raw: &str) -> &'static str {
    match classify_lookup(raw) {
        ProjectLookup::Pk(_) => RESOLVE_BY_PK_SQL,
        ProjectLookup::Identifier(_) => RESOLVE_BY_IDENTIFIER_SQL,
    }
}

/// Resolve raw project input (uuid or identifier) to the project id.
///
/// Returns `None` when nothing matches; the caller raises the generic 404
/// (`Project not found`, input never echoed).
pub async fn fetch_project_id<'e, E>(
    ex: E,
    scope: &TenantScope<'_>,
    raw: &str,
) -> Result<Option<uuid::Uuid>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let row: Option<PgRow> = match classify_lookup(raw) {
        ProjectLookup::Pk(id) => {
            sqlx::query(RESOLVE_BY_PK_SQL)
                .bind(scope.workspace_slug)
                .bind(id)
                .fetch_optional(ex)
                .await?
        }
        ProjectLookup::Identifier(name) => {
            sqlx::query(RESOLVE_BY_IDENTIFIER_SQL)
                .bind(scope.workspace_slug)
                .bind(name)
                .fetch_optional(ex)
                .await?
        }
    };
    row.map(|r| r.try_get("id")).transpose()
}

// ---------------------------------------------------------------------------
// Rows
// ---------------------------------------------------------------------------

/// The six Q1 annotations plus the project identity columns the fixture
/// rows pin (`project_member.rows.json`).
///
/// `sort_order` is `Some` only on the list-GET statement (Q2); on the base
/// statements the column is absent and reads as `None` — mirroring the
/// fixture note that detail responses carry no `sort_order` key from the
/// queryset. `member_role` is NULL (not absent) without a membership row.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectAnnotations {
    pub id: uuid::Uuid,
    pub name: String,
    pub identifier: String,
    pub is_member: bool,
    pub total_members: i64,
    pub total_cycles: i64,
    pub total_modules: i64,
    pub member_role: Option<i32>,
    pub is_deployed: bool,
    pub sort_order: Option<f64>,
}

/// Map the annotation columns of a Q1/Q2 row (column names,
/// order-independent).
pub fn map_project_annotations(row: &PgRow) -> Result<ProjectAnnotations, sqlx::Error> {
    let sort_order = match row.try_get::<Option<f64>, _>("sort_order") {
        Ok(v) => v,
        Err(sqlx::Error::ColumnNotFound(_)) => None,
        Err(e) => return Err(e),
    };
    // `pm.role` is smallint (INT2); sqlx does not widen INT2 into i32.
    let member_role: Option<i16> = row.try_get("member_role")?;
    Ok(ProjectAnnotations {
        id: row.try_get("id")?,
        name: row.try_get("name")?,
        identifier: row.try_get("identifier")?,
        is_member: row.try_get("is_member")?,
        total_members: row.try_get("total_members")?,
        total_cycles: row.try_get("total_cycles")?,
        total_modules: row.try_get("total_modules")?,
        member_role: member_role.map(i32::from),
        is_deployed: row.try_get("is_deployed")?,
        sort_order,
    })
}

/// Annotated project rows for the list base statement.
pub async fn fetch_project_list<'e, E>(
    ex: E,
    scope: &TenantScope<'_>,
) -> Result<Vec<ProjectAnnotations>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let rows: Vec<PgRow> = sqlx::query(&project_list_sql())
        .bind(scope.workspace_slug)
        .bind(scope.actor_id)
        .fetch_all(ex)
        .await?;
    rows.iter().map(map_project_annotations).collect()
}

/// Annotated project rows for the list-GET statement (with `sort_order`).
pub async fn fetch_project_list_get<'e, E>(
    ex: E,
    scope: &TenantScope<'_>,
    order_by: &str,
) -> Result<Vec<ProjectAnnotations>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let rows: Vec<PgRow> = sqlx::query(&project_list_get_sql(order_by))
        .bind(scope.workspace_slug)
        .bind(scope.actor_id)
        .fetch_all(ex)
        .await?;
    rows.iter().map(map_project_annotations).collect()
}

/// Annotated project rows for the detail base statement (no `sort_order`).
pub async fn fetch_project_detail<'e, E>(
    ex: E,
    scope: &TenantScope<'_>,
) -> Result<Vec<ProjectAnnotations>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let rows: Vec<PgRow> = sqlx::query(&project_detail_sql())
        .bind(scope.workspace_slug)
        .bind(scope.actor_id)
        .fetch_all(ex)
        .await?;
    rows.iter().map(map_project_annotations).collect()
}

/// One workspace membership with its workspace (`member.py:86-89` loop shape:
/// user data plus `role`).
#[derive(Debug, Clone, PartialEq)]
pub struct WorkspaceMemberRow {
    pub member_id: uuid::Uuid,
    pub role: i32,
    pub workspace_id: uuid::Uuid,
}

/// Map one Q3 row.
pub fn map_workspace_member(row: &PgRow) -> Result<WorkspaceMemberRow, sqlx::Error> {
    // `workspace_members.role` is smallint (INT2); widen to i32.
    let role: i16 = row.try_get("role")?;
    Ok(WorkspaceMemberRow {
        member_id: row.try_get("member_id")?,
        role: i32::from(role),
        workspace_id: row.try_get("workspace_id")?,
    })
}

/// Workspace memberships for a resolved workspace id.
pub async fn fetch_workspace_members<'e, E>(
    ex: E,
    workspace_id: uuid::Uuid,
) -> Result<Vec<WorkspaceMemberRow>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let rows: Vec<PgRow> = sqlx::query(WORKSPACE_MEMBERS_SQL)
        .bind(workspace_id)
        .fetch_all(ex)
        .await?;
    rows.iter().map(map_workspace_member).collect()
}

/// The `UserLiteSerializer` columns the member views embed
/// (`api/serializers/user.py:13-38`: id, names, email, avatar; `avatar_url`
/// and `display_name` are derived properties, not columns).
#[derive(Debug, Clone, PartialEq)]
pub struct MemberUserLite {
    pub id: uuid::Uuid,
    pub first_name: String,
    pub last_name: String,
    pub email: String,
    pub avatar: Option<String>,
}

/// Map one user row.
pub fn map_member_user(row: &PgRow) -> Result<MemberUserLite, sqlx::Error> {
    Ok(MemberUserLite {
        id: row.try_get("id")?,
        first_name: row.try_get("first_name")?,
        last_name: row.try_get("last_name")?,
        email: row.try_get("email")?,
        avatar: row.try_get("avatar")?,
    })
}

/// Membership reference for Q5 detail lookups.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectMemberRef {
    pub id: uuid::Uuid,
    pub project_id: uuid::Uuid,
    pub member_id: Option<uuid::Uuid>,
}

/// Map one Q5 membership row.
pub fn map_project_member_ref(row: &PgRow) -> Result<ProjectMemberRef, sqlx::Error> {
    Ok(ProjectMemberRef {
        id: row.try_get("id")?,
        project_id: row.try_get("project_id")?,
        member_id: row.try_get("member_id")?,
    })
}

/// The invite columns the fixture row pins (`invite.py` serialize shape).
#[derive(Debug, Clone, PartialEq)]
pub struct InviteRow {
    pub id: uuid::Uuid,
    pub email: String,
    pub accepted: bool,
    pub responded_at: Option<chrono::DateTime<chrono::Utc>>,
    pub role: i32,
}

/// Map one Q6 invite row.
pub fn map_invite_row(row: &PgRow) -> Result<InviteRow, sqlx::Error> {
    // `workspace_member_invites.role` is smallint (INT2); widen to i32.
    let role: i16 = row.try_get("role")?;
    Ok(InviteRow {
        id: row.try_get("id")?,
        email: row.try_get("email")?,
        accepted: row.try_get("accepted")?,
        responded_at: row.try_get("responded_at")?,
        role: i32::from(role),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    fn fixtures_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/v1_projects/queries")
    }

    fn read_fixture() -> String {
        let path = fixtures_dir().join("project_member.sql");
        std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read fixture {}: {e}", path.display()))
    }

    /// One statement line cleaned for comparison: strip a leading `--`
    /// marker (the fixture records some statements behind it), cut trailing
    /// `;` and any trailing `--` comment. Leading indentation of real
    /// statement lines is preserved. (`--` never occurs inside the SQL
    /// itself in this fixture.)
    fn clean(line: &str) -> String {
        let t = line.trim_start();
        let (is_comment, body) = match t.strip_prefix("--") {
            Some(rest) => (true, rest),
            None => (false, line),
        };
        let body = match body.find("--") {
            Some(pos) => &body[..pos],
            None => body,
        };
        let body = body.trim_end().trim_end_matches(';').trim_end();
        if is_comment {
            body.trim_start().to_owned()
        } else {
            body.to_owned()
        }
    }

    /// Joining lines `from`..=`to` (inclusive, 0-based) with `\n`.
    fn block(lines: &[&str], from: usize, to: usize) -> String {
        lines[from..=to]
            .iter()
            .map(|l| clean(l))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn fixture_lines() -> Vec<String> {
        read_fixture().lines().map(str::to_owned).collect()
    }

    #[test]
    fn list_base_matches_fixture_q1() {
        let raw = read_fixture();
        let lines: Vec<&str> = raw.lines().collect();
        let from = lines
            .iter()
            .position(|l| l.trim() == "SELECT p.*,")
            .expect("Q1 SELECT p.*,");
        let to = lines
            .iter()
            .position(|l| l.trim() == "GROUP BY p.id, w.id")
            .expect("Q1 GROUP BY");
        let expected = block(&lines, from, to)
            .replace("%(slug)s", "$1")
            .replace("%(user)s", "$2");
        // The base is Q1 plus the `select_related("project_lead")` join
        // Django emits (`views/project.py:92`; nullable FK, so `LEFT
        // OUTER`); the fixture abbreviates the shape to `SELECT p.*` plus
        // the scoping join, so the comparison drops exactly that line.
        let without_lead: String = PROJECT_LIST_BASE_SQL
            .lines()
            .filter(|l| !l.contains("users project_lead"))
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(without_lead, expected);
        assert!(PROJECT_LIST_BASE_SQL
            .contains("LEFT OUTER JOIN users project_lead ON p.project_lead_id = project_lead.id"));
    }

    #[test]
    fn detail_base_shares_q1_annotations_and_scoping() {
        for fragment in [
            ") AS is_member,",
            ") AS total_members,",
            ") AS total_cycles,",
            ") AS total_modules,",
            ") AS member_role,",
            ") AS is_deployed",
            "LEFT OUTER JOIN project_members vis",
            "AND (vis.id IS NOT NULL OR p.network = 2)",
            "GROUP BY p.id, w.id",
        ] {
            assert!(
                PROJECT_DETAIL_BASE_SQL.contains(fragment),
                "detail base misses {fragment}"
            );
        }
        for join in [
            "INNER JOIN users workspace_owner ON w.owner_id = workspace_owner.id",
            "LEFT OUTER JOIN users default_assignee ON p.default_assignee_id = default_assignee.id",
            "LEFT OUTER JOIN users project_lead ON p.project_lead_id = project_lead.id",
        ] {
            assert!(
                PROJECT_DETAIL_BASE_SQL.contains(join),
                "detail base misses select_related join {join}"
            );
        }
        assert!(
            !PROJECT_DETAIL_BASE_SQL.contains("sort_order"),
            "detail base must not annotate sort_order (fixture rows note)"
        );
    }

    #[test]
    fn list_get_adds_sort_order_annotation_and_default_order() {
        let sql = project_list_get_sql("sort_order");
        assert!(sql.contains(SORT_ORDER_ANNOTATION_SQL));
        assert!(sql.ends_with("ORDER BY sort_order"));
        // Unknown keys fall back to the GET default.
        assert!(project_list_get_sql("bogus").ends_with("ORDER BY sort_order"));
        assert!(project_list_get_sql("-created_at").ends_with("ORDER BY p.created_at DESC"));
        // Base statements keep the kwargs default.
        assert!(project_list_sql().ends_with("ORDER BY p.created_at DESC"));
        assert!(project_detail_sql().ends_with("ORDER BY p.created_at DESC"));
    }

    #[test]
    fn order_by_keys_map() {
        assert_eq!(order_by_sql("sort_order"), "ORDER BY sort_order");
        assert_eq!(order_by_sql("-sort_order"), "ORDER BY sort_order DESC");
        assert_eq!(order_by_sql("created_at"), "ORDER BY p.created_at ASC");
        assert_eq!(order_by_sql("-created_at"), "ORDER BY p.created_at DESC");
        assert_eq!(order_by_sql("updated_at"), "ORDER BY p.updated_at ASC");
        assert_eq!(order_by_sql("-updated_at"), "ORDER BY p.updated_at DESC");
        assert_eq!(order_by_sql("name"), "ORDER BY p.name ASC");
        assert_eq!(order_by_sql("-name"), "ORDER BY p.name DESC");
        assert_eq!(order_by_sql(""), "ORDER BY sort_order");
    }

    #[test]
    fn q2_sort_order_annotation_matches_fixture_shape() {
        let raw = read_fixture();
        // The fixture records the annotation behind `--` comment lines.
        for fragment in [
            "SELECT pm.sort_order FROM project_members pm",
            "pm.member_id = %(user)s AND pm.project_id = p.id",
            "pm.workspace_id = w.id AND pm.is_active",
            "pm.deleted_at IS NULL) AS sort_order",
        ] {
            assert!(raw.contains(fragment), "fixture Q2 misses {fragment}");
        }
        let with_binds = SORT_ORDER_ANNOTATION_SQL.replace("$2", "%(user)s");
        for line in with_binds.lines().map(str::trim) {
            assert!(
                raw.contains(line),
                "annotation line not in fixture Q2: {line}"
            );
        }
    }

    #[test]
    fn workspace_queries_match_fixture_q3() {
        let lines = fixture_lines();
        let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
        let exists_at = refs
            .iter()
            .position(|l| clean(l).starts_with("SELECT 1 FROM workspaces"))
            .expect("Q3 exists");
        let members_at = refs
            .iter()
            .position(|l| l.trim_start().starts_with("SELECT wm.*, u.*"))
            .expect("Q3 members");
        let exists_stmt = clean(refs[exists_at]).replace("%(slug)s", "$1");
        assert_eq!(
            WORKSPACE_EXISTS_SQL
                .strip_suffix(" LIMIT 1")
                .expect("exists LIMIT"),
            exists_stmt
        );
        let members_stmt = block(&refs, members_at, members_at + 2).replace("%(ws)s", "$1");
        assert_eq!(WORKSPACE_MEMBERS_SQL, members_stmt);
    }

    #[test]
    fn project_member_queries_match_fixture_q4() {
        let lines = fixture_lines();
        let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
        let ids_at = refs
            .iter()
            .position(|l| l.trim_start().starts_with("SELECT pm.member_id"))
            .expect("Q4 ids");
        let ids_stmt = block(&refs, ids_at, ids_at + 3)
            .replace("%(project)s", "$1")
            .replace("%(slug)s", "$2");
        assert_eq!(PROJECT_MEMBER_IDS_SQL, ids_stmt);
        let users_line = refs
            .iter()
            .find(|l| l.trim_start().starts_with("SELECT u.* FROM users"))
            .expect("Q4 users");
        let raw_line = clean(users_line);
        assert!(
            raw_line.contains("u.id IN (...)"),
            "fixture Q4 IN shape: {raw_line}"
        );
        assert_eq!(
            project_users_sql(2),
            "SELECT u.* FROM users u WHERE u.id IN ($1, $2) AND u.deleted_at IS NULL"
        );
        assert_eq!(
            project_users_sql(1),
            "SELECT u.* FROM users u WHERE u.id IN ($1) AND u.deleted_at IS NULL"
        );
    }

    #[test]
    fn member_detail_queries_match_fixture_q5() {
        let lines = fixture_lines();
        let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
        let get_at = refs
            .iter()
            .position(|l| {
                l.trim_start()
                    .starts_with("SELECT * FROM project_members pm JOIN workspaces")
            })
            .expect("Q5 get");
        let get_stmt = block(&refs, get_at, get_at + 2)
            .replace("%(project)s", "$1")
            .replace("%(slug)s", "$2")
            .replace("%(pk)s", "$3")
            .replacen("SELECT *", "SELECT pm.*", 1);
        assert_eq!(PROJECT_MEMBER_GET_SQL, get_stmt);
        let user_line = refs
            .iter()
            .find(|l| l.trim_start().starts_with("SELECT * FROM users WHERE"))
            .expect("Q5 user");
        assert_eq!(USER_GET_SQL, clean(user_line).replace("%(member)s", "$1"));
    }

    #[test]
    fn invite_queries_match_fixture_q6() {
        let lines = fixture_lines();
        let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
        let list_at = refs
            .iter()
            .position(|l| {
                l.trim_start()
                    .starts_with("SELECT * FROM workspace_member_invites i")
            })
            .expect("Q6 list");
        let list_stmt = block(&refs, list_at, list_at + 2)
            .replace("%(slug)s", "$1")
            .replacen("SELECT *", "SELECT i.*", 1);
        assert_eq!(INVITES_LIST_SQL, list_stmt);
        // get_object adds the pk predicate to the same scope.
        assert_eq!(
            INVITE_GET_SQL,
            format!("{list_stmt} AND i.id = $2").replace("%(slug)s", "$1")
        );
        assert!(INVITE_GET_SQL.contains("AND i.id = $2"));
    }

    #[test]
    fn resolve_sql_branches_match_lookup() {
        assert_eq!(
            resolve_project_sql("11111111-1111-1111-1111-111111111111"),
            RESOLVE_BY_PK_SQL
        );
        assert_eq!(resolve_project_sql("eng"), RESOLVE_BY_IDENTIFIER_SQL);
        assert_eq!(resolve_project_sql(" ENG "), RESOLVE_BY_IDENTIFIER_SQL);
        assert_eq!(resolve_project_sql(""), RESOLVE_BY_IDENTIFIER_SQL);
    }

    // -- live scratch-DB tests (env-gated) -------------------------------

    /// Scratch Postgres for the Done-when verification. Unset (plain
    /// `cargo test`) skips these; CI sets no database either, so the suite
    /// stays green offline. Run with e.g.
    /// `export DATABASE_URL=postgresql://user@host/db` for the real check.
    async fn scratch_pool() -> Option<sqlx::PgPool> {
        match std::env::var("DATABASE_URL") {
            Ok(url) => Some(
                sqlx::PgPool::connect(&url)
                    .await
                    .expect("connect to scratch DATABASE_URL"),
            ),
            Err(_) => {
                eprintln!("skipping live-db test: DATABASE_URL is not set");
                None
            }
        }
    }

    const LIVE_DDL: &[&str] = &[
        "CREATE TEMPORARY TABLE workspaces (id UUID PRIMARY KEY, slug TEXT NOT NULL UNIQUE, owner_id UUID NOT NULL, deleted_at TIMESTAMPTZ)",
        "CREATE TEMPORARY TABLE users (id UUID PRIMARY KEY, first_name TEXT NOT NULL DEFAULT '', last_name TEXT NOT NULL DEFAULT '', email TEXT NOT NULL DEFAULT '', avatar TEXT, is_bot BOOLEAN NOT NULL DEFAULT FALSE, deleted_at TIMESTAMPTZ)",
        "CREATE TEMPORARY TABLE projects (id UUID PRIMARY KEY, workspace_id UUID NOT NULL, name TEXT NOT NULL DEFAULT '', identifier TEXT NOT NULL DEFAULT '', network INTEGER NOT NULL DEFAULT 2, project_lead_id UUID, default_assignee_id UUID, created_at TIMESTAMPTZ NOT NULL DEFAULT now(), updated_at TIMESTAMPTZ NOT NULL DEFAULT now(), deleted_at TIMESTAMPTZ)",
        "CREATE TEMPORARY TABLE project_members (id UUID PRIMARY KEY, project_id UUID NOT NULL, workspace_id UUID NOT NULL, member_id UUID, role SMALLINT NOT NULL DEFAULT 5, sort_order DOUBLE PRECISION NOT NULL DEFAULT 65535, is_active BOOLEAN NOT NULL DEFAULT TRUE, deleted_at TIMESTAMPTZ)",
        "CREATE TEMPORARY TABLE cycles (id UUID PRIMARY KEY, project_id UUID NOT NULL, deleted_at TIMESTAMPTZ)",
        "CREATE TEMPORARY TABLE modules (id UUID PRIMARY KEY, project_id UUID NOT NULL, deleted_at TIMESTAMPTZ)",
        "CREATE TEMPORARY TABLE deploy_boards (id UUID PRIMARY KEY, project_id UUID NOT NULL, workspace_id UUID NOT NULL, deleted_at TIMESTAMPTZ)",
        "CREATE TEMPORARY TABLE workspace_members (id UUID PRIMARY KEY, workspace_id UUID NOT NULL, member_id UUID NOT NULL, role SMALLINT NOT NULL DEFAULT 5, deleted_at TIMESTAMPTZ)",
        "CREATE TEMPORARY TABLE workspace_member_invites (id UUID PRIMARY KEY, workspace_id UUID NOT NULL, email TEXT NOT NULL, accepted BOOLEAN NOT NULL DEFAULT FALSE, responded_at TIMESTAMPTZ, role SMALLINT NOT NULL DEFAULT 5, deleted_at TIMESTAMPTZ)",
    ];

    fn live_uuid(s: &str) -> uuid::Uuid {
        uuid::Uuid::parse_str(s).expect("fixed test uuid")
    }

    const WS: &str = "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa";
    const MEMBER: &str = "22222222-2222-2222-2222-222222222222";
    const MEMBER2: &str = "77777777-7777-7777-7777-777777777777";
    const BOT: &str = "33333333-3333-3333-3333-333333333333";
    const PROJECT: &str = "11111111-1111-1111-1111-111111111111";
    const PRIVATE_PROJECT: &str = "55555555-5555-5555-5555-555555555555";
    const MEMBERSHIP: &str = "66666666-6666-6666-6666-666666666666";
    const INVITE: &str = "44444444-4444-4444-4444-444444444444";

    async fn live_tx(pool: &sqlx::PgPool) -> sqlx::Transaction<'_, sqlx::Postgres> {
        let mut tx = pool.begin().await.expect("begin scratch tx");
        for ddl in LIVE_DDL {
            sqlx::query(ddl)
                .execute(&mut *tx)
                .await
                .expect("create temp table");
        }
        tx
    }

    /// Seed the fixture shape: workspace `acme`, project Engine (`ENG`,
    /// public), one private project, two human members + one bot membership
    /// on Engine, one invite.
    async fn seed_full(tx: &mut sqlx::Transaction<'_, sqlx::Postgres>) {
        sqlx::query("INSERT INTO workspaces (id, slug, owner_id) VALUES ($1, 'acme', $2)")
            .bind(live_uuid(WS))
            .bind(live_uuid(MEMBER))
            .execute(&mut **tx)
            .await
            .expect("seed workspace");
        for (id, first, last, email, bot) in [
            (MEMBER, "Ct", "Member", "m@example.com", false),
            (MEMBER2, "Second", "Human", "s@example.com", false),
            (BOT, "Bot", "B", "b@example.com", true),
        ] {
            sqlx::query("INSERT INTO users (id, first_name, last_name, email, is_bot) VALUES ($1, $2, $3, $4, $5)")
                .bind(live_uuid(id))
                .bind(first)
                .bind(last)
                .bind(email)
                .bind(bot)
                .execute(&mut **tx)
                .await
                .expect("seed user");
        }
        sqlx::query("INSERT INTO projects (id, workspace_id, name, identifier, network) VALUES ($1, $2, 'Engine', 'ENG', 2)")
            .bind(live_uuid(PROJECT))
            .bind(live_uuid(WS))
            .execute(&mut **tx)
            .await
            .expect("seed project");
        sqlx::query("INSERT INTO projects (id, workspace_id, name, identifier, network) VALUES ($1, $2, 'Secret', 'SEC', 0)")
            .bind(live_uuid(PRIVATE_PROJECT))
            .bind(live_uuid(WS))
            .execute(&mut **tx)
            .await
            .expect("seed private project");
        // `role` binds as i16: the live column is smallint (INT2), and the
        // scratch DDL pins SMALLINT so these tests cover the INT2 decode.
        for (id, project, member, role, sort) in [
            (MEMBERSHIP, PROJECT, MEMBER, 20i16, 55535.0),
            (
                "88888888-8888-8888-8888-888888888888",
                PROJECT,
                MEMBER2,
                15i16,
                60000.0,
            ),
            (
                "99999999-9999-9999-9999-999999999999",
                PROJECT,
                BOT,
                15i16,
                61000.0,
            ),
            // The member also holds the private project: member scoping
            // (`vis.id IS NOT NULL`) lists it, strangers never see it.
            (
                "aaaaaaaa-1111-2222-3333-444444444444",
                PRIVATE_PROJECT,
                MEMBER,
                15i16,
                50000.0,
            ),
        ] {
            sqlx::query("INSERT INTO project_members (id, project_id, workspace_id, member_id, role, sort_order) VALUES ($1, $2, $3, $4, $5, $6)")
                .bind(live_uuid(id))
                .bind(live_uuid(project))
                .bind(live_uuid(WS))
                .bind(live_uuid(member))
                .bind(role)
                .bind(sort)
                .execute(&mut **tx)
                .await
                .expect("seed membership");
        }
        sqlx::query("INSERT INTO workspace_members (id, workspace_id, member_id, role) VALUES ($1, $2, $3, 20)")
            .bind(live_uuid("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"))
            .bind(live_uuid(WS))
            .bind(live_uuid(MEMBER))
            .execute(&mut **tx)
            .await
            .expect("seed workspace membership");
        sqlx::query("INSERT INTO workspace_member_invites (id, workspace_id, email, role) VALUES ($1, $2, 'new@example.com', 15)")
            .bind(live_uuid(INVITE))
            .bind(live_uuid(WS))
            .execute(&mut **tx)
            .await
            .expect("seed invite");
    }

    fn scope() -> TenantScope<'static> {
        TenantScope::new("acme", live_uuid(MEMBER))
    }

    #[tokio::test]
    async fn live_list_annotations_match_fixture_rows() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let mut tx = live_tx(&pool).await;
        seed_full(&mut tx).await;
        // GET variant: the fixture list_row, incl. sort_order.
        let rows = fetch_project_list_get(&mut *tx, &scope(), "sort_order")
            .await
            .expect("list get");
        assert_eq!(rows.len(), 2, "public + member-visible private project");
        let eng = rows
            .iter()
            .find(|r| r.id == live_uuid(PROJECT))
            .expect("Engine row");
        assert_eq!(eng.name, "Engine");
        assert_eq!(eng.identifier, "ENG");
        assert!(eng.is_member);
        assert_eq!(eng.member_role, Some(20));
        // Two humans; the bot membership is excluded.
        assert_eq!(eng.total_members, 2);
        assert_eq!(eng.total_cycles, 0);
        assert_eq!(eng.total_modules, 0);
        assert!(!eng.is_deployed);
        assert_eq!(eng.sort_order, Some(55535.0));
        // The private project lists through the membership branch.
        let sec = rows
            .iter()
            .find(|r| r.id == live_uuid(PRIVATE_PROJECT))
            .expect("Secret row");
        assert!(sec.is_member);
        assert_eq!(sec.member_role, Some(15));
        // Base statement: same annotations, no sort_order column.
        let base = fetch_project_list(&mut *tx, &scope())
            .await
            .expect("list base");
        let eng_base = base
            .iter()
            .find(|r| r.id == live_uuid(PROJECT))
            .expect("Engine base row");
        assert_eq!(eng_base.member_role, Some(20));
        assert_eq!(eng_base.sort_order, None);
    }

    #[tokio::test]
    async fn live_member_scoping() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let mut tx = live_tx(&pool).await;
        seed_full(&mut tx).await;
        // A stranger (no membership): only the public project is listed,
        // with NULL member_role and is_member false.
        let stranger = TenantScope::new("acme", live_uuid("00000000-0000-0000-0000-000000000000"));
        let rows = fetch_project_list(&mut *tx, &stranger)
            .await
            .expect("stranger list");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, live_uuid(PROJECT));
        assert!(!rows[0].is_member);
        assert_eq!(rows[0].member_role, None);
        // Detail base carries the same scoping.
        let detail = fetch_project_detail(&mut *tx, &stranger)
            .await
            .expect("stranger detail");
        assert_eq!(detail.len(), 1);
        // Unknown workspace slug: nothing.
        let elsewhere = TenantScope::new("nope", live_uuid(MEMBER));
        let empty = fetch_project_list(&mut *tx, &elsewhere)
            .await
            .expect("empty");
        assert!(empty.is_empty());
    }

    #[tokio::test]
    async fn live_resolve_uuid_identifier_missing() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let mut tx = live_tx(&pool).await;
        seed_full(&mut tx).await;
        assert_eq!(
            fetch_project_id(&mut *tx, &scope(), PROJECT)
                .await
                .expect("pk"),
            Some(live_uuid(PROJECT))
        );
        // Identifier: exact, case-insensitive input, surrounding space OK.
        assert_eq!(
            fetch_project_id(&mut *tx, &scope(), "eng")
                .await
                .expect("ident"),
            Some(live_uuid(PROJECT))
        );
        assert_eq!(
            fetch_project_id(&mut *tx, &scope(), " ENG ")
                .await
                .expect("padded"),
            Some(live_uuid(PROJECT))
        );
        // A UUID-looking value is never tried as an identifier.
        assert_eq!(
            fetch_project_id(&mut *tx, &scope(), "00000000-0000-0000-0000-000000000000")
                .await
                .expect("bogus uuid"),
            None
        );
        assert_eq!(
            fetch_project_id(&mut *tx, &scope(), "NOPE")
                .await
                .expect("unknown"),
            None
        );
        // Soft-deleted projects do not resolve.
        sqlx::query("UPDATE projects SET deleted_at = now() WHERE id = $1")
            .bind(live_uuid(PROJECT))
            .execute(&mut *tx)
            .await
            .expect("soft delete");
        assert_eq!(
            fetch_project_id(&mut *tx, &scope(), "ENG")
                .await
                .expect("deleted"),
            None
        );
    }

    #[tokio::test]
    async fn live_members_invites_detail() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let mut tx = live_tx(&pool).await;
        seed_full(&mut tx).await;
        assert!(fetch_workspace_exists(&mut *tx, &scope())
            .await
            .expect("exists"));
        assert!(
            !fetch_workspace_exists(&mut *tx, &TenantScope::new("nope", live_uuid(MEMBER)))
                .await
                .expect("missing")
        );
        let ws_id = fetch_workspace_id(&mut *tx, &scope())
            .await
            .expect("ws id")
            .expect("workspace");
        assert_eq!(ws_id, live_uuid(WS));
        let members = fetch_workspace_members(&mut *tx, ws_id)
            .await
            .expect("members");
        assert_eq!(members.len(), 1);
        // Fixture workspace_member_row shape.
        assert_eq!(members[0].member_id, live_uuid(MEMBER));
        assert_eq!(members[0].role, 20);
        assert_eq!(members[0].workspace_id, live_uuid(WS));
        // Q4: ids then users (bot included — no bot filter on this path).
        let ids = fetch_project_member_ids(&mut *tx, &scope(), live_uuid(PROJECT))
            .await
            .expect("ids");
        assert_eq!(ids.len(), 3);
        let users = fetch_project_users(&mut *tx, &ids).await.expect("users");
        assert_eq!(users.len(), 3);
        let ct = users
            .iter()
            .find(|u| u.id == live_uuid(MEMBER))
            .expect("Ct Member");
        // Fixture member_user_row shape.
        assert_eq!(ct.email, "m@example.com");
        assert_eq!(ct.first_name, "Ct");
        assert_eq!(ct.last_name, "Member");
        // Empty id list short-circuits with no query.
        assert!(fetch_project_users(&mut *tx, &[])
            .await
            .expect("empty")
            .is_empty());
        // Q5 detail + user.
        let pm = fetch_project_member(
            &mut *tx,
            &scope(),
            live_uuid(PROJECT),
            live_uuid(MEMBERSHIP),
        )
        .await
        .expect("detail")
        .expect("membership");
        assert_eq!(pm.member_id, Some(live_uuid(MEMBER)));
        let user = fetch_user_by_id(&mut *tx, live_uuid(MEMBER))
            .await
            .expect("user")
            .expect("user row");
        assert_eq!(user.email, "m@example.com");
        assert_eq!(
            fetch_project_member(
                &mut *tx,
                &scope(),
                live_uuid(PROJECT),
                live_uuid("00000000-0000-0000-0000-000000000000")
            )
            .await
            .expect("missing membership"),
            None
        );
        // Q6 invites.
        let invites = fetch_invites(&mut *tx, &scope()).await.expect("invites");
        assert_eq!(invites.len(), 1);
        // Fixture invite_row shape.
        assert_eq!(invites[0].id, live_uuid(INVITE));
        assert_eq!(invites[0].email, "new@example.com");
        assert!(!invites[0].accepted);
        assert_eq!(invites[0].responded_at, None);
        assert_eq!(invites[0].role, 15);
        let one = fetch_invite(&mut *tx, &scope(), live_uuid(INVITE))
            .await
            .expect("invite")
            .expect("invite row");
        assert_eq!(one, invites[0]);
        assert_eq!(
            fetch_invite(
                &mut *tx,
                &scope(),
                live_uuid("00000000-0000-0000-0000-000000000000")
            )
            .await
            .expect("missing invite"),
            None
        );
    }
}
