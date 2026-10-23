//! D-01 license / instance-console handlers (stage 3).
//!
//! Ports `apps/api/pi_dash/license/api/views/` onto the merged D-01
//! foundation. Sibling handler issues own their files and consume shared
//! pieces read-only:
//!
//! * [`handlers_base`] — `views/base.py` (TimezoneMixin, `BaseAPIView`
//!   defaults, the exception matrix, `fields`/`expand`; PIDASHCONV-120).
//! * [`handlers_instance`] — `views/instance.py` (`InstanceEndpoint`
//!   GET/PATCH, `SignUpScreenVisitedEndpoint` POST; PIDASHCONV-120).
//! * [`handlers_admin`] — admins CRUD + me/session/sign-out
//!   (PIDASHCONV-121; `admin.py:44-86` and `admin.py:361-398`) plus the
//!   sign-in/sign-up HTTP shell over the [`handlers_auth_forms`] kernel
//!   (PIDASHCONV-827; `admin.py:89-358`).
//! * [`handlers_auth_forms`] — admin sign-up + sign-in form-POST redirect
//!   flows (PIDASHCONV-122; `admin.py:89-358`).
//! * [`handlers_config_workspace`] — configuration + workspace family
//!   (`views/configuration.py` + `views/workspace.py`; PIDASHCONV-123).
//!
//! [`routes`] merges the owned route groups; every other method on the
//! owned paths proxies to Django through the edge fallback.
//!
//! Shared handler plumbing (used by the config + workspace handlers,
//! mirroring the D-26 `app_issues` shapes):
//!
//! Ports the pieces every `license/api/views/*.py` handler reuses:
//!
//! - [`QueryMap`] / [`query_last`]: Django `QueryDict.get` (last value wins)
//!   over axum's `Query` extractor (`views/workspace.py:23,59`,
//!   `utils/paginator.py:643,678`).
//! - [`Denial`]: the exact error bodies (`views/base.py:58-95`,
//!   DRF `NotAuthenticated` / `PermissionDenied` defaults). License denials
//!   use the DRF-default 403 (`{"detail": ...}`), not the allow-style 403
//!   the `@allow_permission` views answer.
//! - [`Actor`] / [`require_admin`]: `BaseSessionAuthentication` (CSRF
//!   disabled, `authentication/session.py:7-9`) plus Django `auth.get_user`
//!   (session `_auth_user_id` / `_auth_user_backend` / `_auth_user_hash`
//!   verification, inactive users anonymous) plus DRF `SessionAuthentication`
//!   plus `InstanceAdminPermission.has_permission`
//!   (`permissions/instance.py:12-18`) through the auth-layer domain gate
//!   (`pidash_auth::license`, read-only).
//! - [`owned`]: route registration is the cutover granularity (same rule as
//!   the D-26 `app_issues` family): the owned method serves from Rust,
//!   every other method proxies so Django's 405-after-auth and metadata
//!   responses are preserved byte for byte.
//!
//! Sibling handler issues (120-123) extend this module with their own
//! routes; merges keep both sides.

use std::collections::HashMap;

use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Router;
use chrono_tz::Tz;
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

use crate::state::AppState;

/// `BACKEND_SESSION_KEY` value Django accepts here: `AUTHENTICATION_BACKENDS`
/// is exactly `(ModelBackend,)` (`settings/common.py:125`).
pub const MODEL_BACKEND: &str = "django.contrib.auth.backends.ModelBackend";

/// Salt for `user.get_session_auth_hash()`
/// (`django.contrib.auth` `HASH_SESSION_KEY` verification).
pub const SESSION_AUTH_HASH_SALT: &str =
    "django.contrib.auth.models.AbstractBaseUser.get_session_auth_hash";

/// Exact bytes of the DRF `NotAuthenticated` denial: anonymous on a guarded
/// endpoint (`request.successful_authenticator` is `None`, so
/// `permission_denied` raises `NotAuthenticated`, not `PermissionDenied`).
pub const UNAUTHENTICATED_BODY: &str =
    r#"{"detail":"Authentication credentials were not provided."}"#;
/// `views/base.py:92-95` generic branch (also the app base, same bytes).
pub const SERVER_ERROR_BODY: &str = r#"{"error":"Something went wrong please try again later"}"#;
/// `views/base.py:79-82` (`ObjectDoesNotExist` branch).
pub const NOT_FOUND_BODY: &str = r#"{"error":"The required object does not exist."}"#;

/// One query value, repeated or not. Mirrors the D-26 `app_issues` shape:
/// `serde_html_form` does not coerce a lone `?key=value` into a sequence,
/// so callers read first/last like Django's `QueryDict`.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(untagged)]
pub enum OneOrMany {
    One(String),
    Many(Vec<String>),
}

/// The multi-value query map every license handler extracts.
pub type QueryMap = HashMap<String, OneOrMany>;

/// All values for `key`, in order; `None` when absent.
pub fn query_values(query: &QueryMap, key: &str) -> Option<Vec<String>> {
    query.get(key).map(|value| match value {
        OneOrMany::One(one) => vec![one.clone()],
        OneOrMany::Many(many) => many.clone(),
    })
}

/// Django `QueryDict.get`: the last value, or `None`.
pub fn query_last(query: &QueryMap, key: &str) -> Option<String> {
    query_values(query, key).and_then(|values| values.into_iter().last())
}

/// Handler failure with its exact status + body.
#[derive(Debug)]
pub enum Denial {
    /// 401, DRF `NotAuthenticated` (anonymous on a guarded endpoint).
    Unauthorized,
    /// 403, DRF-default `PermissionDenied` (the license permission classes
    /// carry no `message`, so `APIView.permission_denied` renders the
    /// default detail, not the allow-style body).
    Forbidden,
    /// 404, `ObjectDoesNotExist` branch.
    NotFound,
    /// 400, `{"detail": ...}` (`ParseError` and friends).
    BadDetail(String),
    /// 415, unsupported media type (DRF `UnsupportedMediaType` — PIDASHCONV-1102).
    UnsupportedMediaType(String),
    /// 400, `{"error": ...}` (view-inline).
    BadError(String),
    /// 409, duplicate slug (`workspace.py:105-110`).
    Conflict(String, String),
    /// 500, generic branch.
    ServerError,
}

impl Denial {
    fn status_and_body(&self) -> (StatusCode, String) {
        match self {
            Denial::Unauthorized => (StatusCode::UNAUTHORIZED, UNAUTHENTICATED_BODY.to_owned()),
            Denial::Forbidden => (
                StatusCode::FORBIDDEN,
                crate::permissions::DEFAULT_DENIED_BODY.to_owned(),
            ),
            Denial::NotFound => (StatusCode::NOT_FOUND, NOT_FOUND_BODY.to_owned()),
            Denial::BadDetail(message) => (
                StatusCode::BAD_REQUEST,
                format!("{{\"detail\":{}}}", json_string(message)),
            ),
            Denial::UnsupportedMediaType(message) => (
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                format!("{{\"detail\":{}}}", json_string(message)),
            ),
            Denial::BadError(message) => (
                StatusCode::BAD_REQUEST,
                format!("{{\"error\":{}}}", json_string(message)),
            ),
            Denial::Conflict(key, message) => (
                StatusCode::CONFLICT,
                format!("{{\"{}\":{}}}", key, json_string(message)),
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

/// `FileAsset.asset_url` (`db/models/asset.py:80-100`) as seen through a
/// non-required DRF field: `Present(url)` renders the key (`None` is JSON
/// null, the unknown-entity passthrough), `Skip` omits it — the plain
/// `AttributeError` raised inside the property (NULL workspace on
/// attachment/description entities: `None.slug`) is caught by DRF's
/// `Field.get_attribute` and becomes `SkipField`
/// (`rest_framework/fields.py:445-451`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssetUrl {
    Present(Option<String>),
    Skip,
}

/// Pure `asset_url` for a resolved (unscoped) asset row: `entity` is the
/// raw `entity_type` (NULL/other yields `None`, never a fallback),
/// `workspace_slug` the unscoped workspace slug when `workspace_id` is
/// `Some`. A NULL `workspace_id` is the `None.slug` path (`Skip`); a `Some`
/// id with no row (FK-unreachable in practice) raises
/// `RelatedObjectDoesNotExist`, which DRF's `get_attribute` catches as
/// `ObjectDoesNotExist` and renders as null.
pub fn file_asset_url(
    id: &uuid::Uuid,
    entity: Option<&str>,
    workspace_id: Option<&uuid::Uuid>,
    project_id: Option<&uuid::Uuid>,
    issue_id: Option<&uuid::Uuid>,
    workspace_slug: Option<&str>,
) -> AssetUrl {
    match entity {
        Some("WORKSPACE_LOGO" | "USER_AVATAR" | "USER_COVER" | "PROJECT_COVER") => {
            AssetUrl::Present(Some(format!("/api/assets/v2/static/{id}/")))
        }
        Some("ISSUE_ATTACHMENT") => {
            if workspace_id.is_none() {
                return AssetUrl::Skip;
            }
            let Some(slug) = workspace_slug else {
                return AssetUrl::Present(None);
            };
            AssetUrl::Present(Some(format!(
                "/api/assets/v2/workspaces/{}/projects/{}/issues/{}/attachments/{id}/",
                slug,
                none_str(&project_id.map(uuid::Uuid::to_string)),
                none_str(&issue_id.map(uuid::Uuid::to_string)),
            )))
        }
        Some(
            "ISSUE_DESCRIPTION"
            | "COMMENT_DESCRIPTION"
            | "PAGE_DESCRIPTION"
            | "DRAFT_ISSUE_DESCRIPTION",
        ) => {
            if workspace_id.is_none() {
                return AssetUrl::Skip;
            }
            let Some(slug) = workspace_slug else {
                return AssetUrl::Present(None);
            };
            AssetUrl::Present(Some(format!(
                "/api/assets/v2/workspaces/{}/projects/{}/{id}/",
                slug,
                none_str(&project_id.map(uuid::Uuid::to_string)),
            )))
        }
        _ => AssetUrl::Present(None),
    }
}

/// Python `str(None)` inside the asset-URL f-strings.
fn none_str(value: &Option<String>) -> String {
    value.clone().unwrap_or_else(|| "None".to_owned())
}

/// One `users` row for session resolution:
/// `(id, email, first_name, last_name, password, is_active, user_timezone)`.
type UserLookupRow = (
    uuid::Uuid,
    Option<String>,
    String,
    String,
    String,
    bool,
    String,
);

/// The authenticated actor: the `users` row behind a verified session.
#[derive(Debug, Clone)]
pub struct Actor {
    pub id: uuid::Uuid,
    pub email: Option<String>,
    pub first_name: String,
    pub last_name: String,
    /// `TimezoneMixin.initial`: `ZoneInfo(request.user.user_timezone)`.
    /// A bad zone raises in Python (`handle_exception` 500); unparsable
    /// here is therefore [`Denial::ServerError`], not a fallback.
    pub timezone: Tz,
}

/// `request.user` from the Django session, or `None` for anonymous.
/// Mirrors `django.contrib.auth.get_user` + DRF `SessionAuthentication`:
/// the session must carry a UUID `_auth_user_id` for the `ModelBackend`
/// with a matching `_auth_user_hash`, and the user row must exist and be
/// active. Anything else is anonymous (Django flushes the session; the
/// observable outcome — anonymous — is what is ported).
pub async fn resolve_actor(
    pool: &sqlx::PgPool,
    secret_key: &[u8],
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Result<Option<Actor>, Denial> {
    let handle = match extension {
        Some(axum::Extension(handle)) => handle,
        None => return Ok(None),
    };
    let session = handle.snapshot();
    // `session.get` marks `accessed`, like every `SessionBase` getter.
    let mut session = session;
    let user_id_raw = session
        .get("_auth_user_id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_owned();
    let backend = session
        .get("_auth_user_backend")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_owned();
    let session_hash = session
        .get("_auth_user_hash")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_owned();
    if backend != MODEL_BACKEND {
        return Ok(None);
    }
    let user_id: uuid::Uuid = match user_id_raw.parse() {
        Ok(id) => id,
        Err(_) => return Ok(None),
    };
    let row: Option<UserLookupRow> = sqlx::query_as(
        r#"SELECT id, email, first_name, last_name, password, is_active, user_timezone
           FROM users WHERE id = $1"#,
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let Some((id, email, first_name, last_name, password, is_active, user_timezone)) = row else {
        // `ModelBackend.get_user` catches `DoesNotExist` and returns `None`.
        return Ok(None);
    };
    if !is_active {
        // DRF `SessionAuthentication.authenticate`: inactive users are `None`.
        return Ok(None);
    }
    if session_hash.is_empty() || !verify_session_hash(&session_hash, &password, secret_key) {
        return Ok(None);
    }
    let timezone: Tz = user_timezone.parse().map_err(|_| Denial::ServerError)?;
    Ok(Some(Actor {
        id,
        email,
        first_name,
        last_name,
        timezone,
    }))
}

/// `user.get_session_auth_hash()`: `salted_hmac(...get_session_auth_hash,
/// password).hexdigest()`, compared constant-time. Shared by the sibling
/// instance/admin preambles, which verify the same three session facts as
/// [`resolve_actor`] (PIDASHCONV-1097).
pub(crate) fn verify_session_hash(
    session_hash: &str,
    password_field: &str,
    secret_key: &[u8],
) -> bool {
    let key = Sha256::digest([SESSION_AUTH_HASH_SALT.as_bytes(), secret_key].concat());
    let mut mac = Hmac::<Sha256>::new_from_slice(&key).expect("HMAC-SHA256 accepts any key length");
    mac.update(password_field.as_bytes());
    let expected = hex_encode(&mac.finalize().into_bytes());
    constant_time_eq(expected.as_bytes(), session_hash.as_bytes())
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

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for i in 0..a.len() {
        diff |= a[i] ^ b[i];
    }
    diff == 0
}

/// `InstanceAdminPermission` for one request: anonymous denies 401 before
/// any DB access (`permissions/instance.py:14-15`); otherwise the first
/// `Instance` row plus an `InstanceAdmin(role__gte=15)` row for
/// `(instance, user)` decides through the auth-layer domain gate, and a
/// denial is the DRF-default 403.
pub async fn require_admin(
    state: &AppState,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Result<Actor, Denial> {
    let pool = state
        .pools()
        .map(|pools| pools.primary())
        .ok_or(Denial::ServerError)?;
    let actor = resolve_actor(pool, state.settings().secret_key.as_bytes(), extension)
        .await?
        .ok_or(Denial::Unauthorized)?;
    // `Instance.objects.first()`: the exact fixture SQL; only the id is
    // needed, but the row parses through the queries-layer mapper so the
    // emitted SQL stays identical.
    let instance = sqlx::query(pidash_db::license::queries::INSTANCE_FIRST_SQL)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let Some(instance_row) = instance else {
        // `filter(instance=None, ...)` matches nothing (non-nullable FK).
        return Err(Denial::Forbidden);
    };
    let instance_id = pidash_db::license::queries::map_instance_row(&instance_row)
        .map_err(|_| Denial::ServerError)?
        .id;
    // `InstanceAdmin.objects.filter(role__gte=15, instance, user).exists()`:
    // the exact permission-check SQL; presence is the decision.
    let admin = sqlx::query(&pidash_db::license::queries::admin_permission_check_sql())
        .bind(instance_id)
        .bind(pidash_db::license::queries::ADMIN_PERMISSION_ROLE_GTE)
        .bind(actor.id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let allows = pidash_auth::license::permissions::instance_admin_allows(
        &pidash_auth::license::permissions::InstanceAdminFacts {
            authenticated: true,
            instance_present: true,
            has_admin_row: admin.is_some(),
        },
    );
    if allows {
        Ok(actor)
    } else {
        Err(Denial::Forbidden)
    }
}

/// The owned methods on a license path serve from Rust while every other
/// method falls through to Django (its 405-after-auth, sibling actions,
/// and metadata responses live there). `HEAD` proxies explicitly: axum
/// would auto-serve it from `get`, but Django defines no `head` and 405s
/// after auth. `OPTIONS` proxies so DRF metadata (401 anon / 200 authed)
/// is preserved.
pub fn owned(
    methods: axum::routing::MethodRouter<AppState>,
    owned: &[&str],
) -> axum::routing::MethodRouter<AppState> {
    let mut router = methods;
    for other in ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] {
        if owned.contains(&other) {
            continue;
        }
        router = match other {
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

/// Render a value body as compact JSON (`JSONRenderer`, `COMPACT_JSON`):
/// no spaces, struct field order preserved.
pub fn json_response<T: serde::Serialize>(value: &T) -> Response {
    let body = serde_json::to_string(value).expect("serializable response");
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body))
        .expect("json response")
}

/// DRF `DateTimeField(allow_null=True)` over one JSON value: the shared
/// `deleted_at` input shape for the instance PATCH and workspace POST
/// validators (PIDASHCONV-1099). Neither serializer marks `deleted_at`
/// read-only, so DRF maps it to a writable datetime that admins can set
/// (ISO-8601) or clear (`null`).
///
/// * `null` clears (`allow_null`, from `null=True` on the model field).
/// * Only strings parse: `parse_datetime` needs `str`, so every other
///   JSON scalar answers the `invalid` message (`to_internal_value`
///   suppresses the `TypeError` and falls through).
/// * Strings parse through the shared [`parse_drf_datetime`](crate::app_project::handlers_project::parse_drf_datetime)
///   (`parse_datetime` + `enforce_timezone` in the request zone): Django's
///   `dateparse` and DRF's `DateTimeField` are behaviorally identical
///   across the versions in play (4.2/6, 3.15/3.18 — diffed), so there is
///   one oracle and one implementation.
///
/// The `timezone` is the request zone (`TimezoneMixin` activates the
/// actor's `user_timezone`): naive wall times attach there
/// (`make_aware`, `fold=0`, always accepted with `ZoneInfo` — gap and
/// ambiguous times alike, probed), aware values convert there
/// (`astimezone`, `OverflowError` → the `overflow` message when either
/// conversion leaves `0001..=9999`).
pub(crate) fn parse_deleted_at(
    value: &serde_json::Value,
    timezone: Tz,
) -> Result<Option<chrono::DateTime<chrono::Utc>>, &'static str> {
    use crate::app_project::handlers_project::{
        DttmParse, INVALID_DATETIME_MESSAGE, OVERFLOW_DATETIME_MESSAGE,
    };
    if value.is_null() {
        return Ok(None);
    }
    let serde_json::Value::String(text) = value else {
        return Err(INVALID_DATETIME_MESSAGE);
    };
    match crate::app_project::handlers_project::parse_drf_datetime(text, timezone) {
        DttmParse::Ok(when) => Ok(Some(when)),
        DttmParse::Invalid => Err(INVALID_DATETIME_MESSAGE),
        DttmParse::Overflow => Err(OVERFLOW_DATETIME_MESSAGE),
    }
}

pub mod form_body;
pub mod handlers_admin;
pub mod handlers_auth_forms;
pub mod handlers_base;
pub mod handlers_config_workspace;
pub mod handlers_instance;

pub use handlers_config_workspace::config_workspace_routes;

/// Owned D-01 instance-console routes (cutover granularity: registered
/// paths serve from Rust, everything else keeps proxying).
pub fn routes() -> Router<AppState> {
    handlers_instance::routes()
        .merge(config_workspace_routes())
        .merge(handlers_admin::routes())
        // `runner/admin_urls.py`, mounted at `runners/` by `license/urls.py`.
        .merge(crate::runner_admin::routes())
}

#[cfg(test)]
mod asset_url_tests {
    use super::*;

    #[test]
    fn file_asset_url_follows_asset_py_branches() {
        let id = uuid::Uuid::nil();
        let ws = uuid::Uuid::nil();
        let proj = uuid::Uuid::nil();
        let issue = uuid::Uuid::nil();
        // Static entities render without any context.
        assert_eq!(
            file_asset_url(&id, Some("USER_AVATAR"), None, None, None, None),
            AssetUrl::Present(Some(
                "/api/assets/v2/static/00000000-0000-0000-0000-000000000000/".to_owned()
            ))
        );
        // NULL/other entities pass None through (no fallback).
        assert_eq!(
            file_asset_url(&id, None, None, None, None, None),
            AssetUrl::Present(None)
        );
        assert_eq!(
            file_asset_url(&id, Some("DRAFT_ISSUE_ATTACHMENT"), None, None, None, None),
            AssetUrl::Present(None)
        );
        // Attachment/description entities need the workspace slug: a NULL
        // workspace is the AttributeError path (Skip), a dangling id the
        // ObjectDoesNotExist path (null).
        assert_eq!(
            file_asset_url(&id, Some("ISSUE_ATTACHMENT"), None, None, None, None),
            AssetUrl::Skip
        );
        assert_eq!(
            file_asset_url(&id, Some("ISSUE_DESCRIPTION"), Some(&ws), None, None, None),
            AssetUrl::Present(None)
        );
        // NULL project/issue ids render as "None", like the f-strings.
        assert_eq!(
            file_asset_url(
                &id,
                Some("ISSUE_ATTACHMENT"),
                Some(&ws),
                None,
                None,
                Some("acme")
            ),
            AssetUrl::Present(Some(format!(
                "/api/assets/v2/workspaces/acme/projects/None/issues/None/attachments/{id}/"
            )))
        );
        assert_eq!(
            file_asset_url(
                &id,
                Some("PAGE_DESCRIPTION"),
                Some(&ws),
                Some(&proj),
                Some(&issue),
                Some("acme")
            ),
            AssetUrl::Present(Some(format!(
                "/api/assets/v2/workspaces/acme/projects/{proj}/{id}/"
            )))
        );
    }
}

#[cfg(test)]
mod deleted_at_tests {
    use super::*;
    use chrono::{DateTime, Utc};

    /// One oracle row: the UTC instant DRF stores, or the 400 it answers.
    /// Expected values transcribed from the `.probe1099_oracle_312.txt`
    /// battery (real `DateTimeField().to_internal_value`, `USE_TZ`, Python
    /// 3.12 + Django 4.2.30 + DRF 3.15.2 — the CI/production stack).
    enum Expect {
        /// Stored UTC instant, RFC 3339 (`Z`).
        Instant(&'static str),
        Invalid,
        Overflow,
    }

    fn check(text: &str, zone: &str, expect: Expect) {
        use crate::app_project::handlers_project::{
            INVALID_DATETIME_MESSAGE, OVERFLOW_DATETIME_MESSAGE,
        };
        let timezone: Tz = zone.parse().expect("zone");
        let value = serde_json::Value::String(text.to_owned());
        match expect {
            Expect::Instant(want) => {
                let want: DateTime<Utc> = want.parse().expect("want parses");
                assert_eq!(
                    parse_deleted_at(&value, timezone),
                    Ok(Some(want)),
                    "input {text:?} in {zone}"
                );
            }
            Expect::Invalid => assert_eq!(
                parse_deleted_at(&value, timezone),
                Err(INVALID_DATETIME_MESSAGE),
                "input {text:?} in {zone}"
            ),
            Expect::Overflow => assert_eq!(
                parse_deleted_at(&value, timezone),
                Err(OVERFLOW_DATETIME_MESSAGE),
                "input {text:?} in {zone}"
            ),
        }
    }

    #[test]
    fn deleted_at_matches_oracle_in_utc() {
        use Expect::{Instant as At, Invalid, Overflow};
        for (text, expect) in [
            ("2026-01-01T00:00:00Z", At("2026-01-01T00:00:00Z")),
            ("2026-01-01T00:00:00+00:00", At("2026-01-01T00:00:00Z")),
            (
                "2026-01-01T00:00:00.123456Z",
                At("2026-01-01T00:00:00.123456Z"),
            ),
            // Fractions truncate (never round) to micros.
            (
                "2026-01-01T00:00:00.123456789Z",
                At("2026-01-01T00:00:00.123456Z"),
            ),
            (
                "2026-01-01T00:00:00.123456789012",
                At("2026-01-01T00:00:00.123456Z"),
            ),
            (
                "2026-01-01T00:00:00.1234567",
                At("2026-01-01T00:00:00.123456Z"),
            ),
            ("2026-01-01T00:00:00.5", At("2026-01-01T00:00:00.500000Z")),
            // Comma fractions; fractions after minutes.
            (
                "2026-01-01T00:00:00,123456Z",
                At("2026-01-01T00:00:00.123456Z"),
            ),
            ("2026-01-01T00:00,5", At("2026-01-01T00:00:00.500000Z")),
            // Short times, date-only, unpadded (regex fallback).
            ("2026-01-01T00:00Z", At("2026-01-01T00:00:00Z")),
            ("2026-01-01T00:00", At("2026-01-01T00:00:00Z")),
            ("2026-01-01T00", At("2026-01-01T00:00:00Z")),
            ("2026-01-01 00:00:00", At("2026-01-01T00:00:00Z")),
            ("2026-01-01T00:00:00", At("2026-01-01T00:00:00Z")),
            ("2026-01-01", At("2026-01-01T00:00:00Z")),
            ("2026-1-1T0:0", At("2026-01-01T00:00:00Z")),
            // Basic + week dates (3.12 `fromisoformat`; 3.10 rejects).
            ("20260101T000000Z", At("2026-01-01T00:00:00Z")),
            ("20260101T000000", At("2026-01-01T00:00:00Z")),
            ("20260101", At("2026-01-01T00:00:00Z")),
            ("2026-W01-1", At("2025-12-29T00:00:00Z")),
            ("2026-W01-1T00:00:00", At("2025-12-29T00:00:00Z")),
            ("2026-001", Invalid),
            // Any single char separates date and time.
            ("2026-01-01X00:00:00", At("2026-01-01T00:00:00Z")),
            // Offsets: colon, basic, hour-only, seconds, unnormalized.
            ("2026-01-01T00:00:00+05:30", At("2025-12-31T18:30:00Z")),
            ("2026-01-01T00:00:00+0530", At("2025-12-31T18:30:00Z")),
            ("2026-01-01T00:00:00+05", At("2025-12-31T19:00:00Z")),
            ("2026-01-01T00:00:00+05:30:15", At("2025-12-31T18:29:45Z")),
            (
                "2026-01-01T00:00:00+05:30:15.5",
                At("2025-12-31T18:29:44.500000Z"),
            ),
            ("2026-01-01T00:00:00 +05:00", At("2025-12-31T19:00:00Z")),
            (
                "2026-01-01 00:00:00.123 +05:00",
                At("2025-12-31T19:00:00.123000Z"),
            ),
            ("2026-01-01T00:00:00+00:60", At("2025-12-31T23:00:00Z")),
            ("2026-01-01T00:00:00+14:00", At("2025-12-31T10:00:00Z")),
            ("2026-01-01T00:00:00+15:00", At("2025-12-31T09:00:00Z")),
            ("2026-01-01T00:00:00+23:59", At("2025-12-31T00:01:00Z")),
            ("2026-01-01T00:00:00+24:00", Invalid),
            ("2026-01-01T00:00:00+99:99", Invalid),
            ("2026-01-01T00:00:00+05:3", Invalid),
            ("2026-01-01T00:00:00+5:30", Invalid),
            // Trailing whitespace only without a tz designator.
            ("2026-01-01T00:00:00 ", At("2026-01-01T00:00:00Z")),
            ("2026-01-01T00:00:00\n", At("2026-01-01T00:00:00Z")),
            ("2026-01-01T00:00:00\t", At("2026-01-01T00:00:00Z")),
            ("2026-01-01T00:00:00+00:00 ", Invalid),
            ("2026-01-01T00:00:00Z ", Invalid),
            (" 2026-01-01T00:00:00", Invalid),
            ("\t2026-01-01T00:00:00", Invalid),
            ("2026-01-01T00:00:00+00:00\x00", At("2026-01-01T00:00:00Z")),
            ("2026-01-01T00:00:00\x00", At("2026-01-01T00:00:00Z")),
            // Lowercase `z` is not a designator.
            ("2026-01-01t00:00:00z", Invalid),
            // Out-of-range components.
            ("2026-01-01T24:00:00", Invalid),
            ("2026-01-01T25:00:00", Invalid),
            ("2026-01-01T00:00:61", Invalid),
            ("2026-13-01T00:00:00", Invalid),
            ("2026-01-32T00:00:00", Invalid),
            ("2026-02-30T00:00:00", Invalid),
            // Malformed.
            ("2026-01-01T", Invalid),
            ("T00:00:00", Invalid),
            ("00:00:00", Invalid),
            ("2026-01-01T00:00:00ZZ", Invalid),
            ("not-a-date", Invalid),
            ("", Invalid),
            // Year bounds.
            ("9999-12-31T23:59:59Z", At("9999-12-31T23:59:59Z")),
            ("0001-01-01T00:00:00Z", At("0001-01-01T00:00:00Z")),
            (
                "2026-01-01T00:00:00.000000+00:00",
                At("2026-01-01T00:00:00Z"),
            ),
            // DRF's `strptime(value, 'iso-8601')` fallthrough: the literal
            // matches case-insensitively and yields naive 1900-01-01.
            ("iso-8601", At("1900-01-01T00:00:00Z")),
            ("ISO-8601", At("1900-01-01T00:00:00Z")),
            ("Iso-8601", At("1900-01-01T00:00:00Z")),
            (" iso-8601", Invalid),
            ("iso-8601 ", Invalid),
            // `astimezone` overflow (either conversion out of range).
            ("9999-12-31T10:00:00-14:00", Overflow),
            ("9999-12-31T23:59:59-23:59", Overflow),
            ("0001-01-01T00:00:00+23:59", Overflow),
            ("0001-01-01T00:00:00-23:59", At("0001-01-01T23:59:00Z")),
        ] {
            check(text, "UTC", expect);
        }
    }

    #[test]
    fn deleted_at_matches_oracle_in_new_york() {
        use Expect::{Instant as At, Overflow};
        for (text, expect) in [
            // Naive wall times attach in the request zone.
            ("2026-06-01T12:00:00", At("2026-06-01T16:00:00Z")),
            ("2026-01-01", At("2026-01-01T05:00:00Z")),
            ("20260101T000000Z", At("2026-01-01T00:00:00Z")),
            // Gap and ambiguous times are accepted (`fold=0` /
            // pre-transition offset — the `make_aware` 400 is dead with
            // `ZoneInfo`).
            ("2026-03-08T02:30:00", At("2026-03-08T07:30:00Z")),
            ("2025-11-02T01:30:00", At("2025-11-02T05:30:00Z")),
            // Aware values convert through the request zone.
            ("2026-01-01T00:00:00+05:30", At("2025-12-31T18:30:00Z")),
            (
                "2026-01-01T00:00:00+05:30:15.5",
                At("2025-12-31T18:29:44.500000Z"),
            ),
            ("iso-8601", At("1900-01-01T05:00:00Z")),
            ("9999-12-31T10:00:00-14:00", Overflow),
            ("9999-12-31T23:59:59-23:59", Overflow),
            ("0001-01-01T00:00:00+23:59", Overflow),
        ] {
            check(text, "America/New_York", expect);
        }
    }

    #[test]
    fn deleted_at_matches_oracle_round_two() {
        use Expect::{Instant as At, Invalid};
        // Separator + offset splits (`.probe1099_oracle2_312.txt`).
        for (text, expect) in [
            ("2026-01-01t00:00:00Z", At("2026-01-01T00:00:00Z")),
            ("2026-01-01T00:00:00z", Invalid),
            ("2026-01-01_00:00:00Z", At("2026-01-01T00:00:00Z")),
            // A digit separates fine; the short hour fails, not the sep.
            ("2026-01-0100:00:00Z", Invalid),
            // The sign doubles as the separator: naive 05:30.
            ("2026-01-01+05:30", At("2026-01-01T05:30:00Z")),
            ("2026-01-01Z", Invalid),
            ("2026-01-01T00:00:00+00", At("2026-01-01T00:00:00Z")),
            ("2026-01-01T00:00:00-05", At("2026-01-01T05:00:00Z")),
            ("2026-01-01T00:00:00+0000", At("2026-01-01T00:00:00Z")),
            ("2026-01-01T00:00:00+00:00:00", At("2026-01-01T00:00:00Z")),
            // Offset sub-second fractions truncate to the minute.
            ("2026-01-01T00:00:00+00:00:00.5", At("2026-01-01T00:00:00Z")),
            (
                "2026-01-01T00:00:00.123456+05:30",
                At("2025-12-31T18:30:00.123456Z"),
            ),
            // `$` matches before a trailing newline, even after a designator.
            ("2026-01-01T00:00:00Z\n", At("2026-01-01T00:00:00Z")),
            ("2026-01-01T00:00:00+00:00\n", At("2026-01-01T00:00:00Z")),
            ("2026-01-01T00:00:00.", Invalid),
            ("+002026-01-01T00:00:00", Invalid),
            ("0000-01-01T00:00:00Z", Invalid),
            ("10000-01-01T00:00:00Z", Invalid),
            ("2026-01-01T00:00:00+23:60", Invalid),
            ("2026-01-01T00:00:00+99", Invalid),
            // Week-date corners: digit-sep ban, Monday backtrack, Sunday
            // fallback, cross-year resolution, past-9999 rejection.
            ("2024-W01-1200:00", Invalid),
            ("2024W011200", At("2024-01-01T00:00:00Z")),
            ("2024-W01-7x00:00", At("2024-01-07T00:00:00Z")),
            ("2024W013000000", At("2024-01-01T00:00:00Z")),
            ("2020-W53-7", At("2021-01-03T00:00:00Z")),
            ("9999-W52-7", Invalid),
            ("20260101T000000+0530", At("2025-12-31T18:30:00Z")),
            ("2026-W01-1T00:00:00+05:30", At("2025-12-28T18:30:00Z")),
        ] {
            check(text, "UTC", expect);
        }
        // Dateline-skip and half-hour-DST zones: gap walls resolve with
        // the pre-transition offset, never the `make_aware` 400.
        for (zone, text, expect) in [
            (
                "Pacific/Apia",
                "2011-12-30T00:00:00",
                At("2011-12-30T10:00:00Z"),
            ),
            (
                "Pacific/Apia",
                "2011-12-29T12:00:00",
                At("2011-12-29T22:00:00Z"),
            ),
            (
                "Australia/Lord_Howe",
                "2025-10-05T02:15:00",
                At("2025-10-04T15:45:00Z"),
            ),
            (
                "Australia/Lord_Howe",
                "2025-04-06T02:00:00",
                At("2025-04-05T15:30:00Z"),
            ),
        ] {
            check(text, zone, expect);
        }
    }

    #[test]
    fn deleted_at_json_shapes_match_drf() {
        use crate::app_project::handlers_project::INVALID_DATETIME_MESSAGE;
        use serde_json::json;
        let utc: Tz = "UTC".parse().expect("utc");
        // `null` clears.
        assert_eq!(parse_deleted_at(&json!(null), utc), Ok(None));
        // Non-strings never parse (`parse_datetime` needs `str`).
        for raw in [
            json!(123),
            json!(12.5),
            json!(true),
            json!(["x"]),
            json!({"a": 1}),
        ] {
            assert_eq!(
                parse_deleted_at(&raw, utc),
                Err(INVALID_DATETIME_MESSAGE),
                "input {raw}"
            );
        }
    }
}
