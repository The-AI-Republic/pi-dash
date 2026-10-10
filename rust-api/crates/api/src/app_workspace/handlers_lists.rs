//! Workspace read-only list handlers (D-24, stage 5, PIDASHCONV-621).
//!
//! Ports the five workspace-scoped list endpoints whose serializers live in
//! other domains (`apps/api/pi_dash/app/views/workspace/`):
//!
//! - `WorkspaceLabelsEndpoint.get` (`label.py:17-30`, W26): viewer-gated,
//!   `cache_response(2h)`, `LabelSerializer` 7-key rows.
//! - `WorkspaceStatesEndpoint.get` (`state.py:17-41`, W28): entity-gated,
//!   per-group in-memory `order = index / count`, `StateSerializer` rows.
//! - `WorkspaceEstimatesEndpoint.get` (`estimate.py:17-32`, W29):
//!   entity-gated, `cache_response(2h)`, two-query list + points prefetch,
//!   `WorkspaceEstimateSerializer` rows.
//! - `WorkspaceCyclesEndpoint.get` (`cycle.py:19-104`, W31): viewer-gated,
//!   6 count annotations, `CycleSerializer` rows.
//! - `WorkspaceModulesEndpoint.get` (`module.py:19-111`, W30):
//!   viewer-gated, 6 count annotations + member/link prefetches,
//!   `ModuleSerializer` rows.
//!
//! Fixture ids: F-W24-15 (these routes) + consumed F-W24-12 (R1-R5 list
//! rows) and F-W24-13 (gates).
//!
//! Shapes come from the merged ports:
//! `services::app_project::{ser_workflow, ser_shared}` (D-25),
//! `services::app_cycles::shape`, `services::app_modules::shape`; SQL
//! builders from `services::app_workspace::queries_extras` (QRY-D);
//! gates from [`super::gates`] over the F-06 kernel.
//!
//! Layering: the services builders return `:named`-placeholder fragments;
//! [`positional`] rewrites them to `$n` and [`expand_in`] turns
//! `IN (:ids)` into `= ANY($n)` (identical rows, including the empty set,
//! where Django short-circuits). Endpoint-specific SQL (select lists over
//! the ported scopes) lives here because it names this endpoint's
//! projection; the merged predicates are spliced verbatim, never re-ported.
//!
//! # Ported bugs (translate, don't redesign — also listed in the PR)
//!
//! * `?order_by=` is IGNORED on the cycle/module lists
//!   (`cycle.py:100`, `module.py:107` read `self.kwargs`, which never holds
//!   `order_by`): the order is always `-created_at`. No `Query` extractor
//!   is read here at all.
//! * Cycle counts lack `distinct`, count `s.group` (not the link) for the
//!   five grouped counts, and carry an `issues.deleted_at` guard the module
//!   counts lack; module counts are `COUNT(DISTINCT mi.id)` with no issue
//!   tombstone guard. The asymmetry is kept, never unified.
//! * `state.order = index / count` is an in-memory mutation, never saved;
//!   the serializer reads the mutated values ([`state_group_order`]).
//! * The estimates id query has NO member scoping — any project in the
//!   workspace qualifies.
//! * `member_ids` renders literal `null` on the workspace module list: the
//!   declared `ListField(allow_null=True)` reads the missing
//!   `member_ids` attribute, and `allow_null` turns the miss into `None`
//!   instead of `SkipField` (verified against live Django).
//! * `is_favorite` / `status` (cycles) and `total_estimate_points` /
//!   `completed_estimate_points` / `is_favorite` (modules) are OMITTED keys
//!   here, not nulls: the workspace querysets annotate none of them and
//!   the models define none, so the `required=False` fields `SkipField`
//!   (verified against live Django).
//!
//! # Deliberate non-executions (byte-identical JSON either way)
//!
//! * The module `members` + `link_module` prefetches (`module.py:28-35`)
//!   are not executed: `ModuleSerializer` renders neither (only the Detail
//!   variant renders links; `member_ids` is always null here). The QRY-D
//!   prefetch builders stay the documented port.
//! * The `select_related` joins (project/workspace/lead/owned_by) are not
//!   executed either: every rendered key is a local column or a count
//!   annotation.
//! * `cache_response(2h)` performs no cache I/O: the contract/test configs
//!   run Django with `DEBUG=True`, where the decorator never stores, so
//!   fresh compute is exact parity there; in production Django uses
//!   django-redis pickle values, and a Rust JSON value on a shared key
//!   would 500 Django reads (`loads()` is unguarded). Pickle-compatible
//!   interop is a separate foundation-level issue. The sites, key scheme,
//!   and timeout stay pinned through [`super::gates`] in the tests below.
//!
//! # Auth edge (pilot precedent)
//!
//! Like every merged handler family, session auth checks only that the
//! session carries a UUID user id; a session for a deleted/inactive user
//! 403s at the membership gate instead of Django's 401. Unpinned by
//! fixtures and untested by the committed suites.

use std::collections::HashMap;

use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::Response;
use axum::Router;
use chrono_tz::Tz;

use crate::app_issues::Denial;
use crate::app_workspace::gates;
use crate::state::AppState;
use pidash_services::app_project::ser_shared::{
    workspace_estimate_to_representation, EstimatePointRow as SharedEstimatePointRow,
    WorkspaceEstimateRow as SharedWorkspaceEstimateRow,
};
use pidash_services::app_workspace::queries_extras as qx;

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------

/// `GET` on the five owned paths; every other method falls through to
/// Django (the views define `get` only, so Django answers 405 itself).
pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            LABELS_PATH,
            axum::routing::get(labels_list)
                .post(crate::edge::proxy)
                .put(crate::edge::proxy)
                .patch(crate::edge::proxy)
                .delete(crate::edge::proxy)
                .head(crate::edge::proxy)
                .options(crate::edge::proxy),
        )
        .route(
            STATES_PATH,
            axum::routing::get(states_list)
                .post(crate::edge::proxy)
                .put(crate::edge::proxy)
                .patch(crate::edge::proxy)
                .delete(crate::edge::proxy)
                .head(crate::edge::proxy)
                .options(crate::edge::proxy),
        )
        .route(
            ESTIMATES_PATH,
            axum::routing::get(estimates_list)
                .post(crate::edge::proxy)
                .put(crate::edge::proxy)
                .patch(crate::edge::proxy)
                .delete(crate::edge::proxy)
                .head(crate::edge::proxy)
                .options(crate::edge::proxy),
        )
        .route(
            MODULES_PATH,
            axum::routing::get(modules_list)
                .post(crate::edge::proxy)
                .put(crate::edge::proxy)
                .patch(crate::edge::proxy)
                .delete(crate::edge::proxy)
                .head(crate::edge::proxy)
                .options(crate::edge::proxy),
        )
        .route(
            CYCLES_PATH,
            axum::routing::get(cycles_list)
                .post(crate::edge::proxy)
                .put(crate::edge::proxy)
                .patch(crate::edge::proxy)
                .delete(crate::edge::proxy)
                .head(crate::edge::proxy)
                .options(crate::edge::proxy),
        )
}

/// W26 (`app/urls/workspace.py:183-187`).
pub const LABELS_PATH: &str = "/api/workspaces/{slug}/labels/";
/// W28 (`app/urls/workspace.py:193-197`).
pub const STATES_PATH: &str = "/api/workspaces/{slug}/states/";
/// W29 (`app/urls/workspace.py:198-202`).
pub const ESTIMATES_PATH: &str = "/api/workspaces/{slug}/estimates/";
/// W30 (`app/urls/workspace.py:203-207`).
pub const MODULES_PATH: &str = "/api/workspaces/{slug}/modules/";
/// W31 (`app/urls/workspace.py:208-212`).
pub const CYCLES_PATH: &str = "/api/workspaces/{slug}/cycles/";

// ---------------------------------------------------------------------------
// Shared request plumbing (app_issues / handlers_profile precedent)
// ---------------------------------------------------------------------------

type HandlerResult = Result<Response, Denial>;

fn json_response(status: StatusCode, body: String) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body))
        .expect("view response")
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).expect("json string")
}

/// `request.user` from the Django session (`app_issues` actor rule):
/// missing session, missing key, or a non-UUID id is anonymous → 401.
fn actor_user_id(
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Result<uuid::Uuid, Denial> {
    let handle = extension.ok_or(Denial::Unauthorized)?.0;
    let mut session = handle.snapshot();
    session
        .get("_auth_user_id")
        .and_then(|value| value.as_str())
        .and_then(|raw| raw.parse::<uuid::Uuid>().ok())
        .ok_or(Denial::Unauthorized)
}

fn pool_of(state: &AppState) -> Result<sqlx::PgPool, Denial> {
    state
        .pools()
        .map(|pools| pools.primary().clone())
        .ok_or(Denial::ServerError)
}

/// The actor's `user_timezone` (`users.user_timezone`), parsed for DRF
/// serializer rendering (`TimezoneMixin` activates the actor zone, so
/// every datetime renders shifted into it). A missing row or an
/// unparseable zone is a 500 (the pilot reads the same column through
/// its gate context). Runs after the membership gate, so memberless
/// callers 403 before this is reached.
async fn actor_timezone(
    pool: &sqlx::PgPool,
    user_id: &uuid::Uuid,
) -> Result<chrono_tz::Tz, Denial> {
    let row: Option<(String,)> = sqlx::query_as("SELECT user_timezone FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let (name,) = row.ok_or(Denial::ServerError)?;
    name.parse().map_err(|_| Denial::ServerError)
}

/// Workspace role facts for one `(user, slug)` over the same rows the
/// permission classes read (`WorkspaceMember.objects`, soft-deletion
/// scoped, `workspace__slug` + `member` + `is_active`).
async fn fetch_workspace_role(
    pool: &sqlx::PgPool,
    slug: &str,
    user_id: &uuid::Uuid,
) -> Result<Option<i16>, Denial> {
    let row: Option<(i16,)> = sqlx::query_as(
        r#"SELECT wm.role FROM workspace_members wm
           JOIN workspaces w ON w.id = wm.workspace_id
           WHERE wm.member_id = $1 AND w.slug = $2
           AND wm.is_active AND wm.deleted_at IS NULL"#,
    )
    .bind(user_id)
    .bind(slug)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(row.map(|row| row.0))
}

/// Run one class gate (`WorkspaceEntityPermission` /
/// `WorkspaceViewerPermission`): anonymous already 401'd; `Allow`
/// yields `None` (the body runs), anything else yields the denial
/// response. Class denials render [`gates::CLASS_DENIED_BODY`], not
/// `Denial::Forbidden` — so the denial is returned as a response here
/// instead of flowing through [`Denial`].
async fn check_class_gate(
    pool: &sqlx::PgPool,
    gate: &gates::Gate,
    slug: &str,
    user_id: &uuid::Uuid,
) -> Result<Option<Response>, Denial> {
    use pidash_auth::permissions::workspace::WorkspaceFacts;
    use pidash_auth::permissions::{ROLE_ADMIN, ROLE_MEMBER};
    let deny = || {
        Response::builder()
            .status(StatusCode::FORBIDDEN)
            .header(header::CONTENT_TYPE, "application/json")
            .body(axum::body::Body::from(gates::CLASS_DENIED_BODY))
            .expect("class denial response")
    };
    let role = fetch_workspace_role(pool, slug, user_id).await?;
    let facts = WorkspaceFacts {
        workspace: pidash_types::WorkspaceId::from(slug),
        authenticated: true,
        has_admin_or_member_role: role
            .map(|role| i32::from(role) == ROLE_ADMIN || i32::from(role) == ROLE_MEMBER)
            .unwrap_or(false),
        has_admin_role: role
            .map(|role| i32::from(role) == ROLE_ADMIN)
            .unwrap_or(false),
        is_member: role.is_some(),
        is_admin_unfiltered: role
            .map(|role| i32::from(role) == ROLE_ADMIN)
            .unwrap_or(false),
    };
    let scope = gates::tenant_context(slug);
    let outcome = match gate {
        gates::Gate::ClassEntity => gates::decide_class_entity("GET", &scope, &facts),
        gates::Gate::ClassViewer => gates::decide_class_viewer(&scope, &facts),
        _ => gates::GateOutcome::DenyClass,
    };
    if outcome == gates::GateOutcome::Allow {
        Ok(None)
    } else {
        Ok(Some(deny()))
    }
}

/// Rewrite the services builders' `:named` placeholders to positional
/// `$n` binds, in `binds` order. Names are matched longest-first so
/// `:user` never eats the head of a longer placeholder.
fn positional(fragment: &str, binds: &[&str]) -> String {
    let mut ordered: Vec<(usize, &str)> = binds
        .iter()
        .enumerate()
        .map(|(index, name)| (index, *name))
        .collect();
    ordered.sort_by_key(|(_, name)| std::cmp::Reverse(name.len()));
    let mut out = fragment.to_owned();
    for (index, name) in ordered {
        out = out.replace(&format!(":{name}"), &format!("${}", index + 1));
    }
    out
}

/// Rewrite one `IN ($n)` list predicate (after [`positional`]) to
/// `= ANY($n)`: identical rows, including the empty set (Django's
/// `pk__in=[]` short-circuits to no rows; `= ANY('{}')` matches nothing).
fn expand_in(fragment: &str, position: u32) -> String {
    fragment.replace(&format!("IN (${position})"), &format!("= ANY(${position})"))
}

// ---------------------------------------------------------------------------
// Unit 1 — workspace labels (W26, `label.py:17-30`)
// ---------------------------------------------------------------------------

/// `LabelSerializer.Meta.fields` (`app/serializers/issue.py:549-562`), the
/// render order. No merged port covers this 7-key app shape (the merged
/// space-taxonomy `LabelView` ports the 17-key `space` class), so the
/// scalar row renders here; a D-26 follow-up owns the full port.
pub const LABEL_READ_KEYS: [&str; 7] = [
    "parent",
    "name",
    "color",
    "id",
    "project_id",
    "workspace_id",
    "sort_order",
];

/// R1 executed SELECT: the needed label columns over the ported scope +
/// joins (`label_list_sql` without the `*`), `$1` slug, `$2` user.
fn labels_sql() -> String {
    let scope = positional(&qx::label_scope_where(), &["slug", "user"]);
    format!(
        "SELECT labels.id, labels.parent_id, labels.name, labels.color, labels.project_id, labels.workspace_id, labels.sort_order FROM labels JOIN projects ON projects.id = labels.project_id JOIN project_members pm ON pm.project_id = projects.id WHERE {scope} ORDER BY {}",
        qx::LABEL_LIST_ORDER_SQL,
    )
}

#[derive(Debug, sqlx::FromRow)]
struct LabelListRow {
    id: uuid::Uuid,
    parent_id: Option<uuid::Uuid>,
    name: String,
    color: String,
    project_id: Option<uuid::Uuid>,
    workspace_id: uuid::Uuid,
    sort_order: f64,
}

/// Render one label row in [`LABEL_READ_KEYS`] order. `project_id` is
/// always present here (the scope inner-joins projects) but stays
/// optional-typed, like the model.
fn render_label_row(row: &LabelListRow) -> String {
    use std::fmt::Write as _;
    let mut out = String::from("{");
    let parent = row
        .parent_id
        .map(|id| json_string(&id.to_string()))
        .unwrap_or_else(|| "null".to_owned());
    let project = row
        .project_id
        .map(|id| json_string(&id.to_string()))
        .unwrap_or_else(|| "null".to_owned());
    let _ = write!(
        out,
        "\"parent\":{parent},\"name\":{},\"color\":{},\"id\":{},\"project_id\":{project},\"workspace_id\":{},\"sort_order\":{}",
        json_string(&row.name),
        json_string(&row.color),
        json_string(&row.id.to_string()),
        json_string(&row.workspace_id.to_string()),
        crate::paginator::py_float_str(row.sort_order),
    );
    out.push('}');
    out
}

/// `WorkspaceLabelsEndpoint.get` (`label.py:22-30`).
async fn labels_list(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> HandlerResult {
    let pool = pool_of(&state)?;
    let user_id = actor_user_id(extension)?;
    if let Some(denied) =
        check_class_gate(&pool, &gates::Gate::ClassViewer, &slug, &user_id).await?
    {
        return Ok(denied);
    }
    let rows: Vec<LabelListRow> = sqlx::query_as(&labels_sql())
        .bind(&slug)
        .bind(user_id)
        .fetch_all(&pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let mut body = String::from("[");
    for (index, row) in rows.iter().enumerate() {
        if index > 0 {
            body.push(',');
        }
        body.push_str(&render_label_row(row));
    }
    body.push(']');
    Ok(json_response(StatusCode::OK, body))
}

// ---------------------------------------------------------------------------
// Unit 2 — workspace states (W28, `state.py:17-41`)
// ---------------------------------------------------------------------------

/// R2 executed SELECT: the state columns over the ported scope + joins,
/// `$1` slug, `$2` user. Row order is `sequence ASC`; the regroup below
/// preserves it.
fn states_sql() -> String {
    let scope = positional(&qx::state_scope_where(), &["slug", "user"]);
    format!(
        "SELECT states.id, states.project_id, states.workspace_id, states.name, states.color, states.\"group\", states.\"default\" AS is_default, states.description, states.sequence FROM states JOIN projects ON projects.id = states.project_id JOIN project_members pm ON pm.project_id = projects.id WHERE {scope} ORDER BY {}",
        qx::STATE_LIST_ORDER_SQL,
    )
}

#[derive(Debug, sqlx::FromRow)]
struct StateListRow {
    id: uuid::Uuid,
    project_id: uuid::Uuid,
    workspace_id: uuid::Uuid,
    name: String,
    color: String,
    group: String,
    is_default: bool,
    description: String,
    sequence: f64,
}

/// Render one state row: the 10 `STATE_FIELDS` keys. The declared
/// `order` is set in memory on every row here (unlike the D-25 list,
/// where it `SkipField`s), so it renders; `order` arrives precomputed
/// per [`qx::state_group_order`].
fn render_state_row(row: &StateListRow, order: f64) -> String {
    use std::fmt::Write as _;
    let mut out = String::from("{");
    let _ = write!(
        out,
        "\"id\":{},\"project_id\":{},\"workspace_id\":{},\"name\":{},\"color\":{},\"group\":{},\"default\":{},\"description\":{},\"sequence\":{},\"order\":{}",
        json_string(&row.id.to_string()),
        json_string(&row.project_id.to_string()),
        json_string(&row.workspace_id.to_string()),
        json_string(&row.name),
        json_string(&row.color),
        json_string(&row.group),
        row.is_default,
        json_string(&row.description),
        crate::paginator::py_float_str(row.sequence),
        crate::paginator::py_float_str(order),
    );
    out.push('}');
    out
}

/// `WorkspaceStatesEndpoint.get` (`state.py:21-41`): regroup by `group`
/// over the sequence-ordered rows, `order = index / count` per group.
async fn states_list(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> HandlerResult {
    let pool = pool_of(&state)?;
    let user_id = actor_user_id(extension)?;
    if let Some(denied) =
        check_class_gate(&pool, &gates::Gate::ClassEntity, &slug, &user_id).await?
    {
        return Ok(denied);
    }
    let rows: Vec<StateListRow> = sqlx::query_as(&states_sql())
        .bind(&slug)
        .bind(user_id)
        .fetch_all(&pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for row in &rows {
        *counts.entry(row.group.as_str()).or_insert(0) += 1;
    }
    let mut seen: HashMap<&str, usize> = HashMap::new();
    let mut body = String::from("[");
    for (index, row) in rows.iter().enumerate() {
        if index > 0 {
            body.push(',');
        }
        let count = counts[row.group.as_str()];
        let position = seen.entry(row.group.as_str()).or_insert(0);
        *position += 1;
        body.push_str(&render_state_row(
            row,
            qx::state_group_order(*position, count),
        ));
    }
    body.push(']');
    Ok(json_response(StatusCode::OK, body))
}

// ---------------------------------------------------------------------------
// Unit 3 — workspace estimates (W29, `estimate.py:17-32`)
// ---------------------------------------------------------------------------

/// R3 Q1 executed SELECT: project estimate ids in the workspace, `$1`
/// slug. No member scoping (ported bug).
fn estimate_ids_sql() -> String {
    positional(&qx::estimate_ids_q1_sql(), &["slug"])
}

/// R3 Q2 executed SELECT: the estimate columns over the ported scope,
/// `$1` estimate ids, `$2` slug. The `select_related` joins from the
/// representative statement are skipped (every rendered key is local).
fn estimates_sql() -> String {
    let scope = positional(&qx::estimate_scope_where(), &["estimate_ids", "slug"]);
    let scope = expand_in(&scope, 1);
    format!(
        "SELECT estimates.id, estimates.created_at, estimates.updated_at, estimates.deleted_at, estimates.name, estimates.description, estimates.type AS estimate_type, estimates.last_used, estimates.created_by_id, estimates.updated_by_id, estimates.project_id, estimates.workspace_id FROM estimates WHERE {scope} ORDER BY {}",
        qx::ESTIMATE_LIST_ORDER_SQL,
    )
}

/// R3 points prefetch: `$1` estimate ids, `value ASC` (the through
/// default ordering). The ported statement is used verbatim (single
/// table, unambiguous `*` — [`EstimatePointListRow`] reads by name).
fn estimate_points_sql() -> String {
    let sql = positional(&qx::estimate_points_prefetch_sql(), &["estimate_ids"]);
    expand_in(&sql, 1)
}

#[derive(Debug, sqlx::FromRow)]
struct EstimateListRow {
    id: uuid::Uuid,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
    deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    name: String,
    description: String,
    estimate_type: String,
    last_used: bool,
    created_by_id: Option<uuid::Uuid>,
    updated_by_id: Option<uuid::Uuid>,
    project_id: uuid::Uuid,
    workspace_id: uuid::Uuid,
}

#[derive(Debug, sqlx::FromRow)]
struct EstimatePointListRow {
    id: uuid::Uuid,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
    deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    key: i32,
    description: String,
    value: String,
    created_by_id: Option<uuid::Uuid>,
    updated_by_id: Option<uuid::Uuid>,
    project_id: uuid::Uuid,
    workspace_id: uuid::Uuid,
    estimate_id: uuid::Uuid,
}

/// Pre-rendered strings for one estimate row: datetimes in the actor
/// zone, ids as strings, nulls as `None`.
struct EstimateStrings {
    id: String,
    created_at: String,
    updated_at: String,
    deleted_at: Option<String>,
    name: String,
    description: String,
    estimate_type: String,
    last_used: bool,
    created_by: Option<String>,
    updated_by: Option<String>,
    project: String,
    workspace: String,
}

struct EstimatePointStrings {
    id: String,
    created_at: String,
    updated_at: String,
    deleted_at: Option<String>,
    key: i32,
    description: String,
    value: String,
    created_by: Option<String>,
    updated_by: Option<String>,
    project: String,
    workspace: String,
    estimate: String,
}

fn render_dt(dt: &chrono::DateTime<chrono::Utc>, timezone: &Tz) -> String {
    crate::serializer::render_datetime_in(dt, timezone)
}

/// `WorkspaceEstimatesEndpoint.get` (`estimate.py:22-32`): Q1 ids, Q2
/// rows, points prefetch grouped per estimate (prefetch order kept),
/// rendered through the merged [`workspace_estimate_to_representation`].
async fn estimates_list(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> HandlerResult {
    let pool = pool_of(&state)?;
    let user_id = actor_user_id(extension)?;
    if let Some(denied) =
        check_class_gate(&pool, &gates::Gate::ClassEntity, &slug, &user_id).await?
    {
        return Ok(denied);
    }
    let timezone = actor_timezone(&pool, &user_id).await?;
    let ids: Vec<uuid::Uuid> = sqlx::query_scalar(&estimate_ids_sql())
        .bind(&slug)
        .fetch_all(&pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let rows: Vec<EstimateListRow> = sqlx::query_as(&estimates_sql())
        .bind(&ids)
        .bind(&slug)
        .fetch_all(&pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let point_rows: Vec<EstimatePointListRow> = sqlx::query_as(&estimate_points_sql())
        .bind(&ids)
        .fetch_all(&pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let mut points_by_estimate: HashMap<uuid::Uuid, Vec<EstimatePointStrings>> = HashMap::new();
    for point in &point_rows {
        points_by_estimate
            .entry(point.estimate_id)
            .or_default()
            .push(EstimatePointStrings {
                id: point.id.to_string(),
                created_at: render_dt(&point.created_at, &timezone),
                updated_at: render_dt(&point.updated_at, &timezone),
                deleted_at: point.deleted_at.as_ref().map(|dt| render_dt(dt, &timezone)),
                key: point.key,
                description: point.description.clone(),
                value: point.value.clone(),
                created_by: point.created_by_id.map(|id| id.to_string()),
                updated_by: point.updated_by_id.map(|id| id.to_string()),
                project: point.project_id.to_string(),
                workspace: point.workspace_id.to_string(),
                estimate: point.estimate_id.to_string(),
            });
    }
    let mut body = String::from("[");
    for (index, row) in rows.iter().enumerate() {
        if index > 0 {
            body.push(',');
        }
        let strings = EstimateStrings {
            id: row.id.to_string(),
            created_at: render_dt(&row.created_at, &timezone),
            updated_at: render_dt(&row.updated_at, &timezone),
            deleted_at: row.deleted_at.as_ref().map(|dt| render_dt(dt, &timezone)),
            name: row.name.clone(),
            description: row.description.clone(),
            estimate_type: row.estimate_type.clone(),
            last_used: row.last_used,
            created_by: row.created_by_id.map(|id| id.to_string()),
            updated_by: row.updated_by_id.map(|id| id.to_string()),
            project: row.project_id.to_string(),
            workspace: row.workspace_id.to_string(),
        };
        let empty = Vec::new();
        let points = points_by_estimate.get(&row.id).unwrap_or(&empty);
        let point_views: Vec<SharedEstimatePointRow<'_>> = points
            .iter()
            .map(|point| SharedEstimatePointRow {
                id: &point.id,
                created_at: &point.created_at,
                updated_at: &point.updated_at,
                deleted_at: point.deleted_at.as_deref(),
                key: point.key,
                description: &point.description,
                value: &point.value,
                created_by: point.created_by.as_deref(),
                updated_by: point.updated_by.as_deref(),
                project: &point.project,
                workspace: &point.workspace,
                estimate: &point.estimate,
            })
            .collect();
        let shared = SharedWorkspaceEstimateRow {
            id: &strings.id,
            points: &point_views,
            created_at: &strings.created_at,
            updated_at: &strings.updated_at,
            deleted_at: strings.deleted_at.as_deref(),
            name: &strings.name,
            description: &strings.description,
            estimate_type: &strings.estimate_type,
            last_used: strings.last_used,
            created_by: strings.created_by.as_deref(),
            updated_by: strings.updated_by.as_deref(),
            project: &strings.project,
            workspace: &strings.workspace,
        };
        let view = workspace_estimate_to_representation(&shared);
        body.push_str(&serde_json::to_string(&view).map_err(|_| Denial::ServerError)?);
    }
    body.push(']');
    Ok(json_response(StatusCode::OK, body))
}

// ---------------------------------------------------------------------------
// Unit 4 — workspace cycles (W31, `cycle.py:19-104`)
// ---------------------------------------------------------------------------

/// `CycleSerializer` keys as rendered on the workspace list: the 22
/// `CYCLE_SERIALIZER_FIELDS` minus `is_favorite` + `status`, which
/// `SkipField` here (no annotation, no model attribute — verified live).
/// Sibling order (total, cancelled, completed, started, unstarted,
/// backlog) follows `Meta.fields`, not the annotation build order.
pub const WORKSPACE_CYCLE_KEYS: [&str; 20] = [
    "id",
    "workspace_id",
    "project_id",
    "name",
    "description",
    "start_date",
    "end_date",
    "owned_by_id",
    "view_props",
    "sort_order",
    "external_source",
    "external_id",
    "progress_snapshot",
    "logo_props",
    "total_issues",
    "cancelled_issues",
    "completed_issues",
    "started_issues",
    "unstarted_issues",
    "backlog_issues",
];

/// R5 executed SELECT: the cycle columns + the six ported count
/// annotations, `$1` slug. Joins use the builders' `ic`/`i`/`s` aliases
/// verbatim; the `select_related` inner joins are skipped (every
/// rendered key is local). The `.distinct()` is a no-op over
/// `GROUP BY cycles.id` and is not emitted.
fn cycles_sql() -> String {
    let scope = positional(&qx::cycle_scope_where(), &["slug"]);
    let mut annotations = vec![qx::cycle_count_annotation_sql(None)];
    annotations.extend(
        qx::MODULE_COUNT_GROUPS
            .iter()
            .map(|group| qx::cycle_count_annotation_sql(Some(group))),
    );
    format!(
        "SELECT cycles.id, cycles.workspace_id, cycles.project_id, cycles.name, cycles.description, cycles.start_date, cycles.end_date, cycles.owned_by_id, cycles.view_props, cycles.sort_order, cycles.external_source, cycles.external_id, cycles.progress_snapshot, cycles.logo_props, {} FROM cycles LEFT JOIN cycle_issues ic ON ic.cycle_id = cycles.id LEFT JOIN issues i ON i.id = ic.issue_id LEFT JOIN states s ON s.id = i.state_id WHERE {scope} GROUP BY cycles.id ORDER BY {}",
        annotations.join(", "),
        qx::CYCLE_LIST_ORDER_SQL,
    )
}

#[derive(Debug, sqlx::FromRow)]
struct CycleListRow {
    id: uuid::Uuid,
    workspace_id: uuid::Uuid,
    project_id: uuid::Uuid,
    name: String,
    description: String,
    start_date: Option<chrono::DateTime<chrono::Utc>>,
    end_date: Option<chrono::DateTime<chrono::Utc>>,
    owned_by_id: uuid::Uuid,
    view_props: serde_json::Value,
    sort_order: f64,
    external_source: Option<String>,
    external_id: Option<String>,
    progress_snapshot: serde_json::Value,
    logo_props: serde_json::Value,
    total_issues: i64,
    cancelled_issues: i64,
    completed_issues: i64,
    started_issues: i64,
    unstarted_issues: i64,
    backlog_issues: i64,
}

fn render_opt_dt(dt: &Option<chrono::DateTime<chrono::Utc>>, timezone: &Tz) -> String {
    dt.as_ref()
        .map(|dt| json_string(&render_dt(dt, timezone)))
        .unwrap_or_else(|| "null".to_owned())
}

fn render_opt_str(value: &Option<String>) -> String {
    value
        .as_deref()
        .map(json_string)
        .unwrap_or_else(|| "null".to_owned())
}

/// Render one cycle row in [`WORKSPACE_CYCLE_KEYS`] order.
fn render_cycle_row(row: &CycleListRow, timezone: &Tz) -> String {
    use std::fmt::Write as _;
    let mut out = String::from("{");
    let _ = write!(
        out,
        "\"id\":{},\"workspace_id\":{},\"project_id\":{},\"name\":{},\"description\":{},\"start_date\":{},\"end_date\":{},\"owned_by_id\":{},\"view_props\":{},\"sort_order\":{},\"external_source\":{},\"external_id\":{},\"progress_snapshot\":{},\"logo_props\":{},\"total_issues\":{},\"cancelled_issues\":{},\"completed_issues\":{},\"started_issues\":{},\"unstarted_issues\":{},\"backlog_issues\":{}",
        json_string(&row.id.to_string()),
        json_string(&row.workspace_id.to_string()),
        json_string(&row.project_id.to_string()),
        json_string(&row.name),
        json_string(&row.description),
        render_opt_dt(&row.start_date, timezone),
        render_opt_dt(&row.end_date, timezone),
        json_string(&row.owned_by_id.to_string()),
        row.view_props,
        crate::paginator::py_float_str(row.sort_order),
        render_opt_str(&row.external_source),
        render_opt_str(&row.external_id),
        row.progress_snapshot,
        row.logo_props,
        row.total_issues,
        row.cancelled_issues,
        row.completed_issues,
        row.started_issues,
        row.unstarted_issues,
        row.backlog_issues,
    );
    out.push('}');
    out
}

/// `WorkspaceCyclesEndpoint.get` (`cycle.py:22-104`).
async fn cycles_list(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> HandlerResult {
    let pool = pool_of(&state)?;
    let user_id = actor_user_id(extension)?;
    if let Some(denied) =
        check_class_gate(&pool, &gates::Gate::ClassViewer, &slug, &user_id).await?
    {
        return Ok(denied);
    }
    let timezone = actor_timezone(&pool, &user_id).await?;
    let rows: Vec<CycleListRow> = sqlx::query_as(&cycles_sql())
        .bind(&slug)
        .fetch_all(&pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let mut body = String::from("[");
    for (index, row) in rows.iter().enumerate() {
        if index > 0 {
            body.push(',');
        }
        body.push_str(&render_cycle_row(row, &timezone));
    }
    body.push(']');
    Ok(json_response(StatusCode::OK, body))
}

// ---------------------------------------------------------------------------
// Unit 5 — workspace modules (W30, `module.py:19-111`)
// ---------------------------------------------------------------------------

/// `ModuleSerializer` keys as rendered on the workspace list: the 29
/// `MODULE_LIST_FIELD_ORDER` keys minus `total_estimate_points` /
/// `completed_estimate_points` / `is_favorite` (all `SkipField` here —
/// verified live). `member_ids` renders literal `null` (see ported bug).
pub const WORKSPACE_MODULE_KEYS: [&str; 26] = [
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
    "total_issues",
    "cancelled_issues",
    "completed_issues",
    "started_issues",
    "unstarted_issues",
    "backlog_issues",
    "created_at",
    "updated_at",
    "archived_at",
];

/// R4 executed SELECT: the module columns + the six ported count
/// annotations, `$1` slug. Joins use the builders' `mi`/`i`/`s` aliases
/// verbatim; member/link prefetches and `select_related` joins are not
/// executed (results unused by this read shape — see module docs).
fn modules_sql() -> String {
    let scope = positional(&qx::module_scope_where(), &["slug"]);
    let mut annotations = vec![qx::module_count_annotation_sql(None)];
    annotations.extend(
        qx::MODULE_COUNT_GROUPS
            .iter()
            .map(|group| qx::module_count_annotation_sql(Some(group))),
    );
    format!(
        "SELECT modules.id, modules.workspace_id, modules.project_id, modules.name, modules.description, modules.description_text, modules.description_html, modules.start_date, modules.target_date, modules.status, modules.lead_id, modules.view_props, modules.sort_order, modules.external_source, modules.external_id, modules.logo_props, modules.created_at, modules.updated_at, modules.archived_at, {} FROM modules LEFT JOIN module_issues mi ON mi.module_id = modules.id LEFT JOIN issues i ON i.id = mi.issue_id LEFT JOIN states s ON s.id = i.state_id WHERE {scope} GROUP BY modules.id ORDER BY {}",
        annotations.join(", "),
        qx::MODULE_LIST_ORDER_SQL,
    )
}

#[derive(Debug, sqlx::FromRow)]
struct ModuleListRow {
    id: uuid::Uuid,
    workspace_id: uuid::Uuid,
    project_id: uuid::Uuid,
    name: String,
    description: String,
    description_text: Option<serde_json::Value>,
    description_html: Option<serde_json::Value>,
    start_date: Option<chrono::NaiveDate>,
    target_date: Option<chrono::NaiveDate>,
    status: String,
    lead_id: Option<uuid::Uuid>,
    view_props: serde_json::Value,
    sort_order: f64,
    external_source: Option<String>,
    external_id: Option<String>,
    logo_props: serde_json::Value,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
    archived_at: Option<chrono::DateTime<chrono::Utc>>,
    total_issues: i64,
    cancelled_issues: i64,
    completed_issues: i64,
    started_issues: i64,
    unstarted_issues: i64,
    backlog_issues: i64,
}

/// Render one module row in [`WORKSPACE_MODULE_KEYS`] order. `start_date`
/// / `target_date` are `date` columns: DRF renders `YYYY-MM-DD`.
fn render_module_row(row: &ModuleListRow, timezone: &Tz) -> String {
    use std::fmt::Write as _;
    let mut out = String::from("{");
    let lead = row
        .lead_id
        .map(|id| json_string(&id.to_string()))
        .unwrap_or_else(|| "null".to_owned());
    let start = row
        .start_date
        .map(|date| json_string(&date.format("%Y-%m-%d").to_string()))
        .unwrap_or_else(|| "null".to_owned());
    let target = row
        .target_date
        .map(|date| json_string(&date.format("%Y-%m-%d").to_string()))
        .unwrap_or_else(|| "null".to_owned());
    let description_text = row
        .description_text
        .as_ref()
        .map(serde_json::Value::to_string)
        .unwrap_or_else(|| "null".to_owned());
    let description_html = row
        .description_html
        .as_ref()
        .map(serde_json::Value::to_string)
        .unwrap_or_else(|| "null".to_owned());
    let _ = write!(
        out,
        "\"id\":{},\"workspace_id\":{},\"project_id\":{},\"name\":{},\"description\":{},\"description_text\":{description_text},\"description_html\":{description_html},\"start_date\":{start},\"target_date\":{target},\"status\":{},\"lead_id\":{lead},\"member_ids\":null,\"view_props\":{},\"sort_order\":{},\"external_source\":{},\"external_id\":{},\"logo_props\":{},\"total_issues\":{},\"cancelled_issues\":{},\"completed_issues\":{},\"started_issues\":{},\"unstarted_issues\":{},\"backlog_issues\":{},\"created_at\":{},\"updated_at\":{},\"archived_at\":{}",
        json_string(&row.id.to_string()),
        json_string(&row.workspace_id.to_string()),
        json_string(&row.project_id.to_string()),
        json_string(&row.name),
        json_string(&row.description),
        json_string(&row.status),
        row.view_props,
        crate::paginator::py_float_str(row.sort_order),
        render_opt_str(&row.external_source),
        render_opt_str(&row.external_id),
        row.logo_props,
        row.total_issues,
        row.cancelled_issues,
        row.completed_issues,
        row.started_issues,
        row.unstarted_issues,
        row.backlog_issues,
        json_string(&render_dt(&row.created_at, timezone)),
        json_string(&render_dt(&row.updated_at, timezone)),
        render_opt_dt(&row.archived_at, timezone),
    );
    out.push('}');
    out
}

/// `WorkspaceModulesEndpoint.get` (`module.py:22-111`).
async fn modules_list(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> HandlerResult {
    let pool = pool_of(&state)?;
    let user_id = actor_user_id(extension)?;
    if let Some(denied) =
        check_class_gate(&pool, &gates::Gate::ClassViewer, &slug, &user_id).await?
    {
        return Ok(denied);
    }
    let timezone = actor_timezone(&pool, &user_id).await?;
    let rows: Vec<ModuleListRow> = sqlx::query_as(&modules_sql())
        .bind(&slug)
        .fetch_all(&pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let mut body = String::from("[");
    for (index, row) in rows.iter().enumerate() {
        if index > 0 {
            body.push(',');
        }
        body.push_str(&render_module_row(row, &timezone));
    }
    body.push(']');
    Ok(json_response(StatusCode::OK, body))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROUTES_GOLDEN: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/app_workspace/handlers/routes.golden.json"
    );

    fn golden_routes() -> Vec<String> {
        let text = std::fs::read_to_string(ROUTES_GOLDEN).expect("F-W24-15 golden exists");
        let parsed: serde_json::Value =
            serde_json::from_str(&text).expect("F-W24-15 golden is valid JSON");
        parsed["routes"]
            .as_array()
            .expect("golden carries routes")
            .iter()
            .map(|entry| {
                entry
                    .as_str()
                    .expect("route entries are strings")
                    .to_owned()
            })
            .collect()
    }

    /// F-W24-15: the five owned route entries exist with these paths and
    /// handler names, and the axum path consts name the same Django paths.
    #[test]
    fn routes_match_fixture_table() {
        let routes = golden_routes();
        let django = |entry: &str| entry.split(" -> ").next().unwrap_or("").to_owned();
        let find = |id: &str| {
            routes
                .iter()
                .find(|entry| entry.starts_with(id))
                .unwrap_or_else(|| panic!("{id} in F-W24-15"))
                .to_owned()
        };
        let w26 = find("W26");
        assert!(w26.contains("WorkspaceLabelsEndpoint:get"), "{w26}");
        assert_eq!(django(&w26), "W26 GET workspaces/<slug>/labels/");
        assert_eq!(LABELS_PATH, "/api/workspaces/{slug}/labels/");

        let w28 = find("W28");
        assert!(w28.contains("WorkspaceStatesEndpoint:get"), "{w28}");
        assert_eq!(django(&w28), "W28 GET workspaces/<slug>/states/");
        assert_eq!(STATES_PATH, "/api/workspaces/{slug}/states/");

        let w29 = find("W29");
        assert!(w29.contains("WorkspaceEstimatesEndpoint:get"), "{w29}");
        assert_eq!(django(&w29), "W29 GET workspaces/<slug>/estimates/");
        assert_eq!(ESTIMATES_PATH, "/api/workspaces/{slug}/estimates/");

        let w30 = find("W30");
        assert!(w30.contains("WorkspaceModulesEndpoint:get"), "{w30}");
        assert_eq!(django(&w30), "W30 GET workspaces/<slug>/modules/");
        assert_eq!(MODULES_PATH, "/api/workspaces/{slug}/modules/");

        let w31 = find("W31");
        assert!(w31.contains("WorkspaceCyclesEndpoint:get"), "{w31}");
        assert_eq!(django(&w31), "W31 GET workspaces/<slug>/cycles/");
        assert_eq!(CYCLES_PATH, "/api/workspaces/{slug}/cycles/");
    }

    /// F-W24-13: the gate table rows these handlers decide through.
    #[test]
    fn gates_match_fixture_matrix() {
        use gates::Gate;
        let gate = |path: &str| {
            gates::gate_for("GET", path)
                .unwrap_or_else(|| panic!("gate row for {path}"))
                .gate
        };
        assert_eq!(gate("workspaces/<slug>/labels/"), Gate::ClassViewer);
        assert_eq!(gate("workspaces/<slug>/states/"), Gate::ClassEntity);
        assert_eq!(gate("workspaces/<slug>/estimates/"), Gate::ClassEntity);
        assert_eq!(gate("workspaces/<slug>/modules/"), Gate::ClassViewer);
        assert_eq!(gate("workspaces/<slug>/cycles/"), Gate::ClassViewer);
    }

    /// F-W24-13 cache sites: exactly the labels + estimates GETs carry
    /// `cache_response(2h)`; the key scheme and store rule are the gates
    /// port (this issue performs no cache I/O — see module docs).
    #[test]
    fn cache_sites_keys_and_timeout() {
        assert!(gates::cache_response_site(
            "GET",
            "workspaces/<slug>/labels/"
        ));
        assert!(gates::cache_response_site(
            "GET",
            "workspaces/<slug>/estimates/"
        ));
        assert!(!gates::cache_response_site(
            "GET",
            "workspaces/<slug>/states/"
        ));
        assert!(!gates::cache_response_site(
            "GET",
            "workspaces/<slug>/modules/"
        ));
        assert!(!gates::cache_response_site(
            "GET",
            "workspaces/<slug>/cycles/"
        ));
        assert_eq!(gates::CACHE_RESPONSE_TIMEOUT_SECS, 7200);
        assert_eq!(
            gates::cache_response_key("/api/workspaces/acme/labels/", Some("u1")),
            "/api/workspaces/acme/labels/:u1"
        );
        assert!(gates::should_cache_response(200, false));
        assert!(!gates::should_cache_response(200, true));
        assert!(!gates::should_cache_response(403, false));
    }

    /// Denial bytes served on these routes (lowercase `detail`, verified
    /// numerically against live Django).
    #[test]
    fn denial_bodies() {
        assert_eq!(
            gates::CLASS_DENIED_BODY,
            "{\"detail\":\"You do not have permission to perform this action.\"}"
        );
        assert_eq!(
            gates::ANON_BODY,
            "{\"detail\":\"Authentication credentials were not provided.\"}"
        );
    }

    /// Label render order is the 7 `Meta.fields` keys (no merged port
    /// covers this app shape — pinned here against the live oracle).
    #[test]
    fn label_keys_and_row() {
        assert_eq!(
            LABEL_READ_KEYS,
            [
                "parent",
                "name",
                "color",
                "id",
                "project_id",
                "workspace_id",
                "sort_order"
            ]
        );
        let row = LabelListRow {
            id: uuid::Uuid::parse_str("ef78bf83-2d85-404f-96d9-92226c853613").unwrap(),
            parent_id: None,
            name: "Parent".to_owned(),
            color: "#ff0000".to_owned(),
            project_id: Some(
                uuid::Uuid::parse_str("b145d2e7-5c4a-497e-9afe-45025ad9baeb").unwrap(),
            ),
            workspace_id: uuid::Uuid::parse_str("78d801a8-88ac-4485-9a81-927c44d6c4e1").unwrap(),
            sort_order: 100.0,
        };
        assert_eq!(
            render_label_row(&row),
            "{\"parent\":null,\"name\":\"Parent\",\"color\":\"#ff0000\",\"id\":\"ef78bf83-2d85-404f-96d9-92226c853613\",\"project_id\":\"b145d2e7-5c4a-497e-9afe-45025ad9baeb\",\"workspace_id\":\"78d801a8-88ac-4485-9a81-927c44d6c4e1\",\"sort_order\":100.0}"
        );
    }

    /// States render the 10 `STATE_FIELDS` keys with the injected order;
    /// the per-group math is the QRY-D builder (1/3 + 2/3 pinned).
    #[test]
    fn state_keys_order_math_and_row() {
        use pidash_services::app_project::ser_workflow::STATE_FIELDS;
        assert_eq!(
            STATE_FIELDS,
            [
                "id",
                "project_id",
                "workspace_id",
                "name",
                "color",
                "group",
                "default",
                "description",
                "sequence",
                "order"
            ]
        );
        assert_eq!(qx::state_group_order(1, 3), 1.0 / 3.0);
        assert_eq!(
            crate::paginator::py_float_str(qx::state_group_order(1, 3)),
            "0.3333333333333333"
        );
        assert_eq!(
            crate::paginator::py_float_str(qx::state_group_order(2, 3)),
            "0.6666666666666666"
        );
        let row = StateListRow {
            id: uuid::Uuid::parse_str("c12a83f3-0a49-4d4c-8115-f792afc8ef75").unwrap(),
            project_id: uuid::Uuid::parse_str("b145d2e7-5c4a-497e-9afe-45025ad9baeb").unwrap(),
            workspace_id: uuid::Uuid::parse_str("78d801a8-88ac-4485-9a81-927c44d6c4e1").unwrap(),
            name: "B-One".to_owned(),
            color: "#4A90D9".to_owned(),
            group: "backlog".to_owned(),
            is_default: false,
            description: String::new(),
            sequence: 10.0,
        };
        assert_eq!(
            render_state_row(&row, 0.5),
            "{\"id\":\"c12a83f3-0a49-4d4c-8115-f792afc8ef75\",\"project_id\":\"b145d2e7-5c4a-497e-9afe-45025ad9baeb\",\"workspace_id\":\"78d801a8-88ac-4485-9a81-927c44d6c4e1\",\"name\":\"B-One\",\"color\":\"#4A90D9\",\"group\":\"backlog\",\"default\":false,\"description\":\"\",\"sequence\":10.0,\"order\":0.5}"
        );
    }

    /// Estimates render through the merged D-25 shape: key order follows
    /// the wire consts, datetimes arrive pre-rendered.
    #[test]
    fn estimate_wire_order_through_merged_shape() {
        use pidash_services::app_project::ser_shared::{
            ESTIMATE_POINT_WIRE_FIELDS, WORKSPACE_ESTIMATE_WIRE_FIELDS,
        };
        let point = SharedEstimatePointRow {
            id: "1d19490b-9712-4cc1-b171-327af5e6f8c6",
            created_at: "2026-10-03T14:15:36.893522Z",
            updated_at: "2026-10-03T14:15:36.893522Z",
            deleted_at: None,
            key: 1,
            description: "d1",
            value: "1",
            created_by: None,
            updated_by: None,
            project: "b145d2e7-5c4a-497e-9afe-45025ad9baeb",
            workspace: "78d801a8-88ac-4485-9a81-927c44d6c4e1",
            estimate: "95845993-b07e-4300-a896-a4e252d94459",
        };
        let points = [point];
        let shared = SharedWorkspaceEstimateRow {
            id: "95845993-b07e-4300-a896-a4e252d94459",
            points: &points,
            created_at: "2026-10-03T14:15:36.893522Z",
            updated_at: "2026-10-03T14:15:36.893522Z",
            deleted_at: None,
            name: "Fib",
            description: "desc",
            estimate_type: "points",
            last_used: false,
            created_by: None,
            updated_by: None,
            project: "b145d2e7-5c4a-497e-9afe-45025ad9baeb",
            workspace: "78d801a8-88ac-4485-9a81-927c44d6c4e1",
        };
        let view = workspace_estimate_to_representation(&shared);
        let text = serde_json::to_string(&view).expect("view serializes");
        let parsed: serde_json::Value = serde_json::from_str(&text).expect("round-trips");
        let keys: Vec<&str> = parsed
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(keys, WORKSPACE_ESTIMATE_WIRE_FIELDS);
        let point_keys: Vec<&str> = parsed["points"][0]
            .as_object()
            .expect("point object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(point_keys, ESTIMATE_POINT_WIRE_FIELDS);
        assert!(text.starts_with(
            "{\"id\":\"95845993-b07e-4300-a896-a4e252d94459\",\"points\":[{\"id\":\"1d19490b-9712-4cc1-b171-327af5e6f8c6\""
        ));
    }

    /// Workspace cycle keys are the 22 serializer fields minus the two
    /// `SkipField`s, in `Meta.fields` order.
    #[test]
    fn cycle_keys_and_row() {
        use pidash_services::app_cycles::shape::CYCLE_SERIALIZER_FIELDS;
        let kept: Vec<&str> = CYCLE_SERIALIZER_FIELDS
            .iter()
            .filter(|key| **key != "is_favorite" && **key != "status")
            .copied()
            .collect();
        assert_eq!(kept, WORKSPACE_CYCLE_KEYS);
        let row = CycleListRow {
            id: uuid::Uuid::parse_str("13818c94-cc07-4094-baa1-6e723f33144f").unwrap(),
            workspace_id: uuid::Uuid::parse_str("78d801a8-88ac-4485-9a81-927c44d6c4e1").unwrap(),
            project_id: uuid::Uuid::parse_str("b145d2e7-5c4a-497e-9afe-45025ad9baeb").unwrap(),
            name: "C1".to_owned(),
            description: String::new(),
            start_date: None,
            end_date: None,
            owned_by_id: uuid::Uuid::parse_str("72ff6e6b-58bd-4aef-9a25-50f1cfb6f4e9").unwrap(),
            view_props: serde_json::json!({}),
            sort_order: 100.0,
            external_source: None,
            external_id: None,
            progress_snapshot: serde_json::json!({}),
            logo_props: serde_json::json!({}),
            total_issues: 1,
            cancelled_issues: 0,
            completed_issues: 0,
            started_issues: 0,
            unstarted_issues: 1,
            backlog_issues: 0,
        };
        assert_eq!(
            render_cycle_row(&row, &chrono_tz::UTC),
            "{\"id\":\"13818c94-cc07-4094-baa1-6e723f33144f\",\"workspace_id\":\"78d801a8-88ac-4485-9a81-927c44d6c4e1\",\"project_id\":\"b145d2e7-5c4a-497e-9afe-45025ad9baeb\",\"name\":\"C1\",\"description\":\"\",\"start_date\":null,\"end_date\":null,\"owned_by_id\":\"72ff6e6b-58bd-4aef-9a25-50f1cfb6f4e9\",\"view_props\":{},\"sort_order\":100.0,\"external_source\":null,\"external_id\":null,\"progress_snapshot\":{},\"logo_props\":{},\"total_issues\":1,\"cancelled_issues\":0,\"completed_issues\":0,\"started_issues\":0,\"unstarted_issues\":1,\"backlog_issues\":0}"
        );
    }

    /// Workspace module keys are the 29 list fields minus the three
    /// `SkipField`s; `member_ids` renders literal null; dates render
    /// `YYYY-MM-DD`; datetimes shift into the actor zone.
    #[test]
    fn module_keys_and_row() {
        use pidash_services::app_modules::shape::MODULE_LIST_FIELD_ORDER;
        let kept: Vec<&str> = MODULE_LIST_FIELD_ORDER
            .iter()
            .filter(|key| {
                **key != "total_estimate_points"
                    && **key != "completed_estimate_points"
                    && **key != "is_favorite"
            })
            .copied()
            .collect();
        assert_eq!(kept, WORKSPACE_MODULE_KEYS);
        let created = chrono::DateTime::parse_from_rfc3339("2026-10-03T14:15:36.945250Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let row = ModuleListRow {
            id: uuid::Uuid::parse_str("478a049b-0fee-4fce-8c82-970aa64d1247").unwrap(),
            workspace_id: uuid::Uuid::parse_str("78d801a8-88ac-4485-9a81-927c44d6c4e1").unwrap(),
            project_id: uuid::Uuid::parse_str("b145d2e7-5c4a-497e-9afe-45025ad9baeb").unwrap(),
            name: "M1".to_owned(),
            description: String::new(),
            description_text: None,
            description_html: None,
            start_date: Some(chrono::NaiveDate::from_ymd_opt(2026, 1, 2).unwrap()),
            target_date: None,
            status: "planned".to_owned(),
            lead_id: None,
            view_props: serde_json::json!({}),
            sort_order: 65535.0,
            external_source: None,
            external_id: None,
            logo_props: serde_json::json!({"a": 1}),
            created_at: created,
            updated_at: created,
            archived_at: None,
            total_issues: 1,
            cancelled_issues: 0,
            completed_issues: 0,
            started_issues: 0,
            unstarted_issues: 1,
            backlog_issues: 0,
        };
        let utc = render_module_row(&row, &chrono_tz::UTC);
        assert!(utc.contains("\"member_ids\":null"), "{utc}");
        assert!(utc.contains("\"start_date\":\"2026-01-02\""), "{utc}");
        assert!(
            utc.contains("\"created_at\":\"2026-10-03T14:15:36.945250Z\""),
            "{utc}"
        );
        assert!(!utc.contains("is_favorite"), "{utc}");
        assert!(!utc.contains("estimate_points"), "{utc}");
        let eastern: Tz = "America/New_York".parse().unwrap();
        let shifted = render_module_row(&row, &eastern);
        assert!(
            shifted.contains("\"created_at\":\"2026-10-03T10:15:36.945250-04:00\""),
            "{shifted}"
        );
    }

    /// Executed statements splice the ported predicates verbatim: no
    /// `:named` placeholder survives, binds are positional, and the
    /// annotation aliases + orders ride along.
    #[test]
    fn executed_sql_splices_ported_fragments() {
        let labels = labels_sql();
        assert!(
            !labels.contains(":slug") && !labels.contains(":user"),
            "{labels}"
        );
        assert!(labels.contains("$1") && labels.contains("$2"), "{labels}");
        assert!(
            labels.contains("ORDER BY labels.created_at DESC"),
            "{labels}"
        );

        let states = states_sql();
        assert!(
            !states.contains(":slug") && !states.contains(":user"),
            "{states}"
        );
        assert!(states.contains("states.is_triage = FALSE"), "{states}");
        assert!(states.contains("ORDER BY states.sequence ASC"), "{states}");

        let ids = estimate_ids_sql();
        assert!(!ids.contains(":slug"), "{ids}");
        let estimates = estimates_sql();
        assert!(!estimates.contains(":estimate_ids"), "{estimates}");
        assert!(estimates.contains("= ANY($1)"), "{estimates}");
        assert!(
            estimates.contains("ORDER BY estimates.name ASC"),
            "{estimates}"
        );
        let points = estimate_points_sql();
        assert!(points.contains("= ANY($1)"), "{points}");
        assert!(
            points.contains("ORDER BY estimate_points.value ASC"),
            "{points}"
        );

        let cycles = cycles_sql();
        for alias in [
            "total_issues",
            "cancelled_issues",
            "completed_issues",
            "started_issues",
            "unstarted_issues",
            "backlog_issues",
        ] {
            assert!(cycles.contains(&format!("AS {alias}")), "{cycles}");
        }
        assert!(cycles.contains("COUNT(s.group)"), "{cycles}");
        assert!(!cycles.contains("COUNT(DISTINCT"), "{cycles}");
        assert!(cycles.contains("GROUP BY cycles.id"), "{cycles}");
        assert!(
            cycles.contains("ORDER BY cycles.created_at DESC"),
            "{cycles}"
        );

        let modules = modules_sql();
        for alias in [
            "total_issues",
            "cancelled_issues",
            "completed_issues",
            "started_issues",
            "unstarted_issues",
            "backlog_issues",
        ] {
            assert!(modules.contains(&format!("AS {alias}")), "{modules}");
        }
        assert!(modules.contains("COUNT(DISTINCT mi.id)"), "{modules}");
        assert!(modules.contains("GROUP BY modules.id"), "{modules}");
        assert!(
            modules.contains("ORDER BY modules.created_at DESC"),
            "{modules}"
        );
    }
}
