//! Activity + comments + reactions + subscribers handlers (D-26 handlers-D).
//!
//! Ports `apps/api/pi_dash/app/views/issue/activity.py:24-86`
//! (`IssueActivityEndpoint`), `comment.py:36-258` (`IssueCommentViewSet`,
//! `CommentReactionViewSet`), `reaction.py:25-85` (`IssueReactionViewSet`)
//! and `subscriber.py:16-104` (`IssueSubscriberViewSet`) — 5 units over the
//! 10 routes in `app/urls/issue.py:172-235`.
//!
//! Fixture ids: FX-ISS-17 (`rust-api/fixtures/app_issues/handlers/`) pins
//! every behavior below; FX-ISS-21 (`guards/`) pins the enqueue payloads.
//!
//! Layering: the read SQL is `super::queries_engage` builders executed
//! through `super::fetch_json_rows` (single-table leaves) or positional
//! decoding (multi-table joined selects — the builder columns are
//! unaliased, so `row_to_json` maps would collide); activity / subscriber
//! / reaction-lite shapes plus the sync predicates are
//! `pidash_services::app_issues::serializers_engage`; the member roster
//! renders through D-25's `app_project::ser_shared`
//! (`ProjectMemberLiteSerializer`); user / project / workspace lite nests
//! reuse `ser_shared` / `ser_member`. The app comment shape (33 keys with
//! `is_synced` and 12-key reaction nests) and the full issue-reaction
//! shape have no services port — the space `issue_graph` twins differ
//! (no `is_synced`, lite nests, space flats) — so they render here,
//! composed from the same lite ports. Like the sibling handler files this
//! module is self-contained (private `Denial`, private validators);
//! merges keep both sides.
//!
//! Enqueues (`issue_activity`, `model_activity` by Celery wire name, plus
//! the `soft_delete_related_objects` every instance `.delete()` fires)
//! publish best-effort through `pidash_jobs::queue`; without a queue the
//! response still stands (the cycles-handler precedent). Subscriber paths
//! enqueue nothing (no task imports in `subscriber.py`).
//!
//! No `Issue` row is written on any path here and comments carry no
//! post-save receiver (`signals.py:12`), so nothing fires the D-12
//! orchestration pair — the port calls no transition entry.
//!
//! Ported bugs (translate, don't redesign — also listed in the PR):
//!
//! * The unfiltered history branch sorts raw model instances with
//!   `instance["created_at"]` (`activity.py:81-84`) and raises `TypeError`
//!   into the generic 500 — but only when at least one row exists; an
//!   empty feed sorts to `[]` and answers 200. Both halves ported.
//! * Duplicate issue reactions surface the generic `IntegrityError` body
//!   (`{"error": "The payload is not valid"}`): unlike comment reactions,
//!   the view catches nothing (`reaction.py:46-62`).
//! * The subscriber list returns the active project-member roster through
//!   `ProjectMemberLiteSerializer`, not the subscription rows
//!   (`subscriber.py:52-57`).
//! * A guest commenting on a foreign issue is refused with 400, not 403
//!   (`comment.py:86-89`).
//! * `PUT` on a comment runs DRF's default `update` (no decorator gate,
//!   no enqueues, no `edited_at` logic, full-update defaults) while
//!   `PATCH` runs the custom `partial_update` (ADMIN + creator gate,
//!   both enqueues, conditional `edited_at`).
//! * The create response carries `updated_by` set (the description-link
//!   second save stamps the in-memory instance) while the stored row
//!   keeps `updated_by` NULL (`update_fields=["description_id"]`).
//! * `subscribe` passes no `workspace` kwarg; the save backfills it from
//!   the project (`subscriber.py:81-83`, `project.py:309-311`).
//! * The comment `PUT` response carries `is_member` (the `get_object`
//!   queryset is annotated) while create / PATCH responses omit it
//!   (plain `.get()` / fresh instance, DRF `SkipField`).
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Router;
use chrono_tz::Tz;
use serde_json::{Map, Value};
use sqlx::Row;

use crate::state::AppState;
use crate::v1_cycles_modules::body as shared_body;
use pidash_db::app_pages::strip::ml_strip_tags;

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------

/// `IssueActivityEndpoint` (`app/urls/issue.py:172-176`).
pub const HISTORY_PATH: &str =
    "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/history/";
/// `IssueCommentViewSet` collection (`app/urls/issue.py:179-183`).
pub const COMMENTS_PATH: &str =
    "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/comments/";
/// `IssueCommentViewSet` detail (`app/urls/issue.py:184-195`).
pub const COMMENT_PATH: &str =
    "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/comments/{pk}/";
/// `IssueSubscriberViewSet` collection (`app/urls/issue.py:198-202`).
pub const SUBSCRIBERS_PATH: &str =
    "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/issue-subscribers/";
/// `IssueSubscriberViewSet` destroy (`app/urls/issue.py:203-207`). The
/// kwarg is a *user* id, not the row pk.
pub const SUBSCRIBER_PATH: &str =
    "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/issue-subscribers/{subscriber_id}/";
/// `subscribe` / `unsubscribe` / `subscription_status`
/// (`app/urls/issue.py:208-212`).
pub const SUBSCRIBE_PATH: &str =
    "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/subscribe/";
/// `IssueReactionViewSet` collection (`app/urls/issue.py:215-219`).
pub const REACTIONS_PATH: &str =
    "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/reactions/";
/// `IssueReactionViewSet` destroy (`app/urls/issue.py:220-224`). The
/// kwarg is the reaction *string*, not a pk.
pub const REACTION_PATH: &str =
    "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/reactions/{reaction_code}/";
/// `CommentReactionViewSet` collection (`app/urls/issue.py:227-231`).
/// Note: no `issue_id` on these two routes.
pub const COMMENT_REACTIONS_PATH: &str =
    "/api/workspaces/{slug}/projects/{project_id}/comments/{comment_id}/reactions/";
/// `CommentReactionViewSet` destroy (`app/urls/issue.py:232-236`).
pub const COMMENT_REACTION_PATH: &str =
    "/api/workspaces/{slug}/projects/{project_id}/comments/{comment_id}/reactions/{reaction_code}/";

/// Register the ten engage routes. Owned methods serve from Rust; every
/// other method on these paths falls through to Django (its
/// 405-after-auth and metadata responses live there). `HEAD` rides
/// axum's `get` handling like Django's `GET`-backed `HEAD`.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route(HISTORY_PATH, owned(axum::routing::get(history), &["GET"]))
        .route(
            COMMENTS_PATH,
            owned(
                axum::routing::get(comment_list).post(comment_create),
                &["GET", "POST"],
            ),
        )
        .route(
            COMMENT_PATH,
            owned(
                axum::routing::get(comment_retrieve)
                    .put(comment_update)
                    .patch(comment_partial_update)
                    .delete(comment_destroy),
                &["GET", "PUT", "PATCH", "DELETE"],
            ),
        )
        .route(
            SUBSCRIBERS_PATH,
            owned(
                axum::routing::get(subscriber_list).post(subscriber_create),
                &["GET", "POST"],
            ),
        )
        .route(
            SUBSCRIBER_PATH,
            owned(axum::routing::delete(subscriber_destroy), &["DELETE"]),
        )
        .route(
            SUBSCRIBE_PATH,
            owned(
                axum::routing::get(subscription_status)
                    .post(subscribe)
                    .delete(unsubscribe),
                &["GET", "POST", "DELETE"],
            ),
        )
        .route(
            REACTIONS_PATH,
            owned(
                axum::routing::get(issue_reaction_list).post(issue_reaction_create),
                &["GET", "POST"],
            ),
        )
        .route(
            REACTION_PATH,
            owned(axum::routing::delete(issue_reaction_destroy), &["DELETE"]),
        )
        .route(
            COMMENT_REACTIONS_PATH,
            owned(
                axum::routing::get(comment_reaction_list).post(comment_reaction_create),
                &["GET", "POST"],
            ),
        )
        .route(
            COMMENT_REACTION_PATH,
            owned(axum::routing::delete(comment_reaction_destroy), &["DELETE"]),
        )
}

/// An engage path: the owned methods serve from Rust, everything else
/// falls through to Django (the social-handler cutover shape).
fn owned(
    methods: axum::routing::MethodRouter<AppState>,
    owned: &[&str],
) -> axum::routing::MethodRouter<AppState> {
    let mut router = methods;
    for other in ["GET", "POST", "PUT", "PATCH", "DELETE", "OPTIONS"] {
        if owned.contains(&other) {
            continue;
        }
        router = match other {
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

/// Handler failure with its exact status + body.
#[derive(Debug)]
enum Denial {
    /// 401, DRF `NotAuthenticated`.
    Unauthorized,
    /// 403, `@allow_permission` body.
    Forbidden,
    /// 403, DRF `PermissionDenied` (class-permission denials).
    ForbiddenDetail,
    /// 404, `ObjectDoesNotExist` branch (bare `.get()` misses).
    NotFound,
    /// 404, `get_object_or_404` over comments (retrieve / PUT misses).
    NotFoundDetail,
    /// 404, `{"detail": "Project not found"}` (project-kwarg rewrite miss).
    ProjectNotFound,
    /// 400, `{"detail": ...}` (JSON `ParseError`).
    BadDetail(String),
    /// 400, `{"error": ...}` (view-inline + `IntegrityError` mapping).
    BadError(String),
    /// 400, `{"message": ...}` (already-subscribed).
    BadMessage(String),
    /// 400, serializer-errors dict.
    BadJson(Value),
    /// 415, `UnsupportedMediaType` (content negotiation).
    UnsupportedMediaType(String),
    /// 413, `RequestBodySizeLimitMiddleware` past 5 MiB.
    RequestTooLarge,
    /// 500, generic branch.
    ServerError,
}

/// DRF's default permission-denied body.
const VIEWSET_FORBIDDEN_BODY: &str =
    r#"{"detail":"You do not have permission to perform this action."}"#;
/// DRF `get_object_or_404` over comments: the `Http404` message names
/// the model (`django/shortcuts.py`), rendered under lowercase
/// `detail` — verified against live Django.
const COMMENT_NOT_FOUND_BODY: &str = r#"{"detail":"No IssueComment matches the given query."}"#;

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
            Denial::ForbiddenDetail => (StatusCode::FORBIDDEN, VIEWSET_FORBIDDEN_BODY.to_owned()),
            Denial::NotFound => (StatusCode::NOT_FOUND, super::NOT_FOUND_BODY.to_owned()),
            Denial::NotFoundDetail => (StatusCode::NOT_FOUND, COMMENT_NOT_FOUND_BODY.to_owned()),
            Denial::ProjectNotFound => (
                StatusCode::NOT_FOUND,
                super::PROJECT_NOT_FOUND_BODY.to_owned(),
            ),
            Denial::BadDetail(message) => (
                StatusCode::BAD_REQUEST,
                format!("{{\"detail\":{}}}", json_string(message)),
            ),
            Denial::BadError(message) => (
                StatusCode::BAD_REQUEST,
                format!("{{\"error\":{}}}", json_string(message)),
            ),
            Denial::BadMessage(message) => (
                StatusCode::BAD_REQUEST,
                format!("{{\"message\":{}}}", json_string(message)),
            ),
            Denial::BadJson(body) => (
                StatusCode::BAD_REQUEST,
                serde_json::to_string(body).unwrap_or_else(|_| super::SERVER_ERROR_BODY.to_owned()),
            ),
            Denial::UnsupportedMediaType(message) => (
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                format!("{{\"detail\":{}}}", json_string(message)),
            ),
            Denial::RequestTooLarge => (
                StatusCode::PAYLOAD_TOO_LARGE,
                r#"{"error":"REQUEST_BODY_TOO_LARGE","detail":"The size of the request body exceeds the maximum allowed size."}"#.to_owned(),
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
            tracing::warn!("app_issues engage handler: internal error");
        }
        let (status, body) = self.status_and_body();
        Response::builder()
            .status(status)
            .header(header::CONTENT_TYPE, "application/json")
            .body(axum::body::Body::from(body))
            .expect("engage error response")
    }
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).expect("json string")
}

/// Render `body` (already exact JSON bytes) with an explicit status.
fn json_response(status: StatusCode, body: String) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body))
        .expect("engage json response")
}

/// DRF's 204: empty body with the JSON content type.
fn empty_response(status: StatusCode) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::empty())
        .expect("engage empty response")
}

fn pool_of(state: &AppState) -> Result<sqlx::PgPool, Denial> {
    state
        .pools()
        .map(|pools| pools.primary().clone())
        .ok_or(Denial::ServerError)
}

/// Map a write failure the way `handle_exception` does: integrity
/// violations (unique / FK / not-null / check) are the `IntegrityError`
/// 400, anything else the fallback 500.
fn integrity_denial(error: sqlx::Error) -> Denial {
    if let sqlx::Error::Database(db) = &error {
        if db.code().is_some_and(|code| code.starts_with("23")) {
            return Denial::BadError("The payload is not valid".to_owned());
        }
    }
    let _ = error;
    Denial::ServerError
}

/// True when the failure is an integrity violation (SQLSTATE class 23).
fn is_integrity_error(error: &sqlx::Error) -> bool {
    matches!(error, sqlx::Error::Database(db) if db.code().is_some_and(|code| code.starts_with("23")))
}

// ---------------------------------------------------------------------------
// Request plumbing: auth + project rewrite + permission gates
// ---------------------------------------------------------------------------

/// `request.user` from the Django session (`_auth_user_id`). No session,
/// no key, a non-UUID id, or a session pointing at no user row means
/// anonymous → 401. (Django PKs are UUIDs; a session id that is not a
/// UUID cannot be a user.)
async fn actor_user_id(
    pool: &sqlx::PgPool,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Result<uuid::Uuid, Denial> {
    let handle = extension.ok_or(Denial::Unauthorized)?.0;
    let mut session = handle.snapshot();
    let raw = session
        .get("_auth_user_id")
        .and_then(|value| value.as_str().to_owned())
        .ok_or(Denial::Unauthorized)?;
    let id = raw
        .parse::<uuid::Uuid>()
        .map_err(|_| Denial::Unauthorized)?;
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

/// Membership facts for one `(user, slug, project)` over the same rows
/// the decorator and the permission classes read: active project and
/// workspace memberships scoped by slug/project, soft-deleted rows
/// excluded (`SoftDeletionManager`).
struct Membership {
    project_role: Option<i16>,
    workspace_role: Option<i16>,
}

async fn fetch_membership(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    user_id: &uuid::Uuid,
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

/// `allow_permission([ADMIN], creator=True, model)` (`base.py:19-39`):
/// the workspace-member pre-check, then the creator bypass
/// (`model.objects.filter(id=pk, created_by=user)`), then the ADMIN role
/// check. Denies with the decorator `error` body.
async fn check_admin_creator(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    user_id: &uuid::Uuid,
    table: &str,
    pk: &uuid::Uuid,
) -> Result<(), Denial> {
    let membership = fetch_membership(pool, slug, project_id, user_id).await?;
    if membership.workspace_role.is_none() {
        return Err(Denial::Forbidden);
    }
    // The creator check reads the default manager (soft-deleted rows do
    // not count) over `table`, whose name is a static per-call-site
    // literal, never request input.
    let sql = format!(
        "SELECT 1 FROM {table} WHERE id = $1 AND created_by_id = $2 AND deleted_at IS NULL"
    );
    let created: Option<(i32,)> = sqlx::query_as(&sql)
        .bind(pk)
        .bind(user_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    if created.is_some() {
        return Ok(());
    }
    check_allow(&membership, &[20])
}

/// `ProjectEntityPermission` (`project.py:85-116`): safe methods need any
/// active project membership; unsafe methods need ADMIN or MEMBER.
/// Denies with DRF's default `detail` body.
fn check_entity(membership: &Membership, safe: bool) -> Result<(), Denial> {
    if safe {
        if membership.project_role.is_some() {
            return Ok(());
        }
    } else if matches!(membership.project_role, Some(20) | Some(15)) {
        return Ok(());
    }
    Err(Denial::ForbiddenDetail)
}

/// `ProjectLitePermission` (`project.py:133-143`): any active project
/// membership, every method. Denies with DRF's default `detail` body.
fn check_lite(membership: &Membership) -> Result<(), Denial> {
    if membership.project_role.is_some() {
        return Ok(());
    }
    Err(Denial::ForbiddenDetail)
}

/// View-body tenant facts: the actor's timezone. Project existence
/// is per-path (only the reads whose Python calls
/// `Project.objects.get` 404 on a missing row); a missing project
/// here is simply empty scope.
struct Tenant {
    timezone: Tz,
}

async fn tenant_context(pool: &sqlx::PgPool, user_id: &uuid::Uuid) -> Result<Tenant, Denial> {
    let timezone_name: Option<(String,)> =
        sqlx::query_as("SELECT user_timezone FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_optional(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    let timezone: Tz = timezone_name
        .map(|row| row.0)
        .ok_or(Denial::ServerError)?
        .parse()
        .map_err(|_| Denial::ServerError)?;
    Ok(Tenant { timezone })
}

// ---------------------------------------------------------------------------
// Fetch helpers
// ---------------------------------------------------------------------------

/// One bound `$N` parameter for the single-table `row_to_json` fetches.
#[derive(Debug, Clone, Copy)]
enum SqlParam<'a> {
    Text(&'a str),
    Uuid(uuid::Uuid),
}

/// Fetch zero or one row of `inner` as a JSON object. Single-table
/// selects only — the joined builder statements decode positionally
/// (their columns are unaliased, so `row_to_json` maps would collide).
async fn fetch_optional_object(
    pool: &sqlx::PgPool,
    inner: &str,
    params: &[SqlParam<'_>],
) -> Result<Option<Value>, Denial> {
    let sql = format!("SELECT row_to_json(__r)::text AS __row FROM ({inner}) AS __r");
    let mut query = sqlx::query(&sql);
    for param in params {
        query = match param {
            SqlParam::Text(text) => query.bind(*text),
            SqlParam::Uuid(id) => query.bind(*id),
        };
    }
    let row: Option<sqlx::postgres::PgRow> = query
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    match row {
        None => Ok(None),
        Some(row) => {
            let text: String = row.try_get("__row").map_err(|_| Denial::ServerError)?;
            match serde_json::from_str(&text).map_err(|_| Denial::ServerError)? {
                Value::Object(_) => Ok(Some(
                    serde_json::from_str(&text).map_err(|_| Denial::ServerError)?,
                )),
                _ => Err(Denial::ServerError),
            }
        }
    }
}

/// Fetch every row of `inner` as JSON objects (single-table only, as
/// above).
async fn fetch_all_objects(
    pool: &sqlx::PgPool,
    inner: &str,
    params: &[SqlParam<'_>],
) -> Result<Vec<Value>, Denial> {
    let sql = format!("SELECT row_to_json(__r)::text AS __row FROM ({inner}) AS __r");
    let mut query = sqlx::query(&sql);
    for param in params {
        query = match param {
            SqlParam::Text(text) => query.bind(*text),
            SqlParam::Uuid(id) => query.bind(*id),
        };
    }
    let rows: Vec<sqlx::postgres::PgRow> = query
        .fetch_all(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let text: String = row.try_get("__row").map_err(|_| Denial::ServerError)?;
        match serde_json::from_str(&text).map_err(|_| Denial::ServerError)? {
            Value::Object(map) => out.push(Value::Object(map)),
            _ => return Err(Denial::ServerError),
        }
    }
    Ok(out)
}

/// Fetch every row of a [`super::Binder`]-built statement as raw rows
/// for positional decoding.
async fn fetch_positional_rows(
    pool: &sqlx::PgPool,
    sql: &str,
    values: Vec<sea_query::Value>,
) -> Result<Vec<sqlx::postgres::PgRow>, Denial> {
    let query = super::bind_all(sql, values).map_err(|_| Denial::ServerError)?;
    query.fetch_all(pool).await.map_err(|_| Denial::ServerError)
}

fn obj(value: &Value) -> Result<&Map<String, Value>, Denial> {
    value.as_object().ok_or(Denial::ServerError)
}

fn req_str(map: &Map<String, Value>, key: &str) -> Result<String, Denial> {
    match map.get(key) {
        Some(Value::String(s)) => Ok(s.clone()),
        _ => Err(Denial::ServerError),
    }
}

fn opt_str(map: &Map<String, Value>, key: &str) -> Result<Option<String>, Denial> {
    match map.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        _ => Err(Denial::ServerError),
    }
}

fn req_bool(map: &Map<String, Value>, key: &str) -> Result<bool, Denial> {
    match map.get(key) {
        Some(Value::Bool(b)) => Ok(*b),
        _ => Err(Denial::ServerError),
    }
}

// ---- positional cell readers (index into the builder select lists) ----

fn cell_string(row: &sqlx::postgres::PgRow, idx: usize) -> Result<String, Denial> {
    row.try_get::<String, _>(idx)
        .map_err(|_| Denial::ServerError)
}

fn cell_opt_string(row: &sqlx::postgres::PgRow, idx: usize) -> Result<Option<String>, Denial> {
    row.try_get::<Option<String>, _>(idx)
        .map_err(|_| Denial::ServerError)
}

fn cell_uuid(row: &sqlx::postgres::PgRow, idx: usize) -> Result<uuid::Uuid, Denial> {
    row.try_get::<uuid::Uuid, _>(idx)
        .map_err(|_| Denial::ServerError)
}

fn cell_opt_uuid(row: &sqlx::postgres::PgRow, idx: usize) -> Result<Option<uuid::Uuid>, Denial> {
    row.try_get::<Option<uuid::Uuid>, _>(idx)
        .map_err(|_| Denial::ServerError)
}

fn cell_bool(row: &sqlx::postgres::PgRow, idx: usize) -> Result<bool, Denial> {
    row.try_get::<bool, _>(idx).map_err(|_| Denial::ServerError)
}

fn cell_i32(row: &sqlx::postgres::PgRow, idx: usize) -> Result<i32, Denial> {
    row.try_get::<i32, _>(idx).map_err(|_| Denial::ServerError)
}

fn cell_f64(row: &sqlx::postgres::PgRow, idx: usize) -> Result<f64, Denial> {
    row.try_get::<f64, _>(idx).map_err(|_| Denial::ServerError)
}

fn cell_opt_f64(row: &sqlx::postgres::PgRow, idx: usize) -> Result<Option<f64>, Denial> {
    row.try_get::<Option<f64>, _>(idx)
        .map_err(|_| Denial::ServerError)
}

fn cell_json(row: &sqlx::postgres::PgRow, idx: usize) -> Result<Value, Denial> {
    row.try_get::<Value, _>(idx)
        .map_err(|_| Denial::ServerError)
}

fn cell_str_array(row: &sqlx::postgres::PgRow, idx: usize) -> Result<Vec<String>, Denial> {
    row.try_get::<Vec<String>, _>(idx)
        .map_err(|_| Denial::ServerError)
}

fn cell_datetime(
    row: &sqlx::postgres::PgRow,
    idx: usize,
) -> Result<chrono::DateTime<chrono::Utc>, Denial> {
    row.try_get::<chrono::DateTime<chrono::Utc>, _>(idx)
        .map_err(|_| Denial::ServerError)
}

fn cell_opt_datetime(
    row: &sqlx::postgres::PgRow,
    idx: usize,
) -> Result<Option<chrono::DateTime<chrono::Utc>>, Denial> {
    row.try_get::<Option<chrono::DateTime<chrono::Utc>>, _>(idx)
        .map_err(|_| Denial::ServerError)
}

fn cell_opt_date(
    row: &sqlx::postgres::PgRow,
    idx: usize,
) -> Result<Option<chrono::NaiveDate>, Denial> {
    row.try_get::<Option<chrono::NaiveDate>, _>(idx)
        .map_err(|_| Denial::ServerError)
}

// ---------------------------------------------------------------------------
// Datetime rendering
// ---------------------------------------------------------------------------

/// Render a timestamptz exactly like DRF `DateTimeField` with the default
/// `iso-8601` format: ISO-8601 in the request's zone with `+00:00`
/// rewritten to `Z` (serializer kernel rule).
fn render_dt(dt: &chrono::DateTime<chrono::Utc>, tz: &Tz) -> String {
    crate::serializer::render_datetime_in(&dt.fixed_offset(), tz)
}

fn render_dt_opt(dt: Option<chrono::DateTime<chrono::Utc>>, tz: &Tz) -> Option<String> {
    dt.map(|dt| render_dt(&dt, tz))
}

/// Render a `row_to_json` timestamptz string (RFC 3339) the same way.
fn render_dt_str(raw: &str, tz: &Tz) -> Result<String, Denial> {
    let dt = chrono::DateTime::parse_from_rfc3339(raw).map_err(|_| Denial::ServerError)?;
    Ok(crate::serializer::render_datetime_in(&dt, tz))
}

fn render_dt_str_opt(raw: Option<String>, tz: &Tz) -> Result<Option<String>, Denial> {
    raw.map(|text| render_dt_str(&text, tz)).transpose()
}

/// DRF `DateField` wire format.
fn render_date(date: &chrono::NaiveDate) -> String {
    date.format("%Y-%m-%d").to_string()
}

// ---------------------------------------------------------------------------
// Request bodies (DRF JSON parsing)
// ---------------------------------------------------------------------------

/// Django's `DATA_UPLOAD_MAX_MEMORY_SIZE` (5 MiB): past it the
/// `RequestBodySizeLimitMiddleware` answers 413 before the view runs.
const MAX_BODY: usize = 5_242_880;

/// Engage form semantics for the shared body negotiator: the two
/// `ArrayField`s arrive as arrays; `access`/`speaker_type` (not
/// required, `ChoiceField` has no `allow_blank`) skip blank form
/// input; every other blank rule is validated from the map.
const ENGAGE_BODY_SPEC: shared_body::BodySpec = shared_body::BodySpec {
    list_fields: &["attachments", "labels"],
    skip_blank_fields: &["access", "speaker_type"],
};

/// Read the request body the way DRF does: content negotiation (JSON /
/// form / multipart, 415 otherwise), empty → `{}`, malformed JSON →
/// `ParseError` 400, non-object JSON → the `non_field_errors` 400.
/// Past 5 MiB → the middleware 413.
///
/// Form notes: `comment_json` form strings are JSON-*parsed* (DRF
/// `JSONString` — `""` → "Value must be valid JSON."); blank form
/// input for the nullable scalars (`deleted_at`, `edited_at`,
/// `speaker_agent_run_id`) coerces to `None` (`Field.get_value`);
/// uploads surface as `{"__file__": filename}` sentinels (DRF reads
/// the file object: `CharField` invalid, choice/FK echo the filename,
/// `comment_json` invalid).
/// A negotiated request body: the validator map plus whether it came
/// from HTML input (form/multipart). Validators branch on `is_html`
/// exactly where DRF does (`get_value`, `JSONString`, file objects) —
/// a JSON `{"__file__": …}` dict is ordinary data, never a file.
struct RequestBody {
    map: Map<String, Value>,
    is_html: bool,
}

async fn read_body(req: axum::extract::Request) -> Result<RequestBody, Denial> {
    let (parts, body) = req.into_parts();
    let bytes = axum::body::to_bytes(body, MAX_BODY)
        .await
        .map_err(|_| Denial::RequestTooLarge)?;
    match shared_body::negotiate_body(&parts.headers, &bytes, &ENGAGE_BODY_SPEC) {
        Ok(shared_body::NegotiatedBody::Empty) => Ok(RequestBody {
            map: Map::new(),
            is_html: false,
        }),
        Ok(shared_body::NegotiatedBody::JsonText(text)) => {
            parse_json_body(&text).map(|map| RequestBody {
                map,
                is_html: false,
            })
        }
        Ok(shared_body::NegotiatedBody::Form { map, files }) => {
            form_body_map(map, files).map(|map| RequestBody { map, is_html: true })
        }
        Err(shared_body::BodyError::UnsupportedMediaType(message)) => {
            Err(Denial::UnsupportedMediaType(message))
        }
        Err(shared_body::BodyError::ParseDetail(message)) => Err(Denial::BadDetail(message)),
        Err(shared_body::BodyError::ServerError) => Err(Denial::ServerError),
    }
}

/// Parse negotiated JSON source text: objects → the map (floats
/// rewritten to their CPython literal — see `normalize_parse_floats`);
/// `null` → "No data provided"; other shapes → the
/// `non_field_errors` "Expected a dictionary" 400; malformed → the
/// engine-native `ParseError` text (the accepted cross-engine gap —
/// see the D-20 `body.rs` precedent).
fn parse_json_body(text: &str) -> Result<Map<String, Value>, Denial> {
    match serde_json::from_str::<Value>(text) {
        Ok(Value::Object(map)) => {
            let mut normalized: Value = Value::Object(map);
            normalize_parse_floats(&mut normalized);
            match normalized {
                Value::Object(map) => Ok(map),
                _ => unreachable!("object stays an object"),
            }
        }
        Ok(Value::Null) => Err(Denial::BadJson(serde_json::json!({
            "non_field_errors": ["No data provided"],
        }))),
        Ok(other) => Err(Denial::BadJson(serde_json::json!({
            "non_field_errors": [format!(
                "Invalid data. Expected a dictionary, but got {}.",
                json_type_name(&other),
            )],
        }))),
        Err(error) => Err(Denial::BadDetail(format!("JSON parse error - {error}"))),
    }
}

/// A `{"__file__": filename}` upload sentinel (see `read_body`).
fn file_sentinel(filename: &str) -> Value {
    Value::Object(Map::from_iter([(
        "__file__".to_owned(),
        Value::String(filename.to_owned()),
    )]))
}

/// Merge a negotiated form body into the validator map: files merge
/// over texts last-wins (DRF `_full_data`), list fields append file
/// sentinels after texts, blank nullable scalars coerce to `None`,
/// and `comment_json` strings JSON-parse (bare `NaN`/`Infinity` →
/// the 500 Django's `json.dumps` round-trip would die with — the
/// form parser accepts them, the `JSONField` save does not).
fn form_body_map(
    map: Map<String, Value>,
    files: shared_body::FilesMap,
) -> Result<Map<String, Value>, Denial> {
    let mut out = map;
    for (key, parts) in &files {
        if ENGAGE_BODY_SPEC.list_fields.contains(&key.as_str()) {
            // Indexed assemblies (`labels[N]`, `labels[N]suffix`) hold
            // Null placeholders / dict-form arrays for their uploads
            // (form parsing otherwise never emits nulls — the shapes
            // are unambiguous): resolve them positionally from the
            // front of the upload queue, then append leftover
            // exact-key uploads after the texts (DRF `_full_data`
            // extends texts with files).
            let mut consumed = 0usize;
            if let Some(Value::Array(items)) = out.get_mut(key) {
                for item in items.iter_mut() {
                    match item {
                        Value::Null => {
                            if let Some(part) = parts.get(consumed) {
                                *item = file_sentinel(&part.filename);
                                consumed += 1;
                            }
                        }
                        Value::Array(rendered) => {
                            // Dict-form `{suffix: [value]}` pairs: pop
                            // one upload per null value to keep the
                            // queue aligned (the child rejects the
                            // dict either way).
                            for pair in rendered.iter() {
                                if let Value::Object(pair) = pair {
                                    for slot in pair.values() {
                                        if let Value::Array(slot) = slot {
                                            for value in slot.iter() {
                                                if value.is_null() && consumed < parts.len() {
                                                    consumed += 1;
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
                items.extend(
                    parts[consumed..]
                        .iter()
                        .map(|part| file_sentinel(&part.filename)),
                );
            } else {
                out.insert(
                    key.clone(),
                    Value::Array(
                        parts
                            .iter()
                            .map(|part| file_sentinel(&part.filename))
                            .collect(),
                    ),
                );
            }
        } else if let Some(last) = parts.last() {
            out.insert(key.clone(), file_sentinel(&last.filename));
        }
    }
    for field in ["deleted_at", "edited_at", "speaker_agent_run_id"] {
        if out.get(field) == Some(&Value::String(String::new())) {
            out.insert(field.to_owned(), Value::Null);
        }
    }
    if let Some(value) = out.get("comment_json").cloned() {
        // Multi-valued form input reads the LAST value
        // (`QueryDict.__getitem__`); dict-shaped input (indexed keys)
        // is not a JSON string → invalid. Uploads stringify to their
        // filename first (`JSONString(str(file))`) and then parse —
        // a file named `123` yields the number 123.
        let text = match &value {
            Value::String(text) => Some(text.clone()),
            Value::Array(items) => items.last().and_then(|last| match last {
                Value::String(text) => Some(text.clone()),
                other => uploaded_file_name(other),
            }),
            other => uploaded_file_name(other),
        };
        let parsed = match text {
            Some(text) => match serde_json::from_str::<Value>(&text) {
                Ok(parsed) => Some(parsed),
                Err(_) => {
                    if json_has_bare_nonfinite(&text) {
                        // Django's `json.loads` accepts the token, then
                        // the `JSONField` save dies in `get_prep_value`:
                        // the 500, not a field error.
                        return Err(Denial::ServerError);
                    }
                    return Err(Denial::BadJson(serde_json::json!({
                        "comment_json": ["Value must be valid JSON."],
                    })));
                }
            },
            None => {
                return Err(Denial::BadJson(serde_json::json!({
                    "comment_json": ["Value must be valid JSON."],
                })));
            }
        };
        if let Some(parsed) = parsed {
            out.insert("comment_json".to_owned(), parsed);
        }
    }
    let mut normalized = Value::Object(out);
    normalize_parse_floats(&mut normalized);
    match normalized {
        Value::Object(map) => Ok(map),
        _ => unreachable!("object stays an object"),
    }
}

/// Scan JSON source text (string-aware) for bare `NaN`/`Infinity`
/// tokens outside strings.
fn json_has_bare_nonfinite(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut index = 0;
    let mut in_string = false;
    let mut escaped = false;
    let mut token = String::new();
    let flush = |token: &mut String| {
        let hit = matches!(
            token.as_str(),
            "NaN" | "Infinity" | "-Infinity" | "+Infinity"
        );
        token.clear();
        hit
    };
    while index < bytes.len() {
        let byte = bytes[index];
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            index += 1;
            continue;
        }
        if byte == b'"' {
            if flush(&mut token) {
                return true;
            }
            in_string = true;
            index += 1;
            continue;
        }
        if byte.is_ascii_alphanumeric() || byte == b'.' || byte == b'+' || byte == b'-' {
            token.push(byte as char);
        } else if flush(&mut token) {
            return true;
        }
        index += 1;
    }
    flush(&mut token)
}

/// Back-compat shim for the unit tests: parse JSON source bytes the
/// way `JsonText` does (the header-free tests pin the JSON shapes;
/// empty bodies never reach `JsonText` — `Empty` short-circuits).
#[cfg(test)]
fn parse_body(raw: &[u8]) -> Result<Map<String, Value>, Denial> {
    if raw.is_empty() {
        return Ok(Map::new());
    }
    match std::str::from_utf8(raw) {
        Ok(text) => parse_json_body(text),
        Err(_) => Err(Denial::BadDetail(
            "JSON parse error - invalid utf-8".to_owned(),
        )),
    }
}

/// CPython `type(data).__name__` for a parsed JSON value.
fn json_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "NoneType",
        Value::Bool(_) => "bool",
        Value::Number(n) => {
            if n.is_i64() || n.is_u64() {
                "int"
            } else {
                "float"
            }
        }
        Value::String(_) => "str",
        Value::Array(_) => "list",
        Value::Object(_) => "dict",
    }
}

// ---------------------------------------------------------------------------
// `json.dumps` with CPython defaults (Celery `requested_data` payloads)
// ---------------------------------------------------------------------------

/// `json.dumps(value)`: `separators=(', ', ': ')`, `ensure_ascii=True`,
/// insertion-ordered keys. Same implementation as the social handlers'
/// `python_dumps` (convergent copy; that module's is private).
fn python_dumps(value: &serde_json::Value) -> String {
    let mut out = String::new();
    python_dump_into(&mut out, value);
    out
}

fn python_dump_into(out: &mut String, value: &serde_json::Value) {
    match value {
        serde_json::Value::Null => out.push_str("null"),
        serde_json::Value::Bool(true) => out.push_str("true"),
        serde_json::Value::Bool(false) => out.push_str("false"),
        serde_json::Value::Number(number) => out.push_str(&number.to_string()),
        serde_json::Value::String(text) => python_dump_str(out, text),
        serde_json::Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                python_dump_into(out, item);
            }
            out.push(']');
        }
        serde_json::Value::Object(map) => {
            out.push('{');
            for (index, (key, item)) in map.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                python_dump_str(out, key);
                out.push_str(": ");
                python_dump_into(out, item);
            }
            out.push('}');
        }
    }
}

/// CPython `py_encode_basestring_ascii`.
fn python_dump_str(out: &mut String, text: &str) {
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0C}' => out.push_str("\\f"),
            ch if (ch as u32) < 0x20 || (ch as u32) == 0x7F => {
                out.push_str(&format!("\\u{:04x}", ch as u32));
            }
            ch if (ch as u32) > 0x7E => {
                let code = ch as u32;
                if code > 0xFFFF {
                    let v = code - 0x10000;
                    out.push_str(&format!(
                        "\\u{:04x}\\u{:04x}",
                        0xD800 + (v >> 10),
                        0xDC00 + (v & 0x3FF)
                    ));
                } else {
                    out.push_str(&format!("\\u{code:04x}"));
                }
            }
            ch => out.push(ch),
        }
    }
    out.push('"');
}

// Comment `save()` computes `comment_stripped` through
// `pi_dash.utils.html_processor.strip_tags` (`MLStripper`,
// `db/models/issue.py:606`; `""` stays `""`) — the shared
// `ml_strip_tags` port, not Django's `strip_tags`.

// ---------------------------------------------------------------------------
// CPython value semantics (`str` / `repr` / `==` / float rendering)
// ---------------------------------------------------------------------------

/// CPython `repr(float)`: the shortest spelling that round-trips,
/// `.0` for integral plain values, `e±XX` exponents with a sign and
/// at least two digits, exponent form outside `[1e-4, 1e16)`.
/// Verified live: `77.0` → `"77.0"`, `1e16` → `"1e+16"`,
/// `1e-5` → `"1e-05"` — plus a 116-value CPython oracle unit test.
///
/// Render precisions 1..=19 and take the first that parses back to
/// `value` (greedy right-trimming from a long rendering is unsound:
/// intermediate prefixes can round to a neighbor while a shorter one
/// hits — e.g. `1.5e-07`), then lay out per the CPython magnitude
/// rule.
fn py_float_repr(value: f64) -> String {
    if value.is_nan() {
        return "nan".to_owned();
    }
    if value.is_infinite() {
        return if value > 0.0 {
            "inf".to_owned()
        } else {
            "-inf".to_owned()
        };
    }
    if value == 0.0 {
        return if value.is_sign_negative() {
            "-0.0".to_owned()
        } else {
            "0.0".to_owned()
        };
    }
    for precision in 1..=19usize {
        let rendered = format!("{value:.dec$e}", dec = precision - 1);
        if rendered.parse::<f64>().ok() == Some(value) {
            return layout_py_float(&rendered, value);
        }
    }
    // Unreachable in practice (17 significant digits always identify
    // an f64); fall back to the longest rendering laid out.
    layout_py_float(&format!("{value:.18e}"), value)
}

/// Lay out a `d.dddde±xx` rendering per the CPython magnitude rule
/// (plain inside `[1e-4, 1e16)`, exponent outside).
fn layout_py_float(rendered: &str, value: f64) -> String {
    let (mantissa, exp) = match rendered.split_once(['e', 'E']) {
        Some((mantissa, exp)) => (mantissa, exp.parse::<i32>().unwrap_or(0)),
        None => (rendered, 0),
    };
    let negative = mantissa.starts_with('-');
    let digits: String = mantissa.chars().filter(|ch| ch.is_ascii_digit()).collect();
    let digits = digits.trim_start_matches('0');
    let digits = digits.trim_end_matches('0');
    let digits = if digits.is_empty() { "0" } else { digits };
    // Decimal exponent of `0.digits × 10^E`.
    let point_at = mantissa
        .find('.')
        .map(|pos| pos - usize::from(negative))
        .unwrap_or_else(|| mantissa.len() - usize::from(negative));
    let exp10 = exp + point_at as i32;
    let abs = value.abs();
    let mut out = String::new();
    if negative {
        out.push('-');
    }
    if (1e-4..1e16).contains(&abs) {
        // Plain layout.
        if exp10 <= 0 {
            out.push_str("0.");
            out.push_str(&"0".repeat((-exp10) as usize));
            out.push_str(digits);
        } else if exp10 as usize >= digits.len() {
            out.push_str(digits);
            out.push_str(&"0".repeat(exp10 as usize - digits.len()));
            out.push_str(".0");
        } else {
            let at = exp10 as usize;
            out.push_str(&digits[..at]);
            out.push('.');
            out.push_str(&digits[at..]);
        }
        return out;
    }
    // Exponent layout: `d[.ddd]e±XX`.
    let head: String = digits.chars().take(1).collect();
    let tail: String = digits.chars().skip(1).collect();
    out.push_str(&head);
    if !tail.is_empty() {
        out.push('.');
        out.push_str(&tail);
    }
    let exp = exp10 - 1;
    out.push('e');
    out.push(if exp < 0 { '-' } else { '+' });
    let mag = exp.abs().to_string();
    if mag.len() < 2 {
        out.push('0');
    }
    out.push_str(&mag);
    out
}

/// CPython `str()` over a parsed JSON value: `None` / `True` / `False`,
/// decimal ints, [`py_float_repr`] floats, raw strings, and — for
/// containers — brackets with `repr()` elements (verified live:
/// `"['INTERNAL']"`, `"{'a': 1}"`).
fn py_str_value(value: &Value) -> String {
    match value {
        Value::Null => "None".to_owned(),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                int.to_string()
            } else if let Some(uint) = number.as_u64() {
                uint.to_string()
            } else if number.is_f64() {
                match number.as_f64() {
                    Some(float) => py_float_repr(float),
                    None => number.to_string(),
                }
            } else {
                // Arbitrary-precision integers past `u64`: full digits,
                // like CPython `str(int)` (never the lossy `as_f64`).
                number.to_string()
            }
        }
        Value::String(text) => text.clone(),
        Value::Array(items) => {
            let inner: Vec<String> = items.iter().map(py_repr_value).collect();
            format!("[{}]", inner.join(", "))
        }
        Value::Object(map) => {
            let inner: Vec<String> = map
                .iter()
                .map(|(key, item)| format!("{}: {}", py_repr_string(key), py_repr_value(item)))
                .collect();
            format!("{{{}}}", inner.join(", "))
        }
    }
}

/// CPython `repr()` over a parsed JSON value (container elements and dict
/// keys use this; see [`py_str_value`]).
fn py_repr_value(value: &Value) -> String {
    match value {
        Value::String(text) => py_repr_string(text),
        Value::Array(items) => {
            let inner: Vec<String> = items.iter().map(py_repr_value).collect();
            format!("[{}]", inner.join(", "))
        }
        Value::Object(map) => {
            let inner: Vec<String> = map
                .iter()
                .map(|(key, item)| format!("{}: {}", py_repr_string(key), py_repr_value(item)))
                .collect();
            format!("{{{}}}", inner.join(", "))
        }
        other => py_str_value(other),
    }
}

/// CPython `repr()` of a string: single quotes unless the text holds a
/// single quote but no double quote (then double quotes), with the
/// short escapes plus `\xhh` / `\uhhhh` / `\Uhhhhhhhh` for
/// non-printables (controls, DEL, C1, non-space whitespace; format and
/// unassigned codepoints stay literal — an accepted approximation, only
/// reachable inside choice/UUID error text).
fn py_repr_string(text: &str) -> String {
    let use_double = text.contains('\'') && !text.contains('"');
    let quote = if use_double { '"' } else { '\'' };
    let mut out = String::with_capacity(text.len() + 2);
    out.push(quote);
    for ch in text.chars() {
        if ch == quote {
            out.push('\\');
            out.push(ch);
            continue;
        }
        match ch {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch.is_control()
                || (ch as u32) == 0x7f
                || ((ch as u32) >= 0x80 && (ch as u32) < 0xa0)
                || (ch.is_whitespace() && ch != ' ') =>
            {
                let code = ch as u32;
                if code < 0x100 {
                    out.push_str(&format!("\\x{code:02x}"));
                } else if code < 0x10000 {
                    out.push_str(&format!("\\u{code:04x}"));
                } else {
                    out.push_str(&format!("\\U{code:08x}"));
                }
            }
            ch => out.push(ch),
        }
    }
    out.push(quote);
    out
}

/// CPython `==` over parsed JSON values (the sync-lock and
/// `_changes_on_save` comparisons): `True == 1`, `1 == 1.0`, objects
/// order-insensitive, arrays ordered, `1 == "1"` false.
fn py_json_eq(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Null, Value::Null) => true,
        (Value::Bool(a), Value::Bool(b)) => a == b,
        (Value::Bool(a), Value::Number(b)) | (Value::Number(b), Value::Bool(a)) => {
            let num = if *a { 1.0 } else { 0.0 };
            json_number_to_f64(b) == Some(num)
        }
        (Value::Number(a), Value::Number(b)) => {
            if !a.is_f64() && !b.is_f64() {
                // Integers (JSON spellings are canonical — no `+`, no
                // leading zeros): exact, like CPython `int == int`.
                a.to_string() == b.to_string()
            } else {
                json_number_to_f64(a) == json_number_to_f64(b)
            }
        }
        (Value::String(a), Value::String(b)) => a == b,
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| py_json_eq(x, y))
        }
        (Value::Object(a), Value::Object(b)) => {
            a.len() == b.len()
                && a.iter().all(|(key, value)| match b.get(key) {
                    Some(other) => py_json_eq(value, other),
                    None => false,
                })
        }
        _ => false,
    }
}

/// A JSON number as `f64` for [`py_json_eq`] cross-type comparison.
fn json_number_to_f64(number: &serde_json::Number) -> Option<f64> {
    number
        .as_i64()
        .map(|int| int as f64)
        .or_else(|| number.as_u64().map(|uint| uint as f64))
        .or_else(|| number.as_f64())
}

/// Rewrite every float in a parsed body to its [`py_float_repr`]
/// literal (kept as a `Number`, so ints stay ints and dumps/responses
/// render the CPython spelling: `1e16` in, `1e+16` out).
fn normalize_parse_floats(value: &mut Value) {
    match value {
        Value::Number(number) => {
            if number.is_f64() {
                if let Some(float) = number.as_f64() {
                    let literal = py_float_repr(float);
                    if let Ok(Value::Number(replacement)) = serde_json::from_str::<Value>(&literal)
                    {
                        *number = replacement;
                    }
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                normalize_parse_floats(item);
            }
        }
        Value::Object(map) => {
            for item in map.values_mut() {
                normalize_parse_floats(item);
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Tasks: best-effort publishing through the queue
// ---------------------------------------------------------------------------

/// `issue_activity` Celery wire name (bare `@shared_task` default:
/// `bgtasks/issue_activities_task.py:1503-1504`).
pub const ISSUE_ACTIVITY_TASK: &str = "pi_dash.bgtasks.issue_activities_task.issue_activity";
/// `model_activity` Celery wire name (`bgtasks/webhook_task.py:463-464`).
pub const MODEL_ACTIVITY_TASK: &str = "pi_dash.bgtasks.webhook_task.model_activity";

/// Enqueue a worker message for the worker to forward to the broker
/// (Python-owned task). Best-effort: without a queue the response still
/// stands (the intake-handler precedent).
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

/// Best-effort kwargs-only publish (every `.delay(...)` call below).
async fn enqueue_kwargs(pool: &sqlx::PgPool, task: &str, kwargs: Map<String, Value>) {
    let message = pidash_jobs::celery::CeleryTaskMessage::new(task, vec![], kwargs);
    enqueue_message(pool, message).await;
}

/// `soft_delete_related_objects.delay("db", <model>, pk, using=None)`
/// (`db/mixins.py:77`): three positional args, `using` stays a null
/// kwarg. Fires on every instance `.delete()` in these views.
async fn enqueue_soft_delete(pool: &sqlx::PgPool, model: &str, pk: &uuid::Uuid) {
    let mut kwargs = Map::new();
    kwargs.insert("using".to_owned(), Value::Null);
    let message = pidash_jobs::celery::CeleryTaskMessage::new(
        pidash_jobs::tasks_cleanup::deletion::SOFT_DELETE_TASK,
        vec![
            Value::String("db".to_owned()),
            Value::String(model.to_owned()),
            Value::String(pk.to_string()),
        ],
        kwargs,
    );
    enqueue_message(pool, message).await;
}

/// `base_host(request, is_app=True)` (`utils/host.py:17-67`): the app
/// origin for task kwargs, never the inbound host.
fn app_origin(state: &AppState) -> String {
    if let Some(url) = state.settings().urls.app_base_url.clone() {
        return url;
    }
    if let Some(url) = state.settings().urls.web_url.clone() {
        return url;
    }
    "http://localhost".to_owned()
}

// ---------------------------------------------------------------------------
// Asset URLs + nested lite shapes
// ---------------------------------------------------------------------------

/// `SELECT` for the logo/cover/avatar asset row behind `*_url`
/// properties.
fn logo_asset_sql() -> String {
    "SELECT \"a\".\"id\", \"a\".\"entity_type\", \"a\".\"workspace_id\", \"a\".\"project_id\", \"a\".\"issue_id\", \"w\".\"slug\" AS \"workspace_slug\" FROM \"file_assets\" AS \"a\" LEFT OUTER JOIN \"workspaces\" AS \"w\" ON (\"a\".\"workspace_id\" = \"w\".\"id\") WHERE (\"a\".\"deleted_at\" IS NULL AND \"a\".\"id\" = $1)".to_owned()
}

/// Port of `FileAsset.asset_url` (`db/models/asset.py:80-99`):
/// static-asset branches render `/api/assets/v2/static/<id>/`;
/// attachment/description branches interpolate ids; anything else
/// renders `None`.
fn asset_url(
    asset_id: &str,
    entity_type: Option<&str>,
    workspace_slug: Option<&str>,
    project_id: Option<&str>,
    issue_id: Option<&str>,
) -> Option<String> {
    match entity_type {
        Some("WORKSPACE_LOGO" | "USER_AVATAR" | "USER_COVER" | "PROJECT_COVER") => {
            Some(format!("/api/assets/v2/static/{asset_id}/"))
        }
        Some("ISSUE_ATTACHMENT") => {
            let (slug, project, issue) = match (workspace_slug, project_id, issue_id) {
                (Some(s), Some(p), Some(i)) => (s, p, i),
                _ => return None,
            };
            Some(format!(
                "/api/assets/v2/workspaces/{slug}/projects/{project}/issues/{issue}/attachments/{asset_id}/"
            ))
        }
        Some(
            "ISSUE_DESCRIPTION"
            | "COMMENT_DESCRIPTION"
            | "PAGE_DESCRIPTION"
            | "DRAFT_ISSUE_DESCRIPTION",
        ) => {
            let (slug, project) = match (workspace_slug, project_id) {
                (Some(s), Some(p)) => (s, p),
                _ => return None,
            };
            Some(format!(
                "/api/assets/v2/workspaces/{slug}/projects/{project}/{asset_id}/"
            ))
        }
        _ => None,
    }
}

/// Resolve a logo/cover/avatar URL the way the model properties do: the
/// asset's `asset_url` when the FK is set (even when that computes to
/// `None` — no fall-through), else the raw string column when
/// non-empty, else `None`.
async fn logo_or_cover_url(
    pool: &sqlx::PgPool,
    raw: Option<String>,
    asset_id: Option<uuid::Uuid>,
) -> Result<Option<String>, Denial> {
    let Some(asset_id) = asset_id else {
        return Ok(raw.filter(|text| !text.is_empty()));
    };
    let row = fetch_optional_object(pool, &logo_asset_sql(), &[SqlParam::Uuid(asset_id)])
        .await?
        .ok_or(Denial::ServerError)?;
    let o = obj(&row)?;
    let entity_type = opt_str(o, "entity_type")?;
    let workspace_slug = opt_str(o, "workspace_slug")?;
    let project_id = opt_str(o, "project_id")?;
    let issue_id = opt_str(o, "issue_id")?;
    let id = req_str(o, "id")?;
    Ok(asset_url(
        &id,
        entity_type.as_deref(),
        workspace_slug.as_deref(),
        project_id.as_deref(),
        issue_id.as_deref(),
    ))
}

/// Owned app `UserLiteSerializer` row (`user.py:141-153`).
struct UserLiteOwned {
    id: String,
    first_name: String,
    last_name: String,
    avatar: String,
    avatar_url: Option<String>,
    is_bot: bool,
    display_name: String,
}

fn user_lite_value(row: &UserLiteOwned) -> Result<Value, Denial> {
    use pidash_services::app_project::ser_shared::{user_lite_to_representation, UserLiteRow};
    let input = UserLiteRow {
        id: &row.id,
        first_name: &row.first_name,
        last_name: &row.last_name,
        avatar: &row.avatar,
        avatar_url: row.avatar_url.as_deref(),
        is_bot: row.is_bot,
        display_name: &row.display_name,
    };
    let view = user_lite_to_representation(&input);
    serde_json::to_value(&view).map_err(|_| Denial::ServerError)
}

/// `actor_detail` leaf over a user id. The `users` table has no
/// `deleted_at`, so the read is unguarded; a dangling FK is a 500 like
/// Python's `DoesNotExist` into the fallback envelope.
async fn fetch_user_lite(
    pool: &sqlx::PgPool,
    user_id: &uuid::Uuid,
) -> Result<UserLiteOwned, Denial> {
    let row = fetch_optional_object(
        pool,
        "SELECT \"users\".\"id\", \"users\".\"first_name\", \"users\".\"last_name\", \"users\".\"avatar\", \"users\".\"avatar_asset_id\", \"users\".\"is_bot\", \"users\".\"display_name\" FROM \"users\" WHERE (\"users\".\"id\" = $1)",
        &[SqlParam::Uuid(*user_id)],
    )
    .await?
    .ok_or(Denial::ServerError)?;
    let o = obj(&row)?;
    let asset_id = opt_str(o, "avatar_asset_id")?
        .map(|raw| raw.parse::<uuid::Uuid>())
        .transpose()
        .map_err(|_| Denial::ServerError)?;
    let avatar_url = logo_or_cover_url(pool, opt_str(o, "avatar")?, asset_id).await?;
    Ok(UserLiteOwned {
        id: req_str(o, "id")?,
        first_name: req_str(o, "first_name")?,
        last_name: req_str(o, "last_name")?,
        avatar: req_str(o, "avatar")?,
        avatar_url,
        is_bot: req_bool(o, "is_bot")?,
        display_name: req_str(o, "display_name")?,
    })
}

/// Owned app `ProjectLiteSerializer` row (`project.py:120-133`).
struct ProjectLiteOwned {
    id: String,
    identifier: String,
    name: String,
    cover_image: Option<String>,
    cover_image_url: Option<String>,
    logo_props: Value,
    description: String,
    is_default: bool,
}

fn project_lite_value(row: &ProjectLiteOwned) -> Result<Value, Denial> {
    use pidash_services::app_project::ser_member::{
        project_lite_to_representation, ProjectLiteRow,
    };
    let input = ProjectLiteRow {
        id: &row.id,
        identifier: &row.identifier,
        name: &row.name,
        cover_image: row.cover_image.as_deref(),
        cover_image_url: row.cover_image_url.as_deref(),
        logo_props: &row.logo_props,
        description: &row.description,
        is_default: row.is_default,
    };
    let view = project_lite_to_representation(&input);
    serde_json::to_value(&view).map_err(|_| Denial::ServerError)
}

/// `project_detail` leaf over a project id (default-manager guard).
async fn fetch_project_lite(
    pool: &sqlx::PgPool,
    project_id: &uuid::Uuid,
) -> Result<ProjectLiteOwned, Denial> {
    let row = fetch_optional_object(
        pool,
        "SELECT \"projects\".\"id\", \"projects\".\"identifier\", \"projects\".\"name\", \"projects\".\"cover_image\", \"projects\".\"cover_image_asset_id\", \"projects\".\"logo_props\", \"projects\".\"description\", \"projects\".\"is_default\" FROM \"projects\" WHERE (\"projects\".\"deleted_at\" IS NULL AND \"projects\".\"id\" = $1)",
        &[SqlParam::Uuid(*project_id)],
    )
    .await?
    .ok_or(Denial::ServerError)?;
    let o = obj(&row)?;
    let asset_id = opt_str(o, "cover_image_asset_id")?
        .map(|raw| raw.parse::<uuid::Uuid>())
        .transpose()
        .map_err(|_| Denial::ServerError)?;
    let cover_image_url = logo_or_cover_url(pool, opt_str(o, "cover_image")?, asset_id).await?;
    Ok(ProjectLiteOwned {
        id: req_str(o, "id")?,
        identifier: req_str(o, "identifier")?,
        name: req_str(o, "name")?,
        cover_image: opt_str(o, "cover_image")?,
        cover_image_url,
        logo_props: o.get("logo_props").cloned().ok_or(Denial::ServerError)?,
        description: req_str(o, "description")?,
        is_default: req_bool(o, "is_default")?,
    })
}

/// Owned app `WorkspaceLiteSerializer` row (`workspace.py:79-83`).
struct WorkspaceLiteOwned {
    name: String,
    slug: String,
    id: String,
    logo_url: Option<String>,
}

fn workspace_lite_value(row: &WorkspaceLiteOwned) -> Result<Value, Denial> {
    use pidash_services::app_project::ser_member::{
        workspace_lite_to_representation, WorkspaceLiteRow,
    };
    let input = WorkspaceLiteRow {
        name: &row.name,
        slug: &row.slug,
        id: &row.id,
        logo_url: row.logo_url.as_deref(),
    };
    let view = workspace_lite_to_representation(&input);
    serde_json::to_value(&view).map_err(|_| Denial::ServerError)
}

/// `workspace_detail` leaf over a workspace id (default-manager guard).
async fn fetch_workspace_lite(
    pool: &sqlx::PgPool,
    workspace_id: &uuid::Uuid,
) -> Result<WorkspaceLiteOwned, Denial> {
    let row = fetch_optional_object(
        pool,
        "SELECT \"workspaces\".\"id\", \"workspaces\".\"name\", \"workspaces\".\"slug\", \"workspaces\".\"logo\", \"workspaces\".\"logo_asset_id\" FROM \"workspaces\" WHERE (\"workspaces\".\"deleted_at\" IS NULL AND \"workspaces\".\"id\" = $1)",
        &[SqlParam::Uuid(*workspace_id)],
    )
    .await?
    .ok_or(Denial::ServerError)?;
    let o = obj(&row)?;
    let asset_id = opt_str(o, "logo_asset_id")?
        .map(|raw| raw.parse::<uuid::Uuid>())
        .transpose()
        .map_err(|_| Denial::ServerError)?;
    let logo_url = logo_or_cover_url(pool, opt_str(o, "logo")?, asset_id).await?;
    Ok(WorkspaceLiteOwned {
        name: req_str(o, "name")?,
        slug: req_str(o, "slug")?,
        id: req_str(o, "id")?,
        logo_url,
    })
}

/// Owned app `IssueFlatSerializer` row (`issue.py:105-123`).
struct IssueFlatOwned {
    id: String,
    name: String,
    description_json: Value,
    description_html: Option<String>,
    priority: String,
    complexity_score: i32,
    start_date: Option<String>,
    target_date: Option<String>,
    sequence_id: i32,
    sort_order: f64,
    is_draft: bool,
}

fn issue_flat_value(row: &IssueFlatOwned) -> Result<Value, Denial> {
    use pidash_services::app_issues::serializers_engage::{
        app_issue_flat_to_representation, AppIssueFlatRow,
    };
    let input = AppIssueFlatRow {
        id: &row.id,
        name: &row.name,
        description_json: &row.description_json,
        description_html: row.description_html.as_deref().unwrap_or(""),
        priority: &row.priority,
        complexity_score: row.complexity_score,
        start_date: row.start_date.as_deref(),
        target_date: row.target_date.as_deref(),
        sequence_id: row.sequence_id,
        sort_order: row.sort_order,
        is_draft: row.is_draft,
    };
    let view = app_issue_flat_to_representation(&input);
    serde_json::to_value(&view).map_err(|_| Denial::ServerError)
}

/// `issue_detail` leaf over an issue id (default-manager guard).
async fn fetch_issue_flat(
    pool: &sqlx::PgPool,
    issue_id: &uuid::Uuid,
) -> Result<IssueFlatOwned, Denial> {
    let row = fetch_optional_object(
        pool,
        "SELECT \"issues\".\"id\", \"issues\".\"name\", \"issues\".\"description_json\", \"issues\".\"description_html\", \"issues\".\"priority\", \"issues\".\"complexity_score\", \"issues\".\"start_date\", \"issues\".\"target_date\", \"issues\".\"sequence_id\", \"issues\".\"sort_order\", \"issues\".\"is_draft\" FROM \"issues\" WHERE (\"issues\".\"deleted_at\" IS NULL AND \"issues\".\"id\" = $1)",
        &[SqlParam::Uuid(*issue_id)],
    )
    .await?
    .ok_or(Denial::ServerError)?;
    let o = obj(&row)?;
    let complexity_score = match o.get("complexity_score") {
        Some(Value::Number(n)) => n.as_i64().ok_or(Denial::ServerError)? as i32,
        _ => return Err(Denial::ServerError),
    };
    let sequence_id = match o.get("sequence_id") {
        Some(Value::Number(n)) => n.as_i64().ok_or(Denial::ServerError)? as i32,
        _ => return Err(Denial::ServerError),
    };
    let sort_order = match o.get("sort_order") {
        Some(Value::Number(n)) => n.as_f64().ok_or(Denial::ServerError)?,
        _ => return Err(Denial::ServerError),
    };
    Ok(IssueFlatOwned {
        id: req_str(o, "id")?,
        name: req_str(o, "name")?,
        description_json: o
            .get("description_json")
            .cloned()
            .ok_or(Denial::ServerError)?,
        description_html: opt_str(o, "description_html")?,
        priority: req_str(o, "priority")?,
        complexity_score,
        start_date: opt_str(o, "start_date")?,
        target_date: opt_str(o, "target_date")?,
        sequence_id,
        sort_order,
        is_draft: req_bool(o, "is_draft")?,
    })
}

// ---------------------------------------------------------------------------
// Positional decoding of the joined builder statements
// ---------------------------------------------------------------------------

/// Column counts of the builder select lists (the join widths the
/// positional offsets below assume).
const PROJECT_COLS: usize = 46;
const WORKSPACE_COLS: usize = 14;
const ISSUE_COLS: usize = 34;

/// History statements select base + project + workspace + issue + user.
/// Base widths: activities 20, comments 24.
const HISTORY_PROJECT_AT: usize = 20;
const HISTORY_COMMENTS_PROJECT_AT: usize = 24;

/// `comment_list_sql` selects base 24 + `is_member` + project +
/// workspace + issue (no actor join).
const LIST_MEMBER_AT: usize = 24;
const LIST_PROJECT_AT: usize = 25;

/// `history_reactions_prefetch_sql` selects reactions 11 + user 40.
const PREFETCH_USER_AT: usize = 11;

/// Within-table offsets into `PROJECT_COLUMNS` (46).
const P_ID: usize = 5;
const P_NAME: usize = 6;
const P_DESCRIPTION: usize = 7;
const P_IDENTIFIER: usize = 12;
const P_IS_DEFAULT: usize = 24;
const P_COVER_IMAGE: usize = 27;
const P_COVER_IMAGE_ASSET_ID: usize = 28;
const P_LOGO_PROPS: usize = 32;

/// Within-table offsets into `WORKSPACE_COLUMNS` (14).
const W_ID: usize = 5;
const W_NAME: usize = 6;
const W_LOGO: usize = 7;
const W_LOGO_ASSET_ID: usize = 8;
const W_SLUG: usize = 10;

/// Within-table offsets into `PREFETCH_ISSUE_COLUMNS` (34).
const I_ID: usize = 5;
const I_NAME: usize = 12;
const I_DESCRIPTION_JSON: usize = 13;
const I_DESCRIPTION_HTML: usize = 14;
const I_PRIORITY: usize = 17;
const I_COMPLEXITY_SCORE: usize = 18;
const I_START_DATE: usize = 19;
const I_TARGET_DATE: usize = 20;
const I_SEQUENCE_ID: usize = 21;
const I_SORT_ORDER: usize = 22;
const I_IS_DRAFT: usize = 25;

/// Within-table offsets into `USER_COLUMNS` (40).
const U_ID: usize = 2;
const U_DISPLAY_NAME: usize = 6;
const U_FIRST_NAME: usize = 7;
const U_LAST_NAME: usize = 8;
const U_AVATAR: usize = 9;
const U_AVATAR_ASSET_ID: usize = 10;
const U_IS_BOT: usize = 35;

/// Within-table offsets into `COMMENT_COLUMNS` (24).
const C_CREATED_AT: usize = 0;
const C_UPDATED_AT: usize = 1;
const C_CREATED_BY_ID: usize = 2;
const C_UPDATED_BY_ID: usize = 3;
const C_DELETED_AT: usize = 4;
const C_ID: usize = 5;
const C_PROJECT_ID: usize = 6;
const C_WORKSPACE_ID: usize = 7;
const C_STRIPPED: usize = 8;
const C_JSON: usize = 9;
const C_HTML: usize = 10;
const C_DESCRIPTION_ID: usize = 11;
const C_ATTACHMENTS: usize = 12;
const C_LABELS: usize = 13;
const C_ISSUE_ID: usize = 14;
const C_ACTOR_ID: usize = 15;
const C_ACCESS: usize = 16;
const C_EXTERNAL_SOURCE: usize = 17;
const C_EXTERNAL_ID: usize = 18;
const C_SPEAKER_TYPE: usize = 19;
const C_SPEAKER_LABEL: usize = 20;
const C_SPEAKER_RUN_ID: usize = 21;
const C_EDITED_AT: usize = 22;
const C_PARENT_ID: usize = 23;

/// Within-table offsets into `ACTIVITY_COLUMNS` (20).
const A_CREATED_AT: usize = 0;
const A_UPDATED_AT: usize = 1;
const A_CREATED_BY_ID: usize = 2;
const A_UPDATED_BY_ID: usize = 3;
const A_DELETED_AT: usize = 4;
const A_ID: usize = 5;
const A_PROJECT_ID: usize = 6;
const A_WORKSPACE_ID: usize = 7;
const A_ISSUE_ID: usize = 8;
const A_VERB: usize = 9;
const A_FIELD: usize = 10;
const A_OLD_VALUE: usize = 11;
const A_NEW_VALUE: usize = 12;
const A_COMMENT: usize = 13;
const A_ATTACHMENTS: usize = 14;
const A_ISSUE_COMMENT_ID: usize = 15;
const A_ACTOR_ID: usize = 16;
const A_OLD_IDENTIFIER: usize = 17;
const A_NEW_IDENTIFIER: usize = 18;
const A_EPOCH: usize = 19;

/// Within-table offsets into `COMMENT_REACTION_COLUMNS` (11).
const R_CREATED_AT: usize = 0;
const R_UPDATED_AT: usize = 1;
const R_CREATED_BY_ID: usize = 2;
const R_UPDATED_BY_ID: usize = 3;
const R_DELETED_AT: usize = 4;
const R_ID: usize = 5;
const R_PROJECT_ID: usize = 6;
const R_WORKSPACE_ID: usize = 7;
const R_ACTOR_ID: usize = 8;
const R_COMMENT_ID: usize = 9;
const R_REACTION: usize = 10;

/// Decode the project-lite nest from a joined row at `base`.
async fn decode_project_lite(
    pool: &sqlx::PgPool,
    row: &sqlx::postgres::PgRow,
    base: usize,
) -> Result<ProjectLiteOwned, Denial> {
    let asset_id = cell_opt_uuid(row, base + P_COVER_IMAGE_ASSET_ID)?;
    let cover_image = cell_opt_string(row, base + P_COVER_IMAGE)?;
    let cover_image_url = logo_or_cover_url(pool, cover_image.clone(), asset_id).await?;
    Ok(ProjectLiteOwned {
        id: cell_uuid(row, base + P_ID)?.to_string(),
        identifier: cell_string(row, base + P_IDENTIFIER)?,
        name: cell_string(row, base + P_NAME)?,
        cover_image,
        cover_image_url,
        logo_props: cell_json(row, base + P_LOGO_PROPS)?,
        description: cell_string(row, base + P_DESCRIPTION)?,
        is_default: cell_bool(row, base + P_IS_DEFAULT)?,
    })
}

/// Decode the workspace-lite nest from a joined row at `base`.
async fn decode_workspace_lite(
    pool: &sqlx::PgPool,
    row: &sqlx::postgres::PgRow,
    base: usize,
) -> Result<WorkspaceLiteOwned, Denial> {
    let asset_id = cell_opt_uuid(row, base + W_LOGO_ASSET_ID)?;
    let logo = cell_opt_string(row, base + W_LOGO)?;
    let logo_url = logo_or_cover_url(pool, logo, asset_id).await?;
    Ok(WorkspaceLiteOwned {
        name: cell_string(row, base + W_NAME)?,
        slug: cell_string(row, base + W_SLUG)?,
        id: cell_uuid(row, base + W_ID)?.to_string(),
        logo_url,
    })
}

/// Decode the issue-flat nest from a joined row at `base`.
fn decode_issue_flat(row: &sqlx::postgres::PgRow, base: usize) -> Result<IssueFlatOwned, Denial> {
    Ok(IssueFlatOwned {
        id: cell_uuid(row, base + I_ID)?.to_string(),
        name: cell_string(row, base + I_NAME)?,
        description_json: cell_json(row, base + I_DESCRIPTION_JSON)?,
        description_html: cell_opt_string(row, base + I_DESCRIPTION_HTML)?,
        priority: cell_string(row, base + I_PRIORITY)?,
        complexity_score: cell_i32(row, base + I_COMPLEXITY_SCORE)?,
        start_date: cell_opt_date(row, base + I_START_DATE)?.map(|d| render_date(&d)),
        target_date: cell_opt_date(row, base + I_TARGET_DATE)?.map(|d| render_date(&d)),
        sequence_id: cell_i32(row, base + I_SEQUENCE_ID)?,
        sort_order: cell_f64(row, base + I_SORT_ORDER)?,
        is_draft: cell_bool(row, base + I_IS_DRAFT)?,
    })
}

/// Decode the actor-lite nest from a joined row at `base`. `None` when
/// the LEFT JOIN missed (null actor), like the null source rendering
/// `actor_detail` as `null`.
async fn decode_user_lite(
    pool: &sqlx::PgPool,
    row: &sqlx::postgres::PgRow,
    base: usize,
) -> Result<Option<UserLiteOwned>, Denial> {
    if cell_opt_uuid(row, base + U_ID)?.is_none() {
        return Ok(None);
    }
    let asset_id = cell_opt_uuid(row, base + U_AVATAR_ASSET_ID)?;
    let avatar = cell_string(row, base + U_AVATAR)?;
    let avatar_url = logo_or_cover_url(pool, Some(avatar.clone()), asset_id).await?;
    Ok(Some(UserLiteOwned {
        id: cell_uuid(row, base + U_ID)?.to_string(),
        first_name: cell_string(row, base + U_FIRST_NAME)?,
        last_name: cell_string(row, base + U_LAST_NAME)?,
        avatar,
        avatar_url,
        is_bot: cell_bool(row, base + U_IS_BOT)?,
        display_name: cell_string(row, base + U_DISPLAY_NAME)?,
    }))
}

// ---------------------------------------------------------------------------
// Sync predicates
// ---------------------------------------------------------------------------

/// `_comment_is_actively_synced` (`issue.py:79-87`): native rows
/// (`external_source` empty) short-circuit before any DB probe.
async fn comment_is_synced(
    pool: &sqlx::PgPool,
    external_source: Option<&str>,
    comment_id: &uuid::Uuid,
) -> Result<bool, Denial> {
    use pidash_services::app_issues::serializers_engage::{
        GITHUB_COMMENT_SYNC_PROBE_SQL, GIT_COMMENT_SYNC_PROBE_SQL,
    };
    if external_source.unwrap_or("").is_empty() {
        return Ok(false);
    }
    let git: Option<(i32,)> = sqlx::query_as(GIT_COMMENT_SYNC_PROBE_SQL)
        .bind(comment_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    if git.is_some() {
        return Ok(true);
    }
    let github: Option<(i32,)> = sqlx::query_as(GITHUB_COMMENT_SYNC_PROBE_SQL)
        .bind(comment_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(github.is_some())
}

/// The destroy guard (`comment.py:158-160`): a raw OR over both sync
/// tables, with NO `external_source` short-circuit — unlike
/// [`comment_is_synced`].
async fn comment_has_sync_row(
    pool: &sqlx::PgPool,
    comment_id: &uuid::Uuid,
) -> Result<bool, Denial> {
    use pidash_services::app_issues::serializers_engage::{
        GITHUB_COMMENT_SYNC_PROBE_SQL, GIT_COMMENT_SYNC_PROBE_SQL,
    };
    let git: Option<(i32,)> = sqlx::query_as(GIT_COMMENT_SYNC_PROBE_SQL)
        .bind(comment_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    if git.is_some() {
        return Ok(true);
    }
    let github: Option<(i32,)> = sqlx::query_as(GITHUB_COMMENT_SYNC_PROBE_SQL)
        .bind(comment_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(github.is_some())
}

// ---------------------------------------------------------------------------
// Serializer validation (DRF field-error parity)
// ---------------------------------------------------------------------------

/// Push one message onto a field's error list, preserving field order
/// (the map is insertion-ordered).
fn push_error(errors: &mut Map<String, Value>, field: &str, message: String) {
    match errors.get_mut(field) {
        Some(Value::Array(list)) => list.push(Value::String(message)),
        _ => {
            errors.insert(field.to_owned(), Value::Array(vec![Value::String(message)]));
        }
    }
}

/// DRF field-input outcome: missing → the caller applies partial-skip
/// / defaults; null / value otherwise. (Errors push into `errors` and
/// the caller bails when it is non-empty.)
enum Presence<T> {
    Missing,
    Null,
    Value(T),
}

/// DRF `CharField` input (`fields.py:692-788`): ints/floats coerce
/// through `str()` (bools do NOT — `True` is invalid); the blank gate
/// runs on the whitespace-stripped spelling (`"   "` → blank error, or
/// `""` when `allow_blank`); then `MaxLengthValidator` on the stripped
/// value (in *characters*, like Python `len`) and
/// `ProhibitNullCharactersValidator`. Uploaded files (the
/// `{"__file__": …}` sentinel) and other shapes → invalid.
fn check_text(
    errors: &mut Map<String, Value>,
    body: &Map<String, Value>,
    field: &str,
    max_length: Option<usize>,
    allow_null: bool,
    allow_blank: bool,
) -> Presence<String> {
    match body.get(field) {
        None => Presence::Missing,
        Some(Value::Null) => {
            if allow_null {
                Presence::Null
            } else {
                push_error(errors, field, "This field may not be null.".to_owned());
                Presence::Missing
            }
        }
        Some(Value::Number(number)) => {
            let coerced = py_str_value(&Value::Number(number.clone()));
            check_text_tail(errors, field, &coerced, max_length, allow_blank)
        }
        Some(Value::String(text)) => check_text_tail(errors, field, text, max_length, allow_blank),
        _ => {
            push_error(errors, field, "Not a valid string.".to_owned());
            Presence::Missing
        }
    }
}

/// The string tail of [`check_text`]: strip, blank gate, `max_length`,
/// NUL gate. Returns `Missing` after pushing an error.
fn check_text_tail(
    errors: &mut Map<String, Value>,
    field: &str,
    text: &str,
    max_length: Option<usize>,
    allow_blank: bool,
) -> Presence<String> {
    let stripped = text.trim();
    if stripped.is_empty() {
        if !allow_blank {
            push_error(errors, field, "This field may not be blank.".to_owned());
            return Presence::Missing;
        }
        return Presence::Value(String::new());
    }
    // DRF `run_validators` collects EVERY failure: overlong + NUL
    // yields both messages in validator order.
    let mut failed = false;
    if let Some(max) = max_length {
        if stripped.chars().count() > max {
            push_error(
                errors,
                field,
                format!("Ensure this field has no more than {max} characters."),
            );
            failed = true;
        }
    }
    if stripped.contains('\0') {
        push_error(errors, field, "Null characters are not allowed.".to_owned());
        failed = true;
    }
    if failed {
        return Presence::Missing;
    }
    Presence::Value(stripped.to_owned())
}

/// DRF `ChoiceField` semantics (`fields.py:1177-1250`): null → "may not
/// be null" (none of ours allow null); anything else stringifies
/// through CPython `str()` — scalars raw, containers with `repr()`
/// elements (`"['INTERNAL']"`, `"{'a': 1}"`, `"1e+16"`, all probed
/// live) — for the membership test. HTML-input uploads stringify to
/// their filename. No blank/whitespace handling, no validators.
fn check_choice(
    errors: &mut Map<String, Value>,
    body: &Map<String, Value>,
    field: &str,
    valid: &[&str],
    is_html: bool,
) -> Presence<String> {
    match body.get(field) {
        None => Presence::Missing,
        Some(Value::Null) => {
            push_error(errors, field, "This field may not be null.".to_owned());
            Presence::Missing
        }
        Some(value) => {
            let input = match (is_html, uploaded_file_name(value)) {
                (true, Some(name)) => name,
                _ => py_str_value(value),
            };
            if valid.contains(&input.as_str()) {
                Presence::Value(input)
            } else {
                push_error(errors, field, format!("\"{input}\" is not a valid choice."));
                Presence::Missing
            }
        }
    }
}

/// The multipart filename behind a `{"__file__": name}` sentinel value
/// (see `read_body`), if the value is one.
fn uploaded_file_name(value: &Value) -> Option<String> {
    match value {
        Value::Object(map) => match map.get("__file__") {
            Some(Value::String(name)) if map.len() == 1 => Some(name.clone()),
            _ => None,
        },
        _ => None,
    }
}

/// DRF `DateTimeField` input: Django `parse_datetime` is
/// `datetime.fromisoformat` (plus a regex fallback that only accepts what
/// `fromisoformat` already does). Naive reads as UTC under `USE_TZ`;
/// offsets convert. Anything else → the wrong-format message.
///
/// Grammar (all probed live): date `YYYY-MM-DD` (1-2 digit month/day) /
/// `YYYYMMDD` / `YYYY-Www-D` / `YYYYWwwD` (no ordinal, uppercase `W`
/// only); optional time after ANY single separator char (separator
/// required when time is present); time `HH[:MM[:SS[.ffffff]]]` with
/// 1-2 digit parts, hour-alone exactly 2 digits, or basic `HHMM[SS]` +
/// optional `[.,]fraction` (truncated to 6, never rounded); optional
/// `Z` (uppercase, needs a time) or `±HH:MM[:SS[.f]]` / `±HHMM[SS]` /
/// `±HH` with hour < 24. No surrounding-whitespace trim.
fn parse_input_datetime(text: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    if text.is_empty() || text.len() > 64 {
        return None;
    }
    if !text.is_ascii() {
        return None;
    }
    let bytes = text.as_bytes();
    let (date, rest) = split_iso_date(bytes)?;
    if rest.is_empty() {
        return Some(date.and_hms_opt(0, 0, 0)?.and_utc());
    }
    // One separator char (any), then the time; the time must be
    // non-empty, so a lone date+`Z` fails while `+05:00` parses as a
    // time with a `+` separator (both probed live).
    let time_text = rest.get(1..)?;
    if time_text.is_empty() {
        return None;
    }
    let (hour, minute, second, micros, tz_offset) = split_iso_time(time_text)?;
    let naive = date.and_hms_micro_opt(hour, minute, second, micros)?;
    match tz_offset {
        None => Some(naive.and_utc()),
        Some(offset_micros) => naive
            .checked_sub_signed(chrono::Duration::microseconds(offset_micros))
            .map(|utc| utc.and_utc()),
    }
}

/// Split the leading ISO date (`YYYY-MM-DD`, `YYYYMMDD`, `YYYY-Www-D`,
/// `YYYYWwwD`), returning the date plus the unparsed remainder.
fn split_iso_date(bytes: &[u8]) -> Option<(chrono::NaiveDate, &[u8])> {
    if bytes.len() >= 4 && bytes[..4].iter().all(|b| b.is_ascii_digit()) {
        let year: i32 = text_to_int(&bytes[..4])?;
        if !(1..=9999).contains(&year) {
            return None;
        }
        let tail = &bytes[4..];
        // Week dates: `-Www-D` / `WwwD` (uppercase `W` only).
        if let Some(week_tail) = tail.strip_prefix(b"-W").or_else(|| tail.strip_prefix(b"W")) {
            if week_tail.len() >= 2
                && week_tail[..2].iter().all(|b| b.is_ascii_digit())
                && week_tail.get(2) == Some(&b'-')
                && week_tail.len() >= 4
            {
                let week: u32 = text_to_int(&week_tail[..2])?;
                let weekday = week_tail[3];
                if !(b'1'..=b'7').contains(&weekday) {
                    return None;
                }
                let date = chrono::NaiveDate::from_isoywd_opt(year, week, iso_weekday(weekday)?)?;
                return Some((date, &week_tail[4..]));
            }
            if tail.starts_with(b"W")
                && week_tail.len() >= 3
                && week_tail[..2].iter().all(|b| b.is_ascii_digit())
                && (b'1'..=b'7').contains(&week_tail[2])
            {
                let week: u32 = text_to_int(&week_tail[..2])?;
                let date =
                    chrono::NaiveDate::from_isoywd_opt(year, week, iso_weekday(week_tail[2])?)?;
                return Some((date, &week_tail[3..]));
            }
            return None;
        }
        // Calendar dates: `-M-D` / `-MM-DD` / `MMDD`.
        if let Some(dashed) = tail.strip_prefix(b"-") {
            let (month, rest) = split_digits(dashed, 1, 2)?;
            let rest = rest.strip_prefix(b"-")?;
            let (day, rest) = split_digits(rest, 1, 2)?;
            let date = chrono::NaiveDate::from_ymd_opt(year, month, day)?;
            return Some((date, rest));
        }
        if tail.len() >= 4 && tail[..4].iter().all(|b| b.is_ascii_digit()) {
            let month: u32 = text_to_int(&tail[..2])?;
            let day: u32 = text_to_int(&tail[2..4])?;
            let date = chrono::NaiveDate::from_ymd_opt(year, month, day)?;
            return Some((date, &tail[4..]));
        }
    }
    None
}

/// ISO weekday digit (`b'1'`–`b'7'`) → [`chrono::Weekday`].
fn iso_weekday(digit: u8) -> Option<chrono::Weekday> {
    match digit {
        b'1' => Some(chrono::Weekday::Mon),
        b'2' => Some(chrono::Weekday::Tue),
        b'3' => Some(chrono::Weekday::Wed),
        b'4' => Some(chrono::Weekday::Thu),
        b'5' => Some(chrono::Weekday::Fri),
        b'6' => Some(chrono::Weekday::Sat),
        b'7' => Some(chrono::Weekday::Sun),
        _ => None,
    }
}

/// Split 1-`max` ASCII digits (at least `min`), returning the value plus
/// the remainder.
fn split_digits(bytes: &[u8], min: usize, max: usize) -> Option<(u32, &[u8])> {
    let mut len = 0;
    while len < max && len < bytes.len() && bytes[len].is_ascii_digit() {
        len += 1;
    }
    if len < min {
        return None;
    }
    Some((text_to_int(&bytes[..len])?, &bytes[len..]))
}

/// ASCII digits → int (no sign, no whitespace, no empty — Rust's
/// `parse` would accept a leading `+`, which `fromisoformat` rejects).
fn text_to_int<T: std::str::FromStr>(bytes: &[u8]) -> Option<T> {
    if bytes.is_empty() || !bytes.iter().all(|b| b.is_ascii_digit()) {
        return None;
    }
    std::str::from_utf8(bytes).ok()?.parse().ok()
}

/// Split the time + zone: `(hour, minute, second, micros,
/// offset_micros?)`.
fn split_iso_time(text: &[u8]) -> Option<(u32, u32, u32, u32, Option<i64>)> {
    // Zone suffix first: `Z` or a trailing `±...` offset. `Z` needs a
    // non-empty time before it (the caller guarantees non-empty text,
    // so a bare `Z` fails below as a bad time).
    if text == b"Z" {
        return None;
    }
    if let Some(time_text) = text.strip_suffix(b"Z") {
        if time_text.is_empty() {
            return None;
        }
        let (hour, minute, second, micros) = split_iso_clock(time_text)?;
        return Some((hour, minute, second, micros, Some(0)));
    }
    // Trailing offset: scan the last `+`/`-` past position 0 (a leading
    // sign belongs to nothing valid here — dates never start with one).
    let mut sign_at = None;
    for (index, byte) in text.iter().enumerate().skip(1) {
        if *byte == b'+' || *byte == b'-' {
            sign_at = Some(index);
        }
    }
    if let Some(at) = sign_at {
        let (time_text, zone_text) = text.split_at(at);
        if time_text.is_empty() {
            return None;
        }
        let (hour, minute, second, micros) = split_iso_clock(time_text)?;
        let offset = split_iso_offset(zone_text)?;
        return Some((hour, minute, second, micros, Some(offset)));
    }
    let (hour, minute, second, micros) = split_iso_clock(text)?;
    Some((hour, minute, second, micros, None))
}

/// Split the clock (`HH[:MM[:SS[.ffffff]]]`, 1-2 digit parts, or basic
/// `HHMM[SS]` + fraction), returning `(hour, minute, second, micros)`.
fn split_iso_clock(text: &[u8]) -> Option<(u32, u32, u32, u32)> {
    if text.contains(&b':') {
        let mut parts = text.split(|b| *b == b':');
        let hour = parts.next()?;
        let minute = parts.next()?;
        let second = parts.next();
        if parts.next().is_some() {
            return None;
        }
        if hour.is_empty() || hour.len() > 2 || minute.is_empty() || minute.len() > 2 {
            return None;
        }
        let hour: u32 = text_to_int(hour)?;
        let minute: u32 = text_to_int(minute)?;
        let (second, micros) = match second {
            None => (0, 0),
            Some(second) => {
                if second.is_empty() {
                    return None;
                }
                split_iso_seconds(second)?
            }
        };
        if hour > 23 || minute > 59 || second > 59 {
            return None;
        }
        return Some((hour, minute, second, micros));
    }
    // Basic: 2/4/6 digits + optional fraction.
    let (digits, fraction) = match text.iter().position(|b| *b == b'.' || *b == b',') {
        Some(at) => (&text[..at], Some(&text[at + 1..])),
        None => (text, None),
    };
    if digits.is_empty() || !digits.iter().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let (hour, minute, second) = match digits.len() {
        2 => (text_to_int::<u32>(digits)?, 0, 0),
        4 => (
            text_to_int::<u32>(&digits[..2])?,
            text_to_int::<u32>(&digits[2..])?,
            0,
        ),
        6 => (
            text_to_int::<u32>(&digits[..2])?,
            text_to_int::<u32>(&digits[2..4])?,
            text_to_int::<u32>(&digits[4..])?,
        ),
        _ => return None,
    };
    let micros = match fraction {
        None => 0,
        Some(fraction) => iso_fraction_micros(fraction)?,
    };
    if hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    Some((hour, minute, second, micros))
}

/// Split `SS[.ffffff]` (1-2 digit seconds), returning `(second, micros)`.
fn split_iso_seconds(text: &[u8]) -> Option<(u32, u32)> {
    let (digits, fraction) = match text.iter().position(|b| *b == b'.' || *b == b',') {
        Some(at) => (&text[..at], Some(&text[at + 1..])),
        None => (text, None),
    };
    if digits.is_empty() || digits.len() > 2 {
        return None;
    }
    let second: u32 = text_to_int(digits)?;
    let micros = match fraction {
        None => 0,
        Some(fraction) => iso_fraction_micros(fraction)?,
    };
    Some((second, micros))
}

/// Fraction digits → micros, truncated (never rounded) to 6 places.
fn iso_fraction_micros(fraction: &[u8]) -> Option<u32> {
    if fraction.is_empty() || !fraction.iter().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let mut digits = [b'0'; 6];
    let take = fraction.len().min(6);
    digits[..take].copy_from_slice(&fraction[..take]);
    text_to_int::<u32>(&digits)
}

/// `±HH:MM[:SS[.f]]` / `±HHMM[SS]` / `±HH` → offset micros.
fn split_iso_offset(text: &[u8]) -> Option<i64> {
    let (negative, text) = match text.first() {
        Some(b'+') => (false, &text[1..]),
        Some(b'-') => (true, &text[1..]),
        _ => return None,
    };
    if text.is_empty() {
        return None;
    }
    let (hour, minute, second, micros) = if text.contains(&b':') {
        let mut parts = text.split(|b| *b == b':');
        let hour = parts.next()?;
        let minute = parts.next()?;
        let second = parts.next();
        if parts.next().is_some() || hour.len() != 2 || minute.len() != 2 {
            return None;
        }
        let (second, micros) = match second {
            None => (0, 0),
            Some(second) => {
                if second.len() < 2 {
                    return None;
                }
                split_iso_seconds(second)?
            }
        };
        (
            text_to_int::<i64>(hour)?,
            text_to_int::<i64>(minute)?,
            second as i64,
            micros,
        )
    } else {
        if !text.iter().all(|b| b.is_ascii_digit()) || ![2, 4, 6].contains(&text.len()) {
            return None;
        }
        let hour = text_to_int::<i64>(&text[..2])?;
        let minute = if text.len() >= 4 {
            text_to_int::<i64>(&text[2..4])?
        } else {
            0
        };
        let second = if text.len() == 6 {
            text_to_int::<i64>(&text[4..])?
        } else {
            0
        };
        (hour, minute, second, 0)
    };
    if hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    let total = hour * 3_600_000_000 + minute * 60_000_000 + second * 1_000_000 + micros as i64;
    Some(if negative { -total } else { total })
}

const DATETIME_FORMAT_MESSAGE: &str = "Datetime has wrong format. Use one of these formats instead: YYYY-MM-DDThh:mm[:ss[.uuuuuu]][+HH:MM|-HH:MM|Z].";

fn check_datetime(
    errors: &mut Map<String, Value>,
    body: &Map<String, Value>,
    field: &str,
    allow_null: bool,
) -> Presence<chrono::DateTime<chrono::Utc>> {
    match body.get(field) {
        None => Presence::Missing,
        Some(Value::Null) => {
            if allow_null {
                Presence::Null
            } else {
                push_error(errors, field, "This field may not be null.".to_owned());
                Presence::Missing
            }
        }
        Some(Value::String(text)) => match parse_input_datetime(text) {
            Some(dt) => Presence::Value(dt),
            None => {
                push_error(errors, field, DATETIME_FORMAT_MESSAGE.to_owned());
                Presence::Missing
            }
        },
        _ => {
            push_error(errors, field, DATETIME_FORMAT_MESSAGE.to_owned());
            Presence::Missing
        }
    }
}

/// DRF `UUIDField` input (`fields.py:1469-1503`): ints (bools
/// included) go through `UUID(int=…)` (`True` → `…0001`, out of range →
/// invalid); strings go through `UUID(hex=…)` (all four spellings —
/// dashed, plain, braced, `urn:` — parsed, all probed live); anything
/// else (floats, lists, dicts, `""`) → "Must be a valid UUID."
fn check_uuid(
    errors: &mut Map<String, Value>,
    body: &Map<String, Value>,
    field: &str,
    allow_null: bool,
) -> Presence<uuid::Uuid> {
    match body.get(field) {
        None => Presence::Missing,
        Some(Value::Null) => {
            if allow_null {
                Presence::Null
            } else {
                push_error(errors, field, "This field may not be null.".to_owned());
                Presence::Missing
            }
        }
        Some(Value::Bool(flag)) => {
            Presence::Value(uuid::Uuid::from_u128(if *flag { 1 } else { 0 }))
        }
        Some(Value::Number(number)) => match uuid_from_json_int(number) {
            Some(id) => Presence::Value(id),
            None => {
                push_error(errors, field, "Must be a valid UUID.".to_owned());
                Presence::Missing
            }
        },
        Some(Value::String(text)) => match uuid_from_json_hex(text) {
            Some(id) => Presence::Value(id),
            None => {
                // `uuid.UUID(hex="")` raises too (`ValueError` → the same
                // `invalid` message), so `""` needs no special case.
                push_error(errors, field, "Must be a valid UUID.".to_owned());
                Presence::Missing
            }
        },
        _ => {
            push_error(errors, field, "Must be a valid UUID.".to_owned());
            Presence::Missing
        }
    }
}

/// `uuid.UUID(int=…)` over a JSON integer: non-negatives below 2¹²⁸
/// only (negatives and huge values raise `ValueError`).
fn uuid_from_json_int(number: &serde_json::Number) -> Option<uuid::Uuid> {
    if let Some(int) = number.as_i64() {
        if int < 0 {
            return None;
        }
        return Some(uuid::Uuid::from_u128(int as u128));
    }
    if let Some(uint) = number.as_u64() {
        // `u64` always fits the 128-bit space.
        let _ = uint;
        return number
            .to_string()
            .parse::<u128>()
            .ok()
            .map(uuid::Uuid::from_u128);
    }
    // Arbitrary-precision integers past `u64`: `UUID(int=…)` accepts
    // anything below 2¹²⁸.
    number
        .to_string()
        .parse::<u128>()
        .ok()
        .map(uuid::Uuid::from_u128)
}

/// `uuid.UUID(hex=…)` over a string: dashed, plain-hex, braced, and
/// `urn:uuid:` spellings (all four probed live through the PK path).
fn uuid_from_json_hex(text: &str) -> Option<uuid::Uuid> {
    if text.is_empty() {
        return None;
    }
    text.parse::<uuid::Uuid>().ok()
}

/// DRF `PrimaryKeyRelatedField` input over `table`
/// (`relations.py:103-150` + `RelatedField.run_validation`): HTML-input
/// `""` coerces to `None` (`Field.get_value`, allow_null branch —
/// then the null gate); JSON `""` runs `to_internal_value` instead
/// (`UUID(hex="")` → the curly error, `UUIDField.get_prep_value` via
/// `to_python`); bools → `incorrect_type`; ints go through
/// `UUID(int=…)` (out of range → the curly error); strings go through
/// `UUID(hex=…)`; floats/lists/dicts and bad UUIDs surface Django's
/// `ValidationError` — `“<py-str>” is not a valid UUID.` (curly quotes
/// verbatim, CPython `str()` rendering, all probed live) — as a FIELD
/// error, caught per-field by DRF, never the invalid-detail 400.
/// Valid-but-missing pks quote the RAW input: `Invalid pk "<raw>" -
/// object does not exist.` Existence reads the default manager
/// (soft-deleted rows do not count), except `users` which has no
/// `deleted_at`. HTML-input uploads stringify to their filename for
/// the curly error. (HTML `""` on a non-nullable field falls through
/// to the curly error too — right for our one such caller, the
/// required subscriber FK, where `get_value` also falls through.)
async fn check_fk(
    pool: &sqlx::PgPool,
    errors: &mut Map<String, Value>,
    body: &Map<String, Value>,
    field: &str,
    table: &str,
    allow_null: bool,
    is_html: bool,
) -> Result<Presence<uuid::Uuid>, Denial> {
    let raw: Option<(String, Option<uuid::Uuid>)> = match body.get(field) {
        None => return Ok(Presence::Missing),
        Some(Value::Null) => {
            if allow_null {
                return Ok(Presence::Null);
            }
            push_error(errors, field, "This field may not be null.".to_owned());
            return Ok(Presence::Missing);
        }
        Some(Value::String(text)) if text.is_empty() && is_html && allow_null => {
            return Ok(Presence::Null);
        }
        Some(Value::Bool(_)) => {
            push_error(
                errors,
                field,
                "Incorrect type. Expected pk value, received bool.".to_owned(),
            );
            return Ok(Presence::Missing);
        }
        Some(Value::Number(number)) => {
            let rendered = number.to_string();
            Some((rendered, uuid_from_json_int(number)))
        }
        Some(Value::String(text)) => Some((text.clone(), uuid_from_json_hex(text))),
        Some(value) => {
            let rendered = match (is_html, uploaded_file_name(value)) {
                (true, Some(name)) => name,
                _ => py_str_value(value),
            };
            Some((rendered, None))
        }
    };
    let (raw, id) = raw.expect("fk match always yields Some here");
    let Some(id) = id else {
        push_error(
            errors,
            field,
            format!("\u{201c}{raw}\u{201d} is not a valid UUID."),
        );
        return Ok(Presence::Missing);
    };
    let sql = if table == "users" {
        format!("SELECT 1 FROM {table} WHERE id = $1")
    } else {
        format!("SELECT 1 FROM {table} WHERE id = $1 AND deleted_at IS NULL")
    };
    let exists: Option<(i32,)> = sqlx::query_as(&sql)
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    if exists.is_none() {
        push_error(
            errors,
            field,
            format!("Invalid pk \"{raw}\" - object does not exist."),
        );
        return Ok(Presence::Missing);
    }
    Ok(Presence::Value(id))
}

/// Django `URLValidator` (`django/core/validators.py:69-160`), line for
/// line: length/unsafe-chars/scheme gates, the full URL regex
/// (case-insensitive), the IDN→ACE retry, strict IPv6, the 253-char
/// hostname cap. All verdicts below probed live.
fn is_valid_url(text: &str) -> bool {
    if text.chars().count() > 2048 {
        return false;
    }
    if text.contains(['\t', '\r', '\n']) {
        return false;
    }
    // The scheme is the text before the first `://`, lowercased.
    let scheme = text.split("://").next().unwrap_or("");
    if !matches!(
        scheme.to_lowercase().as_str(),
        "http" | "https" | "ftp" | "ftps"
    ) {
        return false;
    }
    // `urlsplit` must not raise: unmatched brackets / bad `%` escapes /
    // NFKC-rejected netloc fail here (all fail the regex too, except a
    // few `urlsplit`-only shapes below).
    if urlsplit_raises(text) {
        return false;
    }
    if match_url_regex(text) {
        return check_ipv6_netloc(text) && check_hostname_length(text);
    }
    // IDN retry: ACE-encode the host and re-run the regex.
    let Some(ascii) = idna_retry_url(text) else {
        return false;
    };
    match_url_regex(&ascii) && check_ipv6_netloc(&ascii) && check_hostname_length(&ascii)
}

/// Shapes `urllib.parse.urlsplit` rejects with `ValueError`: an
/// unmatched `[` in the authority, or a non-ASCII authority holding a
/// char whose NFKC form contains `/?#@:` (`_checknetloc`: fullwidth
/// `／？＃＠：`, small-form `﹕﹖﹟﹫`, presentation `︓︖`, and the
/// `a/c`-style expansions `℀℁℅℆` — all probed live). Bad `%` escapes
/// do NOT raise (`user%zz@…` verifies 201).
fn urlsplit_raises(text: &str) -> bool {
    let authority = url_authority(text);
    if authority.contains('[') && !authority.contains(']') {
        return true;
    }
    if authority.is_ascii() {
        return false;
    }
    authority.chars().any(|ch| {
        matches!(
            ch,
            '\u{ff0f}'
                | '\u{ff1f}'
                | '\u{ff03}'
                | '\u{ff20}'
                | '\u{ff1a}'
                | '\u{fe55}'
                | '\u{fe56}'
                | '\u{fe5f}'
                | '\u{fe6b}'
                | '\u{fe13}'
                | '\u{fe16}'
                | '\u{2100}'
                | '\u{2101}'
                | '\u{2105}'
                | '\u{2106}'
        )
    })
}

/// The authority (userinfo + host + port): after `://`, before the
/// first `/`, `?` or `#`.
fn url_authority(text: &str) -> &str {
    let after_scheme = text.split("://").nth(1).unwrap_or("");
    let end = after_scheme
        .find(['/', '?', '#'])
        .unwrap_or(after_scheme.len());
    &after_scheme[..end]
}

/// The full `URLValidator.regex` match (case-insensitive): optional
/// userinfo, IPv4 / bracketed-IPv6 / hostname, optional port, optional
/// resource path, full-string.
fn match_url_regex(text: &str) -> bool {
    let after_scheme = match text.split_once("://") {
        Some((_, rest)) => rest,
        None => return false,
    };
    // Optional `user[:pass]@` (no whitespace, `:`, `@` or `/` inside;
    // must precede any `/`, `?`, `#`).
    let after_userinfo = match url_split_userinfo(after_scheme) {
        Some(rest) => rest,
        None => return false,
    };
    // Host: IPv4, bracketed IPv6, or hostname/`localhost`.
    let Some((host_end, host_kind)) = url_split_host(after_userinfo) else {
        return false;
    };
    let mut rest = &after_userinfo[host_end..];
    // Optional `:port` (1-5 digits).
    if let Some(port_text) = rest.strip_prefix(':') {
        let digits: String = port_text
            .chars()
            .take_while(|ch| ch.is_ascii_digit())
            .collect();
        if digits.is_empty() || digits.len() > 5 {
            return false;
        }
        rest = &port_text[digits.len()..];
    }
    // Hostname flavor also gates the host match (TLD validity).
    if !host_kind {
        return false;
    }
    // Optional resource path: starts with `/`, `?` or `#`, no
    // whitespace after.
    if rest.is_empty() {
        return true;
    }
    match rest.chars().next() {
        Some('/' | '?' | '#') => !rest.chars().any(char::is_whitespace),
        _ => false,
    }
}

/// Split optional userinfo, returning the host remainder. More than one
/// `@` before the path can never match (the userinfo class excludes
/// `@`, the host classes exclude it too).
fn url_split_userinfo(text: &str) -> Option<&str> {
    let path_at = text.find(['/', '?', '#']).unwrap_or(text.len());
    let (head, _) = text.split_at(path_at);
    let at_count = head.chars().filter(|ch| *ch == '@').count();
    if at_count == 0 {
        return Some(text);
    }
    if at_count > 1 {
        return None;
    }
    let at = head.find('@')?;
    let (user, _) = head.split_at(at);
    if user.is_empty() {
        return None;
    }
    let (name, pass) = match user.split_once(':') {
        Some((name, pass)) => (name, pass),
        None => (user, ""),
    };
    if name.is_empty()
        || name
            .chars()
            .any(|ch| ch.is_whitespace() || matches!(ch, ':' | '@' | '/'))
        || pass
            .chars()
            .any(|ch| ch.is_whitespace() || matches!(ch, ':' | '@' | '/'))
    {
        return None;
    }
    Some(&text[at + 1..])
}

/// Split the host, returning `(byte length, valid)`: strict-quad IPv4,
/// loose-bracket IPv6 (validated later), or hostname/`localhost`.
fn url_split_host(text: &str) -> Option<(usize, bool)> {
    // IPv6: `[{hex}:.]+]` (loose here; strict in `check_ipv6_netloc`).
    if let Some(rest) = text.strip_prefix('[') {
        let end = rest.find(']')?;
        let inner = &rest[..end];
        if inner.is_empty()
            || !inner
                .chars()
                .all(|ch| ch.is_ascii_hexdigit() || ch == ':' || ch == '.')
        {
            return None;
        }
        return Some((end + 2, true));
    }
    // The host runs to the first `:`, `/`, `?` or `#`.
    let end = text.find([':', '/', '?', '#']).unwrap_or(text.len());
    let host = &text[..end];
    if host.is_empty() {
        return None;
    }
    if host.eq_ignore_ascii_case("localhost") {
        return Some((end, true));
    }
    if is_strict_ipv4(host) {
        return Some((end, true));
    }
    // Otherwise the hostname branch decides (labels may start with
    // digits; only the TLD rule can fail a numeric quad here, e.g.
    // `1.2.3.4.5` or `01.2.3.4` — both probed 400).
    Some((end, is_valid_hostname(host)))
}

/// Strict IPv4 quad: four `0`-`255` parts, no leading zeros (except `0`
/// itself): `(?:0|25[0-5]|2[0-4][0-9]|1[0-9]?[0-9]?|[1-9][0-9]?)(\.\1){3}`.
fn is_strict_ipv4(host: &str) -> bool {
    let parts: Vec<&str> = host.split('.').collect();
    if parts.len() != 4 {
        return false;
    }
    parts.iter().all(|part| {
        if part.is_empty() || part.len() > 3 || !part.bytes().all(|b| b.is_ascii_digit()) {
            return false;
        }
        if part.len() > 1 && part.starts_with('0') {
            return false;
        }
        part.parse::<u32>().is_ok_and(|num| num <= 255)
    })
}

/// Hostname: first label 1-63 (`[a-z\u00a1-\uffff0-9]`, dashes inside
/// only), middle labels 1-63 (no leading/trailing dash), TLD dot +
/// (2-63 `[a-z\u00a1-\uffff-]` or `xn--[a-z0-9]{1,59}`, no
/// leading/trailing dash) + optional trailing dot. Case-insensitive.
fn is_valid_hostname(host: &str) -> bool {
    let host = host.strip_suffix('.').unwrap_or(host);
    if host.is_empty() {
        return false;
    }
    let labels: Vec<&str> = host.split('.').collect();
    if labels.len() < 2 {
        return false;
    }
    let (tld, rest) = labels.split_last().expect("hostname has labels");
    if !is_valid_tld(tld) {
        return false;
    }
    let (first, middle) = rest.split_first().expect("hostname has labels");
    if !is_valid_first_label(first) {
        return false;
    }
    middle.iter().all(|label| is_valid_middle_label(label))
}

/// First hostname label: starts/ends `[a-z\u00a1-\uffff0-9]`, up to 61
/// middle `[a-z\u00a1-\uffff0-9-]` chars.
fn is_valid_first_label(label: &str) -> bool {
    let count = label.chars().count();
    if count == 0 || count > 63 {
        return false;
    }
    let mut chars = label.chars();
    let first = chars.next().expect("label is non-empty");
    let last = label.chars().next_back().expect("label is non-empty");
    if !is_hostname_edge_char(first) || !is_hostname_edge_char(last) {
        return false;
    }
    chars.all(is_hostname_middle_char)
}

/// Middle hostname label: 1-63 chars, no leading/trailing dash.
fn is_valid_middle_label(label: &str) -> bool {
    let count = label.chars().count();
    if count == 0 || count > 63 {
        return false;
    }
    if label.starts_with('-') || label.ends_with('-') {
        return false;
    }
    label.chars().all(is_hostname_middle_char)
}

/// TLD: 2-63 `[a-z\u00a1-\uffff-]` (no digits) or punycode
/// `xn--[a-z0-9]{1,59}`, never starting/ending with a dash.
fn is_valid_tld(tld: &str) -> bool {
    if tld.starts_with('-') || tld.ends_with('-') {
        return false;
    }
    if tld.len() >= 4 && tld[..4].eq_ignore_ascii_case("xn--") {
        let rest = &tld[4..];
        return (1..=59).contains(&rest.len()) && rest.bytes().all(|b| b.is_ascii_alphanumeric());
    }
    let count = tld.chars().count();
    if !(2..=63).contains(&count) {
        return false;
    }
    tld.chars()
        .all(|ch| ch == '-' || ch.is_ascii_alphabetic() || is_unicode_letter(ch))
}

/// `[a-z\u00a1-\uffff0-9]` (case-insensitive ASCII).
fn is_hostname_edge_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || is_unicode_letter(ch)
}

/// `[a-z\u00a1-\uffff0-9-]` (case-insensitive ASCII).
fn is_hostname_middle_char(ch: char) -> bool {
    ch == '-' || is_hostname_edge_char(ch)
}

/// The `\u00a1-\uffff` Unicode-letter range.
fn is_unicode_letter(ch: char) -> bool {
    ('\u{a1}'..='\u{ffff}').contains(&ch)
}

/// Strict IPv6 check on a bracketed netloc host
/// (`validate_ipv6_address` over the bracket contents).
fn check_ipv6_netloc(text: &str) -> bool {
    let authority = url_authority(text);
    let host_port = authority.rsplit('@').next().unwrap_or(authority);
    if !host_port.starts_with('[') {
        return true;
    }
    let Some(end) = host_port.find(']') else {
        return false;
    };
    host_port[1..end].parse::<std::net::Ipv6Addr>().is_ok()
}

/// `urlsplit().hostname` present and at most 253 chars (lowercased,
/// brackets stripped, trailing dot kept).
fn check_hostname_length(text: &str) -> bool {
    let authority = url_authority(text);
    let host_port = authority.rsplit('@').next().unwrap_or(authority);
    let host = if host_port.starts_with('[') {
        match host_port.find(']') {
            Some(end) => &host_port[1..end],
            None => return false,
        }
    } else {
        match host_port.find(':') {
            Some(at) => &host_port[..at],
            None => host_port,
        }
    };
    if host.is_empty() {
        return false;
    }
    host.to_lowercase().chars().count() <= 253
}

/// IDN retry: ACE-encode (`idna`, UTS46 — the retry only runs when the
/// direct Unicode match already failed, where IDNA 2003 vs UTS46 agree
/// on every reachable input) the hostname labels and rebuild the URL.
fn idna_retry_url(text: &str) -> Option<String> {
    let scheme_end = text.find("://")? + 3;
    let (scheme, after_scheme) = text.split_at(scheme_end);
    let path_at = after_scheme
        .find(['/', '?', '#'])
        .unwrap_or(after_scheme.len());
    let (authority, path) = after_scheme.split_at(path_at);
    let (userinfo, host_port) = match authority.rfind('@') {
        Some(at) => (&authority[..=at], &authority[at + 1..]),
        None => ("", authority),
    };
    if host_port.starts_with('[') {
        return None;
    }
    let (host, port) = match host_port.find(':') {
        Some(at) => (&host_port[..at], &host_port[at..]),
        None => (host_port, ""),
    };
    if host.is_empty() || host.is_ascii() {
        return None;
    }
    let ascii = idna::domain_to_ascii(host).ok()?;
    Some(format!("{scheme}{userinfo}{ascii}{port}{path}"))
}

/// DRF `ListField` input (`fields.py:1640-1720`) over an
/// `ArrayField`: non-lists fail `not_a_list` (CPython type names:
/// `int`, `str`, `dict`, `bool`, `float` — all probed live); children
/// run the child field and failures collect into a DICT keyed by
/// decimal index (`{"0": […], "2": […]}` — failed positions only);
/// then the `ArrayField(size=…)` max-length validator runs
/// (`ArrayMaxLengthValidator`, ngettext singular/plural, probed live).
/// Child validation is `child(value)`; `max_items` is the array size.
fn check_string_list(
    errors: &mut Map<String, Value>,
    body: &Map<String, Value>,
    field: &str,
    max_items: usize,
    child: &dyn Fn(&Value) -> Result<String, Vec<String>>,
) -> Presence<Vec<String>> {
    let list = match body.get(field) {
        None => return Presence::Missing,
        Some(Value::Null) => {
            push_error(errors, field, "This field may not be null.".to_owned());
            return Presence::Missing;
        }
        Some(Value::Array(list)) => list,
        Some(other) => {
            push_error(
                errors,
                field,
                format!(
                    "Expected a list of items but got type \"{}\".",
                    json_type_name(other),
                ),
            );
            return Presence::Missing;
        }
    };
    let mut out = Vec::with_capacity(list.len());
    let mut item_errors = Map::new();
    for (index, value) in list.iter().enumerate() {
        match child(value) {
            Ok(stored) => out.push(stored),
            Err(messages) => {
                item_errors.insert(
                    index.to_string(),
                    Value::Array(messages.into_iter().map(Value::String).collect()),
                );
            }
        }
    }
    if !item_errors.is_empty() {
        errors.insert(field.to_owned(), Value::Object(item_errors));
        return Presence::Missing;
    }
    if list.len() > max_items {
        let message = if list.len() == 1 {
            format!("List contains 1 item, it should contain no more than {max_items}.")
        } else {
            format!(
                "List contains {} items, it should contain no more than {max_items}.",
                list.len()
            )
        };
        push_error(errors, field, message);
        return Presence::Missing;
    }
    Presence::Value(out)
}

/// The `labels` child (`CharField(max_length=32)`): ints/floats coerce,
/// bools/containers/files invalid, blank gate, `max_length`, NUL gate.
fn validate_label_child(value: &Value) -> Result<String, Vec<String>> {
    let text = match value {
        Value::Null => return Err(vec!["This field may not be null.".to_owned()]),
        Value::Number(number) => py_str_value(&Value::Number(number.clone())),
        Value::String(text) => text.clone(),
        _ => return Err(vec!["Not a valid string.".to_owned()]),
    };
    let stripped = text.trim();
    if stripped.is_empty() {
        return Err(vec!["This field may not be blank.".to_owned()]);
    }
    let mut messages = Vec::new();
    if stripped.chars().count() > 32 {
        messages.push("Ensure this field has no more than 32 characters.".to_owned());
    }
    if stripped.contains('\0') {
        messages.push("Null characters are not allowed.".to_owned());
    }
    if messages.is_empty() {
        Ok(stripped.to_owned())
    } else {
        Err(messages)
    }
}

/// The `attachments` child (`URLField(max_length=200)`): null → the
/// null message; every other shape stringifies through CPython `str()`
/// and runs the child validators in order — `MaxLengthValidator`,
/// `ProhibitNullCharactersValidator`, `URLValidator` (overlong AND bad
/// URLs yield BOTH messages, probed live). The field overrides the
/// `invalid` message, so type failures also read "Enter a valid URL."
fn validate_attachment_child(value: &Value) -> Result<String, Vec<String>> {
    if matches!(value, Value::Null) {
        return Err(vec!["This field may not be null.".to_owned()]);
    }
    let text = match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => py_str_value(&Value::Number(number.clone())),
        Value::Bool(_) | Value::Array(_) | Value::Object(_) => {
            return Err(vec!["Enter a valid URL.".to_owned()]);
        }
        Value::Null => unreachable!("null handled above"),
    };
    let stripped = text.trim();
    if stripped.is_empty() {
        return Err(vec!["This field may not be blank.".to_owned()]);
    }
    // DRF `run_validators` collects EVERY failure (never stops at
    // the first), so overlong + NUL + bad URLs yield up to three
    // messages in validator order.
    let mut messages = Vec::new();
    if stripped.chars().count() > 200 {
        messages.push("Ensure this field has no more than 200 characters.".to_owned());
    }
    if stripped.contains('\0') {
        messages.push("Null characters are not allowed.".to_owned());
    }
    if !is_valid_url(stripped) {
        messages.push("Enter a valid URL.".to_owned());
    }
    if messages.is_empty() {
        Ok(stripped.to_owned())
    } else {
        Err(messages)
    }
}

// ---------------------------------------------------------------------------
// Comments: validation + shapes + render
// ---------------------------------------------------------------------------

/// A decoded `issue_comments` row (positional or map — both land here).
#[derive(Debug, Clone)]
struct CommentBaseOwned {
    id: uuid::Uuid,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
    created_by: Option<uuid::Uuid>,
    updated_by: Option<uuid::Uuid>,
    deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    project_id: uuid::Uuid,
    workspace_id: uuid::Uuid,
    stripped: String,
    json: Value,
    html: String,
    description_id: Option<uuid::Uuid>,
    attachments: Vec<String>,
    labels: Vec<String>,
    issue_id: uuid::Uuid,
    actor_id: Option<uuid::Uuid>,
    access: String,
    external_source: Option<String>,
    external_id: Option<String>,
    speaker_type: String,
    speaker_label: String,
    speaker_run_id: Option<uuid::Uuid>,
    edited_at: Option<chrono::DateTime<chrono::Utc>>,
    parent_id: Option<uuid::Uuid>,
}

impl CommentBaseOwned {
    /// Decode the base columns of a joined builder row (`COMMENT_COLUMNS`
    /// at offset 0).
    fn from_positional(row: &sqlx::postgres::PgRow) -> Result<Self, Denial> {
        Ok(Self {
            id: cell_uuid(row, C_ID)?,
            created_at: cell_datetime(row, C_CREATED_AT)?,
            updated_at: cell_datetime(row, C_UPDATED_AT)?,
            created_by: cell_opt_uuid(row, C_CREATED_BY_ID)?,
            updated_by: cell_opt_uuid(row, C_UPDATED_BY_ID)?,
            deleted_at: cell_opt_datetime(row, C_DELETED_AT)?,
            project_id: cell_uuid(row, C_PROJECT_ID)?,
            workspace_id: cell_uuid(row, C_WORKSPACE_ID)?,
            stripped: cell_string(row, C_STRIPPED)?,
            json: cell_json(row, C_JSON)?,
            html: cell_string(row, C_HTML)?,
            description_id: cell_opt_uuid(row, C_DESCRIPTION_ID)?,
            attachments: cell_str_array(row, C_ATTACHMENTS)?,
            labels: cell_str_array(row, C_LABELS)?,
            issue_id: cell_uuid(row, C_ISSUE_ID)?,
            actor_id: cell_opt_uuid(row, C_ACTOR_ID)?,
            access: cell_string(row, C_ACCESS)?,
            external_source: cell_opt_string(row, C_EXTERNAL_SOURCE)?,
            external_id: cell_opt_string(row, C_EXTERNAL_ID)?,
            speaker_type: cell_string(row, C_SPEAKER_TYPE)?,
            speaker_label: cell_string(row, C_SPEAKER_LABEL)?,
            speaker_run_id: cell_opt_uuid(row, C_SPEAKER_RUN_ID)?,
            edited_at: cell_opt_datetime(row, C_EDITED_AT)?,
            parent_id: cell_opt_uuid(row, C_PARENT_ID)?,
        })
    }

    /// Decode a single-table `row_to_json` map (column names as keys).
    fn from_map(map: &Map<String, Value>) -> Result<Self, Denial> {
        let uuid = |key: &str| -> Result<uuid::Uuid, Denial> {
            req_str(map, key)?
                .parse::<uuid::Uuid>()
                .map_err(|_| Denial::ServerError)
        };
        let opt_uuid = |key: &str| -> Result<Option<uuid::Uuid>, Denial> {
            opt_str(map, key)?
                .map(|raw| raw.parse::<uuid::Uuid>())
                .transpose()
                .map_err(|_| Denial::ServerError)
        };
        let dt = |key: &str| -> Result<chrono::DateTime<chrono::Utc>, Denial> {
            let raw = req_str(map, key)?;
            chrono::DateTime::parse_from_rfc3339(&raw)
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .map_err(|_| Denial::ServerError)
        };
        let opt_dt = |key: &str| -> Result<Option<chrono::DateTime<chrono::Utc>>, Denial> {
            opt_str(map, key)?
                .map(|raw| {
                    chrono::DateTime::parse_from_rfc3339(&raw)
                        .map(|dt| dt.with_timezone(&chrono::Utc))
                })
                .transpose()
                .map_err(|_| Denial::ServerError)
        };
        let str_array = |key: &str| -> Result<Vec<String>, Denial> {
            match map.get(key) {
                Some(Value::Array(items)) => items
                    .iter()
                    .map(|item| match item {
                        Value::String(s) => Ok(s.clone()),
                        _ => Err(Denial::ServerError),
                    })
                    .collect(),
                _ => Err(Denial::ServerError),
            }
        };
        Ok(Self {
            id: uuid("id")?,
            created_at: dt("created_at")?,
            updated_at: dt("updated_at")?,
            created_by: opt_uuid("created_by_id")?.or(opt_uuid("created_by")?),
            updated_by: opt_uuid("updated_by_id")?.or(opt_uuid("updated_by")?),
            deleted_at: opt_dt("deleted_at")?,
            project_id: uuid("project_id")?,
            workspace_id: uuid("workspace_id")?,
            stripped: req_str(map, "comment_stripped")?,
            json: map
                .get("comment_json")
                .cloned()
                .ok_or(Denial::ServerError)?,
            html: req_str(map, "comment_html")?,
            description_id: opt_uuid("description_id")?,
            attachments: str_array("attachments")?,
            labels: str_array("labels")?,
            issue_id: uuid("issue_id")?,
            actor_id: opt_uuid("actor_id")?,
            access: req_str(map, "access")?,
            external_source: opt_str(map, "external_source")?,
            external_id: opt_str(map, "external_id")?,
            speaker_type: req_str(map, "speaker_type")?,
            speaker_label: req_str(map, "speaker_label")?,
            speaker_run_id: opt_uuid("speaker_agent_run_id")?,
            edited_at: opt_dt("edited_at")?,
            parent_id: opt_uuid("parent_id")?,
        })
    }
}

/// Validated `IssueCommentSerializer` input: `None` = absent (the caller
/// applies partial-skip / full defaults), `Some` = provided.
#[derive(Debug, Default)]
struct CommentValidated {
    deleted_at: Option<Option<chrono::DateTime<chrono::Utc>>>,
    stripped: Option<String>,
    json: Option<Value>,
    html: Option<String>,
    description: Option<Option<uuid::Uuid>>,
    attachments: Option<Vec<String>>,
    labels: Option<Vec<String>>,
    actor: Option<Option<uuid::Uuid>>,
    access: Option<String>,
    external_source: Option<Option<String>>,
    external_id: Option<Option<String>>,
    speaker_type: Option<String>,
    speaker_label: Option<String>,
    speaker_run: Option<Option<uuid::Uuid>>,
    edited_at: Option<Option<chrono::DateTime<chrono::Utc>>>,
    parent: Option<Option<uuid::Uuid>>,
}

/// `Presence` → `Some` when provided (null included), `None` when
/// missing or invalid.
fn presence_opt<T>(presence: Presence<T>) -> Option<Option<T>> {
    match presence {
        Presence::Missing => None,
        Presence::Null => Some(None),
        Presence::Value(value) => Some(Some(value)),
    }
}

/// `Presence` → value when a real value validated, else `None`.
fn presence_value<T>(presence: Presence<T>) -> Option<T> {
    match presence {
        Presence::Value(value) => Some(value),
        _ => None,
    }
}

/// `IssueCommentSerializer(data).is_valid()` (`issue.py:948-969`): every
/// writable field in live-DRF serializer order (relations trail the
/// concrete fields: `description`, `actor`, `parent` come after
/// `edited_at` — introspected live, since error-key order is
/// observable). Unknown keys are ignored; the sync-lock `validate()`
/// runs separately (it needs the instance).
#[allow(clippy::field_reassign_with_default)]
async fn validate_comment_input(
    pool: &sqlx::PgPool,
    body: &Map<String, Value>,
    is_html: bool,
) -> Result<CommentValidated, Denial> {
    let mut errors = Map::new();
    let mut out = CommentValidated::default();
    out.deleted_at = presence_opt(check_datetime(&mut errors, body, "deleted_at", true));
    out.stripped = presence_value(check_text(
        &mut errors,
        body,
        "comment_stripped",
        None,
        false,
        true,
    ));
    out.json = match body.get("comment_json") {
        None => None,
        Some(Value::Null) => {
            push_error(
                &mut errors,
                "comment_json",
                "This field may not be null.".to_owned(),
            );
            None
        }
        Some(value) => {
            if is_html && uploaded_file_name(value).is_some() {
                // HTML-input uploads fail the `json.dumps` round-trip
                // check (`TypeError` → invalid).
                push_error(
                    &mut errors,
                    "comment_json",
                    "Value must be valid JSON.".to_owned(),
                );
                None
            } else {
                Some(value.clone())
            }
        }
    };
    out.html = presence_value(check_text(
        &mut errors,
        body,
        "comment_html",
        None,
        false,
        true,
    ));
    out.attachments = presence_value(check_string_list(
        &mut errors,
        body,
        "attachments",
        10,
        &validate_attachment_child,
    ));
    out.labels = presence_value(check_string_list(
        &mut errors,
        body,
        "labels",
        8,
        &validate_label_child,
    ));
    out.access = presence_value(check_choice(
        &mut errors,
        body,
        "access",
        &["INTERNAL", "EXTERNAL"],
        is_html,
    ));
    out.external_source = presence_opt(check_text(
        &mut errors,
        body,
        "external_source",
        Some(255),
        true,
        true,
    ));
    out.external_id = presence_opt(check_text(
        &mut errors,
        body,
        "external_id",
        Some(255),
        true,
        true,
    ));
    out.speaker_type = presence_value(check_choice(
        &mut errors,
        body,
        "speaker_type",
        &["human", "agent", "system", "integration"],
        is_html,
    ));
    out.speaker_label = presence_value(check_text(
        &mut errors,
        body,
        "speaker_label",
        Some(128),
        false,
        true,
    ));
    out.speaker_run = presence_opt(check_uuid(&mut errors, body, "speaker_agent_run_id", true));
    out.edited_at = presence_opt(check_datetime(&mut errors, body, "edited_at", true));
    out.description = presence_opt(
        check_fk(
            pool,
            &mut errors,
            body,
            "description",
            "descriptions",
            true,
            is_html,
        )
        .await?,
    );
    out.actor =
        presence_opt(check_fk(pool, &mut errors, body, "actor", "users", true, is_html).await?);
    out.parent = presence_opt(
        check_fk(
            pool,
            &mut errors,
            body,
            "parent",
            "issue_comments",
            true,
            is_html,
        )
        .await?,
    );
    if !errors.is_empty() {
        return Err(Denial::BadJson(Value::Object(errors)));
    }
    Ok(out)
}

/// The sync-lock `validate()` (`issue.py:974-991`): on a synced
/// instance, touched-AND-changed locked fields refuse with the
/// per-field message.
fn comment_sync_lock_errors(
    stored: &CommentBaseOwned,
    attrs: &CommentValidated,
) -> Map<String, Value> {
    const MESSAGE: &str = "This comment is synced from a Git provider and is read-only. Unbind the project's repository to edit.";
    let mut errors = Map::new();
    if let Some(html) = &attrs.html {
        if *html != stored.html {
            push_error(&mut errors, "comment_html", MESSAGE.to_owned());
        }
    }
    if let Some(json) = &attrs.json {
        // Python `dict.__eq__` (cross-type numerics compare: `1 == 1.0`).
        if !py_json_eq(json, &stored.json) {
            push_error(&mut errors, "comment_json", MESSAGE.to_owned());
        }
    }
    if let Some(stripped) = &attrs.stripped {
        if *stripped != stored.stripped {
            push_error(&mut errors, "comment_stripped", MESSAGE.to_owned());
        }
    }
    errors
}

/// App `CommentReactionSerializer` (`issue.py:917-937`), in
/// `Meta.fields` order. `display_name` reads `actor.display_name`.
#[derive(Debug, serde::Serialize)]
struct CommentReactionView {
    id: String,
    actor: String,
    comment: String,
    reaction: String,
    display_name: String,
    deleted_at: Option<String>,
    workspace: String,
    project: String,
    created_at: String,
    updated_at: String,
    created_by: Option<String>,
    updated_by: Option<String>,
}

/// App `IssueCommentSerializer` (`issue.py:948-972`): `Meta.fields =
/// "__all__"` in live-DRF order (pk, declared, concrete, forward
/// relations). `is_member` is `Some` only on annotated-queryset rows
/// (list / retrieve / PUT); create / PATCH / history-comment omit it
/// (DRF `SkipField`).
#[derive(Debug, serde::Serialize)]
struct CommentView {
    id: String,
    actor_detail: Option<Value>,
    issue_detail: Value,
    project_detail: Value,
    workspace_detail: Value,
    comment_reactions: Vec<CommentReactionView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    is_member: Option<bool>,
    is_synced: bool,
    created_at: String,
    updated_at: String,
    deleted_at: Option<String>,
    comment_stripped: String,
    comment_json: Value,
    comment_html: String,
    attachments: Vec<String>,
    labels: Vec<String>,
    access: String,
    external_source: Option<String>,
    external_id: Option<String>,
    speaker_type: String,
    speaker_label: String,
    speaker_agent_run_id: Option<String>,
    edited_at: Option<String>,
    created_by: Option<String>,
    updated_by: Option<String>,
    project: String,
    workspace: String,
    description: Option<String>,
    issue: String,
    actor: Option<String>,
    parent: Option<String>,
}

/// `display_name` for reaction rows (`source="actor.display_name"`).
/// The `users` read is unguarded (no `deleted_at` column); a dangling
/// FK is a 500 like Python's `DoesNotExist` into the fallback.
async fn fetch_display_name(pool: &sqlx::PgPool, user_id: &uuid::Uuid) -> Result<String, Denial> {
    let row: Option<(String,)> = sqlx::query_as("SELECT display_name FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    row.map(|row| row.0).ok_or(Denial::ServerError)
}

/// Render one reaction map (single-table `row_to_json`, column names as
/// keys) with its `display_name` leaf.
async fn render_comment_reaction_map(
    pool: &sqlx::PgPool,
    map: &Map<String, Value>,
    tz: &Tz,
) -> Result<CommentReactionView, Denial> {
    let actor_id = req_str(map, "actor_id")?
        .parse::<uuid::Uuid>()
        .map_err(|_| Denial::ServerError)?;
    let display_name = fetch_display_name(pool, &actor_id).await?;
    Ok(CommentReactionView {
        id: req_str(map, "id")?,
        actor: req_str(map, "actor_id")?,
        comment: req_str(map, "comment_id")?,
        reaction: req_str(map, "reaction")?,
        display_name,
        deleted_at: render_dt_str_opt(opt_str(map, "deleted_at")?, tz)?,
        workspace: req_str(map, "workspace_id")?,
        project: req_str(map, "project_id")?,
        created_at: render_dt_str(&req_str(map, "created_at")?, tz)?,
        updated_at: render_dt_str(&req_str(map, "updated_at")?, tz)?,
        created_by: opt_str(map, "created_by_id")?.or(opt_str(map, "created_by")?),
        updated_by: opt_str(map, "updated_by_id")?.or(opt_str(map, "updated_by")?),
    })
}

/// The live reaction rows for one comment, newest first (model
/// `ordering = ("-created_at",)`): the reverse-manager read the
/// serializer performs per row outside the history prefetch.
async fn render_comment_reactions_for(
    pool: &sqlx::PgPool,
    comment_id: &uuid::Uuid,
    tz: &Tz,
) -> Result<Vec<CommentReactionView>, Denial> {
    let rows = fetch_all_objects(
        pool,
        "SELECT * FROM comment_reactions WHERE deleted_at IS NULL AND comment_id = $1 ORDER BY created_at DESC",
        &[SqlParam::Uuid(*comment_id)],
    )
    .await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in &rows {
        out.push(render_comment_reaction_map(pool, obj(row)?, tz).await?);
    }
    Ok(out)
}

/// Assemble a [`CommentView`] from a decoded base, resolved nests, the
/// reaction list and the sync verdict.
#[allow(clippy::too_many_arguments)]
fn assemble_comment_view(
    base: &CommentBaseOwned,
    actor_detail: Option<Value>,
    issue_detail: Value,
    project_detail: Value,
    workspace_detail: Value,
    comment_reactions: Vec<CommentReactionView>,
    is_member: Option<bool>,
    is_synced: bool,
    tz: &Tz,
) -> CommentView {
    CommentView {
        id: base.id.to_string(),
        actor_detail,
        issue_detail,
        project_detail,
        workspace_detail,
        comment_reactions,
        is_member,
        is_synced,
        created_at: render_dt(&base.created_at, tz),
        updated_at: render_dt(&base.updated_at, tz),
        deleted_at: render_dt_opt(base.deleted_at, tz),
        comment_stripped: base.stripped.clone(),
        comment_json: base.json.clone(),
        comment_html: base.html.clone(),
        attachments: base.attachments.clone(),
        labels: base.labels.clone(),
        access: base.access.clone(),
        external_source: base.external_source.clone(),
        external_id: base.external_id.clone(),
        speaker_type: base.speaker_type.clone(),
        speaker_label: base.speaker_label.clone(),
        speaker_agent_run_id: base.speaker_run_id.map(|id| id.to_string()),
        edited_at: render_dt_opt(base.edited_at, tz),
        created_by: base.created_by.map(|id| id.to_string()),
        updated_by: base.updated_by.map(|id| id.to_string()),
        project: base.project_id.to_string(),
        workspace: base.workspace_id.to_string(),
        description: base.description_id.map(|id| id.to_string()),
        issue: base.issue_id.to_string(),
        actor: base.actor_id.map(|id| id.to_string()),
        parent: base.parent_id.map(|id| id.to_string()),
    }
}

/// Serialize the owned nests through the shared lite ports.
fn nest_values(
    actor: &Option<UserLiteOwned>,
    issue: &IssueFlatOwned,
    project: &ProjectLiteOwned,
    workspace: &WorkspaceLiteOwned,
) -> Result<(Option<Value>, Value, Value, Value), Denial> {
    let actor_detail = actor.as_ref().map(user_lite_value).transpose()?;
    let issue_detail = issue_flat_value(issue)?;
    let project_detail = project_lite_value(project)?;
    let workspace_detail = workspace_lite_value(workspace)?;
    Ok((actor_detail, issue_detail, project_detail, workspace_detail))
}

/// Render one history-branch comment: every nest comes from the joined
/// row; `is_member` stays absent; reactions come from the batched
/// prefetch map.
async fn render_history_comment(
    pool: &sqlx::PgPool,
    row: &sqlx::postgres::PgRow,
    tz: &Tz,
    reactions: Vec<CommentReactionView>,
) -> Result<Value, Denial> {
    let base = CommentBaseOwned::from_positional(row)?;
    let project = decode_project_lite(pool, row, HISTORY_COMMENTS_PROJECT_AT).await?;
    let workspace =
        decode_workspace_lite(pool, row, HISTORY_COMMENTS_PROJECT_AT + PROJECT_COLS).await?;
    let issue = decode_issue_flat(
        row,
        HISTORY_COMMENTS_PROJECT_AT + PROJECT_COLS + WORKSPACE_COLS,
    )?;
    let actor = decode_user_lite(
        pool,
        row,
        HISTORY_COMMENTS_PROJECT_AT + PROJECT_COLS + WORKSPACE_COLS + ISSUE_COLS,
    )
    .await?;
    let is_synced = comment_is_synced(pool, base.external_source.as_deref(), &base.id).await?;
    let (actor_detail, issue_detail, project_detail, workspace_detail) =
        nest_values(&actor, &issue, &project, &workspace)?;
    let view = assemble_comment_view(
        &base,
        actor_detail,
        issue_detail,
        project_detail,
        workspace_detail,
        reactions,
        None,
        is_synced,
        tz,
    );
    serde_json::to_value(&view).map_err(|_| Denial::ServerError)
}

/// Render one list-queryset comment: project / workspace / issue come
/// from the joined row, the actor loads lazily (no actor join on the
/// list statement), `is_member` comes from the annotation.
async fn render_list_comment(
    pool: &sqlx::PgPool,
    row: &sqlx::postgres::PgRow,
    tz: &Tz,
) -> Result<Value, Denial> {
    let base = CommentBaseOwned::from_positional(row)?;
    let is_member = cell_bool(row, LIST_MEMBER_AT)?;
    let project = decode_project_lite(pool, row, LIST_PROJECT_AT).await?;
    let workspace = decode_workspace_lite(pool, row, LIST_PROJECT_AT + PROJECT_COLS).await?;
    let issue = decode_issue_flat(row, LIST_PROJECT_AT + PROJECT_COLS + WORKSPACE_COLS)?;
    let actor = match base.actor_id {
        None => None,
        Some(id) => Some(fetch_user_lite(pool, &id).await?),
    };
    let reactions = render_comment_reactions_for(pool, &base.id, tz).await?;
    let is_synced = comment_is_synced(pool, base.external_source.as_deref(), &base.id).await?;
    let (actor_detail, issue_detail, project_detail, workspace_detail) =
        nest_values(&actor, &issue, &project, &workspace)?;
    let view = assemble_comment_view(
        &base,
        actor_detail,
        issue_detail,
        project_detail,
        workspace_detail,
        reactions,
        Some(is_member),
        is_synced,
        tz,
    );
    serde_json::to_value(&view).map_err(|_| Denial::ServerError)
}

/// Render a comment from a decoded base alone (create / update /
/// destroy-payload paths): every nest loads lazily, exactly like the
/// serializer over a fresh instance.
async fn render_base_comment(
    pool: &sqlx::PgPool,
    base: &CommentBaseOwned,
    tz: &Tz,
    is_member: Option<bool>,
) -> Result<Value, Denial> {
    let actor = match base.actor_id {
        None => None,
        Some(id) => Some(fetch_user_lite(pool, &id).await?),
    };
    let issue = fetch_issue_flat(pool, &base.issue_id).await?;
    let project = fetch_project_lite(pool, &base.project_id).await?;
    let workspace = fetch_workspace_lite(pool, &base.workspace_id).await?;
    let reactions = render_comment_reactions_for(pool, &base.id, tz).await?;
    let is_synced = comment_is_synced(pool, base.external_source.as_deref(), &base.id).await?;
    let (actor_detail, issue_detail, project_detail, workspace_detail) =
        nest_values(&actor, &issue, &project, &workspace)?;
    let view = assemble_comment_view(
        base,
        actor_detail,
        issue_detail,
        project_detail,
        workspace_detail,
        reactions,
        is_member,
        is_synced,
        tz,
    );
    serde_json::to_value(&view).map_err(|_| Denial::ServerError)
}

// ---------------------------------------------------------------------------
// Activities: shapes + render
// ---------------------------------------------------------------------------

/// The intake source triple behind `source_data`
/// (`activity.py:67-73`).
struct IntakeOwned {
    source: Option<String>,
    source_email: Option<String>,
    extra: Value,
}

/// Owned `IssueActivitySerializer` row: every nest plus the rendered
/// scalars the services shape borrows.
struct ActivityRender {
    id: String,
    actor: Option<UserLiteOwned>,
    issue: Option<IssueFlatOwned>,
    project: ProjectLiteOwned,
    workspace: WorkspaceLiteOwned,
    source: Option<IntakeOwned>,
    created_at: String,
    updated_at: String,
    deleted_at: Option<String>,
    verb: String,
    field: Option<String>,
    old_value: Option<String>,
    new_value: Option<String>,
    comment: String,
    attachments: Vec<String>,
    old_identifier: Option<String>,
    new_identifier: Option<String>,
    epoch: Option<f64>,
    created_by: Option<String>,
    updated_by: Option<String>,
    project_id: String,
    workspace_id: String,
    issue_id: Option<String>,
    issue_comment_id: Option<String>,
    actor_id: Option<String>,
}

/// Render one history-branch activity: every nest comes from the
/// joined row; `source_data` resolves from the intake prefetch map
/// (first element) or stays `None` — exactly the `:529` guard chain.
async fn render_history_activity(
    pool: &sqlx::PgPool,
    row: &sqlx::postgres::PgRow,
    tz: &Tz,
    intake: Option<IntakeOwned>,
) -> Result<Value, Denial> {
    use pidash_services::app_issues::serializers_engage::{
        issue_activity_to_representation, ActivitySourceData, IssueActivityRow,
    };
    let project = decode_project_lite(pool, row, HISTORY_PROJECT_AT).await?;
    let workspace = decode_workspace_lite(pool, row, HISTORY_PROJECT_AT + PROJECT_COLS).await?;
    let issue = decode_issue_flat(row, HISTORY_PROJECT_AT + PROJECT_COLS + WORKSPACE_COLS)?;
    let actor = decode_user_lite(
        pool,
        row,
        HISTORY_PROJECT_AT + PROJECT_COLS + WORKSPACE_COLS + ISSUE_COLS,
    )
    .await?;
    let render = ActivityRender {
        id: cell_uuid(row, A_ID)?.to_string(),
        actor,
        // The history statement pins `issue_id` to the requested issue,
        // so the joined issue is always the activity's own.
        issue: Some(issue),
        project,
        workspace,
        source: intake,
        created_at: render_dt(&cell_datetime(row, A_CREATED_AT)?, tz),
        updated_at: render_dt(&cell_datetime(row, A_UPDATED_AT)?, tz),
        deleted_at: render_dt_opt(cell_opt_datetime(row, A_DELETED_AT)?, tz),
        verb: cell_string(row, A_VERB)?,
        field: cell_opt_string(row, A_FIELD)?,
        old_value: cell_opt_string(row, A_OLD_VALUE)?,
        new_value: cell_opt_string(row, A_NEW_VALUE)?,
        comment: cell_string(row, A_COMMENT)?,
        attachments: cell_str_array(row, A_ATTACHMENTS)?,
        old_identifier: cell_opt_uuid(row, A_OLD_IDENTIFIER)?.map(|id| id.to_string()),
        new_identifier: cell_opt_uuid(row, A_NEW_IDENTIFIER)?.map(|id| id.to_string()),
        epoch: cell_opt_f64(row, A_EPOCH)?,
        created_by: cell_opt_uuid(row, A_CREATED_BY_ID)?.map(|id| id.to_string()),
        updated_by: cell_opt_uuid(row, A_UPDATED_BY_ID)?.map(|id| id.to_string()),
        project_id: cell_uuid(row, A_PROJECT_ID)?.to_string(),
        workspace_id: cell_uuid(row, A_WORKSPACE_ID)?.to_string(),
        issue_id: cell_opt_uuid(row, A_ISSUE_ID)?.map(|id| id.to_string()),
        issue_comment_id: cell_opt_uuid(row, A_ISSUE_COMMENT_ID)?.map(|id| id.to_string()),
        actor_id: cell_opt_uuid(row, A_ACTOR_ID)?.map(|id| id.to_string()),
    };
    use pidash_services::app_issues::serializers_engage::app_issue_flat_to_representation;
    use pidash_services::app_issues::serializers_engage::AppIssueFlatRow;
    use pidash_services::app_project::ser_member::{
        project_lite_to_representation, workspace_lite_to_representation, ProjectLiteRow,
        WorkspaceLiteRow,
    };
    use pidash_services::app_project::ser_shared::{user_lite_to_representation, UserLiteRow};
    let actor_row = render.actor.as_ref().map(|row| UserLiteRow {
        id: &row.id,
        first_name: &row.first_name,
        last_name: &row.last_name,
        avatar: &row.avatar,
        avatar_url: row.avatar_url.as_deref(),
        is_bot: row.is_bot,
        display_name: &row.display_name,
    });
    let actor_detail = actor_row.as_ref().map(user_lite_to_representation);
    let issue_row = render.issue.as_ref().map(|row| AppIssueFlatRow {
        id: &row.id,
        name: &row.name,
        description_json: &row.description_json,
        description_html: row.description_html.as_deref().unwrap_or(""),
        priority: &row.priority,
        complexity_score: row.complexity_score,
        start_date: row.start_date.as_deref(),
        target_date: row.target_date.as_deref(),
        sequence_id: row.sequence_id,
        sort_order: row.sort_order,
        is_draft: row.is_draft,
    });
    let issue_detail = issue_row.as_ref().map(app_issue_flat_to_representation);
    let project_row = ProjectLiteRow {
        id: &render.project.id,
        identifier: &render.project.identifier,
        name: &render.project.name,
        cover_image: render.project.cover_image.as_deref(),
        cover_image_url: render.project.cover_image_url.as_deref(),
        logo_props: &render.project.logo_props,
        description: &render.project.description,
        is_default: render.project.is_default,
    };
    let project_detail = project_lite_to_representation(&project_row);
    let workspace_row = WorkspaceLiteRow {
        name: &render.workspace.name,
        slug: &render.workspace.slug,
        id: &render.workspace.id,
        logo_url: render.workspace.logo_url.as_deref(),
    };
    let workspace_detail = workspace_lite_to_representation(&workspace_row);
    let source_data = render.source.as_ref().map(|intake| ActivitySourceData {
        source: intake.source.as_deref(),
        source_email: intake.source_email.as_deref(),
        extra: &intake.extra,
    });
    let attachments: Vec<&str> = render.attachments.iter().map(String::as_str).collect();
    let input = IssueActivityRow {
        id: &render.id,
        actor_detail,
        issue_detail,
        project_detail,
        workspace_detail,
        source_data,
        created_at: &render.created_at,
        updated_at: &render.updated_at,
        deleted_at: render.deleted_at.as_deref(),
        verb: &render.verb,
        field: render.field.as_deref(),
        old_value: render.old_value.as_deref(),
        new_value: render.new_value.as_deref(),
        comment: &render.comment,
        attachments,
        old_identifier: render.old_identifier.as_deref(),
        new_identifier: render.new_identifier.as_deref(),
        epoch: render.epoch,
        created_by: render.created_by.as_deref(),
        updated_by: render.updated_by.as_deref(),
        project: &render.project_id,
        workspace: &render.workspace_id,
        issue: render.issue_id.as_deref(),
        issue_comment: render.issue_comment_id.as_deref(),
        actor: render.actor_id.as_deref(),
    };
    let view = issue_activity_to_representation(&input);
    serde_json::to_value(&view).map_err(|_| Denial::ServerError)
}

// ---------------------------------------------------------------------------
// Issue reactions: shapes + render
// ---------------------------------------------------------------------------

/// A decoded `issue_reactions` row.
#[derive(Debug, Clone)]
struct IssueReactionBaseOwned {
    id: uuid::Uuid,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
    deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    reaction: String,
    created_by: Option<uuid::Uuid>,
    updated_by: Option<uuid::Uuid>,
    project_id: uuid::Uuid,
    workspace_id: uuid::Uuid,
    issue_id: uuid::Uuid,
    actor_id: uuid::Uuid,
}

impl IssueReactionBaseOwned {
    fn from_map(map: &Map<String, Value>) -> Result<Self, Denial> {
        let uuid = |key: &str| -> Result<uuid::Uuid, Denial> {
            req_str(map, key)?
                .parse::<uuid::Uuid>()
                .map_err(|_| Denial::ServerError)
        };
        let opt_uuid = |key: &str| -> Result<Option<uuid::Uuid>, Denial> {
            opt_str(map, key)?
                .map(|raw| raw.parse::<uuid::Uuid>())
                .transpose()
                .map_err(|_| Denial::ServerError)
        };
        let dt = |key: &str| -> Result<chrono::DateTime<chrono::Utc>, Denial> {
            let raw = req_str(map, key)?;
            chrono::DateTime::parse_from_rfc3339(&raw)
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .map_err(|_| Denial::ServerError)
        };
        let opt_dt = |key: &str| -> Result<Option<chrono::DateTime<chrono::Utc>>, Denial> {
            opt_str(map, key)?
                .map(|raw| {
                    chrono::DateTime::parse_from_rfc3339(&raw)
                        .map(|dt| dt.with_timezone(&chrono::Utc))
                })
                .transpose()
                .map_err(|_| Denial::ServerError)
        };
        Ok(Self {
            id: uuid("id")?,
            created_at: dt("created_at")?,
            updated_at: dt("updated_at")?,
            deleted_at: opt_dt("deleted_at")?,
            reaction: req_str(map, "reaction")?,
            created_by: opt_uuid("created_by_id")?.or(opt_uuid("created_by")?),
            updated_by: opt_uuid("updated_by_id")?.or(opt_uuid("updated_by")?),
            project_id: uuid("project_id")?,
            workspace_id: uuid("workspace_id")?,
            issue_id: uuid("issue_id")?,
            actor_id: uuid("actor_id")?,
        })
    }
}

/// App `IssueReactionSerializer` (`issue.py:900-906`):
/// `Meta.fields = "__all__"` in live-DRF order (declared `id` +
/// `actor_detail`, concrete columns, forward relations in model order —
/// `actor` before `issue`, the declaration order at
/// `db/models/issue.py:727-731`).
#[derive(Debug, serde::Serialize)]
struct IssueReactionView {
    id: String,
    actor_detail: Value,
    created_at: String,
    updated_at: String,
    deleted_at: Option<String>,
    reaction: String,
    created_by: Option<String>,
    updated_by: Option<String>,
    project: String,
    workspace: String,
    actor: String,
    issue: String,
}

/// Render one issue reaction: the actor nest loads lazily (no
/// `select_related` on the list queryset).
async fn render_issue_reaction(
    pool: &sqlx::PgPool,
    base: &IssueReactionBaseOwned,
    tz: &Tz,
) -> Result<Value, Denial> {
    let actor = fetch_user_lite(pool, &base.actor_id).await?;
    let actor_detail = user_lite_value(&actor)?;
    let view = IssueReactionView {
        id: base.id.to_string(),
        actor_detail,
        created_at: render_dt(&base.created_at, tz),
        updated_at: render_dt(&base.updated_at, tz),
        deleted_at: render_dt_opt(base.deleted_at, tz),
        reaction: base.reaction.clone(),
        created_by: base.created_by.map(|id| id.to_string()),
        updated_by: base.updated_by.map(|id| id.to_string()),
        project: base.project_id.to_string(),
        workspace: base.workspace_id.to_string(),
        actor: base.actor_id.to_string(),
        issue: base.issue_id.to_string(),
    };
    serde_json::to_value(&view).map_err(|_| Denial::ServerError)
}

// ---------------------------------------------------------------------------
// Subscribers: shapes + render
// ---------------------------------------------------------------------------

/// A decoded `issue_subscribers` row.
#[derive(Debug, Clone)]
struct SubscriberOwned {
    id: uuid::Uuid,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
    deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    created_by: Option<uuid::Uuid>,
    updated_by: Option<uuid::Uuid>,
    project_id: uuid::Uuid,
    workspace_id: uuid::Uuid,
    issue_id: uuid::Uuid,
    subscriber_id: uuid::Uuid,
}

/// Render one subscriber through the services
/// `IssueSubscriberSerializer` port.
fn render_subscriber(base: &SubscriberOwned, tz: &Tz) -> Result<Value, Denial> {
    use pidash_services::app_issues::serializers_engage::{
        issue_subscriber_to_representation, IssueSubscriberRow,
    };
    let id = base.id.to_string();
    let created_at = render_dt(&base.created_at, tz);
    let updated_at = render_dt(&base.updated_at, tz);
    let deleted_at = render_dt_opt(base.deleted_at, tz);
    let created_by = base.created_by.map(|id| id.to_string());
    let updated_by = base.updated_by.map(|id| id.to_string());
    let project = base.project_id.to_string();
    let workspace = base.workspace_id.to_string();
    let issue = base.issue_id.to_string();
    let subscriber = base.subscriber_id.to_string();
    let input = IssueSubscriberRow {
        id: &id,
        created_at: &created_at,
        updated_at: &updated_at,
        deleted_at: deleted_at.as_deref(),
        created_by: created_by.as_deref(),
        updated_by: updated_by.as_deref(),
        project: &project,
        workspace: &workspace,
        issue: &issue,
        subscriber: &subscriber,
    };
    let view = issue_subscriber_to_representation(&input);
    serde_json::to_value(&view).map_err(|_| Denial::ServerError)
}

// ---------------------------------------------------------------------------
// History
// ---------------------------------------------------------------------------

/// `IssueActivityEndpoint.get` (`activity.py:30-86`).
async fn history(
    State(state): State<AppState>,
    Path((slug, project_raw, issue_raw)): Path<(String, String, String)>,
    Query(query): Query<super::QueryMap>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> Response {
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(error) => return error.into_response(),
    };
    let user_id = match actor_user_id(&pool, extension).await {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    let project_id = match resolve_project_id(&pool, &slug, &project_raw).await {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    let membership = match fetch_membership(&pool, &slug, &project_id, &user_id).await {
        Ok(membership) => membership,
        Err(error) => return error.into_response(),
    };
    // Class permission first, decorator second — Django's order.
    if let Err(error) = check_entity(&membership, true) {
        return error.into_response();
    }
    if let Err(error) = check_allow(&membership, &[20, 15, 5]) {
        return error.into_response();
    }
    let issue_id = match issue_raw.parse::<uuid::Uuid>() {
        Ok(id) => id,
        Err(_) => return crate::edge::proxy(State(state), req).await,
    };
    let tenant = match tenant_context(&pool, &user_id).await {
        Ok(tenant) => tenant,
        Err(error) => return error.into_response(),
    };
    // `?created_at__gt=` passthrough: unparseable input raises Django's
    // `ValidationError` into the invalid-detail 400. Valid input
    // normalizes to UTC (naive reads as UTC under `USE_TZ`), so the
    // text bind compares the same instant regardless of session TZ.
    let created_after = match super::query_last(&query, "created_at__gt") {
        None => None,
        Some(raw) => match parse_input_datetime(&raw) {
            Some(dt) => Some(dt.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true)),
            None => {
                return Denial::BadError("Please provide valid detail".to_owned()).into_response();
            }
        },
    };
    match history_inner(
        &pool,
        &slug,
        &issue_id,
        &user_id,
        &tenant.timezone,
        created_after.as_deref(),
        super::query_last(&query, "activity_type").as_deref(),
    )
    .await
    {
        Ok(body) => json_response(StatusCode::OK, body),
        Err(error) => error.into_response(),
    }
}

async fn history_inner(
    pool: &sqlx::PgPool,
    slug: &str,
    issue_id: &uuid::Uuid,
    user_id: &uuid::Uuid,
    tz: &Tz,
    created_after: Option<&str>,
    activity_type: Option<&str>,
) -> Result<String, Denial> {
    use super::queries_engage::{
        history_activities_sql, history_comments_sql, history_intake_prefetch_sql,
        history_reactions_prefetch_sql,
    };
    use super::Binder;
    use std::collections::HashMap;
    match activity_type {
        Some("issue-property") => {
            let mut binder = Binder::new();
            // NOTE: the builder's `created_after` binds TEXT, which
            // Postgres cannot compare to timestamptz (`operator does
            // not exist`); the predicate splices here with a cast.
            // Same rows Django's timestamptz bind returns.
            let mut sql = history_activities_sql(&mut binder, slug, *issue_id, *user_id, None);
            if let Some(ts) = created_after {
                let holder = binder.bind_string(ts.to_owned());
                sql = splice_and(
                    &sql,
                    &format!("issue_activities.created_at > {holder}::timestamptz"),
                );
            }
            let rows = fetch_positional_rows(pool, &sql, binder.values()).await?;
            // The `issue__issue_intake` prefetch: one issue, so the first
            // (newest) intake row serves every activity — or none.
            let mut intake: Option<IntakeOwned> = None;
            if !rows.is_empty() {
                let mut binder = Binder::new();
                let sql = history_intake_prefetch_sql(&mut binder, &[*issue_id]);
                let intake_rows = fetch_positional_rows(pool, &sql, binder.values()).await?;
                if let Some(row) = intake_rows.first() {
                    intake = Some(IntakeOwned {
                        source: cell_opt_string(row, 1)?,
                        source_email: cell_opt_string(row, 2)?,
                        extra: cell_json(row, 3)?,
                    });
                }
            }
            let mut out = Vec::with_capacity(rows.len());
            for row in &rows {
                // The prefetch list is shared, so clone the small triple
                // per row (same `source_data` object Django attaches).
                let intake = intake.as_ref().map(|owned| IntakeOwned {
                    source: owned.source.clone(),
                    source_email: owned.source_email.clone(),
                    extra: owned.extra.clone(),
                });
                out.push(render_history_activity(pool, row, tz, intake).await?);
            }
            serde_json::to_string(&out).map_err(|_| Denial::ServerError)
        }
        Some("issue-comment") => {
            let mut binder = Binder::new();
            // NOTE: as above — the builder's TEXT bind cannot compare
            // to timestamptz; splice with a cast instead.
            let mut sql = history_comments_sql(&mut binder, slug, *issue_id, *user_id, None);
            if let Some(ts) = created_after {
                let holder = binder.bind_string(ts.to_owned());
                sql = splice_and(
                    &sql,
                    &format!("issue_comments.created_at > {holder}::timestamptz"),
                );
            }
            let rows = fetch_positional_rows(pool, &sql, binder.values()).await?;
            // The `comment_reactions` prefetch: one IN query, grouped by
            // parent in row order.
            let mut grouped: HashMap<uuid::Uuid, Vec<CommentReactionView>> = HashMap::new();
            if !rows.is_empty() {
                let mut ids = Vec::with_capacity(rows.len());
                for row in &rows {
                    ids.push(cell_uuid(row, C_ID)?);
                }
                let mut binder = Binder::new();
                let sql = history_reactions_prefetch_sql(&mut binder, &ids);
                let prefetched = fetch_positional_rows(pool, &sql, binder.values()).await?;
                for row in &prefetched {
                    let view = render_prefetched_reaction(row, tz)?;
                    grouped
                        .entry(cell_uuid(row, R_COMMENT_ID)?)
                        .or_default()
                        .push(view);
                }
            }
            let mut out = Vec::with_capacity(rows.len());
            for row in &rows {
                let reactions = grouped.remove(&cell_uuid(row, C_ID)?).unwrap_or_default();
                out.push(render_history_comment(pool, row, tz, reactions).await?);
            }
            serde_json::to_string(&out).map_err(|_| Denial::ServerError)
        }
        _ => {
            // The default branch sorts RAW instances with
            // `instance["created_at"]`: a `TypeError` → generic 500 on the
            // first row — but an empty feed sorts to `[]` and answers
            // 200. Evaluation order is activities first, then comments.
            let mut binder = Binder::new();
            let mut sql = history_activities_sql(&mut binder, slug, *issue_id, *user_id, None);
            if let Some(ts) = created_after {
                let holder = binder.bind_string(ts.to_owned());
                sql = splice_and(
                    &sql,
                    &format!("issue_activities.created_at > {holder}::timestamptz"),
                );
            }
            let activities = fetch_positional_rows(pool, &sql, binder.values()).await?;
            if !activities.is_empty() {
                return Err(Denial::ServerError);
            }
            let mut binder = Binder::new();
            let mut sql = history_comments_sql(&mut binder, slug, *issue_id, *user_id, None);
            if let Some(ts) = created_after {
                let holder = binder.bind_string(ts.to_owned());
                sql = splice_and(
                    &sql,
                    &format!("issue_comments.created_at > {holder}::timestamptz"),
                );
            }
            let comments = fetch_positional_rows(pool, &sql, binder.values()).await?;
            if !comments.is_empty() {
                return Err(Denial::ServerError);
            }
            Ok("[]".to_owned())
        }
    }
}

/// Render one prefetched reaction: the actor nest comes from the joined
/// user columns (`select_related("actor")`); only `display_name` is
/// read, like the serializer's `source` traversal.
fn render_prefetched_reaction(
    row: &sqlx::postgres::PgRow,
    tz: &Tz,
) -> Result<CommentReactionView, Denial> {
    // The join is INNER (non-null FK), so the user columns are present;
    // `display_name` is read from them, not re-queried.
    let display_name = cell_string(row, PREFETCH_USER_AT + U_DISPLAY_NAME)?;
    Ok(CommentReactionView {
        id: cell_uuid(row, R_ID)?.to_string(),
        actor: cell_uuid(row, R_ACTOR_ID)?.to_string(),
        comment: cell_uuid(row, R_COMMENT_ID)?.to_string(),
        reaction: cell_string(row, R_REACTION)?,
        display_name,
        deleted_at: render_dt_opt(cell_opt_datetime(row, R_DELETED_AT)?, tz),
        workspace: cell_uuid(row, R_WORKSPACE_ID)?.to_string(),
        project: cell_uuid(row, R_PROJECT_ID)?.to_string(),
        created_at: render_dt(&cell_datetime(row, R_CREATED_AT)?, tz),
        updated_at: render_dt(&cell_datetime(row, R_UPDATED_AT)?, tz),
        created_by: cell_opt_uuid(row, R_CREATED_BY_ID)?.map(|id| id.to_string()),
        updated_by: cell_opt_uuid(row, R_UPDATED_BY_ID)?.map(|id| id.to_string()),
    })
}

// ---------------------------------------------------------------------------
// Comments: writes
// ---------------------------------------------------------------------------

/// Splice an AND predicate into a builder statement's WHERE (before the
/// trailing ORDER BY): the `filter_queryset` / `get_object` shape.
fn splice_and(sql: &str, predicate: &str) -> String {
    match sql.rfind("ORDER BY") {
        Some(idx) => format!("{} AND {} {}", &sql[..idx], predicate, &sql[idx..]),
        None => format!("{sql} WHERE {predicate}"),
    }
}

/// Insert a `descriptions` row: the comment-save side effect
/// (`db/models/issue.py:613-617`). Audit is the acting user
/// (`BaseModel.save` over crum); `stripped` arrives already resolved
/// (`None` when the html is empty — `Description.save` recomputes it).
#[allow(clippy::too_many_arguments)]
async fn insert_description(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    workspace_id: &uuid::Uuid,
    project_id: &uuid::Uuid,
    created_by: &uuid::Uuid,
    json: &Value,
    html: &str,
    stripped: Option<&str>,
    now: &chrono::DateTime<chrono::Utc>,
) -> Result<uuid::Uuid, Denial> {
    let id = uuid::Uuid::new_v4();
    sqlx::query(
        r#"INSERT INTO descriptions (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at,
            workspace_id, project_id, description_json, description_html, description_binary, description_stripped)
           VALUES ($1, $2, $3, $4, NULL, NULL, $5, $6, $7, $8, NULL, $9)"#,
    )
    .bind(id)
    .bind(now)
    .bind(now)
    .bind(created_by)
    .bind(workspace_id)
    .bind(project_id)
    .bind(json)
    .bind(html)
    .bind(stripped)
    .execute(&mut **tx)
    .await
    .map_err(integrity_denial)?;
    Ok(id)
}

/// Insert an `issue_comments` row: every concrete column, Django's
/// `objects.create` shape.
#[allow(clippy::too_many_arguments)]
async fn insert_comment(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    base: &CommentBaseOwned,
) -> Result<(), Denial> {
    sqlx::query(
        r#"INSERT INTO issue_comments (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at,
            project_id, workspace_id, comment_stripped, comment_json, comment_html, description_id,
            attachments, labels, issue_id, actor_id, access, external_source, external_id,
            speaker_type, speaker_label, speaker_agent_run_id, edited_at, parent_id)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21, $22, $23, $24)"#,
    )
    .bind(base.id)
    .bind(base.created_at)
    .bind(base.updated_at)
    .bind(base.created_by)
    .bind(base.updated_by)
    .bind(base.deleted_at)
    .bind(base.project_id)
    .bind(base.workspace_id)
    .bind(&base.stripped)
    .bind(&base.json)
    .bind(&base.html)
    .bind(base.description_id)
    .bind(&base.attachments)
    .bind(&base.labels)
    .bind(base.issue_id)
    .bind(base.actor_id)
    .bind(&base.access)
    .bind(base.external_source.as_deref())
    .bind(base.external_id.as_deref())
    .bind(&base.speaker_type)
    .bind(&base.speaker_label)
    .bind(base.speaker_run_id)
    .bind(base.edited_at)
    .bind(base.parent_id)
    .execute(&mut **tx)
    .await
    .map_err(integrity_denial)?;
    Ok(())
}

/// The `IssueComment.save()` update path (`issue.py:598-647`): recompute
/// `comment_stripped`, sync the `Description` triple for the changed
/// fields (or mint one when unlinked), stamp audit. Returns the saved
/// base for rendering.
#[allow(clippy::too_many_arguments)]
async fn save_comment_update(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    stored: &CommentBaseOwned,
    html: String,
    json: Value,
    attachments: Vec<String>,
    labels: Vec<String>,
    access: String,
    speaker_type: String,
    speaker_label: String,
    deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    description: Option<Option<uuid::Uuid>>,
    actor: Option<Option<uuid::Uuid>>,
    external_source: Option<Option<String>>,
    external_id: Option<Option<String>>,
    speaker_run: Option<Option<uuid::Uuid>>,
    edited_at: Option<Option<chrono::DateTime<chrono::Utc>>>,
    parent: Option<Option<uuid::Uuid>>,
    user_id: &uuid::Uuid,
    now: &chrono::DateTime<chrono::Utc>,
) -> Result<CommentBaseOwned, Denial> {
    // `save()` recomputes the stripped text unconditionally — validated
    // `comment_stripped` input never survives (it still validates, and
    // still trips the sync lock).
    let stripped = if html.is_empty() {
        String::new()
    } else {
        ml_strip_tags(&html)
    };
    let description_id = description.unwrap_or(stored.description_id);
    // `_changes_on_save`: the post-recompute triple vs the stored row
    // (Python `==` — cross-type numerics compare on the JSON leg).
    let html_changed = html != stored.html;
    let json_changed = !py_json_eq(&json, &stored.json);
    let stripped_changed = stripped != stored.stripped;
    let description_id = if description_id.is_none() {
        // Unlinked (or cleared): mint a fresh Description over the new
        // triple, `None`-when-empty stripped included.
        let stripped_for_desc = if html.is_empty() {
            None
        } else {
            Some(stripped.as_str())
        };
        Some(
            insert_description(
                tx,
                &stored.workspace_id,
                &stored.project_id,
                user_id,
                &json,
                &html,
                stripped_for_desc,
                now,
            )
            .await?,
        )
    } else {
        if html_changed || json_changed || stripped_changed {
            // Queryset `update` bypasses `Description.save`: the values
            // land verbatim (no `None`-when-empty recompute).
            sqlx::query(
                r#"UPDATE descriptions SET description_html = $1, description_stripped = $2,
                   description_json = $3, updated_by_id = $4, updated_at = $5 WHERE id = $6"#,
            )
            .bind(&html)
            .bind(&stripped)
            .bind(&json)
            .bind(user_id)
            .bind(now)
            .bind(description_id)
            .execute(&mut **tx)
            .await
            .map_err(|_| Denial::ServerError)?;
        }
        description_id
    };
    let base = CommentBaseOwned {
        id: stored.id,
        created_at: stored.created_at,
        updated_at: *now,
        created_by: stored.created_by,
        updated_by: Some(*user_id),
        deleted_at,
        project_id: stored.project_id,
        workspace_id: stored.workspace_id,
        stripped,
        json,
        html,
        description_id,
        attachments,
        labels,
        issue_id: stored.issue_id,
        actor_id: actor.unwrap_or(stored.actor_id),
        access,
        external_source: external_source.unwrap_or_else(|| stored.external_source.clone()),
        external_id: external_id.unwrap_or_else(|| stored.external_id.clone()),
        speaker_type,
        speaker_label,
        speaker_run_id: speaker_run.unwrap_or(stored.speaker_run_id),
        edited_at: edited_at.unwrap_or(stored.edited_at),
        parent_id: parent.unwrap_or(stored.parent_id),
    };
    sqlx::query(
        r#"UPDATE issue_comments SET updated_at = $1, updated_by_id = $2, deleted_at = $3,
           comment_stripped = $4, comment_json = $5, comment_html = $6, description_id = $7,
           attachments = $8, labels = $9, actor_id = $10, access = $11,
           external_source = $12, external_id = $13, speaker_type = $14, speaker_label = $15,
           speaker_agent_run_id = $16, edited_at = $17, parent_id = $18
           WHERE id = $19"#,
    )
    .bind(base.updated_at)
    .bind(base.updated_by)
    .bind(base.deleted_at)
    .bind(&base.stripped)
    .bind(&base.json)
    .bind(&base.html)
    .bind(base.description_id)
    .bind(&base.attachments)
    .bind(&base.labels)
    .bind(base.actor_id)
    .bind(&base.access)
    .bind(base.external_source.as_deref())
    .bind(base.external_id.as_deref())
    .bind(&base.speaker_type)
    .bind(&base.speaker_label)
    .bind(base.speaker_run_id)
    .bind(base.edited_at)
    .bind(base.parent_id)
    .bind(base.id)
    .execute(&mut **tx)
    .await
    .map_err(integrity_denial)?;
    Ok(base)
}

// ---------------------------------------------------------------------------
// Comments: handlers
// ---------------------------------------------------------------------------

/// Shared preamble for the read paths (list / retrieve): session
/// auth, project rewrite, tenant. No membership gate — scoping lives
/// in the queryset.
struct ReadContext {
    pool: sqlx::PgPool,
    slug: String,
    project_id: uuid::Uuid,
    issue_id: uuid::Uuid,
    user_id: uuid::Uuid,
    timezone: Tz,
}

/// Parse the common `(slug, project, issue)` preamble: session auth,
/// project rewrite, tenant. Callers parse their UUID path segments
/// first and proxy garbage ones (Django's `<uuid:>` converter would
/// never route them) before calling.
async fn read_context(
    state: &AppState,
    slug: &str,
    project_raw: &str,
    issue_id: uuid::Uuid,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Result<ReadContext, Denial> {
    let pool = pool_of(state)?;
    let user_id = actor_user_id(&pool, extension).await?;
    let project_id = resolve_project_id(&pool, slug, project_raw).await?;
    let tenant = tenant_context(&pool, &user_id).await?;
    Ok(ReadContext {
        pool,
        slug: slug.to_owned(),
        project_id,
        issue_id,
        user_id,
        timezone: tenant.timezone,
    })
}

/// `IssueCommentViewSet.list` (`comment.py:43-69`): DRF's default over
/// the annotated queryset, bare array, newest first.
async fn comment_list(
    State(state): State<AppState>,
    Path((slug, project_raw, issue_raw)): Path<(String, String, String)>,
    Query(query): Query<super::QueryMap>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> Response {
    let issue_id = match issue_raw.parse::<uuid::Uuid>() {
        Ok(id) => id,
        Err(_) => return crate::edge::proxy(State(state), req).await,
    };
    let ctx = match read_context(&state, &slug, &project_raw, issue_id, extension).await {
        Ok(ctx) => ctx,
        Err(error) => return error.into_response(),
    };
    // `filterset_fields = ["issue__id", "workspace__id"]`: exact UUID
    // matches; garbage answers the invalid-detail 400.
    let mut filters: Vec<(String, uuid::Uuid)> = Vec::new();
    for (param, column) in [
        ("issue__id", "issue_comments.issue_id"),
        ("workspace__id", "issue_comments.workspace_id"),
    ] {
        if let Some(raw) = super::query_last(&query, param) {
            match raw.parse::<uuid::Uuid>() {
                Ok(id) => filters.push((column.to_owned(), id)),
                Err(_) => {
                    return Denial::BadError("Please provide valid detail".to_owned())
                        .into_response();
                }
            }
        }
    }
    // The path issue always scopes; the filterset only narrows further.
    let mut binder = super::Binder::new();
    let mut sql = super::queries_engage::comment_list_sql(
        &mut binder,
        &ctx.slug,
        ctx.project_id,
        ctx.issue_id,
        ctx.user_id,
    );
    for (column, id) in &filters {
        let holder = binder.bind_uuid(*id);
        sql = splice_and(&sql, &format!("{column} = {holder}"));
    }
    let rows = match fetch_positional_rows(&ctx.pool, &sql, binder.values()).await {
        Ok(rows) => rows,
        Err(error) => return error.into_response(),
    };
    let mut out = Vec::with_capacity(rows.len());
    for row in &rows {
        match render_list_comment(&ctx.pool, row, &ctx.timezone).await {
            Ok(value) => out.push(value),
            Err(error) => return error.into_response(),
        }
    }
    match serde_json::to_string(&out) {
        Ok(body) => json_response(StatusCode::OK, body),
        Err(_) => Denial::ServerError.into_response(),
    }
}

/// `IssueCommentViewSet.retrieve`: `get_object` over the list queryset;
/// a miss is DRF's `Http404` detail body.
async fn comment_retrieve(
    State(state): State<AppState>,
    Path((slug, project_raw, issue_raw, pk_raw)): Path<(String, String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> Response {
    let (issue_id, pk) = match (
        issue_raw.parse::<uuid::Uuid>(),
        pk_raw.parse::<uuid::Uuid>(),
    ) {
        (Ok(issue_id), Ok(pk)) => (issue_id, pk),
        _ => return crate::edge::proxy(State(state), req).await,
    };
    let ctx = match read_context(&state, &slug, &project_raw, issue_id, extension).await {
        Ok(ctx) => ctx,
        Err(error) => return error.into_response(),
    };
    let mut binder = super::Binder::new();
    let mut sql = super::queries_engage::comment_list_sql(
        &mut binder,
        &ctx.slug,
        ctx.project_id,
        ctx.issue_id,
        ctx.user_id,
    );
    let holder = binder.bind_uuid(pk);
    sql = splice_and(&sql, &format!("issue_comments.id = {holder}"));
    let rows = match fetch_positional_rows(&ctx.pool, &sql, binder.values()).await {
        Ok(rows) => rows,
        Err(error) => return error.into_response(),
    };
    let Some(row) = rows.first() else {
        return Denial::NotFoundDetail.into_response();
    };
    match render_list_comment(&ctx.pool, row, &ctx.timezone).await {
        Ok(value) => match serde_json::to_string(&value) {
            Ok(body) => json_response(StatusCode::OK, body),
            Err(_) => Denial::ServerError.into_response(),
        },
        Err(error) => error.into_response(),
    }
}

/// `IssueCommentViewSet.create` (`comment.py:71-115`).
async fn comment_create(
    State(state): State<AppState>,
    Path((slug, project_raw, issue_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> Response {
    let issue_id = match issue_raw.parse::<uuid::Uuid>() {
        Ok(id) => id,
        Err(_) => return crate::edge::proxy(State(state), req).await,
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(error) => return error.into_response(),
    };
    let user_id = match actor_user_id(&pool, extension).await {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    let project_id = match resolve_project_id(&pool, &slug, &project_raw).await {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    let membership = match fetch_membership(&pool, &slug, &project_id, &user_id).await {
        Ok(membership) => membership,
        Err(error) => return error.into_response(),
    };
    if let Err(error) = check_allow(&membership, &[20, 15, 5]) {
        return error.into_response();
    }
    // `Project.objects.get` / `Issue.objects.get`: default managers
    // (soft-deleted rows 404), unscoped by tenant (the gate scoped).
    let project: Option<(uuid::Uuid, bool)> = match sqlx::query_as(
        "SELECT workspace_id, guest_view_all_features FROM projects WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(project_id)
    .fetch_optional(&pool)
    .await
    {
        Ok(row) => row,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let Some((workspace_id, guest_view_all)) = project else {
        return Denial::NotFound.into_response();
    };
    let issue: Option<(Option<uuid::Uuid>,)> = match sqlx::query_as(
        "SELECT created_by_id FROM issues WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(issue_id)
    .fetch_optional(&pool)
    .await
    {
        Ok(row) => row,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let Some((issue_created_by,)) = issue else {
        return Denial::NotFound.into_response();
    };
    // The guest-view rule refuses with 400, not 403.
    if membership.project_role == Some(5) && !guest_view_all && issue_created_by != Some(user_id) {
        return Denial::BadError("You are not allowed to comment on the issue".to_owned())
            .into_response();
    }
    let tenant = match tenant_context(&pool, &user_id).await {
        Ok(tenant) => tenant,
        Err(error) => return error.into_response(),
    };
    let request_body = match read_body(req).await {
        Ok(request_body) => request_body,
        Err(error) => return error.into_response(),
    };
    let RequestBody { map: body, is_html } = request_body;
    let attrs = match validate_comment_input(&pool, &body, is_html).await {
        Ok(attrs) => attrs,
        Err(error) => return error.into_response(),
    };
    // Full-create defaults for the missing defaulted fields; input
    // `description` / `actor` / `comment_stripped` never survive the
    // save (overwritten below), but they validated above.
    let html = attrs.html.unwrap_or_else(|| "<p></p>".to_owned());
    let json = attrs.json.unwrap_or(Value::Object(Map::new()));
    let stripped = if html.is_empty() {
        String::new()
    } else {
        ml_strip_tags(&html)
    };
    let now = chrono::Utc::now();
    let mut base = CommentBaseOwned {
        id: uuid::Uuid::new_v4(),
        created_at: now,
        updated_at: now,
        created_by: Some(user_id),
        updated_by: None,
        deleted_at: attrs.deleted_at.unwrap_or(None),
        project_id,
        workspace_id,
        stripped,
        json,
        html,
        description_id: None,
        attachments: attrs.attachments.unwrap_or_default(),
        labels: attrs.labels.unwrap_or_default(),
        issue_id,
        actor_id: Some(user_id),
        access: attrs.access.unwrap_or_else(|| "INTERNAL".to_owned()),
        external_source: attrs.external_source.unwrap_or(None),
        external_id: attrs.external_id.unwrap_or(None),
        speaker_type: attrs.speaker_type.unwrap_or_else(|| "human".to_owned()),
        speaker_label: attrs.speaker_label.unwrap_or_default(),
        speaker_run_id: attrs.speaker_run.unwrap_or(None),
        edited_at: attrs.edited_at.unwrap_or(None),
        parent_id: attrs.parent.unwrap_or(None),
    };
    // Django's order: comment insert, description insert, link update —
    // in one transaction (`IssueComment.save`).
    let mut tx = match pool.begin().await {
        Ok(tx) => tx,
        Err(_) => return Denial::ServerError.into_response(),
    };
    if let Err(error) = insert_comment(&mut tx, &base).await {
        return error.into_response();
    }
    let stripped_for_desc = if base.html.is_empty() {
        None
    } else {
        Some(base.stripped.as_str())
    };
    let description_id = match insert_description(
        &mut tx,
        &workspace_id,
        &project_id,
        &user_id,
        &base.json,
        &base.html,
        stripped_for_desc,
        &now,
    )
    .await
    {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    let now2 = chrono::Utc::now();
    if sqlx::query("UPDATE issue_comments SET description_id = $1, updated_at = $2 WHERE id = $3")
        .bind(description_id)
        .bind(now2)
        .bind(base.id)
        .execute(&mut *tx)
        .await
        .is_err()
    {
        return Denial::ServerError.into_response();
    }
    if tx.commit().await.is_err() {
        return Denial::ServerError.into_response();
    }
    // In-memory post-save state: the link update stamps `updated_at`
    // and (in memory only — `update_fields` does not persist it)
    // `updated_by`.
    base.description_id = Some(description_id);
    base.updated_at = now2;
    base.updated_by = Some(user_id);
    let rendered = match render_base_comment(&pool, &base, &tenant.timezone, None).await {
        Ok(value) => value,
        Err(error) => return error.into_response(),
    };
    let epoch = chrono::Utc::now().timestamp();
    let origin = app_origin(&state);
    let mut activity = Map::new();
    activity.insert(
        "type".to_owned(),
        Value::String("comment.activity.created".to_owned()),
    );
    activity.insert(
        "requested_data".to_owned(),
        Value::String(python_dumps(&rendered)),
    );
    activity.insert("actor_id".to_owned(), Value::String(user_id.to_string()));
    activity.insert("issue_id".to_owned(), Value::String(issue_id.to_string()));
    activity.insert(
        "project_id".to_owned(),
        Value::String(project_id.to_string()),
    );
    activity.insert("current_instance".to_owned(), Value::Null);
    activity.insert("epoch".to_owned(), Value::Number(epoch.into()));
    activity.insert("notification".to_owned(), Value::Bool(true));
    activity.insert("origin".to_owned(), Value::String(origin.clone()));
    enqueue_kwargs(&pool, ISSUE_ACTIVITY_TASK, activity).await;
    let mut model = Map::new();
    model.insert(
        "model_name".to_owned(),
        Value::String("issue_comment".to_owned()),
    );
    model.insert("model_id".to_owned(), Value::String(base.id.to_string()));
    model.insert("requested_data".to_owned(), Value::Object(body));
    model.insert("current_instance".to_owned(), Value::Null);
    model.insert("actor_id".to_owned(), Value::String(user_id.to_string()));
    model.insert("slug".to_owned(), Value::String(slug));
    model.insert("origin".to_owned(), Value::String(origin));
    enqueue_kwargs(&pool, MODEL_ACTIVITY_TASK, model).await;
    match serde_json::to_string(&rendered) {
        Ok(text) => json_response(StatusCode::CREATED, text),
        Err(_) => Denial::ServerError.into_response(),
    }
}

/// The scoped `.get()` behind `partial_update` / `destroy`
/// (`comment.py:128-130,165-167`): default manager (soft-deleted rows
/// miss), tenant-scoped, pk included.
async fn fetch_scoped_comment(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    issue_id: &uuid::Uuid,
    pk: &uuid::Uuid,
) -> Result<Option<CommentBaseOwned>, Denial> {
    let row = fetch_optional_object(
        pool,
        "SELECT issue_comments.* FROM issue_comments INNER JOIN workspaces ON (issue_comments.workspace_id = workspaces.id) WHERE (issue_comments.deleted_at IS NULL AND workspaces.slug = $1 AND issue_comments.project_id = $2 AND issue_comments.issue_id = $3 AND issue_comments.id = $4) LIMIT 1",
        &[
            SqlParam::Text(slug),
            SqlParam::Uuid(*project_id),
            SqlParam::Uuid(*issue_id),
            SqlParam::Uuid(*pk),
        ],
    )
    .await?;
    row.map(|value| CommentBaseOwned::from_map(obj(&value)?))
        .transpose()
}

/// `SoftDeleteModel.delete(soft=True)` (`db/mixins.py:57-78`): stamp
/// `deleted_at` (+ `updated_at`/`updated_by` through the full `save()`),
/// row stays. `table` is a static per-call-site literal.
async fn soft_delete_row(
    pool: &sqlx::PgPool,
    table: &str,
    pk: &uuid::Uuid,
    user_id: &uuid::Uuid,
    now: &chrono::DateTime<chrono::Utc>,
) -> Result<(), Denial> {
    let sql = format!(
        "UPDATE {table} SET deleted_at = $1, updated_at = $1, updated_by_id = $2 WHERE id = $3"
    );
    sqlx::query(&sql)
        .bind(now)
        .bind(user_id)
        .bind(pk)
        .execute(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(())
}

/// DRF's default `update` (`comment.py:184-195` maps PUT with no view
/// override): `get_object` (annotated queryset + filterset), full
/// validation with model defaults for missing defaulted fields, plain
/// save — no decorator gate (class `IsAuthenticated` only), no
/// enqueues, no `edited_at` logic.
async fn comment_update(
    State(state): State<AppState>,
    Path((slug, project_raw, issue_raw, pk_raw)): Path<(String, String, String, String)>,
    Query(query): Query<super::QueryMap>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> Response {
    let (issue_id, pk) = match (
        issue_raw.parse::<uuid::Uuid>(),
        pk_raw.parse::<uuid::Uuid>(),
    ) {
        (Ok(issue_id), Ok(pk)) => (issue_id, pk),
        _ => return crate::edge::proxy(State(state), req).await,
    };
    let ctx = match read_context(&state, &slug, &project_raw, issue_id, extension).await {
        Ok(ctx) => ctx,
        Err(error) => return error.into_response(),
    };
    let mut binder = super::Binder::new();
    let mut sql = super::queries_engage::comment_list_sql(
        &mut binder,
        &ctx.slug,
        ctx.project_id,
        ctx.issue_id,
        ctx.user_id,
    );
    for (param, column) in [
        ("issue__id", "issue_comments.issue_id"),
        ("workspace__id", "issue_comments.workspace_id"),
    ] {
        if let Some(raw) = super::query_last(&query, param) {
            match raw.parse::<uuid::Uuid>() {
                Ok(id) => {
                    let holder = binder.bind_uuid(id);
                    sql = splice_and(&sql, &format!("{column} = {holder}"));
                }
                Err(_) => {
                    return Denial::BadError("Please provide valid detail".to_owned())
                        .into_response();
                }
            }
        }
    }
    let holder = binder.bind_uuid(pk);
    sql = splice_and(&sql, &format!("issue_comments.id = {holder}"));
    let rows = match fetch_positional_rows(&ctx.pool, &sql, binder.values()).await {
        Ok(rows) => rows,
        Err(error) => return error.into_response(),
    };
    let Some(row) = rows.first() else {
        return Denial::NotFoundDetail.into_response();
    };
    let stored = match CommentBaseOwned::from_positional(row) {
        Ok(base) => base,
        Err(error) => return error.into_response(),
    };
    let is_member = match cell_bool(row, LIST_MEMBER_AT) {
        Ok(flag) => flag,
        Err(error) => return error.into_response(),
    };
    let request_body = match read_body(req).await {
        Ok(request_body) => request_body,
        Err(error) => return error.into_response(),
    };
    let RequestBody { map: body, is_html } = request_body;
    let attrs = match validate_comment_input(&ctx.pool, &body, is_html).await {
        Ok(attrs) => attrs,
        Err(error) => return error.into_response(),
    };
    // The sync-lock `validate()` runs on updates (synced instances).
    match comment_is_synced(&ctx.pool, stored.external_source.as_deref(), &stored.id).await {
        Ok(true) => {
            let lock = comment_sync_lock_errors(&stored, &attrs);
            if !lock.is_empty() {
                return Denial::BadJson(Value::Object(lock)).into_response();
            }
        }
        Ok(false) => {}
        Err(error) => return error.into_response(),
    }
    // Full update: missing defaulted fields take their model defaults;
    // missing non-defaulted fields stay untouched.
    let now = chrono::Utc::now();
    let mut tx = match ctx.pool.begin().await {
        Ok(tx) => tx,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let base = match save_comment_update(
        &mut tx,
        &stored,
        attrs.html.unwrap_or_else(|| "<p></p>".to_owned()),
        attrs.json.unwrap_or(Value::Object(Map::new())),
        attrs.attachments.unwrap_or_default(),
        attrs.labels.unwrap_or_default(),
        attrs.access.unwrap_or_else(|| "INTERNAL".to_owned()),
        attrs.speaker_type.unwrap_or_else(|| "human".to_owned()),
        attrs.speaker_label.unwrap_or_default(),
        attrs.deleted_at.unwrap_or(stored.deleted_at),
        attrs.description,
        attrs.actor,
        attrs.external_source,
        attrs.external_id,
        attrs.speaker_run,
        attrs.edited_at,
        attrs.parent,
        &ctx.user_id,
        &now,
    )
    .await
    {
        Ok(base) => base,
        Err(error) => return error.into_response(),
    };
    if tx.commit().await.is_err() {
        return Denial::ServerError.into_response();
    }
    // No enqueues on the default `update` — and the annotation from
    // `get_object` survives, so `is_member` renders.
    match render_base_comment(&ctx.pool, &base, &ctx.timezone, Some(is_member)).await {
        Ok(value) => match serde_json::to_string(&value) {
            Ok(text) => json_response(StatusCode::OK, text),
            Err(_) => Denial::ServerError.into_response(),
        },
        Err(error) => error.into_response(),
    }
}

/// `IssueCommentViewSet.partial_update` (`comment.py:118-155`): ADMIN +
/// creator gate, conditional `edited_at`, both enqueues.
async fn comment_partial_update(
    State(state): State<AppState>,
    Path((slug, project_raw, issue_raw, pk_raw)): Path<(String, String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> Response {
    let (issue_id, pk) = match (
        issue_raw.parse::<uuid::Uuid>(),
        pk_raw.parse::<uuid::Uuid>(),
    ) {
        (Ok(issue_id), Ok(pk)) => (issue_id, pk),
        _ => return crate::edge::proxy(State(state), req).await,
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(error) => return error.into_response(),
    };
    let user_id = match actor_user_id(&pool, extension).await {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    let project_id = match resolve_project_id(&pool, &slug, &project_raw).await {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    if let Err(error) =
        check_admin_creator(&pool, &slug, &project_id, &user_id, "issue_comments", &pk).await
    {
        return error.into_response();
    }
    let stored = match fetch_scoped_comment(&pool, &slug, &project_id, &issue_id, &pk).await {
        Ok(Some(base)) => base,
        Ok(None) => return Denial::NotFound.into_response(),
        Err(error) => return error.into_response(),
    };
    let tenant = match tenant_context(&pool, &user_id).await {
        Ok(tenant) => tenant,
        Err(error) => return error.into_response(),
    };
    let request_body = match read_body(req).await {
        Ok(request_body) => request_body,
        Err(error) => return error.into_response(),
    };
    let RequestBody { map: body, is_html } = request_body;
    let requested_data = python_dumps(&Value::Object(body.clone()));
    let current_rendered = match render_base_comment(&pool, &stored, &tenant.timezone, None).await {
        Ok(value) => value,
        Err(error) => return error.into_response(),
    };
    let current_instance = python_dumps(&current_rendered);
    let attrs = match validate_comment_input(&pool, &body, is_html).await {
        Ok(attrs) => attrs,
        Err(error) => return error.into_response(),
    };
    match comment_is_synced(&pool, stored.external_source.as_deref(), &stored.id).await {
        Ok(true) => {
            let lock = comment_sync_lock_errors(&stored, &attrs);
            if !lock.is_empty() {
                return Denial::BadJson(Value::Object(lock)).into_response();
            }
        }
        Ok(false) => {}
        Err(error) => return error.into_response(),
    }
    // `edited_at` stamps only when raw `comment_html` arrived AND
    // differs (`comment.py:139-142`) — overriding any input value.
    let edited_at = match &attrs.html {
        Some(html) if *html != stored.html => Some(Some(chrono::Utc::now())),
        _ => attrs.edited_at,
    };
    // Partial: missing fields stay untouched (no defaults).
    let now = chrono::Utc::now();
    let mut tx = match pool.begin().await {
        Ok(tx) => tx,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let base = match save_comment_update(
        &mut tx,
        &stored,
        attrs.html.unwrap_or_else(|| stored.html.clone()),
        attrs.json.unwrap_or_else(|| stored.json.clone()),
        attrs
            .attachments
            .unwrap_or_else(|| stored.attachments.clone()),
        attrs.labels.unwrap_or_else(|| stored.labels.clone()),
        attrs.access.unwrap_or_else(|| stored.access.clone()),
        attrs
            .speaker_type
            .unwrap_or_else(|| stored.speaker_type.clone()),
        attrs
            .speaker_label
            .unwrap_or_else(|| stored.speaker_label.clone()),
        attrs.deleted_at.unwrap_or(stored.deleted_at),
        attrs.description,
        attrs.actor,
        attrs.external_source,
        attrs.external_id,
        attrs.speaker_run,
        edited_at,
        attrs.parent,
        &user_id,
        &now,
    )
    .await
    {
        Ok(base) => base,
        Err(error) => return error.into_response(),
    };
    if tx.commit().await.is_err() {
        return Denial::ServerError.into_response();
    }
    let epoch = chrono::Utc::now().timestamp();
    let origin = app_origin(&state);
    let mut activity = Map::new();
    activity.insert(
        "type".to_owned(),
        Value::String("comment.activity.updated".to_owned()),
    );
    activity.insert(
        "requested_data".to_owned(),
        Value::String(requested_data.clone()),
    );
    activity.insert("actor_id".to_owned(), Value::String(user_id.to_string()));
    activity.insert("issue_id".to_owned(), Value::String(issue_id.to_string()));
    activity.insert(
        "project_id".to_owned(),
        Value::String(project_id.to_string()),
    );
    activity.insert(
        "current_instance".to_owned(),
        Value::String(current_instance.clone()),
    );
    activity.insert("epoch".to_owned(), Value::Number(epoch.into()));
    activity.insert("notification".to_owned(), Value::Bool(true));
    activity.insert("origin".to_owned(), Value::String(origin.clone()));
    enqueue_kwargs(&pool, ISSUE_ACTIVITY_TASK, activity).await;
    let mut model = Map::new();
    model.insert(
        "model_name".to_owned(),
        Value::String("issue_comment".to_owned()),
    );
    model.insert("model_id".to_owned(), Value::String(pk.to_string()));
    model.insert("requested_data".to_owned(), Value::Object(body));
    model.insert(
        "current_instance".to_owned(),
        Value::String(current_instance),
    );
    model.insert("actor_id".to_owned(), Value::String(user_id.to_string()));
    model.insert("slug".to_owned(), Value::String(slug));
    model.insert("origin".to_owned(), Value::String(origin));
    enqueue_kwargs(&pool, MODEL_ACTIVITY_TASK, model).await;
    match render_base_comment(&pool, &base, &tenant.timezone, None).await {
        Ok(value) => match serde_json::to_string(&value) {
            Ok(text) => json_response(StatusCode::OK, text),
            Err(_) => Denial::ServerError.into_response(),
        },
        Err(error) => error.into_response(),
    }
}

/// `IssueCommentViewSet.destroy` (`comment.py:157-184`): ADMIN +
/// creator gate, sync 409, soft delete, `issue_activity` only.
async fn comment_destroy(
    State(state): State<AppState>,
    Path((slug, project_raw, issue_raw, pk_raw)): Path<(String, String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> Response {
    let (issue_id, pk) = match (
        issue_raw.parse::<uuid::Uuid>(),
        pk_raw.parse::<uuid::Uuid>(),
    ) {
        (Ok(issue_id), Ok(pk)) => (issue_id, pk),
        _ => return crate::edge::proxy(State(state), req).await,
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(error) => return error.into_response(),
    };
    let user_id = match actor_user_id(&pool, extension).await {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    let project_id = match resolve_project_id(&pool, &slug, &project_raw).await {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    if let Err(error) =
        check_admin_creator(&pool, &slug, &project_id, &user_id, "issue_comments", &pk).await
    {
        return error.into_response();
    }
    let stored = match fetch_scoped_comment(&pool, &slug, &project_id, &issue_id, &pk).await {
        Ok(Some(base)) => base,
        Ok(None) => return Denial::NotFound.into_response(),
        Err(error) => return error.into_response(),
    };
    // The request body is never read on this path (not even for
    // errors); drop `req` without parsing.
    let _ = req;
    let tenant = match tenant_context(&pool, &user_id).await {
        Ok(tenant) => tenant,
        Err(error) => return error.into_response(),
    };
    match comment_has_sync_row(&pool, &stored.id).await {
        Ok(true) => {
            return json_response(
                StatusCode::CONFLICT,
                r#"{"error":"This comment is synced from a Git provider. Unbind the project's repository to delete."}"#.to_owned(),
            );
        }
        Ok(false) => {}
        Err(error) => return error.into_response(),
    }
    let current_rendered = match render_base_comment(&pool, &stored, &tenant.timezone, None).await {
        Ok(value) => value,
        Err(error) => return error.into_response(),
    };
    let now = chrono::Utc::now();
    if let Err(error) = soft_delete_row(&pool, "issue_comments", &stored.id, &user_id, &now).await {
        return error.into_response();
    }
    let mut requested = Map::new();
    requested.insert("comment_id".to_owned(), Value::String(pk.to_string()));
    let epoch = chrono::Utc::now().timestamp();
    let mut activity = Map::new();
    activity.insert(
        "type".to_owned(),
        Value::String("comment.activity.deleted".to_owned()),
    );
    activity.insert(
        "requested_data".to_owned(),
        Value::String(python_dumps(&Value::Object(requested))),
    );
    activity.insert("actor_id".to_owned(), Value::String(user_id.to_string()));
    activity.insert("issue_id".to_owned(), Value::String(issue_id.to_string()));
    activity.insert(
        "project_id".to_owned(),
        Value::String(project_id.to_string()),
    );
    activity.insert(
        "current_instance".to_owned(),
        Value::String(python_dumps(&current_rendered)),
    );
    activity.insert("epoch".to_owned(), Value::Number(epoch.into()));
    activity.insert("notification".to_owned(), Value::Bool(true));
    activity.insert("origin".to_owned(), Value::String(app_origin(&state)));
    // Order mirrors `comment.py:174-185`: `issue_comment.delete()` (which
    // fires `soft_delete_related_objects` from `db/mixins.py:77`) runs
    // BEFORE the `issue_activity.delay` — the reverse of the reaction
    // destroys, where the delay precedes `.delete()`.
    enqueue_soft_delete(&pool, "issuecomment", &stored.id).await;
    enqueue_kwargs(&pool, ISSUE_ACTIVITY_TASK, activity).await;
    empty_response(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// Reactions
// ---------------------------------------------------------------------------

/// Required text input: missing → "This field is required." (other
/// failures already pushed their message).
fn require_text(
    errors: &mut Map<String, Value>,
    body: &Map<String, Value>,
    field: &str,
    allow_blank: bool,
) -> Option<String> {
    match check_text(errors, body, field, None, false, allow_blank) {
        Presence::Value(value) => Some(value),
        _ => {
            if !errors.contains_key(field) {
                push_error(errors, field, "This field is required.".to_owned());
            }
            None
        }
    }
}

/// The lazy forward-FK project read behind reaction / subscriber /
/// subscribe saves: `_base_manager` semantics — soft-deleted rows
/// count, only a truly absent row 404s.
async fn fetch_project_workspace_unguarded(
    pool: &sqlx::PgPool,
    project_id: &uuid::Uuid,
) -> Result<uuid::Uuid, Denial> {
    let row: Option<(uuid::Uuid,)> =
        sqlx::query_as("SELECT workspace_id FROM projects WHERE id = $1")
            .bind(project_id)
            .fetch_optional(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    row.map(|row| row.0).ok_or(Denial::NotFound)
}

/// A decoded `comment_reactions` row (single-table maps).
#[derive(Debug, Clone)]
struct CommentReactionBaseOwned {
    id: uuid::Uuid,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
    deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    created_by: Option<uuid::Uuid>,
    updated_by: Option<uuid::Uuid>,
    project_id: uuid::Uuid,
    workspace_id: uuid::Uuid,
    comment_id: uuid::Uuid,
    actor_id: uuid::Uuid,
    reaction: String,
}

/// Render one comment reaction with its `display_name` leaf.
async fn render_comment_reaction(
    pool: &sqlx::PgPool,
    base: &CommentReactionBaseOwned,
    tz: &Tz,
) -> Result<Value, Denial> {
    let display_name = fetch_display_name(pool, &base.actor_id).await?;
    let view = CommentReactionView {
        id: base.id.to_string(),
        actor: base.actor_id.to_string(),
        comment: base.comment_id.to_string(),
        reaction: base.reaction.clone(),
        display_name,
        deleted_at: render_dt_opt(base.deleted_at, tz),
        workspace: base.workspace_id.to_string(),
        project: base.project_id.to_string(),
        created_at: render_dt(&base.created_at, tz),
        updated_at: render_dt(&base.updated_at, tz),
        created_by: base.created_by.map(|id| id.to_string()),
        updated_by: base.updated_by.map(|id| id.to_string()),
    };
    serde_json::to_value(&view).map_err(|_| Denial::ServerError)
}

/// `CommentReactionViewSet.list` (`comment.py:186-200`): DRF's default,
/// bare array, newest first. No gate beyond `IsAuthenticated`.
async fn comment_reaction_list(
    State(state): State<AppState>,
    Path((slug, project_raw, comment_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> Response {
    let comment_id = match comment_raw.parse::<uuid::Uuid>() {
        Ok(id) => id,
        Err(_) => return crate::edge::proxy(State(state), req).await,
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(error) => return error.into_response(),
    };
    let user_id = match actor_user_id(&pool, extension).await {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    let project_id = match resolve_project_id(&pool, &slug, &project_raw).await {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    let tenant = match tenant_context(&pool, &user_id).await {
        Ok(tenant) => tenant,
        Err(error) => return error.into_response(),
    };
    let mut binder = super::Binder::new();
    let sql = super::queries_engage::comment_reaction_list_sql(
        &mut binder,
        &slug,
        project_id,
        comment_id,
        user_id,
    );
    let rows = match fetch_positional_rows(&pool, &sql, binder.values()).await {
        Ok(rows) => rows,
        Err(error) => return error.into_response(),
    };
    let mut out = Vec::with_capacity(rows.len());
    for row in &rows {
        let base = match comment_reaction_from_positional(row) {
            Ok(base) => base,
            Err(error) => return error.into_response(),
        };
        match render_comment_reaction(&pool, &base, &tenant.timezone).await {
            Ok(value) => out.push(value),
            Err(error) => return error.into_response(),
        }
    }
    match serde_json::to_string(&out) {
        Ok(body) => json_response(StatusCode::OK, body),
        Err(_) => Denial::ServerError.into_response(),
    }
}

/// Decode a `COMMENT_REACTION_COLUMNS` positional row at offset 0.
fn comment_reaction_from_positional(
    row: &sqlx::postgres::PgRow,
) -> Result<CommentReactionBaseOwned, Denial> {
    Ok(CommentReactionBaseOwned {
        id: cell_uuid(row, R_ID)?,
        created_at: cell_datetime(row, R_CREATED_AT)?,
        updated_at: cell_datetime(row, R_UPDATED_AT)?,
        deleted_at: cell_opt_datetime(row, R_DELETED_AT)?,
        created_by: cell_opt_uuid(row, R_CREATED_BY_ID)?,
        updated_by: cell_opt_uuid(row, R_UPDATED_BY_ID)?,
        project_id: cell_uuid(row, R_PROJECT_ID)?,
        workspace_id: cell_uuid(row, R_WORKSPACE_ID)?,
        comment_id: cell_uuid(row, R_COMMENT_ID)?,
        actor_id: cell_uuid(row, R_ACTOR_ID)?,
        reaction: cell_string(row, R_REACTION)?,
    })
}

/// `CommentReactionViewSet.create` (`comment.py:202-212`): the only
/// writable field is `reaction`; any integrity failure (duplicate OR
/// dangling comment) answers the specific message.
async fn comment_reaction_create(
    State(state): State<AppState>,
    Path((slug, project_raw, comment_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> Response {
    let comment_id = match comment_raw.parse::<uuid::Uuid>() {
        Ok(id) => id,
        Err(_) => return crate::edge::proxy(State(state), req).await,
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(error) => return error.into_response(),
    };
    let user_id = match actor_user_id(&pool, extension).await {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    let project_id = match resolve_project_id(&pool, &slug, &project_raw).await {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    let membership = match fetch_membership(&pool, &slug, &project_id, &user_id).await {
        Ok(membership) => membership,
        Err(error) => return error.into_response(),
    };
    if let Err(error) = check_allow(&membership, &[20, 15, 5]) {
        return error.into_response();
    }
    let tenant = match tenant_context(&pool, &user_id).await {
        Ok(tenant) => tenant,
        Err(error) => return error.into_response(),
    };
    let request_body = match read_body(req).await {
        Ok(request_body) => request_body,
        Err(error) => return error.into_response(),
    };
    let RequestBody { map: body, is_html } = request_body;
    let mut errors = Map::new();
    // Text-only validation: `is_html` is irrelevant (no choice, FK or
    // file-sensitive field reads it here).
    let _ = is_html;
    let reaction = require_text(&mut errors, &body, "reaction", false);
    if !errors.is_empty() {
        return Denial::BadJson(Value::Object(errors)).into_response();
    }
    let reaction = match reaction {
        Some(reaction) => reaction,
        None => return Denial::ServerError.into_response(),
    };
    let workspace_id = match fetch_project_workspace_unguarded(&pool, &project_id).await {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    let now = chrono::Utc::now();
    let base = CommentReactionBaseOwned {
        id: uuid::Uuid::new_v4(),
        created_at: now,
        updated_at: now,
        deleted_at: None,
        created_by: Some(user_id),
        updated_by: None,
        project_id,
        workspace_id,
        comment_id,
        actor_id: user_id,
        reaction,
    };
    if let Err(error) = sqlx::query(
        r#"INSERT INTO comment_reactions (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at,
            project_id, workspace_id, actor_id, comment_id, reaction)
           VALUES ($1, $2, $3, $4, NULL, NULL, $5, $6, $7, $8, $9)"#,
    )
    .bind(base.id)
    .bind(base.created_at)
    .bind(base.updated_at)
    .bind(base.created_by)
    .bind(base.project_id)
    .bind(base.workspace_id)
    .bind(base.actor_id)
    .bind(base.comment_id)
    .bind(&base.reaction)
    .execute(&pool)
    .await
    {
        if is_integrity_error(&error) {
            return Denial::BadError("Reaction already exists for the user".to_owned()).into_response();
        }
        let _ = error;
        return Denial::ServerError.into_response();
    }
    let epoch = chrono::Utc::now().timestamp();
    let mut activity = Map::new();
    activity.insert(
        "type".to_owned(),
        Value::String("comment_reaction.activity.created".to_owned()),
    );
    activity.insert(
        "requested_data".to_owned(),
        Value::String(python_dumps(&Value::Object(body))),
    );
    activity.insert("actor_id".to_owned(), Value::String(user_id.to_string()));
    activity.insert("issue_id".to_owned(), Value::Null);
    activity.insert(
        "project_id".to_owned(),
        Value::String(project_id.to_string()),
    );
    activity.insert("current_instance".to_owned(), Value::Null);
    activity.insert("epoch".to_owned(), Value::Number(epoch.into()));
    activity.insert("notification".to_owned(), Value::Bool(true));
    activity.insert("origin".to_owned(), Value::String(app_origin(&state)));
    enqueue_kwargs(&pool, ISSUE_ACTIVITY_TASK, activity).await;
    match render_comment_reaction(&pool, &base, &tenant.timezone).await {
        Ok(value) => match serde_json::to_string(&value) {
            Ok(text) => json_response(StatusCode::CREATED, text),
            Err(_) => Denial::ServerError.into_response(),
        },
        Err(error) => error.into_response(),
    }
}

/// `CommentReactionViewSet.destroy` (`comment.py:230-254`).
async fn comment_reaction_destroy(
    State(state): State<AppState>,
    Path((slug, project_raw, comment_raw, reaction_code)): Path<(String, String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> Response {
    let comment_id = match comment_raw.parse::<uuid::Uuid>() {
        Ok(id) => id,
        Err(_) => return crate::edge::proxy(State(state), req).await,
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(error) => return error.into_response(),
    };
    let user_id = match actor_user_id(&pool, extension).await {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    let project_id = match resolve_project_id(&pool, &slug, &project_raw).await {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    let membership = match fetch_membership(&pool, &slug, &project_id, &user_id).await {
        Ok(membership) => membership,
        Err(error) => return error.into_response(),
    };
    if let Err(error) = check_allow(&membership, &[20, 15, 5]) {
        return error.into_response();
    }
    // The body is never read here; drop `req` without parsing.
    let _ = req;
    let row = match fetch_optional_object(
        &pool,
        "SELECT comment_reactions.* FROM comment_reactions INNER JOIN workspaces ON (comment_reactions.workspace_id = workspaces.id) WHERE (comment_reactions.deleted_at IS NULL AND workspaces.slug = $1 AND comment_reactions.project_id = $2 AND comment_reactions.comment_id = $3 AND comment_reactions.reaction = $4 AND comment_reactions.actor_id = $5) LIMIT 1",
        &[
            SqlParam::Text(&slug),
            SqlParam::Uuid(project_id),
            SqlParam::Uuid(comment_id),
            SqlParam::Text(&reaction_code),
            SqlParam::Uuid(user_id),
        ],
    )
    .await
    {
        Ok(row) => row,
        Err(error) => return error.into_response(),
    };
    let Some(row) = row else {
        return Denial::NotFound.into_response();
    };
    let stored_id = match obj(&row).and_then(|map| req_str(map, "id")) {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    let stored_uuid = match stored_id.parse::<uuid::Uuid>() {
        Ok(id) => id,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let now = chrono::Utc::now();
    if let Err(error) =
        soft_delete_row(&pool, "comment_reactions", &stored_uuid, &user_id, &now).await
    {
        return error.into_response();
    }
    let mut current = Map::new();
    current.insert("reaction".to_owned(), Value::String(reaction_code));
    current.insert("identifier".to_owned(), Value::String(stored_id));
    current.insert(
        "comment_id".to_owned(),
        Value::String(comment_id.to_string()),
    );
    let epoch = chrono::Utc::now().timestamp();
    let mut activity = Map::new();
    activity.insert(
        "type".to_owned(),
        Value::String("comment_reaction.activity.deleted".to_owned()),
    );
    activity.insert("requested_data".to_owned(), Value::Null);
    activity.insert("actor_id".to_owned(), Value::String(user_id.to_string()));
    activity.insert("issue_id".to_owned(), Value::Null);
    activity.insert(
        "project_id".to_owned(),
        Value::String(project_id.to_string()),
    );
    activity.insert(
        "current_instance".to_owned(),
        Value::String(python_dumps(&Value::Object(current))),
    );
    activity.insert("epoch".to_owned(), Value::Number(epoch.into()));
    activity.insert("notification".to_owned(), Value::Bool(true));
    activity.insert("origin".to_owned(), Value::String(app_origin(&state)));
    enqueue_kwargs(&pool, ISSUE_ACTIVITY_TASK, activity).await;
    enqueue_soft_delete(&pool, "commentreaction", &stored_uuid).await;
    empty_response(StatusCode::NO_CONTENT)
}

/// `IssueReactionViewSet.list` (`reaction.py:26-40`): DRF's default
/// over the scoped queryset, bare array, newest first. No gate beyond
/// `IsAuthenticated`. (No queries-layer builder covers this queryset —
/// the statement below follows the same Django shape.)
async fn issue_reaction_list(
    State(state): State<AppState>,
    Path((slug, project_raw, issue_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> Response {
    let issue_id = match issue_raw.parse::<uuid::Uuid>() {
        Ok(id) => id,
        Err(_) => return crate::edge::proxy(State(state), req).await,
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(error) => return error.into_response(),
    };
    let user_id = match actor_user_id(&pool, extension).await {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    let project_id = match resolve_project_id(&pool, &slug, &project_raw).await {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    let tenant = match tenant_context(&pool, &user_id).await {
        Ok(tenant) => tenant,
        Err(error) => return error.into_response(),
    };
    let rows = match fetch_all_objects(
        &pool,
        "SELECT DISTINCT issue_reactions.* FROM issue_reactions INNER JOIN workspaces ON (issue_reactions.workspace_id = workspaces.id) INNER JOIN projects ON (issue_reactions.project_id = projects.id) INNER JOIN project_members ON (issue_reactions.project_id = project_members.project_id) WHERE (issue_reactions.deleted_at IS NULL AND workspaces.slug = $1 AND issue_reactions.project_id = $2 AND issue_reactions.issue_id = $3 AND projects.archived_at IS NULL AND project_members.member_id = $4 AND project_members.is_active) ORDER BY issue_reactions.created_at DESC",
        &[
            SqlParam::Text(&slug),
            SqlParam::Uuid(project_id),
            SqlParam::Uuid(issue_id),
            SqlParam::Uuid(user_id),
        ],
    )
    .await
    {
        Ok(rows) => rows,
        Err(error) => return error.into_response(),
    };
    let mut out = Vec::with_capacity(rows.len());
    for row in &rows {
        let base = match obj(row).and_then(IssueReactionBaseOwned::from_map) {
            Ok(base) => base,
            Err(error) => return error.into_response(),
        };
        match render_issue_reaction(&pool, &base, &tenant.timezone).await {
            Ok(value) => out.push(value),
            Err(error) => return error.into_response(),
        }
    }
    match serde_json::to_string(&out) {
        Ok(body) => json_response(StatusCode::OK, body),
        Err(_) => Denial::ServerError.into_response(),
    }
}

/// `IssueReactionViewSet.create` (`reaction.py:42-62`): `reaction` is
/// the only meaningful input (`created_by` / `updated_by` validate but
/// lose to the save audit). Duplicates surface the GENERIC
/// `IntegrityError` body — the view catches nothing.
async fn issue_reaction_create(
    State(state): State<AppState>,
    Path((slug, project_raw, issue_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> Response {
    let issue_id = match issue_raw.parse::<uuid::Uuid>() {
        Ok(id) => id,
        Err(_) => return crate::edge::proxy(State(state), req).await,
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(error) => return error.into_response(),
    };
    let user_id = match actor_user_id(&pool, extension).await {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    let project_id = match resolve_project_id(&pool, &slug, &project_raw).await {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    let membership = match fetch_membership(&pool, &slug, &project_id, &user_id).await {
        Ok(membership) => membership,
        Err(error) => return error.into_response(),
    };
    if let Err(error) = check_allow(&membership, &[20, 15, 5]) {
        return error.into_response();
    }
    let tenant = match tenant_context(&pool, &user_id).await {
        Ok(tenant) => tenant,
        Err(error) => return error.into_response(),
    };
    let request_body = match read_body(req).await {
        Ok(request_body) => request_body,
        Err(error) => return error.into_response(),
    };
    let RequestBody { map: body, is_html } = request_body;
    let mut errors = Map::new();
    let reaction = require_text(&mut errors, &body, "reaction", false);
    // Writable-but-ignored audit FKs: garbage UUIDs are per-field
    // curly errors, missing users are field errors.
    if let Err(error) = check_fk(
        &pool,
        &mut errors,
        &body,
        "created_by",
        "users",
        true,
        is_html,
    )
    .await
    {
        return error.into_response();
    }
    if let Err(error) = check_fk(
        &pool,
        &mut errors,
        &body,
        "updated_by",
        "users",
        true,
        is_html,
    )
    .await
    {
        return error.into_response();
    }
    if !errors.is_empty() {
        return Denial::BadJson(Value::Object(errors)).into_response();
    }
    let reaction = match reaction {
        Some(reaction) => reaction,
        None => return Denial::ServerError.into_response(),
    };
    let workspace_id = match fetch_project_workspace_unguarded(&pool, &project_id).await {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    let now = chrono::Utc::now();
    let base = IssueReactionBaseOwned {
        id: uuid::Uuid::new_v4(),
        created_at: now,
        updated_at: now,
        deleted_at: None,
        reaction,
        created_by: Some(user_id),
        updated_by: None,
        project_id,
        workspace_id,
        issue_id,
        actor_id: user_id,
    };
    if let Err(error) = sqlx::query(
        r#"INSERT INTO issue_reactions (id, created_at, updated_at, deleted_at, reaction,
            created_by_id, updated_by_id, project_id, workspace_id, issue_id, actor_id)
           VALUES ($1, $2, $3, NULL, $4, $5, NULL, $6, $7, $8, $9)"#,
    )
    .bind(base.id)
    .bind(base.created_at)
    .bind(base.updated_at)
    .bind(&base.reaction)
    .bind(base.created_by)
    .bind(base.project_id)
    .bind(base.workspace_id)
    .bind(base.issue_id)
    .bind(base.actor_id)
    .execute(&pool)
    .await
    {
        return integrity_denial(error).into_response();
    }
    let epoch = chrono::Utc::now().timestamp();
    let mut activity = Map::new();
    activity.insert(
        "type".to_owned(),
        Value::String("issue_reaction.activity.created".to_owned()),
    );
    activity.insert(
        "requested_data".to_owned(),
        Value::String(python_dumps(&Value::Object(body))),
    );
    activity.insert("actor_id".to_owned(), Value::String(user_id.to_string()));
    activity.insert("issue_id".to_owned(), Value::String(issue_id.to_string()));
    activity.insert(
        "project_id".to_owned(),
        Value::String(project_id.to_string()),
    );
    activity.insert("current_instance".to_owned(), Value::Null);
    activity.insert("epoch".to_owned(), Value::Number(epoch.into()));
    activity.insert("notification".to_owned(), Value::Bool(true));
    activity.insert("origin".to_owned(), Value::String(app_origin(&state)));
    enqueue_kwargs(&pool, ISSUE_ACTIVITY_TASK, activity).await;
    match render_issue_reaction(&pool, &base, &tenant.timezone).await {
        Ok(value) => match serde_json::to_string(&value) {
            Ok(text) => json_response(StatusCode::CREATED, text),
            Err(_) => Denial::ServerError.into_response(),
        },
        Err(error) => error.into_response(),
    }
}

/// `IssueReactionViewSet.destroy` (`reaction.py:64-85`).
async fn issue_reaction_destroy(
    State(state): State<AppState>,
    Path((slug, project_raw, issue_raw, reaction_code)): Path<(String, String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> Response {
    let issue_id = match issue_raw.parse::<uuid::Uuid>() {
        Ok(id) => id,
        Err(_) => return crate::edge::proxy(State(state), req).await,
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(error) => return error.into_response(),
    };
    let user_id = match actor_user_id(&pool, extension).await {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    let project_id = match resolve_project_id(&pool, &slug, &project_raw).await {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    let membership = match fetch_membership(&pool, &slug, &project_id, &user_id).await {
        Ok(membership) => membership,
        Err(error) => return error.into_response(),
    };
    if let Err(error) = check_allow(&membership, &[20, 15, 5]) {
        return error.into_response();
    }
    let _ = req;
    let row = match fetch_optional_object(
        &pool,
        "SELECT issue_reactions.* FROM issue_reactions INNER JOIN workspaces ON (issue_reactions.workspace_id = workspaces.id) WHERE (issue_reactions.deleted_at IS NULL AND workspaces.slug = $1 AND issue_reactions.project_id = $2 AND issue_reactions.issue_id = $3 AND issue_reactions.reaction = $4 AND issue_reactions.actor_id = $5) LIMIT 1",
        &[
            SqlParam::Text(&slug),
            SqlParam::Uuid(project_id),
            SqlParam::Uuid(issue_id),
            SqlParam::Text(&reaction_code),
            SqlParam::Uuid(user_id),
        ],
    )
    .await
    {
        Ok(row) => row,
        Err(error) => return error.into_response(),
    };
    let Some(row) = row else {
        return Denial::NotFound.into_response();
    };
    let stored_id = match obj(&row).and_then(|map| req_str(map, "id")) {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    let stored_uuid = match stored_id.parse::<uuid::Uuid>() {
        Ok(id) => id,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let now = chrono::Utc::now();
    if let Err(error) =
        soft_delete_row(&pool, "issue_reactions", &stored_uuid, &user_id, &now).await
    {
        return error.into_response();
    }
    let mut current = Map::new();
    current.insert("reaction".to_owned(), Value::String(reaction_code));
    current.insert("identifier".to_owned(), Value::String(stored_id));
    let epoch = chrono::Utc::now().timestamp();
    let mut activity = Map::new();
    activity.insert(
        "type".to_owned(),
        Value::String("issue_reaction.activity.deleted".to_owned()),
    );
    activity.insert("requested_data".to_owned(), Value::Null);
    activity.insert("actor_id".to_owned(), Value::String(user_id.to_string()));
    activity.insert("issue_id".to_owned(), Value::String(issue_id.to_string()));
    activity.insert(
        "project_id".to_owned(),
        Value::String(project_id.to_string()),
    );
    activity.insert(
        "current_instance".to_owned(),
        Value::String(python_dumps(&Value::Object(current))),
    );
    activity.insert("epoch".to_owned(), Value::Number(epoch.into()));
    activity.insert("notification".to_owned(), Value::Bool(true));
    activity.insert("origin".to_owned(), Value::String(app_origin(&state)));
    enqueue_kwargs(&pool, ISSUE_ACTIVITY_TASK, activity).await;
    enqueue_soft_delete(&pool, "issuereaction", &stored_uuid).await;
    empty_response(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// Subscribers
// ---------------------------------------------------------------------------

/// Required FK input: missing → "This field is required." (garbage
/// UUIDs are per-field curly errors out of `check_fk`).
async fn require_fk(
    pool: &sqlx::PgPool,
    errors: &mut Map<String, Value>,
    body: &Map<String, Value>,
    field: &str,
    table: &str,
    is_html: bool,
) -> Result<Option<uuid::Uuid>, Denial> {
    match check_fk(pool, errors, body, field, table, false, is_html).await? {
        Presence::Value(id) => Ok(Some(id)),
        _ => {
            if !errors.contains_key(field) {
                push_error(errors, field, "This field is required.".to_owned());
            }
            Ok(None)
        }
    }
}

/// `IssueSubscriberViewSet.list` (`subscriber.py:52-57`): the ACTIVE
/// project-member roster (default-manager guard + `is_active`),
/// newest first, each through `ProjectMemberLiteSerializer` — with NO
/// `is_subscribed` annotation, so the key stays absent. The issue
/// itself is never read.
async fn subscriber_list(
    State(state): State<AppState>,
    Path((slug, project_raw, issue_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> Response {
    if issue_raw.parse::<uuid::Uuid>().is_err() {
        return crate::edge::proxy(State(state), req).await;
    }
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(error) => return error.into_response(),
    };
    let user_id = match actor_user_id(&pool, extension).await {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    let project_id = match resolve_project_id(&pool, &slug, &project_raw).await {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    let membership = match fetch_membership(&pool, &slug, &project_id, &user_id).await {
        Ok(membership) => membership,
        Err(error) => return error.into_response(),
    };
    if let Err(error) = check_entity(&membership, true) {
        return error.into_response();
    }
    let rows = match sqlx::query(
        "SELECT pm.id, u.id, u.first_name, u.last_name, u.avatar, u.avatar_asset_id, u.is_bot, u.display_name FROM project_members AS pm INNER JOIN workspaces AS w ON (pm.workspace_id = w.id) LEFT OUTER JOIN users AS u ON (pm.member_id = u.id) WHERE (pm.deleted_at IS NULL AND w.slug = $1 AND pm.project_id = $2 AND pm.is_active) ORDER BY pm.created_at DESC",
    )
    .bind(&slug)
    .bind(project_id)
    .fetch_all(&pool)
    .await
    {
        Ok(rows) => rows,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let mut out = Vec::with_capacity(rows.len());
    for row in &rows {
        let id: uuid::Uuid = match row.try_get(0) {
            Ok(id) => id,
            Err(_) => return Denial::ServerError.into_response(),
        };
        let member_id: Option<uuid::Uuid> = match row.try_get(1) {
            Ok(id) => id,
            Err(_) => return Denial::ServerError.into_response(),
        };
        let member = match member_id {
            None => None,
            Some(member_id) => {
                let avatar: String = match row.try_get(4) {
                    Ok(avatar) => avatar,
                    Err(_) => return Denial::ServerError.into_response(),
                };
                let asset_id: Option<uuid::Uuid> = match row.try_get(5) {
                    Ok(id) => id,
                    Err(_) => return Denial::ServerError.into_response(),
                };
                let avatar_url =
                    match logo_or_cover_url(&pool, Some(avatar.clone()), asset_id).await {
                        Ok(url) => url,
                        Err(error) => return error.into_response(),
                    };
                let first_name: String = match row.try_get(2) {
                    Ok(name) => name,
                    Err(_) => return Denial::ServerError.into_response(),
                };
                let last_name: String = match row.try_get(3) {
                    Ok(name) => name,
                    Err(_) => return Denial::ServerError.into_response(),
                };
                let is_bot: bool = match row.try_get(6) {
                    Ok(flag) => flag,
                    Err(_) => return Denial::ServerError.into_response(),
                };
                let display_name: String = match row.try_get(7) {
                    Ok(name) => name,
                    Err(_) => return Denial::ServerError.into_response(),
                };
                Some(UserLiteOwned {
                    id: member_id.to_string(),
                    first_name,
                    last_name,
                    avatar,
                    avatar_url,
                    is_bot,
                    display_name,
                })
            }
        };
        let id_string = id.to_string();
        let member_row =
            member.as_ref().map(
                |row| pidash_services::app_project::ser_shared::UserLiteRow {
                    id: &row.id,
                    first_name: &row.first_name,
                    last_name: &row.last_name,
                    avatar: &row.avatar,
                    avatar_url: row.avatar_url.as_deref(),
                    is_bot: row.is_bot,
                    display_name: &row.display_name,
                },
            );
        let input = pidash_services::app_project::ser_shared::ProjectMemberLiteRow {
            id: &id_string,
            member: member_row,
            is_subscribed: None,
        };
        let view =
            pidash_services::app_project::ser_shared::project_member_lite_to_representation(&input);
        match serde_json::to_value(&view) {
            Ok(value) => out.push(value),
            Err(_) => return Denial::ServerError.into_response(),
        }
    }
    match serde_json::to_string(&out) {
        Ok(body) => json_response(StatusCode::OK, body),
        Err(_) => Denial::ServerError.into_response(),
    }
}

/// `IssueSubscriberViewSet.create`: DRF's default
/// (`subscriber.py:16-51`). `deleted_at` is REQUIRED input (the
/// `unique_together` uniqueness extras), `subscriber` is a required
/// user FK, audit inputs validate-then-lose to the save.
async fn subscriber_create(
    State(state): State<AppState>,
    Path((slug, project_raw, issue_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> Response {
    let issue_id = match issue_raw.parse::<uuid::Uuid>() {
        Ok(id) => id,
        Err(_) => return crate::edge::proxy(State(state), req).await,
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(error) => return error.into_response(),
    };
    let user_id = match actor_user_id(&pool, extension).await {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    let project_id = match resolve_project_id(&pool, &slug, &project_raw).await {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    let membership = match fetch_membership(&pool, &slug, &project_id, &user_id).await {
        Ok(membership) => membership,
        Err(error) => return error.into_response(),
    };
    if let Err(error) = check_entity(&membership, false) {
        return error.into_response();
    }
    let tenant = match tenant_context(&pool, &user_id).await {
        Ok(tenant) => tenant,
        Err(error) => return error.into_response(),
    };
    let request_body = match read_body(req).await {
        Ok(request_body) => request_body,
        Err(error) => return error.into_response(),
    };
    let RequestBody { map: body, is_html } = request_body;
    let mut errors = Map::new();
    let subscriber =
        match require_fk(&pool, &mut errors, &body, "subscriber", "users", is_html).await {
            Ok(subscriber) => subscriber,
            Err(error) => return error.into_response(),
        };
    let deleted_at = match check_datetime(&mut errors, &body, "deleted_at", true) {
        Presence::Missing => {
            if !errors.contains_key("deleted_at") {
                push_error(
                    &mut errors,
                    "deleted_at",
                    "This field is required.".to_owned(),
                );
            }
            None
        }
        Presence::Null => Some(None),
        Presence::Value(dt) => Some(Some(dt)),
    };
    if let Err(error) = check_fk(
        &pool,
        &mut errors,
        &body,
        "created_by",
        "users",
        true,
        is_html,
    )
    .await
    {
        return error.into_response();
    }
    if let Err(error) = check_fk(
        &pool,
        &mut errors,
        &body,
        "updated_by",
        "users",
        true,
        is_html,
    )
    .await
    {
        return error.into_response();
    }
    if !errors.is_empty() {
        return Denial::BadJson(Value::Object(errors)).into_response();
    }
    let (Some(subscriber_id), Some(deleted_at)) = (subscriber, deleted_at) else {
        return Denial::ServerError.into_response();
    };
    let workspace_id = match fetch_project_workspace_unguarded(&pool, &project_id).await {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    let now = chrono::Utc::now();
    let base = SubscriberOwned {
        id: uuid::Uuid::new_v4(),
        created_at: now,
        updated_at: now,
        deleted_at,
        created_by: Some(user_id),
        updated_by: None,
        project_id,
        workspace_id,
        issue_id,
        subscriber_id,
    };
    if let Err(error) = sqlx::query(
        r#"INSERT INTO issue_subscribers (id, created_at, updated_at, deleted_at, created_by_id, updated_by_id,
            project_id, workspace_id, issue_id, subscriber_id)
           VALUES ($1, $2, $3, $4, $5, NULL, $6, $7, $8, $9)"#,
    )
    .bind(base.id)
    .bind(base.created_at)
    .bind(base.updated_at)
    .bind(base.deleted_at)
    .bind(base.created_by)
    .bind(base.project_id)
    .bind(base.workspace_id)
    .bind(base.issue_id)
    .bind(base.subscriber_id)
    .execute(&pool)
    .await
    {
        return integrity_denial(error).into_response();
    }
    match render_subscriber(&base, &tenant.timezone) {
        Ok(value) => match serde_json::to_string(&value) {
            Ok(text) => json_response(StatusCode::CREATED, text),
            Err(_) => Denial::ServerError.into_response(),
        },
        Err(error) => error.into_response(),
    }
}

/// The subscription-row `.get()` behind `destroy` / `unsubscribe`:
/// default manager, tenant-scoped, keyed by subscriber user id.
async fn fetch_subscription(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    issue_id: &uuid::Uuid,
    subscriber_id: &uuid::Uuid,
) -> Result<Option<uuid::Uuid>, Denial> {
    let row: Option<(uuid::Uuid,)> = sqlx::query_as(
        "SELECT issue_subscribers.id FROM issue_subscribers INNER JOIN workspaces ON (issue_subscribers.workspace_id = workspaces.id) WHERE (issue_subscribers.deleted_at IS NULL AND workspaces.slug = $1 AND issue_subscribers.project_id = $2 AND issue_subscribers.issue_id = $3 AND issue_subscribers.subscriber_id = $4) LIMIT 1",
    )
    .bind(slug)
    .bind(project_id)
    .bind(issue_id)
    .bind(subscriber_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(row.map(|row| row.0))
}

/// `IssueSubscriberViewSet.destroy` (`subscriber.py:59-67`): the kwarg
/// is the subscriber's USER id.
async fn subscriber_destroy(
    State(state): State<AppState>,
    Path((slug, project_raw, issue_raw, subscriber_raw)): Path<(String, String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> Response {
    let (issue_id, subscriber_id) = match (
        issue_raw.parse::<uuid::Uuid>(),
        subscriber_raw.parse::<uuid::Uuid>(),
    ) {
        (Ok(issue_id), Ok(subscriber_id)) => (issue_id, subscriber_id),
        _ => return crate::edge::proxy(State(state), req).await,
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(error) => return error.into_response(),
    };
    let user_id = match actor_user_id(&pool, extension).await {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    let project_id = match resolve_project_id(&pool, &slug, &project_raw).await {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    let membership = match fetch_membership(&pool, &slug, &project_id, &user_id).await {
        Ok(membership) => membership,
        Err(error) => return error.into_response(),
    };
    if let Err(error) = check_entity(&membership, false) {
        return error.into_response();
    }
    let _ = req;
    let pk = match fetch_subscription(&pool, &slug, &project_id, &issue_id, &subscriber_id).await {
        Ok(Some(pk)) => pk,
        Ok(None) => return Denial::NotFound.into_response(),
        Err(error) => return error.into_response(),
    };
    let now = chrono::Utc::now();
    if let Err(error) = soft_delete_row(&pool, "issue_subscribers", &pk, &user_id, &now).await {
        return error.into_response();
    }
    enqueue_soft_delete(&pool, "issuesubscriber", &pk).await;
    empty_response(StatusCode::NO_CONTENT)
}

/// `subscribe` (`subscriber.py:70-84`): lite gate, duplicate 400,
/// `workspace` backfilled from the project at save.
async fn subscribe(
    State(state): State<AppState>,
    Path((slug, project_raw, issue_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> Response {
    let issue_id = match issue_raw.parse::<uuid::Uuid>() {
        Ok(id) => id,
        Err(_) => return crate::edge::proxy(State(state), req).await,
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(error) => return error.into_response(),
    };
    let user_id = match actor_user_id(&pool, extension).await {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    let project_id = match resolve_project_id(&pool, &slug, &project_raw).await {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    let membership = match fetch_membership(&pool, &slug, &project_id, &user_id).await {
        Ok(membership) => membership,
        Err(error) => return error.into_response(),
    };
    if let Err(error) = check_lite(&membership) {
        return error.into_response();
    }
    // The body is never read here; drop `req` without parsing.
    let _ = req;
    let tenant = match tenant_context(&pool, &user_id).await {
        Ok(tenant) => tenant,
        Err(error) => return error.into_response(),
    };
    match fetch_subscription(&pool, &slug, &project_id, &issue_id, &user_id).await {
        Ok(Some(_)) => {
            return Denial::BadMessage("User already subscribed to the issue.".to_owned())
                .into_response();
        }
        Ok(None) => {}
        Err(error) => return error.into_response(),
    }
    let workspace_id = match fetch_project_workspace_unguarded(&pool, &project_id).await {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    let now = chrono::Utc::now();
    let base = SubscriberOwned {
        id: uuid::Uuid::new_v4(),
        created_at: now,
        updated_at: now,
        deleted_at: None,
        created_by: Some(user_id),
        updated_by: None,
        project_id,
        workspace_id,
        issue_id,
        subscriber_id: user_id,
    };
    if let Err(error) = sqlx::query(
        r#"INSERT INTO issue_subscribers (id, created_at, updated_at, deleted_at, created_by_id, updated_by_id,
            project_id, workspace_id, issue_id, subscriber_id)
           VALUES ($1, $2, $3, NULL, $4, NULL, $5, $6, $7, $8)"#,
    )
    .bind(base.id)
    .bind(base.created_at)
    .bind(base.updated_at)
    .bind(base.created_by)
    .bind(base.project_id)
    .bind(base.workspace_id)
    .bind(base.issue_id)
    .bind(base.subscriber_id)
    .execute(&pool)
    .await
    {
        return integrity_denial(error).into_response();
    }
    match render_subscriber(&base, &tenant.timezone) {
        Ok(value) => match serde_json::to_string(&value) {
            Ok(text) => json_response(StatusCode::CREATED, text),
            Err(_) => Denial::ServerError.into_response(),
        },
        Err(error) => error.into_response(),
    }
}

/// `unsubscribe` (`subscriber.py:87-95`).
async fn unsubscribe(
    State(state): State<AppState>,
    Path((slug, project_raw, issue_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> Response {
    let issue_id = match issue_raw.parse::<uuid::Uuid>() {
        Ok(id) => id,
        Err(_) => return crate::edge::proxy(State(state), req).await,
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(error) => return error.into_response(),
    };
    let user_id = match actor_user_id(&pool, extension).await {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    let project_id = match resolve_project_id(&pool, &slug, &project_raw).await {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    let membership = match fetch_membership(&pool, &slug, &project_id, &user_id).await {
        Ok(membership) => membership,
        Err(error) => return error.into_response(),
    };
    if let Err(error) = check_lite(&membership) {
        return error.into_response();
    }
    let _ = req;
    let pk = match fetch_subscription(&pool, &slug, &project_id, &issue_id, &user_id).await {
        Ok(Some(pk)) => pk,
        Ok(None) => return Denial::NotFound.into_response(),
        Err(error) => return error.into_response(),
    };
    let now = chrono::Utc::now();
    if let Err(error) = soft_delete_row(&pool, "issue_subscribers", &pk, &user_id, &now).await {
        return error.into_response();
    }
    enqueue_soft_delete(&pool, "issuesubscriber", &pk).await;
    empty_response(StatusCode::NO_CONTENT)
}

/// `subscription_status` (`subscriber.py:98-104`).
async fn subscription_status(
    State(state): State<AppState>,
    Path((slug, project_raw, issue_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> Response {
    let issue_id = match issue_raw.parse::<uuid::Uuid>() {
        Ok(id) => id,
        Err(_) => return crate::edge::proxy(State(state), req).await,
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(error) => return error.into_response(),
    };
    let user_id = match actor_user_id(&pool, extension).await {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    let project_id = match resolve_project_id(&pool, &slug, &project_raw).await {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    let membership = match fetch_membership(&pool, &slug, &project_id, &user_id).await {
        Ok(membership) => membership,
        Err(error) => return error.into_response(),
    };
    if let Err(error) = check_lite(&membership) {
        return error.into_response();
    }
    let _ = req;
    match fetch_subscription(&pool, &slug, &project_id, &issue_id, &user_id).await {
        Ok(subscribed) => json_response(
            StatusCode::OK,
            format!("{{\"subscribed\":{}}}", subscribed.is_some()),
        ),
        Err(error) => error.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The positional offsets transcribe the builder select lists; pin
    /// the column counts so drift fails loudly.
    #[test]
    fn builder_column_counts_match_offsets() {
        fn count(list: &str) -> usize {
            list.split(',').count()
        }
        assert_eq!(count(super::super::queries_engage::ACTIVITY_COLUMNS), 20);
        assert_eq!(count(super::super::queries_engage::COMMENT_COLUMNS), 24);
        assert_eq!(
            count(super::super::queries_engage::COMMENT_REACTION_COLUMNS),
            11
        );
        assert_eq!(
            count(super::super::queries_core::PROJECT_COLUMNS),
            PROJECT_COLS
        );
        assert_eq!(
            count(super::super::queries_engage::WORKSPACE_COLUMNS),
            WORKSPACE_COLS
        );
        assert_eq!(
            count(super::super::queries_core::PREFETCH_ISSUE_COLUMNS),
            ISSUE_COLS
        );
        assert_eq!(count(super::super::queries_core::USER_COLUMNS), 40);
        // Spot-check the within-table offsets this module decodes.
        let project: Vec<&str> = super::super::queries_core::PROJECT_COLUMNS
            .split(',')
            .collect();
        assert!(project[P_ID].trim_end().ends_with("projects.id"));
        assert!(project[P_IDENTIFIER]
            .trim_end()
            .ends_with("projects.identifier"));
        assert!(project[P_LOGO_PROPS]
            .trim_end()
            .ends_with("projects.logo_props"));
        let user: Vec<&str> = super::super::queries_core::USER_COLUMNS
            .split(',')
            .collect();
        assert!(user[U_DISPLAY_NAME]
            .trim_end()
            .ends_with("users.display_name"));
        assert!(user[U_AVATAR_ASSET_ID]
            .trim_end()
            .ends_with("users.avatar_asset_id"));
        let comment: Vec<&str> = super::super::queries_engage::COMMENT_COLUMNS
            .split(',')
            .collect();
        assert!(comment[C_HTML]
            .trim_end()
            .ends_with("issue_comments.comment_html"));
        assert!(comment[C_PARENT_ID]
            .trim_end()
            .ends_with("issue_comments.parent_id"));
        let reaction: Vec<&str> = super::super::queries_engage::COMMENT_REACTION_COLUMNS
            .split(',')
            .collect();
        assert!(reaction[R_REACTION]
            .trim_end()
            .ends_with("comment_reactions.reaction"));
    }

    #[test]
    fn strip_tags_matches_django() {
        // The shared `MLStripper` port (`html_processor.strip_tags`,
        // vectors verified against the live helper).
        assert_eq!(ml_strip_tags("<p>cycle note</p>"), "cycle note");
        assert_eq!(ml_strip_tags(""), "");
        assert_eq!(ml_strip_tags("plain"), "plain");
        assert_eq!(ml_strip_tags("<p>a<br/>b</p>"), "ab");
        assert_eq!(ml_strip_tags("a < b"), "a < b");
        assert_eq!(ml_strip_tags("<p>x</p><!-- c -->tail"), "xtail");
        assert_eq!(ml_strip_tags("&lt;p&gt;"), "<p>");
    }

    #[test]
    fn python_dumps_matches_cpython() {
        let value = json!({"b": [1, "x\ny"], "a": Value::Null, "u": "caf\u{e9}"});
        assert_eq!(
            python_dumps(&value),
            r#"{"b": [1, "x\ny"], "a": null, "u": "caf\u00e9"}"#
        );
        assert_eq!(
            python_dumps(&json!({"comment_id": "abc"})),
            r#"{"comment_id": "abc"}"#
        );
    }

    #[test]
    fn parse_body_shapes_match_drf() {
        assert!(parse_body(b"").unwrap().is_empty());
        let map = parse_body(br#"{"reaction": "+1"}"#).unwrap();
        assert_eq!(map["reaction"], json!("+1"));
        match parse_body(b"[1]").unwrap_err() {
            Denial::BadJson(value) => assert_eq!(
                value,
                json!({"non_field_errors": ["Invalid data. Expected a dictionary, but got list."]})
            ),
            other => panic!("unexpected {other:?}"),
        }
        match parse_body(b"null").unwrap_err() {
            Denial::BadJson(value) => {
                assert_eq!(value, json!({"non_field_errors": ["No data provided"]}))
            }
            other => panic!("unexpected {other:?}"),
        }
        match parse_body(b"{oops").unwrap_err() {
            Denial::BadDetail(message) => assert!(message.starts_with("JSON parse error - ")),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn py_value_semantics_match_cpython() {
        assert_eq!(py_str_value(&Value::Null), "None");
        assert_eq!(py_str_value(&json!(true)), "True");
        assert_eq!(py_str_value(&json!(77)), "77");
        assert_eq!(py_str_value(&json!("internal")), "internal");
        assert_eq!(py_str_value(&json!(["INTERNAL"])), "['INTERNAL']");
        assert_eq!(py_str_value(&json!({"a": 1})), "{'a': 1}");
        assert_eq!(py_repr_value(&json!("don't")), "\"don't\"");
        assert_eq!(py_repr_value(&json!("say \"hi\"")), "'say \"hi\"'");
        assert_eq!(py_repr_value(&json!("a\nb")), "'a\\nb'");
        assert!(py_json_eq(&json!(true), &json!(1)));
        assert!(py_json_eq(&json!(1), &json!(1.0)));
        assert!(py_json_eq(
            &json!({"a": 1, "b": [1, 2]}),
            &json!({"b": [1, 2], "a": 1.0})
        ));
        assert!(!py_json_eq(&json!(1), &json!("1")));
        assert!(!py_json_eq(&json!([1, 2]), &json!([2, 1])));
    }

    #[test]
    fn char_field_coercion_matches_drf() {
        let mut errors = Map::new();
        let body: Map<String, Value> =
            serde_json::from_value(json!({"r": 77, "f": 4.5, "s": "  x  "})).unwrap();
        assert!(matches!(
            check_text(&mut errors, &body, "r", None, false, false),
            Presence::Value(text) if text == "77"
        ));
        assert!(matches!(
            check_text(&mut errors, &body, "f", None, false, false),
            Presence::Value(text) if text == "4.5"
        ));
        assert!(matches!(
            check_text(&mut errors, &body, "s", None, false, false),
            Presence::Value(text) if text == "x"
        ));
        let body: Map<String, Value> =
            serde_json::from_value(json!({"b": true, "sp": "   ", "n": Value::Null})).unwrap();
        assert!(matches!(
            check_text(&mut errors, &body, "b", None, false, false),
            Presence::Missing
        ));
        assert!(matches!(
            check_text(&mut errors, &body, "sp", None, false, false),
            Presence::Missing
        ));
        assert_eq!(
            Value::Object(errors),
            json!({
                "b": ["Not a valid string."],
                "sp": ["This field may not be blank."],
            })
        );
    }

    #[test]
    fn url_verdicts_match_django_probes() {
        for ok in [
            "https://x.example/a.png",
            "HTTP://X.EXAMPLE/A",
            "http://localhost/",
            "http://127.0.0.1:8080/x",
            "http://[::1]/",
            "http://example.com.",
            "http://user:pw@h.example/x",
            "http://münchen.de/",
            "http://xn--e28h.example/",
            "http://😀.example/",
            "http://user%zz@h.example/",
            "ftp://f.example:21/x",
            "https://x.co/?q=1&r=2",
            "http://[::ffff:1.2.3.4]/",
            "http://a.bc/",
        ] {
            assert!(is_valid_url(ok), "{ok}");
        }
        for bad in [
            "a.b",
            "http://a.b",
            "http://a",
            "http:///x",
            "http://",
            "gopher://x.example/",
            "javascript:alert(1)",
            "https://x.example/a b.png",
            "http://example.com:/x",
            "http://[::1",
            "http://a-.com/",
            "http://-a.com/",
            "http://example.123/",
            "http://foo.bar1/",
            "http://1.2.3.4.5/",
            "http://01.2.3.4/",
            "http://[fe80::1%25eth0]/",
            "http://us＠er@h.example/",
            "http://h／.example/",
        ] {
            assert!(!is_valid_url(bad), "{bad}");
        }
    }

    #[test]
    fn py_float_repr_matches_cpython_oracle() {
        // CPython `repr` over edge + random f64s (seed 654): parsing
        // each spelling must reproduce it exactly.
        for text in [
            "0.0",
            "-0.0",
            "1.0",
            "-1.0",
            "0.1",
            "-0.1",
            "0.5",
            "4.5",
            "77.0",
            "1e+16",
            "-1e+16",
            "1000000000000000.0",
            "9999999999999998.0",
            "0.0001",
            "9.9999e-05",
            "1e-05",
            "1.5e-07",
            "1.23",
            "123.456",
            "1.1",
            "2.2",
            "0.3",
            "1e+300",
            "1.7976931348623157e+308",
            "5e-324",
            "2.2250738585072014e-308",
            "1e+21",
            "123456789.12345679",
            "0.00010000000001",
            "3.141592653589793",
            "100.0",
            "1000000.0",
            "1000000.0",
            "1234567890000000.0",
            "1.0000000000000002",
            "2.0000000000000004",
            "3.1969326087014466e+19",
            "-648.4462954478394",
            "2.7257935151841477e+19",
            "579.4076537460154",
            "8.527238532839549e+19",
            "-913.3955515106092",
            "8.896161802149298e+19",
            "-715.3670336061672",
            "3.588060462234755e+19",
            "435.64988671404353",
            "7.755666951748308e+19",
            "-689.2765889623878",
            "-4.330334504373883e+19",
            "-816.9972547462557",
            "-7.212821885073056e+19",
            "-1679.6052986552581",
            "3.2445178360908284e+19",
            "-204.03326175159876",
            "-8.659247373857653e+18",
            "1208.8390033969868",
            "-4.647260496917678e+19",
            "2331.221077226743",
            "8.5327628826864e+19",
            "1001.189002479752",
            "-8.234019824386068e+19",
            "-495.73367760484905",
            "9.352414724532316e+19",
            "346.0259035320625",
            "-7.1336788737953415e+19",
            "-956.186083621841",
            "-7.678412411071945e+19",
            "743.1470832768787",
            "-2.5012238015527862e+19",
            "-151.39194187298384",
            "5.543620478186435e+19",
            "-402.4753197702496",
            "-9.86234000471552e+19",
            "-1687.6484285668644",
            "-1.2613779274247045e+19",
            "295.5344355622931",
            "-4.708884174573601e+18",
            "143.2457353449458",
            "9.272274689849475e+19",
            "782.9417038372993",
            "-2.365484285927129e+19",
            "213.25467548742972",
            "-4.4287013372000076e+19",
            "-75.27500601863878",
            "7.3002688662026355e+19",
            "-617.7363223782181",
            "2.8132118711075865e+19",
            "642.045311657188",
            "-4.9760184971786445e+19",
            "-493.23593034675645",
            "-5.92865673699603e+19",
            "1271.787532431871",
            "8.862838211447603e+18",
            "-102.63510865808792",
            "2.4099300144340025e+19",
            "-2281.3345229971756",
            "-5.346252045636608e+19",
            "2132.4041415069833",
            "-6.090543569457227e+19",
            "1863.0368182127777",
            "6.0170257457244996e+19",
            "-182.18689918793999",
            "6.525439356052767e+17",
            "-1362.5914219885572",
            "5.1796889258414604e+19",
            "-319.24195464384303",
            "-8.253703237953189e+19",
            "287.06500363070336",
            "-2.591301605754159e+19",
            "1392.893079979731",
            "2.655060505738576e+18",
            "2036.4351785039644",
            "7.427269069333139e+19",
            "1347.4365990857013",
            "-8.434791962696217e+19",
            "-186.0700574057584",
        ] {
            let value: f64 = text.parse().unwrap();
            assert_eq!(py_float_repr(value), text, "{text}");
        }
        assert_eq!(py_float_repr(f64::NAN), "nan");
        assert_eq!(py_float_repr(f64::INFINITY), "inf");
        assert_eq!(py_float_repr(f64::NEG_INFINITY), "-inf");
    }

    #[test]
    fn input_datetime_parsing_matches_drf() {
        // Offset-aware shapes normalize to UTC.
        let dt = parse_input_datetime("2024-05-01T12:30:00+02:00").unwrap();
        assert_eq!(
            dt.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true),
            "2024-05-01T10:30:00Z"
        );
        // `Z` and space separators pass.
        assert!(parse_input_datetime("2024-05-01T10:30:00Z").is_some());
        assert!(parse_input_datetime("2024-05-01 10:30:00").is_some());
        // Naive reads as UTC.
        let naive = parse_input_datetime("2024-05-01T10:30:00").unwrap();
        assert_eq!(
            naive.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true),
            "2024-05-01T10:30:00Z"
        );
        // `fromisoformat` extras (all probed live against Django).
        assert_eq!(
            parse_input_datetime("2024-01-01")
                .unwrap()
                .to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true),
            "2024-01-01T00:00:00Z"
        );
        assert!(parse_input_datetime("2024-1-1T1:1").is_some());
        assert!(parse_input_datetime("20240101T000000").is_some());
        assert!(parse_input_datetime("2024-W01-1").is_some());
        assert!(parse_input_datetime("2024-01-01X00:00:00").is_some());
        assert!(parse_input_datetime("2024-01-01T00:00:00,5").is_some());
        assert_eq!(
            parse_input_datetime("2024-01-01T00:00:00.1234567")
                .unwrap()
                .to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true),
            "2024-01-01T00:00:00.123456Z"
        );
        assert!(parse_input_datetime("2024-01-01T00:00:00+0000").is_some());
        assert!(parse_input_datetime("2024-01-01T00:00:00+00").is_some());
        // Rejections (all probed live).
        for bad in [
            "garbage",
            "",
            "2024-01-01t00:00:00z",
            "2024-01-01T00:00:00z",
            "2024-001",
            "2024001",
            "2024-w01-1",
            "2024-W011",
            "2024-01-01Z",
            " 2024-01-01T00:00:00Z",
            "2024-01-01T00:00:00+0",
            "2024-01-01T24:00:00",
            "2024-01-01T00:00:60",
            "2024-01-01T00:00:00.",
            "2024-02-30T00:00:00",
            "2023-W53-7",
            "2024-01-01T0",
            "2024-01-01T00:00:00+24:00",
        ] {
            assert!(parse_input_datetime(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn text_validation_messages_match_drf() {
        let mut errors = Map::new();
        let body: Map<String, Value> =
            serde_json::from_value(json!({"a": true, "b": Value::Null, "c": "", "d": "toolong"}))
                .unwrap();
        assert!(matches!(
            check_text(&mut errors, &body, "a", None, false, true),
            Presence::Missing
        ));
        assert!(matches!(
            check_text(&mut errors, &body, "b", None, false, true),
            Presence::Missing
        ));
        assert!(matches!(
            check_text(&mut errors, &body, "c", None, false, false),
            Presence::Missing
        ));
        assert!(matches!(
            check_text(&mut errors, &body, "d", Some(3), false, true),
            Presence::Missing
        ));
        assert!(matches!(
            check_text(&mut errors, &body, "zzz", None, false, true),
            Presence::Missing
        ));
        assert_eq!(
            Value::Object(errors),
            json!({
                "a": ["Not a valid string."],
                "b": ["This field may not be null."],
                "c": ["This field may not be blank."],
                "d": ["Ensure this field has no more than 3 characters."],
            })
        );
    }

    #[test]
    fn choice_and_uuid_messages_match_drf() {
        let mut errors = Map::new();
        let body: Map<String, Value> =
            serde_json::from_value(json!({"access": "bogus", "run": "nope"})).unwrap();
        assert!(matches!(
            check_choice(
                &mut errors,
                &body,
                "access",
                &["INTERNAL", "EXTERNAL"],
                false,
            ),
            Presence::Missing
        ));
        assert!(matches!(
            check_uuid(&mut errors, &body, "run", true),
            Presence::Missing
        ));
        assert_eq!(
            Value::Object(errors),
            json!({
                "access": ["\"bogus\" is not a valid choice."],
                "run": ["Must be a valid UUID."],
            })
        );
    }

    #[test]
    fn list_field_errors_match_drf_shape() {
        let mut errors = Map::new();
        let body: Map<String, Value> =
            serde_json::from_value(json!({"attachments": ["https://ok.example/a", "nope", 7]}))
                .unwrap();
        assert!(matches!(
            check_string_list(
                &mut errors,
                &body,
                "attachments",
                10,
                &validate_attachment_child
            ),
            Presence::Missing
        ));
        assert_eq!(
            Value::Object(errors),
            json!({"attachments": {"1": ["Enter a valid URL."], "2": ["Enter a valid URL."]}})
        );
        let mut errors = Map::new();
        let body: Map<String, Value> = serde_json::from_value(json!({"labels": "nope"})).unwrap();
        assert!(matches!(
            check_string_list(&mut errors, &body, "labels", 8, &validate_label_child),
            Presence::Missing
        ));
        assert_eq!(
            Value::Object(errors),
            json!({"labels": ["Expected a list of items but got type \"str\"."]})
        );
    }

    #[test]
    fn url_check_accepts_realistic_attachments() {
        assert!(is_valid_url("https://files.example.com/a.png"));
        assert!(is_valid_url("http://localhost:8000/a"));
        assert!(is_valid_url("ftp://files.example.com/a"));
        assert!(!is_valid_url("not-a-url"));
        assert!(!is_valid_url("gopher://example.com/a"));
        assert!(!is_valid_url("https://"));
    }

    #[test]
    fn denial_bodies_match_drf() {
        let (status, body) = Denial::Unauthorized.status_and_body();
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(
            body,
            r#"{"detail":"Authentication credentials were not provided."}"#
        );
        let (status, body) = Denial::Forbidden.status_and_body();
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(
            body,
            r#"{"error":"You don't have the required permissions."}"#
        );
        let (status, body) = Denial::ForbiddenDetail.status_and_body();
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body, VIEWSET_FORBIDDEN_BODY);
        let (status, body) = Denial::NotFoundDetail.status_and_body();
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(
            body,
            r#"{"detail":"No IssueComment matches the given query."}"#
        );
        let (status, body) = Denial::ProjectNotFound.status_and_body();
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body, r#"{"detail":"Project not found"}"#);
        let (status, body) =
            Denial::BadError("The payload is not valid".to_owned()).status_and_body();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body, r#"{"error":"The payload is not valid"}"#);
        let (status, body) = Denial::BadMessage("User already subscribed to the issue.".to_owned())
            .status_and_body();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(
            body,
            r#"{"message":"User already subscribed to the issue."}"#
        );
        // DRF's `APIException` envelope is lowercase `detail`
        // (`rest_framework/views.py:96`); the 400 parse error and the 415
        // share it.
        let (status, body) =
            Denial::BadDetail("JSON parse error - oops".to_owned()).status_and_body();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body, r#"{"detail":"JSON parse error - oops"}"#);
        let (status, body) = Denial::UnsupportedMediaType(
            "Unsupported media type \"text/plain\" in request.".to_owned(),
        )
        .status_and_body();
        assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
        assert_eq!(
            body,
            r#"{"detail":"Unsupported media type \"text/plain\" in request."}"#
        );
    }

    #[test]
    fn sync_lock_only_fires_on_touched_and_changed() {
        let id = uuid::Uuid::new_v4();
        let now = chrono::Utc::now();
        let stored = CommentBaseOwned {
            id,
            created_at: now,
            updated_at: now,
            created_by: None,
            updated_by: None,
            deleted_at: None,
            project_id: uuid::Uuid::new_v4(),
            workspace_id: uuid::Uuid::new_v4(),
            stripped: "note".to_owned(),
            json: json!({}),
            html: "<p>note</p>".to_owned(),
            description_id: None,
            attachments: vec![],
            labels: vec![],
            issue_id: uuid::Uuid::new_v4(),
            actor_id: None,
            access: "INTERNAL".to_owned(),
            external_source: None,
            external_id: None,
            speaker_type: "human".to_owned(),
            speaker_label: String::new(),
            speaker_run_id: None,
            edited_at: None,
            parent_id: None,
        };
        // Untouched triple: no errors.
        let attrs = CommentValidated {
            access: Some("EXTERNAL".to_owned()),
            ..CommentValidated::default()
        };
        assert!(comment_sync_lock_errors(&stored, &attrs).is_empty());
        // Touched-but-equal: no errors.
        let attrs = CommentValidated {
            html: Some("<p>note</p>".to_owned()),
            ..CommentValidated::default()
        };
        assert!(comment_sync_lock_errors(&stored, &attrs).is_empty());
        // Touched-and-changed: per-field errors.
        let attrs = CommentValidated {
            html: Some("<p>edited</p>".to_owned()),
            json: Some(json!({"a": 1})),
            ..CommentValidated::default()
        };
        let errors = comment_sync_lock_errors(&stored, &attrs);
        assert_eq!(errors.len(), 2);
        assert!(errors.contains_key("comment_html"));
        assert!(errors.contains_key("comment_json"));
    }

    #[test]
    fn comment_view_key_order_matches_drf() {
        let view = CommentView {
            id: "id".to_owned(),
            actor_detail: None,
            issue_detail: json!({}),
            project_detail: json!({}),
            workspace_detail: json!({}),
            comment_reactions: vec![],
            is_member: Some(true),
            is_synced: false,
            created_at: "c".to_owned(),
            updated_at: "u".to_owned(),
            deleted_at: None,
            comment_stripped: "s".to_owned(),
            comment_json: json!({}),
            comment_html: "h".to_owned(),
            attachments: vec![],
            labels: vec![],
            access: "INTERNAL".to_owned(),
            external_source: None,
            external_id: None,
            speaker_type: "human".to_owned(),
            speaker_label: String::new(),
            speaker_agent_run_id: None,
            edited_at: None,
            created_by: None,
            updated_by: None,
            project: "p".to_owned(),
            workspace: "w".to_owned(),
            description: None,
            issue: "i".to_owned(),
            actor: None,
            parent: None,
        };
        let value = serde_json::to_value(&view).unwrap();
        let keys: Vec<String> = value.as_object().unwrap().keys().cloned().collect();
        assert_eq!(
            keys,
            vec![
                "id",
                "actor_detail",
                "issue_detail",
                "project_detail",
                "workspace_detail",
                "comment_reactions",
                "is_member",
                "is_synced",
                "created_at",
                "updated_at",
                "deleted_at",
                "comment_stripped",
                "comment_json",
                "comment_html",
                "attachments",
                "labels",
                "access",
                "external_source",
                "external_id",
                "speaker_type",
                "speaker_label",
                "speaker_agent_run_id",
                "edited_at",
                "created_by",
                "updated_by",
                "project",
                "workspace",
                "description",
                "issue",
                "actor",
                "parent",
            ]
        );
    }

    #[test]
    fn reaction_view_key_orders_match_drf() {
        let reaction = CommentReactionView {
            id: "id".to_owned(),
            actor: "a".to_owned(),
            comment: "c".to_owned(),
            reaction: "r".to_owned(),
            display_name: "d".to_owned(),
            deleted_at: None,
            workspace: "w".to_owned(),
            project: "p".to_owned(),
            created_at: "c".to_owned(),
            updated_at: "u".to_owned(),
            created_by: None,
            updated_by: None,
        };
        let reaction_value = serde_json::to_value(&reaction).unwrap();
        let keys: Vec<String> = reaction_value
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect();
        assert_eq!(
            keys,
            vec![
                "id",
                "actor",
                "comment",
                "reaction",
                "display_name",
                "deleted_at",
                "workspace",
                "project",
                "created_at",
                "updated_at",
                "created_by",
                "updated_by",
            ]
        );
        let issue = IssueReactionView {
            id: "id".to_owned(),
            actor_detail: json!({}),
            created_at: "c".to_owned(),
            updated_at: "u".to_owned(),
            deleted_at: None,
            reaction: "r".to_owned(),
            created_by: None,
            updated_by: None,
            project: "p".to_owned(),
            workspace: "w".to_owned(),
            actor: "a".to_owned(),
            issue: "i".to_owned(),
        };
        let issue_value = serde_json::to_value(&issue).unwrap();
        let keys: Vec<String> = issue_value.as_object().unwrap().keys().cloned().collect();
        assert_eq!(
            keys,
            vec![
                "id",
                "actor_detail",
                "created_at",
                "updated_at",
                "deleted_at",
                "reaction",
                "created_by",
                "updated_by",
                "project",
                "workspace",
                "actor",
                "issue",
            ]
        );
    }

    #[test]
    fn splice_and_keeps_django_shape() {
        let sql = "SELECT a FROM t WHERE x = $1 ORDER BY t.created_at DESC";
        assert_eq!(
            splice_and(sql, "t.id = $2"),
            "SELECT a FROM t WHERE x = $1  AND t.id = $2 ORDER BY t.created_at DESC"
        );
    }

    #[test]
    fn asset_url_matches_model_property() {
        assert_eq!(
            asset_url("abc", Some("USER_AVATAR"), None, None, None),
            Some("/api/assets/v2/static/abc/".to_owned())
        );
        assert_eq!(
            asset_url(
                "abc",
                Some("ISSUE_ATTACHMENT"),
                Some("ws"),
                Some("p"),
                Some("i")
            ),
            Some("/api/assets/v2/workspaces/ws/projects/p/issues/i/attachments/abc/".to_owned())
        );
        assert_eq!(asset_url("abc", Some("BOGUS"), None, None, None), None);
        assert_eq!(
            asset_url("abc", Some("ISSUE_ATTACHMENT"), None, Some("p"), Some("i")),
            None
        );
    }

    fn file_part(filename: &str) -> shared_body::FilePart {
        shared_body::FilePart {
            filename: filename.to_owned(),
            content_type: String::new(),
            bytes: Vec::new(),
            in_memory: true,
        }
    }

    /// `""` on an FK coerces to null for HTML input only (`Field.get_value`);
    /// JSON `""` runs `to_internal_value` → the curly error (no DB read on
    /// either path, so a lazy pool never connects).
    #[tokio::test]
    async fn fk_empty_string_is_html_gated() {
        let pool = sqlx::PgPool::connect_lazy("postgres://localhost/pidash_test_unused").unwrap();
        let mut errors = Map::new();
        let body: Map<String, Value> = serde_json::from_value(json!({"actor": ""})).unwrap();
        assert!(matches!(
            check_fk(&pool, &mut errors, &body, "actor", "users", true, false).await,
            Ok(Presence::Missing)
        ));
        assert_eq!(
            errors.get("actor").unwrap(),
            &json!(["\u{201c}\u{201d} is not a valid UUID."])
        );
        let mut errors = Map::new();
        assert!(matches!(
            check_fk(&pool, &mut errors, &body, "actor", "users", true, true).await,
            Ok(Presence::Null)
        ));
        assert!(errors.is_empty());
        // The required-FK fallthrough: HTML `""` without allow_null runs
        // `to_internal_value` (`get_value` falls through when required).
        let mut errors = Map::new();
        assert!(matches!(
            check_fk(&pool, &mut errors, &body, "actor", "users", false, true).await,
            Ok(Presence::Missing)
        ));
        assert_eq!(
            errors.get("actor").unwrap(),
            &json!(["\u{201c}\u{201d} is not a valid UUID."])
        );
    }

    /// A JSON `{"__file__": …}` dict is ordinary data (py-str rendering in
    /// the curly error); the HTML-input sentinel echoes its filename.
    #[tokio::test]
    async fn fk_file_sentinel_is_html_gated() {
        let pool = sqlx::PgPool::connect_lazy("postgres://localhost/pidash_test_unused").unwrap();
        let body: Map<String, Value> =
            serde_json::from_value(json!({"actor": {"__file__": "pic.png"}})).unwrap();
        let mut errors = Map::new();
        assert!(matches!(
            check_fk(&pool, &mut errors, &body, "actor", "users", true, false).await,
            Ok(Presence::Missing)
        ));
        assert_eq!(
            errors.get("actor").unwrap(),
            &json!(["\u{201c}{'__file__': 'pic.png'}\u{201d} is not a valid UUID."])
        );
        let mut errors = Map::new();
        assert!(matches!(
            check_fk(&pool, &mut errors, &body, "actor", "users", true, true).await,
            Ok(Presence::Missing)
        ));
        assert_eq!(
            errors.get("actor").unwrap(),
            &json!(["\u{201c}pic.png\u{201d} is not a valid UUID."])
        );
    }

    /// Same gate on the choice path: JSON sentinel dicts render py-str,
    /// HTML-input uploads echo the filename into the membership test.
    #[test]
    fn choice_file_sentinel_is_html_gated() {
        let body: Map<String, Value> =
            serde_json::from_value(json!({"access": {"__file__": "INTERNAL"}})).unwrap();
        let mut errors = Map::new();
        assert!(matches!(
            check_choice(
                &mut errors,
                &body,
                "access",
                &["INTERNAL", "EXTERNAL"],
                false
            ),
            Presence::Missing
        ));
        assert_eq!(
            errors.get("access").unwrap(),
            &json!(["\"{'__file__': 'INTERNAL'}\" is not a valid choice."])
        );
        let mut errors = Map::new();
        assert!(matches!(
            check_choice(
                &mut errors,
                &body,
                "access",
                &["INTERNAL", "EXTERNAL"],
                true
            ),
            Presence::Value(_)
        ));
        assert!(errors.is_empty());
    }

    /// Indexed list assemblies resolve positionally: Null placeholders
    /// become the next upload's sentinel, dict-form inner nulls pop for
    /// alignment (no leftover append), exact-key uploads append after
    /// texts.
    #[test]
    fn form_list_indexed_placeholders_resolve_positionally() {
        let map: Map<String, Value> =
            serde_json::from_value(json!({"labels": ["a", null]})).unwrap();
        let mut files = shared_body::FilesMap::new();
        files.insert("labels".to_owned(), vec![file_part("f.txt")]);
        let merged = form_body_map(map, files).unwrap();
        assert_eq!(
            merged.get("labels").unwrap(),
            &json!(["a", {"__file__": "f.txt"}])
        );

        let map: Map<String, Value> =
            serde_json::from_value(json!({"labels": [[{"s": [null]}]]})).unwrap();
        let mut files = shared_body::FilesMap::new();
        files.insert("labels".to_owned(), vec![file_part("g.txt")]);
        let merged = form_body_map(map, files).unwrap();
        assert_eq!(merged.get("labels").unwrap(), &json!([[ {"s": [null]} ]]));

        let map: Map<String, Value> = serde_json::from_value(json!({"labels": ["a"]})).unwrap();
        let mut files = shared_body::FilesMap::new();
        files.insert(
            "labels".to_owned(),
            vec![file_part("p1.txt"), file_part("p2.txt")],
        );
        let merged = form_body_map(map, files).unwrap();
        assert_eq!(
            merged.get("labels").unwrap(),
            &json!(["a", {"__file__": "p1.txt"}, {"__file__": "p2.txt"}])
        );
    }

    /// `comment_json` uploads stringify to the filename before `json.loads`:
    /// a file named `123` yields the number, a prosaic name is invalid.
    #[test]
    fn form_comment_json_filename_parses() {
        let mut files = shared_body::FilesMap::new();
        files.insert("comment_json".to_owned(), vec![file_part("123")]);
        let merged = form_body_map(Map::new(), files).unwrap();
        assert_eq!(merged.get("comment_json").unwrap(), &json!(123));

        let mut files = shared_body::FilesMap::new();
        files.insert("comment_json".to_owned(), vec![file_part("notes.txt")]);
        match form_body_map(Map::new(), files) {
            Err(Denial::BadJson(value)) => assert_eq!(
                value,
                json!({"comment_json": ["Value must be valid JSON."]})
            ),
            other => panic!("expected invalid JSON error, got {other:?}"),
        }
    }
}
