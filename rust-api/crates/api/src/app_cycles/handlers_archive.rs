//! Cycle archive / unarchive handlers (PIDASHCONV-377).
//!
//! Ports `CycleArchiveUnarchiveEndpoint`
//! (`apps/api/pi_dash/app/views/cycle/archive.py:271-611`, drift baseline
//! `01a93e17`) with identical URL paths, status codes and JSON bytes.
//! Routes live in `apps/api/pi_dash/app/urls/cycle.py:82-95` — all three
//! paths serve this one endpoint class:
//!
//! - `GET archived-cycles/` — archived list (`archive.py:273-304`)
//! - `GET archived-cycles/<pk>/` — archived detail (`archive.py:305-584`)
//! - `POST cycles/<cycle_id>/archive/` — archive (`archive.py:586-604`)
//! - `DELETE cycles/<cycle_id>/archive/` — unarchive (`archive.py:606-611`)
//! - `GET cycles/<cycle_id>/archive/` — the list branch again: the URL
//!   binds `cycle_id`, the view signature only binds `pk`, so `pk` stays
//!   `None` and the list renders, ignoring `cycle_id`
//!   (`archive.py:272-273`; ported quirk Q1). No Rust code serves it:
//!   the method proxies to Django, which renders the quirk itself.
//!
//! Fixture ids: F-C27-04 (`rust-api/fixtures/app_cycles/queries_archive.*`)
//! for the queryset, projections and post/delete transitions;
//! F-C27-07 (`rust-api/fixtures/app_cycles/perms.json`) for the
//! ADMIN/MEMBER gates on all five method+path rows (see [`super::gates`]).
//!
//! Handler notes (all verified against the Python source):
//! - The list/detail responses serialize raw `.values()` dicts — no
//!   serializer runs, so datetimes render in stored UTC (`isoformat` with
//!   `+00:00` rewritten to `Z`, DRF `JSONEncoder`), never shifted into the
//!   request zone. [`render_stored_datetime`] reproduces that.
//! - POST answers `{"archived_at": str(cycle.archived_at)}`: Python `str`
//!   of the aware datetime (space separator, `+00:00` suffix), not the
//!   DRF rendering. [`render_python_datetime`] reproduces that.
//! - POST/DELETE fetch with `Cycle.objects.get` (soft-deletion manager,
//!   no archived predicates): a miss raises `DoesNotExist` → the
//!   `ObjectDoesNotExist` 404, never a shaped body. Archiving an
//!   already-archived cycle re-stamps; unarchiving a live cycle is a
//!   no-op save → still 204.
//! - The favorite cleanup on POST is the soft-deletion manager's
//!   queryset `delete()` (`db/mixins.py:48-53`): `UPDATE deleted_at`,
//!   not a hard delete (fixture wording corrected; verified in code).
//! - Detail of a missing/unarchived/foreign pk: `data` is `None` and
//!   `data["estimate_distribution"]` raises `TypeError` → generic 500,
//!   not 404 (ported quirk Q4).
//! - `burndown_plot` is ported for the cycle branch only
//!   (`utils/analytics_plot.py:123-264`, `cycle_id` set, `module_id`
//!   `None`); the module branch belongs to D-28. [`burndown_chart`]
//!   reproduces the date expansion, cumulative sums and future-`None`
//!   rule verbatim.
//!
//! Ported quirks (translate, don't redesign):
//! - Q1: `GET cycles/<cycle_id>/archive/` renders the archived LIST
//!   (the URL kwarg never reaches the view signature).
//! - Q2: `end_date = None` on POST raises `TypeError` (`None >= now`)
//!   → generic 500, not the 400 guard.
//! - Q3: the archived `total_issues` count and `assignee_ids` omit the
//!   bridge `deleted_at` guard the base queryset has (F-C27-04 R2/R4).
//! - Q4: archived detail of a missing row is a 500 (`None[...]`),
//!   unlike the base retrieve's `{"error":"Cycle not found"}` 404.
//! - Q5: the estimate subqueries run on `Issue.issue_objects`, so the
//!   manager exclusions (triage, archived, draft, project-archived)
//!   apply — with Django's `exclude()` NULL rule (`exclude(
//!   state__group=TRIAGE)` compiles to `NOT (group = 'triage' AND group
//!   IS NOT NULL)`, so NULL-state rows are KEPT while triage rows drop;
//!   verified against live Django) — while the `total_*_issues` counts
//!   (on `Cycle.objects`) count triage issues. Both halves ported as
//!   observed.
//!
//! Out of scope (sibling handler issues): every other cycle route
//! (PIDASHCONV-321/323/357/410 own their bodies); the queries builders
//! stay in `pidash_services::app_cycles::queries`, the gates in
//! [`super::gates`].

use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Router;
use chrono::{DateTime, NaiveDate, Utc};
use serde_json::{Map, Value};
use sqlx::Row;

use crate::middleware::SessionHandle;
use crate::state::AppState;

use pidash_auth::permissions::allow::AllowFacts;
use pidash_auth::permissions::{ROLE_ADMIN, ROLE_MEMBER};
use pidash_types::WorkspaceId;

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------

/// Canonical gate-table paths for the five owned method+path rows
/// (mirrors [`super::gates::GATES`] so the table stays the single source
/// of truth; the unit test pins them together).
pub const PATH_ARCHIVE: &str = "workspaces/<slug>/projects/<id>/cycles/<uuid>/archive/";
pub const PATH_ARCHIVED_LIST: &str = "workspaces/<slug>/projects/<id>/archived-cycles/";
pub const PATH_ARCHIVED_DETAIL: &str = "workspaces/<slug>/projects/<id>/archived-cycles/<uuid>/";

/// Register the three archive paths. Owned methods serve from Rust;
/// everything else proxies to Django (its 401-anon-before-405, DRF
/// metadata and sibling actions live there). Non-UUID `pk`/`cycle_id`
/// tails proxy inside the handlers (Django's `<uuid:>` 404 precedes
/// auth — the `app_views_search` precedent).
pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/workspaces/{slug}/projects/{project_id}/archived-cycles/",
            owned(axum::routing::get(archived_list), &["GET"]),
        )
        .route(
            "/api/workspaces/{slug}/projects/{project_id}/archived-cycles/{pk}/",
            owned(axum::routing::get(archived_retrieve), &["GET"]),
        )
        .route(
            "/api/workspaces/{slug}/projects/{project_id}/cycles/{cycle_id}/archive/",
            owned(
                axum::routing::post(archive_post).delete(unarchive_delete),
                &["POST", "DELETE"],
            ),
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
pub const UNAUTHENTICATED_BODY: &str =
    r#"{"detail":"Authentication credentials were not provided."}"#;
/// `handle_exception`'s `ObjectDoesNotExist` branch (`app/views/base.py`):
/// the POST/DELETE `Cycle.objects.get` miss.
pub const NOT_FOUND_BODY: &str = r#"{"error":"The required object does not exist."}"#;
/// `Project.resolve` miss on a non-UUID identifier
/// (`db/models/project.py:213-217`).
pub const PROJECT_NOT_FOUND_BODY: &str = r#"{"detail":"Project not found"}"#;
/// `handle_exception`'s generic 500 branch: the POST `None >= now`
/// `TypeError` (Q2) and the detail `None[...]` `TypeError` (Q4) land here.
pub const SERVER_ERROR_BODY: &str = r#"{"error":"Something went wrong please try again later"}"#;
/// The archive POST date guard (`archive.py:590-594`).
pub const ARCHIVE_REFUSED_BODY: &str = r#"{"error":"Only completed cycles can be archived"}"#;

/// Handler failure with its exact status + body.
#[derive(Debug)]
pub enum Denial {
    /// 401, DRF `NotAuthenticated`.
    Unauthorized,
    /// 403, `@allow_permission` body.
    Forbidden,
    /// 404, `ObjectDoesNotExist` branch.
    NotFound,
    /// 404, `Project.resolve` miss.
    ProjectNotFound,
    /// 400, the archive date guard.
    ArchiveRefused,
    /// 500, generic branch.
    ServerError,
    /// A pre-rendered exact body with its status.
    Raw(StatusCode, String),
}

impl Denial {
    fn status_and_body(&self) -> (StatusCode, String) {
        match self {
            Denial::Unauthorized => (StatusCode::UNAUTHORIZED, UNAUTHENTICATED_BODY.to_owned()),
            Denial::Forbidden => (
                StatusCode::FORBIDDEN,
                super::gates::FORBIDDEN_BODY.to_owned(),
            ),
            Denial::NotFound => (StatusCode::NOT_FOUND, NOT_FOUND_BODY.to_owned()),
            Denial::ProjectNotFound => (StatusCode::NOT_FOUND, PROJECT_NOT_FOUND_BODY.to_owned()),
            Denial::ArchiveRefused => (StatusCode::BAD_REQUEST, ARCHIVE_REFUSED_BODY.to_owned()),
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

fn json_string(value: &str) -> String {
    serde_json::to_string(value).expect("json string")
}

fn json_response(status: StatusCode, body: String) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body))
        .expect("archive response")
}

// ---------------------------------------------------------------------------
// Request context: auth + tenant + membership
// ---------------------------------------------------------------------------

/// Session auth (`BaseSessionAuthentication` + `IsAuthenticated` on the
/// view): anonymous answers the DRF `NotAuthenticated` body before
/// anything else runs (the `app_views_search` precedent).
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

/// `Project.resolve(workspace_slug, value)`: UUIDs pass through (the row
/// check happens in the view body); other identifiers match
/// `UPPER(identifier)` in the workspace; misses raise `Http404`
/// (`db/models/project.py:213-217` — the `app_views_search` precedent).
async fn resolve_project_id(
    pool: &sqlx::PgPool,
    slug: &str,
    raw: &str,
) -> Result<uuid::Uuid, Denial> {
    if let Ok(id) = raw.parse::<uuid::Uuid>() {
        return Ok(id);
    }
    let upper = normalize_resolve_identifier(raw);
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

/// Membership roles for the archive gates, resolved with the same row
/// filters Python uses: active, non-deleted rows scoped to the workspace
/// slug (and project id for the project row) — the `app_views_search`
/// precedent.
struct Membership {
    workspace_role: Option<i16>,
    project_role: Option<i16>,
}

async fn membership(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    user_id: &uuid::Uuid,
) -> Result<Membership, Denial> {
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
    Ok(Membership {
        // `role` is non-nullable; the outer Option is row presence.
        workspace_role: workspace_role.and_then(|row| row.0),
        project_role: row.and_then(|row| row.0),
    })
}

/// Build the gate facts for an archive route: ADMIN/MEMBER project gate,
/// no creator bypass (`archive.py:271/:586/:606`, F-C27-07).
fn archive_facts(slug: &str, member: &Membership) -> AllowFacts {
    let allowed =
        |role: Option<i16>| matches!(role.map(i32::from), Some(ROLE_ADMIN) | Some(ROLE_MEMBER));
    AllowFacts {
        workspace: WorkspaceId::from(slug),
        authenticated: true,
        is_workspace_member: member.workspace_role.is_some(),
        has_allowed_workspace_role: allowed(member.workspace_role),
        is_creator: false,
        has_allowed_project_role: allowed(member.project_role),
        is_project_member: member.project_role.is_some(),
        is_workspace_admin: member.workspace_role.map(i32::from) == Some(ROLE_ADMIN),
    }
}

/// Enforce the gate-table row for one method+path: anonymous never
/// reaches here ([`actor`] denied first); a deny answers the decorator
/// 403.
fn check_gate(method: &str, path: &str, slug: &str, member: &Membership) -> Result<(), Denial> {
    let row = super::gates::gate_for(method, path).ok_or(Denial::ServerError)?;
    let scope = super::gates::tenant_context(slug);
    match super::gates::decide_gate(&row.gate, &scope, &archive_facts(slug, member)) {
        super::gates::GateOutcome::Allow => Ok(()),
        _ => Err(Denial::Forbidden),
    }
}

// ---------------------------------------------------------------------------
// DRF rendering (no serializer runs on these paths)
// ---------------------------------------------------------------------------

/// Render one stored datetime exactly like DRF's `JSONEncoder` on a raw
/// `.values()` dict (`rest_framework/utils/encoders.py:29-33`):
/// `isoformat` with a `+00:00` suffix rewritten to `Z`, microseconds
/// only when nonzero. The value is stored UTC (`USE_TZ`, `TIME_ZONE`
/// `"UTC"`), and no serializer runs here, so unlike the serializer
/// paths there is NO shift into the request zone: the parsed instant is
/// normalized back to UTC first (the pool session zone only affects the
/// `row_to_json` text, never the bytes).
fn render_stored_datetime(value: &Value) -> String {
    let text = match value {
        Value::String(text) => text,
        _ => return "null".to_owned(),
    };
    match DateTime::parse_from_rfc3339(text) {
        Ok(aware) => {
            let utc = aware.with_timezone(&Utc);
            let mut out = utc.format("%Y-%m-%dT%H:%M:%S").to_string();
            let nanos = utc.timestamp_subsec_nanos();
            if nanos != 0 {
                out.push_str(&format!(".{:06}", nanos / 1000));
            }
            out.push('Z');
            json_string(&out)
        }
        Err(_) => json_string(text),
    }
}

/// Render one datetime exactly like Python `str()` of the aware value
/// (`archive.py:604`): space separator, `+00:00` suffix with colon,
/// microseconds only when nonzero.
fn render_python_datetime(value: &DateTime<Utc>) -> String {
    let mut out = value.format("%Y-%m-%d %H:%M:%S").to_string();
    let nanos = value.timestamp_subsec_nanos();
    if nanos != 0 {
        out.push_str(&format!(".{:06}", nanos / 1000));
    }
    out.push_str("+00:00");
    out
}

/// Float-typed keys on the `row_to_json` paths: Postgres renders
/// integral `float8`/`numeric` without a fraction (`65535`, `0`) while
/// DRF renders the Python float (`65535.0`, `0.0`). These keys always
/// render as floats.
const FLOAT_KEYS: &[&str] = &[
    "sort_order",
    "completed_estimate_points",
    "total_estimate_points",
];

/// Render one float-typed value: integral JSON numbers gain the `.0`
/// DRF emits; fractional numbers splice verbatim.
fn render_float_value(value: &Value) -> String {
    match value {
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                format!("{int}.0")
            } else if let Some(uint) = number.as_u64() {
                format!("{uint}.0")
            } else if let Some(float) = number.as_f64() {
                render_float(float)
            } else {
                "null".to_owned()
            }
        }
        Value::Null => "null".to_owned(),
        _ => serde_json::to_string(value).expect("row value"),
    }
}

/// Splice one `row_to_json` value verbatim, except datetimes (re-rendered
/// per [`render_stored_datetime`]) on the named keys and float columns
/// (re-rendered per [`render_float_value`]). Strings, bools, nulls,
/// arrays and objects pass through untouched.
fn splice_value(key: &str, value: &Value, datetime_keys: &[&str]) -> String {
    if datetime_keys.contains(&key) {
        return render_stored_datetime(value);
    }
    if FLOAT_KEYS.contains(&key) {
        return render_float_value(value);
    }
    match value {
        Value::Null => "null".to_owned(),
        Value::String(_) => json_string(value.as_str().expect("string")),
        Value::Number(_) | Value::Bool(_) | Value::Array(_) | Value::Object(_) => {
            serde_json::to_string(value).expect("row value")
        }
    }
}

// ---------------------------------------------------------------------------
// SQL: archived queryset (`archive.py:41-270`)
// ---------------------------------------------------------------------------

/// Archived-list wire keys (`archive.py:275-302`).
///
/// Verified against live Django: `.values()` does NOT preserve argument
/// order — the compiler emits model fields first (in `.values()` argument
/// order among themselves) and then annotations in annotation-DEFINITION
/// order (`get_queryset` `:136-233`: `is_favorite`, `total_issues`,
/// `completed_issues`, `cancelled_issues`, `started/unstarted/backlog`,
/// `status`, `assignee_ids`). So `archived_at` (model) sorts ahead of
/// every annotation, `completed_issues` ahead of `cancelled_issues`, and
/// `status`/`assignee_ids` swap. No `logo_props`, no
/// `version`/`created_by` — the archived list projects a narrower slice
/// than the base list.
pub const ARCHIVED_LIST_KEYS: &[&str] = &[
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
    "archived_at",
    "is_favorite",
    "total_issues",
    "completed_issues",
    "cancelled_issues",
    "started_issues",
    "unstarted_issues",
    "backlog_issues",
    "status",
    "assignee_ids",
];

/// Archived-detail wire keys (`archive.py:321-353`).
///
/// Same `.values()` rule: model fields in argument order, then the
/// kept annotations in definition order — the detail `.annotate(
/// sub_issues=...)` (`:310-320`) runs AFTER the base annotations, so
/// `sub_issues` renders second-to-last (after the two estimate points).
/// `estimate_distribution` + `distribution` append after `sub_issues`.
pub const ARCHIVED_DETAIL_KEYS: &[&str] = &[
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
    "created_by",
    "archived_at",
    "is_favorite",
    "total_issues",
    "completed_issues",
    "cancelled_issues",
    "started_issues",
    "unstarted_issues",
    "backlog_issues",
    "status",
    "assignee_ids",
    "completed_estimate_points",
    "total_estimate_points",
    "sub_issues",
];

/// Datetime keys on the list path (rendered per [`render_stored_datetime`]).
const LIST_DATETIME_KEYS: &[&str] = &["start_date", "end_date", "archived_at"];
/// Datetime keys on the detail path.
const DETAIL_DATETIME_KEYS: &[&str] = &["start_date", "end_date", "archived_at"];

/// Group-count predicate for one annotation (`archive.py:137-207`):
/// plain equality everywhere on the archive path (the base single-element
/// `IN` shape is NOT used here — ported asymmetry).
fn group_predicate(group: &str) -> String {
    format!("AND st.\"group\" = '{group}'")
}

/// One group-count annotation over the archived bridge. The bridge
/// `deleted_at` guard the base queryset has is OMITTED (ported quirk Q3);
/// the issue archived/draft/deleted guards stay.
fn count_annotation(group: Option<&str>) -> String {
    let predicate = group.map(group_predicate).unwrap_or_default();
    format!(
        "(SELECT COUNT(DISTINCT i.id) FROM cycle_issues ci \
          JOIN issues i ON i.id = ci.issue_id \
          LEFT JOIN states st ON st.id = i.state_id \
         WHERE ci.cycle_id = c.id \
           AND i.archived_at IS NULL AND i.is_draft = FALSE AND i.deleted_at IS NULL \
           {predicate})"
    )
}

/// `assignee_ids` annotation (`archive.py:224-233`): no bridge or issue
/// guards at all — only the `NOT NULL` filter (ported quirk Q3).
fn assignee_ids_annotation() -> String {
    "(SELECT COALESCE(ARRAY_AGG(DISTINCT ia.assignee_id) \
       FILTER (WHERE ia.assignee_id IS NOT NULL), '{}') \
      FROM cycle_issues ci \
      JOIN issues i ON i.id = ci.issue_id \
      JOIN issue_assignees ia ON ia.issue_id = i.id \
     WHERE ci.cycle_id = c.id)"
        .to_owned()
}

/// `is_favorite` annotation (`archive.py:42-48`): the soft-deletion
/// manager hides deleted favorites.
fn favorite_exists_annotation() -> String {
    "(SELECT EXISTS (SELECT 1 FROM user_favorites uf \
       JOIN workspaces w2 ON w2.id = uf.workspace_id \
      WHERE uf.user_id = $3 AND uf.entity_identifier = c.id \
        AND uf.entity_type = 'cycle' AND uf.project_id = $2 \
        AND w2.slug = $1 AND uf.deleted_at IS NULL))"
        .to_owned()
}

/// `status` annotation (`archive.py:209-223`): `timezone.now()` passed as
/// `$4` — the project-tz round-trip the base path takes is semantically
/// identical (F-C27-04 R3).
fn status_annotation() -> String {
    "CASE WHEN c.start_date <= $4 AND c.end_date >= $4 THEN 'CURRENT' \
     WHEN c.start_date > $4 THEN 'UPCOMING' \
     WHEN c.end_date < $4 THEN 'COMPLETED' \
     WHEN c.start_date IS NULL AND c.end_date IS NULL THEN 'DRAFT' \
     ELSE 'DRAFT' END"
        .to_owned()
}

/// One per-group estimate annotation (`archive.py:49-113` → `:234-266`):
/// `SUM(CAST(value AS FLOAT))` over `estimates.type = 'points'`, with
/// the `Issue.issue_objects` manager exclusions (ported quirk Q5: triage
/// dropped but NULL-state rows KEPT per the `exclude()` NULL rule,
/// archived/draft/project-archived excluded). `group = None` is the
/// total (no group predicate).
fn estimate_annotation(group: Option<&str>, alias: &str) -> String {
    let predicate = group.map(group_predicate).unwrap_or_default();
    format!(
        "COALESCE((SELECT SUM(CAST(ep.value AS FLOAT)) \
          FROM issues i \
          JOIN cycle_issues ci ON ci.issue_id = i.id \
            AND ci.deleted_at IS NULL AND ci.cycle_id = c.id \
          JOIN estimate_points ep ON ep.id = i.estimate_point_id \
          JOIN estimates e ON e.id = ep.estimate_id AND e.type = 'points' \
          JOIN projects ip ON ip.id = i.project_id \
          LEFT JOIN states st ON st.id = i.state_id \
         WHERE i.archived_at IS NULL AND i.is_draft = FALSE AND i.deleted_at IS NULL \
           AND ip.archived_at IS NULL AND (st.\"group\" != 'triage' OR st.\"group\" IS NULL) \
           {predicate}), 0.0) AS {alias}"
    )
}

/// Shared archived-queryset scope (`archive.py:114-123`): workspace slug,
/// project id, active project membership of the caller, live project,
/// archived only. `$1` slug, `$2` project id, `$3` user id.
fn archived_scope_where() -> String {
    "c.project_id = $2 AND c.deleted_at IS NULL \
     AND EXISTS (SELECT 1 FROM project_members pm \
                  WHERE pm.project_id = c.project_id AND pm.member_id = $3 \
                    AND pm.is_active) \
     AND p.archived_at IS NULL AND c.archived_at IS NOT NULL"
        .to_owned()
}

/// Archived-list statement (`archive.py:273-304`): the six group counts
/// but only the `total_issues` count is projected — started/unstarted/
/// backlog counts are annotated yet dropped by `.values()`, exactly like
/// the base list drops them. Effective order `-is_favorite,-created_at`
/// (`:303`; the queryset `-is_favorite,name` order is dead on list).
fn archived_list_sql() -> String {
    format!(
        "SELECT c.id, c.workspace_id, c.project_id, c.name, c.description, \
           c.start_date, c.end_date, c.owned_by_id, c.view_props, c.sort_order, \
           c.external_source, c.external_id, c.progress_snapshot, \
           {total} AS total_issues, \
           {fav} AS is_favorite, \
           {cancelled} AS cancelled_issues, \
           {completed} AS completed_issues, \
           {started} AS started_issues, \
           {unstarted} AS unstarted_issues, \
           {backlog} AS backlog_issues, \
           {assignees} AS assignee_ids, \
           {status} AS status, \
           c.archived_at \
         FROM cycles c \
         JOIN workspaces w ON w.id = c.workspace_id AND w.slug = $1 \
         JOIN projects p ON p.id = c.project_id \
         WHERE {scope} \
         ORDER BY is_favorite DESC, c.created_at DESC",
        total = count_annotation(None),
        fav = favorite_exists_annotation(),
        cancelled = count_annotation(Some("cancelled")),
        completed = count_annotation(Some("completed")),
        started = count_annotation(Some("started")),
        unstarted = count_annotation(Some("unstarted")),
        backlog = count_annotation(Some("backlog")),
        assignees = assignee_ids_annotation(),
        status = status_annotation(),
        scope = archived_scope_where(),
    )
}

/// Archived-detail base statement (`archive.py:306-354`): the list
/// select plus `sub_issues`, `logo_props`, the two estimate points and
/// `created_by`, constrained to one pk. The caller appends the
/// estimate/issue distributions.
fn archived_detail_sql() -> String {
    format!(
        "SELECT c.id, c.workspace_id, c.project_id, c.name, c.description, \
           c.start_date, c.end_date, c.owned_by_id, c.view_props, c.sort_order, \
           c.external_source, c.external_id, c.progress_snapshot, \
           (SELECT COUNT(*) FROM issues ch \
              JOIN cycle_issues cci ON cci.issue_id = ch.id \
                AND cci.deleted_at IS NULL AND cci.cycle_id = c.id \
              JOIN projects chp ON chp.id = ch.project_id \
              LEFT JOIN states chst ON chst.id = ch.state_id \
             WHERE ch.parent_id IS NOT NULL \
               AND ch.archived_at IS NULL AND ch.is_draft = FALSE AND ch.deleted_at IS NULL \
               AND chp.archived_at IS NULL AND (chst.\"group\" != 'triage' OR chst.\"group\" IS NULL)) AS sub_issues, \
           c.logo_props, \
           {completed_est}, \
           {total_est}, \
           {fav} AS is_favorite, \
           {total} AS total_issues, \
           {cancelled} AS cancelled_issues, \
           {completed} AS completed_issues, \
           {started} AS started_issues, \
           {unstarted} AS unstarted_issues, \
           {backlog} AS backlog_issues, \
           {assignees} AS assignee_ids, \
           {status} AS status, \
           c.created_by_id AS created_by, \
           c.archived_at \
         FROM cycles c \
         JOIN workspaces w ON w.id = c.workspace_id AND w.slug = $1 \
         JOIN projects p ON p.id = c.project_id \
         WHERE {scope} AND c.id = $5",
        completed_est = estimate_annotation(Some("completed"), "completed_estimate_points"),
        total_est = estimate_annotation(None, "total_estimate_points"),
        fav = favorite_exists_annotation(),
        total = count_annotation(None),
        cancelled = count_annotation(Some("cancelled")),
        completed = count_annotation(Some("completed")),
        started = count_annotation(Some("started")),
        unstarted = count_annotation(Some("unstarted")),
        backlog = count_annotation(Some("backlog")),
        assignees = assignee_ids_annotation(),
        status = status_annotation(),
        scope = archived_scope_where(),
    )
}

/// Fetch rows as JSON maps via `row_to_json` (the `app_issues`
/// precedent): everything splices verbatim downstream except the
/// datetime keys.
async fn fetch_rows(
    pool: &sqlx::PgPool,
    sql: &str,
    slug: &str,
    project_id: &uuid::Uuid,
    user_id: &uuid::Uuid,
    now: &DateTime<Utc>,
    extra: Option<&uuid::Uuid>,
) -> Result<Vec<Map<String, Value>>, Denial> {
    let mut query = sqlx::query(sql)
        .bind(slug)
        .bind(project_id)
        .bind(user_id)
        .bind(now);
    if let Some(pk) = extra {
        query = query.bind(pk);
    }
    let rows = query
        .fetch_all(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let text: String = row.try_get("__row").map_err(|_| Denial::ServerError)?;
        let value: Value = serde_json::from_str(&text).map_err(|_| Denial::ServerError)?;
        match value {
            Value::Object(map) => out.push(map),
            _ => return Err(Denial::ServerError),
        }
    }
    Ok(out)
}

fn fetch_list_sql() -> String {
    format!(
        "SELECT row_to_json(__r)::text AS __row FROM ({}) AS __r",
        archived_list_sql()
    )
}

fn fetch_detail_sql() -> String {
    format!(
        "SELECT row_to_json(__r)::text AS __row FROM ({}) AS __r",
        archived_detail_sql()
    )
}

// ---------------------------------------------------------------------------
// SQL: detail distributions (`archive.py:367-573`)
// ---------------------------------------------------------------------------

/// The `Issue.issue_objects` scope shared by the four distribution
/// queries and the burndown inputs: this cycle's live bridges,
/// tenant-scoped, with the manager exclusions (ported quirk Q5).
/// `$1` slug, `$2` project id, `$3` cycle id (these statements bind
/// exactly three parameters).
fn distribution_scope_where() -> String {
    "ci.cycle_id = $3 AND ci.deleted_at IS NULL \
     AND i.archived_at IS NULL AND i.is_draft = FALSE AND i.deleted_at IS NULL \
     AND ip.archived_at IS NULL AND (st.\"group\" != 'triage' OR st.\"group\" IS NULL) \
     AND i.project_id = $2 AND wi.slug = $1"
        .to_owned()
}

fn distribution_from() -> String {
    "FROM issues i \
     JOIN cycle_issues ci ON ci.issue_id = i.id \
     JOIN workspaces wi ON wi.id = i.workspace_id \
     JOIN projects ip ON ip.id = i.project_id \
     LEFT JOIN states st ON st.id = i.state_id"
        .to_owned()
}

/// `avatar_url` `Case` (`archive.py:377-395`, repeated `:482-500`): the
/// asset URL when `avatar_asset` is set, else the raw `avatar` field,
/// else NULL.
fn avatar_url_case() -> String {
    "CASE WHEN u.avatar_asset_id IS NOT NULL \
      THEN CONCAT('/api/assets/v2/static/', u.avatar_asset_id::text, '/') \
      WHEN u.avatar_asset_id IS NULL THEN u.avatar ELSE NULL END"
        .to_owned()
}

/// Estimate-flavoured sums (`archive.py:397-417`): total plus the
/// completed/pending `completed_at` splits, each over live non-draft
/// issues. `SUM` over no rows is NULL → JSON `null`.
fn estimate_sums_select() -> String {
    "SUM(CAST(ep.value AS FLOAT)) \
       FILTER (WHERE i.archived_at IS NULL AND i.is_draft = FALSE) AS total_estimates, \
     SUM(CAST(ep.value AS FLOAT)) \
       FILTER (WHERE i.completed_at IS NOT NULL \
                 AND i.archived_at IS NULL AND i.is_draft = FALSE) AS completed_estimates, \
     SUM(CAST(ep.value AS FLOAT)) \
       FILTER (WHERE i.completed_at IS NULL \
                 AND i.archived_at IS NULL AND i.is_draft = FALSE) AS pending_estimates"
        .to_owned()
}

/// Issue-count splits (`archive.py:509-529`): same shape as
/// [`estimate_sums_select`] but `COUNT(id)`.
fn issue_count_select() -> String {
    "COUNT(i.id) \
       FILTER (WHERE i.archived_at IS NULL AND i.is_draft = FALSE) AS total_issues, \
     COUNT(i.id) \
       FILTER (WHERE i.completed_at IS NOT NULL \
                 AND i.archived_at IS NULL AND i.is_draft = FALSE) AS completed_issues, \
     COUNT(i.id) \
       FILTER (WHERE i.completed_at IS NULL \
                 AND i.archived_at IS NULL AND i.is_draft = FALSE) AS pending_issues"
        .to_owned()
}

fn estimate_join() -> String {
    "JOIN estimate_points ep ON ep.id = i.estimate_point_id \
     JOIN estimates e ON e.id = ep.estimate_id AND e.type = 'points'"
        .to_owned()
}

/// Assignee estimate distribution (`archive.py:367-419`): one row per
/// assignee (plus the NULL row for unassigned issues), ordered by
/// display name. The joins are LEFT (annotation-side) with no
/// soft-delete guards — ported as observed.
fn assignee_estimate_sql() -> String {
    format!(
        "SELECT u.display_name, u.id AS assignee_id, {avatar} AS avatar_url, {sums} \
         {from} \
         LEFT JOIN issue_assignees ia ON ia.issue_id = i.id \
         LEFT JOIN users u ON u.id = ia.assignee_id \
         {est} \
         WHERE {scope} \
         GROUP BY u.display_name, u.id, ({avatar}) \
         ORDER BY u.display_name ASC",
        avatar = avatar_url_case(),
        sums = estimate_sums_select(),
        from = distribution_from(),
        est = estimate_join(),
        scope = distribution_scope_where(),
    )
}

/// Label estimate distribution (`archive.py:421-454`), ordered by label
/// name.
fn label_estimate_sql() -> String {
    format!(
        "SELECT l.name AS label_name, l.color, l.id AS label_id, {sums} \
         {from} \
         LEFT JOIN issue_labels il ON il.issue_id = i.id \
         LEFT JOIN labels l ON l.id = il.label_id \
         {est} \
         WHERE {scope} \
         GROUP BY l.name, l.color, l.id \
         ORDER BY l.name ASC",
        sums = estimate_sums_select(),
        from = distribution_from(),
        est = estimate_join(),
        scope = distribution_scope_where(),
    )
}

/// Assignee issue-count distribution (`archive.py:471-531`), ordered by
/// first/last name.
fn assignee_count_sql() -> String {
    format!(
        "SELECT u.first_name, u.last_name, u.id AS assignee_id, \
           {avatar} AS avatar_url, u.display_name, {counts} \
         {from} \
         LEFT JOIN issue_assignees ia ON ia.issue_id = i.id \
         LEFT JOIN users u ON u.id = ia.assignee_id \
         WHERE {scope} \
         GROUP BY u.first_name, u.last_name, u.id, ({avatar}), u.display_name \
         ORDER BY u.first_name ASC, u.last_name ASC",
        avatar = avatar_url_case(),
        counts = issue_count_select(),
        from = distribution_from(),
        scope = distribution_scope_where(),
    )
}

/// Label issue-count distribution (`archive.py:534-567`), ordered by
/// label name.
fn label_count_sql() -> String {
    format!(
        "SELECT l.name AS label_name, l.color, l.id AS label_id, {counts} \
         {from} \
         LEFT JOIN issue_labels il ON il.issue_id = i.id \
         LEFT JOIN labels l ON l.id = il.label_id \
         WHERE {scope} \
         GROUP BY l.name, l.color, l.id \
         ORDER BY l.name ASC",
        counts = issue_count_select(),
        from = distribution_from(),
        scope = distribution_scope_where(),
    )
}

/// Whether the project estimates in points (`archive.py:358-363`):
/// `Project.objects` is the plain manager — no deleted guards, ported
/// literally.
async fn project_estimates_points(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
) -> Result<bool, Denial> {
    let row: Option<(bool,)> = sqlx::query_as(
        "SELECT EXISTS (SELECT 1 FROM projects p \
           JOIN workspaces w ON w.id = p.workspace_id \
           JOIN estimates e ON e.id = p.estimate_id \
          WHERE w.slug = $1 AND p.id = $2 AND e.type = 'points')",
    )
    .bind(slug)
    .bind(project_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(row.map(|row| row.0).unwrap_or(false))
}

// ---------------------------------------------------------------------------
// SQL: burndown inputs (`utils/analytics_plot.py:123-264`, cycle branch)
// ---------------------------------------------------------------------------

/// Points-burndown rows: one `(date, value)` per estimated issue ordered
/// by physical row order. Python sums the queryset in return order
/// (no `order_by`), which is `ctid` order on a plain seqscan — the
/// `ORDER BY i.ctid` reproduces that summation order so float
/// accumulation matches bit for bit.
fn burndown_points_sql() -> String {
    format!(
        "SELECT (i.completed_at AT TIME ZONE 'UTC')::date AS d, \
           CAST(ep.value AS FLOAT) AS v \
         {from} \
         {est} \
         WHERE {scope} \
         ORDER BY i.ctid",
        from = distribution_from(),
        est = estimate_join(),
        scope = distribution_scope_where(),
    )
}

/// Issues-burndown rows: completed counts per UTC date (`TruncDate`
/// renders in `TIME_ZONE "UTC"`).
fn burndown_issues_sql() -> String {
    format!(
        "SELECT (i.completed_at AT TIME ZONE 'UTC')::date AS d, COUNT(*) AS n \
         {from} \
         WHERE {scope} AND i.completed_at IS NOT NULL \
         GROUP BY d ORDER BY d",
        from = distribution_from(),
        scope = distribution_scope_where(),
    )
}

// ---------------------------------------------------------------------------
// Shaping: distributions + burndown (`archive.py:321-353` + `:365-582`)
// ---------------------------------------------------------------------------

fn render_opt_str(value: Option<String>) -> String {
    match value {
        Some(text) => json_string(&text),
        None => "null".to_owned(),
    }
}

fn render_opt_uuid(value: Option<uuid::Uuid>) -> String {
    match value {
        Some(id) => json_string(&id.to_string()),
        None => "null".to_owned(),
    }
}

fn render_float(value: f64) -> String {
    serde_json::to_string(&serde_json::Number::from_f64(value).expect("finite")).expect("float")
}

fn render_opt_float(value: Option<f64>) -> String {
    match value {
        Some(number) => render_float(number),
        None => "null".to_owned(),
    }
}

struct EstimateRows {
    display: Option<String>,
    ident: Option<uuid::Uuid>,
    avatar: Option<String>,
    total: Option<f64>,
    completed: Option<f64>,
    pending: Option<f64>,
}

async fn fetch_estimate_rows(
    pool: &sqlx::PgPool,
    sql: &str,
    slug: &str,
    project_id: &uuid::Uuid,
    cycle_id: &uuid::Uuid,
    assignee: bool,
) -> Result<Vec<EstimateRows>, Denial> {
    let rows = sqlx::query(sql)
        .bind(slug)
        .bind(project_id)
        .bind(cycle_id)
        .fetch_all(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        if assignee {
            out.push(EstimateRows {
                display: row
                    .try_get("display_name")
                    .map_err(|_| Denial::ServerError)?,
                ident: row
                    .try_get("assignee_id")
                    .map_err(|_| Denial::ServerError)?,
                avatar: row.try_get("avatar_url").map_err(|_| Denial::ServerError)?,
                total: row
                    .try_get("total_estimates")
                    .map_err(|_| Denial::ServerError)?,
                completed: row
                    .try_get("completed_estimates")
                    .map_err(|_| Denial::ServerError)?,
                pending: row
                    .try_get("pending_estimates")
                    .map_err(|_| Denial::ServerError)?,
            });
        } else {
            out.push(EstimateRows {
                display: row.try_get("label_name").map_err(|_| Denial::ServerError)?,
                ident: row.try_get("label_id").map_err(|_| Denial::ServerError)?,
                avatar: row.try_get("color").map_err(|_| Denial::ServerError)?,
                total: row
                    .try_get("total_estimates")
                    .map_err(|_| Denial::ServerError)?,
                completed: row
                    .try_get("completed_estimates")
                    .map_err(|_| Denial::ServerError)?,
                pending: row
                    .try_get("pending_estimates")
                    .map_err(|_| Denial::ServerError)?,
            });
        }
    }
    Ok(out)
}

/// One estimate-distribution row in serialization order
/// (`archive.py:367-454`): assignees are `display_name, assignee_id,
/// avatar_url` (`:375`); labels are `label_name, color, label_id`
/// (`:431`) — annotation-definition order in both cases.
fn shape_estimate_row(row: &EstimateRows, assignee: bool) -> String {
    if assignee {
        format!(
            "{{\"display_name\":{},\"assignee_id\":{},\"avatar_url\":{},\
              \"total_estimates\":{},\"completed_estimates\":{},\
              \"pending_estimates\":{}}}",
            render_opt_str(row.display.clone()),
            render_opt_uuid(row.ident),
            render_opt_str(row.avatar.clone()),
            render_opt_float(row.total),
            render_opt_float(row.completed),
            render_opt_float(row.pending),
        )
    } else {
        format!(
            "{{\"label_name\":{},\"color\":{},\"label_id\":{},\
              \"total_estimates\":{},\"completed_estimates\":{},\
              \"pending_estimates\":{}}}",
            render_opt_str(row.display.clone()),
            render_opt_str(row.avatar.clone()),
            render_opt_uuid(row.ident),
            render_opt_float(row.total),
            render_opt_float(row.completed),
            render_opt_float(row.pending),
        )
    }
}

struct CountRows {
    first: Option<String>,
    last: Option<String>,
    ident: Option<uuid::Uuid>,
    avatar: Option<String>,
    display: Option<String>,
    total: i64,
    completed: i64,
    pending: i64,
}

async fn fetch_assignee_counts(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    cycle_id: &uuid::Uuid,
) -> Result<Vec<CountRows>, Denial> {
    let rows = sqlx::query(&assignee_count_sql())
        .bind(slug)
        .bind(project_id)
        .bind(cycle_id)
        .fetch_all(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        out.push(CountRows {
            first: row.try_get("first_name").map_err(|_| Denial::ServerError)?,
            last: row.try_get("last_name").map_err(|_| Denial::ServerError)?,
            ident: row
                .try_get("assignee_id")
                .map_err(|_| Denial::ServerError)?,
            avatar: row.try_get("avatar_url").map_err(|_| Denial::ServerError)?,
            display: row
                .try_get("display_name")
                .map_err(|_| Denial::ServerError)?,
            total: row
                .try_get("total_issues")
                .map_err(|_| Denial::ServerError)?,
            completed: row
                .try_get("completed_issues")
                .map_err(|_| Denial::ServerError)?,
            pending: row
                .try_get("pending_issues")
                .map_err(|_| Denial::ServerError)?,
        });
    }
    Ok(out)
}

/// One assignee issue-count row in serialization order
/// (`archive.py:502-508` + `:509-529`).
fn shape_assignee_count(row: &CountRows) -> String {
    format!(
        "{{\"first_name\":{},\"last_name\":{},\"assignee_id\":{},\
          \"avatar_url\":{},\"display_name\":{},\
          \"total_issues\":{},\"completed_issues\":{},\"pending_issues\":{}}}",
        render_opt_str(row.first.clone()),
        render_opt_str(row.last.clone()),
        render_opt_uuid(row.ident),
        render_opt_str(row.avatar.clone()),
        render_opt_str(row.display.clone()),
        row.total,
        row.completed,
        row.pending,
    )
}

struct LabelCountRows {
    name: Option<String>,
    color: Option<String>,
    ident: Option<uuid::Uuid>,
    total: i64,
    completed: i64,
    pending: i64,
}

async fn fetch_label_counts(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    cycle_id: &uuid::Uuid,
) -> Result<Vec<LabelCountRows>, Denial> {
    let rows = sqlx::query(&label_count_sql())
        .bind(slug)
        .bind(project_id)
        .bind(cycle_id)
        .fetch_all(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        out.push(LabelCountRows {
            name: row.try_get("label_name").map_err(|_| Denial::ServerError)?,
            color: row.try_get("color").map_err(|_| Denial::ServerError)?,
            ident: row.try_get("label_id").map_err(|_| Denial::ServerError)?,
            total: row
                .try_get("total_issues")
                .map_err(|_| Denial::ServerError)?,
            completed: row
                .try_get("completed_issues")
                .map_err(|_| Denial::ServerError)?,
            pending: row
                .try_get("pending_issues")
                .map_err(|_| Denial::ServerError)?,
        });
    }
    Ok(out)
}

/// One label issue-count row in serialization order
/// (`archive.py:541-544` + `:545-565`).
fn shape_label_count(row: &LabelCountRows) -> String {
    format!(
        "{{\"label_name\":{},\"color\":{},\"label_id\":{},\
          \"total_issues\":{},\"completed_issues\":{},\"pending_issues\":{}}}",
        render_opt_str(row.name.clone()),
        render_opt_str(row.color.clone()),
        render_opt_uuid(row.ident),
        row.total,
        row.completed,
        row.pending,
    )
}

// ---------------------------------------------------------------------------
// Burndown (`utils/analytics_plot.py:157-264`, cycle branch)
// ---------------------------------------------------------------------------

/// One chart value: Python `sum([])` is int `0` while any non-empty sum
/// is float, and future dates are `None` — the enum keeps that typing
/// exact.
#[derive(Debug, Clone, Copy, PartialEq)]
enum ChartValue {
    Int(i64),
    Float(f64),
    Null,
}

fn render_chart_value(value: ChartValue) -> String {
    match value {
        ChartValue::Int(number) => number.to_string(),
        ChartValue::Float(number) => render_float(number),
        ChartValue::Null => "null".to_owned(),
    }
}

fn chart_add(left: ChartValue, right: ChartValue) -> ChartValue {
    match (left, right) {
        (ChartValue::Int(left), ChartValue::Int(right)) => ChartValue::Int(left + right),
        (ChartValue::Int(left), ChartValue::Float(right)) => ChartValue::Float(left as f64 + right),
        (ChartValue::Float(left), ChartValue::Int(right)) => ChartValue::Float(left + right as f64),
        (ChartValue::Float(left), ChartValue::Float(right)) => ChartValue::Float(left + right),
        (ChartValue::Null, _) | (_, ChartValue::Null) => ChartValue::Null,
    }
}

fn chart_sub(total: ChartValue, completed: ChartValue) -> ChartValue {
    match (total, completed) {
        (ChartValue::Int(left), ChartValue::Int(right)) => ChartValue::Int(left - right),
        (ChartValue::Int(left), ChartValue::Float(right)) => ChartValue::Float(left as f64 - right),
        (ChartValue::Float(left), ChartValue::Int(right)) => ChartValue::Float(left - right as f64),
        (ChartValue::Float(left), ChartValue::Float(right)) => ChartValue::Float(left - right),
        (ChartValue::Null, _) | (_, ChartValue::Null) => ChartValue::Null,
    }
}

/// Cumulative pending per date over the inclusive UTC date range.
/// `completed` carries one entry per completion with Python's value
/// typing (ints on the issues path, floats on the points path; date
/// `None` = uncompleted rows, skipped like Python's
/// `item["date"] is not None` guard); `total` is the chart total with
/// Python's int-when-empty typing.
fn burndown_chart(
    total: ChartValue,
    completed: &[(Option<NaiveDate>, ChartValue)],
    start: NaiveDate,
    end: NaiveDate,
    today: NaiveDate,
) -> String {
    let mut out = String::from("{");
    let mut first = true;
    let mut date = start;
    while date <= end {
        if !first {
            out.push(',');
        }
        first = false;
        // Python sums left to right from int 0: `0 + first` keeps int
        // only while every addend is int; the accumulator below
        // reproduces that typing exactly.
        let mut done = ChartValue::Int(0);
        for (when, value) in completed {
            if let Some(day) = when {
                if *day <= date {
                    done = chart_add(done, *value);
                }
            }
        }
        let pending = chart_sub(total, done);
        let value = if date > today {
            ChartValue::Null
        } else {
            pending
        };
        out.push_str(&format!("\"{}\":{}", date, render_chart_value(value)));
        date = date.succ_opt().expect("date range");
    }
    out.push('}');
    out
}

fn parse_utc(value: &Value) -> Option<DateTime<Utc>> {
    match value {
        Value::String(text) => DateTime::parse_from_rfc3339(text)
            .ok()
            .map(|aware| aware.with_timezone(&Utc)),
        _ => None,
    }
}

async fn fetch_burndown_points(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    cycle_id: &uuid::Uuid,
) -> Result<Vec<(Option<NaiveDate>, ChartValue)>, Denial> {
    let rows = sqlx::query(&burndown_points_sql())
        .bind(slug)
        .bind(project_id)
        .bind(cycle_id)
        .fetch_all(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let day: Option<NaiveDate> = row.try_get("d").map_err(|_| Denial::ServerError)?;
        let value: Option<f64> = row.try_get("v").map_err(|_| Denial::ServerError)?;
        if let Some(number) = value {
            out.push((day, ChartValue::Float(number)));
        }
    }
    Ok(out)
}

async fn fetch_burndown_issues(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    cycle_id: &uuid::Uuid,
) -> Result<Vec<(Option<NaiveDate>, ChartValue)>, Denial> {
    let rows = sqlx::query(&burndown_issues_sql())
        .bind(slug)
        .bind(project_id)
        .bind(cycle_id)
        .fetch_all(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let day: Option<NaiveDate> = row.try_get("d").map_err(|_| Denial::ServerError)?;
        let count: Option<i64> = row.try_get("n").map_err(|_| Denial::ServerError)?;
        if let Some(number) = count {
            out.push((day, ChartValue::Int(number)));
        }
    }
    Ok(out)
}

/// Shape one row in the given key order; a missing key is a 500 (the
/// Python path would `KeyError` into the same generic branch).
fn shape_object(
    row: &Map<String, Value>,
    keys: &[&str],
    datetime_keys: &[&str],
) -> Result<String, Denial> {
    let mut out = String::from("{");
    for (index, key) in keys.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        let value = row.get(*key).ok_or(Denial::ServerError)?;
        out.push_str(&format!(
            "\"{}\":{}",
            key,
            splice_value(key, value, datetime_keys)
        ));
    }
    out.push('}');
    Ok(out)
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `GET archived-cycles/` (`archive.py:273-304`).
// `Response` is axum's handle type, so boxing it buys no runtime win;
// the crate-wide `Result<_, Denial>` helper shape stays as-is.
#[allow(clippy::result_large_err)]
async fn archived_list(
    State(state): State<AppState>,
    Path((slug, project_raw)): Path<(String, String)>,
    extension: Option<axum::Extension<SessionHandle>>,
) -> Result<Response, Denial> {
    let resolved = actor(&state, extension).await?;
    let pool = pool_of(&state)?.clone();
    let project_id = resolve_project_id(&pool, &slug, &project_raw).await?;
    let member = membership(&pool, &slug, &project_id, &resolved.id).await?;
    check_gate("GET", PATH_ARCHIVED_LIST, &slug, &member)?;
    let now = Utc::now();
    let rows = fetch_rows(
        &pool,
        &fetch_list_sql(),
        &slug,
        &project_id,
        &resolved.id,
        &now,
        None,
    )
    .await?;
    let mut out = String::from("[");
    for (index, row) in rows.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str(&shape_object(row, ARCHIVED_LIST_KEYS, LIST_DATETIME_KEYS)?);
    }
    out.push(']');
    Ok(json_response(StatusCode::OK, out))
}

/// `GET archived-cycles/<pk>/` (`archive.py:305-584`): a missing row is
/// the generic 500 (ported quirk Q4), not a 404.
#[allow(clippy::result_large_err)]
async fn archived_retrieve(
    State(state): State<AppState>,
    Path((slug, project_raw, pk_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<SessionHandle>>,
    req: axum::extract::Request,
) -> Result<Response, Denial> {
    // Routing first (Django's `<uuid:pk>` 404 precedes auth).
    let pk = match pk_raw.parse::<uuid::Uuid>() {
        Ok(id) => id,
        Err(_) => return Ok(crate::edge::proxy(State(state), req).await),
    };
    let resolved = actor(&state, extension).await?;
    let pool = pool_of(&state)?.clone();
    let project_id = resolve_project_id(&pool, &slug, &project_raw).await?;
    let member = membership(&pool, &slug, &project_id, &resolved.id).await?;
    check_gate("GET", PATH_ARCHIVED_DETAIL, &slug, &member)?;
    let now = Utc::now();
    let rows = fetch_rows(
        &pool,
        &fetch_detail_sql(),
        &slug,
        &project_id,
        &resolved.id,
        &now,
        Some(&pk),
    )
    .await?;
    let row = rows.into_iter().next().ok_or(Denial::ServerError)?;
    let mut out = shape_object(&row, ARCHIVED_DETAIL_KEYS, DETAIL_DATETIME_KEYS)?;

    let estimate_points = project_estimates_points(&pool, &slug, &project_id).await?;
    let start = parse_utc(row.get("start_date").ok_or(Denial::ServerError)?);
    let end = parse_utc(row.get("end_date").ok_or(Denial::ServerError)?);
    let total_issues = row
        .get("total_issues")
        .and_then(Value::as_i64)
        .ok_or(Denial::ServerError)?;
    let today = now.date_naive();

    // `estimate_distribution` only when the project estimates in points
    // (`:358-363`); else `{}` (`:365`).
    let mut estimate_distribution = String::from("{}");
    if estimate_points {
        let assignees = fetch_estimate_rows(
            &pool,
            &assignee_estimate_sql(),
            &slug,
            &project_id,
            &pk,
            true,
        )
        .await?;
        let labels =
            fetch_estimate_rows(&pool, &label_estimate_sql(), &slug, &project_id, &pk, false)
                .await?;
        let mut chart = String::from("{}");
        if let (Some(start), Some(end)) = (start, end) {
            let points = fetch_burndown_points(&pool, &slug, &project_id, &pk).await?;
            // Python `sum(issue_estimates)` starts at int 0 and adds in
            // queryset order — the fold below is that sum, typed.
            let total = points
                .iter()
                .fold(ChartValue::Int(0), |acc, (_, value)| chart_add(acc, *value));
            chart = burndown_chart(total, &points, start.date_naive(), end.date_naive(), today);
        }
        let mut assignees_out = String::from("[");
        for (index, row) in assignees.iter().enumerate() {
            if index > 0 {
                assignees_out.push(',');
            }
            assignees_out.push_str(&shape_estimate_row(row, true));
        }
        assignees_out.push(']');
        let mut labels_out = String::from("[");
        for (index, row) in labels.iter().enumerate() {
            if index > 0 {
                labels_out.push(',');
            }
            labels_out.push_str(&shape_estimate_row(row, false));
        }
        labels_out.push(']');
        estimate_distribution = format!(
            "{{\"assignees\":{},\"labels\":{},\"completion_chart\":{}}}",
            assignees_out, labels_out, chart
        );
    }

    let assignees = fetch_assignee_counts(&pool, &slug, &project_id, &pk).await?;
    let labels = fetch_label_counts(&pool, &slug, &project_id, &pk).await?;
    let mut chart = String::from("{}");
    if let (Some(start), Some(end)) = (start, end) {
        let issues = fetch_burndown_issues(&pool, &slug, &project_id, &pk).await?;
        chart = burndown_chart(
            ChartValue::Int(total_issues),
            &issues,
            start.date_naive(),
            end.date_naive(),
            today,
        );
    }
    let mut assignees_out = String::from("[");
    for (index, row) in assignees.iter().enumerate() {
        if index > 0 {
            assignees_out.push(',');
        }
        assignees_out.push_str(&shape_assignee_count(row));
    }
    assignees_out.push(']');
    let mut labels_out = String::from("[");
    for (index, row) in labels.iter().enumerate() {
        if index > 0 {
            labels_out.push(',');
        }
        labels_out.push_str(&shape_label_count(row));
    }
    labels_out.push(']');
    let distribution = format!(
        "{{\"assignees\":{},\"labels\":{},\"completion_chart\":{}}}",
        assignees_out, labels_out, chart
    );

    // `data["estimate_distribution"]` / `data["distribution"]` append
    // after `archived_at` (`:455-459`, `:569-573`).
    out.pop();
    out.push_str(&format!(
        ",\"estimate_distribution\":{},\"distribution\":{}}}",
        estimate_distribution, distribution
    ));
    Ok(json_response(StatusCode::OK, out))
}

/// `timezone.now()` truncated to microseconds: Postgres `timestamptz`
/// stores micros, and Python datetimes carry micros at most — the
/// response renders the assigned value, so nanos would leak a
/// 9-digit fraction Python never emits.
fn utc_now_micros() -> DateTime<Utc> {
    let now = Utc::now();
    let micros = now.timestamp() * 1_000_000 + i64::from(now.timestamp_subsec_micros());
    DateTime::from_timestamp(
        micros.div_euclid(1_000_000),
        (micros.rem_euclid(1_000_000) as u32) * 1000,
    )
    .expect("now")
}

/// `Cycle.objects.get(pk, project_id, workspace__slug)` (`archive.py:588`,
/// `:608`): the soft-deletion manager, no archived predicates. A miss
/// raises `DoesNotExist` → the `ObjectDoesNotExist` 404.
async fn fetch_cycle_for_write(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    cycle_id: &uuid::Uuid,
) -> Result<Option<DateTime<Utc>>, Denial> {
    let row: Option<(Option<DateTime<Utc>>,)> = sqlx::query_as(
        "SELECT c.end_date FROM cycles c \
         JOIN workspaces w ON w.id = c.workspace_id \
         WHERE c.id = $1 AND c.project_id = $2 AND w.slug = $3 \
           AND c.deleted_at IS NULL",
    )
    .bind(cycle_id)
    .bind(project_id)
    .bind(slug)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    match row {
        None => Err(Denial::NotFound),
        Some((end_date,)) => Ok(end_date),
    }
}

/// `POST cycles/<cycle_id>/archive/` (`archive.py:586-604`).
#[allow(clippy::result_large_err)]
async fn archive_post(
    State(state): State<AppState>,
    Path((slug, project_raw, cycle_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<SessionHandle>>,
    req: axum::extract::Request,
) -> Result<Response, Denial> {
    // Routing first (Django's `<uuid:cycle_id>` 404 precedes auth).
    let cycle_id = match cycle_raw.parse::<uuid::Uuid>() {
        Ok(id) => id,
        Err(_) => return Ok(crate::edge::proxy(State(state), req).await),
    };
    let resolved = actor(&state, extension).await?;
    let pool = pool_of(&state)?.clone();
    let project_id = resolve_project_id(&pool, &slug, &project_raw).await?;
    let member = membership(&pool, &slug, &project_id, &resolved.id).await?;
    check_gate("POST", PATH_ARCHIVE, &slug, &member)?;
    let end_date = fetch_cycle_for_write(&pool, &slug, &project_id, &cycle_id).await?;
    // Q2: `None >= now` raises `TypeError` → generic 500, not the guard.
    let end_date = end_date.ok_or(Denial::ServerError)?;
    let now = utc_now_micros();
    if end_date >= now {
        return Err(Denial::ArchiveRefused);
    }
    sqlx::query(
        "UPDATE cycles SET archived_at = $1, updated_at = $1, updated_by_id = $2 \
         WHERE id = $3",
    )
    .bind(now)
    .bind(resolved.id)
    .bind(cycle_id)
    .execute(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    // The soft-deletion manager's queryset `delete()` (`db/mixins.py:48-53`):
    // `UPDATE deleted_at`, not a hard delete.
    sqlx::query(
        "UPDATE user_favorites SET deleted_at = $1 FROM workspaces w \
         WHERE user_favorites.workspace_id = w.id AND w.slug = $2 \
           AND user_favorites.entity_type = 'cycle' \
           AND user_favorites.entity_identifier = $3 \
           AND user_favorites.project_id = $4 \
           AND user_favorites.deleted_at IS NULL",
    )
    .bind(now)
    .bind(&slug)
    .bind(cycle_id)
    .bind(project_id)
    .execute(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(json_response(
        StatusCode::OK,
        format!(
            "{{\"archived_at\":{}}}",
            json_string(&render_python_datetime(&now))
        ),
    ))
}

/// `DELETE cycles/<cycle_id>/archive/` (`archive.py:606-611`): no date
/// guard — unarchive is `archived_at = None` + save + 204.
#[allow(clippy::result_large_err)]
async fn unarchive_delete(
    State(state): State<AppState>,
    Path((slug, project_raw, cycle_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<SessionHandle>>,
    req: axum::extract::Request,
) -> Result<Response, Denial> {
    // Routing first (Django's `<uuid:cycle_id>` 404 precedes auth).
    let cycle_id = match cycle_raw.parse::<uuid::Uuid>() {
        Ok(id) => id,
        Err(_) => return Ok(crate::edge::proxy(State(state), req).await),
    };
    let resolved = actor(&state, extension).await?;
    let pool = pool_of(&state)?.clone();
    let project_id = resolve_project_id(&pool, &slug, &project_raw).await?;
    let member = membership(&pool, &slug, &project_id, &resolved.id).await?;
    check_gate("DELETE", PATH_ARCHIVE, &slug, &member)?;
    // The `.get()` still runs (and 404s when missing) before the save.
    fetch_cycle_for_write(&pool, &slug, &project_id, &cycle_id).await?;
    let now = utc_now_micros();
    sqlx::query(
        "UPDATE cycles SET archived_at = NULL, updated_at = $1, updated_by_id = $2 \
         WHERE id = $3",
    )
    .bind(now)
    .bind(resolved.id)
    .bind(cycle_id)
    .execute(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(json_response(StatusCode::NO_CONTENT, String::new()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Timelike};

    fn utc(
        year: i32,
        month: u32,
        day: u32,
        hour: u32,
        min: u32,
        sec: u32,
        micros: u32,
    ) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(year, month, day, hour, min, sec)
            .unwrap()
            .with_nanosecond(micros * 1000)
            .unwrap()
    }

    #[test]
    fn gate_table_covers_all_five_archive_rows() {
        // Every owned method+path resolves, all ADMIN/MEMBER project gates
        // (F-C27-07: `archive.py:271/:586/:606`).
        for (method, path) in [
            ("POST", PATH_ARCHIVE),
            ("DELETE", PATH_ARCHIVE),
            ("GET", PATH_ARCHIVED_LIST),
            ("GET", PATH_ARCHIVED_DETAIL),
        ] {
            let row = super::super::gates::gate_for(method, path)
                .unwrap_or_else(|| panic!("gate row {method} {path}"));
            match row.gate {
                super::super::gates::Gate::Project { roles } => {
                    assert_eq!(roles, &[ROLE_ADMIN, ROLE_MEMBER], "{method} {path}");
                }
                other => panic!("unexpected gate {other:?} for {method} {path}"),
            }
        }
        // The GET list quirk on the archive path has its own gate row and
        // stays proxy-served (see `routes`).
        assert!(super::super::gates::gate_for("GET", PATH_ARCHIVE).is_some());
    }

    #[test]
    fn archive_facts_allow_admin_member_deny_guest() {
        let scope = super::super::gates::tenant_context("acme");
        let allow = |workspace_role, project_role| {
            let member = Membership {
                workspace_role,
                project_role,
            };
            super::super::gates::decide_gate(
                &super::super::gates::Gate::Project {
                    roles: &[ROLE_ADMIN, ROLE_MEMBER],
                },
                &scope,
                &archive_facts("acme", &member),
            )
        };
        use super::super::gates::GateOutcome as O;
        assert_eq!(allow(Some(20), Some(20)), O::Allow);
        assert_eq!(allow(Some(15), Some(15)), O::Allow);
        // Workspace admin on the project passes with a guest project role.
        assert_eq!(allow(Some(20), Some(5)), O::Allow);
        assert_eq!(allow(Some(5), Some(5)), O::Deny);
        // The decorator checks project membership only on this path —
        // workspace membership is not required alongside it.
        assert_eq!(allow(None, Some(15)), O::Allow);
        assert_eq!(allow(Some(15), None), O::Deny);
        // Cross-workspace facts deny even with roles.
        let member = Membership {
            workspace_role: Some(20),
            project_role: Some(20),
        };
        let other = super::super::gates::tenant_context("other");
        assert_eq!(
            super::super::gates::decide_gate(
                &super::super::gates::Gate::Project {
                    roles: &[ROLE_ADMIN, ROLE_MEMBER],
                },
                &other,
                &archive_facts("acme", &member),
            ),
            O::Deny
        );
    }

    #[test]
    fn list_keys_match_values_order() {
        // Wire order verified against live Django 2026-09-30: model
        // fields in `.values()` argument order, then annotations in
        // definition order — NOT the `.values()` argument order.
        assert_eq!(
            ARCHIVED_LIST_KEYS,
            &[
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
                "archived_at",
                "is_favorite",
                "total_issues",
                "completed_issues",
                "cancelled_issues",
                "started_issues",
                "unstarted_issues",
                "backlog_issues",
                "status",
                "assignee_ids",
            ]
        );
    }

    #[test]
    fn detail_keys_match_values_order() {
        // Same rule; the detail `.annotate(sub_issues=...)` runs after
        // the base annotations, so `sub_issues` is second-to-last.
        assert_eq!(
            ARCHIVED_DETAIL_KEYS,
            &[
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
                "created_by",
                "archived_at",
                "is_favorite",
                "total_issues",
                "completed_issues",
                "cancelled_issues",
                "started_issues",
                "unstarted_issues",
                "backlog_issues",
                "status",
                "assignee_ids",
                "completed_estimate_points",
                "total_estimate_points",
                "sub_issues",
            ]
        );
    }

    #[test]
    fn integral_floats_gain_dot_zero() {
        // Postgres `float8`/`numeric` text has no fraction on integral
        // values; DRF renders the Python float with one.
        let int = |n: i64| render_float_value(&serde_json::json!(n));
        assert_eq!(int(65535), "65535.0");
        assert_eq!(int(0), "0.0");
        assert_eq!(render_float_value(&serde_json::json!(21.5)), "21.5");
        assert_eq!(render_float_value(&Value::Null), "null");
    }

    #[test]
    fn stored_datetimes_render_in_utc_without_zone_shift() {
        // DRF `JSONEncoder`: isoformat, `+00:00` → `Z`, micros iff
        // nonzero — never the request zone (no serializer runs here).
        let stored = |text: &str| render_stored_datetime(&Value::String(text.to_owned()));
        assert_eq!(
            stored("2026-09-30T04:00:00+00:00"),
            "\"2026-09-30T04:00:00Z\""
        );
        assert_eq!(
            stored("2026-09-30T04:00:00.123456+00:00"),
            "\"2026-09-30T04:00:00.123456Z\""
        );
        // Pool session zones only affect the text: the instant renders UTC.
        assert_eq!(
            stored("2026-09-29T21:00:00.123456-07:00"),
            "\"2026-09-30T04:00:00.123456Z\""
        );
        assert_eq!(
            stored("2026-09-29T21:00:00-07:00"),
            "\"2026-09-30T04:00:00Z\""
        );
        assert_eq!(render_stored_datetime(&Value::Null), "null");
    }

    #[test]
    fn python_str_datetimes_keep_space_and_offset() {
        // `str(cycle.archived_at)`: space separator, `+00:00`, micros
        // iff nonzero.
        assert_eq!(
            render_python_datetime(&utc(2026, 9, 30, 4, 0, 0, 0)),
            "2026-09-30 04:00:00+00:00"
        );
        assert_eq!(
            render_python_datetime(&utc(2026, 9, 30, 4, 0, 0, 123456)),
            "2026-09-30 04:00:00.123456+00:00"
        );
    }

    #[test]
    fn burndown_keeps_python_int_float_typing() {
        use chrono::NaiveDate;
        let day = |y: i32, m: u32, d: u32| NaiveDate::from_ymd_opt(y, m, d).unwrap();
        // Empty range renders `{}`.
        assert_eq!(
            burndown_chart(
                ChartValue::Int(0),
                &[],
                day(2026, 9, 2),
                day(2026, 9, 1),
                day(2026, 9, 30)
            ),
            "{}"
        );
        // Issues path: ints throughout; future dates null.
        let done = [(Some(day(2026, 9, 1)), ChartValue::Int(2))];
        assert_eq!(
            burndown_chart(
                ChartValue::Int(7),
                &done,
                day(2026, 9, 1),
                day(2026, 9, 3),
                day(2026, 9, 2)
            ),
            "{\"2026-09-01\":5,\"2026-09-02\":5,\"2026-09-03\":null}"
        );
        // Points path with no estimates: int zeros, never 0.0.
        assert_eq!(
            burndown_chart(
                ChartValue::Int(0),
                &[],
                day(2026, 9, 1),
                day(2026, 9, 1),
                day(2026, 9, 30)
            ),
            "{\"2026-09-01\":0}"
        );
        // Points path with estimates: floats; uncompleted (`None` date)
        // rows never enter the sums.
        let done = [
            (Some(day(2026, 9, 1)), ChartValue::Float(1.5)),
            (None, ChartValue::Float(100.0)),
        ];
        assert_eq!(
            burndown_chart(
                ChartValue::Float(4.5),
                &done,
                day(2026, 9, 1),
                day(2026, 9, 2),
                day(2026, 9, 30)
            ),
            "{\"2026-09-01\":3.0,\"2026-09-02\":3.0}"
        );
    }

    #[test]
    fn sql_binds_number_in_order() {
        // List binds $1..$4 (slug, project, user, now); detail adds $5.
        for needle in [
            "w.slug = $1",
            "c.project_id = $2",
            "pm.member_id = $3",
            "<= $4",
        ] {
            assert!(fetch_list_sql().contains(needle), "list {needle}");
        }
        assert!(!fetch_list_sql().contains("$5"));
        assert!(fetch_detail_sql().contains("c.id = $5"));
        // Distributions bind exactly three (slug, project, cycle).
        for sql in [
            assignee_estimate_sql(),
            label_estimate_sql(),
            assignee_count_sql(),
            label_count_sql(),
            burndown_points_sql(),
            burndown_issues_sql(),
        ] {
            assert!(sql.contains("ci.cycle_id = $3"), "cycle bind");
            assert!(!sql.contains("$4"), "no fourth bind");
            assert!(!sql.contains("$5"), "no fifth bind");
        }
    }

    #[test]
    fn sql_ports_the_archive_asymmetries() {
        // Q3: no bridge deleted guard on the archive counts / arrays.
        assert!(!archived_list_sql().contains("ci.deleted_at"));
        // The estimate subqueries keep the live-bridge guard they always had.
        assert!(estimate_annotation(None, "x").contains("ci.deleted_at IS NULL"));
        // Q5: the `issue_objects` manager exclusions on the estimate path,
        // with the `exclude()` NULL rule (NULL-state rows kept).
        let estimate = estimate_annotation(Some("completed"), "x");
        assert!(estimate.contains("(st.\"group\" != 'triage' OR st.\"group\" IS NULL)"));
        assert!(estimate.contains("i.archived_at IS NULL"));
        assert!(estimate.contains("ip.archived_at IS NULL"));
        // Plain `=` group predicates on archive (never the base `IN`).
        assert!(estimate.contains("st.\"group\" = 'completed'"));
        assert!(!estimate.contains("IN ("));
        // Archived-only scope; effective list order, not queryset order.
        assert!(archived_list_sql().contains("c.archived_at IS NOT NULL"));
        assert!(archived_list_sql().contains("ORDER BY is_favorite DESC, c.created_at DESC"));
    }

    #[test]
    fn denial_bodies_are_byte_exact() {
        assert_eq!(
            UNAUTHENTICATED_BODY,
            "{\"detail\":\"Authentication credentials were not provided.\"}"
        );
        assert_eq!(
            NOT_FOUND_BODY,
            "{\"error\":\"The required object does not exist.\"}"
        );
        assert_eq!(PROJECT_NOT_FOUND_BODY, "{\"detail\":\"Project not found\"}");
        assert_eq!(
            SERVER_ERROR_BODY,
            "{\"error\":\"Something went wrong please try again later\"}"
        );
        assert_eq!(
            ARCHIVE_REFUSED_BODY,
            "{\"error\":\"Only completed cycles can be archived\"}"
        );
    }
}

#[cfg(test)]
mod pidashconv_736_tests {
    use super::normalize_resolve_identifier;

    #[test]
    fn resolve_identifier_strips_py_whitespace() {
        assert_eq!(normalize_resolve_identifier("  eng "), "ENG");
        // Python `str.strip()` also strips U+001C-U+001F (PIDASHCONV-736):
        // `%1C`-padded identifiers must resolve, not 404.
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
    }
}
