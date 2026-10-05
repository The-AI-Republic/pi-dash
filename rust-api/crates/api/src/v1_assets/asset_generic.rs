//! Generic-asset handlers (D-21, stage 5, PIDASHCONV-421).
//!
//! Ports `GenericAssetEndpoint` (`apps/api/pi_dash/api/views/asset.py:403-621`)
//! with routes from `apps/api/pi_dash/api/urls/asset.py:34-43`:
//!
//! * `POST workspaces/<slug>/assets/` (`asset.py:497-578`)
//! * `GET workspaces/<slug>/assets/<uuid:asset_id>/` (`asset.py:419-466`)
//! * `PATCH workspaces/<slug>/assets/<uuid:asset_id>/` (`asset.py:600-621`)
//!
//! Fixture: `rust-api/fixtures/v1_assets/fx-h-asset-generic.json`
//! (`fx-h-asset-generic`; trace: `rust-api/fixtures/v1_assets/TRACE.md`).
//! The `#[cfg(test)]` suite replays every golden body below, so any drift
//! fails the build. Consumed layers (all merged, read-only here):
//! serializers (`pidash_types::v1_assets::asset`), models
//! (`pidash_db::v1_assets::model::file_asset`), queries
//! (`pidash_db::v1_assets::asset_queries`), gates
//! (`super::permissions`), tasks
//! (`pidash_jobs::v1_assets::tasks::{metadata_job, storage_metadata_missing}`).
//!
//! Shape of the port (translate, don't redesign):
//!
//! * Route registration is the cutover granularity (Porting guide cutover
//!   row, `app_issues::routes` precedent): [`routes`] serves the three
//!   owned methods; every other method falls through to
//!   [`crate::edge::proxy`] so Django answers the 405s, OPTIONS metadata
//!   and resolver 404s exactly as before. Mount wiring (merging [`routes`]
//!   into the app router) belongs to PIDASHCONV-426 — this module must not
//!   touch `overlay.rs` / `routes.rs`.
//! * Authentication is `APIKeyAuthentication`
//!   (`api/middleware/api_authentication.py`): the `X-Api-Key` header only,
//!   via the [`pidash_auth::token`] kernel (exact match, `is_active`,
//!   `expired_at` null or strictly future, `last_used` bump; `mt_` prefix
//!   takes the machine-token path with its revoke-then-deny). Missing/empty
//!   header answers 401; any other failure answers 403 `Given API token is
//!   not valid` (the merged D-19 `v1_projects::handlers_members`
//!   precedent, pinned by contract `test_me_bad_token_403`). The class
//!   carries no extra permission beyond `IsAuthenticated`
//!   (`api/views/base.py:103`), so there is no workspace-membership gate
//!   on any of the three units — exactly like Python.
//! * Order per request mirrors DRF `initial()`: authN (401/403), then the
//!   body. Anonymous callers never reach a lookup.
//! * Bodies are JSON (`request.data` over the default parsers; the contract
//!   suites speak JSON). An empty or malformed JSON body is proxied to
//!   Django untouched (the D-19 `proxy_through` precedent) so the DRF
//!   `ParseError` bytes stay exact; a well-formed non-object body answers
//!   the generic 500, matching the `AttributeError` the view's `.get`
//!   raises on a list/scalar.
//! * Reads reuse `pidash_db::v1_assets::asset_queries`
//!   (`generic_detail_select_sql`, `external_dedupe_select_sql`); the
//!   workspace-first lookup order is handler-owned (a missing workspace
//!   answers `Workspace not found` before the asset lookup runs).
//!   Writes are inline single statements with the same column semantics
//!   as `FileAsset.objects.create` / `save(update_fields=[...])`
//!   (Django `auto_now`/`auto_now_add` become explicit `now()` binds).
//! * S3 presigning mirrors `S3Storage` (`settings/storage.py`) offline
//!   (no network), following the merged D-31 `app_assets` SigV4 shape:
//!   MinIO mode signs against `{scheme}://{Host}` (scheme from
//!   `X-Forwarded-Proto`, default `http`), otherwise against the
//!   configured endpoint URL or the virtual-hosted AWS default. The GET
//!   signs `response-content-disposition=inline;
//!   filename*=UTF-8''<name>`; the POST policy lists conditions in
//!   `storage.py` order with CPython `json.dumps` separators.
//! * The PATCH metadata publish (`get_asset_object_metadata.delay`, the
//!   `asset_id=str(asset_id)` kwarg form at `asset.py:614-615`) enqueues
//!   through [`pidash_jobs::v1_assets::tasks::metadata_job`]. Best-effort:
//!   a failed enqueue warns and the 204 stands (the merged D-31
//!   `handlers_v2_user_workspace::publish_metadata` precedent; the proxy
//!   contract tests never run a worker).
//!
//! Ported bugs (translate, don't redesign; also listed in the PR):
//!
//! * BUG-size-null (`asset.py:505` vs `:511`): the `int()` conversion runs
//!   before the required check, so an explicit `size: null` raises
//!   `TypeError` into `handle_exception`'s generic 500 instead of the
//!   documented 400. Ported: null size answers 500
//!   `Something went wrong please try again later`.
//! * BUG-workspace-body (`asset.py` post vs get): the POST workspace miss
//!   falls through to `handle_exception` (`base.py:154-159`) answering
//!   `{"error": "The requested resource does not exist."}`, while the GET
//!   catches it locally as `{"error": "Workspace not found"}`. Ported:
//!   each unit keeps its own body.
//! * BUG-project-raw (`asset.py:558`): `project_id` is stored with no
//!   UUID/FK validation. The column still coerces through
//!   `UUIDField.to_python` on save: garbage strings and non-integers
//!   raise `ValidationError` (400 `Please provide valid detail`);
//!   `""` is rewritten to NULL by `ForeignObject.get_db_prep_save`
//!   (the 200 empty-string row); ints ride `uuid.UUID(int=…)` (bools
//!   included — `True`/`False` are ints) into the FK check; well-formed
//!   but unknown UUIDs fail the FK (`IntegrityError` → 400 `The payload
//!   is not valid`). Ported branch for branch.
//! * BUG-patch-attrs (`asset.py:600-619`): the generic PATCH never reads
//!   `attributes` (unlike the user PATCH), so attribute payloads are
//!   silently dropped. Ported: only `is_uploaded` is written.
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use hmac::{Hmac, Mac};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use sqlx::Row;
use uuid::Uuid;

use crate::state::AppState;
use pidash_auth::token as auth_token;
use pidash_db::v1_assets::asset_queries;

// ---------------------------------------------------------------------------
// Exact bodies (fx-h-asset-generic `error_bodies`, byte-exact)
// ---------------------------------------------------------------------------

/// GET workspace miss (`asset.py:458`).
pub const WORKSPACE_NOT_FOUND_BODY: &str = r#"{"error":"Workspace not found"}"#;
/// GET/PATCH asset miss (`asset.py:459-460,620-621`).
pub const ASSET_NOT_FOUND_BODY: &str = r#"{"error":"Asset not found"}"#;
/// GET not-uploaded guard (`asset.py:433-437`).
pub const NOT_UPLOADED_BODY: &str = r#"{"error":"Asset not yet uploaded"}"#;
/// GET blanket `except` (`asset.py:461-466`; `log_exception` + 500).
pub const GET_SERVER_ERROR_BODY: &str = r#"{"error":"Internal server error"}"#;
/// POST required check (`asset.py:511-515`).
pub const MISSING_FIELDS_BODY: &str =
    r#"{"error":"Name and size are required fields.","status":false}"#;
/// POST MIME guard (`asset.py:521-525`).
pub const BAD_TYPE_BODY: &str = r#"{"error":"Invalid file type.","status":false}"#;
/// POST workspace miss via `handle_exception` `ObjectDoesNotExist`
/// (`base.py:154-159`).
pub const POST_WORKSPACE_MISSING_BODY: &str =
    r#"{"error":"The requested resource does not exist."}"#;
/// `handle_exception` `IntegrityError` (`base.py:142-146`).
pub const PAYLOAD_NOT_VALID_BODY: &str = r#"{"error":"The payload is not valid"}"#;
/// `handle_exception` `ValidationError` (`base.py:148-152`).
pub const INVALID_DETAIL_BODY: &str = r#"{"error":"Please provide valid detail"}"#;
/// `handle_exception` generic 500 (`base.py:166-171`; `log_exception` + 500).
pub const SERVER_ERROR_BODY: &str = r#"{"error":"Something went wrong please try again later"}"#;
/// DRF `NotAuthenticated` (`IsAuthenticated` denial on every route here).
pub const UNAUTHENTICATED_BODY: &str =
    r#"{"detail":"Authentication credentials were not provided."}"#;
/// `APIKeyAuthentication` failure (invalid, revoked, expired, inactive).
pub const INVALID_TOKEN_BODY: &str = r#"{"detail":"Given API token is not valid"}"#;
/// Resolver 404 for a non-UUID `<uuid:asset_id>` segment (global
/// `handler404`, `pi_dash/urls.py:15`).
pub const PAGE_NOT_FOUND_BODY: &str = r#"{"error":"Page not found."}"#;

/// POST dedupe-hit message (`asset.py:543-545`).
pub const DEDUPE_MESSAGE: &str = "Asset with same external id and source already exists";

/// `ATTACHMENT_MIME_TYPES` (`settings/common.py:652-736`), source order.
/// The generic POST allowlist is the full settings list — not the 5-mime
/// user list (`serializers/asset.py:23-31`).
pub const ATTACHMENT_MIME_TYPES: &[&str] = &[
    "image/jpeg",
    "image/png",
    "image/gif",
    "image/svg+xml",
    "image/webp",
    "image/tiff",
    "image/bmp",
    "application/pdf",
    "application/msword",
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    "application/vnd.ms-excel",
    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
    "application/vnd.ms-powerpoint",
    "application/vnd.openxmlformats-officedocument.presentationml.presentation",
    "text/plain",
    "text/markdown",
    "application/rtf",
    "application/vnd.oasis.opendocument.spreadsheet",
    "application/vnd.oasis.opendocument.text",
    "application/vnd.oasis.opendocument.presentation",
    "application/vnd.oasis.opendocument.graphics",
    "application/vnd.visio",
    "image/x-portable-graymap",
    "image/x-portable-bitmap",
    "image/x-portable-pixmap",
    "application/vnd.oasis.opendocument.database",
    "audio/mpeg",
    "audio/wav",
    "audio/ogg",
    "audio/midi",
    "audio/x-midi",
    "audio/aac",
    "audio/flac",
    "audio/x-m4a",
    "video/mp4",
    "video/mpeg",
    "video/ogg",
    "video/webm",
    "video/quicktime",
    "video/x-msvideo",
    "video/x-ms-wmv",
    "application/zip",
    "application/x-rar",
    "application/x-rar-compressed",
    "application/x-tar",
    "application/gzip",
    "application/x-zip",
    "application/x-zip-compressed",
    "application/x-7z-compressed",
    "application/x-compressed",
    "application/x-compressed-tar",
    "application/x-compressed-tar-gz",
    "application/x-compressed-tar-bz2",
    "application/x-compressed-tar-zip",
    "application/x-compressed-tar-7z",
    "application/x-compressed-tar-rar",
    "application/x-compressed-tar-zip",
    "model/gltf-binary",
    "model/gltf+json",
    "application/octet-stream",
    "font/ttf",
    "font/otf",
    "font/woff",
    "font/woff2",
    "text/css",
    "text/javascript",
    "application/json",
    "text/xml",
    "text/csv",
    "application/xml",
    "application/x-sql",
    "application/x-gzip",
    "text/markdown",
];

/// Minted `entity_type` on the generic POST (`asset.py:558-563`): always
/// `ISSUE_ATTACHMENT` — the row is bound to issues later.
pub const GENERIC_ENTITY_TYPE: &str = "ISSUE_ATTACHMENT";

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------

/// Register the generic-asset routes. PIDASHCONV-426 merges this router
/// into the app router; on rebase keep both sides.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/workspaces/{slug}/assets/",
            post(generic_post).fallback(crate::edge::proxy),
        )
        .route(
            "/api/v1/workspaces/{slug}/assets/{asset_id}/",
            get(generic_get)
                .patch(generic_patch)
                .fallback(crate::edge::proxy),
        )
}

// ---------------------------------------------------------------------------
// Small responders (exact Django bytes)
// ---------------------------------------------------------------------------

fn raw(status: StatusCode, body: &'static str) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body))
        .expect("static response")
}

fn json_body(status: StatusCode, body: Value) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(
            serde_json::to_string(&body).expect("serializable response"),
        ))
        .expect("json response")
}

/// GET blanket `except` (`asset.py:461-466`).
fn get_server_error(error: impl std::fmt::Display) -> Response {
    tracing::warn!(%error, "generic asset GET failed");
    raw(StatusCode::INTERNAL_SERVER_ERROR, GET_SERVER_ERROR_BODY)
}

/// `handle_exception` generic 500 (`base.py:166-171`).
fn server_error(error: impl std::fmt::Display) -> Response {
    tracing::warn!(%error, "generic asset handler failed");
    raw(StatusCode::INTERNAL_SERVER_ERROR, SERVER_ERROR_BODY)
}

fn no_content() -> Response {
    StatusCode::NO_CONTENT.into_response()
}

fn pool(state: &AppState) -> Option<&sqlx::PgPool> {
    state.pools().map(|pools| pools.primary())
}

// ---------------------------------------------------------------------------
// Auth (`APIKeyAuthentication`, `api/middleware/api_authentication.py`)
// ---------------------------------------------------------------------------

/// Handler failure with its exact status + body.
#[derive(Debug)]
pub enum Denial {
    Unauthorized,
    InvalidToken,
    WorkspaceNotFound,
    AssetNotFound,
    NotUploaded,
    MissingFields,
    BadType,
    PostWorkspaceMissing,
    BadPayload,
    InvalidDetail,
    /// 400, `{"error": ...}` (`handle_exception` `KeyError` branch:
    /// unknown stored time zone).
    BadError(String),
    ServerError,
}

impl IntoResponse for Denial {
    fn into_response(self) -> Response {
        match self {
            Denial::Unauthorized => raw(StatusCode::UNAUTHORIZED, UNAUTHENTICATED_BODY),
            Denial::InvalidToken => raw(StatusCode::FORBIDDEN, INVALID_TOKEN_BODY),
            Denial::WorkspaceNotFound => raw(StatusCode::NOT_FOUND, WORKSPACE_NOT_FOUND_BODY),
            Denial::AssetNotFound => raw(StatusCode::NOT_FOUND, ASSET_NOT_FOUND_BODY),
            Denial::NotUploaded => raw(StatusCode::BAD_REQUEST, NOT_UPLOADED_BODY),
            Denial::MissingFields => raw(StatusCode::BAD_REQUEST, MISSING_FIELDS_BODY),
            Denial::BadType => raw(StatusCode::BAD_REQUEST, BAD_TYPE_BODY),
            Denial::PostWorkspaceMissing => raw(StatusCode::NOT_FOUND, POST_WORKSPACE_MISSING_BODY),
            Denial::BadPayload => raw(StatusCode::BAD_REQUEST, PAYLOAD_NOT_VALID_BODY),
            Denial::InvalidDetail => raw(StatusCode::BAD_REQUEST, INVALID_DETAIL_BODY),
            Denial::BadError(message) => json_body(
                StatusCode::BAD_REQUEST,
                serde_json::json!({"error": message}),
            ),
            Denial::ServerError => raw(StatusCode::INTERNAL_SERVER_ERROR, SERVER_ERROR_BODY),
        }
    }
}

/// The authenticated actor: the API token's user id
/// (`validate_api_token` / `validate_machine_token`) plus the RAW stored
/// time-zone name. The zone is NOT parsed here — `TimezoneMixin.initial`
/// (`api/views/base.py:43-48`) activates it after authentication, so a
/// bad zone 400s (an empty one 500s) via [`activate_timezone`], never
/// during auth.
#[derive(Debug, Clone)]
pub struct Actor {
    pub id: Uuid,
    pub timezone: Option<String>,
}

/// Authenticate one request from its `X-Api-Key` header.
///
/// * Missing/empty → 401 (every route here requires `IsAuthenticated`).
/// * `mt_` prefix → machine-token path; anything else → `api_tokens`
///   lookup (exact match, `is_active`, `expired_at` null or strictly
///   future; `last_used` bumped).
/// * Every other failure → 403 `Given API token is not valid`.
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
    Ok(Actor { id, timezone })
}

/// The request time zone (`TimezoneMixin.initial`): the user's stored zone
/// name, loaded but NOT parsed — parsing happens after authentication in
/// [`activate_timezone`].
async fn load_timezone_name(pool: &sqlx::PgPool, user_id: &Uuid) -> Result<Option<String>, Denial> {
    let zone: Option<(Option<String>,)> =
        sqlx::query_as("SELECT user_timezone FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_optional(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    Ok(zone.and_then(|row| row.0))
}

/// Activate the actor's rendering timezone (`TimezoneMixin.initial` runs
/// after authentication; these routes carry no further gate). A missing
/// zone defaults to UTC; an unknown zone name 400s:
/// `zoneinfo.ZoneInfo` raises `ZoneInfoNotFoundError`, which subclasses
/// `KeyError`, so `handle_exception` answers the `KeyError` branch
/// (`api/views/base.py:160-164`). An EMPTY zone 500s: `ZoneInfo('')`
/// raises `ValueError` (not `KeyError`), which falls through to the
/// generic 500 (`api/views/base.py:166-171`).
fn activate_timezone(timezone: Option<&str>) -> Result<Tz, Denial> {
    match timezone {
        None => Ok(chrono_tz::UTC),
        Some("") => Err(Denial::ServerError),
        Some(zone) => zone
            .parse::<Tz>()
            .map_err(|_| Denial::BadError("The required key does not exist.".to_owned())),
    }
}

async fn authenticate_api_token(pool: &sqlx::PgPool, presented: &str) -> Result<Uuid, Denial> {
    // `deleted_at IS NULL` is the `SoftDeletionManager` scope
    // (`db/mixins.py:56-66`): `APIToken.objects` never sees soft-deleted
    // rows, so a soft-deleted token 403s instead of authenticating.
    let row: Option<(Uuid, bool, Option<DateTime<Utc>>)> =
        sqlx::query_as("SELECT user_id, is_active, expired_at FROM api_tokens WHERE token = $1 AND deleted_at IS NULL")
            .bind(presented)
            .fetch_optional(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    let Some((user_id, is_active, expired_at)) = row else {
        return Err(Denial::InvalidToken);
    };
    let now = Utc::now();
    let kernel_row = auth_token::ApiTokenRow {
        token: presented.to_owned(),
        is_active,
        expired_at_unix: expired_at.map(|dt| dt.timestamp()),
    };
    // Exact bytes again (constant-time) plus the `expired_at__gt=now`
    // predicate — strictly greater, null never expires.
    auth_token::validate_api_token(Some(&kernel_row), presented, now.timestamp())
        .map_err(|_| Denial::InvalidToken)?;
    // `api_token.last_used = now; save(update_fields=["last_used"])`.
    sqlx::query("UPDATE api_tokens SET last_used = now() WHERE token = $1")
        .bind(presented)
        .execute(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(user_id)
}

async fn authenticate_machine_token(
    pool: &sqlx::PgPool,
    presented: &str,
    secret_key: &[u8],
) -> Result<Uuid, Denial> {
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
    auth_token::validate_machine_token_static(Some(&kernel_row), &token_hash)
        .map_err(|_| Denial::InvalidToken)?;
    // `is_workspace_member(user, workspace_id)`: a non-member is revoked
    // first, then denied — exactly like Python.
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
        sqlx::query("UPDATE machine_token SET revoked_at = now() WHERE id = $1")
            .bind(row.id)
            .execute(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
        return Err(Denial::InvalidToken);
    }
    sqlx::query("UPDATE machine_token SET last_used_at = now() WHERE id = $1")
        .bind(row.id)
        .execute(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(row.user_id)
}

#[derive(Debug, sqlx::FromRow)]
struct MachineTokenLookup {
    id: Uuid,
    user_id: Uuid,
    workspace_id: Uuid,
    revoked_at: Option<DateTime<Utc>>,
    dev_machine_id: Option<Uuid>,
    dev_revoked_at: Option<DateTime<Utc>>,
}

// ---------------------------------------------------------------------------
// Request-data parsing (JSON; empty/malformed proxy to Django)
// ---------------------------------------------------------------------------

/// Parse `request.data` for the JSON bodies the contract suites send.
///
/// A well-formed JSON object parses as-is. Anything else keeps Django's
/// exact bytes: an empty or malformed body is proxied to Django (DRF
/// `ParseError`), while a well-formed non-object body (list/scalar)
/// answers the generic 500 — the `AttributeError` the view's `.get`
/// raises on it (`asset.py` post/patch read `request.data.get(...)`
/// before anything else validates).
#[allow(clippy::result_large_err)]
fn parse_object(body: &Bytes) -> Result<Map<String, Value>, ProxyOr500> {
    if body.is_empty() {
        return Err(ProxyOr500::Proxy);
    }
    match serde_json::from_slice::<Value>(body) {
        Ok(Value::Object(map)) => Ok(map),
        Ok(_) => Err(ProxyOr500::ServerError),
        Err(_) => Err(ProxyOr500::Proxy),
    }
}

enum ProxyOr500 {
    Proxy,
    ServerError,
}

/// Rebuild a request from its parts and proxy it to Django.
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

// ---------------------------------------------------------------------------
// Python-value kernels (`request.data.get(...)` semantics)
// ---------------------------------------------------------------------------

/// Python truthiness of a parsed JSON value, for guards written as
/// `if not name` (`asset.py:511`): missing keys read as `None`
/// (falsy); empty string, false, zero and empty containers are falsy.
fn is_truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(flag)) => *flag,
        Some(Value::Number(n)) => {
            n.as_i64().is_some_and(|i| i != 0)
                || n.as_u64().is_some_and(|u| u != 0)
                || n.as_f64().is_some_and(|f| f != 0.0)
        }
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(items)) => !items.is_empty(),
        Some(Value::Object(map)) => !map.is_empty(),
    }
}

/// `str(value)` for the f-string renderings (`asset.py:531` asset key,
/// `project_id`/`issue_id` in `asset_url`). Scalars match CPython
/// exactly (`True`/`False`/`None`, ints, float shortest roundtrip);
/// containers use CPython `repr` spelling (single quotes).
fn python_str(value: &Value) -> String {
    match value {
        Value::Null => "None".to_owned(),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        Value::Array(items) => format!(
            "[{}]",
            items.iter().map(python_repr).collect::<Vec<_>>().join(", ")
        ),
        Value::Object(map) => format!(
            "{{{}}}",
            map.iter()
                .map(|(k, v)| format!(
                    "{}: {}",
                    python_repr(&Value::String(k.clone())),
                    python_repr(v)
                ))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

/// CPython `repr` of one JSON scalar (single-quoted strings).
fn python_repr(value: &Value) -> String {
    match value {
        Value::String(s) => format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'")),
        _ => python_str(value),
    }
}

/// Outcome of the POST `size = int(request.data.get("size", LIMIT))`
/// conversion (`asset.py:505`): a parsed byte count, or the
/// `TypeError`/`ValueError` 500 the conversion raises for explicit
/// nulls and non-numeric input (BUG-size-null, ported as-is).
enum SizeOutcome {
    Size(i64),
    ConvertError,
}

/// `int()` over an already-parsed JSON value (`asset.py:505`).
/// Missing keys read the `FILE_SIZE_LIMIT` default; floats truncate
/// toward zero like CPython `int()`; numeric strings take an optional
/// sign plus ASCII digits (a `str` underscore or decimal point raises,
/// exactly like CPython).
fn python_int_size(value: Option<&Value>, default: i64) -> SizeOutcome {
    let Some(value) = value else {
        return SizeOutcome::Size(default);
    };
    match value {
        Value::Null => SizeOutcome::ConvertError,
        Value::Bool(flag) => SizeOutcome::Size(i64::from(*flag)),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                SizeOutcome::Size(i)
            } else if let Some(u) = n.as_u64() {
                i64::try_from(u).map_or(SizeOutcome::ConvertError, SizeOutcome::Size)
            } else if let Some(f) = n.as_f64() {
                // CPython `int()` truncates; out-of-range floats raise.
                if f.is_finite() && f >= i64::MIN as f64 && f <= i64::MAX as f64 {
                    #[allow(clippy::cast_possible_truncation)]
                    SizeOutcome::Size(f.trunc() as i64)
                } else {
                    SizeOutcome::ConvertError
                }
            } else {
                SizeOutcome::ConvertError
            }
        }
        Value::String(s) => {
            // CPython `int(str)`: surrounding whitespace, one sign, then
            // digits with single `_` separators; unbounded width (the
            // column clamps via `min()` later, so overflow saturates).
            let text = s.trim();
            let (negative, digits) = match text.strip_prefix('+') {
                Some(rest) => (false, rest),
                None => match text.strip_prefix('-') {
                    Some(rest) => (true, rest),
                    None => (false, text),
                },
            };
            let clean: String = digits.split('_').collect::<Vec<_>>().join("");
            let well_formed = !digits.is_empty()
                && !digits.starts_with('_')
                && !digits.ends_with('_')
                && !digits.contains("__")
                && !clean.is_empty()
                && clean.bytes().all(|b| b.is_ascii_digit());
            if !well_formed {
                return SizeOutcome::ConvertError;
            }
            match clean.parse::<i64>() {
                Ok(size) => SizeOutcome::Size(if negative { -size } else { size }),
                Err(_) if !negative => SizeOutcome::Size(i64::MAX),
                Err(_) => SizeOutcome::Size(i64::MIN),
            }
        }
        Value::Array(_) | Value::Object(_) => SizeOutcome::ConvertError,
    }
}

/// Coerce an open string column the way `objects.create` stores it:
/// strings as-is, numbers/bools via `str()`, missing/null as SQL NULL.
/// Containers have no psycopg2 adaptation (`ProgrammingError` → the
/// generic 500), so they convert-error here.
fn optional_text(value: Option<&Value>) -> Result<Option<String>, ()> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(Value::Bool(_)) | Some(Value::Number(_)) => {
            Ok(Some(python_str(value.expect("matched"))))
        }
        Some(Value::Array(_)) | Some(Value::Object(_)) => Err(()),
    }
}

/// `BooleanField.get_prep_value` for the PATCH `is_uploaded` assignment
/// (`fields/__init__.py:1118-1122` + `to_python`, `:1102-1114`):
/// `None` passes through to the `NOT NULL` column (`IntegrityError` →
/// 400); `True`/`False` — including `1`/`0`/`1.0`/`0.0` by `==` — and
/// exactly `"t"`/`"True"`/`"1"` / `"f"`/`"False"`/`"0"` convert;
/// everything else (notably `"true"`, `"yes"`, `"on"`, `2`, `[]`)
/// raises `ValidationError` (400 `Please provide valid detail`).
/// Missing keys keep the current row value (`asset.py:611`).
#[derive(Debug)]
enum UploadedOutcome {
    Keep,
    Set(bool),
    NullViolation,
    Invalid,
}

fn patch_is_uploaded(value: Option<&Value>) -> UploadedOutcome {
    let Some(value) = value else {
        return UploadedOutcome::Keep;
    };
    match value {
        Value::Null => UploadedOutcome::NullViolation,
        Value::Bool(flag) => UploadedOutcome::Set(*flag),
        Value::Number(n) => {
            // `value in (True, False)` — numeric equality, any width.
            let one = n.as_i64().is_some_and(|i| i == 1)
                || n.as_u64().is_some_and(|u| u == 1)
                || n.as_f64().is_some_and(|f| f == 1.0);
            let zero = n.as_i64().is_some_and(|i| i == 0)
                || n.as_u64().is_some_and(|u| u == 0)
                || n.as_f64().is_some_and(|f| f == 0.0);
            if one {
                UploadedOutcome::Set(true)
            } else if zero {
                UploadedOutcome::Set(false)
            } else {
                UploadedOutcome::Invalid
            }
        }
        Value::String(s) => match s.as_str() {
            "t" | "True" | "1" => UploadedOutcome::Set(true),
            "f" | "False" | "0" => UploadedOutcome::Set(false),
            _ => UploadedOutcome::Invalid,
        },
        Value::Array(_) | Value::Object(_) => UploadedOutcome::Invalid,
    }
}

/// `UUIDField.to_python` (`fields/__init__.py:2684-2695`) plus the
/// `ForeignObject.get_db_prep_save` empty-string rewrite
/// (`related.py:1119-1129`): missing/null stay NULL; `""` rewrites to
/// NULL (the 200 empty-string row); valid UUID strings bind (unknown
/// ones fail the FK → 400); garbage strings, floats and containers
/// raise `ValidationError` (400 `Please provide valid detail`); ints —
/// bools included, `isinstance(True, int)` — ride `uuid.UUID(int=…)`
/// into the FK check (negatives raise, like CPython).
#[derive(Debug)]
enum ProjectOutcome {
    Null,
    Bind(Uuid),
    Invalid,
}

fn patch_project_id(value: Option<&Value>) -> ProjectOutcome {
    match value {
        None | Some(Value::Null) => ProjectOutcome::Null,
        Some(Value::String(s)) if s.is_empty() => ProjectOutcome::Null,
        Some(Value::String(s)) => match s.parse() {
            Ok(id) => ProjectOutcome::Bind(id),
            Err(_) => ProjectOutcome::Invalid,
        },
        Some(Value::Bool(flag)) => ProjectOutcome::Bind(Uuid::from_u128(u128::from(*flag))),
        Some(Value::Number(n)) => {
            if let Some(i) = n.as_i64() {
                u128::try_from(i).map_or(ProjectOutcome::Invalid, |v| {
                    ProjectOutcome::Bind(Uuid::from_u128(v))
                })
            } else if let Some(u) = n.as_u64() {
                ProjectOutcome::Bind(Uuid::from_u128(u128::from(u)))
            } else {
                ProjectOutcome::Invalid
            }
        }
        Some(Value::Array(_)) | Some(Value::Object(_)) => ProjectOutcome::Invalid,
    }
}

// ---------------------------------------------------------------------------
// Shared lookups
// ---------------------------------------------------------------------------

/// `Workspace.objects.get(slug=slug)` (`asset.py:428,532`): the manager
/// read scopes live rows, so a soft-deleted workspace reads as missing —
/// exactly what both units need. Returns the workspace id.
async fn workspace_id(pool: &sqlx::PgPool, slug: &str) -> Result<Option<Uuid>, sqlx::Error> {
    let row: Option<(Uuid,)> =
        sqlx::query_as("SELECT id FROM workspaces WHERE slug = $1 AND deleted_at IS NULL")
            .bind(slug)
            .fetch_optional(pool)
            .await?;
    Ok(row.map(|row| row.0))
}

fn host_of(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
}

/// Request scheme for MinIO-mode signing: `X-Forwarded-Proto` when the
/// proxy sets it, else `http` (Django's `request.scheme` default).
fn scheme_of(headers: &HeaderMap) -> String {
    headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .map(|v| v.trim().to_lowercase())
        .filter(|v| v == "http" || v == "https")
        .unwrap_or_else(|| "http".to_owned())
}

// ---------------------------------------------------------------------------
// Rows
// ---------------------------------------------------------------------------

/// One `file_assets` row with every column the three units read.
struct AssetRow {
    id: Uuid,
    entity_type: Option<String>,
    is_uploaded: bool,
    storage_metadata: Option<Value>,
    attributes: Option<Value>,
    asset_key: String,
    project_id: Option<Uuid>,
    issue_id: Option<Uuid>,
}

fn asset_row(row: &sqlx::postgres::PgRow) -> Result<AssetRow, sqlx::Error> {
    Ok(AssetRow {
        id: row.try_get("id")?,
        entity_type: row.try_get("entity_type")?,
        is_uploaded: row.try_get("is_uploaded")?,
        storage_metadata: row.try_get("storage_metadata")?,
        attributes: row.try_get("attributes")?,
        asset_key: row.try_get("asset")?,
        project_id: row.try_get("project_id")?,
        issue_id: row.try_get("issue_id")?,
    })
}

/// `FileAsset.asset_url` (`db/models/asset.py:80-100`) for the property
/// responses (POST 200/409): the static branch, the `ISSUE_ATTACHMENT`
/// branch (UUIDs render dashed, `None` renders `None` — the f-string),
/// the description branches, else null. `slug` is the row's workspace
/// slug (the lookups scope it, so the path slug is the row's slug).
fn file_asset_url(
    entity_type: Option<&str>,
    slug: &str,
    project_id: Option<Uuid>,
    issue_id: Option<Uuid>,
    asset_id: &Uuid,
) -> Value {
    match entity_type {
        Some("WORKSPACE_LOGO" | "USER_AVATAR" | "USER_COVER" | "PROJECT_COVER") => {
            Value::String(format!("/api/assets/v2/static/{asset_id}/"))
        }
        Some("ISSUE_ATTACHMENT") => {
            let project = project_id.map_or_else(|| "None".to_owned(), |id| id.to_string());
            let issue = issue_id.map_or_else(|| "None".to_owned(), |id| id.to_string());
            Value::String(format!(
                "/api/assets/v2/workspaces/{slug}/projects/{project}/issues/{issue}/attachments/{asset_id}/"
            ))
        }
        Some(
            "ISSUE_DESCRIPTION"
            | "COMMENT_DESCRIPTION"
            | "PAGE_DESCRIPTION"
            | "DRAFT_ISSUE_DESCRIPTION",
        ) => {
            let project = project_id.map_or_else(|| "None".to_owned(), |id| id.to_string());
            Value::String(format!(
                "/api/assets/v2/workspaces/{slug}/projects/{project}/{asset_id}/"
            ))
        }
        _ => Value::Null,
    }
}

// ---------------------------------------------------------------------------
// Metadata publisher
// ---------------------------------------------------------------------------

/// `get_asset_object_metadata.delay(asset_id=str(asset_id))`
/// (`asset.py:614-615`, kwarg form) through the Postgres-backed queue.
/// Best-effort: a failed enqueue warns and the response stands (the
/// merged D-31 precedent).
async fn publish_metadata(pool: &sqlx::PgPool, asset_id: &Uuid) {
    let job = pidash_jobs::v1_assets::tasks::metadata_job(&asset_id.to_string());
    if let Err(error) = pidash_jobs::queue::enqueue(pool, &job).await {
        tracing::warn!(%error, task = "pi_dash.bgtasks.storage_metadata_task.get_asset_object_metadata", "task enqueue failed; response stands");
    }
}

// ---------------------------------------------------------------------------
// GET `workspaces/<slug>/assets/<uuid:asset_id>/` (`asset.py:419-466`)
// ---------------------------------------------------------------------------

/// Get the presigned download URL for a generic asset. The asset must be
/// uploaded and scoped to the workspace; `asset_url` here is the
/// presigned GET — not the model `asset_url` property.
async fn generic_get(
    State(state): State<AppState>,
    Path((slug, asset_raw)): Path<(String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let _ = body;
    let asset_id: Uuid = match asset_raw.parse() {
        Ok(id) => id,
        Err(_) => return raw(StatusCode::NOT_FOUND, PAGE_NOT_FOUND_BODY),
    };
    let Some(pool) = pool(&state) else {
        return get_server_error("no database pool");
    };
    let actor = match authenticate(pool, &headers, state.settings().secret_key.as_bytes()).await {
        Ok(actor) => actor,
        Err(denial) => return denial.into_response(),
    };
    // Auth passed: activate the stored zone now (`TimezoneMixin.initial`
    // runs after authentication; an unknown zone 400s and an empty one
    // 500s before the workspace lookup). Validation-only: this response
    // renders no datetimes.
    if let Err(denial) = activate_timezone(actor.timezone.as_deref()) {
        return denial.into_response();
    }
    let _ = actor;
    // `workspace = Workspace.objects.get(slug=slug)` → 404 first.
    let workspace = match workspace_id(pool, &slug).await {
        Ok(Some(id)) => id,
        Ok(None) => return Denial::WorkspaceNotFound.into_response(),
        Err(error) => return get_server_error(error),
    };
    let _ = workspace;
    // `asset = FileAsset.objects.get(id, workspace_id, is_deleted=False)`.
    let row = match sqlx::query(&asset_queries::generic_detail_select_sql())
        .bind(asset_id)
        .bind(&slug)
        .fetch_optional(pool)
        .await
    {
        Ok(row) => row,
        Err(error) => return get_server_error(error),
    };
    let Some(row) = row else {
        return Denial::AssetNotFound.into_response();
    };
    let asset = match asset_row(&row) {
        Ok(asset) => asset,
        Err(error) => return get_server_error(error),
    };
    if !asset.is_uploaded {
        return Denial::NotUploaded.into_response();
    }
    // `attributes.get("name")` feeds the filename; a non-object (or a
    // non-string name) raises into the blanket 500 exactly like Python.
    let filename = match asset.attributes.as_ref() {
        Some(Value::Object(map)) => match map.get("name") {
            None | Some(Value::Null) => None,
            Some(Value::String(name)) => Some(name.clone()),
            Some(_) => {
                return get_server_error("attributes.name is not a string");
            }
        },
        _ => {
            return get_server_error("attributes is not an object");
        }
    };
    let attributes = asset
        .attributes
        .as_ref()
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let asset_name = attributes
        .get("name")
        .cloned()
        .unwrap_or(Value::String(String::new()));
    let asset_type = attributes
        .get("type")
        .cloned()
        .unwrap_or(Value::String(String::new()));
    let Some(host) = host_of(&headers) else {
        return get_server_error("missing Host");
    };
    let storage = &state.settings().storage;
    let url = presigned_get_url(
        storage,
        &scheme_of(&headers),
        &host,
        &asset.asset_key,
        "inline",
        filename,
        &Utc::now(),
    );
    json_body(
        StatusCode::OK,
        serde_json::json!({
            "asset_id": asset.id.to_string(),
            "asset_url": url,
            "asset_name": asset_name,
            "asset_type": asset_type,
        }),
    )
}

// ---------------------------------------------------------------------------
// POST `workspaces/<slug>/assets/` (`asset.py:497-578`)
// ---------------------------------------------------------------------------

/// Mint a generic asset row and answer the presigned upload POST.
#[allow(clippy::too_many_lines)]
async fn generic_post(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let Some(pool) = pool(&state) else {
        return server_error("no database pool");
    };
    let actor = match authenticate(pool, &headers, state.settings().secret_key.as_bytes()).await {
        Ok(actor) => actor,
        Err(denial) => return denial.into_response(),
    };
    // Auth passed: activate the stored zone now (`TimezoneMixin.initial`
    // runs after authentication, before `request.data` is touched; an
    // unknown zone 400s and an empty one 500s). Validation-only: this
    // response renders no datetimes.
    if let Err(denial) = activate_timezone(actor.timezone.as_deref()) {
        return denial.into_response();
    }
    let data = match parse_object(&body) {
        Ok(data) => data,
        Err(ProxyOr500::Proxy) => {
            return proxy_through(state, method, uri, headers, body).await;
        }
        Err(ProxyOr500::ServerError) => return server_error("request.data has no .get"),
    };
    // `size = int(request.data.get("size", FILE_SIZE_LIMIT))` runs BEFORE
    // the required check (BUG-size-null): null/non-numeric sizes 500.
    let size = match python_int_size(data.get("size"), state.settings().file_size_limit) {
        SizeOutcome::Size(size) => size,
        SizeOutcome::ConvertError => return server_error("int(size) raised"),
    };
    // `if not name or not size` → 400.
    if !is_truthy(data.get("name")) || size == 0 {
        return Denial::MissingFields.into_response();
    }
    let size_limit = size.min(state.settings().file_size_limit);
    // `if not type or type not in ATTACHMENT_MIME_TYPES` → 400. The
    // membership test needs an exact string, so every non-string 400s.
    let mime: &str = match data.get("type") {
        Some(Value::String(mime)) if ATTACHMENT_MIME_TYPES.contains(&mime.as_str()) => mime,
        _ => return Denial::BadType.into_response(),
    };
    // `workspace = Workspace.objects.get(slug=slug)` → the
    // `handle_exception` 404 body (BUG-workspace-body).
    let workspace = match workspace_id(pool, &slug).await {
        Ok(Some(id)) => id,
        Ok(None) => return Denial::PostWorkspaceMissing.into_response(),
        Err(error) => return server_error(error),
    };
    let name = python_str(data.get("name").expect("required check passed"));
    // `asset_key = f"{workspace.id}/{uuid4hex}-{name}"`.
    let asset_key = format!("{workspace}/{}-{name}", Uuid::new_v4().simple());
    // External dedupe, only when both halves are truthy (`asset.py:534`).
    if is_truthy(data.get("external_id")) && is_truthy(data.get("external_source")) {
        let external_id = python_str(data.get("external_id").expect("truthy"));
        let external_source = python_str(data.get("external_source").expect("truthy"));
        let existing = match sqlx::query(&asset_queries::external_dedupe_select_sql())
            .bind(&external_id)
            .bind(&external_source)
            .bind(&slug)
            .fetch_optional(pool)
            .await
        {
            Ok(row) => row,
            Err(error) => return server_error(error),
        };
        if let Some(row) = existing {
            let asset = match asset_row(&row) {
                Ok(asset) => asset,
                Err(error) => return server_error(error),
            };
            return json_body(
                StatusCode::CONFLICT,
                serde_json::json!({
                    "message": DEDUPE_MESSAGE,
                    "asset_id": asset.id.to_string(),
                    "asset_url": file_asset_url(
                        asset.entity_type.as_deref(),
                        &slug,
                        asset.project_id,
                        asset.issue_id,
                        &asset.id,
                    ),
                }),
            );
        }
    }
    // `project_id` is stored raw (BUG-project-raw): null/missing and
    // `""` stay NULL; valid UUID strings bind (unknown ones fail the FK
    // → 400); garbage strings, floats and containers raise
    // `ValidationError` → 400; ints ride `uuid.UUID(int=…)`.
    let project_id: Option<Uuid> = match patch_project_id(data.get("project_id")) {
        ProjectOutcome::Null => None,
        ProjectOutcome::Bind(id) => Some(id),
        ProjectOutcome::Invalid => return Denial::InvalidDetail.into_response(),
    };
    let external_id = match optional_text(data.get("external_id")) {
        Ok(value) => value,
        Err(()) => return server_error("external_id has no adapter"),
    };
    let external_source = match optional_text(data.get("external_source")) {
        Ok(value) => value,
        Err(()) => return server_error("external_source has no adapter"),
    };
    let attributes = serde_json::json!({
        "name": data.get("name").cloned().unwrap_or(Value::Null),
        "type": mime,
        "size": size_limit,
    });
    let asset_id = Uuid::new_v4();
    let now = Utc::now();
    // `FileAsset.objects.create(...)` (`asset.py:551-563`).
    let inserted = sqlx::query(
        r#"INSERT INTO "file_assets" ("id", "created_at", "updated_at", "created_by_id", "attributes", "asset", "size", "workspace_id", "project_id", "external_id", "external_source", "entity_type", "is_deleted", "is_archived", "is_uploaded", "storage_metadata") VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, false, false, false, '{}')"#,
    )
    .bind(asset_id)
    .bind(now)
    .bind(now)
    .bind(actor.id)
    .bind(sqlx::types::Json(attributes))
    .bind(&asset_key)
    .bind(size_limit as f64)
    .bind(workspace)
    .bind(project_id)
    .bind(external_id)
    .bind(external_source)
    .bind(GENERIC_ENTITY_TYPE)
    .execute(pool)
    .await;
    if let Err(error) = inserted {
        // `handle_exception` (`base.py:142-146`): Django raises
        // `IntegrityError` for SQLSTATE class 23 (unknown-UUID FKs,
        // NOT NULL, check violations) → 400; every other column error
        // (class 22 casts, overlong keys, connection loss) → generic 500.
        let integrity = matches!(&error, sqlx::Error::Database(db) if db.code().is_some_and(|code| code.starts_with("23")));
        if integrity {
            return Denial::BadPayload.into_response();
        }
        return server_error(error);
    }
    let Some(host) = host_of(&headers) else {
        return server_error("missing Host");
    };
    let storage = &state.settings().storage;
    let upload_data = presigned_post(
        storage,
        &scheme_of(&headers),
        &host,
        &asset_key,
        mime,
        size_limit,
        &now,
    );
    json_body(
        StatusCode::OK,
        serde_json::json!({
            "upload_data": upload_data,
            "asset_id": asset_id.to_string(),
            "asset_url": file_asset_url(
                Some(GENERIC_ENTITY_TYPE),
                &slug,
                project_id,
                None,
                &asset_id,
            ),
        }),
    )
}

// ---------------------------------------------------------------------------
// PATCH `workspaces/<slug>/assets/<uuid:asset_id>/` (`asset.py:600-621`)
// ---------------------------------------------------------------------------

/// Mark the asset uploaded and fire metadata extraction. `attributes`
/// payloads are ignored (BUG-patch-attrs); a missing `is_uploaded`
/// keeps the current row value.
async fn generic_patch(
    State(state): State<AppState>,
    Path((slug, asset_raw)): Path<(String, String)>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let asset_id: Uuid = match asset_raw.parse() {
        Ok(id) => id,
        Err(_) => return raw(StatusCode::NOT_FOUND, PAGE_NOT_FOUND_BODY),
    };
    let Some(pool) = pool(&state) else {
        return server_error("no database pool");
    };
    let _actor = match authenticate(pool, &headers, state.settings().secret_key.as_bytes()).await {
        Ok(actor) => actor,
        Err(denial) => return denial.into_response(),
    };
    // Auth passed: activate the stored zone now (`TimezoneMixin.initial`
    // runs after authentication; an unknown zone 400s and an empty one
    // 500s before the asset lookup). Validation-only: this response
    // renders no datetimes.
    if let Err(denial) = activate_timezone(_actor.timezone.as_deref()) {
        return denial.into_response();
    }
    // The lookup runs BEFORE `request.data` is touched (DRF parses
    // lazily): a missing asset 404s even with a malformed body.
    // `FileAsset.objects.get(id, workspace__slug, is_deleted=False)`.
    let row = match sqlx::query(&asset_queries::generic_detail_select_sql())
        .bind(asset_id)
        .bind(&slug)
        .fetch_optional(pool)
        .await
    {
        Ok(row) => row,
        Err(error) => return server_error(error),
    };
    let Some(row) = row else {
        return Denial::AssetNotFound.into_response();
    };
    // `request.data` parses here — after the lookup, like Python.
    let data = match parse_object(&body) {
        Ok(data) => data,
        Err(ProxyOr500::Proxy) => {
            return proxy_through(state, method, uri, headers, body).await;
        }
        Err(ProxyOr500::ServerError) => return server_error("request.data has no .get"),
    };
    let asset = match asset_row(&row) {
        Ok(asset) => asset,
        Err(error) => return server_error(error),
    };
    // `asset.is_uploaded = request.data.get("is_uploaded", current)`.
    let is_uploaded = match patch_is_uploaded(data.get("is_uploaded")) {
        UploadedOutcome::Keep => asset.is_uploaded,
        UploadedOutcome::Set(flag) => flag,
        // Explicit null violates the `NOT NULL` column → 400.
        UploadedOutcome::NullViolation => {
            return Denial::BadPayload.into_response();
        }
        // `BooleanField.to_python` rejection → 400.
        UploadedOutcome::Invalid => {
            return Denial::InvalidDetail.into_response();
        }
    };
    // `if not asset.storage_metadata: delay` — before the save, exactly
    // like Python (`asset.py:614-617`).
    let storage_metadata = asset.storage_metadata.unwrap_or(Value::Null);
    if pidash_jobs::v1_assets::tasks::storage_metadata_missing(&storage_metadata) {
        publish_metadata(pool, &asset_id).await;
    }
    // `asset.save(update_fields=["is_uploaded"])` (plus the `auto_now`
    // `updated_at` Django always writes).
    if let Err(error) = sqlx::query(
        r#"UPDATE "file_assets" SET "is_uploaded" = $1, "updated_at" = now() WHERE "id" = $2"#,
    )
    .bind(is_uploaded)
    .bind(asset_id)
    .execute(pool)
    .await
    {
        return server_error(error);
    }
    no_content()
}

// ---------------------------------------------------------------------------
// SigV4 presigning (offline; mirrors `S3Storage` + botocore)
// ---------------------------------------------------------------------------

/// RFC 3986 percent-encoding for SigV4 (unreserved marks stay bare,
/// everything else `%XX` uppercase — botocore's `quote(..., safe='-_.~')`).
fn uri_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for b in input.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => {
                out.push('%');
                out.push(
                    char::from_digit((b >> 4) as u32, 16)
                        .expect("hex")
                        .to_ascii_uppercase(),
                );
                out.push(
                    char::from_digit((b & 0x0f) as u32, 16)
                        .expect("hex")
                        .to_ascii_uppercase(),
                );
            }
        }
    }
    out
}

/// `urllib.parse.quote(filename)` (default `safe='/'`): like
/// [`uri_encode`] but `/` survives (filenames with slashes keep them,
/// exactly like `_get_content_disposition` over `quote`).
fn quote_filename(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for b in input.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                out.push(b as char);
            }
            _ => {
                out.push('%');
                out.push(
                    char::from_digit((b >> 4) as u32, 16)
                        .expect("hex")
                        .to_ascii_uppercase(),
                );
                out.push(
                    char::from_digit((b & 0x0f) as u32, 16)
                        .expect("hex")
                        .to_ascii_uppercase(),
                );
            }
        }
    }
    out
}

fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

type HmacSha256 = Hmac<Sha256>;

fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC-SHA256 accepts any key length");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

fn sha256_hex(data: &[u8]) -> String {
    hex(&Sha256::digest(data))
}

/// SigV4 signing key: `kDate/kRegion/kService/kSigning`
/// (`storage.py` always signs `s3`).
fn signing_key(secret: &str, date: &str, region: &str) -> Vec<u8> {
    let k_date = hmac_sha256(format!("AWS4{secret}").as_bytes(), date.as_bytes());
    let k_region = hmac_sha256(&k_date, region.as_bytes());
    let k_service = hmac_sha256(&k_region, b"s3");
    hmac_sha256(&k_service, b"aws4_request")
}

fn credential_scope(date: &str, region: &str) -> String {
    format!("{date}/{region}/s3/aws4_request")
}

/// Endpoint + host/path split for signing, mirroring
/// `S3Storage.__init__` with a request (`is_server=False` — the
/// generic endpoint never passes `is_server=True`): MinIO mode signs
/// `{scheme}://{Host}` path-style; an explicit endpoint URL signs
/// path-style against it; otherwise the virtual-hosted AWS default.
fn endpoint_parts(
    storage: &pidash_db::config::StorageSettings,
    scheme: &str,
    host: &str,
) -> (String, String) {
    if storage.use_minio {
        (format!("{scheme}://{host}"), host.to_owned())
    } else if let Some(endpoint) = storage.endpoint_url.as_deref().filter(|e| !e.is_empty()) {
        let endpoint = endpoint.trim_end_matches('/');
        let signed_host = endpoint
            .rsplit("://")
            .next()
            .unwrap_or(endpoint)
            .split('/')
            .next()
            .unwrap_or(endpoint);
        (endpoint.to_owned(), signed_host.to_owned())
    } else {
        let region = storage.region.as_str();
        let base = if region.is_empty() {
            "s3.amazonaws.com".to_owned()
        } else {
            format!("s3.{region}.amazonaws.com")
        };
        (
            format!("https://{}.{}", storage.bucket_name, base),
            format!("{}.{}", storage.bucket_name, base),
        )
    }
}

/// `generate_presigned_url(object_name, disposition, filename)`
/// (`storage.py`): presigned GET with
/// `response-content-disposition=<disposition>; filename*=UTF-8''<name>`.
/// `filename=None` mints a fresh uuid4 hex per call
/// (`_get_content_disposition(disposition, None)`).
fn presigned_get_url(
    storage: &pidash_db::config::StorageSettings,
    scheme: &str,
    host: &str,
    object_name: &str,
    disposition: &str,
    filename: Option<String>,
    now: &DateTime<Utc>,
) -> String {
    let region = storage.region.as_str();
    let (endpoint, signed_host) = endpoint_parts(storage, scheme, host);
    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let date = now.format("%Y%m%d").to_string();
    let scope = credential_scope(&date, region);
    let credential = format!("{}/{}", storage.access_key_id, scope);
    let name = match filename {
        Some(name) => quote_filename(&name),
        None => Uuid::new_v4().simple().to_string(),
    };
    let content_disposition = format!("{disposition}; filename*=UTF-8''{name}");
    // botocore renders the URL in insertion order (the response override
    // first, then the signer params) while the SigV4 canonical string
    // sorts them — verified against live `generate_presigned_url`.
    let params = [
        (
            "response-content-disposition".to_owned(),
            content_disposition,
        ),
        ("X-Amz-Algorithm".to_owned(), "AWS4-HMAC-SHA256".to_owned()),
        ("X-Amz-Credential".to_owned(), credential),
        ("X-Amz-Date".to_owned(), amz_date),
        (
            "X-Amz-Expires".to_owned(),
            storage.signed_url_expiration_secs.to_string(),
        ),
        ("X-Amz-SignedHeaders".to_owned(), "host".to_owned()),
    ];
    let render_query = params
        .iter()
        .map(|(k, v)| format!("{}={}", uri_encode(k), uri_encode(v)))
        .collect::<Vec<_>>()
        .join("&");
    let mut signed = params.to_vec();
    signed.sort_by(|a, b| a.0.cmp(&b.0));
    let canonical_query = signed
        .iter()
        .map(|(k, v)| format!("{}={}", uri_encode(k), uri_encode(v)))
        .collect::<Vec<_>>()
        .join("&");
    let path_style = storage.use_minio
        || storage
            .endpoint_url
            .as_deref()
            .is_some_and(|e| !e.is_empty());
    let canonical_path = if path_style {
        format!("/{}/{}", storage.bucket_name, uri_encode_path(object_name))
    } else {
        format!("/{}", uri_encode_path(object_name))
    };
    let canonical = format!(
        "GET\n{canonical_path}\n{canonical_query}\nhost:{signed_host}\n\nhost\nUNSIGNED-PAYLOAD"
    );
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{}\n{}\n{}",
        params
            .iter()
            .find(|(k, _)| k == "X-Amz-Date")
            .expect("date param")
            .1,
        scope,
        sha256_hex(canonical.as_bytes())
    );
    let signature = hex(&hmac_sha256(
        &signing_key(&storage.secret_access_key, &date, region),
        string_to_sign.as_bytes(),
    ));
    format!("{endpoint}{canonical_path}?{render_query}&X-Amz-Signature={signature}")
}

/// Path encoding for the canonical URI: slashes survive, every
/// segment is RFC 3986-encoded (botocore `quote(path, safe='/~')`
/// with the same unreserved set as [`uri_encode`]).
fn uri_encode_path(path: &str) -> String {
    path.split('/')
        .map(uri_encode)
        .collect::<Vec<_>>()
        .join("/")
}

/// `generate_presigned_post(object_name, file_type, file_size)`
/// (`storage.py`): `{"url","fields"}` with botocore's field order and
/// the `storage.py` condition order.
fn presigned_post(
    storage: &pidash_db::config::StorageSettings,
    scheme: &str,
    host: &str,
    object_name: &str,
    file_type: &str,
    file_size: i64,
    now: &DateTime<Utc>,
) -> Value {
    let region = storage.region.as_str();
    let (endpoint, _) = endpoint_parts(storage, scheme, host);
    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let date = now.format("%Y%m%d").to_string();
    let scope = credential_scope(&date, region);
    let expiration = (*now + chrono::Duration::seconds(storage.signed_url_expiration_secs))
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();
    let credential = format!("{}/{}", storage.access_key_id, scope);
    // Condition order mirrors `storage.py`: bucket, content-length
    // range, Content-Type, key — then the three signer conditions the
    // client-level `generate_presigned_post` appends
    // (`botocore/signers.py`). Serialized with CPython `json.dumps`
    // default separators so the policy bytes match botocore's.
    let conditions = format!(
        "[{{\"bucket\": {}}}, [\"content-length-range\", 1, {}], {{\"Content-Type\": {}}}, {{\"key\": {}}}, {{\"bucket\": {}}}, {{\"key\": {}}}, {{\"x-amz-algorithm\": \"AWS4-HMAC-SHA256\"}}, {{\"x-amz-credential\": {}}}, {{\"x-amz-date\": {}}}]",
        py_json_string(&storage.bucket_name),
        file_size,
        py_json_string(file_type),
        py_json_string(object_name),
        py_json_string(&storage.bucket_name),
        py_json_string(object_name),
        py_json_string(&credential),
        py_json_string(&amz_date),
    );
    let policy_json = format!(
        "{{\"expiration\": {}, \"conditions\": {conditions}}}",
        py_json_string(&expiration),
    );
    let policy_b64 = base64_encode(policy_json.as_bytes());
    let signature = hex(&hmac_sha256(
        &signing_key(&storage.secret_access_key, &date, region),
        policy_b64.as_bytes(),
    ));
    let url = if storage.use_minio
        || storage
            .endpoint_url
            .as_deref()
            .is_some_and(|e| !e.is_empty())
    {
        format!("{endpoint}/{}", storage.bucket_name)
    } else {
        format!("{endpoint}/")
    };
    let mut fields = Map::with_capacity(7);
    fields.insert(
        "Content-Type".to_owned(),
        Value::String(file_type.to_owned()),
    );
    fields.insert("key".to_owned(), Value::String(object_name.to_owned()));
    fields.insert(
        "x-amz-algorithm".to_owned(),
        Value::String("AWS4-HMAC-SHA256".to_owned()),
    );
    fields.insert("x-amz-credential".to_owned(), Value::String(credential));
    fields.insert("x-amz-date".to_owned(), Value::String(amz_date));
    fields.insert("policy".to_owned(), Value::String(policy_b64));
    fields.insert("x-amz-signature".to_owned(), Value::String(signature));
    serde_json::json!({"url": url, "fields": fields})
}

/// CPython `json.dumps` string encoding (`ensure_ascii`): `"` and
/// `\` escaped, C0 controls short/`\u00XX`, everything else non-ASCII
/// as `\uXXXX` (surrogate pairs past the BMP).
fn py_json_string(input: &str) -> String {
    let mut out = String::with_capacity(input.len() + 2);
    out.push('"');
    for c in input.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c if (c as u32) < 0x7f => out.push(c),
            c => {
                let mut buf = [0u16; 2];
                for unit in c.encode_utf16(&mut buf) {
                    out.push_str(&format!("\\u{unit:04x}"));
                }
            }
        }
    }
    out.push('"');
    out
}

/// Standard base64 with padding (policy documents).
fn base64_encode(input: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let mut n: u32 = 0;
        for (i, b) in chunk.iter().enumerate() {
            n |= (*b as u32) << (16 - 8 * i);
        }
        let pad = 3 - chunk.len();
        for i in 0..4 - pad {
            out.push(ALPHABET[((n >> (18 - 6 * i)) & 0x3f) as usize] as char);
        }
        for _ in 0..pad {
            out.push('=');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use pidash_db::config::StorageSettings;
    use pidash_db::v1_assets::model::file_asset;

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

    #[tokio::test]
    async fn bad_error_body_is_byte_exact() {
        let response =
            Denial::BadError("The required key does not exist.".to_owned()).into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .expect("body");
        assert_eq!(
            &body[..],
            br#"{"error":"The required key does not exist."}"#
        );
    }

    fn test_storage() -> StorageSettings {
        StorageSettings {
            use_minio: true,
            access_key_id: "access-key".to_owned(),
            secret_access_key: "secret-key".to_owned(),
            bucket_name: "uploads".to_owned(),
            region: "us-east-1".to_owned(),
            endpoint_url: None,
            signed_url_expiration_secs: 3600,
        }
    }

    fn test_now() -> DateTime<Utc> {
        chrono::DateTime::from_timestamp(1_700_000_000, 0).expect("fixed test time")
    }

    // fx-h-asset-generic `error_bodies`, byte for byte.
    #[test]
    fn error_bodies_match_fixture() {
        assert_eq!(
            WORKSPACE_NOT_FOUND_BODY,
            r#"{"error":"Workspace not found"}"#
        );
        assert_eq!(ASSET_NOT_FOUND_BODY, r#"{"error":"Asset not found"}"#);
        assert_eq!(NOT_UPLOADED_BODY, r#"{"error":"Asset not yet uploaded"}"#);
        assert_eq!(
            GET_SERVER_ERROR_BODY,
            r#"{"error":"Internal server error"}"#
        );
        assert_eq!(
            MISSING_FIELDS_BODY,
            r#"{"error":"Name and size are required fields.","status":false}"#
        );
        assert_eq!(
            BAD_TYPE_BODY,
            r#"{"error":"Invalid file type.","status":false}"#
        );
        assert_eq!(
            POST_WORKSPACE_MISSING_BODY,
            r#"{"error":"The requested resource does not exist."}"#
        );
        assert_eq!(
            PAYLOAD_NOT_VALID_BODY,
            r#"{"error":"The payload is not valid"}"#
        );
        assert_eq!(
            INVALID_DETAIL_BODY,
            r#"{"error":"Please provide valid detail"}"#
        );
        assert_eq!(
            SERVER_ERROR_BODY,
            r#"{"error":"Something went wrong please try again later"}"#
        );
        assert_eq!(
            UNAUTHENTICATED_BODY,
            r#"{"detail":"Authentication credentials were not provided."}"#
        );
        assert_eq!(
            INVALID_TOKEN_BODY,
            r#"{"detail":"Given API token is not valid"}"#
        );
        assert_eq!(PAGE_NOT_FOUND_BODY, r#"{"error":"Page not found."}"#);
        assert_eq!(
            DEDUPE_MESSAGE,
            "Asset with same external id and source already exists"
        );
    }

    // Response key order: DRF renders declaration order.
    #[test]
    fn response_key_order_matches_python_dicts() {
        let get = serde_json::json!({
            "asset_id": "id",
            "asset_url": "url",
            "asset_name": "n",
            "asset_type": "t",
        });
        assert_eq!(
            serde_json::to_string(&get).expect("json"),
            r#"{"asset_id":"id","asset_url":"url","asset_name":"n","asset_type":"t"}"#
        );
        let post = serde_json::json!({
            "upload_data": {},
            "asset_id": "id",
            "asset_url": "url",
        });
        assert_eq!(
            serde_json::to_string(&post).expect("json"),
            r#"{"upload_data":{},"asset_id":"id","asset_url":"url"}"#
        );
        let conflict = serde_json::json!({
            "message": DEDUPE_MESSAGE,
            "asset_id": "id",
            "asset_url": "url",
        });
        assert_eq!(
            serde_json::to_string(&conflict).expect("json"),
            r#"{"message":"Asset with same external id and source already exists","asset_id":"id","asset_url":"url"}"#
        );
        // `status` renders JSON false for Python `False`.
        let missing: Value = serde_json::from_str(MISSING_FIELDS_BODY).expect("fixture body");
        assert_eq!(missing["status"], Value::Bool(false));
    }

    // The POST allowlist is the full settings list (73 entries incl. the
    // two source dups), not the 5-mime user list.
    #[test]
    fn mime_allowlist_is_settings_full_list() {
        assert_eq!(ATTACHMENT_MIME_TYPES.len(), 73);
        for mime in [
            "image/jpeg",
            "image/svg+xml",
            "application/pdf",
            "application/vnd.visio",
            "image/x-portable-bitmap",
            "audio/mpeg",
            "video/mp4",
            "application/zip",
            "model/gltf-binary",
            "application/octet-stream",
            "font/woff2",
            "application/json",
            "application/x-sql",
        ] {
            assert!(ATTACHMENT_MIME_TYPES.contains(&mime), "{mime}");
        }
        for mime in ["text/html", "", "IMAGE/JPEG", "image/jpg"] {
            assert!(!ATTACHMENT_MIME_TYPES.contains(&mime), "{mime}");
        }
    }

    #[test]
    fn required_check_uses_python_truthiness() {
        assert!(!is_truthy(None));
        assert!(!is_truthy(Some(&Value::Null)));
        assert!(!is_truthy(Some(&serde_json::json!(""))));
        assert!(!is_truthy(Some(&serde_json::json!(0))));
        assert!(!is_truthy(Some(&serde_json::json!(false))));
        assert!(!is_truthy(Some(&serde_json::json!([]))));
        assert!(!is_truthy(Some(&serde_json::json!({}))));
        assert!(is_truthy(Some(&serde_json::json!("a.png"))));
        assert!(is_truthy(Some(&serde_json::json!(7))));
    }

    // `int(request.data.get("size", LIMIT))`: missing reads the default;
    // null and non-numeric raise (BUG-size-null → ConvertError → 500).
    #[test]
    fn size_conversion_runs_before_required_check() {
        let limit = 5_242_880_i64;
        assert!(matches!(
            python_int_size(None, limit),
            SizeOutcome::Size(5_242_880)
        ));
        assert!(matches!(
            python_int_size(Some(&Value::Null), limit),
            SizeOutcome::ConvertError
        ));
        assert!(matches!(
            python_int_size(Some(&serde_json::json!("abc")), limit),
            SizeOutcome::ConvertError
        ));
        assert!(matches!(
            python_int_size(Some(&serde_json::json!("12.5")), limit),
            SizeOutcome::ConvertError
        ));
        assert!(matches!(
            python_int_size(Some(&serde_json::json!([1])), limit),
            SizeOutcome::ConvertError
        ));
        assert!(matches!(
            python_int_size(Some(&serde_json::json!("1024")), limit),
            SizeOutcome::Size(1024)
        ));
        assert!(matches!(
            python_int_size(Some(&serde_json::json!(" 42 ")), limit),
            SizeOutcome::Size(42)
        ));
        // CPython underscores + unbounded width (verified live).
        assert!(matches!(
            python_int_size(Some(&serde_json::json!("1_0")), limit),
            SizeOutcome::Size(10)
        ));
        assert!(matches!(
            python_int_size(Some(&serde_json::json!("-1_0")), limit),
            SizeOutcome::Size(-10)
        ));
        assert!(matches!(
            python_int_size(Some(&serde_json::json!("1__0")), limit),
            SizeOutcome::ConvertError
        ));
        assert!(matches!(
            python_int_size(Some(&serde_json::json!("99999999999999999999999")), limit),
            SizeOutcome::Size(i64::MAX)
        ));
        // Floats truncate toward zero like CPython `int()`.
        assert!(matches!(
            python_int_size(Some(&serde_json::json!(12.9)), limit),
            SizeOutcome::Size(12)
        ));
        assert!(matches!(
            python_int_size(Some(&serde_json::json!(true)), limit),
            SizeOutcome::Size(1)
        ));
        assert!(matches!(
            python_int_size(Some(&serde_json::json!(0)), limit),
            SizeOutcome::Size(0)
        ));
        // `size_limit = min(size, FILE_SIZE_LIMIT)`; only `min`, no max
        // check beyond it.
        assert_eq!(1_000_i64.min(limit), 1_000);
        assert_eq!(99_999_999_i64.min(limit), limit);
    }

    #[test]
    fn asset_key_shape_is_workspace_hex_dash_name() {
        let workspace = Uuid::parse_str("123e4567-e89b-12d3-a456-426614174000").expect("uuid");
        let hex = Uuid::parse_str("123e4567-e89b-12d3-a456-426614174001").expect("uuid");
        let key = format!("{workspace}/{}-{}", hex.simple(), "image.jpg");
        assert_eq!(
            key,
            "123e4567-e89b-12d3-a456-426614174000/123e4567e89b12d3a456426614174001-image.jpg"
        );
        assert_eq!(hex.simple().to_string().len(), 32);
    }

    #[test]
    fn python_str_matches_cpython_scalars() {
        assert_eq!(python_str(&Value::Null), "None");
        assert_eq!(python_str(&serde_json::json!(true)), "True");
        assert_eq!(python_str(&serde_json::json!(false)), "False");
        assert_eq!(python_str(&serde_json::json!(7)), "7");
        assert_eq!(python_str(&serde_json::json!("x")), "x");
    }

    // `FileAsset.asset_url` branches (`db/models/asset.py:80-100`).
    #[test]
    fn asset_url_property_branches() {
        let id = Uuid::parse_str("123e4567-e89b-12d3-a456-426614174000").expect("uuid");
        let project = Uuid::parse_str("123e4567-e89b-12d3-a456-426614174001").expect("uuid");
        // Fresh POST row: ISSUE_ATTACHMENT, no project, no issue.
        assert_eq!(
            file_asset_url(Some("ISSUE_ATTACHMENT"), "ws", None, None, &id),
            serde_json::json!("/api/assets/v2/workspaces/ws/projects/None/issues/None/attachments/123e4567-e89b-12d3-a456-426614174000/")
        );
        // Dedupe hit with a project set renders the dashed UUID.
        assert_eq!(
            file_asset_url(Some("ISSUE_ATTACHMENT"), "ws", Some(project), None, &id),
            serde_json::json!("/api/assets/v2/workspaces/ws/projects/123e4567-e89b-12d3-a456-426614174001/issues/None/attachments/123e4567-e89b-12d3-a456-426614174000/")
        );
        assert_eq!(
            file_asset_url(Some("USER_AVATAR"), "ws", None, None, &id),
            serde_json::json!("/api/assets/v2/static/123e4567-e89b-12d3-a456-426614174000/")
        );
        assert_eq!(
            file_asset_url(Some("ISSUE_DESCRIPTION"), "ws", None, None, &id),
            serde_json::json!(
                "/api/assets/v2/workspaces/ws/projects/None/123e4567-e89b-12d3-a456-426614174000/"
            )
        );
        assert_eq!(
            file_asset_url(Some("WHATEVER"), "ws", None, None, &id),
            Value::Null
        );
        assert_eq!(file_asset_url(None, "ws", None, None, &id), Value::Null);
    }

    // PATCH `is_uploaded` (`BooleanField.to_python`, verified live):
    // missing keeps current; null violates NOT NULL (400 payload);
    // `1`/`0`/`1.0` and exactly `"t"`/`"True"`/`"1"` convert; `"true"`,
    // `"yes"`, `"on"`, `2`, `[]` raise (400 detail).
    #[test]
    fn patch_is_uploaded_semantics() {
        assert!(matches!(patch_is_uploaded(None), UploadedOutcome::Keep));
        assert!(matches!(
            patch_is_uploaded(Some(&serde_json::json!(true))),
            UploadedOutcome::Set(true)
        ));
        assert!(matches!(
            patch_is_uploaded(Some(&serde_json::json!(false))),
            UploadedOutcome::Set(false)
        ));
        assert!(matches!(
            patch_is_uploaded(Some(&serde_json::json!(1))),
            UploadedOutcome::Set(true)
        ));
        assert!(matches!(
            patch_is_uploaded(Some(&serde_json::json!(0))),
            UploadedOutcome::Set(false)
        ));
        assert!(matches!(
            patch_is_uploaded(Some(&serde_json::json!(1.0))),
            UploadedOutcome::Set(true)
        ));
        assert!(matches!(
            patch_is_uploaded(Some(&serde_json::json!(0.0))),
            UploadedOutcome::Set(false)
        ));
        assert!(matches!(
            patch_is_uploaded(Some(&serde_json::json!("True"))),
            UploadedOutcome::Set(true)
        ));
        assert!(matches!(
            patch_is_uploaded(Some(&serde_json::json!("t"))),
            UploadedOutcome::Set(true)
        ));
        assert!(matches!(
            patch_is_uploaded(Some(&serde_json::json!("0"))),
            UploadedOutcome::Set(false)
        ));
        assert!(matches!(
            patch_is_uploaded(Some(&Value::Null)),
            UploadedOutcome::NullViolation
        ));
        for bad in ["true", "yes", "on", "maybe", ""] {
            assert!(
                matches!(
                    patch_is_uploaded(Some(&serde_json::json!(bad))),
                    UploadedOutcome::Invalid
                ),
                "{bad}"
            );
        }
        for bad in [
            serde_json::json!(2),
            serde_json::json!(0.5),
            serde_json::json!([]),
        ] {
            assert!(
                matches!(patch_is_uploaded(Some(&bad)), UploadedOutcome::Invalid),
                "{bad}"
            );
        }
    }

    // POST `project_id` (`UUIDField.to_python` + the `""`→NULL rewrite,
    // verified live): null/missing/`""` stay NULL; valid UUIDs bind;
    // garbage strings, floats and containers raise (400 detail); ints
    // ride `uuid.UUID(int=…)` (negatives raise).
    #[test]
    fn patch_project_id_semantics() {
        assert!(matches!(patch_project_id(None), ProjectOutcome::Null));
        assert!(matches!(
            patch_project_id(Some(&Value::Null)),
            ProjectOutcome::Null
        ));
        assert!(matches!(
            patch_project_id(Some(&serde_json::json!(""))),
            ProjectOutcome::Null
        ));
        assert!(matches!(
            patch_project_id(Some(&serde_json::json!(
                "123e4567-e89b-12d3-a456-426614174000"
            ))),
            ProjectOutcome::Bind(_)
        ));
        assert!(matches!(
            patch_project_id(Some(&serde_json::json!("garbage"))),
            ProjectOutcome::Invalid
        ));
        assert!(matches!(
            patch_project_id(Some(&serde_json::json!(1.5))),
            ProjectOutcome::Invalid
        ));
        assert!(matches!(
            patch_project_id(Some(&serde_json::json!([]))),
            ProjectOutcome::Invalid
        ));
        assert!(matches!(
            patch_project_id(Some(&serde_json::json!(-1))),
            ProjectOutcome::Invalid
        ));
        match patch_project_id(Some(&serde_json::json!(123))) {
            ProjectOutcome::Bind(id) => assert_eq!(id.as_u128(), 123),
            other => panic!("123 should bind, got {other:?}"),
        }
        match patch_project_id(Some(&serde_json::json!(true))) {
            ProjectOutcome::Bind(id) => assert_eq!(id.as_u128(), 1),
            other => panic!("true should bind, got {other:?}"),
        }
    }

    // The metadata guard + wire shape come from the merged tasks layer.
    #[test]
    fn metadata_guard_and_wire_match_fixture() {
        use pidash_jobs::v1_assets::tasks::{metadata_job, storage_metadata_missing};
        assert!(storage_metadata_missing(&Value::Null));
        assert!(storage_metadata_missing(&serde_json::json!({})));
        assert!(storage_metadata_missing(&serde_json::json!("")));
        assert!(!storage_metadata_missing(&serde_json::json!({"a": 1})));
        let job = metadata_job("123e4567-e89b-12d3-a456-426614174000");
        assert_eq!(
            job.task,
            "pi_dash.bgtasks.storage_metadata_task.get_asset_object_metadata"
        );
        assert_eq!(
            job.kwargs,
            serde_json::json!({"asset_id": "123e4567-e89b-12d3-a456-426614174000"})
        );
    }

    // The detail + dedupe builders come from the merged query layer;
    // pin the bind contract the handlers rely on.
    #[test]
    fn query_builders_carry_both_binds() {
        let detail = asset_queries::generic_detail_select_sql();
        assert!(detail.contains("\"file_assets\""));
        assert!(detail.contains("$1") && detail.contains("$2"));
        let dedupe = asset_queries::external_dedupe_select_sql();
        assert!(dedupe.contains("$1") && dedupe.contains("$2") && dedupe.contains("$3"));
        assert!(dedupe.contains("LIMIT 1"));
    }

    #[test]
    fn model_defaults_match_create_row() {
        // The INSERT writes these Django defaults explicitly.
        assert!(
            file_asset::DEFAULTS[file_asset::COLUMNS
                .iter()
                .position(|c| *c == "is_deleted")
                .expect("col")]
                == Some("false")
        );
        assert!(
            file_asset::DEFAULTS[file_asset::COLUMNS
                .iter()
                .position(|c| *c == "is_uploaded")
                .expect("col")]
                == Some("false")
        );
        assert_eq!(GENERIC_ENTITY_TYPE, "ISSUE_ATTACHMENT");
        assert!(file_asset::ENTITY_TYPES.contains(&GENERIC_ENTITY_TYPE));
    }

    // SigV4 smoke: deterministic per instant, MinIO-signed against the
    // request host, disposition + filename in the query, nine-condition
    // policy on the POST with botocore field order.
    #[test]
    fn presigned_shapes_are_stable() {
        let storage = test_storage();
        let now = test_now();
        let first = presigned_get_url(
            &storage,
            "http",
            "example.test",
            "ws/hex-image.jpg",
            "inline",
            Some("image.jpg".to_owned()),
            &now,
        );
        let second = presigned_get_url(
            &storage,
            "http",
            "example.test",
            "ws/hex-image.jpg",
            "inline",
            Some("image.jpg".to_owned()),
            &now,
        );
        assert_eq!(first, second);
        assert!(first.starts_with("http://example.test/uploads/ws/hex-image.jpg?"));
        // botocore renders insertion order (override first, signature
        // last) — the live Django shape.
        let query = first.split('?').nth(1).expect("query string");
        let keys: Vec<&str> = query
            .split('&')
            .map(|p| p.split('=').next().expect("param"))
            .collect();
        assert_eq!(
            keys,
            [
                "response-content-disposition",
                "X-Amz-Algorithm",
                "X-Amz-Credential",
                "X-Amz-Date",
                "X-Amz-Expires",
                "X-Amz-SignedHeaders",
                "X-Amz-Signature"
            ]
        );
        assert!(first.contains("response-content-disposition=inline"));
        assert!(first.contains("filename%2A%3DUTF-8%27%27image.jpg"));
        let post = presigned_post(
            &storage,
            "http",
            "example.test",
            "ws/hex-a.jpg",
            "image/jpeg",
            100,
            &now,
        );
        assert_eq!(
            post["url"],
            serde_json::json!("http://example.test/uploads")
        );
        let fields = post["fields"].as_object().expect("fields object");
        let order: Vec<&str> = fields.keys().map(String::as_str).collect();
        assert_eq!(
            order,
            [
                "Content-Type",
                "key",
                "x-amz-algorithm",
                "x-amz-credential",
                "x-amz-date",
                "policy",
                "x-amz-signature"
            ]
        );
        assert_eq!(fields["Content-Type"], serde_json::json!("image/jpeg"));
        assert_eq!(fields["key"], serde_json::json!("ws/hex-a.jpg"));
    }

    #[test]
    fn base64_and_py_json_helpers() {
        assert_eq!(base64_encode(b"Man"), "TWFu");
        assert_eq!(base64_encode(b"Ma"), "TWE=");
        assert_eq!(py_json_string("a\"b\\c"), "\"a\\\"b\\\\c\"");
        assert_eq!(uri_encode("a b/c~"), "a%20b%2Fc~");
        assert_eq!(quote_filename("a b/c"), "a%20b/c");
    }
}
