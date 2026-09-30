#![forbid(unsafe_code)]

//! State / estimate route handlers (D-19, stage 5, PIDASHCONV-372).
//!
//! Ports `apps/api/pi_dash/api/views/state.py` (all of it: `StateListCreateAPIEndpoint`
//! L39-159, `StateDetailAPIEndpoint` L162-300) and the URL table
//! `apps/api/pi_dash/api/urls/state.py` (2 entries) plus
//! `apps/api/pi_dash/api/urls/estimate.py` (3 entries, defined but never registered).
//! Fixture: `rust-api/fixtures/v1_projects/handlers/workflow.golden.json`
//! (FX-H-STATEEST; trace: `rust-api/fixtures/v1_projects/TRACE.md`).
//!
//! Owned paths (registered by [`routes`], cutover granularity — everything else,
//! including every estimate path, stays unmatched and proxies to Django):
//!
//! * `GET+POST /api/v1/workspaces/<slug>/projects/<project_id>/states/`
//!   (`StateListCreateAPIEndpoint`, `views/state.py:39-159`).
//! * `GET+PATCH+DELETE .../states/<uuid:state_id>/`
//!   (`StateDetailAPIEndpoint`, `views/state.py:162-300`).
//!
//! Non-owned methods on those paths proxy to Django through the edge fallback
//! (sibling actions and metadata live there); `HEAD`/`TRACE` are owned and
//! answer DRF's JSON 405 after auth (see [`routes`]); `OPTIONS` proxies so
//! DRF metadata (401 anon / 200 authed) is preserved. Same precedent as
//! `license::owned`.
//!
//! Layering: permission gates come from [`super::perms`] (PIDASHCONV-367, Done);
//! the read shapes and validators from `pidash_services::v1_projects::ser_workflow`
//! (PIDASHCONV-351, Done); the queryset SQL from
//! `pidash_db::v1_projects::queries_stateest` (PIDASHCONV-366, Done); the row types
//! from `pidash_db::v1_projects::models` (PIDASHCONV-352, Done); cursor math and the
//! 12-key envelope from [`crate::paginator`]; datetime rendering from
//! [`crate::serializer`]. This module owns the HTTP shell (routes, API-key auth,
//! the state gate, identifier routing), the write SQL, and the row fetching.
//!
//! Request edge order (mirrors `BaseAPIView.initial`, `views/base.py:107-111`,
//! plus `TimezoneMixin`): API-key authentication first (missing → 401, invalid →
//! 403; only the token row's `is_active` is checked, never the user's), then the
//! slug→UUID rewrite (anonymous callers skip it so slugs cannot be probed), then
//! `check_permissions` (the `ProjectStateEntityPermission` gate → 403), and only
//! then timezone activation for survivors (`TimezoneMixin.initial` runs after
//! `super().initial()`), then the handler body. An unknown stored zone 500s for
//! survivors only — never ahead of a denial.
//!
//! Ported bugs (translate, don't redesign; also listed in the PR):
//!
//! * BUG-EST-404 — the estimate URL patterns (`api/urls/estimate.py`, 3 entries) are
//!   never registered in `api/urls/__init__.py`, so every estimate route answers
//!   Django's default 404 for authenticated AND anonymous callers alike (URL
//!   resolution precedes auth, so no 401/403 oracle). Reproduced by NOT registering
//!   them: unmatched paths fall through to the edge proxy, which serves Django's own
//!   404 byte-for-byte until a follow-up registers the routes.
//! * State create answers 200, not 201 (`views/state.py:113-114`). Ported as-is.
//! * `StateSerializer.validate` runs the default-flip UPDATE before the triage
//!   rejection (`serializers/state.py:19-27`, BUG-4a): a rejected triage payload still
//!   clears its siblings' defaults — on POST only. PATCH builds the serializer
//!   with no context (`views/state.py:279`), so `filter(project_id=None)`
//!   matches zero rows and the flip is a no-op there; the triage rejection
//!   still applies. The flip runs via
//!   [`pidash_services::v1_projects::ser_workflow::state_validate`], which returns the
//!   flip flag alongside the error so callers execute it even on rejection — but only
//!   when field validation passed (DRF never calls `validate()` after field errors).
//! * A caller-supplied `sequence` is overwritten by `State.save()` with
//!   max(sibling sequence)+15000 whenever siblings exist (`db/models/state.py:131-139`,
//!   BUG-4b); the max runs over the triage-excluding default manager.
//! * PATCH external-id clash echoes `str(state.id)` of the TARGET row, not the
//!   conflicting row (`views/state.py:291-297`) — the conflicting id is available but
//!   the code echoes the target. Ported as-is.
//! * PATCH/DELETE direct gets (`views/state.py:231,278`) skip the archived-project
//!   guard (no `projects` join) while list/detail scopes hide archived projects' states.
//!   The DELETE get additionally filters `is_triage=False` (`state.py:231`); the
//!   PATCH get does not (`state.py:278`) — separate scopes, same missing guard.
//! * A name-clash 409 whose lookup misses the holder raises through the generic 500
//!   branch (Python `AttributeError` on `None.id`); a PATCH `save()` unique violation
//!   answers 400 `{"error": "The payload is not valid"}` (uncaught `IntegrityError` →
//!   `handle_exception` branch).
//! * `expand=<unmapped field>` overwrites that field with `null`
//!   (`getattr(instance, f"{expand}_id", None)`, `serializers/base.py`).
//! * Non-ASCII names slugify differently: Django `slugify` folds NFKD → ASCII
//!   (`db/models/state.py`, via `State.save`), while
//!   [`state_model::slugify_name`] keeps non-ASCII alphanumerics
//!   (`char::is_alphanumeric`). ASCII names are byte-exact; non-ASCII names
//!   diverge. The slug helper lives in the models layer (PIDASHCONV-352,
//!   read-only for port agents), so this stays a documented divergence.
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

use std::collections::HashMap;

use axum::body::Bytes;
use axum::extract::{Path, Query, State as AxumState};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Router;
use chrono_tz::Tz;
use serde_json::Value;
use uuid::Uuid;

use pidash_auth::permissions::is_safe_method;
use pidash_auth::permissions::project as perm_project;
use pidash_auth::permissions::workspace as perm_workspace;
use pidash_auth::scope::TenantScope;
use pidash_auth::token as token_kernel;
use pidash_db::v1_projects::models::state as state_model;
use pidash_db::v1_projects::queries_stateest as state_q;
use pidash_services::v1_projects::ser_workflow as ser;
use pidash_types::{ProjectId, WorkspaceId};

use super::perms::{decide, gate_for, V1ProjectsGate, V1Route, CLASS_DENIAL_BODY};
use crate::paginator::{self, Cursor, PageResponse};
use crate::state::AppState;

/// One `machine_token` auth lookup row: user, workspace, dev-machine, revocation.
type MachineTokenLookup = Option<(
    Uuid,
    Uuid,
    Option<Uuid>,
    Option<chrono::DateTime<chrono::Utc>>,
)>;
/// One project lite row for `expand=project` (direct columns + cover asset FK).
type ProjectLiteLookup = Option<(
    Uuid,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<serde_json::Value>,
    Option<String>,
    Option<String>,
    bool,
    Option<Uuid>,
)>;
/// One user lite row for `expand=created_by/updated_by` (+ avatar asset FK).
type UserLiteLookup = Option<(
    Uuid,
    String,
    String,
    Option<String>,
    String,
    String,
    Option<Uuid>,
)>;
/// One `file_assets` row for `asset_url` rendering.
type AssetLookup = Option<(String, Option<Uuid>, Option<Uuid>, Option<Uuid>)>;

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------

/// Register the two owned state paths. Estimate paths are deliberately absent
/// (BUG-EST-404, see module docs): they proxy to Django, which 404s.
///
/// `HEAD` and `TRACE` are owned explicitly (not axum's automatic HEAD-from-GET
/// or its empty-body 405): DRF checks `http_method_names` only after
/// `initial()`, so auth/permission denials win and survivors answer the JSON
/// 405. `OPTIONS` proxies so DRF metadata (401 anon / 200 authed) is
/// preserved.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/workspaces/{slug}/projects/{project_id}/states/",
            axum::routing::get(list_states)
                .post(create_state)
                .put(crate::edge::proxy)
                .patch(crate::edge::proxy)
                .delete(crate::edge::proxy)
                .options(crate::edge::proxy)
                .head(head_state_list)
                .trace(trace_state_list),
        )
        .route(
            "/api/v1/workspaces/{slug}/projects/{project_id}/states/{state_id}/",
            axum::routing::get(retrieve_state)
                .patch(patch_state)
                .delete(delete_state)
                .post(crate::edge::proxy)
                .put(crate::edge::proxy)
                .options(crate::edge::proxy)
                .head(head_state_detail)
                .trace(trace_state_detail),
        )
}

/// Owned-path unowned methods (`HEAD`, `TRACE`): run the full prelude
/// (auth → rewrite → gate) and answer DRF's `MethodNotAllowed` JSON only
/// for survivors — denials keep their 401/403/404.
async fn method_not_allowed(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    project_raw: &str,
    route: V1Route,
    method: &str,
) -> Response {
    match authorize(state, headers, slug, project_raw, route, method).await {
        Ok(_) => Response::builder()
            .status(StatusCode::METHOD_NOT_ALLOWED)
            .header(header::CONTENT_TYPE, "application/json")
            .body(axum::body::Body::from(method_not_allowed_body(method)))
            .expect("405 response"),
        Err(error) => error.into_response(),
    }
}

/// DRF `MethodNotAllowed` detail (`APIView.http_method_not_allowed`):
/// `Method "<METHOD>" not allowed.`, compact-rendered.
fn method_not_allowed_body(method: &str) -> String {
    format!(
        "{{\"detail\":{}}}",
        json_string(&format!("Method \"{method}\" not allowed."))
    )
}

async fn head_state_list(
    AxumState(state): AxumState<AppState>,
    Path((slug, project_raw)): Path<(String, String)>,
    headers: HeaderMap,
) -> Response {
    method_not_allowed(
        &state,
        &headers,
        &slug,
        &project_raw,
        V1Route::StateList,
        "HEAD",
    )
    .await
}

async fn trace_state_list(
    AxumState(state): AxumState<AppState>,
    Path((slug, project_raw)): Path<(String, String)>,
    headers: HeaderMap,
) -> Response {
    method_not_allowed(
        &state,
        &headers,
        &slug,
        &project_raw,
        V1Route::StateList,
        "TRACE",
    )
    .await
}

async fn head_state_detail(
    AxumState(state): AxumState<AppState>,
    Path((slug, project_raw, _)): Path<(String, String, String)>,
    headers: HeaderMap,
) -> Response {
    method_not_allowed(
        &state,
        &headers,
        &slug,
        &project_raw,
        V1Route::StateDetail,
        "HEAD",
    )
    .await
}

async fn trace_state_detail(
    AxumState(state): AxumState<AppState>,
    Path((slug, project_raw, _)): Path<(String, String, String)>,
    headers: HeaderMap,
) -> Response {
    method_not_allowed(
        &state,
        &headers,
        &slug,
        &project_raw,
        V1Route::StateDetail,
        "TRACE",
    )
    .await
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Exact wire bodies (DRF defaults + `views/base.py` handle-exception matrix +
/// the view-inline `{"error": ...}` bodies).
pub const UNAUTHENTICATED_BODY: &str =
    r#"{"detail":"Authentication credentials were not provided."}"#;
/// `AuthenticationFailed("Given API token is not valid")`
/// (`api/middleware/api_authentication.py`).
pub const INVALID_TOKEN_BODY: &str = r#"{"detail":"Given API token is not valid"}"#;
/// `handle_exception`'s `ObjectDoesNotExist` branch (`views/base.py:138-143`).
pub const NOT_FOUND_BODY: &str = r#"{"error":"The requested resource does not exist."}"#;
/// `handle_exception`'s `IntegrityError` branch (`views/base.py:134-137`).
pub const INVALID_PAYLOAD_BODY: &str = r#"{"error":"The payload is not valid"}"#;
/// `handle_exception`'s generic branch (`views/base.py:156-160`).
pub const SERVER_ERROR_BODY: &str = r#"{"error":"Something went wrong please try again later"}"#;

/// Handler failure with its exact status + body, mirroring the `handle_exception`
/// matrix plus the DRF denials these views inherit.
#[derive(Debug)]
enum HandlerError {
    /// 401, DRF `NotAuthenticated` (no API key on a guarded route).
    Unauthorized,
    /// 403, invalid API/machine token (`AuthenticationFailed`).
    InvalidToken,
    /// 403, gate denial (`CLASS_DENIAL_BODY`, the DRF-default `PermissionDenied`).
    Forbidden,
    /// 404, `ObjectDoesNotExist` branch (detail `.get()` misses, direct gets).
    NotFound,
    /// 404, `Http404("Project not found")` (unresolvable project identifier —
    ///   DRF propagates the args: `NotFound("Project not found")`).
    ProjectNotFound,
    /// 400, `{"detail": ...}` (`ParseError`: bad per_page/cursor/JSON).
    BadDetail(String),
    /// 400, serializer `errors` dict (already rendered).
    FieldErrors(String),
    /// 400, view-inline `{"error": ...}` bodies.
    BadError(String),
    /// 409, view-inline conflict bodies (already rendered).
    Conflict(String),
    /// 400, `IntegrityError` branch (PATCH save collision).
    InvalidPayload,
    /// 500, generic branch.
    ServerError,
}

impl HandlerError {
    fn status_and_body(&self) -> (StatusCode, String) {
        match self {
            HandlerError::Unauthorized => {
                (StatusCode::UNAUTHORIZED, UNAUTHENTICATED_BODY.to_owned())
            }
            HandlerError::InvalidToken => (StatusCode::FORBIDDEN, INVALID_TOKEN_BODY.to_owned()),
            HandlerError::Forbidden => (StatusCode::FORBIDDEN, CLASS_DENIAL_BODY.to_owned()),
            HandlerError::NotFound => (StatusCode::NOT_FOUND, NOT_FOUND_BODY.to_owned()),
            HandlerError::ProjectNotFound => (
                StatusCode::NOT_FOUND,
                r#"{"detail":"Project not found"}"#.to_owned(),
            ),
            HandlerError::BadDetail(message) => (
                StatusCode::BAD_REQUEST,
                format!("{{\"detail\":{}}}", json_string(message)),
            ),
            HandlerError::FieldErrors(body) | HandlerError::Conflict(body) => {
                let status = match self {
                    HandlerError::Conflict(_) => StatusCode::CONFLICT,
                    _ => StatusCode::BAD_REQUEST,
                };
                (status, body.clone())
            }
            HandlerError::BadError(message) => (
                StatusCode::BAD_REQUEST,
                format!("{{\"error\":{}}}", json_string(message)),
            ),
            HandlerError::InvalidPayload => {
                (StatusCode::BAD_REQUEST, INVALID_PAYLOAD_BODY.to_owned())
            }
            HandlerError::ServerError => (
                StatusCode::INTERNAL_SERVER_ERROR,
                SERVER_ERROR_BODY.to_owned(),
            ),
        }
    }
}

impl IntoResponse for HandlerError {
    fn into_response(self) -> Response {
        if matches!(self, HandlerError::ServerError) {
            tracing::warn!("v1 state handler: internal error");
        }
        let (status, body) = self.status_and_body();
        Response::builder()
            .status(status)
            .header(header::CONTENT_TYPE, "application/json")
            .body(axum::body::Body::from(body))
            .expect("handler error response")
    }
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).expect("json string")
}

/// Render a 200 JSON response with exact bytes (DRF `JSONRenderer`, compact).
fn json_ok(body: String) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body))
        .expect("handler json response")
}

// ---------------------------------------------------------------------------
// Authentication (`APIKeyAuthentication`, `api/middleware/api_authentication.py`)
// ---------------------------------------------------------------------------

/// The authenticated actor: user id plus rendering timezone
/// (`TimezoneMixin.initial`, `views/base.py:50-58`).
#[derive(Debug, Clone, Copy)]
struct Actor {
    id: Uuid,
    timezone: Tz,
}

fn pool_of(state: &AppState) -> Result<&sqlx::PgPool, HandlerError> {
    state
        .pools()
        .map(|pools| pools.primary())
        .ok_or(HandlerError::ServerError)
}

/// The validated token identity: the user id plus the raw stored timezone.
/// The timezone string is NOT parsed here — `TimezoneMixin.initial`
/// (`views/base.py:43-48`) calls `super().initial()` (auth+permissions)
/// first and only then activates the zone, so a denying gate answers 403
/// even when the stored zone is unknown (which 500s only for survivors).
#[derive(Debug, Clone)]
struct Identity {
    id: Uuid,
    timezone: Option<String>,
}

/// Authenticate `X-Api-Key` (`APIKeyAuthentication.authenticate`): missing/empty →
/// `None` (later 401s via the permission layer); `mt_`-prefixed → machine-token
/// path; otherwise the `api_tokens` path. Any validation failure raises
/// `AuthenticationFailed("Given API token is not valid")` → 403.
async fn authenticate(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<Option<Identity>, HandlerError> {
    let pool = pool_of(state)?;
    let raw = headers
        .get(token_kernel::API_KEY_HEADER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    match token_kernel::classify_token(raw) {
        None => Ok(None),
        Some(token_kernel::TokenKind::Api) => authenticate_api_token(pool, raw).await.map(Some),
        Some(token_kernel::TokenKind::Machine) => {
            authenticate_machine_token(state, pool, raw).await.map(Some)
        }
    }
}

/// `validate_api_token`: exact token match, `is_active`, unexpired
/// (`expired_at__gt=now OR expired_at IS NULL`); stamps `last_used`.
/// Only the TOKEN row's `is_active` is checked — the user row's is not
/// (`api_authentication.py:32-45` returns `api_token.user` unchecked, and
/// DRF `IsAuthenticated` passes inactive users on to the permission path).
async fn authenticate_api_token(pool: &sqlx::PgPool, raw: &str) -> Result<Identity, HandlerError> {
    let row: Option<(Uuid, bool, Option<chrono::DateTime<chrono::Utc>>)> =
        sqlx::query_as(r#"SELECT user_id, is_active, expired_at FROM api_tokens WHERE token = $1"#)
            .bind(raw)
            .fetch_optional(pool)
            .await
            .map_err(|_| HandlerError::ServerError)?;
    let Some((user_id, is_active, expired_at)) = row else {
        return Err(HandlerError::InvalidToken);
    };
    let now_unix = chrono::Utc::now().timestamp();
    let kernel_row = token_kernel::ApiTokenRow {
        token: raw.to_owned(),
        is_active,
        expired_at_unix: expired_at.map(|dt| dt.timestamp()),
    };
    // Same predicates as the kernel documents; every arm is the same 403.
    token_kernel::validate_api_token(Some(&kernel_row), raw, now_unix)
        .map_err(|_| HandlerError::InvalidToken)?;
    // `api_token.save(update_fields=["last_used"])`: operational failures 500
    // through the generic branch, like Python.
    sqlx::query(r#"UPDATE api_tokens SET last_used = now() WHERE token = $1"#)
        .bind(raw)
        .execute(pool)
        .await
        .map_err(|_| HandlerError::ServerError)?;
    load_identity(pool, user_id).await
}

/// `validate_machine_token`: hash match, unrevoked token, unrevoked dev-machine,
/// workspace membership (revoke-then-deny otherwise); stamps `last_used_at`.
async fn authenticate_machine_token(
    state: &AppState,
    pool: &sqlx::PgPool,
    raw: &str,
) -> Result<Identity, HandlerError> {
    let secret = state.settings().secret_key.clone();
    let presented_hash = token_kernel::hash_token(raw, secret.as_bytes());
    let row: MachineTokenLookup = sqlx::query_as(
        r#"SELECT mt.user_id, mt.workspace_id, mt.dev_machine_id, mt.revoked_at
           FROM machine_token mt WHERE mt.token_hash = $1"#,
    )
    .bind(&presented_hash)
    .fetch_optional(pool)
    .await
    .map_err(|_| HandlerError::ServerError)?;
    let Some((user_id, workspace_id, dev_machine_id, revoked_at)) = row else {
        return Err(HandlerError::InvalidToken);
    };
    let dev_revoked = match dev_machine_id {
        None => false,
        Some(dm) => {
            let rev: Option<chrono::DateTime<chrono::Utc>> =
                sqlx::query_scalar(r#"SELECT revoked_at FROM dev_machine WHERE id = $1"#)
                    .bind(dm)
                    .fetch_optional(pool)
                    .await
                    .map_err(|_| HandlerError::ServerError)?
                    .flatten();
            rev.is_some()
        }
    };
    let kernel_row = token_kernel::MachineTokenRow {
        token_hash: presented_hash.clone(),
        revoked_at_unix: revoked_at.map(|dt| dt.timestamp()),
        dev_machine_revoked: dev_revoked,
    };
    token_kernel::validate_machine_token_static(Some(&kernel_row), &presented_hash)
        .map_err(|_| HandlerError::InvalidToken)?;
    // `is_workspace_member(user, workspace_id)`: any active membership row.
    // A non-member is revoked first, then denied (`core/permissions.py:28-36`).
    let member: Option<(i32,)> = sqlx::query_as(
        r#"SELECT 1 FROM workspace_members
           WHERE workspace_id = $1 AND member_id = $2 AND is_active AND deleted_at IS NULL"#,
    )
    .bind(workspace_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| HandlerError::ServerError)?;
    if member.is_none() {
        sqlx::query(r#"UPDATE machine_token SET revoked_at = now() WHERE token_hash = $1"#)
            .bind(&presented_hash)
            .execute(pool)
            .await
            .map_err(|_| HandlerError::ServerError)?;
        return Err(HandlerError::InvalidToken);
    }
    sqlx::query(r#"UPDATE machine_token SET last_used_at = now() WHERE token_hash = $1"#)
        .bind(&presented_hash)
        .execute(pool)
        .await
        .map_err(|_| HandlerError::ServerError)?;
    load_identity(pool, user_id).await
}

/// Load the token owner's identity. An unknown user id 401s (unreachable in
/// practice — both token tables FK to users); an INACTIVE user authenticates
/// normally (see `authenticate_api_token`). The timezone string is returned
/// raw; parsing happens after the gate in [`authorize`].
async fn load_identity(pool: &sqlx::PgPool, user_id: Uuid) -> Result<Identity, HandlerError> {
    let row: Option<(Option<String>,)> =
        sqlx::query_as(r#"SELECT user_timezone FROM users WHERE id = $1"#)
            .bind(user_id)
            .fetch_optional(pool)
            .await
            .map_err(|_| HandlerError::ServerError)?;
    match row {
        Some((timezone,)) => Ok(Identity {
            id: user_id,
            timezone,
        }),
        None => Err(HandlerError::Unauthorized),
    }
}

/// Activate the actor's rendering timezone (`TimezoneMixin.initial` runs
/// after `super().initial()`). A missing zone defaults to UTC; an unknown
/// zone name 500s (`ZoneInfo(...)` raises; no `try/except`).
fn activate_timezone(timezone: Option<&str>) -> Result<Tz, HandlerError> {
    timezone
        .unwrap_or("UTC")
        .parse()
        .map_err(|_| HandlerError::ServerError)
}

// ---------------------------------------------------------------------------
// Tenant + gate (`_rewrite_project_kwarg`, `ProjectStateEntityPermission`)
// ---------------------------------------------------------------------------

/// The authorized request context: who acts, in which project.
struct Gate {
    actor: Actor,
    project_id: Uuid,
}

/// Resolve the slug-or-UUID `project_id` kwarg (`_rewrite_project_kwarg`,
/// `views/base.py:52-104`, via `Project.resolve`, `db/models/project.py:191-224`):
/// UUIDs pass through unverified (the view body 404s/403s them as before);
/// other identifiers match `UPPER(identifier)` in the workspace; misses raise
/// `Http404("Project not found")`, which DRF renders as
/// `{"detail": "Project not found"}`. Anonymous callers skip the rewrite
/// (slug-existence oracle stays closed) and 401 below.
async fn resolve_project_id(
    pool: &sqlx::PgPool,
    slug: &str,
    raw: &str,
) -> Result<Uuid, HandlerError> {
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
    .map_err(|_| HandlerError::ServerError)?;
    row.map(|row| row.0).ok_or(HandlerError::ProjectNotFound)
}

/// Fetch the `ProjectStateEntityPermission` facts and decide
/// (`app/permissions/project.py:146-206`): SAFE methods need any active project row
/// on `view.project_id`; writes go through `can_mutate_states` (project ADMIN, or
/// MEMBER on a `members_can_edit_states` project, or workspace ADMIN holding a
/// project row). Denials answer [`CLASS_DENIAL_BODY`].
async fn resolve_state_gate(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: Uuid,
    actor_id: Option<Uuid>,
    route: V1Route,
    method: &str,
) -> Result<Uuid, HandlerError> {
    let actor_id = actor_id.ok_or(HandlerError::Unauthorized)?;
    let scope = TenantScope::new(WorkspaceId::from(slug));
    // Active project membership on this project (soft-deleted rows excluded,
    // like every other `ProjectMember.objects` read). `role` is `smallint`.
    let membership: Option<(i16,)> = sqlx::query_as(
        r#"SELECT pm.role FROM project_members pm
           JOIN workspaces w ON w.id = pm.workspace_id
           WHERE w.slug = $1 AND pm.project_id = $2 AND pm.member_id = $3
           AND pm.is_active AND pm.deleted_at IS NULL"#,
    )
    .bind(slug)
    .bind(project_id)
    .bind(actor_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| HandlerError::ServerError)?;
    let project_facts = perm_project::ProjectFacts {
        workspace: WorkspaceId::from(slug),
        project_id: ProjectId::from(project_id.to_string()),
        authenticated: true,
        is_workspace_member: false,
        has_workspace_admin_or_member: false,
        is_workspace_admin: false,
        is_project_member: membership.is_some(),
        is_project_admin: false,
        has_project_admin_or_member: false,
        has_identifier_membership: false,
        has_project_identifier: false,
    };
    // `can_mutate_states`: one membership row (role + the project's flag) plus
    // one workspace-admin check; a missing row denies before the admin override.
    let mutation = if is_safe_method(method) {
        perm_project::StateMutationFacts {
            authenticated: true,
            project_role: None,
            members_can_edit_states: false,
            is_workspace_admin: false,
        }
    } else {
        let row: Option<(i16, bool)> = sqlx::query_as(
            r#"SELECT pm.role, p.members_can_edit_states
               FROM project_members pm
               JOIN workspaces w ON w.id = pm.workspace_id
               JOIN projects p ON p.id = pm.project_id
               WHERE w.slug = $1 AND pm.project_id = $2 AND pm.member_id = $3
               AND pm.is_active AND pm.deleted_at IS NULL"#,
        )
        .bind(slug)
        .bind(project_id)
        .bind(actor_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| HandlerError::ServerError)?;
        let (role, flag) = match row {
            Some((role, flag)) => (Some(i32::from(role)), flag),
            None => (None, false),
        };
        let ws_admin: Option<(i32,)> = sqlx::query_as(
            r#"SELECT 1 FROM workspace_members wm
               JOIN workspaces w ON w.id = wm.workspace_id
               WHERE w.slug = $1 AND wm.member_id = $2 AND wm.role = 20
               AND wm.is_active AND wm.deleted_at IS NULL"#,
        )
        .bind(slug)
        .bind(actor_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| HandlerError::ServerError)?;
        perm_project::StateMutationFacts {
            authenticated: true,
            project_role: role,
            members_can_edit_states: flag,
            is_workspace_admin: ws_admin.is_some(),
        }
    };
    let gate = gate_for(route, method);
    debug_assert!(
        gate == V1ProjectsGate::ProjectStateEntity,
        "state routes carry ProjectStateEntityPermission"
    );
    let workspace_facts = perm_workspace::WorkspaceFacts {
        workspace: WorkspaceId::from(slug),
        authenticated: true,
        has_admin_or_member_role: false,
        has_admin_role: false,
        is_member: false,
        is_admin_unfiltered: false,
    };
    if decide(
        gate,
        method,
        &scope,
        &project_facts,
        &workspace_facts,
        &mutation,
    ) {
        Ok(actor_id)
    } else {
        Err(HandlerError::Forbidden)
    }
}

// ---------------------------------------------------------------------------
// Query params, `fields` / `expand`, rendering
// ---------------------------------------------------------------------------

/// One query value, repeated or not (mirrors `app_issues::OneOrMany`: axum's
/// `Query` backend does not coerce repeats, so callers read last-wins like
/// Django's `QueryDict.get`).
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(untagged)]
enum OneOrMany {
    One(String),
    Many(Vec<String>),
}

type QueryMap = HashMap<String, OneOrMany>;

fn query_last(query: &QueryMap, key: &str) -> Option<String> {
    query.get(key).map(|value| match value {
        OneOrMany::One(one) => one.clone(),
        OneOrMany::Many(many) => many.last().cloned().unwrap_or_default(),
    })
}

/// `BaseAPIView.fields` / `.expand` (`views/base.py:203-211`): comma-split,
/// empties dropped, `None` when empty.
fn csv_param(raw: Option<String>) -> Option<Vec<String>> {
    let items: Vec<String> = raw
        .unwrap_or_default()
        .split(',')
        .filter(|part| !part.is_empty())
        .map(str::to_owned)
        .collect();
    if items.is_empty() {
        None
    } else {
        Some(items)
    }
}

/// All `StateSerializer` wire fields, in live-DRF order (probed 2026-09-30 against
/// Django 4.2.30: declared `id`, the time/soft-delete columns, the plain model
/// fields in definition order, then the four FKs — `created_by`, `updated_by`,
/// `project`, `workspace` — last).
///
/// NOTE: this differs from
/// [`pidash_services::v1_projects::ser_workflow::StateView`], which documents
/// "declared `id`, then model definition order" (foundation, PIDASHCONV-351,
/// read-only here). The order below is what Django actually emits; rendering
/// lives in this module, so the port follows the wire, not the doc.
const STATE_FIELDS: &[&str] = &[
    "id",
    "created_at",
    "updated_at",
    "deleted_at",
    "name",
    "description",
    "color",
    "slug",
    "sequence",
    "group",
    "is_triage",
    "default",
    "external_source",
    "external_id",
    "created_by",
    "updated_by",
    "project",
    "workspace",
];

/// Expansion map applicable to states (`serializers/base.py:53-94`):
/// `project` → `ProjectLiteSerializer`, `workspace` → `WorkspaceLiteSerializer`,
/// `created_by` / `updated_by` → `UserLiteSerializer`.
fn expand_lite_kind(name: &str) -> Option<&'static str> {
    match name {
        "project" => Some("project"),
        "workspace" => Some("workspace"),
        "created_by" | "updated_by" => Some("user"),
        _ => None,
    }
}

/// One rendered state object as an ordered map, before `fields`/`expand`.
#[allow(clippy::too_many_arguments)]
fn state_base_map(
    id: &str,
    created_at: &str,
    updated_at: &str,
    created_by: Option<String>,
    updated_by: Option<String>,
    deleted_at: Option<String>,
    project: &str,
    workspace: &str,
    name: &str,
    description: &str,
    color: &str,
    slug: &str,
    sequence: f64,
    group: &str,
    is_triage: bool,
    default: bool,
    external_source: Option<&str>,
    external_id: Option<&str>,
) -> serde_json::Map<String, Value> {
    let mut map = serde_json::Map::with_capacity(STATE_FIELDS.len());
    let opt = |v: Option<String>| v.map(Value::String).unwrap_or(Value::Null);
    map.insert("id".to_owned(), Value::String(id.to_owned()));
    map.insert(
        "created_at".to_owned(),
        Value::String(created_at.to_owned()),
    );
    map.insert(
        "updated_at".to_owned(),
        Value::String(updated_at.to_owned()),
    );
    map.insert("deleted_at".to_owned(), opt(deleted_at));
    map.insert("name".to_owned(), Value::String(name.to_owned()));
    map.insert(
        "description".to_owned(),
        Value::String(description.to_owned()),
    );
    map.insert("color".to_owned(), Value::String(color.to_owned()));
    map.insert("slug".to_owned(), Value::String(slug.to_owned()));
    map.insert(
        "sequence".to_owned(),
        serde_json::Number::from_f64(sequence)
            .map(Value::Number)
            .unwrap_or(Value::Null),
    );
    map.insert("group".to_owned(), Value::String(group.to_owned()));
    map.insert("is_triage".to_owned(), Value::Bool(is_triage));
    map.insert("default".to_owned(), Value::Bool(default));
    map.insert(
        "external_source".to_owned(),
        external_source
            .map(|v| Value::String(v.to_owned()))
            .unwrap_or(Value::Null),
    );
    map.insert(
        "external_id".to_owned(),
        external_id
            .map(|v| Value::String(v.to_owned()))
            .unwrap_or(Value::Null),
    );
    map.insert("created_by".to_owned(), opt(created_by));
    map.insert("updated_by".to_owned(), opt(updated_by));
    map.insert("project".to_owned(), Value::String(project.to_owned()));
    map.insert("workspace".to_owned(), Value::String(workspace.to_owned()));
    map
}

/// Render one state row (`StateSerializer.to_representation`, `state.py:11-41`):
/// datetimes in the actor's zone, UUIDs/FKs as strings, then `fields` filtering
/// (`serializers/base.py:27-61` — unknown names ignored) and `expand`
/// (`base.py:62-94` — mapped names nest lite shapes, unmapped names present in
/// the fields overwrite with `null`).
#[allow(clippy::too_many_arguments)]
async fn render_state(
    pool: &sqlx::PgPool,
    row: &state_model::State,
    tz: &Tz,
    fields: Option<&[String]>,
    expand: Option<&[String]>,
) -> Result<Value, HandlerError> {
    let created_at = crate::serializer::render_datetime_in(&row.created_at, tz);
    let updated_at = crate::serializer::render_datetime_in(&row.updated_at, tz);
    let deleted_at = row
        .deleted_at
        .as_ref()
        .map(|dt| crate::serializer::render_datetime_in(dt, tz));
    let mut map = state_base_map(
        &row.id.to_string(),
        &created_at,
        &updated_at,
        row.created_by_id.map(|id| id.to_string()),
        row.updated_by_id.map(|id| id.to_string()),
        deleted_at,
        &row.project_id.to_string(),
        &row.workspace_id.to_string(),
        &row.name,
        &row.description,
        &row.color,
        &row.slug,
        row.sequence,
        &row.group,
        row.is_triage,
        row.default,
        row.external_source.as_deref(),
        row.external_id.as_deref(),
    );
    if let Some(wanted) = fields {
        let allowed: std::collections::HashSet<&str> = wanted.iter().map(String::as_str).collect();
        map.retain(|key, _| allowed.contains(key.as_str()));
    }
    if let Some(expands) = expand {
        for name in expands {
            if !map.contains_key(name) {
                continue;
            }
            match expand_lite_kind(name) {
                Some("project") => {
                    map.insert(name.clone(), expand_project(pool, &row.project_id).await?);
                }
                Some("workspace") => {
                    map.insert(
                        name.clone(),
                        expand_workspace(pool, &row.workspace_id).await?,
                    );
                }
                Some("user") => {
                    let user_id = if name == "created_by" {
                        row.created_by_id
                    } else {
                        row.updated_by_id
                    };
                    let nested = match user_id {
                        Some(id) => expand_user(pool, &id).await?,
                        // `expansion[expand](None).data` renders `{}` for a
                        // null FK (`serializers/base.py:108-113`, probed:
                        // `UserLiteSerializer(None).data == {}`).
                        None => Value::Object(serde_json::Map::new()),
                    };
                    map.insert(name.clone(), nested);
                }
                _ => {
                    // Unmapped expand present in the fields: `getattr(instance,
                    // f"{expand}_id", None)` — every state field misses, so null.
                    map.insert(name.clone(), Value::Null);
                }
            }
        }
    }
    Ok(Value::Object(map))
}

/// `ProjectLiteSerializer` (`serializers/project.py:354-377`): wire order is the
/// `Meta.fields` list order (probed: `id, identifier, name, cover_image, icon_prop,
/// emoji, description, is_default, cover_image_url`).
async fn expand_project(pool: &sqlx::PgPool, project_id: &Uuid) -> Result<Value, HandlerError> {
    let row: ProjectLiteLookup = sqlx::query_as(
        r#"SELECT id, identifier, name, cover_image, icon_prop, emoji, description,
                  is_default, cover_image_asset_id
           FROM projects WHERE id = $1"#,
    )
    .bind(project_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| HandlerError::ServerError)?;
    let Some((
        id,
        identifier,
        name,
        cover_image,
        icon_prop,
        emoji,
        description,
        is_default,
        cover_asset,
    )) = row
    else {
        return Ok(Value::Null);
    };
    // `cover_image_url` (`db/models/project.py:176-184`): the asset URL when a
    // cover asset exists (even when that URL is null); else the raw value when
    // truthy; else null. A dangling asset FK raises (`DoesNotExist` → 404).
    let cover_url = match cover_asset {
        Some(aid) => asset_url_for(pool, aid).await?,
        None => cover_image.clone().filter(|raw| !raw.is_empty()),
    };
    let mut map = serde_json::Map::with_capacity(9);
    map.insert("id".to_owned(), Value::String(id.to_string()));
    map.insert(
        "identifier".to_owned(),
        identifier.map(Value::String).unwrap_or(Value::Null),
    );
    map.insert(
        "name".to_owned(),
        name.map(Value::String).unwrap_or(Value::Null),
    );
    map.insert(
        "cover_image".to_owned(),
        cover_image.map(Value::String).unwrap_or(Value::Null),
    );
    map.insert("icon_prop".to_owned(), icon_prop.unwrap_or(Value::Null));
    map.insert(
        "emoji".to_owned(),
        emoji.map(Value::String).unwrap_or(Value::Null),
    );
    map.insert(
        "description".to_owned(),
        description.map(Value::String).unwrap_or(Value::Null),
    );
    map.insert("is_default".to_owned(), Value::Bool(is_default));
    map.insert(
        "cover_image_url".to_owned(),
        cover_url.map(Value::String).unwrap_or(Value::Null),
    );
    Ok(Value::Object(map))
}

/// `WorkspaceLiteSerializer` (`serializers/workspace.py:10-21`): wire order is the
/// `Meta.fields` list order (probed: `name, slug, id`).
async fn expand_workspace(pool: &sqlx::PgPool, workspace_id: &Uuid) -> Result<Value, HandlerError> {
    let row: Option<(Uuid, String, String)> =
        sqlx::query_as(r#"SELECT id, name, slug FROM workspaces WHERE id = $1"#)
            .bind(workspace_id)
            .fetch_optional(pool)
            .await
            .map_err(|_| HandlerError::ServerError)?;
    match row {
        Some((id, name, slug)) => {
            let mut map = serde_json::Map::with_capacity(3);
            map.insert("name".to_owned(), Value::String(name));
            map.insert("slug".to_owned(), Value::String(slug));
            map.insert("id".to_owned(), Value::String(id.to_string()));
            Ok(Value::Object(map))
        }
        None => Ok(Value::Null),
    }
}

/// `UserLiteSerializer` (`serializers/user.py:13-38`): declared `id` first, then
/// the model fields in `Meta.fields` order (`first_name`, `last_name`, `email`,
/// `avatar`, `avatar_url`, `display_name` — the duplicated `email` collapses).
async fn expand_user(pool: &sqlx::PgPool, user_id: &Uuid) -> Result<Value, HandlerError> {
    let row: UserLiteLookup = sqlx::query_as(
        r#"SELECT id, first_name, last_name, email, avatar, display_name, avatar_asset_id
           FROM users WHERE id = $1"#,
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| HandlerError::ServerError)?;
    let Some((id, first, last, email, avatar, display, avatar_asset)) = row else {
        return Ok(Value::Null);
    };
    // `avatar_url` (`db/models/user.py:142-151`): the asset URL when an avatar
    // asset exists (even when null); else the raw value when truthy; else null.
    let avatar_url = match avatar_asset {
        Some(aid) => asset_url_for(pool, aid).await?,
        None if !avatar.is_empty() => Some(avatar.clone()),
        None => None,
    };
    let mut map = serde_json::Map::with_capacity(7);
    map.insert("id".to_owned(), Value::String(id.to_string()));
    map.insert("first_name".to_owned(), Value::String(first));
    map.insert("last_name".to_owned(), Value::String(last));
    map.insert(
        "email".to_owned(),
        email.map(Value::String).unwrap_or(Value::Null),
    );
    map.insert("avatar".to_owned(), Value::String(avatar));
    map.insert(
        "avatar_url".to_owned(),
        avatar_url.map(Value::String).unwrap_or(Value::Null),
    );
    map.insert("display_name".to_owned(), Value::String(display));
    Ok(Value::Object(map))
}

/// Shared `avatar_url` / `cover_image_url` rendering (`db/models/user.py:142-151`,
/// `db/models/project.py:176-184` via `Asset.asset_url`, `db/models/asset.py:80-99`).
/// A dangling asset FK raises `DoesNotExist`, which `handle_exception` maps to
/// the 404 branch — hence [`HandlerError::NotFound`] here.
async fn asset_url_for(
    pool: &sqlx::PgPool,
    asset_id: Uuid,
) -> Result<Option<String>, HandlerError> {
    let row: AssetLookup = sqlx::query_as(
        r#"SELECT entity_type, workspace_id, project_id, issue_id FROM file_assets WHERE id = $1"#,
    )
    .bind(asset_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| HandlerError::ServerError)?;
    let Some((entity_type, workspace_id, project_id, issue_id)) = row else {
        return Err(HandlerError::NotFound);
    };
    match entity_type.as_str() {
        "WORKSPACE_LOGO" | "USER_AVATAR" | "USER_COVER" | "PROJECT_COVER" => {
            Ok(Some(format!("/api/assets/v2/static/{asset_id}/")))
        }
        "ISSUE_ATTACHMENT" => {
            let slug: Option<(String,)> = match workspace_id {
                Some(ws) => sqlx::query_as(r#"SELECT slug FROM workspaces WHERE id = $1"#)
                    .bind(ws)
                    .fetch_optional(pool)
                    .await
                    .map_err(|_| HandlerError::ServerError)?,
                None => None,
            };
            match (slug, project_id, issue_id) {
                (Some((slug,)), Some(pid), Some(iid)) => Ok(Some(format!(
                    "/api/assets/v2/workspaces/{slug}/projects/{pid}/issues/{iid}/attachments/{asset_id}/"
                ))),
                _ => Ok(None),
            }
        }
        "ISSUE_DESCRIPTION"
        | "COMMENT_DESCRIPTION"
        | "PAGE_DESCRIPTION"
        | "DRAFT_ISSUE_DESCRIPTION" => {
            let slug: Option<(String,)> = match workspace_id {
                Some(ws) => sqlx::query_as(r#"SELECT slug FROM workspaces WHERE id = $1"#)
                    .bind(ws)
                    .fetch_optional(pool)
                    .await
                    .map_err(|_| HandlerError::ServerError)?,
                None => None,
            };
            match (slug, project_id) {
                (Some((slug,)), Some(pid)) => Ok(Some(format!(
                    "/api/assets/v2/workspaces/{slug}/projects/{pid}/{asset_id}/"
                ))),
                _ => Ok(None),
            }
        }
        _ => Ok(None),
    }
}

// ---------------------------------------------------------------------------
// Request bodies + field validation (`StateSerializer`, `state.py:11-41`)
// ---------------------------------------------------------------------------

/// DRF type names for the non-object body error (`serializers/base.py` via
/// `ValidationError("Invalid data. Expected a dictionary, but got ...")`).
fn drf_type_name(value: &Value) -> &'static str {
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

/// Parse the request body the way DRF's parsers do for these views: an empty
/// body is empty data; otherwise JSON (malformed → 400 `{"detail": ...}`);
/// a non-object JSON value answers 400 `non_field_errors`.
fn read_object_body(body: Bytes) -> Result<serde_json::Map<String, Value>, HandlerError> {
    if body.is_empty() {
        return Ok(serde_json::Map::new());
    }
    let value: Value = serde_json::from_slice(&body)
        .map_err(|e| HandlerError::BadDetail(format!("JSON parse error - {e}")))?;
    match value {
        Value::Object(map) => Ok(map),
        other => Err(HandlerError::FieldErrors(
            serde_json::to_string(&ser::non_field_errors(&format!(
                "Invalid data. Expected a dictionary, but got {}.",
                drf_type_name(&other)
            )))
            .expect("error body serializes"),
        )),
    }
}

/// Python `str()` over a raw JSON scalar: the ORM filters receive the raw
/// request value, and psycopg3 adapts numbers so Postgres infers `varchar`
/// and casts implicitly — the filter behaves exactly as if the value were
/// stringified (probed: numeric re-post 409s with the holder id, numeric
/// external values filter as their string form, never a 500). Null is safe
/// (`IS NULL`); arrays/objects never reach the filters (field validation
/// 400s first); bools are rejected by the `CharField` shape like DRF.
fn py_str_of(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(true) => Some("True".to_owned()),
        Value::Bool(false) => Some("False".to_owned()),
        Value::Null => Some("None".to_owned()),
        _ => None,
    }
}

/// Python truthiness of a raw input value (`request.data.get(...)` guards).
fn py_truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => {
            if let Some(i) = n.as_i64() {
                i != 0
            } else if let Some(u) = n.as_u64() {
                u != 0
            } else {
                n.as_f64().is_some_and(|f| f != 0.0)
            }
        }
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(o)) => !o.is_empty(),
    }
}

/// Validated state write fields (every `StateSerializer` writable field).
#[derive(Debug, Default, Clone)]
struct StateInput {
    name: Option<String>,
    description: Option<String>,
    color: Option<String>,
    sequence: Option<f64>,
    group: Option<String>,
    is_triage: Option<bool>,
    default: Option<bool>,
    external_source: Option<Option<String>>,
    external_id: Option<Option<String>>,
}

/// One field error, in field order at render time.
type FieldErrorList = Vec<(String, Vec<String>)>;

/// DRF `CharField` (no `allow_blank`, no `allow_null`): required-ness and
/// blank/null/length checks (`serializers/state.py` name/color). `required` is
/// false for partial (PATCH) updates, where absent fields are untouched.
fn check_required_str(
    errors: &mut FieldErrorList,
    field: &str,
    required: bool,
    value: Option<&Value>,
    max_length: usize,
) -> Option<String> {
    let Some(value) = value else {
        if required {
            errors.push((field.to_owned(), vec!["This field is required.".to_owned()]));
        }
        return None;
    };
    match value {
        Value::Null => {
            errors.push((
                field.to_owned(),
                vec!["This field may not be null.".to_owned()],
            ));
            None
        }
        // DRF `CharField` strips surrounding whitespace (`trim_whitespace`)
        // before the blank/length checks; the stored value is the stripped one
        // (probed: `"Padded X "` stores `"Padded X"`, `"   "` blanks).
        Value::String(s) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                errors.push((
                    field.to_owned(),
                    vec!["This field may not be blank.".to_owned()],
                ));
                return None;
            }
            if trimmed.chars().count() > max_length {
                errors.push((
                    field.to_owned(),
                    vec![format!(
                        "Ensure this field has no more than {max_length} characters."
                    )],
                ));
                return None;
            }
            Some(trimmed.to_owned())
        }
        Value::Number(n) => {
            let coerced = n.to_string();
            if coerced.chars().count() > max_length {
                errors.push((
                    field.to_owned(),
                    vec![format!(
                        "Ensure this field has no more than {max_length} characters."
                    )],
                ));
                return None;
            }
            Some(coerced)
        }
        _ => {
            errors.push((field.to_owned(), vec!["Not a valid string.".to_owned()]));
            None
        }
    }
}

/// DRF optional text (`blank=True`, no `null`): missing → `None` (the model
/// default `""` applies at save); blank allowed; null rejected.
fn check_optional_text(
    errors: &mut FieldErrorList,
    field: &str,
    value: Option<&Value>,
    max_length: Option<usize>,
) -> Option<Option<String>> {
    let value = value?;
    match value {
        Value::Null => {
            errors.push((
                field.to_owned(),
                vec!["This field may not be null.".to_owned()],
            ));
            None
        }
        // Stripped like every `CharField` (probed: `"  hi  "` stores `"hi"`).
        Value::String(s) => {
            let trimmed = s.trim();
            if let Some(max) = max_length {
                if trimmed.chars().count() > max {
                    errors.push((
                        field.to_owned(),
                        vec![format!(
                            "Ensure this field has no more than {max} characters."
                        )],
                    ));
                    return None;
                }
            }
            Some(Some(trimmed.to_owned()))
        }
        Value::Number(n) => Some(Some(n.to_string())),
        _ => {
            errors.push((field.to_owned(), vec!["Not a valid string.".to_owned()]));
            None
        }
    }
}

/// DRF optional text with `null=True, blank=True` (`external_*`): missing →
/// `None` (stays null); explicit null → `None`; blank allowed; stored stripped
/// (probed: `"  E9  "` stores `"E9"`).
fn check_nullable_text(
    errors: &mut FieldErrorList,
    field: &str,
    value: Option<&Value>,
) -> Option<Option<String>> {
    let value = value?;
    match value {
        Value::Null => Some(None),
        Value::String(s) => {
            let trimmed = s.trim();
            if trimmed.chars().count() > 255 {
                errors.push((
                    field.to_owned(),
                    vec!["Ensure this field has no more than 255 characters.".to_owned()],
                ));
                return None;
            }
            Some(Some(trimmed.to_owned()))
        }
        Value::Number(n) => Some(Some(n.to_string())),
        _ => {
            errors.push((field.to_owned(), vec!["Not a valid string.".to_owned()]));
            None
        }
    }
}

/// DRF `FloatField` (`sequence`): missing → `None` (the `65535` default applies
/// at save); `"A valid number is required."` otherwise.
fn check_optional_float(
    errors: &mut FieldErrorList,
    field: &str,
    value: Option<&Value>,
) -> Option<Option<f64>> {
    let value = value?;
    match value {
        Value::Null => {
            errors.push((
                field.to_owned(),
                vec!["This field may not be null.".to_owned()],
            ));
            None
        }
        Value::Number(n) => match n.as_f64() {
            Some(f) => Some(Some(f)),
            None => {
                errors.push((
                    field.to_owned(),
                    vec!["A valid number is required.".to_owned()],
                ));
                None
            }
        },
        Value::String(s) => match s.trim().parse::<f64>() {
            Ok(f) => Some(Some(f)),
            Err(_) => {
                errors.push((
                    field.to_owned(),
                    vec!["A valid number is required.".to_owned()],
                ));
                None
            }
        },
        Value::Bool(true) => Some(Some(1.0)),
        Value::Bool(false) => Some(Some(0.0)),
        _ => {
            errors.push((
                field.to_owned(),
                vec!["A valid number is required.".to_owned()],
            ));
            None
        }
    }
}

/// DRF `ChoiceField` (`group`, `serializers/state.py` over `StateGroup.choices`):
/// missing → `None` (the `backlog` default applies); anything outside the seven
/// values (plus triage, which passes here and is rejected by `validate()`) fails.
fn check_optional_group(
    errors: &mut FieldErrorList,
    field: &str,
    value: Option<&Value>,
) -> Option<Option<String>> {
    let value = value?;
    if matches!(value, Value::Null) {
        errors.push((
            field.to_owned(),
            vec!["This field may not be null.".to_owned()],
        ));
        return None;
    }
    let text = match value {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        _ => {
            errors.push((
                field.to_owned(),
                vec![format!(
                    "\"{}\" is not a valid choice.",
                    drf_choice_input(value)
                )],
            ));
            return None;
        }
    };
    if state_model::StateGroup::ALL.contains(&text.as_str()) {
        Some(Some(text))
    } else {
        errors.push((
            field.to_owned(),
            vec![format!("\"{text}\" is not a valid choice.")],
        ));
        None
    }
}

/// Python `str()` of a list/dict input for the `ChoiceField`
/// `"..." is not a valid choice.` message (`ChoiceField.to_internal_value`
/// fails with `input=data`, formatted via `str()` — probed:
/// `"['backlog']' is not a valid choice."`). Only arrays/objects reach
/// here; every scalar is stringified before the choice check.
fn drf_choice_input(value: &Value) -> String {
    py_repr_of(value)
}

/// Python `repr()` over a JSON value (single-quoted strings, `True` /
/// `False` / `None`, `[...]` / `{...}` nesting), for error messages that
/// echo the raw input.
fn py_repr_of(value: &Value) -> String {
    match value {
        Value::Null => "None".to_owned(),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'")),
        Value::Array(items) => {
            let inner = items.iter().map(py_repr_of).collect::<Vec<_>>().join(", ");
            format!("[{inner}]")
        }
        Value::Object(map) => {
            let inner = map
                .iter()
                .map(|(key, item)| {
                    format!(
                        "{}: {}",
                        py_repr_of(&Value::String(key.clone())),
                        py_repr_of(item)
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!("{{{inner}}}")
        }
    }
}

/// DRF `BooleanField` (`is_triage`, `default`): missing → `None` (the `False`
/// default applies); the usual truthy/falsy spellings coerce.
fn check_optional_bool(
    errors: &mut FieldErrorList,
    field: &str,
    value: Option<&Value>,
) -> Option<Option<bool>> {
    const TRUTHY: &[&str] = &[
        "t", "T", "y", "Y", "yes", "Yes", "YES", "true", "True", "TRUE", "on", "On", "ON", "1",
    ];
    const FALSY: &[&str] = &[
        "f", "F", "n", "N", "no", "No", "NO", "false", "False", "FALSE", "off", "Off", "OFF", "0",
    ];
    let value = value?;
    match value {
        Value::Null => {
            errors.push((
                field.to_owned(),
                vec!["This field may not be null.".to_owned()],
            ));
            None
        }
        Value::Bool(b) => Some(Some(*b)),
        Value::Number(n) => {
            // DRF `BooleanField` tests membership with `in` (i.e. `==`), so
            // `1.0 == 1` and `0.0 == 0` coerce (probed: `{"default": 1.0}`
            // arms the flip in Python).
            match n.as_f64() {
                Some(1.0) => Some(Some(true)),
                Some(0.0) => Some(Some(false)),
                _ => {
                    errors.push((
                        field.to_owned(),
                        vec!["Must be a valid boolean.".to_owned()],
                    ));
                    None
                }
            }
        }
        Value::String(s) => {
            if TRUTHY.contains(&s.as_str()) {
                Some(Some(true))
            } else if FALSY.contains(&s.as_str()) {
                Some(Some(false))
            } else {
                errors.push((
                    field.to_owned(),
                    vec!["Must be a valid boolean.".to_owned()],
                ));
                None
            }
        }
        _ => {
            errors.push((
                field.to_owned(),
                vec!["Must be a valid boolean.".to_owned()],
            ));
            None
        }
    }
}

/// Validate one state write body. `partial` mirrors DRF partial updates (PATCH):
/// only supplied writable fields validate; required-ness is skipped. Unknown and
/// read-only keys are silently dropped, exactly like `ModelSerializer`.
fn validate_state_input(
    body: &serde_json::Map<String, Value>,
    partial: bool,
) -> Result<StateInput, FieldErrorList> {
    const READ_ONLY: &[&str] = &[
        "id",
        "created_by",
        "updated_by",
        "created_at",
        "updated_at",
        "workspace",
        "project",
        "deleted_at",
        "slug",
    ];
    let get = |field: &str| {
        let value = body.get(field).filter(|_| !READ_ONLY.contains(&field));
        (
            body.contains_key(field) && !READ_ONLY.contains(&field),
            value,
        )
    };
    let mut errors: FieldErrorList = Vec::new();
    let (_, name_raw) = get("name");
    let (_, color_raw) = get("color");
    let name = check_required_str(&mut errors, "name", !partial, name_raw, 255);
    let color = check_required_str(&mut errors, "color", !partial, color_raw, 255);
    let description =
        check_optional_text(&mut errors, "description", body.get("description"), None).flatten();
    let sequence = check_optional_float(&mut errors, "sequence", body.get("sequence")).flatten();
    let group = check_optional_group(&mut errors, "group", body.get("group")).flatten();
    let is_triage = check_optional_bool(&mut errors, "is_triage", body.get("is_triage")).flatten();
    let default = check_optional_bool(&mut errors, "default", body.get("default")).flatten();
    let external_source =
        check_nullable_text(&mut errors, "external_source", body.get("external_source"));
    let external_id = check_nullable_text(&mut errors, "external_id", body.get("external_id"));
    if errors.is_empty() {
        Ok(StateInput {
            name,
            description,
            color,
            sequence,
            group,
            is_triage,
            default,
            external_source,
            external_id,
        })
    } else {
        // Error order is serializer field order (DRF `errors` dict).
        let order = |name: &str| STATE_FIELDS.iter().position(|f| *f == name).unwrap_or(99);
        errors.sort_by_key(|(name, _)| order(name));
        Err(errors)
    }
}

/// Render a field-error list as the compact `{"field": ["msg"]}` body.
fn field_errors_body(errors: &FieldErrorList) -> String {
    let mut map = serde_json::Map::with_capacity(errors.len());
    for (field, messages) in errors {
        map.insert(
            field.clone(),
            Value::Array(messages.iter().map(|m| Value::String(m.clone())).collect()),
        );
    }
    serde_json::to_string(&Value::Object(map)).expect("error body serializes")
}

/// Map a paginator-kernel error to its HTTP fate (same split as the space
/// handlers' `page_denial`: 400-class variants are `ParseError` details, the
/// rest are uncaught-Python-exception 500s).
fn page_denial(error: paginator::PageError) -> HandlerError {
    use paginator::PageError as E;
    match error {
        E::InvalidCursor
        | E::InvalidPerPage
        | E::PerPageTooLarge(_)
        | E::OffsetTooLarge
        | E::NegativeOffset => HandlerError::BadDetail(error.detail()),
        E::NegativeSlice | E::ZeroLimit | E::NonFiniteCursor | E::MissingOrderKey => {
            HandlerError::ServerError
        }
    }
}

// ---------------------------------------------------------------------------
// Shared request prelude (auth → rewrite → gate)
// ---------------------------------------------------------------------------

/// Authenticate, rewrite the project kwarg (for authenticated callers only),
/// and enforce the state gate — in `BaseAPIView.initial` order — and only
/// then activate the rendering timezone (`TimezoneMixin.initial` runs after
/// `super().initial()`, so denials win over an unknown stored zone).
async fn authorize(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    project_raw: &str,
    route: V1Route,
    method: &str,
) -> Result<(sqlx::PgPool, Gate), HandlerError> {
    let pool = pool_of(state)?.clone();
    let identity = authenticate(state, headers).await?;
    let project_id = match &identity {
        Some(_) => resolve_project_id(&pool, slug, project_raw).await?,
        None => Uuid::nil(),
    };
    let actor_id = resolve_state_gate(
        &pool,
        slug,
        project_id,
        identity.as_ref().map(|identity| identity.id),
        route,
        method,
    )
    .await?;
    let timezone = activate_timezone(
        identity
            .as_ref()
            .and_then(|identity| identity.timezone.as_deref()),
    )?;
    let gate = Gate {
        actor: Actor {
            id: actor_id,
            timezone,
        },
        project_id,
    };
    Ok((pool, gate))
}

// ---------------------------------------------------------------------------
// GET states/ — `StateListCreateAPIEndpoint.get` (`views/state.py:149-159`)
// ---------------------------------------------------------------------------

/// List states: `paginate(queryset, on_results=StateSerializer(many, fields,
/// expand))`. Ordering is the model's `Meta.ordering` (`sequence`).
async fn list_states(
    AxumState(state): AxumState<AppState>,
    Path((slug, project_raw)): Path<(String, String)>,
    Query(query): Query<QueryMap>,
    headers: HeaderMap,
) -> Response {
    match list_states_inner(&state, &headers, &slug, &project_raw, &query).await {
        Ok(response) => response,
        Err(error) => error.into_response(),
    }
}

async fn list_states_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    project_raw: &str,
    query: &QueryMap,
) -> Result<Response, HandlerError> {
    let (pool, gate) =
        authorize(state, headers, slug, project_raw, V1Route::StateList, "GET").await?;
    let fields = csv_param(query_last(query, "fields"));
    let expand = csv_param(query_last(query, "expand"));
    let per_page = paginator::parse_per_page(query_last(query, "per_page").as_deref(), 1000, 1000)
        .map_err(page_denial)?;
    let limit = paginator::clamp_limit(per_page, 1000);
    let cursor_raw = query_last(query, "cursor").unwrap_or_else(|| format!("{per_page}:0:0"));
    let cursor = Cursor::from_string(&cursor_raw).map_err(page_denial)?;

    let mut rows = state_q::fetch_state_list(&pool, slug, gate.project_id, gate.actor.id)
        .await
        .map_err(|_| HandlerError::ServerError)?;
    // `Meta.ordering = ("sequence",)`: ascending, nulls last (never null here).
    rows.sort_by(|a, b| {
        a.sequence
            .partial_cmp(&b.sequence)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let total = rows.len() as i64;
    let window = paginator::offset_window(limit, cursor.offset, cursor.value, cursor.is_prev, None)
        .map_err(page_denial)?;
    let start = (window.offset as usize).min(rows.len());
    let end = (window.stop as usize).min(rows.len());
    let has_more = (end.saturating_sub(start) as i64) > limit;
    let page_rows =
        paginator::apply_offset_window(&rows[start..end], limit).map_err(page_denial)?;
    let next = paginator::next_cursor(limit, window.page, has_more);
    let prev = paginator::prev_cursor(limit, window.page);
    let total_pages = paginator::max_hits(total, limit).map_err(page_denial)?;

    let mut rendered = Vec::with_capacity(page_rows.len());
    for row in &page_rows {
        rendered.push(
            render_state(
                &pool,
                row,
                &gate.actor.timezone,
                fields.as_deref(),
                expand.as_deref(),
            )
            .await?,
        );
    }
    let page = PageResponse {
        grouped_by: None,
        sub_grouped_by: None,
        total_count: total,
        next_cursor: next.to_string(),
        prev_cursor: prev.to_string(),
        next_page_results: next.has_results_or_false(),
        prev_page_results: prev.has_results_or_false(),
        count: rendered.len(),
        total_pages,
        total_results: total,
        extra_stats: None,
        results: Value::Array(rendered),
    };
    Ok(json_ok(
        serde_json::to_string(&page).expect("envelope serializes"),
    ))
}

// ---------------------------------------------------------------------------
// POST states/ — `StateListCreateAPIEndpoint.post` (`views/state.py:80-128`)
// ---------------------------------------------------------------------------

/// Create a state: field validation, then `StateSerializer.validate` (the
/// default-flip side effect runs before the triage rejection), then the
/// external-id 409 check, then `save(project_id=...)` — answering 200, not 201.
/// `IntegrityError` maps to the name-clash 409 (whose holder miss 500s).
async fn create_state(
    AxumState(state): AxumState<AppState>,
    Path((slug, project_raw)): Path<(String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    match create_state_inner(&state, &headers, &slug, &project_raw, body).await {
        Ok(response) => response,
        Err(error) => error.into_response(),
    }
}

async fn create_state_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    project_raw: &str,
    body: Bytes,
) -> Result<Response, HandlerError> {
    let (pool, gate) = authorize(
        state,
        headers,
        slug,
        project_raw,
        V1Route::StateList,
        "POST",
    )
    .await?;
    let raw = read_object_body(body)?;
    // Field validation first: DRF never calls `validate()` after field errors,
    // so the default-flip stays dormant on this path.
    let input = validate_state_input(&raw, false)
        .map_err(|errors| HandlerError::FieldErrors(field_errors_body(&errors)))?;
    // `StateSerializer.validate`: the flip flag is computed first because the
    // UPDATE runs before the triage check (BUG-4a).
    let group = input
        .group
        .clone()
        .unwrap_or_else(|| state_model::DEFAULT_GROUP.to_owned());
    let (flip_armed, validation_error) = ser::state_validate(input.default, Some(group.as_str()));
    if flip_armed {
        run_default_flip(&pool, gate.project_id).await?;
    }
    if let Some(error_body) = validation_error {
        return Err(HandlerError::FieldErrors(
            serde_json::to_string(&error_body).expect("error body serializes"),
        ));
    }
    // External-id pre-check (`views/state.py:89-111`).
    if py_truthy(raw.get("external_id")) && py_truthy(raw.get("external_source")) {
        let external_id = raw
            .get("external_id")
            .and_then(py_str_of)
            .unwrap_or_default();
        let external_source = raw
            .get("external_source")
            .and_then(py_str_of)
            .unwrap_or_default();
        if let Some(existing) =
            fetch_state_by_external(&pool, slug, gate.project_id, &external_source, &external_id)
                .await?
        {
            return Err(HandlerError::Conflict(state_conflict_body(
                "State with the same external id and external source already exists",
                &existing.id.to_string(),
            )));
        }
    }
    // `serializer.save(project_id=...)`: slugify, the add-time sequence rule,
    // workspace backfill from the project; `created_by` stamps the actor
    // (`BaseModel.save` via crum), `updated_by` stays null.
    let name = input.name.clone().expect("name validated");
    let color = input.color.clone().expect("color validated");
    let now = micros_now();
    let workspace_id = fetch_project_workspace(&pool, gate.project_id).await?;
    let sequence = match input.sequence {
        Some(caller) => match max_sibling_sequence(&pool, gate.project_id).await? {
            Some(max) => state_model::sequence_on_add(Some(max)).expect("max present"),
            None => caller,
        },
        None => match max_sibling_sequence(&pool, gate.project_id).await? {
            Some(max) => state_model::sequence_on_add(Some(max)).expect("max present"),
            None => state_model::DEFAULT_SEQUENCE,
        },
    };
    let id = Uuid::new_v4();
    let slug_value = state_model::slugify_name(&name);
    let description = input.description.clone().unwrap_or_default();
    let is_triage = input.is_triage.unwrap_or(state_model::DEFAULT_IS_TRIAGE);
    let default = input.default.unwrap_or(state_model::DEFAULT_IS_DEFAULT);
    // Each external field saves independently (missing or explicit null → null).
    let external_source = input.external_source.clone().flatten();
    let external_id = input.external_id.clone().flatten();
    let insert = sqlx::query(
        r#"INSERT INTO states
           (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at,
            project_id, workspace_id, name, description, color, slug, sequence,
            "group", is_triage, "default", external_source, external_id)
           VALUES ($1,$2,$3,$4,$5,NULL,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17)"#,
    )
    .bind(id)
    .bind(now)
    .bind(now)
    .bind(gate.actor.id)
    .bind(Option::<Uuid>::None)
    .bind(gate.project_id)
    .bind(workspace_id)
    .bind(&name)
    .bind(&description)
    .bind(&color)
    .bind(&slug_value)
    .bind(sequence)
    .bind(&group)
    .bind(is_triage)
    .bind(default)
    .bind(&external_source)
    .bind(&external_id);
    match insert.execute(&pool).await {
        Ok(_) => {}
        Err(e) if is_unique_violation(&e) => {
            // Name clash: echo the surviving row's id; a lookup miss 500s
            // (Python `AttributeError` on `None.id` → generic branch). The lookup
            // uses the RAW input (`request.data.get("name")`, unstripped): a padded
            // re-post collides at the DB but misses here → 500 (probed). A
            // numeric raw name filters as its string form (psycopg3
            // adaptation, probed: re-post 409s with the holder id).
            let raw_name = raw.get("name").and_then(py_str_of).unwrap_or_default();
            let holder = fetch_state_by_name(&pool, slug, gate.project_id, &raw_name).await?;
            let Some(holder) = holder else {
                return Err(HandlerError::ServerError);
            };
            return Err(HandlerError::Conflict(state_conflict_body(
                "State with the same name already exists in the project",
                &holder.id.to_string(),
            )));
        }
        Err(_) => return Err(HandlerError::ServerError),
    }
    let created = state_model::State {
        id,
        created_at: now,
        updated_at: now,
        created_by_id: Some(gate.actor.id),
        updated_by_id: None,
        deleted_at: None,
        project_id: gate.project_id,
        workspace_id,
        name,
        description,
        color,
        slug: slug_value,
        sequence,
        group,
        is_triage,
        default,
        external_source,
        external_id,
    };
    let rendered = render_state(&pool, &created, &gate.actor.timezone, None, None).await?;
    Ok(json_ok(
        serde_json::to_string(&rendered).expect("state serializes"),
    ))
}

/// `timezone.now()` at microsecond precision (Django datetimes carry micros;
/// Postgres `timestamptz` stores micros, so the rendered value must be the
/// stored value, not a nanosecond reading).
fn micros_now() -> chrono::DateTime<chrono::Utc> {
    let now = chrono::Utc::now();
    chrono::DateTime::from_timestamp_micros(now.timestamp_micros()).expect("valid micros")
}

/// The default-flip side effect (`serializers/state.py:21-22`): clear `default`
/// on every sibling in the project (soft-deleted and triage rows keep theirs,
/// via the default-manager scope in [`ser::STATE_DEFAULT_FLIP_SQL`]).
async fn run_default_flip(pool: &sqlx::PgPool, project_id: Uuid) -> Result<(), HandlerError> {
    sqlx::query(ser::STATE_DEFAULT_FLIP_SQL)
        .bind(project_id)
        .execute(pool)
        .await
        .map_err(|_| HandlerError::ServerError)?;
    Ok(())
}

/// Project workspace backfill (`ProjectBaseModel.save`, `db/models/project.py:309-311`).
/// A missing project row surfaces as a foreign-key failure at INSERT (→ 500 via
/// the 409-lookup miss), exactly like Python.
async fn fetch_project_workspace(
    pool: &sqlx::PgPool,
    project_id: Uuid,
) -> Result<Uuid, HandlerError> {
    let row: Option<(Uuid,)> = sqlx::query_as(r#"SELECT workspace_id FROM projects WHERE id = $1"#)
        .bind(project_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| HandlerError::ServerError)?;
    // `None` is preserved as a NULL workspace bind would violate NOT NULL; the
    // INSERT below fails the FK/requirement instead — but a UUID is required, so
    // carry a sentinel that fails the same way. In practice the FK fails first.
    row.map(|row| row.0).ok_or(HandlerError::ServerError)
}

/// Max sibling sequence over the default-manager scope (soft-deleted and triage
/// rows excluded): `State.objects.filter(project=...).aggregate(Max("sequence"))`
/// (`db/models/state.py:133-138`).
async fn max_sibling_sequence(
    pool: &sqlx::PgPool,
    project_id: Uuid,
) -> Result<Option<f64>, HandlerError> {
    let row: Option<(Option<f64>,)> = sqlx::query_as(
        r#"SELECT MAX(sequence) FROM states
           WHERE project_id = $1 AND deleted_at IS NULL AND NOT ("group" = 'triage')"#,
    )
    .bind(project_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| HandlerError::ServerError)?;
    Ok(row.and_then(|row| row.0))
}

/// The create-time external-id lookup (`views/state.py:92-104`): `State.objects`
/// scope (soft-deleted and triage rows excluded) with no scope on the joined
/// workspace, exactly like the Django filter.
async fn fetch_state_by_external(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: Uuid,
    external_source: &str,
    external_id: &str,
) -> Result<Option<state_model::State>, HandlerError> {
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(
        r#"SELECT states.* FROM states
           JOIN workspaces ON workspaces.id = states.workspace_id
           WHERE states.project_id = $1 AND workspaces.slug = $2
           AND states.external_source = $3 AND states.external_id = $4
           AND states.deleted_at IS NULL AND NOT (states."group" = 'triage')
           LIMIT 1"#,
    )
    .bind(project_id)
    .bind(slug)
    .bind(external_source)
    .bind(external_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| HandlerError::ServerError)?;
    row.map(|row| state_q::map_state_row(&row).map_err(|_| HandlerError::ServerError))
        .transpose()
}

/// The 409 name-holder lookup (`views/state.py:117-121`): same `State.objects`
/// scope as above.
async fn fetch_state_by_name(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: Uuid,
    name: &str,
) -> Result<Option<state_model::State>, HandlerError> {
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(
        r#"SELECT states.* FROM states
           JOIN workspaces ON workspaces.id = states.workspace_id
           WHERE workspaces.slug = $1 AND states.project_id = $2 AND states.name = $3
           AND states.deleted_at IS NULL AND NOT (states."group" = 'triage')
           LIMIT 1"#,
    )
    .bind(slug)
    .bind(project_id)
    .bind(name)
    .fetch_optional(pool)
    .await
    .map_err(|_| HandlerError::ServerError)?;
    row.map(|row| state_q::map_state_row(&row).map_err(|_| HandlerError::ServerError))
        .transpose()
}

fn state_conflict_body(error: &str, id: &str) -> String {
    serde_json::to_string(&serde_json::json!({"error": error, "id": id}))
        .expect("conflict body serializes")
}

/// True for unique/partial-unique violations (SQLSTATE 23505): the only
/// `IntegrityError` the name-constraint INSERT can raise deterministically.
fn is_unique_violation(e: &sqlx::Error) -> bool {
    e.as_database_error()
        .and_then(|db| db.code())
        .is_some_and(|code| code == "23505")
}

// ---------------------------------------------------------------------------
// GET states/<state_id>/ — `StateDetailAPIEndpoint.get` (`views/state.py:200-211`)
// ---------------------------------------------------------------------------

/// Retrieve one state through the list scope (member visibility + archived
/// guard); a miss answers the `ObjectDoesNotExist` 404.
async fn retrieve_state(
    AxumState(state): AxumState<AppState>,
    Path((slug, project_raw, state_id)): Path<(String, String, String)>,
    Query(query): Query<QueryMap>,
    headers: HeaderMap,
) -> Response {
    match retrieve_state_inner(&state, &headers, &slug, &project_raw, &state_id, &query).await {
        Ok(response) => response,
        Err(error) => error.into_response(),
    }
}

async fn retrieve_state_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    project_raw: &str,
    state_id_raw: &str,
    query: &QueryMap,
) -> Result<Response, HandlerError> {
    let (pool, gate) = authorize(
        state,
        headers,
        slug,
        project_raw,
        V1Route::StateDetail,
        "GET",
    )
    .await?;
    let state_id = parse_state_id(state_id_raw)?;
    let fields = csv_param(query_last(query, "fields"));
    let expand = csv_param(query_last(query, "expand"));
    let row = state_q::fetch_state_detail(&pool, slug, gate.project_id, gate.actor.id, state_id)
        .await
        .map_err(|_| HandlerError::ServerError)?
        .ok_or(HandlerError::NotFound)?;
    let rendered = render_state(
        &pool,
        &row,
        &gate.actor.timezone,
        fields.as_deref(),
        expand.as_deref(),
    )
    .await?;
    Ok(json_ok(
        serde_json::to_string(&rendered).expect("state serializes"),
    ))
}

/// Parse the `<uuid:state_id>` converter value. Django answers a resolver 404
/// (HTML) for non-UUIDs; the JSON 404 is the documented assumption here.
fn parse_state_id(raw: &str) -> Result<Uuid, HandlerError> {
    raw.parse::<Uuid>().map_err(|_| HandlerError::NotFound)
}

// ---------------------------------------------------------------------------
// PATCH states/<state_id>/ — `StateDetailAPIEndpoint.patch` (`views/state.py:272-300`)
// ---------------------------------------------------------------------------

/// Partial update: partial field validation, then `validate()` (flip runs, triage
/// rejects), then the external-id clash check — which echoes the TARGET row's id
/// on conflict (ported bug) — then `save()`, whose unique violation answers 400
/// `{"error": "The payload is not valid"}`.
async fn patch_state(
    AxumState(state): AxumState<AppState>,
    Path((slug, project_raw, state_id)): Path<(String, String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    match patch_state_inner(&state, &headers, &slug, &project_raw, &state_id, body).await {
        Ok(response) => response,
        Err(error) => error.into_response(),
    }
}

async fn patch_state_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    project_raw: &str,
    state_id_raw: &str,
    body: Bytes,
) -> Result<Response, HandlerError> {
    let (pool, gate) = authorize(
        state,
        headers,
        slug,
        project_raw,
        V1Route::StateDetail,
        "PATCH",
    )
    .await?;
    let state_id = parse_state_id(state_id_raw)?;
    // Direct get (`views/state.py:278`): default-manager scope but NO
    // archived-project guard and NO `is_triage` filter (only DELETE has one) —
    // the PATCH-specific scope, not the shared direct scope.
    let mut row = state_q::fetch_state_direct_for_patch(&pool, slug, gate.project_id, state_id)
        .await
        .map_err(|_| HandlerError::ServerError)?
        .ok_or(HandlerError::NotFound)?;
    let raw = read_object_body(body)?;
    let input = validate_state_input(&raw, true)
        .map_err(|errors| HandlerError::FieldErrors(field_errors_body(&errors)))?;
    // Partial `validate()`: the triage check sees the supplied group (or
    // nothing). The default-flip NEVER runs here: PATCH builds the serializer
    // with no context (`views/state.py:279`), so `filter(project_id=None)`
    // matches zero rows — a no-op in Python (`serializers/state.py:22`).
    let (_, validation_error) = ser::state_validate(input.default, input.group.as_deref());
    if let Some(error_body) = validation_error {
        return Err(HandlerError::FieldErrors(
            serde_json::to_string(&error_body).expect("error body serializes"),
        ));
    }
    // External-id clash (`views/state.py:281-297`): only when a new external_id
    // is supplied AND differs from the row's (`str()`-compared), AND a row with
    // the (possibly defaulted) source + id exists. The 409 echoes the TARGET id.
    if py_truthy(raw.get("external_id")) {
        let supplied_raw = raw.get("external_id").expect("truthy external_id");
        let current = row.external_id.clone().unwrap_or_else(|| "None".to_owned());
        let supplied = py_str_of(supplied_raw).unwrap_or_default();
        if current != supplied {
            // `request.data.get("external_source", state.external_source)`:
            // absent → the row's own (possibly NULL → `IS NULL`); explicit
            // null → NULL (`IS NULL`); the columns are nullable.
            let source: Option<String> = match raw.get("external_source") {
                None => row.external_source.clone(),
                Some(Value::Null) => None,
                Some(value) => py_str_of(value),
            };
            if state_external_exists(&pool, slug, gate.project_id, source.as_deref(), &supplied)
                .await?
            {
                return Err(HandlerError::Conflict(state_conflict_body(
                    "State with the same external id and external source already exists",
                    &row.id.to_string(),
                )));
            }
        }
    }
    // Apply the patch: supplied fields only; `save()` recomputes the slug and
    // stamps `updated_by` (crum) + `updated_at` (auto_now).
    if let Some(name) = input.name {
        row.name = name.clone();
        row.slug = state_model::slugify_name(&name);
    }
    if let Some(description) = input.description {
        row.description = description;
    }
    if let Some(color) = input.color {
        row.color = color;
    }
    if let Some(sequence) = input.sequence {
        row.sequence = sequence;
    }
    if let Some(group) = input.group {
        row.group = group;
    }
    if let Some(is_triage) = input.is_triage {
        row.is_triage = is_triage;
    }
    if let Some(default) = input.default {
        row.default = default;
    }
    if let Some(external_source) = input.external_source {
        row.external_source = external_source;
    }
    if let Some(external_id) = input.external_id {
        row.external_id = external_id;
    }
    row.updated_at = micros_now();
    row.updated_by_id = Some(gate.actor.id);
    let update = sqlx::query(
        r#"UPDATE states SET updated_at = $1, updated_by_id = $2, name = $3,
                  description = $4, color = $5, slug = $6, sequence = $7,
                  "group" = $8, is_triage = $9, "default" = $10,
                  external_source = $11, external_id = $12
           WHERE id = $13"#,
    )
    .bind(row.updated_at)
    .bind(row.updated_by_id)
    .bind(&row.name)
    .bind(&row.description)
    .bind(&row.color)
    .bind(&row.slug)
    .bind(row.sequence)
    .bind(&row.group)
    .bind(row.is_triage)
    .bind(row.default)
    .bind(&row.external_source)
    .bind(&row.external_id)
    .bind(row.id);
    match update.execute(&pool).await {
        Ok(_) => {}
        // Uncaught `IntegrityError` → `handle_exception` 400 branch.
        Err(e) if is_unique_violation(&e) => return Err(HandlerError::InvalidPayload),
        Err(_) => return Err(HandlerError::ServerError),
    }
    let rendered = render_state(&pool, &row, &gate.actor.timezone, None, None).await?;
    Ok(json_ok(
        serde_json::to_string(&rendered).expect("state serializes"),
    ))
}

/// The PATCH-time external clash probe (`views/state.py:284-289`): `State.objects`
/// scope (soft-deleted and triage-group rows excluded). The source is
/// `request.data.get("external_source", state.external_source)` — a NULL
/// source (absent key on a NULL-source row, or explicit null) filters as
/// `IS NULL`, exactly like Django's `filter(external_source=None)`.
async fn state_external_exists(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: Uuid,
    external_source: Option<&str>,
    external_id: &str,
) -> Result<bool, HandlerError> {
    // The source predicate shape is fixed per call (`IS NULL` vs `=`), so
    // the statement is built once per shape — no parameter juggling.
    let sql = match external_source {
        None => {
            r#"SELECT states.id FROM states
               JOIN workspaces ON workspaces.id = states.workspace_id
               WHERE states.project_id = $1 AND workspaces.slug = $2
               AND states.external_source IS NULL AND states.external_id = $3
               AND states.deleted_at IS NULL AND NOT (states."group" = 'triage')
               LIMIT 1"#
        }
        Some(_) => {
            r#"SELECT states.id FROM states
               JOIN workspaces ON workspaces.id = states.workspace_id
               WHERE states.project_id = $1 AND workspaces.slug = $2
               AND states.external_source = $3 AND states.external_id = $4
               AND states.deleted_at IS NULL AND NOT (states."group" = 'triage')
               LIMIT 1"#
        }
    };
    let mut query = sqlx::query_as::<_, (Uuid,)>(sql)
        .bind(project_id)
        .bind(slug);
    query = match external_source {
        None => query.bind(external_id),
        Some(source) => query.bind(source).bind(external_id),
    };
    let hit: Option<(Uuid,)> = query
        .fetch_optional(pool)
        .await
        .map_err(|_| HandlerError::ServerError)?;
    Ok(hit.is_some())
}

// ---------------------------------------------------------------------------
// DELETE states/<state_id>/ — `StateDetailAPIEndpoint.delete` (`views/state.py:225-249`)
// ---------------------------------------------------------------------------

/// Delete a state: default states and non-empty states (any non-soft-deleted
/// issue points at them — `Issue.objects`, soft-delete scope only) answer 400;
/// otherwise the row is soft-deleted and the response is an empty 204. The
/// deferred related-object sweep is a documented no-op (nothing references an
/// empty state); `updated_by` stamps the actor like `BaseModel.save` does.
async fn delete_state(
    AxumState(state): AxumState<AppState>,
    Path((slug, project_raw, state_id)): Path<(String, String, String)>,
    headers: HeaderMap,
) -> Response {
    match delete_state_inner(&state, &headers, &slug, &project_raw, &state_id).await {
        Ok(response) => response,
        Err(error) => error.into_response(),
    }
}

async fn delete_state_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    project_raw: &str,
    state_id_raw: &str,
) -> Result<Response, HandlerError> {
    let (pool, gate) = authorize(
        state,
        headers,
        slug,
        project_raw,
        V1Route::StateDetail,
        "DELETE",
    )
    .await?;
    let state_id = parse_state_id(state_id_raw)?;
    let row = state_q::fetch_state_direct(&pool, slug, gate.project_id, state_id)
        .await
        .map_err(|_| HandlerError::ServerError)?
        .ok_or(HandlerError::NotFound)?;
    if row.default {
        return Err(HandlerError::BadError(
            "Default state cannot be deleted".to_owned(),
        ));
    }
    let used: Option<(Uuid,)> = sqlx::query_as(
        r#"SELECT id FROM issues WHERE state_id = $1 AND deleted_at IS NULL LIMIT 1"#,
    )
    .bind(state_id)
    .fetch_optional(&pool)
    .await
    .map_err(|_| HandlerError::ServerError)?;
    if used.is_some() {
        return Err(HandlerError::BadError(
            "The state is not empty, only empty states can be deleted".to_owned(),
        ));
    }
    let now = micros_now();
    sqlx::query(r#"UPDATE states SET deleted_at = $1, updated_by_id = $2 WHERE id = $3"#)
        .bind(now)
        .bind(gate.actor.id)
        .bind(state_id)
        .execute(&pool)
        .await
        .map_err(|_| HandlerError::ServerError)?;
    Ok(Response::builder()
        .status(StatusCode::NO_CONTENT)
        .body(axum::body::Body::empty())
        .expect("204 response"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn denial_bodies_are_byte_identical() {
        assert_eq!(
            UNAUTHENTICATED_BODY,
            r#"{"detail":"Authentication credentials were not provided."}"#
        );
        assert_eq!(
            INVALID_TOKEN_BODY,
            r#"{"detail":"Given API token is not valid"}"#
        );
        assert_eq!(
            NOT_FOUND_BODY,
            r#"{"error":"The requested resource does not exist."}"#
        );
        assert_eq!(
            INVALID_PAYLOAD_BODY,
            r#"{"error":"The payload is not valid"}"#
        );
        assert_eq!(
            SERVER_ERROR_BODY,
            r#"{"error":"Something went wrong please try again later"}"#
        );
    }

    #[test]
    fn state_routes_carry_the_state_entity_gate() {
        // Every owned method on both state paths maps to ProjectStateEntityPermission.
        for method in ["GET", "POST", "PATCH", "DELETE", "HEAD", "OPTIONS"] {
            assert_eq!(
                gate_for(V1Route::StateList, method),
                V1ProjectsGate::ProjectStateEntity,
                "{method}"
            );
            assert_eq!(
                gate_for(V1Route::StateDetail, method),
                V1ProjectsGate::ProjectStateEntity,
                "{method}"
            );
        }
    }

    #[test]
    fn conflict_bodies_carry_error_plus_id_in_order() {
        let body = state_conflict_body(
            "State with the same name already exists in the project",
            "8a38a0e2-1111-4222-8333-444455556666",
        );
        assert_eq!(
            body,
            r#"{"error":"State with the same name already exists in the project","id":"8a38a0e2-1111-4222-8333-444455556666"}"#
        );
    }

    #[test]
    fn triage_create_rejected_with_flip_armed() {
        // BUG-4a: the flip flag is set even though validation fails — the caller
        // must run the UPDATE before answering 400.
        let (flip, error) = ser::state_validate(Some(false), Some("triage"));
        assert!(!flip);
        assert_eq!(
            error.expect("triage rejected"),
            serde_json::json!({"non_field_errors": ["Cannot create triage state"]})
        );
        let (flip, error) = ser::state_validate(Some(true), Some("triage"));
        assert!(flip);
        assert!(error.is_some());
    }

    #[test]
    fn create_requires_name_and_color() {
        let body = serde_json::Map::new();
        let errors = validate_state_input(&body, false).expect_err("missing required");
        let rendered = field_errors_body(&errors);
        assert_eq!(
            rendered,
            r#"{"name":["This field is required."],"color":["This field is required."]}"#
        );
    }

    #[test]
    fn patch_validates_only_supplied_fields() {
        let body: serde_json::Map<String, Value> =
            serde_json::from_value(serde_json::json!({"color": "#0000ff"})).expect("map");
        let input = validate_state_input(&body, true).expect("valid partial");
        assert_eq!(input.color.as_deref(), Some("#0000ff"));
        assert_eq!(input.name, None);
        assert_eq!(input.description, None);
    }

    #[test]
    fn read_only_and_unknown_keys_are_dropped() {
        let body: serde_json::Map<String, Value> = serde_json::from_value(serde_json::json!({
            "name": "N",
            "color": "#fff",
            "id": "8a38a0e2-1111-4222-8333-444455556666",
            "slug": "forged",
            "bogus": 1,
        }))
        .expect("map");
        let input = validate_state_input(&body, false).expect("valid");
        assert_eq!(input.name.as_deref(), Some("N"));
    }

    #[test]
    fn invalid_group_choice_message() {
        let body: serde_json::Map<String, Value> =
            serde_json::from_value(serde_json::json!({"group": "nope"})).expect("map");
        let errors = validate_state_input(&body, true).expect_err("bad choice");
        assert_eq!(
            field_errors_body(&errors),
            r#"{"group":["\"nope\" is not a valid choice."]}"#
        );
    }

    #[test]
    fn raw_truthiness_matches_python_guards() {
        assert!(!py_truthy(None));
        assert!(!py_truthy(Some(&Value::Null)));
        assert!(!py_truthy(Some(&serde_json::json!(""))));
        assert!(py_truthy(Some(&serde_json::json!("GH"))));
        assert!(!py_truthy(Some(&serde_json::json!(0))));
        assert!(py_truthy(Some(&serde_json::json!(1))));
        assert!(!py_truthy(Some(&serde_json::json!(false))));
    }

    #[test]
    fn invalid_state_id_is_a_404() {
        assert!(matches!(
            parse_state_id("not-a-uuid"),
            Err(HandlerError::NotFound)
        ));
    }

    #[test]
    fn state_wire_field_order_is_drf_order() {
        // Probed live order: id, created_at, updated_at, deleted_at, then the
        // plain model fields, then the four FKs last.
        assert_eq!(
            STATE_FIELDS,
            &[
                "id",
                "created_at",
                "updated_at",
                "deleted_at",
                "name",
                "description",
                "color",
                "slug",
                "sequence",
                "group",
                "is_triage",
                "default",
                "external_source",
                "external_id",
                "created_by",
                "updated_by",
                "project",
                "workspace",
            ]
        );
    }

    #[test]
    fn char_fields_store_trimmed() {
        // Probed: `"Padded X "` stores `"Padded X"`; `"   "` blanks.
        let body: serde_json::Map<String, Value> = serde_json::from_value(serde_json::json!({
            "name": "  Padded  ",
            "color": "#fff",
            "description": "  hi  ",
        }))
        .expect("map");
        let input = validate_state_input(&body, false).expect("valid");
        assert_eq!(input.name.as_deref(), Some("Padded"));
        assert_eq!(input.description.as_deref(), Some("hi"));
        let body: serde_json::Map<String, Value> =
            serde_json::from_value(serde_json::json!({"name": "   ", "color": "#fff"}))
                .expect("map");
        let errors = validate_state_input(&body, false).expect_err("blank");
        assert_eq!(
            field_errors_body(&errors),
            r#"{"name":["This field may not be blank."]}"#
        );
    }

    #[test]
    fn boolean_field_coerces_unit_floats() {
        // DRF `BooleanField` tests `in` (i.e. `==`): `1.0 == 1` and
        // `0.0 == 0` coerce; anything else 400s.
        let mut errors = FieldErrorList::new();
        assert_eq!(
            check_optional_bool(&mut errors, "default", Some(&serde_json::json!(1.0))),
            Some(Some(true))
        );
        assert_eq!(
            check_optional_bool(&mut errors, "default", Some(&serde_json::json!(0.0))),
            Some(Some(false))
        );
        assert!(errors.is_empty());
        assert_eq!(
            check_optional_bool(&mut errors, "default", Some(&serde_json::json!(1.5))),
            None
        );
        assert_eq!(
            field_errors_body(&errors),
            r#"{"default":["Must be a valid boolean."]}"#
        );
    }

    #[test]
    fn choice_message_echoes_python_repr() {
        // Probed: `"['backlog']' is not a valid choice."` — `str(input)`,
        // not the JSON rendering.
        assert_eq!(
            drf_choice_input(&serde_json::json!(["backlog"])),
            "['backlog']"
        );
        assert_eq!(drf_choice_input(&serde_json::json!([])), "[]");
        assert_eq!(drf_choice_input(&serde_json::json!({"a": 1})), "{'a': 1}");
        let body: serde_json::Map<String, Value> =
            serde_json::from_value(serde_json::json!({"group": ["backlog"]})).expect("map");
        let errors = validate_state_input(&body, true).expect_err("bad choice");
        assert_eq!(
            field_errors_body(&errors),
            r#"{"group":["\"['backlog']\" is not a valid choice."]}"#
        );
    }

    #[test]
    fn method_not_allowed_body_is_drf_bytes() {
        assert_eq!(
            method_not_allowed_body("HEAD"),
            r#"{"detail":"Method \"HEAD\" not allowed."}"#
        );
        assert_eq!(
            method_not_allowed_body("TRACE"),
            r#"{"detail":"Method \"TRACE\" not allowed."}"#
        );
    }

    #[test]
    fn raw_scalars_filter_as_their_string_form() {
        // psycopg3 adapts numbers so the ORM filter casts implicitly
        // (probed live: numeric re-post 409s with the holder id) — the
        // coercion below is the behavior, not a shortcut.
        assert_eq!(py_str_of(&serde_json::json!(5)).as_deref(), Some("5"));
        assert_eq!(py_str_of(&serde_json::json!(5.0)).as_deref(), Some("5.0"));
        assert_eq!(py_str_of(&serde_json::json!("5")).as_deref(), Some("5"));
    }

    #[test]
    fn timezone_parses_after_the_gate() {
        // Missing zone defaults to UTC; unknown zones fail (500 for
        // survivors, never ahead of a denial — ordering lives in
        // `authorize`, this only pins the parse itself).
        assert!(activate_timezone(None).is_ok());
        assert!(activate_timezone(Some("UTC")).is_ok());
        assert!(matches!(
            activate_timezone(Some("Not/AZone")),
            Err(HandlerError::ServerError)
        ));
    }
}
