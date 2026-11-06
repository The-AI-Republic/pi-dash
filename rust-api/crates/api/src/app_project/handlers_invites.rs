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
//!   L7 gate (403), then body parsing (JSON / form / multipart via the
//!   shared DRF negotiation, 415 / axum rejection; empty body is `{}` per
//!   the cycles live probe), then the handler.
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
//! * Invite create (re-synced to Django `d656243d`, PIDASHCONV-1085 —
//!   PB-0499 retired): the pre-fix port always 500s (`invite.py:62` read
//!   `.role` off a queryset, FX-APROJ-08). The fixed view reads the
//!   invitee workspace role with `values_list("role").first()` (`None`
//!   for non-members, who pass the check through), answers 200
//!   `{"error": "You cannot invite a user with different role than
//!   workspace role"}` when a 5/20 workspace role differs from the
//!   requested role (yes, 200, ported as-is), then bulk-creates the
//!   invites and enqueues `project_invitation.delay(email, project_id,
//!   token, current_site, inviter)` once per row.
//! * Favorites list (re-synced to the PDASHOSS01-293 Django fix — PB-0500
//!   retired): the pre-fix port always 500ed (no `serializer_class`,
//!   FX-APROJ-06/08 are the pre-fix probe records); the fixed viewset
//!   lists through `UserFavoriteSerializer`, 200.
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
use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, FromQueryResult, JoinType,
    QueryFilter, QueryOrder, QuerySelect, RelationTrait,
};
use serde_json::{Map, Value};

use crate::app_project::gates::{
    decide_gate, decide_project_member_gate, gate_for, tenant_context, GateOutcome, ANON_BODY,
    CLASS_DENIED_BODY, FORBIDDEN_BODY,
};
use crate::license::{resolve_actor, Actor};
use crate::middleware::body_limit::read_request_bytes;
use crate::middleware::SessionHandle;
use crate::state::AppState;

use pidash_auth::permissions::allow::AllowFacts;
use pidash_auth::permissions::project::ProjectFacts;
use pidash_auth::permissions::{ROLE_ADMIN, ROLE_MEMBER};
use pidash_compat::db::is_integrity_error;
use pidash_compat::response::{empty_response, json_response};
use pidash_db::entity::{
    deploy_boards as db_entity, file_assets as fa_entity, project_member_invites as pmi_entity,
    project_members as pm_entity, project_user_properties as pup_entity, projects as proj_entity,
    user_favorites as uf_entity, users as user_entity, workspace_members as wm_entity,
    workspaces as ws_entity,
};
use pidash_services::app_project::ser_member::{
    invite_to_representation, project_lite_to_representation, resolve_project_cover_image_url,
    resolve_workspace_logo_url, workspace_lite_to_representation, ProjectLiteRow,
    ProjectMemberInviteRow, WorkspaceLiteRow,
};
use pidash_services::app_project::ser_project::{DeployBoardRead, DEPLOY_BOARD_INITIAL_BODY};
use pidash_services::app_project::tasks::{
    invite_token, INTEGRITY_ERROR_BODY, OBJECT_NOT_FOUND_BODY, SERVER_ERROR_BODY,
    VALIDATION_ERROR_BODY,
};
use pidash_services::domain::workspaces::members;
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
/// authed) where axum would 405. HEAD proxies as well: Django defines no
/// `head` and 405s after auth (axum would auto-serve it from `get`).
fn owned(
    router: axum::routing::MethodRouter<AppState>,
    methods: &[&str],
) -> axum::routing::MethodRouter<AppState> {
    let mut router = router;
    for method in ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] {
        if methods.contains(&method) {
            continue;
        }
        router = match method {
            "GET" => router.get(crate::edge::proxy),
            "POST" => router.post(crate::edge::proxy),
            "PUT" => router.put(crate::edge::proxy),
            "PATCH" => router.patch(crate::edge::proxy),
            "DELETE" => router.delete(crate::edge::proxy),
            "HEAD" => router.head(crate::edge::proxy),
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
/// `ProjectInvitationsViewset.create` when the invitee's workspace role is
/// 5 or 20 and differs from the requested role (`invite.py:66-67` at
/// `d656243d`): Django returns a bare `Response(...)` — status 200,
/// ported as-is.
const ROLE_MISMATCH_BODY: &str =
    r#"{"error":"You cannot invite a user with different role than workspace role"}"#;
/// `ProjectInvitationsViewset.create` success (`invite.py:113`).
const INVITES_SENT_BODY: &str = r#"{"message":"Email sent successfully"}"#;
/// `bulk_create(..., batch_size=10)` (`invite.py:98-100`): one INSERT per
/// 10 rows, each committed separately in autocommit.
const INVITE_BULK_BATCH_SIZE: usize = 10;
/// `pi_dash.bgtasks.project_invitation_task.project_invitation`: the
/// `@shared_task` default name (module path + function name), enqueued
/// once per created invitation (`invite.py:104-111`). Defined locally:
/// the jobs crate is read-only for port agents and still records this
/// task as dead code (no caller before `d656243d`).
const PROJECT_INVITATION_TASK: &str = "pi_dash.bgtasks.project_invitation_task.project_invitation";
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
pub(crate) enum Denial {
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

/// The shared SeaORM connection over an sqlx pool (no I/O — it wraps the
/// pool).
fn sea_conn(pool: &sqlx::PgPool) -> sea_orm::DatabaseConnection {
    pidash_db::pool::sea_orm_connection(pool)
}

/// [`integrity_denial`] over a SeaORM failure.
fn orm_integrity_denial(err: sea_orm::DbErr) -> Denial {
    integrity_denial(pidash_db::orm_tx::into_sqlx(err))
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
    let upper = normalize_resolve_identifier(raw);
    let row = proj_entity::Entity::find()
        .select_only()
        .column(proj_entity::Column::Id)
        .join(JoinType::InnerJoin, proj_entity::Relation::Workspaces.def())
        .filter(ws_entity::Column::Slug.eq(slug))
        .filter(proj_entity::Column::Identifier.eq(upper))
        .filter(proj_entity::Column::DeletedAt.is_null())
        .order_by_desc(proj_entity::Column::CreatedAt)
        .into_tuple::<uuid::Uuid>()
        .one(&sea_conn(pool))
        .await
        .map_err(|_| Denial::ServerError)?;
    row.ok_or(Denial::ResolveNotFound)
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
    let db = sea_conn(pool);
    let workspace_role = crate::app_issues::access::workspace_role(&db, slug, *user_id)
        .await
        .map_err(|_| Denial::ServerError)?;
    let project_roles = pm_entity::Entity::find()
        .select_only()
        .columns([pm_entity::Column::ProjectId, pm_entity::Column::Role])
        .join(JoinType::InnerJoin, pm_entity::Relation::Workspaces.def())
        .filter(ws_entity::Column::Slug.eq(slug))
        .filter(pm_entity::Column::MemberId.eq(*user_id))
        .filter(pm_entity::Column::IsActive.eq(true))
        .filter(pm_entity::Column::DeletedAt.is_null())
        .into_tuple::<(uuid::Uuid, i16)>()
        .all(&db)
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok((workspace_role, project_roles))
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

/// HTML-input shape for the five invite handlers that read
/// `request.data` directly (`invite.py`, `base.py` favorites +
/// board-create): DRF applies no serializer `get_value` rules there —
/// `QueryDict.get` returns the last value verbatim — so no field is a
/// list and no blank is skipped (PIDASHCONV-1508).
const INVITE_BODY_SPEC: crate::v1_cycles_modules::body::BodySpec =
    crate::v1_cycles_modules::body::BodySpec {
        list_fields: &[],
        skip_blank_fields: &[],
    };

/// HTML-input shape for the board PATCH (serializer-validated): a blank
/// boolean behaves as absent (DRF `Field.get_value` — not required, no
/// `allow_blank`, no `allow_null`), and so does a blank `updated_by`
/// (the PATCH always re-stamps it to the caller, so absent and `None`
/// converge — verified live, PIDASHCONV-1508).
const BOARD_PATCH_BODY_SPEC: crate::v1_cycles_modules::body::BodySpec =
    crate::v1_cycles_modules::body::BodySpec {
        list_fields: &[],
        skip_blank_fields: &[
            "is_comments_enabled",
            "is_reactions_enabled",
            "is_votes_enabled",
            "is_activity_enabled",
            "is_disabled",
            "updated_by",
        ],
    };

/// Parse a form/multipart body through the shared DRF negotiation
/// (PIDASHCONV-1508): the text map with the domain's HTML-input
/// semantics. Uploaded files are dropped — no invite field is a file
/// field (same convention as PIDASHCONV-1391).
#[allow(clippy::result_large_err)]
fn parse_html_body(
    headers: &axum::http::HeaderMap,
    body: &[u8],
    spec: &crate::v1_cycles_modules::body::BodySpec,
) -> Result<Value, Response> {
    use crate::v1_cycles_modules::body::{negotiate_body, BodyError, NegotiatedBody};
    match negotiate_body(headers, body, spec) {
        Ok(NegotiatedBody::Form { map, .. }) => Ok(Value::Object(map)),
        // Content-Length says empty (a header/body mismatch — the divert
        // only runs on non-empty bytes): `{}` like the empty arm below.
        Ok(NegotiatedBody::Empty) => Ok(Value::Object(Map::new())),
        // Unreachable by selection (the divert only runs on the HTML
        // mimes): decode defensively, 500 when the text is not JSON.
        Ok(NegotiatedBody::JsonText { text, .. }) => {
            serde_json::from_str(&text).map_err(|_| Denial::ServerError.into_response())
        }
        Err(BodyError::UnsupportedMediaType(detail)) => Err(Denial::Raw(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            format!("{{\"detail\":{}}}", json_string(&detail)),
        )
        .into_response()),
        Err(BodyError::ParseDetail(detail)) => Err(Denial::Raw(
            StatusCode::BAD_REQUEST,
            format!("{{\"detail\":{}}}", json_string(&detail)),
        )
        .into_response()),
        Err(BodyError::ServerError) => Err(Denial::ServerError.into_response()),
    }
}

/// Bodies parse after the gate (the handler builds on `request.data`):
/// an empty body is `{}` (DRF never raises `ParseError` on it — the
/// cycles live probe); form and multipart bodies parse through the
/// shared DRF negotiation (PIDASHCONV-1508); a non-empty body whose
/// content type the JSON parser does not satisfy answers the
/// `UnsupportedMediaType` 415 naming the content type verbatim (empty
/// when the header is missing — uvicorn/prod; runserver's `text/plain`
/// there is a wsgiref artifact the port does not reproduce); otherwise
/// axum's `Json` rejection bytes apply (malformed JSON stays
/// DRF-unmatched — its messages carry parser positions serde cannot
/// reproduce — documented, never hit by the suite). The flag reports
/// HTML (form/multipart) input so the board PATCH can apply the
/// serializer's HTML-only rules.
#[allow(clippy::result_large_err)]
async fn parse_body(
    state: &AppState,
    req: axum::extract::Request,
    spec: &crate::v1_cycles_modules::body::BodySpec,
) -> Result<(Value, bool), Response> {
    use axum::extract::FromRequest;
    let (mut parts, body) = req.into_parts();
    let bytes = read_request_bytes(body).await?;
    if bytes.is_empty() {
        return Ok((Value::Object(Map::new()), false));
    }
    // Latin-1, like `negotiate_body`'s read: obs-text bytes name verbatim
    // in the 415 (PIDASHCONV-1537); `to_str().ok()` blanked them to "".
    let content_type = crate::v1_cycles_modules::body::header_str(&parts.headers, "content-type");
    let mime = content_type.split(';').next().unwrap_or("").trim();
    if mime.eq_ignore_ascii_case("application/x-www-form-urlencoded")
        || mime.eq_ignore_ascii_case("multipart/form-data")
    {
        return parse_html_body(&parts.headers, &bytes, spec).map(|body| (body, true));
    }
    if !matches_json_parser(&content_type) {
        // DRF `UnsupportedMediaType` (`detail`, lowercase; the media
        // type quotes are part of the message — live probe 2026-10-03).
        let message = format!("Unsupported media type \"{content_type}\" in request.");
        let body = format!("{{\"detail\":{}}}", json_string(&message));
        return Err(Denial::Raw(StatusCode::UNSUPPORTED_MEDIA_TYPE, body).into_response());
    }
    // A wildcard content type selects the JSON parser (DRF `select_parser`
    // first-match): normalize the header so axum's `Json` extractor takes
    // the same path — and renders the same rejection bytes — as a plain
    // `application/json` request.
    if !mime.eq_ignore_ascii_case("application/json") {
        parts.headers.insert(
            header::CONTENT_TYPE,
            header::HeaderValue::from_static("application/json"),
        );
    }
    let req = axum::extract::Request::from_parts(parts, axum::body::Body::from(bytes));
    match axum::Json::<Value>::from_request(req, state).await {
        Ok(axum::Json(body)) => Ok((body, false)),
        Err(rejection) => Err(rejection.into_response()),
    }
}

/// DRF `media_type_matches('application/json', content_type)`
/// (`rest_framework/utils/mediatypes.py:_MediaType.match` with the parser
/// side holding no params and no wildcards): the JSON parser is first in
/// `DEFAULT_PARSER_CLASSES` (no override in `settings/common.py`), so any
/// content type it satisfies parses as JSON — `application/json` in any
/// case with any params, plus the `*/*`, `application/*` (and `*/json`)
/// request-side wildcards. The `v1_cycles_modules::body` `select_parser`
/// precedent (PIDASHCONV-627); form/multipart divert to `parse_html_body`
/// above (PIDASHCONV-1508), never reach this check.
fn matches_json_parser(content_type: &str) -> bool {
    let base = content_type.split(';').next().unwrap_or("").trim();
    let Some((main, sub)) = base.split_once('/') else {
        return false;
    };
    (main == "*" || main.eq_ignore_ascii_case("application"))
        && (sub == "*" || sub.eq_ignore_ascii_case("json"))
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
    #[derive(Debug, FromQueryResult)]
    struct AssetFetched {
        id: uuid::Uuid,
        entity_type: Option<String>,
        workspace_slug: Option<String>,
        project_id: Option<uuid::Uuid>,
        issue_id: Option<uuid::Uuid>,
    }
    let rows = fa_entity::Entity::find()
        .select_only()
        .columns([
            fa_entity::Column::Id,
            fa_entity::Column::EntityType,
            fa_entity::Column::ProjectId,
            fa_entity::Column::IssueId,
        ])
        .column_as(ws_entity::Column::Slug, "workspace_slug")
        .join(
            JoinType::LeftJoin,
            fa_entity::Entity::belongs_to(ws_entity::Entity)
                .from(fa_entity::Column::WorkspaceId)
                .to(ws_entity::Column::Id)
                .into(),
        )
        .filter(fa_entity::Column::Id.is_in(asset_ids.to_vec()))
        .filter(fa_entity::Column::DeletedAt.is_null())
        .into_model::<AssetFetched>()
        .all(&sea_conn(pool))
        .await
        .map_err(|_| Denial::ServerError)?;
    for row in rows {
        let asset = AssetRef {
            id: row.id,
            entity_type: row.entity_type,
            workspace_slug: row.workspace_slug,
            project_id: row.project_id,
            issue_id: row.issue_id,
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
#[derive(Debug, Clone, FromQueryResult)]
struct InviteRow {
    id: uuid::Uuid,
    created_at: sea_orm::prelude::DateTimeWithTimeZone,
    updated_at: sea_orm::prelude::DateTimeWithTimeZone,
    deleted_at: Option<sea_orm::prelude::DateTimeWithTimeZone>,
    email: String,
    accepted: bool,
    token: String,
    message: Option<String>,
    responded_at: Option<sea_orm::prelude::DateTimeWithTimeZone>,
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

fn invite_select() -> sea_orm::Select<pmi_entity::Entity> {
    pmi_entity::Entity::find()
        .select_only()
        .columns([
            pmi_entity::Column::Id,
            pmi_entity::Column::CreatedAt,
            pmi_entity::Column::UpdatedAt,
            pmi_entity::Column::DeletedAt,
            pmi_entity::Column::Email,
            pmi_entity::Column::Accepted,
            pmi_entity::Column::Token,
            pmi_entity::Column::Message,
            pmi_entity::Column::RespondedAt,
            pmi_entity::Column::Role,
            pmi_entity::Column::CreatedById,
            pmi_entity::Column::UpdatedById,
        ])
        .column_as(proj_entity::Column::Id, "project_id")
        .column_as(proj_entity::Column::Identifier, "project_identifier")
        .column_as(proj_entity::Column::Name, "project_name")
        .column_as(proj_entity::Column::CoverImage, "project_cover_image")
        .column_as(
            proj_entity::Column::CoverImageAssetId,
            "project_cover_asset_id",
        )
        .column_as(proj_entity::Column::LogoProps, "project_logo_props")
        .column_as(proj_entity::Column::Description, "project_description")
        .column_as(proj_entity::Column::IsDefault, "project_is_default")
        .column_as(ws_entity::Column::Id, "workspace_id")
        .column_as(ws_entity::Column::Name, "workspace_name")
        .column_as(ws_entity::Column::Slug, "workspace_slug")
        .column_as(ws_entity::Column::Logo, "workspace_logo")
        .column_as(ws_entity::Column::LogoAssetId, "workspace_logo_asset_id")
        .join(JoinType::InnerJoin, pmi_entity::Relation::Projects.def())
        .join(JoinType::InnerJoin, pmi_entity::Relation::Workspaces.def())
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
    invite_select()
        .filter(pmi_entity::Column::DeletedAt.is_null())
        .filter(ws_entity::Column::Slug.eq(slug))
        .filter(pmi_entity::Column::ProjectId.eq(*project_id))
        .order_by_desc(pmi_entity::Column::CreatedAt)
        .into_model::<InviteRow>()
        .all(&sea_conn(pool))
        .await
        .map_err(|_| Denial::ServerError)
}

/// One invite over the scoped queryset (DRF-default `get_object()`:
/// `get_object_or_404` — Django's `No ... matches` miss).
async fn fetch_invite_one(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    pk: &uuid::Uuid,
) -> Result<InviteRow, Denial> {
    invite_select()
        .filter(pmi_entity::Column::DeletedAt.is_null())
        .filter(ws_entity::Column::Slug.eq(slug))
        .filter(pmi_entity::Column::ProjectId.eq(*project_id))
        .filter(pmi_entity::Column::Id.eq(*pk))
        .into_model::<InviteRow>()
        .one(&sea_conn(pool))
        .await
        .map_err(|_| Denial::ServerError)?
        .ok_or(Denial::InviteNotFound)
}

/// `UserProjectInvitationsViewset.get_queryset` (`invite.py:120-126`):
/// live invites addressed to the caller's email, across all workspaces
/// (no slug predicate), `-created_at`.
async fn fetch_user_invites(
    pool: &sqlx::PgPool,
    email: Option<&str>,
) -> Result<Vec<InviteRow>, Denial> {
    let mut select = invite_select()
        .filter(pmi_entity::Column::DeletedAt.is_null())
        .order_by_desc(pmi_entity::Column::CreatedAt);
    select = match email {
        Some(email) => select.filter(pmi_entity::Column::Email.eq(email)),
        None => select.filter(pmi_entity::Column::Email.is_null()),
    };
    select
        .into_model::<InviteRow>()
        .all(&sea_conn(pool))
        .await
        .map_err(|_| Denial::ServerError)
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
    let rows = invite_select()
        .filter(pmi_entity::Column::DeletedAt.is_null())
        .filter(ws_entity::Column::Slug.eq(slug))
        .filter(pmi_entity::Column::ProjectId.eq(*project_id))
        .filter(pmi_entity::Column::Id.eq(*pk))
        .into_model::<InviteRow>()
        .all(&sea_conn(pool))
        .await
        .map_err(|_| Denial::ServerError)?;
    if rows.len() > 1 {
        return Err(Denial::ServerError);
    }
    rows.into_iter().next().ok_or(Denial::ObjectNotFound)
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
    let render_dt = |value: &sea_orm::prelude::DateTimeWithTimeZone| {
        crate::serializer::render_datetime_in(&value.with_timezone(&chrono::Utc), tz)
    };
    let rendered = RenderedInvite {
        id: row.id.to_string(),
        project_id: row.project_id.to_string(),
        workspace_id: row.workspace_id.to_string(),
        created_at: render_dt(&row.created_at),
        updated_at: render_dt(&row.updated_at),
        deleted_at: row.deleted_at.as_ref().map(render_dt),
        responded_at: row.responded_at.as_ref().map(render_dt),
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

#[derive(Debug, FromQueryResult)]
struct BoardFull {
    id: uuid::Uuid,
    created_at: sea_orm::prelude::DateTimeWithTimeZone,
    updated_at: sea_orm::prelude::DateTimeWithTimeZone,
    deleted_at: Option<sea_orm::prelude::DateTimeWithTimeZone>,
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
    board_project_id: Option<uuid::Uuid>,
    intake_id: Option<uuid::Uuid>,
}

fn board_select() -> sea_orm::Select<db_entity::Entity> {
    db_entity::Entity::find()
        .select_only()
        .columns([
            db_entity::Column::Id,
            db_entity::Column::CreatedAt,
            db_entity::Column::UpdatedAt,
            db_entity::Column::DeletedAt,
            db_entity::Column::EntityIdentifier,
            db_entity::Column::EntityName,
            db_entity::Column::Anchor,
            db_entity::Column::IsCommentsEnabled,
            db_entity::Column::IsReactionsEnabled,
            db_entity::Column::IsVotesEnabled,
            db_entity::Column::ViewProps,
            db_entity::Column::IsActivityEnabled,
            db_entity::Column::IsDisabled,
            db_entity::Column::CreatedById,
            db_entity::Column::UpdatedById,
            db_entity::Column::WorkspaceId,
        ])
        .column_as(ws_entity::Column::Name, "workspace_name")
        .column_as(ws_entity::Column::Slug, "workspace_slug")
        .column_as(ws_entity::Column::Logo, "workspace_logo")
        .column_as(ws_entity::Column::LogoAssetId, "workspace_logo_asset_id")
        .column_as(proj_entity::Column::Identifier, "project_identifier")
        .column_as(proj_entity::Column::Name, "project_name")
        .column_as(proj_entity::Column::CoverImage, "project_cover_image")
        .column_as(
            proj_entity::Column::CoverImageAssetId,
            "project_cover_asset_id",
        )
        .column_as(proj_entity::Column::LogoProps, "project_logo_props")
        .column_as(proj_entity::Column::Description, "project_description")
        .column_as(proj_entity::Column::IsDefault, "project_is_default")
        .column_as(db_entity::Column::ProjectId, "board_project_id")
        .column_as(db_entity::Column::IntakeId, "intake_id")
        .join(JoinType::InnerJoin, db_entity::Relation::Workspaces.def())
        .join(JoinType::LeftJoin, db_entity::Relation::Projects.def())
}

fn board_from_full(full: BoardFull) -> BoardRow {
    BoardRow {
        id: full.id,
        created_at: full.created_at.with_timezone(&chrono::Utc),
        updated_at: full.updated_at.with_timezone(&chrono::Utc),
        deleted_at: full.deleted_at.map(|dt| dt.with_timezone(&chrono::Utc)),
        entity_identifier: full.entity_identifier,
        entity_name: full.entity_name,
        anchor: full.anchor,
        is_comments_enabled: full.is_comments_enabled,
        is_reactions_enabled: full.is_reactions_enabled,
        is_votes_enabled: full.is_votes_enabled,
        view_props: full.view_props,
        is_activity_enabled: full.is_activity_enabled,
        is_disabled: full.is_disabled,
        created_by_id: full.created_by_id,
        updated_by_id: full.updated_by_id,
        workspace_id: full.workspace_id,
        workspace_name: full.workspace_name,
        workspace_slug: full.workspace_slug,
        workspace_logo: full.workspace_logo,
        workspace_logo_asset_id: full.workspace_logo_asset_id,
        project_identifier: full.project_identifier,
        project_name: full.project_name,
        project_cover_image: full.project_cover_image,
        project_cover_asset_id: full.project_cover_asset_id,
        project_logo_props: full.project_logo_props,
        project_description: full.project_description,
        project_is_default: full.project_is_default,
    }
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
    let row = board_select()
        .filter(db_entity::Column::DeletedAt.is_null())
        .filter(db_entity::Column::EntityName.eq("project"))
        .filter(db_entity::Column::EntityIdentifier.eq(*project_id))
        .filter(ws_entity::Column::Slug.eq(slug))
        .order_by_desc(db_entity::Column::CreatedAt)
        .into_model::<BoardFull>()
        .one(&sea_conn(pool))
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(row.map(|full| {
        let project = full.board_project_id;
        let intake = full.intake_id;
        (board_from_full(full), project, intake)
    }))
}

/// Re-read for the PATCH response render: the PATCH may have *set*
/// `deleted_at`, and Django renders its in-memory instance (200 with
/// the stamp) — a live-only re-read would 404 instead.
async fn fetch_board_for_patch_render(
    pool: &sqlx::PgPool,
    pk: &uuid::Uuid,
) -> Result<(BoardRow, Option<uuid::Uuid>, Option<uuid::Uuid>), Denial> {
    let row = board_select()
        .filter(db_entity::Column::Id.eq(*pk))
        .into_model::<BoardFull>()
        .one(&sea_conn(pool))
        .await
        .map_err(|_| Denial::ServerError)?
        .ok_or(Denial::ServerError)?;
    let project = row.board_project_id;
    let intake = row.intake_id;
    Ok((board_from_full(row), project, intake))
}

/// `DeployBoardViewSet` retrieve / partial_update / destroy lookup: the
/// viewset defines no `get_queryset`, so DRF-default `get_object()` runs
/// over the global `model.objects.all()` — any live board UUID resolves,
/// regardless of workspace or project. A miss is Django's `No DeployBoard matches` 404.
async fn fetch_board_global(
    pool: &sqlx::PgPool,
    pk: &uuid::Uuid,
) -> Result<(BoardRow, Option<uuid::Uuid>, Option<uuid::Uuid>), Denial> {
    let row = board_select()
        .filter(db_entity::Column::DeletedAt.is_null())
        .filter(db_entity::Column::Id.eq(*pk))
        .into_model::<BoardFull>()
        .one(&sea_conn(pool))
        .await
        .map_err(|_| Denial::ServerError)?
        .ok_or(Denial::BoardNotFound)?;
    let project = row.board_project_id;
    let intake = row.intake_id;
    Ok((board_from_full(row), project, intake))
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

/// Board create `views` default (inline in `base.py:561-569`).
fn default_board_views_json() -> String {
    serde_json::json!({
        "list": true, "kanban": true, "calendar": true, "gantt": true, "spreadsheet": true,
    })
    .to_string()
}

/// `soft_delete_related_objects.delay("db", <model>, pk, using=None)`
/// (`db/mixins.py:77`): three positional args, `using` stays a null
/// kwarg (FX-APROJ-08 live capture).
fn soft_delete_message(model: &str, pk: &uuid::Uuid) -> pidash_jobs::TaskMessage {
    let mut kwargs = Map::new();
    kwargs.insert("using".to_owned(), Value::Null);
    pidash_jobs::TaskMessage::new(
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
async fn enqueue_message(pool: &sqlx::PgPool, message: pidash_jobs::TaskMessage) {
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
pub(crate) async fn create_project_member(
    pool: &sqlx::PgPool,
    project_id: &uuid::Uuid,
    member_id: Option<uuid::Uuid>,
    role: i16,
    created_by: Option<uuid::Uuid>,
) -> Result<(), Denial> {
    let db = sea_conn(pool);
    let workspace_id = proj_entity::Entity::find_by_id(*project_id)
        .select_only()
        .column(proj_entity::Column::WorkspaceId)
        .filter(proj_entity::Column::DeletedAt.is_null())
        .into_tuple::<uuid::Uuid>()
        .one(&db)
        .await
        .map_err(|_| Denial::ServerError)?
        .ok_or(Denial::ObjectNotFound)?;
    if let Some(member_id) = member_id {
        #[derive(Debug, FromQueryResult)]
        struct SortMin {
            sort_min: Option<f64>,
        }
        let min_sort = pup_entity::Entity::find()
            .select_only()
            .column_as(
                sea_orm::sea_query::Expr::col(pup_entity::Column::SortOrder).min(),
                "sort_min",
            )
            .filter(pup_entity::Column::WorkspaceId.eq(workspace_id))
            .filter(pup_entity::Column::UserId.eq(member_id))
            .filter(pup_entity::Column::DeletedAt.is_null())
            .into_model::<SortMin>()
            .one(&db)
            .await
            .map_err(|_| Denial::ServerError)?
            .and_then(|row| row.sort_min);
        let sort_order = min_sort.map(|min| min - 10000.0).unwrap_or(65535.0);
        let filters: Value =
            serde_json::from_str(&default_issue_filters_json()).map_err(|_| Denial::ServerError)?;
        let display_filters: Value = serde_json::from_str(&default_issue_display_filters_json())
            .map_err(|_| Denial::ServerError)?;
        let display_properties: Value =
            serde_json::from_str(&default_issue_display_properties_json())
                .map_err(|_| Denial::ServerError)?;
        let prefs: Value =
            serde_json::from_str(&default_preferences_json()).map_err(|_| Denial::ServerError)?;
        pidash_services::domain::projects::members::add_property_db_now(
            pool,
            &pidash_services::domain::projects::members::NewMemberProperty {
                workspace_id,
                project_id: *project_id,
                user_id: member_id,
                filters: &filters,
                display_filters: &display_filters,
                display_properties: &display_properties,
                rich_filters: &serde_json::json!({}),
                preferences: &prefs,
                sort_order,
                created_by,
                now: chrono::Utc::now(),
            },
        )
        .await
        .map_err(integrity_denial)?;
    }
    let props: Value =
        serde_json::from_str(&default_member_props_json()).map_err(|_| Denial::ServerError)?;
    let prefs: Value =
        serde_json::from_str(&default_preferences_json()).map_err(|_| Denial::ServerError)?;
    pidash_services::domain::projects::members::add(
        pool,
        &pidash_services::domain::projects::members::NewProjectMember {
            workspace_id,
            project_id: Some(*project_id),
            user_id: member_id,
            role,
            created_by,
            view_props: &props,
            default_props: &props,
            preferences: &prefs,
            now: chrono::Utc::now(),
            ignore_conflicts: false,
            db_now: true,
        },
    )
    .await
    .map_err(integrity_denial)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Invite create: Python semantics + writes (`invite.py:54-113` at `d656243d`)
// ---------------------------------------------------------------------------

/// Python `str.strip()` with no arguments, over the shared
/// [`is_py_strip_ws`] (Rust `White_Space` plus U+001C-U+001F).
fn py_strip(value: &str) -> &str {
    value.trim_matches(is_py_strip_ws)
}

/// What CPython `int()` strips: Unicode whitespace EXCEPT `\x1c`-`\x1f`
/// (probed in the `app_workspace` port — `int("\x1c5")` raises while
/// `"\x1c5".strip()` is `"5"`), i.e. exactly `char::is_whitespace`.
fn py_int_strip(value: &str) -> &str {
    value.trim_matches(|c: char| c.is_whitespace())
}

/// Python `int()` over a JSON value (the stored invite `role`,
/// `IntegerField.get_prep_value`): bools are 0/1, floats truncate toward
/// zero, strings parse after stripping with an optional sign
/// (underscores allowed between digits, ASCII digits only — non-ASCII
/// digit strings are a documented approximation). Anything else (null,
/// arrays, objects, unparseable strings) is a `TypeError` / `ValueError`,
/// which `handle_exception` answers 500 for. (Copy of the merged
/// `app_workspace::handlers_invites::py_int`; that module keeps its
/// private copy, as `v1_projects` keeps its own `py_strip`.)
fn py_int(value: &Value) -> Result<i128, ()> {
    match value {
        Value::Bool(true) => Ok(1),
        Value::Bool(false) => Ok(0),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(i as i128)
            } else if let Some(u) = n.as_u64() {
                Ok(u as i128)
            } else if let Some(f) = n.as_f64() {
                // `arbitrary_precision` numbers that fit neither int form
                // are floats; huge magnitudes saturate (still off-scale
                // for the smallint column, which is all the caller needs).
                Ok(f.trunc().clamp(i128::MIN as f64, i128::MAX as f64) as i128)
            } else if !n.to_string().contains(['.', 'e', 'E']) {
                // `as_f64` filters non-finite, so `None` on a digit-only
                // literal is a Python int past f64 range (`int()` still
                // succeeds): saturate by sign like the string arm below.
                Ok(if n.to_string().starts_with('-') {
                    -i128::MAX
                } else {
                    i128::MAX
                })
            } else {
                // A float literal past f64 range (`1e999`): Python's
                // `int()` raises `OverflowError` → 500.
                Err(())
            }
        }
        Value::String(raw) => {
            let text = py_int_strip(raw);
            let digits = text
                .strip_prefix('+')
                .or_else(|| text.strip_prefix('-'))
                .unwrap_or(text);
            if digits.is_empty() {
                return Err(());
            }
            // Underscores only between digits (`int("2_0") == 20`).
            let mut cleaned = String::with_capacity(digits.len());
            let mut prev_underscore = true;
            for c in digits.chars() {
                if c == '_' {
                    if prev_underscore {
                        return Err(());
                    }
                    prev_underscore = true;
                } else if c.is_ascii_digit() {
                    cleaned.push(c);
                    prev_underscore = false;
                } else {
                    return Err(());
                }
            }
            if prev_underscore {
                return Err(());
            }
            // Saturate on overflow: a huge value still 500s the smallint
            // column — the same outcome unbounded Python ints get.
            let magnitude: i128 = cleaned.parse().unwrap_or(i128::MAX);
            Ok(if text.starts_with('-') {
                -magnitude
            } else {
                magnitude
            })
        }
        Value::Null | Value::Array(_) | Value::Object(_) => Err(()),
    }
}

/// Python truthiness (`if not emails`, `invite.py:58`).
/// (Copy of the merged `app_workspace` port.)
fn py_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                i != 0
            } else if let Some(u) = n.as_u64() {
                u != 0
            } else {
                // `as_f64` filters non-finite, so `None` here is a ±inf
                // literal — and Python `bool(inf)` is `True`.
                n.as_f64().is_none_or(|f| f != 0.0)
            }
        }
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(m) => !m.is_empty(),
    }
}

/// Python `repr()` of a JSON scalar's float: the shortest round-trip
/// digits (as `serde_json` renders them) laid out by CPython's rule —
/// fixed notation when `-4 < point <= 16` (where the value is
/// `0.digits × 10^point`), scientific with `e±XX` otherwise, and a
/// `.0` whenever fixed notation would show no point (`1e16` →
/// `"1e+16"`, `1.5e-5` → `"1.5e-05"`, `5.0` → `"5.0"`).
/// (Copy of the merged `app_workspace` port.)
fn py_float_repr(f: f64) -> String {
    if f == 0.0 {
        return if f.is_sign_negative() {
            "-0.0".to_owned()
        } else {
            "0.0".to_owned()
        };
    }
    let raw = serde_json::Number::from_f64(f.abs())
        .map(|n| n.to_string())
        .unwrap_or_default();
    // Split the ryu rendering into digit string + decimal point.
    let (mantissa, exp_value) = match raw.split_once('e') {
        Some((mantissa, exp)) => (mantissa, exp.parse::<i32>().unwrap_or(0)),
        None => (raw.as_str(), 0),
    };
    let (int_part, frac_part) = match mantissa.split_once('.') {
        Some((int_part, frac_part)) => (int_part, frac_part),
        None => (mantissa, ""),
    };
    let digits = format!("{int_part}{frac_part}");
    let digits = digits.trim_start_matches('0');
    let digits = if digits.is_empty() { "0" } else { digits };
    // value = D × 10^E with D = int(digits).
    let exp_total = exp_value - frac_part.len() as i32;
    let point = exp_total + digits.len() as i32;
    let sign = if f.is_sign_negative() { "-" } else { "" };
    if !(-4 < point && point <= 16) {
        let head = &digits[..1];
        let tail = &digits[1..];
        let tail = tail.trim_end_matches('0');
        let coefficient = if tail.is_empty() {
            head.to_owned()
        } else {
            format!("{head}.{tail}")
        };
        let exp = point - 1;
        let (esign, edigits) = if exp < 0 {
            ("-", (-exp).to_string())
        } else {
            ("+", exp.to_string())
        };
        let edigits = if edigits.len() < 2 {
            format!("0{edigits}")
        } else {
            edigits
        };
        return format!("{sign}{coefficient}e{esign}{edigits}");
    }
    if point <= 0 {
        return format!("{sign}0.{}{digits}", "0".repeat(-point as usize));
    }
    if point as usize >= digits.len() {
        let mut out = format!(
            "{sign}{digits}{}",
            "0".repeat(point as usize - digits.len())
        );
        out.push_str(".0");
        return out;
    }
    let (head, tail) = digits.split_at(point as usize);
    format!("{sign}{head}.{tail}")
}

/// Python `str()` of a JSON number (`TextField.get_prep_value` for the
/// role-loop `member__email` lookup): ints render exact digits, float
/// literals render via [`py_float_repr`], and float literals past f64
/// range render `"inf"`/`"-inf"`. A digit-only literal past f64 range is
/// a Python int and keeps its exact digits.
/// (Copy of the merged `app_workspace` port.)
fn py_num_str(n: &serde_json::Number) -> String {
    if let Some(i) = n.as_i64() {
        i.to_string()
    } else if let Some(u) = n.as_u64() {
        u.to_string()
    } else if let Some(f) = n.as_f64() {
        let raw = n.to_string();
        if raw.contains(['.', 'e', 'E']) {
            py_float_repr(f)
        } else {
            raw
        }
    } else {
        let raw = n.to_string();
        if raw.contains(['.', 'e', 'E']) {
            if raw.starts_with('-') {
                "-inf".to_owned()
            } else {
                "inf".to_owned()
            }
        } else {
            raw
        }
    }
}

/// Python `repr()` of a JSON string: single quotes unless the value holds
/// a single quote and no double quote; control characters escaped,
/// printable Unicode verbatim.
/// (Copy of the merged `app_workspace` port.)
fn py_str_repr(value: &str) -> String {
    let use_double = value.contains('\'') && !value.contains('"');
    let quote = if use_double { '"' } else { '\'' };
    let mut out = String::with_capacity(value.len() + 2);
    out.push(quote);
    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c if (c as u32) < 0x20 || (c as u32) == 0x7f => {
                out.push_str(&format!("\\x{:02x}", c as u32));
            }
            c if c.is_control() => {
                let code = c as u32;
                if code <= 0xffff {
                    out.push_str(&format!("\\u{code:04x}"));
                } else {
                    out.push_str(&format!("\\U{code:08x}"));
                }
            }
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

/// Python `repr()` of a JSON value, for the invalid-email 400
/// (`invite.py:92`): dicts render insertion-ordered with single-quoted
/// keys (`serde_json` runs with `preserve_order`, so the wire order
/// survives parsing).
/// (Copy of the merged `app_workspace` port.)
fn py_repr(value: &Value) -> String {
    match value {
        Value::Null => "None".to_owned(),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::Number(n) => py_num_str(n),
        Value::String(s) => py_str_repr(s),
        Value::Array(items) => {
            let inner: Vec<String> = items.iter().map(py_repr).collect();
            format!("[{}]", inner.join(", "))
        }
        Value::Object(map) => {
            let inner: Vec<String> = map
                .iter()
                .map(|(k, v)| format!("{}: {}", py_str_repr(k), py_repr(v)))
                .collect();
            format!("{{{}}}", inner.join(", "))
        }
    }
}

/// `datetime.now().timestamp()` (`invite.py:81`): epoch seconds as an f64.
/// Microsecond precision like CPython (whole microseconds, exactly
/// representable below 2^53).
/// (Copy of the merged `app_workspace` port.)
fn epoch_float(now: &DateTime<Utc>) -> f64 {
    let micros = now.timestamp_micros();
    micros as f64 / 1_000_000.0
}

/// How the `emails` input classifies (`invite.py:55-59`): missing,
/// null, or falsy input 400s (`if not emails`); a truthy non-list
/// iterates to an `AttributeError`/`TypeError` (500) — only a
/// non-empty list reaches the role loop.
enum EmailsInput<'a> {
    /// A non-empty list: validate each entry.
    Entries(&'a Vec<Value>),
    /// Missing/null/falsy: the emails-required 400.
    Missing,
    /// Truthy non-list: 500.
    ServerError,
}

fn classify_emails_input(value: Option<&Value>) -> EmailsInput<'_> {
    match value {
        Some(Value::Array(entries)) if !entries.is_empty() => EmailsInput::Entries(entries),
        Some(value) if py_truthy(value) => EmailsInput::ServerError,
        _ => EmailsInput::Missing,
    }
}

/// Split the direct `validate_email` call (`invite.py:74`) into
/// Django's three outcomes (probed live in the `app_workspace` port):
/// falsy input (`None`, `""`, `0`, `[]`, …) fails the validator's
/// `not value` check (`ValidationError` → 400); a container holding
/// `"@"` passes the membership check and dies in `.rsplit`
/// (`AttributeError` → 500); containers without it fail validation
/// (400); truthy numbers/bools fail the `in` check (`TypeError` → 500);
/// strings go to the `EmailValidator` port.
enum EmailCheck {
    /// Failed validation: answer the invalid-email 400.
    Invalid,
    /// `TypeError`/`AttributeError`: answer 500.
    TypeError,
    /// A string to run through [`is_valid_email_str`].
    Candidate(String),
}

fn classify_invite_email(value: Option<&Value>) -> EmailCheck {
    match value {
        None | Some(Value::Null) => EmailCheck::Invalid,
        Some(value) if !py_truthy(value) => EmailCheck::Invalid,
        Some(Value::String(s)) => EmailCheck::Candidate(s.clone()),
        // `"@" in value`: element equality for lists, key lookup for
        // dicts — reaching `.rsplit` 500s; missing it 400s.
        Some(Value::Array(items))
            if items
                .iter()
                .any(|item| item.as_str().is_some_and(|s| s == "@")) =>
        {
            EmailCheck::TypeError
        }
        Some(Value::Object(map)) if map.contains_key("@") => EmailCheck::TypeError,
        Some(Value::Array(_)) | Some(Value::Object(_)) => EmailCheck::Invalid,
        Some(_) => EmailCheck::TypeError,
    }
}

/// Reuse the merged Django `EmailValidator` port rather than forking it
/// (`v1_projects::handlers_members::is_valid_email`, probed against
/// Django 4.2 in PIDASHCONV-371).
fn is_valid_email_str(value: &str) -> bool {
    crate::v1_projects::handlers_members::is_valid_email(value)
}

/// The `member__email` lookup value for the role loop (`invite.py:62`):
/// `EmailField.get_prep_value` stringifies (`None` becomes the `IS NULL`
/// lookup, which matches no row — `email` is non-null — so `None`
/// short-circuits to no query at all).
fn invite_lookup_email(value: Option<&Value>) -> Option<String> {
    match value {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(s.clone()),
        Some(Value::Number(n)) => Some(py_num_str(n)),
        Some(Value::Bool(true)) => Some("True".to_owned()),
        Some(Value::Bool(false)) => Some("False".to_owned()),
        // Only arrays/objects remain: `str(value)` is the `repr`.
        Some(container) => Some(py_repr(container)),
    }
}

/// The role-mismatch comparison (`invite.py:66`,
/// `workspace_role != email.get("role", 5)`): true when the requested
/// role EQUALS the workspace role (no error). A missing key defaults to
/// int 5; numbers compare numerically (`5 == 5.0` in Python); bools are
/// 0/1; anything else (strings, null, containers) is unequal.
fn role_matches_workspace(workspace_role: i16, requested: Option<&Value>) -> bool {
    let expected = i128::from(workspace_role);
    match requested {
        None => expected == 5,
        Some(Value::Number(n)) => {
            if let Some(i) = n.as_i64() {
                i128::from(i) == expected
            } else if let Some(u) = n.as_u64() {
                expected >= 0 && u == expected as u64
            } else if let Some(f) = n.as_f64() {
                f == f64::from(workspace_role)
            } else {
                false
            }
        }
        Some(Value::Bool(true)) => expected == 1,
        Some(Value::Bool(false)) => expected == 0,
        Some(_) => false,
    }
}

/// The invitee's workspace role (`invite.py:61-65`):
/// `WorkspaceMember.objects.filter(workspace__slug=slug,
/// member__email=email, is_active=True).values_list("role",
/// flat=True).first()` — `None` for non-members. Forward-FK joins carry
/// no related-manager filter (no `workspaces.deleted_at` guard — same
/// SQL semantics as Django, per [`membership`]). Django's `.first()`
/// applies the model's `-created_at` ordering, but at most one live row
/// ever matches (`users.email` is unique and `(workspace, member)` is
/// live-unique), so the `ORDER BY wm.id` tiebreak below never decides.
async fn fetch_invitee_workspace_role(
    pool: &sqlx::PgPool,
    slug: &str,
    email: &str,
) -> Result<Option<i16>, Denial> {
    wm_entity::Entity::find()
        .select_only()
        .column(wm_entity::Column::Role)
        .join(JoinType::InnerJoin, wm_entity::Relation::Workspaces.def())
        .join(
            JoinType::InnerJoin,
            wm_entity::Entity::belongs_to(user_entity::Entity)
                .from(wm_entity::Column::MemberId)
                .to(user_entity::Column::Id)
                .into(),
        )
        .filter(ws_entity::Column::Slug.eq(slug))
        .filter(user_entity::Column::Email.eq(email))
        .filter(wm_entity::Column::IsActive.eq(true))
        .filter(wm_entity::Column::DeletedAt.is_null())
        .order_by_asc(wm_entity::Column::Id)
        .into_tuple::<i16>()
        .one(&sea_conn(pool))
        .await
        .map_err(|_| Denial::ServerError)
}

/// `Workspace.objects.get(slug=slug)` (`invite.py:69`): the live-only
/// default manager, so deleted workspaces miss; a miss is the L8
/// `ObjectDoesNotExist` 404 (unreachable past the gate in practice —
/// a missing slug denies first — but mapped all the same).
async fn fetch_workspace_id(pool: &sqlx::PgPool, slug: &str) -> Result<uuid::Uuid, Denial> {
    ws_entity::Entity::find()
        .select_only()
        .column(ws_entity::Column::Id)
        .filter(ws_entity::Column::Slug.eq(slug))
        .filter(ws_entity::Column::DeletedAt.is_null())
        .into_tuple::<uuid::Uuid>()
        .one(&sea_conn(pool))
        .await
        .map_err(|_| Denial::ServerError)?
        .ok_or(Denial::ObjectNotFound)
}

/// One validated invite-create row: the stored address, the raw
/// requested role (`None` when the key is missing — Django defaults to
/// 5; an explicit null stores NULL and dies with `IntegrityError` in
/// the bulk), and the token.
struct PendingInvite {
    email: String,
    role: Option<Value>,
    token: String,
}

/// `project_invitation.delay(email, project_id, token, current_site,
/// invitor)` (`invite.py:104-111`): five positional args, no kwargs.
fn project_invitation_message(
    email: &str,
    project_id: &uuid::Uuid,
    token: &str,
    current_site: &str,
    inviter: &str,
) -> pidash_jobs::TaskMessage {
    pidash_jobs::TaskMessage::new(
        PROJECT_INVITATION_TASK,
        vec![
            Value::String(email.to_owned()),
            Value::String(project_id.to_string()),
            Value::String(token.to_owned()),
            Value::String(current_site.to_owned()),
            Value::String(inviter.to_owned()),
        ],
        Map::new(),
    )
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

/// `POST .../projects/<project_id>/invitations/` (`invite.py:54-113` at
/// `d656243d`, ADMIN-gated), in Django's order: emails-required →
/// role-match loop → workspace → per-email validate → bulk-create →
/// per-row enqueues → 200.
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
    let body = match parse_body(&state, req, &INVITE_BODY_SPEC).await {
        Ok((body, _)) => body,
        Err(response) => return Ok(response),
    };
    // `request.data.get("emails", [])` — a non-dict body has no `.get`
    // (`AttributeError` → 500).
    let data = match body.as_object() {
        Some(data) => data,
        None => return Err(Denial::ServerError),
    };
    // `if not emails` — missing, null, and falsy values 400; a truthy
    // non-list 500s in the iteration (`:61`).
    let entries: &Vec<Value> = match classify_emails_input(data.get("emails")) {
        EmailsInput::Entries(entries) => entries,
        EmailsInput::Missing => {
            return Ok(json_response(
                StatusCode::BAD_REQUEST,
                EMAILS_REQUIRED_BODY.to_owned(),
            ));
        }
        EmailsInput::ServerError => return Err(Denial::ServerError),
    };
    // The role-match loop (`:61-67`): a non-dict entry has no `.get`
    // (500); a 5/20 invitee workspace role that differs from the
    // requested role answers the mismatch 200 — before the workspace
    // fetch, exactly like Django.
    for entry in entries.iter() {
        let Some(item) = entry.as_object() else {
            return Err(Denial::ServerError);
        };
        let workspace_role = match invite_lookup_email(item.get("email")) {
            Some(address) => fetch_invitee_workspace_role(&pool, &slug, &address).await?,
            None => None,
        };
        if let Some(role @ (5 | 20)) = workspace_role {
            if !role_matches_workspace(role, item.get("role")) {
                return Ok(json_response(StatusCode::OK, ROLE_MISMATCH_BODY.to_owned()));
            }
        }
    }
    // The workspace (`:69`): a direct `Workspace.objects.get`.
    let workspace_id = fetch_workspace_id(&pool, &slug).await?;
    // Per-email validate + row build (`:72-95`): the FIRST invalid email
    // aborts (nothing is created — the bulk runs after the loop).
    let now = Utc::now();
    let secret = state.settings().secret_key.clone();
    let mut pending: Vec<PendingInvite> = Vec::with_capacity(entries.len());
    for entry in entries.iter() {
        let Some(item) = entry.as_object() else {
            return Err(Denial::ServerError);
        };
        let valid = match classify_invite_email(item.get("email")) {
            EmailCheck::Invalid => false,
            EmailCheck::TypeError => return Err(Denial::ServerError),
            EmailCheck::Candidate(candidate) => is_valid_email_str(&candidate),
        };
        if !valid {
            // Single key, so key order is fixed; `serde_json` escapes
            // exactly like DRF's compact rendering.
            let mut error = Map::new();
            error.insert(
                "error".to_owned(),
                Value::String(format!(
                    "Invalid email - {} provided a valid email address is required to send the invite",
                    py_repr(entry)
                )),
            );
            return Ok(json_response(
                StatusCode::BAD_REQUEST,
                serde_json::to_string(&Value::Object(error)).expect("error serializes"),
            ));
        }
        let address = item
            .get("email")
            .and_then(Value::as_str)
            .ok_or(Denial::ServerError)?;
        // `varchar(255)` (`db/models/project.py:315`): `validate_email`
        // allows 320 chars, so overlong addresses die in the bulk with a
        // `DataError` (500, not `IntegrityError`).
        let stored = py_strip(address).to_lowercase();
        if stored.chars().count() > 255 {
            return Err(Denial::ServerError);
        }
        let token = invite_token(item, epoch_float(&now), &secret);
        if token.len() > 255 {
            return Err(Denial::ServerError);
        }
        pending.push(PendingInvite {
            email: stored,
            role: item.get("role").cloned(),
            token,
        });
    }
    // `bulk_create(batch_size=10, ignore_conflicts=True)` (`:101-103`):
    // Django wraps the WHOLE bulk in one `transaction.atomic`
    // (`query.py:785`) and prepares each batch's values just before
    // inserting that batch (`_batched_insert` → `_insert`), so an error
    // in any batch — a role `int()` failure or a failed INSERT — rolls
    // back every batch: all invitations or none. The per-batch
    // conversion below is load-bearing order, not an accident: a
    // batch-1 INSERT failure (400) wins over a batch-2 conversion
    // failure (500), exactly like Django. `bulk_create` skips `save()`:
    // no audit stamp beyond the explicit `created_by`, `accepted`
    // false, `message`/`responded_at` NULL. The table carries no unique
    // constraint besides the pk, so `ON CONFLICT DO NOTHING` only ever
    // meets uuid collisions. Dropping `tx` uncommitted on any `return`
    // below rolls the batches back.
    let mut tx = pool.begin().await.map_err(|_| Denial::ServerError)?;
    let conn = pidash_db::orm_tx::TxConn::new(&mut tx);
    let now_ts: sea_orm::prelude::DateTimeWithTimeZone = now.into();
    for batch in pending.chunks(INVITE_BULK_BATCH_SIZE) {
        let mut models: Vec<pmi_entity::ActiveModel> = Vec::with_capacity(batch.len());
        for invite in batch.iter() {
            let role = match invite.role.as_ref() {
                // Missing key: `email.get("role", 5)`.
                None => Some(5),
                // Explicit null: `get_prep_value` passes NULL through and
                // the NOT NULL column raises `IntegrityError` (400) —
                // insert NULL and let the bulk raise it.
                Some(Value::Null) => None,
                Some(value) => match py_int(value) {
                    Ok(role) if (i128::from(i16::MIN)..=i128::from(i16::MAX)).contains(&role) => {
                        Some(role as i16)
                    }
                    _ => return Err(Denial::ServerError),
                },
            };
            models.push(pmi_entity::ActiveModel {
                id: Set(uuid::Uuid::new_v4()),
                created_at: Set(now_ts),
                updated_at: Set(now_ts),
                created_by_id: Set(Some(resolved.id)),
                updated_by_id: Set(None),
                deleted_at: Set(None),
                project_id: Set(project_id),
                workspace_id: Set(workspace_id),
                email: Set(invite.email.clone()),
                accepted: Set(false),
                token: Set(invite.token.clone()),
                message: Set(None),
                responded_at: Set(None),
                role: match role {
                    Some(role) => Set(role),
                    None => sea_orm::ActiveValue::NotSet,
                },
            });
        }
        if let Err(error) = pmi_entity::Entity::insert_many(models)
            .on_conflict_do_nothing()
            .exec(&conn)
            .await
        {
            return Err(orm_integrity_denial(error));
        }
    }
    tx.commit().await.map_err(|_| Denial::ServerError)?;
    // Per-row enqueues (`:104-111`): one `project_invitation` job per
    // invitation, best-effort (without the queue the response stands).
    let current_site = crate::v1_projects::handlers_project::app_origin(&state);
    let inviter = resolved.email.clone().unwrap_or_default();
    for invite in pending.iter() {
        enqueue_message(
            &pool,
            project_invitation_message(
                &invite.email,
                &project_id,
                &invite.token,
                &current_site,
                &inviter,
            ),
        )
        .await;
    }
    Ok(json_response(StatusCode::OK, INVITES_SENT_BODY.to_owned()))
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
    let conn = sea_conn(&pool);
    let now_ts: sea_orm::prelude::DateTimeWithTimeZone = chrono::Utc::now().into();
    pmi_entity::Entity::update_many()
        .col_expr(
            pmi_entity::Column::DeletedAt,
            sea_orm::sea_query::Expr::value(now_ts),
        )
        .col_expr(
            pmi_entity::Column::UpdatedAt,
            sea_orm::sea_query::Expr::value(now_ts),
        )
        .col_expr(
            pmi_entity::Column::UpdatedById,
            sea_orm::sea_query::Expr::value(resolved.id),
        )
        .filter(pmi_entity::Column::Id.eq(pk))
        .filter(pmi_entity::Column::DeletedAt.is_null())
        .exec(&conn)
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
    let props: Value =
        serde_json::from_str(&default_member_props_json()).map_err(|_| Denial::ServerError)?;
    let prefs: Value =
        serde_json::from_str(&default_preferences_json()).map_err(|_| Denial::ServerError)?;
    let rows: Vec<pidash_services::domain::projects::members::BulkRow> = project_ids
        .iter()
        .map(
            |project_id| pidash_services::domain::projects::members::BulkRow {
                workspace_id: *workspace_id,
                project_id: Some(*project_id),
                user_id: *user_id,
                role: pidash_services::domain::projects::members::RoleBind::Int(i64::from(role)),
                created_by: Some(*user_id),
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            },
        )
        .collect();
    pidash_services::domain::projects::members::bulk_add(pool, &rows, &props, &prefs, true)
        .await
        .map_err(integrity_denial)?;
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
    let filters: Value =
        serde_json::from_str(&default_issue_filters_json()).map_err(|_| Denial::ServerError)?;
    let display_filters: Value = serde_json::from_str(&default_issue_display_filters_json())
        .map_err(|_| Denial::ServerError)?;
    let display_properties: Value = serde_json::from_str(&default_issue_display_properties_json())
        .map_err(|_| Denial::ServerError)?;
    let prefs: Value =
        serde_json::from_str(&default_preferences_json()).map_err(|_| Denial::ServerError)?;
    let rich = serde_json::json!({});
    let rows: Vec<pidash_services::domain::projects::members::BulkProperty<'_>> = project_ids
        .iter()
        .map(
            |project_id| pidash_services::domain::projects::members::BulkProperty {
                workspace_id: *workspace_id,
                project_id: *project_id,
                user_id: *user_id,
                filters: &filters,
                display_filters: &display_filters,
                display_properties: &display_properties,
                rich_filters: &rich,
                preferences: &prefs,
                sort_order: 65535.0,
                created_by: Some(*user_id),
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            },
        )
        .collect();
    pidash_services::domain::projects::members::bulk_add_properties(pool, &rows, true)
        .await
        .map_err(integrity_denial)?;
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
/// (`ON CONFLICT DO NOTHING`), answering 201. Unknown ids vanish from
/// the guard's queryset but fail the member insert (FK `IntegrityError`
/// 400); a bad-UUID id is the `ValidationError` 400.
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
    let body = match parse_body(&state, req, &INVITE_BODY_SPEC).await {
        Ok((body, _)) => body,
        Err(response) => return Ok(response),
    };
    // `request.data.get("project_ids", [])` — a non-dict body has no
    // `.get` (`AttributeError` → 500).
    let data = match body.as_object() {
        Some(map) => map,
        None => return Err(Denial::ServerError),
    };
    // `WorkspaceMember.objects.get(...)` (`invite.py:133`) evaluates
    // before the `id__in` list is ever prepped (`:136` is lazy): a miss
    // 404s ahead of any 400. The gate passed, so the row exists; a miss
    // is a race, answering the `.get()` 404 like Python.
    let db = sea_conn(&pool);
    let member = wm_entity::Entity::find()
        .select_only()
        .columns([wm_entity::Column::WorkspaceId, wm_entity::Column::Role])
        .join(JoinType::InnerJoin, wm_entity::Relation::Workspaces.def())
        .filter(wm_entity::Column::MemberId.eq(resolved.id))
        .filter(ws_entity::Column::Slug.eq(slug.clone()))
        .filter(wm_entity::Column::IsActive.eq(true))
        .filter(wm_entity::Column::DeletedAt.is_null())
        .into_tuple::<(uuid::Uuid, i16)>()
        .one(&db)
        .await
        .map_err(|_| Denial::ServerError)?;
    let Some((workspace_id, workspace_role)) = member else {
        return Err(Denial::ObjectNotFound);
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
        // fails); a dict iterates its keys; anything else is not
        // iterable (`TypeError` → 500).
        Some(Value::String(raw)) => {
            if raw.is_empty() {
                Vec::new()
            } else {
                return Err(Denial::BadValidation);
            }
        }
        Some(Value::Object(map)) => {
            let mut ids = Vec::with_capacity(map.len());
            for key in map.keys() {
                ids.push(prep_uuid(&Value::String(key.clone()))?);
            }
            ids
        }
        Some(_) => return Err(Denial::ServerError),
    };
    // `Project.objects.filter(id__in=..., workspace__slug=...)`: live
    // rows only; unknown ids vanish silently.
    let projects: Vec<(uuid::Uuid, i16)> = if project_ids.is_empty() {
        Vec::new()
    } else {
        proj_entity::Entity::find()
            .select_only()
            .columns([proj_entity::Column::Id, proj_entity::Column::Network])
            .join(JoinType::InnerJoin, proj_entity::Relation::Workspaces.def())
            .filter(proj_entity::Column::Id.is_in(project_ids.clone()))
            .filter(ws_entity::Column::Slug.eq(slug.clone()))
            .filter(proj_entity::Column::DeletedAt.is_null())
            .into_tuple::<(uuid::Uuid, i16)>()
            .all(&db)
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
        pidash_services::domain::projects::members::reactivate_slug(
            &pool,
            &slug,
            &project_ids,
            resolved.id,
        )
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
/// `AllowAny`): the invite lookup 404s first, then the email check
/// 403s, then a repeat response 400s, then the response is recorded
/// and an accept creates/reactivates the workspace + project
/// memberships (with the member-save user-property side effect). A
/// decline stops after the record.
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
    // The `.get()` runs before `request.data` is touched
    // (`invite.py:187` precedes `:189`): a miss 404s ahead of any
    // 415 / malformed-body / non-dict failure.
    let pk = pk.parse::<uuid::Uuid>().map_err(|_| Denial::ServerError)?;
    let invite = fetch_join_invite(&pool, &slug, &project_id, &pk).await?;
    let body = match parse_body(&state, req, &INVITE_BODY_SPEC).await {
        Ok((body, _)) => body,
        Err(response) => return Ok(response),
    };
    // `request.data.get(...)` — a non-dict body has no `.get`.
    let data = match body.as_object() {
        Some(map) => map,
        None => return Err(Denial::ServerError),
    };
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
    // `accepted = request.data.get("accepted", False)` runs through
    // `BooleanField.to_python` on `save()` (`invite.py:201-203`): the
    // stored bool is the prepped value, but the accept/decline branch
    // tests the in-memory raw value (`:205`), so `"False"`/`"f"`/`"0"`
    // accept while storing `false` (the workspace join keeps this too).
    let missing = Value::Bool(false);
    let raw = data.get("accepted").unwrap_or(&missing);
    let accepted = prep_bool(raw)?;
    let accept = py_truthy(raw);
    let created_by = caller.as_ref().map(|actor| actor.id);
    // Record the response (`responded_at` + `accepted`, `updated_by` via
    // crum — `NULL` for anonymous joins). An explicit-null `accepted`
    // falls to the NOT NULL column: the `IntegrityError` 400.
    let db = sea_conn(&pool);
    let now_ts: sea_orm::prelude::DateTimeWithTimeZone = chrono::Utc::now().into();
    let mut update = pmi_entity::Entity::update_many();
    update = update.col_expr(
        pmi_entity::Column::Accepted,
        sea_orm::sea_query::Expr::value(accepted),
    );
    update = update.col_expr(
        pmi_entity::Column::RespondedAt,
        sea_orm::sea_query::Expr::value(now_ts),
    );
    update = update.col_expr(
        pmi_entity::Column::UpdatedAt,
        sea_orm::sea_query::Expr::value(now_ts),
    );
    update = update.col_expr(
        pmi_entity::Column::UpdatedById,
        sea_orm::sea_query::Expr::value(created_by),
    );
    update
        .filter(pmi_entity::Column::Id.eq(pk))
        .exec(&db)
        .await
        .map_err(orm_integrity_denial)?;
    if !accept {
        return Ok(json_response(StatusCode::OK, JOIN_DECLINED_BODY.to_owned()));
    }
    // The account lookup: `User.objects.filter(email=...)` (no
    // soft-delete manager on users), `-created_at` first. A missing user
    // flows into the workspace insert as `NULL` and 400s there.
    let user_id = user_entity::Entity::find()
        .select_only()
        .column(user_entity::Column::Id)
        .filter(user_entity::Column::Email.eq(email))
        .order_by_desc(user_entity::Column::CreatedAt)
        .into_tuple::<uuid::Uuid>()
        .one(&db)
        .await
        .map_err(|_| Denial::ServerError)?;
    let db = sea_conn(&pool);
    let ws_member = wm_entity::Entity::find()
        .select_only()
        .column(wm_entity::Column::Id)
        .join(JoinType::InnerJoin, wm_entity::Relation::Workspaces.def())
        .filter(ws_entity::Column::Slug.eq(slug))
        .filter(match user_id {
            Some(id) => wm_entity::Column::MemberId.eq(id),
            None => wm_entity::Column::MemberId.is_null(),
        })
        .filter(wm_entity::Column::DeletedAt.is_null())
        .order_by_desc(wm_entity::Column::CreatedAt)
        .into_tuple::<uuid::Uuid>()
        .one(&db)
        .await
        .map_err(|_| Denial::ServerError)?;
    match ws_member {
        None => {
            let role = if i32::from(invite.role) >= 15 {
                15i16
            } else {
                invite.role
            };
            // The model defaults serialize from fixed `json!` values;
            // reparse for the domain input (jsonb storage normalizes
            // key order either way, so the committed bytes match).
            let view_props: Value = serde_json::from_str(&default_workspace_props_json())
                .map_err(|_| Denial::ServerError)?;
            let issue_props: Value = serde_json::from_str(&default_issue_props_json())
                .map_err(|_| Denial::ServerError)?;
            members::add(
                &pidash_db::pool::sea_orm_connection(&pool),
                &members::NewMember {
                    workspace_id: invite.workspace_id,
                    user_id,
                    role,
                    company_role: None,
                    created_by,
                    view_props: &view_props,
                    default_props: &view_props,
                    issue_props: &issue_props,
                    now: chrono::Utc::now(),
                    ignore_conflicts: false,
                    db_now: true,
                },
            )
            .await
            .map_err(integrity_denial)?;
        }
        Some(id) => {
            // `ws_member` is a SeaORM `Option<Uuid>` (not the old sqlx
            // tuple); the connection follows the main-side convention
            // (`sea_orm_connection`), as in the `members::add` call above.
            members::activate_by_id(&pidash_db::pool::sea_orm_connection(&pool), id, created_by)
                .await
                .map_err(|_| Denial::ServerError)?;
        }
    }
    // The existing project membership is looked up by workspace only —
    // no project predicate (`invite.py:221-223`).
    let project_member = pm_entity::Entity::find()
        .select_only()
        .column(pm_entity::Column::Id)
        .filter(pm_entity::Column::WorkspaceId.eq(invite.workspace_id))
        .filter(match user_id {
            Some(id) => pm_entity::Column::MemberId.eq(id),
            None => pm_entity::Column::MemberId.is_null(),
        })
        .filter(pm_entity::Column::DeletedAt.is_null())
        .order_by_desc(pm_entity::Column::CreatedAt)
        .into_tuple::<uuid::Uuid>()
        .one(&db)
        .await
        .map_err(|_| Denial::ServerError)?;
    match project_member {
        None => {
            create_project_member(&pool, &project_id, user_id, invite.role, created_by).await?;
        }
        Some(id) => {
            // `project_member.role = project_member.role`: the role
            // re-saves unchanged (ported quirk).
            pidash_services::domain::projects::members::reactivate_by_id(&pool, id, created_by)
                .await
                .map_err(|_| Denial::ServerError)?;
        }
    }
    Ok(json_response(StatusCode::OK, JOIN_ACCEPTED_BODY.to_owned()))
}

// ---------------------------------------------------------------------------
// Handlers: favorites
// ---------------------------------------------------------------------------

/// The favorite list-row: the shared 10-key `UserFavoriteSerializer`
/// shape over SeaORM-decoded columns (byte-identical to the sibling
/// renderer, which decodes only from `sqlx` rows).
#[derive(Debug, FromQueryResult)]
struct FavoriteListRow {
    id: uuid::Uuid,
    entity_type: String,
    entity_identifier: Option<uuid::Uuid>,
    name: Option<String>,
    is_folder: bool,
    sequence: f64,
    parent_id: Option<uuid::Uuid>,
    workspace_id: uuid::Uuid,
    project_id: Option<uuid::Uuid>,
}

fn favorite_json_float(value: f64) -> String {
    if !value.is_finite() {
        return match value {
            f64::INFINITY => "Infinity".to_owned(),
            f64::NEG_INFINITY => "-Infinity".to_owned(),
            _ => "NaN".to_owned(),
        };
    }
    crate::paginator::py_float_str(value)
}

fn render_project_favorite(row: &FavoriteListRow, entity: Option<&Value>) -> String {
    let json_string =
        |value: &str| serde_json::to_string(value).unwrap_or_else(|_| "null".to_owned());
    let opt_string = |value: Option<&str>| match value {
        Some(text) => json_string(text),
        None => "null".to_owned(),
    };
    let opt_uuid = |value: Option<uuid::Uuid>| match value {
        Some(id) => json_string(&id.to_string()),
        None => "null".to_owned(),
    };
    let entity_json = entity.map(|value| value.to_string());
    format!(
        "{{\"id\":{},\"entity_type\":{},\"entity_identifier\":{},\"name\":{},\"is_folder\":{},\
         \"sort_order\":null,\"parent\":{},\"sequence\":{},\"project\":{},\"workspace\":{},\
         \"entity\":{}}}",
        json_string(&row.id.to_string()),
        json_string(&row.entity_type),
        opt_uuid(row.entity_identifier),
        opt_string(row.name.as_deref()),
        row.is_folder,
        opt_uuid(row.parent_id),
        favorite_json_float(row.sequence),
        opt_uuid(row.project_id),
        json_string(&row.workspace_id.to_string()),
        entity_json.unwrap_or_else(|| "null".to_owned()),
    )
}

/// `GET .../user-favorite-projects/` (`base.py:503-514`): the DRF-default
/// list over `UserFavoriteSerializer` (PDASHOSS01-293 — the viewset used
/// to inherit `serializer_class = None` and 500, PB-0500 retired). After
/// the auth-only gate, the caller's live favorites in the slug's
/// workspace — every entity type, folders and children included (the
/// queryset filters neither) — `-created_at` first (`Meta.ordering`), as
/// an unpaginated JSON array (no default pagination class). Each row
/// renders the shared 10-key `UserFavoriteSerializer` shape, the same
/// renderer as the workspace `user-favorites/` list.
async fn fav_list(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    extension: Option<axum::Extension<SessionHandle>>,
) -> Result<Response, Denial> {
    use crate::app_workspace::handlers_favorites_drafts::fetch_favorite_entity;
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
    // `.filter(workspace__slug=)` joins the workspace row without its
    // manager's `deleted_at` scope; `select_related` adds no filter (the
    // inner joins follow non-null FKs).
    let rows = uf_entity::Entity::find()
        .select_only()
        .columns([
            uf_entity::Column::Id,
            uf_entity::Column::EntityType,
            uf_entity::Column::EntityIdentifier,
            uf_entity::Column::Name,
            uf_entity::Column::IsFolder,
            uf_entity::Column::Sequence,
            uf_entity::Column::ParentId,
            uf_entity::Column::WorkspaceId,
            uf_entity::Column::ProjectId,
        ])
        .join(JoinType::InnerJoin, uf_entity::Relation::Workspaces.def())
        .filter(uf_entity::Column::DeletedAt.is_null())
        .filter(ws_entity::Column::Slug.eq(slug.clone()))
        .filter(uf_entity::Column::UserId.eq(resolved.id))
        .order_by_desc(uf_entity::Column::CreatedAt)
        .into_model::<FavoriteListRow>()
        .all(&sea_conn(&pool))
        .await
        .map_err(|_| Denial::ServerError)?;
    let mut rendered = Vec::with_capacity(rows.len());
    for row in &rows {
        // The shared helpers fail with the app_issues `Denial`; every
        // failure there is a server error.
        let entity = fetch_favorite_entity(
            &pool,
            &row.entity_type,
            row.entity_identifier
                .as_ref()
                .map(|id| id.to_string())
                .as_deref(),
        )
        .await
        .map_err(|_| Denial::ServerError)?;
        rendered.push(render_project_favorite(row, entity.as_ref()));
    }
    Ok(json_response(
        StatusCode::OK,
        format!("[{}]", rendered.join(",")),
    ))
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
    let body = match parse_body(&state, req, &INVITE_BODY_SPEC).await {
        Ok((body, _)) => body,
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
    let db = sea_conn(&pool);
    let row = proj_entity::Entity::find_by_id(project_id)
        .select_only()
        .column(proj_entity::Column::WorkspaceId)
        .filter(proj_entity::Column::DeletedAt.is_null())
        .into_tuple::<uuid::Uuid>()
        .one(&db)
        .await
        .map_err(|_| Denial::ServerError)?;
    let Some(workspace_id) = row else {
        return Err(Denial::ObjectNotFound);
    };
    #[derive(Debug, FromQueryResult)]
    struct SequenceMax {
        sequence_max: Option<f64>,
    }
    let largest = uf_entity::Entity::find()
        .select_only()
        .column_as(
            sea_orm::sea_query::Expr::col(uf_entity::Column::Sequence).max(),
            "sequence_max",
        )
        .filter(uf_entity::Column::WorkspaceId.eq(workspace_id))
        .filter(uf_entity::Column::DeletedAt.is_null())
        .into_model::<SequenceMax>()
        .one(&db)
        .await
        .map_err(|_| Denial::ServerError)?
        .and_then(|row| row.sequence_max);
    let sequence = largest.map(|max| max + 10000.0).unwrap_or(65535.0);
    let now_ts: sea_orm::prelude::DateTimeWithTimeZone = chrono::Utc::now().into();
    uf_entity::ActiveModel {
        id: Set(uuid::Uuid::new_v4()),
        workspace_id: Set(workspace_id),
        project_id: Set(Some(project_id)),
        user_id: Set(resolved.id),
        entity_type: Set("project".to_owned()),
        entity_identifier: Set(Some(project_id)),
        name: Set(None),
        is_folder: Set(false),
        sequence: Set(sequence),
        parent_id: Set(None),
        created_by_id: Set(Some(resolved.id)),
        updated_by_id: Set(None),
        created_at: Set(now_ts),
        updated_at: Set(now_ts),
        deleted_at: Set(None),
    }
    .insert(&db)
    .await
    .map_err(orm_integrity_denial)?;
    Ok(empty_response(StatusCode::NO_CONTENT))
}

/// `DELETE .../user-favorite-projects/<project_id>/` (`base.py:528-537`):
/// hard-delete (`soft=False`) the caller's matching favorite with its
/// folder subtree, 204. The `<str:project_id>` rewrites like any
/// project kwarg; a miss is the `.get()` 404.
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
    let rows = uf_entity::Entity::find()
        .select_only()
        .column(uf_entity::Column::Id)
        .join(JoinType::InnerJoin, uf_entity::Relation::Workspaces.def())
        .filter(uf_entity::Column::EntityIdentifier.eq(project_id))
        .filter(uf_entity::Column::EntityType.eq("project"))
        .filter(uf_entity::Column::ProjectId.eq(project_id))
        .filter(uf_entity::Column::UserId.eq(resolved.id))
        .filter(ws_entity::Column::Slug.eq(slug))
        .filter(uf_entity::Column::DeletedAt.is_null())
        .into_tuple::<uuid::Uuid>()
        .all(&sea_conn(&pool))
        .await
        .map_err(|_| Denial::ServerError)?;
    if rows.len() > 1 {
        return Err(Denial::ServerError);
    }
    let Some(id) = rows.first().copied() else {
        return Err(Denial::ObjectNotFound);
    };
    // Hard delete cascades through the self-FK (`parent`,
    // `on_delete=CASCADE`): Django's Collector gathers descendants via
    // the unfiltered base manager, so children in any `deleted_at`
    // state go with the row — one statement, `NO ACTION` checks at
    // statement end. No signals hang off the model.
    // raw-sql: recursive CTE subtree delete cannot be built with the entity API.
    sqlx::query(
        "WITH RECURSIVE subtree AS (
             SELECT id FROM user_favorites WHERE id = $1
             UNION ALL
             SELECT f.id FROM user_favorites f JOIN subtree s ON f.parent_id = s.id
         )
         DELETE FROM user_favorites WHERE id IN (SELECT id FROM subtree)",
    )
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
/// missing project row is the `.get()` 404, raised by the create path's
/// workspace backfill (the `get` hit never reads the project row); any
/// non-null `intake` raises `ValueError` on the FK-descriptor
/// assignment (the generic 500); a bad bool flag is the
/// `ValidationError` 400; an explicit-null flag or view falls to its
/// NOT NULL column (the `IntegrityError` 400). The intake and flag
/// failures land after `get_or_create`, so a just-created row persists
/// past them.
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
    let body = match parse_body(&state, req, &INVITE_BODY_SPEC).await {
        Ok((body, _)) => body,
        Err(response) => return Ok(response),
    };
    // `request.data.get(...)` — a non-dict body has no `.get`.
    let data = match body.as_object() {
        Some(map) => map,
        None => return Err(Denial::ServerError),
    };
    let views = match data.get("views") {
        None => serde_json::from_str(&default_board_views_json()).expect("default views parse"),
        Some(value) => value.clone(),
    };
    // `get_or_create(entity_name, entity_identifier, project_id)` —
    // unscoped (no workspace predicate), live manager only. The `get`
    // runs before anything else touches the database: when it hits, no
    // project row is ever read (a deleted project under a live board
    // still answers 200).
    let db = sea_conn(&pool);
    let existing = db_entity::Entity::find()
        .select_only()
        .column(db_entity::Column::Id)
        .filter(db_entity::Column::EntityName.eq("project"))
        .filter(db_entity::Column::EntityIdentifier.eq(project_id))
        .filter(db_entity::Column::ProjectId.eq(project_id))
        .filter(db_entity::Column::DeletedAt.is_null())
        .into_tuple::<uuid::Uuid>()
        .one(&db)
        .await
        .map_err(|_| Denial::ServerError)?;
    let board_id = match existing {
        Some(id) => id,
        None => {
            // The create path's workspace backfill
            // (`WorkspaceBaseModel.save`) fetches the project: a miss is
            // the 404, before the row is inserted.
            let workspace = proj_entity::Entity::find_by_id(project_id)
                .select_only()
                .column(proj_entity::Column::WorkspaceId)
                .filter(proj_entity::Column::DeletedAt.is_null())
                .into_tuple::<uuid::Uuid>()
                .one(&db)
                .await
                .map_err(|_| Denial::ServerError)?;
            let Some(workspace_id) = workspace else {
                return Err(Denial::ObjectNotFound);
            };
            let id = uuid::Uuid::new_v4();
            let now_ts: sea_orm::prelude::DateTimeWithTimeZone = chrono::Utc::now().into();
            let insert = db_entity::ActiveModel {
                id: Set(id),
                workspace_id: Set(workspace_id),
                project_id: Set(Some(project_id)),
                entity_identifier: Set(Some(project_id)),
                entity_name: Set(Some("project".to_owned())),
                anchor: Set(uuid::Uuid::new_v4().simple().to_string()),
                is_comments_enabled: Set(false),
                is_reactions_enabled: Set(false),
                intake_id: Set(None),
                is_votes_enabled: Set(false),
                view_props: Set(serde_json::Map::new().into()),
                is_activity_enabled: Set(true),
                is_disabled: Set(false),
                created_by_id: Set(Some(resolved.id)),
                updated_by_id: Set(None),
                created_at: Set(now_ts),
                updated_at: Set(now_ts),
                deleted_at: Set(None),
            }
            .insert(&db)
            .await;
            match insert {
                Ok(_) => id,
                Err(err) => {
                    if !is_integrity_error(&pidash_db::orm_tx::into_sqlx(err)) {
                        return Err(Denial::ServerError);
                    }
                    // The `get_or_create` retry: one more `get`, whose
                    // miss raises like the first.
                    let retry = db_entity::Entity::find()
                        .select_only()
                        .column(db_entity::Column::Id)
                        .filter(db_entity::Column::EntityName.eq("project"))
                        .filter(db_entity::Column::EntityIdentifier.eq(project_id))
                        .filter(db_entity::Column::ProjectId.eq(project_id))
                        .filter(db_entity::Column::DeletedAt.is_null())
                        .into_tuple::<uuid::Uuid>()
                        .one(&db)
                        .await
                        .map_err(|_| Denial::ServerError)?;
                    let Some(id) = retry else {
                        return Err(Denial::ObjectNotFound);
                    };
                    id
                }
            }
        }
    };
    // `project_deploy_board.intake = ...` runs after `get_or_create`
    // (a created row persists past the failure): the FK-descriptor
    // assignment only accepts an `Intake` instance or `None` — any JSON
    // value raises `ValueError`, the generic 500.
    if !matches!(data.get("intake"), None | Some(Value::Null)) {
        return Err(Denial::ServerError);
    }
    // The second save's `BooleanField.get_prep_value`, in model field
    // order: missing is `False`, explicit `null` stays `None` (the NOT
    // NULL column raises the `IntegrityError` 400 on the write),
    // anything else preps or is the `ValidationError` 400. A created row
    // persists past these failures too.
    let comments = prep_bool_flag(data.get("is_comments_enabled"))?;
    let reactions = prep_bool_flag(data.get("is_reactions_enabled"))?;
    let votes = prep_bool_flag(data.get("is_votes_enabled"))?;
    // The assignment + second save: flags, views and intake land here
    // (explicit nulls fall to their NOT NULL columns → 400), and
    // `updated_by`/`updated_at` stamp on both the created and the found
    // path.
    let views_text = serde_json::to_string(&views).expect("views serialize");
    let now_ts: sea_orm::prelude::DateTimeWithTimeZone = chrono::Utc::now().into();
    db_entity::Entity::update_many()
        .col_expr(
            db_entity::Column::IntakeId,
            sea_orm::sea_query::Expr::value(None::<uuid::Uuid>),
        )
        .col_expr(
            db_entity::Column::ViewProps,
            sea_orm::sea_query::Expr::value(if views.is_null() {
                None
            } else {
                Some(serde_json::from_str::<serde_json::Value>(&views_text).expect("views parse"))
            }),
        )
        .col_expr(
            db_entity::Column::IsVotesEnabled,
            sea_orm::sea_query::Expr::value(votes),
        )
        .col_expr(
            db_entity::Column::IsCommentsEnabled,
            sea_orm::sea_query::Expr::value(comments),
        )
        .col_expr(
            db_entity::Column::IsReactionsEnabled,
            sea_orm::sea_query::Expr::value(reactions),
        )
        .col_expr(
            db_entity::Column::UpdatedAt,
            sea_orm::sea_query::Expr::value(now_ts),
        )
        .col_expr(
            db_entity::Column::UpdatedById,
            sea_orm::sea_query::Expr::value(resolved.id),
        )
        .filter(db_entity::Column::Id.eq(board_id))
        .exec(&db)
        .await
        .map_err(orm_integrity_denial)?;
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
    let now_ts: sea_orm::prelude::DateTimeWithTimeZone = chrono::Utc::now().into();
    db_entity::Entity::update_many()
        .col_expr(
            db_entity::Column::DeletedAt,
            sea_orm::sea_query::Expr::value(now_ts),
        )
        .col_expr(
            db_entity::Column::UpdatedAt,
            sea_orm::sea_query::Expr::value(now_ts),
        )
        .col_expr(
            db_entity::Column::UpdatedById,
            sea_orm::sea_query::Expr::value(resolved.id),
        )
        .filter(db_entity::Column::Id.eq(pk))
        .filter(db_entity::Column::DeletedAt.is_null())
        .exec(&sea_conn(&pool))
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
/// `deleted_at`: datetimes parse (RFC 3339 plus the space-separated,
/// minute-precision and colon-less-offset spellings Django's
/// `parse_datetime` accepts — single-digit components ride chrono's
/// padding leniency, trailing whitespace rides the regex's `\s*`),
/// naive values assume UTC (`USE_TZ`, `TIME_ZONE = "UTC"`).
fn validate_patch_datetime(value: &Value) -> Result<Option<chrono::DateTime<chrono::Utc>>, String> {
    if value.is_null() {
        return Ok(None);
    }
    let raw_input = match value {
        Value::String(raw) => raw,
        _ => return Err(datetime_format_error()),
    };
    let raw = raw_input.trim_end();
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(raw) {
        return Ok(Some(dt.with_timezone(&chrono::Utc)));
    }
    for format in [
        "%Y-%m-%dT%H:%M:%S%.f%z",
        "%Y-%m-%d %H:%M:%S%.f%z",
        "%Y-%m-%dT%H:%M%z",
        "%Y-%m-%d %H:%M%z",
    ] {
        if let Ok(dt) = chrono::DateTime::parse_from_str(raw, format) {
            return Ok(Some(dt.with_timezone(&chrono::Utc)));
        }
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
    // DRF's `strptime(value, 'iso-8601')` fallthrough (`to_internal_value`
    // runs it when `parse_datetime` returns `None` — PIDASHCONV-773):
    // the literal matches case-insensitively and yields naive
    // 1900-01-01, read as UTC like every naive input here. Checked on
    // the RAW text — Django's `strptime` sees the value before any
    // strip (this port's `trim_end` is a pre-existing quirk), so a
    // padded literal still fails. Exact match (probed); ASCII-only
    // (765 unicode-gap family).
    if raw_input.eq_ignore_ascii_case("iso-8601") {
        let naive = chrono::NaiveDate::from_ymd_opt(1900, 1, 1)
            .and_then(|date| date.and_hms_opt(0, 0, 0))
            .expect("1900-01-01 valid");
        return Ok(Some(naive.and_utc()));
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
/// null), and so does the exact empty string (DRF
/// `RelatedField.run_validation` forces `''` to `None` on every input,
/// JSON included — PIDASHCONV-1518); bools fail `incorrect_type`
/// outright (DRF raises `TypeError` before the lookup); strings and
/// ints prep as UUIDs — a prep failure is Django's curly-quote
/// `invalid` message *as a field error* (DRF
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
    if matches!(value, Value::String(text) if text.is_empty()) {
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
    let db = sea_conn(pool);
    let row = match (table, live_only) {
        ("users", false) => user_entity::Entity::find_by_id(id)
            .select_only()
            .column(user_entity::Column::Id)
            .into_tuple::<uuid::Uuid>()
            .one(&db)
            .await
            .map_err(|_| Denial::ServerError)?,
        ("intakes", true) => pidash_db::entity::intakes::Entity::find_by_id(id)
            .select_only()
            .column(pidash_db::entity::intakes::Column::Id)
            .filter(pidash_db::entity::intakes::Column::DeletedAt.is_null())
            .into_tuple::<uuid::Uuid>()
            .one(&db)
            .await
            .map_err(|_| Denial::ServerError)?,
        _ => None,
    };
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
    let (body, from_form) = match parse_body(&state, req, &BOARD_PATCH_BODY_SPEC).await {
        Ok(parsed) => parsed,
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
    // HTML-only input rules (DRF `Field.get_value`, `fields.py:407-429` +
    // `JSONField.get_value`, verified live — PIDASHCONV-1508): a blank
    // `allow_null` scalar arrives as `None` (the blank-skips in
    // `BOARD_PATCH_BODY_SPEC` already dropped the bools and
    // `updated_by`), and an HTML `view_props` string JSON-decodes (the
    // `JSONString` mark) — or fails the field below. The JSON arm skips
    // all of this: `""` runs `to_internal_value` there, except on the
    // PK keys, where the `RelatedField` rule maps it to null first
    // (PIDASHCONV-1518).
    let mut data = data.clone();
    let mut view_props_json_failed = false;
    let mut view_props_decoded_null = false;
    if from_form {
        for key in ["entity_identifier", "deleted_at", "created_by", "intake"] {
            if matches!(data.get(key), Some(Value::String(text)) if text.is_empty()) {
                data.insert(key.to_owned(), Value::Null);
            }
        }
        if let Some(Value::String(text)) = data.remove("view_props") {
            match serde_json::from_str(&text) {
                Ok(Value::Null) => view_props_decoded_null = true,
                Ok(parsed) => {
                    data.insert("view_props".to_owned(), parsed);
                }
                Err(_) => view_props_json_failed = true,
            }
        }
    }
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
    if view_props_json_failed {
        errors.push("view_props", "Value must be valid JSON.".to_owned());
    } else if view_props_decoded_null {
        // HTML `view_props=null` decodes to None after
        // `validate_empty_values` ran (the `JSONString` is non-empty, so
        // the null check never fires): it validates, then dies at save
        // with the not-null `IntegrityError` 400. The NULL set below
        // reproduces that write; sibling field errors still win first.
        sets.push(("view_props", PatchValue::Null));
    } else if let Some(value) = data.get("view_props") {
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
                // `updated_by` validates (a bad value still 400s) but
                // never writes: `BaseModel.save()` re-stamps
                // `updated_by` to the current user on every update
                // (`db/models/base.py`, via crum), overwriting the
                // validated value — so the audit stamp below always
                // wins, and emitting both would double-SET the column
                // (Postgres 42601, PIDASHCONV-1518).
                PkValidation::Null => {
                    if field != "updated_by" {
                        sets.push((column, PatchValue::Null));
                    }
                }
                PkValidation::Id(id) => {
                    if field != "updated_by" {
                        sets.push((column, PatchValue::Uuid(id)));
                    }
                }
                PkValidation::FieldError(message) => errors.push(field, message),
            }
        }
    }
    if !errors.is_empty() {
        return Err(Denial::Raw(StatusCode::BAD_REQUEST, errors.body()));
    }
    // `UniqueTogetherValidator` over (`entity_name`,
    // `entity_identifier`, `deleted_at`) (`validators.py`, update path):
    // missing keys fill from the instance, then the check runs only
    // when a triple field changed — and never when a changed value is
    // `None`. The clash filter carries the effective `deleted_at`, and
    // live rows all carry null, so a set `deleted_at` never matches
    // (soft-delete-plus-rename PATCHes pass). DRF derives no validator
    // from the conditional `UniqueConstraint`: at most the one triple
    // message renders, and a write-time race there surfaces as the
    // `IntegrityError` 400 instead.
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
    let mut any_changed = false;
    let mut changed_null = false;
    if new_name != current.entity_name {
        any_changed = true;
        changed_null |= new_name.is_none();
    }
    if new_identifier != current.entity_identifier {
        any_changed = true;
        changed_null |= new_identifier.is_none();
    }
    if new_deleted_at != current.deleted_at {
        any_changed = true;
        changed_null |= new_deleted_at.is_none();
    }
    if any_changed && !changed_null && new_deleted_at.is_none() {
        let clash = db_entity::Entity::find()
            .select_only()
            .column(db_entity::Column::Id)
            .filter(match new_name {
                Some(name) => db_entity::Column::EntityName.eq(name),
                None => db_entity::Column::EntityName.is_null(),
            })
            .filter(match new_identifier {
                Some(id) => db_entity::Column::EntityIdentifier.eq(id),
                None => db_entity::Column::EntityIdentifier.is_null(),
            })
            .filter(db_entity::Column::DeletedAt.is_null())
            .filter(db_entity::Column::Id.ne(pk))
            .into_tuple::<uuid::Uuid>()
            .one(&sea_conn(&pool))
            .await
            .map_err(|_| Denial::ServerError)?;
        if clash.is_some() {
            errors.push(
                "non_field_errors",
                "The fields entity_name, entity_identifier, deleted_at must make a unique set."
                    .to_owned(),
            );
        }
    }
    if !errors.is_empty() {
        return Err(Denial::Raw(StatusCode::BAD_REQUEST, errors.body()));
    }
    // `ModelSerializer.update`: one `save()` — the assignments plus the
    // audit stamp (an empty PATCH stamps too).
    let mut update = db_entity::Entity::update_many();
    for (column, value) in &sets {
        update = match (*column, value) {
            ("entity_name", PatchValue::Text(text)) => update.col_expr(
                db_entity::Column::EntityName,
                sea_orm::sea_query::Expr::value(text.clone()),
            ),
            ("entity_name", PatchValue::Null) => update.col_expr(
                db_entity::Column::EntityName,
                sea_orm::sea_query::Expr::value(None::<String>),
            ),
            ("entity_identifier", PatchValue::Uuid(id)) => update.col_expr(
                db_entity::Column::EntityIdentifier,
                sea_orm::sea_query::Expr::value(*id),
            ),
            ("entity_identifier", PatchValue::Null) => update.col_expr(
                db_entity::Column::EntityIdentifier,
                sea_orm::sea_query::Expr::value(None::<uuid::Uuid>),
            ),
            ("deleted_at", PatchValue::Timestamp(dt)) => update.col_expr(
                db_entity::Column::DeletedAt,
                sea_orm::sea_query::Expr::value(*dt),
            ),
            ("deleted_at", PatchValue::Null) => update.col_expr(
                db_entity::Column::DeletedAt,
                sea_orm::sea_query::Expr::value(None::<chrono::DateTime<chrono::Utc>>),
            ),
            ("is_comments_enabled", PatchValue::Bool(flag)) => update.col_expr(
                db_entity::Column::IsCommentsEnabled,
                sea_orm::sea_query::Expr::value(*flag),
            ),
            ("is_comments_enabled", PatchValue::Null) => update.col_expr(
                db_entity::Column::IsCommentsEnabled,
                sea_orm::sea_query::Expr::value(None::<bool>),
            ),
            ("is_reactions_enabled", PatchValue::Bool(flag)) => update.col_expr(
                db_entity::Column::IsReactionsEnabled,
                sea_orm::sea_query::Expr::value(*flag),
            ),
            ("is_reactions_enabled", PatchValue::Null) => update.col_expr(
                db_entity::Column::IsReactionsEnabled,
                sea_orm::sea_query::Expr::value(None::<bool>),
            ),
            ("is_votes_enabled", PatchValue::Bool(flag)) => update.col_expr(
                db_entity::Column::IsVotesEnabled,
                sea_orm::sea_query::Expr::value(*flag),
            ),
            ("is_votes_enabled", PatchValue::Null) => update.col_expr(
                db_entity::Column::IsVotesEnabled,
                sea_orm::sea_query::Expr::value(None::<bool>),
            ),
            ("is_activity_enabled", PatchValue::Bool(flag)) => update.col_expr(
                db_entity::Column::IsActivityEnabled,
                sea_orm::sea_query::Expr::value(*flag),
            ),
            ("is_activity_enabled", PatchValue::Null) => update.col_expr(
                db_entity::Column::IsActivityEnabled,
                sea_orm::sea_query::Expr::value(None::<bool>),
            ),
            ("is_disabled", PatchValue::Bool(flag)) => update.col_expr(
                db_entity::Column::IsDisabled,
                sea_orm::sea_query::Expr::value(*flag),
            ),
            ("is_disabled", PatchValue::Null) => update.col_expr(
                db_entity::Column::IsDisabled,
                sea_orm::sea_query::Expr::value(None::<bool>),
            ),
            ("view_props", PatchValue::Json(text)) => update.col_expr(
                db_entity::Column::ViewProps,
                sea_orm::sea_query::Expr::value(
                    serde_json::from_str::<serde_json::Value>(text).expect("view_props parse"),
                ),
            ),
            ("view_props", PatchValue::Null) => update.col_expr(
                db_entity::Column::ViewProps,
                sea_orm::sea_query::Expr::value(None::<serde_json::Value>),
            ),
            ("created_by_id", PatchValue::Uuid(id)) => update.col_expr(
                db_entity::Column::CreatedById,
                sea_orm::sea_query::Expr::value(*id),
            ),
            ("created_by_id", PatchValue::Null) => update.col_expr(
                db_entity::Column::CreatedById,
                sea_orm::sea_query::Expr::value(None::<uuid::Uuid>),
            ),
            ("intake_id", PatchValue::Uuid(id)) => update.col_expr(
                db_entity::Column::IntakeId,
                sea_orm::sea_query::Expr::value(*id),
            ),
            ("intake_id", PatchValue::Null) => update.col_expr(
                db_entity::Column::IntakeId,
                sea_orm::sea_query::Expr::value(None::<uuid::Uuid>),
            ),
            _ => update,
        };
    }
    update = update
        .col_expr(
            db_entity::Column::UpdatedAt,
            sea_orm::sea_query::Expr::value(chrono::Utc::now()),
        )
        .col_expr(
            db_entity::Column::UpdatedById,
            sea_orm::sea_query::Expr::value(resolved.id),
        );
    update
        .filter(db_entity::Column::Id.eq(pk))
        .exec(&sea_conn(&pool))
        .await
        .map_err(orm_integrity_denial)?;
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
    use pidash_services::app_project::tasks::{detail_body, handle_exception};

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

    /// `matches_json_parser` truth table, mirroring DRF
    /// `media_type_matches('application/json', content_type)`
    /// (`rest_framework/utils/mediatypes.py`), verified against the
    /// installed DRF (PIDASHCONV-1517).
    #[test]
    fn json_parser_match_table() {
        let cases: &[(&str, bool)] = &[
            ("application/json", true),
            ("Application/JSON", true),
            ("application/json; charset=utf-8", true),
            ("*/*", true),
            ("application/*", true),
            ("*/json", true),
            ("text/plain", false),
            ("", false),
            ("foo", false),
            ("application/json-patch", false),
            ("text/*", false),
            ("multipart/*", false),
            ("*", false),
            ("application/xml", false),
            ("application / json", false),
            ("* /*", false),
        ];
        for (content_type, want) in cases {
            assert_eq!(
                matches_json_parser(content_type),
                *want,
                "content type {content_type:?}"
            );
        }
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
        // The generic FX-APROJ-08 500 body. (Neither invite create nor the
        // favorites list 500s any more — the `d656243d` re-sync,
        // PIDASHCONV-1085, and PDASHOSS01-293; the `invite_create_500` and
        // `favorites_list_500` fixture entries are pre-fix probe records.)
        let fx08 = fixture("FX-APROJ-08.tasks.json");
        assert_eq!(parse(SERVER_ERROR_BODY), fx08["generic_500_body"]);
        // The fixed invite-create bodies (no fixture entry — pinned here).
        assert_eq!(
            parse(ROLE_MISMATCH_BODY),
            serde_json::json!({"error": "You cannot invite a user with different role than workspace role"})
        );
        assert_eq!(
            parse(INVITES_SENT_BODY),
            serde_json::json!({"message": "Email sent successfully"})
        );
    }

    #[test]
    fn invite_message_shape() {
        let project_id = uuid::Uuid::parse_str("5952e951-baad-4770-8ee8-64faf7f753fd").unwrap();
        let message = project_invitation_message(
            "new@example.com",
            &project_id,
            "tok",
            "https://app.example",
            "boss@example.com",
        );
        assert_eq!(
            message.task,
            "pi_dash.bgtasks.project_invitation_task.project_invitation"
        );
        assert_eq!(
            message.args,
            vec![
                Value::String("new@example.com".to_owned()),
                Value::String("5952e951-baad-4770-8ee8-64faf7f753fd".to_owned()),
                Value::String("tok".to_owned()),
                Value::String("https://app.example".to_owned()),
                Value::String("boss@example.com".to_owned()),
            ]
        );
        assert!(message.kwargs.is_empty());
    }

    #[test]
    fn role_match_matrix() {
        // Equal roles pass; anything else rejects.
        assert!(role_matches_workspace(5, Some(&serde_json::json!(5))));
        assert!(role_matches_workspace(20, Some(&serde_json::json!(20))));
        assert!(role_matches_workspace(5, None));
        assert!(!role_matches_workspace(20, None));
        assert!(!role_matches_workspace(5, Some(&serde_json::json!(15))));
        assert!(!role_matches_workspace(20, Some(&serde_json::json!(5))));
        // Python numeric equality: 5 == 5.0.
        assert!(role_matches_workspace(5, Some(&serde_json::json!(5.0))));
        // Strings, bools, null and containers never equal the int.
        assert!(!role_matches_workspace(5, Some(&serde_json::json!("5"))));
        assert!(!role_matches_workspace(5, Some(&serde_json::json!(true))));
        assert!(!role_matches_workspace(5, Some(&serde_json::json!(null))));
        assert!(!role_matches_workspace(5, Some(&serde_json::json!([5]))));
    }

    #[test]
    fn emails_input_classification() {
        assert!(matches!(classify_emails_input(None), EmailsInput::Missing));
        for falsy in [
            serde_json::json!(null),
            serde_json::json!([]),
            serde_json::json!({}),
            serde_json::json!(""),
            serde_json::json!(0),
            serde_json::json!(false),
        ] {
            assert!(
                matches!(classify_emails_input(Some(&falsy)), EmailsInput::Missing),
                "falsy pins Missing: {falsy}"
            );
        }
        for truthy in [
            serde_json::json!("x"),
            serde_json::json!(5),
            serde_json::json!({"a": 1}),
            serde_json::json!(true),
        ] {
            assert!(
                matches!(
                    classify_emails_input(Some(&truthy)),
                    EmailsInput::ServerError
                ),
                "truthy non-list pins 500: {truthy}"
            );
        }
        let entries = serde_json::json!([{"email": "a@b.c"}]);
        assert!(matches!(
            classify_emails_input(Some(&entries)),
            EmailsInput::Entries(_)
        ));
    }

    #[test]
    fn lookup_email_rendering() {
        assert_eq!(invite_lookup_email(None), None);
        assert_eq!(invite_lookup_email(Some(&serde_json::json!(null))), None);
        assert_eq!(
            invite_lookup_email(Some(&serde_json::json!("a@b.c"))),
            Some("a@b.c".to_owned())
        );
        assert_eq!(
            invite_lookup_email(Some(&serde_json::json!(5))),
            Some("5".to_owned())
        );
        assert_eq!(
            invite_lookup_email(Some(&serde_json::json!(true))),
            Some("True".to_owned())
        );
        assert_eq!(
            invite_lookup_email(Some(&serde_json::json!({"a": 1}))),
            Some("{'a': 1}".to_owned())
        );
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
        // Django's `parse_datetime` extras: colon-less offsets,
        // single-digit components, trailing whitespace.
        let flat = validate_patch_datetime(&Value::String("2026-10-02T22:22:39+0200".to_owned()))
            .unwrap()
            .unwrap();
        assert_eq!(
            flat.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            "2026-10-02T20:22:39Z"
        );
        let padded = validate_patch_datetime(&Value::String("2026-1-2T3:04 ".to_owned()))
            .unwrap()
            .unwrap();
        assert_eq!(
            padded.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            "2026-01-02T03:04:00Z"
        );
        let err = validate_patch_datetime(&Value::String("tomorrow".to_owned())).unwrap_err();
        assert!(err.starts_with("Datetime has wrong format. Use one of these formats instead: "));
        assert!(validate_patch_datetime(&serde_json::json!(5)).is_err());
    }

    #[test]
    fn patch_datetime_iso8601_literal_fallback() {
        // PIDASHCONV-773: DRF `to_internal_value` falls through to
        // `strptime(value, 'iso-8601')` when `parse_datetime` returns None;
        // the literal matches case-insensitively and yields naive
        // 1900-01-01, read as UTC (probed live both backends).
        for text in [
            "iso-8601", "ISO-8601", "Iso-8601", "iSo-8601", "isO-8601", "ISo-8601", "IsO-8601",
            "iSO-8601",
        ] {
            let parsed = validate_patch_datetime(&Value::String(text.to_owned()))
                .expect(text)
                .expect("some");
            assert_eq!(
                parsed.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                "1900-01-01T00:00:00Z",
                "{text:?}"
            );
        }
        // Near-misses stay invalid (exact match on the raw text, both
        // sides probed — padding fails even though this port trims the
        // tail for datetimes).
        for text in [
            "iso8601",
            "xiso-8601",
            "iso-8601x",
            " iso-8601",
            "iso-8601 ",
            "iso-8601\n",
            "\tiso-8601",
        ] {
            assert!(
                validate_patch_datetime(&Value::String(text.to_owned())).is_err(),
                "{text:?}"
            );
        }
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

    #[test]
    fn html_body_negotiates() {
        // PIDASHCONV-1508: form/multipart parse through the shared DRF
        // negotiation (last value wins; blanks kept or skipped per spec).
        let headers_for = |content_type: &str, len: usize| {
            let mut headers = axum::http::HeaderMap::new();
            headers.insert("content-type", content_type.parse().unwrap());
            headers.insert("content-length", len.to_string().parse().unwrap());
            headers
        };
        // Raw-`.get` handlers: blanks stay in the map, last value wins.
        let body = b"email=x%40example.com&accepted=0&accepted=1";
        let headers = headers_for("application/x-www-form-urlencoded", body.len());
        let value = parse_html_body(&headers, body, &INVITE_BODY_SPEC).expect("form");
        assert_eq!(value["email"], Value::String("x@example.com".to_owned()));
        assert_eq!(value["accepted"], Value::String("1".to_owned()));
        let body = b"emails=";
        let headers = headers_for(
            "application/x-www-form-urlencoded; charset=utf-8",
            body.len(),
        );
        let value = parse_html_body(&headers, body, &INVITE_BODY_SPEC).expect("blank form");
        assert_eq!(value["emails"], Value::String(String::new()));
        // Board PATCH: blank bools and `updated_by` behave as absent;
        // other blanks stay for the handler's blank-to-null rules.
        let body = b"is_votes_enabled=&updated_by=&entity_name=&intake=";
        let headers = headers_for("application/x-www-form-urlencoded", body.len());
        let value = parse_html_body(&headers, body, &BOARD_PATCH_BODY_SPEC).expect("patch form");
        let map = value.as_object().expect("object");
        assert!(!map.contains_key("is_votes_enabled"));
        assert!(!map.contains_key("updated_by"));
        assert_eq!(map["entity_name"], Value::String(String::new()));
        assert_eq!(map["intake"], Value::String(String::new()));
        // Text-only multipart decodes like the equivalent form body.
        let body = b"--BOUND\r\nContent-Disposition: form-data; name=\"accepted\"\r\n\r\n0\r\n--BOUND--\r\n";
        let headers = headers_for("multipart/form-data; boundary=BOUND", body.len());
        let value = parse_html_body(&headers, body, &INVITE_BODY_SPEC).expect("multipart");
        assert_eq!(value["accepted"], Value::String("0".to_owned()));
        // Multipart without a boundary is DRF's `ParseError` 400.
        let body = b"not multipart";
        let headers = headers_for("multipart/form-data", body.len());
        let response = parse_html_body(&headers, body, &INVITE_BODY_SPEC).expect_err("400");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
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

/// Invite reads against constraint-free temp tables on a one-connection
/// pool: the `project_member_invites` JOIN `projects` JOIN `workspaces`
/// queryset keeps its soft-delete scope, slug+project filters and
/// `-created_at` order, and the single-row fetch keeps its 404 miss.
#[cfg(test)]
mod live_invite_reads {
    use super::*;
    use uuid::Uuid;

    async fn live_pool() -> Option<sqlx::PgPool> {
        let Ok(url) = std::env::var("DATABASE_URL") else {
            eprintln!("skipping live-db test: DATABASE_URL is not set");
            return None;
        };
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .expect("connect");
        for table in ["workspaces", "projects", "project_member_invites"] {
            sqlx::query(&format!(
                "CREATE TEMP TABLE {table} AS SELECT * FROM public.{table} WITH NO DATA"
            ))
            .execute(&pool)
            .await
            .expect("temp table");
        }
        Some(pool)
    }

    async fn seed(pool: &sqlx::PgPool, ws: Uuid, proj: Uuid) {
        for sql in [
            format!(
                "INSERT INTO workspaces (id, name, slug) VALUES ('{ws}', 'Acme', 'acme')"
            ),
            format!(
                "INSERT INTO projects (id, identifier, name, description, logo_props, is_default, workspace_id) \
                 VALUES ('{proj}', 'ENG', 'Engine', '', '{{}}', false, '{ws}')"
            ),
        ] {
            sqlx::query(&sql).execute(pool).await.expect("seed");
        }
    }

    #[tokio::test]
    async fn live_fetch_invites() {
        let Some(pool) = live_pool().await else {
            return;
        };
        let (ws, proj, other_proj, older, newer, gone) = (
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
        );
        seed(&pool, ws, proj).await;
        sqlx::query(&format!(
            "INSERT INTO projects (id, identifier, name, description, logo_props, is_default, workspace_id) \
             VALUES ('{other_proj}', 'OTS', 'Other', '', '{{}}', false, '{ws}')"
        ))
        .execute(&pool)
        .await
        .expect("seed");
        for sql in [
            format!(
                "INSERT INTO project_member_invites (id, email, accepted, token, role, project_id, workspace_id, created_at, updated_at, deleted_at) \
                 VALUES ('{older}', 'old@x.io', false, 't-old', 1, '{proj}', '{ws}', now() - interval '1 day', now(), NULL), \
                        ('{newer}', 'new@x.io', false, 't-new', 2, '{proj}', '{ws}', now(), now(), NULL), \
                        ('{gone}', 'gone@x.io', false, 't-gone', 1, '{proj}', '{ws}', now(), now(), now()), \
                        (gen_random_uuid(), 'other@x.io', false, 't-other', 1, '{other_proj}', '{ws}', now(), now(), NULL)"
            ),
        ] {
            sqlx::query(&sql).execute(&pool).await.expect("seed");
        }
        let rows = fetch_invites(&pool, "acme", &proj).await.expect("read");
        assert_eq!(
            rows.iter()
                .map(|row| row.email.as_str())
                .collect::<Vec<_>>(),
            vec!["new@x.io", "old@x.io"],
            "live only, newest first"
        );
        assert_eq!(rows[0].project_identifier, "ENG");
        assert_eq!(rows[0].workspace_slug, "acme");

        let one = fetch_invite_one(&pool, "acme", &proj, &older)
            .await
            .expect("one");
        assert_eq!(one.email, "old@x.io");
        assert!(matches!(
            fetch_invite_one(&pool, "acme", &proj, &gone).await,
            Err(Denial::InviteNotFound)
        ));
        assert!(matches!(
            fetch_invite_one(&pool, "missing-slug", &proj, &older).await,
            Err(Denial::InviteNotFound)
        ));

        let mine = fetch_user_invites(&pool, Some("old@x.io"))
            .await
            .expect("user invites");
        assert_eq!(
            mine.iter()
                .map(|row| row.email.as_str())
                .collect::<Vec<_>>(),
            vec!["old@x.io"]
        );
        let none = fetch_user_invites(&pool, Some("nobody@x.io"))
            .await
            .expect("user invites miss");
        assert!(none.is_empty());
    }
}
