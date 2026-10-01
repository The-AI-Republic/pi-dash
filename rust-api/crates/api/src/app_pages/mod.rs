#![forbid(unsafe_code)]

//! App page favorites + description + state-op + write handlers (D-30,
//! stage 5, PIDASHCONV-332, PIDASHCONV-328 and PIDASHCONV-322).
//!
//! Ports twelve endpoints from `apps/api/pi_dash/app/views/page/base.py`
//! (routes in `apps/api/pi_dash/app/urls/page.py`):
//!
//! * `POST .../pages/` (`PageViewSet.create`, `base.py:129-152`,
//!   PIDASHCONV-322): `PageSerializer` validation over the request body
//!   with `description_*` from the raw body (context), the `ProjectPage`
//!   bridge plus `PageLabel` bulk rows, an unconditional
//!   `page_transaction` publish, a re-read through `get_queryset`, and a
//!   `PageDetailSerializer` 201.
//! * `PATCH .../pages/<page_id>/` (`partial_update`, `base.py:154-200`,
//!   PIDASHCONV-322): the scoped fetch, the lock guard, the scoped parent
//!   re-fetch, the owner-only access rule (whose body the `DoesNotExist`
//!   branch reuses), the partial `PageDetailSerializer` save with the
//!   conditional `page_transaction` publish, 200.
//! * `DELETE .../pages/<page_id>/` (`destroy`, `base.py:368-419`,
//!   PIDASHCONV-322): must-be-archived, owner-or-admin-20, children
//!   detach, the soft page delete, favorite soft-delete plus recent-visit
//!   hard-delete cleanup, 204.
//!
//! * `POST .../favorite-pages/<page_id>/` (`PageFavoriteViewSet.create`,
//!   `base.py:472-483`): `@allow_permission([ADMIN, MEMBER])`, creates a
//!   `UserFavorite` row (`project`, entity `page`/`page_id`, user), 204.
//! * `DELETE .../favorite-pages/<page_id>/` (`destroy`, `base.py:485-495`):
//!   scoped favorite fetch plus hard delete (`delete(soft=False)`), 204.
//! * `GET .../pages/<page_id>/description/` (`PagesDescriptionViewSet.retrieve`,
//!   `base.py:501-519`): `Q(owned_by=user) | Q(access=0)` scoped fetch,
//!   streaming `application/octet-stream` body of `description_binary`
//!   (empty bytes when null), `Content-Disposition: attachment;
//!   filename="page_description.bin"`.
//! * `PATCH .../pages/<page_id>/description/` (`partial_update`,
//!   `base.py:521-575`): same scoping, `PAGE_LOCKED` 400, `PAGE_ARCHIVED`
//!   400, old-HTML capture, `existing_instance` JSON dump,
//!   `PageBinaryUpdateSerializer` save, conditional `page_transaction`
//!   plus unconditional `track_page_version` publishes, 200
//!   `{"message":"Updated successfully"}`.
//! * `POST .../pages/<page_id>/archive/` (`archive`, `base.py:308-337`):
//!   owner-or-admin (`role <= 15` member who is not the owner 400s),
//!   favorite rows soft-deleted, recursive CTE stamps the subtree, 200
//!   `{"archived_at": ...}` from a second `now()` call.
//! * `DELETE .../pages/<page_id>/archive/` (`unarchive`, `base.py:339-366`):
//!   same guard ("un archive" body), parent-archived detach, CTE with
//!   `NULL`, 204.
//! * `POST .../pages/<page_id>/lock/` (`lock`, `base.py:246-256`) and
//!   `DELETE .../lock/` (`unlock`, `base.py:258-269`): `is_locked` flip
//!   plus save, 204.
//! * `POST .../pages/<page_id>/access/` (`access`, `base.py:271-289`):
//!   access defaults to 0, owner-only change 400, save, 204.
//!
//! Fixtures: `rust-api/fixtures/app_pages/handlers/io.golden.json`
//! (F30-11, per-action I/O + routes), `queries/archive_cte.sql` +
//! `.rows.json` (F30-08, CTE + hierarchy before/after),
//! `serializers/page_binary_update.golden.json`
//! (F30-02, binary/HTML validation vectors),
//! `guards/permissions.golden.json` (F30-09, class + inline guards),
//! `tasks/publish.golden.json` (F30-10, publish envelopes).
//! Trace: `rust-api/fixtures/app_pages/TRACE.md`.
//!
//! Layering: the class half of auth lives in [`gate`] (`ProjectPagePermission`
//! via `resolve_gate`, following the `app_issues` `Gate` precedent); the
//! favorites `@allow_permission([ADMIN, MEMBER])` project-role gate is ported
//! inline here (it performs no page lookup, unlike the class); field
//! validation mirrors `pidash_services::app_pages::shape` (binary decode,
//! HTML substitution rule, DRF error bodies); the HTML cleaner and
//! `strip_tags` come from `crate::space::sanitize` (`ammonia`, the same
//! crate `nh3` binds); publishes use the merged
//! `pidash_jobs::app_pages` constructors and enqueue best-effort post-commit
//! like the D-02 intake handlers.
//!
//! Gate order (preserved, not redesigned): URL resolution (`<uuid:page_id>`
//! mismatch proxies to Django's resolver 404, the `app_cycles` precedent),
//! `BaseViewSet.initial` project rewrite (`Project.resolve`, authenticated
//! only) and session auth, then the per-route gate, then the view body with
//! its inline guards. Anonymous callers 401 before any gate (`IsAuthenticated`
//! precedes the favorite decorator, which is why the decorator's own 403
//! never fires for them).
//!
//! Ported bugs and quirks (translate, don't redesign — also in the PR):
//!
//! * QUIRK-no-guard-on-get (`base.py:501-519` vs `:521-575`): the GET has no
//!   lock/archived guard while the PATCH has both; a locked or archived
//!   page still streams its bytes.
//! * QUIRK-no-page-check-on-favorite (`base.py:472-483`): favorite create
//!   performs no page lookup, so an unknown `page_id` still answers 204.
//! * QUIRK-duplicate-favorite-400 (`base.py:475-483`): a second POST for the
//!   same page raises `IntegrityError`, which `handle_exception` maps to
//!   400 `{"error":"The payload is not valid"}` — not a 409.
//! * QUIRK-raw-html-gate (`base.py:560-565`): `page_transaction` publishes
//!   when the RAW `request.data` HTML is truthy (even whitespace-only,
//!   which the serializer strips to `""` before storing) with the raw
//!   value as `new_description_html`; absent/empty publishes nothing.
//! * QUIRK-version-always (`base.py:568-572`): `track_page_version`
//!   publishes on every successful PATCH, even `{}` with no fields.
//! * QUIRK-non-dict-body: a non-object PATCH body answers 400
//!   `{"non_field_errors":["Invalid data. Expected a dictionary, but got
//!   <Type>."]}` (`serializers.py`, `NoneType`/`bool`/`int`/`float`/
//!   `str`/`list`); unknown keys are silently ignored.
//! * QUIRK-blank-char (`fields.py` `CharField.run_validation`): a
//!   whitespace-only binary/HTML string validates to `""` (stored empty,
//!   stripped `NULL`); `null` fails with `"This field may not be null."`;
//!   bools/lists/dicts fail with `"Not a valid string."`; numbers coerce
//!   via `str()`; `description_json` accepts any JSON value as-is.
//! * QUIRK-double-now (`base.py:335` vs `:337`): the archive CTE stamps
//!   `datetime.now()` and the 200 body renders `str(datetime.now())`
//!   from a SECOND call, so the body timestamp may differ from the stored
//!   `archived_at` by microseconds. Two `now()` calls are ported, not one.
//! * QUIRK-access-default (`base.py:272`): the access endpoint defaults a
//!   missing `access` key to `0`, so posting `{}` resets the page to
//!   public; the guard compares against the stored value, so the absent
//!   key never denies.
//! * QUIRK-member-fallthrough (`base.py:317-322,348-353`): the
//!   archive/unarchive guard denies only when the requester IS an active
//!   member with `role <= 15` and is not the owner — an admin (`role 20`)
//!   or a non-member falls through to the action (the permission class
//!   normally gates first).
//!
//! Task delivery: `serve` carries no AMQP publisher (only the worker does),
//! so `.delay()` calls enqueue a [`pidash_jobs::queue::NewJob`] into
//! `rust_job_queue`; the worker forwards Python-owned names to the broker.
//! Enqueue is best-effort after commit: a missing queue table must not turn
//! user-visible writes into 500s, so failures are traced and the response
//! stands.

pub mod gate;

use axum::extract::{Path, Request, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Router};
use bytes::Bytes;
use http_body_util::channel::Channel;
use http_body_util::BodyExt;
use serde_json::{Map, Value};
use sqlx::PgPool;
use std::convert::Infallible;

use crate::middleware::SessionHandle;
use crate::state::AppState;

// ---------------------------------------------------------------------------
// Exact bodies
// ---------------------------------------------------------------------------

/// DRF `IsAuthenticated` denial (401): anonymous callers on every route.
pub const UNAUTHENTICATED_BODY: &str =
    r#"{"detail":"Authentication credentials were not provided."}"#;
/// `handle_exception`'s `ObjectDoesNotExist` branch (404): scoped-lookup
/// misses — unknown/cross-workspace/soft-deleted page or favorite id.
pub const OBJECT_NOT_FOUND_BODY: &str = r#"{"error":"The required object does not exist."}"#;
/// `handle_exception`'s generic 500 branch.
pub const SERVER_ERROR_BODY: &str = r#"{"error":"Something went wrong please try again later"}"#;
/// `Project.resolve` miss (`db/models/project.py:213-217`): verbatim
/// `Http404` args through DRF's handler (no trailing period).
pub const PROJECT_NOT_FOUND_BODY: &str = r#"{"detail":"Project not found"}"#;
/// `handle_exception`'s `IntegrityError` branch (400): duplicate favorites
/// (and impossible FK writes) land here, never a 409.
pub const INVALID_PAYLOAD_BODY: &str = r#"{"error":"The payload is not valid"}"#;
/// Description PATCH on a locked page (`base.py:536-542`,
/// `ERROR_CODES["PAGE_LOCKED"] == 4701`).
pub const PAGE_LOCKED_BODY: &str = r#"{"error_code":4701,"error_message":"PAGE_LOCKED"}"#;
/// Description PATCH on an archived page (`base.py:544-550`,
/// `ERROR_CODES["PAGE_ARCHIVED"] == 4702`).
pub const PAGE_ARCHIVED_BODY: &str = r#"{"error_code":4702,"error_message":"PAGE_ARCHIVED"}"#;
/// Description PATCH success (`base.py:573`).
pub const DESCRIPTION_UPDATED_BODY: &str = r#"{"message":"Updated successfully"}"#;
/// Django `ValidationError` branch (`app/views/base.py:126-130`): what
/// non-UUID parent lookups render on the write paths.
pub const VALID_DETAIL_BODY: &str = r#"{"error":"Please provide valid detail"}"#;
/// Streaming GET content type (`base.py:517`).
pub const DESCRIPTION_CONTENT_TYPE: &str = "application/octet-stream";
/// Streaming GET disposition (`base.py:518`).
pub const DESCRIPTION_CONTENT_DISPOSITION: &str = r#"attachment; filename="page_description.bin""#;

/// `UserFavorite.entity_type` for page favorites (`base.py:479,490`).
pub const FAVORITE_ENTITY_TYPE: &str = "page";
/// `UserFavorite.sequence` default (`db/models/favorite.py:20`) and the
/// `+10000` step the `save()` override adds to the workspace max (`:45`).
pub const FAVORITE_DEFAULT_SEQUENCE: f64 = 65535.0;
/// Sequence step (`db/models/favorite.py:45`).
pub const FAVORITE_SEQUENCE_STEP: f64 = 10000.0;
/// Archive owner-or-admin denial (`base.py:317-326`).
pub const ARCHIVE_OWNER_ADMIN_BODY: &str =
    r#"{"error":"Only the owner or admin can archive the page"}"#;
/// Unarchive owner-or-admin denial (`base.py:348-357`; "un archive" with a
/// space, ported verbatim).
pub const UNARCHIVE_OWNER_ADMIN_BODY: &str =
    r#"{"error":"Only the owner or admin can un archive the page"}"#;
/// Owner-only access-change denial (`base.py:281-285`; the same body also
/// covers `partial_update` at `:176-180` and its `DoesNotExist` branch at
/// `:196-200`).
pub const ACCESS_OWNER_BODY: &str =
    r#"{"error":"Access cannot be updated since this page is owned by someone else"}"#;
/// The archive/unarchive recursive CTE (`base.py:59-72`), executed with
/// params `($1, $2) = (page_id, archived_at)`: `archived_at` is the first
/// `now()` on archive (`:335`) and `NULL` on unarchive (`:364`).
/// `archived_at` is a `DateField` (`db/models/page.py:47`), so the stamp
/// binds as a date (Django truncates its `datetime.now()` on write).
/// Same statement as
/// [`pidash_services::app_pages::queries::archive_cte_sql`] with sqlx
/// positional placeholders; the unit test pins them equivalent.
pub const STATE_CTE_SQL: &str = "WITH RECURSIVE descendants AS (SELECT id FROM pages WHERE id = $1 UNION ALL SELECT pages.id FROM pages, descendants WHERE pages.parent_id = descendants.id) UPDATE pages SET archived_at = $2 WHERE id IN (SELECT id FROM descendants)";

// ---------------------------------------------------------------------------
// Denial
// ---------------------------------------------------------------------------

/// Handler failure with its exact status + body.
#[derive(Debug)]
pub enum Denial {
    /// 401, DRF `NotAuthenticated`.
    Unauthorized,
    /// 403, `@allow_permission` fallthrough
    /// (`"You don't have the required permissions."`). Class denials flow
    /// through [`gate::Denial`]'s own renderer instead.
    Forbidden,
    /// 404, `ObjectDoesNotExist` branch.
    ObjectNotFound,
    /// 404, `Project.resolve` miss.
    ProjectNotFound,
    /// 400, `{"detail": ...}` (body parse errors).
    BadDetail(String),
    /// 400, `{"error": ...}` (view-inline / integrity branch).
    BadError(String),
    /// 400, pre-rendered serializer-errors body (`{"field": [...]}`).
    BadJson(Value),
    /// 400, locked description PATCH.
    PageLocked,
    /// 400, archived description PATCH.
    PageArchived,
    /// 400, Django `ValidationError` branch (`handle_exception`,
    /// `app/views/base.py:126-130`): non-UUID parent lookups and other ORM
    /// validation failures on the write paths.
    ValidDetail,
    /// 403, `{"error": ...}` view-inline denials (destroy owner-or-admin).
    ForbiddenError(String),
    /// 500, generic branch.
    ServerError,
}

impl Denial {
    fn status_and_body(&self) -> (StatusCode, String) {
        match self {
            Denial::Unauthorized => (StatusCode::UNAUTHORIZED, UNAUTHENTICATED_BODY.to_owned()),
            Denial::Forbidden => (
                StatusCode::FORBIDDEN,
                crate::permissions::PERMISSION_DENIED_BODY.to_owned(),
            ),
            Denial::ObjectNotFound => (StatusCode::NOT_FOUND, OBJECT_NOT_FOUND_BODY.to_owned()),
            Denial::ProjectNotFound => (StatusCode::NOT_FOUND, PROJECT_NOT_FOUND_BODY.to_owned()),
            Denial::BadDetail(message) => (
                StatusCode::BAD_REQUEST,
                format!("{{\"detail\":{}}}", json_string(message)),
            ),
            Denial::BadError(message) => (
                StatusCode::BAD_REQUEST,
                format!("{{\"error\":{}}}", json_string(message)),
            ),
            Denial::BadJson(body) => (
                StatusCode::BAD_REQUEST,
                serde_json::to_string(body).expect("serializable denial"),
            ),
            Denial::PageLocked => (StatusCode::BAD_REQUEST, PAGE_LOCKED_BODY.to_owned()),
            Denial::PageArchived => (StatusCode::BAD_REQUEST, PAGE_ARCHIVED_BODY.to_owned()),
            Denial::ValidDetail => (StatusCode::BAD_REQUEST, VALID_DETAIL_BODY.to_owned()),
            Denial::ForbiddenError(message) => (
                StatusCode::FORBIDDEN,
                format!("{{\"error\":{}}}", json_string(message)),
            ),
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
        .expect("json response")
}

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------

/// An owned path: the owned methods serve from Rust, every other method
/// falls through to Django (its 405-after-auth, metadata, and CSRF-failure
/// responses live there — answering 405 in Rust would mistranslate the
/// body). `HEAD` rides axum's `get` handling like Django's `GET`-backed
/// `HEAD` on the description path; on the favorite path both sides 405 it.
fn owned(
    handler: axum::routing::MethodRouter<AppState>,
    unowned: &[&str],
) -> axum::routing::MethodRouter<AppState> {
    let mut router = handler;
    for method in unowned {
        router = match *method {
            "POST" => router.post(crate::edge::proxy),
            "PUT" => router.put(crate::edge::proxy),
            "PATCH" => router.patch(crate::edge::proxy),
            "DELETE" => router.delete(crate::edge::proxy),
            "OPTIONS" => router.options(crate::edge::proxy),
            _ => router.get(crate::edge::proxy),
        };
    }
    router
}

/// Register the favorites + description + state-op + write routes
/// (`app/urls/page.py`). Sibling D-30 handler issues extend this router
/// with their own paths; merges keep both sides.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/workspaces/{slug}/projects/{project_id}/favorite-pages/{page_id}/",
            owned(
                axum::routing::post(favorite_create).delete(favorite_destroy),
                &["GET", "PUT", "PATCH", "OPTIONS"],
            ),
        )
        .route(
            "/api/workspaces/{slug}/projects/{project_id}/pages/{page_id}/description/",
            owned(
                axum::routing::get(description_retrieve).patch(description_partial_update),
                &["POST", "PUT", "DELETE", "OPTIONS"],
            ),
        )
        .route(
            "/api/workspaces/{slug}/projects/{project_id}/pages/{page_id}/archive/",
            owned(
                axum::routing::post(archive_page).delete(unarchive_page),
                &["GET", "PUT", "PATCH", "OPTIONS"],
            ),
        )
        .route(
            "/api/workspaces/{slug}/projects/{project_id}/pages/{page_id}/lock/",
            owned(
                axum::routing::post(lock_page).delete(unlock_page),
                &["GET", "PUT", "PATCH", "OPTIONS"],
            ),
        )
        .route(
            "/api/workspaces/{slug}/projects/{project_id}/pages/{page_id}/access/",
            owned(
                axum::routing::post(access_page),
                &["GET", "PUT", "PATCH", "DELETE", "OPTIONS"],
            ),
        )
        .route(
            "/api/workspaces/{slug}/projects/{project_id}/pages/",
            owned(
                axum::routing::post(page_create),
                &["GET", "PUT", "PATCH", "DELETE", "OPTIONS"],
            ),
        )
        .route(
            "/api/workspaces/{slug}/projects/{project_id}/pages/{page_id}/",
            owned(
                axum::routing::patch(page_partial_update).delete(page_destroy),
                &["GET", "PUT", "POST", "OPTIONS"],
            ),
        )
}

// ---------------------------------------------------------------------------
// Shared request plumbing
// ---------------------------------------------------------------------------

fn pool_of(state: &AppState) -> Result<&PgPool, Denial> {
    state
        .pools()
        .map(|pools| pools.primary())
        .ok_or(Denial::ServerError)
}

/// `request.user` from the Django session (mirrors
/// [`gate`]'s reader: no session, no key, or a non-UUID id means anonymous
/// → 401). `IsAuthenticated` precedes every route here — including the
/// favorite decorator, whose own 403 never fires for anonymous callers.
fn actor_user_id(extension: Option<Extension<SessionHandle>>) -> Result<uuid::Uuid, Denial> {
    let handle = extension.ok_or(Denial::Unauthorized)?.0;
    let mut session = handle.snapshot();
    session
        .get("_auth_user_id")
        .and_then(|value| value.as_str().map(str::to_owned))
        .and_then(|raw| raw.parse::<uuid::Uuid>().ok())
        .ok_or(Denial::Unauthorized)
}

/// `_rewrite_project_kwarg` + `Project.resolve` (`app/views/base.py`,
/// `db/models/project.py:192-219`): UUIDs pass through when the row exists
/// in this workspace; other identifiers match `UPPER(identifier)` after
/// trimming; misses raise `Http404("Project not found")`. Runs for
/// authenticated requests only — anonymous callers 401 before it.
async fn resolve_project_id(pool: &PgPool, slug: &str, raw: &str) -> Result<uuid::Uuid, Denial> {
    if let Ok(id) = raw.parse::<uuid::Uuid>() {
        let row: Option<(uuid::Uuid,)> = sqlx::query_as(
            r#"SELECT p.id FROM projects p
               JOIN workspaces w ON w.id = p.workspace_id
               WHERE p.id = $1 AND w.slug = $2 AND p.deleted_at IS NULL"#,
        )
        .bind(id)
        .bind(slug)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
        return row.map(|row| row.0).ok_or(Denial::ProjectNotFound);
    }
    let upper = raw.trim().to_uppercase();
    let row: Option<(uuid::Uuid,)> = sqlx::query_as(
        r#"SELECT p.id FROM projects p
           JOIN workspaces w ON w.id = p.workspace_id
           WHERE w.slug = $1 AND p.identifier = $2 AND p.deleted_at IS NULL"#,
    )
    .bind(slug)
    .bind(upper)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    row.map(|row| row.0).ok_or(Denial::ProjectNotFound)
}

/// The resolved `@allow_permission([ADMIN, MEMBER])` project context for
/// the favorite endpoints: who acts, in which project and workspace.
pub struct FavoriteGate {
    pub user_id: uuid::Uuid,
    pub project_id: uuid::Uuid,
    pub workspace_id: uuid::Uuid,
}

/// `@allow_permission([ROLE.ADMIN, ROLE.MEMBER])` at `PROJECT` level
/// (`app/permissions/base.py:19-85`): an active project-membership row with
/// role 20/15 passes; otherwise an active membership of any role plus an
/// active workspace-admin row passes; anything else denies with
/// `{"error":"You don't have the required permissions."}`. No page lookup
/// runs here (unlike the `ProjectPagePermission` class): the view creates
/// or deletes the favorite row directly.
async fn favorite_gate(
    pool: &PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    user_id: &uuid::Uuid,
) -> Result<FavoriteGate, Denial> {
    // `role` is a `PositiveSmallIntegerField` (SMALLINT): decode as `i16`.
    let role: Option<(i16,)> = sqlx::query_as(
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
    let workspace_id: Option<(uuid::Uuid,)> = sqlx::query_as(
        r#"SELECT p.workspace_id FROM projects p
           JOIN workspaces w ON w.id = p.workspace_id
           WHERE p.id = $1 AND w.slug = $2 AND p.deleted_at IS NULL"#,
    )
    .bind(project_id)
    .bind(slug)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    // `resolve_project_id` guarantees the project row; the lookup above
    // re-reads it so the gate owns its workspace id.
    use pidash_auth::permissions::{ROLE_ADMIN, ROLE_MEMBER};
    let workspace_id = workspace_id.map(|row| row.0).ok_or(Denial::ServerError)?;
    match role.map(|row| i32::from(row.0)) {
        Some(ROLE_ADMIN) | Some(ROLE_MEMBER) => Ok(FavoriteGate {
            user_id: *user_id,
            project_id: *project_id,
            workspace_id,
        }),
        Some(_) => {
            let admin: Option<(i16,)> = sqlx::query_as(
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
            if admin.is_some_and(|row| i32::from(row.0) == ROLE_ADMIN) {
                Ok(FavoriteGate {
                    user_id: *user_id,
                    project_id: *project_id,
                    workspace_id,
                })
            } else {
                Err(Denial::Forbidden)
            }
        }
        None => Err(Denial::Forbidden),
    }
}

/// Best-effort post-commit task enqueue (the D-02 intake pattern): without
/// a queue table the response still stands.
async fn enqueue_best_effort(pool: &PgPool, job: &pidash_jobs::queue::NewJob) {
    if let Err(error) = pidash_jobs::queue::enqueue(pool, job).await {
        tracing::warn!(%error, task = job.task.as_str(), "task enqueue failed; response stands");
    }
}

/// Parse `<uuid:page_id>`: a mismatch proxies to Django's resolver 404
/// (routing precedes auth — the `app_cycles` precedent).
fn parse_page_id(raw: &str) -> Result<uuid::Uuid, ()> {
    raw.parse::<uuid::Uuid>().map_err(|_| ())
}

// ---------------------------------------------------------------------------
// Favorites
// ---------------------------------------------------------------------------

/// `POST .../favorite-pages/<page_id>/` (`PageFavoriteViewSet.create`,
/// `base.py:472-483`).
pub async fn favorite_create(
    State(state): State<AppState>,
    Path((slug, project_raw, page_raw)): Path<(String, String, String)>,
    extension: Option<Extension<SessionHandle>>,
    req: Request,
) -> Response {
    let Ok(page_id) = parse_page_id(&page_raw) else {
        return crate::edge::proxy(State(state), req).await;
    };
    let user_id = match actor_user_id(extension) {
        Ok(id) => id,
        Err(denial) => return denial.into_response(),
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let project_id = match resolve_project_id(pool, &slug, &project_raw).await {
        Ok(id) => id,
        Err(denial) => return denial.into_response(),
    };
    let gate = match favorite_gate(pool, &slug, &project_id, &user_id).await {
        Ok(gate) => gate,
        Err(denial) => return denial.into_response(),
    };
    // `WorkspaceBaseModel.save` (`db/models/workspace.py:192-195`): the
    // workspace comes from the project row (already resolved above).
    // `UserFavorite.save` (`db/models/favorite.py:40-47`): `sequence` is
    // the workspace max plus 10000 over live rows, else the 65535 default.
    let max: Option<(Option<f64>,)> = match sqlx::query_as(
        r#"SELECT MAX(uf.sequence) FROM user_favorites uf WHERE uf.workspace_id = $1 AND uf.deleted_at IS NULL"#,
    )
    .bind(gate.workspace_id)
    .fetch_optional(pool)
    .await
    {
        Ok(row) => row,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let sequence = max
        .and_then(|row| row.0)
        .map(|top| top + FAVORITE_SEQUENCE_STEP)
        .unwrap_or(FAVORITE_DEFAULT_SEQUENCE);
    // Full-column INSERT in audit-then-definition order
    // (`db/mixins.py`, `db/models/favorite.py:14-38`): `created_by` is the
    // request user (`BaseModel.save` via crum), `updated_by` stays null on
    // create, `name`/`parent` stay null, `is_folder` false.
    let insert = sqlx::query(
        r#"INSERT INTO user_favorites
           (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at,
            workspace_id, project_id, user_id, entity_type, entity_identifier,
            name, is_folder, sequence, parent_id)
           VALUES ($1, now(), now(), $2, NULL, NULL, $3, $4, $5, 'page', $6, NULL, false, $7, NULL)"#,
    )
    .bind(uuid::Uuid::new_v4())
    .bind(gate.user_id)
    .bind(gate.workspace_id)
    .bind(gate.project_id)
    .bind(gate.user_id)
    .bind(page_id)
    .bind(sequence)
    .execute(pool)
    .await;
    match insert {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => {
            // `handle_exception`'s `IntegrityError` branch (`app/views/base.py`):
            // duplicate (entity_type, entity_identifier, user) rows — and
            // impossible FK writes — answer 400, never 409.
            let integrity = error
                .as_database_error()
                .and_then(|db| db.code())
                .is_some_and(|code| code.starts_with("23"));
            if integrity {
                Denial::BadError("The payload is not valid".to_owned()).into_response()
            } else {
                Denial::ServerError.into_response()
            }
        }
    }
}

/// `DELETE .../favorite-pages/<page_id>/` (`PageFavoriteViewSet.destroy`,
/// `base.py:485-495`): the project/user/workspace/entity scoped fetch, then
/// a HARD delete (`delete(soft=False)` — the row is gone, no `deleted_at`
/// stamp, no follow-up task).
pub async fn favorite_destroy(
    State(state): State<AppState>,
    Path((slug, project_raw, page_raw)): Path<(String, String, String)>,
    extension: Option<Extension<SessionHandle>>,
    req: Request,
) -> Response {
    let Ok(page_id) = parse_page_id(&page_raw) else {
        return crate::edge::proxy(State(state), req).await;
    };
    let user_id = match actor_user_id(extension) {
        Ok(id) => id,
        Err(denial) => return denial.into_response(),
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let project_id = match resolve_project_id(pool, &slug, &project_raw).await {
        Ok(id) => id,
        Err(denial) => return denial.into_response(),
    };
    let gate = match favorite_gate(pool, &slug, &project_id, &user_id).await {
        Ok(gate) => gate,
        Err(denial) => return denial.into_response(),
    };
    let row: Option<(uuid::Uuid,)> = match sqlx::query_as(
        r#"SELECT uf.id FROM user_favorites uf
           JOIN workspaces w ON w.id = uf.workspace_id
           WHERE uf.project_id = $1 AND uf.user_id = $2 AND w.slug = $3
           AND uf.entity_identifier = $4 AND uf.entity_type = 'page'
           AND uf.deleted_at IS NULL"#,
    )
    .bind(gate.project_id)
    .bind(gate.user_id)
    .bind(slug.as_str())
    .bind(page_id)
    .fetch_optional(pool)
    .await
    {
        Ok(row) => row,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let Some((favorite_id,)) = row else {
        return Denial::ObjectNotFound.into_response();
    };
    if sqlx::query(r#"DELETE FROM user_favorites WHERE id = $1"#)
        .bind(favorite_id)
        .execute(pool)
        .await
        .is_err()
    {
        return Denial::ServerError.into_response();
    }
    StatusCode::NO_CONTENT.into_response()
}

// ---------------------------------------------------------------------------
// State ops (archive / unarchive / lock / unlock / access)
// ---------------------------------------------------------------------------

/// One project-scoped state-op row (`base.py:247-252` lock, `:260-264`
/// unlock, `:273-278` access, `:309-314` archive, `:341-345` unarchive):
/// `Page.objects.get(pk, workspace__slug, projects__id,
/// project_pages__deleted_at__isnull=True)` — the default manager drops
/// soft-deleted pages. A miss is `DoesNotExist` → 404
/// ([`OBJECT_NOT_FOUND_BODY`]).
#[derive(Debug, sqlx::FromRow)]
struct StatePage {
    owned_by_id: uuid::Uuid,
    access: i16,
    parent_id: Option<uuid::Uuid>,
    description_html: String,
}

async fn fetch_state_page(
    pool: &PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    page_id: &uuid::Uuid,
) -> Result<Option<StatePage>, Denial> {
    sqlx::query_as(
        r#"SELECT p.owned_by_id, p.access, p.parent_id, p.description_html
           FROM pages p
           JOIN workspaces w ON w.id = p.workspace_id
           JOIN project_pages pp ON pp.page_id = p.id
               AND pp.project_id = $2 AND pp.deleted_at IS NULL
           WHERE p.id = $1 AND w.slug = $3 AND p.deleted_at IS NULL"#,
    )
    .bind(page_id)
    .bind(project_id)
    .bind(slug)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)
}

/// The archive/unarchive owner-or-admin-15 probe (`base.py:318-320`,
/// `:349-351`): an active membership row with `role <= 15` for this
/// project — no workspace scoping, ported as observed. Admins (`role 20`)
/// never match it; non-members match nothing (both fall through to the
/// action; the permission class normally gates first).
async fn member_lte15_exists(
    pool: &PgPool,
    project_id: &uuid::Uuid,
    user_id: &uuid::Uuid,
) -> Result<bool, Denial> {
    // `role` is a `PositiveSmallIntegerField` (SMALLINT); the comparison
    // stays in SQL so no decode is needed.
    let row: (bool,) = sqlx::query_as(
        r#"SELECT EXISTS(SELECT 1 FROM project_members pm
           WHERE pm.project_id = $1 AND pm.member_id = $2
           AND pm.is_active AND pm.role <= 15 AND pm.deleted_at IS NULL)"#,
    )
    .bind(project_id)
    .bind(user_id)
    .fetch_one(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(row.0)
}

/// `str(datetime.now())` (`base.py:337`): naive local time rendered with
/// a space separator and microseconds only when nonzero (CPython omits
/// `.000000`).
pub fn python_datetime_string(stamp: &chrono::NaiveDateTime) -> String {
    use chrono::Timelike;
    let base = stamp.format("%Y-%m-%d %H:%M:%S").to_string();
    let micros = stamp.nanosecond() / 1_000;
    if micros == 0 {
        base
    } else {
        format!("{base}.{micros:06}")
    }
}

/// The archive 200 body from the SECOND `now()` call (`base.py:337`,
/// QUIRK-double-now).
pub fn archived_at_body(second_now: &chrono::NaiveDateTime) -> String {
    format!(
        "{{\"archived_at\":{}}}",
        json_string(&python_datetime_string(second_now))
    )
}

/// The guard-domain coercion for the `access` request value
/// (`base.py:281`): the guard compares the raw value against the stored
/// integer with Python `!=`, so only values that can numerically equal a
/// stored integer coerce — JSON integers, integral floats and bools
/// (`1.0` and `true` equal `1`); strings, non-integral floats, nulls,
/// lists and dicts never equal an integer (the caller maps them to a
/// never-equal sentinel).
pub fn access_int(value: &Value) -> Option<i32> {
    if let Some(int) = value.as_i64() {
        return i32::try_from(int).ok();
    }
    if let Some(float) = value.as_f64() {
        if float.fract() == 0.0 && float >= f64::from(i32::MIN) && float <= f64::from(i32::MAX) {
            return Some(float as i32);
        }
        return None;
    }
    value.as_bool().map(i32::from)
}

/// The assignment-domain coercion for the `access` request value
/// (`base.py:272,287`): `page.access = access` runs the
/// `PositiveSmallIntegerField` prep, i.e. `int(value)`, and `save()`
/// skips validators — so non-integral floats truncate toward zero
/// (`1.5` stores `1`), numeric strings parse (`"1"` stores `1`), and
/// anything `int()` rejects (null, lists, dicts, `"abc"`, `"1.5"`,
/// out-of-range integers) fails at prep or at the SMALLINT column, both
/// surfacing as 500. `None` (absent key) defaults to `0`. A `None` return
/// is the 500 branch; out-of-`i16` integers stay `i32` here so the
/// database itself rejects them exactly like Django's column does.
pub fn access_assign(raw: Option<&Value>) -> Option<i32> {
    let value = match raw {
        None => return Some(0),
        Some(value) => value,
    };
    if let Some(int) = value.as_i64() {
        return i32::try_from(int).ok();
    }
    if let Some(float) = value.as_f64() {
        if float >= f64::from(i32::MIN) && float <= f64::from(i32::MAX) {
            return Some(float as i32);
        }
        return None;
    }
    if let Some(boolean) = value.as_bool() {
        return Some(i32::from(boolean));
    }
    if let Some(text) = value.as_str() {
        return access_int_str(text);
    }
    None
}

/// `int(text, 10)` for the assignment path above (ASCII-digit subset —
/// the API domain): surrounding whitespace stripped, one optional sign,
/// digits with single separators between them (`"1_0"` is `10`);
/// anything else (`""`, `"0x1"`, `"1.5"`, `"1__0"`) is not an integer.
fn access_int_str(raw: &str) -> Option<i32> {
    let text = raw.trim_matches(|c: char| c.is_whitespace());
    let digits = text
        .strip_prefix('+')
        .or_else(|| text.strip_prefix('-'))
        .unwrap_or(text);
    if digits.is_empty()
        || !digits.bytes().all(|b| b.is_ascii_digit() || b == b'_')
        || !digits.bytes().next().is_some_and(|b| b.is_ascii_digit())
        || !digits.bytes().last().is_some_and(|b| b.is_ascii_digit())
        || digits.as_bytes().windows(2).any(|w| w == b"__")
    {
        return None;
    }
    let mut magnitude: i64 = 0;
    for byte in digits.bytes() {
        if byte == b'_' {
            continue;
        }
        magnitude = magnitude
            .checked_mul(10)?
            .checked_add(i64::from(byte - b'0'))?;
    }
    let signed = if text.starts_with('-') {
        -magnitude
    } else {
        magnitude
    };
    i32::try_from(signed).ok()
}

/// `Page.save()`'s `description_stripped` recompute
/// (`db/models/page.py:70-77`): `None` when the stored HTML is empty,
/// else `strip_tags`. Every state-op write below is a plain `save()`, so
/// it rewrites the column with the recomputed value (a no-op here — the
/// HTML is untouched — but ported so the statement writes what Django
/// writes).
fn stripped_of(html: &str) -> Option<String> {
    if html.is_empty() {
        None
    } else {
        Some(crate::space::sanitize::strip_tags(html))
    }
}

/// `request.data` for the access endpoint (`base.py:272`): empty bodies
/// validate as `{}`; anything else must parse as JSON.
async fn read_json_body(req: Request) -> Result<Value, Denial> {
    let (_parts, body) = req.into_parts();
    let bytes = match body.collect().await {
        Ok(collected) => collected.to_bytes(),
        Err(_) => return Err(Denial::ServerError),
    };
    if bytes.is_empty() {
        return Ok(Value::Object(Map::new()));
    }
    serde_json::from_slice(&bytes).map_err(|_| Denial::BadDetail("JSON parse error".to_owned()))
}

/// `POST .../pages/<page_id>/archive/` (`PageViewSet.archive`,
/// `base.py:308-337`): the owner-or-admin-15 guard, the favorites
/// soft-delete (`:328-333`, a queryset `.delete()` → `deleted_at` stamp,
/// no task), the CTE with the FIRST `now()` (`:335`), and the 200 body
/// from the SECOND `now()` (`:337`, QUIRK-double-now).
pub async fn archive_page(
    State(state): State<AppState>,
    Path((slug, project_raw, page_raw)): Path<(String, String, String)>,
    extension: Option<Extension<SessionHandle>>,
    req: Request,
) -> Response {
    let Ok(page_id) = parse_page_id(&page_raw) else {
        return crate::edge::proxy(State(state), req).await;
    };
    // `IsAuthenticated` precedes the project rewrite: anonymous callers
    // 401 here, never 404 on the project id. The gate re-reads the same
    // session user below.
    if let Err(denial) = actor_user_id(extension.clone()) {
        return denial.into_response();
    }
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let project_id = match resolve_project_id(pool, &slug, &project_raw).await {
        Ok(id) => id,
        Err(denial) => return denial.into_response(),
    };
    let gate = match gate::resolve_gate(
        &state,
        "POST",
        &slug,
        &project_id,
        Some(page_id),
        extension,
    )
    .await
    {
        Ok(gate) => gate,
        Err(denial) => return denial.into_response(),
    };
    let page = match fetch_state_page(pool, &slug, &project_id, &page_id).await {
        Ok(page) => page,
        Err(denial) => return denial.into_response(),
    };
    let Some(page) = page else {
        return Denial::ObjectNotFound.into_response();
    };
    let is_owner = page.owned_by_id == gate.user_id;
    let lte15 = match member_lte15_exists(pool, &project_id, &gate.user_id).await {
        Ok(lte15) => lte15,
        Err(denial) => return denial.into_response(),
    };
    if gate::check_archive(is_owner, lte15) == gate::StateOpOutcome::Deny {
        return json_response(StatusCode::BAD_REQUEST, ARCHIVE_OWNER_ADMIN_BODY.to_owned());
    }
    if sqlx::query(
        r#"UPDATE user_favorites SET deleted_at = now()
           WHERE entity_type = 'page' AND entity_identifier = $1 AND project_id = $2
           AND workspace_id = (SELECT id FROM workspaces WHERE slug = $3)
           AND deleted_at IS NULL"#,
    )
    .bind(page_id)
    .bind(project_id)
    .bind(slug.as_str())
    .execute(pool)
    .await
    .is_err()
    {
        return Denial::ServerError.into_response();
    }
    let now_db = chrono::Utc::now().naive_utc();
    if sqlx::query(STATE_CTE_SQL)
        .bind(page_id)
        .bind(now_db.date())
        .execute(pool)
        .await
        .is_err()
    {
        return Denial::ServerError.into_response();
    }
    let now_body = chrono::Utc::now().naive_utc();
    json_response(StatusCode::OK, archived_at_body(&now_body))
}

/// `DELETE .../pages/<page_id>/archive/` (`PageViewSet.unarchive`,
/// `base.py:339-366`): the same guard ("un archive" body), the
/// parent-archived detach (`:360-362`), the CTE with `NULL` (`:364`), 204.
pub async fn unarchive_page(
    State(state): State<AppState>,
    Path((slug, project_raw, page_raw)): Path<(String, String, String)>,
    extension: Option<Extension<SessionHandle>>,
    req: Request,
) -> Response {
    let Ok(page_id) = parse_page_id(&page_raw) else {
        return crate::edge::proxy(State(state), req).await;
    };
    // `IsAuthenticated` precedes the project rewrite: anonymous callers
    // 401 here, never 404 on the project id. The gate re-reads the same
    // session user below.
    if let Err(denial) = actor_user_id(extension.clone()) {
        return denial.into_response();
    }
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let project_id = match resolve_project_id(pool, &slug, &project_raw).await {
        Ok(id) => id,
        Err(denial) => return denial.into_response(),
    };
    let gate = match gate::resolve_gate(
        &state,
        "DELETE",
        &slug,
        &project_id,
        Some(page_id),
        extension,
    )
    .await
    {
        Ok(gate) => gate,
        Err(denial) => return denial.into_response(),
    };
    let page = match fetch_state_page(pool, &slug, &project_id, &page_id).await {
        Ok(page) => page,
        Err(denial) => return denial.into_response(),
    };
    let Some(page) = page else {
        return Denial::ObjectNotFound.into_response();
    };
    let is_owner = page.owned_by_id == gate.user_id;
    let lte15 = match member_lte15_exists(pool, &project_id, &gate.user_id).await {
        Ok(lte15) => lte15,
        Err(denial) => return denial.into_response(),
    };
    if gate::check_unarchive(is_owner, lte15) == gate::StateOpOutcome::Deny {
        return json_response(
            StatusCode::BAD_REQUEST,
            UNARCHIVE_OWNER_ADMIN_BODY.to_owned(),
        );
    }
    // Detach when the parent is still archived (`:360-362`): unarchiving
    // a child of an archived parent breaks the hierarchy — the page is
    // reparented to the top before the CTE clears its own subtree, and
    // the archived parent keeps its timestamp.
    if let Some(parent_id) = page.parent_id {
        let parent: Option<(Option<chrono::NaiveDate>,)> = match sqlx::query_as(
            r#"SELECT p.archived_at FROM pages p WHERE p.id = $1 AND p.deleted_at IS NULL"#,
        )
        .bind(parent_id)
        .fetch_optional(pool)
        .await
        {
            Ok(parent) => parent,
            Err(_) => return Denial::ServerError.into_response(),
        };
        // A dangling `parent_id` (row gone) raises `DoesNotExist` on
        // `page.parent` in Django → the 404 branch, ported as observed.
        let Some((parent_archived,)) = parent else {
            return Denial::ObjectNotFound.into_response();
        };
        if parent_archived.is_some()
            && sqlx::query(
                r#"UPDATE pages SET parent_id = NULL, updated_at = now(), updated_by_id = $1
                   WHERE id = $2"#,
            )
            .bind(gate.user_id)
            .bind(page_id)
            .execute(pool)
            .await
            .is_err()
        {
            return Denial::ServerError.into_response();
        }
    }
    if sqlx::query(STATE_CTE_SQL)
        .bind(page_id)
        .bind(None::<chrono::NaiveDate>)
        .execute(pool)
        .await
        .is_err()
    {
        return Denial::ServerError.into_response();
    }
    StatusCode::NO_CONTENT.into_response()
}

/// `POST .../pages/<page_id>/lock/` (`PageViewSet.lock`,
/// `base.py:246-256`): `is_locked = True` plus `save()`, 204.
pub async fn lock_page(
    State(state): State<AppState>,
    Path((slug, project_raw, page_raw)): Path<(String, String, String)>,
    extension: Option<Extension<SessionHandle>>,
    req: Request,
) -> Response {
    write_lock(
        State(state),
        slug,
        project_raw,
        page_raw,
        extension,
        req,
        true,
    )
    .await
}

/// `DELETE .../pages/<page_id>/lock/` (`PageViewSet.unlock`,
/// `base.py:258-269`): `is_locked = False` plus `save()`, 204.
pub async fn unlock_page(
    State(state): State<AppState>,
    Path((slug, project_raw, page_raw)): Path<(String, String, String)>,
    extension: Option<Extension<SessionHandle>>,
    req: Request,
) -> Response {
    write_lock(
        State(state),
        slug,
        project_raw,
        page_raw,
        extension,
        req,
        false,
    )
    .await
}

/// The lock/unlock closure over the scoped page fetch (`:246-269`): the
/// two actions differ only in the stored flag and the gate method.
async fn write_lock(
    State(state): State<AppState>,
    slug: String,
    project_raw: String,
    page_raw: String,
    extension: Option<Extension<SessionHandle>>,
    req: Request,
    locked: bool,
) -> Response {
    let Ok(page_id) = parse_page_id(&page_raw) else {
        return crate::edge::proxy(State(state), req).await;
    };
    // `IsAuthenticated` precedes the project rewrite: anonymous callers
    // 401 here, never 404 on the project id. The gate re-reads the same
    // session user below.
    if let Err(denial) = actor_user_id(extension.clone()) {
        return denial.into_response();
    }
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let project_id = match resolve_project_id(pool, &slug, &project_raw).await {
        Ok(id) => id,
        Err(denial) => return denial.into_response(),
    };
    let method = if locked { "POST" } else { "DELETE" };
    let gate = match gate::resolve_gate(
        &state,
        method,
        &slug,
        &project_id,
        Some(page_id),
        extension,
    )
    .await
    {
        Ok(gate) => gate,
        Err(denial) => return denial.into_response(),
    };
    let page = match fetch_state_page(pool, &slug, &project_id, &page_id).await {
        Ok(page) => page,
        Err(denial) => return denial.into_response(),
    };
    let Some(page) = page else {
        return Denial::ObjectNotFound.into_response();
    };
    if sqlx::query(
        r#"UPDATE pages SET is_locked = $1, description_stripped = $2,
           updated_at = now(), updated_by_id = $3 WHERE id = $4"#,
    )
    .bind(locked)
    .bind(stripped_of(page.description_html.as_str()))
    .bind(gate.user_id)
    .bind(page_id)
    .execute(pool)
    .await
    .is_err()
    {
        return Denial::ServerError.into_response();
    }
    StatusCode::NO_CONTENT.into_response()
}

/// `POST .../pages/<page_id>/access/` (`PageViewSet.access`,
/// `base.py:271-289`): the access value defaults to 0 (`:272`, so `{}`
/// posts reset the page to public — QUIRK-access-default); the guard
/// denies a change by a non-owner (`:281-285`); the save writes the value,
/// 204.
pub async fn access_page(
    State(state): State<AppState>,
    Path((slug, project_raw, page_raw)): Path<(String, String, String)>,
    extension: Option<Extension<SessionHandle>>,
    req: Request,
) -> Response {
    let Ok(page_id) = parse_page_id(&page_raw) else {
        return crate::edge::proxy(State(state), req).await;
    };
    // `IsAuthenticated` precedes the project rewrite: anonymous callers
    // 401 here, never 404 on the project id. The gate re-reads the same
    // session user below.
    if let Err(denial) = actor_user_id(extension.clone()) {
        return denial.into_response();
    }
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let project_id = match resolve_project_id(pool, &slug, &project_raw).await {
        Ok(id) => id,
        Err(denial) => return denial.into_response(),
    };
    let gate = match gate::resolve_gate(
        &state,
        "POST",
        &slug,
        &project_id,
        Some(page_id),
        extension,
    )
    .await
    {
        Ok(gate) => gate,
        Err(denial) => return denial.into_response(),
    };
    let page = match fetch_state_page(pool, &slug, &project_id, &page_id).await {
        Ok(page) => page,
        Err(denial) => return denial.into_response(),
    };
    let Some(page) = page else {
        return Denial::ObjectNotFound.into_response();
    };
    let parsed = match read_json_body(req).await {
        Ok(parsed) => parsed,
        Err(denial) => return denial.into_response(),
    };
    let object = match parsed.as_object() {
        Some(object) => object,
        // `request.data.get("access", …)` on a non-object body: Django
        // raises before the view runs — the 400 detail branch, ported as
        // observed on the description PATCH sibling.
        None => return Denial::BadDetail("Invalid request".to_owned()).into_response(),
    };
    // Guard input (`:281`): the absent key defaults to the stored value
    // (never denies); a present key carries its integer, or a
    // never-equal sentinel when it is not an integer (Python `!=`
    // against the stored int is always true there).
    let raw = object.get("access");
    let requested = raw.and_then(access_int);
    let probe = raw.map(|_| requested.unwrap_or(i32::MIN));
    if gate::check_access(page.access.into(), probe, page.owned_by_id == gate.user_id)
        == gate::AccessOutcome::Deny
    {
        return json_response(StatusCode::BAD_REQUEST, ACCESS_OWNER_BODY.to_owned());
    }
    // Assignment (`:272,287`): `request.data.get("access", 0)` then
    // `page.access = access` runs the field prep (`int(value)`, verified
    // against `PositiveSmallIntegerField.get_prep_value`: `1.5` → `1`,
    // `"1"` → `1`). Anything `int()` rejects fails at prep or at the
    // SMALLINT column — both 500 here. The value binds as `i32` so an
    // out-of-range integer is rejected by the column itself, exactly
    // like Django's save.
    let Some(effective) = access_assign(raw) else {
        return Denial::ServerError.into_response();
    };
    if sqlx::query(
        r#"UPDATE pages SET access = $1, description_stripped = $2,
           updated_at = now(), updated_by_id = $3 WHERE id = $4"#,
    )
    .bind(effective)
    .bind(stripped_of(page.description_html.as_str()))
    .bind(gate.user_id)
    .bind(page_id)
    .execute(pool)
    .await
    .is_err()
    {
        return Denial::ServerError.into_response();
    }
    StatusCode::NO_CONTENT.into_response()
}

// ---------------------------------------------------------------------------
// Description
// ---------------------------------------------------------------------------

/// The `Q(owned_by=user) | Q(access=0)` project-scoped page row the
/// description endpoints fetch (`base.py:502-508` retrieve, `:522-528`
/// partial_update): workspace slug, project bridge (live rows only), live
/// page, owner-or-public predicate. A miss is `DoesNotExist` → 404.
async fn fetch_description_page(
    pool: &PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    page_id: &uuid::Uuid,
    user_id: &uuid::Uuid,
) -> Result<Option<DescriptionPage>, Denial> {
    sqlx::query_as(
        r#"SELECT p.is_locked, p.archived_at, p.description_html, p.description_binary, p.description_json
           FROM pages p
           JOIN workspaces w ON w.id = p.workspace_id
           JOIN project_pages pp ON pp.page_id = p.id
               AND pp.project_id = $2 AND pp.deleted_at IS NULL
           WHERE p.id = $1 AND w.slug = $3 AND p.deleted_at IS NULL
           AND (p.owned_by_id = $4 OR p.access = 0)"#,
    )
    .bind(page_id)
    .bind(project_id)
    .bind(slug)
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)
}

/// One description-scoped page row (`archived_at` is a `DateField`).
#[derive(Debug, sqlx::FromRow)]
struct DescriptionPage {
    is_locked: bool,
    archived_at: Option<chrono::NaiveDate>,
    description_html: String,
    description_binary: Option<Vec<u8>>,
    description_json: Option<Value>,
}

/// `GET .../pages/<page_id>/description/` (`PagesDescriptionViewSet.retrieve`,
/// `base.py:501-519`): streams the raw binary — one yield of the stored
/// bytes when truthy, one yield of `b""` when null or empty — with no
/// lock/archived guard (ported as observed; the PATCH below has both).
pub async fn description_retrieve(
    State(state): State<AppState>,
    Path((slug, project_raw, page_raw)): Path<(String, String, String)>,
    extension: Option<Extension<SessionHandle>>,
    req: Request,
) -> Response {
    let Ok(page_id) = parse_page_id(&page_raw) else {
        return crate::edge::proxy(State(state), req).await;
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    // The rewrite runs for authenticated requests only (`_rewrite_project_kwarg`
    // skips the oracle for anonymous callers, who 401 inside the gate): the
    // project value is unused on the anonymous path, so a raw fallback stands in.
    let project_id = if actor_user_id(extension.clone()).is_ok() {
        match resolve_project_id(pool, &slug, &project_raw).await {
            Ok(id) => id,
            Err(denial) => return denial.into_response(),
        }
    } else {
        project_raw
            .parse::<uuid::Uuid>()
            .unwrap_or(uuid::Uuid::nil())
    };
    let gate = match gate::resolve_gate(&state, "GET", &slug, &project_id, Some(page_id), extension)
        .await
    {
        Ok(gate) => gate,
        Err(denial) => return denial.into_response(),
    };
    let page = match fetch_description_page(pool, &slug, &project_id, &page_id, &gate.user_id).await
    {
        Ok(page) => page,
        Err(denial) => return denial.into_response(),
    };
    let Some(page) = page else {
        return Denial::ObjectNotFound.into_response();
    };
    // `stream_data` (`base.py:509-514`): exactly one chunk either way, so
    // the response is chunked with no content-length like Django's
    // `StreamingHttpResponse` (never a `Body::from` with a fixed length).
    let chunk = page.description_binary.unwrap_or_default();
    let (mut sender, channel) = Channel::<Bytes, Infallible>::new(1);
    if sender.send_data(Bytes::from(chunk)).await.is_err() {
        return Denial::ServerError.into_response();
    }
    drop(sender);
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, DESCRIPTION_CONTENT_TYPE)
        .header(header::CONTENT_DISPOSITION, DESCRIPTION_CONTENT_DISPOSITION)
        .body(axum::body::Body::new(channel))
        .expect("streaming description response")
}

// ---------------------------------------------------------------------------
// Description PATCH body (`PageBinaryUpdateSerializer`, `page.py:173-225`)
// ---------------------------------------------------------------------------

/// `PageBinaryUpdateSerializer` field names in declaration order
/// (`page.py:176-178`): error objects preserve this order.
pub const BINARY_FIELD: &str = "description_binary";
/// See [`BINARY_FIELD`].
pub const HTML_FIELD: &str = "description_html";
/// See [`BINARY_FIELD`].
pub const JSON_FIELD: &str = "description_json";

/// A validated PATCH body: each `Some` arm is a present key to overwrite
/// (even with an empty value); `None` arms are untouched
/// (`page.py:213-225`). `raw_html` is the untouched request value, which
/// alone decides the `page_transaction` publish (`base.py:560`).
#[derive(Debug, Clone, PartialEq)]
pub struct ValidPatch {
    pub binary: Option<Vec<u8>>,
    pub html: Option<String>,
    pub json: Option<Option<Value>>,
    pub raw_html: Option<Value>,
}

/// Why a PATCH body fails validation.
#[derive(Debug, Clone, PartialEq)]
pub enum PatchRejection {
    /// Non-object JSON (`serializers.py`: `non_field_errors`).
    NonDict(String),
    /// Per-field errors in declaration order.
    Fields(Vec<(String, String)>),
}

/// Mirror `CharField(required=False, allow_blank=True)` input handling
/// (`fields.py` `CharField.run_validation` + `to_internal_value`):
/// `null` fails, bools/lists/dicts fail, numbers coerce via `str()`, and a
/// value that is empty or whitespace-only validates to `""` (the
/// serializer's own `if not value` guard then passes it through untouched).
fn char_internal(value: &Value) -> Result<String, &'static str> {
    match value {
        Value::Null => Err("This field may not be null."),
        Value::Bool(_) | Value::Array(_) | Value::Object(_) => Err("Not a valid string."),
        Value::Number(number) => {
            let raw = number.to_string();
            Ok(if raw.trim().is_empty() {
                String::new()
            } else {
                raw.trim().to_owned()
            })
        }
        Value::String(text) => Ok(if text.is_empty() || text.trim().is_empty() {
            String::new()
        } else {
            text.trim().to_owned()
        }),
    }
}

/// Mirror `validate_description_html`'s size gate plus
/// `validate_html_content` (`content_validator.py:211-247`): the byte check
/// runs on the post-`CharField` (trimmed) value; the cleaner is
/// `crate::space::sanitize` (`ammonia`, the crate `nh3` binds), and any
/// cleaner failure is `"Failed to sanitize HTML"` (`:242-247`).
fn sanitize_page_html(trimmed: &str) -> Result<String, String> {
    use pidash_services::app_pages::shape::{HTML_SANITIZE_FAILED_MESSAGE, HTML_TOO_LARGE_MESSAGE};
    if trimmed.len() > crate::space::sanitize::MAX_HTML_BYTES {
        return Err(HTML_TOO_LARGE_MESSAGE.to_owned());
    }
    match crate::space::sanitize::sanitize_html(trimmed) {
        crate::space::sanitize::Sanitize::Clean(clean) => Ok(clean),
        crate::space::sanitize::Sanitize::Invalid => Err(HTML_SANITIZE_FAILED_MESSAGE.to_owned()),
    }
}

/// `json.dumps` type name for the non-dictionary error
/// (`serializers.py`: `type(data).__name__`).
fn json_datatype(value: &Value) -> &'static str {
    match value {
        Value::Null => "NoneType",
        Value::Bool(_) => "bool",
        Value::Number(number) if number.is_i64() || number.is_u64() => "int",
        Value::Number(_) => "float",
        Value::String(_) => "str",
        Value::Array(_) => "list",
        Value::Object(_) => "dict",
    }
}

/// Validate a parsed PATCH body against `PageBinaryUpdateSerializer`
/// (`page.py:173-225`): unknown keys ignored, every declared key validated
/// independently with all field errors collected (DRF accumulates, never
/// fail-fast).
pub fn validate_patch_body(body: &Value) -> Result<ValidPatch, PatchRejection> {
    use pidash_services::app_pages::shape::validate_description_binary;
    use pidash_services::app_pages::shape::BinaryFieldOutcome;
    let object = match body.as_object() {
        Some(map) => map,
        None => return Err(PatchRejection::NonDict(json_datatype(body).to_owned())),
    };
    let mut errors: Vec<(String, String)> = Vec::new();
    let mut binary: Option<Vec<u8>> = None;
    let mut html: Option<String> = None;
    let mut json: Option<Option<Value>> = None;
    if let Some(value) = object.get(BINARY_FIELD) {
        match char_internal(value) {
            Err(message) => errors.push((BINARY_FIELD.to_owned(), message.to_owned())),
            Ok(coerced) if coerced.is_empty() => binary = Some(Vec::new()),
            Ok(coerced) => match validate_description_binary(&coerced) {
                Ok(BinaryFieldOutcome::Decoded(bytes)) => binary = Some(bytes),
                Ok(BinaryFieldOutcome::Passthrough) => binary = Some(Vec::new()),
                Err(message) => errors.push((BINARY_FIELD.to_owned(), message)),
            },
        }
    }
    if let Some(value) = object.get(HTML_FIELD) {
        match char_internal(value) {
            Err(message) => errors.push((HTML_FIELD.to_owned(), message.to_owned())),
            Ok(coerced) if coerced.is_empty() => html = Some(String::new()),
            Ok(coerced) => match sanitize_page_html(&coerced) {
                Ok(clean) => html = Some(clean),
                Err(message) => errors.push((HTML_FIELD.to_owned(), message)),
            },
        }
    }
    // `JSONField(required=False, allow_null=True)`: any JSON value passes
    // as-is, `null` stores NULL (`page.py:178`, `fields.py`).
    if let Some(value) = object.get(JSON_FIELD) {
        json = Some(if value.is_null() {
            None
        } else {
            Some(value.clone())
        });
    }
    if !errors.is_empty() {
        return Err(PatchRejection::Fields(errors));
    }
    Ok(ValidPatch {
        binary,
        html,
        json,
        raw_html: object.get(HTML_FIELD).cloned(),
    })
}

/// Render per-field errors (`{"field": ["message"]}`, declaration order).
pub fn field_errors_body(errors: &[(String, String)]) -> Value {
    let mut map = Map::new();
    for (field, message) in errors {
        map.insert(
            field.clone(),
            Value::Array(vec![Value::String(message.clone())]),
        );
    }
    Value::Object(map)
}

/// Render the non-dictionary error
/// (`serializers.py:342`: `"Invalid data. Expected a dictionary, but got
/// {datatype}."` under `non_field_errors`).
pub fn non_dict_body(datatype: &str) -> Value {
    Value::Object(Map::from_iter([(
        "non_field_errors".to_owned(),
        Value::Array(vec![Value::String(format!(
            "Invalid data. Expected a dictionary, but got {datatype}."
        ))]),
    )]))
}

/// Python truthiness of the RAW `description_html` request value
/// (`base.py:560`: `if request.data.get("description_html"):`) — `None`,
/// missing, `""`, `0`, `false`, `[]`, `{}` are falsy; a whitespace-only
/// string is truthy.
pub fn python_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                int != 0
            } else if let Some(uint) = number.as_u64() {
                uint != 0
            } else if let Some(float) = number.as_f64() {
                float != 0.0
            } else {
                true
            }
        }
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
    }
}

/// The description-update `page_transaction` publish (`base.py:560-565`):
/// only when the raw HTML is truthy, with the raw value as
/// `new_description_html` (default `<p></p>` is unreachable — absent is
/// falsy) and the pre-save snapshot as old. String raws go through the
/// merged constructor; non-string truthy raws (numbers, bools, lists,
/// dicts) are passed through verbatim, exactly like `.delay()` does.
pub fn description_page_txn_job(
    page_id: &str,
    old_html: Option<&str>,
    raw: Option<&Value>,
) -> Option<pidash_jobs::queue::NewJob> {
    let raw = raw?;
    if !python_truthy(raw) {
        return None;
    }
    if let Some(text) = raw.as_str() {
        return pidash_jobs::app_pages::page_transaction_save_job(page_id, old_html, Some(text));
    }
    let template =
        pidash_jobs::app_pages::page_transaction_save_message(page_id, old_html, Some("x"))?;
    let mut kwargs = template.kwargs;
    kwargs.insert("new_description_html".to_owned(), raw.clone());
    Some(pidash_jobs::queue::NewJob::new(
        template.task,
        Value::Array(Vec::new()),
        Value::Object(kwargs),
    ))
}

/// `PATCH .../pages/<page_id>/description/` (`PagesDescriptionViewSet.partial_update`,
/// `base.py:521-575`).
pub async fn description_partial_update(
    State(state): State<AppState>,
    Path((slug, project_raw, page_raw)): Path<(String, String, String)>,
    extension: Option<Extension<SessionHandle>>,
    req: Request,
) -> Response {
    let Ok(page_id) = parse_page_id(&page_raw) else {
        return crate::edge::proxy(State(state), req).await;
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let project_id = if actor_user_id(extension.clone()).is_ok() {
        match resolve_project_id(pool, &slug, &project_raw).await {
            Ok(id) => id,
            Err(denial) => return denial.into_response(),
        }
    } else {
        project_raw
            .parse::<uuid::Uuid>()
            .unwrap_or(uuid::Uuid::nil())
    };
    let gate = match gate::resolve_gate(
        &state,
        "PATCH",
        &slug,
        &project_id,
        Some(page_id),
        extension,
    )
    .await
    {
        Ok(gate) => gate,
        Err(denial) => return denial.into_response(),
    };
    let page = match fetch_description_page(pool, &slug, &project_id, &page_id, &gate.user_id).await
    {
        Ok(page) => page,
        Err(denial) => return denial.into_response(),
    };
    let Some(page) = page else {
        return Denial::ObjectNotFound.into_response();
    };
    // Lock first, then archived (`base.py:531-550`), both before the body
    // is even parsed (`request.data` is read at `:554`).
    if page.is_locked {
        return Denial::PageLocked.into_response();
    }
    if page.archived_at.is_some() {
        return Denial::PageArchived.into_response();
    }
    // The old snapshot and its JSON dump precede validation
    // (`base.py:549-552`).
    let old_html = page.description_html.clone();
    let existing_instance = pidash_jobs::app_pages::existing_instance_json(Some(old_html.as_str()));
    // `request.data` (`base.py:554`): empty bodies validate as `{}`.
    let (_parts, body) = req.into_parts();
    let bytes = match body.collect().await {
        Ok(collected) => collected.to_bytes(),
        Err(_) => return Denial::ServerError.into_response(),
    };
    let parsed: Value = if bytes.is_empty() {
        Value::Object(Map::new())
    } else {
        match serde_json::from_slice(&bytes) {
            Ok(value) => value,
            Err(_) => {
                return Denial::BadDetail("JSON parse error".to_owned()).into_response();
            }
        }
    };
    let patch = match validate_patch_body(&parsed) {
        Ok(patch) => patch,
        Err(PatchRejection::NonDict(datatype)) => {
            return Denial::BadJson(non_dict_body(&datatype)).into_response();
        }
        Err(PatchRejection::Fields(errors)) => {
            return Denial::BadJson(field_errors_body(&errors)).into_response();
        }
    };
    // `update()` (`page.py:213-225`) plus `Page.save()`'s
    // `description_stripped` recompute (`db/models/page.py:70-77`: `None`
    // when the final HTML is empty, else `strip_tags`). Django's `save()`
    // writes every column, so the UPDATE is static with computed values.
    let final_binary = patch.binary.or(page.description_binary);
    let final_html = patch.html.unwrap_or(page.description_html);
    let final_json: Option<Value> = match patch.json {
        Some(value) => value,
        None => page.description_json,
    };
    let final_stripped: Option<String> = if final_html.is_empty() {
        None
    } else {
        Some(crate::space::sanitize::strip_tags(final_html.as_str()))
    };
    let final_json_text: Option<String> =
        final_json.map(|value| serde_json::to_string(&value).expect("json serializes"));
    if sqlx::query(
        r#"UPDATE pages SET description_binary = $1, description_html = $2,
           description_json = CAST($3 AS jsonb), description_stripped = $4,
           updated_at = now(), updated_by_id = $5 WHERE id = $6"#,
    )
    .bind(final_binary)
    .bind(final_html.as_str())
    .bind(final_json_text)
    .bind(final_stripped)
    .bind(gate.user_id)
    .bind(page_id)
    .execute(pool)
    .await
    .is_err()
    {
        return Denial::ServerError.into_response();
    }
    // Publishes observe the post-save state (`base.py:556-572`): the
    // conditional `page_transaction`, then the unconditional
    // `track_page_version` — both best-effort after commit.
    let page_id_text = page_id.to_string();
    if let Some(job) = description_page_txn_job(
        &page_id_text,
        Some(old_html.as_str()),
        patch.raw_html.as_ref(),
    ) {
        enqueue_best_effort(pool, &job).await;
    }
    enqueue_best_effort(
        pool,
        &pidash_jobs::app_pages::track_page_version_job(
            &page_id_text,
            &existing_instance,
            &gate.user_id.to_string(),
        ),
    )
    .await;
    json_response(StatusCode::OK, DESCRIPTION_UPDATED_BODY.to_owned())
}

// ---------------------------------------------------------------------------
// Page writes: validation (`PageSerializer` / `PageDetailSerializer`,
// `app/serializers/page.py:25-133` against DRF 3.15.2 `fields.py` +
// `relations.py`)
// ---------------------------------------------------------------------------

/// One field's write errors: a flat message list, or per-index message lists
/// for the `labels` / `label_ids` / `project_ids` list fields (DRF renders
/// those as `{"0": [...], ...}` objects, never arrays).
#[derive(Debug, Clone, PartialEq)]
pub enum FieldError {
    Flat(Vec<String>),
    Indexed(Vec<(usize, Vec<String>)>),
}

/// Serializer errors in `Meta.fields` declaration order (`page.py:38-58`,
/// plus `description_html` for the detail serializer): the 400 body preserves
/// this order.
pub type OrderedErrors = Vec<(String, FieldError)>;

/// Why page-write validation fails: collected field errors (400
/// serializer-errors body), or the Django-`ValidationError` short-circuit
/// (400 [`VALID_DETAIL_BODY`]) when an ORM lookup itself rejects the value
/// — float/list/dict parent or label items, out-of-range int PKs — exactly
/// like the exception propagating out of `to_internal_value` in DRF.
#[derive(Debug, Clone, PartialEq)]
pub enum WriteRejection {
    Fields(OrderedErrors),
    ValidDetail,
}

/// Validation outcome including the database: field-level rejections render
/// 400s, row-fetch failures 500.
#[derive(Debug)]
pub enum PageFailure {
    Invalid(WriteRejection),
    Db(Denial),
}

impl From<Denial> for PageFailure {
    fn from(denial: Denial) -> Self {
        PageFailure::Db(denial)
    }
}

fn failure_response(failure: PageFailure) -> Response {
    match failure {
        PageFailure::Invalid(rejection) => rejection_response(rejection),
        PageFailure::Db(denial) => denial.into_response(),
    }
}

fn push_flat(errors: &mut OrderedErrors, field: &str, message: String) {
    match errors.iter_mut().find(|(name, _)| name == field) {
        Some((_, FieldError::Flat(messages))) => messages.push(message),
        _ => errors.push((field.to_owned(), FieldError::Flat(vec![message]))),
    }
}

fn push_indexed(errors: &mut OrderedErrors, field: &str, index: usize, message: String) {
    match errors.iter_mut().find(|(name, _)| name == field) {
        Some((_, FieldError::Indexed(items))) => {
            match items.iter_mut().find(|(i, _)| *i == index) {
                Some((_, messages)) => messages.push(message),
                None => items.push((index, vec![message])),
            }
        }
        _ => errors.push((
            field.to_owned(),
            FieldError::Indexed(vec![(index, vec![message])]),
        )),
    }
}

fn escape_into(out: &mut String, value: &str) {
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
}

fn quoted(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    escape_into(&mut out, value);
    out.push('"');
    out
}

/// Render the collected serializer errors byte-identically: `{"field":
/// ["message"]}` flat arms, `{"field": {"0": ["message"]}}` indexed arms,
/// fields in declaration order, compact separators, literal UTF-8 (DRF
/// `JSONRenderer` with `ensure_ascii=False`).
pub fn write_errors_body(errors: &[(String, FieldError)]) -> String {
    let mut out = String::from("{");
    for (index, (field, error)) in errors.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str(&quoted(field));
        out.push(':');
        match error {
            FieldError::Flat(messages) => {
                out.push('[');
                for (i, message) in messages.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    out.push_str(&quoted(message));
                }
                out.push(']');
            }
            FieldError::Indexed(items) => {
                out.push('{');
                for (i, (position, messages)) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    out.push_str(&quoted(&position.to_string()));
                    out.push_str(":[");
                    for (j, message) in messages.iter().enumerate() {
                        if j > 0 {
                            out.push(',');
                        }
                        out.push_str(&quoted(message));
                    }
                    out.push(']');
                }
                out.push('}');
            }
        }
    }
    out.push('}');
    out
}

/// Python `str()` of a JSON scalar for `invalid_choice` messages
/// (`'"%s" is not a valid choice' % value`): bools render `True`/`False`,
/// ints plainly, floats Python-style, strings as-is. Compounds never reach
/// these messages in the suites; they fall back to compact JSON.
fn py_scalar(value: &Value) -> String {
    match value {
        Value::Null => "None".to_owned(),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                int.to_string()
            } else if let Some(uint) = number.as_u64() {
                uint.to_string()
            } else if let Some(float) = number.as_f64() {
                crate::paginator::py_float_str(float)
            } else {
                number.to_string()
            }
        }
        Value::String(text) => text.clone(),
        other => serde_json::to_string(other).unwrap_or("null".to_owned()),
    }
}

/// Parse like `uuid.UUID(hex=value)` (`relations.py` delegates there for
/// `str` PKs): optional `urn:`/`uuid:` prefixes stripped, braces trimmed,
/// dashes removed, then exactly 32 hex chars. Uppercase accepted.
fn parse_uuid_hex(raw: &str) -> Option<uuid::Uuid> {
    let mut text = raw.replace("urn:", "").replace("uuid:", "");
    if text.starts_with('{') && text.ends_with('}') && text.len() > 2 {
        text = text[1..text.len() - 1].to_owned();
    }
    let compact: String = text.chars().filter(|c| *c != '-').collect();
    if compact.len() != 32 || !compact.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    uuid::Uuid::parse_str(&compact).ok()
}

/// `"..." is not a valid UUID.` with DRF's curly quotes (`relations.py` via
/// the `Label`/user PK fields; the `labels`/`parent`/`created_by` path).
fn curly_uuid_error(raw: &str) -> String {
    format!("\u{201c}{raw}\u{201d} is not a valid UUID.")
}

/// `Invalid pk "..." - object does not exist.` (`relations.py`).
fn invalid_pk_error(raw: &str) -> String {
    format!("Invalid pk \"{raw}\" - object does not exist.")
}

/// Field names of `PageSerializer.Meta.fields` (`page.py:38-58`) that accept
/// writes, in declaration order; error objects follow this order.
pub const WRITE_VALIDATION_ORDER: &[&str] = &[
    "name",
    "access",
    "color",
    "labels",
    "parent",
    "is_locked",
    "archived_at",
    "created_by",
    "updated_by",
    "view_props",
    "logo_props",
    "label_ids",
    "project_ids",
    "description_html",
];

/// `PageSerializer` `CharField` coercion (`fields.py` `CharField`: bools and
/// composites fail, numbers stringify, strings trim): `allow_blank`
/// decides whether the trimmed-empty value passes or fails `blank`.
fn coerce_char(value: &Value, allow_blank: bool) -> Result<String, &'static str> {
    match value {
        Value::Null => Err("This field may not be null."),
        Value::Bool(_) | Value::Array(_) | Value::Object(_) => Err("Not a valid string."),
        Value::Number(_) => {
            let text = py_scalar(value);
            let trimmed = text.trim().to_owned();
            if trimmed.is_empty() && !allow_blank {
                return Err("This field may not be blank.");
            }
            Ok(trimmed)
        }
        Value::String(text) => {
            let trimmed = text.trim().to_owned();
            if trimmed.is_empty() && !allow_blank {
                return Err("This field may not be blank.");
            }
            Ok(trimmed)
        }
    }
}

/// `access` ChoiceField (`((0, "Public"), (1, "Private"))`,
/// `fields.py` `ChoiceField.to_internal_value`): `choice_strings_to_values`
/// lookup over `str(data)`, failure renders the Python-`str` input.
fn coerce_access(value: &Value) -> Result<i32, String> {
    if matches!(value, Value::Null) {
        return Err("This field may not be null.".to_owned());
    }
    let key = match value {
        Value::Bool(flag) => {
            if *flag {
                "True".to_owned()
            } else {
                "False".to_owned()
            }
        }
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                int.to_string()
            } else if let Some(uint) = number.as_u64() {
                uint.to_string()
            } else if let Some(float) = number.as_f64() {
                crate::paginator::py_float_str(float)
            } else {
                number.to_string()
            }
        }
        Value::String(text) => text.clone(),
        _ => py_scalar(value),
    };
    match key.as_str() {
        "0" => Ok(0),
        "1" => Ok(1),
        _ => Err(format!("\"{}\" is not a valid choice.", py_scalar(value))),
    }
}

/// `is_locked` BooleanField (`fields.py`, lower-cased string sets; numeric
/// `1`/`1.0` truthy and `0`/`0.0` falsy via `==` set membership).
fn coerce_bool(value: &Value) -> Result<bool, &'static str> {
    const INVALID: &str = "Must be a valid boolean.";
    match value {
        Value::Null => Err("This field may not be null."),
        Value::Bool(flag) => Ok(*flag),
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                return match int {
                    1 => Ok(true),
                    0 => Ok(false),
                    _ => Err(INVALID),
                };
            }
            if let Some(uint) = number.as_u64() {
                return match uint {
                    1 => Ok(true),
                    0 => Ok(false),
                    _ => Err(INVALID),
                };
            }
            if let Some(float) = number.as_f64() {
                if float == 1.0 {
                    return Ok(true);
                }
                if float == 0.0 {
                    return Ok(false);
                }
            }
            Err(INVALID)
        }
        Value::String(text) => match text.to_lowercase().as_str() {
            "t" | "y" | "yes" | "true" | "on" | "1" => Ok(true),
            "f" | "n" | "no" | "false" | "off" | "0" => Ok(false),
            _ => Err(INVALID),
        },
        Value::Array(_) | Value::Object(_) => Err(INVALID),
    }
}

/// `archived_at` DateField (`YYYY-MM-DD`, `allow_null` from `null=True`):
/// `""` falls through to the parser and fails `invalid` (never `blank`).
fn coerce_date(value: &Value) -> Result<Option<chrono::NaiveDate>, &'static str> {
    const INVALID: &str = "Date has wrong format. Use one of these formats instead: YYYY-MM-DD.";
    match value {
        Value::Null => Ok(None),
        Value::String(text) => chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d")
            .map(Some)
            .map_err(|_| INVALID),
        _ => Err(INVALID),
    }
}

/// `label_ids` / `project_ids` UUID items (`fields.py` `UUIDField`: ints —
/// including bools — go through `UUID(int=)`, everything else non-string
/// fails `invalid`; only `ValueError` is caught, and `int` overflow raises
/// it). Renders dashed (`hex_verbose`).
fn coerce_uuid_item(value: &Value) -> Result<uuid::Uuid, &'static str> {
    const INVALID: &str = "Must be a valid UUID.";
    match value {
        Value::Null => Err("This field may not be null."),
        // `bool` is an `int` subclass: `UUID(int=True)` is `...0001`.
        Value::Bool(flag) => Ok(uuid::Uuid::from_u128(u128::from(*flag as u8))),
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                if int < 0 {
                    return Err(INVALID);
                }
                return Ok(uuid::Uuid::from_u128(int as u128));
            }
            if let Some(uint) = number.as_u64() {
                return Ok(uuid::Uuid::from_u128(u128::from(uint)));
            }
            Err(INVALID)
        }
        Value::String(text) => parse_uuid_hex(text).ok_or(INVALID),
        Value::Array(_) | Value::Object(_) => Err(INVALID),
    }
}

// ---------------------------------------------------------------------------
// Page writes: validated shape, row SQL, response rendering
// ---------------------------------------------------------------------------

/// Validated `PageSerializer` / `PageDetailSerializer` writes: `None` arms
/// are absent keys (defaults on create, untouched on patch);
/// `parent`/`archived_at`/`created_by` nest a second `Option` because an
/// explicit `null` writes `NULL` (detach). `label_ids` / `project_ids` /
/// `description_html` only exist on the detail (PATCH) shape.
#[derive(Debug, Default, Clone)]
pub struct ValidPage {
    pub name: Option<String>,
    pub access: Option<i32>,
    pub color: Option<String>,
    pub labels: Option<Vec<uuid::Uuid>>,
    pub parent: Option<Option<uuid::Uuid>>,
    pub is_locked: Option<bool>,
    pub archived_at: Option<Option<chrono::NaiveDate>>,
    pub created_by: Option<Option<uuid::Uuid>>,
    pub view_props: Option<Value>,
    pub logo_props: Option<Value>,
    pub label_ids: Option<Vec<uuid::Uuid>>,
    pub project_ids: Option<Vec<uuid::Uuid>>,
    pub description_html: Option<String>,
}

/// PK existence scope for relational validation.
#[derive(Debug, Clone, Copy)]
enum PkTable {
    Pages,
    Labels,
    Users,
}

async fn pk_exists(pool: &PgPool, table: PkTable, id: &uuid::Uuid) -> Result<bool, Denial> {
    let found: Option<(uuid::Uuid,)> = match table {
        PkTable::Pages => {
            sqlx::query_as(r#"SELECT p.id FROM pages p WHERE p.id = $1 AND p.deleted_at IS NULL"#)
        }
        PkTable::Labels => {
            sqlx::query_as(r#"SELECT l.id FROM labels l WHERE l.id = $1 AND l.deleted_at IS NULL"#)
        }
        PkTable::Users => sqlx::query_as(r#"SELECT u.id FROM users u WHERE u.id = $1"#),
    }
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(found.is_some())
}

/// DRF `type(data).__name__` for the `incorrect_type` / `not_a_list`
/// messages (`relations.py`, `fields.py`).
fn pk_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "NoneType",
        Value::Bool(_) => "bool",
        Value::Number(number) if number.is_i64() || number.is_u64() => "int",
        Value::Number(_) => "float",
        Value::String(_) => "str",
        Value::Array(_) => "list",
        Value::Object(_) => "dict",
    }
}

fn incorrect_type_error(value: &Value) -> String {
    format!(
        "Incorrect type. Expected pk value, received {}.",
        pk_type_name(value)
    )
}

/// One relational PK item's failure: a field message, the ORM-level
/// short-circuit, or a row-fetch failure.
#[derive(Debug)]
enum PkFailure {
    Field(String),
    Reject(WriteRejection),
    Db(Denial),
}

impl From<Denial> for PkFailure {
    fn from(denial: Denial) -> Self {
        PkFailure::Db(denial)
    }
}

impl PkFailure {
    /// Field messages are pushed and validation continues (DRF collects
    /// every field's errors); ORM-level and database failures return.
    fn push_or_return(self, field: &str, errors: &mut OrderedErrors) -> Result<(), PageFailure> {
        match self {
            PkFailure::Field(message) => {
                push_flat(errors, field, message);
                Ok(())
            }
            PkFailure::Reject(rejection) => Err(PageFailure::Invalid(rejection)),
            PkFailure::Db(denial) => Err(PageFailure::Db(denial)),
        }
    }
}

/// Validate one relational PK item (`relations.py`
/// `PrimaryKeyRelatedField.to_internal_value` over the live manager):
/// `""` coerces to `None` (`RelatedField.run_validation`), bools fail
/// `incorrect_type`, ints go through `UUID(int=)` (out-of-range is the
/// Django-`ValidationError` short-circuit), strings through `UUID(hex=)`
/// (curly-quotes error), then the live-row existence check. Floats, lists
/// and dicts raise inside the ORM `get` — the [`WriteRejection::ValidDetail`]
/// short-circuit, exactly like the propagating exception.
async fn validate_pk_item(
    pool: &PgPool,
    value: &Value,
    table: PkTable,
) -> Result<Option<uuid::Uuid>, PkFailure> {
    match value {
        Value::Null => Ok(None),
        Value::String(text) if text.is_empty() => Ok(None),
        Value::String(text) => {
            let id =
                parse_uuid_hex(text).ok_or_else(|| PkFailure::Field(curly_uuid_error(text)))?;
            if pk_exists(pool, table, &id).await? {
                Ok(Some(id))
            } else {
                Err(PkFailure::Field(invalid_pk_error(text)))
            }
        }
        Value::Bool(_) => Err(PkFailure::Field(incorrect_type_error(value))),
        Value::Number(number) => {
            let raw = if let Some(int) = number.as_i64() {
                if int < 0 {
                    return Err(PkFailure::Reject(WriteRejection::ValidDetail));
                }
                int.to_string()
            } else if let Some(uint) = number.as_u64() {
                uint.to_string()
            } else {
                return Err(PkFailure::Reject(WriteRejection::ValidDetail));
            };
            let as_int: u128 = raw
                .parse()
                .map_err(|_| PkFailure::Reject(WriteRejection::ValidDetail))?;
            let id = uuid::Uuid::from_u128(as_int);
            if pk_exists(pool, table, &id).await? {
                Ok(Some(id))
            } else {
                Err(PkFailure::Field(invalid_pk_error(&raw)))
            }
        }
        Value::Array(_) | Value::Object(_) => Err(PkFailure::Reject(WriteRejection::ValidDetail)),
    }
}

/// Validate a `labels`-style PK list into live ids: `null` and non-lists
/// fail flat, items fail indexed (`fields.py` `ListField` plus the child
/// relation). ORM-level rejections short-circuit the whole body.
async fn validate_pk_list(
    pool: &PgPool,
    field: &str,
    value: &Value,
    table: PkTable,
    errors: &mut OrderedErrors,
) -> Result<Option<Vec<uuid::Uuid>>, PageFailure> {
    match value {
        Value::Null => {
            push_flat(errors, field, "This field may not be null.".to_owned());
            Ok(None)
        }
        Value::Array(items) => {
            let mut ids = Vec::with_capacity(items.len());
            let mut failed = false;
            for (index, item) in items.iter().enumerate() {
                match validate_pk_item(pool, item, table).await {
                    Ok(Some(id)) => ids.push(id),
                    Ok(None) => {
                        push_indexed(
                            errors,
                            field,
                            index,
                            "This field may not be null.".to_owned(),
                        );
                        failed = true;
                    }
                    Err(PkFailure::Field(message)) => {
                        push_indexed(errors, field, index, message);
                        failed = true;
                    }
                    Err(PkFailure::Reject(rejection)) => {
                        return Err(PageFailure::Invalid(rejection));
                    }
                    Err(PkFailure::Db(denial)) => return Err(PageFailure::Db(denial)),
                }
            }
            if failed {
                Ok(None)
            } else {
                Ok(Some(ids))
            }
        }
        other => {
            push_flat(
                errors,
                field,
                format!(
                    "Expected a list of items but got type \"{}\".",
                    pk_type_name(other)
                ),
            );
            Ok(None)
        }
    }
}

/// Validate a `label_ids`-style UUID list (`fields.py` `ListField` +
/// `UUIDField`, `hex_verbose` rendering).
fn validate_uuid_list(
    field: &str,
    value: &Value,
    errors: &mut OrderedErrors,
) -> Option<Vec<uuid::Uuid>> {
    match value {
        Value::Null => {
            push_flat(errors, field, "This field may not be null.".to_owned());
            None
        }
        Value::Array(items) => {
            let mut ids = Vec::with_capacity(items.len());
            let mut failed = false;
            for (index, item) in items.iter().enumerate() {
                match coerce_uuid_item(item) {
                    Ok(id) => ids.push(id),
                    Err(message) => {
                        push_indexed(errors, field, index, message.to_owned());
                        failed = true;
                    }
                }
            }
            if failed {
                None
            } else {
                Some(ids)
            }
        }
        other => {
            push_flat(
                errors,
                field,
                format!(
                    "Expected a list of items but got type \"{}\".",
                    pk_type_name(other)
                ),
            );
            None
        }
    }
}

/// Validate one page-write body in `Meta.fields` order (`page.py:38-58`):
/// unknown and read-only keys ignored, absent keys untouched (defaults fill
/// them on create), every declared key validated independently with all
/// field errors collected. `detail` selects the `PageDetailSerializer`
/// shape (PATCH): `description_html` becomes a declared field.
pub async fn validate_page_fields(
    pool: &PgPool,
    body: &Map<String, Value>,
    detail: bool,
) -> Result<ValidPage, PageFailure> {
    let mut errors: OrderedErrors = Vec::new();
    let mut valid = ValidPage::default();
    if let Some(value) = body.get("name") {
        match coerce_char(value, true) {
            Ok(name) => valid.name = Some(name),
            Err(message) => push_flat(&mut errors, "name", message.to_owned()),
        }
    }
    if let Some(value) = body.get("access") {
        match coerce_access(value) {
            Ok(access) => valid.access = Some(access),
            Err(message) => push_flat(&mut errors, "access", message),
        }
    }
    if let Some(value) = body.get("color") {
        match coerce_char(value, true) {
            Ok(color) => {
                if color.chars().count() > 255 {
                    push_flat(
                        &mut errors,
                        "color",
                        "Ensure this field has no more than 255 characters.".to_owned(),
                    );
                } else {
                    valid.color = Some(color);
                }
            }
            Err(message) => push_flat(&mut errors, "color", message.to_owned()),
        }
    }
    if let Some(value) = body.get("labels") {
        valid.labels =
            validate_pk_list(pool, "labels", value, PkTable::Labels, &mut errors).await?;
    }
    if let Some(value) = body.get("parent") {
        match validate_pk_item(pool, value, PkTable::Pages).await {
            Ok(parent) => valid.parent = Some(parent),
            Err(failure) => failure.push_or_return("parent", &mut errors)?,
        }
    }
    if let Some(value) = body.get("is_locked") {
        match coerce_bool(value) {
            Ok(locked) => valid.is_locked = Some(locked),
            Err(message) => push_flat(&mut errors, "is_locked", message.to_owned()),
        }
    }
    if let Some(value) = body.get("archived_at") {
        match coerce_date(value) {
            Ok(date) => valid.archived_at = Some(date),
            Err(message) => push_flat(&mut errors, "archived_at", message.to_owned()),
        }
    }
    if let Some(value) = body.get("created_by") {
        match validate_pk_item(pool, value, PkTable::Users).await {
            Ok(owner) => valid.created_by = Some(owner),
            Err(failure) => failure.push_or_return("created_by", &mut errors)?,
        }
    }
    // `updated_by` validates like any user PK (`page.py` declares the
    // audit pair) but `BaseModel.save` overwrites it with the request user
    // on every update — and on create with `None` — so the value is never
    // stored; only its errors render.
    if let Some(value) = body.get("updated_by") {
        match validate_pk_item(pool, value, PkTable::Users).await {
            Ok(_) => {}
            Err(failure) => failure.push_or_return("updated_by", &mut errors)?,
        }
    }
    for field in ["view_props", "logo_props"] {
        if let Some(value) = body.get(field) {
            if value.is_null() {
                push_flat(&mut errors, field, "This field may not be null.".to_owned());
            } else if field == "view_props" {
                valid.view_props = Some(value.clone());
            } else {
                valid.logo_props = Some(value.clone());
            }
        }
    }
    // `validate_uuid_list` returns `None` exactly when it pushed errors.
    if let Some(value) = body.get("label_ids") {
        valid.label_ids = validate_uuid_list("label_ids", value, &mut errors);
    }
    if let Some(value) = body.get("project_ids") {
        valid.project_ids = validate_uuid_list("project_ids", value, &mut errors);
    }
    if detail {
        if let Some(value) = body.get("description_html") {
            match coerce_char(value, false) {
                Ok(html) => valid.description_html = Some(html),
                Err(message) => push_flat(&mut errors, "description_html", message.to_owned()),
            }
        }
    }
    if errors.is_empty() {
        Ok(valid)
    } else {
        Err(PageFailure::Invalid(WriteRejection::Fields(errors)))
    }
}

/// One workspace+project+bridge scoped page row: the `partial_update`
/// (`base.py:156-161`) and `destroy` (`:369-374`) fetches plus every column
/// the PATCH render needs. The default manager hides soft-deleted rows.
#[derive(Debug, Clone, sqlx::FromRow)]
struct ScopedPage {
    id: uuid::Uuid,
    name: String,
    owned_by_id: uuid::Uuid,
    access: i16,
    color: String,
    parent_id: Option<uuid::Uuid>,
    is_locked: bool,
    archived_at: Option<chrono::NaiveDate>,
    workspace_id: uuid::Uuid,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
    created_by_id: Option<uuid::Uuid>,
    updated_by_id: Option<uuid::Uuid>,
    view_props: Value,
    logo_props: Value,
    description_html: String,
}

/// The `PageViewSet.get_queryset` re-read row for create (`base.py:81-127`
/// narrowed to one `pk`): the scope predicates that can still miss
/// (`parent__isnull`, owner-or-public, project bridge) plus the
/// `is_favorite` / `label_ids` / `project_ids` annotations. `project_ids`
/// keeps the ported `~Q(projects__id=True)` no-op shape: Django compiles
/// the `True` through `UUIDField.get_prep_value` into `UUID(int=1)`, so the
/// predicate below compares against the zero-`1` UUID, never boolean `TRUE`.
#[derive(Debug, Clone, sqlx::FromRow)]
struct DetailRow {
    #[sqlx(flatten)]
    page: ScopedPage,
    is_favorite: bool,
    label_ids: Vec<uuid::Uuid>,
    project_ids: Vec<uuid::Uuid>,
}

const SCOPED_PAGE_COLUMNS: &str = "p.id, p.name, p.owned_by_id, p.access, p.color, p.parent_id, \
    p.is_locked, p.archived_at, p.workspace_id, p.created_at, p.updated_at, p.created_by_id, \
    p.updated_by_id, p.view_props, p.logo_props, p.description_html";

/// The scoped page fetch (`partial_update` `:156-161`, `destroy` `:369-374`):
/// `pk` + workspace slug + URL-project bridge (live rows only). A miss is
/// `DoesNotExist`: the PATCH maps it to the owner-access 400 body
/// (`:196-200`), the DELETE lets it 404 through `handle_exception`.
async fn fetch_scoped_page(
    pool: &PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    page_id: &uuid::Uuid,
) -> Result<Option<ScopedPage>, Denial> {
    sqlx::query_as(&format!(
        r#"SELECT {SCOPED_PAGE_COLUMNS} FROM pages p
           JOIN workspaces w ON w.id = p.workspace_id
           JOIN project_pages pp ON pp.page_id = p.id
               AND pp.project_id = $2 AND pp.deleted_at IS NULL
           WHERE p.id = $1 AND w.slug = $3 AND p.deleted_at IS NULL"#
    ))
    .bind(page_id)
    .bind(project_id)
    .bind(slug)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)
}

/// The create re-read (`base.py:149`: `self.get_queryset().get(pk=...)`):
/// the full detail scope — top-level only (`:97`), owner-or-public (`:98`),
/// the URL-project bridge (`:125`) — with the three annotations. A child
/// page (or any row the queryset hides) misses here and answers 404 through
/// `handle_exception`, exactly like Django.
async fn fetch_detail_row(
    pool: &PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    page_id: &uuid::Uuid,
    user_id: &uuid::Uuid,
) -> Result<Option<DetailRow>, Denial> {
    sqlx::query_as(&format!(
        r#"SELECT {SCOPED_PAGE_COLUMNS},
           EXISTS(SELECT 1 FROM user_favorites uf
                  WHERE uf.user_id = $4 AND uf.entity_type = 'page'
                  AND uf.entity_identifier = p.id AND uf.workspace_id = p.workspace_id
                  AND uf.deleted_at IS NULL) AS is_favorite,
           COALESCE((SELECT ARRAY_AGG(DISTINCT pl.label_id) FROM page_labels pl
                     WHERE pl.page_id = p.id AND pl.label_id IS NOT NULL
                     AND pl.deleted_at IS NULL), '{{}}') AS label_ids,
           COALESCE((SELECT ARRAY_AGG(DISTINCT pp2.project_id) FROM project_pages pp2
                     WHERE pp2.page_id = p.id
                     AND NOT (pp2.project_id = '00000000-0000-0000-0000-000000000001')
                     AND pp2.deleted_at IS NULL), '{{}}') AS project_ids
           FROM pages p
           JOIN workspaces w ON w.id = p.workspace_id
           JOIN project_pages pp ON pp.page_id = p.id
               AND pp.project_id = $2 AND pp.deleted_at IS NULL
           WHERE p.id = $1 AND w.slug = $3 AND p.deleted_at IS NULL
           AND p.parent_id IS NULL AND (p.owned_by_id = $4 OR p.access = 0)"#
    ))
    .bind(page_id)
    .bind(project_id)
    .bind(slug)
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)
}

/// The actor's render zone (`TimezoneMixin.initial`, `app/views/base.py:44`):
/// datetimes render in the request user's `user_timezone`, like DRF's
/// `iso-8601` (`+00:00` rewritten to `Z`).
async fn actor_timezone(pool: &PgPool, user_id: &uuid::Uuid) -> Result<chrono_tz::Tz, Denial> {
    let row: Option<(String,)> =
        sqlx::query_as(r#"SELECT u.user_timezone FROM users u WHERE u.id = $1"#)
            .bind(user_id)
            .fetch_optional(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    let (name,) = row.ok_or(Denial::ServerError)?;
    name.parse().map_err(|_| Denial::ServerError)
}

fn render_dt(dt: &chrono::DateTime<chrono::Utc>, tz: &chrono_tz::Tz) -> String {
    quoted(&crate::serializer::render_datetime_in(dt, tz))
}

fn render_uuid(id: &uuid::Uuid) -> String {
    quoted(&id.to_string())
}

fn render_opt_uuid(id: &Option<uuid::Uuid>) -> String {
    id.as_ref()
        .map(render_uuid)
        .unwrap_or_else(|| "null".to_owned())
}

fn render_date(date: &Option<chrono::NaiveDate>) -> String {
    date.map(|day| quoted(&day.format("%Y-%m-%d").to_string()))
        .unwrap_or_else(|| "null".to_owned())
}

fn render_json(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or("null".to_owned())
}

/// The create 201 shape: `PageDetailSerializer` over the re-read row —
/// `Meta.fields` order minus write-only `labels` plus `description_html`
/// (`page.py:129-133`), 19 keys.
fn render_create_row(row: &DetailRow, tz: &chrono_tz::Tz) -> String {
    let page = &row.page;
    let mut out = String::from("{");
    out.push_str(&format!("\"id\":{},", render_uuid(&page.id)));
    out.push_str(&format!("\"name\":{},", quoted(&page.name)));
    out.push_str(&format!("\"owned_by\":{},", render_uuid(&page.owned_by_id)));
    out.push_str(&format!("\"access\":{},", i32::from(page.access)));
    out.push_str(&format!("\"color\":{},", quoted(&page.color)));
    out.push_str(&format!("\"parent\":{},", render_opt_uuid(&page.parent_id)));
    out.push_str(&format!(
        "\"is_favorite\":{},",
        if row.is_favorite { "true" } else { "false" }
    ));
    out.push_str(&format!(
        "\"is_locked\":{},",
        if page.is_locked { "true" } else { "false" }
    ));
    out.push_str(&format!(
        "\"archived_at\":{},",
        render_date(&page.archived_at)
    ));
    out.push_str(&format!(
        "\"workspace\":{},",
        render_uuid(&page.workspace_id)
    ));
    out.push_str(&format!(
        "\"created_at\":{},",
        render_dt(&page.created_at, tz)
    ));
    out.push_str(&format!(
        "\"updated_at\":{},",
        render_dt(&page.updated_at, tz)
    ));
    out.push_str(&format!(
        "\"created_by\":{},",
        render_opt_uuid(&page.created_by_id)
    ));
    out.push_str(&format!(
        "\"updated_by\":{},",
        render_opt_uuid(&page.updated_by_id)
    ));
    out.push_str(&format!(
        "\"view_props\":{},",
        render_json(&page.view_props)
    ));
    out.push_str(&format!(
        "\"logo_props\":{},",
        render_json(&page.logo_props)
    ));
    out.push_str(&format!(
        "\"label_ids\":[{}],",
        row.label_ids
            .iter()
            .map(|id| quoted(&id.to_string()))
            .collect::<Vec<_>>()
            .join(",")
    ));
    out.push_str(&format!(
        "\"project_ids\":[{}],",
        row.project_ids
            .iter()
            .map(|id| quoted(&id.to_string()))
            .collect::<Vec<_>>()
            .join(",")
    ));
    out.push_str(&format!(
        "\"description_html\":{}}}",
        quoted(&page.description_html)
    ));
    out
}

/// The PATCH 200 shape: the update-bound `PageDetailSerializer` over the
/// plain instance — the annotation-backed keys (`is_favorite`,
/// `label_ids`, `project_ids`) have no attribute and DRF drops them
/// (`SkipField`), except `label_ids` / `project_ids` render when the
/// request body set them as ad-hoc attributes (16 base keys).
#[derive(Debug, Clone, Default)]
pub struct PatchRender {
    pub page: RenderPage,
    pub label_ids: Option<Vec<uuid::Uuid>>,
    pub project_ids: Option<Vec<uuid::Uuid>>,
}

/// Owned render values for one PATCH response.
#[derive(Debug, Clone, Default)]
pub struct RenderPage {
    pub id: uuid::Uuid,
    pub name: String,
    pub owned_by_id: uuid::Uuid,
    pub access: i32,
    pub color: String,
    pub parent_id: Option<uuid::Uuid>,
    pub is_locked: bool,
    pub archived_at: Option<chrono::NaiveDate>,
    pub workspace_id: uuid::Uuid,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub created_by_id: Option<uuid::Uuid>,
    pub updated_by_id: Option<uuid::Uuid>,
    pub view_props: Value,
    pub logo_props: Value,
    pub description_html: String,
}

fn render_patch_row(render: &PatchRender, tz: &chrono_tz::Tz) -> String {
    let page = &render.page;
    let mut out = String::from("{");
    out.push_str(&format!("\"id\":{},", render_uuid(&page.id)));
    out.push_str(&format!("\"name\":{},", quoted(&page.name)));
    out.push_str(&format!("\"owned_by\":{},", render_uuid(&page.owned_by_id)));
    out.push_str(&format!("\"access\":{},", page.access));
    out.push_str(&format!("\"color\":{},", quoted(&page.color)));
    out.push_str(&format!("\"parent\":{},", render_opt_uuid(&page.parent_id)));
    out.push_str(&format!(
        "\"is_locked\":{},",
        if page.is_locked { "true" } else { "false" }
    ));
    out.push_str(&format!(
        "\"archived_at\":{},",
        render_date(&page.archived_at)
    ));
    out.push_str(&format!(
        "\"workspace\":{},",
        render_uuid(&page.workspace_id)
    ));
    out.push_str(&format!(
        "\"created_at\":{},",
        render_dt(&page.created_at, tz)
    ));
    out.push_str(&format!(
        "\"updated_at\":{},",
        render_dt(&page.updated_at, tz)
    ));
    out.push_str(&format!(
        "\"created_by\":{},",
        render_opt_uuid(&page.created_by_id)
    ));
    out.push_str(&format!(
        "\"updated_by\":{},",
        render_opt_uuid(&page.updated_by_id)
    ));
    out.push_str(&format!(
        "\"view_props\":{},",
        render_json(&page.view_props)
    ));
    out.push_str(&format!(
        "\"logo_props\":{},",
        render_json(&page.logo_props)
    ));
    if let Some(ids) = &render.label_ids {
        out.push_str(&format!(
            "\"label_ids\":[{}],",
            ids.iter()
                .map(|id| quoted(&id.to_string()))
                .collect::<Vec<_>>()
                .join(",")
        ));
    }
    if let Some(ids) = &render.project_ids {
        out.push_str(&format!(
            "\"project_ids\":[{}],",
            ids.iter()
                .map(|id| quoted(&id.to_string()))
                .collect::<Vec<_>>()
                .join(",")
        ));
    }
    out.push_str(&format!(
        "\"description_html\":{}}}",
        quoted(&page.description_html)
    ));
    out
}

// ---------------------------------------------------------------------------
// Page writes: handlers
// ---------------------------------------------------------------------------

/// Read the request body as DRF `request.data` (`base.py` reads it lazily
/// per handler): empty bodies validate as `{}`; malformed JSON answers the
/// 400 `ParseError` detail; a well-formed non-object answers the 500
/// generic branch — every handler calls `.get(...)` on the data, and that
/// `AttributeError` is what Django renders for lists, strings and `null`.
async fn read_write_body(req: Request) -> Result<Map<String, Value>, Denial> {
    let (_parts, body) = req.into_parts();
    let bytes = match body.collect().await {
        Ok(collected) => collected.to_bytes(),
        Err(_) => return Err(Denial::ServerError),
    };
    if bytes.is_empty() {
        return Ok(Map::new());
    }
    let parsed: Value = match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(error) => {
            return Err(Denial::BadDetail(format!("JSON parse error - {error}")));
        }
    };
    match parsed {
        Value::Object(map) => Ok(map),
        _ => Err(Denial::ServerError),
    }
}

fn rejection_response(rejection: WriteRejection) -> Response {
    match rejection {
        WriteRejection::Fields(errors) => Denial::BadJson(
            serde_json::from_str(&write_errors_body(&errors)).expect("rendered errors parse"),
        )
        .into_response(),
        WriteRejection::ValidDetail => Denial::ValidDetail.into_response(),
    }
}

/// `POST .../pages/` (`PageViewSet.create`, `base.py:129-152`).
pub async fn page_create(
    State(state): State<AppState>,
    Path((slug, project_raw)): Path<(String, String)>,
    extension: Option<Extension<SessionHandle>>,
    req: Request,
) -> Response {
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let user_id = match actor_user_id(extension.clone()) {
        Ok(id) => id,
        Err(denial) => return denial.into_response(),
    };
    let project_id = match resolve_project_id(pool, &slug, &project_raw).await {
        Ok(id) => id,
        Err(denial) => return denial.into_response(),
    };
    if let Err(denial) =
        gate::resolve_gate(&state, "POST", &slug, &project_id, None, extension).await
    {
        return denial.into_response();
    }
    let body = match read_write_body(req).await {
        Ok(body) => body,
        Err(denial) => return denial.into_response(),
    };
    // `PageSerializer` validation first (`:130-131`); the ported `TypeError`
    // bug follows: supplied `label_ids` / `project_ids` validate as UUID
    // lists, then blow up inside `Page.objects.create(**validated_data)`
    // — the 500 generic branch, never a field error.
    let valid = match validate_page_fields(pool, &body, false).await {
        Ok(valid) => valid,
        Err(failure) => return failure_response(failure),
    };
    if valid.label_ids.is_some() || valid.project_ids.is_some() {
        return Denial::ServerError.into_response();
    }
    // Serializer context (`:132-138`): `description_*` come from the RAW
    // body, never from `validated_data` (the F30-01 ported bug). `null`
    // JSON values fail the `NOT NULL` columns (`page.py` never validates
    // them) — the `IntegrityError` 400 branch; non-string HTML and any
    // non-null binary fail earlier (`strip_tags` / the driver) — the 500
    // branch.
    enum Html {
        Default,
        Text(String),
    }
    let html = match body.get("description_html") {
        None => Html::Default,
        Some(Value::Null) => {
            return Denial::BadError("The payload is not valid".to_owned()).into_response()
        }
        Some(Value::String(text)) => Html::Text(text.clone()),
        Some(_) => return Denial::ServerError.into_response(),
    };
    let description_json = match body.get("description_json") {
        None => Value::Object(Map::new()),
        Some(Value::Null) => {
            return Denial::BadError("The payload is not valid".to_owned()).into_response();
        }
        Some(value) => value.clone(),
    };
    if body
        .get("description_binary")
        .is_some_and(|value| !value.is_null())
    {
        return Denial::ServerError.into_response();
    }
    let workspace_id: uuid::Uuid =
        match sqlx::query_as(r#"SELECT p.workspace_id FROM projects p WHERE p.id = $1"#)
            .bind(project_id)
            .fetch_optional(pool)
            .await
        {
            Ok(Some((id,))) => id,
            Ok(None) => return Denial::ProjectNotFound.into_response(),
            Err(_) => return Denial::ServerError.into_response(),
        };
    let timezone = match actor_timezone(pool, &user_id).await {
        Ok(tz) => tz,
        Err(denial) => return denial.into_response(),
    };
    let stored_html = match &html {
        Html::Default => "<p></p>".to_owned(),
        Html::Text(text) => text.clone(),
    };
    let stored_stripped: Option<String> = if stored_html.is_empty() {
        None
    } else {
        Some(crate::space::sanitize::strip_tags(stored_html.as_str()))
    };
    let stored_json = serde_json::to_string(&description_json).expect("json serializes");
    let page_id = uuid::Uuid::new_v4();
    // `auto_now_add` / `auto_now` are two Python `now()` calls (the
    // response usually shows `created_at != updated_at` microseconds
    // apart), never one database timestamp.
    let created_at = chrono::Utc::now();
    let updated_at = chrono::Utc::now();
    // `Page.objects.create` (`page.py:73-80`) + the `ProjectPage` bridge
    // (`:83-89`) + the label bulk rows (`:92-105`, no-op when `None`) run
    // as one write unit; `created_by` is the request user and `updated_by`
    // stays null (`BaseModel.save` via crum).
    let mut tx = match pool.begin().await {
        Ok(tx) => tx,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let inserted = sqlx::query(
        r#"INSERT INTO pages
           (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at,
            workspace_id, name, description_json, description_binary, description_html,
            description_stripped, owned_by_id, access, color, parent_id, archived_at,
            is_locked, view_props, logo_props, is_global, sort_order,
            moved_to_page, moved_to_project, external_id, external_source)
           VALUES ($1, $2, $3, $4, NULL, NULL, $5, $6, CAST($7 AS jsonb), NULL,
               $8, $9, $4, $10, $11, $12, $13, $14, CAST($15 AS jsonb), CAST($16 AS jsonb),
               false, 65535, NULL, NULL, NULL, NULL)"#,
    )
    .bind(page_id)
    .bind(created_at)
    .bind(updated_at)
    .bind(user_id)
    .bind(workspace_id)
    .bind(valid.name.as_deref().unwrap_or(""))
    .bind(stored_json)
    .bind(stored_html.as_str())
    .bind(stored_stripped)
    .bind(valid.access.unwrap_or(0) as i16)
    .bind(valid.color.as_deref().unwrap_or(""))
    .bind(valid.parent.unwrap_or(None))
    .bind(valid.archived_at.unwrap_or(None))
    .bind(valid.is_locked.unwrap_or(false))
    .bind(
        serde_json::to_string(valid.view_props.as_ref().unwrap_or(&default_view_props()))
            .expect("json serializes"),
    )
    .bind(
        serde_json::to_string(
            valid
                .logo_props
                .as_ref()
                .unwrap_or(&Value::Object(Map::new())),
        )
        .expect("json serializes"),
    )
    .execute(&mut *tx)
    .await;
    if let Err(error) = inserted {
        let _ = tx.rollback().await;
        return db_write_denial(&error);
    }
    if sqlx::query(
        r#"INSERT INTO project_pages
           (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at,
            workspace_id, project_id, page_id)
           VALUES ($1, now(), now(), $2, NULL, NULL, $3, $4, $5)"#,
    )
    .bind(uuid::Uuid::new_v4())
    .bind(user_id)
    .bind(workspace_id)
    .bind(project_id)
    .bind(page_id)
    .execute(&mut *tx)
    .await
    .is_err()
    {
        let _ = tx.rollback().await;
        return Denial::ServerError.into_response();
    }
    if let Some(labels) = valid.labels.clone() {
        for chunk in labels.chunks(10) {
            for label_id in chunk {
                if sqlx::query(
                    r#"INSERT INTO page_labels
                       (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at,
                        label_id, page_id, workspace_id)
                       VALUES ($1, now(), now(), $2, NULL, NULL, $3, $4, $5)"#,
                )
                .bind(uuid::Uuid::new_v4())
                .bind(user_id)
                .bind(label_id)
                .bind(page_id)
                .bind(workspace_id)
                .execute(&mut *tx)
                .await
                .is_err()
                {
                    let _ = tx.rollback().await;
                    return Denial::ServerError.into_response();
                }
            }
        }
    }
    if tx.commit().await.is_err() {
        return Denial::ServerError.into_response();
    }
    // Unconditional `page_transaction` after the save (`:144-148`), with
    // the raw HTML (default `<p></p>` when absent) and no old snapshot. A
    // present non-string HTML never reaches here (500 above), so the
    // `as_str` projection is exact.
    enqueue_best_effort(
        pool,
        &pidash_jobs::app_pages::page_transaction_create_job(
            &page_id.to_string(),
            body.get("description_html").and_then(Value::as_str),
        ),
    )
    .await;
    // Re-read through the annotated queryset (`:149-150`); a miss (a child
    // page, or any other row the queryset hides) 404s through
    // `handle_exception`, exactly like the propagating `DoesNotExist`.
    match fetch_detail_row(pool, &slug, &project_id, &page_id, &user_id).await {
        Ok(Some(row)) => json_response(StatusCode::CREATED, render_create_row(&row, &timezone)),
        Ok(None) => Denial::ObjectNotFound.into_response(),
        Err(denial) => denial.into_response(),
    }
}

/// Map a write-path DB error like `handle_exception`
/// (`app/views/base.py:120-124`): integrity violations (including the
/// `NOT NULL` failures the unvalidated context values hit) answer the 400
/// payload body, everything else the 500 branch.
fn db_write_denial(error: &sqlx::Error) -> Response {
    let integrity = error
        .as_database_error()
        .and_then(|db| db.code())
        .is_some_and(|code| code.starts_with("23"));
    if integrity {
        Denial::BadError("The payload is not valid".to_owned()).into_response()
    } else {
        Denial::ServerError.into_response()
    }
}

/// `view_props` model default (`db/models/page.py:19-20`, `get_view_props`).
fn default_view_props() -> Value {
    Value::Object(Map::from_iter([(
        "full_width".to_owned(),
        Value::Bool(false),
    )]))
}

/// Scoped existence for the `partial_update` parent re-fetch
/// (`base.py:166-173`): the same workspace+project+bridge scope as the page
/// fetch. A miss is `DoesNotExist` → the owner-access 400 (`:196-200`).
async fn scoped_page_exists(
    pool: &PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    page_id: &uuid::Uuid,
) -> Result<bool, Denial> {
    let found: Option<(uuid::Uuid,)> = sqlx::query_as(
        r#"SELECT p.id FROM pages p
           JOIN workspaces w ON w.id = p.workspace_id
           JOIN project_pages pp ON pp.page_id = p.id
               AND pp.project_id = $2 AND pp.deleted_at IS NULL
           WHERE p.id = $1 AND w.slug = $3 AND p.deleted_at IS NULL"#,
    )
    .bind(page_id)
    .bind(project_id)
    .bind(slug)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(found.is_some())
}

/// `PATCH .../pages/<page_id>/` (`PageViewSet.partial_update`,
/// `base.py:154-200`).
pub async fn page_partial_update(
    State(state): State<AppState>,
    Path((slug, project_raw, page_raw)): Path<(String, String, String)>,
    extension: Option<Extension<SessionHandle>>,
    req: Request,
) -> Response {
    let Ok(page_id) = parse_page_id(&page_raw) else {
        return crate::edge::proxy(State(state), req).await;
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let user_id = match actor_user_id(extension.clone()) {
        Ok(id) => id,
        Err(denial) => return denial.into_response(),
    };
    let project_id = match resolve_project_id(pool, &slug, &project_raw).await {
        Ok(id) => id,
        Err(denial) => return denial.into_response(),
    };
    let gate = match gate::resolve_gate(
        &state,
        "PATCH",
        &slug,
        &project_id,
        Some(page_id),
        extension,
    )
    .await
    {
        Ok(gate) => gate,
        Err(denial) => return denial.into_response(),
    };
    let body = match read_write_body(req).await {
        Ok(body) => body,
        Err(denial) => return denial.into_response(),
    };
    // The scoped fetch (`:156-161`): a miss is `DoesNotExist`, which the
    // `except` branch (`:196-200`) renders as the owner-access 400 — never
    // a 404 (a fully unknown id never reaches here: the class lookup 404s
    // first).
    let page = match fetch_scoped_page(pool, &slug, &project_id, &page_id).await {
        Ok(Some(page)) => page,
        Ok(None) => {
            return json_response(StatusCode::BAD_REQUEST, gate::ACCESS_OWNER_BODY.to_owned());
        }
        Err(denial) => return denial.into_response(),
    };
    if page.is_locked {
        return json_response(StatusCode::BAD_REQUEST, gate::PAGE_LOCKED_BODY.to_owned());
    }
    // The parent re-fetch (`:166-173`): only when the raw value is truthy
    // (`request.data.get("parent")`), through the same scope; a miss — or
    // an ORM-level rejection — lands in the same `except` body.
    if body.get("parent").is_some_and(python_truthy) {
        let parent_id = match &body["parent"] {
            Value::String(text) => match parse_uuid_hex(text) {
                Some(id) => id,
                None => return Denial::ValidDetail.into_response(),
            },
            Value::Bool(flag) => uuid::Uuid::from_u128(u128::from(*flag as u8)),
            Value::Number(number) => {
                let raw = if let Some(int) = number.as_i64() {
                    if int < 0 {
                        return Denial::ValidDetail.into_response();
                    }
                    int as u128
                } else if let Some(uint) = number.as_u64() {
                    u128::from(uint)
                } else {
                    return Denial::ValidDetail.into_response();
                };
                uuid::Uuid::from_u128(raw)
            }
            _ => return Denial::ValidDetail.into_response(),
        };
        match scoped_page_exists(pool, &slug, &project_id, &parent_id).await {
            Ok(true) => {}
            Ok(false) => {
                return json_response(StatusCode::BAD_REQUEST, gate::ACCESS_OWNER_BODY.to_owned());
            }
            Err(denial) => return denial.into_response(),
        }
    }
    // Owner-only access change (`:176-180`): the RAW request value compares
    // against the stored one with Python equality (`"0" != 0`,
    // `False == 0`), before any serializer runs.
    let stored_access = i32::from(page.access);
    let access_changed = match body.get("access") {
        None => false,
        Some(Value::Number(number)) => {
            let same = number
                .as_i64()
                .is_some_and(|value| value == i64::from(stored_access))
                || number
                    .as_u64()
                    .is_some_and(|value| value == u64::from(stored_access as u16))
                || number
                    .as_f64()
                    .is_some_and(|value| value == f64::from(stored_access));
            !same
        }
        Some(Value::Bool(flag)) => i32::from(*flag) != stored_access,
        Some(_) => true,
    };
    if access_changed && page.owned_by_id != user_id {
        return json_response(StatusCode::BAD_REQUEST, gate::ACCESS_OWNER_BODY.to_owned());
    }
    let valid = match validate_page_fields(pool, &body, true).await {
        Ok(valid) => valid,
        Err(failure) => return failure_response(failure),
    };
    let old_html = page.description_html.clone();
    // `update()` label replacement (`page.py:108-126`): wipe the live
    // `PageLabel` rows, then bulk-create the new set (`batch_size=10`)
    // with the page's own audit ids — never the requester's.
    if let Some(labels) = &valid.labels {
        if sqlx::query(r#"DELETE FROM page_labels WHERE page_id = $1 AND deleted_at IS NULL"#)
            .bind(page_id)
            .execute(pool)
            .await
            .is_err()
        {
            return Denial::ServerError.into_response();
        }
        for chunk in labels.chunks(10) {
            for label_id in chunk {
                if sqlx::query(
                    r#"INSERT INTO page_labels
                       (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at,
                        label_id, page_id, workspace_id)
                       VALUES ($1, now(), now(), $2, $3, NULL, $4, $5, $6)"#,
                )
                .bind(uuid::Uuid::new_v4())
                .bind(page.created_by_id)
                .bind(page.updated_by_id)
                .bind(label_id)
                .bind(page_id)
                .bind(page.workspace_id)
                .execute(pool)
                .await
                .is_err()
                {
                    return Denial::ServerError.into_response();
                }
            }
        }
    }
    // `super().update(instance, validated_data)` plus `Page.save()`: the
    // validated columns, always `updated_at`/`updated_by` (even `{}`), and
    // the unconditional `description_stripped` recompute.
    let final_html = valid
        .description_html
        .clone()
        .unwrap_or(page.description_html);
    let final_stripped: Option<String> = if final_html.is_empty() {
        None
    } else {
        Some(crate::space::sanitize::strip_tags(final_html.as_str()))
    };
    let mut sql = String::from("UPDATE pages SET updated_at = now(), updated_by_id = $1");
    let mut tail = 2u32;
    macro_rules! column {
        ($name:literal) => {{
            sql.push_str(&format!(", {} = ${}", $name, tail));
            tail += 1;
        }};
    }
    if valid.name.is_some() {
        column!("name");
    }
    if valid.access.is_some() {
        column!("access");
    }
    if valid.color.is_some() {
        column!("color");
    }
    if valid.parent.is_some() {
        column!("parent_id");
    }
    if valid.is_locked.is_some() {
        column!("is_locked");
    }
    if valid.archived_at.is_some() {
        column!("archived_at");
    }
    if valid.created_by.is_some() {
        column!("created_by_id");
    }
    if valid.view_props.is_some() {
        column!("view_props");
    }
    if valid.logo_props.is_some() {
        column!("logo_props");
    }
    if valid.description_html.is_some() {
        column!("description_html");
    }
    column!("description_stripped");
    sql.push_str(&format!(" WHERE id = ${tail}"));
    let mut query = sqlx::query(&sql).bind(user_id);
    if let Some(name) = &valid.name {
        query = query.bind(name);
    }
    if let Some(access) = valid.access {
        query = query.bind(access as i16);
    }
    if let Some(color) = &valid.color {
        query = query.bind(color);
    }
    if let Some(parent) = &valid.parent {
        query = query.bind(parent);
    }
    if let Some(locked) = valid.is_locked {
        query = query.bind(locked);
    }
    if let Some(archived) = &valid.archived_at {
        query = query.bind(archived);
    }
    if let Some(created_by) = &valid.created_by {
        query = query.bind(created_by);
    }
    if let Some(props) = &valid.view_props {
        query = query.bind(props.clone());
    }
    if let Some(props) = &valid.logo_props {
        query = query.bind(props.clone());
    }
    if valid.description_html.is_some() {
        query = query.bind(final_html.as_str());
    }
    query = query.bind(&final_stripped).bind(page_id);
    if let Err(error) = query.execute(pool).await {
        return db_write_denial(&error);
    }
    // The conditional `page_transaction` (`:187-192`): only when the RAW
    // `description_html` is truthy, with the pre-save snapshot as old and
    // the URL id — the same gate and shape as the description PATCH, so
    // the shared helper serves both.
    let page_id_text = page_id.to_string();
    if let Some(job) = description_page_txn_job(
        &page_id_text,
        Some(old_html.as_str()),
        body.get("description_html"),
    ) {
        enqueue_best_effort(pool, &job).await;
    }
    let timezone = match actor_timezone(pool, &gate.user_id).await {
        Ok(tz) => tz,
        Err(denial) => return denial.into_response(),
    };
    let render = PatchRender {
        page: RenderPage {
            id: page.id,
            name: valid.name.unwrap_or(page.name),
            owned_by_id: page.owned_by_id,
            access: valid.access.unwrap_or(stored_access),
            color: valid.color.unwrap_or(page.color),
            parent_id: valid.parent.unwrap_or(page.parent_id),
            is_locked: valid.is_locked.unwrap_or(page.is_locked),
            archived_at: valid.archived_at.unwrap_or(page.archived_at),
            workspace_id: page.workspace_id,
            created_at: page.created_at,
            updated_at: chrono::Utc::now(),
            created_by_id: valid.created_by.unwrap_or(page.created_by_id),
            updated_by_id: Some(user_id),
            view_props: valid.view_props.unwrap_or(page.view_props),
            logo_props: valid.logo_props.unwrap_or(page.logo_props),
            description_html: final_html,
        },
        label_ids: valid.label_ids,
        project_ids: valid.project_ids,
    };
    json_response(StatusCode::OK, render_patch_row(&render, &timezone))
}

/// `DELETE .../pages/<page_id>/` (`PageViewSet.destroy`, `base.py:368-419`).
pub async fn page_destroy(
    State(state): State<AppState>,
    Path((slug, project_raw, page_raw)): Path<(String, String, String)>,
    extension: Option<Extension<SessionHandle>>,
    req: Request,
) -> Response {
    let Ok(page_id) = parse_page_id(&page_raw) else {
        return crate::edge::proxy(State(state), req).await;
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let user_id = match actor_user_id(extension.clone()) {
        Ok(id) => id,
        Err(denial) => return denial.into_response(),
    };
    let project_id = match resolve_project_id(pool, &slug, &project_raw).await {
        Ok(id) => id,
        Err(denial) => return denial.into_response(),
    };
    if let Err(denial) = gate::resolve_gate(
        &state,
        "DELETE",
        &slug,
        &project_id,
        Some(page_id),
        extension,
    )
    .await
    {
        return denial.into_response();
    }
    // The scoped fetch (`:369-374`): no `try` here, so a miss 404s through
    // `handle_exception` (an unlinked page for a member, or a second
    // DELETE after the soft delete hid the row).
    let page = match fetch_scoped_page(pool, &slug, &project_id, &page_id).await {
        Ok(Some(page)) => page,
        Ok(None) => return Denial::ObjectNotFound.into_response(),
        Err(denial) => return denial.into_response(),
    };
    if page.archived_at.is_none() {
        return json_response(
            StatusCode::BAD_REQUEST,
            gate::DESTROY_MUST_ARCHIVE_BODY.to_owned(),
        );
    }
    // Owner-or-admin-20 (`:382-394`): the requester passes as the owner, or
    // with an active `role == 20` project row — anything else 403s.
    if page.owned_by_id != user_id {
        let admin: Option<(i16,)> = match sqlx::query_as(
            r#"SELECT pm.role FROM project_members pm
               JOIN workspaces w ON w.id = pm.workspace_id
               WHERE pm.member_id = $1 AND pm.project_id = $2 AND w.slug = $3
               AND pm.role = 20 AND pm.is_active AND pm.deleted_at IS NULL"#,
        )
        .bind(user_id)
        .bind(project_id)
        .bind(slug.as_str())
        .fetch_optional(pool)
        .await
        {
            Ok(admin) => admin,
            Err(_) => return Denial::ServerError.into_response(),
        };
        if admin.is_none() {
            return json_response(
                StatusCode::FORBIDDEN,
                gate::DESTROY_OWNER_ADMIN_BODY.to_owned(),
            );
        }
    }
    // Children lose their parent (`:397-402`): a queryset `update`, so only
    // the one column moves — `updated_at` stays.
    if sqlx::query(
        r#"UPDATE pages child SET parent_id = NULL FROM project_pages pp, workspaces w
           WHERE child.parent_id = $1 AND child.workspace_id = w.id AND w.slug = $2
           AND pp.page_id = child.id AND pp.project_id = $3 AND pp.deleted_at IS NULL
           AND child.deleted_at IS NULL"#,
    )
    .bind(page_id)
    .bind(slug.as_str())
    .bind(project_id)
    .execute(pool)
    .await
    .is_err()
    {
        return Denial::ServerError.into_response();
    }
    // `page.delete()` (`:404`): the soft delete — `deleted_at` plus the
    // full-save `updated_at`/`updated_by` (`BaseModel.save` via crum); the
    // related-objects sweep enqueues best-effort below.
    if sqlx::query(
        r#"UPDATE pages SET deleted_at = now(), updated_at = now(), updated_by_id = $1
           WHERE id = $2"#,
    )
    .bind(user_id)
    .bind(page_id)
    .execute(pool)
    .await
    .is_err()
    {
        return Denial::ServerError.into_response();
    }
    let sweep = pidash_jobs::celery::CeleryTaskMessage::new(
        pidash_jobs::tasks_cleanup::deletion::SOFT_DELETE_TASK,
        vec![
            Value::String("db".to_owned()),
            Value::String("page".to_owned()),
            Value::String(page_id.to_string()),
            Value::Null,
        ],
        Default::default(),
    );
    enqueue_best_effort(
        pool,
        &pidash_jobs::queue::NewJob::new(
            sweep.task.clone(),
            Value::Array(sweep.args.clone()),
            Value::Object(sweep.kwargs.clone()),
        ),
    )
    .await;
    // Favorite cleanup (`:406-411`): the same scoped filter, soft — only
    // `deleted_at` moves. Recent-visit cleanup (`:413-418`): hard
    // (`delete(soft=False)`), live rows only like the filtered queryset.
    if sqlx::query(
        r#"UPDATE user_favorites SET deleted_at = now() FROM workspaces w
           WHERE user_favorites.project_id = $1 AND user_favorites.workspace_id = w.id
           AND w.slug = $2 AND user_favorites.entity_identifier = $3
           AND user_favorites.entity_type = 'page' AND user_favorites.deleted_at IS NULL"#,
    )
    .bind(project_id)
    .bind(slug.as_str())
    .bind(page_id)
    .execute(pool)
    .await
    .is_err()
    {
        return Denial::ServerError.into_response();
    }
    if sqlx::query(
        r#"DELETE FROM user_recent_visits USING workspaces w
           WHERE user_recent_visits.project_id = $1 AND user_recent_visits.workspace_id = w.id
           AND w.slug = $2 AND user_recent_visits.entity_identifier = $3
           AND user_recent_visits.entity_name = 'page' AND user_recent_visits.deleted_at IS NULL"#,
    )
    .bind(project_id)
    .bind(slug.as_str())
    .bind(page_id)
    .execute(pool)
    .await
    .is_err()
    {
        return Denial::ServerError.into_response();
    }
    StatusCode::NO_CONTENT.into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pidash_services::app_pages::shape::{
        DECODE_FAILED_MESSAGE, HTML_SANITIZE_FAILED_MESSAGE, HTML_TOO_LARGE_MESSAGE,
        INVALID_BINARY_PREFIX,
    };

    const F30_02: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/app_pages/serializers/page_binary_update.golden.json"
    );
    const F30_10: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/app_pages/tasks/publish.golden.json"
    );
    const F30_11: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/app_pages/handlers/io.golden.json"
    );
    const F30_01: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/app_pages/serializers/page_serializers.golden.json"
    );

    fn golden(path: &str) -> Value {
        let raw = std::fs::read_to_string(path).expect("fixture golden exists");
        serde_json::from_str(&raw).expect("fixture golden is valid JSON")
    }

    fn action<'a>(parsed: &'a Value, name: &str) -> &'a Value {
        parsed["actions"]
            .as_array()
            .expect("golden carries actions")
            .iter()
            .find(|action| action["action"] == name)
            .unwrap_or_else(|| panic!("golden carries {name}"))
    }

    /// Every response body/status byte this issue owns, as the contract
    /// suites pin them (F30-11).
    #[test]
    fn response_bodies_match_f30_11() {
        let parsed = golden(F30_11);
        let favorite_create = action(&parsed, "favorite create");
        assert_eq!(favorite_create["valid"]["status"], 204);
        assert!(favorite_create["permission"]
            .as_str()
            .expect("permission note")
            .contains("ADMIN, MEMBER"));
        let favorite_destroy = action(&parsed, "favorite destroy");
        assert_eq!(favorite_destroy["valid"]["status"], 204);
        let retrieve = action(&parsed, "description retrieve (streaming)");
        assert_eq!(retrieve["valid"]["status"], 200);
        assert_eq!(
            retrieve["headers"]["Content-Type"],
            "application/octet-stream"
        );
        assert_eq!(
            retrieve["headers"]["Content-Disposition"],
            r#"attachment; filename="page_description.bin""#
        );
        assert_eq!(DESCRIPTION_CONTENT_TYPE, "application/octet-stream");
        assert_eq!(
            DESCRIPTION_CONTENT_DISPOSITION,
            r#"attachment; filename="page_description.bin""#
        );
        let update = action(&parsed, "description partial_update");
        assert_eq!(update["valid"]["status"], 200);
        assert_eq!(
            update["valid"]["body"],
            serde_json::json!({"message": "Updated successfully"})
        );
        assert_eq!(update["locked"]["status"], 400);
        assert_eq!(
            update["locked"]["body"],
            serde_json::json!({"error_code": 4701, "error_message": "PAGE_LOCKED"})
        );
        assert_eq!(update["archived"]["status"], 400);
        assert_eq!(
            update["archived"]["body"],
            serde_json::json!({"error_code": 4702, "error_message": "PAGE_ARCHIVED"})
        );
        assert_eq!(
            DESCRIPTION_UPDATED_BODY,
            r#"{"message":"Updated successfully"}"#
        );
        assert_eq!(
            PAGE_LOCKED_BODY,
            r#"{"error_code":4701,"error_message":"PAGE_LOCKED"}"#
        );
        assert_eq!(
            PAGE_ARCHIVED_BODY,
            r#"{"error_code":4702,"error_message":"PAGE_ARCHIVED"}"#
        );
        // Both owned paths are registered; sibling D-30 paths keep proxying.
        let routes: Vec<&str> = parsed["routes"]
            .as_array()
            .expect("golden carries routes")
            .iter()
            .map(|route| route.as_str().expect("route strings"))
            .collect();
        assert!(routes.iter().any(|route| route.contains("favorite-pages")));
        assert!(routes.iter().any(|route| route.contains("description")));
    }

    /// Shared denial bodies both the gate and these handlers render.
    #[test]
    fn denial_bodies_are_byte_exact() {
        assert_eq!(
            UNAUTHENTICATED_BODY,
            r#"{"detail":"Authentication credentials were not provided."}"#
        );
        assert_eq!(
            OBJECT_NOT_FOUND_BODY,
            r#"{"error":"The required object does not exist."}"#
        );
        assert_eq!(PROJECT_NOT_FOUND_BODY, r#"{"detail":"Project not found"}"#);
        assert_eq!(
            INVALID_PAYLOAD_BODY,
            r#"{"error":"The payload is not valid"}"#
        );
        assert_eq!(
            SERVER_ERROR_BODY,
            r#"{"error":"Something went wrong please try again later"}"#
        );
        let (status, _) = Denial::PageLocked.status_and_body();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let (status, _) = Denial::PageArchived.status_and_body();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let (status, _) = Denial::ProjectNotFound.status_and_body();
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (status, _) = Denial::ObjectNotFound.status_and_body();
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (status, _) = Denial::Unauthorized.status_and_body();
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        let (status, _) = Denial::Forbidden.status_and_body();
        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    /// F30-02 golden vectors replayed through the PATCH field path.
    #[test]
    fn golden_vectors_replay_f30_02() {
        let parsed = golden(F30_02);
        let cases = parsed["cases"].as_array().expect("golden carries cases");
        let case = |name: &str| {
            cases
                .iter()
                .find(|case| case["name"] == name)
                .unwrap_or_else(|| panic!("golden carries {name}"))
        };
        // Decode failure message.
        let fail = case("base64 decode fail");
        assert_eq!(
            fail["output"]["errors"]["description_binary"][0],
            serde_json::json!(DECODE_FAILED_MESSAGE)
        );
        // Suspicious-content message (with the `Invalid binary data: ` prefix).
        let suspicious = case("binary validator fail (suspicious content)");
        assert_eq!(
            suspicious["output"]["errors"]["description_binary"][0],
            serde_json::json!(format!(
                "{INVALID_BINARY_PREFIX}Binary data contains suspicious content patterns"
            ))
        );
        // Empty binary passes through (falsy guard, no decode attempted).
        let empty = case("empty binary passes through");
        assert_eq!(empty["input"]["description_binary"], serde_json::json!(""));
        let patch = validate_patch_body(&serde_json::json!({"description_binary": ""}))
            .expect("empty binary");
        assert_eq!(patch.binary, Some(Vec::new()));
        // HTML sanitize substitution replaces the input with the cleaned HTML.
        let sanitize = case("HTML sanitize substitution");
        let patch = validate_patch_body(&serde_json::json!({
            "description_html": sanitize["input"]["description_html"].clone()
        }))
        .expect("sanitizable html");
        assert_eq!(
            patch.html.as_deref(),
            sanitize["output"]["description_html_applied"].as_str()
        );
        // Empty HTML passes through.
        let patch =
            validate_patch_body(&serde_json::json!({"description_html": ""})).expect("empty html");
        assert_eq!(patch.html, Some(String::new()));
    }

    /// F30-02 binary vectors through the PATCH field path.
    #[test]
    fn binary_vectors_match_f30_02() {
        // Valid base64 decodes to bytes (the serializer yields decoded
        // bytes, never the input string).
        let patch = validate_patch_body(&serde_json::json!({"description_binary": "iVBORw0KGgo="}))
            .expect("valid binary");
        assert_eq!(
            patch.binary,
            Some(vec![0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'])
        );
        // Decode failure.
        let errors = match validate_patch_body(
            &serde_json::json!({"description_binary": "!!!not-base64!!!"}),
        ) {
            Err(PatchRejection::Fields(errors)) => errors,
            other => panic!("expected field errors, got {other:?}"),
        };
        assert_eq!(
            errors,
            vec![(BINARY_FIELD.to_owned(), DECODE_FAILED_MESSAGE.to_owned())]
        );
        // Suspicious content (the `<html` pattern over the decoded bytes).
        let errors = match validate_patch_body(
            &serde_json::json!({"description_binary": "PGh0bWw+YmluYXJ5"}),
        ) {
            Err(PatchRejection::Fields(errors)) => errors,
            other => panic!("expected field errors, got {other:?}"),
        };
        assert_eq!(
            errors,
            vec![(
                BINARY_FIELD.to_owned(),
                format!("{INVALID_BINARY_PREFIX}Binary data contains suspicious content patterns")
            )]
        );
        // Empty passes through as an empty overwrite.
        let patch = validate_patch_body(&serde_json::json!({"description_binary": ""}))
            .expect("empty binary");
        assert_eq!(patch.binary, Some(Vec::new()));
        // Error bodies render byte-identically.
        assert_eq!(
            field_errors_body(&errors).to_string(),
            r#"{"description_binary":["Invalid binary data: Binary data contains suspicious content patterns"]}"#
        );
    }

    /// F30-02 HTML vectors: sanitize substitution, empty passthrough, and
    /// the untouched contract input.
    #[test]
    fn html_vectors_match_f30_02() {
        let patch = validate_patch_body(
            &serde_json::json!({"description_html": "<p>Hello</p><script>alert(1)</script>"}),
        )
        .expect("sanitizable html");
        assert_eq!(patch.html, Some("<p>Hello</p>".to_owned()));
        let patch =
            validate_patch_body(&serde_json::json!({"description_html": ""})).expect("empty html");
        assert_eq!(patch.html, Some(String::new()));
        // The contract suite's update input survives the cleaner verbatim.
        let patch = validate_patch_body(&serde_json::json!({"description_html": "<p>hi</p>"}))
            .expect("contract html");
        assert_eq!(patch.html, Some("<p>hi</p>".to_owned()));
        // Oversize HTML fails before the cleaner runs.
        let big = "p".repeat(crate::space::sanitize::MAX_HTML_BYTES + 1);
        let errors = match validate_patch_body(&serde_json::json!({"description_html": big})) {
            Err(PatchRejection::Fields(errors)) => errors,
            other => panic!("expected field errors, got {other:?}"),
        };
        assert_eq!(
            errors,
            vec![(HTML_FIELD.to_owned(), HTML_TOO_LARGE_MESSAGE.to_owned())]
        );
        assert_eq!(HTML_SANITIZE_FAILED_MESSAGE, "Failed to sanitize HTML");
    }

    /// `CharField` coercion edges (`fields.py`): null, wrong types, numeric
    /// coercion, whitespace-only blanking, JSON pass-through.
    #[test]
    fn char_coercion_edges() {
        // Null fails on both char fields.
        for field in [BINARY_FIELD, HTML_FIELD] {
            let mut map = Map::new();
            map.insert(field.to_owned(), Value::Null);
            let errors = match validate_patch_body(&Value::Object(map)) {
                Err(PatchRejection::Fields(errors)) => errors,
                other => panic!("expected field errors, got {other:?}"),
            };
            assert_eq!(
                errors,
                vec![(field.to_owned(), "This field may not be null.".to_owned())]
            );
        }
        // Bools, lists, dicts fail.
        for raw in [
            serde_json::json!(true),
            serde_json::json!([1]),
            serde_json::json!({"a": 1}),
        ] {
            let mut map = Map::new();
            map.insert(HTML_FIELD.to_owned(), raw);
            let errors = match validate_patch_body(&Value::Object(map)) {
                Err(PatchRejection::Fields(errors)) => errors,
                other => panic!("expected field errors, got {other:?}"),
            };
            assert_eq!(
                errors,
                vec![(HTML_FIELD.to_owned(), "Not a valid string.".to_owned())]
            );
        }
        // Whitespace-only validates to the empty overwrite (never stored
        // with its spaces).
        let patch = validate_patch_body(&serde_json::json!({"description_html": "   "}))
            .expect("blank html");
        assert_eq!(patch.html, Some(String::new()));
        // Unknown keys are ignored; absent keys are untouched.
        let patch = validate_patch_body(&serde_json::json!({"nope": 1})).expect("unknown keys");
        assert_eq!(
            patch,
            ValidPatch {
                binary: None,
                html: None,
                json: None,
                raw_html: None,
            }
        );
        // JSON accepts any value as-is; null stores NULL.
        let patch = validate_patch_body(&serde_json::json!({"description_json": {"ops": []}}))
            .expect("json object");
        assert_eq!(patch.json, Some(Some(serde_json::json!({"ops": []}))));
        let patch = validate_patch_body(&serde_json::json!({"description_json": Value::Null}))
            .expect("json null");
        assert_eq!(patch.json, Some(None));
    }

    /// Non-dictionary bodies (`serializers.py:342`).
    #[test]
    fn non_dict_bodies() {
        for (raw, datatype) in [
            (Value::Null, "NoneType"),
            (serde_json::json!(true), "bool"),
            (serde_json::json!(7), "int"),
            (serde_json::json!(1.5), "float"),
            (serde_json::json!("x"), "str"),
            (serde_json::json!([1]), "list"),
        ] {
            let rejection = validate_patch_body(&raw).expect_err("non-dict rejects");
            assert_eq!(rejection, PatchRejection::NonDict(datatype.to_owned()));
            assert_eq!(
                non_dict_body(datatype).to_string(),
                format!(
                    "{{\"non_field_errors\":[\"Invalid data. Expected a dictionary, but got {datatype}.\"]}}"
                )
            );
        }
    }

    /// Raw-HTML truthiness gate for `page_transaction` (`base.py:560`).
    #[test]
    fn raw_html_truthiness_gate() {
        assert!(python_truthy(&serde_json::json!("<p>x</p>")));
        assert!(python_truthy(&serde_json::json!("   ")));
        assert!(!python_truthy(&serde_json::json!("")));
        assert!(!python_truthy(&Value::Null));
        assert!(!python_truthy(&serde_json::json!(0)));
        assert!(python_truthy(&serde_json::json!(5)));
        assert!(!python_truthy(&serde_json::json!(false)));
        assert!(python_truthy(&serde_json::json!(true)));
        assert!(!python_truthy(&serde_json::json!([])));
        assert!(!python_truthy(&serde_json::json!({})));
    }

    /// F30-10 description publishes: conditional `page_transaction`,
    /// unconditional `track_page_version` inputs.
    #[test]
    fn description_publishes_match_f30_10() {
        let parsed = golden(F30_10);
        let calls = parsed["calls"].as_array().expect("golden carries calls");
        assert!(calls.iter().any(|call| call["source"]
            .as_str()
            .expect("source")
            .contains("base.py:560-565")));
        assert!(calls.iter().any(|call| call["source"]
            .as_str()
            .expect("source")
            .contains("base.py:568-572")));
        // Present truthy HTML publishes with the raw value and the snapshot.
        let job = description_page_txn_job(
            "11111111-1111-1111-1111-111111111111",
            Some("<p>old</p>"),
            Some(&serde_json::json!("<p>new</p>")),
        )
        .expect("html present publishes");
        assert_eq!(
            job.kwargs["new_description_html"],
            serde_json::json!("<p>new</p>")
        );
        assert_eq!(
            job.kwargs["old_description_html"],
            serde_json::json!("<p>old</p>")
        );
        // Absent, empty, and whitespace-falsy raws publish nothing (absent
        // and "" are falsy; note "   " IS truthy and publishes raw).
        assert!(description_page_txn_job("p", Some("<p>o</p>"), None).is_none());
        assert!(
            description_page_txn_job("p", Some("<p>o</p>"), Some(&serde_json::json!(""))).is_none()
        );
        let whitespace_job =
            description_page_txn_job("p", Some("<p>o</p>"), Some(&serde_json::json!("   ")))
                .expect("whitespace raw is truthy");
        assert_eq!(
            whitespace_job.kwargs["new_description_html"],
            serde_json::json!("   ")
        );
        // Non-string truthy raws pass through verbatim.
        let int_job = description_page_txn_job("p", Some("<p>o</p>"), Some(&serde_json::json!(5)))
            .expect("int raw publishes");
        assert_eq!(int_job.kwargs["new_description_html"], serde_json::json!(5));
        assert_eq!(int_job.task, job.task);
        // The version publish input is the `:552` dump.
        assert_eq!(
            pidash_jobs::app_pages::existing_instance_json(Some("<p>old</p>")),
            r#"{"description_html": "<p>old</p>"}"#
        );
    }

    /// Favorite constants against the models fixture sources.
    #[test]
    fn favorite_contract_constants() {
        assert_eq!(FAVORITE_ENTITY_TYPE, "page");
        assert_eq!(FAVORITE_DEFAULT_SEQUENCE, 65535.0);
        assert_eq!(FAVORITE_SEQUENCE_STEP, 10000.0);
    }

    /// Every response body/status byte the state ops own, as the contract
    /// suite pins them (F30-11).
    #[test]
    fn state_op_bodies_match_f30_11() {
        let parsed = golden(F30_11);
        let archive = action(&parsed, "archive");
        assert_eq!(archive["valid"]["status"], 200);
        assert_eq!(
            archive["valid"]["body"]["archived_at"].as_str().expect("archived_at key"),
            "<str(datetime.now()) \u{2014} SECOND now() call, may differ from DB value (double-now)>"
        );
        assert_eq!(archive["non_owner_non_admin"]["status"], 400);
        assert_eq!(
            archive["non_owner_non_admin"]["body"],
            serde_json::json!({"error": "Only the owner or admin can archive the page"})
        );
        assert_eq!(
            ARCHIVE_OWNER_ADMIN_BODY,
            r#"{"error":"Only the owner or admin can archive the page"}"#
        );
        assert_eq!(ARCHIVE_OWNER_ADMIN_BODY, gate::ARCHIVE_OWNER_ADMIN_BODY);
        let unarchive = action(&parsed, "unarchive");
        assert_eq!(unarchive["valid"]["status"], 204);
        assert_eq!(unarchive["non_owner_non_admin"]["status"], 400);
        assert_eq!(
            unarchive["non_owner_non_admin"]["body"],
            serde_json::json!({"error": "Only the owner or admin can un archive the page"})
        );
        assert_eq!(
            UNARCHIVE_OWNER_ADMIN_BODY,
            r#"{"error":"Only the owner or admin can un archive the page"}"#
        );
        assert_eq!(UNARCHIVE_OWNER_ADMIN_BODY, gate::UNARCHIVE_OWNER_ADMIN_BODY);
        assert!(UNARCHIVE_OWNER_ADMIN_BODY.contains("un archive"));
        let lock = action(&parsed, "lock");
        assert_eq!(lock["valid"]["status"], 204);
        let unlock = action(&parsed, "unlock");
        assert_eq!(unlock["valid"]["status"], 204);
        let access = action(&parsed, "access");
        assert_eq!(access["valid"]["status"], 204);
        assert_eq!(access["non_owner_change"]["status"], 400);
        assert_eq!(
            access["non_owner_change"]["body"],
            serde_json::json!({"error": "Access cannot be updated since this page is owned by someone else"})
        );
        assert_eq!(
            ACCESS_OWNER_BODY,
            r#"{"error":"Access cannot be updated since this page is owned by someone else"}"#
        );
        assert_eq!(ACCESS_OWNER_BODY, gate::ACCESS_OWNER_BODY);
        // The three owned paths are registered with Django's methods.
        let routes: Vec<&str> = parsed["routes"]
            .as_array()
            .expect("golden carries routes")
            .iter()
            .map(|route| route.as_str().expect("route strings"))
            .collect();
        assert!(routes.iter().any(|route| route.contains("archive")));
        assert!(routes.iter().any(|route| route.contains("lock")));
        assert!(routes.iter().any(|route| route.contains("access")));
    }

    /// The executable CTE is the fixture CTE verbatim (F30-08,
    /// `base.py:59-72`), modulo Django `%s` vs sqlx `$n` placeholders.
    #[test]
    fn state_cte_matches_f30_08_verbatim() {
        let verbatim = pidash_services::app_pages::queries::archive_cte_sql().replace("%s", "{}");
        let executable = STATE_CTE_SQL.replace("$1", "{}").replace("$2", "{}");
        let squeeze = |sql: &str| sql.split_whitespace().collect::<Vec<_>>().join(" ");
        assert_eq!(squeeze(&executable), squeeze(&verbatim));
        assert_eq!(STATE_CTE_SQL.matches("$1").count(), 1);
        assert_eq!(STATE_CTE_SQL.matches("$2").count(), 1);
    }

    /// `str(datetime.now())` rendering (`base.py:337`): space separator,
    /// microseconds only when nonzero; the 200 body carries the single
    /// `archived_at` key.
    #[test]
    fn archive_body_renders_like_py_datetime() {
        use chrono::NaiveDate;
        let with_micros = NaiveDate::from_ymd_opt(2026, 9, 30)
            .expect("date")
            .and_hms_micro_opt(13, 5, 22, 123_456)
            .expect("datetime");
        assert_eq!(
            python_datetime_string(&with_micros),
            "2026-09-30 13:05:22.123456"
        );
        let whole_second = NaiveDate::from_ymd_opt(2026, 9, 30)
            .expect("date")
            .and_hms_opt(13, 5, 22)
            .expect("datetime");
        assert_eq!(python_datetime_string(&whole_second), "2026-09-30 13:05:22");
        assert_eq!(
            archived_at_body(&with_micros),
            r#"{"archived_at":"2026-09-30 13:05:22.123456"}"#
        );
        // Sub-microsecond residue is truncated like CPython's microsecond
        // clock (999ns never rounds up into the body).
        let sub_micro = NaiveDate::from_ymd_opt(2026, 9, 30)
            .expect("date")
            .and_hms_nano_opt(13, 5, 22, 999)
            .expect("datetime");
        assert_eq!(python_datetime_string(&sub_micro), "2026-09-30 13:05:22");
    }

    /// `access` value coercion (`base.py:272,287`): integers pass, integral
    /// floats and bools match Python equality, the rest is unstorable.
    #[test]
    fn access_value_coercion() {
        assert_eq!(access_int(&serde_json::json!(0)), Some(0));
        assert_eq!(access_int(&serde_json::json!(1)), Some(1));
        assert_eq!(access_int(&serde_json::json!(1.0)), Some(1));
        assert_eq!(access_int(&serde_json::json!(true)), Some(1));
        assert_eq!(access_int(&serde_json::json!(false)), Some(0));
        assert_eq!(access_int(&serde_json::json!(1.5)), None);
        assert_eq!(access_int(&serde_json::json!("1")), None);
        assert_eq!(access_int(&serde_json::json!(null)), None);
        assert_eq!(access_int(&serde_json::json!([1])), None);
        assert_eq!(access_int(&serde_json::json!({"a": 1})), None);
        // Out-of-domain integers do not fit the SMALLINT column.
        assert_eq!(
            access_int(&serde_json::json!(i64::from(i32::MAX) + 1)),
            None
        );
    }

    /// The assignment coercion follows `int(value)` (verified against
    /// `PositiveSmallIntegerField.get_prep_value` on the Django venv:
    /// `1.5` → `1`, `"1"` → `1`, `True` → `1`, `70000` passes prep and
    /// fails at the column).
    #[test]
    fn access_assign_matches_py_int() {
        let assign = |v: Option<serde_json::Value>| access_assign(v.as_ref());
        assert_eq!(assign(None), Some(0));
        assert_eq!(assign(Some(serde_json::json!(1))), Some(1));
        assert_eq!(assign(Some(serde_json::json!(1.0))), Some(1));
        assert_eq!(assign(Some(serde_json::json!(1.5))), Some(1));
        assert_eq!(assign(Some(serde_json::json!(-1.5))), Some(-1));
        assert_eq!(assign(Some(serde_json::json!(true))), Some(1));
        assert_eq!(assign(Some(serde_json::json!("1"))), Some(1));
        assert_eq!(assign(Some(serde_json::json!("  +1 "))), Some(1));
        assert_eq!(assign(Some(serde_json::json!("1_0"))), Some(10));
        assert_eq!(assign(Some(serde_json::json!("-2"))), Some(-2));
        // Out-of-`i16` integers pass prep; the SMALLINT column rejects
        // them (bound as `i32` so the database itself says no).
        assert_eq!(assign(Some(serde_json::json!(70_000))), Some(70_000));
        assert_eq!(assign(Some(serde_json::json!("abc"))), None);
        assert_eq!(assign(Some(serde_json::json!("1.5"))), None);
        assert_eq!(assign(Some(serde_json::json!(""))), None);
        assert_eq!(assign(Some(serde_json::json!("0x1"))), None);
        assert_eq!(assign(Some(serde_json::json!("1__0"))), None);
        assert_eq!(assign(Some(serde_json::json!(null))), None);
        assert_eq!(assign(Some(serde_json::json!([1]))), None);
        assert_eq!(assign(Some(serde_json::json!({"a": 1}))), None);
        assert_eq!(assign(Some(serde_json::json!(1e30))), None);
    }

    /// The unarchive detach predicate (F30-08 `:360-362`): only a page
    /// with a parent whose parent is still archived is reparented; the
    /// archived parent keeps its timestamp while the subtree clears.
    #[test]
    fn unarchive_hierarchy_matches_f30_08() {
        const F30_08: &str = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/app_pages/queries/archive_cte.rows.json"
        );
        let raw = std::fs::read_to_string(F30_08).expect("fixture exists");
        let fixture: Value = serde_json::from_str(&raw).expect("fixture is valid JSON");
        let child = fixture["after_unarchive_child_with_archived_parent"]
            .as_array()
            .expect("after unarchive")
            .iter()
            .find(|row| row["id"] == "C")
            .expect("child row");
        assert!(child["archived_at"].is_null());
        assert!(child["parent_id"].is_null());
        let root = fixture["after_unarchive_child_with_archived_parent"]
            .as_array()
            .expect("after unarchive")
            .iter()
            .find(|row| row["id"] == "R")
            .expect("root row");
        assert_eq!(root["archived_at"], "<now1>");
        // Both ported bugs are recorded on the fixture.
        let bugs = fixture["bugs"].as_array().expect("bugs array");
        assert!(bugs
            .iter()
            .any(|bug| bug.as_str().unwrap_or("").contains("datetime.now")));
    }

    /// Page writes: the create 201 shape is the detail field list
    /// (F30-01 `field_list` + `description_html`), in order.
    #[test]
    fn write_create_shape_matches_f30_01() {
        let parsed = golden(F30_01);
        let mut expected: Vec<String> = parsed["field_list"]
            .as_array()
            .expect("golden carries field_list")
            .iter()
            .map(|field| field.as_str().expect("field names are strings").to_owned())
            .collect();
        expected.retain(|field| field.as_str() != "labels");
        expected.push("description_html".to_owned());
        let row = sample_detail_row();
        let rendered = render_create_row(&row, &chrono_tz::UTC);
        let body: Value = serde_json::from_str(&rendered).expect("renders JSON");
        let keys: Vec<String> = body
            .as_object()
            .expect("object body")
            .keys()
            .cloned()
            .collect();
        // `serde_json` keeps insertion order (`preserve_order`), so the
        // parsed key order is the render order.
        assert_eq!(keys, expected);
        assert_eq!(keys.len(), 19);
        // Spot values: FKs render as PK strings, annotations render, the
        // default HTML survives.
        assert_eq!(body["name"], serde_json::json!("Created page"));
        assert_eq!(body["description_html"], serde_json::json!("<p></p>"));
        assert_eq!(body["is_favorite"], serde_json::json!(false));
        assert_eq!(body["label_ids"], serde_json::json!([]));
        assert_eq!(
            body["project_ids"],
            serde_json::json!(["33333333-3333-3333-3333-333333333333"])
        );
        assert_eq!(
            body["created_at"],
            serde_json::json!("2026-09-30T23:09:27.150286Z")
        );
    }

    /// Page writes: the PATCH 200 shape is the detail list minus the three
    /// annotation-backed keys (F30 `PATCH_ROW_KEYS` in `test_pages.py`),
    /// with `label_ids` / `project_ids` rendered only when the request set
    /// them.
    #[test]
    fn write_patch_shape_matches_contract_keys() {
        let row = sample_detail_row();
        let base = PatchRender {
            page: render_page_from(&row),
            label_ids: None,
            project_ids: None,
        };
        let keys = rendered_keys(&render_patch_row(&base, &chrono_tz::UTC));
        assert_eq!(keys.len(), 16);
        assert!(!keys.contains(&"is_favorite".to_owned()));
        assert!(!keys.contains(&"label_ids".to_owned()));
        assert!(!keys.contains(&"project_ids".to_owned()));
        assert_eq!(keys.last().unwrap(), "description_html");
        // Ad-hoc attributes render in field position when present.
        let with_ids = PatchRender {
            label_ids: Some(vec![]),
            project_ids: Some(vec![uuid::Uuid::parse_str(
                "33333333-3333-3333-3333-333333333333",
            )
            .unwrap()]),
            ..base
        };
        let keys = rendered_keys(&render_patch_row(&with_ids, &chrono_tz::UTC));
        assert_eq!(keys.len(), 18);
        let logo = keys.iter().position(|key| key == "logo_props").unwrap();
        assert_eq!(keys[logo + 1], "label_ids");
        assert_eq!(keys[logo + 2], "project_ids");
        assert_eq!(keys.last().unwrap(), "description_html");
    }

    /// Page writes: every probed validation vector through the pure field
    /// path (F30-01 error shapes, DRF 3.15.2 `fields.py` / `relations.py`).
    #[test]
    fn write_field_vectors() {
        // Char coercion: null fails, bools/composites fail, numbers
        // stringify, strings trim.
        assert_eq!(
            coerce_char(&Value::Null, true),
            Err("This field may not be null.")
        );
        assert_eq!(
            coerce_char(&serde_json::json!(true), true),
            Err("Not a valid string.")
        );
        assert_eq!(coerce_char(&serde_json::json!(5), true), Ok("5".to_owned()));
        assert_eq!(
            coerce_char(&serde_json::json!("  x  "), true),
            Ok("x".to_owned())
        );
        // Detail HTML rejects blank (allow_blank=False), even whitespace.
        assert_eq!(
            coerce_char(&serde_json::json!(""), false),
            Err("This field may not be blank.")
        );
        assert_eq!(
            coerce_char(&serde_json::json!("   "), false),
            Err("This field may not be blank.")
        );
        // Access choices: "0"/0 pass, True renders Python-style.
        assert_eq!(coerce_access(&serde_json::json!(0)), Ok(0));
        assert_eq!(coerce_access(&serde_json::json!("1")), Ok(1));
        assert_eq!(
            coerce_access(&serde_json::json!(7)),
            Err("\"7\" is not a valid choice.".to_owned())
        );
        assert_eq!(
            coerce_access(&serde_json::json!(true)),
            Err("\"True\" is not a valid choice.".to_owned())
        );
        // Booleans: case-insensitive sets, numeric 1/0 (and 1.0/0.0).
        assert_eq!(coerce_bool(&serde_json::json!("TRUE")), Ok(true));
        assert_eq!(coerce_bool(&serde_json::json!("yes")), Ok(true));
        assert_eq!(coerce_bool(&serde_json::json!(1)), Ok(true));
        assert_eq!(coerce_bool(&serde_json::json!(0.0)), Ok(false));
        assert_eq!(
            coerce_bool(&serde_json::json!("maybe")),
            Err("Must be a valid boolean.")
        );
        // Dates: strict YYYY-MM-DD, null allowed, "" is invalid.
        assert_eq!(
            coerce_date(&serde_json::json!("2026-01-02")),
            Ok(Some(chrono::NaiveDate::from_ymd_opt(2026, 1, 2).unwrap()))
        );
        assert_eq!(coerce_date(&Value::Null), Ok(None));
        assert!(coerce_date(&serde_json::json!("")).is_err());
        assert!(coerce_date(&serde_json::json!("not-a-date")).is_err());
        // UUID items: dashed/simple/braced/urn parse, ints via UUID(int),
        // bools included, anything else invalid.
        let dashed = "f135d694-e5f7-40a2-a12f-40f55e2dd5a5";
        assert_eq!(
            coerce_uuid_item(&serde_json::json!(dashed)),
            Ok(uuid::Uuid::parse_str(dashed).unwrap())
        );
        assert_eq!(
            coerce_uuid_item(&serde_json::json!(5)),
            Ok(uuid::Uuid::from_u128(5))
        );
        assert!(coerce_uuid_item(&serde_json::json!("x")).is_err());
        assert_eq!(
            coerce_uuid_item(&serde_json::json!("x")),
            Err("Must be a valid UUID.")
        );
        // Curly-quote PK errors and Invalid-pk bodies.
        assert_eq!(
            curly_uuid_error("not-a-uuid"),
            "\u{201c}not-a-uuid\u{201d} is not a valid UUID."
        );
        assert_eq!(
            invalid_pk_error("not-a-uuid"),
            "Invalid pk \"not-a-uuid\" - object does not exist."
        );
        // UUID(hex=) forms.
        assert!(parse_uuid_hex(dashed).is_some());
        assert!(parse_uuid_hex(&dashed.replace('-', "")).is_some());
        assert!(parse_uuid_hex(&format!("{{{dashed}}}")).is_some());
        assert!(parse_uuid_hex("not-a-uuid").is_none());
    }

    /// Page writes: the 400 error-body renderer (flat + indexed arms,
    /// declaration order, compact, literal UTF-8).
    #[test]
    fn write_error_bodies_render() {
        let errors: OrderedErrors = vec![
            (
                "name".to_owned(),
                FieldError::Flat(vec!["This field may not be null.".to_owned()]),
            ),
            (
                "labels".to_owned(),
                FieldError::Indexed(vec![(
                    0,
                    vec!["\u{201c}nope\u{201d} is not a valid UUID.".to_owned()],
                )]),
            ),
        ];
        assert_eq!(
            write_errors_body(&errors),
            "{\"name\":[\"This field may not be null.\"],\"labels\":{\"0\":[\"\u{201c}nope\u{201d} is not a valid UUID.\"]}}"
        );
        assert_eq!(
            write_errors_body(&[(
                "access".to_owned(),
                FieldError::Flat(vec!["\"7\" is not a valid choice.".to_owned()])
            )]),
            "{\"access\":[\"\\\"7\\\" is not a valid choice.\"]}"
        );
        // Declaration order follows Meta.fields, detail appends HTML last.
        assert_eq!(WRITE_VALIDATION_ORDER.last(), Some(&"description_html"));
        assert!(
            WRITE_VALIDATION_ORDER
                .iter()
                .position(|field| *field == "labels")
                .unwrap()
                < WRITE_VALIDATION_ORDER
                    .iter()
                    .position(|field| *field == "parent")
                    .unwrap()
        );
    }

    /// Page writes: denial bodies and statuses (F30-11 write actions +
    /// the gate inline guards they share).
    #[test]
    fn write_denial_bodies() {
        assert_eq!(
            VALID_DETAIL_BODY,
            r#"{"error":"Please provide valid detail"}"#
        );
        let (status, _) = Denial::ValidDetail.status_and_body();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let (status, body) =
            Denial::ForbiddenError("Only admin or owner can delete the page".to_owned())
                .status_and_body();
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(
            body,
            r#"{"error":"Only admin or owner can delete the page"}"#
        );
        assert_eq!(
            gate::ACCESS_OWNER_BODY,
            r#"{"error":"Access cannot be updated since this page is owned by someone else"}"#
        );
        assert_eq!(
            gate::DESTROY_MUST_ARCHIVE_BODY,
            r#"{"error":"The page should be archived before deleting"}"#
        );
        // F30-11 write-action statuses the handlers answer.
        let parsed = golden(F30_11);
        let action = |name: &str| {
            parsed["actions"]
                .as_array()
                .expect("golden carries actions")
                .iter()
                .find(|action| action["action"] == name)
                .unwrap_or_else(|| panic!("golden carries {name}"))
        };
        assert_eq!(action("create")["valid"]["status"], 201);
        assert_eq!(action("create")["invalid"]["status"], 400);
        assert_eq!(action("partial_update")["valid"]["status"], 200);
        assert_eq!(
            action("partial_update")["missing_or_parent_missing"]["body"],
            serde_json::json!({
                "error": "Access cannot be updated since this page is owned by someone else"
            })
        );
        assert_eq!(action("destroy")["valid"]["status"], 204);
        assert_eq!(action("destroy")["not_archived"]["status"], 400);
        assert_eq!(action("destroy")["not_owner_not_admin20"]["status"], 403);
    }

    /// Page writes: publish gates (F30-10 `page_transaction` call sites):
    /// create always publishes with the raw-or-default HTML, partial_update
    /// only on truthy raw HTML with the pre-save snapshot as old.
    #[test]
    fn write_publishes_match_f30_10() {
        let parsed = golden(F30_10);
        let calls = parsed["calls"].as_array().expect("golden carries calls");
        assert!(calls.iter().any(|call| call["source"]
            .as_str()
            .expect("source")
            .contains("base.py:144-148")));
        assert!(calls.iter().any(|call| call["source"]
            .as_str()
            .expect("source")
            .contains("base.py:187-192")));
        // Create: unconditional; absent HTML defaults, present passes raw.
        let job = pidash_jobs::app_pages::page_transaction_create_job("new-id", None);
        assert_eq!(
            job.kwargs["new_description_html"],
            serde_json::json!("<p></p>")
        );
        let job = pidash_jobs::app_pages::page_transaction_create_job("new-id", Some("<p>Hi</p>"));
        assert_eq!(
            job.kwargs["new_description_html"],
            serde_json::json!("<p>Hi</p>")
        );
        assert_eq!(job.kwargs["old_description_html"], Value::Null);
        // Partial update: the shared raw-truthiness helper serves the
        // `:187` gate — whitespace-only still publishes raw, empty does not.
        assert!(
            description_page_txn_job("p", Some("<p>o</p>"), Some(&serde_json::json!("   ")))
                .is_some()
        );
        assert!(
            description_page_txn_job("p", Some("<p>o</p>"), Some(&serde_json::json!(""))).is_none()
        );
        assert!(description_page_txn_job("p", Some("<p>o</p>"), None).is_none());
        let job = description_page_txn_job(
            "p",
            Some("<p>old</p>"),
            Some(&serde_json::json!("<p>n</p>")),
        )
        .expect("truthy publishes");
        assert_eq!(
            job.kwargs["old_description_html"],
            serde_json::json!("<p>old</p>")
        );
    }

    fn sample_detail_row() -> DetailRow {
        let page_id = uuid::Uuid::parse_str("f135d694-e5f7-40a2-a12f-40f55e2dd5a5").unwrap();
        let owner_id = uuid::Uuid::parse_str("7eb6c4a8-5eb0-474c-9393-407970825d66").unwrap();
        let workspace_id = uuid::Uuid::parse_str("1b5a21c1-a5d7-4fc8-a9f5-806bb4e5fde6").unwrap();
        let project_id = uuid::Uuid::parse_str("33333333-3333-3333-3333-333333333333").unwrap();
        let created_at = chrono::DateTime::parse_from_rfc3339("2026-09-30T23:09:27.150286Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let updated_at = chrono::DateTime::parse_from_rfc3339("2026-09-30T23:09:27.150294Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        DetailRow {
            page: ScopedPage {
                id: page_id,
                name: "Created page".to_owned(),
                owned_by_id: owner_id,
                access: 0,
                color: String::new(),
                parent_id: None,
                is_locked: false,
                archived_at: None,
                workspace_id,
                created_at,
                updated_at,
                created_by_id: Some(owner_id),
                updated_by_id: None,
                view_props: serde_json::json!({"full_width": false}),
                logo_props: serde_json::json!({}),
                description_html: "<p></p>".to_owned(),
            },
            is_favorite: false,
            label_ids: vec![],
            project_ids: vec![project_id],
        }
    }

    fn render_page_from(row: &DetailRow) -> RenderPage {
        RenderPage {
            id: row.page.id,
            name: row.page.name.clone(),
            owned_by_id: row.page.owned_by_id,
            access: i32::from(row.page.access),
            color: row.page.color.clone(),
            parent_id: row.page.parent_id,
            is_locked: row.page.is_locked,
            archived_at: row.page.archived_at,
            workspace_id: row.page.workspace_id,
            created_at: row.page.created_at,
            updated_at: row.page.updated_at,
            created_by_id: row.page.created_by_id,
            updated_by_id: row.page.updated_by_id,
            view_props: row.page.view_props.clone(),
            logo_props: row.page.logo_props.clone(),
            description_html: row.page.description_html.clone(),
        }
    }

    fn rendered_keys(rendered: &str) -> Vec<String> {
        serde_json::from_str::<Value>(rendered)
            .expect("renders JSON")
            .as_object()
            .expect("object body")
            .keys()
            .cloned()
            .collect()
    }

    /// The streaming transport (`base.py:509-518`): one chunk per response —
    /// stored bytes when truthy, a single empty chunk when null — collected
    /// back byte-identically, with the exact content headers.
    #[tokio::test]
    async fn streaming_body_yields_single_chunk() {
        use http_body_util::BodyExt;
        for payload in [
            Bytes::from_static(b"\x89contract-binary-payload"),
            Bytes::new(),
        ] {
            let (mut sender, channel) = Channel::<Bytes, Infallible>::new(1);
            sender
                .send_data(payload.clone())
                .await
                .expect("buffer holds one chunk");
            drop(sender);
            let collected = axum::body::Body::new(channel)
                .collect()
                .await
                .expect("streaming body collects")
                .to_bytes();
            assert_eq!(collected, payload);
        }
        assert_eq!(DESCRIPTION_CONTENT_TYPE, "application/octet-stream");
        assert_eq!(
            DESCRIPTION_CONTENT_DISPOSITION,
            r#"attachment; filename="page_description.bin""#
        );
    }
}
