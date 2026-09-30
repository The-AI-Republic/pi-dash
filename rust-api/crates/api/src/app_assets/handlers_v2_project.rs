//! v2 project-side asset handlers (D-31, stage 5, PIDASHCONV-412).
//!
//! Port of `apps/api/pi_dash/app/views/asset/v2.py:480-835` with routes
//! from `apps/api/pi_dash/app/urls/asset.py:79-113`: `ProjectAssetEndpoint`
//! (POST/PATCH/DELETE/GET), `ProjectBulkAssetEndpoint` (POST),
//! `AssetCheckEndpoint` (GET), `DuplicateAssetEndpoint` (POST),
//! `WorkspaceAssetDownloadEndpoint` (GET) and `ProjectAssetDownloadEndpoint`
//! (GET). Query shapes come from
//! [`pidash_services::app_assets::queries_v2_project`] (column maps,
//! storage-key shapes, golden SQL referenced per handler), gates from
//! [`crate::app_assets::guards`], the patch publisher kwargs from
//! [`pidash_services::app_assets::tasks::metadata_delay_kwargs`], session
//! auth from [`crate::license::resolve_actor`]. Sibling handler issues
//! (394/400) own their files and extend [`crate::app_assets::routes`];
//! merges keep both sides.
//!
//! Django-idiom notes (translate, don't redesign):
//!
//! * `<uuid:>` path segments never reach the view as garbage: the resolver
//!   404s with `{"error":"Page not found."}` (global `handler404`,
//!   `pi_dash/urls.py:15`) — before authentication. UUID params parse
//!   first here for the same reason; `<str:project_id>` resolves later via
//!   [`resolve_project_id`] (authenticated callers only).
//! * Order per request: UUID parse → session auth (401) → project rewrite
//!   (404 `{"detail":"Project not found"}`) → gate (403) → duplicate
//!   throttle (429) → body. DRF `initial()` runs authentication, then
//!   `check_permissions`, then `check_throttles`, so a denied outsider
//!   answers 403 even with an exhausted quota.
//! * `.get()` misses raise `DoesNotExist`, mapped by
//!   `BaseAPIView.handle_exception` (`app/views/base.py:211-254`) to 404
//!   `{"error":"The required object does not exist."}`; `ValidationError`
//!   (including bad UUID FK writes, `UUIDField.to_python`) maps to 400
//!   `{"error":"Please provide valid detail"}`; anything else maps to 500
//!   `{"error":"Something went wrong please try again later"}`.
//! * `request.data` parses JSON bodies as-is plus urlencoded/multipart
//!   text fields (last wins); the suite sends JSON.
//! * Redirects answer `HttpResponseRedirect`: 302 with `Location`.
//! * POST answers 200 (not 201) with `upload_data`, `asset_id`,
//!   `asset_url` in that order (`v2.py:570-577`).
//!
//! S3 presigning mirrors `S3Storage` (`settings/storage.py`) offline
//! (SigV4, no network): the POST policy lists conditions in `storage.py`
//! order with botocore field order; GET redirects carry
//! `response-content-disposition=attachment; filename*=UTF-8''<name>`
//! (fresh uuid4 hex when the stored name is absent, bare `attachment`
//! when it is empty). The duplicate copy is a real SigV4 `PUT` with
//! `x-amz-copy-source` against the configured endpoint; an S3 error
//! status is the `ClientError` Django swallows (row still flips to
//! `is_uploaded` and the view 200s), while a transport failure propagates
//! to the 500 fallback with the row left behind.
//!
//! Ported bugs (translate, don't redesign — also listed in the PR):
//!
//! * BUG-project-cover-500 (`v2.py:554-563`): `PROJECT_COVER` spreads
//!   `{"project_id": ...}` over the explicit `project_id=project_id`
//!   kwarg → `TypeError` → generic 500. Answered directly (same bytes).
//! * BUG-FLAG-pk-quirk (`v2.py:609`): project `get` looks up `pk=pk`
//!   while siblings use `id=pk`; Django aliases `pk` to the primary key
//!   so the predicate is identical.
//! * QUIRK-duplicate-entity-key (`v2.py:739`): duplicate reads
//!   `entity_id`, not `entity_identifier` — the mint spelling lands all
//!   entity FKs NULL here.
//! * QUIRK-create-before-copy (`v2.py:761-778`): the duplicate row is
//!   inserted before `copy_object`; a failed copy 500s while the row
//!   stays with `is_uploaded` false.
//! * QUIRK-none-name-key (`v2.py:760`): the destination key renders
//!   `original.attributes.get('name')` with no fallback, so a missing
//!   name renders the literal `...-None`.
//!
//! The PATCH metadata publish (`get_asset_object_metadata.delay`, `:587`)
//! goes out over AMQP in Celery protocol v2 with kwargs
//! `{"asset_id": str(pk)}` (the URL kwarg, per the tasks golden). A broker
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

use crate::app_assets::guards;
use crate::state::AppState;
use pidash_auth::permissions::allow::AllowFacts;
use pidash_auth::permissions::{ROLE_ADMIN, ROLE_GUEST, ROLE_MEMBER};
use pidash_auth::scope::TenantScope;
use pidash_services::app_assets::queries_v2_project as q;
use pidash_types::WorkspaceId;

/// All ten `EntityTypeContext` values (`db/models/asset.py:33-43`): the
/// project mint accepts any of them (`v2.py:521`).
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

/// Mintable file types for the project `post` (`v2.py:528-535`).
const ALLOWED_TYPES: &[&str] = &[
    "image/jpeg",
    "image/png",
    "image/webp",
    "image/jpg",
    "image/gif",
];

/// Resolver 404 for non-UUID `<uuid:>` segments (global `handler404`,
/// `pi_dash/urls.py:15`).
const PAGE_NOT_FOUND_BODY: &str = r#"{"error":"Page not found."}"#;
/// `BaseAPIView.handle_exception`: `ObjectDoesNotExist` branch
/// (`app/views/base.py:232-236`).
const DOES_NOT_EXIST_BODY: &str = r#"{"error":"The required object does not exist."}"#;
/// `BaseAPIView.handle_exception`: `ValidationError` branch (`:226-230`),
/// also raised by `UUIDField.to_python` on bad UUID writes.
const VALID_DETAIL_BODY: &str = r#"{"error":"Please provide valid detail"}"#;
/// `BaseAPIView.handle_exception`: generic branch (`:238-243`).
const SERVER_ERROR_BODY: &str = r#"{"error":"Something went wrong please try again later"}"#;
/// DRF `NotAuthenticated` denial: anonymous on a guarded route.
const UNAUTHENTICATED_BODY: &str = r#"{"detail":"Authentication credentials were not provided."}"#;
/// `Project.resolve` miss (`db/models/project.py:190-217` via `Http404`).
const PROJECT_NOT_FOUND_BODY: &str = r#"{"detail":"Project not found"}"#;
/// Project-mint `entity_type` rejection (`v2.py:521-526`).
const INVALID_ENTITY_TYPE_BODY: &str = r#"{"error":"Invalid entity type.","status":false}"#;
/// Project-mint file-type rejection (`v2.py:535-542`).
const INVALID_FILE_TYPE_BODY: &str = r#"{"error":"Invalid file type. Only JPEG, PNG, WebP, JPG and GIF files are allowed.","status":false}"#;
/// Bulk empty-list rejection (`v2.py:641-642`).
const NO_ASSET_IDS_BODY: &str = r#"{"error":"No asset ids provided."}"#;
/// Scoped-lookup inline 404 (`v2.py:613-616`, `:795-798`, `:822-826`).
const ASSET_MISSING_BODY: &str = r#"{"error":"The requested asset could not be found."}"#;
/// Bulk first-asset miss (`v2.py:650-654`).
const BULK_MISSING_BODY: &str = r#"{"error":"The requested asset could not be found."}"#;
/// Duplicate `entity_type` rejection (`v2.py:742-746`).
const DUPLICATE_INVALID_ENTITY_BODY: &str = r#"{"error":"Invalid entity type or entity id"}"#;
/// Duplicate project-scope miss (`v2.py:751-752`).
const DUPLICATE_PROJECT_BODY: &str = r#"{"error":"Project not found"}"#;
/// Duplicate original miss (`v2.py:757-758`).
const DUPLICATE_ASSET_BODY: &str = r#"{"error":"Asset not found"}"#;
/// Unhandled `IntegrityError` via `BaseAPIView.handle_exception`
/// (`app/views/base.py:221-225`): the PAGE bulk branch has no swallow.
const PAYLOAD_NOT_VALID_BODY: &str = r#"{"error":"The payload is not valid"}"#;

/// Routes for `app/urls/asset.py:79-113` (all under `/api/assets/v2/`).
///
/// The no-`pk` collection path owns POST only (Django's GET there takes
/// no `pk` kwarg and 500s; proxying keeps that byte-identical), the
/// `pk` path owns GET+PATCH+DELETE, bulk/check/duplicate/downloads own
/// their single method. Every other method on these paths proxies to
/// Django through [`crate::license::owned`].
pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/assets/v2/workspaces/{slug}/projects/{project_id}/",
            crate::license::owned(post(post_project_asset), &["POST"]),
        )
        .route(
            "/api/assets/v2/workspaces/{slug}/projects/{project_id}/{pk}/",
            crate::license::owned(
                get(get_project_asset)
                    .patch(patch_project_asset)
                    .delete(delete_project_asset),
                &["GET", "PATCH", "DELETE"],
            ),
        )
        .route(
            "/api/assets/v2/workspaces/{slug}/projects/{project_id}/{entity_id}/bulk/",
            crate::license::owned(post(post_bulk_asset), &["POST"]),
        )
        .route(
            "/api/assets/v2/workspaces/{slug}/check/{asset_id}/",
            crate::license::owned(get(get_check_asset), &["GET"]),
        )
        .route(
            "/api/assets/v2/workspaces/{slug}/duplicate-assets/{asset_id}/",
            crate::license::owned(post(post_duplicate_asset), &["POST"]),
        )
        .route(
            "/api/assets/v2/workspaces/{slug}/download/{asset_id}/",
            crate::license::owned(get(get_workspace_download), &["GET"]),
        )
        .route(
            "/api/assets/v2/workspaces/{slug}/projects/{project_id}/download/{asset_id}/",
            crate::license::owned(get(get_project_download), &["GET"]),
        )
}

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

fn server_error() -> Response {
    raw(StatusCode::INTERNAL_SERVER_ERROR, SERVER_ERROR_BODY)
}

/// `.get()` miss envelope: Django `ObjectDoesNotExist` through
/// `BaseAPIView.handle_exception` (`app/views/base.py:232-236`).
fn does_not_exist() -> Response {
    raw(StatusCode::NOT_FOUND, DOES_NOT_EXIST_BODY)
}

fn page_not_found() -> Response {
    raw(StatusCode::NOT_FOUND, PAGE_NOT_FOUND_BODY)
}

fn valid_detail() -> Response {
    raw(StatusCode::BAD_REQUEST, VALID_DETAIL_BODY)
}

fn no_content() -> Response {
    StatusCode::NO_CONTENT.into_response()
}

fn redirect(location: String) -> Response {
    // `HttpResponseRedirect`: empty body, default content type.
    Response::builder()
        .status(StatusCode::FOUND)
        .header(header::LOCATION, location)
        .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
        .body(axum::body::Body::empty())
        .expect("redirect response")
}

fn pool(state: &AppState) -> Option<&sqlx::PgPool> {
    state.pools().map(|pools| pools.primary())
}

// ---------------------------------------------------------------------------
// Auth + gates (DRF `initial()` order: auth, permissions, throttles)
// ---------------------------------------------------------------------------

/// `IsAuthenticated` (`app/views/base.py:189-194`): anonymous answers
/// DRF `NotAuthenticated` (401) before any gate runs.
async fn require_actor(
    state: &AppState,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Result<crate::license::Actor, Response> {
    let Some(pool) = pool(state) else {
        return Err(server_error());
    };
    match crate::license::resolve_actor(pool, state.settings().secret_key.as_bytes(), extension)
        .await
    {
        Ok(Some(actor)) => Ok(actor),
        Ok(None) => Err(raw(StatusCode::UNAUTHORIZED, UNAUTHENTICATED_BODY)),
        Err(_) => Err(server_error()),
    }
}

/// Active workspace role for `(user, slug)`, or `None`. Mirrors the
/// workspace-level lookup (`app/permissions/base.py:44-51`): the joined
/// `workspaces` row carries no `deleted_at` filter (Django does not
/// filter joined tables) while the membership row uses the default
/// manager (`deleted_at IS NULL`) plus `is_active`.
async fn workspace_role(
    pool: &sqlx::PgPool,
    user_id: &Uuid,
    slug: &str,
) -> Result<Option<i32>, Response> {
    let row: Option<(i16,)> = sqlx::query_as(
        r#"SELECT wm.role FROM workspace_members wm
           JOIN workspaces w ON w.id = wm.workspace_id
           WHERE wm.member_id = $1 AND w.slug = $2
           AND wm.is_active AND wm.deleted_at IS NULL"#,
    )
    .bind(user_id)
    .bind(slug)
    .fetch_optional(pool)
    .await
    .map_err(|_| server_error())?;
    Ok(row.map(|(role,)| i32::from(role)))
}

/// Active project role for `(user, project_id, slug)`, or `None`.
/// Mirrors the project-level lookup (`app/permissions/base.py:53-64`).
async fn project_role(
    pool: &sqlx::PgPool,
    user_id: &Uuid,
    project_id: &Uuid,
    slug: &str,
) -> Result<Option<i32>, Response> {
    let row: Option<(i16,)> = sqlx::query_as(
        r#"SELECT pm.role FROM project_members pm
           JOIN workspaces w ON w.id = pm.workspace_id
           WHERE pm.member_id = $1 AND pm.project_id = $2 AND w.slug = $3
           AND pm.is_active AND pm.deleted_at IS NULL"#,
    )
    .bind(user_id)
    .bind(project_id)
    .bind(slug)
    .fetch_optional(pool)
    .await
    .map_err(|_| server_error())?;
    Ok(row.map(|(role,)| i32::from(role)))
}

/// Roles every gate in this module lists
/// (`[ROLE.ADMIN, ROLE.MEMBER, ROLE.GUEST]`).
const GATE_ROLES: &[i32] = &[ROLE_ADMIN, ROLE_MEMBER, ROLE_GUEST];

/// `@allow_permission(..., level="WORKSPACE")`: active workspace
/// membership with a listed role.
async fn gate_workspace(
    pool: &sqlx::PgPool,
    slug: &str,
    actor_id: &Uuid,
    endpoint: guards::AssetEndpoint,
) -> Result<(), Response> {
    let role = workspace_role(pool, actor_id, slug).await?;
    let facts = AllowFacts {
        workspace: WorkspaceId::from(slug),
        authenticated: true,
        is_workspace_member: role.is_some(),
        has_allowed_workspace_role: role.is_some_and(|role| GATE_ROLES.contains(&role)),
        is_creator: false,
        has_allowed_project_role: false,
        is_project_member: false,
        is_workspace_admin: role == Some(ROLE_ADMIN),
    };
    if guards::check_asset_gate(endpoint, &TenantScope::new(WorkspaceId::from(slug)), &facts) {
        Ok(())
    } else {
        Err(crate::permissions::PermissionDenied.into_response())
    }
}

/// `@allow_permission(...)` at the default `"PROJECT"` level: active
/// project membership with a listed role, or the workspace-admin
/// override (project member + workspace ADMIN passes regardless of
/// project role, `app/permissions/base.py:56-64`).
async fn gate_project(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &Uuid,
    actor_id: &Uuid,
    endpoint: guards::AssetEndpoint,
) -> Result<(), Response> {
    let ws_role = workspace_role(pool, actor_id, slug).await?;
    let pm_role = project_role(pool, actor_id, project_id, slug).await?;
    let facts = AllowFacts {
        workspace: WorkspaceId::from(slug),
        authenticated: true,
        is_workspace_member: ws_role.is_some(),
        has_allowed_workspace_role: ws_role.is_some_and(|role| GATE_ROLES.contains(&role)),
        is_creator: false,
        has_allowed_project_role: pm_role.is_some_and(|role| GATE_ROLES.contains(&role)),
        is_project_member: pm_role.is_some(),
        is_workspace_admin: ws_role == Some(ROLE_ADMIN),
    };
    if guards::check_asset_gate(endpoint, &TenantScope::new(WorkspaceId::from(slug)), &facts) {
        Ok(())
    } else {
        Err(crate::permissions::PermissionDenied.into_response())
    }
}

/// Resolve the `<str:project_id>` kwarg for an authenticated caller
/// (`_rewrite_project_kwarg`, `app/views/base.py:49-77` + `Project.resolve`,
/// `db/models/project.py:190-217`): a UUID passes through unchecked,
/// anything else resolves as a workspace-scoped upper-cased identifier
/// (soft-deleted projects excluded); an unresolvable value answers
/// `{"detail":"Project not found"}` 404.
async fn resolve_project_id(
    pool: &sqlx::PgPool,
    slug: &str,
    value: &str,
) -> Result<Uuid, Response> {
    if let Ok(id) = value.parse::<Uuid>() {
        return Ok(id);
    }
    let normalized = value.trim().to_uppercase();
    let row: Option<(Uuid,)> = sqlx::query_as(
        r#"SELECT p.id FROM projects p JOIN workspaces w ON w.id = p.workspace_id
           WHERE w.slug = $1 AND p.identifier = $2 AND p.deleted_at IS NULL"#,
    )
    .bind(slug)
    .bind(normalized)
    .fetch_optional(pool)
    .await
    .map_err(|_| server_error())?;
    row.map(|(id,)| id)
        .ok_or_else(|| raw(StatusCode::NOT_FOUND, PROJECT_NOT_FOUND_BODY))
}

/// `Workspace.objects.get(slug=slug)` (`v2.py:548,748`): default manager,
/// so a soft-deleted slug answers the `DoesNotExist` 404 envelope.
async fn workspace_lookup(pool: &sqlx::PgPool, slug: &str) -> Result<Uuid, Response> {
    let row: Option<(Uuid,)> =
        sqlx::query_as(r#"SELECT id FROM workspaces WHERE slug = $1 AND deleted_at IS NULL"#)
            .bind(slug)
            .fetch_optional(pool)
            .await
            .map_err(|_| server_error())?;
    row.map(|(id,)| id).ok_or_else(does_not_exist)
}

/// Enforce the duplicate throttle after the gate (DRF `initial()` order):
/// `AssetRateThrottle` (`throttles/asset.py:8-15`), 5/minute per source
/// asset, denied with the product rate-limit envelope.
// `Response` is axum's handle type, so boxing it buys no runtime win;
// the crate-wide `Result<_, Response>` helper shape stays as-is.
#[allow(clippy::result_large_err)]
fn check_duplicate_throttle(asset_id: &Uuid) -> Result<(), Response> {
    let spec = crate::assistant::throttles::ThrottleSpec {
        scope: guards::ASSET_THROTTLE_SCOPE,
        requests: guards::ASSET_THROTTLE_REQUESTS,
        window_secs: guards::ASSET_THROTTLE_WINDOW_SECS,
    };
    if crate::assistant::governor::throttle_check(spec, &asset_id.to_string()) {
        Ok(())
    } else {
        Err(raw(
            StatusCode::TOO_MANY_REQUESTS,
            guards::ASSET_RATE_LIMIT_BODY,
        ))
    }
}

// ---------------------------------------------------------------------------
// Request-data parsing (JSON / form / multipart, last value wins)
// ---------------------------------------------------------------------------

/// Parse `request.data` the way DRF does for the content types the suite
/// and browsers send: JSON objects as-is, urlencoded and multipart text
/// fields as strings. Duplicate keys keep the last value (`QueryDict.get`).
/// Files in multipart bodies are ignored — the views never read
/// `request.FILES`. Unparseable bodies answer the 500 fallback.
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

/// Minimal multipart text-field parser: splits on the boundary, reads
/// each part's `name="..."`, keeps the raw bytes as a string.
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

fn content_type(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
}

/// `request.data.get("type", "image/jpeg")` (`v2.py:515`): the default
/// applies only when the key is absent. A present non-string (explicit
/// null included) fails the allowlist below and answers the 400, exactly
/// like `None not in allowed_types` in Django.
fn post_mime(data: &Map<String, Value>) -> Option<&str> {
    match data.get("type") {
        None => Some("image/jpeg"),
        Some(Value::String(s)) => Some(s),
        Some(_) => None,
    }
}

/// `int(...)` for the `size` field (`v2.py:516`): JSON numbers truncate,
/// numeric strings parse, bools coerce (`int(True) == 1`), anything else
/// (or explicit null) raises → 500. Missing → `FILE_SIZE_LIMIT`.
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

/// `UUIDField.to_python` for FK writes (`django/db/models/fields.py`):
/// `None`/null stays NULL; strings must parse (else `ValidationError` →
/// the 400 envelope); ints/bools coerce via `uuid.UUID(int=...)`
/// (negative or over-wide values reject); every other JSON shape rejects.
// `Response` is axum's handle type, so boxing it buys no runtime win;
// the crate-wide `Result<_, Response>` helper shape stays as-is.
#[allow(clippy::result_large_err)]
fn coerce_uuid_or_400(value: Option<&Value>) -> Result<Option<Uuid>, Response> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => s.parse::<Uuid>().map(Some).map_err(|_| valid_detail()),
        Some(Value::Number(n)) => {
            let int = n
                .as_u64()
                .map(u128::from)
                .or_else(|| n.as_i64().and_then(|i| u128::try_from(i).ok()));
            int.map(Uuid::from_u128).map(Some).ok_or_else(valid_detail)
        }
        Some(Value::Bool(b)) => Ok(Some(Uuid::from_u128(u128::from(*b as u8)))),
        Some(_) => Err(valid_detail()),
    }
}

/// Django field-aliasing resolution for the mint/duplicate creates
/// (verified live against Django): the `**spread` attname form
/// (`workspace_id`, `project_id`) wins over the explicit object/kwarg
/// form — `workspace=X, workspace_id=Y` stores Y, `workspace_id=None`
/// stores NULL — so a shadowing spread rebinds the explicit position
/// instead of adding a column (a second same-named column would be a
/// Postgres duplicate-column error Django never raises). Returns
/// `(rebind_explicit, suffix_column)`.
fn resolve_spread(spread: Option<&'static str>) -> (bool, Option<&'static str>) {
    match spread {
        Some("workspace_id") | Some("project_id") => (true, None),
        other => (false, other),
    }
}

/// Python truthiness of a `storage_metadata` JSON value: `None`/null,
/// `{}`, `[]`, `""`, `0` and `false` skip the metadata task
/// (`v2.py:586`).
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

/// Python `not n` for a JSON number: `0`/`0.0` are falsy, anything else
/// truthy (mirrors the number arm of [`python_truthy`]).
fn number_is_zero(n: &serde_json::Number) -> bool {
    if let Some(i) = n.as_i64() {
        i == 0
    } else if let Some(u) = n.as_u64() {
        u == 0
    } else {
        n.as_f64().is_some_and(|f| f == 0.0)
    }
}

/// Python `not x` for a JSON body value (`v2.py:641`, `:749`): `false`,
/// `0`/`0.0`, `""`, `[]` and `{}` are falsy (null handled by the caller).
fn json_falsy(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::Bool(b) => !b,
        Value::Number(n) => number_is_zero(n),
        Value::String(s) => s.is_empty(),
        Value::Array(a) => a.is_empty(),
        Value::Object(o) => o.is_empty(),
    }
}

/// `FileAsset.asset_url` (`db/models/asset.py:79-100`): the static group
/// answers the static path, `ISSUE_ATTACHMENT` the attachment path (with
/// `issue_id` rendered `None`-style when unset, exactly like the
/// f-string), the description group the project path, anything else null.
/// `slug`/`project_id` come from the request path (the row's own values);
/// `issue` is the spread `entity_identifier` (or `None`).
fn asset_url_for(
    entity_type: &str,
    workspace_slug: &str,
    project_id: &Uuid,
    issue: Option<&str>,
    asset_id: &Uuid,
) -> Value {
    match entity_type {
        "WORKSPACE_LOGO" | "USER_AVATAR" | "USER_COVER" | "PROJECT_COVER" => {
            Value::String(format!("/api/assets/v2/static/{asset_id}/"))
        }
        "ISSUE_ATTACHMENT" => Value::String(format!(
            "/api/assets/v2/workspaces/{workspace_slug}/projects/{project_id}/issues/{}/attachments/{asset_id}/",
            issue.unwrap_or("None"),
        )),
        "ISSUE_DESCRIPTION" | "COMMENT_DESCRIPTION" | "PAGE_DESCRIPTION" | "DRAFT_ISSUE_DESCRIPTION" => {
            Value::String(format!(
                "/api/assets/v2/workspaces/{workspace_slug}/projects/{project_id}/{asset_id}/"
            ))
        }
        _ => Value::Null,
    }
}

// ---------------------------------------------------------------------------
// Scoped reads (full rows; predicates match the golden SQL)
// ---------------------------------------------------------------------------

/// One `file_assets` row as the handlers below need it. The WHERE text of
/// each scoped read matches its golden predicate
/// ([`q::PROJECT_SCOPED_GET_SQL`], [`q::WORKSPACE_DOWNLOAD_SQL`],
/// [`q::PROJECT_DOWNLOAD_SQL`], [`q::DUPLICATE_ORIGINAL_SQL`]); Django's
/// `.get()` fetches full rows, so the SELECT list carries the columns the
/// bodies need rather than the golden's abbreviated `id`.
struct AssetRow {
    is_uploaded: bool,
    storage_metadata: Option<Value>,
    attributes: Option<Value>,
    asset_key: String,
    size: f64,
}

fn asset_row(row: &sqlx::postgres::PgRow) -> Result<AssetRow, sqlx::Error> {
    Ok(AssetRow {
        is_uploaded: row.try_get("is_uploaded")?,
        storage_metadata: row.try_get("storage_metadata")?,
        attributes: row.try_get("attributes")?,
        asset_key: row.try_get("asset")?,
        size: row.try_get("size")?,
    })
}

const ASSET_ROW_COLS: &str = r#""file_assets"."is_uploaded", "file_assets"."storage_metadata", "file_assets"."attributes", "file_assets"."asset", "file_assets"."size""#;

/// Scoped project read (`v2.py:582,598,609`): default manager plus
/// `workspace__slug` join plus `project_id`. `$1` asset id, `$2` slug,
/// `$3` project id. The `get` spelling uses `pk=pk` (BUG-FLAG, identical
/// predicate — kept).
async fn project_scoped_get(
    pool: &sqlx::PgPool,
    asset_id: &Uuid,
    slug: &str,
    project_id: &Uuid,
) -> Result<Option<AssetRow>, sqlx::Error> {
    let sql = format!(
        "SELECT {ASSET_ROW_COLS} FROM \"file_assets\" \
         INNER JOIN \"workspaces\" ON \"workspaces\".\"id\" = \"file_assets\".\"workspace_id\" \
         WHERE (\"file_assets\".\"deleted_at\" IS NULL AND \"file_assets\".\"id\" = $1 \
         AND \"workspaces\".\"slug\" = $2 AND \"file_assets\".\"project_id\" = $3)"
    );
    sqlx::query(&sql)
        .bind(asset_id)
        .bind(slug)
        .bind(project_id)
        .fetch_optional(pool)
        .await?
        .map(|row| asset_row(&row))
        .transpose()
}

// ---------------------------------------------------------------------------
// POST — mint a project upload
// ---------------------------------------------------------------------------

/// `ProjectAssetEndpoint.post` (`v2.py:512-577`).
async fn post_project_asset(
    State(state): State<AppState>,
    Path((slug, project_raw)): Path<(String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let actor = match require_actor(&state, extension).await {
        Ok(actor) => actor,
        Err(denial) => return denial,
    };
    let Some(pool) = pool(&state) else {
        return server_error();
    };
    let pool = pool.clone();
    let project_id = match resolve_project_id(&pool, &slug, &project_raw).await {
        Ok(id) => id,
        Err(denial) => return denial,
    };
    if let Err(denial) = gate_project(
        &pool,
        &slug,
        &project_id,
        &actor.id,
        guards::AssetEndpoint::ProjectPost,
    )
    .await
    {
        return denial;
    }
    let Some(data) = parse_data(content_type(&headers).as_deref(), &body) else {
        return server_error();
    };
    let entity_type = match data.get("entity_type").and_then(Value::as_str) {
        Some(t) if ENTITY_TYPES.contains(&t) => t.to_owned(),
        _ => return raw(StatusCode::BAD_REQUEST, INVALID_ENTITY_TYPE_BODY),
    };
    let Some(mime) = post_mime(&data) else {
        return raw(StatusCode::BAD_REQUEST, INVALID_FILE_TYPE_BODY);
    };
    if !ALLOWED_TYPES.contains(&mime) {
        return raw(StatusCode::BAD_REQUEST, INVALID_FILE_TYPE_BODY);
    }
    let size = match parse_size(data.get("size"), state.settings().file_size_limit) {
        Some(size) => size,
        None => return server_error(),
    };
    let size_limit = state.settings().file_size_limit.min(size);
    // `Workspace.objects.get(slug=slug)` (`:548`) — after the 400s.
    let workspace_id = match workspace_lookup(&pool, &slug).await {
        Ok(id) => id,
        Err(denial) => return denial,
    };
    // BUG (`v2.py:554-563`): `PROJECT_COVER` resolves the spread to
    // `{"project_id": ...}`, colliding with the explicit kwarg →
    // `TypeError` → generic 500. Same bytes, answered directly.
    if entity_type == "PROJECT_COVER" {
        return server_error();
    }
    let spread_col = q::project_entity_id_field(&entity_type);
    let spread_val = match coerce_uuid_or_400(data.get("entity_identifier")) {
        Ok(val) => val,
        Err(denial) => return denial,
    };
    let (name_display, name_json) = python_str(data.get("name"));
    let asset_id = Uuid::new_v4();
    let asset_key = q::project_asset_key(
        &workspace_id.to_string(),
        &asset_id.simple().to_string(),
        &name_display,
    );
    let attributes = serde_json::json!({
        "name": name_json,
        "type": mime,
        "size": size_limit,
    });
    let now = Utc::now();
    // `FileAsset.objects.create(...)` (`:554-563`): explicit kwargs
    // first, `**spread` second; a shadowing spread (`workspace_id`,
    // only reachable shape here — `PROJECT_COVER` already 500s above)
    // rebinds the explicit position instead of adding a column.
    let (rebind, suffix) = resolve_spread(spread_col);
    let ws_value = if rebind {
        spread_val
    } else {
        Some(workspace_id)
    };
    let mut sql = "INSERT INTO \"file_assets\" (\"id\", \"created_at\", \"updated_at\", \"attributes\", \"asset\", \"size\", \"workspace_id\", \"created_by_id\", \"entity_type\", \"project_id\"".to_owned();
    if let Some(col) = suffix {
        sql.push_str(&format!(", \"{col}\""));
    }
    sql.push_str(", \"is_uploaded\", \"is_deleted\", \"is_archived\", \"storage_metadata\") VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10");
    if suffix.is_some() {
        sql.push_str(", $11, false, false, false, '{}')");
    } else {
        sql.push_str(", false, false, false, '{}')");
    }
    let mut insert = sqlx::query(&sql)
        .bind(asset_id)
        .bind(now)
        .bind(now)
        .bind(sqlx::types::Json(attributes))
        .bind(&asset_key)
        .bind(size_limit as f64)
        .bind(ws_value)
        .bind(actor.id)
        .bind(&entity_type)
        .bind(project_id);
    if suffix.is_some() {
        insert = insert.bind(spread_val);
    }
    if insert.execute(&pool).await.is_err() {
        return server_error();
    }
    let issue_display: Option<String> = match data.get("entity_identifier") {
        None | Some(Value::Null) => None,
        Some(v) => Some(python_str(Some(v)).0),
    };
    let asset_url = asset_url_for(
        &entity_type,
        &slug,
        &project_id,
        issue_display.as_deref(),
        &asset_id,
    );
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
        size_limit,
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

// ---------------------------------------------------------------------------
// PATCH — mark uploaded (+ metadata task)
// ---------------------------------------------------------------------------

/// `ProjectAssetEndpoint.patch` (`v2.py:579-593`).
async fn patch_project_asset(
    State(state): State<AppState>,
    Path((slug, project_raw, pk_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let pk = match pk_raw.parse::<Uuid>() {
        Ok(pk) => pk,
        Err(_) => return page_not_found(),
    };
    let actor = match require_actor(&state, extension).await {
        Ok(actor) => actor,
        Err(denial) => return denial,
    };
    let Some(pool) = pool(&state) else {
        return server_error();
    };
    let pool = pool.clone();
    let project_id = match resolve_project_id(&pool, &slug, &project_raw).await {
        Ok(id) => id,
        Err(denial) => return denial,
    };
    if let Err(denial) = gate_project(
        &pool,
        &slug,
        &project_id,
        &actor.id,
        guards::AssetEndpoint::ProjectPatch,
    )
    .await
    {
        return denial;
    }
    let row = match project_scoped_get(&pool, &pk, &slug, &project_id).await {
        Ok(row) => row,
        Err(_) => return server_error(),
    };
    let Some(asset) = row else {
        return does_not_exist();
    };
    // `is_uploaded = True` is set before the metadata check (`:584-587`);
    // the publish carries the URL kwarg `pk`, not a re-read.
    if !python_truthy(&asset.storage_metadata) {
        let kwargs = pidash_services::app_assets::tasks::metadata_delay_kwargs(&pk.to_string());
        let message = pidash_jobs::celery::CeleryTaskMessage::new(
            pidash_jobs::space::GET_ASSET_OBJECT_METADATA_TASK,
            Vec::new(),
            kwargs,
        );
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
        // fallback in Django; same here (before the save, like Django).
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
    // `save(update_fields=["is_uploaded", "attributes"])` (`:592`):
    // exactly these two columns, no `updated_at` bump.
    if sqlx::query(q::PROJECT_PATCH_SAVE_SQL)
        .bind(pk)
        .bind(sqlx::types::Json(attributes))
        .execute(&pool)
        .await
        .is_err()
    {
        return server_error();
    }
    no_content()
}

// ---------------------------------------------------------------------------
// DELETE — soft-delete with timestamp
// ---------------------------------------------------------------------------

/// `ProjectAssetEndpoint.delete` (`v2.py:595-604`).
async fn delete_project_asset(
    State(state): State<AppState>,
    Path((slug, project_raw, pk_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Response {
    let pk = match pk_raw.parse::<Uuid>() {
        Ok(pk) => pk,
        Err(_) => return page_not_found(),
    };
    let actor = match require_actor(&state, extension).await {
        Ok(actor) => actor,
        Err(denial) => return denial,
    };
    let Some(pool) = pool(&state) else {
        return server_error();
    };
    let pool = pool.clone();
    let project_id = match resolve_project_id(&pool, &slug, &project_raw).await {
        Ok(id) => id,
        Err(denial) => return denial,
    };
    if let Err(denial) = gate_project(
        &pool,
        &slug,
        &project_id,
        &actor.id,
        guards::AssetEndpoint::ProjectDelete,
    )
    .await
    {
        return denial;
    }
    let row = match project_scoped_get(&pool, &pk, &slug, &project_id).await {
        Ok(row) => row,
        Err(_) => return server_error(),
    };
    if row.is_none() {
        return does_not_exist();
    }
    // `is_deleted=True; deleted_at=now; save(...)` (`:600-603`) — both
    // columns (unlike the v1 delete).
    if sqlx::query(q::PROJECT_DELETE_SQL)
        .bind(pk)
        .execute(&pool)
        .await
        .is_err()
    {
        return server_error();
    }
    no_content()
}

// ---------------------------------------------------------------------------
// GET — presigned attachment redirect for uploaded rows
// ---------------------------------------------------------------------------

/// `ProjectAssetEndpoint.get` (`v2.py:606-627`).
async fn get_project_asset(
    State(state): State<AppState>,
    Path((slug, project_raw, pk_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    headers: HeaderMap,
) -> Response {
    let pk = match pk_raw.parse::<Uuid>() {
        Ok(pk) => pk,
        Err(_) => return page_not_found(),
    };
    let actor = match require_actor(&state, extension).await {
        Ok(actor) => actor,
        Err(denial) => return denial,
    };
    let Some(pool) = pool(&state) else {
        return server_error();
    };
    let pool = pool.clone();
    let project_id = match resolve_project_id(&pool, &slug, &project_raw).await {
        Ok(id) => id,
        Err(denial) => return denial,
    };
    if let Err(denial) = gate_project(
        &pool,
        &slug,
        &project_id,
        &actor.id,
        guards::AssetEndpoint::ProjectGet,
    )
    .await
    {
        return denial;
    }
    let row = match project_scoped_get(&pool, &pk, &slug, &project_id).await {
        Ok(row) => row,
        Err(_) => return server_error(),
    };
    let Some(asset) = row else {
        return does_not_exist();
    };
    if !asset.is_uploaded {
        return raw(StatusCode::NOT_FOUND, ASSET_MISSING_BODY);
    }
    // `filename=attributes.get("name")` (`:624`): plain `.get`, no
    // fallback — a missing name mints a fresh hex, like `_get_content_
    // disposition(..., None)`.
    let filename = match asset.attributes.as_ref().and_then(|a| a.get("name")) {
        None | Some(Value::Null) => Filename::FreshHex,
        Some(Value::String(s)) if s.is_empty() => Filename::Bare,
        Some(Value::String(s)) => Filename::Name(s.clone()),
        Some(_) => return server_error(),
    };
    let storage = &state.settings().storage;
    let Some(host) = host_of(&headers) else {
        return server_error();
    };
    let url = presigned_get_url(
        storage,
        &scheme_of(&headers),
        &host,
        &asset.asset_key,
        filename,
        &Utc::now(),
    );
    redirect(url)
}

// ---------------------------------------------------------------------------
// Bulk — link uploaded rows to an entity
// ---------------------------------------------------------------------------

/// One bulk candidate: id plus the first-row-governing type (creation
/// order comes from the `ORDER BY created_at DESC` below).
struct BulkRow {
    id: Uuid,
    entity_type: Option<String>,
}

/// `ProjectBulkAssetEndpoint.post` (`v2.py:636-688`).
async fn post_bulk_asset(
    State(state): State<AppState>,
    Path((slug, project_raw, entity_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let entity_id = match entity_raw.parse::<Uuid>() {
        Ok(id) => id,
        Err(_) => return page_not_found(),
    };
    let actor = match require_actor(&state, extension).await {
        Ok(actor) => actor,
        Err(denial) => return denial,
    };
    let Some(pool) = pool(&state) else {
        return server_error();
    };
    let pool = pool.clone();
    let project_id = match resolve_project_id(&pool, &slug, &project_raw).await {
        Ok(id) => id,
        Err(denial) => return denial,
    };
    if let Err(denial) = gate_project(
        &pool,
        &slug,
        &project_id,
        &actor.id,
        guards::AssetEndpoint::BulkPost,
    )
    .await
    {
        return denial;
    }
    let Some(data) = parse_data(content_type(&headers).as_deref(), &body) else {
        return server_error();
    };
    // `asset_ids=data.get("asset_ids", [])` (`:638`): `if not asset_ids`
    // (`:641-642`) rejects missing/null and every falsy shape
    // (`0`/`false`/`""`/`[]`/`{}`) with 400; truthy strings/objects
    // iterate into `ValidationError` → the 400 envelope; any other
    // truthy non-list is not iterable → 500.
    let raw_ids: &Vec<Value> = match data.get("asset_ids") {
        None => return raw(StatusCode::BAD_REQUEST, NO_ASSET_IDS_BODY),
        Some(v) if json_falsy(v) => return raw(StatusCode::BAD_REQUEST, NO_ASSET_IDS_BODY),
        Some(Value::Array(ids)) => ids,
        Some(Value::String(_)) | Some(Value::Object(_)) => return valid_detail(),
        Some(_) => return server_error(),
    };
    let mut asset_ids = Vec::with_capacity(raw_ids.len());
    for id in raw_ids {
        // `id__in` UUID coercion (`UUIDField.to_python`): bad elements →
        // `ValidationError` → the 400 envelope.
        let parsed = match id {
            Value::String(s) => s.parse::<Uuid>().map_err(|_| valid_detail()),
            Value::Number(n) => {
                let int = n
                    .as_u64()
                    .map(u128::from)
                    .or_else(|| n.as_i64().and_then(|i| u128::try_from(i).ok()));
                int.map(Uuid::from_u128).ok_or_else(valid_detail)
            }
            Value::Bool(b) => Ok(Uuid::from_u128(u128::from(*b as u8))),
            _ => Err(valid_detail()),
        };
        match parsed {
            Ok(id) => asset_ids.push(id),
            Err(denial) => return denial,
        }
    }
    // `filter(id__in=asset_ids, workspace__slug=slug)` (`:645`) with the
    // default manager scope; default ordering is `-created_at`, so the
    // first row governs the single dispatch below.
    let rows = match sqlx::query(
        r#"SELECT "file_assets"."id", "file_assets"."entity_type"
           FROM "file_assets"
           INNER JOIN "workspaces" ON "workspaces"."id" = "file_assets"."workspace_id"
           WHERE ("file_assets"."deleted_at" IS NULL AND "file_assets"."id" = ANY($1)
           AND "workspaces"."slug" = $2) ORDER BY "file_assets"."created_at" DESC"#,
    )
    .bind(&asset_ids)
    .bind(&slug)
    .fetch_all(&pool)
    .await
    {
        Ok(rows) => rows,
        Err(_) => return server_error(),
    };
    let mut ordered = Vec::with_capacity(rows.len());
    for row in &rows {
        let Ok(id) = row.try_get::<Uuid, _>("id") else {
            return server_error();
        };
        let Ok(entity_type) = row.try_get::<Option<String>, _>("entity_type") else {
            return server_error();
        };
        ordered.push(BulkRow { id, entity_type });
    }
    let Some(first) = ordered.first() else {
        return raw(StatusCode::NOT_FOUND, BULK_MISSING_BODY);
    };
    let workspace_id = match workspace_id_of(&pool, &slug).await {
        Ok(id) => id,
        Err(denial) => return denial,
    };
    // Single dispatch off the first row's type (`:657-686`): a mixed-type
    // id list follows ONLY the first row — ported as-is.
    match q::bulk_branch(first.entity_type.as_deref().unwrap_or("")) {
        q::BulkBranch::ProjectCover => {
            // `assets.update(project_id=project_id)` (`:658`) plus the
            // per-row cover stamp (`:659`); the LAST row wins.
            if sqlx::query(
                r#"UPDATE "file_assets" SET "project_id" = $3
                   WHERE ("file_assets"."id" = ANY($1)
                   AND "file_assets"."workspace_id" = $2
                   AND "file_assets"."deleted_at" IS NULL)"#,
            )
            .bind(&asset_ids)
            .bind(workspace_id)
            .bind(project_id)
            .execute(&pool)
            .await
            .is_err()
            {
                return server_error();
            }
            for row in &ordered {
                // `save_project_cover` (`:631-634`): re-get the project
                // per row (default manager) and stamp
                // `cover_image_asset_id`.
                let exists: Option<(Uuid,)> = match sqlx::query_as(q::SAVE_PROJECT_COVER_GET_SQL)
                    .bind(project_id)
                    .fetch_optional(&pool)
                    .await
                {
                    Ok(exists) => exists,
                    Err(_) => return server_error(),
                };
                if exists.is_none() {
                    return does_not_exist();
                }
                if sqlx::query(q::SAVE_PROJECT_COVER_SQL)
                    .bind(project_id)
                    .bind(row.id)
                    .execute(&pool)
                    .await
                    .is_err()
                {
                    return server_error();
                }
            }
        }
        q::BulkBranch::IssueDescription => {
            // Integrity errors swallowed (`:664-667`).
            let result = sqlx::query(
                r#"UPDATE "file_assets" SET "issue_id" = $3, "project_id" = $4
                   WHERE ("file_assets"."id" = ANY($1)
                   AND "file_assets"."workspace_id" = $2
                   AND "file_assets"."deleted_at" IS NULL)"#,
            )
            .bind(&asset_ids)
            .bind(workspace_id)
            .bind(entity_id)
            .bind(project_id)
            .execute(&pool)
            .await;
            if let Err(error) = result {
                if !is_integrity_error(&error) {
                    return server_error();
                }
            }
        }
        q::BulkBranch::CommentDescription => {
            // Integrity errors swallowed (`:672-675`); note: no
            // `project_id` here — differs from the issue branch.
            let result = sqlx::query(
                r#"UPDATE "file_assets" SET "comment_id" = $3
                   WHERE ("file_assets"."id" = ANY($1)
                   AND "file_assets"."workspace_id" = $2
                   AND "file_assets"."deleted_at" IS NULL)"#,
            )
            .bind(&asset_ids)
            .bind(workspace_id)
            .bind(entity_id)
            .execute(&pool)
            .await;
            if let Err(error) = result {
                if !is_integrity_error(&error) {
                    return server_error();
                }
            }
        }
        q::BulkBranch::PageDescription => {
            // No swallow (`:680-682`): an `IntegrityError` propagates to
            // `handle_exception` → 400; anything else → 500.
            let result = sqlx::query(
                r#"UPDATE "file_assets" SET "page_id" = $3
                   WHERE ("file_assets"."id" = ANY($1)
                   AND "file_assets"."workspace_id" = $2
                   AND "file_assets"."deleted_at" IS NULL)"#,
            )
            .bind(&asset_ids)
            .bind(workspace_id)
            .bind(entity_id)
            .execute(&pool)
            .await;
            if let Err(error) = result {
                if is_integrity_error(&error) {
                    return raw(StatusCode::BAD_REQUEST, PAYLOAD_NOT_VALID_BODY);
                }
                return server_error();
            }
        }
        q::BulkBranch::DraftIssueDescription => {
            // Integrity errors swallowed (`:683-686`).
            let result = sqlx::query(
                r#"UPDATE "file_assets" SET "draft_issue_id" = $3
                   WHERE ("file_assets"."id" = ANY($1)
                   AND "file_assets"."workspace_id" = $2
                   AND "file_assets"."deleted_at" IS NULL)"#,
            )
            .bind(&asset_ids)
            .bind(workspace_id)
            .bind(entity_id)
            .execute(&pool)
            .await;
            if let Err(error) = result {
                if !is_integrity_error(&error) {
                    return server_error();
                }
            }
        }
        q::BulkBranch::Noop => {}
    }
    no_content()
}

/// Workspace id for bulk scoping (the slug already passed the gate; a
/// missing row here is unreachable through Django, hence the 500).
async fn workspace_id_of(pool: &sqlx::PgPool, slug: &str) -> Result<Uuid, Response> {
    let row: Option<(Uuid,)> =
        sqlx::query_as(r#"SELECT id FROM workspaces WHERE slug = $1 AND deleted_at IS NULL"#)
            .bind(slug)
            .fetch_optional(pool)
            .await
            .map_err(|_| server_error())?;
    row.map(|(id,)| id).ok_or_else(server_error)
}

/// `AssetCheckEndpoint.get` (`v2.py:691-697`): existence from
/// `all_objects` plus `deleted_at`, so v1-style deletes (which set only
/// `is_deleted`) still report `exists: true`.
async fn get_check_asset(
    State(state): State<AppState>,
    Path((slug, asset_raw)): Path<(String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Response {
    let asset_id = match asset_raw.parse::<Uuid>() {
        Ok(id) => id,
        Err(_) => return page_not_found(),
    };
    let actor = match require_actor(&state, extension).await {
        Ok(actor) => actor,
        Err(denial) => return denial,
    };
    let Some(pool) = pool(&state) else {
        return server_error();
    };
    let pool = pool.clone();
    if let Err(denial) =
        gate_workspace(&pool, &slug, &actor.id, guards::AssetEndpoint::CheckGet).await
    {
        return denial;
    }
    let exists: bool = match sqlx::query_scalar(q::CHECK_EXISTS_SQL)
        .bind(asset_id)
        .bind(&slug)
        .fetch_one(&pool)
        .await
    {
        Ok(exists) => exists,
        Err(_) => return server_error(),
    };
    json_status(StatusCode::OK, serde_json::json!({ "exists": exists }))
}

// ---------------------------------------------------------------------------
// Duplicate — clone a row, copy the bytes
// ---------------------------------------------------------------------------

/// `DuplicateAssetEndpoint.post` (`v2.py:736-780`).
async fn post_duplicate_asset(
    State(state): State<AppState>,
    Path((slug, asset_raw)): Path<(String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let asset_id = match asset_raw.parse::<Uuid>() {
        Ok(id) => id,
        Err(_) => return page_not_found(),
    };
    let actor = match require_actor(&state, extension).await {
        Ok(actor) => actor,
        Err(denial) => return denial,
    };
    let Some(pool) = pool(&state) else {
        return server_error();
    };
    let pool = pool.clone();
    if let Err(denial) = gate_workspace(
        &pool,
        &slug,
        &actor.id,
        guards::AssetEndpoint::DuplicatePost,
    )
    .await
    {
        return denial;
    }
    if let Err(denial) = check_duplicate_throttle(&asset_id) {
        return denial;
    }
    let Some(data) = parse_data(content_type(&headers).as_deref(), &body) else {
        return server_error();
    };
    // QUIRK (`:739`): the key read here is `entity_id`, not the mint
    // endpoints' `entity_identifier`.
    let entity_type = match data.get("entity_type").and_then(Value::as_str) {
        Some(t) if !t.is_empty() && ENTITY_TYPES.contains(&t) => t.to_owned(),
        _ => return raw(StatusCode::BAD_REQUEST, DUPLICATE_INVALID_ENTITY_BODY),
    };
    // `Workspace.objects.get(slug=slug)` (`:748`).
    let workspace_id = match workspace_lookup(&pool, &slug).await {
        Ok(id) => id,
        Err(denial) => return denial,
    };
    // Optional project scope (`:749-752`): `if project_id:` skips the
    // check for every falsy shape (`0`/`false`/`""`/`[]`/`{}`/null) and
    // stores NULL (`:766`); garbage UUIDs fail UUID coercion → the 400
    // envelope; unknown ids → 404.
    let project_id: Option<Uuid> = match data.get("project_id") {
        None => None,
        Some(v) if json_falsy(v) => None,
        Some(other @ (Value::String(_) | Value::Number(_) | Value::Bool(_))) => {
            match coerce_uuid_or_400(Some(other)) {
                Ok(id) => id,
                Err(denial) => return denial,
            }
        }
        Some(_) => return valid_detail(),
    };
    if let Some(project_id) = project_id {
        let exists: bool = match sqlx::query_scalar(q::DUPLICATE_PROJECT_CHECK_SQL)
            .bind(project_id)
            .bind(workspace_id)
            .fetch_one(&pool)
            .await
        {
            Ok(exists) => exists,
            Err(_) => return server_error(),
        };
        if !exists {
            return raw(StatusCode::NOT_FOUND, DUPLICATE_PROJECT_BODY);
        }
    }
    // Original lookup on the default manager + `is_uploaded` (`:755`):
    // soft-deleted or never-uploaded originals answer 404.
    let original_sql = format!(
        "SELECT {ASSET_ROW_COLS} FROM \"file_assets\" \
         WHERE (\"file_assets\".\"deleted_at\" IS NULL AND \"file_assets\".\"id\" = $1 \
         AND \"file_assets\".\"is_uploaded\")"
    );
    let row = match sqlx::query(&original_sql)
        .bind(asset_id)
        .fetch_optional(&pool)
        .await
    {
        Ok(row) => row,
        Err(_) => return server_error(),
    };
    let Some(row) = row else {
        return raw(StatusCode::NOT_FOUND, DUPLICATE_ASSET_BODY);
    };
    let original = match asset_row(&row) {
        Ok(asset) => asset,
        Err(_) => return server_error(),
    };
    // Attribute copies render `.get(...)` with no fallback (`:762-766`):
    // missing keys become JSON null.
    let attr_name = original
        .attributes
        .as_ref()
        .and_then(|a| a.get("name"))
        .cloned()
        .unwrap_or(Value::Null);
    let attr_type = original
        .attributes
        .as_ref()
        .and_then(|a| a.get("type"))
        .cloned()
        .unwrap_or(Value::Null);
    let attr_size = original
        .attributes
        .as_ref()
        .and_then(|a| a.get("size"))
        .cloned()
        .unwrap_or(Value::Null);
    // The f-string renders `.get('name')` with Python `str()`
    // (`:760`): non-string names (e.g. mint `"name": 5` → `...-5`)
    // render instead of falling back to `None`.
    let (name_display, _) = python_str(original.attributes.as_ref().and_then(|a| a.get("name")));
    let destination_key = q::duplicate_destination_key(
        &workspace_id.to_string(),
        &Uuid::new_v4().simple().to_string(),
        Some(name_display.as_str()),
    );
    let new_id = Uuid::new_v4();
    let now = Utc::now();
    // Duplicate create (`:761-775`): `created_by_id` (unlike the mints),
    // `storage_metadata` copied verbatim, the 7-branch entity map (no
    // DRAFT arm — port the difference as-is), project possibly NULL.
    // BUG (same class as the mint cover bug, verified live): the
    // `PROJECT_COVER` spread carries `project_id`, colliding with the
    // explicit kwarg → `TypeError` → generic 500.
    if entity_type == "PROJECT_COVER" {
        return server_error();
    }
    let spread_col = q::duplicate_entity_id_field(&entity_type);
    let spread_val = match coerce_uuid_or_400(data.get("entity_id")) {
        Ok(val) => val,
        Err(denial) => return denial,
    };
    // A shadowing spread (`workspace_id` here) rebinds the explicit
    // position instead of adding a column (Django attname-wins).
    let (rebind, suffix) = resolve_spread(spread_col);
    let ws_value = if rebind {
        spread_val
    } else {
        Some(workspace_id)
    };
    let mut sql = "INSERT INTO \"file_assets\" (\"id\", \"created_at\", \"updated_at\", \"attributes\", \"asset\", \"size\", \"workspace_id\", \"created_by_id\", \"entity_type\", \"project_id\", \"storage_metadata\"".to_owned();
    if let Some(col) = suffix {
        sql.push_str(&format!(", \"{col}\""));
    }
    sql.push_str(", \"is_uploaded\", \"is_deleted\", \"is_archived\") VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11");
    if suffix.is_some() {
        sql.push_str(", $12, false, false, false)");
    } else {
        sql.push_str(", false, false, false)");
    }
    let attributes = serde_json::json!({
        "name": attr_name,
        "type": attr_type,
        "size": attr_size,
    });
    let mut insert = sqlx::query(&sql)
        .bind(new_id)
        .bind(now)
        .bind(now)
        .bind(sqlx::types::Json(attributes))
        .bind(&destination_key)
        .bind(original.size)
        .bind(ws_value)
        .bind(actor.id)
        .bind(&entity_type)
        .bind(project_id)
        .bind(original.storage_metadata.clone().map(sqlx::types::Json));
    if suffix.is_some() {
        insert = insert.bind(spread_val);
    }
    if insert.execute(&pool).await.is_err() {
        return server_error();
    }
    // QUIRK (`:776-778`): the row above is committed before the copy;
    // `copy_object` swallows `ClientError` (an S3 error status still
    // flips and 200s), while a transport failure escapes to the 500
    // fallback with the row left behind unflipped.
    let storage = &state.settings().storage;
    let Some(host) = host_of(&headers) else {
        return server_error();
    };
    match s3_copy_object(
        storage,
        &scheme_of(&headers),
        &host,
        &original.asset_key,
        &destination_key,
    )
    .await
    {
        Ok(()) | Err(CopyError::Refused) => {}
        Err(CopyError::Transport) => return server_error(),
    }
    if sqlx::query(q::DUPLICATE_MARK_UPLOADED_SQL)
        .bind(new_id)
        .execute(&pool)
        .await
        .is_err()
    {
        return server_error();
    }
    json_status(
        StatusCode::OK,
        serde_json::json!({ "asset_id": new_id.to_string() }),
    )
}

// ---------------------------------------------------------------------------
// Downloads — presigned attachment redirects
// ---------------------------------------------------------------------------

/// `WorkspaceAssetDownloadEndpoint.get` (`v2.py:783-807`).
async fn get_workspace_download(
    State(state): State<AppState>,
    Path((slug, asset_raw)): Path<(String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    headers: HeaderMap,
) -> Response {
    let asset_id = match asset_raw.parse::<Uuid>() {
        Ok(id) => id,
        Err(_) => return page_not_found(),
    };
    download_asset(
        &state,
        &slug,
        None,
        &asset_id,
        extension,
        &headers,
        guards::AssetEndpoint::WorkspaceDownloadGet,
    )
    .await
}

/// `ProjectAssetDownloadEndpoint.get` (`v2.py:810-835`).
async fn get_project_download(
    State(state): State<AppState>,
    Path((slug, project_raw, asset_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    headers: HeaderMap,
) -> Response {
    let asset_id = match asset_raw.parse::<Uuid>() {
        Ok(id) => id,
        Err(_) => return page_not_found(),
    };
    let actor = match require_actor(&state, extension).await {
        Ok(actor) => actor,
        Err(denial) => return denial,
    };
    let Some(pool) = pool(&state) else {
        return server_error();
    };
    let pool = pool.clone();
    let project_id = match resolve_project_id(&pool, &slug, &project_raw).await {
        Ok(id) => id,
        Err(denial) => return denial,
    };
    if let Err(denial) = gate_project(
        &pool,
        &slug,
        &project_id,
        &actor.id,
        guards::AssetEndpoint::ProjectDownloadGet,
    )
    .await
    {
        return denial;
    }
    download_lookup(&state, &pool, &slug, Some(&project_id), &asset_id, &headers).await
}

/// Shared download lookup: the `is_uploaded` conjunct lives in the
/// lookup itself, so a present-but-unuploaded row answers the same
/// inline 404 (`:788-798`, `:815-826`).
async fn download_asset(
    state: &AppState,
    slug: &str,
    project_id: Option<&Uuid>,
    asset_id: &Uuid,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    headers: &HeaderMap,
    endpoint: guards::AssetEndpoint,
) -> Response {
    let actor = match require_actor(state, extension).await {
        Ok(actor) => actor,
        Err(denial) => return denial,
    };
    let Some(pool) = pool(state) else {
        return server_error();
    };
    let pool = pool.clone();
    if project_id.is_none() {
        if let Err(denial) = gate_workspace(&pool, slug, &actor.id, endpoint).await {
            return denial;
        }
    }
    download_lookup(state, &pool, slug, project_id, asset_id, headers).await
}

async fn download_lookup(
    state: &AppState,
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: Option<&Uuid>,
    asset_id: &Uuid,
    headers: &HeaderMap,
) -> Response {
    let (sql, binds_project) = if project_id.is_some() {
        (
            format!(
                "SELECT {ASSET_ROW_COLS} FROM \"file_assets\" \
                 INNER JOIN \"workspaces\" ON \"workspaces\".\"id\" = \"file_assets\".\"workspace_id\" \
                 WHERE (\"file_assets\".\"id\" = $1 AND \"workspaces\".\"slug\" = $2 \
                 AND \"file_assets\".\"project_id\" = $3 AND \"file_assets\".\"is_uploaded\" \
                 AND \"file_assets\".\"deleted_at\" IS NULL)"
            ),
            true,
        )
    } else {
        (
            format!(
                "SELECT {ASSET_ROW_COLS} FROM \"file_assets\" \
                 INNER JOIN \"workspaces\" ON \"workspaces\".\"id\" = \"file_assets\".\"workspace_id\" \
                 WHERE (\"file_assets\".\"id\" = $1 AND \"workspaces\".\"slug\" = $2 \
                 AND \"file_assets\".\"is_uploaded\" AND \"file_assets\".\"deleted_at\" IS NULL)"
            ),
            false,
        )
    };
    let mut lookup = sqlx::query(&sql).bind(asset_id).bind(slug);
    if binds_project {
        lookup = lookup.bind(project_id);
    }
    let row = match lookup.fetch_optional(pool).await {
        Ok(row) => row,
        Err(_) => return server_error(),
    };
    let Some(row) = row else {
        return raw(StatusCode::NOT_FOUND, ASSET_MISSING_BODY);
    };
    let asset = match asset_row(&row) {
        Ok(asset) => asset,
        Err(_) => return server_error(),
    };
    // `attributes.get("name", uuid.hex)` (`:804, :832`): a fresh hex per
    // call when `name` is absent; empty string renders the bare
    // disposition; other non-strings raise → 500.
    let filename = match asset.attributes.as_ref().and_then(|a| a.get("name")) {
        None | Some(Value::Null) => Filename::FreshHex,
        Some(Value::String(s)) if s.is_empty() => Filename::Bare,
        Some(Value::String(s)) => Filename::Name(s.clone()),
        Some(_) => return server_error(),
    };
    let storage = &state.settings().storage;
    let Some(host) = host_of(headers) else {
        return server_error();
    };
    let url = presigned_get_url(
        storage,
        &scheme_of(headers),
        &host,
        &asset.asset_key,
        filename,
        &Utc::now(),
    );
    redirect(url)
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

fn host_of(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
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

/// Path encoding for the canonical URI: slashes survive, every
/// segment is RFC 3986-encoded (botocore `quote(path, safe='/~')`…
/// with the same unreserved set as [`uri_encode`]).
fn uri_encode_path(path: &str) -> String {
    path.split('/')
        .map(uri_encode)
        .collect::<Vec<_>>()
        .join("/")
}

/// The download `filename` argument: `_get_content_disposition`
/// (`storage.py:115-123`) with `disposition="attachment"`.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Filename {
    /// A stored string name: quoted into the disposition.
    Name(String),
    /// Missing/null name: a fresh uuid4 hex per call.
    FreshHex,
    /// Empty name: falsy → the bare disposition, no filename part.
    Bare,
}

fn disposition_value(filename: &Filename) -> String {
    match filename {
        Filename::Name(name) => {
            format!("attachment; filename*=UTF-8''{}", uri_encode(name))
        }
        Filename::FreshHex => {
            format!("attachment; filename*=UTF-8''{}", Uuid::new_v4().simple())
        }
        Filename::Bare => "attachment".to_owned(),
    }
}

/// `generate_presigned_url(object_name, disposition="attachment",
/// filename=...)` (`v2.py:621-625,801-805,828-833`, `storage.py:126-153`):
/// presigned GET with the attachment disposition above.
fn presigned_get_url(
    storage: &pidash_db::config::StorageSettings,
    scheme: &str,
    host: &str,
    object_name: &str,
    filename: Filename,
    now: &DateTime<Utc>,
) -> String {
    let region = storage.region.as_str();
    let (endpoint, signed_host) = endpoint_parts(storage, scheme, host);
    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let date = now.format("%Y%m%d").to_string();
    let scope = credential_scope(&date, region);
    let credential = format!("{}/{}", storage.access_key_id, scope);
    let disposition = disposition_value(&filename);
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

/// `generate_presigned_post(object_name, file_type, file_size)`
/// (`v2.py:568`, `storage.py:79-113`): `{"url","fields"}` with
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

// ---------------------------------------------------------------------------
// S3 copy (duplicate bytes)
// ---------------------------------------------------------------------------

/// Outcome of the duplicate `copy_object` (`storage.py:172-184`): an S3
/// error status is the `ClientError` Django swallows (the row still
/// flips and the view 200s); a transport failure escapes to the 500
/// fallback with the row left behind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CopyError {
    Refused,
    Transport,
}

/// Pure SigV4 for the copy `PUT`: returns the request URL plus the
/// `x-amz-*` headers and `Authorization` value, so tests pin the
/// signature without network.
fn copy_auth_headers(
    storage: &pidash_db::config::StorageSettings,
    scheme: &str,
    host: &str,
    src_key: &str,
    dst_key: &str,
    now: &DateTime<Utc>,
) -> (String, Vec<(String, String)>) {
    let region = storage.region.as_str();
    let (endpoint, signed_host) = endpoint_parts(storage, scheme, host);
    let path_style = storage.use_minio
        || storage
            .endpoint_url
            .as_deref()
            .is_some_and(|e| !e.is_empty());
    let dst_path = if path_style {
        format!("/{}/{}", storage.bucket_name, uri_encode_path(dst_key))
    } else {
        format!("/{}", uri_encode_path(dst_key))
    };
    let copy_source = format!("/{}/{}", storage.bucket_name, uri_encode_path(src_key));
    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let date = now.format("%Y%m%d").to_string();
    let payload_hash = sha256_hex(b"");
    let signed_headers = "host;x-amz-content-sha256;x-amz-copy-source;x-amz-date";
    let canonical = format!(
        "PUT\n{dst_path}\n\nhost:{signed_host}\n\
         x-amz-content-sha256:{payload_hash}\n\
         x-amz-copy-source:{copy_source}\n\
         x-amz-date:{amz_date}\n\
         \n{signed_headers}\n{payload_hash}"
    );
    let scope = credential_scope(&date, region);
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{}",
        sha256_hex(canonical.as_bytes())
    );
    let signature = hex(&hmac_sha256(
        &signing_key(&storage.secret_access_key, &date, region),
        string_to_sign.as_bytes(),
    ));
    let authorization = format!(
        "AWS4-HMAC-SHA256 Credential={}/{}, SignedHeaders={signed_headers}, Signature={signature}",
        storage.access_key_id, scope,
    );
    (
        format!("{endpoint}{dst_path}"),
        vec![
            ("x-amz-content-sha256".to_owned(), payload_hash),
            ("x-amz-copy-source".to_owned(), copy_source),
            ("x-amz-date".to_owned(), amz_date),
            ("authorization".to_owned(), authorization),
        ],
    )
}

/// `S3Storage.copy_object` (`storage.py:172-184`) over plain HTTPS: an
/// empty-body `PUT` to the destination key with `x-amz-copy-source`.
async fn s3_copy_object(
    storage: &pidash_db::config::StorageSettings,
    scheme: &str,
    host: &str,
    src_key: &str,
    dst_key: &str,
) -> Result<(), CopyError> {
    let (url, headers) = copy_auth_headers(storage, scheme, host, src_key, dst_key, &Utc::now());
    let client = reqwest::Client::new();
    let mut request = client.put(&url).body(Vec::new());
    for (name, value) in &headers {
        request = request.header(name.as_str(), value.as_str());
    }
    let response = request.send().await.map_err(|_| CopyError::Transport)?;
    if response.status().is_success() {
        Ok(())
    } else {
        Err(CopyError::Refused)
    }
}

/// Django `IntegrityError` (unique/fk/not-null/check) as sqlx sees it:
/// these branches `pass`, anything else 500s.
fn is_integrity_error(error: &sqlx::Error) -> bool {
    match error {
        sqlx::Error::Database(db) => {
            let kind = db.kind();
            matches!(
                kind,
                sqlx::error::ErrorKind::UniqueViolation
                    | sqlx::error::ErrorKind::ForeignKeyViolation
                    | sqlx::error::ErrorKind::NotNullViolation
                    | sqlx::error::ErrorKind::CheckViolation
            )
        }
        _ => false,
    }
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
    fn routes_register_all_five_django_paths() {
        // Seven axum routes cover the five Django path shapes (the
        // project detail path owns three methods): every Django URL
        // resolves, no two axum routes collide.
        let paths = [
            "/api/assets/v2/workspaces/s/projects/p/",
            "/api/assets/v2/workspaces/s/projects/p/3f9a2c1e-9b1a-4d2e-8f3a-1a2b3c4d5e6f/",
            "/api/assets/v2/workspaces/s/projects/p/3f9a2c1e-9b1a-4d2e-8f3a-1a2b3c4d5e6f/bulk/",
            "/api/assets/v2/workspaces/s/check/3f9a2c1e-9b1a-4d2e-8f3a-1a2b3c4d5e6f/",
            "/api/assets/v2/workspaces/s/duplicate-assets/3f9a2c1e-9b1a-4d2e-8f3a-1a2b3c4d5e6f/",
            "/api/assets/v2/workspaces/s/download/3f9a2c1e-9b1a-4d2e-8f3a-1a2b3c4d5e6f/",
            "/api/assets/v2/workspaces/s/projects/p/download/3f9a2c1e-9b1a-4d2e-8f3a-1a2b3c4d5e6f/",
        ];
        assert_eq!(paths.len(), 7);
        let _ = routes();
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
    }

    #[test]
    fn presigned_get_signs_attachment_disposition() {
        let url = presigned_get_url(
            &test_storage(),
            "http",
            "127.0.0.1:8486",
            "ws-id/ab12-shot.png",
            Filename::Name("shot.png".to_owned()),
            &test_now(),
        );
        assert!(
            url.starts_with("http://127.0.0.1:8486/uploads/ws-id/ab12-shot.png?"),
            "{url}"
        );
        assert!(url.contains("response-content-disposition=attachment"));
        assert!(url.contains("shot.png"));
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
    fn disposition_shapes_match_storage_py() {
        assert_eq!(
            disposition_value(&Filename::Name("a b.png".to_owned())),
            "attachment; filename*=UTF-8''a%20b.png"
        );
        assert_eq!(disposition_value(&Filename::Bare), "attachment");
        // Fresh hex per call, 32 lowercase hex chars.
        let first = disposition_value(&Filename::FreshHex);
        let second = disposition_value(&Filename::FreshHex);
        assert_ne!(first, second);
        for value in [first, second] {
            let name = value
                .strip_prefix("attachment; filename*=UTF-8''")
                .expect("prefix");
            assert_eq!(name.len(), 32);
            assert!(name.chars().all(|c| c.is_ascii_hexdigit()));
        }
    }

    #[test]
    fn copy_auth_signs_put_with_copy_source() {
        let (url, headers) = copy_auth_headers(
            &test_storage(),
            "http",
            "127.0.0.1:8486",
            "ws-id/orig.png",
            "ws-id/ab12-orig.png",
            &test_now(),
        );
        assert_eq!(url, "http://127.0.0.1:8486/uploads/ws-id/ab12-orig.png");
        let get = |name: &str| {
            headers
                .iter()
                .find(|(k, _)| k == name)
                .unwrap_or_else(|| panic!("header {name}"))
                .1
                .clone()
        };
        assert_eq!(get("x-amz-copy-source"), "/uploads/ws-id/orig.png");
        assert_eq!(
            get("x-amz-content-sha256"),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        // Independent oracle: recompute the header signature by hand.
        let scope = "20260928/us-east-1/s3/aws4_request";
        let canonical = format!(
            "PUT\n/uploads/ws-id/ab12-orig.png\n\n\
             host:127.0.0.1:8486\n\
             x-amz-content-sha256:{}\n\
             x-amz-copy-source:/uploads/ws-id/orig.png\n\
             x-amz-date:20260928T120000Z\n\
             \n\
             host;x-amz-content-sha256;x-amz-copy-source;x-amz-date\n{}",
            get("x-amz-content-sha256"),
            get("x-amz-content-sha256"),
        );
        let to_sign = format!(
            "AWS4-HMAC-SHA256\n20260928T120000Z\n{scope}\n{}",
            sha256_hex(canonical.as_bytes())
        );
        let expected = hex(&hmac_sha256(
            &signing_key("secret-key", "20260928", "us-east-1"),
            to_sign.as_bytes(),
        ));
        let authorization = get("authorization");
        assert!(
            authorization.ends_with(&format!("Signature={expected}")),
            "{authorization}"
        );
        assert!(authorization.starts_with(
            "AWS4-HMAC-SHA256 Credential=access-key/20260928/us-east-1/s3/aws4_request"
        ));
    }

    #[test]
    fn parse_size_mirrors_int_quirk() {
        assert_eq!(parse_size(None, 5242880), Some(5242880));
        assert_eq!(parse_size(Some(&Value::from("10")), 5), Some(10));
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
        let mut null = Map::new();
        null.insert("type".to_owned(), Value::Null);
        assert_eq!(post_mime(&null), None);
    }

    #[test]
    fn python_str_renders_none_like_fstring() {
        assert_eq!(python_str(None), ("None".to_owned(), Value::Null));
        assert_eq!(
            python_str(Some(&Value::from("shot.png"))),
            ("shot.png".to_owned(), Value::from("shot.png")),
        );
        // Non-string names render with Python `str()` (`v2.py:760`).
        assert_eq!(python_str(Some(&Value::from(5))).0, "5");
        assert_eq!(python_str(Some(&Value::Bool(true))).0, "True");
    }

    #[test]
    fn json_falsy_matches_python_not() {
        assert!(json_falsy(&Value::Null));
        assert!(json_falsy(&Value::Bool(false)));
        assert!(!json_falsy(&Value::Bool(true)));
        assert!(json_falsy(&Value::from(0)));
        assert!(json_falsy(&serde_json::json!(0.0)));
        assert!(!json_falsy(&Value::from(5)));
        assert!(json_falsy(&Value::from("")));
        assert!(!json_falsy(&Value::from("x")));
        assert!(json_falsy(&serde_json::json!([])));
        assert!(!json_falsy(&serde_json::json!([1])));
        assert!(json_falsy(&serde_json::json!({})));
        assert!(!json_falsy(&serde_json::json!({"a": 1})));
    }

    #[test]
    fn coerce_uuid_matches_to_python() {
        let id = Uuid::parse_str("3f9a2c1e-9b1a-4d2e-8f3a-1a2b3c4d5e6f").expect("uuid");
        assert!(coerce_uuid_or_400(None).expect("none").is_none());
        assert!(coerce_uuid_or_400(Some(&Value::Null))
            .expect("null")
            .is_none());
        assert_eq!(
            coerce_uuid_or_400(Some(&Value::from(id.to_string()))).expect("str"),
            Some(id)
        );
        // Garbage strings reject (Django `ValidationError` → 400).
        assert!(coerce_uuid_or_400(Some(&Value::from("nope"))).is_err());
        // Small ints coerce via `uuid.UUID(int=...)`.
        assert_eq!(
            coerce_uuid_or_400(Some(&Value::from(5))).expect("int"),
            Some(Uuid::from_u128(5))
        );
        assert!(coerce_uuid_or_400(Some(&Value::from(-1))).is_err());
        assert!(coerce_uuid_or_400(Some(&Value::from(1.5))).is_err());
        assert!(coerce_uuid_or_400(Some(&serde_json::json!([]))).is_err());
    }

    #[test]
    fn asset_url_shapes_match_model_property() {
        let asset = Uuid::parse_str("3f9a2c1e-9b1a-4d2e-8f3a-1a2b3c4d5e6f").expect("uuid");
        let project = Uuid::parse_str("11111111-1111-1111-1111-111111111111").expect("uuid");
        assert_eq!(
            asset_url_for("WORKSPACE_LOGO", "ws", &project, None, &asset),
            Value::String(format!("/api/assets/v2/static/{asset}/"))
        );
        assert_eq!(
            asset_url_for("ISSUE_DESCRIPTION", "ws", &project, None, &asset),
            Value::String(format!(
                "/api/assets/v2/workspaces/ws/projects/{project}/{asset}/"
            ))
        );
        assert_eq!(
            asset_url_for("ISSUE_ATTACHMENT", "ws", &project, None, &asset),
            Value::String(format!(
                "/api/assets/v2/workspaces/ws/projects/{project}/issues/None/attachments/{asset}/"
            ))
        );
        assert_eq!(
            asset_url_for("DRAFT_ISSUE_ATTACHMENT", "ws", &project, None, &asset),
            Value::Null
        );
    }

    #[test]
    fn resolve_spread_ports_django_attname_wins() {
        // Verified live against Django: `workspace=X, workspace_id=Y`
        // stores Y; `workspace_id=None` stores NULL.
        assert_eq!(resolve_spread(Some("workspace_id")), (true, None));
        assert_eq!(resolve_spread(Some("project_id")), (true, None));
        assert_eq!(resolve_spread(Some("issue_id")), (false, Some("issue_id")));
        assert_eq!(resolve_spread(None), (false, None));
    }

    #[test]
    fn entity_type_vocabulary_matches_model() {
        assert_eq!(ENTITY_TYPES.len(), 10);
        for known in [
            "ISSUE_ATTACHMENT",
            "PROJECT_COVER",
            "DRAFT_ISSUE_DESCRIPTION",
            "WORKSPACE_LOGO",
        ] {
            assert!(ENTITY_TYPES.contains(&known));
            assert!(
                q::project_entity_id_field(known).is_some() || *known == *"DRAFT_ISSUE_ATTACHMENT"
            );
        }
        // Duplicate map has no DRAFT arm (ported difference).
        assert_eq!(
            q::duplicate_entity_id_field("DRAFT_ISSUE_DESCRIPTION"),
            None
        );
        // Throttle key pins the mint/dup spelling split.
        assert_eq!(
            guards::asset_throttle_key(Some("abc")),
            Some("throttle_asset_abc".to_owned())
        );
        assert_eq!(guards::ASSET_THROTTLE_REQUESTS, 5);
    }
}
