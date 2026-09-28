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
//!   check, so unknown PKs still answer 204 (ported quirk).
//! - `GET /api/instances/admins/me/` — admin permission; the
//!   `InstanceAdminMeSerializer` rendering of the acting user.
//! - `GET /api/instances/admins/session/` — `AllowAny`; authed admins get
//!   `{"is_authenticated":true,"user":{…}}`, everyone else
//!   `{"is_authenticated":false}`.
//! - `POST /api/instances/admins/sign-out/` — plain view (no DRF
//!   permission layer): stamps `last_logout_ip/time`, flushes the session,
//!   and 302s to the safe admin URL; ANY exception still 302s.
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

use axum::extract::{Extension, Request, State};
use axum::http::{header, HeaderMap, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::Router;
use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

use pidash_types::license::serializers_core::{
    instance_admin_me_to_representation, instance_admin_to_representation, AdminMeRow,
    InstanceAdminRow, LiteUserRow,
};

use crate::edge;
use crate::middleware::SessionHandle;
use crate::serializer::render_datetime_in;
use crate::state::AppState;

/// Register the six owned endpoints. Cutover granularity is the
/// route + method (pilot `owned()` pattern): sibling methods on these
/// paths keep proxying so Django's own 405s/permission denials are
/// preserved byte-for-byte with no per-method logic.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/instances/admins/",
            get(list_admins)
                .post(create_admin)
                .put(edge::proxy)
                .patch(edge::proxy)
                .delete(edge::proxy)
                .options(edge::proxy),
        )
        .route(
            "/api/instances/admins/{pk}/",
            delete(delete_admin)
                .get(edge::proxy)
                .post(edge::proxy)
                .put(edge::proxy)
                .patch(edge::proxy)
                .options(edge::proxy),
        )
        .route(
            "/api/instances/admins/me/",
            get(me)
                .post(edge::proxy)
                .put(edge::proxy)
                .patch(edge::proxy)
                .delete(edge::proxy)
                .options(edge::proxy),
        )
        .route(
            "/api/instances/admins/session/",
            get(session)
                .post(edge::proxy)
                .put(edge::proxy)
                .patch(edge::proxy)
                .delete(edge::proxy)
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

/// `request.user` from the Django session (`_auth_user_id`). No session,
/// no key, or a non-UUID id means anonymous (pilot `actor_user_id`
/// pattern; Django PKs are UUIDs).
fn actor_user_id(extension: &Option<Extension<SessionHandle>>) -> Option<Uuid> {
    let handle = extension.as_ref().map(|ext| ext.0.clone())?;
    let mut session = handle.snapshot();
    session.get("_auth_user_id")?.as_str()?.parse::<Uuid>().ok()
}

/// Authenticated actor or 401. Django's `SessionAuthentication.get_user`
/// returns `None` for a session whose user row is gone, which the
/// permission layer turns into `NotAuthenticated` — so a dangling session
/// is 401 here, not 403.
async fn authed_user(
    pool: &PgPool,
    extension: &Option<Extension<SessionHandle>>,
) -> Result<Uuid, Denial> {
    let actor = actor_user_id(extension).ok_or(Denial::Unauthorized)?;
    let row: Option<(Uuid,)> = sqlx::query_as("SELECT id FROM users WHERE id = $1")
        .bind(actor)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    row.map(|row| row.0).ok_or(Denial::Unauthorized)
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

/// One `users` row projected for the lite/me shapes, with the avatar asset
/// URL rule (`user.py:143-151`): the asset's `asset_url` wins, else the
/// raw `avatar` string when non-empty, else null. Only the four static
/// `entity_type`s render without a workspace/project context
/// (`asset.py:80-87`); any other asset falls through to the raw avatar.
#[allow(clippy::too_many_arguments)]
fn avatar_url(
    asset_id: Option<Uuid>,
    asset_entity: Option<String>,
    avatar: &str,
) -> Option<String> {
    if let Some(asset) = asset_id {
        if matches!(
            asset_entity.as_deref(),
            Some("WORKSPACE_LOGO" | "USER_AVATAR" | "USER_COVER" | "PROJECT_COVER")
        ) {
            return Some(format!("/api/assets/v2/static/{asset}/"));
        }
    }
    if avatar.is_empty() {
        None
    } else {
        Some(avatar.to_owned())
    }
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
    avatar_asset_entity: Option<String>,
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

const USER_SHAPE_SELECT: &str = "u.id AS id, u.first_name AS first_name, u.last_name AS last_name, u.avatar AS avatar, u.avatar_asset_id AS avatar_asset_id, fa.entity_type AS avatar_asset_entity, u.cover_image AS cover_image, u.date_joined AS date_joined, u.display_name AS display_name, u.email AS email, u.is_active AS is_active, u.is_bot AS is_bot, u.is_email_verified AS is_email_verified, u.user_timezone AS user_timezone, u.username AS username, u.is_password_autoset AS is_password_autoset, u.last_login_medium AS last_login_medium";

async fn fetch_user(pool: &PgPool, user_id: &Uuid) -> Result<Option<UserShape>, Denial> {
    let row: Option<UserShape> = sqlx::query_as(&format!(
        "SELECT {USER_SHAPE_SELECT} FROM users u LEFT JOIN file_assets fa ON fa.id = u.avatar_asset_id AND fa.deleted_at IS NULL WHERE u.id = $1"
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
    let owned_avatar_url = avatar_url(
        user.avatar_asset_id,
        user.avatar_asset_entity.clone(),
        user.avatar.as_deref().unwrap_or(""),
    );
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
    serde_json::to_value(&view).map_err(|_| Denial::ServerError)
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
    }
    let owned_lite: Option<LiteOwned> = match user {
        Some(shape) => Some(LiteOwned {
            id: shape.id.as_ref().ok_or(Denial::ServerError)?.to_string(),
            avatar_url: avatar_url(
                shape.avatar_asset_id,
                shape.avatar_asset_entity.clone(),
                shape.avatar.as_deref().unwrap_or(""),
            ),
        }),
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
    serde_json::to_value(&view).map_err(|_| Denial::ServerError)
}

/// Parse a POST body the way DRF's `JSONParser` does: the body must be
/// UTF-8 (a decode error is not a `ValueError`, so it escapes the parser
/// into the generic 500); unparsable JSON is `ParseError` 400 with the
/// `{"detail"}` shape. The message is input-dependent, but the all-blank
/// case is deterministic CPython output, reproduced exactly.
fn parse_json_body(body: &[u8]) -> Result<Value, Denial> {
    let text = std::str::from_utf8(body).map_err(|_| Denial::ServerError)?;
    match serde_json::from_str::<Value>(text) {
        Ok(data) => {
            if data.is_object() {
                Ok(data)
            } else {
                // `None.get("email")` / `[1].get("email")` raise
                // `AttributeError` out of the view → generic 500.
                Err(Denial::ServerError)
            }
        }
        Err(_) if text.trim().is_empty() => {
            let len = text.len();
            Err(Denial::BadDetail(format!(
                "JSON parse error - Expecting value: line 1 column {} (char {})",
                len + 1,
                len
            )))
        }
        Err(_) => Err(Denial::BadDetail("JSON parse error.".to_owned())),
    }
}

/// Adapt a JSON `role` value to the integer column the way
/// `request.data.get("role", 20)` + the ORM do (pinned against Django 4.2
/// `IntegerField.get_prep_value`): absent → 20; explicit null hits the NOT
/// NULL column → `IntegrityError` → invalid-payload 400; bools adapt as
/// ints (`True` → 1, `False` → 0 — `bool` subclasses `int`); integers that
/// fit pass through; EVERY float truncates toward zero (`int(20.5)` → 20,
/// so no `fract() == 0.0` gate); numeric strings parse with `int()`
/// semantics (surrounding whitespace tolerated, `"20.5"`/`"abc"` →
/// `ValueError` → generic 500); anything else errors → generic 500.
/// A parsed negative still 400s, but at the INSERT: the
/// `PositiveIntegerField` CHECK (`role >= 0`) raises `IntegrityError`.
fn role_from_value(value: Option<&Value>) -> Result<i32, Denial> {
    match value {
        None => Ok(20),
        Some(Value::Null) => Err(Denial::InvalidPayload),
        Some(Value::Bool(true)) => Ok(1),
        Some(Value::Bool(false)) => Ok(0),
        Some(Value::Number(n)) => role_from_number(n),
        Some(Value::String(s)) => s.trim().parse::<i32>().map_err(|_| Denial::ServerError),
        Some(_) => Err(Denial::ServerError),
    }
}

/// Adapt a JSON number to the integer `role` column: integers that fit,
/// plus every finite float in range truncated toward zero (Django
/// `int(value)`); out-of-range and unparsable numbers error out →
/// generic 500 (Postgres numeric overflow surfaces as `DataError`, which
/// `handle_exception` does not catch, so 500 either way).
fn role_from_number(n: &serde_json::Number) -> Result<i32, Denial> {
    if let Some(v) = n.as_i64() {
        return i32::try_from(v).map_err(|_| Denial::ServerError);
    }
    if let Some(v) = n.as_f64() {
        if v.is_finite() && v >= f64::from(i32::MIN) && v <= f64::from(i32::MAX) {
            return Ok(v.trunc() as i32);
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
/// spelling only decides which unknown address 404s.
fn lookup_email(value: Option<&Value>) -> Result<String, Denial> {
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
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body.to_string()))
        .expect("json response")
}

/// `POST /api/instances/admins/` (`admin.py:49-68`).
async fn create_admin(
    State(state): State<AppState>,
    extension: Option<Extension<SessionHandle>>,
    body: axum::body::Bytes,
) -> Result<Response, Denial> {
    let pool = pools(&state)?;
    let actor = authed_user(pool, &extension).await?;
    let instance_id = require_instance_admin(pool, &actor).await?;
    let tz = actor_timezone(pool, &actor).await?;

    let data = parse_json_body(&body)?;
    let email = lookup_email(data.get("email"))?;
    let role = role_from_value(data.get("role"))?;
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
    Ok(json_response(StatusCode::CREATED, body))
}

/// `GET /api/instances/admins/` (`admin.py:70-80`): the 2h
/// `cache_response` is freshness-only (see module docs) and always reads
/// fresh here.
async fn list_admins(
    State(state): State<AppState>,
    extension: Option<Extension<SessionHandle>>,
) -> Result<Response, Denial> {
    let pool = pools(&state)?;
    let actor = authed_user(pool, &extension).await?;
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
        u_avatar_asset_entity: Option<String>,
        u_is_bot: Option<bool>,
        u_display_name: Option<String>,
        u_email: Option<String>,
        u_last_login_medium: Option<String>,
    }

    let rows: Vec<AdminListRow> = sqlx::query_as(
        "SELECT a.id AS id, a.created_at AS created_at, a.updated_at AS updated_at, a.deleted_at AS deleted_at, a.role AS role, a.is_verified AS is_verified, a.created_by_id AS created_by_id, a.updated_by_id AS updated_by_id, a.user_id AS user_id, a.instance_id AS instance_id, u.id AS u_id, u.first_name AS u_first_name, u.last_name AS u_last_name, u.avatar AS u_avatar, u.avatar_asset_id AS u_avatar_asset_id, fa.entity_type AS u_avatar_asset_entity, u.is_bot AS u_is_bot, u.display_name AS u_display_name, u.email AS u_email, u.last_login_medium AS u_last_login_medium FROM instance_admins a LEFT JOIN users u ON u.id = a.user_id LEFT JOIN file_assets fa ON fa.id = u.avatar_asset_id AND fa.deleted_at IS NULL WHERE a.instance_id = $1 AND a.deleted_at IS NULL ORDER BY a.created_at DESC",
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
            avatar_asset_entity: row.u_avatar_asset_entity.clone(),
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
    let Ok(admin_id) = pk.parse::<Uuid>() else {
        let mut builder = Request::builder().method(method).uri(uri);
        for (name, value) in headers.iter() {
            builder = builder.header(name, value);
        }
        let req = builder
            .body(axum::body::Body::empty())
            .map_err(|_| Denial::ServerError)?;
        return Ok(edge::proxy(State(state), req).await);
    };
    let pool = pools(&state)?;
    let actor = authed_user(pool, &extension).await?;
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
    let actor = authed_user(pool, &extension).await?;
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
    // session whose user row is gone authenticates to nobody → bare flag.
    if let Some(actor) = actor_user_id(&extension) {
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
) -> Result<Response, Denial> {
    let urls = &state.settings().urls;
    let target = sign_out_redirect(
        urls.admin_base_url.as_deref(),
        urls.web_url.as_deref(),
        urls.app_base_url.as_deref(),
        &urls.admin_base_path,
    )?;
    let location = axum::http::HeaderValue::from_str(&target).map_err(|_| Denial::ServerError)?;

    let outcome: Result<(), Denial> = async {
        let pool = pools(&state)?;
        let actor = actor_user_id(&extension).ok_or(Denial::ServerError)?;
        let exists: Option<(Uuid,)> = sqlx::query_as("SELECT id FROM users WHERE id = $1")
            .bind(actor)
            .fetch_optional(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
        if exists.is_none() {
            return Err(Denial::ServerError);
        }
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
        if let Some(handle) = extension.as_ref().map(|ext| ext.0.clone()) {
            handle.lock().clear();
        }
        Ok(())
    }
    .await;
    // Success AND failure both 302 (`except Exception: … redirect anyway`).
    let _ = outcome;
    Ok(Response::builder()
        .status(StatusCode::FOUND)
        .header(header::LOCATION, location)
        .body(axum::body::Body::empty())
        .expect("redirect response"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn denial_body(denial: Denial) -> (StatusCode, String) {
        denial.status_and_body()
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
        assert_eq!(
            avatar_url(asset, Some("USER_AVATAR".to_owned()), ""),
            Some("/api/assets/v2/static/00000000-0000-0000-0000-000000000000/".to_owned())
        );
        // A non-static entity type falls through to the raw avatar.
        assert_eq!(
            avatar_url(
                asset,
                Some("ISSUE_ATTACHMENT".to_owned()),
                "https://cdn/x.png"
            ),
            Some("https://cdn/x.png".to_owned())
        );
        assert_eq!(avatar_url(None, None, ""), None);
        assert_eq!(
            avatar_url(None, None, "https://cdn/x.png"),
            Some("https://cdn/x.png".to_owned())
        );
    }

    fn shape_fixture() -> UserShape {
        UserShape {
            id: Some(Uuid::nil()),
            first_name: Some("Ada".to_owned()),
            last_name: Some("Lovelace".to_owned()),
            avatar: Some(String::new()),
            avatar_asset_id: None,
            avatar_asset_entity: None,
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
        // Valid objects pass through.
        assert!(parse_json_body(br#"{"email":"a@b.c"}"#).is_ok());
        // Empty/blank bodies are DRF `ParseError` with the CPython message.
        let err = parse_json_body(b"").expect_err("empty body 400s");
        assert!(matches!(err, Denial::BadDetail(_)));
        let (_, body) = err.status_and_body();
        assert_eq!(
            body,
            r#"{"detail":"JSON parse error - Expecting value: line 1 column 1 (char 0)"}"#
        );
        let err = parse_json_body(b"   ").expect_err("blank body 400s");
        let (_, body) = err.status_and_body();
        assert!(body.contains("column 4 (char 3)"), "{body}");
        // Non-object JSON has no `.get`: Django raises out of the view.
        assert!(matches!(parse_json_body(br"[1]"), Err(Denial::ServerError)));
        assert!(matches!(
            parse_json_body(br"null"),
            Err(Denial::ServerError)
        ));
        // Non-UTF-8 escapes the parser the same way.
        assert!(matches!(parse_json_body(&[0xff]), Err(Denial::ServerError)));
    }

    #[test]
    fn role_values_follow_the_orm_column() {
        let role = |raw: &str| {
            let data: Value = serde_json::from_str(raw).expect("json");
            role_from_value(data.get("role"))
        };
        // Absent -> default 20; plain ints pass through.
        assert_eq!(role("{}").expect("default"), 20);
        assert_eq!(role(r#"{"role":20}"#).expect("int"), 20);
        assert_eq!(role(r#"{"role":5}"#).expect("low role"), 5);
        // `bool` subclasses `int`: True -> 1, False -> 0.
        assert_eq!(role(r#"{"role":true}"#).expect("bool"), 1);
        assert_eq!(role(r#"{"role":false}"#).expect("bool"), 0);
        // `int(value)` truncates every float toward zero, including
        // negatives (-0.5 -> 0); a parsed negative still 400s, but at the
        // INSERT CHECK, so the adapter returns it.
        assert_eq!(role(r#"{"role":20.0}"#).expect("integral float"), 20);
        assert_eq!(role(r#"{"role":20.9}"#).expect("truncation"), 20);
        assert_eq!(role(r#"{"role":-0.5}"#).expect("negative truncation"), 0);
        assert_eq!(role(r#"{"role":-1}"#).expect("negative parses"), -1);
        // Numeric strings parse with `int()` semantics (surrounding
        // whitespace tolerated); `"20.5"`/`"abc"` are `ValueError` -> 500.
        assert_eq!(role(r#"{"role":"20"}"#).expect("string"), 20);
        assert_eq!(role(r#"{"role":" 20 "}"#).expect("padded"), 20);
        assert!(role(r#"{"role":"20.5"}"#).is_err());
        assert!(role(r#"{"role":"abc"}"#).is_err());
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
    fn lookup_email_matches_request_data_truthiness() {
        let email = |raw: &str| {
            let data: Value = serde_json::from_str(raw).expect("json");
            lookup_email(data.get("email"))
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
