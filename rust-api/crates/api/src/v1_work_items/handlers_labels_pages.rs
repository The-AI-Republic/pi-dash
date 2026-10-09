//! Label + page handlers (D-18 handlers G, PIDASHCONV-679).
//!
//! Ports the five `apps/api/pi_dash/api/views/issue.py` label units and the
//! `apps/api/pi_dash/api/views/page.py` endpoint units plus the page module
//! helpers as their closure:
//!
//! * `LabelListCreateAPIEndpoint.post` (`issue.py:1355-1405`) — create +
//!   external-dup 409 + `IntegrityError`→name-409 arm.
//! * `LabelListCreateAPIEndpoint.get` (`issue.py:1428-1437`) — paginated
//!   label list, `fields=`/`expand=` from the query string.
//! * `LabelDetailAPIEndpoint.get` (`issue.py:1463-1470`) — one label.
//! * `LabelDetailAPIEndpoint.patch` (`issue.py:1493-1525`) — partial update
//!   + external-dup 409 (self excluded).
//! * `LabelDetailAPIEndpoint.delete` (`issue.py:1538-1546`) — soft delete +
//!   related-objects sweep enqueue.
//! * `PageListAPIEndpoint.get` (`page.py:261-274`) — paginated page list
//!   (`include_archived` flag, metadata only).
//! * `PageListAPIEndpoint.post` (`page.py:298-341`) — create through the
//!   live document server (503 when unavailable).
//! * `PageDetailAPIEndpoint.get` (`page.py:365-376`) — one page with the
//!   derived `description_markdown`.
//! * `PageDetailAPIEndpoint.patch` (`page.py:404-477`) — partial update
//!   with lock/archive guards, live conversion, and a `SELECT FOR UPDATE`
//!   re-check.
//! * `PageArchiveAPIEndpoint.post` (`page.py:518-538`) — archive a page and
//!   its descendants (favourites dropped).
//! * `PageArchiveAPIEndpoint.delete` (`page.py:554-573`) — unarchive
//!   (detach-if-parent-archived).
//! * Page module helpers (`page.py:104-172`): `_error`, `_conversion_failed`,
//!   `render_body_html`, `_document_fields`, `_record_body_write`.
//!
//! Registered by [`super::routes`] at the `api/urls/label.py` (2) and
//! `api/urls/page.py` (3) paths.
//!
//! Layering (all foundation use is read-only): row shapes in
//! `pidash_services::v1_work_items::shape_labels` (S2, PIDASHCONV-661) and
//! `shape_pages` (S7, PIDASHCONV-666), queryset semantics in `queries_sub`
//! (Q2, PIDASHCONV-669) and `queries_search` (Q3, PIDASHCONV-670) — the
//! representative SQL carries `:named` placeholders, so this module binds
//! the executable `$n` form — gate decisions in [`super::perms`] (P1,
//! PIDASHCONV-671), task kwargs in `tasks` (T1, PIDASHCONV-672), the archive
//! CTE + binary validator in the merged D-30 `app_pages` ports, and the
//! `validate_html_content` / MLStripper ports in `space::sanitize` /
//! `db::app_pages::strip`. This module owns the HTTP shell (API-key auth,
//! the slug→UUID rewrite, permission wiring, body parsing, the paginated
//! envelope) plus the label INSERT/UPDATE/DELETE SQL (no layer ports it),
//! the page INSERT/UPDATE SQL, the live-document `convert_document` caller,
//! and the two body converters (`markdown_converter.py`).
//!
//! Request order per inner: preamble (401 anonymous before any DB) →
//! project-id rewrite (404 identifier miss) → gate (403) → timezone
//! activation (400 unknown zone) → body parse (writes only) →
//! shape validation (400) → handler SQL → fan-out → response.
//!
//! Ported bugs (all verified against live Django):
//!
//! * BUG-1 (`views/issue.py:1335`): `.order_by()` reads `self.kwargs`
//!   (URL kwargs — never `order_by`), so `?order_by=` is ignored and the
//!   label list always orders `-created_at` (same ported bug as the
//!   reviewed siblings).
//! * BUG-2 (`db/models/label.py:46-54`): `Label.save` overwrites an
//!   explicitly passed `sort_order` with `MAX+10000` whenever the project
//!   already has labels; the explicit value survives only on the first
//!   label of a project.
//! * BUG-3 (`views/issue.py:1393-1405`): the POST `IntegrityError` arm
//!   dereferences `label.id` without a miss check — a constraint failure
//!   with no same-name row (e.g. a parent-FK violation) 500s on
//!   `None.id` instead of 409ing.
//! * BUG-4 (`serializers` via `db/models/label.py:29-33`): the name
//!   `UniqueValidator` only covers the project-NULL constraint, so a
//!   within-project rename sails through validation and 400s on the bare
//!   `IntegrityError` (`handle_exception`: "The payload is not valid").
//! * BUG-5 (`app/permissions/project.py:62-65`): label GET gates on a
//!   workspace-scoped membership (no `project_id`), so a member of any
//!   project in the workspace passes the gate and then reads whatever the
//!   project-scoped queryset yields.
//!
//! Deliberate edges (all unpinned — no fixture or contract case sends them):
//!
//! * `expand=` on rows whose workspace/project/user row is hard-missing
//!   renders null; Python follows the FK descriptor and may 404 instead.
//!   Same edge as the reviewed siblings.
//! * Multipart/file inputs are ignored (no file field exists here); Python
//!   would 400 them as non-strings.
//! * Extreme-magnitude floats render with serde's exponent spelling where
//!   CPython spells `1e+16` — same accepted edge as every merged shape
//!   module.
//! * The markdown converters parse with html5ever (scraper) / pulldown
//!   instead of html.parser / markdown-it-py; byte identity holds over the
//!   Tiptap corpus (pinned by vectors generated from the live Python).
//!
//! Fixture: `F18-11` (`rust-api/fixtures/v1_work_items/handlers/` —
//! `label_create`, `label_create_dup`, `label_list`, `label_detail`,
//! `label_patch`, `label_delete`, `page_list`, `page_detail`,
//! `page_detail_404`, `page_create`, `page_create_invalid`, `page_patch`,
//! `archive_forbidden`, `archive_post`, `archive_post_noop`,
//! `archive_delete`).
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

use std::collections::HashMap;

use axum::extract::{OriginalUri, Path, Query, Request, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use serde_json::{Map, Value};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use pidash_auth::permissions::project;
use pidash_auth::scope::TenantScope;
use pidash_services::v1_work_items::shape_issue::BASE_EXPANSION_NAMES;
use pidash_services::v1_work_items::shape_labels::{
    name_lookup_key, parent_lookup_key, render_issue_lite, render_label, validate_label_write,
    IssueLiteRow, LabelRepresentationInput, LabelRow, LabelWriteInput,
};
use pidash_services::v1_work_items::shape_pages::{
    render_page_detail, render_page_lite, validate_page_write, PageReadInput, PageRow, WriteMode,
    PAGE_DETAIL_FIELDS, PAGE_LITE_FIELDS,
};
use pidash_services::v1_work_items::tasks as work_tasks;
use pidash_services::v1_work_items::{filter_fields, FieldSpec};

use super::perms::{check_can_archive, decide, gate_for, ArchiveDecision, V1WorkItemsRoute};
use crate::state::AppState;
use ego_tree::NodeRef;
use pulldown_cmark::{
    CodeBlockKind, CowStr, Event, HeadingLevel, LinkType, Options, Parser, Tag, TagEnd,
};
use scraper::{Html, Node};
use std::borrow::Cow;

// ---------------------------------------------------------------------------
// Responses
// ---------------------------------------------------------------------------

/// `handle_exception`'s `ObjectDoesNotExist` branch
/// (`api/views/base.py:154-158`): every `.get()` miss on these endpoints.
pub const RESOURCE_NOT_FOUND_BODY: &str = r#"{"error":"The requested resource does not exist."}"#;
/// `handle_exception`'s generic branch (`api/views/base.py:166-170`).
pub const SERVER_ERROR_BODY: &str = r#"{"error":"Something went wrong please try again later"}"#;
/// `handle_exception`'s `IntegrityError` branch
/// (`api/views/base.py:142-147`): label PATCH saves and the page-create
/// transaction.
pub const PAYLOAD_NOT_VALID_BODY: &str = r#"{"error":"The payload is not valid"}"#;
/// Label create/update external-duplicate 409 message
/// (`views/issue.py:1382,1516`).
pub const LABEL_DUP_MESSAGE: &str =
    "Label with the same external id and external source already exists";
/// Label create name-duplicate 409 message (`views/issue.py:1401`).
pub const LABEL_NAME_DUP_MESSAGE: &str = "Label with the same name already exists in the project";
/// Archived-page 409 (`views/page.py:420-421,471-472`; key order `error`,
/// `error_code`, `error_message` as built by `_error`).
pub const PAGE_ARCHIVED_BODY: &str =
    r#"{"error":"Page is archived","error_code":4702,"error_message":"PAGE_ARCHIVED"}"#;
/// Page PATCH access-change 403 (`views/page.py:422-423`).
pub const PAGE_ACCESS_DENIAL_BODY: &str =
    r#"{"error":"Only the page owner can change its access"}"#;
/// Page create on an archived project 409 (`views/page.py:308-309`).
pub const PROJECT_ARCHIVED_BODY: &str = r#"{"error":"The project is archived"}"#;
/// `LIVE_URL` unset (`utils/live_document.py:83-85`): the only conversion
/// failure reachable without a live server.
pub const LIVE_URL_MISSING_MESSAGE: &str =
    "LIVE_URL is not configured, so the page document cannot be regenerated";
/// The related-objects sweep enqueued by `SoftDeleteModel.delete()`
/// (`db/mixins.py:72-78` over `bgtasks/deletion_task.py:18`).
pub const SOFT_DELETE_TASK: &str = "pi_dash.bgtasks.deletion_task.soft_delete_related_objects";

/// Handler failure with its exact status + body.
#[derive(Debug, PartialEq, Eq)]
pub enum Denial {
    /// 401, DRF `NotAuthenticated` (no `X-Api-Key` header).
    Unauthorized,
    /// 403, invalid/expired/inactive API or machine token.
    InvalidToken,
    /// 403, the DRF-default `PermissionDenied` body (no D-18 guard class
    /// sets `message`).
    Forbidden,
    /// 404, `{"detail":"Project not found"}` (identifier rewrite miss —
    /// `Project.resolve` raises `Http404`, `db/models/project.py:213-217`).
    ProjectNotFound,
    /// 400, `{"detail": ...}` (DRF `ParseError`: malformed JSON, bad
    /// `per_page`/`cursor`).
    BadDetail(String),
    /// 400, `{"error": ...}` (view-inline: unknown timezones, parent and
    /// body-render failures).
    BadError(String),
    /// 400, serializer `errors` dict (pre-rendered bytes, field order).
    FieldErrors(String),
    /// 415, `{"detail": ...}` (DRF `UnsupportedMediaType`).
    UnsupportedMediaType(String),
    /// 403, view-inline `{"error": ...}` (page access/archive guards).
    ForbiddenError(String),
    /// 404, view-inline `{"error": ...}` with the full body.
    NotFound(String),
    /// 409, view-inline body (label duplicates, page lock/archive guards).
    Conflict(String),
    /// 503, `_conversion_failed` (the live document server is unavailable
    /// and nothing was written).
    ServiceUnavailable(String),
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
            Denial::FieldErrors(body) => (StatusCode::BAD_REQUEST, body.clone()),
            Denial::UnsupportedMediaType(message) => (
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                format!("{{\"detail\":{}}}", json_string(message)),
            ),
            Denial::ForbiddenError(body) => (StatusCode::FORBIDDEN, body.clone()),
            Denial::NotFound(body) => (StatusCode::NOT_FOUND, body.clone()),
            Denial::Conflict(body) => (StatusCode::CONFLICT, body.clone()),
            Denial::ServiceUnavailable(body) => (StatusCode::SERVICE_UNAVAILABLE, body.clone()),
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

/// Render a 201 JSON response with exact bytes.
fn json_created(body: String) -> Response {
    json_response(StatusCode::CREATED, body)
}

/// Map a database/driver failure to the generic 500 while logging the site
/// and error for operators (no secrets: messages never include tokens).
fn db_error<E: std::fmt::Display>(error: E, site: &str) -> Denial {
    tracing::warn!(%error, site, "v1_work_items labels_pages database failure");
    Denial::ServerError
}

/// `timezone.now()` truncated to microseconds: Python datetimes carry no
/// nanos, and `timestamptz` stores micros — an untruncated `Utc::now()`
/// would render nanos in the response while the DB row reads back micros.
fn now_utc() -> DateTime<Utc> {
    trunc_micros(Utc::now())
}

/// Truncate an instant to microsecond precision (see [`now_utc`]).
fn trunc_micros(dt: DateTime<Utc>) -> DateTime<Utc> {
    let nanos = dt.timestamp_subsec_nanos();
    dt - chrono::Duration::nanoseconds(i64::from(nanos % 1000))
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

/// A Postgres integrity failure (`IntegrityError` in Python): any `23xxx`
/// SQLSTATE (unique, FK, not-null, check).
fn is_integrity_error(error: &sqlx::Error) -> bool {
    match error {
        sqlx::Error::Database(db) => db
            .code()
            .as_deref()
            .is_some_and(|code| code.starts_with("23")),
        _ => false,
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
    // proxies them with the original request (the `app_issues` precedent).
    router.fallback(crate::edge::proxy)
}

/// The label list path owns GET + POST (`urls/label.py:11`,
/// `as_view(http_method_names=["get", "post"])`).
pub fn owned_label_list(
    router: axum::routing::MethodRouter<AppState>,
) -> axum::routing::MethodRouter<AppState> {
    owned(router, &["GET", "POST"])
}

/// The label detail path owns GET + PATCH + DELETE (`urls/label.py:16`,
/// `as_view(http_method_names=["get", "patch", "delete"])`).
pub fn owned_label_detail(
    router: axum::routing::MethodRouter<AppState>,
) -> axum::routing::MethodRouter<AppState> {
    owned(router, &["GET", "PATCH", "DELETE"])
}

/// The page list path owns GET + POST (`urls/page.py:14`,
/// `as_view(http_method_names=["get", "post"])`).
pub fn owned_page_list(
    router: axum::routing::MethodRouter<AppState>,
) -> axum::routing::MethodRouter<AppState> {
    owned(router, &["GET", "POST"])
}

/// The page detail path owns GET + PATCH (`urls/page.py:19`,
/// `as_view(http_method_names=["get", "patch"])`).
pub fn owned_page_detail(
    router: axum::routing::MethodRouter<AppState>,
) -> axum::routing::MethodRouter<AppState> {
    owned(router, &["GET", "PATCH"])
}

/// The page archive path owns POST + DELETE (`urls/page.py:24`,
/// `as_view(http_method_names=["post", "delete"])`).
pub fn owned_page_archive(
    router: axum::routing::MethodRouter<AppState>,
) -> axum::routing::MethodRouter<AppState> {
    owned(router, &["POST", "DELETE"])
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

/// Activate the actor's rendering timezone (`TimezoneMixin.initial` runs
/// after `super().initial()`). A missing zone defaults to UTC; an unknown
/// zone name 400s: `zoneinfo.ZoneInfo` raises `ZoneInfoNotFoundError`,
/// which subclasses `KeyError`, so `handle_exception` answers the
/// `KeyError` branch (`api/views/base.py:160-164`), never a 500.
fn activate_timezone(timezone: Option<&str>) -> Result<Tz, Denial> {
    timezone
        .unwrap_or("UTC")
        .parse()
        .map_err(|_| Denial::BadError("The required key does not exist.".to_owned()))
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

/// Fetch the project-permission facts for the Entity (page) and Member
/// (label) gates (`app/permissions/project.py:56-116`); [`decide`] applies
/// the per-route arm. The trailing bool is the workspace-scoped project
/// membership the Member-SAFE arm reads (BUG-5).
///
/// `role` is `smallint`: sqlx does not widen `INT2` into `i32` on decode,
/// so read `i16` and compare as integers. `deleted_at IS NULL` is the
/// `SoftDeletionManager` scope (`db/mixins.py:56-58`).
async fn gate_facts(
    pool: &PgPool,
    workspace_id: &uuid::Uuid,
    workspace_slug: &str,
    user_id: &uuid::Uuid,
    project_id: &uuid::Uuid,
) -> Result<(project::ProjectFacts, bool), Denial> {
    // Project-scoped roles (Entity SAFE + every unsafe arm).
    let roles: Vec<i16> = sqlx::query_scalar(
        r#"SELECT "role" FROM "project_members" WHERE "workspace_id" = $1 AND "member_id" = $2 AND "project_id" = $3 AND "is_active" AND "deleted_at" IS NULL"#,
    )
    .bind(workspace_id)
    .bind(user_id)
    .bind(project_id)
    .fetch_all(pool)
    .await
    .map_err(|error| db_error(error, "gate-roles"))?;
    // Workspace-scoped project membership (Member SAFE, BUG-5 —
    // `project.py:62-65` deliberately omits `project_id`).
    let workspace_scoped_member: bool = sqlx::query_scalar(
        r#"SELECT EXISTS(SELECT 1 FROM "project_members" WHERE "workspace_id" = $1 AND "member_id" = $2 AND "is_active" AND "deleted_at" IS NULL)"#,
    )
    .bind(workspace_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| db_error(error, "gate-ws-member"))?
    .unwrap_or(false);
    // Workspace admin-or-member (Member POST, `project.py:67-73`).
    let workspace_admin_or_member: bool = sqlx::query_scalar(
        r#"SELECT EXISTS(SELECT 1 FROM "workspace_members" WHERE "workspace_id" = $1 AND "member_id" = $2 AND "role" IN (20, 15) AND "is_active" AND "deleted_at" IS NULL)"#,
    )
    .bind(workspace_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| db_error(error, "gate-ws-roles"))?
    .unwrap_or(false);
    Ok((
        project::ProjectFacts {
            workspace: pidash_types::WorkspaceId::from(workspace_slug.to_owned()),
            project_id: pidash_types::ProjectId::from(project_id.to_string()),
            authenticated: true,
            is_workspace_member: false,
            has_workspace_admin_or_member: workspace_admin_or_member,
            is_workspace_admin: false,
            // The Member-SAFE arm reads the workspace-scoped fact while
            // the Entity-SAFE arm reads the project-scoped one; the two
            // are split in `require_gate`, which patches this field per
            // route.
            is_project_member: !roles.is_empty(),
            is_project_admin: roles.contains(&20),
            has_project_admin_or_member: roles.iter().any(|r| *r == 20 || *r == 15),
            has_identifier_membership: false,
            has_project_identifier: false,
        },
        workspace_scoped_member,
    ))
}

/// Run the route's gate; deny 403 on failure. The label routes carry
/// `ProjectMemberPermission`, the page routes `ProjectEntityPermission`
/// (see [`gate_for`]).
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
    let (mut facts, workspace_scoped_member) =
        gate_facts(pool, workspace_id, workspace_slug, user_id, project_id).await?;
    if matches!(
        route,
        V1WorkItemsRoute::LabelList | V1WorkItemsRoute::LabelDetail
    ) && pidash_auth::permissions::is_safe_method(method)
    {
        // BUG-5: Member SAFE reads the workspace-scoped membership.
        facts.is_project_member = workspace_scoped_member;
    }
    let scope = TenantScope::new(pidash_types::WorkspaceId::from(workspace_slug.to_owned()));
    if decide(gate, method, &scope, &facts) {
        Ok(())
    } else {
        Err(Denial::Forbidden)
    }
}

// ---------------------------------------------------------------------------
// Lookups
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

fn row_bool(row: &sqlx::postgres::PgRow, column: &str) -> Result<bool, Denial> {
    row.try_get(column).map_err(|_| Denial::ServerError)
}

fn row_f64(row: &sqlx::postgres::PgRow, column: &str) -> Result<f64, Denial> {
    row.try_get(column).map_err(|_| Denial::ServerError)
}

fn row_i16(row: &sqlx::postgres::PgRow, column: &str) -> Result<i16, Denial> {
    row.try_get(column).map_err(|_| Denial::ServerError)
}

fn row_date_opt(
    row: &sqlx::postgres::PgRow,
    column: &str,
) -> Result<Option<chrono::NaiveDate>, Denial> {
    row.try_get(column).map_err(|_| Denial::ServerError)
}

/// Label list/detail rows (`views/issue.py:1322-1336`): the recorded F18-07
/// predicates — live rows for this project/slug, an active membership of
/// the caller on the project (no `deleted_at` scope on the join span —
/// fixture-verbatim), a live project. Order is always `-created_at`:
/// `.order_by()` reads `self.kwargs`, which never carries `order_by`, so
/// `?order_by=` is ignored (BUG-1).
async fn fetch_label_rows(
    pool: &PgPool,
    slug: &str,
    project_id: &Uuid,
    user_id: &Uuid,
) -> Result<Vec<sqlx::postgres::PgRow>, Denial> {
    sqlx::query(
        r#"SELECT DISTINCT l.* FROM "labels" l
           WHERE l."deleted_at" IS NULL
             AND l."workspace_id" = (SELECT "id" FROM "workspaces" WHERE "slug" = $1)
             AND l."project_id" = $2
             AND EXISTS (SELECT 1 FROM "project_members" pm
                         WHERE pm."project_id" = l."project_id" AND pm."member_id" = $3 AND pm."is_active")
             AND EXISTS (SELECT 1 FROM "projects" p
                         WHERE p."id" = l."project_id" AND p."archived_at" IS NULL)
           ORDER BY l."created_at" DESC"#,
    )
    .bind(slug)
    .bind(project_id)
    .bind(user_id)
    .fetch_all(pool)
    .await
    .map_err(|error| db_error(error, "label-list"))
}

/// Label detail row (`:1468`, `:1499`, `:1544`): the list chain plus `id=pk`.
async fn fetch_label_detail(
    pool: &PgPool,
    slug: &str,
    project_id: &Uuid,
    user_id: &Uuid,
    pk: &Uuid,
) -> Result<Option<sqlx::postgres::PgRow>, Denial> {
    sqlx::query(
        r#"SELECT DISTINCT l.* FROM "labels" l
           WHERE l."deleted_at" IS NULL
             AND l."workspace_id" = (SELECT "id" FROM "workspaces" WHERE "slug" = $1)
             AND l."project_id" = $2
             AND EXISTS (SELECT 1 FROM "project_members" pm
                         WHERE pm."project_id" = l."project_id" AND pm."member_id" = $3 AND pm."is_active")
             AND EXISTS (SELECT 1 FROM "projects" p
                         WHERE p."id" = l."project_id" AND p."archived_at" IS NULL)
             AND l."id" = $4
           ORDER BY l."created_at" DESC"#,
    )
    .bind(slug)
    .bind(project_id)
    .bind(user_id)
    .bind(pk)
    .fetch_optional(pool)
    .await
    .map_err(|error| db_error(error, "label-detail"))
}

/// Page list rows (`views/page.py:180-197,267-273`): the F18-08 visibility
/// chain — live rows for this slug, the `Exists` over the through table
/// (member + live project, no row fan-out), public-or-owned — plus the
/// `include_archived` arm. Order is always `-created_at`.
async fn fetch_page_rows(
    pool: &PgPool,
    slug: &str,
    project_id: &Uuid,
    user_id: &Uuid,
    include_archived: bool,
) -> Result<Vec<sqlx::postgres::PgRow>, Denial> {
    let archived = if include_archived {
        ""
    } else {
        r#" AND p."archived_at" IS NULL"#
    };
    let sql = format!(
        r#"SELECT p.* FROM "pages" p
           WHERE p."deleted_at" IS NULL
             AND p."workspace_id" = (SELECT "id" FROM "workspaces" WHERE "slug" = $1)
             AND EXISTS (SELECT 1 FROM "project_pages" pp
                         INNER JOIN "projects" pr ON pp."project_id" = pr."id"
                         INNER JOIN "project_members" pm ON pr."id" = pm."project_id"
                         WHERE pp."deleted_at" IS NULL AND pp."page_id" = p."id"
                           AND pp."project_id" = $2 AND pr."archived_at" IS NULL
                           AND pm."is_active" AND pm."member_id" = $3)
             AND (p."access" = 0 OR p."owned_by_id" = $3){archived}
           ORDER BY p."created_at" DESC"#,
    );
    sqlx::query(&sql)
        .bind(slug)
        .bind(project_id)
        .bind(user_id)
        .fetch_all(pool)
        .await
        .map_err(|error| db_error(error, "page-list"))
}

/// One visible page (`get_page_or_error`, `detail_response`,
/// `validate_parent`): visibility plus `id`. `.get()` clears ordering;
/// `.first()` keeps `Meta` ordering — same row either way, so one fetch.
async fn fetch_visible_page(
    pool: &PgPool,
    slug: &str,
    project_id: &Uuid,
    user_id: &Uuid,
    page_id: &Uuid,
) -> Result<Option<sqlx::postgres::PgRow>, Denial> {
    sqlx::query(
        r#"SELECT p.* FROM "pages" p
           WHERE p."deleted_at" IS NULL
             AND p."workspace_id" = (SELECT "id" FROM "workspaces" WHERE "slug" = $1)
             AND EXISTS (SELECT 1 FROM "project_pages" pp
                         INNER JOIN "projects" pr ON pp."project_id" = pr."id"
                         INNER JOIN "project_members" pm ON pr."id" = pm."project_id"
                         WHERE pp."deleted_at" IS NULL AND pp."page_id" = p."id"
                           AND pp."project_id" = $2 AND pr."archived_at" IS NULL
                           AND pm."is_active" AND pm."member_id" = $3)
             AND (p."access" = 0 OR p."owned_by_id" = $3)
             AND p."id" = $4
           ORDER BY p."created_at" DESC"#,
    )
    .bind(slug)
    .bind(project_id)
    .bind(user_id)
    .bind(page_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| db_error(error, "page-visible"))
}

// ---------------------------------------------------------------------------
// Decode + render
// ---------------------------------------------------------------------------

/// One decoded label row: owned strings the [`LabelRow`] borrows.
struct DecodedLabel {
    id: String,
    created_at: String,
    updated_at: String,
    deleted_at: Option<String>,
    name: String,
    description: String,
    color: String,
    sort_order: f64,
    external_source: Option<String>,
    external_id: Option<String>,
    created_by: Option<String>,
    updated_by: Option<String>,
    workspace: String,
    project: Option<String>,
    parent: Option<String>,
}

fn decode_label(row: &sqlx::postgres::PgRow, tz: &Tz) -> Result<DecodedLabel, Denial> {
    Ok(DecodedLabel {
        id: row_uuid(row, "id")?.to_string(),
        created_at: crate::serializer::render_datetime_in(&row_datetime(row, "created_at")?, tz),
        updated_at: crate::serializer::render_datetime_in(&row_datetime(row, "updated_at")?, tz),
        deleted_at: row_datetime_opt(row, "deleted_at")?
            .map(|dt| crate::serializer::render_datetime_in(&dt, tz)),
        name: row_string(row, "name")?,
        description: row_string(row, "description")?,
        color: row_string(row, "color")?,
        sort_order: row_f64(row, "sort_order")?,
        external_source: row_string_opt(row, "external_source")?,
        external_id: row_string_opt(row, "external_id")?,
        created_by: row_uuid_opt(row, "created_by_id")?.map(|id| id.to_string()),
        updated_by: row_uuid_opt(row, "updated_by_id")?.map(|id| id.to_string()),
        workspace: row_uuid(row, "workspace_id")?.to_string(),
        project: row_uuid_opt(row, "project_id")?.map(|id| id.to_string()),
        parent: row_uuid_opt(row, "parent_id")?.map(|id| id.to_string()),
    })
}

fn render_label_value(
    decoded: &DecodedLabel,
    field_specs: Option<&[FieldSpec]>,
    expand_refs: &[&str],
    expansions: &[(&str, Option<Value>)],
) -> Result<Value, Denial> {
    let row = LabelRow {
        id: &decoded.id,
        created_at: &decoded.created_at,
        updated_at: &decoded.updated_at,
        deleted_at: decoded.deleted_at.as_deref(),
        name: &decoded.name,
        description: &decoded.description,
        color: &decoded.color,
        sort_order: decoded.sort_order,
        external_source: decoded.external_source.as_deref(),
        external_id: decoded.external_id.as_deref(),
        created_by: decoded.created_by.as_deref(),
        updated_by: decoded.updated_by.as_deref(),
        workspace: &decoded.workspace,
        project: decoded.project.as_deref(),
        parent: decoded.parent.as_deref(),
    };
    let out = render_label(&LabelRepresentationInput {
        row: &row,
        fields: field_specs,
        expand: expand_refs,
        expansions,
    })
    .map_err(|_| Denial::ServerError)?;
    Ok(Value::Object(out))
}

/// One decoded page row: owned strings the [`PageRow`] borrows.
struct DecodedPage {
    id: String,
    name: String,
    parent: Option<String>,
    owned_by: Option<String>,
    access: i64,
    is_locked: bool,
    archived_at: Option<String>,
    created_at: String,
    updated_at: String,
    description_html: Option<String>,
    description_stripped: Option<String>,
    description_markdown: String,
}

fn decode_page(row: &sqlx::postgres::PgRow, tz: &Tz) -> Result<DecodedPage, Denial> {
    let description_html = row_string_opt(row, "description_html")?;
    let description_markdown = html_to_markdown(description_html.as_deref());
    Ok(DecodedPage {
        id: row_uuid(row, "id")?.to_string(),
        name: row_string(row, "name")?,
        parent: row_uuid_opt(row, "parent_id")?.map(|id| id.to_string()),
        owned_by: row_uuid_opt(row, "owned_by_id")?.map(|id| id.to_string()),
        access: i64::from(row_i16(row, "access")?),
        is_locked: row_bool(row, "is_locked")?,
        archived_at: row_date_opt(row, "archived_at")?.map(|d| d.format("%Y-%m-%d").to_string()),
        created_at: crate::serializer::render_datetime_in(&row_datetime(row, "created_at")?, tz),
        updated_at: crate::serializer::render_datetime_in(&row_datetime(row, "updated_at")?, tz),
        description_html,
        description_stripped: row_string_opt(row, "description_stripped")?,
        description_markdown,
    })
}

fn render_page_lite_value(
    decoded: &DecodedPage,
    field_specs: Option<&[FieldSpec]>,
    expand_refs: &[&str],
    owner: Option<&pidash_services::v1_projects::ser_collab::UserLiteRow<'_>>,
    expansions: &[(&str, Option<Value>)],
) -> Result<Value, Denial> {
    render_page_value(decoded, field_specs, expand_refs, owner, expansions, true)
}

fn render_page_detail_value(
    decoded: &DecodedPage,
    field_specs: Option<&[FieldSpec]>,
    expand_refs: &[&str],
    owner: Option<&pidash_services::v1_projects::ser_collab::UserLiteRow<'_>>,
    expansions: &[(&str, Option<Value>)],
) -> Result<Value, Denial> {
    render_page_value(decoded, field_specs, expand_refs, owner, expansions, false)
}

fn render_page_value(
    decoded: &DecodedPage,
    field_specs: Option<&[FieldSpec]>,
    expand_refs: &[&str],
    owner: Option<&pidash_services::v1_projects::ser_collab::UserLiteRow<'_>>,
    expansions: &[(&str, Option<Value>)],
    lite: bool,
) -> Result<Value, Denial> {
    let row = PageRow {
        id: &decoded.id,
        name: &decoded.name,
        parent: decoded.parent.as_deref(),
        owned_by: decoded.owned_by.as_deref(),
        access: decoded.access,
        is_locked: decoded.is_locked,
        archived_at: decoded.archived_at.as_deref(),
        created_at: &decoded.created_at,
        updated_at: &decoded.updated_at,
        description_html: decoded.description_html.as_deref(),
        description_stripped: decoded.description_stripped.as_deref(),
        description_markdown: &decoded.description_markdown,
    };
    let out = if lite {
        render_page_lite(&PageReadInput {
            row: &row,
            fields: field_specs,
            expand: expand_refs,
            owner,
            expansions,
        })
    } else {
        render_page_detail(&PageReadInput {
            row: &row,
            fields: field_specs,
            expand: expand_refs,
            owner,
            expansions,
        })
    }
    .map_err(|_| Denial::ServerError)?;
    Ok(Value::Object(out))
}

// ---------------------------------------------------------------------------
// Expansions
// ---------------------------------------------------------------------------

/// Decoded user row for [`expand_actor`]: names, nullable `email`,
/// `COALESCE`d avatar, optional avatar asset.
type ExpandUserLookup = (String, String, Option<String>, String, Option<Uuid>);

/// Resolve an avatar/cover file-asset id to its served URL (the D-19
/// `resolve_avatar_url` inputs).
async fn file_asset_url(pool: &PgPool, asset_id: &Uuid) -> Result<Option<String>, Denial> {
    let url: Option<Option<String>> =
        sqlx::query_scalar(r#"SELECT "file_url" FROM "file_assets" WHERE "id" = $1"#)
            .bind(asset_id)
            .fetch_optional(pool)
            .await
            .map_err(|error| db_error(error, "file-asset-url"))?;
    Ok(url.flatten())
}

/// `expand=created_by/updated_by/owned_by/actor`: `UserLiteSerializer`
/// (`user.py:13-38`) via the D-19 `user_lite_to_representation` kernel. A
/// missing row renders null.
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

/// The owner row for `expand=owned_by` on pages: the shape renders the
/// D-19 `UserLite` itself from this row, so the fetch returns the owned
/// strings the row borrows. A missing row is a 500 (Django's FK
/// descriptor raises `RelatedObjectDoesNotExist`).
struct OwnerStrings {
    id: String,
    first_name: String,
    last_name: String,
    email: Option<String>,
    avatar: String,
    avatar_url: Option<String>,
    display_name: String,
}

async fn fetch_owner_strings(
    pool: &PgPool,
    user_id: &Uuid,
) -> Result<Option<OwnerStrings>, Denial> {
    let row: Option<ExpandUserLookup> = sqlx::query_as(
        r#"SELECT "first_name", "last_name", "email", COALESCE("avatar", ''), "avatar_asset_id" FROM "users" WHERE "id" = $1"#,
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| db_error(error, "expand-owner"))?;
    let Some((first_name, last_name, email, avatar, avatar_asset)) = row else {
        return Ok(None);
    };
    let avatar_url = match avatar_asset {
        Some(id) => file_asset_url(pool, &id).await?,
        None => None,
    };
    let avatar_url = pidash_services::v1_projects::ser_collab::resolve_avatar_url(
        avatar_asset.is_some(),
        avatar_url.as_deref(),
        &avatar,
    )
    .map(str::to_owned);
    let display_name: Option<String> =
        sqlx::query_scalar(r#"SELECT "display_name" FROM "users" WHERE "id" = $1"#)
            .bind(user_id)
            .fetch_optional(pool)
            .await
            .map_err(|error| db_error(error, "expand-owner-name"))?
            .flatten();
    Ok(Some(OwnerStrings {
        id: user_id.to_string(),
        first_name,
        last_name,
        email,
        avatar,
        avatar_url,
        display_name: display_name.unwrap_or_default(),
    }))
}

/// `expand=workspace`: `WorkspaceLiteSerializer`: `name`, `slug`, `id` in
/// field order. A missing row renders null.
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
        Value::Bool(row_bool(&row, "is_default")?),
    );
    map.insert(
        "cover_image_url".to_owned(),
        cover_url.map(Value::String).unwrap_or(Value::Null),
    );
    Ok(Value::Object(map))
}

/// `expand=parent` on labels: `IssueLiteSerializer` over the parent LABEL
/// (`serializers/issue.py:497-508`): `id` + `project_id`, with
/// `sequence_id` skipped (a `Label` has none). No live scope: Django
/// renders soft-deleted parents (verified live). A hard-missing row is
/// unreachable (`CASCADE`) and renders `{}` via `None`.
async fn expand_parent_label(pool: &PgPool, parent_id: &Uuid) -> Result<Option<Value>, Denial> {
    let row: Option<(Uuid, Option<Uuid>)> =
        sqlx::query_as(r#"SELECT "id", "project_id" FROM "labels" WHERE "id" = $1"#)
            .bind(parent_id)
            .fetch_optional(pool)
            .await
            .map_err(|error| db_error(error, "expand-parent-label"))?;
    match row {
        Some((id, project_id)) => {
            let id_text = id.to_string();
            match project_id {
                Some(project_id) => {
                    let project_text = project_id.to_string();
                    let map = render_issue_lite(&IssueLiteRow {
                        id: &id_text,
                        sequence_id: None,
                        project_id: &project_text,
                    });
                    Ok(Some(Value::Object(map)))
                }
                // A project-null parent label: `project_id` reads `None`
                // (DRF renders null; `sequence_id` still skips).
                None => {
                    let mut map = Map::with_capacity(2);
                    map.insert("id".to_owned(), Value::String(id_text));
                    map.insert("project_id".to_owned(), Value::Null);
                    Ok(Some(Value::Object(map)))
                }
            }
        }
        None => Ok(None),
    }
}

/// `expand=parent` on pages: `IssueLiteSerializer` over the parent PAGE:
/// `id` only — a `Page` carries neither `sequence_id` nor `project_id`,
/// so both skip (`serializers/issue.py:497-508` over
/// `serializers/base.py:72-117`). No live scope (soft-deleted parents
/// render); a hard-missing row renders `{}` via `None`.
async fn expand_parent_page(pool: &PgPool, parent_id: &Uuid) -> Result<Option<Value>, Denial> {
    let row: Option<Uuid> = sqlx::query_scalar(r#"SELECT "id" FROM "pages" WHERE "id" = $1"#)
        .bind(parent_id)
        .fetch_optional(pool)
        .await
        .map_err(|error| db_error(error, "expand-parent-page"))?;
    match row {
        Some(id) => {
            let mut map = Map::with_capacity(1);
            map.insert("id".to_owned(), Value::String(id.to_string()));
            Ok(Some(Value::Object(map)))
        }
        None => Ok(None),
    }
}

/// Caller expansion values for one label row: every `expand` name in the
/// kept fields with a map hit (`project`, `workspace`, `created_by`,
/// `updated_by`, `parent`). Null FKs supply `None` (the shape renders
/// `{}`); dead rows render null. Anything else is the shape's
/// passthrough/null rule.
async fn label_expansions<'a>(
    pool: &PgPool,
    decoded: &DecodedLabel,
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
            "project" => match decoded.project.as_deref() {
                Some(raw) => {
                    let id = raw.parse::<Uuid>().map_err(|_| Denial::ServerError)?;
                    Some(expand_project(pool, &id).await?)
                }
                None => None,
            },
            "workspace" => {
                let id = decoded
                    .workspace
                    .parse::<Uuid>()
                    .map_err(|_| Denial::ServerError)?;
                Some(expand_workspace(pool, &id).await?)
            }
            "created_by" => match decoded.created_by.as_deref() {
                Some(raw) => {
                    let id = raw.parse::<Uuid>().map_err(|_| Denial::ServerError)?;
                    Some(expand_actor(pool, &id).await?)
                }
                None => None,
            },
            "updated_by" => match decoded.updated_by.as_deref() {
                Some(raw) => {
                    let id = raw.parse::<Uuid>().map_err(|_| Denial::ServerError)?;
                    Some(expand_actor(pool, &id).await?)
                }
                None => None,
            },
            "parent" => match decoded.parent.as_deref() {
                Some(raw) => {
                    let id = raw.parse::<Uuid>().map_err(|_| Denial::ServerError)?;
                    expand_parent_label(pool, &id).await?
                }
                None => None,
            },
            _ => continue,
        };
        expansions.push((*name, value));
    }
    Ok(expansions)
}

/// Caller expansion values for one page row: `parent` only (`owned_by`
/// renders from the fetched owner row). Null FKs supply `None` (the shape
/// renders `{}`).
async fn page_expansions<'a>(
    pool: &PgPool,
    decoded: &DecodedPage,
    expand_refs: &[&'a str],
    kept: &[String],
) -> Result<Vec<(&'a str, Option<Value>)>, Denial> {
    let mut expansions: Vec<(&'a str, Option<Value>)> = Vec::new();
    for name in expand_refs {
        if !kept.iter().any(|kept| kept == name) {
            continue;
        }
        if *name != "parent" {
            continue;
        }
        let value = match decoded.parent.as_deref() {
            Some(raw) => {
                let id = raw.parse::<Uuid>().map_err(|_| Denial::ServerError)?;
                expand_parent_page(pool, &id).await?
            }
            None => None,
        };
        expansions.push((*name, value));
    }
    Ok(expansions)
}

// ---------------------------------------------------------------------------
// Pagination
// ---------------------------------------------------------------------------

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

/// Slice evaluated rows into the requested window: the paginator's
/// `[offset, stop)` over `queryset[offset:stop]`, the backwards-walk
/// mismatch 500, and the `[:limit]` trim.
fn window_rows<'a, T>(
    rows: &'a [T],
    per_page: i64,
    cursor: &crate::paginator::Cursor,
) -> Result<(&'a [T], bool), Denial> {
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
    // (a negative offset still 400s first).
    if per_page < 0 {
        return Err(page_denial(crate::paginator::PageError::NegativeSlice));
    }
    let start = (window.offset as usize).min(rows.len());
    let stop = (window.stop as usize).min(rows.len());
    let window_rows = &rows[start..stop];
    // Backwards walk with a mismatched cursor value reads nothing
    // (`results[-(limit+1):]` on the lazy queryset raises into the 500).
    if !cursor.value.equals_limit(per_page) && cursor.is_prev {
        return Err(Denial::ServerError);
    }
    let has_more = window_rows.len() as i64 > per_page;
    let trim = (per_page as usize).min(window_rows.len());
    Ok((&window_rows[..trim], has_more))
}

/// Split `fields=`/`expand=` into shape specs plus the kept field list
/// (for expansion gating).
type FieldSelection = (Option<Vec<FieldSpec>>, Vec<String>, Vec<String>);

fn field_selection(query: &QueryMap, available: &[&str]) -> Result<FieldSelection, Denial> {
    let fields = fields_param(query, "fields");
    let specs: Vec<FieldSpec> = fields
        .as_deref()
        .unwrap_or(&[])
        .iter()
        .map(|name| FieldSpec::Include(name.clone()))
        .collect();
    let field_specs = if fields.is_some() { Some(specs) } else { None };
    let kept = filter_fields(available, field_specs.as_deref()).map_err(|_| Denial::ServerError)?;
    let expand = fields_param(query, "expand").unwrap_or_default();
    Ok((field_specs, kept, expand))
}

// ---------------------------------------------------------------------------
// Request bodies
// ---------------------------------------------------------------------------

/// The write body spec: no field of either write serializer is a
/// `ListField`, so repeated form values last-win everywhere
/// (`fields.py:407-429` `get_value` scalar rule).
const WRITE_BODY_SPEC: crate::v1_cycles_modules::body::BodySpec =
    crate::v1_cycles_modules::body::BodySpec {
        list_fields: &[],
        skip_blank_fields: &[],
    };

/// A parsed write body: the JSON value plus whether it arrived as an HTML
/// form (DRF `Field.get_value` maps present-`''` per field for HTML
/// input, while JSON `""` runs `to_internal_value` — see
/// [`apply_form_blanks`]).
struct WriteBody {
    value: Value,
    from_form: bool,
}

/// Parse a label/page write body: content-type dispatch (415 for the
/// rest), empty bodies to `{}`, JSON through the CPython parser, forms
/// through the HTML-input kernel. Uploads are ignored (no file field
/// exists here — an unpinned edge; Python would 400 them as non-strings).
fn parse_write_body(headers: &HeaderMap, body: &[u8]) -> Result<WriteBody, Denial> {
    use crate::v1_cycles_modules::body::{negotiate_body, BodyError, NegotiatedBody};
    use crate::v1_cycles_modules::json_cpython::{
        parse_json_text_spans, to_serde_publish, to_serde_publish_map, JsonFail,
    };
    match negotiate_body(headers, body, &WRITE_BODY_SPEC) {
        Err(BodyError::UnsupportedMediaType(detail)) => Err(Denial::UnsupportedMediaType(detail)),
        Err(BodyError::ParseDetail(detail)) => Err(Denial::BadDetail(detail)),
        Err(BodyError::ServerError) => Err(Denial::ServerError),
        Ok(NegotiatedBody::Empty) => Ok(WriteBody {
            value: Value::Object(Map::new()),
            from_form: false,
        }),
        Ok(NegotiatedBody::Form { map, .. }) => Ok(WriteBody {
            value: Value::Object(map),
            from_form: true,
        }),
        Ok(NegotiatedBody::JsonText { text, surr }) => match parse_json_text_spans(&text, &surr) {
            Err(JsonFail::Message(detail)) => {
                Err(Denial::BadDetail(format!("JSON parse error - {detail}")))
            }
            Err(JsonFail::Recursion) => Err(Denial::ServerError),
            Ok(value) => {
                if value.is_object() {
                    let object = value.into_object().expect("checked object");
                    Ok(WriteBody {
                        value: Value::Object(to_serde_publish_map(&object)),
                        from_form: false,
                    })
                } else {
                    Ok(WriteBody {
                        value: to_serde_publish(&value),
                        from_form: false,
                    })
                }
            }
        },
    }
}

/// DRF `Field.get_value` (`fields.py:407-429`) for HTML-form input: a
/// present-`''` maps per field, while JSON `""` always runs
/// `to_internal_value`. `skip` keys drop the `''` (as if absent);
/// `to_null` keys map it to JSON null. Every other key keeps `''`, which
/// validates exactly like JSON `""`.
///
/// * label POST/PATCH: `sort_order` (not required, no null/blank → skip),
///   `parent` (null allowed, blank not → null).
/// * page POST: `access` (skip), `parent` (null); `name` is required, so
///   `''` fails blank like JSON.
/// * page PATCH: `name` + `access` (skip), `parent` (null).
fn apply_form_blanks(body: &mut Value, skip: &[&str], to_null: &[&str]) {
    let Value::Object(map) = body else {
        return;
    };
    for key in skip {
        if map
            .get(*key)
            .is_some_and(|v| v == &Value::String(String::new()))
        {
            map.remove(*key);
        }
    }
    for key in to_null {
        if map
            .get(*key)
            .is_some_and(|v| v == &Value::String(String::new()))
        {
            map.insert((*key).to_owned(), Value::Null);
        }
    }
}

// ---------------------------------------------------------------------------
// Label handlers
// ---------------------------------------------------------------------------

async fn read_body(body: axum::body::Body) -> Result<Vec<u8>, Denial> {
    axum::body::to_bytes(body, usize::MAX)
        .await
        .map(|bytes| bytes.to_vec())
        .map_err(|error| db_error(error, "read-body"))
}

/// Rebuild the request against the ORIGINAL path for the proxy
/// (Django's `<uuid:>` converter would not match — before auth runs, as URL
/// resolving precedes it).
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

/// The related-objects sweep payload (`SoftDeleteModel.delete()` calls
/// `.delay(app_label, model_name, pk, using=None)`): positionals
/// `("db", model, pk)`, kwargs `{"using": null}`.
fn soft_delete_sweep(model_name: &str, pk: &str) -> (Vec<Value>, Map<String, Value>) {
    let mut kwargs = Map::with_capacity(1);
    kwargs.insert("using".to_owned(), Value::Null);
    (
        vec![
            Value::String("db".to_owned()),
            Value::String(model_name.to_owned()),
            Value::String(pk.to_owned()),
        ],
        kwargs,
    )
}

/// Python truthiness of a raw request-data value for the external-dup
/// probes (`request.data.get(...) and ...`, `views/issue.py:1364-1366,
/// 1502-1504`): missing/null/blank-string/zero/false/empty-array all skip.
fn is_truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
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

/// `GET .../labels/` (`views/issue.py:1428-1437`).
pub async fn get_label_list(
    State(state): State<AppState>,
    Path((slug, project_id)): Path<(String, String)>,
    Query(query): Query<QueryMap>,
    headers: HeaderMap,
) -> Response {
    match label_list_inner(&state, &headers, &slug, &project_id, &query).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

async fn label_list_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    project_id_raw: &str,
    query: &QueryMap,
) -> Result<Response, Denial> {
    use pidash_services::v1_work_items::shape_labels::LABEL_READ_FIELDS;
    let pre = preamble(state, headers, slug).await?;
    let project_id = rewrite_project_id(&pre.pool, slug, project_id_raw).await?;
    let workspace_id = pre.workspace_id.ok_or(Denial::Forbidden)?;
    require_gate(
        &pre.pool,
        &workspace_id,
        slug,
        &pre.actor.id,
        &project_id,
        V1WorkItemsRoute::LabelList,
        "GET",
    )
    .await?;
    let tz = activate_timezone(pre.actor.timezone.as_deref())?;
    let per_page =
        crate::paginator::parse_per_page(query_last(query, "per_page").as_deref(), 1000, 1000)
            .map_err(page_denial)?;
    let cursor_raw = query_last(query, "cursor").unwrap_or_else(|| format!("{per_page}:0:0"));
    let cursor = crate::paginator::Cursor::from_string(&cursor_raw).map_err(page_denial)?;
    let rows = fetch_label_rows(&pre.pool, slug, &project_id, &pre.actor.id).await?;
    let total_count = rows.len() as i64;
    let (page_rows, has_more) = window_rows(&rows, per_page, &cursor)?;
    let (field_specs, kept, expand) = field_selection(query, LABEL_READ_FIELDS)?;
    let expand_refs: Vec<&str> = expand.iter().map(String::as_str).collect();
    let mut rendered: Vec<Value> = Vec::with_capacity(page_rows.len());
    for row in page_rows {
        let decoded = decode_label(row, &tz)?;
        let expansions = label_expansions(&pre.pool, &decoded, &expand_refs, &kept).await?;
        rendered.push(render_label_value(
            &decoded,
            field_specs.as_deref(),
            &expand_refs,
            &expansions,
        )?);
    }
    let next = crate::paginator::next_cursor(per_page, cursor.offset, has_more);
    let prev = crate::paginator::prev_cursor(per_page, cursor.offset);
    envelope(total_count, per_page, &next, &prev, Value::Array(rendered))
}

/// `POST .../labels/` (`views/issue.py:1355-1405`).
pub async fn post_label(
    State(state): State<AppState>,
    Path((slug, project_id)): Path<(String, String)>,
    headers: HeaderMap,
    body: axum::body::Body,
) -> Response {
    let raw = match read_body(body).await {
        Ok(raw) => raw,
        Err(denial) => return denial.into_response(),
    };
    match label_post_inner(&state, &headers, &slug, &project_id, &raw).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

async fn label_post_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    project_id_raw: &str,
    raw_body: &[u8],
) -> Result<Response, Denial> {
    let pre = preamble(state, headers, slug).await?;
    let project_id = rewrite_project_id(&pre.pool, slug, project_id_raw).await?;
    let workspace_id = pre.workspace_id.ok_or(Denial::Forbidden)?;
    require_gate(
        &pre.pool,
        &workspace_id,
        slug,
        &pre.actor.id,
        &project_id,
        V1WorkItemsRoute::LabelList,
        "POST",
    )
    .await?;
    let tz = activate_timezone(pre.actor.timezone.as_deref())?;
    let mut parsed = parse_write_body(headers, raw_body)?;
    if parsed.from_form {
        apply_form_blanks(&mut parsed.value, &["sort_order"], &["parent"]);
    }
    // The `UniqueValidator` fact (project-null, non-deleted clash,
    // `db/models/label.py:29-33` — BUG-4: the within-project constraint
    // has no validator).
    let name_key = name_lookup_key(&parsed.value);
    let name_exists = match name_key.as_deref() {
        Some(name) => Some(label_name_exists(&pre.pool, name, None).await?),
        None => None,
    };
    let parent_key = parent_lookup_key(&parsed.value);
    let parent_exists = match parent_key.as_deref() {
        Some(key) => Some(label_pk_exists(&pre.pool, key).await?),
        None => None,
    };
    let validated = validate_label_write(&LabelWriteInput {
        body: &parsed.value,
        partial: false,
        parent_exists,
        name_exists,
    })
    .map_err(|error| match error.body() {
        Some(body) => Denial::FieldErrors(body.to_owned()),
        None => Denial::ServerError,
    })?;
    // The external-dup probe reads the RAW request data (`:1364-1372`),
    // before the save.
    if is_truthy(parsed.value.get("external_id")) && is_truthy(parsed.value.get("external_source"))
    {
        let external_id = raw_text(parsed.value.get("external_id"));
        let external_source = raw_text(parsed.value.get("external_source"));
        if let Some(existing) = label_external_hit(
            &pre.pool,
            slug,
            &project_id,
            &external_source,
            &external_id,
            None,
        )
        .await?
        {
            return Err(label_dup_conflict(&existing.to_string()));
        }
    }
    // `WorkspaceBaseModel.save` reads `self.project.workspace`
    // (`workspace.py:192-195`): a bogus project UUID misses here with
    // `RelatedObjectDoesNotExist` → the 404 branch, before any INSERT.
    let label_workspace: Option<Uuid> =
        sqlx::query_scalar(r#"SELECT "workspace_id" FROM "projects" WHERE "id" = $1"#)
            .bind(project_id)
            .fetch_optional(&pre.pool)
            .await
            .map_err(|error| db_error(error, "label-post-workspace"))?
            .flatten();
    let Some(label_workspace) = label_workspace else {
        return Err(Denial::NotFound(RESOURCE_NOT_FOUND_BODY.to_owned()));
    };
    // `Label.save` (`label.py:46-54`, BUG-2): `MAX(sort_order)+10000` over
    // the project's live labels when any exist (an explicit `sort_order`
    // is overwritten); otherwise the passed value or the 65535 default.
    let max_sort: Option<f64> = sqlx::query_scalar(
        r#"SELECT MAX("sort_order") FROM "labels" WHERE "project_id" = $1 AND "deleted_at" IS NULL"#,
    )
    .bind(project_id)
    .fetch_optional(&pre.pool)
    .await
    .map_err(|error| db_error(error, "label-post-max"))?
    .flatten();
    let sort_order = match max_sort {
        Some(max) => max + 10000.0,
        None => validated.sort_order.unwrap_or(65535.0),
    };
    let label_id = Uuid::new_v4();
    // Two `now()` calls like the two `auto_now`/`auto_now_add` pre_saves.
    let created_at = now_utc();
    let updated_at = now_utc();
    let parent_id: Option<Uuid> = match validated.parent {
        Some(Some(ref raw)) => Some(raw.parse::<Uuid>().map_err(|_| Denial::ServerError)?),
        _ => None,
    };
    let insert = sqlx::query(
        r#"INSERT INTO "labels" ("id", "created_at", "updated_at", "created_by_id", "updated_by_id",
            "workspace_id", "project_id", "parent_id", "name", "description", "color",
            "sort_order", "external_source", "external_id")
           VALUES ($1, $2, $3, $4, NULL, $5, $6, $7, $8, $9, $10, $11, $12, $13)"#,
    )
    .bind(label_id)
    .bind(created_at)
    .bind(updated_at)
    .bind(pre.actor.id)
    .bind(label_workspace)
    .bind(project_id)
    .bind(parent_id)
    .bind(validated.name.clone().unwrap_or_default())
    .bind(validated.description.clone().unwrap_or_default())
    .bind(validated.color.clone().unwrap_or_default())
    .bind(sort_order)
    .bind(validated.external_source.clone().unwrap_or(None))
    .bind(validated.external_id.clone().unwrap_or(None))
    .execute(&pre.pool)
    .await;
    if let Err(error) = insert {
        if is_integrity_error(&error) {
            // The `except IntegrityError` arm (`:1393-1405`, BUG-3): name
            // lookup by RAW name → 409, or the `None.id` 500.
            return label_post_integrity_arm(
                &pre.pool,
                slug,
                &project_id,
                parsed.value.get("name"),
            )
            .await;
        }
        return Err(db_error(error, "label-post-insert"));
    }
    // The response re-reads the row (`Label.objects.get`, `:1389`).
    let row = fetch_label_row_by_pk(&pre.pool, &label_id).await?;
    let Some(row) = row else {
        return Err(Denial::ServerError);
    };
    let decoded = decode_label(&row, &tz)?;
    let body = render_label_value(&decoded, None, &[], &[])?;
    Ok(json_created(
        serde_json::to_string(&body).map_err(|_| Denial::ServerError)?,
    ))
}

/// The POST `except IntegrityError` arm (`views/issue.py:1393-1405`):
/// look the RAW name up in this project; a hit 409s with its id, a miss
/// 500s on `None.id` (BUG-3).
async fn label_post_integrity_arm(
    pool: &PgPool,
    slug: &str,
    project_id: &Uuid,
    raw_name: Option<&Value>,
) -> Result<Response, Denial> {
    let name = raw_text(raw_name);
    let hit: Option<Uuid> = sqlx::query_scalar(
        r#"SELECT l."id" FROM "labels" l
           WHERE l."deleted_at" IS NULL
             AND l."workspace_id" = (SELECT "id" FROM "workspaces" WHERE "slug" = $1)
             AND l."project_id" = $2 AND l."name" = $3
           ORDER BY l."created_at" DESC LIMIT 1"#,
    )
    .bind(slug)
    .bind(project_id)
    .bind(&name)
    .fetch_optional(pool)
    .await
    .map_err(|error| db_error(error, "label-post-dup-lookup"))?
    .flatten();
    match hit {
        Some(id) => Err(label_name_conflict(&id.to_string())),
        // BUG-3: `str(label.id)` with `label is None` raises
        // `AttributeError` → the generic 500 branch.
        None => Err(Denial::ServerError),
    }
}

/// `GET .../labels/<pk>/` (`views/issue.py:1463-1470`).
pub async fn get_label_detail(
    State(state): State<AppState>,
    OriginalUri(original): OriginalUri,
    Path((slug, project_id, pk)): Path<(String, String, String)>,
    headers: HeaderMap,
) -> Response {
    if !crate::runner_runs::is_uuid_path_segment(&pk) {
        return proxy_request(&state, "GET", original.to_string()).await;
    }
    let pk = pk.parse::<Uuid>().expect("checked segment");
    match label_detail_inner(&state, &headers, &slug, &project_id, &pk).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

async fn label_detail_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    project_id_raw: &str,
    pk: &Uuid,
) -> Result<Response, Denial> {
    let pre = preamble(state, headers, slug).await?;
    let project_id = rewrite_project_id(&pre.pool, slug, project_id_raw).await?;
    let workspace_id = pre.workspace_id.ok_or(Denial::Forbidden)?;
    require_gate(
        &pre.pool,
        &workspace_id,
        slug,
        &pre.actor.id,
        &project_id,
        V1WorkItemsRoute::LabelDetail,
        "GET",
    )
    .await?;
    let tz = activate_timezone(pre.actor.timezone.as_deref())?;
    let row = fetch_label_detail(&pre.pool, slug, &project_id, &pre.actor.id, pk).await?;
    let Some(row) = row else {
        // `.get()` miss → `ObjectDoesNotExist` → the 404 branch.
        return Err(Denial::NotFound(RESOURCE_NOT_FOUND_BODY.to_owned()));
    };
    let decoded = decode_label(&row, &tz)?;
    let body = render_label_value(&decoded, None, &[], &[])?;
    Ok(json_response(
        StatusCode::OK,
        serde_json::to_string(&body).map_err(|_| Denial::ServerError)?,
    ))
}

/// `PATCH .../labels/<pk>/` (`views/issue.py:1493-1525`).
pub async fn patch_label(
    State(state): State<AppState>,
    OriginalUri(original): OriginalUri,
    Path((slug, project_id, pk)): Path<(String, String, String)>,
    headers: HeaderMap,
    body: axum::body::Body,
) -> Response {
    if !crate::runner_runs::is_uuid_path_segment(&pk) {
        return proxy_request(&state, "PATCH", original.to_string()).await;
    }
    let pk = pk.parse::<Uuid>().expect("checked segment");
    let raw = match read_body(body).await {
        Ok(raw) => raw,
        Err(denial) => return denial.into_response(),
    };
    match label_patch_inner(&state, &headers, &slug, &project_id, &pk, &raw).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

async fn label_patch_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    project_id_raw: &str,
    pk: &Uuid,
    raw_body: &[u8],
) -> Result<Response, Denial> {
    let pre = preamble(state, headers, slug).await?;
    let project_id = rewrite_project_id(&pre.pool, slug, project_id_raw).await?;
    let workspace_id = pre.workspace_id.ok_or(Denial::Forbidden)?;
    require_gate(
        &pre.pool,
        &workspace_id,
        slug,
        &pre.actor.id,
        &project_id,
        V1WorkItemsRoute::LabelDetail,
        "PATCH",
    )
    .await?;
    let tz = activate_timezone(pre.actor.timezone.as_deref())?;
    // The queryset `.get()` runs BEFORE validation (`:1499`).
    let existing = fetch_label_detail(&pre.pool, slug, &project_id, &pre.actor.id, pk).await?;
    if existing.is_none() {
        return Err(Denial::NotFound(RESOURCE_NOT_FOUND_BODY.to_owned()));
    }
    let mut parsed = parse_write_body(headers, raw_body)?;
    if parsed.from_form {
        apply_form_blanks(&mut parsed.value, &["sort_order"], &["parent"]);
    }
    let name_key = name_lookup_key(&parsed.value);
    let name_exists = match name_key.as_deref() {
        // DRF excludes the instance from the unique check.
        Some(name) => Some(label_name_exists(&pre.pool, name, Some(pk)).await?),
        None => None,
    };
    let parent_key = parent_lookup_key(&parsed.value);
    let parent_exists = match parent_key.as_deref() {
        Some(key) => Some(label_pk_exists(&pre.pool, key).await?),
        None => None,
    };
    let validated = validate_label_write(&LabelWriteInput {
        body: &parsed.value,
        partial: true,
        parent_exists,
        name_exists,
    })
    .map_err(|error| match error.body() {
        Some(body) => Denial::FieldErrors(body.to_owned()),
        None => Denial::ServerError,
    })?;
    if is_truthy(parsed.value.get("external_id")) && is_truthy(parsed.value.get("external_source"))
    {
        let external_id = raw_text(parsed.value.get("external_id"));
        let external_source = raw_text(parsed.value.get("external_source"));
        if label_external_hit(
            &pre.pool,
            slug,
            &project_id,
            &external_source,
            &external_id,
            Some(pk),
        )
        .await?
        .is_some()
        {
            // The PATCH 409 reports the PATCHED label's id (`:1517`),
            // not the conflicting row's.
            return Err(label_dup_conflict(&pk.to_string()));
        }
    }
    // `serializer.save()` is a full save (BUG-4: a within-project name
    // clash raises the uncaught `IntegrityError` → 400); crum stamps
    // `updated_by`, `auto_now` stamps `updated_at`.
    let updated_at = now_utc();
    let parent_id: Option<Option<Uuid>> = match validated.parent {
        Some(Some(ref raw)) => Some(Some(raw.parse::<Uuid>().map_err(|_| Denial::ServerError)?)),
        Some(None) => Some(None),
        None => None,
    };
    let update = sqlx::query(
        r#"UPDATE "labels" SET "updated_at" = $1, "updated_by_id" = $2,
               "name" = COALESCE($3, "name"),
               "description" = COALESCE($4, "description"),
               "color" = COALESCE($5, "color"),
               "sort_order" = COALESCE($6, "sort_order"),
               "external_source" = CASE WHEN $7 THEN NULL ELSE COALESCE($8, "external_source") END,
               "external_id" = CASE WHEN $9 THEN NULL ELSE COALESCE($10, "external_id") END,
               "parent_id" = CASE WHEN $11 THEN NULL ELSE COALESCE($12, "parent_id") END
           WHERE "id" = $13"#,
    )
    .bind(updated_at)
    .bind(pre.actor.id)
    .bind(validated.name.clone())
    .bind(validated.description.clone())
    .bind(validated.color.clone())
    .bind(validated.sort_order)
    .bind(validated.external_source == Some(None))
    .bind(validated.external_source.clone().unwrap_or(None))
    .bind(validated.external_id == Some(None))
    .bind(validated.external_id.clone().unwrap_or(None))
    .bind(parent_id == Some(None))
    .bind(parent_id.unwrap_or(None))
    .bind(pk)
    .execute(&pre.pool)
    .await;
    if let Err(error) = update {
        // BUG-4: uncaught → `handle_exception` → 400.
        if is_integrity_error(&error) {
            return Err(Denial::FieldErrors(PAYLOAD_NOT_VALID_BODY.to_owned()));
        }
        return Err(db_error(error, "label-patch-update"));
    }
    let row = fetch_label_row_by_pk(&pre.pool, pk).await?;
    let Some(row) = row else {
        return Err(Denial::ServerError);
    };
    let decoded = decode_label(&row, &tz)?;
    let body = render_label_value(&decoded, None, &[], &[])?;
    Ok(json_response(
        StatusCode::OK,
        serde_json::to_string(&body).map_err(|_| Denial::ServerError)?,
    ))
}

/// `DELETE .../labels/<pk>/` (`views/issue.py:1538-1546`): soft delete
/// (`deleted_at` + full-save stamps) plus the related-objects sweep.
pub async fn delete_label(
    State(state): State<AppState>,
    OriginalUri(original): OriginalUri,
    Path((slug, project_id, pk)): Path<(String, String, String)>,
    headers: HeaderMap,
) -> Response {
    if !crate::runner_runs::is_uuid_path_segment(&pk) {
        return proxy_request(&state, "DELETE", original.to_string()).await;
    }
    let pk = pk.parse::<Uuid>().expect("checked segment");
    match label_delete_inner(&state, &headers, &slug, &project_id, &pk).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

async fn label_delete_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    project_id_raw: &str,
    pk: &Uuid,
) -> Result<Response, Denial> {
    let pre = preamble(state, headers, slug).await?;
    let project_id = rewrite_project_id(&pre.pool, slug, project_id_raw).await?;
    let workspace_id = pre.workspace_id.ok_or(Denial::Forbidden)?;
    require_gate(
        &pre.pool,
        &workspace_id,
        slug,
        &pre.actor.id,
        &project_id,
        V1WorkItemsRoute::LabelDetail,
        "DELETE",
    )
    .await?;
    let existing = fetch_label_detail(&pre.pool, slug, &project_id, &pre.actor.id, pk).await?;
    if existing.is_none() {
        return Err(Denial::NotFound(RESOURCE_NOT_FOUND_BODY.to_owned()));
    }
    // `delete()`: `deleted_at = now()` then a full `save()` — two `now()`
    // calls plus the crum `updated_by` stamp (`mixins.py:72-78`).
    let deleted_at = now_utc();
    let updated_at = now_utc();
    sqlx::query(
        r#"UPDATE "labels" SET "deleted_at" = $1, "updated_at" = $2, "updated_by_id" = $3 WHERE "id" = $4"#,
    )
    .bind(deleted_at)
    .bind(updated_at)
    .bind(pre.actor.id)
    .bind(pk)
    .execute(&pre.pool)
    .await
    .map_err(|error| db_error(error, "label-delete"))?;
    let (sweep_args, sweep_kwargs) = soft_delete_sweep("label", &pk.to_string());
    enqueue_best_effort(&pre.pool, SOFT_DELETE_TASK, sweep_args, sweep_kwargs).await;
    Ok(Response::builder()
        .status(StatusCode::NO_CONTENT)
        .body(axum::body::Body::empty())
        .expect("empty response"))
}

// ---------------------------------------------------------------------------
// Label write probes
// ---------------------------------------------------------------------------

/// The `UniqueValidator` fact: a project-null, non-deleted label carrying
/// `name` (`Label.objects`, self excluded on PATCH).
async fn label_name_exists(
    pool: &PgPool,
    name: &str,
    exclude_pk: Option<&Uuid>,
) -> Result<bool, Denial> {
    let hit: bool = if let Some(pk) = exclude_pk {
        sqlx::query_scalar(
            r#"SELECT EXISTS(SELECT 1 FROM "labels" WHERE "deleted_at" IS NULL AND "project_id" IS NULL AND "name" = $1 AND "id" != $2)"#,
        )
        .bind(name)
        .bind(pk)
        .fetch_optional(pool)
        .await
        .map_err(|error| db_error(error, "label-name-exists"))?
        .unwrap_or(false)
    } else {
        sqlx::query_scalar(
            r#"SELECT EXISTS(SELECT 1 FROM "labels" WHERE "deleted_at" IS NULL AND "project_id" IS NULL AND "name" = $1)"#,
        )
        .bind(name)
        .fetch_optional(pool)
        .await
        .map_err(|error| db_error(error, "label-name-exists"))?
        .unwrap_or(false)
    };
    Ok(hit)
}

/// The parent-PK fact: a live label carrying the canonical pk
/// (`PrimaryKeyRelatedField`, global `Label.objects` scope).
async fn label_pk_exists(pool: &PgPool, canonical_pk: &str) -> Result<bool, Denial> {
    let pk = canonical_pk
        .parse::<Uuid>()
        .map_err(|_| Denial::ServerError)?;
    let hit: bool = sqlx::query_scalar(
        r#"SELECT EXISTS(SELECT 1 FROM "labels" WHERE "deleted_at" IS NULL AND "id" = $1)"#,
    )
    .bind(pk)
    .fetch_optional(pool)
    .await
    .map_err(|error| db_error(error, "label-pk-exists"))?
    .unwrap_or(false);
    Ok(hit)
}

/// The external-dup probe (`:1367-1372`, `:1505-1512`): same
/// project/slug/source/id (PATCH excludes self). Returns the conflicting
/// row id (POST reports it; PATCH reports the patched id instead).
async fn label_external_hit(
    pool: &PgPool,
    slug: &str,
    project_id: &Uuid,
    external_source: &str,
    external_id: &str,
    exclude_pk: Option<&Uuid>,
) -> Result<Option<Uuid>, Denial> {
    if let Some(pk) = exclude_pk {
        sqlx::query_scalar(
            r#"SELECT l."id" FROM "labels" l
               WHERE l."deleted_at" IS NULL AND l."project_id" = $1
                 AND l."workspace_id" = (SELECT "id" FROM "workspaces" WHERE "slug" = $2)
                 AND l."external_source" = $3 AND l."external_id" = $4 AND l."id" != $5
               ORDER BY l."created_at" DESC LIMIT 1"#,
        )
        .bind(project_id)
        .bind(slug)
        .bind(external_source)
        .bind(external_id)
        .bind(pk)
        .fetch_optional(pool)
        .await
        .map_err(|error| db_error(error, "label-external-hit"))
    } else {
        sqlx::query_scalar(
            r#"SELECT l."id" FROM "labels" l
               WHERE l."deleted_at" IS NULL AND l."project_id" = $1
                 AND l."workspace_id" = (SELECT "id" FROM "workspaces" WHERE "slug" = $2)
                 AND l."external_source" = $3 AND l."external_id" = $4
               ORDER BY l."created_at" DESC LIMIT 1"#,
        )
        .bind(project_id)
        .bind(slug)
        .bind(external_source)
        .bind(external_id)
        .fetch_optional(pool)
        .await
        .map_err(|error| db_error(error, "label-external-hit"))
    }
}

/// Python `str()` of a raw request-data scalar for the dup probes
/// (Django compares the raw value in SQL; non-strings stringify).
fn raw_text(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Bool(true)) => "True".to_owned(),
        Some(Value::Bool(false)) => "False".to_owned(),
        Some(Value::Null) | None => "None".to_owned(),
        Some(Value::Array(_) | Value::Object(_)) => value
            .map(std::string::ToString::to_string)
            .unwrap_or_default(),
    }
}

fn label_dup_conflict(id: &str) -> Denial {
    Denial::Conflict(format!(
        "{{\"error\":{},\"id\":{}}}",
        json_string(LABEL_DUP_MESSAGE),
        json_string(id)
    ))
}

fn label_name_conflict(id: &str) -> Denial {
    Denial::Conflict(format!(
        "{{\"error\":{},\"id\":{}}}",
        json_string(LABEL_NAME_DUP_MESSAGE),
        json_string(id)
    ))
}

/// Re-read one label by pk, unscoped (`Label.objects.get`, `:1389,:1522`).
async fn fetch_label_row_by_pk(
    pool: &PgPool,
    pk: &Uuid,
) -> Result<Option<sqlx::postgres::PgRow>, Denial> {
    sqlx::query(r#"SELECT * FROM "labels" WHERE "deleted_at" IS NULL AND "id" = $1"#)
        .bind(pk)
        .fetch_optional(pool)
        .await
        .map_err(|error| db_error(error, "label-reread"))
}

// ---------------------------------------------------------------------------
// Page module helpers (views/page.py:104-172)
// ---------------------------------------------------------------------------

/// `_error` (`page.py:104-111`): the v1 error envelope; `code` adds the
/// numeric `error_code` plus the `error_message`.
fn page_error(message: &str, status: StatusCode, code: Option<(&str, u32)>) -> Denial {
    let mut body = format!("{{\"error\":{}}}", json_string(message));
    if let Some((name, numeric)) = code {
        body = format!(
            "{{\"error\":{},\"error_code\":{numeric},\"error_message\":{}}}",
            json_string(message),
            json_string(name)
        );
    }
    match status {
        StatusCode::BAD_REQUEST => Denial::BadError(message.to_owned()),
        StatusCode::FORBIDDEN => Denial::ForbiddenError(body),
        StatusCode::NOT_FOUND => Denial::NotFound(body),
        StatusCode::CONFLICT => Denial::Conflict(body),
        _ => Denial::ServerError,
    }
}

/// `_conversion_failed` (`page.py:114-118`): 503, nothing was written.
fn conversion_failed(message: &str) -> Denial {
    Denial::ServiceUnavailable(format!(
        "{{\"error\":{}}}",
        json_string(&format!(
            "The page body could not be saved and nothing was written: {message}."
        ))
    ))
}

/// `render_body_html` (`page.py:121-134`): turn a validated write
/// payload's body into sanitised Tiptap HTML. `Err` carries the 400
/// message (`ValueError` in Python).
fn render_body_html(
    description_markdown: Option<&str>,
    description_html: Option<&str>,
) -> Result<String, String> {
    if let Some(markdown) = description_markdown {
        return markdown_to_html(markdown).map_err(|message| message.clone());
    }
    let html = description_html.unwrap_or("");
    if html.trim().is_empty() {
        return Ok(EMPTY_BODY_HTML.to_owned());
    }
    match crate::space::sanitize::sanitize_html(html) {
        crate::space::sanitize::Sanitize::Clean(clean) => Ok(if clean.is_empty() {
            EMPTY_BODY_HTML.to_owned()
        } else {
            clean
        }),
        crate::space::sanitize::Sanitize::Invalid => {
            Err(if html.len() > crate::space::sanitize::MAX_HTML_BYTES {
                "HTML content exceeds maximum size limit (10MB)".to_owned()
            } else {
                "Failed to sanitize HTML".to_owned()
            })
        }
    }
}

/// Tiptap's empty document (`markdown_converter.py:205`).
const EMPTY_BODY_HTML: &str = "<p></p>";

/// The three stored body fields `_document_fields` returns.
struct DocumentFields {
    description_html: String,
    description_json: Value,
    description_binary: Vec<u8>,
}

/// `_document_fields` (`page.py:137-153`) through
/// `PageBinaryUpdateSerializer` (`app/serializers/page.py:173-225`):
/// base64 → `validate_binary_data`, HTML → `validate_html_content` +
/// sanitised, JSON passes through. `Err` carries the byte-exact 400
/// errors body (`is_valid(raise_exception=True)`).
fn document_fields(document: &LiveDocument) -> Result<DocumentFields, String> {
    // `validate_description_binary` (`:180-198`): empty stays empty (then
    // `validated_data["description_binary"]` below — the live server
    // always sends a non-empty binary, so the missing arm is unreachable
    // past `convert_document`).
    let binary = if document.description_binary.is_empty() {
        Vec::new()
    } else {
        // The serializer base64-round-trips the bytes (`:143-146`
        // encode, `:188` decode — lenient, like `b64decode` without
        // `validate`); the D-30 port covers the validator messages.
        let encoded = base64_encode(&document.description_binary);
        let decoded = base64_decode_lenient(&encoded).ok_or_else(|| {
            field_errors_body(&[(
                "description_binary",
                vec!["Failed to decode base64 data".to_owned()],
            )])
        })?;
        if let Err(message) = pidash_services::app_pages::shape::validate_binary_data(&decoded) {
            return Err(field_errors_body(&[(
                "description_binary",
                vec![format!("Invalid binary data: {message}")],
            )]));
        }
        decoded
    };
    // `validate_description_html` (`:200-211`): empty stays empty (then
    // `or EMPTY_BODY_HTML` below); otherwise sanitise.
    let html = if document.description_html.is_empty() {
        String::new()
    } else {
        match crate::space::sanitize::sanitize_html(&document.description_html) {
            crate::space::sanitize::Sanitize::Clean(clean) => clean,
            crate::space::sanitize::Sanitize::Invalid => {
                let message =
                    if document.description_html.len() > crate::space::sanitize::MAX_HTML_BYTES {
                        "HTML content exceeds maximum size limit (10MB)"
                    } else {
                        "Failed to sanitize HTML"
                    };
                return Err(field_errors_body(&[(
                    "description_html",
                    vec![message.to_owned()],
                )]));
            }
        }
    };
    Ok(DocumentFields {
        description_html: if html.is_empty() {
            EMPTY_BODY_HTML.to_owned()
        } else {
            html
        },
        description_json: if document.description_json.is_null() {
            Value::Object(Map::new())
        } else {
            document.description_json.clone()
        },
        description_binary: binary,
    })
}

/// `{"<field>": ["<message>", …]}` in field order (DRF errors dict).
fn field_errors_body(entries: &[(&str, Vec<String>)]) -> String {
    let mut map = Map::with_capacity(entries.len());
    for (field, messages) in entries {
        map.insert(
            (*field).to_owned(),
            Value::Array(messages.iter().cloned().map(Value::String).collect()),
        );
    }
    serde_json::to_string(&map).expect("error bodies are always serializable")
}

fn base64_encode(bytes: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// `base64.b64decode(value)` without `validate=True`: non-alphabet bytes
/// are discarded before decoding; padding errors still fail.
fn base64_decode_lenient(text: &str) -> Option<Vec<u8>> {
    use base64::Engine;
    let cleaned: String = text
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '+' || *c == '/' || *c == '=')
        .collect();
    base64::engine::general_purpose::STANDARD
        .decode(cleaned.as_bytes())
        .ok()
}

/// `_record_body_write` (`page.py:156-170`): mentions/backlinks, then page
/// history. Best-effort: the response stands either way.
async fn record_body_write(
    pool: &PgPool,
    page_id: &Uuid,
    old_description_html: Option<&str>,
    new_description_html: &str,
    user_id: &Uuid,
) {
    let page_text = page_id.to_string();
    let user_text = user_id.to_string();
    for (task, kwargs) in work_tasks::record_body_write(
        &page_text,
        old_description_html,
        new_description_html,
        &user_text,
    ) {
        enqueue_best_effort(pool, task, Vec::new(), kwargs).await;
    }
}

// ---------------------------------------------------------------------------
// Live document conversion (utils/live_document.py)
// ---------------------------------------------------------------------------

/// `LiveDocument` (`live_document.py:56-60`).
struct LiveDocument {
    description_html: String,
    description_json: Value,
    description_binary: Vec<u8>,
}

/// `LiveConversionError` message carrier.
struct LiveConversionError(String);

/// `convert_document` (`live_document.py:63-115`):
/// `POST {LIVE_URL}/convert-document/`, `LIVE_URL =
/// urljoin(LIVE_BASE_URL, LIVE_BASE_PATH)` (`settings/common.py:642`).
async fn convert_document(
    live_base_url: Option<&str>,
    live_base_path: &str,
    description_html: &str,
    base_binary: Option<&[u8]>,
    title: Option<&str>,
) -> Result<LiveDocument, LiveConversionError> {
    const LIVE_CONVERSION_TIMEOUT_SECS: u64 = 15;
    const PAGE_VARIANT: &str = "document";
    let Some(base) = live_base_url.filter(|u| !u.is_empty()) else {
        return Err(LiveConversionError(LIVE_URL_MISSING_MESSAGE.to_owned()));
    };
    let live_url = urljoin(base, live_base_path);
    let mut payload = Map::with_capacity(4);
    payload.insert(
        "description_html".to_owned(),
        Value::String(if description_html.is_empty() {
            EMPTY_BODY_HTML.to_owned()
        } else {
            description_html.to_owned()
        }),
    );
    payload.insert("variant".to_owned(), Value::String(PAGE_VARIANT.to_owned()));
    if let Some(binary) = base_binary.filter(|b| !b.is_empty()) {
        payload.insert(
            "description_binary".to_owned(),
            Value::String(base64_encode(binary)),
        );
    }
    if let Some(title) = title {
        payload.insert("title".to_owned(), Value::String(title.to_owned()));
    }
    let url = normalize_url_path(&format!("{live_url}/convert-document/"));
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(LIVE_CONVERSION_TIMEOUT_SECS))
        .build()
        .map_err(|_| {
            LiveConversionError("the live server is unreachable: ClientError".to_owned())
        })?;
    let request_body = serde_json::to_string(&Value::Object(payload)).unwrap_or_default();
    let response = client
        .post(url)
        .header("Content-Type", "application/json")
        .body(request_body)
        .send()
        .await
        .map_err(|error| {
            LiveConversionError(format!(
                "the live server is unreachable: {}",
                if error.is_timeout() {
                    "Timeout"
                } else if error.is_connect() {
                    "ConnectionError"
                } else {
                    "RequestException"
                }
            ))
        })?;
    if response.status() != reqwest::StatusCode::OK {
        return Err(LiveConversionError(format!(
            "the live server rejected the conversion (HTTP {})",
            response.status().as_u16()
        )));
    }
    let body = response.text().await.map_err(|_| {
        LiveConversionError("the live server returned an unusable document".to_owned())
    })?;
    let data: Value = serde_json::from_str(&body).map_err(|_| {
        LiveConversionError("the live server returned an unusable document".to_owned())
    })?;
    let encoded = data
        .get("description_binary")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            LiveConversionError("the live server returned an unusable document".to_owned())
        })?;
    let binary = base64_decode_strict(encoded).ok_or_else(|| {
        LiveConversionError("the live server returned an unusable document".to_owned())
    })?;
    if binary.is_empty() {
        return Err(LiveConversionError(
            "the live server returned an empty document".to_owned(),
        ));
    }
    let sent_html = data
        .get("description_html")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| {
            if description_html.is_empty() {
                EMPTY_BODY_HTML.to_owned()
            } else {
                description_html.to_owned()
            }
        });
    let json = data.get("description_json").cloned().unwrap_or(Value::Null);
    let json = if json.is_null() {
        Value::Object(Map::new())
    } else {
        json
    };
    Ok(LiveDocument {
        description_html: sent_html,
        description_json: json,
        description_binary: binary,
    })
}

/// `base64.b64decode(encoded, validate=True)`: strict alphabet.
fn base64_decode_strict(text: &str) -> Option<Vec<u8>> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(text.as_bytes())
        .ok()
}

/// `urllib.parse.urljoin(base, path)` for the `LIVE_URL` composition
/// (`settings/common.py:642`): with a non-empty base, RFC 3986 merge —
/// an absolute `path` replaces the base path, a relative one merges.
/// Both settings values are absolute-or-rooted in practice; the general
/// merge below covers the rest.
fn urljoin(base: &str, path: &str) -> String {
    if path.starts_with("http://") || path.starts_with("https://") {
        return path.to_owned();
    }
    let (scheme_host, base_path) = match base.find("://").and_then(|i| base[i + 3..].find('/')) {
        Some(j) => {
            let cut = base.find("://").expect("checked") + 3 + j;
            (&base[..cut], &base[cut..])
        }
        None => (base, "/"),
    };
    if path.starts_with('/') {
        return format!("{scheme_host}{}", normalize_path(path));
    }
    let merged = match base_path.rfind('/') {
        Some(i) => format!("{}{}", &base_path[..=i], path),
        None => format!("/{path}"),
    };
    format!("{scheme_host}{}", normalize_path(&merged))
}

/// Collapse `.`/`..` segments (RFC 3986 `remove_dot_segments`, short
/// form — enough for the settings values in play).
fn normalize_path(path: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    for segment in path.split('/') {
        match segment {
            "." | "" if !out.is_empty() => {}
            ".." => {
                out.pop();
            }
            _ => out.push(segment),
        }
    }
    let joined = out.join("/");
    if joined.starts_with('/') {
        joined
    } else {
        format!("/{joined}")
    }
}

/// `normalize_url_path` (`utils/url.py:110`): collapse duplicate slashes
/// in the path component only (protocol/host/query/fragment untouched).
fn normalize_url_path(url: &str) -> String {
    let scheme_end = url.find("://").map(|i| i + 3).unwrap_or(0);
    let (head, rest) = url.split_at(scheme_end);
    let path_end = rest.find(['?', '#']).unwrap_or(rest.len());
    let (path, tail) = rest.split_at(path_end);
    let mut collapsed = String::with_capacity(path.len());
    let mut prev_slash = false;
    for c in path.chars() {
        if c == '/' {
            if prev_slash {
                continue;
            }
            prev_slash = true;
        } else {
            prev_slash = false;
        }
        collapsed.push(c);
    }
    format!("{head}{collapsed}{tail}")
}

// ---------------------------------------------------------------------------
// Page guards + writes
// ---------------------------------------------------------------------------

/// `detail_response` (`page.py:228-230`): re-read the visible row and
/// render the detail shape.
async fn detail_response(
    pool: &PgPool,
    slug: &str,
    project_id: &Uuid,
    user_id: &Uuid,
    page_id: &Uuid,
    tz: &Tz,
    status: StatusCode,
) -> Result<Response, Denial> {
    let row = fetch_visible_page(pool, slug, project_id, user_id, page_id).await?;
    let Some(row) = row else {
        return Err(Denial::ServerError);
    };
    let decoded = decode_page(&row, tz)?;
    let body = render_page_detail_value(&decoded, None, &[], None, &[])?;
    Ok(json_response(
        status,
        serde_json::to_string(&body).map_err(|_| Denial::ServerError)?,
    ))
}

/// `validate_parent` (`page.py:199-220`): the parent must be visible in
/// this project, not archived, and — on update — not the page itself or
/// one of its descendants. `page_id` is `Some` on update (cycle check)
/// and `None` on create. Returns the validated parent id (`None` =
/// absent or explicit null). Lookup outcomes reuse the Q3 port's types;
/// the ancestor walk awaits per hop (the port's closure is sync).
async fn check_parent(
    pool: &PgPool,
    slug: &str,
    project_id: &Uuid,
    user_id: &Uuid,
    parent: Option<Option<&str>>,
    page_id: Option<Uuid>,
) -> Result<Option<Uuid>, Denial> {
    use pidash_services::v1_work_items::queries_search::{
        ParentLookup, ParentRef, ValidateParentError,
    };
    // Absent or explicit null: no lookup (`:206-207`).
    let raw = match parent {
        Some(Some(raw)) => raw,
        _ => return Ok(None),
    };
    let parent_id = raw.parse::<Uuid>().map_err(|_| Denial::ServerError)?;
    let lookup = match fetch_visible_page(pool, slug, project_id, user_id, &parent_id).await? {
        None => ParentLookup::Missing,
        Some(row) => ParentLookup::Found(ParentRef {
            id: row_uuid(&row, "id")?,
            archived: row_date_opt(&row, "archived_at")?.is_some(),
        }),
    };
    let found = match lookup {
        ParentLookup::Absent => return Ok(None),
        ParentLookup::Missing => return Err(parent_denial(ValidateParentError::NotFound)),
        ParentLookup::Found(found) => found,
    };
    if found.archived {
        return Err(parent_denial(ValidateParentError::Archived));
    }
    let Some(target) = page_id else {
        return Ok(Some(parent_id));
    };
    // Cycle walk (`:214-219`): unscoped, `seen`-guarded; a NULL parent
    // or a missing row ends the walk silently (quirk 11).
    let mut ancestor = found.id;
    let mut seen = std::collections::HashSet::new();
    loop {
        if !seen.insert(ancestor) {
            return Ok(Some(parent_id));
        }
        if ancestor == target {
            return Err(parent_denial(ValidateParentError::SelfNest));
        }
        let next: Option<Uuid> = sqlx::query_scalar(
            r#"SELECT "parent_id" FROM "pages" WHERE "deleted_at" IS NULL AND "id" = $1 ORDER BY "created_at" DESC LIMIT 1"#,
        )
        .bind(ancestor)
        .fetch_optional(pool)
        .await
        .map_err(|error| db_error(error, "parent-walk"))?
        .flatten()
        .flatten();
        match next {
            Some(id) => ancestor = id,
            None => return Ok(Some(parent_id)),
        }
    }
}

fn parent_denial(
    error: pidash_services::v1_work_items::queries_search::ValidateParentError,
) -> Denial {
    // The Q3 port renders the exact `_error` body; it answers 400.
    Denial::FieldErrors(error.body())
}

// ---------------------------------------------------------------------------
// Page handlers
// ---------------------------------------------------------------------------

/// `GET .../pages/` (`views/page.py:261-274`).
pub async fn get_page_list(
    State(state): State<AppState>,
    Path((slug, project_id)): Path<(String, String)>,
    Query(query): Query<QueryMap>,
    headers: HeaderMap,
) -> Response {
    match page_list_inner(&state, &headers, &slug, &project_id, &query).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

async fn page_list_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    project_id_raw: &str,
    query: &QueryMap,
) -> Result<Response, Denial> {
    let pre = preamble(state, headers, slug).await?;
    let project_id = rewrite_project_id(&pre.pool, slug, project_id_raw).await?;
    let workspace_id = pre.workspace_id.ok_or(Denial::Forbidden)?;
    require_gate(
        &pre.pool,
        &workspace_id,
        slug,
        &pre.actor.id,
        &project_id,
        V1WorkItemsRoute::PageList,
        "GET",
    )
    .await?;
    let tz = activate_timezone(pre.actor.timezone.as_deref())?;
    let per_page =
        crate::paginator::parse_per_page(query_last(query, "per_page").as_deref(), 1000, 1000)
            .map_err(page_denial)?;
    let cursor_raw = query_last(query, "cursor").unwrap_or_else(|| format!("{per_page}:0:0"));
    let cursor = crate::paginator::Cursor::from_string(&cursor_raw).map_err(page_denial)?;
    // `TRUE_VALUES = {"true","1","yes"}` (`page.py:99`), lowercased.
    let include_archived = matches!(
        query_last(query, "include_archived")
            .unwrap_or_default()
            .to_lowercase()
            .as_str(),
        "true" | "1" | "yes"
    );
    let rows = fetch_page_rows(
        &pre.pool,
        slug,
        &project_id,
        &pre.actor.id,
        include_archived,
    )
    .await?;
    let total_count = rows.len() as i64;
    let (page_rows, has_more) = window_rows(&rows, per_page, &cursor)?;
    let (field_specs, kept, expand) = field_selection(query, PAGE_LITE_FIELDS)?;
    let expand_refs: Vec<&str> = expand.iter().map(String::as_str).collect();
    let mut rendered: Vec<Value> = Vec::with_capacity(page_rows.len());
    for row in page_rows {
        let decoded = decode_page(row, &tz)?;
        rendered.push(
            render_page_lite_expanded(
                &pre.pool,
                &decoded,
                field_specs.as_deref(),
                &expand_refs,
                &kept,
            )
            .await?,
        );
    }
    let next = crate::paginator::next_cursor(per_page, cursor.offset, has_more);
    let prev = crate::paginator::prev_cursor(per_page, cursor.offset);
    envelope(total_count, per_page, &next, &prev, Value::Array(rendered))
}

/// Render one lite row with its expansions: `owned_by` from the fetched
/// owner row (missing → 500 like Django), `parent` from the caller value.
async fn render_page_lite_expanded(
    pool: &PgPool,
    decoded: &DecodedPage,
    field_specs: Option<&[FieldSpec]>,
    expand_refs: &[&str],
    kept: &[String],
) -> Result<Value, Denial> {
    render_page_expanded(pool, decoded, field_specs, expand_refs, kept, true).await
}

/// Render one detail row with its expansions.
async fn render_page_detail_expanded(
    pool: &PgPool,
    decoded: &DecodedPage,
    field_specs: Option<&[FieldSpec]>,
    expand_refs: &[&str],
    kept: &[String],
) -> Result<Value, Denial> {
    render_page_expanded(pool, decoded, field_specs, expand_refs, kept, false).await
}

async fn render_page_expanded(
    pool: &PgPool,
    decoded: &DecodedPage,
    field_specs: Option<&[FieldSpec]>,
    expand_refs: &[&str],
    kept: &[String],
    lite: bool,
) -> Result<Value, Denial> {
    let wants_owner = expand_refs.contains(&"owned_by") && kept.iter().any(|k| k == "owned_by");
    let owner_strings = if wants_owner {
        match decoded.owned_by.as_deref() {
            Some(raw) => {
                let id = raw.parse::<Uuid>().map_err(|_| Denial::ServerError)?;
                Some(
                    fetch_owner_strings(pool, &id)
                        .await?
                        .ok_or(Denial::ServerError)?,
                )
            }
            // Corrupt NULL owner with the expand: Django raises
            // `RelatedObjectDoesNotExist` → 500.
            None => return Err(Denial::ServerError),
        }
    } else {
        None
    };
    let owner_row =
        owner_strings
            .as_ref()
            .map(|o| pidash_services::v1_projects::ser_collab::UserLiteRow {
                id: &o.id,
                first_name: &o.first_name,
                last_name: &o.last_name,
                email: o.email.as_deref(),
                avatar: &o.avatar,
                avatar_url: o.avatar_url.as_deref(),
                display_name: &o.display_name,
            });
    let expansions = page_expansions(pool, decoded, expand_refs, kept).await?;
    if lite {
        render_page_lite_value(
            decoded,
            field_specs,
            expand_refs,
            owner_row.as_ref(),
            &expansions,
        )
    } else {
        render_page_detail_value(
            decoded,
            field_specs,
            expand_refs,
            owner_row.as_ref(),
            &expansions,
        )
    }
}

/// `POST .../pages/` (`views/page.py:298-341`).
pub async fn post_page(
    State(state): State<AppState>,
    Path((slug, project_id)): Path<(String, String)>,
    headers: HeaderMap,
    body: axum::body::Body,
) -> Response {
    let raw = match read_body(body).await {
        Ok(raw) => raw,
        Err(denial) => return denial.into_response(),
    };
    match page_post_inner(&state, &headers, &slug, &project_id, &raw).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

async fn page_post_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    project_id_raw: &str,
    raw_body: &[u8],
) -> Result<Response, Denial> {
    let pre = preamble(state, headers, slug).await?;
    let project_id = rewrite_project_id(&pre.pool, slug, project_id_raw).await?;
    let workspace_id = pre.workspace_id.ok_or(Denial::Forbidden)?;
    require_gate(
        &pre.pool,
        &workspace_id,
        slug,
        &pre.actor.id,
        &project_id,
        V1WorkItemsRoute::PageList,
        "POST",
    )
    .await?;
    let tz = activate_timezone(pre.actor.timezone.as_deref())?;
    let mut parsed = parse_write_body(headers, raw_body)?;
    if parsed.from_form {
        apply_form_blanks(&mut parsed.value, &["access"], &["parent"]);
    }
    let validated = validate_page_write(&parsed.value, WriteMode::Create)
        .map_err(|errors| Denial::FieldErrors(errors.body()))?;
    // Order: archived-project check → parent → convert (`:308-323`).
    let project_archived: bool = sqlx::query_scalar(
        r#"SELECT EXISTS(SELECT 1 FROM "projects" WHERE "id" = $1 AND "archived_at" IS NOT NULL)"#,
    )
    .bind(project_id)
    .fetch_optional(&pre.pool)
    .await
    .map_err(|error| db_error(error, "page-post-archived"))?
    .unwrap_or(false);
    if project_archived {
        return Err(Denial::Conflict(PROJECT_ARCHIVED_BODY.to_owned()));
    }
    let parent_id = check_parent(
        &pre.pool,
        slug,
        &project_id,
        &pre.actor.id,
        validated.parent.as_ref().map(|p| p.as_deref()),
        None,
    )
    .await?;
    let html = if validated.has_body() {
        render_body_html(
            validated.description_markdown.as_deref(),
            validated.description_html.as_deref(),
        )
        .map_err(Denial::BadError)?
    } else {
        EMPTY_BODY_HTML.to_owned()
    };
    // Create always converts (even bodiless): without the live server
    // the 503 answers and no page is created (`:320-323`).
    let title = validated.name.clone().unwrap_or_default();
    let document = convert_document(
        state.settings().urls.live_base_url.as_deref(),
        &state.settings().urls.live_base_path,
        &html,
        None,
        Some(&title),
    )
    .await
    .map_err(|error| conversion_failed(&error.0))?;
    let fields = document_fields(&document).map_err(Denial::FieldErrors)?;
    // `PageSerializer(data={name, access, parent})` (`:328-336`): `parent`
    // re-validates against the GLOBAL pk scope — a parent deleted in the
    // race answers the DRF `does_not_exist` 400 here, before the write.
    if let Some(parent_id) = parent_id {
        let still_there: bool = sqlx::query_scalar(
            r#"SELECT EXISTS(SELECT 1 FROM "pages" WHERE "deleted_at" IS NULL AND "id" = $1)"#,
        )
        .bind(parent_id)
        .fetch_optional(&pre.pool)
        .await
        .map_err(|error| db_error(error, "page-post-parent-race"))?
        .unwrap_or(false);
        if !still_there {
            return Err(Denial::FieldErrors(field_errors_body(&[(
                "parent",
                vec![format!(
                    "Invalid pk \"{parent_id}\" - object does not exist."
                )],
            )])));
        }
    }
    // `PageSerializer.create` (`app/serializers/page.py:61-89`): the page,
    // then the `ProjectPage` link, in one transaction. No labels arrive
    // from v1, so the `PageLabel` bulk step is skipped.
    let page_id = Uuid::new_v4();
    let created_at = now_utc();
    let updated_at = now_utc();
    let stripped = strip_description(&fields.description_html);
    let link_created_at = now_utc();
    let link_updated_at = now_utc();
    let mut tx = pre
        .pool
        .begin()
        .await
        .map_err(|error| db_error(error, "page-post-tx"))?;
    let insert_page = sqlx::query(
        r#"INSERT INTO "pages" ("id", "created_at", "updated_at", "created_by_id", "updated_by_id",
            "workspace_id", "name", "description_json", "description_binary", "description_html",
            "description_stripped", "owned_by_id", "access", "color", "parent_id", "archived_at",
            "is_locked", "view_props", "logo_props", "is_global", "moved_to_page", "moved_to_project",
            "sort_order", "external_id", "external_source")
           VALUES ($1, $2, $3, $4, NULL, $5, $6, $7, $8, $9, $10, $11, $12, '', NULL, NULL,
            FALSE, '{"full_width": false}', '{}', FALSE, NULL, NULL, 65535, NULL, NULL)"#,
    )
    .bind(page_id)
    .bind(created_at)
    .bind(updated_at)
    .bind(pre.actor.id)
    .bind(workspace_id)
    .bind(validated.name.clone().unwrap_or_default())
    .bind(fields.description_json.clone())
    .bind(fields.description_binary.clone())
    .bind(fields.description_html.clone())
    .bind(stripped.clone())
    .bind(pre.actor.id)
    .bind(validated.access.unwrap_or(0) as i16)
    .bind(parent_id)
    .execute(&mut *tx)
    .await;
    if let Err(error) = insert_page {
        let _ = tx.rollback().await;
        if is_integrity_error(&error) {
            return Err(Denial::FieldErrors(PAYLOAD_NOT_VALID_BODY.to_owned()));
        }
        return Err(db_error(error, "page-post-insert"));
    }
    let insert_link = sqlx::query(
        r#"INSERT INTO "project_pages" ("id", "created_at", "updated_at", "created_by_id", "updated_by_id",
            "workspace_id", "project_id", "page_id")
           VALUES ($1, $2, $3, $4, NULL, $5, $6, $7)"#,
    )
    .bind(Uuid::new_v4())
    .bind(link_created_at)
    .bind(link_updated_at)
    .bind(pre.actor.id)
    .bind(workspace_id)
    .bind(project_id)
    .bind(page_id)
    .execute(&mut *tx)
    .await;
    if let Err(error) = insert_link {
        let _ = tx.rollback().await;
        if is_integrity_error(&error) {
            return Err(Denial::FieldErrors(PAYLOAD_NOT_VALID_BODY.to_owned()));
        }
        return Err(db_error(error, "page-post-link"));
    }
    tx.commit()
        .await
        .map_err(|error| db_error(error, "page-post-commit"))?;
    // Unconditional on create (`:340`): `old` is `None`.
    record_body_write(
        &pre.pool,
        &page_id,
        None,
        &fields.description_html,
        &pre.actor.id,
    )
    .await;
    detail_response(
        &pre.pool,
        slug,
        &project_id,
        &pre.actor.id,
        &page_id,
        &tz,
        StatusCode::CREATED,
    )
    .await
}

/// `Page.save` (`db/models/page.py:70-77`): `NULL` for empty/missing HTML,
/// else the MLStripper text — via the `db` port (NOT Django's
/// `strip_tags`, which keeps references verbatim).
fn strip_description(html: &str) -> Option<String> {
    if html.is_empty() {
        None
    } else {
        Some(pidash_db::app_pages::strip::ml_strip_tags(html))
    }
}

/// `GET .../pages/<page_id>/` (`views/page.py:365-376`).
pub async fn get_page_detail(
    State(state): State<AppState>,
    OriginalUri(original): OriginalUri,
    Path((slug, project_id, page_id)): Path<(String, String, String)>,
    Query(query): Query<QueryMap>,
    headers: HeaderMap,
) -> Response {
    if !crate::runner_runs::is_uuid_path_segment(&page_id) {
        return proxy_request(&state, "GET", original.to_string()).await;
    }
    let page_id = page_id.parse::<Uuid>().expect("checked segment");
    match page_detail_inner(&state, &headers, &slug, &project_id, &page_id, &query).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

async fn page_detail_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    project_id_raw: &str,
    page_id: &Uuid,
    query: &QueryMap,
) -> Result<Response, Denial> {
    let pre = preamble(state, headers, slug).await?;
    let project_id = rewrite_project_id(&pre.pool, slug, project_id_raw).await?;
    let workspace_id = pre.workspace_id.ok_or(Denial::Forbidden)?;
    require_gate(
        &pre.pool,
        &workspace_id,
        slug,
        &pre.actor.id,
        &project_id,
        V1WorkItemsRoute::PageDetail,
        "GET",
    )
    .await?;
    let tz = activate_timezone(pre.actor.timezone.as_deref())?;
    let row = fetch_visible_page(&pre.pool, slug, &project_id, &pre.actor.id, page_id).await?;
    let Some(row) = row else {
        // `.get()` miss → `ObjectDoesNotExist` → the 404 branch (NOT the
        // "Page not found" body — that is `get_page_or_error` only).
        return Err(Denial::NotFound(RESOURCE_NOT_FOUND_BODY.to_owned()));
    };
    let (field_specs, kept, expand) = field_selection(query, PAGE_DETAIL_FIELDS)?;
    let expand_refs: Vec<&str> = expand.iter().map(String::as_str).collect();
    let decoded = decode_page(&row, &tz)?;
    let body = render_page_detail_expanded(
        &pre.pool,
        &decoded,
        field_specs.as_deref(),
        &expand_refs,
        &kept,
    )
    .await?;
    Ok(json_response(
        StatusCode::OK,
        serde_json::to_string(&body).map_err(|_| Denial::ServerError)?,
    ))
}

/// `PATCH .../pages/<page_id>/` (`views/page.py:404-477`).
pub async fn patch_page(
    State(state): State<AppState>,
    OriginalUri(original): OriginalUri,
    Path((slug, project_id, page_id)): Path<(String, String, String)>,
    headers: HeaderMap,
    body: axum::body::Body,
) -> Response {
    if !crate::runner_runs::is_uuid_path_segment(&page_id) {
        return proxy_request(&state, "PATCH", original.to_string()).await;
    }
    let page_id = page_id.parse::<Uuid>().expect("checked segment");
    let raw = match read_body(body).await {
        Ok(raw) => raw,
        Err(denial) => return denial.into_response(),
    };
    match page_patch_inner(&state, &headers, &slug, &project_id, &page_id, &raw).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

async fn page_patch_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    project_id_raw: &str,
    page_id: &Uuid,
    raw_body: &[u8],
) -> Result<Response, Denial> {
    let pre = preamble(state, headers, slug).await?;
    let project_id = rewrite_project_id(&pre.pool, slug, project_id_raw).await?;
    let workspace_id = pre.workspace_id.ok_or(Denial::Forbidden)?;
    require_gate(
        &pre.pool,
        &workspace_id,
        slug,
        &pre.actor.id,
        &project_id,
        V1WorkItemsRoute::PageDetail,
        "PATCH",
    )
    .await?;
    let tz = activate_timezone(pre.actor.timezone.as_deref())?;
    // `get_page_or_error` (`:410-412`): the "Page not found" 404.
    let row = fetch_visible_page(&pre.pool, slug, &project_id, &pre.actor.id, page_id).await?;
    let Some(row) = row else {
        return Err(Denial::NotFound(
            pidash_services::v1_work_items::queries_search::page_not_found_body(),
        ));
    };
    let mut parsed = parse_write_body(headers, raw_body)?;
    if parsed.from_form {
        apply_form_blanks(&mut parsed.value, &["name", "access"], &["parent"]);
    }
    let validated = validate_page_write(&parsed.value, WriteMode::Update)
        .map_err(|errors| Denial::FieldErrors(errors.body()))?;
    // Pre-lock guards (`:418-427`): lock → archived-body → access → parent.
    let is_locked = row_bool(&row, "is_locked")?;
    if is_locked {
        return Err(page_error(
            "Page is locked",
            StatusCode::CONFLICT,
            Some(("PAGE_LOCKED", 4701)),
        ));
    }
    let archived = row_date_opt(&row, "archived_at")?.is_some();
    if validated.has_body() && archived {
        return Err(page_error(
            "Page is archived",
            StatusCode::CONFLICT,
            Some(("PAGE_ARCHIVED", 4702)),
        ));
    }
    let current_access = i64::from(row_i16(&row, "access")?);
    let owned_by_id = row_uuid_opt(&row, "owned_by_id")?;
    if let Some(access) = validated.access {
        if access != current_access && owned_by_id != Some(pre.actor.id) {
            return Err(Denial::ForbiddenError(PAGE_ACCESS_DENIAL_BODY.to_owned()));
        }
    }
    // `"parent" in data` — presence, not value (`:424`): explicit null
    // clears without a lookup.
    let parent_present = validated.parent.is_some();
    let parent_id = check_parent(
        &pre.pool,
        slug,
        &project_id,
        &pre.actor.id,
        if parent_present {
            validated.parent.as_ref().map(|p| p.as_deref())
        } else {
            None
        },
        parent_present.then_some(*page_id),
    )
    .await?;
    let current_name = row_string(&row, "name")?;
    let current_html = row_string_opt(&row, "description_html")?.unwrap_or_default();
    let current_binary: Option<Vec<u8>> = row
        .try_get("description_binary")
        .map_err(|_| Denial::ServerError)?;
    let renamed = validated
        .name
        .as_deref()
        .is_some_and(|name| name != current_name);
    // A rename rewrites the title inside the binary too (`:431-433`).
    let needs_document =
        validated.has_body() || (renamed && current_binary.as_ref().is_some_and(|b| !b.is_empty()));
    let document = if needs_document {
        let html = if validated.has_body() {
            render_body_html(
                validated.description_markdown.as_deref(),
                validated.description_html.as_deref(),
            )
            .map_err(Denial::BadError)?
        } else {
            current_html.clone()
        };
        let html_ref = if html.is_empty() {
            EMPTY_BODY_HTML
        } else {
            &html
        };
        let title = validated.name.clone().unwrap_or(current_name.clone());
        Some(
            convert_document(
                state.settings().urls.live_base_url.as_deref(),
                &state.settings().urls.live_base_path,
                html_ref,
                current_binary.as_deref(),
                Some(&title),
            )
            .await
            .map_err(|error| conversion_failed(&error.0))?,
        )
    } else {
        None
    };
    let fields = match document.as_ref() {
        Some(document) => Some(document_fields(document).map_err(Denial::FieldErrors)?),
        None => None,
    };
    let stripped = fields
        .as_ref()
        .map(|f| strip_description(&f.description_html));
    // Re-check the guards under a row lock and write only the changed
    // fields (`:467-473`).
    let updated_at = now_utc();
    let mut tx = pre
        .pool
        .begin()
        .await
        .map_err(|error| db_error(error, "page-patch-tx"))?;
    let locked: Option<(bool, Option<chrono::NaiveDate>)> = sqlx::query_as(
        r#"SELECT "is_locked", "archived_at" FROM "pages" WHERE "deleted_at" IS NULL AND "id" = $1 FOR UPDATE"#,
    )
    .bind(page_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|error| db_error(error, "page-patch-lock"))?;
    let Some((locked_now, archived_now)) = locked else {
        let _ = tx.rollback().await;
        return Err(Denial::NotFound(RESOURCE_NOT_FOUND_BODY.to_owned()));
    };
    if locked_now {
        let _ = tx.rollback().await;
        return Err(page_error(
            "Page is locked",
            StatusCode::CONFLICT,
            Some(("PAGE_LOCKED", 4701)),
        ));
    }
    if validated.has_body() && archived_now.is_some() {
        let _ = tx.rollback().await;
        return Err(page_error(
            "Page is archived",
            StatusCode::CONFLICT,
            Some(("PAGE_ARCHIVED", 4702)),
        ));
    }
    let new_name = validated.name.clone();
    let new_access = validated.access.map(|a| a as i16);
    let update = sqlx::query(
        r#"UPDATE "pages" SET "updated_at" = $1, "updated_by_id" = $2,
               "name" = COALESCE($3, "name"),
               "access" = COALESCE($4, "access"),
               "parent_id" = CASE WHEN $5 THEN NULL WHEN $6 IS NULL THEN "parent_id" ELSE $6 END,
               "description_html" = COALESCE($7, "description_html"),
               "description_json" = COALESCE($8, "description_json"),
               "description_binary" = COALESCE($9, "description_binary"),
               "description_stripped" = COALESCE($10, "description_stripped")
           WHERE "id" = $11"#,
    )
    .bind(updated_at)
    .bind(pre.actor.id)
    .bind(new_name)
    .bind(new_access)
    .bind(parent_present && validated.parent == Some(None))
    .bind(parent_id)
    .bind(fields.as_ref().map(|f| f.description_html.clone()))
    .bind(fields.as_ref().map(|f| f.description_json.clone()))
    .bind(fields.as_ref().map(|f| f.description_binary.clone()))
    .bind(stripped.unwrap_or(None))
    .bind(page_id)
    .execute(&mut *tx)
    .await;
    if let Err(error) = update {
        let _ = tx.rollback().await;
        if is_integrity_error(&error) {
            return Err(Denial::FieldErrors(PAYLOAD_NOT_VALID_BODY.to_owned()));
        }
        return Err(db_error(error, "page-patch-update"));
    }
    tx.commit()
        .await
        .map_err(|error| db_error(error, "page-patch-commit"))?;
    if validated.has_body() {
        let new_html = fields
            .as_ref()
            .map(|f| f.description_html.clone())
            .unwrap_or_default();
        record_body_write(
            &pre.pool,
            page_id,
            Some(&current_html),
            &new_html,
            &pre.actor.id,
        )
        .await;
    }
    detail_response(
        &pre.pool,
        slug,
        &project_id,
        &pre.actor.id,
        page_id,
        &tz,
        StatusCode::OK,
    )
    .await
}

// ---------------------------------------------------------------------------
// Page archive handlers (`PageArchiveAPIEndpoint`, views/page.py:480-573)
// ---------------------------------------------------------------------------

/// `POST .../pages/<page_id>/archive/` (`views/page.py:518-538`): archive
/// the page and its descendants (favourites dropped); archiving an
/// already-archived page is a no-op returning the detail body.
pub async fn post_page_archive(
    State(state): State<AppState>,
    OriginalUri(original): OriginalUri,
    Path((slug, project_id, page_id)): Path<(String, String, String)>,
    headers: HeaderMap,
) -> Response {
    if !crate::runner_runs::is_uuid_path_segment(&page_id) {
        return proxy_request(&state, "POST", original.to_string()).await;
    }
    let page_id = page_id.parse::<Uuid>().expect("checked segment");
    match page_archive_inner(&state, &headers, &slug, &project_id, &page_id, true).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

/// `DELETE .../pages/<page_id>/archive/` (`views/page.py:554-573`):
/// unarchive the page and its descendants (detach-if-parent-archived);
/// unarchiving a live page is a no-op returning the detail body.
pub async fn delete_page_archive(
    State(state): State<AppState>,
    OriginalUri(original): OriginalUri,
    Path((slug, project_id, page_id)): Path<(String, String, String)>,
    headers: HeaderMap,
) -> Response {
    if !crate::runner_runs::is_uuid_path_segment(&page_id) {
        return proxy_request(&state, "DELETE", original.to_string()).await;
    }
    let page_id = page_id.parse::<Uuid>().expect("checked segment");
    match page_archive_inner(&state, &headers, &slug, &project_id, &page_id, false).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

async fn page_archive_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    project_id_raw: &str,
    page_id: &Uuid,
    is_archive: bool,
) -> Result<Response, Denial> {
    let pre = preamble(state, headers, slug).await?;
    let project_id = rewrite_project_id(&pre.pool, slug, project_id_raw).await?;
    let workspace_id = pre.workspace_id.ok_or(Denial::Forbidden)?;
    require_gate(
        &pre.pool,
        &workspace_id,
        slug,
        &pre.actor.id,
        &project_id,
        V1WorkItemsRoute::PageArchive,
        if is_archive { "POST" } else { "DELETE" },
    )
    .await?;
    let tz = activate_timezone(pre.actor.timezone.as_deref())?;
    // `get_page_or_error` (`:222-226`): the visibility queryset plus pk;
    // a miss answers `{"error":"Page not found"}` (NOT the `.get()`
    // `handle_exception` branch the GET-detail path uses).
    let row = fetch_visible_page(&pre.pool, slug, &project_id, &pre.actor.id, page_id).await?;
    let Some(row) = row else {
        return Err(Denial::NotFound(
            pidash_services::v1_work_items::queries_search::page_not_found_body(),
        ));
    };
    // `_check_can_archive` (`:491-502`): lock first (409 even for the
    // owner), then owner-or-project-admin (403 otherwise). The admin
    // probe is the Q3 port's `archive_admin_exists_sql` in executable
    // `$n` form (no workspace scope, unlike the delete guard).
    let is_locked = row_bool(&row, "is_locked")?;
    let owned_by = row_uuid(&row, "owned_by_id")?;
    let is_admin: bool = sqlx::query_scalar(
        r#"SELECT EXISTS(SELECT 1 FROM "project_members" WHERE "deleted_at" IS NULL AND "project_id" = $1 AND "member_id" = $2 AND "is_active" AND "role" = 20)"#,
    )
    .bind(project_id)
    .bind(pre.actor.id)
    .fetch_optional(&pre.pool)
    .await
    .map_err(|error| db_error(error, "archive-admin"))?
    .unwrap_or(false);
    match check_can_archive(is_locked, owned_by, pre.actor.id, is_admin) {
        ArchiveDecision::Allow => {}
        ArchiveDecision::Locked => {
            return Err(Denial::Conflict(super::perms::PAGE_LOCKED_BODY.to_owned()));
        }
        ArchiveDecision::Forbidden => {
            return Err(Denial::ForbiddenError(
                super::perms::PAGE_OWNER_DENIAL_BODY.to_owned(),
            ));
        }
    }
    let archived = row_date_opt(&row, "archived_at")?.is_some();
    if is_archive {
        if !archived {
            // Queryset `.delete()` is `.update(deleted_at=now())`
            // (`db/mixins.py:48-51`) — no `updated_at` stamp, no sweep
            // task (the D-30 merged precedent,
            // `app_pages/archive_page`).
            sqlx::query(
                r#"UPDATE user_favorites SET deleted_at = now()
                   WHERE entity_type = 'page' AND entity_identifier = $1 AND project_id = $2
                   AND workspace_id = (SELECT id FROM workspaces WHERE slug = $3)
                   AND deleted_at IS NULL"#,
            )
            .bind(page_id)
            .bind(project_id)
            .bind(slug)
            .execute(&pre.pool)
            .await
            .map_err(|error| db_error(error, "archive-favorites"))?;
            // `unarchive_archive_page_and_descendants(page_id,
            // timezone.now().date())` (`:537`): a DATE, not a datetime
            // (the first-party `datetime.now()` differs — ported as the
            // v1 call site reads). The CTE text is the merged D-30
            // `STATE_CTE_SQL`.
            let today = chrono::Utc::now().date_naive();
            sqlx::query(crate::app_pages::STATE_CTE_SQL)
                .bind(page_id)
                .bind(today)
                .execute(&pre.pool)
                .await
                .map_err(|error| db_error(error, "archive-cte"))?;
        }
    } else if archived {
        // Same as the first-party view (`:566-571`): an unarchived child
        // of a still-archived parent would be unreachable, so it moves
        // to the top. `page.parent` is an unscoped FK fetch (`_base_manager`):
        // a soft-deleted parent is FOUND and only its `archived_at` decides
        // the detach; a hard-missing `parent_id` raises `DoesNotExist` →
        // the v1 `handle_exception` 404 branch (no `deleted_at` scope here).
        if let Some(parent_id) = row_uuid_opt(&row, "parent_id")? {
            let parent: Option<(Option<chrono::NaiveDate>,)> =
                sqlx::query_as(r#"SELECT p.archived_at FROM pages p WHERE p.id = $1"#)
                    .bind(parent_id)
                    .fetch_optional(&pre.pool)
                    .await
                    .map_err(|error| db_error(error, "unarchive-parent"))?;
            let Some((parent_archived,)) = parent else {
                return Err(Denial::NotFound(RESOURCE_NOT_FOUND_BODY.to_owned()));
            };
            if parent_archived.is_some() {
                // `page.save(update_fields=["parent"])`: the auto_now
                // `updated_at` and the crum `updated_by` still stamp
                // (the D-30 merged precedent ports the same line).
                sqlx::query(
                    r#"UPDATE pages SET parent_id = NULL, updated_at = now(), updated_by_id = $1
                       WHERE id = $2"#,
                )
                .bind(pre.actor.id)
                .bind(page_id)
                .execute(&pre.pool)
                .await
                .map_err(|error| db_error(error, "unarchive-detach"))?;
            }
        }
        sqlx::query(crate::app_pages::STATE_CTE_SQL)
            .bind(page_id)
            .bind(None::<chrono::NaiveDate>)
            .execute(&pre.pool)
            .await
            .map_err(|error| db_error(error, "unarchive-cte"))?;
    }
    detail_response(
        &pre.pool,
        slug,
        &project_id,
        &pre.actor.id,
        page_id,
        &tz,
        StatusCode::OK,
    )
    .await
}

// ---------------------------------------------------------------------------
// Body converters (`utils/markdown_converter.py`)
// ---------------------------------------------------------------------------
//
// `html_to_markdown` ports `TiptapMarkdownConverter`
// (`markdown_converter.py:87-172`) over markdownify 1.1.0
// (`MarkdownConverter`, `markdownify/__init__.py`), and `markdown_to_html`
// ports `TiptapHTMLRenderer` (`:275-431`) over the markdown-it-py 4.2.0 token
// model (`commonmark` preset with `html=False`, `table` + `strikethrough`
// enabled, `mdit-py-plugins` 0.6.1 `tasklists`). Options both subclasses pin
// are hardcoded with a line reference instead of plumbed through: ATX
// headings, `-` bullets, all three escapes off, `code_language_callback`,
// `strip_document="strip"`, no wrap, no header inference.
//
// Both converters are pinned byte-for-byte by the vectors in
// `rust-api/fixtures/v1_work_items/converters/` (generated from the live
// Python by `/tmp/gen_vectors.py`; see the `converter_tests` module below).
// Deliberate parser edges (all outside the Tiptap corpus — the editor and
// the sanitizer never emit these shapes, and the sanitizer normalizes
// stored HTML through the same html5ever parser this port uses):
//
// * `<pre>` (and `<listing>`, `<textarea>`) drops a leading newline per the
//   HTML spec; html.parser keeps it. Stored bodies always open `<pre>` with
//   `<code>`, never a newline.
// * Raw `\r` is normalized to `\n` at parse time; html.parser keeps it, so
//   a raw carriage return inside `<pre>` (the only place it survives the
//   whitespace collapser) renders differently.
// * Orphan table parts (`<td>`, `<tr>`, `<tbody>` outside a table) are
//   dropped with foster parenting; html.parser keeps them. Stored tables
//   always serialize with an explicit `<tbody>`, which also keeps the
//   auto-`<tbody>` insertion behavior-neutral (verified in `convert_tr`).
// * Stray text directly under `<table>` is foster-parented before it;
//   html.parser keeps it inside (so it joins the first row's output).
// * A `<caption>`/`<colgroup>` directly followed by `<tr>` without an
//   explicit `<tbody>` changes `is_first_row`; stored tables always carry
//   the `<tbody>` the sanitizer serializes.
// * Adoption-agency cases (`<a>` inside `<a>`, misnesting beyond what the
//   battery pins) and duplicate attributes (first wins here, last wins in
//   html.parser) follow the spec parser.
// * `<![CDATA[..]]>` in HTML content parses as a comment (ignored); bs4
//   sees text. `<!--?...-->` reads as a `<?...?>` processing instruction
//   (kept as text); the two are indistinguishable after parsing.
// * Entities inside `<title>`/`<textarea>` (RCDATA) decode here but stay
//   raw in bs4's CDATA mode.
// * Tag depth past [`MAX_TAG_DEPTH`] falls back to the stripped text,
//   mirroring CPython's `RecursionError` arm; the exact depth differs under
//   Django's deeper call stack.

/// Internal failure of the markdownify port: fall back to the stripped
/// text (the `except Exception` arm of `html_to_markdown`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct HtmlConvertFail;

/// Tag-depth ceiling, counting element levels below the fragment root (the
/// auto-inserted `<html>` wrapper is depth 1). CPython with the default
/// limit of 1000 converts 494-deep uniform nesting and raises
/// `RecursionError` at 495 (two frames per level); the walk is iterative so
/// this limit only replicates the fallback, never guards the stack. The
/// threshold shifts under Django's deeper call stack; pathological either
/// way (pinned by the `deep-490`/`deep-500` vectors for direct calls).
const MAX_TAG_DEPTH: usize = 495;

/// Safety cap on the total `colspan` repetition of one row. Python happily
/// formats gigabytes for `<td colspan="999999999">`; `str::repeat` past the
/// allocator's reach panics (caught below) or aborts the worker on OOM,
/// where CPython raises `MemoryError` into the same fallback. Past one
/// million the port takes the fallback directly.
const MAX_COLSPAN_TOTAL: u64 = 1_000_000;

/// `html_to_markdown` (`markdown_converter.py:174-189`): render stored
/// Tiptap HTML as markdown. Never fails: empty input yields `""`, and any
/// conversion failure (including a caught panic, mirroring `except
/// Exception`) degrades to the tag-stripped text via the D-30 `MLStripper`
/// port (infallible, so the second `except` arm is unreachable here).
fn html_to_markdown(html: Option<&str>) -> String {
    let html = match html {
        Some(text) if !text.is_empty() => text,
        _ => return String::new(),
    };
    let converted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        convert_html_to_markdown(html)
    }));
    match converted {
        Ok(Ok(text)) => substitute_placeholders(text.trim()).into_owned(),
        _ => substitute_placeholders(
            pidash_db::app_pages::strip::ml_strip_tags(&escape_ph_openers(html)).trim(),
        )
        .into_owned(),
    }
}

/// `process_tag`'s `parent_tags`: only these four memberships are ever read
/// by any stock or Tiptap `convert_*`, so the whole set collapses to four
/// flags (`markdownify/__init__.py:247-256` sets them; every read is one of
/// `'li'`, `'pre'`, `'_inline'`, `'_noformat'`).
#[derive(Debug, Clone, Copy, Default)]
struct ParentTags {
    li: bool,
    pre: bool,
    inline: bool,
    noformat: bool,
}

// ---------------------------------------------------------------------------
// Entity + rawtext pre-scan: BeautifulSoup(`html.parser`) vs html5ever
// ---------------------------------------------------------------------------
//
// markdownify parses with bs4 over CPython's `html.parser`, whose entity
// rules differ from the HTML5 spec html5ever implements, and whose set of
// rawtext elements is just `script`/`style`. The pre-scan rewrites the
// source so both parsers land on the same tree and text:
//
// * Text `&name;` unknown to the entity table loses its `;` in bs4
//   (`handle_entityref` re-emits `&name`); html5ever keeps it. The scan
//   drops the `;` up front for oracle-confirmed misses.
// * Text `&#…` the `charref` regex rejects (notably `&#[0-9]+[a-fA-F]`,
//   where the hex letter blocks the decimal match) stays literal in bs4
//   — and when no `;` follows anywhere, the *rest of the input* becomes
//   data (`goahead` breaks unconsumed; `close()` flushes). html5ever
//   would resolve the numeric prefix. The scan protects the `&` and, on
//   the death trigger, escapes every later `&`/`<`.
// * `&#0;` is NUL in bs4 but U+FFFD in html5ever; surrogates are kept in
//   bs4 but U+FFFD in html5ever (unrepresentable in a Rust `String`, so
//   the port keeps html5ever's U+FFFD — recorded in the PR).
// * Attribute values go through `html.unescape` (legacy names resolve
//   even before alphanumerics/`=`, controls and noncharacters drop),
//   where html5ever attributes follow the spec. Divergent sequences are
//   rewritten to placeholders.
// * `noscript` is ordinary to `html.parser` (bs4 leaves scripting off)
//   but rawtext in html5ever; renaming it to inert `xnoscript` recovers
//   the bs4 tree through the stock parser. `script`/`style`/`xmp`/
//   `iframe`/`noembed`/`noframes` are CDATA in both (skipped verbatim),
//   `title`/`textarea` RCDATA in both (tags are data, entities apply),
//   `plaintext` swallows to EOF in both.
// * `<pre>`/`<listing>`/`<textarea>` swallow one leading `\n` in
//   html5ever but keep it in `html.parser`; doubling the newline
//   compensates (LF only — `\r\n` normalizes lossily before the strip,
//   recorded in the PR).
//
// Placeholders (`\u{E000}{HEX}\u{E001}` per codepoint) carry resolutions
// past the parser; [`substitute_placeholders`] restores them in extracted
// attribute values and in the final string. Pre-existing openers are
// escaped by the same scan so they round-trip byte-exactly.

/// Placeholder opener/closer (private-use, never emitted by parsing).
const PH_OPEN: char = '\u{E000}';
const PH_CLOSE: char = '\u{E001}';
const PH_OPEN_LEN: usize = 3;

/// Restore `\u{E000}{HEX}\u{E001}` placeholders. Malformed shapes pass
/// through untouched (defensive only).
fn substitute_placeholders(text: &str) -> Cow<'_, str> {
    if !text.contains(PH_OPEN) {
        return Cow::Borrowed(text);
    }
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < bytes.len() {
        if text[i..].starts_with(PH_OPEN) {
            let mut j = i + PH_OPEN_LEN;
            while j < bytes.len() && bytes[j].is_ascii_hexdigit() {
                j += 1;
            }
            if j > i + PH_OPEN_LEN && text[j..].starts_with(PH_CLOSE) {
                if let Some(ch) = u32::from_str_radix(&text[i + PH_OPEN_LEN..j], 16)
                    .ok()
                    .and_then(char::from_u32)
                {
                    out.push(ch);
                    i = j + PH_CLOSE.len_utf8();
                    continue;
                }
            }
        }
        if bytes[i].is_ascii() {
            out.push(bytes[i] as char);
            i += 1;
        } else {
            let ch = text[i..].chars().next().unwrap_or('\u{FFFD}');
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    Cow::Owned(out)
}

/// Escape pre-existing placeholder openers so post-substitution restores
/// them byte-exactly.
fn escape_ph_openers(text: &str) -> Cow<'_, str> {
    if !text.contains(PH_OPEN) {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        if ch == PH_OPEN {
            push_placeholder(&mut out, PH_OPEN);
        } else {
            out.push(ch);
        }
    }
    Cow::Owned(out)
}

fn push_placeholder(out: &mut String, ch: char) {
    out.push(PH_OPEN);
    out.push_str(&format!("{:X}", ch as u32));
    out.push(PH_CLOSE);
}

/// html5ever as its own oracle: probe fragments distinguish table hits
/// from misses without embedding the 2500-name entity table. Memoized
/// per conversion (candidates are rare; names repeat).
struct EntityOracle {
    memo_full: std::collections::HashMap<String, Option<String>>,
    memo_legacy: std::collections::HashMap<String, Option<String>>,
}

impl EntityOracle {
    fn probe_paragraph(inner: &str) -> String {
        let html = Html::parse_fragment(&format!("<p>{inner}</p>"));
        let selector = scraper::Selector::parse("p").expect("static selector");
        html.select(&selector)
            .next()
            .map(|p| p.text().collect())
            .unwrap_or_default()
    }

    /// Full `&name;` resolution, `None` for a miss. A miss is either the
    /// fully literal input or a legacy-prefix resolution with the `;`
    /// left over (no multi-char WHATWG resolution ends with `;`,
    /// verified against CPython's table, so the test is exact).
    fn full(&mut self, name: &str) -> Option<String> {
        if let Some(hit) = self.memo_full.get(name) {
            return hit.clone();
        }
        let literal = format!("&{name};");
        let text = Self::probe_paragraph(&literal);
        let miss = text == literal || (text.ends_with(';') && text.chars().count() > 1);
        let result = if miss { None } else { Some(text) };
        self.memo_full.insert(name.to_owned(), result.clone());
        result
    }

    /// Legacy (semicolon-less) match: `&P` resolves on its own exactly
    /// when appending a guard character keeps the same resolution.
    fn legacy(&mut self, prefix: &str) -> Option<String> {
        if let Some(hit) = self.memo_legacy.get(prefix) {
            return hit.clone();
        }
        let resolved = Self::probe_paragraph(&format!("&{prefix};"));
        let guarded = Self::probe_paragraph(&format!("&{prefix}\u{2225}"));
        let result = if guarded == format!("{resolved}\u{2225}") {
            Some(resolved)
        } else {
            None
        };
        self.memo_legacy.insert(prefix.to_owned(), result.clone());
        result
    }
}

/// Longest `_html5`-matchable prefix, CPython `_replace_charref` order
/// (full length down to 2). Returns the resolution and prefix length.
fn backtrack_legacy(oracle: &mut EntityOracle, name: &str) -> Option<(String, usize)> {
    // (`name` is ASCII alphanumerics; byte indexing is safe.)
    for len in (2..=name.len()).rev() {
        if let Some(resolved) = oracle.legacy(&name[..len]) {
            return Some((resolved, len));
        }
    }
    None
}

/// `html._invalid_codepoints`: dropped by `unescape` (attribute values).
fn is_dropped_numeric(value: u64) -> bool {
    matches!(value, 1..=8 | 11 | 14..=31 | 127)
        || (0xFDD0..=0xFDEF).contains(&value)
        || (value <= 0x10FFFF && matches!(value & 0xFFFF, 0xFFFE | 0xFFFF))
}

/// Text `charref`: `&#` + digits/hex + a *required* trailing non-hex
/// char (at end of input the regex fails). Returns (value, span end —
/// past the `;` when present, else before the trailing char).
fn match_charref(input: &[u8], i: usize) -> Option<(u64, usize)> {
    let mut j = i + 2;
    let hex = matches!(input.get(j), Some(b'x' | b'X'));
    if hex {
        j += 1;
    }
    let digits = j;
    while j < input.len()
        && (if hex {
            input[j].is_ascii_hexdigit()
        } else {
            input[j].is_ascii_digit()
        })
    {
        j += 1;
    }
    if j == digits || j >= input.len() || input[j].is_ascii_hexdigit() {
        return None;
    }
    let text = std::str::from_utf8(&input[digits..j]).ok()?;
    let value = u64::from_str_radix(text, if hex { 16 } else { 10 }).unwrap_or(u64::MAX);
    let end = if input[j] == b';' { j + 1 } else { j };
    Some((value, end))
}

/// Text `entityref`: `&` + alpha + name chars + a *required* trailing
/// non-alnum. Returns the name span.
fn match_entityref(input: &[u8], i: usize) -> Option<(usize, usize)> {
    let mut j = i + 1;
    if j >= input.len() || !input[j].is_ascii_alphabetic() {
        return None;
    }
    j += 1;
    while j < input.len() && (input[j].is_ascii_alphanumeric() || matches!(input[j], b'-' | b'.')) {
        j += 1;
    }
    if j >= input.len() || input[j].is_ascii_alphanumeric() {
        return None;
    }
    Some((i + 1, j))
}

/// Attribute numeric (`unescape`, no trailing requirement). Returns
/// (value, span end past the optional `;`).
fn match_attr_charref(input: &[u8], i: usize) -> Option<(u64, usize)> {
    let mut j = i + 2;
    let hex = matches!(input.get(j), Some(b'x' | b'X'));
    if hex {
        j += 1;
    }
    let digits = j;
    while j < input.len()
        && (if hex {
            input[j].is_ascii_hexdigit()
        } else {
            input[j].is_ascii_digit()
        })
    {
        j += 1;
    }
    if j == digits {
        return None;
    }
    let text = std::str::from_utf8(&input[digits..j]).ok()?;
    let value = u64::from_str_radix(text, if hex { 16 } else { 10 }).unwrap_or(u64::MAX);
    if input.get(j) == Some(&b';') {
        j += 1;
    }
    Some((value, j))
}

/// Attribute name run: `[A-Za-z][A-Za-z0-9]*` (capped like `unescape`'s
/// 32-char group) plus `;` presence. Non-alnum name chars (`-`, `.`, …)
/// can never start a backtrack hit, so the maximal alnum run decides
/// exactly like the full group.
fn scan_attr_name(input: &[u8], i: usize) -> Option<(usize, usize, bool)> {
    let mut j = i + 1;
    if j >= input.len() || !input[j].is_ascii_alphabetic() {
        return None;
    }
    j += 1;
    while j < input.len() && input[j].is_ascii_alphanumeric() && j - i < 33 {
        j += 1;
    }
    let semi = input.get(j) == Some(&b';');
    Some((i + 1, j, semi))
}

/// Rewrite one attribute value to `html.unescape` semantics, touching
/// only what html5ever attributes resolve differently: legacy names
/// without `;` (placeholders), full-miss backtracks with `;`
/// (placeholders), and dropped numerics (deleted). Everything else
/// copies verbatim — full hits, unknowns, and agreeing numerics —
/// because both sides resolve those identically.
fn process_attr_value<'v>(value: &'v str, oracle: &mut EntityOracle) -> Cow<'v, str> {
    if !value.contains('&') && !value.contains(PH_OPEN) {
        return Cow::Borrowed(value);
    }
    let bytes = value.as_bytes();
    let mut out = String::with_capacity(value.len());
    let mut i = 0;
    let mut dirty = false;
    while i < bytes.len() {
        if value[i..].starts_with(PH_OPEN) {
            push_placeholder(&mut out, PH_OPEN);
            i += PH_OPEN_LEN;
            dirty = true;
            continue;
        }
        if bytes[i] != b'&' {
            if bytes[i].is_ascii() {
                out.push(bytes[i] as char);
                i += 1;
            } else {
                let ch = value[i..].chars().next().unwrap_or('\u{FFFD}');
                out.push(ch);
                i += ch.len_utf8();
            }
            continue;
        }
        if bytes.get(i + 1) == Some(&b'#') {
            if let Some((num, end)) = match_attr_charref(bytes, i) {
                if num == PH_OPEN as u64 {
                    if let Some(ch) = char::from_u32(num as u32) {
                        push_placeholder(&mut out, ch);
                    }
                    dirty = true;
                } else if is_dropped_numeric(num) {
                    dirty = true; // dropped: emit nothing
                } else {
                    out.push_str(&value[i..end]);
                }
                i = end;
                continue;
            }
            out.push('&');
            i += 1;
            continue;
        }
        if let Some((ns, ne, semi)) = scan_attr_name(bytes, i) {
            let name = &value[ns..ne];
            let mut done = false;
            if semi {
                if oracle.full(name).is_none() {
                    if let Some((resolved, len)) = backtrack_legacy(oracle, name) {
                        for ch in resolved.chars() {
                            push_placeholder(&mut out, ch);
                        }
                        out.push_str(&name[len..]);
                        out.push(';');
                        done = true;
                    }
                }
            } else if let Some((resolved, len)) = backtrack_legacy(oracle, name) {
                for ch in resolved.chars() {
                    push_placeholder(&mut out, ch);
                }
                out.push_str(&name[len..]);
                done = true;
            }
            if done {
                dirty = true;
                i = ne + usize::from(semi);
            } else {
                out.push('&');
                i += 1;
            }
            continue;
        }
        out.push('&');
        i += 1;
    }
    if dirty {
        Cow::Owned(out)
    } else {
        Cow::Borrowed(value)
    }
}

/// Elements `html.parser` treats as ordinary but html5ever parses as
/// rawtext with scripting enabled: just `noscript` (bs4 leaves scripting
/// off). Renaming it to an inert `x…` name recovers the bs4 tree through
/// the stock parser.
const RENAMED_ELEMENTS: &[&str] = &["noscript"];

fn is_renamed(name: &str) -> bool {
    RENAMED_ELEMENTS
        .iter()
        .any(|n| name.eq_ignore_ascii_case(n))
}

/// `html.parser.CDATA_CONTENT_ELEMENTS`: content is raw text in both
/// parsers (no tags, no entities, no death rule); skipped verbatim.
fn is_rawtext(name: &str) -> bool {
    name.eq_ignore_ascii_case("script")
        || name.eq_ignore_ascii_case("style")
        || name.eq_ignore_ascii_case("xmp")
        || name.eq_ignore_ascii_case("iframe")
        || name.eq_ignore_ascii_case("noembed")
        || name.eq_ignore_ascii_case("noframes")
}

/// `html.parser.RCDATA_CONTENT_ELEMENTS`: content is text in both
/// parsers but entities (and the `&#` death rule) still apply; tags
/// inside are data.
fn is_rcdata(name: &str) -> bool {
    name.eq_ignore_ascii_case("title") || name.eq_ignore_ascii_case("textarea")
}

fn is_plaintext(name: &str) -> bool {
    name.eq_ignore_ascii_case("plaintext")
}

fn is_pre_like(name: &str) -> bool {
    name.eq_ignore_ascii_case("pre")
        || name.eq_ignore_ascii_case("listing")
        || name.eq_ignore_ascii_case("textarea")
}

fn is_tag_name_char(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b':' | b'.')
}

fn find_byte(hay: &[u8], from: usize, needle: u8) -> Option<usize> {
    hay.get(from..)?
        .iter()
        .position(|&b| b == needle)
        .map(|p| p + from)
}

fn find_bytes(hay: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || from >= hay.len() {
        return None;
    }
    (from..=hay.len().saturating_sub(needle.len())).find(|&p| &hay[p..p + needle.len()] == needle)
}

/// End of a start tag (index past `>`), respecting quoted `>`s.
/// `None` when the tag never closes.
fn tag_end_quoted(input: &[u8], mut j: usize) -> Option<usize> {
    while j < input.len() {
        match input[j] {
            b'\'' | b'"' => {
                let quote = input[j];
                j += 1;
                while j < input.len() && input[j] != quote {
                    j += 1;
                }
                if j >= input.len() {
                    return None;
                }
                j += 1;
            }
            b'>' => return Some(j + 1),
            _ => j += 1,
        }
    }
    None
}

/// Whether the tag ends `/>` (`/` immediately before `>`, matching
/// both parsers: `html.parser` checks `endswith('/>')`, html5ever takes
/// `/` only directly before `>`).
fn is_self_closed(input: &[u8], _from: usize, tag_end: usize) -> bool {
    tag_end >= 2 && input.get(tag_end - 1) == Some(&b'>') && input.get(tag_end - 2) == Some(&b'/')
}

fn tag_name_eq(input: &[u8], at: usize, target: &[u8]) -> bool {
    input.len() >= at + target.len() && input[at..at + target.len()].eq_ignore_ascii_case(target)
}

/// Whether the (lowercased) input mentions anything the tag-walker must
/// see. The entity scans run regardless once `&` is present.
fn needs_tag_walk(lower: &str) -> bool {
    RENAMED_ELEMENTS.iter().any(|name| lower.contains(name))
        || lower.contains("<pre")
        || lower.contains("<listing")
        || lower.contains("<textarea")
}

/// Normalize one HTML source for the html5ever parse. Returns the input
/// untouched when no trigger fires (no `&`, no renamed or
/// rawtext-adjacent tags).
fn normalize_html_source(html: &str) -> Cow<'_, str> {
    if !html.contains('&') {
        let lower = html.to_ascii_lowercase();
        if !needs_tag_walk(&lower) {
            return Cow::Borrowed(html);
        }
    }
    let mut walker = SourceWalker {
        input: html.as_bytes(),
        text: html,
        pos: 0,
        out: String::with_capacity(html.len()),
        oracle: EntityOracle {
            memo_full: std::collections::HashMap::new(),
            memo_legacy: std::collections::HashMap::new(),
        },
        last_semi: html.rfind(';'),
    };
    walker.run();
    Cow::Owned(walker.out)
}

struct SourceWalker<'t> {
    input: &'t [u8],
    text: &'t str,
    pos: usize,
    out: String,
    oracle: EntityOracle,
    last_semi: Option<usize>,
}

impl SourceWalker<'_> {
    fn run(&mut self) {
        while self.pos < self.input.len() {
            let text = self.text;
            if text[self.pos..].starts_with(PH_OPEN) {
                push_placeholder(&mut self.out, PH_OPEN);
                self.pos += PH_OPEN_LEN;
                continue;
            }
            match self.input[self.pos] {
                b'<' => self.walk_markup(),
                b'&' => self.walk_text_entity(),
                byte if byte.is_ascii() => {
                    self.out.push(byte as char);
                    self.pos += 1;
                }
                _ => {
                    let ch = text[self.pos..].chars().next().unwrap_or('\u{FFFD}');
                    self.out.push(ch);
                    self.pos += ch.len_utf8();
                }
            }
        }
    }

    /// Copy one markup construct with entity rewrites inside attribute
    /// values, rawtext skipping past `script`/`style`, and renames +
    /// pre-newline compensation.
    fn walk_markup(&mut self) {
        let input = self.input;
        let text = self.text;
        let i = self.pos;
        // Comments, declarations, PIs: verbatim (no entities inside).
        if input[i..].starts_with(b"<!--") {
            let end = find_bytes(input, i + 4, b"-->").map_or(input.len(), |p| p + 3);
            self.out.push_str(&text[i..end]);
            self.pos = end;
            return;
        }
        if input[i..].starts_with(b"<?") || input[i..].starts_with(b"<!") {
            let end = find_byte(input, i + 2, b'>').map_or(input.len(), |p| p + 1);
            self.out.push_str(&text[i..end]);
            self.pos = end;
            return;
        }
        if input.get(i + 1) == Some(&b'/') {
            self.walk_end_tag();
            return;
        }
        // A start tag needs `<` + ASCII letter, else literal `<`.
        if !(i + 1 < input.len() && input[i + 1].is_ascii_alphabetic()) {
            self.out.push('<');
            self.pos += 1;
            return;
        }
        self.walk_start_tag();
    }

    /// Copy one end tag through its first `>` (matching `html.parser`,
    /// which — unlike html5ever — does not respect quotes here), with
    /// the rename applied to the name.
    fn walk_end_tag(&mut self) {
        let input = self.input;
        let text = self.text;
        let i = self.pos;
        let mut j = i + 2;
        while j < input.len() && input[j].is_ascii_whitespace() {
            j += 1;
        }
        let ns = j;
        while j < input.len() && is_tag_name_char(input[j]) {
            j += 1;
        }
        let tag_end = find_byte(input, j, b'>').map_or(input.len(), |p| p + 1);
        if is_renamed(&text[ns..j]) {
            self.out.push_str(&text[i..ns]);
            self.out.push('x');
            self.out.push_str(&text[ns..tag_end]);
        } else {
            self.out.push_str(&text[i..tag_end]);
        }
        self.pos = tag_end;
    }

    /// Copy one start tag with processed attribute values, then skip any
    /// rawtext body and compensate the pre-newline strip.
    fn walk_start_tag(&mut self) {
        let input = self.input;
        let text = self.text;
        let i = self.pos;
        let mut j = i + 1;
        while j < input.len() && is_tag_name_char(input[j]) {
            j += 1;
        }
        let name = &text[i + 1..j];
        let Some(tag_end) = tag_end_quoted(input, j) else {
            // Unterminated: both parsers diverge here anyway (recorded);
            // copy verbatim.
            self.out.push_str(&text[i..]);
            self.pos = input.len();
            return;
        };
        let renamed = is_renamed(name);
        let self_closed = is_self_closed(input, j, tag_end);
        let rawtext = !renamed && is_rawtext(name) && !self_closed;
        let rcdata = !renamed && is_rcdata(name) && !self_closed;
        let plaintext = !renamed && is_plaintext(name) && !self_closed;
        let pre_like = !renamed && is_pre_like(name) && !self_closed;
        self.out.push('<');
        if renamed {
            self.out.push('x');
        }
        self.out.push_str(name);
        self.copy_tag_rest(j, tag_end);
        self.pos = tag_end;
        if rawtext {
            self.skip_rawtext(name);
        } else if rcdata {
            self.skip_rcdata(name);
        } else if plaintext {
            // `plaintext` swallows the rest of the input in both parsers.
            self.out.push_str(&text[self.pos..]);
            self.pos = input.len();
            return;
        }
        // `<pre>`/`<listing>`/`<textarea>` keep one leading `\n` in
        // `html.parser` that html5ever strips; doubling it compensates
        // (LF only).
        if pre_like && self.input.get(self.pos) == Some(&b'\n') {
            self.out.push('\n');
        }
    }

    /// Copy a start tag's attribute span, rewriting values in place.
    /// Everything but the values copies byte-exactly (spacing, quotes,
    /// name case).
    fn copy_tag_rest(&mut self, mut j: usize, tag_end: usize) {
        let input = self.input;
        let text = self.text;
        while j < tag_end {
            if input[j].is_ascii_whitespace() || input[j] == b'/' || input[j] == b'>' {
                self.out.push(input[j] as char);
                j += 1;
                continue;
            }
            // Attribute name.
            let ns = j;
            while j < tag_end
                && !input[j].is_ascii_whitespace()
                && !matches!(input[j], b'=' | b'/' | b'>')
            {
                j += 1;
            }
            self.out.push_str(&text[ns..j]);
            let mut k = j;
            while k < tag_end && input[k].is_ascii_whitespace() {
                k += 1;
            }
            if input.get(k) != Some(&b'=') {
                j = k;
                continue;
            }
            self.out.push_str(&text[j..=k]);
            j = k + 1;
            while j < tag_end && input[j].is_ascii_whitespace() {
                self.out.push(input[j] as char);
                j += 1;
            }
            if j < tag_end && matches!(input[j], b'\'' | b'"') {
                let quote = input[j];
                self.out.push(quote as char);
                j += 1;
                let vs = j;
                while j < tag_end && input[j] != quote {
                    j += 1;
                }
                let processed = process_attr_value(&text[vs..j], &mut self.oracle);
                self.out.push_str(&processed);
                if j < tag_end {
                    self.out.push(quote as char);
                    j += 1;
                }
                continue;
            }
            // Unquoted value: to whitespace or `>`.
            let vs = j;
            while j < tag_end && !input[j].is_ascii_whitespace() && input[j] != b'>' {
                j += 1;
            }
            let processed = process_attr_value(&text[vs..j], &mut self.oracle);
            self.out.push_str(&processed);
        }
    }

    /// Locate the matching `</name>` (case-insensitive, boundary char
    /// after the name — whitespace, `/` or `>` — exactly like the
    /// tokenizer patterns; CDATA allows whitespace after the `/`,
    /// RCDATA does not). Returns (close_tag_start, index past its `>`);
    /// both are EOF when unclosed.
    fn find_close(&self, name: &str, allow_ws: bool) -> (usize, usize) {
        let input = self.input;
        let target = name.as_bytes();
        let mut j = self.pos;
        while j < input.len() {
            if input[j] == b'<' && input.get(j + 1) == Some(&b'/') {
                let mut k = j + 2;
                if allow_ws {
                    while k < input.len() && input[k].is_ascii_whitespace() {
                        k += 1;
                    }
                }
                if tag_name_eq(input, k, target) {
                    let after = k + target.len();
                    if after < input.len()
                        && (matches!(input[after], b'>' | b'/')
                            || input[after].is_ascii_whitespace())
                    {
                        let end = find_byte(input, after, b'>').map_or(input.len(), |p| p + 1);
                        return (j, end);
                    }
                }
            }
            j += 1;
        }
        (input.len(), input.len())
    }

    /// Copy a CDATA body verbatim through its matching close.
    fn skip_rawtext(&mut self, name: &str) {
        let (_, end) = self.find_close(name, true);
        self.out.push_str(&self.text[self.pos..end]);
        self.pos = end;
    }

    /// Process an RCDATA body: tags inside are data, but entities (and
    /// the `&#` death rule, which also swallows the close tag) apply.
    fn skip_rcdata(&mut self, name: &str) {
        let (close_start, close_end) = self.find_close(name, false);
        while self.pos < close_start {
            let text = self.text;
            if text[self.pos..].starts_with(PH_OPEN) {
                push_placeholder(&mut self.out, PH_OPEN);
                self.pos += PH_OPEN_LEN;
                continue;
            }
            match self.input[self.pos] {
                // (`<` is data here, not markup.)
                b'&' => self.walk_text_entity(),
                byte if byte.is_ascii() => {
                    self.out.push(byte as char);
                    self.pos += 1;
                }
                _ => {
                    let ch = text[self.pos..].chars().next().unwrap_or('\u{FFFD}');
                    self.out.push(ch);
                    self.pos += ch.len_utf8();
                }
            }
        }
        // (A death inside already consumed everything past the close.)
        if self.pos < close_end {
            self.out.push_str(&self.text[self.pos..close_end]);
            self.pos = close_end;
        }
    }

    /// Process one `&` in text: `charref`/`entityref` semantics with
    /// oracle-confirmed rewrites, plus the `&#` death rule.
    fn walk_text_entity(&mut self) {
        let input = self.input;
        let text = self.text;
        let i = self.pos;
        if input.get(i + 1) == Some(&b'#') {
            if let Some((value, end)) = match_charref(input, i) {
                if value == 0 || value == PH_OPEN as u64 {
                    if let Some(ch) = char::from_u32(value as u32) {
                        push_placeholder(&mut self.out, ch);
                    } else {
                        self.out.push_str(&text[i..end]);
                    }
                    self.pos = end;
                } else {
                    self.out.push('&');
                    self.pos = i + 1;
                }
                return;
            }
            // Rejected: literal `&`, plus the death rule — with no `;`
            // anywhere after, the rest of the input is data.
            self.out.push_str("&amp;");
            if self.last_semi.is_some_and(|s| s > i) {
                self.pos = i + 1;
            } else {
                self.escape_rest(i + 1);
            }
            return;
        }
        if let Some((ns, ne)) = match_entityref(input, i) {
            let name = &text[ns..ne];
            if self.oracle.full(name).is_some() {
                self.out.push('&');
                self.pos = i + 1;
                return;
            }
            // Unknown: `&name` with the `;` consumed when present; any
            // other trailing char is ordinary input (often `<`).
            self.out.push_str("&amp;");
            self.out.push_str(name);
            if input[ne] != b';' {
                self.pos = ne;
            } else {
                self.pos = ne + 1;
            }
            return;
        }
        // No match (alnum-follow or end of input): literal `&`, but
        // protected — html5ever text would resolve a legacy prefix.
        self.out.push_str("&amp;");
        self.pos = i + 1;
    }

    /// Death-rule escape: from `from` on, `&` → `&amp;`, `<` → `&lt;`,
    /// everything else verbatim. Ends the scan.
    fn escape_rest(&mut self, from: usize) {
        let input = self.input;
        let text = self.text;
        let mut j = from;
        while j < input.len() {
            if text[j..].starts_with(PH_OPEN) {
                push_placeholder(&mut self.out, PH_OPEN);
                j += PH_OPEN_LEN;
                continue;
            }
            match input[j] {
                b'&' => {
                    self.out.push_str("&amp;");
                    j += 1;
                }
                b'<' => {
                    self.out.push_str("&lt;");
                    j += 1;
                }
                byte if byte.is_ascii() => {
                    self.out.push(byte as char);
                    j += 1;
                }
                _ => {
                    let ch = text[j..].chars().next().unwrap_or('\u{FFFD}');
                    self.out.push(ch);
                    j += ch.len_utf8();
                }
            }
        }
        self.pos = input.len();
    }
}

/// Attribute read with placeholder substitution (the pre-scan may have
/// rewritten entity sequences inside values).
fn fixed_attr(element: &scraper::node::Element, name: &str) -> Option<String> {
    element
        .attr(name)
        .map(|value| substitute_placeholders(value).into_owned())
}

fn convert_html_to_markdown(html: &str) -> Result<String, HtmlConvertFail> {
    let normalized = normalize_html_source(html);
    let dom = Html::parse_fragment(&normalized);
    let root = dom.tree.root();
    enum Work<'a> {
        Enter {
            node: NodeRef<'a, Node>,
            tags: ParentTags,
            depth: usize,
            is_root: bool,
        },
        Exit {
            node: NodeRef<'a, Node>,
            tags: ParentTags,
            in_pre: bool,
            child_count: usize,
            is_root: bool,
        },
    }
    let mut work = vec![Work::Enter {
        node: root,
        tags: ParentTags::default(),
        depth: 0,
        is_root: true,
    }];
    // Post-order value stack: every pushed child yields exactly one string.
    let mut values: Vec<String> = Vec::new();
    while let Some(step) = work.pop() {
        match step {
            Work::Enter {
                node,
                tags,
                depth,
                is_root,
            } => match node.value() {
                Node::Text(text) => {
                    values.push(process_text_node(node, &text.text, tags));
                }
                Node::Comment(comment) => {
                    // bs4 parses `<?...?>` as a *text* node
                    // (`ProcessingInstruction` subclasses `NavigableString`);
                    // html5ever reports a bogus comment whose content keeps
                    // the leading `?`. Stripping one `?` recovers bs4's text
                    // (`<?php echo 1 ?>` -> `php echo 1 ?`, `<?foo?>` ->
                    // `foo?`). A real `<!--?...-->` is indistinguishable and
                    // reads the same way (documented edge).
                    if let Some(inner) = comment.comment.strip_prefix('?') {
                        values.push(process_text_node(node, inner, tags));
                    } else {
                        values.push(String::new());
                    }
                }
                Node::ProcessingInstruction(pi) => {
                    // Unreachable from HTML parsing (see the `Comment` arm),
                    // kept for parity in case the parser ever yields one:
                    // bs4's text is the raw `<?...>` inner.
                    let mut inner = pi.target.to_string();
                    if !pi.data.is_empty() {
                        inner.push(' ');
                        inner.push_str(&pi.data);
                    }
                    values.push(process_text_node(node, &inner, tags));
                }
                Node::Doctype(_) => values.push(String::new()),
                Node::Fragment | Node::Document => {
                    // The fragment root plays `[document]`; a nested
                    // `Fragment` (template contents in html5ever) is a
                    // transparent pass-through with no name of its own.
                    let kids = visible_children(node, "");
                    work.push(Work::Exit {
                        node,
                        tags,
                        in_pre: false,
                        child_count: kids.len(),
                        is_root,
                    });
                    for kid in kids.into_iter().rev() {
                        work.push(Work::Enter {
                            node: kid,
                            tags,
                            depth: depth + 1,
                            is_root: false,
                        });
                    }
                }
                Node::Element(element) => {
                    if depth > MAX_TAG_DEPTH {
                        return Err(HtmlConvertFail);
                    }
                    let name = element.name();
                    let mut child_tags = tags;
                    if name == "li" {
                        child_tags.li = true;
                    }
                    if name == "pre" {
                        child_tags.pre = true;
                    }
                    if matches!(name, "pre" | "code" | "kbd" | "samp") {
                        child_tags.noformat = true;
                    }
                    if name == "td" || name == "th" || is_heading_name(name) {
                        child_tags.inline = true;
                    }
                    let kids = visible_children(node, name);
                    work.push(Work::Exit {
                        node,
                        tags,
                        in_pre: name == "pre" || has_pre_ancestor(node),
                        child_count: kids.len(),
                        is_root: false,
                    });
                    for kid in kids.into_iter().rev() {
                        work.push(Work::Enter {
                            node: kid,
                            tags: child_tags,
                            depth: depth + 1,
                            is_root: false,
                        });
                    }
                }
            },
            Work::Exit {
                node,
                tags,
                in_pre,
                child_count,
                is_root,
            } => {
                let mut kids: Vec<String> = values.split_off(values.len() - child_count);
                kids.retain(|s| !s.is_empty());
                let mut text = if in_pre {
                    kids.concat()
                } else {
                    collapse_child_boundaries(&kids)
                };
                if is_root {
                    // `convert__document_` with `strip_document="strip"`.
                    text = text.trim_matches('\n').to_owned();
                } else if let Node::Element(element) = node.value() {
                    text = convert_element(node, element, text, tags)?;
                }
                values.push(text);
            }
        }
    }
    debug_assert_eq!(values.len(), 1);
    values.pop().ok_or(HtmlConvertFail)
}

/// `process_tag`'s `_can_ignore` (`__init__.py:215-238`): tags are always
/// processed, comments/doctypes ignored, whitespace-only text dropped next
/// to block boundaries. `elem_name` is `""` for the fragment root (which is
/// in no whitespace set, like `[document]`).
fn visible_children<'a>(node: NodeRef<'a, Node>, elem_name: &str) -> Vec<NodeRef<'a, Node>> {
    node.children()
        .filter(|kid| match kid.value() {
            Node::Element(_) | Node::Fragment | Node::Document => true,
            // `ProcessingInstruction` nodes are text (see `Enter`).
            Node::ProcessingInstruction(_) => true,
            // Comments are ignored — except the `<?...?>` bogus-comment
            // shape, which bs4 reads as text (see `Enter`).
            Node::Comment(comment) => comment.comment.starts_with('?'),
            Node::Doctype(_) => false,
            Node::Text(text) => {
                if !text.text.trim().is_empty() {
                    return true;
                }
                if should_remove_inside_name(elem_name)
                    && (kid.prev_sibling().is_none() || kid.next_sibling().is_none())
                {
                    return false;
                }
                !(should_remove_outside_node(kid.prev_sibling())
                    || should_remove_outside_node(kid.next_sibling()))
            }
        })
        .collect()
}

/// `should_remove_whitespace_inside` (`__init__.py:100-111`).
fn should_remove_inside_name(name: &str) -> bool {
    if is_heading_name(name) {
        return true;
    }
    matches!(
        name,
        "p" | "blockquote"
            | "article"
            | "div"
            | "section"
            | "ol"
            | "ul"
            | "li"
            | "dl"
            | "dt"
            | "dd"
            | "table"
            | "thead"
            | "tbody"
            | "tfoot"
            | "tr"
            | "td"
            | "th"
    )
}

/// `should_remove_whitespace_outside` (`__init__.py:114-116`).
fn should_remove_outside_node(node: Option<NodeRef<'_, Node>>) -> bool {
    match node {
        Some(sib) => match sib.value() {
            Node::Element(element) => {
                should_remove_inside_name(element.name()) || element.name() == "pre"
            }
            _ => false,
        },
        None => false,
    }
}

/// `re_html_heading` (`__init__.py:13`): a *prefix* match of `h` plus
/// decimal digits (`\d` is Unicode-aware; `²` is not `\d`, so `<h²>` is not
/// a heading — unlike the `isnumeric` checks elsewhere).
fn is_heading_name(name: &str) -> bool {
    let Some(rest) = name.strip_prefix('h') else {
        return false;
    };
    rest.chars().next().is_some_and(|c| c.is_ascii_digit())
}

/// `process_text` (`__init__.py:315-346`) with `wrap=False`. `escape()` is
/// skipped: all three escape options are off, so it is the identity.
fn process_text_node(node: NodeRef<'_, Node>, text: &str, tags: ParentTags) -> String {
    let mut out = if tags.pre {
        text.to_owned()
    } else {
        // `re_newline_whitespace.sub('\n')` then `re_whitespace.sub(' ')`:
        // every maximal `[\t \r\n]` run holding a `\r` or `\n` becomes one
        // `\n`, other runs (only tabs/spaces) become one space.
        let mut collapsed = String::with_capacity(text.len());
        let mut run_has_newline = false;
        let mut in_run = false;
        for ch in text.chars() {
            if matches!(ch, '\t' | ' ' | '\r' | '\n') {
                if ch == '\r' || ch == '\n' {
                    run_has_newline = true;
                }
                in_run = true;
            } else {
                if in_run {
                    collapsed.push(if run_has_newline { '\n' } else { ' ' });
                    run_has_newline = false;
                    in_run = false;
                }
                collapsed.push(ch);
            }
        }
        if in_run {
            collapsed.push(if run_has_newline { '\n' } else { ' ' });
        }
        collapsed
    };
    let prev = node.prev_sibling();
    let next = node.next_sibling();
    let parent_inside = matches!(node.parent().and_then(element_name), Some(name) if should_remove_inside_name(name));
    // Note the asymmetry: lstripping uses the explicit set, rstripping is
    // a full Unicode strip — verbatim from `:340-344`.
    if should_remove_outside_node(prev) || (parent_inside && prev.is_none()) {
        out = out.trim_start_matches([' ', '\t', '\r', '\n']).to_owned();
    }
    if should_remove_outside_node(next) || (parent_inside && next.is_none()) {
        out = out.trim_end().to_owned();
    }
    out
}

/// Newline collapsing at child boundaries (`__init__.py:271-288`) with
/// `re_extract_newlines` (`:20`).
fn collapse_child_boundaries(kids: &[String]) -> String {
    let mut out: Vec<String> = vec![String::new()];
    for kid in kids {
        let (mut leading, content, trailing) = split_newlines(kid);
        if !out.last().is_some_and(std::string::String::is_empty) && !leading.is_empty() {
            let prev = out.pop().unwrap_or_default();
            let joined = "\n".repeat(std::cmp::min(2, prev.len().max(leading.len())));
            leading = joined;
        }
        out.push(leading);
        out.push(content);
        out.push(trailing);
    }
    out.concat()
}

/// Split `(leading_newlines, content, trailing_newlines)` exactly like
/// `^(\n*)((?:.*[^\n])?)(\n*)$`: an all-newline string yields it all as
/// leading newlines with empty content and trailing.
fn split_newlines(text: &str) -> (String, String, String) {
    let bytes = text.as_bytes();
    let mut lead = 0;
    while lead < bytes.len() && bytes[lead] == b'\n' {
        lead += 1;
    }
    let mut trail = 0;
    while trail + lead < bytes.len() && bytes[bytes.len() - 1 - trail] == b'\n' {
        trail += 1;
    }
    (
        text[..lead].to_owned(),
        text[lead..bytes.len() - trail].to_owned(),
        text[bytes.len() - trail..].to_owned(),
    )
}

/// Element name, if this node is one.
fn element_name(node: NodeRef<'_, Node>) -> Option<&str> {
    match node.value() {
        Node::Element(element) => Some(element.name()),
        _ => None,
    }
}

/// Nearest previous sibling that is a tag (`find_previous_sibling()` with
/// no filter skips strings *and* comments — verified against live bs4).
fn prev_tag_sibling(node: NodeRef<'_, Node>) -> Option<NodeRef<'_, Node>> {
    let mut sib = node.prev_sibling();
    while let Some(candidate) = sib {
        if matches!(candidate.value(), Node::Element(_)) {
            return Some(candidate);
        }
        sib = candidate.prev_sibling();
    }
    None
}

/// `_next_block_content_sibling` (`__init__.py:145-151`): tags and
/// non-whitespace text count; comments, doctypes and whitespace text do
/// not. (`<?...?>` bogus comments count as their text, like bs4's
/// `ProcessingInstruction` nodes.)
fn next_block_content_sibling(node: NodeRef<'_, Node>) -> Option<NodeRef<'_, Node>> {
    let mut sib = node.next_sibling();
    while let Some(candidate) = sib {
        let is_content = match candidate.value() {
            Node::Element(_) | Node::Fragment | Node::Document => true,
            Node::ProcessingInstruction(_) => true,
            Node::Comment(comment) => comment
                .comment
                .strip_prefix('?')
                .is_some_and(|inner| !inner.trim().is_empty()),
            Node::Doctype(_) => false,
            Node::Text(text) => !text.text.trim().is_empty(),
        };
        if is_content {
            return Some(candidate);
        }
        sib = candidate.next_sibling();
    }
    None
}

/// Pre-order descendant elements (`find_all` traverses descendants only,
/// document order, tags only).
fn descendant_elements<'a>(node: NodeRef<'a, Node>) -> Vec<NodeRef<'a, Node>> {
    let mut out = Vec::new();
    let mut stack: Vec<NodeRef<'a, Node>> = node.children().rev().collect();
    while let Some(kid) = stack.pop() {
        // Push children reversed so the pop order stays document order.
        let kids: Vec<NodeRef<'a, Node>> = kid.children().collect();
        for grand in kids.into_iter().rev() {
            stack.push(grand);
        }
        if matches!(kid.value(), Node::Element(_)) {
            out.push(kid);
        }
    }
    out
}

/// `node.find_parent('pre')`: any ancestor element named `pre`.
fn has_pre_ancestor(node: NodeRef<'_, Node>) -> bool {
    let mut cursor = node.parent();
    while let Some(parent) = cursor {
        if element_name(parent) == Some("pre") {
            return true;
        }
        cursor = parent.parent();
    }
    false
}

/// `get_conv_fn` (`__init__.py:357-374`) + dispatch. `strip`/`convert` are
/// both `None`, so every tag converts; headings go through `_convert_hn`
/// and anything without a `convert_*` passes its text through untouched.
fn convert_element(
    node: NodeRef<'_, Node>,
    element: &scraper::node::Element,
    text: String,
    tags: ParentTags,
) -> Result<String, HtmlConvertFail> {
    if let Some(level) = heading_level(element.name()) {
        return Ok(convert_hn(level, &text, tags));
    }
    // `re_make_convert_fn_name` (`:16`): `[]:-` become `_`, so both
    // `issue-embed-component` and `issue_embed_component` dispatch to the
    // same converter — exactly like the Python `getattr` chain.
    let cname = element.name().replace(['[', ']', ':', '-'], "_");
    match cname.as_str() {
        "a" => Ok(convert_a(element, &text, tags)),
        "b" | "strong" => Ok(inline_wrap("**", &text, tags)),
        "em" | "i" => Ok(inline_wrap("*", &text, tags)),
        "del" | "s" => Ok(inline_wrap("~~", &text, tags)),
        "sub" | "sup" => Ok(inline_wrap("", &text, tags)),
        "code" | "kbd" | "samp" => Ok(convert_code(&text, tags)),
        "blockquote" => Ok(convert_blockquote(&text, tags)),
        "br" => Ok(convert_br(tags)),
        // Our `convert_div` override; `article`, `section` and `dl` alias
        // the *stock* `convert_div` (bound at class-definition time, before
        // the subclass overrode it), so the horizontal-rule arm must not
        // apply to them.
        "div" => Ok(convert_div_tiptap(element, &text, tags)),
        "article" | "section" | "dl" => Ok(convert_div_stock(&text, tags)),
        "dd" => Ok(convert_dd(&text, tags)),
        "dt" => Ok(convert_dt(&text, tags)),
        "hr" => Ok(convert_hr()),
        "img" => Ok(convert_img(node, element, &text, tags)),
        "video" => Ok(convert_video(node, element, &text, tags)),
        "ul" | "ol" => Ok(convert_list(node, text, tags)),
        "li" => convert_li_tiptap(node, element, text, tags),
        "p" => Ok(convert_p(&text, tags)),
        "pre" => Ok(convert_pre(node, &text)),
        "script" | "style" => Ok(String::new()),
        "table" => Ok(convert_table(&text)),
        "caption" => Ok(convert_caption(&text)),
        "figcaption" => Ok(convert_figcaption(&text)),
        "td" => convert_td(element, &text),
        "th" => convert_th(element, &text),
        "tr" => convert_tr(node, &text),
        "input" => Ok(String::new()),
        "mention_component" => Ok(convert_mention(element)),
        "image_component" => Ok(convert_image_component(element)),
        "issue_embed_component" => Ok(convert_issue_embed(element)),
        _ => Ok(text),
    }
}

/// Heading level from `re_html_heading` (`:13`): maximal leading `\d` run
/// after `h`, clamped to 1..=6 (`_convert_hn` `:517-518`). `\d` is
/// Unicode-decimal only, so no `int()` failure is possible (unlike the
/// `isnumeric`/`isdigit` checks below).
fn heading_level(name: &str) -> Option<u32> {
    let rest = name.strip_prefix('h')?;
    let mut digits = String::new();
    for ch in rest.chars() {
        if let Some(digit) = ch.to_digit(10) {
            digits.push(char::from_digit(digit, 10).unwrap_or('0'));
        } else {
            break;
        }
    }
    if digits.is_empty() {
        return None;
    }
    if digits.len() > 1 {
        return Some(6);
    }
    Some(digits.parse::<u32>().unwrap_or(0).clamp(1, 6))
}

/// `chomp` (`__init__.py:60-70`): hoist a single leading/trailing ASCII
/// space out of the stripped text.
fn chomp(text: &str) -> (bool, bool, &str) {
    let prefix = text.starts_with(' ');
    let suffix = text.ends_with(' ');
    (prefix, suffix, text.trim())
}

/// `abstract_inline_conversion` (`__init__.py:73-93`) with
/// `strong_em_symbol="*"` and empty sub/sup symbols (no markup starting
/// with `<` is possible, so the tag-like suffix arm is dead).
fn inline_wrap(markup: &str, text: &str, tags: ParentTags) -> String {
    if tags.noformat {
        return text.to_owned();
    }
    let (prefix, suffix, inner) = chomp(text);
    if inner.is_empty() {
        return String::new();
    }
    format!(
        "{}{}{}{}{}",
        if prefix { " " } else { "" },
        markup,
        inner,
        markup,
        if suffix { " " } else { "" }
    )
}

/// `convert_a` (`__init__.py:406-424`) with `autolinks=True` and
/// `default_title=False`.
fn convert_a(element: &scraper::node::Element, text: &str, tags: ParentTags) -> String {
    if tags.noformat {
        return text.to_owned();
    }
    let (prefix, suffix, inner) = chomp(text);
    if inner.is_empty() {
        return String::new();
    }
    let href = fixed_attr(element, "href").unwrap_or_default();
    let title = fixed_attr(element, "title").filter(|t| !t.is_empty());
    if title.is_none() && inner.replace("\\_", "_") == href {
        return format!("<{href}>");
    }
    if href.is_empty() {
        return inner.to_owned();
    }
    let title_part = title.map_or_else(String::new, |t| format!(" \"{}\"", t.replace('"', "\\\"")));
    format!(
        "{}[{inner}]({href}{title_part}){}",
        if prefix { " " } else { "" },
        if suffix { " " } else { "" }
    )
}

/// Prefix every line (`re_line_with_content`, `^(.*)` multiline): empty
/// lines get the bare marker.
fn prefix_lines(text: &str, marker: &str, indent: &str) -> String {
    text.split('\n')
        .map(|line| {
            if line.is_empty() {
                marker.to_owned()
            } else {
                format!("{indent}{line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// `convert_blockquote` (`__init__.py:428-442`).
fn convert_blockquote(text: &str, tags: ParentTags) -> String {
    let text = text.trim_matches([' ', '\t', '\r', '\n']);
    if tags.inline {
        return format!(" {text} ");
    }
    if text.is_empty() {
        return "\n".to_owned();
    }
    format!("\n{}\n\n", prefix_lines(text, ">", "> "))
}

/// `convert_br` (`__init__.py:444-451`) with `newline_style="spaces"`.
fn convert_br(tags: ParentTags) -> String {
    if tags.inline {
        return " ".to_owned();
    }
    "  \n".to_owned()
}

/// `convert_code` (`__init__.py:453-457`): inside `<pre>` the text passes
/// through, otherwise it is a backtick span.
fn convert_code(text: &str, tags: ParentTags) -> String {
    if tags.pre {
        return text.to_owned();
    }
    inline_wrap("`", text, tags)
}

/// `convert_div` (`__init__.py:461-465`) — the stock arm.
fn convert_div_stock(text: &str, tags: ParentTags) -> String {
    if tags.inline {
        return format!(" {} ", text.trim());
    }
    let text = text.trim();
    if text.is_empty() {
        String::new()
    } else {
        format!("\n\n{text}\n\n")
    }
}

/// Our `convert_div` (`markdown_converter.py:137-143`): the editor's
/// horizontal-rule wrapper converts as `<hr>`.
fn convert_div_tiptap(element: &scraper::node::Element, text: &str, tags: ParentTags) -> String {
    if fixed_attr(element, "data-type").as_deref() == Some("horizontalRule") {
        return convert_hr();
    }
    convert_div_stock(text, tags)
}

/// `convert_dd` (`__init__.py:475-491`).
fn convert_dd(text: &str, tags: ParentTags) -> String {
    let text = text.trim();
    if tags.inline {
        return format!(" {text} ");
    }
    if text.is_empty() {
        return "\n".to_owned();
    }
    let mut indented = prefix_lines(text, "", "    ");
    // The text is non-empty and stripped, so the first line holds the
    // 4-space indent; swapping its first byte for `:` is ASCII-safe.
    indented.replace_range(0..1, ":");
    format!("{indented}\n")
}

/// `convert_dt` (`__init__.py:498-510`).
fn convert_dt(text: &str, tags: ParentTags) -> String {
    let text = text.trim();
    let single: String = {
        let mut out = String::with_capacity(text.len());
        let mut in_ws = false;
        for ch in text.chars() {
            // `re_all_whitespace` (`:11`): the explicit `[\t \r\n]` set.
            if matches!(ch, '\t' | ' ' | '\r' | '\n') {
                if !in_ws {
                    out.push(' ');
                    in_ws = true;
                }
            } else {
                out.push(ch);
                in_ws = false;
            }
        }
        out
    };
    if tags.inline {
        return format!(" {single} ");
    }
    if single.is_empty() {
        return "\n".to_owned();
    }
    format!("\n\n{single}\n")
}

/// `_convert_hn` (`__init__.py:512-529`) with `heading_style="ATX"`.
fn convert_hn(level: u32, text: &str, tags: ParentTags) -> String {
    if tags.inline {
        return text.to_owned();
    }
    let level = level.clamp(1, 6) as usize;
    let mut single = String::with_capacity(text.len());
    let mut in_ws = false;
    for ch in text.trim().chars() {
        if matches!(ch, '\t' | ' ' | '\r' | '\n') {
            if !in_ws {
                single.push(' ');
                in_ws = true;
            }
        } else {
            single.push(ch);
            in_ws = false;
        }
    }
    format!("\n\n{} {single}\n\n", "#".repeat(level))
}

/// `convert_hr` (`__init__.py:531-532`).
fn convert_hr() -> String {
    "\n\n---\n\n".to_owned()
}

/// `convert_img` (`__init__.py:536-545`) with `keep_inline_images_in=[]`
/// (so the `_inline` arm always returns the alt text).
fn convert_img(
    node: NodeRef<'_, Node>,
    element: &scraper::node::Element,
    _text: &str,
    tags: ParentTags,
) -> String {
    let alt = fixed_attr(element, "alt").unwrap_or_default();
    if tags.inline && !node_inside_keep_images(node) {
        return alt;
    }
    let src = fixed_attr(element, "src").unwrap_or_default();
    let title_part = fixed_attr(element, "title")
        .filter(|t| !t.is_empty())
        .map_or_else(String::new, |t| format!(" \"{}\"", t.replace('"', "\\\"")));
    format!("![{alt}]({src}{title_part})")
}

/// `el.parent.name not in self.options['keep_inline_images_in']` — the list
/// is pinned empty, so this is always true; kept as a function so the read
/// stays visible next to its line reference.
fn node_inside_keep_images(_node: NodeRef<'_, Node>) -> bool {
    false
}

/// `convert_video` (`__init__.py:547-563`).
fn convert_video(
    node: NodeRef<'_, Node>,
    element: &scraper::node::Element,
    text: &str,
    tags: ParentTags,
) -> String {
    if tags.inline && !node_inside_keep_images(node) {
        return text.to_owned();
    }
    let mut src = fixed_attr(element, "src").unwrap_or_default();
    if src.is_empty() {
        src = descendant_elements(node)
            .into_iter()
            .filter(|el| element_name(*el) == Some("source"))
            .find_map(|el| match el.value() {
                Node::Element(source) => fixed_attr(source, "src"),
                _ => None,
            })
            .unwrap_or_default();
    }
    let poster = fixed_attr(element, "poster").unwrap_or_default();
    if !src.is_empty() && !poster.is_empty() {
        return format!("[![{text}]({poster})]({src})");
    }
    if !src.is_empty() {
        return format!("[{text}]({src})");
    }
    if !poster.is_empty() {
        return format!("![{text}]({poster})");
    }
    text.to_owned()
}

/// `convert_list` (`__init__.py:565-577`), shared by `<ul>` and `<ol>`.
fn convert_list(node: NodeRef<'_, Node>, text: String, tags: ParentTags) -> String {
    let mut before_paragraph = false;
    if let Some(next) = next_block_content_sibling(node) {
        // A text node has no name (`None`), which is not `ul`/`ol`.
        let name = element_name(next);
        if name != Some("ul") && name != Some("ol") {
            before_paragraph = true;
        }
    }
    if tags.li {
        return format!("\n{}", text.trim_end());
    }
    if before_paragraph {
        format!("\n\n{text}\n")
    } else {
        format!("\n\n{text}")
    }
}

/// `convert_li` (`__init__.py:579-610`) with `bullets="-"`.
fn convert_li_super(node: NodeRef<'_, Node>, text: String) -> Result<String, HtmlConvertFail> {
    let text = text.trim().to_owned();
    if text.is_empty() {
        return Ok("\n".to_owned());
    }
    let parent = node.parent().filter(|p| element_name(*p).is_some());
    let bullet = if parent.and_then(element_name) == Some("ol") {
        // `start` parses only when non-empty and `isnumeric`; the
        // conversion back to `int` can still fail (², roman numerals,
        // vulgar fractions), which raises into the stripped fallback.
        let start_attr = parent
            .and_then(|p| match p.value() {
                Node::Element(parent_el) => fixed_attr(parent_el, "start"),
                _ => None,
            })
            .unwrap_or_default();
        let start = if !start_attr.is_empty() && start_attr.chars().all(is_python_numeric) {
            decimal_value(&start_attr).ok_or(HtmlConvertFail)?
        } else {
            "1".to_owned()
        };
        let mut sib = node.prev_sibling();
        let mut count: usize = 0;
        while let Some(candidate) = sib {
            if element_name(candidate) == Some("li") {
                count += 1;
            }
            sib = candidate.prev_sibling();
        }
        format!("{}.", add_usize_to_decimal(&start, count))
    } else {
        let mut depth: i64 = -1;
        let mut cursor = Some(node);
        while let Some(el) = cursor {
            if element_name(el) == Some("ul") {
                depth += 1;
            }
            cursor = el.parent();
        }
        const BULLETS: &[u8] = b"-";
        (BULLETS[depth.rem_euclid(BULLETS.len() as i64) as usize] as char).to_string()
    };
    let bullet = format!("{bullet} ");
    let width = bullet.len();
    // The first line is non-empty (stripped above), so slicing the indent
    // back off is ASCII-safe.
    let mut indented = prefix_lines(&text, "", &" ".repeat(width));
    indented.replace_range(0..width, &bullet);
    Ok(format!("{indented}\n"))
}

/// Our `convert_li` (`markdown_converter.py:96-122`): editor task items get
/// a `[ ]`/`[x]` marker spliced after the bullet; ordered-list items have
/// no `- ` marker, so they pass through unchanged (`find` returns -1).
fn convert_li_tiptap(
    node: NodeRef<'_, Node>,
    element: &scraper::node::Element,
    text: String,
    _tags: ParentTags,
) -> Result<String, HtmlConvertFail> {
    let converted = convert_li_super(node, text)?;
    if fixed_attr(element, "data-type").as_deref() != Some("taskItem") {
        return Ok(converted);
    }
    let checked = fixed_attr(element, "data-checked")
        .is_some_and(|value| matches!(value.to_lowercase().as_str(), "" | "true"));
    let marker = if checked { "[x] " } else { "[ ] " };
    match converted.find("- ") {
        Some(at) => {
            let mut out = converted;
            out.insert_str(at + 2, marker);
            Ok(out)
        }
        None => Ok(converted),
    }
}

/// `str.isnumeric`: Unicode `Numeric` (`Nd`/`Nl`/`No`).
fn is_python_numeric(ch: char) -> bool {
    ch.is_numeric()
}

/// `int()` over a `isnumeric` string: decimal digits fold normally;
/// anything else (`²`, `Ⅷ`, `½`, …) fails like CPython into the fallback.
/// `char::to_digit` is ASCII-only, so the Unicode decimal blocks ship
/// explicitly (all 42 `Nd` blocks of Unicode 15.0, verified digit by
/// digit against CPython).
fn decimal_value(digits: &str) -> Option<String> {
    /// Block starts of the 10-digit `Nd` runs CPython 3.12 accepts.
    const ND_BLOCKS: &[u32] = &[
        0x0030, 0x0660, 0x06F0, 0x07C0, 0x0966, 0x09E6, 0x0A66, 0x0AE6, 0x0B66, 0x0BE6, 0x0C66,
        0x0CE6, 0x0D66, 0x0DE6, 0x0E50, 0x0ED0, 0x0F20, 0x1040, 0x1090, 0x17E0, 0x1810, 0x1946,
        0x19D0, 0x1A80, 0x1A90, 0x1B50, 0x1BB0, 0x1C40, 0x1C50, 0xA620, 0xA8D0, 0xA900, 0xA9D0,
        0xA9F0, 0xAA50, 0xABF0, 0xFF10, 0x104A0, 0x1D7CE, 0x1D7D8, 0x1D7E2, 0x1D7EC, 0x1D7F6,
    ];
    fn digit_value(ch: char) -> Option<u32> {
        if ch.is_ascii_digit() {
            return Some(ch as u32 - '0' as u32);
        }
        let code = ch as u32;
        ND_BLOCKS
            .iter()
            .find(|start| code >= **start && code < **start + 10)
            .map(|start| code - *start)
    }
    let mut value = String::new();
    for ch in digits.chars() {
        value.push(char::from_digit(digit_value(ch)?, 10)?);
    }
    if value.is_empty() {
        return None;
    }
    Some(value)
}

/// Exact decimal addition of a small count (ordered-list numbers are
/// arbitrary precision in Python; the `ol-start-huge2` vector pins 50
/// digits).
fn add_usize_to_decimal(decimal: &str, add: usize) -> String {
    let mut digits: Vec<u8> = decimal.bytes().map(|b| b - b'0').collect();
    let mut carry = add;
    let mut i = digits.len();
    while carry > 0 {
        if i == 0 {
            digits.insert(0, 0);
            i = 1;
        }
        i -= 1;
        let sum = digits[i] as usize + carry;
        digits[i] = (sum % 10) as u8;
        carry = sum / 10;
    }
    let first = digits
        .iter()
        .position(|&d| d != 0)
        .unwrap_or(digits.len() - 1);
    digits[first..].iter().map(|d| (b'0' + d) as char).collect()
}

/// `convert_p` (`__init__.py:612-622`) with `wrap=False`.
fn convert_p(text: &str, tags: ParentTags) -> String {
    if tags.inline {
        return format!(" {} ", text.trim_matches([' ', '\t', '\r', '\n']));
    }
    let text = text.trim();
    if text.is_empty() {
        String::new()
    } else {
        format!("\n\n{text}\n\n")
    }
}

/// `convert_pre` (`__init__.py:624-631`) with our
/// `code_language_callback` (`markdown_converter.py:158-172`): the first
/// `language-*` class of the first descendant `<code>` (bs4 reports `class`
/// as a whitespace-split list, hence `split_whitespace` here).
fn convert_pre(node: NodeRef<'_, Node>, text: &str) -> String {
    if text.is_empty() {
        return String::new();
    }
    let mut language = String::new();
    if let Some(code) = descendant_elements(node)
        .into_iter()
        .find(|el| element_name(*el) == Some("code"))
    {
        if let Node::Element(code_el) = code.value() {
            if let Some(classes) = fixed_attr(code_el, "class") {
                for class in classes.split_whitespace() {
                    if let Some(suffix) = class.strip_prefix("language-") {
                        language = suffix.to_owned();
                        break;
                    }
                }
            }
        }
    }
    format!("\n\n```{language}\n{text}\n```\n\n")
}

/// `convert_table` (`__init__.py:633-635`). Note there is no empty arm:
/// an empty table still yields four newlines.
fn convert_table(text: &str) -> String {
    format!("\n\n{}\n\n", text.trim())
}

/// `convert_caption` (`__init__.py:637-638`).
fn convert_caption(text: &str) -> String {
    format!("{}\n\n", text.trim())
}

/// `convert_figcaption` (`__init__.py:640-641`).
fn convert_figcaption(text: &str) -> String {
    format!("\n\n{}\n\n", text.trim())
}

/// `str.isdigit`: Unicode decimal digits plus the superscript/subscript
/// digit blocks. (Superscripts fail the later `int()` and raise into the
/// fallback; vulgar fractions and roman numerals are *not* `isdigit`, so a
/// `colspan` of `½` quietly means 1.)
fn is_python_digit(ch: char) -> bool {
    ch.is_ascii_digit()
        || matches!(
            ch,
            '⁰'..='⁹' | '₀'..='₉' | '²' | '³' | '¹'
        )
}

/// `colspan` of one cell: absent (or empty/non-digit) means 1; a digit
/// string parses, and a digit string `int()` rejects (`²`) fails the whole
/// conversion into the stripped fallback.
fn cell_colspan(element: &scraper::node::Element) -> Result<u64, HtmlConvertFail> {
    let Some(raw) = fixed_attr(element, "colspan") else {
        return Ok(1);
    };
    if raw.is_empty() || !raw.chars().all(is_python_digit) {
        return Ok(1);
    }
    let mut value: u64 = 0;
    for ch in raw.chars() {
        match ch.to_digit(10) {
            Some(digit) => {
                value = value
                    .checked_mul(10)
                    .and_then(|v| v.checked_add(digit as u64))
                    .unwrap_or(u64::MAX);
            }
            None => return Err(HtmlConvertFail),
        }
    }
    Ok(value)
}

/// `convert_td` (`__init__.py:643-648`).
fn convert_td(element: &scraper::node::Element, text: &str) -> Result<String, HtmlConvertFail> {
    convert_cell(element, text)
}

/// `convert_th` (`__init__.py:650-655`) — identical to `convert_td`.
fn convert_th(element: &scraper::node::Element, text: &str) -> Result<String, HtmlConvertFail> {
    convert_cell(element, text)
}

fn convert_cell(element: &scraper::node::Element, text: &str) -> Result<String, HtmlConvertFail> {
    let colspan = cell_colspan(element)?;
    if colspan > MAX_COLSPAN_TOTAL {
        return Err(HtmlConvertFail);
    }
    Ok(format!(
        " {}{}",
        text.trim().replace('\n', " "),
        " |".repeat(colspan as usize)
    ))
}

/// `convert_tr` (`__init__.py:657-695`) with `table_inference=False`.
///
/// On the auto-inserted `<tbody>`: html5ever wraps direct `<tr>` children
/// of `<table>` in a `<tbody>` that html.parser never builds. The wrap is
/// behavior-neutral here: a first row under the auto-`<tbody>` takes the
/// same `is_head_row_missing` branch (no `<thead>` sibling exists in that
/// shape) and the same overline branch (`tbody` without a previous tag
/// sibling), as the `table-no-sections`/`table-colspan` vectors prove. Only
/// a `<caption>`/`<colgroup>` directly before a `<tbody>`-less `<tr>` reads
/// differently (documented edge; stored tables always serialize `<tbody>`).
fn convert_tr(node: NodeRef<'_, Node>, text: &str) -> Result<String, HtmlConvertFail> {
    let cells: Vec<NodeRef<'_, Node>> = descendant_elements(node)
        .into_iter()
        .filter(|el| matches!(element_name(*el), Some("td" | "th")))
        .collect();
    let is_first_row = prev_tag_sibling(node).is_none();
    let parent = node.parent().filter(|p| element_name(*p).is_some());
    let parent_name = parent.and_then(element_name).unwrap_or("");
    // Note: `all([])` is true, so a cell-less row counts as a head row.
    let is_headrow = cells.iter().all(|cell| element_name(*cell) == Some("th"))
        || (parent_name == "thead"
            && parent.map_or(0, |p| {
                descendant_elements(p)
                    .iter()
                    .filter(|el| element_name(**el) == Some("tr"))
                    .count()
            }) == 1);
    let grandparent = parent.and_then(|p| p.parent());
    let grandparent_theads = grandparent.map_or(0, |gp| {
        descendant_elements(gp)
            .iter()
            .filter(|el| element_name(**el) == Some("thead"))
            .count()
    });
    let is_head_row_missing = (is_first_row && parent_name != "tbody")
        || (is_first_row && parent_name == "tbody" && grandparent_theads < 1);
    let mut full_colspan: u64 = 0;
    for cell in &cells {
        let colspan = match cell.value() {
            Node::Element(cell_el) => cell_colspan(cell_el)?,
            _ => 1,
        };
        full_colspan = full_colspan.saturating_add(colspan);
        if full_colspan > MAX_COLSPAN_TOTAL {
            return Err(HtmlConvertFail);
        }
    }
    let dashes = vec!["---"; full_colspan as usize].join(" | ");
    let blanks = vec![""; full_colspan as usize].join(" | ");
    let mut underline = String::new();
    let mut overline = String::new();
    if is_headrow && is_first_row {
        underline = format!("| {dashes} |\n");
    } else if is_head_row_missing
        || (is_first_row
            && (parent_name == "table"
                || (parent_name == "tbody" && parent.and_then(|p| prev_tag_sibling(p)).is_none())))
    {
        overline = format!("| {blanks} |\n| {dashes} |\n");
    }
    Ok(format!("{overline}|{text}\n{underline}"))
}

/// `_first_attr` (`markdown_converter.py:83-86`): the first candidate whose
/// raw value is present and non-empty — then stripped (a whitespace-only
/// value stops the search and yields `""`).
fn first_attr(element: &scraper::node::Element, names: &[&str]) -> String {
    for name in names {
        if let Some(value) = fixed_attr(element, name) {
            if !value.is_empty() {
                return value.trim().to_owned();
            }
        }
    }
    String::new()
}

/// `convert_mention_component` (`markdown_converter.py:145-152`).
fn convert_mention(element: &scraper::node::Element) -> String {
    let identifier = first_attr(element, &["entity_identifier", "id"]);
    let entity = first_attr(element, &["entity_name"]);
    let entity = if entity.is_empty() {
        "mention"
    } else {
        &entity
    };
    if identifier.is_empty() {
        format!("@{entity}")
    } else {
        format!("@{entity}:{identifier}")
    }
}

/// `convert_image_component` (`markdown_converter.py:154-162`).
fn convert_image_component(element: &scraper::node::Element) -> String {
    let src = first_attr(element, &["src"]);
    if !src.is_empty() {
        return format!("![]({src})");
    }
    let asset = first_attr(element, &["id"]);
    if asset.is_empty() {
        "[image]".to_owned()
    } else {
        format!("[image {asset}]")
    }
}

/// `convert_issue_embed_component` (`markdown_converter.py:164-172`).
fn convert_issue_embed(element: &scraper::node::Element) -> String {
    let entity = first_attr(element, &["entity_identifier", "id"]);
    let project = first_attr(element, &["project_identifier"]);
    let workspace = first_attr(element, &["workspace_identifier"]);
    let label = if entity.is_empty() {
        "work item".to_owned()
    } else {
        format!("work item {entity}")
    };
    if !entity.is_empty() && !project.is_empty() && !workspace.is_empty() {
        format!("[{label}](/{workspace}/projects/{project}/issues/{entity})")
    } else {
        format!("[{label}]")
    }
}

// ---------------------------------------------------------------------------
// `markdown_to_html` (`markdown_converter.py:192-272`)
// ---------------------------------------------------------------------------
//
// The renderer below replays `TiptapHTMLRenderer` rule by rule over
// pulldown-cmark's CommonMark event stream (`Options::ENABLE_TABLES |
// ENABLE_STRIKETHROUGH`, tasklists detected manually to match the plugin's
// exact space-only checkbox rule). Paragraphs always render (the port sets
// every `hidden` flag false), and every link/image destination passes
// through the mdurl 0.1.2 + punycode ports first, exactly as
// `normalizeLink`/`normalizeLinkText`/`validateLink` run at markdown-it
// parse time. Deliberate edges (beyond the ones listed above):
//
// * Invalid-destination links/images (`javascript:`, …) literalize with an
//   *exact* raw suffix from the source span — except a rejected *reference
//   definition*, which pulldown consumes silently (Python renders the
//   definition line as a paragraph), and a valid outer link around an
//   invalid inner one (pulldown resolves the inner link first).
// * A hard break right after a task marker (`- [ ]  \n…` vs `- [ ]\\\n…`)
//   disambiguates through the source span; both shapes are pathological.
// * Raw-HTML blocks re-render through the inline sub-parser (the `x`-prefix
//   trick) so emphasis, links, escapes and entities inside them match the
//   `html=False` paragraph parse; only `\r` inside them diverges (pulldown
//   normalizes it away, markdown-it keeps it).
// * Image-alt nesting past 500 drops the deeper levels (CPython raises
//   `RecursionError` into a 500 there; the `Result` here cannot express
//   that, so it degrades instead).

/// `mdurl.URL` (`mdurl/_url.py` + `_parse.py`).
#[derive(Debug, Clone, Default)]
struct MdUrl {
    protocol: Option<String>,
    slashes: bool,
    auth: Option<String>,
    port: Option<String>,
    hostname: Option<String>,
    hash: Option<String>,
    search: Option<String>,
    pathname: Option<String>,
}

/// `mdurl.parse` (`mdurl/_parse.py`) with `slashes_denote_host=True`
/// (both `normalizeLink` and `normalizeLinkText` pass `True`, so the fast
/// path is dead). Empirically this round-trips byte-identically through
/// `mdurl_format` for every realistic link destination except surrounding
/// whitespace, which is stripped.
fn mdurl_parse(url: &str) -> MdUrl {
    let mut out = MdUrl::default();
    // `rest = rest.strip()` (`_parse.py:123`): full Unicode strip.
    let mut rest = url.trim().to_owned();
    if rest.is_empty() {
        return out;
    }
    // `PROTOCOL_PATTERN = ^([a-z0-9.+-]+:)` case-insensitive.
    let mut proto_end = None;
    for (i, ch) in rest.char_indices() {
        if ch == ':' {
            if i > 0 {
                proto_end = Some(i);
            }
            break;
        }
        if !(ch.is_ascii_alphanumeric() || matches!(ch, '.' | '+' | '-')) {
            break;
        }
    }
    let mut proto = String::new();
    if let Some(i) = proto_end {
        proto = rest[..i + 1].to_owned();
        rest = rest[i + 1..].to_owned();
    }
    out.protocol = if proto.is_empty() {
        None
    } else {
        Some(proto.clone())
    };
    // `HOSTLESS_PROTOCOL` / `SLASHED_PROTOCOL`, looked up with the proto as
    // parsed (case-sensitive here — `HTTP:` misses, unlike below).
    let hostless = proto == "javascript" || proto == "javascript:";
    let slashed = matches!(
        proto.as_str(),
        "http"
            | "https"
            | "ftp"
            | "gopher"
            | "file"
            | "http:"
            | "https:"
            | "ftp:"
            | "gopher:"
            | "file:"
    );
    // `slashes_denote_host` is always true, so this branch always runs.
    // Note `slashes` keeps the `startswith` value for the hostname-block
    // condition below even when the strip is skipped (javascript).
    let slashes = rest.starts_with("//");
    if slashes && (proto.is_empty() || !hostless) {
        // `if slashes and not (proto and HOSTLESS[proto])`
        rest = rest[2..].to_owned();
        out.slashes = true;
    }
    if !hostless && (slashes || (!proto.is_empty() && !slashed)) {
        // Hostname block (`_parse.py:151-260`).
        // `host_end`: first of `HOST_ENDING_CHARS` (`/`, `?`, `#`).
        let mut host_end = rest.len();
        for (i, b) in rest.bytes().enumerate() {
            if matches!(b, b'/' | b'?' | b'#') {
                host_end = i;
                break;
            }
        }
        // `rest.rfind("@", 0, host_end + 1)` when bounded (the endpoint is
        // the terminator itself, never `@`, so `..host_end` is equivalent),
        // else over the whole rest.
        let bounded = host_end < rest.len();
        let haystack = if bounded {
            &rest[..host_end]
        } else {
            &rest[..]
        };
        if let Some(at) = haystack.rfind('@') {
            out.auth = Some(rest[..at].to_owned());
            rest = rest[at + 1..].to_owned();
        }
        // The host runs to the first of `NON_HOST_CHARS`
        // (`%/?;#` + `'"` + space/controls + `{}|\^`` + `<>"`` +
        // ` \r\n\t`), with one trailing colon trimmed back into the path.
        fn is_non_host(ch: char) -> bool {
            matches!(
                ch,
                '%' | '/'
                    | '?'
                    | ';'
                    | '#'
                    | '\''
                    | '{'
                    | '}'
                    | '|'
                    | '\\'
                    | '^'
                    | '`'
                    | '<'
                    | '>'
                    | '"'
                    | ' '
                    | '\r'
                    | '\n'
                    | '\t'
            )
        }
        let mut end = rest.len();
        for (i, ch) in rest.char_indices() {
            if is_non_host(ch) {
                end = i;
                break;
            }
        }
        if end > 0 && rest.as_bytes()[end - 1] == b':' {
            end -= 1;
        }
        let host = rest[..end].to_owned();
        rest = rest[end..].to_owned();
        // `parse_host`: `PORT_PATTERN = :[0-9]*$` searched (first match).
        // A bare trailing colon drops the colon and keeps no port.
        let mut hostname = host.clone();
        let mut port = None;
        let hbytes = host.as_bytes();
        let mut pi = 0;
        while pi < hbytes.len() {
            if hbytes[pi] == b':' && host[pi + 1..].bytes().all(|b| b.is_ascii_digit()) {
                if pi + 1 < host.len() {
                    port = Some(host[pi + 1..].to_owned());
                }
                hostname = host[..pi].to_owned();
                break;
            }
            pi += 1;
        }
        out.port = port;
        let ipv6 = hostname.starts_with('[') && hostname.ends_with(']');
        if !ipv6 {
            // Hostname validation (`_parse.py:200-247`): an invalid label
            // splits the rest back into the path. `HOSTNAME_PART_PATTERN`
            // is `^[+a-z0-9A-Z_-]{0,63}$` searched.
            let parts: Vec<&str> = hostname.split('.').collect();
            let mut valid_parts: Vec<&str> = Vec::new();
            for (i, part) in parts.iter().enumerate() {
                if part.is_empty() {
                    continue;
                }
                let valid = part.len() <= 63
                    && part
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'-' | b'_'));
                if valid {
                    valid_parts.push(part);
                    continue;
                }
                // `newpart`: non-ASCII becomes `x`, then test again (the
                // original part is kept when it passes).
                let newpart: String = part
                    .chars()
                    .map(|c| if (c as u32) > 127 { 'x' } else { c })
                    .collect();
                if newpart.len() <= 63
                    && newpart
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'-' | b'_'))
                {
                    valid_parts.push(part);
                    continue;
                }
                // `HOSTNAME_PART_START`: leading valid run + the rest.
                let mut cut = 0;
                for (j, b) in part.bytes().enumerate() {
                    if b.is_ascii_alphanumeric() || matches!(b, b'+' | b'-' | b'_') {
                        cut = j + 1;
                    } else {
                        break;
                    }
                }
                valid_parts.push(&part[..cut]);
                let mut not_host = vec![&part[cut..]];
                not_host.extend(parts[i + 1..].iter().copied());
                rest = format!("{}{}", not_host.join("."), rest);
                break;
            }
            // Note: when every label is empty (`hostname` all dots), Python
            // leaves `hostname` unset; `valid_parts` joining to `""` would
            // set it to `Some("")` instead. `format` treats both the same
            // (both render empty), and `normalizeLink`'s punycode arm skips
            // falsy hostnames either way — observably identical.
            out.hostname = Some(valid_parts.join("."));
        } else {
            out.hostname = Some(hostname);
        }
        if out.hostname.as_deref().is_some_and(|h| h.len() > 255) {
            out.hostname = Some(String::new());
        }
        if ipv6 {
            if let Some(h) = out.hostname.take() {
                out.hostname = Some(h[1..h.len() - 1].to_owned());
            }
        }
    }
    if let Some(i) = rest.find('#') {
        out.hash = Some(rest[i..].to_owned());
        rest = rest[..i].to_owned();
    }
    if let Some(i) = rest.find('?') {
        out.search = Some(rest[i..].to_owned());
        rest = rest[..i].to_owned();
    }
    if !rest.is_empty() {
        out.pathname = Some(rest);
    }
    // This lookup uses the *lowercased* proto (`lower_proto`), unlike the
    // case-sensitive checks above.
    if matches!(
        proto.to_lowercase().as_str(),
        "http"
            | "https"
            | "ftp"
            | "gopher"
            | "file"
            | "http:"
            | "https:"
            | "ftp:"
            | "gopher:"
            | "file:"
    ) && out.hostname.as_deref().is_some_and(|h| !h.is_empty())
        && out.pathname.is_none()
    {
        out.pathname = Some(String::new());
    }
    out
}

/// `mdurl.format` (`mdurl/_format.py`): `protocol//auth@host:port
/// pathname?search#hash`, bracketing IPv6 hosts (any hostname holding `:`).
fn mdurl_format(url: &MdUrl) -> String {
    let mut out = String::new();
    if let Some(protocol) = &url.protocol {
        out.push_str(protocol);
    }
    if url.slashes {
        out.push_str("//");
    }
    if let Some(auth) = &url.auth {
        out.push_str(auth);
        out.push('@');
    }
    if let Some(hostname) = &url.hostname {
        if hostname.contains(':') {
            out.push('[');
            out.push_str(hostname);
            out.push(']');
        } else {
            out.push_str(hostname);
        }
    }
    if let Some(port) = &url.port {
        out.push(':');
        out.push_str(port);
    }
    if let Some(pathname) = &url.pathname {
        out.push_str(pathname);
    }
    if let Some(search) = &url.search {
        out.push_str(search);
    }
    if let Some(hash) = &url.hash {
        out.push_str(hash);
    }
    out
}

/// `mdurl.encode` (`mdurl/_encode.py`) with the default char set:
/// alphanumerics plus `;/?:@&=+$,-_.!~*'()#` pass through, valid `%XX`
/// escapes are kept, everything else percent-encodes (uppercase hex,
/// UTF-8 for non-ASCII). Rust `str` cannot hold lone surrogates, so the
/// surrogate arms are unreachable.
fn mdurl_encode(text: &str) -> String {
    fn unreserved(byte: u8) -> bool {
        byte.is_ascii_alphanumeric() || b";/?:@&=+$,-_.!~*'()#".contains(&byte)
    }
    fn is_hex(byte: u8) -> bool {
        byte.is_ascii_hexdigit()
    }
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < bytes.len() {
        let byte = bytes[i];
        // `keep_escaped`: a `%` followed by two hex digits passes through
        // — but only when `i + 2 < len` (strict).
        if byte == b'%' && i + 2 < bytes.len() && is_hex(bytes[i + 1]) && is_hex(bytes[i + 2]) {
            out.push('%');
            out.push(bytes[i + 1] as char);
            out.push(bytes[i + 2] as char);
            i += 3;
            continue;
        }
        if byte < 0x80 {
            if unreserved(byte) {
                out.push(byte as char);
            } else {
                out.push_str(&format!("%{byte:02X}"));
            }
            i += 1;
        } else {
            // Non-ASCII char: percent-encode its UTF-8 bytes (`quote`).
            let ch = text[i..].chars().next().unwrap_or('\u{FFFD}');
            let mut buf = [0u8; 4];
            for b in ch.encode_utf8(&mut buf).bytes() {
                out.push_str(&format!("%{b:02X}"));
            }
            i += ch.len_utf8();
        }
    }
    out
}

/// `mdurl.decode` (`mdurl/_decode.py`) with `exclude =
/// DECODE_DEFAULT_CHARS + "%"`: maximal `%XX` runs decode as UTF-8 (strict:
/// overlong/surrogate/out-of-range sequences yield one U+FFFD per `%XX`
/// unit); decoded chars in the exclude set re-encode uppercase.
fn mdurl_decode(text: &str) -> String {
    const EXCLUDE: &[u8] = b";/?:@&=+$,#%";
    fn is_hex(byte: u8) -> bool {
        byte.is_ascii_hexdigit()
    }
    fn hex_val(byte: u8) -> u8 {
        match byte {
            b'0'..=b'9' => byte - b'0',
            b'a'..=b'f' => byte - b'a' + 10,
            b'A'..=b'F' => byte - b'A' + 10,
            _ => 0,
        }
    }
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < bytes.len() {
        // Match a maximal `(%XX)+` run.
        if bytes[i] == b'%'
            && bytes.get(i + 1).is_some_and(|b| is_hex(*b))
            && bytes.get(i + 2).is_some_and(|b| is_hex(*b))
        {
            let run_start = i;
            let mut run_end = i;
            while bytes.get(run_end) == Some(&b'%')
                && bytes.get(run_end + 1).is_some_and(|b| is_hex(*b))
                && bytes.get(run_end + 2).is_some_and(|b| is_hex(*b))
            {
                run_end += 3;
            }
            // Decode the run (`decodeCache` semantics inline).
            let run = &bytes[run_start..run_end];
            let mut j = 0;
            while j < run.len() {
                let b1 = hex_val(run[j + 1]) * 16 + hex_val(run[j + 2]);
                if b1 < 0x80 {
                    if EXCLUDE.contains(&b1) {
                        out.push_str(&format!("%{b1:02X}"));
                    } else {
                        out.push(b1 as char);
                    }
                    j += 3;
                    continue;
                }
                // Multi-byte assembly with the strict bounds from
                // `_decode.py` (`i + 3 < l`, …): each arm requires the
                // full unit run to be present.
                let need: usize = if b1 & 0xE0 == 0xC0 {
                    2
                } else if b1 & 0xF0 == 0xE0 {
                    3
                } else if b1 & 0xF8 == 0xF0 {
                    4
                } else {
                    out.push('\u{FFFD}');
                    j += 3;
                    continue;
                };
                let l = run.len();
                let fits = match need {
                    2 => j + 3 < l,
                    3 => j + 3 < l && j + 6 < l,
                    _ => j + 3 < l && j + 6 < l && j + 9 < l,
                };
                let mut seq = vec![b1];
                let mut ok = fits;
                if ok {
                    for k in 1..need {
                        let b = hex_val(run[j + k * 3 + 1]) * 16 + hex_val(run[j + k * 3 + 2]);
                        if b & 0xC0 != 0x80 {
                            ok = false;
                            break;
                        }
                        seq.push(b);
                    }
                }
                if ok {
                    match std::str::from_utf8(&seq) {
                        Ok(s) => out.push_str(s),
                        Err(_) => {
                            for _ in 0..need {
                                out.push('\u{FFFD}');
                            }
                        }
                    }
                    j += need * 3;
                } else {
                    out.push('\u{FFFD}');
                    j += 3;
                }
            }
            i = run_end;
        } else {
            // Bytes outside `%XX` runs pass through untouched (bounds stay
            // on char boundaries: runs are pure ASCII).
            let ch = text[i..].chars().next().unwrap_or('\u{FFFD}');
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    out
}

// --- Punycode (RFC 3492, matching `markdown_it._punycode`, itself the
// `codecs` algorithm) ----------------------------------------------------

/// `punycode_encode`: `None` on arithmetic overflow (absurd labels only;
/// callers keep the original hostname, mirroring `suppress(Exception)`).
fn puny_encode(label: &str) -> Option<String> {
    const BASE: u64 = 36;
    const TMIN: u64 = 1;
    const TMAX: u64 = 26;
    const SKEW: u64 = 38;
    const DAMP: u64 = 700;
    const INITIAL_BIAS: u64 = 72;
    const INITIAL_N: u32 = 128;
    fn digit(d: u64) -> char {
        if d < 26 {
            (b'a' + d as u8) as char
        } else {
            (b'0' + (d - 26) as u8) as char
        }
    }
    fn adapt(mut delta: u64, numpoints: u64, first: bool) -> Option<u64> {
        delta = if first { delta / DAMP } else { delta / 2 };
        delta += delta / numpoints;
        let mut k = 0u64;
        while delta > (BASE - TMIN) * TMAX / 2 {
            delta /= BASE - TMIN;
            k += BASE;
        }
        k.checked_add((BASE - TMIN + 1) * delta / (delta + SKEW))
    }
    let input: Vec<u32> = label.chars().map(|c| c as u32).collect();
    let mut output = String::new();
    for &c in &input {
        // Basic code points copy through with case preserved (`MÜNCHEN`
        // keeps its uppercase, unlike IDNA processing).
        if c < 128 {
            output.push(c as u8 as char);
        }
    }
    let mut h = output.len() as u64;
    let b = h;
    if b > 0 {
        output.push('-');
    }
    let mut n = INITIAL_N;
    let mut delta: u64 = 0;
    let mut bias = INITIAL_BIAS;
    while h < input.len() as u64 {
        let m = input.iter().copied().filter(|&c| c >= n).min()?;
        delta = delta.checked_add((m - n) as u64 * (h + 1))?;
        n = m;
        for &c in &input {
            if c < n {
                delta = delta.checked_add(1)?;
            }
            if c == n {
                let mut q = delta;
                let mut k = BASE;
                loop {
                    let t = if k <= bias {
                        TMIN
                    } else if k >= bias + TMAX {
                        TMAX
                    } else {
                        k - bias
                    };
                    if q < t {
                        break;
                    }
                    output.push(digit(t + (q - t) % (BASE - t)));
                    q = (q - t) / (BASE - t);
                    k = k.checked_add(BASE)?;
                }
                output.push(digit(q));
                bias = adapt(delta, h + 1, h == b)?;
                delta = 0;
                h += 1;
            }
        }
        delta = delta.checked_add(1)?;
        n = n.checked_add(1)?;
    }
    Some(output)
}

/// `punycode_decode`: `None` on invalid digits, overflow, or unrepresentable
/// code points (surrogates, past U+10FFFF) — all suppressed upstream.
fn puny_decode(code: &str) -> Option<String> {
    const BASE: u64 = 36;
    const TMIN: u64 = 1;
    const TMAX: u64 = 26;
    const SKEW: u64 = 38;
    const DAMP: u64 = 700;
    const INITIAL_BIAS: u64 = 72;
    const INITIAL_N: u32 = 128;
    fn value(ch: char) -> Option<u64> {
        match ch {
            'a'..='z' => Some((ch as u64) - ('a' as u64)),
            'A'..='Z' => Some((ch as u64) - ('A' as u64)),
            '0'..='9' => Some((ch as u64) - ('0' as u64) + 26),
            _ => None,
        }
    }
    fn adapt(mut delta: u64, numpoints: u64, first: bool) -> Option<u64> {
        delta = if first { delta / DAMP } else { delta / 2 };
        delta += delta / numpoints;
        let mut k = 0u64;
        while delta > (BASE - TMIN) * TMAX / 2 {
            delta /= BASE - TMIN;
            k += BASE;
        }
        k.checked_add((BASE - TMIN + 1) * delta / (delta + SKEW))
    }
    let (basic, extended) = match code.rfind('-') {
        Some(at) => (&code[..at], &code[at + 1..]),
        None => (code, ""),
    };
    let mut output: Vec<u32> = Vec::with_capacity(basic.len() + 1);
    for ch in basic.chars() {
        if ch as u32 >= 128 {
            return None;
        }
        output.push(ch as u32);
    }
    let ext: Vec<char> = extended.chars().collect();
    let mut pos = 0usize;
    let mut n = INITIAL_N;
    let mut i: u64 = 0;
    let mut bias = INITIAL_BIAS;
    while pos < ext.len() {
        let oldi = i;
        let mut w: u64 = 1;
        let mut k = BASE;
        loop {
            if pos >= ext.len() {
                return None;
            }
            let d = value(ext[pos])?;
            pos += 1;
            i = i.checked_add(d.checked_mul(w)?)?;
            let t = if k <= bias {
                TMIN
            } else if k >= bias + TMAX {
                TMAX
            } else {
                k - bias
            };
            if d < t {
                break;
            }
            w = w.checked_mul(BASE - t)?;
            k = k.checked_add(BASE)?;
        }
        let out_len = output.len() as u64 + 1;
        bias = adapt(i - oldi, out_len, oldi == 0)?;
        n = n.checked_add((i / out_len).try_into().ok()?)?;
        i %= out_len;
        char::from_u32(n)?;
        output.insert(i as usize, n);
        i = i.checked_add(1)?;
    }
    output.into_iter().map(char::from_u32).collect()
}

/// `map_domain` (`_punycode.py`): split `user@host` at the *first* `@`
/// (further `@` parts are dropped — literal `parts[1]` semantics), map each
/// dot label (dots: `.`, `。`, `．`, `｡`), rejoin with `.`.
fn map_domain(host: &str, map: impl Fn(&str) -> Option<String>) -> Option<String> {
    const SEPS: &[char] = &['.', '。', '．', '｡'];
    let (prefix, labels) = match host.split_once('@') {
        Some((user, rest)) => {
            let rest = rest.split('@').next().unwrap_or("");
            (format!("{user}@"), rest)
        }
        None => (String::new(), host),
    };
    let mut mapped = Vec::new();
    for label in labels.split(SEPS) {
        mapped.push(map(label)?);
    }
    Some(format!("{prefix}{}", mapped.join(".")))
}

/// `to_ascii`: labels holding any char past `~` (DEL included) encode.
fn puny_to_ascii(host: &str) -> Option<String> {
    map_domain(host, |label| {
        if label.chars().any(|c| c == '\x7f' || c > '\x7e') {
            Some(format!("xn--{}", puny_encode(label)?))
        } else {
            Some(label.to_owned())
        }
    })
}

/// `to_unicode`: `xn--` labels (case-sensitive) decode after lowercasing.
fn puny_to_unicode(host: &str) -> Option<String> {
    map_domain(host, |label| {
        if let Some(code) = label.strip_prefix("xn--") {
            puny_decode(&code.to_lowercase())
        } else {
            Some(label.to_owned())
        }
    })
}

// --- URL normalization (`markdown_it.common.normalize_url`) ---------------

/// `normalizeLink`: parse, punycode the hostname for schemeless/`http`/
/// `https`/`mailto:` URLs (failures keep the original), re-encode.
fn normalize_link(url: &str) -> String {
    let mut parsed = mdurl_parse(url);
    let recode = parsed.protocol.is_none()
        || matches!(
            parsed.protocol.as_deref(),
            Some("http:" | "https:" | "mailto:")
        );
    if recode {
        if let Some(hostname) = parsed.hostname.clone() {
            if !hostname.is_empty() {
                if let Some(ascii) = puny_to_ascii(&hostname) {
                    parsed.hostname = Some(ascii);
                }
            }
        }
    }
    mdurl_encode(&mdurl_format(&parsed))
}

/// `normalizeLinkText`: same, but decode for display.
fn normalize_link_text(url: &str) -> String {
    let mut parsed = mdurl_parse(url);
    let recode = parsed.protocol.is_none()
        || matches!(
            parsed.protocol.as_deref(),
            Some("http:" | "https:" | "mailto:")
        );
    if recode {
        if let Some(hostname) = parsed.hostname.clone() {
            if !hostname.is_empty() {
                if let Some(unicode) = puny_to_unicode(&hostname) {
                    parsed.hostname = Some(unicode);
                }
            }
        }
    }
    mdurl_decode(&mdurl_format(&parsed))
}

/// `validateLink`: bad protocols (`vbscript`/`javascript`/`file`/`data`)
/// fail unless the URL is an image `data:` URL. Runs on the stripped,
/// lowercased URL — including for autolinks (the autolink rule calls it).
fn validate_link(url: &str) -> bool {
    let folded = url.trim().to_lowercase();
    let bad = folded.starts_with("vbscript:")
        || folded.starts_with("javascript:")
        || folded.starts_with("file:")
        || folded.starts_with("data:");
    if !bad {
        return true;
    }
    folded.starts_with("data:image/gif;")
        || folded.starts_with("data:image/png;")
        || folded.starts_with("data:image/jpeg;")
        || folded.starts_with("data:image/webp;")
}

/// `TiptapHTMLRenderer._is_safe_url` (`:240-249`): empty and
/// `http`/`https`/`mailto`/`tel` schemes render `href`; anything else (or
/// an unparseable URL) drops it.
fn is_safe_url(url: &str) -> bool {
    match url_scheme(url) {
        Err(()) => false,
        Ok(scheme) => {
            scheme.is_empty() || matches!(scheme.as_str(), "http" | "https" | "mailto" | "tel")
        }
    }
}

/// `urllib.parse.urlsplit` reduced to scheme-or-`ValueError`. Only the
/// scheme and the bracket validation are reachable here (destinations are
/// mdurl-normalized, so `_checknetloc`'s non-ASCII arm never fires — proven
/// by construction, skipped below).
fn url_scheme(url: &str) -> Result<String, ()> {
    // Strip WHATWG C0-or-space (`\x00`-`\x20`) on the left, then drop every
    // `\t`/`\r`/`\n`.
    let stripped: String = url
        .trim_start_matches(|c: char| (c as u32) <= 0x20)
        .chars()
        .filter(|c| !matches!(c, '\t' | '\r' | '\n'))
        .collect();
    // Scheme: `[^:]+:` with a leading ASCII letter and
    // `[a-zA-Z0-9+.-]*` after.
    let mut scheme = String::new();
    let mut rest = stripped.as_str();
    if let Some(i) = stripped.find(':') {
        if i > 0 {
            let candidate = &stripped[..i];
            let mut chars = candidate.chars();
            if chars.next().is_some_and(|c| c.is_ascii_alphabetic())
                && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-'))
            {
                scheme = candidate.to_lowercase();
                rest = &stripped[i + 1..];
            }
        }
    }
    // Bracket validation on the netloc, if any (`_check_bracketed_netloc`
    // + `_check_bracketed_host`).
    if let Some(after) = rest.strip_prefix("//") {
        let mut netloc = after;
        for (i, b) in after.bytes().enumerate() {
            if matches!(b, b'/' | b'?' | b'#') {
                netloc = &after[..i];
                break;
            }
        }
        let has_open = netloc.contains('[');
        let has_close = netloc.contains(']');
        if has_open != has_close {
            return Err(());
        }
        if has_open {
            let host_part = netloc.rsplit('@').next().unwrap_or("");
            let Some(bracketed) = host_part.strip_prefix('[') else {
                return Err(());
            };
            let Some(close_at) = bracketed.find(']') else {
                return Err(());
            };
            let after = &bracketed[close_at + 1..];
            if !(after.is_empty() || after.starts_with(':')) {
                return Err(());
            }
            check_bracketed_host(&bracketed[..close_at])?;
        }
    }
    Ok(scheme)
}

/// `_check_bracketed_host`: `v...` must match IPvFuture, anything else must
/// parse as an IP literal that is not IPv4. (CPython also rejects
/// leading-zero IPv4, which `IpAddr` would accept — matched explicitly.)
fn check_bracketed_host(hostname: &str) -> Result<(), ()> {
    if let Some(rest) = hostname.strip_prefix('v') {
        // `\Av[a-fA-F0-9]+\..+\Z` (no DOTALL, but `\n` cannot occur here).
        let mut chars = rest.chars();
        let mut hex_count = 0;
        while chars.clone().next().is_some_and(|c| c.is_ascii_hexdigit()) {
            chars.next();
            hex_count += 1;
        }
        if hex_count == 0 {
            return Err(());
        }
        if chars.next() != Some('.') {
            return Err(());
        }
        if chars.next().is_none() {
            return Err(());
        }
        return Ok(());
    }
    if hostname
        .split('.')
        .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
        && hostname.split('.').count() == 4
        && hostname
            .split('.')
            .any(|part| part.len() > 1 && part.starts_with('0'))
    {
        return Err(());
    }
    match hostname.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V6(_)) => Ok(()),
        _ => Err(()),
    }
}

// --- Renderer (`TiptapHTMLRenderer`) ----------------------------------------

/// `markdown_it.common.utils.escapeHtml`: exactly `&`, `<`, `>`, `"`.
fn escape_html4(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(ch),
        }
    }
    out
}

type MdEvent<'a> = (Event<'a>, std::ops::Range<usize>);

/// Per-item verdict of `_resolve_task_lists` (`:251-273`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ItemKind {
    /// No task marker: render untouched.
    Plain,
    /// Marker matched but the char after it is not a space (`\t`, `\n`,
    /// …): the plugin stripped the marker and left an *empty* (invisible)
    /// checkbox, so the rest renders verbatim with no marker.
    StripOnly,
    /// A real task item: strip the marker, `lstrip` the rest.
    Task { checked: bool },
    /// A would-be task in a non-task list: re-emit the normalized marker
    /// (`[x]` even for `[X]`) ahead of the verbatim rest.
    Restore { checked: bool },
}

/// `_resolve_task_lists` over the pulldown event stream: decide every
/// list's `task_list` flag and every item's [`ItemKind`]. The plugin only
/// ever *inserts* the checkbox and strips the marker; the `_is_task_item`
/// predicate it feeds on reduces to "marker matched and the 4th char is a
/// space" (any other GFM whitespace yields the empty checkbox, which fails
/// the `_TASK_CHECKBOX_MARKER in content` test).
fn resolve_lists(
    events: &mut Vec<MdEvent<'_>>,
    md: &str,
) -> (Vec<Option<ItemKind>>, Vec<Option<bool>>) {
    struct ListFrame {
        bullet: bool,
        items: Vec<usize>,
        start: usize,
    }
    let mut kinds: Vec<Option<ItemKind>> = vec![None; events.len()];
    let mut task_lists: Vec<Option<bool>> = vec![None; events.len()];
    // item index -> (matched, fourth_is_space, checked).
    let mut marks: std::collections::HashMap<usize, (bool, bool, bool)> =
        std::collections::HashMap::new();
    let mut all_items: Vec<usize> = Vec::new();
    let mut stack: Vec<ListFrame> = Vec::new();
    for (i, (event, _)) in events.iter().enumerate() {
        match event {
            Event::Start(Tag::List(ordered)) => {
                stack.push(ListFrame {
                    bullet: ordered.is_none(),
                    items: Vec::new(),
                    start: i,
                });
            }
            Event::End(TagEnd::List(_)) => {
                if let Some(frame) = stack.pop() {
                    let task_list = frame.bullet
                        && !frame.items.is_empty()
                        && frame
                            .items
                            .iter()
                            .all(|item| marks.get(item).is_some_and(|m| m.0 && m.1));
                    task_lists[frame.start] = Some(task_list);
                    for item in &frame.items {
                        let (matched, fourth_space, checked) =
                            marks.get(item).copied().unwrap_or((false, false, false));
                        kinds[*item] = Some(if !matched {
                            ItemKind::Plain
                        } else if !fourth_space {
                            ItemKind::StripOnly
                        } else if task_list {
                            ItemKind::Task { checked }
                        } else {
                            ItemKind::Restore { checked }
                        });
                    }
                }
            }
            Event::Start(Tag::Item) => {
                if let Some(frame) = stack.last_mut() {
                    frame.items.push(i);
                    all_items.push(i);
                    marks.insert(i, scan_item_marker(events, i, md));
                } else {
                    kinds[i] = Some(ItemKind::Plain);
                }
            }
            _ => {}
        }
    }
    // Splice every stripped marker into its post-plugin form now, while
    // the scan verdicts are fresh. Each splice replaces N runs with N
    // events (one replacement plus empty padding), so no index moves.
    for item_at in all_items {
        if let Some(kind) = kinds[item_at] {
            if kind != ItemKind::Plain {
                strip_splice(events, item_at, md, kind);
            }
        }
    }
    (kinds, task_lists)
}

/// An item's first-inline range: its first paragraph's content, or the
/// bare run up to the first nested block.
fn item_inline_range(events: &[MdEvent<'_>], item_at: usize) -> Option<(usize, usize)> {
    let item_end = matching_end(events, item_at);
    if item_at + 1 >= item_end {
        return None;
    }
    let inline_at = item_at + 1;
    if matches!(events[inline_at].0, Event::Start(Tag::Paragraph)) {
        let mut end = inline_at + 1;
        while end < item_end && !matches!(events[end].0, Event::End(TagEnd::Paragraph)) {
            end += 1;
        }
        return Some((inline_at + 1, end.min(item_end)));
    }
    let mut end = inline_at;
    while end < item_end {
        if let Event::Start(tag) = &events[end].0 {
            if is_block_tag(tag) {
                break;
            }
        }
        if matches!(events[end].0, Event::End(_)) {
            break;
        }
        end += 1;
    }
    Some((inline_at, end))
}

/// Match the tasklists plugin's `^\[[ xX]]` + GFM-whitespace test against
/// an item's first inline run. Returns (matched, fourth_is_space, checked).
fn scan_item_marker(events: &[MdEvent<'_>], item_at: usize, md: &str) -> (bool, bool, bool) {
    let Some((inline_at, inline_end)) = item_inline_range(events, item_at) else {
        return (false, false, false);
    };
    scan_marker_in(events, inline_at, inline_end, md)
}

/// Splice an item's leading marker runs into their post-plugin form.
/// The plugin slices the first 3 chars off the first text child and,
/// for task items, lstrips the rest — all within that one child, never
/// across a break. Pulldown splits the child into runs (`[`, ` `, `]`,
/// …) and trims a trailing space before a break, so the splice
/// re-stitches runs + break-gap first, then writes one replacement
/// plus empty padding (event count unchanged, indices stay valid).
fn strip_splice(events: &mut Vec<MdEvent<'_>>, item_at: usize, md: &str, kind: ItemKind) {
    let Some((inline_at, inline_end)) = item_inline_range(events, item_at) else {
        return;
    };
    let mut k = inline_at;
    let mut stitched = String::new();
    while k < inline_end {
        if let Event::Text(text) = &events[k].0 {
            stitched.push_str(text);
            k += 1;
        } else {
            break;
        }
    }
    if k == inline_at || stitched.len() < 3 {
        return;
    }
    // A space pulldown trimmed before a break is still content to the
    // plugin (a trimmed paragraph-end space is not — markdown-it never
    // had it — so only breaks contribute a gap).
    let runs_end = events[k - 1].1.end;
    let gap_end = if k < inline_end && matches!(events[k].0, Event::SoftBreak | Event::HardBreak) {
        events[k].1.start
    } else {
        runs_end
    };
    stitched.push_str(md.get(runs_end..gap_end).unwrap_or(""));
    // The marker is 3 ASCII chars (`[`, `[ xX]`, `]`), so byte
    // indexing at 3 is safe.
    let rest = &stitched[3..];
    let replacement = match kind {
        // `children[0].content.lstrip()` — full Unicode strip.
        ItemKind::Task { .. } => rest.trim_start().to_owned(),
        ItemKind::StripOnly => rest.to_owned(),
        // The marker goes back normalized (`[x]` even for `[X]`)
        // ahead of the verbatim rest.
        ItemKind::Restore { checked } => {
            format!("{}{rest}", if checked { "[x]" } else { "[ ]" })
        }
        ItemKind::Plain => return,
    };
    let mut patched: Vec<MdEvent<'_>> = Vec::with_capacity(k - inline_at);
    patched.push((
        Event::Text(CowStr::from(replacement)),
        events[inline_at].1.start..gap_end,
    ));
    for event in &events[inline_at + 1..k] {
        patched.push((Event::Text(CowStr::Borrowed("")), event.1.clone()));
    }
    events.splice(inline_at..k, patched);
}

/// Start of the whitespace run pulldown trimmed before a break at
/// `pos`: spaces, tabs, vertical tabs and form feeds (never `\r` — a
/// carriage return is always a line boundary of its own).
fn gap_start_before(md: &str, pos: usize) -> usize {
    let bytes = md.as_bytes();
    let mut start = pos.min(bytes.len());
    while start > 0 && matches!(bytes[start - 1], b' ' | b'\t' | 0x0b | 0x0c) {
        start -= 1;
    }
    start
}

/// GFM whitespace (`mdit_py_plugins.tasklists`: `[ \t\n\v\f\r]`).
fn is_gfm_whitespace(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

fn scan_marker_in(
    events: &[MdEvent<'_>],
    inline_at: usize,
    inline_end: usize,
    md: &str,
) -> (bool, bool, bool) {
    // Stitch the leading text runs (pulldown splits at brackets/entities;
    // markdown-it joins them, so both observe the same string).
    let mut stitched = String::new();
    let mut k = inline_at;
    while k < inline_end {
        if let Event::Text(text) = &events[k].0 {
            stitched.push_str(text);
            k += 1;
        } else {
            break;
        }
    }
    let bytes = stitched.as_bytes();
    if bytes.len() < 3
        || bytes[0] != b'['
        || !matches!(bytes[1], b' ' | b'x' | b'X')
        || bytes[2] != b']'
    {
        return (false, false, false);
    }
    let checked = bytes[1] != b' ';
    // The 4th char: from the stitched text, a soft break (`\n`), or — for a
    // hard break — the source byte itself (a space continues the marker, a
    // backslash does not; the events are identical either way).
    let fourth = if bytes.len() > 3 {
        Some(bytes[3])
    } else if k < inline_end {
        match &events[k].0 {
            Event::SoftBreak => Some(b'\n'),
            Event::HardBreak => md.as_bytes().get(events[k].1.start).copied(),
            _ => None,
        }
    } else {
        None
    };
    match fourth {
        Some(byte) if is_gfm_whitespace(byte) => (true, byte == b' ', checked),
        _ => (false, false, false),
    }
}

/// Index of the `End` matching the `Start` at `start` (all tags counted).
fn matching_end(events: &[MdEvent<'_>], start: usize) -> usize {
    let mut depth = 0usize;
    let mut i = start;
    while i < events.len() {
        match &events[i].0 {
            Event::Start(_) => depth += 1,
            Event::End(_) => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return i;
                }
            }
            _ => {}
        }
        i += 1;
    }
    events.len()
}

/// Tags that open a block context (close a synthetic item paragraph, stop
/// the item-marker scan, count toward the lone-image depth).
fn is_block_tag(tag: &Tag<'_>) -> bool {
    matches!(
        tag,
        Tag::Paragraph
            | Tag::Heading { .. }
            | Tag::BlockQuote(_)
            | Tag::CodeBlock(_)
            | Tag::HtmlBlock
            | Tag::List(_)
            | Tag::Item
            | Tag::FootnoteDefinition(_)
            | Tag::Table(_)
            | Tag::TableHead
            | Tag::TableRow
            | Tag::TableCell
            | Tag::MetadataBlock(_)
            | Tag::DefinitionList
    )
}

/// `End` of a block tag (mirrors [`is_block_tag`]).
fn is_block_end(tag: &TagEnd) -> bool {
    matches!(
        tag,
        TagEnd::Paragraph
            | TagEnd::Heading(_)
            | TagEnd::BlockQuote(_)
            | TagEnd::CodeBlock
            | TagEnd::HtmlBlock
            | TagEnd::List(_)
            | TagEnd::Item
            | TagEnd::FootnoteDefinition
            | TagEnd::Table
            | TagEnd::TableHead
            | TagEnd::TableRow
            | TagEnd::TableCell
            | TagEnd::MetadataBlock(_)
            | TagEnd::DefinitionList
            | TagEnd::DefinitionListTitle
            | TagEnd::DefinitionListDefinition
    )
}

enum Frame {
    /// A list item: `synth_p` tracks the synthesized `<p>` pulldown omits
    /// for tight items (Python renders every paragraph, `hidden=False`).
    /// Marker strips were spliced at resolve time, so `kind` only picks
    /// the `<li>` shape here.
    Item {
        kind: ItemKind,
        synth_p: bool,
    },
    Paragraph {
        suppressed: bool,
    },
    Heading {
        level: u8,
    },
    Table {
        in_head: bool,
    },
    /// An invalid-destination link/image: children render as inline and the
    /// exact raw suffix (`](…)`, including original quote style) closes it.
    /// Autolinks swallow their raw text child (already rendered via a
    /// nested inline parse) and close with a literal `&gt;`.
    LitLink {
        link_start: usize,
        link_end: usize,
        last_child_end: Option<usize>,
        prefix_len: usize,
        swallow: bool,
        close_gt: bool,
    },
    /// A *valid* autolink/email link: its (single, raw) text child renders
    /// through `normalizeLinkText`, which is what markdown-it stores in the
    /// text token at parse time.
    AutoLink,
}

struct Renderer<'a, 'e> {
    events: &'e [MdEvent<'a>],
    md: &'a str,
    item_kinds: Vec<Option<ItemKind>>,
    task_lists: Vec<Option<bool>>,
    out: String,
    stack: Vec<Frame>,
    block_depth: usize,
    /// Nested autolink-content render: no paragraphs, no lone-image
    /// figures, no synthesized blocks — inline events only.
    inline_only: bool,
}

impl Renderer<'_, '_> {
    /// Close a synthetic item paragraph when a nested block starts.
    fn close_synth_before_block(&mut self) {
        if let Some(Frame::Item { synth_p, .. }) = self.stack.last_mut() {
            if *synth_p {
                self.out.push_str("</p>");
                *synth_p = false;
            }
        }
    }

    /// Open a synthetic `<p>` for bare tight-item inline runs.
    fn open_synth_for_inline(&mut self) {
        if self.inline_only {
            return;
        }
        if let Some(Frame::Item { synth_p, .. }) = self.stack.last_mut() {
            if !*synth_p {
                self.out.push_str("<p>");
                *synth_p = true;
            }
        }
    }

    /// `_lone_image` (`:313-322`): a top-level paragraph holding exactly one
    /// image with a safe `src` drops its `<p>` wrapper.
    fn lone_image_ahead(&self, para_at: usize) -> bool {
        if self.inline_only {
            return false;
        }
        let mut para_end = para_at + 1;
        while para_end < self.events.len()
            && !matches!(self.events[para_end].0, Event::End(TagEnd::Paragraph))
        {
            para_end += 1;
        }
        if para_end >= self.events.len() || para_at + 1 >= para_end {
            return false;
        }
        let (Event::Start(Tag::Image { dest_url, .. }), _) = &self.events[para_at + 1] else {
            return false;
        };
        // The single image must span the whole paragraph.
        let mut depth = 0usize;
        let mut i = para_at + 1;
        let mut image_end = None;
        while i < para_end {
            match &self.events[i].0 {
                Event::Start(Tag::Image { .. }) => depth += 1,
                Event::End(TagEnd::Image) => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        image_end = Some(i);
                        break;
                    }
                }
                _ => {}
            }
            i += 1;
        }
        if image_end != Some(para_end - 1) {
            return false;
        }
        let normalized = normalize_link(dest_url);
        validate_link(&normalized) && is_safe_url(&normalized)
    }

    /// Pop the top frame when it is a paragraph.
    fn pop_paragraph(&mut self) -> Option<bool> {
        match self.stack.last() {
            Some(Frame::Paragraph { suppressed }) => {
                let suppressed = *suppressed;
                self.stack.pop();
                Some(suppressed)
            }
            _ => None,
        }
    }

    fn run(&mut self) {
        let mut i = 0;
        while i < self.events.len() {
            // Owned per iteration: `CowStr` clones are pointer copies for
            // borrowed spans, and ownership keeps the loop borrow-safe.
            let (event, range) = self.events[i].clone();
            // Track the raw suffix start for every open literal link: the
            // maximum end of its inner events. `Start` spans can cover
            // their whole construct (`*b*`), so a plain assignment would
            // move the suffix start *backwards* past the closing marker
            // when the inner text arrives; `End` spans are tracked too
            // for the same reason. Only the frame's own closing `End`
            // is excluded — it would collapse the suffix to nothing.
            let closes_lit = matches!(event, Event::End(TagEnd::Link | TagEnd::Image))
                && matches!(self.stack.last(), Some(Frame::LitLink { .. }));
            let stack_len = self.stack.len();
            for (idx, frame) in self.stack.iter_mut().enumerate() {
                if let Frame::LitLink {
                    last_child_end,
                    swallow,
                    ..
                } = frame
                {
                    if !*swallow && !(closes_lit && idx + 1 == stack_len) {
                        let prev = last_child_end.unwrap_or(0);
                        *last_child_end = Some(prev.max(range.end));
                    }
                }
            }
            match event {
                Event::Start(tag) => {
                    // Code and HTML blocks jump past their `End`, so they
                    // must not count depth (nothing decrements them).
                    let jumped = matches!(tag, Tag::CodeBlock(_) | Tag::HtmlBlock);
                    if !jumped && is_block_tag(&tag) {
                        self.block_depth += 1;
                    }
                    match tag {
                        Tag::Paragraph => {
                            self.close_synth_before_block();
                            let suppressed = self.block_depth == 1 && self.lone_image_ahead(i);
                            if !suppressed {
                                self.out.push_str("<p>");
                            }
                            self.stack.push(Frame::Paragraph { suppressed });
                        }
                        Tag::Heading { level, .. } => {
                            self.close_synth_before_block();
                            let n = match level {
                                HeadingLevel::H1 => 1,
                                HeadingLevel::H2 => 2,
                                HeadingLevel::H3 => 3,
                                HeadingLevel::H4 => 4,
                                HeadingLevel::H5 => 5,
                                HeadingLevel::H6 => 6,
                            };
                            self.out.push_str(&format!("<h{n}>"));
                            self.stack.push(Frame::Heading { level: n });
                        }
                        Tag::BlockQuote(_) => {
                            self.close_synth_before_block();
                            self.out.push_str("<blockquote>");
                        }
                        Tag::CodeBlock(_) => {
                            self.close_synth_before_block();
                            i = self.render_code_block(i);
                        }
                        Tag::HtmlBlock => {
                            self.close_synth_before_block();
                            i = self.render_html_block(i);
                        }
                        Tag::List(ordered) => {
                            self.close_synth_before_block();
                            match ordered {
                                None => {
                                    if self.task_lists[i] == Some(true) {
                                        self.out.push_str("<ul data-type=\"taskList\">");
                                    } else {
                                        self.out.push_str("<ul>");
                                    }
                                }
                                Some(start) => {
                                    if start != 1 {
                                        self.out.push_str(&format!(
                                            "<ol start=\"{}\">",
                                            escape_html4(&start.to_string())
                                        ));
                                    } else {
                                        self.out.push_str("<ol>");
                                    }
                                }
                            }
                        }
                        Tag::Item => {
                            self.close_synth_before_block();
                            let kind = self.item_kinds[i].unwrap_or(ItemKind::Plain);
                            match kind {
                                ItemKind::Task { checked } => {
                                    let flag = if checked { "true" } else { "false" };
                                    let checkbox = if checked {
                                        "<input type=\"checkbox\" checked=\"checked\">"
                                    } else {
                                        "<input type=\"checkbox\">"
                                    };
                                    self.out.push_str(&format!(
                                        "<li data-type=\"taskItem\" data-checked=\"{flag}\"><label>{checkbox}<span></span></label><div>"
                                    ));
                                }
                                _ => self.out.push_str("<li>"),
                            }
                            self.stack.push(Frame::Item {
                                kind,
                                synth_p: false,
                            });
                        }
                        Tag::Table(_) => {
                            self.close_synth_before_block();
                            self.out.push_str("<table><tbody>");
                            self.stack.push(Frame::Table { in_head: false });
                        }
                        Tag::TableHead => {
                            self.close_synth_before_block();
                            self.out.push_str("<tr>");
                            if let Some(Frame::Table { in_head }) = self.stack.last_mut() {
                                *in_head = true;
                            }
                        }
                        Tag::TableRow => {
                            self.out.push_str("<tr>");
                        }
                        Tag::TableCell => {
                            let head =
                                matches!(self.stack.last(), Some(Frame::Table { in_head: true }));
                            if head {
                                self.out.push_str("<th><p>");
                            } else {
                                self.out.push_str("<td><p>");
                            }
                        }
                        Tag::Emphasis => {
                            self.open_synth_for_inline();
                            self.out.push_str("<em>");
                        }
                        Tag::Strong => {
                            self.open_synth_for_inline();
                            self.out.push_str("<strong>");
                        }
                        Tag::Strikethrough => {
                            self.open_synth_for_inline();
                            self.out.push_str("<s>");
                        }
                        Tag::Link {
                            link_type,
                            dest_url,
                            title,
                            ..
                        } => {
                            self.open_synth_for_inline();
                            self.render_link_start(i, link_type, &dest_url, &title);
                        }
                        Tag::Image {
                            dest_url, title, ..
                        } => {
                            self.open_synth_for_inline();
                            i = self.render_image_start(i, &dest_url, &title);
                        }
                        _ => {
                            // Unreachable with the enabled options
                            // (footnotes, math, definition lists, … are all
                            // off): children render transparently.
                        }
                    }
                }
                Event::End(tag) => {
                    if matches!(tag, TagEnd::Link | TagEnd::Image)
                        && matches!(self.stack.last(), Some(Frame::LitLink { .. }))
                    {
                        // Close a literal link with its exact raw suffix;
                        // outer literal links still observe this `End`.
                        if let Some(Frame::LitLink {
                            link_start,
                            link_end,
                            last_child_end,
                            prefix_len,
                            swallow,
                            close_gt,
                        }) = self.stack.pop()
                        {
                            if !swallow {
                                let from = last_child_end.unwrap_or(link_start + prefix_len);
                                self.out.push_str(self.md.get(from..link_end).unwrap_or(""));
                            } else if close_gt {
                                self.out.push_str("&gt;");
                            }
                            for frame in self.stack.iter_mut() {
                                if let Frame::LitLink {
                                    last_child_end,
                                    swallow,
                                    ..
                                } = frame
                                {
                                    if !*swallow {
                                        let prev = last_child_end.unwrap_or(0);
                                        *last_child_end = Some(prev.max(range.end));
                                    }
                                }
                            }
                        }
                    } else {
                        match tag {
                            TagEnd::Paragraph => {
                                if let Some(suppressed) = self.pop_paragraph() {
                                    if !suppressed {
                                        self.out.push_str("</p>");
                                    }
                                }
                            }
                            TagEnd::Heading(_) => {
                                if let Some(Frame::Heading { level }) = self.stack.pop() {
                                    self.out.push_str(&format!("</h{level}>"));
                                } else {
                                    self.out.push_str("</h1>");
                                }
                            }
                            TagEnd::BlockQuote(_) => self.out.push_str("</blockquote>"),
                            TagEnd::List(ordered) => {
                                if ordered {
                                    self.out.push_str("</ol>");
                                } else {
                                    self.out.push_str("</ul>");
                                }
                            }
                            TagEnd::Item => {
                                if let Some(Frame::Item { kind, synth_p, .. }) = self.stack.pop() {
                                    if synth_p {
                                        self.out.push_str("</p>");
                                    }
                                    match kind {
                                        ItemKind::Task { .. } => {
                                            self.out.push_str("</div></li>");
                                        }
                                        _ => self.out.push_str("</li>"),
                                    }
                                }
                            }
                            TagEnd::Table => {
                                self.stack.pop();
                                self.out.push_str("</tbody></table>");
                            }
                            TagEnd::TableHead => {
                                if let Some(Frame::Table { in_head }) = self.stack.last_mut() {
                                    *in_head = false;
                                }
                                self.out.push_str("</tr>");
                            }
                            TagEnd::TableRow => self.out.push_str("</tr>"),
                            TagEnd::TableCell => {
                                let head = matches!(
                                    self.stack.last(),
                                    Some(Frame::Table { in_head: true })
                                );
                                if head {
                                    self.out.push_str("</p></th>");
                                } else {
                                    self.out.push_str("</p></td>");
                                }
                            }
                            TagEnd::Emphasis => self.out.push_str("</em>"),
                            TagEnd::Strong => self.out.push_str("</strong>"),
                            TagEnd::Strikethrough => self.out.push_str("</s>"),
                            TagEnd::Link => {
                                if matches!(self.stack.last(), Some(Frame::AutoLink)) {
                                    self.stack.pop();
                                }
                                self.out.push_str("</a>");
                            }
                            // Valid images jump past their `End`; code/HTML
                            // blocks likewise. Anything else unknown renders
                            // transparently.
                            _ => {}
                        }
                    }
                    if is_block_end(&tag) {
                        self.block_depth = self.block_depth.saturating_sub(1);
                    }
                }
                Event::Text(text) => {
                    let swallowed = matches!(
                        self.stack.last(),
                        Some(Frame::LitLink { swallow: true, .. })
                    );
                    if !swallowed {
                        self.open_synth_for_inline();
                        if matches!(self.stack.last(), Some(Frame::AutoLink)) {
                            // Autolink text is raw in pulldown but
                            // `normalizeLinkText`-normalized in markdown-it
                            // (one text child; no breaks are possible).
                            self.out
                                .push_str(&escape_html4(&normalize_link_text(&text)));
                        } else {
                            self.out.push_str(&escape_html4(&text));
                        }
                    }
                }
                Event::Code(code) => {
                    let swallowed = matches!(
                        self.stack.last(),
                        Some(Frame::LitLink { swallow: true, .. })
                    );
                    if !swallowed {
                        self.open_synth_for_inline();
                        // Base-class `code_inline` (inherited, not
                        // overridden): no attributes render.
                        self.out
                            .push_str(&format!("<code>{}</code>", escape_html4(&code)));
                    }
                }
                Event::SoftBreak => {
                    let swallowed = matches!(
                        self.stack.last(),
                        Some(Frame::LitLink { swallow: true, .. })
                    );
                    if !swallowed {
                        self.open_synth_for_inline();
                        // Pulldown trims the line's trailing whitespace;
                        // markdown-it keeps it. Recover the gap between
                        // the previous content and this break.
                        let gap_start = gap_start_before(self.md, range.start);
                        self.out
                            .push_str(self.md.get(gap_start..range.start).unwrap_or(""));
                        // A soft break after a space collapses (the space
                        // already separates the words); otherwise it
                        // renders one space. Tabs do not collapse.
                        if !self.out.ends_with(' ') {
                            self.out.push(' ');
                        }
                    }
                }
                Event::HardBreak => {
                    let swallowed = matches!(
                        self.stack.last(),
                        Some(Frame::LitLink { swallow: true, .. })
                    );
                    if !swallowed {
                        self.open_synth_for_inline();
                        let gap_start = gap_start_before(self.md, range.start);
                        self.out
                            .push_str(self.md.get(gap_start..range.start).unwrap_or(""));
                        // The span covers the break itself (`  \n`,
                        // `\\\n`, `\t  \n`, …): re-emit whatever precedes
                        // the trailing spaces/backslash + newline.
                        let span = self.md.get(range.clone()).unwrap_or("");
                        let body = span.strip_suffix('\n').unwrap_or(span);
                        let body = body.strip_suffix('\r').unwrap_or(body);
                        let body = match body.strip_suffix('\\') {
                            Some(prefix) => prefix,
                            None => body.trim_end_matches(' '),
                        };
                        self.out.push_str(body);
                        self.out.push_str("<br>");
                    }
                }
                Event::InlineHtml(html) => {
                    let swallowed = matches!(
                        self.stack.last(),
                        Some(Frame::LitLink { swallow: true, .. })
                    );
                    if !swallowed {
                        self.open_synth_for_inline();
                        // With `html=False` this tag is paragraph text and
                        // its newline a soft break — not the (dead)
                        // `html_inline` rule.
                        self.out.push_str(&escape_html4(&html.replace('\n', " ")));
                    }
                }
                Event::Rule => {
                    self.close_synth_before_block();
                    self.out
                        .push_str("<div data-type=\"horizontalRule\"><div></div></div>");
                }
                // `Event::Html` only occurs inside `HtmlBlock` (jumped
                // over); tasklist markers, footnotes and math are off.
                _ => {}
            }
            i += 1;
        }
    }

    /// `link_open` (`:408-415`): email destinations gain their `mailto:`
    /// (pulldown withholds it); invalid destinations literalize.
    fn render_link_start(&mut self, at: usize, link_type: LinkType, dest_url: &str, title: &str) {
        let raw = match link_type {
            LinkType::Email => format!("mailto:{dest_url}"),
            _ => dest_url.to_owned(),
        };
        let normalized = normalize_link(&raw);
        if !validate_link(&normalized) {
            let range = self.events[at].1.clone();
            match link_type {
                LinkType::Autolink | LinkType::Email => {
                    // A failed autolink is literal text: `<`, then the
                    // inner source re-parsed as inline markdown
                    // (emphasis and code spans inside still render),
                    // then `>`. The raw text child is swallowed
                    // (already rendered below).
                    self.out.push_str("&lt;");
                    let inner = self.render_autolink_inner(range.start, range.end);
                    self.out.push_str(&inner);
                    self.stack.push(Frame::LitLink {
                        link_start: range.start,
                        link_end: range.end,
                        last_child_end: None,
                        prefix_len: 0,
                        swallow: true,
                        close_gt: true,
                    });
                }
                _ => {
                    self.out.push('[');
                    self.stack.push(Frame::LitLink {
                        link_start: range.start,
                        link_end: range.end,
                        last_child_end: None,
                        prefix_len: 1,
                        swallow: false,
                        close_gt: false,
                    });
                }
            }
            return;
        }
        let mut attrs = String::new();
        if is_safe_url(&normalized) {
            attrs.push_str(&format!(" href=\"{}\"", escape_html4(&normalized)));
        }
        if !title.is_empty() {
            attrs.push_str(&format!(" title=\"{}\"", escape_html4(title)));
        }
        self.out.push_str(&format!("<a{attrs}>"));
        if matches!(link_type, LinkType::Autolink | LinkType::Email) {
            self.stack.push(Frame::AutoLink);
        }
    }

    /// Render an invalid autolink's inner source as inline markdown.
    /// markdown-it fails the whole autolink rule, so `<` stays literal
    /// and the content tokenizes normally (`<javascript:*x*>` renders
    /// its emphasis). The content holds no `<`, `>` or whitespace, so
    /// a nested parse is almost always one paragraph; anything else (a
    /// lone `***` rule, a swallowed link definition) falls back to the
    /// literal escaped text. (Reference links inside resolve against
    /// nothing — the nested parse sees no definitions — where Python
    /// would use the outer document's; recorded in the PR.)
    fn render_autolink_inner(&self, link_start: usize, link_end: usize) -> String {
        let Some(inner) = self.md.get(link_start + 1..link_end.saturating_sub(1)) else {
            return String::new();
        };
        let options = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH;
        let nested: Vec<MdEvent<'_>> = Parser::new_ext(inner, options)
            .into_offset_iter()
            .map(|(event, range)| {
                (
                    event,
                    range.start + link_start + 1..range.end + link_start + 1,
                )
            })
            .collect();
        let Some(inline) = single_paragraph_inline(&nested) else {
            return escape_html4(inner);
        };
        let mut sub = Renderer {
            events: inline,
            md: self.md,
            item_kinds: vec![None; nested.len()],
            task_lists: vec![None; nested.len()],
            out: String::new(),
            stack: Vec::new(),
            block_depth: 0,
            inline_only: true,
        };
        sub.run();
        sub.out
    }

    /// `image` (`:417-431`): invalid destinations literalize with `![`;
    /// valid ones jump past their children with the computed alt text.
    /// Returns the index to continue from (the matching `End`, or `at`).
    fn render_image_start(&mut self, at: usize, dest_url: &str, title: &str) -> usize {
        let normalized = normalize_link(dest_url);
        if !validate_link(&normalized) {
            let range = self.events[at].1.clone();
            self.out.push_str("![");
            self.stack.push(Frame::LitLink {
                link_start: range.start,
                link_end: range.end,
                last_child_end: None,
                prefix_len: 2,
                swallow: false,
                close_gt: false,
            });
            return at;
        }
        let end = matching_image_end(self.events, at);
        let alt = alt_text(self.events, at + 1, end, self.md, 0);
        if normalized.is_empty() || !is_safe_url(&normalized) {
            self.out.push_str(&escape_html4(&alt));
        } else {
            let mut attrs = format!(" src=\"{}\"", escape_html4(&normalized));
            if !alt.is_empty() {
                attrs.push_str(&format!(" alt=\"{}\"", escape_html4(&alt)));
            }
            if !title.is_empty() {
                attrs.push_str(&format!(" title=\"{}\"", escape_html4(title)));
            }
            self.out.push_str(&format!("<img{attrs}>"));
        }
        end
    }

    /// `fence` / `code_block` (`:368-379`): the info string comes from the
    /// source span — pulldown unescapes it, but `token.info` is raw.
    /// Returns the matching `End` index.
    fn render_code_block(&mut self, at: usize) -> usize {
        let mut end = at + 1;
        while end < self.events.len()
            && !matches!(self.events[end].0, Event::End(TagEnd::CodeBlock))
        {
            end += 1;
        }
        let mut body = String::new();
        for (event, _) in &self.events[at + 1..end.min(self.events.len())] {
            if let Event::Text(text) = event {
                body.push_str(text);
            }
        }
        // `.removesuffix("\n")` — exactly one trailing newline.
        if body.ends_with('\n') {
            body.pop();
        }
        let language = match &self.events[at].0 {
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(_))) => {
                let start = self.events[at].1.start;
                let line = self
                    .md
                    .get(start..)
                    .and_then(|s| s.lines().next())
                    .unwrap_or("");
                let fence = line.chars().next().unwrap_or('`');
                let fence = if fence == '~' { '~' } else { '`' };
                let info = line.trim_start_matches(fence);
                info.split_whitespace().next().unwrap_or("").to_owned()
            }
            _ => String::new(),
        };
        if language.is_empty() {
            self.out.push_str("<pre><code>");
        } else {
            self.out.push_str(&format!(
                "<pre><code class=\"language-{}\">",
                escape_html4(&language)
            ));
        }
        self.out.push_str(&escape_html4(&body));
        self.out.push_str("</code></pre>");
        end.min(self.events.len().saturating_sub(1))
    }

    /// Raw-HTML blocks render as the paragraphs markdown-it parses them as
    /// (`html=False`): blank-line-separated groups, lines joined on soft
    /// breaks, each line inline-parsed (escapes, entities, emphasis, links)
    /// through the `x`-prefix sub-parse. Returns the matching `End` index.
    fn render_html_block(&mut self, at: usize) -> usize {
        let mut end = at + 1;
        while end < self.events.len()
            && !matches!(self.events[end].0, Event::End(TagEnd::HtmlBlock))
        {
            end += 1;
        }
        let mut raw = String::new();
        for (event, _) in &self.events[at + 1..end.min(self.events.len())] {
            if let Event::Html(html) = event {
                raw.push_str(html);
            }
        }
        // Paragraph groups split on blank (`[ \t]*`-only) lines.
        let mut group: Vec<&str> = Vec::new();
        let mut groups: Vec<Vec<&str>> = Vec::new();
        for line in raw.split('\n') {
            if line.trim_matches([' ', '\t']).is_empty() {
                if !group.is_empty() {
                    groups.push(std::mem::take(&mut group));
                }
            } else {
                group.push(line);
            }
        }
        if !group.is_empty() {
            groups.push(group);
        }
        for group in &groups {
            let mut parts: Vec<(String, bool)> = Vec::new();
            for (li, line) in group.iter().enumerate() {
                let last = li + 1 == group.len();
                let stripped = line.trim_start_matches([' ', '\t']);
                // Trailing-break detection, mirroring the escape rule (a
                // backslash hard break needs the backslash immediately
                // before the newline) feeding the newline rule (2+ spaces
                // hard, 1 stripped soft, tab kept soft).
                let (content, hard) = if last {
                    (stripped.trim_end_matches([' ', '\t']), false)
                } else {
                    let spaces = stripped.len() - stripped.trim_end_matches(' ').len();
                    if spaces >= 2 {
                        (stripped.trim_end_matches(' '), true)
                    } else {
                        let no_space = stripped.strip_suffix(' ').unwrap_or(stripped);
                        let was_space = no_space.len() != stripped.len();
                        let backslashes = no_space.len() - no_space.trim_end_matches('\\').len();
                        if !was_space && backslashes % 2 == 1 {
                            (&no_space[..no_space.len() - 1], true)
                        } else {
                            (no_space, false)
                        }
                    }
                };
                parts.push((render_html_line(content), hard));
            }
            let mut joined = String::new();
            for (k, (inner, _)) in parts.iter().enumerate() {
                if k > 0 {
                    joined.push_str(if parts[k - 1].1 { "<br>" } else { " " });
                }
                joined.push_str(inner);
            }
            self.out.push_str(&format!("<p>{joined}</p>"));
        }
        end.min(self.events.len().saturating_sub(1))
    }
}

/// Inline-parse one raw-HTML-block line: the `x` prefix neutralizes every
/// block start (a single line can only ever render one paragraph), then the
/// wrapper and prefix come back off. Single-line inputs cannot recurse into
/// `HtmlBlock`, so this terminates after one level. Trailing whitespace
/// rides outside the sub-parse (pulldown would trim it) and is re-appended
/// verbatim — spaces, tabs and vertical whitespace need no escaping.
fn render_html_line(line: &str) -> String {
    let trimmed = line.trim_end_matches([' ', '\t', '\u{b}', '\u{c}']);
    let tail = &line[trimmed.len()..];
    let doc = format!("x{trimmed}");
    let html = render_markdown_inner(&doc);
    let inner = html
        .strip_prefix("<p>")
        .and_then(|s| s.strip_suffix("</p>"))
        .unwrap_or(html.as_str());
    format!("{}{tail}", inner.strip_prefix('x').unwrap_or(inner))
}

/// Index of the `End(Image)` matching the `Start(Image)` at `start`.
fn matching_image_end(events: &[MdEvent<'_>], start: usize) -> usize {
    let mut depth = 0usize;
    let mut i = start;
    while i < events.len() {
        match &events[i].0 {
            Event::Start(Tag::Image { .. }) => depth += 1,
            Event::End(TagEnd::Image) => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return i;
                }
            }
            _ => {}
        }
        i += 1;
    }
    events.len()
}

/// `renderInlineAsText` (`renderer.py:182-195`): text contributes, nested
/// valid images recurse, soft breaks become newlines, everything else
/// (code, hard breaks, valid-link tags, emphasis) drops. Invalid nested
/// images *and links* literalize with their exact raw suffix; invalid
/// autolinks contribute their literal `<url>`; valid autolink text is
/// `normalizeLinkText`-normalized. Past 500 image levels the deeper
/// subtrees drop (CPython raises `RecursionError` into a 500 there).
fn alt_text(events: &[MdEvent<'_>], from: usize, to: usize, md: &str, depth: usize) -> String {
    /// Per-link context: normalize the text, and/or close a literal `<`.
    struct AltLink {
        normalize_text: bool,
        literal_close: bool,
    }
    let mut alt = String::new();
    let mut links: Vec<AltLink> = Vec::new();
    let mut i = from.min(events.len());
    let to = to.min(events.len());
    while i < to {
        match &events[i].0 {
            Event::Text(text) => {
                if links.last().is_some_and(|link| link.normalize_text) {
                    alt.push_str(&normalize_link_text(text));
                } else {
                    alt.push_str(text);
                }
            }
            Event::SoftBreak => alt.push('\n'),
            Event::Start(Tag::Link {
                link_type,
                dest_url,
                title,
                ..
            }) => {
                let raw = match link_type {
                    LinkType::Email => format!("mailto:{dest_url}"),
                    _ => dest_url.to_string(),
                };
                let normalized = normalize_link(&raw);
                let valid = validate_link(&normalized);
                let auto = matches!(link_type, LinkType::Autolink | LinkType::Email);
                if !valid && !auto {
                    // A failed inline link is literal text, brackets and
                    // all — with the exact raw suffix.
                    let end = matching_link_end(events, i).min(to);
                    let inner = alt_text(events, i + 1, end, md, depth);
                    alt.push('[');
                    alt.push_str(&inner);
                    alt.push_str(&raw_link_suffix(events, i, end, md, 1, dest_url, title));
                    i = end + 1;
                    continue;
                }
                if !valid {
                    alt.push('<');
                }
                links.push(AltLink {
                    normalize_text: valid && auto,
                    literal_close: !valid,
                });
            }
            Event::End(TagEnd::Link) => {
                if let Some(link) = links.pop() {
                    if link.literal_close {
                        alt.push('>');
                    }
                }
            }
            Event::Start(Tag::Image {
                dest_url, title, ..
            }) => {
                let end = matching_image_end(events, i).min(to);
                if depth < 500 {
                    let inner = alt_text(events, i + 1, end, md, depth + 1);
                    let normalized = normalize_link(dest_url);
                    if validate_link(&normalized) {
                        alt.push_str(&inner);
                    } else {
                        alt.push_str("![");
                        alt.push_str(&inner);
                        alt.push_str(&raw_link_suffix(events, i, end, md, 2, dest_url, title));
                    }
                }
                i = end + 1;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    alt
}

/// The inline events of a nested single-paragraph parse, when the parse
/// is exactly one paragraph of inline content (no blocks, rules, or
/// other definitions).
fn single_paragraph_inline<'b, 'a>(nested: &'b [MdEvent<'a>]) -> Option<&'b [MdEvent<'a>]> {
    let [first, rest @ ..] = nested else {
        return None;
    };
    if !matches!(first.0, Event::Start(Tag::Paragraph)) || rest.is_empty() {
        return None;
    }
    let (middle, last) = rest.split_at(rest.len() - 1);
    if !matches!(last[0].0, Event::End(TagEnd::Paragraph)) {
        return None;
    }
    for (event, _) in middle {
        match event {
            Event::Text(_)
            | Event::Code(_)
            | Event::SoftBreak
            | Event::HardBreak
            | Event::InlineHtml(_)
            | Event::End(_) => {}
            Event::Start(tag) => {
                if !matches!(
                    tag,
                    Tag::Emphasis
                        | Tag::Strong
                        | Tag::Strikethrough
                        | Tag::Link { .. }
                        | Tag::Image { .. }
                ) {
                    return None;
                }
            }
            _ => return None,
        }
    }
    Some(middle)
}

/// Index of the `End(Link)` matching the `Start(Link)` at `start`.
fn matching_link_end(events: &[MdEvent<'_>], start: usize) -> usize {
    let mut depth = 0usize;
    let mut i = start;
    while i < events.len() {
        match &events[i].0 {
            Event::Start(Tag::Link { .. }) => depth += 1,
            Event::End(TagEnd::Link) => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return i;
                }
            }
            _ => {}
        }
        i += 1;
    }
    events.len()
}

/// Exact raw `](…)` suffix of an invalid link/image from the source span
/// (falls back to reconstruction only when the spans surprise).
fn raw_link_suffix(
    events: &[MdEvent<'_>],
    start: usize,
    end: usize,
    md: &str,
    prefix_len: usize,
    dest_url: &str,
    title: &str,
) -> String {
    let link_start = events.get(start).map(|e| e.1.start).unwrap_or(0);
    let link_end = events.get(end).map(|e| e.1.end).unwrap_or(0);
    let last = events
        .get(start + 1..end.max(start + 1))
        .and_then(|slice| slice.iter().map(|e| e.1.end).max());
    let from = last.unwrap_or(link_start + prefix_len);
    if let Some(suffix) = md.get(from..link_end) {
        return suffix.to_owned();
    }
    if title.is_empty() {
        format!("]({dest_url})")
    } else {
        format!("]({dest_url} \"{title}\")")
    }
}

/// Parse + resolve + render, without sanitizing (shared by
/// `markdown_to_html` and the raw-HTML-line sub-parse).
fn render_markdown_inner(md: &str) -> String {
    let options = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH;
    let mut events: Vec<MdEvent<'_>> = Parser::new_ext(md, options).into_offset_iter().collect();
    let (item_kinds, task_lists) = resolve_lists(&mut events, md);
    let mut renderer = Renderer {
        events: &events,
        md,
        item_kinds,
        task_lists,
        out: String::new(),
        stack: Vec::new(),
        block_depth: 0,
        inline_only: false,
    };
    renderer.run();
    renderer.out
}

/// `markdown_to_html` (`markdown_converter.py:192-224`): render agent
/// markdown to sanitized Tiptap HTML. Blank input and empty renders yield
/// the empty-document stub; a rejected render errors with the validator's
/// message (the `or "Invalid HTML content"` fallback is dead — the
/// validator always sets a message when it fails).
fn markdown_to_html(markdown: &str) -> Result<String, String> {
    if markdown.trim().is_empty() {
        return Ok("<p></p>".to_owned());
    }
    let html = render_markdown_inner(markdown);
    if html.is_empty() {
        return Ok("<p></p>".to_owned());
    }
    match crate::space::sanitize::sanitize_html(&html) {
        crate::space::sanitize::Sanitize::Clean(clean) => Ok(if clean.is_empty() {
            "<p></p>".to_owned()
        } else {
            clean
        }),
        crate::space::sanitize::Sanitize::Invalid => {
            Err(if html.len() > crate::space::sanitize::MAX_HTML_BYTES {
                "HTML content exceeds maximum size limit (10MB)".to_owned()
            } else {
                "Failed to sanitize HTML".to_owned()
            })
        }
    }
}

#[cfg(test)]
mod converter_tests {
    use super::{html_to_markdown, markdown_to_html};

    #[derive(serde::Deserialize)]
    struct Vector {
        id: String,
        input: Option<String>,
        output: Option<String>,
        error: Option<String>,
    }

    fn load(name: &str) -> Vec<Vector> {
        let text = if name == "html" {
            include_str!("../../../../fixtures/v1_work_items/converters/html_to_markdown.json")
        } else {
            include_str!("../../../../fixtures/v1_work_items/converters/markdown_to_html.json")
        };
        serde_json::from_str(text).expect("vectors parse")
    }

    fn floor_char_boundary(text: &str, max: usize) -> usize {
        let mut end = text.len().min(max);
        while end > 0 && !text.is_char_boundary(end) {
            end -= 1;
        }
        end
    }

    fn short(text: &str) -> String {
        const MAX: usize = 240;
        if text.len() <= MAX {
            text.to_owned()
        } else {
            let end = floor_char_boundary(text, MAX);
            format!("{}…<{} bytes>", &text[..end], text.len())
        }
    }

    #[test]
    fn short_floors_to_char_boundary() {
        // `…` is 3 bytes: byte 240 lands mid-char, which must not panic.
        let text = "a".repeat(239) + "…";
        assert_eq!(floor_char_boundary(&text, 240), 239);
        let clipped = short(&text);
        assert!(clipped.starts_with(&"a".repeat(239)));
        assert!(clipped.ends_with(&format!("<{} bytes>", text.len())));
        assert_eq!(short("tiny"), "tiny");
    }

    #[test]
    fn html_to_markdown_vectors() {
        let mut failures = Vec::new();
        for vector in load("html") {
            let got = html_to_markdown(vector.input.as_deref());
            let want = vector.output.unwrap_or_default();
            if got != want {
                failures.push(format!(
                    "[{}] input={:?} want={:?} got={:?}",
                    vector.id,
                    short(&vector.input.unwrap_or_default()),
                    short(&want),
                    short(&got)
                ));
            }
        }
        assert!(
            failures.is_empty(),
            "{} html_to_markdown mismatches:\n{}",
            failures.len(),
            failures.join("\n")
        );
    }

    #[test]
    fn markdown_to_html_vectors() {
        let mut failures = Vec::new();
        for vector in load("md") {
            let Some(input) = vector.input else {
                // `markdown_to_html(None)`: the `&str` signature cannot
                // express it; the `not markdown` arm is the same path the
                // `empty`/`blank` vectors pin.
                continue;
            };
            match markdown_to_html(&input) {
                Ok(got) => match vector.output {
                    Some(want) if got == want => {}
                    Some(want) => failures.push(format!(
                        "[{}] input={:?} want={:?} got={:?}",
                        vector.id,
                        short(&input),
                        short(&want),
                        short(&got)
                    )),
                    None => failures.push(format!(
                        "[{}] input={:?} want error {:?} got Ok({:?})",
                        vector.id,
                        short(&input),
                        vector.error.unwrap_or_default(),
                        short(&got)
                    )),
                },
                Err(got) => match vector.error {
                    Some(want) if got == want => {}
                    _ => failures.push(format!(
                        "[{}] input={:?} want Ok({:?}) got Err({:?})",
                        vector.id,
                        short(&input),
                        short(&vector.output.unwrap_or_default()),
                        short(&got)
                    )),
                },
            }
        }
        assert!(
            failures.is_empty(),
            "{} markdown_to_html mismatches:\n{}",
            failures.len(),
            failures.join("\n")
        );
    }

    #[test]
    fn markdown_to_html_size_arm() {
        let big = "x".repeat(crate::space::sanitize::MAX_HTML_BYTES + 1);
        assert_eq!(
            markdown_to_html(&big),
            Err("HTML content exceeds maximum size limit (10MB)".to_owned())
        );
    }

    #[test]
    fn markdown_to_html_blank_arms() {
        assert_eq!(markdown_to_html(""), Ok("<p></p>".to_owned()));
        assert_eq!(markdown_to_html("  \n\t "), Ok("<p></p>".to_owned()));
    }

    #[test]
    fn html_to_markdown_empty_arms() {
        assert_eq!(html_to_markdown(None), "");
        assert_eq!(html_to_markdown(Some("")), "");
    }
}
