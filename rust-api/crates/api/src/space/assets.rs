//! S3 asset endpoints (D-02, stage 4): `EntityAssetEndpoint`
//! (GET/POST/PATCH/DELETE), `AssetRestoreEndpoint` (POST) and
//! `EntityBulkAssetEndpoint` (POST).
//!
//! Port of `apps/api/pi_dash/space/views/asset.py:26-226` with routes
//! from `apps/api/pi_dash/space/urls/asset.py:16-35`. Query text comes
//! from [`pidash_services::space::queries::intake_assets`] (builders used
//! verbatim, `$N` params bound in documented order), exact error bodies
//! from [`pidash_services::space::guards`], and the patch metadata task
//! from [`pidash_jobs::space::asset_metadata_message`]. Session auth
//! reuses [`crate::license::resolve_actor`]; anything else is handler
//! work documented below.
//!
//! Method matrix (`views/asset.py:27-32`): GET is `AllowAny`, every
//! mutation is `IsAuthenticated` (restore/bulk inherit the
//! `BaseAPIView` default). Anonymous on a mutation answers DRF
//! `NotAuthenticated`: 401
//! `{"detail":"Authentication credentials were not provided."}`.
//!
//! Django-idiom notes (translate, don't redesign):
//!
//! * Routes use `<uuid:pk>` / `<uuid:entity_id>` converters, so a
//!   non-UUID segment never reaches the view: the resolver 404s with
//!   `{"error":"Page not found."}` (global `handler404`,
//!   `pi_dash/urls.py:15`). Bad-UUID params answer that body here.
//! * `.get()` misses raise `DoesNotExist`, which the view's own
//!   `handle_exception` (`views/base.py:149-186`, reached through DRF
//!   `dispatch`) maps to 404
//!   `{"error":"The required object does not exist."}` — the envelope,
//!   not the endpoint's inline 404. Scoped-lookup misses (cross-tenant
//!   ids, wrong entity types, unknown restore targets) all answer it.
//! * `size=int(data.get("size", FILE_SIZE_LIMIT))` (`:78`): missing →
//!   `5242880`, present non-numeric (or explicit null) → `ValueError` →
//!   the 500 fallback. No max check — the default is the only limit.
//! * `asset_key = f"{workspace_id}/{uuid4hex}-{name}"` (`:107`): `name`
//!   missing renders as `None`, exactly like the f-string.
//! * `comment_id=entity_identifier` unconditionally (`:110-119`), even
//!   for non-comment entity types. A non-UUID identifier reaches the
//!   uuid column and the database raises → 500, same as Django.
//! * Bulk reassigns `comment_id` only when the first asset's type is
//!   `COMMENT_DESCRIPTION`; every other type silently no-ops yet still
//!   204s (`:223-226`).
//! * Restore reads through `all_objects` (`:183`): the unfiltered
//!   manager sees soft-deleted rows, so its SELECT has no
//!   `deleted_at IS NULL` conjunct.
//! * GET answers `HttpResponseRedirect`, not a DRF `Response` (`:66`):
//!   302 with a `Location` header and an empty body.
//! * POST answers 200 (not 201) with keys `upload_data`, `asset_id`,
//!   `asset_url` in that order (`:126-133`).
//!
//! S3 presigning mirrors `S3Storage` (`settings/storage.py`): offline
//! SigV4 (no network), `USE_MINIO` mode signing against
//! `{scheme}://{Host}` (scheme from `X-Forwarded-Proto`, default
//! `http`; the `MINIO_ENDPOINT_SSL` switch lives in Django-only config
//! and is not visible to this layer), otherwise against the configured
//! endpoint URL or the virtual-hosted AWS default. The GET redirect
//! carries `response-content-disposition=inline; filename*=UTF-8''<hex>`
//! with a fresh uuid4 hex per request
//! (`_get_content_disposition("inline", None)`); the POST policy lists
//! conditions in `storage.py` order (bucket, content-length-range,
//! Content-Type, key) plus the three `x-amz-*` signer conditions, and
//! `fields` keeps botocore's insertion order (caller fields, then
//! `x-amz-algorithm`, `x-amz-credential`, `x-amz-date`, `policy`,
//! `x-amz-signature`).
//!
//! Ported bugs (also listed in the PR):
//!
//! * BUG-bulk-silent-noop (`views/asset.py:223-226`): bulk updates
//!   nothing for non-`COMMENT_DESCRIPTION` first assets yet 204s.
//! * BUG-unconditional-comment-id (`:118`): `entity_identifier` is
//!   stored into `comment_id` for every entity type.
//! * BUG-size-no-max (`:78`): `size` has no upper check; only the
//!   missing-value default exists.
//! * BUG-none-name-key (`:76,107`): a missing `name` renders as the
//!   literal `None` inside `asset_key` and as JSON null in attributes.
//!
//! The PATCH metadata publish (`get_asset_object_metadata.delay`, `:148`)
//! goes out over AMQP in Celery protocol v2 (the coexistence transport;
//! Django's own broker is AMQP, `settings/common.py:399-401`). A broker
//! failure answers the 500 fallback, mirroring `.delay()` raising when
//! Django's broker is unreachable.

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use sqlx::Row;
use uuid::Uuid;

use crate::state::AppState;
use pidash_services::space::guards;
use pidash_services::space::queries::intake_assets as q;

/// Routes for `space/urls/asset.py` (all under `/api/public/`).
///
/// The no-`pk` collection path owns POST only (Django's GET there takes
/// no `pk` kwarg and 500s; proxying keeps that byte-identical), the
/// `pk` path owns GET+PATCH+DELETE, restore and bulk own POST.
/// Every other method on these paths proxies to Django.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/public/assets/v2/anchor/{anchor}/",
            crate::license::owned(post(post_asset), &["POST"]),
        )
        .route(
            "/api/public/assets/v2/anchor/{anchor}/{pk}/",
            crate::license::owned(
                get(get_asset).patch(patch_asset).delete(delete_asset),
                &["GET", "PATCH", "DELETE"],
            ),
        )
        .route(
            "/api/public/assets/v2/anchor/{anchor}/restore/{pk}/",
            crate::license::owned(post(restore_asset), &["POST"]),
        )
        .route(
            "/api/public/assets/v2/anchor/{anchor}/{entity_id}/bulk/",
            crate::license::owned(post(bulk_asset), &["POST"]),
        )
}

/// All ten `EntityTypeContext` values (`db/models/asset.py:33-43`):
/// POST accepts any of them (`views/asset.py:83`).
const ENTITY_TYPES: &[&str] = &[
    "ISSUE_ATTACHMENT",
    "ISSUE_DESCRIPTION",
    "COMMENT_DESCRIPTION",
    "PAGE_DESCRIPTION",
    "USER_COVER",
    "USER_AVATAR",
    "WORKSPACE_LOGO",
    "PROJECT_COVER",
    "DRAFT_ISSUE_ATTACHMENT",
    "DRAFT_ISSUE_DESCRIPTION",
];

/// POST mime allowlist (`views/asset.py:90-97`).
const ALLOWED_TYPES: &[&str] = &[
    "image/jpeg",
    "image/png",
    "image/webp",
    "image/jpg",
    "image/gif",
];

/// Resolver 404 for non-UUID `<uuid:pk>` / `<uuid:entity_id>` segments
/// (global `handler404`, `pi_dash/urls.py:15`).
const PAGE_NOT_FOUND_BODY: &str = r#"{"error":"Page not found."}"#;

// ---------------------------------------------------------------------------
// Small responders
// ---------------------------------------------------------------------------

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

fn guard_err(err: guards::ErrorBody) -> Response {
    json_status(
        StatusCode::from_u16(err.status).expect("valid guard status"),
        err.body,
    )
}

fn server_error() -> Response {
    raw(
        StatusCode::INTERNAL_SERVER_ERROR,
        crate::license::SERVER_ERROR_BODY,
    )
}

/// `.get()` miss envelope: Django `ObjectDoesNotExist` through the
/// view's `handle_exception` (`views/base.py:170-174`).
fn does_not_exist() -> Response {
    raw(StatusCode::NOT_FOUND, crate::license::NOT_FOUND_BODY)
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

/// `IsAuthenticated` for the mutation endpoints: anonymous answers
/// DRF `NotAuthenticated` (401), exactly like the license handlers.
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
// Board + asset row reads
// ---------------------------------------------------------------------------

struct Board {
    workspace_id: Uuid,
    project_id: Uuid,
}

async fn board_first(
    pool: &sqlx::PgPool,
    scoped: bool,
    anchor: &str,
) -> Result<Option<Board>, sqlx::Error> {
    let sql = if scoped {
        q::asset_board_first_scoped_sql()
    } else {
        q::asset_board_first_unscoped_sql()
    };
    sqlx::query(&sql)
        .bind(anchor)
        .fetch_optional(pool)
        .await?
        .map(|row| {
            Ok::<Board, sqlx::Error>(Board {
                workspace_id: row.try_get("workspace_id")?,
                project_id: row.try_get("project_id")?,
            })
        })
        .transpose()
}

struct AssetRow {
    id: Uuid,
    entity_type: Option<String>,
    is_uploaded: bool,
    storage_metadata: Option<Value>,
    attributes: Option<Value>,
    asset_key: String,
}

fn asset_row(row: &sqlx::postgres::PgRow) -> Result<AssetRow, sqlx::Error> {
    Ok(AssetRow {
        id: row.try_get("id")?,
        entity_type: row.try_get("entity_type")?,
        is_uploaded: row.try_get("is_uploaded")?,
        storage_metadata: row.try_get("storage_metadata")?,
        attributes: row.try_get("attributes")?,
        asset_key: row.try_get("asset")?,
    })
}

/// Python truthiness of a `storage_metadata` JSON value: `None`/null,
/// `{}`, `[]`, `""`, `0` and `false` skip the metadata task
/// (`views/asset.py:147`).
fn python_truthy(value: &Option<Value>) -> bool {
    match value {
        None => false,
        Some(Value::Null) => false,
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

// ---------------------------------------------------------------------------
// Request-data parsing (form / multipart / JSON, last value wins)
// ---------------------------------------------------------------------------

/// Parse `request.data` the way DRF does for the three content types
/// the suite and browsers send: JSON objects as-is, urlencoded and
/// multipart text fields as strings. Duplicate keys keep the last
/// value (`QueryDict.get`). Files in multipart bodies are ignored —
/// the view never reads `request.FILES`.
fn parse_data(content_type: Option<&str>, body: &[u8]) -> Option<Map<String, Value>> {
    if body.is_empty() {
        return Some(Map::new());
    }
    let ct = content_type.unwrap_or("");
    if ct.contains("application/json") {
        match serde_json::from_slice::<Value>(body) {
            Ok(Value::Object(map)) => Some(map),
            // A JSON list/scalar has no `.get`: `AttributeError` → 500.
            _ => None,
        }
    } else if ct.contains("application/x-www-form-urlencoded") {
        Some(parse_urlencoded(body))
    } else if ct.contains("multipart/form-data") {
        let boundary = ct.split("boundary=").nth(1).unwrap_or("").trim();
        if boundary.is_empty() {
            return None;
        }
        Some(parse_multipart(body, boundary))
    } else {
        None
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

/// Minimal multipart text-field parser: splits on the boundary,
/// reads each part's `name="..."`, keeps the raw bytes as a string.
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

fn data_str<'a>(data: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    data.get(key).and_then(Value::as_str)
}

/// `request.data.get("type", "image/jpeg")` (`views/asset.py:77`): the
/// default applies only when the key is absent. A present non-string
/// (explicit null included) fails the allowlist below and answers the
/// 400, exactly like `None not in allowed_types` in Django.
fn post_mime(data: &Map<String, Value>) -> Option<&str> {
    match data.get("type") {
        None => Some("image/jpeg"),
        Some(Value::String(s)) => Some(s),
        Some(_) => None,
    }
}

/// `int(...)` for the `size` field (`views/asset.py:78`): JSON numbers
/// truncate, numeric strings parse, anything else (or explicit null)
/// raises → 500. Missing → `FILE_SIZE_LIMIT`.
fn parse_size(value: Option<&Value>, default: i64) -> Option<i64> {
    match value {
        None => Some(default),
        Some(Value::Number(n)) => {
            if let Some(i) = n.as_i64() {
                Some(i)
            } else if let Some(u) = n.as_u64() {
                i64::try_from(u).ok()
            } else {
                n.as_f64().map(|f| f.trunc() as i64)
            }
        }
        // `int(True)` is `1`: bools are ints in Python.
        Some(Value::Bool(b)) => Some(i64::from(*b)),
        Some(Value::String(s)) => s.trim().parse::<i64>().ok(),
        _ => None,
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
// GET — AllowAny; presigned-URL 302 redirect
// ---------------------------------------------------------------------------

/// `EntityAssetEndpoint.get` (`views/asset.py:34-66`).
async fn get_asset(
    State(state): State<AppState>,
    Path((anchor, pk)): Path<(String, String)>,
    headers: HeaderMap,
) -> Response {
    let Some(pool) = pool(&state) else {
        return server_error();
    };
    let board = match board_first(pool, false, &anchor).await {
        Ok(board) => board,
        Err(_) => return server_error(),
    };
    let Some(board) = board else {
        return guard_err(guards::requested_resource_not_found());
    };
    let pk = match pk.parse::<Uuid>() {
        Ok(pk) => pk,
        Err(_) => return page_not_found(),
    };
    let asset = match sqlx::query(&q::asset_get_sql())
        .bind(board.workspace_id)
        .bind(pk)
        .fetch_optional(pool)
        .await
    {
        Ok(asset) => asset,
        Err(_) => return server_error(),
    };
    let Some(row) = asset else {
        return does_not_exist();
    };
    let asset = match asset_row(&row) {
        Ok(asset) => asset,
        Err(_) => return server_error(),
    };
    if !asset.is_uploaded {
        return guard_err(guards::requested_asset_not_found());
    }
    let storage = &state.settings().storage;
    let Some(host) = host_of(&headers) else {
        return server_error();
    };
    let url = presigned_get_url(
        storage,
        &scheme_of(&headers),
        &host,
        &asset.asset_key,
        &Utc::now(),
    );
    // `HttpResponseRedirect`: empty body, default content type.
    Response::builder()
        .status(StatusCode::FOUND)
        .header(header::LOCATION, url)
        .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
        .body(axum::body::Body::empty())
        .expect("redirect response")
}

// ---------------------------------------------------------------------------
// POST — mint an upload
// ---------------------------------------------------------------------------

/// `EntityAssetEndpoint.post` (`views/asset.py:68-133`).
async fn post_asset(
    State(state): State<AppState>,
    Path((anchor,)): Path<(String,)>,
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
    let board = match board_first(pool, false, &anchor).await {
        Ok(board) => board,
        Err(_) => return server_error(),
    };
    let Some(board) = board else {
        return guard_err(guards::project_not_published());
    };
    let Some(data) = parse_data(content_type(&headers).as_deref(), &body) else {
        return server_error();
    };
    let entity_type = data_str(&data, "entity_type").unwrap_or("");
    if !ENTITY_TYPES.contains(&entity_type) {
        return guard_err(guards::invalid_entity_type());
    }
    let Some(mime) = post_mime(&data) else {
        return guard_err(guards::invalid_file_type());
    };
    if !ALLOWED_TYPES.contains(&mime) {
        return guard_err(guards::invalid_file_type());
    }
    let size = match parse_size(data.get("size"), state.settings().file_size_limit) {
        Some(size) => size,
        None => return server_error(),
    };
    let (name_display, name_json) = python_str(data.get("name"));
    // `comment_id=entity_identifier` unconditionally, even for
    // non-comment entity types (`:118`); a non-UUID value reaches the
    // uuid column and the database raises → 500, same as Django.
    let comment_id: Option<Uuid> = match data.get("entity_identifier") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => match s.parse::<Uuid>() {
            Ok(id) => Some(id),
            Err(_) => return server_error(),
        },
        Some(_) => return server_error(),
    };

    let asset_id = Uuid::new_v4();
    let asset_key = format!(
        "{}/{}-{name_display}",
        board.workspace_id,
        asset_id.simple()
    );
    let attributes = serde_json::json!({
        "name": name_json,
        "type": mime,
        "size": size,
    });
    let now = Utc::now();
    if sqlx::query(&asset_insert_complete_sql())
        .bind(asset_id)
        .bind(now)
        .bind(now)
        .bind(sqlx::types::Json(attributes))
        .bind(&asset_key)
        .bind(size as f64)
        .bind(board.workspace_id)
        .bind(actor.id)
        .bind(entity_type)
        .bind(board.project_id)
        .bind(comment_id)
        .execute(pool)
        .await
        .is_err()
    {
        return server_error();
    }
    let slug: Option<String> = match sqlx::query_scalar("SELECT slug FROM workspaces WHERE id = $1")
        .bind(board.workspace_id)
        .fetch_optional(pool)
        .await
    {
        Ok(slug) => slug,
        Err(_) => return server_error(),
    };
    let Some(slug) = slug else {
        return server_error();
    };
    let asset_url = asset_url_for(entity_type, &slug, &board.project_id, &asset_id);
    let storage = &state.settings().storage;
    let Some(host) = host_of(&headers) else {
        return server_error();
    };
    let upload_data = presigned_post(
        storage,
        &scheme_of(&headers),
        &host,
        &asset_key,
        mime,
        size,
        &now,
    );
    json_status(
        StatusCode::OK,
        serde_json::json!({
            "upload_data": upload_data,
            "asset_id": asset_id.to_string(),
            "asset_url": asset_url,
        }),
    )
}

/// POST INSERT, execution-complete edition of
/// [`q::asset_insert_sql`]: `$1..$11` bind in the builder's documented
/// order, plus the three literals Django's ORM always sends alongside
/// them. The builder omits `is_deleted` / `is_archived` (both
/// `NOT NULL` without a database default) and `storage_metadata`
/// (Python-side `default=dict`), so executing it verbatim fails live
/// with a null-violation (`is_deleted`) — verified against Postgres.
/// Django emits the full column list with `is_deleted=false`,
/// `is_archived=false`, `storage_metadata='{}'`; nullable columns not
/// passed (`updated_by_id`, `user_id`, `issue_id`, `page_id`,
/// `draft_issue_id`, `entity_identifier`, `external_*`, `deleted_at`)
/// default to `NULL` on both sides. The builder fix belongs to the
/// queries layer (follow-up issue); this statement keeps the same
/// resulting row.
fn asset_insert_complete_sql() -> String {
    "INSERT INTO \"file_assets\" (\"id\", \"created_at\", \"updated_at\", \"attributes\", \"asset\", \"size\", \"workspace_id\", \"created_by_id\", \"entity_type\", \"project_id\", \"comment_id\", \"is_uploaded\", \"is_deleted\", \"is_archived\", \"storage_metadata\") VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, false, false, false, '{}')"
        .to_string()
}

/// `FileAsset.asset_url` (`db/models/asset.py:80-98`): `issue_id` is
/// always `None` here (POST never sets it), rendered as `None` exactly
/// like the f-string.
fn asset_url_for(
    entity_type: &str,
    workspace_slug: &str,
    project_id: &Uuid,
    asset_id: &Uuid,
) -> Value {
    match entity_type {
        "WORKSPACE_LOGO" | "USER_AVATAR" | "USER_COVER" | "PROJECT_COVER" => {
            Value::String(format!("/api/assets/v2/static/{asset_id}/"))
        }
        "ISSUE_ATTACHMENT" => Value::String(format!(
            "/api/assets/v2/workspaces/{workspace_slug}/projects/{project_id}/issues/None/attachments/{asset_id}/"
        )),
        "ISSUE_DESCRIPTION"
        | "COMMENT_DESCRIPTION"
        | "PAGE_DESCRIPTION"
        | "DRAFT_ISSUE_DESCRIPTION" => Value::String(format!(
            "/api/assets/v2/workspaces/{workspace_slug}/projects/{project_id}/{asset_id}/"
        )),
        _ => Value::Null,
    }
}

// ---------------------------------------------------------------------------
// PATCH — mark uploaded (+ metadata task)
// ---------------------------------------------------------------------------

/// `EntityAssetEndpoint.patch` (`views/asset.py:135-154`).
async fn patch_asset(
    State(state): State<AppState>,
    Path((anchor, pk)): Path<(String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let _actor = match require_user(&state, extension).await {
        Ok(actor) => actor,
        Err(denial) => return denial.into_response(),
    };
    let Some(pool) = pool(&state) else {
        return server_error();
    };
    let board = match board_first(pool, false, &anchor).await {
        Ok(board) => board,
        Err(_) => return server_error(),
    };
    let Some(board) = board else {
        return guard_err(guards::project_not_published());
    };
    let pk = match pk.parse::<Uuid>() {
        Ok(pk) => pk,
        Err(_) => return page_not_found(),
    };
    // Patch passes no project scoping (`:143`) — port.
    let row = match sqlx::query(&q::asset_patch_get_sql())
        .bind(pk)
        .bind(board.workspace_id)
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
    // `is_uploaded = True` is set before the metadata check (`:145-148`).
    if !python_truthy(&asset.storage_metadata) {
        let message = pidash_jobs::space::asset_metadata_message(&asset.id.to_string());
        let published = match pidash_jobs::AmqpConfig::from_env() {
            Ok(config) => match pidash_jobs::Publisher::connect(&config).await {
                Ok(publisher) => {
                    let result = publisher.publish(&message).await;
                    let _ = publisher.close().await;
                    result
                }
                Err(error) => Err(error),
            },
            Err(error) => Err(error),
        };
        // `.delay()` raising on an unreachable broker answers the 500
        // fallback in Django; same here.
        if published.is_err() {
            return server_error();
        }
    }
    let Some(data) = parse_data(content_type(&headers).as_deref(), &body) else {
        return server_error();
    };
    let attributes = data
        .get("attributes")
        .cloned()
        .unwrap_or_else(|| asset.attributes.clone().unwrap_or(Value::Null));
    if sqlx::query(&q::asset_patch_sql())
        .bind(pk)
        .bind(sqlx::types::Json(attributes))
        .execute(pool)
        .await
        .is_err()
    {
        return server_error();
    }
    no_content()
}

// ---------------------------------------------------------------------------
// DELETE — soft-delete (project-scoped board)
// ---------------------------------------------------------------------------

/// `EntityAssetEndpoint.delete` (`views/asset.py:156-169`).
async fn delete_asset(
    State(state): State<AppState>,
    Path((anchor, pk)): Path<(String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Response {
    let _actor = match require_user(&state, extension).await {
        Ok(actor) => actor,
        Err(denial) => return denial.into_response(),
    };
    let Some(pool) = pool(&state) else {
        return server_error();
    };
    let board = match board_first(pool, true, &anchor).await {
        Ok(board) => board,
        Err(_) => return server_error(),
    };
    let Some(board) = board else {
        return guard_err(guards::project_not_published());
    };
    let pk = match pk.parse::<Uuid>() {
        Ok(pk) => pk,
        Err(_) => return page_not_found(),
    };
    let row = match sqlx::query(&q::asset_scoped_get_sql())
        .bind(pk)
        .bind(board.workspace_id)
        .bind(board.project_id)
        .fetch_optional(pool)
        .await
    {
        Ok(row) => row,
        Err(_) => return server_error(),
    };
    if row.is_none() {
        return does_not_exist();
    }
    if sqlx::query(&q::asset_soft_delete_sql())
        .bind(pk)
        .bind(Utc::now())
        .execute(pool)
        .await
        .is_err()
    {
        return server_error();
    }
    no_content()
}

// ---------------------------------------------------------------------------
// Restore — unflip through the unfiltered manager
// ---------------------------------------------------------------------------

/// `AssetRestoreEndpoint.post` (`views/asset.py:175-187`).
async fn restore_asset(
    State(state): State<AppState>,
    Path((anchor, pk)): Path<(String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Response {
    let _actor = match require_user(&state, extension).await {
        Ok(actor) => actor,
        Err(denial) => return denial.into_response(),
    };
    let Some(pool) = pool(&state) else {
        return server_error();
    };
    let board = match board_first(pool, true, &anchor).await {
        Ok(board) => board,
        Err(_) => return server_error(),
    };
    let Some(board) = board else {
        return guard_err(guards::project_not_published());
    };
    let pk = match pk.parse::<Uuid>() {
        Ok(pk) => pk,
        Err(_) => return page_not_found(),
    };
    // `all_objects`: no `deleted_at IS NULL` conjunct (`:183`) — port.
    let row = match sqlx::query(&q::asset_restore_get_sql())
        .bind(pk)
        .bind(board.workspace_id)
        .fetch_optional(pool)
        .await
    {
        Ok(row) => row,
        Err(_) => return server_error(),
    };
    if row.is_none() {
        return does_not_exist();
    }
    if sqlx::query(&q::asset_restore_sql())
        .bind(pk)
        .execute(pool)
        .await
        .is_err()
    {
        return server_error();
    }
    no_content()
}

// ---------------------------------------------------------------------------
// Bulk — COMMENT_DESCRIPTION-only reassignment (silent no-op otherwise)
// ---------------------------------------------------------------------------

/// `EntityBulkAssetEndpoint.post` (`views/asset.py:193-226`).
async fn bulk_asset(
    State(state): State<AppState>,
    Path((anchor, entity_id)): Path<(String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let _actor = match require_user(&state, extension).await {
        Ok(actor) => actor,
        Err(denial) => return denial.into_response(),
    };
    let Some(pool) = pool(&state) else {
        return server_error();
    };
    let board = match board_first(pool, true, &anchor).await {
        Ok(board) => board,
        Err(_) => return server_error(),
    };
    let Some(board) = board else {
        return guard_err(guards::project_not_published());
    };
    let entity_id = match entity_id.parse::<Uuid>() {
        Ok(entity_id) => entity_id,
        Err(_) => return page_not_found(),
    };
    let Some(data) = parse_data(content_type(&headers).as_deref(), &body) else {
        return server_error();
    };
    // `asset_ids=data.get("asset_ids", [])` (`:200`): missing or empty
    // → 400 (`:203-204`); a non-UUID element fails the uuid lookup the
    // way Django's `ValidationError` does → the 400 envelope.
    let raw_ids = match data.get("asset_ids") {
        Some(Value::Array(ids)) => ids.clone(),
        _ => Vec::new(),
    };
    if raw_ids.is_empty() {
        return guard_err(guards::no_asset_ids());
    }
    let mut asset_ids = Vec::with_capacity(raw_ids.len());
    for id in &raw_ids {
        match id.as_str().and_then(|s| s.parse::<Uuid>().ok()) {
            Some(id) => asset_ids.push(id),
            None => {
                return json_status(
                    StatusCode::BAD_REQUEST,
                    serde_json::json!({"error": "Please provide valid detail"}),
                );
            }
        }
    }
    let filter_sql = q::bulk_filter_sql(asset_ids.len());
    let mut filter = sqlx::query(&filter_sql);
    for id in &asset_ids {
        filter = filter.bind(id);
    }
    let rows = match filter
        .bind(board.workspace_id)
        .bind(board.project_id)
        .fetch_all(pool)
        .await
    {
        Ok(rows) => rows,
        Err(_) => return server_error(),
    };
    // `assets.first()`: default ordering is `-created_at`, so the gate
    // reads the most recent match.
    let mut ordered: Vec<&sqlx::postgres::PgRow> = rows.iter().collect();
    ordered.sort_by(|a, b| {
        let at: DateTime<Utc> = a.try_get("created_at").unwrap_or(DateTime::UNIX_EPOCH);
        let bt: DateTime<Utc> = b.try_get("created_at").unwrap_or(DateTime::UNIX_EPOCH);
        bt.cmp(&at)
    });
    let Some(first) = ordered.first() else {
        return guard_err(guards::requested_asset_not_found());
    };
    let first = match asset_row(first) {
        Ok(asset) => asset,
        Err(_) => return server_error(),
    };
    if first.entity_type.as_deref() == Some("COMMENT_DESCRIPTION") {
        let reassign_sql = q::bulk_reassign_sql(asset_ids.len());
        let mut query = sqlx::query(&reassign_sql);
        for id in &asset_ids {
            query = query.bind(id);
        }
        if query
            .bind(board.workspace_id)
            .bind(board.project_id)
            .bind(entity_id)
            .execute(pool)
            .await
            .is_err()
        {
            return server_error();
        }
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
/// `S3Storage.__init__` with a request (`is_server=False`):
/// MinIO mode signs `{scheme}://{Host}` path-style; an explicit
/// endpoint URL signs path-style against it; otherwise the
/// virtual-hosted AWS default.
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
            format!("https://{}.{base}", storage.bucket_name),
            format!("{}.{base}", storage.bucket_name),
        )
    }
}

/// `generate_presigned_url(object_name)` for `get_object`
/// (`views/asset.py:64`, `storage.py`): presigned GET with
/// `response-content-disposition=inline; filename*=UTF-8''<hex>`.
fn presigned_get_url(
    storage: &pidash_db::config::StorageSettings,
    scheme: &str,
    host: &str,
    object_name: &str,
    now: &DateTime<Utc>,
) -> String {
    let region = storage.region.as_str();
    let (endpoint, signed_host) = endpoint_parts(storage, scheme, host);
    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let date = now.format("%Y%m%d").to_string();
    let scope = credential_scope(&date, region);
    let credential = format!("{}/{}", storage.access_key_id, scope);
    // Fresh uuid4 hex per call (`_get_content_disposition(..., None)`).
    let disposition = format!("inline; filename*=UTF-8''{}", Uuid::new_v4().simple());
    let mut params = [
        ("response-content-disposition".to_owned(), disposition),
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
    format!("{endpoint}{canonical_path}?{canonical_query}&X-Amz-Signature={signature}")
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
/// (`views/asset.py:124`, `storage.py`): `{"url","fields"}` with
/// botocore's field order and the `storage.py` condition order.
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
        chrono::DateTime::parse_from_rfc3339("2026-09-28T12:00:00Z")
            .expect("fixed time")
            .with_timezone(&Utc)
    }

    #[test]
    fn presigned_post_field_order_matches_botocore() {
        let post = presigned_post(
            &test_storage(),
            "http",
            "127.0.0.1:8486",
            "ws-id/ab12-shot.png",
            "image/png",
            10,
            &test_now(),
        );
        let fields = post["fields"].as_object().expect("fields object");
        let keys: Vec<&str> = fields.keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            [
                "Content-Type",
                "key",
                "x-amz-algorithm",
                "x-amz-credential",
                "x-amz-date",
                "policy",
                "x-amz-signature"
            ],
        );
        assert_eq!(post["url"], "http://127.0.0.1:8486/uploads");
        // (MinIO-mode URL; the virtual-hosted default carries a trailing
        // slash instead — verified against live Django in the PR.)
        // Independent oracle: recompute the policy signature by hand.
        let policy = fields["policy"].as_str().expect("policy");
        let date = "20260928";
        let scope = format!("{date}/us-east-1/s3/aws4_request");
        assert_eq!(
            fields["x-amz-credential"].as_str().expect("credential"),
            format!("access-key/{scope}"),
        );
        let expected = hex(&hmac_sha256(
            &signing_key("secret-key", date, "us-east-1"),
            policy.as_bytes(),
        ));
        assert_eq!(
            fields["x-amz-signature"].as_str().expect("signature"),
            expected
        );
        // Policy decodes to the storage.py condition order.
        let raw = base64_decode(policy);
        let doc: Value = serde_json::from_slice(&raw).expect("policy json");
        assert_eq!(doc["expiration"], "2026-09-28T13:00:00Z");
        let conditions = doc["conditions"].as_array().expect("conditions");
        assert_eq!(conditions.len(), 9);
        assert_eq!(conditions[0], serde_json::json!({"bucket": "uploads"}));
        assert_eq!(
            conditions[1],
            serde_json::json!(["content-length-range", 1, 10])
        );
        assert_eq!(
            conditions[2],
            serde_json::json!({"Content-Type": "image/png"})
        );
        assert_eq!(
            conditions[3],
            serde_json::json!({"key": "ws-id/ab12-shot.png"})
        );
        // Client-level auto conditions (verified against live Django).
        assert_eq!(conditions[4], serde_json::json!({"bucket": "uploads"}));
        assert_eq!(
            conditions[5],
            serde_json::json!({"key": "ws-id/ab12-shot.png"})
        );
    }

    #[test]
    fn presigned_get_signs_disposition_and_host() {
        let url = presigned_get_url(
            &test_storage(),
            "http",
            "127.0.0.1:8486",
            "ws-id/ab12-shot.png",
            &test_now(),
        );
        assert!(
            url.starts_with("http://127.0.0.1:8486/uploads/ws-id/ab12-shot.png?"),
            "{url}"
        );
        assert!(url.contains("X-Amz-Algorithm=AWS4-HMAC-SHA256"));
        assert!(url.contains("X-Amz-SignedHeaders=host"));
        assert!(url.contains("response-content-disposition=inline"));
        // Independent oracle: recompute the query signature by hand.
        let (query, signature) = url
            .split_once('?')
            .expect("query")
            .1
            .rsplit_once("X-Amz-Signature=")
            .expect("signature");
        let query = query.strip_suffix('&').expect("trailing amp");
        let scope = "20260928/us-east-1/s3/aws4_request";
        let canonical = format!(
            "GET\n/uploads/ws-id/ab12-shot.png\n{query}\nhost:127.0.0.1:8486\n\nhost\nUNSIGNED-PAYLOAD"
        );
        let to_sign = format!(
            "AWS4-HMAC-SHA256\n20260928T120000Z\n{scope}\n{}",
            sha256_hex(canonical.as_bytes())
        );
        let expected = hex(&hmac_sha256(
            &signing_key("secret-key", "20260928", "us-east-1"),
            to_sign.as_bytes(),
        ));
        assert_eq!(signature, expected);
    }

    #[test]
    fn py_json_string_matches_cpython_escapes() {
        assert_eq!(py_json_string("shot.png"), "\"shot.png\"");
        assert_eq!(py_json_string("a\"b\\c"), "\"a\\\"b\\\\c\"");
        assert_eq!(py_json_string("é"), "\"\\u00e9\"");
        assert_eq!(py_json_string("𝄞"), "\"\\ud834\\udd1e\"");
    }

    #[test]
    fn parse_size_mirrors_int_quirk() {
        assert_eq!(parse_size(None, 5242880), Some(5242880));
        assert_eq!(parse_size(Some(&Value::from("10")), 5), Some(10));
        assert_eq!(parse_size(Some(&Value::from(" 10 ")), 5), Some(10));
        assert_eq!(parse_size(Some(&Value::from(10)), 5), Some(10));
        assert_eq!(parse_size(Some(&Value::from(10.9)), 5), Some(10));
        assert_eq!(parse_size(Some(&Value::Bool(true)), 5), Some(1));
        assert_eq!(parse_size(Some(&Value::from("abc")), 5), None);
        assert_eq!(parse_size(Some(&Value::Null), 5), None);
    }

    #[test]
    fn post_mime_defaults_only_when_absent() {
        let empty = Map::new();
        assert_eq!(post_mime(&empty), Some("image/jpeg"));
        let mut present = Map::new();
        present.insert("type".to_owned(), Value::from("image/png"));
        assert_eq!(post_mime(&present), Some("image/png"));
        // Explicit null (or any other non-string) fails the allowlist
        // like Django's `None not in allowed_types` — no default.
        let mut null = Map::new();
        null.insert("type".to_owned(), Value::Null);
        assert_eq!(post_mime(&null), None);
        let mut number = Map::new();
        number.insert("type".to_owned(), Value::from(5));
        assert_eq!(post_mime(&number), None);
    }

    #[test]
    fn python_str_renders_none_like_fstring() {
        assert_eq!(python_str(None), ("None".to_owned(), Value::Null),);
        assert_eq!(
            python_str(Some(&Value::from("shot.png"))),
            ("shot.png".to_owned(), Value::from("shot.png")),
        );
    }

    #[test]
    fn python_truthy_matches_storage_metadata_guard() {
        assert!(!python_truthy(&None));
        assert!(!python_truthy(&Some(Value::Null)));
        assert!(!python_truthy(&Some(serde_json::json!({}))));
        assert!(python_truthy(&Some(serde_json::json!({"seeded": true}))));
    }

    #[test]
    fn multipart_and_urlencoded_parse_last_wins() {
        let form = parse_urlencoded(b"name=a.png&name=b.png&size=10");
        assert_eq!(form["name"], "b.png");
        let body =
            b"--B\r\nContent-Disposition: form-data; name=\"type\"\r\n\r\nimage/png\r\n--B--\r\n";
        let multi = parse_multipart(body, "B");
        assert_eq!(multi["type"], "image/png");
    }

    fn base64_decode(input: &str) -> Vec<u8> {
        const ALPHABET: &[u8; 128] = &{
            let mut table = [255u8; 128];
            let bytes = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
            let mut i = 0;
            while i < bytes.len() {
                table[bytes[i] as usize] = i as u8;
                i += 1;
            }
            table
        };
        let clean: Vec<u8> = input.bytes().filter(|b| *b != b'=').collect();
        let mut out = Vec::with_capacity(clean.len() * 3 / 4);
        for chunk in clean.chunks(4) {
            let mut n: u32 = 0;
            for (i, b) in chunk.iter().enumerate() {
                n |= (ALPHABET[*b as usize] as u32) << (18 - 6 * i);
            }
            for i in 0..chunk.len() - 1 {
                out.push(((n >> (16 - 8 * i)) & 0xff) as u8);
            }
        }
        out
    }
}
