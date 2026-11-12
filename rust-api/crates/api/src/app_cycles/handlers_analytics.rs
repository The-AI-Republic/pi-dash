//! Cycle analytics handler (D-27, stage 5, PIDASHCONV-410).
//!
//! Ports `CycleAnalyticsEndpoint.get`
//! (`apps/api/pi_dash/app/views/cycle/base.py:786-1049`, drift baseline
//! `01a93e17`) with identical URL path, status codes and JSON bytes:
//!
//! - `GET workspaces/<slug>/projects/<id>/cycles/<uuid>/analytics/?type=`
//!   (`app/urls/cycle.py`, routes 13-14 family).
//!
//! Handler notes (all verified against the Python source):
//! - Gate is GUEST (`base.py:787`, [`crate::app_cycles::gates`] row
//!   `GET .../analytics/`): session authN first (anon 401), then the
//!   `@allow_permission` PROJECT check (403), then the body.
//! - `type` defaults to `"issues"` (`request.GET.get("type", "issues")`,
//!   `:789`); `?type=points` runs the estimate branch only when the
//!   project has a `"points"` estimate (`:832-837`), otherwise the
//!   empty envelope. The `issues` branch is a separate `if` (`:940`),
//!   so an unknown `type` answers the empty envelope
//!   `{assignees: [], labels: [], completion_chart: {}}`.
//! - A missing cycle is an `AttributeError` (`None.start_date`,
//!   `:807`): the generic 500, NOT a 404 — there is deliberately no
//!   `Cycle not found` branch here (differs from progress, ported).
//! - A cycle without start or end date answers 400
//!   `{"error":"Cycle has no start or end date"}` (`:807-811`).
//! - A truthy `progress_snapshot` short-circuits to the stored
//!   `distribution` (`:821-830`) with envelope order `labels,
//!   assignees, completion_chart` — different from the final
//!   envelope order (`assignees, labels, completion_chart`, `:1042`).
//! - Distribution rows follow `.values()` + annotation order: assignee
//!   rows `display_name, assignee_id, avatar_url,
//!   total_*, completed_*, pending_*`; label rows `label_name, color,
//!   label_id, ...`. `Sum`s render `NULL` as JSON `null` (no `or 0`
//!   here); `Count`s are ints. The `total_*` sums carry no
//!   archived/draft filter while `completed_*`/`pending_*` do
//!   (`:873-931`) — ported as written.
//! - `avatar_url` is the `Case`/`Concat` (`:951-969`):
//!   `'/api/assets/v2/static/<uuid>/'` when `avatar_asset` is set,
//!   else the raw `avatar` field (possibly `""`), else `NULL`.
//! - `completion_chart` is `burndown_plot` (`utils/analytics_plot.py:
//!   123-264`, cycle path): per-day pending (total minus completed on
//!   or before the day) over the inclusive start..end range; future
//!   days are `null`; the range endpoints and "today" are UTC
//!   (`TruncDate` on a range endpoint is a plain date), while the
//!   completion buckets truncate in the actor's active zone
//!   (`TimezoneMixin` activates `user_timezone`). The `points` chart
//!   needs the points estimate (only called when it exists); the
//!   `issues` chart counts completions per day.
//!
//! Fixture oracle: F-C27-09
//! (`rust-api/fixtures/app_cycles/analytics.json` + `TRACE.md`).
//!
//! Ported bugs (translation, don't redesign; also listed in the PR):
//! - B1: unknown cycle 500s (`AttributeError`) instead of 404.
//! - B2: M2M joins (`issue_assignees`, `issue_labels`) carry no
//!   `deleted_at` guard — soft-deleted assignments still distribute.
//! - B3: `total_*` sums include archived/draft rows where the base
//!   manager lets them through (no explicit filter, unlike the
//!   completed/pending sums).
//! - B4: unknown `?type=` silently answers the empty envelope.
//!
//! Sibling plumbing mirrors `handlers_progress` (and through it the
//! D-29 `app_views_search` shape): [`owned`], session [`actor`], exact
//! denial bodies, manual envelope assembly for DRF key order/bytes.

use std::collections::HashMap;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Router;
use chrono::NaiveDate;
use serde_json::{Map, Value};

use crate::middleware::SessionHandle;
use crate::state::AppState;

use super::gates::{decide_gate, gate_for, tenant_context, GateOutcome, ANON_BODY, FORBIDDEN_BODY};
use pidash_compat::response::json_response;
pub use pidash_compat::response::SERVER_ERROR_BODY;

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------

/// Route path template for the analytics endpoint, in
/// `app/urls/cycle.py` form (the `GET .../analytics/` row also lives in
/// [`crate::app_cycles::gates::GATES`]).
pub const ANALYTICS_PATH: &str = "workspaces/<slug>/projects/<id>/cycles/<uuid>/analytics/";

/// Register the analytics GET route. Nothing else: sibling paths stay
/// unmatched and proxy to Django, and every non-owned method on the
/// owned path falls through to Django too.
pub fn routes() -> Router<AppState> {
    Router::new().route(
        "/api/workspaces/{slug}/projects/{project_id}/cycles/{cycle_id}/analytics/",
        owned(axum::routing::get(analytics_get), &["GET"]),
    )
}

/// An owned path: listed methods serve from Rust, everything else proxies
/// to Django (DRF metadata, 401-anon-before-405). HEAD proxies too: Django
/// defines no `head` and 405s after auth (axum would auto-serve it from
/// `get`).
fn owned(
    router: axum::routing::MethodRouter<AppState>,
    methods: &[&str],
) -> axum::routing::MethodRouter<AppState> {
    let mut router = router;
    for method in ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] {
        if methods.contains(&method) {
            continue;
        }
        router = match method {
            "GET" => router.get(crate::edge::proxy),
            "POST" => router.post(crate::edge::proxy),
            "PUT" => router.put(crate::edge::proxy),
            "PATCH" => router.patch(crate::edge::proxy),
            "DELETE" => router.delete(crate::edge::proxy),
            "HEAD" => router.head(crate::edge::proxy),
            _ => router.options(crate::edge::proxy),
        };
    }
    router
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// No-dates 400 (`base.py:807-811`).
pub const NO_DATES_BODY: &str = r#"{"error":"Cycle has no start or end date"}"#;
/// ORM `ValidationError` 400 (`app/views/base.py:126-130`).
pub const VALIDATION_BODY: &str = r#"{"error":"Please provide valid detail"}"#;
/// `Project.resolve` miss on a non-UUID identifier
/// (`db/models/project.py:213-217`) — `_rewrite_project_kwarg`'s
/// `Http404("Project not found")`.
pub const PROJECT_NOT_FOUND_BODY: &str = r#"{"detail":"Project not found"}"#;

/// Handler denials with byte-exact bodies.
pub enum Denial {
    Unauthorized,
    Forbidden,
    NoDates,
    BadValidation,
    ProjectNotFound,
    ServerError,
}

impl Denial {
    fn status_and_body(&self) -> (StatusCode, String) {
        match self {
            Denial::Unauthorized => (StatusCode::UNAUTHORIZED, ANON_BODY.to_owned()),
            Denial::Forbidden => (StatusCode::FORBIDDEN, FORBIDDEN_BODY.to_owned()),
            Denial::NoDates => (StatusCode::BAD_REQUEST, NO_DATES_BODY.to_owned()),
            Denial::BadValidation => (StatusCode::BAD_REQUEST, VALIDATION_BODY.to_owned()),
            Denial::ProjectNotFound => (StatusCode::NOT_FOUND, PROJECT_NOT_FOUND_BODY.to_owned()),
            Denial::ServerError => (
                StatusCode::INTERNAL_SERVER_ERROR,
                SERVER_ERROR_BODY.to_owned(),
            ),
        }
    }
}

impl IntoResponse for Denial {
    fn into_response(self) -> Response {
        let (status, body) = self.status_and_body();
        (
            status,
            [(axum::http::header::CONTENT_TYPE, "application/json")],
            body,
        )
            .into_response()
    }
}

// ---------------------------------------------------------------------------
// Request context: auth + tenant + membership
// ---------------------------------------------------------------------------

async fn actor(
    state: &AppState,
    extension: Option<axum::Extension<SessionHandle>>,
) -> Result<crate::license::Actor, Denial> {
    let pool = pool_of(state)?;
    crate::license::resolve_actor(pool, state.settings().secret_key.as_bytes(), extension)
        .await
        .map_err(|_| Denial::ServerError)?
        .ok_or(Denial::Unauthorized)
}

fn pool_of(state: &AppState) -> Result<&sqlx::PgPool, Denial> {
    state
        .pools()
        .map(|pools| pools.primary())
        .ok_or(Denial::ServerError)
}

/// The shared SeaORM connection over an sqlx pool (no I/O — it wraps the
/// pool).
fn sea_conn(pool: &sqlx::PgPool) -> sea_orm::DatabaseConnection {
    pidash_db::pool::sea_orm_connection(pool)
}

/// Python `str.strip()` membership (`db/models/project.py:210`): Rust
/// `White_Space` plus U+001C-U+001F (verified by exhaustively diffing
/// `str.strip` against `char::is_whitespace` over all code points —
/// those four are the only differences).
fn is_py_strip_ws(ch: char) -> bool {
    ch.is_whitespace() || matches!(ch, '\u{1c}'..='\u{1f}')
}

/// Normalize a non-UUID identifier for the equality lookup
/// (`db/models/project.py:210`): `str(value).strip().upper()`.
fn normalize_resolve_identifier(raw: &str) -> String {
    raw.trim_matches(is_py_strip_ws).to_uppercase()
}

/// `_rewrite_project_kwarg` (`app/views/base.py:49-80`): authenticated
/// callers only (anonymous already 401'd); UUIDs pass through unchecked;
/// other identifiers resolve `UPPER(identifier)` in the workspace, else
/// `Http404("Project not found")` (`db/models/project.py:192-219`).
async fn resolve_project_id(
    pool: &sqlx::PgPool,
    slug: &str,
    raw: &str,
) -> Result<uuid::Uuid, Denial> {
    if let Ok(id) = raw.parse::<uuid::Uuid>() {
        return Ok(id);
    }
    let upper = normalize_resolve_identifier(raw);
    use pidash_db::entity::projects as proj_entity;
    use pidash_db::entity::workspaces as ws_entity;
    use sea_orm::{ColumnTrait, EntityTrait, JoinType, QueryFilter, QuerySelect, RelationTrait};
    let row: Option<uuid::Uuid> = proj_entity::Entity::find()
        .join(JoinType::InnerJoin, proj_entity::Relation::Workspaces.def())
        .filter(ws_entity::Column::Slug.eq(slug))
        .filter(proj_entity::Column::Identifier.eq(upper))
        .filter(proj_entity::Column::DeletedAt.is_null())
        .select_only()
        .column(proj_entity::Column::Id)
        .into_tuple()
        .one(&sea_conn(pool))
        .await
        .map_err(|_| Denial::ServerError)?;
    row.ok_or(Denial::ProjectNotFound)
}

async fn membership(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    user_id: &uuid::Uuid,
) -> Result<(Option<i32>, Option<i32>), Denial> {
    use pidash_db::entity::project_members as pm_entity;
    use pidash_db::entity::workspace_members as wm_entity;
    use pidash_db::entity::workspaces as ws_entity;
    use sea_orm::{ColumnTrait, EntityTrait, JoinType, QueryFilter, QuerySelect, RelationTrait};
    // `w.deleted_at IS NULL` in the raw join `ON` clause is the same
    // predicate on an inner join as a `WHERE` filter on the joined
    // table, so it stays a filter here.
    let workspace_role: Option<i16> = wm_entity::Entity::find()
        .join(JoinType::InnerJoin, wm_entity::Relation::Workspaces.def())
        .filter(ws_entity::Column::Slug.eq(slug))
        .filter(ws_entity::Column::DeletedAt.is_null())
        .filter(wm_entity::Column::MemberId.eq(*user_id))
        .filter(wm_entity::Column::IsActive.eq(true))
        .filter(wm_entity::Column::DeletedAt.is_null())
        .select_only()
        .column(wm_entity::Column::Role)
        .into_tuple()
        .one(&sea_conn(pool))
        .await
        .map_err(|_| Denial::ServerError)?;
    let project_role: Option<i16> = pm_entity::Entity::find()
        .join(JoinType::InnerJoin, pm_entity::Relation::Workspaces.def())
        .filter(ws_entity::Column::Slug.eq(slug))
        .filter(ws_entity::Column::DeletedAt.is_null())
        .filter(pm_entity::Column::ProjectId.eq(*project_id))
        .filter(pm_entity::Column::MemberId.eq(Some(*user_id)))
        .filter(pm_entity::Column::IsActive.eq(true))
        .filter(pm_entity::Column::DeletedAt.is_null())
        .select_only()
        .column(pm_entity::Column::Role)
        .into_tuple()
        .one(&sea_conn(pool))
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok((workspace_role.map(i32::from), project_role.map(i32::from)))
}

// ---------------------------------------------------------------------------
// Sea-query builders (distributions + scope)
// ---------------------------------------------------------------------------

/// Assignee distribution rows, points flavor, decoded by name.
#[derive(Debug, Clone, sea_orm::FromQueryResult)]
struct AssigneePointsRow {
    display_name: Option<String>,
    assignee_id: Option<uuid::Uuid>,
    avatar_url: Option<String>,
    total_estimates: Option<f64>,
    completed_estimates: Option<f64>,
    pending_estimates: Option<f64>,
}

/// Assignee distribution rows, issues flavor, decoded by name.
#[derive(Debug, Clone, sea_orm::FromQueryResult)]
struct AssigneeIssuesRow {
    display_name: Option<String>,
    assignee_id: Option<uuid::Uuid>,
    avatar_url: Option<String>,
    total_issues: Option<i64>,
    completed_issues: Option<i64>,
    pending_issues: Option<i64>,
}

/// Label distribution rows, points flavor, decoded by name.
#[derive(Debug, Clone, sea_orm::FromQueryResult)]
struct LabelPointsRow {
    label_name: Option<String>,
    color: Option<String>,
    label_id: Option<uuid::Uuid>,
    total_estimates: Option<f64>,
    completed_estimates: Option<f64>,
    pending_estimates: Option<f64>,
}

/// Label distribution rows, issues flavor, decoded by name.
#[derive(Debug, Clone, sea_orm::FromQueryResult)]
struct LabelIssuesRow {
    label_name: Option<String>,
    color: Option<String>,
    label_id: Option<uuid::Uuid>,
    total_issues: Option<i64>,
    completed_issues: Option<i64>,
    pending_issues: Option<i64>,
}

/// One total-points row (`SUM` over the cycle scope).
#[derive(Debug, Clone, Copy, sea_orm::FromQueryResult)]
struct TotalPointsRow {
    total_points: Option<f64>,
}

/// Base scope shared by the distribution and burndown-total queries:
/// live bridge row plus the `IssueManager` excludes (the explicit
/// archived/draft filters on the completed/pending aggregates layer on
/// top). Each query adds its own distribution joins between the FROM
/// and WHERE parts, matching Django's join order.
///
/// The triage exclusion is `NOT (group = 'triage')` — three-valued, so
/// `NULL`-state rows drop with it (`<>` is `NOT (=)` under the same
/// NULL rule). Joined tables carry no `deleted_at` predicate: Django
/// applies the soft-delete manager to the base table only.
fn dist_scope_stmt(
    slug: &str,
    project_id: uuid::Uuid,
    cycle_id: uuid::Uuid,
) -> sea_orm::sea_query::SelectStatement {
    use pidash_db::entity::{cycle_issues, issues, projects, states, workspaces};
    use sea_orm::sea_query::{Alias, Condition, Expr, JoinType, Query};
    let a = Alias::new;
    let mut stmt = Query::select();
    stmt.from_as(issues::Entity, a("i"))
        .join_as(
            JoinType::InnerJoin,
            cycle_issues::Entity,
            a("ci"),
            Condition::all()
                .add(
                    Expr::col((a("ci"), cycle_issues::Column::IssueId))
                        .equals((a("i"), issues::Column::Id)),
                )
                .add(Expr::col((a("ci"), cycle_issues::Column::CycleId)).eq(cycle_id))
                .add(Expr::col((a("ci"), cycle_issues::Column::DeletedAt)).is_null()),
        )
        .join_as(
            JoinType::InnerJoin,
            workspaces::Entity,
            a("w"),
            Condition::all()
                .add(
                    Expr::col((a("w"), workspaces::Column::Id))
                        .equals((a("i"), issues::Column::WorkspaceId)),
                )
                .add(Expr::col((a("w"), workspaces::Column::Slug)).eq(slug.to_owned())),
        )
        .join_as(
            JoinType::InnerJoin,
            projects::Entity,
            a("p"),
            Condition::all()
                .add(
                    Expr::col((a("p"), projects::Column::Id))
                        .equals((a("i"), issues::Column::ProjectId)),
                )
                .add(Expr::col((a("p"), projects::Column::Id)).eq(project_id))
                .add(Expr::col((a("p"), projects::Column::ArchivedAt)).is_null()),
        )
        .join_as(
            JoinType::LeftJoin,
            states::Entity,
            a("s"),
            Expr::col((a("s"), states::Column::Id)).equals((a("i"), issues::Column::StateId)),
        )
        .and_where(Expr::col((a("i"), issues::Column::DeletedAt)).is_null())
        .and_where(Expr::col((a("i"), issues::Column::ArchivedAt)).is_null())
        .and_where(Expr::col((a("i"), issues::Column::IsDraft)).eq(false))
        .and_where(Expr::col((a("s"), states::Column::Group)).ne("triage"));
    stmt
}

/// `avatar_url` Case/Concat (`base.py:951-969`): asset URL when the
/// user has an `avatar_asset_id`, else the stored avatar text.
fn avatar_expr() -> sea_orm::sea_query::SimpleExpr {
    use pidash_db::entity::users;
    use sea_orm::sea_query::{Alias, CaseStatement, Expr, SimpleExpr};
    use sea_query::extension::postgres::PgExpr;
    let a = Alias::new;
    let asset_id = Expr::col((a("u"), users::Column::AvatarAssetId));
    let asset_text = asset_id.clone().cast_as(a("text"));
    let url = Expr::val("/api/assets/v2/static/")
        .concat(asset_text)
        .concat(Expr::val("/"));
    SimpleExpr::Case(Box::new(
        CaseStatement::new()
            .case(asset_id.clone().is_not_null(), url)
            .case(
                asset_id.is_null(),
                Expr::col((a("u"), users::Column::Avatar)),
            )
            .finally(Expr::val(Option::<String>::None)),
    ))
}

/// `CAST(ep.value AS DOUBLE PRECISION)` over the `ep` alias.
fn estimate_value() -> sea_orm::sea_query::SimpleExpr {
    use pidash_db::entity::estimate_points;
    use sea_orm::sea_query::{Alias, Expr};
    Expr::col((Alias::new("ep"), estimate_points::Column::Value))
        .cast_as(Alias::new("double precision"))
}

/// `SUM(x) FILTER (WHERE cond)` compiled to `SUM(CASE WHEN cond THEN x
/// END)`: rows where the condition is NOT TRUE (incl. NULL) contribute
/// NULL, so the sum is NULL when nothing matches — as with `FILTER`.
fn filtered_sum(
    value: sea_orm::sea_query::SimpleExpr,
    cond: sea_orm::sea_query::Condition,
) -> sea_orm::sea_query::SimpleExpr {
    use sea_orm::sea_query::{CaseStatement, Func, SimpleExpr};
    Func::sum(SimpleExpr::Case(Box::new(
        CaseStatement::new().case(cond, value),
    )))
    .into()
}

/// `COUNT(x) FILTER (WHERE cond)` compiled to `COUNT(CASE WHEN cond
/// THEN x END)`: same NULL-skipping as `FILTER`.
fn filtered_count(
    value: sea_orm::sea_query::SimpleExpr,
    cond: sea_orm::sea_query::Condition,
) -> sea_orm::sea_query::SimpleExpr {
    use sea_orm::sea_query::{CaseStatement, Func, SimpleExpr};
    Func::count(SimpleExpr::Case(Box::new(
        CaseStatement::new().case(cond, value),
    )))
    .into()
}

/// Completed/pending conditions over the `i` alias (the `FILTER`
/// clauses of the four distributions).
fn completed_cond() -> sea_orm::sea_query::Condition {
    use pidash_db::entity::issues;
    use sea_orm::sea_query::{Alias, Condition, Expr};
    let a = Alias::new;
    Condition::all()
        .add(Expr::col((a("i"), issues::Column::CompletedAt)).is_not_null())
        .add(Expr::col((a("i"), issues::Column::ArchivedAt)).is_null())
        .add(Expr::col((a("i"), issues::Column::IsDraft)).eq(false))
}

fn pending_cond() -> sea_orm::sea_query::Condition {
    use pidash_db::entity::issues;
    use sea_orm::sea_query::{Alias, Condition, Expr};
    let a = Alias::new;
    Condition::all()
        .add(Expr::col((a("i"), issues::Column::CompletedAt)).is_null())
        .add(Expr::col((a("i"), issues::Column::ArchivedAt)).is_null())
        .add(Expr::col((a("i"), issues::Column::IsDraft)).eq(false))
}

/// `total_issues` uses only the archived/draft filter (no
/// completed split) in the issues flavor.
fn issues_total_cond() -> sea_orm::sea_query::Condition {
    use pidash_db::entity::issues;
    use sea_orm::sea_query::{Alias, Condition, Expr};
    let a = Alias::new;
    Condition::all()
        .add(Expr::col((a("i"), issues::Column::ArchivedAt)).is_null())
        .add(Expr::col((a("i"), issues::Column::IsDraft)).eq(false))
}

/// Points assignee distribution (`base.py:843-899`): per-assignee
/// estimate sums ordered by display name. `total_estimates` has no
/// archived/draft filter (B3); completed/pending do. Django never
/// filters on `estimate_point` (a nullable FK), so the ORM emits a LEFT
/// OUTER JOIN and unestimated issues keep their group with NULL sums
/// (PIDASHCONV-1185).
fn points_assignee_stmt(
    slug: &str,
    project_id: uuid::Uuid,
    cycle_id: uuid::Uuid,
) -> sea_orm::sea_query::SelectStatement {
    use pidash_db::entity::{estimate_points, issue_assignees, issues, users};
    use sea_orm::sea_query::{Alias, Expr, Func, JoinType, Order};
    let a = Alias::new;
    let mut stmt = dist_scope_stmt(slug, project_id, cycle_id);
    stmt.join_as(
        JoinType::LeftJoin,
        estimate_points::Entity,
        a("ep"),
        Expr::col((a("ep"), estimate_points::Column::Id))
            .equals((a("i"), issues::Column::EstimatePointId)),
    )
    .join_as(
        JoinType::LeftJoin,
        issue_assignees::Entity,
        a("ia"),
        Expr::col((a("ia"), issue_assignees::Column::IssueId)).equals((a("i"), issues::Column::Id)),
    )
    .join_as(
        JoinType::LeftJoin,
        users::Entity,
        a("u"),
        Expr::col((a("u"), users::Column::Id))
            .equals((a("ia"), issue_assignees::Column::AssigneeId)),
    )
    .expr_as(
        Expr::col((a("u"), users::Column::DisplayName)),
        "display_name",
    )
    .expr_as(Expr::col((a("u"), users::Column::Id)), "assignee_id")
    .expr_as(avatar_expr(), "avatar_url")
    .expr_as(Func::sum(estimate_value()), "total_estimates")
    .expr_as(
        filtered_sum(estimate_value(), completed_cond()),
        "completed_estimates",
    )
    .expr_as(
        filtered_sum(estimate_value(), pending_cond()),
        "pending_estimates",
    )
    .add_group_by([Expr::col((a("u"), users::Column::DisplayName)).into()])
    .add_group_by([Expr::col((a("u"), users::Column::Id)).into()])
    .add_group_by([avatar_expr()])
    .order_by((a("u"), users::Column::DisplayName), Order::Asc);
    stmt
}

/// Points label distribution (`base.py:901-931`). LEFT-joins
/// `estimate_points` like the assignee query above (PIDASHCONV-1185).
fn points_label_stmt(
    slug: &str,
    project_id: uuid::Uuid,
    cycle_id: uuid::Uuid,
) -> sea_orm::sea_query::SelectStatement {
    use pidash_db::entity::{estimate_points, issue_labels, issues, labels};
    use sea_orm::sea_query::{Alias, Expr, Func, JoinType, Order};
    let a = Alias::new;
    let mut stmt = dist_scope_stmt(slug, project_id, cycle_id);
    stmt.join_as(
        JoinType::LeftJoin,
        estimate_points::Entity,
        a("ep"),
        Expr::col((a("ep"), estimate_points::Column::Id))
            .equals((a("i"), issues::Column::EstimatePointId)),
    )
    .join_as(
        JoinType::LeftJoin,
        issue_labels::Entity,
        a("il"),
        Expr::col((a("il"), issue_labels::Column::IssueId)).equals((a("i"), issues::Column::Id)),
    )
    .join_as(
        JoinType::LeftJoin,
        labels::Entity,
        a("l"),
        Expr::col((a("l"), labels::Column::Id)).equals((a("il"), issue_labels::Column::LabelId)),
    )
    .expr_as(Expr::col((a("l"), labels::Column::Name)), "label_name")
    .expr_as(Expr::col((a("l"), labels::Column::Color)), "color")
    .expr_as(Expr::col((a("l"), labels::Column::Id)), "label_id")
    .expr_as(Func::sum(estimate_value()), "total_estimates")
    .expr_as(
        filtered_sum(estimate_value(), completed_cond()),
        "completed_estimates",
    )
    .expr_as(
        filtered_sum(estimate_value(), pending_cond()),
        "pending_estimates",
    )
    .add_group_by([Expr::col((a("l"), labels::Column::Name)).into()])
    .add_group_by([Expr::col((a("l"), labels::Column::Color)).into()])
    .add_group_by([Expr::col((a("l"), labels::Column::Id)).into()])
    .order_by((a("l"), labels::Column::Name), Order::Asc);
    stmt
}

/// Issues assignee distribution (`base.py:942-998`): `Count` over the
/// join key (NULL-skipping, like Django's `Count("assignee_id")`).
fn issues_assignee_stmt(
    slug: &str,
    project_id: uuid::Uuid,
    cycle_id: uuid::Uuid,
) -> sea_orm::sea_query::SelectStatement {
    use pidash_db::entity::{issue_assignees, issues, users};
    use sea_orm::sea_query::{Alias, Expr, JoinType, Order};
    let a = Alias::new;
    let mut stmt = dist_scope_stmt(slug, project_id, cycle_id);
    stmt.join_as(
        JoinType::LeftJoin,
        issue_assignees::Entity,
        a("ia"),
        Expr::col((a("ia"), issue_assignees::Column::IssueId)).equals((a("i"), issues::Column::Id)),
    )
    .join_as(
        JoinType::LeftJoin,
        users::Entity,
        a("u"),
        Expr::col((a("u"), users::Column::Id))
            .equals((a("ia"), issue_assignees::Column::AssigneeId)),
    )
    .expr_as(
        Expr::col((a("u"), users::Column::DisplayName)),
        "display_name",
    )
    .expr_as(Expr::col((a("u"), users::Column::Id)), "assignee_id")
    .expr_as(avatar_expr(), "avatar_url")
    .expr_as(
        filtered_count(
            Expr::col((a("u"), users::Column::Id)).into(),
            issues_total_cond(),
        ),
        "total_issues",
    )
    .expr_as(
        filtered_count(
            Expr::col((a("u"), users::Column::Id)).into(),
            completed_cond(),
        ),
        "completed_issues",
    )
    .expr_as(
        filtered_count(
            Expr::col((a("u"), users::Column::Id)).into(),
            pending_cond(),
        ),
        "pending_issues",
    )
    .add_group_by([Expr::col((a("u"), users::Column::DisplayName)).into()])
    .add_group_by([Expr::col((a("u"), users::Column::Id)).into()])
    .add_group_by([avatar_expr()])
    .order_by((a("u"), users::Column::DisplayName), Order::Asc);
    stmt
}

/// Issues label distribution (`base.py:1000-1036`).
fn issues_label_stmt(
    slug: &str,
    project_id: uuid::Uuid,
    cycle_id: uuid::Uuid,
) -> sea_orm::sea_query::SelectStatement {
    use pidash_db::entity::{issue_labels, issues, labels};
    use sea_orm::sea_query::{Alias, Expr, JoinType, Order};
    let a = Alias::new;
    let mut stmt = dist_scope_stmt(slug, project_id, cycle_id);
    stmt.join_as(
        JoinType::LeftJoin,
        issue_labels::Entity,
        a("il"),
        Expr::col((a("il"), issue_labels::Column::IssueId)).equals((a("i"), issues::Column::Id)),
    )
    .join_as(
        JoinType::LeftJoin,
        labels::Entity,
        a("l"),
        Expr::col((a("l"), labels::Column::Id)).equals((a("il"), issue_labels::Column::LabelId)),
    )
    .expr_as(Expr::col((a("l"), labels::Column::Name)), "label_name")
    .expr_as(Expr::col((a("l"), labels::Column::Color)), "color")
    .expr_as(Expr::col((a("l"), labels::Column::Id)), "label_id")
    .expr_as(
        filtered_count(
            Expr::col((a("l"), labels::Column::Id)).into(),
            issues_total_cond(),
        ),
        "total_issues",
    )
    .expr_as(
        filtered_count(
            Expr::col((a("l"), labels::Column::Id)).into(),
            completed_cond(),
        ),
        "completed_issues",
    )
    .expr_as(
        filtered_count(
            Expr::col((a("l"), labels::Column::Id)).into(),
            pending_cond(),
        ),
        "pending_issues",
    )
    .add_group_by([Expr::col((a("l"), labels::Column::Name)).into()])
    .add_group_by([Expr::col((a("l"), labels::Column::Color)).into()])
    .add_group_by([Expr::col((a("l"), labels::Column::Id)).into()])
    .order_by((a("l"), labels::Column::Name), Order::Asc);
    stmt
}

/// Total burndown points (`base.py` total over the cycle scope with the
/// estimate join and the `ep.value IS NOT NULL` guard).
fn total_points_stmt(
    slug: &str,
    project_id: uuid::Uuid,
    cycle_id: uuid::Uuid,
) -> sea_orm::sea_query::SelectStatement {
    use pidash_db::entity::{estimate_points, issues};
    use sea_orm::sea_query::{Alias, Expr, Func, JoinType};
    let a = Alias::new;
    let mut stmt = dist_scope_stmt(slug, project_id, cycle_id);
    stmt.join_as(
        JoinType::InnerJoin,
        estimate_points::Entity,
        a("ep"),
        Expr::col((a("ep"), estimate_points::Column::Id))
            .equals((a("i"), issues::Column::EstimatePointId)),
    )
    .and_where(Expr::col((a("ep"), estimate_points::Column::Value)).is_not_null())
    .expr_as(Func::sum(estimate_value()), "total_points");
    stmt
}

/// Run a built statement into typed rows, decoded by name.
async fn query_built<T>(
    db: &sea_orm::DatabaseConnection,
    stmt: &sea_orm::sea_query::SelectStatement,
) -> Result<Vec<T>, Denial>
where
    T: sea_orm::FromQueryResult,
{
    use sea_orm::ConnectionTrait;
    let rows = db
        .query_all(sea_orm::DbBackend::Postgres.build(stmt))
        .await
        .map_err(|_| Denial::ServerError)?;
    rows.iter()
        .map(|row| T::from_query_result(row, "").map_err(|_| Denial::ServerError))
        .collect()
}

/// Run a built statement into one typed row, decoded by name.
async fn query_one_built<T>(
    db: &sea_orm::DatabaseConnection,
    stmt: &sea_orm::sea_query::SelectStatement,
) -> Result<Option<T>, Denial>
where
    T: sea_orm::FromQueryResult,
{
    use sea_orm::ConnectionTrait;
    let rows = db
        .query_all(sea_orm::DbBackend::Postgres.build(stmt))
        .await
        .map_err(|_| Denial::ServerError)?;
    rows.first()
        .map(|row| T::from_query_result(row, "").map_err(|_| Denial::ServerError))
        .transpose()
}

/// Burndown completion rows, points flavor
/// (`analytics_plot.py:166-180`, cycle path): every bridged issue with
/// an estimate, datestamp from `TruncDate(completed_at)` in the actor's
/// active zone (`TimezoneMixin` activates `user_timezone`), bound as
/// `$4`; the caller drops `NULL` dates (`item["date"] is not None`).
/// sea-query cannot express the `AT TIME ZONE` conversion, so the
/// statement stays hand-written (see the `// raw-sql:` at the call).
const BURNDOWN_POINTS_SQL: &str =
    "SELECT (i.completed_at AT TIME ZONE $4)::date AS d, ep.value AS v \
     FROM issues i \
     JOIN cycle_issues ci ON ci.issue_id = i.id AND ci.cycle_id = $3 AND ci.deleted_at IS NULL \
     JOIN workspaces w ON w.id = i.workspace_id AND w.slug = $1 \
     JOIN projects p ON p.id = i.project_id AND p.id = $2 AND p.archived_at IS NULL \
     LEFT JOIN states s ON s.id = i.state_id \
     JOIN estimate_points ep ON ep.id = i.estimate_point_id \
     WHERE i.deleted_at IS NULL AND i.archived_at IS NULL AND i.is_draft = FALSE \
     AND NOT (s.group = 'triage') AND ep.value IS NOT NULL \
     ORDER BY d";

/// Burndown completion rows, issues flavor
/// (`analytics_plot.py:182-193`, cycle path): per-day completed counts,
/// truncated in the actor's active zone (bound as `$4`).
const BURNDOWN_ISSUES_SQL: &str =
    "SELECT (i.completed_at AT TIME ZONE $4)::date AS d, COUNT(*) AS c \
     FROM issues i \
     JOIN cycle_issues ci ON ci.issue_id = i.id AND ci.cycle_id = $3 AND ci.deleted_at IS NULL \
     JOIN workspaces w ON w.id = i.workspace_id AND w.slug = $1 \
     JOIN projects p ON p.id = i.project_id AND p.id = $2 AND p.archived_at IS NULL \
     LEFT JOIN states s ON s.id = i.state_id \
     WHERE i.deleted_at IS NULL AND i.archived_at IS NULL AND i.is_draft = FALSE \
     AND NOT (s.group = 'triage') \
     GROUP BY d ORDER BY d";

// ---------------------------------------------------------------------------
// Row shaping (`.values()` + annotation key order)
// ---------------------------------------------------------------------------

fn json_float(v: f64) -> Value {
    serde_json::Number::from_f64(v)
        .map(Value::Number)
        .unwrap_or(Value::Null)
}

fn opt_str(v: Option<String>) -> Value {
    v.map(Value::String).unwrap_or(Value::Null)
}

/// Assignee row, issues flavor (`:940-998` key order).
fn shape_assignee_issues(
    display_name: Option<String>,
    assignee_id: Option<uuid::Uuid>,
    avatar_url: Option<String>,
    total: i64,
    completed: i64,
    pending: i64,
) -> Map<String, Value> {
    let mut m = Map::with_capacity(6);
    m.insert("display_name".to_owned(), opt_str(display_name));
    m.insert(
        "assignee_id".to_owned(),
        assignee_id
            .map(|id| Value::String(id.to_string()))
            .unwrap_or(Value::Null),
    );
    m.insert("avatar_url".to_owned(), opt_str(avatar_url));
    m.insert("total_issues".to_owned(), total.into());
    m.insert("completed_issues".to_owned(), completed.into());
    m.insert("pending_issues".to_owned(), pending.into());
    m
}

/// Assignee row, points flavor (`:873-899` key order).
fn shape_assignee_points(
    display_name: Option<String>,
    assignee_id: Option<uuid::Uuid>,
    avatar_url: Option<String>,
    total: Option<f64>,
    completed: Option<f64>,
    pending: Option<f64>,
) -> Map<String, Value> {
    let mut m = Map::with_capacity(6);
    m.insert("display_name".to_owned(), opt_str(display_name));
    m.insert(
        "assignee_id".to_owned(),
        assignee_id
            .map(|id| Value::String(id.to_string()))
            .unwrap_or(Value::Null),
    );
    m.insert("avatar_url".to_owned(), opt_str(avatar_url));
    m.insert(
        "total_estimates".to_owned(),
        total.map(json_float).unwrap_or(Value::Null),
    );
    m.insert(
        "completed_estimates".to_owned(),
        completed.map(json_float).unwrap_or(Value::Null),
    );
    m.insert(
        "pending_estimates".to_owned(),
        pending.map(json_float).unwrap_or(Value::Null),
    );
    m
}

/// Label row, issues flavor (`:1000-1036` key order).
fn shape_label_issues(
    label_name: Option<String>,
    color: Option<String>,
    label_id: Option<uuid::Uuid>,
    total: i64,
    completed: i64,
    pending: i64,
) -> Map<String, Value> {
    let mut m = Map::with_capacity(6);
    m.insert("label_name".to_owned(), opt_str(label_name));
    m.insert("color".to_owned(), opt_str(color));
    m.insert(
        "label_id".to_owned(),
        label_id
            .map(|id| Value::String(id.to_string()))
            .unwrap_or(Value::Null),
    );
    m.insert("total_issues".to_owned(), total.into());
    m.insert("completed_issues".to_owned(), completed.into());
    m.insert("pending_issues".to_owned(), pending.into());
    m
}

/// Label row, points flavor (`:908-931` key order).
fn shape_label_points(
    label_name: Option<String>,
    color: Option<String>,
    label_id: Option<uuid::Uuid>,
    total: Option<f64>,
    completed: Option<f64>,
    pending: Option<f64>,
) -> Map<String, Value> {
    let mut m = Map::with_capacity(6);
    m.insert("label_name".to_owned(), opt_str(label_name));
    m.insert("color".to_owned(), opt_str(color));
    m.insert(
        "label_id".to_owned(),
        label_id
            .map(|id| Value::String(id.to_string()))
            .unwrap_or(Value::Null),
    );
    m.insert(
        "total_estimates".to_owned(),
        total.map(json_float).unwrap_or(Value::Null),
    );
    m.insert(
        "completed_estimates".to_owned(),
        completed.map(json_float).unwrap_or(Value::Null),
    );
    m.insert(
        "pending_estimates".to_owned(),
        pending.map(json_float).unwrap_or(Value::Null),
    );
    m
}

// ---------------------------------------------------------------------------
// Burndown (`utils/analytics_plot.py:123-264`, cycle path)
// ---------------------------------------------------------------------------

/// One chart series: `(date, pending)` with future days `None`, in
/// chronological order. `completions` carries only non-null dates (the
/// `item["date"] is not None` filter); values are floats for the
/// `"points"` plot and ints for `"issues"`.
fn burndown_series(
    start: NaiveDate,
    end: NaiveDate,
    today: NaiveDate,
    total: f64,
    completions: &[(NaiveDate, f64)],
    as_float: bool,
) -> Vec<(String, Value)> {
    if end < start {
        return Vec::new();
    }
    let days = (end - start).num_days();
    (0..=days)
        .map(|offset| {
            let day = start + chrono::Days::new(offset as u64);
            let done: f64 = completions
                .iter()
                .filter(|(d, _)| *d <= day)
                .map(|(_, v)| v)
                .sum();
            let value = if day > today {
                Value::Null
            } else if as_float {
                json_float(total - done)
            } else {
                Value::Number(((total - done) as i64).into())
            };
            (day.to_string(), value)
        })
        .collect()
}

fn burndown_object(series: Vec<(String, Value)>) -> Value {
    let mut m = Map::with_capacity(series.len());
    for (day, value) in series {
        m.insert(day, value);
    }
    Value::Object(m)
}

/// Final envelope order (`base.py:1042-1049`).
fn final_envelope(assignees: Vec<Value>, labels: Vec<Value>, chart: Value) -> String {
    let mut out = Map::with_capacity(3);
    out.insert("assignees".to_owned(), Value::Array(assignees));
    out.insert("labels".to_owned(), Value::Array(labels));
    out.insert("completion_chart".to_owned(), chart);
    serde_json::to_string(&Value::Object(out)).unwrap_or("null".to_owned())
}

/// Snapshot envelope order (`base.py:823-827`): labels first.
fn snapshot_envelope(distribution: &Map<String, Value>) -> String {
    let mut out = Map::with_capacity(3);
    out.insert(
        "labels".to_owned(),
        distribution
            .get("labels")
            .cloned()
            .unwrap_or(Value::Array(vec![])),
    );
    out.insert(
        "assignees".to_owned(),
        distribution
            .get("assignees")
            .cloned()
            .unwrap_or(Value::Array(vec![])),
    );
    out.insert(
        "completion_chart".to_owned(),
        distribution
            .get("completion_chart")
            .cloned()
            .unwrap_or(Value::Object(Map::new())),
    );
    serde_json::to_string(&Value::Object(out)).unwrap_or("null".to_owned())
}

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

/// `CycleAnalyticsEndpoint.get` (`base.py:788-1049`).
async fn analytics_get(
    State(state): State<AppState>,
    Path((slug, project_raw, cycle_raw)): Path<(String, String, String)>,
    Query(query): Query<HashMap<String, String>>,
    extension: Option<axum::Extension<SessionHandle>>,
    req: axum::extract::Request,
) -> Result<Response, Denial> {
    // `<uuid:cycle_id>` never matches a non-UUID tail in Django: proxy
    // so the routing 404 comes from Django byte-for-byte.
    let cycle_id: uuid::Uuid = match cycle_raw.parse() {
        Ok(id) if super::is_canonical_uuid(&cycle_raw) => id,
        _ => return Ok(crate::edge::proxy(State(state), req).await),
    };
    let resolved = actor(&state, extension).await?;
    let pool = pool_of(&state)?.clone();
    // `_rewrite_project_kwarg` resolves a non-UUID project_id before the
    // permission gate (`app/views/base.py:202-204`): a matching identifier
    // becomes the project UUID, a miss is a 404 (`Project not found`).
    let project_id = resolve_project_id(&pool, &slug, &project_raw).await?;
    let (workspace_role, project_role) =
        membership(&pool, &slug, &project_id, &resolved.id).await?;
    let row = gate_for("GET", ANALYTICS_PATH).expect("analytics route must have a gate");
    let facts = gate_facts(&slug, workspace_role, project_role);
    match decide_gate(&row.gate, &tenant_context(&slug), &facts) {
        GateOutcome::Allow => {}
        GateOutcome::Deny => return Err(Denial::Forbidden),
        GateOutcome::Unauthenticated => return Err(Denial::Unauthorized),
    }

    // `request.GET.get("type", "issues")` — last wins on repeats.
    let analytic_type = query.get("type").map(String::as_str).unwrap_or("issues");

    // `Project` has a `"points"` estimate (`base.py:832-837`).
    // `Project.objects` is the soft-deletion manager, so a soft-deleted
    // project is excluded.
    use pidash_db::entity::estimates as est_entity;
    use pidash_db::entity::projects as proj_entity;
    use pidash_db::entity::workspaces as ws_entity;
    use sea_orm::{ColumnTrait, EntityTrait, JoinType, PaginatorTrait, QueryFilter, RelationTrait};
    let estimate_type = proj_entity::Entity::find()
        .join(JoinType::InnerJoin, proj_entity::Relation::Workspaces.def())
        .join(
            JoinType::InnerJoin,
            proj_entity::Entity::belongs_to(est_entity::Entity)
                .from(proj_entity::Column::EstimateId)
                .to(est_entity::Column::Id)
                .into(),
        )
        .filter(ws_entity::Column::Slug.eq(slug.as_str()))
        .filter(est_entity::Column::Type.eq("points"))
        .filter(proj_entity::Column::Id.eq(project_id))
        .filter(proj_entity::Column::DeletedAt.is_null())
        .exists(&sea_conn(&pool))
        .await
        .map_err(|_| Denial::ServerError)?;

    // The annotation runs on the same fetch as the cycle itself; a
    // missing cycle leaves `None` and the `start_date` dereference
    // 500s (B1) — there is no 404 branch in Python.
    /// Cycle row plus the `total_issues` annotation (distinct bridged
    /// issues that are not archived, not draft, not deleted, on a live
    /// bridge row; no `IssueManager` excludes across the join).
    #[derive(Debug, Clone, sea_orm::FromQueryResult)]
    #[allow(dead_code)]
    struct AnalyticsCycleRow {
        id: uuid::Uuid,
        start_date: Option<chrono::DateTime<chrono::Utc>>,
        end_date: Option<chrono::DateTime<chrono::Utc>>,
        progress_snapshot: Option<Value>,
        total_issues: Option<i64>,
    }
    use pidash_db::entity::cycle_issues as ci_entity;
    use pidash_db::entity::cycles as c_entity;
    use pidash_db::entity::issues as i_entity;
    use sea_orm::sea_query::{Alias, Condition, Expr, Func, Query, SimpleExpr, SubQueryStatement};
    let a = Alias::new;
    let mut count_sub = Query::select();
    count_sub
        .expr_as(
            Func::count_distinct(Expr::col((a("ci"), ci_entity::Column::IssueId))),
            "n",
        )
        .from_as(ci_entity::Entity, a("ci"))
        .join_as(
            JoinType::InnerJoin,
            i_entity::Entity,
            a("i"),
            Condition::all()
                .add(
                    Expr::col((a("i"), i_entity::Column::Id))
                        .equals((a("ci"), ci_entity::Column::IssueId)),
                )
                .add(Expr::col((a("i"), i_entity::Column::DeletedAt)).is_null())
                .add(Expr::col((a("i"), i_entity::Column::ArchivedAt)).is_null())
                .add(Expr::col((a("i"), i_entity::Column::IsDraft)).eq(false)),
        )
        .and_where(
            Expr::col((a("ci"), ci_entity::Column::CycleId))
                .equals((a("cycles"), c_entity::Column::Id)),
        )
        .and_where(Expr::col((a("ci"), ci_entity::Column::DeletedAt)).is_null());
    let total_sub: SimpleExpr = SimpleExpr::SubQuery(
        None,
        Box::new(SubQueryStatement::SelectStatement(count_sub)),
    );
    use sea_orm::QuerySelect;
    let cycle: Option<AnalyticsCycleRow> = c_entity::Entity::find()
        .join(JoinType::InnerJoin, c_entity::Relation::Workspaces.def())
        .filter(ws_entity::Column::Slug.eq(slug.as_str()))
        .filter(c_entity::Column::ProjectId.eq(project_id))
        .filter(c_entity::Column::Id.eq(cycle_id))
        .filter(c_entity::Column::DeletedAt.is_null())
        .select_only()
        .column(c_entity::Column::Id)
        .column(c_entity::Column::StartDate)
        .column(c_entity::Column::EndDate)
        .column(c_entity::Column::ProgressSnapshot)
        .expr_as(total_sub, "total_issues")
        .into_model::<AnalyticsCycleRow>()
        .one(&sea_conn(&pool))
        .await
        .map_err(|_| Denial::ServerError)?;
    let Some(AnalyticsCycleRow {
        start_date,
        end_date,
        progress_snapshot,
        total_issues,
        ..
    }) = cycle
    else {
        return Err(Denial::ServerError);
    };
    // A DB-level `NULL` snapshot reads falsy, like Python `None`.
    let snapshot = progress_snapshot.unwrap_or(Value::Null);
    let (Some(start_date), Some(end_date)) = (start_date, end_date) else {
        return Err(Denial::NoDates);
    };

    // `if cycle.progress_snapshot:` — truthiness of the stored dict.
    if let Value::Object(map) = &snapshot {
        if !map.is_empty() {
            // `distribution = cycle.progress_snapshot.get("distribution", {})`
            // (`base.py:821-822`): the servable envelope is the snapshot's
            // nested `distribution`, not the snapshot itself.
            let distribution = match map.get("distribution") {
                Some(Value::Object(inner)) => inner.clone(),
                _ => Map::new(),
            };
            return Ok(json_response(
                StatusCode::OK,
                snapshot_envelope(&distribution),
            ));
        }
    }

    let today = chrono::Utc::now().date_naive();
    let start = start_date.date_naive();
    let end = end_date.date_naive();

    let mut assignees: Vec<Value> = Vec::new();
    let mut labels: Vec<Value> = Vec::new();
    let mut chart = Value::Object(Map::new());

    if analytic_type == "points" && estimate_type {
        let rows: Vec<AssigneePointsRow> = query_built(
            &sea_conn(&pool),
            &points_assignee_stmt(&slug, project_id, cycle_id),
        )
        .await?;
        assignees = rows
            .into_iter()
            .map(
                |AssigneePointsRow {
                     display_name,
                     assignee_id,
                     avatar_url,
                     total_estimates,
                     completed_estimates,
                     pending_estimates,
                 }| {
                    Value::Object(shape_assignee_points(
                        display_name,
                        assignee_id,
                        avatar_url,
                        total_estimates,
                        completed_estimates,
                        pending_estimates,
                    ))
                },
            )
            .collect();
        let rows: Vec<LabelPointsRow> = query_built(
            &sea_conn(&pool),
            &points_label_stmt(&slug, project_id, cycle_id),
        )
        .await?;
        labels = rows
            .into_iter()
            .map(
                |LabelPointsRow {
                     label_name,
                     color,
                     label_id,
                     total_estimates,
                     completed_estimates,
                     pending_estimates,
                 }| {
                    Value::Object(shape_label_points(
                        label_name,
                        color,
                        label_id,
                        total_estimates,
                        completed_estimates,
                        pending_estimates,
                    ))
                },
            )
            .collect();
        let total_points_row: Option<TotalPointsRow> = query_one_built(
            &sea_conn(&pool),
            &total_points_stmt(&slug, project_id, cycle_id),
        )
        .await?;
        let total_points = total_points_row
            .and_then(|row| row.total_points)
            .unwrap_or(0.0);
        // raw-sql: AT TIME ZONE conversion to a date is not buildable by sea-query (no `AT TIME ZONE` helper); the scope/order match Django's TruncDate.
        let rows: Vec<(Option<NaiveDate>, String)> = sqlx::query_as(BURNDOWN_POINTS_SQL)
            .bind(&slug)
            .bind(project_id)
            .bind(cycle_id)
            .bind(resolved.timezone.name())
            .fetch_all(&pool)
            .await
            .map_err(|_| Denial::ServerError)?;
        let mut completions = Vec::with_capacity(rows.len());
        for (day, value) in rows {
            let (Some(day), Ok(v)) = (day, value.parse::<f64>()) else {
                continue;
            };
            completions.push((day, v));
        }
        chart = burndown_object(burndown_series(
            start,
            end,
            today,
            total_points,
            &completions,
            true,
        ));
    }

    if analytic_type == "issues" {
        let rows: Vec<AssigneeIssuesRow> = query_built(
            &sea_conn(&pool),
            &issues_assignee_stmt(&slug, project_id, cycle_id),
        )
        .await?;
        assignees = rows
            .into_iter()
            .map(
                |AssigneeIssuesRow {
                     display_name,
                     assignee_id,
                     avatar_url,
                     total_issues,
                     completed_issues,
                     pending_issues,
                 }| {
                    Value::Object(shape_assignee_issues(
                        display_name,
                        assignee_id,
                        avatar_url,
                        // FILTER counts are never NULL; the shape
                        // takes plain i64 (Django Count).
                        total_issues.unwrap_or(0),
                        completed_issues.unwrap_or(0),
                        pending_issues.unwrap_or(0),
                    ))
                },
            )
            .collect();
        let rows: Vec<LabelIssuesRow> = query_built(
            &sea_conn(&pool),
            &issues_label_stmt(&slug, project_id, cycle_id),
        )
        .await?;
        labels = rows
            .into_iter()
            .map(
                |LabelIssuesRow {
                     label_name,
                     color,
                     label_id,
                     total_issues,
                     completed_issues,
                     pending_issues,
                 }| {
                    Value::Object(shape_label_issues(
                        label_name,
                        color,
                        label_id,
                        total_issues.unwrap_or(0),
                        completed_issues.unwrap_or(0),
                        pending_issues.unwrap_or(0),
                    ))
                },
            )
            .collect();
        // raw-sql: AT TIME ZONE conversion to a date is not buildable by sea-query (no `AT TIME ZONE` helper); the scope/order match Django's TruncDate.
        let rows: Vec<(Option<NaiveDate>, i64)> = sqlx::query_as(BURNDOWN_ISSUES_SQL)
            .bind(&slug)
            .bind(project_id)
            .bind(cycle_id)
            .bind(resolved.timezone.name())
            .fetch_all(&pool)
            .await
            .map_err(|_| Denial::ServerError)?;
        let completions: Vec<(NaiveDate, f64)> = rows
            .into_iter()
            .filter_map(|(day, count)| day.map(|d| (d, count as f64)))
            .collect();
        chart = burndown_object(burndown_series(
            start,
            end,
            today,
            total_issues.unwrap_or(0) as f64,
            &completions,
            false,
        ));
    }

    Ok(json_response(
        StatusCode::OK,
        final_envelope(assignees, labels, chart),
    ))
}

/// [`AllowFacts`] for the GUEST analytics gate over the fetched roles.
fn gate_facts(
    slug: &str,
    workspace_role: Option<i32>,
    project_role: Option<i32>,
) -> pidash_auth::permissions::allow::AllowFacts {
    use pidash_auth::permissions::{ROLE_ADMIN, ROLE_GUEST, ROLE_MEMBER};
    use pidash_types::WorkspaceId;
    let allowed = [ROLE_ADMIN, ROLE_MEMBER, ROLE_GUEST];
    pidash_auth::permissions::allow::AllowFacts {
        workspace: WorkspaceId::from(slug),
        authenticated: true,
        is_workspace_member: workspace_role.is_some(),
        has_allowed_workspace_role: workspace_role.is_some_and(|r| allowed.contains(&r)),
        is_creator: false,
        has_allowed_project_role: project_role.is_some_and(|r| allowed.contains(&r)),
        is_project_member: project_role.is_some(),
        is_workspace_admin: workspace_role == Some(ROLE_ADMIN),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Value {
        let raw = include_str!("../../../../fixtures/app_cycles/analytics.json");
        serde_json::from_str(raw).expect("analytics fixture must parse")
    }

    #[test]
    fn final_envelope_order_is_assignees_labels_chart() {
        let body = final_envelope(vec![], vec![], Value::Object(Map::new()));
        assert_eq!(
            body,
            r#"{"assignees":[],"labels":[],"completion_chart":{}}"#
        );
        let owned: Value = serde_json::from_str(&body).expect("envelope");
        let order: Vec<&str> = owned
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(order, ["assignees", "labels", "completion_chart"]);
    }

    #[test]
    fn snapshot_envelope_order_is_labels_first_with_defaults() {
        let dist: Map<String, Value> =
            serde_json::from_str(r#"{"assignees":[{"display_name":"a"}],"distribution_noise":1}"#)
                .expect("map");
        let body = snapshot_envelope(&dist);
        let parsed: Value = serde_json::from_str(&body).expect("envelope");
        let order: Vec<&str> = parsed
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(order, ["labels", "assignees", "completion_chart"]);
        assert_eq!(parsed["labels"], Value::Array(vec![]));
        assert_eq!(parsed["completion_chart"], Value::Object(Map::new()));
        assert_eq!(parsed["assignees"][0]["display_name"], "a");
    }

    #[test]
    fn row_shapes_follow_values_plus_annotation_order() {
        let id = "12345678-1234-1234-1234-123456789abc".parse().ok();
        let row = shape_assignee_issues(Some("Ann".into()), id, None, 3, 1, 2);
        let order: Vec<&str> = row.keys().map(String::as_str).collect();
        assert_eq!(
            order,
            [
                "display_name",
                "assignee_id",
                "avatar_url",
                "total_issues",
                "completed_issues",
                "pending_issues"
            ]
        );
        assert_eq!(
            serde_json::to_string(&Value::Object(row)).expect("row"),
            r#"{"display_name":"Ann","assignee_id":"12345678-1234-1234-1234-123456789abc","avatar_url":null,"total_issues":3,"completed_issues":1,"pending_issues":2}"#
        );
        let row = shape_assignee_points(None, None, Some("/x".into()), None, Some(0.0), Some(2.5));
        assert_eq!(
            serde_json::to_string(&Value::Object(row)).expect("row"),
            r#"{"display_name":null,"assignee_id":null,"avatar_url":"/x","total_estimates":null,"completed_estimates":0.0,"pending_estimates":2.5}"#
        );
        let row = shape_label_issues(Some("bug".into()), Some("#fff".into()), id, 2, 2, 0);
        let order: Vec<&str> = row.keys().map(String::as_str).collect();
        assert_eq!(
            order,
            [
                "label_name",
                "color",
                "label_id",
                "total_issues",
                "completed_issues",
                "pending_issues"
            ]
        );
        let row = shape_label_points(None, None, None, Some(8.0), None, None);
        assert_eq!(
            serde_json::to_string(&Value::Object(row)).expect("row"),
            r#"{"label_name":null,"color":null,"label_id":null,"total_estimates":8.0,"completed_estimates":null,"pending_estimates":null}"#
        );
    }

    #[test]
    fn burndown_counts_down_and_nulls_the_future() {
        let start = NaiveDate::from_ymd_opt(2024, 1, 1).expect("date");
        let end = NaiveDate::from_ymd_opt(2024, 1, 4).expect("date");
        let today = NaiveDate::from_ymd_opt(2024, 1, 2).expect("date");
        let series = burndown_series(
            start,
            end,
            today,
            3.0,
            &[(NaiveDate::from_ymd_opt(2024, 1, 2).expect("date"), 1.0)],
            false,
        );
        assert_eq!(
            series
                .iter()
                .map(|(d, v)| (d.clone(), v.to_string()))
                .collect::<Vec<_>>(),
            vec![
                ("2024-01-01".to_owned(), "3".to_owned()),
                ("2024-01-02".to_owned(), "2".to_owned()),
                ("2024-01-03".to_owned(), "null".to_owned()),
                ("2024-01-04".to_owned(), "null".to_owned()),
            ]
        );
        let series = burndown_series(start, end, end, 18.0, &[], true);
        assert_eq!(series[0].1.to_string(), "18.0");
        assert!(burndown_series(end, start, end, 1.0, &[], true).is_empty());
    }

    #[test]
    fn analytics_gate_is_guest_with_no_dates_400() {
        use pidash_auth::permissions::{ROLE_ADMIN, ROLE_GUEST, ROLE_MEMBER};
        let row = gate_for("GET", ANALYTICS_PATH).expect("analytics route must have a gate");
        let scope = tenant_context("acme");
        let allow = |role: Option<i32>| {
            // Anonymous callers never reach the gate in the handler
            // (`actor()` 401s first); model that here explicitly.
            let Some(role) = role else {
                return decide_gate(
                    &row.gate,
                    &scope,
                    &pidash_auth::permissions::allow::AllowFacts {
                        workspace: pidash_types::WorkspaceId::from("acme"),
                        authenticated: false,
                        is_workspace_member: false,
                        has_allowed_workspace_role: false,
                        is_creator: false,
                        has_allowed_project_role: false,
                        is_project_member: false,
                        is_workspace_admin: false,
                    },
                );
            };
            decide_gate(
                &row.gate,
                &scope,
                &gate_facts("acme", Some(role), Some(role)),
            )
        };
        // Same-role-in-both-projects is the contract-suite world; the
        // gate test below covers the matrix corners through GATES.
        let _ = (ROLE_ADMIN, ROLE_MEMBER);
        assert_eq!(allow(Some(ROLE_GUEST)), GateOutcome::Allow);
        assert_eq!(allow(None), GateOutcome::Unauthenticated);
        assert_eq!(
            Denial::NoDates.status_and_body(),
            (StatusCode::BAD_REQUEST, NO_DATES_BODY.to_owned())
        );
        assert_eq!(fixture()["branches"][0]["output"]["status"], 400);
        assert_eq!(
            serde_json::to_string(&fixture()["branches"][0]["output"]["body"]).expect("body"),
            NO_DATES_BODY
        );
    }

    fn render(stmt: &sea_orm::sea_query::SelectStatement) -> String {
        use sea_orm::sea_query::PostgresQueryBuilder;
        stmt.to_string(PostgresQueryBuilder)
    }

    fn fixture_ids() -> (uuid::Uuid, uuid::Uuid) {
        (
            uuid::Uuid::parse_str("12345678-1234-1234-1234-123456789abc").expect("id"),
            uuid::Uuid::parse_str("22345678-2234-2234-2234-223456789abc").expect("id"),
        )
    }

    #[test]
    fn sql_carries_scope_filters_and_avatar_case() {
        let (project_id, cycle_id) = fixture_ids();
        for sql in [
            render(&points_assignee_stmt("acme", project_id, cycle_id)),
            render(&points_label_stmt("acme", project_id, cycle_id)),
            render(&issues_assignee_stmt("acme", project_id, cycle_id)),
            render(&issues_label_stmt("acme", project_id, cycle_id)),
            render(&total_points_stmt("acme", project_id, cycle_id)),
        ] {
            for fragment in [
                "cycle_id",
                "\"deleted_at\" IS NULL",
                "\"slug\"",
                "archived_at",
                "\"group\" <> 'triage'",
            ] {
                assert!(
                    sql.contains(fragment),
                    "distribution SQL must carry {fragment}: {sql}"
                );
            }
        }
        assert!(render(&points_assignee_stmt("acme", project_id, cycle_id))
            .contains("double precision"));
        assert!(render(&issues_assignee_stmt("acme", project_id, cycle_id)).contains("COUNT"));
        assert!(render(&points_assignee_stmt("acme", project_id, cycle_id))
            .contains("/api/assets/v2/static/"));
        // PIDASHCONV-1184: only the completion buckets truncate in the
        // actor's active zone; the range endpoints and "today" stay UTC.
        assert!(BURNDOWN_POINTS_SQL.contains("AT TIME ZONE $4"));
        assert!(BURNDOWN_ISSUES_SQL.contains("AT TIME ZONE $4"));
    }

    #[test]
    fn distribution_sql_joins_before_where() {
        // PIDASHCONV-490: the four distribution builders once emitted
        // `... WHERE ... LEFT JOIN ...`, which Postgres rejects — every
        // analytics request answered 500. SQL joins before it filters,
        // so the last JOIN must precede WHERE, which must precede
        // GROUP BY.
        let (project_id, cycle_id) = fixture_ids();
        for sql in [
            render(&points_assignee_stmt("acme", project_id, cycle_id)),
            render(&points_label_stmt("acme", project_id, cycle_id)),
            render(&issues_assignee_stmt("acme", project_id, cycle_id)),
            render(&issues_label_stmt("acme", project_id, cycle_id)),
        ] {
            let last_join = sql.rfind("JOIN").expect("distribution joins");
            let where_at = sql.rfind("WHERE").expect("distribution filter");
            let group_at = sql.find("GROUP BY").expect("distribution grouping");
            assert!(
                last_join < where_at && where_at < group_at,
                "distribution SQL must order JOIN ... WHERE ... GROUP BY: {sql}"
            );
        }
    }

    #[test]
    fn cycle_total_annotation_carries_no_manager_excludes() {
        // `base.py:792-804`: the `total_issues` annotation filters only
        // archived/draft/deleted (+ live bridge). The join is not
        // queried through `issue_objects`, so no triage and no
        // project-archive predicate — unlike the distribution scope.
        use pidash_db::entity::{cycle_issues, cycles, issues};
        use sea_orm::sea_query::{Alias, Condition, Expr, Func, JoinType, Query};
        let a = Alias::new;
        let mut count_sub = Query::select();
        count_sub
            .expr_as(
                Func::count_distinct(Expr::col((a("ci"), cycle_issues::Column::IssueId))),
                "n",
            )
            .from_as(cycle_issues::Entity, a("ci"))
            .join_as(
                JoinType::InnerJoin,
                issues::Entity,
                a("i"),
                Condition::all()
                    .add(
                        Expr::col((a("i"), issues::Column::Id))
                            .equals((a("ci"), cycle_issues::Column::IssueId)),
                    )
                    .add(Expr::col((a("i"), issues::Column::DeletedAt)).is_null())
                    .add(Expr::col((a("i"), issues::Column::ArchivedAt)).is_null())
                    .add(Expr::col((a("i"), issues::Column::IsDraft)).eq(false)),
            )
            .and_where(
                Expr::col((a("ci"), cycle_issues::Column::CycleId))
                    .equals((a("cycles"), cycles::Column::Id)),
            )
            .and_where(Expr::col((a("ci"), cycle_issues::Column::DeletedAt)).is_null());
        let sub_sql = render(&count_sub);
        for fragment in [
            "\"deleted_at\" IS NULL",
            "\"archived_at\" IS NULL",
            "\"is_draft\" = FALSE",
        ] {
            assert!(
                sub_sql.contains(fragment),
                "total_issues subquery must carry {fragment}: {sub_sql}"
            );
        }
        for fragment in ["triage", "projects", "states"] {
            assert!(
                !sub_sql.contains(fragment),
                "total_issues subquery must not carry {fragment}: {sub_sql}"
            );
        }
    }
}
