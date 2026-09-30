#![forbid(unsafe_code)]

//! App page favorites + description + state-op handlers (D-30, stage 5,
//! PIDASHCONV-332 and PIDASHCONV-328).
//!
//! Ports nine endpoints from `apps/api/pi_dash/app/views/page/base.py`
//! (routes in `apps/api/pi_dash/app/urls/page.py`):
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

/// Register the favorites + description + state-op routes
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
