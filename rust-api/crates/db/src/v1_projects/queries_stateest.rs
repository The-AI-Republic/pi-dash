//! State / estimate queryset reads (D-19, stage 5).
//!
//! Ports the database read shapes behind
//! `apps/api/pi_dash/api/views/state.py` (`StateListCreateAPIEndpoint.get_queryset`
//! L47-60, `StateDetailAPIEndpoint.get_queryset` L170-183, the delete direct get
//! L231 and the patch direct get L278) and
//! `apps/api/pi_dash/api/views/estimate.py`
//! (`ProjectEstimateAPIEndpoint.get_queryset` L35-36,
//! `EstimatePointListCreateAPIEndpoint.get_queryset` L144-149,
//! `EstimatePointDetailAPIEndpoint.get_queryset` L241-246, the parent-row
//! pre-checks L48-54/L169-173/L197-201, and the point scoping L265/L287).
//! Fixture FX-Q-STATEEST
//! (`rust-api/fixtures/v1_projects/queries/state_estimate.{sql,rows.json}`).
//!
//! Dynamic statements are sea-query builders; fixed reads are string
//! constants executed with runtime `sqlx::query` (no `query!` macros:
//! there is no build-time database, same as the merged `license/queries`
//! and `app_integrations/queries_git` precedents). Every executor is
//! generic over `sqlx::Executor`. Row structs are the D-19 models
//! (`super::models::{state, estimate, estimate_point}`, PIDASHCONV-352,
//! Done) — this module ports queries only, never models.
//!
//! Tenant context is explicit on every path: each builder and fetch takes
//! the request's `(workspace_slug, project_id, ...)` triple as arguments.
//! There is no ambient tenant and no unscoped handle. `project_id` arrives
//! UUID-rewritten (identifier routing is `Project::resolve`, PIDASHCONV-352,
//! applied by the handlers layer, not here). The membership user id is the
//! acting user Django filters on
//! (`project__project_projectmember__member=self.request.user`).
//!
//! SQL semantics are Django's, quirks included (translate, don't redesign):
//!
//! * The default managers apply on every read: `State.objects` excludes
//!   `group = 'triage'` plus soft-deleted rows (`db/models/state.py:79-84`);
//!   `Estimate` / `EstimatePoint` use the soft-delete manager
//!   (`db/mixins.py:57-58`). The shared single-table helpers
//!   (`state::objects_condition`) are NOT reused here because every scope
//!   below joins tables that also carry `deleted_at` / `group`, so Django
//!   qualifies each predicate per table.
//! * The state list/detail scopes carry an explicit `SELECT DISTINCT`
//!   (`state.py:59,182`): the `project__project_projectmember` traversal is
//!   a multi-valued reverse-FK join. (`project_members` has
//!   `unique_together = ["project", "member", "deleted_at"]`, so the join is
//!   fanout-free in practice, but Django still emits `DISTINCT`.)
//! * `select_related("project")` / `select_related("workspace")`
//!   (`state.py:57-58,180-181`; estimate points L149) only add joined
//!   columns to the round trip; they change neither the row set nor the
//!   filter. The builders below project the base-table columns the row
//!   structs need, and the handlers layer re-adds the related selects when
//!   serializing nested objects.
//! * Scope builders carry no `ORDER BY`: `Meta.ordering` (`sequence` for
//!   states, `name` for estimates, the *string* `value` column for estimate
//!   points) applies at queryset evaluation / pagination time, which the
//!   handlers layer owns (PIDASHCONV-372).
//! * S2 ported asymmetry: both direct gets go through `State.objects`
//!   (triage-group and soft-deleted rows stay excluded) but do NOT join
//!   `projects`, so they skip the `archived_at` guard — states of an
//!   archived project can still be patched and deleted while the
//!   list/detail scopes hide them. The delete get (`state.py:231`) adds an
//!   explicit `is_triage=False`; the patch get (`state.py:278`) does NOT,
//!   so an `is_triage=True` row keeps serving PATCH while every other
//!   path hides it.
//! * E1 ported shape: one estimate per project is a code-level check
//!   (`.first()` + 409, `estimate.py:56-62`), not a DB constraint —
//!   concurrent POSTs can create two rows.

use sea_query::{Alias, Condition, Expr, JoinType, Query, SelectStatement};
use sqlx::postgres::PgRow;
use sqlx::Row;

use super::models::{estimate, estimate_point, project, project_member, state};

/// `workspaces` table (`db/models/workspace.py:181`).
const WORKSPACE_TABLE: &str = "workspaces";
/// `projects` table (`db/models/project.py`, `db_table = "projects"`).
const PROJECT_TABLE: &str = project::TABLE;
/// `project_members` table for the membership visibility check
/// (`project__project_projectmember`, `db/models/project.py:303,332-346`).
const VIS_TABLE: &str = project_member::TABLE;

/// Project the base-table columns of `table` in `COLUMNS` order.
fn select_table_columns(sel: &mut SelectStatement, table: &str, columns: &[&str]) {
    for col in columns {
        sel.column((Alias::new(table.to_owned()), Alias::new((*col).to_owned())));
    }
}

/// Inner join `workspaces` on `<table>.workspace_id = workspaces.id`.
fn join_workspace(sel: &mut SelectStatement, table: &str) {
    sel.join(
        JoinType::InnerJoin,
        Alias::new(WORKSPACE_TABLE.to_owned()),
        Condition::all().add(
            Expr::col((Alias::new(table.to_owned()), Alias::new("workspace_id")))
                .equals((Alias::new(WORKSPACE_TABLE), Alias::new("id"))),
        ),
    );
}

/// Inner join `projects` on `<table>.project_id = projects.id`.
fn join_project(sel: &mut SelectStatement, table: &str) {
    sel.join(
        JoinType::InnerJoin,
        Alias::new(PROJECT_TABLE.to_owned()),
        Condition::all().add(
            Expr::col((Alias::new(table.to_owned()), Alias::new("project_id")))
                .equals((Alias::new(PROJECT_TABLE), Alias::new("id"))),
        ),
    );
}

/// Inner join `project_members` visibility rows on
/// `vis.project_id = <table>.project_id` (the
/// `project__project_projectmember` traversal, `db/models/project.py:303`).
/// The member/is_active/deleted predicates live in the `WHERE`, same row
/// set as Django's `ON`-placed multi-valued-join conditions for an inner
/// join.
fn join_member_visibility(sel: &mut SelectStatement, table: &str) {
    sel.join(
        JoinType::InnerJoin,
        Alias::new(VIS_TABLE.to_owned()),
        Condition::all().add(
            Expr::col((Alias::new(VIS_TABLE.to_owned()), Alias::new("project_id")))
                .equals((Alias::new(table.to_owned()), Alias::new("project_id"))),
        ),
    );
}

// ---------------------------------------------------------------------------
// S1 state list/detail scope (views/state.py:47-60,170-183)
// ---------------------------------------------------------------------------

/// Shared `WHERE` for the state list and detail scopes
/// (`views/state.py:49-56,172-179`): workspace slug, project id, an active
/// membership row for the acting user, the `is_triage=False` view filter,
/// the default-manager triage exclusion, the soft-delete scope on the
/// `states` and `project_members` rows, plus the archived-project guard.
/// The `workspaces` / `projects` joins carry NO deleted predicate: Django
/// only scopes the base model through its manager, and the joined tables
/// ride along unscoped (verified against live Django SQL — states in a
/// soft-deleted workspace/project stay visible).
/// Binds `$1 = workspace slug`, `$2 = project id`, `$3 = member (user) id`.
fn state_scope_condition() -> Condition {
    let states = Alias::new(state::TABLE.to_owned());
    let workspaces = Alias::new(WORKSPACE_TABLE.to_owned());
    let projects = Alias::new(PROJECT_TABLE.to_owned());
    let vis = Alias::new(VIS_TABLE.to_owned());
    Condition::all()
        .add(Expr::col((workspaces, Alias::new("slug"))).eq(Expr::cust("$1")))
        .add(Expr::col((states.clone(), Alias::new("project_id"))).eq(Expr::cust("$2")))
        // `project__project_projectmember__member=user,
        // project__project_projectmember__is_active=True` (the project_id
        // correlation is the `ON` clause in `join_member_visibility`;
        // `project_members` rows also carry the soft-delete scope).
        .add(Expr::col((vis.clone(), Alias::new("member_id"))).eq(Expr::cust("$3")))
        .add(Expr::col((vis.clone(), Alias::new("is_active"))).eq(true))
        .add(Expr::col((vis, Alias::new("deleted_at"))).is_null())
        // View filter (`state.py:55,178`): the `is_triage` boolean column.
        .add(Expr::col((states.clone(), Alias::new("is_triage"))).eq(false))
        // Default-manager scope (`StateManager`, `db/models/state.py:82-83`).
        .add(Expr::col((states.clone(), Alias::new("group"))).ne(state::TRIAGE_GROUP))
        .add(Expr::col((states, Alias::new("deleted_at"))).is_null())
        // `project__archived_at__isnull=True` (`state.py:56,179`).
        .add(Expr::col((projects, Alias::new("archived_at"))).is_null())
}

/// `State.objects.filter(...)...distinct()` for the list and detail views
/// (`views/state.py:47-60,170-183`). `SELECT DISTINCT` mirrors the explicit
/// `.distinct()` (`state.py:59,182`).
/// Binds `$1 = workspace slug`, `$2 = project id`, `$3 = member (user) id`.
pub fn state_list_sql() -> String {
    use sea_query::PostgresQueryBuilder;
    let mut sel = Query::select();
    sel.distinct();
    sel.from(Alias::new(state::TABLE.to_owned()));
    select_table_columns(&mut sel, state::TABLE, state::COLUMNS);
    join_workspace(&mut sel, state::TABLE);
    join_project(&mut sel, state::TABLE);
    join_member_visibility(&mut sel, state::TABLE);
    sel.cond_where(state_scope_condition());
    sel.to_string(PostgresQueryBuilder)
}

/// Visible states for one project and acting user
/// (`views/state.py:149-159`, paginated by the handlers layer).
pub async fn fetch_state_list<'e, E>(
    ex: E,
    workspace_slug: &str,
    project_id: uuid::Uuid,
    member_id: uuid::Uuid,
) -> Result<Vec<state::State>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let rows: Vec<PgRow> = sqlx::query(&state_list_sql())
        .bind(workspace_slug)
        .bind(project_id)
        .bind(member_id)
        .fetch_all(ex)
        .await?;
    rows.iter().map(map_state_row).collect()
}

/// The detail scope is the list scope plus the pk
/// (`self.get_queryset().get(pk=state_id)`, `views/state.py:206-211`).
/// Binds `$1 = workspace slug`, `$2 = project id`, `$3 = member (user) id`,
/// `$4 = state id`.
pub fn state_detail_sql() -> String {
    use sea_query::PostgresQueryBuilder;
    let mut sel = Query::select();
    sel.distinct();
    sel.from(Alias::new(state::TABLE.to_owned()));
    select_table_columns(&mut sel, state::TABLE, state::COLUMNS);
    join_workspace(&mut sel, state::TABLE);
    join_project(&mut sel, state::TABLE);
    join_member_visibility(&mut sel, state::TABLE);
    sel.cond_where(state_scope_condition().add(
        Expr::col((Alias::new(state::TABLE.to_owned()), Alias::new("id"))).eq(Expr::cust("$4")),
    ));
    sel.limit(1);
    sel.to_string(PostgresQueryBuilder)
}

/// One visible state by id, or `None` (the view's 404 branch).
pub async fn fetch_state_detail<'e, E>(
    ex: E,
    workspace_slug: &str,
    project_id: uuid::Uuid,
    member_id: uuid::Uuid,
    state_id: uuid::Uuid,
) -> Result<Option<state::State>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let row: Option<PgRow> = sqlx::query(&state_detail_sql())
        .bind(workspace_slug)
        .bind(project_id)
        .bind(member_id)
        .bind(state_id)
        .fetch_optional(ex)
        .await?;
    row.map(|r| map_state_row(&r)).transpose()
}

/// Map one `states` row (column names, order-independent).
pub fn map_state_row(row: &PgRow) -> Result<state::State, sqlx::Error> {
    Ok(state::State {
        id: row.try_get("id")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
        created_by_id: row.try_get("created_by_id")?,
        updated_by_id: row.try_get("updated_by_id")?,
        deleted_at: row.try_get("deleted_at")?,
        project_id: row.try_get("project_id")?,
        workspace_id: row.try_get("workspace_id")?,
        name: row.try_get("name")?,
        description: row.try_get("description")?,
        color: row.try_get("color")?,
        slug: row.try_get("slug")?,
        sequence: row.try_get("sequence")?,
        group: row.try_get("group")?,
        is_triage: row.try_get("is_triage")?,
        default: row.try_get("default")?,
        external_source: row.try_get("external_source")?,
        external_id: row.try_get("external_id")?,
    })
}

// ---------------------------------------------------------------------------
// S2 state delete/patch direct gets (views/state.py:231,278)
// ---------------------------------------------------------------------------

/// `State.objects.get(is_triage=False, pk=state_id, project_id=...,
/// workspace__slug=...)` — the DELETE direct get (`views/state.py:231`).
///
/// Ported as-is: this goes through the default manager (triage-group and
/// soft-deleted rows stay excluded) plus the view's explicit
/// `is_triage=False`, but does NOT join `projects`, so the `archived_at`
/// guard is skipped — states of an archived project can still be deleted.
/// The `workspaces` join carries no deleted predicate (same scoping rule
/// as the list/detail scopes above).
/// Binds `$1 = workspace slug`, `$2 = project id`, `$3 = state id`.
pub fn state_direct_sql() -> String {
    use sea_query::PostgresQueryBuilder;
    let states = Alias::new(state::TABLE.to_owned());
    let workspaces = Alias::new(WORKSPACE_TABLE.to_owned());
    let mut sel = Query::select();
    sel.from(Alias::new(state::TABLE.to_owned()));
    select_table_columns(&mut sel, state::TABLE, state::COLUMNS);
    join_workspace(&mut sel, state::TABLE);
    sel.cond_where(
        Condition::all()
            .add(Expr::col((workspaces, Alias::new("slug"))).eq(Expr::cust("$1")))
            .add(Expr::col((states.clone(), Alias::new("project_id"))).eq(Expr::cust("$2")))
            .add(Expr::col((states.clone(), Alias::new("id"))).eq(Expr::cust("$3")))
            .add(Expr::col((states.clone(), Alias::new("is_triage"))).eq(false))
            .add(Expr::col((states.clone(), Alias::new("group"))).ne(state::TRIAGE_GROUP))
            .add(Expr::col((states, Alias::new("deleted_at"))).is_null()),
    );
    sel.limit(1);
    sel.to_string(PostgresQueryBuilder)
}

/// One state row for the delete path, or `None`.
pub async fn fetch_state_direct<'e, E>(
    ex: E,
    workspace_slug: &str,
    project_id: uuid::Uuid,
    state_id: uuid::Uuid,
) -> Result<Option<state::State>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let row: Option<PgRow> = sqlx::query(&state_direct_sql())
        .bind(workspace_slug)
        .bind(project_id)
        .bind(state_id)
        .fetch_optional(ex)
        .await?;
    row.map(|r| map_state_row(&r)).transpose()
}

/// `State.objects.get(workspace__slug=..., project_id=..., pk=...)` — the
/// PATCH direct get (`views/state.py:278`). Unlike the delete get it carries
/// NO `is_triage` filter: `is_triage` is a writable field, so a row flipped
/// to `is_triage=True` keeps serving PATCH while the list/detail/delete
/// paths hide it. The default-manager scope (triage-group + soft-deleted
/// rows excluded) still applies; the `archived_at` guard is skipped like
/// the delete get.
/// Binds `$1 = workspace slug`, `$2 = project id`, `$3 = state id`.
pub fn state_direct_for_patch_sql() -> String {
    use sea_query::PostgresQueryBuilder;
    let states = Alias::new(state::TABLE.to_owned());
    let workspaces = Alias::new(WORKSPACE_TABLE.to_owned());
    let mut sel = Query::select();
    sel.from(Alias::new(state::TABLE.to_owned()));
    select_table_columns(&mut sel, state::TABLE, state::COLUMNS);
    join_workspace(&mut sel, state::TABLE);
    sel.cond_where(
        Condition::all()
            .add(Expr::col((workspaces, Alias::new("slug"))).eq(Expr::cust("$1")))
            .add(Expr::col((states.clone(), Alias::new("project_id"))).eq(Expr::cust("$2")))
            .add(Expr::col((states.clone(), Alias::new("id"))).eq(Expr::cust("$3")))
            .add(Expr::col((states.clone(), Alias::new("group"))).ne(state::TRIAGE_GROUP))
            .add(Expr::col((states, Alias::new("deleted_at"))).is_null()),
    );
    sel.limit(1);
    sel.to_string(PostgresQueryBuilder)
}

/// One state row for the patch path, or `None`.
pub async fn fetch_state_direct_for_patch<'e, E>(
    ex: E,
    workspace_slug: &str,
    project_id: uuid::Uuid,
    state_id: uuid::Uuid,
) -> Result<Option<state::State>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let row: Option<PgRow> = sqlx::query(&state_direct_for_patch_sql())
        .bind(workspace_slug)
        .bind(project_id)
        .bind(state_id)
        .fetch_optional(ex)
        .await?;
    row.map(|r| map_state_row(&r)).transpose()
}

// ---------------------------------------------------------------------------
// E1 estimate queryset (views/estimate.py:35-36) + parent pre-checks
// ---------------------------------------------------------------------------

/// `Estimate.objects.filter(workspace__slug=..., project_id=...)`
/// (`views/estimate.py:35-36`): no membership filter at the queryset level
/// (membership is enforced by `ProjectEntityPermission`, PIDASHCONV-367),
/// no archived-project guard. The soft-delete manager applies to both
/// tables. Call sites take `.first()` (get/patch/delete L84/L108/L130),
/// which is `LIMIT 1` in the fetch below.
/// Binds `$1 = workspace slug`, `$2 = project id`.
pub fn estimate_for_project_sql() -> String {
    use sea_query::PostgresQueryBuilder;
    let estimates = Alias::new(estimate::TABLE.to_owned());
    let workspaces = Alias::new(WORKSPACE_TABLE.to_owned());
    let mut sel = Query::select();
    sel.from(Alias::new(estimate::TABLE.to_owned()));
    select_table_columns(&mut sel, estimate::TABLE, estimate::COLUMNS);
    join_workspace(&mut sel, estimate::TABLE);
    sel.cond_where(
        Condition::all()
            .add(Expr::col((workspaces.clone(), Alias::new("slug"))).eq(Expr::cust("$1")))
            .add(Expr::col((estimates.clone(), Alias::new("project_id"))).eq(Expr::cust("$2")))
            .add(Expr::col((estimates, Alias::new("deleted_at"))).is_null())
            .add(Expr::col((workspaces, Alias::new("deleted_at"))).is_null()),
    );
    sel.limit(1);
    sel.to_string(PostgresQueryBuilder)
}

/// The project's estimate, or `None` (the views' 404 `Estimate not found`
/// branch, `views/estimate.py:84-86,108-110,130-132`).
pub async fn fetch_estimate_for_project<'e, E>(
    ex: E,
    workspace_slug: &str,
    project_id: uuid::Uuid,
) -> Result<Option<estimate::Estimate>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let row: Option<PgRow> = sqlx::query(&estimate_for_project_sql())
        .bind(workspace_slug)
        .bind(project_id)
        .fetch_optional(ex)
        .await?;
    row.map(|r| map_estimate_row(&r)).transpose()
}

/// Map one `estimates` row (column names, order-independent). The `type`
/// column maps to `estimate_type` (`type` is a Rust keyword).
pub fn map_estimate_row(row: &PgRow) -> Result<estimate::Estimate, sqlx::Error> {
    Ok(estimate::Estimate {
        id: row.try_get("id")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
        created_by_id: row.try_get("created_by_id")?,
        updated_by_id: row.try_get("updated_by_id")?,
        deleted_at: row.try_get("deleted_at")?,
        project_id: row.try_get("project_id")?,
        workspace_id: row.try_get("workspace_id")?,
        name: row.try_get("name")?,
        description: row.try_get("description")?,
        estimate_type: row.try_get("type")?,
        last_used: row.try_get("last_used")?,
    })
}

/// `Project.objects.filter(id=project_id, workspace__slug=slug).first()`
/// (`views/estimate.py:48`): the create path checks the parent project
/// before the queryset. Binds `$1 = project id`, `$2 = workspace slug`.
pub fn project_exists_sql() -> String {
    use sea_query::PostgresQueryBuilder;
    let projects = Alias::new(PROJECT_TABLE.to_owned());
    let workspaces = Alias::new(WORKSPACE_TABLE.to_owned());
    let mut sel = Query::select();
    sel.from(Alias::new(PROJECT_TABLE.to_owned()));
    sel.column((projects.clone(), Alias::new("id")));
    join_workspace(&mut sel, PROJECT_TABLE);
    sel.cond_where(
        Condition::all()
            .add(Expr::col((projects.clone(), Alias::new("id"))).eq(Expr::cust("$1")))
            .add(Expr::col((workspaces.clone(), Alias::new("slug"))).eq(Expr::cust("$2")))
            .add(Expr::col((projects, Alias::new("deleted_at"))).is_null())
            .add(Expr::col((workspaces, Alias::new("deleted_at"))).is_null()),
    );
    sel.limit(1);
    sel.to_string(PostgresQueryBuilder)
}

/// Whether the parent project row exists (the create path's 404
/// `Project not found` branch, `views/estimate.py:48-50`).
pub async fn fetch_project_exists<'e, E>(
    ex: E,
    project_id: uuid::Uuid,
    workspace_slug: &str,
) -> Result<bool, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let row: Option<PgRow> = sqlx::query(&project_exists_sql())
        .bind(project_id)
        .bind(workspace_slug)
        .fetch_optional(ex)
        .await?;
    Ok(row.is_some())
}

/// `Workspace.objects.filter(slug=slug).first()`
/// (`views/estimate.py:52`): the create path's second pre-check.
/// Binds `$1 = workspace slug`.
pub fn workspace_exists_sql() -> String {
    use sea_query::PostgresQueryBuilder;
    let workspaces = Alias::new(WORKSPACE_TABLE.to_owned());
    let mut sel = Query::select();
    sel.from(Alias::new(WORKSPACE_TABLE.to_owned()));
    sel.column((workspaces.clone(), Alias::new("id")));
    sel.cond_where(
        Condition::all()
            .add(Expr::col((workspaces.clone(), Alias::new("slug"))).eq(Expr::cust("$1")))
            .add(Expr::col((workspaces, Alias::new("deleted_at"))).is_null()),
    );
    sel.limit(1);
    sel.to_string(PostgresQueryBuilder)
}

/// Whether the workspace row exists (the create path's 404
/// `Workspace not found` branch, `views/estimate.py:52-54`).
pub async fn fetch_workspace_exists<'e, E>(ex: E, workspace_slug: &str) -> Result<bool, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let row: Option<PgRow> = sqlx::query(&workspace_exists_sql())
        .bind(workspace_slug)
        .fetch_optional(ex)
        .await?;
    Ok(row.is_some())
}

// ---------------------------------------------------------------------------
// E2 estimate-point list / E3 point detail (views/estimate.py:144-149,241-246)
// ---------------------------------------------------------------------------

/// Shared `WHERE` for the point list and detail scopes
/// (`views/estimate.py:145-148,242-245`): parent estimate, workspace slug,
/// project id, plus the soft-delete scope on both tables.
/// Binds `$1 = estimate id`, `$2 = workspace slug`, `$3 = project id`.
fn estimate_point_scope_condition() -> Condition {
    let points = Alias::new(estimate_point::TABLE.to_owned());
    let workspaces = Alias::new(WORKSPACE_TABLE.to_owned());
    Condition::all()
        .add(Expr::col((points.clone(), Alias::new("estimate_id"))).eq(Expr::cust("$1")))
        .add(Expr::col((workspaces.clone(), Alias::new("slug"))).eq(Expr::cust("$2")))
        .add(Expr::col((points.clone(), Alias::new("project_id"))).eq(Expr::cust("$3")))
        .add(Expr::col((points, Alias::new("deleted_at"))).is_null())
        .add(Expr::col((workspaces, Alias::new("deleted_at"))).is_null())
}

/// `EstimatePoint.objects.filter(estimate_id=..., workspace__slug=...,
/// project_id=...)` (`views/estimate.py:144-149`). The
/// `.select_related("estimate", "workspace", "project")` (L149) is a fetch
/// optimization only; see the module docs.
/// Binds `$1 = estimate id`, `$2 = workspace slug`, `$3 = project id`.
pub fn estimate_point_list_sql() -> String {
    use sea_query::PostgresQueryBuilder;
    let mut sel = Query::select();
    sel.from(Alias::new(estimate_point::TABLE.to_owned()));
    select_table_columns(&mut sel, estimate_point::TABLE, estimate_point::COLUMNS);
    join_workspace(&mut sel, estimate_point::TABLE);
    sel.cond_where(estimate_point_scope_condition());
    sel.to_string(PostgresQueryBuilder)
}

/// Every point of one estimate in scope (the list view serializes the whole
/// queryset, `views/estimate.py:176-178`).
pub async fn fetch_estimate_point_list<'e, E>(
    ex: E,
    estimate_id: uuid::Uuid,
    workspace_slug: &str,
    project_id: uuid::Uuid,
) -> Result<Vec<estimate_point::EstimatePoint>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let rows: Vec<PgRow> = sqlx::query(&estimate_point_list_sql())
        .bind(estimate_id)
        .bind(workspace_slug)
        .bind(project_id)
        .fetch_all(ex)
        .await?;
    rows.iter().map(map_estimate_point_row).collect()
}

/// The detail scope plus the point id
/// (`self.get_queryset().filter(id=estimate_point_id).first()`,
/// `views/estimate.py:265,287`).
/// Binds `$1 = estimate id`, `$2 = workspace slug`, `$3 = project id`,
/// `$4 = point id`.
pub fn estimate_point_detail_sql() -> String {
    use sea_query::PostgresQueryBuilder;
    let mut sel = Query::select();
    sel.from(Alias::new(estimate_point::TABLE.to_owned()));
    select_table_columns(&mut sel, estimate_point::TABLE, estimate_point::COLUMNS);
    join_workspace(&mut sel, estimate_point::TABLE);
    sel.cond_where(
        estimate_point_scope_condition().add(
            Expr::col((
                Alias::new(estimate_point::TABLE.to_owned()),
                Alias::new("id"),
            ))
            .eq(Expr::cust("$4")),
        ),
    );
    sel.limit(1);
    sel.to_string(PostgresQueryBuilder)
}

/// One point by id in scope, or `None` (the views' 404
/// `Estimate point not found` branch, `views/estimate.py:265-267,287-289`).
pub async fn fetch_estimate_point_detail<'e, E>(
    ex: E,
    estimate_id: uuid::Uuid,
    workspace_slug: &str,
    project_id: uuid::Uuid,
    point_id: uuid::Uuid,
) -> Result<Option<estimate_point::EstimatePoint>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let row: Option<PgRow> = sqlx::query(&estimate_point_detail_sql())
        .bind(estimate_id)
        .bind(workspace_slug)
        .bind(project_id)
        .bind(point_id)
        .fetch_optional(ex)
        .await?;
    row.map(|r| map_estimate_point_row(&r)).transpose()
}

/// Map one `estimate_points` row (column names, order-independent). `key`
/// is a Python `IntegerField` (unbounded) read here as `i64` — values
/// outside `i64` range are a ported limitation shared with every other
/// unbounded-int read (Porting guide semantic traps row).
pub fn map_estimate_point_row(row: &PgRow) -> Result<estimate_point::EstimatePoint, sqlx::Error> {
    Ok(estimate_point::EstimatePoint {
        id: row.try_get("id")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
        created_by_id: row.try_get("created_by_id")?,
        updated_by_id: row.try_get("updated_by_id")?,
        deleted_at: row.try_get("deleted_at")?,
        project_id: row.try_get("project_id")?,
        workspace_id: row.try_get("workspace_id")?,
        estimate_id: row.try_get("estimate_id")?,
        key: row.try_get("key")?,
        description: row.try_get("description")?,
        value: row.try_get("value")?,
    })
}

/// `Estimate.objects.filter(id=estimate_id, workspace__slug=slug,
/// project_id=project_id).first()` (`views/estimate.py:169-173,197-201`):
/// the point list/get entry points verify the parent estimate row first and
/// 404 `Estimate not found` even when points exist.
/// Binds `$1 = estimate id`, `$2 = workspace slug`, `$3 = project id`.
pub fn parent_estimate_exists_sql() -> String {
    use sea_query::PostgresQueryBuilder;
    let estimates = Alias::new(estimate::TABLE.to_owned());
    let workspaces = Alias::new(WORKSPACE_TABLE.to_owned());
    let mut sel = Query::select();
    sel.from(Alias::new(estimate::TABLE.to_owned()));
    sel.column((estimates.clone(), Alias::new("id")));
    join_workspace(&mut sel, estimate::TABLE);
    sel.cond_where(
        Condition::all()
            .add(Expr::col((estimates.clone(), Alias::new("id"))).eq(Expr::cust("$1")))
            .add(Expr::col((workspaces.clone(), Alias::new("slug"))).eq(Expr::cust("$2")))
            .add(Expr::col((estimates.clone(), Alias::new("project_id"))).eq(Expr::cust("$3")))
            .add(Expr::col((estimates, Alias::new("deleted_at"))).is_null())
            .add(Expr::col((workspaces, Alias::new("deleted_at"))).is_null()),
    );
    sel.limit(1);
    sel.to_string(PostgresQueryBuilder)
}

/// Whether the parent estimate row exists in scope (the point views'
/// first 404 branch, `views/estimate.py:174-175,202-203`).
pub async fn fetch_parent_estimate_exists<'e, E>(
    ex: E,
    estimate_id: uuid::Uuid,
    workspace_slug: &str,
    project_id: uuid::Uuid,
) -> Result<bool, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let row: Option<PgRow> = sqlx::query(&parent_estimate_exists_sql())
        .bind(estimate_id)
        .bind(workspace_slug)
        .bind(project_id)
        .fetch_optional(ex)
        .await?;
    Ok(row.is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_list_is_distinct_with_member_visibility() {
        // FX-Q-STATEEST S1 (`views/state.py:47-60`).
        let sql = state_list_sql();
        assert!(sql.contains("SELECT DISTINCT"), "{sql}");
        assert!(sql.contains(r#"FROM "states""#), "{sql}");
        assert!(sql.contains(r#"INNER JOIN "workspaces""#), "{sql}");
        assert!(sql.contains(r#"INNER JOIN "projects""#), "{sql}");
        assert!(sql.contains(r#"INNER JOIN "project_members""#), "{sql}");
        assert!(
            sql.contains(r#""project_members"."project_id" = "states"."project_id""#),
            "{sql}"
        );
        assert!(sql.contains(r#""workspaces"."slug" = ($1)"#), "{sql}");
        assert!(sql.contains(r#""states"."project_id" = ($2)"#), "{sql}");
        assert!(
            sql.contains(r#""project_members"."member_id" = ($3)"#),
            "{sql}"
        );
        assert!(
            sql.contains(r#""project_members"."is_active" = TRUE"#),
            "{sql}"
        );
        assert!(sql.contains(r#""states"."is_triage" = FALSE"#), "{sql}");
        assert!(sql.contains(r#""states"."group" <> 'triage'"#), "{sql}");
        assert!(sql.contains(r#""projects"."archived_at" IS NULL"#), "{sql}");
        // Soft-delete scope on the base table + the membership rows only.
        // The `workspaces` / `projects` joins carry NO deleted predicate:
        // Django scopes only the base model through its manager (verified
        // against live Django SQL; the old count of 4 hid states of
        // soft-deleted workspaces/projects that Python still serves).
        assert_eq!(sql.matches(r#""deleted_at" IS NULL"#).count(), 2, "{sql}");
        assert!(!sql.contains(r#""workspaces"."deleted_at""#), "{sql}");
        assert!(!sql.contains(r#""projects"."deleted_at""#), "{sql}");
        // No point/estimate filter leaks into the state scope.
        assert!(!sql.contains("estimate"), "{sql}");
    }

    #[test]
    fn state_detail_adds_pk_to_list_scope() {
        // FX-Q-STATEEST S1 detail (`views/state.py:170-183,206-211`).
        let sql = state_detail_sql();
        assert!(sql.contains("SELECT DISTINCT"), "{sql}");
        assert!(sql.contains(r#""states"."id" = ($4)"#), "{sql}");
        assert!(sql.contains(r#""projects"."archived_at" IS NULL"#), "{sql}");
        assert!(sql.contains("LIMIT 1"), "{sql}");
    }

    #[test]
    fn state_direct_skips_archived_guard() {
        // FX-Q-STATEEST S2 delete get (`views/state.py:231`): triage-flag +
        // triage-group + soft-deleted rows still excluded, but no `projects`
        // join and no archived check. The `workspaces` join carries no
        // deleted predicate (same scoping rule as S1; the old count of 2
        // hid direct gets in soft-deleted workspaces that Python serves).
        let sql = state_direct_sql();
        assert!(!sql.contains("DISTINCT"), "{sql}");
        assert!(!sql.contains(r#"JOIN "projects""#), "{sql}");
        assert!(!sql.contains("archived_at"), "{sql}");
        assert!(sql.contains(r#""states"."id" = ($3)"#), "{sql}");
        assert!(sql.contains(r#""states"."is_triage" = FALSE"#), "{sql}");
        assert!(sql.contains(r#""states"."group" <> 'triage'"#), "{sql}");
        assert_eq!(sql.matches(r#""deleted_at" IS NULL"#).count(), 1, "{sql}");
        assert!(sql.contains("LIMIT 1"), "{sql}");
    }

    #[test]
    fn state_direct_for_patch_has_no_is_triage_filter() {
        // FX-Q-STATEEST S2b patch get (`views/state.py:278`): the
        // default-manager scope (triage-group + soft-deleted excluded) with
        // NO `is_triage` filter and NO archived check.
        let sql = state_direct_for_patch_sql();
        assert!(!sql.contains("DISTINCT"), "{sql}");
        assert!(!sql.contains(r#"JOIN "projects""#), "{sql}");
        assert!(!sql.contains("archived_at"), "{sql}");
        // No `is_triage` filter predicate (the column is still projected
        // for the row struct).
        assert!(!sql.contains(r#""is_triage" = FALSE"#), "{sql}");
        assert!(sql.contains(r#""states"."id" = ($3)"#), "{sql}");
        assert!(sql.contains(r#""states"."group" <> 'triage'"#), "{sql}");
        assert_eq!(sql.matches(r#""deleted_at" IS NULL"#).count(), 1, "{sql}");
        assert!(sql.contains("LIMIT 1"), "{sql}");
    }

    #[test]
    fn estimate_scope_has_no_membership_or_archived_filter() {
        // FX-Q-STATEEST E1 (`views/estimate.py:35-36`): membership is a
        // permission-layer concern (PIDASHCONV-367), not a queryset filter.
        let sql = estimate_for_project_sql();
        assert!(!sql.contains("DISTINCT"), "{sql}");
        assert!(!sql.contains("project_members"), "{sql}");
        assert!(!sql.contains("archived_at"), "{sql}");
        assert!(sql.contains(r#"FROM "estimates""#), "{sql}");
        assert!(sql.contains(r#""workspaces"."slug" = ($1)"#), "{sql}");
        assert!(sql.contains(r#""estimates"."project_id" = ($2)"#), "{sql}");
        assert_eq!(sql.matches(r#""deleted_at" IS NULL"#).count(), 2, "{sql}");
        assert!(sql.contains("LIMIT 1"), "{sql}");
    }

    #[test]
    fn estimate_point_scopes_cover_list_and_detail() {
        // FX-Q-STATEEST E2/E3 (`views/estimate.py:144-149,241-246`).
        let list = estimate_point_list_sql();
        assert!(list.contains(r#"FROM "estimate_points""#), "{list}");
        assert!(
            list.contains(r#""estimate_points"."estimate_id" = ($1)"#),
            "{list}"
        );
        assert!(list.contains(r#""workspaces"."slug" = ($2)"#), "{list}");
        assert!(
            list.contains(r#""estimate_points"."project_id" = ($3)"#),
            "{list}"
        );
        assert!(!list.contains("LIMIT"), "{list}");
        let detail = estimate_point_detail_sql();
        assert!(
            detail.contains(r#""estimate_points"."id" = ($4)"#),
            "{detail}"
        );
        assert!(detail.contains("LIMIT 1"), "{detail}");
        let parent = parent_estimate_exists_sql();
        assert!(parent.contains(r#""estimates"."id" = ($1)"#), "{parent}");
        assert!(parent.contains(r#""workspaces"."slug" = ($2)"#), "{parent}");
        assert!(
            parent.contains(r#""estimates"."project_id" = ($3)"#),
            "{parent}"
        );
        assert!(parent.contains("LIMIT 1"), "{parent}");
    }

    #[test]
    fn parent_precheck_shapes() {
        // `views/estimate.py:48,52`: project-then-workspace existence.
        let proj = project_exists_sql();
        assert!(proj.contains(r#""projects"."id" = ($1)"#), "{proj}");
        assert!(proj.contains(r#""workspaces"."slug" = ($2)"#), "{proj}");
        let ws = workspace_exists_sql();
        assert!(ws.contains(r#"FROM "workspaces""#), "{ws}");
        assert!(ws.contains(r#""workspaces"."slug" = ($1)"#), "{ws}");
        assert!(!ws.contains("projects"), "{ws}");
    }

    // -- live scratch-DB tests (env-gated, FX-Q-STATEEST replay) ----------

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
        "CREATE TEMPORARY TABLE workspaces (id UUID PRIMARY KEY, slug VARCHAR(48) NOT NULL UNIQUE, deleted_at TIMESTAMPTZ)",
        "CREATE TEMPORARY TABLE projects (id UUID PRIMARY KEY, workspace_id UUID NOT NULL, archived_at TIMESTAMPTZ, deleted_at TIMESTAMPTZ)",
        "CREATE TEMPORARY TABLE project_members (id UUID PRIMARY KEY, project_id UUID NOT NULL, workspace_id UUID NOT NULL, member_id UUID, is_active BOOLEAN NOT NULL, deleted_at TIMESTAMPTZ)",
        "CREATE TEMPORARY TABLE states (id UUID PRIMARY KEY, created_at TIMESTAMPTZ NOT NULL, updated_at TIMESTAMPTZ NOT NULL, created_by_id UUID, updated_by_id UUID, deleted_at TIMESTAMPTZ, project_id UUID NOT NULL, workspace_id UUID NOT NULL, name VARCHAR(255) NOT NULL, description TEXT NOT NULL, color VARCHAR(255) NOT NULL, slug VARCHAR(100) NOT NULL, sequence FLOAT8 NOT NULL, \"group\" VARCHAR(20) NOT NULL, is_triage BOOLEAN NOT NULL, \"default\" BOOLEAN NOT NULL, external_source VARCHAR(255), external_id VARCHAR(255))",
        "CREATE TEMPORARY TABLE estimates (id UUID PRIMARY KEY, created_at TIMESTAMPTZ NOT NULL, updated_at TIMESTAMPTZ NOT NULL, created_by_id UUID, updated_by_id UUID, deleted_at TIMESTAMPTZ, project_id UUID NOT NULL, workspace_id UUID NOT NULL, name VARCHAR(255) NOT NULL, description TEXT NOT NULL, \"type\" VARCHAR(255) NOT NULL, last_used BOOLEAN NOT NULL)",
        "CREATE TEMPORARY TABLE estimate_points (id UUID PRIMARY KEY, created_at TIMESTAMPTZ NOT NULL, updated_at TIMESTAMPTZ NOT NULL, created_by_id UUID, updated_by_id UUID, deleted_at TIMESTAMPTZ, project_id UUID NOT NULL, workspace_id UUID NOT NULL, estimate_id UUID NOT NULL, key BIGINT NOT NULL, description TEXT NOT NULL, value VARCHAR(255) NOT NULL)",
    ];

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

    fn live_uuid(suffix: u8) -> uuid::Uuid {
        uuid::Uuid::parse_str(&format!("44444444-4444-4444-4444-4444444444{suffix:02}"))
            .expect("fixed test uuid")
    }

    fn live_ts(secs: i64) -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::from_timestamp(secs, 0).expect("fixed test timestamp")
    }

    async fn seed_workspace(
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        id: uuid::Uuid,
        slug: &str,
    ) {
        sqlx::query("INSERT INTO workspaces (id, slug, deleted_at) VALUES ($1, $2, NULL)")
            .bind(id)
            .bind(slug)
            .execute(&mut **tx)
            .await
            .expect("seed workspace");
    }

    async fn seed_project(
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        id: uuid::Uuid,
        ws: uuid::Uuid,
        archived: bool,
    ) {
        sqlx::query("INSERT INTO projects (id, workspace_id, archived_at, deleted_at) VALUES ($1, $2, $3, NULL)")
            .bind(id)
            .bind(ws)
            .bind(if archived {
                Some(live_ts(1_700_000_100))
            } else {
                None
            })
            .execute(&mut **tx)
            .await
            .expect("seed project");
    }

    async fn seed_membership(
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        id: uuid::Uuid,
        proj: uuid::Uuid,
        ws: uuid::Uuid,
        user: uuid::Uuid,
        active: bool,
    ) {
        sqlx::query("INSERT INTO project_members (id, project_id, workspace_id, member_id, is_active, deleted_at) VALUES ($1, $2, $3, $4, $5, NULL)")
            .bind(id)
            .bind(proj)
            .bind(ws)
            .bind(user)
            .bind(active)
            .execute(&mut **tx)
            .await
            .expect("seed membership");
    }

    #[allow(clippy::too_many_arguments)]
    async fn seed_state(
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        id: uuid::Uuid,
        proj: uuid::Uuid,
        ws: uuid::Uuid,
        name: &str,
        group: &str,
        is_triage: bool,
        deleted: bool,
    ) {
        sqlx::query("INSERT INTO states (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, project_id, workspace_id, name, description, color, slug, sequence, \"group\", is_triage, \"default\", external_source, external_id) VALUES ($1, $2, $2, NULL, NULL, $3, $4, $5, $6, '', '#000000', $6, 35000.0, $7, $8, FALSE, NULL, NULL)")
            .bind(id)
            .bind(live_ts(1_700_000_000))
            .bind(if deleted {
                Some(live_ts(1_700_000_100))
            } else {
                None
            })
            .bind(proj)
            .bind(ws)
            .bind(name)
            .bind(group)
            .bind(is_triage)
            .execute(&mut **tx)
            .await
            .expect("seed state");
    }

    async fn seed_estimate(
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        id: uuid::Uuid,
        proj: uuid::Uuid,
        ws: uuid::Uuid,
    ) {
        sqlx::query("INSERT INTO estimates (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, project_id, workspace_id, name, description, \"type\", last_used) VALUES ($1, $2, $2, NULL, NULL, NULL, $3, $4, 'Fibonacci', '', 'points', FALSE)")
            .bind(id)
            .bind(live_ts(1_700_000_000))
            .bind(proj)
            .bind(ws)
            .execute(&mut **tx)
            .await
            .expect("seed estimate");
    }

    #[allow(clippy::too_many_arguments)]
    async fn seed_point(
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        id: uuid::Uuid,
        est: uuid::Uuid,
        proj: uuid::Uuid,
        ws: uuid::Uuid,
        key: i64,
        value: &str,
        deleted: bool,
    ) {
        sqlx::query("INSERT INTO estimate_points (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, project_id, workspace_id, estimate_id, key, description, value) VALUES ($1, $2, $2, NULL, NULL, $3, $4, $5, $6, $7, '', $8)")
            .bind(id)
            .bind(live_ts(1_700_000_000))
            .bind(if deleted {
                Some(live_ts(1_700_000_100))
            } else {
                None
            })
            .bind(proj)
            .bind(ws)
            .bind(est)
            .bind(key)
            .bind(value)
            .execute(&mut **tx)
            .await
            .expect("seed point");
    }

    /// FX-Q-STATEEST S1 replay: membership scoping, triage exclusion,
    /// archived-project hiding, soft-delete.
    #[tokio::test]
    async fn live_state_list_scoping() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let mut tx = live_tx(&pool).await;
        let (wsa, wsb) = (live_uuid(1), live_uuid(2));
        let (proja, arch) = (live_uuid(10), live_uuid(11));
        let (user, stranger, idle) = (live_uuid(20), live_uuid(21), live_uuid(22));
        seed_workspace(&mut tx, wsa, "acme").await;
        seed_workspace(&mut tx, wsb, "other").await;
        seed_project(&mut tx, proja, wsa, false).await;
        seed_project(&mut tx, arch, wsa, true).await;
        seed_membership(&mut tx, live_uuid(30), proja, wsa, user, true).await;
        seed_membership(&mut tx, live_uuid(31), proja, wsa, idle, false).await;
        // Visible row.
        seed_state(
            &mut tx,
            live_uuid(40),
            proja,
            wsa,
            "in-progress",
            "started",
            false,
            false,
        )
        .await;
        // Triage group: hidden by the default manager.
        seed_state(
            &mut tx,
            live_uuid(41),
            proja,
            wsa,
            "triage",
            "triage",
            false,
            false,
        )
        .await;
        // is_triage flag: hidden by the view filter.
        seed_state(
            &mut tx,
            live_uuid(42),
            proja,
            wsa,
            "odd",
            "backlog",
            true,
            false,
        )
        .await;
        // Soft-deleted: hidden by the manager.
        seed_state(
            &mut tx,
            live_uuid(43),
            proja,
            wsa,
            "gone",
            "backlog",
            false,
            true,
        )
        .await;
        // Archived project: hidden from the list scope.
        seed_state(
            &mut tx,
            live_uuid(44),
            arch,
            wsa,
            "archived-state",
            "backlog",
            false,
            false,
        )
        .await;

        let got = fetch_state_list(&mut *tx, "acme", proja, user)
            .await
            .expect("list");
        assert_eq!(
            got.iter().map(|s| s.id).collect::<Vec<_>>(),
            vec![live_uuid(40)]
        );
        assert_eq!(got[0].name, "in-progress");
        // Inactive membership sees nothing; strangers see nothing.
        assert!(fetch_state_list(&mut *tx, "acme", proja, idle)
            .await
            .expect("idle list")
            .is_empty());
        assert!(fetch_state_list(&mut *tx, "acme", proja, stranger)
            .await
            .expect("stranger list")
            .is_empty());
        // Detail finds the visible row and misses everything else.
        assert_eq!(
            fetch_state_detail(&mut *tx, "acme", proja, user, live_uuid(40))
                .await
                .expect("detail")
                .map(|s| s.name),
            Some("in-progress".to_string())
        );
        for hidden in [41u8, 42, 43] {
            assert_eq!(
                fetch_state_detail(&mut *tx, "acme", proja, user, live_uuid(hidden))
                    .await
                    .expect("hidden detail"),
                None
            );
        }
        assert_eq!(
            fetch_state_detail(&mut *tx, "acme", arch, user, live_uuid(44))
                .await
                .expect("archived detail"),
            None
        );
        tx.rollback().await.expect("rollback");
    }

    /// FX-Q-STATEEST S2 replay: the delete/patch direct gets skip the
    /// archived-project guard but keep the triage + soft-delete guards.
    #[tokio::test]
    async fn live_state_direct_gets() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let mut tx = live_tx(&pool).await;
        let ws = live_uuid(1);
        let (proja, arch) = (live_uuid(10), live_uuid(11));
        seed_workspace(&mut tx, ws, "acme").await;
        seed_project(&mut tx, proja, ws, false).await;
        seed_project(&mut tx, arch, ws, true).await;
        seed_state(
            &mut tx,
            live_uuid(50),
            arch,
            ws,
            "archived-state",
            "backlog",
            false,
            false,
        )
        .await;
        seed_state(
            &mut tx,
            live_uuid(51),
            proja,
            ws,
            "triage",
            "triage",
            false,
            false,
        )
        .await;

        // Archived-project state: reachable for patch/delete (S2), hidden
        // from list/detail (S1).
        assert_eq!(
            fetch_state_direct(&mut *tx, "acme", arch, live_uuid(50))
                .await
                .expect("direct")
                .map(|s| s.name),
            Some("archived-state".to_string())
        );
        // Triage row stays excluded even on the direct path.
        assert_eq!(
            fetch_state_direct(&mut *tx, "acme", proja, live_uuid(51))
                .await
                .expect("triage direct"),
            None
        );
        // Unknown id reads as absent.
        assert_eq!(
            fetch_state_direct(&mut *tx, "acme", proja, live_uuid(59))
                .await
                .expect("miss"),
            None
        );
        tx.rollback().await.expect("rollback");
    }

    /// FX-Q-STATEEST S2b replay: the PATCH direct get (`state.py:278`)
    /// carries no `is_triage` filter, so a row flipped to `is_triage=True`
    /// (writable field) stays reachable for PATCH while the delete direct
    /// get and the list/detail scopes hide it.
    #[tokio::test]
    async fn live_state_direct_for_patch_ignores_is_triage() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let mut tx = live_tx(&pool).await;
        let ws = live_uuid(1);
        let proja = live_uuid(10);
        seed_workspace(&mut tx, ws, "acme").await;
        seed_project(&mut tx, proja, ws, false).await;
        // Backlog row with the triage FLAG set (not the triage group).
        seed_state(
            &mut tx,
            live_uuid(52),
            proja,
            ws,
            "flagged",
            "backlog",
            true,
            false,
        )
        .await;

        // PATCH direct get serves it …
        assert_eq!(
            fetch_state_direct_for_patch(&mut *tx, "acme", proja, live_uuid(52))
                .await
                .expect("patch direct")
                .map(|s| s.name),
            Some("flagged".to_string())
        );
        // … while the DELETE direct get 404s it …
        assert_eq!(
            fetch_state_direct(&mut *tx, "acme", proja, live_uuid(52))
                .await
                .expect("delete direct"),
            None
        );
        // … and the list scope hides it too.
        let user = live_uuid(20);
        seed_membership(&mut tx, live_uuid(30), proja, ws, user, true).await;
        assert!(fetch_state_list(&mut *tx, "acme", proja, user)
            .await
            .expect("list")
            .is_empty());
        tx.rollback().await.expect("rollback");
    }

    /// FX-Q-STATEEST E1 replay: estimate read + create-path pre-checks.
    #[tokio::test]
    async fn live_estimate_reads() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let mut tx = live_tx(&pool).await;
        let (wsa, wsb) = (live_uuid(1), live_uuid(2));
        let (proja, other) = (live_uuid(10), live_uuid(12));
        seed_workspace(&mut tx, wsa, "acme").await;
        seed_workspace(&mut tx, wsb, "other").await;
        seed_project(&mut tx, proja, wsa, false).await;
        seed_project(&mut tx, other, wsa, false).await;
        seed_estimate(&mut tx, live_uuid(60), proja, wsa).await;

        let got = fetch_estimate_for_project(&mut *tx, "acme", proja)
            .await
            .expect("estimate");
        assert_eq!(got.map(|e| e.id), Some(live_uuid(60)));
        // Wrong project / wrong slug read as absent.
        assert_eq!(
            fetch_estimate_for_project(&mut *tx, "acme", other)
                .await
                .expect("miss"),
            None
        );
        assert_eq!(
            fetch_estimate_for_project(&mut *tx, "other", proja)
                .await
                .expect("miss"),
            None
        );
        // Create-path pre-checks (`estimate.py:48-54`).
        assert!(fetch_project_exists(&mut *tx, proja, "acme")
            .await
            .expect("project exists"));
        assert!(!fetch_project_exists(&mut *tx, proja, "other")
            .await
            .expect("project miss"));
        assert!(fetch_workspace_exists(&mut *tx, "acme")
            .await
            .expect("workspace exists"));
        assert!(!fetch_workspace_exists(&mut *tx, "nope")
            .await
            .expect("workspace miss"));
        tx.rollback().await.expect("rollback");
    }

    /// FX-Q-STATEEST E2/E3 replay: point list/detail scoping + the
    /// parent-estimate-first check.
    #[tokio::test]
    async fn live_estimate_point_reads() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let mut tx = live_tx(&pool).await;
        let ws = live_uuid(1);
        let (proja, other) = (live_uuid(10), live_uuid(12));
        let (est, ghost) = (live_uuid(60), live_uuid(61));
        seed_workspace(&mut tx, ws, "acme").await;
        seed_project(&mut tx, proja, ws, false).await;
        seed_project(&mut tx, other, ws, false).await;
        seed_estimate(&mut tx, est, proja, ws).await;
        seed_point(&mut tx, live_uuid(70), est, proja, ws, 1, "1", false).await;
        seed_point(&mut tx, live_uuid(71), est, proja, ws, 2, "2", false).await;
        // Soft-deleted point stays out of the list.
        seed_point(&mut tx, live_uuid(72), est, proja, ws, 3, "3", true).await;

        let mut got = fetch_estimate_point_list(&mut *tx, est, "acme", proja)
            .await
            .expect("list");
        got.sort_by_key(|a| a.id);
        assert_eq!(
            got.iter().map(|p| p.id).collect::<Vec<_>>(),
            vec![live_uuid(70), live_uuid(71)]
        );
        // Wrong estimate / project read as empty.
        assert!(fetch_estimate_point_list(&mut *tx, ghost, "acme", proja)
            .await
            .expect("miss")
            .is_empty());
        assert!(fetch_estimate_point_list(&mut *tx, est, "acme", other)
            .await
            .expect("miss")
            .is_empty());
        // Detail by id; missing point reads as absent.
        assert_eq!(
            fetch_estimate_point_detail(&mut *tx, est, "acme", proja, live_uuid(70))
                .await
                .expect("detail")
                .map(|p| p.value),
            Some("1".to_string())
        );
        assert_eq!(
            fetch_estimate_point_detail(&mut *tx, est, "acme", proja, live_uuid(79))
                .await
                .expect("miss"),
            None
        );
        // Parent check passes for the live estimate and fails for the
        // ghost — the views 404 `Estimate not found` first
        // (`estimate.py:174-175,202-203`) even when points exist.
        assert!(fetch_parent_estimate_exists(&mut *tx, est, "acme", proja)
            .await
            .expect("parent exists"));
        assert!(
            !fetch_parent_estimate_exists(&mut *tx, ghost, "acme", proja)
                .await
                .expect("parent miss")
        );
        tx.rollback().await.expect("rollback");
    }
}
