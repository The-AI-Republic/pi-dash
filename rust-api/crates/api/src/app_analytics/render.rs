//! Workspace advance-analytics handlers (PIDASHCONV-414, handlers-C).
//!
//! Ports the three workspace routes of
//! `apps/api/pi_dash/app/views/analytic/advance.py` (drift baseline
//! `01a93e17`) with identical URL paths, status codes and JSON bytes:
//!
//! - `GET workspaces/<slug>/advance-analytics/` (`AdvanceAnalyticsEndpoint.get`,
//!   `advance.py:137-154`): `?tab=overview` (default) or `?tab=work-items`;
//!   anything else is 400 `{"message": "Invalid tab"}`.
//! - `GET workspaces/<slug>/advance-analytics-stats/`
//!   (`AdvanceAnalyticsStatsEndpoint.get`, `advance.py:191-204`): only
//!   `?type=work-items` (default) serves; anything else is 400
//!   `{"message": "Invalid type"}`.
//! - `GET workspaces/<slug>/advance-analytics-charts/`
//!   (`AdvanceAnalyticsChartEndpoint.get`, `advance.py:318-351`):
//!   `?type=projects` (default), `custom-work-items` or `work-items`;
//!   anything else is 400 `{"message": "Invalid type"}`.
//!
//! `AdvanceAnalyticsBaseView.initialize_workspace` (`advance.py:32-43`) runs
//! first on every path: `get_analytics_filters(slug, type, user, date_filter,
//! project_ids)` (`utils/date_utils.py:125-191`) splits into `base_filters`
//! / `project_filters` plus `analytics_date_range` (`type="analytics"`) or
//! `chart_period_range` (`type="chart"`). The scope fragments below reproduce
//! those Django predicates verbatim (see [`FilterCtx`]).
//!
//! Layering: the SQL shells live in
//! `pidash_services::app_analytics::queries` (FX-A-Q-04, PIDASHCONV-349) and
//! the gates in [`super::gates`] (FX-A-G-01, PIDASHCONV-358). This module owns
//! the HTTP shell (routes, session auth, gate), the scope fragments, the row
//! fetching and the DRF-byte rendering — including `build_analytics_chart`
//! (`utils/build_chart.py:151-194`), whose owner is this handlers layer (the
//! queries layer returns only its scoped predicate).
//!
//! Fixture ids: FX-A-H-01 (these 3 routes,
//! `rust-api/fixtures/app_analytics/handlers/analytics_handlers.golden.json`),
//! FX-A-Q-04 (query builders), FX-A-G-01 (these routes).
//!
//! Ported quirks (translate, don't redesign; also listed in the PR):
//! - B2 (`advance.py:62-64`): `get_filtered_counts` answers `{"count": n}`
//!   only — the previous-window helper exists but its response line is
//!   commented out.
//! - Dead method: `AdvanceAnalyticsStatsEndpoint.get_project_issues_stats`
//!   (`advance.py:156-175`, which would apply `chart_period_range`) is never
//!   called; `get` calls `get_work_items_stats` (`advance.py:177-189`), so
//!   `?date_filter=` is silently ignored on the stats route.
//! - Intake differs by path: the overview total filters
//!   `issue_intake__status__in=["-2","-1","0","1","2"]` (`advance.py:118-122`,
//!   with its TODO comment) while `project_chart` counts
//!   `issue_intake__isnull=False` (`advance.py:222-224`).
//! - The overview intake (and only it) reads `Issue.objects` — the inherited
//!   soft-delete manager with no triage/archived/draft exclusions
//!   (`mixins.py:61-67`) — so triage, archived and draft issues count there
//!   (every other issue read uses `issue_objects`).
//! - With `?project_ids=` the member source *switches* to `ProjectMember`
//!   (`advance.py:106-111`) and drops the workspace scoping entirely.
//! - The chart member count excludes neither bots nor inactive-beyond-active
//!   (`advance.py:231-233` — no `member__is_bot` predicate, unlike overview).
//! - A malformed `?project_ids=` entry 400s (`{"error": "Please provide valid
//!   detail"}`, the `handle_exception` `ValidationError` branch) because the
//!   UUID coercion runs inside dispatch.
//! - `get_chart_period_range` returns `None` (not the documented
//!   `"last_7_days"` default) when `date_filter` is absent or unknown
//!   (`date_utils.py:122-124`); `get_analytics_date_range` likewise.
//! - The monthly zero-fill reuses `start_date` as-is when the period range
//!   is set (`advance.py:267-270` overwrites the workspace month-start with
//!   the raw period start): a mid-month start empties the loop outright, and
//!   the day-preserving month step 500s on invalid days (e.g. Aug 31 ->
//!   Sept 31, Python `replace` raising `ValueError`).
//! - The chart `ValidationError`s render as a one-element JSON LIST
//!   (`["Invalid x_axis field: ..."]`, DRF wrapping the bare string), and
//!   the Invalid tab/type 400s render compact (`{"message":"..."}`) — both
//!   verified against live Django.
//!
//! Out of scope (sibling handler issues): workspace base analytics + the
//! analytic-view viewset (PIDASHCONV-389), export-issues (PIDASHCONV-430).
//! Query builders stay in `pidash_services::app_analytics::queries`, gates in
//! [`super::gates`].
//! Handlers-B (PIDASHCONV-399) lives in this same module: the four
//! owned routes `saved-analytic-view/<analytic_id>/` (GET),
//! `export-analytics/` (POST), `default-analytics/` (GET) and
//! `project-stats/` (GET) with their session-auth-then-gate order, plot
//! COUNT/SUM shapes and DRF-byte rendering. Name-sharing with the
//! advance shell resolved at merge: [`routes`] serves all ten paths,
//! `json_value_response` / `pool_owned` / `IssuesDenial` are the
//! handlers-B spellings of the shared private helpers.
//!
//! Handlers-D (PIDASHCONV-424, this issue) lives here too: the three
//! project advance routes of
//! `apps/api/pi_dash/app/views/analytic/project_analytics.py` —
//! `GET projects/<project_id>/advance-analytics[/-stats/-charts]/`
//! (`ProjectAdvanceAnalyticsEndpoint.get`, `:85-96`,
//! `ProjectAdvanceAnalyticsStatsEndpoint.get`, `:166-181`,
//! `ProjectAdvanceAnalyticsChartEndpoint.get`, `:318-367`, plus
//! `initialize_workspace`, `:32-43`) — reusing [`build_analytics_chart`]
//! (with a project `extra_scope`), the scope helpers and the monthly
//! zero-fill ([`completion_monthly_body`]) — same module, no fork.
//! Fixture ids: FX-A-H-01 (these 3 routes), FX-A-Q-05 (query builders),
//! FX-A-G-01 (these routes).
//!
//! Ported project quirks (also listed in the PR):
//! - B3 (`project_analytics.py:320`): the chart `type` defaults to
//!   `"projects"` but no `projects` branch exists, so a bare GET 400s.
//! - B4 (`project_analytics.py:63-74` and siblings): the cycle/module
//!   branches scope the through id-list with `base_filters` but drop the
//!   route's `project_id` entirely (only the custom chart keeps both).
//! - B5 (`project_analytics.py:253`): the daily branch counts
//!   `count = created + completed`, so completed rows count twice.
//! - The cycle/module chart lookups are global by id (no scoping); a
//!   missing row or start date is `{"data":[],"schema":{}}`, while a
//!   missing END date 500s (`cycle.end_date.date()` / `<= None`).
//! - `?cycle_id=` (empty) is "present" (`GET.get` returns `""`), so it
//!   400s on the UUID coercion; cycle wins over module (`if`/`elif`).
//! - The stats method never reads `chart_period_range`: `?date_filter=`
//!   is silently ignored there.
//! - The Q-05 `project_completion_daily_sql` sketch is not valid SQL (it
//!   quotes the Django lookup `"issue"."state__group"` verbatim); the
//!   daily path owns the complete statement instead (services is
//!   read-only for this layer).
//!

use std::collections::HashMap;

use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use chrono::{DateTime, Datelike, NaiveDate, Utc};
use serde_json::{Map, Value};
use sqlx::Row;

use crate::middleware::SessionHandle;
use crate::state::AppState;

use super::gates::{decide_gate, gate_for, tenant_context, GateOutcome};
use crate::app_issues::{fetch_json_rows, query_last, Denial as IssuesDenial, QueryMap};
use pidash_auth::permissions::allow::AllowFacts;
use pidash_auth::permissions::{ROLE_ADMIN, ROLE_GUEST, ROLE_MEMBER};
use pidash_db::issue_filters::issue_filters_get;
use pidash_services::app_analytics::queries as q;
use pidash_types::WorkspaceId;

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------

/// Canonical gate-table paths for the three owned rows (mirrors
/// [`super::gates::GATES` so the table stays the single source of truth).
pub const PATH_ADVANCE: &str = "workspaces/<slug>/advance-analytics/";
pub const PATH_STATS: &str = "workspaces/<slug>/advance-analytics-stats/";
pub const PATH_CHARTS: &str = "workspaces/<slug>/advance-analytics-charts/";
/// Canonical gate-table paths for the three project advance rows
/// (PIDASHCONV-424, handlers-D).
pub const PATH_PROJECT_ADVANCE: &str = "workspaces/<slug>/projects/<id>/advance-analytics/";
pub const PATH_PROJECT_STATS: &str = "workspaces/<slug>/projects/<id>/advance-analytics-stats/";
pub const PATH_PROJECT_CHARTS: &str = "workspaces/<slug>/projects/<id>/advance-analytics-charts/";

/// Register the owned D-35 analytic paths: the three workspace
/// advance-analytics GETs (PIDASHCONV-414, handlers-C) plus the four
/// handlers-B routes (PIDASHCONV-399) — saved-analytic-view GET,
/// export-analytics POST, default-analytics GET, project-stats GET —
/// plus the three project advance-analytics GETs (PIDASHCONV-424,
/// handlers-D).
/// Owned methods serve from Rust; everything else proxies to Django (its
/// 401-anon-before-405 and DRF metadata live there) — the `app_cycles`
/// precedent. Sibling handler issues extend this merge; merges keep both
/// sides.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/workspaces/{slug}/advance-analytics/",
            owned(axum::routing::get(advance_get), &["GET"]),
        )
        .route(
            "/api/workspaces/{slug}/advance-analytics-stats/",
            owned(axum::routing::get(stats_get), &["GET"]),
        )
        .route(
            "/api/workspaces/{slug}/advance-analytics-charts/",
            owned(axum::routing::get(charts_get), &["GET"]),
        )
        .route(
            "/api/workspaces/{slug}/projects/{project_id}/advance-analytics/",
            owned(axum::routing::get(project_advance_get), &["GET"]),
        )
        .route(
            "/api/workspaces/{slug}/projects/{project_id}/advance-analytics-stats/",
            owned(axum::routing::get(project_advance_stats_get), &["GET"]),
        )
        .route(
            "/api/workspaces/{slug}/projects/{project_id}/advance-analytics-charts/",
            owned(axum::routing::get(project_advance_charts_get), &["GET"]),
        )
        .route(
            "/api/workspaces/{slug}/saved-analytic-view/{analytic_id}/",
            owned_get(get(get_saved_analytic)),
        )
        .route(
            "/api/workspaces/{slug}/export-analytics/",
            owned_post(post(post_export_analytics)),
        )
        .route(
            "/api/workspaces/{slug}/default-analytics/",
            owned_get(get(get_default_analytics)),
        )
        .route(
            "/api/workspaces/{slug}/project-stats/",
            owned_get(get(get_project_stats)),
        )
}

/// An owned path: listed methods serve from Rust, everything else proxies
/// to Django. OPTIONS proxies too: DRF answers metadata (401 anon / 200
/// authed) where axum would 405. HEAD rides axum's `get` handling like
/// Django's `GET`-backed `HEAD` (the `app_views_search` precedent).
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

/// Exact bytes of the DRF `IsAuthenticated` denial.
pub const UNAUTHENTICATED_BODY: &str = super::gates::ANON_BODY;
/// The `@allow_permission` 403 (`app/permissions/base.py`).
pub const FORBIDDEN_BODY: &str = super::gates::FORBIDDEN_BODY;
/// `handle_exception`'s `ObjectDoesNotExist` branch (`app/views/base.py`):
/// the monthly chart's `Workspace.objects.get(slug)` miss (unreachable past
/// the gate, kept for fidelity).
pub const NOT_FOUND_BODY: &str = super::gates::NOT_FOUND_BODY;
/// `handle_exception`'s `ValidationError` branch: malformed `?project_ids=`
/// entries (UUID coercion inside dispatch).
pub const INVALID_DETAIL_BODY: &str = r#"{"error":"Please provide valid detail"}"#;
/// `handle_exception`'s generic 500 branch.
pub const SERVER_ERROR_BODY: &str = r#"{"error":"Something went wrong please try again later"}"#;
/// `{"message": "Invalid tab"}` 400 (`advance.py:152`) in DRF's compact
/// rendering (no space after the colon — verified against live Django; the
/// Q-04 builder const keeps a space and is not used for the body).
pub const INVALID_TAB_BODY: &str = r#"{"message":"Invalid tab"}"#;
/// `{"message": "Invalid type"}` 400 (`advance.py:351`) in DRF's compact
/// rendering (same caveat as [`INVALID_TAB_BODY`]).
pub const INVALID_TYPE_BODY: &str = r#"{"message":"Invalid type"}"#;
/// `Project.resolve` miss (`Http404("Project not found")`,
/// `project.py:218`): DRF propagates the args
/// (`NotFound("Project not found")`), rendered compact with a lowercase
/// `detail` key (verified against live Django; the `v1_projects`
/// `{"Detail": ...}` spelling does not match this path).
pub const PROJECT_NOT_FOUND_BODY: &str = r#"{"detail":"Project not found"}"#;
/// Empty chart body when cycle/module/project dates are missing
/// (`project_analytics.py:201,213,221`) in DRF's compact rendering (the
/// Q-05 builder const keeps spaces and is not used for the body).
pub const EMPTY_PROJECT_CHART_BODY: &str = r#"{"data":[],"schema":{}}"#;
/// DRF's rendering of the `build_analytics_chart` `ValidationError`
/// (`rest_framework.exceptions.ValidationError` is answered by DRF's own
/// `exception_handler`, not the view's `handle_exception` branch): a bare
/// string detail wraps into a one-element LIST, 400 `["<message>"]`
/// (verified against live Django).
fn invalid_axis_body(message: &str) -> String {
    serde_json::to_string(&vec![message]).expect("axis body")
}

/// Handler failure with its exact status + body.
#[derive(Debug)]
pub(crate) enum Denial {
    /// 401, DRF `NotAuthenticated`.
    Unauthorized,
    /// 403, `@allow_permission` body.
    Forbidden,
    /// 400, `{"message": "Invalid tab"}`.
    InvalidTab,
    /// 400, `{"message": "Invalid type"}`.
    InvalidType,
    /// 400, `{"error": "Please provide valid detail"}`.
    InvalidDetail,
    /// 404, `ObjectDoesNotExist` branch.
    NotFound,
    /// 404, `Project.resolve` miss on a non-UUID project identifier.
    ProjectNotFound,
    /// 500, generic branch.
    ServerError,
    /// A pre-rendered exact body with its status.
    Raw(StatusCode, String),
}

impl Denial {
    fn status_and_body(&self) -> (StatusCode, String) {
        match self {
            Denial::Unauthorized => (StatusCode::UNAUTHORIZED, UNAUTHENTICATED_BODY.to_owned()),
            Denial::Forbidden => (StatusCode::FORBIDDEN, FORBIDDEN_BODY.to_owned()),
            Denial::InvalidTab => (StatusCode::BAD_REQUEST, INVALID_TAB_BODY.to_owned()),
            Denial::InvalidType => (StatusCode::BAD_REQUEST, INVALID_TYPE_BODY.to_owned()),
            Denial::InvalidDetail => (StatusCode::BAD_REQUEST, INVALID_DETAIL_BODY.to_owned()),
            Denial::NotFound => (StatusCode::NOT_FOUND, NOT_FOUND_BODY.to_owned()),
            Denial::ProjectNotFound => (StatusCode::NOT_FOUND, PROJECT_NOT_FOUND_BODY.to_owned()),
            Denial::ServerError => (
                StatusCode::INTERNAL_SERVER_ERROR,
                SERVER_ERROR_BODY.to_owned(),
            ),
            Denial::Raw(status, body) => (*status, body.clone()),
        }
    }
}

impl IntoResponse for Denial {
    fn into_response(self) -> Response {
        let (status, body) = self.status_and_body();
        Response::builder()
            .status(status)
            .header(header::CONTENT_TYPE, "application/json")
            .body(axum::body::Body::from(body))
            .expect("static denial response")
    }
}

fn json_response(status: StatusCode, body: String) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body))
        .expect("advance response")
}

// ---------------------------------------------------------------------------
// Request context: auth + tenant + membership
// ---------------------------------------------------------------------------

/// Session auth (`BaseSessionAuthentication` + `IsAuthenticated` on the
/// view): anonymous answers the DRF `NotAuthenticated` body before
/// anything else runs (the `app_cycles` precedent).
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

/// The active workspace-membership role for the advance gates, resolved with
/// the same row filters Python uses: `member=user, workspace__slug=slug,
/// role__in=[ADMIN, MEMBER], is_active=True` (`app/permissions/base.py`) —
/// the `app_cycles` precedent (soft-deleted rows excluded on both sides).
async fn workspace_role(
    pool: &sqlx::PgPool,
    slug: &str,
    user_id: &uuid::Uuid,
) -> Result<Option<i16>, Denial> {
    let row: Option<(Option<i16>,)> = sqlx::query_as(
        r#"SELECT wm.role FROM workspace_members wm
           JOIN workspaces w ON w.id = wm.workspace_id AND w.deleted_at IS NULL
           WHERE w.slug = $1 AND wm.member_id = $2 AND wm.is_active AND wm.deleted_at IS NULL"#,
    )
    .bind(slug)
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    // `role` is non-nullable; the outer Option is row presence.
    Ok(row.and_then(|row| row.0))
}

/// Build the gate facts for a workspace advance route: WORKSPACE-level
/// ADMIN/MEMBER, no creator bypass (`advance.py:137,191,318`, FX-A-G-01).
fn advance_facts(slug: &str, role: Option<i16>) -> AllowFacts {
    let allowed = matches!(role.map(i32::from), Some(ROLE_ADMIN) | Some(ROLE_MEMBER));
    AllowFacts {
        workspace: WorkspaceId::from(slug),
        authenticated: true,
        is_workspace_member: role.is_some(),
        has_allowed_workspace_role: allowed,
        is_creator: false,
        has_allowed_project_role: false,
        is_project_member: false,
        is_workspace_admin: role.map(i32::from) == Some(ROLE_ADMIN),
    }
}

/// Enforce the gate-table row for one method+path: anonymous never
/// reaches here ([`actor`] denied first); a deny answers the decorator
/// 403.
fn check_gate(method: &str, path: &str, slug: &str, role: Option<i16>) -> Result<(), Denial> {
    let row = super::gates::gate_for(method, path).ok_or(Denial::ServerError)?;
    let scope = super::gates::tenant_context(slug);
    match super::gates::decide_gate(&row.gate, &scope, &advance_facts(slug, role)) {
        super::gates::GateOutcome::Allow => Ok(()),
        _ => Err(Denial::Forbidden),
    }
}

/// Resolve the slug-or-UUID `project_id` kwarg (`_rewrite_project_kwarg`,
/// `app/views/base.py:49-77`, via `Project.resolve`,
/// `db/models/project.py:191-219`): UUIDs pass through unverified (the gate
/// 403s unknown ones); other identifiers match `identifier = UPPER(TRIM(raw))`
/// in the workspace; misses are `Http404("Project not found")`. Anonymous
/// callers skip the rewrite ([`actor`] denies first) so the slug-existence
/// oracle stays closed — the `v1_projects` precedent (PIDASHCONV-372).
async fn resolve_project_id(
    pool: &sqlx::PgPool,
    slug: &str,
    raw: &str,
) -> Result<uuid::Uuid, Denial> {
    if let Ok(id) = raw.parse::<uuid::Uuid>() {
        return Ok(id);
    }
    let upper = raw.trim().to_uppercase();
    let row: Option<(uuid::Uuid,)> = sqlx::query_as(
        r#"SELECT p.id FROM projects p JOIN workspaces w ON w.id = p.workspace_id
           WHERE w.slug = $1 AND p.identifier = $2 AND p.deleted_at IS NULL"#,
    )
    .bind(slug)
    .bind(upper)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    row.map(|row| row.0).ok_or(Denial::ProjectNotFound)
}

/// The active project-membership role for the PROJECT-level gates:
/// `member=user, workspace__slug=slug, project_id=project_id, is_active=True`
/// (`app/permissions/base.py:53-59`) — the `app_cycles` precedent
/// (soft-deleted rows excluded on both sides).
async fn project_role(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    user_id: &uuid::Uuid,
) -> Result<Option<i16>, Denial> {
    let row: Option<(Option<i16>,)> = sqlx::query_as(
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
    Ok(row.and_then(|row| row.0))
}

/// Build the gate facts for a project advance route: PROJECT-level
/// ADMIN/MEMBER (advance + stats) or ADMIN/MEMBER/GUEST (charts), with the
/// workspace-admin override (`app/permissions/base.py:61-78`, FX-A-G-01).
/// `allowed` carries the route's roles; `ws_role` / `project_role` are the
/// active membership rows (any role).
fn project_facts(
    slug: &str,
    ws_role: Option<i16>,
    project_role: Option<i16>,
    allowed: &[i32],
) -> AllowFacts {
    AllowFacts {
        workspace: WorkspaceId::from(slug),
        authenticated: true,
        is_workspace_member: ws_role.is_some(),
        has_allowed_workspace_role: false,
        is_creator: false,
        has_allowed_project_role: project_role
            .map(i32::from)
            .is_some_and(|role| allowed.contains(&role)),
        is_project_member: project_role.is_some(),
        is_workspace_admin: ws_role.map(i32::from) == Some(ROLE_ADMIN),
    }
}

/// Enforce the gate-table row for one project advance path: anonymous never
/// reaches here ([`actor`] denied first); a deny answers the decorator 403.
fn check_project_gate(
    path: &str,
    slug: &str,
    ws_role: Option<i16>,
    project_role: Option<i16>,
    allowed: &[i32],
) -> Result<(), Denial> {
    let row = super::gates::gate_for("GET", path).ok_or(Denial::ServerError)?;
    let scope = super::gates::tenant_context(slug);
    match super::gates::decide_gate(
        &row.gate,
        &scope,
        &project_facts(slug, ws_role, project_role, allowed),
    ) {
        super::gates::GateOutcome::Allow => Ok(()),
        _ => Err(Denial::Forbidden),
    }
}

/// Allowed roles per project advance route (`project_analytics.py:84,165,317`).
const PROJECT_ADMIN_MEMBER: &[i32] = &[ROLE_ADMIN, ROLE_MEMBER];
const PROJECT_ADMIN_MEMBER_GUEST: &[i32] = &[ROLE_ADMIN, ROLE_MEMBER, ROLE_GUEST];

// ---------------------------------------------------------------------------
// Filters: get_analytics_filters + date ranges
// ---------------------------------------------------------------------------

/// The parsed `get_analytics_filters` result
/// (`utils/date_utils.py:125-191`): the project-id narrowing plus whichever
/// date range the endpoint type selects. `type="analytics"` yields
/// `analytics_date_range` (datetimes, `get_analytics_date_range`) and
/// `type="chart"` yields `chart_period_range` (dates,
/// `get_chart_period_range`).
pub(crate) struct FilterCtx {
    /// `project_ids.split(",")` verbatim (`date_utils.py:155-156`); each
    /// entry must parse as a UUID (the `id__in` coercion, else the
    /// `ValidationError` 400).
    pub(crate) project_ids: Vec<String>,
    /// Current-window `(gte, lte)` on `created_at` (naive datetimes read in
    /// `TIME_ZONE "UTC"`, `settings/common.py:361-362`).
    pub(crate) window: Option<(DateTime<Utc>, DateTime<Utc>)>,
    /// Chart `(start, end)` dates on `created_at::date`.
    pub(crate) period: Option<(NaiveDate, NaiveDate)>,
}

/// Parse one `?project_ids=` csv exactly like `get_analytics_filters`:
/// `str()` each comma entry, no validation here — validation happens at
/// query time (the UUID coercion), mirrored by [`FilterCtx::checked_ids`].
fn parse_project_ids(raw: Option<&str>) -> Vec<String> {
    match raw {
        Some(csv) if !csv.is_empty() => csv.split(',').map(str::to_owned).collect(),
        _ => Vec::new(),
    }
}

/// `get_analytics_date_range` (`date_utils.py:17-92`): the current (and
/// previous, unused here — ported quirk B2) window for one `date_filter`
/// name. `custom` without explicit bounds is out of scope: the advance
/// views never pass `start_date`/`end_date`, so it always falls through to
/// `None`, exactly like Python with its missing arguments.
fn analytics_date_range(
    date_filter: Option<&str>,
    now: DateTime<Utc>,
) -> Option<(DateTime<Utc>, DateTime<Utc>)> {
    let today = now.date_naive();
    let days_back = match date_filter? {
        "yesterday" => return Some(day_bounds(today.pred_opt()?)),
        "last_7_days" => 7,
        "last_30_days" => 30,
        "last_3_months" => 90,
        _ => return None,
    };
    let start = today - chrono::Days::new(days_back);
    Some(day_bounds2(start, today))
}

fn day_bounds(day: NaiveDate) -> (DateTime<Utc>, DateTime<Utc>) {
    day_bounds2(day, day)
}

/// `datetime.combine(day, datetime.min.time())` /
/// `datetime.combine(day, datetime.max.time())` (23:59:59.999999), read in
/// UTC.
fn day_bounds2(start: NaiveDate, end: NaiveDate) -> (DateTime<Utc>, DateTime<Utc>) {
    let gte = start.and_hms_opt(0, 0, 0).expect("midnight").and_utc();
    let lte = end
        .and_hms_micro_opt(23, 59, 59, 999_999)
        .expect("end of day")
        .and_utc();
    (gte, lte)
}

/// `get_chart_period_range` (`date_utils.py:94-124`): `(start, end)` dates.
/// Absent or unknown names answer `None` — the docstring's `"last_7_days"`
/// default is not in the code (ported quirk).
fn chart_period_range(
    date_filter: Option<&str>,
    now: DateTime<Utc>,
) -> Option<(NaiveDate, NaiveDate)> {
    let today = now.date_naive();
    match date_filter? {
        "yesterday" => {
            let day = today.pred_opt()?;
            Some((day, day))
        }
        "last_7_days" => Some((today - chrono::Days::new(7), today)),
        "last_30_days" => Some((today - chrono::Days::new(30), today)),
        "last_3_months" => Some((today - chrono::Days::new(90), today)),
        _ => None,
    }
}

impl FilterCtx {
    /// `initialize_workspace` + `get_analytics_filters` for one request:
    /// `kind` is `"analytics"` (`AdvanceAnalyticsEndpoint`) or `"chart"`
    /// (stats + charts endpoints).
    pub(crate) fn parse(params: &HashMap<String, String>, kind: &str, now: DateTime<Utc>) -> Self {
        let project_ids = parse_project_ids(params.get("project_ids").map(String::as_str));
        let date_filter = params.get("date_filter").map(String::as_str);
        let (window, period) = match kind {
            "analytics" => (analytics_date_range(date_filter, now), None),
            _ => (None, chart_period_range(date_filter, now)),
        };
        Self {
            project_ids,
            window,
            period,
        }
    }

    /// The UUID coercion Django applies to every `id__in`/`project_id__in`
    /// entry at query time: valid entries pass through verbatim (bound as
    /// parameters, like Django), a malformed one is the `ValidationError`
    /// 400. Quote-escapes the literals (same doubling as the Q-03 project
    /// stats builder) so the verbatim list cannot break out of the string.
    pub(crate) fn checked_id_list(&self) -> Result<Vec<String>, Denial> {
        self.project_ids
            .iter()
            .map(|id| {
                id.parse::<uuid::Uuid>()
                    .map(|parsed| format!("'{}'", parsed.as_hyphenated()))
                    .map_err(|_| Denial::InvalidDetail)
            })
            .collect()
    }

    /// `AND "issues"."project_id" IN (...)` / `AND "projects"."id" IN (...)`
    /// narrowing (`base_filters["project_id__in"]` /
    /// `project_filters["id__in"]`, `date_utils.py:168-170`).
    pub(crate) fn project_narrowing(&self, column: &str) -> Result<String, Denial> {
        let ids = self.checked_id_list()?;
        if ids.is_empty() {
            return Ok(String::new());
        }
        Ok(format!(" AND {column} IN ({})", ids.join(",")))
    }

    /// `(gte_ph, lte_ph)` placeholders for the current window, or `None`.
    /// The standard statements bind `$1` = slug, `$2` = user first, so the
    /// window starts at `$3`; member statements with fewer leading binds
    /// pass their own `next` via [`FilterCtx::window_ph_at`].
    pub(crate) fn window_ph(&self) -> Option<(String, String)> {
        self.window_ph_at(3)
    }

    /// `(gte_ph, lte_ph)` placeholders for the chart period, or `None`.
    pub(crate) fn period_ph(&self) -> Option<(String, String)> {
        self.period_ph_at(3)
    }

    /// Window placeholders starting at bind `next` (Postgres `$N` must be
    /// dense over the statement's actual binds — sqlx binds positionally).
    pub(crate) fn window_ph_at(&self, next: u32) -> Option<(String, String)> {
        self.window
            .as_ref()
            .map(|_| (format!("${next}"), format!("${}", next + 1)))
    }

    /// Period placeholders starting at bind `next` (see
    /// [`FilterCtx::window_ph_at`]).
    pub(crate) fn period_ph_at(&self, next: u32) -> Option<(String, String)> {
        self.period
            .as_ref()
            .map(|_| (format!("${next}"), format!("${}", next + 1)))
    }
}

// ---------------------------------------------------------------------------
// Scope fragments: base_filters / project_filters as SQL
// ---------------------------------------------------------------------------

/// The `Issue.issue_objects` manager scope (`db/models/issue.py:95-104`):
/// soft-delete plus the four `.exclude()`s. The triage exclusion compiles
/// with Django's `exclude()` NULL rule over the nullable `state` FK
/// (`null=True`, `issue.py:125`): `NOT (group = 'triage' AND group IS NOT
/// NULL)`, so NULL-state rows are KEPT while triage rows drop (the
/// `app_cycles` Q5 precedent, verified against live Django).
pub const ISSUE_MANAGER_SCOPE: &str = "\"issues\".\"deleted_at\" IS NULL \
     AND NOT (\"states\".\"group\" = 'triage' AND \"states\".\"group\" IS NOT NULL) \
     AND \"issues\".\"archived_at\" IS NULL \
     AND \"projects\".\"archived_at\" IS NULL \
     AND \"issues\".\"is_draft\" = FALSE";

/// JOINs every issue read needs: project + workspace + the project-membership
/// row (`project__project_projectmember__member/is_active`,
/// `date_utils.py:160-166`) plus the nullable state join for the manager
/// scope above.
pub const ISSUE_JOINS: &str =
    " INNER JOIN \"projects\" ON (\"issues\".\"project_id\" = \"projects\".\"id\") \
     INNER JOIN \"workspaces\" ON (\"projects\".\"workspace_id\" = \"workspaces\".\"id\") \
     INNER JOIN \"project_members\" ON (\"projects\".\"id\" = \"project_members\".\"project_id\") \
     LEFT OUTER JOIN \"states\" ON (\"issues\".\"state_id\" = \"states\".\"id\")";

/// Splice JOINs into a Q-04 builder shell: the builders emit
/// `FROM "<table>" WHERE (` with the caller-owned joins outstanding (the
/// Q-04 contract, `queries.rs`), so the handler fills them here. The
/// filtered-count shell addresses the window column bare
/// (`AND "created_at" >= ...`, like the fixture shorthand); Django always
/// qualifies it, so qualify per table now that the joins introduce
/// same-named columns (unqualified is `ambiguous column` once joined).
fn with_joins(shell: String, table: &str, joins: &str) -> String {
    let needle = format!("FROM \"{table}\" WHERE (");
    shell
        .replacen(&needle, &format!("FROM \"{table}\"{joins} WHERE ("), 1)
        .replace(
            "AND \"created_at\" >= ",
            &format!("AND \"{table}\".\"created_at\" >= "),
        )
        .replace(
            "AND \"created_at\" <= ",
            &format!("AND \"{table}\".\"created_at\" <= "),
        )
}

/// `base_filters` (`date_utils.py:159-166`) over the issue joins:
/// `$1` = workspace slug, `$2` = user id.
fn issue_scope(ctx: &FilterCtx) -> Result<String, Denial> {
    let narrowing = ctx.project_narrowing("\"issues\".\"project_id\"")?;
    Ok(format!(
        "\"workspaces\".\"slug\" = $1 \
         AND \"project_members\".\"member_id\" = $2 AND \"project_members\".\"is_active\" \
         AND \"project_members\".\"deleted_at\" IS NULL \
         AND \"projects\".\"deleted_at\" IS NULL \
         AND {ISSUE_MANAGER_SCOPE}{narrowing}"
    ))
}

/// `base_filters` without the `issue_objects` manager: `Issue.objects`
/// (the inherited `SoftDeleteModel.objects` — `deleted_at IS NULL` only,
/// `mixins.py:61-67`; `issue.py` defines just `issue_objects` on top) for
/// the overview intake path (`advance.py:118-122`, ported quirk): no
/// triage/archived/draft exclusions there.
fn plain_issue_scope(ctx: &FilterCtx) -> Result<String, Denial> {
    let narrowing = ctx.project_narrowing("\"issues\".\"project_id\"")?;
    Ok(format!(
        "\"workspaces\".\"slug\" = $1 \
         AND \"project_members\".\"member_id\" = $2 AND \"project_members\".\"is_active\" \
         AND \"project_members\".\"deleted_at\" IS NULL \
         AND \"projects\".\"deleted_at\" IS NULL \
         AND \"projects\".\"archived_at\" IS NULL \
         AND \"issues\".\"deleted_at\" IS NULL{narrowing}"
    ))
}

/// `project_filters` (`date_utils.py:169-175`) for the project count:
/// `$1` = workspace slug, `$2` = user id.
fn project_scope(ctx: &FilterCtx) -> Result<String, Denial> {
    let narrowing = ctx.project_narrowing("\"projects\".\"id\"")?;
    Ok(format!(
        "\"workspaces\".\"slug\" = $1 \
         AND \"project_members\".\"member_id\" = $2 AND \"project_members\".\"is_active\" \
         AND \"project_members\".\"deleted_at\" IS NULL \
         AND \"projects\".\"deleted_at\" IS NULL \
         AND \"projects\".\"archived_at\" IS NULL{narrowing}"
    ))
}

/// JOINs for the project-count read.
pub const PROJECT_JOINS: &str =
    " INNER JOIN \"workspaces\" ON (\"projects\".\"workspace_id\" = \"workspaces\".\"id\") \
     INNER JOIN \"project_members\" ON (\"projects\".\"id\" = \"project_members\".\"project_id\")";

/// `Cycle`/`Module.objects.filter(**base_filters)` (`advance.py:216-217`):
/// both extend `ProjectBaseModel` (direct `project_id` + `workspace_id`
/// columns), so the scope mirrors the issue one minus the manager. `table`
/// is `"cycles"` or `"modules"`.
fn cycle_like_scope(table: &str, ctx: &FilterCtx) -> Result<String, Denial> {
    let narrowing = ctx.project_narrowing(&format!("\"{table}\".\"project_id\""))?;
    Ok(format!(
        "\"workspaces\".\"slug\" = $1 \
         AND \"project_members\".\"member_id\" = $2 AND \"project_members\".\"is_active\" \
         AND \"project_members\".\"deleted_at\" IS NULL \
         AND \"projects\".\"deleted_at\" IS NULL \
         AND \"projects\".\"archived_at\" IS NULL \
         AND \"{table}\".\"deleted_at\" IS NULL{narrowing}"
    ))
}

/// JOINs for the cycle/module reads (`workspace__slug` resolves over the
/// models' own `workspace` FK, `project.py:304`).
fn cycle_like_joins(table: &str) -> String {
    format!(
        " INNER JOIN \"projects\" ON (\"{table}\".\"project_id\" = \"projects\".\"id\") \
         INNER JOIN \"workspaces\" ON (\"{table}\".\"workspace_id\" = \"workspaces\".\"id\") \
         INNER JOIN \"project_members\" ON (\"projects\".\"id\" = \"project_members\".\"project_id\")"
    )
}

/// `ProjectPage` / `IssueView.objects.filter(**base_filters)`
/// (`advance.py:236-237`): `ProjectPage` carries its own `workspace` FK
/// (`page.py:138`); `IssueView` extends `WorkspaceBaseModel` (nullable
/// `project` FK, `workspace.py:186-187` — NULL-project rows drop on the
/// inner project join, exactly like Django).
fn page_like_scope(table: &str, ctx: &FilterCtx) -> Result<String, Denial> {
    cycle_like_scope(table, ctx)
}

fn page_like_joins(table: &str) -> String {
    cycle_like_joins(table)
}

/// `AgentRun` scope (`advance.py:72-78`): `workspace__slug` over the run's
/// own `workspace` FK, `pod__project` membership active, pod project live
/// (`deleted_at`/`archived_at` null). Neither the plain `AgentRun` manager
/// nor the forward `pod` join applies a deleted filter — kept verbatim.
fn agent_scope(ctx: &FilterCtx) -> Result<String, Denial> {
    let narrowing = ctx.project_narrowing("\"projects\".\"id\"")?;
    Ok(format!(
        "\"workspaces\".\"slug\" = $1 \
         AND \"project_members\".\"member_id\" = $2 AND \"project_members\".\"is_active\" \
         AND \"project_members\".\"deleted_at\" IS NULL \
         AND \"projects\".\"deleted_at\" IS NULL \
         AND \"projects\".\"archived_at\" IS NULL{narrowing}"
    ))
}

/// JOINs for the agent-run aggregate.
pub const AGENT_JOINS: &str =
    " INNER JOIN \"workspaces\" ON (\"agent_run\".\"workspace_id\" = \"workspaces\".\"id\") \
     INNER JOIN \"pod\" ON (\"agent_run\".\"pod_id\" = \"pod\".\"id\") \
     INNER JOIN \"projects\" ON (\"pod\".\"project_id\" = \"projects\".\"id\") \
     INNER JOIN \"project_members\" ON (\"projects\".\"id\" = \"project_members\".\"project_id\")";

/// Bind `$1`/`$2` (slug, user) plus the optional range of one statement.
/// Window bounds bind as UTC timestamptz (naive datetimes read in
/// `TIME_ZONE "UTC"`); period bounds bind as dates. A macro because
/// `sqlx::query` and `sqlx::query_as` are distinct types with the same
/// `.bind` chain.
macro_rules! bind_range {
    ($query:expr, $slug:expr, $user_id:expr, $ctx:expr, $kind:expr) => {{
        let __query = $query.bind($slug).bind(*$user_id);
        match ($kind, $ctx) {
            (
                RangeKind::Window,
                FilterCtx {
                    window: Some((gte, lte)),
                    ..
                },
            ) => __query.bind(*gte).bind(*lte),
            (
                RangeKind::Period,
                FilterCtx {
                    period: Some((start, end)),
                    ..
                },
            ) => __query.bind(*start).bind(*end),
            _ => __query,
        }
    }};
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RangeKind {
    Window,
    Period,
    None,
}

async fn count_one(
    pool: &sqlx::PgPool,
    sql: &str,
    slug: &str,
    user_id: &uuid::Uuid,
    ctx: &FilterCtx,
    range_kind: RangeKind,
) -> Result<i64, Denial> {
    let row: (i64,) = bind_range!(sqlx::query_as(sql), slug, user_id, ctx, range_kind)
        .fetch_one(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(row.0)
}

fn count_obj(count: i64) -> Value {
    let mut map = Map::new();
    map.insert("count".to_owned(), Value::from(count));
    Value::Object(map)
}

// ---------------------------------------------------------------------------
// GET advance-analytics/
// ---------------------------------------------------------------------------

/// `AdvanceAnalyticsEndpoint.get` (`advance.py:137-154`).
async fn advance_get(
    State(state): State<AppState>,
    extension: Option<axum::Extension<SessionHandle>>,
    Path(slug): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Response, Denial> {
    let actor = actor(&state, extension).await?;
    let pool = pool_of(&state)?;
    let role = workspace_role(pool, &slug, &actor.id).await?;
    check_gate("GET", PATH_ADVANCE, &slug, role)?;

    // `initialize_workspace(slug, type="analytics")` (advance.py:32-43).
    let now = Utc::now();
    let ctx = FilterCtx::parse(&params, "analytics", now);
    let tab = params.get("tab").map(String::as_str).unwrap_or("overview");

    if tab == "overview" {
        let body = overview_data(pool, &slug, &actor.id, &ctx).await?;
        return Ok(json_response(StatusCode::OK, body));
    }
    if tab == "work-items" {
        let body = work_items_stats(pool, &slug, &actor.id, &ctx).await?;
        return Ok(json_response(StatusCode::OK, body));
    }
    Err(Denial::InvalidTab)
}

/// `get_overview_data` (`advance.py:98-124`): keys in
/// [`q::ADVANCE_OVERVIEW_KEYS`] order, each `{"count": n}` (ported quirk B2).
async fn overview_data(
    pool: &sqlx::PgPool,
    slug: &str,
    user_id: &uuid::Uuid,
    ctx: &FilterCtx,
) -> Result<String, Denial> {
    let window = ctx.window_ph();
    let wph = || {
        window
            .as_ref()
            .map(|(gte, lte)| (gte.as_str(), lte.as_str()))
    };

    // Members: `WorkspaceMember(active, non-bot)` — but with `?project_ids=`
    // the source SWITCHES to `ProjectMember(project_id__in, active, non-bot)`
    // with no workspace scoping (`advance.py:102-111`, ported quirk).
    let is_workspace_path = ctx.project_ids.is_empty();
    let (member_table, member_extra) = if is_workspace_path {
        (
            "workspace_members",
            " AND \"users\".\"is_bot\" = FALSE AND \"workspaces\".\"slug\" = $1".to_owned(),
        )
    } else {
        let ids = ctx.checked_id_list()?.join(",");
        (
            "project_members",
            format!(" AND \"users\".\"is_bot\" = FALSE AND \"project_members\".\"project_id\" IN ({ids})"),
        )
    };
    // Member statements bind fewer leading params (slug only, or none), so
    // their window placeholders start at `$2` / `$1`.
    let mwindow = ctx.window_ph_at(if is_workspace_path { 2 } else { 1 });
    let mph = || {
        mwindow
            .as_ref()
            .map(|(gte, lte)| (gte.as_str(), lte.as_str()))
    };
    let member_joins = match member_table {
        "workspace_members" => {
            " INNER JOIN \"workspaces\" ON (\"workspace_members\".\"workspace_id\" = \"workspaces\".\"id\") \
             INNER JOIN \"users\" ON (\"workspace_members\".\"member_id\" = \"users\".\"id\")"
                .to_owned()
        }
        _ => {
            " INNER JOIN \"users\" ON (\"project_members\".\"member_id\" = \"users\".\"id\")".to_owned()
        }
    };
    let member_base = format!(
        "\"{member_table}\".\"is_active\" AND \"{member_table}\".\"deleted_at\" IS NULL{member_extra}"
    );
    // The member reads bind `$1` = slug only on the workspace path; the
    // project path binds nothing positional before the window, so each
    // statement binds in its own order here (not via `count_one`).
    // Copy the window bounds out: the `async move` future below must own
    // them (`DateTime` is `Copy`; the `window` Option itself stays put for
    // `wph` and the agent scope below).
    let mwindow_vals = ctx.window;
    let run_member_count = |sql: String| async move {
        let mut query = sqlx::query_as::<_, (i64,)>(&sql);
        if is_workspace_path {
            query = query.bind(slug);
        }
        if let Some((gte, lte)) = mwindow_vals {
            query = query.bind(gte).bind(lte);
        }
        let row: (i64,) = query
            .fetch_one(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
        Ok::<i64, Denial>(row.0)
    };
    // Role predicates reference the member table directly: `role=20/15/5`
    // (`ROLE.ADMIN/MEMBER/GUEST`, `app/permissions/base.py:13-15`); `None`
    // is the unfiltered total.
    let role_sql = |role: Option<i16>| {
        let mut scope = member_base.clone();
        if let Some(role) = role {
            scope.push_str(&format!(" AND \"{member_table}\".\"role\" = {role}"));
        }
        with_joins(
            q::advance_filtered_count_sql(member_table, &scope, mph()),
            member_table,
            &member_joins,
        )
    };
    let total_users = run_member_count(role_sql(None)).await?;
    let total_admins = run_member_count(role_sql(Some(20))).await?;
    let total_members = run_member_count(role_sql(Some(15))).await?;
    let total_guests = run_member_count(role_sql(Some(5))).await?;

    // Projects (`project_filters` through `get_filtered_counts`, so the
    // current window applies exactly like every other overview count —
    // `advance.py:62-65` takes any queryset, including projects).
    let pscope = project_scope(ctx)?;
    let psql = with_joins(
        q::advance_filtered_count_sql("projects", &pscope, wph()),
        "projects",
        PROJECT_JOINS,
    );
    let total_projects = count_one(pool, &psql, slug, user_id, ctx, RangeKind::Window).await?;

    // Work items + cycles (`base_filters`, current window when set).
    let iscope = issue_scope(ctx)?;
    let wsql = with_joins(
        q::advance_filtered_count_sql("issues", &iscope, wph()),
        "issues",
        ISSUE_JOINS,
    );
    let total_work_items = count_one(pool, &wsql, slug, user_id, ctx, RangeKind::Window).await?;
    let cscope = cycle_like_scope("cycles", ctx)?;
    let csql = with_joins(
        q::advance_filtered_count_sql("cycles", &cscope, wph()),
        "cycles",
        &cycle_like_joins("cycles"),
    );
    let total_cycles = count_one(pool, &csql, slug, user_id, ctx, RangeKind::Window).await?;

    // Intake: `Issue.objects` (soft-delete manager only) +
    // `issue_intake__status__in` (`advance.py:118-122`, ported quirk). The
    // physical table is `intake_issues`, aliased to the relation name the
    // Q-04 builder shell addresses (`overview_intake_where`).
    let piscope = plain_issue_scope(ctx)?;
    let intake_where = q::overview_intake_where(&piscope);
    let intake_joins = format!(
        "{ISSUE_JOINS} INNER JOIN \"intake_issues\" AS \"issue_intake\" ON (\"issues\".\"id\" = \"issue_intake\".\"issue_id\")"
    );
    let tsql = with_joins(
        q::advance_filtered_count_sql("issues", &intake_where, wph()),
        "issues",
        &intake_joins,
    );
    let total_intake = count_one(pool, &tsql, slug, user_id, ctx, RangeKind::Window).await?;

    // Agent-run token sums (`advance.py:67-96`): the current window applies
    // to `created_at` when set (`advance.py:86-91`).
    let ascope = agent_scope(ctx)?;
    let ascope = match window.as_ref() {
        Some((gte_ph, lte_ph)) => format!(
            "{ascope} AND \"agent_run\".\"created_at\" >= {gte_ph} AND \"agent_run\".\"created_at\" <= {lte_ph}"
        ),
        None => ascope,
    };
    let asql = with_joins(
        q::advance_agent_run_usage_sql(&ascope),
        "agent_run",
        AGENT_JOINS,
    );
    // Django's `Sum` answers Python ints; Postgres sums `bigint` to
    // `numeric`, which sqlx will not decode into `i64` — cast the three
    // totals back to `bigint` (the `,0)` shape occurs only in this select
    // list, never in the scope).
    let asql = asql.replace(",0)", ",0)::BIGINT");
    let tokens: (Option<i64>, Option<i64>, Option<i64>) =
        bind_range!(sqlx::query_as(&asql), slug, user_id, ctx, RangeKind::Window)
            .fetch_one(pool)
            .await
            .map_err(|_| Denial::ServerError)?;

    let mut out = Map::new();
    out.insert("total_users".to_owned(), count_obj(total_users));
    out.insert("total_admins".to_owned(), count_obj(total_admins));
    out.insert("total_members".to_owned(), count_obj(total_members));
    out.insert("total_guests".to_owned(), count_obj(total_guests));
    out.insert("total_projects".to_owned(), count_obj(total_projects));
    out.insert("total_work_items".to_owned(), count_obj(total_work_items));
    out.insert("total_cycles".to_owned(), count_obj(total_cycles));
    out.insert("total_intake".to_owned(), count_obj(total_intake));
    out.insert(
        "agent_run_input_tokens".to_owned(),
        count_obj(tokens.0.unwrap_or(0)),
    );
    out.insert(
        "agent_run_output_tokens".to_owned(),
        count_obj(tokens.1.unwrap_or(0)),
    );
    out.insert(
        "agent_run_total_tokens".to_owned(),
        count_obj(tokens.2.unwrap_or(0)),
    );
    Ok(Value::Object(out).to_string())
}

/// `get_work_items_stats` (`advance.py:126-135`): the base scope plus one
/// `state__group` filter per key, in [`q::ADVANCE_WORK_ITEM_KEYS`] order.
/// `cancelled` is NOT present here (it appears in the stats variants).
async fn work_items_stats(
    pool: &sqlx::PgPool,
    slug: &str,
    user_id: &uuid::Uuid,
    ctx: &FilterCtx,
) -> Result<String, Denial> {
    let base = issue_scope(ctx)?;
    let window = ctx.window_ph();
    let wph = || {
        window
            .as_ref()
            .map(|(gte, lte)| (gte.as_str(), lte.as_str()))
    };
    let mut out = Map::new();
    for (key, group) in q::ADVANCE_WORK_ITEM_KEYS {
        let sql = with_joins(
            q::advance_work_item_stat_sql(&base, *group, wph()),
            "issues",
            ISSUE_JOINS,
        );
        let count = count_one(pool, &sql, slug, user_id, ctx, RangeKind::Window).await?;
        out.insert((*key).to_owned(), count_obj(count));
    }
    Ok(Value::Object(out).to_string())
}

// ---------------------------------------------------------------------------
// GET advance-analytics-stats/
// ---------------------------------------------------------------------------

/// `AdvanceAnalyticsStatsEndpoint.get` (`advance.py:191-204`).
async fn stats_get(
    State(state): State<AppState>,
    extension: Option<axum::Extension<SessionHandle>>,
    Path(slug): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Response, Denial> {
    let actor = actor(&state, extension).await?;
    let pool = pool_of(&state)?;
    let role = workspace_role(pool, &slug, &actor.id).await?;
    check_gate("GET", PATH_STATS, &slug, role)?;

    // `initialize_workspace(slug, type="chart")` (advance.py:32-43).
    let now = Utc::now();
    let ctx = FilterCtx::parse(&params, "chart", now);
    let chart_type = params
        .get("type")
        .map(String::as_str)
        .unwrap_or("work-items");
    if chart_type != "work-items" {
        return Err(Denial::InvalidType);
    }
    // NOTE: `get` calls `get_work_items_stats` (`advance.py:177-189`), NOT
    // `get_project_issues_stats` (`advance.py:156-175`) — the chart period
    // range some other path would apply is ignored here (dead method, kept
    // as-is).
    let body = project_issues_stats(pool, &slug, &actor.id, &ctx).await?;
    Ok(json_response(StatusCode::OK, body))
}

/// `values("project_id", "project__name")` + five `Count(id, filter=Q(...))`
/// `.order_by("project_id")` (`advance.py:177-189`).
async fn project_issues_stats(
    pool: &sqlx::PgPool,
    slug: &str,
    user_id: &uuid::Uuid,
    ctx: &FilterCtx,
) -> Result<String, Denial> {
    let base = issue_scope(ctx)?;
    // The builder selects `\"projects\".\"name\"`, which the base issue joins
    // already expose — no extra join needed.
    let shell = q::advance_project_issues_stats_sql(&base, None);
    let sql = with_joins(shell, "issues", ISSUE_JOINS);
    let rows: Vec<(uuid::Uuid, String, i64, i64, i64, i64, i64)> = sqlx::query_as(&sql)
        .bind(slug)
        .bind(user_id)
        .fetch_all(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let mut arr = Vec::with_capacity(rows.len());
    for (project_id, name, cancelled, completed, backlog, un_started, started) in rows {
        let mut row = Map::new();
        row.insert("project_id".to_owned(), Value::from(project_id.to_string()));
        row.insert("project__name".to_owned(), Value::from(name));
        row.insert("cancelled_work_items".to_owned(), Value::from(cancelled));
        row.insert("completed_work_items".to_owned(), Value::from(completed));
        row.insert("backlog_work_items".to_owned(), Value::from(backlog));
        row.insert("un_started_work_items".to_owned(), Value::from(un_started));
        row.insert("started_work_items".to_owned(), Value::from(started));
        arr.push(Value::Object(row));
    }
    Ok(Value::Array(arr).to_string())
}

// ---------------------------------------------------------------------------
// GET advance-analytics-charts/
// ---------------------------------------------------------------------------

/// `AdvanceAnalyticsChartEndpoint.get` (`advance.py:318-351`).
async fn charts_get(
    State(state): State<AppState>,
    extension: Option<axum::Extension<SessionHandle>>,
    Path(slug): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Response, Denial> {
    let actor = actor(&state, extension).await?;
    let pool = pool_of(&state)?;
    let role = workspace_role(pool, &slug, &actor.id).await?;
    check_gate("GET", PATH_CHARTS, &slug, role)?;

    // `initialize_workspace(slug, type="chart")` (advance.py:32-43).
    let now = Utc::now();
    let ctx = FilterCtx::parse(&params, "chart", now);
    let chart_type = params.get("type").map(String::as_str).unwrap_or("projects");

    if chart_type == "projects" {
        let body = project_chart(pool, &slug, &actor.id, &ctx).await?;
        return Ok(json_response(StatusCode::OK, body));
    }
    if chart_type == "custom-work-items" {
        // `x_axis` defaults to `"PRIORITY"` (`advance.py:323`).
        let x_axis = params
            .get("x_axis")
            .map(String::as_str)
            .unwrap_or(q::DEFAULT_CHART_X_AXIS);
        let group_by = params.get("group_by").map(String::as_str);
        let body =
            build_analytics_chart(pool, &slug, &actor.id, &ctx, x_axis, group_by, "").await?;
        return Ok(json_response(StatusCode::OK, body));
    }
    if chart_type == "work-items" {
        let body = work_item_completion_chart(pool, &slug, &actor.id, &ctx, now).await?;
        return Ok(json_response(StatusCode::OK, body));
    }
    Err(Denial::InvalidType)
}

/// `project_chart` (`advance.py:206-248`): seven independent counts under
/// the SAME `date_filter` (`created_at::date` range when `chart_period_range`
/// is set), rendered `[{"key","name": Title(key),"count": v or 0}]` in
/// [`q::ADVANCE_PROJECT_CHART_KEYS`] order.
async fn project_chart(
    pool: &sqlx::PgPool,
    slug: &str,
    user_id: &uuid::Uuid,
    ctx: &FilterCtx,
) -> Result<String, Denial> {
    let period = ctx.period_ph();
    // `created_at__date` bounds apply per-table (`date_filter`,
    // `advance.py:211-215`).
    let date_pred = |table: &str| {
        match period.as_ref() {
        Some((gte_ph, lte_ph)) => format!(
            " AND \"{table}\".\"created_at\"::date >= {gte_ph} AND \"{table}\".\"created_at\"::date <= {lte_ph}"
        ),
        None => String::new(),
    }
    };

    // work_items: `issue_objects` + base, with dates.
    let base = issue_scope(ctx)?;
    let wsql = with_joins(
        q::advance_project_chart_count_sql("issues", &format!("{base}{}", date_pred("issues"))),
        "issues",
        ISSUE_JOINS,
    );
    let work_items = count_one(pool, &wsql, slug, user_id, ctx, RangeKind::Period).await?;

    // cycles / modules: base + dates over their own tables.
    let cscope = format!(
        "{}{}",
        cycle_like_scope("cycles", ctx)?,
        date_pred("cycles")
    );
    let csql = with_joins(
        q::advance_project_chart_count_sql("cycles", &cscope),
        "cycles",
        &cycle_like_joins("cycles"),
    );
    let cycles = count_one(pool, &csql, slug, user_id, ctx, RangeKind::Period).await?;
    let mscope = format!(
        "{}{}",
        cycle_like_scope("modules", ctx)?,
        date_pred("modules")
    );
    let msql = with_joins(
        q::advance_project_chart_count_sql("modules", &mscope),
        "modules",
        &cycle_like_joins("modules"),
    );
    let modules = count_one(pool, &msql, slug, user_id, ctx, RangeKind::Period).await?;

    // intake: `Issue.objects` (soft-delete manager) +
    // `issue_intake__isnull=False` (`advance.py:222-224`) — an INNER JOIN,
    // deliberately different from the overview intake (ported quirk).
    let pbase = plain_issue_scope(ctx)?;
    let iscope = format!(
        "{pbase} AND \"issue_intake\".\"issue_id\" IS NOT NULL{}",
        date_pred("issues")
    );
    let isql = with_joins(
        q::advance_project_chart_count_sql("issues", &iscope),
        "issues",
        &format!(
            "{ISSUE_JOINS} INNER JOIN \"intake_issues\" AS \"issue_intake\" ON (\"issues\".\"id\" = \"issue_intake\".\"issue_id\")"
        ),
    );
    let intake = count_one(pool, &isql, slug, user_id, ctx, RangeKind::Period).await?;

    // members: active + slug + dates, NO bot exclusion (`advance.py:231-233`,
    // ported quirk). `WorkspaceMember.objects` still soft-filters. Binds
    // `$1` = slug plus the optional `$2`/`$3` dates (no user slot: the
    // member scope carries no membership predicate), so placeholders start
    // at `$2` here.
    let mem_scope = match ctx.period_ph_at(2).as_ref() {
        Some((gte_ph, lte_ph)) => format!(
            "\"workspaces\".\"slug\" = $1 AND \"workspace_members\".\"is_active\" \
             AND \"workspace_members\".\"deleted_at\" IS NULL \
             AND \"workspace_members\".\"created_at\"::date >= {gte_ph} \
             AND \"workspace_members\".\"created_at\"::date <= {lte_ph}"
        ),
        None => "\"workspaces\".\"slug\" = $1 AND \"workspace_members\".\"is_active\" \
             AND \"workspace_members\".\"deleted_at\" IS NULL"
            .to_owned(),
    };
    let mem_sql = with_joins(
        q::advance_project_chart_count_sql("workspace_members", &mem_scope),
        "workspace_members",
        " INNER JOIN \"workspaces\" ON (\"workspace_members\".\"workspace_id\" = \"workspaces\".\"id\")",
    );
    let mut mem_query = sqlx::query_as::<_, (i64,)>(&mem_sql).bind(slug);
    if let Some((start, end)) = ctx.period.as_ref() {
        mem_query = mem_query.bind(*start).bind(*end);
    }
    let members: (i64,) = mem_query
        .fetch_one(pool)
        .await
        .map_err(|_| Denial::ServerError)?;

    // pages / views: base + dates over their own tables.
    let pgscope = format!(
        "{}{}",
        page_like_scope("project_pages", ctx)?,
        date_pred("project_pages")
    );
    let pgsql = with_joins(
        q::advance_project_chart_count_sql("project_pages", &pgscope),
        "project_pages",
        &page_like_joins("project_pages"),
    );
    let pages = count_one(pool, &pgsql, slug, user_id, ctx, RangeKind::Period).await?;
    let vwscope = format!(
        "{}{}",
        page_like_scope("issue_views", ctx)?,
        date_pred("issue_views")
    );
    let vwssql = with_joins(
        q::advance_project_chart_count_sql("issue_views", &vwscope),
        "issue_views",
        &page_like_joins("issue_views"),
    );
    let views = count_one(pool, &vwssql, slug, user_id, ctx, RangeKind::Period).await?;

    let counts = [
        ("work_items", work_items),
        ("cycles", cycles),
        ("modules", modules),
        ("intake", intake),
        ("members", members.0),
        ("pages", pages),
        ("views", views),
    ];
    debug_assert_eq!(
        counts.map(|(key, _)| key),
        q::ADVANCE_PROJECT_CHART_KEYS,
        "chart key order follows the builder const"
    );
    let mut arr = Vec::with_capacity(counts.len());
    for (key, count) in counts {
        let mut row = Map::new();
        row.insert("key".to_owned(), Value::from(key));
        row.insert("name".to_owned(), Value::from(q::chart_key_name(key)));
        // `value or 0`: COUNT never returns NULL, but keep the shape.
        row.insert("count".to_owned(), Value::from(count));
        arr.push(Value::Object(row));
    }
    Ok(Value::Array(arr).to_string())
}

/// `work_item_completion_chart` (`advance.py:250-316`): monthly
/// `TruncMonth` buckets of created (all) vs completed
/// (`state__group="completed"`), zero-filled from the workspace
/// `created_at` month-start through the current month-start. Each row is
/// `{key, name, count=created, completed_issues, created_issues}` with the
/// `{"completed_issues": ..., "created_issues": ...}` schema.
async fn work_item_completion_chart(
    pool: &sqlx::PgPool,
    slug: &str,
    user_id: &uuid::Uuid,
    ctx: &FilterCtx,
    now: DateTime<Utc>,
) -> Result<String, Denial> {
    // `Workspace.objects.get(slug=...)` — 404 when missing (unreachable past
    // the gate; the gate denies unknown slugs first).
    let ws: Option<(DateTime<Utc>,)> = sqlx::query_as(
        "SELECT \"created_at\" FROM \"workspaces\" WHERE \"slug\" = $1 AND \"deleted_at\" IS NULL",
    )
    .bind(slug)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let (ws_created,) = ws.ok_or(Denial::NotFound)?;
    let start = ws_created.date_naive().with_day(1).expect("month start");

    let base = issue_scope(ctx)?;
    let period = ctx.period_ph();
    let pph = || {
        period
            .as_ref()
            .map(|(gte, lte)| (gte.as_str(), lte.as_str()))
    };
    let shell = q::advance_completion_monthly_sql(&base, pph());
    let sql = with_joins(shell, "issues", ISSUE_JOINS);
    let rows: Vec<(DateTime<Utc>, i64, i64)> =
        bind_range!(sqlx::query_as(&sql), slug, user_id, ctx, RangeKind::Period)
            .fetch_all(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    // `stat["month"].strftime("%Y-%m-%d")` keys the stats dict.
    let mut stats = HashMap::new();
    for (month, created, completed) in rows {
        stats.insert(month.format("%Y-%m-%d").to_string(), (created, completed));
    }
    // The zero-fill walks `current_month` from `start_date` through the
    // current month-start (`advance.py:287-309`) — shared with the project
    // chart (PIDASHCONV-424); see [`completion_monthly_body`].
    let first = match ctx.period {
        Some((period_start, _)) => period_start,
        None => start,
    };
    completion_monthly_body(&stats, first, now)
}

// ---------------------------------------------------------------------------
// build_analytics_chart (utils/build_chart.py:151-194)
// ---------------------------------------------------------------------------

/// A decoded chart key preserving its JSON type: DRF renders ints as
/// numbers and UUIDs/dates as strings, while falsy keys (`None`, `""`, `0`)
/// become `"None"` (simple path) or `"none"` (grouped path).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum KeyVal {
    Null,
    Str(String),
    Int(i64),
    Date(NaiveDate),
    Uuid(uuid::Uuid),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeyKind {
    Uuid,
    Text,
    Int,
    Date,
}

/// One `get_x_axis_field` row (`build_chart.py:47-76`): the key/name
/// expressions, the extra joins, and the additional deleted-guard filter.
/// Joins reuse the base issue joins; the `states` join is shared with the
/// manager scope (exactly like Django reuses its single join).
struct Axis {
    joins: &'static str,
    key_expr: &'static str,
    name_expr: &'static str,
    filter: &'static str,
    key_kind: KeyKind,
    /// Decode kind of the display-name column: `Text` everywhere except the
    /// date axes, whose name is the same date (`name_field ==
    /// created_at__date`, ...). Decoding a non-NULL date as text 500s in
    /// sqlx (NULLs slip through as `None`), so the kind must match.
    name_kind: KeyKind,
}

/// `get_x_axis_field()` (`build_chart.py:47-76`) + `x_axis_mapper`
/// (`build_chart.py:20-34`). Lookup is case-sensitive (a plain dict `in`
/// check, `build_chart.py:155`).
fn axis_for(name: &str) -> Option<Axis> {
    match name {
        // `("state__id", "state__name", None)` — shares the base states join.
        "STATES" => Some(Axis {
            joins: "",
            key_expr: "\"issues\".\"state_id\"",
            name_expr: "\"states\".\"name\"",
            filter: "",
            key_kind: KeyKind::Uuid,
            name_kind: KeyKind::Text,
        }),
        "STATE_GROUPS" => Some(Axis {
            joins: "",
            key_expr: "\"states\".\"group\"",
            name_expr: "\"states\".\"group\"",
            filter: "",
            key_kind: KeyKind::Text,
            name_kind: KeyKind::Text,
        }),
        // `{"label_issue__deleted_at__isnull": True}`.
        "LABELS" => Some(Axis {
            joins: " LEFT OUTER JOIN \"issue_labels\" ON (\"issues\".\"id\" = \"issue_labels\".\"issue_id\") \
                     LEFT OUTER JOIN \"labels\" ON (\"issue_labels\".\"label_id\" = \"labels\".\"id\")",
            key_expr: "\"issue_labels\".\"label_id\"",
            name_expr: "\"labels\".\"name\"",
            filter: " AND \"issue_labels\".\"deleted_at\" IS NULL",
            key_kind: KeyKind::Uuid,
            name_kind: KeyKind::Text,
        }),
        // `{"issue_assignee__deleted_at__isnull": True}`.
        "ASSIGNEES" => Some(Axis {
            joins: " LEFT OUTER JOIN \"issue_assignees\" ON (\"issues\".\"id\" = \"issue_assignees\".\"issue_id\") \
                     LEFT OUTER JOIN \"users\" AS \"ax_users\" ON (\"issue_assignees\".\"assignee_id\" = \"ax_users\".\"id\")",
            key_expr: "\"issue_assignees\".\"assignee_id\"",
            name_expr: "\"ax_users\".\"display_name\"",
            filter: " AND \"issue_assignees\".\"deleted_at\" IS NULL",
            key_kind: KeyKind::Uuid,
            name_kind: KeyKind::Text,
        }),
        // Forward nullable FK (`issues.estimate_point_id`, `issue.py:130`):
        // key is the integer `key`, name the `value` text (`estimate.py`).
        "ESTIMATE_POINTS" => Some(Axis {
            joins: " LEFT OUTER JOIN \"estimate_points\" ON (\"issues\".\"estimate_point_id\" = \"estimate_points\".\"id\")",
            key_expr: "\"estimate_points\".\"key\"",
            name_expr: "\"estimate_points\".\"value\"",
            filter: "",
            key_kind: KeyKind::Int,
            name_kind: KeyKind::Text,
        }),
        // `{"issue_cycle__deleted_at__isnull": True}`.
        "CYCLES" => Some(Axis {
            joins: " LEFT OUTER JOIN \"cycle_issues\" ON (\"issues\".\"id\" = \"cycle_issues\".\"issue_id\") \
                     LEFT OUTER JOIN \"cycles\" AS \"ax_cycles\" ON (\"cycle_issues\".\"cycle_id\" = \"ax_cycles\".\"id\")",
            key_expr: "\"cycle_issues\".\"cycle_id\"",
            name_expr: "\"ax_cycles\".\"name\"",
            filter: " AND \"cycle_issues\".\"deleted_at\" IS NULL",
            key_kind: KeyKind::Uuid,
            name_kind: KeyKind::Text,
        }),
        // `{"issue_module__deleted_at__isnull": True}`.
        "MODULES" => Some(Axis {
            joins: " LEFT OUTER JOIN \"module_issues\" ON (\"issues\".\"id\" = \"module_issues\".\"issue_id\") \
                     LEFT OUTER JOIN \"modules\" AS \"ax_modules\" ON (\"module_issues\".\"module_id\" = \"ax_modules\".\"id\")",
            key_expr: "\"module_issues\".\"module_id\"",
            name_expr: "\"ax_modules\".\"name\"",
            filter: " AND \"module_issues\".\"deleted_at\" IS NULL",
            key_kind: KeyKind::Uuid,
            name_kind: KeyKind::Text,
        }),
        "PRIORITY" => Some(Axis {
            joins: "",
            key_expr: "\"issues\".\"priority\"",
            name_expr: "\"issues\".\"priority\"",
            filter: "",
            key_kind: KeyKind::Text,
            name_kind: KeyKind::Text,
        }),
        "START_DATE" => Some(Axis {
            joins: "",
            key_expr: "\"issues\".\"start_date\"",
            name_expr: "\"issues\".\"start_date\"",
            filter: "",
            key_kind: KeyKind::Date,
            name_kind: KeyKind::Date,
        }),
        "TARGET_DATE" => Some(Axis {
            joins: "",
            key_expr: "\"issues\".\"target_date\"",
            name_expr: "\"issues\".\"target_date\"",
            filter: "",
            key_kind: KeyKind::Date,
            name_kind: KeyKind::Date,
        }),
        "CREATED_AT" => Some(Axis {
            joins: "",
            key_expr: "\"issues\".\"created_at\"::date",
            name_expr: "\"issues\".\"created_at\"::date",
            filter: "",
            key_kind: KeyKind::Date,
            name_kind: KeyKind::Date,
        }),
        "COMPLETED_AT" => Some(Axis {
            joins: "",
            key_expr: "\"issues\".\"completed_at\"::date",
            name_expr: "\"issues\".\"completed_at\"::date",
            filter: "",
            key_kind: KeyKind::Date,
            name_kind: KeyKind::Date,
        }),
        // The users alias must not collide with the ASSIGNEES axis (`ax_users`):
        // Django reuses one join per association, so `x_axis=ASSIGNEES` with
        // `group_by=CREATED_BY` (or the reverse) serves 200 there — a shared
        // alias 500s here on `specified more than once`.
        "CREATED_BY" => Some(Axis {
            joins: " LEFT OUTER JOIN \"users\" AS \"ax_created_users\" ON (\"issues\".\"created_by_id\" = \"ax_created_users\".\"id\")",
            key_expr: "\"issues\".\"created_by_id\"",
            name_expr: "\"ax_created_users\".\"display_name\"",
            filter: "",
            key_kind: KeyKind::Uuid,
            name_kind: KeyKind::Text,
        }),
        _ => None,
    }
}

impl KeyVal {
    /// Python truthiness for chart keys: `None`, `""` and `0` are falsy.
    fn is_falsy(&self) -> bool {
        match self {
            KeyVal::Null => true,
            KeyVal::Str(text) => text.is_empty(),
            KeyVal::Int(n) => *n == 0,
            KeyVal::Date(_) | KeyVal::Uuid(_) => false,
        }
    }

    /// Python `str()` of the raw value (grouped `group_key`, schema keys).
    fn raw_string(&self) -> String {
        match self {
            KeyVal::Null => "None".to_owned(),
            KeyVal::Str(text) => text.clone(),
            KeyVal::Int(n) => n.to_string(),
            KeyVal::Date(day) => day.format("%Y-%m-%d").to_string(),
            KeyVal::Uuid(id) => id.to_string(),
        }
    }

    /// The simple-path `item["key"] if item["key"] else "None"` rendering,
    /// preserving JSON types (ints stay numbers).
    fn simple_json(&self) -> Value {
        if self.is_falsy() {
            return Value::from("None");
        }
        match self {
            KeyVal::Str(text) => Value::from(text.clone()),
            KeyVal::Int(n) => Value::from(*n),
            KeyVal::Date(day) => Value::from(day.format("%Y-%m-%d").to_string()),
            KeyVal::Uuid(id) => Value::from(id.to_string()),
            KeyVal::Null => Value::from("None"),
        }
    }

    /// The grouped-path row `"key"`: `key if key else "none"`
    /// (`process_grouped_data`, lowercase) — and like the simple path the
    /// truthy key keeps its JSON type, so grouped estimate keys render as
    /// numbers exactly as DRF renders the raw int.
    fn grouped_key_json(&self) -> Value {
        if self.is_falsy() {
            return Value::from("none");
        }
        match self {
            KeyVal::Str(text) => Value::from(text.clone()),
            KeyVal::Int(n) => Value::from(*n),
            KeyVal::Date(day) => Value::from(day.format("%Y-%m-%d").to_string()),
            KeyVal::Uuid(id) => Value::from(id.to_string()),
            KeyVal::Null => Value::from("none"),
        }
    }

    /// The grouped-path row `"name"`: `display_name if display_name else
    /// "None"` (`process_grouped_data` — the `.get()` default never fires,
    /// the annotation is always present, so a missing name is `"None"`, not
    /// the key).
    fn grouped_name_json(&self) -> Value {
        self.simple_json()
    }
}

/// The grouped-path schema entry: `group_name or "None"`
/// (`process_grouped_data` — unlike the row key, the group key itself never
/// substitutes, so an empty estimate value schemas as `"None"`).
fn grouped_schema_json(group_name: &KeyVal) -> Value {
    if group_name.is_falsy() {
        Value::from("None")
    } else {
        group_name.simple_json()
    }
}

async fn decode_key(
    row: &sqlx::postgres::PgRow,
    index: usize,
    kind: KeyKind,
) -> Result<KeyVal, Denial> {
    match kind {
        KeyKind::Uuid => {
            let raw: Option<uuid::Uuid> = row.try_get(index).map_err(|_| Denial::ServerError)?;
            Ok(raw.map(KeyVal::Uuid).unwrap_or(KeyVal::Null))
        }
        KeyKind::Text => {
            let raw: Option<String> = row.try_get(index).map_err(|_| Denial::ServerError)?;
            Ok(raw.map(KeyVal::Str).unwrap_or(KeyVal::Null))
        }
        KeyKind::Int => {
            let raw: Option<i32> = row.try_get(index).map_err(|_| Denial::ServerError)?;
            Ok(raw
                .map(|n| KeyVal::Int(i64::from(n)))
                .unwrap_or(KeyVal::Null))
        }
        KeyKind::Date => {
            let raw: Option<NaiveDate> = row.try_get(index).map_err(|_| Denial::ServerError)?;
            Ok(raw.map(KeyVal::Date).unwrap_or(KeyVal::Null))
        }
    }
}

/// Join assembly for the chart selects: the base issue joins plus the axis
/// joins plus the group joins — except Django reuses one join per
/// association (`build_simple_chart_response` / `build_grouped_chart_response`
/// annotate over a single queryset), so when the group axis repeats the x
/// axis joins they merge instead of repeating: a repeated table or alias is
/// a Postgres `specified more than once` error (500) where Django serves 200
/// (e.g. `x_axis=LABELS&group_by=LABELS`).
fn chart_joins(axis: &Axis, group: Option<&Axis>) -> String {
    match group {
        Some(group) if group.joins != axis.joins => {
            format!("{ISSUE_JOINS}{}{}", axis.joins, group.joins)
        }
        _ => format!("{ISSUE_JOINS}{}", axis.joins),
    }
}

/// `build_analytics_chart(queryset, x_axis, group_by)`
/// (`build_chart.py:151-194`).
///
/// The scoped base queryset (same `base_filters` + `chart_period_range` as
/// the sibling chart paths, fetch plans aside) grouped by the x axis —
/// simply, or per group when `group_by` is set — with
/// `Count("id", distinct=True)`. Shared with the project advance chart
/// path (PIDASHCONV-424) from this module — no fork: the project path
/// passes its `.filter(project_id=...)` (+ optional cycle/module id-list)
/// predicate as `extra_scope` (empty on the workspace path).
pub(crate) async fn build_analytics_chart(
    pool: &sqlx::PgPool,
    slug: &str,
    user_id: &uuid::Uuid,
    ctx: &FilterCtx,
    x_axis: &str,
    group_by: Option<&str>,
    extra_scope: &str,
) -> Result<String, Denial> {
    // `if x_axis not in x_axis_mapper: raise ValidationError(...)`.
    let axis = axis_for(x_axis).ok_or_else(|| {
        Denial::Raw(
            StatusCode::BAD_REQUEST,
            invalid_axis_body(&format!("Invalid x_axis field: {x_axis}")),
        )
    })?;
    // `if group_by and group_by not in x_axis_mapper: raise ...` — an empty
    // `?group_by=` is falsy in Python, so it takes the simple path, not the
    // 400 (`advance.py:323` passes the raw `GET.get`, `build_chart.py:158`).
    let group = match group_by {
        Some(name) if !name.is_empty() => Some(axis_for(name).ok_or_else(|| {
            Denial::Raw(
                StatusCode::BAD_REQUEST,
                invalid_axis_body(&format!("Invalid group_by field: {name}")),
            )
        })?),
        _ => None,
    };

    let base = issue_scope(ctx)?;
    let period = ctx.period_ph();
    let date_pred = match period.as_ref() {
        Some((gte_ph, lte_ph)) => format!(
            " AND \"issues\".\"created_at\"::date >= {gte_ph} AND \"issues\".\"created_at\"::date <= {lte_ph}"
        ),
        None => String::new(),
    };
    // Queryset filters apply in this order: base scope, the axis additional
    // filters, then the group-axis additional filters (`build_chart.py`).
    // The project path's predicate (a conjunction like the rest) appends
    // last; AND order is immaterial.
    let group_filter = group.as_ref().map(|axis| axis.filter).unwrap_or("");
    let scope = format!(
        "{base}{}{group_filter}{date_pred}{extra_scope}",
        axis.filter
    );
    let joins = chart_joins(&axis, group.as_ref());

    if let Some(group) = group {
        grouped_chart_response(
            pool,
            slug,
            user_id,
            ctx,
            period.is_some(),
            &scope,
            &joins,
            &axis,
            &group,
        )
        .await
    } else {
        simple_chart_response(
            pool,
            slug,
            user_id,
            ctx,
            period.is_some(),
            &scope,
            &joins,
            &axis,
        )
        .await
    }
}

/// `build_simple_chart_response` (`build_chart.py:133-149`):
/// `annotate(key, display_name).values(...).annotate(count).order_by("key")`.
#[allow(clippy::too_many_arguments)]
async fn simple_chart_response(
    pool: &sqlx::PgPool,
    slug: &str,
    user_id: &uuid::Uuid,
    ctx: &FilterCtx,
    has_period: bool,
    scope: &str,
    joins: &str,
    axis: &Axis,
) -> Result<String, Denial> {
    // Positional GROUP BY: the select aliases (`key`, `display_name`) can also
    // name input columns of the joined tables, and two axes can expose the
    // same bare name (users `display_name` twice, `labels.name` beside
    // `cycles.name`) — a bare name then misresolves to the inputs (GROUP BY
    // prefers input columns) and errors `ambiguous` where Django, grouping
    // the same four select expressions, serves 200.
    let sql = format!(
        "SELECT {} AS \"key\", {} AS \"display_name\", COUNT(DISTINCT \"issues\".\"id\") AS \"count\" \
         FROM \"issues\"{joins} WHERE ({scope}) GROUP BY 1, 2 ORDER BY \"key\"",
        axis.key_expr, axis.name_expr
    );
    let kind = if has_period {
        RangeKind::Period
    } else {
        RangeKind::None
    };
    let rows = bind_range!(sqlx::query(&sql), slug, user_id, ctx, kind)
        .fetch_all(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let mut arr = Vec::with_capacity(rows.len());
    for row in rows {
        let key = decode_key(&row, 0, axis.key_kind).await?;
        let name = decode_key(&row, 1, axis.name_kind).await?;
        let count: i64 = row.try_get(2).map_err(|_| Denial::ServerError)?;
        let mut item = Map::new();
        item.insert("key".to_owned(), key.simple_json());
        item.insert("name".to_owned(), name.simple_json());
        item.insert("count".to_owned(), Value::from(count));
        arr.push(Value::Object(item));
    }
    let mut out = Map::new();
    out.insert("data".to_owned(), Value::Array(arr));
    out.insert("schema".to_owned(), Value::Object(Map::new()));
    Ok(Value::Object(out).to_string())
}

/// `build_grouped_chart_response` + `process_grouped_data`
/// (`build_chart.py:84-131`): grouped counts ordered by `-count`, folded so
/// each key carries its group buckets plus the total.
#[allow(clippy::too_many_arguments)]
async fn grouped_chart_response(
    pool: &sqlx::PgPool,
    slug: &str,
    user_id: &uuid::Uuid,
    ctx: &FilterCtx,
    has_period: bool,
    scope: &str,
    joins: &str,
    axis: &Axis,
    group: &Axis,
) -> Result<String, Denial> {
    // Positional GROUP BY (see the simple path): the same four select
    // expressions Django groups, without the bare-name input misresolution.
    let sql = format!(
        "SELECT {} AS \"key\", {} AS \"group_key\", {} AS \"group_name\", {} AS \"display_name\", \
         COUNT(DISTINCT \"issues\".\"id\") AS \"count\" \
         FROM \"issues\"{joins} WHERE ({scope}) \
         GROUP BY 1, 2, 3, 4 ORDER BY \"count\" DESC",
        axis.key_expr, group.key_expr, group.name_expr, axis.name_expr
    );
    let kind = if has_period {
        RangeKind::Period
    } else {
        RangeKind::None
    };
    let rows = bind_range!(sqlx::query(&sql), slug, user_id, ctx, kind)
        .fetch_all(pool)
        .await
        .map_err(|_| Denial::ServerError)?;

    // `process_grouped_data`: insertion-ordered (first-seen row order is
    // the `-count` order), `key if key else "none"` (lowercase, raw type),
    // `display_name or "None"`, `str(group_key) or "none"`,
    // `schema[g] = group_name or "None"`.
    struct Bucket {
        key: Value,
        name: Value,
        groups: Vec<(String, i64)>,
        total: i64,
    }
    let mut order: Vec<KeyVal> = Vec::new();
    let mut buckets: HashMap<KeyVal, Bucket> = HashMap::new();
    let mut schema: Vec<(String, Value)> = Vec::new();
    for row in rows {
        let key = decode_key(&row, 0, axis.key_kind).await?;
        let group_key = decode_key(&row, 1, group.key_kind).await?;
        let group_name = decode_key(&row, 2, group.name_kind).await?;
        let display = decode_key(&row, 3, axis.name_kind).await?;
        let count: i64 = row.try_get(4).map_err(|_| Denial::ServerError)?;

        let bucket = buckets.entry(key.clone()).or_insert_with(|| {
            order.push(key.clone());
            Bucket {
                key: key.grouped_key_json(),
                name: display.grouped_name_json(),
                groups: Vec::new(),
                total: 0,
            }
        });
        let g = if group_key.is_falsy() {
            "none".to_owned()
        } else {
            group_key.raw_string()
        };
        let gname = grouped_schema_json(&group_name);
        if !schema.iter().any(|(name, _)| *name == g) {
            schema.push((g.clone(), gname));
        }
        match bucket.groups.iter_mut().find(|(name, _)| *name == g) {
            Some(slot) => slot.1 += count,
            None => bucket.groups.push((g, count)),
        }
        bucket.total += count;
    }
    let mut arr = Vec::with_capacity(order.len());
    for key in &order {
        let bucket = buckets.remove(key).expect("bucketed key");
        let mut item = Map::new();
        item.insert("key".to_owned(), bucket.key);
        item.insert("name".to_owned(), bucket.name);
        item.insert("count".to_owned(), Value::from(bucket.total));
        for (name, count) in bucket.groups {
            item.insert(name, Value::from(count));
        }
        arr.push(Value::Object(item));
    }
    let mut schema_map = Map::new();
    for (name, value) in schema {
        schema_map.insert(name, value);
    }
    let mut out = Map::new();
    out.insert("data".to_owned(), Value::Array(arr));
    out.insert("schema".to_owned(), Value::Object(schema_map));
    Ok(Value::Object(out).to_string())
}

// ---------------------------------------------------------------------------
// Project advance shell (PIDASHCONV-424, handlers-D)
// ---------------------------------------------------------------------------

/// JOINs for the stats assignee annotations (`assignees__display_name` /
/// `assignees__id` / avatar `Case`, `project_analytics.py:134-153`): the M2M
/// through rows plus their users, both LEFT (a missing assignee is the NULL
/// bucket, not a dropped row) — the handlers-B `assignee_joins` precedent.
/// No deleted guard on the through join there either (probe-verified below).
pub const ASSIGNEE_JOINS: &str =
    " LEFT OUTER JOIN \"issue_assignees\" ON (\"issues\".\"id\" = \"issue_assignees\".\"issue_id\") \
     LEFT OUTER JOIN \"users\" ON (\"issue_assignees\".\"assignee_id\" = \"users\".\"id\")";

/// Inline one validated UUID as a quoted literal (the
/// [`FilterCtx::checked_id_list`] precedent: parse-validated, so quoting is
/// exact and cannot break out of the string).
fn uuid_literal(id: &uuid::Uuid) -> String {
    format!("'{}'", id.as_hyphenated())
}

/// The `?cycle_id=` / `?module_id=` branch (`project_analytics.py:63-74` and
/// siblings): cycle wins when both are present (`if`/`elif`); `Plain` keeps
/// the route's `project_id` scoping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ThroughBranch {
    Plain,
    Cycle(uuid::Uuid),
    Module(uuid::Uuid),
}

/// Read the branch off the query params. An empty value is still "present"
/// (`request.GET.get` returns `""`, which `is not None`), so it 400s on the
/// UUID coercion exactly like any malformed id (the `handle_exception`
/// `ValidationError` branch).
fn through_branch(params: &HashMap<String, String>) -> Result<ThroughBranch, Denial> {
    if let Some(raw) = params.get("cycle_id") {
        return raw
            .parse::<uuid::Uuid>()
            .map(ThroughBranch::Cycle)
            .map_err(|_| Denial::InvalidDetail);
    }
    if let Some(raw) = params.get("module_id") {
        return raw
            .parse::<uuid::Uuid>()
            .map(ThroughBranch::Module)
            .map_err(|_| Denial::InvalidDetail);
    }
    Ok(ThroughBranch::Plain)
}

/// The through-table id list for one branch
/// (`CycleIssue.objects.filter(**base_filters, cycle_id=...)` /
/// `ModuleIssue...`, `project_analytics.py:64-72`): the Q-05
/// [`q::project_through_ids_sql`] shell with the through-table joins spliced
/// in. The through models extend `ProjectBaseModel` (own `project` +
/// `workspace` columns), so their `base_filters` scope is exactly
/// [`cycle_like_scope`]. `Plain` has no list (empty string, unused).
fn through_ids_subquery(branch: ThroughBranch, ctx: &FilterCtx) -> Result<String, Denial> {
    let (table, fk_col, id) = match branch {
        ThroughBranch::Cycle(id) => ("cycle_issues", "cycle_id", id),
        ThroughBranch::Module(id) => ("module_issues", "module_id", id),
        ThroughBranch::Plain => return Ok(String::new()),
    };
    let base = cycle_like_scope(table, ctx)?;
    let shell = q::project_through_ids_sql(table, fk_col, &base, &uuid_literal(&id));
    Ok(with_joins(shell, table, &cycle_like_joins(table)))
}

/// `GET workspaces/<slug>/projects/<id>/advance-analytics/`
/// (`ProjectAdvanceAnalyticsEndpoint.get`, `project_analytics.py:84-94`).
async fn project_advance_get(
    State(state): State<AppState>,
    extension: Option<axum::Extension<SessionHandle>>,
    Path((slug, project_id_raw)): Path<(String, String)>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Response, Denial> {
    let actor = actor(&state, extension).await?;
    let pool = pool_of(&state)?;
    let project_id = resolve_project_id(pool, &slug, &project_id_raw).await?;
    let ws_role = workspace_role(pool, &slug, &actor.id).await?;
    let pm_role = project_role(pool, &slug, &project_id, &actor.id).await?;
    check_project_gate(
        PATH_PROJECT_ADVANCE,
        &slug,
        ws_role,
        pm_role,
        PROJECT_ADMIN_MEMBER,
    )?;

    // `initialize_workspace(slug, type="analytics")` (project_analytics.py:32-43).
    let now = Utc::now();
    let ctx = FilterCtx::parse(&params, "analytics", now);
    let branch = through_branch(&params)?;
    let body = project_overview_data(pool, &slug, &actor.id, &ctx, &project_id, branch).await?;
    Ok(json_response(StatusCode::OK, body))
}

/// `get_work_items_stats` (`project_analytics.py:58-82`): the base scope plus
/// the route's `project_id` — or the through-table id list on the
/// cycle/module branches (which carry `base_filters` but NO project scoping,
/// ported quirk B4) — plus one `state__group` filter per key, in
/// [`q::PROJECT_WORK_ITEM_KEYS`] order.
async fn project_overview_data(
    pool: &sqlx::PgPool,
    slug: &str,
    user_id: &uuid::Uuid,
    ctx: &FilterCtx,
    project_id: &uuid::Uuid,
    branch: ThroughBranch,
) -> Result<String, Denial> {
    let base = issue_scope(ctx)?;
    let window = ctx.window_ph();
    let wph = || {
        window
            .as_ref()
            .map(|(gte, lte)| (gte.as_str(), lte.as_str()))
    };
    let narrowing = match branch {
        ThroughBranch::Plain => format!(
            " AND \"issues\".\"project_id\" = {}",
            uuid_literal(project_id)
        ),
        _ => format!(
            " AND \"issues\".\"id\" IN ({})",
            through_ids_subquery(branch, ctx)?
        ),
    };
    let mut out = Map::new();
    for (key, group) in q::PROJECT_WORK_ITEM_KEYS {
        let mut scope = format!("{base}{narrowing}");
        if let Some(group) = group {
            scope.push_str(&format!(" AND \"states\".\"group\" = '{group}'"));
        }
        let sql = with_joins(
            q::project_filtered_count_sql("issues", &scope, wph()),
            "issues",
            ISSUE_JOINS,
        );
        let count = count_one(pool, &sql, slug, user_id, ctx, RangeKind::Window).await?;
        out.insert((*key).to_owned(), count_obj(count));
    }
    Ok(Value::Object(out).to_string())
}

/// `ProjectAdvanceAnalyticsStatsEndpoint.get`
/// (`project_analytics.py:165-179`).
async fn project_advance_stats_get(
    State(state): State<AppState>,
    extension: Option<axum::Extension<SessionHandle>>,
    Path((slug, project_id_raw)): Path<(String, String)>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Response, Denial> {
    let actor = actor(&state, extension).await?;
    let pool = pool_of(&state)?;
    let project_id = resolve_project_id(pool, &slug, &project_id_raw).await?;
    let ws_role = workspace_role(pool, &slug, &actor.id).await?;
    let pm_role = project_role(pool, &slug, &project_id, &actor.id).await?;
    check_project_gate(
        PATH_PROJECT_STATS,
        &slug,
        ws_role,
        pm_role,
        PROJECT_ADMIN_MEMBER,
    )?;

    // `initialize_workspace(slug, type="chart")` (project_analytics.py:32-43).
    let now = Utc::now();
    let ctx = FilterCtx::parse(&params, "chart", now);
    let stats_type = params
        .get("type")
        .map(String::as_str)
        .unwrap_or("work-items");
    if stats_type != "work-items" {
        return Err(Denial::InvalidType);
    }
    let branch = through_branch(&params)?;
    let body = project_assignee_stats(pool, &slug, &actor.id, &ctx, &project_id, branch).await?;
    Ok(json_response(StatusCode::OK, body))
}

/// `get_work_items_stats` (`project_analytics.py:119-163`):
/// `values("display_name", "assignee_id", "avatar_url")` + five
/// `Count(id, filter=Q(...), distinct=True)` `.order_by("display_name")`.
/// NOTE: the method never reads `chart_period_range`, so `?date_filter=` is
/// silently ignored here (like the workspace stats dead-method quirk).
async fn project_assignee_stats(
    pool: &sqlx::PgPool,
    slug: &str,
    user_id: &uuid::Uuid,
    ctx: &FilterCtx,
    project_id: &uuid::Uuid,
    branch: ThroughBranch,
) -> Result<String, Denial> {
    let base = issue_scope(ctx)?;
    let sub = match branch {
        ThroughBranch::Plain => None,
        _ => Some(through_ids_subquery(branch, ctx)?),
    };
    let shell = q::project_assignee_stats_sql(&base, &uuid_literal(project_id), sub.as_deref());
    let joins = format!("{ISSUE_JOINS}{ASSIGNEE_JOINS}");
    let sql = with_joins(shell, "issues", &joins);
    type AssigneeStatRow = (
        Option<String>,
        Option<uuid::Uuid>,
        Option<String>,
        i64,
        i64,
        i64,
        i64,
        i64,
    );
    let rows: Vec<AssigneeStatRow> = sqlx::query_as(&sql)
        .bind(slug)
        .bind(user_id)
        .fetch_all(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let mut arr = Vec::with_capacity(rows.len());
    for (
        display_name,
        assignee_id,
        avatar_url,
        cancelled,
        completed,
        backlog,
        un_started,
        started,
    ) in rows
    {
        let mut row = Map::new();
        row.insert(
            "display_name".to_owned(),
            display_name.map(Value::from).unwrap_or(Value::Null),
        );
        row.insert(
            "assignee_id".to_owned(),
            assignee_id
                .map(|id| Value::from(id.to_string()))
                .unwrap_or(Value::Null),
        );
        row.insert(
            "avatar_url".to_owned(),
            avatar_url.map(Value::from).unwrap_or(Value::Null),
        );
        row.insert("cancelled_work_items".to_owned(), Value::from(cancelled));
        row.insert("completed_work_items".to_owned(), Value::from(completed));
        row.insert("backlog_work_items".to_owned(), Value::from(backlog));
        row.insert("un_started_work_items".to_owned(), Value::from(un_started));
        row.insert("started_work_items".to_owned(), Value::from(started));
        arr.push(Value::Object(row));
    }
    Ok(Value::Array(arr).to_string())
}

/// `ProjectAdvanceAnalyticsChartEndpoint.get`
/// (`project_analytics.py:317-367`).
async fn project_advance_charts_get(
    State(state): State<AppState>,
    extension: Option<axum::Extension<SessionHandle>>,
    Path((slug, project_id_raw)): Path<(String, String)>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Response, Denial> {
    let actor = actor(&state, extension).await?;
    let pool = pool_of(&state)?;
    let project_id = resolve_project_id(pool, &slug, &project_id_raw).await?;
    let ws_role = workspace_role(pool, &slug, &actor.id).await?;
    let pm_role = project_role(pool, &slug, &project_id, &actor.id).await?;
    check_project_gate(
        PATH_PROJECT_CHARTS,
        &slug,
        ws_role,
        pm_role,
        PROJECT_ADMIN_MEMBER_GUEST,
    )?;

    // `initialize_workspace(slug, type="chart")` (project_analytics.py:32-43).
    let now = Utc::now();
    let ctx = FilterCtx::parse(&params, "chart", now);
    let chart_type = params.get("type").map(String::as_str).unwrap_or("projects");

    if chart_type == "custom-work-items" {
        // `x_axis` defaults to `"PRIORITY"` (`project_analytics.py:322`).
        let x_axis = params
            .get("x_axis")
            .map(String::as_str)
            .unwrap_or(q::DEFAULT_CHART_X_AXIS);
        let group_by = params.get("group_by").map(String::as_str);
        let branch = through_branch(&params)?;
        // Unlike the other branches, the custom path KEEPS the route's
        // `project_id` scoping alongside the optional id list
        // (`project_analytics.py:327-345`).
        let mut extra = format!(
            " AND \"issues\".\"project_id\" = {}",
            uuid_literal(&project_id)
        );
        if branch != ThroughBranch::Plain {
            extra.push_str(&format!(
                " AND \"issues\".\"id\" IN ({})",
                through_ids_subquery(branch, &ctx)?
            ));
        }
        let body =
            build_analytics_chart(pool, &slug, &actor.id, &ctx, x_axis, group_by, &extra).await?;
        return Ok(json_response(StatusCode::OK, body));
    }
    if chart_type == "work-items" {
        let branch = through_branch(&params)?;
        let body = project_completion_chart(pool, &slug, &actor.id, &ctx, &project_id, branch, now)
            .await?;
        return Ok(json_response(StatusCode::OK, body));
    }
    Err(Denial::InvalidType)
}

/// `work_item_completion_chart` (`project_analytics.py:183-315`): the daily
/// through-table chart on the cycle/module branches, the monthly
/// `TruncMonth` chart otherwise. The cycle/module lookups are global by id
/// (no workspace/project scoping — `Cycle.objects.filter(id=...)`); a missing
/// row or start date is the empty body, while a missing END date 500s
/// (`cycle.end_date.date()` / the `<= None` comparison raising into the
/// generic branch).
#[allow(clippy::too_many_arguments)]
async fn project_completion_chart(
    pool: &sqlx::PgPool,
    slug: &str,
    user_id: &uuid::Uuid,
    ctx: &FilterCtx,
    project_id: &uuid::Uuid,
    branch: ThroughBranch,
    now: DateTime<Utc>,
) -> Result<String, Denial> {
    match branch {
        ThroughBranch::Cycle(id) => {
            type CycleDatesRow = (Option<DateTime<Utc>>, Option<DateTime<Utc>>);
            let row: Option<CycleDatesRow> = sqlx::query_as(
                "SELECT \"start_date\", \"end_date\" FROM \"cycles\" WHERE \"id\" = $1 AND \"deleted_at\" IS NULL",
            )
            .bind(id)
            .fetch_optional(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
            let (start, end) = match row {
                Some((Some(start), end)) => (start.date_naive(), end),
                _ => return Ok(EMPTY_PROJECT_CHART_BODY.to_owned()),
            };
            let end = end.map(|end| end.date_naive()).ok_or(Denial::ServerError)?;
            project_completion_daily(
                pool,
                slug,
                user_id,
                ctx,
                "cycle_issues",
                "cycle_id",
                &id,
                start,
                end,
            )
            .await
        }
        ThroughBranch::Module(id) => {
            let row: Option<(Option<NaiveDate>, Option<NaiveDate>)> = sqlx::query_as(
                "SELECT \"start_date\", \"target_date\" FROM \"modules\" WHERE \"id\" = $1 AND \"deleted_at\" IS NULL",
            )
            .bind(id)
            .fetch_optional(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
            let (start, end) = match row {
                Some((Some(start), end)) => (start, end),
                _ => return Ok(EMPTY_PROJECT_CHART_BODY.to_owned()),
            };
            let end = end.ok_or(Denial::ServerError)?;
            project_completion_daily(
                pool,
                slug,
                user_id,
                ctx,
                "module_issues",
                "module_id",
                &id,
                start,
                end,
            )
            .await
        }
        ThroughBranch::Plain => {
            // `Project.objects.filter(id=project_id).first()` (default manager;
            // unreachable past the gate for live projects, but a soft-deleted
            // project keeps its membership rows, so the MISS branch is live:
            // `project.created_at` on `None` is the generic 500).
            let row: Option<(Option<DateTime<Utc>>,)> = sqlx::query_as(
                "SELECT \"created_at\" FROM \"projects\" WHERE \"id\" = $1 AND \"deleted_at\" IS NULL",
            )
            .bind(project_id)
            .fetch_optional(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
            let start = match row {
                Some((Some(created),)) => created.date_naive().with_day(1).expect("month start"),
                Some((None,)) => return Ok(EMPTY_PROJECT_CHART_BODY.to_owned()),
                None => return Err(Denial::ServerError),
            };
            let mut scope = issue_scope(ctx)?;
            scope.push_str(&format!(
                " AND \"issues\".\"project_id\" = {}",
                uuid_literal(project_id)
            ));
            let period = ctx.period_ph();
            let pph = || {
                period
                    .as_ref()
                    .map(|(gte, lte)| (gte.as_str(), lte.as_str()))
            };
            let shell = q::advance_completion_monthly_sql(&scope, pph());
            let sql = with_joins(shell, "issues", ISSUE_JOINS);
            let rows: Vec<(DateTime<Utc>, i64, i64)> =
                bind_range!(sqlx::query_as(&sql), slug, user_id, ctx, RangeKind::Period)
                    .fetch_all(pool)
                    .await
                    .map_err(|_| Denial::ServerError)?;
            let mut stats = HashMap::new();
            for (month, created, completed) in rows {
                stats.insert(month.format("%Y-%m-%d").to_string(), (created, completed));
            }
            let first = match ctx.period {
                Some((period_start, _)) => period_start,
                None => start,
            };
            completion_monthly_body(&stats, first, now)
        }
    }
}

/// The cycle/module daily branch (`project_analytics.py:223-258`):
/// `values("created_at__date")` over the through rows (their OWN
/// `created_at`), `created=Count(id)` / `completed=Count(id,
/// filter=Q(issue__state__group="completed"))`, then a per-day zero-fill.
/// NOTE: the Q-05 `project_completion_daily_sql` sketch is not valid SQL (it
/// quotes the Django lookup `"issue"."state__group"` verbatim), so this path
/// owns the complete statement: the completed count joins `issues` (plain
/// forward join — Django never applies a related manager there) and `states`.
/// Ported bug B5: `count = created + completed`, so completed rows count
/// twice in `count`. No project scoping and no period filter here.
#[allow(clippy::too_many_arguments)]
async fn project_completion_daily(
    pool: &sqlx::PgPool,
    slug: &str,
    user_id: &uuid::Uuid,
    ctx: &FilterCtx,
    through_table: &str,
    fk_col: &str,
    fk_id: &uuid::Uuid,
    start: NaiveDate,
    end: NaiveDate,
) -> Result<String, Denial> {
    let base = cycle_like_scope(through_table, ctx)?;
    let joins = format!(
        "{0} LEFT OUTER JOIN \"issues\" ON (\"{1}\".\"issue_id\" = \"issues\".\"id\") \
         LEFT OUTER JOIN \"states\" ON (\"issues\".\"state_id\" = \"states\".\"id\")",
        cycle_like_joins(through_table),
        through_table,
    );
    let sql = format!(
        "SELECT \"{t}\".\"created_at\"::date AS \"created_at__date\", \
         COUNT(\"{t}\".\"id\") AS \"created_count\", \
         COUNT(\"{t}\".\"id\") FILTER (WHERE \"states\".\"group\" = 'completed') AS \"completed_count\" \
         FROM \"{t}\"{joins} WHERE ({base} AND \"{t}\".\"{fk}\" = {lit}) \
         GROUP BY \"{t}\".\"created_at\"::date ORDER BY \"{t}\".\"created_at\"::date",
        t = through_table,
        joins = joins,
        base = base,
        fk = fk_col,
        lit = uuid_literal(fk_id),
    );
    let rows: Vec<(NaiveDate, i64, i64)> = sqlx::query_as(&sql)
        .bind(slug)
        .bind(user_id)
        .fetch_all(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let mut stats = HashMap::new();
    for (day, created, completed) in rows {
        stats.insert(day.format("%Y-%m-%d").to_string(), (created, completed));
    }
    let mut data = Vec::new();
    let mut current = start;
    while current <= end {
        let key = current.format("%Y-%m-%d").to_string();
        let (created, completed) = stats.get(&key).copied().unwrap_or((0, 0));
        let mut row = Map::new();
        row.insert("key".to_owned(), Value::from(key.clone()));
        row.insert("name".to_owned(), Value::from(key));
        row.insert("count".to_owned(), Value::from(created + completed));
        row.insert("completed_issues".to_owned(), Value::from(completed));
        row.insert("created_issues".to_owned(), Value::from(created));
        data.push(Value::Object(row));
        current = current.succ_opt().ok_or(Denial::ServerError)?;
    }
    let mut schema = Map::new();
    schema.insert(
        "completed_issues".to_owned(),
        Value::from(q::COMPLETION_SCHEMA_COMPLETED),
    );
    schema.insert(
        "created_issues".to_owned(),
        Value::from(q::COMPLETION_SCHEMA_CREATED),
    );
    let mut out = Map::new();
    out.insert("data".to_owned(), Value::Array(data));
    out.insert("schema".to_owned(), Value::Object(schema));
    Ok(Value::Object(out).to_string())
}

/// Render the monthly zero-fill body shared by the workspace
/// (`advance.py:287-309`) and project (`project_analytics.py:285-308`)
/// completion charts: `first` is the loop start (the `created_at`
/// month-start, OVERWRITTEN as-is by the period start when
/// `chart_period_range` is set — no month-start normalization, ported quirk:
/// a mid-month start both empties the loop when it falls after the 1st and
/// can 500 on the day-preserving month step, e.g. Aug 31 -> Sept 31, exactly
/// like Python's `replace` raising `ValueError` into the generic 500). Keys
/// after the first are therefore not necessarily month-starts and never match
/// the month-keyed stats dict (count 0 there).
fn completion_monthly_body(
    stats: &HashMap<String, (i64, i64)>,
    first: NaiveDate,
    now: DateTime<Utc>,
) -> Result<String, Denial> {
    let mut current = first;
    let last_month = now.date_naive().with_day(1).expect("month start");
    let mut data = Vec::new();
    while current <= last_month {
        let key = current.format("%Y-%m-%d").to_string();
        let (created, completed) = stats.get(&key).copied().unwrap_or((0, 0));
        let mut row = Map::new();
        row.insert("key".to_owned(), Value::from(key.clone()));
        row.insert("name".to_owned(), Value::from(key));
        row.insert("count".to_owned(), Value::from(created));
        row.insert("completed_issues".to_owned(), Value::from(completed));
        row.insert("created_issues".to_owned(), Value::from(created));
        data.push(Value::Object(row));
        // Day-preserving month step (`replace(year, month)`); an invalid
        // day is the 500 above.
        let (next_year, next_month) = if current.month() == 12 {
            (current.year() + 1, 1)
        } else {
            (current.year(), current.month() + 1)
        };
        current = NaiveDate::from_ymd_opt(next_year, next_month, current.day())
            .ok_or(Denial::ServerError)?;
    }
    let mut schema = Map::new();
    schema.insert(
        "completed_issues".to_owned(),
        Value::from(q::COMPLETION_SCHEMA_COMPLETED),
    );
    schema.insert(
        "created_issues".to_owned(),
        Value::from(q::COMPLETION_SCHEMA_CREATED),
    );
    let mut out = Map::new();
    out.insert("data".to_owned(), Value::Array(data));
    out.insert("schema".to_owned(), Value::Object(schema));
    Ok(Value::Object(out).to_string())
}

// ---------------------------------------------------------------------------
// Handlers-B shell (PIDASHCONV-399); routes merge into routes() above
// ---------------------------------------------------------------------------

/// A GET-owned path: the GET handler owns reads, everything else falls
/// through to Django. `HEAD` rides axum's `get` handling like Django's
/// `GET`-backed `HEAD`.
fn owned_get(
    get_handler: axum::routing::MethodRouter<AppState>,
) -> axum::routing::MethodRouter<AppState> {
    get_handler
        .post(crate::edge::proxy)
        .put(crate::edge::proxy)
        .patch(crate::edge::proxy)
        .delete(crate::edge::proxy)
        .options(crate::edge::proxy)
}

/// The POST-owned export path: POST owns the acknowledgement, everything
/// else falls through to Django.
fn owned_post(
    post_handler: axum::routing::MethodRouter<AppState>,
) -> axum::routing::MethodRouter<AppState> {
    post_handler
        .get(crate::edge::proxy)
        .put(crate::edge::proxy)
        .patch(crate::edge::proxy)
        .delete(crate::edge::proxy)
        .options(crate::edge::proxy)
}

fn json_value_response(status: StatusCode, value: &Value) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(value.to_string()))
        .expect("analytic response")
}

// ---------------------------------------------------------------------------
// Auth + gate
// ---------------------------------------------------------------------------

/// The authenticated caller the handlers need: the email for the
/// export acknowledgement (`str(request.user.email)`). Membership
/// rows are consumed inside the gate; no handler re-scopes by user.
struct Authed {
    email: Option<String>,
}

fn pool_owned(state: &AppState) -> Result<sqlx::PgPool, IssuesDenial> {
    state
        .pools()
        .map(|pools| pools.primary().clone())
        .ok_or(IssuesDenial::ServerError)
}

/// Session auth (`BaseAPIView`: session auth + `IsAuthenticated`) then
/// the route's `@allow_permission` gate from [`gate_for`], in Django's
/// order. Anonymous answers 401 before anything else (the slug-existence
/// oracle stays closed); a caller with no allowed-role row — including
/// unknown slugs and cross-tenant slugs — answers the decorator 403.
async fn authed_gate(
    state: &AppState,
    slug: &str,
    method: &str,
    path: &str,
    extension: Option<axum::Extension<SessionHandle>>,
) -> Result<Authed, IssuesDenial> {
    let pool = pool_owned(state)?;
    let actor =
        crate::license::resolve_actor(&pool, state.settings().secret_key.as_bytes(), extension)
            .await
            .map_err(|_| IssuesDenial::ServerError)?
            .ok_or(IssuesDenial::Unauthorized)?;
    let row = gate_for(method, path).ok_or(IssuesDenial::ServerError)?;
    // Same row filters as the decorator (`is_active=True`, soft-delete
    // scope, slug scoping); `role` is non-nullable, the outer Option is
    // row presence.
    let member: Option<(Option<i16>,)> = sqlx::query_as(
        r#"SELECT wm.role FROM workspace_members wm
           JOIN workspaces w ON w.id = wm.workspace_id
           WHERE w.slug = $1 AND wm.member_id = $2 AND wm.is_active AND wm.deleted_at IS NULL"#,
    )
    .bind(slug)
    .bind(actor.id)
    .fetch_optional(&pool)
    .await
    .map_err(|_| IssuesDenial::ServerError)?;
    let role = member.and_then(|row| row.0).map(i32::from);
    let allowed = match row.gate {
        super::gates::Gate::Workspace { roles } | super::gates::Gate::Project { roles } => {
            role.is_some_and(|role| roles.contains(&role))
        }
        // `WorkSpaceAdminPermission` allows ADMIN or MEMBER
        // (`permissions/workspace.py:61-71`); only the denial body
        // differs, which never applies to these four routes.
        super::gates::Gate::ViewsetAdmin => role.is_some_and(|role| {
            role == ROLE_ADMIN || role == pidash_auth::permissions::ROLE_MEMBER
        }),
    };
    let facts = AllowFacts {
        workspace: WorkspaceId::from(slug),
        authenticated: true,
        is_workspace_member: role.is_some(),
        has_allowed_workspace_role: allowed,
        is_creator: false,
        has_allowed_project_role: false,
        is_project_member: false,
        is_workspace_admin: role == Some(ROLE_ADMIN),
    };
    match decide_gate(&row.gate, &tenant_context(slug), &facts) {
        GateOutcome::Allow => Ok(Authed { email: actor.email }),
        GateOutcome::Deny => Err(IssuesDenial::Forbidden),
        GateOutcome::Unauthenticated => Err(IssuesDenial::Unauthorized),
    }
}

// ---------------------------------------------------------------------------
// Execution SQL: scope, joins, legacy/stored filters
// ---------------------------------------------------------------------------

/// `FROM` + joins shared by every `Issue.issue_objects` read on this
/// path. Django's manager exclusion (`db/models/issue.py:95-104`) joins
/// `states` (triage), `projects` (project-archived) and `workspaces`
/// (slug); `workspaces` joins on the direct `issues.workspace_id` FK,
/// which is what `workspace__slug` traverses. All three are INNER, so a
/// NULL state row drops out exactly like the manager's
/// `NOT (group = 'triage')` three-valued exclusion. Caller-supplied link
/// joins (legacy/stored filters, dimensions) append as `extra_joins`.
/// `with_states` drops the manager `states` join when a dimension join
/// already targets the table (e.g. `state__group` segments): Postgres
/// rejects the same table name twice while Django reuses the single
/// join, and the triage predicate applies over the remaining join.
fn issue_from(extra_joins: &str) -> String {
    issue_from_states(extra_joins, true)
}

fn issue_from_states(extra_joins: &str, with_states: bool) -> String {
    let states = if with_states {
        " INNER JOIN \"states\" ON (\"issues\".\"state_id\" = \"states\".\"id\")"
    } else {
        ""
    };
    format!(
        "FROM \"issues\" \
         INNER JOIN \"projects\" ON (\"issues\".\"project_id\" = \"projects\".\"id\") \
         INNER JOIN \"workspaces\" ON (\"issues\".\"workspace_id\" = \"workspaces\".\"id\"){states}{extra_joins}"
    )
}

/// `WHERE` predicates for one `issue_objects` read: the workspace slug
/// (`$1`, bound first by every handler), the manager + soft-delete scope
/// ([`q::ISSUE_OBJECTS_SCOPE`], pinned spelling owned by the services
/// layer), and the caller's compiled filter fragment, if any.
fn issue_where(slug_ph: &str, filters: Option<&str>) -> String {
    let mut parts = vec![
        format!("\"workspaces\".\"slug\" = {slug_ph}"),
        q::ISSUE_OBJECTS_SCOPE.to_owned(),
    ];
    if let Some(fragment) = filters {
        parts.push(format!("({fragment})"));
    }
    parts.join(" AND ")
}

/// Start a bind chain with the workspace slug at `$1`, matching the
/// services builders' hardcoded slug placeholder. Returns the binder
/// (later binds continue at `$2`) — the `"$1"` text itself is discarded
/// since the statements spell it literally.
fn slugs_first(slug: &str) -> crate::app_issues::Binder {
    let mut binder = crate::app_issues::Binder::new();
    let first = binder.bind_string(slug.to_owned());
    debug_assert_eq!(first, "$1");
    binder
}

/// Remap one `issue_filters` predicate fragment from the D-26
/// (`app_issues`) alias spelling to this path's quoted-table spelling.
/// The D-26 renderer emits Django's query aliases (`issue.`,
/// `label_issue.`, `state.` …); the analytics statements address the
/// real tables (`"issues".`, `"issue_labels".`, `"states".` …) per the
/// fixture templates. Longer aliases first so no prefix shadows another
/// (`issue.` never matches `issue_assignee.` — the patterns all carry
/// their trailing dot).
fn remap_legacy(fragment: &str) -> String {
    fragment
        .replace("issue_intake.", "\"intake_issues\".")
        .replace("issue_subscribers.", "\"issue_subscribers\".")
        .replace("issue_mention.", "\"issue_mentions\".")
        .replace("issue_assignee.", "\"issue_assignees\".")
        .replace("label_issue.", "\"issue_labels\".")
        .replace("issue_cycle.", "\"cycle_issues\".")
        .replace("issue_module.", "\"module_issues\".")
        .replace("state.", "\"states\".")
        .replace("issue.", "\"issues\".")
}

/// `LEFT JOIN`s for every link table a remapped fragment references.
/// Uniformly LEFT (never Django's `filter()` INNER): the predicates sit
/// in `WHERE`, so positive references filter identically either way,
/// while `__isnull` tests require the outer join. Mirrors the D-26
/// `RELATION_JOINS` table-to-alias map. Tables already present in
/// `existing` (dimension/assignee joins composed earlier) are skipped:
/// Postgres rejects the same table name twice, while Django reuses the
/// single join.
fn link_joins_for(fragment: &str, existing: &str) -> String {
    const LINKS: &[&str] = &[
        "\"issue_labels\"",
        "\"issue_assignees\"",
        "\"cycle_issues\"",
        "\"module_issues\"",
        "\"issue_mentions\"",
        "\"issue_subscribers\"",
        "\"intake_issues\"",
    ];
    let mut out = String::new();
    for table in LINKS {
        if fragment.contains(&format!("{table}.")) && !existing.contains(table) {
            out.push_str(&format!(
                " LEFT JOIN {table} ON ({table}.\"issue_id\" = \"issues\".\"id\")"
            ));
        }
    }
    out
}

/// Compile the request's `issue_filters(request.GET, "GET")` predicates
/// (`DefaultAnalyticsEndpoint`, `base.py:255`) into a remapped `WHERE`
/// fragment. Relative dates resolve against the UTC date, exactly like
/// `timezone.now().date()` in `issue_filters.py`. Link joins compose at
/// the call site via [`link_joins_for`] (per statement family, so joins
/// already present are skipped). An empty filter set compiles to no
/// fragment (the suite's world sends no filter params on these routes).
fn legacy_filters(
    query: &QueryMap,
    binder: &mut crate::app_issues::Binder,
) -> Result<Option<String>, IssuesDenial> {
    let flat: HashMap<String, String> = query
        .keys()
        .filter_map(|key| query_last(query, key).map(|last| (key.clone(), last)))
        .collect();
    let today = chrono::Utc::now().date_naive();
    let legacy = issue_filters_get(&flat, "", today).map_err(|_| IssuesDenial::ServerError)?;
    let mut parts = Vec::new();
    for (name, value) in legacy.predicates() {
        // Error bodies are already exact (`{"error": "Please provide
        // valid detail"}` for unparseable UUIDs, 500 for unknown
        // lookups — Django's `FieldError` generic 500); propagate.
        let fragment = crate::app_issues::legacy_sql(binder, name, value)?;
        parts.push(remap_legacy(&fragment));
    }
    if parts.is_empty() {
        return Ok(None);
    }
    Ok(Some(parts.join(" AND ")))
}

// ---------------------------------------------------------------------------
// Stored `AnalyticView.query` compiler (saved-analytic scope)
// ---------------------------------------------------------------------------

/// Quoted column for one stored-query lookup path. Covers the ORM forms
/// `issue_filters(..., "POST"/"PATCH")` emits (the only writers of this
/// column, via `AnalyticViewSerializer.create/update`) plus the
/// hand-written `workspace__slug` pin the seed uses. Anything else is a
/// Django `FieldError` → generic 500.
fn stored_column(path: &str) -> Result<&'static str, IssuesDenial> {
    Ok(match path {
        "workspace__slug" => "\"workspaces\".\"slug\"",
        "priority" | "priority__in" => "\"issues\".\"priority\"",
        "state__group" | "state__group__in" => "\"states\".\"group\"",
        "state" | "state__in" => "\"issues\".\"state_id\"",
        "parent" | "parent__in" => "\"issues\".\"parent_id\"",
        "project" | "project__in" => "\"issues\".\"project_id\"",
        "estimate_point" | "estimate_point__in" => "\"issues\".\"estimate_point_id\"",
        "created_by" | "created_by__in" => "\"issues\".\"created_by_id\"",
        "name" => "\"issues\".\"name\"",
        _ => return Err(IssuesDenial::ServerError),
    })
}

/// True for the UUID-typed stored columns: values parse-or-500 (Django
/// coerces via `UUIDField.get_prep_value`; garbage is a
/// `ValidationError` only on some paths and a DB error on others — both
/// become the generic 500 here, matching the project-stats B7 handling).
fn stored_is_uuid(path: &str) -> bool {
    matches!(
        path,
        "state"
            | "state__in"
            | "parent"
            | "parent__in"
            | "project"
            | "project__in"
            | "estimate_point"
            | "estimate_point__in"
            | "created_by"
            | "created_by__in"
    )
}

fn bind_stored_value(
    binder: &mut crate::app_issues::Binder,
    column: &str,
    value: &Value,
    is_uuid: bool,
) -> Result<String, IssuesDenial> {
    if value.is_null() {
        // Django `filter(field=None)` → `IS NULL`.
        return Ok(format!("{column} IS NULL"));
    }
    if is_uuid {
        let raw = value.as_str().ok_or(IssuesDenial::ServerError)?;
        let id: uuid::Uuid = raw.parse().map_err(|_| IssuesDenial::ServerError)?;
        let holder = binder.bind(sea_query::Value::Uuid(Some(Box::new(id))));
        return Ok(format!("{column} = {holder}"));
    }
    let holder = match value {
        Value::String(text) => binder.bind_string(text.clone()),
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                binder.bind(sea_query::Value::BigInt(Some(int)))
            } else if let Some(uint) = number.as_u64() {
                binder.bind(sea_query::Value::BigUnsigned(Some(uint)))
            } else if let Some(float) = number.as_f64() {
                binder.bind(sea_query::Value::Double(Some(float)))
            } else {
                return Err(IssuesDenial::ServerError);
            }
        }
        Value::Bool(flag) => binder.bind(sea_query::Value::Bool(Some(*flag))),
        Value::Array(_) | Value::Object(_) => return Err(IssuesDenial::ServerError),
        Value::Null => unreachable!("null handled above"),
    };
    Ok(format!("{column} = {holder}"))
}

/// One stored-query entry (`key: value` of the `AnalyticView.query`
/// JSON, used VERBATIM as ORM kwargs, `base.py:195-196`).
fn stored_predicate(
    key: &str,
    value: &Value,
    binder: &mut crate::app_issues::Binder,
) -> Result<String, IssuesDenial> {
    if let Some(path) = key.strip_suffix("__isnull") {
        let column = stored_column(path)?;
        let flag = value.as_bool().ok_or(IssuesDenial::ServerError)?;
        return Ok(if flag {
            format!("{column} IS NULL")
        } else {
            format!("{column} IS NOT NULL")
        });
    }
    if key.ends_with("__in") {
        let column = stored_column(key)?;
        let items: Vec<&Value> = match value {
            Value::Array(items) => items.iter().collect(),
            // `{"priority__in": "high"}` (the analytic-view CRUD
            // contract): a lone scalar is a one-element `IN`.
            single => vec![single],
        };
        if items.is_empty() {
            return Ok("FALSE".to_owned());
        }
        let is_uuid = stored_is_uuid(key);
        let mut holders = Vec::with_capacity(items.len());
        for item in items {
            if item.is_null() {
                // `IN` never matches NULL; Django emits a separate
                // `IS NULL` clause, which an `IN` list cannot spell —
                // none of the writers produce it. Stay silent-free: 500.
                return Err(IssuesDenial::ServerError);
            }
            if is_uuid {
                let raw = item.as_str().ok_or(IssuesDenial::ServerError)?;
                let id: uuid::Uuid = raw.parse().map_err(|_| IssuesDenial::ServerError)?;
                holders.push(binder.bind(sea_query::Value::Uuid(Some(Box::new(id)))));
            } else {
                let holder = match item {
                    Value::String(text) => binder.bind_string(text.clone()),
                    Value::Number(number) => {
                        if let Some(int) = number.as_i64() {
                            binder.bind(sea_query::Value::BigInt(Some(int)))
                        } else if let Some(uint) = number.as_u64() {
                            binder.bind(sea_query::Value::BigUnsigned(Some(uint)))
                        } else if let Some(float) = number.as_f64() {
                            binder.bind(sea_query::Value::Double(Some(float)))
                        } else {
                            return Err(IssuesDenial::ServerError);
                        }
                    }
                    Value::Bool(flag) => binder.bind(sea_query::Value::Bool(Some(*flag))),
                    Value::Array(_) | Value::Object(_) | Value::Null => {
                        return Err(IssuesDenial::ServerError)
                    }
                };
                holders.push(holder);
            }
        }
        return Ok(format!("{column} IN ({})", holders.join(",")));
    }
    let column = stored_column(key)?;
    bind_stored_value(binder, column, value, stored_is_uuid(key))
}

/// Compile the stored `AnalyticView.query` JSON to a `WHERE` fragment.
/// A non-object query is Django's `filter(**None)` `TypeError` →
/// generic 500. Axes never come from here (they come from `query_dict`,
/// `base.py:198-199`).
fn stored_filters(
    query: &Value,
    binder: &mut crate::app_issues::Binder,
) -> Result<Option<String>, IssuesDenial> {
    let map = match query {
        Value::Object(map) => map,
        _ => return Err(IssuesDenial::ServerError),
    };
    let mut parts = Vec::new();
    for (key, value) in map {
        parts.push(stored_predicate(key, value, binder)?);
    }
    if parts.is_empty() {
        return Ok(None);
    }
    Ok(Some(parts.join(" AND ")))
}

// ---------------------------------------------------------------------------
// Plot regroup (build_graph_plot tail, analytics_plot.py:117-120 + 64-70)
// ---------------------------------------------------------------------------

/// Python `str()` over a dimension/segment scalar: `None` → `"None"`
/// (the dead-annotation quirk above), bools capitalised, numbers plain.
/// Dimensions are scalars in every exercised path.
fn py_str(value: &Value) -> String {
    match value {
        Value::Null => "None".to_owned(),
        Value::Bool(flag) => {
            if *flag {
                "True".to_owned()
            } else {
                "False".to_owned()
            }
        }
        Value::Number(number) => number.to_string(),
        Value::String(text) => text.clone(),
        Value::Array(_) | Value::Object(_) => value.to_string(),
    }
}

/// Regroup ordered plot rows the way `itertools.groupby` + `sort_data`
/// do: contiguous rows (the SQL `ORDER BY "dimension"` guarantees it)
/// keyed by `str(dimension)`, items `{"dimension", ["segment",]
/// value_key}`, groups emitted in [`q::sort_data_keys`] order
/// (`priority` keeps `low,medium,high,urgent,none` present keys —
/// dropping the `"None"` NULL bucket; every other axis sorts with
/// `"none"` last).
fn regroup_plot(
    rows: &[Map<String, Value>],
    temp_axis: &str,
    value_key: &str,
    segmented: bool,
) -> Value {
    let mut order: Vec<String> = Vec::new();
    let mut groups: HashMap<String, Vec<Value>> = HashMap::new();
    for row in rows {
        let dimension = row.get("dimension").cloned().unwrap_or(Value::Null);
        let key = py_str(&dimension);
        let mut item = Map::new();
        item.insert("dimension".to_owned(), dimension);
        if segmented {
            item.insert(
                "segment".to_owned(),
                row.get("segment").cloned().unwrap_or(Value::Null),
            );
        }
        item.insert(
            value_key.to_owned(),
            row.get(value_key).cloned().unwrap_or(Value::Null),
        );
        if !groups.contains_key(&key) {
            order.push(key.clone());
        }
        groups.entry(key).or_default().push(Value::Object(item));
    }
    let mut out = Map::new();
    for key in q::sort_data_keys(&order, temp_axis) {
        if let Some(items) = groups.remove(&key) {
            out.insert(key, Value::Array(items));
        }
    }
    Value::Object(out)
}

// ---------------------------------------------------------------------------
// Saved-analytic plot statements
// ---------------------------------------------------------------------------

/// `SavedAnalyticEndpoint` count + distribution over the stored scope:
/// `Issue.issue_objects.filter(**stored)` (`base.py:196`), axes from
/// `query_dict` (`:198-199`), `segment` from the request (`:207`),
/// distribution/total reuse the Q-01b/Q-01a shapes. Dimension SQL comes
/// from [`q::dimension_sql`] (same `F(x_axis)` + join mapping the
/// services layer pins); the scope (slug + manager + stored filters)
/// composes here because only the handler owns the `FROM` joins.
fn saved_statements(
    x_axis: &str,
    y_axis: &str,
    segment: Option<&str>,
    stored: Option<&str>,
    link_joins: &str,
) -> Option<(String, String)> {
    let (dim_expr, dim_join) = q::dimension_sql(x_axis)?;
    let (seg_select, seg_group, seg_join) = match segment {
        Some(seg) if !seg.is_empty() => {
            let (expr, join) = q::dimension_sql(seg)?;
            // Date segments render through the monthly `Concat`
            // (`analytics_plot.py:89-91`).
            let aliased = if q::is_date_axis(seg) {
                q::month_dimension_expr(seg)
            } else {
                expr
            };
            (
                format!(", {aliased} AS \"segment\""),
                ", \"segment\"".to_owned(),
                join,
            )
        }
        _ => (String::new(), String::new(), String::new()),
    };
    // One join per table: a dimension/segment join targeting `states`
    // (e.g. `state__group`) subsumes the manager `states` join (the
    // triage predicate applies over it), and textually identical joins
    // merge (segment can never equal `x_axis`, so this is defense).
    let with_states = !(dim_join.contains("\"states\"") || seg_join.contains("\"states\""));
    let seg_join = if seg_join == dim_join {
        String::new()
    } else {
        seg_join
    };
    let plot_joins = format!("{dim_join}{seg_join}{link_joins}");
    let scope = issue_where("$1", stored);
    // `queryset.count()`: the bare filtered scope — dimension/segment
    // joins stay out, or their fanout would inflate the total
    // (`base.py:216`). The count keeps the manager `states` join (the
    // plot's `with_states` only concerns the dimension/segment joins).
    let count_sql = format!("SELECT COUNT(*) {} WHERE ({scope})", issue_from(link_joins));
    let plot_sql = if y_axis == "estimate" {
        // `SUM(CAST(estimate_point__value AS float))` grouped by
        // dimension (`analytics_plot.py:110-115`). The estimate table
        // joins exactly once: through the dimension/segment when one of
        // them already brings it, otherwise through the base join.
        let (_, estimate_join) = q::dimension_sql("estimate_point__value")?;
        let already = dim_join.contains(q::ESTIMATE_POINT_TABLE)
            || seg_join.contains(q::ESTIMATE_POINT_TABLE);
        let base_join = if already {
            String::new()
        } else {
            estimate_join
        };
        let seg_join = if segment == Some("estimate_point__value") {
            String::new()
        } else {
            seg_join
        };
        let from = issue_from_states(
            &format!("{base_join}{dim_join}{seg_join}{link_joins}"),
            with_states,
        );
        format!(
            "SELECT {dim_expr} AS \"dimension\"{seg_select}, \
             SUM(CAST(\"estimate_points\".\"value\" AS DOUBLE PRECISION)) AS \"estimate\" \
             {from} WHERE ({scope}) GROUP BY \"dimension\"{seg_group} ORDER BY \"dimension\" ASC"
        )
    } else {
        // `COUNT(*)` grouped by dimension (+ segment),
        // (`analytics_plot.py:96-107`). Date axes exclude NULL
        // dimensions (`:85-86`); the guard tests the raw column — a
        // `CONCAT` expression is never NULL in Postgres (it skips null
        // inputs), so guarding the expression would keep NULL dates as
        // `'-'` buckets instead of excluding them.
        let null_guard = if q::is_date_axis(x_axis) {
            format!(" AND \"issues\".\"{x_axis}\" IS NOT NULL")
        } else {
            String::new()
        };
        let from = issue_from_states(&plot_joins, with_states);
        format!(
            "SELECT \"dimension\", COUNT(*) AS \"count\" FROM \
             (SELECT {dim_expr} AS \"dimension\"{seg_select} {from} WHERE ({scope}){null_guard}) \
             GROUP BY \"dimension\"{seg_group} ORDER BY \"dimension\" ASC"
        )
    };
    Some((count_sql, plot_sql))
}

/// `GET workspaces/<slug>/saved-analytic-view/<analytic_id>/`
/// (`base.py:190-222`). Gate first, then the
/// `AnalyticView.objects.get(pk, workspace__slug)` lookup (miss →
/// `ObjectDoesNotExist` 404), then axes validation from `query_dict`,
/// then the stored-scope count + plot.
async fn get_saved_analytic(
    State(state): State<AppState>,
    Path((slug, analytic_id)): Path<(String, String)>,
    Query(query): Query<QueryMap>,
    extension: Option<axum::Extension<SessionHandle>>,
    req: axum::extract::Request,
) -> Result<Response, IssuesDenial> {
    // Django's `<uuid:analytic_id>` converter rejects non-UUID segments
    // at routing time (HTML 404, never reaching the view). Axum path
    // captures match any segment, so non-UUID tails proxy to Django,
    // reproducing its routing 404 in every `DEBUG` setting.
    // Routing precedes auth, like the views-search detail routes.
    let view_id: uuid::Uuid = match analytic_id.parse() {
        Ok(id) => id,
        Err(_) => return Ok(crate::edge::proxy(State(state), req).await),
    };
    authed_gate(
        &state,
        &slug,
        "GET",
        "workspaces/<slug>/saved-analytic-view/<uuid>/",
        extension,
    )
    .await?;
    let pool = pool_owned(&state)?;
    // `AnalyticView.objects.get(pk=analytic_id, workspace__slug=slug)` —
    // the soft-delete-scoped default manager (`AuditModel` carries
    // `SoftDeleteModel`).
    let found: Option<(Option<Value>, Option<Value>)> = sqlx::query_as(
        "SELECT \"analytic_views\".\"query\", \"analytic_views\".\"query_dict\" \
         FROM \"analytic_views\" \
         INNER JOIN \"workspaces\" ON (\"analytic_views\".\"workspace_id\" = \"workspaces\".\"id\") \
         WHERE (\"analytic_views\".\"id\" = $1 AND \"workspaces\".\"slug\" = $2 \
         AND \"analytic_views\".\"deleted_at\" IS NULL) LIMIT 1",
    )
    .bind(view_id)
    .bind(&slug)
    .fetch_optional(&pool)
    .await
    .map_err(|_| IssuesDenial::ServerError)?;
    let (stored_query, query_dict) = found.ok_or(IssuesDenial::NotFound)?;
    // `analytic_view.query_dict.get(...)`: a NULL column is
    // `None.get` → `AttributeError` → generic 500.
    let query_dict = query_dict.ok_or(IssuesDenial::ServerError)?;
    let x_axis = query_dict.get("x_axis").and_then(Value::as_str);
    let y_axis = query_dict.get("y_axis").and_then(Value::as_str);
    match q::validate_base_axes(x_axis, y_axis, None) {
        Ok(()) => {}
        Err(q::AxisError::Axes) => {
            return Ok(json_value_response(
                StatusCode::BAD_REQUEST,
                &q::axes_error_body(),
            ));
        }
        Err(q::AxisError::Segment) => {
            return Ok(json_value_response(
                StatusCode::BAD_REQUEST,
                &q::segment_error_body(),
            ));
        }
    }
    let (x_axis, y_axis) = (x_axis.unwrap_or(""), y_axis.unwrap_or(""));
    // `segment` still comes from the request (`base.py:207`); empty is
    // falsy and skips the check, exactly like `validate_base_axes`.
    let segment = query_last(&query, "segment");
    match q::validate_base_axes(Some(x_axis), Some(y_axis), segment.as_deref()) {
        Ok(()) => {}
        Err(q::AxisError::Axes) => {
            return Ok(json_value_response(
                StatusCode::BAD_REQUEST,
                &q::axes_error_body(),
            ));
        }
        Err(q::AxisError::Segment) => {
            return Ok(json_value_response(
                StatusCode::BAD_REQUEST,
                &q::segment_error_body(),
            ));
        }
    }
    let segmented = segment.as_deref().is_some_and(|seg| !seg.is_empty());
    let mut binder = slugs_first(&slug);
    let stored = stored_filters(&stored_query.ok_or(IssuesDenial::ServerError)?, &mut binder)?;
    let (count_sql, plot_sql) =
        saved_statements(x_axis, y_axis, segment.as_deref(), stored.as_deref(), "")
            .ok_or(IssuesDenial::ServerError)?;
    let values = binder.values();
    let total = crate::app_issues::fetch_count(&pool, &count_sql, values.clone()).await?;
    let rows = fetch_json_rows(&pool, &plot_sql, values).await?;
    let value_key = if y_axis == "estimate" {
        "estimate"
    } else {
        "count"
    };
    let distribution = regroup_plot(&rows, x_axis, value_key, segmented);
    let mut body = Map::new();
    body.insert("total".to_owned(), Value::from(total));
    body.insert("distribution".to_owned(), distribution);
    Ok(json_value_response(StatusCode::OK, &Value::Object(body)))
}

// ---------------------------------------------------------------------------
// Export-analytics endpoint
// ---------------------------------------------------------------------------

/// Celery task enqueued by `ExportAnalyticsEndpoint.post`: the bare
/// `@shared_task` path (`analytic_plot_export.py:349-350`, D-09 owned —
/// the Rust worker forwards unregistered names to the broker, so the
/// endpoint only publishes).
const ANALYTIC_EXPORT_TASK: &str = "pi_dash.bgtasks.analytic_plot_export.analytic_export_task";

/// Best-effort deferred publish (the space-intake / views-search
/// precedent): without the queue the response still stands.
async fn enqueue_message(pool: &sqlx::PgPool, message: pidash_jobs::celery::CeleryTaskMessage) {
    let job = pidash_jobs::queue::NewJob::new(
        message.task.clone(),
        Value::Array(message.args.clone()),
        Value::Object(message.kwargs.clone()),
    );
    if let Err(error) = pidash_jobs::queue::enqueue(pool, &job).await {
        tracing::warn!(%error, task = message.task.as_str(), "task enqueue failed; response stands");
    }
}

/// `POST workspaces/<slug>/export-analytics/` (`base.py:223-251`).
/// Same axis validation as the analytics GET, then
/// `analytic_export_task.delay(email, data, slug)` and the emailed-to
/// acknowledgement. No queryset runs here (Q-02c).
async fn post_export_analytics(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    extension: Option<axum::Extension<SessionHandle>>,
    body: Option<axum::Json<Value>>,
) -> Result<Response, IssuesDenial> {
    let authed = authed_gate(
        &state,
        &slug,
        "POST",
        "workspaces/<slug>/export-analytics/",
        extension,
    )
    .await?;
    // `request.data` is the parsed body, `{}` when absent.
    let data = body.map(|body| body.0).unwrap_or(Value::Object(Map::new()));
    let x_axis = data.get("x_axis").and_then(Value::as_str);
    let y_axis = data.get("y_axis").and_then(Value::as_str);
    let segment = data.get("segment").and_then(Value::as_str);
    match q::validate_base_axes(x_axis, y_axis, segment) {
        Ok(()) => {}
        Err(q::AxisError::Axes) => {
            return Ok(json_value_response(
                StatusCode::BAD_REQUEST,
                &q::axes_error_body(),
            ));
        }
        Err(q::AxisError::Segment) => {
            return Ok(json_value_response(
                StatusCode::BAD_REQUEST,
                &q::segment_error_body(),
            ));
        }
    }
    // `str(request.user.email)`: a NULL email prints `"None"`.
    let email = authed.email.as_deref().unwrap_or("None");
    let pool = pool_owned(&state)?;
    let mut kwargs = Map::new();
    kwargs.insert("email".to_owned(), Value::from(email));
    kwargs.insert("data".to_owned(), data);
    kwargs.insert("slug".to_owned(), Value::from(slug));
    enqueue_message(
        &pool,
        pidash_jobs::celery::CeleryTaskMessage::new(ANALYTIC_EXPORT_TASK, vec![], kwargs),
    )
    .await;
    Ok(json_value_response(
        StatusCode::OK,
        &q::base_export_message(email),
    ))
}

// ---------------------------------------------------------------------------
// Default-analytics endpoint
// ---------------------------------------------------------------------------

/// Avatar-url `Case` over an already-joined `"users"` table, shared by
/// the three user aggregations (`base.py:296-311,327-342,352-367`).
/// Asset id present → `/api/assets/v2/static/<asset>/`, asset null →
/// the plain `avatar` column, else SQL `NULL` — which is why the seed
/// renders `""` for the created user (empty-string avatar) but `NULL`
/// for the pending bucket (no join row at all).
fn avatar_case(out_alias: &str) -> String {
    q::avatar_case_sql("\"users\"", out_alias)
}

/// `GET workspaces/<slug>/default-analytics/` (`base.py:252-390`).
/// Nine reads over one base scope (`issue_objects` + slug +
/// `issue_filters(GET)`): totals, classified totals, open count +
/// classified, completed-month-wise (current UTC year), top-5 creators,
/// top-5 closers, pending assignees (no limit), and the two estimate
/// sums. Key order follows the Python response dict (`:375-386`).
async fn get_default_analytics(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    Query(query): Query<QueryMap>,
    extension: Option<axum::Extension<SessionHandle>>,
) -> Result<Response, IssuesDenial> {
    authed_gate(
        &state,
        &slug,
        "GET",
        "workspaces/<slug>/default-analytics/",
        extension,
    )
    .await?;
    let pool = pool_owned(&state)?;

    let mut binder = slugs_first(&slug);
    let legacy = legacy_filters(&query, &mut binder)?;
    let scope = issue_where("$1", legacy.as_deref());
    // Link joins per statement family: the base family carries the full
    // set, while the assignee family already joins `issue_assignees`
    // and skips it (one join per table).
    let legacy_joins = link_joins_for(legacy.as_deref().unwrap_or(""), "");

    // Total + classified (base.py:258-264).
    let from = issue_from(&legacy_joins);
    let total_sql = format!("SELECT COUNT(*) {from} WHERE ({scope})");
    let classified_sql = format!(
        "SELECT \"states\".\"group\" AS \"state_group\", COUNT(\"states\".\"group\") AS \"state_count\" \
         {from} WHERE ({scope}) GROUP BY \"states\".\"group\" ORDER BY \"state_group\" ASC"
    );
    // Open scope (base.py:266-273): same shape over OPEN_STATE_GROUPS.
    let open_filter = format!(
        " AND \"states\".\"group\" IN ({})",
        q::group_list(&q::open_state_groups())
    );
    let open_scope = format!("{scope}{open_filter}");
    let open_sql = format!("SELECT COUNT(*) {from} WHERE ({open_scope})");
    let open_classified_sql = format!(
        "SELECT \"states\".\"group\" AS \"state_group\", COUNT(\"states\".\"group\") AS \"state_count\" \
         {from} WHERE ({open_scope}) GROUP BY \"states\".\"group\" ORDER BY \"state_group\" ASC"
    );
    // Completed month-wise, current UTC year only (base.py:275-282).
    let year = chrono::Utc::now().year();
    let year_holder = binder.bind(sea_query::Value::Int(Some(year)));
    let month_sql = format!(
        "SELECT EXTRACT(MONTH FROM \"issues\".\"completed_at\")::INTEGER AS \"month\", COUNT(*) AS \"count\" \
         {from} WHERE ({scope} AND EXTRACT(YEAR FROM \"issues\".\"completed_at\") = {year_holder}) \
         GROUP BY \"month\" ORDER BY \"month\" ASC"
    );
    // Top-5 creators (base.py:291-313): NULL creators excluded; avatar
    // `Case`; `ORDER BY count DESC LIMIT 5`.
    let creator_cols = q::CREATED_BY_DETAILS
        .iter()
        .map(|alias| {
            let col = alias.strip_prefix("created_by__").unwrap_or(alias);
            format!("\"users\".\"{col}\" AS \"{alias}\"")
        })
        .collect::<Vec<_>>()
        .join(", ");
    let creator_group = q::CREATED_BY_DETAILS
        .iter()
        .map(|alias| {
            let col = alias.strip_prefix("created_by__").unwrap_or(alias);
            format!("\"users\".\"{col}\"")
        })
        .collect::<Vec<_>>()
        .join(", ");
    let creator_from = issue_from(&format!(
        "{legacy_joins} INNER JOIN \"users\" ON (\"issues\".\"created_by_id\" = \"users\".\"id\")"
    ));
    let creators_sql = format!(
        "SELECT {creator_cols}, COUNT(\"issues\".\"id\") AS \"count\", {} \
         {creator_from} WHERE ({scope} AND \"issues\".\"created_by_id\" IS NOT NULL) \
         GROUP BY {creator_group} ORDER BY \"count\" DESC LIMIT 5",
        avatar_case("created_by__avatar_url")
    );
    // Top-5 closers (base.py:322-345): completed only, NULL assignees
    // excluded. Pending (base.py:347-369): open only, same shape, no
    // limit. Both traverse the m2m assignee join, hence LEFT JOINs (a
    // missing assignee is the NULL pending bucket, not a dropped row).
    let assignee_cols = q::ASSIGNEE_DETAILS
        .iter()
        .map(|alias| {
            let col = alias.strip_prefix("assignees__").unwrap_or(alias);
            format!("\"users\".\"{col}\" AS \"{alias}\"")
        })
        .collect::<Vec<_>>()
        .join(", ");
    let assignee_group = q::ASSIGNEE_DETAILS
        .iter()
        .map(|alias| {
            let col = alias.strip_prefix("assignees__").unwrap_or(alias);
            format!("\"users\".\"{col}\"")
        })
        .collect::<Vec<_>>()
        .join(", ");
    let assignee_joins =
        " LEFT JOIN \"issue_assignees\" ON (\"issue_assignees\".\"issue_id\" = \"issues\".\"id\") \
         LEFT JOIN \"users\" ON (\"issue_assignees\".\"assignee_id\" = \"users\".\"id\")";
    let assignee_legacy_joins = link_joins_for(legacy.as_deref().unwrap_or(""), assignee_joins);
    let assignee_from = issue_from(&format!("{assignee_legacy_joins}{assignee_joins}"));
    let closer_avatar = avatar_case("assignees__avatar_url");
    let closers_sql = format!(
        "SELECT {assignee_cols}, {closer_avatar}, COUNT(\"issues\".\"id\") AS \"count\" \
         {assignee_from} WHERE ({scope} AND \"issues\".\"completed_at\" IS NOT NULL \
         AND \"users\".\"id\" IS NOT NULL) \
         GROUP BY {assignee_group} ORDER BY \"count\" DESC LIMIT 5"
    );
    let pending_sql = format!(
        "SELECT {assignee_cols}, COUNT(\"issues\".\"id\") AS \"count\", {} \
         {assignee_from} WHERE ({scope} AND \"issues\".\"completed_at\" IS NULL) \
         GROUP BY {assignee_group} ORDER BY \"count\" DESC",
        avatar_case("assignees__avatar_url")
    );
    // Estimate sums (base.py:371-372): ported bug B1 keeps
    // `SUM("issues"."point")`. `NULL` (not 0) when no rows match.
    let est_open_sql =
        format!("SELECT SUM(\"issues\".\"point\") AS \"sum\" {from} WHERE ({open_scope})");
    let est_total_sql =
        format!("SELECT SUM(\"issues\".\"point\") AS \"sum\" {from} WHERE ({scope})");

    let values = binder.values();
    let total = crate::app_issues::fetch_count(&pool, &total_sql, values.clone()).await?;
    let classified = fetch_json_rows(&pool, &classified_sql, values.clone()).await?;
    let open = crate::app_issues::fetch_count(&pool, &open_sql, values.clone()).await?;
    let open_classified = fetch_json_rows(&pool, &open_classified_sql, values.clone()).await?;
    let monthwise = fetch_json_rows(&pool, &month_sql, values.clone()).await?;
    let creators = fetch_json_rows(&pool, &creators_sql, values.clone()).await?;
    let closers = fetch_json_rows(&pool, &closers_sql, values.clone()).await?;
    let pending = fetch_json_rows(&pool, &pending_sql, values.clone()).await?;
    let est_open = fetch_json_rows(&pool, &est_open_sql, values.clone()).await?;
    let est_total = fetch_json_rows(&pool, &est_total_sql, values).await?;

    let rows_of =
        |rows: Vec<Map<String, Value>>| Value::Array(rows.into_iter().map(Value::Object).collect());
    let single_sum = |rows: Vec<Map<String, Value>>| {
        rows.into_iter()
            .next()
            .and_then(|mut row| row.remove("sum"))
            .unwrap_or(Value::Null)
    };
    let mut body = Map::new();
    body.insert("total_issues".to_owned(), Value::from(total));
    body.insert("total_issues_classified".to_owned(), rows_of(classified));
    body.insert("open_issues".to_owned(), Value::from(open));
    body.insert(
        "open_issues_classified".to_owned(),
        rows_of(open_classified),
    );
    body.insert("issue_completed_month_wise".to_owned(), rows_of(monthwise));
    body.insert("most_issue_created_user".to_owned(), rows_of(creators));
    body.insert("most_issue_closed_user".to_owned(), rows_of(closers));
    body.insert("pending_issue_user".to_owned(), rows_of(pending));
    body.insert("open_estimate_sum".to_owned(), single_sum(est_open));
    body.insert("total_estimate_sum".to_owned(), single_sum(est_total));
    Ok(json_value_response(StatusCode::OK, &Value::Object(body)))
}

// ---------------------------------------------------------------------------
// Project-stats endpoint
// ---------------------------------------------------------------------------

/// `GET workspaces/<slug>/project-stats/` (`base.py:391-455`).
/// `?fields=` intersects the five valid fields (empty/unknown → all
/// five, [`q::project_stats_fields`]); `?project_ids=` narrows by id
/// (entries verbatim, malformed → generic 500, ported bug B7). Each
/// annotation is a correlated scalar subquery; the issue subqueries
/// carry the `issue_objects` manager scope (triage/archived/draft
/// exclusions) that the fixture template sketches omit — Python
/// semantics win, and the seed world is clean so the suite cannot tell.
async fn get_project_stats(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    Query(query): Query<QueryMap>,
    extension: Option<axum::Extension<SessionHandle>>,
) -> Result<Response, IssuesDenial> {
    authed_gate(
        &state,
        &slug,
        "GET",
        "workspaces/<slug>/project-stats/",
        extension,
    )
    .await?;
    let pool = pool_owned(&state)?;

    let fields_csv = query_last(&query, "fields").unwrap_or_default();
    let fields = q::project_stats_fields(&fields_csv);
    let ids_csv = query_last(&query, "project_ids").unwrap_or_default();

    let mut binder = slugs_first(&slug);
    // `id__in` from the verbatim csv split (`base.py:411`).
    let mut id_filter = String::new();
    if !ids_csv.is_empty() {
        let mut holders = Vec::new();
        for raw in ids_csv.split(',') {
            let id: uuid::Uuid = raw.parse().map_err(|_| IssuesDenial::ServerError)?;
            holders.push(binder.bind(sea_query::Value::Uuid(Some(Box::new(id)))));
        }
        id_filter = format!(" AND \"projects\".\"id\" IN ({})", holders.join(","));
    }

    let manager = "U0.\"deleted_at\" IS NULL AND S0.\"group\" != 'triage' \
         AND U0.\"archived_at\" IS NULL AND P0.\"archived_at\" IS NULL AND U0.\"is_draft\" = FALSE";
    let issue_scope = |extra: &str| {
        format!(
            "U0.\"project_id\" = \"projects\".\"id\" AND {manager}{extra}",
            manager = manager,
            extra = extra
        )
    };
    let mut selects = vec!["\"projects\".\"id\"".to_owned()];
    for field in &fields {
        let sub = match *field {
            "total_issues" => format!(
                "(SELECT COUNT(U0.\"id\") FROM \"issues\" U0 \
                 INNER JOIN \"states\" S0 ON (U0.\"state_id\" = S0.\"id\") \
                 INNER JOIN \"projects\" P0 ON (U0.\"project_id\" = P0.\"id\") \
                 WHERE ({})) AS \"total_issues\"",
                issue_scope("")
            ),
            "completed_issues" => format!(
                "(SELECT COUNT(U0.\"id\") FROM \"issues\" U0 \
                 INNER JOIN \"states\" S0 ON (U0.\"state_id\" = S0.\"id\") \
                 INNER JOIN \"projects\" P0 ON (U0.\"project_id\" = P0.\"id\") \
                 WHERE ({} AND S0.\"group\" IN ({}))) AS \"completed_issues\"",
                issue_scope(""),
                q::group_list(&q::closed_state_groups())
            ),
            "total_cycles" => "(SELECT COUNT(U0.\"id\") FROM \"cycles\" U0 \
                 WHERE (U0.\"project_id\" = \"projects\".\"id\" AND U0.\"deleted_at\" IS NULL)) \
                 AS \"total_cycles\""
                .to_owned(),
            "total_modules" => "(SELECT COUNT(U0.\"id\") FROM \"modules\" U0 \
                 WHERE (U0.\"project_id\" = \"projects\".\"id\" AND U0.\"deleted_at\" IS NULL)) \
                 AS \"total_modules\""
                .to_owned(),
            // `member__is_bot=False, is_active=True` (`base.py:448`).
            // Spelled inline rather than through
            // [`q::project_stats_member_where`]: that atom quotes the
            // alias (`"U0"."is_active"`), but an unquoted `U0` alias
            // folds to lowercase, so the quoted reference misses the
            // `FROM` entry and the statement fails to plan. (Flagged
            // for the queries-layer owner; the services crate is
            // read-only for port agents.)
            "total_members" => "(SELECT COUNT(U0.\"id\") FROM \"project_members\" U0 \
                 INNER JOIN \"users\" ON (U0.\"member_id\" = \"users\".\"id\") \
                 WHERE (U0.\"project_id\" = \"projects\".\"id\" AND NOT \"users\".\"is_bot\" \
                 AND U0.\"is_active\" AND U0.\"deleted_at\" IS NULL)) \
                 AS \"total_members\""
                .to_owned(),
            _ => continue,
        };
        selects.push(sub);
    }
    // `Project.objects.filter(workspace__slug)` is soft-delete scoped
    // (`SoftDeletionManager`); the fixture sketch omits the guard.
    let sql = format!(
        "SELECT {} FROM \"projects\" \
         INNER JOIN \"workspaces\" ON (\"projects\".\"workspace_id\" = \"workspaces\".\"id\") \
         WHERE (\"workspaces\".\"slug\" = $1 AND \"projects\".\"deleted_at\" IS NULL{id_filter})",
        selects.join(", ")
    );
    let rows = fetch_json_rows(&pool, &sql, binder.values()).await?;
    Ok(json_value_response(
        StatusCode::OK,
        &Value::Array(rows.into_iter().map(Value::Object).collect()),
    ))
}

// DRF parity rendering for the D-35 handlers-A family (PIDASHCONV-389).
//
// - `build_graph_plot` regroup (`utils/analytics_plot.py:117-120`): rows
//   arrive ordered by dimension; Python `groupby` groups consecutive rows
//   by `str(dimension)`, then `sort_data` orders the keys (priority axes
//   sort low/medium/high/urgent/none with missing keys dropped, every
//   other axis sorts with `'none'` last).
// - Estimate sums render as JSON numbers; a NULL sum renders `null`
//   (Python `None` in the row dict). Non-finite floats cannot come back
//   from Postgres here, but map to `null` rather than erroring.
// - Envelope key order is DRF `.values()`/dict order, preserved through
//   `serde_json` (`preserve_order`): `total, distribution, extras` with
//   `state_details, assignee_details, label_details, cycle_details,
//   module_details`; buckets carry `dimension, [segment,] count|estimate`.

use pidash_services::app_analytics::queries::sort_data_keys;

/// Regrouped buckets: keys in first-seen (row) order plus each key's
/// bucket list. `serde_json::Map` has no `entry` API, so grouping runs
/// over a plain map and only the ordered assembly below touches `Map`.
struct Grouped {
    order: Vec<String>,
    buckets: HashMap<String, Vec<Value>>,
}

impl Grouped {
    fn new() -> Self {
        Self {
            order: Vec::new(),
            buckets: HashMap::new(),
        }
    }

    /// Push one row: the bucket renders `dimension, [segment,] value` in
    /// `.values()` order (`dimension` first — it is inserted before the
    /// caller-supplied tail). `has_segment` says whether a segment axis is
    /// active: without one the key stays absent; with one a NULL segment
    /// renders an explicit `null` (Django keeps the key with `None`).
    /// A NULL dimension always renders `null` (the row dict holds `None`);
    /// only the grouping key is `str(None)` (`"None"`).
    fn push(
        &mut self,
        dimension: Option<String>,
        segment: Option<String>,
        has_segment: bool,
        value: (String, Value),
    ) {
        let key = dimension.clone().unwrap_or_else(|| "None".to_owned());
        let mut bucket = Map::new();
        bucket.insert(
            "dimension".to_owned(),
            dimension.map(Value::String).unwrap_or(Value::Null),
        );
        if has_segment {
            bucket.insert(
                "segment".to_owned(),
                segment.map(Value::String).unwrap_or(Value::Null),
            );
        }
        bucket.insert(value.0, value.1);
        if !self.buckets.contains_key(&key) {
            self.order.push(key.clone());
            self.buckets.insert(key.clone(), Vec::new());
        }
        if let Some(items) = self.buckets.get_mut(&key) {
            items.push(Value::Object(bucket));
        }
    }

    fn into_map(self) -> Map<String, Value> {
        let mut out = Map::new();
        for key in &self.order {
            if let Some(items) = self.buckets.get(key) {
                out.insert(key.clone(), Value::Array(items.clone()));
            }
        }
        out
    }
}

/// One `issue_count` plot row: `(dimension, segment, count)`. Dimensions
/// group under `str(dimension)` — `None` under `"None"` (Python
/// `str(None)`). `has_segment` threads through to [`Grouped::push`].
pub fn distribution_from_count_rows(
    rows: Vec<(Option<String>, Option<String>, i64)>,
    x_axis: &str,
    has_segment: bool,
) -> Map<String, Value> {
    let mut grouped = Grouped::new();
    for (dimension, segment, count) in rows {
        grouped.push(
            dimension,
            segment,
            has_segment,
            ("count".to_owned(), Value::from(count)),
        );
    }
    order_distribution(grouped.into_map(), x_axis)
}

/// One `estimate` plot row: `(dimension, segment, estimate)`.
pub fn distribution_from_estimate_rows(
    rows: Vec<(Option<String>, Option<String>, Option<f64>)>,
    x_axis: &str,
    has_segment: bool,
) -> Map<String, Value> {
    let mut grouped = Grouped::new();
    for (dimension, segment, estimate) in rows {
        grouped.push(
            dimension,
            segment,
            has_segment,
            ("estimate".to_owned(), render_estimate(estimate)),
        );
    }
    order_distribution(grouped.into_map(), x_axis)
}

/// Apply `sort_data` key order to a grouped distribution, exactly like
/// Python (`analytics_plot.py:64-70`): the priority path keeps only
/// `low/medium/high/urgent/none` keys present in the data (anything else —
/// e.g. the `'None'` NULL bucket — is dropped); every other axis sorts all
/// keys with `'none'` last.
fn order_distribution(grouped: Map<String, Value>, x_axis: &str) -> Map<String, Value> {
    let keys: Vec<String> = grouped.keys().cloned().collect();
    let ordered = sort_data_keys(&keys, x_axis);
    let mut out = Map::new();
    for key in &ordered {
        if let Some(items) = grouped.get(key) {
            out.insert(key.clone(), items.clone());
        }
    }
    out
}

/// Render one estimate sum: JSON number, or `null` for NULL.
pub fn render_estimate(estimate: Option<f64>) -> Value {
    match estimate {
        Some(value) => serde_json::Number::from_f64(value)
            .map(Value::Number)
            .unwrap_or(Value::Null),
        None => Value::Null,
    }
}

/// The `extras` envelope: `state_details, assignee_details, label_details,
/// cycle_details, module_details` in `base.py:165-171` order. Each arm is
/// `{}` (empty object, not `[]`) when its axis does not apply.
pub fn extras_envelope(
    state_details: Option<Vec<Map<String, Value>>>,
    assignee_details: Option<Vec<Map<String, Value>>>,
    label_details: Option<Vec<Map<String, Value>>>,
    cycle_details: Option<Vec<Map<String, Value>>>,
    module_details: Option<Vec<Map<String, Value>>>,
) -> Map<String, Value> {
    fn arm(rows: Option<Vec<Map<String, Value>>>) -> Value {
        match rows {
            Some(rows) => Value::Array(rows.into_iter().map(Value::Object).collect()),
            None => Value::Object(Map::new()),
        }
    }
    let mut extras = Map::new();
    extras.insert("state_details".to_owned(), arm(state_details));
    extras.insert("assignee_details".to_owned(), arm(assignee_details));
    extras.insert("label_details".to_owned(), arm(label_details));
    extras.insert("cycle_details".to_owned(), arm(cycle_details));
    extras.insert("module_details".to_owned(), arm(module_details));
    extras
}

/// The full analytics envelope in `base.py:161-174` key order.
pub fn analytics_envelope(
    total: i64,
    distribution: Map<String, Value>,
    extras: Map<String, Value>,
) -> String {
    let mut body = Map::new();
    body.insert("total".to_owned(), Value::from(total));
    body.insert("distribution".to_owned(), Value::Object(distribution));
    body.insert("extras".to_owned(), Value::Object(extras));
    serde_json::to_string(&Value::Object(body)).expect("analytics envelope")
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use serde_json::json;

    fn utc(y: i32, m: u32, d: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, m, d, 12, 0, 0).unwrap()
    }

    #[test]
    fn gate_paths_match_gate_table() {
        // The handler checks the exact gate-table rows (FX-A-G-01); a rename
        // on either side fails here instead of silently proxying.
        for path in [PATH_ADVANCE, PATH_STATS, PATH_CHARTS] {
            let row = super::super::gates::gate_for("GET", path).expect("gate row");
            assert!(
                matches!(row.gate, super::super::gates::Gate::Workspace { .. }),
                "{path}"
            );
        }
        assert_eq!(
            super::super::gates::deny_body(&super::super::gates::Gate::Workspace { roles: &[] }),
            FORBIDDEN_BODY
        );
    }

    #[test]
    fn denial_bodies_are_byte_exact() {
        assert_eq!(
            UNAUTHENTICATED_BODY,
            r#"{"detail":"Authentication credentials were not provided."}"#
        );
        assert_eq!(
            FORBIDDEN_BODY,
            r#"{"error":"You don't have the required permissions."}"#
        );
        assert_eq!(
            NOT_FOUND_BODY,
            r#"{"error":"The required object does not exist."}"#
        );
        assert_eq!(
            INVALID_DETAIL_BODY,
            r#"{"error":"Please provide valid detail"}"#
        );
        assert_eq!(q::INVALID_TAB_BODY, r#"{"message": "Invalid tab"}"#);
        assert_eq!(q::INVALID_TYPE_BODY, r#"{"message": "Invalid type"}"#);
        assert_eq!(
            Denial::InvalidTab.status_and_body().1,
            r#"{"message":"Invalid tab"}"#.to_owned()
        );
        assert_eq!(
            Denial::InvalidType.status_and_body().1,
            r#"{"message":"Invalid type"}"#.to_owned()
        );
        assert_eq!(INVALID_TAB_BODY, r#"{"message":"Invalid tab"}"#);
        assert_eq!(INVALID_TYPE_BODY, r#"{"message":"Invalid type"}"#);
        assert_eq!(
            invalid_axis_body("Invalid x_axis field: NOPE"),
            r#"["Invalid x_axis field: NOPE"]"#
        );
    }

    #[test]
    fn date_ranges_follow_date_utils() {
        let now = utc(2026, 9, 30);
        // Unknown/absent names answer None (the docstring default is not code).
        assert!(analytics_date_range(None, now).is_none());
        assert!(analytics_date_range(Some("nope"), now).is_none());
        assert!(chart_period_range(None, now).is_none());
        assert!(chart_period_range(Some("nope"), now).is_none());
        // Named ranges: last_7_days covers today-7..today, datetimes at
        // midnight / end-of-day (datetime.min/max.time()).
        let (gte, lte) = analytics_date_range(Some("last_7_days"), now).unwrap();
        assert_eq!(
            gte.format("%Y-%m-%d %H:%M:%S").to_string(),
            "2026-09-23 00:00:00"
        );
        assert_eq!(
            lte.format("%Y-%m-%d %H:%M:%S%.6f").to_string(),
            "2026-09-30 23:59:59.999999"
        );
        let (start, end) = chart_period_range(Some("last_30_days"), now).unwrap();
        assert_eq!(
            (start.to_string(), end.to_string()),
            ("2026-08-31".to_owned(), "2026-09-30".to_owned())
        );
        let (day, same) = chart_period_range(Some("yesterday"), now).unwrap();
        assert_eq!(
            (day.to_string(), same.to_string()),
            ("2026-09-29".to_owned(), "2026-09-29".to_owned())
        );
    }

    #[test]
    fn all_thirteen_axes_resolve_case_sensitively() {
        for name in [
            "STATES",
            "STATE_GROUPS",
            "LABELS",
            "ASSIGNEES",
            "ESTIMATE_POINTS",
            "CYCLES",
            "MODULES",
            "PRIORITY",
            "START_DATE",
            "TARGET_DATE",
            "CREATED_AT",
            "COMPLETED_AT",
            "CREATED_BY",
        ] {
            assert!(axis_for(name).is_some(), "{name}");
        }
        assert!(axis_for("priority").is_none());
        assert!(axis_for("").is_none());
        // Deleted guards ride the axis scope (build_chart.py additional filters).
        assert!(axis_for("LABELS").unwrap().filter.contains("issue_labels"));
        assert!(axis_for("ASSIGNEES")
            .unwrap()
            .filter
            .contains("issue_assignees"));
        assert!(axis_for("CYCLES").unwrap().filter.contains("cycle_issues"));
        assert!(axis_for("MODULES")
            .unwrap()
            .filter
            .contains("module_issues"));
        assert_eq!(axis_for("PRIORITY").unwrap().filter, "");
        assert_eq!(axis_for("STATES").unwrap().key_kind, KeyKind::Uuid);
        assert_eq!(axis_for("ESTIMATE_POINTS").unwrap().key_kind, KeyKind::Int);
        assert_eq!(axis_for("START_DATE").unwrap().key_kind, KeyKind::Date);
    }

    #[test]
    fn key_rendering_keeps_types_and_falsy_rules() {
        // Simple path: falsy (None, "", 0) renders "None"; ints stay numbers.
        assert_eq!(KeyVal::Null.simple_json(), Value::from("None"));
        assert_eq!(
            KeyVal::Str(String::new()).simple_json(),
            Value::from("None")
        );
        assert_eq!(KeyVal::Int(0).simple_json(), Value::from("None"));
        assert_eq!(KeyVal::Int(3).simple_json(), Value::from(3));
        assert_eq!(
            KeyVal::Str("high".to_owned()).simple_json(),
            Value::from("high")
        );
        assert_eq!(
            KeyVal::Date(NaiveDate::from_ymd_opt(2026, 9, 1).unwrap()).simple_json(),
            Value::from("2026-09-01")
        );
        let id = uuid::Uuid::nil();
        assert_eq!(KeyVal::Uuid(id).simple_json(), Value::from(id.to_string()));
        // Grouped path uses lowercase "none" for the row key; schema names
        // keep "None".
        assert!(KeyVal::Null.is_falsy());
        assert!(!KeyVal::Uuid(id).is_falsy());
        assert_eq!(KeyVal::Int(3).raw_string(), "3");
    }

    #[test]
    fn grouped_key_and_name_follow_process_grouped_data() {
        // Grouped row key: falsy renders lowercase "none", truthy keeps its
        // JSON type (grouped estimate keys are numbers, as DRF renders them).
        assert_eq!(KeyVal::Null.grouped_key_json(), Value::from("none"));
        assert_eq!(
            KeyVal::Str(String::new()).grouped_key_json(),
            Value::from("none")
        );
        assert_eq!(KeyVal::Int(0).grouped_key_json(), Value::from("none"));
        assert_eq!(KeyVal::Int(3).grouped_key_json(), Value::from(3));
        assert_eq!(
            KeyVal::Str("high".to_owned()).grouped_key_json(),
            Value::from("high")
        );
        // Grouped row name: display_name or "None" — never the key.
        assert_eq!(
            KeyVal::Str("AN Backlog".to_owned()).grouped_name_json(),
            Value::from("AN Backlog")
        );
        assert_eq!(KeyVal::Null.grouped_name_json(), Value::from("None"));
        assert_eq!(
            KeyVal::Str(String::new()).grouped_name_json(),
            Value::from("None")
        );
        // Grouped schema entry: group_name or "None" — never the group key
        // (an empty estimate value schemas as "None", not "5").
        assert_eq!(
            grouped_schema_json(&KeyVal::Str("3pts".to_owned())),
            Value::from("3pts")
        );
        assert_eq!(
            grouped_schema_json(&KeyVal::Str(String::new())),
            Value::from("None")
        );
        assert_eq!(grouped_schema_json(&KeyVal::Int(5)), Value::from(5));
        assert_eq!(grouped_schema_json(&KeyVal::Null), Value::from("None"));
    }

    #[test]
    fn chart_joins_merge_repeated_axis_joins() {
        let labels = axis_for("LABELS").unwrap();
        let created_by = axis_for("CREATED_BY").unwrap();
        let assignees = axis_for("ASSIGNEES").unwrap();
        // Self-grouping reuses the single Django join (a repeated alias is a
        // Postgres error where Django serves 200).
        let self_grouped = chart_joins(&labels, Some(&labels));
        assert_eq!(self_grouped.matches(labels.joins).count(), 1);
        assert!(self_grouped.contains(ISSUE_JOINS));
        // Distinct axes concatenate.
        let mixed = chart_joins(&labels, Some(&created_by));
        assert!(mixed.contains("issue_labels"));
        assert!(mixed.contains("ax_created_users"));
        // The two users axes no longer share an alias.
        let users = chart_joins(&assignees, Some(&created_by));
        assert!(users.contains("ax_users"));
        assert!(users.contains("ax_created_users"));
        // No group: axis joins only.
        assert_eq!(
            chart_joins(&labels, None),
            format!("{ISSUE_JOINS}{}", labels.joins)
        );
    }

    #[test]
    fn project_ids_validate_like_the_uuid_coercion() {
        let good = FilterCtx {
            project_ids: vec!["00000000-0000-0000-0000-000000000000".to_owned()],
            window: None,
            period: None,
        };
        assert!(good.checked_id_list().is_ok());
        assert!(good
            .project_narrowing("\"issues\".\"project_id\"")
            .unwrap()
            .contains("IN ("));
        let empty = FilterCtx {
            project_ids: Vec::new(),
            window: None,
            period: None,
        };
        assert_eq!(
            empty
                .project_narrowing("\"issues\".\"project_id\"")
                .unwrap(),
            ""
        );
        let bad = FilterCtx {
            project_ids: vec!["nope".to_owned()],
            window: None,
            period: None,
        };
        assert!(matches!(bad.checked_id_list(), Err(Denial::InvalidDetail)));
    }

    #[test]
    fn join_splice_and_chart_key_order() {
        let shell = q::advance_filtered_count_sql("issues", "TRUE", None);
        let spliced = with_joins(shell, "issues", " JOIN x ON TRUE");
        assert!(spliced.contains("FROM \"issues\" JOIN x ON TRUE WHERE (TRUE)"));
        // The window column qualifies per table once joined (Django always
        // qualifies; bare is `ambiguous column`).
        let windowed = with_joins(
            q::advance_filtered_count_sql("issues", "TRUE", Some(("$3", "$4"))),
            "issues",
            " JOIN x ON TRUE",
        );
        assert!(windowed.contains("AND \"issues\".\"created_at\" >= $3"));
        assert!(windowed.contains("AND \"issues\".\"created_at\" <= $4"));
        assert!(!windowed.contains("AND \"created_at\" >="));
        // The seven project-chart keys render in builder order.
        assert_eq!(
            q::ADVANCE_PROJECT_CHART_KEYS,
            &[
                "work_items",
                "cycles",
                "modules",
                "intake",
                "members",
                "pages",
                "views"
            ]
        );
        assert_eq!(q::chart_key_name("work_items"), "Work Items");
        // Overview / work-items key orders are the response orders.
        assert_eq!(q::ADVANCE_OVERVIEW_KEYS[0], "total_users");
        assert_eq!(q::ADVANCE_OVERVIEW_KEYS.len(), 11);
        assert_eq!(q::ADVANCE_WORK_ITEM_KEYS.len(), 5);
    }

    fn row(pairs: &[(&str, Value)]) -> Map<String, Value> {
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), value.clone()))
            .collect()
    }

    #[test]
    fn routes_cover_all_four_gated_paths() {
        // Every handlers-B path has its FX-A-G-01 gate row.
        for (method, path) in [
            ("GET", "workspaces/<slug>/saved-analytic-view/<uuid>/"),
            ("POST", "workspaces/<slug>/export-analytics/"),
            ("GET", "workspaces/<slug>/default-analytics/"),
            ("GET", "workspaces/<slug>/project-stats/"),
        ] {
            assert!(
                gate_for(method, path).is_some(),
                "missing gate row: {method} {path}"
            );
        }
        assert_eq!(
            ANALYTIC_EXPORT_TASK,
            "pi_dash.bgtasks.analytic_plot_export.analytic_export_task"
        );
    }

    #[test]
    fn py_str_matches_python_str_for_scalars() {
        assert_eq!(py_str(&Value::Null), "None");
        assert_eq!(py_str(&json!(true)), "True");
        assert_eq!(py_str(&json!(false)), "False");
        assert_eq!(py_str(&json!("high")), "high");
        assert_eq!(py_str(&json!(3)), "3");
    }

    #[test]
    fn regroup_issue_count_matches_seed_distribution() {
        // Seed world: one issue per priority bucket (FX-A-H-01 saved pair).
        let rows = vec![
            row(&[("dimension", json!("high")), ("count", json!(1))]),
            row(&[("dimension", json!("medium")), ("count", json!(1))]),
            row(&[("dimension", json!("urgent")), ("count", json!(1))]),
        ];
        assert_eq!(
            regroup_plot(&rows, "priority", "count", false),
            json!({
                "high": [{"dimension": "high", "count": 1}],
                "medium": [{"dimension": "medium", "count": 1}],
                "urgent": [{"dimension": "urgent", "count": 1}],
            })
        );
    }

    #[test]
    fn regroup_drops_null_bucket_on_priority() {
        // The dead-annotation quirk: NULL dimensions group as "None"
        // and `sort_data` drops the key on priority axes.
        let rows = vec![
            row(&[("dimension", Value::Null), ("count", json!(2))]),
            row(&[("dimension", json!("high")), ("count", json!(1))]),
        ];
        assert_eq!(
            regroup_plot(&rows, "priority", "count", false),
            json!({"high": [{"dimension": "high", "count": 1}]})
        );
    }

    #[test]
    fn regroup_segmented_items_carry_segment_key() {
        let rows = vec![
            row(&[
                ("dimension", json!("high")),
                ("segment", json!("backlog")),
                ("count", json!(1)),
            ]),
            row(&[
                ("dimension", json!("high")),
                ("segment", json!("completed")),
                ("count", json!(2)),
            ]),
        ];
        assert_eq!(
            regroup_plot(&rows, "priority", "count", true),
            json!({
                "high": [
                    {"dimension": "high", "segment": "backlog", "count": 1},
                    {"dimension": "high", "segment": "completed", "count": 2},
                ]
            })
        );
    }

    #[test]
    fn regroup_empty_is_empty_object() {
        assert_eq!(regroup_plot(&[], "priority", "count", false), json!({}));
    }

    #[test]
    fn remap_legacy_rewrites_d26_aliases() {
        // The D-26 renderer emits Django's aliases with unquoted
        // columns; the remap rewrites the table qualifier only (both
        // spellings are the same SQL semantics).
        assert_eq!(
            remap_legacy("issue.priority IN ($2)"),
            "\"issues\".priority IN ($2)"
        );
        assert_eq!(
            remap_legacy("label_issue.label_id IN ($2)"),
            "\"issue_labels\".label_id IN ($2)"
        );
        assert_eq!(
            remap_legacy("state.\"group\" IN ('backlog')"),
            "\"states\".\"group\" IN ('backlog')"
        );
        assert_eq!(
            remap_legacy("issue.created_at::date >= $2::date"),
            "\"issues\".created_at::date >= $2::date"
        );
    }

    #[test]
    fn link_joins_cover_every_remapped_table() {
        let fragment =
            "\"issue_labels\".\"label_id\" IN ($2) AND \"cycle_issues\".\"cycle_id\" IS NULL";
        let joins = link_joins_for(fragment, "");
        assert!(joins.contains("LEFT JOIN \"issue_labels\""));
        assert!(joins.contains("LEFT JOIN \"cycle_issues\""));
        assert!(!joins.contains("\"issue_assignees\""));
        assert_eq!(link_joins_for("\"issues\".priority IN ($2)", ""), "");
    }

    #[test]
    fn link_joins_skip_already_joined_tables() {
        // The assignee family composes its own `issue_assignees` join;
        // the legacy set must not repeat it (duplicate table error).
        let fragment =
            "\"issue_assignees\".assignee_id IN ($2) AND \"issue_labels\".label_id IN ($3)";
        let joins = link_joins_for(fragment, " LEFT JOIN \"issue_assignees\" ON (...)");
        assert!(!joins.contains("\"issue_assignees\""));
        assert!(joins.contains("LEFT JOIN \"issue_labels\""));
    }

    #[test]
    fn segmented_states_reuse_one_states_join() {
        // `?segment=state__group` (the suite's segmented saved case):
        // the segment join subsumes the manager `states` join, and the
        // bare count keeps the manager join with no dimension joins.
        let (count, plot) = saved_statements(
            "priority",
            "issue_count",
            Some("state__group"),
            Some("\"workspaces\".\"slug\" = $1"),
            "",
        )
        .expect("known axes");
        assert_eq!(plot.matches("JOIN \"states\"").count(), 1);
        assert!(count.contains("JOIN \"states\""));
        assert!(!count.contains("\"segment\""));
    }

    #[test]
    fn saved_count_has_no_dimension_fanout() {
        // The total is the bare filtered scope: a labels dimension must
        // not leak its fanning join into the count.
        let (count, _) = saved_statements(
            "labels__id",
            "issue_count",
            None,
            Some("\"workspaces\".\"slug\" = $1"),
            "",
        )
        .expect("known axes");
        assert!(!count.contains("issue_labels"));
    }

    #[test]
    fn stored_seed_query_compiles_to_slug_predicate() {
        // AV1: query={"workspace__slug": "an-ws"} (conftest seed).
        let mut binder = crate::app_issues::Binder::new();
        let out = stored_filters(&json!({"workspace__slug": "an-ws"}), &mut binder)
            .expect("seed query compiles")
            .expect("non-empty");
        assert_eq!(out, "\"workspaces\".\"slug\" = $1");
    }

    #[test]
    fn stored_in_accepts_lone_scalar() {
        // `{"priority__in": "high"}` (the analytic-view CRUD contract).
        let mut binder = crate::app_issues::Binder::new();
        let out = stored_filters(&json!({"priority__in": "high"}), &mut binder)
            .expect("compiles")
            .expect("non-empty");
        assert_eq!(out, "\"issues\".\"priority\" IN ($1)");
    }

    #[test]
    fn stored_unknown_key_is_500() {
        let mut binder = crate::app_issues::Binder::new();
        let err = stored_filters(&json!({"nope__in": ["x"]}), &mut binder).unwrap_err();
        assert!(matches!(err, IssuesDenial::ServerError));
    }

    #[test]
    fn stored_non_object_is_500() {
        let mut binder = crate::app_issues::Binder::new();
        let err = stored_filters(&json!(["workspace__slug"]), &mut binder).unwrap_err();
        assert!(matches!(err, IssuesDenial::ServerError));
    }

    #[test]
    fn saved_statements_use_dimension_and_scope() {
        let (count, plot) = saved_statements(
            "priority",
            "issue_count",
            None,
            Some("\"workspaces\".\"slug\" = $1"),
            "",
        )
        .expect("known axes");
        assert!(count.starts_with("SELECT COUNT(*)"));
        assert!(count.contains("\"workspaces\".\"slug\" = $1"));
        assert!(count.contains(q::ISSUE_OBJECTS_SCOPE));
        assert!(plot.contains("GROUP BY \"dimension\""));
        assert!(plot.contains("ORDER BY \"dimension\" ASC"));
        assert!(!plot.contains("\"segment\""));
    }

    #[test]
    fn saved_estimate_plot_sums_cast_float_once() {
        let (_, plot) =
            saved_statements("priority", "estimate", None, None, "").expect("known axes");
        assert!(plot.contains("SUM(CAST(\"estimate_points\".\"value\" AS DOUBLE PRECISION))"));
        assert_eq!(plot.matches("estimate_points").count(), 3);
    }

    #[test]
    fn default_scope_threads_slug_first() {
        let mut binder = slugs_first("an-ws");
        let legacy = legacy_filters(&QueryMap::new(), &mut binder).expect("empty params compile");
        assert!(legacy.is_none());
        let scope = issue_where("$1", legacy.as_deref());
        assert!(scope.contains("\"workspaces\".\"slug\" = $1"));
        assert!(scope.contains(q::ISSUE_OBJECTS_SCOPE));
    }

    #[test]
    fn count_distribution_groups_and_sorts_priority() {
        let rows = vec![
            (Some("urgent".to_owned()), None, 1),
            (Some("high".to_owned()), None, 1),
            (Some("medium".to_owned()), None, 1),
        ];
        let dist = distribution_from_count_rows(rows, "priority", false);
        let keys: Vec<&str> = dist.keys().map(String::as_str).collect();
        assert_eq!(keys, vec!["medium", "high", "urgent"]);
        assert_eq!(dist["medium"], json!([{"dimension": "medium", "count": 1}]));
    }

    #[test]
    fn count_distribution_seed_shape() {
        // FX-A-H-01 first pair: three priority buckets, extras empty.
        let rows = vec![
            (Some("high".to_owned()), None, 1),
            (Some("medium".to_owned()), None, 1),
            (Some("urgent".to_owned()), None, 1),
        ];
        let dist = distribution_from_count_rows(rows, "priority", false);
        let body = analytics_envelope(3, dist, extras_envelope(None, None, None, None, None));
        let parsed: Value = serde_json::from_str(&body).expect("json");
        assert_eq!(parsed["total"], json!(3));
        assert_eq!(
            parsed["distribution"],
            json!({
                "medium": [{"dimension": "medium", "count": 1}],
                "high": [{"dimension": "high", "count": 1}],
                "urgent": [{"dimension": "urgent", "count": 1}],
            })
        );
        assert_eq!(
            parsed["extras"],
            json!({
                "state_details": {},
                "assignee_details": {},
                "label_details": {},
                "cycle_details": {},
                "module_details": {},
            })
        );
        // Byte order: total, distribution, extras.
        assert!(body.starts_with(r#"{"total":3,"distribution":{"#));
    }

    #[test]
    fn null_dimension_groups_under_none() {
        // `str(None)` groups under 'None', which the priority `sort_data`
        // drops (not in low/medium/high/urgent/none); other axes keep it
        // (sorted with 'none' last only for the exact 'none' spelling).
        // The bucket key is 'None' but the row's dimension renders null
        // (the Python row dict holds `None`).
        let rows = vec![(None, None, 2)];
        let dist = distribution_from_count_rows(rows, "priority", false);
        assert!(dist.is_empty());
        let rows = vec![(None, None, 2)];
        let dist = distribution_from_count_rows(rows, "state__group", false);
        assert_eq!(dist["None"], json!([{"dimension": null, "count": 2}]));
    }

    #[test]
    fn estimate_null_renders_null() {
        let rows = vec![(Some("high".to_owned()), None, None)];
        let dist = distribution_from_estimate_rows(rows, "priority", false);
        assert_eq!(
            dist["high"],
            json!([{"dimension": "high", "estimate": null}])
        );
        let rows = vec![(Some("high".to_owned()), None, Some(8.0))];
        let dist = distribution_from_estimate_rows(rows, "priority", false);
        assert_eq!(
            dist["high"],
            json!([{"dimension": "high", "estimate": 8.0}])
        );
    }

    #[test]
    fn null_dimension_renders_null_in_estimate_arm() {
        // The estimate arm has no NULL-dimension exclusion, so a NULL group
        // reaches the response: key 'None', dimension null.
        let rows = vec![(None, None, Some(5.0))];
        let dist = distribution_from_estimate_rows(rows, "state__group", false);
        assert_eq!(dist["None"], json!([{"dimension": null, "estimate": 5.0}]));
    }

    #[test]
    fn segment_rides_the_bucket() {
        let rows = vec![(Some("high".to_owned()), Some("backlog".to_owned()), 1)];
        let dist = distribution_from_count_rows(rows, "priority", true);
        assert_eq!(
            dist["high"],
            json!([{"dimension": "high", "segment": "backlog", "count": 1}])
        );
    }

    #[test]
    fn null_segment_renders_explicit_null() {
        // A NULL segment value keeps its key with null (Django's row dict
        // holds `None`); without a segment axis the key stays absent.
        let rows = vec![(Some("high".to_owned()), None, 1)];
        let dist = distribution_from_count_rows(rows, "priority", true);
        assert_eq!(
            dist["high"],
            json!([{"dimension": "high", "segment": null, "count": 1}])
        );
        let rows = vec![(Some("high".to_owned()), None, 1)];
        let dist = distribution_from_count_rows(rows, "priority", false);
        assert_eq!(dist["high"], json!([{"dimension": "high", "count": 1}]));
    }

    // -- PIDASHCONV-424 (handlers-D) --------------------------------------

    #[test]
    fn project_gate_paths_match_gate_table() {
        // The three project rows are PROJECT-level (FX-A-G-01): charts admits
        // GUEST, the other two stop at MEMBER.
        for (path, guest) in [
            (PATH_PROJECT_ADVANCE, false),
            (PATH_PROJECT_STATS, false),
            (PATH_PROJECT_CHARTS, true),
        ] {
            let row = super::super::gates::gate_for("GET", path).expect("gate row");
            match row.gate {
                super::super::gates::Gate::Project { roles } => {
                    assert!(roles.contains(&ROLE_ADMIN), "{path}");
                    assert!(roles.contains(&ROLE_MEMBER), "{path}");
                    assert_eq!(roles.contains(&ROLE_GUEST), guest, "{path}");
                }
                _ => panic!("{path} is not a project gate"),
            }
        }
    }

    #[test]
    fn project_denial_bodies_are_byte_exact() {
        let (status, body) = Denial::ProjectNotFound.status_and_body();
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body, PROJECT_NOT_FOUND_BODY);
        assert_eq!(PROJECT_NOT_FOUND_BODY, r#"{"detail":"Project not found"}"#);
        // The empty chart body is DRF-compact (no spaces).
        assert_eq!(EMPTY_PROJECT_CHART_BODY, r#"{"data":[],"schema":{}}"#);
        assert_eq!(
            serde_json::from_str::<Value>(EMPTY_PROJECT_CHART_BODY).expect("json"),
            json!({"data": [], "schema": {}}),
        );
    }

    #[test]
    fn through_branch_precedence_and_validation() {
        let id = "12345678-1234-1234-1234-1234567890ab";
        let params = |pairs: &[(&str, &str)]| {
            pairs
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect::<HashMap<String, String>>()
        };
        // Cycle wins when both are present (`if`/`elif`).
        assert!(matches!(
            through_branch(&params(&[("cycle_id", id), ("module_id", id)])).expect("branch"),
            ThroughBranch::Cycle(_)
        ));
        assert!(matches!(
            through_branch(&params(&[("module_id", id)])).expect("branch"),
            ThroughBranch::Module(_)
        ));
        assert_eq!(
            through_branch(&params(&[])).expect("branch"),
            ThroughBranch::Plain
        );
        // Malformed ids — including the empty-but-present value — 400 on the
        // UUID coercion (the `ValidationError` branch).
        for raw in ["nope", "", "123"] {
            assert!(
                matches!(
                    through_branch(&params(&[("cycle_id", raw)])),
                    Err(Denial::InvalidDetail)
                ),
                "{raw:?}"
            );
            assert!(
                matches!(
                    through_branch(&params(&[("module_id", raw)])),
                    Err(Denial::InvalidDetail)
                ),
                "{raw:?}"
            );
        }
    }

    #[test]
    fn project_facts_follow_the_project_gate() {
        // (ws role, project role, allowed, advance?, charts?)
        let cases = [
            (Some(20), Some(20), true, true), // admin member
            (Some(15), Some(15), true, true), // member member
            (Some(5), Some(5), false, true),  // guest: charts only
            (None, None, false, false),       // outsider
            (Some(20), Some(5), true, true),  // ws-admin override: any project row
            (Some(20), None, false, false),   // ws admin without a project row still denies
            (None, Some(15), true, true),     // project row alone suffices
        ];
        for (ws, pm, advance, charts) in cases {
            let facts = project_facts("an-ws", ws, pm, PROJECT_ADMIN_MEMBER);
            let row = super::super::gates::gate_for("GET", PATH_PROJECT_ADVANCE).expect("gate row");
            let scope = super::super::gates::tenant_context("an-ws");
            let outcome = super::super::gates::decide_gate(&row.gate, &scope, &facts);
            assert_eq!(
                outcome,
                if advance {
                    super::super::gates::GateOutcome::Allow
                } else {
                    super::super::gates::GateOutcome::Deny
                },
                "advance ws={ws:?} pm={pm:?}"
            );
            assert_eq!(
                check_project_gate(
                    PATH_PROJECT_CHARTS,
                    "an-ws",
                    ws,
                    pm,
                    PROJECT_ADMIN_MEMBER_GUEST
                )
                .is_ok(),
                charts,
                "charts ws={ws:?} pm={pm:?}"
            );
        }
    }

    #[test]
    fn through_ids_subquery_carries_base_scope() {
        let ctx = FilterCtx::parse(&HashMap::new(), "analytics", utc(2026, 9, 30));
        let id = uuid::Uuid::parse_str("12345678-1234-1234-1234-1234567890ab").expect("uuid");
        let sub = through_ids_subquery(ThroughBranch::Cycle(id), &ctx).expect("subquery");
        // The id list selects through issue ids over the spliced joins (the
        // Q-05 shell leaves joins to the handler).
        assert!(
            sub.contains("SELECT \"issue_id\" FROM \"cycle_issues\""),
            "{sub}"
        );
        assert!(sub.contains("INNER JOIN \"projects\""), "{sub}");
        assert!(sub.contains("INNER JOIN \"project_members\""), "{sub}");
        assert!(sub.contains("\"workspaces\".\"slug\" = $1"), "{sub}");
        assert!(
            sub.contains("\"cycle_id\" = '12345678-1234-1234-1234-1234567890ab'"),
            "{sub}"
        );
        let sub = through_ids_subquery(ThroughBranch::Module(id), &ctx).expect("subquery");
        assert!(sub.contains("FROM \"module_issues\""), "{sub}");
        assert!(sub.contains("\"module_id\" = "), "{sub}");
        assert_eq!(
            through_ids_subquery(ThroughBranch::Plain, &ctx).expect("plain"),
            String::new()
        );
    }

    #[test]
    fn uuid_literal_quotes_hyphenated() {
        let id = uuid::Uuid::parse_str("12345678-1234-1234-1234-1234567890ab").expect("uuid");
        assert_eq!(uuid_literal(&id), "'12345678-1234-1234-1234-1234567890ab'");
    }

    #[test]
    fn project_work_item_keys_are_the_five() {
        assert_eq!(
            q::PROJECT_WORK_ITEM_KEYS
                .iter()
                .map(|(key, _)| *key)
                .collect::<Vec<_>>(),
            [
                "total_work_items",
                "started_work_items",
                "backlog_work_items",
                "un_started_work_items",
                "completed_work_items",
            ]
        );
    }

    #[test]
    fn completion_monthly_body_zero_fills_and_steps_months() {
        let mut stats = HashMap::new();
        stats.insert("2026-09-01".to_owned(), (3, 1));
        // Plain month-start: one bucket per month through the current one.
        let body = completion_monthly_body(
            &stats,
            NaiveDate::from_ymd_opt(2026, 9, 1).expect("date"),
            utc(2026, 9, 30),
        )
        .expect("body");
        let parsed: Value = serde_json::from_str(&body).expect("json");
        assert_eq!(
            parsed["data"],
            json!([{
                "key": "2026-09-01",
                "name": "2026-09-01",
                "count": 3,
                "completed_issues": 1,
                "created_issues": 3,
            }]),
        );
        assert_eq!(
            parsed["schema"],
            json!({"completed_issues": "completed_issues", "created_issues": "created_issues"}),
        );
        // Mid-month start after the 1st empties the loop outright (ported quirk).
        let body = completion_monthly_body(
            &stats,
            NaiveDate::from_ymd_opt(2026, 9, 15).expect("date"),
            utc(2026, 9, 30),
        )
        .expect("body");
        let parsed: Value = serde_json::from_str(&body).expect("json");
        assert_eq!(parsed["data"], json!([]));
        // Day-preserving step onto an invalid day is the generic 500 (Aug 31
        // -> Sept 31, like Python's `replace` raising `ValueError`).
        assert!(matches!(
            completion_monthly_body(
                &stats,
                NaiveDate::from_ymd_opt(2026, 8, 31).expect("date"),
                utc(2026, 10, 15),
            ),
            Err(Denial::ServerError)
        ));
    }
}
