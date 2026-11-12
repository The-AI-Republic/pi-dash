//! Cycle progress handler (D-27, stage 5, PIDASHCONV-410).
//!
//! Ports `CycleProgressEndpoint.get`
//! (`apps/api/pi_dash/app/views/cycle/base.py:658-783`, drift baseline
//! `01a93e17`) with identical URL path, status codes and JSON bytes:
//!
//! - `GET workspaces/<slug>/projects/<id>/cycles/<uuid>/progress/`
//!   (`app/urls/cycle.py`, routes 13-14 family).
//!
//! Handler notes (all verified against the Python source):
//! - Gate is GUEST (`base.py:659`, [`crate::app_cycles::gates`] row
//!   `GET .../progress/`): Django-session authN first (anon 401), then
//!   the `@allow_permission` PROJECT check (403), then the body.
//! - `project_id` is a `<str:>` converter: a non-UUID value reaches the
//!   view, where the ORM UUID validation raises `ValidationError` and
//!   `handle_exception` answers 400
//!   `{"error":"Please provide valid detail"}` (`app/views/base.py`).
//! - `cycle_id` is a `<uuid:>` converter: a non-UUID tail never reaches
//!   the view (Django routing 404). Axum captures any segment, so the
//!   handler proxies those tails to Django instead of inventing a 400.
//! - The cycle lookup uses `Cycle.objects` (`base.py:661`), the
//!   soft-deletion manager (`mixins.py:61-67`), so a soft-deleted cycle
//!   reads as missing and the view answers 404 `{"error":"Cycle not
//!   found"}` like an unknown id.
//! - Estimate aggregates (`:664-711`) scope to issues whose
//!   `estimate_point.estimate.type` is `"points"` with a live bridge
//!   row, plus the `IssueManager` base excludes (not triage, not
//!   archived, live project, not draft). Per-group `Case`/`Sum`s with no
//!   default come back `NULL` on empty and render through `or 0`
//!   (`:769-773`): `None` AND `0.0` both answer int `0`; only nonzero
//!   sums render as floats. `total` uses `Sum(default=0)` (`:710`) and
//!   is NOT passed through `or 0` (`:774`): empty answers int `0`, a
//!   real zero sum answers `0.0`.
//! - Counts (`:712-765`): when `progress_snapshot` is truthy the six
//!   counts come from the snapshot keys with `.get(key, 0)` defaults;
//!   otherwise six live `COUNT`s run (same bridge + manager scope, no
//!   archived/draft guards — differs from the list counts, ported).
//! - Envelope key order is the Python literal order (`:767-783`),
//!   including the `total_issues` second quirk and the singular
//!   aggregate names mapped to plural keys.
//!
//! Fixture oracle: F-C27-08
//! (`rust-api/fixtures/app_cycles/progress.json` + `TRACE.md`); the unit
//! tests below pin the envelope order, the `or 0` rule and the snapshot
//! branch against that file so transcription drift fails the build.
//!
//! Ported bugs (translation, don't redesign; also listed in the PR):
//! - B1 (refuted): the lookup was ported with no `deleted_at` guard on a
//!   false "plain manager" premise; Django's `Cycle.objects` soft-deletion
//!   manager makes a soft-deleted cycle answer 404, not 200.
//! - B2: the estimate aggregate and the live counts carry no
//!   archived/is-draft guards beyond the manager excludes (differs from
//!   the list endpoint, which counts drafts/archived differently).
//! - B3: `0.0` estimate sums collapse to int `0` via `or 0`, while
//!   `total` keeps `0.0` — the two keys disagree on zero by design of
//!   the source, not by rounding.
//!
//! Sibling plumbing mirrors the D-29 `app_views_search` shape:
//! [`owned`] (unowned methods proxy), session [`actor`], exact denial
//! bodies, and manual envelope assembly for DRF key order/bytes.

use std::collections::HashMap;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Router;
use serde_json::{Map, Value};

use crate::middleware::SessionHandle;
use crate::state::AppState;

use super::gates::{decide_gate, gate_for, tenant_context, GateOutcome, ANON_BODY, FORBIDDEN_BODY};
use pidash_compat::response::json_response;
pub use pidash_compat::response::SERVER_ERROR_BODY;

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------

/// Route path template for the progress endpoint, in `app/urls/cycle.py`
/// form (the `GET .../progress/` row also lives in
/// [`crate::app_cycles::gates::GATES`]).
pub const PROGRESS_PATH: &str = "workspaces/<slug>/projects/<id>/cycles/<uuid>/progress/";

/// Register the progress GET route. Nothing else: sibling paths stay
/// unmatched and proxy to Django, and every non-owned method on the
/// owned path falls through to Django too (its 401-anon-before-405 and
/// DRF metadata live there).
pub fn routes() -> Router<AppState> {
    Router::new().route(
        "/api/workspaces/{slug}/projects/{project_id}/cycles/{cycle_id}/progress/",
        owned(axum::routing::get(progress_get), &["GET"]),
    )
}

/// An owned path: listed methods serve from Rust, everything else proxies
/// to Django. OPTIONS proxies too: DRF answers metadata (401 anon / 200
/// authed) where axum would 405. HEAD proxies as well: Django defines no
/// `head` and 405s after auth (axum would auto-serve it from `get`).
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

/// `Cycle not found` 404 (`base.py:661-663`).
pub const CYCLE_NOT_FOUND_BODY: &str = r#"{"error":"Cycle not found"}"#;
/// ORM `ValidationError` 400 (`app/views/base.py:126-130`): a non-UUID
/// `project_id` reaches the view through the `<str:>` converter.
pub const VALIDATION_BODY: &str = r#"{"error":"Please provide valid detail"}"#;
/// `Project.resolve` miss on a non-UUID identifier
/// (`db/models/project.py:213-217`) — `_rewrite_project_kwarg`'s
/// `Http404("Project not found")`.
pub const PROJECT_NOT_FOUND_BODY: &str = r#"{"detail":"Project not found"}"#;

/// Handler denials with byte-exact bodies.
pub enum Denial {
    Unauthorized,
    Forbidden,
    NotFound,
    BadValidation,
    ProjectNotFound,
    ServerError,
}

impl Denial {
    fn status_and_body(&self) -> (StatusCode, String) {
        match self {
            Denial::Unauthorized => (StatusCode::UNAUTHORIZED, ANON_BODY.to_owned()),
            Denial::Forbidden => (StatusCode::FORBIDDEN, FORBIDDEN_BODY.to_owned()),
            Denial::NotFound => (StatusCode::NOT_FOUND, CYCLE_NOT_FOUND_BODY.to_owned()),
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

/// Session auth (`BaseSessionAuthentication` + `IsAuthenticated`):
/// anonymous answers the DRF `NotAuthenticated` body before anything
/// else runs.
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

/// The shared SeaORM connection over an sqlx pool (no I/O — it wraps the
/// pool).
fn sea_conn(pool: &sqlx::PgPool) -> sea_orm::DatabaseConnection {
    pidash_db::pool::sea_orm_connection(pool)
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

/// Membership facts for the GUEST progress gate, resolved with the same
/// row filters Python uses: active, non-deleted rows scoped to the
/// workspace slug (and project id for the project row).
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
// Estimates + live counts (sea-query builders)
// ---------------------------------------------------------------------------

/// One estimate-aggregate row: five per-group sums plus the total.
#[derive(Debug, Clone, Copy, sea_orm::FromQueryResult)]
struct EstimateAgg {
    backlog: Option<f64>,
    unstarted: Option<f64>,
    started: Option<f64>,
    cancelled: Option<f64>,
    completed: Option<f64>,
    total: Option<f64>,
}

/// One live-count row: five per-group counts plus the total.
#[derive(Debug, Clone, Copy, sea_orm::FromQueryResult)]
struct LiveCounts {
    backlog: i64,
    unstarted: i64,
    started: i64,
    cancelled: i64,
    completed: i64,
    total: i64,
}

/// Shared issue scope for every progress query (`base.py:664-671` plus
/// the `IssueManager` base excludes): live bridge row, workspace slug
/// and project of the issue itself, `estimate_point` on a `"points"`
/// estimate, not triage / not archived / live project / not draft.
///
/// The triage exclusion is `NOT (group = 'triage')` over the join —
/// three-valued, so `NULL`-state rows drop with it (the app_issues
/// precedent: `<>` is `NOT (=)` under the same NULL rule). Joined
/// tables carry no `deleted_at` predicate: Django applies the
/// soft-delete manager to the base table only.
fn progress_scope_stmt(
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

/// One-row estimate aggregate (`base.py:664-711`): five per-group
/// `Case`/`Sum`s (NULL on empty — the caller applies `or 0`) plus the
/// `total` `Sum(default=0)` (never NULL — the caller renders it raw).
fn estimates_stmt(
    slug: &str,
    project_id: uuid::Uuid,
    cycle_id: uuid::Uuid,
) -> sea_orm::sea_query::SelectStatement {
    use pidash_db::entity::{estimate_points, estimates, issues, states};
    use sea_orm::sea_query::{Alias, CaseStatement, Condition, Expr, Func, JoinType, SimpleExpr};
    let a = Alias::new;
    let mut stmt = progress_scope_stmt(slug, project_id, cycle_id);
    stmt.join_as(
        JoinType::InnerJoin,
        estimate_points::Entity,
        a("ep"),
        Expr::col((a("ep"), estimate_points::Column::Id))
            .equals((a("i"), issues::Column::EstimatePointId)),
    )
    .join_as(
        JoinType::InnerJoin,
        estimates::Entity,
        a("e"),
        Condition::all()
            .add(
                Expr::col((a("e"), estimates::Column::Id))
                    .equals((a("ep"), estimate_points::Column::EstimateId)),
            )
            .add(Expr::col((a("e"), estimates::Column::Type)).eq("points")),
    );
    let value = Expr::col((a("ep"), estimate_points::Column::Value)).cast_as(a("double precision"));
    let sum_for = |group: &'static str| {
        Func::sum(SimpleExpr::Case(Box::new(
            CaseStatement::new()
                .case(
                    Expr::col((a("s"), states::Column::Group)).eq(group),
                    value.clone(),
                )
                .finally(Expr::val(0.0)),
        )))
    };
    stmt.expr_as(sum_for("backlog"), "backlog")
        .expr_as(sum_for("unstarted"), "unstarted")
        .expr_as(sum_for("started"), "started")
        .expr_as(sum_for("cancelled"), "cancelled")
        .expr_as(sum_for("completed"), "completed")
        .expr_as(
            Func::coalesce([Func::sum(value).into(), Expr::val(0.0).into()]),
            "total",
        );
    stmt
}

/// Six live counts plus the total in one round trip (`base.py:719-765`):
/// same bridge + slug + project + manager scope as the aggregate, one
/// `FILTER` count per state group. Result-identical to the six separate
/// `COUNT` queries; only the result rows cross into the envelope.
/// The `FILTER` counts compile to `COUNT(CASE WHEN ...)`: rows where the
/// condition is NULL count in neither (three-valued as in the raw SQL).
fn live_counts_stmt(
    slug: &str,
    project_id: uuid::Uuid,
    cycle_id: uuid::Uuid,
) -> sea_orm::sea_query::SelectStatement {
    use pidash_db::entity::states;
    use sea_orm::sea_query::{Alias, Asterisk, CaseStatement, Expr, Func, SimpleExpr};
    let a = Alias::new;
    let mut stmt = progress_scope_stmt(slug, project_id, cycle_id);
    let count_for = |group: &'static str| {
        Func::count(SimpleExpr::Case(Box::new(CaseStatement::new().case(
            Expr::col((a("s"), states::Column::Group)).eq(group),
            Expr::val(1),
        ))))
    };
    stmt.expr_as(count_for("backlog"), "backlog")
        .expr_as(count_for("unstarted"), "unstarted")
        .expr_as(count_for("started"), "started")
        .expr_as(count_for("cancelled"), "cancelled")
        .expr_as(count_for("completed"), "completed")
        .expr_as(Func::count(Expr::col(Asterisk)), "total");
    stmt
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

// ---------------------------------------------------------------------------
// Envelope: the `or 0` rule + Python key order
// ---------------------------------------------------------------------------

/// `aggregate[k] or 0` (`base.py:769-773`): `None` and `0.0` both answer
/// int `0`; only nonzero sums stay floats.
fn or_zero(value: Option<f64>) -> Value {
    match value {
        Some(v) if v != 0.0 => json_float(v),
        _ => Value::Number(0.into()),
    }
}

/// `total` is NOT passed through `or 0` (`base.py:774`): Django's
/// `Sum(default=0, output_field=FloatField())` compiles to
/// `COALESCE(SUM(...), 0)` typed `double precision`, so the empty
/// zero answers float `0.0` and renders raw.
fn total_num(value: Option<f64>) -> Value {
    json_float(value.unwrap_or(0.0))
}

fn json_float(v: f64) -> Value {
    serde_json::Number::from_f64(v)
        .map(Value::Number)
        .unwrap_or(Value::Null)
}

/// Envelope in the exact Python literal key order (`base.py:767-783`).
#[allow(clippy::too_many_arguments)]
fn progress_envelope(estimates: [Option<f64>; 5], total: Option<f64>, counts: [i64; 6]) -> String {
    let mut out = Map::with_capacity(12);
    let keys = [
        "backlog_estimate_points",
        "unstarted_estimate_points",
        "started_estimate_points",
        "cancelled_estimate_points",
        "completed_estimate_points",
    ];
    for (key, value) in keys.iter().zip(estimates.iter()) {
        out.insert((*key).to_owned(), or_zero(*value));
    }
    out.insert("total_estimate_points".to_owned(), total_num(total));
    for (key, value) in [
        "backlog_issues",
        "total_issues",
        "completed_issues",
        "cancelled_issues",
        "started_issues",
        "unstarted_issues",
    ]
    .iter()
    .zip(counts.iter())
    {
        out.insert((*key).to_owned(), Value::Number((*value).into()));
    }
    serde_json::to_string(&Value::Object(out)).unwrap_or("null".to_owned())
}

/// Snapshot count lookup: `snapshot.get(key, 0)` (`base.py:713-718`).
fn snapshot_count(snapshot: &Map<String, Value>, key: &str) -> i64 {
    match snapshot.get(key) {
        Some(Value::Number(n)) => n
            .as_i64()
            .unwrap_or_else(|| n.as_f64().unwrap_or(0.0) as i64),
        _ => 0,
    }
}

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

/// `CycleProgressEndpoint.get` (`base.py:660-783`).
async fn progress_get(
    State(state): State<AppState>,
    Path((slug, project_raw, cycle_raw)): Path<(String, String, String)>,
    Query(_query): Query<HashMap<String, String>>,
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
    let user_id = resolved.id;
    // `_rewrite_project_kwarg` resolves a non-UUID project_id before the
    // permission gate (`app/views/base.py:202-204`): a matching identifier
    // becomes the project UUID, a miss is a 404 (`Project not found`).
    let project_id = resolve_project_id(&pool, &slug, &project_raw).await?;
    let (workspace_role, project_role) = membership(&pool, &slug, &project_id, &user_id).await?;
    let row = gate_for("GET", PROGRESS_PATH).expect("progress route must have a gate");
    let facts = gate_facts(&slug, workspace_role, project_role, user_id);
    match decide_gate(&row.gate, &tenant_context(&slug), &facts) {
        GateOutcome::Allow => {}
        GateOutcome::Deny => return Err(Denial::Forbidden),
        GateOutcome::Unauthenticated => return Err(Denial::Unauthorized),
    }

    // `Cycle.objects` is the soft-deletion manager (`mixins.py:61-67`), so
    // a soft-deleted cycle reads as missing: `.first()` returns None and the
    // view answers 404 (`base.py:661-663`). `NULL` snapshot decodes as `None`
    // and reads falsy, like Python.
    use pidash_db::entity::cycles as c_entity;
    use pidash_db::entity::workspaces as ws_entity;
    use sea_orm::{ColumnTrait, EntityTrait, JoinType, QueryFilter, QuerySelect, RelationTrait};
    #[derive(Debug, Clone, sea_orm::FromQueryResult)]
    #[allow(dead_code)]
    struct CycleRow {
        id: uuid::Uuid,
        progress_snapshot: Option<Value>,
    }
    let cycle: Option<CycleRow> = c_entity::Entity::find()
        .join(JoinType::InnerJoin, c_entity::Relation::Workspaces.def())
        .filter(ws_entity::Column::Slug.eq(slug.as_str()))
        .filter(c_entity::Column::ProjectId.eq(project_id))
        .filter(c_entity::Column::Id.eq(cycle_id))
        .filter(c_entity::Column::DeletedAt.is_null())
        .select_only()
        .column(c_entity::Column::Id)
        .column(c_entity::Column::ProgressSnapshot)
        .into_model::<CycleRow>()
        .one(&sea_conn(&pool))
        .await
        .map_err(|_| Denial::ServerError)?;
    let Some(CycleRow {
        progress_snapshot, ..
    }) = cycle
    else {
        return Err(Denial::NotFound);
    };
    let snapshot = progress_snapshot.unwrap_or(Value::Null);

    let agg: Option<EstimateAgg> = query_one_built(
        &sea_conn(&pool),
        &estimates_stmt(&slug, project_id, cycle_id),
    )
    .await?;
    let EstimateAgg {
        backlog,
        unstarted,
        started,
        cancelled,
        completed,
        total,
    } = agg.unwrap_or(EstimateAgg {
        backlog: None,
        unstarted: None,
        started: None,
        cancelled: None,
        completed: None,
        total: None,
    });

    // `if cycle.progress_snapshot:` — truthiness of the stored dict.
    let counts: [i64; 6] = match &snapshot {
        Value::Object(map) if !map.is_empty() => [
            snapshot_count(map, "backlog_issues"),
            snapshot_count(map, "total_issues"),
            snapshot_count(map, "completed_issues"),
            snapshot_count(map, "cancelled_issues"),
            snapshot_count(map, "started_issues"),
            snapshot_count(map, "unstarted_issues"),
        ],
        _ => {
            let row: Option<LiveCounts> = query_one_built(
                &sea_conn(&pool),
                &live_counts_stmt(&slug, project_id, cycle_id),
            )
            .await?;
            let LiveCounts {
                backlog: b,
                unstarted: u,
                started: s,
                cancelled: c,
                completed: d,
                total: t,
            } = row.unwrap_or(LiveCounts {
                backlog: 0,
                unstarted: 0,
                started: 0,
                cancelled: 0,
                completed: 0,
                total: 0,
            });
            // Envelope order is backlog, total, completed, cancelled,
            // started, unstarted (`base.py:775-781`).
            [b, t, d, c, s, u]
        }
    };

    Ok(json_response(
        StatusCode::OK,
        progress_envelope(
            [backlog, unstarted, started, cancelled, completed],
            total,
            counts,
        ),
    ))
}

/// [`AllowFacts`] for the GUEST progress gate over the fetched roles.
fn gate_facts(
    slug: &str,
    workspace_role: Option<i32>,
    project_role: Option<i32>,
    _user_id: uuid::Uuid,
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

    /// The golden vectors pinned against the real fixture file, so a
    /// fixture edit that changes the contract fails the build here.
    fn fixture() -> Value {
        let raw = include_str!("../../../../fixtures/app_cycles/progress.json");
        serde_json::from_str(raw).expect("progress fixture must parse")
    }

    #[test]
    fn envelope_key_order_matches_fixture_output_keys() {
        let body = progress_envelope([None; 5], None, [0; 6]);
        let parsed: Value = serde_json::from_str(&body).expect("envelope must parse");
        let order: Vec<&str> = parsed
            .as_object()
            .expect("envelope is an object")
            .keys()
            .map(String::as_str)
            .collect();
        let owned = fixture();
        let expected: Vec<&str> = owned["output_keys"]
            .as_array()
            .expect("output_keys is a list")
            .iter()
            .map(|v| v.as_str().expect("key is a string"))
            .collect();
        assert_eq!(order, expected);
    }

    #[test]
    fn or_zero_collapses_none_and_zero_to_int() {
        assert_eq!(or_zero(None), Value::Number(0.into()));
        assert_eq!(or_zero(Some(0.0)), Value::Number(0.into()));
        assert_eq!(or_zero(Some(-0.0)), Value::Number(0.into()));
        assert_eq!(
            or_zero(Some(5.0)),
            serde_json::Number::from_f64(5.0)
                .map(Value::Number)
                .unwrap()
        );
        assert_eq!(or_zero(Some(2.5)).to_string(), "2.5");
    }

    #[test]
    fn total_coalesce_zero_is_float() {
        assert_eq!(total_num(None).to_string(), "0.0");
        assert_eq!(total_num(Some(0.0)).to_string(), "0.0");
        assert_eq!(total_num(Some(14.5)).to_string(), "14.5");
    }

    #[test]
    fn snapshot_branch_renders_fixture_golden() {
        // F-C27-08 "snapshot branch": estimates live, counts from keys.
        let body = progress_envelope(
            [Some(5.0), Some(0.0), Some(0.0), Some(0.0), Some(13.0)],
            Some(18.0),
            [1, 3, 2, 0, 0, 0],
        );
        let golden = &fixture()["cases"][0];
        let parsed: Value = serde_json::from_str(&body).expect("envelope must parse");
        for (key, want) in golden["estimates"].as_object().expect("estimates") {
            assert_eq!(&parsed[key], want, "estimate key {key}");
        }
        for (key, want) in golden["issues"].as_object().expect("issues") {
            assert_eq!(&parsed[key], want, "issue key {key}");
        }
    }

    #[test]
    fn empty_cycle_renders_float_total_int_groups() {
        // F-C27-08 "live-count branch, empty cycle": the five group keys
        // go through `or 0` (int 0); the total renders raw as 0.0.
        let body = progress_envelope([None; 5], None, [0; 6]);
        assert_eq!(
            body,
            r#"{"backlog_estimate_points":0,"unstarted_estimate_points":0,"started_estimate_points":0,"cancelled_estimate_points":0,"completed_estimate_points":0,"total_estimate_points":0.0,"backlog_issues":0,"total_issues":0,"completed_issues":0,"cancelled_issues":0,"started_issues":0,"unstarted_issues":0}"#
        );
    }

    #[test]
    fn snapshot_counts_default_missing_keys_to_zero() {
        let snap: Map<String, Value> = serde_json::from_str(r#"{"total_issues": 4}"#).expect("map");
        assert_eq!(snapshot_count(&snap, "total_issues"), 4);
        assert_eq!(snapshot_count(&snap, "backlog_issues"), 0);
        let empty = Map::new();
        assert_eq!(snapshot_count(&empty, "total_issues"), 0);
    }

    #[test]
    fn progress_gate_is_guest_and_denies_by_default() {
        use pidash_auth::permissions::{ROLE_ADMIN, ROLE_GUEST, ROLE_MEMBER};
        let row = gate_for("GET", PROGRESS_PATH).expect("progress route must have a gate");
        let scope = tenant_context("acme");
        let allow = |role: Option<i32>| {
            let allowed = [ROLE_ADMIN, ROLE_MEMBER, ROLE_GUEST];
            let facts = pidash_auth::permissions::allow::AllowFacts {
                workspace: pidash_types::WorkspaceId::from("acme"),
                authenticated: role.is_some(),
                is_workspace_member: role.is_some(),
                has_allowed_workspace_role: role.is_some_and(|r| allowed.contains(&r)),
                is_creator: false,
                has_allowed_project_role: role.is_some_and(|r| allowed.contains(&r)),
                is_project_member: role.is_some(),
                is_workspace_admin: role == Some(ROLE_ADMIN),
            };
            decide_gate(&row.gate, &scope, &facts)
        };
        assert_eq!(allow(Some(ROLE_GUEST)), GateOutcome::Allow);
        assert_eq!(allow(Some(ROLE_MEMBER)), GateOutcome::Allow);
        assert_eq!(allow(Some(ROLE_ADMIN)), GateOutcome::Allow);
        assert_eq!(allow(None), GateOutcome::Unauthenticated);
    }

    #[test]
    fn denial_bodies_are_byte_exact() {
        assert_eq!(Denial::Unauthorized.status_and_body().1, ANON_BODY);
        assert_eq!(Denial::Forbidden.status_and_body().1, FORBIDDEN_BODY);
        assert_eq!(
            Denial::NotFound.status_and_body(),
            (StatusCode::NOT_FOUND, CYCLE_NOT_FOUND_BODY.to_owned())
        );
        assert_eq!(
            Denial::BadValidation.status_and_body(),
            (StatusCode::BAD_REQUEST, VALIDATION_BODY.to_owned())
        );
        let missing = fixture()["missing_cycle"].clone();
        assert_eq!(missing["status"], 404);
        assert_eq!(
            serde_json::to_string(&missing["body"]).expect("body"),
            CYCLE_NOT_FOUND_BODY
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
    fn sql_carries_bridge_scope_and_manager_excludes() {
        let (project_id, cycle_id) = fixture_ids();
        let sql = render(&estimates_stmt("acme", project_id, cycle_id));
        for fragment in [
            "cycle_id",
            "\"deleted_at\" IS NULL",
            "\"slug\"",
            "\"archived_at\" IS NULL",
            "\"type\" = 'points'",
            "\"is_draft\" = FALSE",
            "\"group\" <> 'triage'",
            "double precision",
            "COALESCE",
        ] {
            assert!(
                sql.contains(fragment),
                "estimates SQL must carry {fragment}: {sql}"
            );
        }
        let counts = render(&live_counts_stmt("acme", project_id, cycle_id));
        assert!(counts.contains("COUNT"));
        assert!(counts.contains("\"group\" = 'backlog'"));
    }
}
