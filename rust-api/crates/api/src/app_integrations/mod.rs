//! D-33 app-integrations handlers (stage 5).
//!
//! [`gates`] ports the `allow_permission` role matrix, the two `AllowAny`
//! endpoints, and the manual admin checks (PIDASHCONV-436); [`hmac`] ports
//! the `verify_webhook_signature` HMAC-SHA256 guard. Both modules are pure:
//! handlers fetch membership rows through the workspace-scoped handle and
//! decide here.
//!
//! [`handlers_external`] owns the external-integration HTTP shell
//! (PIDASHCONV-454): the two AI-assistant POSTs and the Unsplash GET.
//! [`handlers_github_proj`] owns the project-level GitHub HTTP shell
//! (PIDASHCONV-450): bind POST plus status GET/PATCH/DELETE. Sibling
//! handler issues merge their own routers into [`routes`]; merges keep
//! both sides.
//!
//! [`handlers_webhook`] serves the webhook family (PIDASHCONV-453); sibling
//! handler issues own their files and share this module's plumbing
//! (mirrors the D-32 `app_intake` layout):
//!
//! - [`Denial`]: exact error bodies (`app/views/base.py` matrix, DRF
//!   `NotAuthenticated` default, the allow-style 403).
//! - [`owned`]: cutover granularity — owned methods serve from Rust, the
//!   rest proxy so Django's 405-after-auth responses survive byte for byte.
//! - [`actor`] / [`pool_of`]: Django-session auth + pool access.
//! - [`require_workspace_admin`]: the `@allow_permission([ADMIN],
//!   level="WORKSPACE")` decorator through the [`gates`] table.
//!
//! Sibling handler issues extend [`routes`] with their own routers; merges
//! keep both sides.

pub mod gates;
pub mod handlers_external;
pub mod handlers_git_accounts;
pub mod handlers_git_repo;
pub mod handlers_github_proj;
pub mod handlers_github_ws;
pub mod handlers_webhook;
pub mod hmac;

use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Router;
use chrono_tz::Tz;
use sqlx::PgPool;
use uuid::Uuid;

use pidash_auth::permissions::allow::AllowFacts;
use pidash_types::WorkspaceId;

use crate::state::AppState;

/// Owned D-33 app-integration routes (cutover granularity: registered
/// paths serve from Rust, everything else keeps proxying). Sibling
/// handler issues extend this merge; merges keep both sides.
pub fn routes() -> Router<AppState> {
    handlers_external::routes()
        .merge(handlers_git_accounts::routes())
        .merge(handlers_github_proj::routes())
        .merge(handlers_github_ws::routes())
        .merge(handlers_git_repo::routes())
        .merge(handlers_webhook::routes())
}

/// Exact bytes of the DRF `IsAuthenticated` denial: anonymous on a guarded
/// route (`request.successful_authenticator` is `None`).
pub const UNAUTHENTICATED_BODY: &str =
    r#"{"detail":"Authentication credentials were not provided."}"#;
/// `handle_exception`'s `ObjectDoesNotExist` branch
/// (`app/views/base.py`).
pub const NOT_FOUND_BODY: &str = r#"{"error":"The required object does not exist."}"#;
/// `handle_exception`'s generic 500 branch.
pub const SERVER_ERROR_BODY: &str = r#"{"error":"Something went wrong please try again later"}"#;
/// `handle_exception`'s `IntegrityError` branch.
pub const INVALID_PAYLOAD_BODY: &str = r#"{"error":"The payload is not valid"}"#;
/// POST duplicate URL (`app/views/webhook/base.py:31-35`).
pub const URL_CONFLICT_BODY: &str = r#"{"error":"URL already exists for the workspace"}"#;

/// Handler failure with its exact status + body.
#[derive(Debug)]
pub enum Denial {
    /// 401, DRF `NotAuthenticated` (anonymous on a guarded route).
    Unauthorized,
    /// 403, `@allow_permission` body.
    Forbidden,
    /// 404, `ObjectDoesNotExist` branch.
    NotFound,
    /// 409, duplicate webhook URL for the workspace.
    Conflict,
    /// 400, pre-rendered JSON body (serializer `errors`, guard bodies,
    /// parse errors).
    BadJson(serde_json::Value),
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
            Denial::NotFound => (StatusCode::NOT_FOUND, NOT_FOUND_BODY.to_owned()),
            Denial::Conflict => (StatusCode::CONFLICT, URL_CONFLICT_BODY.to_owned()),
            Denial::BadJson(body) => (
                StatusCode::BAD_REQUEST,
                serde_json::to_string(body).expect("serializable denial"),
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

pub fn json_string(value: &str) -> String {
    serde_json::to_string(value).expect("json string")
}

/// Render a `serde_json::Map` built in serializer field order as compact
/// JSON (`JSONRenderer` default: no spaces; `preserve_order` keeps the
/// insertion order DRF emits).
pub fn json_response(status: StatusCode, body: String) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body))
        .expect("json response")
}

/// Authenticated actor plus time zone (`TimezoneMixin.initial` activates
/// the user's zone; datetimes render in it).
pub struct Actor {
    pub id: Uuid,
    pub timezone: Tz,
}

/// `request.user` through Django-session auth.
///
/// `BaseSessionAuthentication` + `IsAuthenticated`
/// (`app/views/base.py`): anonymous answers the DRF `NotAuthenticated`
/// body before anything else runs.
pub async fn actor(
    state: &AppState,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Result<Actor, Denial> {
    let pool = pool_of(state)?;
    let resolved =
        crate::license::resolve_actor(pool, state.settings().secret_key.as_bytes(), extension)
            .await
            .map_err(|_| Denial::ServerError)?
            .ok_or(Denial::Unauthorized)?;
    Ok(Actor {
        id: resolved.id,
        timezone: resolved.timezone,
    })
}

pub fn pool_of(state: &AppState) -> Result<&PgPool, Denial> {
    state
        .pools()
        .map(|pools| pools.primary())
        .ok_or(Denial::ServerError)
}

/// Parse a UUID path segment: Django's `<uuid:>` converter 404s on
/// garbage, so unparseable ids behave as missing rows (the D-02 `space`
/// `parse_id` precedent).
#[allow(clippy::result_large_err)]
pub fn parse_id(raw: &str) -> Result<Uuid, Response> {
    raw.parse::<Uuid>()
        .map_err(|_| Denial::NotFound.into_response())
}

/// An app path: the owned methods serve from Rust, everything else falls
/// through to Django (its 405-after-auth and metadata responses live
/// there). `HEAD` rides axum's `get` handling like Django's `GET`-backed
/// `HEAD`.
pub fn owned(
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

/// The `@allow_permission(allowed_roles=[ADMIN], level="WORKSPACE")`
/// decorator (`app/permissions/base.py:44-51`) driven through the
/// [`gates`] table: the handler fetches the caller's workspace role (the
/// `tenant_context` half of the pilot pattern) and decides here. Unknown
/// slugs deny: the membership `exists()` finds no row, exactly like the
/// decorator, so the body's `Workspace.objects.get` 404 only fires for
/// callers who pass the gate.
pub async fn require_workspace_admin(
    pool: &PgPool,
    slug: &str,
    user_id: &Uuid,
    method: &str,
    path: &str,
) -> Result<(), Denial> {
    let membership: Option<(i16,)> = sqlx::query_as(
        r#"SELECT wm.role FROM workspace_members wm
           JOIN workspaces w ON w.id = wm.workspace_id
           WHERE wm.member_id = $1 AND w.slug = $2
           AND wm.is_active AND wm.deleted_at IS NULL
           AND w.deleted_at IS NULL"#,
    )
    .bind(user_id)
    .bind(slug)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let gate = gates::gate_for(method, path).ok_or(Denial::ServerError)?;
    let facts = AllowFacts {
        workspace: WorkspaceId::from(slug),
        authenticated: true,
        is_workspace_member: membership.is_some(),
        has_allowed_workspace_role: matches!(membership, Some((20,))),
        is_creator: false,
        has_allowed_project_role: false,
        is_project_member: false,
        is_workspace_admin: matches!(membership, Some((20,))),
    };
    match gates::decide_gate(&gate.gate, &gates::tenant_context(slug), &facts) {
        gates::GateOutcome::Allow => Ok(()),
        gates::GateOutcome::Deny | gates::GateOutcome::Unauthenticated => Err(Denial::Forbidden),
    }
}

/// `Workspace.objects.get(slug=slug)` (`webhook/base.py:23`): the live
/// row's id, or the 404 branch when the slug names nothing.
pub async fn workspace_id_or_404(pool: &PgPool, slug: &str) -> Result<Uuid, Denial> {
    pidash_db::app_integrations::fetch_workspace_id_by_slug(pool, slug)
        .await
        .map_err(|_| Denial::ServerError)?
        .ok_or(Denial::NotFound)
}
