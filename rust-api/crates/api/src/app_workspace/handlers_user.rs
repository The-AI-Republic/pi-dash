//! User identity endpoints (D-24, stage 5, PIDASHCONV-619).
//!
//! Ports the `UserEndpoint` identity family from
//! `apps/api/pi_dash/app/views/user/base.py:59-390`, routes
//! `apps/api/pi_dash/app/urls/user.py:24-64` (`users/me/`,
//! `users/session/`, `users/me/settings/`, `users/me/email/generate-code/`,
//! `users/me/email/`, `users/me/instance-admin/`, `users/me/onboard/`,
//! `users/me/tour-completed/`).
//!
//! Wiring only, no new logic: auth through
//! [`crate::license::resolve_actor`] (Django-session `request.user`),
//! shapes from `pidash_services::app_workspace::ser_user` (PIDASHCONV-603),
//! SQL from `...::queries_user` (PIDASHCONV-612), throttle specs + header
//! bytes from [`super::gates`] (PIDASHCONV-613), publishers from
//! `...::tasks` (PIDASHCONV-614), `User.save()` semantics from
//! `...::models_user::user` (PIDASHCONV-607). Fixture:
//! `rust-api/fixtures/app_workspace/handlers/routes.golden.json` (F-W24-15,
//! routes U01-U05 + U09-U11 and every error body below; trace:
//! `rust-api/fixtures/app_workspace/TRACE.md`).
//!
//! Request order per route (Django's order, preserved): session auth
//! (`BaseViewSet`/`BaseAPIView` + `IsAuthenticated`, 401
//! [`gates::ANON_BODY`] for anonymous callers — except the `AllowAny`
//! session endpoint), then the throttle on generate-code only (DRF
//! `initial()` order: auth, permissions, throttles — the throttle runs
//! before the body even validates), then the handler body.
//!
//! Ported quirks (translate, don't redesign; also listed in the PR):
//!
//! * The deactivate sole-admin scans keep their vacuous `GROUP BY` rows
//!   (`queries_user` bug 4): `other_admin_exists`/`total_members` are 1
//!   for every membership, so both "only admin" 400s are unreachable —
//!   every active membership is collected. The loop still evaluates the
//!   condition per row.
//! * `bulk_update` stamps no `updated_at` and filters no `deleted_at`
//!   (bug 5); an empty collection issues no statement.
//! * Onboard/tour bind the raw `request.data.get(flag, False)` value with
//!   no serializer validation (bug 6), but the column's
//!   `BooleanField.to_python` still coerces (`1`/`0`, `"t"`/`"f"`,
//!   `"True"`/`"False"`, `"1"`/`"0"`) or rejects anything else with
//!   the whole-body 400 — including `null` (→ 400 payload-invalid via
//!   the `IntegrityError` branch) and composites (→ 400, not 500).
//! * `update_email` compares `str(stored_token) != str(code)`, so a
//!   missing stored token renders `"None"` and can only match the
//!   literal code `"None"`. A composite stored token (array/object —
//!   only via a foreign cache write, since generate-code always stores
//!   `{"token": "<6 digits>"}`) answers the failed-verify 400 where
//!   Python's `str()` would compare and answer the mismatch 400.
//! * `partial_update` silently drops read-only keys (email included) and
//!   unknown keys, and a non-object body is the DRF non-mapping 400
//!   (`non_field_errors`); for `avatar_asset`/`cover_image_asset` a
//!   bool answers a field error, `0` queries the nil UUID (field
//!   error), and unparseable strings, negative ints, floats, and
//!   composites answer the whole-body 400 `{"error": "Please provide
//!   valid detail"}` (Django `ValidationError` branch —
//!   `UUIDField.get_prep_value` wraps the `ValueError`/`AttributeError`)
//!   — straight from `PrimaryKeyRelatedField.to_internal_value`.
//! * `generate_email_verification_code` consumes throttle quota even when
//!   the email fails validation (throttles run in `initial()`).
//! * `retrieve_instance_admin` binds `instance_id IS NULL` when no
//!   `Instance` row exists.
//! * `deactivate` stamps `last_logout_ip` through `get_client_ip`
//!   (`X-Forwarded-For` first entry unstripped, else the peer address,
//!   always present — the required-`ConnectInfo` precedent); a `None`
//!   would answer 400 payload-invalid through the `IntegrityError`
//!   branch.
//!
//! Documented approximations (no fixture input covers them):
//!
//! * Non-string JSON scalars coerce via Rust number formatting, which
//!   differs from Python `repr` for scientific-notation floats (same
//!   choice as the `app_pages`/`app_scheduler` precedents).
//! * `DateTimeField` inputs accept RFC 3339, the naive `T`/space forms
//!   (unpadded parts included, like Django's `\d{1,2}` arm), padded
//!   date-only, comma/dot fractions truncated to six digits, and a
//!   bare trailing dot. Still rejected where 3.12 `fromisoformat`
//!   parses: lowercase-`t` separators, week dates, basic-format
//!   `T103045`, and surrounding whitespace.
//! * The email cache round-trip uses the raw
//!   `magic_email_update_{user}_{email}` key with a plain-JSON value
//!   (the `queries_user` contract): no Django `:1:` key prefix, no
//!   pickle framing — cutover owns these keys on both sides.
//! * Task `.delay()` failures keep the response standing (the codebase
//!   `enqueue_best_effort` precedent) where Django answers 400/500;
//!   the queue shares the request pool, so the divergence needs a
//!   half-dead database.
//! * `user_timezone` `ChoiceField` input rendering for JSON arrays/objects
//!   uses compact JSON where Python `str()` renders single quotes.
//! * `ISSUE_ATTACHMENT`/description asset URLs render missing
//!   project/issue ids as `""` (the reviewed `render_asset_url`
//!   precedent) where Django renders `"None"`.
//! * Non-JSON request bodies answer the JSON-parse 400 (the
//!   `app_scheduler` precedent); DRF would negotiate form/multipart.

use axum::extract::{ConnectInfo, Request, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json, Router};
use chrono::{DateTime, Utc};
use serde_json::{Map, Value};
use sqlx::PgPool;
use std::net::SocketAddr;

use pidash_services::app_workspace::{models_user, queries_user, ser_extras, ser_user, tasks};
use pidash_services::auth_session::{guards as throttle_kernel, shapes as auth_shapes};

use super::gates;
use crate::middleware::SessionHandle;
use crate::state::AppState;

// ---------------------------------------------------------------------------
// Exact bodies (F-W24-15 `errors`, byte-pinned in `tests` below)
// ---------------------------------------------------------------------------

/// `ObjectDoesNotExist` branch (`app/views/base.py:132-136,232-236`):
/// bare `.get()` misses (profile reads, session re-read, dangling FKs).
pub const OBJECT_NOT_FOUND_BODY: &str = r#"{"error":"The required object does not exist."}"#;
/// Generic branch (`app/views/base.py:145-149,244-248`).
pub const SERVER_ERROR_BODY: &str = r#"{"error":"Something went wrong please try again later"}"#;
/// `IntegrityError` branch (`app/views/base.py:120-124,220-224`).
pub const INVALID_PAYLOAD_BODY: &str = r#"{"error":"The payload is not valid"}"#;
/// Django `ValidationError` branch (`app/views/base.py:126-130,226-230`):
/// invalid-UUID asset pks on `partial_update`.
pub const INVALID_DETAIL_BODY: &str = r#"{"error":"Please provide valid detail"}"#;
/// Generate-code success (`user/base.py:165-168`).
pub const CODE_SENT_BODY: &str = r#"{"message":"Verification code sent to email"}"#;
/// Onboard/tour success (`user/base.py:381,389`).
pub const UPDATED_BODY: &str = r#"{"message":"Updated successfully"}"#;
/// `_validate_new_email` branches (`user/base.py:106-133`).
pub const EMAIL_REQUIRED_BODY: &str = r#"{"error":"Email is required"}"#;
pub const EMAIL_FORMAT_BODY: &str = r#"{"error":"Invalid email format"}"#;
pub const EMAIL_SAME_BODY: &str = r#"{"error":"New email must be different from current email"}"#;
pub const EMAIL_TAKEN_BODY: &str = r#"{"error":"An account with this email already exists"}"#;
/// `update_email` code branches (`user/base.py:191-222`).
pub const CODE_REQUIRED_BODY: &str = r#"{"error":"Verification code is required"}"#;
pub const CODE_EXPIRED_BODY: &str = r#"{"error":"Verification code has expired or is invalid"}"#;
pub const CODE_INVALID_BODY: &str = r#"{"error":"Invalid verification code"}"#;
pub const CODE_FAILED_BODY: &str = r#"{"error":"Failed to verify code. Please try again."}"#;
/// Generate-code `except` branch (`user/base.py:169-174`).
pub const GENERATE_FAILED_BODY: &str =
    r#"{"error":"Failed to generate verification code. Please try again."}"#;
/// Deactivate guards (`user/base.py:257-261,282-285,302-306`).
pub const DEACTIVATE_INSTANCE_ADMIN_BODY: &str =
    r#"{"error":"You cannot deactivate your account since you are an instance admin"}"#;
pub const DEACTIVATE_SOLE_PROJECT_ADMIN_BODY: &str =
    r#"{"error":"You cannot deactivate account as you are the only admin in some projects."}"#;
pub const DEACTIVATE_SOLE_WORKSPACE_ADMIN_BODY: &str =
    r#"{"error":"You cannot deactivate account as you are the only admin in some workspaces."}"#;

/// DRF `ParseError` prefix (`rest_framework/parsers.py`); the
/// `serde_json` suffix is backend-specific and not ported (the
/// `app_scheduler` precedent). Lowercase `detail`: DRF 3.15.2
/// `exception_handler` renders `{'detail': ...}` (ord-verified in the
/// installed source; PIDASHCONV-724).
const JSON_PARSE_ERROR: &str = "JSON parse error";

// ---------------------------------------------------------------------------
// Handler-level denial
// ---------------------------------------------------------------------------

/// What a user-identity handler answers without running the happy path.
#[derive(Debug)]
pub enum Denial {
    /// Anonymous on an authenticated route: 401 [`gates::ANON_BODY`].
    Unauthorized,
    /// Bare `.get()` miss: 404 [`OBJECT_NOT_FOUND_BODY`].
    ObjectNotFound,
    /// 400, `{"detail": ...}` (body parse errors).
    BadDetail(String),
    /// 400, [`INVALID_DETAIL_BODY`] (Django `ValidationError`
    /// branch: invalid-UUID asset pks, uncoercible onboard/tour
    /// flags).
    InvalidDetail,
    /// 400, `{"error": ...}` (view-inline / `IntegrityError` /
    /// Django-`ValidationError` branches).
    BadError(String),
    /// 400, pre-rendered serializer-errors body (`{"field": [...]}`).
    BadJson(Value),
    /// Throttle denial: 429 + the 5900 dict.
    Throttled,
    /// Database/cache failure, or the writer's 500 half: 500
    /// [`SERVER_ERROR_BODY`].
    ServerError,
}

/// `{"detail": message}` envelope (DRF `exception_handler` shape for
/// scalar details; lowercase per installed DRF 3.15.2).
fn detail_envelope(message: String) -> Value {
    let mut body = Map::new();
    body.insert("detail".to_owned(), Value::String(message));
    Value::Object(body)
}

/// `{"error": message}` envelope (`handle_exception` branch shape).
fn error_envelope(message: String) -> Value {
    let mut body = Map::new();
    body.insert("error".to_owned(), Value::String(message));
    Value::Object(body)
}

fn json_response(status: StatusCode, body: &str) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body.to_owned()))
        .expect("user-handler response")
}

impl IntoResponse for Denial {
    fn into_response(self) -> Response {
        match self {
            Denial::Unauthorized => json_response(StatusCode::UNAUTHORIZED, gates::ANON_BODY),
            Denial::ObjectNotFound => json_response(StatusCode::NOT_FOUND, OBJECT_NOT_FOUND_BODY),
            Denial::BadDetail(message) => {
                (StatusCode::BAD_REQUEST, Json(detail_envelope(message))).into_response()
            }
            Denial::InvalidDetail => json_response(StatusCode::BAD_REQUEST, INVALID_DETAIL_BODY),
            Denial::BadError(message) => {
                (StatusCode::BAD_REQUEST, Json(error_envelope(message))).into_response()
            }
            Denial::BadJson(value) => (StatusCode::BAD_REQUEST, Json(value)).into_response(),
            Denial::Throttled => (
                StatusCode::TOO_MANY_REQUESTS,
                [(header::CONTENT_TYPE, "application/json")],
                gates::throttle_denied_json(),
            )
                .into_response(),
            Denial::ServerError => {
                json_response(StatusCode::INTERNAL_SERVER_ERROR, SERVER_ERROR_BODY)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Shared plumbing (the `app_scheduler` handler precedent)
// ---------------------------------------------------------------------------

fn pool_of(state: &AppState) -> Result<&PgPool, Denial> {
    state
        .pools()
        .map(|pools| pools.primary())
        .ok_or(Denial::ServerError)
}

/// `request.user` or the 401: [`crate::license::resolve_actor`] mirrors
/// `get_user` + DRF `SessionAuthentication` (UUID `_auth_user_id` for the
/// model backend, matching `_auth_user_hash`, existing active row).
async fn authed_actor(
    state: &AppState,
    pool: &PgPool,
    extension: Option<Extension<SessionHandle>>,
) -> Result<crate::license::Actor, Denial> {
    match crate::license::resolve_actor(pool, state.settings().secret_key.as_bytes(), extension)
        .await
    {
        Ok(Some(actor)) => Ok(actor),
        Ok(None) => Err(Denial::Unauthorized),
        Err(_) => Err(Denial::ServerError),
    }
}

/// `request.data`: empty bodies validate as `{}`; anything else must
/// parse as JSON (`app_scheduler` precedent).
async fn read_json_body(req: Request) -> Result<Value, Denial> {
    let (_parts, body) = req.into_parts();
    let bytes = match http_body_util::BodyExt::collect(body).await {
        Ok(collected) => collected.to_bytes(),
        Err(_) => return Err(Denial::ServerError),
    };
    if bytes.is_empty() {
        return Ok(Value::Object(Map::new()));
    }
    serde_json::from_slice(&bytes)
        .map_err(|error| Denial::BadDetail(format!("{JSON_PARSE_ERROR} - {error}")))
}

/// Whether a sqlx failure is an integrity violation (SQLSTATE class
/// `23`), which Django's `handle_exception` answers 400 for.
fn is_integrity_error(error: &sqlx::Error) -> bool {
    error
        .as_database_error()
        .and_then(|db| db.code())
        .is_some_and(|code| code.starts_with("23"))
}

/// Map a write failure through `handle_exception`: integrity → 400
/// payload-invalid, everything else → 500.
fn write_denial(error: sqlx::Error) -> Denial {
    if is_integrity_error(&error) {
        Denial::BadError("The payload is not valid".to_owned())
    } else {
        Denial::ServerError
    }
}

/// Convert a [`queries_user`] `:named` statement to sqlx `$n`
/// placeholders. Numbering follows `params` order; a `:name` only
/// matches when followed by a non-identifier byte. (Local copy —
// sibling handler issues never fork a shared helper.)
pub fn positional(sql: &str, params: &[&str]) -> String {
    let bytes = sql.as_bytes();
    let mut out = String::with_capacity(sql.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b':' {
            let mut j = i + 1;
            while j < bytes.len() && is_ident_byte(bytes[j]) {
                j += 1;
            }
            if j > i + 1 {
                let name = &sql[i + 1..j];
                if let Some(position) = params.iter().position(|candidate| *candidate == name) {
                    out.push('$');
                    out.push_str(&(position + 1).to_string());
                    i = j;
                    continue;
                }
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

fn is_ident_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// Python `str.strip()` with no arguments: Unicode whitespace plus
/// `\x1c`-`\x1f`, which Rust's `char::is_whitespace` does not cover
/// (the `v1_projects` `py_strip` precedent, verified against CPython).
fn py_strip(value: &str) -> &str {
    value.trim_matches(|c: char| c.is_whitespace() || ('\x1c'..='\x1f').contains(&c))
}

/// `request.data.get("email", "").strip().lower()`
/// (`user/base.py:144,183`).
fn normalize_email(value: &str) -> String {
    py_strip(value).to_lowercase()
}

// ---------------------------------------------------------------------------
// Rows (`sqlx::FromRow` maps by column name; extra columns ignored)
// ---------------------------------------------------------------------------

/// One `users` row: every `UserSerializer` wire column plus the two
/// asset FK ids and the `User.save()` inputs (`db/models/user.py:56-126`
/// + `AbstractBaseUser.last_login`).
#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub struct UserRow {
    pub id: uuid::Uuid,
    pub last_login: Option<DateTime<Utc>>,
    pub username: String,
    pub mobile_number: Option<String>,
    pub email: Option<String>,
    pub display_name: String,
    pub first_name: String,
    pub last_name: String,
    pub avatar: String,
    pub avatar_asset_id: Option<uuid::Uuid>,
    pub cover_image: Option<String>,
    pub cover_image_asset_id: Option<uuid::Uuid>,
    pub date_joined: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_location: String,
    pub created_location: String,
    pub is_superuser: bool,
    pub is_managed: bool,
    pub is_password_expired: bool,
    pub is_active: bool,
    pub is_staff: bool,
    pub is_email_verified: bool,
    pub is_password_autoset: bool,
    pub is_password_reset_required: bool,
    pub token: String,
    pub last_active: Option<DateTime<Utc>>,
    pub last_login_time: Option<DateTime<Utc>>,
    pub last_logout_time: Option<DateTime<Utc>>,
    pub last_login_ip: String,
    pub last_logout_ip: String,
    pub last_login_medium: String,
    pub last_login_uagent: String,
    pub token_updated_at: Option<DateTime<Utc>>,
    pub is_bot: bool,
    pub bot_type: Option<String>,
    pub user_timezone: String,
    pub is_email_valid: bool,
    pub masked_at: Option<DateTime<Utc>>,
}

/// Columns for the user reads, `UserSerializer` wire order
/// (`ser_user::USER_WIRE_FIELDS`) with the asset FK ids folded in.
/// `password` is never selected (never rendered, never compared here).
const USER_ROW_COLUMNS: &str = "id, last_login, username, mobile_number, email, display_name, \
    first_name, last_name, avatar, avatar_asset_id, cover_image, cover_image_asset_id, \
    date_joined, created_at, updated_at, last_location, created_location, is_superuser, \
    is_managed, is_password_expired, is_active, is_staff, is_email_verified, \
    is_password_autoset, is_password_reset_required, token, last_active, last_login_time, \
    last_logout_time, last_login_ip, last_logout_ip, last_login_medium, last_login_uagent, \
    token_updated_at, is_bot, bot_type, user_timezone, is_email_valid, masked_at";

/// `User.objects.get(pk=...)`: `users` carries no `deleted_at`, so the
/// lookup is unscoped. A miss is [`Denial::ObjectNotFound`].
async fn fetch_user(pool: &PgPool, user_id: uuid::Uuid) -> Result<UserRow, Denial> {
    let sql = format!("SELECT {USER_ROW_COLUMNS} FROM users WHERE users.id = $1");
    sqlx::query_as(&sql)
        .bind(user_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?
        .ok_or(Denial::ObjectNotFound)
}

/// One `profiles` row for the identity paths: the pk plus
/// `last_workspace_id` (plain nullable UUID, `user.py:236`).
#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub struct ProfileRow {
    pub id: uuid::Uuid,
    pub last_workspace_id: Option<uuid::Uuid>,
}

/// `Profile.objects.get(user=...)` (`queries_user::profile_by_user_sql`
/// shape; `profiles` carries no `deleted_at`). A miss is
/// [`Denial::ObjectNotFound`].
async fn fetch_profile(pool: &PgPool, user_id: uuid::Uuid) -> Result<ProfileRow, Denial> {
    sqlx::query_as("SELECT id, last_workspace_id FROM profiles WHERE profiles.user_id = $1")
        .bind(user_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?
        .ok_or(Denial::ObjectNotFound)
}

/// One `workspaces` row for the settings branches: the re-fetch /
/// fallback columns decoded out of the fixture `SELECT workspaces.*`
/// statements (extra columns ignored).
#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub struct SettingsWorkspaceRow {
    pub id: uuid::Uuid,
    pub slug: String,
    pub name: String,
    pub logo_asset_id: Option<uuid::Uuid>,
}

/// One `instances` row for `Instance.first`: the pk decoded out of
/// the fixture `SELECT instances.*` statement.
#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub struct InstanceRow {
    pub id: uuid::Uuid,
}

/// One `file_assets` row for `asset_url` rendering: the plain-manager
/// columns (forward-FK access runs through `_base_manager`, unscoped —
/// the `queries_user` bug-13 precedent).
#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub struct AssetRow {
    pub id: uuid::Uuid,
    pub entity_type: Option<String>,
    pub workspace_id: Option<uuid::Uuid>,
    pub project_id: Option<uuid::Uuid>,
    pub issue_id: Option<uuid::Uuid>,
}

/// Unscoped asset fetch for one set FK. A dangling FK raises through
/// the `ObjectDoesNotExist` branch (404), like Django's
/// `RelatedObjectDoesNotExist`.
async fn fetch_asset(pool: &PgPool, asset_id: uuid::Uuid) -> Result<AssetRow, Denial> {
    sqlx::query_as(
        "SELECT id, entity_type, workspace_id, project_id, issue_id FROM file_assets WHERE file_assets.id = $1",
    )
    .bind(asset_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?
    .ok_or(Denial::ObjectNotFound)
}

/// Render `FileAsset.asset_url` (`db/models/asset.py:79-109`) through
/// the reviewed `v1_projects` port. Non-static branches need the
/// workspace slug (forward FK, unscoped): a `None` workspace is the
/// `AttributeError` 500, a dangling one the 404.
async fn render_asset_url(pool: &PgPool, asset: &AssetRow) -> Result<Option<String>, Denial> {
    let needs_slug = matches!(
        asset.entity_type.as_deref(),
        Some(
            "ISSUE_ATTACHMENT"
                | "ISSUE_DESCRIPTION"
                | "COMMENT_DESCRIPTION"
                | "PAGE_DESCRIPTION"
                | "DRAFT_ISSUE_DESCRIPTION"
        )
    );
    let workspace_slug: Option<String> = if needs_slug {
        let Some(workspace_id) = asset.workspace_id else {
            return Err(Denial::ServerError);
        };
        let row: Option<(String,)> =
            sqlx::query_as("SELECT slug FROM workspaces WHERE workspaces.id = $1")
                .bind(workspace_id)
                .fetch_optional(pool)
                .await
                .map_err(|_| Denial::ServerError)?;
        Some(row.ok_or(Denial::ObjectNotFound)?.0)
    } else {
        None
    };
    let asset_ref = crate::v1_projects::handlers_members::AssetRef {
        id: asset.id,
        entity_type: asset.entity_type.clone(),
        workspace_slug,
        project_id: asset.project_id,
        issue_id: asset.issue_id,
    };
    Ok(crate::v1_projects::handlers_members::render_asset_url(
        &asset_ref,
    ))
}

/// Resolve `User.avatar_url` (`db/models/user.py:142-151`): a set FK
/// returns the asset URL as-is (even `None`); an unset FK falls back
/// to the direct column when non-empty
/// (`models_user::user::avatar_url`).
async fn resolve_avatar_url(
    pool: &PgPool,
    asset_id: Option<uuid::Uuid>,
    avatar: &str,
) -> Result<Option<String>, Denial> {
    if asset_id.is_none() {
        return Ok(models_user::user::avatar_url(false, None, avatar).map(str::to_owned));
    }
    let asset = fetch_asset(pool, asset_id.expect("checked")).await?;
    let url = render_asset_url(pool, &asset).await?;
    Ok(models_user::user::avatar_url(true, url.as_deref(), avatar).map(str::to_owned))
}

/// Resolve `User.cover_image_url` (`db/models/user.py:153-162`), same
/// shape over `cover_image_asset` / `cover_image`.
async fn resolve_cover_image_url(
    pool: &PgPool,
    asset_id: Option<uuid::Uuid>,
    cover_image: Option<&str>,
) -> Result<Option<String>, Denial> {
    if asset_id.is_none() {
        return Ok(models_user::user::cover_image_url(false, None, cover_image).map(str::to_owned));
    }
    let asset = fetch_asset(pool, asset_id.expect("checked")).await?;
    let url = render_asset_url(pool, &asset).await?;
    Ok(models_user::user::cover_image_url(true, url.as_deref(), cover_image).map(str::to_owned))
}

/// One deactivate-scan row: the pk plus the two annotations. The
/// executed statement is the full `GROUP BY` scan
/// (`queries_user::project_deactivate_scan_sql` /
/// `workspace_deactivate_scan_sql`); the remaining columns decode past
/// this struct untouched.
#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub struct DeactivateScanRow {
    pub id: uuid::Uuid,
    pub other_admin_exists: i64,
    pub total_members: i64,
}

// ---------------------------------------------------------------------------
// Datetimes (DRF `DateTimeField`, `USE_TZ=True`, user zone active)
// ---------------------------------------------------------------------------

/// DRF `DateTimeField` invalid-input message, verified live against the
/// installed DRF 3.15.2 (`fields.py`, `DATETIME_INPUT_FORMATS` default
/// `['iso-8601']`).
pub const DATETIME_INVALID_MESSAGE: &str = "Datetime has wrong format. Use one of these formats instead: YYYY-MM-DDThh:mm[:ss[.uuuuuu]][+HH:MM|-HH:MM|Z].";

fn datetime_make_aware_message(timezone: &str) -> String {
    format!("Invalid datetime for the timezone \"{timezone}\".")
}

/// Strip a trailing `Z`/`±HH:MM`/`±HHMM`/`±HH` offset, returning the
/// naive part and the offset in seconds east of UTC.
fn split_datetime_offset(raw: &str) -> Option<(&str, i32)> {
    if let Some(naive) = raw.strip_suffix('Z') {
        return Some((naive, 0));
    }
    if raw.len() < 3 {
        return None;
    }
    // The offset sign is the last `+`/`-` past the date (position 10).
    let mut sign_at = None;
    for (i, b) in raw.as_bytes().iter().enumerate().skip(10) {
        if *b == b'+' || *b == b'-' {
            sign_at = Some(i);
        }
    }
    let at = sign_at?;
    let (naive, offset) = raw.split_at(at);
    let digits: String = offset.chars().filter(|c| c.is_ascii_digit()).collect();
    let seconds = match digits.len() {
        2 => digits.parse::<i32>().ok()? * 3600,
        4 => {
            let hours: i32 = digits[..2].parse().ok()?;
            let minutes: i32 = digits[2..].parse().ok()?;
            if minutes > 59 {
                return None;
            }
            hours * 3600 + minutes * 60
        }
        _ => return None,
    };
    if offset.starts_with('-') {
        Some((naive, -seconds))
    } else {
        Some((naive, seconds))
    }
}

const NAIVE_DATETIME_FORMATS: &[&str] = &[
    "%Y-%m-%dT%H:%M:%S%.f",
    "%Y-%m-%dT%H:%M:%S",
    "%Y-%m-%dT%H:%M",
    "%Y-%m-%d %H:%M:%S%.f",
    "%Y-%m-%d %H:%M:%S",
    "%Y-%m-%d %H:%M",
];

/// Whether the naive part has Django's shape: a 4-digit year on
/// every path (`\d{4}`), and a fully padded date when no time part
/// follows (date-only takes the strict `fromisoformat` path, which
/// rejects `2024-1-5`; with a time part the `\d{1,2}` regex arm
/// accepts unpadded parts, like chrono).
fn naive_shape_ok(raw: &str) -> bool {
    let bytes = raw.as_bytes();
    if bytes.len() < 4 || !bytes[..4].iter().all(|b| b.is_ascii_digit()) {
        return false;
    }
    if raw.contains(['T', ' ']) {
        return true;
    }
    bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[5..7].iter().all(|b| b.is_ascii_digit())
        && bytes[8..10].iter().all(|b| b.is_ascii_digit())
}

/// Parse one naive datetime: the `T`/space forms plus date-only
/// (`fromisoformat` accepts padded date-only — probed live on the
/// 3.12 runtime). Fractions of any length truncate to six digits
/// (Django's `\d{1,6}\d{0,6}`; 3.12 `fromisoformat` truncates longer
/// runs the same way); comma fractions normalize to dots; a bare
/// trailing dot carries no fraction.
fn parse_naive_datetime(raw: &str) -> Option<chrono::NaiveDateTime> {
    if !naive_shape_ok(raw) {
        return None;
    }
    let mut owned = raw.to_owned();
    if let Some(dot) = owned.rfind(['.', ',']) {
        if owned[..dot].matches(':').count() >= 2 {
            let tail = owned[dot + 1..].to_owned();
            let digits: String = tail.chars().take_while(|c| c.is_ascii_digit()).collect();
            let rest: String = tail.chars().skip_while(|c| c.is_ascii_digit()).collect();
            let head = owned[..dot + 1].to_owned();
            if digits.is_empty() && rest.is_empty() {
                if raw.as_bytes()[dot] == b'.' {
                    owned = head[..head.len() - 1].to_owned();
                }
            } else {
                let kept = if digits.len() > 6 {
                    &digits[..6]
                } else {
                    digits.as_str()
                };
                if raw.as_bytes()[dot] == b',' && digits.len() == tail.len() {
                    owned = format!("{}.{}", &head[..head.len() - 1], kept);
                } else if digits.len() > 6 {
                    owned = format!("{head}{kept}");
                    owned.push_str(&rest);
                }
            }
        }
    }
    for format in NAIVE_DATETIME_FORMATS {
        if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(&owned, format) {
            return Some(naive);
        }
    }
    if let Ok(date) = chrono::NaiveDate::parse_from_str(&owned, "%Y-%m-%d") {
        return date.and_hms_opt(0, 0, 0);
    }
    None
}

/// DRF `DateTimeField.to_internal_value` (`fields.py`, `iso-8601` input
/// format): aware inputs convert to UTC; naive inputs are made aware in
/// the request user's zone (`enforce_timezone`). Gap times answer the
/// `make_aware` message; ambiguous times take the earlier fold (zoneinfo
/// `fold=0`, which DRF's `valid_datetime` accepts).
fn parse_drf_datetime(
    value: &Value,
    timezone: &chrono_tz::Tz,
    timezone_name: &str,
) -> Result<DateTime<Utc>, String> {
    let invalid = || DATETIME_INVALID_MESSAGE.to_owned();
    let Value::String(raw) = value else {
        return Err(invalid());
    };
    // Django's tz arm is `Z` or a numeric offset only; a lowercase
    // `z` matches neither `fromisoformat` nor the regex.
    if raw.ends_with('z') {
        return Err(invalid());
    }
    if let Ok(aware) = DateTime::parse_from_rfc3339(raw) {
        return Ok(aware.with_timezone(&Utc));
    }
    let (naive_raw, offset) = match split_datetime_offset(raw) {
        Some((naive_raw, offset)) => (naive_raw, Some(offset)),
        None => (raw.as_str(), None),
    };
    let naive = parse_naive_datetime(naive_raw).ok_or_else(invalid)?;
    if let Some(offset) = offset {
        return Ok(naive.and_utc() - chrono::Duration::seconds(i64::from(offset)));
    }
    use chrono::TimeZone;
    match timezone.from_local_datetime(&naive) {
        chrono::LocalResult::Single(aware) => Ok(aware.with_timezone(&Utc)),
        chrono::LocalResult::Ambiguous(first, _) => Ok(first.with_timezone(&Utc)),
        chrono::LocalResult::None => Err(datetime_make_aware_message(timezone_name)),
    }
}

/// Render one datetime through the request user's zone, DRF
/// `isoformat` with the `+00:00` → `Z` rewrite
/// (`crate::serializer::render_datetime_in`).
fn render_datetime(dt: &DateTime<Utc>, timezone: &chrono_tz::Tz) -> String {
    crate::serializer::render_datetime_in(dt, timezone)
}

fn render_datetime_opt(dt: &Option<DateTime<Utc>>, timezone: &chrono_tz::Tz) -> Option<String> {
    dt.as_ref().map(|dt| render_datetime(dt, timezone))
}

// ---------------------------------------------------------------------------
// `partial_update` validation (`UserSerializer`, `user.py:15-61`)
// ---------------------------------------------------------------------------

const NULL_MESSAGE: &str = "This field may not be null.";
const INVALID_STRING_MESSAGE: &str = "Not a valid string.";
const BLANK_MESSAGE: &str = "This field may not be blank.";
const INVALID_BOOLEAN_MESSAGE: &str = "Must be a valid boolean.";
const INVALID_URL_MESSAGE: &str = "Enter a valid URL.";
const NULL_CHARACTERS_MESSAGE: &str = "Null characters are not allowed.";

fn max_length_message(max_length: usize) -> String {
    format!("Ensure this field has no more than {max_length} characters.")
}

/// Mirror DRF `CharField` input handling (`fields.py`): `null` fails
/// unless allowed; bools/composites fail; numbers coerce via `str()`;
/// the value strips; blank fails unless allowed; then `max_length` and
/// the NUL check accumulate in validator order. `None` (present null)
/// vs missing is distinguished by the caller.
fn char_field(
    value: &Value,
    allow_blank: bool,
    allow_null: bool,
    max_length: Option<usize>,
    invalid_message: &str,
) -> Result<Option<String>, Vec<String>> {
    if value.is_null() {
        if allow_null {
            return Ok(None);
        }
        return Err(vec![NULL_MESSAGE.to_owned()]);
    }
    let raw = match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => number.to_string(),
        _ => return Err(vec![invalid_message.to_owned()]),
    };
    let stripped = py_strip(&raw);
    if stripped.is_empty() {
        if !allow_blank {
            return Err(vec![BLANK_MESSAGE.to_owned()]);
        }
        return Ok(Some(String::new()));
    }
    let stripped = stripped.to_owned();
    let mut errors = Vec::new();
    if let Some(max) = max_length {
        if stripped.chars().count() > max {
            errors.push(max_length_message(max));
        }
    }
    if stripped.contains('\0') {
        errors.push(NULL_CHARACTERS_MESSAGE.to_owned());
    }
    if errors.is_empty() {
        Ok(Some(stripped))
    } else {
        Err(errors)
    }
}

/// DRF `BooleanField.to_internal_value` (`fields.py:699-711`, the
/// `app_notifications` precedent): `1`/`1.0` count as true, `0`/`0.0`
/// as false; the string sets match case-insensitively with no strip.
fn parse_drf_bool(value: &Value) -> Result<bool, ()> {
    match value {
        Value::Bool(flag) => Ok(*flag),
        Value::Number(number) => {
            if number.as_i64() == Some(1) || number.as_f64() == Some(1.0) {
                Ok(true)
            } else if number.as_i64() == Some(0) || number.as_f64() == Some(0.0) {
                Ok(false)
            } else {
                Err(())
            }
        }
        Value::String(text) => match text.to_lowercase().as_str() {
            "t" | "y" | "yes" | "true" | "on" | "1" => Ok(true),
            "f" | "n" | "no" | "false" | "off" | "0" => Ok(false),
            _ => Err(()),
        },
        _ => Err(()),
    }
}

fn bool_field(value: &Value) -> Result<bool, Vec<String>> {
    if value.is_null() {
        return Err(vec![NULL_MESSAGE.to_owned()]);
    }
    parse_drf_bool(value).map_err(|()| vec![INVALID_BOOLEAN_MESSAGE.to_owned()])
}

/// `USER_TIMEZONE_CHOICES` (`pytz.common_timezones`, pinned pytz==2024.1, 433 zones).
/// Exact membership list for the `user_timezone` ChoiceField.
const PYTZ_2024_1_ZONES: &[&str] = &[
    "Africa/Abidjan",
    "Africa/Accra",
    "Africa/Addis_Ababa",
    "Africa/Algiers",
    "Africa/Asmara",
    "Africa/Bamako",
    "Africa/Bangui",
    "Africa/Banjul",
    "Africa/Bissau",
    "Africa/Blantyre",
    "Africa/Brazzaville",
    "Africa/Bujumbura",
    "Africa/Cairo",
    "Africa/Casablanca",
    "Africa/Ceuta",
    "Africa/Conakry",
    "Africa/Dakar",
    "Africa/Dar_es_Salaam",
    "Africa/Djibouti",
    "Africa/Douala",
    "Africa/El_Aaiun",
    "Africa/Freetown",
    "Africa/Gaborone",
    "Africa/Harare",
    "Africa/Johannesburg",
    "Africa/Juba",
    "Africa/Kampala",
    "Africa/Khartoum",
    "Africa/Kigali",
    "Africa/Kinshasa",
    "Africa/Lagos",
    "Africa/Libreville",
    "Africa/Lome",
    "Africa/Luanda",
    "Africa/Lubumbashi",
    "Africa/Lusaka",
    "Africa/Malabo",
    "Africa/Maputo",
    "Africa/Maseru",
    "Africa/Mbabane",
    "Africa/Mogadishu",
    "Africa/Monrovia",
    "Africa/Nairobi",
    "Africa/Ndjamena",
    "Africa/Niamey",
    "Africa/Nouakchott",
    "Africa/Ouagadougou",
    "Africa/Porto-Novo",
    "Africa/Sao_Tome",
    "Africa/Tripoli",
    "Africa/Tunis",
    "Africa/Windhoek",
    "America/Adak",
    "America/Anchorage",
    "America/Anguilla",
    "America/Antigua",
    "America/Araguaina",
    "America/Argentina/Buenos_Aires",
    "America/Argentina/Catamarca",
    "America/Argentina/Cordoba",
    "America/Argentina/Jujuy",
    "America/Argentina/La_Rioja",
    "America/Argentina/Mendoza",
    "America/Argentina/Rio_Gallegos",
    "America/Argentina/Salta",
    "America/Argentina/San_Juan",
    "America/Argentina/San_Luis",
    "America/Argentina/Tucuman",
    "America/Argentina/Ushuaia",
    "America/Aruba",
    "America/Asuncion",
    "America/Atikokan",
    "America/Bahia",
    "America/Bahia_Banderas",
    "America/Barbados",
    "America/Belem",
    "America/Belize",
    "America/Blanc-Sablon",
    "America/Boa_Vista",
    "America/Bogota",
    "America/Boise",
    "America/Cambridge_Bay",
    "America/Campo_Grande",
    "America/Cancun",
    "America/Caracas",
    "America/Cayenne",
    "America/Cayman",
    "America/Chicago",
    "America/Chihuahua",
    "America/Ciudad_Juarez",
    "America/Costa_Rica",
    "America/Creston",
    "America/Cuiaba",
    "America/Curacao",
    "America/Danmarkshavn",
    "America/Dawson",
    "America/Dawson_Creek",
    "America/Denver",
    "America/Detroit",
    "America/Dominica",
    "America/Edmonton",
    "America/Eirunepe",
    "America/El_Salvador",
    "America/Fort_Nelson",
    "America/Fortaleza",
    "America/Glace_Bay",
    "America/Goose_Bay",
    "America/Grand_Turk",
    "America/Grenada",
    "America/Guadeloupe",
    "America/Guatemala",
    "America/Guayaquil",
    "America/Guyana",
    "America/Halifax",
    "America/Havana",
    "America/Hermosillo",
    "America/Indiana/Indianapolis",
    "America/Indiana/Knox",
    "America/Indiana/Marengo",
    "America/Indiana/Petersburg",
    "America/Indiana/Tell_City",
    "America/Indiana/Vevay",
    "America/Indiana/Vincennes",
    "America/Indiana/Winamac",
    "America/Inuvik",
    "America/Iqaluit",
    "America/Jamaica",
    "America/Juneau",
    "America/Kentucky/Louisville",
    "America/Kentucky/Monticello",
    "America/Kralendijk",
    "America/La_Paz",
    "America/Lima",
    "America/Los_Angeles",
    "America/Lower_Princes",
    "America/Maceio",
    "America/Managua",
    "America/Manaus",
    "America/Marigot",
    "America/Martinique",
    "America/Matamoros",
    "America/Mazatlan",
    "America/Menominee",
    "America/Merida",
    "America/Metlakatla",
    "America/Mexico_City",
    "America/Miquelon",
    "America/Moncton",
    "America/Monterrey",
    "America/Montevideo",
    "America/Montserrat",
    "America/Nassau",
    "America/New_York",
    "America/Nome",
    "America/Noronha",
    "America/North_Dakota/Beulah",
    "America/North_Dakota/Center",
    "America/North_Dakota/New_Salem",
    "America/Nuuk",
    "America/Ojinaga",
    "America/Panama",
    "America/Paramaribo",
    "America/Phoenix",
    "America/Port-au-Prince",
    "America/Port_of_Spain",
    "America/Porto_Velho",
    "America/Puerto_Rico",
    "America/Punta_Arenas",
    "America/Rankin_Inlet",
    "America/Recife",
    "America/Regina",
    "America/Resolute",
    "America/Rio_Branco",
    "America/Santarem",
    "America/Santiago",
    "America/Santo_Domingo",
    "America/Sao_Paulo",
    "America/Scoresbysund",
    "America/Sitka",
    "America/St_Barthelemy",
    "America/St_Johns",
    "America/St_Kitts",
    "America/St_Lucia",
    "America/St_Thomas",
    "America/St_Vincent",
    "America/Swift_Current",
    "America/Tegucigalpa",
    "America/Thule",
    "America/Tijuana",
    "America/Toronto",
    "America/Tortola",
    "America/Vancouver",
    "America/Whitehorse",
    "America/Winnipeg",
    "America/Yakutat",
    "Antarctica/Casey",
    "Antarctica/Davis",
    "Antarctica/DumontDUrville",
    "Antarctica/Macquarie",
    "Antarctica/Mawson",
    "Antarctica/McMurdo",
    "Antarctica/Palmer",
    "Antarctica/Rothera",
    "Antarctica/Syowa",
    "Antarctica/Troll",
    "Antarctica/Vostok",
    "Arctic/Longyearbyen",
    "Asia/Aden",
    "Asia/Almaty",
    "Asia/Amman",
    "Asia/Anadyr",
    "Asia/Aqtau",
    "Asia/Aqtobe",
    "Asia/Ashgabat",
    "Asia/Atyrau",
    "Asia/Baghdad",
    "Asia/Bahrain",
    "Asia/Baku",
    "Asia/Bangkok",
    "Asia/Barnaul",
    "Asia/Beirut",
    "Asia/Bishkek",
    "Asia/Brunei",
    "Asia/Chita",
    "Asia/Choibalsan",
    "Asia/Colombo",
    "Asia/Damascus",
    "Asia/Dhaka",
    "Asia/Dili",
    "Asia/Dubai",
    "Asia/Dushanbe",
    "Asia/Famagusta",
    "Asia/Gaza",
    "Asia/Hebron",
    "Asia/Ho_Chi_Minh",
    "Asia/Hong_Kong",
    "Asia/Hovd",
    "Asia/Irkutsk",
    "Asia/Jakarta",
    "Asia/Jayapura",
    "Asia/Jerusalem",
    "Asia/Kabul",
    "Asia/Kamchatka",
    "Asia/Karachi",
    "Asia/Kathmandu",
    "Asia/Khandyga",
    "Asia/Kolkata",
    "Asia/Krasnoyarsk",
    "Asia/Kuala_Lumpur",
    "Asia/Kuching",
    "Asia/Kuwait",
    "Asia/Macau",
    "Asia/Magadan",
    "Asia/Makassar",
    "Asia/Manila",
    "Asia/Muscat",
    "Asia/Nicosia",
    "Asia/Novokuznetsk",
    "Asia/Novosibirsk",
    "Asia/Omsk",
    "Asia/Oral",
    "Asia/Phnom_Penh",
    "Asia/Pontianak",
    "Asia/Pyongyang",
    "Asia/Qatar",
    "Asia/Qostanay",
    "Asia/Qyzylorda",
    "Asia/Riyadh",
    "Asia/Sakhalin",
    "Asia/Samarkand",
    "Asia/Seoul",
    "Asia/Shanghai",
    "Asia/Singapore",
    "Asia/Srednekolymsk",
    "Asia/Taipei",
    "Asia/Tashkent",
    "Asia/Tbilisi",
    "Asia/Tehran",
    "Asia/Thimphu",
    "Asia/Tokyo",
    "Asia/Tomsk",
    "Asia/Ulaanbaatar",
    "Asia/Urumqi",
    "Asia/Ust-Nera",
    "Asia/Vientiane",
    "Asia/Vladivostok",
    "Asia/Yakutsk",
    "Asia/Yangon",
    "Asia/Yekaterinburg",
    "Asia/Yerevan",
    "Atlantic/Azores",
    "Atlantic/Bermuda",
    "Atlantic/Canary",
    "Atlantic/Cape_Verde",
    "Atlantic/Faroe",
    "Atlantic/Madeira",
    "Atlantic/Reykjavik",
    "Atlantic/South_Georgia",
    "Atlantic/St_Helena",
    "Atlantic/Stanley",
    "Australia/Adelaide",
    "Australia/Brisbane",
    "Australia/Broken_Hill",
    "Australia/Darwin",
    "Australia/Eucla",
    "Australia/Hobart",
    "Australia/Lindeman",
    "Australia/Lord_Howe",
    "Australia/Melbourne",
    "Australia/Perth",
    "Australia/Sydney",
    "Canada/Atlantic",
    "Canada/Central",
    "Canada/Eastern",
    "Canada/Mountain",
    "Canada/Newfoundland",
    "Canada/Pacific",
    "Europe/Amsterdam",
    "Europe/Andorra",
    "Europe/Astrakhan",
    "Europe/Athens",
    "Europe/Belgrade",
    "Europe/Berlin",
    "Europe/Bratislava",
    "Europe/Brussels",
    "Europe/Bucharest",
    "Europe/Budapest",
    "Europe/Busingen",
    "Europe/Chisinau",
    "Europe/Copenhagen",
    "Europe/Dublin",
    "Europe/Gibraltar",
    "Europe/Guernsey",
    "Europe/Helsinki",
    "Europe/Isle_of_Man",
    "Europe/Istanbul",
    "Europe/Jersey",
    "Europe/Kaliningrad",
    "Europe/Kirov",
    "Europe/Kyiv",
    "Europe/Lisbon",
    "Europe/Ljubljana",
    "Europe/London",
    "Europe/Luxembourg",
    "Europe/Madrid",
    "Europe/Malta",
    "Europe/Mariehamn",
    "Europe/Minsk",
    "Europe/Monaco",
    "Europe/Moscow",
    "Europe/Oslo",
    "Europe/Paris",
    "Europe/Podgorica",
    "Europe/Prague",
    "Europe/Riga",
    "Europe/Rome",
    "Europe/Samara",
    "Europe/San_Marino",
    "Europe/Sarajevo",
    "Europe/Saratov",
    "Europe/Simferopol",
    "Europe/Skopje",
    "Europe/Sofia",
    "Europe/Stockholm",
    "Europe/Tallinn",
    "Europe/Tirane",
    "Europe/Ulyanovsk",
    "Europe/Vaduz",
    "Europe/Vatican",
    "Europe/Vienna",
    "Europe/Vilnius",
    "Europe/Volgograd",
    "Europe/Warsaw",
    "Europe/Zagreb",
    "Europe/Zurich",
    "GMT",
    "Indian/Antananarivo",
    "Indian/Chagos",
    "Indian/Christmas",
    "Indian/Cocos",
    "Indian/Comoro",
    "Indian/Kerguelen",
    "Indian/Mahe",
    "Indian/Maldives",
    "Indian/Mauritius",
    "Indian/Mayotte",
    "Indian/Reunion",
    "Pacific/Apia",
    "Pacific/Auckland",
    "Pacific/Bougainville",
    "Pacific/Chatham",
    "Pacific/Chuuk",
    "Pacific/Easter",
    "Pacific/Efate",
    "Pacific/Fakaofo",
    "Pacific/Fiji",
    "Pacific/Funafuti",
    "Pacific/Galapagos",
    "Pacific/Gambier",
    "Pacific/Guadalcanal",
    "Pacific/Guam",
    "Pacific/Honolulu",
    "Pacific/Kanton",
    "Pacific/Kiritimati",
    "Pacific/Kosrae",
    "Pacific/Kwajalein",
    "Pacific/Majuro",
    "Pacific/Marquesas",
    "Pacific/Midway",
    "Pacific/Nauru",
    "Pacific/Niue",
    "Pacific/Norfolk",
    "Pacific/Noumea",
    "Pacific/Pago_Pago",
    "Pacific/Palau",
    "Pacific/Pitcairn",
    "Pacific/Pohnpei",
    "Pacific/Port_Moresby",
    "Pacific/Rarotonga",
    "Pacific/Saipan",
    "Pacific/Tahiti",
    "Pacific/Tarawa",
    "Pacific/Tongatapu",
    "Pacific/Wake",
    "Pacific/Wallis",
    "US/Alaska",
    "US/Arizona",
    "US/Central",
    "US/Eastern",
    "US/Hawaii",
    "US/Mountain",
    "US/Pacific",
    "UTC",
];

/// DRF `ChoiceField` over `USER_TIMEZONE_CHOICES`
/// (`db/models/user.py:121-122`): exact string membership; anything
/// else fails with `"input" is not a valid choice.` (Python `str()`
/// rendering for scalars — the `v1_projects` precedent).
fn choice_timezone(value: &Value) -> Result<String, Vec<String>> {
    if value.is_null() {
        return Err(vec![NULL_MESSAGE.to_owned()]);
    }
    if let Value::String(text) = value {
        if PYTZ_2024_1_ZONES.contains(&text.as_str()) {
            return Ok(text.clone());
        }
        return Err(vec![format!("\"{text}\" is not a valid choice.")]);
    }
    let shown = match value {
        Value::Number(number) => number.to_string(),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        _ => value.to_string(),
    };
    Err(vec![format!("\"{shown}\" is not a valid choice.")])
}

/// Outcome of one asset-FK validation beyond the field error.
enum PkOutcome {
    /// Django `ValidationError` (unparseable UUID): the whole-body 400
    /// [`INVALID_DETAIL_BODY`].
    WholeBody,
    /// Exists-check DB failure: 500.
    ServerError,
}

/// Pure input half of the asset-FK check (DB-free, unit-tested):
/// `null` clears; bools fail `incorrect_type`; ints bind through
/// `uuid(int=...)` (`0` is the nil UUID, negatives fail); unparseable
/// strings, negative ints, floats, >u64 ints, and composites raise
/// Django `ValidationError` (`UUIDField.get_prep_value` wraps the
/// `ValueError`/`AttributeError` — probed on Django 4.2), which the
/// view answers with the whole-body 400.
fn asset_pk_input(value: &Value) -> Result<Option<uuid::Uuid>, Result<String, PkOutcome>> {
    if value.is_null() {
        return Ok(None);
    }
    if let Value::Bool(_) = value {
        return Err(Ok(
            "Incorrect type. Expected pk value, received bool.".to_owned()
        ));
    }
    match value {
        Value::String(raw) => match raw.parse::<uuid::Uuid>() {
            Ok(id) => Ok(Some(id)),
            Err(_) => Err(Err(PkOutcome::WholeBody)),
        },
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                if int < 0 {
                    return Err(Err(PkOutcome::WholeBody));
                }
                Ok(Some(uuid::Uuid::from_u128(int as u128)))
            } else if let Some(uint) = number.as_u64() {
                Ok(Some(uuid::Uuid::from_u128(u128::from(uint))))
            } else {
                Err(Err(PkOutcome::WholeBody))
            }
        }
        _ => Err(Err(PkOutcome::WholeBody)),
    }
}

/// DRF `PrimaryKeyRelatedField` over `FileAsset._default_manager`
/// (scoped `deleted_at IS NULL`): the input classifies through
/// [`asset_pk_input`], then the row must exist — a miss fails
/// `does_not_exist` with the original input echoed.
async fn validate_asset_pk(
    pool: &PgPool,
    value: &Value,
) -> Result<Option<uuid::Uuid>, Result<String, PkOutcome>> {
    let id = match asset_pk_input(value) {
        Ok(inner) => match inner {
            Some(id) => id,
            None => return Ok(None),
        },
        Err(outcome) => return Err(outcome),
    };
    let exists: Option<(uuid::Uuid,)> = sqlx::query_as(
        "SELECT id FROM file_assets WHERE file_assets.id = $1 AND file_assets.deleted_at IS NULL",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Err(PkOutcome::ServerError))?;
    if exists.is_none() {
        let shown = match value {
            Value::String(raw) => raw.clone(),
            Value::Number(number) => number.to_string(),
            _ => value.to_string(),
        };
        return Err(Ok(format!(
            "Invalid pk \"{shown}\" - object does not exist."
        )));
    }
    Ok(Some(id))
}

/// One `{"field": ["message"]}` entry, the serializer-errors shape.
fn field_error(message: String) -> Value {
    Value::Array(vec![Value::String(message)])
}

/// DRF `Serializer.to_internal_value` non-mapping branch
/// (`serializers.py`): a non-object body is a 400
/// `{"non_field_errors": ["Invalid data. Expected a dictionary, but
/// got {Type}."]}` with CPython type names — never a 500.
fn non_mapping_body(body: &Value) -> Value {
    let datatype = match body {
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
    };
    let mut errors = Map::new();
    errors.insert(
        "non_field_errors".to_owned(),
        Value::Array(vec![Value::String(format!(
            "Invalid data. Expected a dictionary, but got {datatype}."
        ))]),
    );
    Value::Object(errors)
}

/// A validated `PATCH users/me/` body: `None` per field means absent
/// (partial); inner `Option` is the nullable-column value.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UserPatch {
    pub last_login: Option<Option<DateTime<Utc>>>,
    pub display_name: Option<String>,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub avatar: Option<String>,
    pub avatar_asset: Option<Option<uuid::Uuid>>,
    pub cover_image: Option<Option<String>>,
    pub cover_image_asset: Option<Option<uuid::Uuid>>,
    pub is_password_expired: Option<bool>,
    pub is_password_reset_required: Option<bool>,
    pub bot_type: Option<Option<String>>,
    pub user_timezone: Option<String>,
    pub is_email_valid: Option<bool>,
    pub masked_at: Option<Option<DateTime<Utc>>>,
}

/// Validate the PATCH body through `UserSerializer`
/// (`user.py:15-61`): read-only keys (email included) and unknown keys
/// are silently ignored; errors accumulate in `Meta.fields` order.
/// `date_joined` is `auto_now_add` (DRF auto-read-only) and ignored.
async fn validate_patch_body(
    pool: &PgPool,
    body: &Map<String, Value>,
    timezone: &chrono_tz::Tz,
    timezone_name: &str,
) -> Result<UserPatch, Result<Value, PkOutcome>> {
    let mut errors = Map::new();
    let mut patch = UserPatch::default();

    if let Some(value) = body.get("last_login") {
        if value.is_null() {
            patch.last_login = Some(None);
        } else {
            match parse_drf_datetime(value, timezone, timezone_name) {
                Ok(dt) => patch.last_login = Some(Some(dt)),
                Err(message) => {
                    errors.insert("last_login".to_owned(), field_error(message));
                }
            }
        }
    }
    if let Some(value) = body.get("display_name") {
        match char_field(value, false, false, Some(255), INVALID_STRING_MESSAGE) {
            Ok(inner) => patch.display_name = inner,
            Err(messages) => {
                errors.insert(
                    "display_name".to_owned(),
                    Value::Array(messages.into_iter().map(Value::String).collect()),
                );
            }
        }
    }
    if let Some(value) = body.get("first_name") {
        match char_field(value, true, false, Some(255), INVALID_STRING_MESSAGE) {
            Ok(Some(text)) => match ser_user::validate_first_name(&text) {
                Ok(_) => patch.first_name = Some(text),
                Err(error) => {
                    errors.insert("first_name".to_owned(), field_error(error.to_string()));
                }
            },
            Ok(None) => {}
            Err(messages) => {
                errors.insert(
                    "first_name".to_owned(),
                    Value::Array(messages.into_iter().map(Value::String).collect()),
                );
            }
        }
    }
    if let Some(value) = body.get("last_name") {
        match char_field(value, true, false, Some(255), INVALID_STRING_MESSAGE) {
            Ok(Some(text)) => match ser_user::validate_last_name(&text) {
                Ok(_) => patch.last_name = Some(text),
                Err(error) => {
                    errors.insert("last_name".to_owned(), field_error(error.to_string()));
                }
            },
            Ok(None) => {}
            Err(messages) => {
                errors.insert(
                    "last_name".to_owned(),
                    Value::Array(messages.into_iter().map(Value::String).collect()),
                );
            }
        }
    }
    if let Some(value) = body.get("avatar") {
        match char_field(value, true, false, None, INVALID_STRING_MESSAGE) {
            Ok(inner) => patch.avatar = inner,
            Err(messages) => {
                errors.insert(
                    "avatar".to_owned(),
                    Value::Array(messages.into_iter().map(Value::String).collect()),
                );
            }
        }
    }
    if let Some(value) = body.get("avatar_asset") {
        match validate_asset_pk(pool, value).await {
            Ok(inner) => patch.avatar_asset = Some(inner),
            Err(Ok(message)) => {
                errors.insert("avatar_asset".to_owned(), field_error(message));
            }
            Err(Err(outcome)) => return Err(Err(outcome)),
        }
    }
    if let Some(value) = body.get("cover_image") {
        // `URLField` validators (`MaxLength(800)`, NUL, `URLValidator`)
        // accumulate: a long invalid URL reports both messages.
        match char_field(value, true, true, None, INVALID_URL_MESSAGE) {
            Ok(Some(text)) => {
                let mut url_errors = Vec::new();
                if text.chars().count() > 800 {
                    url_errors.push(max_length_message(800));
                }
                if text.contains('\0') {
                    url_errors.push(NULL_CHARACTERS_MESSAGE.to_owned());
                }
                // Django's `URLValidator` skips `EMPTY_VALUES` (`''`
                // passes after `allow_blank`); only non-blank values
                // face the validator.
                if !text.is_empty() && ser_extras::validate_user_link_url(&text).is_err() {
                    url_errors.push(INVALID_URL_MESSAGE.to_owned());
                }
                if url_errors.is_empty() {
                    patch.cover_image = Some(Some(text));
                } else {
                    errors.insert(
                        "cover_image".to_owned(),
                        Value::Array(url_errors.into_iter().map(Value::String).collect()),
                    );
                }
            }
            Ok(None) => patch.cover_image = Some(None),
            Err(messages) => {
                errors.insert(
                    "cover_image".to_owned(),
                    Value::Array(messages.into_iter().map(Value::String).collect()),
                );
            }
        }
    }
    if let Some(value) = body.get("cover_image_asset") {
        match validate_asset_pk(pool, value).await {
            Ok(inner) => patch.cover_image_asset = Some(inner),
            Err(Ok(message)) => {
                errors.insert("cover_image_asset".to_owned(), field_error(message));
            }
            Err(Err(outcome)) => return Err(Err(outcome)),
        }
    }
    if let Some(value) = body.get("is_password_expired") {
        match bool_field(value) {
            Ok(flag) => patch.is_password_expired = Some(flag),
            Err(messages) => {
                errors.insert(
                    "is_password_expired".to_owned(),
                    Value::Array(messages.into_iter().map(Value::String).collect()),
                );
            }
        }
    }
    if let Some(value) = body.get("is_password_reset_required") {
        match bool_field(value) {
            Ok(flag) => patch.is_password_reset_required = Some(flag),
            Err(messages) => {
                errors.insert(
                    "is_password_reset_required".to_owned(),
                    Value::Array(messages.into_iter().map(Value::String).collect()),
                );
            }
        }
    }
    if let Some(value) = body.get("bot_type") {
        match char_field(value, true, true, Some(30), INVALID_STRING_MESSAGE) {
            Ok(inner) => patch.bot_type = Some(inner),
            Err(messages) => {
                errors.insert(
                    "bot_type".to_owned(),
                    Value::Array(messages.into_iter().map(Value::String).collect()),
                );
            }
        }
    }
    if let Some(value) = body.get("user_timezone") {
        match choice_timezone(value) {
            Ok(zone) => patch.user_timezone = Some(zone),
            Err(messages) => {
                errors.insert(
                    "user_timezone".to_owned(),
                    Value::Array(messages.into_iter().map(Value::String).collect()),
                );
            }
        }
    }
    if let Some(value) = body.get("is_email_valid") {
        match bool_field(value) {
            Ok(flag) => patch.is_email_valid = Some(flag),
            Err(messages) => {
                errors.insert(
                    "is_email_valid".to_owned(),
                    Value::Array(messages.into_iter().map(Value::String).collect()),
                );
            }
        }
    }
    if let Some(value) = body.get("masked_at") {
        if value.is_null() {
            patch.masked_at = Some(None);
        } else {
            match parse_drf_datetime(value, timezone, timezone_name) {
                Ok(dt) => patch.masked_at = Some(Some(dt)),
                Err(message) => {
                    errors.insert("masked_at".to_owned(), field_error(message));
                }
            }
        }
    }

    if errors.is_empty() {
        Ok(patch)
    } else {
        Err(Ok(Value::Object(errors)))
    }
}

// ---------------------------------------------------------------------------
// Rendering (`UserMeSerializer` / `UserSerializer` via `ser_user`)
// ---------------------------------------------------------------------------

/// Owned render inputs: datetimes pre-rendered through the request
/// user's zone, ids stringified (the `BaseSerializer` id rule).
pub struct RenderedUser {
    pub id: String,
    pub last_login: Option<String>,
    pub username: String,
    pub mobile_number: Option<String>,
    pub email: Option<String>,
    pub display_name: String,
    pub first_name: String,
    pub last_name: String,
    pub avatar: String,
    pub avatar_asset: Option<String>,
    pub cover_image: Option<String>,
    pub cover_image_asset: Option<String>,
    pub avatar_url: Option<String>,
    pub cover_image_url: Option<String>,
    pub date_joined: String,
    pub created_at: String,
    pub updated_at: String,
    pub last_location: String,
    pub created_location: String,
    pub is_superuser: bool,
    pub is_managed: bool,
    pub is_password_expired: bool,
    pub is_active: bool,
    pub is_staff: bool,
    pub is_email_verified: bool,
    pub is_password_autoset: bool,
    pub is_password_reset_required: bool,
    pub token: String,
    pub last_active: Option<String>,
    pub last_login_time: Option<String>,
    pub last_logout_time: Option<String>,
    pub last_login_ip: String,
    pub last_logout_ip: String,
    pub last_login_medium: String,
    pub last_login_uagent: String,
    pub token_updated_at: Option<String>,
    pub is_bot: bool,
    pub bot_type: Option<String>,
    pub user_timezone: String,
    pub is_email_valid: bool,
    pub masked_at: Option<String>,
}

impl RenderedUser {
    fn me_row(&self) -> ser_user::UserMeRow<'_> {
        ser_user::UserMeRow {
            id: &self.id,
            avatar: &self.avatar,
            cover_image: self.cover_image.as_deref(),
            avatar_url: self.avatar_url.as_deref(),
            cover_image_url: self.cover_image_url.as_deref(),
            date_joined: &self.date_joined,
            display_name: &self.display_name,
            email: self.email.as_deref(),
            first_name: &self.first_name,
            last_name: &self.last_name,
            is_active: self.is_active,
            is_bot: self.is_bot,
            is_email_verified: self.is_email_verified,
            user_timezone: &self.user_timezone,
            username: &self.username,
            is_password_autoset: self.is_password_autoset,
            last_login_medium: &self.last_login_medium,
            last_login_time: self.last_login_time.as_deref(),
        }
    }

    fn full_row(&self) -> ser_user::UserRow<'_> {
        ser_user::UserRow {
            last_login: self.last_login.as_deref(),
            id: &self.id,
            username: &self.username,
            mobile_number: self.mobile_number.as_deref(),
            email: self.email.as_deref(),
            display_name: &self.display_name,
            first_name: &self.first_name,
            last_name: &self.last_name,
            avatar: &self.avatar,
            avatar_asset: self.avatar_asset.as_deref(),
            cover_image: self.cover_image.as_deref(),
            cover_image_asset: self.cover_image_asset.as_deref(),
            date_joined: &self.date_joined,
            created_at: &self.created_at,
            updated_at: &self.updated_at,
            last_location: &self.last_location,
            created_location: &self.created_location,
            is_superuser: self.is_superuser,
            is_managed: self.is_managed,
            is_password_expired: self.is_password_expired,
            is_active: self.is_active,
            is_staff: self.is_staff,
            is_email_verified: self.is_email_verified,
            is_password_autoset: self.is_password_autoset,
            is_password_reset_required: self.is_password_reset_required,
            token: &self.token,
            last_active: self.last_active.as_deref(),
            last_login_time: self.last_login_time.as_deref(),
            last_logout_time: self.last_logout_time.as_deref(),
            last_login_ip: &self.last_login_ip,
            last_logout_ip: &self.last_logout_ip,
            last_login_medium: &self.last_login_medium,
            last_login_uagent: &self.last_login_uagent,
            token_updated_at: self.token_updated_at.as_deref(),
            is_bot: self.is_bot,
            bot_type: self.bot_type.as_deref(),
            user_timezone: &self.user_timezone,
            is_email_valid: self.is_email_valid,
            masked_at: self.masked_at.as_deref(),
        }
    }

    /// `UserMeSerializer(request.user).data` (`user/base.py:78,249,365`).
    pub fn render_me(&self) -> Value {
        serde_json::to_value(ser_user::user_me_to_representation(&self.me_row()))
            .expect("me view serializes")
    }

    /// `UserSerializer` read shape (the `partial_update` 200).
    pub fn render_full(&self) -> Value {
        serde_json::to_value(ser_user::user_to_representation(&self.full_row()))
            .expect("user view serializes")
    }
}

/// Render one fetched user row: datetimes through `timezone`
/// (`TimezoneMixin`), asset URLs resolved.
async fn render_user(
    pool: &PgPool,
    row: &UserRow,
    timezone: &chrono_tz::Tz,
) -> Result<RenderedUser, Denial> {
    let avatar_url = resolve_avatar_url(pool, row.avatar_asset_id, &row.avatar).await?;
    let cover_image_url =
        resolve_cover_image_url(pool, row.cover_image_asset_id, row.cover_image.as_deref()).await?;
    Ok(RenderedUser {
        id: row.id.to_string(),
        last_login: render_datetime_opt(&row.last_login, timezone),
        username: row.username.clone(),
        mobile_number: row.mobile_number.clone(),
        email: row.email.clone(),
        display_name: row.display_name.clone(),
        first_name: row.first_name.clone(),
        last_name: row.last_name.clone(),
        avatar: row.avatar.clone(),
        avatar_asset: row.avatar_asset_id.map(|id| id.to_string()),
        cover_image: row.cover_image.clone(),
        cover_image_asset: row.cover_image_asset_id.map(|id| id.to_string()),
        avatar_url,
        cover_image_url,
        date_joined: render_datetime(&row.date_joined, timezone),
        created_at: render_datetime(&row.created_at, timezone),
        updated_at: render_datetime(&row.updated_at, timezone),
        last_location: row.last_location.clone(),
        created_location: row.created_location.clone(),
        is_superuser: row.is_superuser,
        is_managed: row.is_managed,
        is_password_expired: row.is_password_expired,
        is_active: row.is_active,
        is_staff: row.is_staff,
        is_email_verified: row.is_email_verified,
        is_password_autoset: row.is_password_autoset,
        is_password_reset_required: row.is_password_reset_required,
        token: row.token.clone(),
        last_active: render_datetime_opt(&row.last_active, timezone),
        last_login_time: render_datetime_opt(&row.last_login_time, timezone),
        last_logout_time: render_datetime_opt(&row.last_logout_time, timezone),
        last_login_ip: row.last_login_ip.clone(),
        last_logout_ip: row.last_logout_ip.clone(),
        last_login_medium: row.last_login_medium.clone(),
        last_login_uagent: row.last_login_uagent.clone(),
        token_updated_at: render_datetime_opt(&row.token_updated_at, timezone),
        is_bot: row.is_bot,
        bot_type: row.bot_type.clone(),
        user_timezone: row.user_timezone.clone(),
        is_email_valid: row.is_email_valid,
        masked_at: render_datetime_opt(&row.masked_at, timezone),
    })
}

// ---------------------------------------------------------------------------
// Cache (the `auth_session::magic` `Cache` precedent: GET/SETEX/DEL)
// ---------------------------------------------------------------------------

/// Short-lived multiplexed client off the same `REDIS_URL` — `None`
/// when the URL is unset, in which case every cache touch fails the
/// way a dead Django cache fails on that path (see each call site).
#[derive(Debug, Clone)]
struct Cache {
    client: redis::Client,
}

impl Cache {
    fn from_state(state: &AppState) -> Option<Self> {
        let url = state
            .settings()
            .redis
            .url
            .as_deref()
            .filter(|u| !u.is_empty())?;
        redis::Client::open(url).ok().map(|client| Self { client })
    }

    async fn get(&self, key: &str) -> Result<Option<String>, redis::RedisError> {
        let mut conn = self.client.get_multiplexed_async_connection().await?;
        redis::AsyncCommands::get(&mut conn, key).await
    }

    async fn set_ex(
        &self,
        key: &str,
        value: &str,
        expiry_secs: u64,
    ) -> Result<(), redis::RedisError> {
        let mut conn = self.client.get_multiplexed_async_connection().await?;
        redis::AsyncCommands::set_ex::<_, _, ()>(&mut conn, key, value, expiry_secs).await
    }

    async fn del(&self, key: &str) -> Result<(), redis::RedisError> {
        let mut conn = self.client.get_multiplexed_async_connection().await?;
        redis::AsyncCommands::del::<_, ()>(&mut conn, key).await
    }
}

// ---------------------------------------------------------------------------
// Throttle (generate-code only, `EmailVerificationThrottle` 3/hour)
// ---------------------------------------------------------------------------

/// Evaluate the wired throttle (DRF `initial()` order — after auth, so
/// the caller is authenticated and the key is the user pk). Histories
/// are compact-JSON float arrays; a miss, an unreadable value, a
/// failed re-cache, or a missing cache fails open to allow (the merged
/// magic/assistant-throttle precedent).
async fn check_email_throttle(cache: Option<&Cache>, user_id: &uuid::Uuid) -> bool {
    let spec = gates::EMAIL_VERIFICATION_THROTTLE;
    let key = throttle_kernel::user_cache_key(spec.scope, Some(&user_id.to_string()), "");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    let history: Vec<f64> = match cache {
        Some(cache) => match cache.get(&key).await {
            Ok(Some(raw)) => serde_json::from_str(&raw).unwrap_or_default(),
            _ => Vec::new(),
        },
        None => Vec::new(),
    };
    let decision = throttle_kernel::allow_request(&history, spec.requests, spec.window_secs, now);
    if !decision.allowed {
        return false;
    }
    if let Some(cache) = cache {
        if let Err(error) = cache
            .set_ex(
                &key,
                &serde_json::to_string(&decision.history).expect("float vec serializes"),
                spec.window_secs,
            )
            .await
        {
            tracing::debug!(%error, key = key.as_str(), "email throttle: re-cache failed; allowance stands");
        }
    }
    true
}

// ---------------------------------------------------------------------------
// Tasks (best-effort post-write enqueue, the `app_modules` precedent)
// ---------------------------------------------------------------------------

async fn enqueue_emit(pool: &PgPool, task: &str, args: Vec<Value>) {
    let message = pidash_jobs::celery::CeleryTaskMessage::new(task, args, Map::new());
    let job = pidash_jobs::queue::NewJob::new(
        message.task.clone(),
        Value::Array(message.args.clone()),
        Value::Object(message.kwargs.clone()),
    );
    if let Err(error) = pidash_jobs::queue::enqueue(pool, &job).await {
        tracing::warn!(%error, task, "task enqueue failed; response stands");
    }
}

// ---------------------------------------------------------------------------
// Logout (`django.contrib.auth.logout`: flush + delete-cookie via middleware)
// ---------------------------------------------------------------------------

/// `logout(request)` (`user/base.py:241,355`): delete the presented
/// session row ([`queries_user::session_flush_sql`]) and `clear()` the
/// live session, so the session middleware emits the delete-cookie +
/// `Vary: Cookie` exactly like Django's `process_response`.
async fn logout(pool: &PgPool, extension: &Option<Extension<SessionHandle>>) -> Result<(), Denial> {
    let key = extension
        .as_ref()
        .map(|Extension(handle)| handle.snapshot().key)
        .unwrap_or(None);
    if let Some(key) = key {
        let sql = positional(&queries_user::session_flush_sql(), &["key"]);
        sqlx::query(&sql)
            .bind(key)
            .execute(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    }
    if let Some(Extension(handle)) = extension {
        handle.lock().clear();
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// `_validate_new_email` (`user/base.py:95-135`)
// ---------------------------------------------------------------------------

/// The 4-branch email validation shared by generate-code and
/// update-email. `current_email` is the user's stored address.
async fn validate_new_email(
    pool: &PgPool,
    user_id: uuid::Uuid,
    current_email: Option<&str>,
    new_email: &str,
) -> Result<(), Denial> {
    if new_email.is_empty() {
        return Err(Denial::BadError("Email is required".to_owned()));
    }
    if !crate::v1_projects::handlers_members::is_valid_email(new_email) {
        return Err(Denial::BadError("Invalid email format".to_owned()));
    }
    if Some(new_email) == current_email {
        return Err(Denial::BadError(
            "New email must be different from current email".to_owned(),
        ));
    }
    let sql = positional(
        &queries_user::email_availability_exists_sql(),
        &["email", "user"],
    );
    let taken: Option<(i32,)> = sqlx::query_as(&sql)
        .bind(new_email)
        .bind(user_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    if taken.is_some() {
        return Err(Denial::BadError(
            "An account with this email already exists".to_owned(),
        ));
    }
    Ok(())
}

/// `request.data.get(key, "")` for the email endpoints: the body must
/// be an object and a present value must be a string — anything else
/// is the `AttributeError` 500 (`None.strip()`, `list.get`).
fn email_body_string(body: &Value, key: &str) -> Result<String, Denial> {
    let Value::Object(map) = body else {
        return Err(Denial::ServerError);
    };
    match map.get(key) {
        None => Ok(String::new()),
        Some(Value::String(text)) => Ok(text.clone()),
        Some(_) => Err(Denial::ServerError),
    }
}

// ---------------------------------------------------------------------------
// `User.save()` derived values (`db/models/user.py:169-187`)
// ---------------------------------------------------------------------------

/// The `User.save()` side effects shared by every user write
/// (`models_user::user`): email normalization (`None` is the
/// `AttributeError` 500), display-name backfill, token regen (iff
/// `token_updated_at` is set — then it re-stamps, forever), superuser
/// → staff.
struct SaveDerived {
    email: String,
    display_name: String,
    token: String,
    token_updated_at: Option<DateTime<Utc>>,
    is_staff: bool,
}

fn derive_save(
    email: Option<&str>,
    display_name: &str,
    token: &str,
    token_updated_at: Option<DateTime<Utc>>,
    is_superuser: bool,
    is_staff: bool,
    now: DateTime<Utc>,
) -> Result<SaveDerived, Denial> {
    use models_user::user as user_model;
    let email = user_model::save_email(email).map_err(|_| Denial::ServerError)?;
    let display_name = user_model::save_display_name(display_name, &email);
    let (token, token_updated_at) =
        if user_model::should_regenerate_token(token_updated_at.is_some()) {
            (user_model::regenerated_token(), Some(now))
        } else {
            (token.to_owned(), None)
        };
    Ok(SaveDerived {
        email,
        display_name,
        token,
        token_updated_at,
        is_staff: user_model::save_is_staff(is_superuser, is_staff),
    })
}

/// Invite-count statement, positional. `email_none` renders the
/// Django `filter(email=None)` `IS NULL` arm (no bind).
fn invite_count_statement(email_none: bool) -> String {
    let sql = positional(&queries_user::invite_count_sql(), &["email"]);
    if email_none {
        sql.replace(
            "workspace_member_invites.email = $1",
            "workspace_member_invites.email IS NULL",
        )
    } else {
        sql
    }
}

/// Instance-admin exists statement, positional. `instance_none`
/// renders the `instance_id IS NULL` arm and renumbers the surviving
/// user predicate to `$1`.
fn instance_admin_exists_statement(instance_none: bool) -> String {
    let sql = positional(
        &queries_user::instance_admin_exists_sql(),
        &["instance", "user"],
    );
    if instance_none {
        sql.replace(
            "instance_admins.instance_id = $1",
            "instance_admins.instance_id IS NULL",
        )
        .replace(
            "instance_admins.user_id = $2",
            "instance_admins.user_id = $1",
        )
    } else {
        sql
    }
}

// ---------------------------------------------------------------------------
// Handlers — core (`user/base.py:59-93`)
// ---------------------------------------------------------------------------

/// Attach the decorator headers for one route+method
/// (`gates::headers_for`): `Cache-Control: private, max-age=12` +
/// `Vary: Cookie` on the me/settings GETs.
fn with_route_headers(mut response: Response, method: &str, path: &str) -> Response {
    let headers = gates::headers_for(method, path);
    if let Some(cache_control) = headers.cache_control {
        if let Ok(value) = header::HeaderValue::from_str(cache_control) {
            response.headers_mut().insert(header::CACHE_CONTROL, value);
        }
    }
    if let Some(vary) = headers.vary {
        if let Ok(value) = header::HeaderValue::from_str(vary) {
            response.headers_mut().insert(header::VARY, value);
        }
    }
    response
}

/// `GET /api/users/me/` (`user/base.py:75-79`): the `UserMe` shape with
/// the `cache_control` + `vary_on_cookie` decorator headers.
pub async fn me_retrieve(
    State(state): State<AppState>,
    extension: Option<Extension<SessionHandle>>,
) -> Response {
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let actor = match authed_actor(&state, pool, extension).await {
        Ok(actor) => actor,
        Err(denial) => return denial.into_response(),
    };
    let row = match fetch_user(pool, actor.id).await {
        Ok(row) => row,
        Err(denial) => return denial.into_response(),
    };
    let rendered = match render_user(pool, &row, &actor.timezone).await {
        Ok(rendered) => rendered,
        Err(denial) => return denial.into_response(),
    };
    let response = (StatusCode::OK, Json(rendered.render_me())).into_response();
    with_route_headers(response, "GET", "users/me/")
}

/// `GET /api/users/me/settings/` (`user/base.py:81-85` + serializer
/// `user.py:90-139`): `id`/`email`/`workspace` with the same decorator
/// headers. The invite count runs on every call; a missing profile is
/// the 404; the last-workspace branch needs the membership check, else
/// the earliest-created fallback.
pub async fn me_settings(
    State(state): State<AppState>,
    extension: Option<Extension<SessionHandle>>,
) -> Response {
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let actor = match authed_actor(&state, pool, extension).await {
        Ok(actor) => actor,
        Err(denial) => return denial.into_response(),
    };
    let row = match fetch_user(pool, actor.id).await {
        Ok(row) => row,
        Err(denial) => return denial.into_response(),
    };
    // Invite count (`user.py:99`, no `accepted` filter — bug 1). A
    // `None` email renders `IS NULL`, like Django's `filter(email=None)`.
    let invites: Option<(i64,)> = match row.email.as_deref() {
        Some(email) => {
            let count_sql = invite_count_statement(false);
            match sqlx::query_as(&count_sql)
                .bind(email)
                .fetch_optional(pool)
                .await
            {
                Ok(count) => count,
                Err(_) => return Denial::ServerError.into_response(),
            }
        }
        None => {
            let null_sql = invite_count_statement(true);
            match sqlx::query_as(&null_sql).fetch_optional(pool).await {
                Ok(count) => count,
                Err(_) => return Denial::ServerError.into_response(),
            }
        }
    };
    let invites = invites.map(|row| row.0).unwrap_or(0);
    let profile = match fetch_profile(pool, actor.id).await {
        Ok(profile) => profile,
        Err(denial) => return denial.into_response(),
    };
    // Last-workspace branch (`user.py:103-125`): set id + active
    // membership check, then the re-fetch (bug 3: a second round-trip).
    let mut last_workspace: Option<ser_user::MeSettingsLastWorkspace<'_>> = None;
    let mut last_strings: Option<(String, String, String, String)> = None;
    if let Some(last_id) = profile.last_workspace_id {
        let check_sql = positional(&queries_user::last_workspace_exists_sql(), &["ws", "user"]);
        let member: Option<(i32,)> = match sqlx::query_as(&check_sql)
            .bind(last_id)
            .bind(actor.id)
            .fetch_optional(pool)
            .await
        {
            Ok(member) => member,
            Err(_) => return Denial::ServerError.into_response(),
        };
        if member.is_some() {
            let fetch_sql = positional(&queries_user::last_workspace_fetch_sql(), &["ws", "user"]);
            let workspace: Option<SettingsWorkspaceRow> = match sqlx::query_as(&fetch_sql)
                .bind(last_id)
                .bind(actor.id)
                .fetch_optional(pool)
                .await
            {
                Ok(workspace) => workspace,
                Err(_) => return Denial::ServerError.into_response(),
            };
            // The `.first()` miss after a passing `.exists()` is the
            // `AttributeError` 500 (`user.py:116` dereferences first).
            let Some(workspace) = workspace else {
                return Denial::ServerError.into_response();
            };
            let SettingsWorkspaceRow {
                id,
                slug,
                name,
                logo_asset_id,
            } = workspace;
            let logo = match logo_asset_id {
                None => String::new(),
                Some(asset_id) => {
                    let asset = match fetch_asset(pool, asset_id).await {
                        Ok(asset) => asset,
                        Err(denial) => return denial.into_response(),
                    };
                    match render_asset_url(pool, &asset).await {
                        Ok(url) => url.unwrap_or_default(),
                        Err(denial) => return denial.into_response(),
                    }
                }
            };
            last_strings = Some((id.to_string(), slug, name, logo));
        }
    }
    // Fallback (`user.py:127-131`): only the else branch queries — when
    // the last-workspace branch hit, Python returns without touching
    // the earliest-created membership.
    let fallback_strings: Option<(String, String)> = if last_strings.is_none() {
        let fallback_sql = positional(&queries_user::fallback_workspace_sql(), &["user"]);
        let fallback: Option<SettingsWorkspaceRow> = match sqlx::query_as(&fallback_sql)
            .bind(actor.id)
            .fetch_optional(pool)
            .await
        {
            Ok(fallback) => fallback,
            Err(_) => return Denial::ServerError.into_response(),
        };
        fallback.map(|row| (row.id.to_string(), row.slug))
    } else {
        None
    };
    if let Some((id, slug, name, logo)) = last_strings.as_ref() {
        last_workspace = Some(ser_user::MeSettingsLastWorkspace {
            id,
            slug,
            name,
            logo,
        });
    }
    let fallback_workspace = fallback_strings
        .as_ref()
        .map(|(id, slug)| ser_user::MeSettingsFallbackWorkspace { id, slug });
    let id = row.id.to_string();
    let settings_row = ser_user::UserMeSettingsRow {
        id: &id,
        email: row.email.as_deref(),
        last_workspace,
        fallback_workspace,
        invites,
    };
    let view = ser_user::me_settings_to_representation(&settings_row);
    let body = serde_json::to_value(view).expect("settings view serializes");
    let response = (StatusCode::OK, Json(body)).into_response();
    with_route_headers(response, "GET", "users/me/settings/")
}

/// `GET /api/users/me/instance-admin/` (`user/base.py:87-90`):
/// `Instance.first` + `InstanceAdmin` exists →
/// `{"is_instance_admin"}`. A missing `Instance` row binds `IS NULL`.
pub async fn me_instance_admin(
    State(state): State<AppState>,
    extension: Option<Extension<SessionHandle>>,
) -> Response {
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let actor = match authed_actor(&state, pool, extension).await {
        Ok(actor) => actor,
        Err(denial) => return denial.into_response(),
    };
    let first_sql = positional(&queries_user::instance_first_sql(), &[]);
    let instance: Option<InstanceRow> = match sqlx::query_as(&first_sql).fetch_optional(pool).await
    {
        Ok(instance) => instance,
        Err(_) => return Denial::ServerError.into_response(),
    };
    // A missing `Instance` row binds `IS NULL` (`:89`); the surviving
    // `$2` renumbers to `$1` since binds are positional by order.
    let admin: Option<(i32,)> = match instance {
        Some(row) => {
            let id = row.id;
            let exists_sql = instance_admin_exists_statement(false);
            match sqlx::query_as(&exists_sql)
                .bind(id)
                .bind(actor.id)
                .fetch_optional(pool)
                .await
            {
                Ok(admin) => admin,
                Err(_) => return Denial::ServerError.into_response(),
            }
        }
        None => {
            let null_sql = instance_admin_exists_statement(true);
            match sqlx::query_as(&null_sql)
                .bind(actor.id)
                .fetch_optional(pool)
                .await
            {
                Ok(admin) => admin,
                Err(_) => return Denial::ServerError.into_response(),
            }
        }
    };
    let mut body = Map::new();
    body.insert("is_instance_admin".to_owned(), Value::Bool(admin.is_some()));
    (StatusCode::OK, Json(Value::Object(body))).into_response()
}

/// `PATCH /api/users/me/` (`user/base.py:92-93` → DRF
/// `partial_update`): validate through `UserSerializer`, save with the
/// `User.save()` side effects, 200 with the full 39-key shape rendered
/// in the pre-patch zone (`TimezoneMixin.initial` runs first).
pub async fn me_partial_update(
    State(state): State<AppState>,
    extension: Option<Extension<SessionHandle>>,
    req: Request,
) -> Response {
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let actor = match authed_actor(&state, pool, extension).await {
        Ok(actor) => actor,
        Err(denial) => return denial.into_response(),
    };
    let body = match read_json_body(req).await {
        Ok(body) => body,
        Err(denial) => return denial.into_response(),
    };
    // `serializer(data=request.data)`: a non-object body is the
    // DRF non-mapping 400 (`non_field_errors`).
    let map = match &body {
        Value::Object(map) => map,
        other => return Denial::BadJson(non_mapping_body(other)).into_response(),
    };
    let row = match fetch_user(pool, actor.id).await {
        Ok(row) => row,
        Err(denial) => return denial.into_response(),
    };
    let patch = match validate_patch_body(pool, map, &actor.timezone, &row.user_timezone).await {
        Ok(patch) => patch,
        Err(Ok(errors)) => return Denial::BadJson(errors).into_response(),
        Err(Err(PkOutcome::WholeBody)) => return Denial::InvalidDetail.into_response(),
        Err(Err(PkOutcome::ServerError)) => return Denial::ServerError.into_response(),
    };
    // Merge over the current row, then `User.save()`.
    let now = Utc::now();
    let merged = UserRow {
        last_login: patch.last_login.unwrap_or(row.last_login),
        display_name: patch
            .display_name
            .unwrap_or_else(|| row.display_name.clone()),
        first_name: patch.first_name.unwrap_or_else(|| row.first_name.clone()),
        last_name: patch.last_name.unwrap_or_else(|| row.last_name.clone()),
        avatar: patch.avatar.unwrap_or_else(|| row.avatar.clone()),
        avatar_asset_id: patch.avatar_asset.unwrap_or(row.avatar_asset_id),
        cover_image: patch.cover_image.unwrap_or_else(|| row.cover_image.clone()),
        cover_image_asset_id: patch.cover_image_asset.unwrap_or(row.cover_image_asset_id),
        is_password_expired: patch.is_password_expired.unwrap_or(row.is_password_expired),
        is_password_reset_required: patch
            .is_password_reset_required
            .unwrap_or(row.is_password_reset_required),
        bot_type: patch.bot_type.unwrap_or_else(|| row.bot_type.clone()),
        user_timezone: patch
            .user_timezone
            .unwrap_or_else(|| row.user_timezone.clone()),
        is_email_valid: patch.is_email_valid.unwrap_or(row.is_email_valid),
        masked_at: patch.masked_at.unwrap_or(row.masked_at),
        updated_at: now,
        ..row.clone()
    };
    let derived = match derive_save(
        row.email.as_deref(),
        &merged.display_name,
        &row.token,
        row.token_updated_at,
        row.is_superuser,
        row.is_staff,
        now,
    ) {
        Ok(derived) => derived,
        Err(denial) => return denial.into_response(),
    };
    let update = sqlx::query(
        "UPDATE users SET last_login = $1, display_name = $2, first_name = $3, last_name = $4, \
        avatar = $5, avatar_asset_id = $6, cover_image = $7, cover_image_asset_id = $8, \
        is_password_expired = $9, is_password_reset_required = $10, bot_type = $11, \
        user_timezone = $12, is_email_valid = $13, masked_at = $14, email = $15, token = $16, \
        token_updated_at = $17, is_staff = $18, updated_at = $19 WHERE id = $20",
    )
    .bind(merged.last_login)
    .bind(&merged.display_name)
    .bind(&merged.first_name)
    .bind(&merged.last_name)
    .bind(&merged.avatar)
    .bind(merged.avatar_asset_id)
    .bind(merged.cover_image.as_deref())
    .bind(merged.cover_image_asset_id)
    .bind(merged.is_password_expired)
    .bind(merged.is_password_reset_required)
    .bind(merged.bot_type.as_deref())
    .bind(&merged.user_timezone)
    .bind(merged.is_email_valid)
    .bind(merged.masked_at)
    .bind(&derived.email)
    .bind(&derived.token)
    .bind(derived.token_updated_at)
    .bind(derived.is_staff)
    .bind(now)
    .bind(row.id)
    .execute(pool)
    .await;
    if let Err(error) = update {
        return write_denial(error).into_response();
    }
    // The 200 re-renders the saved in-memory row (the `app_scheduler`
    // PATCH precedent): merged values + save-derived values.
    let saved = UserRow {
        display_name: derived.display_name,
        email: Some(derived.email),
        token: derived.token,
        token_updated_at: derived.token_updated_at,
        is_staff: derived.is_staff,
        ..merged
    };
    let rendered = match render_user(pool, &saved, &actor.timezone).await {
        Ok(rendered) => rendered,
        Err(denial) => return denial.into_response(),
    };
    (StatusCode::OK, Json(rendered.render_full())).into_response()
}

// ---------------------------------------------------------------------------
// Handlers — email change (`user/base.py:137-250`)
// ---------------------------------------------------------------------------

/// Draw the 6-digit code (`secrets.randbelow(900000) + 100000`,
/// `user/base.py:157`).
fn draw_email_code() -> String {
    use rand::Rng;
    rand::rng()
        .random_range(queries_user::EMAIL_UPDATE_CODE_MIN..=queries_user::EMAIL_UPDATE_CODE_MAX)
        .to_string()
}

/// `POST /api/users/me/email/generate-code/` (`user/base.py:137-174`):
/// throttle (3/hour, consumed even on validation failure), the
/// 4-branch validation, the 6-digit code to cache (600s), the
/// magic-code enqueue. Any failure inside the `try` answers 400
/// "Failed to generate...".
pub async fn email_generate_code(
    State(state): State<AppState>,
    extension: Option<Extension<SessionHandle>>,
    req: Request,
) -> Response {
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let actor = match authed_actor(&state, pool, extension).await {
        Ok(actor) => actor,
        Err(denial) => return denial.into_response(),
    };
    let cache = Cache::from_state(&state);
    if !check_email_throttle(cache.as_ref(), &actor.id).await {
        return Denial::Throttled.into_response();
    }
    let body = match read_json_body(req).await {
        Ok(body) => body,
        Err(denial) => return denial.into_response(),
    };
    let raw_email = match email_body_string(&body, "email") {
        Ok(raw) => raw,
        Err(denial) => return denial.into_response(),
    };
    let new_email = normalize_email(&raw_email);
    let row = match fetch_user(pool, actor.id).await {
        Ok(row) => row,
        Err(denial) => return denial.into_response(),
    };
    if let Err(denial) = validate_new_email(pool, actor.id, row.email.as_deref(), &new_email).await
    {
        return denial.into_response();
    }
    // The `try` body (`:151-168`): cache write + enqueue.
    let key = queries_user::email_update_cache_key(&actor.id.to_string(), &new_email);
    let token = draw_email_code();
    let value = queries_user::email_update_cache_value(&token);
    let stored = match cache.as_ref() {
        Some(cache) => cache
            .set_ex(
                &key,
                &value,
                u64::from(queries_user::EMAIL_UPDATE_CODE_TTL_SECS),
            )
            .await
            .is_ok(),
        None => false,
    };
    if !stored {
        return json_response(StatusCode::BAD_REQUEST, GENERATE_FAILED_BODY);
    }
    let emit = tasks::email_update_magic_code_emit(&new_email, &token);
    enqueue_emit(pool, emit.task_name(), emit.args()).await;
    json_response(StatusCode::OK, CODE_SENT_BODY)
}

/// Python `str()` over a cached token value (`user/base.py:212`):
/// strings as-is, `True`/`False`/`None` capitalized, numbers plain.
fn py_token_str(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Bool(true) => Some("True".to_owned()),
        Value::Bool(false) => Some("False".to_owned()),
        Value::Null => Some("None".to_owned()),
        Value::Number(number) => Some(number.to_string()),
        Value::Array(_) | Value::Object(_) => None,
    }
}

/// Outcome of the cache-verification step (`user/base.py:198-222`).
#[derive(Debug, PartialEq)]
enum VerifyOutcome {
    Match,
    Expired,
    Failed,
    Mismatch,
}

/// Pure verify step (DB/cache-free, unit-tested): a missing or empty
/// cache value reads as expired (`if not cached_data`); unparseable
/// JSON, non-object JSON, or a composite token reads as
/// failed-verify; a missing `token` key reads as `None`
/// (`data.get("token")` → `None`, and `str(None) == "None"` can
/// still match the literal code).
fn verify_cached_code(cached: Option<&str>, code: &str) -> VerifyOutcome {
    let Some(cached) = cached else {
        return VerifyOutcome::Expired;
    };
    if cached.is_empty() {
        return VerifyOutcome::Expired;
    }
    let Ok(data) = serde_json::from_str::<Value>(cached) else {
        return VerifyOutcome::Failed;
    };
    if !data.is_object() {
        return VerifyOutcome::Failed;
    }
    let token = data.get("token").unwrap_or(&Value::Null);
    let Some(stored) = py_token_str(token) else {
        return VerifyOutcome::Failed;
    };
    if stored == code {
        VerifyOutcome::Match
    } else {
        VerifyOutcome::Mismatch
    }
}

/// `PATCH /api/users/me/email/` (`user/base.py:176-250`): validation,
/// the code-required 400, cache verification (expired/invalid/mismatch
/// bodies), the availability re-check, the save
/// (`is_email_verified=False` + `User.save()` side effects), cache
/// delete, logout, the two confirmation enqueues, the `UserMe` shape.
pub async fn email_update(
    State(state): State<AppState>,
    extension: Option<Extension<SessionHandle>>,
    req: Request,
) -> Response {
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let actor = match authed_actor(&state, pool, extension.clone()).await {
        Ok(actor) => actor,
        Err(denial) => return denial.into_response(),
    };
    let body = match read_json_body(req).await {
        Ok(body) => body,
        Err(denial) => return denial.into_response(),
    };
    let raw_email = match email_body_string(&body, "email") {
        Ok(raw) => raw,
        Err(denial) => return denial.into_response(),
    };
    let raw_code = match email_body_string(&body, "code") {
        Ok(raw) => raw,
        Err(denial) => return denial.into_response(),
    };
    let new_email = normalize_email(&raw_email);
    let code = py_strip(&raw_code).to_owned();
    let row = match fetch_user(pool, actor.id).await {
        Ok(row) => row,
        Err(denial) => return denial.into_response(),
    };
    if let Err(denial) = validate_new_email(pool, actor.id, row.email.as_deref(), &new_email).await
    {
        return denial.into_response();
    }
    if code.is_empty() {
        return json_response(StatusCode::BAD_REQUEST, CODE_REQUIRED_BODY);
    }
    // The verify `try` (`:198-222`): any failure answers 400
    // "Failed to verify code. Please try again."
    let key = queries_user::email_update_cache_key(&actor.id.to_string(), &new_email);
    let cached: Option<String> = match cache_get(&state, &key).await {
        Ok(cached) => cached,
        Err(()) => return json_response(StatusCode::BAD_REQUEST, CODE_FAILED_BODY),
    };
    match verify_cached_code(cached.as_deref(), &code) {
        VerifyOutcome::Match => {}
        VerifyOutcome::Expired => {
            return json_response(StatusCode::BAD_REQUEST, CODE_EXPIRED_BODY);
        }
        VerifyOutcome::Failed => {
            return json_response(StatusCode::BAD_REQUEST, CODE_FAILED_BODY);
        }
        VerifyOutcome::Mismatch => {
            return json_response(StatusCode::BAD_REQUEST, CODE_INVALID_BODY);
        }
    }
    // Final availability re-check (`:225-229`), then the save.
    let recheck_sql = positional(
        &queries_user::email_availability_exists_sql(),
        &["email", "user"],
    );
    let taken: Option<(i32,)> = match sqlx::query_as(&recheck_sql)
        .bind(&new_email)
        .bind(actor.id)
        .fetch_optional(pool)
        .await
    {
        Ok(taken) => taken,
        Err(_) => return Denial::ServerError.into_response(),
    };
    if taken.is_some() {
        return json_response(StatusCode::BAD_REQUEST, EMAIL_TAKEN_BODY);
    }
    let old_email = row.email.clone();
    let now = Utc::now();
    let derived = match derive_save(
        Some(&new_email),
        &row.display_name,
        &row.token,
        row.token_updated_at,
        row.is_superuser,
        row.is_staff,
        now,
    ) {
        Ok(derived) => derived,
        Err(denial) => return denial.into_response(),
    };
    // `email_save_sql` carries the changed columns; the `User.save()`
    // side effects (display-name fill, token regen, staff) extend it.
    let save_sql = email_save_statement();
    if let Err(error) = sqlx::query(&save_sql)
        .bind(&derived.email)
        .bind(now)
        .bind(actor.id)
        .bind(&derived.display_name)
        .bind(&derived.token)
        .bind(derived.token_updated_at)
        .bind(derived.is_staff)
        .execute(pool)
        .await
    {
        return write_denial(error).into_response();
    }
    // Cache delete (`:238`, outside the `try` — a failure 500s).
    match cache_del(&state, &key).await {
        Ok(()) => {}
        Err(()) => return Denial::ServerError.into_response(),
    }
    if let Err(denial) = logout(pool, &extension).await {
        return denial.into_response();
    }
    // Two confirmations: new address first, old address second (`:244-246`).
    let new_emit = tasks::email_update_confirmation_emit(&new_email);
    enqueue_emit(pool, new_emit.task_name(), new_emit.args()).await;
    let old_arg = match old_email.as_deref() {
        Some(old) => {
            let old_emit = tasks::email_update_confirmation_emit(old);
            old_emit.args()
        }
        None => vec![Value::Null],
    };
    enqueue_emit(pool, tasks::SEND_EMAIL_UPDATE_CONFIRMATION_TASK, old_arg).await;
    // The 200 re-renders the saved in-memory user (`:249`).
    let saved = UserRow {
        email: Some(derived.email.clone()),
        display_name: derived.display_name.clone(),
        token: derived.token.clone(),
        token_updated_at: derived.token_updated_at,
        is_staff: derived.is_staff,
        is_email_verified: false,
        updated_at: now,
        ..row.clone()
    };
    let rendered = match render_user(pool, &saved, &actor.timezone).await {
        Ok(rendered) => rendered,
        Err(denial) => return denial.into_response(),
    };
    (StatusCode::OK, Json(rendered.render_me())).into_response()
}

/// Email-save statement, positional: [`queries_user::email_save_sql`]
/// plus the `User.save()` side-effect columns. Binds in order:
/// `$1` email, `$2` now, `$3` pk, `$4` display name, `$5` token, `$6`
/// token updated at, `$7` staff.
fn email_save_statement() -> String {
    positional(&queries_user::email_save_sql(), &["email", "now", "pk"]).replace(
        "SET email = $1, is_email_verified = FALSE, updated_at = $2",
        "SET email = $1, is_email_verified = FALSE, updated_at = $2, display_name = $4, token = $5, token_updated_at = $6, is_staff = $7",
    )
}

/// `cache.get`: `None` for a missing key; `Err` for a dead/missing
/// cache (Django raises inside the verify `try` → 400 failed-verify).
async fn cache_get(state: &AppState, key: &str) -> Result<Option<String>, ()> {
    let Some(cache) = Cache::from_state(state) else {
        return Err(());
    };
    cache.get(key).await.map_err(|_| ())
}

/// `cache.delete`: `Err` for a dead/missing cache (outside any `try`
/// → 500).
async fn cache_del(state: &AppState, key: &str) -> Result<(), ()> {
    let Some(cache) = Cache::from_state(state) else {
        return Err(());
    };
    cache.del(key).await.map_err(|_| ())
}

// ---------------------------------------------------------------------------
// Handlers — deactivate (`user/base.py:252-357`)
// ---------------------------------------------------------------------------

/// `make_password` salt (`BasePasswordHasher.salt()`, 128 bits): 22
/// alphanumerics from Django's `RANDOM_STRING_CHARS`.
fn password_salt() -> String {
    use rand::Rng;
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut rng = rand::rng();
    (0..22)
        .map(|_| ALPHABET[rng.random_range(0..ALPHABET.len())] as char)
        .collect()
}

/// `user.set_password(uuid4().hex)` (`user/base.py:343`): PBKDF2-SHA256
/// at the pinned Django 4.2 iteration count (600000 — the
/// `auth_oauth`/`workspace_seed` precedent).
fn encode_random_password() -> String {
    let raw = uuid::Uuid::new_v4().simple().to_string();
    pidash_auth::password::hash_password(&raw, &password_salt(), 600_000)
}

/// Bulk-deactivate statement for one pk chunk: the builder's `IN
/// (:pks)` expands to a positional list. An empty chunk issues no
/// statement (the caller skips it).
fn bulk_deactivate_statement(table: &str, chunk_len: usize) -> String {
    let placeholders: Vec<String> = (1..=chunk_len).map(|n| format!("${n}")).collect();
    let (prefix, suffix) = if table == "project_members" {
        (
            queries_user::project_bulk_deactivate_sql(),
            "project_members.id IN (:pks)",
        )
    } else {
        (
            queries_user::workspace_bulk_deactivate_sql(),
            "workspace_members.id IN (:pks)",
        )
    };
    prefix.replace(
        suffix,
        &format!("{table}.id IN ({})", placeholders.join(", ")),
    )
}

/// Invite-purge statement for a `None` email, positional: the
/// builder with the email predicate rewritten to `IS NULL` (binds
/// `$1` now only).
fn invite_purge_statement() -> String {
    positional(
        &queries_user::deactivate_invite_purge_sql().replace(
            "workspace_member_invites.email = :email",
            "workspace_member_invites.email IS NULL",
        ),
        &["now"],
    )
}

/// Run the bulk deactivation in `batch_size=100` chunks (bug 5: no
/// `updated_at`, no `deleted_at` filter, pk-direct).
async fn bulk_deactivate(pool: &PgPool, table: &str, ids: &[uuid::Uuid]) -> Result<(), Denial> {
    for chunk in ids.chunks(queries_user::BULK_DEACTIVATE_BATCH_SIZE) {
        if chunk.is_empty() {
            continue;
        }
        let sql = bulk_deactivate_statement(table, chunk.len());
        let mut query = sqlx::query(&sql);
        for id in chunk {
            query = query.bind(*id);
        }
        query.execute(pool).await.map_err(|_| Denial::ServerError)?;
    }
    Ok(())
}

/// `HostSettings` over the resolved F-03 URLs (the `auth_session`
/// `host_settings` precedent).
fn host_settings(settings: &pidash_db::config::Settings) -> auth_shapes::HostSettings<'_> {
    let urls = &settings.urls;
    auth_shapes::HostSettings {
        web_url: urls.web_url.as_deref(),
        app_base_url: urls.app_base_url.as_deref(),
        admin_base_url: urls.admin_base_url.as_deref(),
        space_base_url: urls.space_base_url.as_deref(),
        admin_base_path: Some(urls.admin_base_path.as_str()),
        space_base_path: Some(urls.space_base_path.as_str()),
    }
}

/// Deactivate user-save statement, positional:
/// [`queries_user::deactivate_user_save_sql`] plus the `User.save()`
/// side-effect columns. Binds in order: `$1` hash, `$2` ip, `$3` now,
/// `$4` pk, `$5` email, `$6` display name, `$7` token, `$8` token
/// updated at, `$9` staff.
fn deactivate_user_save_statement() -> String {
    positional(
        &queries_user::deactivate_user_save_sql(),
        &["hash", "ip", "now", "pk"],
    )
    .replace(
        "SET password = $1, is_password_autoset = TRUE, is_active = FALSE, last_logout_ip = $2, last_logout_time = $3, updated_at = $3",
        "SET password = $1, is_password_autoset = TRUE, is_active = FALSE, last_logout_ip = $2, last_logout_time = $3, updated_at = $3, email = $5, display_name = $6, token = $7, token_updated_at = $8, is_staff = $9",
    )
}

/// `DELETE /api/users/me/` (`user/base.py:252-357`): the instance-admin
/// 400, the (vacuous) sole-admin scans, bulk deactivations,
/// invite/session deletes, the profile reset, password randomization,
/// the deactivation enqueue, logout, 204.
pub async fn me_deactivate(
    State(state): State<AppState>,
    extension: Option<Extension<SessionHandle>>,
    headers: HeaderMap,
    peer: ConnectInfo<SocketAddr>,
) -> Response {
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let actor = match authed_actor(&state, pool, extension.clone()).await {
        Ok(actor) => actor,
        Err(denial) => return denial.into_response(),
    };
    let row = match fetch_user(pool, actor.id).await {
        Ok(row) => row,
        Err(denial) => return denial.into_response(),
    };
    // Instance-admin guard (`:257-261`, user-only — no instance predicate).
    let guard_sql = positional(
        &queries_user::deactivate_instance_admin_guard_sql(),
        &["user"],
    );
    let guarded: Option<(i32,)> = match sqlx::query_as(&guard_sql)
        .bind(actor.id)
        .fetch_optional(pool)
        .await
    {
        Ok(guarded) => guarded,
        Err(_) => return Denial::ServerError.into_response(),
    };
    if guarded.is_some() {
        return json_response(StatusCode::BAD_REQUEST, DEACTIVATE_INSTANCE_ADMIN_BODY);
    }
    // Sole-admin scans (`:266-306`): the loop evaluates the condition
    // per row even though the vacuous annotations make the 400s
    // unreachable (bug 4).
    let project_sql = positional(&queries_user::project_deactivate_scan_sql(), &["user"]);
    let projects: Vec<DeactivateScanRow> = match sqlx::query_as(&project_sql)
        .bind(actor.id)
        .fetch_all(pool)
        .await
    {
        Ok(projects) => projects,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let mut project_ids = Vec::with_capacity(projects.len());
    for project in &projects {
        if project.other_admin_exists > 0 || project.total_members == 1 {
            project_ids.push(project.id);
        } else {
            return json_response(StatusCode::BAD_REQUEST, DEACTIVATE_SOLE_PROJECT_ADMIN_BODY);
        }
    }
    let workspace_sql = positional(&queries_user::workspace_deactivate_scan_sql(), &["user"]);
    let workspaces: Vec<DeactivateScanRow> = match sqlx::query_as(&workspace_sql)
        .bind(actor.id)
        .fetch_all(pool)
        .await
    {
        Ok(workspaces) => workspaces,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let mut workspace_ids = Vec::with_capacity(workspaces.len());
    for workspace in &workspaces {
        if workspace.other_admin_exists > 0 || workspace.total_members == 1 {
            workspace_ids.push(workspace.id);
        } else {
            return json_response(
                StatusCode::BAD_REQUEST,
                DEACTIVATE_SOLE_WORKSPACE_ADMIN_BODY,
            );
        }
    }
    if let Err(denial) = bulk_deactivate(pool, "project_members", &project_ids).await {
        return denial.into_response();
    }
    if let Err(denial) = bulk_deactivate(pool, "workspace_members", &workspace_ids).await {
        return denial.into_response();
    }
    // One `:now` sample for the run (the `queries_user` contract —
    // Django samples per statement, but the stamps are unobservable
    // through the 204).
    let now = Utc::now();
    // Invite purge (`:313`, soft `deleted_at` stamp — a `None` email
    // renders `IS NULL`, like Django's `filter(email=None)`).
    if let Some(email) = row.email.as_deref() {
        let purge_sql = positional(
            &queries_user::deactivate_invite_purge_sql(),
            &["now", "email"],
        );
        if let Err(error) = sqlx::query(&purge_sql)
            .bind(now)
            .bind(email)
            .execute(pool)
            .await
        {
            return write_denial(error).into_response();
        }
    } else {
        let null_sql = invite_purge_statement();
        if let Err(error) = sqlx::query(&null_sql).bind(now).execute(pool).await {
            return write_denial(error).into_response();
        }
    }
    // Session purge (`:316`, hard delete, UUID-as-text — bug 11).
    let session_purge_sql = positional(&queries_user::deactivate_session_purge_sql(), &["user"]);
    if let Err(error) = sqlx::query(&session_purge_sql)
        .bind(actor.id.to_string())
        .execute(pool)
        .await
    {
        return write_denial(error).into_response();
    }
    // Profile reset (`:319-339`; a miss is the 404).
    let profile = match fetch_profile(pool, actor.id).await {
        Ok(profile) => profile,
        Err(denial) => return denial.into_response(),
    };
    let step: Value =
        serde_json::from_str(queries_user::ONBOARDING_RESET_JSON).expect("static reset json");
    let profile_reset_sql = positional(
        &queries_user::deactivate_profile_reset_sql(),
        &["step_json", "now", "pk"],
    );
    if let Err(error) = sqlx::query(&profile_reset_sql)
        .bind(step)
        .bind(now)
        .bind(profile.id)
        .execute(pool)
        .await
    {
        return write_denial(error).into_response();
    }
    // User save (`:342-349`): password randomize + autoset, inactive,
    // logout stamps, plus the `User.save()` side effects.
    let forwarded = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok());
    let remote = peer.0.ip().to_string();
    let ip = crate::license::handlers_auth_forms::client_ip(forwarded, Some(remote.as_str()));
    let derived = match derive_save(
        row.email.as_deref(),
        &row.display_name,
        &row.token,
        row.token_updated_at,
        row.is_superuser,
        row.is_staff,
        now,
    ) {
        Ok(derived) => derived,
        Err(denial) => return denial.into_response(),
    };
    let password_hash = encode_random_password();
    // The builder lists the semantically-changed columns; the
    // `User.save()` side effects extend it (`$5`-`$9`).
    let user_save_sql = deactivate_user_save_statement();
    if let Err(error) = sqlx::query(&user_save_sql)
        .bind(&password_hash)
        .bind(ip.as_deref())
        .bind(now)
        .bind(actor.id)
        .bind(&derived.email)
        .bind(&derived.display_name)
        .bind(&derived.token)
        .bind(derived.token_updated_at)
        .bind(derived.is_staff)
        .execute(pool)
        .await
    {
        return write_denial(error).into_response();
    }
    // Deactivation email (`:352`): `base_host(app)` + user id. No
    // configured origin is the `ImproperlyConfigured` 500.
    let settings = state.settings();
    let origin_set = settings
        .urls
        .web_url
        .as_deref()
        .is_some_and(|u| !u.is_empty())
        || settings
            .urls
            .app_base_url
            .as_deref()
            .is_some_and(|u| !u.is_empty());
    if !origin_set {
        return Denial::ServerError.into_response();
    }
    let site = auth_shapes::base_host(&host_settings(settings), false, false, true);
    let emit = tasks::user_deactivation_email_emit(&site, &actor.id.to_string());
    enqueue_emit(pool, emit.task_name(), emit.args()).await;
    if let Err(denial) = logout(pool, &extension).await {
        return denial.into_response();
    }
    StatusCode::NO_CONTENT.into_response()
}

// ---------------------------------------------------------------------------
// Handlers — session / onboard / tour (`user/base.py:359-390`)
// ---------------------------------------------------------------------------

/// `GET /api/users/session/` (`user/base.py:359-371`, `AllowAny`): the
/// authenticated branch re-reads the user and answers
/// `{"is_authenticated": true, "user": <me>}`; the anonymous branch
/// answers `{"is_authenticated": false}` and issues no SQL.
pub async fn session_get(
    State(state): State<AppState>,
    extension: Option<Extension<SessionHandle>>,
) -> Response {
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let actor = match crate::license::resolve_actor(
        pool,
        state.settings().secret_key.as_bytes(),
        extension,
    )
    .await
    {
        Ok(actor) => actor,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let mut body = Map::new();
    let Some(actor) = actor else {
        body.insert("is_authenticated".to_owned(), Value::Bool(false));
        return (StatusCode::OK, Json(Value::Object(body))).into_response();
    };
    let row = match fetch_user(pool, actor.id).await {
        Ok(row) => row,
        Err(denial) => return denial.into_response(),
    };
    let rendered = match render_user(pool, &row, &actor.timezone).await {
        Ok(rendered) => rendered,
        Err(denial) => return denial.into_response(),
    };
    body.insert("is_authenticated".to_owned(), Value::Bool(true));
    body.insert("user".to_owned(), rendered.render_me());
    (StatusCode::OK, Json(Value::Object(body))).into_response()
}

/// Coerce one raw onboard/tour flag value the way Django's
/// `BooleanField.get_prep_value` does (DB-free, unit-tested): it
/// runs `to_python`, so `True`/`False` (and `1`/`0`/`1.0`/`0.0`,
/// which compare equal) bind as bools, the `"t"`/`"True"`/`"1"` and
/// `"f"`/`"False"`/`"0"` spellings coerce, `null` binds NULL (the
/// NOT NULL column then 400s through `IntegrityError`), and
/// everything else raises Django `ValidationError` — the whole-body
/// 400, answered before any SQL runs.
fn profile_flag_value(value: &Value) -> Result<Option<bool>, Denial> {
    match value {
        Value::Bool(flag) => Ok(Some(*flag)),
        Value::Number(number) => {
            if number.as_i64() == Some(1) || number.as_f64() == Some(1.0) {
                Ok(Some(true))
            } else if number.as_i64() == Some(0) || number.as_f64() == Some(0.0) {
                Ok(Some(false))
            } else {
                Err(Denial::InvalidDetail)
            }
        }
        Value::String(text) => match text.as_str() {
            "t" | "True" | "1" => Ok(Some(true)),
            "f" | "False" | "0" => Ok(Some(false)),
            _ => Err(Denial::InvalidDetail),
        },
        Value::Null => Ok(None),
        Value::Array(_) | Value::Object(_) => Err(Denial::InvalidDetail),
    }
}

/// Write one raw onboard/tour flag value (bug 6: no serializer
/// validation — whatever JSON the client sent — but the column's
/// `to_python` still coerces or rejects before the save).
async fn write_profile_flag(
    pool: &PgPool,
    profile_id: uuid::Uuid,
    sql: &str,
    value: &Value,
    now: DateTime<Utc>,
) -> Result<(), Denial> {
    let flag = profile_flag_value(value)?;
    sqlx::query(sql)
        .bind(flag)
        .bind(now)
        .bind(profile_id)
        .execute(pool)
        .await
        .map(|_| ())
        .map_err(write_denial)
}

/// Shared onboard/tour PATCH (`user/base.py:373-389`):
/// `Profile.objects.get` (404 on miss) + single-flag
/// `save(update_fields=[flag, updated_at])` → 200
/// `{"message": "Updated successfully"}`. A non-object body is the
/// `.get` `AttributeError` 500.
async fn profile_flag_patch(
    state: &AppState,
    extension: Option<Extension<SessionHandle>>,
    req: Request,
    flag: &str,
) -> Response {
    let pool = match pool_of(state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let actor = match authed_actor(state, pool, extension).await {
        Ok(actor) => actor,
        Err(denial) => return denial.into_response(),
    };
    let body = match read_json_body(req).await {
        Ok(body) => body,
        Err(denial) => return denial.into_response(),
    };
    let Value::Object(map) = &body else {
        return Denial::ServerError.into_response();
    };
    // `request.data.get(flag, False)`: absent keys default to `False`.
    let value = map.get(flag).cloned().unwrap_or(Value::Bool(false));
    let profile = match fetch_profile(pool, actor.id).await {
        Ok(profile) => profile,
        Err(denial) => return denial.into_response(),
    };
    let builder = if flag == "is_onboarded" {
        queries_user::onboard_patch_sql()
    } else {
        queries_user::tour_patch_sql()
    };
    let sql = positional(&builder, &["value", "now", "pk"]);
    if let Err(denial) = write_profile_flag(pool, profile.id, &sql, &value, Utc::now()).await {
        return denial.into_response();
    }
    json_response(StatusCode::OK, UPDATED_BODY)
}

/// `PATCH /api/users/me/onboard/` (`user/base.py:373-381`).
pub async fn onboard_patch(
    State(state): State<AppState>,
    extension: Option<Extension<SessionHandle>>,
    req: Request,
) -> Response {
    profile_flag_patch(&state, extension, req, "is_onboarded").await
}

/// `PATCH /api/users/me/tour-completed/` (`user/base.py:384-389`).
pub async fn tour_patch(
    State(state): State<AppState>,
    extension: Option<Extension<SessionHandle>>,
    req: Request,
) -> Response {
    profile_flag_patch(&state, extension, req, "is_tour_completed").await
}

// ---------------------------------------------------------------------------
// Routes (`app/urls/user.py:24-64`, this issue's paths)
// ---------------------------------------------------------------------------

/// A user-identity path: the owned methods serve from Rust, everything
/// else falls through to Django (its 405s and DRF metadata live
/// there). OPTIONS proxies too: DRF answers metadata where axum
/// would 405. (The `app_scheduler` precedent; PIDASHCONV-624 merges
/// this into the domain `routes()` + overlay wiring.)
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

/// This issue's routes (U01-U05, U09-U11). Sibling D-24 handler files
/// expose their own `routes()`; PIDASHCONV-624 merges them (merges
/// keep both sides).
pub fn routes() -> Router<AppState> {
    use axum::routing::{get, patch, post};
    Router::new()
        .route(
            "/api/users/me/",
            owned(
                get(me_retrieve)
                    .patch(me_partial_update)
                    .delete(me_deactivate),
                &["POST", "PUT", "OPTIONS"],
            ),
        )
        .route(
            "/api/users/session/",
            owned(
                get(session_get),
                &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"],
            ),
        )
        .route(
            "/api/users/me/settings/",
            owned(
                get(me_settings),
                &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"],
            ),
        )
        .route(
            "/api/users/me/email/generate-code/",
            owned(
                post(email_generate_code),
                &["GET", "PUT", "PATCH", "DELETE", "OPTIONS"],
            ),
        )
        .route(
            "/api/users/me/email/",
            owned(
                patch(email_update),
                &["GET", "POST", "PUT", "DELETE", "OPTIONS"],
            ),
        )
        .route(
            "/api/users/me/instance-admin/",
            owned(
                get(me_instance_admin),
                &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"],
            ),
        )
        .route(
            "/api/users/me/onboard/",
            owned(
                patch(onboard_patch),
                &["GET", "POST", "PUT", "DELETE", "OPTIONS"],
            ),
        )
        .route(
            "/api/users/me/tour-completed/",
            owned(
                patch(tour_patch),
                &["GET", "POST", "PUT", "DELETE", "OPTIONS"],
            ),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const FIXTURE_ROUTES: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/app_workspace/handlers/routes.golden.json"
    );

    fn fixture_json(path: &str) -> Value {
        let text = std::fs::read_to_string(path).expect("fixture readable");
        serde_json::from_str(&text).expect("fixture JSON")
    }

    fn route_entries() -> Vec<String> {
        fixture_json(FIXTURE_ROUTES)["routes"]
            .as_array()
            .expect("routes array")
            .iter()
            .map(|entry| entry.as_str().expect("route string").to_owned())
            .collect()
    }

    fn error_entry(source_sub: &str) -> (String, i64) {
        let errors = fixture_json(FIXTURE_ROUTES)["errors"]
            .as_array()
            .expect("errors array")
            .clone();
        let entry = errors
            .iter()
            .find(|entry| {
                entry["source"]
                    .as_str()
                    .is_some_and(|source| source.contains(source_sub))
            })
            .unwrap_or_else(|| panic!("error entry for {source_sub}"));
        (
            serde_json::to_string(&entry["body"]).expect("body JSON"),
            entry["status"].as_i64().expect("status"),
        )
    }

    // -- F-W24-15 route table (U01-U05, U09-U11) ------------------------------

    #[test]
    fn route_table_covers_this_issues_paths() {
        let routes = route_entries().join("\n");
        for expected in [
            "U01 users/me/ -> UserEndpoint:retrieve+partial_update+deactivate GET+PATCH+DELETE",
            "U02 GET users/session/ -> UserSessionEndpoint:get (AllowAny",
            "U03 GET users/me/settings/ -> UserEndpoint:retrieve_user_settings",
            "U04 POST users/me/email/generate-code/ -> UserEndpoint:generate_email_verification_code (throttle 3/hour",
            "U05 PATCH users/me/email/ -> UserEndpoint:update_email",
            "U09 GET users/me/instance-admin/ -> UserEndpoint:retrieve_instance_admin",
            "U10 PATCH users/me/onboard/ -> UpdateUserOnBoardedEndpoint:patch",
            "U11 PATCH users/me/tour-completed/ -> UpdateUserTourCompletedEndpoint:patch",
        ] {
            assert!(
                routes.contains(expected),
                "route table pins {expected}"
            );
        }
    }

    #[test]
    fn routes_build_without_panicking() {
        // axum validates path templates at construction; a typo panics here.
        let _ = routes();
    }

    // -- F-W24-15 error bodies ------------------------------------------------

    #[test]
    fn email_validation_bodies_match_fixture() {
        for (source, body, status) in [
            ("views/user/base.py:106-110", EMAIL_REQUIRED_BODY, 400),
            ("views/user/base.py:113-119", EMAIL_FORMAT_BODY, 400),
            ("views/user/base.py:122-126", EMAIL_SAME_BODY, 400),
            ("views/user/base.py:128-133", EMAIL_TAKEN_BODY, 400),
        ] {
            let (expected, expected_status) = error_entry(source);
            assert_eq!(body, expected, "{source} body");
            assert_eq!(status, expected_status, "{source} status");
        }
    }

    #[test]
    fn code_bodies_match_fixture() {
        for (source, body, status) in [
            ("views/user/base.py:191-195", CODE_REQUIRED_BODY, 400),
            ("views/user/base.py:202-207", CODE_EXPIRED_BODY, 400),
            ("views/user/base.py:212-216", CODE_INVALID_BODY, 400),
            ("views/user/base.py:218-222", CODE_FAILED_BODY, 400),
            ("views/user/base.py:170-174", GENERATE_FAILED_BODY, 400),
        ] {
            let (expected, expected_status) = error_entry(source);
            assert_eq!(body, expected, "{source} body");
            assert_eq!(status, expected_status, "{source} status");
        }
    }

    #[test]
    fn deactivate_guard_bodies_match_fixture() {
        for (source, body, status) in [
            (
                "views/user/base.py:257-261",
                DEACTIVATE_INSTANCE_ADMIN_BODY,
                400,
            ),
            (
                "views/user/base.py:282-285",
                DEACTIVATE_SOLE_PROJECT_ADMIN_BODY,
                400,
            ),
            (
                "views/user/base.py:302-306",
                DEACTIVATE_SOLE_WORKSPACE_ADMIN_BODY,
                400,
            ),
        ] {
            let (expected, expected_status) = error_entry(source);
            assert_eq!(body, expected, "{source} body");
            assert_eq!(status, expected_status, "{source} status");
        }
    }

    // -- Shared bodies + envelopes --------------------------------------------

    #[test]
    fn shared_bodies_match_reviewed_precedents() {
        assert_eq!(
            OBJECT_NOT_FOUND_BODY,
            crate::app_pages::OBJECT_NOT_FOUND_BODY
        );
        assert_eq!(SERVER_ERROR_BODY, crate::app_pages::SERVER_ERROR_BODY);
        assert_eq!(INVALID_PAYLOAD_BODY, crate::app_pages::INVALID_PAYLOAD_BODY);
    }

    #[test]
    fn invalid_detail_body_is_the_django_validation_error_branch() {
        assert_eq!(
            INVALID_DETAIL_BODY,
            r#"{"error":"Please provide valid detail"}"#
        );
    }

    /// The `BadDetail` envelope key is lowercase `detail`: DRF 3.15.2
    /// `exception_handler` renders `{'detail': ...}` (ord-verified in
    /// the installed `rest_framework/views.py`; PIDASHCONV-724), the
    /// merged fixtures pin lowercase (`routes.golden.json`: 2
    /// lowercase, 0 uppercase), and the live-Django contract tests pin
    /// the lowercase 401 (`test_user.py:175`). Tied to
    /// `gates::ANON_BODY` so the two cannot drift apart again.
    #[test]
    fn bad_detail_key_is_the_drf_lowercase_detail() {
        let mine = detail_envelope("probe".to_owned());
        assert_eq!(mine, json!({"detail": "probe"}));
        let gate: Value = serde_json::from_str(gates::ANON_BODY).expect("gate body JSON");
        let gate_key = gate
            .as_object()
            .expect("gate object")
            .keys()
            .next()
            .expect("one key");
        let mine_key = mine
            .as_object()
            .expect("mine object")
            .keys()
            .next()
            .expect("one key");
        assert_eq!(mine_key, gate_key);
        assert_eq!(mine_key.as_bytes()[0], 100);
    }

    #[test]
    fn success_bodies_are_byte_exact() {
        assert_eq!(
            CODE_SENT_BODY,
            r#"{"message":"Verification code sent to email"}"#
        );
        assert_eq!(UPDATED_BODY, r#"{"message":"Updated successfully"}"#);
    }

    // -- Consumed F-W24-13: throttle wiring + headers --------------------------

    #[test]
    fn throttle_wired_on_generate_code_only() {
        let wiring = gates::throttle_for("POST", "users/me/email/generate-code/")
            .expect("generate-code throttle");
        assert_eq!(wiring.spec, gates::EMAIL_VERIFICATION_THROTTLE);
        assert_eq!(wiring.spec.requests, 3);
        assert_eq!(wiring.spec.window_secs, 3600);
        assert!(matches!(wiring.ident, gates::ThrottleIdent::UserPkOrIp));
        for (method, path) in [
            ("PATCH", "users/me/email/"),
            ("GET", "users/me/"),
            ("PATCH", "users/me/"),
            ("DELETE", "users/me/"),
            ("GET", "users/me/settings/"),
            ("GET", "users/me/instance-admin/"),
            ("PATCH", "users/me/onboard/"),
            ("PATCH", "users/me/tour-completed/"),
            ("GET", "users/session/"),
        ] {
            assert!(
                gates::throttle_for(method, path).is_none(),
                "{method} {path} carries no throttle"
            );
        }
    }

    #[test]
    fn decorator_headers_cover_me_and_settings_gets() {
        for path in ["users/me/", "users/me/settings/"] {
            let headers = gates::headers_for("GET", path);
            assert_eq!(headers.cache_control, Some(gates::CACHE_CONTROL_PRIVATE_12));
            assert_eq!(headers.vary, Some(gates::VARY_COOKIE));
            assert!(!headers.gzip);
        }
        assert_eq!(gates::CACHE_CONTROL_PRIVATE_12, "private, max-age=12");
        assert_eq!(gates::VARY_COOKIE, "Cookie");
        for (method, path) in [
            ("PATCH", "users/me/"),
            ("DELETE", "users/me/"),
            ("POST", "users/me/email/generate-code/"),
            ("PATCH", "users/me/email/"),
            ("GET", "users/me/instance-admin/"),
            ("GET", "users/session/"),
            ("PATCH", "users/me/onboard/"),
            ("PATCH", "users/me/tour-completed/"),
        ] {
            let headers = gates::headers_for(method, path);
            assert_eq!(headers.cache_control, None, "{method} {path}");
            assert_eq!(headers.vary, None, "{method} {path}");
        }
    }

    // -- Consumed F-W24-04/12/14: builders this module binds --------------------

    #[test]
    fn email_cache_contract_matches_queries_layer() {
        assert_eq!(
            queries_user::email_update_cache_key("9f6c", "a@x.io"),
            "magic_email_update_9f6c_a@x.io"
        );
        assert_eq!(queries_user::EMAIL_UPDATE_CODE_TTL_SECS, 600);
        assert_eq!(queries_user::EMAIL_UPDATE_CODE_MIN, 100000);
        assert_eq!(queries_user::EMAIL_UPDATE_CODE_MAX, 999999);
        assert_eq!(
            queries_user::email_update_cache_value("123456"),
            r#"{"token": "123456"}"#
        );
    }

    #[test]
    fn task_names_and_arg_orders_match_tasks_layer() {
        assert_eq!(
            tasks::SEND_EMAIL_UPDATE_MAGIC_CODE_TASK,
            "pi_dash.bgtasks.user_email_update_task.send_email_update_magic_code"
        );
        assert_eq!(
            tasks::SEND_EMAIL_UPDATE_CONFIRMATION_TASK,
            "pi_dash.bgtasks.user_email_update_task.send_email_update_confirmation"
        );
        assert_eq!(
            tasks::USER_DEACTIVATION_EMAIL_TASK,
            "pi_dash.bgtasks.user_deactivation_email_task.user_deactivation_email"
        );
        assert_eq!(
            tasks::SEND_EMAIL_UPDATE_MAGIC_CODE_ARG_ORDER,
            &["email", "token"]
        );
        assert_eq!(tasks::SEND_EMAIL_UPDATE_CONFIRMATION_ARG_ORDER, &["email"]);
        assert_eq!(
            tasks::USER_DEACTIVATION_EMAIL_ARG_ORDER,
            &["current_site", "user_id"]
        );
        let magic = tasks::email_update_magic_code_emit("n@x.io", "654321");
        assert_eq!(magic.args(), vec![json!("n@x.io"), json!("654321")]);
        let confirm = tasks::email_update_confirmation_emit("n@x.io");
        assert_eq!(confirm.args(), vec![json!("n@x.io")]);
        let goodbye = tasks::user_deactivation_email_emit("https://app.test", "uid-1");
        assert_eq!(
            goodbye.args(),
            vec![json!("https://app.test"), json!("uid-1")]
        );
    }

    #[test]
    fn deactivate_constants_match_queries_layer() {
        assert_eq!(queries_user::BULK_DEACTIVATE_BATCH_SIZE, 100);
        let step: Value = serde_json::from_str(queries_user::ONBOARDING_RESET_JSON).expect("json");
        assert_eq!(
            step,
            json!({
                "workspace_join": false,
                "profile_complete": false,
                "workspace_create": false,
                "workspace_invite": false,
            })
        );
        assert_eq!(
            queries_user::ONBOARD_UPDATE_FIELDS,
            &["is_onboarded", "updated_at"]
        );
        assert_eq!(
            queries_user::TOUR_UPDATE_FIELDS,
            &["is_tour_completed", "updated_at"]
        );
    }

    // -- Statements ------------------------------------------------------------

    #[test]
    fn positional_numbers_params_in_order() {
        assert_eq!(positional(":a :b :a", &["a", "b"]), "$1 $2 $1");
        assert_eq!(positional("x:y :now2", &["now"]), "x:y :now2");
    }

    #[test]
    fn nullable_predicate_statements_render_is_null() {
        let count_some = invite_count_statement(false);
        assert!(count_some.contains("workspace_member_invites.email = $1"));
        // The email arm is `= $1`, not `IS NULL` (the base statement
        // legitimately keeps its `deleted_at IS NULL` soft-delete
        // scope from `queries_user::invite_count_sql`).
        assert!(!count_some.contains("workspace_member_invites.email IS NULL"));
        let count_none = invite_count_statement(true);
        assert!(count_none.contains("workspace_member_invites.email IS NULL"));
        assert!(!count_none.contains("$1"));

        let exists_some = instance_admin_exists_statement(false);
        assert!(exists_some.contains("instance_admins.instance_id = $1"));
        assert!(exists_some.contains("instance_admins.user_id = $2"));
        let exists_none = instance_admin_exists_statement(true);
        assert!(exists_none.contains("instance_admins.instance_id IS NULL"));
        assert!(exists_none.contains("instance_admins.user_id = $1"));
        assert!(!exists_none.contains("$2"));

        let purge_none = invite_purge_statement();
        assert!(purge_none.contains("workspace_member_invites.email IS NULL"));
        assert!(purge_none.contains("deleted_at = $1"));
        assert!(!purge_none.contains("$2"));
    }

    #[test]
    fn extended_save_statements_bind_save_side_effects() {
        let email = email_save_statement();
        assert!(email.contains("display_name = $4"));
        assert!(email.contains("token = $5"));
        assert!(email.contains("token_updated_at = $6"));
        assert!(email.contains("is_staff = $7"));
        assert!(email.contains("WHERE id = $3"));

        let deactivate = deactivate_user_save_statement();
        assert!(deactivate.contains("email = $5"));
        assert!(deactivate.contains("display_name = $6"));
        assert!(deactivate.contains("token = $7"));
        assert!(deactivate.contains("token_updated_at = $8"));
        assert!(deactivate.contains("is_staff = $9"));
        assert!(deactivate.contains("WHERE id = $4"));
    }

    #[test]
    fn bulk_statement_expands_one_placeholder_per_id() {
        assert_eq!(
            bulk_deactivate_statement("project_members", 2),
            "UPDATE project_members SET is_active = FALSE WHERE project_members.id IN ($1, $2)"
        );
        assert_eq!(
            bulk_deactivate_statement("workspace_members", 1),
            "UPDATE workspace_members SET is_active = FALSE WHERE workspace_members.id IN ($1)"
        );
        let wide = bulk_deactivate_statement("project_members", 100);
        assert!(wide.contains("$100"));
        assert!(!wide.contains("$101"));
    }

    // -- Email + token helpers --------------------------------------------------

    #[test]
    fn normalize_email_strips_and_lowercases() {
        assert_eq!(normalize_email("  A@X.IO\t"), "a@x.io");
        assert_eq!(normalize_email("\u{1c}a@x.io\u{1f}"), "a@x.io");
        assert_eq!(normalize_email(""), "");
    }

    #[test]
    fn email_body_string_rejects_non_strings() {
        let body = json!({"email": "a@x.io", "code": 123});
        assert_eq!(email_body_string(&body, "email").unwrap(), "a@x.io");
        assert!(email_body_string(&body, "code").is_err());
        assert_eq!(email_body_string(&body, "missing").unwrap(), "");
        assert!(email_body_string(&json!([1, 2]), "email").is_err());
        assert!(email_body_string(&json!(null), "email").is_err());
    }

    #[test]
    fn py_token_str_renders_python_str() {
        assert_eq!(py_token_str(&json!("123456")).as_deref(), Some("123456"));
        assert_eq!(py_token_str(&json!(123456)).as_deref(), Some("123456"));
        assert_eq!(py_token_str(&json!(true)).as_deref(), Some("True"));
        assert_eq!(py_token_str(&json!(false)).as_deref(), Some("False"));
        assert_eq!(py_token_str(&json!(null)).as_deref(), Some("None"));
        assert_eq!(py_token_str(&json!([1])), None);
        assert_eq!(py_token_str(&json!({"t": 1})), None);
    }

    #[test]
    fn verify_cached_code_edges_match_python() {
        assert_eq!(verify_cached_code(None, "1"), VerifyOutcome::Expired);
        assert_eq!(verify_cached_code(Some(""), "1"), VerifyOutcome::Expired);
        assert_eq!(
            verify_cached_code(Some(r#"{"token": "123456"}"#), "123456"),
            VerifyOutcome::Match
        );
        assert_eq!(
            verify_cached_code(Some(r#"{"token": "123456"}"#), "654321"),
            VerifyOutcome::Mismatch
        );
        assert_eq!(
            verify_cached_code(Some("not json"), "1"),
            VerifyOutcome::Failed
        );
        assert_eq!(verify_cached_code(Some("[1]"), "1"), VerifyOutcome::Failed);
        assert_eq!(
            verify_cached_code(Some(r#"{"token": [1]}"#), "1"),
            VerifyOutcome::Failed
        );
        // A missing key reads as None: only the literal "None" matches.
        assert_eq!(verify_cached_code(Some("{}"), "None"), VerifyOutcome::Match);
        assert_eq!(
            verify_cached_code(Some("{}"), "123456"),
            VerifyOutcome::Mismatch
        );
        assert_eq!(
            verify_cached_code(Some(r#"{"token": null}"#), "None"),
            VerifyOutcome::Match
        );
    }

    #[test]
    fn profile_flag_value_follows_boolean_to_python() {
        for value in [
            json!(true),
            json!(1),
            json!(1.0),
            json!("t"),
            json!("True"),
            json!("1"),
        ] {
            assert_eq!(profile_flag_value(&value).unwrap(), Some(true), "{value}");
        }
        for value in [
            json!(false),
            json!(0),
            json!(0.0),
            json!("f"),
            json!("False"),
            json!("0"),
        ] {
            assert_eq!(profile_flag_value(&value).unwrap(), Some(false), "{value}");
        }
        assert_eq!(profile_flag_value(&json!(null)).unwrap(), None);
        for value in [
            json!("true"),
            json!("false"),
            json!("yes"),
            json!("on"),
            json!(""),
            json!(2),
            json!(-1),
            json!(1.5),
            json!([1]),
            json!({"a": 1}),
        ] {
            assert!(
                matches!(profile_flag_value(&value), Err(Denial::InvalidDetail)),
                "{value}"
            );
        }
    }

    #[test]
    fn email_format_check_matches_django_validator() {
        use crate::v1_projects::handlers_members::is_valid_email;
        assert!(is_valid_email("a@x.io"));
        assert!(is_valid_email("first.last+tag@sub.example.co"));
        assert!(!is_valid_email("no-at-sign"));
        assert!(!is_valid_email("a@"));
        assert!(!is_valid_email("@x.io"));
        assert!(!is_valid_email(""));
    }

    #[test]
    fn drawn_codes_are_six_digits() {
        for _ in 0..50 {
            let code = draw_email_code();
            assert_eq!(code.len(), 6);
            let value: u32 = code.parse().expect("digits");
            assert!((100000..=999999).contains(&value));
        }
    }

    // -- DRF field pipelines -----------------------------------------------------

    #[test]
    fn char_field_pipeline_matches_drf() {
        // Null.
        assert_eq!(
            char_field(&json!(null), false, true, Some(5), INVALID_STRING_MESSAGE).unwrap(),
            None
        );
        assert_eq!(
            char_field(&json!(null), false, false, Some(5), INVALID_STRING_MESSAGE).unwrap_err(),
            vec![NULL_MESSAGE.to_owned()]
        );
        // Blank (after trim) fails before validators run.
        assert_eq!(
            char_field(&json!("   "), false, false, None, INVALID_STRING_MESSAGE).unwrap_err(),
            vec![BLANK_MESSAGE.to_owned()]
        );
        assert_eq!(
            char_field(&json!("   "), true, false, None, INVALID_STRING_MESSAGE).unwrap(),
            Some(String::new())
        );
        // Numbers coerce; bools/composites fail.
        assert_eq!(
            char_field(&json!(42), false, false, None, INVALID_STRING_MESSAGE).unwrap(),
            Some("42".to_owned())
        );
        assert_eq!(
            char_field(&json!(true), false, false, None, INVALID_STRING_MESSAGE).unwrap_err(),
            vec![INVALID_STRING_MESSAGE.to_owned()]
        );
        assert_eq!(
            char_field(&json!([1]), false, false, None, "custom-invalid").unwrap_err(),
            vec!["custom-invalid".to_owned()]
        );
        // Max length counts chars; NUL accumulates after it.
        assert_eq!(
            char_field(
                &json!("abcdef"),
                false,
                false,
                Some(5),
                INVALID_STRING_MESSAGE
            )
            .unwrap_err(),
            vec![max_length_message(5)]
        );
        assert_eq!(
            char_field(
                &json!("ab\0cdef"),
                false,
                false,
                Some(5),
                INVALID_STRING_MESSAGE
            )
            .unwrap_err(),
            vec![max_length_message(5), NULL_CHARACTERS_MESSAGE.to_owned()]
        );
        // Trim applies.
        assert_eq!(
            char_field(&json!("  hi  "), false, false, None, INVALID_STRING_MESSAGE).unwrap(),
            Some("hi".to_owned())
        );
    }

    #[test]
    fn bool_field_matches_drf_truth_tables() {
        for value in [
            json!(true),
            json!(1),
            json!(1.0),
            json!("True"),
            json!("ON"),
            json!("1"),
        ] {
            assert!(bool_field(&value).unwrap(), "{value}");
        }
        for value in [
            json!(false),
            json!(0),
            json!(0.0),
            json!("no"),
            json!("F"),
            json!("0"),
        ] {
            assert!(!bool_field(&value).unwrap(), "{value}");
        }
        for value in [
            json!("x"),
            json!(2),
            json!(1.5),
            json!([1]),
            json!({"a": 1}),
        ] {
            assert_eq!(
                bool_field(&value).unwrap_err(),
                vec![INVALID_BOOLEAN_MESSAGE.to_owned()],
                "{value}"
            );
        }
        assert_eq!(
            bool_field(&json!(null)).unwrap_err(),
            vec![NULL_MESSAGE.to_owned()]
        );
        // No strip: padded strings fail.
        assert!(bool_field(&json!(" true ")).is_err());
    }

    #[test]
    fn timezone_choice_accepts_exact_members_only() {
        assert_eq!(choice_timezone(&json!("UTC")).unwrap(), "UTC".to_owned());
        assert_eq!(
            choice_timezone(&json!("America/New_York")).unwrap(),
            "America/New_York".to_owned()
        );
        assert_eq!(
            choice_timezone(&json!("UTC ")).unwrap_err(),
            vec!["\"UTC \" is not a valid choice.".to_owned()]
        );
        assert_eq!(
            choice_timezone(&json!("utc")).unwrap_err(),
            vec!["\"utc\" is not a valid choice.".to_owned()]
        );
        assert_eq!(
            choice_timezone(&json!(5)).unwrap_err(),
            vec!["\"5\" is not a valid choice.".to_owned()]
        );
        assert_eq!(
            choice_timezone(&json!(true)).unwrap_err(),
            vec!["\"True\" is not a valid choice.".to_owned()]
        );
        assert_eq!(
            choice_timezone(&json!(null)).unwrap_err(),
            vec![NULL_MESSAGE.to_owned()]
        );
    }

    #[test]
    fn pytz_list_is_the_pinned_2024_1_transcription() {
        assert_eq!(PYTZ_2024_1_ZONES.len(), 433);
        for zone in [
            "UTC",
            "Asia/Kolkata",
            "America/New_York",
            "Europe/Berlin",
            "Pacific/Auckland",
        ] {
            assert!(PYTZ_2024_1_ZONES.contains(&zone), "{zone} listed");
        }
        // pytz 2024.1 membership (differs from the `v1_projects`
        // transcription by one zone each way).
        assert!(PYTZ_2024_1_ZONES.contains(&"Asia/Choibalsan"));
        assert!(!PYTZ_2024_1_ZONES.contains(&"America/Coyhaique"));
    }

    #[test]
    fn name_guards_reject_urls() {
        assert!(ser_user::validate_first_name("Ada").is_ok());
        assert_eq!(
            ser_user::validate_first_name("hi example.com")
                .unwrap_err()
                .to_string(),
            "First name cannot contain a URL."
        );
        assert_eq!(
            ser_user::validate_last_name("www.x.io")
                .unwrap_err()
                .to_string(),
            "Last name cannot contain a URL."
        );
    }

    #[test]
    fn cover_image_url_check_matches_django_validator() {
        assert!(ser_extras::validate_user_link_url("https://example.com/x").is_ok());
        assert!(ser_extras::validate_user_link_url("not a url").is_err());
    }

    #[test]
    fn non_object_patch_body_is_the_drf_mapping_400() {
        for (value, datatype) in [
            (json!(null), "NoneType"),
            (json!(true), "bool"),
            (json!(7), "int"),
            (json!(1.5), "float"),
            (json!("x"), "str"),
            (json!([1]), "list"),
        ] {
            assert_eq!(
                non_mapping_body(&value),
                json!({"non_field_errors": [format!("Invalid data. Expected a dictionary, but got {datatype}.")]})
            );
        }
    }

    #[test]
    fn asset_pk_input_branches_match_django_prep() {
        // Null clears.
        assert_eq!(asset_pk_input(&json!(null)).unwrap(), None);
        // Bools fail incorrect_type (DRF raises TypeError first).
        assert!(matches!(
            asset_pk_input(&json!(true)),
            Err(Ok(message)) if message == "Incorrect type. Expected pk value, received bool."
        ));
        // Zero binds the nil UUID (Django queries it; the miss is a
        // field error, not a 500).
        assert_eq!(asset_pk_input(&json!(0)).unwrap(), Some(uuid::Uuid::nil()));
        assert_eq!(
            asset_pk_input(&json!(5)).unwrap(),
            Some(uuid::Uuid::from_u128(5))
        );
        let id = uuid::Uuid::new_v4();
        assert_eq!(asset_pk_input(&json!(id.to_string())).unwrap(), Some(id));
        // Unparseable strings, negative ints, floats, composites, and
        // >u64 ints raise Django ValidationError (whole-body 400).
        let huge: Value = serde_json::from_str("340282366920938463463374607431768211457")
            .expect("huge int parses");
        for value in [
            json!("not-a-uuid"),
            json!(-5),
            json!(1.5),
            json!(2.0),
            json!({"a": 1}),
            json!([1]),
            huge,
        ] {
            assert!(
                matches!(asset_pk_input(&value), Err(Err(PkOutcome::WholeBody))),
                "{value}"
            );
        }
    }

    // -- Datetimes -----------------------------------------------------------------

    #[test]
    fn drf_datetime_parses_rfc3339_and_naive_forms() {
        let tz: chrono_tz::Tz = "America/New_York".parse().unwrap();
        // Aware inputs convert to the same instant.
        let utc =
            parse_drf_datetime(&json!("2024-01-15T10:00:00Z"), &tz, "America/New_York").unwrap();
        assert_eq!(utc.to_rfc3339(), "2024-01-15T10:00:00+00:00");
        let off = parse_drf_datetime(&json!("2024-01-15T12:00:00+02:00"), &tz, "America/New_York")
            .unwrap();
        assert_eq!(off.to_rfc3339(), "2024-01-15T10:00:00+00:00");
        // Naive inputs attach the user zone (EST, UTC-5 in January).
        let naive =
            parse_drf_datetime(&json!("2024-01-15T10:00:00"), &tz, "America/New_York").unwrap();
        assert_eq!(naive.to_rfc3339(), "2024-01-15T15:00:00+00:00");
        let space =
            parse_drf_datetime(&json!("2024-01-15 10:00:00"), &tz, "America/New_York").unwrap();
        assert_eq!(space.to_rfc3339(), "2024-01-15T15:00:00+00:00");
        // Date-only means midnight.
        let date = parse_drf_datetime(&json!("2024-01-15"), &tz, "America/New_York").unwrap();
        assert_eq!(date.to_rfc3339(), "2024-01-15T05:00:00+00:00");
    }

    #[test]
    fn drf_datetime_rejects_garbage_with_the_drf_message() {
        let tz: chrono_tz::Tz = "UTC".parse().unwrap();
        for value in [
            json!("zzz"),
            json!("2024-13-45"),
            json!(12),
            json!(true),
            json!([1]),
        ] {
            assert_eq!(
                parse_drf_datetime(&value, &tz, "UTC").unwrap_err(),
                DATETIME_INVALID_MESSAGE,
                "{value}"
            );
        }
        // Gap time in New York (2024-03-10 02:30 never happened).
        let gap = parse_drf_datetime(
            &json!("2024-03-10T02:30:00"),
            &"America/New_York".parse().unwrap(),
            "America/New_York",
        );
        assert_eq!(
            gap.unwrap_err(),
            "Invalid datetime for the timezone \"America/New_York\"."
        );
    }

    #[test]
    fn offset_suffix_forms_parse() {
        assert_eq!(
            split_datetime_offset("2024-01-01T00:00:00Z"),
            Some(("2024-01-01T00:00:00", 0))
        );
        assert_eq!(
            split_datetime_offset("2024-01-01T00:00:00+02:00"),
            Some(("2024-01-01T00:00:00", 7200))
        );
        assert_eq!(
            split_datetime_offset("2024-01-01T00:00:00-0530"),
            Some(("2024-01-01T00:00:00", -(5 * 3600 + 30 * 60)))
        );
        assert_eq!(
            split_datetime_offset("2024-01-01T00:00:00+05"),
            Some(("2024-01-01T00:00:00", 5 * 3600))
        );
        assert_eq!(split_datetime_offset("2024-01-01T00:00:00"), None);
        assert_eq!(split_datetime_offset("2024-01-01"), None);
    }

    #[test]
    fn drf_datetime_edges_match_live_django() {
        let tz: chrono_tz::Tz = "UTC".parse().unwrap();
        // A lowercase zulu is not a tz arm.
        assert_eq!(
            parse_drf_datetime(&json!("2024-01-15T10:30:45z"), &tz, "UTC").unwrap_err(),
            DATETIME_INVALID_MESSAGE
        );
        // Fractions of any length truncate to six digits.
        for raw in [
            "2024-01-15T10:30:45.1234567",
            "2024-01-15T10:30:45.123456789012",
            "2024-01-15T10:30:45.1234567890123",
        ] {
            let parsed = parse_drf_datetime(&json!(raw), &tz, "UTC").unwrap();
            assert_eq!(
                parsed.to_rfc3339_opts(chrono::SecondsFormat::Micros, false),
                "2024-01-15T10:30:45.123456+00:00",
                "{raw}"
            );
        }
        // A bare trailing dot carries no fraction.
        let dot = parse_drf_datetime(&json!("2024-01-15T10:30:45."), &tz, "UTC").unwrap();
        assert_eq!(dot.to_rfc3339(), "2024-01-15T10:30:45+00:00");
        // Date-only must be padded; the year must be four digits.
        for value in [
            json!("2024-1-5"),
            json!("999-01-15T10:00:00"),
            json!("24-01-15"),
        ] {
            assert_eq!(
                parse_drf_datetime(&value, &tz, "UTC").unwrap_err(),
                DATETIME_INVALID_MESSAGE,
                "{value}"
            );
        }
        // Unpadded parts with a time part parse (Django's \d{1,2} arm).
        let loose = parse_drf_datetime(&json!("2024-1-5T1:2"), &tz, "UTC").unwrap();
        assert_eq!(loose.to_rfc3339(), "2024-01-05T01:02:00+00:00");
    }

    // -- Save derivation ---------------------------------------------------------------

    #[test]
    fn derive_save_applies_user_save_rules() {
        let now = Utc::now();
        // Normalization + backfill + no regen + staff passthrough.
        let derived = derive_save(Some("  A@X.IO "), "", "tok", None, false, false, now).unwrap();
        assert_eq!(derived.email, "a@x.io");
        assert_eq!(derived.display_name, "a");
        assert_eq!(derived.token, "tok");
        assert_eq!(derived.token_updated_at, None);
        assert!(!derived.is_staff);
        // Regen fires when `token_updated_at` is set, and re-stamps.
        let derived =
            derive_save(Some("a@x.io"), "Ada", "tok", Some(now), false, false, now).unwrap();
        assert_eq!(derived.token.len(), 64);
        assert_ne!(derived.token, "tok");
        assert_eq!(derived.token_updated_at, Some(now));
        // Superusers are staff; kept display names survive.
        let derived = derive_save(Some("a@x.io"), "Ada", "tok", None, true, false, now).unwrap();
        assert!(derived.is_staff);
        assert_eq!(derived.display_name, "Ada");
        // `None` email is the `AttributeError` 500.
        assert!(matches!(
            derive_save(None, "", "tok", None, false, false, now),
            Err(Denial::ServerError)
        ));
    }

    // -- Rendering ----------------------------------------------------------------------

    fn sample_rendered() -> RenderedUser {
        RenderedUser {
            id: "11111111-1111-1111-1111-111111111111".to_owned(),
            last_login: None,
            username: "ada".to_owned(),
            mobile_number: None,
            email: Some("ada@x.io".to_owned()),
            display_name: "Ada".to_owned(),
            first_name: "Ada".to_owned(),
            last_name: "L".to_owned(),
            avatar: "".to_owned(),
            avatar_asset: None,
            cover_image: None,
            cover_image_asset: None,
            avatar_url: None,
            cover_image_url: None,
            date_joined: "2024-01-01T00:00:00Z".to_owned(),
            created_at: "2024-01-01T00:00:00Z".to_owned(),
            updated_at: "2024-01-02T00:00:00Z".to_owned(),
            last_location: "".to_owned(),
            created_location: "".to_owned(),
            is_superuser: false,
            is_managed: false,
            is_password_expired: false,
            is_active: true,
            is_staff: false,
            is_email_verified: true,
            is_password_autoset: false,
            is_password_reset_required: false,
            token: "tok".to_owned(),
            last_active: None,
            last_login_time: Some("2024-01-03T00:00:00Z".to_owned()),
            last_logout_time: None,
            last_login_ip: "".to_owned(),
            last_logout_ip: "".to_owned(),
            last_login_medium: "email".to_owned(),
            last_login_uagent: "".to_owned(),
            token_updated_at: None,
            is_bot: false,
            bot_type: None,
            user_timezone: "UTC".to_owned(),
            is_email_valid: false,
            masked_at: None,
        }
    }

    #[test]
    fn me_render_key_order_matches_wire_fields() {
        let body = sample_rendered().render_me();
        let keys: Vec<&str> = body
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(keys, ser_user::USER_ME_WIRE_FIELDS);
        assert_eq!(body["email"], json!("ada@x.io"));
        assert_eq!(body["last_login_time"], json!("2024-01-03T00:00:00Z"));
    }

    #[test]
    fn full_render_key_order_matches_wire_fields() {
        let body = sample_rendered().render_full();
        let keys: Vec<&str> = body
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(keys, ser_user::USER_WIRE_FIELDS);
        assert!(!body.as_object().expect("object").contains_key("password"));
    }

    #[test]
    fn settings_branch_shapes_match_serializer_layer() {
        let full = ser_user::MeSettingsWorkspaceFullView {
            last_workspace_id: "w1",
            last_workspace_slug: "acme",
            last_workspace_name: "Acme",
            last_workspace_logo: "https://logo",
            fallback_workspace_id: "w1",
            fallback_workspace_slug: "acme",
            invites: 2,
        };
        let value = serde_json::to_value(&full).expect("serializes");
        let keys: Vec<&str> = value
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(keys, ser_user::ME_SETTINGS_WORKSPACE_FULL_KEYS);

        let row = ser_user::UserMeSettingsRow {
            id: "u1",
            email: Some("a@x.io"),
            last_workspace: Some(ser_user::MeSettingsLastWorkspace {
                id: "w1",
                slug: "acme",
                name: "Acme",
                logo: "https://logo",
            }),
            fallback_workspace: Some(ser_user::MeSettingsFallbackWorkspace {
                id: "w2",
                slug: "b",
            }),
            invites: 2,
        };
        let body =
            serde_json::to_value(ser_user::me_settings_to_representation(&row)).expect("view");
        assert_eq!(body["workspace"]["fallback_workspace_id"], json!("w1"));

        let row = ser_user::UserMeSettingsRow {
            id: "u1",
            email: Some("a@x.io"),
            last_workspace: None,
            fallback_workspace: Some(ser_user::MeSettingsFallbackWorkspace {
                id: "w2",
                slug: "b",
            }),
            invites: 0,
        };
        let body =
            serde_json::to_value(ser_user::me_settings_to_representation(&row)).expect("view");
        let keys: Vec<&str> = body["workspace"]
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(keys, ser_user::ME_SETTINGS_WORKSPACE_FALLBACK_KEYS);
        assert_eq!(body["workspace"]["last_workspace_id"], Value::Null);
    }

    // -- Password + misc ------------------------------------------------------------------

    #[test]
    fn password_salt_is_22_alphanumerics() {
        for _ in 0..20 {
            let salt = password_salt();
            assert_eq!(salt.len(), 22);
            assert!(salt.bytes().all(|b| b.is_ascii_alphanumeric()));
        }
    }

    #[test]
    fn random_password_hash_has_the_django_42_envelope() {
        let hash = encode_random_password();
        let parts: Vec<&str> = hash.split('$').collect();
        assert_eq!(parts.len(), 4);
        assert_eq!(parts[0], "pbkdf2_sha256");
        assert_eq!(parts[1], "600000");
        assert_eq!(parts[2].len(), 22);
        assert!(!parts[3].is_empty());
    }

    #[test]
    fn client_ip_matches_get_client_ip() {
        use crate::license::handlers_auth_forms::client_ip;
        assert_eq!(
            client_ip(Some("1.2.3.4, 5.6.7.8"), Some("9.9.9.9")).as_deref(),
            Some("1.2.3.4")
        );
        // Unstripped first entry.
        assert_eq!(
            client_ip(Some(" 1.2.3.4 "), None).as_deref(),
            Some(" 1.2.3.4 ")
        );
        assert_eq!(client_ip(None, Some("9.9.9.9")).as_deref(), Some("9.9.9.9"));
        assert_eq!(client_ip(None, None), None);
        assert_eq!(
            client_ip(Some(""), Some("9.9.9.9")).as_deref(),
            Some("9.9.9.9")
        );
    }
}
