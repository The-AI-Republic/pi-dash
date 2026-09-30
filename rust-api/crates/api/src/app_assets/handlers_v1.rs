//! Legacy v1 file-asset endpoints (D-31, stage 5).
//!
//! Ports `apps/api/pi_dash/app/views/asset/base.py:1-86` — one closure,
//! 3 classes / 7 methods — with routes from `app/urls/asset.py:27-47`
//! (the 5 legacy patterns). The data plane (SQL text + status/body
//! contract) lives in [`pidash_services::app_assets::queries_v1`]; row
//! rendering in [`pidash_services::app_assets::shape`]; the v1 gate
//! statement (default auth only) in [`super::guards`]. This module owns
//! the HTTP shell: routes, session auth, parser quirks, and row fetching.
//!
//! Method map (trace: Python lines):
//! - `FileAssetEndpoint.get` (`base.py:23-33`): workspace-composed key,
//!   `many=True` envelope `{"data", "status": true}` vs 200 miss body.
//! - `FileAssetEndpoint.post` (`base.py:35-42`): file validation first,
//!   then the workspace-slug lookup, then the storage 500 (ported bug).
//! - `FileAssetEndpoint.delete` (`base.py:44-49`): single-row get plus
//!   the `is_deleted`-only flip; 204, empty body.
//! - `FileAssetViewSet.restore` (`base.py:52-58`): same lookup shape,
//!   unflip; 204, empty body.
//! - `UserAssetsEndpoint.get` (`base.py:64-73`): raw key plus
//!   `created_by` scoping; miss 200, rows 500 (ported bug, no `many`).
//! - `UserAssetsEndpoint.post` (`base.py:75-80`): no serializer kwargs;
//!   FormParser-only, so JSON 415s.
//! - `UserAssetsEndpoint.delete` (`base.py:82-86`): creator-scoped get
//!   plus the flip; 204, empty body.
//!
//! Parser quirks (translate, don't redesign):
//! - `FileAssetEndpoint.parser_classes` includes `JSONParser`
//!   (`base.py:17`); `UserAssetsEndpoint` lists only MultiPart + Form
//!   (`base.py:62`), so a JSON POST there answers 415
//!   `{"detail": "Unsupported media type ..."}` before the view runs.
//! - An empty body parses to empty data on both endpoints (no 415);
//!   validation then reports the missing file (400).
//! - Validation runs before the workspace lookup on workspace POST
//!   (`base.py:36-39`): no-file + unknown slug answers 400, while a
//!   real file + unknown slug answers 404.
//!
//! Ported bugs (also listed in the PR):
//! - `BUG (base.py: storage)`: `S3Storage.__init__` never calls
//!   `super().__init__`, so django-storages' setup is missing and every
//!   real file upload dies with the generic 500. The handlers return
//!   that 500 without touching the storage seam (read-only).
//! - `BUG (base.py:67)`: the user GET serializes the queryset WITHOUT
//!   `many=True`, so any existing user row raises into the generic 500.
//!   Handlers return the 500 whenever the user filter matches.
//! - `BUG (base.py:47-48)`: delete/restore write ONLY `is_deleted`
//!   (`save(update_fields=["is_deleted"])`); `deleted_at` stays NULL so
//!   the row remains visible to reads (GET-after-DELETE still 200s
//!   `status: true`). The statement names only `is_deleted`
//!   ([`pidash_services::app_assets::queries_v1::set_deleted_sql`]).
//!
//! Auth: all five routes inherit the `BaseAPIView` / `BaseViewSet`
//! default (`permission_classes=[IsAuthenticated]`,
//! `app/views/base.py:189-194`); anonymous answers 401 before anything
//! else runs. Row scoping on the user endpoints is
//! `created_by=request.user` in the queries, not a membership gate.
//!
//! Fixtures: `rust-api/fixtures/app_assets/queries/v1.golden.json`
//! (PIDASHCONV-306), `serializers/fileasset.golden.json`
//! (PIDASHCONV-319), `guards/permissions.golden.json` (PIDASHCONV-378).
//! Oracle: `rust-api/contract-tests/app_assets/test_legacy_v1.py`
//! (PIDASHCONV-89).

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use sqlx::Row;
use uuid::Uuid;

use crate::state::AppState;
use pidash_services::app_assets::{queries_v1, shape};

// ---------------------------------------------------------------------------
// Small responders (exact Django bytes)
// ---------------------------------------------------------------------------

/// Resolver 404 for a non-UUID `<uuid:workspace_id>` segment (global
/// `handler404`, `pi_dash/urls.py:15`). The `str` converter on
/// `asset_key` / `slug` never rejects, so only the workspace id parses.
const PAGE_NOT_FOUND_BODY: &str = r#"{"error":"Page not found."}"#;

/// `FileAssetSerializer` missing-file error (`asset.py` + DRF `FileField`):
/// 400 `{"asset": ["No file was submitted."]}`.
const NO_FILE_BODY: &str = r#"{"asset":["No file was submitted."]}"#;

fn raw(status: StatusCode, body: &'static str) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body))
        .expect("static response")
}

fn json_status(status: StatusCode, body: &Value) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(
            serde_json::to_string(body).expect("serializable response"),
        ))
        .expect("json response")
}

fn server_error() -> Response {
    raw(
        StatusCode::INTERNAL_SERVER_ERROR,
        crate::license::SERVER_ERROR_BODY,
    )
}

/// `.get()` miss envelope: `ObjectDoesNotExist` through the view's
/// `handle_exception` (`app/views/base.py:234-238`).
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

/// `IsAuthenticated` (the v1 default): anonymous answers DRF
/// `NotAuthenticated` (401) before any gate or query runs.
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
// Request parsing (DRF parser_classes, per endpoint)
// ---------------------------------------------------------------------------

fn content_type_of(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
}

/// Multipart boundary (`multipart/form-data; boundary=...`, optionally
/// quoted), mirroring the password-handler parser.
fn multipart_boundary(content_type: &str) -> Option<String> {
    for param in content_type.split(';').skip(1) {
        let param = param.trim();
        if let Some(boundary) = param.strip_prefix("boundary=") {
            let boundary = boundary.trim();
            let boundary = boundary
                .strip_prefix('"')
                .and_then(|s| s.strip_suffix('"'))
                .unwrap_or(boundary);
            if !boundary.is_empty() && boundary.len() <= 70 {
                return Some(boundary.to_owned());
            }
        }
    }
    None
}

/// Whether the multipart body carries a file part for the `asset` field:
/// a part named `asset` whose headers carry `filename=`. Text-only parts
/// never count (DRF reads those into `request.data`, not `request.FILES`).
fn multipart_has_asset_file(body: &[u8], boundary: &str) -> bool {
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
        let head = &part[..sep];
        let Some(name_start) = head.find("name=\"") else {
            continue;
        };
        let rest = &head[name_start + 6..];
        let Some(name_end) = rest.find('"') else {
            continue;
        };
        if &rest[..name_end] == "asset" && head.contains("filename=") {
            return true;
        }
    }
    false
}

/// How a POST body classifies: a real file (the storage-500 path), no
/// file (the 400 path), or a parse-level rejection answered before the
/// view body runs (415/400 with a `detail` envelope).
#[derive(Debug, Clone, PartialEq, Eq)]
enum PostBody {
    /// A file part for `asset` is present: validation passes and the
    /// request proceeds (into the workspace lookup, then the storage
    /// 500 — the ported `S3Storage` bug).
    File,
    /// No file: the serializer reports
    /// `{"asset": ["No file was submitted."]}` (400).
    NoFile,
    /// No parser handles the media type: 415
    /// `{"detail": "Unsupported media type ... in request."}`.
    UnsupportedMediaType(String),
    /// The media type matches but the payload is malformed: 400
    /// `{"detail": ...}` (DRF `ParseError`).
    Malformed(String),
}

impl PostBody {
    fn into_response(self) -> Option<Response> {
        match self {
            PostBody::File | PostBody::NoFile => None,
            PostBody::UnsupportedMediaType(content_type) => Some(json_status(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                &json!({"detail": format!("Unsupported media type \"{content_type}\" in request.")}),
            )),
            PostBody::Malformed(detail) => Some(json_status(
                StatusCode::BAD_REQUEST,
                &json!({"detail": detail}),
            )),
        }
    }
}

/// Classify a workspace-POST body (`MultiPartParser, FormParser,
/// JSONParser`, `base.py:17`).
fn classify_workspace_post(content_type: Option<&str>, body: &[u8]) -> PostBody {
    if body.is_empty() {
        return PostBody::NoFile;
    }
    let ct = content_type.unwrap_or("");
    if ct.contains("multipart/form-data") {
        match multipart_boundary(ct) {
            Some(boundary) if multipart_has_asset_file(body, &boundary) => PostBody::File,
            Some(_) => PostBody::NoFile,
            None => {
                PostBody::Malformed("Multipart form parse error - invalid boundary.".to_owned())
            }
        }
    } else if ct.contains("application/x-www-form-urlencoded") {
        PostBody::NoFile
    } else if ct.contains("application/json") {
        match serde_json::from_slice::<Value>(body) {
            Ok(_) => PostBody::NoFile,
            Err(e) => PostBody::Malformed(format!("JSON parse error - {e}")),
        }
    } else {
        PostBody::UnsupportedMediaType(ct.to_owned())
    }
}

/// Classify a user-POST body (`MultiPartParser, FormParser` — NO
/// `JSONParser`, `base.py:62`): a JSON content type 415s.
fn classify_user_post(content_type: Option<&str>, body: &[u8]) -> PostBody {
    if body.is_empty() {
        return PostBody::NoFile;
    }
    let ct = content_type.unwrap_or("");
    if ct.contains("multipart/form-data") {
        match multipart_boundary(ct) {
            Some(boundary) if multipart_has_asset_file(body, &boundary) => PostBody::File,
            Some(_) => PostBody::NoFile,
            None => {
                PostBody::Malformed("Multipart form parse error - invalid boundary.".to_owned())
            }
        }
    } else if ct.contains("application/x-www-form-urlencoded") {
        PostBody::NoFile
    } else {
        // BUG-PRESERVING QUIRK (`base.py:62`): without `JSONParser` no
        // parser handles `application/json`, so DRF answers 415 before
        // the view runs (oracle `test_user_post_json_415`).
        PostBody::UnsupportedMediaType(ct.to_owned())
    }
}

// ---------------------------------------------------------------------------
// Row rendering (FileAssetSerializer shape, PIDASHCONV-319)
// ---------------------------------------------------------------------------

fn uuid_opt(value: Option<Uuid>) -> Option<String> {
    value.map(|id| id.to_string())
}

fn dt_in(value: DateTime<Utc>, tz: &chrono_tz::Tz) -> String {
    crate::serializer::render_datetime_in(&value, tz)
}

/// Map one `SELECT *` row onto the 24-key serializer shape. Datetimes
/// render in the request user's zone (`TimezoneMixin`,
/// `app/views/base.py:36-43`); FKs render as raw pks or null.
fn file_asset_record(
    row: &sqlx::postgres::PgRow,
    tz: &chrono_tz::Tz,
) -> Result<shape::FileAssetRecord, sqlx::Error> {
    let created_at: DateTime<Utc> = row.try_get("created_at")?;
    let updated_at: DateTime<Utc> = row.try_get("updated_at")?;
    let deleted_at: Option<DateTime<Utc>> = row.try_get("deleted_at")?;
    let attributes: Option<Value> = row.try_get("attributes")?;
    let storage_metadata: Option<Value> = row.try_get("storage_metadata")?;
    Ok(shape::FileAssetRecord {
        id: row.try_get::<Uuid, _>("id")?.to_string(),
        created_at: dt_in(created_at, tz),
        updated_at: dt_in(updated_at, tz),
        deleted_at: deleted_at.map(|dt| dt_in(dt, tz)),
        attributes: attributes.unwrap_or(Value::Null),
        asset: row.try_get("asset")?,
        entity_type: row.try_get("entity_type")?,
        entity_identifier: row.try_get("entity_identifier")?,
        is_deleted: row.try_get("is_deleted")?,
        is_archived: row.try_get("is_archived")?,
        external_id: row.try_get("external_id")?,
        external_source: row.try_get("external_source")?,
        size: row.try_get("size")?,
        is_uploaded: row.try_get("is_uploaded")?,
        storage_metadata: storage_metadata.unwrap_or(Value::Null),
        created_by: uuid_opt(row.try_get::<Option<Uuid>, _>("created_by_id")?),
        updated_by: uuid_opt(row.try_get::<Option<Uuid>, _>("updated_by_id")?),
        user: uuid_opt(row.try_get::<Option<Uuid>, _>("user_id")?),
        workspace: uuid_opt(row.try_get::<Option<Uuid>, _>("workspace_id")?),
        draft_issue: uuid_opt(row.try_get::<Option<Uuid>, _>("draft_issue_id")?),
        project: uuid_opt(row.try_get::<Option<Uuid>, _>("project_id")?),
        issue: uuid_opt(row.try_get::<Option<Uuid>, _>("issue_id")?),
        comment: uuid_opt(row.try_get::<Option<Uuid>, _>("comment_id")?),
        page: uuid_opt(row.try_get::<Option<Uuid>, _>("page_id")?),
    })
}

/// 200 `{"data": [...], "status": true}` (`base.py:28`).
fn found_envelope(items: Vec<Value>) -> Response {
    json_status(
        StatusCode::from_u16(queries_v1::FOUND_STATUS).expect("valid status"),
        &json!({"data": items, "status": true}),
    )
}

/// 200 `{"error": "Asset key does not exist", "status": false}`
/// (`base.py:30-33,70-73`): the miss is signalled only by the body.
fn miss_body() -> Response {
    json_status(
        StatusCode::from_u16(queries_v1::MISS_STATUS).expect("valid status"),
        &queries_v1::miss_body(),
    )
}

/// 500 `{"error": "Something went wrong please try again later"}`: the
/// user-GET serializer bug (`base.py:67`) and the real-file-upload
/// storage failure both land here via `handle_exception`.
fn unhandled() -> Response {
    json_status(
        StatusCode::from_u16(queries_v1::USER_GET_FOUND_STATUS).expect("valid status"),
        &queries_v1::unhandled_body(),
    )
}

// ---------------------------------------------------------------------------
// FileAssetEndpoint — workspace scope
// ---------------------------------------------------------------------------

/// `FileAssetEndpoint.get` (`base.py:23-33`).
async fn workspace_get(
    State(state): State<AppState>,
    Path((workspace_id_raw, asset_key)): Path<(String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Response {
    // URL resolution precedes authentication in Django: a non-UUID
    // segment 404s (`<uuid:workspace_id>` converter) even anonymously.
    let Ok(workspace_id) = workspace_id_raw.parse::<Uuid>() else {
        return page_not_found();
    };
    let actor = match require_user(&state, extension).await {
        Ok(actor) => actor,
        Err(denial) => return denial.into_response(),
    };
    let Some(pool) = pool(&state) else {
        return server_error();
    };
    let key = queries_v1::workspace_asset_key(&workspace_id.to_string(), &asset_key);
    let rows = match sqlx::query(&queries_v1::workspace_filter_sql())
        .bind(&key)
        .fetch_all(pool)
        .await
    {
        Ok(rows) => rows,
        Err(_) => return server_error(),
    };
    if rows.is_empty() {
        return miss_body();
    }
    let mut items = Vec::with_capacity(rows.len());
    for row in &rows {
        match file_asset_record(row, &actor.timezone) {
            Ok(record) => items.push(shape::row_to_json(&record)),
            Err(_) => return server_error(),
        }
    }
    found_envelope(items)
}

/// `FileAssetEndpoint.post` (`base.py:35-42`): validation errors answer
/// before the workspace lookup; a real file passes validation and then
/// dies in storage (ported 500).
async fn workspace_post(
    State(state): State<AppState>,
    Path((slug,)): Path<(String,)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let actor = match require_user(&state, extension).await {
        Ok(actor) => actor,
        Err(denial) => return denial.into_response(),
    };
    let _ = actor;
    let Some(pool) = pool(&state) else {
        return server_error();
    };
    match classify_workspace_post(content_type_of(&headers).as_deref(), &body) {
        PostBody::NoFile => return raw(StatusCode::BAD_REQUEST, NO_FILE_BODY),
        PostBody::File => {}
        other => return other.into_response().expect("rejection renders"),
    }
    // `Workspace.objects.get(slug=slug)` (`base.py:39`): a miss raises
    // into the 404 branch of `handle_exception`.
    let workspace = match sqlx::query(&queries_v1::workspace_lookup_sql())
        .bind(&slug)
        .fetch_optional(pool)
        .await
    {
        Ok(workspace) => workspace,
        Err(_) => return server_error(),
    };
    if workspace.is_none() {
        return does_not_exist();
    }
    // BUG (`S3Storage.__init__` never ran the django-storages setup):
    // the validated upload fails before the network with the generic
    // 500. No row is written; the storage seam stays untouched.
    unhandled()
}

/// Shared single-row flip for workspace delete and restore
/// (`base.py:44-49,52-58`): `.get()` miss 404s, otherwise only
/// `is_deleted` is written (ported bug: `deleted_at` untouched).
async fn workspace_flip(
    state: &AppState,
    workspace_id: &Uuid,
    asset_key: &str,
    deleted: bool,
) -> Response {
    let Some(pool) = pool(state) else {
        return server_error();
    };
    let key = queries_v1::workspace_asset_key(&workspace_id.to_string(), asset_key);
    let row = match sqlx::query(&queries_v1::workspace_get_sql())
        .bind(&key)
        .fetch_optional(pool)
        .await
    {
        Ok(row) => row,
        Err(_) => return server_error(),
    };
    let Some(row) = row else {
        return does_not_exist();
    };
    let id: Uuid = match row.try_get("id") {
        Ok(id) => id,
        Err(_) => return server_error(),
    };
    match sqlx::query(&queries_v1::set_deleted_sql())
        .bind(deleted)
        .bind(id)
        .execute(pool)
        .await
    {
        Ok(_) => no_content(),
        Err(_) => server_error(),
    }
}

/// `FileAssetEndpoint.delete` (`base.py:44-49`).
async fn workspace_delete(
    State(state): State<AppState>,
    Path((workspace_id_raw, asset_key)): Path<(String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Response {
    let Ok(workspace_id) = workspace_id_raw.parse::<Uuid>() else {
        return page_not_found();
    };
    if let Err(denial) = require_user(&state, extension).await {
        return denial.into_response();
    }
    workspace_flip(&state, &workspace_id, &asset_key, true).await
}

/// `FileAssetViewSet.restore` (`base.py:52-58`).
async fn workspace_restore(
    State(state): State<AppState>,
    Path((workspace_id_raw, asset_key)): Path<(String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Response {
    let Ok(workspace_id) = workspace_id_raw.parse::<Uuid>() else {
        return page_not_found();
    };
    if let Err(denial) = require_user(&state, extension).await {
        return denial.into_response();
    }
    workspace_flip(&state, &workspace_id, &asset_key, false).await
}

// ---------------------------------------------------------------------------
// UserAssetsEndpoint — creator scope, raw key
// ---------------------------------------------------------------------------

/// `UserAssetsEndpoint.get` (`base.py:64-73`): a miss 200s the miss
/// body; any existing row 500s (ported missing-`many=True` bug).
async fn user_get(
    State(state): State<AppState>,
    Path((asset_key,)): Path<(String,)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Response {
    let actor = match require_user(&state, extension).await {
        Ok(actor) => actor,
        Err(denial) => return denial.into_response(),
    };
    let Some(pool) = pool(&state) else {
        return server_error();
    };
    let rows = match sqlx::query(&queries_v1::user_filter_sql())
        .bind(&asset_key)
        .bind(actor.id)
        .fetch_all(pool)
        .await
    {
        Ok(rows) => rows,
        Err(_) => return server_error(),
    };
    if rows.is_empty() {
        return miss_body();
    }
    // BUG (`base.py:67`): the queryset is serialized without
    // `many=True`, so any existing row raises into the generic 500.
    // Port it exactly: never render the row here.
    unhandled()
}

/// `UserAssetsEndpoint.post` (`base.py:75-80`): no FK kwargs on save;
/// a real file dies in storage (ported 500).
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
    let _ = actor;
    match classify_user_post(content_type_of(&headers).as_deref(), &body) {
        PostBody::NoFile => return raw(StatusCode::BAD_REQUEST, NO_FILE_BODY),
        PostBody::File => {}
        other => return other.into_response().expect("rejection renders"),
    }
    // BUG (storage, as on workspace POST): the validated upload fails
    // before the network with the generic 500. No row is written.
    unhandled()
}

/// `UserAssetsEndpoint.delete` (`base.py:82-86`): the lookup scopes by
/// `created_by`, so another creator's row 404s.
async fn user_delete(
    State(state): State<AppState>,
    Path((asset_key,)): Path<(String,)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Response {
    let actor = match require_user(&state, extension).await {
        Ok(actor) => actor,
        Err(denial) => return denial.into_response(),
    };
    let Some(pool) = pool(&state) else {
        return server_error();
    };
    let row = match sqlx::query(&queries_v1::user_get_sql())
        .bind(&asset_key)
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
    let id: Uuid = match row.try_get("id") {
        Ok(id) => id,
        Err(_) => return server_error(),
    };
    match sqlx::query(&queries_v1::set_deleted_sql())
        .bind(true)
        .bind(id)
        .execute(pool)
        .await
    {
        Ok(_) => no_content(),
        Err(_) => server_error(),
    }
}

// ---------------------------------------------------------------------------
// Routes (the 5 legacy patterns, app/urls/asset.py:27-47)
// ---------------------------------------------------------------------------

/// Register the legacy v1 asset routes. Only the methods Django
/// implements are owned; every other method proxies so DRF's own
/// 405-after-auth and metadata responses are preserved byte for byte.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/workspaces/{slug}/file-assets/",
            crate::license::owned(post(workspace_post), &["POST"]),
        )
        .route(
            "/api/workspaces/file-assets/{workspace_id}/{asset_key}/",
            crate::license::owned(
                get(workspace_get).delete(workspace_delete),
                &["GET", "DELETE"],
            ),
        )
        .route(
            "/api/users/file-assets/",
            crate::license::owned(post(user_post), &["POST"]),
        )
        .route(
            "/api/users/file-assets/{asset_key}/",
            crate::license::owned(get(user_get).delete(user_delete), &["GET", "DELETE"]),
        )
        .route(
            "/api/workspaces/file-assets/{workspace_id}/{asset_key}/restore/",
            crate::license::owned(post(workspace_restore), &["POST"]),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn golden(rel: &str) -> Value {
        let path = format!(
            "{}/../../fixtures/app_assets/{rel}",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(&path).expect("golden exists"))
            .expect("golden parses")
    }

    #[test]
    fn miss_and_error_bodies_match_golden() {
        let v1 = golden("queries/v1.golden.json");
        assert_eq!(
            queries_v1::miss_body(),
            v1["endpoint_FileAssetEndpoint"]["get"]["empty_body"]
        );
        assert_eq!(
            queries_v1::miss_body(),
            v1["endpoint_UserAssetsEndpoint"]["get"]["empty_body"]
        );
        assert_eq!(
            queries_v1::not_found_body(),
            v1["endpoint_FileAssetEndpoint"]["delete"]["errors"]["unknown_key"]["body"]
        );
        assert_eq!(
            queries_v1::unhandled_body(),
            v1["error_envelopes"]["unhandled"]["body"]
        );
        assert_eq!(NO_FILE_BODY, r#"{"asset":["No file was submitted."]}"#);
        assert_eq!(
            serde_json::from_str::<Value>(NO_FILE_BODY).expect("valid json"),
            v1["endpoint_FileAssetEndpoint"]["post"]["errors"]["no_file"]["body"]
        );
    }

    #[test]
    fn status_contract_matches_queries_layer() {
        assert_eq!(queries_v1::FOUND_STATUS, 200);
        assert_eq!(queries_v1::MISS_STATUS, 200);
        assert_eq!(queries_v1::DELETE_STATUS, 204);
        assert_eq!(queries_v1::RESTORE_STATUS, 204);
        assert_eq!(queries_v1::NOT_FOUND_STATUS, 404);
        assert_eq!(queries_v1::USER_GET_FOUND_STATUS, 500);
    }

    #[test]
    fn workspace_post_accepts_json_but_user_post_415s() {
        // JSONParser quirk (`base.py:17` vs `base.py:62`).
        let body = br#"{"x": 1}"#;
        assert_eq!(
            classify_workspace_post(Some("application/json"), body),
            PostBody::NoFile
        );
        match classify_user_post(Some("application/json"), body) {
            PostBody::UnsupportedMediaType(ct) => assert_eq!(ct, "application/json"),
            other => panic!("user JSON must 415, got {other:?}"),
        }
    }

    #[test]
    fn empty_body_is_no_file_on_both_endpoints() {
        for classify in [classify_workspace_post, classify_user_post] {
            assert_eq!(classify(None, b""), PostBody::NoFile);
            assert_eq!(
                classify(Some("application/x-www-form-urlencoded"), b""),
                PostBody::NoFile
            );
        }
    }

    #[test]
    fn multipart_file_detection_is_asset_field_only() {
        let boundary = "AaB03x";
        let with_file = format!(
            "--{b}\r\nContent-Disposition: form-data; name=\"asset\"; filename=\"f.txt\"\r\nContent-Type: text/plain\r\n\r\nhello\r\n--{b}--\r\n",
            b = boundary
        );
        assert!(multipart_has_asset_file(with_file.as_bytes(), boundary));
        let text_only = format!(
            "--{b}\r\nContent-Disposition: form-data; name=\"asset\"\r\n\r\nnothing\r\n--{b}--\r\n",
            b = boundary
        );
        assert!(!multipart_has_asset_file(text_only.as_bytes(), boundary));
        let other_field = format!(
            "--{b}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"f.txt\"\r\n\r\nhi\r\n--{b}--\r\n",
            b = boundary
        );
        assert!(!multipart_has_asset_file(other_field.as_bytes(), boundary));
        assert_eq!(
            classify_workspace_post(
                Some(&format!("multipart/form-data; boundary={boundary}")),
                with_file.as_bytes()
            ),
            PostBody::File
        );
        assert_eq!(
            classify_user_post(
                Some(&format!("multipart/form-data; boundary={boundary}")),
                text_only.as_bytes()
            ),
            PostBody::NoFile
        );
    }

    #[test]
    fn malformed_bodies_reject_before_the_view() {
        match classify_workspace_post(Some("application/json"), b"{oops") {
            PostBody::Malformed(_) => {}
            other => panic!("bad JSON must 400, got {other:?}"),
        }
        match classify_user_post(Some("text/csv"), b"a,b") {
            PostBody::UnsupportedMediaType(ct) => assert_eq!(ct, "text/csv"),
            other => panic!("unknown type must 415, got {other:?}"),
        }
        match classify_workspace_post(Some("multipart/form-data"), b"x") {
            PostBody::Malformed(_) => {}
            other => panic!("missing boundary must 400, got {other:?}"),
        }
    }

    #[test]
    fn unsupported_media_type_body_shape() {
        let response = PostBody::UnsupportedMediaType("application/json".to_owned())
            .into_response()
            .expect("renders");
        assert_eq!(response.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }

    #[test]
    fn found_envelope_key_order_and_types() {
        let body = json!({"data": [], "status": true});
        let rendered = serde_json::to_string(&body).expect("renders");
        assert_eq!(rendered, r#"{"data":[],"status":true}"#);
        let miss = serde_json::to_string(&queries_v1::miss_body()).expect("renders");
        assert_eq!(
            miss,
            r#"{"error":"Asset key does not exist","status":false}"#
        );
    }

    #[test]
    fn routes_build_without_path_collisions() {
        // Merging twice would panic on duplicate paths; one merge must hold.
        let _ = crate::routes::with_routes(AppState::new("0.1.0"), routes());
    }

    #[test]
    fn guards_fixture_pins_v1_default_auth() {
        let guards = golden("guards/permissions.golden.json");
        let endpoints = guards["endpoints"].as_array().expect("endpoints");
        let v1: Vec<&str> = endpoints
            .iter()
            .filter_map(|e| {
                let name = e["endpoint"].as_str().unwrap_or("");
                name.contains("(v1").then_some(name)
            })
            .collect();
        assert_eq!(v1.len(), 3, "all three v1 units gate on default auth only");
        for name in v1 {
            let entry = endpoints
                .iter()
                .find(|e| e["endpoint"].as_str() == Some(name))
                .expect("entry");
            let gate = entry["gate"].as_str().unwrap_or("");
            assert!(
                gate.contains("IsAuthenticated")
                    || (gate.contains("none") && gate.contains("default")),
                "v1 gate is default auth: {name}"
            );
        }
    }
}
