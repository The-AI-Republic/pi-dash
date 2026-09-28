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
//!   (PIDASHCONV-121; `admin.py:44-86` and `admin.py:361-398`).
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
/// password).hexdigest()`, compared constant-time.
fn verify_session_hash(session_hash: &str, password_field: &str, secret_key: &[u8]) -> bool {
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
/// and metadata responses live there). `HEAD` rides axum's `get` handling
/// like Django's `GET`-backed `HEAD`; `OPTIONS` proxies so DRF metadata
/// (401 anon / 200 authed) is preserved.
pub fn owned(
    methods: axum::routing::MethodRouter<AppState>,
    owned: &[&str],
) -> axum::routing::MethodRouter<AppState> {
    let mut router = methods;
    for other in ["GET", "POST", "PUT", "PATCH", "DELETE", "OPTIONS"] {
        if owned.contains(&other) {
            continue;
        }
        router = match other {
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

pub mod handlers_admin;
pub mod handlers_auth_forms;
pub mod handlers_base;
pub mod handlers_config_workspace;
pub mod handlers_instance;

pub use handlers_config_workspace::config_workspace_routes;

/// Owned D-01 instance-console routes (cutover granularity: registered
/// paths serve from Rust, everything else keeps proxying).
pub fn routes() -> Router<AppState> {
    handlers_instance::routes().merge(config_workspace_routes())
}
