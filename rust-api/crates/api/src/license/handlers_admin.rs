#![forbid(unsafe_code)]

//! Instance-admin console handlers (D-01, PIDASHCONV-121).
//!
//! Ports the JSON units of `apps/api/pi_dash/license/api/views/admin.py`
//! (`InstanceAdminEndpoint` POST/GET/DELETE at `admin.py:49-86`,
//! `InstanceAdminUserMeEndpoint` at `:361-366`,
//! `InstanceAdminUserSessionEndpoint` at `:369-379`, and
//! `InstanceAdminSignOutEndpoint` at `:382-398`):
//!
//! - `POST /api/instances/admins/` — admin permission; missing email 400
//!   `{"error":"Email is required"}`; no instance 403; unknown user 404 via
//!   the `ObjectDoesNotExist` branch; creates the row and answers 201 with
//!   the `InstanceAdminSerializer` rendering.
//! - `GET /api/instances/admins/` — admin permission; no instance 403;
//!   the `InstanceAdminSerializer` list ordered `-created_at`.
//! - `DELETE /api/instances/admins/<uuid:pk>/` — admin permission; soft
//!   UPDATEs `deleted_at` on `filter(instance, pk)` with no existence
//!   check, so unknown PKs still answer 204 (ported quirk). A pk spelling
//!   the `<uuid:pk>` converter does not route (uppercase, simple, braced,
//!   `urn:uuid:`) resolver-404s before auth, so those segments proxy to
//!   Django (PIDASHCONV-1132).
//! - `GET /api/instances/admins/me/` — admin permission; the
//!   `InstanceAdminMeSerializer` rendering of the acting user.
//! - `GET /api/instances/admins/session/` — `AllowAny`; authed admins get
//!   `{"is_authenticated":true,"user":{…}}`, everyone else
//!   `{"is_authenticated":false}`.
//! - `POST /api/instances/admins/sign-out/` — plain view (no DRF
//!   permission layer): stamps `last_logout_ip/time`, flushes the session,
//!   and 302s to the safe admin URL; ANY exception still 302s.
//! - `POST /api/instances/admins/sign-up/` + `POST .../sign-in/` — plain
//!   form views (`admin.py:89-358`): branch evaluation comes from the
//!   [`handlers_auth_forms`](super::handlers_auth_forms) kernel, this
//!   module adds the HTTP shell (form parsing, snapshots, row writes,
//!   session issuance, 302s).
//!
//! The three plain-view POSTs sit behind `CsrfViewMiddleware` (unlike the
//! DRF units, which use the CSRF-exempt session auth): a missing cookie
//! or token answers the 200 `csrf_failure` page, never the view. The gate
//! mirrors `auth_session/email.rs` and renders the shared
//! [`crate::auth_session::render::render_csrf_failure`] page.
//!
//! Layering: this module owns the HTTP shell (routes, session auth, the
//! `InstanceAdminPermission` gate, SQL text, row fetching). Output shapes
//! come from `pidash_types::license::serializers_core`, datetimes render
//! through the F-07 kernel in the acting user's zone (`TimezoneMixin`).
//!
//! Ported bugs and deliberate deviations (also listed in the PR):
//! - the 2h `cache_response` on GET plus `invalidate_cache` on POST/DELETE
//!   are not reproduced: they are freshness-only (the invalidation keeps
//!   every contract-visible read consistent), so Rust always reads fresh.
//! - DELETE publishes no `soft_delete_related_objects` task because Python
//!   publishes none either: queryset `.delete()` is a bare
//!   `update(deleted_at=…)` (`db/mixins.py`), and the `.delay` only fires
//!   in the instance `delete()` path, which this endpoint never takes.
//! - fixture BUG-2 ("unknown-user POST is Django 500") is NOT ported:
//!   DRF's `dispatch` calls `self.handle_exception` inside its own try, so
//!   a handler-body `DoesNotExist` answers the 404 branch before
//!   `BaseAPIView.dispatch` (which drops responses) is ever reached; the
//!   contract suite pins 404 and is green against Django.
//! - key order of serializer objects follows the Rust view declaration,
//!   not DRF's `user_detail`-first order; the suites compare parsed JSON.
//! - POST 201 echoes the raw request `role` (`ChoiceField` over the
//!   still-raw in-memory attribute: only `"20"` maps to `20`, floats,
//!   bools and padded strings echo verbatim), not the stored `i32`
//!   (PIDASHCONV-1129).

use axum::extract::{ConnectInfo, Extension, Request, State};
use axum::http::{header, HeaderMap, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::Router;
use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use hmac::Hmac;
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use std::net::SocketAddr;
use uuid::Uuid;

use pidash_auth::csrf as csrf_kernel;
use pidash_auth::session as session_kernel;
use pidash_auth::signing::{Signer, SESSION_SIGNING_SALT};
use pidash_types::license::serializers_core::{
    instance_admin_me_to_representation, instance_admin_to_representation, AdminMeRow,
    InstanceAdminRow, LiteUserRow,
};

use super::form_body::{
    parse_form_body, parse_multipart_body, select_body_kind, BodyFail, BodyField, BodyKind,
    ParsedBody,
};
use super::handlers_auth_forms as forms;
use super::{file_asset_url, AssetUrl};
use crate::auth_session::render::render_csrf_failure;
use crate::edge;
use crate::middleware::SessionHandle;
use crate::paginator::py_float_str;
use crate::serializer::render_datetime_in;
use crate::state::AppState;
use crate::uuid_path::is_canonical_uuid;
use crate::v1_cycles_modules::json_cpython::{self, JsonFail};

/// Register the seven owned endpoints. Cutover granularity is the
/// route + method (pilot `owned()` pattern): sibling methods on these
/// paths keep proxying so Django's own 405s/permission denials are
/// preserved byte-for-byte with no per-method logic. Every route carries
/// an explicit `.head(edge::proxy)` arm: Django's `View.setup()` aliases
/// `head=get`, so HEAD runs the GET handler server-side (200, empty
/// body), and the proxy preserves those exact bytes instead of axum's
/// auto-HEAD over the Rust handler.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/instances/admins/",
            get(list_admins)
                .post(create_admin)
                .put(edge::proxy)
                .patch(edge::proxy)
                .delete(edge::proxy)
                .head(edge::proxy)
                .options(edge::proxy),
        )
        .route(
            "/api/instances/admins/{pk}/",
            delete(delete_admin)
                .get(edge::proxy)
                .post(edge::proxy)
                .put(edge::proxy)
                .patch(edge::proxy)
                .head(edge::proxy)
                .options(edge::proxy),
        )
        .route(
            "/api/instances/admins/me/",
            get(me)
                .post(edge::proxy)
                .put(edge::proxy)
                .patch(edge::proxy)
                .delete(edge::proxy)
                .head(edge::proxy)
                .options(edge::proxy),
        )
        .route(
            "/api/instances/admins/session/",
            get(session)
                .post(edge::proxy)
                .put(edge::proxy)
                .patch(edge::proxy)
                .delete(edge::proxy)
                .head(edge::proxy)
                .options(edge::proxy),
        )
        .route(
            "/api/instances/admins/sign-out/",
            post(sign_out)
                .get(edge::proxy)
                .head(edge::proxy)
                .put(edge::proxy)
                .patch(edge::proxy)
                .delete(edge::proxy)
                .options(edge::proxy),
        )
        .route(
            "/api/instances/admins/sign-in/",
            post(sign_in)
                .get(edge::proxy)
                .head(edge::proxy)
                .put(edge::proxy)
                .patch(edge::proxy)
                .delete(edge::proxy)
                .options(edge::proxy),
        )
        .route(
            "/api/instances/admins/sign-up/",
            post(sign_up)
                .get(edge::proxy)
                .head(edge::proxy)
                .put(edge::proxy)
                .patch(edge::proxy)
                .delete(edge::proxy)
                .options(edge::proxy),
        )
}

/// Handler failure with its exact status + body (`base.py:58-95`
/// `handle_exception` matrix plus the DRF auth/permission denials).
#[derive(Debug)]
enum Denial {
    /// 401, DRF `NotAuthenticated` (session missing or unknown).
    Unauthorized,
    /// 403, DRF-default permission denial (`InstanceAdminPermission`
    /// has no `message`, so `permission_denied` raises with the default
    /// detail: `test_permissions.py` pins this body).
    DefaultForbidden,
    /// 403, view-inline "Instance is not registered yet" (reachable only
    /// when the permission gate passed without an instance row, which the
    /// gate itself prevents — kept so the order matches Python).
    NoInstance,
    /// 404, `ObjectDoesNotExist` branch (unknown user on POST).
    NotFound,
    /// 400, `{"error": …}` (view-inline: email required).
    BadError(String),
    /// 400, `{"detail": …}` (DRF `ParseError`, e.g. malformed JSON body).
    BadDetail(String),
    /// 415, unsupported media type (DRF `UnsupportedMediaType` — PIDASHCONV-1102).
    UnsupportedMediaType(String),
    /// 400, `IntegrityError` branch (duplicate admin, null role).
    InvalidPayload,
    /// 500, generic branch.
    ServerError,
}

impl Denial {
    fn status_and_body(&self) -> (StatusCode, String) {
        match self {
            Denial::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                crate::app_issues::UNAUTHENTICATED_BODY.to_owned(),
            ),
            Denial::DefaultForbidden => (
                StatusCode::FORBIDDEN,
                crate::permissions::DEFAULT_DENIED_BODY.to_owned(),
            ),
            Denial::NoInstance => (
                StatusCode::FORBIDDEN,
                format!(
                    "{{\"error\":{}}}",
                    json_string("Instance is not registered yet")
                ),
            ),
            Denial::NotFound => (
                StatusCode::NOT_FOUND,
                crate::app_issues::NOT_FOUND_BODY.to_owned(),
            ),
            Denial::BadError(message) => (
                StatusCode::BAD_REQUEST,
                format!("{{\"error\":{}}}", json_string(message)),
            ),
            Denial::BadDetail(message) => (
                StatusCode::BAD_REQUEST,
                format!("{{\"detail\":{}}}", json_string(message)),
            ),
            Denial::UnsupportedMediaType(message) => (
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                format!("{{\"detail\":{}}}", json_string(message)),
            ),
            Denial::InvalidPayload => (
                StatusCode::BAD_REQUEST,
                format!("{{\"error\":{}}}", json_string("The payload is not valid")),
            ),
            Denial::ServerError => (
                StatusCode::INTERNAL_SERVER_ERROR,
                crate::app_issues::SERVER_ERROR_BODY.to_owned(),
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

/// `request.user` from the Django session. Mirrors
/// `django.contrib.auth.get_user` plus DRF `SessionAuthentication`: the
/// session must carry a UUID `_auth_user_id` for the `ModelBackend` with
/// a matching `_auth_user_hash`, and the user row must exist and be
/// active — the same three facts [`super::resolve_actor`] verifies
/// (PIDASHCONV-1097). Anything else authenticates as nobody. Inactive
/// counts as nobody even on the plain-View sign-out:
/// `ModelBackend.get_user` rejects `is_active=False` before the hash
/// check, so `get_user` yields anonymous there too.
async fn verified_actor(
    pool: &PgPool,
    secret_key: &[u8],
    extension: &Option<Extension<SessionHandle>>,
) -> Result<Option<Uuid>, Denial> {
    let Some(handle) = extension.as_ref().map(|ext| ext.0.clone()) else {
        return Ok(None);
    };
    let mut session = handle.snapshot();
    let backend = session
        .get("_auth_user_backend")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if backend != super::MODEL_BACKEND {
        return Ok(None);
    }
    let Some(id) = session
        .get("_auth_user_id")
        .and_then(|v| v.as_str())
        .and_then(|raw| raw.parse::<Uuid>().ok())
    else {
        return Ok(None);
    };
    let session_hash = session
        .get("_auth_user_hash")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_owned();
    let row: Option<(String, bool)> =
        sqlx::query_as("SELECT password, is_active FROM users WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    let Some((password, true)) = row else {
        return Ok(None);
    };
    if session_hash.is_empty() || !super::verify_session_hash(&session_hash, &password, secret_key)
    {
        return Ok(None);
    }
    Ok(Some(id))
}

/// Authenticated actor or 401. A dangling session (user row gone), a
/// stale hash, a wrong backend, or an inactive user all authenticate as
/// nobody, which the permission layer turns into `NotAuthenticated` — so
/// every one of them is 401 here, not 403.
async fn authed_user(
    pool: &PgPool,
    secret_key: &[u8],
    extension: &Option<Extension<SessionHandle>>,
) -> Result<Uuid, Denial> {
    verified_actor(pool, secret_key, extension)
        .await?
        .ok_or(Denial::Unauthorized)
}

/// `Instance.objects.first()`: `Meta.ordering = ("-created_at",)` over the
/// soft-deletion manager (`queries/instance_first`; every view here uses it).
async fn instance_first(pool: &PgPool) -> Result<Option<Uuid>, Denial> {
    let row: Option<(Uuid,)> = sqlx::query_as(
        "SELECT id FROM instances WHERE deleted_at IS NULL ORDER BY created_at DESC LIMIT 1",
    )
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(row.map(|row| row.0))
}

/// `InstanceAdminPermission.has_permission` (`permissions/instance.py`):
/// anonymous is denied by the caller (401); otherwise the instance's first
/// row plus an `InstanceAdmin` with `role__gte=15` for
/// `(instance, user)` must exist. The `15` is a latent wider gate
/// (`ROLE_CHOICES` only defines 20) — kept exactly (PIDASHCONV-118 owns
/// the shared guard; this is the same SQL inline until it lands).
async fn require_instance_admin(pool: &PgPool, user_id: &Uuid) -> Result<Uuid, Denial> {
    let Some(instance_id) = instance_first(pool).await? else {
        return Err(Denial::DefaultForbidden);
    };
    let row: Option<(i32,)> = sqlx::query_as(
        "SELECT 1 FROM instance_admins WHERE instance_id = $1 AND user_id = $2 AND role >= 15 AND deleted_at IS NULL LIMIT 1",
    )
    .bind(instance_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    if row.is_none() {
        return Err(Denial::DefaultForbidden);
    }
    Ok(instance_id)
}

/// The acting user's zone (`TimezoneMixin.initial`: authenticated →
/// `timezone.activate(ZoneInfo(user_timezone))`). A stored zone that does
/// not parse fails the request; Django raises out of `initial` into the
/// generic 500 branch.
async fn actor_timezone(pool: &PgPool, user_id: &Uuid) -> Result<Tz, Denial> {
    let row: Option<(String,)> = sqlx::query_as("SELECT user_timezone FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let (zone,) = row.ok_or(Denial::ServerError)?;
    zone.parse::<Tz>().map_err(|_| Denial::ServerError)
}

fn pools(state: &AppState) -> Result<&PgPool, Denial> {
    state
        .pools()
        .map(|pools| pools.primary())
        .ok_or(Denial::ServerError)
}

/// `User.avatar_url` (`db/models/user.py:143-151`): the `avatar_asset`
/// row resolved *unscoped* — the forward-FK descriptor reads through
/// `_base_manager`, so a soft-deleted asset still renders — feeds
/// [`file_asset_url`]; only a NULL FK falls back to the raw `avatar` string
/// (non-empty) or null. A dangling FK (row gone; FK-unreachable in practice)
/// raises `RelatedObjectDoesNotExist`, which DRF's `get_attribute` catches
/// as `ObjectDoesNotExist` and renders as null.
#[allow(clippy::too_many_arguments)]
fn avatar_url(
    asset_id: Option<Uuid>,
    asset_row: Option<Uuid>,
    asset_entity: Option<String>,
    asset_workspace: Option<Uuid>,
    asset_project: Option<Uuid>,
    asset_issue: Option<Uuid>,
    asset_workspace_slug: Option<String>,
    avatar: &str,
) -> AssetUrl {
    if asset_id.is_none() {
        if avatar.is_empty() {
            return AssetUrl::Present(None);
        }
        return AssetUrl::Present(Some(avatar.to_owned()));
    }
    let Some(row) = asset_row else {
        return AssetUrl::Present(None);
    };
    file_asset_url(
        &row,
        asset_entity.as_deref(),
        asset_workspace.as_ref(),
        asset_project.as_ref(),
        asset_issue.as_ref(),
        asset_workspace_slug.as_deref(),
    )
}

/// Full `users` projection shared by `me`, `session`, and `user_detail`.
/// Every user column rides a LEFT JOIN somewhere, so all read `Option`
/// and the caller decides what a missing row means. `sqlx::FromRow` maps
/// by column name (wide selects exceed the 16-tuple `FromRow` impls), so
/// the SELECT aliases below must match these field names exactly.
#[derive(Debug, Clone, sqlx::FromRow)]
struct UserShape {
    id: Option<Uuid>,
    first_name: Option<String>,
    last_name: Option<String>,
    avatar: Option<String>,
    avatar_asset_id: Option<Uuid>,
    avatar_asset_row: Option<Uuid>,
    avatar_asset_entity: Option<String>,
    avatar_asset_workspace: Option<Uuid>,
    avatar_asset_project: Option<Uuid>,
    avatar_asset_issue: Option<Uuid>,
    avatar_asset_workspace_slug: Option<String>,
    cover_image: Option<String>,
    date_joined: Option<DateTime<Utc>>,
    display_name: Option<String>,
    email: Option<String>,
    is_active: Option<bool>,
    is_bot: Option<bool>,
    is_email_verified: Option<bool>,
    user_timezone: Option<String>,
    username: Option<String>,
    is_password_autoset: Option<bool>,
    last_login_medium: Option<String>,
}

const USER_SHAPE_SELECT: &str = "u.id AS id, u.first_name AS first_name, u.last_name AS last_name, u.avatar AS avatar, u.avatar_asset_id AS avatar_asset_id, fa.id AS avatar_asset_row, fa.entity_type AS avatar_asset_entity, fa.workspace_id AS avatar_asset_workspace, fa.project_id AS avatar_asset_project, fa.issue_id AS avatar_asset_issue, w.slug AS avatar_asset_workspace_slug, u.cover_image AS cover_image, u.date_joined AS date_joined, u.display_name AS display_name, u.email AS email, u.is_active AS is_active, u.is_bot AS is_bot, u.is_email_verified AS is_email_verified, u.user_timezone AS user_timezone, u.username AS username, u.is_password_autoset AS is_password_autoset, u.last_login_medium AS last_login_medium";

async fn fetch_user(pool: &PgPool, user_id: &Uuid) -> Result<Option<UserShape>, Denial> {
    // Both joins are deliberately unscoped: the forward-FK descriptor reads
    // through `_base_manager`, so soft-deleted assets/workspaces resolve.
    let row: Option<UserShape> = sqlx::query_as(&format!(
        "SELECT {USER_SHAPE_SELECT} FROM users u LEFT JOIN file_assets fa ON fa.id = u.avatar_asset_id LEFT JOIN workspaces w ON w.id = fa.workspace_id WHERE u.id = $1"
    ))
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    // A present row always carries its PK; its absence means the caller
    // joined a null FK (`user_detail: null`, `user: null`).
    Ok(row.filter(|shape| shape.id.is_some()))
}

/// Render a `UserShape` through `InstanceAdminMeSerializer`
/// (`serializers/admin.py:12-33`). `None` on a NOT NULL column is corrupt
/// data — Django would 500 rendering it, so this maps to `ServerError`.
fn render_me(user: &UserShape, tz: &Tz) -> Result<Value, Denial> {
    let date_joined = user.date_joined.as_ref().ok_or(Denial::ServerError)?;
    let resolved_avatar_url = avatar_url(
        user.avatar_asset_id,
        user.avatar_asset_row,
        user.avatar_asset_entity.clone(),
        user.avatar_asset_workspace,
        user.avatar_asset_project,
        user.avatar_asset_issue,
        user.avatar_asset_workspace_slug.clone(),
        user.avatar.as_deref().unwrap_or(""),
    );
    let skip_avatar_url = matches!(resolved_avatar_url, AssetUrl::Skip);
    let owned_avatar_url = match resolved_avatar_url {
        AssetUrl::Present(url) => url,
        AssetUrl::Skip => None,
    };
    // Owned strings first: the row borrows them and the view borrows the
    // row, so everything must outlive `to_value` below.
    let owned_id = user.id.as_ref().ok_or(Denial::ServerError)?.to_string();
    let owned_date_joined = render_datetime_in(date_joined, tz);
    let row = AdminMeRow {
        id: &owned_id,
        avatar: user.avatar.as_deref().unwrap_or(""),
        avatar_url: owned_avatar_url.as_deref(),
        cover_image: user.cover_image.as_deref(),
        date_joined: &owned_date_joined,
        display_name: user.display_name.as_deref().unwrap_or(""),
        email: user.email.as_deref().unwrap_or(""),
        first_name: user.first_name.as_deref().unwrap_or(""),
        last_name: user.last_name.as_deref().unwrap_or(""),
        is_active: user.is_active.unwrap_or(false),
        is_bot: user.is_bot.unwrap_or(false),
        is_email_verified: user.is_email_verified.unwrap_or(false),
        user_timezone: user.user_timezone.as_deref().unwrap_or("UTC"),
        username: user.username.as_deref().unwrap_or(""),
        is_password_autoset: user.is_password_autoset.unwrap_or(false),
    };
    let view = instance_admin_me_to_representation(&row);
    let mut body = serde_json::to_value(&view).map_err(|_| Denial::ServerError)?;
    if skip_avatar_url {
        // DRF `SkipField`: the key is omitted, not nulled.
        if let Some(object) = body.as_object_mut() {
            object.remove("avatar_url");
        }
    }
    Ok(body)
}

/// Render one admin row through `InstanceAdminSerializer`
/// (`serializers/admin.py:36-42`, `fields = "__all__"` + nested
/// `user_detail`).
#[allow(clippy::too_many_arguments)]
fn render_admin(
    id: &Uuid,
    created_at: &DateTime<Utc>,
    updated_at: &DateTime<Utc>,
    deleted_at: Option<&DateTime<Utc>>,
    role: i32,
    is_verified: bool,
    created_by: Option<Uuid>,
    updated_by: Option<Uuid>,
    user_id: Option<Uuid>,
    instance_id: &Uuid,
    user: Option<&UserShape>,
    tz: &Tz,
) -> Result<Value, Denial> {
    // Owned strings first: the rows borrow them and the view borrows the
    // rows, so everything must outlive `to_value` below.
    let owned_id = id.to_string();
    let owned_created_at = render_datetime_in(created_at, tz);
    let owned_updated_at = render_datetime_in(updated_at, tz);
    let owned_deleted_at = deleted_at.map(|dt| render_datetime_in(dt, tz));
    let owned_created_by = created_by.map(|id| id.to_string());
    let owned_updated_by = updated_by.map(|id| id.to_string());
    let owned_user_id = user_id.map(|id| id.to_string());
    let owned_instance_id = instance_id.to_string();
    struct LiteOwned {
        id: String,
        avatar_url: Option<String>,
        skip_avatar_url: bool,
    }
    let owned_lite: Option<LiteOwned> = match user {
        Some(shape) => {
            let resolved = avatar_url(
                shape.avatar_asset_id,
                shape.avatar_asset_row,
                shape.avatar_asset_entity.clone(),
                shape.avatar_asset_workspace,
                shape.avatar_asset_project,
                shape.avatar_asset_issue,
                shape.avatar_asset_workspace_slug.clone(),
                shape.avatar.as_deref().unwrap_or(""),
            );
            Some(LiteOwned {
                id: shape.id.as_ref().ok_or(Denial::ServerError)?.to_string(),
                skip_avatar_url: matches!(resolved, AssetUrl::Skip),
                avatar_url: match resolved {
                    AssetUrl::Present(url) => url,
                    AssetUrl::Skip => None,
                },
            })
        }
        None => None,
    };
    let lite = owned_lite
        .as_ref()
        .zip(user)
        .map(|(owned, shape)| LiteUserRow {
            id: &owned.id,
            first_name: shape.first_name.as_deref().unwrap_or(""),
            last_name: shape.last_name.as_deref().unwrap_or(""),
            avatar: shape.avatar.as_deref().unwrap_or(""),
            avatar_url: owned.avatar_url.as_deref(),
            is_bot: shape.is_bot.unwrap_or(false),
            display_name: shape.display_name.as_deref().unwrap_or(""),
            email: shape.email.as_deref().unwrap_or(""),
            last_login_medium: shape.last_login_medium.as_deref(),
        });
    let row = InstanceAdminRow {
        id: &owned_id,
        user: lite,
        created_at: &owned_created_at,
        updated_at: &owned_updated_at,
        deleted_at: owned_deleted_at.as_deref(),
        role,
        is_verified,
        created_by: owned_created_by.as_deref(),
        updated_by: owned_updated_by.as_deref(),
        user_id: owned_user_id.as_deref(),
        instance_id: &owned_instance_id,
    };
    let view = instance_admin_to_representation(&row);
    let mut body = serde_json::to_value(&view).map_err(|_| Denial::ServerError)?;
    if owned_lite
        .as_ref()
        .is_some_and(|owned| owned.skip_avatar_url)
    {
        // DRF `SkipField`: the key is omitted, not nulled.
        if let Some(detail) = body.get_mut("user_detail").and_then(Value::as_object_mut) {
            detail.remove("avatar_url");
        }
    }
    Ok(body)
}

/// Parse a POST body the way DRF `_parse` does: empty body is `{}`
/// without parsing; otherwise the content-type selects the parser in
/// `select_parser` order (JSON, form, multipart —
/// `form_body::select_body_kind`); unmatched types are 415. On the JSON
/// branch the body must be an object (`None.get("email")` /
/// `[1].get("email")` raise `AttributeError` out of the view → generic
/// 500); otherwise it parses through the shared CPython layer
/// (`json_cpython`, PIDASHCONV-1128), so malformed JSON, strict constants,
/// int-limit and codec errors carry CPython's exact text as `ParseError`
/// 400 `{"detail"}`; past the depth cap is Django's JSON 500.
fn parse_json_body(
    headers: &HeaderMap,
    body: &[u8],
    memory_limit: i64,
) -> Result<ParsedBody, Denial> {
    if body.is_empty() {
        return Ok(ParsedBody::json(Value::Object(serde_json::Map::new())));
    }
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    match select_body_kind(content_type) {
        Some(BodyKind::Json) => match json_cpython::parse_request_bytes(body) {
            Ok(jval) => {
                let data = json_cpython::to_serde_publish(&jval);
                if data.is_object() {
                    Ok(ParsedBody::json(data))
                } else {
                    // `None.get("email")` / `[1].get("email")` raise
                    // `AttributeError` out of the view → generic 500.
                    Err(Denial::ServerError)
                }
            }
            Err(JsonFail::Message(detail)) => Err(Denial::BadDetail(format!(
                "{}{detail}",
                json_cpython::JSON_PARSE_PREFIX
            ))),
            Err(JsonFail::Recursion) => Err(Denial::ServerError),
        },
        Some(BodyKind::Form) => match parse_form_body(body) {
            Ok(pairs) => Ok(ParsedBody::form(pairs)),
            // Cap errors escape the view into the base
            // `handle_exception` (generic 500) — the POST-only 400 page
            // seen locally is the Debug Toolbar re-parsing the body in
            // middleware (DEBUG-only; prod has no toolbar).
            Err(_) => Err(Denial::ServerError),
        },
        Some(BodyKind::Multipart) => match parse_multipart_body(content_type, body, memory_limit) {
            Ok(pairs) => Ok(ParsedBody::form(pairs)),
            Err(BodyFail::Parse(message)) => Err(Denial::BadDetail(message)),
            Err(BodyFail::Fatal) | Err(BodyFail::TooMany) => Err(Denial::ServerError),
        },
        None => Err(Denial::UnsupportedMediaType(format!(
            "Unsupported media type \"{content_type}\" in request."
        ))),
    }
}

/// Adapt a JSON `role` value to the integer column the way
/// `request.data.get("role", 20)` + the ORM do (pinned against Django 4.2
/// `IntegerField.get_prep_value`): absent → 20; explicit null hits the NOT
/// NULL column → `IntegrityError` → invalid-payload 400; bools adapt as
/// ints (`True` → 1, `False` → 0 — `bool` subclasses `int`); integers that
/// fit pass through; EVERY float truncates toward zero BEFORE the range
/// check (`int(20.5)` → 20, so no `fract() == 0.0` gate, and
/// `int(2147483647.5)` → `2147483647` fits); numeric strings parse with
/// `int()`
/// semantics (ASCII-whitespace tolerated, underscores between digits,
/// Unicode decimal digits, `"20.5"`/`"abc"` → `ValueError` → generic
/// 500); anything else errors → generic 500. Past-`i32` values 500 at
/// the INSERT (`DataError`, uncaught). A parsed negative still 400s,
/// but at the INSERT: the `PositiveIntegerField` CHECK (`role >= 0`)
/// raises `IntegrityError`. An uploaded file always 500s: `int(file)`
/// is a `TypeError` regardless of the filename (probed) — unlike
/// numeric strings.
///
/// Returns the INSERT int plus the 201 echo: the model field carries
/// `choices=((20, "Admin"),)`, so the serializer renders a
/// `ChoiceField.to_representation` over the RAW input —
/// `choice_strings_to_values.get(str(value), value)` — echoing the
/// input verbatim unless `str(value) == "20"` (probed: `" 20 "`,
/// `"1_0"`, `"٢٠"`, `true`, `20.5` all echo).
fn role_from_value(found: BodyField<'_>) -> Result<(i32, Value), Denial> {
    let value = match found {
        BodyField::File(_) => return Err(Denial::ServerError),
        BodyField::Json(value) => Some(value),
        BodyField::Missing => None,
    };
    match value {
        None => Ok((20, Value::Number(20.into()))),
        Some(Value::Null) => Err(Denial::InvalidPayload),
        Some(Value::Bool(b)) => Ok((i32::from(*b), Value::Bool(*b))),
        Some(Value::Number(n)) => {
            let parsed = role_from_number(n)?;
            // `str(value) == "20"` is exactly the int token `20`
            // (`as_i64` is `None` for floats and out-of-range ints).
            let echo = if n.as_i64() == Some(20) {
                Value::Number(20.into())
            } else {
                Value::Number(n.clone())
            };
            Ok((parsed, echo))
        }
        Some(Value::String(s)) => {
            let parsed = py_int_i32(s).map_err(|_| Denial::ServerError)?;
            let echo = if s == "20" {
                Value::Number(20.into())
            } else {
                Value::String(s.clone())
            };
            Ok((parsed, echo))
        }
        Some(_) => Err(Denial::ServerError),
    }
}

/// CPython `int(s)` for a base-10 string, then an `i32` range check:
/// ASCII-whitespace stripped, one optional sign, digits `0-9` plus
/// Unicode decimal (`Nd`) digits, single underscores allowed only
/// between digits. Anything else (empty, `"20.5"`, `"0x10"`, `"1__2"`,
/// lone sign) is a `ValueError`.
fn py_int_i32(text: &str) -> Result<i32, ()> {
    // `int()` strips Unicode whitespace (`\u{a0}`, `\u{2003}`, `\u{85}`
    // all strip; `\u{1c}` does not — probed).
    let text = text.trim_matches(|c: char| c.is_whitespace());
    let (negative, digits) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    };
    if digits.is_empty() {
        return Err(());
    }
    let mut value: i64 = 0;
    let mut count = 0u32;
    let mut prev_underscore = false;
    let mut any_digit = false;
    for c in digits.chars() {
        if c == '_' {
            if !any_digit || prev_underscore {
                return Err(());
            }
            prev_underscore = true;
            continue;
        }
        let Some(digit) = py_decimal_value(c) else {
            return Err(());
        };
        prev_underscore = false;
        any_digit = true;
        // Only significant digits count toward the 10-digit `i32` bound:
        // leading zeros don't change the value (`int("00000000001")`
        // is 1 — probed). Past 4300 digits `int()` itself refuses, but
        // that is a 500 either way, so over-long input just fails below.
        if digit != 0 || count > 0 {
            count += 1;
            if count <= 10 {
                value = value * 10 + digit as i64;
            }
        }
    }
    if prev_underscore || count > 10 {
        return Err(());
    }
    let value = if negative { -value } else { value };
    i32::try_from(value).map_err(|_| ())
}

/// The decimal value of an ASCII or Unicode decimal (`Nd`) digit, via
/// each script's zero point (`int()` accepts every `Nd` char — probed).
/// Zeros generated from CPython 3.12's `unicodedata` (Unicode 15.0).
fn py_decimal_value(c: char) -> Option<u32> {
    if c.is_ascii_digit() {
        return Some(c as u32 - '0' as u32);
    }
    const ZEROS: [u32; 67] = [
        0x0660, 0x06F0, 0x07C0, 0x0966, 0x09E6, 0x0A66, 0x0AE6, 0x0B66, 0x0BE6, 0x0C66, 0x0CE6,
        0x0D66, 0x0DE6, 0x0E50, 0x0ED0, 0x0F20, 0x1040, 0x1090, 0x17E0, 0x1810, 0x1946, 0x19D0,
        0x1A80, 0x1A90, 0x1B50, 0x1BB0, 0x1C40, 0x1C50, 0xA620, 0xA8D0, 0xA900, 0xA9D0, 0xA9F0,
        0xAA50, 0xABF0, 0xFF10, 0x104A0, 0x10D30, 0x11066, 0x110F0, 0x11136, 0x111D0, 0x112F0,
        0x11450, 0x114D0, 0x11650, 0x116C0, 0x11730, 0x118E0, 0x11950, 0x11C50, 0x11D50, 0x11DA0,
        0x11F50, 0x16A60, 0x16AC0, 0x16B50, 0x1D7CE, 0x1D7D8, 0x1D7E2, 0x1D7EC, 0x1D7F6, 0x1E140,
        0x1E2F0, 0x1E4F0, 0x1E950, 0x1FBF0,
    ];
    let code = c as u32;
    ZEROS.iter().find_map(|zero| {
        if (0..10).contains(&(code.wrapping_sub(*zero))) {
            Some(code - zero)
        } else {
            None
        }
    })
}

/// Adapt a JSON number to the integer `role` column: integers that fit,
/// plus every finite float truncated toward zero FIRST and then
/// range-checked (Python `int(value)` semantics: `int(2147483647.5)` →
/// `2147483647` fits, `int(-2147483648.5)` → `-2147483648` fits and the
/// role CHECK then decides at INSERT); out-of-range-after-truncation and
/// unparsable numbers error out → generic 500 (Postgres numeric overflow
/// surfaces as `DataError`, which `handle_exception` does not catch, so
/// 500 either way).
fn role_from_number(n: &serde_json::Number) -> Result<i32, Denial> {
    if let Some(v) = n.as_i64() {
        return i32::try_from(v).map_err(|_| Denial::ServerError);
    }
    if let Some(v) = n.as_f64() {
        if v.is_finite() {
            let truncated = v.trunc();
            if truncated >= f64::from(i32::MIN) && truncated <= f64::from(i32::MAX) {
                return Ok(truncated as i32);
            }
        }
    }
    Err(Denial::ServerError)
}

/// Adapt the `email` value to the `User.objects.get(email=…)` lookup the
/// way `request.data` + the ORM do: `request.data.get("email", False)` +
/// `if not email` 400s on missing, null, `""`, `false`, numeric zero, and
/// EMPTY containers; every other value reaches the lookup, where
/// `CharField.get_prep_value` stringifies it (`True` → `"True"`,
/// `[1]` → `"[1]"`) and an unknown address 404s. Container spellings
/// differ textually from CPython `str()` (`["a"]` vs `['a']`), but either
/// spelling only decides which unknown address 404s. An uploaded file is
/// truthy (`bool(name)`) and stringifies to its name in the lookup, so a
/// file 404s unless a user is literally named for it (probed).
fn lookup_email(found: BodyField<'_>) -> Result<String, Denial> {
    if let BodyField::File(file) = found {
        return Ok(file.filename.clone());
    }
    let value = match found {
        BodyField::Json(value) => Some(value),
        _ => None,
    };
    match value {
        Some(Value::String(s)) if !s.is_empty() => Ok(s.clone()),
        Some(Value::Bool(true)) => Ok("True".to_owned()),
        Some(Value::Number(n)) if n.as_i64() != Some(0) && n.as_f64() != Some(0.0) => {
            Ok(n.to_string())
        }
        Some(Value::Array(items)) if !items.is_empty() => {
            Ok(Value::Array(items.clone()).to_string())
        }
        Some(Value::Object(map)) if !map.is_empty() => Ok(Value::Object(map.clone()).to_string()),
        None
        | Some(Value::Null)
        | Some(Value::String(_))
        | Some(Value::Bool(_))
        | Some(Value::Number(_))
        | Some(Value::Array(_))
        | Some(Value::Object(_)) => Err(Denial::BadError("Email is required".to_owned())),
    }
}

fn json_response(status: StatusCode, body: Value) -> Response {
    json_response_text(status, body.to_string())
}

fn json_response_text(status: StatusCode, text: String) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(text))
        .expect("json response")
}

/// Sentinel marking the 201 `role` slot for the echo splice below. NUL
/// cannot come from Postgres text, and an int-parseable echo literal never
/// contains one, so the quoted needle is unique in the body.
const ROLE_ECHO_SENTINEL: &str = "\u{0}role\u{0}";
const ROLE_ECHO_QUOTED: &str = "\"\\u0000role\\u0000\"";

/// Render the 201 `role` slot's exact literal from the raw echo
/// [`role_from_value`] produced. That helper applies the `ChoiceField`
/// map (`str(value) == "20"` -> the `20` number) and returns the raw
/// input otherwise; this turns the value into the bytes Django writes.
/// Serde cannot spell Python floats (`1e-05`, never `1e-5`), so floats go
/// through [`py_float_str`]; ints render as `str(int)` (`-0` -> `0`);
/// strings keep their JSON quoting (DRF's `ensure_ascii=False` matches
/// serde's raw UTF-8); bools are JSON `true`/`false` (PIDASHCONV-1129).
fn role_echo_literal(echo: &Value) -> String {
    match echo {
        Value::Number(n) if n.as_i64().is_none() => n
            .as_f64()
            .map(py_float_str)
            .unwrap_or_else(|| n.to_string()),
        Value::String(s) => serde_json::to_string(s).unwrap_or_else(|_| "\"20\"".to_owned()),
        other => other.to_string(),
    }
}

/// Splice a pre-rendered 201 `role` literal into a serialized admin body
/// over the sentinel slot. `serde_json` cannot emit Python float spellings
/// (`1e-05`) or the verbatim echo, so the literal goes in as text; the
/// overwrite keeps the `role` key exactly where the view declared it.
fn splice_role_echo(body: &Value, echo: &str) -> String {
    let mut owned = body.clone();
    owned["role"] = Value::String(ROLE_ECHO_SENTINEL.to_owned());
    owned.to_string().replace(ROLE_ECHO_QUOTED, echo)
}

/// `POST /api/instances/admins/` (`admin.py:49-68`).
async fn create_admin(
    State(state): State<AppState>,
    extension: Option<Extension<SessionHandle>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Result<Response, Denial> {
    let pool = pools(&state)?;
    let actor = authed_user(pool, state.settings().secret_key.as_bytes(), &extension).await?;
    let instance_id = require_instance_admin(pool, &actor).await?;
    let tz = actor_timezone(pool, &actor).await?;

    let data = parse_json_body(&headers, &body, state.settings().file_size_limit)?;
    let email = lookup_email(data.field("email"))?;
    if instance_first(pool).await?.is_none() {
        // Near-dead: the permission gate above already denied the
        // instance-less case with the default 403. Kept for order parity.
        return Err(Denial::NoInstance);
    }

    // `User.objects.get(email=email)`: exact match, no manager filter
    // (stock Django `UserManager`); `DoesNotExist` → 404 branch.
    let user_row: Option<(Uuid,)> = sqlx::query_as("SELECT id FROM users WHERE email = $1")
        .bind(&email)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let Some((user_id,)) = user_row else {
        return Err(Denial::NotFound);
    };

    // The role is parsed here, after the user lookup, because Django only
    // touches it inside `.create()`: an unknown email 404s no matter how
    // unparsable the role is (PIDASHCONV-1120).
    let (role, role_echo) = role_from_value(data.field("role"))?;

    // `InstanceAdmin.objects.create(...)`: `BaseModel.save` stamps
    // `created_by` from the request user and leaves `updated_by` null.
    // `unique_together(instance, user)` violations → 400 invalid-payload.
    // The id renders server-side (`gen_random_uuid()` is uuid4, like
    // Django's default) so no uuid RNG feature is needed client-side.
    let now = Utc::now();
    let created: Result<(Uuid,), sqlx::Error> = sqlx::query_as(
        "INSERT INTO instance_admins (id, created_at, updated_at, deleted_at, role, is_verified, created_by_id, updated_by_id, user_id, instance_id) VALUES (gen_random_uuid(), $1, $2, NULL, $3, FALSE, $4, NULL, $5, $6) RETURNING id",
    )
    .bind(now)
    .bind(now)
    .bind(role)
    .bind(actor)
    .bind(user_id)
    .bind(instance_id)
    .fetch_one(pool)
    .await;
    let admin_id = match created {
        Ok((id,)) => id,
        // `23505` is the `unique_together(instance, user)` violation;
        // `23514` is the `PositiveIntegerField` CHECK (`role >= 0`). Both
        // are `IntegrityError` → invalid-payload 400; nothing else on this
        // INSERT can raise a CHECK.
        Err(sqlx::Error::Database(db))
            if matches!(db.code().as_deref(), Some("23505" | "23514")) =>
        {
            return Err(Denial::InvalidPayload);
        }
        Err(_) => return Err(Denial::ServerError),
    };

    let user = fetch_user(pool, &user_id)
        .await?
        .ok_or(Denial::ServerError)?;
    let body = render_admin(
        &admin_id,
        &now,
        &now,
        None,
        role,
        false,
        Some(actor),
        None,
        Some(user_id),
        &instance_id,
        Some(&user),
        &tz,
    )?;
    // The 201 renders the in-memory `create()` instance, whose `role`
    // is still the RAW input: the `choices` `ChoiceField` echoes it
    // unless `str(value) == "20"`. The shared row struct only carries
    // the INSERT int (foundation-owned), and serde cannot spell Python
    // floats (`1e-05`), so `role_from_value` returns the raw echo and
    // this splices its exact literal over the `role` slot (PIDASHCONV-1129).
    let echo = role_echo_literal(&role_echo);
    Ok(json_response_text(
        StatusCode::CREATED,
        splice_role_echo(&body, &echo),
    ))
}

/// `GET /api/instances/admins/` (`admin.py:70-80`): the 2h
/// `cache_response` is freshness-only (see module docs) and always reads
/// fresh here.
async fn list_admins(
    State(state): State<AppState>,
    extension: Option<Extension<SessionHandle>>,
) -> Result<Response, Denial> {
    let pool = pools(&state)?;
    let actor = authed_user(pool, state.settings().secret_key.as_bytes(), &extension).await?;
    let instance_id = require_instance_admin(pool, &actor).await?;
    let tz = actor_timezone(pool, &actor).await?;
    if instance_first(pool).await?.is_none() {
        // Near-dead (see `create_admin`); kept for order parity.
        return Err(Denial::NoInstance);
    }

    /// One list row: the admin columns plus the nested user's lite
    /// columns. Aliases must match these field names (`FromRow` by name).
    #[derive(Debug, Clone, sqlx::FromRow)]
    struct AdminListRow {
        id: Uuid,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
        deleted_at: Option<DateTime<Utc>>,
        role: i32,
        is_verified: bool,
        created_by_id: Option<Uuid>,
        updated_by_id: Option<Uuid>,
        user_id: Option<Uuid>,
        instance_id: Uuid,
        u_id: Option<Uuid>,
        u_first_name: Option<String>,
        u_last_name: Option<String>,
        u_avatar: Option<String>,
        u_avatar_asset_id: Option<Uuid>,
        u_avatar_asset_row: Option<Uuid>,
        u_avatar_asset_entity: Option<String>,
        u_avatar_asset_workspace: Option<Uuid>,
        u_avatar_asset_project: Option<Uuid>,
        u_avatar_asset_issue: Option<Uuid>,
        u_avatar_asset_workspace_slug: Option<String>,
        u_is_bot: Option<bool>,
        u_display_name: Option<String>,
        u_email: Option<String>,
        u_last_login_medium: Option<String>,
    }

    // The asset joins are deliberately unscoped (see `fetch_user`): the
    // forward-FK descriptor reads through `_base_manager`.
    let rows: Vec<AdminListRow> = sqlx::query_as(
        "SELECT a.id AS id, a.created_at AS created_at, a.updated_at AS updated_at, a.deleted_at AS deleted_at, a.role AS role, a.is_verified AS is_verified, a.created_by_id AS created_by_id, a.updated_by_id AS updated_by_id, a.user_id AS user_id, a.instance_id AS instance_id, u.id AS u_id, u.first_name AS u_first_name, u.last_name AS u_last_name, u.avatar AS u_avatar, u.avatar_asset_id AS u_avatar_asset_id, fa.id AS u_avatar_asset_row, fa.entity_type AS u_avatar_asset_entity, fa.workspace_id AS u_avatar_asset_workspace, fa.project_id AS u_avatar_asset_project, fa.issue_id AS u_avatar_asset_issue, w.slug AS u_avatar_asset_workspace_slug, u.is_bot AS u_is_bot, u.display_name AS u_display_name, u.email AS u_email, u.last_login_medium AS u_last_login_medium FROM instance_admins a LEFT JOIN users u ON u.id = a.user_id LEFT JOIN file_assets fa ON fa.id = u.avatar_asset_id LEFT JOIN workspaces w ON w.id = fa.workspace_id WHERE a.instance_id = $1 AND a.deleted_at IS NULL ORDER BY a.created_at DESC",
    )
    .bind(instance_id)
    .fetch_all(pool)
    .await
    .map_err(|_| Denial::ServerError)?;

    let mut items = Vec::with_capacity(rows.len());
    for row in &rows {
        let user = row.u_id.map(|uid| UserShape {
            id: Some(uid),
            first_name: row.u_first_name.clone(),
            last_name: row.u_last_name.clone(),
            avatar: row.u_avatar.clone(),
            avatar_asset_id: row.u_avatar_asset_id,
            avatar_asset_row: row.u_avatar_asset_row,
            avatar_asset_entity: row.u_avatar_asset_entity.clone(),
            avatar_asset_workspace: row.u_avatar_asset_workspace,
            avatar_asset_project: row.u_avatar_asset_project,
            avatar_asset_issue: row.u_avatar_asset_issue,
            avatar_asset_workspace_slug: row.u_avatar_asset_workspace_slug.clone(),
            cover_image: None,
            date_joined: None,
            display_name: row.u_display_name.clone(),
            email: row.u_email.clone(),
            is_active: None,
            is_bot: row.u_is_bot,
            is_email_verified: None,
            user_timezone: None,
            username: None,
            is_password_autoset: None,
            last_login_medium: row.u_last_login_medium.clone(),
        });
        items.push(render_admin(
            &row.id,
            &row.created_at,
            &row.updated_at,
            row.deleted_at.as_ref(),
            row.role,
            row.is_verified,
            row.created_by_id,
            row.updated_by_id,
            row.user_id,
            &row.instance_id,
            user.as_ref(),
            &tz,
        )?);
    }
    Ok(json_response(StatusCode::OK, Value::Array(items)))
}

/// `DELETE /api/instances/admins/<uuid:pk>/` (`admin.py:82-86`):
/// `filter(instance, pk).delete()` is a soft UPDATE of `deleted_at` only
/// (no `updated_at` touch, no `save()` signals) with no existence check —
/// unknown PKs still 204. A non-UUID `pk` never reaches the view in
/// Django (the `<uuid:pk>` converter 404s), so it falls through to the
/// proxy for Django's own 404.
async fn delete_admin(
    State(state): State<AppState>,
    extension: Option<Extension<SessionHandle>>,
    axum::extract::Path(pk): axum::extract::Path<String>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, Denial> {
    // Django routes this URL through the `<uuid:pk>` converter
    // (`license/urls.py:37`), whose regex is lowercase-hyphenated only and
    // compiled case-sensitively, so every other spelling resolver-404s
    // before auth (and before the view). `Uuid::parse_str` accepts
    // uppercase, simple, braced and URN spellings, so gate on the raw
    // segment first and proxy the rest — parseable-but-non-canonical and
    // garbage alike — to Django for its own resolver 404
    // (PIDASHCONV-1095 / PB-0340 precedent).
    let admin_id = match pk.parse::<Uuid>() {
        Ok(admin_id) if is_canonical_uuid(&pk) => admin_id,
        _ => {
            let mut builder = Request::builder().method(method).uri(uri);
            for (name, value) in headers.iter() {
                builder = builder.header(name, value);
            }
            let req = builder
                .body(axum::body::Body::empty())
                .map_err(|_| Denial::ServerError)?;
            return Ok(edge::proxy(State(state), req).await);
        }
    };
    let pool = pools(&state)?;
    let actor = authed_user(pool, state.settings().secret_key.as_bytes(), &extension).await?;
    let instance_id = require_instance_admin(pool, &actor).await?;

    // The manager's `deleted_at IS NULL` rides the filter. Queryset
    // `.delete()` is `update(deleted_at=…)` (`mixins.py:SoftDeletionQuerySet`):
    // no `updated_at` touch, no signals, and no `soft_delete_related_objects`
    // publish — that `.delay` only fires in the instance `delete()` path, so
    // there is nothing to skip.
    sqlx::query(
        "UPDATE instance_admins SET deleted_at = $1 WHERE instance_id = $2 AND id = $3 AND deleted_at IS NULL",
    )
    .bind(Utc::now())
    .bind(instance_id)
    .bind(admin_id)
    .execute(pool)
    .await
    .map_err(|_| Denial::ServerError)?;

    Ok(Response::builder()
        .status(StatusCode::NO_CONTENT)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::empty())
        .expect("empty 204 response"))
}

/// `GET /api/instances/admins/me/` (`admin.py:361-366`): admin
/// permission, then the me serializer over the acting user.
async fn me(
    State(state): State<AppState>,
    extension: Option<Extension<SessionHandle>>,
) -> Result<Response, Denial> {
    let pool = pools(&state)?;
    let actor = authed_user(pool, state.settings().secret_key.as_bytes(), &extension).await?;
    require_instance_admin(pool, &actor).await?;
    let tz = actor_timezone(pool, &actor).await?;
    // `authed_user` already proved the row exists.
    let user = fetch_user(pool, &actor).await?.ok_or(Denial::ServerError)?;
    Ok(json_response(StatusCode::OK, render_me(&user, &tz)?))
}

/// `GET /api/instances/admins/session/` (`admin.py:369-379`, `AllowAny`):
/// authed admins (any instance, any role — the filter is on `user` alone)
/// get the me shape under `user`; everyone else gets the bare flag.
async fn session(
    State(state): State<AppState>,
    extension: Option<Extension<SessionHandle>>,
) -> Result<Response, Denial> {
    let pool = pools(&state)?;
    // `TimezoneMixin.initial` runs even under `AllowAny`: an authenticated
    // session resolves its zone (bad zone → 500) before the view body, so
    // the zone resolves before the admin check, exactly like Python. A
    // session whose user row is gone — or whose backend/hash fails, or
    // whose user is inactive — authenticates to nobody → bare flag.
    if let Some(actor) =
        verified_actor(pool, state.settings().secret_key.as_bytes(), &extension).await?
    {
        if let Some(user) = fetch_user(pool, &actor).await? {
            let tz: Tz = user
                .user_timezone
                .as_deref()
                .unwrap_or("UTC")
                .parse()
                .map_err(|_| Denial::ServerError)?;
            let row: Option<(i32,)> = sqlx::query_as(
                "SELECT 1 FROM instance_admins WHERE user_id = $1 AND deleted_at IS NULL LIMIT 1",
            )
            .bind(actor)
            .fetch_optional(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
            if row.is_some() {
                let mut data = serde_json::Map::with_capacity(2);
                data.insert("is_authenticated".to_owned(), Value::Bool(true));
                data.insert("user".to_owned(), render_me(&user, &tz)?);
                return Ok(json_response(StatusCode::OK, Value::Object(data)));
            }
        }
    }
    Ok(json_response(
        StatusCode::OK,
        serde_json::json!({"is_authenticated": false}),
    ))
}

/// Client IP (`ip_address.py:get_client_ip`): first `X-Forwarded-For`
/// entry, else the peer address — which axum handlers do not see without
/// `ConnectInfo`, so absent means empty (sign-out is untested; the shape
/// of the stamp matters, not the value).
fn client_ip(headers: &HeaderMap) -> String {
    headers
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
        .and_then(|forwarded| forwarded.split(',').next())
        .map(str::trim)
        .unwrap_or("")
        .to_owned()
}

/// Safe sign-out target (`host.py:base_host` with `is_admin=True` through
/// `path_validator.py:get_safe_redirect_url` with an empty next path):
/// `(ADMIN_BASE_URL or (WEB_URL or APP_BASE_URL)) + ADMIN_BASE_PATH`,
/// trailing slashes stripped, no query (the empty path validates to "").
fn sign_out_redirect(
    admin_base_url: Option<&str>,
    web_url: Option<&str>,
    app_base_url: Option<&str>,
    admin_base_path: &str,
) -> Result<String, Denial> {
    let mut path = admin_base_path.to_owned();
    if !path.starts_with('/') {
        path.insert(0, '/');
    }
    if !path.ends_with('/') {
        path.push('/');
    }
    let base = admin_base_url
        .filter(|s| !s.is_empty())
        .or_else(|| web_url.filter(|s| !s.is_empty()))
        .or_else(|| app_base_url.filter(|s| !s.is_empty()))
        .ok_or(Denial::ServerError)?;
    Ok(format!("{base}{path}").trim_end_matches('/').to_owned())
}

/// `POST /api/instances/admins/sign-out/` (`admin.py:382-398`): a plain
/// Django `View`, so there is no auth gate — an anonymous hit raises out
/// of `User.objects.get(pk=None)` and still redirects. The success path
/// stamps `last_logout_ip/time` (plus `updated_at`, via `auto_now`) and
/// flushes the session; ANY failure skips the save and the flush and only
/// redirects.
async fn sign_out(
    State(state): State<AppState>,
    extension: Option<Extension<SessionHandle>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Result<Response, Denial> {
    // `CsrfViewMiddleware` runs before the view body: a token-less POST
    // never reaches `admin.py:385`.
    let pairs = form_pairs(content_type_of(&headers), &body);
    if csrf_gate(&headers, &pairs).is_err() {
        return Ok(csrf_failure_response(&state));
    }
    let urls = &state.settings().urls;
    let target = sign_out_redirect(
        urls.admin_base_url.as_deref(),
        urls.web_url.as_deref(),
        urls.app_base_url.as_deref(),
        &urls.admin_base_path,
    )?;
    let outcome: Result<(), Denial> = async {
        let pool = pools(&state)?;
        // `get_user` yields anonymous for a missing row, a wrong backend,
        // a stale hash, or an inactive user (`ModelBackend.get_user`
        // rejects those before the hash check); the view's `get(pk=None)`
        // then raises before any stamp — the same early-out.
        let actor = verified_actor(pool, state.settings().secret_key.as_bytes(), &extension)
            .await?
            .ok_or(Denial::ServerError)?;
        let now = Utc::now();
        sqlx::query(
            "UPDATE users SET last_logout_ip = $1, last_logout_time = $2, updated_at = $2 WHERE id = $3",
        )
        .bind(client_ip(&headers))
        .bind(now)
        .bind(actor)
        .execute(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
        // `logout()`: `Session.flush()` drops the presented row (this
        // route always reads the admin cookie); the middleware then
        // answers the cookie deletion because the session reads empty.
        // Anonymous hits — no session, or a session that fails the
        // backend/hash/active check — never reach here (the `get(pk=None)`
        // raises first, skipping `logout()` entirely — same early-out).
        let cookie_header = headers
            .get(header::COOKIE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        if let Some(key) =
            session_kernel::cookie_value(cookie_header, session_kernel::ADMIN_SESSION_COOKIE_NAME)
        {
            sqlx::query("DELETE FROM sessions WHERE session_key = $1")
                .bind(key)
                .execute(pool)
                .await
                .map_err(|_| Denial::ServerError)?;
        }
        if let Some(handle) = extension.as_ref().map(|ext| ext.0.clone()) {
            handle.lock().clear();
        }
        Ok(())
    }
    .await;
    // Success AND failure both 302 (`except Exception: … redirect anyway`),
    // via the shared `HttpResponseRedirect` replay (302 + `Location` +
    // `Content-Type: text/html` + `Vary: Cookie`; the Vary middleware
    // dedupes the `Vary` leg).
    let _ = outcome;
    redirect_response(&target, Vec::new())
}

// ---------------------------------------------------------------------------
// Form-POST auth flows: sign-up / sign-in (`admin.py:89-358`)
// ---------------------------------------------------------------------------

/// `CsrfViewMiddleware` cookie name (`settings/common.py`, Django default).
const CSRF_COOKIE_NAME: &str = "csrftoken";

/// `CSRF_COOKIE_AGE` (Django default): the rotated login cookie's age.
const CSRF_AGE_SECS: i64 = 31_449_600;

/// `make_password` iteration count (Django 4.2 default hasher).
const PBKDF2_ITERATIONS: u32 = 600_000;

/// `BasePasswordHasher.salt()` length: 128 bits over the 62-char alphabet.
const PASSWORD_SALT_LEN: usize = 22;

/// Salt for `user.get_session_auth_hash()` (same construction as
/// `super::SESSION_AUTH_HASH_SALT`; `auth_session/email.rs` carries its
/// own copy too).
const SESSION_AUTH_HASH_SALT: &str =
    "django.contrib.auth.models.AbstractBaseUser.get_session_auth_hash";

/// Why the CSRF gate rejected (Django `REASON_*`; the failure page never
/// renders the reason, so these are log labels only).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CsrfDenial {
    NoCookie,
    BadToken,
}

/// Where the request token came from (`CsrfViewMiddleware._check_token`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TokenSource {
    Post,
    Header,
}

impl TokenSource {
    /// `_bad_token_message` source rendering (`csrf.py:347-352`).
    fn describe(self) -> &'static str {
        match self {
            TokenSource::Post => "POST",
            TokenSource::Header => "the 'X-Csrftoken' HTTP header",
        }
    }
}

fn content_type_of(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
}

/// `request.POST` as last-wins pairs, only for form content types (any
/// other content type leaves `POST` empty in Django).
fn form_pairs(content_type: Option<&str>, body: &[u8]) -> Vec<(String, String)> {
    let is_form = content_type.is_some_and(|ct| {
        ct.split(';').next().is_some_and(|mime| {
            mime.trim()
                .eq_ignore_ascii_case("application/x-www-form-urlencoded")
        })
    });
    if !is_form {
        return Vec::new();
    }
    serde_urlencoded::from_bytes::<Vec<(String, String)>>(body).unwrap_or_default()
}

/// `QueryDict.get`: the last value for `key`, or `None`.
fn form_last(pairs: &[(String, String)], key: &str) -> Option<String> {
    pairs
        .iter()
        .rev()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.clone())
}

/// Resolve the request token in Django's order: the last
/// `csrfmiddlewaretoken` form value when non-empty, else the header when
/// present (even empty — an empty header fails format, it does not fall
/// through to "missing").
fn csrf_token_source(
    pairs: &[(String, String)],
    header_token: Option<&str>,
) -> Option<(String, TokenSource)> {
    if let Some(value) = pairs
        .iter()
        .rev()
        .find(|(k, _)| k == "csrfmiddlewaretoken")
        .map(|(_, v)| v.as_str())
        .filter(|v| !v.is_empty())
    {
        return Some((value.to_owned(), TokenSource::Post));
    }
    header_token.map(|token| (token.to_owned(), TokenSource::Header))
}

/// `_check_token` + `_does_token_match` over one token: the format gate
/// first, then the unmask-and-compare, rendered like `_bad_token_message`.
fn csrf_check_token(secret: &str, token: &str, source: TokenSource) -> Result<(), String> {
    if let Err(error) = csrf_kernel::check_token_format(token) {
        return Err(format!(
            "CSRF token from {} {}.",
            source.describe(),
            error.reason()
        ));
    }
    if csrf_kernel::tokens_match(token, secret) {
        Ok(())
    } else {
        Err(format!("CSRF token from {} incorrect.", source.describe()))
    }
}

/// Mirror `CsrfViewMiddleware.process_view`: the `csrftoken` cookie must be
/// present and the request token must check out. A malformed cookie secret
/// can never match (Django would mint a fresh secret and then fail the
/// compare the same way).
fn csrf_gate(headers: &HeaderMap, pairs: &[(String, String)]) -> Result<(), CsrfDenial> {
    let cookie_header = headers
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let secret = session_kernel::cookie_value(cookie_header, CSRF_COOKIE_NAME)
        .ok_or(CsrfDenial::NoCookie)?;
    let header_token = headers.get("x-csrftoken").and_then(|v| v.to_str().ok());
    let (token, source) = csrf_token_source(pairs, header_token).ok_or(CsrfDenial::BadToken)?;
    csrf_check_token(&secret, &token, source).map_err(|_| CsrfDenial::BadToken)
}

/// `base_host` with no surface flags (`WEB_URL or APP_BASE_URL`): the
/// `root_url` the `csrf_failure` view renders.
fn failure_root_url(state: &AppState) -> String {
    let urls = &state.settings().urls;
    urls.web_url
        .as_deref()
        .filter(|s| !s.is_empty())
        .or_else(|| urls.app_base_url.as_deref().filter(|s| !s.is_empty()))
        .unwrap_or("")
        .to_owned()
}

/// The `csrf_failure` page: 200 + `text/html`.
fn csrf_failure_response(state: &AppState) -> Response {
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (header::VARY, "Cookie"),
        ],
        render_csrf_failure(&failure_root_url(state)),
    )
        .into_response()
}

/// `HttpResponseRedirect`: 302 + `Location`, `Vary: Cookie`, and any
/// `Set-Cookie` values the branch issued.
fn redirect_response(location: &str, set_cookies: Vec<String>) -> Result<Response, Denial> {
    let location = crate::response_headers::header_value(location).ok_or(Denial::ServerError)?;
    let mut builder = Response::builder()
        .status(StatusCode::FOUND)
        .header(header::LOCATION, location)
        .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
        .header(header::VARY, "Cookie");
    for cookie in set_cookies {
        builder = builder.header(header::SET_COOKIE, cookie);
    }
    Ok(builder
        .body(axum::body::Body::empty())
        .expect("static redirect"))
}

/// Admin base URL for the redirect targets (`host.py:16-39` through the
/// §3 kernel): `ADMIN_BASE_URL` wins when configured and non-empty, else
/// `WEB_URL or APP_BASE_URL`. Both-unset with no admin URL is a
/// deployment misconfiguration that raises `TypeError` in Python (500).
fn admin_base(state: &AppState) -> Result<String, Denial> {
    let urls = &state.settings().urls;
    let admin_url = urls.admin_base_url.as_deref().filter(|s| !s.is_empty());
    let origin = failure_root_url(state);
    if admin_url.is_none() && origin.is_empty() {
        return Err(Denial::ServerError);
    }
    Ok(forms::admin_base_url(
        &origin,
        admin_url,
        Some(urls.admin_base_path.as_str()),
    ))
}

/// `user.get_session_auth_hash()`: `salted_hmac(...get_session_auth_hash,
/// password).hexdigest()`.
fn session_auth_hash(password_field: &str, secret_key: &[u8]) -> String {
    use hmac::Mac as _;
    let key = Sha256::digest([SESSION_AUTH_HASH_SALT.as_bytes(), secret_key].concat());
    let mut mac = Hmac::<Sha256>::new_from_slice(&key).expect("HMAC-SHA256 accepts any key length");
    mac.update(password_field.as_bytes());
    hex_encode(&mac.finalize().into_bytes())
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

/// `get_random_string(22)` over the Django alphabet
/// (`BasePasswordHasher.salt()`), drawn from UUID4 bytes with rejection
/// (`auth_session/magic.rs` `django_random_string`).
fn django_random_string(n: usize) -> String {
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut out = String::with_capacity(n);
    let mut rand = Uuid::new_v4().into_bytes();
    let mut at = 0;
    while out.len() < n {
        if at == rand.len() {
            rand = Uuid::new_v4().into_bytes();
            at = 0;
        }
        let byte = rand[at];
        at += 1;
        if byte < 248 {
            out.push(ALPHABET[(byte % 62) as usize] as char);
        }
    }
    out
}

/// `make_password(password)` (`admin.py:215`): PBKDF2, fresh salt, the
/// pinned Django 4.2 iteration count.
fn encode_password(password: &str) -> String {
    forms::encode_password(
        password,
        &django_random_string(PASSWORD_SALT_LEN),
        PBKDF2_ITERATIONS,
    )
}

/// `zxcvbn(password)["score"]` (same estimator, 0-4 scale; `admin.py:191`
/// scores the raw posted password).
fn password_score(password: &str) -> u8 {
    zxcvbn::zxcvbn(password, &[]).score() as u8
}

/// `User.save` display-name fill (`user.py:177-182`): the local part
/// (`split("@")[0]`; the `len(...)` guard is always truthy).
fn display_name_for(email: &str) -> String {
    email.split('@').next().unwrap_or("").to_owned()
}

/// `get_random_color` (`utils/color.py:9`): `#` + six hex digits.
fn background_color() -> String {
    format!("#{}", hex_encode(&Uuid::new_v4().into_bytes()[..3]))
}

/// Cookie attributes shared by the session + rotated CSRF cookies.
struct CookieCtx {
    domain: Option<String>,
    secure: bool,
}

fn cookie_ctx(state: &AppState) -> CookieCtx {
    let session = &state.settings().session;
    CookieCtx {
        domain: session.cookie_domain.clone(),
        secure: session.cookie_secure,
    }
}

/// `process_response` session cookie for the fresh/cycled admin key:
/// `set_expiry(ADMIN_SESSION_COOKIE_AGE)` means the cookie carries the age
/// (`get_expiry_age()`), never browser-close semantics.
fn session_set_cookie(key: &str, ctx: &CookieCtx, age_secs: i64, now_unix: i64) -> String {
    session_kernel::render_set_cookie(&session_kernel::SetCookie {
        name: session_kernel::ADMIN_SESSION_COOKIE_NAME.to_owned(),
        value: key.to_owned(),
        expires: Some(session_kernel::http_date(now_unix + age_secs)),
        max_age: Some(age_secs),
        domain: ctx.domain.clone(),
        path: "/".to_owned(),
        secure: ctx.secure,
        httponly: true,
        samesite: "Lax".to_owned(),
    })
}

/// `rotate_token`: fresh 32-char CSRF secret (`login()` rotates the CSRF
/// secret on every login via `rotate_token`).
fn csrf_rotate_cookie(secret: &str, ctx: &CookieCtx) -> String {
    session_kernel::render_set_cookie(&session_kernel::SetCookie {
        name: CSRF_COOKIE_NAME.to_owned(),
        value: secret.to_owned(),
        expires: Some(session_kernel::http_date(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0)
                + CSRF_AGE_SECS,
        )),
        max_age: Some(CSRF_AGE_SECS),
        domain: ctx.domain.clone(),
        path: "/".to_owned(),
        secure: ctx.secure,
        httponly: true,
        samesite: "Lax".to_owned(),
    })
}

/// The auth keys `login()` writes into the session payload.
fn auth_payload_entries(
    user_id: Uuid,
    password_field: &str,
    device_info: &serde_json::Value,
    secret_key: &[u8],
) -> Vec<(String, serde_json::Value)> {
    vec![
        (
            "_auth_user_id".to_owned(),
            serde_json::Value::String(user_id.to_string()),
        ),
        (
            "_auth_user_backend".to_owned(),
            serde_json::Value::String(forms::LOGIN_BACKEND.to_owned()),
        ),
        (
            "_auth_user_hash".to_owned(),
            serde_json::Value::String(session_auth_hash(password_field, secret_key)),
        ),
        ("device_info".to_owned(), device_info.clone()),
    ]
}

/// `login()` + `user_login`, keeping Django's key discipline
/// (`django/contrib/auth/__init__.py:106-118`): a presented session that
/// already authenticates this same user keeps its key (entries merged over
/// the surviving payload, row updated in place); anything else cycles the
/// key (presented row deleted, fresh row inserted).
/// `expire_date = now + ADMIN_SESSION_COOKIE_AGE` on both arms. Returns
/// the two `Set-Cookie` values (admin session + rotated CSRF secret).
#[allow(clippy::too_many_arguments)]
async fn issue_admin_session(
    pool: &PgPool,
    presented_key: Option<&str>,
    user_id: Uuid,
    password_field: &str,
    device_info: serde_json::Value,
    secret_key: &[u8],
    ctx: &CookieCtx,
    age_secs: i64,
    now_unix: i64,
) -> Result<Vec<String>, sqlx::Error> {
    let signer = Signer::new(secret_key, SESSION_SIGNING_SALT);
    let entries = auth_payload_entries(user_id, password_field, &device_info, secret_key);
    let user_id_str = user_id.to_string();
    let expire_unix = now_unix + age_secs;
    if let Some(old) = presented_key.filter(|key| session_kernel::is_plausible_session_key(key)) {
        let row: Option<(String, i64)> = sqlx::query_as(
            r#"SELECT "session_data", EXTRACT(EPOCH FROM "expire_date")::BIGINT FROM "sessions" WHERE "session_key" = $1"#,
        )
        .bind(old)
        .fetch_optional(pool)
        .await?;
        if let Some((session_data, expire_date_unix)) = row {
            if !session_kernel::is_expired(expire_date_unix, now_unix) {
                let unwrapped: Option<serde_json::Map<String, serde_json::Value>> =
                    signer.unsign_object(&session_data).ok();
                if let Some(mut payload) = unwrapped {
                    let same_user = payload.get("_auth_user_id").and_then(|v| v.as_str())
                        == Some(user_id_str.as_str())
                        && payload.get("_auth_user_backend").and_then(|v| v.as_str())
                            == Some(forms::LOGIN_BACKEND)
                        && payload.get("_auth_user_hash").and_then(|v| v.as_str())
                            == Some(session_auth_hash(password_field, secret_key).as_str());
                    if same_user {
                        for (key, value) in &entries {
                            payload.insert(key.clone(), value.clone());
                        }
                        let session_data = signer
                            .sign_object(&payload, now_unix as u64)
                            .map_err(|_| sqlx::Error::RowNotFound)?;
                        sqlx::query(
                            r#"UPDATE "sessions" SET "session_data" = $2,
                               "expire_date" = to_timestamp($3::DOUBLE PRECISION),
                               "user_id" = $4, "device_info" = $5
                               WHERE "session_key" = $1"#,
                        )
                        .bind(old)
                        .bind(&session_data)
                        .bind(expire_unix as f64)
                        .bind(user_id.to_string())
                        .bind(sqlx::types::Json(device_info.clone()))
                        .execute(pool)
                        .await?;
                        return Ok(vec![
                            session_set_cookie(old, ctx, age_secs, now_unix),
                            csrf_rotate_cookie(&csrf_kernel::new_secret(), ctx),
                        ]);
                    }
                }
            }
            sqlx::query(r#"DELETE FROM "sessions" WHERE "session_key" = $1"#)
                .bind(old)
                .execute(pool)
                .await?;
        }
    }
    let mut payload = serde_json::Map::new();
    for (key, value) in &entries {
        payload.insert(key.clone(), value.clone());
    }
    let session_data = signer
        .sign_object(&payload, now_unix as u64)
        .map_err(|_| sqlx::Error::RowNotFound)?;
    let key = loop {
        let key = session_kernel::generate_session_key();
        let inserted = sqlx::query(
            r#"INSERT INTO "sessions" ("session_key", "session_data", "expire_date", "user_id", "device_info")
               VALUES ($1, $2, to_timestamp($3::DOUBLE PRECISION), $4, $5)
               ON CONFLICT ("session_key") DO NOTHING"#,
        )
        .bind(&key)
        .bind(&session_data)
        .bind(expire_unix as f64)
        .bind(user_id.to_string())
        .bind(sqlx::types::Json(device_info.clone()))
        .execute(pool)
        .await?;
        if inserted.rows_affected() > 0 {
            break key;
        }
    };
    Ok(vec![
        session_set_cookie(&key, ctx, age_secs, now_unix),
        csrf_rotate_cookie(&csrf_kernel::new_secret(), ctx),
    ])
}

/// `InstanceAdmin.objects.first()` existence (`admin.py:108`): any live row.
async fn any_admin_exists(pool: &PgPool) -> Result<bool, Denial> {
    let row: Option<(i32,)> =
        sqlx::query_as("SELECT 1 FROM instance_admins WHERE deleted_at IS NULL LIMIT 1")
            .fetch_optional(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    Ok(row.is_some())
}

/// `User.objects.filter(email).exists()` (`admin.py:173`).
async fn user_exists_by_email(pool: &PgPool, email: &str) -> Result<bool, Denial> {
    let row: Option<(i32,)> = sqlx::query_as("SELECT 1 FROM users WHERE email = $1 LIMIT 1")
        .bind(email)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(row.is_some())
}

/// `User.objects.filter(email).first()` (`admin.py:294`): id, active flag,
/// password field.
async fn signin_user_lookup(
    pool: &PgPool,
    email: &str,
) -> Result<Option<(Uuid, bool, String)>, Denial> {
    sqlx::query_as(
        "SELECT id, is_active, password FROM users WHERE email = $1 ORDER BY created_at DESC LIMIT 1",
    )
    .bind(email)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)
}

/// `InstanceAdmin.objects.filter(instance, user)` existence (`admin.py:335`):
/// any live row — no role gate, no verified gate.
async fn is_instance_admin(
    pool: &PgPool,
    instance_id: &Uuid,
    user_id: &Uuid,
) -> Result<bool, Denial> {
    let row: Option<(i32,)> = sqlx::query_as(
        "SELECT 1 FROM instance_admins WHERE instance_id = $1 AND user_id = $2 AND deleted_at IS NULL LIMIT 1",
    )
    .bind(instance_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(row.is_some())
}

/// `User.objects.create(...)` (`admin.py:210-217`) + `User.save()` side
/// effects (`user.py:169-187`): every NOT NULL column is supplied (Django
/// emits no DB-level defaults). `is_active` is already TRUE (model
/// default); `token` stays `""` (`token_updated_at` NULL → no rotation).
#[allow(clippy::too_many_arguments)]
async fn insert_signup_user(
    pool: &PgPool,
    now: DateTime<Utc>,
    email: &str,
    first_name: &str,
    last_name: &str,
    password_hash: &str,
) -> Result<Uuid, Denial> {
    let id = Uuid::new_v4();
    let username = Uuid::new_v4().simple().to_string();
    sqlx::query(
        "INSERT INTO \"users\" (\"id\", \"password\", \"last_login\", \"username\", \"mobile_number\", \
         \"email\", \"display_name\", \"first_name\", \"last_name\", \"avatar\", \"avatar_asset_id\", \
         \"cover_image\", \"cover_image_asset_id\", \"date_joined\", \"created_at\", \"updated_at\", \
         \"last_location\", \"created_location\", \"is_superuser\", \"is_managed\", \
         \"is_password_expired\", \"is_active\", \"is_staff\", \"is_email_verified\", \
         \"is_password_autoset\", \"is_password_reset_required\", \"token\", \"last_active\", \
         \"last_login_time\", \"last_logout_time\", \"last_login_ip\", \"last_logout_ip\", \
         \"last_login_medium\", \"last_login_uagent\", \"token_updated_at\", \"is_bot\", \"bot_type\", \
         \"user_timezone\", \"is_email_valid\", \"masked_at\") \
         VALUES ($1, $2, NULL, $3, NULL, $4, $5, $6, $7, '', NULL, NULL, NULL, $8, $8, $8, \
         '', '', FALSE, FALSE, FALSE, TRUE, FALSE, FALSE, FALSE, FALSE, '', $8, NULL, NULL, '', '', \
         'email', '', NULL, FALSE, NULL, 'UTC', FALSE, NULL)",
    )
    .bind(id)
    .bind(password_hash)
    .bind(&username)
    .bind(email)
    .bind(display_name_for(email))
    .bind(first_name)
    .bind(last_name)
    .bind(now)
    .execute(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(id)
}

/// `post_save` receiver `create_user_notification` (`user.py:306-320`): a
/// preferences row on every non-bot create (anonymous save → NULL audit).
async fn insert_notification_prefs(
    pool: &PgPool,
    now: DateTime<Utc>,
    user_id: &Uuid,
) -> Result<(), Denial> {
    sqlx::query(
        "INSERT INTO \"user_notification_preferences\" (\"id\", \"created_at\", \"updated_at\", \
         \"created_by_id\", \"updated_by_id\", \"deleted_at\", \"user_id\", \"workspace_id\", \
         \"project_id\", \"property_change\", \"state_change\", \"comment\", \"mention\", \
         \"issue_completed\") \
         VALUES ($1, $2, $2, NULL, NULL, NULL, $3, NULL, NULL, TRUE, TRUE, TRUE, TRUE, TRUE)",
    )
    .bind(Uuid::new_v4())
    .bind(now)
    .bind(user_id)
    .execute(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(())
}

/// `Profile.objects.create(user, company_name)` (`admin.py:218`) with the
/// model defaults (`user.py:200-278` + `color.py:9-14`).
async fn insert_signup_profile(
    pool: &PgPool,
    now: DateTime<Utc>,
    user_id: &Uuid,
    company_name: &str,
) -> Result<(), Denial> {
    sqlx::query(
        "INSERT INTO \"profiles\" (\"id\", \"created_at\", \"updated_at\", \
         \"user_id\", \"theme\", \"is_app_rail_docked\", \
         \"is_tour_completed\", \"onboarding_step\", \"use_case\", \"role\", \"is_onboarded\", \
         \"last_workspace_id\", \"billing_address_country\", \"billing_address\", \
         \"has_billing_address\", \"company_name\", \"notification_view_mode\", \
         \"is_smooth_cursor_enabled\", \"is_mobile_onboarded\", \"mobile_onboarding_step\", \
         \"mobile_timezone_auto_set\", \"language\", \"start_of_the_week\", \"goals\", \
         \"background_color\", \"is_navigation_tour_completed\", \"has_marketing_email_consent\", \
         \"is_subscribed_to_changelog\", \"product_tour\", \"settings\") \
         VALUES ($1, $2, $2, $3, '{}', TRUE, FALSE, \
         '{\"profile_complete\": false, \"workspace_create\": false, \
           \"workspace_invite\": false, \"workspace_join\": false}', \
         NULL, NULL, FALSE, NULL, 'INDIA', NULL, FALSE, $4, 'full', FALSE, FALSE, \
         '{\"profile_complete\": false, \"workspace_create\": false, \"workspace_join\": false}', \
         FALSE, 'en', 0, '{}', $5, FALSE, FALSE, FALSE, \
         '{\"work_items\": false, \"cycles\": false, \"modules\": false, \"intake\": false, \
           \"pages\": false}', '{}')",
    )
    .bind(Uuid::new_v4())
    .bind(now)
    .bind(user_id)
    .bind(company_name)
    .bind(background_color())
    .execute(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(())
}

/// The stamp `save()` (`admin.py:220-226,347-353`): login stamps plus the
/// `User.save` token rotation (`token_updated_at` set → fresh 64-hex
/// `token`). `None` ip/user-agent renders NULL: both columns are NOT NULL,
/// so such a request raises — the 500 Python answers uncaught.
async fn apply_login_stamps(
    pool: &PgPool,
    user_id: &Uuid,
    now: DateTime<Utc>,
    ip: Option<&str>,
    user_agent: Option<&str>,
) -> Result<(), Denial> {
    let rotated = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    sqlx::query(
        "UPDATE \"users\" SET \"is_active\" = TRUE, \"last_active\" = $1, \
         \"last_login_time\" = $1, \"last_login_ip\" = $2, \"last_login_uagent\" = $3, \
         \"token_updated_at\" = $1, \"token\" = $4, \"updated_at\" = $1 \
         WHERE \"id\" = $5",
    )
    .bind(now)
    .bind(ip)
    .bind(user_agent)
    .bind(rotated)
    .bind(user_id)
    .execute(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(())
}

/// Request client identity for the login stamps: `get_client_ip` (first
/// forwarded entry, else the peer) plus the raw `User-Agent`. Either may
/// be absent; the stamp columns render NULL for `None`.
fn client_ip_ua(headers: &HeaderMap, peer: Option<&str>) -> (Option<String>, Option<String>) {
    let forwarded = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok());
    let ip = forms::client_ip(forwarded, peer);
    let user_agent = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    (ip, user_agent)
}

/// The `user_logged_in` signal (`update_last_login`): `login()` sets
/// `last_login = now` on every login. Replayed as an explicit UPDATE —
/// the same shape as `auth_session::email::stamp_last_login` — and kept
/// OUT of [`apply_login_stamps`] (the `user.save()` replay) on purpose:
/// `login()` runs after the admin insert and the instance save, so on the
/// telemetry-500 path Python has stamped the user but `last_login` stays
/// NULL (`login()` never runs). This lives in [`finish_login`] (the
/// `user_login` replay) so both success paths and that failure path
/// match.
async fn stamp_last_login(pool: &PgPool, user_id: &Uuid, now: DateTime<Utc>) -> Result<(), Denial> {
    sqlx::query("UPDATE \"users\" SET \"last_login\" = $1 WHERE \"id\" = $2")
        .bind(now)
        .bind(user_id)
        .execute(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(())
}

/// `InstanceAdmin.objects.create(user, instance)` (`admin.py:229`): role 20
/// + unverified (column defaults), NULL audit (anonymous save).
async fn insert_signup_admin(
    pool: &PgPool,
    now: DateTime<Utc>,
    user_id: &Uuid,
    instance_id: &Uuid,
) -> Result<(), Denial> {
    sqlx::query(
        "INSERT INTO instance_admins (id, created_at, updated_at, deleted_at, role, is_verified, created_by_id, updated_by_id, user_id, instance_id) VALUES ($1, $2, $2, NULL, 20, FALSE, NULL, NULL, $3, $4)",
    )
    .bind(Uuid::new_v4())
    .bind(now)
    .bind(user_id)
    .bind(instance_id)
    .execute(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(())
}

/// `instance.save()` (`admin.py:231-234`): setup done, the company name
/// verbatim, telemetry as coerced, both audit columns NULLed (the save
/// runs anonymous, before `user_login`). `Invalid` raises on save in
/// Python (500 *after* the earlier rows committed), so the caller returns
/// before this UPDATE runs.
async fn apply_instance_setup(
    pool: &PgPool,
    instance_id: &Uuid,
    now: DateTime<Utc>,
    company_name: &str,
    telemetry: bool,
) -> Result<(), Denial> {
    sqlx::query(
        "UPDATE instances SET is_setup_done = TRUE, instance_name = $1, is_telemetry_enabled = $2, updated_at = $3, created_by_id = NULL, updated_by_id = NULL WHERE id = $4",
    )
    .bind(company_name)
    .bind(telemetry)
    .bind(now)
    .bind(instance_id)
    .execute(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(())
}

/// Shared success tail: the `user_login` replay (`login()` → the session
/// row) plus the 302. The `user.save()` stamps are NOT applied here: each
/// caller runs [`apply_login_stamps`] at its Python position (sign-up
/// stamps before the admin insert, `admin.py:220-229`; sign-in stamps
/// before `user_login()`, `admin.py:347-356`). `ip`/`user_agent` arrive
/// precomputed ([`client_ip_ua`]) so stamps and `device_info` share one
/// reading of the request.
#[allow(clippy::too_many_arguments)]
async fn finish_login(
    state: &AppState,
    pool: &PgPool,
    headers: &HeaderMap,
    ip: Option<String>,
    user_agent: Option<String>,
    user_id: Uuid,
    password_field: &str,
    base: &str,
    location: &str,
) -> Result<Response, Denial> {
    let now = Utc::now();
    stamp_last_login(pool, &user_id, now).await?;
    let settings = state.settings();
    let info = forms::device_info(user_agent.as_deref().unwrap_or(""), ip.as_deref(), base);
    let cookie_header = headers
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let presented =
        session_kernel::cookie_value(cookie_header, session_kernel::ADMIN_SESSION_COOKIE_NAME);
    let cookies = issue_admin_session(
        pool,
        presented.as_deref(),
        user_id,
        password_field,
        info,
        settings.secret_key.as_bytes(),
        &cookie_ctx(state),
        settings.session.admin_cookie_age_secs,
        now.timestamp(),
    )
    .await
    .map_err(|_| Denial::ServerError)?;
    redirect_response(location, cookies)
}

/// `POST /api/instances/admins/sign-up/` (`admin.py:89-239`): the CSRF
/// gate, then the kernel branch evaluation over live snapshots, then — on
/// success only — the writes in Python order (each its own autocommit
/// statement; no `transaction.atomic` in `admin.py`), session issuance,
/// and the 302. `invalidate_cache` is freshness-only (see module docs).
async fn sign_up(State(state): State<AppState>, req: Request) -> Result<Response, Denial> {
    let (parts, body) = req.into_parts();
    let headers = &parts.headers;
    let body = axum::body::to_bytes(body, 1024 * 1024)
        .await
        .unwrap_or_default();
    let peer = parts
        .extensions
        .get::<ConnectInfo<SocketAddr>>()
        .map(|info| info.0.ip().to_string());
    let pairs = form_pairs(content_type_of(headers), &body);
    if csrf_gate(headers, &pairs).is_err() {
        return Ok(csrf_failure_response(&state));
    }
    let pool = pools(&state)?;
    let base = admin_base(&state)?;
    let form = forms::SignupForm {
        email: form_last(&pairs, "email"),
        password: form_last(&pairs, "password"),
        first_name: form_last(&pairs, "first_name"),
        last_name: form_last(&pairs, "last_name").unwrap_or_default(),
        company_name: form_last(&pairs, "company_name").unwrap_or_default(),
        telemetry: match form_last(&pairs, "is_telemetry_enabled") {
            Some(text) => forms::TelemetryRaw::Text(text),
            None => forms::TelemetryRaw::Missing,
        },
    };
    // Snapshots in Python-evaluation order: instance, any-admin, then
    // (presence is form-only) the normalized email's validity, user
    // existence, and the raw password's zxcvbn score.
    let instance_id = instance_first(pool).await?;
    let admin_exists = any_admin_exists(pool).await?;
    let email = form
        .email
        .as_deref()
        .map(forms::normalize_email)
        .unwrap_or_default();
    let email_valid = forms::email_is_valid(&email);
    let user_exists = user_exists_by_email(pool, &email).await?;
    let score = form.password.as_deref().map(password_score).unwrap_or(0);
    let outcome = forms::decide_signup(
        &base,
        &form,
        &forms::SignupSnapshot {
            instance_exists: instance_id.is_some(),
            admin_exists,
            user_exists,
            email_valid,
            password_score: score,
        },
    );
    if outcome.code.is_some() {
        return redirect_response(&outcome.location, Vec::new());
    }
    // Success: users → (post_save) prefs → profile → stamps → admin →
    // instance → session. `code: None` proves presence, so the first-name
    // and password unwraps hold.
    let password = form.password.clone().ok_or(Denial::ServerError)?;
    let first_name = form.first_name.clone().ok_or(Denial::ServerError)?;
    let password_hash = encode_password(&password);
    let now = Utc::now();
    let user_id = insert_signup_user(
        pool,
        now,
        &email,
        &first_name,
        &form.last_name,
        &password_hash,
    )
    .await?;
    insert_notification_prefs(pool, now, &user_id).await?;
    insert_signup_profile(pool, now, &user_id, &form.company_name).await?;
    // `user.save()` (`admin.py:220-227`) lands BEFORE the admin insert
    // (`admin.py:229`): a NULL User-Agent 500s with NO admin row (a retry
    // then 5180s, not 5150s), and a bad-telemetry 500 leaves a stamped
    // user — both exactly as in Python.
    let (ip, user_agent) = client_ip_ua(headers, peer.as_deref());
    apply_login_stamps(pool, &user_id, now, ip.as_deref(), user_agent.as_deref()).await?;
    let instance_id = instance_id.ok_or(Denial::ServerError)?;
    insert_signup_admin(pool, now, &user_id, &instance_id).await?;
    let forms::TelemetryStored::Bool(telemetry) = forms::coerce_telemetry(&form.telemetry) else {
        return Err(Denial::ServerError);
    };
    apply_instance_setup(pool, &instance_id, now, &form.company_name, telemetry).await?;
    finish_login(
        &state,
        pool,
        headers,
        ip,
        user_agent,
        user_id,
        &password_hash,
        &base,
        &outcome.location,
    )
    .await
}

/// `POST /api/instances/admins/sign-in/` (`admin.py:242-358`): same shell
/// as [`sign_up`]; the membership check has no role gate (any live
/// `InstanceAdmin` row for `(instance, user)` passes).
async fn sign_in(State(state): State<AppState>, req: Request) -> Result<Response, Denial> {
    let (parts, body) = req.into_parts();
    let headers = &parts.headers;
    let body = axum::body::to_bytes(body, 1024 * 1024)
        .await
        .unwrap_or_default();
    let peer = parts
        .extensions
        .get::<ConnectInfo<SocketAddr>>()
        .map(|info| info.0.ip().to_string());
    let pairs = form_pairs(content_type_of(headers), &body);
    if csrf_gate(headers, &pairs).is_err() {
        return Ok(csrf_failure_response(&state));
    }
    let pool = pools(&state)?;
    let base = admin_base(&state)?;
    let form = forms::SigninForm {
        email: form_last(&pairs, "email"),
        password: form_last(&pairs, "password"),
    };
    let instance_id = instance_first(pool).await?;
    let email = form
        .email
        .as_deref()
        .map(forms::normalize_email)
        .unwrap_or_default();
    let email_valid = forms::email_is_valid(&email);
    let lookup = signin_user_lookup(pool, &email).await?;
    let user = match &lookup {
        None => None,
        Some((id, is_active, password_field)) => {
            let password_ok = form
                .password
                .as_deref()
                .is_some_and(|candidate| forms::check_password(candidate, password_field));
            let membership = match instance_id {
                Some(instance) => is_instance_admin(pool, &instance, id).await?,
                None => false,
            };
            Some(forms::SigninUserState {
                is_active: *is_active,
                password_ok,
                is_instance_admin: membership,
            })
        }
    };
    let outcome = forms::decide_signin(
        &base,
        &form,
        &forms::SigninSnapshot {
            instance_exists: instance_id.is_some(),
            email_valid,
            user,
        },
    );
    if outcome.code.is_some() {
        return redirect_response(&outcome.location, Vec::new());
    }
    let (user_id, _, password_field) = lookup.ok_or(Denial::ServerError)?;
    // `user.save()` (`admin.py:347-353`) lands BEFORE `user_login()`
    // (`admin.py:356`), as it always did when the stamps lived in
    // `finish_login`.
    let now = Utc::now();
    let (ip, user_agent) = client_ip_ua(headers, peer.as_deref());
    apply_login_stamps(pool, &user_id, now, ip.as_deref(), user_agent.as_deref()).await?;
    finish_login(
        &state,
        pool,
        headers,
        ip,
        user_agent,
        user_id,
        &password_field,
        &base,
        &outcome.location,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn denial_body(denial: Denial) -> (StatusCode, String) {
        denial.status_and_body()
    }

    fn admin_app() -> axum::Router {
        // Port 1 is never bound, so proxied (unowned/non-canonical) requests
        // fail closed with 502 instead of reaching a dev server.
        crate::routes::with_routes(
            AppState::with_edge(
                "0.1.0",
                crate::edge::EdgeHandle::for_tests("http://127.0.0.1:1"),
            ),
            super::routes(),
        )
    }

    async fn admin_status(method: &str, uri: &str) -> StatusCode {
        use tower::ServiceExt;

        let request = axum::http::Request::builder()
            .method(method)
            .uri(uri)
            .body(axum::body::Body::empty())
            .expect("request");
        admin_app().oneshot(request).await.expect("serve").status()
    }

    /// Non-canonical UUID spellings in the path proxy to Django before any
    /// auth runs (502 fail-closed with the test edge), so Django's resolver
    /// 404 answers byte for byte rather than Rust's 401/403/204
    /// (PIDASHCONV-1132).
    #[tokio::test]
    async fn noncanonical_uuid_segments_proxy_before_auth() {
        for segment in [
            "12345678-1234-1234-1234-1234567890ab".to_uppercase(),
            "123456781234123412341234567890ab".to_owned(),
            "%7B12345678-1234-1234-1234-1234567890ab%7D".to_owned(),
            "urn:uuid:12345678-1234-1234-1234-1234567890ab".to_owned(),
        ] {
            assert_eq!(
                admin_status("DELETE", &format!("/api/instances/admins/{segment}/")).await,
                StatusCode::BAD_GATEWAY,
                "{segment}"
            );
        }
    }

    /// A canonical spelling still serves from Rust, never a proxy 502. The
    /// test edge has no database pool and `delete_admin` resolves it before
    /// the auth gate, so the handler stops at its own 500 rather than
    /// proxying.
    #[tokio::test]
    async fn canonical_uuid_segment_is_served_not_proxied() {
        assert_eq!(
            admin_status(
                "DELETE",
                "/api/instances/admins/12345678-1234-1234-1234-1234567890ab/"
            )
            .await,
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }

    #[test]
    fn csrf_token_source_prefers_the_form_field() {
        let pairs = vec![("csrfmiddlewaretoken".to_owned(), "from-post".to_owned())];
        assert_eq!(
            csrf_token_source(&pairs, Some("from-header")),
            Some(("from-post".to_owned(), TokenSource::Post))
        );
        let empty: Vec<(String, String)> = Vec::new();
        assert_eq!(
            csrf_token_source(&empty, Some("h")),
            Some(("h".to_owned(), TokenSource::Header))
        );
        // An empty header is still a (failing) token, not "missing".
        assert_eq!(
            csrf_token_source(&empty, Some("")),
            Some(("".to_owned(), TokenSource::Header))
        );
        assert_eq!(csrf_token_source(&empty, None), None);
        // An empty form field falls through to the header.
        let blank = vec![("csrfmiddlewaretoken".to_owned(), String::new())];
        assert_eq!(
            csrf_token_source(&blank, Some("h")),
            Some(("h".to_owned(), TokenSource::Header))
        );
        assert_eq!(csrf_token_source(&blank, None), None);
    }

    #[test]
    fn csrf_gate_round_trip() {
        // No cookie → NoCookie, even with a token present.
        let mut headers = HeaderMap::new();
        let pairs = vec![("csrfmiddlewaretoken".to_owned(), "x".repeat(64))];
        assert_eq!(csrf_gate(&headers, &pairs), Err(CsrfDenial::NoCookie));
        // Cookie + masked token → pass.
        let secret = csrf_kernel::new_secret();
        let token = csrf_kernel::mask_secret(&secret).expect("mask");
        headers.insert(
            header::COOKIE,
            format!("csrftoken={secret}").parse().expect("cookie"),
        );
        let pairs = vec![("csrfmiddlewaretoken".to_owned(), token)];
        assert_eq!(csrf_gate(&headers, &pairs), Ok(()));
        // Cookie + wrong token → BadToken.
        let pairs = vec![("csrfmiddlewaretoken".to_owned(), "x".repeat(64))];
        assert_eq!(csrf_gate(&headers, &pairs), Err(CsrfDenial::BadToken));
        // Cookie but no token anywhere → BadToken.
        let empty: Vec<(String, String)> = Vec::new();
        assert_eq!(csrf_gate(&headers, &empty), Err(CsrfDenial::BadToken));
    }

    #[test]
    fn csrf_check_token_renders_django_reasons() {
        let secret = csrf_kernel::new_secret();
        let masked = csrf_kernel::mask_secret(&secret).expect("mask");
        assert!(csrf_check_token(&secret, &masked, TokenSource::Post).is_ok());
        assert_eq!(
            csrf_check_token(&secret, &"x".repeat(64), TokenSource::Post),
            Err("CSRF token from POST incorrect.".to_owned())
        );
        assert!(csrf_check_token(&secret, "short", TokenSource::Header)
            .unwrap_err()
            .contains("the 'X-Csrftoken' HTTP header"));
    }

    #[test]
    fn form_pairs_only_reads_forms_last_wins() {
        let pairs = form_pairs(Some("application/x-www-form-urlencoded"), b"a=1&a=2");
        assert_eq!(form_last(&pairs, "a").as_deref(), Some("2"));
        assert!(form_pairs(Some("application/json"), b"a=1").is_empty());
        assert!(form_pairs(None, b"a=1").is_empty());
        let pairs = form_pairs(
            Some("application/x-www-form-urlencoded; charset=UTF-8"),
            b"a=1",
        );
        assert_eq!(form_last(&pairs, "a").as_deref(), Some("1"));
    }

    #[test]
    fn denial_bodies_match_drf_byte_for_byte() {
        assert_eq!(
            denial_body(Denial::Unauthorized),
            (
                StatusCode::UNAUTHORIZED,
                r#"{"detail":"Authentication credentials were not provided."}"#.to_owned()
            )
        );
        assert_eq!(
            denial_body(Denial::DefaultForbidden),
            (
                StatusCode::FORBIDDEN,
                r#"{"detail":"You do not have permission to perform this action."}"#.to_owned()
            )
        );
        assert_eq!(
            denial_body(Denial::NoInstance),
            (
                StatusCode::FORBIDDEN,
                r#"{"error":"Instance is not registered yet"}"#.to_owned()
            )
        );
        assert_eq!(
            denial_body(Denial::NotFound),
            (
                StatusCode::NOT_FOUND,
                r#"{"error":"The required object does not exist."}"#.to_owned()
            )
        );
        assert_eq!(
            denial_body(Denial::BadError("Email is required".to_owned())),
            (
                StatusCode::BAD_REQUEST,
                r#"{"error":"Email is required"}"#.to_owned()
            )
        );
        assert_eq!(
            denial_body(Denial::BadDetail("JSON parse error.".to_owned())),
            (
                StatusCode::BAD_REQUEST,
                r#"{"detail":"JSON parse error."}"#.to_owned()
            )
        );
        assert_eq!(
            denial_body(Denial::InvalidPayload),
            (
                StatusCode::BAD_REQUEST,
                r#"{"error":"The payload is not valid"}"#.to_owned()
            )
        );
        assert_eq!(
            denial_body(Denial::ServerError),
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                r#"{"error":"Something went wrong please try again later"}"#.to_owned()
            )
        );
    }

    #[test]
    fn sign_out_redirect_shapes_match_base_host_plus_validator() {
        // `ADMIN_BASE_URL + ADMIN_BASE_PATH`, trailing slash stripped.
        assert_eq!(
            sign_out_redirect(
                Some("https://admin.example.com"),
                Some("https://app.example.com"),
                None,
                "/god-mode/"
            )
            .expect("redirect"),
            "https://admin.example.com/god-mode"
        );
        // Empty admin URL falls back to `WEB_URL or APP_BASE_URL`.
        assert_eq!(
            sign_out_redirect(
                Some(""),
                Some("https://app.example.com"),
                None,
                "/god-mode/"
            )
            .expect("redirect"),
            "https://app.example.com/god-mode"
        );
        assert_eq!(
            sign_out_redirect(None, None, Some("https://app.example.com"), "/god-mode/")
                .expect("redirect"),
            "https://app.example.com/god-mode"
        );
        // The path is normalized exactly like `base_host`.
        assert_eq!(
            sign_out_redirect(None, Some("https://x.test"), None, "god-mode").expect("redirect"),
            "https://x.test/god-mode"
        );
        // No base anywhere: Python raises `TypeError` out of the view (500).
        assert!(sign_out_redirect(None, Some(""), None, "/god-mode/").is_err());
    }

    #[test]
    fn client_ip_prefers_first_forwarded_entry() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-forwarded-for",
            "203.0.113.7, 70.41.3.18".parse().unwrap(),
        );
        assert_eq!(client_ip(&headers), "203.0.113.7");
        assert_eq!(client_ip(&HeaderMap::new()), "");
    }

    #[test]
    fn avatar_url_follows_the_property_branches() {
        let asset = Some(Uuid::nil());
        let ws = Some(Uuid::nil());
        // A resolved static-entity row renders its URL (soft-deleted rows
        // resolve too — the join is unscoped, like `_base_manager`).
        assert_eq!(
            avatar_url(
                asset,
                asset,
                Some("USER_AVATAR".to_owned()),
                None,
                None,
                None,
                None,
                ""
            ),
            AssetUrl::Present(Some(
                "/api/assets/v2/static/00000000-0000-0000-0000-000000000000/".to_owned()
            ))
        );
        // NULL/other entities pass None through (no fallback to the raw
        // avatar); a NULL workspace on an attachment skips the key.
        assert_eq!(
            avatar_url(
                asset,
                asset,
                None,
                None,
                None,
                None,
                None,
                "https://cdn/x.png"
            ),
            AssetUrl::Present(None)
        );
        assert_eq!(
            avatar_url(
                asset,
                asset,
                Some("ISSUE_ATTACHMENT".to_owned()),
                None,
                None,
                None,
                None,
                "https://cdn/x.png"
            ),
            AssetUrl::Skip
        );
        // A workspace-backed attachment renders its full URL.
        assert_eq!(
            avatar_url(
                asset,
                asset,
                Some("ISSUE_ATTACHMENT".to_owned()),
                ws,
                None,
                None,
                Some("acme".to_owned()),
                "https://cdn/x.png"
            ),
            AssetUrl::Present(Some(
                "/api/assets/v2/workspaces/acme/projects/None/issues/None/attachments/00000000-0000-0000-0000-000000000000/"
                    .to_owned()
            ))
        );
        // A dangling FK renders null (`RelatedObjectDoesNotExist` is an
        // `ObjectDoesNotExist`, caught by DRF's `get_attribute`); only a
        // NULL FK falls back to the raw avatar.
        assert_eq!(
            avatar_url(
                asset,
                None,
                None,
                None,
                None,
                None,
                None,
                "https://cdn/x.png"
            ),
            AssetUrl::Present(None)
        );
        assert_eq!(
            avatar_url(None, None, None, None, None, None, None, ""),
            AssetUrl::Present(None)
        );
        assert_eq!(
            avatar_url(
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                "https://cdn/x.png"
            ),
            AssetUrl::Present(Some("https://cdn/x.png".to_owned()))
        );
    }

    fn shape_fixture() -> UserShape {
        UserShape {
            id: Some(Uuid::nil()),
            first_name: Some("Ada".to_owned()),
            last_name: Some("Lovelace".to_owned()),
            avatar: Some(String::new()),
            avatar_asset_id: None,
            avatar_asset_row: None,
            avatar_asset_entity: None,
            avatar_asset_workspace: None,
            avatar_asset_project: None,
            avatar_asset_issue: None,
            avatar_asset_workspace_slug: None,
            cover_image: None,
            date_joined: Some(
                DateTime::parse_from_rfc3339("2026-01-15T12:30:45Z")
                    .expect("fixture datetime")
                    .with_timezone(&Utc),
            ),
            display_name: Some("Ada Lovelace".to_owned()),
            email: Some("admin@acme.test".to_owned()),
            is_active: Some(true),
            is_bot: Some(false),
            is_email_verified: Some(false),
            user_timezone: Some("UTC".to_owned()),
            username: Some("ada".to_owned()),
            is_password_autoset: Some(false),
            last_login_medium: Some("email".to_owned()),
        }
    }

    #[test]
    fn me_shape_carries_the_sixteen_meta_keys() {
        let user = shape_fixture();
        let body = render_me(&user, &chrono_tz::UTC).expect("renders");
        let keys: std::collections::BTreeSet<&str> = body
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        let expected: std::collections::BTreeSet<&str> = [
            "id",
            "avatar",
            "avatar_url",
            "cover_image",
            "date_joined",
            "display_name",
            "email",
            "first_name",
            "last_name",
            "is_active",
            "is_bot",
            "is_email_verified",
            "user_timezone",
            "username",
            "is_password_autoset",
        ]
        .into_iter()
        .collect();
        assert_eq!(keys, expected);
        assert_eq!(body["email"], "admin@acme.test");
        assert_eq!(body["date_joined"], "2026-01-15T12:30:45Z");
    }

    #[test]
    fn parse_json_body_matches_drf_parser_edges() {
        use axum::http::header;
        let json_headers = || {
            let mut headers = HeaderMap::new();
            headers.insert(header::CONTENT_TYPE, "application/json".parse().unwrap());
            headers
        };
        // Valid objects pass through.
        assert!(parse_json_body(&json_headers(), br#"{"email":"a@b.c"}"#, 5_242_880).is_ok());
        // Empty body is `{}` without parsing (DRF `_parse`: stream None).
        let empty = parse_json_body(&json_headers(), b"", 5_242_880).expect("empty is {}");
        assert_eq!(empty.value(), &serde_json::json!({}));
        // Blank bodies are DRF `ParseError` with the CPython message.
        let err = parse_json_body(&json_headers(), b"   ", 5_242_880).expect_err("blank body 400s");
        let (_, body) = err.status_and_body();
        assert!(body.contains("column 4 (char 3)"), "{body}");
        // Non-JSON content-type with a body is 415.
        let mut plain = HeaderMap::new();
        plain.insert(header::CONTENT_TYPE, "text/plain".parse().unwrap());
        let err = parse_json_body(&plain, br#"{"email":"a@b.c"}"#, 5_242_880)
            .expect_err("text/plain 415s");
        let (status, body) = err.status_and_body();
        assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
        assert_eq!(
            body,
            r#"{"detail":"Unsupported media type \"text/plain\" in request."}"#
        );
        // Missing content-type with a body is 415 (Rust echoes "").
        let err = parse_json_body(&HeaderMap::new(), br#"{"email":"a@b.c"}"#, 5_242_880)
            .expect_err("missing 415s");
        let (status, _) = err.status_and_body();
        assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
        // Case-variant JSON serves (Django lowercases the base).
        let mut mixed = HeaderMap::new();
        mixed.insert(header::CONTENT_TYPE, "Application/JSON".parse().unwrap());
        assert!(parse_json_body(&mixed, br#"{"email":"a@b.c"}"#, 5_242_880).is_ok());
        // Wildcards serve (DRF `_MediaType.match` honors `*` either side).
        for ct in ["*/*", "application/*", "*/json"] {
            let mut wild = HeaderMap::new();
            wild.insert(header::CONTENT_TYPE, ct.parse().unwrap());
            assert!(
                parse_json_body(&wild, br#"{"email":"a@b.c"}"#, 5_242_880).is_ok(),
                "{ct}"
            );
        }
        // Non-object JSON has no `.get`: Django raises out of the view.
        assert!(matches!(
            parse_json_body(&json_headers(), br"[1]", 5_242_880),
            Err(Denial::ServerError)
        ));
        assert!(matches!(
            parse_json_body(&json_headers(), br"null", 5_242_880),
            Err(Denial::ServerError)
        ));
        // Malformed JSON carries CPython's exact text (PIDASHCONV-1128).
        let err =
            parse_json_body(&json_headers(), b"{bad}", 5_242_880).expect_err("malformed 400s");
        let (status, body) = err.status_and_body();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(
            body,
            r#"{"detail":"JSON parse error - Expecting property name enclosed in double quotes: line 1 column 2 (char 1)"}"#
        );
        // Non-UTF-8 is a codec `ParseError` 400 (DRF catches `ValueError`,
        // which includes `UnicodeDecodeError`), not a 500.
        let err = parse_json_body(&json_headers(), &[0xff], 5_242_880).expect_err("codec 400s");
        let (status, body) = err.status_and_body();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body.contains("codec can't decode byte 0xff"), "{body}");
    }

    #[test]
    fn parse_json_body_serves_forms_like_drf() {
        use axum::http::header;
        let headers_for = |content_type: &str| {
            let mut headers = HeaderMap::new();
            headers.insert(header::CONTENT_TYPE, content_type.parse().unwrap());
            headers
        };
        // Form bodies decode to last-wins text (probed end to end).
        let body = parse_json_body(
            &headers_for("application/x-www-form-urlencoded"),
            b"email=a@b.c&role=20",
            5_242_880,
        )
        .expect("form parses");
        assert!(body.is_form());
        assert_eq!(lookup_email(body.field("email")).expect("email"), "a@b.c");
        assert_eq!(
            role_from_value(body.field("role")).expect("role"),
            (20, serde_json::json!(20))
        );
        // Multipart bodies decode fields; framing failures are 400s.
        let multipart =
            b"--B\r\nContent-Disposition: form-data; name=\"email\"\r\n\r\na@b.c\r\n--B--\r\n";
        let body = parse_json_body(
            &headers_for("multipart/form-data; boundary=B"),
            multipart,
            5_242_880,
        )
        .expect("multipart parses");
        assert!(body.is_form());
        assert_eq!(lookup_email(body.field("email")).expect("email"), "a@b.c");
        let err = parse_json_body(&headers_for("multipart/form-data"), multipart, 5_242_880)
            .expect_err("missing boundary 400s");
        assert!(matches!(err, Denial::BadDetail(_)));
        // Upload caps escape the view into the base `handle_exception`
        // (generic 500) on every endpoint — including POST ones. The 400
        // page seen locally on POST is the Debug Toolbar re-parsing the
        // body in middleware (DEBUG-only; prod has no toolbar — probed
        // with DEBUG=False: 500 JSON on all five endpoints).
        let over = (0..1001)
            .map(|i| format!("u{i}=v"))
            .collect::<Vec<_>>()
            .join("&");
        assert!(matches!(
            parse_json_body(
                &headers_for("application/x-www-form-urlencoded"),
                over.as_bytes(),
                5_242_880
            ),
            Err(Denial::ServerError)
        ));
    }

    #[test]
    fn admin_files_email_looks_up_but_role_500s() {
        use super::super::form_body::{FormFile, FormValue};
        // A file email stringifies to its name in the lookup (probed:
        // truthy, then 404-or-hit downstream).
        let body = ParsedBody::form(vec![(
            "email".to_owned(),
            FormValue::File(FormFile {
                filename: "e.txt".to_owned(),
                size: 18,
            }),
        )]);
        assert_eq!(
            lookup_email(body.field("email")).expect("file email"),
            "e.txt"
        );
        // A file role is `int(file)` → `TypeError` → 500 however the
        // filename spells (probed with both numeric and junk names).
        for filename in ["20", "abc"] {
            let body = ParsedBody::form(vec![(
                "role".to_owned(),
                FormValue::File(FormFile {
                    filename: filename.to_owned(),
                    size: 1,
                }),
            )]);
            assert!(
                matches!(
                    role_from_value(body.field("role")),
                    Err(Denial::ServerError)
                ),
                "{filename}"
            );
        }
    }

    #[test]
    fn role_values_follow_the_orm_column() {
        let role = |raw: &str| {
            let data: Value = serde_json::from_str(raw).expect("json");
            role_from_value(ParsedBody::json(data).field("role"))
        };
        // Absent -> default 20; plain ints pass through. Each case
        // asserts `(insert int, 201 echo)`: the `choices` `ChoiceField`
        // echoes the raw input unless `str(value) == "20"` (probed).
        assert_eq!(role("{}").expect("default"), (20, serde_json::json!(20)));
        assert_eq!(
            role(r#"{"role":20}"#).expect("int"),
            (20, serde_json::json!(20))
        );
        assert_eq!(
            role(r#"{"role":5}"#).expect("low role"),
            (5, serde_json::json!(5))
        );
        // `bool` subclasses `int`: True -> 1, False -> 0; the echo keeps
        // the bools (`str(True)` is not `"20"`).
        assert_eq!(
            role(r#"{"role":true}"#).expect("bool"),
            (1, serde_json::json!(true))
        );
        assert_eq!(
            role(r#"{"role":false}"#).expect("bool"),
            (0, serde_json::json!(false))
        );
        // `int(value)` truncates every float toward zero, including
        // negatives (-0.5 -> 0); a parsed negative still 400s, but at the
        // INSERT CHECK, so the adapter returns it. Floats echo raw.
        assert_eq!(
            role(r#"{"role":20.0}"#).expect("integral float"),
            (20, serde_json::json!(20.0))
        );
        assert_eq!(
            role(r#"{"role":20.9}"#).expect("truncation"),
            (20, serde_json::json!(20.9))
        );
        assert_eq!(
            role(r#"{"role":-0.5}"#).expect("negative truncation"),
            (0, serde_json::json!(-0.5))
        );
        assert_eq!(
            role(r#"{"role":-1}"#).expect("negative parses"),
            (-1, serde_json::json!(-1))
        );
        // Truncation runs BEFORE the range check (PIDASHCONV-1120): the
        // boundary floats fit after `int()`, while floats that are still
        // out of range after truncation error like a DB overflow. The
        // echo stays raw.
        assert_eq!(
            role(r#"{"role":2147483647.5}"#).expect("max boundary float"),
            (2147483647, serde_json::json!(2147483647.5))
        );
        assert_eq!(
            role(r#"{"role":-2147483648.5}"#).expect("min boundary float"),
            (-2147483648, serde_json::json!(-2147483648.5))
        );
        assert!(role(r#"{"role":2147483648.0}"#).is_err());
        assert!(role(r#"{"role":-2147483649.0}"#).is_err());
        // Numeric strings parse with `int()` semantics (surrounding
        // whitespace tolerated); `"20.5"`/`"abc"` are `ValueError` -> 500.
        // Only the exact `"20"` echoes as an int; the rest echo raw.
        assert_eq!(
            role(r#"{"role":"20"}"#).expect("string"),
            (20, serde_json::json!(20))
        );
        assert_eq!(
            role(r#"{"role":" 20 "}"#).expect("padded"),
            (20, serde_json::json!(" 20 "))
        );
        assert_eq!(
            role(r#"{"role":"1_0"}"#).expect("underscores"),
            (10, serde_json::json!("1_0"))
        );
        assert_eq!(
            role(r#"{"role":"\u0662\u0660"}"#).expect("unicode digits"),
            (20, serde_json::json!("٢٠"))
        );
        assert_eq!(
            role(r#"{"role":"+20"}"#).expect("sign"),
            (20, serde_json::json!("+20"))
        );
        assert!(role(r#"{"role":"20.5"}"#).is_err());
        assert!(role(r#"{"role":"abc"}"#).is_err());
        assert!(role(r#"{"role":"1__2"}"#).is_err());
        assert!(role(r#"{"role":""}"#).is_err());
        assert!(role(r#"{"role":"0x10"}"#).is_err());
        // Leading zeros don't count toward the `i32` bound: `int()`
        // takes the value, not the width (`int("00000000001") == 1`
        // — probed). Only the exact `"20"` echoes as an int.
        assert_eq!(
            role(r#"{"role":"00000000001"}"#).expect("leading zeros"),
            (1, serde_json::json!("00000000001"))
        );
        assert_eq!(
            role(r#"{"role":"-00000000001"}"#).expect("negative zeros"),
            (-1, serde_json::json!("-00000000001"))
        );
        assert_eq!(
            role(r#"{"role":"00000000000000000000000000000000000000000000001"}"#)
                .expect("many zeros"),
            (
                1,
                serde_json::json!("00000000000000000000000000000000000000000000001")
            )
        );
        assert!(role(r#"{"role":"99999999999"}"#).is_err());
        // Explicit null is the NOT NULL column -> invalid-payload 400;
        // out-of-range ints and containers are generic 500s.
        assert!(matches!(
            role(r#"{"role":null}"#),
            Err(Denial::InvalidPayload)
        ));
        assert!(role(r#"{"role":2147483648}"#).is_err());
        assert!(role(r#"{"role":[20]}"#).is_err());
    }

    #[test]
    fn role_echo_matches_choicefield_over_raw_input() {
        let echo = |raw: &str| {
            let data: Value = serde_json::from_str(raw).expect("json");
            let (_, raw_echo) =
                role_from_value(ParsedBody::json(data).field("role")).expect("role");
            role_echo_literal(&raw_echo)
        };
        // Missing means the default 20; only `str(value) == "20"` maps to
        // the `20` number (int 20 and string "20" alike).
        assert_eq!(echo("{}"), "20");
        assert_eq!(echo(r#"{"role":20}"#), "20");
        assert_eq!(echo(r#"{"role":"20"}"#), "20");
        assert_eq!(echo(r#"{"role":1}"#), "1");
        assert_eq!(echo(r#"{"role":-0}"#), "0");
        // Floats echo in Python spelling, including exponents, which serde
        // alone would spell `1e-5`.
        assert_eq!(echo(r#"{"role":2147483647.5}"#), "2147483647.5");
        assert_eq!(echo(r#"{"role":20.0}"#), "20.0");
        assert_eq!(echo(r#"{"role":0.00001}"#), "1e-05");
        assert_eq!(echo(r#"{"role":1E-5}"#), "1e-05");
        assert_eq!(echo(r#"{"role":-0.5}"#), "-0.5");
        // Bools echo as JSON bools, not 1/0.
        assert_eq!(echo(r#"{"role":true}"#), "true");
        assert_eq!(echo(r#"{"role":false}"#), "false");
        // Non-canonical numeric strings echo as strings.
        assert_eq!(echo(r#"{"role":" 20 "}"#), r#"" 20 ""#);
        assert_eq!(echo(r#"{"role":"+20"}"#), r#""+20""#);
        assert_eq!(echo(r#"{"role":"020"}"#), r#""020""#);
        let nbsp = format!("{{\"role\":\"{}20\"}}", '\u{a0}');
        assert_eq!(echo(&nbsp), format!("\"{}20\"", '\u{a0}'));
    }

    #[test]
    fn role_echo_splice_replaces_only_the_role_slot() {
        let body = serde_json::json!({"id": "abc", "role": 20, "user_detail": {"email": "a@b.c"}});
        let out = splice_role_echo(&body, "2147483647.5");
        assert_eq!(
            out,
            r#"{"id":"abc","role":2147483647.5,"user_detail":{"email":"a@b.c"}}"#
        );
        // String echoes stay quoted; key order is untouched; no sentinel
        // survives (this also pins `ROLE_ECHO_QUOTED` against serde's
        // escaping of the sentinel).
        let out = splice_role_echo(&body, r#"" 20 ""#);
        assert!(out.contains(r#""role":" 20 ""#), "{out}");
        assert!(!out.contains("u0000"), "{out}");
    }

    #[test]
    fn lookup_email_matches_request_data_truthiness() {
        let email = |raw: &str| {
            let data: Value = serde_json::from_str(raw).expect("json");
            lookup_email(ParsedBody::json(data).field("email"))
        };
        // `if not email`: missing, null, empty string, false, zero, and
        // EMPTY containers all 400.
        for raw in [
            "{}",
            r#"{"email":null}"#,
            r#"{"email":""}"#,
            r#"{"email":false}"#,
            r#"{"email":0}"#,
            r#"{"email":0.0}"#,
            r#"{"email":[]}"#,
            r#"{"email":{}}"#,
        ] {
            assert!(matches!(email(raw), Err(Denial::BadError(_))), "{raw}");
        }
        // Everything else reaches the lookup stringified like
        // `CharField.get_prep_value` (`True` -> `"True"`).
        assert_eq!(email(r#"{"email":"a@b.c"}"#).expect("str"), "a@b.c");
        assert_eq!(email(r#"{"email":true}"#).expect("bool"), "True");
        assert_eq!(email(r#"{"email":20}"#).expect("int"), "20");
        assert_eq!(email(r#"{"email":[1]}"#).expect("list"), "[1]");
    }

    #[test]
    fn routes_register_without_conflict() {
        // Building the router proves the static `me/` route coexists with
        // the `{pk}/` capture (static wins) instead of panicking.
        let _ = routes();
    }

    #[test]
    fn session_anonymous_body_is_the_bare_flag() {
        // `json_response` serializes compact (`COMPACT_JSON`), so the anon
        // session body is exactly these bytes on the wire.
        assert_eq!(
            serde_json::json!({"is_authenticated": false}).to_string(),
            r#"{"is_authenticated":false}"#
        );
    }
}
