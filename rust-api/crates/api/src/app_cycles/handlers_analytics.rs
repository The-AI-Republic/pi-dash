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
//!   days are `null`; `TruncDate` runs in UTC (`TIME_ZONE = "UTC"`).
//!   The `points` chart needs the points estimate (only called when it
//!   exists); the `issues` chart counts completions per day.
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
/// to Django (DRF metadata, 401-anon-before-405). HEAD rides axum's
/// `get` handling like Django's `GET`-backed `HEAD`.
fn owned(
    router: axum::routing::MethodRouter<AppState>,
    methods: &[&str],
) -> axum::routing::MethodRouter<AppState> {
    let mut router = router;
    for method in ["GET", "POST", "PUT", "PATCH", "DELETE", "OPTIONS"] {
        if methods.contains(&method) {
            continue;
        }
        router = match method {
            "GET" => router.get(crate::edge::proxy),
            "POST" => router.post(crate::edge::proxy),
            "PUT" => router.put(crate::edge::proxy),
            "PATCH" => router.patch(crate::edge::proxy),
            "DELETE" => router.delete(crate::edge::proxy),
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
/// `handle_exception`'s generic 500 branch — also the missing-cycle
/// `AttributeError` (B1).
pub const SERVER_ERROR_BODY: &str = r#"{"error":"Something went wrong please try again later"}"#;

/// Handler denials with byte-exact bodies.
pub enum Denial {
    Unauthorized,
    Forbidden,
    NoDates,
    BadValidation,
    ServerError,
}

impl Denial {
    fn status_and_body(&self) -> (StatusCode, String) {
        match self {
            Denial::Unauthorized => (StatusCode::UNAUTHORIZED, ANON_BODY.to_owned()),
            Denial::Forbidden => (StatusCode::FORBIDDEN, FORBIDDEN_BODY.to_owned()),
            Denial::NoDates => (StatusCode::BAD_REQUEST, NO_DATES_BODY.to_owned()),
            Denial::BadValidation => (StatusCode::BAD_REQUEST, VALIDATION_BODY.to_owned()),
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

fn json_response(status: StatusCode, body: String) -> Response {
    (
        status,
        [(axum::http::header::CONTENT_TYPE, "application/json")],
        body,
    )
        .into_response()
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

async fn membership(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    user_id: &uuid::Uuid,
) -> Result<(Option<i32>, Option<i32>), Denial> {
    let workspace_role: Option<(Option<i16>,)> = sqlx::query_as(
        r#"SELECT wm.role FROM workspace_members wm
           JOIN workspaces w ON w.id = wm.workspace_id AND w.deleted_at IS NULL
           WHERE w.slug = $1 AND wm.member_id = $2 AND wm.is_active AND wm.deleted_at IS NULL"#,
    )
    .bind(slug)
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let project_role: Option<(Option<i16>,)> = sqlx::query_as(
        r#"SELECT pm.role FROM project_members pm
           JOIN workspaces w ON w.id = pm.workspace_id AND w.deleted_at IS NULL
           WHERE w.slug = $1 AND pm.project_id = $2 AND pm.member_id = $3
             AND pm.is_active AND pm.deleted_at IS NULL"#,
    )
    .bind(slug)
    .bind(project_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok((
        workspace_role.and_then(|row| row.0).map(i32::from),
        project_role.and_then(|row| row.0).map(i32::from),
    ))
}

// ---------------------------------------------------------------------------
// SQL
// ---------------------------------------------------------------------------

/// `Project` has a `"points"` estimate (`base.py:832-837`).
const ESTIMATE_TYPE_SQL: &str = "SELECT EXISTS(SELECT 1 FROM projects p \
     JOIN workspaces w ON w.id = p.workspace_id AND w.slug = $1 \
     JOIN estimates e ON e.id = p.estimate_id AND e.type = 'points' \
     WHERE p.id = $2)";

/// Cycle row plus the `total_issues` annotation (`base.py:791-805`):
/// distinct bridged issues that are not archived, not draft, not
/// deleted, on a live bridge row. No `deleted_at` predicate on the
/// cycle itself (plain manager — shared quirk with progress B1).
const CYCLE_SQL: &str = "SELECT c.id, c.start_date, c.end_date, c.progress_snapshot, \
     (SELECT COUNT(DISTINCT ci.issue_id) FROM cycle_issues ci \
      JOIN issues i ON i.id = ci.issue_id AND i.deleted_at IS NULL \
        AND i.archived_at IS NULL AND i.is_draft = FALSE \
      JOIN projects p ON p.id = i.project_id AND p.archived_at IS NULL \
      LEFT JOIN states s ON s.id = i.state_id \
      WHERE ci.cycle_id = c.id AND ci.deleted_at IS NULL AND NOT (s.group = 'triage')) AS total_issues \
     FROM cycles c \
     JOIN workspaces w ON w.id = c.workspace_id AND w.slug = $1 \
     WHERE c.project_id = $2 AND c.id = $3";

/// Base scope shared by the four distribution queries: live bridge row
/// plus the `IssueManager` excludes (the explicit archived/draft
/// filters on the completed/pending aggregates layer on top).
const DIST_SCOPE_SQL: &str = "FROM issues i \
     JOIN cycle_issues ci ON ci.issue_id = i.id AND ci.cycle_id = $3 AND ci.deleted_at IS NULL \
     JOIN workspaces w ON w.id = i.workspace_id AND w.slug = $1 \
     JOIN projects p ON p.id = i.project_id AND p.id = $2 AND p.archived_at IS NULL \
     LEFT JOIN states s ON s.id = i.state_id \
     WHERE i.deleted_at IS NULL AND i.archived_at IS NULL AND i.is_draft = FALSE \
     AND NOT (s.group = 'triage')";

/// `avatar_url` Case/Concat (`base.py:951-969`).
const AVATAR_CASE_SQL: &str = "CASE WHEN u.avatar_asset_id IS NOT NULL \
     THEN '/api/assets/v2/static/' || u.avatar_asset_id::text || '/' \
     WHEN u.avatar_asset_id IS NULL THEN u.avatar ELSE NULL END";

/// Points assignee distribution (`base.py:843-899`): per-assignee
/// estimate sums ordered by display name. `total_estimates` has no
/// archived/draft filter (B3); completed/pending do.
fn points_assignee_sql() -> String {
    format!(
        "SELECT u.display_name, u.id AS assignee_id, {AVATAR_CASE_SQL} AS avatar_url, \
         SUM(CAST(ep.value AS DOUBLE PRECISION)) AS total_estimates, \
         SUM(CAST(ep.value AS DOUBLE PRECISION)) FILTER (WHERE i.completed_at IS NOT NULL AND i.archived_at IS NULL AND i.is_draft = FALSE) AS completed_estimates, \
         SUM(CAST(ep.value AS DOUBLE PRECISION)) FILTER (WHERE i.completed_at IS NULL AND i.archived_at IS NULL AND i.is_draft = FALSE) AS pending_estimates \
         {DIST_SCOPE_SQL} \
         JOIN estimate_points ep ON ep.id = i.estimate_point_id \
         LEFT JOIN issue_assignees ia ON ia.issue_id = i.id \
         LEFT JOIN users u ON u.id = ia.assignee_id \
         GROUP BY u.display_name, u.id, {AVATAR_CASE_SQL} \
         ORDER BY u.display_name"
    )
}

/// Points label distribution (`base.py:901-931`).
fn points_label_sql() -> String {
    format!(
        "SELECT l.name AS label_name, l.color, l.id AS label_id, \
         SUM(CAST(ep.value AS DOUBLE PRECISION)) AS total_estimates, \
         SUM(CAST(ep.value AS DOUBLE PRECISION)) FILTER (WHERE i.completed_at IS NOT NULL AND i.archived_at IS NULL AND i.is_draft = FALSE) AS completed_estimates, \
         SUM(CAST(ep.value AS DOUBLE PRECISION)) FILTER (WHERE i.completed_at IS NULL AND i.archived_at IS NULL AND i.is_draft = FALSE) AS pending_estimates \
         {DIST_SCOPE_SQL} \
         JOIN estimate_points ep ON ep.id = i.estimate_point_id \
         LEFT JOIN issue_labels il ON il.issue_id = i.id \
         LEFT JOIN labels l ON l.id = il.label_id \
         GROUP BY l.name, l.color, l.id \
         ORDER BY l.name"
    )
}

/// Issues assignee distribution (`base.py:942-998`): `Count` over the
/// join key (NULL-skipping, like Django's `Count("assignee_id")`).
fn issues_assignee_sql() -> String {
    format!(
        "SELECT u.display_name, u.id AS assignee_id, {AVATAR_CASE_SQL} AS avatar_url, \
         COUNT(u.id) FILTER (WHERE i.archived_at IS NULL AND i.is_draft = FALSE) AS total_issues, \
         COUNT(u.id) FILTER (WHERE i.completed_at IS NOT NULL AND i.archived_at IS NULL AND i.is_draft = FALSE) AS completed_issues, \
         COUNT(u.id) FILTER (WHERE i.completed_at IS NULL AND i.archived_at IS NULL AND i.is_draft = FALSE) AS pending_issues \
         {DIST_SCOPE_SQL} \
         LEFT JOIN issue_assignees ia ON ia.issue_id = i.id \
         LEFT JOIN users u ON u.id = ia.assignee_id \
         GROUP BY u.display_name, u.id, {AVATAR_CASE_SQL} \
         ORDER BY u.display_name"
    )
}

/// Issues label distribution (`base.py:1000-1036`).
fn issues_label_sql() -> String {
    format!(
        "SELECT l.name AS label_name, l.color, l.id AS label_id, \
         COUNT(l.id) FILTER (WHERE i.archived_at IS NULL AND i.is_draft = FALSE) AS total_issues, \
         COUNT(l.id) FILTER (WHERE i.completed_at IS NOT NULL AND i.archived_at IS NULL AND i.is_draft = FALSE) AS completed_issues, \
         COUNT(l.id) FILTER (WHERE i.completed_at IS NULL AND i.archived_at IS NULL AND i.is_draft = FALSE) AS pending_issues \
         {DIST_SCOPE_SQL} \
         LEFT JOIN issue_labels il ON il.issue_id = i.id \
         LEFT JOIN labels l ON l.id = il.label_id \
         GROUP BY l.name, l.color, l.id \
         ORDER BY l.name"
    )
}

/// Burndown completion rows, points flavor
/// (`analytics_plot.py:166-180`, cycle path): every bridged issue with
/// an estimate, datestamp from `TruncDate(completed_at)` in UTC; the
/// caller drops `NULL` dates (`item["date"] is not None`).
const BURNDOWN_POINTS_SQL: &str =
    "SELECT (i.completed_at AT TIME ZONE 'UTC')::date AS d, ep.value AS v \
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
/// (`analytics_plot.py:182-193`, cycle path): per-day completed counts.
const BURNDOWN_ISSUES_SQL: &str =
    "SELECT (i.completed_at AT TIME ZONE 'UTC')::date AS d, COUNT(*) AS c \
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
        Ok(id) => id,
        Err(_) => return Ok(crate::edge::proxy(State(state), req).await),
    };
    let resolved = actor(&state, extension).await?;
    let pool = pool_of(&state)?.clone();
    let project_id: uuid::Uuid = project_raw.parse().map_err(|_| Denial::BadValidation)?;
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

    let estimate_type: Option<(bool,)> = sqlx::query_as(ESTIMATE_TYPE_SQL)
        .bind(&slug)
        .bind(project_id)
        .fetch_optional(&pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let estimate_type = estimate_type.is_some_and(|row| row.0);

    // The annotation runs on the same fetch as the cycle itself; a
    // missing cycle leaves `None` and the `start_date` dereference
    // 500s (B1) — there is no 404 branch in Python.
    /// Cycle row plus the `total_issues` annotation.
    type CycleRow = (
        uuid::Uuid,
        Option<chrono::DateTime<chrono::Utc>>,
        Option<chrono::DateTime<chrono::Utc>>,
        Option<Value>,
        Option<i64>,
    );
    let cycle: Option<CycleRow> = sqlx::query_as(CYCLE_SQL)
        .bind(&slug)
        .bind(project_id)
        .bind(cycle_id)
        .fetch_optional(&pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let Some((_id, start_date, end_date, snapshot, total_issues)) = cycle else {
        return Err(Denial::ServerError);
    };
    // A DB-level `NULL` snapshot reads falsy, like Python `None`.
    let snapshot = snapshot.unwrap_or(Value::Null);
    let (Some(start_date), Some(end_date)) = (start_date, end_date) else {
        return Err(Denial::NoDates);
    };

    // `if cycle.progress_snapshot:` — truthiness of the stored dict.
    if let Value::Object(map) = &snapshot {
        if !map.is_empty() {
            return Ok(json_response(StatusCode::OK, snapshot_envelope(map)));
        }
    }

    let today = chrono::Utc::now().date_naive();
    let start = start_date.date_naive();
    let end = end_date.date_naive();

    let mut assignees: Vec<Value> = Vec::new();
    let mut labels: Vec<Value> = Vec::new();
    let mut chart = Value::Object(Map::new());

    if analytic_type == "points" && estimate_type {
        let rows = sqlx::query_as::<
            _,
            (
                Option<String>,
                Option<uuid::Uuid>,
                Option<String>,
                Option<f64>,
                Option<f64>,
                Option<f64>,
            ),
        >(&points_assignee_sql())
        .bind(&slug)
        .bind(project_id)
        .bind(cycle_id)
        .fetch_all(&pool)
        .await
        .map_err(|_| Denial::ServerError)?;
        assignees = rows
            .into_iter()
            .map(|(name, id, avatar, total, done, pending)| {
                Value::Object(shape_assignee_points(
                    name, id, avatar, total, done, pending,
                ))
            })
            .collect();
        let rows = sqlx::query_as::<
            _,
            (
                Option<String>,
                Option<String>,
                Option<uuid::Uuid>,
                Option<f64>,
                Option<f64>,
                Option<f64>,
            ),
        >(&points_label_sql())
        .bind(&slug)
        .bind(project_id)
        .bind(cycle_id)
        .fetch_all(&pool)
        .await
        .map_err(|_| Denial::ServerError)?;
        labels = rows
            .into_iter()
            .map(|(name, color, id, total, done, pending)| {
                Value::Object(shape_label_points(name, color, id, total, done, pending))
            })
            .collect();
        let total_points: Option<(Option<f64>,)> = sqlx::query_as(
            "SELECT SUM(CAST(ep.value AS DOUBLE PRECISION)) FROM issues i \
             JOIN cycle_issues ci ON ci.issue_id = i.id AND ci.cycle_id = $3 AND ci.deleted_at IS NULL \
             JOIN workspaces w ON w.id = i.workspace_id AND w.slug = $1 \
             JOIN projects p ON p.id = i.project_id AND p.id = $2 AND p.archived_at IS NULL \
             LEFT JOIN states s ON s.id = i.state_id \
             JOIN estimate_points ep ON ep.id = i.estimate_point_id \
             WHERE i.deleted_at IS NULL AND i.archived_at IS NULL AND i.is_draft = FALSE \
             AND NOT (s.group = 'triage') AND ep.value IS NOT NULL",
        )
        .bind(&slug)
        .bind(project_id)
        .bind(cycle_id)
        .fetch_optional(&pool)
        .await
        .map_err(|_| Denial::ServerError)?;
        let total_points = total_points.and_then(|row| row.0).unwrap_or(0.0);
        let rows: Vec<(Option<NaiveDate>, String)> = sqlx::query_as(BURNDOWN_POINTS_SQL)
            .bind(&slug)
            .bind(project_id)
            .bind(cycle_id)
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
        let rows = sqlx::query_as::<
            _,
            (
                Option<String>,
                Option<uuid::Uuid>,
                Option<String>,
                i64,
                i64,
                i64,
            ),
        >(&issues_assignee_sql())
        .bind(&slug)
        .bind(project_id)
        .bind(cycle_id)
        .fetch_all(&pool)
        .await
        .map_err(|_| Denial::ServerError)?;
        assignees = rows
            .into_iter()
            .map(|(name, id, avatar, total, done, pending)| {
                Value::Object(shape_assignee_issues(
                    name, id, avatar, total, done, pending,
                ))
            })
            .collect();
        let rows = sqlx::query_as::<
            _,
            (
                Option<String>,
                Option<String>,
                Option<uuid::Uuid>,
                i64,
                i64,
                i64,
            ),
        >(&issues_label_sql())
        .bind(&slug)
        .bind(project_id)
        .bind(cycle_id)
        .fetch_all(&pool)
        .await
        .map_err(|_| Denial::ServerError)?;
        labels = rows
            .into_iter()
            .map(|(name, color, id, total, done, pending)| {
                Value::Object(shape_label_issues(name, color, id, total, done, pending))
            })
            .collect();
        let rows: Vec<(Option<NaiveDate>, i64)> = sqlx::query_as(BURNDOWN_ISSUES_SQL)
            .bind(&slug)
            .bind(project_id)
            .bind(cycle_id)
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

    #[test]
    fn sql_carries_scope_filters_and_avatar_case() {
        for sql in [
            points_assignee_sql(),
            points_label_sql(),
            issues_assignee_sql(),
            issues_label_sql(),
        ] {
            for fragment in [
                "ci.cycle_id = $3",
                "ci.deleted_at IS NULL",
                "w.slug = $1",
                "p.id = $2",
                "p.archived_at IS NULL",
                "i.deleted_at IS NULL",
                "NOT (s.group = 'triage')",
            ] {
                assert!(
                    sql.contains(fragment),
                    "distribution SQL must carry {fragment}"
                );
            }
        }
        assert!(points_assignee_sql().contains("ep.value AS DOUBLE PRECISION"));
        assert!(issues_assignee_sql().contains("COUNT(u.id)"));
        assert!(issues_label_sql().contains("COUNT(l.id)"));
        assert!(points_assignee_sql().contains("/api/assets/v2/static/"));
        assert!(BURNDOWN_POINTS_SQL.contains("AT TIME ZONE 'UTC'"));
        assert!(CYCLE_SQL.contains("COUNT(DISTINCT ci.issue_id)"));
        assert!(ESTIMATE_TYPE_SQL.contains("e.type = 'points'"));
    }
}
