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
//! analytic-view viewset (PIDASHCONV-389), saved/export/default/project-stats
//! (PIDASHCONV-399), project advance (PIDASHCONV-424, which reuses
//! [`build_analytics_chart`] and the scope helpers here — same module, no
//! fork), export-issues (PIDASHCONV-430). Query builders stay in
//! `pidash_services::app_analytics::queries`, gates in [`super::gates`].

use std::collections::HashMap;

use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Router;
use chrono::{DateTime, Datelike, NaiveDate, Utc};
use serde_json::{Map, Value};
use sqlx::Row;

use crate::middleware::SessionHandle;
use crate::state::AppState;

use pidash_auth::permissions::allow::AllowFacts;
use pidash_auth::permissions::{ROLE_ADMIN, ROLE_MEMBER};
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

/// Register the three workspace advance-analytics GET paths. Owned methods
/// serve from Rust; everything else proxies to Django (its 401-anon-before-405
/// and DRF metadata live there) — the `app_cycles` precedent.
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
        let body = build_analytics_chart(pool, &slug, &actor.id, &ctx, x_axis, group_by).await?;
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
    // current month-start (`advance.py:287-309`). `start_date` is the
    // workspace month-start, OVERWRITTEN as-is by the period start when
    // `chart_period_range` is set (`advance.py:267-270` — no month-start
    // normalization, ported quirk: a mid-month start both empties the loop
    // when it falls after the 1st and can 500 on the day-preserving month
    // step, e.g. Aug 31 -> Sept 31, exactly like Python's `replace` raising
    // `ValueError` into the generic 500). Keys after the first are
    // therefore not necessarily month-starts and never match the
    // month-keyed stats dict (count 0 there).
    let mut current = match ctx.period {
        Some((period_start, _)) => period_start,
        None => start,
    };
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
/// path (PIDASHCONV-424) from this module — no fork.
pub(crate) async fn build_analytics_chart(
    pool: &sqlx::PgPool,
    slug: &str,
    user_id: &uuid::Uuid,
    ctx: &FilterCtx,
    x_axis: &str,
    group_by: Option<&str>,
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
    let group_filter = group.as_ref().map(|axis| axis.filter).unwrap_or("");
    let scope = format!("{base}{}{group_filter}{date_pred}", axis.filter);
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

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
}
