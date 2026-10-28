#![forbid(unsafe_code)]

//! Issue version + description-version + move handlers (D-26, stage 5, PIDASHCONV-657).
//!
//! Ports `app/views/issue/version.py` (2 units, 144 lines) and
//! `app/views/issue/move.py` (1 unit, 46 lines):
//!
//! - `GET issues/<issue_id>/versions/` (`IssueVersionEndpoint.get`,
//!   `version.py:36-74`): the 9-key `paginate` envelope over 10-key rows
//!   (`required_fields`, model `-created_at` order). No parent checks at
//!   all — a missing issue is an empty page, not a 404.
//! - `GET issues/<issue_id>/versions/<pk>/` (`:38-44`): the 30-key
//!   `IssueVersionDetailSerializer` row (the dup-`name` collapses to one,
//!   first position — the 640 finding).
//! - `GET work-items/<work_item_id>/description-versions/`
//!   (`WorkItemDescriptionVersionEndpoint.get`, `:86-144`):
//!   `Project.get` + `Issue.get` (404s), the guest-view 403 gate, then
//!   the same 9-key envelope over the explicit
//!   `.order_by("-created_at")` list.
//! - `GET work-items/<work_item_id>/description-versions/<pk>/`
//!   (`:107-116`): the 14-key `IssueDescriptionVersionDetailSerializer`
//!   row (`description_binary` renders base64, null stays null).
//! - `POST work-items/<pk>/move/` (`IssueMoveEndpoint.post`,
//!   `move.py:28-46`): the ADMIN/MEMBER gate, the non-dict-body `{}` coercion,
//!   `domain::issues::move_to_project` over the 650 kernel, `MoveRejection`
//!   mapped to `{"error"}` + status, the 594 signal pair and the enqueues
//!   folded into the commit chain around the state-changing save, then the
//!   22-key APP `IssueSerializer` read shape (a bare instance: the 7
//!   annotation keys are `SkipField`-omitted, the 639 rule, verified live).
//!
//! Every other method on those paths proxies to Django (route table in
//! `super::routes` via [`routes`]); `HEAD` rides axum's `get` handling
//! on the GET-owned paths like Django's GET-backed `HEAD`.
//!
//! Reuse (call, don't copy): the 649 version SQL builders
//! (`super::queries_engage`), pilot-2's `v2_page` cursor math
//! (`super::render`), the 640 version detail rendering
//! (`serializers_assoc`), the 639 base-shape rendering
//! (`serializers_detail`), the move domain service
//! (`domain::issues::move_to_project`, with the 594 fire, the D-14
//! pod drains and the D-12 creation drain behind its hooks), and the
//! shared DRF body negotiator (`v1_cycles_modules::body`).
//!
//! # Ported bugs (translate, don't redesign — also listed in the PR)
//!
//! * The versions list never 404s: a missing issue (or project, past the
//!   gate) is an empty 200 page (`filter`, never `get`).
//! * `IssueDescriptionVersion` declares no `Meta.ordering`, so the detail
//!   `.get()` carries no `ORDER BY` (unlike its version sibling) — the
//!   649 SQL preserves the asymmetry.
//! * `PaginateCursor`'s offset part is parsed but never read (`start_index`
//!   derives from `current_page` alone) — inherited from pilot-2's
//!   `v2_page`, reused, not re-ported.
//! * The move's `now` is drawn once per request (Python draws it inside
//!   the transaction, after the locks); the 650 driver takes it frozen.
//! * A non-finite `sort_order` answers 500 (DRF renders with
//!   `allow_nan=False` under the default `STRICT_JSON=True`).
//!
//! # Known gaps (documented, not silent)
//!
//! * Past a ticking-state transition the 594 fire needs the same runtime
//!   (ticker reconcile, entry-run dispatch); the seam answers store
//!   errors there, which the fire swallows by design (counter + the
//!   verbatim log line) while the move stands. Non-trigger transitions
//!   (every gate seed, the backlog norm) run no seam I/O past the two
//!   state lookups and are fully faithful.
//! * `v2_page` parses cursor ints strictly (Rust `parse`), where Python's
//!   `int()` accepts whitespace/signs/underscores — pilot-2 shared code,
//!   reused, not re-ported.
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Router;
use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use serde_json::{Map, Value};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use pidash_services::app_issues::serializers_assoc::{
    issue_description_version_detail_to_representation, issue_version_detail_to_representation,
    IssueDescriptionVersionDetailRow, IssueVersionDetailRow,
};
use pidash_services::app_issues::{
    issue_detail_base_to_representation, issue_is_actively_synced, IssueDetailBaseRow,
    SourceIssueRow, GITHUB_ISSUE_SYNC_PROBE_SQL, GIT_ISSUE_SYNC_PROBE_SQL,
};
use pidash_services::domain;
use pidash_services::domain::activity::TaskEnqueueHook;
use pidash_services::domain::issues as domain_issues;
use pidash_services::domain::issues::{
    CreationDeps, MoveDomainOutcome, MovePostCommitHook, MoveRejection,
};
use pidash_services::domain::transition::IssueTransitionHook;
use pidash_services::domain::{WriteActor, WriteCtx, WriteScope};

use super::queries_engage::{
    description_count_sql, description_detail_sql, description_list_sql, version_count_sql,
    version_detail_sql, version_list_sql, VERSION_PAGE_KEYS,
};
use super::render::{v2_page, V2Page};
use super::{query_last, Binder, QueryMap};
use crate::domain_hooks::{AppMoveAdmissionCache, AppMovePostCommitHook, AppTaskEnqueueHook};
use crate::issue_transition::AppTransitionHook;
use crate::middleware::body_limit::read_request_bytes;
use crate::orchestration::SpanCreationDeps;
use crate::state::AppState;
use crate::v1_cycles_modules::body as shared_body;
use crate::v1_cycles_modules::json::{
    parse_json_text, to_serde_publish, JsonFail, JSON_PARSE_PREFIX,
};
use pidash_compat::datetime::{render_date, render_datetime_in, render_uuid_opt, uuid_string};
use pidash_compat::response::escape_u2028 as escape_json_text;

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------

/// `GET .../issues/<issue_id>/versions/` (`urls/issue.py:279-283`).
pub const VERSIONS_PATH: &str =
    "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/versions/";
/// `GET .../issues/<issue_id>/versions/<pk>/` (`urls/issue.py:284-288`).
pub const VERSION_PATH: &str =
    "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/versions/{pk}/";
/// `GET .../work-items/<work_item_id>/description-versions/` (`:289-293`).
pub const DESC_VERSIONS_PATH: &str =
    "/api/workspaces/{slug}/projects/{project_id}/work-items/{work_item_id}/description-versions/";
/// `GET .../work-items/<work_item_id>/description-versions/<pk>/` (`:294-298`).
pub const DESC_VERSION_PATH: &str =
    "/api/workspaces/{slug}/projects/{project_id}/work-items/{work_item_id}/description-versions/{pk}/";
/// `POST .../work-items/<pk>/move/` (`:309-313`,
/// `http_method_names=["post"]`).
pub const MOVE_PATH: &str = "/api/workspaces/{slug}/projects/{project_id}/work-items/{pk}/move/";

/// Register the five versions/move routes. Nothing else: sibling paths
/// stay unmatched and proxy to Django.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            VERSIONS_PATH,
            owned(axum::routing::get(version_list), &["GET"]),
        )
        .route(
            VERSION_PATH,
            owned(axum::routing::get(version_detail), &["GET"]),
        )
        .route(
            DESC_VERSIONS_PATH,
            owned(axum::routing::get(desc_version_list), &["GET"]),
        )
        .route(
            DESC_VERSION_PATH,
            owned(axum::routing::get(desc_version_detail), &["GET"]),
        )
        .route(MOVE_PATH, owned(axum::routing::post(move_issue), &["POST"]))
}

/// A versions/move path: the owned methods serve from Rust, everything
/// else falls through to Django (the engage cutover shape — DRF
/// authenticates before it checks the method, so proxying reproduces
/// the 401-anon and Django's own 405s with no per-method logic).
fn owned(
    methods: axum::routing::MethodRouter<AppState>,
    owned: &[&str],
) -> axum::routing::MethodRouter<AppState> {
    let mut router = methods;
    for other in [
        "GET", "POST", "PUT", "PATCH", "DELETE", "OPTIONS", "HEAD", "TRACE",
    ] {
        if owned.contains(&other) {
            continue;
        }
        router = match other {
            "GET" => router.get(crate::edge::proxy),
            "POST" => router.post(crate::edge::proxy),
            "PUT" => router.put(crate::edge::proxy),
            "PATCH" => router.patch(crate::edge::proxy),
            "DELETE" => router.delete(crate::edge::proxy),
            "HEAD" => router.head(crate::edge::proxy),
            "TRACE" => router.trace(crate::edge::proxy),
            _ => router.options(crate::edge::proxy),
        };
    }
    router
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Handler failure with its exact status + body.
#[derive(Debug)]
enum Denial {
    /// 401, DRF `NotAuthenticated`.
    Unauthorized,
    /// 403, `@allow_permission` body.
    Forbidden,
    /// 403, the description-versions guest-view gate
    /// (`version.py:102-105`).
    GuestView,
    /// 404, `handle_exception`'s `ObjectDoesNotExist` branch.
    NotFound,
    /// 404, `{"detail":"Project not found"}` (project-kwarg rewrite miss
    /// and the move's `Project.resolve` miss — DRF propagates `Http404`
    /// args via `NotFound(*exc.args)`).
    ProjectNotFound,
    /// 400, `{"detail": ...}` (JSON `ParseError`).
    BadDetail(String),
    /// 400/403/409, `{"error": ...}` (the move's `IssueMoveError` mapping).
    MoveRejection { status: u16, message: String },
    /// 415, `UnsupportedMediaType` (content negotiation).
    UnsupportedMediaType(String),
    /// 502, truncated request-body read (the shared bad-gateway shape).
    TruncatedBody,
    /// 500, generic branch.
    ServerError,
}

/// The description-versions guest-view refusal
/// (`version.py:102-105`).
const GUEST_VIEW_BODY: &str = r#"{"error":"You are not allowed to view this issue"}"#;

impl Denial {
    fn status_and_body(&self) -> (StatusCode, String) {
        match self {
            Denial::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                super::UNAUTHENTICATED_BODY.to_owned(),
            ),
            Denial::Forbidden => (
                StatusCode::FORBIDDEN,
                crate::permissions::PERMISSION_DENIED_BODY.to_owned(),
            ),
            Denial::GuestView => (StatusCode::FORBIDDEN, GUEST_VIEW_BODY.to_owned()),
            Denial::NotFound => (StatusCode::NOT_FOUND, super::NOT_FOUND_BODY.to_owned()),
            Denial::ProjectNotFound => (
                StatusCode::NOT_FOUND,
                super::PROJECT_NOT_FOUND_BODY.to_owned(),
            ),
            Denial::BadDetail(message) => (
                StatusCode::BAD_REQUEST,
                format!("{{\"detail\":{}}}", json_string(message)),
            ),
            Denial::MoveRejection { status, message } => (
                StatusCode::from_u16(*status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
                format!("{{\"error\":{}}}", json_string(message)),
            ),
            Denial::UnsupportedMediaType(message) => (
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                format!("{{\"detail\":{}}}", json_string(message)),
            ),
            Denial::TruncatedBody => (
                StatusCode::BAD_GATEWAY,
                crate::edge::BAD_GATEWAY_BODY.to_owned(),
            ),
            Denial::ServerError => (
                StatusCode::INTERNAL_SERVER_ERROR,
                super::SERVER_ERROR_BODY.to_owned(),
            ),
        }
    }
}

impl IntoResponse for Denial {
    fn into_response(self) -> Response {
        if matches!(self, Denial::ServerError) {
            tracing::warn!("app_issues versions/move handler: internal error");
        }
        let (status, body) = self.status_and_body();
        Response::builder()
            .status(status)
            .header(header::CONTENT_TYPE, "application/json")
            .body(axum::body::Body::from(escape_json_text(&body)))
            .expect("versions/move error response")
    }
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).expect("json string")
}

/// Render `body` (already exact JSON bytes) with an explicit status,
/// escaped through the canonical `escape_u2028` (OVL-04).
fn json_response(status: StatusCode, body: String) -> Response {
    pidash_compat::response::json_response(status, escape_json_text(&body))
}

type HandlerResult = Result<Response, Denial>;

// ---------------------------------------------------------------------------
// Request plumbing: auth + project rewrite + permission gates
// ---------------------------------------------------------------------------

/// `request.user` from the Django session (`_auth_user_id`). No session,
/// no key, a non-UUID id, or a session pointing at no user row means
/// anonymous → 401. (Django PKs are UUIDs; a session id that is not a
/// UUID cannot be a user.)
async fn actor_user_id(
    pool: &PgPool,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Result<Uuid, Denial> {
    let handle = extension.ok_or(Denial::Unauthorized)?.0;
    let mut session = handle.snapshot();
    let raw = session
        .get("_auth_user_id")
        .and_then(|value| value.as_str().to_owned())
        .ok_or(Denial::Unauthorized)?;
    let id = raw.parse::<Uuid>().map_err(|_| Denial::Unauthorized)?;
    let exists: Option<(i32,)> = sqlx::query_as("SELECT 1 FROM users WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    if exists.is_none() {
        return Err(Denial::Unauthorized);
    }
    Ok(id)
}

/// `_rewrite_project_kwarg` (`app/views/base.py:49-80`): UUIDs pass
/// through unchecked; other identifiers resolve `UPPER(identifier)` in
/// the workspace, else `Http404("Project not found")`.
async fn resolve_project_id(pool: &PgPool, slug: &str, raw: &str) -> Result<Uuid, Denial> {
    if let Ok(id) = raw.parse::<Uuid>() {
        return Ok(id);
    }
    let upper = raw.trim().to_uppercase();
    let row: Option<(Uuid,)> = sqlx::query_as(
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

/// Parse a `<uuid:>` path segment the way Django's `UUIDConverter` does:
/// lowercase hex, hyphenated, 36 chars (`django/urls/converters.py:25`).
/// Uppercase / simple / braced / `urn:` spellings never route in Django
/// (resolver 404), so they proxy here instead of serving.
fn path_uuid(raw: &str) -> Result<Uuid, ()> {
    if raw.len() != 36 {
        return Err(());
    }
    let strict = raw.bytes().enumerate().all(|(index, byte)| {
        if index == 8 || index == 13 || index == 18 || index == 23 {
            byte == b'-'
        } else {
            byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()
        }
    });
    if !strict {
        return Err(());
    }
    raw.parse::<Uuid>().map_err(|_| ())
}

/// Membership facts for one `(user, slug, project)` over the same rows
/// the decorator reads: active project and workspace memberships scoped
/// by slug/project, soft-deleted rows excluded (`SoftDeletionManager`).
struct Membership {
    project_role: Option<i16>,
    workspace_role: Option<i16>,
}

async fn fetch_membership(
    pool: &PgPool,
    slug: &str,
    project_id: &Uuid,
    user_id: &Uuid,
) -> Result<Membership, Denial> {
    let project_role: Option<(i16,)> = sqlx::query_as(
        r#"SELECT pm.role FROM project_members pm
           JOIN workspaces w ON w.id = pm.workspace_id
           WHERE pm.member_id = $1 AND pm.project_id = $2 AND w.slug = $3
           AND pm.is_active AND pm.deleted_at IS NULL"#,
    )
    .bind(user_id)
    .bind(project_id)
    .bind(slug)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let workspace_role: Option<(i16,)> = sqlx::query_as(
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
    Ok(Membership {
        project_role: project_role.map(|row| row.0),
        workspace_role: workspace_role.map(|row| row.0),
    })
}

/// `allow_permission(allowed_roles)` at `PROJECT` level
/// (`app/permissions/base.py:19-86`): an allowed project role, or any
/// active project membership plus an active workspace ADMIN membership.
/// Denies with the decorator `error` body.
fn check_allow(membership: &Membership, allowed: &[i16]) -> Result<(), Denial> {
    if membership
        .project_role
        .is_some_and(|role| allowed.contains(&role))
    {
        return Ok(());
    }
    if membership.project_role.is_some() && membership.workspace_role == Some(20) {
        return Ok(());
    }
    Err(Denial::Forbidden)
}

/// View-body tenant facts: the actor's render timezone
/// (`TimezoneMixin` activation; a bad zone or a missing user row is a
/// 500). Project existence is per-path (only the reads whose Python
/// calls `Project.objects.get` 404 on a missing row).
struct Tenant {
    timezone: Tz,
}

async fn fetch_tenant(pool: &PgPool, user_id: &Uuid) -> Result<Tenant, Denial> {
    let timezone_name: Option<(String,)> =
        sqlx::query_as("SELECT user_timezone FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_optional(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    let timezone: Tz = timezone_name
        .ok_or(Denial::ServerError)?
        .0
        .parse()
        .map_err(|_| Denial::ServerError)?;
    Ok(Tenant { timezone })
}

/// The pool behind the request. `None` until the binary connects, so a
/// missing pool is a 500, never a panic.
fn pool_of(state: &AppState) -> Result<PgPool, Denial> {
    state
        .pools()
        .map(|pools| pools.primary().clone())
        .ok_or(Denial::ServerError)
}

// ---------------------------------------------------------------------------
// Datetime rendering
// ---------------------------------------------------------------------------

/// Render a timestamptz exactly like DRF `DateTimeField` with the default
/// `iso-8601` format: ISO-8601 in the request's zone with `+00:00`
/// rewritten to `Z` (serializer kernel rule — DRF converts aware values
/// to the current timezone, `fields.py:1168-1231`, which `TimezoneMixin`
/// set to the actor's).
fn render_dt(dt: &DateTime<Utc>, tz: &Tz) -> String {
    render_datetime_in(dt, tz)
}

fn render_dt_opt(dt: Option<DateTime<Utc>>, tz: &Tz) -> Option<String> {
    dt.map(|dt| render_dt(&dt, tz))
}

/// Render a timestamptz exactly like DRF's `JSONEncoder` (the `.values()`
/// list rows, which bypass `DateTimeField`): the stored instant as-is,
/// `+00:00` rewritten to `Z` (`encoders.py:28-32` — no timezone
/// conversion, unlike the field path above).
fn render_dt_utc(dt: &DateTime<Utc>) -> String {
    crate::serializer::render_datetime(dt)
}

/// A finite `sort_order` for the wire; a non-finite one answers 500
/// (DRF renders with `allow_nan=False` under the default
/// `STRICT_JSON=True`, so `ValueError` escapes into the generic 500).
fn render_sort_order(value: f64) -> Result<f64, Denial> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(Denial::ServerError)
    }
}

// ---------------------------------------------------------------------------
// Issue versions (`version.py:27-74`)
// ---------------------------------------------------------------------------

/// `GET .../issues/<issue_id>/versions/`: the gate, the count, the
/// `paginate` page over the model's `-created_at` order, the 10-key rows
/// with `created_at`/`updated_at` shifted to the actor's zone. No parent
/// checks — a missing issue is an empty 200 page.
async fn version_list(
    State(state): State<AppState>,
    Path((slug, project_raw, issue_raw)): Path<(String, String, String)>,
    Query(query): Query<QueryMap>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> HandlerResult {
    let Ok(issue_id) = path_uuid(&issue_raw) else {
        return Ok(crate::edge::proxy(State(state), req).await);
    };
    let pool = pool_of(&state)?;
    let user_id = actor_user_id(&pool, extension).await?;
    let project_id = resolve_project_id(&pool, &slug, &project_raw).await?;
    let membership = fetch_membership(&pool, &slug, &project_id, &user_id).await?;
    check_allow(&membership, &[20, 15, 5])?;
    let tenant = fetch_tenant(&pool, &user_id).await?;
    let total = version_count(&pool, &slug, &project_id, &issue_id).await?;
    let cursor = query_last(&query, "cursor");
    let page = v2_page(cursor.as_deref(), total).map_err(|_| Denial::ServerError)?;
    let mut binder = Binder::default();
    let sql = version_list_sql(&mut binder, &slug, project_id, issue_id, &page);
    let rows = sqlx::query(&sql)
        .bind(issue_id)
        .bind(project_id)
        .bind(&slug)
        .fetch_all(&pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let mut results = Vec::with_capacity(rows.len());
    for row in &rows {
        results.push(version_page_row(row, &tenant.timezone)?);
    }
    Ok(json_response(
        StatusCode::OK,
        version_envelope(&page, total, results),
    ))
}

/// `GET .../issues/<issue_id>/versions/<pk>/`: the gate, then the full
/// row through the 640 detail rendering. A miss (no row, or a row
/// outside the slug/project/issue scope) is the 404.
async fn version_detail(
    State(state): State<AppState>,
    Path((slug, project_raw, issue_raw, pk_raw)): Path<(String, String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> HandlerResult {
    let (Ok(issue_id), Ok(version_id)) = (path_uuid(&issue_raw), path_uuid(&pk_raw)) else {
        return Ok(crate::edge::proxy(State(state), req).await);
    };
    let pool = pool_of(&state)?;
    let user_id = actor_user_id(&pool, extension).await?;
    let project_id = resolve_project_id(&pool, &slug, &project_raw).await?;
    let membership = fetch_membership(&pool, &slug, &project_id, &user_id).await?;
    check_allow(&membership, &[20, 15, 5])?;
    let tenant = fetch_tenant(&pool, &user_id).await?;
    let mut binder = Binder::default();
    let sql = version_detail_sql(&mut binder, &slug, project_id, issue_id, version_id);
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(&sql)
        .bind(issue_id)
        .bind(version_id)
        .bind(project_id)
        .bind(&slug)
        .fetch_optional(&pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let Some(row) = row else {
        return Err(Denial::NotFound);
    };
    let body = render_version_detail(&row, &tenant.timezone)?;
    Ok(json_response(StatusCode::OK, body))
}

/// `paginate`'s `base_queryset.count()`.
async fn version_count(
    pool: &PgPool,
    slug: &str,
    project_id: &Uuid,
    issue_id: &Uuid,
) -> Result<i64, Denial> {
    let mut binder = Binder::default();
    let sql = version_count_sql(&mut binder, slug, *project_id, *issue_id);
    let row: Option<(i64,)> = sqlx::query_as(&sql)
        .bind(issue_id)
        .bind(project_id)
        .bind(slug)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    row.map(|row| row.0).ok_or(Denial::ServerError)
}

/// One `.values(*required_fields)` row in [`VERSION_PAGE_KEYS`] order.
/// Every value renders DRF-nullable (a NULL reads `null`, never 500s);
/// `created_at`/`updated_at` shift to the actor's zone (the explicit
/// `user_timezone_converter` pass), `last_saved_at` renders as stored
/// (the JSON-encoder path converts nothing).
fn version_page_row(row: &sqlx::postgres::PgRow, tz: &Tz) -> Result<Value, Denial> {
    let id: Option<Uuid> = row.try_get("id").map_err(|_| Denial::ServerError)?;
    let workspace_id: Option<Uuid> = row
        .try_get("workspace_id")
        .map_err(|_| Denial::ServerError)?;
    let project_id: Option<Uuid> = row.try_get("project_id").map_err(|_| Denial::ServerError)?;
    let issue_id: Option<Uuid> = row.try_get("issue_id").map_err(|_| Denial::ServerError)?;
    let last_saved_at: Option<DateTime<Utc>> = row
        .try_get("last_saved_at")
        .map_err(|_| Denial::ServerError)?;
    let owned_by_id: Option<Uuid> = row
        .try_get("owned_by_id")
        .map_err(|_| Denial::ServerError)?;
    let created_at: Option<DateTime<Utc>> =
        row.try_get("created_at").map_err(|_| Denial::ServerError)?;
    let updated_at: Option<DateTime<Utc>> =
        row.try_get("updated_at").map_err(|_| Denial::ServerError)?;
    let created_by_id: Option<Uuid> = row
        .try_get("created_by_id")
        .map_err(|_| Denial::ServerError)?;
    let updated_by_id: Option<Uuid> = row
        .try_get("updated_by_id")
        .map_err(|_| Denial::ServerError)?;
    let last_saved_at = last_saved_at
        .as_ref()
        .map(render_dt_utc)
        .map(Value::String)
        .unwrap_or(Value::Null);
    let created_at = render_dt_opt(created_at, tz)
        .map(Value::String)
        .unwrap_or(Value::Null);
    let updated_at = render_dt_opt(updated_at, tz)
        .map(Value::String)
        .unwrap_or(Value::Null);
    // Iterate the fixture's key list so the row order is structural,
    // not a second literal that could drift from `required_fields`.
    let mut map = Map::with_capacity(VERSION_PAGE_KEYS.len());
    for &key in VERSION_PAGE_KEYS.iter() {
        let value = match key {
            "id" => render_uuid_opt(&id),
            "workspace" => render_uuid_opt(&workspace_id),
            "project" => render_uuid_opt(&project_id),
            "issue" => render_uuid_opt(&issue_id),
            "last_saved_at" => last_saved_at.clone(),
            "owned_by" => render_uuid_opt(&owned_by_id),
            "created_at" => created_at.clone(),
            "updated_at" => updated_at.clone(),
            "created_by" => render_uuid_opt(&created_by_id),
            "updated_by" => render_uuid_opt(&updated_by_id),
            _ => unreachable!("VERSION_PAGE_KEYS"),
        };
        map.insert(key.to_owned(), value);
    }
    Ok(Value::Object(map))
}

/// The 9-key `paginate` envelope (`global_paginator.py:75-85`), in dict
/// order. `page_count` is this page's row count.
fn version_envelope(page: &V2Page, total_results: i64, results: Vec<Value>) -> String {
    let mut map = Map::with_capacity(9);
    map.insert(
        "prev_cursor".to_owned(),
        Value::String(page.prev_cursor.clone()),
    );
    map.insert("cursor".to_owned(), Value::String(page.cursor.clone()));
    map.insert(
        "next_cursor".to_owned(),
        page.next_cursor
            .clone()
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    map.insert(
        "prev_page_results".to_owned(),
        Value::Bool(page.prev_page_results),
    );
    map.insert(
        "next_page_results".to_owned(),
        Value::Bool(page.next_page_results),
    );
    map.insert(
        "page_count".to_owned(),
        Value::Number((results.len() as i64).into()),
    );
    map.insert(
        "total_results".to_owned(),
        Value::Number(total_results.into()),
    );
    map.insert(
        "total_pages".to_owned(),
        Value::Number(page.total_pages.into()),
    );
    map.insert("results".to_owned(), Value::Array(results));
    serde_json::to_string(&Value::Object(map)).expect("version envelope")
}

/// The full version row through the 640 detail rendering. All datetimes
/// render in the request's zone (DRF `DateTimeField`); dates render
/// bare; the UUID arrays render hyphenated strings.
fn render_version_detail(row: &sqlx::postgres::PgRow, tz: &Tz) -> Result<String, Denial> {
    let id: Uuid = row.try_get("id").map_err(|_| Denial::ServerError)?;
    let workspace_id: Uuid = row
        .try_get("workspace_id")
        .map_err(|_| Denial::ServerError)?;
    let project_id: Uuid = row.try_get("project_id").map_err(|_| Denial::ServerError)?;
    let issue_id: Uuid = row.try_get("issue_id").map_err(|_| Denial::ServerError)?;
    let id = uuid_string(&id);
    let workspace = uuid_string(&workspace_id);
    let project = uuid_string(&project_id);
    let issue = uuid_string(&issue_id);
    let parent: Option<Uuid> = row.try_get("parent").map_err(|_| Denial::ServerError)?;
    let state: Option<Uuid> = row.try_get("state").map_err(|_| Denial::ServerError)?;
    let estimate_point: Option<Uuid> = row
        .try_get("estimate_point")
        .map_err(|_| Denial::ServerError)?;
    let parent = parent.map(|id| uuid_string(&id));
    let state = state.map(|id| uuid_string(&id));
    let estimate_point = estimate_point.map(|id| uuid_string(&id));
    let name: String = row.try_get("name").map_err(|_| Denial::ServerError)?;
    let priority: String = row.try_get("priority").map_err(|_| Denial::ServerError)?;
    let start_date: Option<chrono::NaiveDate> =
        row.try_get("start_date").map_err(|_| Denial::ServerError)?;
    let target_date: Option<chrono::NaiveDate> = row
        .try_get("target_date")
        .map_err(|_| Denial::ServerError)?;
    let start_date = start_date.as_ref().map(render_date);
    let target_date = target_date.as_ref().map(render_date);
    let assignees: Vec<Uuid> = row.try_get("assignees").map_err(|_| Denial::ServerError)?;
    let labels: Vec<Uuid> = row.try_get("labels").map_err(|_| Denial::ServerError)?;
    let modules: Vec<Uuid> = row.try_get("modules").map_err(|_| Denial::ServerError)?;
    let assignees: Vec<String> = assignees.iter().map(uuid_string).collect();
    let labels: Vec<String> = labels.iter().map(uuid_string).collect();
    let modules: Vec<String> = modules.iter().map(uuid_string).collect();
    let assignees_ref: Vec<&str> = assignees.iter().map(String::as_str).collect();
    let labels_ref: Vec<&str> = labels.iter().map(String::as_str).collect();
    let modules_ref: Vec<&str> = modules.iter().map(String::as_str).collect();
    let sequence_id: i32 = row
        .try_get("sequence_id")
        .map_err(|_| Denial::ServerError)?;
    let sort_order: f64 = row.try_get("sort_order").map_err(|_| Denial::ServerError)?;
    let sort_order = render_sort_order(sort_order)?;
    let completed_at: Option<DateTime<Utc>> = row
        .try_get("completed_at")
        .map_err(|_| Denial::ServerError)?;
    let archived_at: Option<chrono::NaiveDate> = row
        .try_get("archived_at")
        .map_err(|_| Denial::ServerError)?;
    let completed_at = render_dt_opt(completed_at, tz);
    let archived_at = archived_at.as_ref().map(render_date);
    let is_draft: bool = row.try_get("is_draft").map_err(|_| Denial::ServerError)?;
    let external_source: Option<String> = row
        .try_get("external_source")
        .map_err(|_| Denial::ServerError)?;
    let external_id: Option<String> = row
        .try_get("external_id")
        .map_err(|_| Denial::ServerError)?;
    let r#type: Option<Uuid> = row.try_get("type").map_err(|_| Denial::ServerError)?;
    let cycle: Option<Uuid> = row.try_get("cycle").map_err(|_| Denial::ServerError)?;
    let r#type = r#type.map(|id| uuid_string(&id));
    let cycle = cycle.map(|id| uuid_string(&id));
    let meta: Value = row.try_get("meta").map_err(|_| Denial::ServerError)?;
    let last_saved_at: DateTime<Utc> = row
        .try_get("last_saved_at")
        .map_err(|_| Denial::ServerError)?;
    let last_saved_at = render_dt(&last_saved_at, tz);
    let owned_by_id: Uuid = row
        .try_get("owned_by_id")
        .map_err(|_| Denial::ServerError)?;
    let owned_by = uuid_string(&owned_by_id);
    let created_at: Option<DateTime<Utc>> =
        row.try_get("created_at").map_err(|_| Denial::ServerError)?;
    let updated_at: Option<DateTime<Utc>> =
        row.try_get("updated_at").map_err(|_| Denial::ServerError)?;
    let created_at = render_dt_opt(created_at, tz);
    let updated_at = render_dt_opt(updated_at, tz);
    let created_by_id: Option<Uuid> = row
        .try_get("created_by_id")
        .map_err(|_| Denial::ServerError)?;
    let updated_by_id: Option<Uuid> = row
        .try_get("updated_by_id")
        .map_err(|_| Denial::ServerError)?;
    let created_by = created_by_id.map(|id| uuid_string(&id));
    let updated_by = updated_by_id.map(|id| uuid_string(&id));
    let row = IssueVersionDetailRow {
        id: &id,
        workspace: &workspace,
        project: &project,
        issue: &issue,
        parent: parent.as_deref(),
        state: state.as_deref(),
        estimate_point: estimate_point.as_deref(),
        name: &name,
        priority: &priority,
        start_date: start_date.as_deref(),
        target_date: target_date.as_deref(),
        assignees: &assignees_ref,
        sequence_id,
        labels: &labels_ref,
        sort_order,
        completed_at: completed_at.as_deref(),
        archived_at: archived_at.as_deref(),
        is_draft,
        external_source: external_source.as_deref(),
        external_id: external_id.as_deref(),
        r#type: r#type.as_deref(),
        cycle: cycle.as_deref(),
        modules: &modules_ref,
        meta: &meta,
        last_saved_at: &last_saved_at,
        owned_by: &owned_by,
        created_at: created_at.as_deref(),
        updated_at: updated_at.as_deref(),
        created_by: created_by.as_deref(),
        updated_by: updated_by.as_deref(),
    };
    let view = issue_version_detail_to_representation(&row);
    serde_json::to_string(&view).map_err(|_| Denial::ServerError)
}

// ---------------------------------------------------------------------------
// Description versions (`version.py:77-144`)
// ---------------------------------------------------------------------------

/// `GET .../work-items/<work_item_id>/description-versions/`: the parent
/// `Project.get` + `Issue.get`, the guest-view gate, then the `paginate`
/// page over the explicit `.order_by("-created_at")` list.
async fn desc_version_list(
    State(state): State<AppState>,
    Path((slug, project_raw, work_item_raw)): Path<(String, String, String)>,
    Query(query): Query<QueryMap>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> HandlerResult {
    let Ok(work_item_id) = path_uuid(&work_item_raw) else {
        return Ok(crate::edge::proxy(State(state), req).await);
    };
    let pool = pool_of(&state)?;
    let user_id = actor_user_id(&pool, extension).await?;
    let project_id = resolve_project_id(&pool, &slug, &project_raw).await?;
    let membership = fetch_membership(&pool, &slug, &project_id, &user_id).await?;
    check_allow(&membership, &[20, 15, 5])?;
    let tenant = fetch_tenant(&pool, &user_id).await?;
    desc_version_parents(&pool, &slug, &project_id, &work_item_id, &user_id).await?;
    let total = desc_version_count(&pool, &slug, &project_id, &work_item_id).await?;
    let cursor = query_last(&query, "cursor");
    let page = v2_page(cursor.as_deref(), total).map_err(|_| Denial::ServerError)?;
    let mut binder = Binder::default();
    let sql = description_list_sql(&mut binder, &slug, project_id, work_item_id, &page);
    let rows = sqlx::query(&sql)
        .bind(work_item_id)
        .bind(project_id)
        .bind(&slug)
        .fetch_all(&pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let mut results = Vec::with_capacity(rows.len());
    for row in &rows {
        results.push(version_page_row(row, &tenant.timezone)?);
    }
    Ok(json_response(
        StatusCode::OK,
        version_envelope(&page, total, results),
    ))
}

/// `GET .../work-items/<work_item_id>/description-versions/<pk>/`: the
/// parents + gate, then the full row through the 640 rendering.
async fn desc_version_detail(
    State(state): State<AppState>,
    Path((slug, project_raw, work_item_raw, pk_raw)): Path<(String, String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> HandlerResult {
    let (Ok(work_item_id), Ok(version_id)) = (path_uuid(&work_item_raw), path_uuid(&pk_raw)) else {
        return Ok(crate::edge::proxy(State(state), req).await);
    };
    let pool = pool_of(&state)?;
    let user_id = actor_user_id(&pool, extension).await?;
    let project_id = resolve_project_id(&pool, &slug, &project_raw).await?;
    let membership = fetch_membership(&pool, &slug, &project_id, &user_id).await?;
    check_allow(&membership, &[20, 15, 5])?;
    let tenant = fetch_tenant(&pool, &user_id).await?;
    desc_version_parents(&pool, &slug, &project_id, &work_item_id, &user_id).await?;
    let mut binder = Binder::default();
    let sql = description_detail_sql(&mut binder, &slug, project_id, work_item_id, version_id);
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(&sql)
        .bind(work_item_id)
        .bind(version_id)
        .bind(project_id)
        .bind(&slug)
        .fetch_optional(&pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let Some(row) = row else {
        return Err(Denial::NotFound);
    };
    let body = render_desc_version_detail(&row, &tenant.timezone)?;
    Ok(json_response(StatusCode::OK, body))
}

/// The description-versions view preamble (`version.py:88-105`):
/// `Project.objects.get(pk)` (no slug scope, soft-delete scope only),
/// `Issue.objects.get(workspace, project, pk)` (soft-delete scope only
/// — triage/archived/draft rows stay visible here), then the guest-view
/// gate: an active GUEST membership + `not guest_view_all_features` +
/// a foreign `created_by` refuses 403.
async fn desc_version_parents(
    pool: &PgPool,
    slug: &str,
    project_id: &Uuid,
    work_item_id: &Uuid,
    user_id: &Uuid,
) -> Result<(), Denial> {
    let project: Option<(bool,)> = sqlx::query_as(
        "SELECT guest_view_all_features FROM projects WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(project_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let Some((guest_view_all,)) = project else {
        return Err(Denial::NotFound);
    };
    let issue: Option<(Option<Uuid>,)> = sqlx::query_as(
        r#"SELECT i.created_by_id FROM issues i
           JOIN workspaces w ON w.id = i.workspace_id
           WHERE i.deleted_at IS NULL AND w.slug = $1 AND i.project_id = $2 AND i.id = $3"#,
    )
    .bind(slug)
    .bind(project_id)
    .bind(work_item_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let Some((issue_created_by,)) = issue else {
        return Err(Denial::NotFound);
    };
    let is_guest: Option<(i32,)> = sqlx::query_as(
        r#"SELECT 1 FROM project_members pm
           JOIN workspaces w ON w.id = pm.workspace_id
           WHERE pm.member_id = $1 AND pm.project_id = $2 AND w.slug = $3
           AND pm.role = 5 AND pm.is_active AND pm.deleted_at IS NULL"#,
    )
    .bind(user_id)
    .bind(project_id)
    .bind(slug)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    if is_guest.is_some() && !guest_view_all && issue_created_by != Some(*user_id) {
        return Err(Denial::GuestView);
    }
    Ok(())
}

/// `paginate`'s `base_queryset.count()` for description versions.
async fn desc_version_count(
    pool: &PgPool,
    slug: &str,
    project_id: &Uuid,
    work_item_id: &Uuid,
) -> Result<i64, Denial> {
    let mut binder = Binder::default();
    let sql = description_count_sql(&mut binder, slug, *project_id, *work_item_id);
    let row: Option<(i64,)> = sqlx::query_as(&sql)
        .bind(work_item_id)
        .bind(project_id)
        .bind(slug)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    row.map(|row| row.0).ok_or(Denial::ServerError)
}

/// The full description-version row through the 640 rendering
/// (`description_binary` re-encodes base64 inside the port).
fn render_desc_version_detail(row: &sqlx::postgres::PgRow, tz: &Tz) -> Result<String, Denial> {
    let id: Uuid = row.try_get("id").map_err(|_| Denial::ServerError)?;
    let workspace_id: Uuid = row
        .try_get("workspace_id")
        .map_err(|_| Denial::ServerError)?;
    let project_id: Uuid = row.try_get("project_id").map_err(|_| Denial::ServerError)?;
    let issue_id: Uuid = row.try_get("issue_id").map_err(|_| Denial::ServerError)?;
    let id = uuid_string(&id);
    let workspace = uuid_string(&workspace_id);
    let project = uuid_string(&project_id);
    let issue = uuid_string(&issue_id);
    let description_binary: Option<Vec<u8>> = row
        .try_get("description_binary")
        .map_err(|_| Denial::ServerError)?;
    let description_html: String = row
        .try_get("description_html")
        .map_err(|_| Denial::ServerError)?;
    let description_stripped: Option<String> = row
        .try_get("description_stripped")
        .map_err(|_| Denial::ServerError)?;
    let description_json: Value = row
        .try_get("description_json")
        .map_err(|_| Denial::ServerError)?;
    let last_saved_at: DateTime<Utc> = row
        .try_get("last_saved_at")
        .map_err(|_| Denial::ServerError)?;
    let last_saved_at = render_dt(&last_saved_at, tz);
    let owned_by_id: Uuid = row
        .try_get("owned_by_id")
        .map_err(|_| Denial::ServerError)?;
    let owned_by = uuid_string(&owned_by_id);
    let created_at: Option<DateTime<Utc>> =
        row.try_get("created_at").map_err(|_| Denial::ServerError)?;
    let updated_at: Option<DateTime<Utc>> =
        row.try_get("updated_at").map_err(|_| Denial::ServerError)?;
    let created_at = render_dt_opt(created_at, tz);
    let updated_at = render_dt_opt(updated_at, tz);
    let created_by_id: Option<Uuid> = row
        .try_get("created_by_id")
        .map_err(|_| Denial::ServerError)?;
    let updated_by_id: Option<Uuid> = row
        .try_get("updated_by_id")
        .map_err(|_| Denial::ServerError)?;
    let created_by = created_by_id.map(|id| uuid_string(&id));
    let updated_by = updated_by_id.map(|id| uuid_string(&id));
    let row = IssueDescriptionVersionDetailRow {
        id: &id,
        workspace: &workspace,
        project: &project,
        issue: &issue,
        description_binary: description_binary.as_deref(),
        description_html: &description_html,
        description_stripped: description_stripped.as_deref(),
        description_json: &description_json,
        last_saved_at: &last_saved_at,
        owned_by: &owned_by,
        created_at: created_at.as_deref(),
        updated_at: updated_at.as_deref(),
        created_by: created_by.as_deref(),
        updated_by: updated_by.as_deref(),
    };
    let view = issue_description_version_detail_to_representation(&row);
    serde_json::to_string(&view).map_err(|_| Denial::ServerError)
}

// ---------------------------------------------------------------------------
// Move (`move.py:17-46`)
// ---------------------------------------------------------------------------

/// Move body shape for the shared negotiator: a single scalar,
/// last-value-wins; uploads are dropped (the view reads `request.data`
/// only, and no contract body is multipart).
const MOVE_BODY_SPEC: shared_body::BodySpec = shared_body::BodySpec {
    list_fields: &[],
    skip_blank_fields: &[],
};

/// `POST .../work-items/<pk>/move/`: the ADMIN/MEMBER gate, the body
/// read, the pre-save snapshot, `domain::issues::move_to_project`
/// inside a write transaction (the 650 kernel over the SeaORM move
/// store, with the creation-outbox drain, the post-commit
/// cancel/drain, the 594 fire, the completion comment and the
/// enqueues folded into the commit chain), then the refreshed issue
/// in the 22-key APP read shape.
async fn move_issue(
    State(state): State<AppState>,
    Path((slug, project_raw, pk_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> HandlerResult {
    let Ok(pk) = path_uuid(&pk_raw) else {
        return Ok(crate::edge::proxy(State(state), req).await);
    };
    let pool = pool_of(&state)?;
    let user_id = actor_user_id(&pool, extension).await?;
    let project_id = resolve_project_id(&pool, &slug, &project_raw).await?;
    let membership = fetch_membership(&pool, &slug, &project_id, &user_id).await?;
    check_allow(&membership, &[20, 15])?;
    let tenant = fetch_tenant(&pool, &user_id).await?;
    let target_ref = read_move_target(req).await?;
    let origin = move_origin(&state)?;
    let web_base_url = move_web_base_url(&state);
    let now = Utc::now();
    // The pre-save snapshot, before the move's own write.
    let prev = crate::issue_transition::capture_prior_state_pool(&pool, Some(pk))
        .await
        .map_err(|_| Denial::ServerError)?;
    let orm = state.orm().ok_or(Denial::ServerError)?;
    let ctx = WriteCtx::new(WriteActor {
        user_id,
        timezone: Some(tenant.timezone.name().to_owned()),
    })
    .with_scope(WriteScope {
        workspace_id: None,
        project_id: Some(project_id),
    });
    let transition: Arc<dyn IssueTransitionHook> = Arc::new(AppTransitionHook::new(&state, &pool));
    let enqueue: Arc<dyn TaskEnqueueHook> = Arc::new(AppTaskEnqueueHook::new(&pool));
    let span_deps = SpanCreationDeps::from_state(&state);
    let drain: Arc<dyn MovePostCommitHook> = Arc::new(AppMovePostCommitHook::new(
        &pool,
        &state,
        span_deps.cache.clone(),
    ));
    let creation = CreationDeps {
        pool: pool.clone(),
        cloud: span_deps.cloud.clone(),
        managed: span_deps.managed.clone(),
        now_unix_secs: span_deps.now_unix_secs,
        toolsets: span_deps.toolsets.clone(),
        assistant_seam: span_deps.assistant_seam.clone(),
        admission: Arc::new(AppMoveAdmissionCache::new(span_deps.cache)),
    };
    let outcome = domain::run(&ctx, orm, |txn| {
        let transition = transition.clone();
        let enqueue = enqueue.clone();
        let drain = drain.clone();
        let ctx = ctx.clone();
        Box::pin(async move {
            txn.hooks_mut().register(transition);
            txn.hooks_mut().register(enqueue);
            txn.hooks_mut().register(drain);
            domain_issues::move_to_project(
                &ctx,
                txn,
                domain_issues::MoveInput {
                    slug: &slug,
                    project_id,
                    pk,
                    target_ref: &target_ref,
                    origin: origin.clone(),
                    web_base_url: web_base_url.as_deref(),
                    now,
                    prev_state: prev,
                    options: domain_issues::MoveOptions::django(),
                    creation,
                },
            )
            .await
        })
    })
    .await
    .map_err(map_move_domain_error)?;
    match outcome {
        MoveDomainOutcome::AlreadyThere { row, .. } => {
            // Same project: no writes at all, no signals, no enqueues —
            // just the issue in the read shape.
            let body = render_moved_issue(&pool, &row, &tenant.timezone).await?;
            Ok(json_response(StatusCode::OK, body))
        }
        MoveDomainOutcome::Moved { row, chain } => {
            if chain.stopped().is_some() {
                return Err(Denial::ServerError);
            }
            let body = render_moved_issue(&pool, &row.row, &tenant.timezone).await?;
            Ok(json_response(StatusCode::OK, body))
        }
        MoveDomainOutcome::Rejected(rejection) => Err(map_move_rejection(rejection)),
    }
}

/// The move's `target_ref` (`move.py:33-39`): `request.data` coerced to
/// `{}` when it is not a dict, then `.get("project")` (missing reads
/// JSON null, which the driver blanks to the required-400).
async fn read_move_target(req: axum::extract::Request) -> Result<Value, Denial> {
    let (parts, body) = req.into_parts();
    let bytes = read_request_bytes(body)
        .await
        .map_err(|_| Denial::TruncatedBody)?;
    match shared_body::negotiate_body(&parts.headers, &bytes, &MOVE_BODY_SPEC) {
        Ok(shared_body::NegotiatedBody::Empty) => Ok(Value::Null),
        Ok(shared_body::NegotiatedBody::JsonText { text, .. }) => {
            match parse_json_text(&text) {
                Ok(parsed) => match parsed.into_object() {
                    Some(map) => Ok(map
                        .get("project")
                        .map(to_serde_publish)
                        .unwrap_or(Value::Null)),
                    None => Ok(Value::Null),
                },
                Err(JsonFail::Message(detail)) => {
                    Err(Denial::BadDetail(format!("{JSON_PARSE_PREFIX}{detail}")))
                }
                // Deep nesting: Django's `RecursionError` escapes DRF's
                // `ValueError` catch into the generic 500.
                Err(JsonFail::Recursion) => Err(Denial::ServerError),
            }
        }
        Ok(shared_body::NegotiatedBody::Form { map, .. }) => {
            Ok(map.get("project").cloned().unwrap_or(Value::Null))
        }
        Err(shared_body::BodyError::UnsupportedMediaType(message)) => {
            Err(Denial::UnsupportedMediaType(message))
        }
        Err(shared_body::BodyError::ParseDetail(message)) => Err(Denial::BadDetail(message)),
        Err(shared_body::BodyError::ServerError) => Err(Denial::ServerError),
    }
}

/// `base_host(request, is_app=True)`: `APP_BASE_URL` when set, else the
/// `WEB_URL or APP_BASE_URL` origin — unset everywhere is a 500
/// (`ImproperlyConfigured`). Empty strings fall through like Python's
/// `or`, as in the v1 twin.
fn move_origin(state: &AppState) -> Result<String, Denial> {
    let urls = &state.settings().urls;
    urls.app_base_url
        .clone()
        .filter(|s| !s.is_empty())
        .or_else(|| urls.web_url.clone().filter(|s| !s.is_empty()))
        .ok_or(Denial::ServerError)
}

/// `web_base_url()`: `WEB_URL`, falling back to `APP_BASE_URL`, trailing
/// slash stripped — `None` when neither is configured, so the
/// `current_instance` render omits the url. Empty strings fall through
/// like Python's `or`, as in the v1 twin.
fn move_web_base_url(state: &AppState) -> Option<String> {
    let urls = &state.settings().urls;
    urls.web_url
        .clone()
        .filter(|s| !s.is_empty())
        .or_else(|| urls.app_base_url.clone().filter(|s| !s.is_empty()))
        .map(|base| base.trim_end_matches('/').to_owned())
}

/// Map the 650 driver's failures onto the view's responses: the
/// recoverable `{"error"}` + status, the source-miss 404, the
/// resolve-miss 404 detail, anything else the 500.
fn map_move_rejection(rejection: MoveRejection) -> Denial {
    match rejection {
        MoveRejection::Issue {
            message,
            status_code,
        } => Denial::MoveRejection {
            status: status_code,
            message,
        },
        MoveRejection::SourceNotFound => Denial::NotFound,
        MoveRejection::TargetNotFound => Denial::ProjectNotFound,
        // Unreachable: this driver passes `TargetOnly` (the view checked
        // the source project up front).
        MoveRejection::SourceDenied => Denial::Forbidden,
    }
}

/// Map a domain-run failure: a store error is the generic 500; a work
/// error is the kernel's post-save 409 (the multiple-executing-runs
/// arm — the span rolled back, as Django's `atomic` exit on raise).
fn map_move_domain_error(error: domain::DomainError) -> Denial {
    match error {
        domain::DomainError::Db(_) => Denial::ServerError,
        domain::DomainError::Work(message) => Denial::MoveRejection {
            status: 409,
            message,
        },
    }
}

// ---------------------------------------------------------------------------
// Move response
// ---------------------------------------------------------------------------

/// The move's 200 body: `IssueSerializer(issue).data` over the
/// refreshed (or same-project) issue — a bare instance, so the 7
/// queryset-annotation keys omit via `SkipField` and the 639 base shape
/// renders 22 keys. Datetimes render in the request's zone (DRF
/// `DateTimeField`); `archived_at` renders a bare date; `is_synced`
/// answers the merged predicate against the live sync tables.
async fn render_moved_issue(
    pool: &PgPool,
    issue: &SourceIssueRow,
    tz: &Tz,
) -> Result<String, Denial> {
    let sort_order = render_sort_order(issue.sort_order)?;
    let completed_at = render_dt_opt(issue.completed_at, tz);
    let created_at = render_dt(&issue.created_at, tz);
    let updated_at = render_dt(&issue.updated_at, tz);
    let archived_at = issue
        .archived_at
        .as_ref()
        .map(|at| render_date(&at.date_naive()));
    let is_synced = moved_is_synced(pool, issue).await?;
    let id = uuid_string(&issue.id);
    let state_id = issue.state_id.map(|id| uuid_string(&id));
    let parent_id = issue.parent_id.map(|id| uuid_string(&id));
    let assigned_pod_id = issue.assigned_pod_id.map(|id| uuid_string(&id));
    let project_id = uuid_string(&issue.project_id);
    let estimate_point = issue.estimate_point_id.map(|id| uuid_string(&id));
    let created_by = issue.created_by_id.map(|id| uuid_string(&id));
    let updated_by = issue.updated_by_id.map(|id| uuid_string(&id));
    let sequence_id = i32::try_from(issue.sequence_id).map_err(|_| Denial::ServerError)?;
    let complexity_score =
        i32::try_from(issue.complexity_score).map_err(|_| Denial::ServerError)?;
    let start_date = issue.start_date.as_ref().map(render_date);
    let target_date = issue.target_date.as_ref().map(render_date);
    let row = IssueDetailBaseRow {
        id: &id,
        name: &issue.name,
        state_id: state_id.as_deref(),
        sort_order,
        completed_at: completed_at.as_deref(),
        estimate_point: estimate_point.as_deref(),
        priority: &issue.priority,
        complexity_score,
        start_date: start_date.as_deref(),
        target_date: target_date.as_deref(),
        sequence_id,
        project_id: &project_id,
        parent_id: parent_id.as_deref(),
        cycle_id: None,
        assigned_pod_id: assigned_pod_id.as_deref(),
        agent_executor: issue.agent_executor.as_deref(),
        module_ids: None,
        label_ids: None,
        assignee_ids: None,
        sub_issues_count: None,
        created_at: &created_at,
        updated_at: &updated_at,
        created_by: created_by.as_deref(),
        updated_by: updated_by.as_deref(),
        attachment_count: None,
        link_count: None,
        is_draft: issue.is_draft,
        archived_at: archived_at.as_deref(),
        is_synced,
    };
    let view = issue_detail_base_to_representation(&row);
    serde_json::to_string(&view).map_err(|_| Denial::ServerError)
}

/// The move's `is_synced` (`serializers/issue.py:297-323`): the bare
/// instance carries no annotation, so the merged predicate runs — no
/// probe when `external_source` is blank, else git then github.
async fn moved_is_synced(pool: &PgPool, issue: &SourceIssueRow) -> Result<bool, Denial> {
    let blank = issue
        .external_source
        .as_deref()
        .is_none_or(|source| source.trim().is_empty());
    if blank {
        return Ok(issue_is_actively_synced(
            issue.external_source.as_deref(),
            None,
            || false,
            || false,
        ));
    }
    let git_hit: Option<(i32,)> = sqlx::query_as(GIT_ISSUE_SYNC_PROBE_SQL)
        .bind(issue.id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    if git_hit.is_some() {
        return Ok(issue_is_actively_synced(
            issue.external_source.as_deref(),
            None,
            || true,
            || false,
        ));
    }
    let github_hit: Option<(i32,)> = sqlx::query_as(GITHUB_ISSUE_SYNC_PROBE_SQL)
        .bind(issue.id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(issue_is_actively_synced(
        issue.external_source.as_deref(),
        None,
        || false,
        || github_hit.is_some(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_match_django_routes() {
        assert_eq!(
            VERSIONS_PATH,
            "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/versions/"
        );
        assert_eq!(
            VERSION_PATH,
            "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/versions/{pk}/"
        );
        assert_eq!(
            DESC_VERSIONS_PATH,
            "/api/workspaces/{slug}/projects/{project_id}/work-items/{work_item_id}/description-versions/"
        );
        assert_eq!(
            DESC_VERSION_PATH,
            "/api/workspaces/{slug}/projects/{project_id}/work-items/{work_item_id}/description-versions/{pk}/"
        );
        assert_eq!(
            MOVE_PATH,
            "/api/workspaces/{slug}/projects/{project_id}/work-items/{pk}/move/"
        );
    }

    #[test]
    fn envelope_key_order_and_empty_page() {
        let page = v2_page(None, 0).expect("empty page");
        let body = version_envelope(&page, 0, vec![]);
        let value: Value = serde_json::from_str(&body).expect("json");
        let keys: Vec<&str> = value
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            vec![
                "prev_cursor",
                "cursor",
                "next_cursor",
                "prev_page_results",
                "next_page_results",
                "page_count",
                "total_results",
                "total_pages",
                "results",
            ]
        );
        assert_eq!(
            body,
            // `prev_cursor` is `"1000:-1:0"` even on the empty first page
            // (Python formats `current_page - 1` unconditionally).
            r#"{"prev_cursor":"1000:-1:0","cursor":"1000:0:0","next_cursor":null,"prev_page_results":false,"next_page_results":false,"page_count":0,"total_results":0,"total_pages":0,"results":[]}"#
        );
    }

    #[test]
    fn envelope_mid_page_cursors() {
        let page = v2_page(Some("1000:2:0"), 2500).expect("page");
        let body = version_envelope(&page, 2500, vec![Value::Bool(true)]);
        assert_eq!(
            body,
            r#"{"prev_cursor":"1000:1:0","cursor":"1000:2:0","next_cursor":null,"prev_page_results":true,"next_page_results":false,"page_count":1,"total_results":2500,"total_pages":3,"results":[true]}"#
        );
    }

    #[test]
    fn envelope_first_page_next_cursor() {
        let page = v2_page(None, 1500).expect("page");
        let body = version_envelope(&page, 1500, vec![]);
        let value: Value = serde_json::from_str(&body).expect("json");
        assert_eq!(value["next_cursor"], Value::String("1000:1:0".to_owned()));
        assert_eq!(value["next_page_results"], Value::Bool(true));
        assert_eq!(value["total_pages"], Value::Number(2.into()));
    }

    #[test]
    fn bad_cursors_answer_server_error() {
        assert!(v2_page(Some("nope"), 10).is_err());
        assert!(v2_page(Some("0:0:0"), 10).is_err());
        assert!(v2_page(Some("1000:0"), 10).is_err());
    }

    #[test]
    fn denial_bodies() {
        let (status, body) = Denial::Unauthorized.status_and_body();
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body, super::super::UNAUTHENTICATED_BODY);
        let (status, body) = Denial::Forbidden.status_and_body();
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body, crate::permissions::PERMISSION_DENIED_BODY);
        let (status, body) = Denial::GuestView.status_and_body();
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(
            body,
            r#"{"error":"You are not allowed to view this issue"}"#
        );
        let (status, body) = Denial::NotFound.status_and_body();
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body, super::super::NOT_FOUND_BODY);
        let (status, body) = Denial::ProjectNotFound.status_and_body();
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body, super::super::PROJECT_NOT_FOUND_BODY);
        let (status, body) = Denial::BadDetail("JSON parse error - x".to_owned()).status_and_body();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body, r#"{"detail":"JSON parse error - x"}"#);
        let (status, body) = Denial::MoveRejection {
            status: 409,
            message: "Target project has no default pod".to_owned(),
        }
        .status_and_body();
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body, r#"{"error":"Target project has no default pod"}"#);
        let (status, body) = Denial::TruncatedBody.status_and_body();
        assert_eq!(status, StatusCode::BAD_GATEWAY);
        assert_eq!(body, crate::edge::BAD_GATEWAY_BODY);
        let (status, body) = Denial::UnsupportedMediaType("m".to_owned()).status_and_body();
        assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
        assert_eq!(body, r#"{"detail":"m"}"#);
        let (status, body) = Denial::ServerError.status_and_body();
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(body, super::super::SERVER_ERROR_BODY);
    }

    #[test]
    fn move_rejection_statuses() {
        for (status, code) in [
            (400, StatusCode::BAD_REQUEST),
            (403, StatusCode::FORBIDDEN),
            (409, StatusCode::CONFLICT),
        ] {
            let (got, _) = Denial::MoveRejection {
                status,
                message: "x".to_owned(),
            }
            .status_and_body();
            assert_eq!(got, code);
        }
    }

    #[test]
    fn json_text_escapes_line_separators() {
        assert_eq!(
            escape_json_text("a b c"),
            "a\u{2028}b\u{2029}c"
                .replace('\u{2028}', "\\u2028")
                .replace('\u{2029}', "\\u2029")
        );
        assert_eq!(escape_json_text("a\u{2028}b\u{2029}c"), "a\\u2028b\\u2029c");
        assert_eq!(escape_json_text(r#"{"a":1}"#), r#"{"a":1}"#);
    }

    #[test]
    fn sort_order_finite_only() {
        assert_eq!(render_sort_order(65535.0).expect("finite"), 65535.0);
        assert!(render_sort_order(f64::INFINITY).is_err());
        assert!(render_sort_order(f64::NAN).is_err());
    }

    #[test]
    fn path_uuid_strictness() {
        let id = "12345678-1234-1234-1234-1234567890ab";
        assert_eq!(path_uuid(id).expect("valid").to_string(), id);
        assert!(path_uuid("12345678-1234-1234-1234-1234567890AB").is_err());
        assert!(path_uuid("123456781234123412341234567890ab").is_err());
        assert!(path_uuid("{12345678-1234-1234-1234-1234567890ab}").is_err());
        assert!(path_uuid("urn:uuid:12345678-1234-1234-1234-1234567890ab").is_err());
        assert!(path_uuid("").is_err());
        assert!(path_uuid("not-a-uuid-at-all----------------").is_err());
    }

    #[test]
    fn allow_matrix() {
        let member = |project_role, workspace_role| Membership {
            project_role,
            workspace_role,
        };
        assert!(check_allow(&member(Some(20), None), &[20, 15]).is_ok());
        assert!(check_allow(&member(Some(15), None), &[20, 15]).is_ok());
        assert!(check_allow(&member(Some(5), None), &[20, 15, 5]).is_ok());
        assert!(check_allow(&member(Some(5), None), &[20, 15]).is_err());
        assert!(check_allow(&member(Some(5), Some(20)), &[20, 15]).is_ok());
        assert!(check_allow(&member(Some(10), Some(20)), &[20, 15, 5]).is_ok());
        assert!(check_allow(&member(Some(10), Some(15)), &[20, 15, 5]).is_err());
        assert!(check_allow(&member(None, Some(20)), &[20, 15, 5]).is_err());
        assert!(check_allow(&member(None, None), &[20, 15, 5]).is_err());
    }

    #[test]
    fn move_error_mapping() {
        let denial = map_move_rejection(MoveRejection::SourceNotFound);
        assert!(matches!(denial, Denial::NotFound));
        let denial = map_move_rejection(MoveRejection::TargetNotFound);
        assert!(matches!(denial, Denial::ProjectNotFound));
        let denial = map_move_rejection(MoveRejection::SourceDenied);
        assert!(matches!(denial, Denial::Forbidden));
        let denial = map_move_rejection(MoveRejection::Issue {
            message: "m".to_owned(),
            status_code: 403,
        });
        match denial {
            Denial::MoveRejection { status, message } => {
                assert_eq!(status, 403);
                assert_eq!(message, "m");
            }
            other => panic!("unexpected {other:?}"),
        }
        let denial = map_move_domain_error(domain::DomainError::Work("m".to_owned()));
        match denial {
            Denial::MoveRejection { status, message } => {
                assert_eq!(status, 409);
                assert_eq!(message, "m");
            }
            other => panic!("unexpected {other:?}"),
        }
        let denial = map_move_domain_error(domain::DomainError::Db(sea_orm::DbErr::Custom(
            "x".to_owned(),
        )));
        assert!(matches!(denial, Denial::ServerError));
    }

    #[test]
    fn page_keys_cover_both_lists() {
        assert_eq!(
            VERSION_PAGE_KEYS,
            [
                "id",
                "workspace",
                "project",
                "issue",
                "last_saved_at",
                "owned_by",
                "created_at",
                "updated_at",
                "created_by",
                "updated_by",
            ]
        );
    }

    #[test]
    fn datetime_renders() {
        let utc: Tz = "UTC".parse().expect("utc");
        let dt = chrono::DateTime::parse_from_rfc3339("2026-10-04T03:04:05.123456789Z")
            .expect("dt")
            .with_timezone(&Utc);
        // Sub-micro nanos truncate to micros like DRF (PIDASHCONV-867).
        assert_eq!(render_dt(&dt, &utc), "2026-10-04T03:04:05.123456Z");
        assert_eq!(render_dt_utc(&dt), "2026-10-04T03:04:05.123456Z");
        let kolkata: Tz = "Asia/Kolkata".parse().expect("tz");
        assert_eq!(render_dt(&dt, &kolkata), "2026-10-04T08:34:05.123456+05:30");
        assert_eq!(render_dt_utc(&dt), "2026-10-04T03:04:05.123456Z");
        assert_eq!(render_dt_opt(None, &utc), None);
        let date = chrono::NaiveDate::from_ymd_opt(2026, 10, 4).expect("date");
        assert_eq!(render_date(&date), "2026-10-04");
    }

    fn origin_state(app_base_url: Option<&str>, web_url: Option<&str>) -> AppState {
        let mut settings = pidash_db::config::Settings::test_defaults();
        settings.urls.app_base_url = app_base_url.map(str::to_owned);
        settings.urls.web_url = web_url.map(str::to_owned);
        AppState::with_settings("test", settings)
    }

    /// `base_host(request, is_app=True)` treats `""` as missing
    /// (PIDASHCONV-1577): an empty `APP_BASE_URL` falls through to
    /// `WEB_URL`, and empty everywhere is the 500.
    #[test]
    fn move_origin_falls_through_empty_strings() {
        let web = "https://web.example.invalid";
        let state = origin_state(Some(""), Some(web));
        assert_eq!(move_origin(&state).expect("falls through"), web);
        let state = origin_state(Some(""), Some(""));
        assert!(matches!(move_origin(&state), Err(Denial::ServerError)));
        let state = origin_state(None, None);
        assert!(matches!(move_origin(&state), Err(Denial::ServerError)));
        let state = origin_state(Some("https://app.example.invalid"), Some(web));
        assert_eq!(
            move_origin(&state).expect("app base wins"),
            "https://app.example.invalid"
        );
    }

    /// `web_base_url()` treats `""` as missing (PIDASHCONV-1577): empty
    /// everywhere is `None` (callers omit the url), and an empty
    /// `WEB_URL` falls back to `APP_BASE_URL`.
    #[test]
    fn move_web_base_url_falls_through_empty_strings() {
        let app = "https://app.example.invalid";
        let state = origin_state(Some(""), Some(""));
        assert_eq!(move_web_base_url(&state), None);
        let state = origin_state(None, None);
        assert_eq!(move_web_base_url(&state), None);
        let state = origin_state(Some(app), Some(""));
        assert_eq!(move_web_base_url(&state).as_deref(), Some(app));
        let state = origin_state(Some(app), Some("https://web.example.invalid/"));
        assert_eq!(
            move_web_base_url(&state).as_deref(),
            Some("https://web.example.invalid")
        );
    }
}
