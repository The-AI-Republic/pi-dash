#![forbid(unsafe_code)]

//! Project invitations / join / favorites / deploy-board handlers (D-25 L11, PIDASHCONV-573).
//!
//! Ports `apps/api/pi_dash/app/views/project/invite.py:37-254`
//! (`ProjectInvitationsViewset`, `UserProjectInvitationsViewset`,
//! `ProjectJoinEndpoint`) and the favorites + deploy-board slice of
//! `apps/api/pi_dash/app/views/project/base.py:503-581`
//! (`ProjectFavoritesViewSet`, `DeployBoardViewSet`) onto the merged L2 /
//! L4 / L5 / L6 / L7 / L8 kernels. Routes are 5-9 + 13-14 of
//! `apps/api/pi_dash/app/urls/project.py:52-76,102-121`:
//!
//! * `GET+POST .../projects/<project_id>/invitations/` and
//!   `GET+DELETE .../invitations/<uuid:pk>/`
//! * `GET+POST users/me/workspaces/<slug>/projects/invitations/`
//! * `GET+POST .../projects/<project_id>/join/<uuid:pk>/` (`AllowAny`)
//! * `GET+POST .../user-favorite-projects/` and
//!   `DELETE .../user-favorite-projects/<project_id>/`
//! * `GET+POST .../projects/<project_id>/project-deploy-boards/` and
//!   `GET+PATCH+DELETE .../project-deploy-boards/<uuid:pk>/`
//!
//! Shape of the port (translate, don't redesign):
//!
//! * Route registration is the cutover granularity (Porting guide cutover
//!   row, `app_issues::routes` precedent): owned methods serve Rust, every
//!   other method falls through to [`crate::edge::proxy`] so Django answers
//!   the 405s (`Method "PUT" not allowed`), OPTIONS metadata and resolver
//!   404s exactly as before. Non-UUID `<uuid:pk>` tails proxy too (the
//!   `app_cycles` precedent): Django's converter rejects them at routing
//!   time, before any view logic.
//! * Order per request mirrors DRF `initial()` + the view: session auth
//!   (401), the `project_id` rewrite for authenticated callers only (404
//!   `{"Detail":"Project not found"}` on a miss — the rewrite runs even on
//!   the `AllowAny` join routes, where authentication still runs), then the
//!   L7 gate (403), then body parsing (415 / axum rejection; empty body is
//!   `{}` per the cycles live probe), then the handler.
//! * Reads render through the L2/L1 kernels
//!   (`ser_member::invite_to_representation`, `ser_project::DeployBoardRead`)
//!   with datetimes in the caller's zone (`crate::serializer`, `Z` rewrite)
//!   and asset URLs resolved like the api-v1 member port.
//! * Writes mirror the model `save()` side effects: the join-accept
//!   `ProjectMember.create` also inserts its `ProjectUserProperty` row
//!   (`min - 10000`, `db/models/project.py:348-364`); favorite create takes
//!   `max(sequence) + 10000` and backfills the workspace from the project;
//!   board create is the unscoped `get_or_create` plus a second save (so
//!   `updated_by` is always stamped); soft destroys stamp `deleted_at` +
//!   `updated_by` and publish `soft_delete_related_objects` best-effort.
//!   Statements run in Django's autocommit order — no explicit transaction
//!   (the settings carry no `ATOMIC_REQUESTS`).
//! * Errors render through the L8 table (`tasks::handle_exception`): the
//!   rewrite miss and `get_object()` misses are lowercase-`Detail` 404s,
//!   `.get()` misses in custom actions are the `error` 404, field-prep
//!   failures (bad UUIDs, bad bools) are the `ValidationError` 400, and
//!   `23xxx` write failures are the `IntegrityError` 400.
//!
//! Ported bugs and quirks (translate, don't redesign; also listed in the PR):
//!
//! * Invite create always 500s on a non-empty payload: `invite.py:62`
//!   reads `.role` off a queryset (`AttributeError`, before `bulk_create`,
//!   so 0 rows persist) and the `.delay`-on-a-list at `:104` is unreachable
//!   (FX-APROJ-08). The role-mismatch 400 inside that loop is dead with it.
//! * Favorites list 500s: the viewset has no `serializer_class`, so the
//!   DRF-default list raises `AssertionError` (FX-APROJ-06/08).
//! * Board retrieve / partial_update / destroy look the row up globally:
//!   `DeployBoardViewSet` defines no `get_queryset`, so any live board UUID
//!   resolves regardless of workspace or project.
//! * Join accept looks the existing project membership up by workspace only
//!   (no project predicate, `invite.py:221-223`), and the reactivate branch
//!   re-saves the unchanged role (`:233`).
//! * `accepted` / the board flags coerce through Django's
//!   `BooleanField.to_python` (`1`/`0`/`"t"`/`"True"`/… pass; anything else
//!   is the `ValidationError` 400; explicit `null` falls to the NOT NULL
//!   `IntegrityError` 400).
//! * Board PATCH validates through the serializer: unknown and read-only
//!   keys are ignored, every field error is collected into one 400 dict,
//!   and a non-dict body answers `non_field_errors` (with Python type
//!   names); a bad UUID inside a PK field is Django's curly-quote field
//!   error (DRF converts the escaped `ValidationError`).
//! * Board create 500s on any non-null `intake`: the FK-descriptor
//!   assignment only accepts an `Intake` instance or `None`.
//!
//! Fixture ids: FX-APROJ-09 (`rust-api/fixtures/app_project/`
//! `FX-APROJ-09.handlers_project.json` + `TRACE.md`); the 500 pins live in
//! FX-APROJ-08, the gate rows in FX-APROJ-07.
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

use std::collections::HashMap;

use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get};
use axum::Router;
use chrono_tz::Tz;
use serde_json::{Map, Value};
use sqlx::Row;

use crate::app_project::gates::{
    decide_gate, decide_project_member_gate, gate_for, tenant_context, GateOutcome, ANON_BODY,
    CLASS_DENIED_BODY, FORBIDDEN_BODY,
};
use crate::license::{resolve_actor, Actor};
use crate::middleware::SessionHandle;
use crate::state::AppState;

use pidash_auth::permissions::allow::AllowFacts;
use pidash_auth::permissions::project::ProjectFacts;
use pidash_auth::permissions::{ROLE_ADMIN, ROLE_MEMBER};
use pidash_services::app_project::ser_member::{
    invite_to_representation, project_lite_to_representation, resolve_project_cover_image_url,
    resolve_workspace_logo_url, workspace_lite_to_representation, ProjectLiteRow,
    ProjectMemberInviteRow, WorkspaceLiteRow,
};
use pidash_services::app_project::ser_project::{DeployBoardRead, DEPLOY_BOARD_INITIAL_BODY};
use pidash_services::app_project::tasks::{
    INTEGRITY_ERROR_BODY, OBJECT_NOT_FOUND_BODY, SERVER_ERROR_BODY, VALIDATION_ERROR_BODY,
};
use pidash_types::{ProjectId, WorkspaceId};

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------

/// Register the eight invite / join / favorite / deploy-board paths. Owned
/// methods serve from Rust; every other method proxies to Django (its 405s
/// and OPTIONS metadata live there). Sibling D-25 handler issues merge
/// their routers into `super::routes()`; on rebase keep both sides.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/workspaces/{slug}/projects/{project_id}/invitations/",
            owned(get(invite_list).post(invite_create), &["GET", "POST"]),
        )
        .route(
            "/api/workspaces/{slug}/projects/{project_id}/invitations/{pk}/",
            owned(
                get(invite_retrieve).delete(invite_destroy),
                &["GET", "DELETE"],
            ),
        )
        .route(
            "/api/users/me/workspaces/{slug}/projects/invitations/",
            owned(
                get(user_invite_list).post(user_invite_create),
                &["GET", "POST"],
            ),
        )
        .route(
            "/api/workspaces/{slug}/projects/{project_id}/join/{pk}/",
            owned(get(join_get).post(join_post), &["GET", "POST"]),
        )
        .route(
            "/api/workspaces/{slug}/user-favorite-projects/",
            owned(get(fav_list).post(fav_create), &["GET", "POST"]),
        )
        .route(
            "/api/workspaces/{slug}/user-favorite-projects/{project_id}/",
            owned(delete(fav_destroy), &["DELETE"]),
        )
        .route(
            "/api/workspaces/{slug}/projects/{project_id}/project-deploy-boards/",
            owned(get(board_list).post(board_create), &["GET", "POST"]),
        )
        .route(
            "/api/workspaces/{slug}/projects/{project_id}/project-deploy-boards/{pk}/",
            owned(
                get(board_retrieve)
                    .patch(board_partial_update)
                    .delete(board_destroy),
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

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// `Project.resolve` miss on a project-scoped route: the
/// `Http404("Project not found")` message survives DRF's `Http404`
/// conversion verbatim, lowercase `Detail`, compact rendering
/// (FX-APROJ-08 `handle_exception_viewset.DjangoHttp404`, live shell
/// probe — not the bare DRF default).
const RESOLVE_NOT_FOUND_BODY: &str = r#"{"detail":"Project not found"}"#;
/// DRF-default `get_object()` miss on the invite queryset: Django's
/// `_get_object_or_404` raises `Http404` carrying `No <Model> matches the
/// given query.`, which DRF renders verbatim (live probe 2026-10-03 —
/// not the bare `NotFound` default).
const INVITE_NOT_FOUND_BODY: &str =
    r#"{"detail":"No ProjectMemberInvite matches the given query."}"#;
/// DRF-default `get_object()` miss on the board queryset (same
/// mechanism, live probe 2026-10-03).
const BOARD_NOT_FOUND_BODY: &str = r#"{"detail":"No DeployBoard matches the given query."}"#;
/// `ProjectInvitationsViewset.create` with a missing or empty `emails`
/// (`invite.py:58-59`).
const EMAILS_REQUIRED_BODY: &str = r#"{"error":"Emails are required"}"#;
/// `ProjectJoinEndpoint.post` accept (`invite.py:236-239`).
const JOIN_ACCEPTED_BODY: &str = r#"{"message":"Project Invitation Accepted"}"#;
/// `ProjectJoinEndpoint.post` decline (`invite.py:241-244`).
const JOIN_DECLINED_BODY: &str = r#"{"message":"Project Invitation was not accepted"}"#;
/// `UserProjectInvitationsViewset.create` success (`invite.py:180`).
const PROJECTS_JOINED_BODY: &str = r#"{"message":"Projects joined successfully"}"#;
/// `ProjectJoinEndpoint.post` email mismatch (`invite.py:191-195`).
const WRONG_EMAIL_BODY: &str = r#"{"error":"You do not have permission to join the project"}"#;
/// `ProjectJoinEndpoint.post` repeat response (`invite.py:246-249`).
const ALREADY_RESPONDED_BODY: &str =
    r#"{"error":"You have already responded to the invitation request"}"#;
/// `UserProjectInvitationsViewset.create` SECRET-project guard
/// (`invite.py:139-143`).
const SECRET_JOIN_BODY: &str = r#"{"error":"Only workspace admins can join private project"}"#;

/// Handler failure with its exact status + body.
#[derive(Debug)]
enum Denial {
    /// 401, DRF `NotAuthenticated`.
    Unauthorized,
    /// 403, `@allow_permission` body.
    Forbidden,
    /// 403, permission-class body (`ProjectMemberPermission`).
    ClassDenied,
    /// 404, `{"Detail":"Project not found"}` (identifier-rewrite miss).
    ResolveNotFound,
    /// 404, invite `get_object()` miss (Django's `No ... matches` message).
    InviteNotFound,
    /// 404, board `get_object()` miss (Django's `No ... matches` message).
    BoardNotFound,
    /// 404, `ObjectDoesNotExist` branch (`.get()` in custom actions).
    ObjectNotFound,
    /// 400, `ValidationError` branch (field-prep failures).
    BadValidation,
    /// 400, `IntegrityError` branch (write constraint failures).
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
            Denial::Unauthorized => (StatusCode::UNAUTHORIZED, ANON_BODY.to_owned()),
            Denial::Forbidden => (StatusCode::FORBIDDEN, FORBIDDEN_BODY.to_owned()),
            Denial::ClassDenied => (StatusCode::FORBIDDEN, CLASS_DENIED_BODY.to_owned()),
            Denial::ResolveNotFound => (StatusCode::NOT_FOUND, RESOLVE_NOT_FOUND_BODY.to_owned()),
            Denial::InviteNotFound => (StatusCode::NOT_FOUND, INVITE_NOT_FOUND_BODY.to_owned()),
            Denial::BoardNotFound => (StatusCode::NOT_FOUND, BOARD_NOT_FOUND_BODY.to_owned()),
            Denial::ObjectNotFound => (StatusCode::NOT_FOUND, OBJECT_NOT_FOUND_BODY.to_owned()),
            Denial::BadValidation => (StatusCode::BAD_REQUEST, VALIDATION_ERROR_BODY.to_owned()),
            Denial::BadPayload => (StatusCode::BAD_REQUEST, INTEGRITY_ERROR_BODY.to_owned()),
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
        .expect("invite response")
}

fn empty_response(status: StatusCode) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::empty())
        .expect("empty invite response")
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).expect("json string")
}

/// Map a gate outcome to its denial. `Allow` is unreachable here; the
/// caller returns early on it.
fn gate_denial(outcome: GateOutcome, class_gate: bool) -> Denial {
    match outcome {
        GateOutcome::Allow => Denial::ServerError,
        GateOutcome::Deny => {
            if class_gate {
                Denial::ClassDenied
            } else {
                Denial::Forbidden
            }
        }
        GateOutcome::Unauthenticated => Denial::Unauthorized,
    }
}

/// Whether a sqlx failure is an integrity-constraint (`23xxx`) failure.
fn is_integrity_error(err: &sqlx::Error) -> bool {
    match err {
        sqlx::Error::Database(db) => db
            .code()
            .as_deref()
            .is_some_and(|code| code.starts_with("23")),
        _ => false,
    }
}

/// `handle_exception` `IntegrityError` → 400 `The payload is not valid`:
/// any `23xxx` database failure on a write. Anything else is the generic
/// 500 (Django's `DataError` included).
fn integrity_denial(err: sqlx::Error) -> Denial {
    if is_integrity_error(&err) {
        Denial::BadPayload
    } else {
        Denial::ServerError
    }
}

// ---------------------------------------------------------------------------
// Request context: auth + tenant + membership
// ---------------------------------------------------------------------------

/// Session auth (`BaseSessionAuthentication` + `IsAuthenticated`):
/// anonymous answers the DRF `NotAuthenticated` body before anything else
/// runs — including before the project-identifier rewrite, so the
/// slug-existence oracle stays closed.
async fn actor(
    state: &AppState,
    extension: Option<axum::Extension<SessionHandle>>,
) -> Result<Actor, Denial> {
    let pool = pool_of(state)?;
    resolve_actor(pool, state.settings().secret_key.as_bytes(), extension)
        .await
        .map_err(|_| Denial::ServerError)?
        .ok_or(Denial::Unauthorized)
}

/// Session auth for the `AllowAny` join routes: authentication still runs
/// (so the rewrite and the crum stamps see the caller), but anonymous
/// callers reach the body.
async fn optional_actor(
    state: &AppState,
    extension: Option<axum::Extension<SessionHandle>>,
) -> Result<Option<Actor>, Denial> {
    let pool = pool_of(state)?;
    resolve_actor(pool, state.settings().secret_key.as_bytes(), extension)
        .await
        .map_err(|_| Denial::ServerError)
}

fn pool_of(state: &AppState) -> Result<&sqlx::PgPool, Denial> {
    state
        .pools()
        .map(|pools| pools.primary())
        .ok_or(Denial::ServerError)
}

/// `_rewrite_project_kwarg` (`app/views/base.py:52-81`): UUIDs pass through
/// unchecked (the row check happens in the view body); anything else
/// matches the stripped-upper `identifier` in the workspace; misses raise
/// `Http404("Project not found")`. Only ever called for authenticated
/// callers — anonymous skips the rewrite.
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
           WHERE w.slug = $1 AND p.identifier = $2 AND p.deleted_at IS NULL
           ORDER BY p.created_at DESC LIMIT 1"#,
    )
    .bind(slug)
    .bind(upper)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    row.map(|row| row.0).ok_or(Denial::ResolveNotFound)
}

/// Membership roles for one `(slug, user)`: the workspace role plus every
/// live project role in the workspace. Two statements; the gates derive
/// scoped and unscoped facts from them. Forward-FK joins carry no
/// related-manager filter, so no `workspaces.deleted_at` guard — same SQL
/// semantics as Django.
async fn membership(
    pool: &sqlx::PgPool,
    slug: &str,
    user_id: &uuid::Uuid,
) -> Result<(Option<i16>, Vec<(uuid::Uuid, i16)>), Denial> {
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
    let project_roles: Vec<(uuid::Uuid, Option<i16>)> = sqlx::query_as(
        r#"SELECT pm.project_id, pm.role FROM project_members pm
           JOIN workspaces w ON w.id = pm.workspace_id
           WHERE w.slug = $1 AND pm.member_id = $2 AND pm.is_active AND pm.deleted_at IS NULL"#,
    )
    .bind(slug)
    .bind(user_id)
    .fetch_all(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let project_roles = project_roles
        .into_iter()
        .filter_map(|(id, role)| role.map(|role| (id, role)))
        .collect();
    Ok((workspace_role.and_then(|row| row.0), project_roles))
}

/// Build the [`AllowFacts`] for one decorator gate from the membership rows.
fn allow_facts(
    slug: &str,
    allowed: &[i32],
    workspace_role: Option<i16>,
    project_role: Option<i16>,
) -> AllowFacts {
    let workspace_role = workspace_role.map(i32::from);
    let project_role = project_role.map(i32::from);
    AllowFacts {
        workspace: WorkspaceId::from(slug),
        authenticated: true,
        is_workspace_member: workspace_role.is_some(),
        has_allowed_workspace_role: workspace_role.is_some_and(|role| allowed.contains(&role)),
        is_creator: false,
        has_allowed_project_role: project_role.is_some_and(|role| allowed.contains(&role)),
        is_project_member: project_role.is_some(),
        is_workspace_admin: workspace_role == Some(ROLE_ADMIN),
    }
}

/// Check one decorator/default `GATES` row: fetch membership, decide, map
/// the outcome. Class gates never reach here — boards use
/// [`check_board_gate`].
async fn check_gate(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: Option<&uuid::Uuid>,
    user_id: &uuid::Uuid,
    method: &str,
    path: &str,
) -> Result<(), Denial> {
    let route = gate_for(method, path).ok_or(Denial::ServerError)?;
    let allowed: &[i32] = match &route.gate {
        crate::app_project::gates::Gate::Authenticated => &[],
        crate::app_project::gates::Gate::Workspace { roles } => roles,
        crate::app_project::gates::Gate::Project { roles } => roles,
        _ => return Err(Denial::ServerError),
    };
    let (workspace_role, project_roles) = membership(pool, slug, user_id).await?;
    let project_role = project_id.and_then(|id| {
        project_roles
            .iter()
            .find(|(pid, _)| pid == id)
            .map(|(_, role)| *role)
    });
    let scope = tenant_context(slug);
    let facts = allow_facts(slug, allowed, workspace_role, project_role);
    match decide_gate(&route.gate, &scope, &facts) {
        GateOutcome::Allow => Ok(()),
        outcome => Err(gate_denial(outcome, false)),
    }
}

/// Check one `ProjectMemberPermission` row (deploy boards): safe methods
/// need any live project row in the workspace (**no `project_id` filter**,
/// `project.py:63-65`); POST needs workspace Admin/Member; anything else
/// needs project Admin/Member.
async fn check_board_gate(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    user_id: &uuid::Uuid,
    method: &str,
) -> Result<(), Denial> {
    let (workspace_role, project_roles) = membership(pool, slug, user_id).await?;
    let ws = workspace_role.map(i32::from);
    let scoped: Vec<i32> = project_roles
        .iter()
        .filter(|(pid, _)| pid == project_id)
        .map(|(_, role)| i32::from(*role))
        .collect();
    let facts = ProjectFacts {
        workspace: WorkspaceId::from(slug),
        project_id: ProjectId::from(project_id.to_string()),
        authenticated: true,
        is_workspace_member: ws.is_some(),
        has_workspace_admin_or_member: ws
            .is_some_and(|role| role == ROLE_ADMIN || role == ROLE_MEMBER),
        is_workspace_admin: ws == Some(ROLE_ADMIN),
        is_project_member: !project_roles.is_empty(),
        is_project_admin: scoped.contains(&ROLE_ADMIN),
        has_project_admin_or_member: scoped
            .iter()
            .any(|role| *role == ROLE_ADMIN || *role == ROLE_MEMBER),
        has_identifier_membership: false,
        has_project_identifier: false,
    };
    let scope = tenant_context(slug);
    match decide_project_member_gate(method, &scope, &facts) {
        GateOutcome::Allow => Ok(()),
        outcome => Err(gate_denial(outcome, true)),
    }
}

/// Bodies parse after the gate (the handler builds on `request.data`):
/// an empty body is `{}` (DRF never raises `ParseError` on it — the
/// cycles live probe); a non-empty body without a JSON content type
/// answers the `UnsupportedMediaType` 415 naming the content type
/// (`text/plain` when missing); otherwise axum's `Json` rejection bytes
/// apply (malformed JSON stays DRF-unmatched — its messages carry parser
/// positions serde cannot reproduce — documented, never hit by the suite).
#[allow(clippy::result_large_err)]
async fn parse_body(state: &AppState, req: axum::extract::Request) -> Result<Value, Response> {
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
        // DRF `UnsupportedMediaType` (`detail`, lowercase; the media
        // type quotes are part of the message — live probe 2026-10-03).
        let message = format!("Unsupported media type \"{named}\" in request.");
        let body = format!("{{\"detail\":{}}}", json_string(&message));
        return Err(Denial::Raw(StatusCode::UNSUPPORTED_MEDIA_TYPE, body).into_response());
    }
    let req = axum::extract::Request::from_parts(parts, axum::body::Body::from(bytes));
    match axum::Json::<Value>::from_request(req, state).await {
        Ok(axum::Json(body)) => Ok(body),
        Err(rejection) => Err(rejection.into_response()),
    }
}

// ---------------------------------------------------------------------------
// Field prep: `UUIDField` / `BooleanField` `to_python`
// ---------------------------------------------------------------------------

/// `UUIDField.to_python` (`django/db/models/fields/__init__.py`): ints
/// take the `int=` form (bools are ints), strings the `hex=` form
/// (hyphenated, simple, braced and URN spellings — exactly what
/// `Uuid::parse_str` accepts); anything else is the `ValidationError` 400.
fn prep_uuid(value: &Value) -> Result<uuid::Uuid, Denial> {
    match value {
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                if int < 0 {
                    return Err(Denial::BadValidation);
                }
                Ok(uuid::Uuid::from_u128(int as u128))
            } else if let Some(uint) = number.as_u64() {
                Ok(uuid::Uuid::from_u128(uint as u128))
            } else {
                Err(Denial::BadValidation)
            }
        }
        Value::String(raw) => raw.parse::<uuid::Uuid>().map_err(|_| Denial::BadValidation),
        Value::Bool(flag) => Ok(uuid::Uuid::from_u128(u128::from(*flag as u8))),
        _ => Err(Denial::BadValidation),
    }
}

/// `BooleanField.to_python` (`null=False`,
/// `django/db/models/fields/__init__.py`): `True`/`False` (and anything
/// `==`-equal: `1`/`0`, `1.0`/`0.0`) coerce, `"t"`/`"True"`/`"1"` pass,
/// `"f"`/`"False"`/`"0"` fail; `None` stays `None` (the NOT NULL column
/// then raises the `IntegrityError` 400); anything else is the
/// `ValidationError` 400.
fn prep_bool(value: &Value) -> Result<Option<bool>, Denial> {
    match value {
        Value::Null => Ok(None),
        Value::Bool(flag) => Ok(Some(*flag)),
        Value::Number(number) => {
            if number.as_i64() == Some(1) || number.as_u64() == Some(1) {
                Ok(Some(true))
            } else if number.as_i64() == Some(0) || number.as_u64() == Some(0) {
                Ok(Some(false))
            } else if number.as_f64() == Some(1.0) {
                Ok(Some(true))
            } else if number.as_f64() == Some(0.0) {
                Ok(Some(false))
            } else {
                Err(Denial::BadValidation)
            }
        }
        Value::String(raw) => match raw.as_str() {
            "t" | "True" | "1" => Ok(Some(true)),
            "f" | "False" | "0" => Ok(Some(false)),
            _ => Err(Denial::BadValidation),
        },
        _ => Err(Denial::BadValidation),
    }
}

// ---------------------------------------------------------------------------
// Assets
// ---------------------------------------------------------------------------

/// One `file_assets` row's `asset_url` (`db/models/asset.py:80-100`).
struct AssetRef {
    id: uuid::Uuid,
    entity_type: Option<String>,
    workspace_slug: Option<String>,
    project_id: Option<uuid::Uuid>,
    issue_id: Option<uuid::Uuid>,
}

/// Render `FileAsset.asset_url` for one asset row (the api-v1 member
/// port's transcription).
fn render_asset_url(asset: &AssetRef) -> Option<String> {
    match asset.entity_type.as_deref() {
        Some("WORKSPACE_LOGO")
        | Some("USER_AVATAR")
        | Some("USER_COVER")
        | Some("PROJECT_COVER") => Some(format!("/api/assets/v2/static/{}/", asset.id)),
        Some("ISSUE_ATTACHMENT") => Some(format!(
            "/api/assets/v2/workspaces/{}/projects/{}/issues/{}/attachments/{}/",
            asset.workspace_slug.as_deref().unwrap_or(""),
            asset
                .project_id
                .map(|id| id.to_string())
                .unwrap_or_default(),
            asset.issue_id.map(|id| id.to_string()).unwrap_or_default(),
            asset.id,
        )),
        Some("ISSUE_DESCRIPTION")
        | Some("COMMENT_DESCRIPTION")
        | Some("PAGE_DESCRIPTION")
        | Some("DRAFT_ISSUE_DESCRIPTION") => Some(format!(
            "/api/assets/v2/workspaces/{}/projects/{}/{}/",
            asset.workspace_slug.as_deref().unwrap_or(""),
            asset
                .project_id
                .map(|id| id.to_string())
                .unwrap_or_default(),
            asset.id,
        )),
        _ => None,
    }
}

/// Fetch `asset_url` values for a set of asset ids (one statement; the
/// api-v1 member port's shape). A dangling id maps to no URL — Django
/// would raise on the forward-FK fetch, but ids always resolve in
/// practice (same settled approximation as the api-v1 port).
async fn fetch_asset_urls(
    pool: &sqlx::PgPool,
    asset_ids: &[uuid::Uuid],
) -> Result<HashMap<uuid::Uuid, String>, Denial> {
    let mut out = HashMap::new();
    if asset_ids.is_empty() {
        return Ok(out);
    }
    let placeholders: Vec<String> = (1..=asset_ids.len()).map(|i| format!("${i}")).collect();
    let sql = format!(
        "SELECT a.id, a.entity_type, w.slug AS workspace_slug, a.project_id, a.issue_id
         FROM file_assets a LEFT JOIN workspaces w ON w.id = a.workspace_id
         WHERE a.id IN ({}) AND a.deleted_at IS NULL",
        placeholders.join(", ")
    );
    let mut query = sqlx::query(&sql);
    for id in asset_ids {
        query = query.bind(*id);
    }
    let rows = query
        .fetch_all(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    for row in rows {
        let asset = AssetRef {
            id: row.try_get("id").map_err(|_| Denial::ServerError)?,
            entity_type: row
                .try_get("entity_type")
                .map_err(|_| Denial::ServerError)?,
            workspace_slug: row
                .try_get("workspace_slug")
                .map_err(|_| Denial::ServerError)?,
            project_id: row.try_get("project_id").map_err(|_| Denial::ServerError)?,
            issue_id: row.try_get("issue_id").map_err(|_| Denial::ServerError)?,
        };
        if let Some(url) = render_asset_url(&asset) {
            out.insert(asset.id, url);
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Invite reads
// ---------------------------------------------------------------------------

/// One invite row with its nested lite columns: the
/// `select_related("project", "workspace")` half of the invite querysets
/// (`invite.py:49-50,125`). Forward-FK joins carry no related-manager
/// filter — a soft-deleted project or workspace still renders.
struct InviteRow {
    id: uuid::Uuid,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
    deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    email: String,
    accepted: bool,
    token: String,
    message: Option<String>,
    responded_at: Option<chrono::DateTime<chrono::Utc>>,
    role: i16,
    created_by_id: Option<uuid::Uuid>,
    updated_by_id: Option<uuid::Uuid>,
    project_id: uuid::Uuid,
    project_identifier: String,
    project_name: String,
    project_cover_image: Option<String>,
    project_cover_asset_id: Option<uuid::Uuid>,
    project_logo_props: Value,
    project_description: String,
    project_is_default: bool,
    workspace_id: uuid::Uuid,
    workspace_name: String,
    workspace_slug: String,
    workspace_logo: Option<String>,
    workspace_logo_asset_id: Option<uuid::Uuid>,
}

const INVITE_SELECT: &str = "i.id, i.created_at, i.updated_at, i.deleted_at, i.email, i.accepted, \
     i.token, i.message, i.responded_at, i.role, i.created_by_id, i.updated_by_id, \
     p.id AS project_id, p.identifier AS project_identifier, p.name AS project_name, \
     p.cover_image AS project_cover_image, p.cover_image_asset_id AS project_cover_asset_id, \
     p.logo_props AS project_logo_props, p.description AS project_description, \
     p.is_default AS project_is_default, \
     w.id AS workspace_id, w.name AS workspace_name, w.slug AS workspace_slug, \
     w.logo AS workspace_logo, w.logo_asset_id AS workspace_logo_asset_id \
     FROM project_member_invites i \
     JOIN projects p ON p.id = i.project_id \
     JOIN workspaces w ON w.id = i.workspace_id";

fn invite_row(row: &sqlx::postgres::PgRow) -> Result<InviteRow, Denial> {
    Ok(InviteRow {
        id: row.try_get("id").map_err(|_| Denial::ServerError)?,
        created_at: row.try_get("created_at").map_err(|_| Denial::ServerError)?,
        updated_at: row.try_get("updated_at").map_err(|_| Denial::ServerError)?,
        deleted_at: row.try_get("deleted_at").map_err(|_| Denial::ServerError)?,
        email: row.try_get("email").map_err(|_| Denial::ServerError)?,
        accepted: row.try_get("accepted").map_err(|_| Denial::ServerError)?,
        token: row.try_get("token").map_err(|_| Denial::ServerError)?,
        message: row.try_get("message").map_err(|_| Denial::ServerError)?,
        responded_at: row
            .try_get("responded_at")
            .map_err(|_| Denial::ServerError)?,
        role: row.try_get("role").map_err(|_| Denial::ServerError)?,
        created_by_id: row
            .try_get("created_by_id")
            .map_err(|_| Denial::ServerError)?,
        updated_by_id: row
            .try_get("updated_by_id")
            .map_err(|_| Denial::ServerError)?,
        project_id: row.try_get("project_id").map_err(|_| Denial::ServerError)?,
        project_identifier: row
            .try_get("project_identifier")
            .map_err(|_| Denial::ServerError)?,
        project_name: row
            .try_get("project_name")
            .map_err(|_| Denial::ServerError)?,
        project_cover_image: row
            .try_get("project_cover_image")
            .map_err(|_| Denial::ServerError)?,
        project_cover_asset_id: row
            .try_get("project_cover_asset_id")
            .map_err(|_| Denial::ServerError)?,
        project_logo_props: row
            .try_get("project_logo_props")
            .map_err(|_| Denial::ServerError)?,
        project_description: row
            .try_get("project_description")
            .map_err(|_| Denial::ServerError)?,
        project_is_default: row
            .try_get("project_is_default")
            .map_err(|_| Denial::ServerError)?,
        workspace_id: row
            .try_get("workspace_id")
            .map_err(|_| Denial::ServerError)?,
        workspace_name: row
            .try_get("workspace_name")
            .map_err(|_| Denial::ServerError)?,
        workspace_slug: row
            .try_get("workspace_slug")
            .map_err(|_| Denial::ServerError)?,
        workspace_logo: row
            .try_get("workspace_logo")
            .map_err(|_| Denial::ServerError)?,
        workspace_logo_asset_id: row
            .try_get("workspace_logo_asset_id")
            .map_err(|_| Denial::ServerError)?,
    })
}

/// `ProjectInvitationsViewset.get_queryset` (`invite.py:43-51`): live
/// invites in this workspace slug on this project, `-created_at`. No
/// pagination (`paginator_class` unset) and the filter backends are
/// configured empty, so the DRF-default list renders the full array.
async fn fetch_invites(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
) -> Result<Vec<InviteRow>, Denial> {
    let sql = format!(
        "SELECT {INVITE_SELECT} WHERE i.deleted_at IS NULL AND w.slug = $1 AND i.project_id = $2 \
         ORDER BY i.created_at DESC"
    );
    let rows = sqlx::query(&sql)
        .bind(slug)
        .bind(project_id)
        .fetch_all(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    rows.iter().map(invite_row).collect()
}

/// One invite over the scoped queryset (DRF-default `get_object()`:
/// `get_object_or_404` — Django's `No ... matches` miss).
async fn fetch_invite_one(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    pk: &uuid::Uuid,
) -> Result<InviteRow, Denial> {
    let sql = format!(
        "SELECT {INVITE_SELECT} WHERE i.deleted_at IS NULL AND w.slug = $1 AND i.project_id = $2 \
         AND i.id = $3"
    );
    let row = sqlx::query(&sql)
        .bind(slug)
        .bind(project_id)
        .bind(pk)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    row.as_ref()
        .map(invite_row)
        .transpose()?
        .ok_or(Denial::InviteNotFound)
}

/// `UserProjectInvitationsViewset.get_queryset` (`invite.py:120-126`):
/// live invites addressed to the caller's email, across all workspaces
/// (no slug predicate), `-created_at`.
async fn fetch_user_invites(
    pool: &sqlx::PgPool,
    email: Option<&str>,
) -> Result<Vec<InviteRow>, Denial> {
    let sql = if email.is_some() {
        format!(
            "SELECT {INVITE_SELECT} WHERE i.deleted_at IS NULL AND i.email = $1 \
             ORDER BY i.created_at DESC"
        )
    } else {
        format!(
            "SELECT {INVITE_SELECT} WHERE i.deleted_at IS NULL AND i.email IS NULL \
             ORDER BY i.created_at DESC"
        )
    };
    let mut query = sqlx::query(&sql);
    if let Some(email) = email {
        query = query.bind(email);
    }
    let rows = query
        .fetch_all(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    rows.iter().map(invite_row).collect()
}

/// `ProjectJoinEndpoint.get` / `.post` lookup (`invite.py:187,252`):
/// `.get()` over the slug + project + pk — a miss is the
/// `ObjectDoesNotExist` 404, and a duplicate (impossible under the live
/// manager — `pk` is unique) would be the generic 500.
async fn fetch_join_invite(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    pk: &uuid::Uuid,
) -> Result<InviteRow, Denial> {
    let sql = format!(
        "SELECT {INVITE_SELECT} WHERE i.deleted_at IS NULL AND w.slug = $1 AND i.project_id = $2 \
         AND i.id = $3"
    );
    let rows = sqlx::query(&sql)
        .bind(slug)
        .bind(project_id)
        .bind(pk)
        .fetch_all(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    if rows.len() > 1 {
        return Err(Denial::ServerError);
    }
    rows.first()
        .map(invite_row)
        .transpose()?
        .ok_or(Denial::ObjectNotFound)
}

/// Rendered strings owned by the caller so the borrowed L2 views can
/// reference them.
struct RenderedInvite {
    id: String,
    project_id: String,
    workspace_id: String,
    created_at: String,
    updated_at: String,
    deleted_at: Option<String>,
    responded_at: Option<String>,
    created_by: Option<String>,
    updated_by: Option<String>,
    cover_image_url: Option<String>,
    logo_url: Option<String>,
}

fn render_invite(
    row: &InviteRow,
    tz: &Tz,
    assets: &HashMap<uuid::Uuid, String>,
) -> (RenderedInvite, Value) {
    let rendered = RenderedInvite {
        id: row.id.to_string(),
        project_id: row.project_id.to_string(),
        workspace_id: row.workspace_id.to_string(),
        created_at: crate::serializer::render_datetime_in(&row.created_at, tz),
        updated_at: crate::serializer::render_datetime_in(&row.updated_at, tz),
        deleted_at: row
            .deleted_at
            .as_ref()
            .map(|dt| crate::serializer::render_datetime_in(dt, tz)),
        responded_at: row
            .responded_at
            .as_ref()
            .map(|dt| crate::serializer::render_datetime_in(dt, tz)),
        created_by: row.created_by_id.map(|id| id.to_string()),
        updated_by: row.updated_by_id.map(|id| id.to_string()),
        cover_image_url: resolve_project_cover_image_url(
            row.project_cover_asset_id.is_some(),
            row.project_cover_asset_id
                .and_then(|id| assets.get(&id).map(String::as_str)),
            row.project_cover_image.as_deref(),
        )
        .map(str::to_owned),
        logo_url: resolve_workspace_logo_url(
            row.workspace_logo_asset_id.is_some(),
            row.workspace_logo_asset_id
                .and_then(|id| assets.get(&id).map(String::as_str)),
            row.workspace_logo.as_deref(),
        )
        .map(str::to_owned),
    };
    let input = ProjectMemberInviteRow {
        id: &rendered.id,
        project: ProjectLiteRow {
            id: &rendered.project_id,
            identifier: &row.project_identifier,
            name: &row.project_name,
            cover_image: row.project_cover_image.as_deref(),
            cover_image_url: rendered.cover_image_url.as_deref(),
            logo_props: &row.project_logo_props,
            description: &row.project_description,
            is_default: row.project_is_default,
        },
        workspace: WorkspaceLiteRow {
            name: &row.workspace_name,
            slug: &row.workspace_slug,
            id: &rendered.workspace_id,
            logo_url: rendered.logo_url.as_deref(),
        },
        created_at: &rendered.created_at,
        updated_at: &rendered.updated_at,
        deleted_at: rendered.deleted_at.as_deref(),
        email: &row.email,
        accepted: row.accepted,
        token: &row.token,
        message: row.message.as_deref(),
        responded_at: rendered.responded_at.as_deref(),
        role: i64::from(row.role),
        created_by: rendered.created_by.as_deref(),
        updated_by: rendered.updated_by.as_deref(),
    };
    let view = invite_to_representation(&input);
    let body = serde_json::to_value(&view).expect("invite view serializes");
    (rendered, body)
}

/// Collect the asset ids one invite row references.
fn invite_asset_ids(row: &InviteRow, out: &mut Vec<uuid::Uuid>) {
    out.extend(row.project_cover_asset_id);
    out.extend(row.workspace_logo_asset_id);
}

// ---------------------------------------------------------------------------
// Board reads
// ---------------------------------------------------------------------------

/// One board row with its nested lite columns. The project FK is
/// nullable (`WorkspaceBaseModel.project`), so its half arrives `None`
/// when unset — `project_details` then renders `null`.
struct BoardRow {
    id: uuid::Uuid,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
    deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    entity_identifier: Option<uuid::Uuid>,
    entity_name: Option<String>,
    anchor: String,
    is_comments_enabled: bool,
    is_reactions_enabled: bool,
    is_votes_enabled: bool,
    view_props: Value,
    is_activity_enabled: bool,
    is_disabled: bool,
    created_by_id: Option<uuid::Uuid>,
    updated_by_id: Option<uuid::Uuid>,
    workspace_id: uuid::Uuid,
    workspace_name: String,
    workspace_slug: String,
    workspace_logo: Option<String>,
    workspace_logo_asset_id: Option<uuid::Uuid>,
    project_identifier: Option<String>,
    project_name: Option<String>,
    project_cover_image: Option<String>,
    project_cover_asset_id: Option<uuid::Uuid>,
    project_logo_props: Option<Value>,
    project_description: Option<String>,
    project_is_default: Option<bool>,
}

const BOARD_SELECT: &str = "b.id, b.created_at, b.updated_at, b.deleted_at, b.entity_identifier, \
     b.entity_name, b.anchor, b.is_comments_enabled, b.is_reactions_enabled, b.is_votes_enabled, \
     b.view_props, b.is_activity_enabled, b.is_disabled, b.created_by_id, b.updated_by_id, \
     b.workspace_id, w.name AS workspace_name, w.slug AS workspace_slug, \
     w.logo AS workspace_logo, w.logo_asset_id AS workspace_logo_asset_id, \
     p.id AS project_id, p.identifier AS project_identifier, p.name AS project_name, \
     p.cover_image AS project_cover_image, p.cover_image_asset_id AS project_cover_asset_id, \
     p.logo_props AS project_logo_props, p.description AS project_description, \
     p.is_default AS project_is_default, b.project_id AS board_project_id, b.intake_id AS intake_id \
     FROM deploy_boards b \
     JOIN workspaces w ON w.id = b.workspace_id \
     LEFT JOIN projects p ON p.id = b.project_id";

fn board_row(
    row: &sqlx::postgres::PgRow,
) -> Result<(BoardRow, Option<uuid::Uuid>, Option<uuid::Uuid>), Denial> {
    let board_project_id: Option<uuid::Uuid> = row
        .try_get("board_project_id")
        .map_err(|_| Denial::ServerError)?;
    let intake_id: Option<uuid::Uuid> =
        row.try_get("intake_id").map_err(|_| Denial::ServerError)?;
    Ok((
        BoardRow {
            id: row.try_get("id").map_err(|_| Denial::ServerError)?,
            created_at: row.try_get("created_at").map_err(|_| Denial::ServerError)?,
            updated_at: row.try_get("updated_at").map_err(|_| Denial::ServerError)?,
            deleted_at: row.try_get("deleted_at").map_err(|_| Denial::ServerError)?,
            entity_identifier: row
                .try_get("entity_identifier")
                .map_err(|_| Denial::ServerError)?,
            entity_name: row
                .try_get("entity_name")
                .map_err(|_| Denial::ServerError)?,
            anchor: row.try_get("anchor").map_err(|_| Denial::ServerError)?,
            is_comments_enabled: row
                .try_get("is_comments_enabled")
                .map_err(|_| Denial::ServerError)?,
            is_reactions_enabled: row
                .try_get("is_reactions_enabled")
                .map_err(|_| Denial::ServerError)?,
            is_votes_enabled: row
                .try_get("is_votes_enabled")
                .map_err(|_| Denial::ServerError)?,
            view_props: row.try_get("view_props").map_err(|_| Denial::ServerError)?,
            is_activity_enabled: row
                .try_get("is_activity_enabled")
                .map_err(|_| Denial::ServerError)?,
            is_disabled: row
                .try_get("is_disabled")
                .map_err(|_| Denial::ServerError)?,
            created_by_id: row
                .try_get("created_by_id")
                .map_err(|_| Denial::ServerError)?,
            updated_by_id: row
                .try_get("updated_by_id")
                .map_err(|_| Denial::ServerError)?,
            workspace_id: row
                .try_get("workspace_id")
                .map_err(|_| Denial::ServerError)?,
            workspace_name: row
                .try_get("workspace_name")
                .map_err(|_| Denial::ServerError)?,
            workspace_slug: row
                .try_get("workspace_slug")
                .map_err(|_| Denial::ServerError)?,
            workspace_logo: row
                .try_get("workspace_logo")
                .map_err(|_| Denial::ServerError)?,
            workspace_logo_asset_id: row
                .try_get("workspace_logo_asset_id")
                .map_err(|_| Denial::ServerError)?,
            project_identifier: row
                .try_get("project_identifier")
                .map_err(|_| Denial::ServerError)?,
            project_name: row
                .try_get("project_name")
                .map_err(|_| Denial::ServerError)?,
            project_cover_image: row
                .try_get("project_cover_image")
                .map_err(|_| Denial::ServerError)?,
            project_cover_asset_id: row
                .try_get("project_cover_asset_id")
                .map_err(|_| Denial::ServerError)?,
            project_logo_props: row
                .try_get("project_logo_props")
                .map_err(|_| Denial::ServerError)?,
            project_description: row
                .try_get("project_description")
                .map_err(|_| Denial::ServerError)?,
            project_is_default: row
                .try_get("project_is_default")
                .map_err(|_| Denial::ServerError)?,
        },
        board_project_id,
        intake_id,
    ))
}

/// `DeployBoardViewSet.list` (`base.py:544-551`): the latest live
/// project-scoped row, or `None` (which serializes to the 12-key initial
/// shape — the DRF `get_initial()` bytes in
/// [`DEPLOY_BOARD_INITIAL_BODY`]).
async fn fetch_board_first(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
) -> Result<Option<(BoardRow, Option<uuid::Uuid>, Option<uuid::Uuid>)>, Denial> {
    let sql = format!(
        "SELECT {BOARD_SELECT} WHERE b.deleted_at IS NULL AND b.entity_name = 'project' \
         AND b.entity_identifier = $1 AND w.slug = $2 ORDER BY b.created_at DESC LIMIT 1"
    );
    let row = sqlx::query(&sql)
        .bind(project_id)
        .bind(slug)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    row.as_ref().map(board_row).transpose()
}

/// Re-read for the PATCH response render: the PATCH may have *set*
/// `deleted_at`, and Django renders its in-memory instance (200 with
/// the stamp) — a live-only re-read would 404 instead.
async fn fetch_board_for_patch_render(
    pool: &sqlx::PgPool,
    pk: &uuid::Uuid,
) -> Result<(BoardRow, Option<uuid::Uuid>, Option<uuid::Uuid>), Denial> {
    let sql = format!("SELECT {BOARD_SELECT} WHERE b.id = $1");
    let row = sqlx::query(&sql)
        .bind(pk)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    row.as_ref()
        .map(board_row)
        .transpose()?
        .ok_or(Denial::ServerError)
}

/// `DeployBoardViewSet` retrieve / partial_update / destroy lookup: the
/// viewset defines no `get_queryset`, so DRF-default `get_object()` runs
/// over the global `model.objects.all()` — any live board UUID resolves,
/// regardless of workspace or project. A miss is Django's `No DeployBoard matches` 404.
async fn fetch_board_global(
    pool: &sqlx::PgPool,
    pk: &uuid::Uuid,
) -> Result<(BoardRow, Option<uuid::Uuid>, Option<uuid::Uuid>), Denial> {
    let sql = format!("SELECT {BOARD_SELECT} WHERE b.deleted_at IS NULL AND b.id = $1");
    let row = sqlx::query(&sql)
        .bind(pk)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    row.as_ref()
        .map(board_row)
        .transpose()?
        .ok_or(Denial::BoardNotFound)
}

/// Render one board row. `views_override` carries the just-assigned
/// `view_props` for create / PATCH-with-views: Django serializes its
/// in-memory instance (Python dict order), while a re-read would come
/// back in jsonb-normalized key order.
fn render_board(
    row: &BoardRow,
    project_id: Option<uuid::Uuid>,
    intake_id: Option<uuid::Uuid>,
    tz: &Tz,
    assets: &HashMap<uuid::Uuid, String>,
    views_override: Option<&Value>,
) -> String {
    let project_details = match (
        project_id,
        row.project_identifier.as_deref(),
        row.project_name.as_deref(),
        row.project_logo_props.as_ref(),
        row.project_description.as_deref(),
        row.project_is_default,
    ) {
        (
            Some(id),
            Some(identifier),
            Some(name),
            Some(logo_props),
            Some(description),
            Some(is_default),
        ) => {
            let cover_image_url = resolve_project_cover_image_url(
                row.project_cover_asset_id.is_some(),
                row.project_cover_asset_id
                    .and_then(|id| assets.get(&id).map(String::as_str)),
                row.project_cover_image.as_deref(),
            );
            let id = id.to_string();
            serde_json::to_value(project_lite_to_representation(&ProjectLiteRow {
                id: &id,
                identifier,
                name,
                cover_image: row.project_cover_image.as_deref(),
                cover_image_url,
                logo_props,
                description,
                is_default,
            }))
            .expect("project lite serializes")
        }
        _ => Value::Null,
    };
    let workspace_id = row.workspace_id.to_string();
    let logo_url = resolve_workspace_logo_url(
        row.workspace_logo_asset_id.is_some(),
        row.workspace_logo_asset_id
            .and_then(|id| assets.get(&id).map(String::as_str)),
        row.workspace_logo.as_deref(),
    );
    let workspace_detail =
        serde_json::to_value(workspace_lite_to_representation(&WorkspaceLiteRow {
            name: &row.workspace_name,
            slug: &row.workspace_slug,
            id: &workspace_id,
            logo_url,
        }))
        .expect("workspace lite serializes");
    let read = DeployBoardRead {
        id: row.id.to_string(),
        project_details: if project_details.is_null() {
            None
        } else {
            Some(project_details)
        },
        workspace_detail,
        created_at: crate::serializer::render_datetime_in(&row.created_at, tz),
        updated_at: crate::serializer::render_datetime_in(&row.updated_at, tz),
        deleted_at: row
            .deleted_at
            .as_ref()
            .map(|dt| crate::serializer::render_datetime_in(dt, tz)),
        entity_identifier: row.entity_identifier.map(|id| id.to_string()),
        entity_name: row.entity_name.clone(),
        anchor: row.anchor.clone(),
        is_comments_enabled: row.is_comments_enabled,
        is_reactions_enabled: row.is_reactions_enabled,
        is_votes_enabled: row.is_votes_enabled,
        view_props: views_override
            .cloned()
            .unwrap_or_else(|| row.view_props.clone()),
        is_activity_enabled: row.is_activity_enabled,
        is_disabled: row.is_disabled,
        created_by: row.created_by_id.map(|id| id.to_string()),
        updated_by: row.updated_by_id.map(|id| id.to_string()),
        workspace: workspace_id,
        project: project_id.map(|id| id.to_string()),
        intake: intake_id.map(|id| id.to_string()),
    };
    serde_json::to_string(&read).expect("board read serializes")
}

/// Collect the asset ids one board row references.
fn board_asset_ids(row: &BoardRow, out: &mut Vec<uuid::Uuid>) {
    out.extend(row.project_cover_asset_id);
    out.extend(row.workspace_logo_asset_id);
}

// ---------------------------------------------------------------------------
// Write helpers: model defaults + deferred publishes
// ---------------------------------------------------------------------------

/// `db/models/project.py:get_default_props` (member `view_props` /
/// `default_props`): filters + display filters, no display properties.
fn default_member_props_json() -> String {
    serde_json::json!({
        "filters": {
            "priority": null, "state": null, "state_group": null,
            "assignees": null, "created_by": null, "labels": null,
            "start_date": null, "target_date": null, "subscriber": null,
        },
        "display_filters": {
            "group_by": null, "order_by": "-created_at", "type": null,
            "sub_issue": true, "show_empty_groups": true,
            "layout": "list", "calendar_date_range": "",
        },
    })
    .to_string()
}

/// `db/models/project.py:get_default_preferences`.
fn default_preferences_json() -> String {
    serde_json::json!({
        "pages": {"block_display": true},
        "navigation": {"default_tab": "work_items", "hide_in_more_menu": []},
    })
    .to_string()
}

/// `db/models/issue.py:get_default_filters` (user-property `filters`).
fn default_issue_filters_json() -> String {
    serde_json::json!({
        "priority": null, "state": null, "state_group": null,
        "assignees": null, "created_by": null, "labels": null,
        "start_date": null, "target_date": null, "subscriber": null,
    })
    .to_string()
}

/// `db/models/issue.py:get_default_display_filters` (flat form).
fn default_issue_display_filters_json() -> String {
    serde_json::json!({
        "group_by": null, "order_by": "-created_at", "type": null,
        "sub_issue": true, "show_empty_groups": true,
        "layout": "list", "calendar_date_range": "",
    })
    .to_string()
}

/// `db/models/issue.py:get_default_display_properties` (flat form).
fn default_issue_display_properties_json() -> String {
    serde_json::json!({
        "assignee": true, "attachment_count": true, "created_on": true,
        "due_date": true, "estimate": true, "key": true, "labels": true,
        "link": true, "priority": true, "start_date": true, "state": true,
        "sub_issue_count": true, "updated_on": true,
    })
    .to_string()
}

/// `db/models/workspace.py:get_default_props` (workspace-member
/// `view_props` / `default_props`): filters + display filters + display
/// properties — unlike the project twin above.
fn default_workspace_props_json() -> String {
    serde_json::json!({
        "filters": {
            "priority": null, "state": null, "state_group": null,
            "assignees": null, "created_by": null, "labels": null,
            "start_date": null, "target_date": null, "subscriber": null,
        },
        "display_filters": {
            "group_by": null, "order_by": "-created_at", "type": null,
            "sub_issue": true, "show_empty_groups": true,
            "layout": "list", "calendar_date_range": "",
        },
        "display_properties": {
            "assignee": true, "attachment_count": true, "created_on": true,
            "due_date": true, "estimate": true, "key": true, "labels": true,
            "link": true, "priority": true, "start_date": true, "state": true,
            "sub_issue_count": true, "updated_on": true,
        },
    })
    .to_string()
}

/// `db/models/workspace.py:get_issue_props` (workspace-member
/// `issue_props`).
fn default_issue_props_json() -> String {
    serde_json::json!({"subscribed": true, "assigned": true, "created": true, "all_issues": true})
        .to_string()
}

/// `db/models/project.py:get_default_views` (board create `views`
/// default).
fn default_board_views_json() -> String {
    serde_json::json!({
        "list": true, "kanban": true, "calendar": true, "gantt": true, "spreadsheet": true,
    })
    .to_string()
}

/// `soft_delete_related_objects.delay("db", <model>, pk, using=None)`
/// (`db/mixins.py:77`): three positional args, `using` stays a null
/// kwarg (FX-APROJ-08 live capture).
fn soft_delete_message(model: &str, pk: &uuid::Uuid) -> pidash_jobs::celery::CeleryTaskMessage {
    let mut kwargs = Map::new();
    kwargs.insert("using".to_owned(), Value::Null);
    pidash_jobs::celery::CeleryTaskMessage::new(
        pidash_jobs::tasks_cleanup::deletion::SOFT_DELETE_TASK,
        vec![
            Value::String("db".to_owned()),
            Value::String(model.to_owned()),
            Value::String(pk.to_string()),
        ],
        kwargs,
    )
}

/// Best-effort deferred publish (the cycles precedent): without the
/// queue the response still stands.
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

/// `ProjectMember.objects.create(...)` (`invite.py:226-230`,
/// `db/models/project.py:348-364`): the save backfills the workspace from
/// the project (a missing project row raises first — the FK fetch), then
/// the add-branch inserts the `ProjectUserProperty` row with the member's
/// minimum workspace sort order minus 10000 (65535 when none), and only
/// then does the member row insert. `created_by` is the crum user (`None`
/// for anonymous joins); `updated_by` stays `NULL`.
async fn create_project_member(
    pool: &sqlx::PgPool,
    project_id: &uuid::Uuid,
    member_id: Option<uuid::Uuid>,
    role: i16,
    created_by: Option<uuid::Uuid>,
) -> Result<(), Denial> {
    let workspace_id: Option<(uuid::Uuid,)> =
        sqlx::query_as("SELECT workspace_id FROM projects WHERE id = $1 AND deleted_at IS NULL")
            .bind(project_id)
            .fetch_optional(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    let Some((workspace_id,)) = workspace_id else {
        return Err(Denial::ObjectNotFound);
    };
    if let Some(member_id) = member_id {
        let min_sort: Option<f64> = sqlx::query_scalar(
            "SELECT MIN(sort_order) FROM project_user_properties
             WHERE workspace_id = $1 AND user_id = $2 AND deleted_at IS NULL",
        )
        .bind(workspace_id)
        .bind(member_id)
        .fetch_one(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
        let sort_order = min_sort.map(|min| min - 10000.0).unwrap_or(65535.0);
        sqlx::query(
            "INSERT INTO project_user_properties (id, workspace_id, project_id, user_id,
                filters, display_filters, display_properties, rich_filters, preferences,
                sort_order, created_by_id, updated_by_id, created_at, updated_at, deleted_at)
             VALUES ($1, $2, $3, $4,
                $5::jsonb, $6::jsonb, $7::jsonb, '{}', $8::jsonb,
                $9, $10, NULL, now(), now(), NULL)",
        )
        .bind(uuid::Uuid::new_v4())
        .bind(workspace_id)
        .bind(project_id)
        .bind(member_id)
        .bind(default_issue_filters_json())
        .bind(default_issue_display_filters_json())
        .bind(default_issue_display_properties_json())
        .bind(default_preferences_json())
        .bind(sort_order)
        .bind(created_by)
        .execute(pool)
        .await
        .map_err(integrity_denial)?;
    }
    sqlx::query(
        "INSERT INTO project_members (id, workspace_id, project_id, member_id, role,
            comment, view_props, default_props, preferences, sort_order,
            is_active, created_by_id, updated_by_id, created_at, updated_at, deleted_at)
         VALUES ($1, $2, $3, $4, $5,
            NULL, $6::jsonb, $6::jsonb, $7::jsonb, 65535,
            true, $8, NULL, now(), now(), NULL)",
    )
    .bind(uuid::Uuid::new_v4())
    .bind(workspace_id)
    .bind(project_id)
    .bind(member_id)
    .bind(role)
    .bind(default_member_props_json())
    .bind(default_preferences_json())
    .bind(created_by)
    .execute(pool)
    .await
    .map_err(integrity_denial)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Handlers: project invitations
// ---------------------------------------------------------------------------

/// `GET .../projects/<project_id>/invitations/` (`invite.py:37-51`):
/// DRF-default list over the scoped queryset (full array, `-created_at`).
/// The gate is auth-only — any signed-in user lists.
async fn invite_list(
    State(state): State<AppState>,
    Path((slug, project_raw)): Path<(String, String)>,
    extension: Option<axum::Extension<SessionHandle>>,
) -> Result<Response, Denial> {
    let resolved = actor(&state, extension).await?;
    let pool = pool_of(&state)?.clone();
    let project_id = resolve_project_id(&pool, &slug, &project_raw).await?;
    check_gate(
        &pool,
        &slug,
        Some(&project_id),
        &resolved.id,
        "GET",
        "workspaces/<slug>/projects/<project_id>/invitations/",
    )
    .await?;
    let rows = fetch_invites(&pool, &slug, &project_id).await?;
    let mut asset_ids = Vec::new();
    for row in &rows {
        invite_asset_ids(row, &mut asset_ids);
    }
    let assets = fetch_asset_urls(&pool, &asset_ids).await?;
    let mut out = String::from("[");
    for (index, row) in rows.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        let (_, body) = render_invite(row, &resolved.timezone, &assets);
        out.push_str(&serde_json::to_string(&body).expect("invite serializes"));
    }
    out.push(']');
    Ok(json_response(StatusCode::OK, out))
}

/// `POST .../projects/<project_id>/invitations/` (`invite.py:54-113`,
/// ADMIN-gated): an empty `emails` answers the 400; anything else raises
/// `AttributeError` at `:62` (`.role` off a queryset) before any row is
/// written — the generic 500 with 0 rows persisted (FX-APROJ-08). The
/// role-mismatch 400 and the `.delay`-on-a-list at `:104` are dead with it.
async fn invite_create(
    State(state): State<AppState>,
    Path((slug, project_raw)): Path<(String, String)>,
    extension: Option<axum::Extension<SessionHandle>>,
    req: axum::extract::Request,
) -> Result<Response, Denial> {
    let resolved = actor(&state, extension).await?;
    let pool = pool_of(&state)?.clone();
    let project_id = resolve_project_id(&pool, &slug, &project_raw).await?;
    check_gate(
        &pool,
        &slug,
        Some(&project_id),
        &resolved.id,
        "POST",
        "workspaces/<slug>/projects/<project_id>/invitations/",
    )
    .await?;
    let body = match parse_body(&state, req).await {
        Ok(body) => body,
        Err(response) => return Ok(response),
    };
    // `request.data.get("emails", [])` — a non-dict body has no `.get`
    // (`AttributeError` → 500); a missing or empty `emails` is the 400.
    let emails = match body.as_object() {
        Some(map) => map.get("emails"),
        None => return Err(Denial::ServerError),
    };
    let empty = match emails {
        None => true,
        Some(Value::Null) => true,
        Some(Value::Array(items)) => items.is_empty(),
        Some(Value::Object(map)) => map.is_empty(),
        Some(Value::String(s)) => s.is_empty(),
        Some(Value::Bool(flag)) => !flag,
        Some(Value::Number(number)) => {
            number.as_i64() == Some(0) || number.as_u64() == Some(0) || number.as_f64() == Some(0.0)
        }
    };
    if empty {
        return Ok(json_response(
            StatusCode::BAD_REQUEST,
            EMAILS_REQUIRED_BODY.to_owned(),
        ));
    }
    // `:62` raises before any row is written (FX-APROJ-08): the generic
    // 500, no database touch.
    Err(Denial::ServerError)
}

/// `GET .../invitations/<uuid:pk>/` (`invite.py:37-51`): DRF-default
/// retrieve over the scoped queryset; a miss is Django's `No ProjectMemberInvite matches` 404.
async fn invite_retrieve(
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
    let project_id = resolve_project_id(&pool, &slug, &project_raw).await?;
    check_gate(
        &pool,
        &slug,
        Some(&project_id),
        &resolved.id,
        "GET",
        "workspaces/<slug>/projects/<project_id>/invitations/<pk>/",
    )
    .await?;
    let pk = pk.parse::<uuid::Uuid>().map_err(|_| Denial::ServerError)?;
    let row = fetch_invite_one(&pool, &slug, &project_id, &pk).await?;
    let mut asset_ids = Vec::new();
    invite_asset_ids(&row, &mut asset_ids);
    let assets = fetch_asset_urls(&pool, &asset_ids).await?;
    let (_, body) = render_invite(&row, &resolved.timezone, &assets);
    Ok(json_response(
        StatusCode::OK,
        serde_json::to_string(&body).expect("invite serializes"),
    ))
}

/// `DELETE .../invitations/<uuid:pk>/` (`invite.py:37-51`): DRF-default
/// destroy — scoped lookup, soft delete, deferred related-objects
/// publish, 204.
async fn invite_destroy(
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
    let project_id = resolve_project_id(&pool, &slug, &project_raw).await?;
    check_gate(
        &pool,
        &slug,
        Some(&project_id),
        &resolved.id,
        "DELETE",
        "workspaces/<slug>/projects/<project_id>/invitations/<pk>/",
    )
    .await?;
    let pk = pk.parse::<uuid::Uuid>().map_err(|_| Denial::ServerError)?;
    fetch_invite_one(&pool, &slug, &project_id, &pk).await?;
    sqlx::query(
        "UPDATE project_member_invites SET deleted_at = now(), updated_at = now(), updated_by_id = $1
         WHERE id = $2 AND deleted_at IS NULL",
    )
    .bind(resolved.id)
    .bind(pk)
    .execute(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    enqueue_message(&pool, soft_delete_message("projectmemberinvite", &pk)).await;
    Ok(empty_response(StatusCode::NO_CONTENT))
}

// ---------------------------------------------------------------------------
// Handlers: user project invitations
// ---------------------------------------------------------------------------

/// One multi-row `INSERT ... ON CONFLICT DO NOTHING` for the member
/// half of `UserProjectInvitationsViewset.create`'s `bulk_create`s: a
/// single bad row fails the whole statement (a constraint failure is
/// the `IntegrityError` 400); an empty id list inserts nothing.
async fn bulk_insert_members(
    pool: &sqlx::PgPool,
    project_ids: &[uuid::Uuid],
    workspace_id: &uuid::Uuid,
    user_id: &uuid::Uuid,
    role: i16,
) -> Result<(), Denial> {
    if project_ids.is_empty() {
        return Ok(());
    }
    let props = default_member_props_json();
    let prefs = default_preferences_json();
    let mut sql = String::from(
        "INSERT INTO project_members (id, workspace_id, project_id, member_id, role,
            comment, view_props, default_props, preferences, sort_order,
            is_active, created_by_id, updated_by_id, created_at, updated_at, deleted_at)
         VALUES ",
    );
    for (index, _) in project_ids.iter().enumerate() {
        if index > 0 {
            sql.push_str(", ");
        }
        let base = index * 8;
        sql.push_str(&format!(
            "(${}, ${}, ${}, ${}, ${}, NULL, ${}::jsonb, ${}::jsonb, ${}::jsonb, \
             65535, true, ${}, NULL, now(), now(), NULL)",
            base + 1,
            base + 2,
            base + 3,
            base + 4,
            base + 5,
            base + 6,
            base + 6,
            base + 7,
            base + 8,
        ));
    }
    sql.push_str(" ON CONFLICT DO NOTHING");
    let mut query = sqlx::query(&sql);
    for project_id in project_ids {
        query = query
            .bind(uuid::Uuid::new_v4())
            .bind(*workspace_id)
            .bind(*project_id)
            .bind(*user_id)
            .bind(role)
            .bind(props.clone())
            .bind(prefs.clone())
            .bind(*user_id);
    }
    query.execute(pool).await.map_err(integrity_denial)?;
    Ok(())
}

/// One multi-row `INSERT ... ON CONFLICT DO NOTHING` for the
/// user-property half (same statement shape as
/// [`bulk_insert_members`]).
async fn bulk_insert_user_properties(
    pool: &sqlx::PgPool,
    project_ids: &[uuid::Uuid],
    workspace_id: &uuid::Uuid,
    user_id: &uuid::Uuid,
) -> Result<(), Denial> {
    if project_ids.is_empty() {
        return Ok(());
    }
    let filters = default_issue_filters_json();
    let display_filters = default_issue_display_filters_json();
    let display_properties = default_issue_display_properties_json();
    let prefs = default_preferences_json();
    let mut sql = String::from(
        "INSERT INTO project_user_properties (id, workspace_id, project_id, user_id,
            filters, display_filters, display_properties, rich_filters, preferences,
            sort_order, created_by_id, updated_by_id, created_at, updated_at, deleted_at)
         VALUES ",
    );
    for (index, _) in project_ids.iter().enumerate() {
        if index > 0 {
            sql.push_str(", ");
        }
        let base = index * 9;
        sql.push_str(&format!(
            "(${}, ${}, ${}, ${}, ${}::jsonb, ${}::jsonb, ${}::jsonb, '{{}}', ${}::jsonb, \
             65535, ${}, NULL, now(), now(), NULL)",
            base + 1,
            base + 2,
            base + 3,
            base + 4,
            base + 5,
            base + 6,
            base + 7,
            base + 8,
            base + 9,
        ));
    }
    sql.push_str(" ON CONFLICT DO NOTHING");
    let mut query = sqlx::query(&sql);
    for project_id in project_ids {
        query = query
            .bind(uuid::Uuid::new_v4())
            .bind(*workspace_id)
            .bind(*project_id)
            .bind(*user_id)
            .bind(filters.clone())
            .bind(display_filters.clone())
            .bind(display_properties.clone())
            .bind(prefs.clone())
            .bind(*user_id);
    }
    query.execute(pool).await.map_err(integrity_denial)?;
    Ok(())
}

/// `GET users/me/workspaces/<slug>/projects/invitations/`
/// (`invite.py:116-126`): DRF-default list over the caller's-email
/// queryset (full array, `-created_at`, no slug scoping). Auth-only gate.
async fn user_invite_list(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    extension: Option<axum::Extension<SessionHandle>>,
) -> Result<Response, Denial> {
    let resolved = actor(&state, extension).await?;
    let pool = pool_of(&state)?.clone();
    check_gate(
        &pool,
        &slug,
        None,
        &resolved.id,
        "GET",
        "users/me/workspaces/<slug>/projects/invitations/",
    )
    .await?;
    let rows = fetch_user_invites(&pool, resolved.email.as_deref()).await?;
    let mut asset_ids = Vec::new();
    for row in &rows {
        invite_asset_ids(row, &mut asset_ids);
    }
    let assets = fetch_asset_urls(&pool, &asset_ids).await?;
    let mut out = String::from("[");
    for (index, row) in rows.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        let (_, body) = render_invite(row, &resolved.timezone, &assets);
        out.push_str(&serde_json::to_string(&body).expect("invite serializes"));
    }
    out.push(']');
    Ok(json_response(StatusCode::OK, out))
}

/// `POST users/me/workspaces/<slug>/projects/invitations/`
/// (`invite.py:128-180`, workspace ADMIN/MEMBER-gated): join `project_ids`
/// — the SECRET-project guard 403s non-admins, then live memberships are
/// reactivated and missing member + user-property rows are bulk-inserted
/// (`ON CONFLICT DO NOTHING`), answering 201. Unknown ids are skipped
/// silently; a bad-UUID id is the `ValidationError` 400.
async fn user_invite_create(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    extension: Option<axum::Extension<SessionHandle>>,
    req: axum::extract::Request,
) -> Result<Response, Denial> {
    let resolved = actor(&state, extension).await?;
    let pool = pool_of(&state)?.clone();
    check_gate(
        &pool,
        &slug,
        None,
        &resolved.id,
        "POST",
        "users/me/workspaces/<slug>/projects/invitations/",
    )
    .await?;
    let body = match parse_body(&state, req).await {
        Ok(body) => body,
        Err(response) => return Ok(response),
    };
    // `request.data.get("project_ids", [])` — a non-dict body has no
    // `.get` (`AttributeError` → 500).
    let data = match body.as_object() {
        Some(map) => map,
        None => return Err(Denial::ServerError),
    };
    let project_ids = match data.get("project_ids") {
        None => Vec::new(),
        Some(Value::Null) => return Err(Denial::ServerError),
        Some(Value::Array(items)) => {
            let mut ids = Vec::with_capacity(items.len());
            for item in items {
                // `id__in=[None]` matches nothing (uuid pk is never
                // null); every other item preps or is the 400.
                if item.is_null() {
                    continue;
                }
                ids.push(prep_uuid(item)?);
            }
            ids
        }
        // A string iterates its chars through UUID prep (each one
        // fails); anything else is not iterable (`TypeError` → 500).
        Some(Value::String(raw)) => {
            if raw.is_empty() {
                Vec::new()
            } else {
                return Err(Denial::BadValidation);
            }
        }
        Some(Value::Object(map)) => {
            if map.is_empty() {
                Vec::new()
            } else {
                return Err(Denial::BadValidation);
            }
        }
        Some(_) => return Err(Denial::ServerError),
    };
    // The gate passed, so the workspace row exists; a miss (a race)
    // answers the `.get()` 404 like Python.
    let member: Option<(uuid::Uuid, i16)> = sqlx::query_as(
        r#"SELECT wm.workspace_id, wm.role FROM workspace_members wm
           JOIN workspaces w ON w.id = wm.workspace_id
           WHERE wm.member_id = $1 AND w.slug = $2 AND wm.is_active AND wm.deleted_at IS NULL"#,
    )
    .bind(resolved.id)
    .bind(&slug)
    .fetch_optional(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let Some((workspace_id, workspace_role)) = member else {
        return Err(Denial::ObjectNotFound);
    };
    // `Project.objects.filter(id__in=..., workspace__slug=...)`: live
    // rows only; unknown ids vanish silently.
    let projects: Vec<(uuid::Uuid, i16)> = if project_ids.is_empty() {
        Vec::new()
    } else {
        sqlx::query_as(
            r#"SELECT p.id, p.network FROM projects p
               JOIN workspaces w ON w.id = p.workspace_id
               WHERE p.id = ANY($1) AND w.slug = $2 AND p.deleted_at IS NULL"#,
        )
        .bind(&project_ids)
        .bind(&slug)
        .fetch_all(&pool)
        .await
        .map_err(|_| Denial::ServerError)?
    };
    for (_, network) in &projects {
        if *network == 0 && i32::from(workspace_role) != ROLE_ADMIN {
            return Ok(json_response(
                StatusCode::FORBIDDEN,
                SECRET_JOIN_BODY.to_owned(),
            ));
        }
    }
    if !project_ids.is_empty() {
        // Reactivate live rows (`QuerySet.update` stamps no `updated_at`).
        sqlx::query(
            r#"UPDATE project_members pm SET is_active = true
               FROM workspaces w
               WHERE pm.workspace_id = w.id AND w.slug = $1 AND pm.project_id = ANY($2)
                 AND pm.member_id = $3 AND pm.deleted_at IS NULL"#,
        )
        .bind(&slug)
        .bind(&project_ids)
        .bind(resolved.id)
        .execute(&pool)
        .await
        .map_err(|_| Denial::ServerError)?;
        // `bulk_create(..., ignore_conflicts=True)` — ONE statement
        // each (a single bad row fails the whole insert),
        // `ON CONFLICT DO NOTHING`, `created_by` stamped,
        // `updated_by` NULL; a constraint failure (e.g. an unknown
        // project's FK) is the `IntegrityError` 400. `bulk_create`
        // never calls `save()`, so no user-property side effect rides
        // the member insert.
        bulk_insert_members(
            &pool,
            &project_ids,
            &workspace_id,
            &resolved.id,
            workspace_role,
        )
        .await?;
        bulk_insert_user_properties(&pool, &project_ids, &workspace_id, &resolved.id).await?;
    }
    Ok(json_response(
        StatusCode::CREATED,
        PROJECTS_JOINED_BODY.to_owned(),
    ))
}

// ---------------------------------------------------------------------------
// Handlers: project join
// ---------------------------------------------------------------------------

/// `GET .../projects/<project_id>/join/<uuid:pk>/` (`invite.py:251-254`,
/// `AllowAny`): the invite shape, or the `.get()` 404. Anonymous callers
/// skip the rewrite (rendering in UTC); authenticated callers rewrite
/// identifiers first.
async fn join_get(
    State(state): State<AppState>,
    Path((slug, project_raw, pk)): Path<(String, String, String)>,
    extension: Option<axum::Extension<SessionHandle>>,
    req: axum::extract::Request,
) -> Result<Response, Denial> {
    if pk.parse::<uuid::Uuid>().is_err() {
        return Ok(crate::edge::proxy(State(state), req).await);
    }
    let pool = pool_of(&state)?.clone();
    let caller = optional_actor(&state, extension).await?;
    let timezone = caller
        .as_ref()
        .map(|actor| actor.timezone)
        .unwrap_or(chrono_tz::UTC);
    let project_id = match &caller {
        Some(_) => resolve_project_id(&pool, &slug, &project_raw).await?,
        None => project_raw
            .parse::<uuid::Uuid>()
            .map_err(|_| Denial::BadValidation)?,
    };
    let pk = pk.parse::<uuid::Uuid>().map_err(|_| Denial::ServerError)?;
    let row = fetch_join_invite(&pool, &slug, &project_id, &pk).await?;
    let mut asset_ids = Vec::new();
    invite_asset_ids(&row, &mut asset_ids);
    let assets = fetch_asset_urls(&pool, &asset_ids).await?;
    let (_, body) = render_invite(&row, &timezone, &assets);
    Ok(json_response(
        StatusCode::OK,
        serde_json::to_string(&body).expect("invite serializes"),
    ))
}

/// `POST .../projects/<project_id>/join/<uuid:pk>/` (`invite.py:186-249`,
/// `AllowAny`): the email check 403s first, then a repeat response 400s,
/// then the response is recorded and an accept creates/reactivates the
/// workspace + project memberships (with the member-save user-property
/// side effect). A decline stops after the record.
async fn join_post(
    State(state): State<AppState>,
    Path((slug, project_raw, pk)): Path<(String, String, String)>,
    extension: Option<axum::Extension<SessionHandle>>,
    req: axum::extract::Request,
) -> Result<Response, Denial> {
    if pk.parse::<uuid::Uuid>().is_err() {
        return Ok(crate::edge::proxy(State(state), req).await);
    }
    let pool = pool_of(&state)?.clone();
    let caller = optional_actor(&state, extension).await?;
    let project_id = match &caller {
        Some(_) => resolve_project_id(&pool, &slug, &project_raw).await?,
        None => project_raw
            .parse::<uuid::Uuid>()
            .map_err(|_| Denial::BadValidation)?,
    };
    let body = match parse_body(&state, req).await {
        Ok(body) => body,
        Err(response) => return Ok(response),
    };
    // `request.data.get(...)` — a non-dict body has no `.get`.
    let data = match body.as_object() {
        Some(map) => map,
        None => return Err(Denial::ServerError),
    };
    let pk = pk.parse::<uuid::Uuid>().map_err(|_| Denial::ServerError)?;
    let invite = fetch_join_invite(&pool, &slug, &project_id, &pk).await?;
    // `email == "" or invite.email != email` — plain `!=`, so a missing
    // or mistyped email 403s before anything else.
    let email = data.get("email").and_then(Value::as_str).unwrap_or("");
    if email.is_empty() || invite.email != email {
        return Ok(json_response(
            StatusCode::FORBIDDEN,
            WRONG_EMAIL_BODY.to_owned(),
        ));
    }
    if invite.responded_at.is_some() {
        return Ok(json_response(
            StatusCode::BAD_REQUEST,
            ALREADY_RESPONDED_BODY.to_owned(),
        ));
    }
    let accepted = prep_bool(data.get("accepted").unwrap_or(&Value::Bool(false)))?;
    let created_by = caller.as_ref().map(|actor| actor.id);
    // Record the response (`responded_at` + `accepted`, `updated_by` via
    // crum — `NULL` for anonymous joins). An explicit-null `accepted`
    // falls to the NOT NULL column: the `IntegrityError` 400.
    sqlx::query(
        "UPDATE project_member_invites
         SET accepted = $1, responded_at = now(), updated_at = now(), updated_by_id = $2
         WHERE id = $3",
    )
    .bind(accepted)
    .bind(created_by)
    .bind(pk)
    .execute(&pool)
    .await
    .map_err(integrity_denial)?;
    if accepted != Some(true) {
        return Ok(json_response(StatusCode::OK, JOIN_DECLINED_BODY.to_owned()));
    }
    // The account lookup: `User.objects.filter(email=...)` (no
    // soft-delete manager on users), `-created_at` first. A missing user
    // flows into the workspace insert as `NULL` and 400s there.
    let user: Option<(uuid::Uuid,)> =
        sqlx::query_as("SELECT id FROM users WHERE email = $1 ORDER BY created_at DESC LIMIT 1")
            .bind(email)
            .fetch_optional(&pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    let user_id = user.map(|row| row.0);
    let ws_member: Option<(uuid::Uuid,)> = sqlx::query_as(
        r#"SELECT wm.id FROM workspace_members wm
           JOIN workspaces w ON w.id = wm.workspace_id
           WHERE w.slug = $1 AND wm.member_id IS NOT DISTINCT FROM $2 AND wm.deleted_at IS NULL
           ORDER BY wm.created_at DESC LIMIT 1"#,
    )
    .bind(&slug)
    .bind(user_id)
    .fetch_optional(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    match ws_member {
        None => {
            let role = if i32::from(invite.role) >= 15 {
                15i16
            } else {
                invite.role
            };
            sqlx::query(
                "INSERT INTO workspace_members (id, workspace_id, member_id, role,
                    company_role, view_props, default_props, issue_props, is_active,
                    getting_started_checklist, tips, explored_features,
                    created_by_id, updated_by_id, created_at, updated_at, deleted_at)
                 VALUES ($1, $2, $3, $4,
                    NULL, $5::jsonb, $5::jsonb, $6::jsonb, true,
                    '{}', '{}', '{}',
                    $7, NULL, now(), now(), NULL)",
            )
            .bind(uuid::Uuid::new_v4())
            .bind(invite.workspace_id)
            .bind(user_id)
            .bind(role)
            .bind(default_workspace_props_json())
            .bind(default_issue_props_json())
            .bind(created_by)
            .execute(&pool)
            .await
            .map_err(integrity_denial)?;
        }
        Some((id,)) => {
            sqlx::query(
                "UPDATE workspace_members SET is_active = true, updated_at = now(), updated_by_id = $1
                 WHERE id = $2",
            )
            .bind(created_by)
            .bind(id)
            .execute(&pool)
            .await
            .map_err(|_| Denial::ServerError)?;
        }
    }
    // The existing project membership is looked up by workspace only —
    // no project predicate (`invite.py:221-223`).
    let project_member: Option<(uuid::Uuid,)> = sqlx::query_as(
        "SELECT id FROM project_members
         WHERE workspace_id = $1 AND member_id IS NOT DISTINCT FROM $2 AND deleted_at IS NULL
         ORDER BY created_at DESC LIMIT 1",
    )
    .bind(invite.workspace_id)
    .bind(user_id)
    .fetch_optional(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    match project_member {
        None => {
            create_project_member(&pool, &project_id, user_id, invite.role, created_by).await?;
        }
        Some((id,)) => {
            // `project_member.role = project_member.role`: the role
            // re-saves unchanged (ported quirk).
            sqlx::query(
                "UPDATE project_members SET is_active = true, updated_at = now(), updated_by_id = $1
                 WHERE id = $2",
            )
            .bind(created_by)
            .bind(id)
            .execute(&pool)
            .await
            .map_err(|_| Denial::ServerError)?;
        }
    }
    Ok(json_response(StatusCode::OK, JOIN_ACCEPTED_BODY.to_owned()))
}

// ---------------------------------------------------------------------------
// Handlers: favorites
// ---------------------------------------------------------------------------

/// `GET .../user-favorite-projects/` (`base.py:503-514`): the DRF-default
/// list raises `AssertionError` — the viewset inherits
/// `serializer_class = None` — so the endpoint always answers the generic
/// 500 (FX-APROJ-06/08), after the auth-only gate.
async fn fav_list(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    extension: Option<axum::Extension<SessionHandle>>,
) -> Result<Response, Denial> {
    let resolved = actor(&state, extension).await?;
    let pool = pool_of(&state)?.clone();
    check_gate(
        &pool,
        &slug,
        None,
        &resolved.id,
        "GET",
        "workspaces/<slug>/user-favorite-projects/",
    )
    .await?;
    Err(Denial::ServerError)
}

/// `POST .../user-favorite-projects/` (`base.py:519-526`): create the
/// caller's project favorite — `sequence` is `max + 10000` over the live
/// project-workspace favorites (65535 when none), the workspace backfills
/// from the project — answering 204. A bad-UUID `project` is the
/// `ValidationError` 400 (raised by the `self.project` fetch prep); a
/// missing project row is the `.get()` 404; a live duplicate is the
/// `IntegrityError` 400.
async fn fav_create(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    extension: Option<axum::Extension<SessionHandle>>,
    req: axum::extract::Request,
) -> Result<Response, Denial> {
    let resolved = actor(&state, extension).await?;
    let pool = pool_of(&state)?.clone();
    check_gate(
        &pool,
        &slug,
        None,
        &resolved.id,
        "POST",
        "workspaces/<slug>/user-favorite-projects/",
    )
    .await?;
    let body = match parse_body(&state, req).await {
        Ok(body) => body,
        Err(response) => return Ok(response),
    };
    // `request.data.get("project")` — a non-dict body has no `.get`.
    let data = match body.as_object() {
        Some(map) => map,
        None => return Err(Denial::ServerError),
    };
    let project_id = match data.get("project") {
        // A missing `project` 404s: the `else` branch touches
        // `self.workspace` with a null FK, raising
        // `RelatedObjectDoesNotExist` (live probe 2026-10-03).
        None | Some(Value::Null) => return Err(Denial::ObjectNotFound),
        Some(value) => prep_uuid(value)?,
    };
    // `UserFavorite.save`: `if self.project` fetches the row (miss →
    // 404), then the live-workspace max sequence decides the insert's
    // `sequence`; `WorkspaceBaseModel.save` backfills the workspace from
    // the project.
    let row: Option<(uuid::Uuid,)> =
        sqlx::query_as("SELECT workspace_id FROM projects WHERE id = $1 AND deleted_at IS NULL")
            .bind(project_id)
            .fetch_optional(&pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    let Some((workspace_id,)) = row else {
        return Err(Denial::ObjectNotFound);
    };
    let largest: Option<f64> = sqlx::query_scalar(
        "SELECT MAX(sequence) FROM user_favorites
         WHERE workspace_id IS NOT DISTINCT FROM $1 AND deleted_at IS NULL",
    )
    .bind(workspace_id)
    .fetch_one(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let sequence = largest.map(|max| max + 10000.0).unwrap_or(65535.0);
    sqlx::query(
        "INSERT INTO user_favorites (id, workspace_id, project_id, user_id, entity_type,
            entity_identifier, name, is_folder, sequence, parent_id,
            created_by_id, updated_by_id, created_at, updated_at, deleted_at)
         VALUES ($1, $2, $3, $4, 'project',
            $5, NULL, false, $6, NULL,
            $7, NULL, now(), now(), NULL)",
    )
    .bind(uuid::Uuid::new_v4())
    .bind(workspace_id)
    .bind(project_id)
    .bind(resolved.id)
    .bind(project_id)
    .bind(sequence)
    .bind(resolved.id)
    .execute(&pool)
    .await
    .map_err(integrity_denial)?;
    Ok(empty_response(StatusCode::NO_CONTENT))
}

/// `DELETE .../user-favorite-projects/<project_id>/` (`base.py:528-537`):
/// hard-delete (`soft=False`) the caller's matching favorite, 204. The
/// `<str:project_id>` rewrites like any project kwarg; a miss is the
/// `.get()` 404.
async fn fav_destroy(
    State(state): State<AppState>,
    Path((slug, project_raw)): Path<(String, String)>,
    extension: Option<axum::Extension<SessionHandle>>,
) -> Result<Response, Denial> {
    let resolved = actor(&state, extension).await?;
    let pool = pool_of(&state)?.clone();
    let project_id = resolve_project_id(&pool, &slug, &project_raw).await?;
    check_gate(
        &pool,
        &slug,
        Some(&project_id),
        &resolved.id,
        "DELETE",
        "workspaces/<slug>/user-favorite-projects/<project_id>/",
    )
    .await?;
    let rows: Vec<(uuid::Uuid,)> = sqlx::query_as(
        r#"SELECT f.id FROM user_favorites f
           JOIN workspaces w ON w.id = f.workspace_id
           WHERE f.entity_identifier = $1 AND f.entity_type = 'project' AND f.project_id = $1
             AND f.user_id = $2 AND w.slug = $3 AND f.deleted_at IS NULL"#,
    )
    .bind(project_id)
    .bind(resolved.id)
    .bind(&slug)
    .fetch_all(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    if rows.len() > 1 {
        return Err(Denial::ServerError);
    }
    let Some((id,)) = rows.first() else {
        return Err(Denial::ObjectNotFound);
    };
    sqlx::query("DELETE FROM user_favorites WHERE id = $1")
        .bind(id)
        .execute(&pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(empty_response(StatusCode::NO_CONTENT))
}

// ---------------------------------------------------------------------------
// Handlers: deploy boards
// ---------------------------------------------------------------------------

/// `GET .../projects/<project_id>/project-deploy-boards/`
/// (`base.py:544-551`, `ProjectMemberPermission`-gated): the latest live
/// project-scoped row, or the 12-key initial shape when none exists.
async fn board_list(
    State(state): State<AppState>,
    Path((slug, project_raw)): Path<(String, String)>,
    extension: Option<axum::Extension<SessionHandle>>,
) -> Result<Response, Denial> {
    let resolved = actor(&state, extension).await?;
    let pool = pool_of(&state)?.clone();
    let project_id = resolve_project_id(&pool, &slug, &project_raw).await?;
    check_board_gate(&pool, &slug, &project_id, &resolved.id, "GET").await?;
    let row = fetch_board_first(&pool, &slug, &project_id).await?;
    match row {
        None => Ok(json_response(
            StatusCode::OK,
            DEPLOY_BOARD_INITIAL_BODY.to_owned(),
        )),
        Some((row, project_id, intake_id)) => {
            let mut asset_ids = Vec::new();
            board_asset_ids(&row, &mut asset_ids);
            let assets = fetch_asset_urls(&pool, &asset_ids).await?;
            Ok(json_response(
                StatusCode::OK,
                render_board(
                    &row,
                    project_id,
                    intake_id,
                    &resolved.timezone,
                    &assets,
                    None,
                ),
            ))
        }
    }
}

/// `POST .../projects/<project_id>/project-deploy-boards/`
/// (`base.py:553-581`, `ProjectMemberPermission`-gated): the unscoped
/// `get_or_create`, then the flags / views assignment and a second save
/// (so `updated_by` is always stamped), answering 200 — never 201. A
/// missing project row is the `.get()` 404 (raised by the workspace
/// backfill); a bad-UUID flag is the `ValidationError` 400; an
/// explicit-null flag or view falls to its NOT NULL column (the
/// `IntegrityError` 400); any non-null `intake` raises `ValueError` on
/// the FK-descriptor assignment (the generic 500).
async fn board_create(
    State(state): State<AppState>,
    Path((slug, project_raw)): Path<(String, String)>,
    extension: Option<axum::Extension<SessionHandle>>,
    req: axum::extract::Request,
) -> Result<Response, Denial> {
    let resolved = actor(&state, extension).await?;
    let pool = pool_of(&state)?.clone();
    let project_id = resolve_project_id(&pool, &slug, &project_raw).await?;
    check_board_gate(&pool, &slug, &project_id, &resolved.id, "POST").await?;
    let body = match parse_body(&state, req).await {
        Ok(body) => body,
        Err(response) => return Ok(response),
    };
    // `request.data.get(...)` — a non-dict body has no `.get`.
    let data = match body.as_object() {
        Some(map) => map,
        None => return Err(Denial::ServerError),
    };
    let comments = prep_bool_flag(data.get("is_comments_enabled"))?;
    let reactions = prep_bool_flag(data.get("is_reactions_enabled"))?;
    let votes = prep_bool_flag(data.get("is_votes_enabled"))?;
    // `project_deploy_board.intake = ...` assigns through the FK
    // descriptor, which only accepts an `Intake` instance or `None` —
    // any JSON value raises `ValueError` (live probe 2026-10-03), so
    // only a missing or null `intake` reaches the write.
    if !matches!(data.get("intake"), None | Some(Value::Null)) {
        return Err(Denial::ServerError);
    }
    let views = match data.get("views") {
        None => serde_json::from_str(&default_board_views_json()).expect("default views parse"),
        Some(value) => value.clone(),
    };
    // The workspace backfill (`WorkspaceBaseModel.save`) fetches the
    // project first: a miss is the 404, before any board row is read.
    let workspace: Option<(uuid::Uuid,)> =
        sqlx::query_as("SELECT workspace_id FROM projects WHERE id = $1 AND deleted_at IS NULL")
            .bind(project_id)
            .fetch_optional(&pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    let Some((workspace_id,)) = workspace else {
        return Err(Denial::ObjectNotFound);
    };
    // `get_or_create(entity_name, entity_identifier, project_id)` —
    // unscoped (no workspace predicate), live manager only.
    let existing: Option<(uuid::Uuid,)> = sqlx::query_as(
        "SELECT id FROM deploy_boards
         WHERE entity_name = 'project' AND entity_identifier = $1 AND project_id = $2
           AND deleted_at IS NULL",
    )
    .bind(project_id)
    .bind(project_id)
    .fetch_optional(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let board_id = match existing {
        Some((id,)) => id,
        None => {
            let id = uuid::Uuid::new_v4();
            let insert = sqlx::query(
                "INSERT INTO deploy_boards (id, workspace_id, project_id, entity_identifier,
                    entity_name, anchor, is_comments_enabled, is_reactions_enabled, intake_id,
                    is_votes_enabled, view_props, is_activity_enabled, is_disabled,
                    created_by_id, updated_by_id, created_at, updated_at, deleted_at)
                 VALUES ($1, $2, $3, $4,
                    'project', $5, false, false, NULL,
                    false, '{}', true, false,
                    $6, NULL, now(), now(), NULL)",
            )
            .bind(id)
            .bind(workspace_id)
            .bind(project_id)
            .bind(project_id)
            .bind(uuid::Uuid::new_v4().simple().to_string())
            .bind(resolved.id)
            .execute(&pool)
            .await;
            match insert {
                Ok(_) => id,
                Err(err) => {
                    if !is_integrity_error(&err) {
                        return Err(Denial::ServerError);
                    }
                    // The `get_or_create` retry: one more `get`, whose
                    // miss raises like the first.
                    let retry: Option<(uuid::Uuid,)> = sqlx::query_as(
                        "SELECT id FROM deploy_boards
                         WHERE entity_name = 'project' AND entity_identifier = $1 AND project_id = $2
                           AND deleted_at IS NULL",
                    )
                    .bind(project_id)
                    .bind(project_id)
                    .fetch_optional(&pool)
                    .await
                    .map_err(|_| Denial::ServerError)?;
                    let Some((id,)) = retry else {
                        return Err(Denial::ObjectNotFound);
                    };
                    id
                }
            }
        }
    };
    // The assignment + second save: flags, views and intake land here
    // (explicit nulls fall to their NOT NULL columns → 400), and
    // `updated_by`/`updated_at` stamp on both the created and the found
    // path.
    let views_text = serde_json::to_string(&views).expect("views serialize");
    sqlx::query(
        "UPDATE deploy_boards
         SET intake_id = $1, view_props = $2::jsonb, is_votes_enabled = $3,
             is_comments_enabled = $4, is_reactions_enabled = $5,
             updated_at = now(), updated_by_id = $6
         WHERE id = $7",
    )
    .bind(None::<uuid::Uuid>)
    .bind(if views.is_null() {
        None
    } else {
        Some(views_text)
    })
    .bind(votes)
    .bind(comments)
    .bind(reactions)
    .bind(resolved.id)
    .bind(board_id)
    .execute(&pool)
    .await
    .map_err(integrity_denial)?;
    let (row, board_project_id, intake_id) = fetch_board_global(&pool, &board_id).await?;
    let mut asset_ids = Vec::new();
    board_asset_ids(&row, &mut asset_ids);
    let assets = fetch_asset_urls(&pool, &asset_ids).await?;
    Ok(json_response(
        StatusCode::OK,
        render_board(
            &row,
            board_project_id,
            intake_id,
            &resolved.timezone,
            &assets,
            Some(&views),
        ),
    ))
}

/// `request.data.get(<flag>, False)` through model-bool prep: missing is
/// `False`; explicit `null` stays `None` (the NOT NULL column raises the
/// `IntegrityError` 400 on the write); anything else preps or is the
/// `ValidationError` 400.
fn prep_bool_flag(value: Option<&Value>) -> Result<Option<bool>, Denial> {
    match value {
        None => Ok(Some(false)),
        Some(value) => prep_bool(value),
    }
}

/// `GET .../project-deploy-boards/<uuid:pk>/` (`base.py:540-543`):
/// DRF-default retrieve over the *global* board queryset (no
/// `get_queryset` override); a miss is Django's `No DeployBoard matches` 404.
async fn board_retrieve(
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
    let project_id = resolve_project_id(&pool, &slug, &project_raw).await?;
    check_board_gate(&pool, &slug, &project_id, &resolved.id, "GET").await?;
    let pk = pk.parse::<uuid::Uuid>().map_err(|_| Denial::ServerError)?;
    let (row, board_project_id, intake_id) = fetch_board_global(&pool, &pk).await?;
    let mut asset_ids = Vec::new();
    board_asset_ids(&row, &mut asset_ids);
    let assets = fetch_asset_urls(&pool, &asset_ids).await?;
    Ok(json_response(
        StatusCode::OK,
        render_board(
            &row,
            board_project_id,
            intake_id,
            &resolved.timezone,
            &assets,
            None,
        ),
    ))
}

/// `DELETE .../project-deploy-boards/<uuid:pk>/` (`base.py:540-543`):
/// DRF-default destroy — global lookup, soft delete, deferred
/// related-objects publish, 204.
async fn board_destroy(
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
    let project_id = resolve_project_id(&pool, &slug, &project_raw).await?;
    check_board_gate(&pool, &slug, &project_id, &resolved.id, "DELETE").await?;
    let pk = pk.parse::<uuid::Uuid>().map_err(|_| Denial::ServerError)?;
    fetch_board_global(&pool, &pk).await?;
    sqlx::query(
        "UPDATE deploy_boards SET deleted_at = now(), updated_at = now(), updated_by_id = $1
         WHERE id = $2 AND deleted_at IS NULL",
    )
    .bind(resolved.id)
    .bind(pk)
    .execute(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    enqueue_message(&pool, soft_delete_message("deployboard", &pk)).await;
    Ok(empty_response(StatusCode::NO_CONTENT))
}

// ---------------------------------------------------------------------------
// Board partial_update validation (`DeployBoardSerializer`, partial)
// ---------------------------------------------------------------------------

/// DRF `DateTimeField` wrong-format message
/// (`DATETIME_INPUT_FORMATS = ['iso-8601']`).
const DATETIME_FORMAT_HINT: &str = "YYYY-MM-DDThh:mm[:ss[.uuuuuu]][+HH:MM|-HH:MM|Z]";
/// `entity_name` max length (`CharField(max_length=30)`).
const ENTITY_NAME_MAX_LENGTH: usize = 30;

/// One validated PATCH assignment: the column plus its bound value.
enum PatchValue {
    Null,
    Bool(bool),
    Uuid(uuid::Uuid),
    Text(String),
    Json(String),
    Timestamp(chrono::DateTime<chrono::Utc>),
}

/// `DeployBoardSerializer` field errors in writable-field order:
/// `deleted_at`, `entity_identifier`, `entity_name`, the three flags,
/// `view_props`, `is_activity_enabled`, `is_disabled`, `created_by`,
/// `updated_by`, `intake`. Unknown and read-only keys are ignored;
/// every failure is collected into the one 400 dict.
struct BoardPatchErrors {
    errors: Vec<(&'static str, String)>,
}

impl BoardPatchErrors {
    fn new() -> Self {
        BoardPatchErrors { errors: Vec::new() }
    }

    fn push(&mut self, field: &'static str, message: String) {
        self.errors.push((field, message));
    }

    fn is_empty(&self) -> bool {
        self.errors.is_empty()
    }

    /// DRF's `errors` dict: one key per field, messages in push order
    /// (`non_field_errors` can carry both uniqueness messages).
    fn body(&self) -> String {
        let mut fields: Vec<&str> = Vec::new();
        for (field, _) in &self.errors {
            if !fields.contains(field) {
                fields.push(field);
            }
        }
        let mut out = String::from("{");
        for (index, field) in fields.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            let messages: Vec<String> = self
                .errors
                .iter()
                .filter(|(name, _)| name == field)
                .map(|(_, message)| json_string(message))
                .collect();
            out.push_str(&format!("{}:[{}]", json_string(field), messages.join(",")));
        }
        out.push('}');
        out
    }
}

/// DRF `BooleanField.to_internal_value` (no `allow_null`): the TRUE /
/// FALSE sets, matched case-insensitively for strings (`1.0`/`0.0` ride
/// `==`); explicit `null` is the `null` error, anything else `invalid`.
fn validate_patch_bool(value: &Value) -> Result<bool, &'static str> {
    if value.is_null() {
        return Err("This field may not be null.");
    }
    let lower = match value {
        Value::Bool(flag) => return Ok(*flag),
        Value::Number(number) => {
            if number.as_i64() == Some(1) || number.as_u64() == Some(1) {
                return Ok(true);
            }
            if number.as_i64() == Some(0) || number.as_u64() == Some(0) {
                return Ok(false);
            }
            if number.as_f64() == Some(1.0) {
                return Ok(true);
            }
            if number.as_f64() == Some(0.0) {
                return Ok(false);
            }
            return Err("Must be a valid boolean.");
        }
        Value::String(raw) => raw.to_lowercase(),
        _ => return Err("Must be a valid boolean."),
    };
    match lower.as_str() {
        "t" | "y" | "yes" | "true" | "on" | "1" => Ok(true),
        "f" | "n" | "no" | "false" | "off" | "0" => Ok(false),
        _ => Err("Must be a valid boolean."),
    }
}

/// DRF `UUIDField.to_internal_value` with `allow_null`: ints (bools are
/// ints) take the `int=` form, strings the `hex=` form, `None` stays.
fn validate_patch_uuid(value: &Value) -> Result<Option<uuid::Uuid>, &'static str> {
    if value.is_null() {
        return Ok(None);
    }
    match value {
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                if int >= 0 {
                    return Ok(Some(uuid::Uuid::from_u128(int as u128)));
                }
            } else if let Some(uint) = number.as_u64() {
                return Ok(Some(uuid::Uuid::from_u128(uint as u128)));
            }
            Err("Must be a valid UUID.")
        }
        Value::String(raw) => raw
            .parse::<uuid::Uuid>()
            .map(Some)
            .map_err(|_| "Must be a valid UUID."),
        Value::Bool(flag) => Ok(Some(uuid::Uuid::from_u128(u128::from(*flag as u8)))),
        _ => Err("Must be a valid UUID."),
    }
}

/// DRF `CharField.to_internal_value` + `MaxLengthValidator` for
/// `entity_name`: bools and composites fail, numerics stringify, the
/// value strips (`trim_whitespace`), blanks pass (`allow_blank`),
/// over-30s fail.
fn validate_patch_name(value: &Value) -> Result<Option<String>, &'static str> {
    if value.is_null() {
        return Ok(None);
    }
    let text = match value {
        Value::Bool(_) => return Err("Not a valid string."),
        Value::String(raw) => raw.clone(),
        Value::Number(number) => number.to_string(),
        _ => return Err("Not a valid string."),
    };
    let text = text.trim().to_owned();
    if text.chars().count() > ENTITY_NAME_MAX_LENGTH {
        return Err("Ensure this field has no more than 30 characters.");
    }
    Ok(Some(text))
}

/// DRF `DateTimeField.to_internal_value` (`iso-8601`, `allow_null`) for
/// `deleted_at`: datetimes parse (RFC 3339 plus the space-separated and
/// minute-precision spellings Django accepts), naive values assume UTC
/// (`USE_TZ`, `TIME_ZONE = "UTC"`).
fn validate_patch_datetime(value: &Value) -> Result<Option<chrono::DateTime<chrono::Utc>>, String> {
    if value.is_null() {
        return Ok(None);
    }
    let raw = match value {
        Value::String(raw) => raw,
        _ => return Err(datetime_format_error()),
    };
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(raw) {
        return Ok(Some(dt.with_timezone(&chrono::Utc)));
    }
    for format in [
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%dT%H:%M",
        "%Y-%m-%d %H:%M",
    ] {
        if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(raw, format) {
            return Ok(Some(naive.and_utc()));
        }
    }
    Err(datetime_format_error())
}

fn datetime_format_error() -> String {
    format!("Datetime has wrong format. Use one of these formats instead: {DATETIME_FORMAT_HINT}.")
}

/// Python `str()` of a JSON value, for the `does_not_exist` message and
/// the UUID `invalid` message's `%(value)s` (dict keys and nested
/// strings render `repr`-style with single quotes).
fn python_str(value: &Value) -> String {
    match value {
        Value::String(raw) => raw.clone(),
        Value::Bool(flag) => {
            if *flag {
                "True".to_owned()
            } else {
                "False".to_owned()
            }
        }
        Value::Number(number) => python_number(number),
        Value::Null => "None".to_owned(),
        Value::Array(items) => {
            let inner: Vec<String> = items.iter().map(python_repr).collect();
            format!("[{}]", inner.join(", "))
        }
        Value::Object(map) => {
            let inner: Vec<String> = map
                .iter()
                .map(|(key, val)| {
                    format!(
                        "{}: {}",
                        python_repr(&Value::String(key.clone())),
                        python_str(val)
                    )
                })
                .collect();
            format!("{{{}}}", inner.join(", "))
        }
    }
}

/// Python `repr()` of a JSON string (single quotes unless the value
/// holds one — then double; backslashes and the quote char escape).
fn python_repr(value: &Value) -> String {
    match value {
        Value::String(raw) => {
            if !raw.contains('\'') {
                format!("'{}'", raw.replace('\\', "\\\\"))
            } else if !raw.contains('"') {
                format!("\"{}\"", raw.replace('\\', "\\\\"))
            } else {
                format!("'{}'", raw.replace('\\', "\\\\").replace('\'', "\\'"))
            }
        }
        Value::Bool(flag) => {
            if *flag {
                "True".to_owned()
            } else {
                "False".to_owned()
            }
        }
        Value::Number(number) => python_number(number),
        Value::Null => "None".to_owned(),
        other => python_str(other),
    }
}

/// Python `str()` of a JSON number: ints print exactly (the
/// `arbitrary_precision` literal is the digits); floats print shortest
/// with a `.0` when integral (exotic magnitudes diverge from CPython's
/// exponent padding — unreachable in practice).
fn python_number(number: &serde_json::Number) -> String {
    if let Some(int) = number.as_i64() {
        return int.to_string();
    }
    if let Some(uint) = number.as_u64() {
        return uint.to_string();
    }
    match number.as_f64() {
        Some(float) => {
            let text = float.to_string();
            if text.contains(['.', 'e', 'E', 'n']) {
                text
            } else {
                format!("{text}.0")
            }
        }
        None => number.to_string(),
    }
}

/// Django 4.2 `UUIDField` `invalid` message (curly quotes).
fn uuid_invalid_message(value: &Value) -> String {
    format!("\u{201c}{}\u{201d} is not a valid UUID.", python_str(value))
}

/// Python `type(x).__name__`, for the non-dict PATCH body message.
fn python_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "NoneType",
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
        Value::Object(_) => "dict",
    }
}

/// `PrimaryKeyRelatedField.to_internal_value` for the `created_by` /
/// `updated_by` / `intake` PATCH keys: `None` stays (all three allow
/// null); bools fail `incorrect_type` outright (DRF raises `TypeError`
/// before the lookup); strings and ints prep as UUIDs — a prep failure
/// is Django's curly-quote `invalid` message *as a field error* (DRF
/// converts the escaped `ValidationError` via `get_error_detail`, live
/// probe 2026-10-03), and a prep success must exist in the queryset or
/// fail `does_not_exist` (the `users` table has no soft-delete column;
/// `intakes` is live-only).
enum PkValidation {
    Null,
    Id(uuid::Uuid),
    FieldError(String),
}

async fn validate_patch_pk(
    pool: &sqlx::PgPool,
    table: &str,
    live_only: bool,
    value: &Value,
) -> Result<PkValidation, Denial> {
    if value.is_null() {
        return Ok(PkValidation::Null);
    }
    if value.is_boolean() {
        return Ok(PkValidation::FieldError(
            "Incorrect type. Expected pk value, received bool.".to_owned(),
        ));
    }
    let id = match prep_patch_pk_uuid(value) {
        Some(id) => id,
        None => return Ok(PkValidation::FieldError(uuid_invalid_message(value))),
    };
    let sql = if live_only {
        format!("SELECT id FROM {table} WHERE id = $1 AND deleted_at IS NULL")
    } else {
        format!("SELECT id FROM {table} WHERE id = $1")
    };
    let row: Option<(uuid::Uuid,)> = sqlx::query_as(&sql)
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    match row {
        Some(_) => Ok(PkValidation::Id(id)),
        None => Ok(PkValidation::FieldError(format!(
            "Invalid pk \"{}\" - object does not exist.",
            python_str(value)
        ))),
    }
}

/// UUID prep for PATCH PK inputs: strings parse, ints (except bools,
/// rejected earlier) take the `int=` form; anything else fails.
fn prep_patch_pk_uuid(value: &Value) -> Option<uuid::Uuid> {
    match value {
        Value::String(raw) => raw.parse::<uuid::Uuid>().ok(),
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                if int >= 0 {
                    return Some(uuid::Uuid::from_u128(int as u128));
                }
            } else if let Some(uint) = number.as_u64() {
                return Some(uuid::Uuid::from_u128(uint as u128));
            }
            None
        }
        _ => None,
    }
}

/// `PATCH .../project-deploy-boards/<uuid:pk>/` (`base.py:540-543`):
/// DRF-default partial update over the global board queryset — lookup,
/// serializer validation (one collected 400 dict), save, 200 with the
/// full shape. An empty PATCH still stamps `updated_at`/`updated_by`.
async fn board_partial_update(
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
    let project_id = resolve_project_id(&pool, &slug, &project_raw).await?;
    check_board_gate(&pool, &slug, &project_id, &resolved.id, "PATCH").await?;
    let pk = pk.parse::<uuid::Uuid>().map_err(|_| Denial::ServerError)?;
    let (current, _, _) = fetch_board_global(&pool, &pk).await?;
    let body = match parse_body(&state, req).await {
        Ok(body) => body,
        Err(response) => return Ok(response),
    };
    // `Serializer.to_internal_value` on a non-dict: `non_field_errors`
    // with the Python type name.
    let data = match body.as_object() {
        Some(map) => map,
        None => {
            let message = format!(
                "Invalid data. Expected a dictionary, but got {}.",
                python_type_name(&body)
            );
            let mut errors = BoardPatchErrors::new();
            errors.push("non_field_errors", message);
            return Err(Denial::Raw(StatusCode::BAD_REQUEST, errors.body()));
        }
    };
    let mut errors = BoardPatchErrors::new();
    let mut sets: Vec<(&'static str, PatchValue)> = Vec::new();
    // The just-assigned views render in-memory (user key order), like Django.
    let views_override = data.get("view_props").cloned();
    if let Some(value) = data.get("deleted_at") {
        match validate_patch_datetime(value) {
            Ok(parsed) => sets.push((
                "deleted_at",
                match parsed {
                    Some(dt) => PatchValue::Timestamp(dt),
                    None => PatchValue::Null,
                },
            )),
            Err(message) => errors.push("deleted_at", message),
        }
    }
    if let Some(value) = data.get("entity_identifier") {
        match validate_patch_uuid(value) {
            Ok(parsed) => sets.push((
                "entity_identifier",
                match parsed {
                    Some(id) => PatchValue::Uuid(id),
                    None => PatchValue::Null,
                },
            )),
            Err(message) => errors.push("entity_identifier", message.to_owned()),
        }
    }
    if let Some(value) = data.get("entity_name") {
        match validate_patch_name(value) {
            Ok(parsed) => sets.push((
                "entity_name",
                match parsed {
                    Some(text) => PatchValue::Text(text),
                    None => PatchValue::Null,
                },
            )),
            Err(message) => errors.push("entity_name", message.to_owned()),
        }
    }
    for (field, column) in [
        ("is_comments_enabled", "is_comments_enabled"),
        ("is_reactions_enabled", "is_reactions_enabled"),
        ("is_votes_enabled", "is_votes_enabled"),
    ] {
        if let Some(value) = data.get(field) {
            match validate_patch_bool(value) {
                Ok(flag) => sets.push((column, PatchValue::Bool(flag))),
                Err(message) => errors.push(field, message.to_owned()),
            }
        }
    }
    if let Some(value) = data.get("view_props") {
        if value.is_null() {
            errors.push("view_props", "This field may not be null.".to_owned());
        } else {
            sets.push((
                "view_props",
                PatchValue::Json(serde_json::to_string(value).expect("view_props serialize")),
            ));
        }
    }
    for (field, column) in [
        ("is_activity_enabled", "is_activity_enabled"),
        ("is_disabled", "is_disabled"),
    ] {
        if let Some(value) = data.get(field) {
            match validate_patch_bool(value) {
                Ok(flag) => sets.push((column, PatchValue::Bool(flag))),
                Err(message) => errors.push(field, message.to_owned()),
            }
        }
    }
    for (field, column, table, live_only) in [
        ("created_by", "created_by_id", "users", false),
        ("updated_by", "updated_by_id", "users", false),
        ("intake", "intake_id", "intakes", true),
    ] {
        if let Some(value) = data.get(field) {
            match validate_patch_pk(&pool, table, live_only, value).await? {
                PkValidation::Null => sets.push((column, PatchValue::Null)),
                PkValidation::Id(id) => sets.push((column, PatchValue::Uuid(id))),
                PkValidation::FieldError(message) => errors.push(field, message),
            }
        }
    }
    if !errors.is_empty() {
        return Err(Denial::Raw(StatusCode::BAD_REQUEST, errors.body()));
    }
    // `UniqueTogetherValidator` pair (`unique_together` + the
    // conditional `UniqueConstraint`): both run over the live manager
    // excluding self, against the effective triple — missing keys fill
    // from the instance, so the check runs on every PATCH. The triple
    // validator only fires when the new `deleted_at` stays null; the
    // pair validator fires on any live (name, identifier) clash
    // (live probes 2026-10-03).
    let mut new_name = current.entity_name.clone();
    let mut new_identifier = current.entity_identifier;
    let mut new_deleted_at = current.deleted_at;
    for (column, value) in &sets {
        match (*column, value) {
            ("entity_name", PatchValue::Text(text)) => new_name = Some(text.clone()),
            ("entity_name", PatchValue::Null) => new_name = None,
            ("entity_identifier", PatchValue::Uuid(id)) => new_identifier = Some(*id),
            ("entity_identifier", PatchValue::Null) => new_identifier = None,
            ("deleted_at", PatchValue::Timestamp(dt)) => new_deleted_at = Some(*dt),
            ("deleted_at", PatchValue::Null) => new_deleted_at = None,
            _ => {}
        }
    }
    let clash: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM deploy_boards
         WHERE entity_name IS NOT DISTINCT FROM $1
           AND entity_identifier IS NOT DISTINCT FROM $2
           AND deleted_at IS NULL AND id <> $3)",
    )
    .bind(new_name)
    .bind(new_identifier)
    .bind(pk)
    .fetch_one(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    if clash {
        if new_deleted_at.is_none() {
            errors.push(
                "non_field_errors",
                "The fields entity_name, entity_identifier, deleted_at must make a unique set."
                    .to_owned(),
            );
        }
        errors.push(
            "non_field_errors",
            "The fields entity_name, entity_identifier must make a unique set.".to_owned(),
        );
    }
    if !errors.is_empty() {
        return Err(Denial::Raw(StatusCode::BAD_REQUEST, errors.body()));
    }
    // `ModelSerializer.update`: one `save()` — the assignments plus the
    // audit stamp (an empty PATCH stamps too).
    let mut sql = String::from("UPDATE deploy_boards SET ");
    let mut position = 1usize;
    let mut fragments: Vec<String> = Vec::with_capacity(sets.len());
    for (column, value) in &sets {
        let fragment = match value {
            PatchValue::Null => format!("{column} = NULL"),
            PatchValue::Bool(_) | PatchValue::Uuid(_) | PatchValue::Text(_) => {
                let out = format!("{column} = ${position}");
                position += 1;
                out
            }
            PatchValue::Json(_) => {
                let out = format!("{column} = ${position}::jsonb");
                position += 1;
                out
            }
            PatchValue::Timestamp(_) => {
                let out = format!("{column} = ${position}");
                position += 1;
                out
            }
        };
        fragments.push(fragment);
    }
    fragments.push("updated_at = now()".to_owned());
    fragments.push(format!("updated_by_id = ${position}"));
    position += 1;
    sql.push_str(&fragments.join(", "));
    sql.push_str(&format!(" WHERE id = ${position}"));
    let mut query = sqlx::query(&sql);
    for (_, value) in &sets {
        query = match value {
            PatchValue::Null => query,
            PatchValue::Bool(flag) => query.bind(*flag),
            PatchValue::Uuid(id) => query.bind(*id),
            PatchValue::Text(text) => query.bind(text.clone()),
            PatchValue::Json(text) => query.bind(text.clone()),
            PatchValue::Timestamp(dt) => query.bind(*dt),
        };
    }
    query = query.bind(resolved.id).bind(pk);
    query.execute(&pool).await.map_err(integrity_denial)?;
    let (row, board_project_id, intake_id) = fetch_board_for_patch_render(&pool, &pk).await?;
    let mut asset_ids = Vec::new();
    board_asset_ids(&row, &mut asset_ids);
    let assets = fetch_asset_urls(&pool, &asset_ids).await?;
    Ok(json_response(
        StatusCode::OK,
        render_board(
            &row,
            board_project_id,
            intake_id,
            &resolved.timezone,
            &assets,
            views_override.as_ref(),
        ),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pidash_services::app_project::tasks::ViewError;
    use pidash_services::app_project::tasks::{
        detail_body, handle_exception, invite_create_failure,
    };

    fn fixture(name: &str) -> Value {
        let path = format!(
            "{}/../../fixtures/app_project/{name}",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(&path).expect("fixture exists"))
            .expect("fixture parses")
    }

    /// Top-level keys of a `{...}` JSON object string, in byte order.
    fn object_keys(span: &str) -> Vec<String> {
        let value: Value = serde_json::from_str(span).expect("object parses");
        value.as_object().expect("object").keys().cloned().collect()
    }

    #[test]
    fn denial_bodies_match_l8_kernel() {
        assert_eq!(
            (500, SERVER_ERROR_BODY.to_owned()),
            handle_exception(&ViewError::Other)
        );
        assert_eq!(invite_create_failure(), (500, SERVER_ERROR_BODY));
        assert_eq!(
            RESOLVE_NOT_FOUND_BODY,
            handle_exception(&ViewError::Http404("Project not found".to_owned())).1
        );
        assert_eq!(
            INVITE_NOT_FOUND_BODY,
            detail_body(&Value::String(
                "No ProjectMemberInvite matches the given query.".to_owned()
            ))
        );
        assert_eq!(
            BOARD_NOT_FOUND_BODY,
            detail_body(&Value::String(
                "No DeployBoard matches the given query.".to_owned()
            ))
        );
        assert_eq!(
            OBJECT_NOT_FOUND_BODY,
            handle_exception(&ViewError::ObjectDoesNotExist).1
        );
        assert_eq!(
            VALIDATION_ERROR_BODY,
            handle_exception(&ViewError::ValidationError).1
        );
        assert_eq!(
            INTEGRITY_ERROR_BODY,
            handle_exception(&ViewError::IntegrityError).1
        );
    }

    #[test]
    fn denial_statuses() {
        let cases: &[(Denial, u16)] = &[
            (Denial::Unauthorized, 401),
            (Denial::Forbidden, 403),
            (Denial::ClassDenied, 403),
            (Denial::ResolveNotFound, 404),
            (Denial::InviteNotFound, 404),
            (Denial::BoardNotFound, 404),
            (Denial::ObjectNotFound, 404),
            (Denial::BadValidation, 400),
            (Denial::BadPayload, 400),
            (Denial::ServerError, 500),
        ];
        for (denial, status) in cases {
            assert_eq!(
                denial.status_and_body().0,
                StatusCode::from_u16(*status).unwrap()
            );
        }
        assert_eq!(
            Denial::Raw(StatusCode::IM_A_TEAPOT, "x".to_owned()).status_and_body(),
            (StatusCode::IM_A_TEAPOT, "x".to_owned())
        );
    }

    #[test]
    fn message_bodies_match_fixture() {
        let fx = fixture("FX-APROJ-09.handlers_project.json");
        let parse = |body: &str| -> Value { serde_json::from_str(body).unwrap() };
        assert_eq!(
            parse(EMAILS_REQUIRED_BODY),
            fx["invites"]["invite_create_empty_400"]["body"]
        );
        assert_eq!(parse(JOIN_ACCEPTED_BODY), fx["join"]["join_accept"]["body"]);
        assert_eq!(
            parse(JOIN_DECLINED_BODY),
            fx["join"]["join_decline"]["body"]
        );
        assert_eq!(
            parse(PROJECTS_JOINED_BODY),
            fx["user_invites"]["user_invite_join"]["body"]
        );
        assert_eq!(
            parse(WRONG_EMAIL_BODY),
            fx["join"]["join_wrong_email_403"]["body"]
        );
        assert_eq!(
            parse(WRONG_EMAIL_BODY),
            fx["join"]["join_empty_email_403"]["body"]
        );
        assert_eq!(
            parse(ALREADY_RESPONDED_BODY),
            fx["join"]["join_already_responded_400"]["body"]
        );
        assert_eq!(
            parse(SECRET_JOIN_BODY),
            fx["user_invites"]["user_invite_join_secret_403"]["body"]
        );
        // The two ported 500s render the generic FX-APROJ-08 body.
        let fx08 = fixture("FX-APROJ-08.tasks.json");
        assert_eq!(parse(SERVER_ERROR_BODY), fx08["generic_500_body"]);
        assert_eq!(parse(SERVER_ERROR_BODY), fx08["invite_create_500"]["body"]);
        assert_eq!(parse(SERVER_ERROR_BODY), fx08["favorites_list_500"]["body"]);
    }

    #[test]
    fn prep_uuid_matrix() {
        let good = "5952e951-baad-4770-8ee8-64faf7f753fd";
        assert!(prep_uuid(&Value::String(good.to_owned())).is_ok());
        assert!(matches!(
            prep_uuid(&Value::String("not-a-uuid".to_owned())),
            Err(Denial::BadValidation)
        ));
        assert!(prep_uuid(&serde_json::json!(123)).is_ok());
        assert!(matches!(
            prep_uuid(&serde_json::json!(-1)),
            Err(Denial::BadValidation)
        ));
        assert!(matches!(
            prep_uuid(&serde_json::json!(1.5)),
            Err(Denial::BadValidation)
        ));
        assert!(prep_uuid(&Value::Bool(true)).is_ok());
        assert!(matches!(
            prep_uuid(&Value::Null),
            Err(Denial::BadValidation)
        ));
        assert!(matches!(
            prep_uuid(&serde_json::json!(["x"])),
            Err(Denial::BadValidation)
        ));
        assert!(matches!(
            prep_uuid(&serde_json::json!({"a": 1})),
            Err(Denial::BadValidation)
        ));
        // Braced and URN spellings pass like `uuid.UUID(hex=...)`.
        assert!(prep_uuid(&Value::String(
            "{5952e951-baad-4770-8ee8-64faf7f753fd}".to_owned()
        ))
        .is_ok());
    }

    #[test]
    fn prep_bool_matrix() {
        // `value in (True, False)` — 1/0 and 1.0/0.0 ride `==`.
        for truthy in [
            serde_json::json!(true),
            serde_json::json!(1),
            serde_json::json!(1.0),
        ] {
            assert_eq!(prep_bool(&truthy).unwrap(), Some(true));
        }
        for falsy in [
            serde_json::json!(false),
            serde_json::json!(0),
            serde_json::json!(0.0),
        ] {
            assert_eq!(prep_bool(&falsy).unwrap(), Some(false));
        }
        // Exact string spellings only — lowercase variants 400.
        for raw in ["t", "True", "1"] {
            assert_eq!(
                prep_bool(&Value::String(raw.to_owned())).unwrap(),
                Some(true)
            );
        }
        for raw in ["f", "False", "0"] {
            assert_eq!(
                prep_bool(&Value::String(raw.to_owned())).unwrap(),
                Some(false)
            );
        }
        for raw in ["true", "false", "yes", "no", "on", "off", "YES", ""] {
            assert!(matches!(
                prep_bool(&Value::String(raw.to_owned())),
                Err(Denial::BadValidation)
            ));
        }
        assert_eq!(prep_bool(&Value::Null).unwrap(), None);
        assert!(matches!(
            prep_bool(&serde_json::json!(2)),
            Err(Denial::BadValidation)
        ));
        assert!(matches!(
            prep_bool(&serde_json::json!([])),
            Err(Denial::BadValidation)
        ));
    }

    #[test]
    fn patch_bool_is_case_insensitive_drf_set() {
        for raw in ["YES", "True", "ON", "t", "1"] {
            assert!(validate_patch_bool(&Value::String(raw.to_owned())).unwrap());
        }
        for raw in ["NO", "False", "OFF", "f", "0"] {
            assert!(!validate_patch_bool(&Value::String(raw.to_owned())).unwrap());
        }
        assert!(validate_patch_bool(&serde_json::json!(1.0)).unwrap());
        assert_eq!(
            validate_patch_bool(&Value::Null).unwrap_err(),
            "This field may not be null."
        );
        assert_eq!(
            validate_patch_bool(&Value::String("".to_owned())).unwrap_err(),
            "Must be a valid boolean."
        );
        assert_eq!(
            validate_patch_bool(&serde_json::json!(2)).unwrap_err(),
            "Must be a valid boolean."
        );
    }

    #[test]
    fn patch_uuid_and_name() {
        assert_eq!(validate_patch_uuid(&Value::Null).unwrap(), None);
        assert_eq!(
            validate_patch_uuid(&Value::String("bad".to_owned())).unwrap_err(),
            "Must be a valid UUID."
        );
        assert!(validate_patch_uuid(&serde_json::json!(7)).is_ok());
        assert_eq!(validate_patch_name(&Value::Null).unwrap(), None);
        assert_eq!(
            validate_patch_name(&Value::String("  padded  ".to_owned())).unwrap(),
            Some("padded".to_owned())
        );
        assert_eq!(
            validate_patch_name(&serde_json::json!(123)).unwrap(),
            Some("123".to_owned())
        );
        assert_eq!(
            validate_patch_name(&Value::Bool(true)).unwrap_err(),
            "Not a valid string."
        );
        assert_eq!(
            validate_patch_name(&Value::String("".to_owned())).unwrap(),
            Some("".to_owned())
        );
        let over = "x".repeat(31);
        assert_eq!(
            validate_patch_name(&Value::String(over)).unwrap_err(),
            "Ensure this field has no more than 30 characters."
        );
        let exact = "y".repeat(30);
        assert_eq!(
            validate_patch_name(&Value::String(exact.clone())).unwrap(),
            Some(exact)
        );
    }

    #[test]
    fn patch_datetime_forms() {
        assert_eq!(validate_patch_datetime(&Value::Null).unwrap(), None);
        let utc = validate_patch_datetime(&Value::String("2026-10-02T22:22:39.069273Z".to_owned()))
            .unwrap()
            .unwrap();
        assert_eq!(
            utc.to_rfc3339_opts(chrono::SecondsFormat::Micros, true),
            "2026-10-02T22:22:39.069273Z"
        );
        let offset =
            validate_patch_datetime(&Value::String("2026-10-02T22:22:39+02:00".to_owned()))
                .unwrap()
                .unwrap();
        assert_eq!(
            offset.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            "2026-10-02T20:22:39Z"
        );
        // Naive spellings assume UTC.
        let naive = validate_patch_datetime(&Value::String("2026-10-02 22:22:39".to_owned()))
            .unwrap()
            .unwrap();
        assert_eq!(
            naive.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            "2026-10-02T22:22:39Z"
        );
        let minute = validate_patch_datetime(&Value::String("2026-10-02T22:22".to_owned()))
            .unwrap()
            .unwrap();
        assert_eq!(
            minute.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            "2026-10-02T22:22:00Z"
        );
        let err = validate_patch_datetime(&Value::String("tomorrow".to_owned())).unwrap_err();
        assert!(err.starts_with("Datetime has wrong format. Use one of these formats instead: "));
        assert!(validate_patch_datetime(&serde_json::json!(5)).is_err());
    }

    #[test]
    fn python_names() {
        assert_eq!(python_type_name(&Value::Null), "NoneType");
        assert_eq!(python_type_name(&Value::Bool(true)), "bool");
        assert_eq!(python_type_name(&serde_json::json!(3)), "int");
        assert_eq!(python_type_name(&serde_json::json!(3.5)), "float");
        assert_eq!(python_type_name(&Value::String("x".to_owned())), "str");
        assert_eq!(python_type_name(&serde_json::json!([])), "list");
        assert_eq!(python_type_name(&serde_json::json!({})), "dict");
        assert_eq!(python_str(&Value::Bool(true)), "True");
        assert_eq!(python_str(&serde_json::json!(123)), "123");
        assert_eq!(python_str(&Value::String("a".to_owned())), "a");
        assert_eq!(python_str(&serde_json::json!([1, "x"])), "[1, 'x']");
        assert_eq!(python_str(&serde_json::json!({"a": 1})), "{'a': 1}");
        assert_eq!(python_str(&serde_json::json!(2.0)), "2.0");
        assert_eq!(
            uuid_invalid_message(&Value::String("nope".to_owned())),
            "\u{201c}nope\u{201d} is not a valid UUID."
        );
        // PATCH PK prep: strings parse, non-negative ints take the
        // `int=` form, bools never reach it (`incorrect_type` first).
        assert!(prep_patch_pk_uuid(&Value::String(
            "5952e951-baad-4770-8ee8-64faf7f753fd".to_owned()
        ))
        .is_some());
        assert!(prep_patch_pk_uuid(&serde_json::json!(7)).is_some());
        assert!(prep_patch_pk_uuid(&serde_json::json!(-7)).is_none());
        assert!(prep_patch_pk_uuid(&serde_json::json!(1.5)).is_none());
        assert!(prep_patch_pk_uuid(&Value::Bool(true)).is_none());
    }

    #[test]
    fn patch_error_body_shape_and_order() {
        let mut errors = BoardPatchErrors::new();
        assert!(errors.is_empty());
        errors.push("entity_name", "Not a valid string.".to_owned());
        errors.push("is_votes_enabled", "Must be a valid boolean.".to_owned());
        assert_eq!(
            errors.body(),
            r#"{"entity_name":["Not a valid string."],"is_votes_enabled":["Must be a valid boolean."]}"#
        );
        let mut unique = BoardPatchErrors::new();
        unique.push(
            "non_field_errors",
            "The fields a, b must make a unique set.".to_owned(),
        );
        unique.push(
            "non_field_errors",
            "The fields a must make a unique set.".to_owned(),
        );
        assert_eq!(
            unique.body(),
            r#"{"non_field_errors":["The fields a, b must make a unique set.","The fields a must make a unique set."]}"#
        );
    }

    #[test]
    fn invite_wire_order_matches_fixture() {
        let fx = fixture("FX-APROJ-09.handlers_project.json");
        let golden = &fx["invites"]["invite_list"]["body"][0];
        let expected = object_keys(&serde_json::to_string(golden).unwrap());
        let logo_props = serde_json::json!({});
        let row = ProjectMemberInviteRow {
            id: "id",
            project: ProjectLiteRow {
                id: "p",
                identifier: "I",
                name: "n",
                cover_image: None,
                cover_image_url: None,
                logo_props: &logo_props,
                description: "",
                is_default: true,
            },
            workspace: WorkspaceLiteRow {
                name: "w",
                slug: "s",
                id: "wid",
                logo_url: None,
            },
            created_at: "c",
            updated_at: "u",
            deleted_at: None,
            email: "e",
            accepted: false,
            token: "t",
            message: None,
            responded_at: None,
            role: 15,
            created_by: None,
            updated_by: None,
        };
        let rendered = serde_json::to_string(&invite_to_representation(&row)).unwrap();
        assert_eq!(object_keys(&rendered), expected);
    }

    #[test]
    fn board_shapes_match_fixture() {
        let fx = fixture("FX-APROJ-09.handlers_project.json");
        // The empty-list None shape, byte for byte.
        let empty = &fx["deploy_boards"]["boards_empty"]["body"];
        assert_eq!(
            serde_json::to_string(empty).unwrap(),
            DEPLOY_BOARD_INITIAL_BODY
        );
        // The full shape key order.
        let created = &fx["deploy_boards"]["boards_create"]["body"];
        let expected = object_keys(&serde_json::to_string(created).unwrap());
        assert_eq!(
            expected,
            pidash_services::app_project::ser_project::DEPLOY_BOARD_KEY_ORDER
                .iter()
                .map(|key| key.to_string())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn default_blobs_carry_python_keys() {
        let member: Value = serde_json::from_str(&default_member_props_json()).unwrap();
        assert!(member.get("filters").is_some());
        assert!(member.get("display_filters").is_some());
        assert!(member.get("display_properties").is_none());
        let workspace: Value = serde_json::from_str(&default_workspace_props_json()).unwrap();
        assert!(workspace.get("display_properties").is_some());
        let prefs: Value = serde_json::from_str(&default_preferences_json()).unwrap();
        assert_eq!(prefs["navigation"]["default_tab"], "work_items");
        let views: Value = serde_json::from_str(&default_board_views_json()).unwrap();
        assert_eq!(
            views,
            serde_json::json!({"list": true, "kanban": true, "calendar": true, "gantt": true, "spreadsheet": true})
        );
        let filters: Value = serde_json::from_str(&default_issue_filters_json()).unwrap();
        assert_eq!(filters["subscriber"], Value::Null);
    }

    #[test]
    fn soft_delete_message_shape() {
        use pidash_jobs::tasks_cleanup::deletion::SOFT_DELETE_TASK;
        let pk = uuid::Uuid::nil();
        let message = soft_delete_message("deployboard", &pk);
        assert_eq!(message.task.as_str(), SOFT_DELETE_TASK);
        assert_eq!(
            message.args,
            vec![
                Value::String("db".to_owned()),
                Value::String("deployboard".to_owned()),
                Value::String(pk.to_string()),
            ]
        );
        assert_eq!(message.kwargs.get("using"), Some(&Value::Null));
    }
}
