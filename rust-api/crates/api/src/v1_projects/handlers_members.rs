//! Member / invite / user handlers (D-19, stage 5, PIDASHCONV-371).
//!
//! Ports `apps/api/pi_dash/api/views/{member,invite,user}.py` onto the
//! merged query layer (PIDASHCONV-364) and permission gates (PIDASHCONV-367):
//!
//! * `GET workspaces/<slug>/members/` (`WorkspaceMemberAPIEndpoint`, `member.py:31-92`)
//! * `GET+POST .../projects/<project_id>/members/` and the `project-members/`
//!   alias pair (`ProjectMemberListCreateAPIEndpoint`, `member.py:94-156`)
//! * `GET+PATCH+DELETE .../members/<uuid:pk>/` + alias
//!   (`ProjectMemberDetailAPIEndpoint`, `member.py:160-222`)
//! * `GET+POST+GET+PATCH+DELETE workspaces/<slug>/invitations[/<pk>]/`
//!   (`WorkspaceInvitationsViewset`, `invite.py:24-154`, router in
//!   `api/urls/invite.py`)
//! * `GET users/me/` (`UserEndpoint`, `user.py:18-41`)
//!
//! Fixture: `rust-api/fixtures/v1_projects/handlers/collab.golden.json`
//! (FX-H-MEM; trace: `rust-api/fixtures/v1_projects/TRACE.md`).
//!
//! Shape of the port (translate, don't redesign):
//!
//! * Route registration is the cutover granularity (Porting guide cutover
//!   row, `app_issues::routes` precedent): owned methods serve Rust, every
//!   other method falls through to [`crate::edge::proxy`] so Django answers
//!   the 405s (`Method "PUT" not allowed`), OPTIONS metadata, redirects and
//!   resolver 404s exactly as before.
//! * Authentication is `APIKeyAuthentication`
//!   (`api/middleware/api_authentication.py`) — the `X-Api-Key` header only;
//!   there is no session auth on these routes. This module is the first Rust
//!   consumer of the [`pidash_auth::token`] kernel. Missing/empty header →
//!   401; unknown/revoked/expired/inactive token → 403 `Given API token is
//!   not valid` (pinned by contract `test_me_bad_token_403`).
//! * Order per request mirrors DRF `initial()`: authN (401/403), the
//!   slug→UUID rewrite (`base.py:51-98`, 404 `Project not found` on an
//!   unresolvable identifier — UUID-looking input passes through unchecked),
//!   then `check_permissions` (403 [`CLASS_DENIAL_BODY`]), then the body.
//!   Anonymous callers skip the rewrite so slugs cannot be probed via
//!   404-vs-401 (the `base.py:60-66` guard).
//! * Reads reuse `pidash_db::v1_projects::queries_projmem` wherever its
//!   columns suffice. Three shapes need columns that layer doesn't carry,
//!   so they are fetched here with the same JOIN/WHERE semantics (no fork —
//!   the predicates are quoted per statement): invite `created_at` /
//!   `updated_at` (the layer pins the 5-column serialize shape only),
//!   `users.avatar_asset_id` for `avatar_url`, and all writes.
//! * Writes mirror the model `save()` side effects: member POST sets
//!   `workspace_id` from the project (`ProjectBaseModel.save`,
//!   `db/models/project.py:309-311`) and creates the `ProjectUserProperty`
//!   row (`ProjectMember.save`, `:348-364`); audit columns follow
//!   `BaseModel.save` (`db/models/base.py:23-44`).
//!
//! Ported bugs and quirks (translate, don't redesign; also listed in the PR):
//!
//! * BUG-6 `WorkspaceOwnerPermission` has no `is_active` filter — inactive
//!   workspace admins pass the invite gate (honored via [`decide`]).
//! * BUG-7 `ProjectMemberPermission` SAFE is workspace-scoped, not
//!   project-scoped — any project membership in the workspace reads every
//!   project's member list (honored via [`decide`]).
//! * Re-adding a deactivated member 400s: member DELETE flips `is_active`
//!   but never sets `deleted_at`, so the partial-unique
//!   `(project, member) WHERE deleted_at IS NULL` still matches and the
//!   re-POST fails with `The payload is not valid`.
//! * Invite `token` is stored as `''`: the model field has no default
//!   generator and the view never sets one (Django `CharField.get_default`).
//! * Invite PATCH with a non-dict body 500s: `partial_update()` calls
//!   `request.data.get("email")` before the serializer runs, so the
//!   `AttributeError` escapes to `handle_exception`'s generic 500
//!   (`invite.py:113`).
//! * The `Provided workspace does not exist` 400 branches sit after the
//!   gates, whose slug-scoped `exists()` denies first — unreachable through
//!   the routes, ported in place.
//! * `HEAD` on an owned GET proxies explicitly where Django's
//!   `http_method_names` would 405 (axum would auto-serve it from `get`).
//!
//! Throttles (verified, not ported): no view here declares
//! `throttle_classes` — rate limiting is the shared `get_throttles`
//! infrastructure (PIDASHCONV-367 record).
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

use std::collections::HashMap;

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, patch, post};
use axum::{Json, Router};
use chrono_tz::Tz;
use serde_json::Value;
use sqlx::Row;

use crate::state::AppState;
use crate::uuid_path::{classify_uuid_segment, UuidSegment};
use pidash_auth::permissions::{project, workspace};
use pidash_auth::scope::TenantScope as AuthScope;
use pidash_auth::token as auth_token;
use pidash_db::v1_projects::queries_projmem::{self, TenantScope as QueryScope};
use pidash_types::{ProjectId, WorkspaceId};

use super::perms::{self, V1Route, CLASS_DENIAL_BODY};
pub use pidash_compat::response::SERVER_ERROR_BODY;

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------

/// Register the member / invite / user routes. Sibling D-19 handler issues
/// merge their routers into `super::routes()`; on rebase keep both sides.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/workspaces/{slug}/members/", owned(get(ws_members)))
        .route(
            "/api/v1/workspaces/{slug}/projects/{project_id}/members/",
            owned_get_post(get(pm_list), post(pm_create)),
        )
        .route(
            "/api/v1/workspaces/{slug}/projects/{project_id}/members/{pk}/",
            owned_detail(
                get(pm_detail_get),
                patch(pm_detail_patch),
                delete(pm_detail_delete),
            ),
        )
        .route(
            "/api/v1/workspaces/{slug}/projects/{project_id}/project-members/",
            owned_get_post(get(pm_list), post(pm_create)),
        )
        .route(
            "/api/v1/workspaces/{slug}/projects/{project_id}/project-members/{pk}/",
            owned_detail(
                get(pm_detail_get),
                patch(pm_detail_patch),
                delete(pm_detail_delete),
            ),
        )
        .route(
            "/api/v1/workspaces/{slug}/invitations/",
            owned_get_post(get(inv_list), post(inv_create)),
        )
        .route(
            "/api/v1/workspaces/{slug}/invitations/{pk}/",
            owned_detail(
                get(inv_detail_get),
                patch(inv_detail_patch),
                delete(inv_detail_delete),
            ),
        )
        .route("/api/v1/users/me/", owned(get(users_me)))
}

/// An owned GET-only path: reads serve Rust, everything else falls through
/// to Django (its 405s/OPTIONS live there), following the
/// `app_issues::owned` precedent.
fn owned(
    get_handler: axum::routing::MethodRouter<AppState>,
) -> axum::routing::MethodRouter<AppState> {
    get_handler
        .post(crate::edge::proxy)
        .put(crate::edge::proxy)
        .patch(crate::edge::proxy)
        .delete(crate::edge::proxy)
        .head(crate::edge::proxy)
        .options(crate::edge::proxy)
}

/// An owned GET+POST path (member / invite list+create).
fn owned_get_post(
    get_handler: axum::routing::MethodRouter<AppState>,
    post_handler: axum::routing::MethodRouter<AppState>,
) -> axum::routing::MethodRouter<AppState> {
    get_handler
        .merge(post_handler)
        .put(crate::edge::proxy)
        .patch(crate::edge::proxy)
        .delete(crate::edge::proxy)
        .head(crate::edge::proxy)
        .options(crate::edge::proxy)
}

/// An owned GET+PATCH+DELETE path (member / invite detail).
fn owned_detail(
    get_handler: axum::routing::MethodRouter<AppState>,
    patch_handler: axum::routing::MethodRouter<AppState>,
    delete_handler: axum::routing::MethodRouter<AppState>,
) -> axum::routing::MethodRouter<AppState> {
    get_handler
        .merge(patch_handler)
        .merge(delete_handler)
        .post(crate::edge::proxy)
        .put(crate::edge::proxy)
        .head(crate::edge::proxy)
        .options(crate::edge::proxy)
}

// ---------------------------------------------------------------------------
// Exact bodies
// ---------------------------------------------------------------------------

/// DRF `NotAuthenticated` (`IsAuthenticated` denial on every route here).
pub const UNAUTHENTICATED_BODY: &str =
    r#"{"detail":"Authentication credentials were not provided."}"#;
/// `APIKeyAuthentication` failure (invalid, revoked, expired, inactive).
pub const INVALID_TOKEN_BODY: &str = r#"{"detail":"Given API token is not valid"}"#;
/// `Project.resolve` miss (`Http404("Project not found")` via DRF).
pub const PROJECT_NOT_FOUND_BODY: &str = r#"{"detail":"Project not found"}"#;
/// `BaseAPIView.handle_exception` `ObjectDoesNotExist` (member detail).
pub const MEMBER_NOT_FOUND_BODY: &str = r#"{"error":"The requested resource does not exist."}"#;
/// `BaseViewSet.handle_exception` `ObjectDoesNotExist` (invites).
pub const INVITE_NOT_FOUND_BODY: &str = r#"{"error":"The required object does not exist."}"#;
/// `BaseViewSet.handle_exception` `ValidationError` (bad invite pk).
pub const INVALID_DETAIL_BODY: &str = r#"{"error":"Please provide valid detail"}"#;
/// `handle_exception` `IntegrityError` (both bases, same text).
pub const INVALID_PAYLOAD_BODY: &str = r#"{"error":"The payload is not valid"}"#;
/// The workspace-missing 400 (`views/member.py:77-80,128-132,182-186`).
pub const NO_WORKSPACE_BODY: &str = r#"{"error":"Provided workspace does not exist"}"#;

/// Role values (`pi_dash/app/permissions/base.py:13-16`, same numbers in
/// `pi_dash/utils/permissions`).
pub const ROLE_ADMIN: i32 = 20;
pub const ROLE_MEMBER: i32 = 15;
pub const ROLE_GUEST: i32 = 5;

// ---------------------------------------------------------------------------
// Failure type
// ---------------------------------------------------------------------------

/// Handler failure with its exact status + body.
#[derive(Debug)]
pub enum Denial {
    Unauthorized,
    InvalidToken,
    Forbidden,
    ProjectNotFound,
    MemberNotFound,
    InviteNotFound,
    BadDetail,
    BadError(String),
    BadJson(Value),
    BadPayload,
    UnsupportedMediaType(String),
    ParseDetail(String),
    ServerError,
}

impl Denial {
    fn status_and_body(&self) -> (StatusCode, Option<String>) {
        match self {
            Denial::Unauthorized => (StatusCode::UNAUTHORIZED, Some(UNAUTHENTICATED_BODY.into())),
            Denial::InvalidToken => (StatusCode::FORBIDDEN, Some(INVALID_TOKEN_BODY.into())),
            Denial::Forbidden => (StatusCode::FORBIDDEN, Some(CLASS_DENIAL_BODY.into())),
            Denial::ProjectNotFound => (StatusCode::NOT_FOUND, Some(PROJECT_NOT_FOUND_BODY.into())),
            Denial::MemberNotFound => (StatusCode::NOT_FOUND, Some(MEMBER_NOT_FOUND_BODY.into())),
            Denial::InviteNotFound => (StatusCode::NOT_FOUND, Some(INVITE_NOT_FOUND_BODY.into())),
            Denial::BadDetail => (StatusCode::BAD_REQUEST, Some(INVALID_DETAIL_BODY.into())),
            Denial::BadError(message) => (
                StatusCode::BAD_REQUEST,
                Some(format!("{{\"error\":{}}}", json_string(message))),
            ),
            Denial::BadJson(value) => (
                StatusCode::BAD_REQUEST,
                Some(serde_json::to_string(value).expect("json body")),
            ),
            Denial::BadPayload => (StatusCode::BAD_REQUEST, Some(INVALID_PAYLOAD_BODY.into())),
            Denial::UnsupportedMediaType(message) => (
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                Some(format!("{{\"detail\":{}}}", json_string(message))),
            ),
            Denial::ParseDetail(message) => (
                StatusCode::BAD_REQUEST,
                Some(format!("{{\"detail\":{}}}", json_string(message))),
            ),
            Denial::ServerError => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Some(SERVER_ERROR_BODY.into()),
            ),
        }
    }
}

impl IntoResponse for Denial {
    fn into_response(self) -> Response {
        let (status, body) = self.status_and_body();
        match body {
            Some(body) => Response::builder()
                .status(status)
                .header(header::CONTENT_TYPE, "application/json")
                .body(axum::body::Body::from(body))
                .expect("static denial response"),
            None => Response::builder()
                .status(status)
                .body(axum::body::Body::empty())
                .expect("empty denial response"),
        }
    }
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).expect("json string")
}

/// 200 JSON with DRF field order (serde_json `preserve_order`).
fn ok_json(value: Value) -> Response {
    (StatusCode::OK, Json(value)).into_response()
}

/// 201 JSON (member / invite create).
fn created_json(value: Value) -> Response {
    (StatusCode::CREATED, Json(value)).into_response()
}

// ---------------------------------------------------------------------------
// Authentication (`APIKeyAuthentication`)
// ---------------------------------------------------------------------------

/// The authenticated actor: user id plus the RAW stored time-zone name.
/// The zone is NOT parsed here — `TimezoneMixin.initial`
/// (`views/base.py:43-48`) calls `super().initial()` (auth +
/// permissions) first and only then activates the zone, so a denying
/// gate answers 403 even when the stored zone is unknown (which 400s
/// only for survivors, via [`activate_timezone`]).
#[derive(Debug, Clone)]
pub struct Actor {
    pub id: uuid::Uuid,
    pub timezone: Option<String>,
}

/// Authenticate one request from its `X-Api-Key` header.
///
/// * Missing/empty → `None` from `authenticate()` → 401 (every route here
///   requires `IsAuthenticated`, globally or via its class).
/// * `mt_` prefix → machine-token path (`validate_machine_token`); anything
///   else → `api_tokens` lookup (`validate_api_token`: exact match,
///   `is_active`, `expired_at` null or strictly future).
/// * Every failure → 403 `Given API token is not valid` — the kernel's
///   `TokenError` variants stay log-only and never reach the wire.
pub async fn authenticate(
    pool: &sqlx::PgPool,
    headers: &HeaderMap,
    secret_key: &[u8],
) -> Result<Actor, Denial> {
    let presented = headers
        .get(auth_token::API_KEY_HEADER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    // `if not token: return None` (`api_authentication.py`).
    let kind = auth_token::classify_token(presented).ok_or(Denial::Unauthorized)?;
    let id = match kind {
        auth_token::TokenKind::Api => authenticate_api_token(pool, presented).await?,
        auth_token::TokenKind::Machine => {
            authenticate_machine_token(pool, presented, secret_key).await?
        }
    };
    let timezone = load_timezone_name(pool, &id).await?;
    // The token loggers stamp this id on the `process_logs` payload
    // (Django reads `request.user` post-view; PIDASHCONV-937).
    crate::middleware::publish_logger_user_id(&id.to_string());
    Ok(Actor { id, timezone })
}

async fn authenticate_api_token(
    pool: &sqlx::PgPool,
    presented: &str,
) -> Result<uuid::Uuid, Denial> {
    // `deleted_at IS NULL` is the `SoftDeletionManager` scope
    // (`db/mixins.py:56-66`): `APIToken.objects` never sees soft-deleted
    // rows, so a soft-deleted token 403s instead of authenticating.
    let row: Option<(uuid::Uuid, bool, Option<chrono::DateTime<chrono::Utc>>)> =
        sqlx::query_as("SELECT user_id, is_active, expired_at FROM api_tokens WHERE token = $1 AND deleted_at IS NULL")
            .bind(presented)
            .fetch_optional(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    let Some((user_id, is_active, expired_at)) = row else {
        return Err(Denial::InvalidToken);
    };
    let now = chrono::Utc::now();
    let row = auth_token::ApiTokenRow {
        token: presented.to_owned(),
        is_active,
        expired_at_unix: expired_at.map(|dt| dt.timestamp()),
    };
    // The kernel compares the exact token bytes again (constant-time) and
    // the `expired_at__gt=now` predicate — strictly greater, null never
    // expires.
    auth_token::validate_api_token(Some(&row), presented, now.timestamp())
        .map_err(|_| Denial::InvalidToken)?;
    // `api_token.last_used = now; save(update_fields=["last_used"])`.
    pidash_services::domain::tokens::touch_api_token_last_used_by_token(pool, presented)
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(user_id)
}

async fn authenticate_machine_token(
    pool: &sqlx::PgPool,
    presented: &str,
    secret_key: &[u8],
) -> Result<uuid::Uuid, Denial> {
    let token_hash = auth_token::hash_token(presented, secret_key);
    let row: Option<MachineTokenLookup> = sqlx::query_as(
        r#"SELECT mt.id, mt.user_id, mt.workspace_id, mt.revoked_at,
                  mt.dev_machine_id, dm.revoked_at AS dev_revoked_at
           FROM machine_token mt
           LEFT JOIN dev_machine dm ON dm.id = mt.dev_machine_id
           WHERE mt.token_hash = $1"#,
    )
    .bind(&token_hash)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let Some(row) = row else {
        return Err(Denial::InvalidToken);
    };
    let kernel_row = auth_token::MachineTokenRow {
        token_hash: token_hash.clone(),
        revoked_at_unix: row.revoked_at.map(|dt| dt.timestamp()),
        dev_machine_revoked: row.dev_machine_id.is_some() && row.dev_revoked_at.is_some(),
    };
    // Hash match, token unrevoked, dev-machine unrevoked
    // (`dev_machine_id is not None and revoked_at is not None`).
    auth_token::validate_machine_token_static(Some(&kernel_row), &token_hash)
        .map_err(|_| Denial::InvalidToken)?;
    // `is_workspace_member(user, workspace_id)`: active row, any role.
    // A non-member is revoked first, then denied — exactly like Python.
    let member: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM workspace_members
          WHERE workspace_id = $1 AND member_id = $2 AND is_active
          AND deleted_at IS NULL)",
    )
    .bind(row.workspace_id)
    .bind(row.user_id)
    .fetch_one(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    if !member {
        pidash_services::domain::tokens::revoke_machine_token_by_id_now(pool, row.id)
            .await
            .map_err(|_| Denial::ServerError)?;
        return Err(Denial::InvalidToken);
    }
    pidash_services::domain::tokens::touch_machine_token_last_used_by_id_now(pool, row.id)
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(row.user_id)
}

#[derive(Debug, sqlx::FromRow)]
struct MachineTokenLookup {
    id: uuid::Uuid,
    user_id: uuid::Uuid,
    workspace_id: uuid::Uuid,
    revoked_at: Option<chrono::DateTime<chrono::Utc>>,
    dev_machine_id: Option<uuid::Uuid>,
    dev_revoked_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// `request.user.user_timezone` (`TimezoneMixin`): the stored zone name,
/// loaded but NOT parsed — parsing happens after the gate in
/// [`activate_timezone`].
async fn load_timezone_name(
    pool: &sqlx::PgPool,
    user_id: &uuid::Uuid,
) -> Result<Option<String>, Denial> {
    let zone: Option<(Option<String>,)> =
        sqlx::query_as("SELECT user_timezone FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_optional(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    Ok(zone.and_then(|row| row.0))
}

/// Activate the actor's rendering timezone (`TimezoneMixin.initial` runs
/// after `super().initial()`). A missing zone defaults to UTC; an unknown
/// zone name 400s: `zoneinfo.ZoneInfo` raises `ZoneInfoNotFoundError`,
/// which subclasses `KeyError`, so `handle_exception` answers the
/// `KeyError` branch (`api/views/base.py:160-164`). An EMPTY zone 500s:
/// `ZoneInfo('')` raises `ValueError` (not `KeyError`), which falls
/// through to the generic 500 (`api/views/base.py:166-171`).
fn activate_timezone(timezone: Option<&str>) -> Result<Tz, Denial> {
    match timezone {
        None => Ok(chrono_tz::UTC),
        Some("") => Err(Denial::ServerError),
        Some(zone) => zone
            .parse::<Tz>()
            .map_err(|_| Denial::BadError("The required key does not exist.".to_owned())),
    }
}

// ---------------------------------------------------------------------------
// Gates (permission classes via `super::perms` + the F-06 kernel)
// ---------------------------------------------------------------------------

/// Membership facts for the project-scoped gates, fetched exactly as the
/// guard classes filter (`app/permissions/project.py`, `workspace.py`).
struct Facts {
    workspace_id: Option<uuid::Uuid>,
    project_member_any_ws: bool,
    project_admin: bool,
    ws_admin_or_member: bool,
    ws_owner_unfiltered: bool,
}

/// Load every fact the collab gates read. `project_id` is `None` on the
/// workspace-members and invite routes (their classes carry no project
/// filter); the rewrite already guarantees `Some` on project routes.
async fn load_facts(
    pool: &sqlx::PgPool,
    slug: &str,
    actor: &Actor,
    project_id: Option<uuid::Uuid>,
) -> Result<Facts, Denial> {
    let workspace_id: Option<(uuid::Uuid,)> =
        sqlx::query_as("SELECT id FROM workspaces WHERE slug = $1 AND deleted_at IS NULL")
            .bind(slug)
            .fetch_optional(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    let workspace_id = workspace_id.map(|row| row.0);

    // BUG-7: `ProjectMemberPermission` SAFE has no `project_id` filter.
    let project_member_any_ws: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM project_members pm
           JOIN workspaces w ON w.id = pm.workspace_id
           WHERE w.slug = $1 AND pm.member_id = $2 AND pm.is_active
           AND pm.deleted_at IS NULL)",
    )
    .bind(slug)
    .bind(actor.id)
    .fetch_one(pool)
    .await
    .map_err(|_| Denial::ServerError)?;

    let project_admin: bool = match project_id {
        None => false,
        Some(pid) => sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM project_members pm
               JOIN workspaces w ON w.id = pm.workspace_id
               WHERE w.slug = $1 AND pm.member_id = $2 AND pm.project_id = $3
               AND pm.role = 20 AND pm.is_active AND pm.deleted_at IS NULL)",
        )
        .bind(slug)
        .bind(actor.id)
        .bind(pid)
        .fetch_one(pool)
        .await
        .map_err(|_| Denial::ServerError)?,
    };

    let ws_admin_or_member: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM workspace_members wm
           JOIN workspaces w ON w.id = wm.workspace_id
           WHERE w.slug = $1 AND wm.member_id = $2 AND wm.role IN (20, 15)
           AND wm.is_active AND wm.deleted_at IS NULL)",
    )
    .bind(slug)
    .bind(actor.id)
    .fetch_one(pool)
    .await
    .map_err(|_| Denial::ServerError)?;

    // BUG-6: `WorkspaceOwnerPermission` has no `is_active` filter.
    let ws_owner_unfiltered: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM workspace_members wm
           JOIN workspaces w ON w.id = wm.workspace_id
           WHERE w.slug = $1 AND wm.member_id = $2 AND wm.role = 20
           AND wm.deleted_at IS NULL)",
    )
    .bind(slug)
    .bind(actor.id)
    .fetch_one(pool)
    .await
    .map_err(|_| Denial::ServerError)?;

    Ok(Facts {
        workspace_id,
        project_member_any_ws,
        project_admin,
        ws_admin_or_member,
        ws_owner_unfiltered,
    })
}

/// Run the gate for a route+method: unknown workspace denies (the slug
/// filter matches nothing in Python), otherwise [`perms::decide`].
fn check_gate(
    route: V1Route,
    method: &str,
    facts: &Facts,
    project_id: Option<uuid::Uuid>,
) -> Result<(), Denial> {
    let gate = perms::gate_for(route, method);
    let Some(workspace_id) = facts.workspace_id else {
        return Err(Denial::Forbidden);
    };
    let scope = AuthScope::new(WorkspaceId::from(workspace_id.to_string()));
    let ws_id = WorkspaceId::from(workspace_id.to_string());
    let project_facts = project::ProjectFacts {
        workspace: ws_id.clone(),
        project_id: ProjectId::from(project_id.unwrap_or(workspace_id).to_string()),
        authenticated: true,
        is_workspace_member: facts.ws_admin_or_member || facts.ws_owner_unfiltered,
        has_workspace_admin_or_member: facts.ws_admin_or_member,
        is_workspace_admin: facts.ws_admin_or_member,
        // BUG-7: workspace-scoped, no `project_id` (see [`load_facts`]).
        is_project_member: facts.project_member_any_ws,
        is_project_admin: facts.project_admin,
        has_project_admin_or_member: facts.project_admin,
        has_identifier_membership: false,
        has_project_identifier: false,
    };
    let workspace_facts = workspace::WorkspaceFacts {
        workspace: ws_id,
        authenticated: true,
        has_admin_or_member_role: facts.ws_admin_or_member,
        has_admin_role: facts.ws_admin_or_member,
        is_member: facts.ws_admin_or_member || facts.ws_owner_unfiltered,
        // BUG-6: no `is_active` filter (see [`load_facts`]).
        is_admin_unfiltered: facts.ws_owner_unfiltered,
    };
    // State gates never fire on collab routes; the facts are inert.
    let mutation = project::StateMutationFacts {
        authenticated: true,
        project_role: None,
        members_can_edit_states: false,
        is_workspace_admin: false,
    };
    if perms::decide(
        gate,
        method,
        &scope,
        &project_facts,
        &workspace_facts,
        &mutation,
    ) {
        Ok(())
    } else {
        Err(Denial::Forbidden)
    }
}

// ---------------------------------------------------------------------------
// Shared request plumbing
// ---------------------------------------------------------------------------

/// The pool, or 500 when the binary runs without one (unit-test states).
fn pool(state: &AppState) -> Result<sqlx::PgPool, Denial> {
    state
        .pools()
        .map(|pools| pools.primary().clone())
        .ok_or(Denial::ServerError)
}

/// The owned request parts a gated handler hands back to Django when the
/// JSON body turns out to be unparseable (bundled so the four body handlers
/// stay under the argument-count lint).
struct ProxyRequest {
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
}

/// Decode negotiated JSON source text (`None` = unparseable).
///
/// Callers hand `None` back to Django via [`proxy_through`] for the exact
/// `ParseError` bytes (an unauthenticated caller with a bad body still sees
/// 401/403 first — parsing runs after the gate — `parsers.py:56-68`). Empty
/// bodies never reach here: negotiation maps them to `{}` (verified live —
/// the earlier `ParseError`-for-empty reading was wrong).
fn parse_body_json(body: &[u8]) -> Option<Value> {
    serde_json::from_slice(body).ok()
}

/// Member write-path body spec: no list fields; a blank form `role` behaves
/// as absent (the `ChoiceField` blank-input rule — not required, no
/// `allow_blank` — verified live). A blank form `member` stays in the map so
/// the `RelatedField` null rule fires downstream.
const MEMBER_BODY_SPEC: crate::v1_cycles_modules::body::BodySpec =
    crate::v1_cycles_modules::body::BodySpec {
        list_fields: &[],
        skip_blank_fields: &["role"],
    };

/// Invitation write-path body spec: same shape as the member spec (`role`
/// is the same optional choice; `email` keeps `''` so the `CharField` blank
/// rule fires — verified live).
const INVITE_BODY_SPEC: crate::v1_cycles_modules::body::BodySpec =
    crate::v1_cycles_modules::body::BodySpec {
        list_fields: &[],
        skip_blank_fields: &["role"],
    };

/// Content-negotiated member/invitation write body (PIDASHCONV-1391): empty
/// bodies are `{}`, form/multipart arrives as the HTML-input map (last value
/// wins, blank `role` skipped per `spec`), JSON keeps the existing decode
/// (`None` still proxies for Django's exact `ParseError` bytes), and a media
/// type no DRF parser accepts (or a missing `Content-Type` with a body) is
/// the 415. The flag reports HTML (form/multipart) input so member call sites
/// can apply the `RelatedField` blank rule.
///
/// Multipart *file* parts are dropped: no member/invitation field is a file
/// field, so a text-only multipart request is the whole named surface (same
/// convention as PIDASHCONV-1149).
fn parse_write_body(
    headers: &HeaderMap,
    body: &[u8],
    spec: &crate::v1_cycles_modules::body::BodySpec,
) -> Result<(Option<Value>, bool), Denial> {
    use crate::v1_cycles_modules::body::{negotiate_body, BodyError, NegotiatedBody};
    match negotiate_body(headers, body, spec) {
        Err(BodyError::UnsupportedMediaType(detail)) => Err(Denial::UnsupportedMediaType(detail)),
        Err(BodyError::ParseDetail(detail)) => Err(Denial::ParseDetail(detail)),
        Err(BodyError::ServerError) => Err(Denial::ServerError),
        Ok(NegotiatedBody::Empty) => Ok((Some(Value::Object(serde_json::Map::new())), false)),
        Ok(NegotiatedBody::JsonText { text, .. }) => Ok((parse_body_json(text.as_bytes()), false)),
        Ok(NegotiatedBody::Form { map, .. }) => Ok((Some(Value::Object(map)), true)),
    }
}

/// DRF `RelatedField.run_validation` forces a present-but-empty HTML value to
/// `None` before the null check (`relations.py:150-154`, probed POST +
/// PATCH), so form `member=` fails `may not be null` instead of the UUID
/// message. Applied to the form arm only: the JSON arm keeps the existing
/// path (which answers the UUID message — a pre-existing deviation, see PR
/// notes).
fn member_blank_to_null(data: &mut serde_json::Map<String, Value>) {
    if matches!(data.get("member"), Some(Value::String(s)) if s.is_empty()) {
        data.insert("member".to_owned(), Value::Null);
    }
}

/// `type(data).__name__` for the serializer non-dict message
/// (`serializers.py:485`): DRF names the runtime type, so `null` is
/// `NoneType`, `true` is `bool`, and `1`/`1.5` are `int`/`float`.
fn python_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "NoneType",
        Value::Bool(_) => "bool",
        Value::Number(n) if n.is_i64() || n.is_u64() => "int",
        Value::Number(_) => "float",
        Value::String(_) => "str",
        Value::Array(_) => "list",
        Value::Object(_) => "dict",
    }
}

/// The serializer non-dict 400 (`serializers.py:340,481-485`): any parsed
/// body that is not an object fails before field validation on every
/// serializer-driven handler, including PATCH. `null` never reaches the
/// dict check — `validate_empty_values` fails it with code `null`, which
/// the serializer `.errors` property rewrites (`serializers.py:574-582`).
fn non_dict_denial(value: &Value) -> Denial {
    if value.is_null() {
        return Denial::BadJson(serde_json::json!({
            "non_field_errors": ["No data provided"]
        }));
    }
    Denial::BadJson(serde_json::json!({
        "non_field_errors": [format!(
            "Invalid data. Expected a dictionary, but got {}.",
            python_type_name(value)
        )]
    }))
}

/// Python truthiness of a parsed JSON value, for guards written as
/// `request.data.get("email")` (`invite.py:113`): empty string, null,
/// false, zero and empty containers skip the guard exactly like `None`.
fn is_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(n) => {
            n.as_i64().is_some_and(|i| i != 0)
                || n.as_u64().is_some_and(|u| u != 0)
                || n.as_f64().is_some_and(|f| f != 0.0)
        }
        Value::String(s) => !s.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
    }
}

/// Rebuild a request from its parts and proxy it to Django (the resolver
/// 404 for a `pk` Django itself would never route to the view).
async fn proxy_through(
    state: AppState,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let mut req = axum::http::Request::builder()
        .method(method)
        .uri(uri)
        .body(axum::body::Body::from(body))
        .expect("rebuild proxy request");
    *req.headers_mut() = headers;
    crate::edge::proxy(State(state), req).await
}

/// Resolve `project_id` exactly like `_rewrite_project_kwarg`
/// (`base.py:51-98`): UUID-looking input passes through unchecked; other
/// input resolves via `Project.resolve` and 404s `Project not found` on a
/// miss. Anonymous callers never reach here (auth runs first).
async fn rewrite_project_id(
    pool: &sqlx::PgPool,
    slug: &str,
    actor_id: uuid::Uuid,
    raw: &str,
) -> Result<uuid::Uuid, Denial> {
    if let Ok(id) = raw.parse::<uuid::Uuid>() {
        return Ok(id);
    }
    let scope = QueryScope {
        workspace_slug: slug,
        actor_id,
    };
    queries_projmem::fetch_project_id(pool, &scope, raw)
        .await
        .map_err(|_| Denial::ServerError)?
        .ok_or(Denial::ProjectNotFound)
}

/// The workspace-missing 400 (`member.py:75-80,127-132,181-186`). Dead
/// behind the gates (their slug-scoped `exists()` denies first), ported in
/// the same position.
async fn require_workspace(pool: &sqlx::PgPool, slug: &str) -> Result<(), Denial> {
    let scope = QueryScope {
        workspace_slug: slug,
        actor_id: uuid::Uuid::nil(),
    };
    let known = queries_projmem::fetch_workspace_exists(pool, &scope)
        .await
        .map_err(|_| Denial::ServerError)?;
    if known {
        Ok(())
    } else {
        Err(Denial::BadError("Provided workspace does not exist".into()))
    }
}

// ---------------------------------------------------------------------------
// Renderers (`UserLiteSerializer`, membership + invite shapes)
// ---------------------------------------------------------------------------

/// One user row plus its derived `avatar_url`
/// (`serializers/user.py:13-38`, `db/models/user.py:142-151`).
/// `email` is nullable (`CharField(null=True)`, `db/models/user.py:61`);
/// `None` renders `null` like the serializer.
#[derive(Debug, Clone)]
pub struct UserLite {
    pub id: uuid::Uuid,
    pub first_name: String,
    pub last_name: String,
    pub email: Option<String>,
    pub avatar: String,
    pub avatar_asset_id: Option<uuid::Uuid>,
    pub display_name: String,
}

/// Render `UserLiteSerializer` in field order
/// (`Meta.fields`, `user.py:28-37` — the duplicated `email` emits once).
pub fn render_user_lite(user: &UserLite, avatar_url: Option<String>) -> Value {
    serde_json::json!({
        "id": user.id.to_string(),
        "first_name": user.first_name,
        "last_name": user.last_name,
        "email": user.email,
        "avatar": user.avatar,
        "avatar_url": avatar_url,
        "display_name": user.display_name,
    })
}

/// `avatar_url` property (`db/models/user.py:142-151`): the asset URL
/// as-is when `avatar_asset` is set (even when the asset type maps to no
/// URL — no fall-through to the avatar text), else the avatar text when
/// non-empty, else null.
pub fn avatar_url_for(user: &UserLite, assets: &HashMap<uuid::Uuid, String>) -> Option<String> {
    if let Some(asset_id) = user.avatar_asset_id {
        return assets.get(&asset_id).cloned();
    }
    if user.avatar.is_empty() {
        None
    } else {
        Some(user.avatar.clone())
    }
}

/// One `file_assets` row's `asset_url` (`db/models/asset.py:79-109`).
#[derive(Debug, Clone)]
pub struct AssetRef {
    pub id: uuid::Uuid,
    pub entity_type: Option<String>,
    pub workspace_slug: Option<String>,
    pub project_id: Option<uuid::Uuid>,
    pub issue_id: Option<uuid::Uuid>,
}

/// Render `FileAsset.asset_url` for one asset row.
pub fn render_asset_url(asset: &AssetRef) -> Option<String> {
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

/// Fetch `asset_url` values for a set of asset ids (one statement).
pub async fn fetch_asset_urls(
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

/// Render `ProjectMemberSerializer` (`serializers/member.py:40-43`).
/// The FK is nullable, so a null member renders `null`.
pub fn render_membership(id: uuid::Uuid, member_id: Option<uuid::Uuid>, role: i32) -> Value {
    serde_json::json!({
        "id": id.to_string(),
        "member": member_id.map(|mid| mid.to_string()),
        "role": role,
    })
}

/// One invite row with the datetimes the serialize shape carries
/// (`serializers/invite.py:23-31`). The query layer pins the 5-column shape;
/// the datetimes ride this handler-local statement with the same
/// JOIN/WHERE as Q6 (`views/invite.py:36-37`).
#[derive(Debug, Clone)]
pub struct InviteFull {
    pub id: uuid::Uuid,
    pub email: String,
    pub role: i32,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub responded_at: Option<chrono::DateTime<chrono::Utc>>,
    pub accepted: bool,
}

/// Render `WorkspaceInviteSerializer` in `Meta.fields` order
/// (`serializers/invite.py:23-31`), datetimes in the request zone.
pub fn render_invite(inv: &InviteFull, tz: &Tz) -> Value {
    serde_json::json!({
        "id": inv.id.to_string(),
        "email": inv.email,
        "role": inv.role,
        "created_at": crate::serializer::render_datetime_in(&inv.created_at, tz),
        "updated_at": crate::serializer::render_datetime_in(&inv.updated_at, tz),
        "responded_at": inv.responded_at.as_ref().map(|dt| crate::serializer::render_datetime_in(dt, tz)),
        "accepted": inv.accepted,
    })
}

fn map_invite_full(row: &sqlx::postgres::PgRow) -> Result<InviteFull, Denial> {
    Ok(InviteFull {
        id: row.try_get("id").map_err(|_| Denial::ServerError)?,
        email: row.try_get("email").map_err(|_| Denial::ServerError)?,
        role: row
            .try_get::<i16, _>("role")
            .map(i32::from)
            .map_err(|_| Denial::ServerError)?,
        created_at: row.try_get("created_at").map_err(|_| Denial::ServerError)?,
        updated_at: row.try_get("updated_at").map_err(|_| Denial::ServerError)?,
        responded_at: row
            .try_get("responded_at")
            .map_err(|_| Denial::ServerError)?,
        accepted: row.try_get("accepted").map_err(|_| Denial::ServerError)?,
    })
}

const INVITE_FULL_SELECT: &str =
    "SELECT i.id, i.email, i.role, i.created_at, i.updated_at, i.responded_at, i.accepted
FROM workspace_member_invites i
JOIN workspaces w ON w.id = i.workspace_id";

async fn fetch_invites_full(pool: &sqlx::PgPool, slug: &str) -> Result<Vec<InviteFull>, Denial> {
    // `WorkspaceMemberInvite.Meta.ordering = ("-created_at",)`
    // (`db/models/workspace.py:255`) — the fixture Q6 text omits it
    // (same defect family as its `deleted_at` predicate; flagged on
    // PIDASHCONV-373), but the live queryset carries it.
    let rows = sqlx::query(&format!(
        "{INVITE_FULL_SELECT}\nWHERE w.slug = $1 AND i.deleted_at IS NULL\nORDER BY i.created_at DESC"
    ))
    .bind(slug)
    .fetch_all(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    rows.iter().map(map_invite_full).collect()
}

async fn fetch_invite_full(
    pool: &sqlx::PgPool,
    slug: &str,
    pk: uuid::Uuid,
) -> Result<Option<InviteFull>, Denial> {
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(&format!(
        "{INVITE_FULL_SELECT}\nWHERE w.slug = $1 AND i.deleted_at IS NULL AND i.id = $2"
    ))
    .bind(slug)
    .bind(pk)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    row.map(|row| map_invite_full(&row)).transpose()
}

// ---------------------------------------------------------------------------
// Validation (serializer `validate_*`, byte-identical messages)
// ---------------------------------------------------------------------------

/// Collect field errors in serializer declaration order: the member
/// serializer declares `member` then `role`, the invite serializer `email`
/// then `role` — emitting `email`, `member`, `role` satisfies both, since
/// each route only ever fills its own serializer's keys.
#[derive(Debug, Default)]
pub struct FieldErrors {
    member: Vec<String>,
    role: Vec<String>,
    email: Vec<String>,
    non_field: Vec<String>,
}

impl FieldErrors {
    fn is_empty(&self) -> bool {
        self.member.is_empty()
            && self.role.is_empty()
            && self.email.is_empty()
            && self.non_field.is_empty()
    }

    /// Render `serializer.errors` (field order, compact).
    fn body(&self) -> Value {
        let mut map = serde_json::Map::new();
        if !self.email.is_empty() {
            map.insert("email".into(), Value::from(self.email.clone()));
        }
        if !self.member.is_empty() {
            map.insert("member".into(), Value::from(self.member.clone()));
        }
        if !self.role.is_empty() {
            map.insert("role".into(), Value::from(self.role.clone()));
        }
        if !self.non_field.is_empty() {
            map.insert(
                "non_field_errors".into(),
                Value::from(self.non_field.clone()),
            );
        }
        Value::Object(map)
    }
}

/// Validate one `member` UUID value (`serializers/member.py:20-33`):
/// `PrimaryKeyRelatedField(queryset=User.objects.all(), required=True)`
/// plus `validate_member` (workspace membership, no `is_active` filter).
async fn validate_member_value(
    pool: &sqlx::PgPool,
    slug: &str,
    value: &Value,
    errors: &mut FieldErrors,
) -> Option<uuid::Uuid> {
    let raw = match value {
        Value::Null => {
            errors.member.push("This field may not be null.".into());
            return None;
        }
        Value::String(raw) => raw.clone(),
        _ => {
            // `PrimaryKeyRelatedField.incorrect_type`: `type(data).__name__`.
            let kind = match value {
                Value::Number(n) if n.is_i64() || n.is_u64() => "int",
                Value::Number(_) => "float",
                Value::Bool(_) => "bool",
                Value::Array(_) => "list",
                _ => "dict",
            };
            errors.member.push(format!(
                "Incorrect type. Expected pk value, received {kind}."
            ));
            return None;
        }
    };
    let id: uuid::Uuid = match raw.parse() {
        Ok(id) => id,
        Err(_) => {
            errors
                .member
                .push(format!("\u{201c}{raw}\u{201d} is not a valid UUID."));
            return None;
        }
    };
    let known: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE id = $1)")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap_or(false);
    if !known {
        errors
            .member
            .push(format!("Invalid pk \"{id}\" - object does not exist."));
        return None;
    }
    let in_workspace: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM workspace_members wm
           JOIN workspaces w ON w.id = wm.workspace_id
           WHERE w.slug = $1 AND wm.member_id = $2 AND wm.deleted_at IS NULL)",
    )
    .bind(slug)
    .bind(id)
    .fetch_one(pool)
    .await
    .unwrap_or(false);
    if !in_workspace {
        errors.member.push("Member not found in workspace".into());
        return None;
    }
    Some(id)
}

/// Validate one `role` value (`serializers/member.py`, `invite.py`).
///
/// The model field carries `choices`, so DRF maps it to a `ChoiceField`:
/// unknown values fail there with `"X" is not a valid choice.` — the
/// `validate_role` methods (`Invalid role`) only ever see values already in
/// the accepted set, so their message is unreachable through the routes
/// (the FX-COLLAB-SER `role_unknown` golden records the dead message;
/// flagged on the domain gate PIDASHCONV-373). Numeric strings coerce
/// through the choice map (`"15"` → `15`).
fn validate_role_value(value: &Value, errors: &mut FieldErrors) -> Option<i32> {
    if *value == Value::Null {
        errors.role.push("This field may not be null.".into());
        return None;
    }
    // The choice display used in the message (`'"%s" is not a valid
    // choice.' % data`, Python `str()` rendering).
    let display: Option<String> = match value {
        Value::Number(n) => Some(n.to_string()),
        Value::String(s) => Some(s.clone()),
        Value::Bool(true) => Some("True".into()),
        Value::Bool(false) => Some("False".into()),
        _ => None,
    };
    let coerced: Option<i64> = match value {
        Value::Number(n) => n.as_i64(),
        Value::String(s) => s.parse::<i64>().ok(),
        _ => None,
    };
    match coerced {
        Some(role)
            if [
                i64::from(ROLE_ADMIN),
                i64::from(ROLE_MEMBER),
                i64::from(ROLE_GUEST),
            ]
            .contains(&role) =>
        {
            Some(role as i32)
        }
        _ => {
            let shown = display.unwrap_or_else(|| value.to_string());
            errors
                .role
                .push(format!("\"{shown}\" is not a valid choice."));
            None
        }
    }
}

/// Python `str.strip()` with no arguments: Unicode whitespace plus
/// `\x1c`-`\x1f`, which Rust's `char::is_whitespace` does not cover
/// (verified against CPython: `'\x1c'.isspace()` is true).
fn py_strip(value: &str) -> &str {
    value.trim_matches(|c: char| c.is_whitespace() || ('\x1c'..='\x1f').contains(&c))
}

/// Validate `email` through DRF's `CharField(max_length=255)` pipeline
/// (`rest_framework/fields.py`, DRF 3.15). The model field is a plain
/// `CharField` (`db/models/workspace.py:236`), so the format check is only
/// the serializer's custom `validate_email` (`serializers/invite.py:41-47`,
/// Django `validate_email` → `Invalid email address`); everything before it
/// is `CharField` machinery, in order:
///
/// * blank: `data == ''`, or whitespace-only after the trim
///   (`trim_whitespace=True`) → `This field may not be blank.`
/// * null → `This field may not be null.`
/// * bools, arrays, objects (anything but `str`/`int`/`float`) →
///   `Not a valid string.`; numerics coerce via `str()`.
/// * `MaxLengthValidator(255)` then `ProhibitNullCharactersValidator`,
///   in validator order; any failure skips the custom `validate_email`
///   (`run_validators` collects, `to_internal_value` raises before the
///   `validate_<field>` hook). `ProhibitSurrogateCharactersValidator`
///   cannot fire: neither JSON nor Rust strings carry a lone surrogate.
///
/// Returns the stripped string Django stores (`to_internal_value` output).
fn validate_email_value(value: &Value, errors: &mut FieldErrors) -> Option<String> {
    if let Value::String(raw) = value {
        if py_strip(raw).is_empty() {
            errors.email.push("This field may not be blank.".into());
            return None;
        }
    }
    let coerced: String = match value {
        Value::String(raw) => py_strip(raw).to_string(),
        Value::Null => {
            errors.email.push("This field may not be null.".into());
            return None;
        }
        Value::Bool(_) | Value::Array(_) | Value::Object(_) => {
            errors.email.push("Not a valid string.".into());
            return None;
        }
        // Python `str()` of an int matches exactly; a float repr never
        // contains `@` and is far below every length limit, so the exact
        // exponent rendering cannot change the message below.
        Value::Number(n) => n.to_string(),
    };
    if coerced.chars().count() > 255 {
        errors
            .email
            .push("Ensure this field has no more than 255 characters.".into());
    }
    if coerced.contains('\x00') {
        errors.email.push("Null characters are not allowed.".into());
    }
    if !errors.email.is_empty() {
        return None;
    }
    if is_valid_email(&coerced) {
        Some(coerced)
    } else {
        errors.email.push("Invalid email address".into());
        None
    }
}

/// Django's `EmailValidator` (`django/core/validators.py`, Django 4.2):
/// dot-atom or quoted-string user, ASCII domain labels, IP literals,
/// `localhost`, IDN via punycode. Every failure maps to the serializer's
/// `Invalid email address` at the call site. (The 320-char ceiling is dead
/// behind the field's `max_length=255` but kept, as written.)
pub fn is_valid_email(value: &str) -> bool {
    if value.is_empty() || !value.contains('@') || value.chars().count() > 320 {
        return false;
    }
    let (user, domain) = value.rsplit_once('@').expect("contains @");
    if !email_user_valid(user) {
        return false;
    }
    // `domain_allowlist = ["localhost"]`: exact match, case-sensitive.
    if domain == "localhost" {
        return true;
    }
    if email_domain_part_valid(domain) {
        return true;
    }
    // Possible IDN: `punycode()` (`django/utils/encoding.py`) re-checks the
    // ASCII form (`idna` 1.x is already a crate dependency).
    match idna::domain_to_ascii(domain) {
        Ok(ascii) => email_domain_part_valid(&ascii),
        Err(_) => false,
    }
}

/// The user part: dot-atom atoms, or the quoted-string alternative
/// (`user_regex`, case-insensitive).
fn email_user_valid(user: &str) -> bool {
    if user.starts_with('"') {
        return email_quoted_string_valid(user);
    }
    !user.is_empty()
        && user
            .split('.')
            .all(|atom| !atom.is_empty() && atom.bytes().all(email_atom_byte))
}

/// One dot-atom character (`[-!#$%&'*+/=?^_`{}|~0-9A-Z]`, case-insensitive).
fn email_atom_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || b"-!#$%&'*+/=?^_`{}|~".contains(&byte)
}

/// The quoted-string alternative, over bytes:
/// `^"([\001-\010\013\014\016-\037!#-\[\]-\177]|\\[\001-\011\013\014\016-\177])*"\Z`.
fn email_quoted_string_valid(user: &str) -> bool {
    let bytes = user.as_bytes();
    if bytes.len() < 2 || !user.starts_with('"') || !user.ends_with('"') {
        return false;
    }
    let mut inner = &bytes[1..bytes.len() - 1];
    while let Some((&first, rest)) = inner.split_first() {
        if first == b'\\' {
            let Some((&escaped, rest)) = rest.split_first() else {
                return false;
            };
            if !matches!(escaped, 0x01..=0x09 | 0x0b | 0x0c | 0x0e..=0x7f) {
                return false;
            }
            inner = rest;
        } else {
            if !matches!(
                first,
                0x01..=0x08 | 0x0b | 0x0c | 0x0e..=0x1f | 0x21 | 0x23..=0x5b | 0x5d..=0x7f
            ) {
                return false;
            }
            inner = rest;
        }
    }
    true
}

/// The domain part: the ASCII `domain_regex` or an IP literal
/// (`literal_regex` + `validate_ipv46_address`).
fn email_domain_part_valid(domain: &str) -> bool {
    if email_domain_valid(domain) {
        return true;
    }
    if domain.len() >= 2 && domain.starts_with('[') && domain.ends_with(']') {
        let inner = &domain[1..domain.len() - 1];
        if !inner.is_empty()
            && inner
                .bytes()
                .all(|b| b.is_ascii_hexdigit() || b == b':' || b == b'.')
            && inner.parse::<std::net::IpAddr>().is_ok()
        {
            return true;
        }
    }
    false
}

/// The ASCII domain alternative, `((?:label\.)+)(TLD)\Z`
/// (case-insensitive): at least one dotted label, every label 1-63 chars of
/// alnum/hyphen not starting or ending with `-`; the TLD is
/// `[A-Z0-9-]{2,63}` with only a trailing `-` excluded (`(?<!-)`), so a
/// leading-dash TLD such as `a@b.-cd` passes while `a@b.c` fails.
fn email_domain_valid(domain: &str) -> bool {
    let mut labels = domain.split('.');
    let tld = labels.next_back().unwrap_or("");
    if !(2..=63).contains(&tld.len()) || !tld.bytes().all(email_domain_byte) || tld.ends_with('-') {
        return false;
    }
    let mut any = false;
    for label in labels {
        any = true;
        if !(1..=63).contains(&label.len())
            || !label.bytes().all(email_domain_byte)
            || label.starts_with('-')
            || label.ends_with('-')
        {
            return false;
        }
    }
    any
}

/// One ASCII domain character (`[A-Z0-9-]`, case-insensitive).
fn email_domain_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'-'
}

// ---------------------------------------------------------------------------
// User fetching (full `UserLite` columns)
// ---------------------------------------------------------------------------

const USER_LITE_SELECT: &str =
    "SELECT u.id, u.first_name, u.last_name, u.email, u.avatar, u.avatar_asset_id, u.display_name
FROM users u";

fn map_user_lite(row: &sqlx::postgres::PgRow) -> Result<UserLite, Denial> {
    Ok(UserLite {
        id: row.try_get("id").map_err(|_| Denial::ServerError)?,
        first_name: row.try_get("first_name").map_err(|_| Denial::ServerError)?,
        last_name: row.try_get("last_name").map_err(|_| Denial::ServerError)?,
        // `email` is nullable (`db/models/user.py:61`); a non-opt decode
        // 500s the whole list on one NULL row.
        email: row
            .try_get::<Option<String>, _>("email")
            .map_err(|_| Denial::ServerError)?,
        avatar: row
            .try_get::<Option<String>, _>("avatar")
            .map_err(|_| Denial::ServerError)?
            .unwrap_or_default(),
        avatar_asset_id: row
            .try_get("avatar_asset_id")
            .map_err(|_| Denial::ServerError)?,
        display_name: row
            .try_get::<Option<String>, _>("display_name")
            .map_err(|_| Denial::ServerError)?
            .unwrap_or_default(),
    })
}

/// Full `UserLite` rows for an id list (the member views embed the user
/// shape, `member.py:87,140,191`).
///
/// No soft-delete scope: the live `users` table has no `deleted_at` column
/// and Django's `UserManager` adds no scope — unlike the FX-Q-PROJMEM
/// fixture text (fixture defect, flagged on the domain gate PIDASHCONV-373),
/// so the predicate is dropped here and the layer functions carrying it are
/// not used for user rows.
/// `User.Meta.ordering = ("-created_at",)` (`db/models/user.py:137`): the
/// `id__in` list query carries it, so callers that render the users
/// queryset (the project-member list) iterate this order; callers walking
/// another queryset (workspace memberships) re-key into a map.
async fn fetch_users_lite(
    pool: &sqlx::PgPool,
    ids: &[uuid::Uuid],
) -> Result<Vec<UserLite>, Denial> {
    if ids.is_empty() {
        // Django's `id__in=[]` empty-result short-circuit: no statement.
        return Ok(Vec::new());
    }
    let placeholders: Vec<String> = (1..=ids.len()).map(|i| format!("${i}")).collect();
    let sql = format!(
        "{USER_LITE_SELECT} WHERE u.id IN ({}) ORDER BY u.created_at DESC",
        placeholders.join(", ")
    );
    let mut query = sqlx::query(&sql);
    for id in ids {
        query = query.bind(*id);
    }
    let rows = query
        .fetch_all(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    rows.iter().map(map_user_lite).collect()
}

/// Key user rows by id (for callers walking another queryset's order).
fn users_by_id(users: Vec<UserLite>) -> HashMap<uuid::Uuid, UserLite> {
    users.into_iter().map(|user| (user.id, user)).collect()
}

/// One user by id (`User.objects.get`, `member.py:190`) — the plain
/// manager, no soft-delete scope (see [`fetch_users_lite`]).
async fn fetch_user_lite(pool: &sqlx::PgPool, id: uuid::Uuid) -> Result<Option<UserLite>, Denial> {
    let row: Option<sqlx::postgres::PgRow> =
        sqlx::query(&format!("{USER_LITE_SELECT} WHERE u.id = $1"))
            .bind(id)
            .fetch_optional(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    row.map(|row| map_user_lite(&row)).transpose()
}

// ---------------------------------------------------------------------------
// Handlers: workspace members
// ---------------------------------------------------------------------------

/// `GET workspaces/<slug>/members/` (`member.py:69-91`): `UserLite` + the
/// workspace `role` per entry, in queryset order.
async fn ws_members(
    State(state): State<AppState>,
    Path((slug,)): Path<(String,)>,
    headers: HeaderMap,
) -> Response {
    let result = ws_members_inner(&state, &slug, &headers).await;
    into_result_response(result)
}

async fn ws_members_inner(
    state: &AppState,
    slug: &str,
    headers: &HeaderMap,
) -> Result<Response, Denial> {
    let pool = pool(state)?;
    let actor = authenticate(&pool, headers, state.settings().secret_key.as_bytes()).await?;
    let facts = load_facts(&pool, slug, &actor, None).await?;
    check_gate(V1Route::WorkspaceMembers, "GET", &facts, None)?;
    // Gate passed: activate the stored zone now (`TimezoneMixin.initial`
    // runs after permissions; an unknown zone 400s only for survivors).
    activate_timezone(actor.timezone.as_deref())?;
    require_workspace(&pool, slug).await?;
    let Some(workspace_id) = facts.workspace_id else {
        return Err(Denial::ServerError);
    };
    // Memberships only — the layer's Q3 joins `users` with the same
    // non-existent `deleted_at` predicate (see [`fetch_users_lite`]), so the
    // ids+roles ride this statement and users are fetched separately.
    // `WorkspaceMember.Meta.ordering = ("-created_at",)`
    // (`db/models/workspace.py:227`) — the Python loop walks this order.
    let memberships: Vec<(uuid::Uuid, i16)> = sqlx::query_as(
        "SELECT wm.member_id, wm.role FROM workspace_members wm
         WHERE wm.workspace_id = $1 AND wm.deleted_at IS NULL
         ORDER BY wm.created_at DESC",
    )
    .bind(workspace_id)
    .fetch_all(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let ids: Vec<uuid::Uuid> = memberships.iter().map(|row| row.0).collect();
    let users = users_by_id(fetch_users_lite(&pool, &ids).await?);
    let asset_ids: Vec<uuid::Uuid> = users
        .values()
        .filter_map(|user| user.avatar_asset_id)
        .collect();
    let assets = fetch_asset_urls(&pool, &asset_ids).await?;
    // The Python loop emits one entry per workspace membership in queryset
    // order, skipping members whose user row is gone.
    let mut body = Vec::with_capacity(memberships.len());
    for (member_id, role) in &memberships {
        let Some(user) = users.get(member_id) else {
            continue;
        };
        let mut entry = render_user_lite(user, avatar_url_for(user, &assets));
        entry
            .as_object_mut()
            .expect("user object")
            .insert("role".into(), Value::from(i32::from(*role)));
        body.push(entry);
    }
    Ok(ok_json(Value::Array(body)))
}

// ---------------------------------------------------------------------------
// Handlers: project members
// ---------------------------------------------------------------------------

/// `GET .../members/` and the `project-members/` alias (`member.py:121-141`):
/// bare array of `UserLite` — no role key, unlike the workspace list.
async fn pm_list(
    State(state): State<AppState>,
    Path((slug, project_raw)): Path<(String, String)>,
    headers: HeaderMap,
) -> Response {
    into_result_response(pm_list_inner(&state, &slug, &project_raw, &headers).await)
}

async fn pm_list_inner(
    state: &AppState,
    slug: &str,
    project_raw: &str,
    headers: &HeaderMap,
) -> Result<Response, Denial> {
    let pool = pool(state)?;
    let actor = authenticate(&pool, headers, state.settings().secret_key.as_bytes()).await?;
    let project_id = rewrite_project_id(&pool, slug, actor.id, project_raw).await?;
    let facts = load_facts(&pool, slug, &actor, Some(project_id)).await?;
    check_gate(V1Route::ProjectMembers, "GET", &facts, Some(project_id))?;
    // Gate passed: activate the stored zone now (`TimezoneMixin.initial`
    // runs after permissions; an unknown zone 400s only for survivors).
    activate_timezone(actor.timezone.as_deref())?;
    require_workspace(&pool, slug).await?;
    let scope = QueryScope {
        workspace_slug: slug,
        actor_id: actor.id,
    };
    let ids = queries_projmem::fetch_project_member_ids(&pool, &scope, project_id)
        .await
        .map_err(|_| Denial::ServerError)?;
    // The rendered order is the *users* queryset order (`-created_at`),
    // not the ids order — iterate the fetch, not `ids`.
    let users = fetch_users_lite(&pool, &ids).await?;
    let asset_ids: Vec<uuid::Uuid> = users
        .iter()
        .filter_map(|user| user.avatar_asset_id)
        .collect();
    let assets = fetch_asset_urls(&pool, &asset_ids).await?;
    let body: Vec<Value> = users
        .iter()
        .map(|user| render_user_lite(user, avatar_url_for(user, &assets)))
        .collect();
    Ok(ok_json(Value::Array(body)))
}

/// `POST .../members/` (`member.py:152-156`): validate, save with the URL
/// `project_id`, answer the membership shape.
async fn pm_create(
    State(state): State<AppState>,
    Path((slug, project_raw)): Path<(String, String)>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    into_result_response(
        pm_create_inner(
            &state,
            &slug,
            &project_raw,
            ProxyRequest {
                method,
                uri,
                headers,
                body,
            },
        )
        .await,
    )
}

async fn pm_create_inner(
    state: &AppState,
    slug: &str,
    project_raw: &str,
    req: ProxyRequest,
) -> Result<Response, Denial> {
    let pool = pool(state)?;
    let actor = authenticate(&pool, &req.headers, state.settings().secret_key.as_bytes()).await?;
    let project_id = rewrite_project_id(&pool, slug, actor.id, project_raw).await?;
    let facts = load_facts(&pool, slug, &actor, Some(project_id)).await?;
    check_gate(V1Route::ProjectMembers, "POST", &facts, Some(project_id))?;
    // Gate passed: activate the stored zone now (`TimezoneMixin.initial`
    // runs after permissions; an unknown zone 400s only for survivors).
    activate_timezone(actor.timezone.as_deref())?;
    // The body parses after the gate (`post()`: the serializer is built
    // from `request.data`); unparseable JSON proxies for Django's exact
    // `ParseError` bytes, while empty bodies are `{}` and a media type no
    // DRF parser accepts is the 415 (PIDASHCONV-1391).
    let (maybe_json, from_form) = parse_write_body(&req.headers, &req.body, &MEMBER_BODY_SPEC)?;
    let Some(json) = maybe_json else {
        let ProxyRequest {
            method,
            uri,
            headers,
            body,
        } = req;
        return Ok(proxy_through(state.clone(), method, uri, headers, body).await);
    };

    let mut data = match json {
        Value::Object(map) => map,
        ref other => return Err(non_dict_denial(other)),
    };
    if from_form {
        member_blank_to_null(&mut data);
    }
    let mut errors = FieldErrors::default();
    let member_id = match data.get("member") {
        None => {
            errors.member.push("This field is required.".into());
            None
        }
        Some(value) => validate_member_value(&pool, slug, value, &mut errors).await,
    };
    // The model default (`role=5`) makes the field optional, exactly like
    // DRF's `required=False` inference.
    let role = match data.get("role") {
        None => Some(ROLE_GUEST),
        Some(value) => validate_role_value(value, &mut errors),
    };
    if !errors.is_empty() {
        return Err(Denial::BadJson(errors.body()));
    }
    let (member_id, role) = (member_id.expect("validated"), role.expect("validated"));

    // `ProjectBaseModel.save`: `workspace = project.workspace`.
    let workspace_id: Option<(uuid::Uuid,)> =
        sqlx::query_as("SELECT workspace_id FROM projects WHERE id = $1 AND deleted_at IS NULL")
            .bind(project_id)
            .fetch_optional(&pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    let Some((workspace_id,)) = workspace_id else {
        return Err(Denial::ServerError);
    };

    let mut tx = pool.begin().await.map_err(|_| Denial::ServerError)?;
    let props: Value =
        serde_json::from_str(&default_props_json()).map_err(|_| Denial::ServerError)?;
    let prefs: Value =
        serde_json::from_str(&default_preferences_json()).map_err(|_| Denial::ServerError)?;
    let membership_id = match pidash_services::domain::projects::members::add(
        &mut *tx,
        &pidash_services::domain::projects::members::NewProjectMember {
            workspace_id,
            project_id: Some(project_id),
            user_id: Some(member_id),
            role: role as i16,
            created_by: Some(actor.id),
            view_props: &props,
            default_props: &props,
            preferences: &prefs,
            now: chrono::Utc::now(),
            ignore_conflicts: false,
            db_now: true,
        },
    )
    .await
    {
        Ok(id) => id,
        Err(err) => {
            let _ = tx.rollback().await;
            return Err(integrity_denial(&err));
        }
    };
    // `ProjectMember.save` side effect: one `ProjectUserProperty` row with
    // the member's minimum workspace sort order minus 10000.
    if let Err(err) =
        create_project_user_property(&mut tx, workspace_id, project_id, member_id, actor.id).await
    {
        let _ = tx.rollback().await;
        return Err(err);
    }
    tx.commit().await.map_err(|_| Denial::ServerError)?;
    Ok(created_json(render_membership(
        membership_id,
        Some(member_id),
        role,
    )))
}

fn default_props_json() -> String {
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

fn default_preferences_json() -> String {
    serde_json::json!({
        "pages": {"block_display": true},
        "navigation": {"default_tab": "work_items", "hide_in_more_menu": []},
    })
    .to_string()
}

/// `ProjectMember.save` (`db/models/project.py:348-364`): create the
/// `ProjectUserProperty` row on add.
async fn create_project_user_property(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    workspace_id: uuid::Uuid,
    project_id: uuid::Uuid,
    member_id: uuid::Uuid,
    actor_id: uuid::Uuid,
) -> Result<(), Denial> {
    let min_sort: Option<f64> = sqlx::query_scalar(
        "SELECT MIN(sort_order) FROM project_user_properties
         WHERE workspace_id = $1 AND user_id = $2 AND deleted_at IS NULL",
    )
    .bind(workspace_id)
    .bind(member_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(|_| Denial::ServerError)?;
    let sort_order = min_sort.map(|min| min - 10000.0).unwrap_or(65535.0);
    let filters: Value =
        serde_json::from_str(&default_issue_filters_json()).map_err(|_| Denial::ServerError)?;
    let display_filters: Value = serde_json::from_str(&default_issue_display_filters_json())
        .map_err(|_| Denial::ServerError)?;
    let display_properties: Value = serde_json::from_str(&default_issue_display_properties_json())
        .map_err(|_| Denial::ServerError)?;
    let prefs: Value =
        serde_json::from_str(&default_preferences_json()).map_err(|_| Denial::ServerError)?;
    let insert = pidash_services::domain::projects::members::add_property_db_now(
        &mut **tx,
        &pidash_services::domain::projects::members::NewMemberProperty {
            workspace_id,
            project_id,
            user_id: member_id,
            filters: &filters,
            display_filters: &display_filters,
            display_properties: &display_properties,
            rich_filters: &serde_json::json!({}),
            preferences: &prefs,
            sort_order,
            created_by: Some(actor_id),
            now: chrono::Utc::now(),
        },
    )
    .await;
    if let Err(err) = insert {
        return Err(integrity_denial(&err));
    }
    Ok(())
}

fn default_issue_filters_json() -> String {
    serde_json::json!({
        "priority": null, "state": null, "state_group": null,
        "assignees": null, "created_by": null, "labels": null,
        "start_date": null, "target_date": null, "subscriber": null,
    })
    .to_string()
}

fn default_issue_display_filters_json() -> String {
    serde_json::json!({
        "group_by": null, "order_by": "-created_at", "type": null,
        "sub_issue": true, "show_empty_groups": true,
        "layout": "list", "calendar_date_range": "",
    })
    .to_string()
}

fn default_issue_display_properties_json() -> String {
    serde_json::json!({
        "assignee": true, "attachment_count": true, "created_on": true,
        "due_date": true, "estimate": true, "key": true, "labels": true,
        "link": true, "priority": true, "start_date": true, "state": true,
        "sub_issue_count": true, "updated_on": true,
    })
    .to_string()
}

/// `handle_exception` `IntegrityError` → 400 `The payload is not valid`
/// (both bases). Any database error on these inserts is an integrity
/// failure in Django terms (unique, FK race); anything else already
/// surfaced as `ServerError` before the write.
fn integrity_denial(err: &sqlx::Error) -> Denial {
    match err {
        sqlx::Error::Database(_) => Denial::BadPayload,
        _ => Denial::ServerError,
    }
}

/// `GET .../members/<pk>/` (`member.py:175-192`): the *user* profile, not
/// the membership row. Only the canonical spelling reaches the view in
/// Django (the `<uuid:pk>` converter rejects the rest), so proxy every
/// other spelling for the exact resolver 404.
async fn pm_detail_get(
    State(state): State<AppState>,
    Path((slug, project_raw, pk_raw)): Path<(String, String, String)>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    // Django routes this segment through the `<uuid:pk>` converter
    // (`api/urls/member.py:21,31`), whose regex is lowercase-hyphenated
    // canonical form only, matched case-sensitively — every other spelling
    // resolver-404s before auth. `Uuid::parse_str` also accepts uppercase,
    // simple, braced and `urn:uuid:` spellings, so gate on the raw segment
    // and proxy the rest for Django's own resolver 404 (PIDASHCONV-1095 /
    // -1132 / -1162 / -1193 / -1205 precedent, PIDASHCONV-1393).
    let pk = match classify_uuid_segment(&pk_raw) {
        UuidSegment::Canonical(pk) => pk,
        UuidSegment::NonCanonical | UuidSegment::Garbage => {
            return proxy_through(state, method, uri, headers, body).await;
        }
    };
    into_result_response(pm_detail_get_inner(&state, &slug, &project_raw, pk, &headers).await)
}

async fn pm_detail_get_inner(
    state: &AppState,
    slug: &str,
    project_raw: &str,
    pk: uuid::Uuid,
    headers: &HeaderMap,
) -> Result<Response, Denial> {
    let pool = pool(state)?;
    let actor = authenticate(&pool, headers, state.settings().secret_key.as_bytes()).await?;
    let project_id = rewrite_project_id(&pool, slug, actor.id, project_raw).await?;
    let facts = load_facts(&pool, slug, &actor, Some(project_id)).await?;
    check_gate(
        V1Route::ProjectMemberDetail,
        "GET",
        &facts,
        Some(project_id),
    )?;
    // Gate passed: activate the stored zone now (`TimezoneMixin.initial`
    // runs after permissions; an unknown zone 400s only for survivors).
    activate_timezone(actor.timezone.as_deref())?;
    require_workspace(&pool, slug).await?;
    let scope = QueryScope {
        workspace_slug: slug,
        actor_id: actor.id,
    };
    let membership = queries_projmem::fetch_project_member(&pool, &scope, project_id, pk)
        .await
        .map_err(|_| Denial::ServerError)?
        .ok_or(Denial::MemberNotFound)?;
    // `member.py:190`: `User.objects.get(id=...)` — a second 404 when the
    // user row is gone.
    let Some(member_id) = membership.member_id else {
        return Err(Denial::MemberNotFound);
    };
    let user = fetch_user_lite(&pool, member_id)
        .await?
        .ok_or(Denial::MemberNotFound)?;
    let assets =
        fetch_asset_urls(&pool, &user.avatar_asset_id.into_iter().collect::<Vec<_>>()).await?;
    Ok(ok_json(render_user_lite(
        &user,
        avatar_url_for(&user, &assets),
    )))
}

/// `PATCH .../members/<pk>/` (`member.py:203-208`): the *membership* shape.
/// No workspace-exists check in Python — ported as written.
async fn pm_detail_patch(
    State(state): State<AppState>,
    Path((slug, project_raw, pk_raw)): Path<(String, String, String)>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    // Same canonical-spelling gate as the GET handler: only a
    // lowercase-hyphenated segment matches Django's `<uuid:pk>` route;
    // everything else proxies for the resolver 404 (PIDASHCONV-1393).
    let pk = match classify_uuid_segment(&pk_raw) {
        UuidSegment::Canonical(pk) => pk,
        UuidSegment::NonCanonical | UuidSegment::Garbage => {
            return proxy_through(state, method, uri, headers, body).await;
        }
    };
    into_result_response(
        pm_detail_patch_inner(
            &state,
            &slug,
            &project_raw,
            pk,
            ProxyRequest {
                method,
                uri,
                headers,
                body,
            },
        )
        .await,
    )
}

async fn pm_detail_patch_inner(
    state: &AppState,
    slug: &str,
    project_raw: &str,
    pk: uuid::Uuid,
    req: ProxyRequest,
) -> Result<Response, Denial> {
    let pool = pool(state)?;
    let actor = authenticate(&pool, &req.headers, state.settings().secret_key.as_bytes()).await?;
    let project_id = rewrite_project_id(&pool, slug, actor.id, project_raw).await?;
    let facts = load_facts(&pool, slug, &actor, Some(project_id)).await?;
    check_gate(
        V1Route::ProjectMemberDetail,
        "PATCH",
        &facts,
        Some(project_id),
    )?;
    // Gate passed: activate the stored zone now (`TimezoneMixin.initial`
    // runs after permissions; an unknown zone 400s only for survivors).
    activate_timezone(actor.timezone.as_deref())?;
    let scope = QueryScope {
        workspace_slug: slug,
        actor_id: actor.id,
    };
    let membership = queries_projmem::fetch_project_member(&pool, &scope, project_id, pk)
        .await
        .map_err(|_| Denial::ServerError)?
        .ok_or(Denial::MemberNotFound)?;
    // The body parses after the row fetch (`patch()`: `objects.get` runs
    // before the serializer reads `request.data`); unparseable JSON proxies
    // for Django's exact `ParseError` bytes, while empty bodies are `{}` and
    // a media type no DRF parser accepts is the 415 (PIDASHCONV-1391).
    // Non-dict bodies 400 with no write.
    let (maybe_json, from_form) = parse_write_body(&req.headers, &req.body, &MEMBER_BODY_SPEC)?;
    let Some(json) = maybe_json else {
        let ProxyRequest {
            method,
            uri,
            headers,
            body,
        } = req;
        return Ok(proxy_through(state.clone(), method, uri, headers, body).await);
    };

    let mut data = match json {
        Value::Object(map) => map,
        ref other => return Err(non_dict_denial(other)),
    };
    if from_form {
        member_blank_to_null(&mut data);
    }
    let mut errors = FieldErrors::default();
    // Partial: absent keys keep their rows; present keys validate. A failed
    // present key leaves the old value in place, but the errors below stop
    // the write — exactly like DRF's `is_valid` gate before `save`.
    let mut member_id = membership.member_id;
    let mut role = membership_role(&pool, &membership.id).await?;
    if let Some(value) = data.get("member") {
        if let Some(valid) = validate_member_value(&pool, slug, value, &mut errors).await {
            member_id = Some(valid);
        }
    }
    if let Some(value) = data.get("role") {
        if let Some(valid) = validate_role_value(value, &mut errors) {
            role = Some(valid);
        }
    }
    if !errors.is_empty() {
        return Err(Denial::BadJson(errors.body()));
    }
    pidash_services::domain::projects::members::update_member_identity(
        &pool,
        membership.id,
        member_id,
        role,
        actor.id,
    )
    .await
    .map_err(|err| integrity_denial(&err))?;
    // `role` is non-nullable in the model; `member` may be null and renders
    // `null` (the `PrimaryKeyRelatedField` null shape).
    let role = role.ok_or(Denial::ServerError)?;
    Ok(ok_json(render_membership(membership.id, member_id, role)))
}

/// Current role of a membership row (the PATCH base when `role` is absent).
async fn membership_role(pool: &sqlx::PgPool, id: &uuid::Uuid) -> Result<Option<i32>, Denial> {
    let row: Option<(Option<i16>,)> =
        sqlx::query_as("SELECT role FROM project_members WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    Ok(row.and_then(|row| row.0).map(i32::from))
}

/// `DELETE .../members/<pk>/` (`member.py:218-222`): flip `is_active`, keep
/// the row — not a `deleted_at` soft-delete.
async fn pm_detail_delete(
    State(state): State<AppState>,
    Path((slug, project_raw, pk_raw)): Path<(String, String, String)>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    // Same canonical-spelling gate as the GET handler: only a
    // lowercase-hyphenated segment matches Django's `<uuid:pk>` route;
    // everything else proxies for the resolver 404 (PIDASHCONV-1393).
    let pk = match classify_uuid_segment(&pk_raw) {
        UuidSegment::Canonical(pk) => pk,
        UuidSegment::NonCanonical | UuidSegment::Garbage => {
            return proxy_through(state, method, uri, headers, body).await;
        }
    };
    into_result_response(pm_detail_delete_inner(&state, &slug, &project_raw, pk, &headers).await)
}

async fn pm_detail_delete_inner(
    state: &AppState,
    slug: &str,
    project_raw: &str,
    pk: uuid::Uuid,
    headers: &HeaderMap,
) -> Result<Response, Denial> {
    let pool = pool(state)?;
    let actor = authenticate(&pool, headers, state.settings().secret_key.as_bytes()).await?;
    let project_id = rewrite_project_id(&pool, slug, actor.id, project_raw).await?;
    let facts = load_facts(&pool, slug, &actor, Some(project_id)).await?;
    check_gate(
        V1Route::ProjectMemberDetail,
        "DELETE",
        &facts,
        Some(project_id),
    )?;
    // Gate passed: activate the stored zone now (`TimezoneMixin.initial`
    // runs after permissions; an unknown zone 400s only for survivors).
    activate_timezone(actor.timezone.as_deref())?;
    let scope = QueryScope {
        workspace_slug: slug,
        actor_id: actor.id,
    };
    let membership = queries_projmem::fetch_project_member(&pool, &scope, project_id, pk)
        .await
        .map_err(|_| Denial::ServerError)?
        .ok_or(Denial::MemberNotFound)?;
    pidash_services::domain::projects::members::deactivate_actor_first(
        &pool,
        membership.id,
        actor.id,
    )
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

fn into_result_response(result: Result<Response, Denial>) -> Response {
    match result {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

// ---------------------------------------------------------------------------
// Handlers: invites (`WorkspaceInvitationsViewset`, router prefix
// `workspaces/<slug>/invitations/`)
// ---------------------------------------------------------------------------

/// `GET invitations/` (`invite.py:55-58`): bare array, never paginated.
async fn inv_list(
    State(state): State<AppState>,
    Path((slug,)): Path<(String,)>,
    headers: HeaderMap,
) -> Response {
    into_result_response(inv_list_inner(&state, &slug, &headers).await)
}

async fn inv_list_inner(
    state: &AppState,
    slug: &str,
    headers: &HeaderMap,
) -> Result<Response, Denial> {
    let pool = pool(state)?;
    let actor = authenticate(&pool, headers, state.settings().secret_key.as_bytes()).await?;
    let facts = load_facts(&pool, slug, &actor, None).await?;
    check_gate(V1Route::Invites, "GET", &facts, None)?;
    // Gate passed: activate the stored zone now (`TimezoneMixin.initial`
    // runs after permissions; an unknown zone 400s only for survivors).
    let timezone = activate_timezone(actor.timezone.as_deref())?;
    let invites = fetch_invites_full(&pool, slug).await?;
    let body: Vec<Value> = invites
        .iter()
        .map(|inv| render_invite(inv, &timezone))
        .collect();
    Ok(ok_json(Value::Array(body)))
}

/// `POST invitations/` (`invite.py:89-94`).
async fn inv_create(
    State(state): State<AppState>,
    Path((slug,)): Path<(String,)>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    into_result_response(
        inv_create_inner(
            &state,
            &slug,
            ProxyRequest {
                method,
                uri,
                headers,
                body,
            },
        )
        .await,
    )
}

async fn inv_create_inner(
    state: &AppState,
    slug: &str,
    req: ProxyRequest,
) -> Result<Response, Denial> {
    let pool = pool(state)?;
    let actor = authenticate(&pool, &req.headers, state.settings().secret_key.as_bytes()).await?;
    let facts = load_facts(&pool, slug, &actor, None).await?;
    check_gate(V1Route::Invites, "POST", &facts, None)?;
    // Gate passed: activate the stored zone now (`TimezoneMixin.initial`
    // runs after permissions; an unknown zone 400s only for survivors).
    let timezone = activate_timezone(actor.timezone.as_deref())?;

    // `Workspace.objects.get(slug=slug)` — miss 404s through the viewset
    // exception handler (unreachable behind the gate, ported in place).
    let workspace_id: Option<(uuid::Uuid,)> =
        sqlx::query_as("SELECT id FROM workspaces WHERE slug = $1 AND deleted_at IS NULL")
            .bind(slug)
            .fetch_optional(&pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    let Some((workspace_id,)) = workspace_id else {
        return Err(Denial::InviteNotFound);
    };
    // The body parses after the workspace fetch (`create()`: `objects.get`
    // runs before the serializer reads `request.data`); unparseable JSON
    // proxies for Django's exact `ParseError` bytes, while empty bodies are
    // `{}` and a media type no DRF parser accepts is the 415 (PIDASHCONV-1391).
    let (maybe_json, _) = parse_write_body(&req.headers, &req.body, &INVITE_BODY_SPEC)?;
    let Some(json) = maybe_json else {
        let ProxyRequest {
            method,
            uri,
            headers,
            body,
        } = req;
        return Ok(proxy_through(state.clone(), method, uri, headers, body).await);
    };

    let data = match json {
        Value::Object(map) => map,
        ref other => return Err(non_dict_denial(other)),
    };
    let mut errors = FieldErrors::default();
    let email = match data.get("email") {
        None => {
            errors.email.push("This field is required.".into());
            None
        }
        // The `CharField` pipeline (blank/bool/max-length checks, Django
        // `EmailValidator`) lives in `validate_email_value`.
        Some(value) => validate_email_value(value, &mut errors),
    };
    let role = match data.get("role") {
        None => Some(ROLE_GUEST),
        Some(value) => validate_role_value(value, &mut errors),
    };
    // `validate()`: duplicate email in the workspace (`invite.py:53-60`).
    // DRF runs `to_internal_value` before `validate()`, so any field-level
    // failure means this check never runs — skip it when errors exist.
    if errors.is_empty() {
        if let Some(email) = email.as_deref() {
            let duplicate: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM workspace_member_invites i
                   JOIN workspaces w ON w.id = i.workspace_id
                   WHERE i.email = $1 AND w.slug = $2 AND i.deleted_at IS NULL)",
            )
            .bind(email)
            .bind(slug)
            .fetch_one(&pool)
            .await
            .map_err(|_| Denial::ServerError)?;
            if duplicate {
                errors.non_field.push("Email already invited".into());
            }
        }
    }
    if !errors.is_empty() {
        return Err(Denial::BadJson(errors.body()));
    }
    let (email, role) = (email.expect("validated"), role.expect("validated"));

    let invite_id = uuid::Uuid::new_v4();
    let insert = sqlx::query(
        "INSERT INTO workspace_member_invites (id, email, accepted, token, message,
            responded_at, role, workspace_id, created_by_id, updated_by_id,
            created_at, updated_at, deleted_at)
         VALUES ($1, $2, false, '', NULL,
            NULL, $3, $4, $5, NULL,
            now(), now(), NULL)",
    )
    .bind(invite_id)
    .bind(&email)
    .bind(role)
    .bind(workspace_id)
    .bind(actor.id)
    .execute(&pool)
    .await;
    if let Err(err) = insert {
        return Err(integrity_denial(&err));
    }
    let invite = fetch_invite_full(&pool, slug, invite_id)
        .await?
        .ok_or(Denial::ServerError)?;
    Ok(created_json(render_invite(&invite, &timezone)))
}

/// Parse the router `pk` (`DefaultRouter`, `[^/.]+`): anything matches, so a
/// non-UUID reaches the viewset and fails UUID coercion → the viewset
/// `ValidationError` branch, 400 `Please provide valid detail`.
/// (`invite.py:40`, `base.py:258-270`.)
fn parse_invite_pk(raw: &str) -> Result<uuid::Uuid, Denial> {
    raw.parse::<uuid::Uuid>().map_err(|_| Denial::BadDetail)
}

/// True when the detail segment is a DRF format-suffix spelling
/// (`{pk}.{format}`, `api/urls/invite.py` router): Django's pk regex is
/// `[^/.]+`, so a dotted segment never matches the detail route — Django
/// serves the format variant instead. Detail handlers proxy such requests
/// before auth for Django's exact bytes (PIDASHCONV-828); dotless input
/// (even garbage) stays in Rust and 400s in [`parse_invite_pk`].
fn is_format_suffix_pk(raw: &str) -> bool {
    raw.contains('.')
}

/// `GET invitations/<pk>/` (`invite.py:75-78`).
async fn inv_detail_get(
    State(state): State<AppState>,
    Path((slug, pk_raw)): Path<(String, String)>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
) -> Response {
    // Dotted segments are format-suffix spellings, never pks: proxy before
    // auth for Django's exact bytes (`invite.py` router `[^/.]+`).
    if is_format_suffix_pk(&pk_raw) {
        return proxy_through(state, method, uri, headers, Bytes::new()).await;
    }
    into_result_response(inv_detail_get_inner(&state, &slug, &pk_raw, &headers).await)
}

async fn inv_detail_get_inner(
    state: &AppState,
    slug: &str,
    pk_raw: &str,
    headers: &HeaderMap,
) -> Result<Response, Denial> {
    let pool = pool(state)?;
    let actor = authenticate(&pool, headers, state.settings().secret_key.as_bytes()).await?;
    let facts = load_facts(&pool, slug, &actor, None).await?;
    check_gate(V1Route::Invites, "GET", &facts, None)?;
    // Gate passed: activate the stored zone now (`TimezoneMixin.initial`
    // runs after permissions; an unknown zone 400s only for survivors).
    let timezone = activate_timezone(actor.timezone.as_deref())?;
    // The router pk parses after the gate (`initial()` authN/permissions
    // run before `get_object()`), so gated-out callers see 401/403 first.
    let pk = parse_invite_pk(pk_raw)?;
    let invite = fetch_invite_full(&pool, slug, pk)
        .await?
        .ok_or(Denial::InviteNotFound)?;
    Ok(ok_json(render_invite(&invite, &timezone)))
}

/// `PATCH invitations/<pk>/` (`invite.py:112-124`): any truthy `email`
/// value — even the unchanged address — 400s before validation.
async fn inv_detail_patch(
    State(state): State<AppState>,
    Path((slug, pk_raw)): Path<(String, String)>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    // Dotted segments are format-suffix spellings, never pks: proxy before
    // auth for Django's exact bytes (`invite.py` router `[^/.]+`). The
    // real body rides along — `partial_update()` reads `request.data`.
    if is_format_suffix_pk(&pk_raw) {
        return proxy_through(state, method, uri, headers, body).await;
    }
    into_result_response(
        inv_detail_patch_inner(
            &state,
            &slug,
            &pk_raw,
            ProxyRequest {
                method,
                uri,
                headers,
                body,
            },
        )
        .await,
    )
}

async fn inv_detail_patch_inner(
    state: &AppState,
    slug: &str,
    pk_raw: &str,
    req: ProxyRequest,
) -> Result<Response, Denial> {
    let pool = pool(state)?;
    let actor = authenticate(&pool, &req.headers, state.settings().secret_key.as_bytes()).await?;
    let facts = load_facts(&pool, slug, &actor, None).await?;
    check_gate(V1Route::Invites, "PATCH", &facts, None)?;
    // Gate passed: activate the stored zone now (`TimezoneMixin.initial`
    // runs after permissions; an unknown zone 400s only for survivors).
    let timezone = activate_timezone(actor.timezone.as_deref())?;
    let pk = parse_invite_pk(pk_raw)?;
    let invite = fetch_invite_full(&pool, slug, pk)
        .await?
        .ok_or(Denial::InviteNotFound)?;
    // The body parses after the row fetch (`partial_update()`: `get_object`
    // runs before `request.data`); unparseable JSON proxies for Django's
    // exact `ParseError` bytes, while empty bodies are `{}` and a media type
    // no DRF parser accepts is the 415 (PIDASHCONV-1391). Non-dict bodies
    // 500 with no write (the email guard reads `.get` before validation).
    let (maybe_json, _) = parse_write_body(&req.headers, &req.body, &INVITE_BODY_SPEC)?;
    let Some(json) = maybe_json else {
        let ProxyRequest {
            method,
            uri,
            headers,
            body,
        } = req;
        return Ok(proxy_through(state.clone(), method, uri, headers, body).await);
    };

    let data = match json {
        Value::Object(map) => map,
        // `partial_update()` calls `request.data.get("email")` before the
        // serializer ever runs, so any non-dict body raises `AttributeError`
        // and `handle_exception` answers 500 (`invite.py:113`).
        _ => return Err(Denial::ServerError),
    };
    // `request.data.get("email")` — truthiness, so `""`/`null` skip the
    // guard and fail field validation downstream instead.
    if data.get("email").is_some_and(is_truthy) {
        return Err(Denial::BadJson(serde_json::json!({
            "error": "Email cannot be updated after invite is created.",
            "code": "EMAIL_CANNOT_BE_UPDATED",
        })));
    }
    let mut errors = FieldErrors::default();
    // Present-but-falsy values skip the guard but still validate: partial
    // serializers validate every present key.
    if let Some(value) = data.get("email") {
        validate_email_value(value, &mut errors);
    }
    let mut role = invite.role;
    if let Some(value) = data.get("role") {
        if let Some(valid) = validate_role_value(value, &mut errors) {
            role = valid;
        }
    }
    if !errors.is_empty() {
        return Err(Denial::BadJson(errors.body()));
    }
    sqlx::query(
        "UPDATE workspace_member_invites SET role = $1,
            updated_by_id = $2, updated_at = now()
         WHERE id = $3",
    )
    .bind(role)
    .bind(actor.id)
    .bind(invite.id)
    .execute(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let invite = fetch_invite_full(&pool, slug, pk)
        .await?
        .ok_or(Denial::ServerError)?;
    Ok(ok_json(render_invite(&invite, &timezone)))
}

/// `DELETE invitations/<pk>/` (`invite.py:141-154`): accepted and
/// responded guards, then the instance soft-delete (`deleted_at` +
/// `updated_*`, plus the fan-out enqueue).
async fn inv_detail_delete(
    State(state): State<AppState>,
    Path((slug, pk_raw)): Path<(String, String)>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
) -> Response {
    // Dotted segments are format-suffix spellings, never pks: proxy before
    // auth for Django's exact bytes (`invite.py` router `[^/.]+`).
    if is_format_suffix_pk(&pk_raw) {
        return proxy_through(state, method, uri, headers, Bytes::new()).await;
    }
    into_result_response(inv_detail_delete_inner(&state, &slug, &pk_raw, &headers).await)
}

async fn inv_detail_delete_inner(
    state: &AppState,
    slug: &str,
    pk_raw: &str,
    headers: &HeaderMap,
) -> Result<Response, Denial> {
    let pool = pool(state)?;
    let actor = authenticate(&pool, headers, state.settings().secret_key.as_bytes()).await?;
    let facts = load_facts(&pool, slug, &actor, None).await?;
    check_gate(V1Route::Invites, "DELETE", &facts, None)?;
    // Gate passed: activate the stored zone now (`TimezoneMixin.initial`
    // runs after permissions; an unknown zone 400s only for survivors).
    activate_timezone(actor.timezone.as_deref())?;
    let pk = parse_invite_pk(pk_raw)?;
    let invite = fetch_invite_full(&pool, slug, pk)
        .await?
        .ok_or(Denial::InviteNotFound)?;
    if invite.accepted {
        return Err(Denial::BadJson(serde_json::json!({
            "error": "Invite already accepted",
            "code": "INVITE_ALREADY_ACCEPTED",
        })));
    }
    if invite.responded_at.is_some() {
        return Err(Denial::BadJson(serde_json::json!({
            "error": "Invite already responded",
            "code": "INVITE_ALREADY_RESPONDED",
        })));
    }
    // Instance `delete()` → `save()`: `deleted_at` + `updated_*`
    // (`mixins.py:72-77`, `base.py:23-44`), then the unconditional
    // `soft_delete_related_objects.delay(...)` enqueue (`:78`).
    let now = chrono::Utc::now();
    sqlx::query(
        "UPDATE workspace_member_invites SET deleted_at = $1, updated_at = $1, updated_by_id = $2 WHERE id = $3",
    )
    .bind(now)
    .bind(actor.id)
    .bind(invite.id)
    .execute(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    super::handlers_project::enqueue_soft_delete(&pool, "workspacememberinvite", &invite.id).await;
    Ok(StatusCode::NO_CONTENT.into_response())
}

// ---------------------------------------------------------------------------
// Handlers: current user
// ---------------------------------------------------------------------------

/// `GET users/me/` (`user.py:34-41`): no `permission_classes`, so the base
/// `IsAuthenticated` is the only gate — any valid token passes.
async fn users_me(State(state): State<AppState>, headers: HeaderMap) -> Response {
    into_result_response(users_me_inner(&state, &headers).await)
}

async fn users_me_inner(state: &AppState, headers: &HeaderMap) -> Result<Response, Denial> {
    let pool = pool(state)?;
    let actor = authenticate(&pool, headers, state.settings().secret_key.as_bytes()).await?;
    // No view gate beyond `IsAuthenticated`, but `TimezoneMixin.initial`
    // still activates the zone before the body: an unknown stored zone
    // 400s here too.
    activate_timezone(actor.timezone.as_deref())?;
    // `request.user` straight from authentication — no manager scope, so a
    // soft-deleted row still renders (a missing row entirely 500s, as the
    // FK fetch would in Python).
    let row: Option<sqlx::postgres::PgRow> =
        sqlx::query(&format!("{USER_LITE_SELECT} WHERE u.id = $1"))
            .bind(actor.id)
            .fetch_optional(&pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    let Some(row) = row else {
        return Err(Denial::ServerError);
    };
    let user = map_user_lite(&row)?;
    let assets =
        fetch_asset_urls(&pool, &user.avatar_asset_id.into_iter().collect::<Vec<_>>()).await?;
    Ok(ok_json(render_user_lite(
        &user,
        avatar_url_for(&user, &assets),
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activate_timezone_empty_zone_500s() {
        // `ZoneInfo('')` raises `ValueError` (not `KeyError`), so an
        // empty stored zone is the generic 500 while an unknown zone is
        // the `KeyError`-branch 400 (PIDASHCONV-747, live-probed).
        assert!(matches!(
            activate_timezone(Some("")),
            Err(Denial::ServerError)
        ));
        assert!(matches!(
            activate_timezone(Some("Not/AZone")),
            Err(Denial::BadError(_))
        ));
        assert_eq!(activate_timezone(None).expect("none"), chrono_tz::UTC);
        assert_eq!(activate_timezone(Some("UTC")).expect("utc"), chrono_tz::UTC);
    }

    #[test]
    fn format_suffix_pk_is_dotted_only() {
        assert!(is_format_suffix_pk(
            "8dbd5acc-b3ce-4259-91b2-570768a0b918.json"
        ));
        assert!(is_format_suffix_pk("x.x"));
        assert!(!is_format_suffix_pk("8dbd5acc-b3ce-4259-91b2-570768a0b918"));
        assert!(!is_format_suffix_pk("x"));
        assert!(!is_format_suffix_pk(""));
    }

    #[tokio::test]
    async fn dotted_invite_pk_proxies_before_auth() {
        use tower::ServiceExt;
        // Port 1 is never bound, so a proxied request fail-closed 502s
        // while a handled one answers from here (500, no pool): 502 on an
        // anonymous dotted pk proves the proxy runs before auth
        // (PIDASHCONV-828).
        let app = crate::routes::with_routes(
            AppState::with_edge(
                "0.1.0",
                crate::edge::EdgeHandle::for_tests("http://127.0.0.1:1"),
            ),
            routes(),
        );
        for (method, uri) in [
            ("GET", "/api/v1/workspaces/acme/invitations/x.json/"),
            ("PATCH", "/api/v1/workspaces/acme/invitations/x.json/"),
            ("DELETE", "/api/v1/workspaces/acme/invitations/x.json/"),
        ] {
            let request = axum::http::Request::builder()
                .method(method)
                .uri(uri)
                .body(axum::body::Body::empty())
                .expect("request");
            let status = app.clone().oneshot(request).await.expect("serve").status();
            assert_eq!(status, StatusCode::BAD_GATEWAY, "{method} {uri}");
        }
    }

    fn lite(id: &str) -> UserLite {
        UserLite {
            id: id.parse().expect("uuid"),
            first_name: "Ct".into(),
            last_name: "User".into(),
            email: Some("ct@example.com".into()),
            avatar: String::new(),
            avatar_asset_id: None,
            display_name: "Ct User".into(),
        }
    }

    #[test]
    fn user_lite_key_order_and_null_avatar_url() {
        let rendered = render_user_lite(&lite("11111111-1111-1111-1111-111111111111"), None);
        let keys: Vec<&str> = rendered
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            vec![
                "id",
                "first_name",
                "last_name",
                "email",
                "avatar",
                "avatar_url",
                "display_name",
            ]
        );
        assert_eq!(rendered["avatar_url"], Value::Null);
    }

    #[test]
    fn user_lite_null_email_renders_null() {
        let mut user = lite("11111111-1111-1111-1111-111111111111");
        user.email = None;
        let rendered = render_user_lite(&user, None);
        assert_eq!(rendered["email"], Value::Null);
        let keys: Vec<&str> = rendered
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            vec![
                "id",
                "first_name",
                "last_name",
                "email",
                "avatar",
                "avatar_url",
                "display_name",
            ]
        );
    }

    #[test]
    fn avatar_url_prefers_asset_then_avatar_then_null() {
        let mut user = lite("11111111-1111-1111-1111-111111111111");
        let assets = HashMap::new();
        assert_eq!(avatar_url_for(&user, &assets), None);
        user.avatar = "https://example.com/a.png".into();
        assert_eq!(
            avatar_url_for(&user, &assets),
            Some("https://example.com/a.png".into())
        );
        let asset_id: uuid::Uuid = "22222222-2222-2222-2222-222222222222"
            .parse()
            .expect("uuid");
        user.avatar_asset_id = Some(asset_id);
        let mut assets = HashMap::new();
        assets.insert(
            asset_id,
            "/api/assets/v2/static/22222222-2222-2222-2222-222222222222/".into(),
        );
        assert_eq!(
            avatar_url_for(&user, &assets),
            Some("/api/assets/v2/static/22222222-2222-2222-2222-222222222222/".into())
        );
    }

    #[test]
    fn avatar_url_no_fallthrough_when_asset_unresolvable() {
        // `db/models/user.py:145-146`: the asset arm returns as-is, even
        // when the asset type maps to no URL — non-empty avatar text must
        // not leak into `avatar_url`.
        let mut user = lite("11111111-1111-1111-1111-111111111111");
        user.avatar = "https://example.com/a.png".into();
        user.avatar_asset_id = Some(
            "22222222-2222-2222-2222-222222222222"
                .parse()
                .expect("uuid"),
        );
        assert_eq!(avatar_url_for(&user, &HashMap::new()), None);
    }

    #[test]
    fn asset_url_branches() {
        let base = AssetRef {
            id: "33333333-3333-3333-3333-333333333333"
                .parse()
                .expect("uuid"),
            entity_type: Some("USER_AVATAR".into()),
            workspace_slug: None,
            project_id: None,
            issue_id: None,
        };
        assert_eq!(
            render_asset_url(&base),
            Some("/api/assets/v2/static/33333333-3333-3333-3333-333333333333/".into())
        );
        let unknown = AssetRef {
            entity_type: Some("BANNER".into()),
            ..base.clone()
        };
        assert_eq!(render_asset_url(&unknown), None);
        let bare = AssetRef {
            entity_type: None,
            ..base
        };
        assert_eq!(render_asset_url(&bare), None);
    }

    #[test]
    fn membership_shape_key_order() {
        let rendered = render_membership(
            "11111111-1111-1111-1111-111111111111"
                .parse()
                .expect("uuid"),
            Some(
                "22222222-2222-2222-2222-222222222222"
                    .parse()
                    .expect("uuid"),
            ),
            ROLE_MEMBER,
        );
        let keys: Vec<&str> = rendered
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(keys, vec!["id", "member", "role"]);
        let null_member = render_membership(
            "11111111-1111-1111-1111-111111111111"
                .parse()
                .expect("uuid"),
            None,
            ROLE_MEMBER,
        );
        assert_eq!(null_member["member"], Value::Null);
    }

    #[test]
    fn invite_shape_key_order() {
        let inv = InviteFull {
            id: "11111111-1111-1111-1111-111111111111"
                .parse()
                .expect("uuid"),
            email: "ct@example.com".into(),
            role: ROLE_MEMBER,
            created_at: chrono::DateTime::from_timestamp(1700000000, 123456789).expect("ts"),
            updated_at: chrono::DateTime::from_timestamp(1700000000, 0).expect("ts"),
            responded_at: None,
            accepted: false,
        };
        let rendered = render_invite(&inv, &chrono_tz::UTC);
        let keys: Vec<&str> = rendered
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            vec![
                "id",
                "email",
                "role",
                "created_at",
                "updated_at",
                "responded_at",
                "accepted",
            ]
        );
        assert_eq!(rendered["responded_at"], Value::Null);
        // DRF `iso-8601` rendering through the kernel: sub-micro nanos
        // truncate to micros, `Z` (PIDASHCONV-867).
        assert_eq!(rendered["created_at"], "2023-11-14T22:13:20.123456Z");
        assert_eq!(rendered["updated_at"], "2023-11-14T22:13:20Z");
    }

    #[test]
    fn email_validation() {
        // Django `EmailValidator` fidelity (`django/core/validators.py`).
        assert!(is_valid_email("ct-new@example.com"));
        assert!(is_valid_email("a@b.12"));
        assert!(is_valid_email("a@localhost"));
        assert!(is_valid_email("a@[1.2.3.4]"));
        assert!(is_valid_email("a@[::1]"));
        assert!(is_valid_email("a@xn--bcher-kva.example"));
        assert!(is_valid_email("a@bücher.de"));
        assert!(is_valid_email("\"ab\"@example.com"));
        assert!(is_valid_email("Test@Example.COM"));
        // The TLD lookbehind `(?<!-)` excludes only a trailing dash.
        assert!(is_valid_email("a@b.-cd"));
        assert!(is_valid_email(&format!("{}@example.com", "a".repeat(65))));
        assert!(!is_valid_email("not-an-email"));
        assert!(!is_valid_email("a@b"));
        assert!(!is_valid_email("a@b.c"));
        assert!(!is_valid_email("a@b.c-"));
        assert!(!is_valid_email("@example.com"));
        assert!(!is_valid_email("a@1.2.3"));
        assert!(!is_valid_email("a..b@example.com"));
        assert!(!is_valid_email(".a@example.com"));
        assert!(!is_valid_email("a.@example.com"));
        assert!(!is_valid_email("a@LOCALHOST"));
        assert!(!is_valid_email("\"a b\"@example.com"));
        assert!(!is_valid_email("a@[999.1.1.1]"));
        assert!(!is_valid_email("  a@b.com  "));
    }

    #[test]
    fn email_field_pipeline_messages() {
        // DRF `CharField(max_length=255)` order: blank, null, invalid-type,
        // max-length, null-chars, then the Django email check.
        let check = |value: serde_json::Value| {
            let mut errors = FieldErrors::default();
            let valid = validate_email_value(&value, &mut errors);
            (valid, errors.body())
        };
        assert_eq!(
            check(serde_json::json!("  ct-new@example.com  ")),
            (Some("ct-new@example.com".into()), serde_json::json!({}))
        );
        assert_eq!(
            check(serde_json::json!("")),
            (
                None,
                serde_json::json!({"email": ["This field may not be blank."]})
            )
        );
        assert_eq!(
            check(serde_json::json!("  \u{1c} ")),
            (
                None,
                serde_json::json!({"email": ["This field may not be blank."]})
            )
        );
        assert_eq!(
            check(serde_json::Value::Null),
            (
                None,
                serde_json::json!({"email": ["This field may not be null."]})
            )
        );
        assert_eq!(
            check(serde_json::json!(true)),
            (None, serde_json::json!({"email": ["Not a valid string."]}))
        );
        assert_eq!(
            check(serde_json::json!(["a@b.com"])),
            (None, serde_json::json!({"email": ["Not a valid string."]}))
        );
        assert_eq!(
            check(serde_json::json!(7)),
            (
                None,
                serde_json::json!({"email": ["Invalid email address"]})
            )
        );
        let long = format!("{}@example.com", "a".repeat(250));
        assert_eq!(
            check(serde_json::json!(long)),
            (
                None,
                serde_json::json!({"email": ["Ensure this field has no more than 255 characters."]})
            )
        );
        assert_eq!(
            check(serde_json::json!("a\x00@example.com")),
            (
                None,
                serde_json::json!({"email": ["Null characters are not allowed."]})
            )
        );
        assert_eq!(
            check(serde_json::json!("plain")),
            (
                None,
                serde_json::json!({"email": ["Invalid email address"]})
            )
        );
    }

    #[test]
    fn non_dict_type_names() {
        // `type(data).__name__` (`serializers.py:485`).
        let body = |value: serde_json::Value| match non_dict_denial(&value) {
            Denial::BadJson(body) => body,
            _ => unreachable!(),
        };
        for (value, name) in [
            (serde_json::json!([1]), "list"),
            (serde_json::json!("x"), "str"),
            (serde_json::json!(1), "int"),
            (serde_json::json!(1.5), "float"),
            (serde_json::json!(true), "bool"),
        ] {
            assert_eq!(
                body(value),
                serde_json::json!({
                    "non_field_errors": [format!("Invalid data. Expected a dictionary, but got {name}.")]
                })
            );
        }
        // `null` is rewritten by the serializer `.errors` property
        // (`serializers.py:574-582`), never reaching the dict check.
        assert_eq!(
            body(serde_json::Value::Null),
            serde_json::json!({"non_field_errors": ["No data provided"]})
        );
    }

    #[test]
    fn field_error_key_order() {
        // Invite declaration order (`email`, then `role`), e.g. a PATCH
        // carrying a bad email and a bad role together.
        let mut errors = FieldErrors::default();
        errors.role.push("\"99\" is not a valid choice.".into());
        errors.email.push("This field may not be blank.".into());
        let rendered = errors.body();
        let keys: Vec<&str> = rendered
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(keys, vec!["email", "role"]);
    }

    #[test]
    fn email_guard_truthiness() {
        // `request.data.get("email")` (`invite.py:113`): only truthy values
        // trip the guard.
        assert!(is_truthy(&serde_json::json!("a@b.com")));
        assert!(is_truthy(&serde_json::json!("  ")));
        assert!(is_truthy(&serde_json::json!(1)));
        assert!(is_truthy(&serde_json::json!({"a": 1})));
        assert!(!is_truthy(&serde_json::json!("")));
        assert!(!is_truthy(&serde_json::Value::Null));
        assert!(!is_truthy(&serde_json::json!(false)));
        assert!(!is_truthy(&serde_json::json!(0)));
        assert!(!is_truthy(&serde_json::json!(0.0)));
        assert!(!is_truthy(&serde_json::json!([])));
        assert!(!is_truthy(&serde_json::json!({})));
    }

    #[test]
    fn role_validation_messages() {
        let mut errors = FieldErrors::default();
        assert_eq!(
            validate_role_value(&serde_json::json!(ROLE_MEMBER), &mut errors),
            Some(ROLE_MEMBER)
        );
        assert!(errors.is_empty());
        assert_eq!(
            validate_role_value(&serde_json::json!(99), &mut errors),
            None
        );
        assert_eq!(
            errors.body(),
            serde_json::json!({"role": ["\"99\" is not a valid choice."]})
        );
        let mut errors = FieldErrors::default();
        assert_eq!(
            validate_role_value(&serde_json::json!("15"), &mut errors),
            Some(ROLE_MEMBER)
        );
        assert!(errors.is_empty());
        let mut errors = FieldErrors::default();
        assert_eq!(
            validate_role_value(&serde_json::json!("x"), &mut errors),
            None
        );
        assert_eq!(
            errors.body(),
            serde_json::json!({"role": ["\"x\" is not a valid choice."]})
        );
        let mut errors = FieldErrors::default();
        assert_eq!(
            validate_role_value(&serde_json::json!(true), &mut errors),
            None
        );
        assert_eq!(
            errors.body(),
            serde_json::json!({"role": ["\"True\" is not a valid choice."]})
        );
    }

    #[test]
    fn gate_routing_matches_views() {
        // `get_permissions` (`member.py:98-101`): GET reads, everything else
        // administers — HEAD included, exactly as written.
        assert_eq!(
            perms::gate_for(V1Route::ProjectMembers, "GET"),
            perms::V1ProjectsGate::ProjectMember
        );
        assert_eq!(
            perms::gate_for(V1Route::ProjectMembers, "POST"),
            perms::V1ProjectsGate::ProjectAdmin
        );
        assert_eq!(
            perms::gate_for(V1Route::ProjectMemberDetail, "HEAD"),
            perms::V1ProjectsGate::ProjectAdmin
        );
        assert_eq!(
            perms::gate_for(V1Route::Invites, "DELETE"),
            perms::V1ProjectsGate::WorkspaceOwner
        );
        assert_eq!(
            perms::gate_for(V1Route::UserMe, "GET"),
            perms::V1ProjectsGate::AuthOnly
        );
    }

    #[test]
    fn denial_bodies_are_byte_exact() {
        let body = |d: Denial| d.status_and_body().1.expect("body");
        assert_eq!(
            body(Denial::Unauthorized),
            r#"{"detail":"Authentication credentials were not provided."}"#
        );
        assert_eq!(
            body(Denial::InvalidToken),
            r#"{"detail":"Given API token is not valid"}"#
        );
        assert_eq!(
            body(Denial::Forbidden),
            r#"{"detail":"You do not have permission to perform this action."}"#
        );
        assert_eq!(
            body(Denial::ProjectNotFound),
            r#"{"detail":"Project not found"}"#
        );
        assert_eq!(
            body(Denial::MemberNotFound),
            r#"{"error":"The requested resource does not exist."}"#
        );
        assert_eq!(
            body(Denial::InviteNotFound),
            r#"{"error":"The required object does not exist."}"#
        );
        assert_eq!(
            body(Denial::BadJson(serde_json::json!({
                "error": "Email cannot be updated after invite is created.",
                "code": "EMAIL_CANNOT_BE_UPDATED",
            }))),
            r#"{"error":"Email cannot be updated after invite is created.","code":"EMAIL_CANNOT_BE_UPDATED"}"#
        );
    }

    #[test]
    fn invite_pk_parsing() {
        assert!(parse_invite_pk("11111111-1111-1111-1111-111111111111").is_ok());
        assert!(matches!(parse_invite_pk("abc"), Err(Denial::BadDetail)));
    }

    #[test]
    fn write_body_negotiates() {
        let headers_for = |content_type: &str, len: usize| {
            let mut headers = HeaderMap::new();
            headers.insert("content-type", content_type.parse().unwrap());
            headers.insert("content-length", len.to_string().parse().unwrap());
            headers
        };
        // Form input: last value wins, blank `role` behaves as absent.
        let body = b"member=ae5c9c1e-7b6e-4c7e-bf2c-9d1e5f6a7b8c&role=";
        let headers = headers_for("application/x-www-form-urlencoded", body.len());
        let (json, from_form) = parse_write_body(&headers, body, &MEMBER_BODY_SPEC).expect("form");
        assert!(from_form);
        let map = json.expect("map").as_object().cloned().expect("object");
        assert_eq!(
            map.get("member"),
            Some(&Value::String(
                "ae5c9c1e-7b6e-4c7e-bf2c-9d1e5f6a7b8c".to_owned()
            ))
        );
        assert!(!map.contains_key("role"));
        // A blank form `member` fails like null downstream.
        let mut blank = serde_json::Map::new();
        blank.insert("member".to_owned(), Value::String(String::new()));
        member_blank_to_null(&mut blank);
        assert_eq!(blank.get("member"), Some(&Value::Null));
        // The JSON arm keeps the existing decode.
        let json_body = br#"{"role": 5}"#;
        let headers = headers_for("application/json", json_body.len());
        let (json, from_form) =
            parse_write_body(&headers, json_body, &MEMBER_BODY_SPEC).expect("json");
        assert!(!from_form);
        assert_eq!(json, Some(serde_json::json!({"role": 5})));
        // Unknown media types 415 with the DRF detail.
        let text_body = br#"{"role": 5}"#;
        let headers = headers_for("text/plain", text_body.len());
        let err = parse_write_body(&headers, text_body, &MEMBER_BODY_SPEC).expect_err("415");
        let (status, body) = err.status_and_body();
        assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
        assert_eq!(
            body.as_deref(),
            Some(r#"{"detail":"Unsupported media type \"text/plain\" in request."}"#)
        );
        assert!(matches!(err, Denial::UnsupportedMediaType(_)));
        // Empty bodies are `{}`.
        let (json, from_form) =
            parse_write_body(&HeaderMap::new(), b"", &MEMBER_BODY_SPEC).expect("empty");
        assert!(!from_form);
        assert_eq!(json, Some(Value::Object(serde_json::Map::new())));
    }
}

/// Live scratch-DB round trips (env-gated, the `queries_projmem` precedent):
/// without `DATABASE_URL` (plain `cargo test`) these skip; the runbook sets
/// `export DATABASE_URL=postgresql://…/pidash_371_test` for the real check.
/// Every test seeds its own workspace + users (unique slugs), so parallel
/// tests never share rows and no teardown is needed.
#[cfg(test)]
mod live_tests {
    use super::*;
    use axum::http::Request;
    use tower::ServiceExt;

    const ADMIN: i32 = 20;
    const MEMBER: i32 = 15;

    struct Seed {
        slug: String,
        project_id: String,
        owner_id: String,
        member_id: String,
        guest_id: String,
        outsider_id: String,
        owner_key: String,
        guest_key: String,
        outsider_key: String,
    }

    async fn live_pool() -> Option<sqlx::PgPool> {
        match std::env::var("DATABASE_URL") {
            Ok(url) => Some(
                sqlx::postgres::PgPoolOptions::new()
                    .max_connections(5)
                    .connect(&url)
                    .await
                    .expect("connect to scratch DATABASE_URL"),
            ),
            Err(_) => {
                eprintln!("skipping live-db test: DATABASE_URL is not set");
                None
            }
        }
    }

    async fn live_app(pool: &sqlx::PgPool) -> Option<Router> {
        let url = std::env::var("DATABASE_URL").ok()?;
        let cfg = pidash_db::config::DbConfig::new(url, 5).expect("db config");
        let pools = pidash_db::Pools::connect(&cfg, None)
            .await
            .expect("connect pools");
        let state = AppState::new("test").with_pools(pools);
        let _ = pool;
        Some(Router::new().merge(routes()).with_state(state))
    }

    async fn call(
        app: Router,
        method: &str,
        path: &str,
        key: Option<&str>,
        json: Option<Value>,
    ) -> (StatusCode, Vec<u8>) {
        let body = json.map(|v| v.to_string()).unwrap_or_default();
        let mut builder = Request::builder().method(method).uri(path);
        if let Some(key) = key {
            builder = builder.header("X-Api-Key", key);
        }
        if !body.is_empty() {
            builder = builder.header("content-type", "application/json");
            // Hyper sets this on the wire; negotiation keys emptiness off it
            // exactly like Django keys off CONTENT_LENGTH.
            builder = builder.header("content-length", body.len().to_string());
        }
        let req = builder.body(axum::body::Body::from(body)).expect("request");
        let response = app.oneshot(req).await.expect("serve");
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 8 * 1024 * 1024)
            .await
            .expect("body");
        (status, bytes.to_vec())
    }

    fn json_of(bytes: &[u8]) -> Value {
        serde_json::from_slice(bytes).expect("json body")
    }

    fn uid(raw: &str) -> uuid::Uuid {
        raw.parse().expect("uuid")
    }

    async fn seed_collab(pool: &sqlx::PgPool) -> Seed {
        let tag = uuid::Uuid::new_v4().simple().to_string();
        let short = &tag[..8];
        let owner_id = uuid::Uuid::new_v4().to_string();
        let member_id = uuid::Uuid::new_v4().to_string();
        let guest_id = uuid::Uuid::new_v4().to_string();
        let outsider_id = uuid::Uuid::new_v4().to_string();
        for (user_id, first) in [
            (&owner_id, "CtOwner"),
            (&member_id, "CtMember"),
            (&guest_id, "CtGuest"),
            (&outsider_id, "CtOutsider"),
        ] {
            let email = format!("ct-{short}-{first}@example.com");
            // Column list mirrors the contract harness `create_user`.
            sqlx::query(
                "INSERT INTO users (id, password, username, email, first_name, last_name,
                    avatar, date_joined, created_at, updated_at, last_location,
                    created_location, is_superuser, is_managed, is_password_expired,
                    is_active, is_staff, is_email_verified, is_password_autoset, token,
                    user_timezone, last_login_ip, last_logout_ip, last_login_medium,
                    last_login_uagent, is_bot, display_name, is_email_valid,
                    is_password_reset_required)
                 VALUES ($1, '!', $2, $2, $3, 'User',
                    '', now(), now(), now(), '',
                    '', false, false, false,
                    true, false, true, false, '',
                    'UTC', '', '', '',
                    '', false, $4, true,
                    false)",
            )
            .bind(uid(user_id))
            .bind(&email)
            .bind(first)
            .bind(format!("{first} User"))
            .execute(pool)
            .await
            .expect("seed user");
        }
        let ws_id = uuid::Uuid::new_v4().to_string();
        let slug = format!("ct-{short}");
        sqlx::query(
            "INSERT INTO workspaces (id, name, slug, owner_id, created_by_id,
                updated_by_id, timezone, background_color, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $4, $4, 'UTC', '', now(), now())",
        )
        .bind(uid(&ws_id))
        .bind(format!("CT {short}"))
        .bind(&slug)
        .bind(uid(&owner_id))
        .execute(pool)
        .await
        .expect("seed workspace");
        for (user, role) in [
            (&owner_id, ADMIN),
            (&member_id, MEMBER),
            (&guest_id, MEMBER),
        ] {
            // Column list mirrors the contract harness `add_workspace_member`.
            sqlx::query(
                "INSERT INTO workspace_members (id, workspace_id, member_id, role,
                    is_active, view_props, default_props, issue_props,
                    explored_features, getting_started_checklist, tips,
                    created_at, updated_at)
                 VALUES ($1, $2, $3, $4,
                    true, '{}', '{}', '{}',
                    '{}', '{}', '{}',
                    now(), now())",
            )
            .bind(uuid::Uuid::new_v4())
            .bind(uid(&ws_id))
            .bind(uid(user))
            .bind(role)
            .execute(pool)
            .await
            .expect("seed workspace member");
        }
        let project_id = uuid::Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO projects (id, name, description, network, identifier,
                workspace_id, created_by_id, cycle_view, module_view,
                issue_views_view, page_view, intake_view, archive_in, close_in,
                logo_props, is_time_tracking_enabled, is_issue_type_enabled,
                guest_view_all_features, timezone, members_can_edit_states,
                repo_url, base_branch, agent_default_interval_seconds,
                agent_default_max_ticks, agent_ticking_enabled, is_default,
                agent_review_default_interval_seconds, default_agent_executor,
                agent_test_default_interval_seconds, created_at, updated_at)
             VALUES ($1, $2, '', 0, $3,
                $4, $5, true, true,
                true, true, false, 0, 0,
                '{}', true, true,
                false, 'UTC', false,
                '', '', 0,
                0, false, false,
                0, '', 0, now(), now())",
        )
        .bind(uid(&project_id))
        .bind(format!("CT Project {short}"))
        .bind(format!("CT{short}").to_uppercase())
        .bind(uid(&ws_id))
        .bind(uid(&owner_id))
        .execute(pool)
        .await
        .expect("seed project");
        for user in [&owner_id, &member_id] {
            sqlx::query(
                "INSERT INTO project_members (id, workspace_id, project_id, member_id,
                    role, is_active, view_props, default_props, preferences,
                    sort_order, created_at, updated_at)
                 VALUES ($1, $2, $3, $4, $5, true, '{}', '{}', '{}', 0, now(), now())",
            )
            .bind(uuid::Uuid::new_v4())
            .bind(uid(&ws_id))
            .bind(uid(&project_id))
            .bind(uid(user))
            .bind(ADMIN)
            .execute(pool)
            .await
            .expect("seed project member");
        }
        let owner_key = format!("ct-live-{short}-o");
        let guest_key = format!("ct-live-{short}-g");
        let outsider_key = format!("ct-live-{short}-x");
        for (user, token) in [
            (&owner_id, &owner_key),
            (&guest_id, &guest_key),
            (&outsider_id, &outsider_key),
        ] {
            sqlx::query(
                "INSERT INTO api_tokens (id, token, label, user_type, user_id,
                    description, is_active, is_service, allowed_rate_limit,
                    created_at, updated_at)
                 VALUES ($1, $2, $3, 0, $4, '', true, false, '', now(), now())",
            )
            .bind(uuid::Uuid::new_v4())
            .bind(token)
            .bind(format!("ct-{short}"))
            .bind(uid(user))
            .execute(pool)
            .await
            .expect("seed token");
        }
        Seed {
            slug,
            project_id,
            owner_id,
            member_id,
            guest_id,
            outsider_id,
            owner_key,
            guest_key,
            outsider_key,
        }
    }

    fn user_keys() -> std::collections::HashSet<String> {
        [
            "id",
            "first_name",
            "last_name",
            "email",
            "avatar",
            "avatar_url",
            "display_name",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect()
    }

    #[tokio::test]
    async fn live_workspace_members_list() {
        let Some(pool) = live_pool().await else {
            return;
        };
        let Some(app) = live_app(&pool).await else {
            return;
        };
        let seed = seed_collab(&pool).await;
        let path = format!("/api/v1/workspaces/{}/members/", seed.slug);
        let (status, bytes) = call(app.clone(), "GET", &path, Some(&seed.owner_key), None).await;
        assert_eq!(status, StatusCode::OK);
        let body = json_of(&bytes);
        let by_id: HashMap<String, Value> = body
            .as_array()
            .expect("array")
            .iter()
            .map(|u| (u["id"].as_str().expect("id").to_owned(), u.clone()))
            .collect();
        assert!(by_id.contains_key(&seed.owner_id));
        assert!(by_id.contains_key(&seed.member_id));
        assert!(!by_id.contains_key(&seed.outsider_id));
        let owner = &by_id[&seed.owner_id];
        let keys: std::collections::HashSet<String> =
            owner.as_object().expect("object").keys().cloned().collect();
        let mut expected = user_keys();
        expected.insert("role".into());
        assert_eq!(keys, expected);
        assert_eq!(owner["role"], ADMIN);
        // Auth edges: anon 401, bogus token 403 with the exact body.
        let (status, _) = call(app.clone(), "GET", &path, None, None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        let (status, bytes) = call(app.clone(), "GET", &path, Some("ct-bogus"), None).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(
            json_of(&bytes),
            serde_json::json!({"detail": "Given API token is not valid"})
        );
        // Outsider is authenticated but has no membership: 403 denial body.
        let (status, bytes) = call(app, "GET", &path, Some(&seed.outsider_key), None).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(
            json_of(&bytes),
            serde_json::json!({"detail": "You do not have permission to perform this action."})
        );
    }

    #[tokio::test]
    async fn live_project_members_list_aliases_identical() {
        let Some(pool) = live_pool().await else {
            return;
        };
        let Some(app) = live_app(&pool).await else {
            return;
        };
        let seed = seed_collab(&pool).await;
        let base = format!(
            "/api/v1/workspaces/{}/projects/{}",
            seed.slug, seed.project_id
        );
        let (status_a, a) = call(
            app.clone(),
            "GET",
            &format!("{base}/members/"),
            Some(&seed.owner_key),
            None,
        )
        .await;
        let (status_b, b) = call(
            app.clone(),
            "GET",
            &format!("{base}/project-members/"),
            Some(&seed.owner_key),
            None,
        )
        .await;
        assert_eq!(status_a, StatusCode::OK);
        assert_eq!(status_b, StatusCode::OK);
        assert_eq!(a, b, "aliases serve identical bytes");
        let body = json_of(&a);
        let ids: std::collections::HashSet<String> = body
            .as_array()
            .expect("array")
            .iter()
            .map(|u| u["id"].as_str().expect("id").to_owned())
            .collect();
        assert!(ids.contains(&seed.owner_id));
        assert!(ids.contains(&seed.member_id));
        for entry in body.as_array().expect("array") {
            let keys: std::collections::HashSet<String> =
                entry.as_object().expect("object").keys().cloned().collect();
            assert_eq!(keys, user_keys());
        }
        // Detail aliases likewise.
        let mid: String = sqlx::query_scalar("SELECT id::text FROM project_members WHERE project_id = $1::uuid AND member_id = $2::uuid")
            .bind(&seed.project_id)
            .bind(&seed.member_id)
            .fetch_one(&pool)
            .await
            .expect("membership");
        let (status_a, a) = call(
            app.clone(),
            "GET",
            &format!("{base}/members/{mid}/"),
            Some(&seed.owner_key),
            None,
        )
        .await;
        let (status_b, b) = call(
            app,
            "GET",
            &format!("{base}/project-members/{mid}/"),
            Some(&seed.owner_key),
            None,
        )
        .await;
        assert_eq!((status_a, status_b), (StatusCode::OK, StatusCode::OK));
        assert_eq!(a, b, "detail aliases serve identical bytes");
    }

    #[tokio::test]
    async fn live_member_create_validation() {
        let Some(pool) = live_pool().await else {
            return;
        };
        let Some(app) = live_app(&pool).await else {
            return;
        };
        let seed = seed_collab(&pool).await;
        let path = format!(
            "/api/v1/workspaces/{}/projects/{}/members/",
            seed.slug, seed.project_id
        );
        // Unknown role.
        let (status, bytes) = call(
            app.clone(),
            "POST",
            &path,
            Some(&seed.owner_key),
            Some(serde_json::json!({"member": seed.member_id, "role": 99})),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(json_of(&bytes).get("role").is_some());
        // Existing user outside the workspace.
        let (status, bytes) = call(
            app.clone(),
            "POST",
            &path,
            Some(&seed.owner_key),
            Some(serde_json::json!({"member": seed.outsider_id, "role": MEMBER})),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(json_of(&bytes).get("member").is_some());
        // Missing member.
        let (status, bytes) = call(
            app.clone(),
            "POST",
            &path,
            Some(&seed.owner_key),
            Some(serde_json::json!({"role": MEMBER})),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(json_of(&bytes).get("member").is_some());
        // Happy path via a fresh workspace user.
        let fresh = uuid::Uuid::new_v4().to_string();
        let email = format!("ct-fresh-{}.{}@example.com", seed.slug, &fresh[..6]);
        sqlx::query(
            "INSERT INTO users (id, password, username, email, first_name, last_name,
                    avatar, date_joined, created_at, updated_at, last_location,
                    created_location, is_superuser, is_managed, is_password_expired,
                    is_active, is_staff, is_email_verified, is_password_autoset, token,
                    user_timezone, last_login_ip, last_logout_ip, last_login_medium,
                    last_login_uagent, is_bot, display_name, is_email_valid,
                    is_password_reset_required)
                 VALUES ($1, '!', $2, $2, 'Ct', 'Fresh',
                    '', now(), now(), now(), '',
                    '', false, false, false,
                    true, false, true, false, '',
                    'UTC', '', '', '',
                    '', false, 'Ct Fresh', true,
                    false)",
        )
        .bind(uid(&fresh))
        .bind(&email)
        .execute(&pool)
        .await
        .expect("user");
        sqlx::query("INSERT INTO workspace_members (id, workspace_id, member_id, role, is_active, view_props, default_props, issue_props, explored_features, getting_started_checklist, tips, created_at, updated_at) VALUES ($1, (SELECT id FROM workspaces WHERE slug = $2), $3::uuid, $4, true, '{}', '{}', '{}', '{}', '{}', '{}', now(), now())")
            .bind(uuid::Uuid::new_v4()).bind(&seed.slug).bind(&fresh).bind(MEMBER).execute(&pool).await.expect("ws member");
        let (status, bytes) = call(
            app.clone(),
            "POST",
            &path,
            Some(&seed.owner_key),
            Some(serde_json::json!({"member": fresh, "role": MEMBER})),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        let created = json_of(&bytes);
        assert_eq!(created["member"], fresh);
        assert_eq!(created["role"], MEMBER);
        // The save() side effect: a ProjectUserProperty row exists.
        let pups: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM project_user_properties WHERE project_id = $1::uuid AND user_id = $2::uuid AND deleted_at IS NULL")
            .bind(&seed.project_id).bind(&fresh).fetch_one(&pool).await.expect("pup");
        assert_eq!(pups, 1);
    }

    #[tokio::test]
    async fn live_member_create_after_app_accept_insert() {
        // The app-project invite-accept INSERT is byte-identical to this
        // handler's, so on one connection the v1 add reuses the statement
        // the app path prepared; both must bind role as i16 (PIDASHCONV-1509).
        let url = match std::env::var("DATABASE_URL") {
            Ok(url) => url,
            Err(_) => {
                eprintln!("skipping live-db test: DATABASE_URL is not set");
                return;
            }
        };
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .expect("connect to scratch DATABASE_URL");
        let seed = seed_collab(&pool).await;
        crate::app_project::handlers_invites::create_project_member(
            &pool,
            &uid(&seed.project_id),
            Some(uid(&seed.outsider_id)),
            15_i16,
            Some(uid(&seed.owner_id)),
        )
        .await
        .expect("app accept insert");
        let state = AppState::new("test").with_pools(pidash_db::Pools::primary_only(pool.clone()));
        let app = Router::new().merge(routes()).with_state(state);
        let (status, bytes) = call(
            app,
            "POST",
            &format!(
                "/api/v1/workspaces/{}/projects/{}/members/",
                seed.slug, seed.project_id
            ),
            Some(&seed.owner_key),
            Some(serde_json::json!({"member": seed.guest_id, "role": MEMBER})),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::CREATED,
            "body: {}",
            String::from_utf8_lossy(&bytes)
        );
    }

    #[tokio::test]
    async fn live_member_detail_get_patch_delete() {
        let Some(pool) = live_pool().await else {
            return;
        };
        let Some(app) = live_app(&pool).await else {
            return;
        };
        let seed = seed_collab(&pool).await;
        let mid: String = sqlx::query_scalar("SELECT id::text FROM project_members WHERE project_id = $1::uuid AND member_id = $2::uuid")
            .bind(&seed.project_id).bind(&seed.member_id).fetch_one(&pool).await.expect("mid");
        let base = format!(
            "/api/v1/workspaces/{}/projects/{}",
            seed.slug, seed.project_id
        );
        // Detail GET returns the user profile, not the membership row.
        let (status, bytes) = call(
            app.clone(),
            "GET",
            &format!("{base}/members/{mid}/"),
            Some(&seed.owner_key),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let got = json_of(&bytes);
        let keys: std::collections::HashSet<String> =
            got.as_object().expect("object").keys().cloned().collect();
        assert_eq!(keys, user_keys());
        assert_eq!(got["id"], seed.member_id);
        // PATCH changes the role; the answer is the membership shape.
        let (status, bytes) = call(
            app.clone(),
            "PATCH",
            &format!("{base}/members/{mid}/"),
            Some(&seed.owner_key),
            Some(serde_json::json!({"role": 5})),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json_of(&bytes)["role"], 5);
        // Guest (workspace member, no project row) PATCHes: 403.
        let (status, _) = call(
            app.clone(),
            "PATCH",
            &format!("{base}/members/{mid}/"),
            Some(&seed.guest_key),
            Some(serde_json::json!({"role": 5})),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        // DELETE flips is_active, keeps the row.
        let (status, _) = call(
            app.clone(),
            "DELETE",
            &format!("{base}/members/{mid}/"),
            Some(&seed.owner_key),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let row: (bool,) =
            sqlx::query_as("SELECT is_active FROM project_members WHERE id = $1::uuid")
                .bind(&mid)
                .fetch_one(&pool)
                .await
                .expect("row");
        assert!(!row.0);
    }

    #[tokio::test]
    async fn live_member_denied_and_isolation() {
        let Some(pool) = live_pool().await else {
            return;
        };
        let Some(app) = live_app(&pool).await else {
            return;
        };
        let seed = seed_collab(&pool).await;
        let other = seed_collab(&pool).await;
        let path = format!(
            "/api/v1/workspaces/{}/projects/{}/members/",
            seed.slug, seed.project_id
        );
        let (status, _) = call(app.clone(), "GET", &path, Some(&seed.outsider_key), None).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, _) = call(
            app.clone(),
            "POST",
            &path,
            Some(&seed.outsider_key),
            Some(serde_json::json!({"member": seed.guest_id, "role": MEMBER})),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        // Cross-workspace: the other owner cannot reach this project.
        let (status, _) = call(app, "GET", &path, Some(&other.owner_key), None).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn live_invites_crud_and_guards() {
        let Some(pool) = live_pool().await else {
            return;
        };
        let Some(app) = live_app(&pool).await else {
            return;
        };
        let seed = seed_collab(&pool).await;
        let base = format!("/api/v1/workspaces/{}/invitations", seed.slug);
        // Create.
        let email = format!("ct-inv-{}.{}@example.com", seed.slug, &seed.project_id[..6]);
        let (status, bytes) = call(
            app.clone(),
            "POST",
            &format!("{base}/"),
            Some(&seed.owner_key),
            Some(serde_json::json!({"email": email, "role": MEMBER})),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        let created = json_of(&bytes);
        let keys: std::collections::HashSet<String> = created
            .as_object()
            .expect("object")
            .keys()
            .cloned()
            .collect();
        assert_eq!(
            keys,
            [
                "id",
                "email",
                "role",
                "created_at",
                "updated_at",
                "responded_at",
                "accepted"
            ]
            .iter()
            .map(|s| s.to_string())
            .collect()
        );
        assert_eq!(created["accepted"], false);
        let iid = created["id"].as_str().expect("id").to_owned();
        // Duplicate email in the workspace: 400.
        let (status, bytes) = call(
            app.clone(),
            "POST",
            &format!("{base}/"),
            Some(&seed.owner_key),
            Some(serde_json::json!({"email": email, "role": MEMBER})),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let dup = json_of(&bytes);
        assert!(dup.get("non_field_errors").is_some() || dup.get("email").is_some());
        // Malformed email + unknown role.
        let (status, bytes) = call(
            app.clone(),
            "POST",
            &format!("{base}/"),
            Some(&seed.owner_key),
            Some(serde_json::json!({"email": "not-an-email", "role": MEMBER})),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(json_of(&bytes).get("email").is_some());
        let (status, bytes) = call(
            app.clone(),
            "POST",
            &format!("{base}/"),
            Some(&seed.owner_key),
            Some(serde_json::json!({"email": "ct-ok@example.com", "role": 99})),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(json_of(&bytes).get("role").is_some());
        // Retrieve + patch role; email is immutable even unchanged.
        let (status, _) = call(
            app.clone(),
            "GET",
            &format!("{base}/{iid}/"),
            Some(&seed.owner_key),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let (status, bytes) = call(
            app.clone(),
            "PATCH",
            &format!("{base}/{iid}/"),
            Some(&seed.owner_key),
            Some(serde_json::json!({"role": 5})),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json_of(&bytes)["role"], 5);
        let (status, bytes) = call(
            app.clone(),
            "PATCH",
            &format!("{base}/{iid}/"),
            Some(&seed.owner_key),
            Some(serde_json::json!({"email": email})),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(json_of(&bytes)["code"], "EMAIL_CANNOT_BE_UPDATED");
        // Non-admin workspace member may not manage invites.
        let (status, _) = call(
            app.clone(),
            "GET",
            &format!("{base}/"),
            Some(&seed.guest_key),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        // Destroy, then 404; accepted invite 400s with its code.
        let (status, _) = call(
            app.clone(),
            "DELETE",
            &format!("{base}/{iid}/"),
            Some(&seed.owner_key),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (status, _) = call(
            app.clone(),
            "GET",
            &format!("{base}/{iid}/"),
            Some(&seed.owner_key),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let email2 = format!("ct-acc-{}.{}@example.com", seed.slug, &seed.project_id[..6]);
        sqlx::query("INSERT INTO workspace_member_invites (id, email, accepted, token, role, workspace_id, created_by_id, responded_at, created_at, updated_at) VALUES ($1, $2, true, 't', $3, (SELECT id FROM workspaces WHERE slug = $4), $5::uuid, now(), now(), now())")
            .bind(uuid::Uuid::new_v4()).bind(&email2).bind(MEMBER).bind(&seed.slug).bind(&seed.owner_id)
            .execute(&pool).await.expect("accepted invite");
        let acc_id: String =
            sqlx::query_scalar("SELECT id::text FROM workspace_member_invites WHERE email = $1")
                .bind(&email2)
                .fetch_one(&pool)
                .await
                .expect("id");
        let (status, bytes) = call(
            app,
            "DELETE",
            &format!("{base}/{acc_id}/"),
            Some(&seed.owner_key),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(json_of(&bytes)["code"], "INVITE_ALREADY_ACCEPTED");
    }

    #[tokio::test]
    async fn live_users_me() {
        let Some(pool) = live_pool().await else {
            return;
        };
        let Some(app) = live_app(&pool).await else {
            return;
        };
        let seed = seed_collab(&pool).await;
        let (status, bytes) = call(
            app.clone(),
            "GET",
            "/api/v1/users/me/",
            Some(&seed.owner_key),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let me = json_of(&bytes);
        let keys: std::collections::HashSet<String> =
            me.as_object().expect("object").keys().cloned().collect();
        assert_eq!(keys, user_keys());
        assert_eq!(me["id"], seed.owner_id);
        let (status, _) = call(app.clone(), "GET", "/api/v1/users/me/", None, None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        let (status, bytes) = call(
            app,
            "GET",
            "/api/v1/users/me/",
            Some("ct-bogus-token"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(
            json_of(&bytes),
            serde_json::json!({"detail": "Given API token is not valid"})
        );
    }
}
