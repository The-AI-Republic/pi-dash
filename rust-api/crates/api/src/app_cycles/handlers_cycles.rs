//! CycleViewSet CRUD handlers (PIDASHCONV-321).
//!
//! Ports the five `CycleViewSet` units in
//! `apps/api/pi_dash/app/views/cycle/base.py:64-519` (drift baseline
//! `01a93e17`) with identical URL paths, status codes and JSON bytes:
//! `list` (`:183-268`), `create` (`:270-333`), `partial_update`
//! (`:335-408`), `retrieve` (`:410-475`), `destroy` (`:477-517`). Routes
//! are 1-2 of `apps/api/pi_dash/app/urls/cycle.py:21-39`.
//!
//! The queryset behind every read is the annotated `get_queryset`
//! (`base.py:69-182`, F-C27-03) built with the
//! `pidash_services::app_cycles::queries` fragments; the write paths go
//! through the `CycleWriteSerializer` rules (F-C27-01,
//! `pidash_services::app_cycles::shape`) and the `@allow_permission`
//! gates (F-C27-07, [`crate::app_cycles::gates`]). Non-owned methods
//! (`PUT`, `OPTIONS`, and everything else on these paths) proxy to
//! Django per the F-02 cutover edge, so the undecorated PUT-update
//! fallthrough and DRF metadata survive byte for byte.
//!
//! Ported bugs and rendering rules (translate, don't redesign — all
//! verified live against Django):
//! 1. `cycle_view=current` with zero matches answers `[]`, not ALL: the
//!    fallthrough re-reads the already-filtered queryset (`:205` +
//!    `:239`). The F-C27-03 fixture note claiming ALL is a code-reading
//!    error; live behavior rules.
//! 2. Every `.values()` row renders model fields first (in `.values()`
//!    order: `...logo_props, version, created_by`) and then the
//!    projected annotations in queryset-definition order — so the plain
//!    list, the current-view branch and all write re-reads share one
//!    shape per key set, and the `.values()` order differences between
//!    branches are unobservable. Retrieve's `sub_issues` renders last.
//! 3. The completed-cycle `sort_order`-only narrowing is dead:
//!    `request_data` is reassigned but the serializer gets the raw
//!    `request.data` (`:352` vs `:359`), so a completed cycle still
//!    takes a full update when `sort_order` is present.
//! 4. `PATCH` on a missing pk answers the generic 500 (`None.archived_at`
//!    `AttributeError`, `:338-339`), while `retrieve`/`destroy` answer
//!    their own 404s (`:458-459`, `ObjectDoesNotExist` branch).
//! 5. `Cycle.save` overrides an explicit `sort_order` whenever the
//!    project already has a cycle (`min - 10000`, `db/models/cycle.py`).
//! 6. A lone date skips both the ordering check and the `convert_to_utc`
//!    rewrite and is stored raw (`serializers/cycle.py:16-23`); the
//!    create-time XOR rule lives in the view (`:272-274`), so the
//!    serializer alone accepts a lone date.
//! 7. `created_by`/`updated_by` come from `BaseModel.save` via crum
//!    (`db/models/base.py:23-42`): create stamps `created_by`, update
//!    stamps `updated_by` — never from the request body.
//!
//! Deferred publishes (best-effort `rust_job_queue` rows, response
//! stands): `model_activity` on create/partial_update
//! (`bgtasks/webhook_task.py:463`), `recent_visited_task` on retrieve,
//! `issue_activity` + `soft_delete_related_objects` on destroy. The
//! `current_instance` before-image on partial_update is the fetched row
//! map rendered as a JSON string (an async payload, not an API body).

use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Router;
use chrono_tz::Tz;
use serde_json::{Map, Value};
use sqlx::Row;

use crate::app_cycles::gates::{decide_gate, gate_for, GateOutcome};
use crate::middleware::SessionHandle;
use crate::state::AppState;

use pidash_auth::permissions::allow::AllowFacts;

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------

/// Register the two `CycleViewSet` paths. Owned methods serve from Rust;
/// every other method (notably the undecorated `PUT` fallthrough and
/// `OPTIONS` metadata) proxies to Django, whose 401-anon-before-405,
/// routing and sibling-action behavior lives there.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/workspaces/{slug}/projects/{project_id}/cycles/",
            owned(
                axum::routing::get(cycle_list).post(cycle_create),
                &["GET", "POST"],
            ),
        )
        .route(
            "/api/workspaces/{slug}/projects/{project_id}/cycles/{pk}/",
            owned(
                axum::routing::get(cycle_retrieve)
                    .patch(cycle_partial_update)
                    .delete(cycle_destroy),
                &["GET", "PATCH", "DELETE"],
            ),
        )
}

/// An owned path: listed methods serve from Rust, everything else proxies
/// to Django. OPTIONS proxies too: DRF answers metadata (401 anon / 200
/// authed) where axum would 405. HEAD rides axum's `get` handling like
/// Django's `GET`-backed `HEAD`.
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

/// Django's `<uuid:pk>` converter rejects non-UUID segments at routing
/// time (HTML 404 in DEBUG, never reaching the view). Axum path captures
/// match any segment, so every detail handler proxies non-UUID tails to
/// Django first, reproducing its routing 404 byte-for-byte.
///
/// Bodies parse through one `Json` shape on every write path (verified
/// live): an empty body is `{}` (DRF never raises `ParseError` on it —
/// an empty POST answers the name-required 400, an empty PATCH is a
/// no-op 200); a non-empty body without a JSON content type answers the
/// `UnsupportedMediaType` 415 naming the content type (`text/plain` when
/// missing); otherwise axum's `Json` rejection bytes apply (malformed
/// JSON stays DRF-unmatched — its messages carry parser positions serde
/// cannot reproduce — documented, never hit by the suite).
#[allow(clippy::result_large_err)]
async fn detail_body(state: &AppState, req: axum::extract::Request) -> Result<Value, Response> {
    use axum::extract::FromRequest;
    let (parts, body) = req.into_parts();
    // Same default limit as axum's `Json` (2 MiB).
    let bytes = axum::body::to_bytes(body, 2 * 1024 * 1024)
        .await
        .map_err(|_| Denial::ServerError.into_response())?;
    if bytes.is_empty() {
        return Ok(Value::Object(Map::new()));
    }
    let content_type = parts
        .headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    let mime = content_type.split(';').next().unwrap_or("").trim();
    if !mime.eq_ignore_ascii_case("application/json") {
        let named = if content_type.is_empty() {
            "text/plain"
        } else {
            content_type
        };
        let body = format!(
            "{{\"detail\":\"Unsupported media type {} in request.\"}}",
            json_string(named)
        );
        return Err(Denial::Raw(StatusCode::UNSUPPORTED_MEDIA_TYPE, body).into_response());
    }
    let req = axum::extract::Request::from_parts(parts, axum::body::Body::from(bytes));
    match axum::Json::<Value>::from_request(req, state).await {
        Ok(axum::Json(body)) => Ok(body),
        Err(rejection) => Err(rejection.into_response()),
    }
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Exact bytes of the DRF `IsAuthenticated` denial.
pub const UNAUTHENTICATED_BODY: &str =
    r#"{"detail":"Authentication credentials were not provided."}"#;
/// `handle_exception`'s `ObjectDoesNotExist` branch (`app/views/base.py`).
pub const NOT_FOUND_BODY: &str = r#"{"error":"The required object does not exist."}"#;
/// `handle_exception`'s `ValidationError` branch.
pub const INVALID_DETAIL_BODY: &str = r#"{"error":"Please provide valid detail"}"#;
/// `handle_exception`'s `IntegrityError` branch.
pub const INVALID_PAYLOAD_BODY: &str = r#"{"error":"The payload is not valid"}"#;
/// `handle_exception`'s generic 500 branch.
pub const SERVER_ERROR_BODY: &str = r#"{"error":"Something went wrong please try again later"}"#;
/// `CycleViewSet.retrieve` miss (`base.py:458-459`).
pub const CYCLE_NOT_FOUND_BODY: &str = r#"{"error":"Cycle not found"}"#;
/// `Project.resolve` miss on a non-UUID identifier
/// (`db/models/project.py:213-217`): the `Http404("Project not found")`
/// message survives DRF's `Http404` conversion verbatim.
pub const PROJECT_NOT_FOUND_BODY: &str = r#"{"detail":"Project not found"}"#;
/// The create-time XOR date rule (`base.py:330-333`).
pub const DATE_XOR_BODY: &str =
    r#"{"error":"Both start date and end date are either required or are to be null"}"#;
/// Archived-cycle guard on `partial_update` (`base.py:340-343`).
pub const ARCHIVED_UPDATE_BODY: &str = r#"{"error":"Archived cycle cannot be updated"}"#;
/// Completed-cycle guard on `partial_update` (`base.py:354-357`).
pub const COMPLETED_UPDATE_BODY: &str =
    r#"{"error":"The Cycle has already been completed so it cannot be edited"}"#;
/// DRF `DateTimeField` wrong-format message
/// (`rest_framework/fields.py`, `DATETIME_INPUT_FORMATS = ['iso-8601']`).
pub const DATETIME_FORMAT_HINT: &str = "YYYY-MM-DDThh:mm[:ss[.uuuuuu]][+HH:MM|-HH:MM|Z]";
/// The `CycleWriteSerializer.validate` ordering error
/// (`app/serializers/cycle.py:22`).
pub const START_AFTER_END_BODY: &str =
    r#"{"non_field_errors":["Start date cannot exceed end date"]}"#;

/// Handler failure with its exact status + body.
#[derive(Debug)]
pub enum Denial {
    /// 401, DRF `NotAuthenticated`.
    Unauthorized,
    /// 403, `@allow_permission` body.
    Forbidden,
    /// 404, `ObjectDoesNotExist` branch.
    NotFound,
    /// 404, `{"detail": ...}` (project-identifier miss).
    NotFoundDetail,
    /// 404, `{"error": "Cycle not found"}` (retrieve miss).
    CycleNotFound,
    /// 400, `{"error": ...}` (view-inline).
    BadError(String),
    /// 400, `ValidationError` branch.
    BadDetail,
    /// 400, `IntegrityError` branch.
    BadPayload,
    /// 500, generic branch.
    ServerError,
    /// A pre-rendered exact body with its status (serializer `errors`
    /// dicts, whose key order DRF fixes).
    Raw(StatusCode, String),
}

impl Denial {
    fn status_and_body(&self) -> (StatusCode, String) {
        match self {
            Denial::Unauthorized => (StatusCode::UNAUTHORIZED, UNAUTHENTICATED_BODY.to_owned()),
            Denial::Forbidden => (
                StatusCode::FORBIDDEN,
                crate::app_cycles::gates::FORBIDDEN_BODY.to_owned(),
            ),
            Denial::NotFound => (StatusCode::NOT_FOUND, NOT_FOUND_BODY.to_owned()),
            Denial::NotFoundDetail => (StatusCode::NOT_FOUND, PROJECT_NOT_FOUND_BODY.to_owned()),
            Denial::CycleNotFound => (StatusCode::NOT_FOUND, CYCLE_NOT_FOUND_BODY.to_owned()),
            Denial::BadError(message) => (
                StatusCode::BAD_REQUEST,
                format!("{{\"error\":{}}}", json_string(message)),
            ),
            Denial::BadDetail => (StatusCode::BAD_REQUEST, INVALID_DETAIL_BODY.to_owned()),
            Denial::BadPayload => (StatusCode::BAD_REQUEST, INVALID_PAYLOAD_BODY.to_owned()),
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
        .expect("cycle response")
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).expect("json string")
}

/// Map a gate outcome to its denial. `Allow` is unreachable here; the
/// caller returns early on it.
fn gate_denial(outcome: GateOutcome, gate_method: &str, gate_path: &str) -> Denial {
    let _ = (gate_method, gate_path);
    match outcome {
        GateOutcome::Allow => Denial::ServerError,
        GateOutcome::Deny => Denial::Forbidden,
        GateOutcome::Unauthenticated => Denial::Unauthorized,
    }
}

// ---------------------------------------------------------------------------
// Request context: auth + tenant + membership
// ---------------------------------------------------------------------------

/// Session auth (`BaseSessionAuthentication` + `IsAuthenticated` on
/// `BaseViewSet`): anonymous answers the DRF `NotAuthenticated` body
/// before anything else runs — including before the project-identifier
/// rewrite, so the slug-existence oracle stays closed.
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

/// `_rewrite_project_kwarg` (`app/views/base.py`): UUIDs pass through
/// (the row check happens in the view body); other identifiers match
/// `UPPER(identifier)` in the workspace; misses raise
/// `Http404("Project not found")`. Skipped for anonymous callers, who
/// never reach here (see [`actor`]).
async fn resolve_project_id(
    pool: &sqlx::PgPool,
    slug: &str,
    raw: &str,
) -> Result<uuid::Uuid, Denial> {
    if let Ok(id) = raw.parse::<uuid::Uuid>() {
        return Ok(id);
    }
    // `Project.save()` normalizes `identifier` to upper, so an equality
    // match on the upper-cased input uses the plain btree
    // (`db/models/project.py:200-212`); no `__iexact`, same as Python.
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
    row.map(|row| row.0).ok_or(Denial::NotFoundDetail)
}

/// One project row: `Project.objects.get(id=...)` raises `DoesNotExist`
/// on a miss or a soft-deleted row (the `ObjectDoesNotExist` 404). No
/// workspace filter — the view looks the project up by id alone; tenant
/// scoping happens in the queryset.
struct ProjectRow {
    workspace_id: uuid::Uuid,
    archived_at: Option<chrono::DateTime<chrono::Utc>>,
}

async fn project_row(pool: &sqlx::PgPool, project_id: &uuid::Uuid) -> Result<ProjectRow, Denial> {
    let row: Option<(uuid::Uuid, Option<chrono::DateTime<chrono::Utc>>)> = sqlx::query_as(
        r#"SELECT workspace_id, archived_at FROM projects
           WHERE id = $1 AND deleted_at IS NULL"#,
    )
    .bind(project_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let Some((workspace_id, archived_at)) = row else {
        return Err(Denial::NotFound);
    };
    Ok(ProjectRow {
        workspace_id,
        archived_at,
    })
}

/// Membership facts for the `@allow_permission` gates, with exactly the
/// row filters Python uses (`app/permissions/base.py:19-86`): active,
/// non-deleted rows scoped to the workspace slug (and project id for
/// the project row). Forward-FK joins carry no related-manager filter,
/// so no `workspaces.deleted_at` guard — same SQL semantics as Django.
async fn membership(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    user_id: &uuid::Uuid,
) -> Result<(Option<i16>, Option<i16>), Denial> {
    let workspace_role: Option<(Option<i16>,)> = sqlx::query_as(
        r#"SELECT wm.role FROM workspace_members wm
           JOIN workspaces w ON w.id = wm.workspace_id
           WHERE w.slug = $1 AND wm.member_id = $2 AND wm.is_active AND wm.deleted_at IS NULL"#,
    )
    .bind(slug)
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let workspace_role = workspace_role.and_then(|row| row.0);
    let row: Option<(Option<i16>,)> = sqlx::query_as(
        r#"SELECT pm.role FROM project_members pm
           JOIN workspaces w ON w.id = pm.workspace_id
           WHERE w.slug = $1 AND pm.project_id = $2 AND pm.member_id = $3
             AND pm.is_active AND pm.deleted_at IS NULL"#,
    )
    .bind(slug)
    .bind(project_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok((workspace_role, row.and_then(|row| row.0)))
}

/// Build the [`AllowFacts`] for one gate from the membership rows.
/// `allowed` are the integer role values on the gate row.
fn allow_facts(
    slug: &str,
    allowed: &[i32],
    workspace_role: Option<i16>,
    project_role: Option<i16>,
    is_creator: bool,
) -> AllowFacts {
    let workspace_role = workspace_role.map(i32::from);
    let project_role = project_role.map(i32::from);
    AllowFacts {
        workspace: pidash_types::WorkspaceId::from(slug),
        authenticated: true,
        is_workspace_member: workspace_role.is_some(),
        has_allowed_workspace_role: workspace_role.is_some_and(|role| allowed.contains(&role)),
        is_creator,
        has_allowed_project_role: project_role.is_some_and(|role| allowed.contains(&role)),
        is_project_member: project_role.is_some(),
        is_workspace_admin: workspace_role == Some(pidash_auth::permissions::ROLE_ADMIN),
    }
}

/// Check one `GATES` row: fetch membership, decide, map the outcome.
/// `is_creator` is only consulted by the destroy gate; other callers
/// pass `false`.
async fn check_gate(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    user_id: &uuid::Uuid,
    method: &str,
    path: &str,
    is_creator: bool,
) -> Result<(), Denial> {
    let route = gate_for(method, path).ok_or(Denial::ServerError)?;
    let allowed: &[i32] = match &route.gate {
        crate::app_cycles::gates::Gate::Authenticated => &[],
        crate::app_cycles::gates::Gate::Project { roles } => roles,
        crate::app_cycles::gates::Gate::ProjectCreator { roles } => roles,
    };
    let (workspace_role, project_role) = membership(pool, slug, project_id, user_id).await?;
    let scope = crate::app_cycles::gates::tenant_context(slug);
    let facts = allow_facts(slug, allowed, workspace_role, project_role, is_creator);
    match decide_gate(&route.gate, &scope, &facts) {
        GateOutcome::Allow => Ok(()),
        outcome => Err(gate_denial(outcome, method, path)),
    }
}

/// The destroy creator fact: `Cycle.objects.filter(id=pk,
/// created_by=user).exists()` — default manager, so live rows only.
async fn is_cycle_creator(
    pool: &sqlx::PgPool,
    pk: &uuid::Uuid,
    user_id: &uuid::Uuid,
) -> Result<bool, Denial> {
    let row: Option<(uuid::Uuid,)> = sqlx::query_as(
        r#"SELECT id FROM cycles WHERE id = $1 AND created_by_id = $2 AND deleted_at IS NULL"#,
    )
    .bind(pk)
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(row.is_some())
}

/// A malformed pk never reaches here (detail routes proxy non-UUID tails
/// at routing time); the fallback 400 mirrors the ORM `ValidationError`
/// body for defense in depth.
fn parse_pk(raw: &str) -> Option<uuid::Uuid> {
    raw.parse::<uuid::Uuid>().ok()
}

/// `base_host(request, is_app=True)` (`utils/host.py:17`): `WEB_URL`
/// else `APP_BASE_URL`; unset is `ImproperlyConfigured` → 500.
fn request_origin(state: &AppState) -> Result<String, Denial> {
    state
        .settings()
        .urls
        .web_url
        .clone()
        .or_else(|| state.settings().urls.app_base_url.clone())
        .ok_or(Denial::ServerError)
}

/// Best-effort deferred publish (the space intake precedent): without
/// the queue the response still stands.
async fn enqueue_message(pool: &sqlx::PgPool, message: pidash_jobs::celery::CeleryTaskMessage) {
    let job = pidash_jobs::queue::NewJob::new(
        message.task.clone(),
        serde_json::Value::Array(message.args.clone()),
        serde_json::Value::Object(message.kwargs.clone()),
    );
    if let Err(error) = pidash_jobs::queue::enqueue(pool, &job).await {
        tracing::warn!(%error, task = message.task.as_str(), "task enqueue failed; response stands");
    }
}
// ---------------------------------------------------------------------------
// Annotated reads: the `get_queryset` SQL + projections
// ---------------------------------------------------------------------------

/// Shared annotation core: `is_favorite` Exists, the group counts, the
/// `status` Case and `assignee_ids` (`base.py:113-178`). `$1` slug, `$2`
/// project id, `$3` user id, `$4` now. The caller appends its own
/// predicates; the `GROUP BY` stays here so every read groups the same
/// way. Joins are all LEFT (Django's nullable-FK fan-out): cycles with
/// no issues still return one row with zero counts and `[]` assignees.
fn annotated_from(current_only: bool, extra_pk: bool, include_archived: bool) -> String {
    let current = if current_only {
        " AND c.start_date <= $4 AND c.end_date >= $4"
    } else {
        ""
    };
    let pk = if extra_pk { " AND c.id = $5" } else { "" };
    // `partial_update` reads its row through the unfiltered `get_queryset`
    // (`base.py:337-338` carries no archived guard — the guard is the
    // explicit 400 below), so its pre-read keeps archived rows; every
    // other read excludes them.
    let archived = if include_archived {
        ""
    } else {
        " AND c.archived_at IS NULL"
    };
    format!(
        r#"FROM cycles c
           JOIN workspaces w ON w.id = c.workspace_id AND w.slug = $1
           JOIN projects p ON p.id = c.project_id
           LEFT JOIN cycle_issues ci ON ci.cycle_id = c.id
           LEFT JOIN issues i ON i.id = ci.issue_id
           LEFT JOIN states s ON s.id = i.state_id
           LEFT JOIN issue_assignees ia ON ia.issue_id = i.id
           WHERE c.project_id = $2 AND c.deleted_at IS NULL
             AND p.archived_at IS NULL
             AND EXISTS (SELECT 1 FROM project_members pm
                         WHERE pm.project_id = c.project_id AND pm.member_id = $3
                           AND pm.is_active AND pm.deleted_at IS NULL){archived}{current}{pk}
           GROUP BY c.id"#
    )
}

/// Full select list over [`annotated_from`]: model columns plus the
/// annotations, named exactly like the `.values()` keys they feed.
/// `sub_issues` rides along only for retrieve (a correlated count, not a
/// group key, so it stays out of `GROUP BY`).
/// Select list shared by every read: model columns plus the annotations,
/// named exactly like the `.values()` keys they feed. `archived_at` rides
/// along for the partial_update guard; shaping ignores keys outside the
/// `.values()` projection.
const SELECT_BASE: &str = r#"c.id, c.workspace_id, c.project_id, c.name, c.description,
       c.start_date, c.end_date, c.owned_by_id, c.view_props, c.sort_order,
       c.external_source, c.external_id, c.progress_snapshot, c.logo_props,
       c.version, c.created_by_id AS created_by, c.archived_at,
       EXISTS (SELECT 1 FROM user_favorites uf
               WHERE uf.user_id = $3 AND uf.entity_identifier = c.id
                 AND uf.entity_type = 'cycle' AND uf.project_id = $2
                 AND uf.workspace_id = c.workspace_id AND uf.deleted_at IS NULL) AS is_favorite,
       COUNT(DISTINCT i.id) FILTER (WHERE i.archived_at IS NULL AND i.is_draft = FALSE AND ci.deleted_at IS NULL AND i.deleted_at IS NULL) AS total_issues,
       COUNT(DISTINCT i.id) FILTER (WHERE i.archived_at IS NULL AND i.is_draft = FALSE AND ci.deleted_at IS NULL AND i.deleted_at IS NULL AND s."group" = 'completed') AS completed_issues,
       COUNT(DISTINCT i.id) FILTER (WHERE i.archived_at IS NULL AND i.is_draft = FALSE AND ci.deleted_at IS NULL AND i.deleted_at IS NULL AND s."group" IN ('cancelled')) AS cancelled_issues,
       COALESCE(ARRAY_AGG(DISTINCT ia.assignee_id) FILTER (WHERE ia.assignee_id IS NOT NULL AND ia.deleted_at IS NULL), '{}') AS assignee_ids,
       CASE WHEN c.start_date <= $4 AND c.end_date >= $4 THEN 'CURRENT' WHEN c.start_date > $4 THEN 'UPCOMING' WHEN c.end_date < $4 THEN 'COMPLETED' WHEN c.start_date IS NULL AND c.end_date IS NULL THEN 'DRAFT' ELSE 'DRAFT' END AS status"#;

/// Full select list over [`annotated_from`]. `sub_issues` rides along
/// only for retrieve: a correlated count over the grouped row, which is
/// why it lives in the select list and not in `GROUP BY`. It goes through
/// `Issue.issue_objects` (`base.py:418`), so the `IssueManager`
/// exclusions apply (`db/models/issue.py:95-104`): triage states,
/// archived issues and drafts never count. The `project__archived_at`
/// exclusion is vacuous here — the outer row already proves the project
/// live — so only the three row-level guards are ported. The `states`
/// join is inner, like Django's single-valued-FK exclude traversal, so
/// null-state rows drop out on both sides.
fn annotated_select(sub_issues: bool) -> String {
    if !sub_issues {
        return SELECT_BASE.to_owned();
    }
    format!(
        "{SELECT_BASE}, (SELECT COUNT(*) FROM issues si JOIN cycle_issues sci ON sci.issue_id = si.id JOIN states s2 ON s2.id = si.state_id WHERE sci.cycle_id = c.id AND sci.deleted_at IS NULL AND si.parent_id IS NOT NULL AND si.project_id = $2 AND si.deleted_at IS NULL AND si.archived_at IS NULL AND si.is_draft = FALSE AND s2.\"group\" != 'triage') AS sub_issues"
    )
}

/// Fetch annotated rows: the list/re-read SQL with its tail order
/// (`-is_favorite`, `-created_at`, `base.py:189`).
async fn fetch_cycle_rows(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    user_id: &uuid::Uuid,
    now: &chrono::DateTime<chrono::Utc>,
    current_only: bool,
) -> Result<Vec<Map<String, Value>>, Denial> {
    let sql = format!(
        "SELECT row_to_json(__r)::text AS __row FROM (SELECT {} {} ORDER BY is_favorite DESC, c.created_at DESC) AS __r",
        annotated_select(false),
        annotated_from(current_only, false, false),
    );
    let rows = sqlx::query(&sql)
        .bind(slug)
        .bind(project_id)
        .bind(user_id)
        .bind(now)
        .fetch_all(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    rows_to_maps(&rows)
}

/// Fetch one annotated row by pk (create/partial_update re-read,
/// retrieve): same SQL plus the pk constraint, no tail order.
/// `include_archived` is only for the `partial_update` pre-read, whose
/// queryset has no archived guard (`base.py:337-339`: a missing row is
/// the 500, an archived row is the explicit 400).
#[allow(clippy::too_many_arguments)]
async fn fetch_cycle_by_pk(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    user_id: &uuid::Uuid,
    now: &chrono::DateTime<chrono::Utc>,
    pk: &uuid::Uuid,
    sub_issues: bool,
    include_archived: bool,
) -> Result<Option<Map<String, Value>>, Denial> {
    let sql = format!(
        "SELECT row_to_json(__r)::text AS __row FROM (SELECT {} {} ) AS __r",
        annotated_select(sub_issues),
        annotated_from(false, true, include_archived),
    );
    let row: Option<(String,)> = sqlx::query_as(&sql)
        .bind(slug)
        .bind(project_id)
        .bind(user_id)
        .bind(now)
        .bind(pk)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    match row {
        None => Ok(None),
        Some((text,)) => Ok(Some(json_text_to_map(&text)?)),
    }
}

fn rows_to_maps(rows: &[sqlx::postgres::PgRow]) -> Result<Vec<Map<String, Value>>, Denial> {
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let text: String = row.try_get("__row").map_err(|_| Denial::ServerError)?;
        out.push(json_text_to_map(&text)?);
    }
    Ok(out)
}

fn json_text_to_map(text: &str) -> Result<Map<String, Value>, Denial> {
    let value: Value = serde_json::from_str(text).map_err(|_| Denial::ServerError)?;
    match value {
        Value::Object(map) => Ok(map),
        _ => Err(Denial::ServerError),
    }
}

// ---------------------------------------------------------------------------
// Shaping: row maps to DRF key order + bytes
// ---------------------------------------------------------------------------

/// Render one `row_to_json` timestamptz in the actor zone: DRF renders
/// `DateTimeField` in the activated actor zone (`TimezoneMixin`), and the
/// `user_timezone_converter` project-tz detour is a pure zone round-trip
/// (`x.astimezone(A).astimezone(B) == x.astimezone(B)`), so the wire form
/// is the stored instant in the actor zone. `null` stays `null`.
fn shape_datetime(value: Option<&Value>, timezone: &Tz) -> String {
    let text = match value {
        Some(Value::String(text)) => text,
        _ => return "null".to_owned(),
    };
    match chrono::DateTime::parse_from_rfc3339(text) {
        Ok(aware) => {
            let rendered = crate::serializer::render_datetime_in(&aware, timezone);
            serde_json::to_string(&rendered).unwrap_or("null".to_owned())
        }
        Err(_) => serde_json::to_string(value.unwrap_or(&Value::Null)).unwrap_or("null".to_owned()),
    }
}

fn raw_str(row: &Map<String, Value>, key: &str) -> String {
    match row.get(key) {
        Some(Value::String(text)) => json_string(text),
        Some(Value::Null) | None => "null".to_owned(),
        Some(other) => serde_json::to_string(other).unwrap_or("null".to_owned()),
    }
}

fn raw_uuid(row: &Map<String, Value>, key: &str) -> String {
    raw_str(row, key)
}

fn raw_json(row: &Map<String, Value>, key: &str) -> String {
    match row.get(key) {
        Some(value) => serde_json::to_string(value).unwrap_or("null".to_owned()),
        None => "null".to_owned(),
    }
}

fn raw_int(row: &Map<String, Value>, key: &str) -> String {
    match row.get(key).and_then(Value::as_i64) {
        Some(n) => n.to_string(),
        None => "null".to_owned(),
    }
}

fn raw_bool(row: &Map<String, Value>, key: &str) -> String {
    match row.get(key).and_then(Value::as_bool) {
        Some(b) => b.to_string(),
        None => "null".to_owned(),
    }
}

/// Shared `id` → `progress_snapshot` prefix (every projection opens with
/// these keys in this order).
fn shape_head(row: &Map<String, Value>, timezone: &Tz) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    let _ = write!(out, "\"id\":{}", raw_uuid(row, "id"));
    let _ = write!(out, ",\"workspace_id\":{}", raw_uuid(row, "workspace_id"));
    let _ = write!(out, ",\"project_id\":{}", raw_uuid(row, "project_id"));
    let _ = write!(out, ",\"name\":{}", raw_str(row, "name"));
    let _ = write!(out, ",\"description\":{}", raw_str(row, "description"));
    let _ = write!(
        out,
        ",\"start_date\":{}",
        shape_datetime(row.get("start_date"), timezone)
    );
    let _ = write!(
        out,
        ",\"end_date\":{}",
        shape_datetime(row.get("end_date"), timezone)
    );
    let _ = write!(out, ",\"owned_by_id\":{}", raw_uuid(row, "owned_by_id"));
    let _ = write!(out, ",\"view_props\":{}", raw_json(row, "view_props"));
    let sort_order = row.get("sort_order").and_then(Value::as_f64).unwrap_or(0.0);
    let _ = write!(
        out,
        ",\"sort_order\":{}",
        crate::paginator::py_float_str(sort_order)
    );
    let _ = write!(
        out,
        ",\"external_source\":{}",
        raw_str(row, "external_source")
    );
    let _ = write!(out, ",\"external_id\":{}", raw_str(row, "external_id"));
    let _ = write!(
        out,
        ",\"progress_snapshot\":{}",
        raw_json(row, "progress_snapshot")
    );
    out
}

/// Which annotated tail a projection carries. Verified live: every
/// `.values()` row renders its model fields first (in `.values()` order:
/// `...logo_props, version, created_by`) and then the projected
/// annotations in queryset-definition order (`is_favorite`,
/// `total_issues`, `completed_issues`, `cancelled_issues`, `status`,
/// `assignee_ids`, with retrieve's `sub_issues` last) — regardless of the
/// annotation positions inside each `.values()` call. In particular the
/// plain-list (`:257-260`) and current-view (`:224-227`) projections are
/// byte-identical in shape; their `.values()` order difference is
/// unobservable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TailOrder {
    /// List + current-view: the six base annotations.
    List,
    /// Create/partial_update re-read: no `cancelled_issues`.
    Write,
    /// Retrieve: write annotations plus trailing `sub_issues`.
    Retrieve,
}

/// Shared tail: `logo_props, version, created_by`, then the projected
/// annotations in queryset-definition order.
fn shape_tail(row: &Map<String, Value>, order: TailOrder) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    let _ = write!(out, "\"logo_props\":{}", raw_json(row, "logo_props"));
    let _ = write!(out, ",\"version\":{}", raw_int(row, "version"));
    let _ = write!(out, ",\"created_by\":{}", raw_uuid(row, "created_by"));
    let _ = write!(out, ",\"is_favorite\":{}", raw_bool(row, "is_favorite"));
    let _ = write!(out, ",\"total_issues\":{}", raw_int(row, "total_issues"));
    let _ = write!(
        out,
        ",\"completed_issues\":{}",
        raw_int(row, "completed_issues")
    );
    if order == TailOrder::List {
        let _ = write!(
            out,
            ",\"cancelled_issues\":{}",
            raw_int(row, "cancelled_issues")
        );
    }
    let _ = write!(out, ",\"status\":{}", raw_str(row, "status"));
    let _ = write!(out, ",\"assignee_ids\":{}", raw_json(row, "assignee_ids"));
    if order == TailOrder::Retrieve {
        let _ = write!(out, ",\"sub_issues\":{}", raw_int(row, "sub_issues"));
    }
    out
}

/// Shape one row for the plain list (`base.py:239-265`, 22 keys).
fn shape_list_row(row: &Map<String, Value>, timezone: &Tz) -> String {
    let mut out = String::from("{");
    out.push_str(&shape_head(row, timezone));
    out.push(',');
    out.push_str(&shape_tail(row, TailOrder::List));
    out.push('}');
    out
}

/// Shape one row for the current-view branch (`base.py:207-232`): same
/// bytes as the plain list (see [`TailOrder`]).
fn shape_current_row(row: &Map<String, Value>, timezone: &Tz) -> String {
    shape_list_row(row, timezone)
}

/// Shape one row for the create/partial_update re-read
/// (`base.py:281-306`, `:362-387`: 21 keys, no `cancelled_issues`).
fn shape_write_row(row: &Map<String, Value>, timezone: &Tz) -> String {
    let mut out = String::from("{");
    out.push_str(&shape_head(row, timezone));
    out.push(',');
    out.push_str(&shape_tail(row, TailOrder::Write));
    out.push('}');
    out
}

/// Shape one row for retrieve (`base.py:428-455`: write keys plus
/// `sub_issues` after `progress_snapshot`).
fn shape_retrieve_row(row: &Map<String, Value>, timezone: &Tz) -> String {
    let mut out = String::from("{");
    out.push_str(&shape_head(row, timezone));
    out.push(',');
    out.push_str(&shape_tail(row, TailOrder::Retrieve));
    out.push('}');
    out
}
// ---------------------------------------------------------------------------
// CycleWriteSerializer port (`app/serializers/cycle.py:15-44`)
// ---------------------------------------------------------------------------

/// One validated datetime: the parsed instant plus the calendar date
/// Python's `validate()` reads (`serializers/cycle.py:29-37`:
/// `str(data["start_date"].date())`). DRF only re-zones naive inputs
/// (`make_aware` in the actor zone); aware inputs keep their own offset,
/// so their date is read in the input offset, not the actor zone. Naive
/// inputs (date-only strings, naive datetimes) keep their wall date,
/// which attaching the actor zone never changes.
#[derive(Debug, Clone, Copy)]
struct ParsedDate {
    instant: chrono::DateTime<chrono::Utc>,
    date: chrono::NaiveDate,
}

/// Parse a serializer date input the way DRF does with
/// `DATETIME_INPUT_FORMATS = ['iso-8601']` (Django 4.2
/// `parse_datetime`, `datetime.fromisoformat` first): full ISO datetimes
/// and date-only strings parse; anything else fails `invalid` with the
/// `DATETIME_FORMAT_HINT` message. Naive results are made aware in the
/// actor zone (`enforce_timezone` under `USE_TZ`); aware results are
/// converted to it. Non-string JSON (numbers, bools, arrays, objects)
/// fails the same way — JSON has no date objects, and DRF's `strptime`
/// suppresses the `TypeError` into `invalid`.
fn parse_serializer_date(value: &Value, timezone: &Tz) -> Result<ParsedDate, &'static str> {
    let text = match value {
        Value::String(text) => text.as_str(),
        _ => return Err("invalid"),
    };
    if let Ok(aware) = chrono::DateTime::parse_from_rfc3339(text) {
        return Ok(ParsedDate {
            instant: aware.with_timezone(&chrono::Utc),
            date: aware.date_naive(),
        });
    }
    // `datetime.fromisoformat` extras Django accepts: a space separator
    // and a bare `Z`. Normalize both into strict RFC 3339.
    let mut normalized = text.replace(' ', "T");
    if normalized.ends_with('Z') {
        normalized.pop();
        normalized.push_str("+00:00");
    }
    if let Ok(aware) = chrono::DateTime::parse_from_rfc3339(&normalized) {
        return Ok(ParsedDate {
            instant: aware.with_timezone(&chrono::Utc),
            date: aware.date_naive(),
        });
    }
    // Date-only (`datetime.fromisoformat` on 3.11+): naive midnight,
    // made aware in the actor zone.
    if let Ok(date) = chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d") {
        let naive = date.and_hms_opt(0, 0, 0).expect("midnight exists");
        return Ok(ParsedDate {
            instant: naive_to_actor(naive, timezone),
            date,
        });
    }
    // Naive ISO datetime (`fromisoformat` without offset): made aware in
    // the actor zone; the wall date survives the attach.
    if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(&normalized, "%Y-%m-%dT%H:%M:%S%.f") {
        return Ok(ParsedDate {
            instant: naive_to_actor(naive, timezone),
            date: naive.date(),
        });
    }
    if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(&normalized, "%Y-%m-%dT%H:%M:%S") {
        return Ok(ParsedDate {
            instant: naive_to_actor(naive, timezone),
            date: naive.date(),
        });
    }
    Err("invalid")
}

/// `enforce_timezone` for a naive wall time: attach the actor zone
/// (`timezone.make_aware`, DST gaps resolve like `zoneinfo`, which never
/// raises — the views precedent's `render_naive_datetime_in` fallback).
fn naive_to_actor(naive: chrono::NaiveDateTime, timezone: &Tz) -> chrono::DateTime<chrono::Utc> {
    use chrono::{LocalResult, TimeZone as _};
    match timezone.from_local_datetime(&naive) {
        LocalResult::Single(local) | LocalResult::Ambiguous(local, _) => {
            local.with_timezone(&chrono::Utc)
        }
        LocalResult::None => naive.and_utc(),
    }
}

/// Validated write fields. `None` on a date/JSON/scalar means explicit
/// JSON null (or absent on create); `present` tracks whether the key was
/// sent at all, which is what partial updates switch on.
#[derive(Debug, Clone, Default)]
struct CycleWrite {
    present: Vec<&'static str>,
    name: Option<String>,
    description: Option<Value>,
    start: Option<chrono::DateTime<chrono::Utc>>,
    end: Option<chrono::DateTime<chrono::Utc>>,
    start_date: Option<chrono::NaiveDate>,
    end_date: Option<chrono::NaiveDate>,
    view_props: Option<Value>,
    sort_order: Option<f64>,
    external_source: Option<Value>,
    external_id: Option<Value>,
    progress_snapshot: Option<Value>,
    logo_props: Option<Value>,
    timezone_name: Option<String>,
    version: Option<i64>,
    created_by: Option<uuid::Uuid>,
    updated_by: Option<uuid::Uuid>,
    deleted_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl CycleWrite {
    fn has(&self, key: &str) -> bool {
        self.present.contains(&key)
    }
}

/// Validate one request object through the `CycleWriteSerializer` field
/// rules. `partial` is DRF `partial=True` (PATCH: nothing required).
/// Errors render as the `{"field": ["msg"]}` 400; key order follows the
/// model field order. Unknown keys and read-only keys (`id`,
/// `workspace`, `project`, `owned_by`, `archived_at`, plus
/// `created_at`/`updated_at` which are `editable=False`) are ignored.
fn validate_write(
    object: &Map<String, Value>,
    partial: bool,
    timezone: &Tz,
) -> Result<CycleWrite, (StatusCode, String)> {
    let mut errors: Vec<(String, String)> = Vec::new();
    let mut out = CycleWrite::default();
    let err = |field: &'static str, message: String| (field.to_owned(), message);

    // `name`: CharField(max_length=255), required unless partial.
    match object.get("name") {
        None if partial => {}
        None => errors.push(err("name", "This field is required.".to_owned())),
        Some(Value::Null) => errors.push(err("name", "This field may not be null.".to_owned())),
        Some(Value::String(text)) => {
            out.present.push("name");
            if text.is_empty() {
                errors.push(err("name", "This field may not be blank.".to_owned()));
            } else if text.chars().count() > 255 {
                errors.push(err(
                    "name",
                    "Ensure this field has no more than 255 characters.".to_owned(),
                ));
            } else {
                out.name = Some(text.clone());
            }
        }
        Some(Value::Bool(_)) | Some(Value::Array(_)) | Some(Value::Object(_)) => {
            errors.push(err("name", "Not a valid string.".to_owned()));
        }
        // DRF CharField coerces numbers to strings.
        Some(Value::Number(n)) => {
            out.present.push("name");
            out.name = Some(n.to_string());
        }
    }
    // `description`: TextField(blank=True) — default `""`, no max length.
    match object.get("description") {
        None => {
            if !partial {
                out.description = Some(Value::String(String::new()));
            }
        }
        Some(Value::Null) => {
            errors.push(err("description", "This field may not be null.".to_owned()));
        }
        Some(Value::String(text)) => {
            out.present.push("description");
            out.description = Some(Value::String(text.clone()));
        }
        Some(Value::Bool(_)) | Some(Value::Array(_)) | Some(Value::Object(_)) => {
            errors.push(err("description", "Not a valid string.".to_owned()));
        }
        Some(Value::Number(n)) => {
            out.present.push("description");
            out.description = Some(Value::String(n.to_string()));
        }
    }

    // `start_date` / `end_date`: DateTimeField(null+blank).
    for field in ["start_date", "end_date"] {
        match object.get(field) {
            None => {}
            Some(Value::Null) => {
                out.present.push(field);
            }
            Some(value) => match parse_serializer_date(value, timezone) {
                Ok(parsed) => {
                    out.present.push(field);
                    if field == "start_date" {
                        out.start = Some(parsed.instant);
                        out.start_date = Some(parsed.date);
                    } else {
                        out.end = Some(parsed.instant);
                        out.end_date = Some(parsed.date);
                    }
                }
                Err(_) => errors.push(err(
                    field,
                    format!(
                        "Datetime has wrong format. Use one of these formats instead: {DATETIME_FORMAT_HINT}."
                    ),
                )),
            },
        }
    }

    // JSON fields: any JSON passes through verbatim (even strings that
    // are not JSON text — DRF `JSONField` does not re-parse); explicit
    // null is rejected; absent is `{}` on create.
    for field in ["view_props", "progress_snapshot", "logo_props"] {
        match object.get(field) {
            None => {
                if !partial {
                    let target = match field {
                        "view_props" => &mut out.view_props,
                        "progress_snapshot" => &mut out.progress_snapshot,
                        _ => &mut out.logo_props,
                    };
                    *target = Some(Value::Object(Map::new()));
                }
            }
            Some(Value::Null) => {
                errors.push(err(field, "This field may not be null.".to_owned()));
            }
            Some(value) => {
                out.present.push(field);
                let target = match field {
                    "view_props" => &mut out.view_props,
                    "progress_snapshot" => &mut out.progress_snapshot,
                    _ => &mut out.logo_props,
                };
                *target = Some(value.clone());
            }
        }
    }

    // `sort_order`: FloatField(default=65535).
    match object.get("sort_order") {
        None => {}
        Some(Value::Null) => {
            errors.push(err("sort_order", "This field may not be null.".to_owned()));
        }
        Some(Value::Number(n)) => {
            out.present.push("sort_order");
            out.sort_order = n.as_f64();
        }
        Some(Value::String(text)) => match text.parse::<f64>() {
            Ok(n) => {
                out.present.push("sort_order");
                out.sort_order = Some(n);
            }
            Err(_) => errors.push(err("sort_order", "A valid number is required.".to_owned())),
        },
        _ => errors.push(err("sort_order", "A valid number is required.".to_owned())),
    }

    // `external_source` / `external_id`: CharField(max_length=255,
    // null+blank).
    for field in ["external_source", "external_id"] {
        match object.get(field) {
            None => {}
            Some(Value::Null) => {
                out.present.push(field);
            }
            Some(Value::String(text)) => {
                if text.chars().count() > 255 {
                    errors.push(err(
                        field,
                        "Ensure this field has no more than 255 characters.".to_owned(),
                    ));
                } else {
                    out.present.push(field);
                    let target = if field == "external_source" {
                        &mut out.external_source
                    } else {
                        &mut out.external_id
                    };
                    *target = Some(Value::String(text.clone()));
                }
            }
            Some(Value::Bool(_)) | Some(Value::Array(_)) | Some(Value::Object(_)) => {
                errors.push(err(field, "Not a valid string.".to_owned()));
            }
            Some(Value::Number(n)) => {
                out.present.push(field);
                let target = if field == "external_source" {
                    &mut out.external_source
                } else {
                    &mut out.external_id
                };
                *target = Some(Value::String(n.to_string()));
            }
        }
    }

    // `timezone`: CharField(choices=TIMEZONE_CHOICES, default="UTC").
    match object.get("timezone") {
        None => {
            if !partial {
                out.timezone_name = Some("UTC".to_owned());
            }
        }
        Some(Value::Null) => {
            errors.push(err("timezone", "This field may not be null.".to_owned()));
        }
        Some(Value::String(text)) => {
            // `pytz.common_timezones` membership; `chrono-tz` parses the
            // same IANA names (documented approximation for exotic
            // aliases, never hit by the suite).
            if text.parse::<Tz>().is_err() {
                errors.push(err(
                    "timezone",
                    format!("\"{text}\" is not a valid choice."),
                ));
            } else {
                out.present.push("timezone");
                out.timezone_name = Some(text.clone());
            }
        }
        Some(Value::Bool(_)) | Some(Value::Array(_)) | Some(Value::Object(_)) => {
            errors.push(err("timezone", "Not a valid string.".to_owned()));
        }
        Some(Value::Number(n)) => {
            let text = n.to_string();
            if text.parse::<Tz>().is_err() {
                errors.push(err(
                    "timezone",
                    format!("\"{text}\" is not a valid choice."),
                ));
            } else {
                out.present.push("timezone");
                out.timezone_name = Some(text);
            }
        }
    }

    // `version`: IntegerField(default=1).
    match object.get("version") {
        None => {
            if !partial {
                out.version = Some(1);
            }
        }
        Some(Value::Null) => {
            errors.push(err("version", "This field may not be null.".to_owned()));
        }
        Some(Value::Number(n)) => {
            if n.is_i64() {
                out.present.push("version");
                out.version = n.as_i64();
            } else {
                errors.push(err("version", "A valid integer is required.".to_owned()));
            }
        }
        Some(Value::String(text)) => match text.parse::<i64>() {
            Ok(n) => {
                out.present.push("version");
                out.version = Some(n);
            }
            Err(_) => errors.push(err("version", "A valid integer is required.".to_owned())),
        },
        _ => errors.push(err("version", "A valid integer is required.".to_owned())),
    }

    // `created_by` / `updated_by`: writable PK refs (validated, then
    // overwritten by `BaseModel.save` on create; honored on update).
    for field in ["created_by", "updated_by"] {
        match object.get(field) {
            None | Some(Value::Null) => {}
            Some(Value::String(text)) => match text.parse::<uuid::Uuid>() {
                Ok(id) => {
                    out.present.push(field);
                    if field == "created_by" {
                        out.created_by = Some(id);
                    } else {
                        out.updated_by = Some(id);
                    }
                }
                Err(_) => errors.push(err(
                    field,
                    format!("Invalid pk \"{text}\" - object does not exist."),
                )),
            },
            _ => errors.push(err(
                field,
                "Incorrect type. Expected pk value, received value.".to_owned(),
            )),
        }
    }

    // `deleted_at`: writable datetime-or-null (honored on update).
    match object.get("deleted_at") {
        None | Some(Value::Null) => {}
        Some(value) => match parse_serializer_date(value, timezone) {
            Ok(parsed) => {
                out.present.push("deleted_at");
                out.deleted_at = Some(parsed.instant);
            }
            Err(_) => errors.push(err(
                "deleted_at",
                format!(
                    "Datetime has wrong format. Use one of these formats instead: {DATETIME_FORMAT_HINT}."
                ),
            )),
        },
    }

    if !errors.is_empty() {
        // Error dict in model field order; messages are singletons here.
        let mut body = String::from("{");
        for (index, (field, message)) in errors.iter().enumerate() {
            if index > 0 {
                body.push(',');
            }
            body.push_str(&format!(
                "{}:[{}]",
                json_string(field),
                json_string(message)
            ));
        }
        body.push('}');
        return Err((StatusCode::BAD_REQUEST, body));
    }
    // Referenced-PK existence: `Invalid pk` when the row is gone (DRF
    // `PrimaryKeyRelatedField` queryset check). Runs after the shape
    // checks, like DRF's field order.
    Ok(out)
}

/// Check referenced-user existence for validated `created_by` /
/// `updated_by` values (`Invalid pk ... object does not exist.`).
async fn check_user_refs(
    pool: &sqlx::PgPool,
    write: &CycleWrite,
) -> Result<(), (StatusCode, String)> {
    for (field, id) in [
        ("created_by", write.created_by),
        ("updated_by", write.updated_by),
    ] {
        if write.has(field) {
            if let Some(id) = id {
                let row: Option<(uuid::Uuid,)> =
                    sqlx::query_as(r#"SELECT id FROM users WHERE id = $1"#)
                        .bind(id)
                        .fetch_optional(pool)
                        .await
                        .map_err(|_| {
                            (
                                StatusCode::INTERNAL_SERVER_ERROR,
                                SERVER_ERROR_BODY.to_owned(),
                            )
                        })?;
                if row.is_none() {
                    let body = format!(
                        "{{\"{field}\":[\"Invalid pk \\\"{id}\\\" - object does not exist.\"]}}"
                    );
                    return Err((StatusCode::BAD_REQUEST, body));
                }
            }
        }
    }
    Ok(())
}
// ---------------------------------------------------------------------------
// convert_to_utc rewrite (`utils/timezone_converter.py:40-94`)
// ---------------------------------------------------------------------------

/// Resolve the project whose timezone the rewrite uses
/// (`serializers/cycle.py:24-28`): `initial_data.project_id` wins, then
/// the instance's, then the request context's — first non-null wins. On
/// create there is no instance, so the raw body value (truthy) wins over
/// the URL project; on update the instance project is the URL project.
/// A truthy non-UUID body value makes `Project.objects.get` raise
/// `ValidationError` (400 invalid-detail); a well-formed but missing id
/// raises `DoesNotExist` (404).
async fn rewrite_project_id(
    pool: &sqlx::PgPool,
    raw_body: &Map<String, Value>,
    url_project: &uuid::Uuid,
) -> Result<uuid::Uuid, Denial> {
    let override_id = match raw_body.get("project_id") {
        None | Some(Value::Null) => None,
        Some(Value::String(text)) if text.is_empty() => None,
        Some(Value::String(text)) => match text.parse::<uuid::Uuid>() {
            Ok(id) => Some(id),
            Err(_) => return Err(Denial::BadDetail),
        },
        // Truthy non-strings (`0`/`false` are falsy and fall through):
        // `objects.get(id=...)` raises `ValidationError`.
        Some(Value::Number(n)) => {
            if n.as_i64() == Some(0) || n.as_f64() == Some(0.0) {
                None
            } else {
                return Err(Denial::BadDetail);
            }
        }
        Some(Value::Bool(false)) => None,
        Some(_) => return Err(Denial::BadDetail),
    };
    let id = override_id.unwrap_or(*url_project);
    let row: Option<(uuid::Uuid,)> =
        sqlx::query_as(r#"SELECT id FROM projects WHERE id = $1 AND deleted_at IS NULL"#)
            .bind(id)
            .fetch_optional(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    match row {
        Some((id,)) => Ok(id),
        None => Err(if override_id.is_some() {
            Denial::NotFound
        } else {
            // Unreached through the normal flow (`project_row` already
            // proved the URL project), kept for the override parity.
            Denial::NotFound
        }),
    }
}

/// Fetch the rewrite timezone; a missing/empty timezone raises
/// `ValueError` (an uncaught 500 in the `validate()` path — ported, not
/// softened, `timezone_converter.py:53-55`).
async fn rewrite_timezone(pool: &sqlx::PgPool, project_id: &uuid::Uuid) -> Result<Tz, Denial> {
    let row: Option<(String,)> =
        sqlx::query_as(r#"SELECT timezone FROM projects WHERE id = $1 AND deleted_at IS NULL"#)
            .bind(project_id)
            .fetch_optional(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    match row {
        Some((tz,)) if !tz.is_empty() => tz.parse::<Tz>().map_err(|_| Denial::ServerError),
        _ => Err(Denial::ServerError),
    }
}

/// `convert_to_utc` for a start date (`timezone_converter.py:69-83`):
/// local midnight plus one second, shifted to UTC — unless the date is
/// today in the project zone, when the current instant is returned.
/// `pytz.localize(is_dst=False)` picks standard time for ambiguous wall
/// times (the later instant); a DST gap raises (ported as 500).
fn convert_start(
    project_tz: &Tz,
    date: chrono::NaiveDate,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<chrono::DateTime<chrono::Utc>, Denial> {
    if date == now.with_timezone(project_tz).date_naive() {
        return Ok(now);
    }
    let naive = date.and_hms_opt(0, 0, 0).expect("midnight exists");
    use chrono::{LocalResult, TimeZone as _};
    match project_tz.from_local_datetime(&naive) {
        LocalResult::Single(local) => {
            Ok(local.with_timezone(&chrono::Utc) + chrono::Duration::seconds(1))
        }
        LocalResult::Ambiguous(_, later) => {
            Ok(later.with_timezone(&chrono::Utc) + chrono::Duration::seconds(1))
        }
        LocalResult::None => Err(Denial::ServerError),
    }
}

/// `convert_to_utc` for an end date (`timezone_converter.py:84-94`):
/// local 23:59:00 shifted to UTC. No same-day branch.
fn convert_end(
    project_tz: &Tz,
    date: chrono::NaiveDate,
) -> Result<chrono::DateTime<chrono::Utc>, Denial> {
    let naive = date.and_hms_opt(23, 59, 0).expect("23:59 exists");
    use chrono::{LocalResult, TimeZone as _};
    match project_tz.from_local_datetime(&naive) {
        LocalResult::Single(local) => Ok(local.with_timezone(&chrono::Utc)),
        LocalResult::Ambiguous(_, later) => Ok(later.with_timezone(&chrono::Utc)),
        LocalResult::None => Err(Denial::ServerError),
    }
}

/// The `validate()` tail (`serializers/cycle.py:16-38`): ordering check
/// then the `convert_to_utc` rewrite, both only when both dates are
/// non-null. Returns the rewritten pair. The rewritten dates come from
/// the parsed inputs (`str(data["start_date"].date())`), which read
/// aware datetimes in their own offset — never re-zoned into the actor
/// zone (see [`ParsedDate`]).
async fn validate_and_rewrite(
    pool: &sqlx::PgPool,
    raw_body: &Map<String, Value>,
    url_project: &uuid::Uuid,
    start: Option<chrono::DateTime<chrono::Utc>>,
    end: Option<chrono::DateTime<chrono::Utc>>,
    start_date: Option<chrono::NaiveDate>,
    end_date: Option<chrono::NaiveDate>,
) -> Result<
    (
        Option<chrono::DateTime<chrono::Utc>>,
        Option<chrono::DateTime<chrono::Utc>>,
    ),
    Denial,
> {
    let (Some(start), Some(end)) = (start, end) else {
        return Ok((start, end));
    };
    if start > end {
        return Err(Denial::Raw(
            StatusCode::BAD_REQUEST,
            START_AFTER_END_BODY.to_owned(),
        ));
    }
    let now = chrono::Utc::now();
    let rewrite_project = rewrite_project_id(pool, raw_body, url_project).await?;
    let project_tz = rewrite_timezone(pool, &rewrite_project).await?;
    let (Some(start_date), Some(end_date)) = (start_date, end_date) else {
        return Err(Denial::ServerError);
    };
    let start = convert_start(&project_tz, start_date, now)?;
    let end = convert_end(&project_tz, end_date)?;
    Ok((Some(start), Some(end)))
}

// ---------------------------------------------------------------------------
// Task publishes (deferred, best-effort)
// ---------------------------------------------------------------------------

/// Celery wire name for `model_activity`
/// (`bgtasks/webhook_task.py:463`).
const MODEL_ACTIVITY_TASK: &str = "pi_dash.bgtasks.webhook_task.model_activity";
/// Celery wire name for `recent_visited_task` (bare `@shared_task`
/// default).
const RECENT_VISITED_TASK: &str = "pi_dash.bgtasks.recent_visited_task.recent_visited_task";
/// Celery wire name for `issue_activity`
/// (`bgtasks/issue_activities_task.py:1504`).
const ISSUE_ACTIVITY_TASK: &str = "pi_dash.bgtasks.issue_activities_task.issue_activity";

/// One `model_activity.delay(...)` enqueue (`base.py:318-326`,
/// `:397-405`): kwargs in call order; `requested_data` is the raw
/// `request.data` object; `current_instance` is `None` on create and the
/// before-image JSON string on update. UUIDs render as strings (kombu's
/// JSON encoder does that in Python).
fn model_activity_message(
    model_id: &uuid::Uuid,
    requested_data: Value,
    current_instance: Option<String>,
    actor_id: &uuid::Uuid,
    slug: &str,
    origin: &str,
) -> pidash_jobs::celery::CeleryTaskMessage {
    let mut kwargs = Map::with_capacity(7);
    kwargs.insert("model_name".to_owned(), Value::String("cycle".to_owned()));
    kwargs.insert("model_id".to_owned(), Value::String(model_id.to_string()));
    kwargs.insert("requested_data".to_owned(), requested_data);
    kwargs.insert(
        "current_instance".to_owned(),
        current_instance.map_or(Value::Null, Value::String),
    );
    kwargs.insert("actor_id".to_owned(), Value::String(actor_id.to_string()));
    kwargs.insert("slug".to_owned(), Value::String(slug.to_owned()));
    kwargs.insert("origin".to_owned(), Value::String(origin.to_owned()));
    pidash_jobs::celery::CeleryTaskMessage::new(MODEL_ACTIVITY_TASK, vec![], kwargs)
}

/// One `recent_visited_task.delay(...)` enqueue (`base.py:468-474`).
fn recent_visited_message(
    pk: &uuid::Uuid,
    user_id: &uuid::Uuid,
    project_id: &uuid::Uuid,
    slug: &str,
) -> pidash_jobs::celery::CeleryTaskMessage {
    let mut kwargs = Map::with_capacity(5);
    kwargs.insert("entity_name".to_owned(), Value::String("cycle".to_owned()));
    kwargs.insert(
        "entity_identifier".to_owned(),
        Value::String(pk.to_string()),
    );
    kwargs.insert("user_id".to_owned(), Value::String(user_id.to_string()));
    kwargs.insert(
        "project_id".to_owned(),
        Value::String(project_id.to_string()),
    );
    kwargs.insert("slug".to_owned(), Value::String(slug.to_owned()));
    pidash_jobs::celery::CeleryTaskMessage::new(RECENT_VISITED_TASK, vec![], kwargs)
}

/// One `issue_activity.delay(...)` enqueue (`base.py:483-499`): nine
/// kwargs exactly as the view passes them (`subscriber` keeps its
/// default); `requested_data` is the `json.dumps` STRING of the
/// cycle/issue payload.
fn issue_activity_message(
    pk: &uuid::Uuid,
    cycle_name: &str,
    issue_ids: &[uuid::Uuid],
    actor_id: &uuid::Uuid,
    project_id: &uuid::Uuid,
    epoch: i64,
    origin: &str,
) -> pidash_jobs::celery::CeleryTaskMessage {
    let payload = serde_json::json!({
        "cycle_id": pk.to_string(),
        "cycle_name": cycle_name.to_owned(),
        "issues": issue_ids.iter().map(uuid::Uuid::to_string).collect::<Vec<_>>(),
    });
    let mut kwargs = Map::with_capacity(9);
    kwargs.insert(
        "type".to_owned(),
        Value::String("cycle.activity.deleted".to_owned()),
    );
    kwargs.insert(
        "requested_data".to_owned(),
        Value::String(payload.to_string()),
    );
    kwargs.insert("actor_id".to_owned(), Value::String(actor_id.to_string()));
    kwargs.insert("issue_id".to_owned(), Value::String(pk.to_string()));
    kwargs.insert(
        "project_id".to_owned(),
        Value::String(project_id.to_string()),
    );
    kwargs.insert("current_instance".to_owned(), Value::Null);
    kwargs.insert("epoch".to_owned(), Value::from(epoch));
    kwargs.insert("notification".to_owned(), Value::Bool(true));
    kwargs.insert("origin".to_owned(), Value::String(origin.to_owned()));
    pidash_jobs::celery::CeleryTaskMessage::new(ISSUE_ACTIVITY_TASK, vec![], kwargs)
}

/// `soft_delete_related_objects.delay("db", "cycle", pk, "default")`
/// (`db/mixins.py:77`): positional args, no kwargs.
fn soft_delete_message(pk: &uuid::Uuid) -> pidash_jobs::celery::CeleryTaskMessage {
    pidash_jobs::celery::CeleryTaskMessage::new(
        pidash_jobs::tasks_cleanup::deletion::SOFT_DELETE_TASK,
        vec![
            Value::String("db".to_owned()),
            Value::String("cycle".to_owned()),
            Value::String(pk.to_string()),
            Value::String("default".to_owned()),
        ],
        Map::new(),
    )
}
// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `GET .../cycles/` (`base.py:183-268`): GUEST list over the annotated
/// queryset. `cycle_view=current` narrows to the current window; its
/// empty fallthrough re-reads the same filtered queryset, so it answers
/// `[]` (ported bug 1). The nonempty current branch projects
/// `completed_issues` before `cancelled_issues` (ported bug 2).
async fn cycle_list(
    State(state): State<AppState>,
    Path((slug, project_raw)): Path<(String, String)>,
    Query(query): Query<crate::app_issues::QueryMap>,
    extension: Option<axum::Extension<SessionHandle>>,
) -> Result<Response, Denial> {
    let resolved = actor(&state, extension).await?;
    let pool = pool_of(&state)?.clone();
    let user_id = resolved.id;
    let timezone = resolved.timezone;
    let project_id = resolve_project_id(&pool, &slug, &project_raw).await?;
    check_gate(
        &pool,
        &slug,
        &project_id,
        &user_id,
        "GET",
        "workspaces/<slug>/projects/<id>/cycles/",
        false,
    )
    .await?;
    // `Project.objects.get` inside `get_queryset` runs before any row is
    // read: a missing project is the `ObjectDoesNotExist` 404.
    project_row(&pool, &project_id).await?;
    let now = chrono::Utc::now();
    let cycle_view = crate::app_issues::query_last(&query, "cycle_view");
    if cycle_view.as_deref() == Some("current") {
        // The fallthrough re-reads the already-filtered queryset
        // (`:205` + `:239`): zero current cycles answer `[]`, not ALL.
        let rows = fetch_cycle_rows(&pool, &slug, &project_id, &user_id, &now, true).await?;
        let mut out = String::from("[");
        for (index, row) in rows.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            out.push_str(&shape_current_row(row, &timezone));
        }
        out.push(']');
        return Ok(json_response(StatusCode::OK, out));
    }
    let rows = fetch_cycle_rows(&pool, &slug, &project_id, &user_id, &now, false).await?;
    let mut out = String::from("[");
    for (index, row) in rows.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str(&shape_list_row(row, &timezone));
    }
    out.push(']');
    Ok(json_response(StatusCode::OK, out))
}

/// Normalize a write body the way the view sees `request.data`: JSON
/// `null`/missing parses to `{}`, an object passes through, and any
/// other JSON shape hits the attribute errors the view code hits (the
/// generic 500 — verified: `[]` answers 500, `null` answers the
/// name-required 400).
fn write_object(body: Value) -> Result<Map<String, Value>, Denial> {
    match body {
        Value::Object(map) => Ok(map),
        Value::Null => Ok(Map::new()),
        _ => Err(Denial::ServerError),
    }
}

/// Raw XOR date rule (`base.py:272-274`): both dates null-or-absent, or
/// both present-and-non-null — anything else is the 400. Runs on the raw
/// object, before serializer validation.
fn check_date_xor(object: &Map<String, Value>) -> Result<(bool, bool), Denial> {
    let has = |key: &str| matches!(object.get(key), Some(v) if !v.is_null());
    let (start, end) = (has("start_date"), has("end_date"));
    if start != end {
        return Err(Denial::Raw(
            StatusCode::BAD_REQUEST,
            DATE_XOR_BODY.to_owned(),
        ));
    }
    Ok((start, end))
}

/// `POST .../cycles/` (`base.py:270-333`): MEMBER create — 201 with the
/// annotated re-read (narrow projection, no `cancelled_issues`).
async fn cycle_create(
    State(state): State<AppState>,
    Path((slug, project_raw)): Path<(String, String)>,
    extension: Option<axum::Extension<SessionHandle>>,
    req: axum::extract::Request,
) -> Result<Response, Denial> {
    let resolved = actor(&state, extension).await?;
    let pool = pool_of(&state)?.clone();
    let user_id = resolved.id;
    let timezone = resolved.timezone;
    let project_id = resolve_project_id(&pool, &slug, &project_raw).await?;
    check_gate(
        &pool,
        &slug,
        &project_id,
        &user_id,
        "POST",
        "workspaces/<slug>/projects/<id>/cycles/",
        false,
    )
    .await?;
    let body = match detail_body(&state, req).await {
        Ok(body) => body,
        Err(into) => return Ok(into),
    };
    let object = write_object(body)?;
    check_date_xor(&object)?;
    let write = validate_write(&object, false, &timezone)
        .map_err(|(status, body)| Denial::Raw(status, body))?;
    check_user_refs(&pool, &write)
        .await
        .map_err(|(status, body)| Denial::Raw(status, body))?;
    let (start, end) = validate_and_rewrite(
        &pool,
        &object,
        &project_id,
        write.start,
        write.end,
        write.start_date,
        write.end_date,
    )
    .await?;
    // `Project.objects.get` (via the rewrite, or here for the dateless
    // path whose save would dereference the project): a missing project
    // is the `ObjectDoesNotExist` 404 either way.
    let project = project_row(&pool, &project_id).await?;
    let now = chrono::Utc::now();
    // `Cycle.save`: min sort over the project's live cycles minus 10000
    // whenever one exists — even over an explicit input (ported bug 5).
    let min_sort: Option<(Option<f64>,)> = sqlx::query_as(
        r#"SELECT MIN(sort_order) FROM cycles WHERE project_id = $1 AND deleted_at IS NULL"#,
    )
    .bind(project_id)
    .fetch_optional(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let sort_order = match min_sort.and_then(|row| row.0) {
        Some(min) => min - 10000.0,
        None => write.sort_order.unwrap_or(65535.0),
    };
    let id = uuid::Uuid::new_v4();
    let insert = sqlx::query(
        r#"INSERT INTO cycles (id, created_at, updated_at, name, description,
            start_date, end_date, created_by_id, owned_by_id, project_id,
            updated_by_id, workspace_id, view_props, sort_order,
            external_source, external_id, progress_snapshot, archived_at,
            logo_props, deleted_at, timezone, version)
           VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22)"#,
    )
    .bind(id)
    .bind(now)
    .bind(now)
    .bind(write.name.clone().unwrap_or_default())
    .bind(
        write
            .description
            .clone()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default(),
    )
    .bind(start)
    .bind(end)
    .bind(user_id)
    .bind(user_id)
    .bind(project_id)
    .bind(None::<uuid::Uuid>)
    .bind(project.workspace_id)
    .bind(write.view_props.clone().unwrap_or(Value::Object(Map::new())))
    .bind(sort_order)
    .bind(write.external_source.clone().and_then(|v| match v {
        Value::String(s) => Some(s),
        _ => None,
    }))
    .bind(write.external_id.clone().and_then(|v| match v {
        Value::String(s) => Some(s),
        _ => None,
    }))
    .bind(write.progress_snapshot.clone().unwrap_or(Value::Object(Map::new())))
    .bind(None::<chrono::DateTime<chrono::Utc>>)
    .bind(write.logo_props.clone().unwrap_or(Value::Object(Map::new())))
    .bind(None::<chrono::DateTime<chrono::Utc>>)
    .bind(write.timezone_name.clone().unwrap_or_else(|| "UTC".to_owned()))
    .bind(write.version.unwrap_or(1))
    .execute(&pool)
    .await
    .map_err(|error| match &error {
        sqlx::Error::Database(db) if db.code().as_deref().unwrap_or("").starts_with("23") => {
            Denial::BadPayload
        }
        _ => Denial::ServerError,
    })?;
    debug_assert_eq!(insert.rows_affected(), 1);
    let row = fetch_cycle_by_pk(&pool, &slug, &project_id, &user_id, &now, &id, false, false)
        .await?
        .ok_or(Denial::ServerError)?;
    if let Ok(origin) = request_origin(&state) {
        enqueue_message(
            &pool,
            model_activity_message(&id, Value::Object(object), None, &user_id, &slug, &origin),
        )
        .await;
    }
    Ok(json_response(
        StatusCode::CREATED,
        shape_write_row(&row, &timezone),
    ))
}

/// `PATCH .../cycles/<pk>/` (`base.py:335-408`): MEMBER partial update.
/// A missing row answers the generic 500 (ported bug 4); archived and
/// completed guards precede serializer validation; the completed-cycle
/// narrowing is dead code (ported bug 3).
async fn cycle_partial_update(
    State(state): State<AppState>,
    Path((slug, project_raw, pk)): Path<(String, String, String)>,
    extension: Option<axum::Extension<SessionHandle>>,
    req: axum::extract::Request,
) -> Result<Response, Denial> {
    if pk.parse::<uuid::Uuid>().is_err() {
        return Ok(crate::edge::proxy(State(state), req).await);
    }
    let resolved = actor(&state, extension).await?;
    let pool = pool_of(&state)?.clone();
    let user_id = resolved.id;
    let timezone = resolved.timezone;
    let project_id = resolve_project_id(&pool, &slug, &project_raw).await?;
    check_gate(
        &pool,
        &slug,
        &project_id,
        &user_id,
        "PATCH",
        "workspaces/<slug>/projects/<id>/cycles/<uuid>/",
        false,
    )
    .await?;
    let body = match detail_body(&state, req).await {
        Ok(body) => body,
        Err(into) => return Ok(into),
    };
    let object = write_object(body)?;
    let pk = parse_pk(&pk).ok_or(Denial::BadDetail)?;
    // `get_queryset` (with its `Project.objects.get`) runs before the
    // row read: a missing project is the 404, and an archived project
    // behaves like a missing row (the 500 below).
    let project = project_row(&pool, &project_id).await?;
    let now = chrono::Utc::now();
    let before = if project.archived_at.is_some() {
        None
    } else {
        fetch_cycle_by_pk(&pool, &slug, &project_id, &user_id, &now, &pk, false, true).await?
    };
    let Some(before) = before else {
        // `cycle.archived_at` on `None`: `AttributeError` → generic 500.
        return Err(Denial::ServerError);
    };
    if before.get("archived_at").is_some_and(|v| !v.is_null()) {
        return Err(Denial::Raw(
            StatusCode::BAD_REQUEST,
            ARCHIVED_UPDATE_BODY.to_owned(),
        ));
    }
    // The completed-cycle branch reads the RAW body (`request_data`,
    // `:349-357`); the narrowing it computes is never used (ported
    // bug 3), so only the gate half is ported here.
    if let Some(Value::String(end_text)) = before.get("end_date") {
        if let Ok(end) = chrono::DateTime::parse_from_rfc3339(end_text) {
            if end.with_timezone(&chrono::Utc) < now && !object.contains_key("sort_order") {
                return Err(Denial::Raw(
                    StatusCode::BAD_REQUEST,
                    COMPLETED_UPDATE_BODY.to_owned(),
                ));
            }
        }
    }
    let write = validate_write(&object, true, &timezone)
        .map_err(|(status, body)| Denial::Raw(status, body))?;
    check_user_refs(&pool, &write)
        .await
        .map_err(|(status, body)| Denial::Raw(status, body))?;
    let (start, end) = validate_and_rewrite(
        &pool,
        &object,
        &project_id,
        write.start,
        write.end,
        write.start_date,
        write.end_date,
    )
    .await?;
    let current_instance =
        serde_json::to_string(&Value::Object(before.clone())).unwrap_or("null".to_owned());
    apply_cycle_update(&pool, &pk, &user_id, &now, &write, start, end).await?;
    let row = fetch_cycle_by_pk(&pool, &slug, &project_id, &user_id, &now, &pk, false, false)
        .await?
        .ok_or(Denial::ServerError)?;
    if let Ok(origin) = request_origin(&state) {
        enqueue_message(
            &pool,
            model_activity_message(
                &pk,
                Value::Object(object),
                Some(current_instance),
                &user_id,
                &slug,
                &origin,
            ),
        )
        .await;
    }
    Ok(json_response(
        StatusCode::OK,
        shape_write_row(&row, &timezone),
    ))
}

/// Apply one validated partial update: only sent fields move, plus the
/// audit stamp (`BaseModel.save` sets `updated_by`, never `created_by`,
/// `db/models/base.py:23-42`).
#[allow(clippy::too_many_arguments)]
async fn apply_cycle_update(
    pool: &sqlx::PgPool,
    pk: &uuid::Uuid,
    user_id: &uuid::Uuid,
    now: &chrono::DateTime<chrono::Utc>,
    write: &CycleWrite,
    start: Option<chrono::DateTime<chrono::Utc>>,
    end: Option<chrono::DateTime<chrono::Utc>>,
) -> Result<(), Denial> {
    // Static SET list: every column bound positionally, untouched columns
    // keep their values via `COALESCE`-free conditional updates below.
    // Simpler and audit-clean: one UPDATE per sent field would reorder
    // `updated_at` writes, so instead a single UPDATE sets exactly the
    // sent columns using `CASE WHEN $n IS NOT DISTINCT FROM ...` sentinels
    // is overkill — build the SET clause dynamically over sent fields.
    let mut sets: Vec<String> = Vec::new();
    let mut index = 3;
    if write.has("name") {
        sets.push(format!("name = ${index}"));
        index += 1;
    }
    if write.has("description") {
        sets.push(format!("description = ${index}"));
        index += 1;
    }
    if write.has("start_date") {
        sets.push(format!("start_date = ${index}"));
        index += 1;
    }
    if write.has("end_date") {
        sets.push(format!("end_date = ${index}"));
        index += 1;
    }
    if write.has("view_props") {
        sets.push(format!("view_props = ${index}"));
        index += 1;
    }
    if write.has("sort_order") {
        sets.push(format!("sort_order = ${index}"));
        index += 1;
    }
    if write.has("external_source") {
        sets.push(format!("external_source = ${index}"));
        index += 1;
    }
    if write.has("external_id") {
        sets.push(format!("external_id = ${index}"));
        index += 1;
    }
    if write.has("progress_snapshot") {
        sets.push(format!("progress_snapshot = ${index}"));
        index += 1;
    }
    if write.has("logo_props") {
        sets.push(format!("logo_props = ${index}"));
        index += 1;
    }
    if write.has("timezone") {
        sets.push(format!("timezone = ${index}"));
        index += 1;
    }
    if write.has("version") {
        sets.push(format!("version = ${index}"));
        index += 1;
    }
    if write.has("created_by") {
        sets.push(format!("created_by_id = ${index}"));
        index += 1;
    }
    if write.has("deleted_at") {
        sets.push(format!("deleted_at = ${index}"));
        index += 1;
    }
    sets.push("updated_by_id = $1".to_owned());
    sets.push("updated_at = $2".to_owned());
    let sql = format!("UPDATE cycles SET {} WHERE id = ${index}", sets.join(", "));
    let mut query = sqlx::query(&sql).bind(user_id).bind(now);
    if write.has("name") {
        query = query.bind(write.name.clone().unwrap_or_default());
    }
    if write.has("description") {
        query = query.bind(
            write
                .description
                .clone()
                .and_then(|v| v.as_str().map(str::to_owned)),
        );
    }
    if write.has("start_date") {
        query = query.bind(start);
    }
    if write.has("end_date") {
        query = query.bind(end);
    }
    if write.has("view_props") {
        query = query.bind(write.view_props.clone().unwrap_or(Value::Null));
    }
    if write.has("sort_order") {
        query = query.bind(write.sort_order);
    }
    if write.has("external_source") {
        query = query.bind(write.external_source.clone().and_then(|v| match v {
            Value::String(s) => Some(s),
            _ => None,
        }));
    }
    if write.has("external_id") {
        query = query.bind(write.external_id.clone().and_then(|v| match v {
            Value::String(s) => Some(s),
            _ => None,
        }));
    }
    if write.has("progress_snapshot") {
        query = query.bind(write.progress_snapshot.clone().unwrap_or(Value::Null));
    }
    if write.has("logo_props") {
        query = query.bind(write.logo_props.clone().unwrap_or(Value::Null));
    }
    if write.has("timezone") {
        query = query.bind(write.timezone_name.clone());
    }
    if write.has("version") {
        query = query.bind(write.version);
    }
    if write.has("created_by") {
        query = query.bind(write.created_by);
    }
    if write.has("deleted_at") {
        query = query.bind(write.deleted_at);
    }
    query = query.bind(pk);
    query.execute(pool).await.map_err(|_| Denial::ServerError)?;
    Ok(())
}

/// `GET .../cycles/<pk>/` (`base.py:410-475`): MEMBER retrieve with the
/// `sub_issues` annotation; a miss is `{"error": "Cycle not found"}`.
async fn cycle_retrieve(
    State(state): State<AppState>,
    Path((slug, project_raw, pk)): Path<(String, String, String)>,
    extension: Option<axum::Extension<SessionHandle>>,
    req: axum::extract::Request,
) -> Result<Response, Denial> {
    if pk.parse::<uuid::Uuid>().is_err() {
        return Ok(crate::edge::proxy(State(state), req).await);
    }
    let resolved = actor(&state, extension).await?;
    let pool = pool_of(&state)?.clone();
    let user_id = resolved.id;
    let timezone = resolved.timezone;
    let project_id = resolve_project_id(&pool, &slug, &project_raw).await?;
    check_gate(
        &pool,
        &slug,
        &project_id,
        &user_id,
        "GET",
        "workspaces/<slug>/projects/<id>/cycles/<uuid>/",
        false,
    )
    .await?;
    project_row(&pool, &project_id).await?;
    let now = chrono::Utc::now();
    let pk = parse_pk(&pk).ok_or(Denial::BadDetail)?;
    let row =
        fetch_cycle_by_pk(&pool, &slug, &project_id, &user_id, &now, &pk, true, false).await?;
    let Some(row) = row else {
        return Err(Denial::CycleNotFound);
    };
    enqueue_message(
        &pool,
        recent_visited_message(&pk, &user_id, &project_id, &slug),
    )
    .await;
    Ok(json_response(
        StatusCode::OK,
        shape_retrieve_row(&row, &timezone),
    ))
}

/// `DELETE .../cycles/<pk>/` (`base.py:477-517`): ADMIN creator-gated
/// destroy — soft-deletes the cycle (the `TODO` stays a soft delete),
/// clears its favorites and recent visits, answers 204.
async fn cycle_destroy(
    State(state): State<AppState>,
    Path((slug, project_raw, pk)): Path<(String, String, String)>,
    extension: Option<axum::Extension<SessionHandle>>,
    req: axum::extract::Request,
) -> Result<Response, Denial> {
    if pk.parse::<uuid::Uuid>().is_err() {
        return Ok(crate::edge::proxy(State(state), req).await);
    }
    let resolved = actor(&state, extension).await?;
    let pool = pool_of(&state)?.clone();
    let user_id = resolved.id;
    let project_id = resolve_project_id(&pool, &slug, &project_raw).await?;
    let pk = parse_pk(&pk).ok_or(Denial::BadDetail)?;
    let creator = is_cycle_creator(&pool, &pk, &user_id).await?;
    check_gate(
        &pool,
        &slug,
        &project_id,
        &user_id,
        "DELETE",
        "workspaces/<slug>/projects/<id>/cycles/<uuid>/",
        creator,
    )
    .await?;
    // `Cycle.objects.get(...)`: no archived filter, live rows only — a
    // miss is the `ObjectDoesNotExist` 404 (ported bug 4).
    let row: Option<(String,)> = sqlx::query_as(
        r#"SELECT c.name FROM cycles c JOIN workspaces w ON w.id = c.workspace_id AND w.slug = $1
           WHERE c.id = $2 AND c.project_id = $3 AND c.deleted_at IS NULL"#,
    )
    .bind(&slug)
    .bind(pk)
    .bind(project_id)
    .fetch_optional(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let Some((cycle_name,)) = row else {
        return Err(Denial::NotFound);
    };
    // `cycle_issues` ids for the activity payload (live bridge rows;
    // note the ported `self.kwargs.get("pk")` — the cycle pk).
    let issue_rows: Vec<(uuid::Uuid,)> = sqlx::query_as(
        r#"SELECT issue_id FROM cycle_issues WHERE cycle_id = $1 AND deleted_at IS NULL"#,
    )
    .bind(pk)
    .fetch_all(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let issue_ids: Vec<uuid::Uuid> = issue_rows.into_iter().map(|row| row.0).collect();
    let now = chrono::Utc::now();
    if let Ok(origin) = request_origin(&state) {
        enqueue_message(
            &pool,
            issue_activity_message(
                &pk,
                &cycle_name,
                &issue_ids,
                &user_id,
                &project_id,
                now.timestamp(),
                &origin,
            ),
        )
        .await;
    }
    // Soft delete (`SoftDeleteModel.delete` → `save()` → crum stamps
    // `updated_by`, `auto_now` stamps `updated_at`).
    sqlx::query(
        r#"UPDATE cycles SET deleted_at = $2, updated_at = $2, updated_by_id = $3 WHERE id = $1"#,
    )
    .bind(pk)
    .bind(now)
    .bind(user_id)
    .execute(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    enqueue_message(&pool, soft_delete_message(&pk)).await;
    // Favorites go through the soft-delete queryset (only `deleted_at`
    // moves — a direct `update`, no `save()` stamp).
    sqlx::query(
        r#"UPDATE user_favorites SET deleted_at = $4
           WHERE user_id = $1 AND entity_type = 'cycle' AND entity_identifier = $2
             AND project_id = $3 AND deleted_at IS NULL"#,
    )
    .bind(user_id)
    .bind(pk)
    .bind(project_id)
    .bind(now)
    .execute(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    // Recent visits are hard-deleted (`delete(soft=False)`).
    sqlx::query(
        r#"DELETE FROM user_recent_visits USING workspaces w
           WHERE user_recent_visits.workspace_id = w.id AND w.slug = $1
             AND user_recent_visits.project_id = $2
             AND user_recent_visits.entity_identifier = $3
             AND user_recent_visits.entity_name = 'cycle'"#,
    )
    .bind(&slug)
    .bind(project_id)
    .bind(pk)
    .execute(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(json_response(StatusCode::NO_CONTENT, String::new()))
}
// ---------------------------------------------------------------------------
// Unit tests (pure shaping/validation; the live contract suite owns the DB)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use chrono_tz::UTC;
    use serde_json::json;

    fn row_map() -> Map<String, Value> {
        json!({
            "id": "11111111-1111-1111-1111-111111111111",
            "workspace_id": "22222222-2222-2222-2222-222222222222",
            "project_id": "33333333-3333-3333-3333-333333333333",
            "name": "Sprint 3",
            "description": "",
            "start_date": "2026-03-01T00:00:01+00:00",
            "end_date": "2026-03-31T23:59:00+00:00",
            "owned_by_id": "44444444-4444-4444-4444-444444444444",
            "view_props": {},
            "sort_order": 55535.0,
            "external_source": null,
            "external_id": null,
            "progress_snapshot": {},
            "logo_props": {},
            "version": 1,
            "created_by": "44444444-4444-4444-4444-444444444444",
            "is_favorite": true,
            "total_issues": 5,
            "completed_issues": 2,
            "cancelled_issues": 1,
            "assignee_ids": [],
            "status": "CURRENT",
            "sub_issues": 3,
            "archived_at": null,
        })
        .as_object()
        .expect("object")
        .clone()
    }

    #[test]
    fn list_projection_renders_model_fields_then_annotations() {
        // Live Django order: `...logo_props, version, created_by`,
        // then annotations in queryset-definition order
        // (`is_favorite, total, completed, cancelled, status, assignee`).
        let body = shape_list_row(&row_map(), &UTC);
        let version = body.find("\"version\"").expect("version");
        let created = body.find("\"created_by\"").expect("created");
        let favorite = body.find("\"is_favorite\"").expect("fav");
        let completed = body.find("\"completed_issues\"").expect("completed");
        let cancelled = body.find("\"cancelled_issues\"").expect("cancelled");
        let status = body.find("\"status\"").expect("status");
        let assignees = body.find("\"assignee_ids\"").expect("assignees");
        assert!(
            version < created
                && created < favorite
                && completed < cancelled
                && cancelled < status
                && status < assignees,
            "{body}"
        );
        assert!(body.starts_with("{\"id\":\"11111111-1111-1111-1111-111111111111\""));
        assert!(body.contains("\"sort_order\":55535.0"));
        assert!(body.contains("\"start_date\":\"2026-03-01T00:00:01Z\""));
        assert!(body.ends_with("\"assignee_ids\":[]}"));
    }

    #[test]
    fn current_projection_matches_list_bytes() {
        // The `.values()` order difference between the branches is
        // unobservable (see `TailOrder`).
        assert_eq!(
            shape_current_row(&row_map(), &UTC),
            shape_list_row(&row_map(), &UTC)
        );
    }

    #[test]
    fn write_projection_has_no_cancelled_issues() {
        let body = shape_write_row(&row_map(), &UTC);
        assert!(!body.contains("cancelled_issues"), "{body}");
        assert!(body.contains("\"completed_issues\":2"));
        assert!(body.contains(
            "\"version\":1,\"created_by\":\"44444444-4444-4444-4444-444444444444\",\"is_favorite\":true"
        ));
    }

    #[test]
    fn retrieve_projection_carries_trailing_sub_issues() {
        // Live Django order ends `..., status, assignee_ids, sub_issues`.
        let body = shape_retrieve_row(&row_map(), &UTC);
        assert!(body.ends_with(",\"sub_issues\":3}"), "{body}");
        let status = body.find("\"status\"").expect("status");
        let assignees = body.find("\"assignee_ids\"").expect("assignees");
        let sub = body.find("\"sub_issues\":3").expect("sub");
        assert!(status < assignees && assignees < sub, "{body}");
        assert!(!body.contains("cancelled_issues"), "{body}");
    }

    #[test]
    fn null_dates_and_missing_keys_render_null() {
        let mut row = row_map();
        row.insert("start_date".to_owned(), Value::Null);
        row.remove("created_by");
        let body = shape_write_row(&row, &UTC);
        assert!(body.contains("\"start_date\":null"), "{body}");
        assert!(body.contains("\"created_by\":null"), "{body}");
    }

    #[test]
    fn date_inputs_parse_like_drf() {
        let tz = UTC;
        // Full ISO with Z.
        let parsed = parse_serializer_date(&json!("2026-03-10T00:00:00Z"), &tz).expect("zulu");
        assert_eq!(parsed.instant.to_rfc3339(), "2026-03-10T00:00:00+00:00");
        // Date-only → midnight UTC under a UTC actor.
        let parsed = parse_serializer_date(&json!("2026-03-01"), &tz).expect("date");
        assert_eq!(parsed.instant.to_rfc3339(), "2026-03-01T00:00:00+00:00");
        // Garbage and non-strings fail invalid.
        assert!(parse_serializer_date(&json!("not-a-date"), &tz).is_err());
        assert!(parse_serializer_date(&json!(123), &tz).is_err());
        assert!(parse_serializer_date(&json!(true), &tz).is_err());
    }

    #[test]
    fn aware_dates_read_in_input_offset_not_actor_zone() {
        // `serializers/cycle.py:29-37` reads `data["start_date"].date()`
        // on the DRF-parsed value, which keeps an aware input's own
        // offset — a Zulu instant just after UTC midnight is still the
        // previous day even for a +05:30 actor.
        let actor: Tz = "Asia/Kolkata".parse().expect("tz");
        let parsed = parse_serializer_date(&json!("2026-02-28T23:30:00Z"), &actor).expect("aware");
        assert_eq!(
            parsed.date,
            chrono::NaiveDate::from_ymd_opt(2026, 2, 28).expect("date")
        );
        // Naive inputs keep their wall date (the actor-zone attach never
        // moves the wall clock).
        let parsed = parse_serializer_date(&json!("2026-02-28"), &actor).expect("date-only");
        assert_eq!(
            parsed.date,
            chrono::NaiveDate::from_ymd_opt(2026, 2, 28).expect("date")
        );
        let parsed = parse_serializer_date(&json!("2026-02-28T04:00:00"), &actor).expect("naive");
        assert_eq!(
            parsed.date,
            chrono::NaiveDate::from_ymd_opt(2026, 2, 28).expect("date")
        );
    }

    #[test]
    fn sub_issues_carries_issue_manager_exclusions() {
        // `Issue.issue_objects` (`db/models/issue.py:95-104`) drops
        // triage, archived and draft rows from the retrieve count.
        let select = annotated_select(true);
        assert!(
            select.contains("JOIN states s2 ON s2.id = si.state_id"),
            "{select}"
        );
        assert!(select.contains("si.archived_at IS NULL"), "{select}");
        assert!(select.contains("si.is_draft = FALSE"), "{select}");
        assert!(select.contains("s2.\"group\" != 'triage'"), "{select}");
        assert!(!annotated_select(false).contains("sub_issues"));
    }

    #[test]
    fn update_pre_read_keeps_archived_rows() {
        // `partial_update` reads through the unfiltered `get_queryset`
        // (`base.py:337-339`): only its pre-read keeps archived rows, so
        // the archived guard below it stays reachable.
        let pre = annotated_from(false, true, true);
        assert!(!pre.contains("c.archived_at IS NULL"), "{pre}");
        let reread = annotated_from(false, true, false);
        assert!(reread.contains("AND c.archived_at IS NULL"), "{reread}");
        let list = annotated_from(false, false, false);
        assert!(list.contains("AND c.archived_at IS NULL"), "{list}");
    }

    #[test]
    fn write_validation_mirrors_drf_bodies() {
        let tz = UTC;
        // Missing name on create.
        let err = validate_write(&Map::new(), false, &tz).expect_err("required");
        assert_eq!(err.0, StatusCode::BAD_REQUEST);
        assert_eq!(err.1, r#"{"name":["This field is required."]}"#);
        // Missing name on partial is fine.
        validate_write(&Map::new(), true, &tz).expect("partial empty");
        // Long name.
        let mut object = Map::new();
        object.insert("name".to_owned(), Value::String("x".repeat(300)));
        let err = validate_write(&object, false, &tz).expect_err("max");
        assert_eq!(
            err.1,
            r#"{"name":["Ensure this field has no more than 255 characters."]}"#
        );
        // Null view_props.
        let mut object = Map::new();
        object.insert("name".to_owned(), Value::String("N".to_owned()));
        object.insert("view_props".to_owned(), Value::Null);
        let err = validate_write(&object, false, &tz).expect_err("null json");
        assert_eq!(err.1, r#"{"view_props":["This field may not be null."]}"#);
        // Bad sort + bad version.
        let mut object = Map::new();
        object.insert("name".to_owned(), Value::String("N".to_owned()));
        object.insert("sort_order".to_owned(), Value::String("abc".to_owned()));
        let err = validate_write(&object, false, &tz).expect_err("sort");
        assert_eq!(err.1, r#"{"sort_order":["A valid number is required."]}"#);
    }

    #[test]
    fn denial_bodies_are_byte_exact() {
        assert_eq!(
            Denial::Unauthorized.status_and_body().1,
            UNAUTHENTICATED_BODY
        );
        assert_eq!(Denial::NotFound.status_and_body().1, NOT_FOUND_BODY);
        assert_eq!(
            Denial::CycleNotFound.status_and_body().1,
            CYCLE_NOT_FOUND_BODY
        );
        assert_eq!(
            Denial::Raw(StatusCode::BAD_REQUEST, DATE_XOR_BODY.to_owned())
                .status_and_body()
                .1,
            DATE_XOR_BODY
        );
    }

    #[test]
    fn convert_boundaries_mirror_timezone_converter() {
        // Project at +05:30: start is local midnight + 1s, end is local
        // 23:59, both in UTC.
        let tz: Tz = "Asia/Kolkata".parse().expect("tz");
        let now = chrono::DateTime::parse_from_rfc3339("2026-09-30T00:00:00Z")
            .expect("now")
            .with_timezone(&chrono::Utc);
        let date = chrono::NaiveDate::from_ymd_opt(2026, 3, 1).expect("date");
        let start = convert_start(&tz, date, now).expect("start");
        assert_eq!(start.to_rfc3339(), "2026-02-28T18:30:01+00:00");
        let end = convert_end(&tz, date).expect("end");
        assert_eq!(end.to_rfc3339(), "2026-03-01T18:29:00+00:00");
        // Same-day start returns now.
        let today = now.with_timezone(&tz).date_naive();
        assert_eq!(convert_start(&tz, today, now).expect("today"), now);
    }
}
