//! Activity + attachment handlers (D-18 handlers C, PIDASHCONV-675).
//!
//! Ports `apps/api/pi_dash/api/views/issue.py` (4 units):
//!
//! * `IssueActivityListAPIEndpoint.get` (`:2150-2173`) — paginated activity
//!   list, `order_by`/`fields=`/`expand=` from the query string.
//! * `IssueActivityDetailAPIEndpoint.get` (`:2205-2232`) — one activity or
//!   the custom `{"message": "Activity not found.", "code": "NOT_FOUND"}` 404.
//! * `IssueAttachmentListCreateAPIEndpoint` (`:2312-2447`) — presigned-POST
//!   upload mint (`post`) and the bare-list `get`.
//! * `IssueAttachmentDetailAPIEndpoint` (`:2468-2651`) — soft-delete
//!   (`delete`, 204), presigned-redirect download (`get`, 302) and
//!   upload confirm (`patch`, 204).
//!
//! Registered by [`super::routes`] at the eight
//! `apps/api/pi_dash/api/urls/work_item.py:79-98,173-192` paths (four
//! `work-items/` routes plus their deprecated `issues/` twins, which share
//! the view classes and therefore the handlers).
//!
//! Layering (all foundation use is read-only): shapes in
//! `pidash_services::v1_work_items::shape_social`, representative SQL in
//! `queries_sub`, POST validation + S3 offline inputs in `tasks`, gates in
//! [`super::perms`] over the F-06 kernel (`pidash_auth::permissions`),
//! request bodies through `crate::v1_cycles_modules::{body, json_cpython}`,
//! pagination through `crate::paginator`, task fan-out through
//! `pidash_jobs::queue`. This module owns the HTTP shell: API-key auth, the
//! slug→UUID rewrite, permission wiring, the write statements, the SigV4
//! presigning, the read-shape rendering and the paginated envelope.
//!
//! Request order (preserved, not redesigned): UUID-segment shape (proxy when
//! Django's `<uuid:>` converter would not match — before auth, as URL
//! resolving precedes it), API-key authentication (anonymous 401s before any
//! pool or database access), the slug→UUID rewrite
//! (`api/views/base.py:51-98`, skipped for anonymous callers),
//! `check_permissions`, then the handler body. Inside the attachment `post`
//! the body parses only after the issue load and the permission check,
//! because the view touches `request.data` there (`:2332`); the `patch`
//! never reads the body at all, so any bytes (even malformed JSON) still
//! confirm.
//!
//! Ported bugs (also listed in the PR):
//!
//! * BUG-1 (`views/issue.py:2438-2444`): the attachment list carries NO
//!   member or archived guard, unlike every sibling chain — any
//!   authenticated caller lists any issue's attachments.
//! * BUG-2 (`views/issue.py:2339-2341` vs the `:2286-2298` OpenAPI block):
//!   a nameless/sizeless POST answers `{"error": "Invalid request."}`,
//!   not the documented `"Name and size are required fields."`.
//!
//! Deliberate edges (all unpinned — no fixture or contract case sends them):
//!
//! * Extreme-magnitude floats (`1e16`, `1e-05`) render with serde's
//!   exponent spelling (`1e16`, `1.5e-5`) where CPython spells `1e+16`,
//!   `1.5e-05` — same accepted edge as every merged shape module, which
//!   all render floats through serde.
//! * `order_by=?` (random) is honored; forward related-span orders
//!   (`project__name`, `actor__email`, …) JOIN and 200 like Django
//!   (PIDASHCONV-748), and bare FK names (`actor`) order by the
//!   related `Meta.ordering` while bare attnames (`actor_id`) order by
//!   the local column (PIDASHCONV-759). Spans through reverse
//!   relations, M2M fields (Django 200s with row fanout), datetime
//!   transforms (`created_at__date`), and models outside the span graph
//!   (`DraftIssue`, `Page`, auth tables) still 500 here — narrowed
//!   residual edge, all verified by live-Django probe.
//!
//! Fixture: `F18-11` (`rust-api/fixtures/v1_work_items/handlers/` —
//! `activity_list`, `activity_detail`, `attachment_list`,
//! `attachment_post_presign_full`, `attachment_confirm`,
//! `attachment_get_redirect`, `attachment_delete`, plus the four
//! deprecated twins).
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

use std::collections::HashMap;

use axum::extract::{OriginalUri, Path, Query, Request, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use hmac::{Hmac, Mac};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use pidash_auth::permissions::project;
use pidash_auth::scope::TenantScope;
use pidash_services::v1_work_items::queries_sub;
use pidash_services::v1_work_items::shape_issue::BASE_EXPANSION_NAMES;
use pidash_services::v1_work_items::shape_social::{
    render_activity, render_attachment, ActivityRepresentationInput, ActivityRow,
    AttachmentRepresentationInput, AttachmentRow,
};
use pidash_services::v1_work_items::tasks as work_tasks;

use crate::state::AppState;

use super::perms::{
    gate_for, user_has_issue_permission, V1WorkItemsRoute, ATTACHMENT_DELETE_DENIAL_BODY,
    ATTACHMENT_DOWNLOAD_DENIAL_BODY, ATTACHMENT_UPLOAD_DENIAL_BODY,
};

// ---------------------------------------------------------------------------
// Responses
// ---------------------------------------------------------------------------

/// `handle_exception`'s `ObjectDoesNotExist` branch
/// (`api/views/base.py:156-160`): every `.get()` miss on these endpoints.
pub const RESOURCE_NOT_FOUND_BODY: &str = r#"{"error":"The requested resource does not exist."}"#;
/// `handle_exception`'s generic branch (`api/views/base.py:166-170`).
pub const SERVER_ERROR_BODY: &str = r#"{"error":"Something went wrong please try again later"}"#;
/// Attachment POST nameless/sizeless 400 (`views/issue.py:2339-2343`).
pub const INVALID_REQUEST_BODY: &str = r#"{"error":"Invalid request.","status":false}"#;
/// Attachment POST bad-type 400 (`views/issue.py:2347-2351`).
pub const INVALID_FILE_TYPE_BODY: &str = r#"{"error":"Invalid file type.","status":false}"#;
/// Attachment download before confirm 400 (`views/issue.py:2565-2569`).
pub const ASSET_NOT_UPLOADED_BODY: &str =
    r#"{"error":"The asset is not uploaded.","status":false}"#;
/// External-duplicate 409 message (`views/issue.py:2381`).
pub const EXTERNAL_DUP_MESSAGE: &str =
    "Issue with the same external id and external source already exists";

/// Handler failure with its exact status + body.
#[derive(Debug)]
pub enum Denial {
    /// 401, DRF `NotAuthenticated` (no `X-Api-Key` header).
    Unauthorized,
    /// 403, invalid/expired/inactive API or machine token.
    InvalidToken,
    /// 403, the DRF-default `PermissionDenied` body (no D-18 guard class
    /// sets `message`).
    Forbidden,
    /// 403, attachment upload denial (`Response({"error": ...})`,
    /// `:2327-2330` and `:2621-2624`).
    ForbiddenUpload,
    /// 403, attachment download denial (`:2556-2559`).
    ForbiddenDownload,
    /// 403, attachment delete denial (`:2483-2486`).
    ForbiddenDelete,
    /// 404, `{"detail":"Project not found"}` (identifier rewrite miss —
    /// `Project.resolve` raises `Http404`, `db/models/project.py:213-217`).
    ProjectNotFound,
    /// 400, `{"detail": ...}` (DRF `ParseError`: malformed JSON, bad
    /// `per_page`/`cursor`).
    BadDetail(String),
    /// 400, `{"error": ...}` (view-inline: unknown timezones).
    BadError(String),
    /// 415, `{"detail": ...}` (DRF `UnsupportedMediaType`).
    UnsupportedMediaType(String),
    /// 404, view-inline `{"error": ...}` with the full body.
    NotFound(String),
    /// 409, view-inline body (external duplicate).
    Conflict(String),
    /// 500, generic branch.
    ServerError,
}

impl Denial {
    fn status_and_body(&self) -> (StatusCode, String) {
        match self {
            Denial::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                super::perms::UNAUTHENTICATED_BODY.to_owned(),
            ),
            Denial::InvalidToken => (
                StatusCode::FORBIDDEN,
                r#"{"detail":"Given API token is not valid"}"#.to_owned(),
            ),
            Denial::Forbidden => (
                StatusCode::FORBIDDEN,
                super::perms::CLASS_DENIAL_BODY.to_owned(),
            ),
            Denial::ForbiddenUpload => (
                StatusCode::FORBIDDEN,
                ATTACHMENT_UPLOAD_DENIAL_BODY.to_owned(),
            ),
            Denial::ForbiddenDownload => (
                StatusCode::FORBIDDEN,
                ATTACHMENT_DOWNLOAD_DENIAL_BODY.to_owned(),
            ),
            Denial::ForbiddenDelete => (
                StatusCode::FORBIDDEN,
                ATTACHMENT_DELETE_DENIAL_BODY.to_owned(),
            ),
            Denial::ProjectNotFound => (
                StatusCode::NOT_FOUND,
                r#"{"detail":"Project not found"}"#.to_owned(),
            ),
            Denial::BadDetail(message) => (
                StatusCode::BAD_REQUEST,
                format!("{{\"detail\":{}}}", json_string(message)),
            ),
            Denial::BadError(message) => (
                StatusCode::BAD_REQUEST,
                format!("{{\"error\":{}}}", json_string(message)),
            ),
            Denial::UnsupportedMediaType(message) => (
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                format!("{{\"detail\":{}}}", json_string(message)),
            ),
            Denial::NotFound(body) => (StatusCode::NOT_FOUND, body.clone()),
            Denial::Conflict(body) => (StatusCode::CONFLICT, body.clone()),
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
        json_response(status, body)
    }
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).expect("json string")
}

/// Render a JSON response with exact bytes and status. DRF's `JSONRenderer`
/// post-pass escapes U+2028/U+2029 (`rest_framework/renderers.py`); the
/// `app_project` `escape_u2028` precedent, applied to every JSON body here.
fn json_response(status: StatusCode, body: String) -> Response {
    let body = body
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029");
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body))
        .expect("handler json response")
}

/// Map a database/driver failure to the generic 500 while logging the site
/// and error for operators (no secrets: messages never include tokens).
fn db_error<E: std::fmt::Display>(error: E, site: &str) -> Denial {
    tracing::warn!(%error, site, "v1_work_items activity database failure");
    Denial::ServerError
}

fn pool_of(state: &AppState) -> Result<PgPool, Denial> {
    state
        .pools()
        .map(|pools| pools.primary().clone())
        .ok_or(Denial::ServerError)
}

/// Best-effort post-write task fan-out (the `.delay()` calls): without a
/// queue table the response still stands (the `v1_projects`
/// `enqueue_best_effort` precedent).
async fn enqueue_best_effort(
    pool: &PgPool,
    task: &str,
    args: Vec<Value>,
    kwargs: Map<String, Value>,
) {
    let message = pidash_jobs::celery::CeleryTaskMessage::new(task, args, kwargs);
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
// Cutover wiring
// ---------------------------------------------------------------------------

/// Route registration is the cutover granularity (the pilot `owned()`
/// pattern shared with `v1_projects`): the owned methods serve from Rust,
/// every other method on the path proxies to Django so its 405-after-auth
/// and metadata responses are preserved byte for byte.
fn owned(
    router: axum::routing::MethodRouter<AppState>,
    methods: &[&str],
) -> axum::routing::MethodRouter<AppState> {
    let mut router = router;
    for method in ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] {
        if !methods.contains(&method) {
            router = match method {
                "GET" => router.get(crate::edge::proxy),
                "POST" => router.post(crate::edge::proxy),
                "PUT" => router.put(crate::edge::proxy),
                "PATCH" => router.patch(crate::edge::proxy),
                "DELETE" => router.delete(crate::edge::proxy),
                "HEAD" => router.head(crate::edge::proxy),
                _ => router.options(crate::edge::proxy),
            };
        }
    }
    // DRF runs `initial()` (auth → permissions) before its method check, so
    // exotic methods (TRACE et al.) answer 401/403/405 JSON. The fallback
    // proxies them with the original request (the `handlers_social`
    // `owned()` position).
    router.fallback(crate::edge::proxy)
}

/// The activity paths own GET only
/// (`urls/work_item.py:79-87,173-182`, each
/// `as_view(http_method_names=["get"])`).
pub fn owned_activity(
    router: axum::routing::MethodRouter<AppState>,
) -> axum::routing::MethodRouter<AppState> {
    owned(router, &["GET"])
}

/// The attachment list paths own GET + POST (`urls/work_item.py:89-93`,
/// `:183-187`, `as_view(http_method_names=["get", "post"])`).
pub fn owned_attachment_list(
    router: axum::routing::MethodRouter<AppState>,
) -> axum::routing::MethodRouter<AppState> {
    owned(router, &["GET", "POST"])
}

/// The attachment detail paths own GET + PATCH + DELETE
/// (`urls/work_item.py:94-98`, `:188-192`,
/// `as_view(http_method_names=["get", "patch", "delete"])`).
pub fn owned_attachment_detail(
    router: axum::routing::MethodRouter<AppState>,
) -> axum::routing::MethodRouter<AppState> {
    owned(router, &["GET", "PATCH", "DELETE"])
}

// ---------------------------------------------------------------------------
// Query params
// ---------------------------------------------------------------------------

/// One query value, repeated or not (the `v1_projects` shape: axum's
/// `Query` backend does not coerce a lone `?key=value` into a sequence, so
/// callers read last like Django's `QueryDict`).
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(untagged)]
pub enum OneOrMany {
    One(String),
    Many(Vec<String>),
}

/// The multi-value query map every handler extracts.
pub type QueryMap = HashMap<String, OneOrMany>;

/// Django `QueryDict.get`: the last value, or `None`.
pub fn query_last(query: &QueryMap, key: &str) -> Option<String> {
    query.get(key).map(|value| match value {
        OneOrMany::One(one) => one.clone(),
        OneOrMany::Many(many) => many.last().cloned().unwrap_or_default(),
    })
}

/// `self.fields` / `self.expand` (`api/views/base.py:212-220`): comma
/// split, empties dropped, `None` when empty.
pub fn fields_param(query: &QueryMap, key: &str) -> Option<Vec<String>> {
    let raw = query_last(query, key).unwrap_or_default();
    let fields: Vec<String> = raw
        .split(',')
        .filter(|f| !f.is_empty())
        .map(str::to_owned)
        .collect();
    if fields.is_empty() {
        None
    } else {
        Some(fields)
    }
}

// ---------------------------------------------------------------------------
// Authentication
// ---------------------------------------------------------------------------

/// Decoded `api_tokens` row for [`resolve_api_token`].
type ApiTokenLookup = (
    String,
    bool,
    Option<chrono::DateTime<chrono::Utc>>,
    uuid::Uuid,
);

/// Decoded `machine_token` row for [`resolve_machine_token`].
type MachineTokenLookup = (
    String,
    uuid::Uuid,
    Option<uuid::Uuid>,
    uuid::Uuid,
    Option<chrono::DateTime<chrono::Utc>>,
    Option<bool>,
);

/// The authenticated actor: user id plus the stored time zone name, loaded
/// but NOT parsed — parsing happens after the gate in
/// [`activate_timezone`] (the PIDASHCONV-737 fixed preamble).
pub struct Actor {
    pub id: uuid::Uuid,
    pub timezone: Option<String>,
}

/// `APIKeyAuthentication` (`api/middleware/api_authentication.py:19-88`):
/// the `X-Api-Key` header carries either an `api_tokens` token or an
/// `mt_`-prefixed machine token. No header means anonymous (`authenticate`
/// returns `None`); the permission layer then 401s before any view code.
pub async fn actor(pool: &PgPool, headers: &HeaderMap, secret_key: &[u8]) -> Result<Actor, Denial> {
    let raw = headers
        .get(pidash_auth::token::API_KEY_HEADER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if raw.is_empty() {
        return Err(Denial::Unauthorized);
    }
    let user_id = match pidash_auth::token::classify_token(raw) {
        None => return Err(Denial::Unauthorized),
        Some(pidash_auth::token::TokenKind::Api) => resolve_api_token(pool, raw).await?,
        Some(pidash_auth::token::TokenKind::Machine) => {
            resolve_machine_token(pool, raw, secret_key).await?
        }
    };
    let timezone = load_timezone_name(pool, &user_id).await?;
    // `api_tokens.last_used` is stamped on every validated call
    // (`api_authentication.py:41-44`); machine tokens stamp `last_used_at`.
    // Best-effort: a failed stamp must not fail the request.
    if !raw.starts_with(pidash_auth::token::MACHINE_TOKEN_PREFIX) {
        let _ = sqlx::query(r#"UPDATE "api_tokens" SET "last_used" = now() WHERE "token" = $1"#)
            .bind(raw)
            .execute(pool)
            .await;
    }
    Ok(Actor {
        id: user_id,
        timezone,
    })
}

/// The `api_tokens` columns the validator reads (`db/models/api.py:35-57`).
/// The `deleted_at IS NULL` conjunct is the `SoftDeletionManager` scope
/// (`db/mixins.py:56-66`): `APIToken.objects` never sees soft-deleted
/// rows, so a soft-deleted token 403s instead of authenticating.
async fn resolve_api_token(pool: &PgPool, presented: &str) -> Result<uuid::Uuid, Denial> {
    let row: Option<ApiTokenLookup> = sqlx::query_as(
        r#"SELECT "token", "is_active", "expired_at", "user_id" FROM "api_tokens" WHERE "token" = $1 AND "deleted_at" IS NULL"#,
    )
    .bind(presented)
    .fetch_optional(pool)
    .await
    .map_err(|error| db_error(error, "api-token-lookup"))?;
    let Some((token, is_active, expired_at, user_id)) = row else {
        return Err(Denial::InvalidToken);
    };
    let now_unix = chrono::Utc::now().timestamp();
    let row = pidash_auth::token::ApiTokenRow {
        token,
        is_active,
        expired_at_unix: expired_at.map(|dt| dt.timestamp()),
    };
    pidash_auth::token::validate_api_token(Some(&row), presented, now_unix)
        .map_err(|_| Denial::InvalidToken)?;
    // No `users.is_active` check: `validate_api_token` returns
    // `api_token.user` unchecked, and neither DRF `IsAuthenticated` nor
    // the project permission classes consult it — Django serves a
    // deactivated user's token when membership passes. Port the wart.
    Ok(user_id)
}

/// The machine-token path (`api_authentication.py:47-70`, table
/// `machine_token`): HMAC lookup, revoke checks, dev-machine check, then
/// the workspace-membership check (revoke-then-deny like Python).
async fn resolve_machine_token(
    pool: &PgPool,
    presented: &str,
    secret_key: &[u8],
) -> Result<uuid::Uuid, Denial> {
    let presented_hash = pidash_auth::token::hash_token(presented, secret_key);
    let row: Option<MachineTokenLookup> = sqlx::query_as(
        r#"SELECT mt."token_hash", mt."user_id", mt."dev_machine_id", mt."workspace_id", mt."revoked_at",
                  dm."revoked_at" IS NOT NULL AS "dev_revoked"
           FROM "machine_token" mt LEFT OUTER JOIN "dev_machine" dm ON dm."id" = mt."dev_machine_id"
           WHERE mt."token_hash" = $1"#,
    )
    .bind(&presented_hash)
    .fetch_optional(pool)
    .await
    .map_err(|error| db_error(error, "machine-token-lookup"))?;
    let Some((token_hash, user_id, _dev_machine_id, workspace_id, revoked_at, dev_revoked)) = row
    else {
        return Err(Denial::InvalidToken);
    };
    let static_row = pidash_auth::token::MachineTokenRow {
        token_hash,
        revoked_at_unix: revoked_at.map(|dt| dt.timestamp()),
        dev_machine_revoked: dev_revoked.unwrap_or(false),
    };
    // Hash match is implied by the lookup; the kernel still checks the
    // revocation arms.
    pidash_auth::token::validate_machine_token_static(Some(&static_row), &presented_hash)
        .map_err(|_| Denial::InvalidToken)?;
    // `is_workspace_member(user, workspace_id)` (`core/permissions.py:28`);
    // a `false` revokes the token, then denies.
    let member: bool = sqlx::query_scalar(
        r#"SELECT EXISTS(SELECT 1 FROM "workspace_members" WHERE "workspace_id" = $1 AND "member_id" = $2 AND "is_active" AND "deleted_at" IS NULL)"#,
    )
    .bind(workspace_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| db_error(error, "machine-token-member"))?
    .unwrap_or(false);
    if !member {
        // `revoke()` stamps `revoked_at` only (`runner/models.py:865-869`);
        // `last_used_at` is stamped on success only.
        let _ = sqlx::query(
            r#"UPDATE "machine_token" SET "revoked_at" = now() WHERE "token_hash" = $1"#,
        )
        .bind(&presented_hash)
        .execute(pool)
        .await;
        return Err(Denial::InvalidToken);
    }
    let _ =
        sqlx::query(r#"UPDATE "machine_token" SET "last_used_at" = now() WHERE "token_hash" = $1"#)
            .bind(&presented_hash)
            .execute(pool)
            .await;
    Ok(user_id)
}

/// The request time zone (`TimezoneMixin.initial`): the user's stored zone
/// name, loaded but NOT parsed — parsing happens after the gate in
/// [`activate_timezone`].
async fn load_timezone_name(pool: &PgPool, user_id: &uuid::Uuid) -> Result<Option<String>, Denial> {
    let name: Option<Option<String>> =
        sqlx::query_scalar(r#"SELECT "user_timezone" FROM "users" WHERE "id" = $1"#)
            .bind(user_id)
            .fetch_optional(pool)
            .await
            .map_err(|error| db_error(error, "request-timezone"))?;
    Ok(name.unwrap_or(None))
}

/// `posixpath.normpath` (`CPython/Lib/posixpath.py`) for relative keys
/// (absolute keys never reach it — rejected below first): empty collapses
/// to `.`, `.`/empty segments drop, `..` pops or is kept when nothing is
/// left to pop.
fn posix_normpath(path: &str) -> String {
    if path.is_empty() {
        return ".".to_owned();
    }
    let mut kept: Vec<&str> = Vec::new();
    for comp in path.split('/') {
        if comp.is_empty() || comp == "." {
            continue;
        }
        if comp != ".." || kept.is_empty() || kept.last().is_some_and(|last| *last == "..") {
            kept.push(comp);
        } else {
            kept.pop();
        }
    }
    if kept.is_empty() {
        ".".to_owned()
    } else {
        kept.join("/")
    }
}

/// `_validate_tzfile_path` (`CPython/Lib/zoneinfo/_tzpath.py`): absolute
/// keys, keys whose normalization changes their length (`a/../b`,
/// `a/./b`, `a//b`, trailing slashes, the empty key), and keys escaping
/// the search root (`..`, `.`, `../x`) raise `ValueError` — the 500 arm.
/// A NUL byte raises `ValueError` at `open()` time. Well-formed but
/// missing keys raise `ZoneInfoNotFoundError` (`KeyError`) instead — the
/// 400 arm below. (PIDASHCONV-789#6, live-probed.)
fn zoneinfo_value_error(zone: &str) -> bool {
    if zone.starts_with('/') || zone.contains('\0') {
        return true;
    }
    let normal = posix_normpath(zone);
    if normal.len() != zone.len() {
        return true;
    }
    normal == "." || normal == ".." || normal.starts_with("../")
}

/// Activate the actor's rendering timezone (`TimezoneMixin.initial` runs
/// after `super().initial()`). `None` is unreachable (the column is
/// `NOT NULL`) and would be Python's `TypeError` 500, not a UTC default;
/// an unknown zone name 400s: `zoneinfo.ZoneInfo` raises
/// `ZoneInfoNotFoundError`, which subclasses `KeyError`, so
/// `handle_exception` answers the `KeyError` branch
/// (`api/views/base.py:160-164`).
fn activate_timezone(timezone: Option<&str>) -> Result<Tz, Denial> {
    match timezone {
        None => Err(Denial::ServerError),
        Some(zone) if zoneinfo_value_error(zone) => Err(Denial::ServerError),
        Some(zone) => zone
            .parse::<Tz>()
            .map_err(|_| Denial::BadError("The required key does not exist.".to_owned())),
    }
}

// ---------------------------------------------------------------------------
// Identifier rewrite + permissions
// ---------------------------------------------------------------------------

/// Request preamble: pool, actor and optional workspace id. An unknown slug
/// yields `None`: every gate then denies 403, exactly like the
/// slug-filtered membership checks missing in Python.
pub struct Preamble {
    pub pool: PgPool,
    pub actor: Actor,
    pub workspace_id: Option<uuid::Uuid>,
}

pub async fn preamble(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
) -> Result<Preamble, Denial> {
    // Anonymous callers 401 before any pool or database access
    // (`APIKeyAuthentication.authenticate` returns `None` without a key).
    if headers
        .get(pidash_auth::token::API_KEY_HEADER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .is_empty()
    {
        return Err(Denial::Unauthorized);
    }
    let pool = pool_of(state)?;
    let actor = actor(&pool, headers, state.settings().secret_key.as_bytes()).await?;
    let workspace_id: Option<uuid::Uuid> =
        sqlx::query_scalar(r#"SELECT "id" FROM "workspaces" WHERE "slug" = $1"#)
            .bind(slug)
            .fetch_optional(&pool)
            .await
            .map_err(|error| db_error(error, "preamble-workspace"))?
            .flatten();
    Ok(Preamble {
        pool,
        actor,
        workspace_id,
    })
}

/// `_rewrite_project_kwarg` (`api/views/base.py:51-98`): a slug-or-UUID
/// `project_id` becomes the canonical project UUID before permission
/// checks. UUID-looking input passes through unverified (the view body
/// 404/403s it as before); identifier misses answer the
/// `{"detail":"Project not found"}` 404. Reuses the `v1_projects`
/// `Project.resolve` port — the rewrite is shared view code, never
/// re-ported per domain.
pub async fn rewrite_project_id(
    pool: &PgPool,
    workspace_slug: &str,
    raw: &str,
) -> Result<uuid::Uuid, Denial> {
    use pidash_db::v1_projects::models::project::{classify_lookup, ProjectLookup};
    use pidash_db::v1_projects::queries_projmem as q;
    let scope = q::TenantScope {
        workspace_slug,
        actor_id: uuid::Uuid::nil(),
    };
    match classify_lookup(raw) {
        ProjectLookup::Pk(id) => Ok(id),
        ProjectLookup::Identifier(_) => q::fetch_project_id(pool, &scope, raw)
            .await
            .map_err(|error| db_error(error, "rewrite-project"))?
            .ok_or(Denial::ProjectNotFound),
    }
}

/// Fetch the `ProjectEntityPermission` facts
/// (`app/permissions/project.py:85-116`): active project membership plus
/// the Admin/Member role arm, workspace- and project-scoped.
async fn entity_facts(
    pool: &PgPool,
    workspace_id: &uuid::Uuid,
    workspace_slug: &str,
    user_id: &uuid::Uuid,
    project_id: &uuid::Uuid,
) -> Result<project::ProjectFacts, Denial> {
    // `role` is `smallint`: sqlx does not widen `INT2` into `i32` on
    // decode, so read `i16` and compare as integers. `deleted_at IS NULL`
    // is the `SoftDeletionManager` scope (`db/mixins.py:56-58`).
    let roles: Vec<i16> = sqlx::query_scalar(
        r#"SELECT "role" FROM "project_members" WHERE "workspace_id" = $1 AND "member_id" = $2 AND "project_id" = $3 AND "is_active" AND "deleted_at" IS NULL"#,
    )
    .bind(workspace_id)
    .bind(user_id)
    .bind(project_id)
    .fetch_all(pool)
    .await
    .map_err(|error| db_error(error, "entity-roles"))?;
    Ok(project::ProjectFacts {
        workspace: pidash_types::WorkspaceId::from(workspace_slug.to_owned()),
        project_id: pidash_types::ProjectId::from(project_id.to_string()),
        authenticated: true,
        is_workspace_member: false,
        has_workspace_admin_or_member: false,
        is_workspace_admin: false,
        is_project_member: !roles.is_empty(),
        is_project_admin: roles.contains(&20),
        has_project_admin_or_member: roles.iter().any(|r| *r == 20 || *r == 15),
        has_identifier_membership: false,
        has_project_identifier: false,
    })
}

/// Run the route's gate; deny 403 on failure. The activity routes carry
/// `ProjectEntityPermission` (GET → any active project membership); the
/// attachment routes carry `IsAuthenticated` only (`AuthOnly`, always true
/// past the auth layer) and enforce their own inline checks in the body.
async fn require_gate(
    pool: &PgPool,
    workspace_id: &uuid::Uuid,
    workspace_slug: &str,
    user_id: &uuid::Uuid,
    project_id: &uuid::Uuid,
    route: V1WorkItemsRoute,
    method: &str,
) -> Result<(), Denial> {
    let gate = gate_for(route, method);
    let facts = entity_facts(pool, workspace_id, workspace_slug, user_id, project_id).await?;
    let scope = TenantScope::new(pidash_types::WorkspaceId::from(workspace_slug.to_owned()));
    if super::perms::decide(gate, method, &scope, &facts) {
        Ok(())
    } else {
        Err(Denial::Forbidden)
    }
}

/// `user_has_issue_permission` (`views/issue.py:175-189`) with its SQL:
/// creator short-circuit, else the `(project_id, member_id, is_active)`
/// membership with `role__in` when the call site passes roles. No
/// workspace filter — the Python has none either.
async fn issue_permission(
    pool: &PgPool,
    user_id: &uuid::Uuid,
    issue_created_by_id: Option<uuid::Uuid>,
    project_id: &uuid::Uuid,
    allowed_roles: Option<&[i16]>,
    allow_creator: bool,
) -> Result<bool, Denial> {
    if allow_creator && issue_created_by_id == Some(*user_id) {
        return Ok(true);
    }
    let exists: bool = match allowed_roles {
        Some(roles) => sqlx::query_scalar(
            r#"SELECT EXISTS(SELECT 1 FROM "project_members" WHERE "project_id" = $1 AND "member_id" = $2 AND "is_active" AND "role" = ANY($3) AND "deleted_at" IS NULL)"#,
        )
        .bind(project_id)
        .bind(user_id)
        .bind(roles)
        .fetch_one(pool)
        .await
        .map_err(|error| db_error(error, "issue-permission"))?,
        None => sqlx::query_scalar(
            r#"SELECT EXISTS(SELECT 1 FROM "project_members" WHERE "project_id" = $1 AND "member_id" = $2 AND "is_active" AND "deleted_at" IS NULL)"#,
        )
        .bind(project_id)
        .bind(user_id)
        .fetch_one(pool)
        .await
        .map_err(|error| db_error(error, "issue-permission"))?,
    };
    Ok(user_has_issue_permission(
        *user_id,
        issue_created_by_id,
        exists,
        allow_creator,
    ))
}

/// Attachment role set: `[ADMIN, MEMBER, GUEST]` (`db/models/project.py:28-31`).
const ATTACHMENT_ROLES: &[i16] = &[20, 15, 5];

// ---------------------------------------------------------------------------
// Lookups
// ---------------------------------------------------------------------------

/// `Issue.objects.get(pk, workspace__slug, project_id)` for the attachment
/// permission checks (`:2318`, `:2474`, `:2612`): the plain manager
/// (`deleted_at IS NULL` only — triage/archived/draft rows load here).
async fn fetch_issue_head(
    pool: &PgPool,
    slug: &str,
    project_id: &Uuid,
    issue_id: &Uuid,
) -> Result<Option<(Uuid, Option<Uuid>)>, Denial> {
    sqlx::query_as(
        r#"SELECT i."id", i."created_by_id" FROM "issues" i
           INNER JOIN "workspaces" w ON w."id" = i."workspace_id"
           WHERE i."deleted_at" IS NULL AND i."id" = $1 AND i."project_id" = $2 AND w."slug" = $3"#,
    )
    .bind(issue_id)
    .bind(project_id)
    .bind(slug)
    .fetch_optional(pool)
    .await
    .map_err(|error| db_error(error, "attachment-issue"))
}

/// `Workspace.objects.get(slug=slug)` (`:2354`): the key factory needs the
/// workspace id. Unreachable with a bogus slug (the issue load 404s
/// first), but ported faithfully.
async fn fetch_workspace_id(pool: &PgPool, slug: &str) -> Result<Option<Uuid>, Denial> {
    sqlx::query_scalar(
        r#"SELECT "id" FROM "workspaces" WHERE "slug" = $1 AND "deleted_at" IS NULL"#,
    )
    .bind(slug)
    .fetch_optional(pool)
    .await
    .map_err(|error| db_error(error, "attachment-workspace"))
}

/// Resolved activity `ORDER BY`: the extra JOIN clauses (empty when the
/// order needs none) plus the `ORDER BY` fragment (without the keywords).
#[derive(Debug, Clone, PartialEq, Eq)]
struct ActivityOrder {
    /// `LEFT JOIN ...` clauses (each already newline+indent prefixed, so an
    /// empty string leaves the base SQL byte-identical).
    joins: String,
    /// `ORDER BY` terms without the keywords, e.g. `p."name" ASC`.
    order: String,
}

/// One node of the forward-span model graph (`db/models/*.py`,
/// `runner/models.py`): table, pk column, `Meta.ordering` terms and the
/// forward many-to-one (plus the one forward one-to-one,
/// `IssueComment.description`, which spans like an FK) map. Field names
/// come from live Django introspection (all columns equal their field
/// names; every pk is `id`); reverse relations and M2M fields are absent,
/// so spans through them 500 (residual edge — Django 200s with row
/// fanout, verified by probe).
struct SpanModel {
    table: &'static str,
    pk: &'static str,
    /// `Meta.ordering` as `(column, descending)` terms; empty means the
    /// model declares no ordering (terminal FKs then collapse to the
    /// parent's FK column, per the `issue__type` probe).
    ordering: &'static [(&'static str, bool)],
    /// Concrete non-FK field names (all are their own columns), plus the
    /// attnames of FKs whose targets sit outside the graph
    /// (`FileAsset.draft_issue_id/page_id`) so they still resolve as
    /// terminal columns.
    columns: &'static [&'static str],
    /// Forward FKs: `(field name, attname, target model index)`.
    /// Hops match the field name or the attname (`project_id__name`
    /// 200s in Django, verified by probe).
    fks: &'static [(&'static str, &'static str, usize)],
}

const IDX_PROJECT: usize = 0;
const IDX_WORKSPACE: usize = 1;
const IDX_ISSUE: usize = 2;
const IDX_USER: usize = 3;
const IDX_COMMENT: usize = 4;
const IDX_FILE_ASSET: usize = 5;
const IDX_STATE: usize = 6;
const IDX_ESTIMATE: usize = 7;
const IDX_ESTIMATE_POINT: usize = 8;
const IDX_ISSUE_TYPE: usize = 9;
const IDX_DESCRIPTION: usize = 10;
const IDX_POD: usize = 11;

const SPAN_MODELS: &[SpanModel] = &[
    SpanModel {
        // db.Project
        table: "projects",
        pk: "id",
        ordering: &[("created_at", true)],
        columns: &[
            "agent_default_interval_seconds",
            "agent_default_max_ticks",
            "agent_review_default_interval_seconds",
            "agent_test_default_interval_seconds",
            "agent_ticking_enabled",
            "archive_in",
            "archived_at",
            "base_branch",
            "close_in",
            "cover_image",
            "created_at",
            "cycle_view",
            "default_agent_executor",
            "deleted_at",
            "description",
            "description_html",
            "description_text",
            "emoji",
            "external_id",
            "external_source",
            "guest_view_all_features",
            "icon_prop",
            "id",
            "identifier",
            "intake_view",
            "is_default",
            "is_issue_type_enabled",
            "is_time_tracking_enabled",
            "issue_views_view",
            "logo_props",
            "members_can_edit_states",
            "module_view",
            "name",
            "network",
            "page_view",
            "repo_url",
            "timezone",
            "updated_at",
        ],
        fks: &[
            ("cover_image_asset", "cover_image_asset_id", IDX_FILE_ASSET),
            ("created_by", "created_by_id", IDX_USER),
            ("default_assignee", "default_assignee_id", IDX_USER),
            ("default_state", "default_state_id", IDX_STATE),
            ("estimate", "estimate_id", IDX_ESTIMATE),
            ("project_lead", "project_lead_id", IDX_USER),
            ("updated_by", "updated_by_id", IDX_USER),
            ("workspace", "workspace_id", IDX_WORKSPACE),
        ],
    },
    SpanModel {
        // db.Workspace
        table: "workspaces",
        pk: "id",
        ordering: &[("created_at", true)],
        columns: &[
            "background_color",
            "created_at",
            "deleted_at",
            "id",
            "logo",
            "name",
            "organization_size",
            "slug",
            "timezone",
            "updated_at",
        ],
        fks: &[
            ("created_by", "created_by_id", IDX_USER),
            ("logo_asset", "logo_asset_id", IDX_FILE_ASSET),
            ("owner", "owner_id", IDX_USER),
            ("updated_by", "updated_by_id", IDX_USER),
        ],
    },
    SpanModel {
        // db.Issue
        table: "issues",
        pk: "id",
        ordering: &[("created_at", true)],
        columns: &[
            "agent_executor",
            "archived_at",
            "completed_at",
            "complexity_score",
            "created_at",
            "created_via",
            "deleted_at",
            "description_binary",
            "description_html",
            "description_json",
            "description_stripped",
            "external_id",
            "external_source",
            "git_work_branch",
            "id",
            "is_draft",
            "name",
            "point",
            "priority",
            "sequence_id",
            "sort_order",
            "start_date",
            "target_date",
            "updated_at",
            "workpad",
        ],
        fks: &[
            ("assigned_pod", "assigned_pod_id", IDX_POD),
            ("created_by", "created_by_id", IDX_USER),
            ("estimate_point", "estimate_point_id", IDX_ESTIMATE_POINT),
            ("parent", "parent_id", IDX_ISSUE),
            ("project", "project_id", IDX_PROJECT),
            ("state", "state_id", IDX_STATE),
            ("type", "type_id", IDX_ISSUE_TYPE),
            ("updated_by", "updated_by_id", IDX_USER),
            ("workspace", "workspace_id", IDX_WORKSPACE),
        ],
    },
    SpanModel {
        // db.User
        table: "users",
        pk: "id",
        ordering: &[("created_at", true)],
        columns: &[
            "avatar",
            "bot_type",
            "cover_image",
            "created_at",
            "created_location",
            "date_joined",
            "display_name",
            "email",
            "first_name",
            "id",
            "is_active",
            "is_bot",
            "is_email_valid",
            "is_email_verified",
            "is_managed",
            "is_password_autoset",
            "is_password_expired",
            "is_password_reset_required",
            "is_staff",
            "is_superuser",
            "last_active",
            "last_location",
            "last_login",
            "last_login_ip",
            "last_login_medium",
            "last_login_time",
            "last_login_uagent",
            "last_logout_ip",
            "last_logout_time",
            "last_name",
            "masked_at",
            "mobile_number",
            "password",
            "token",
            "token_updated_at",
            "updated_at",
            "user_timezone",
            "username",
        ],
        fks: &[
            ("avatar_asset", "avatar_asset_id", IDX_FILE_ASSET),
            ("cover_image_asset", "cover_image_asset_id", IDX_FILE_ASSET),
        ],
    },
    SpanModel {
        // db.IssueComment
        table: "issue_comments",
        pk: "id",
        ordering: &[("created_at", true)],
        columns: &[
            "access",
            "attachments",
            "comment_html",
            "comment_json",
            "comment_stripped",
            "created_at",
            "deleted_at",
            "edited_at",
            "external_id",
            "external_source",
            "id",
            "labels",
            "speaker_agent_run_id",
            "speaker_label",
            "speaker_type",
            "updated_at",
        ],
        fks: &[
            ("actor", "actor_id", IDX_USER),
            ("created_by", "created_by_id", IDX_USER),
            ("description", "description_id", IDX_DESCRIPTION),
            ("issue", "issue_id", IDX_ISSUE),
            ("parent", "parent_id", IDX_COMMENT),
            ("project", "project_id", IDX_PROJECT),
            ("updated_by", "updated_by_id", IDX_USER),
            ("workspace", "workspace_id", IDX_WORKSPACE),
        ],
    },
    SpanModel {
        // db.FileAsset (`draft_issue`/`page` targets sit outside the
        // graph: their attnames resolve as terminal columns but hops
        // through them 500 — residual edge).
        table: "file_assets",
        pk: "id",
        ordering: &[("created_at", true)],
        columns: &[
            "asset",
            "attributes",
            "created_at",
            "deleted_at",
            "draft_issue_id",
            "entity_identifier",
            "entity_type",
            "external_id",
            "external_source",
            "id",
            "is_archived",
            "is_deleted",
            "is_uploaded",
            "page_id",
            "size",
            "storage_metadata",
            "updated_at",
        ],
        fks: &[
            ("comment", "comment_id", IDX_COMMENT),
            ("created_by", "created_by_id", IDX_USER),
            ("issue", "issue_id", IDX_ISSUE),
            ("project", "project_id", IDX_PROJECT),
            ("updated_by", "updated_by_id", IDX_USER),
            ("user", "user_id", IDX_USER),
            ("workspace", "workspace_id", IDX_WORKSPACE),
        ],
    },
    SpanModel {
        // db.State
        table: "states",
        pk: "id",
        ordering: &[("sequence", false)],
        columns: &[
            "color",
            "created_at",
            "default",
            "deleted_at",
            "description",
            "external_id",
            "external_source",
            "group",
            "id",
            "is_triage",
            "name",
            "sequence",
            "slug",
            "updated_at",
        ],
        fks: &[
            ("created_by", "created_by_id", IDX_USER),
            ("project", "project_id", IDX_PROJECT),
            ("updated_by", "updated_by_id", IDX_USER),
            ("workspace", "workspace_id", IDX_WORKSPACE),
        ],
    },
    SpanModel {
        // db.Estimate
        table: "estimates",
        pk: "id",
        ordering: &[("name", false)],
        columns: &[
            "created_at",
            "deleted_at",
            "description",
            "id",
            "last_used",
            "name",
            "type",
            "updated_at",
        ],
        fks: &[
            ("created_by", "created_by_id", IDX_USER),
            ("project", "project_id", IDX_PROJECT),
            ("updated_by", "updated_by_id", IDX_USER),
            ("workspace", "workspace_id", IDX_WORKSPACE),
        ],
    },
    SpanModel {
        // db.EstimatePoint
        table: "estimate_points",
        pk: "id",
        ordering: &[("value", false)],
        columns: &[
            "created_at",
            "deleted_at",
            "description",
            "id",
            "key",
            "updated_at",
            "value",
        ],
        fks: &[
            ("created_by", "created_by_id", IDX_USER),
            ("estimate", "estimate_id", IDX_ESTIMATE),
            ("project", "project_id", IDX_PROJECT),
            ("updated_by", "updated_by_id", IDX_USER),
            ("workspace", "workspace_id", IDX_WORKSPACE),
        ],
    },
    SpanModel {
        // db.IssueType (no Meta.ordering: terminal FKs to it collapse
        // to the parent's FK column, verified by the `issue__type` probe).
        table: "issue_types",
        pk: "id",
        ordering: &[],
        columns: &[
            "created_at",
            "deleted_at",
            "description",
            "external_id",
            "external_source",
            "id",
            "is_active",
            "is_default",
            "is_epic",
            "level",
            "logo_props",
            "name",
            "updated_at",
        ],
        fks: &[
            ("created_by", "created_by_id", IDX_USER),
            ("updated_by", "updated_by_id", IDX_USER),
            ("workspace", "workspace_id", IDX_WORKSPACE),
        ],
    },
    SpanModel {
        // db.Description
        table: "descriptions",
        pk: "id",
        ordering: &[("created_at", true)],
        columns: &[
            "created_at",
            "deleted_at",
            "description_binary",
            "description_html",
            "description_json",
            "description_stripped",
            "id",
            "updated_at",
        ],
        fks: &[
            ("created_by", "created_by_id", IDX_USER),
            ("project", "project_id", IDX_PROJECT),
            ("updated_by", "updated_by_id", IDX_USER),
            ("workspace", "workspace_id", IDX_WORKSPACE),
        ],
    },
    SpanModel {
        // runner.Pod (multi-term Meta.ordering, verified by probe).
        table: "pod",
        pk: "id",
        ordering: &[("is_default", true), ("created_at", false)],
        columns: &[
            "created_at",
            "deleted_at",
            "description",
            "id",
            "is_default",
            "name",
            "updated_at",
        ],
        fks: &[
            ("created_by", "created_by_id", IDX_USER),
            ("project", "project_id", IDX_PROJECT),
            ("workspace", "workspace_id", IDX_WORKSPACE),
        ],
    },
];

/// `IssueActivity` forward FKs: `(field name, attname, target model index)`.
const ACTIVITY_FK: &[(&str, &str, usize)] = &[
    ("created_by", "created_by_id", IDX_USER),
    ("issue", "issue_id", IDX_ISSUE),
    ("issue_comment", "issue_comment_id", IDX_COMMENT),
    ("project", "project_id", IDX_PROJECT),
    ("updated_by", "updated_by_id", IDX_USER),
    ("workspace", "workspace_id", IDX_WORKSPACE),
    ("actor", "actor_id", IDX_USER),
];

/// Match one hop segment against an FK map by field name or attname.
fn match_hop(
    fks: &[(&'static str, &'static str, usize)],
    segment: &str,
) -> Option<(&'static str, usize)> {
    for (field, attname, target) in fks {
        if segment == *field || segment == *attname {
            return Some((attname, *target));
        }
    }
    None
}

/// Emit the JOIN for one order hop. First-hop `project`/`workspace` reuse
/// the list/detail queries' existing `p`/`w` aliases (the same joins Django
/// reuses off its filter path, verified by probe); every other hop adds a
/// row-preserving `LEFT JOIN` (`o1`, `o2`, …) like Django's ordering joins.
/// Returns the hop's alias; `counter` feeds the `oN` numbering.
fn emit_order_join(
    joins: &mut String,
    counter: &mut u32,
    parent: &str,
    attname: &str,
    target: usize,
    first: bool,
) -> String {
    if first && (target == IDX_PROJECT || target == IDX_WORKSPACE) {
        return if target == IDX_PROJECT {
            "p".to_owned()
        } else {
            "w".to_owned()
        };
    }
    *counter += 1;
    let alias = format!("o{counter}", counter = *counter);
    let table = SPAN_MODELS[target].table;
    let pk = SPAN_MODELS[target].pk;
    joins.push_str(&format!(
        "\n           LEFT JOIN \"{table}\" {alias} ON {alias}.\"{pk}\" = {parent}.\"{attname}\""
    ));
    alias
}

/// `order_by` resolution for the activity chains (`.order_by(...)`,
/// `:2165`, `:2222`): exactly `?` orders randomly; otherwise one leading
/// `-` selects descending and the rest is either a model field name, an
/// FK attname, or `pk` (verified against Django's `names_to_path`), or a
/// forward `__` span across relations. A bare FK *field* name (`actor`)
/// follows the relation and orders by the target's `Meta.ordering`
/// (PIDASHCONV-759); a bare attname (`actor_id`) is a concrete column
/// and keeps the local-column order. Span hops reuse the list/detail
/// queries' existing `p`/`w` joins for first-hop `project`/`workspace`
/// (the same joins Django reuses off its filter path, verified by probe)
/// and add row-preserving `LEFT JOIN`s (`o1`, `o2`, …) for every other
/// hop, like Django's ordering joins. A terminal `pk`/pk-column
/// collapses to the parent's FK column with no new join
/// (`project__workspace__id` → `p."workspace_id"`); a terminal FK name
/// orders by the target's `Meta.ordering` (`issue__state` →
/// `states."sequence"`), and a leading `-` flips every term. Anything
/// else raises `FieldError` into the generic 500.
fn resolve_activity_order(raw: Option<&str>) -> Result<ActivityOrder, Denial> {
    const COLUMNS: &[&str] = &[
        "created_at",
        "updated_at",
        "id",
        "verb",
        "field",
        "old_value",
        "new_value",
        "comment",
        "attachments",
        "new_identifier",
        "old_identifier",
        "epoch",
        "deleted_at",
    ];
    fn direction(descending: bool) -> &'static str {
        if descending {
            "DESC"
        } else {
            "ASC"
        }
    }
    let text = raw.unwrap_or(queries_sub::ACTIVITY_ORDER_DEFAULT);
    if text == "?" {
        return Ok(ActivityOrder {
            joins: String::new(),
            order: "RANDOM()".to_owned(),
        });
    }
    let (descending, name) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    if !name.contains("__") {
        // Bare FK field name (`actor`): Django follows the relation and
        // orders by the target's `Meta.ordering` — the terminal-FK rule
        // at the top level — and a leading `-` flips every term. Targets
        // without ordering collapse to the local FK column with no join
        // (no activity FK target lacks ordering today; the arm mirrors
        // the terminal rule so a future one stays local).
        if let Some((attname, target)) = match_hop(ACTIVITY_FK, name) {
            if ACTIVITY_FK.iter().any(|(field, _, _)| *field == name) {
                let ordering = SPAN_MODELS[target].ordering;
                if ordering.is_empty() {
                    return Ok(ActivityOrder {
                        joins: String::new(),
                        order: format!(r#"a."{attname}" {}"#, direction(descending)),
                    });
                }
                let mut joins = String::new();
                let mut counter = 0u32;
                let alias = emit_order_join(&mut joins, &mut counter, "a", attname, target, true);
                let mut terms = Vec::with_capacity(ordering.len());
                for &(column, term_descending) in ordering {
                    terms.push(format!(
                        r#"{alias}."{column}" {}"#,
                        direction(term_descending ^ descending)
                    ));
                }
                return Ok(ActivityOrder {
                    joins,
                    order: terms.join(", "),
                });
            }
            // Bare attname (`actor_id`): a concrete column, so the order
            // stays on the local FK column (verified by probe).
            return Ok(ActivityOrder {
                joins: String::new(),
                order: format!(r#"a."{attname}" {}"#, direction(descending)),
            });
        }
        let mut column: Option<&str> = None;
        if name == "pk" {
            column = Some("id");
        } else if COLUMNS.contains(&name) {
            column = Some(name);
        }
        let Some(column) = column else {
            return Err(Denial::ServerError);
        };
        // Qualified with the list/detail queries' `a` alias (a bare
        // `"issue_activities"` reference is invisible once aliased).
        return Ok(ActivityOrder {
            joins: String::new(),
            order: format!(r#"a."{column}" {}"#, direction(descending)),
        });
    }
    resolve_activity_span(name, descending)
}

/// Forward-span `order_by` (`project__name`, `actor__email`,
/// `project__workspace__name`, …): join every hop but the last, then
/// resolve the terminal on the last hop's target.
fn resolve_activity_span(name: &str, descending: bool) -> Result<ActivityOrder, Denial> {
    let segments: Vec<&str> = name.split("__").collect();
    if segments.iter().any(|segment| segment.is_empty()) {
        return Err(Denial::ServerError);
    }
    let (hops, terminal) = segments.split_at(segments.len() - 1);
    let terminal = terminal[0];
    let mut joins = String::new();
    let mut alias_counter = 0u32;
    // Parent alias of the hop being joined (`a` for the first hop).
    let mut parent_alias = "a".to_owned();
    // Model index the next hop resolves against (`None` = IssueActivity).
    let mut model: Option<usize> = None;
    // Join every hop but the last (the last hop joins only when the
    // terminal needs its table: plain columns and FK names do, a
    // terminal pk collapses to the parent's FK column without one).
    for (index, segment) in hops.iter().enumerate() {
        let last = index + 1 == hops.len();
        let (attname, target) = match model {
            None => match_hop(ACTIVITY_FK, segment),
            Some(model) => match_hop(SPAN_MODELS[model].fks, segment),
        }
        .ok_or(Denial::ServerError)?;
        if last {
            let order = resolve_span_terminal(
                terminal,
                target,
                &parent_alias,
                attname,
                descending,
                index == 0,
                &mut alias_counter,
                &mut joins,
            )?;
            return Ok(ActivityOrder { joins, order });
        }
        let alias = emit_order_join(
            &mut joins,
            &mut alias_counter,
            &parent_alias,
            attname,
            target,
            index == 0,
        );
        parent_alias = alias;
        model = Some(target);
    }
    // Unreachable: spans hold at least one hop plus a terminal.
    Err(Denial::ServerError)
}

/// Resolve the terminal segment against the last hop's target. `parent`
/// is the alias the last hop joins from, `attname` its FK column.
#[allow(clippy::too_many_arguments)]
fn resolve_span_terminal(
    terminal: &str,
    target: usize,
    parent: &str,
    attname: &str,
    descending: bool,
    first_hop: bool,
    counter: &mut u32,
    joins: &mut String,
) -> Result<String, Denial> {
    fn direction(descending: bool) -> &'static str {
        if descending {
            "DESC"
        } else {
            "ASC"
        }
    }
    let model = &SPAN_MODELS[target];
    // Terminal pk collapses to the parent's FK column (no new join):
    // `actor__id` → `a."actor_id"`, `project__workspace__id` →
    // `p."workspace_id"` (verified by probe).
    if terminal == "pk" || terminal == model.pk {
        return Ok(format!(r#"{parent}."{attname}" {}"#, direction(descending)));
    }
    // Terminal concrete column (field name and attname coincide for
    // non-FK fields; FK attnames resolve to their columns too, e.g.
    // `project__workspace_id` → `p."workspace_id"`).
    if model.columns.contains(&terminal)
        || model
            .fks
            .iter()
            .any(|(_, fk_attname, _)| terminal == *fk_attname)
    {
        let alias = emit_order_join(joins, counter, parent, attname, target, first_hop);
        return Ok(format!(r#"{alias}."{terminal}" {}"#, direction(descending)));
    }
    // Terminal FK name orders by the target's `Meta.ordering`
    // (`issue__state` → `o1."sequence" ASC`); a leading `-` flips
    // every term. Models without ordering collapse to the parent's FK
    // column with no target join (`issue__type` → `o1."type_id"`).
    // (Attnames resolved as columns above, so only field names match.)
    let fk_hit = model
        .fks
        .iter()
        .find(|(field, _, _)| terminal == *field)
        .map(|(_, fk_attname, fk_target)| (*fk_attname, *fk_target));
    if let Some((fk_attname, fk_target)) = fk_hit {
        let ordering = SPAN_MODELS[fk_target].ordering;
        let alias = emit_order_join(joins, counter, parent, attname, target, first_hop);
        if ordering.is_empty() {
            return Ok(format!(
                r#"{alias}."{fk_attname}" {}"#,
                direction(descending)
            ));
        }
        let mut terms = Vec::with_capacity(ordering.len());
        let target_alias = emit_order_join(joins, counter, &alias, fk_attname, fk_target, false);
        for &(column, term_descending) in ordering {
            terms.push(format!(
                r#"{target_alias}."{column}" {}"#,
                direction(term_descending ^ descending)
            ));
        }
        return Ok(terms.join(", "));
    }
    Err(Denial::ServerError)
}

/// Activity list rows (`:2156-2165`): the recorded F18-07 predicates —
/// live rows for this issue/project/slug, the comment/vote/reaction/draft
/// exclusion, an active membership of the caller on the project, a live
/// project. Joined tables carry no `deleted_at` scope (Django filters
/// never inject related managers' scopes — fixture-verbatim).
async fn fetch_activity_rows(
    pool: &PgPool,
    slug: &str,
    project_id: &Uuid,
    issue_id: &Uuid,
    user_id: &Uuid,
    order: &ActivityOrder,
) -> Result<Vec<sqlx::postgres::PgRow>, Denial> {
    let sql = format!(
        r#"SELECT a.* FROM "issue_activities" a
           INNER JOIN "workspaces" w ON w."id" = a."workspace_id"
           INNER JOIN "projects" p ON p."id" = a."project_id"
           INNER JOIN "project_members" pm ON pm."project_id" = p."id"{joins}
           WHERE a."deleted_at" IS NULL
             AND a."issue_id" = $1 AND a."project_id" = $2 AND w."slug" = $3
             AND NOT (a."field" IN ('comment', 'vote', 'reaction', 'draft') AND a."field" IS NOT NULL)
             AND pm."member_id" = $4 AND pm."is_active"
             AND p."archived_at" IS NULL
           ORDER BY {terms}"#,
        joins = order.joins,
        terms = order.order,
    );
    sqlx::query(&sql)
        .bind(issue_id)
        .bind(project_id)
        .bind(slug)
        .bind(user_id)
        .fetch_all(pool)
        .await
        .map_err(|error| db_error(error, "activity-list"))
}

/// Activity detail row (`:2211-2224`): the list chain plus `id=pk` with
/// `.order_by(...).first()` (`LIMIT 1`).
async fn fetch_activity_detail(
    pool: &PgPool,
    slug: &str,
    project_id: &Uuid,
    issue_id: &Uuid,
    user_id: &Uuid,
    pk: &Uuid,
    order: &ActivityOrder,
) -> Result<Option<sqlx::postgres::PgRow>, Denial> {
    let sql = format!(
        r#"SELECT a.* FROM "issue_activities" a
           INNER JOIN "workspaces" w ON w."id" = a."workspace_id"
           INNER JOIN "projects" p ON p."id" = a."project_id"
           INNER JOIN "project_members" pm ON pm."project_id" = p."id"{joins}
           WHERE a."deleted_at" IS NULL
             AND a."issue_id" = $1 AND a."project_id" = $2 AND w."slug" = $3 AND a."id" = $4
             AND NOT (a."field" IN ('comment', 'vote', 'reaction', 'draft') AND a."field" IS NOT NULL)
             AND pm."member_id" = $5 AND pm."is_active"
             AND p."archived_at" IS NULL
           ORDER BY {terms} LIMIT 1"#,
        joins = order.joins,
        terms = order.order,
    );
    sqlx::query(&sql)
        .bind(issue_id)
        .bind(project_id)
        .bind(slug)
        .bind(pk)
        .bind(user_id)
        .fetch_optional(pool)
        .await
        .map_err(|error| db_error(error, "activity-detail"))
}

/// Attachment list rows (`:2438-2444`): live uploaded `ISSUE_ATTACHMENT`
/// rows for this issue/project/slug, `Meta.ordering` (`-created_at`) —
/// and no member or archived guard (BUG-1).
async fn fetch_attachment_rows(
    pool: &PgPool,
    slug: &str,
    project_id: &Uuid,
    issue_id: &Uuid,
) -> Result<Vec<sqlx::postgres::PgRow>, Denial> {
    sqlx::query(
        r#"SELECT fa.* FROM "file_assets" fa
           INNER JOIN "workspaces" w ON w."id" = fa."workspace_id"
           WHERE fa."deleted_at" IS NULL
             AND fa."issue_id" = $1 AND fa."entity_type" = 'ISSUE_ATTACHMENT'
             AND w."slug" = $2 AND fa."project_id" = $3 AND fa."is_uploaded"
           ORDER BY fa."created_at" DESC"#,
    )
    .bind(issue_id)
    .bind(slug)
    .bind(project_id)
    .fetch_all(pool)
    .await
    .map_err(|error| db_error(error, "attachment-list"))
}

/// Attachment detail lookup (`:2488`, `:2562`, `:2626`):
/// `FileAsset.objects.get(pk, workspace__slug, project_id)`.
async fn fetch_attachment(
    pool: &PgPool,
    slug: &str,
    project_id: &Uuid,
    pk: &Uuid,
) -> Result<Option<sqlx::postgres::PgRow>, Denial> {
    sqlx::query(
        r#"SELECT fa.* FROM "file_assets" fa
           INNER JOIN "workspaces" w ON w."id" = fa."workspace_id"
           WHERE fa."deleted_at" IS NULL AND fa."id" = $1 AND w."slug" = $2 AND fa."project_id" = $3"#,
    )
    .bind(pk)
    .bind(slug)
    .bind(project_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| db_error(error, "attachment-detail"))
}

/// External-duplicate probe (`:2359-2378`): the `exists()` plus the
/// `.first()` id in one round trip (same 409 outcome; the concurrent
/// delete between the two Python queries is unobservable here).
async fn fetch_external_duplicate(
    pool: &PgPool,
    slug: &str,
    project_id: &Uuid,
    issue_id: &Uuid,
    external_source: &str,
    external_id: &str,
) -> Result<Option<Uuid>, Denial> {
    sqlx::query_scalar(
        r#"SELECT fa."id" FROM "file_assets" fa
           INNER JOIN "workspaces" w ON w."id" = fa."workspace_id"
           WHERE fa."deleted_at" IS NULL
             AND fa."project_id" = $1 AND w."slug" = $2
             AND fa."external_source" = $3 AND fa."external_id" = $4
             AND fa."issue_id" = $5 AND fa."entity_type" = 'ISSUE_ATTACHMENT'
           LIMIT 1"#,
    )
    .bind(project_id)
    .bind(slug)
    .bind(external_source)
    .bind(external_id)
    .bind(issue_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| db_error(error, "attachment-dedupe"))
}

// ---------------------------------------------------------------------------
// Row mapping + renders
// ---------------------------------------------------------------------------

fn row_uuid(row: &sqlx::postgres::PgRow, column: &str) -> Result<Uuid, Denial> {
    row.try_get(column).map_err(|_| Denial::ServerError)
}

fn row_uuid_opt(row: &sqlx::postgres::PgRow, column: &str) -> Result<Option<Uuid>, Denial> {
    row.try_get(column).map_err(|_| Denial::ServerError)
}

fn row_string(row: &sqlx::postgres::PgRow, column: &str) -> Result<String, Denial> {
    row.try_get(column).map_err(|_| Denial::ServerError)
}

fn row_string_opt(row: &sqlx::postgres::PgRow, column: &str) -> Result<Option<String>, Denial> {
    row.try_get(column).map_err(|_| Denial::ServerError)
}

fn row_datetime(row: &sqlx::postgres::PgRow, column: &str) -> Result<DateTime<Utc>, Denial> {
    row.try_get(column).map_err(|_| Denial::ServerError)
}

fn row_datetime_opt(
    row: &sqlx::postgres::PgRow,
    column: &str,
) -> Result<Option<DateTime<Utc>>, Denial> {
    row.try_get(column).map_err(|_| Denial::ServerError)
}

/// One decoded activity row: owned strings the [`ActivityRow`] borrows.
struct DecodedActivity {
    id: String,
    created_at: String,
    updated_at: String,
    deleted_at: Option<String>,
    verb: String,
    field: Option<String>,
    old_value: Option<String>,
    new_value: Option<String>,
    comment: String,
    attachments: Vec<Option<String>>,
    old_identifier: Option<String>,
    new_identifier: Option<String>,
    epoch: Option<f64>,
    project: String,
    workspace: String,
    issue: Option<String>,
    issue_comment: Option<String>,
    actor: Option<String>,
}

fn decode_activity(row: &sqlx::postgres::PgRow, tz: &Tz) -> Result<DecodedActivity, Denial> {
    let attachments: Vec<Option<String>> = row
        .try_get("attachments")
        .map_err(|_| Denial::ServerError)?;
    Ok(DecodedActivity {
        id: row_uuid(row, "id")?.to_string(),
        created_at: crate::serializer::render_datetime_in(&row_datetime(row, "created_at")?, tz),
        updated_at: crate::serializer::render_datetime_in(&row_datetime(row, "updated_at")?, tz),
        deleted_at: row_datetime_opt(row, "deleted_at")?
            .map(|dt| crate::serializer::render_datetime_in(&dt, tz)),
        verb: row_string(row, "verb")?,
        field: row_string_opt(row, "field")?,
        old_value: row_string_opt(row, "old_value")?,
        new_value: row_string_opt(row, "new_value")?,
        comment: row_string(row, "comment")?,
        attachments,
        old_identifier: row_uuid_opt(row, "old_identifier")?.map(|id| id.to_string()),
        new_identifier: row_uuid_opt(row, "new_identifier")?.map(|id| id.to_string()),
        epoch: row.try_get("epoch").map_err(|_| Denial::ServerError)?,
        project: row_uuid(row, "project_id")?.to_string(),
        workspace: row_uuid(row, "workspace_id")?.to_string(),
        issue: row_uuid_opt(row, "issue_id")?.map(|id| id.to_string()),
        issue_comment: row_uuid_opt(row, "issue_comment_id")?.map(|id| id.to_string()),
        actor: row_uuid_opt(row, "actor_id")?.map(|id| id.to_string()),
    })
}

/// Sentinel nucleus for a non-finite float inside a dumped map
/// (`current_instance` only — responses 500 on non-finite floats under
/// DRF strict rendering). NUL bytes cannot arrive from Postgres text, so
/// the marker survives serialization verbatim and is swapped for the
/// `json.dumps` literal afterwards (`allow_nan`, the dumps default).
const NONFINITE_SENTINEL: &str = "pidash-nonfinite-\u{0}";

/// Swap sentinel strings for the DRF `NaN`/`Infinity` literals. The
/// quoted form comes from serde itself (the NUL nucleus serializes as
/// `\u0000`), so the match is exact.
fn apply_nonfinite(body: String, replacements: &[(&str, &str)]) -> String {
    let mut body = body;
    for (sentinel, literal) in replacements {
        let quoted = json_string(sentinel);
        body = body.replace(&quoted, literal);
    }
    body
}

fn py_float_literal(value: f64) -> &'static str {
    if value.is_nan() {
        "NaN"
    } else if value.is_sign_negative() {
        "-Infinity"
    } else {
        "Infinity"
    }
}

/// Render one activity row through [`render_activity`]: every shape
/// error 500s like Django — including a non-finite `epoch`, which trips
/// DRF's strict renderer (`STRICT_JSON`, `allow_nan=False`) into a
/// `ValueError` 500.
fn render_activity_value(
    decoded: &DecodedActivity,
    fields: Option<&[pidash_services::v1_work_items::FieldSpec]>,
    expand: &[&str],
    expansions: &[(&str, Option<Value>)],
) -> Result<Value, Denial> {
    let attachments: Vec<Option<&str>> = decoded
        .attachments
        .iter()
        .map(|item| item.as_deref())
        .collect();
    let epoch = decoded.epoch;
    let row = ActivityRow {
        id: &decoded.id,
        created_at: &decoded.created_at,
        updated_at: &decoded.updated_at,
        deleted_at: decoded.deleted_at.as_deref(),
        verb: &decoded.verb,
        field: decoded.field.as_deref(),
        old_value: decoded.old_value.as_deref(),
        new_value: decoded.new_value.as_deref(),
        comment: &decoded.comment,
        attachments: &attachments,
        old_identifier: decoded.old_identifier.as_deref(),
        new_identifier: decoded.new_identifier.as_deref(),
        epoch,
        project: &decoded.project,
        workspace: &decoded.workspace,
        issue: decoded.issue.as_deref(),
        issue_comment: decoded.issue_comment.as_deref(),
        actor: decoded.actor.as_deref(),
    };
    let out = render_activity(&ActivityRepresentationInput {
        row: &row,
        fields,
        expand,
        expansions,
    })
    .map_err(|_| Denial::ServerError)?;
    Ok(Value::Object(out))
}

/// Swap every non-finite sentinel for its `json.dumps` literal on
/// dumped text. Applied unconditionally: absent sentinels are no-op
/// replaces, and real data cannot contain the NUL nucleus (Postgres text
/// rejects NUL).
fn finalize_nonfinite(body: String) -> String {
    let nan = format!("{NONFINITE_SENTINEL}NaN");
    let inf = format!("{NONFINITE_SENTINEL}Infinity");
    let ninf = format!("{NONFINITE_SENTINEL}-Infinity");
    // The three quoted sentinels share no substring (the `-` breaks the
    // `Infinity` tail), so one pass in any order is exact.
    apply_nonfinite(
        body,
        &[(&nan, "NaN"), (&inf, "Infinity"), (&ninf, "-Infinity")],
    )
}

/// One decoded attachment row: owned strings the [`AttachmentRow`] borrows.
struct DecodedAttachment {
    id: String,
    created_at: String,
    updated_at: String,
    deleted_at: Option<String>,
    attributes: Value,
    asset: String,
    entity_type: Option<String>,
    entity_identifier: Option<String>,
    is_deleted: bool,
    is_archived: bool,
    external_id: Option<String>,
    external_source: Option<String>,
    size: f64,
    is_uploaded: bool,
    storage_metadata: Option<Value>,
    created_by: Option<String>,
    updated_by: Option<String>,
    user: Option<String>,
    workspace: Option<String>,
    draft_issue: Option<String>,
    project: Option<String>,
    issue: Option<String>,
    comment: Option<String>,
    page: Option<String>,
}

fn decode_attachment(row: &sqlx::postgres::PgRow, tz: &Tz) -> Result<DecodedAttachment, Denial> {
    Ok(DecodedAttachment {
        id: row_uuid(row, "id")?.to_string(),
        created_at: crate::serializer::render_datetime_in(&row_datetime(row, "created_at")?, tz),
        updated_at: crate::serializer::render_datetime_in(&row_datetime(row, "updated_at")?, tz),
        deleted_at: row_datetime_opt(row, "deleted_at")?
            .map(|dt| crate::serializer::render_datetime_in(&dt, tz)),
        attributes: row.try_get("attributes").map_err(|_| Denial::ServerError)?,
        asset: row_string(row, "asset")?,
        entity_type: row_string_opt(row, "entity_type")?,
        entity_identifier: row_string_opt(row, "entity_identifier")?,
        is_deleted: row.try_get("is_deleted").map_err(|_| Denial::ServerError)?,
        is_archived: row
            .try_get("is_archived")
            .map_err(|_| Denial::ServerError)?,
        external_id: row_string_opt(row, "external_id")?,
        external_source: row_string_opt(row, "external_source")?,
        size: row.try_get("size").map_err(|_| Denial::ServerError)?,
        is_uploaded: row
            .try_get("is_uploaded")
            .map_err(|_| Denial::ServerError)?,
        storage_metadata: row
            .try_get("storage_metadata")
            .map_err(|_| Denial::ServerError)?,
        created_by: row_uuid_opt(row, "created_by_id")?.map(|id| id.to_string()),
        updated_by: row_uuid_opt(row, "updated_by_id")?.map(|id| id.to_string()),
        user: row_uuid_opt(row, "user_id")?.map(|id| id.to_string()),
        workspace: row_uuid_opt(row, "workspace_id")?.map(|id| id.to_string()),
        draft_issue: row_uuid_opt(row, "draft_issue_id")?.map(|id| id.to_string()),
        project: row_uuid_opt(row, "project_id")?.map(|id| id.to_string()),
        issue: row_uuid_opt(row, "issue_id")?.map(|id| id.to_string()),
        comment: row_uuid_opt(row, "comment_id")?.map(|id| id.to_string()),
        page: row_uuid_opt(row, "page_id")?.map(|id| id.to_string()),
    })
}

/// Render one attachment row through [`render_attachment`] (no
/// `fields=`/`expand=` — neither call site passes them): every shape
/// error 500s, including a non-finite `size` (DRF strict renderer).
fn render_attachment_value(decoded: &DecodedAttachment) -> Result<Value, Denial> {
    let size = decoded.size;
    let row = AttachmentRow {
        id: &decoded.id,
        created_at: &decoded.created_at,
        updated_at: &decoded.updated_at,
        deleted_at: decoded.deleted_at.as_deref(),
        attributes: &decoded.attributes,
        asset: &decoded.asset,
        entity_type: decoded.entity_type.as_deref(),
        entity_identifier: decoded.entity_identifier.as_deref(),
        is_deleted: decoded.is_deleted,
        is_archived: decoded.is_archived,
        external_id: decoded.external_id.as_deref(),
        external_source: decoded.external_source.as_deref(),
        size,
        is_uploaded: decoded.is_uploaded,
        storage_metadata: decoded.storage_metadata.as_ref(),
        created_by: decoded.created_by.as_deref(),
        updated_by: decoded.updated_by.as_deref(),
        user: decoded.user.as_deref(),
        workspace: decoded.workspace.as_deref(),
        draft_issue: decoded.draft_issue.as_deref(),
        project: decoded.project.as_deref(),
        issue: decoded.issue.as_deref(),
        comment: decoded.comment.as_deref(),
        page: decoded.page.as_deref(),
    };
    let out = render_attachment(&AttachmentRepresentationInput {
        row: &row,
        fields: None,
        expand: &[],
        expansions: &[],
    })
    .map_err(|_| Denial::ServerError)?;
    Ok(Value::Object(out))
}

/// Render one attachment row for `current_instance` dumps: unlike
/// responses, the confirm path dumps with plain `json.dumps`
/// (`allow_nan`), so a non-finite `size` becomes a sentinel string
/// (swapped for the literal by [`finalize_nonfinite`] on the dumped
/// text) instead of 500ing.
fn render_attachment_for_dump(decoded: &DecodedAttachment) -> Result<Value, Denial> {
    let mut size = decoded.size;
    let mut nonfinite: Option<String> = None;
    if !size.is_finite() {
        nonfinite = Some(format!("{NONFINITE_SENTINEL}{}", py_float_literal(size)));
        size = 0.0;
    }
    let row = AttachmentRow {
        id: &decoded.id,
        created_at: &decoded.created_at,
        updated_at: &decoded.updated_at,
        deleted_at: decoded.deleted_at.as_deref(),
        attributes: &decoded.attributes,
        asset: &decoded.asset,
        entity_type: decoded.entity_type.as_deref(),
        entity_identifier: decoded.entity_identifier.as_deref(),
        is_deleted: decoded.is_deleted,
        is_archived: decoded.is_archived,
        external_id: decoded.external_id.as_deref(),
        external_source: decoded.external_source.as_deref(),
        size,
        is_uploaded: decoded.is_uploaded,
        storage_metadata: decoded.storage_metadata.as_ref(),
        created_by: decoded.created_by.as_deref(),
        updated_by: decoded.updated_by.as_deref(),
        user: decoded.user.as_deref(),
        workspace: decoded.workspace.as_deref(),
        draft_issue: decoded.draft_issue.as_deref(),
        project: decoded.project.as_deref(),
        issue: decoded.issue.as_deref(),
        comment: decoded.comment.as_deref(),
        page: decoded.page.as_deref(),
    };
    let mut out = render_attachment(&AttachmentRepresentationInput {
        row: &row,
        fields: None,
        expand: &[],
        expansions: &[],
    })
    .map_err(|_| Denial::ServerError)?;
    if let Some(sentinel) = &nonfinite {
        out.insert("size".to_owned(), Value::String(sentinel.clone()));
    }
    Ok(Value::Object(out))
}

/// `FileAsset.asset_url` (`db/models/asset.py:80-100`) for an asset id:
/// static types render `/api/assets/v2/static/<id>/`, attachments and
/// description assets join their workspace slug. A missing row (or slug)
/// renders null like `getattr` on a dead FK (the `v1_projects`
/// `file_asset_url` precedent).
/// Decoded `file_assets` row for [`file_asset_url`].
type AssetUrlLookup = (Option<String>, Option<Uuid>, Option<Uuid>, Option<Uuid>);

async fn file_asset_url(pool: &PgPool, asset_id: &Uuid) -> Result<Option<String>, Denial> {
    let row: Option<AssetUrlLookup> =
        sqlx::query_as(
            r#"SELECT fa."entity_type", fa."workspace_id", fa."project_id", fa."issue_id" FROM "file_assets" fa WHERE fa."id" = $1"#,
        )
        .bind(asset_id)
        .fetch_optional(pool)
        .await
        .map_err(|error| db_error(error, "file-asset-url"))?;
    let Some((entity_type, workspace_id, project_id, issue_id)) = row else {
        return Ok(None);
    };
    async fn workspace_slug(
        pool: &PgPool,
        workspace_id: Option<Uuid>,
    ) -> Result<Option<String>, Denial> {
        match workspace_id {
            Some(id) => sqlx::query_scalar(r#"SELECT "slug" FROM "workspaces" WHERE "id" = $1"#)
                .bind(id)
                .fetch_optional(pool)
                .await
                .map_err(|error| db_error(error, "file-asset-slug"))
                .map(|slug: Option<Option<String>>| slug.flatten()),
            None => Ok(None),
        }
    }
    match entity_type.as_deref() {
        Some("WORKSPACE_LOGO")
        | Some("USER_AVATAR")
        | Some("USER_COVER")
        | Some("PROJECT_COVER") => Ok(Some(format!("/api/assets/v2/static/{asset_id}/"))),
        Some("ISSUE_ATTACHMENT") => {
            let slug = workspace_slug(pool, workspace_id).await?;
            Ok(slug.map(|s| {
                format!(
                    "/api/assets/v2/workspaces/{s}/projects/{}/issues/{}/attachments/{asset_id}/",
                    project_id.map(|v| v.to_string()).unwrap_or_default(),
                    issue_id.map(|v| v.to_string()).unwrap_or_default(),
                )
            }))
        }
        Some("ISSUE_DESCRIPTION")
        | Some("COMMENT_DESCRIPTION")
        | Some("PAGE_DESCRIPTION")
        | Some("DRAFT_ISSUE_DESCRIPTION") => {
            let slug = workspace_slug(pool, workspace_id).await?;
            Ok(slug.map(|s| {
                format!(
                    "/api/assets/v2/workspaces/{s}/projects/{}/{asset_id}/",
                    project_id.map(|v| v.to_string()).unwrap_or_default(),
                )
            }))
        }
        _ => Ok(None),
    }
}

/// The POST response `asset_url`: the `ISSUE_ATTACHMENT` branch of
/// `asset_url` with the path slug and rewritten project id (no lookup —
/// the workspace row was just loaded).
fn issue_attachment_asset_url(
    slug: &str,
    project_id: &Uuid,
    issue_id: &Uuid,
    asset_id: &Uuid,
) -> String {
    format!("/api/assets/v2/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/attachments/{asset_id}/")
}

/// `expand=workspace`: `WorkspaceLiteSerializer` (`workspace.py:10-21`):
/// `{"name", "slug", "id"}` in field order. A missing row renders null
/// (the `v1_projects` `expand_workspace` precedent).
async fn expand_workspace(pool: &PgPool, workspace_id: &Uuid) -> Result<Value, Denial> {
    let row: Option<(String, String)> =
        sqlx::query_as(r#"SELECT "name", "slug" FROM "workspaces" WHERE "id" = $1"#)
            .bind(workspace_id)
            .fetch_optional(pool)
            .await
            .map_err(|error| db_error(error, "expand-workspace"))?;
    Ok(match row {
        Some((name, slug)) => {
            let mut map = Map::with_capacity(3);
            map.insert("name".to_owned(), Value::String(name));
            map.insert("slug".to_owned(), Value::String(slug));
            map.insert("id".to_owned(), Value::String(workspace_id.to_string()));
            Value::Object(map)
        }
        None => Value::Null,
    })
}

/// Decoded user row for [`expand_actor`]: names, nullable `email`,
/// `COALESCE`d avatar, optional avatar asset.
type ExpandUserLookup = (String, String, Option<String>, String, Option<Uuid>);

/// `expand=actor`: `UserLiteSerializer` (`user.py:13-38`) via the D-19
/// `user_lite_to_representation` kernel. A missing row renders null.
async fn expand_actor(pool: &PgPool, user_id: &Uuid) -> Result<Value, Denial> {
    let row: Option<ExpandUserLookup> = sqlx::query_as(
        r#"SELECT "first_name", "last_name", "email", COALESCE("avatar", ''), "avatar_asset_id" FROM "users" WHERE "id" = $1"#,
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| db_error(error, "expand-actor"))?;
    let Some((first_name, last_name, email, avatar, avatar_asset)) = row else {
        return Ok(Value::Null);
    };
    let avatar_url = match avatar_asset {
        Some(id) => file_asset_url(pool, &id).await?,
        None => None,
    };
    let avatar_url = pidash_services::v1_projects::ser_collab::resolve_avatar_url(
        avatar_asset.is_some(),
        avatar_url.as_deref(),
        &avatar,
    );
    let display_name: Option<String> =
        sqlx::query_scalar(r#"SELECT "display_name" FROM "users" WHERE "id" = $1"#)
            .bind(user_id)
            .fetch_optional(pool)
            .await
            .map_err(|error| db_error(error, "expand-actor-name"))?
            .flatten();
    let id = user_id.to_string();
    let display_name = display_name.unwrap_or_default();
    let row = pidash_services::v1_projects::ser_collab::UserLiteRow {
        id: &id,
        first_name: &first_name,
        last_name: &last_name,
        email: email.as_deref(),
        avatar: &avatar,
        avatar_url,
        display_name: &display_name,
    };
    let view = pidash_services::v1_projects::ser_collab::user_lite_to_representation(&row);
    serde_json::to_value(&view).map_err(|_| Denial::ServerError)
}

/// `expand=project`: `ProjectLiteSerializer` (`project.py:354-377`): `id`,
/// `identifier`, `name`, `cover_image`, `icon_prop`, `emoji`,
/// `description`, `is_default`, `cover_image_url` in field order. A
/// missing row renders null.
async fn expand_project(pool: &PgPool, project_id: &Uuid) -> Result<Value, Denial> {
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(
        r#"SELECT "id", "identifier", "name", "cover_image", "icon_prop", "emoji", "description", "is_default", "cover_image_asset_id" FROM "projects" WHERE "id" = $1"#,
    )
    .bind(project_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| db_error(error, "expand-project"))?;
    let Some(row) = row else {
        return Ok(Value::Null);
    };
    let cover_asset: Option<Uuid> = row_uuid_opt(&row, "cover_image_asset_id")?;
    let cover_text: Option<String> = row_string_opt(&row, "cover_image")?;
    // `Project.cover_image_url` (`db/models/project.py:176-185`): the
    // asset URL wins, then the stored text, else null.
    let cover_url = match cover_asset {
        Some(id) => file_asset_url(pool, &id).await?,
        None => None,
    };
    let cover_url = cover_url.or_else(|| cover_text.clone().filter(|text| !text.is_empty()));
    let icon_prop: Option<Value> = row.try_get("icon_prop").map_err(|_| Denial::ServerError)?;
    let mut map = Map::with_capacity(9);
    map.insert(
        "id".to_owned(),
        Value::String(row_uuid(&row, "id")?.to_string()),
    );
    map.insert(
        "identifier".to_owned(),
        Value::String(row_string(&row, "identifier")?),
    );
    map.insert("name".to_owned(), Value::String(row_string(&row, "name")?));
    map.insert(
        "cover_image".to_owned(),
        cover_text.map(Value::String).unwrap_or(Value::Null),
    );
    map.insert("icon_prop".to_owned(), icon_prop.unwrap_or(Value::Null));
    map.insert(
        "emoji".to_owned(),
        row_string_opt(&row, "emoji")?
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    map.insert(
        "description".to_owned(),
        Value::String(row_string(&row, "description")?),
    );
    map.insert(
        "is_default".to_owned(),
        Value::Bool(row.try_get("is_default").map_err(|_| Denial::ServerError)?),
    );
    map.insert(
        "cover_image_url".to_owned(),
        cover_url.map(Value::String).unwrap_or(Value::Null),
    );
    Ok(Value::Object(map))
}

/// `expand=issue`: the full `IssueSerializer` (`serializers/issue.py:436-494`
/// over `render_issue`): assignee/label id lists in queryset
/// (`-created_at`) order, the blocker summary (single payloads always
/// query), no viewer relations (the nested construction carries no
/// context). A missing row renders null.
async fn expand_issue(
    pool: &PgPool,
    workspace_slug: &str,
    issue_id: &Uuid,
    tz: &Tz,
    web_base_url: Option<&str>,
) -> Result<Value, Denial> {
    use pidash_services::v1_work_items::shape_issue as shape;
    // Unscoped: `select_related("issue")` joins the issue row without the
    // soft-deletion scope, so a soft-deleted issue still renders (with its
    // `deleted_at`) instead of collapsing to null (PIDASHCONV-789#4,
    // live-probed).
    let row: Option<sqlx::postgres::PgRow> =
        sqlx::query(r#"SELECT * FROM "issues" WHERE "id" = $1"#)
            .bind(issue_id)
            .fetch_optional(pool)
            .await
            .map_err(|error| db_error(error, "expand-issue"))?;
    let Some(row) = row else {
        return Ok(Value::Null);
    };
    let id = row_uuid(&row, "id")?.to_string();
    let created_at = crate::serializer::render_datetime_in(&row_datetime(&row, "created_at")?, tz);
    let updated_at = crate::serializer::render_datetime_in(&row_datetime(&row, "updated_at")?, tz);
    let deleted_at = row_datetime_opt(&row, "deleted_at")?
        .map(|dt| crate::serializer::render_datetime_in(&dt, tz));
    let completed_at = row_datetime_opt(&row, "completed_at")?
        .map(|dt| crate::serializer::render_datetime_in(&dt, tz));
    let archived_at = row_datetime_opt(&row, "archived_at")?
        .map(|dt| crate::serializer::render_datetime_in(&dt, tz));
    let point: Option<i32> = row.try_get("point").map_err(|_| Denial::ServerError)?;
    let name = row_string(&row, "name")?;
    let description_html: Option<String> = row
        .try_get("description_html")
        .map_err(|_| Denial::ServerError)?;
    let description_binary: Option<Vec<u8>> = row
        .try_get("description_binary")
        .map_err(|_| Denial::ServerError)?;
    let priority = row_string(&row, "priority")?;
    let complexity_score: i32 = row
        .try_get("complexity_score")
        .map_err(|_| Denial::ServerError)?;
    let start_date: Option<chrono::NaiveDate> =
        row.try_get("start_date").map_err(|_| Denial::ServerError)?;
    let target_date: Option<chrono::NaiveDate> = row
        .try_get("target_date")
        .map_err(|_| Denial::ServerError)?;
    let sequence_id: i32 = row
        .try_get("sequence_id")
        .map_err(|_| Denial::ServerError)?;
    let sort_order: f64 = row.try_get("sort_order").map_err(|_| Denial::ServerError)?;
    let is_draft: bool = row.try_get("is_draft").map_err(|_| Denial::ServerError)?;
    let external_source = row_string_opt(&row, "external_source")?;
    let external_id = row_string_opt(&row, "external_id")?;
    let git_work_branch: Option<String> = row
        .try_get("git_work_branch")
        .map_err(|_| Denial::ServerError)?;
    let created_via = row_string_opt(&row, "created_via")?;
    let agent_executor = row_string_opt(&row, "agent_executor")?;
    let created_by = row_uuid_opt(&row, "created_by_id")?.map(|id| id.to_string());
    let updated_by = row_uuid_opt(&row, "updated_by_id")?.map(|id| id.to_string());
    let project = row_uuid_opt(&row, "project_id")?.map(|id| id.to_string());
    let workspace = row_uuid_opt(&row, "workspace_id")?.map(|id| id.to_string());
    let parent = row_uuid_opt(&row, "parent_id")?.map(|id| id.to_string());
    let state = row_uuid_opt(&row, "state_id")?.map(|id| id.to_string());
    let estimate_point = row_uuid_opt(&row, "estimate_point_id")?.map(|id| id.to_string());
    let assigned_pod = row_uuid_opt(&row, "assigned_pod_id")?.map(|id| id.to_string());
    let type_id = row_uuid_opt(&row, "type_id")?.map(|id| id.to_string());
    // `get_url` (`serializers/issue.py:423-434`): present and absolute, or
    // the key is omitted — never null, never relative.
    let url = match (web_base_url, project.as_deref(), sequence_id) {
        (Some(base), Some(_), _) => {
            let identifier: Option<String> =
                sqlx::query_scalar(r#"SELECT "identifier" FROM "projects" WHERE "id" = $1"#)
                    .bind(row_uuid_opt(&row, "project_id")?.expect("project"))
                    .fetch_optional(pool)
                    .await
                    .map_err(|error| db_error(error, "expand-issue-url"))?
                    .flatten();
            match identifier {
                Some(identifier) => shape::issue_url(
                    Some(base),
                    Some(workspace_slug),
                    Some(&identifier),
                    Some(i64::from(sequence_id)),
                ),
                None => None,
            }
        }
        _ => None,
    };
    let start_date = start_date.map(|d| d.to_string());
    let target_date = target_date.map(|d| d.to_string());
    let issue_row = shape::IssueRow {
        id: &id,
        type_id: type_id.as_deref(),
        url,
        created_at: &created_at,
        updated_at: &updated_at,
        deleted_at: deleted_at.as_deref(),
        point: point.map(i64::from),
        name: &name,
        description_html: description_html.as_deref().unwrap_or(""),
        description_binary: description_binary.as_deref(),
        priority: &priority,
        complexity_score: i64::from(complexity_score),
        start_date: start_date.as_deref(),
        target_date: target_date.as_deref(),
        sequence_id: i64::from(sequence_id),
        sort_order,
        completed_at: completed_at.as_deref(),
        archived_at: archived_at.as_deref(),
        is_draft,
        external_source: external_source.as_deref(),
        external_id: external_id.as_deref(),
        git_work_branch: git_work_branch.as_deref().unwrap_or(""),
        created_via: created_via.as_deref(),
        agent_executor: agent_executor.as_deref(),
        created_by: created_by.as_deref(),
        updated_by: updated_by.as_deref(),
        project: project.as_deref().unwrap_or(""),
        workspace: workspace.as_deref().unwrap_or(""),
        parent: parent.as_deref(),
        state: state.as_deref(),
        estimate_point: estimate_point.as_deref(),
        assigned_pod: assigned_pod.as_deref(),
    };
    let assignee_ids = fetch_assignee_ids(pool, issue_id).await?;
    let assignee_refs: Vec<&str> = assignee_ids.iter().map(String::as_str).collect();
    let label_ids = fetch_label_ids(pool, issue_id).await?;
    let label_refs: Vec<&str> = label_ids.iter().map(String::as_str).collect();
    let rows = fetch_blocker_rows(pool, issue_id).await?;
    let blocked_by: Vec<shape::SummaryItem<'_>> = rows
        .blocked_by
        .iter()
        .map(|(identifier, state, group)| shape::SummaryItem {
            identifier: identifier.clone(),
            state: state.as_deref(),
            state_group: group.as_deref(),
        })
        .collect();
    let blocking: Vec<shape::SummaryItem<'_>> = rows
        .blocking
        .iter()
        .map(|(identifier, state, group)| shape::SummaryItem {
            identifier: identifier.clone(),
            state: state.as_deref(),
            state_group: group.as_deref(),
        })
        .collect();
    let blockers = shape::BlockerSummary {
        blocked_by,
        blocking,
        has_open_blockers: rows.has_open_blockers,
    };
    let out = shape::render_issue(&shape::RepresentationInput {
        row: &issue_row,
        fields: None,
        expand: &[],
        is_list: false,
        assignee_ids: &assignee_refs,
        assignee_rows: &[],
        label_ids: &label_refs,
        expanded_labels: &[],
        blockers: Some(&blockers),
        relations: None,
        expansions: &[],
    })
    .map_err(|_| Denial::ServerError)?;
    Ok(Value::Object(out))
}

/// `IssueAssignee` ids in queryset (`-created_at`) order
/// (`serializers/issue.py:453-456`).
async fn fetch_assignee_ids(pool: &PgPool, issue_id: &Uuid) -> Result<Vec<String>, Denial> {
    let ids: Vec<Uuid> = sqlx::query_scalar(
        r#"SELECT "assignee_id" FROM "issue_assignees" WHERE "issue_id" = $1 AND "deleted_at" IS NULL ORDER BY "created_at" DESC"#,
    )
    .bind(issue_id)
    .fetch_all(pool)
    .await
    .map_err(|error| db_error(error, "expand-assignees"))?;
    Ok(ids.iter().map(Uuid::to_string).collect())
}

/// `IssueLabel` ids in queryset (`-created_at`) order
/// (`serializers/issue.py:466-468`).
async fn fetch_label_ids(pool: &PgPool, issue_id: &Uuid) -> Result<Vec<String>, Denial> {
    let ids: Vec<Uuid> = sqlx::query_scalar(
        r#"SELECT "label_id" FROM "issue_labels" WHERE "issue_id" = $1 AND "deleted_at" IS NULL ORDER BY "created_at" DESC"#,
    )
    .bind(issue_id)
    .fetch_all(pool)
    .await
    .map_err(|error| db_error(error, "expand-labels"))?;
    Ok(ids.iter().map(Uuid::to_string).collect())
}

/// One blocker-target row: id, project identifier, sequence, state name,
/// state group.
type BlockerTargetRow = (Uuid, String, i32, Option<String>, Option<String>);

/// Sort key per `_summary_list`: resolved flag, identifier, sequence,
/// then the rendered triple.
type SummarySortKey = (i64, String, i64, Option<String>, Option<String>);

/// Owned `relations_summary` rows: `(identifier, state, state_group)`
/// per direction plus the uncapped open flag. The shape borrows these;
/// the caller holds them across the render.
struct BlockerRows {
    blocked_by: Vec<(String, Option<String>, Option<String>)>,
    blocking: Vec<(String, Option<String>, Option<String>)>,
    has_open_blockers: bool,
}

/// `relations_summary` (`orchestration/blockers.py:186-198`): per-direction
/// `{identifier, state, state_group}` lists, open first, capped at 100
/// (`SUMMARY_LIMIT`), plus the uncapped `has_open_blockers`. Targets are
/// live `issue_objects` rows in the relation's workspace
/// (`blockers.py:95-104`): not deleted/archived/draft, project live, and
/// — via `exclude()`'s INNER JOINs, verified against Django — a non-null
/// non-triage state.
async fn fetch_blocker_rows(pool: &PgPool, issue_id: &Uuid) -> Result<BlockerRows, Denial> {
    async fn direction(
        pool: &PgPool,
        issue_id: &Uuid,
        blocked_by: bool,
    ) -> Result<Vec<(String, Option<String>, Option<String>)>, Denial> {
        // `_blocked_by_edges` / `_blocking_edges` (`blockers.py:64-107`):
        // forward `blocked_by` rows plus stored-reversed `blocking` rows.
        // A target stored under both names appears once (one issue row
        // matches the Python OR).
        let mut seen = std::collections::HashSet::new();
        let mut items: Vec<SummarySortKey> = Vec::new();
        for relation_type in ["blocked_by", "blocking"] {
            let (anchor_col, target_col) = if blocked_by {
                match relation_type {
                    "blocked_by" => ("issue_id", "related_issue_id"),
                    _ => ("related_issue_id", "issue_id"),
                }
            } else {
                match relation_type {
                    "blocked_by" => ("related_issue_id", "issue_id"),
                    _ => ("issue_id", "related_issue_id"),
                }
            };
            let rows: Vec<BlockerTargetRow> = sqlx::query_as(&format!(
                r#"SELECT t."id", p."identifier", t."sequence_id", s."name", s."group"
                   FROM "issue_relations" r
                   INNER JOIN "issues" t ON t."id" = r."{target_col}"
                   INNER JOIN "states" s ON s."id" = t."state_id"
                   INNER JOIN "projects" p ON p."id" = t."project_id"
                   WHERE r."deleted_at" IS NULL AND r."issue_id" <> r."related_issue_id"
                     AND r."{anchor_col}" = $1 AND r."relation_type" = $2
                     AND t."deleted_at" IS NULL AND t."archived_at" IS NULL AND t."is_draft" = FALSE
                     AND t."workspace_id" = r."workspace_id"
                     AND s."group" <> 'triage' AND p."archived_at" IS NULL"#,
            ))
            .bind(issue_id)
            .bind(relation_type)
            .fetch_all(pool)
            .await
            .map_err(|error| db_error(error, "expand-blockers"))?;
            for (id, identifier, sequence_id, state, group) in rows {
                if seen.insert(id) {
                    let resolved =
                        i64::from(matches!(group.as_deref(), Some("completed" | "cancelled")));
                    items.push((resolved, identifier, i64::from(sequence_id), state, group));
                }
            }
        }
        // `_summary_list` (`blockers.py:173-183`): open first, then
        // identifier, then sequence, capped at `SUMMARY_LIMIT`.
        items.sort_by(|a, b| (a.0, &a.1, a.2).cmp(&(b.0, &b.1, b.2)));
        items.truncate(100);
        Ok(items
            .into_iter()
            .map(|(_, identifier, sequence_id, state, group)| {
                (format!("{identifier}-{sequence_id}"), state, group)
            })
            .collect())
    }
    let blocked_by = direction(pool, issue_id, true).await?;
    let blocking = direction(pool, issue_id, false).await?;
    // `has_open_blockers` (`blockers.py:143-144`): `_open()` over the same
    // live target set — INNER JOIN states, group outside closed.
    let has_open_blockers: bool = sqlx::query_scalar(
        r#"SELECT EXISTS(
             SELECT 1 FROM "issue_relations" r
             INNER JOIN "issues" t ON t."id" = CASE WHEN r."relation_type" = 'blocked_by' THEN r."related_issue_id" ELSE r."issue_id" END
             INNER JOIN "states" s ON s."id" = t."state_id"
             INNER JOIN "projects" p ON p."id" = t."project_id"
             WHERE r."deleted_at" IS NULL AND r."issue_id" <> r."related_issue_id"
               AND t."deleted_at" IS NULL AND t."archived_at" IS NULL AND t."is_draft" = FALSE
               AND t."workspace_id" = r."workspace_id"
               AND s."group" <> 'triage' AND p."archived_at" IS NULL
               AND (
                 (r."issue_id" = $1 AND r."relation_type" = 'blocked_by')
                 OR (r."related_issue_id" = $1 AND r."relation_type" = 'blocking')
               )
               AND s."group" NOT IN ('completed', 'cancelled')
           )"#,
    )
    .bind(issue_id)
    .fetch_one(pool)
    .await
    .map_err(|error| db_error(error, "expand-open-blockers"))?;
    Ok(BlockerRows {
        blocked_by,
        blocking,
        has_open_blockers,
    })
}

/// `json.dumps(value)` with CPython defaults (`", "`/`": "` separators,
/// `ensure_ascii=True`), as the confirm path dumps `current_instance`
/// (`views/issue.py:2637` over `DjangoJSONEncoder` — the input is already
/// rendered primitives, so no encoder hook fires). The `app_project`
/// `cpython_dumps` precedent.
fn cpython_dumps(value: &Value) -> String {
    let mut out = String::new();
    cpython_write(&mut out, value);
    out
}

fn cpython_write(out: &mut String, value: &Value) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Number(number) => out.push_str(&number.to_string()),
        Value::String(text) => cpython_write_string(out, text),
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                cpython_write(out, item);
            }
            out.push(']');
        }
        Value::Object(map) => {
            out.push('{');
            for (index, (key, item)) in map.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                cpython_write_string(out, key);
                out.push_str(": ");
                cpython_write(out, item);
            }
            out.push('}');
        }
    }
}

fn cpython_write_string(out: &mut String, text: &str) {
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0C}' => out.push_str("\\f"),
            ch if (ch as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", ch as u32));
            }
            ch if (ch as u32) < 0x7F => out.push(ch),
            ch => {
                let code = ch as u32;
                if code > 0xFFFF {
                    let code = code - 0x1_0000;
                    out.push_str(&format!(
                        "\\u{:04x}\\u{:04x}",
                        0xD800 + (code >> 10),
                        0xDC00 + (code & 0x3FF)
                    ));
                } else {
                    out.push_str(&format!("\\u{code:04x}"));
                }
            }
        }
    }
    out.push('"');
}

/// Python `str(value)` for a request-data scalar, as the `f"{...}-{name}"`
/// key factory stringifies a non-string `name` (`:2357`): bools spell
/// `True`/`False`, numbers their JSON spelling, strings pass through.
/// Containers spell their `repr` (single quotes, `", "`/`": "`).
fn py_str(value: &Value) -> String {
    match value {
        Value::Null => "None".to_owned(),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::Number(number) => number.to_string(),
        Value::String(text) => text.clone(),
        Value::Array(items) => {
            let inner: Vec<String> = items.iter().map(py_repr).collect();
            format!("[{}]", inner.join(", "))
        }
        Value::Object(map) => {
            let inner: Vec<String> = map
                .iter()
                .map(|(key, item)| {
                    format!(
                        "{}: {}",
                        py_repr(&Value::String(key.clone())),
                        py_repr(item)
                    )
                })
                .collect();
            format!("{{{}}}", inner.join(", "))
        }
    }
}

/// Python `repr(value)` for request data nested under a container `name`.
fn py_repr(value: &Value) -> String {
    match value {
        Value::String(text) => {
            // `repr` prefers single quotes, doubling backslashes and
            // escaping the quote in use plus the C0 controls.
            let mut out = String::with_capacity(text.len() + 2);
            let quote = if text.contains('\'') && !text.contains('"') {
                '"'
            } else {
                '\''
            };
            out.push(quote);
            for ch in text.chars() {
                match ch {
                    '\\' => out.push_str("\\\\"),
                    '\n' => out.push_str("\\n"),
                    '\r' => out.push_str("\\r"),
                    '\t' => out.push_str("\\t"),
                    ch if ch == quote => {
                        out.push('\\');
                        out.push(ch);
                    }
                    ch if (ch as u32) < 0x20 => {
                        out.push_str(&format!("\\x{:02x}", ch as u32));
                    }
                    ch => out.push(ch),
                }
            }
            out.push(quote);
            out
        }
        Value::Array(items) => {
            let inner: Vec<String> = items.iter().map(py_repr).collect();
            format!("[{}]", inner.join(", "))
        }
        Value::Object(map) => {
            let inner: Vec<String> = map
                .iter()
                .map(|(key, item)| {
                    format!(
                        "{}: {}",
                        py_repr(&Value::String(key.clone())),
                        py_repr(item)
                    )
                })
                .collect();
            format!("{{{}}}", inner.join(", "))
        }
        other => py_str(other),
    }
}

/// A request-data number as CPython spells the parsed float: JSON
/// `1e2`/`1.50`/`0.5e1` arrive as floats, so the policy, the response
/// echo and the row all carry `100.0`/`1.5`/`5.0` — never the request's
/// literal. Only float-spelled literals (a `.`/`e`/`E` marker; huge ints
/// have an `f64` too, so the marker check is load-bearing) with a finite
/// value normalize; ints and overflow spellings (`1e999`,
/// PIDASHCONV-782) pass through verbatim. (PIDASHCONV-789#2,
/// live-probed.)
fn normalize_float_spelling(value: &Value) -> Value {
    let Value::Number(number) = value else {
        return value.clone();
    };
    if !number
        .to_string()
        .bytes()
        .any(|b| b == b'.' || b == b'e' || b == b'E')
    {
        return value.clone();
    }
    let Some(float) = number.as_f64().filter(|f| f.is_finite()) else {
        return value.clone();
    };
    serde_json::from_str::<Value>(&crate::paginator::py_float_str(float))
        .unwrap_or_else(|_| value.clone())
}

/// A `size_limit` number as the presigned policy spells it: bools as
/// JSON literals, numbers verbatim (float spellings are normalized to
/// CPython's `repr` upstream by [`normalize_float_spelling`]).
fn policy_number(value: &Value) -> String {
    match value {
        Value::Bool(true) => "true".to_owned(),
        Value::Bool(false) => "false".to_owned(),
        Value::Number(number) => number.to_string(),
        _ => "null".to_owned(),
    }
}

// ---------------------------------------------------------------------------
// SigV4 presigning (offline; mirrors `S3Storage` + botocore)
// ---------------------------------------------------------------------------

/// Request scheme for MinIO-mode signing: `X-Forwarded-Proto` when the
/// proxy sets it, else `http` (Django's `request.scheme` default).
fn scheme_of(headers: &HeaderMap) -> String {
    headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .map(|v| v.trim().to_ascii_lowercase())
        .filter(|v| v == "http" || v == "https")
        .unwrap_or_else(|| "http".to_owned())
}

fn host_of(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
}

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

/// A resolved S3 endpoint for signing: URL base, signed `host`
/// header, base path and addressing style (botocore's S3
/// endpoint-ruleset output for our inputs —
/// `endpoint-rule-set-1.json`, live-probed).
#[derive(Debug, Clone, PartialEq, Eq)]
struct ResolvedEndpoint {
    /// `{scheme}://{authority}`: scheme lowercased, authority verbatim
    /// (the base path travels separately in `base_path`).
    url_base: String,
    /// Signed `host` header (`auth._host_from_url`): lowercased (a
    /// `%zone` tail verbatim), no userinfo, default/empty port stripped,
    /// IPv6 re-bracketed.
    signed_host: String,
    /// Normalized base path: `""` or `"/base/path"` (no trailing slash).
    base_path: String,
    /// The bucket travels in the path (custom/MinIO endpoints always;
    /// AWS endpoints for non-virtual-hostable names).
    path_style: bool,
}

/// `aws.partition` over `partitions.json` (`endpoint_provider.py`):
/// explicit-region membership, else `regionRegex` in file order, else
/// the default (`aws`). Every explicit region matches its own regex, so
/// the regex pass alone is exact. Returns the partition's `dnsSuffix`
/// and whether it is `aws` (`use_global_endpoint` presigns global only
/// there). FIPS/`s3-external-1` pseudo-regions match nothing and take
/// the default — their special endpoints/scopes are a known residual
/// (see the PR).
fn partition_for_region(region: &str) -> (&'static str, bool) {
    const PARTITIONS: &[(&[&str], &str)] = &[
        (
            &["us", "eu", "ap", "sa", "ca", "me", "af", "il"],
            "amazonaws.com",
        ),
        (&["cn"], "amazonaws.com.cn"),
        (&["us-gov"], "amazonaws.com"),
        (&["us-iso"], "c2s.ic.gov"),
        (&["us-isob"], "sc2s.sgov.gov"),
        (&["eu-isoe"], "cloud.adc-e.uk"),
        (&["us-isof"], "csp.hci.ic.gov"),
    ];
    // `^(prefix)-\w+-\d+$` (`\w`/`\d` ASCII: regions pass the host-label
    // gate first).
    fn region_matches(prefixes: &[&str], region: &str) -> bool {
        prefixes.iter().any(|prefix| {
            region
                .strip_prefix(prefix)
                .and_then(|rest| rest.strip_prefix('-'))
                .is_some_and(|rest| {
                    let mut parts = rest.rsplitn(2, '-');
                    let digits = parts.next().unwrap_or_default();
                    let middle = parts.next().unwrap_or_default();
                    !middle.is_empty()
                        && middle
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b == b'_')
                        && !digits.is_empty()
                        && digits.bytes().all(|b| b.is_ascii_digit())
                })
        })
    }
    for (index, (prefixes, suffix)) in PARTITIONS.iter().enumerate() {
        if region_matches(prefixes, region) {
            return (suffix, index == 0);
        }
    }
    ("amazonaws.com", true)
}

/// `^(?:[0-9]{1,3}\.){3}[0-9]{1,3}$` (`botocore/compat.py`): range-loose
/// on purpose (also the `ls32` IPv4 tail below).
fn is_ipv4_literal(value: &str) -> bool {
    let mut parts = value.split('.');
    for _ in 0..4 {
        let Some(part) = parts.next() else {
            return false;
        };
        if part.is_empty() || part.len() > 3 || !part.bytes().all(|b| b.is_ascii_digit()) {
            return false;
        }
    }
    parts.next().is_none()
}

/// One `RFC 1123` host label (`^(?!-)[a-zA-Z\d-]{1,63}(?<!-)$`,
/// ASCII-only — callers gate non-ASCII out first).
fn is_host_label(label: &str) -> bool {
    if label.is_empty() || label.len() > 63 {
        return false;
    }
    let bytes = label.as_bytes();
    if bytes[0] == b'-' || bytes[bytes.len() - 1] == b'-' {
        return false;
    }
    bytes
        .iter()
        .all(|b| b.is_ascii_alphanumeric() || *b == b'-')
}

/// `isVirtualHostableS3Bucket(bucket, False)`
/// (`endpoint_provider.py`): 3+ chars, all lowercase, not IPv4-shaped,
/// no dots, and a valid host label. The length reads the RAW name (so
/// `ab\n` counts 3) while the label match enjoys `$` (so `ab\n` is
/// virtual-hosted).
fn is_virtual_hostable_bucket(bucket: &str) -> bool {
    if bucket.len() < 3 || bucket.bytes().any(|b| b.is_ascii_uppercase()) {
        return false;
    }
    let bucket = strip_regex_newline(bucket);
    if is_ipv4_literal(bucket) || bucket.contains('.') {
        return false;
    }
    is_host_label(bucket)
}

/// Python `$` matches before one trailing newline, so `abc\n`
/// validates like `abc` in every `$`-anchored bucket/ARN/label match.
fn strip_regex_newline(value: &str) -> &str {
    value.strip_suffix('\n').unwrap_or(value)
}

/// `validate_bucket_name` (`botocore/handlers.py`,
/// `before-parameter-build.s3`): `^[a-zA-Z0-9.\-_]{1,255}$` or an
/// access-point/outpost ARN. Anything else is `ParamValidationError` —
/// the Django 500.
fn is_valid_bucket_name(bucket: &str) -> bool {
    let bucket = strip_regex_newline(bucket);
    if !bucket.is_empty()
        && bucket.len() <= 255
        && bucket
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-' || b == b'_')
    {
        return true;
    }
    is_accesspoint_arn(bucket) || is_outpost_arn(bucket)
}

/// `^arn:aws:.*:(s3|s3-object-lambda):[a-z\-0-9]*:[0-9]{12}:accesspoint[/:][a-zA-Z0-9\-.]{1,63}$`
/// with greedy `.*` (the rightmost colon split wins).
fn is_accesspoint_arn(value: &str) -> bool {
    let value = strip_regex_newline(value);
    let Some(tail) = value.strip_prefix("arn:aws") else {
        return false;
    };
    for (index, _) in tail.rmatch_indices(':') {
        let rest = &tail[index + 1..];
        let rest = if let Some(rest) = rest.strip_prefix("s3-object-lambda:") {
            rest
        } else if let Some(rest) = rest.strip_prefix("s3:") {
            rest
        } else {
            continue;
        };
        // `[a-z\-0-9]*:` — the terminator sits outside the class, so the
        // run ends at the first colon.
        let run = rest
            .bytes()
            .take_while(|b| b.is_ascii_lowercase() || *b == b'-' || b.is_ascii_digit())
            .count();
        let Some(after) = rest.get(run..).and_then(|s| s.strip_prefix(':')) else {
            continue;
        };
        if after.len() < 12 || !after.as_bytes()[..12].iter().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let Some(after) = after.get(12..).and_then(|s| s.strip_prefix(":accesspoint")) else {
            continue;
        };
        let Some(name) = after.strip_prefix('/').or_else(|| after.strip_prefix(':')) else {
            continue;
        };
        if !name.is_empty()
            && name.len() <= 63
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
        {
            return true;
        }
    }
    false
}

/// `^arn:aws:.*:s3-outposts:[a-z\-0-9]+:[0-9]{12}:outpost[/:][a-zA-Z0-9\-]{1,63}[/:]accesspoint[/:][a-zA-Z0-9\-]{1,63}$`
/// with greedy `.*` (the rightmost colon split wins; greedy outpost id
/// likewise).
fn is_outpost_arn(value: &str) -> bool {
    fn is_arn_name(name: &str) -> bool {
        !name.is_empty()
            && name.len() <= 63
            && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    }
    let value = strip_regex_newline(value);
    let Some(tail) = value.strip_prefix("arn:aws") else {
        return false;
    };
    for (index, _) in tail.rmatch_indices(':') {
        let Some(rest) = tail[index + 1..].strip_prefix("s3-outposts:") else {
            continue;
        };
        let run = rest
            .bytes()
            .take_while(|b| b.is_ascii_lowercase() || *b == b'-' || b.is_ascii_digit())
            .count();
        if run == 0 {
            continue;
        }
        let Some(after) = rest.get(run..).and_then(|s| s.strip_prefix(':')) else {
            continue;
        };
        if after.len() < 12 || !after.as_bytes()[..12].iter().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let Some(after) = after.get(12..).and_then(|s| s.strip_prefix(":outpost")) else {
            continue;
        };
        let Some(after) = after.strip_prefix('/').or_else(|| after.strip_prefix(':')) else {
            continue;
        };
        // `{id}[/:]accesspoint[/:]{name}` to the end — names hold no
        // separators, so the rightmost `accesspoint` slot wins.
        for (slot, _) in after.rmatch_indices("accesspoint") {
            let (id, sep) = (&after[..slot], &after[slot..]);
            let Some(id) = id.strip_suffix('/').or_else(|| id.strip_suffix(':')) else {
                continue;
            };
            let Some(name) = sep
                .strip_prefix("accesspoint")
                .and_then(|s| s.strip_prefix('/').or_else(|| s.strip_prefix(':')))
            else {
                continue;
            };
            if is_arn_name(id) && is_arn_name(name) {
                return true;
            }
        }
    }
    false
}

/// `urllib.parse.urlsplit` (`CPython/Lib/urllib/parse.py`) reduced to
/// endpoint parsing: leading C0/space strip, `\t\r\n` removal, scheme
/// split (lowercased), `//` netloc split with IPv6-bracket validation,
/// fragment drop, query split. Returns `(scheme, netloc, path, query)`,
/// or `None` when urlsplit raises. Non-ASCII netlocs fail (botocore runs
/// an NFKC check there that needs normalization tables — residual, see
/// the PR).
fn urlsplit_parts(url: &str) -> Option<(String, String, String, String)> {
    let trimmed: String = url
        .trim_start_matches(|c: char| c <= ' ')
        .chars()
        .filter(|c| *c != '\t' && *c != '\r' && *c != '\n')
        .collect();
    let mut scheme = String::new();
    let mut rest = trimmed.as_str();
    if let Some(colon) = trimmed.find(':') {
        let candidate = &trimmed[..colon];
        if colon > 0
            && candidate.as_bytes()[0].is_ascii_alphabetic()
            && candidate
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'-' || b == b'.')
        {
            scheme = candidate.to_ascii_lowercase();
            rest = &trimmed[colon + 1..];
        }
    }
    let mut netloc = String::new();
    if let Some(after) = rest.strip_prefix("//") {
        let end = after.find(['/', '?', '#']).unwrap_or(after.len());
        netloc = after[..end].to_owned();
        rest = &after[end..];
        let has_open = netloc.contains('[');
        let has_close = netloc.contains(']');
        if has_open != has_close || (has_open && !check_bracketed_netloc(&netloc)) {
            return None;
        }
    }
    if !netloc.is_ascii() {
        return None;
    }
    let rest = match rest.find('#') {
        Some(hash) => &rest[..hash],
        None => rest,
    };
    let (path, query) = match rest.find('?') {
        Some(mark) => (rest[..mark].to_owned(), rest[mark + 1..].to_owned()),
        None => (rest.to_owned(), String::new()),
    };
    Some((scheme, netloc, path, query))
}

/// `_check_bracketed_netloc`: userinfo split at the last `@`; nothing
/// before `[`, nothing-but-`:port` after `]`; the bracketed host itself
/// goes to [`check_bracketed_host`].
fn check_bracketed_netloc(netloc: &str) -> bool {
    let hostinfo = netloc.rsplit('@').next().unwrap_or_default();
    let Some(open) = hostinfo.find('[') else {
        // The brackets lived in the userinfo: the bare host part still
        // validates as a bracketed host.
        let hostname = hostinfo.split(':').next().unwrap_or_default();
        return check_bracketed_host(hostname);
    };
    if !hostinfo[..open].is_empty() {
        return false;
    }
    let inside = &hostinfo[open + 1..];
    let Some(close) = inside.find(']') else {
        return false;
    };
    let after = &inside[close + 1..];
    if !after.is_empty() && !after.starts_with(':') {
        return false;
    }
    check_bracketed_host(&inside[..close])
}

/// `_check_bracketed_host`: IPvFuture (`v` + hex + `.` + more) or a
/// strict IPv6 literal (an IPv4 address in brackets fails). A `%zone`
/// tail strips before the strict parse (validated by the ADDRZ pass
/// later); embedded-IPv4 octets reject C-style leading zeros like
/// `ipaddress` does.
fn check_bracketed_host(hostname: &str) -> bool {
    if let Some(tail) = hostname.strip_prefix('v') {
        let hex_len = tail.bytes().take_while(u8::is_ascii_hexdigit).count();
        let Some(rest) = tail.get(hex_len..).and_then(|s| s.strip_prefix('.')) else {
            return false;
        };
        return !rest.is_empty() && !rest.contains('\n');
    }
    let head = hostname.split('%').next().unwrap_or_default();
    if head.parse::<std::net::Ipv6Addr>().is_err() {
        return false;
    }
    if head.contains('.') {
        // Dots only occur in the embedded-IPv4 tail: every octet is
        // decimal without C-style leading zeros.
        let dotted = &head[head.rfind(':').map_or(0, |i| i + 1)..];
        for octet in dotted.split('.') {
            if octet.len() > 1 && octet.starts_with('0') {
                return false;
            }
        }
    }
    true
}

/// `_hostinfo` (`urllib/parse.py`): userinfo split at the last `@`,
/// bracket-aware host/port split, empty port reads as missing.
fn split_hostinfo(netloc: &str) -> (String, Option<String>) {
    let hostinfo = netloc.rsplit('@').next().unwrap_or_default();
    let (hostname, port) = match hostinfo.find('[') {
        Some(open) => {
            let after_open = &hostinfo[open + 1..];
            match after_open.find(']') {
                Some(close) => {
                    let after_close = &after_open[close + 1..];
                    let port = after_close.split(':').nth(1).unwrap_or_default();
                    (&after_open[..close], port)
                }
                None => (after_open, ""),
            }
        }
        None => {
            let mut parts = hostinfo.splitn(2, ':');
            (
                parts.next().unwrap_or_default(),
                parts.next().unwrap_or_default(),
            )
        }
    };
    let port = if port.is_empty() {
        None
    } else {
        Some(port.to_owned())
    };
    (hostname.to_owned(), port)
}

/// `.port` (`urllib/parse.py`): ASCII digits, `0..=65535` — `None`
/// (absent) passes through, anything else fails the parse.
fn parse_port_number(port: Option<&str>) -> Option<Option<u16>> {
    match port {
        None => Some(None),
        Some(text) => {
            if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            match text.parse::<u32>() {
                Ok(number) if number <= 65535 => Some(Some(number as u16)),
                _ => None,
            }
        }
    }
}

/// `.hostname` (`urllib/parse.py`): missing when empty, lowercased
/// except a `%zone` tail (kept verbatim).
fn normalize_hostname(hostname: &str) -> Option<String> {
    if hostname.is_empty() {
        return None;
    }
    match hostname.find('%') {
        Some(zone) => Some(format!(
            "{}{}",
            hostname[..zone].to_ascii_lowercase(),
            &hostname[zone..]
        )),
        None => Some(hostname.to_ascii_lowercase()),
    }
}

/// The urllib3 IPv6 alternatives (`botocore/compat.py`): full form is
/// exactly 8 groups with a loose-IPv4-or-`h16:h16` tail; `::` (at most
/// once) compresses at least one group. The embedded IPv4 tail is
/// range-loose (`999` included).
fn is_ipv6_loose(head: &str) -> bool {
    fn is_hex16(group: &str) -> bool {
        !group.is_empty() && group.len() <= 4 && group.bytes().all(|b| b.is_ascii_hexdigit())
    }
    // Explicit colon-groups in a `::` half: all hex, except the last may
    // be a loose-IPv4 tail worth two groups. Returns the group count.
    fn half_groups(half: &str) -> Option<usize> {
        if half.is_empty() {
            return Some(0);
        }
        let groups: Vec<&str> = half.split(':').collect();
        if groups.iter().any(|g| g.is_empty()) {
            return None;
        }
        let last = groups.len() - 1;
        if groups[last].contains('.') {
            if !is_ipv4_literal(groups[last]) || groups[..last].iter().any(|g| !is_hex16(g)) {
                return None;
            }
            Some(groups.len() + 1)
        } else if groups.iter().all(|g| is_hex16(g)) {
            Some(groups.len())
        } else {
            None
        }
    }
    match head.split_once("::") {
        None => {
            if head.contains('.') {
                let Some(colon) = head.rfind(':') else {
                    return false;
                };
                let (front, tail) = (&head[..colon], &head[colon + 1..]);
                front.split(':').count() == 6
                    && front.split(':').all(is_hex16)
                    && is_ipv4_literal(tail)
            } else {
                let groups: Vec<&str> = head.split(':').collect();
                groups.len() == 8 && groups.iter().all(|g| is_hex16(g))
            }
        }
        Some((left, right)) => {
            if right.contains("::") {
                return false;
            }
            match (half_groups(left), half_groups(right)) {
                (Some(left), Some(right)) => left + right <= 7,
                _ => false,
            }
        }
    }
}

/// `is_valid_ipv6_endpoint_url`: `'[' + hostname + ']'` against
/// `IPV6_ADDRZ_RE` — a loose IPv6 literal with an optional `%zone`
/// (`(?:%25|%)(?:[unreserved]|%HH)+`, unreserved `A-Za-z0-9._!~-`).
fn is_valid_ipv6_bracketed(hostname: &str) -> bool {
    let (head, zone) = match hostname.find('%') {
        Some(at) => (&hostname[..at], Some(&hostname[at..])),
        None => (hostname, None),
    };
    if let Some(zone) = zone {
        let tail = if let Some(tail) = zone.strip_prefix("%25") {
            tail
        } else if let Some(tail) = zone.strip_prefix('%') {
            tail
        } else {
            return false;
        };
        if tail.is_empty() {
            return false;
        }
        let mut rest = tail;
        while !rest.is_empty() {
            if let Some(hex) = rest.strip_prefix('%') {
                if hex.len() < 2 || !hex.as_bytes()[..2].iter().all(|b| b.is_ascii_hexdigit()) {
                    return false;
                }
                rest = &rest[3..];
            } else {
                let byte = rest.as_bytes()[0];
                if !(byte.is_ascii_alphanumeric()
                    || byte == b'.'
                    || byte == b'_'
                    || byte == b'!'
                    || byte == b'~'
                    || byte == b'-')
                {
                    return false;
                }
                rest = &rest[1..];
            }
        }
    }
    is_ipv6_loose(head)
}

/// `is_valid_endpoint_url or is_valid_ipv6_endpoint_url`
/// (`botocore/utils.py`, `endpoint.py:400`): a scheme and host, no
/// `\t\r\n`, hostname ≤ 255 chars, valid labels after one trailing-dot
/// strip — or a bracketable IPv6 literal. Anything else is
/// `ValueError: Invalid endpoint` (the Django 500).
fn is_valid_endpoint_str(url: &str) -> bool {
    if url.contains(['\t', '\r', '\n']) {
        return false;
    }
    let Some((_, netloc, _, _)) = urlsplit_parts(url) else {
        return false;
    };
    let (hostname, _) = split_hostinfo(&netloc);
    let Some(hostname) = normalize_hostname(&hostname) else {
        return false;
    };
    if hostname.len() > 255 {
        return false;
    }
    let stripped = hostname.strip_suffix('.').unwrap_or(&hostname);
    if stripped.split('.').all(is_host_label) {
        return true;
    }
    is_valid_ipv6_bracketed(&hostname)
}

/// `normalize_url_path` + `remove_dot_segments` (`botocore/utils.py`):
/// RFC 3986 §5.2.4 plus consecutive-slash collapsing (leading empties
/// drop, over-pops vanish — the result never starts with `/`).
fn normalize_url_path(path: &str) -> String {
    if path.is_empty() {
        return "/".to_owned();
    }
    let mut kept: Vec<&str> = Vec::new();
    for segment in path.split('/') {
        if segment.is_empty() || segment == "." {
            continue;
        }
        if segment == ".." {
            kept.pop();
        } else {
            kept.push(segment);
        }
    }
    kept.join("/")
}

/// `parseURL` (`endpoint_provider.py`) plus the client-creation gate:
/// strict custom-endpoint validation returning the lowercased scheme,
/// the verbatim authority and the normalized base path (`""` or
/// `"/base"`, no trailing slash). Fragments drop; queries,
/// non-`http(s)` schemes, bad ports and invalid hosts fail — every
/// failure is the Django 500.
fn parse_custom_endpoint(url: &str) -> Option<(String, String, String)> {
    if !is_valid_endpoint_str(url) {
        return None;
    }
    let (scheme, netloc, path, query) = urlsplit_parts(url)?;
    if scheme != "http" && scheme != "https" {
        return None;
    }
    if !query.is_empty() {
        return None;
    }
    let (_, port) = split_hostinfo(&netloc);
    parse_port_number(port.as_deref())?;
    // `normalizedPath`: dot-segments resolved, doubles collapsed,
    // `quote(path, safe='/')` (exactly [`uri_encode_path`]), trailing
    // slash ensured — then trimmed back off for joining.
    let mut normal = uri_encode_path(&normalize_url_path(&path));
    if !normal.ends_with('/') {
        normal.push('/');
    }
    let base = normal.trim_end_matches('/');
    let base_path = if base.is_empty() {
        String::new()
    } else {
        format!("/{base}")
    };
    Some((scheme, netloc, base_path))
}

/// `_host_from_url` (`botocore/auth.py`): the signed `host` header —
/// lowercased (zone tail verbatim), no userinfo, default/empty port
/// stripped, IPv6 re-bracketed.
fn signed_host_for(scheme: &str, authority: &str) -> Option<String> {
    let (hostname, port) = split_hostinfo(authority);
    let mut host = normalize_hostname(&hostname)?;
    if is_valid_ipv6_bracketed(&host) {
        host = format!("[{host}]");
    }
    if let Some(port) = port {
        let number: u16 = port.parse().ok()?;
        let default = match scheme {
            "http" => 80,
            "https" => 443,
            _ => 0,
        };
        if number != default {
            host = format!("{host}:{number}");
        }
    }
    Some(host)
}

/// Endpoint + signed host + base path + addressing for signing,
/// mirroring `S3Storage.__init__` with a request (`is_server=False`)
/// plus botocore's S3 endpoint ruleset: MinIO mode signs
/// `{scheme}://{Host}` path-style; an explicit endpoint URL signs
/// path-style against its normalized base; otherwise the virtual-hosted
/// AWS default (path-style for non-host-label buckets). Presigned URLs
/// resolve the global endpoint in `aws` (`use_global_endpoint`) and the
/// regional host in every other partition — every caller here presigns,
/// so there is no header-auth split.
/// `None` when the region, bucket or endpoint cannot be signed (garbage
/// regions, `ParamValidationError` buckets, invalid endpoint URLs, an
/// empty region against a derived endpoint — botocore `ValueError:
/// Invalid endpoint`) or when MinIO mode needs the request host and none
/// was sent (Django reads `request.get_host()` solely in that branch —
/// `storage.py:52-60` — so AWS/custom-endpoint signing never requires a
/// `Host` header). Python raises from the constructor, so every use site
/// propagates it to the 500 fallback (`handle_exception`).
fn endpoint_parts(
    storage: &pidash_db::config::StorageSettings,
    scheme: &str,
    host: Option<&str>,
) -> Option<ResolvedEndpoint> {
    if !pidash_db::config::is_valid_region_name(&storage.region) {
        return None;
    }
    // Python `$` matches before one trailing newline, so `abc\n`
    // passes the bucket gate and signs exactly like `abc` — except
    // path-style URL segments and the POST policy carry the raw spelling
    // (`%0A` / `\n`). The stripped form drives the gate and the virtual
    // host; addressing reads the raw name (length counts the newline);
    // path segments encode the raw bucket below.
    // (PIDASHCONV-789#7, live-probed.)
    let bucket = strip_regex_newline(&storage.bucket_name);
    if !is_valid_bucket_name(bucket) {
        return None;
    }
    // `MINIO_ENDPOINT_SSL=1` signs https in MinIO mode (`storage.py:46-51`).
    let scheme = storage.endpoint_protocol(scheme);
    if storage.use_minio {
        let host = host?;
        let (scheme, authority, base_path) = parse_custom_endpoint(&format!("{scheme}://{host}"))?;
        return Some(ResolvedEndpoint {
            url_base: format!("{scheme}://{authority}"),
            signed_host: signed_host_for(&scheme, &authority)?,
            base_path,
            path_style: true,
        });
    }
    if let Some(endpoint) = storage.endpoint_url.as_deref().filter(|e| !e.is_empty()) {
        let (scheme, authority, base_path) = parse_custom_endpoint(endpoint)?;
        return Some(ResolvedEndpoint {
            url_base: format!("{scheme}://{authority}"),
            signed_host: signed_host_for(&scheme, &authority)?,
            base_path,
            path_style: true,
        });
    }
    if storage.region.is_empty() {
        // botocore derives `https://s3..amazonaws.com` and rejects
        // it (`ValueError: Invalid endpoint`).
        return None;
    }
    let (suffix, is_aws) = partition_for_region(&storage.region);
    // Presigned URLs use the global endpoint in `aws`
    // (`use_global_endpoint`); every other partition signs regional.
    let base = if is_aws {
        "s3.amazonaws.com".to_owned()
    } else {
        format!("s3.{}.{suffix}", storage.region)
    };
    // Access-point ARNs pass the bucket gate but route to special
    // endpoints botocore-side; this port signs them virtual-hosted
    // (known residual, see the PR). The RAW name drives addressing:
    // `is_virtual_hostable_bucket` reads the raw length (`ab\n` counts
    // 3) and strips for the label match itself — the stripped `bucket`
    // below is only the virtual-host spelling.
    let virtual_host = is_accesspoint_arn(&storage.bucket_name)
        || is_outpost_arn(&storage.bucket_name)
        || is_virtual_hostable_bucket(&storage.bucket_name);
    if virtual_host {
        let host = format!("{bucket}.{base}");
        Some(ResolvedEndpoint {
            url_base: format!("https://{host}"),
            signed_host: host,
            base_path: String::new(),
            path_style: false,
        })
    } else {
        Some(ResolvedEndpoint {
            url_base: format!("https://{base}"),
            signed_host: base,
            base_path: String::new(),
            path_style: true,
        })
    }
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

/// `generate_presigned_url(object_name, disposition="attachment",
/// filename=...)` (`views/issue.py:2571-2577`, `storage.py`): presigned
/// GET with the attachment disposition. `filename` is
/// `attributes["name"]` (`None` when missing/null — a fresh uuid4 hex per
/// call; non-strings are a caller-side `TypeError` 500 before this runs).
fn presigned_get_url(
    storage: &pidash_db::config::StorageSettings,
    scheme: &str,
    host: Option<&str>,
    object_name: &str,
    filename: Option<&str>,
    now: &DateTime<Utc>,
) -> Option<String> {
    let region = storage.region.as_str();
    let resolved = endpoint_parts(storage, scheme, host)?;
    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let date = now.format("%Y%m%d").to_string();
    let scope = credential_scope(&date, region);
    let credential = format!("{}/{}", storage.access_key_id, scope);
    let generated = Uuid::new_v4().simple().to_string();
    let disposition = work_tasks::content_disposition("attachment", filename, &generated);
    // Auth params in botocore's insertion order
    // (`SigV4QueryAuth._modify_request_before_signing`).
    let auth_params = [
        ("X-Amz-Algorithm".to_owned(), "AWS4-HMAC-SHA256".to_owned()),
        ("X-Amz-Credential".to_owned(), credential),
        ("X-Amz-Date".to_owned(), amz_date.clone()),
        (
            "X-Amz-Expires".to_owned(),
            storage.signed_url_expiration_secs.to_string(),
        ),
        ("X-Amz-SignedHeaders".to_owned(), "host".to_owned()),
    ];
    // The canonical query sorts everything (signing input); the URL
    // keeps operation params before auth params (botocore: "The spec is
    // particular about this").
    let mut canonical: Vec<(String, String)> = Vec::with_capacity(6);
    canonical.push((
        "response-content-disposition".to_owned(),
        disposition.clone(),
    ));
    canonical.extend(auth_params.iter().cloned());
    canonical.sort_by(|a, b| a.0.cmp(&b.0));
    let canonical_query = canonical
        .iter()
        .map(|(k, v)| format!("{}={}", uri_encode(k), uri_encode(v)))
        .collect::<Vec<_>>()
        .join("&");
    let mut url_query = format!("response-content-disposition={}", uri_encode(&disposition));
    for (key, value) in &auth_params {
        url_query.push('&');
        url_query.push_str(&uri_encode(key));
        url_query.push('=');
        url_query.push_str(&uri_encode(value));
    }
    let mut canonical_path = resolved.base_path.clone();
    if resolved.path_style {
        canonical_path.push('/');
        // Raw spelling, URI-encoded (`a\n` travels as `/a%0A/`).
        canonical_path.push_str(&uri_encode(&storage.bucket_name));
    }
    canonical_path.push('/');
    canonical_path.push_str(&uri_encode_path(object_name));
    let canonical = format!(
        "GET\n{canonical_path}\n{canonical_query}\nhost:{}\n\nhost\nUNSIGNED-PAYLOAD",
        resolved.signed_host
    );
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{}",
        sha256_hex(canonical.as_bytes())
    );
    let signature = hex(&hmac_sha256(
        &signing_key(&storage.secret_access_key, &date, region),
        string_to_sign.as_bytes(),
    ));
    Some(format!(
        "{}{canonical_path}?{url_query}&X-Amz-Signature={signature}",
        resolved.url_base
    ))
}

/// The `${filename}` template token: `key[: -len(token)]` is the
/// `starts-with` prefix (a char slice — `[:-11]` on a short key is `""`,
/// exactly like Python).
const FILENAME_TOKEN: &str = "${filename}";

fn strip_filename_token(key: &str) -> String {
    let chars: Vec<char> = key.chars().collect();
    chars[..chars.len().saturating_sub(FILENAME_TOKEN.len())]
        .iter()
        .collect()
}

/// `generate_presigned_post(object_name, file_type, file_size)`
/// (`views/issue.py:2404`, `storage.py:66-100`): `{"url","fields"}` with
/// botocore's field order and the `storage.py` condition order.
/// `file_size` is the pre-spelled policy number ([`policy_number`]).
fn presigned_post(
    storage: &pidash_db::config::StorageSettings,
    scheme: &str,
    host: Option<&str>,
    object_name: &str,
    file_type: &str,
    file_size: &str,
    now: &DateTime<Utc>,
) -> Option<Value> {
    let region = storage.region.as_str();
    let resolved = endpoint_parts(storage, scheme, host)?;
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
    // the signer's (`botocore/signers.py`), so the policy carries nine —
    // except a key ending in `${filename}` appends
    // `["starts-with", "$key", <prefix>]` instead of its `{"key"}`
    // (while `fields.key` keeps the full key). `storage.py:83-84` has
    // its own template branch for keys STARTING with the token (the
    // prefix comes from the same trailing-11 slice — port the quirk).
    // (PIDASHCONV-789#1, live-probed.)
    let caller_key_condition = if object_name.starts_with(FILENAME_TOKEN) {
        format!(
            "[\"starts-with\", \"$key\", {}]",
            py_json_string(&strip_filename_token(object_name))
        )
    } else {
        format!("{{\"key\": {}}}", py_json_string(object_name))
    };
    let client_key_condition = if object_name.ends_with(FILENAME_TOKEN) {
        format!(
            "[\"starts-with\", \"$key\", {}]",
            py_json_string(&strip_filename_token(object_name))
        )
    } else {
        format!("{{\"key\": {}}}", py_json_string(object_name))
    };
    let conditions = format!(
        "[{{\"bucket\": {}}}, [\"content-length-range\", 1, {}], {{\"Content-Type\": {}}}, {}, {{\"bucket\": {}}}, {}, {{\"x-amz-algorithm\": \"AWS4-HMAC-SHA256\"}}, {{\"x-amz-credential\": {}}}, {{\"x-amz-date\": {}}}]",
        py_json_string(&storage.bucket_name),
        file_size,
        py_json_string(file_type),
        caller_key_condition,
        py_json_string(&storage.bucket_name),
        client_key_condition,
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
    // `url`: path-style appends the bucket to the base, virtual-hosted
    // is the bucket root with its trailing slash.
    let url = if resolved.path_style {
        format!(
            "{}{}/{}",
            resolved.url_base,
            resolved.base_path,
            uri_encode(&storage.bucket_name)
        )
    } else {
        format!("{}/", resolved.url_base)
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
    Some(serde_json::json!({"url": url, "fields": fields}))
}

/// CPython `json.dumps` string encoding (`ensure_ascii`): `"` and
/// `\` escaped, C0 controls short/`\u00XX`, everything else non-ASCII
/// as `\uXXXX` (surrogate pairs past the BMP).
fn py_json_string(input: &str) -> String {
    let mut out = String::with_capacity(input.len() + 2);
    out.push('"');
    for ch in input.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0C}' => out.push_str("\\f"),
            ch if (ch as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", ch as u32));
            }
            ch if (ch as u32) < 0x7F => out.push(ch),
            ch => {
                let code = ch as u32;
                if code > 0xFFFF {
                    let code = code - 0x1_0000;
                    out.push_str(&format!(
                        "\\u{:04x}\\u{:04x}",
                        0xD800 + (code >> 10),
                        0xDC00 + (code & 0x3FF)
                    ));
                } else {
                    out.push_str(&format!("\\u{code:04x}"));
                }
            }
        }
    }
    out.push('"');
    out
}

fn base64_encode(input: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(input)
}

/// Whether the storage settings can sign: boto3 without credentials
/// raises `NoCredentialsError` (not a `ClientError`) into the 500, so an
/// empty key or secret answers [`Denial::ServerError`] before signing.
fn storage_can_sign(storage: &pidash_db::config::StorageSettings) -> bool {
    !storage.access_key_id.is_empty() && !storage.secret_access_key.is_empty()
}

/// CPython `float()` over a JSON number that `as_f64` cannot read
/// (an arbitrary-precision overflow literal or a huge int): float
/// spellings saturate to ±inf exactly like `float("1e999")`; huge int
/// literals stay exact, so they read as `None`. (Mirrors the `tasks`
/// kernel, which keeps its copy private.)
fn saturated_float(n: &serde_json::Number) -> Option<f64> {
    if n.to_string()
        .bytes()
        .any(|b| b == b'.' || b == b'e' || b == b'E')
    {
        n.to_string().parse::<f64>().ok()
    } else {
        None
    }
}

/// Python truthiness over request data (`if not x`): null/false/zero/
/// empty all falsy (mirrors the `tasks` kernel, which keeps its copy
/// private).
fn is_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                i != 0
            } else if let Some(u) = n.as_u64() {
                u != 0
            } else if let Some(f) = n.as_f64() {
                f != 0.0
            } else if let Some(f) = saturated_float(n) {
                // Overflow float literal (`-1e999`): CPython saturates
                // to ±inf (truthy).
                f != 0.0
            } else {
                // Huge int literal past u64/i64: exact in Python —
                // truthy iff any nonzero digit.
                n.to_string()
                    .bytes()
                    .any(|b| b.is_ascii_digit() && b != b'0')
            }
        }
        Value::String(s) => !s.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
    }
}

/// Whether `json.dumps(attributes)` (`views/issue.py:2387` JSONField
/// prep) renders a non-finite float: Python spells ±inf `Infinity` /
/// `-Infinity`, which the jsonb cast rejects (`DataError` 500). The
/// driver would send the raw literal (`-1e999`), which Postgres
/// numeric accepts — so an overflow float anywhere in the map 500s
/// first. Huge ints dump and store fine on both sides, so only floats
/// fail. Iterative: values nest to thousands of levels (see
/// `to_serde_publish`), past a recursive walk's stack on a 2MB tokio
/// worker.
fn attributes_dump_fails(attributes: &Map<String, Value>) -> bool {
    let mut stack: Vec<&Value> = attributes.values().collect();
    while let Some(value) = stack.pop() {
        match value {
            Value::Number(n) => {
                if n.as_f64().is_some_and(|f| !f.is_finite())
                    || saturated_float(n).is_some_and(|f| !f.is_finite())
                {
                    return true;
                }
            }
            Value::Array(items) => stack.extend(items.iter()),
            Value::Object(map) => stack.extend(map.values()),
            _ => {}
        }
    }
    false
}

/// A `size_limit` plan value as the `float8` size column stores it:
/// bools as 0/1, integers exactly-or-rounded, floats verbatim. An
/// integer past `f64` range is Python's `OverflowError` 500
/// (`FloatField` prep on INSERT).
fn size_limit_f64(value: &Value) -> Result<f64, Denial> {
    match value {
        Value::Bool(b) => Ok(if *b { 1.0 } else { 0.0 }),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(i as f64)
            } else if let Some(u) = n.as_u64() {
                Ok(u as f64)
            } else if let Some(f) = n.as_f64() {
                if f.is_finite() {
                    Ok(f)
                } else {
                    Err(Denial::ServerError)
                }
            } else {
                Err(Denial::ServerError)
            }
        }
        _ => Err(Denial::ServerError),
    }
}

/// `base_host(request, is_app=True)` (`utils/host.py:17-60`): the app
/// base URL when set, else the web-or-app origin; a missing pair raises
/// `ImproperlyConfigured` into the 500.
fn app_origin(urls: &pidash_db::config::UrlSettings) -> Result<String, Denial> {
    if let Some(url) = urls.app_base_url.as_deref().filter(|s| !s.is_empty()) {
        return Ok(url.to_owned());
    }
    if let Some(url) = urls.web_url.as_deref().filter(|s| !s.is_empty()) {
        return Ok(url.to_owned());
    }
    Err(Denial::ServerError)
}

/// The POST body spec: scalar `.get` reads, no serializer — so no list
/// fields and no blank-skipping (a present-empty form value stays `""`,
/// which the truthiness checks then reject).
const POST_BODY_SPEC: crate::v1_cycles_modules::body::BodySpec =
    crate::v1_cycles_modules::body::BodySpec {
        list_fields: &[],
        skip_blank_fields: &[],
    };

/// Negotiated POST data: the text map plus the per-key upload filename
/// (DRF merges files into `request.data` after texts, so a file wins —
/// `FormBody::last_is_file`).
#[derive(Debug)]
struct PostData {
    map: Map<String, Value>,
    files: std::collections::BTreeMap<String, String>,
}

/// Parse the attachment POST body: content-type dispatch (415 for the
/// rest), empty bodies to `{}`, JSON through the CPython parser,
/// forms through the HTML-input kernel. A non-object JSON body 500s
/// (Python calls `.get` on the parsed list — `AttributeError`).
fn parse_post_data(headers: &HeaderMap, body: &[u8]) -> Result<PostData, Denial> {
    use crate::v1_cycles_modules::body::{negotiate_body, BodyError, NegotiatedBody};
    use crate::v1_cycles_modules::json_cpython::{
        parse_json_text_spans, to_serde_publish_map, JsonFail,
    };
    match negotiate_body(headers, body, &POST_BODY_SPEC) {
        Err(BodyError::UnsupportedMediaType(detail)) => Err(Denial::UnsupportedMediaType(detail)),
        Err(BodyError::ParseDetail(detail)) => Err(Denial::BadDetail(detail)),
        Err(BodyError::ServerError) => Err(Denial::ServerError),
        Ok(NegotiatedBody::Empty) => Ok(PostData {
            map: Map::new(),
            files: Default::default(),
        }),
        Ok(NegotiatedBody::Form { map, files, .. }) => Ok(PostData {
            map,
            files: files
                .iter()
                .filter_map(|(key, parts)| {
                    parts
                        .last()
                        .map(|part| (key.clone(), part.filename.clone()))
                })
                .collect(),
        }),
        Ok(NegotiatedBody::JsonText { text, surr }) => match parse_json_text_spans(&text, &surr) {
            Err(JsonFail::Message(detail)) => {
                Err(Denial::BadDetail(format!("JSON parse error - {detail}")))
            }
            Err(JsonFail::Recursion) => Err(Denial::ServerError),
            Ok(value) => match value.into_object() {
                Some(object) => Ok(PostData {
                    map: to_serde_publish_map(&object),
                    files: Default::default(),
                }),
                None => Err(Denial::ServerError),
            },
        },
    }
}

/// Map a paginator failure: parse errors answer the `ParseError` 400,
/// evaluation failures the generic 500 (the `v1_projects` mapping).
fn page_denial(error: crate::paginator::PageError) -> Denial {
    use crate::paginator::PageError as E;
    match error {
        E::InvalidPerPage
        | E::PerPageTooLarge(_)
        | E::InvalidCursor
        | E::OffsetTooLarge
        | E::NegativeOffset => Denial::BadDetail(error.detail()),
        E::ZeroLimit | E::NegativeSlice | E::NonFiniteCursor | E::MissingOrderKey => {
            Denial::ServerError
        }
    }
}

/// The 12-key `BasePaginator.paginate` envelope
/// (`utils/paginator.py:714-731`), keys in order via the kernel.
fn envelope(
    total_count: i64,
    per_page: i64,
    next: &crate::paginator::Cursor,
    prev: &crate::paginator::Cursor,
    results: Value,
) -> Result<Response, Denial> {
    use crate::paginator::{max_hits, PageResponse};
    let total_pages = max_hits(total_count, per_page).map_err(page_denial)?;
    let page = PageResponse {
        grouped_by: None,
        sub_grouped_by: None,
        total_count,
        next_cursor: next.to_string(),
        prev_cursor: prev.to_string(),
        next_page_results: next.has_results_or_false(),
        prev_page_results: prev.has_results_or_false(),
        count: results.as_array().map(|a| a.len()).unwrap_or(0),
        total_pages,
        total_results: total_count,
        extra_stats: None,
        results,
    };
    let body = serde_json::to_string(&page).map_err(|_| Denial::ServerError)?;
    Ok(json_response(StatusCode::OK, body))
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

async fn read_body(body: axum::body::Body) -> Result<Vec<u8>, Denial> {
    axum::body::to_bytes(body, usize::MAX)
        .await
        .map(|bytes| bytes.to_vec())
        .map_err(|error| db_error(error, "read-body"))
}

/// Rebuild the request against the ORIGINAL path for the proxy
/// (Django's `<uuid:>` converter would not match — before auth runs, as URL
/// resolving precedes it). The URI must be the request's own: Django's 404
/// page echoes the path, so rebuilding the `work-items` spelling for a
/// deprecated `issues/` twin answers the wrong bytes.
fn proxy_request<'a>(
    state: &'a AppState,
    method: &'a str,
    uri: String,
) -> impl std::future::Future<Output = Response> + 'a {
    let req = Request::builder()
        .method(method)
        .uri(uri)
        .body(axum::body::Body::empty())
        .expect("proxy request");
    crate::edge::proxy(State(state.clone()), req)
}

/// Caller expansion values for one activity row: every `expand` name in
/// the kept fields with a map hit (`project`, `workspace`, `issue`,
/// `actor`). Null FKs supply `None` (the shape renders `{}`); dead rows
/// render null. Anything else is the shape's passthrough/null rule.
async fn activity_expansions<'a>(
    pool: &PgPool,
    row: &sqlx::postgres::PgRow,
    slug: &str,
    tz: &Tz,
    web_base: Option<&str>,
    expand_refs: &[&'a str],
    kept: &[String],
) -> Result<Vec<(&'a str, Option<Value>)>, Denial> {
    let mut expansions: Vec<(&'a str, Option<Value>)> = Vec::new();
    for name in expand_refs {
        if !kept.iter().any(|kept| kept == name) {
            continue;
        }
        if !BASE_EXPANSION_NAMES.contains(name) {
            continue;
        }
        let value = match *name {
            "project" => {
                let id = row_uuid(row, "project_id")?;
                Some(expand_project(pool, &id).await?)
            }
            "workspace" => {
                let id = row_uuid(row, "workspace_id")?;
                Some(expand_workspace(pool, &id).await?)
            }
            "issue" => match row_uuid_opt(row, "issue_id")? {
                Some(id) => Some(expand_issue(pool, slug, &id, tz, web_base).await?),
                None => None,
            },
            "actor" => match row_uuid_opt(row, "actor_id")? {
                Some(id) => Some(expand_actor(pool, &id).await?),
                None => None,
            },
            // `expand ∩ kept ∩ map` over the activity fields is exactly
            // the four arms above (verified against
            // `ACTIVITY_READ_FIELDS`); anything else is unreachable.
            _ => continue,
        };
        expansions.push((*name, value));
    }
    Ok(expansions)
}

/// `GET .../activities/` (`views/issue.py:2150-2173`).
pub async fn get_activity_list(
    State(state): State<AppState>,
    OriginalUri(original): OriginalUri,
    Path((slug, project_id, issue_id)): Path<(String, String, String)>,
    Query(query): Query<QueryMap>,
    headers: HeaderMap,
) -> Response {
    if !crate::runner_runs::is_uuid_path_segment(&issue_id) {
        return proxy_request(&state, "GET", original.to_string()).await;
    }
    let issue_id = issue_id.parse::<Uuid>().expect("checked segment");
    match activity_list_inner(&state, &headers, &slug, &project_id, &issue_id, &query).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

async fn activity_list_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    project_id_raw: &str,
    issue_id: &Uuid,
    query: &QueryMap,
) -> Result<Response, Denial> {
    use pidash_services::v1_work_items::shape_social::ACTIVITY_READ_FIELDS;
    use pidash_services::v1_work_items::{filter_fields, FieldSpec};
    let pre = preamble(state, headers, slug).await?;
    let project_id = rewrite_project_id(&pre.pool, slug, project_id_raw).await?;
    let workspace_id = pre.workspace_id.ok_or(Denial::Forbidden)?;
    require_gate(
        &pre.pool,
        &workspace_id,
        slug,
        &pre.actor.id,
        &project_id,
        V1WorkItemsRoute::ActivityList,
        "GET",
    )
    .await?;
    // `TimezoneMixin.initial` runs after the gate but before the body: a
    // bad zone wins over pagination/row errors below.
    let tz = activate_timezone(pre.actor.timezone.as_deref())?;
    // `.order_by(...)` validates EAGERLY (`Query.add_ordering` calls
    // `names_to_path` at build time), so a bad `order_by` 500s before
    // `paginate` ever parses `per_page`/`cursor` (PIDASHCONV-789#5,
    // live-probed: `?order_by=bogus&per_page=abc` is a 500).
    let order = resolve_activity_order(query_last(query, "order_by").as_deref())?;
    let per_page =
        crate::paginator::parse_per_page(query_last(query, "per_page").as_deref(), 1000, 1000)
            .map_err(page_denial)?;
    let cursor_raw = query_last(query, "cursor").unwrap_or_else(|| format!("{per_page}:0:0"));
    let cursor = crate::paginator::Cursor::from_string(&cursor_raw).map_err(page_denial)?;
    let rows = fetch_activity_rows(
        &pre.pool,
        slug,
        &project_id,
        issue_id,
        &pre.actor.id,
        &order,
    )
    .await?;
    let window = crate::paginator::offset_window(
        per_page,
        cursor.offset,
        cursor.value,
        cursor.is_prev,
        None,
    )
    .map_err(page_denial)?;
    // `results[:limit]` runs on the lazy queryset, whose negative indexing
    // raises `ValueError` into the 500 — after the offset checks above
    // (a negative offset still 400s first). `per_page=-1` passes
    // `offset_window` (stop 0), so it must be refused here.
    if per_page < 0 {
        return Err(page_denial(crate::paginator::PageError::NegativeSlice));
    }
    let total_count = rows.len() as i64;
    // `queryset[offset:stop]` over the evaluated rows.
    let start = (window.offset as usize).min(rows.len());
    let stop = (window.stop as usize).min(rows.len());
    let window_rows = &rows[start..stop];
    // Backwards walk with a mismatched cursor value reads nothing
    // (`results[-(limit+1):]` on the lazy queryset raises into the 500).
    if !cursor.value.equals_limit(per_page) && cursor.is_prev {
        return Err(Denial::ServerError);
    }
    let has_more = window_rows.len() as i64 > per_page;
    // `results[:limit]` over the evaluated window (negative limits were
    // refused above).
    let trim = usize::try_from(per_page)
        .unwrap_or(usize::MAX)
        .min(window_rows.len());
    let page_rows = &window_rows[..trim];
    let fields = fields_param(query, "fields");
    let expand = fields_param(query, "expand");
    let specs: Vec<FieldSpec> = fields
        .as_deref()
        .unwrap_or(&[])
        .iter()
        .map(|name| FieldSpec::Include(name.clone()))
        .collect();
    let field_specs = if fields.is_some() {
        Some(specs.as_slice())
    } else {
        None
    };
    let expand_refs: Vec<&str> = expand
        .as_deref()
        .unwrap_or(&[])
        .iter()
        .map(String::as_str)
        .collect();
    // Only `expand` names in the kept fields with a map hit need caller
    // values; anything else is the shape's passthrough/null rule.
    let kept = filter_fields(ACTIVITY_READ_FIELDS, field_specs).map_err(|_| Denial::ServerError)?;
    let web_base = pidash_services::v1_work_items::shape_issue::web_base_url(
        state.settings().urls.web_url.as_deref(),
        state.settings().urls.app_base_url.as_deref(),
    );
    let mut rendered: Vec<Value> = Vec::with_capacity(page_rows.len());
    for row in page_rows {
        let decoded = decode_activity(row, &tz)?;
        let expansions = activity_expansions(
            &pre.pool,
            row,
            slug,
            &tz,
            web_base.as_deref(),
            &expand_refs,
            &kept,
        )
        .await?;
        rendered.push(render_activity_value(
            &decoded,
            field_specs,
            &expand_refs,
            &expansions,
        )?);
    }
    let next = crate::paginator::next_cursor(per_page, cursor.offset, has_more);
    let prev = crate::paginator::prev_cursor(per_page, cursor.offset);
    envelope(total_count, per_page, &next, &prev, Value::Array(rendered))
}

/// `GET .../activities/<pk>/` (`views/issue.py:2205-2232`).
pub async fn get_activity_detail(
    State(state): State<AppState>,
    OriginalUri(original): OriginalUri,
    Path((slug, project_id, issue_id, pk)): Path<(String, String, String, String)>,
    Query(query): Query<QueryMap>,
    headers: HeaderMap,
) -> Response {
    if !crate::runner_runs::is_uuid_path_segment(&issue_id)
        || !crate::runner_runs::is_uuid_path_segment(&pk)
    {
        return proxy_request(&state, "GET", original.to_string()).await;
    }
    let issue_id = issue_id.parse::<Uuid>().expect("checked segment");
    let pk = pk.parse::<Uuid>().expect("checked segment");
    match activity_detail_inner(&state, &headers, &slug, &project_id, &issue_id, &pk, &query).await
    {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

async fn activity_detail_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    project_id_raw: &str,
    issue_id: &Uuid,
    pk: &Uuid,
    query: &QueryMap,
) -> Result<Response, Denial> {
    use pidash_services::v1_work_items::shape_social::ACTIVITY_READ_FIELDS;
    use pidash_services::v1_work_items::{filter_fields, FieldSpec};
    let pre = preamble(state, headers, slug).await?;
    let project_id = rewrite_project_id(&pre.pool, slug, project_id_raw).await?;
    let workspace_id = pre.workspace_id.ok_or(Denial::Forbidden)?;
    require_gate(
        &pre.pool,
        &workspace_id,
        slug,
        &pre.actor.id,
        &project_id,
        V1WorkItemsRoute::ActivityDetail,
        "GET",
    )
    .await?;
    // `TimezoneMixin.initial` runs after the gate but before the body: a
    // bad zone wins over the row 404 below.
    let tz = activate_timezone(pre.actor.timezone.as_deref())?;
    let order = resolve_activity_order(query_last(query, "order_by").as_deref())?;
    let row = fetch_activity_detail(
        &pre.pool,
        slug,
        &project_id,
        issue_id,
        &pre.actor.id,
        pk,
        &order,
    )
    .await?;
    let Some(row) = row else {
        return Err(Denial::NotFound(
            queries_sub::ACTIVITY_NOT_FOUND_BODY.to_owned(),
        ));
    };
    let fields = fields_param(query, "fields");
    let expand = fields_param(query, "expand");
    let specs: Vec<FieldSpec> = fields
        .as_deref()
        .unwrap_or(&[])
        .iter()
        .map(|name| FieldSpec::Include(name.clone()))
        .collect();
    let field_specs = if fields.is_some() {
        Some(specs.as_slice())
    } else {
        None
    };
    let expand_refs: Vec<&str> = expand
        .as_deref()
        .unwrap_or(&[])
        .iter()
        .map(String::as_str)
        .collect();
    let kept = filter_fields(ACTIVITY_READ_FIELDS, field_specs).map_err(|_| Denial::ServerError)?;
    let web_base = pidash_services::v1_work_items::shape_issue::web_base_url(
        state.settings().urls.web_url.as_deref(),
        state.settings().urls.app_base_url.as_deref(),
    );
    let decoded = decode_activity(&row, &tz)?;
    let expansions = activity_expansions(
        &pre.pool,
        &row,
        slug,
        &tz,
        web_base.as_deref(),
        &expand_refs,
        &kept,
    )
    .await?;
    let value = render_activity_value(&decoded, field_specs, &expand_refs, &expansions)?;
    let body = serde_json::to_string(&value).map_err(|_| Denial::ServerError)?;
    Ok(json_response(StatusCode::OK, body))
}

/// `GET .../attachments/` (`views/issue.py:2432-2447`).
pub async fn get_attachment_list(
    State(state): State<AppState>,
    OriginalUri(original): OriginalUri,
    Path((slug, project_id, issue_id)): Path<(String, String, String)>,
    headers: HeaderMap,
) -> Response {
    if !crate::runner_runs::is_uuid_path_segment(&issue_id) {
        return proxy_request(&state, "GET", original.to_string()).await;
    }
    let issue_id = issue_id.parse::<Uuid>().expect("checked segment");
    match attachment_list_inner(&state, &headers, &slug, &project_id, &issue_id).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

async fn attachment_list_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    project_id_raw: &str,
    issue_id: &Uuid,
) -> Result<Response, Denial> {
    let pre = preamble(state, headers, slug).await?;
    let project_id = rewrite_project_id(&pre.pool, slug, project_id_raw).await?;
    // No gate: the view declares no `permission_classes`, so only
    // `IsAuthenticated` runs — and no member/archived filter either
    // (BUG-1). `TimezoneMixin.initial` still activates for the
    // authenticated caller.
    let tz = activate_timezone(pre.actor.timezone.as_deref())?;
    let rows = fetch_attachment_rows(&pre.pool, slug, &project_id, issue_id).await?;
    let mut rendered: Vec<Value> = Vec::with_capacity(rows.len());
    for row in rows {
        let decoded = decode_attachment(&row, &tz)?;
        rendered.push(render_attachment_value(&decoded)?);
    }
    let body = serde_json::to_string(&Value::Array(rendered)).map_err(|_| Denial::ServerError)?;
    Ok(json_response(StatusCode::OK, body))
}

/// `POST .../attachments/` (`views/issue.py:2312-2430`): validate, mint
/// the `FileAsset` row, presign the upload, answer 200.
pub async fn post_attachment(
    State(state): State<AppState>,
    OriginalUri(original): OriginalUri,
    Path((slug, project_id, issue_id)): Path<(String, String, String)>,
    headers: HeaderMap,
    body: axum::body::Body,
) -> Response {
    if !crate::runner_runs::is_uuid_path_segment(&issue_id) {
        return proxy_request(&state, "POST", original.to_string()).await;
    }
    let issue_id = issue_id.parse::<Uuid>().expect("checked segment");
    let bytes = match read_body(body).await {
        Ok(bytes) => bytes,
        Err(denial) => return denial.into_response(),
    };
    match attachment_post_inner(&state, &headers, &slug, &project_id, &issue_id, &bytes).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

async fn attachment_post_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    project_id_raw: &str,
    issue_id: &Uuid,
    body: &[u8],
) -> Result<Response, Denial> {
    let pre = preamble(state, headers, slug).await?;
    let project_id = rewrite_project_id(&pre.pool, slug, project_id_raw).await?;
    // `TimezoneMixin.initial` (`views/base.py:43-46`) runs before the view
    // body, so a bad zone wins over the inline 404/403 below (these views
    // carry `IsAuthenticated` only; the issue/permission checks are body
    // code).
    let tz = activate_timezone(pre.actor.timezone.as_deref())?;
    // No gate (`IsAuthenticated` only); the permission check is inline.
    let Some((_, created_by_id)) = fetch_issue_head(&pre.pool, slug, &project_id, issue_id).await?
    else {
        return Err(Denial::NotFound(RESOURCE_NOT_FOUND_BODY.to_owned()));
    };
    if !issue_permission(
        &pre.pool,
        &pre.actor.id,
        created_by_id,
        &project_id,
        Some(ATTACHMENT_ROLES),
        true,
    )
    .await?
    {
        return Err(Denial::ForbiddenUpload);
    }
    // The body parses here (`request.data` is first touched at `:2332`):
    // a 400/415/500 from the bytes never precedes the 404/403 above.
    let data = parse_post_data(headers, body)?;
    // An uploaded file is truthy in `request.data` (and wins over texts
    // for its key), so the `if not name or not size` check still 400s
    // when a file sits on one key and the OTHER field is missing — only
    // once both read truthy do the file values trip `min()`'s `TypeError`
    // 500 (a multipart `size` is a file or a text string either way).
    // `type` files miss the MIME list (400); externals stringify like
    // `CharField` prep. (PIDASHCONV-789#3, live-probed.)
    let name_value = data.map.get("name").cloned().unwrap_or(Value::Null);
    let size_value = data.map.get("size").cloned().unwrap_or(Value::Null);
    if !(data.files.contains_key("name") || is_truthy(&name_value))
        || !(data.files.contains_key("size") || is_truthy(&size_value))
    {
        return Ok(json_response(
            StatusCode::BAD_REQUEST,
            INVALID_REQUEST_BODY.to_owned(),
        ));
    }
    if data.files.contains_key("size") || data.files.contains_key("name") {
        return Err(Denial::ServerError);
    }
    // Float-spelled numbers (`1e2`) read as floats from here on: the
    // `min` winner, the policy, the key and the row echo all carry
    // CPython's spelling (`100.0`).
    let name_value = normalize_float_spelling(&name_value);
    let size_value = normalize_float_spelling(&size_value);
    // `request.data.get("type", False)` (`:2335`): missing reads as
    // `False`, which then misses the MIME list.
    let mime_value = data.map.get("type").cloned().unwrap_or(Value::Bool(false));
    let plan = work_tasks::attachment_post_plan(
        &name_value,
        &size_value,
        &mime_value,
        state.settings().file_size_limit,
    );
    let size_limit = match plan {
        work_tasks::AttachmentPostPlan::InvalidRequest => {
            return Ok(json_response(
                StatusCode::BAD_REQUEST,
                INVALID_REQUEST_BODY.to_owned(),
            ));
        }
        work_tasks::AttachmentPostPlan::SizeTypeError => return Err(Denial::ServerError),
        work_tasks::AttachmentPostPlan::InvalidType => {
            return Ok(json_response(
                StatusCode::BAD_REQUEST,
                INVALID_FILE_TYPE_BODY.to_owned(),
            ));
        }
        work_tasks::AttachmentPostPlan::Ready { size_limit } => size_limit,
    };
    // The plan passed, so `type` is a listed MIME string; `name` stays
    // verbatim (any truthy JSON value).
    let mime = mime_value.as_str().expect("planned MIME");
    let Some(workspace_id) = fetch_workspace_id(&pre.pool, slug).await? else {
        return Err(Denial::NotFound(RESOURCE_NOT_FOUND_BODY.to_owned()));
    };
    let external_id = external_text(data.map.get("external_id"), data.files.get("external_id"));
    let external_source = external_text(
        data.map.get("external_source"),
        data.files.get("external_source"),
    );
    // The dup probe runs only when both raw values are truthy; `filter()`
    // stringifies non-strings through `CharField` prep (no 500).
    let raw_id = data.map.get("external_id").cloned().unwrap_or(Value::Null);
    let raw_source = data
        .map
        .get("external_source")
        .cloned()
        .unwrap_or(Value::Null);
    let probe = (is_truthy(&raw_id) || data.files.contains_key("external_id"))
        && (is_truthy(&raw_source) || data.files.contains_key("external_source"));
    if probe {
        let id_text = external_id.clone().unwrap_or_default();
        let source_text = external_source.clone().unwrap_or_default();
        if let Some(dup) = fetch_external_duplicate(
            &pre.pool,
            slug,
            &project_id,
            issue_id,
            &source_text,
            &id_text,
        )
        .await?
        {
            let mut map = Map::with_capacity(2);
            map.insert(
                "error".to_owned(),
                Value::String(EXTERNAL_DUP_MESSAGE.to_owned()),
            );
            map.insert("id".to_owned(), Value::String(dup.to_string()));
            let body =
                serde_json::to_string(&Value::Object(map)).map_err(|_| Denial::ServerError)?;
            return Err(Denial::Conflict(body));
        }
    }
    // `FileAsset.objects.create(...)` (`:2386-2401`): explicit nulls and
    // model defaults, `created_at`/`updated_at` from separate `now()`s
    // (`auto_now_add`/`auto_now` evaluate independently).
    let asset_id = Uuid::new_v4();
    let created_at = Utc::now();
    let updated_at = Utc::now();
    let name_text = py_str(&name_value);
    // The key hex is an INDEPENDENT uuid4 (`:2357` mints it before the
    // row exists — the row id comes from the DB default), never the row
    // id.
    let key = work_tasks::attachment_asset_key(
        &workspace_id.to_string(),
        &Uuid::new_v4().simple().to_string(),
        &name_text,
    );
    let mut attributes = Map::with_capacity(3);
    attributes.insert("name".to_owned(), name_value);
    attributes.insert("type".to_owned(), Value::String(mime.to_owned()));
    attributes.insert("size".to_owned(), size_limit.clone());
    // `json.dumps(attributes)` (`:2387` JSONField prep) renders ±inf as
    // `Infinity`, which the jsonb cast rejects (`DataError` 500). Runs
    // after the dup probe (`:2353-2384` precedes `:2386`).
    if attributes_dump_fails(&attributes) {
        return Err(Denial::ServerError);
    }
    let size_f64 = size_limit_f64(&size_limit)?;
    let storage = &state.settings().storage;
    let insert = sqlx::query(
        r#"INSERT INTO "file_assets"
           ("id", "attributes", "asset", "size", "workspace_id", "created_by_id", "issue_id",
            "project_id", "entity_type", "external_id", "external_source", "is_uploaded",
            "storage_metadata", "is_deleted", "is_archived", "created_at", "updated_at",
            "deleted_at", "updated_by_id", "user_id", "comment_id", "page_id",
            "draft_issue_id", "entity_identifier")
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 'ISSUE_ATTACHMENT', $9, $10, FALSE, '{}', FALSE, FALSE, $11, $12,
                   NULL, NULL, NULL, NULL, NULL, NULL, NULL)"#,
    )
    .bind(asset_id)
    .bind(Value::Object(attributes.clone()))
    .bind(&key)
    .bind(size_f64)
    .bind(workspace_id)
    .bind(pre.actor.id)
    .bind(issue_id)
    .bind(project_id)
    .bind(external_id.as_deref())
    .bind(external_source.as_deref())
    .bind(created_at)
    .bind(updated_at)
    .execute(&pre.pool)
    .await;
    if let Err(error) = insert {
        // `IntegrityError` (a `23xxx` code — the concurrent-delete race on
        // the FKs) answers the payload-valid 400; every other driver
        // failure is the generic 500.
        let integrity = matches!(&error, sqlx::Error::Database(db) if db.code().is_some_and(|code| code.starts_with("23")));
        if integrity {
            return Ok(json_response(
                StatusCode::BAD_REQUEST,
                r#"{"error":"The payload is not valid"}"#.to_owned(),
            ));
        }
        return Err(db_error(error, "attachment-insert"));
    }
    // The signability check runs AFTER the INSERT: Django creates the
    // row first (`:2386-2401`), then builds the client (`S3Storage(...)`
    // raises on empty credentials) — a broken config 500s with the row
    // already stored.
    if !storage_can_sign(storage) {
        return Err(Denial::ServerError);
    }
    let scheme = scheme_of(headers);
    let host = host_of(headers);
    let Some(upload_data) = presigned_post(
        storage,
        &scheme,
        host.as_deref(),
        &key,
        mime,
        &policy_number(&size_limit),
        &Utc::now(),
    ) else {
        return Err(Denial::ServerError);
    };
    // The 200 renders the in-memory row (`serializer(asset)` before any
    // re-fetch — `attributes` keeps insertion order, `storage_metadata`
    // the `{}` default), keys `upload_data`, `asset_id`, `attachment`,
    // `asset_url` in order (`:2407-2429`).
    let decoded = DecodedAttachment {
        id: asset_id.to_string(),
        created_at: crate::serializer::render_datetime_in(&created_at, &tz),
        updated_at: crate::serializer::render_datetime_in(&updated_at, &tz),
        deleted_at: None,
        attributes: Value::Object(attributes),
        asset: key,
        entity_type: Some("ISSUE_ATTACHMENT".to_owned()),
        entity_identifier: None,
        is_deleted: false,
        is_archived: false,
        external_id,
        external_source,
        size: size_f64,
        is_uploaded: false,
        storage_metadata: Some(Value::Object(Map::new())),
        created_by: Some(pre.actor.id.to_string()),
        updated_by: None,
        user: None,
        workspace: Some(workspace_id.to_string()),
        draft_issue: None,
        project: Some(project_id.to_string()),
        issue: Some(issue_id.to_string()),
        comment: None,
        page: None,
    };
    let attachment = render_attachment_value(&decoded)?;
    let mut map = Map::with_capacity(4);
    map.insert("upload_data".to_owned(), upload_data);
    map.insert("asset_id".to_owned(), Value::String(asset_id.to_string()));
    map.insert("attachment".to_owned(), attachment);
    map.insert(
        "asset_url".to_owned(),
        Value::String(issue_attachment_asset_url(
            slug,
            &project_id,
            issue_id,
            &asset_id,
        )),
    );
    let body = serde_json::to_string(&Value::Object(map)).map_err(|_| Denial::ServerError)?;
    Ok(json_response(StatusCode::OK, body))
}

/// `CharField` prep for `external_id`/`external_source`: `None` stays
/// NULL, strings pass through, everything else stringifies (a file value
/// stringifies to its name).
fn external_text(value: Option<&Value>, file: Option<&String>) -> Option<String> {
    if let Some(filename) = file {
        return Some(filename.clone());
    }
    match value {
        None | Some(Value::Null) => None,
        Some(Value::String(text)) => Some(text.clone()),
        Some(other) => Some(py_str(other)),
    }
}

/// `GET .../attachments/<pk>/` (`views/issue.py:2552-2585`): presign and
/// 302-redirect to the object.
pub async fn get_attachment(
    State(state): State<AppState>,
    OriginalUri(original): OriginalUri,
    Path((slug, project_id, issue_id, pk)): Path<(String, String, String, String)>,
    headers: HeaderMap,
) -> Response {
    if !crate::runner_runs::is_uuid_path_segment(&issue_id)
        || !crate::runner_runs::is_uuid_path_segment(&pk)
    {
        return proxy_request(&state, "GET", original.to_string()).await;
    }
    let issue_id = issue_id.parse::<Uuid>().expect("checked segment");
    let pk = pk.parse::<Uuid>().expect("checked segment");
    match attachment_get_inner(&state, &headers, &slug, &project_id, &issue_id, &pk).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

/// `asset.attributes.get("name")` (`views/issue.py:2571-2577`) into the
/// `filename` for [`presigned_get_url`]: a non-object `attributes` has no
/// `.get` (`AttributeError` 500); missing/null mints a fresh hex per call
/// (`None`); a truthy non-string hits `quote()`'s `TypeError` 500; a
/// falsy non-string takes the bare-disposition arm (`Some("")` — `if
/// filename:` is false, `storage.py:115-124`).
fn download_filename(attributes: &Value) -> Result<Option<String>, Denial> {
    let map = attributes.as_object().ok_or(Denial::ServerError)?;
    match map.get("name") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(name)) => Ok(Some(name.clone())),
        Some(other) => {
            if is_truthy(other) {
                Err(Denial::ServerError)
            } else {
                Ok(Some(String::new()))
            }
        }
    }
}

async fn attachment_get_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    project_id_raw: &str,
    _issue_id: &Uuid,
    pk: &Uuid,
) -> Result<Response, Denial> {
    let pre = preamble(state, headers, slug).await?;
    let project_id = rewrite_project_id(&pre.pool, slug, project_id_raw).await?;
    // `TimezoneMixin.initial` (`views/base.py:43-46`) runs before the view
    // body, so a bad zone wins over the inline 404/403 below (these views
    // carry `IsAuthenticated` only; the issue/permission checks are body
    // code).
    let _tz = activate_timezone(pre.actor.timezone.as_deref())?;
    // The download check passes `issue=None` (creator arm off) and no
    // roles (any active project membership): `has_issue_download_access`.
    if !issue_permission(&pre.pool, &pre.actor.id, None, &project_id, None, false).await? {
        return Err(Denial::ForbiddenDownload);
    }
    let Some(row) = fetch_attachment(&pre.pool, slug, &project_id, pk).await? else {
        return Err(Denial::NotFound(RESOURCE_NOT_FOUND_BODY.to_owned()));
    };
    let is_uploaded: bool = row
        .try_get("is_uploaded")
        .map_err(|_| Denial::ServerError)?;
    if !is_uploaded {
        return Ok(json_response(
            StatusCode::BAD_REQUEST,
            ASSET_NOT_UPLOADED_BODY.to_owned(),
        ));
    }
    let attributes: Value = row.try_get("attributes").map_err(|_| Denial::ServerError)?;
    let filename = download_filename(&attributes)?;
    let storage = &state.settings().storage;
    if !storage_can_sign(storage) {
        return Err(Denial::ServerError);
    }
    let scheme = scheme_of(headers);
    let host = host_of(headers);
    let key: String = row_string(&row, "asset")?;
    let Some(location) = presigned_get_url(
        storage,
        &scheme,
        host.as_deref(),
        &key,
        filename.as_deref(),
        &Utc::now(),
    ) else {
        return Err(Denial::ServerError);
    };
    // `HttpResponseRedirect`: 302, `Location`, `text/html`, empty body.
    Ok(Response::builder()
        .status(StatusCode::FOUND)
        .header(header::LOCATION, location)
        .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
        .body(axum::body::Body::empty())
        .expect("redirect response"))
}

/// `PATCH .../attachments/<pk>/` (`views/issue.py:2606-2651`): confirm
/// the upload, 204.
pub async fn patch_attachment(
    State(state): State<AppState>,
    OriginalUri(original): OriginalUri,
    Path((slug, project_id, issue_id, pk)): Path<(String, String, String, String)>,
    headers: HeaderMap,
    body: axum::body::Body,
) -> Response {
    if !crate::runner_runs::is_uuid_path_segment(&issue_id)
        || !crate::runner_runs::is_uuid_path_segment(&pk)
    {
        return proxy_request(&state, "PATCH", original.to_string()).await;
    }
    let issue_id = issue_id.parse::<Uuid>().expect("checked segment");
    let pk = pk.parse::<Uuid>().expect("checked segment");
    // The view never reads the body: drain it unread (malformed bytes
    // still confirm).
    if read_body(body).await.is_err() {
        return Denial::ServerError.into_response();
    }
    match attachment_patch_inner(&state, &headers, &slug, &project_id, &issue_id, &pk).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

async fn attachment_patch_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    project_id_raw: &str,
    issue_id: &Uuid,
    pk: &Uuid,
) -> Result<Response, Denial> {
    let pre = preamble(state, headers, slug).await?;
    let project_id = rewrite_project_id(&pre.pool, slug, project_id_raw).await?;
    // `TimezoneMixin.initial` (`views/base.py:43-46`) runs before the view
    // body, so a bad zone wins over the inline 404/403 below (these views
    // carry `IsAuthenticated` only; the issue/permission checks are body
    // code).
    let tz = activate_timezone(pre.actor.timezone.as_deref())?;
    let Some((_, created_by_id)) = fetch_issue_head(&pre.pool, slug, &project_id, issue_id).await?
    else {
        return Err(Denial::NotFound(RESOURCE_NOT_FOUND_BODY.to_owned()));
    };
    if !issue_permission(
        &pre.pool,
        &pre.actor.id,
        created_by_id,
        &project_id,
        Some(ATTACHMENT_ROLES),
        true,
    )
    .await?
    {
        return Err(Denial::ForbiddenUpload);
    }
    let Some(row) = fetch_attachment(&pre.pool, slug, &project_id, pk).await? else {
        return Err(Denial::NotFound(RESOURCE_NOT_FOUND_BODY.to_owned()));
    };
    // `current_instance` serializes the row BEFORE the update (`:2634`,
    // `:2637`): still un-uploaded, still the old creator.
    let decoded = decode_attachment(&row, &tz)?;
    let current = render_attachment_for_dump(&decoded)?;
    let current_instance = finalize_nonfinite(cpython_dumps(&current));
    let storage_metadata: Option<Value> = row
        .try_get("storage_metadata")
        .map_err(|_| Denial::ServerError)?;
    let now = Utc::now();
    if !decoded.is_uploaded {
        let origin = app_origin(&state.settings().urls)?;
        let kwargs = work_tasks::issue_activity_notify_kwargs(
            work_tasks::ACTIVITY_ATTACHMENT_CREATED,
            None,
            &pre.actor.id.to_string(),
            &issue_id.to_string(),
            &project_id.to_string(),
            Some(&current_instance),
            now.timestamp(),
            true,
            &origin,
        );
        enqueue_best_effort(&pre.pool, work_tasks::ISSUE_ACTIVITY_TASK, vec![], kwargs).await;
    }
    if work_tasks::storage_metadata_missing(storage_metadata.as_ref()) {
        let args = work_tasks::asset_metadata_args(&pk.to_string());
        enqueue_best_effort(
            &pre.pool,
            work_tasks::GET_ASSET_OBJECT_METADATA_TASK,
            args,
            Map::new(),
        )
        .await;
    }
    // `.save()` always runs: `updated_at`/`updated_by` move even when
    // already uploaded (`BaseModel.save` auto-user).
    sqlx::query(
        r#"UPDATE "file_assets" SET "is_uploaded" = TRUE, "created_by_id" = CASE WHEN "is_uploaded" THEN "created_by_id" ELSE $1 END,
                  "updated_by_id" = $1, "updated_at" = $2 WHERE "id" = $3"#,
    )
    .bind(pre.actor.id)
    .bind(now)
    .bind(pk)
    .execute(&pre.pool)
    .await
    .map_err(|error| db_error(error, "attachment-confirm"))?;
    Ok(Response::builder()
        .status(StatusCode::NO_CONTENT)
        .body(axum::body::Body::empty())
        .expect("empty response"))
}

/// `DELETE .../attachments/<pk>/` (`views/issue.py:2468-2550`): soft
/// delete, 204.
pub async fn delete_attachment(
    State(state): State<AppState>,
    OriginalUri(original): OriginalUri,
    Path((slug, project_id, issue_id, pk)): Path<(String, String, String, String)>,
    headers: HeaderMap,
    body: axum::body::Body,
) -> Response {
    if !crate::runner_runs::is_uuid_path_segment(&issue_id)
        || !crate::runner_runs::is_uuid_path_segment(&pk)
    {
        return proxy_request(&state, "DELETE", original.to_string()).await;
    }
    let issue_id = issue_id.parse::<Uuid>().expect("checked segment");
    let pk = pk.parse::<Uuid>().expect("checked segment");
    if read_body(body).await.is_err() {
        return Denial::ServerError.into_response();
    }
    match attachment_delete_inner(&state, &headers, &slug, &project_id, &issue_id, &pk).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

async fn attachment_delete_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    project_id_raw: &str,
    issue_id: &Uuid,
    pk: &Uuid,
) -> Result<Response, Denial> {
    let pre = preamble(state, headers, slug).await?;
    let project_id = rewrite_project_id(&pre.pool, slug, project_id_raw).await?;
    // `TimezoneMixin.initial` (`views/base.py:43-46`) runs before the view
    // body, so a bad zone wins over the inline 404/403 below (these views
    // carry `IsAuthenticated` only; the issue/permission checks are body
    // code).
    let _tz = activate_timezone(pre.actor.timezone.as_deref())?;
    let Some((_, created_by_id)) = fetch_issue_head(&pre.pool, slug, &project_id, issue_id).await?
    else {
        return Err(Denial::NotFound(RESOURCE_NOT_FOUND_BODY.to_owned()));
    };
    if !issue_permission(
        &pre.pool,
        &pre.actor.id,
        created_by_id,
        &project_id,
        Some(ATTACHMENT_ROLES),
        true,
    )
    .await?
    {
        return Err(Denial::ForbiddenDelete);
    }
    let Some(row) = fetch_attachment(&pre.pool, slug, &project_id, pk).await? else {
        return Err(Denial::NotFound(RESOURCE_NOT_FOUND_BODY.to_owned()));
    };
    // The two `.save()`s collapse into one UPDATE (`is_deleted`,
    // `deleted_at`, `updated_by` via auto-user, `updated_at`); the
    // response carries no row, so the merged timestamp is unobservable.
    let now = Utc::now();
    sqlx::query(
        r#"UPDATE "file_assets" SET "is_deleted" = TRUE, "deleted_at" = $1, "updated_by_id" = $2, "updated_at" = $1 WHERE "id" = $3"#,
    )
    .bind(now)
    .bind(pre.actor.id)
    .bind(pk)
    .execute(&pre.pool)
    .await
    .map_err(|error| db_error(error, "attachment-delete"))?;
    let origin = app_origin(&state.settings().urls)?;
    let kwargs = work_tasks::issue_activity_notify_kwargs(
        work_tasks::ACTIVITY_ATTACHMENT_DELETED,
        None,
        &pre.actor.id.to_string(),
        &issue_id.to_string(),
        &project_id.to_string(),
        None,
        now.timestamp(),
        true,
        &origin,
    );
    enqueue_best_effort(&pre.pool, work_tasks::ISSUE_ACTIVITY_TASK, vec![], kwargs).await;
    let storage_metadata: Option<Value> = row
        .try_get("storage_metadata")
        .map_err(|_| Denial::ServerError)?;
    if work_tasks::storage_metadata_missing(storage_metadata.as_ref()) {
        let args = work_tasks::asset_metadata_args(&pk.to_string());
        enqueue_best_effort(
            &pre.pool,
            work_tasks::GET_ASSET_OBJECT_METADATA_TASK,
            args,
            Map::new(),
        )
        .await;
    }
    Ok(Response::builder()
        .status(StatusCode::NO_CONTENT)
        .body(axum::body::Body::empty())
        .expect("empty response"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    const F18_11: &str =
        include_str!("../../../../fixtures/v1_work_items/handlers/F18-11.work_items.json");
    const F18_10: &str = include_str!("../../../../fixtures/v1_work_items/tasks/F18-10.tasks.json");

    fn calls() -> Map<String, Value> {
        let fixture: Value = serde_json::from_str(F18_11).expect("fixture parses");
        fixture
            .get("calls")
            .and_then(Value::as_object)
            .cloned()
            .expect("calls map")
    }

    fn call(name: &str) -> Value {
        calls().get(name).cloned().unwrap_or(Value::Null)
    }

    fn f18_10_units() -> Map<String, Value> {
        let fixture: Value = serde_json::from_str(F18_10).expect("fixture parses");
        fixture
            .get("units")
            .and_then(Value::as_object)
            .cloned()
            .expect("units map")
    }

    fn str_field(body: &Value, key: &str) -> String {
        body.get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    }

    fn opt_str_field(body: &Value, key: &str) -> Option<String> {
        body.get(key).and_then(|v| match v {
            Value::Null => None,
            Value::String(text) => Some(text.clone()),
            _ => None,
        })
    }

    fn decoded_activity(body: &Value) -> DecodedActivity {
        let attachments = body
            .get("attachments")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
            .iter()
            .map(|item| item.as_str().map(str::to_owned))
            .collect();
        DecodedActivity {
            id: str_field(body, "id"),
            created_at: str_field(body, "created_at"),
            updated_at: str_field(body, "updated_at"),
            deleted_at: opt_str_field(body, "deleted_at"),
            verb: str_field(body, "verb"),
            field: opt_str_field(body, "field"),
            old_value: opt_str_field(body, "old_value"),
            new_value: opt_str_field(body, "new_value"),
            comment: str_field(body, "comment"),
            attachments,
            old_identifier: opt_str_field(body, "old_identifier"),
            new_identifier: opt_str_field(body, "new_identifier"),
            epoch: body.get("epoch").and_then(Value::as_f64),
            project: str_field(body, "project"),
            workspace: str_field(body, "workspace"),
            issue: opt_str_field(body, "issue"),
            issue_comment: opt_str_field(body, "issue_comment"),
            actor: opt_str_field(body, "actor"),
        }
    }

    fn decoded_attachment(body: &Value) -> DecodedAttachment {
        DecodedAttachment {
            id: str_field(body, "id"),
            created_at: str_field(body, "created_at"),
            updated_at: str_field(body, "updated_at"),
            deleted_at: opt_str_field(body, "deleted_at"),
            attributes: body.get("attributes").cloned().unwrap_or(Value::Null),
            asset: str_field(body, "asset"),
            entity_type: opt_str_field(body, "entity_type"),
            entity_identifier: opt_str_field(body, "entity_identifier"),
            is_deleted: body
                .get("is_deleted")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            is_archived: body
                .get("is_archived")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            external_id: opt_str_field(body, "external_id"),
            external_source: opt_str_field(body, "external_source"),
            size: body.get("size").and_then(Value::as_f64).unwrap_or(0.0),
            is_uploaded: body
                .get("is_uploaded")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            storage_metadata: body.get("storage_metadata").cloned(),
            created_by: opt_str_field(body, "created_by"),
            updated_by: opt_str_field(body, "updated_by"),
            user: opt_str_field(body, "user"),
            workspace: opt_str_field(body, "workspace"),
            draft_issue: opt_str_field(body, "draft_issue"),
            project: opt_str_field(body, "project"),
            issue: opt_str_field(body, "issue"),
            comment: opt_str_field(body, "comment"),
            page: opt_str_field(body, "page"),
        }
    }

    fn keys_in_order(map: &Map<String, Value>) -> Vec<String> {
        map.keys().cloned().collect()
    }

    // --- F18-11 replays: status + body byte-identical per route ---

    #[tokio::test]
    async fn replay_activity_list_envelope_and_rows() {
        let record = call("activity_list");
        assert_eq!(record.get("status").and_then(Value::as_u64), Some(200));
        let recorded = record.get("envelope").expect("envelope");
        let expected_keys = [
            "grouped_by",
            "sub_grouped_by",
            "total_count",
            "next_cursor",
            "prev_cursor",
            "next_page_results",
            "prev_page_results",
            "count",
            "total_pages",
            "total_results",
            "extra_stats",
        ];
        assert_eq!(
            keys_in_order(recorded.as_object().expect("object")),
            expected_keys
                .iter()
                .map(|key| key.to_string())
                .collect::<Vec<_>>()
        );
        assert_eq!(
            recorded.get("next_cursor").and_then(Value::as_str),
            Some("1000:1:0")
        );
        assert_eq!(
            recorded.get("prev_cursor").and_then(Value::as_str),
            Some("1000:-1:1")
        );
        assert_eq!(
            recorded.get("total_results").and_then(Value::as_i64),
            Some(7)
        );
        // The two full rows round-trip byte-identically through the row
        // mapping + shape (volatile values ride in from the fixture, so
        // the test pins mapping, key order and render).
        let full = record
            .get("results_full")
            .and_then(Value::as_array)
            .expect("results_full");
        assert_eq!(full.len(), 2);
        for row in full {
            let decoded = decoded_activity(row);
            let rendered = render_activity_value(&decoded, None, &[], &[]).expect("render");
            assert_eq!(
                serde_json::to_string(&rendered).expect("json"),
                serde_json::to_string(row).expect("json"),
            );
        }
        assert_eq!(
            record
                .get("results_rest_ids")
                .and_then(Value::as_array)
                .map(Vec::len),
            Some(5)
        );
        // The envelope builder reproduces the recorded envelope scalars
        // and key order for the same inputs (only `count`/`results`
        // differ — the fixture carries two full rows of seven).
        let rendered_rows: Vec<Value> = full
            .iter()
            .map(|row| {
                render_activity_value(&decoded_activity(row), None, &[], &[]).expect("render")
            })
            .collect();
        let next = crate::paginator::next_cursor(1000, 0, false);
        let prev = crate::paginator::prev_cursor(1000, 0);
        let response =
            envelope(7, 1000, &next, &prev, Value::Array(rendered_rows)).expect("envelope");
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let mine: Value = serde_json::from_slice(&bytes).expect("envelope parses");
        for key in expected_keys {
            if key == "count" || key == "results" {
                continue;
            }
            assert_eq!(mine.get(key), recorded.get(key), "{key}");
        }
        // Same key order (plus the trailing `results` the recording
        // splits out).
        let mine_ordered: Vec<String> = keys_in_order(mine.as_object().expect("object"))
            .into_iter()
            .filter(|key| expected_keys.contains(&key.as_str()))
            .collect();
        assert_eq!(
            mine_ordered,
            expected_keys
                .iter()
                .map(|key| key.to_string())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn replay_activity_detail_body() {
        let record = call("activity_detail");
        assert_eq!(record.get("status").and_then(Value::as_u64), Some(200));
        let body = record.get("body").expect("body");
        let decoded = decoded_activity(body);
        let rendered = render_activity_value(&decoded, None, &[], &[]).expect("render");
        assert_eq!(
            serde_json::to_string(&rendered).expect("json"),
            serde_json::to_string(body).expect("json"),
        );
    }

    #[test]
    fn replay_attachment_list_body() {
        let record = call("attachment_list");
        assert_eq!(record.get("status").and_then(Value::as_u64), Some(200));
        let body = record
            .get("body")
            .and_then(Value::as_array)
            .expect("list body");
        assert!(!body.is_empty());
        for item in body {
            let decoded = decoded_attachment(item);
            let rendered = render_attachment_value(&decoded).expect("render");
            assert_eq!(
                serde_json::to_string(&rendered).expect("json"),
                serde_json::to_string(item).expect("json"),
            );
        }
    }

    /// The recorded POST `response_body` (F18-10 process log) pins the
    /// top-level key order and the presigned field order.
    #[test]
    fn replay_attachment_post_key_order() {
        let units = f18_10_units();
        let unit = units.get("attachment_post_presign").expect("unit");
        let body = unit
            .get("messages")
            .and_then(Value::as_array)
            .and_then(|m| m.first())
            .and_then(|m| m.get("kwargs"))
            .and_then(|k| k.get("log_data"))
            .and_then(|l| l.get("response_body"))
            .and_then(Value::as_str)
            .expect("response_body");
        let parsed: Value = serde_json::from_str(body).expect("response parses");
        assert_eq!(
            keys_in_order(parsed.as_object().expect("object")),
            ["upload_data", "asset_id", "attachment", "asset_url"]
        );
        let upload = parsed.get("upload_data").expect("upload_data");
        assert_eq!(
            keys_in_order(upload.as_object().expect("object")),
            ["url", "fields"]
        );
        assert_eq!(
            keys_in_order(
                upload
                    .get("fields")
                    .and_then(Value::as_object)
                    .expect("fields")
            ),
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
        // The recorded attachment renders byte-identically too.
        let attachment = parsed.get("attachment").expect("attachment");
        let decoded = decoded_attachment(attachment);
        let rendered = render_attachment_value(&decoded).expect("render");
        assert_eq!(
            serde_json::to_string(&rendered).expect("json"),
            serde_json::to_string(attachment).expect("json"),
        );
    }

    /// The presigned policy bytes match botocore's capture exactly (the
    /// signature itself needs the unrecorded secret, so the policy —
    /// which the secret never touches — is the oracle).
    #[test]
    fn replay_presigned_post_policy_bytes() {
        let units = f18_10_units();
        let unit = units.get("attachment_post_presign").expect("unit");
        let body = unit
            .get("messages")
            .and_then(Value::as_array)
            .and_then(|m| m.first())
            .and_then(|m| m.get("kwargs"))
            .and_then(|k| k.get("log_data"))
            .and_then(|l| l.get("response_body"))
            .and_then(Value::as_str)
            .expect("response_body");
        let parsed: Value = serde_json::from_str(body).expect("response parses");
        let recorded_fields = parsed
            .get("upload_data")
            .and_then(|u| u.get("fields"))
            .and_then(Value::as_object)
            .expect("recorded fields");
        let recorded_key = recorded_fields
            .get("key")
            .and_then(Value::as_str)
            .expect("recorded key");
        // The F18-10 capture ran at 23:36:29Z in MinIO mode against
        // the runserver host with `size: 100` (its own request — not the
        // F18-11 presign_full inputs).
        let storage = pidash_db::config::StorageSettings {
            use_minio: true,
            minio_endpoint_ssl: false,
            access_key_id: "conv659test".to_owned(),
            secret_access_key: "unrecorded-secret".to_owned(),
            bucket_name: "conv659".to_owned(),
            region: "us-east-1".to_owned(),
            endpoint_url: None,
            signed_url_expiration_secs: 3600,
        };
        let now = chrono::Utc
            .with_ymd_and_hms(2026, 10, 2, 23, 36, 29)
            .single()
            .expect("recorded instant");
        let out = presigned_post(
            &storage,
            "http",
            Some("127.0.0.1:18359"),
            recorded_key,
            "application/pdf",
            "100",
            &now,
        )
        .expect("signs");
        let mine = out
            .get("fields")
            .and_then(Value::as_object)
            .expect("fields");
        assert_eq!(
            mine.get("policy"),
            recorded_fields.get("policy"),
            "policy bytes match botocore's capture"
        );
        assert_eq!(
            mine.get("x-amz-credential"),
            recorded_fields.get("x-amz-credential")
        );
        assert_eq!(mine.get("x-amz-date"), recorded_fields.get("x-amz-date"));
        assert_eq!(
            out.get("url").and_then(Value::as_str),
            parsed
                .get("upload_data")
                .and_then(|u| u.get("url"))
                .and_then(Value::as_str)
        );
        assert_eq!(
            keys_in_order(mine),
            keys_in_order(recorded_fields),
            "field order matches botocore"
        );
    }

    /// The recorded `current_instance` text (F18-10 confirm) round-trips
    /// through `cpython_dumps` byte-identically.
    #[test]
    fn replay_current_instance_dump() {
        let units = f18_10_units();
        let unit = units.get("attachment_confirm").expect("unit");
        let text = unit
            .get("messages")
            .and_then(Value::as_array)
            .and_then(|m| {
                m.iter().find(|m| {
                    m.get("task")
                        .and_then(Value::as_str)
                        .is_some_and(|t| t.ends_with("issue_activity"))
                })
            })
            .and_then(|m| m.get("kwargs"))
            .and_then(|k| k.get("current_instance"))
            .and_then(Value::as_str)
            .expect("current_instance");
        assert!(text.starts_with("{\"id\": \""), "{text}");
        let parsed: Value = serde_json::from_str(text).expect("instance parses");
        assert_eq!(cpython_dumps(&parsed), text);
    }

    #[test]
    fn replay_confirm_delete_redirect_statuses() {
        assert_eq!(
            call("attachment_confirm")
                .get("status")
                .and_then(Value::as_u64),
            Some(204)
        );
        assert_eq!(
            call("attachment_confirm")
                .get("body")
                .and_then(Value::as_str),
            Some("")
        );
        assert_eq!(
            call("attachment_delete")
                .get("status")
                .and_then(Value::as_u64),
            Some(204)
        );
        assert_eq!(
            call("attachment_get_redirect")
                .get("status")
                .and_then(Value::as_u64),
            Some(302)
        );
        assert_eq!(
            call("attachment_get_redirect")
                .get("body")
                .and_then(|b| b.get("<non-json>"))
                .and_then(Value::as_str),
            Some("")
        );
    }

    /// The four deprecated twins pin byte-identity with the new routes;
    /// they share these handlers, so registration is the whole port.
    #[test]
    fn replay_deprecated_twins_same_body() {
        let fixture: Value = serde_json::from_str(F18_11).expect("fixture parses");
        let twins = fixture
            .get("deprecated_twins")
            .and_then(Value::as_object)
            .expect("twins");
        for name in [
            "activity_list",
            "activity_detail",
            "attachment_list",
            "attachment_detail",
        ] {
            assert_eq!(
                twins.get(name).and_then(|t| t.get("same_body")),
                Some(&Value::Bool(true)),
                "{name}"
            );
        }
    }

    // --- Denials: exact statuses + bodies ---

    fn denial_response(denial: Denial) -> (StatusCode, String) {
        denial.status_and_body()
    }

    #[test]
    fn denial_statuses_and_bodies() {
        assert_eq!(
            denial_response(Denial::Unauthorized),
            (
                StatusCode::UNAUTHORIZED,
                r#"{"detail":"Authentication credentials were not provided."}"#.to_owned()
            )
        );
        assert_eq!(
            denial_response(Denial::InvalidToken),
            (
                StatusCode::FORBIDDEN,
                r#"{"detail":"Given API token is not valid"}"#.to_owned()
            )
        );
        assert_eq!(
            denial_response(Denial::Forbidden),
            (
                StatusCode::FORBIDDEN,
                r#"{"detail":"You do not have permission to perform this action."}"#.to_owned()
            )
        );
        assert_eq!(
            denial_response(Denial::ForbiddenUpload),
            (
                StatusCode::FORBIDDEN,
                r#"{"error":"You are not allowed to upload this attachment"}"#.to_owned()
            )
        );
        assert_eq!(
            denial_response(Denial::ForbiddenDownload),
            (
                StatusCode::FORBIDDEN,
                r#"{"error":"You are not allowed to download this attachment"}"#.to_owned()
            )
        );
        assert_eq!(
            denial_response(Denial::ForbiddenDelete),
            (
                StatusCode::FORBIDDEN,
                r#"{"error":"You are not allowed to delete this attachment"}"#.to_owned()
            )
        );
        assert_eq!(
            denial_response(Denial::ProjectNotFound),
            (
                StatusCode::NOT_FOUND,
                r#"{"detail":"Project not found"}"#.to_owned()
            )
        );
        assert_eq!(
            denial_response(Denial::BadDetail("Invalid per_page parameter.".to_owned())),
            (
                StatusCode::BAD_REQUEST,
                r#"{"detail":"Invalid per_page parameter."}"#.to_owned()
            )
        );
        assert_eq!(
            denial_response(Denial::BadError(
                "The required key does not exist.".to_owned()
            )),
            (
                StatusCode::BAD_REQUEST,
                r#"{"error":"The required key does not exist."}"#.to_owned()
            )
        );
        assert_eq!(
            denial_response(Denial::UnsupportedMediaType(
                "Unsupported media type \"text/plain\" in request.".to_owned()
            )),
            (
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                r#"{"detail":"Unsupported media type \"text/plain\" in request."}"#.to_owned()
            )
        );
        assert_eq!(
            denial_response(Denial::NotFound(RESOURCE_NOT_FOUND_BODY.to_owned())),
            (StatusCode::NOT_FOUND, RESOURCE_NOT_FOUND_BODY.to_owned())
        );
        assert_eq!(
            denial_response(Denial::NotFound(
                queries_sub::ACTIVITY_NOT_FOUND_BODY.to_owned()
            )),
            (
                StatusCode::NOT_FOUND,
                r#"{"message":"Activity not found.","code":"NOT_FOUND"}"#.to_owned()
            )
        );
        assert_eq!(
            denial_response(Denial::ServerError),
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                SERVER_ERROR_BODY.to_owned()
            )
        );
    }

    #[test]
    fn activate_timezone_zone_arms() {
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
        // `None` is unreachable (the column is `NOT NULL`) and would be
        // Python's `TypeError` 500, not a UTC default; invalid keys
        // (`_validate_tzfile_path`) are the `ValueError` 500 while
        // well-formed-but-missing keys are the 400 (PIDASHCONV-789#6,
        // live-probed).
        assert!(matches!(activate_timezone(None), Err(Denial::ServerError)));
        for bad in [
            "../x", "..", ".", "/abs", "a/b/../c", "a/./b", "a//b", "a/b/", "a\x00b",
        ] {
            assert!(
                matches!(activate_timezone(Some(bad)), Err(Denial::ServerError)),
                "zone {bad:?}"
            );
        }
        for missing in ["No/Such", "posix/Europe/London", "a..b"] {
            assert!(
                matches!(activate_timezone(Some(missing)), Err(Denial::BadError(_))),
                "zone {missing:?}"
            );
        }
        assert_eq!(activate_timezone(Some("UTC")).expect("utc"), chrono_tz::UTC);
    }

    #[test]
    fn download_filename_arms() {
        use serde_json::json;
        // Missing/null mints a fresh hex per call (`None`).
        assert_eq!(
            download_filename(&json!({"type": "application/pdf"})).expect("missing"),
            None
        );
        assert_eq!(
            download_filename(&json!({"name": null})).expect("null"),
            None
        );
        // Strings pass through ("" renders bare — the kernel maps it).
        assert_eq!(
            download_filename(&json!({"name": "f.pdf"})).expect("string"),
            Some("f.pdf".to_owned())
        );
        assert_eq!(
            download_filename(&json!({"name": ""})).expect("empty"),
            Some(String::new())
        );
        // Truthy non-strings hit `quote()`'s `TypeError` 500; falsy
        // non-strings take the bare-disposition arm.
        assert!(matches!(
            download_filename(&json!({"name": 5})),
            Err(Denial::ServerError)
        ));
        assert!(matches!(
            download_filename(&json!({"name": true})),
            Err(Denial::ServerError)
        ));
        assert_eq!(
            download_filename(&json!({"name": 0})).expect("zero"),
            Some(String::new())
        );
        assert_eq!(
            download_filename(&json!({"name": false})).expect("false"),
            Some(String::new())
        );
        // A non-object `attributes` has no `.get` (`AttributeError` 500).
        assert!(matches!(
            download_filename(&json!(["name"])),
            Err(Denial::ServerError)
        ));
        assert!(matches!(
            download_filename(&json!("name")),
            Err(Denial::ServerError)
        ));
    }

    #[test]
    fn inline_error_bodies() {
        assert_eq!(
            INVALID_REQUEST_BODY,
            r#"{"error":"Invalid request.","status":false}"#
        );
        assert_eq!(
            INVALID_FILE_TYPE_BODY,
            r#"{"error":"Invalid file type.","status":false}"#
        );
        assert_eq!(
            ASSET_NOT_UPLOADED_BODY,
            r#"{"error":"The asset is not uploaded.","status":false}"#
        );
    }

    #[test]
    fn external_dup_body_key_order() {
        let mut map = Map::with_capacity(2);
        map.insert(
            "error".to_owned(),
            Value::String(EXTERNAL_DUP_MESSAGE.to_owned()),
        );
        map.insert("id".to_owned(), Value::String("some-id".to_owned()));
        assert_eq!(
            serde_json::to_string(&Value::Object(map)).expect("json"),
            r#"{"error":"Issue with the same external id and external source already exists","id":"some-id"}"#
        );
    }

    // --- order_by ---

    #[test]
    fn activity_order_resolution() {
        fn resolved(raw: Option<&str>) -> ActivityOrder {
            resolve_activity_order(raw).expect("order")
        }
        fn plain(order: &str) -> ActivityOrder {
            ActivityOrder {
                joins: String::new(),
                order: order.to_owned(),
            }
        }
        assert_eq!(resolved(None), plain(r#"a."created_at" ASC"#));
        assert_eq!(resolved(Some("created_at")), plain(r#"a."created_at" ASC"#));
        assert_eq!(
            resolved(Some("-updated_at")),
            plain(r#"a."updated_at" DESC"#)
        );
        // Bare FK field names follow the relation into the target's
        // Meta.ordering (PIDASHCONV-759); a leading `-` flips every
        // term. Bare attnames stay on the local FK column.
        assert_eq!(
            resolved(Some("actor")),
            ActivityOrder {
                joins: "\n           LEFT JOIN \"users\" o1 ON o1.\"id\" = a.\"actor_id\""
                    .to_owned(),
                order: r#"o1."created_at" DESC"#.to_owned(),
            }
        );
        assert_eq!(
            resolved(Some("-actor")),
            ActivityOrder {
                joins: "\n           LEFT JOIN \"users\" o1 ON o1.\"id\" = a.\"actor_id\""
                    .to_owned(),
                order: r#"o1."created_at" ASC"#.to_owned(),
            }
        );
        assert_eq!(
            resolved(Some("issue")),
            ActivityOrder {
                joins: "\n           LEFT JOIN \"issues\" o1 ON o1.\"id\" = a.\"issue_id\""
                    .to_owned(),
                order: r#"o1."created_at" DESC"#.to_owned(),
            }
        );
        assert_eq!(
            resolved(Some("issue_comment")),
            ActivityOrder {
                joins: "\n           LEFT JOIN \"issue_comments\" o1 ON o1.\"id\" = a.\"issue_comment_id\""
                    .to_owned(),
                order: r#"o1."created_at" DESC"#.to_owned(),
            }
        );
        assert_eq!(
            resolved(Some("-created_by")),
            ActivityOrder {
                joins: "\n           LEFT JOIN \"users\" o1 ON o1.\"id\" = a.\"created_by_id\""
                    .to_owned(),
                order: r#"o1."created_at" ASC"#.to_owned(),
            }
        );
        assert_eq!(
            resolved(Some("updated_by")),
            ActivityOrder {
                joins: "\n           LEFT JOIN \"users\" o1 ON o1.\"id\" = a.\"updated_by_id\""
                    .to_owned(),
                order: r#"o1."created_at" DESC"#.to_owned(),
            }
        );
        // Bare `project`/`workspace` reuse the base query's `p`/`w`
        // joins with no extra JOIN.
        assert_eq!(resolved(Some("project")), plain(r#"p."created_at" DESC"#));
        assert_eq!(resolved(Some("-project")), plain(r#"p."created_at" ASC"#));
        assert_eq!(resolved(Some("workspace")), plain(r#"w."created_at" DESC"#));
        assert_eq!(resolved(Some("actor_id")), plain(r#"a."actor_id" ASC"#));
        assert_eq!(resolved(Some("-actor_id")), plain(r#"a."actor_id" DESC"#));
        assert_eq!(resolved(Some("project_id")), plain(r#"a."project_id" ASC"#));
        assert_eq!(resolved(Some("pk")), plain(r#"a."id" ASC"#));
        assert_eq!(resolved(Some("?")), plain("RANDOM()"));
        // Forward spans (PIDASHCONV-748): first-hop project/workspace
        // reuse the base query's `p`/`w` joins with no extra JOIN.
        assert_eq!(resolved(Some("project__name")), plain(r#"p."name" ASC"#));
        assert_eq!(resolved(Some("-project__name")), plain(r#"p."name" DESC"#));
        assert_eq!(resolved(Some("project_id__name")), plain(r#"p."name" ASC"#));
        assert_eq!(resolved(Some("workspace__slug")), plain(r#"w."slug" ASC"#));
        assert_eq!(
            resolved(Some("project__workspace_id")),
            plain(r#"p."workspace_id" ASC"#)
        );
        // Terminal pk collapses to the parent's FK column (no new join).
        assert_eq!(resolved(Some("actor__id")), plain(r#"a."actor_id" ASC"#));
        assert_eq!(
            resolved(Some("project__pk")),
            plain(r#"a."project_id" ASC"#)
        );
        assert_eq!(
            resolved(Some("project__workspace__id")),
            plain(r#"p."workspace_id" ASC"#)
        );
        // Other hops add row-preserving LEFT JOINs (`o1`, `o2`, …).
        assert_eq!(
            resolved(Some("actor__email")),
            ActivityOrder {
                joins: "\n           LEFT JOIN \"users\" o1 ON o1.\"id\" = a.\"actor_id\""
                    .to_owned(),
                order: r#"o1."email" ASC"#.to_owned(),
            }
        );
        assert_eq!(
            resolved(Some("-issue__created_at")),
            ActivityOrder {
                joins: "\n           LEFT JOIN \"issues\" o1 ON o1.\"id\" = a.\"issue_id\""
                    .to_owned(),
                order: r#"o1."created_at" DESC"#.to_owned(),
            }
        );
        assert_eq!(
            resolved(Some("project__workspace__name")),
            ActivityOrder {
                joins: "\n           LEFT JOIN \"workspaces\" o1 ON o1.\"id\" = p.\"workspace_id\""
                    .to_owned(),
                order: r#"o1."name" ASC"#.to_owned(),
            }
        );
        assert_eq!(
            resolved(Some("issue__project__name")),
            ActivityOrder {
                joins: [
                    "\n           LEFT JOIN \"issues\" o1 ON o1.\"id\" = a.\"issue_id\"",
                    "\n           LEFT JOIN \"projects\" o2 ON o2.\"id\" = o1.\"project_id\"",
                ]
                .concat(),
                order: r#"o2."name" ASC"#.to_owned(),
            }
        );
        // Terminal FK names order by the target's Meta.ordering (a
        // leading `-` flips every term); targets without ordering
        // collapse to the parent's FK column with no target join.
        assert_eq!(
            resolved(Some("issue__state")),
            ActivityOrder {
                joins: [
                    "\n           LEFT JOIN \"issues\" o1 ON o1.\"id\" = a.\"issue_id\"",
                    "\n           LEFT JOIN \"states\" o2 ON o2.\"id\" = o1.\"state_id\"",
                ]
                .concat(),
                order: r#"o2."sequence" ASC"#.to_owned(),
            }
        );
        assert_eq!(
            resolved(Some("-project__created_by")),
            ActivityOrder {
                joins: "\n           LEFT JOIN \"users\" o1 ON o1.\"id\" = p.\"created_by_id\""
                    .to_owned(),
                order: r#"o1."created_at" ASC"#.to_owned(),
            }
        );
        assert_eq!(
            resolved(Some("issue__type")),
            ActivityOrder {
                joins: "\n           LEFT JOIN \"issues\" o1 ON o1.\"id\" = a.\"issue_id\""
                    .to_owned(),
                order: r#"o1."type_id" ASC"#.to_owned(),
            }
        );
        assert_eq!(
            resolved(Some("issue__assigned_pod")),
            ActivityOrder {
                joins: [
                    "\n           LEFT JOIN \"issues\" o1 ON o1.\"id\" = a.\"issue_id\"",
                    "\n           LEFT JOIN \"pod\" o2 ON o2.\"id\" = o1.\"assigned_pod_id\"",
                ]
                .concat(),
                order: r#"o2."is_default" DESC, o2."created_at" ASC"#.to_owned(),
            }
        );
        for bad in [
            "",
            "nope",
            "-?",
            "+id",
            "?,created_at",
            // Unknown relation / terminal / empty segments: FieldError.
            "project__bogus",
            "bogus__name",
            "project__",
            "__name",
            "name__",
            "project___name",
            "project__name__",
            // Hop through a concrete column, not a relation.
            "verb__x",
            "id__name",
            // Residual edges (Django 200s; documented in the module docs).
            "created_at__date",
            "issue__assignees__email",
            "project__project_issueactivity__verb",
            "actor__avatar_asset__draft_issue__name",
        ] {
            assert!(
                resolve_activity_order(Some(bad)).is_err(),
                "{bad} must 500 like FieldError"
            );
        }
    }

    // --- query params ---

    #[test]
    fn query_last_and_fields_param() {
        let mut query = QueryMap::new();
        query.insert("a".to_owned(), OneOrMany::One("1".to_owned()));
        query.insert(
            "b".to_owned(),
            OneOrMany::Many(vec!["x".to_owned(), "y".to_owned()]),
        );
        query.insert("empty".to_owned(), OneOrMany::One(String::new()));
        assert_eq!(query_last(&query, "a").as_deref(), Some("1"));
        assert_eq!(query_last(&query, "b").as_deref(), Some("y"));
        assert_eq!(query_last(&query, "missing"), None);
        let mut fields = QueryMap::new();
        fields.insert("fields".to_owned(), OneOrMany::One("id,,verb".to_owned()));
        assert_eq!(
            fields_param(&fields, "fields"),
            Some(vec!["id".to_owned(), "verb".to_owned()])
        );
        assert_eq!(fields_param(&fields, "expand"), None);
        let mut blank = QueryMap::new();
        blank.insert("fields".to_owned(), OneOrMany::One(String::new()));
        assert_eq!(fields_param(&blank, "fields"), None);
    }

    // --- POST body ---

    fn json_headers() -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            "application/json".parse().expect("header"),
        );
        headers
    }

    fn headers_len(content_type: &str, len: usize) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(header::CONTENT_TYPE, content_type.parse().expect("header"));
        headers.insert(
            header::CONTENT_LENGTH,
            len.to_string().parse().expect("header"),
        );
        headers
    }

    #[test]
    fn post_data_json_forms_and_errors() {
        // Empty body is `{}` whatever the content type (missing or zero
        // Content-Length alike).
        let data = parse_post_data(&json_headers(), b"").expect("empty");
        assert!(data.map.is_empty());
        let data = parse_post_data(&headers_len("application/json", 0), b"").expect("empty");
        assert!(data.map.is_empty());
        // Valid JSON.
        let raw = br#"{"name":"a","size":3}"#;
        let data = parse_post_data(&headers_len("application/json", raw.len()), raw).expect("json");
        assert_eq!(data.map.get("size"), Some(&Value::Number(3.into())));
        // Malformed JSON is the CPython-text 400.
        let raw = b"{oops";
        let err = parse_post_data(&headers_len("application/json", raw.len()), raw)
            .expect_err("malformed");
        match err {
            Denial::BadDetail(detail) => {
                assert!(detail.starts_with("JSON parse error - "), "{detail}")
            }
            other => panic!("{other:?}"),
        }
        // Non-object JSON 500s (`.get` on a list).
        let raw = b"[1]";
        assert!(matches!(
            parse_post_data(&headers_len("application/json", raw.len()), raw),
            Err(Denial::ServerError)
        ));
        // Unsupported media type 415s.
        let raw = b"{}";
        assert!(matches!(
            parse_post_data(&headers_len("text/plain", raw.len()), raw),
            Err(Denial::UnsupportedMediaType(_))
        ));
        // Strict constants reject.
        let raw = b"{\"size\": NaN}";
        let err =
            parse_post_data(&headers_len("application/json", raw.len()), raw).expect_err("nan");
        match err {
            Denial::BadDetail(detail) => assert!(detail.contains("not JSON compliant"), "{detail}"),
            other => panic!("{other:?}"),
        }
        // Urlencoded forms parse to text.
        let raw = b"name=a&size=3";
        let data = parse_post_data(
            &headers_len("application/x-www-form-urlencoded", raw.len()),
            raw,
        )
        .expect("form");
        assert_eq!(data.map.get("size"), Some(&Value::String("3".to_owned())));
    }

    // --- text helpers ---

    #[test]
    fn truthiness_matches_python() {
        assert!(!is_truthy(&Value::Null));
        assert!(!is_truthy(&Value::Bool(false)));
        assert!(!is_truthy(&serde_json::json!(0)));
        assert!(!is_truthy(&serde_json::json!(0.0)));
        assert!(!is_truthy(&serde_json::json!("")));
        assert!(!is_truthy(&serde_json::json!([])));
        assert!(!is_truthy(&serde_json::json!({})));
        assert!(is_truthy(&Value::Bool(true)));
        assert!(is_truthy(&serde_json::json!(1)));
        assert!(is_truthy(&serde_json::json!(-0.5)));
        assert!(is_truthy(&serde_json::json!("0")));
        assert!(is_truthy(&serde_json::json!([false])));
    }

    #[test]
    fn overflow_numbers_match_python_truthiness_and_dumps() {
        // PIDASHCONV-782: overflow literals saturate to ±inf (truthy);
        // huge ints are truthy iff nonzero. This crate builds with
        // arbitrary_precision, so literals parse exactly as the request
        // path publishes them.
        let num =
            |literal: &str| -> Value { serde_json::from_str(literal).expect("number literal") };
        let huge_pos = format!("1{}", "0".repeat(400));
        let huge_neg = format!("-{huge_pos}");
        for literal in ["-1e999", "1e999", &huge_pos, &huge_neg] {
            assert!(is_truthy(&num(literal)), "{literal}");
        }
        assert!(!is_truthy(&num("1e-999")));
        // `json.dumps` renders ±inf `Infinity`, which jsonb rejects —
        // at any depth; huge ints dump fine.
        let mut attributes = Map::new();
        attributes.insert("name".to_owned(), num("-1e999"));
        attributes.insert("type".to_owned(), serde_json::json!("application/pdf"));
        attributes.insert("size".to_owned(), serde_json::json!(100));
        assert!(attributes_dump_fails(&attributes));
        let nested: Value = serde_json::from_str(r#"{"n": [1e999]}"#).expect("nested parses");
        let mut deep = Map::new();
        deep.insert("name".to_owned(), nested);
        assert!(attributes_dump_fails(&deep));
        let mut huge = Map::new();
        huge.insert("name".to_owned(), num(&huge_neg));
        huge.insert("size".to_owned(), num(&huge_pos));
        assert!(!attributes_dump_fails(&huge));
        let mut plain = Map::new();
        plain.insert("name".to_owned(), serde_json::json!("f.pdf"));
        plain.insert("size".to_owned(), serde_json::json!(100));
        assert!(!attributes_dump_fails(&plain));
    }

    #[test]
    fn cpython_dumps_separators_and_ascii() {
        let value = serde_json::json!({"b": [1, true, null], "a": "é☃"});
        assert_eq!(
            cpython_dumps(&value),
            r#"{"b": [1, true, null], "a": "é☃"}"#
                .replace("é", "\\u00e9")
                .replace("☃", "\\u2603")
        );
        // Astral planes become surrogate pairs; C0 controls short-escape.
        assert_eq!(
            cpython_dumps(&serde_json::json!("𝄞\n")),
            r#""\ud834\udd1e\n""#
        );
        assert_eq!(cpython_dumps(&serde_json::json!("\u{0}")), r#""\u0000""#);
    }

    #[test]
    fn py_str_spellings() {
        assert_eq!(py_str(&Value::Null), "None");
        assert_eq!(py_str(&Value::Bool(true)), "True");
        assert_eq!(py_str(&Value::Bool(false)), "False");
        assert_eq!(py_str(&serde_json::json!(200)), "200");
        assert_eq!(py_str(&serde_json::json!(200.5)), "200.5");
        assert_eq!(py_str(&serde_json::json!("f.pdf")), "f.pdf");
        assert_eq!(py_str(&serde_json::json!([1, "a"])), "[1, 'a']");
        assert_eq!(py_str(&serde_json::json!({"a": 1})), "{'a': 1}");
        assert_eq!(py_repr(&serde_json::json!("it's")), "\"it's\"");
    }

    #[test]
    fn policy_numbers_and_size_floats() {
        assert_eq!(policy_number(&serde_json::json!(200)), "200");
        assert_eq!(policy_number(&serde_json::json!(true)), "true");
        assert_eq!(size_limit_f64(&serde_json::json!(200)).expect("int"), 200.0);
        assert_eq!(size_limit_f64(&Value::Bool(true)).expect("bool"), 1.0);
        assert_eq!(size_limit_f64(&Value::Bool(false)).expect("bool"), 0.0);
        // Past f64 range: Python's OverflowError 500.
        let huge: Value = serde_json::from_str("1e1000").expect("huge parses");
        assert!(size_limit_f64(&huge).is_err());
    }

    #[test]
    fn external_text_charfield_prep() {
        assert_eq!(external_text(None, None), None);
        assert_eq!(external_text(Some(&Value::Null), None), None);
        assert_eq!(
            external_text(Some(&serde_json::json!("x")), None),
            Some("x".to_owned())
        );
        assert_eq!(
            external_text(Some(&serde_json::json!(0)), None),
            Some("0".to_owned())
        );
        assert_eq!(
            external_text(Some(&serde_json::json!(true)), None),
            Some("True".to_owned())
        );
        assert_eq!(
            external_text(None, Some(&"f.bin".to_owned())),
            Some("f.bin".to_owned())
        );
    }

    #[test]
    fn app_origin_prefers_app_base() {
        let mut urls = pidash_db::config::UrlSettings {
            admin_base_url: None,
            admin_base_path: String::new(),
            space_base_url: None,
            space_base_path: String::new(),
            app_base_url: Some("https://app.example".to_owned()),
            app_base_path: String::new(),
            live_base_url: None,
            live_base_path: String::new(),
            web_url: Some("https://web.example".to_owned()),
        };
        assert_eq!(app_origin(&urls).expect("origin"), "https://app.example");
        urls.app_base_url = None;
        assert_eq!(app_origin(&urls).expect("origin"), "https://web.example");
        urls.web_url = Some(String::new());
        assert!(app_origin(&urls).is_err());
    }

    // --- non-finite floats ---

    #[test]
    fn nonfinite_literals_finalize() {
        let nan = format!("{NONFINITE_SENTINEL}NaN");
        let body = serde_json::to_string(&serde_json::json!({"epoch": nan})).expect("json");
        assert_eq!(finalize_nonfinite(body), r#"{"epoch":NaN}"#);
        let inf = format!("{NONFINITE_SENTINEL}Infinity");
        let body = serde_json::to_string(&serde_json::json!({"size": inf})).expect("json");
        assert_eq!(finalize_nonfinite(body), r#"{"size":Infinity}"#);
        let ninf = format!("{NONFINITE_SENTINEL}-Infinity");
        let body = serde_json::to_string(&serde_json::json!({"size": ninf})).expect("json");
        assert_eq!(finalize_nonfinite(body), r#"{"size":-Infinity}"#);
        assert_eq!(finalize_nonfinite(r#"{"a":1}"#.to_owned()), r#"{"a":1}"#);
    }

    #[test]
    fn render_nonfinite_floats_strict() {
        // Responses 500 on non-finite floats (DRF strict renderer).
        let record = call("activity_detail");
        let body = record.get("body").expect("body");
        let mut decoded = decoded_activity(body);
        decoded.epoch = Some(f64::NAN);
        assert!(render_activity_value(&decoded, None, &[], &[]).is_err());
        let list = call("attachment_list");
        let item = list
            .get("body")
            .and_then(Value::as_array)
            .and_then(|b| b.first())
            .expect("item");
        let mut decoded = decoded_attachment(item);
        decoded.size = f64::INFINITY;
        assert!(render_attachment_value(&decoded).is_err());
        // Dumps spell the `json.dumps` literal (`allow_nan`).
        let dumped = render_attachment_for_dump(&decoded).expect("dump render");
        let text = finalize_nonfinite(cpython_dumps(&dumped));
        assert_eq!(text.matches("\"size\": Infinity").count(), 1);
        decoded.size = f64::NAN;
        let dumped = render_attachment_for_dump(&decoded).expect("dump render");
        let text = finalize_nonfinite(cpython_dumps(&dumped));
        assert_eq!(text.matches("\"size\": NaN").count(), 1);
    }

    // --- SigV4 ---

    fn test_storage() -> pidash_db::config::StorageSettings {
        pidash_db::config::StorageSettings {
            use_minio: false,
            minio_endpoint_ssl: false,
            access_key_id: "AKID".to_owned(),
            secret_access_key: "SECRET".to_owned(),
            bucket_name: "bucket".to_owned(),
            region: "us-east-1".to_owned(),
            endpoint_url: None,
            signed_url_expiration_secs: 3600,
        }
    }

    #[test]
    fn uri_encoding_and_hash_helpers() {
        assert_eq!(uri_encode("aB0-_.~"), "aB0-_.~");
        assert_eq!(uri_encode("a b/c+d"), "a%20b%2Fc%2Bd");
        assert_eq!(uri_encode("é"), "%C3%A9");
        assert_eq!(uri_encode_path("a b/c"), "a%20b/c");
        assert_eq!(hex(b"\x00\xff"), "00ff");
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        // RFC 4231-style HMAC-SHA256 check via a known vector.
        assert_eq!(
            hex(&hmac_sha256(
                b"key",
                b"The quick brown fox jumps over the lazy dog"
            )),
            "f7bc83f430538424b13298e6aa6fb143ef4d59a14946175997479dbc2d1a3cd8"
        );
        assert_eq!(
            credential_scope("20260101", "us-east-1"),
            "20260101/us-east-1/s3/aws4_request"
        );
    }

    #[test]
    fn endpoint_parts_modes() {
        fn resolved(
            storage: &pidash_db::config::StorageSettings,
            scheme: &str,
            host: Option<&str>,
        ) -> ResolvedEndpoint {
            endpoint_parts(storage, scheme, host).expect("resolves")
        }
        fn endpoint(
            url_base: &str,
            signed_host: &str,
            base_path: &str,
            path_style: bool,
        ) -> ResolvedEndpoint {
            ResolvedEndpoint {
                url_base: url_base.to_owned(),
                signed_host: signed_host.to_owned(),
                base_path: base_path.to_owned(),
                path_style,
            }
        }
        let storage = test_storage();
        // us-east-1 serves from the global endpoint (botocore table).
        assert_eq!(
            resolved(&storage, "http", Some("h.example")),
            endpoint(
                "https://bucket.s3.amazonaws.com",
                "bucket.s3.amazonaws.com",
                "",
                false
            )
        );
        // Every caller here presigns, so `aws` regions resolve the global
        // endpoint (`use_global_endpoint`) while other partitions sign
        // regional (PIDASHCONV-789#8, live-probed).
        let mut regional = storage.clone();
        regional.region = "ap-south-1".to_owned();
        assert_eq!(
            resolved(&regional, "http", Some("h.example")),
            endpoint(
                "https://bucket.s3.amazonaws.com",
                "bucket.s3.amazonaws.com",
                "",
                false
            )
        );
        regional.region = "cn-north-1".to_owned();
        assert_eq!(
            resolved(&regional, "http", Some("h.example")),
            endpoint(
                "https://bucket.s3.cn-north-1.amazonaws.com.cn",
                "bucket.s3.cn-north-1.amazonaws.com.cn",
                "",
                false
            )
        );
        // Buckets that are not valid host labels sign path-style
        // (PIDASHCONV-789#7, live-probed).
        let mut dots = storage.clone();
        dots.bucket_name = "with.dots".to_owned();
        assert_eq!(
            resolved(&dots, "http", Some("h.example")),
            endpoint("https://s3.amazonaws.com", "s3.amazonaws.com", "", true)
        );
        // Buckets outside the param-validation shape fail
        // (`ParamValidationError` 500).
        for bad in ["", "with space", "foo/bar", "arn:aws:s3:::x"] {
            let mut invalid = storage.clone();
            invalid.bucket_name = bad.to_owned();
            assert_eq!(
                endpoint_parts(&invalid, "http", Some("h.example")),
                None,
                "bucket {bad:?}"
            );
        }
        let mut minio = storage.clone();
        minio.use_minio = true;
        assert_eq!(
            resolved(&minio, "https", Some("minio:9000")),
            endpoint("https://minio:9000", "minio:9000", "", true)
        );
        assert_eq!(endpoint_parts(&minio, "https", None), None);
        // `MINIO_ENDPOINT_SSL=1` signs https in MinIO mode (`storage.py:46-51`).
        let mut ssl = minio.clone();
        ssl.minio_endpoint_ssl = true;
        assert_eq!(
            resolved(&ssl, "http", Some("h:9")),
            endpoint("https://h:9", "h:9", "", true)
        );
        // The signed host drops default ports and lowercases while the URL
        // keeps the authority verbatim (PIDASHCONV-789#9, live-probed).
        assert_eq!(
            resolved(&ssl, "https", Some("H:443")),
            endpoint("https://H:443", "h", "", true)
        );
        let mut custom = storage.clone();
        custom.endpoint_url = Some("https://s3.custom:443/prefix/".to_owned());
        assert_eq!(
            resolved(&custom, "http", Some("h.example")),
            endpoint("https://s3.custom:443", "s3.custom", "/prefix", true)
        );
        // Invalid endpoint URLs fail (`ValueError`/`EndpointResolutionError`).
        for bad in [
            "custom:9000",
            "http://",
            "ftp://custom:9000",
            "http://custom:9000?q=1",
            "http://custom:abc",
            "http://[a]b@host/",
        ] {
            let mut invalid = storage.clone();
            invalid.endpoint_url = Some(bad.to_owned());
            assert_eq!(
                endpoint_parts(&invalid, "http", Some("h.example")),
                None,
                "endpoint {bad:?}"
            );
        }
        // Empty region with a derived endpoint fails (`ValueError`); empty
        // region against a custom endpoint still signs.
        let mut no_region = storage.clone();
        no_region.region = String::new();
        assert_eq!(endpoint_parts(&no_region, "http", None), None);
        no_region.endpoint_url = Some("http://127.0.0.1:9000".to_owned());
        assert_eq!(
            resolved(&no_region, "http", None),
            endpoint("http://127.0.0.1:9000", "127.0.0.1:9000", "", true)
        );
        // Garbage regions fail everywhere (`InvalidRegionError`).
        let mut garbage = storage.clone();
        garbage.region = "!!".to_owned();
        assert_eq!(endpoint_parts(&garbage, "http", Some("h.example")), None);
        let mut empty = storage.clone();
        empty.secret_access_key = String::new();
        assert!(!storage_can_sign(&empty));
        empty.secret_access_key = "x".to_owned();
        empty.access_key_id = String::new();
        assert!(!storage_can_sign(&empty));
        assert!(storage_can_sign(&storage));
    }

    #[test]
    fn presigned_get_url_shape_and_determinism() {
        let storage = test_storage();
        let now = chrono::Utc
            .with_ymd_and_hms(2026, 1, 2, 3, 4, 5)
            .single()
            .expect("instant");
        let first = presigned_get_url(
            &storage,
            "http",
            Some("h.example"),
            "w/k-f.pdf",
            Some("f.pdf"),
            &now,
        )
        .expect("signs");
        let second = presigned_get_url(
            &storage,
            "http",
            Some("h.example"),
            "w/k-f.pdf",
            Some("f.pdf"),
            &now,
        )
        .expect("signs");
        assert_eq!(first, second, "fixed inputs sign deterministically");
        assert!(
            first.starts_with(
                "https://bucket.s3.amazonaws.com/w/k-f.pdf?response-content-disposition="
            ),
            "{first}"
        );
        assert!(first.contains("X-Amz-Signature="), "{first}");
        assert!(first.contains("X-Amz-Expires=3600"), "{first}");
        assert!(
            first.contains(
                "response-content-disposition=attachment%3B%20filename%2A%3DUTF-8%27%27f.pdf"
            ),
            "{first}"
        );
        // Missing names mint a fresh hex per call.
        let third = presigned_get_url(&storage, "http", Some("h.example"), "w/k", None, &now)
            .expect("signs");
        let fourth = presigned_get_url(&storage, "http", Some("h.example"), "w/k", None, &now)
            .expect("signs");
        assert_ne!(third, fourth);
        for url in [&third, &fourth] {
            let disposition = url
                .split("response-content-disposition=")
                .nth(1)
                .and_then(|s| s.split('&').next())
                .expect("disposition");
            assert!(
                disposition.starts_with("attachment%3B%20filename%2A%3DUTF-8%27%27"),
                "{url}"
            );
            assert_eq!(
                disposition.len(),
                "attachment%3B%20filename%2A%3DUTF-8%27%27".len() + 32
            );
        }
        // Empty names render bare.
        let bare = presigned_get_url(&storage, "http", Some("h.example"), "w/k", Some(""), &now)
            .expect("signs");
        assert!(
            bare.contains("response-content-disposition=attachment&"),
            "{bare}"
        );
    }

    #[test]
    fn presigned_post_minio_and_policy_layout() {
        let mut storage = test_storage();
        storage.use_minio = true;
        let now = chrono::Utc
            .with_ymd_and_hms(2026, 1, 2, 3, 4, 5)
            .single()
            .expect("instant");
        let out = presigned_post(
            &storage,
            "https",
            Some("minio:9000"),
            "w/k",
            "text/plain",
            "200",
            &now,
        )
        .expect("signs");
        assert_eq!(
            out.get("url").and_then(Value::as_str),
            Some("https://minio:9000/bucket")
        );
        let fields = out
            .get("fields")
            .and_then(Value::as_object)
            .expect("fields");
        assert_eq!(
            keys_in_order(fields),
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
        use base64::Engine;
        let policy_b64 = fields
            .get("policy")
            .and_then(Value::as_str)
            .expect("policy");
        let policy = base64::engine::general_purpose::STANDARD
            .decode(policy_b64)
            .expect("policy decodes");
        let policy_text = String::from_utf8(policy).expect("policy utf8");
        assert!(
            policy_text.starts_with("{\"expiration\": \"2026-01-02T04:04:05Z\", \"conditions\": ["),
            "{policy_text}"
        );
        assert!(
            policy_text.contains("[\"content-length-range\", 1, 200]"),
            "{policy_text}"
        );
        assert_eq!(policy_text.matches("{\"bucket\": \"bucket\"}").count(), 2);
    }

    /// Full SigV4 differential against live botocore captures (boto3
    /// 1.34, `s3v4`, no endpoint): same credentials, instant, key and
    /// size — the policy, the POST signature and the complete GET URL
    /// (param order included) match byte for byte.
    #[test]
    fn sigv4_matches_botocore_capture() {
        use chrono::TimeZone as _;
        let storage = pidash_db::config::StorageSettings {
            use_minio: false,
            minio_endpoint_ssl: false,
            access_key_id: "AKID675TEST".to_owned(),
            secret_access_key: "SECRET675TESTSECRET675TESTSECRET12".to_owned(),
            bucket_name: "bucket675".to_owned(),
            region: "us-east-1".to_owned(),
            endpoint_url: None,
            signed_url_expiration_secs: 3600,
        };
        let key = "ws-id/abcdef0123456789abcdef0123456789-f.pdf";
        let now = chrono::Utc
            .with_ymd_and_hms(2026, 10, 5, 2, 56, 16)
            .single()
            .expect("capture instant");
        // No `Host` header: AWS-mode signing never needs the request host.
        let out = presigned_post(&storage, "http", None, key, "application/pdf", "200", &now)
            .expect("signs");
        assert_eq!(
            out.get("url").and_then(Value::as_str),
            Some("https://bucket675.s3.amazonaws.com/")
        );
        let fields = out
            .get("fields")
            .and_then(Value::as_object)
            .expect("fields");
        assert_eq!(
            fields.get("policy").and_then(Value::as_str),
            Some("eyJleHBpcmF0aW9uIjogIjIwMjYtMTAtMDVUMDM6NTY6MTZaIiwgImNvbmRpdGlvbnMiOiBbeyJidWNrZXQiOiAiYnVja2V0Njc1In0sIFsiY29udGVudC1sZW5ndGgtcmFuZ2UiLCAxLCAyMDBdLCB7IkNvbnRlbnQtVHlwZSI6ICJhcHBsaWNhdGlvbi9wZGYifSwgeyJrZXkiOiAid3MtaWQvYWJjZGVmMDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODktZi5wZGYifSwgeyJidWNrZXQiOiAiYnVja2V0Njc1In0sIHsia2V5IjogIndzLWlkL2FiY2RlZjAxMjM0NTY3ODlhYmNkZWYwMTIzNDU2Nzg5LWYucGRmIn0sIHsieC1hbXotYWxnb3JpdGhtIjogIkFXUzQtSE1BQy1TSEEyNTYifSwgeyJ4LWFtei1jcmVkZW50aWFsIjogIkFLSUQ2NzVURVNULzIwMjYxMDA1L3VzLWVhc3QtMS9zMy9hd3M0X3JlcXVlc3QifSwgeyJ4LWFtei1kYXRlIjogIjIwMjYxMDA1VDAyNTYxNloifV19")
        );
        assert_eq!(
            fields.get("x-amz-signature").and_then(Value::as_str),
            Some("cee82e107e3b6db0ef80b5a048a04b1eabc96501375cdd24c0da9c6793fcd2b8")
        );
        let url =
            presigned_get_url(&storage, "http", None, key, Some("f.pdf"), &now).expect("signs");
        assert_eq!(
            url,
            "https://bucket675.s3.amazonaws.com/ws-id/abcdef0123456789abcdef0123456789-f.pdf?response-content-disposition=attachment%3B%20filename%2A%3DUTF-8%27%27f.pdf&X-Amz-Algorithm=AWS4-HMAC-SHA256&X-Amz-Credential=AKID675TEST%2F20261005%2Fus-east-1%2Fs3%2Faws4_request&X-Amz-Date=20261005T025616Z&X-Amz-Expires=3600&X-Amz-SignedHeaders=host&X-Amz-Signature=795c047ad3a950c8ef48ebdbdc70acef89d3b2574c26114e713bed3c3d45fe4a"
        );
    }

    #[test]
    fn py_json_string_escapes() {
        assert_eq!(py_json_string("a\"b\\c"), r#""a\"b\\c""#);
        assert_eq!(py_json_string("é"), r#""\u00e9""#);
        assert_eq!(py_json_string("𝄞"), r#""\ud834\udd1e""#);
        assert_eq!(py_json_string("a\nb"), r#""a\nb""#);
    }

    // --- asset urls ---

    #[test]
    fn issue_attachment_asset_url_shape() {
        let url = issue_attachment_asset_url(
            "acme",
            &Uuid::parse_str("11111111-1111-1111-1111-111111111111").expect("uuid"),
            &Uuid::parse_str("22222222-2222-2222-2222-222222222222").expect("uuid"),
            &Uuid::parse_str("33333333-3333-3333-3333-333333333333").expect("uuid"),
        );
        assert_eq!(
            url,
            "/api/assets/v2/workspaces/acme/projects/11111111-1111-1111-1111-111111111111/issues/22222222-2222-2222-2222-222222222222/attachments/33333333-3333-3333-3333-333333333333/"
        );
    }

    // --- response shell ---

    #[tokio::test]
    async fn json_response_escapes_u2028() {
        let response = json_response(StatusCode::OK, "\u{2028}\u{2029}".to_owned());
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        assert_eq!(body.as_ref(), b"\\u2028\\u2029");
    }

    // --- PIDASHCONV-789 edge vectors ---

    #[test]
    fn float_spelling_normalization_vectors() {
        fn normalized(literal: &str) -> String {
            let value: Value = serde_json::from_str(literal).expect("number");
            normalize_float_spelling(&value).to_string()
        }
        // Float spellings read as CPython floats (PIDASHCONV-789#2,
        // live-probed).
        assert_eq!(normalized("1e2"), "100.0");
        assert_eq!(normalized("1E2"), "100.0");
        assert_eq!(normalized("1.50"), "1.5");
        assert_eq!(normalized("0.5e1"), "5.0");
        assert_eq!(normalized("0.1"), "0.1");
        assert_eq!(normalized("1e16"), "1e+16");
        assert_eq!(normalized("1.5e-5"), "1.5e-05");
        assert_eq!(normalized("-0.0"), "-0.0");
        assert_eq!(normalized("0e0"), "0.0");
        // Ints (however huge), overflow spellings and non-numbers pass
        // through verbatim (`1e999` is PIDASHCONV-782's).
        assert_eq!(normalized("100"), "100");
        assert_eq!(normalized("-7"), "-7");
        let huge = "123456789012345678901234567890123456789012345678901234567890";
        assert_eq!(normalized(huge), huge);
        // (`serde_json` normalizes exponents at parse: `1e999` reads
        // `1e+999` — the verbatim passthrough preserves the PARSED form.)
        assert_eq!(normalized("1e999"), "1e+999");
        assert_eq!(normalized("-1e999"), "-1e+999");
        for literal in ["true", "false", "null", r#""1e2""#, "[1]", "{}"] {
            let value: Value = serde_json::from_str(literal).expect("value");
            assert_eq!(normalize_float_spelling(&value).to_string(), literal);
        }
    }

    #[test]
    fn posix_normpath_vectors() {
        // `posixpath.normpath` for relative keys (`CPython/Lib/posixpath.py`).
        for (raw, normal) in [
            ("", "."),
            (".", "."),
            ("..", ".."),
            ("a/b", "a/b"),
            ("a//b", "a/b"),
            ("a/./b", "a/b"),
            ("a/b/", "a/b"),
            ("a/../b", "b"),
            ("../x", "../x"),
            ("../..", "../.."),
            ("a/..", "."),
            ("a/b/..", "a"),
            ("a/../../x", "../x"),
            ("a..b", "a..b"),
        ] {
            assert_eq!(posix_normpath(raw), normal, "normpath {raw:?}");
        }
    }

    #[test]
    fn partition_resolution_vectors() {
        // (region, dns suffix, is_aws) — `partitions.json` order, regex
        // pass only (every explicit region matches its own regex).
        for (region, suffix, is_aws) in [
            ("us-east-1", "amazonaws.com", true),
            ("eu-west-1", "amazonaws.com", true),
            ("ap-south-1", "amazonaws.com", true),
            ("eusc-de-east-1", "amazonaws.com", true),
            ("cn-north-1", "amazonaws.com.cn", false),
            ("cn-northwest-1", "amazonaws.com.cn", false),
            ("us-gov-west-1", "amazonaws.com", false),
            ("us-gov-east-1", "amazonaws.com", false),
            ("us-iso-east-1", "c2s.ic.gov", false),
            ("us-isob-east-1", "sc2s.sgov.gov", false),
            ("eu-isoe-west-1", "cloud.adc-e.uk", false),
            ("us-isof-south-1", "csp.hci.ic.gov", false),
            // Unknown/odd regions take the default partition.
            ("xx-foo-1", "amazonaws.com", true),
            ("US-EAST-1", "amazonaws.com", true),
            ("us-global", "amazonaws.com", true),
            ("fips-us-east-1", "amazonaws.com", true),
            ("us-east-1-fips", "amazonaws.com", true),
            ("s3-external-1", "amazonaws.com", true),
            ("cn-1", "amazonaws.com", true),
            ("", "amazonaws.com", true),
            // Order matters: `us-gov-1` matches `aws` first.
            ("us-gov-1", "amazonaws.com", true),
            ("us-gov-west-10", "amazonaws.com", false),
        ] {
            assert_eq!(
                partition_for_region(region),
                (suffix, is_aws),
                "region {region:?}"
            );
        }
    }

    #[test]
    fn virtual_hostable_bucket_vectors() {
        // `isVirtualHostableS3Bucket(bucket, False)` — live-probed.
        for bucket in [
            "bucket",
            "abc",
            "0abc",
            "123",
            "a-b",
            "a--b",
            "ab3",
            "9a9",
            "xn--foo",
            "dummy789bucket",
        ] {
            assert!(is_virtual_hostable_bucket(bucket), "virtual {bucket:?}");
        }
        assert!(is_virtual_hostable_bucket(&"a".repeat(63)));
        for bucket in [
            "a",
            "ab",
            "with.dots",
            "with_underscores",
            "UPPER",
            "MiXeD",
            "Abc",
            "192.168.1.1",
            "foo-",
            "-foo",
            "a0-",
            "_ab",
            "ab_",
            "",
        ] {
            assert!(!is_virtual_hostable_bucket(bucket), "path {bucket:?}");
        }
        assert!(!is_virtual_hostable_bucket(&"a".repeat(64)));
        assert!(!is_virtual_hostable_bucket(&"a".repeat(200)));
        // One trailing newline: the length reads raw (so `ab\n` counts
        // 3) while the label match enjoys `$` (so `ab\n` is virtual).
        assert!(is_virtual_hostable_bucket("ab\n"));
        assert!(is_virtual_hostable_bucket("abc\n"));
        assert!(!is_virtual_hostable_bucket("a\n"));
        assert!(!is_virtual_hostable_bucket("abc\n\n"));
        assert!(!is_virtual_hostable_bucket("\n\n\n"));
    }

    #[test]
    fn bucket_gate_vectors() {
        // `validate_bucket_name` verdicts, botocore `re` output baked
        // (`before-parameter-build.s3`).
        assert!(is_valid_bucket_name(&"a".repeat(255)));
        assert!(!is_valid_bucket_name(&"a".repeat(256)));
        for bucket in [
            "abc",
            "abc\n",
            "a_b.c-d",
            "a",
            "arn:aws:s3:us-east-1:123456789012:accesspoint/myap",
            "arn:aws:s3:us-east-1:123456789012:accesspoint/my-ap.1",
            "arn:aws:s3-outposts:us-east-1:123456789012:outpost/op-1/accesspoint/myap",
            "arn:aws:s3-outposts:us-east-1:123456789012:outpost:op-1:accesspoint:myap",
            "arn:aws:x:s3-object-lambda:r:123456789012:accesspoint/ap1",
            "arn:aws:s3:r:123456789012:accesspoint:ap1",
            "arn:aws:s3::123456789012:accesspoint:ap1",
            "arn:aws2:s3:r:123456789012:accesspoint:ap1",
            "arn:aws:s3:r:123456789012:accesspoint:ap1\n",
        ] {
            assert!(is_valid_bucket_name(bucket), "valid {bucket:?}");
        }
        for bucket in [
            "",
            "with space",
            "foo/bar",
            "arn:aws:s3:::x",
            "ARN:aws:s3:::x",
            "arn:aws:s3:us-east-1:12345678901:accesspoint/myap",
            "arn:aws:s3:us-east-1:123456789012:accesspoint/",
            "arn:aws:s3:us-east-1:123456789012:accesspoint:a/b",
            "arn:aws:s3-outposts:us-east-1:123456789012:outpost/op-1/accesspoint/my.ap",
            "arn:aws:s3:r:123456789012:accesspoint",
            "ARN:AWS:S3:R:123456789012:ACCESSPOINT:AP1",
            "arn:aws:s3:r:123456789012:accesspoint:ap1:extra",
            "abc\n\n",
        ] {
            assert!(!is_valid_bucket_name(bucket), "invalid {bucket:?}");
        }
        assert!(!is_valid_bucket_name(&format!(
            "arn:aws:s3:us-east-1:123456789012:accesspoint:{}",
            "a".repeat(64)
        )));
    }

    #[test]
    fn urlsplit_vectors() {
        fn split(url: &str) -> Option<(String, String, String, String)> {
            urlsplit_parts(url)
        }
        // (input, (scheme, netloc, path, query)) — `urllib.parse.urlsplit`.
        for (url, expected) in [
            ("http://h/x?y#z", ("http", "h", "/x", "y")),
            ("HTTP://h", ("http", "h", "", "")),
            (" http://h", ("http", "h", "", "")),
            ("custom:9000", ("custom", "", "9000", "")),
            ("a/b:c", ("", "", "a/b:c", "")),
            ("http://[::1]:9000/x", ("http", "[::1]:9000", "/x", "")),
            ("http://a]b@[::1]:9000", ("http", "a]b@[::1]:9000", "", "")),
            ("http://user@", ("http", "user@", "", "")),
            ("http://custom./", ("http", "custom.", "/", "")),
            ("http://h/a#b?c", ("http", "h", "/a", "")),
            ("http://h/a?b#c", ("http", "h", "/a", "b")),
            ("http://h\t", ("http", "h", "", "")),
            ("http://[v1.fe]/", ("http", "[v1.fe]", "/", "")),
            (
                "http://[::ffff:1.2.3.4]/",
                ("http", "[::ffff:1.2.3.4]", "/", ""),
            ),
            (
                "http://[fe80::1%25eth0]/",
                ("http", "[fe80::1%25eth0]", "/", ""),
            ),
            ("ftp://h", ("ftp", "h", "", "")),
        ] {
            assert_eq!(
                split(url),
                Some((
                    expected.0.to_owned(),
                    expected.1.to_owned(),
                    expected.2.to_owned(),
                    expected.3.to_owned()
                )),
                "split {url:?}"
            );
        }
        // Bracket/encoding failures (`ValueError` in urlsplit).
        for bad in [
            "http://[a]b@host/",
            "http://[::1",
            "http://a]b/",
            "http://a[b/",
            "http://[V1.fe]/",
            "http://[1.2.3.4]/",
            "http://[v1]/",
            "http://[::ffff:01.2.3.4]/",
            "http://münchen/",
        ] {
            assert_eq!(split(bad), None, "split {bad:?}");
        }
        // Both Rust's IPv6 parser and `ipaddress` reject the
        // leading-zero tail — `check_bracketed_host` agrees.
        assert!("::ffff:01.2.3.4".parse::<std::net::Ipv6Addr>().is_err());
        assert!(!check_bracketed_host("::ffff:01.2.3.4"));
        assert!(check_bracketed_host("::ffff:1.2.3.4"));
        assert!(check_bracketed_host("v1.fe"));
        assert!(check_bracketed_host("fe80::1%eth0"));
    }

    #[test]
    fn endpoint_validation_vectors() {
        // `is_valid_endpoint_url or is_valid_ipv6_endpoint_url` —
        // live-probed (client-creation gate).
        for url in [
            "http://custom:9000",
            "http://custom:9000/base/path",
            "http://CUSTOM:9000",
            "http://custom:80",
            "https://custom:443",
            "HTTP://custom:9000",
            "http://custom:/",
            "http://custom:00080",
            "http://custom:0",
            "http://[::1]:9000",
            "http://[::1]:9000/b",
            "http://user@[::1]:9000",
            "http://a]b@[::1]:9000",
            "http://custom./",
            "http://custom:9000?q=1",
            "http://custom:9000#frag",
            "http://custom:9000/x#frag?noq",
            "ftp://custom:9000",
            " http://h",
            "http://h:abc",
            "http://h:65536",
            "http://h/b ase",
            "http://h/%41",
            "http://h/Ä",
            "http://h/a/../b",
        ] {
            assert!(is_valid_endpoint_str(url), "valid {url:?}");
        }
        for url in [
            "custom:9000",
            "http://",
            "http://user@",
            "http://?/x",
            "",
            "http://[a]b@host/",
            "http://h\t",
            "http://h ",
            "http://hü/",
        ] {
            assert!(!is_valid_endpoint_str(url), "invalid {url:?}");
        }
        // Known residual (see the PR): botocore SIGNS non-ASCII userinfo
        // with an ASCII host (NFKC check), this port fails closed.
        assert!(!is_valid_endpoint_str("http://münchen@h/"));
        assert!(!is_valid_endpoint_str(&format!(
            "http://{}/",
            "a".repeat(256)
        )));
        assert!(is_valid_endpoint_str(&format!(
            "http://{}/",
            "a".repeat(63)
        )));
        assert!(!is_valid_endpoint_str(&format!(
            "http://{}.{}",
            "a".repeat(64),
            "b"
        )));
    }

    #[test]
    fn signed_host_vectors() {
        // `_host_from_url` — live-probed.
        for (scheme, authority, signed) in [
            ("http", "CUSTOM:9000", "custom:9000"),
            ("http", "h:80", "h"),
            ("https", "h:443", "h"),
            ("https", "h:80", "h:80"),
            ("http", "h", "h"),
            ("http", "h:", "h"),
            ("http", "h:0", "h:0"),
            ("http", "h:00080", "h"),
            ("http", "user@[::1]:9000", "[::1]:9000"),
            ("http", "[::1]:9000", "[::1]:9000"),
            ("http", "[::1]:80", "[::1]"),
            ("http", "a]b@[::1]:9000", "[::1]:9000"),
            ("http", "custom.", "custom."),
            ("http", "[fe80::1%25eth0]", "[fe80::1%25eth0]"),
            ("http", "[fe80::1%tESt]", "[fe80::1%tESt]"),
            ("http", "h:9000", "h:9000"),
        ] {
            assert_eq!(
                signed_host_for(scheme, authority).as_deref(),
                Some(signed),
                "host {scheme}://{authority}"
            );
        }
    }

    #[test]
    fn base_path_vectors() {
        // `parseURL` base paths — live-probed.
        for (url, scheme, authority, base) in [
            ("http://h", "http", "h", ""),
            ("http://h/", "http", "h", ""),
            ("http://h/a", "http", "h", "/a"),
            (
                "http://custom:9000/base/path",
                "http",
                "custom:9000",
                "/base/path",
            ),
            (
                "http://custom:9000/base/path/",
                "http",
                "custom:9000",
                "/base/path",
            ),
            ("http://h/a///", "http", "h", "/a"),
            ("http://h//", "http", "h", ""),
            ("http://h/a//b", "http", "h", "/a/b"),
            ("http://h/b ase", "http", "h", "/b%20ase"),
            ("http://h/a/../b", "http", "h", "/b"),
            ("http://h/./x", "http", "h", "/x"),
            ("http://h/%41", "http", "h", "/%2541"),
            ("http://h/Ä", "http", "h", "/%C3%84"),
            ("http://h/a/./../b/", "http", "h", "/b"),
            ("http://h/../x", "http", "h", "/x"),
            ("http://h/..", "http", "h", ""),
            ("HTTP://h/BASE/", "http", "h", "/BASE"),
            ("http://CUSTOM:9000", "http", "CUSTOM:9000", ""),
            ("http://user@[::1]:9000", "http", "user@[::1]:9000", ""),
            ("http://h/x#frag", "http", "h", "/x"),
            ("http://h/x#frag?noq", "http", "h", "/x"),
            (" http://h/a", "http", "h", "/a"),
        ] {
            assert_eq!(
                parse_custom_endpoint(url),
                Some((scheme.to_owned(), authority.to_owned(), base.to_owned())),
                "endpoint {url:?}"
            );
        }
        // Queries, non-http(s) schemes, bad ports and invalid hosts fail.
        for bad in [
            "http://h?q=1",
            "http://h/a?b#c",
            "ftp://h",
            "FTP://h",
            "custom:9000",
            "http://",
            "http://h:abc",
            "http://h:65536",
            "http://h:-1",
            "http://h:80x",
            "http://h:9000  ",
            "http://[a]b@host/",
            "http://user@",
            "http://h\t",
            "http://h ",
        ] {
            assert_eq!(parse_custom_endpoint(bad), None, "endpoint {bad:?}");
        }
    }

    #[test]
    fn ipv6_loose_vectors() {
        // The urllib3 alternatives: full form is exactly 8 groups;
        // `::` compresses at least one; the embedded-IPv4 tail is
        // range-loose.
        for head in [
            "::1",
            "::",
            "1:2:3:4:5:6:7:8",
            "::ffff:1.2.3.4",
            "1::2",
            "1:2:3:4:5:6:7::",
            "::1.2.3.4",
            "1::1.2.3.4",
            "::ffff:999.1.1.1",
            "FE80::1",
            "1:2:3:4:5::8",
        ] {
            assert!(is_ipv6_loose(head), "loose {head:?}");
        }
        for head in [
            "",
            ":::",
            "1::2::3",
            "1:2:3:4:5:6:7:8:9",
            "1:2:3:4:5:6:7",
            "1:2:3:4:5:6:7::8",
            "12345::",
            "gggg::1",
            "1.2.3.4",
            "::ffff:1.2.3.9999",
            "ab",
        ] {
            assert!(!is_ipv6_loose(head), "strict {head:?}");
        }
        // `%zone` tails (`(?:%25|%)(?:[unreserved]|%HH)+`).
        assert!(is_valid_ipv6_bracketed("::1"));
        assert!(is_valid_ipv6_bracketed("fe80::1%eth0"));
        assert!(is_valid_ipv6_bracketed("fe80::1%25eth0"));
        assert!(is_valid_ipv6_bracketed("fe80::1%tESt"));
        assert!(!is_valid_ipv6_bracketed("::1%"));
        assert!(!is_valid_ipv6_bracketed("::1%25"));
        assert!(!is_valid_ipv6_bracketed("::1%a%b"));
        assert!(!is_valid_ipv6_bracketed("ab"));
    }

    #[test]
    fn presigned_vectors_match_botocore() {
        let goldens: Value = serde_json::from_str(include_str!(
            "../../../../fixtures/v1_work_items/handlers/activity_signing.golden.json"
        ))
        .expect("goldens parse");
        let now = chrono::Utc
            .with_ymd_and_hms(2026, 10, 5, 2, 56, 16)
            .single()
            .expect("instant");
        for case in goldens["get"].as_array().expect("get cases") {
            let mut storage = test_storage();
            storage.access_key_id = "dummy789key".to_owned();
            storage.secret_access_key = "dummy789secret".to_owned();
            storage.bucket_name = case["bucket"].as_str().expect("bucket").to_owned();
            storage.region = case["region"].as_str().expect("region").to_owned();
            storage.endpoint_url = case["endpoint"].as_str().map(str::to_owned);
            let url = presigned_get_url(
                &storage,
                "http",
                Some("h.example"),
                "ws-id/hexhex-f.pdf",
                Some("f.pdf"),
                &now,
            )
            .expect("signs");
            assert_eq!(url, case["url"].as_str().expect("url"), "GET {case:?}");
        }
        for case in goldens["post"].as_array().expect("post cases") {
            let mut storage = test_storage();
            storage.access_key_id = "dummy789key".to_owned();
            storage.secret_access_key = "dummy789secret".to_owned();
            storage.bucket_name = case["bucket"].as_str().expect("bucket").to_owned();
            storage.region = case["region"].as_str().expect("region").to_owned();
            storage.endpoint_url = case["endpoint"].as_str().map(str::to_owned);
            let response = presigned_post(
                &storage,
                "http",
                Some("h.example"),
                case["key"].as_str().expect("key"),
                "application/pdf",
                "10",
                &now,
            )
            .expect("signs");
            assert_eq!(response["url"], case["url"], "POST url {case:?}");
            // String compare: field ORDER is part of the pin.
            assert_eq!(
                serde_json::to_string(&response["fields"]).expect("fields"),
                serde_json::to_string(&case["fields"]).expect("golden fields"),
                "POST fields {case:?}"
            );
        }
    }

    #[test]
    fn presigned_post_filename_template() {
        use base64::Engine;
        fn conditions(object_name: &str) -> Value {
            let storage = test_storage();
            let now = chrono::Utc
                .with_ymd_and_hms(2026, 10, 5, 2, 56, 16)
                .single()
                .expect("instant");
            let response = presigned_post(
                &storage,
                "http",
                Some("h.example"),
                object_name,
                "application/pdf",
                "10",
                &now,
            )
            .expect("signs");
            let policy = response["fields"]["policy"].as_str().expect("policy");
            let decoded = base64::engine::general_purpose::STANDARD
                .decode(policy)
                .expect("b64");
            let policy: Value = serde_json::from_slice(&decoded).expect("json");
            assert_eq!(
                response["fields"]["key"].as_str(),
                Some(object_name),
                "fields.key keeps the full key"
            );
            policy["conditions"].clone()
        }
        // A key ending in the token appends `starts-with` instead of the
        // client's `{"key"}` (PIDASHCONV-789#1, live-probed).
        let ends = conditions("x${filename}");
        assert_eq!(ends[3], serde_json::json!({"key": "x${filename}"}));
        assert_eq!(ends[5], serde_json::json!(["starts-with", "$key", "x"]));
        // The `storage.py` leading-token branch conditions on the same
        // trailing-11 slice (port the quirk).
        let starts = conditions("${filename}x");
        assert_eq!(starts[3], serde_json::json!(["starts-with", "$key", "$"]));
        assert_eq!(starts[5], serde_json::json!({"key": "${filename}x"}));
        let exact = conditions("${filename}");
        assert_eq!(exact[3], serde_json::json!(["starts-with", "$key", ""]));
        assert_eq!(exact[5], serde_json::json!(["starts-with", "$key", ""]));
        // Ordinary keys keep both `{"key"}` conditions.
        let plain = conditions("f.pdf");
        assert_eq!(plain[3], serde_json::json!({"key": "f.pdf"}));
        assert_eq!(plain[5], serde_json::json!({"key": "f.pdf"}));
    }
}
