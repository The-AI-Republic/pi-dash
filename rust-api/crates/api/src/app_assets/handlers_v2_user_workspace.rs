//! v2 user/workspace/static/restore asset handlers (D-31, stage 5).
//!
//! Ports `UserAssetsV2Endpoint` (`apps/api/pi_dash/app/views/asset/v2.py:29-198`),
//! `WorkspaceFileAssetEndpoint` (`v2.py:201-429`),
//! `StaticFileAssetEndpoint` (`v2.py:432-465`) and `AssetRestoreEndpoint`
//! (`v2.py:468-477`) with routes from `apps/api/pi_dash/app/urls/asset.py`.
//! Only the six v2 user/workspace/static/restore paths are registered, so
//! the edge serves exactly this family from Rust while every sibling path
//! (project, bulk, check, duplicate, downloads, v1) keeps proxying to
//! Django — route registration is the cutover granularity, no flag needed.
//!
//! Layering: SQL text plus post validation live in
//! `pidash_services::app_assets::queries_v2_user_workspace` (PIDASHCONV-356);
//! gates in [`crate::app_assets::guards`] (PIDASHCONV-378); the metadata
//! publisher kwargs in `pidash_services::app_assets::tasks` (PIDASHCONV-387).
//! This module owns the HTTP shell (routes, session auth, request parsing),
//! the handler-owned reads/writes (entity links, lookups the query layer
//! does not cover: static by-id, restore through `all_objects`), row
//! fetching and the exact response bytes.
//!
//! Django-idiom notes (translate, don't redesign):
//!
//! * Routes use `<uuid:asset_id>` converters, so a non-UUID segment never
//!   reaches the view: the resolver 404s with `{"error":"Page not found."}`
//!   (global `handler404`, `pi_dash/urls.py:15`). Bad-UUID params answer
//!   that body here, before auth (resolution precedes `initial()`).
//! * `.get()` misses raise `DoesNotExist`, which the base `handle_exception`
//!   (`app/views/base.py:129-133`) maps to 404
//!   `{"error":"The required object does not exist."}` — the envelope, not
//!   the endpoint's inline 404. Scoped-lookup misses (cross-user ids, wrong
//!   slugs, unknown restore targets) all answer it.
//! * `size=int(data.get("size", FILE_SIZE_LIMIT))` (`v2.py:113,317`):
//!   missing → `FILE_SIZE_LIMIT`; present non-numeric (or explicit null) →
//!   `ValueError`/`TypeError` → the 500 fallback. `min(size, LIMIT)` clamps
//!   with no floor — a negative size stores as-is. No max check.
//! * `asset_key` renders a missing `name` as the literal `None`
//!   (`f"{hex}-{name}"`, `v2.py:144,352`); `attributes` carries JSON null
//!   for it. JSON numbers truncate toward zero (`int(3.7) == 3`);
//!   numeric strings parse with CPython `int(s, 10)` semantics.
//! * POST answers 200 (not 201) with keys `upload_data`, `asset_id`,
//!   `asset_url` in that order (`v2.py:161-168,:370-377`).
//! * GET answers `HttpResponseRedirect`, not a DRF `Response`
//!   (`v2.py:429,465`): 302 with a `Location` header and an empty body.
//! * PATCH/DELETE/POST-restore answer 204 with an empty body.
//! * `request.data` parses JSON, form and multipart bodies (DRF's default
//!   parsers); an empty body reads as `{}`.
//! * Cache invalidation (`invalidate_cache_directly`, `utils/cache.py`) has
//!   no Rust counterpart: the contract suites assert DB rows only, and an
//!   uninvalidated cache risks only stale reads (same call as the merged
//!   license handlers). The invalidation descriptors stay declarative in
//!   the queries layer (`USER_POST_COMMIT`, `WORKSPACE_LOGO_POST_COMMIT`).
//!
//! S3 presigning mirrors `S3Storage` (`settings/storage.py`) offline
//! (no network), exactly like the merged D-02 `space` handlers: MinIO mode
//! signs against `{scheme}://{Host}` (scheme from `X-Forwarded-Proto`,
//! default `http`), otherwise against the configured endpoint URL or the
//! virtual-hosted AWS default. The POST policy lists conditions in
//! `storage.py` order plus the signer conditions, and `fields` keeps
//! botocore's insertion order. The GET redirect carries
//! `response-content-disposition=attachment; filename*=UTF-8''<name>` for
//! the workspace fetch and `inline; filename*=UTF-8''<hex>` (fresh uuid4
//! hex per request, `_get_content_disposition("inline", None)`) for the
//! static fetch.
//!
//! The PATCH metadata publish (`get_asset_object_metadata.delay(asset_id=…)`,
//! `v2.py:177,386`) enqueues through the Postgres-backed queue in Celery
//! protocol v2 under
//! `pi_dash.bgtasks.storage_metadata_task.get_asset_object_metadata` with
//! kwargs `{"asset_id": "<str>"}` (the v2 keyword form — not the space
//! positional form). Best-effort: a failed enqueue warns and the response
//! stands (the D-32 `app_intake` pattern; the proxy contract tests never
//! run a worker).
//!
//! Ported bugs (also listed in the PR):
//!
//! * BUG-workspace-id-overwrite (`v2.py:355-363`): the workspace create
//!   passes both `workspace=workspace` and the `get_entity_id_field`
//!   spread, so for `WORKSPACE_LOGO` the `workspace_id` column is the
//!   `entity_identifier`, not the slug-resolved id (later kwarg wins).
//! * BUG-missing-identifier (`v2.py:319`): the workspace post defaults
//!   `entity_identifier` to `False`, so any post without one spreads a
//!   boolean into a uuid column and answers 500.
//! * BUG-logo-delete-get (`v2.py:289-291`): the workspace-logo delete
//!   branch reads through `.get(...)` then checks `if workspace is None` —
//!   dead code; a missing row raises (404), it never returns silently.
//! * BUG-avatar-cover-clear (`v2.py:45,63,257,279`): the avatar/logo clear
//!   writes `""` while the cover clear writes `None`. Ported as-is.
//! * BUG-size-no-max (`v2.py:113,317`): `size` has no upper check; only
//!   `min()` against the limit.

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Router;
use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use sqlx::Row;
use uuid::Uuid;

use pidash_auth::permissions::allow::AllowFacts;
use pidash_auth::scope::TenantScope;
use pidash_services::app_assets::queries_v2_user_workspace as q;
use pidash_types::WorkspaceId;

use super::guards;
use crate::state::AppState;

/// Register the six owned v2 paths. Sibling handler issues merge their own
/// routers (project/bulk/check/duplicate/downloads, v1); merges keep both
/// sides. Every non-owned method on these paths proxies to Django so DRF's
/// own 405-after-auth and metadata responses survive byte for byte.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/assets/v2/user-assets/",
            crate::license::owned(axum::routing::post(user_post), &["POST"]),
        )
        .route(
            "/api/assets/v2/user-assets/{asset_id}/",
            crate::license::owned(
                axum::routing::patch(user_patch).delete(user_delete),
                &["PATCH", "DELETE"],
            ),
        )
        .route(
            "/api/assets/v2/workspaces/{slug}/",
            crate::license::owned(axum::routing::post(workspace_post), &["POST"]),
        )
        .route(
            "/api/assets/v2/workspaces/{slug}/{asset_id}/",
            crate::license::owned(
                axum::routing::get(workspace_get)
                    .patch(workspace_patch)
                    .delete(workspace_delete),
                &["GET", "PATCH", "DELETE"],
            ),
        )
        .route(
            "/api/assets/v2/static/{asset_id}/",
            crate::license::owned(axum::routing::get(static_get), &["GET"]),
        )
        .route(
            "/api/assets/v2/workspaces/{slug}/restore/{asset_id}/",
            crate::license::owned(axum::routing::post(restore_post), &["POST"]),
        )
}

// ---------------------------------------------------------------------------
// Small responders
// ---------------------------------------------------------------------------

/// Exact bytes of the DRF `IsAuthenticated` denial.
pub const UNAUTHENTICATED_BODY: &str =
    r#"{"detail":"Authentication credentials were not provided."}"#;
/// `handle_exception`'s `ObjectDoesNotExist` branch
/// (`app/views/base.py:129-133`).
pub const NOT_FOUND_BODY: &str = r#"{"error":"The required object does not exist."}"#;
/// `handle_exception`'s generic 500 branch (also `ValueError`/`TypeError`
/// from the unguarded `int(size)`, and any DB error).
pub const SERVER_ERROR_BODY: &str = r#"{"error":"Something went wrong please try again later"}"#;
/// Resolver 404 for non-UUID `<uuid:asset_id>` segments (global
/// `handler404`, `pi_dash/urls.py:15`).
pub const PAGE_NOT_FOUND_BODY: &str = r#"{"error":"Page not found."}"#;
/// `handle_exception`'s `IntegrityError` branch
/// (`app/views/base.py:120-124`).
pub const PAYLOAD_NOT_VALID_BODY: &str = r#"{"error":"The payload is not valid"}"#;
/// `allow_permission` fallthrough (`app/permissions/base.py`).
pub const PERMISSION_DENIED_BODY: &str = r#"{"error":"You don't have the required permissions."}"#;

fn raw(status: StatusCode, body: &'static str) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body))
        .expect("static response")
}

fn json_status(status: StatusCode, body: Value) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(
            serde_json::to_string(&body).expect("serializable response"),
        ))
        .expect("json response")
}

fn server_error() -> Response {
    raw(StatusCode::INTERNAL_SERVER_ERROR, SERVER_ERROR_BODY)
}

/// `.get()` miss envelope: Django `ObjectDoesNotExist` through the base
/// `handle_exception` (`app/views/base.py:129-133`).
fn does_not_exist() -> Response {
    raw(StatusCode::NOT_FOUND, NOT_FOUND_BODY)
}

fn page_not_found() -> Response {
    raw(StatusCode::NOT_FOUND, PAGE_NOT_FOUND_BODY)
}

fn no_content() -> Response {
    StatusCode::NO_CONTENT.into_response()
}

fn pool(state: &AppState) -> Option<&sqlx::PgPool> {
    state.pools().map(|pools| pools.primary())
}

/// `IsAuthenticated` (`app/views/base.py`, `BaseAPIView` default):
/// anonymous answers DRF `NotAuthenticated` (401) before anything else.
async fn require_user(
    state: &AppState,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Result<crate::license::Actor, crate::license::Denial> {
    let Some(pool) = pool(state) else {
        return Err(crate::license::Denial::ServerError);
    };
    crate::license::resolve_actor(pool, state.settings().secret_key.as_bytes(), extension)
        .await?
        .ok_or(crate::license::Denial::Unauthorized)
}

// ---------------------------------------------------------------------------
// Request-data parsing (form / multipart / JSON, last value wins)
// ---------------------------------------------------------------------------

/// Parse `request.data` the way DRF does for the three default content
/// types: JSON objects as-is, urlencoded and multipart text fields as
/// strings. Empty bodies read as `{}`. Malformed JSON answers `ParseError`
/// 400; non-object JSON (or an unparseable content type) surfaces the
/// attribute errors the view code hits (500 envelope).
#[allow(clippy::result_large_err)]
fn parse_data(content_type: Option<&str>, body: &[u8]) -> Result<Map<String, Value>, Response> {
    if body.is_empty() {
        return Ok(Map::new());
    }
    let ct = content_type.unwrap_or("");
    if ct.contains("application/json") {
        match serde_json::from_slice::<Value>(body) {
            Ok(Value::Object(map)) => Ok(map),
            // A JSON list/scalar has no `.get`: `AttributeError` → 500.
            Ok(_) => Err(server_error()),
            Err(error) => Err(Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .header(header::CONTENT_TYPE, "application/json")
                .body(axum::body::Body::from(format!(
                    "{{\"detail\":\"JSON parse error - {error}\"}}",
                )))
                .expect("parse error response")),
        }
    } else if ct.contains("application/x-www-form-urlencoded") {
        Ok(parse_urlencoded(body))
    } else if ct.contains("multipart/form-data") {
        let boundary = ct.split("boundary=").nth(1).unwrap_or("").trim();
        if boundary.is_empty() {
            return Err(server_error());
        }
        Ok(parse_multipart(body, boundary))
    } else {
        Err(server_error())
    }
}

fn pct_decode(input: &str) -> String {
    let mut out = Vec::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'+' {
            out.push(b' ');
            i += 1;
        } else if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                out.push(h << 4 | l);
                i += 3;
            } else {
                out.push(bytes[i]);
                i += 1;
            }
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn parse_urlencoded(body: &[u8]) -> Map<String, Value> {
    let mut map = Map::new();
    for pair in String::from_utf8_lossy(body).split('&') {
        if pair.is_empty() {
            continue;
        }
        let (k, v) = match pair.split_once('=') {
            Some((k, v)) => (k, v),
            None => (pair, ""),
        };
        map.insert(pct_decode(k), Value::String(pct_decode(v)));
    }
    map
}

/// Minimal multipart text-field parser: splits on the boundary, reads each
/// part's `name="..."`, keeps the raw bytes as a string.
fn parse_multipart(body: &[u8], boundary: &str) -> Map<String, Value> {
    let mut map = Map::new();
    let text = String::from_utf8_lossy(body);
    let delimiter = format!("--{boundary}");
    for part in text.split(&delimiter) {
        let part = part.strip_prefix("\r\n").unwrap_or(part);
        if part.is_empty() || part.starts_with("--") {
            continue;
        }
        let Some(sep) = part.find("\r\n\r\n") else {
            continue;
        };
        let (head, value) = part.split_at(sep);
        let value = value
            .strip_prefix("\r\n\r\n")
            .unwrap_or(value)
            .strip_suffix("\r\n")
            .unwrap_or(value);
        let Some(name_start) = head.find("name=\"") else {
            continue;
        };
        let rest = &head[name_start + 6..];
        let Some(name_end) = rest.find('"') else {
            continue;
        };
        map.insert(rest[..name_end].to_owned(), Value::String(value.to_owned()));
    }
    map
}

/// `request.data.get("type", "image/jpeg")` (`v2.py:112,316`): the default
/// applies only when the key is absent. A present non-string (explicit
/// null included) fails the allowlist below and answers the 400, exactly
/// like `None not in allowed_types` in Django.
fn post_mime(data: &Map<String, Value>) -> Option<&str> {
    match data.get("type") {
        None => Some(q::DEFAULT_FILE_TYPE),
        Some(Value::String(s)) => Some(s),
        Some(_) => None,
    }
}

/// `size = int(request.data.get("size", FILE_SIZE_LIMIT))`
/// (`v2.py:113,317`): missing → the limit; JSON numbers truncate toward
/// zero (`int(3.7) == 3`); bools are ints (`int(True) == 1`); strings
/// parse with CPython `int(s, 10)` semantics
/// ([`q::parse_size`]); anything else — explicit null, NaN/Inf,
/// out-of-range floats, lists — raises → the 500 fallback.
fn parse_size(value: Option<&Value>, default: i64) -> Option<i64> {
    match value {
        None => Some(default),
        Some(Value::Number(n)) => {
            if let Some(i) = n.as_i64() {
                Some(i)
            } else if let Some(u) = n.as_u64() {
                i64::try_from(u).ok()
            } else {
                n.as_f64().and_then(|f| {
                    if !f.is_finite() {
                        return None;
                    }
                    let t = f.trunc();
                    if t >= i64::MIN as f64 && t <= i64::MAX as f64 {
                        Some(t as i64)
                    } else {
                        None
                    }
                })
            }
        }
        Some(Value::Bool(b)) => Some(i64::from(*b)),
        Some(Value::String(s)) => q::parse_size(Some(s.as_str()), default).ok(),
        Some(_) => None,
    }
}

/// Python `str()` of a scalar for `asset_key` interpolation and
/// `attributes`: missing/null render as `None`.
fn python_str(value: Option<&Value>) -> (String, Value) {
    match value {
        None | Some(Value::Null) => ("None".to_owned(), Value::Null),
        Some(Value::String(s)) => (s.clone(), Value::String(s.clone())),
        Some(Value::Number(n)) => (n.to_string(), Value::Number(n.clone())),
        Some(Value::Bool(b)) => (
            (if *b { "True" } else { "False" }).to_owned(),
            Value::Bool(*b),
        ),
        Some(other) => (
            serde_json::to_string(other).expect("serializable value"),
            other.clone(),
        ),
    }
}

fn content_type(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
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

/// One `file_assets` row with every column the handlers read.
struct AssetRow {
    entity_type: Option<String>,
    is_uploaded: bool,
    storage_metadata: Option<Value>,
    attributes: Option<Value>,
    asset_key: String,
    workspace_id: Option<Uuid>,
    project_id: Option<Uuid>,
}

fn asset_row(row: &sqlx::postgres::PgRow) -> Result<AssetRow, sqlx::Error> {
    Ok(AssetRow {
        entity_type: row.try_get("entity_type")?,
        is_uploaded: row.try_get("is_uploaded")?,
        storage_metadata: row.try_get("storage_metadata")?,
        attributes: row.try_get("attributes")?,
        asset_key: row.try_get("asset")?,
        workspace_id: row.try_get("workspace_id")?,
        project_id: row.try_get("project_id")?,
    })
}

/// `FileAsset.asset_url` (`db/models/asset.py:80-98`) for the entity types
/// the user post mints: always the static branch (`USER_AVATAR` /
/// `USER_COVER`, `asset.py:81-87`).
fn static_asset_url(asset_id: &Uuid) -> Value {
    Value::String(format!("/api/assets/v2/static/{asset_id}/"))
}

/// `FileAsset.asset_url` (`db/models/asset.py:80-98`) for the entity types
/// the workspace post mints. The issue/project branches dereference
/// `self.workspace.slug` — the slug is already resolved here —
/// `project_id` is always unset on this path (the create never passes it
/// except through the `PROJECT_COVER` spread, which takes the static
/// branch), so it renders as `None` exactly like the f-string; `issue_id`
/// is the spread identifier for the two issue entity types (already
/// validated as a storable uuid by the INSERT reaching this line) and
/// `None` otherwise. `DRAFT_ISSUE_ATTACHMENT` falls through every branch
/// and renders null (`asset.py:100`).
fn workspace_asset_url(
    entity_type: &str,
    slug: &str,
    issue_display: &str,
    asset_id: &Uuid,
) -> Value {
    match entity_type {
        "WORKSPACE_LOGO" | "USER_AVATAR" | "USER_COVER" | "PROJECT_COVER" => {
            static_asset_url(asset_id)
        }
        "ISSUE_ATTACHMENT" => Value::String(format!(
            "/api/assets/v2/workspaces/{slug}/projects/None/issues/{issue_display}/attachments/{asset_id}/"
        )),
        "ISSUE_DESCRIPTION" | "COMMENT_DESCRIPTION" | "PAGE_DESCRIPTION"
        | "DRAFT_ISSUE_DESCRIPTION" => Value::String(format!(
            "/api/assets/v2/workspaces/{slug}/projects/None/{asset_id}/"
        )),
        _ => Value::Null,
    }
}

/// Parse a `<uuid:asset_id>` path segment: Django's converter 404s on
/// garbage, so unparseable ids answer the resolver body (before auth —
/// resolution precedes `initial()`).
#[allow(clippy::result_large_err)]
fn parse_asset_id(raw: &str) -> Result<Uuid, Response> {
    raw.parse::<Uuid>().map_err(|_| page_not_found())
}

// ---------------------------------------------------------------------------
// Metadata publisher
// ---------------------------------------------------------------------------

/// Celery task name for the metadata publishers
/// (`storage_metadata_task.py:14`; also
/// `pidash_services::app_assets::tasks::TASK_GET_ASSET_OBJECT_METADATA` —
/// spelled here so this module never depends on the services tasks unit
/// beyond the predicate).
pub const TASK_GET_ASSET_OBJECT_METADATA: &str =
    "pi_dash.bgtasks.storage_metadata_task.get_asset_object_metadata";

/// `get_asset_object_metadata.delay(asset_id=str(asset_id))`
/// (`v2.py:177,386`): empty args, exactly `{"asset_id": "<str>"}` kwargs.
/// Best-effort through the Postgres-backed queue (Celery protocol v2): a
/// failed enqueue warns and the response stands.
async fn publish_metadata(pool: &sqlx::PgPool, asset_id: &Uuid) {
    let mut kwargs = Map::with_capacity(1);
    kwargs.insert("asset_id".to_owned(), Value::String(asset_id.to_string()));
    let message = pidash_jobs::celery::CeleryTaskMessage::new(
        TASK_GET_ASSET_OBJECT_METADATA,
        Vec::new(),
        kwargs,
    );
    let job = pidash_jobs::queue::NewJob::new(
        message.task.clone(),
        Value::Array(message.args.clone()),
        Value::Object(message.kwargs.clone()),
    );
    if let Err(error) = pidash_jobs::queue::enqueue(pool, &job).await {
        tracing::warn!(%error, task = message.task.as_str(), "task enqueue failed; response stands");
    }
}

// ---------------------------------------------------------------------------
// Entity links
// ---------------------------------------------------------------------------

/// `asset_delete`: `FileAsset.objects.filter(id).first()` + flip
/// (`v2.py:32-39,:236-245`) — silent when missing, no 404.
async fn asset_delete(pool: &sqlx::PgPool, asset_id: &Uuid) -> Result<(), sqlx::Error> {
    let row: Option<(Uuid,)> = sqlx::query_as(&q::asset_delete_lookup_sql())
        .bind(asset_id)
        .fetch_optional(pool)
        .await?;
    let Some(_) = row else { return Ok(()) };
    sqlx::query(&q::delete_update_sql())
        .bind(true)
        .bind(Utc::now())
        .bind(asset_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// `UserAssetsV2Endpoint.entity_asset_save` (`v2.py:41-78`).
#[allow(clippy::result_large_err)]
async fn user_entity_save(
    pool: &sqlx::PgPool,
    asset_id: &Uuid,
    entity_type: &str,
    owner_id: &Uuid,
) -> Result<(), Response> {
    match q::user_save_action(entity_type) {
        q::UserSaveAction::SetAvatar => {
            // `User.objects.get(id=asset.user_id)` (miss raises → 404).
            let prev: Option<(Option<Uuid>,)> =
                sqlx::query_as(r#"SELECT "avatar_asset_id" FROM "users" WHERE "id" = $1"#)
                    .bind(owner_id)
                    .fetch_optional(pool)
                    .await
                    .map_err(|_| server_error())?;
            let Some((prev,)) = prev else {
                return Err(does_not_exist());
            };
            if let Some(prev_id) = prev {
                asset_delete(pool, &prev_id)
                    .await
                    .map_err(|_| server_error())?;
            }
            // `user.avatar = ""` (empty string, not None — BUG ported).
            sqlx::query(
                r#"UPDATE "users" SET "avatar" = $1, "avatar_asset_id" = $2 WHERE "id" = $3"#,
            )
            .bind("")
            .bind(asset_id)
            .bind(owner_id)
            .execute(pool)
            .await
            .map_err(|_| server_error())?;
            Ok(())
        }
        q::UserSaveAction::SetCover => {
            let prev: Option<(Option<Uuid>,)> =
                sqlx::query_as(r#"SELECT "cover_image_asset_id" FROM "users" WHERE "id" = $1"#)
                    .bind(owner_id)
                    .fetch_optional(pool)
                    .await
                    .map_err(|_| server_error())?;
            let Some((prev,)) = prev else {
                return Err(does_not_exist());
            };
            if let Some(prev_id) = prev {
                asset_delete(pool, &prev_id)
                    .await
                    .map_err(|_| server_error())?;
            }
            // `user.cover_image = None` (asymmetry with avatar — ported).
            sqlx::query(
                r#"UPDATE "users" SET "cover_image" = NULL, "cover_image_asset_id" = $1 WHERE "id" = $2"#,
            )
            .bind(asset_id)
            .bind(owner_id)
            .execute(pool)
            .await
            .map_err(|_| server_error())?;
            Ok(())
        }
        q::UserSaveAction::Noop => Ok(()),
    }
}

/// `UserAssetsV2Endpoint.entity_asset_delete` (`v2.py:80-107`): clears the
/// link FK only (the `avatar`/`cover_image` strings are untouched here).
#[allow(clippy::result_large_err)]
async fn user_entity_delete(
    pool: &sqlx::PgPool,
    entity_type: &str,
    owner_id: &Uuid,
) -> Result<(), Response> {
    match q::user_delete_action(entity_type) {
        q::UserDeleteAction::ClearAvatar => {
            let row: Option<(Uuid,)> =
                sqlx::query_as(r#"SELECT "id" FROM "users" WHERE "id" = $1"#)
                    .bind(owner_id)
                    .fetch_optional(pool)
                    .await
                    .map_err(|_| server_error())?;
            row.ok_or_else(does_not_exist)?;
            sqlx::query(r#"UPDATE "users" SET "avatar_asset_id" = NULL WHERE "id" = $1"#)
                .bind(owner_id)
                .execute(pool)
                .await
                .map_err(|_| server_error())?;
            Ok(())
        }
        q::UserDeleteAction::ClearCover => {
            let row: Option<(Uuid,)> =
                sqlx::query_as(r#"SELECT "id" FROM "users" WHERE "id" = $1"#)
                    .bind(owner_id)
                    .fetch_optional(pool)
                    .await
                    .map_err(|_| server_error())?;
            row.ok_or_else(does_not_exist)?;
            sqlx::query(r#"UPDATE "users" SET "cover_image_asset_id" = NULL WHERE "id" = $1"#)
                .bind(owner_id)
                .execute(pool)
                .await
                .map_err(|_| server_error())?;
            Ok(())
        }
        q::UserDeleteAction::Noop => Ok(()),
    }
}

/// `WorkspaceFileAssetEndpoint.entity_asset_save` (`v2.py:247-284`).
#[allow(clippy::result_large_err)]
async fn workspace_entity_save(
    pool: &sqlx::PgPool,
    asset_id: &Uuid,
    entity_type: &str,
    asset: &AssetRow,
) -> Result<(), Response> {
    match q::workspace_save_action(entity_type) {
        q::WorkspaceSaveAction::SetLogo => {
            // `filter(id=None).first()` is `None` → silent return.
            let Some(workspace_id) = asset.workspace_id else {
                return Ok(());
            };
            // `.filter(id).first()` — miss returns silently.
            let row: Option<(Option<Uuid>,)> = sqlx::query_as(
                r#"SELECT "logo_asset_id" FROM "workspaces" WHERE "id" = $1 AND "deleted_at" IS NULL"#,
            )
            .bind(workspace_id)
            .fetch_optional(pool)
            .await
            .map_err(|_| server_error())?;
            let Some((prev,)) = row else { return Ok(()) };
            if let Some(prev_id) = prev {
                asset_delete(pool, &prev_id)
                    .await
                    .map_err(|_| server_error())?;
            }
            sqlx::query(
                r#"UPDATE "workspaces" SET "logo" = $1, "logo_asset_id" = $2 WHERE "id" = $3"#,
            )
            .bind("")
            .bind(asset_id)
            .bind(workspace_id)
            .execute(pool)
            .await
            .map_err(|_| server_error())?;
            Ok(())
        }
        q::WorkspaceSaveAction::SetCover => {
            let Some(project_id) = asset.project_id else {
                return Ok(());
            };
            let row: Option<(Option<Uuid>,)> = sqlx::query_as(
                r#"SELECT "cover_image_asset_id" FROM "projects" WHERE "id" = $1 AND "deleted_at" IS NULL"#,
            )
            .bind(project_id)
            .fetch_optional(pool)
            .await
            .map_err(|_| server_error())?;
            let Some((prev,)) = row else { return Ok(()) };
            if let Some(prev_id) = prev {
                asset_delete(pool, &prev_id)
                    .await
                    .map_err(|_| server_error())?;
            }
            sqlx::query(
                r#"UPDATE "projects" SET "cover_image" = $1, "cover_image_asset_id" = $2 WHERE "id" = $3"#,
            )
            .bind("")
            .bind(asset_id)
            .bind(project_id)
            .execute(pool)
            .await
            .map_err(|_| server_error())?;
            Ok(())
        }
        q::WorkspaceSaveAction::Noop => Ok(()),
    }
}

/// `WorkspaceFileAssetEndpoint.entity_asset_delete` (`v2.py:286-312`).
#[allow(clippy::result_large_err)]
async fn workspace_entity_delete(
    pool: &sqlx::PgPool,
    entity_type: &str,
    asset: &AssetRow,
) -> Result<(), Response> {
    match q::workspace_delete_action(entity_type) {
        q::WorkspaceDeleteAction::ClearLogo => {
            // `.get(id=None)` raises → the 404 envelope.
            let Some(workspace_id) = asset.workspace_id else {
                return Err(does_not_exist());
            };
            // `.get(id)` through the scoped manager — miss raises (the
            // `if workspace is None` check after it is dead code; BUG
            // ported). The scope covers soft-deleted rows too, so a
            // deleted workspace answers 404 with the asset untouched.
            let row: Option<(Uuid,)> = sqlx::query_as(
                r#"SELECT "id" FROM "workspaces" WHERE "id" = $1 AND "deleted_at" IS NULL"#,
            )
            .bind(workspace_id)
            .fetch_optional(pool)
            .await
            .map_err(|_| server_error())?;
            row.ok_or_else(does_not_exist)?;
            sqlx::query(r#"UPDATE "workspaces" SET "logo_asset_id" = NULL WHERE "id" = $1"#)
                .bind(workspace_id)
                .execute(pool)
                .await
                .map_err(|_| server_error())?;
            Ok(())
        }
        q::WorkspaceDeleteAction::ClearCover => {
            let Some(project_id) = asset.project_id else {
                return Ok(());
            };
            let row: Option<(Option<Uuid>,)> = sqlx::query_as(
                r#"SELECT "cover_image_asset_id" FROM "projects" WHERE "id" = $1 AND "deleted_at" IS NULL"#,
            )
            .bind(project_id)
            .fetch_optional(pool)
            .await
            .map_err(|_| server_error())?;
            let Some(_) = row else { return Ok(()) };
            sqlx::query(r#"UPDATE "projects" SET "cover_image_asset_id" = NULL WHERE "id" = $1"#)
                .bind(project_id)
                .execute(pool)
                .await
                .map_err(|_| server_error())?;
            Ok(())
        }
        q::WorkspaceDeleteAction::Noop => Ok(()),
    }
}

// ---------------------------------------------------------------------------
// INSERT (Django ORM emits the full column list)
// ---------------------------------------------------------------------------

/// POST INSERT mirroring `FileAsset.objects.create(...)`: every column the
/// ORM sends, with Django's defaults for the untouched ones
/// (`is_deleted/is_archived/is_uploaded=false`, `storage_metadata='{}'`).
/// Binds: `$1` id, `$2` created_at, `$3` updated_at, `$4` attributes,
/// `$5` asset key, `$6` size (float), `$7` user_id, `$8` workspace_id,
/// `$9` project_id, `$10` issue_id, `$11` comment_id, `$12` page_id,
/// `$13` created_by_id, `$14` entity_type.
fn asset_insert_sql() -> String {
    r#"INSERT INTO "file_assets" ("id", "created_at", "updated_at", "attributes", "asset", "size", "user_id", "workspace_id", "project_id", "issue_id", "comment_id", "page_id", "created_by_id", "entity_type", "is_deleted", "is_archived", "is_uploaded", "storage_metadata") VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, false, false, false, '{}')"#
        .to_string()
}

// ---------------------------------------------------------------------------
// UserAssetsV2Endpoint
// ---------------------------------------------------------------------------

/// `UserAssetsV2Endpoint.post` (`v2.py:109-168`).
async fn user_post(
    State(state): State<AppState>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let actor = match require_user(&state, extension).await {
        Ok(actor) => actor,
        Err(denial) => return denial.into_response(),
    };
    let Some(pool) = pool(&state) else {
        return server_error();
    };
    let data = match parse_data(content_type(&headers).as_deref(), &body) {
        Ok(data) => data,
        Err(response) => return response,
    };
    // `size = int(...)` raises before either validation (`v2.py:113`).
    let size = match parse_size(data.get("size"), state.settings().file_size_limit) {
        Some(size) => size,
        None => return server_error(),
    };
    // `if not entity_type or entity_type not in [...]` (`v2.py:120`): a
    // hardcoded 2-value list, NOT `EntityTypeContext.values`.
    let entity_type = match data.get("entity_type") {
        Some(Value::String(s)) => s.clone(),
        _ => String::new(),
    };
    if q::validate_user_entity_type(Some(entity_type.as_str())).is_err() {
        return json_status(StatusCode::BAD_REQUEST, q::invalid_entity_type_body());
    }
    let Some(mime) = post_mime(&data) else {
        return json_status(StatusCode::BAD_REQUEST, q::invalid_file_type_body());
    };
    if q::validate_file_type(mime).is_err() {
        return json_status(StatusCode::BAD_REQUEST, q::invalid_file_type_body());
    }
    let size_limit = q::clamp_size(size, state.settings().file_size_limit);
    let (name_display, name_json) = python_str(data.get("name"));
    let asset_id = Uuid::new_v4();
    // `asset_key = f"{uuid.uuid4().hex}-{name}"` (`v2.py:144`): bare
    // `<hex>-<name>`, no workspace prefix (`q::user_asset_key` shape, with
    // the `str(name)` rendering `name_display` already carries).
    let asset_key = format!("{}-{name_display}", asset_id.simple());
    let attributes = serde_json::json!({
        "name": name_json,
        "type": mime,
        "size": size_limit,
    });
    let now = Utc::now();
    if sqlx::query(&asset_insert_sql())
        .bind(asset_id)
        .bind(now)
        .bind(now)
        .bind(sqlx::types::Json(attributes))
        .bind(&asset_key)
        .bind(size_limit as f64)
        .bind(actor.id)
        .bind(None::<Uuid>)
        .bind(None::<Uuid>)
        .bind(None::<Uuid>)
        .bind(None::<Uuid>)
        .bind(None::<Uuid>)
        .bind(actor.id)
        .bind(entity_type.as_str())
        .execute(pool)
        .await
        .is_err()
    {
        return server_error();
    }
    let Some(host) = host_of(&headers) else {
        return server_error();
    };
    let storage = &state.settings().storage;
    let Ok(upload_data) = presigned_post(
        storage,
        &scheme_of(&headers),
        &host,
        &asset_key,
        mime,
        size_limit,
        &now,
    ) else {
        return server_error();
    };
    json_status(
        StatusCode::OK,
        serde_json::json!({
            "upload_data": upload_data,
            "asset_id": asset_id.to_string(),
            "asset_url": static_asset_url(&asset_id),
        }),
    )
}

/// `UserAssetsV2Endpoint.patch` (`v2.py:170-189`).
async fn user_patch(
    State(state): State<AppState>,
    Path(asset_raw): Path<String>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let asset_id = match parse_asset_id(&asset_raw) {
        Ok(id) => id,
        Err(response) => return response,
    };
    let actor = match require_user(&state, extension).await {
        Ok(actor) => actor,
        Err(denial) => return denial.into_response(),
    };
    let Some(pool) = pool(&state) else {
        return server_error();
    };
    let row = match sqlx::query(&q::user_asset_lookup_sql())
        .bind(asset_id)
        .bind(actor.id)
        .fetch_optional(pool)
        .await
    {
        Ok(row) => row,
        Err(_) => return server_error(),
    };
    let Some(row) = row else {
        return does_not_exist();
    };
    let asset = match asset_row(&row) {
        Ok(asset) => asset,
        Err(_) => return server_error(),
    };
    // `is_uploaded = True` is set before the metadata check
    // (`v2.py:174-177`).
    if q::should_publish_metadata(asset.storage_metadata.as_ref()) {
        publish_metadata(pool, &asset_id).await;
    }
    let entity_type = asset.entity_type.clone().unwrap_or_default();
    if let Err(response) = user_entity_save(pool, &asset_id, &entity_type, &actor.id).await {
        return response;
    }
    let data = match parse_data(content_type(&headers).as_deref(), &body) {
        Ok(data) => data,
        Err(response) => return response,
    };
    // `request.data.get("attributes", asset.attributes)`.
    let attributes = data
        .get("attributes")
        .cloned()
        .unwrap_or_else(|| asset.attributes.clone().unwrap_or(Value::Null));
    // `attributes = None` violates the NOT NULL column → IntegrityError.
    if attributes.is_null() {
        return raw(StatusCode::BAD_REQUEST, PAYLOAD_NOT_VALID_BODY);
    }
    if sqlx::query(&q::confirm_update_sql())
        .bind(true)
        .bind(sqlx::types::Json(attributes))
        .bind(asset_id)
        .execute(pool)
        .await
        .is_err()
    {
        return server_error();
    }
    no_content()
}

/// `UserAssetsV2Endpoint.delete` (`v2.py:191-198`).
async fn user_delete(
    State(state): State<AppState>,
    Path(asset_raw): Path<String>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Response {
    let asset_id = match parse_asset_id(&asset_raw) {
        Ok(id) => id,
        Err(response) => return response,
    };
    let actor = match require_user(&state, extension).await {
        Ok(actor) => actor,
        Err(denial) => return denial.into_response(),
    };
    let Some(pool) = pool(&state) else {
        return server_error();
    };
    let row = match sqlx::query(&q::user_asset_lookup_sql())
        .bind(asset_id)
        .bind(actor.id)
        .fetch_optional(pool)
        .await
    {
        Ok(row) => row,
        Err(_) => return server_error(),
    };
    let Some(row) = row else {
        return does_not_exist();
    };
    let asset = match asset_row(&row) {
        Ok(asset) => asset,
        Err(_) => return server_error(),
    };
    let entity_type = asset.entity_type.clone().unwrap_or_default();
    if let Err(response) = user_entity_delete(pool, &entity_type, &actor.id).await {
        return response;
    }
    if sqlx::query(&q::delete_update_sql())
        .bind(true)
        .bind(Utc::now())
        .bind(asset_id)
        .execute(pool)
        .await
        .is_err()
    {
        return server_error();
    }
    no_content()
}

// ---------------------------------------------------------------------------
// WorkspaceFileAssetEndpoint
// ---------------------------------------------------------------------------

/// Resolve `slug` the way `Workspace.objects.get(slug=slug)` does
/// (`v2.py:349`): miss raises → the 404 envelope.
#[allow(clippy::result_large_err)]
async fn workspace_id_for(pool: &sqlx::PgPool, slug: &str) -> Result<Uuid, Response> {
    let row: Option<(Uuid,)> = sqlx::query_as(
        r#"SELECT "id" FROM "workspaces" WHERE "slug" = $1 AND "deleted_at" IS NULL"#,
    )
    .bind(slug)
    .fetch_optional(pool)
    .await
    .map_err(|_| server_error())?;
    row.map(|row| row.0).ok_or_else(does_not_exist)
}

/// The `**get_entity_id_field(...)` spread value (`v2.py:362`): the raw
/// identifier text (Postgres casts valid UUID strings; anything else
/// raises → 500, same as Django), `None` for an explicit null, and the
/// `MissingBecomesFalse` bug for an absent key (boolean into a uuid
/// column → 500, `v2.py:319` ported as-is).
enum EntityIdValue {
    Text(String),
    Null,
    Missing,
}

fn entity_id_value(data: &Map<String, Value>) -> EntityIdValue {
    match data.get("entity_identifier") {
        None => EntityIdValue::Missing,
        Some(Value::Null) => EntityIdValue::Null,
        Some(Value::String(s)) => EntityIdValue::Text(s.clone()),
        Some(_) => EntityIdValue::Missing,
    }
}

/// `WorkspaceFileAssetEndpoint.post` (`v2.py:314-377`).
#[allow(clippy::too_many_lines)]
async fn workspace_post(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let actor = match require_user(&state, extension).await {
        Ok(actor) => actor,
        Err(denial) => return denial.into_response(),
    };
    let Some(pool) = pool(&state) else {
        return server_error();
    };
    let data = match parse_data(content_type(&headers).as_deref(), &body) {
        Ok(data) => data,
        Err(response) => return response,
    };
    // `size = int(...)` raises before either validation (`v2.py:317`).
    let size = match parse_size(data.get("size"), state.settings().file_size_limit) {
        Some(size) => size,
        None => return server_error(),
    };
    // `if entity_type not in FileAsset.EntityTypeContext.values`
    // (`v2.py:322`): the full 10-value set.
    let entity_type = match data.get("entity_type") {
        Some(Value::String(s)) => s.clone(),
        _ => String::new(),
    };
    if q::validate_workspace_entity_type(Some(entity_type.as_str())).is_err() {
        return json_status(StatusCode::BAD_REQUEST, q::invalid_entity_type_body());
    }
    let Some(mime) = post_mime(&data) else {
        return json_status(StatusCode::BAD_REQUEST, q::invalid_file_type_body());
    };
    if q::validate_file_type(mime).is_err() {
        return json_status(StatusCode::BAD_REQUEST, q::invalid_file_type_body());
    }
    let size_limit = q::clamp_size(size, state.settings().file_size_limit);
    let workspace_id = match workspace_id_for(pool, &slug).await {
        Ok(id) => id,
        Err(response) => return response,
    };
    let (name_display, name_json) = python_str(data.get("name"));
    let asset_id = Uuid::new_v4();
    let asset_key = format!("{workspace_id}/{}-{name_display}", asset_id.simple());
    let attributes = serde_json::json!({
        "name": name_json,
        "type": mime,
        "size": size_limit,
    });
    // The spread (`v2.py:362`): for `WORKSPACE_LOGO` the identifier
    // overwrites `workspace=workspace` (later kwarg wins); every other
    // branch fills its own FK column. A missing key spreads `False`.
    let fk_col = q::get_entity_id_field(entity_type.as_str());
    let id_value = entity_id_value(&data);
    // `asset_url`'s `{issue_id}` (`asset.py:90`): the spread identifier
    // for the two issue entity types, `None` rendered otherwise (an
    // explicit-null identifier stores NULL; anything invalid 500s below).
    let issue_display: &str = match (&entity_type as &str, &id_value) {
        ("ISSUE_ATTACHMENT" | "ISSUE_DESCRIPTION", EntityIdValue::Text(s)) => s,
        _ => "None",
    };
    // Resolve each FK column: `None` binds NULL; text parses to `Uuid`
    // (an unparseable identifier fails the uuid column in Django too →
    // 500); the missing-key bug answers 500 with no row. Binds are typed
    // `Uuid` because Postgres casts no explicitly-typed text to uuid.
    let mut user_fk: Option<Option<Uuid>> = None;
    let mut workspace_fk: Option<Option<Uuid>> = None;
    let mut project_fk: Option<Option<Uuid>> = None;
    let mut issue_fk: Option<Option<Uuid>> = None;
    let mut comment_fk: Option<Option<Uuid>> = None;
    let mut page_fk: Option<Option<Uuid>> = None;
    if let Some(col) = fk_col {
        let value = match &id_value {
            EntityIdValue::Text(s) => match s.parse::<Uuid>() {
                Ok(id) => Some(id),
                Err(_) => return server_error(),
            },
            EntityIdValue::Null => None,
            EntityIdValue::Missing => return server_error(),
        };
        match col {
            "workspace_id" => workspace_fk = Some(value),
            "project_id" => project_fk = Some(value),
            "user_id" => user_fk = Some(value),
            "issue_id" => issue_fk = Some(value),
            "comment_id" => comment_fk = Some(value),
            "page_id" => page_fk = Some(value),
            _ => {}
        }
    }
    // BUG-workspace-id-overwrite: `workspace=workspace` then
    // `workspace_id=identifier` — the spread wins for `WORKSPACE_LOGO`.
    let row_workspace_id: Option<Uuid> = match workspace_fk {
        Some(v) => v,
        None => Some(workspace_id),
    };
    let now = Utc::now();
    if sqlx::query(&asset_insert_sql())
        .bind(asset_id)
        .bind(now)
        .bind(now)
        .bind(sqlx::types::Json(attributes))
        .bind(&asset_key)
        .bind(size_limit as f64)
        .bind(user_fk.unwrap_or(None))
        .bind(row_workspace_id)
        .bind(project_fk.unwrap_or(None))
        .bind(issue_fk.unwrap_or(None))
        .bind(comment_fk.unwrap_or(None))
        .bind(page_fk.unwrap_or(None))
        .bind(actor.id)
        .bind(entity_type.as_str())
        .execute(pool)
        .await
        .is_err()
    {
        return server_error();
    }
    let Some(host) = host_of(&headers) else {
        return server_error();
    };
    let storage = &state.settings().storage;
    let Ok(upload_data) = presigned_post(
        storage,
        &scheme_of(&headers),
        &host,
        &asset_key,
        mime,
        size_limit,
        &now,
    ) else {
        return server_error();
    };
    json_status(
        StatusCode::OK,
        serde_json::json!({
            "upload_data": upload_data,
            "asset_id": asset_id.to_string(),
            "asset_url": workspace_asset_url(
                entity_type.as_str(),
                &slug,
                issue_display,
                &asset_id,
            ),
        }),
    )
}

/// `WorkspaceFileAssetEndpoint.patch` (`v2.py:379-398`).
async fn workspace_patch(
    State(state): State<AppState>,
    Path((slug, asset_raw)): Path<(String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let asset_id = match parse_asset_id(&asset_raw) {
        Ok(id) => id,
        Err(response) => return response,
    };
    let _actor = match require_user(&state, extension).await {
        Ok(actor) => actor,
        Err(denial) => return denial.into_response(),
    };
    let Some(pool) = pool(&state) else {
        return server_error();
    };
    let row = match sqlx::query(&q::workspace_asset_lookup_sql())
        .bind(asset_id)
        .bind(&slug)
        .fetch_optional(pool)
        .await
    {
        Ok(row) => row,
        Err(_) => return server_error(),
    };
    let Some(row) = row else {
        return does_not_exist();
    };
    let asset = match asset_row(&row) {
        Ok(asset) => asset,
        Err(_) => return server_error(),
    };
    if q::should_publish_metadata(asset.storage_metadata.as_ref()) {
        publish_metadata(pool, &asset_id).await;
    }
    let entity_type = asset.entity_type.clone().unwrap_or_default();
    if let Err(response) = workspace_entity_save(pool, &asset_id, &entity_type, &asset).await {
        return response;
    }
    let data = match parse_data(content_type(&headers).as_deref(), &body) {
        Ok(data) => data,
        Err(response) => return response,
    };
    let attributes = data
        .get("attributes")
        .cloned()
        .unwrap_or_else(|| asset.attributes.clone().unwrap_or(Value::Null));
    if attributes.is_null() {
        return raw(StatusCode::BAD_REQUEST, PAYLOAD_NOT_VALID_BODY);
    }
    if sqlx::query(&q::confirm_update_sql())
        .bind(true)
        .bind(sqlx::types::Json(attributes))
        .bind(asset_id)
        .execute(pool)
        .await
        .is_err()
    {
        return server_error();
    }
    no_content()
}

/// `WorkspaceFileAssetEndpoint.delete` (`v2.py:400-407`).
async fn workspace_delete(
    State(state): State<AppState>,
    Path((slug, asset_raw)): Path<(String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Response {
    let asset_id = match parse_asset_id(&asset_raw) {
        Ok(id) => id,
        Err(response) => return response,
    };
    let _actor = match require_user(&state, extension).await {
        Ok(actor) => actor,
        Err(denial) => return denial.into_response(),
    };
    let Some(pool) = pool(&state) else {
        return server_error();
    };
    let row = match sqlx::query(&q::workspace_asset_lookup_sql())
        .bind(asset_id)
        .bind(&slug)
        .fetch_optional(pool)
        .await
    {
        Ok(row) => row,
        Err(_) => return server_error(),
    };
    let Some(row) = row else {
        return does_not_exist();
    };
    let asset = match asset_row(&row) {
        Ok(asset) => asset,
        Err(_) => return server_error(),
    };
    let entity_type = asset.entity_type.clone().unwrap_or_default();
    if let Err(response) = workspace_entity_delete(pool, &entity_type, &asset).await {
        return response;
    }
    if sqlx::query(&q::delete_update_sql())
        .bind(true)
        .bind(Utc::now())
        .bind(asset_id)
        .execute(pool)
        .await
        .is_err()
    {
        return server_error();
    }
    no_content()
}

/// `WorkspaceFileAssetEndpoint.get` (`v2.py:409-429`).
async fn workspace_get(
    State(state): State<AppState>,
    Path((slug, asset_raw)): Path<(String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    headers: HeaderMap,
) -> Response {
    let asset_id = match parse_asset_id(&asset_raw) {
        Ok(id) => id,
        Err(response) => return response,
    };
    let _actor = match require_user(&state, extension).await {
        Ok(actor) => actor,
        Err(denial) => return denial.into_response(),
    };
    let Some(pool) = pool(&state) else {
        return server_error();
    };
    let row = match sqlx::query(&q::workspace_asset_lookup_sql())
        .bind(asset_id)
        .bind(&slug)
        .fetch_optional(pool)
        .await
    {
        Ok(row) => row,
        Err(_) => return server_error(),
    };
    let Some(row) = row else {
        return does_not_exist();
    };
    let asset = match asset_row(&row) {
        Ok(asset) => asset,
        Err(_) => return server_error(),
    };
    if !asset.is_uploaded {
        return json_status(StatusCode::NOT_FOUND, q::not_uploaded_body());
    }
    // `filename=asset.attributes.get("name")`: a NULL attributes column
    // raises (`AttributeError` → 500); a missing/non-string name falls
    // back to the fresh hex (`_get_content_disposition`).
    let attributes = match asset.attributes.as_ref() {
        Some(Value::Object(map)) => map,
        _ => return server_error(),
    };
    let filename = match attributes.get("name") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(s.clone()),
        Some(_) => return server_error(),
    };
    let Some(host) = host_of(&headers) else {
        return server_error();
    };
    let storage = &state.settings().storage;
    let Ok(url) = presigned_get_url(
        storage,
        &scheme_of(&headers),
        &host,
        &asset.asset_key,
        "attachment",
        filename,
        &Utc::now(),
    ) else {
        return server_error();
    };
    // `HttpResponseRedirect`: empty body, default content type.
    Response::builder()
        .status(StatusCode::FOUND)
        .header(header::LOCATION, url)
        .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
        .body(axum::body::Body::empty())
        .expect("redirect response")
}

// ---------------------------------------------------------------------------
// StaticFileAssetEndpoint
// ---------------------------------------------------------------------------

/// Entity types the static fetch serves (`v2.py:449-454`).
const STATIC_ENTITY_TYPES: &[&str] = &[
    "USER_AVATAR",
    "USER_COVER",
    "WORKSPACE_LOGO",
    "PROJECT_COVER",
];

/// `StaticFileAssetEndpoint.get` (`v2.py:437-465`): the only `AllowAny`
/// route in the domain — no auth of any kind.
async fn static_get(
    State(state): State<AppState>,
    Path(asset_raw): Path<String>,
    headers: HeaderMap,
) -> Response {
    let asset_id = match parse_asset_id(&asset_raw) {
        Ok(id) => id,
        Err(response) => return response,
    };
    let Some(pool) = pool(&state) else {
        return server_error();
    };
    // `FileAsset.objects.get(id)` — the default manager (soft-delete
    // scoped), no tenant predicate at all.
    let row = match sqlx::query(
        r#"SELECT "entity_type", "is_uploaded", "storage_metadata", "attributes", "asset", "workspace_id", "project_id" FROM "file_assets" WHERE "id" = $1 AND "deleted_at" IS NULL"#,
    )
    .bind(asset_id)
    .fetch_optional(pool)
    .await
    {
        Ok(row) => row,
        Err(_) => return server_error(),
    };
    let Some(row) = row else {
        return does_not_exist();
    };
    let asset = match asset_row(&row) {
        Ok(asset) => asset,
        Err(_) => return server_error(),
    };
    if !asset.is_uploaded {
        return json_status(StatusCode::NOT_FOUND, q::not_uploaded_body());
    }
    let entity_allowed = match asset.entity_type.as_deref() {
        Some(t) => STATIC_ENTITY_TYPES.contains(&t),
        None => false,
    };
    if !entity_allowed {
        return json_status(StatusCode::BAD_REQUEST, q::invalid_entity_type_body());
    }
    let Some(host) = host_of(&headers) else {
        return server_error();
    };
    let storage = &state.settings().storage;
    // `generate_presigned_url(object_name)` with the defaults:
    // `disposition="inline"`, `filename=None` → fresh hex.
    let Ok(url) = presigned_get_url(
        storage,
        &scheme_of(&headers),
        &host,
        &asset.asset_key,
        "inline",
        None,
        &Utc::now(),
    ) else {
        return server_error();
    };
    Response::builder()
        .status(StatusCode::FOUND)
        .header(header::LOCATION, url)
        .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
        .body(axum::body::Body::empty())
        .expect("redirect response")
}

// ---------------------------------------------------------------------------
// AssetRestoreEndpoint
// ---------------------------------------------------------------------------

/// `AssetRestoreEndpoint.post` (`v2.py:471-477`) behind
/// `@allow_permission([ADMIN, MEMBER, GUEST], level="WORKSPACE")`.
async fn restore_post(
    State(state): State<AppState>,
    Path((slug, asset_raw)): Path<(String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Response {
    let asset_id = match parse_asset_id(&asset_raw) {
        Ok(id) => id,
        Err(response) => return response,
    };
    let actor = match require_user(&state, extension).await {
        Ok(actor) => actor,
        Err(denial) => return denial.into_response(),
    };
    let Some(pool) = pool(&state) else {
        return server_error();
    };
    // The WORKSPACE gate (`app/permissions/base.py:45-51`): one EXISTS
    // probe over the soft-delete-scoped membership rows.
    let role: Option<(i16,)> = match sqlx::query_as(
        r#"SELECT wm.role FROM workspace_members wm JOIN workspaces w ON w.id = wm.workspace_id WHERE w.slug = $1 AND wm.member_id = $2 AND wm.is_active AND wm.deleted_at IS NULL LIMIT 1"#,
    )
    .bind(&slug)
    .bind(actor.id)
    .fetch_optional(pool)
    .await
    {
        Ok(role) => role,
        Err(_) => return server_error(),
    };
    let facts = AllowFacts {
        workspace: WorkspaceId::from(slug.as_str()),
        authenticated: true,
        is_workspace_member: role.is_some(),
        has_allowed_workspace_role: role.map(|(r,)| [20, 15, 5].contains(&r)).unwrap_or(false),
        is_creator: false,
        has_allowed_project_role: false,
        is_project_member: false,
        is_workspace_admin: false,
    };
    let scope = TenantScope::new(WorkspaceId::from(slug.as_str()));
    if !guards::check_asset_gate(guards::AssetEndpoint::RestorePost, &scope, &facts) {
        return raw(StatusCode::FORBIDDEN, PERMISSION_DENIED_BODY);
    }
    // `FileAsset.all_objects.get(id, workspace__slug)` (`v2.py:473`): the
    // unfiltered manager sees soft-deleted rows — no `deleted_at` conjunct.
    let row: Option<(Uuid,)> = match sqlx::query_as(
        r#"SELECT "file_assets"."id" FROM "file_assets" INNER JOIN "workspaces" U0 ON ("file_assets"."workspace_id" = U0."id") WHERE "file_assets"."id" = $1 AND U0."slug" = $2"#,
    )
    .bind(asset_id)
    .bind(&slug)
    .fetch_optional(pool)
    .await
    {
        Ok(row) => row,
        Err(_) => return server_error(),
    };
    if row.is_none() {
        return does_not_exist();
    }
    if sqlx::query(
        r#"UPDATE "file_assets" SET "is_deleted" = false, "deleted_at" = NULL WHERE "id" = $1"#,
    )
    .bind(asset_id)
    .execute(pool)
    .await
    .is_err()
    {
        return server_error();
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

/// `S3Storage.__init__` failure (`ValueError: Invalid endpoint: ...` /
/// `InvalidRegionError`): the region cannot be signed — not a valid
/// host label, or empty with no endpoint URL to derive from. Python
/// raises from the constructor, so every use site propagates it to the
/// 500 fallback (`handle_exception`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct InvalidEndpoint;

impl std::fmt::Display for InvalidEndpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("s3 region cannot be signed against the derived endpoint")
    }
}

impl std::error::Error for InvalidEndpoint {}

/// Endpoint + host/path split for signing, mirroring
/// `S3Storage.__init__` with a request (`is_server=False`):
/// MinIO mode signs `{scheme}://{Host}` path-style; an explicit
/// endpoint URL signs path-style against it; otherwise the
/// virtual-hosted AWS default. Presigned URLs resolve the global
/// endpoint for every region (`use_global_endpoint`,
/// `botocore/signers.py:859`); header auth keeps the regional host.
fn endpoint_parts(
    storage: &pidash_db::config::StorageSettings,
    scheme: &str,
    host: &str,
    presign: bool,
) -> Result<(String, String), InvalidEndpoint> {
    if !pidash_db::config::is_valid_region_name(&storage.region) {
        return Err(InvalidEndpoint);
    }
    // `MINIO_ENDPOINT_SSL=1` signs https in MinIO mode (`storage.py:46-51`).
    let scheme = storage.endpoint_protocol(scheme);
    if storage.use_minio {
        Ok((format!("{scheme}://{host}"), host.to_owned()))
    } else if let Some(endpoint) = storage.endpoint_url.as_deref().filter(|e| !e.is_empty()) {
        let endpoint = endpoint.trim_end_matches('/');
        let signed_host = endpoint
            .rsplit("://")
            .next()
            .unwrap_or(endpoint)
            .split('/')
            .next()
            .unwrap_or(endpoint);
        Ok((endpoint.to_owned(), signed_host.to_owned()))
    } else {
        let region = storage.region.as_str();
        if region.is_empty() {
            // botocore derives `https://s3..amazonaws.com` and rejects
            // it (`ValueError: Invalid endpoint`).
            return Err(InvalidEndpoint);
        }
        // botocore's s3 endpoint table serves us-east-1 from the global
        // endpoint (`s3.amazonaws.com`, no region infix); presigned
        // URLs use it for every region, header auth stays regional.
        let base = if presign || region == "us-east-1" {
            "s3.amazonaws.com".to_owned()
        } else {
            format!("s3.{region}.amazonaws.com")
        };
        Ok((
            format!("https://{}.{base}", storage.bucket_name),
            format!("{}.{base}", storage.bucket_name),
        ))
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
) -> Result<String, InvalidEndpoint> {
    let region = storage.region.as_str();
    let (endpoint, signed_host) = endpoint_parts(storage, scheme, host, true)?;
    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let date = now.format("%Y%m%d").to_string();
    let scope = credential_scope(&date, region);
    let credential = format!("{}/{}", storage.access_key_id, scope);
    let name = match filename {
        Some(name) => quote_filename(&name),
        None => Uuid::new_v4().simple().to_string(),
    };
    let content_disposition = format!("{disposition}; filename*=UTF-8''{name}");
    let mut params = [
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
    params.sort_by(|a, b| a.0.cmp(&b.0));
    let canonical_query = params
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
    Ok(format!(
        "{endpoint}{canonical_path}?{canonical_query}&X-Amz-Signature={signature}"
    ))
}

/// Path encoding for the canonical URI: slashes survive, every
/// segment is RFC 3986-encoded (botocore `quote(path, safe='/~')`…
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
) -> Result<Value, InvalidEndpoint> {
    let region = storage.region.as_str();
    let (endpoint, _) = endpoint_parts(storage, scheme, host, true)?;
    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let date = now.format("%Y%m%d").to_string();
    let scope = credential_scope(&date, region);
    let expiration = (*now + chrono::Duration::seconds(storage.signed_url_expiration_secs))
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();
    // Condition order mirrors `storage.py`: bucket, content-length
    // range, Content-Type, key — then the three signer conditions.
    // Serialized with CPython `json.dumps` default separators
    // (`, `, `: `, `ensure_ascii`) so the policy bytes — and hence the
    // signature S3 verifies — match botocore's.
    let credential = format!("{}/{}", storage.access_key_id, scope);
    // The client-level `generate_presigned_post` appends its own
    // `{"bucket"}` / `{"key"}` conditions after the caller's and before
    // the signer's (`botocore/signers.py`), so the policy carries nine.
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
    // `url`: path-style against a custom/MinIO endpoint, otherwise the
    // virtual-hosted bucket root with its trailing slash
    // (`https://uploads.s3.amazonaws.com/`).
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
    Ok(serde_json::json!({"url": url, "fields": fields}))
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

    fn test_storage() -> StorageSettings {
        StorageSettings {
            use_minio: true,
            minio_endpoint_ssl: false,
            access_key_id: "access-key".to_owned(),
            secret_access_key: "secret-key".to_owned(),
            bucket_name: "uploads".to_owned(),
            region: "us-east-1".to_owned(),
            endpoint_url: None,
            signed_url_expiration_secs: 3600,
        }
    }

    #[test]
    fn endpoint_parts_minio_ssl_and_global_east() {
        // `MINIO_ENDPOINT_SSL=1` signs https in MinIO mode (`storage.py:46-51`).
        let mut ssl = test_storage();
        ssl.minio_endpoint_ssl = true;
        assert_eq!(
            endpoint_parts(&ssl, "http", "h:9", true).expect("minio"),
            ("https://h:9".to_owned(), "h:9".to_owned())
        );
        // us-east-1 resolves to the global endpoint (botocore probe A/L).
        let mut aws = test_storage();
        aws.use_minio = false;
        assert_eq!(
            endpoint_parts(&aws, "http", "h:9", true).expect("aws"),
            (
                "https://uploads.s3.amazonaws.com".to_owned(),
                "uploads.s3.amazonaws.com".to_owned()
            )
        );
    }

    #[test]
    fn endpoint_parts_presign_global_and_invalid_region() {
        // Presigned URLs resolve the global endpoint for every region
        // (`use_global_endpoint`, botocore probe J); header auth keeps
        // the regional host (probe J HEAD).
        let mut regional = test_storage();
        regional.use_minio = false;
        regional.region = "eu-west-1".to_owned();
        assert_eq!(
            endpoint_parts(&regional, "http", "h:9", true).expect("presign"),
            (
                "https://uploads.s3.amazonaws.com".to_owned(),
                "uploads.s3.amazonaws.com".to_owned()
            )
        );
        assert_eq!(
            endpoint_parts(&regional, "http", "h:9", false).expect("header auth"),
            (
                "https://uploads.s3.eu-west-1.amazonaws.com".to_owned(),
                "uploads.s3.eu-west-1.amazonaws.com".to_owned()
            )
        );
        // Empty region with a derived endpoint fails (`ValueError`,
        // probe D); garbage regions fail everywhere
        // (`InvalidRegionError`).
        let mut empty = test_storage();
        empty.use_minio = false;
        empty.region = String::new();
        assert_eq!(
            endpoint_parts(&empty, "http", "h:9", true),
            Err(InvalidEndpoint)
        );
        let mut garbage = test_storage();
        garbage.region = "!!".to_owned();
        assert_eq!(
            endpoint_parts(&garbage, "http", "h:9", true),
            Err(InvalidEndpoint)
        );
    }

    fn test_now() -> DateTime<Utc> {
        chrono::DateTime::parse_from_rfc3339("2026-09-28T12:00:00Z")
            .expect("fixed time")
            .with_timezone(&Utc)
    }

    /// Top-level JSON key order of a rendered body (struct serialization
    /// emits declaration order even though a `Value` object would iterate
    /// alphabetically without `preserve_order`).
    fn rendered_keys(body: &Value) -> Vec<String> {
        let rendered = serde_json::to_string(body).expect("serializes");
        let mut keys = Vec::new();
        let mut depth = 0usize;
        let mut chars = rendered.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '{' => depth += 1,
                '}' => depth -= 1,
                '"' if depth == 1 => {
                    let mut key = String::new();
                    while let Some(&next) = chars.peek() {
                        chars.next();
                        if next == '"' {
                            break;
                        }
                        key.push(next);
                    }
                    if chars.peek() == Some(&':') {
                        keys.push(key);
                    }
                }
                _ => {}
            }
        }
        keys
    }

    #[test]
    fn error_bodies_are_byte_exact() {
        assert_eq!(
            UNAUTHENTICATED_BODY,
            r#"{"detail":"Authentication credentials were not provided."}"#
        );
        assert_eq!(
            NOT_FOUND_BODY,
            r#"{"error":"The required object does not exist."}"#
        );
        assert_eq!(
            SERVER_ERROR_BODY,
            r#"{"error":"Something went wrong please try again later"}"#
        );
        assert_eq!(PAGE_NOT_FOUND_BODY, r#"{"error":"Page not found."}"#);
        assert_eq!(
            PAYLOAD_NOT_VALID_BODY,
            r#"{"error":"The payload is not valid"}"#
        );
        assert_eq!(
            PERMISSION_DENIED_BODY,
            r#"{"error":"You don't have the required permissions."}"#
        );
        assert_eq!(
            q::not_uploaded_body(),
            serde_json::json!({"error": "The requested asset could not be found."})
        );
        assert_eq!(
            q::invalid_entity_type_body(),
            serde_json::json!({"error": "Invalid entity type.", "status": false})
        );
    }

    #[test]
    fn workspace_asset_url_branches() {
        let id: Uuid = "3f9a2c1e-9b1a-4d2e-8f3a-1a2b3c4d5e6f"
            .parse()
            .expect("uuid");
        // Image types take the static branch.
        for entity in [
            "WORKSPACE_LOGO",
            "USER_AVATAR",
            "USER_COVER",
            "PROJECT_COVER",
        ] {
            assert_eq!(
                workspace_asset_url(entity, "acme", "None", &id),
                Value::String(format!("/api/assets/v2/static/{id}/")),
                "{entity}"
            );
        }
        // The attachment branch renders the spread issue identifier.
        assert_eq!(
            workspace_asset_url(
                "ISSUE_ATTACHMENT",
                "acme",
                "11111111-1111-1111-1111-111111111111",
                &id
            ),
            Value::String(format!(
                "/api/assets/v2/workspaces/acme/projects/None/issues/11111111-1111-1111-1111-111111111111/attachments/{id}/"
            ))
        );
        // Description branches render no issue id.
        for entity in [
            "ISSUE_DESCRIPTION",
            "COMMENT_DESCRIPTION",
            "PAGE_DESCRIPTION",
            "DRAFT_ISSUE_DESCRIPTION",
        ] {
            assert_eq!(
                workspace_asset_url(entity, "acme", "None", &id),
                Value::String(format!(
                    "/api/assets/v2/workspaces/acme/projects/None/{id}/"
                )),
                "{entity}"
            );
        }
        // Anything else renders null.
        assert_eq!(
            workspace_asset_url("DRAFT_ISSUE_ATTACHMENT", "acme", "None", &id),
            Value::Null
        );
    }

    #[test]
    fn post_body_key_order_matches_drf() {
        let body = serde_json::json!({
            "upload_data": {"url": "u", "fields": {}},
            "asset_id": "id",
            "asset_url": "url",
        });
        assert_eq!(
            rendered_keys(&body),
            vec!["upload_data", "asset_id", "asset_url"]
        );
    }

    #[test]
    fn static_allowlist_is_the_four_image_types() {
        assert_eq!(
            STATIC_ENTITY_TYPES,
            &[
                "USER_AVATAR",
                "USER_COVER",
                "WORKSPACE_LOGO",
                "PROJECT_COVER"
            ]
        );
        // The static route is the one AllowAny gate in the domain.
        assert_eq!(
            guards::gate_for(guards::AssetEndpoint::StaticGet),
            guards::AssetGate::AllowAny
        );
        // Restore is the one WORKSPACE gate in this module.
        assert_eq!(
            guards::gate_for(guards::AssetEndpoint::RestorePost),
            guards::AssetGate::Workspace
        );
    }

    #[test]
    fn restore_gate_passes_member_roles_only() {
        use pidash_auth::permissions::allow::AllowFacts;
        use pidash_auth::scope::TenantScope;
        let scope = TenantScope::new(WorkspaceId::from("acme"));
        let facts = |role: Option<i16>| AllowFacts {
            workspace: WorkspaceId::from("acme"),
            authenticated: true,
            is_workspace_member: role.is_some(),
            has_allowed_workspace_role: role.map(|r| [20, 15, 5].contains(&r)).unwrap_or(false),
            is_creator: false,
            has_allowed_project_role: false,
            is_project_member: false,
            is_workspace_admin: false,
        };
        for role in [20, 15, 5] {
            assert!(guards::check_asset_gate(
                guards::AssetEndpoint::RestorePost,
                &scope,
                &facts(Some(role))
            ));
        }
        for role in [None, Some(1), Some(0)] {
            assert!(!guards::check_asset_gate(
                guards::AssetEndpoint::RestorePost,
                &scope,
                &facts(role)
            ));
        }
    }

    #[test]
    fn size_parsing_mirrors_int() {
        let limit = 5_242_880;
        assert_eq!(parse_size(None, limit), Some(limit));
        assert_eq!(parse_size(Some(&serde_json::json!(512)), limit), Some(512));
        // Floats truncate toward zero, like `int()`.
        assert_eq!(parse_size(Some(&serde_json::json!(3.7)), limit), Some(3));
        assert_eq!(parse_size(Some(&serde_json::json!(-3.7)), limit), Some(-3));
        // Bools are ints.
        assert_eq!(parse_size(Some(&serde_json::json!(true)), limit), Some(1));
        // Strings use CPython `int(s, 10)` rules.
        assert_eq!(
            parse_size(Some(&serde_json::json!(" 512 ")), limit),
            Some(512)
        );
        assert_eq!(
            parse_size(Some(&serde_json::json!("1_000")), limit),
            Some(1000)
        );
        // Everything else raises → 500.
        for bad in [
            serde_json::json!(null),
            serde_json::json!("abc"),
            serde_json::json!(""),
            serde_json::json!([1]),
            serde_json::json!({"n": 1}),
        ] {
            assert_eq!(parse_size(Some(&bad), limit), None, "{bad}");
        }
        assert_eq!(parse_size(Some(&serde_json::json!(f64::NAN)), limit), None);
        assert_eq!(
            parse_size(Some(&serde_json::json!(f64::INFINITY)), limit),
            None
        );
    }

    #[test]
    fn name_rendering_mirrors_str() {
        assert_eq!(python_str(None), ("None".to_owned(), Value::Null));
        assert_eq!(
            python_str(Some(&Value::Null)),
            ("None".to_owned(), Value::Null)
        );
        assert_eq!(
            python_str(Some(&serde_json::json!("a.png"))),
            ("a.png".to_owned(), serde_json::json!("a.png"))
        );
        assert_eq!(
            python_str(Some(&serde_json::json!(5))),
            ("5".to_owned(), serde_json::json!(5))
        );
        assert_eq!(
            python_str(Some(&serde_json::json!(true))),
            ("True".to_owned(), serde_json::json!(true))
        );
    }

    #[test]
    fn mime_default_applies_only_when_absent() {
        let mut data = Map::new();
        assert_eq!(post_mime(&data), Some("image/jpeg"));
        data.insert("type".to_owned(), serde_json::json!("image/png"));
        assert_eq!(post_mime(&data), Some("image/png"));
        // Present non-strings fail the allowlist (400), never the default.
        for bad in [
            serde_json::json!(null),
            serde_json::json!(5),
            serde_json::json!(true),
        ] {
            data.insert("type".to_owned(), bad);
            assert_eq!(post_mime(&data), None);
        }
    }

    #[test]
    fn parse_data_shapes() {
        // Empty → `{}` for every content type.
        assert_eq!(
            parse_data(Some("application/json"), b"").expect("empty"),
            Map::new()
        );
        let map = parse_data(Some("application/json"), br#"{"name":"a.png"}"#).expect("json");
        assert_eq!(map.get("name"), Some(&serde_json::json!("a.png")));
        // Malformed JSON → 400; a JSON list → 500-shaped error.
        assert!(parse_data(Some("application/json"), b"{oops").is_err());
        assert!(parse_data(Some("application/json"), b"[1]").is_err());
        // Urlencoded: last value wins, `+` is a space.
        let map = parse_data(
            Some("application/x-www-form-urlencoded"),
            b"name=a+b.png&name=c.png",
        )
        .expect("form");
        assert_eq!(map.get("name"), Some(&serde_json::json!("c.png")));
        // Multipart text fields.
        let body =
            b"--b\r\nContent-Disposition: form-data; name=\"name\"\r\n\r\na.png\r\n--b--\r\n";
        let map = parse_data(Some("multipart/form-data; boundary=b"), body).expect("mp");
        assert_eq!(map.get("name"), Some(&serde_json::json!("a.png")));
    }

    #[test]
    fn entity_identifier_spread_values() {
        let mut data = Map::new();
        assert!(matches!(entity_id_value(&data), EntityIdValue::Missing));
        data.insert("entity_identifier".to_owned(), Value::Null);
        assert!(matches!(entity_id_value(&data), EntityIdValue::Null));
        data.insert(
            "entity_identifier".to_owned(),
            serde_json::json!("3f9a2c1e-9b1a-4d2e-8f3a-1a2b3c4d5e6f"),
        );
        assert!(matches!(entity_id_value(&data), EntityIdValue::Text(_)));
        // Non-string scalars take the missing (500) path, like the `False`
        // default — Postgres would reject them either way.
        data.insert("entity_identifier".to_owned(), serde_json::json!(5));
        assert!(matches!(entity_id_value(&data), EntityIdValue::Missing));
    }

    #[test]
    fn bad_uuid_answers_resolver_404() {
        assert!(parse_asset_id("not-a-uuid").is_err());
        assert!(parse_asset_id("3f9a2c1e-9b1a-4d2e-8f3a-1a2b3c4d5e6f").is_ok());
    }

    #[test]
    fn metadata_task_name_and_kwargs_shape() {
        assert_eq!(
            TASK_GET_ASSET_OBJECT_METADATA,
            "pi_dash.bgtasks.storage_metadata_task.get_asset_object_metadata"
        );
        assert_eq!(
            TASK_GET_ASSET_OBJECT_METADATA,
            pidash_services::app_assets::tasks::TASK_GET_ASSET_OBJECT_METADATA
        );
    }

    #[test]
    fn presigned_post_field_order_matches_botocore() {
        let post = presigned_post(
            &test_storage(),
            "http",
            "127.0.0.1:8486",
            "wid/ab12-avatar.png",
            "image/png",
            512,
            &test_now(),
        )
        .expect("minio signs");
        let obj = post.as_object().expect("object");
        assert_eq!(rendered_keys(&post), vec!["url", "fields"]);
        let fields = obj.get("fields").expect("fields");
        assert_eq!(
            rendered_keys(fields),
            vec![
                "Content-Type",
                "key",
                "x-amz-algorithm",
                "x-amz-credential",
                "x-amz-date",
                "policy",
                "x-amz-signature"
            ]
        );
        assert_eq!(
            post.get("url").expect("url"),
            "http://127.0.0.1:8486/uploads"
        );
        // Policy conditions carry the storage.py order; the
        // content-length-range caps at the clamped size.
        let policy_b64 = fields.get("policy").expect("policy").as_str().expect("str");
        let decoded = engine_decode(policy_b64);
        let policy: Value = serde_json::from_slice(&decoded).expect("policy json");
        let conditions = policy.get("conditions").expect("conditions");
        // (`Value` display is compact: no spaces after `,`/`:`.)
        assert!(conditions
            .to_string()
            .contains("\"content-length-range\",1,512"));
        assert!(conditions
            .to_string()
            .starts_with(r#"[{"bucket":"uploads"}"#));
    }

    /// Minimal standard-base64 decode for the policy assertion above
    /// (test-only; the module ships its own encoder).
    fn engine_decode(padded: &str) -> Vec<u8> {
        const ALPHABET: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let pad = padded
            .bytes()
            .rev()
            .take_while(|&b| b == b'=')
            .count()
            .min(2);
        let mut vals = Vec::new();
        for b in padded.bytes() {
            if b == b'=' {
                vals.push(0);
            } else if let Some(i) = ALPHABET.iter().position(|&a| a == b) {
                vals.push(i as u32);
            }
        }
        let mut out = Vec::new();
        for q in vals.chunks(4) {
            if q.len() < 4 {
                break;
            }
            let n = (q[0] << 18) | (q[1] << 12) | (q[2] << 6) | q[3];
            out.push((n >> 16) as u8);
            out.push((n >> 8) as u8);
            out.push(n as u8);
        }
        out.truncate(out.len().saturating_sub(pad));
        out
    }

    #[test]
    fn presigned_get_carries_disposition_and_key() {
        // Workspace fetch: attachment + stored filename.
        let url = presigned_get_url(
            &test_storage(),
            "http",
            "127.0.0.1:8486",
            "wid/ab12-logo.png",
            "attachment",
            Some("logo.png".to_owned()),
            &test_now(),
        )
        .expect("minio signs");
        assert!(url.starts_with("http://127.0.0.1:8486/uploads/wid/ab12-logo.png?"));
        assert!(url.contains("response-content-disposition=attachment"));
        assert!(url.contains("logo.png"));
        // Static fetch: inline + fresh hex (no filename passes through).
        let first = presigned_get_url(
            &test_storage(),
            "http",
            "127.0.0.1:8486",
            "ab12-avatar.png",
            "inline",
            None,
            &test_now(),
        )
        .expect("minio signs");
        let second = presigned_get_url(
            &test_storage(),
            "http",
            "127.0.0.1:8486",
            "ab12-avatar.png",
            "inline",
            None,
            &test_now(),
        )
        .expect("minio signs");
        assert!(first.contains("response-content-disposition=inline"));
        assert_ne!(first, second, "fresh hex per call");
    }

    #[test]
    fn filename_quoting_matches_quote() {
        assert_eq!(quote_filename("logo image.png"), "logo%20image.png");
        assert_eq!(quote_filename("a/b.png"), "a/b.png");
        assert_eq!(quote_filename("ünï.png"), "%C3%BCn%C3%AF.png");
    }

    #[test]
    fn base64_encoder_matches_standard_vectors() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
    }
}
