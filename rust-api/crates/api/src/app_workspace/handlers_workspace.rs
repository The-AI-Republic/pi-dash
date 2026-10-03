#![forbid(unsafe_code)]

//! Workspace CRUD + member-workspaces + slug-check + themes + activity-export handlers (D-24, stage 5, PIDASHCONV-615).
//!
//! Ports `apps/api/pi_dash/app/views/workspace/base.py:55-254,351-420`:
//!
//! - `WorkSpaceViewSet` (`:55-201`): `get_queryset` (`:65-81`, member scope +
//!   `total_members` annotation, R1) + `create` (`:83-166`) + the pinned LIST
//!   400 (`:168-170`) + `partial_update` (`:172-174`, ADMIN) + `destroy`
//!   (`:183-201`, ADMIN, profile-pointer cleanup + track) + the inherited
//!   stock `retrieve` and PUT `update`, all over `get_queryset`.
//! - `UserWorkSpacesEndpoint.get` (`:209-240`, R2: role + total_members
//!   annotations, member prefetch, distinct) and
//!   `WorkSpaceAvailabilityCheckEndpoint.get` (`:244-254`).
//! - `WorkspaceThemeViewSet` (`:351-365`): Admin-gated list + create
//!   (201/400) + stock retrieve/patch/delete.
//! - `ExportWorkspaceUserActivityEndpoint.post` (`:379-420`, R5): CSV
//!   (`csv.QUOTE_ALL`, sanitized rows, 9-column header, `text/csv` +
//!   Content-Disposition).
//!
//! Routes (`app/urls/workspace.py:45-66,148-172`, `app/urls/user.py:67`,
//! mounted under `/api/`): `workspace-slug-check/` (W01),
//! `workspaces/` (W02: GET list, POST create),
//! `workspaces/<slug>/` (W03: GET/PUT/PATCH/DELETE),
//! `users/me/workspaces/` (U13),
//! `workspaces/<slug>/workspace-themes/` (W19: GET/POST),
//! `workspaces/<slug>/workspace-themes/<pk>/` (W20: GET/PATCH/DELETE),
//! `workspaces/<slug>/user-activity/<user_id>/export/` (W23: POST).
//! Every other method on those paths replays the view's auth/gate
//! prelude first (DRF answers 401/403 before the 405 lookup) and
//! answers DRF's 405 only for survivors, except OPTIONS which proxies
//! (DRF metadata is unportable).
//!
//! Fixture ids: F-W24-15
//! (`rust-api/fixtures/app_workspace/handlers/routes.golden.json`);
//! consumed F-W24-01 (via `ser_workspace`), F-W24-09 (via `queries_core`),
//! F-W24-13 (via `super::gates`), F-W24-14 (via `tasks`).
//! `WorkspaceThemeSerializer` shapes come from SER-B (`ser_invite`,
//! PIDASHCONV-601); `DISABLE_WORKSPACE_CREATION` resolves through the D-01
//! legacy config shim (`pidash_db::config::legacy`); the workspace
//! `timezone` choice list is `v1_projects::tz_zones` (single owner, never
//! forked); `RESTRICTED_WORKSPACE_SLUGS` is the foundation port.
//!
//! # Ported bugs (translate, don't redesign — also listed in the PR)
//!
//! * W02 list always 400s: the `@allow_permission(level="WORKSPACE")`
//!   decorator reads `kwargs["slug"]`, absent on the collection route, so
//!   `handle_exception` answers `{"error": "The required key does not
//!   exist."}` for every authenticated caller.
//! * PUT update is looser than PATCH: PUT runs under
//!   `permission_classes` only (Admin/Member) while PATCH adds the ADMIN
//!   decorator.
//! * The create 201 appends `total_members`/`role` AFTER `owner` (dict
//!   insertion order), while list/detail rows carry them in wire position;
//!   R1 rows have no `role` key at all (unannotated `SkipField`).
//! * The custom slug-charset message is unreachable: DRF's own slug regex
//!   (same charset) runs first, so charset failures render DRF's `Enter a
//!   valid "slug" ...` message; the restricted message survives via the
//!   model validator with identical bytes.
//! * Theme `deleted_at` is forced required (DRF uniqueness forcing from
//!   `unique_together`), unlike the workspace serializer where it is
//!   optional — same model field, different serializer.
//! * Soft delete rewrites the slug to `{slug}__{epoch}` and re-stamps
//!   `updated_by` on the deleted row.
//! * Export silently caps at 10000 rows with no truncation marker.
//! * `?search=` splits on commas and honors quotes (`search_smart_split`);
//!   `?owner=` 400s on unknown users (ModelChoice semantics), not just bad
//!   UUIDs.
//! * `logo_asset=""` validates to None; integer PKs look up `UUID(int)` and
//!   miss with `does_not_exist`; naive `deleted_at` input attaches the
//!   request user's zone.
//!
//! # Deliberate approximations (unreachable by the contract suite)
//!
//! * Bodies are JSON or urlencoded/multipart forms; exotic JSON charsets
//!   (utf-16/32) answer 500, and malformed-JSON `ParseError` detail text
//!   follows serde, not CPython.
//! * A non-`"already exists"` IntegrityError inside create answers the
//!   computed JSON 500 (Django's dispatch bug discards it for an HTML 500).
//! * Python-`repr` float edges in error strings.
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

use std::collections::BTreeMap;

use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Router;
use chrono::TimeZone;
use chrono_tz::Tz;
use serde_json::{Map, Value};

use pidash_auth::permissions::allow::AllowFacts;
use pidash_auth::permissions::workspace::WorkspaceFacts;
use pidash_auth::permissions::{ROLE_ADMIN, ROLE_MEMBER};
use pidash_services::app_workspace::{
    models_workspace, queries_core, ser_invite, ser_workspace, tasks,
};
use pidash_types::license::serializers_workspace::RESTRICTED_WORKSPACE_SLUGS;
use pidash_types::WorkspaceId;

use super::gates;
use crate::app_issues::{query_last, Denial, QueryMap};
use crate::serializer::render_datetime_in;
use crate::state::AppState;
use crate::v1_projects::tz_zones::PYTZ_COMMON_TIMEZONES;

/// `workspace-slug-check/` (`urls/workspace.py:45-49`, W01).
pub const SLUG_CHECK_PATH: &str = "/api/workspace-slug-check/";
/// `workspaces/` (`urls/workspace.py:50-54`, W02).
pub const WORKSPACES_PATH: &str = "/api/workspaces/";
/// `workspaces/<slug>/` (`urls/workspace.py:55-66`, W03).
pub const WORKSPACE_PATH: &str = "/api/workspaces/{slug}/";
/// `users/me/workspaces/` (`urls/user.py:67`, U13).
pub const ME_WORKSPACES_PATH: &str = "/api/users/me/workspaces/";
/// `workspaces/<slug>/workspace-themes/` (`urls/workspace.py:148-152`, W19).
pub const THEMES_PATH: &str = "/api/workspaces/{slug}/workspace-themes/";
/// `workspaces/<slug>/workspace-themes/<pk>/` (`urls/workspace.py:153-157`, W20).
pub const THEME_PATH: &str = "/api/workspaces/{slug}/workspace-themes/{pk}/";
/// `workspaces/<slug>/user-activity/<user_id>/export/`
/// (`urls/workspace.py:168-172`, W23).
pub const EXPORT_PATH: &str = "/api/workspaces/{slug}/user-activity/{user_id}/export/";

/// DRF `MethodNotAllowed` (`exceptions.py:194-199`): unmapped methods on
/// owned paths answer it inline (byte-identical, Django-independent);
/// OPTIONS proxies (DRF metadata is unportable).
fn method_not_allowed_response(method: &str) -> Response {
    json_response(
        StatusCode::METHOD_NOT_ALLOWED,
        format!("{{\"Detail\":\"Method \\\"{method}\\\" not allowed.\"}}"),
    )
}

/// Unowned methods on owned paths: DRF's `dispatch` runs `initial()`
/// (authentication + `check_permissions`) *before* the handler lookup
/// that produces the 405 (`rest_framework/views.py:497-504`), so each
/// route below replays its view's exact prelude — the actor first (401
/// when anonymous), then that view's `permission_classes` entry with
/// the actual request method — and answers the 405 bytes only for
/// survivors (the `v1_projects::handlers_state_estimate` precedent).
/// The `@allow_permission` decorators never run here: they wrap
/// dispatched actions, and no action dispatches on an unowned method.
/// Object lookups never run either (`get_object` lives inside the
/// actions), so bogus slugs/pks still 405 for survivors, never 404.
async fn slug_check_not_allowed(
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    method: axum::http::Method,
) -> HandlerResult {
    // `WorkSpaceAvailabilityCheckEndpoint` keeps the default
    // `[IsAuthenticated]`: any login survives.
    let _user_id = actor_user_id(extension)?;
    Ok(method_not_allowed_response(method.as_str()))
}

async fn workspaces_not_allowed(
    State(state): State<AppState>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    method: axum::http::Method,
) -> HandlerResult {
    // `WorkSpaceViewSet.permission_classes = [WorkSpaceBasePermission]`
    // with `workspace_slug=None` on the collection: PUT/PATCH/DELETE
    // deny (no membership row can match a null slug), POST and the
    // safe methods pass any login. The list-action decorator never
    // runs (no action dispatches), so no 400 here.
    let user_id = actor_user_id(extension)?;
    let pool = pool_of(&state)?;
    if let Err(response) = check_class_base(&pool, None, &user_id, method.as_str()).await {
        return Ok(response);
    }
    Ok(method_not_allowed_response(method.as_str()))
}

async fn workspace_not_allowed(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    method: axum::http::Method,
) -> HandlerResult {
    // Same class with the URL slug; POST and HEAD pass any login. No
    // object lookup: `get_object` never runs before the 405.
    let user_id = actor_user_id(extension)?;
    let pool = pool_of(&state)?;
    if let Err(response) = check_class_base(&pool, Some(&slug), &user_id, method.as_str()).await {
        return Ok(response);
    }
    Ok(method_not_allowed_response(method.as_str()))
}

async fn my_workspaces_not_allowed(
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    method: axum::http::Method,
) -> HandlerResult {
    // `UserWorkSpacesEndpoint` keeps the default `[IsAuthenticated]`:
    // any login survives.
    let _user_id = actor_user_id(extension)?;
    Ok(method_not_allowed_response(method.as_str()))
}

async fn themes_not_allowed(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    method: axum::http::Method,
) -> HandlerResult {
    // `WorkspaceThemeViewSet.permission_classes =
    // [WorkSpaceAdminPermission]`, active Admin/Member, every method.
    let user_id = actor_user_id(extension)?;
    let pool = pool_of(&state)?;
    if let Err(response) = check_class_admin(&pool, &slug, &user_id).await {
        return Ok(response);
    }
    Ok(method_not_allowed_response(method.as_str()))
}

async fn theme_not_allowed(
    State(state): State<AppState>,
    Path((slug, _pk)): Path<(String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    method: axum::http::Method,
) -> HandlerResult {
    // Same admin class; the pk is unread (no lookup before the 405).
    let user_id = actor_user_id(extension)?;
    let pool = pool_of(&state)?;
    if let Err(response) = check_class_admin(&pool, &slug, &user_id).await {
        return Ok(response);
    }
    Ok(method_not_allowed_response(method.as_str()))
}

async fn export_not_allowed(
    State(state): State<AppState>,
    Path((slug, _user_id_text)): Path<(String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    method: axum::http::Method,
) -> HandlerResult {
    // `ExportWorkspaceUserActivityEndpoint.permission_classes =
    // [WorkspaceEntityPermission]` with the actual method: safe
    // methods need any active membership, writes need Admin/Member.
    // No user lookup before the 405.
    let user_id = actor_user_id(extension)?;
    let pool = pool_of(&state)?;
    if let Err(response) = check_class_entity(&pool, &slug, &user_id, method.as_str()).await {
        return Ok(response);
    }
    Ok(method_not_allowed_response(method.as_str()))
}

/// Owned methods per route; anything else replays the view's prelude
/// and 405s for survivors (OPTIONS proxies).
pub fn routes() -> Router<AppState> {
    use axum::routing::{get, post};
    Router::new()
        .route(
            SLUG_CHECK_PATH,
            get(slug_check)
                .post(slug_check_not_allowed)
                .put(slug_check_not_allowed)
                .patch(slug_check_not_allowed)
                .delete(slug_check_not_allowed)
                .head(slug_check_not_allowed)
                .options(crate::edge::proxy),
        )
        .route(
            WORKSPACES_PATH,
            get(list_workspaces)
                .post(create_workspace)
                .put(workspaces_not_allowed)
                .patch(workspaces_not_allowed)
                .delete(workspaces_not_allowed)
                .head(workspaces_not_allowed)
                .options(crate::edge::proxy),
        )
        .route(
            WORKSPACE_PATH,
            get(retrieve_workspace)
                .put(update_workspace)
                .patch(partial_update_workspace)
                .delete(destroy_workspace)
                .post(workspace_not_allowed)
                .head(workspace_not_allowed)
                .options(crate::edge::proxy),
        )
        .route(
            ME_WORKSPACES_PATH,
            get(my_workspaces)
                .post(my_workspaces_not_allowed)
                .put(my_workspaces_not_allowed)
                .patch(my_workspaces_not_allowed)
                .delete(my_workspaces_not_allowed)
                .head(my_workspaces_not_allowed)
                .options(crate::edge::proxy),
        )
        .route(
            THEMES_PATH,
            get(list_themes)
                .post(create_theme)
                .put(themes_not_allowed)
                .patch(themes_not_allowed)
                .delete(themes_not_allowed)
                .head(themes_not_allowed)
                .options(crate::edge::proxy),
        )
        .route(
            THEME_PATH,
            get(retrieve_theme)
                .patch(partial_update_theme)
                .delete(destroy_theme)
                .post(theme_not_allowed)
                .put(theme_not_allowed)
                .head(theme_not_allowed)
                .options(crate::edge::proxy),
        )
        .route(
            EXPORT_PATH,
            post(export_activity)
                .get(export_not_allowed)
                .put(export_not_allowed)
                .patch(export_not_allowed)
                .delete(export_not_allowed)
                .head(export_not_allowed)
                .options(crate::edge::proxy),
        )
}

fn gate_for_list() -> &'static gates::Gate {
    &gates::gate_for("GET", "workspaces/")
        .expect("workspace list gate")
        .gate
}

fn gate_for_create() -> &'static gates::Gate {
    &gates::gate_for("POST", "workspaces/")
        .expect("workspace create gate")
        .gate
}

fn gate_for_retrieve() -> &'static gates::Gate {
    &gates::gate_for("GET", "workspaces/<slug>/")
        .expect("workspace retrieve gate")
        .gate
}

fn gate_for_update() -> &'static gates::Gate {
    &gates::gate_for("PUT", "workspaces/<slug>/")
        .expect("workspace update gate")
        .gate
}

fn gate_for_partial_update() -> &'static gates::Gate {
    &gates::gate_for("PATCH", "workspaces/<slug>/")
        .expect("workspace partial_update gate")
        .gate
}

fn gate_for_destroy() -> &'static gates::Gate {
    &gates::gate_for("DELETE", "workspaces/<slug>/")
        .expect("workspace destroy gate")
        .gate
}

fn gate_for_slug_check() -> &'static gates::Gate {
    &gates::gate_for("GET", "workspace-slug-check/")
        .expect("slug check gate")
        .gate
}

fn gate_for_my_workspaces() -> &'static gates::Gate {
    &gates::gate_for("GET", "users/me/workspaces/")
        .expect("my workspaces gate")
        .gate
}

fn gate_for_theme_list() -> &'static gates::Gate {
    &gates::gate_for("GET", "workspaces/<slug>/workspace-themes/")
        .expect("theme list gate")
        .gate
}

fn gate_for_theme_create() -> &'static gates::Gate {
    &gates::gate_for("POST", "workspaces/<slug>/workspace-themes/")
        .expect("theme create gate")
        .gate
}

fn gate_for_theme_detail(method: &str) -> &'static gates::Gate {
    &gates::gate_for(method, "workspaces/<slug>/workspace-themes/<pk>/")
        .expect("theme detail gate")
        .gate
}

fn gate_for_export() -> &'static gates::Gate {
    &gates::gate_for("POST", "workspaces/<slug>/user-activity/<user_id>/export/")
        .expect("export gate")
        .gate
}

// ---------------------------------------------------------------------------
// Shared request plumbing (pilot-2 / D-27 / D-28 precedent)
// ---------------------------------------------------------------------------

pub(crate) type HandlerResult = Result<Response, Denial>;

pub(crate) fn json_response(status: StatusCode, body: String) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body))
        .expect("view response")
}

pub(crate) fn empty_response(status: StatusCode) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(Vec::new()))
        .expect("empty response")
}

/// `request.user` from the Django session: missing session, missing key,
/// or a non-UUID id is anonymous → 401.
pub(crate) fn actor_user_id(
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

pub(crate) fn pool_of(state: &AppState) -> Result<sqlx::PgPool, Denial> {
    state
        .pools()
        .map(|pools| pools.primary().clone())
        .ok_or(Denial::ServerError)
}

/// `request.user.user_timezone` (`TimezoneMixin`): unknown zones 500
/// through the same branch Django's `zoneinfo` activation raises into.
pub(crate) async fn actor_timezone(
    pool: &sqlx::PgPool,
    user_id: &uuid::Uuid,
) -> Result<Tz, Denial> {
    let row: Option<(String,)> =
        sqlx::query_as(r#"SELECT u.user_timezone FROM users u WHERE u.id = $1"#)
            .bind(user_id)
            .fetch_optional(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    let (name,) = row.ok_or(Denial::ServerError)?;
    name.parse().map_err(|_| Denial::ServerError)
}

/// Best-effort deferred publish of a `.delay(...)` call (handlers
/// precedent): enqueue failures never change the response.
pub(crate) async fn enqueue_task(
    pool: &sqlx::PgPool,
    task: &str,
    args: Vec<Value>,
    kwargs: Map<String, Value>,
) {
    let message = pidash_jobs::celery::CeleryTaskMessage::new(task, args, kwargs);
    let job = pidash_jobs::queue::NewJob::new(
        message.task.clone(),
        Value::Array(message.args.clone()),
        Value::Object(message.kwargs.clone()),
    );
    if let Err(error) = pidash_jobs::queue::enqueue(pool, &job).await {
        tracing::warn!(%error, task, "task enqueue failed; response stands");
    }
}

/// Badly-formed UUIDs in paths render the `ValidationError` branch
/// (`app/views/base.py:126-130`): 400 `{"error": "Please provide valid
/// detail"}` (D-28 precedent for `<uuid:>` path params).
pub(crate) const INVALID_DETAIL_MSG: &str = "Please provide valid detail";

pub(crate) fn parse_uuid_or_invalid(raw: &str) -> Result<uuid::Uuid, Denial> {
    raw.parse::<uuid::Uuid>()
        .map_err(|_| Denial::BadError(INVALID_DETAIL_MSG.to_owned()))
}

/// The authenticated 403 of the `permission_classes` entries
/// (`APIView.permission_denied` with `message=None`): no `Denial`
/// variant renders it, so handlers answer it directly.
pub(crate) fn class_denied_response() -> Response {
    json_response(StatusCode::FORBIDDEN, gates::CLASS_DENIED_BODY.to_owned())
}

/// Map one gate outcome onto the wire: `Allow` runs the body, anything
/// else answers the exact denial bytes.
#[allow(clippy::result_large_err)]
pub(crate) fn apply_outcome(outcome: gates::GateOutcome) -> Result<(), Response> {
    match outcome {
        gates::GateOutcome::Allow => Ok(()),
        gates::GateOutcome::Deny => Err(Denial::Forbidden.into_response()),
        gates::GateOutcome::DenyClass => Err(class_denied_response()),
        gates::GateOutcome::MissingSlug => Err(json_response(
            StatusCode::BAD_REQUEST,
            gates::MISSING_KEY_BODY.to_owned(),
        )),
        gates::GateOutcome::Unauthenticated => Err(Denial::Unauthorized.into_response()),
    }
}

/// The caller's live membership role for one slug, plus whether any
/// (active or not) Admin row exists: the exact `(user, slug)` facts the
/// Python gates read — the workspace join is deliberately unscoped
/// (Django never scopes join tables), the member rows are live-scoped,
/// and the unfiltered Admin check drops `is_active` (the Owner
/// omission, ported even though no gate on these routes reads it).
async fn membership_facts(
    pool: &sqlx::PgPool,
    slug: &str,
    user_id: &uuid::Uuid,
) -> Result<(Option<i32>, bool), Denial> {
    let row: Option<(Option<i16>, bool)> = sqlx::query_as(
        r#"SELECT
             (SELECT wm.role FROM workspace_members wm
              INNER JOIN workspaces w ON w.id = wm.workspace_id
              WHERE w.slug = $1 AND wm.member_id = $2 AND wm.is_active AND wm.deleted_at IS NULL),
             EXISTS(SELECT 1 FROM workspace_members wm
              INNER JOIN workspaces w ON w.id = wm.workspace_id
              WHERE w.slug = $1 AND wm.member_id = $2 AND wm.role = 20 AND wm.deleted_at IS NULL)"#,
    )
    .bind(slug)
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let (role, admin_unfiltered) = row.unwrap_or((None, false));
    Ok((role.map(i32::from), admin_unfiltered))
}

fn allow_facts_for(slug: &str, role: Option<i32>, allowed: &[i32]) -> AllowFacts {
    AllowFacts {
        workspace: WorkspaceId::from(slug),
        authenticated: true,
        is_workspace_member: role.is_some(),
        has_allowed_workspace_role: role.map(|role| allowed.contains(&role)).unwrap_or(false),
        is_creator: false,
        has_allowed_project_role: false,
        is_project_member: false,
        is_workspace_admin: role == Some(ROLE_ADMIN),
    }
}

fn class_facts_for(slug: &str, role: Option<i32>, admin_unfiltered: bool) -> WorkspaceFacts {
    WorkspaceFacts {
        workspace: WorkspaceId::from(slug),
        authenticated: true,
        has_admin_or_member_role: role
            .map(|role| role == ROLE_ADMIN || role == ROLE_MEMBER)
            .unwrap_or(false),
        has_admin_role: role == Some(ROLE_ADMIN),
        is_member: role.is_some(),
        is_admin_unfiltered: admin_unfiltered,
    }
}

/// Run `WorkSpaceBasePermission` for one method (W02 create, W03
/// retrieve/update): anonymous already 401'd.
#[allow(clippy::result_large_err)]
async fn check_class_base(
    pool: &sqlx::PgPool,
    slug: Option<&str>,
    user_id: &uuid::Uuid,
    method: &str,
) -> Result<(), Response> {
    let scope = gates::tenant_context(slug.unwrap_or_default());
    let facts = match slug {
        Some(slug) => {
            let (role, admin_unfiltered) = membership_facts(pool, slug, user_id)
                .await
                .map_err(|denial| denial.into_response())?;
            class_facts_for(slug, role, admin_unfiltered)
        }
        None => class_facts_for("", None, false),
    };
    apply_outcome(gates::decide_class_base(method, &scope, &facts))
}

/// Run the composed PATCH/DELETE gates on `workspaces/<slug>/`
/// (class step, then the ADMIN decorator).
#[allow(clippy::result_large_err)]
async fn check_base_then_allow(
    pool: &sqlx::PgPool,
    slug: &str,
    user_id: &uuid::Uuid,
    method: &str,
) -> Result<(), Response> {
    let (role, admin_unfiltered) = membership_facts(pool, slug, user_id)
        .await
        .map_err(|denial| denial.into_response())?;
    let scope = gates::tenant_context(slug);
    let allow = allow_facts_for(slug, role, &[ROLE_ADMIN]);
    let class = class_facts_for(slug, role, admin_unfiltered);
    apply_outcome(gates::decide_base_then_allow(
        method, &scope, &allow, &class,
    ))
}

/// Run `WorkSpaceAdminPermission` (themes, every method).
#[allow(clippy::result_large_err)]
async fn check_class_admin(
    pool: &sqlx::PgPool,
    slug: &str,
    user_id: &uuid::Uuid,
) -> Result<(), Response> {
    let (role, admin_unfiltered) = membership_facts(pool, slug, user_id)
        .await
        .map_err(|denial| denial.into_response())?;
    let scope = gates::tenant_context(slug);
    let facts = class_facts_for(slug, role, admin_unfiltered);
    apply_outcome(gates::decide_class_admin(&scope, &facts))
}

/// Run `WorkspaceEntityPermission` (export POST: a write).
#[allow(clippy::result_large_err)]
async fn check_class_entity(
    pool: &sqlx::PgPool,
    slug: &str,
    user_id: &uuid::Uuid,
    method: &str,
) -> Result<(), Response> {
    let (role, admin_unfiltered) = membership_facts(pool, slug, user_id)
        .await
        .map_err(|denial| denial.into_response())?;
    let scope = gates::tenant_context(slug);
    let facts = class_facts_for(slug, role, admin_unfiltered);
    apply_outcome(gates::decide_class_entity(method, &scope, &facts))
}

// ---------------------------------------------------------------------------
// Body negotiation (`Request._load_stream` + `_parse` + `select_parser`)
// ---------------------------------------------------------------------------

/// Django's upload caps (`settings/common.py` sets none, so the Django
/// defaults rule): past them the parsers raise into the generic 500.
const MAX_FORM_FIELDS: usize = 1000;
const MAX_FORM_FILES: usize = 1000;

/// One negotiated request body: decoded JSON, or form/multipart texts
/// with file presence per key (DRF merges files into `request.data`,
/// file-wins per key).
#[derive(Debug)]
pub(crate) struct InputBody {
    /// True for urlencoded/multipart (HTML-input blank + JSONString rules).
    pub is_html: bool,
    /// Last-wins values: the JSON object, or form texts (present-`''`
    /// kept unless the route spec skips the key).
    pub map: Map<String, Value>,
    /// Multipart keys carrying uploads (last filename wins): presence
    /// plus the name (some messages render it).
    pub files: BTreeMap<String, String>,
    /// The whole non-object JSON value (`null`, list, scalar) for the
    /// top-level arms. Stored separately (never as a map entry) so a
    /// genuine object containing an `""` key still validates as one.
    pub scalar: Option<Value>,
}

impl InputBody {
    fn json(value: Value) -> Self {
        match value {
            Value::Object(map) => Self {
                is_html: false,
                map,
                files: BTreeMap::new(),
                scalar: None,
            },
            other => Self {
                is_html: false,
                map: Map::new(),
                files: BTreeMap::new(),
                scalar: Some(other),
            },
        }
    }

    /// Whether the JSON body decoded to an object (non-objects take the
    /// `non_field_errors` / manual-`.get` arms, never field validation).
    fn json_is_object(&self) -> bool {
        !self.is_html && self.scalar.is_none()
    }

    /// The whole non-object JSON value (`null`, list, scalar) for the
    /// top-level arms. Only called when `json_is_object` is false.
    fn json_scalar(&self) -> &Value {
        self.scalar.as_ref().expect("non-object body")
    }

    /// DRF `request.data.get(key)`: the file wins when the key carries
    /// one, else the last text value, else missing.
    pub fn get(&self, key: &str) -> InputValue<'_> {
        if let Some(name) = self.files.get(key) {
            return InputValue::File(name);
        }
        match self.map.get(key) {
            Some(value) => InputValue::Value(value),
            None => InputValue::Missing,
        }
    }
}

/// One field lookup: missing, a JSON/form value, or a winning upload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InputValue<'a> {
    Missing,
    Value(&'a Value),
    File(&'a str),
}

/// Negotiate + parse the request body. Empty bytes decode to `{}` whatever
/// the content type is; a missing/unknown content type with a body 415s;
/// JSON (exact `application/json`, `*/*`, `application/*`) parses with the
/// charset applied; urlencoded and multipart forms parse last-wins.
/// `skip_blank` names the scalar fields where a present-but-empty form
/// value behaves as absent (`Field.get_value`: not required, no
/// `allow_blank`, no `allow_null`).
#[allow(clippy::result_large_err)]
pub(crate) fn negotiate_body(
    headers: &HeaderMap,
    bytes: &[u8],
    skip_blank: &[&str],
) -> Result<InputBody, Response> {
    if bytes.is_empty() {
        return Ok(InputBody {
            is_html: false,
            map: Map::new(),
            files: BTreeMap::new(),
            scalar: None,
        });
    }
    let content_type: String = headers
        .get(header::CONTENT_TYPE)
        .map(|value| value.as_bytes().iter().map(|byte| *byte as char).collect())
        .unwrap_or_default();
    if content_type.is_empty() {
        return Err(unsupported_media_type(""));
    }
    let (base, params) = split_content_type(&content_type);
    if base_matches_json(&base) {
        let text = decode_json_bytes(bytes, params.get("charset").map(String::as_str))
            .map_err(map_decode_error)?;
        let value: Value = serde_json::from_str(&text)
            .map_err(|error| parse_error_response(format!("JSON parse error - {error}")))?;
        return Ok(InputBody::json(value));
    }
    if base == "application/x-www-form-urlencoded" {
        let map = parse_urlencoded(bytes, skip_blank).map_err(|()| server_error_response())?;
        return Ok(InputBody {
            is_html: true,
            map,
            files: BTreeMap::new(),
            scalar: None,
        });
    }
    if base == "multipart/form-data" {
        let (map, files) = parse_multipart(
            bytes,
            params.get("boundary").map(String::as_str),
            skip_blank,
        )
        .map_err(|()| server_error_response())?;
        return Ok(InputBody {
            is_html: true,
            map,
            files,
            scalar: None,
        });
    }
    Err(unsupported_media_type(&content_type))
}

fn unsupported_media_type(content_type: &str) -> Response {
    // The content type is attacker-controlled: serde-escape it like the
    // parse-error arm (DRF's `JSONRenderer` escapes the same way).
    let message = format!("Unsupported media type \"{content_type}\" in request.");
    json_response(
        StatusCode::UNSUPPORTED_MEDIA_TYPE,
        format!(
            "{{\"Detail\":{}}}",
            serde_json::to_string(&message).expect("415 string")
        ),
    )
}

fn parse_error_response(detail: String) -> Response {
    json_response(
        StatusCode::BAD_REQUEST,
        format!(
            "{{\"Detail\":{}}}",
            serde_json::to_string(&detail).expect("detail string")
        ),
    )
}

fn server_error_response() -> Response {
    Denial::ServerError.into_response()
}

/// Split `type/subtype; p=v; ...` into the lowercased base and unquoted
/// params (DRF `parse_header_parameters` half).
fn split_content_type(content_type: &str) -> (String, BTreeMap<String, String>) {
    let mut parts = content_type.split(';');
    let base = parts.next().unwrap_or_default().trim().to_ascii_lowercase();
    let mut params = BTreeMap::new();
    for part in parts {
        let (key, value) = match part.split_once('=') {
            Some(pair) => pair,
            None => continue,
        };
        let key = key.trim().to_ascii_lowercase();
        let mut value = value.trim();
        if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
            value = &value[1..value.len() - 1];
        }
        params.insert(key, value.to_owned());
    }
    (base, params)
}

/// `media_type_matches(parser, request)` for the JSON parser:
/// `application/json` exactly, or a request-side wildcard (`*/*`,
/// `application/*` — verified live by the v1 negotiation port).
fn base_matches_json(base: &str) -> bool {
    matches!(base, "application/json" | "*/*" | "application/*")
}

/// JSON stream decode: utf-8/utf8/ascii strict, latin-1 total; any other
/// charset label is a `LookupError` 500 (exotic-but-valid labels like
/// utf-16 share the arm — documented approximation).
fn decode_json_bytes(bytes: &[u8], charset: Option<&str>) -> Result<String, String> {
    let label = charset.unwrap_or("utf-8").trim().to_ascii_lowercase();
    // `charset=""` (empty param) falls back to the default in CPython.
    let label = if label.is_empty() { "utf-8" } else { &label };
    match label {
        "utf-8" | "utf8" => std::str::from_utf8(bytes)
            .map(str::to_owned)
            .map_err(|error| {
                format!("JSON parse error - 'utf-8' codec can't decode bytes: {error}")
            }),
        "ascii" => {
            if bytes.is_ascii() {
                Ok(bytes.iter().map(|byte| *byte as char).collect())
            } else {
                Err("JSON parse error - 'ascii' codec can't decode bytes".to_owned())
            }
        }
        "latin-1" | "latin1" | "iso-8859-1" | "iso8859-1" => {
            Ok(bytes.iter().map(|byte| *byte as char).collect())
        }
        _ => Err("LOOKUP_ERROR".to_owned()),
    }
}

/// Translate a decode failure: the `LookupError` arm 500s, anything else
/// is the `ParseError` 400.
fn map_decode_error(error: String) -> Response {
    if error == "LOOKUP_ERROR" {
        server_error_response()
    } else {
        parse_error_response(error)
    }
}

/// Parse urlencoded bodies last-wins (`QueryDict`): `+` is a space,
/// `%XX` decodes UTF-8 lossily, bare keys carry `''`, past
/// `DATA_UPLOAD_MAX_NUMBER_OF_FIELDS` pairs the 500 arm fires.
fn parse_urlencoded(bytes: &[u8], skip_blank: &[&str]) -> Result<Map<String, Value>, ()> {
    let text = String::from_utf8_lossy(bytes);
    let mut map = Map::new();
    let mut count = 0usize;
    for pair in text.split('&') {
        count += 1;
        if count > MAX_FORM_FIELDS {
            return Err(());
        }
        let (raw_key, raw_value) = match pair.split_once('=') {
            Some(pair) => pair,
            None => (pair, ""),
        };
        let key = unquote_plus(raw_key);
        let value = unquote_plus(raw_value);
        if value.is_empty() && skip_blank.contains(&key.as_str()) {
            map.remove(&key);
            continue;
        }
        map.insert(key, Value::String(value));
    }
    Ok(map)
}

fn unquote_plus(raw: &str) -> String {
    let mut bytes_out: Vec<u8> = Vec::with_capacity(raw.len());
    let bytes = raw.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => {
                bytes_out.push(b' ');
                index += 1;
            }
            b'%' if index + 2 < bytes.len() + 1 => {
                let hex = &raw[index + 1..];
                let digits: String = hex.chars().take(2).collect();
                if digits.len() == 2 {
                    if let Ok(byte) = u8::from_str_radix(&digits, 16) {
                        bytes_out.push(byte);
                        index += 1 + digits.len();
                        continue;
                    }
                }
                bytes_out.push(b'%');
                index += 1;
            }
            byte => {
                bytes_out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&bytes_out).into_owned()
}

/// Parsed multipart body: text fields plus files.
type MultipartParts = (Map<String, Value>, BTreeMap<String, String>);

/// Parse multipart fields (Django `MultiPartParser`, fields-only half):
/// text parts last-wins with the skip-blank rule, file parts record
/// presence + filename (file-wins at lookup). A missing boundary or a
/// malformed frame is the `MultiPartParserError` 500 arm.
fn parse_multipart(
    bytes: &[u8],
    boundary: Option<&str>,
    skip_blank: &[&str],
) -> Result<MultipartParts, ()> {
    let boundary = boundary.unwrap_or_default();
    if boundary.is_empty() || boundary.len() > 200 {
        return Err(());
    }
    let delimiter = format!("--{boundary}");
    let text = String::from_utf8_lossy(bytes);
    let mut map = Map::new();
    let mut files = BTreeMap::new();
    let mut field_count = 0usize;
    let mut file_count = 0usize;
    // Split on boundary lines; the preamble precedes the first one and
    // the `--` suffixed delimiter closes the body.
    let mut parts = text.split(&delimiter).peekable();
    let _preamble = parts.next().ok_or(())?;
    for part in parts {
        let part = part
            .strip_prefix("\r\n")
            .or_else(|| part.strip_prefix('\n'))
            .unwrap_or(part);
        if part.starts_with("--") {
            break;
        }
        let (raw_headers, body) = split_part(part).ok_or(())?;
        let mut name: Option<String> = None;
        let mut filename: Option<String> = None;
        for header_line in raw_headers.lines() {
            let header_line = header_line.trim_end_matches('\r');
            let Some((key, value)) = header_line.split_once(':') else {
                continue;
            };
            if !key.trim().eq_ignore_ascii_case("content-disposition") {
                continue;
            }
            for param in value.split(';').skip(1) {
                let Some((param_key, param_value)) = param.split_once('=') else {
                    continue;
                };
                let mut param_value = param_value.trim();
                if param_value.len() >= 2
                    && param_value.starts_with('"')
                    && param_value.ends_with('"')
                {
                    param_value = &param_value[1..param_value.len() - 1];
                }
                match param_key.trim().to_ascii_lowercase().as_str() {
                    "name" => name = Some(param_value.to_owned()),
                    "filename" => filename = Some(param_value.to_owned()),
                    _ => {}
                }
            }
        }
        let Some(name) = name else {
            return Err(());
        };
        if name.is_empty() {
            return Err(());
        }
        if let Some(filename) = filename {
            file_count += 1;
            if file_count > MAX_FORM_FILES {
                return Err(());
            }
            files.insert(name, filename);
        } else {
            field_count += 1;
            if field_count > MAX_FORM_FIELDS {
                return Err(());
            }
            let value = body.to_owned();
            if value.is_empty() && skip_blank.contains(&name.as_str()) {
                map.remove(&name);
                continue;
            }
            map.insert(name, Value::String(value));
        }
    }
    Ok((map, files))
}

/// Split one part into headers + body (the body keeps its bytes minus the
/// framing CRLF).
fn split_part(part: &str) -> Option<(&str, String)> {
    let (raw_headers, body) = part
        .split_once("\r\n\r\n")
        .or_else(|| part.split_once("\n\n"))?;
    let body = body
        .strip_suffix("\r\n")
        .or_else(|| body.strip_suffix('\n'))
        .unwrap_or(body);
    Some((raw_headers, body.to_owned()))
}

// ---------------------------------------------------------------------------
// Python value kernels (truthiness, len, str, strip)
// ---------------------------------------------------------------------------

/// Python truthiness over JSON values (`if not name or not slug`,
/// `if not request.data.get("date")`).
pub(crate) fn py_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                int != 0
            } else if let Some(uint) = number.as_u64() {
                uint != 0
            } else {
                number.as_f64().map(|float| float != 0.0).unwrap_or(true)
            }
        }
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
    }
}

/// Python `len()` over JSON values: strings count code points, arrays
/// and objects count items; anything else is a `TypeError` (the 500
/// arm on the manual paths).
pub(crate) fn py_len(value: &Value) -> Option<usize> {
    match value {
        Value::String(text) => Some(text.chars().count()),
        Value::Array(items) => Some(items.len()),
        Value::Object(map) => Some(map.len()),
        _ => None,
    }
}

/// Python `str()` over JSON values (coercions, `ChoiceField` keys,
/// `company_role`, error strings).
pub(crate) fn py_str(value: &Value) -> String {
    match value {
        Value::Null => "None".to_owned(),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::Number(number) => py_num_str(number),
        Value::String(text) => text.clone(),
        Value::Array(_) | Value::Object(_) => py_repr(value),
    }
}

/// Python `str()` over JSON numbers: integers echo decimally, floats
/// render shortest-round-trip (`repr` rules: exponent iff the decimal
/// exponent is < -4 or >= 16, signed zero-padded 2-digit exponents).
pub(crate) fn py_num_str(number: &serde_json::Number) -> String {
    if let Some(int) = number.as_i64() {
        return int.to_string();
    }
    if let Some(uint) = number.as_u64() {
        return uint.to_string();
    }
    // Arbitrary-precision floats echo their input text; re-render
    // Python-style from the value instead.
    let Some(float) = number.as_f64() else {
        return number.to_string();
    };
    py_float_str(float)
}

pub(crate) fn py_float_str(float: f64) -> String {
    if float == 0.0 {
        if float.is_sign_negative() {
            return "-0.0".to_owned();
        }
        return "0.0".to_owned();
    }
    let debug = format!("{float:?}");
    let (mantissa, exponent) = match debug.split_once('e') {
        Some(pair) => pair,
        None => return debug,
    };
    let exponent: i32 = exponent.parse().unwrap_or(0);
    if !(-4..16).contains(&exponent) {
        let sign = if exponent < 0 { '-' } else { '+' };
        return format!("{mantissa}e{sign}{:02}", exponent.abs());
    }
    // Rust `{:?}` already expands these; normalize `-0.0`-style edges.
    if mantissa == "-0" || mantissa == "-0.0" {
        return "-0.0".to_owned();
    }
    debug
}

/// Python `repr()` over JSON values (container error strings): single
/// quotes unless the string holds one, C0/C1 escapes, `', '` item gaps.
pub(crate) fn py_repr(value: &Value) -> String {
    match value {
        Value::Null => "None".to_owned(),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::Number(number) => py_num_str(number),
        Value::String(text) => py_repr_str(text),
        Value::Array(items) => {
            let inner: Vec<String> = items.iter().map(py_repr).collect();
            format!("[{}]", inner.join(", "))
        }
        Value::Object(map) => {
            let inner: Vec<String> = map
                .iter()
                .map(|(key, item)| format!("{}: {}", py_repr_str(key), py_repr(item)))
                .collect();
            format!("{{{}}}", inner.join(", "))
        }
    }
}

fn py_repr_str(text: &str) -> String {
    let quote = if text.contains('\'') && !text.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::with_capacity(text.len() + 2);
    out.push(quote);
    for ch in text.chars() {
        match ch {
            _ if ch == quote => {
                out.push('\\');
                out.push(ch);
            }
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ if (ch as u32) < 0x20 || (0x7f..0xa0).contains(&(ch as u32)) => {
                out.push_str(&format!("\\x{:02x}", ch as u32));
            }
            _ => out.push(ch),
        }
    }
    out.push(quote);
    out
}

/// Python `str.strip()` (no args): Rust `trim()` plus `\x1c-\x1f`/`\x85`
/// (stripped by CPython, not `White_Space`).
pub(crate) fn py_strip(text: &str) -> &str {
    text.trim_matches(|ch: char| ch.is_whitespace() || matches!(ch, '\u{1c}'..='\u{1f}' | '\u{85}'))
}

/// A JSON string holding `\x00` (Django's
/// `ProhibitNullCharactersValidator`). Surrogates cannot occur in Rust
/// strings, so that validator is vacuous here.
fn has_null_char(text: &str) -> bool {
    text.contains('\x00')
}

// ---------------------------------------------------------------------------
// DRF field validators (verified live against DRF 3.15)
// ---------------------------------------------------------------------------

/// Collected serializer errors in field order: `{"field": ["message"]}`.
pub(crate) type FieldErrors = Vec<(String, Vec<String>)>;

pub(crate) fn field_errors_response(errors: FieldErrors) -> Response {
    let mut map = Map::with_capacity(errors.len());
    for (field, messages) in errors {
        map.insert(
            field,
            Value::Array(messages.into_iter().map(Value::String).collect()),
        );
    }
    json_response(StatusCode::BAD_REQUEST, Value::Object(map).to_string())
}

pub(crate) fn single_field_error(field: &str, message: String) -> Response {
    field_errors_response(vec![(field.to_owned(), vec![message])])
}

/// The top-level arms for non-object JSON bodies: `null` is `No data
/// provided`, anything else names its `type().__name__`.
pub(crate) fn non_object_body_response(value: &Value) -> Response {
    if value.is_null() {
        return single_field_error("non_field_errors", "No data provided".to_owned());
    }
    let datatype = match value {
        Value::Bool(_) => "bool",
        Value::Number(number) => {
            if number.is_i64() || number.is_u64() {
                "int"
            } else {
                "float"
            }
        }
        Value::String(_) => "str",
        Value::Array(_) => "list",
        Value::Null | Value::Object(_) => unreachable!("non-object arm"),
    };
    single_field_error(
        "non_field_errors",
        format!("Invalid data. Expected a dictionary, but got {datatype}."),
    )
}

/// `CharField` through `to_internal_value` + blank/max-length/null-char:
/// `Required`/`Null`/`Invalid`/`Blank`/`TooLong`/`NullChar`, else the
/// trimmed value. `is_html_empty_means` handles the HTML `''` mapping
/// the caller resolved (`None` for `allow_null` fields).
pub(crate) enum CharOutcome {
    /// Missing and not required (partial update, or an optional field).
    Skip,
    /// Validated string (trimmed).
    Value(String),
    /// HTML `''` on an `allow_null` field.
    Null,
    /// Field error message.
    Error(String),
}

pub(crate) fn validate_char_field(
    input: InputValue<'_>,
    required: bool,
    allow_blank: bool,
    allow_null: bool,
    max_length: Option<usize>,
) -> CharOutcome {
    let value = match input {
        InputValue::Missing | InputValue::File(_) => {
            // Uploads fail the type check below; missing honors required.
            if matches!(input, InputValue::Missing) {
                if required {
                    return CharOutcome::Error("This field is required.".to_owned());
                }
                return CharOutcome::Skip;
            }
            return CharOutcome::Error("Not a valid string.".to_owned());
        }
        InputValue::Value(value) => value,
    };
    if value.is_null() {
        if allow_null {
            return CharOutcome::Null;
        }
        return CharOutcome::Error("This field may not be null.".to_owned());
    }
    // `run_validation` tests the empty string first (pre-trim and trim).
    if let Value::String(text) = value {
        if text.is_empty() || py_strip(text).is_empty() {
            if !allow_blank {
                return CharOutcome::Error("This field may not be blank.".to_owned());
            }
            return CharOutcome::Value(String::new());
        }
    }
    // `to_internal_value`: bools and composites fail, numerics coerce.
    let coerced = match value {
        Value::Bool(_) | Value::Array(_) | Value::Object(_) => {
            return CharOutcome::Error("Not a valid string.".to_owned());
        }
        Value::Number(number) => py_num_str(number),
        Value::String(text) => text.clone(),
        Value::Null => unreachable!("null handled above"),
    };
    let trimmed = py_strip(&coerced).to_owned();
    if let Some(max) = max_length {
        if trimmed.chars().count() > max {
            return CharOutcome::Error(format!(
                "Ensure this field has no more than {max} characters."
            ));
        }
    }
    if has_null_char(&trimmed) {
        return CharOutcome::Error("Null characters are not allowed.".to_owned());
    }
    CharOutcome::Value(trimmed)
}

/// `ChoiceField` (`timezone`, `TIMEZONE_CHOICES`): `str()` the input,
/// then membership — no trim, no blank arm.
pub(crate) fn validate_choice_field(input: InputValue<'_>, required: bool) -> CharOutcome {
    let value = match input {
        InputValue::Missing => {
            if required {
                return CharOutcome::Error("This field is required.".to_owned());
            }
            return CharOutcome::Skip;
        }
        // Uploads stringify to their name, then miss the choices.
        InputValue::File(name) => {
            return CharOutcome::Error(format!("\"{name}\" is not a valid choice."));
        }
        InputValue::Value(value) => value,
    };
    if value.is_null() {
        return CharOutcome::Error("This field may not be null.".to_owned());
    }
    let key = py_str(value);
    if PYTZ_COMMON_TIMEZONES.contains(&key.as_str()) {
        CharOutcome::Value(key)
    } else {
        CharOutcome::Error(format!("\"{key}\" is not a valid choice."))
    }
}

/// DRF `DateTimeField` input (`deleted_at`): strings parse ISO-8601
/// (naive attaches the request zone), anything else is `invalid`.
/// Returns the UTC instant, or `None` for JSON/form null.
pub(crate) enum DateTimeOutcome {
    Skip,
    Value(chrono::DateTime<chrono::Utc>),
    Null,
    Error(String),
}

pub(crate) const DATETIME_INVALID_MSG: &str =
    "Datetime has wrong format. Use one of these formats instead: YYYY-MM-DDThh:mm[:ss[.uuuuuu]][+HH:MM|-HH:MM|Z].";

pub(crate) fn validate_datetime_field(
    input: InputValue<'_>,
    required: bool,
    allow_null: bool,
    zone: &Tz,
) -> DateTimeOutcome {
    let value = match input {
        InputValue::Missing => {
            if required {
                return DateTimeOutcome::Error("This field is required.".to_owned());
            }
            return DateTimeOutcome::Skip;
        }
        // Uploads are not strings: the `invalid` arm.
        InputValue::File(_) => return DateTimeOutcome::Error(DATETIME_INVALID_MSG.to_owned()),
        InputValue::Value(value) => value,
    };
    if value.is_null() {
        if allow_null {
            return DateTimeOutcome::Null;
        }
        return DateTimeOutcome::Error("This field may not be null.".to_owned());
    }
    let Value::String(text) = value else {
        return DateTimeOutcome::Error(DATETIME_INVALID_MSG.to_owned());
    };
    match parse_drf_datetime(text) {
        None => DateTimeOutcome::Error(DATETIME_INVALID_MSG.to_owned()),
        Some(ParsedDateTime::Naive(naive)) => match zone.from_local_datetime(&naive) {
            chrono::MappedLocalTime::Single(aware) => {
                DateTimeOutcome::Value(aware.with_timezone(&chrono::Utc))
            }
            chrono::MappedLocalTime::Ambiguous(first, _) => {
                DateTimeOutcome::Value(first.with_timezone(&chrono::Utc))
            }
            chrono::MappedLocalTime::None => {
                DateTimeOutcome::Error(format!("Invalid datetime for the timezone \"{zone}\"."))
            }
        },
        Some(ParsedDateTime::Aware(instant)) => DateTimeOutcome::Value(instant),
    }
}

enum ParsedDateTime {
    Naive(chrono::NaiveDateTime),
    Aware(chrono::DateTime<chrono::Utc>),
}

/// `django.utils.dateparse.parse_datetime` (3.12 `fromisoformat` +
/// `datetime_re` fallback, verified live): strict ISO plus space sep,
/// 1-2 digit fields, any single-char date/time separator, comma/dot
/// fractions (truncated to micros), `Z`/numeric offsets, date-only
/// (midnight), basic `YYYYMMDD`, and `Www` week dates. Anything else —
/// or an impossible calendar — is `None`.
fn parse_drf_datetime(text: &str) -> Option<ParsedDateTime> {
    // Week dates first (`2026-W05-3`, optional time): 2-digit week only.
    if let Some(parsed) = parse_week_datetime(text) {
        return Some(parsed);
    }
    // Split the trailing zone (`Z`, `+HH`, `+HHMM`, `+HH:MM`).
    let (head, offset_secs) = split_datetime_zone(text)?;
    // Cursor-parse the extended date (`YYYY-M-D`, 1-2 digit fields).
    let bytes = head.as_bytes();
    if bytes.len() < 8 || !bytes[0..4].iter().all(|byte| byte.is_ascii_digit()) || bytes[4] != b'-'
    {
        // Basic `YYYYMMDD` (+ optional basic time) instead.
        return parse_basic_datetime(head, offset_secs);
    }
    let mut cursor = 5;
    let month = take_digits(head, &mut cursor, 2)?;
    if head.as_bytes().get(cursor) != Some(&b'-') {
        return None;
    }
    cursor += 1;
    let day = take_digits(head, &mut cursor, 2)?;
    let year: i32 = head[0..4].parse().ok()?;
    let date = chrono::NaiveDate::from_ymd_opt(year, month, day)?;
    if cursor == head.len() {
        // Date-only means midnight.
        return Some(attach_offset(date.and_hms_opt(0, 0, 0)?, offset_secs));
    }
    // Any single char separates date and time (`T`, space, `x`, ...).
    let separator = head[cursor..].chars().next()?;
    let time_part = &head[cursor + separator.len_utf8()..];
    if time_part.is_empty() {
        return None;
    }
    let (hour, minute, second, micros) = parse_hms(time_part)?;
    let naive = date.and_hms_micro_opt(hour, minute, second, micros)?;
    Some(attach_offset(naive, offset_secs))
}

/// Take 1-2 ASCII digits at the cursor (advancing past them).
fn take_digits(text: &str, cursor: &mut usize, max: usize) -> Option<u32> {
    let bytes = text.as_bytes();
    let mut end = *cursor;
    while end < bytes.len() && end - *cursor < max && bytes[end].is_ascii_digit() {
        end += 1;
    }
    if end == *cursor {
        return None;
    }
    let value = text[*cursor..end].parse().ok()?;
    *cursor = end;
    Some(value)
}

/// Basic-format datetimes (`YYYYMMDD[THHMMSS]`, verified live).
fn parse_basic_datetime(head: &str, offset_secs: Option<i32>) -> Option<ParsedDateTime> {
    if head.len() < 8 || !head.bytes().take(8).all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let date = chrono::NaiveDate::from_ymd_opt(
        head[0..4].parse().ok()?,
        head[4..6].parse().ok()?,
        head[6..8].parse().ok()?,
    )?;
    let rest = &head[8..];
    if rest.is_empty() {
        return Some(attach_offset(date.and_hms_opt(0, 0, 0)?, offset_secs));
    }
    // A single separator char, then basic `HHMM[SS]`.
    let separator = rest.chars().next()?;
    let time_part = &rest[separator.len_utf8()..];
    if time_part.is_empty() {
        return None;
    }
    let (hour, minute, second, micros) = parse_hms_basic(time_part)?;
    let naive = date.and_hms_micro_opt(hour, minute, second, micros)?;
    Some(attach_offset(naive, offset_secs))
}

fn parse_hms(text: &str) -> Option<(u32, u32, u32, u32)> {
    let mut parts = text.split(':');
    let hour: u32 = parts.next()?.parse().ok()?;
    let minute: u32 = parts.next()?.parse().ok()?;
    let (second, micros) = match parts.next() {
        None => (0, 0),
        Some(rest) => {
            if let Some(dot) = rest.find(['.', ',']) {
                let seconds: u32 = rest[..dot].parse().ok()?;
                let fraction = &rest[dot + 1..];
                if fraction.is_empty() || !fraction.bytes().all(|byte| byte.is_ascii_digit()) {
                    return None;
                }
                let mut micros_text = fraction.to_owned();
                micros_text.truncate(6);
                while micros_text.len() < 6 {
                    micros_text.push('0');
                }
                (seconds, micros_text.parse().ok()?)
            } else {
                (rest.parse().ok()?, 0)
            }
        }
    };
    if parts.next().is_some() {
        return None;
    }
    Some((hour, minute, second, micros))
}

fn parse_hms_basic(text: &str) -> Option<(u32, u32, u32, u32)> {
    let (head, fraction) = match text.find(['.', ',']) {
        Some(dot) => (&text[..dot], Some(&text[dot + 1..])),
        None => (text, None),
    };
    if head.len() != 4 && head.len() != 6 {
        return None;
    }
    let hour: u32 = head.get(0..2)?.parse().ok()?;
    let minute: u32 = head.get(2..4)?.parse().ok()?;
    let second: u32 = if head.len() == 6 {
        head.get(4..6)?.parse().ok()?
    } else {
        0
    };
    let micros = match fraction {
        None => 0,
        Some(digits) => {
            if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
                return None;
            }
            let mut padded = digits.to_owned();
            padded.truncate(6);
            while padded.len() < 6 {
                padded.push('0');
            }
            padded.parse().ok()?
        }
    };
    Some((hour, minute, second, micros))
}

/// `Www` week dates (`2026-W05-3`, optional time suffix): 2-digit week,
/// Monday-anchored ISO weeks.
fn parse_week_datetime(text: &str) -> Option<ParsedDateTime> {
    let (head, offset_secs) = split_datetime_zone(text)?;
    let week_pos = head.find("-W")?;
    let (year_text, rest) = head.split_at(week_pos);
    let rest = rest.strip_prefix("-W")?;
    if year_text.len() != 4 || !year_text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let year: i32 = year_text.parse().ok()?;
    let (week_text, day_text) = rest.split_once('-')?;
    if week_text.len() != 2 {
        return None;
    }
    let week: u32 = week_text.parse().ok()?;
    // Optional time suffix after the weekday digit.
    let (day_text, time_text) = match day_text.len() {
        1 => (day_text, None),
        len if len > 2 => {
            let day = day_text.get(0..1)?;
            let time = day_text.get(1..)?.strip_prefix(['T', ' '])?;
            (day, Some(time))
        }
        _ => return None,
    };
    let weekday: u32 = day_text.parse().ok()?;
    if !(1..=7).contains(&weekday) {
        return None;
    }
    let weekday = chrono::Weekday::try_from(weekday as u8 - 1).ok()?;
    let date = chrono::NaiveDate::from_isoywd_opt(year, week, weekday)?;
    let naive = match time_text {
        None => date.and_hms_opt(0, 0, 0)?,
        Some(time) => {
            let (hour, minute, second, micros) = parse_hms(time)?;
            date.and_hms_micro_opt(hour, minute, second, micros)?
        }
    };
    Some(attach_offset(naive, offset_secs))
}

/// Split the trailing zone designator: `Z` is UTC (lowercase `z` is
/// rejected — verified live), `±HH[:MM]` / `±HHMM` / `±HH` under 24h
/// are offsets, anything else is naive. Returns `None` when the tail
/// is a malformed offset.
fn split_datetime_zone(text: &str) -> Option<(&str, Option<i32>)> {
    if let Some(head) = text.strip_suffix('Z') {
        return Some((head, Some(0)));
    }
    // A zone sigil can only follow the time (position 13+ guards dates).
    let bytes = text.as_bytes();
    let mut sigil_at: Option<usize> = None;
    for (index, byte) in bytes.iter().enumerate().skip(13) {
        if *byte == b'+' || *byte == b'-' {
            sigil_at = Some(index);
        }
    }
    let Some(at) = sigil_at else {
        return Some((text, None));
    };
    let (head, tail) = text.split_at(at);
    let sign = if tail.starts_with('+') { 1 } else { -1 };
    let digits = tail[1..].replace(':', "");
    if digits.len() != 2 && digits.len() != 4 {
        return None;
    }
    if !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let hours: i32 = digits[0..2].parse().ok()?;
    let minutes: i32 = if digits.len() == 4 {
        digits[2..4].parse().ok()?
    } else {
        0
    };
    if hours > 23 || minutes > 59 {
        return None;
    }
    Some((head, Some(sign * (hours * 3600 + minutes * 60))))
}

fn attach_offset(naive: chrono::NaiveDateTime, offset_secs: Option<i32>) -> ParsedDateTime {
    match offset_secs {
        None => ParsedDateTime::Naive(naive),
        Some(offset) => {
            let instant =
                chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(naive, chrono::Utc)
                    - chrono::TimeDelta::seconds(i64::from(offset));
            ParsedDateTime::Aware(instant)
        }
    }
}

/// `PrimaryKeyRelatedField` (`logo_asset`): `''`/missing/null first,
/// then UUID prep (curly `not a valid UUID`), then the live-row lookup
/// (`does_not_exist`). Ints ride `UUID(int)` into the lookup (too big
/// for 128 bits is `invalid`); every other shape is `invalid` with the
/// Python `str()` of the value.
pub(crate) enum PrimaryKeyOutcome {
    Skip,
    Value(Option<uuid::Uuid>),
    Error(String),
}

pub(crate) fn invalid_uuid_message(rendered: &str) -> String {
    format!("\u{201c}{rendered}\u{201d} is not a valid UUID.")
}

pub(crate) async fn validate_pk_field(
    pool: &sqlx::PgPool,
    input: InputValue<'_>,
    required: bool,
) -> Result<PrimaryKeyOutcome, Denial> {
    let value = match input {
        InputValue::Missing => {
            if required {
                return Ok(PrimaryKeyOutcome::Error(
                    "This field is required.".to_owned(),
                ));
            }
            return Ok(PrimaryKeyOutcome::Skip);
        }
        InputValue::File(name) => {
            return Ok(PrimaryKeyOutcome::Error(invalid_uuid_message(name)));
        }
        InputValue::Value(value) => value,
    };
    if value.is_null() {
        return Ok(PrimaryKeyOutcome::Value(None));
    }
    if let Value::String(text) = value {
        if text.is_empty() {
            return Ok(PrimaryKeyOutcome::Value(None));
        }
    }
    if let Value::Bool(_) = value {
        return Ok(PrimaryKeyOutcome::Error(
            "Incorrect type. Expected pk value, received bool.".to_owned(),
        ));
    }
    // UUID prep: ints ride `UUID(int)`, strings parse as hex, everything
    // else (floats, composites) is `invalid`.
    let candidate: uuid::Uuid = match value {
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                if int < 0 {
                    return Ok(PrimaryKeyOutcome::Error(invalid_uuid_message(&py_str(
                        value,
                    ))));
                }
                uuid::Uuid::from_u128(int as u128)
            } else if let Some(uint) = number.as_u64() {
                uuid::Uuid::from_u128(u128::from(uint))
            } else {
                return Ok(PrimaryKeyOutcome::Error(invalid_uuid_message(&py_str(
                    value,
                ))));
            }
        }
        Value::String(text) => match text.parse::<uuid::Uuid>() {
            Ok(parsed) => parsed,
            Err(_) => return Ok(PrimaryKeyOutcome::Error(invalid_uuid_message(text))),
        },
        _ => {
            return Ok(PrimaryKeyOutcome::Error(invalid_uuid_message(&py_str(
                value,
            ))))
        }
    };
    // Huge ints (past u64) parse as f64, so they take the `invalid` arm
    // above exactly like `UUID(int)` overflow.
    let row: Option<(uuid::Uuid,)> = sqlx::query_as(
        r#"SELECT fa.id FROM file_assets fa WHERE fa.id = $1 AND fa.deleted_at IS NULL"#,
    )
    .bind(candidate)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    match row {
        Some((id,)) => Ok(PrimaryKeyOutcome::Value(Some(id))),
        None => Ok(PrimaryKeyOutcome::Error(format!(
            "Invalid pk \"{}\" - object does not exist.",
            py_pk_display(value, &candidate)
        ))),
    }
}

/// The `pk_value` rendering in `does_not_exist`: ints echo via `py_str`,
/// strings echo the pre-prep input verbatim (DRF fails with `data`, so
/// uppercase/URN spellings are never normalized).
fn py_pk_display(value: &Value, candidate: &uuid::Uuid) -> String {
    match value {
        Value::Number(_) => py_str(value),
        Value::String(text) => text.clone(),
        _ => candidate.to_string(),
    }
}

// ---------------------------------------------------------------------------
// SQL assembly (the services crate owns the text, this module only binds)
// ---------------------------------------------------------------------------

/// Rewrite `:name` placeholders to positional `$n` in first-appearance
/// order, returning the rewritten SQL and the names. `::` casts and
/// single-quoted literals pass through untouched.
pub(crate) fn positional_placeholders(sql: &str) -> (String, Vec<String>) {
    let bytes = sql.as_bytes();
    let mut out = String::with_capacity(sql.len());
    let mut names: Vec<String> = Vec::new();
    let mut index = 0;
    let mut in_string = false;
    while index < bytes.len() {
        let byte = bytes[index];
        if in_string {
            out.push(byte as char);
            if byte == b'\'' {
                if bytes.get(index + 1) == Some(&b'\'') {
                    out.push('\'');
                    index += 1;
                } else {
                    in_string = false;
                }
            }
            index += 1;
            continue;
        }
        if byte == b'\'' {
            in_string = true;
            out.push('\'');
            index += 1;
            continue;
        }
        if byte == b':' {
            if bytes.get(index + 1) == Some(&b':') {
                out.push_str("::");
                index += 2;
                continue;
            }
            let mut end = index + 1;
            while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
                end += 1;
            }
            if end > index + 1 {
                let name = sql[index + 1..end].to_owned();
                let position = match names.iter().position(|known| *known == name) {
                    Some(position) => position + 1,
                    None => {
                        names.push(name);
                        names.len()
                    }
                };
                out.push('$');
                out.push_str(&position.to_string());
                index = end;
                continue;
            }
        }
        out.push(byte as char);
        index += 1;
    }
    (out, names)
}

// ---------------------------------------------------------------------------
// List filters (`filter_queryset`: DjangoFilterBackend, then SearchFilter)
// ---------------------------------------------------------------------------

/// Parsed R1/R2 filters: ANDed LIKE patterns plus an optional owner id.
pub(crate) struct ListFilters {
    pub search_patterns: Vec<String>,
    pub owner_id: Option<uuid::Uuid>,
}

/// Parse `?owner=` (ModelChoice: unknown users 400 with `invalid_choice`,
/// malformed UUIDs 400 with the UUID message, empties ignored, repeats
/// last-win like `QueryDict.get`) and `?search=` (smart-split terms, ANDed).
#[allow(clippy::result_large_err)]
pub(crate) async fn parse_list_filters(
    pool: &sqlx::PgPool,
    query: &QueryMap,
) -> Result<ListFilters, Response> {
    let mut owner_id: Option<uuid::Uuid> = None;
    if let Some(raw) = query_last(query, "owner") {
        if !raw.is_empty() {
            match raw.parse::<uuid::Uuid>() {
                Ok(parsed) => {
                    let exists: Option<(i32,)> =
                        sqlx::query_as("SELECT 1 FROM users WHERE id = $1")
                            .bind(parsed)
                            .fetch_optional(pool)
                            .await
                            .map_err(|_| Denial::ServerError.into_response())?;
                    if exists.is_none() {
                        return Err(single_field_error(
                            "owner",
                            "Select a valid choice. That choice is not one of the available choices."
                                .to_owned(),
                        ));
                    }
                    owner_id = Some(parsed);
                }
                Err(_) => {
                    return Err(single_field_error("owner", invalid_uuid_message(&raw)));
                }
            }
        }
    }
    let mut search_patterns = Vec::new();
    if let Some(raw) = query_last(query, "search") {
        for term in search_terms(&raw) {
            search_patterns.push(format!("%{}%", escape_like(&term)));
        }
    }
    Ok(ListFilters {
        search_patterns,
        owner_id,
    })
}

/// `SearchFilter.get_search_terms`: collapse NULs, commas split, quotes
/// group (`search_smart_split`).
pub(crate) fn search_terms(raw: &str) -> Vec<String> {
    let cleaned: String = raw.chars().filter(|ch| *ch != '\x00').collect();
    let mut terms = Vec::new();
    for bit in django_smart_split(&cleaned) {
        let stripped = bit.trim_matches(',').trim();
        if stripped.is_empty() {
            continue;
        }
        let mut chars = stripped.chars();
        let first = chars.next().expect("non-empty term");
        if (first == '"' || first == '\'') && stripped.ends_with(first) {
            terms.push(unescape_string_literal(stripped));
        } else {
            for piece in stripped.split(',') {
                let piece = piece.trim();
                if !piece.is_empty() {
                    terms.push(piece.to_owned());
                }
            }
        }
    }
    terms
}

/// Django `smart_split`: whitespace splits, quoted phrases (either
/// quote, backslash escapes) stay whole; an unterminated quote degrades
/// to a bare `\S+` run.
pub(crate) fn django_smart_split(text: &str) -> Vec<String> {
    fn is_split_ws(ch: char) -> bool {
        ch.is_whitespace() || matches!(ch, '\u{1c}'..='\u{1f}' | '\u{85}')
    }
    let chars: Vec<char> = text.chars().collect();
    let mut bits = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        while index < chars.len() && is_split_ws(chars[index]) {
            index += 1;
        }
        if index >= chars.len() {
            break;
        }
        let start = index;
        let mut cursor = index;
        let mut saw_quoted = false;
        // First alternative: bare runs punctuated by quoted groups.
        loop {
            while cursor < chars.len()
                && !is_split_ws(chars[cursor])
                && chars[cursor] != '"'
                && chars[cursor] != '\''
            {
                cursor += 1;
            }
            if cursor < chars.len() && (chars[cursor] == '"' || chars[cursor] == '\'') {
                let quote = chars[cursor];
                cursor += 1;
                let mut closed = false;
                while cursor < chars.len() {
                    if chars[cursor] == '\\' {
                        cursor += 2;
                        continue;
                    }
                    if chars[cursor] == quote {
                        cursor += 1;
                        closed = true;
                        break;
                    }
                    cursor += 1;
                }
                if !closed {
                    cursor = start;
                    break;
                }
                saw_quoted = true;
                continue;
            }
            break;
        }
        if saw_quoted {
            bits.push(chars[start..cursor].iter().collect());
        } else {
            // Bare `\S+` run (also the unterminated-quote fallback).
            while cursor < chars.len() && !is_split_ws(chars[cursor]) {
                cursor += 1;
            }
            bits.push(chars[start..cursor].iter().collect());
        }
        index = cursor;
    }
    bits
}

/// Django `unescape_string_literal`: strip the quotes, unescape
/// quote-backslashes, then double backslashes.
pub(crate) fn unescape_string_literal(term: &str) -> String {
    if term.len() < 2 {
        return String::new();
    }
    let quote = term.chars().next().expect("quoted term");
    let inner = &term[quote.len_utf8()..term.len() - quote.len_utf8()];
    let escaped_quote = format!("\\{quote}");
    inner
        .replace(&escaped_quote, &quote.to_string())
        .replace("\\\\", "\\")
}

/// `connection.ops.prep_for_like_query`: backslash first, then `%`/`_`.
pub(crate) fn escape_like(term: &str) -> String {
    let mut out = String::with_capacity(term.len());
    for ch in term.chars() {
        if matches!(ch, '\\' | '%' | '_') {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

// ---------------------------------------------------------------------------
// Workspace rows + statements (R1/R2 over `queries_core`)
// ---------------------------------------------------------------------------

type WorkspaceRowTuple = (
    uuid::Uuid,
    chrono::DateTime<chrono::Utc>,
    chrono::DateTime<chrono::Utc>,
    Option<chrono::DateTime<chrono::Utc>>,
    String,
    Option<String>,
    String,
    Option<String>,
    String,
    String,
    Option<uuid::Uuid>,
    Option<uuid::Uuid>,
    Option<uuid::Uuid>,
    uuid::Uuid,
    Option<i64>,
    Option<i16>,
);

const WORKSPACE_COLUMNS: &str = "workspaces.id, workspaces.created_at, workspaces.updated_at, workspaces.deleted_at, workspaces.name, workspaces.logo, workspaces.slug, workspaces.organization_size, workspaces.timezone, workspaces.background_color, workspaces.created_by_id, workspaces.updated_by_id, workspaces.logo_asset_id, workspaces.owner_id";

/// Assemble the R1 detail statement (member scope, owner join,
/// `total_members` annotation) plus optional search/owner/get filters.
/// Extra search terms AND as indexed copies of the owned fragment.
fn workspace_detail_sql(filters: &ListFilters, by_slug: bool) -> (String, Vec<String>) {
    let mut sql = format!(
        "SELECT {WORKSPACE_COLUMNS}, ({}) AS total_members, NULL::SMALLINT AS role FROM workspaces {} {} WHERE {}",
        queries_core::member_count_sql(),
        queries_core::OWNER_JOIN_SQL,
        queries_core::MEMBERSHIP_JOIN_SQL,
        queries_core::workspace_list_where_sql(
            !filters.search_patterns.is_empty(),
            filters.owner_id.is_some()
        ),
    );
    for (index, _) in filters.search_patterns.iter().enumerate().skip(1) {
        sql.push_str(&format!(
            " AND {}",
            queries_core::search_name_sql().replace(":search", &format!(":search{index}"))
        ));
    }
    if by_slug {
        sql.push_str(" AND workspaces.slug = :slug");
    }
    sql.push_str(" ORDER BY ");
    sql.push_str(queries_core::WORKSPACE_NAME_ORDER_SQL);
    positional_placeholders(&sql)
}

/// Assemble the R2 statement (member scope, role + total_members
/// annotations, distinct, newest first).
fn my_workspaces_sql(filters: &ListFilters) -> (String, Vec<String>) {
    let mut sql = format!(
        "SELECT DISTINCT {WORKSPACE_COLUMNS}, ({}) AS total_members, ({}) AS role FROM workspaces {} WHERE {}",
        queries_core::member_count_sql(),
        queries_core::workspace_role_sql(),
        queries_core::MEMBERSHIP_JOIN_SQL,
        queries_core::user_workspaces_where_sql(
            !filters.search_patterns.is_empty(),
            filters.owner_id.is_some()
        ),
    );
    for (index, _) in filters.search_patterns.iter().enumerate().skip(1) {
        sql.push_str(&format!(
            " AND {}",
            queries_core::search_name_sql().replace(":search", &format!(":search{index}"))
        ));
    }
    sql.push_str(" ORDER BY ");
    sql.push_str(queries_core::WORKSPACE_DEFAULT_ORDER_SQL);
    positional_placeholders(&sql)
}

async fn run_workspace_query(
    pool: &sqlx::PgPool,
    sql: &str,
    names: &[String],
    user_id: &uuid::Uuid,
    slug: Option<&str>,
    filters: &ListFilters,
) -> Result<Vec<WorkspaceRowTuple>, Denial> {
    let mut query = sqlx::query_as::<_, WorkspaceRowTuple>(sql);
    for name in names {
        if name == "user" {
            query = query.bind(user_id);
        } else if name == "slug" {
            query = query.bind(slug.unwrap_or_default());
        } else if name == "owner" {
            query = query.bind(filters.owner_id);
        } else if name == "search" || name.starts_with("search") {
            // First term `:search`, extras `:search1..`.
            let position: usize = if name == "search" {
                0
            } else {
                name["search".len()..]
                    .parse()
                    .map_err(|_| Denial::ServerError)?
            };
            let pattern = filters
                .search_patterns
                .get(position)
                .ok_or(Denial::ServerError)?;
            query = query.bind(pattern);
        } else {
            return Err(Denial::ServerError);
        }
    }
    query.fetch_all(pool).await.map_err(|_| Denial::ServerError)
}

/// One fetched workspace row with its annotations.
pub(crate) struct WorkspaceRowData {
    pub id: uuid::Uuid,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    pub name: String,
    pub logo: Option<String>,
    pub slug: String,
    pub organization_size: Option<String>,
    pub timezone: String,
    pub background_color: String,
    pub created_by_id: Option<uuid::Uuid>,
    pub updated_by_id: Option<uuid::Uuid>,
    pub logo_asset_id: Option<uuid::Uuid>,
    pub owner_id: uuid::Uuid,
    pub total_members: Option<i64>,
    pub role: Option<i64>,
}

fn workspace_row_data(tuple: WorkspaceRowTuple) -> WorkspaceRowData {
    let (
        id,
        created_at,
        updated_at,
        deleted_at,
        name,
        logo,
        slug,
        organization_size,
        timezone,
        background_color,
        created_by_id,
        updated_by_id,
        logo_asset_id,
        owner_id,
        total_members,
        role,
    ) = tuple;
    WorkspaceRowData {
        id,
        created_at,
        updated_at,
        deleted_at,
        name,
        logo,
        slug,
        organization_size,
        timezone,
        background_color,
        created_by_id,
        updated_by_id,
        logo_asset_id,
        owner_id,
        total_members,
        role: role.map(i64::from),
    }
}

/// Render one R1/R2 row: datetimes in the actor zone, `total_members`
/// from the annotation, `role` only when the statement annotates it
/// (R1 omits it — the key stays absent, exactly like DRF's `SkipField`).
async fn render_workspace_row(
    pool: &sqlx::PgPool,
    data: &WorkspaceRowData,
    zone: &Tz,
) -> Result<Value, Denial> {
    let logo_url = resolve_logo_url(pool, data.logo.as_deref(), data.logo_asset_id).await?;
    render_workspace_data(data, logo_url.as_deref(), zone)
}

/// Render one row from pre-resolved parts (shared by reads and the
/// create/update envelopes).
fn render_workspace_data(
    data: &WorkspaceRowData,
    logo_url: Option<&str>,
    zone: &Tz,
) -> Result<Value, Denial> {
    let id = data.id.to_string();
    let created_at = render_datetime_in(&data.created_at, zone);
    let updated_at = render_datetime_in(&data.updated_at, zone);
    let deleted_at = data
        .deleted_at
        .map(|instant| render_datetime_in(&instant, zone));
    let created_by = data.created_by_id.map(|id| id.to_string());
    let updated_by = data.updated_by_id.map(|id| id.to_string());
    let logo_asset = data.logo_asset_id.map(|id| id.to_string());
    let owner = data.owner_id.to_string();
    let row = ser_workspace::WorkspaceRow {
        id: &id,
        total_members: data.total_members.map(Some),
        logo_url,
        role: data.role.map(Some),
        created_at: &created_at,
        updated_at: &updated_at,
        deleted_at: deleted_at.as_deref(),
        name: &data.name,
        logo: data.logo.as_deref(),
        slug: &data.slug,
        organization_size: data.organization_size.as_deref(),
        timezone: &data.timezone,
        background_color: &data.background_color,
        created_by: created_by.as_deref(),
        updated_by: updated_by.as_deref(),
        logo_asset: logo_asset.as_deref(),
        owner: &owner,
    };
    let rendered = ser_workspace::workspace_to_representation(&row);
    serde_json::to_value(&rendered).map_err(|_| Denial::ServerError)
}

/// One `file_assets` scope row for logo resolution.
type AssetScopeRow = (
    Option<String>,
    Option<uuid::Uuid>,
    Option<uuid::Uuid>,
    Option<uuid::Uuid>,
);

/// `logo_url`: the asset URL of `logo_asset_id` wins when set, else
/// the truthy explicit `logo`, else null (forward FK derefs use the
/// unfiltered base manager — soft-deleted assets still resolve).
async fn resolve_logo_url(
    pool: &sqlx::PgPool,
    logo: Option<&str>,
    logo_asset_id: Option<uuid::Uuid>,
) -> Result<Option<String>, Denial> {
    // `logo_url` (`db/models/workspace.py:146-154`): the asset wins when
    // set, else the truthy explicit logo, else `None` (an explicit `""`
    // is falsy, not a URL).
    let Some(asset_id) = logo_asset_id else {
        return match logo {
            Some(text) if !text.is_empty() => Ok(Some(text.to_owned())),
            _ => Ok(None),
        };
    };
    let asset: Option<AssetScopeRow> = sqlx::query_as(
        r#"SELECT fa.entity_type, fa.workspace_id, fa.project_id, fa.issue_id
               FROM file_assets fa WHERE fa.id = $1"#,
    )
    .bind(asset_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let Some((entity_type, workspace_id, project_id, issue_id)) = asset else {
        // A dangling FK raises `DoesNotExist` into the 500 arm.
        return Err(Denial::ServerError);
    };
    let entity_type = entity_type.unwrap_or_default();
    if matches!(
        entity_type.as_str(),
        "WORKSPACE_LOGO" | "USER_AVATAR" | "USER_COVER" | "PROJECT_COVER"
    ) {
        return Ok(Some(format!("/api/assets/v2/static/{asset_id}/")));
    }
    if entity_type == "ISSUE_ATTACHMENT" {
        let slug = asset_workspace_slug(pool, workspace_id).await?;
        return Ok(Some(format!(
            "/api/assets/v2/workspaces/{slug}/projects/{}/issues/{}/attachments/{asset_id}/",
            project_id
                .map(|id| id.to_string())
                .unwrap_or_else(|| "None".to_owned()),
            issue_id
                .map(|id| id.to_string())
                .unwrap_or_else(|| "None".to_owned()),
        )));
    }
    if matches!(
        entity_type.as_str(),
        "ISSUE_DESCRIPTION"
            | "COMMENT_DESCRIPTION"
            | "PAGE_DESCRIPTION"
            | "DRAFT_ISSUE_DESCRIPTION"
    ) {
        let slug = asset_workspace_slug(pool, workspace_id).await?;
        return Ok(Some(format!(
            "/api/assets/v2/workspaces/{slug}/projects/{}/{asset_id}/",
            project_id
                .map(|id| id.to_string())
                .unwrap_or_else(|| "None".to_owned()),
        )));
    }
    Ok(None)
}

/// The workspace slug behind an asset link (`workspace.slug` on a null
/// workspace is the `AttributeError` 500 arm; the lookup itself is
/// unfiltered like every forward FK deref).
async fn asset_workspace_slug(
    pool: &sqlx::PgPool,
    workspace_id: Option<uuid::Uuid>,
) -> Result<String, Denial> {
    let Some(workspace_id) = workspace_id else {
        return Err(Denial::ServerError);
    };
    let row: Option<(String,)> = sqlx::query_as("SELECT w.slug FROM workspaces w WHERE w.id = $1")
        .bind(workspace_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    row.map(|(slug,)| slug).ok_or(Denial::ServerError)
}

// ---------------------------------------------------------------------------
// Workspace write validation (`WorkSpaceSerializer`, field order)
// ---------------------------------------------------------------------------

/// Validated workspace input: `None` fields were absent (create applies
/// model defaults; update leaves the row alone).
#[derive(Debug)]
pub(crate) struct ValidatedWorkspace {
    pub deleted_at: Option<Option<chrono::DateTime<chrono::Utc>>>,
    pub name: Option<String>,
    pub logo: Option<Option<String>>,
    pub slug: Option<String>,
    pub organization_size: Option<Option<String>>,
    pub timezone: Option<String>,
    pub background_color: Option<String>,
    pub logo_asset_id: Option<Option<uuid::Uuid>>,
}

/// Validate one workspace write in serializer field order
/// (`deleted_at`, `name`, `logo`, `slug`, `organization_size`,
/// `timezone`, `background_color`, `logo_asset`), collecting every
/// field's first error. `exclude_id` scopes the slug `UniqueValidator`
/// (update excludes itself). HTML `''` on `deleted_at`/`logo_asset`
/// means null (`Field.get_value`); the route spec already dropped the
/// skip-blank keys.
pub(crate) async fn validate_workspace_input(
    pool: &sqlx::PgPool,
    body: &InputBody,
    zone: &Tz,
    partial: bool,
    exclude_id: Option<&uuid::Uuid>,
) -> Result<Result<ValidatedWorkspace, FieldErrors>, Denial> {
    let mut errors: FieldErrors = Vec::new();
    let required = !partial;

    let mut deleted_at = match validate_datetime_field(body.get("deleted_at"), false, true, zone) {
        DateTimeOutcome::Skip => None,
        DateTimeOutcome::Value(instant) => Some(Some(instant)),
        DateTimeOutcome::Null => Some(None),
        DateTimeOutcome::Error(message) => {
            errors.push(("deleted_at".to_owned(), vec![message]));
            None
        }
    };
    // HTML `''` on `allow_null` fields reads as null, not `invalid`.
    if body.is_html {
        if let InputValue::Value(Value::String(text)) = body.get("deleted_at") {
            if text.is_empty() {
                deleted_at = Some(None);
                errors.retain(|(field, _)| field != "deleted_at");
            }
        }
    }

    let name = match validate_char_field(body.get("name"), required, false, false, Some(80)) {
        CharOutcome::Skip => None,
        CharOutcome::Value(text) => match ser_workspace::validate_name(&text) {
            Ok(_) => Some(text),
            Err(error) => {
                errors.push(("name".to_owned(), vec![error.to_string()]));
                None
            }
        },
        CharOutcome::Null => {
            errors.push((
                "name".to_owned(),
                vec!["This field may not be null.".to_owned()],
            ));
            None
        }
        CharOutcome::Error(message) => {
            errors.push(("name".to_owned(), vec![message]));
            None
        }
    };

    let logo = match validate_char_field(body.get("logo"), false, true, true, None) {
        CharOutcome::Skip => None,
        CharOutcome::Value(text) => Some(Some(text)),
        CharOutcome::Null => Some(None),
        CharOutcome::Error(message) => {
            errors.push(("logo".to_owned(), vec![message]));
            None
        }
    };

    let slug = match validate_slug_field(pool, body, required, exclude_id).await? {
        Ok(slug) => slug,
        Err(message) => {
            errors.push(("slug".to_owned(), vec![message]));
            None
        }
    };

    let organization_size =
        match validate_char_field(body.get("organization_size"), false, true, true, Some(20)) {
            CharOutcome::Skip => None,
            CharOutcome::Value(text) => Some(Some(text)),
            CharOutcome::Null => Some(None),
            CharOutcome::Error(message) => {
                errors.push(("organization_size".to_owned(), vec![message]));
                None
            }
        };

    let timezone = match validate_choice_field(body.get("timezone"), false) {
        CharOutcome::Skip => None,
        CharOutcome::Value(text) => Some(text),
        // `ChoiceField` has no null arm of its own: nulls and misses
        // both surface as errors from the single call above.
        CharOutcome::Null => {
            errors.push((
                "timezone".to_owned(),
                vec!["This field may not be null.".to_owned()],
            ));
            None
        }
        CharOutcome::Error(message) => {
            errors.push(("timezone".to_owned(), vec![message]));
            None
        }
    };

    let background_color =
        match validate_char_field(body.get("background_color"), false, false, false, Some(255)) {
            CharOutcome::Skip => None,
            CharOutcome::Value(text) => Some(text),
            CharOutcome::Null => {
                errors.push((
                    "background_color".to_owned(),
                    vec!["This field may not be null.".to_owned()],
                ));
                None
            }
            CharOutcome::Error(message) => {
                errors.push(("background_color".to_owned(), vec![message]));
                None
            }
        };

    let logo_asset_id = match validate_pk_field(pool, body.get("logo_asset"), false).await? {
        PrimaryKeyOutcome::Skip => None,
        PrimaryKeyOutcome::Value(id) => Some(id),
        PrimaryKeyOutcome::Error(message) => {
            errors.push(("logo_asset".to_owned(), vec![message]));
            None
        }
    };

    if !errors.is_empty() {
        return Ok(Err(errors));
    }
    Ok(Ok(ValidatedWorkspace {
        deleted_at,
        name,
        logo,
        slug,
        organization_size,
        timezone,
        background_color,
        logo_asset_id,
    }))
}

/// The slug pipeline in validator order: required/null/type/trim/blank,
/// then the restricted check, then the live-row `UniqueValidator`
/// (self-excluded on update), then max-length, null-char, and DRF's own
/// slug regex. The custom charset message is unreachable (DRF's regex
/// covers the same charset first — verified live), so the restricted
/// arm reuses the SER validator and the charset arm renders DRF's text.
async fn validate_slug_field(
    pool: &sqlx::PgPool,
    body: &InputBody,
    required: bool,
    exclude_id: Option<&uuid::Uuid>,
) -> Result<Result<Option<String>, String>, Denial> {
    let trimmed = match validate_char_field(body.get("slug"), required, false, false, None) {
        CharOutcome::Skip => return Ok(Ok(None)),
        CharOutcome::Value(text) => text,
        CharOutcome::Null => return Ok(Err("This field may not be null.".to_owned())),
        CharOutcome::Error(message) => return Ok(Err(message)),
    };
    if RESTRICTED_WORKSPACE_SLUGS.contains(&trimmed.as_str()) {
        return Ok(Err(ser_workspace::SlugError::Restricted.to_string()));
    }
    let conflict: Option<(uuid::Uuid,)> = if let Some(exclude) = exclude_id {
        sqlx::query_as("SELECT w.id FROM workspaces w WHERE w.slug = $1 AND w.deleted_at IS NULL AND w.id <> $2")
            .bind(&trimmed)
            .bind(exclude)
            .fetch_optional(pool)
            .await
            .map_err(|_| Denial::ServerError)?
    } else {
        sqlx::query_as("SELECT w.id FROM workspaces w WHERE w.slug = $1 AND w.deleted_at IS NULL")
            .bind(&trimmed)
            .fetch_optional(pool)
            .await
            .map_err(|_| Denial::ServerError)?
    };
    if conflict.is_some() {
        return Ok(Err("Workspace with this slug already exists.".to_owned()));
    }
    if trimmed.chars().count() > 48 {
        return Ok(Err(
            "Ensure this field has no more than 48 characters.".to_owned()
        ));
    }
    if has_null_char(&trimmed) {
        return Ok(Err("Null characters are not allowed.".to_owned()));
    }
    if !is_drf_slug(&trimmed) {
        return Ok(Err(
            "Enter a valid \"slug\" consisting of letters, numbers, underscores or hyphens."
                .to_owned(),
        ));
    }
    Ok(Ok(Some(trimmed)))
}

/// DRF `SlugField` regex `^[-a-zA-Z0-9_]+$`: the input is already
/// trimmed, so the `$`-before-newline quirk cannot trigger.
fn is_drf_slug(text: &str) -> bool {
    !text.is_empty()
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

/// `get_random_color` (`db/models/workspace.py:14-16`): `#` + six draws
/// from `string.hexdigits` (`0-9a-fA-F`, mixed case).
pub(crate) fn random_background_color() -> String {
    use rand::seq::IndexedRandom;
    const HEXDIGITS: &[u8] = b"0123456789abcdefABCDEF";
    let mut rng = rand::rng();
    let mut color = String::with_capacity(7);
    color.push('#');
    for _ in 0..6 {
        color.push(*HEXDIGITS.choose(&mut rng).expect("hexdigits") as char);
    }
    color
}

// ---------------------------------------------------------------------------
// Theme write validation (`WorkspaceThemeSerializer`, field order)
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub(crate) struct ValidatedTheme {
    pub deleted_at: Option<Option<chrono::DateTime<chrono::Utc>>>,
    pub name: Option<String>,
    pub colors: Option<Value>,
}

/// Validate one theme write (`deleted_at`, `name`, `colors`):
/// `deleted_at` is forced required on create (DRF uniqueness forcing),
/// optional on partial; `colors` accepts any non-null JSON (raw), and
/// parses `JSONString` input on HTML forms.
pub(crate) fn validate_theme_input(
    body: &InputBody,
    zone: &Tz,
    partial: bool,
) -> Result<Result<ValidatedTheme, FieldErrors>, Denial> {
    let mut errors: FieldErrors = Vec::new();
    let required = !partial;

    let mut deleted_at = match validate_datetime_field(body.get("deleted_at"), required, true, zone)
    {
        DateTimeOutcome::Skip => None,
        DateTimeOutcome::Value(instant) => Some(Some(instant)),
        DateTimeOutcome::Null => Some(None),
        DateTimeOutcome::Error(message) => {
            errors.push(("deleted_at".to_owned(), vec![message]));
            None
        }
    };
    // HTML `''` on `allow_null` fields reads as null — which also
    // satisfies the forced required on create (verified live).
    if body.is_html {
        if let InputValue::Value(Value::String(text)) = body.get("deleted_at") {
            if text.is_empty() {
                deleted_at = Some(None);
                errors.retain(|(field, _)| field != "deleted_at");
            }
        }
    }

    let name = match validate_char_field(body.get("name"), required, false, false, Some(300)) {
        CharOutcome::Skip => None,
        CharOutcome::Value(text) => Some(text),
        CharOutcome::Null => {
            errors.push((
                "name".to_owned(),
                vec!["This field may not be null.".to_owned()],
            ));
            None
        }
        CharOutcome::Error(message) => {
            errors.push(("name".to_owned(), vec![message]));
            None
        }
    };

    let colors = match validate_colors_field(body) {
        Ok(colors) => colors,
        Err(ColorsFailure::Message(message)) => {
            errors.push(("colors".to_owned(), vec![message]));
            None
        }
        // `jsonb` cannot store infinities: Django's `DataError` escapes
        // into the 500 arm.
        Err(ColorsFailure::NonFinite) => return Err(Denial::ServerError),
    };

    if !errors.is_empty() {
        return Ok(Err(errors));
    }
    Ok(Ok(ValidatedTheme {
        deleted_at,
        name,
        colors,
    }))
}

enum ColorsFailure {
    Message(String),
    NonFinite,
}

/// `colors` (`JSONField`, `default=dict`): JSON bodies pass any non-null
/// value through untouched; HTML forms parse the text as JSON
/// (`JSONString`); uploads fail the `dumps` check.
fn validate_colors_field(body: &InputBody) -> Result<Option<Value>, ColorsFailure> {
    match body.get("colors") {
        InputValue::Missing => Ok(None),
        InputValue::File(_) => Err(ColorsFailure::Message(
            "Value must be valid JSON.".to_owned(),
        )),
        InputValue::Value(value) => {
            if value.is_null() {
                return Err(ColorsFailure::Message(
                    "This field may not be null.".to_owned(),
                ));
            }
            if !body.is_html {
                if !json_floats_finite(value) {
                    return Err(ColorsFailure::NonFinite);
                }
                return Ok(Some(value.clone()));
            }
            let Value::String(text) = value else {
                return Err(ColorsFailure::Message(
                    "Value must be valid JSON.".to_owned(),
                ));
            };
            match serde_json::from_str::<Value>(text) {
                Ok(parsed) => {
                    if !json_floats_finite(&parsed) {
                        return Err(ColorsFailure::NonFinite);
                    }
                    Ok(Some(parsed))
                }
                Err(_) => Err(ColorsFailure::Message(
                    "Value must be valid JSON.".to_owned(),
                )),
            }
        }
    }
}

/// Reject non-finite floats anywhere in a JSON value (unrepresentable
/// in `jsonb`; Django 500s on the `DataError`).
fn json_floats_finite(value: &Value) -> bool {
    match value {
        Value::Number(number) => number
            .as_f64()
            .map(|float| float.is_finite())
            .unwrap_or(true),
        Value::Array(items) => items.iter().all(json_floats_finite),
        Value::Object(map) => map.values().all(json_floats_finite),
        _ => true,
    }
}

// ---------------------------------------------------------------------------
// Theme rows + statements (T1/T2 over `queries_core`)
// ---------------------------------------------------------------------------

type ThemeRowTuple = (
    uuid::Uuid,
    chrono::DateTime<chrono::Utc>,
    chrono::DateTime<chrono::Utc>,
    Option<chrono::DateTime<chrono::Utc>>,
    String,
    Value,
    Option<uuid::Uuid>,
    Option<uuid::Uuid>,
    uuid::Uuid,
    uuid::Uuid,
);

const THEME_COLUMNS: &str = "workspace_themes.id, workspace_themes.created_at, workspace_themes.updated_at, workspace_themes.deleted_at, workspace_themes.name, workspace_themes.colors, workspace_themes.created_by_id, workspace_themes.updated_by_id, workspace_themes.workspace_id, workspace_themes.actor_id";

fn theme_list_sql() -> (String, Vec<String>) {
    let sql = format!(
        "SELECT {THEME_COLUMNS} FROM workspace_themes {} WHERE {} ORDER BY {}",
        queries_core::THEME_JOIN_SQL,
        queries_core::theme_where_sql(),
        queries_core::THEME_DEFAULT_ORDER_SQL,
    );
    positional_placeholders(&sql)
}

fn theme_detail_sql() -> (String, Vec<String>) {
    let sql = format!(
        "SELECT {THEME_COLUMNS} FROM workspace_themes {} WHERE {} AND workspace_themes.id = :theme_id ORDER BY {}",
        queries_core::THEME_JOIN_SQL,
        queries_core::theme_where_sql(),
        queries_core::THEME_DEFAULT_ORDER_SQL,
    );
    positional_placeholders(&sql)
}

async fn run_theme_query(
    pool: &sqlx::PgPool,
    sql: &str,
    names: &[String],
    slug: &str,
    theme_id: Option<&uuid::Uuid>,
) -> Result<Vec<ThemeRowTuple>, Denial> {
    let mut query = sqlx::query_as::<_, ThemeRowTuple>(sql);
    for name in names {
        if name == "slug" {
            query = query.bind(slug);
        } else if name == "theme_id" {
            query = query.bind(theme_id);
        } else {
            return Err(Denial::ServerError);
        }
    }
    query.fetch_all(pool).await.map_err(|_| Denial::ServerError)
}

fn render_theme_row(tuple: &ThemeRowTuple, zone: &Tz) -> Result<Value, Denial> {
    let (
        id,
        created_at,
        updated_at,
        deleted_at,
        name,
        colors,
        created_by_id,
        updated_by_id,
        workspace_id,
        actor_id,
    ) = tuple;
    let id_text = id.to_string();
    let created_at_text = render_datetime_in(created_at, zone);
    let updated_at_text = render_datetime_in(updated_at, zone);
    let deleted_at_text = deleted_at.map(|instant| render_datetime_in(&instant, zone));
    let created_by_text = created_by_id.map(|id| id.to_string());
    let updated_by_text = updated_by_id.map(|id| id.to_string());
    let workspace_text = workspace_id.to_string();
    let actor_text = actor_id.to_string();
    let row = ser_invite::ThemeRow {
        id: &id_text,
        created_at: &created_at_text,
        updated_at: &updated_at_text,
        deleted_at: deleted_at_text.as_deref(),
        name,
        colors,
        created_by: created_by_text.as_deref(),
        updated_by: updated_by_text.as_deref(),
        workspace: &workspace_text,
        actor: &actor_text,
    };
    let rendered = ser_invite::theme_to_representation(&row);
    serde_json::to_value(&rendered).map_err(|_| Denial::ServerError)
}

// ---------------------------------------------------------------------------
// Export rows + CSV (R5 over `queries_core`)
// ---------------------------------------------------------------------------

type ExportRowTuple = (
    String,
    String,
    String,
    Option<i32>,
    chrono::DateTime<chrono::Utc>,
    chrono::DateTime<chrono::Utc>,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
);

const EXPORT_COLUMNS: &str = "users.display_name, projects.identifier, projects.name, issues.sequence_id, issue_activities.created_at, issue_activities.updated_at, issue_activities.verb, issue_activities.field, issue_activities.old_value, issue_activities.new_value";

fn export_sql() -> (String, Vec<String>) {
    let sql = format!(
        "SELECT {EXPORT_COLUMNS} FROM issue_activities {} WHERE {} ORDER BY {} LIMIT {}",
        queries_core::EXPORT_JOINS_SQL,
        queries_core::export_where_sql(),
        queries_core::ACTIVITY_DEFAULT_ORDER_SQL,
        queries_core::EXPORT_LIMIT,
    );
    positional_placeholders(&sql)
}

/// `parse_date` for the export `date` (every shape below probed
/// through `DateField.to_python`, Django 4.2.30): dashed
/// `YYYY-M-D` with a 4-digit year 1-9999 and 1-2 digit month/day, or
/// basic `YYYYMMDD` with year >= 1; anything else is the
/// `ValidationError` 400. Non-string truthy input is the `TypeError`
/// 500 arm (handled by the caller via `py_str` gating). Accepted gap:
/// a trailing `\n` is valid in Django (the regex `$` matches before
/// it) but rejected here — pathological, not chased.
pub(crate) fn parse_export_date(text: &str) -> Option<chrono::NaiveDate> {
    if text.len() == 8 && text.bytes().all(|byte| byte.is_ascii_digit()) {
        let year: i32 = text[0..4].parse().ok()?;
        if year < 1 {
            return None;
        }
        return chrono::NaiveDate::from_ymd_opt(
            year,
            text[4..6].parse().ok()?,
            text[6..8].parse().ok()?,
        );
    }
    let mut parts = text.split('-');
    let (year_seg, month_seg, day_seg) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() {
        return None;
    }
    // Rust's int parser accepts a leading `+`, which Django's `\d`
    // never matches, so the segments must be pure ASCII digits first.
    if year_seg.len() != 4
        || !(1..=2).contains(&month_seg.len())
        || !(1..=2).contains(&day_seg.len())
    {
        return None;
    }
    if !year_seg.bytes().all(|byte| byte.is_ascii_digit())
        || !month_seg.bytes().all(|byte| byte.is_ascii_digit())
        || !day_seg.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    let year: i32 = year_seg.parse().ok()?;
    let month: u32 = month_seg.parse().ok()?;
    let day: u32 = day_seg.parse().ok()?;
    if !(1..=9999).contains(&year) {
        return None;
    }
    chrono::NaiveDate::from_ymd_opt(year, month, day)
}

/// Render the export CSV: `QUOTE_ALL`, sanitized cells, 9-column header,
/// `\r\n` line ends.
pub(crate) fn render_export_csv(rows: &[ExportRowTuple], zone: &Tz) -> String {
    let mut out = String::new();
    let header = [
        "Actor name",
        "Issue ID",
        "Project",
        "Created at",
        "Updated at",
        "Action",
        "Field",
        "Old value",
        "New value",
    ];
    write_csv_row(&mut out, &header.map(Some));
    for row in rows {
        let (
            display_name,
            identifier,
            project_name,
            sequence_id,
            created_at,
            updated_at,
            verb,
            field,
            old_value,
            new_value,
        ) = row;
        let issue = format!(
            "{identifier} - {}",
            sequence_id
                .map(|sequence| sequence.to_string())
                .unwrap_or_default()
        );
        let cells: [Option<String>; 9] = [
            Some(display_name.clone()),
            Some(issue),
            Some(project_name.clone()),
            Some(format_csv_datetime(created_at, zone)),
            Some(format_csv_datetime(updated_at, zone)),
            Some(verb.clone()),
            field.clone(),
            old_value.clone(),
            new_value.clone(),
        ];
        let borrowed: Vec<Option<&str>> = cells.iter().map(|cell| cell.as_deref()).collect();
        write_csv_row_ref(&mut out, &borrowed);
    }
    out
}

/// One CSV row, `QUOTE_ALL`: every cell quoted, `"` doubled, `None`
/// renders empty, `\r\n` terminated. Sanitizing runs first and only
/// touches strings (datetimes stringify after, like the writer's
/// `str()`).
fn write_csv_row(out: &mut String, cells: &[Option<&str>; 9]) {
    let rendered: Vec<String> = cells
        .iter()
        .map(|cell| match cell {
            None => "\"\"".to_owned(),
            Some(text) => format!("\"{}\"", sanitize_csv_value(text).replace('"', "\"\"")),
        })
        .collect();
    out.push_str(&rendered.join(","));
    out.push_str("\r\n");
}

fn write_csv_row_ref(out: &mut String, cells: &[Option<&str>]) {
    let rendered: Vec<String> = cells
        .iter()
        .map(|cell| match cell {
            None => "\"\"".to_owned(),
            Some(text) => format!("\"{}\"", sanitize_csv_value(text).replace('"', "\"\"")),
        })
        .collect();
    out.push_str(&rendered.join(","));
    out.push_str("\r\n");
}

/// `sanitize_csv_value` (`utils/csv_utils.py:31-38`): prefix `'` when a
/// leading `=`, `+`, `-`, `@`, tab, CR, or LF would formula-inject.
pub(crate) fn sanitize_csv_value(value: &str) -> String {
    const TRIGGERS: &[char] = &['=', '+', '-', '@', '\t', '\r', '\n'];
    match value.chars().next() {
        Some(first) if TRIGGERS.contains(&first) => format!("'{value}"),
        _ => value.to_owned(),
    }
}

/// Python `str()` over the UTC-aware export instant: `csv.writer`
/// stringifies the datetime object as the ORM returned it (UTC, so the
/// offset is always `+00:00`) — `TimezoneMixin.activate` never affects
/// `str()`. Micros iff nonzero. The zone rides along for the call
/// shape; only the `AT TIME ZONE` *filter* is zoned.
pub(crate) fn format_csv_datetime(instant: &chrono::DateTime<chrono::Utc>, _zone: &Tz) -> String {
    use chrono::{Datelike, Timelike};
    let mut text = format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        instant.year(),
        instant.month(),
        instant.day(),
        instant.hour(),
        instant.minute(),
        instant.second()
    );
    let micros = instant.nanosecond() / 1000;
    if micros != 0 {
        text.push_str(&format!(".{micros:06}"));
    }
    text.push_str("+00:00");
    text
}

// ---------------------------------------------------------------------------
// Integrity mapping (`handle_exception`, create's own `except`)
// ---------------------------------------------------------------------------

/// Whether a sqlx failure is a Postgres integrity error (the
/// `IntegrityError` umbrella: unique 23505, FK 23503, check 23514,
/// not-null 23502).
fn is_integrity_error(error: &sqlx::Error) -> bool {
    matches!(
        error
            .as_database_error()
            .and_then(|db| db.code())
            .as_deref(),
        Some("23505" | "23503" | "23514" | "23502")
    )
}

fn is_unique_violation(error: &sqlx::Error) -> bool {
    matches!(
        error
            .as_database_error()
            .and_then(|db| db.code())
            .as_deref(),
        Some("23505")
    )
}

/// Stock `get_object` miss: `get_object_or_404` raises `Http404("No <Model>
/// matches the given query.")`, rewrapped by DRF as `NotFound` — 404
/// `{"Detail":"No <Model> matches the given query."}` (DRF `NotFound`).
fn detail_not_found_response(model: &str) -> Response {
    json_response(
        StatusCode::NOT_FOUND,
        format!("{{\"Detail\":\"No {model} matches the given query.\"}}"),
    )
}
/// One `soft_delete_related_objects.delay(app_label, model_name, pk,
/// using=None)` enqueue (`db/mixins.py:78`): positional triple plus the
/// explicit null `using` kwarg, exactly as `.delay()` serializes it.
async fn enqueue_soft_delete_related(
    pool: &sqlx::PgPool,
    app_label: &str,
    model_name: &str,
    instance_pk: &uuid::Uuid,
) {
    let mut kwargs = Map::with_capacity(1);
    kwargs.insert("using".to_owned(), Value::Null);
    enqueue_task(
        pool,
        pidash_jobs::tasks_cleanup::deletion::SOFT_DELETE_TASK,
        vec![
            Value::String(app_label.to_owned()),
            Value::String(model_name.to_owned()),
            Value::String(instance_pk.to_string()),
        ],
        kwargs,
    )
    .await;
}

// ---------------------------------------------------------------------------
// Handlers: `WorkSpaceViewSet`
// ---------------------------------------------------------------------------

/// GET `workspaces/` — the pinned LIST bug (`:168-170`): the
/// collection route carries `Allow(WORKSPACE)`, whose `kwargs["slug"]`
/// lookup raises `KeyError` for every authenticated caller.
pub async fn list_workspaces(
    State(_state): State<AppState>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> HandlerResult {
    let _user_id = actor_user_id(extension)?;
    debug_assert!(matches!(*gate_for_list(), gates::Gate::CollectionList));
    let scope = gates::tenant_context("");
    let facts = allow_facts_for("", None, &[]);
    match apply_outcome(gates::decide_gate(gate_for_list(), &scope, &facts)) {
        Ok(()) => Err(Denial::ServerError),
        Err(response) => Ok(response),
    }
}

/// Form blank specs (`Field.get_value`): keys where a present-but-empty
/// form value behaves as absent.
const WORKSPACE_SKIP_BLANK: &[&str] = &["timezone", "background_color"];
const THEME_SKIP_BLANK: &[&str] = &["colors"];
const NO_SKIP_BLANK: &[&str] = &[];

/// `RETURNING` stamps of the workspace/theme `INSERT`s.
type InsertedStamps = Option<(chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>)>;

/// POST `workspaces/` (`:83-166`).
pub async fn create_workspace(
    State(state): State<AppState>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    headers: HeaderMap,
    body: bytes::Bytes,
) -> HandlerResult {
    let user_id = actor_user_id(extension)?;
    let pool = pool_of(&state)?;
    debug_assert!(matches!(*gate_for_create(), gates::Gate::ClassBase));
    if let Err(response) = check_class_base(&pool, None, &user_id, "POST").await {
        return Ok(response);
    }
    // `DISABLE_WORKSPACE_CREATION` gates before `request.data` is touched
    // (`base.py:85-100`): disabled+malformed still 403s.
    if workspace_creation_disabled(&state, &pool).await? {
        return Ok(json_response(
            StatusCode::FORBIDDEN,
            "{\"error\":\"Workspace creation is not allowed\"}".to_owned(),
        ));
    }
    let input = match negotiate_body(&headers, &body, WORKSPACE_SKIP_BLANK) {
        Ok(input) => input,
        Err(response) => return Ok(response),
    };
    // `request.data.get("name", False)` on a non-object body is the
    // `AttributeError` 500 arm (the manual reads run before validation).
    if !input.json_is_object() && !input.is_html {
        return Err(Denial::ServerError);
    }
    let name_value = match input.get("name") {
        InputValue::Missing => None,
        InputValue::File(_) => return Err(Denial::ServerError),
        InputValue::Value(value) => Some(value),
    };
    let slug_value = match input.get("slug") {
        InputValue::Missing => None,
        InputValue::File(_) => return Err(Denial::ServerError),
        InputValue::Value(value) => Some(value),
    };
    if !name_value.map(py_truthy).unwrap_or(false) || !slug_value.map(py_truthy).unwrap_or(false) {
        return Ok(json_response(
            StatusCode::BAD_REQUEST,
            "{\"error\":\"Both name and slug are required\"}".to_owned(),
        ));
    }
    let (name_value, slug_value) = (
        name_value.expect("truthy name"),
        slug_value.expect("truthy slug"),
    );
    let (Some(name_len), Some(slug_len)) = (py_len(name_value), py_len(slug_value)) else {
        // `len()` on a number/bool is the `TypeError` 500 arm.
        return Err(Denial::ServerError);
    };
    if name_len > 80 || slug_len > 48 {
        return Ok(json_response(
            StatusCode::BAD_REQUEST,
            "{\"error\":\"The maximum length for name is 80 and for slug is 48\"}".to_owned(),
        ));
    }
    let Value::String(name_text) = name_value else {
        // `contains_url` calls `.split` — composites are the 500 arm.
        return Err(Denial::ServerError);
    };
    if ser_workspace::contains_url(name_text) {
        return Ok(json_response(
            StatusCode::BAD_REQUEST,
            "{\"error\":\"Name cannot contain a URL\"}".to_owned(),
        ));
    }
    let zone = actor_timezone(&pool, &user_id).await?;
    let validated = match validate_workspace_input(&pool, &input, &zone, false, None).await? {
        Ok(validated) => validated,
        Err(errors) => return Ok(field_errors_response(errors)),
    };
    // The manual gates above guarantee both; the serializer agrees.
    let (Some(name), Some(slug)) = (validated.name, validated.slug) else {
        return Err(Denial::ServerError);
    };
    let background_color = validated
        .background_color
        .unwrap_or_else(random_background_color);
    let timezone = validated.timezone.unwrap_or_else(|| "UTC".to_owned());
    let logo = validated.logo.flatten();
    let organization_size = validated.organization_size.flatten();
    let workspace_id = uuid::Uuid::new_v4();
    let now = chrono::Utc::now();
    let insert: Result<InsertedStamps, sqlx::Error> =
        sqlx::query_as(
            r#"INSERT INTO workspaces
               (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, name, logo, logo_asset_id, owner_id, slug, organization_size, timezone, background_color)
               VALUES ($1, $2, $2, $3, NULL, $4, $5, $6, $7, $3, $8, $9, $10, $11)
               RETURNING created_at, updated_at"#,
        )
        .bind(workspace_id)
        .bind(now)
        .bind(user_id)
        .bind(validated.deleted_at.flatten())
        .bind(&name)
        .bind(logo.as_deref())
        .bind(validated.logo_asset_id.flatten())
        .bind(&slug)
        .bind(organization_size.as_deref())
        .bind(&timezone)
        .bind(&background_color)
        .fetch_optional(&pool)
        .await;
    let (created_at, updated_at) = match insert {
        Ok(Some(timestamps)) => timestamps,
        Ok(None) => return Err(Denial::ServerError),
        Err(error) if is_unique_violation(&error) => {
            return Ok(json_response(
                StatusCode::CONFLICT,
                "{\"slug\":\"The workspace with the slug already exists\"}".to_owned(),
            ));
        }
        Err(_) => return Err(Denial::ServerError),
    };
    // `WorkspaceMember.objects.create(...)` (`:126-132`): the `company_role`
    // echo (`:127`) renders through Python `str()` (null stays null).
    let company_role = match input.get("company_role") {
        InputValue::Missing => Some(String::new()),
        InputValue::File(_) => return Err(Denial::ServerError),
        InputValue::Value(Value::Null) => None,
        InputValue::Value(value) => Some(py_str(value)),
    };
    let member_insert: Result<_, sqlx::Error> = sqlx::query(
        r#"INSERT INTO workspace_members
           (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, workspace_id, member_id, role, company_role, view_props, default_props, issue_props, getting_started_checklist, tips, explored_features, is_active)
           VALUES ($1, $2, $2, $3, NULL, NULL, $4, $3, 20, $5, $6, $7, $8, $9, $10, $11, TRUE)"#,
    )
    .bind(uuid::Uuid::new_v4())
    .bind(now)
    .bind(user_id)
    .bind(workspace_id)
    .bind(company_role)
    .bind(member_default_json(
        models_workspace::workspace_member::DEFAULT_VIEW_PROPS_JSON,
    )?)
    // `default_props` shares `get_default_props` with `view_props`
    // (`workspace.py:207-208`), so the one ported default serves both.
    .bind(member_default_json(
        models_workspace::workspace_member::DEFAULT_VIEW_PROPS_JSON,
    )?)
    .bind(member_default_json(
        models_workspace::workspace_member::DEFAULT_ISSUE_PROPS_JSON,
    )?)
    .bind(member_default_json(models_workspace::workspace_member::EMPTY_DICT_JSON)?)
    .bind(member_default_json(models_workspace::workspace_member::EMPTY_DICT_JSON)?)
    .bind(member_default_json(models_workspace::workspace_member::EMPTY_DICT_JSON)?)
    .execute(&pool)
    .await;
    if let Err(error) = member_insert {
        // Any unique violation (even the member row's) answers the 409:
        // Postgres renders every one with "already exists".
        if is_unique_violation(&error) {
            return Ok(json_response(
                StatusCode::CONFLICT,
                "{\"slug\":\"The workspace with the slug already exists\"}".to_owned(),
            ));
        }
        return Err(Denial::ServerError);
    }
    let total_members: (i64,) = sqlx::query_as(
        "SELECT COUNT(wm.id) FROM workspace_members wm WHERE wm.workspace_id = $1 AND wm.deleted_at IS NULL",
    )
    .bind(workspace_id)
    .fetch_one(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    // The 201 envelope appends `total_members`/`role` AFTER `owner`
    // (dict insertion order — `:161-163`).
    let data = WorkspaceRowData {
        id: workspace_id,
        created_at,
        updated_at,
        deleted_at: validated.deleted_at.flatten(),
        name: name.clone(),
        logo,
        slug: slug.clone(),
        organization_size,
        timezone: timezone.clone(),
        background_color: background_color.clone(),
        created_by_id: Some(user_id),
        updated_by_id: None,
        logo_asset_id: validated.logo_asset_id.flatten(),
        owner_id: user_id,
        total_members: None,
        role: None,
    };
    let logo_url = resolve_logo_url(&pool, data.logo.as_deref(), data.logo_asset_id).await?;
    let mut rendered = render_workspace_data(&data, logo_url.as_deref(), &zone)?;
    rendered
        .as_object_mut()
        .expect("workspace object")
        .insert("total_members".to_owned(), Value::from(total_members.0));
    rendered
        .as_object_mut()
        .expect("workspace object")
        .insert("role".to_owned(), Value::from(ROLE_ADMIN));
    let created_at_text = render_datetime_in(&created_at, &zone);
    let seed = tasks::workspace_seed_emit(&workspace_id.to_string());
    enqueue_task(
        &pool,
        seed.task_name(),
        vec![Value::String(seed.workspace_id.clone())],
        Map::new(),
    )
    .await;
    let created = tasks::workspace_created_event(
        &user_id.to_string(),
        &workspace_id.to_string(),
        &slug,
        &name,
        &created_at_text,
    );
    enqueue_task(&pool, created.task_name(), Vec::new(), created.kwargs()).await;
    Ok(json_response(StatusCode::CREATED, rendered.to_string()))
}

/// Parse one member JSON default fresh per row (callable defaults —
/// shared references would alias the same object).
fn member_default_json(text: &str) -> Result<Value, Denial> {
    serde_json::from_str(text).map_err(|_| Denial::ServerError)
}

/// `get_configuration_value("DISABLE_WORKSPACE_CREATION", "db",
/// default=os.environ.get(..., "0")) == "1"` (D-01 legacy shim).
async fn workspace_creation_disabled(
    state: &AppState,
    pool: &sqlx::PgPool,
) -> Result<bool, Denial> {
    use pidash_db::config::accessor::PgConfigStore;
    use pidash_db::config::encryption::Keyring;
    use pidash_db::config::legacy::{get_configuration_values, LegacyItem};
    use pidash_db::config::{registry, value::ConfigValue};
    let store = PgConfigStore::new(pool.clone());
    let keyring = Keyring::from_secret(&state.settings().secret_key);
    let default = std::env::var("DISABLE_WORKSPACE_CREATION").unwrap_or_else(|_| "0".to_owned());
    let items = vec![LegacyItem::new(
        "DISABLE_WORKSPACE_CREATION",
        ConfigValue::Str(default),
    )];
    let values = get_configuration_values(registry::global(), &store, &keyring, &items)
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(matches!(values.first(), Some(ConfigValue::Str(flag)) if flag == "1"))
}

/// Fetch one R1 row for the stock detail actions (`get_object` over the
/// member scope + `filter_queryset`): miss renders DRF's `NotFound`.
#[allow(clippy::result_large_err)]
async fn get_workspace_object(
    pool: &sqlx::PgPool,
    slug: &str,
    user_id: &uuid::Uuid,
    filters: &ListFilters,
) -> Result<WorkspaceRowData, Response> {
    let (sql, names) = workspace_detail_sql(filters, true);
    let rows = run_workspace_query(pool, &sql, &names, user_id, Some(slug), filters)
        .await
        .map_err(|denial| denial.into_response())?;
    rows.into_iter()
        .next()
        .map(workspace_row_data)
        .ok_or_else(|| detail_not_found_response("Workspace"))
}

/// GET `workspaces/<slug>/` (stock retrieve).
pub async fn retrieve_workspace(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    Query(query): Query<QueryMap>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> HandlerResult {
    let user_id = actor_user_id(extension)?;
    let pool = pool_of(&state)?;
    debug_assert!(matches!(*gate_for_retrieve(), gates::Gate::ClassBase));
    if let Err(response) = check_class_base(&pool, Some(&slug), &user_id, "GET").await {
        return Ok(response);
    }
    let filters = match parse_list_filters(&pool, &query).await {
        Ok(filters) => filters,
        Err(response) => return Ok(response),
    };
    let data = match get_workspace_object(&pool, &slug, &user_id, &filters).await {
        Ok(data) => data,
        Err(response) => return Ok(response),
    };
    let zone = actor_timezone(&pool, &user_id).await?;
    let rendered = render_workspace_row(&pool, &data, &zone).await?;
    Ok(json_response(StatusCode::OK, rendered.to_string()))
}

/// PUT `workspaces/<slug>/` (stock full update, Admin/Member gate).
pub async fn update_workspace(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    Query(query): Query<QueryMap>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    headers: HeaderMap,
    body: bytes::Bytes,
) -> HandlerResult {
    update_workspace_inner(state, slug, query, extension, headers, body, false).await
}

/// PATCH `workspaces/<slug>/` (`:172-174`, Admin/Member class step then
/// the ADMIN decorator).
pub async fn partial_update_workspace(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    Query(query): Query<QueryMap>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    headers: HeaderMap,
    body: bytes::Bytes,
) -> HandlerResult {
    update_workspace_inner(state, slug, query, extension, headers, body, true).await
}

async fn update_workspace_inner(
    state: AppState,
    slug: String,
    query: QueryMap,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    headers: HeaderMap,
    body: bytes::Bytes,
    partial: bool,
) -> HandlerResult {
    let user_id = actor_user_id(extension)?;
    let pool = pool_of(&state)?;
    if partial {
        debug_assert!(matches!(
            *gate_for_partial_update(),
            gates::Gate::BaseThenAllow { .. }
        ));
        if let Err(response) = check_base_then_allow(&pool, &slug, &user_id, "PATCH").await {
            return Ok(response);
        }
    } else {
        debug_assert!(matches!(*gate_for_update(), gates::Gate::ClassBase));
        if let Err(response) = check_class_base(&pool, Some(&slug), &user_id, "PUT").await {
            return Ok(response);
        }
    }
    let input = match negotiate_body(&headers, &body, WORKSPACE_SKIP_BLANK) {
        Ok(input) => input,
        Err(response) => return Ok(response),
    };
    if !input.json_is_object() && !input.is_html {
        let scalar = input.json_scalar().clone();
        return Ok(non_object_body_response(&scalar));
    }
    let filters = match parse_list_filters(&pool, &query).await {
        Ok(filters) => filters,
        Err(response) => return Ok(response),
    };
    let mut data = match get_workspace_object(&pool, &slug, &user_id, &filters).await {
        Ok(data) => data,
        Err(response) => return Ok(response),
    };
    let zone = actor_timezone(&pool, &user_id).await?;
    let validated =
        match validate_workspace_input(&pool, &input, &zone, partial, Some(&data.id)).await? {
            Ok(validated) => validated,
            Err(errors) => return Ok(field_errors_response(errors)),
        };
    // `update()`: validated attrs land on the row, `save()` re-stamps
    // `updated_at`/`updated_by` even when nothing changed.
    let now = chrono::Utc::now();
    let mut sets: Vec<String> = Vec::new();
    if let Some(deleted_at) = validated.deleted_at {
        data.deleted_at = deleted_at;
        sets.push("deleted_at".to_owned());
    }
    if let Some(name) = validated.name {
        data.name = name;
        sets.push("name".to_owned());
    }
    if let Some(logo) = validated.logo {
        data.logo = logo;
        sets.push("logo".to_owned());
    }
    if let Some(slug) = validated.slug {
        data.slug = slug;
        sets.push("slug".to_owned());
    }
    if let Some(organization_size) = validated.organization_size {
        data.organization_size = organization_size;
        sets.push("organization_size".to_owned());
    }
    if let Some(timezone) = validated.timezone {
        data.timezone = timezone;
        sets.push("timezone".to_owned());
    }
    if let Some(background_color) = validated.background_color {
        data.background_color = background_color;
        sets.push("background_color".to_owned());
    }
    if let Some(logo_asset_id) = validated.logo_asset_id {
        data.logo_asset_id = logo_asset_id;
        sets.push("logo_asset_id".to_owned());
    }
    let outcome = apply_workspace_update(&pool, &data, &sets, &user_id, now).await;
    if let Err(error) = outcome {
        // Unhandled `IntegrityError` (a lost unique race): the
        // `payload is not valid` 400 arm.
        if is_integrity_error(&error) {
            return Ok(json_response(
                StatusCode::BAD_REQUEST,
                "{\"error\":\"The payload is not valid\"}".to_owned(),
            ));
        }
        return Err(Denial::ServerError);
    }
    data.updated_at = now;
    data.updated_by_id = Some(user_id);
    let rendered = render_workspace_row(&pool, &data, &zone).await?;
    Ok(json_response(StatusCode::OK, rendered.to_string()))
}

/// The stock `update()` write: validated columns plus the
/// `updated_at`/`updated_by` re-stamp.
async fn apply_workspace_update(
    pool: &sqlx::PgPool,
    data: &WorkspaceRowData,
    sets: &[String],
    user_id: &uuid::Uuid,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<(), sqlx::Error> {
    let mut assignments: Vec<String> = sets
        .iter()
        .enumerate()
        .map(|(index, column)| format!("{column} = ${}", index + 1))
        .collect();
    assignments.push(format!("updated_at = ${}", sets.len() + 1));
    assignments.push(format!("updated_by_id = ${}", sets.len() + 2));
    let sql = format!(
        "UPDATE workspaces SET {} WHERE id = ${}",
        assignments.join(", "),
        sets.len() + 3
    );
    let mut query = sqlx::query(&sql);
    for column in sets {
        query = match column.as_str() {
            "deleted_at" => query.bind(data.deleted_at),
            "name" => query.bind(&data.name),
            "logo" => query.bind(data.logo.as_deref()),
            "slug" => query.bind(&data.slug),
            "organization_size" => query.bind(data.organization_size.as_deref()),
            "timezone" => query.bind(&data.timezone),
            "background_color" => query.bind(&data.background_color),
            "logo_asset_id" => query.bind(data.logo_asset_id),
            _ => query.bind(Option::<String>::None),
        };
    }
    query
        .bind(now)
        .bind(user_id)
        .bind(data.id)
        .execute(pool)
        .await?;
    Ok(())
}

/// DELETE `workspaces/<slug>/` (`:183-201`): profile-pointer cleanup,
/// the track enqueue, then the soft delete + slug rename + related
/// soft-delete fan-out.
pub async fn destroy_workspace(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    Query(query): Query<QueryMap>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> HandlerResult {
    let user_id = actor_user_id(extension)?;
    let pool = pool_of(&state)?;
    debug_assert!(matches!(
        *gate_for_destroy(),
        gates::Gate::BaseThenAllow { .. }
    ));
    if let Err(response) = check_base_then_allow(&pool, &slug, &user_id, "DELETE").await {
        return Ok(response);
    }
    let filters = match parse_list_filters(&pool, &query).await {
        Ok(filters) => filters,
        Err(response) => return Ok(response),
    };
    let data = match get_workspace_object(&pool, &slug, &user_id, &filters).await {
        Ok(data) => data,
        Err(response) => return Ok(response),
    };
    // `remove_last_workspace_ids_from_user_settings` (`:187`).
    sqlx::query("UPDATE profiles SET last_workspace_id = NULL WHERE last_workspace_id = $1")
        .bind(data.id)
        .execute(&pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    // `str(timezone.now().isoformat())`: micros-truncated UTC, `+00:00`.
    let now_micros = chrono::Utc::now().timestamp_micros();
    let now = chrono::DateTime::from_timestamp_micros(now_micros).expect("micros in range");
    let deleted_at_text = now.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, false);
    let deleted = tasks::workspace_deleted_event(
        &user_id.to_string(),
        &data.id.to_string(),
        &data.slug,
        &data.name,
        &deleted_at_text,
    );
    enqueue_task(&pool, deleted.task_name(), Vec::new(), deleted.kwargs()).await;
    // `super().destroy()` (`mixins.py:72-78` + `workspace.py:107-118`):
    // soft delete, slug tombstone, `updated_by` re-stamp, fan-out.
    let tombstone = models_workspace::workspace::soft_deleted_slug(&data.slug, now);
    sqlx::query(
        "UPDATE workspaces SET deleted_at = $1, updated_at = $1, updated_by_id = $2, slug = $3 WHERE id = $4",
    )
    .bind(now)
    .bind(user_id)
    .bind(&tombstone)
    .bind(data.id)
    .execute(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    enqueue_soft_delete_related(&pool, "db", "workspace", &data.id).await;
    Ok(empty_response(StatusCode::NO_CONTENT))
}

// ---------------------------------------------------------------------------
// Handlers: `UserWorkSpacesEndpoint`, `WorkSpaceAvailabilityCheckEndpoint`
// ---------------------------------------------------------------------------

/// GET `users/me/workspaces/` (`:209-240`, R2): the `?fields=` param is
/// dead (full shape always), then search/owner filters, newest first.
pub async fn my_workspaces(
    State(state): State<AppState>,
    Query(query): Query<QueryMap>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> HandlerResult {
    let user_id = actor_user_id(extension)?;
    let pool = pool_of(&state)?;
    debug_assert!(matches!(
        *gate_for_my_workspaces(),
        gates::Gate::Authenticated
    ));
    let scope = gates::tenant_context("");
    let facts = allow_facts_for("", None, &[]);
    debug_assert!(
        apply_outcome(gates::decide_gate(gate_for_my_workspaces(), &scope, &facts)).is_ok()
    );
    let filters = match parse_list_filters(&pool, &query).await {
        Ok(filters) => filters,
        Err(response) => return Ok(response),
    };
    let (sql, names) = my_workspaces_sql(&filters);
    let rows = run_workspace_query(&pool, &sql, &names, &user_id, None, &filters).await?;
    let zone = actor_timezone(&pool, &user_id).await?;
    let mut rendered = Vec::with_capacity(rows.len());
    for tuple in &rows {
        let data = workspace_row_data(tuple.clone());
        rendered.push(render_workspace_row(&pool, &data, &zone).await?);
    }
    Ok(json_response(
        StatusCode::OK,
        Value::Array(rendered).to_string(),
    ))
}

/// GET `workspace-slug-check/` (`:244-254`): missing `?slug=` 400s, a
/// live row or a restricted slug answers `{"status": false}`.
pub async fn slug_check(
    State(state): State<AppState>,
    Query(query): Query<QueryMap>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> HandlerResult {
    let _user_id = actor_user_id(extension)?;
    let pool = pool_of(&state)?;
    debug_assert!(matches!(*gate_for_slug_check(), gates::Gate::Authenticated));
    let scope = gates::tenant_context("");
    let facts = allow_facts_for("", None, &[]);
    debug_assert!(apply_outcome(gates::decide_gate(gate_for_slug_check(), &scope, &facts)).is_ok());
    let Some(slug) = query_last(&query, "slug") else {
        return Ok(json_response(
            StatusCode::BAD_REQUEST,
            "{\"error\":\"Workspace Slug is required\"}".to_owned(),
        ));
    };
    if slug.is_empty() {
        return Ok(json_response(
            StatusCode::BAD_REQUEST,
            "{\"error\":\"Workspace Slug is required\"}".to_owned(),
        ));
    }
    let row: Option<(uuid::Uuid,)> =
        sqlx::query_as("SELECT w.id FROM workspaces w WHERE w.slug = $1 AND w.deleted_at IS NULL")
            .bind(&slug)
            .fetch_optional(&pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    let taken = row.is_some() || RESTRICTED_WORKSPACE_SLUGS.contains(&slug.as_str());
    Ok(json_response(
        StatusCode::OK,
        format!("{{\"status\":{}}}", if taken { "false" } else { "true" }),
    ))
}

// ---------------------------------------------------------------------------
// Handlers: `WorkspaceThemeViewSet`
// ---------------------------------------------------------------------------

/// GET `workspaces/<slug>/workspace-themes/` (Admin/Member, newest first).
pub async fn list_themes(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> HandlerResult {
    let user_id = actor_user_id(extension)?;
    let pool = pool_of(&state)?;
    debug_assert!(matches!(*gate_for_theme_list(), gates::Gate::ClassAdmin));
    if let Err(response) = check_class_admin(&pool, &slug, &user_id).await {
        return Ok(response);
    }
    let (sql, names) = theme_list_sql();
    let rows = run_theme_query(&pool, &sql, &names, &slug, None).await?;
    let zone = actor_timezone(&pool, &user_id).await?;
    let mut rendered = Vec::with_capacity(rows.len());
    for tuple in &rows {
        rendered.push(render_theme_row(tuple, &zone)?);
    }
    Ok(json_response(
        StatusCode::OK,
        Value::Array(rendered).to_string(),
    ))
}

/// POST `workspaces/<slug>/workspace-themes/` (`:356-365`): the workspace
/// lookup 404s before validation; a lost `(workspace, name)` race 400s
/// through the `IntegrityError` arm.
pub async fn create_theme(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    headers: HeaderMap,
    body: bytes::Bytes,
) -> HandlerResult {
    let user_id = actor_user_id(extension)?;
    let pool = pool_of(&state)?;
    debug_assert!(matches!(*gate_for_theme_create(), gates::Gate::ClassAdmin));
    if let Err(response) = check_class_admin(&pool, &slug, &user_id).await {
        return Ok(response);
    }
    let input = match negotiate_body(&headers, &body, THEME_SKIP_BLANK) {
        Ok(input) => input,
        Err(response) => return Ok(response),
    };
    if !input.json_is_object() && !input.is_html {
        let scalar = input.json_scalar().clone();
        return Ok(non_object_body_response(&scalar));
    }
    let workspace_id: Option<(uuid::Uuid,)> =
        sqlx::query_as("SELECT w.id FROM workspaces w WHERE w.slug = $1 AND w.deleted_at IS NULL")
            .bind(&slug)
            .fetch_optional(&pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    let Some((workspace_id,)) = workspace_id else {
        return Err(Denial::NotFound);
    };
    let zone = actor_timezone(&pool, &user_id).await?;
    let validated = match validate_theme_input(&input, &zone, false)? {
        Ok(validated) => validated,
        Err(errors) => return Ok(field_errors_response(errors)),
    };
    let (Some(deleted_at), Some(name)) = (validated.deleted_at, validated.name) else {
        return Err(Denial::ServerError);
    };
    let colors = validated
        .colors
        .unwrap_or_else(|| Value::Object(Map::new()));
    let theme_id = uuid::Uuid::new_v4();
    let now = chrono::Utc::now();
    let insert: Result<InsertedStamps, sqlx::Error> =
        sqlx::query_as(
            r#"INSERT INTO workspace_themes
               (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, workspace_id, name, actor_id, colors)
               VALUES ($1, $2, $2, $3, NULL, $4, $5, $6, $3, $7)
               RETURNING created_at, updated_at"#,
        )
        .bind(theme_id)
        .bind(now)
        .bind(user_id)
        .bind(deleted_at)
        .bind(workspace_id)
        .bind(&name)
        .bind(&colors)
        .fetch_optional(&pool)
        .await;
    let (created_at, updated_at) = match insert {
        Ok(Some(timestamps)) => timestamps,
        Ok(None) => return Err(Denial::ServerError),
        Err(error) if is_integrity_error(&error) => {
            return Ok(json_response(
                StatusCode::BAD_REQUEST,
                "{\"error\":\"The payload is not valid\"}".to_owned(),
            ));
        }
        Err(_) => return Err(Denial::ServerError),
    };
    let tuple: ThemeRowTuple = (
        theme_id,
        created_at,
        updated_at,
        deleted_at,
        name,
        colors,
        Some(user_id),
        None,
        workspace_id,
        user_id,
    );
    let rendered = render_theme_row(&tuple, &zone)?;
    Ok(json_response(StatusCode::CREATED, rendered.to_string()))
}

/// Fetch one slug-scoped theme (`get_object`): miss renders DRF's
/// `NotFound`, a malformed `pk` the `ValidationError` 400.
#[allow(clippy::result_large_err)]
async fn get_theme_object(
    pool: &sqlx::PgPool,
    slug: &str,
    pk: &str,
) -> Result<ThemeRowTuple, Response> {
    let theme_id = parse_uuid_or_invalid(pk).map_err(|denial| denial.into_response())?;
    let (sql, names) = theme_detail_sql();
    let rows = run_theme_query(pool, &sql, &names, slug, Some(&theme_id))
        .await
        .map_err(|denial| denial.into_response())?;
    rows.into_iter()
        .next()
        .ok_or_else(|| detail_not_found_response("WorkspaceTheme"))
}

/// GET `workspaces/<slug>/workspace-themes/<pk>/` (stock retrieve).
pub async fn retrieve_theme(
    State(state): State<AppState>,
    Path((slug, pk)): Path<(String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> HandlerResult {
    let user_id = actor_user_id(extension)?;
    let pool = pool_of(&state)?;
    debug_assert!(matches!(
        *gate_for_theme_detail("GET"),
        gates::Gate::ClassAdmin
    ));
    if let Err(response) = check_class_admin(&pool, &slug, &user_id).await {
        return Ok(response);
    }
    let tuple = match get_theme_object(&pool, &slug, &pk).await {
        Ok(tuple) => tuple,
        Err(response) => return Ok(response),
    };
    let zone = actor_timezone(&pool, &user_id).await?;
    let rendered = render_theme_row(&tuple, &zone)?;
    Ok(json_response(StatusCode::OK, rendered.to_string()))
}

/// PATCH `workspaces/<slug>/workspace-themes/<pk>/` (stock partial
/// update): the re-stamp runs even when nothing changed.
pub async fn partial_update_theme(
    State(state): State<AppState>,
    Path((slug, pk)): Path<(String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    headers: HeaderMap,
    body: bytes::Bytes,
) -> HandlerResult {
    let user_id = actor_user_id(extension)?;
    let pool = pool_of(&state)?;
    debug_assert!(matches!(
        *gate_for_theme_detail("PATCH"),
        gates::Gate::ClassAdmin
    ));
    if let Err(response) = check_class_admin(&pool, &slug, &user_id).await {
        return Ok(response);
    }
    let input = match negotiate_body(&headers, &body, THEME_SKIP_BLANK) {
        Ok(input) => input,
        Err(response) => return Ok(response),
    };
    if !input.json_is_object() && !input.is_html {
        let scalar = input.json_scalar().clone();
        return Ok(non_object_body_response(&scalar));
    }
    let mut tuple = match get_theme_object(&pool, &slug, &pk).await {
        Ok(tuple) => tuple,
        Err(response) => return Ok(response),
    };
    let zone = actor_timezone(&pool, &user_id).await?;
    let validated = match validate_theme_input(&input, &zone, true)? {
        Ok(validated) => validated,
        Err(errors) => return Ok(field_errors_response(errors)),
    };
    let now = chrono::Utc::now();
    let outcome = apply_theme_update(&pool, &mut tuple, &validated, &user_id, now).await;
    if let Err(error) = outcome {
        if is_integrity_error(&error) {
            return Ok(json_response(
                StatusCode::BAD_REQUEST,
                "{\"error\":\"The payload is not valid\"}".to_owned(),
            ));
        }
        return Err(Denial::ServerError);
    }
    tuple.2 = now;
    tuple.7 = Some(user_id);
    let rendered = render_theme_row(&tuple, &zone)?;
    Ok(json_response(StatusCode::OK, rendered.to_string()))
}

async fn apply_theme_update(
    pool: &sqlx::PgPool,
    tuple: &mut ThemeRowTuple,
    validated: &ValidatedTheme,
    user_id: &uuid::Uuid,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<(), sqlx::Error> {
    let mut assignments: Vec<String> = Vec::new();
    let mut index = 1;
    if let Some(deleted_at) = validated.deleted_at {
        tuple.3 = deleted_at;
        assignments.push(format!("deleted_at = ${index}"));
        index += 1;
    }
    if let Some(name) = validated.name.clone() {
        tuple.4 = name;
        assignments.push(format!("name = ${index}"));
        index += 1;
    }
    if let Some(colors) = validated.colors.clone() {
        tuple.5 = colors;
        assignments.push(format!("colors = ${index}"));
        index += 1;
    }
    assignments.push(format!("updated_at = ${index}"));
    index += 1;
    assignments.push(format!("updated_by_id = ${index}"));
    index += 1;
    let sql = format!(
        "UPDATE workspace_themes SET {} WHERE id = ${index}",
        assignments.join(", ")
    );
    let mut query = sqlx::query(&sql);
    if let Some(deleted_at) = validated.deleted_at {
        query = query.bind(deleted_at);
    }
    if let Some(name) = validated.name.as_deref() {
        query = query.bind(name);
    }
    if let Some(colors) = validated.colors.as_ref() {
        query = query.bind(colors);
    }
    query
        .bind(now)
        .bind(user_id)
        .bind(tuple.0)
        .execute(pool)
        .await?;
    Ok(())
}

/// DELETE `workspaces/<slug>/workspace-themes/<pk>/` (stock destroy):
/// soft delete + related fan-out, 204.
pub async fn destroy_theme(
    State(state): State<AppState>,
    Path((slug, pk)): Path<(String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> HandlerResult {
    let user_id = actor_user_id(extension)?;
    let pool = pool_of(&state)?;
    debug_assert!(matches!(
        *gate_for_theme_detail("DELETE"),
        gates::Gate::ClassAdmin
    ));
    if let Err(response) = check_class_admin(&pool, &slug, &user_id).await {
        return Ok(response);
    }
    let tuple = match get_theme_object(&pool, &slug, &pk).await {
        Ok(tuple) => tuple,
        Err(response) => return Ok(response),
    };
    let now = chrono::Utc::now();
    sqlx::query(
        "UPDATE workspace_themes SET deleted_at = $1, updated_at = $1, updated_by_id = $2 WHERE id = $3",
    )
    .bind(now)
    .bind(user_id)
    .bind(tuple.0)
    .execute(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    enqueue_soft_delete_related(&pool, "db", "workspacetheme", &tuple.0).await;
    Ok(empty_response(StatusCode::NO_CONTENT))
}

// ---------------------------------------------------------------------------
// Handler: `ExportWorkspaceUserActivityEndpoint`
// ---------------------------------------------------------------------------

/// POST `workspaces/<slug>/user-activity/<user_id>/export/` (`:379-420`):
/// `date`-required, then the R5 CSV (`text/csv` + Content-Disposition).
pub async fn export_activity(
    State(state): State<AppState>,
    Path((slug, user_id_text)): Path<(String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    headers: HeaderMap,
    body: bytes::Bytes,
) -> HandlerResult {
    let user_id = actor_user_id(extension)?;
    let pool = pool_of(&state)?;
    debug_assert!(matches!(*gate_for_export(), gates::Gate::ClassEntity));
    if let Err(response) = check_class_entity(&pool, &slug, &user_id, "POST").await {
        return Ok(response);
    }
    let input = match negotiate_body(&headers, &body, NO_SKIP_BLANK) {
        Ok(input) => input,
        Err(response) => return Ok(response),
    };
    // `request.data.get("date")` on a non-object body is the
    // `AttributeError` 500 arm.
    if !input.json_is_object() && !input.is_html {
        return Err(Denial::ServerError);
    }
    let date_value = match input.get("date") {
        InputValue::Missing => None,
        InputValue::File(_) => return Err(Denial::ServerError),
        InputValue::Value(value) => Some(value),
    };
    if !date_value.map(py_truthy).unwrap_or(false) {
        return Ok(json_response(
            StatusCode::BAD_REQUEST,
            "{\"error\":\"Date is required\"}".to_owned(),
        ));
    }
    let date_value = date_value.expect("truthy date");
    let Value::String(date_text) = date_value else {
        // Non-string truthy input is the `TypeError` 500 arm.
        return Err(Denial::ServerError);
    };
    let Some(date) = parse_export_date(date_text) else {
        return Err(Denial::BadError(INVALID_DETAIL_MSG.to_owned()));
    };
    let target_id = parse_uuid_or_invalid(&user_id_text)?;
    let zone = actor_timezone(&pool, &user_id).await?;
    let zone_name = zone.name().to_owned();
    let (sql, names) = export_sql();
    let mut query = sqlx::query_as::<_, ExportRowTuple>(&sql);
    for name in &names {
        if name == "slug" {
            query = query.bind(&slug);
        } else if name == "user" {
            query = query.bind(user_id);
        } else if name == "user_id" {
            query = query.bind(target_id);
        } else if name == "tzname" {
            query = query.bind(&zone_name);
        } else if name == "date" {
            query = query.bind(date);
        } else {
            return Err(Denial::ServerError);
        }
    }
    let rows = query
        .fetch_all(&pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let csv = render_export_csv(&rows, &zone);
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/csv")
        .header(
            header::CONTENT_DISPOSITION,
            "attachment; filename=\"workspace-user-activity.csv\"",
        )
        .body(axum::body::Body::from(csv))
        .expect("csv response"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::header::HeaderValue;
    use pidash_auth::permissions::ROLE_GUEST;

    const FIXTURE_ROUTES: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/app_workspace/handlers/routes.golden.json"
    );

    fn fixture() -> Value {
        let text = std::fs::read_to_string(FIXTURE_ROUTES).expect("F-W24-15 fixture");
        serde_json::from_str(&text).expect("fixture JSON")
    }

    /// Fixture error entries: `(status, body, source)`.
    fn fixture_errors() -> Vec<(u64, Value, String)> {
        let loaded = fixture();
        loaded["errors"]
            .as_array()
            .expect("errors array")
            .iter()
            .map(|entry| {
                (
                    entry["status"].as_u64().unwrap_or_default(),
                    entry["body"].clone(),
                    entry["source"].as_str().unwrap_or_default().to_owned(),
                )
            })
            .collect()
    }

    fn assert_fixture_error(status: u64, body: Value, source_part: &str) {
        let entries = fixture_errors();
        assert!(
            entries
                .iter()
                .any(|(entry_status, entry_body, entry_source)| {
                    *entry_status == status
                        && *entry_body == body
                        && entry_source.contains(source_part)
                }),
            "fixture pins {status} {body} ({source_part})"
        );
    }

    fn json_body(pairs: &[(&str, Value)]) -> InputBody {
        let mut map = Map::new();
        for (key, value) in pairs {
            map.insert((*key).to_owned(), value.clone());
        }
        InputBody {
            is_html: false,
            map,
            files: BTreeMap::new(),
            scalar: None,
        }
    }

    fn str_value(text: &str) -> Value {
        Value::String(text.to_owned())
    }

    fn utc_zone() -> Tz {
        "UTC".parse().expect("UTC zone")
    }

    // -- fixture cross-checks (F-W24-15) ------------------------------------

    #[test]
    fn fixture_pins_my_error_bodies() {
        for (status, body, source) in [
            (
                403,
                serde_json::json!({"error": "Workspace creation is not allowed"}),
                "workspace/base.py:94-98",
            ),
            (
                400,
                serde_json::json!({"error": "Both name and slug are required"}),
                "workspace/base.py:105-109",
            ),
            (
                400,
                serde_json::json!({"error": "The maximum length for name is 80 and for slug is 48"}),
                "workspace/base.py:111-115",
            ),
            (
                400,
                serde_json::json!({"error": "Name cannot contain a URL"}),
                "workspace/base.py:117-121",
            ),
            (
                409,
                serde_json::json!({"slug": "The workspace with the slug already exists"}),
                "workspace/base.py:161-166",
            ),
            (
                400,
                serde_json::json!({"error": "Workspace Slug is required"}),
                "workspace/base.py:247-251",
            ),
            (
                400,
                serde_json::json!({"error": "The required key does not exist."}),
                "workspace/base.py:168-170",
            ),
            (
                400,
                serde_json::json!({"error": "Date is required"}),
                "workspace/base.py:380-381",
            ),
            (
                404,
                serde_json::json!({"error": "The required object does not exist."}),
                "draft.py:190-194",
            ),
        ] {
            assert_fixture_error(status, body, source);
        }
    }

    #[test]
    fn fixture_pins_export_csv_shape() {
        let loaded = fixture();
        let csv = loaded["csv"].as_object().expect("csv notes");
        assert_eq!(
            csv["header"],
            serde_json::json!([
                "Actor name",
                "Issue ID",
                "Project",
                "Created at",
                "Updated at",
                "Action",
                "Field",
                "Old value",
                "New value"
            ])
        );
        assert_eq!(csv["content_type"], "text/csv");
        assert!(csv["filename"]
            .as_str()
            .expect("filename")
            .contains("workspace-user-activity.csv"));
    }

    #[test]
    fn route_paths_match_fixture_table() {
        let loaded = fixture();
        let routes = loaded["routes"].as_array().expect("routes");
        let table: Vec<String> = routes
            .iter()
            .filter_map(|entry| entry.as_str().map(str::to_owned))
            .collect();
        for needle in [
            "W01 GET workspace-slug-check/ -> WorkSpaceAvailabilityCheckEndpoint:get",
            "W02 GET workspaces/ -> WorkSpaceViewSet:list [400 BUG] + POST -> :create",
            "W03 workspaces/<slug>/ -> WorkSpaceViewSet:retrieve+update+partial_update+destroy GET+PUT+PATCH+DELETE",
            "W19 GET+POST workspaces/<slug>/workspace-themes/ -> WorkspaceThemeViewSet:list+create",
            "W20 workspaces/<slug>/workspace-themes/<pk>/ -> WorkspaceThemeViewSet:retrieve+partial_update+destroy GET+PATCH+DELETE",
            "W23 POST workspaces/<slug>/user-activity/<user_id>/export/ -> ExportWorkspaceUserActivityEndpoint:post (CSV;",
        ] {
            assert!(
                table.iter().any(|entry| entry.contains(needle)),
                "fixture pins {needle}"
            );
        }
        assert_eq!(SLUG_CHECK_PATH, "/api/workspace-slug-check/");
        assert_eq!(WORKSPACES_PATH, "/api/workspaces/");
        assert_eq!(THEME_PATH, "/api/workspaces/{slug}/workspace-themes/{pk}/");
        assert_eq!(
            EXPORT_PATH,
            "/api/workspaces/{slug}/user-activity/{user_id}/export/"
        );
    }

    // -- gate rows + outcomes ------------------------------------------------

    #[test]
    fn gate_rows_cover_all_twelve_routes() {
        assert!(matches!(*gate_for_list(), gates::Gate::CollectionList));
        assert!(matches!(*gate_for_create(), gates::Gate::ClassBase));
        assert!(matches!(*gate_for_retrieve(), gates::Gate::ClassBase));
        assert!(matches!(*gate_for_update(), gates::Gate::ClassBase));
        assert!(matches!(
            *gate_for_partial_update(),
            gates::Gate::BaseThenAllow { .. }
        ));
        assert!(matches!(
            *gate_for_destroy(),
            gates::Gate::BaseThenAllow { .. }
        ));
        assert!(matches!(*gate_for_slug_check(), gates::Gate::Authenticated));
        assert!(matches!(
            *gate_for_my_workspaces(),
            gates::Gate::Authenticated
        ));
        assert!(matches!(*gate_for_theme_list(), gates::Gate::ClassAdmin));
        assert!(matches!(*gate_for_theme_create(), gates::Gate::ClassAdmin));
        assert!(matches!(
            *gate_for_theme_detail("GET"),
            gates::Gate::ClassAdmin
        ));
        assert!(matches!(
            *gate_for_theme_detail("PATCH"),
            gates::Gate::ClassAdmin
        ));
        assert!(matches!(
            *gate_for_theme_detail("DELETE"),
            gates::Gate::ClassAdmin
        ));
        assert!(matches!(*gate_for_export(), gates::Gate::ClassEntity));
        // No PUT row on the theme detail: PUT 405s.
        assert!(gates::gate_for("PUT", "workspaces/<slug>/workspace-themes/<pk>/").is_none());
    }

    #[test]
    fn list_bug_missing_slug_without_membership_consult() {
        let scope = gates::tenant_context("");
        let facts = allow_facts_for("", None, &[]);
        assert!(matches!(
            gates::decide_gate(gate_for_list(), &scope, &facts),
            gates::GateOutcome::MissingSlug
        ));
        assert_eq!(
            gates::MISSING_KEY_BODY,
            "{\"error\":\"The required key does not exist.\"}"
        );
    }

    #[test]
    fn class_outcomes_follow_python_matrix() {
        let scope = gates::tenant_context("acme");
        let member = class_facts_for("acme", Some(ROLE_MEMBER), false);
        let guest = class_facts_for("acme", Some(ROLE_GUEST), false);
        let outsider = class_facts_for("acme", None, false);
        // Create POST passes any login.
        assert!(matches!(
            gates::decide_class_base("POST", &scope, &outsider),
            gates::GateOutcome::Allow
        ));
        // Retrieve GET passes any login (the member scope 404s later).
        assert!(matches!(
            gates::decide_class_base("GET", &scope, &outsider),
            gates::GateOutcome::Allow
        ));
        // PUT: Admin/Member only.
        assert!(matches!(
            gates::decide_class_base("PUT", &scope, &member),
            gates::GateOutcome::Allow
        ));
        assert!(matches!(
            gates::decide_class_base("PUT", &scope, &guest),
            gates::GateOutcome::DenyClass
        ));
        // Class-level bodies differ from decorator bodies.
        assert_eq!(
            gates::CLASS_DENIED_BODY,
            "{\"detail\":\"You do not have permission to perform this action.\"}"
        );
        assert_eq!(
            gates::FORBIDDEN_BODY,
            "{\"error\":\"You don't have the required permissions.\"}"
        );
    }

    #[test]
    fn patch_delete_compose_class_then_admin() {
        let scope = gates::tenant_context("acme");
        let member_allow = allow_facts_for("acme", Some(ROLE_MEMBER), &[ROLE_ADMIN]);
        let member_class = class_facts_for("acme", Some(ROLE_MEMBER), false);
        // Member clears the class step, then the ADMIN decorator denies.
        assert!(matches!(
            gates::decide_base_then_allow("PATCH", &scope, &member_allow, &member_class),
            gates::GateOutcome::Deny
        ));
        let admin_allow = allow_facts_for("acme", Some(ROLE_ADMIN), &[ROLE_ADMIN]);
        let admin_class = class_facts_for("acme", Some(ROLE_ADMIN), true);
        assert!(matches!(
            gates::decide_base_then_allow("DELETE", &scope, &admin_allow, &admin_class),
            gates::GateOutcome::Allow
        ));
        let guest_allow = allow_facts_for("acme", Some(ROLE_GUEST), &[ROLE_ADMIN]);
        let guest_class = class_facts_for("acme", Some(ROLE_GUEST), false);
        assert!(matches!(
            gates::decide_base_then_allow("DELETE", &scope, &guest_allow, &guest_class),
            gates::GateOutcome::DenyClass
        ));
    }

    #[test]
    fn theme_and_export_gates() {
        let scope = gates::tenant_context("acme");
        let member = class_facts_for("acme", Some(ROLE_MEMBER), false);
        let guest = class_facts_for("acme", Some(ROLE_GUEST), false);
        assert!(matches!(
            gates::decide_class_admin(&scope, &member),
            gates::GateOutcome::Allow
        ));
        assert!(matches!(
            gates::decide_class_admin(&scope, &guest),
            gates::GateOutcome::DenyClass
        ));
        // Export POST is a write: Admin/Member only.
        assert!(matches!(
            gates::decide_class_entity("POST", &scope, &member),
            gates::GateOutcome::Allow
        ));
        assert!(matches!(
            gates::decide_class_entity("POST", &scope, &guest),
            gates::GateOutcome::DenyClass
        ));
    }

    #[tokio::test]
    async fn not_allowed_preludes_deny_before_405() {
        // DRF runs initial() before the 405 lookup, so unowned methods
        // deny first: anonymous 401s everywhere, denied members 403,
        // and only survivors see the 405 bytes. The class checks below
        // are the exact calls the per-route 405 handlers make (pool
        // access makes the handlers themselves untestable without a
        // live database). Both denial bodies are lowercase today via
        // the shared consts — PIDASHCONV-708 owns the capital fix, so
        // these literals pin the current bytes truthfully.
        assert!(matches!(actor_user_id(None), Err(Denial::Unauthorized)));
        let response = Denial::Unauthorized.into_response();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            response_text(response).await,
            "{\"detail\":\"Authentication credentials were not provided.\"}"
        );
        // Authed PUT/PATCH/DELETE on the collection: the class check
        // runs with workspace_slug=None, so every caller denies ...
        let collection_scope = gates::tenant_context("");
        let no_facts = class_facts_for("", None, false);
        for method in ["PUT", "PATCH", "DELETE"] {
            assert!(
                matches!(
                    gates::decide_class_base(method, &collection_scope, &no_facts),
                    gates::GateOutcome::DenyClass
                ),
                "{method} on the collection denies"
            );
        }
        // ... while POST and the safe methods pass any login ...
        for method in ["POST", "GET", "HEAD"] {
            assert!(
                matches!(
                    gates::decide_class_base(method, &collection_scope, &no_facts),
                    gates::GateOutcome::Allow
                ),
                "{method} on the collection survives to the 405"
            );
        }
        // ... and the denial renders the shared class-denial bytes.
        let denied =
            apply_outcome(gates::GateOutcome::DenyClass).expect_err("deny maps to a response");
        assert_eq!(denied.status(), StatusCode::FORBIDDEN);
        assert_eq!(
            response_text(denied).await,
            "{\"detail\":\"You do not have permission to perform this action.\"}"
        );
        // Outsider GET on export hits the entity safe arm (any
        // membership), outsider writes hit the Admin/Member arm;
        // members survive both.
        let scope = gates::tenant_context("acme");
        let member = class_facts_for("acme", Some(ROLE_MEMBER), false);
        let outsider = class_facts_for("acme", None, false);
        assert!(matches!(
            gates::decide_class_entity("GET", &scope, &outsider),
            gates::GateOutcome::DenyClass
        ));
        assert!(matches!(
            gates::decide_class_entity("PUT", &scope, &outsider),
            gates::GateOutcome::DenyClass
        ));
        assert!(matches!(
            gates::decide_class_entity("GET", &scope, &member),
            gates::GateOutcome::Allow
        ));
        assert!(matches!(
            gates::decide_class_entity("DELETE", &scope, &member),
            gates::GateOutcome::Allow
        ));
        // Guests read but do not write under the entity permission.
        let guest = class_facts_for("acme", Some(ROLE_GUEST), false);
        assert!(matches!(
            gates::decide_class_entity("GET", &scope, &guest),
            gates::GateOutcome::Allow
        ));
        assert!(matches!(
            gates::decide_class_entity("PATCH", &scope, &guest),
            gates::GateOutcome::DenyClass
        ));
        // Theme routes: outsiders deny on every method, members survive.
        assert!(matches!(
            gates::decide_class_admin(&scope, &outsider),
            gates::GateOutcome::DenyClass
        ));
        assert!(matches!(
            gates::decide_class_admin(&scope, &member),
            gates::GateOutcome::Allow
        ));
        // Detail POST/HEAD pass any login (no lookup precedes them).
        assert!(matches!(
            gates::decide_class_base("POST", &scope, &outsider),
            gates::GateOutcome::Allow
        ));
        assert!(matches!(
            gates::decide_class_base("HEAD", &scope, &outsider),
            gates::GateOutcome::Allow
        ));
    }

    #[tokio::test]
    async fn anon_unowned_methods_answer_401_on_every_route() {
        use axum::http::Request;
        use tower::ServiceExt;
        // One unowned method per route (each 405 handler runs at least
        // once): anonymous callers 401 through the router without ever
        // touching the database, so no pool is attached.
        let app = Router::new()
            .merge(routes())
            .with_state(AppState::new("test"));
        for (method, uri) in [
            ("POST", "/api/workspace-slug-check/"),
            ("PUT", "/api/workspaces/"),
            ("POST", "/api/workspaces/acme/"),
            ("DELETE", "/api/users/me/workspaces/"),
            ("PUT", "/api/workspaces/acme/workspace-themes/"),
            (
                "POST",
                "/api/workspaces/acme/workspace-themes/11111111-1111-1111-1111-111111111111/",
            ),
            (
                "GET",
                "/api/workspaces/acme/user-activity/11111111-1111-1111-1111-111111111111/export/",
            ),
            (
                "HEAD",
                "/api/workspaces/acme/user-activity/11111111-1111-1111-1111-111111111111/export/",
            ),
        ] {
            let request = Request::builder()
                .method(method)
                .uri(uri)
                .body(axum::body::Body::empty())
                .expect("test request");
            let response = app.clone().oneshot(request).await.expect("route serves");
            assert_eq!(
                response.status(),
                StatusCode::UNAUTHORIZED,
                "{method} {uri}"
            );
            // Axum strips the body for HEAD (wire-correct: the status
            // and headers still answer); every other method serves it.
            let expected = if method == "HEAD" {
                String::new()
            } else {
                "{\"detail\":\"Authentication credentials were not provided.\"}".to_owned()
            };
            assert_eq!(response_text(response).await, expected, "{method} {uri}");
        }
    }

    // -- body negotiation ----------------------------------------------------

    fn json_headers() -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
        headers
    }

    #[test]
    fn empty_body_decodes_to_empty_object() {
        let input = negotiate_body(&json_headers(), b"", NO_SKIP_BLANK).expect("empty body");
        assert!(!input.is_html);
        assert!(input.json_is_object());
        assert!(input.map.is_empty());
        // Whatever the content type claims.
        let mut headers = HeaderMap::new();
        headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("text/plain"));
        let input = negotiate_body(&headers, b"", NO_SKIP_BLANK).expect("empty body");
        assert!(input.map.is_empty());
    }

    #[test]
    fn missing_or_unknown_content_type_415s() {
        let response = negotiate_body(&HeaderMap::new(), b"{}", NO_SKIP_BLANK).expect_err("415");
        assert_eq!(response.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
        let mut headers = HeaderMap::new();
        headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("text/csv"));
        let response = negotiate_body(&headers, b"a,b", NO_SKIP_BLANK).expect_err("415");
        assert_eq!(response.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }

    #[test]
    fn malformed_json_is_parse_error_400() {
        let response =
            negotiate_body(&json_headers(), b"{oops", NO_SKIP_BLANK).expect_err("parse error");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn json_bodies_keep_whole_values() {
        let input =
            negotiate_body(&json_headers(), br#"{"name":"X"}"#, NO_SKIP_BLANK).expect("object");
        assert!(input.json_is_object());
        let input = negotiate_body(&json_headers(), b"[1]", NO_SKIP_BLANK).expect("list");
        assert!(!input.json_is_object());
        assert_eq!(*input.json_scalar(), serde_json::json!([1]));
        let input = negotiate_body(&json_headers(), b"null", NO_SKIP_BLANK).expect("null");
        assert!(input.json_scalar().is_null());
    }

    #[test]
    fn empty_key_objects_stay_objects() {
        // A genuine object containing an "" key is still an object (the
        // non-object sentinel must not collide with it).
        let input = negotiate_body(&json_headers(), br#"{"":1,"name":"X"}"#, NO_SKIP_BLANK)
            .expect("object");
        assert!(input.json_is_object());
        assert_eq!(input.map.len(), 2);
    }

    #[test]
    fn urlencoded_parses_last_wins_with_skip_blank() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/x-www-form-urlencoded"),
        );
        let input = negotiate_body(
            &headers,
            b"name=A&name=B&timezone=&background_color=&logo=",
            WORKSPACE_SKIP_BLANK,
        )
        .expect("form");
        assert!(input.is_html);
        assert_eq!(input.map["name"], str_value("B"));
        assert!(!input.map.contains_key("timezone"));
        assert!(!input.map.contains_key("background_color"));
        assert_eq!(input.map["logo"], str_value(""));
        assert_eq!(unquote_plus("a+b%20c%2F"), "a b c/");
        assert_eq!(unquote_plus("100%25"), "100%");
    }

    #[test]
    fn multipart_parses_fields_and_file_presence() {
        let boundary = "BOUNDARY";
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_str(&format!("multipart/form-data; boundary={boundary}"))
                .expect("ct"),
        );
        let body = format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"name\"\r\n\r\nX\r\n\
             --{boundary}\r\nContent-Disposition: form-data; name=\"colors\"\r\n\r\n\r\n\
             --{boundary}\r\nContent-Disposition: form-data; name=\"logo\"; filename=\"l.png\"\r\nContent-Type: image/png\r\n\r\nBYTES\r\n\
             --{boundary}--\r\n"
        );
        let input = negotiate_body(&headers, body.as_bytes(), THEME_SKIP_BLANK).expect("multipart");
        assert!(input.is_html);
        assert_eq!(input.map["name"], str_value("X"));
        assert!(!input.map.contains_key("colors"));
        assert_eq!(input.files["logo"], "l.png");
        assert!(matches!(input.get("logo"), InputValue::File("l.png")));
        // A missing boundary is the parser-error 500 arm.
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("multipart/form-data"),
        );
        let response =
            negotiate_body(&headers, b"--x\r\n\r\n", NO_SKIP_BLANK).expect_err("no boundary");
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn json_charset_matrix() {
        assert_eq!(
            decode_json_bytes("{}".as_bytes(), None).expect("default utf-8"),
            "{}"
        );
        assert!(decode_json_bytes(b"\xff", None).is_err());
        assert_eq!(
            decode_json_bytes(b"caf\xe9", Some("latin-1")).expect("latin-1"),
            "caf\u{e9}"
        );
        let lookup = decode_json_bytes(b"{}", Some("utf-16")).expect_err("lookup");
        assert_eq!(lookup, "LOOKUP_ERROR");
        let response = map_decode_error(lookup);
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    // -- Python kernels --------------------------------------------------------

    #[test]
    fn truthiness_matches_python() {
        assert!(!py_truthy(&Value::Null));
        assert!(!py_truthy(&serde_json::json!(false)));
        assert!(py_truthy(&serde_json::json!(true)));
        assert!(!py_truthy(&serde_json::json!(0)));
        assert!(py_truthy(&serde_json::json!(5)));
        assert!(!py_truthy(&serde_json::json!("")));
        assert!(py_truthy(&serde_json::json!("x")));
        assert!(!py_truthy(&serde_json::json!([])));
        assert!(py_truthy(&serde_json::json!([0])));
        assert!(!py_truthy(&serde_json::json!({})));
        assert_eq!(py_len(&serde_json::json!("héllo")), Some(5));
        assert_eq!(py_len(&serde_json::json!([1, 2])), Some(2));
        assert_eq!(py_len(&serde_json::json!(5)), None);
        assert_eq!(py_len(&serde_json::json!(true)), None);
    }

    #[test]
    fn number_strings_match_python_repr() {
        assert_eq!(py_str(&serde_json::json!(5)), "5");
        assert_eq!(py_str(&serde_json::json!(-5)), "-5");
        assert_eq!(py_str(&serde_json::json!(5.5)), "5.5");
        assert_eq!(py_str(&serde_json::json!(1e20)), "1e+20");
        assert_eq!(py_str(&serde_json::json!(0.00001)), "1e-05");
        assert_eq!(py_str(&serde_json::json!(0.0001)), "0.0001");
        assert_eq!(py_str(&serde_json::json!(true)), "True");
        assert_eq!(py_str(&serde_json::json!(null)), "None");
        assert_eq!(py_str(&serde_json::json!([1, "a"])), "[1, 'a']");
        assert_eq!(py_str(&serde_json::json!({"a": 1})), "{'a': 1}");
        assert_eq!(py_repr(&str_value("it's")), "\"it's\"");
        assert_eq!(py_repr(&str_value("a\nb")), "'a\\nb'");
    }

    #[test]
    fn strip_matches_python_on_controls() {
        assert_eq!(py_strip("  x  "), "x");
        assert_eq!(py_strip("\u{1c}\u{85}\u{a0}x\u{85}"), "x");
        assert_eq!(py_strip("   "), "");
    }

    // -- DRF fields ------------------------------------------------------------

    #[test]
    fn char_field_matrix() {
        let missing = validate_char_field(InputValue::Missing, true, false, false, Some(80));
        assert!(matches!(missing, CharOutcome::Error(_)));
        let missing = validate_char_field(InputValue::Missing, false, false, false, Some(80));
        assert!(matches!(missing, CharOutcome::Skip));
        let null = validate_char_field(
            InputValue::Value(&Value::Null),
            false,
            false,
            false,
            Some(80),
        );
        assert!(matches!(null, CharOutcome::Error(_)));
        let blank = validate_char_field(
            InputValue::Value(&str_value("   ")),
            true,
            false,
            false,
            Some(80),
        );
        assert!(matches!(blank, CharOutcome::Error(_)));
        let coerced = validate_char_field(
            InputValue::Value(&serde_json::json!(123)),
            true,
            false,
            false,
            Some(80),
        );
        assert!(matches!(coerced, CharOutcome::Value(text) if text == "123"));
        let trimmed = validate_char_field(
            InputValue::Value(&str_value("  x  ")),
            true,
            false,
            false,
            Some(80),
        );
        assert!(matches!(trimmed, CharOutcome::Value(text) if text == "x"));
        let long = "n".repeat(81);
        let too_long = validate_char_field(
            InputValue::Value(&str_value(&long)),
            true,
            false,
            false,
            Some(80),
        );
        assert!(
            matches!(too_long, CharOutcome::Error(message) if message.contains("no more than 80"))
        );
        let nul = validate_char_field(
            InputValue::Value(&str_value("a\x00b")),
            true,
            false,
            false,
            Some(80),
        );
        assert!(
            matches!(nul, CharOutcome::Error(message) if message == "Null characters are not allowed.")
        );
        let boolean = validate_char_field(
            InputValue::Value(&serde_json::json!(true)),
            true,
            false,
            false,
            Some(80),
        );
        assert!(matches!(boolean, CharOutcome::Error(message) if message == "Not a valid string."));
        let file = validate_char_field(InputValue::File("a.png"), true, false, false, Some(80));
        assert!(matches!(file, CharOutcome::Error(message) if message == "Not a valid string."));
    }

    #[test]
    fn choice_field_has_no_trim_or_blank() {
        let zone = "America/New_York";
        assert!(PYTZ_COMMON_TIMEZONES.contains(&zone));
        let valid = validate_choice_field(InputValue::Value(&str_value(zone)), false);
        assert!(matches!(valid, CharOutcome::Value(_)));
        let padded = validate_choice_field(InputValue::Value(&str_value(" UTC")), false);
        assert!(
            matches!(padded, CharOutcome::Error(message) if message == "\" UTC\" is not a valid choice.")
        );
        let missing = validate_choice_field(InputValue::Missing, false);
        assert!(matches!(missing, CharOutcome::Skip));
    }

    #[test]
    fn datetime_inputs_attach_the_actor_zone() {
        let zone: Tz = "America/New_York".parse().expect("zone");
        let naive = validate_datetime_field(
            InputValue::Value(&str_value("2026-01-01T00:00:00")),
            false,
            true,
            &zone,
        );
        match naive {
            DateTimeOutcome::Value(instant) => {
                assert_eq!(instant.to_rfc3339(), "2026-01-01T05:00:00+00:00");
            }
            _ => panic!("naive parses"),
        }
        let aware = validate_datetime_field(
            InputValue::Value(&str_value("2026-01-01T00:00:00+02:00")),
            false,
            true,
            &zone,
        );
        match aware {
            DateTimeOutcome::Value(instant) => {
                assert_eq!(instant.to_rfc3339(), "2025-12-31T22:00:00+00:00");
            }
            _ => panic!("aware parses"),
        }
        let bad = validate_datetime_field(
            InputValue::Value(&str_value("not-a-date")),
            false,
            true,
            &zone,
        );
        assert!(matches!(bad, DateTimeOutcome::Error(message) if message == DATETIME_INVALID_MSG));
        let gap = validate_datetime_field(
            InputValue::Value(&str_value("2026-03-08T02:30:00")),
            false,
            true,
            &zone,
        );
        assert!(
            matches!(gap, DateTimeOutcome::Error(message) if message == "Invalid datetime for the timezone \"America/New_York\".")
        );
    }

    #[test]
    fn datetime_grammar_covers_verified_live_cases() {
        for text in [
            "2026-01-01T00:00:00",
            "2026-01-01 00:00:00",
            "2026-1-1T1:2",
            "2026-01-01T00:00:00.123456789",
            "2026-01-01T00:00:00,5",
            "20260101T000000",
            "2026-01-01T00:00:00Z",
            "2026-01-01T00:00:00+0200",
            "2026-01-01T00:00:00+02",
            "2026-01-01",
            "2026-01-01x00:00:00",
            "2026-01-01T00:00:00+23:59",
            "2026-W05-3",
        ] {
            assert!(parse_drf_datetime(text).is_some(), "{text} parses");
        }
        for text in [
            "2026-13-01T00:00:00",
            "2026-01-01T25:00:00",
            "2026-W5-3",
            "2026-01-01T00:00:00+24:00",
            "2026-01-01t00:00:00z",
            "2026/01/01",
            "not-a-date",
            "",
            "2026-024",
        ] {
            assert!(parse_drf_datetime(text).is_none(), "{text} rejects");
        }
        // Fractions truncate to micros, like CPython.
        match parse_drf_datetime("2026-01-01T00:00:00.123456789").expect("fraction") {
            ParsedDateTime::Naive(naive) => {
                assert_eq!(naive.and_utc().timestamp_subsec_micros(), 123456);
            }
            _ => panic!("naive"),
        }
    }

    #[test]
    fn datetime_multibyte_inputs_reject_without_panic() {
        // Multibyte chars at slicing positions reject (Django 400s), never panic.
        assert!(parse_drf_datetime("20260101Taéa").is_none());
        assert!(parse_drf_datetime("2026-W05-éa").is_none());
        assert!(parse_hms_basic("aéa").is_none());
        assert!(parse_week_datetime("2026-W05-éa").is_none());
    }

    #[test]
    fn pk_miss_echoes_original_string() {
        // UUID misses echo the pre-prep input verbatim (DRF `does_not_exist`
        // renders `pk_value=data`), never normalized lowercase.
        let upper = "AAAAAAAA-1111-1111-1111-111111111111";
        let candidate: uuid::Uuid = upper.parse().expect("uuid");
        assert_eq!(
            py_pk_display(&Value::String(upper.to_owned()), &candidate),
            upper
        );
        let urn = "urn:uuid:aaaaaaaa-1111-1111-1111-111111111111";
        let candidate: uuid::Uuid = urn.parse().expect("urn uuid");
        assert_eq!(
            py_pk_display(&Value::String(urn.to_owned()), &candidate),
            urn
        );
        assert_eq!(
            py_pk_display(&serde_json::json!(7), &uuid::Uuid::nil()),
            "7"
        );
    }

    #[test]
    fn theme_validation_matches_contract_bodies() {
        let zone = utc_zone();
        // `{}`: `deleted_at` first, then `name` (field order).
        let errors = validate_theme_input(&json_body(&[]), &zone, false)
            .expect("no transport")
            .expect_err("missing fields");
        assert_eq!(
            errors,
            vec![
                (
                    "deleted_at".to_owned(),
                    vec!["This field is required.".to_owned()]
                ),
                (
                    "name".to_owned(),
                    vec!["This field is required.".to_owned()]
                ),
            ]
        );
        // Name alone still needs `deleted_at` (uniqueness forcing).
        let errors = validate_theme_input(&json_body(&[("name", str_value("T"))]), &zone, false)
            .expect("no transport")
            .expect_err("deleted_at required");
        assert_eq!(
            errors,
            vec![(
                "deleted_at".to_owned(),
                vec!["This field is required.".to_owned()]
            )]
        );
        // Partial skips the forcing.
        let valid = validate_theme_input(&json_body(&[("name", str_value("T"))]), &zone, true)
            .expect("no transport")
            .expect("partial valid");
        assert_eq!(valid.name, Some("T".to_owned()));
        assert!(valid.deleted_at.is_none());
        // `colors` passes JSON through raw, rejects null.
        let valid = validate_theme_input(
            &json_body(&[
                ("name", str_value("T")),
                ("deleted_at", Value::Null),
                ("colors", str_value("x")),
            ]),
            &zone,
            false,
        )
        .expect("no transport")
        .expect("colors passthrough");
        assert_eq!(valid.colors, Some(str_value("x")));
        let errors = validate_theme_input(
            &json_body(&[
                ("name", str_value("T")),
                ("deleted_at", Value::Null),
                ("colors", Value::Null),
            ]),
            &zone,
            false,
        )
        .expect("no transport")
        .expect_err("colors null");
        assert_eq!(
            errors,
            vec![(
                "colors".to_owned(),
                vec!["This field may not be null.".to_owned()]
            )]
        );
    }

    #[test]
    fn theme_form_colors_parse_json_strings() {
        let zone = utc_zone();
        let mut map = Map::new();
        map.insert("name".to_owned(), str_value("T"));
        map.insert("deleted_at".to_owned(), str_value(""));
        map.insert("colors".to_owned(), str_value("{\"a\": 1}"));
        let body = InputBody {
            is_html: true,
            map,
            files: BTreeMap::new(),
            scalar: None,
        };
        let valid = validate_theme_input(&body, &zone, false)
            .expect("no transport")
            .expect("form valid");
        assert_eq!(valid.deleted_at, Some(None));
        assert_eq!(valid.colors, Some(serde_json::json!({"a": 1})));
    }

    // -- top-level bodies ------------------------------------------------------

    async fn response_text(response: Response) -> String {
        let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("body bytes");
        String::from_utf8(bytes.to_vec()).expect("body text")
    }

    #[tokio::test]
    async fn non_object_body_messages() {
        for (value, expected) in [
            (
                serde_json::json!(null),
                "{\"non_field_errors\":[\"No data provided\"]}",
            ),
            (
                serde_json::json!([1]),
                "{\"non_field_errors\":[\"Invalid data. Expected a dictionary, but got list.\"]}",
            ),
            (
                serde_json::json!("x"),
                "{\"non_field_errors\":[\"Invalid data. Expected a dictionary, but got str.\"]}",
            ),
            (
                serde_json::json!(5),
                "{\"non_field_errors\":[\"Invalid data. Expected a dictionary, but got int.\"]}",
            ),
            (
                serde_json::json!(5.5),
                "{\"non_field_errors\":[\"Invalid data. Expected a dictionary, but got float.\"]}",
            ),
            (
                serde_json::json!(true),
                "{\"non_field_errors\":[\"Invalid data. Expected a dictionary, but got bool.\"]}",
            ),
        ] {
            let response = non_object_body_response(&value);
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
            assert_eq!(response_text(response).await, expected);
        }
    }

    #[tokio::test]
    async fn method_not_allowed_names_the_method() {
        let response = method_not_allowed_response("GET");
        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
        assert_eq!(
            response_text(response).await,
            "{\"Detail\":\"Method \\\"GET\\\" not allowed.\"}"
        );
        let response = unsupported_media_type("text/csv");
        assert_eq!(response.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
        assert_eq!(
            response_text(response).await,
            "{\"Detail\":\"Unsupported media type \\\"text/csv\\\" in request.\"}"
        );
        // Attacker-controlled content types escape like the renderer.
        let response = unsupported_media_type("a\"b\\c");
        assert_eq!(response.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
        assert_eq!(
            response_text(response).await,
            "{\"Detail\":\"Unsupported media type \\\"a\\\"b\\\\c\\\" in request.\"}"
        );
        for (model, want) in [
            ("Workspace", "No Workspace matches the given query."),
            (
                "WorkspaceTheme",
                "No WorkspaceTheme matches the given query.",
            ),
        ] {
            let response = detail_not_found_response(model);
            assert_eq!(response.status(), StatusCode::NOT_FOUND);
            assert_eq!(
                response_text(response).await,
                format!("{{{:?}:{:?}}}", "Detail", want)
            );
        }
        let response = class_denied_response();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    // -- search ----------------------------------------------------------------

    #[test]
    fn search_terms_follow_smart_split() {
        assert_eq!(search_terms("Acme"), vec!["Acme".to_owned()]);
        assert_eq!(search_terms(""), Vec::<String>::new());
        assert_eq!(search_terms("  , ,"), Vec::<String>::new());
        assert_eq!(
            search_terms("foo,bar  baz"),
            vec!["foo".to_owned(), "bar".to_owned(), "baz".to_owned()]
        );
        assert_eq!(
            search_terms("\"foo bar\" baz"),
            vec!["foo bar".to_owned(), "baz".to_owned()]
        );
        assert_eq!(search_terms("'it\\'s'"), vec!["it's".to_owned()]);
        assert_eq!(escape_like("100%_x\\y"), "100\\%\\_x\\\\y");
    }

    // -- export ----------------------------------------------------------------

    #[test]
    fn export_date_parsing_matches_parse_date() {
        assert!(parse_export_date("2026-09-24").is_some());
        assert!(parse_export_date("2026-9-4").is_some());
        assert!(parse_export_date("20260924").is_some());
        assert!(parse_export_date("2026-13-01").is_none());
        assert!(parse_export_date("2026-02-30").is_none());
        assert!(parse_export_date("xyz").is_none());
        assert!(parse_export_date("").is_none());
        assert!(parse_export_date("2026-09-24T00:00").is_none());
        // Grammar gaps (every shape probed via `DateField.to_python`,
        // Django 4.2.30): dashed years 1-999 are valid ...
        assert!(parse_export_date("0001-01-01").is_some());
        assert!(parse_export_date("0001-1-1").is_some());
        assert!(parse_export_date("00010101").is_some());
        // ... 3-digit month/day segments are not ...
        assert!(parse_export_date("2026-007-04").is_none());
        assert!(parse_export_date("2026-07-004").is_none());
        assert!(parse_export_date("2026-1-111").is_none());
        // ... and year 0 is not, in either arm.
        assert!(parse_export_date("0000-01-01").is_none());
        assert!(parse_export_date("00000101").is_none());
        // Still rejected: non-4-digit years, `+` (Rust's int parser
        // would accept it, Django's `\d` never does), padding.
        assert!(parse_export_date("999-01-01").is_none());
        assert!(parse_export_date("10000-01-01").is_none());
        assert!(parse_export_date("+2026-01-01").is_none());
        assert!(parse_export_date("2026-+1-01").is_none());
        assert!(parse_export_date(" 2026-01-01").is_none());
        assert!(parse_export_date("2026-01-01 ").is_none());
        assert!(parse_export_date("2026-00-10").is_none());
        assert!(parse_export_date("2026-01-00").is_none());
    }

    #[test]
    fn csv_rendering_is_quote_all_crlf() {
        let zone = utc_zone();
        let created = chrono::DateTime::parse_from_rfc3339("2026-09-24T10:11:12.500000+00:00")
            .expect("csv instant")
            .with_timezone(&chrono::Utc);
        let rows: Vec<ExportRowTuple> = vec![(
            "=cmd|'/c calc'!A0".to_owned(),
            "ENG".to_owned(),
            "Pro\"ject".to_owned(),
            Some(7),
            created,
            created,
            "created".to_owned(),
            None,
            Some("-5".to_owned()),
            Some("a\"b".to_owned()),
        )];
        let csv = render_export_csv(&rows, &zone);
        let expected = concat!(
            "\"Actor name\",\"Issue ID\",\"Project\",\"Created at\",\"Updated at\",\"Action\",\"Field\",\"Old value\",\"New value\"\r\n",
            "\"'=cmd|'/c calc'!A0\",\"ENG - 7\",\"Pro\"\"ject\",\"2026-09-24 10:11:12.500000+00:00\",\"2026-09-24 10:11:12.500000+00:00\",\"created\",\"\",\"'-5\",\"a\"\"b\"\r\n",
        );
        assert_eq!(csv, expected);
    }

    #[test]
    fn csv_datetimes_render_utc_regardless_of_zone() {
        let zone: Tz = "America/New_York".parse().expect("zone");
        let instant = chrono::DateTime::parse_from_rfc3339("2026-01-01T05:00:00+00:00")
            .expect("instant")
            .with_timezone(&chrono::Utc);
        assert_eq!(
            format_csv_datetime(&instant, &zone),
            "2026-01-01 05:00:00+00:00"
        );
        assert_eq!(sanitize_csv_value("+1"), "'+1");
        assert_eq!(sanitize_csv_value("ok"), "ok");
    }

    // -- SQL assembly ------------------------------------------------------------

    #[test]
    fn placeholders_skip_casts_and_quotes() {
        let (sql, names) = positional_placeholders(
            "SELECT UPPER(name::TEXT) LIKE UPPER(:search) AND slug = :slug AND note = ':x' AND slug = :slug",
        );
        assert_eq!(
            sql,
            "SELECT UPPER(name::TEXT) LIKE UPPER($1) AND slug = $2 AND note = ':x' AND slug = $2"
        );
        assert_eq!(names, vec!["search".to_owned(), "slug".to_owned()]);
    }

    #[test]
    fn invalid_uuid_message_uses_curly_quotes() {
        assert_eq!(
            invalid_uuid_message("xyz"),
            "\u{201c}xyz\u{201d} is not a valid UUID."
        );
    }

    #[test]
    fn random_colors_match_hexdigits_shape() {
        for _ in 0..50 {
            let color = random_background_color();
            assert_eq!(color.len(), 7);
            assert!(color.starts_with('#'));
            assert!(color[1..]
                .bytes()
                .all(|byte| "0123456789abcdefABCDEF".contains(byte as char)));
        }
    }

    #[test]
    fn create_envelope_appends_annotations_last() {
        let zone = utc_zone();
        let instant = chrono::DateTime::parse_from_rfc3339("2026-01-01T00:00:00+00:00")
            .expect("instant")
            .with_timezone(&chrono::Utc);
        let id = uuid::Uuid::nil();
        let data = WorkspaceRowData {
            id,
            created_at: instant,
            updated_at: instant,
            deleted_at: None,
            name: "Acme".to_owned(),
            logo: None,
            slug: "acme".to_owned(),
            organization_size: None,
            timezone: "UTC".to_owned(),
            background_color: "#FF0000".to_owned(),
            created_by_id: Some(id),
            updated_by_id: None,
            logo_asset_id: None,
            owner_id: id,
            total_members: None,
            role: None,
        };
        let mut rendered = render_workspace_data(&data, None, &zone).expect("render");
        let keys_before: Vec<String> = rendered
            .as_object()
            .expect("object")
            .keys()
            .cloned()
            .collect();
        assert!(!keys_before.contains(&"total_members".to_owned()));
        assert!(!keys_before.contains(&"role".to_owned()));
        rendered
            .as_object_mut()
            .expect("object")
            .insert("total_members".to_owned(), Value::from(1));
        rendered
            .as_object_mut()
            .expect("object")
            .insert("role".to_owned(), Value::from(20));
        let keys: Vec<String> = rendered
            .as_object()
            .expect("object")
            .keys()
            .cloned()
            .collect();
        assert_eq!(keys[keys.len() - 2], "total_members");
        assert_eq!(keys[keys.len() - 1], "role");
    }

    #[test]
    fn detail_rows_carry_total_members_without_role() {
        let zone = utc_zone();
        let instant = chrono::DateTime::parse_from_rfc3339("2026-01-01T00:00:00+00:00")
            .expect("instant")
            .with_timezone(&chrono::Utc);
        let id = uuid::Uuid::nil();
        let data = WorkspaceRowData {
            id,
            created_at: instant,
            updated_at: instant,
            deleted_at: None,
            name: "Acme".to_owned(),
            logo: None,
            slug: "acme".to_owned(),
            organization_size: None,
            timezone: "UTC".to_owned(),
            background_color: "#FF0000".to_owned(),
            created_by_id: Some(id),
            updated_by_id: None,
            logo_asset_id: None,
            owner_id: id,
            total_members: Some(3),
            role: None,
        };
        let rendered = render_workspace_data(&data, None, &zone).expect("render");
        let object = rendered.as_object().expect("object");
        assert_eq!(object["total_members"], serde_json::json!(3));
        assert!(!object.contains_key("role"));
    }
}
