//! Link + comment handlers (D-18 handlers B, PIDASHCONV-674).
//!
//! Ports `apps/api/pi_dash/api/views/issue.py` (4 units):
//!
//! * `IssueLinkListCreateAPIEndpoint.get` (`:1593-1604`) — paginated link
//!   list, `fields=`/`expand=` from the query string.
//! * `IssueLinkListCreateAPIEndpoint.post` (`:1626-1650`) — create +
//!   `created_by` override + crawl/activity fan-out.
//! * `IssueLinkDetailAPIEndpoint.get` (`:1697-1714`) — one link (the
//!   `pk is None` list branch is unreachable on the wire: the route always
//!   binds `pk`).
//! * `IssueLinkDetailAPIEndpoint.patch` (`:1737-1761`) — Show-serializer
//!   partial update (NOT the Update serializer) + crawl/activity fan-out.
//! * `IssueLinkDetailAPIEndpoint.delete` (`:1775-1793`) — soft-delete +
//!   activity fan-out + related-objects sweep enqueue.
//! * `IssueCommentListCreateAPIEndpoint.get` (`:1851-1862`) — paginated
//!   comment list with the `is_member` annotation.
//! * `IssueCommentListCreateAPIEndpoint.post` (`:1885-1949`) — external
//!   dup guard + create + overrides + activity/webhook fan-out.
//! * `IssueCommentDetailAPIEndpoint.get` (`:2003-2010`) — one comment.
//! * `IssueCommentDetailAPIEndpoint.patch` (`:2034-2089`) — external dup
//!   guard + Create-serializer partial update + fan-out.
//! * `IssueCommentDetailAPIEndpoint.delete` (`:2103-2121`) — soft-delete +
//!   activity fan-out + related-objects sweep enqueue.
//!
//! Registered by [`super::routes`] at the eight
//! `apps/api/pi_dash/api/urls/work_item.py:60-77,154-171` paths (four
//! `work-items/` routes plus their deprecated `issues/` twins, which share
//! the view classes and therefore the handlers).
//!
//! Layering (all foundation use is read-only): shapes in
//! `pidash_services::v1_work_items::shape_links` / `shape_social`,
//! representative SQL in `queries_sub`, POST validation in the shapes,
//! task kwargs in `tasks`, gates in [`super::perms`] over the F-06 kernel
//! (`pidash_auth::permissions`), request bodies through
//! `crate::v1_cycles_modules::{body, json_cpython}`, pagination through
//! `crate::paginator`, task fan-out through `pidash_jobs::queue`. This
//! module owns the HTTP shell: API-key auth, the slug→UUID rewrite,
//! permission wiring, the write statements, the read-shape rendering and
//! the paginated envelope.
//!
//! Request order (preserved, not redesigned): UUID-segment shape (proxy when
//! Django's `<uuid:>` converter would not match — before auth, as URL
//! resolving precedes it), API-key authentication (anonymous 401s before any
//! pool or database access), the slug→UUID rewrite
//! (`api/views/base.py:51-98`, skipped for anonymous callers),
//! `check_permissions`, timezone activation (unknown zones 400 —
//! `ZoneInfoNotFoundError` subclasses `KeyError`), then the handler body.
//! Bodies parse only after the gate: the views touch `request.data` inside
//! the handler, never in `initial()`.
//!
//! Ported bugs (also listed in the PR):
//!
//! * BUG-1 (`views/issue.py:1743,1781,2040,2109`): patch/delete use a direct
//!   `objects.get(slug, project, issue, pk)` that bypasses the queryset's
//!   member/archived guards — any project member (past the gate) can
//!   patch/delete any issue's links/comments.
//! * BUG-2 (`views/issue.py:1746`): link PATCH runs the full Show serializer
//!   with `partial=True`, NOT `IssueLinkUpdateSerializer` — no URL-format
//!   check, no duplicate guard, and `deleted_at` is writable (soft-delete
//!   via PATCH).
//! * BUG-3 (`views/issue.py:1923-1924`): comment create assigns `actor_id`
//!   from the `created_by` override but saves only
//!   `update_fields=["created_at","created_by"]` — the DB keeps the
//!   requester while the response renders the override.
//! * BUG-4 (`views/issue.py:1567,1672,1826,1982`): `.order_by()` reads
//!   `self.kwargs` (URL kwargs — never `order_by`), so `?order_by=` is
//!   ignored and every list orders `-created_at`.
//! * BUG-5 (`views/issue.py:1648,1947,1759,2087`): write responses render
//!   the in-memory instance, so `updated_by` shows the actor while the DB
//!   row keeps NULL (the override saves only its own columns).
//! * BUG-6 (`views/issue.py:1702-1711`): link detail GET keeps a `pk is None`
//!   list branch that no route can reach.
//!
//! Deliberate edges (all unpinned — no fixture or contract case sends them):
//!
//! * Multipart/file inputs are ignored (no file field exists here); Python
//!   would 400 them as non-strings.
//! * `expand=` on rows whose parent (workspace/project/issue/actor) is
//!   soft-deleted renders null; Python follows the default manager and may
//!   404 instead. Same edge as the reviewed sibling
//!   (`handlers_activity.rs`, PIDASHCONV-675).
//! * Extreme-magnitude floats render with serde's exponent spelling where
//!   CPython spells `1e+16` — same accepted edge as every merged shape
//!   module.
//!
//! Fixture: `F18-11` (`rust-api/fixtures/v1_work_items/handlers/` —
//! `link_list`, `link_detail`, `link_create`, `link_patch`,
//! `link_create_bad_url`, `link_delete`, `comment_list`, `comment_detail`,
//! `comment_create`, `comment_patch`, `comment_delete`, plus the deprecated
//! twins).
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

use std::collections::HashMap;

use axum::extract::{OriginalUri, Path, Query, Request, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, TimeZone, Utc};
use chrono_tz::Tz;
use serde_json::{Map, Value};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use pidash_auth::permissions::project;
use pidash_auth::scope::TenantScope;
use pidash_services::v1_work_items::shape_issue::BASE_EXPANSION_NAMES;
use pidash_services::v1_work_items::shape_links::{
    render_link, validate_link_create, LinkRow, LinkShowInput, LinkWriteError,
};
use pidash_services::v1_work_items::shape_social::{
    render_comment, render_comment_create, validate_comment_create,
    CommentCreateRepresentationInput, CommentCreateRow, CommentRepresentationInput, CommentRow,
    CommentWriteInput,
};
use pidash_services::v1_work_items::tasks as work_tasks;
use pidash_services::v1_work_items::{filter_fields, python_number_str, FieldSpec};

use crate::state::AppState;

use super::perms::{decide, gate_for, V1WorkItemsRoute};

// ---------------------------------------------------------------------------
// Responses
// ---------------------------------------------------------------------------

/// `handle_exception`'s `ObjectDoesNotExist` branch
/// (`api/views/base.py:154-158`): every `.get()` miss on these endpoints.
pub const RESOURCE_NOT_FOUND_BODY: &str = r#"{"error":"The requested resource does not exist."}"#;
/// `handle_exception`'s generic branch (`api/views/base.py:166-170`).
pub const SERVER_ERROR_BODY: &str = r#"{"error":"Something went wrong please try again later"}"#;
/// `handle_exception`'s `IntegrityError` branch
/// (`api/views/base.py:142-147`): FK violations on the `created_by`
/// override saves.
pub const PAYLOAD_NOT_VALID_BODY: &str = r#"{"error":"The payload is not valid"}"#;
/// `handle_exception`'s `ValidationError` branch
/// (`api/views/base.py:149-153`): unparseable `created_by`/`created_at`
/// overrides and `deleted_at` input.
pub const VALID_DETAIL_BODY: &str = r#"{"error":"Please provide valid detail"}"#;
/// Comment external-duplicate 409 message (`views/issue.py:1910,2057`).
pub const COMMENT_DUP_MESSAGE: &str =
    "Work item comment with the same external id and external source already exists";
/// Link create duplicate 400 (`serializers/issue.py:617-620`): the guard
/// raises `ValidationError({"error": ...})` out of `save()`, and DRF
/// renders the dict as-is — a STRING value, not the list form that
/// `is_valid()` errors take (verified live). NOTE:
/// `shape_links::duplicate_url_body` renders the list form (it follows the
/// F18-02 exception internals, not the wire body) — do not use it here;
/// its fix is PIDASHCONV-755 so this handler stays byte-identical.
pub const LINK_DUP_BODY: &str = r#"{"error":"URL already exists for this Issue"}"#;
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
    /// 400, `{"error": ...}` (view-inline: unknown timezones).
    BadError(String),
    /// 400, serializer `errors` dict (pre-rendered bytes, field order).
    FieldErrors(String),
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

/// Render a 201 JSON response with exact bytes.
fn json_created(body: String) -> Response {
    json_response(StatusCode::CREATED, body)
}

/// Map a database/driver failure to the generic 500 while logging the site
/// and error for operators (no secrets: messages never include tokens).
fn db_error<E: std::fmt::Display>(error: E, site: &str) -> Denial {
    tracing::warn!(%error, site, "v1_work_items social database failure");
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

/// The link list paths own GET + POST (`urls/work_item.py:60-63`,
/// `:154-157`, `as_view(http_method_names=["get", "post"])`).
pub fn owned_link_list(
    router: axum::routing::MethodRouter<AppState>,
) -> axum::routing::MethodRouter<AppState> {
    owned(router, &["GET", "POST"])
}

/// The link detail paths own GET + PATCH + DELETE
/// (`urls/work_item.py:64-67`, `:158-162`,
/// `as_view(http_method_names=["get", "patch", "delete"])`).
pub fn owned_link_detail(
    router: axum::routing::MethodRouter<AppState>,
) -> axum::routing::MethodRouter<AppState> {
    owned(router, &["GET", "PATCH", "DELETE"])
}

/// The comment list paths own GET + POST (`urls/work_item.py:68-72`,
/// `:163-167`).
pub fn owned_comment_list(
    router: axum::routing::MethodRouter<AppState>,
) -> axum::routing::MethodRouter<AppState> {
    owned(router, &["GET", "POST"])
}

/// The comment detail paths own GET + PATCH + DELETE
/// (`urls/work_item.py:73-77`, `:168-172`).
pub fn owned_comment_detail(
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

/// Fetch the project-permission facts (`app/permissions/project.py`): active
/// project membership plus the Admin/Member role arms, workspace- and
/// project-scoped. Shared by the Entity (links) and Lite (comments) gates;
/// [`decide`] applies the per-route arm.
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

/// Run the route's gate; deny 403 on failure. The link routes carry
/// `ProjectEntityPermission`, the comment routes `ProjectLitePermission`
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
    let facts = entity_facts(pool, workspace_id, workspace_slug, user_id, project_id).await?;
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

/// Link list/detail rows (`:1557-1569`, `:1662-1675`): the recorded F18-07
/// predicates — live rows for this issue/project/slug, an active
/// membership of the caller on the project (no `deleted_at` scope on the
/// join span — fixture-verbatim), a live project. Order is always
/// `-created_at`: `.order_by()` reads `self.kwargs`, which never carries
/// `order_by`, so `?order_by=` is ignored (BUG-4).
async fn fetch_link_rows(
    pool: &PgPool,
    slug: &str,
    project_id: &Uuid,
    issue_id: &Uuid,
    user_id: &Uuid,
) -> Result<Vec<sqlx::postgres::PgRow>, Denial> {
    sqlx::query(
        r#"SELECT DISTINCT l.* FROM "issue_links" l
           WHERE l."deleted_at" IS NULL
             AND l."workspace_id" = (SELECT "id" FROM "workspaces" WHERE "slug" = $1)
             AND l."project_id" = $2 AND l."issue_id" = $3
             AND EXISTS (SELECT 1 FROM "project_members" pm
                         WHERE pm."project_id" = l."project_id" AND pm."member_id" = $4 AND pm."is_active")
             AND EXISTS (SELECT 1 FROM "projects" p
                         WHERE p."id" = l."project_id" AND p."archived_at" IS NULL)
           ORDER BY l."created_at" DESC"#,
    )
    .bind(slug)
    .bind(project_id)
    .bind(issue_id)
    .bind(user_id)
    .fetch_all(pool)
    .await
    .map_err(|error| db_error(error, "link-list"))
}

/// Link detail row (`:1712`): the list chain plus `id=pk`.
async fn fetch_link_detail(
    pool: &PgPool,
    slug: &str,
    project_id: &Uuid,
    issue_id: &Uuid,
    user_id: &Uuid,
    pk: &Uuid,
) -> Result<Option<sqlx::postgres::PgRow>, Denial> {
    sqlx::query(
        r#"SELECT DISTINCT l.* FROM "issue_links" l
           WHERE l."deleted_at" IS NULL
             AND l."workspace_id" = (SELECT "id" FROM "workspaces" WHERE "slug" = $1)
             AND l."project_id" = $2 AND l."issue_id" = $3
             AND EXISTS (SELECT 1 FROM "project_members" pm
                         WHERE pm."project_id" = l."project_id" AND pm."member_id" = $4 AND pm."is_active")
             AND EXISTS (SELECT 1 FROM "projects" p
                         WHERE p."id" = l."project_id" AND p."archived_at" IS NULL)
             AND l."id" = $5
           ORDER BY l."created_at" DESC"#,
    )
    .bind(slug)
    .bind(project_id)
    .bind(issue_id)
    .bind(user_id)
    .bind(pk)
    .fetch_optional(pool)
    .await
    .map_err(|error| db_error(error, "link-detail"))
}

/// Link patch/delete direct lookup (`:1743`, `:1781`): live + slug +
/// project + issue + pk — BUG-1 bypasses the member/archived guards.
async fn fetch_link_direct(
    pool: &PgPool,
    slug: &str,
    project_id: &Uuid,
    issue_id: &Uuid,
    pk: &Uuid,
) -> Result<Option<sqlx::postgres::PgRow>, Denial> {
    sqlx::query(
        r#"SELECT l.* FROM "issue_links" l
           WHERE l."deleted_at" IS NULL
             AND l."workspace_id" = (SELECT "id" FROM "workspaces" WHERE "slug" = $1)
             AND l."project_id" = $2 AND l."issue_id" = $3 AND l."id" = $4"#,
    )
    .bind(slug)
    .bind(project_id)
    .bind(issue_id)
    .bind(pk)
    .fetch_optional(pool)
    .await
    .map_err(|error| db_error(error, "link-direct"))
}

/// Link create duplicate guard (`serializers/issue.py:618`): a live row
/// with this URL on this issue. The manager scope applies, so soft-deleted
/// rows do not block re-adding.
async fn link_url_exists(pool: &PgPool, url: &str, issue_id: &Uuid) -> Result<bool, Denial> {
    sqlx::query_scalar(
        r#"SELECT EXISTS(SELECT 1 FROM "issue_links" WHERE "deleted_at" IS NULL AND "url" = $1 AND "issue_id" = $2)"#,
    )
    .bind(url)
    .bind(issue_id)
    .fetch_one(pool)
    .await
    .map_err(|error| db_error(error, "link-dup"))
}

/// Comment list rows (`:1805-1828`): the link predicates plus the
/// `is_member=Exists(...)` annotation (explicit `ProjectMember.objects`
/// filter — WITH the manager's `deleted_at IS NULL`, unlike the guard).
async fn fetch_comment_rows(
    pool: &PgPool,
    slug: &str,
    project_id: &Uuid,
    issue_id: &Uuid,
    user_id: &Uuid,
) -> Result<Vec<sqlx::postgres::PgRow>, Denial> {
    sqlx::query(
        r#"SELECT DISTINCT c.*, EXISTS (
             SELECT 1 FROM "project_members" pm
             WHERE pm."deleted_at" IS NULL AND pm."is_active"
               AND pm."member_id" = $4 AND pm."project_id" = $2
               AND pm."workspace_id" = (SELECT "id" FROM "workspaces" WHERE "slug" = $1)
           ) AS "is_member"
           FROM "issue_comments" c
           WHERE c."deleted_at" IS NULL
             AND c."workspace_id" = (SELECT "id" FROM "workspaces" WHERE "slug" = $1)
             AND c."project_id" = $2 AND c."issue_id" = $3
             AND EXISTS (SELECT 1 FROM "project_members" pm
                         WHERE pm."project_id" = c."project_id" AND pm."member_id" = $4 AND pm."is_active")
             AND EXISTS (SELECT 1 FROM "projects" p
                         WHERE p."id" = c."project_id" AND p."archived_at" IS NULL)
           ORDER BY c."created_at" DESC"#,
    )
    .bind(slug)
    .bind(project_id)
    .bind(issue_id)
    .bind(user_id)
    .fetch_all(pool)
    .await
    .map_err(|error| db_error(error, "comment-list"))
}

/// Comment detail row (`:2008`): the list chain (annotation included) plus
/// `id=pk`.
async fn fetch_comment_detail(
    pool: &PgPool,
    slug: &str,
    project_id: &Uuid,
    issue_id: &Uuid,
    user_id: &Uuid,
    pk: &Uuid,
) -> Result<Option<sqlx::postgres::PgRow>, Denial> {
    sqlx::query(
        r#"SELECT DISTINCT c.*, EXISTS (
             SELECT 1 FROM "project_members" pm
             WHERE pm."deleted_at" IS NULL AND pm."is_active"
               AND pm."member_id" = $4 AND pm."project_id" = $2
               AND pm."workspace_id" = (SELECT "id" FROM "workspaces" WHERE "slug" = $1)
           ) AS "is_member"
           FROM "issue_comments" c
           WHERE c."deleted_at" IS NULL
             AND c."workspace_id" = (SELECT "id" FROM "workspaces" WHERE "slug" = $1)
             AND c."project_id" = $2 AND c."issue_id" = $3
             AND EXISTS (SELECT 1 FROM "project_members" pm
                         WHERE pm."project_id" = c."project_id" AND pm."member_id" = $4 AND pm."is_active")
             AND EXISTS (SELECT 1 FROM "projects" p
                         WHERE p."id" = c."project_id" AND p."archived_at" IS NULL)
             AND c."id" = $5
           ORDER BY c."created_at" DESC"#,
    )
    .bind(slug)
    .bind(project_id)
    .bind(issue_id)
    .bind(user_id)
    .bind(pk)
    .fetch_optional(pool)
    .await
    .map_err(|error| db_error(error, "comment-detail"))
}

/// Comment patch/delete direct lookup (`:2040`, `:2109`): live + slug +
/// project + issue + pk — BUG-1 bypasses the guards and the annotation.
async fn fetch_comment_direct(
    pool: &PgPool,
    slug: &str,
    project_id: &Uuid,
    issue_id: &Uuid,
    pk: &Uuid,
) -> Result<Option<sqlx::postgres::PgRow>, Denial> {
    sqlx::query(
        r#"SELECT c.* FROM "issue_comments" c
           WHERE c."deleted_at" IS NULL
             AND c."workspace_id" = (SELECT "id" FROM "workspaces" WHERE "slug" = $1)
             AND c."project_id" = $2 AND c."issue_id" = $3 AND c."id" = $4"#,
    )
    .bind(slug)
    .bind(project_id)
    .bind(issue_id)
    .bind(pk)
    .fetch_optional(pool)
    .await
    .map_err(|error| db_error(error, "comment-direct"))
}

/// Comment external-dedupe `exists()` (`:1895-1900`, `:2048-2053`): live +
/// project + slug + source + id. No pk exclusion — the patch caller
/// compares `external_id` in Python first (`:2047`, ported asymmetry 8).
/// A `None` source filters `IS NULL` (the patch's stored-source default
/// may be NULL — Django renders `None` as `IS NULL`).
async fn comment_external_exists(
    pool: &PgPool,
    project_id: &Uuid,
    slug: &str,
    source: Option<&str>,
    external_id: &str,
) -> Result<bool, Denial> {
    sqlx::query_scalar(
        r#"SELECT EXISTS(SELECT 1 FROM "issue_comments" c
           WHERE c."deleted_at" IS NULL AND c."project_id" = $1
             AND c."workspace_id" = (SELECT "id" FROM "workspaces" WHERE "slug" = $2)
             AND c."external_source" IS NOT DISTINCT FROM $3 AND c."external_id" = $4)"#,
    )
    .bind(project_id)
    .bind(slug)
    .bind(source)
    .bind(external_id)
    .fetch_one(pool)
    .await
    .map_err(|error| db_error(error, "comment-dedupe"))
}

/// The create-409 `"id"`: the duplicate row's id
/// (`:1902-1907`, `.first()` over the `-created_at` default ordering).
async fn comment_external_first_id(
    pool: &PgPool,
    project_id: &Uuid,
    slug: &str,
    source: &str,
    external_id: &str,
) -> Result<Option<Uuid>, Denial> {
    sqlx::query_scalar(
        r#"SELECT c."id" FROM "issue_comments" c
           WHERE c."deleted_at" IS NULL AND c."project_id" = $1
             AND c."workspace_id" = (SELECT "id" FROM "workspaces" WHERE "slug" = $2)
             AND c."external_source" = $3 AND c."external_id" = $4
           ORDER BY c."created_at" DESC LIMIT 1"#,
    )
    .bind(project_id)
    .bind(slug)
    .bind(source)
    .bind(external_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| db_error(error, "comment-dup-first"))
}

// ---------------------------------------------------------------------------
// Decode + render
// ---------------------------------------------------------------------------

/// One decoded link row: owned strings the [`LinkRow`] borrows.
struct DecodedLink {
    id: String,
    created_at: String,
    updated_at: String,
    deleted_at: Option<String>,
    title: Option<String>,
    url: String,
    metadata: Value,
    created_by: Option<String>,
    updated_by: Option<String>,
    project: String,
    workspace: String,
    issue: String,
}

fn decode_link(row: &sqlx::postgres::PgRow, tz: &Tz) -> Result<DecodedLink, Denial> {
    let metadata: Value = row.try_get("metadata").map_err(|_| Denial::ServerError)?;
    Ok(DecodedLink {
        id: row_uuid(row, "id")?.to_string(),
        created_at: crate::serializer::render_datetime_in(&row_datetime(row, "created_at")?, tz),
        updated_at: crate::serializer::render_datetime_in(&row_datetime(row, "updated_at")?, tz),
        deleted_at: row_datetime_opt(row, "deleted_at")?
            .map(|dt| crate::serializer::render_datetime_in(&dt, tz)),
        title: row_string_opt(row, "title")?,
        url: row_string(row, "url")?,
        metadata,
        created_by: row_uuid_opt(row, "created_by_id")?.map(|id| id.to_string()),
        updated_by: row_uuid_opt(row, "updated_by_id")?.map(|id| id.to_string()),
        project: row_uuid(row, "project_id")?.to_string(),
        workspace: row_uuid(row, "workspace_id")?.to_string(),
        issue: row_uuid(row, "issue_id")?.to_string(),
    })
}

fn render_link_value(
    decoded: &DecodedLink,
    field_specs: Option<&[FieldSpec]>,
    expand_refs: &[&str],
    expansions: &[(&str, Option<Value>)],
) -> Result<Value, Denial> {
    let row = LinkRow {
        id: &decoded.id,
        created_at: &decoded.created_at,
        updated_at: &decoded.updated_at,
        deleted_at: decoded.deleted_at.as_deref(),
        title: decoded.title.as_deref(),
        url: &decoded.url,
        metadata: &decoded.metadata,
        created_by: decoded.created_by.as_deref(),
        updated_by: decoded.updated_by.as_deref(),
        project: &decoded.project,
        workspace: &decoded.workspace,
        issue: &decoded.issue,
    };
    let out = render_link(&LinkShowInput {
        row: &row,
        fields: field_specs,
        expand: expand_refs,
        expansions,
    })
    .map_err(|_| Denial::ServerError)?;
    Ok(Value::Object(out))
}

/// The link-create activity `requested_data`
/// (`json.dumps(serializer.data)` at `:1641`): the 3-field create read
/// shape (`Meta.fields`, `serializers/issue.py:590`), in field order.
fn link_create_data(title: Option<&str>, url: &str, issue_id: &str) -> Value {
    let mut map = Map::with_capacity(3);
    map.insert(
        "title".to_owned(),
        title
            .map(|text| Value::String(text.to_owned()))
            .unwrap_or(Value::Null),
    );
    map.insert("url".to_owned(), Value::String(url.to_owned()));
    map.insert("issue_id".to_owned(), Value::String(issue_id.to_owned()));
    Value::Object(map)
}

/// One decoded comment row: owned strings the [`CommentRow`] borrows.
struct DecodedComment {
    id: String,
    is_member: Option<bool>,
    created_at: String,
    updated_at: String,
    deleted_at: Option<String>,
    comment_json: Value,
    comment_html: String,
    attachments: Vec<Option<String>>,
    labels: Vec<Option<String>>,
    access: String,
    external_source: Option<String>,
    external_id: Option<String>,
    speaker_type: String,
    speaker_label: String,
    speaker_agent_run_id: Option<String>,
    edited_at: Option<String>,
    created_by: Option<String>,
    updated_by: Option<String>,
    project: String,
    workspace: String,
    description: Option<String>,
    issue: String,
    actor: Option<String>,
    parent: Option<String>,
}

fn decode_comment(row: &sqlx::postgres::PgRow, tz: &Tz) -> Result<DecodedComment, Denial> {
    let attachments: Vec<Option<String>> = row
        .try_get("attachments")
        .map_err(|_| Denial::ServerError)?;
    let labels: Vec<Option<String>> = row.try_get("labels").map_err(|_| Denial::ServerError)?;
    let comment_json: Value = row
        .try_get("comment_json")
        .map_err(|_| Denial::ServerError)?;
    let is_member: Option<bool> = row.try_get("is_member").unwrap_or(None);
    Ok(DecodedComment {
        id: row_uuid(row, "id")?.to_string(),
        is_member,
        created_at: crate::serializer::render_datetime_in(&row_datetime(row, "created_at")?, tz),
        updated_at: crate::serializer::render_datetime_in(&row_datetime(row, "updated_at")?, tz),
        deleted_at: row_datetime_opt(row, "deleted_at")?
            .map(|dt| crate::serializer::render_datetime_in(&dt, tz)),
        comment_json,
        comment_html: row_string(row, "comment_html")?,
        attachments,
        labels,
        access: row_string(row, "access")?,
        external_source: row_string_opt(row, "external_source")?,
        external_id: row_string_opt(row, "external_id")?,
        speaker_type: row_string(row, "speaker_type")?,
        speaker_label: row_string(row, "speaker_label")?,
        speaker_agent_run_id: row_uuid_opt(row, "speaker_agent_run_id")?.map(|id| id.to_string()),
        edited_at: row_datetime_opt(row, "edited_at")?
            .map(|dt| crate::serializer::render_datetime_in(&dt, tz)),
        created_by: row_uuid_opt(row, "created_by_id")?.map(|id| id.to_string()),
        updated_by: row_uuid_opt(row, "updated_by_id")?.map(|id| id.to_string()),
        project: row_uuid(row, "project_id")?.to_string(),
        workspace: row_uuid(row, "workspace_id")?.to_string(),
        description: row_uuid_opt(row, "description_id")?.map(|id| id.to_string()),
        issue: row_uuid(row, "issue_id")?.to_string(),
        actor: row_uuid_opt(row, "actor_id")?.map(|id| id.to_string()),
        parent: row_uuid_opt(row, "parent_id")?.map(|id| id.to_string()),
    })
}

fn render_comment_value(
    decoded: &DecodedComment,
    url: Option<String>,
    field_specs: Option<&[FieldSpec]>,
    expand_refs: &[&str],
    expansions: &[(&str, Option<Value>)],
) -> Result<Value, Denial> {
    let attachments: Vec<Option<&str>> = decoded
        .attachments
        .iter()
        .map(|item| item.as_deref())
        .collect();
    let labels: Vec<Option<&str>> = decoded.labels.iter().map(|item| item.as_deref()).collect();
    let row = CommentRow {
        id: &decoded.id,
        is_member: decoded.is_member,
        url,
        created_at: &decoded.created_at,
        updated_at: &decoded.updated_at,
        deleted_at: decoded.deleted_at.as_deref(),
        comment_html: &decoded.comment_html,
        attachments: &attachments,
        labels: &labels,
        access: &decoded.access,
        external_source: decoded.external_source.as_deref(),
        external_id: decoded.external_id.as_deref(),
        speaker_type: &decoded.speaker_type,
        speaker_label: &decoded.speaker_label,
        speaker_agent_run_id: decoded.speaker_agent_run_id.as_deref(),
        edited_at: decoded.edited_at.as_deref(),
        created_by: decoded.created_by.as_deref(),
        updated_by: decoded.updated_by.as_deref(),
        project: &decoded.project,
        workspace: &decoded.workspace,
        description: decoded.description.as_deref(),
        issue: &decoded.issue,
        actor: decoded.actor.as_deref(),
        parent: decoded.parent.as_deref(),
    };
    let out = render_comment(&CommentRepresentationInput {
        row: &row,
        fields: field_specs,
        expand: expand_refs,
        expansions,
    })
    .map_err(|_| Denial::ServerError)?;
    Ok(Value::Object(out))
}

/// The comment-create activity `requested_data`
/// (`json.dumps(serializer.data)` at `:1928`): the 9-field create read
/// shape over the saved row.
fn render_comment_create_value(decoded: &DecodedComment) -> Result<Value, Denial> {
    let labels: Vec<Option<&str>> = decoded.labels.iter().map(|item| item.as_deref()).collect();
    let row = CommentCreateRow {
        comment_json: &decoded.comment_json,
        comment_html: &decoded.comment_html,
        access: &decoded.access,
        external_source: decoded.external_source.as_deref(),
        external_id: decoded.external_id.as_deref(),
        labels: &labels,
        speaker_type: &decoded.speaker_type,
        speaker_label: &decoded.speaker_label,
        speaker_agent_run_id: decoded.speaker_agent_run_id.as_deref(),
    };
    let out = render_comment_create(&CommentCreateRepresentationInput {
        row: &row,
        fields: None,
        expand: &[],
        expansions: &[],
    })
    .map_err(|_| Denial::ServerError)?;
    Ok(Value::Object(out))
}

/// `IssueCommentSerializer.get_url` (`serializers/issue.py:994-999`):
/// `issue_web_url(workspace.slug, project.identifier, issue.sequence_id)`
/// over the ROW's FKs (fetched, never the path params).
async fn comment_url(
    pool: &PgPool,
    workspace_id: &Uuid,
    project_id: &Uuid,
    issue_id: &Uuid,
    web_base: Option<&str>,
) -> Result<Option<String>, Denial> {
    let slug: Option<String> =
        sqlx::query_scalar(r#"SELECT "slug" FROM "workspaces" WHERE "id" = $1"#)
            .bind(workspace_id)
            .fetch_optional(pool)
            .await
            .map_err(|error| db_error(error, "comment-url-slug"))?
            .flatten();
    let identifier: Option<String> =
        sqlx::query_scalar(r#"SELECT "identifier" FROM "projects" WHERE "id" = $1"#)
            .bind(project_id)
            .fetch_optional(pool)
            .await
            .map_err(|error| db_error(error, "comment-url-ident"))?
            .flatten();
    let sequence: Option<i32> =
        sqlx::query_scalar(r#"SELECT "sequence_id" FROM "issues" WHERE "id" = $1"#)
            .bind(issue_id)
            .fetch_optional(pool)
            .await
            .map_err(|error| db_error(error, "comment-url-seq"))?
            .flatten();
    Ok(pidash_services::v1_work_items::shape_issue::issue_url(
        web_base,
        slug.as_deref(),
        identifier.as_deref(),
        sequence.map(i64::from),
    ))
}

// ---------------------------------------------------------------------------
// Expansion
// ---------------------------------------------------------------------------

/// Decoded `file_assets` row for [`file_asset_url`].
type AssetUrlLookup = (Option<String>, Option<Uuid>, Option<Uuid>, Option<Uuid>);

/// `FileAsset.asset_url` (`db/models/asset.py:80-100`) for an asset id:
/// static types render `/api/assets/v2/static/<id>/`, attachments and
/// description assets join their workspace slug. A missing row (or slug)
/// renders null like `getattr` on a dead FK (the `v1_projects`
/// `file_asset_url` precedent).
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

/// `expand=parent`: `IssueLiteSerializer` over the parent COMMENT
/// (`serializers/issue.py:505-508`): `id`, `sequence_id` (skipped —
/// comments have none), `project_id`, in field order. No live scope:
/// Django renders soft-deleted parents (verified live). A hard-missing
/// row is unreachable (`CASCADE`) and renders `{}` via `None`.
async fn expand_parent_comment(pool: &PgPool, parent_id: &Uuid) -> Result<Option<Value>, Denial> {
    let row: Option<(Uuid, Uuid)> =
        sqlx::query_as(r#"SELECT "id", "project_id" FROM "issue_comments" WHERE "id" = $1"#)
            .bind(parent_id)
            .fetch_optional(pool)
            .await
            .map_err(|error| db_error(error, "expand-parent"))?;
    match row {
        Some((id, project_id)) => {
            let mut map = Map::with_capacity(2);
            map.insert("id".to_owned(), Value::String(id.to_string()));
            map.insert(
                "project_id".to_owned(),
                Value::String(project_id.to_string()),
            );
            Ok(Some(Value::Object(map)))
        }
        None => Ok(None),
    }
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
    let row: Option<sqlx::postgres::PgRow> =
        sqlx::query(r#"SELECT * FROM "issues" WHERE "id" = $1 AND "deleted_at" IS NULL"#)
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
/// (`SUMMARY_LIMIT`), plus the uncapped `has_open_blockers`.
async fn fetch_blocker_rows(pool: &PgPool, issue_id: &Uuid) -> Result<BlockerRows, Denial> {
    async fn direction(
        pool: &PgPool,
        issue_id: &Uuid,
        blocked_by: bool,
    ) -> Result<Vec<(String, Option<String>, Option<String>)>, Denial> {
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

/// Caller expansion values for one link row: every `expand` name in the
/// kept fields with a map hit (`project`, `workspace`, `issue`).
/// `IssueLink` carries no `actor` field. Null FKs supply `None` (the shape
/// renders `{}`); dead rows render null.
async fn link_expansions<'a>(
    pool: &PgPool,
    decoded: &DecodedLink,
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
                let id = decoded
                    .project
                    .parse::<Uuid>()
                    .map_err(|_| Denial::ServerError)?;
                Some(expand_project(pool, &id).await?)
            }
            "workspace" => {
                let id = decoded
                    .workspace
                    .parse::<Uuid>()
                    .map_err(|_| Denial::ServerError)?;
                Some(expand_workspace(pool, &id).await?)
            }
            "issue" => {
                let id = decoded
                    .issue
                    .parse::<Uuid>()
                    .map_err(|_| Denial::ServerError)?;
                Some(expand_issue(pool, slug, &id, tz, web_base).await?)
            }
            // `created_by`/`updated_by` map to `UserLiteSerializer`
            // (`serializers/base.py:95-96`); null FKs supply `None` (the
            // shape renders `{}`).
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
            _ => continue,
        };
        expansions.push((*name, value));
    }
    Ok(expansions)
}

/// Caller expansion values for one comment row: every `expand` name in the
/// kept fields with a map hit (`project`, `workspace`, `issue`, `actor`,
/// `created_by`, `updated_by`, `parent`). Null FKs supply `None` (the
/// shape renders `{}`); dead rows render null. Anything else is the
/// shape's passthrough/null rule.
async fn comment_expansions<'a>(
    pool: &PgPool,
    decoded: &DecodedComment,
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
                let id = decoded
                    .project
                    .parse::<Uuid>()
                    .map_err(|_| Denial::ServerError)?;
                Some(expand_project(pool, &id).await?)
            }
            "workspace" => {
                let id = decoded
                    .workspace
                    .parse::<Uuid>()
                    .map_err(|_| Denial::ServerError)?;
                Some(expand_workspace(pool, &id).await?)
            }
            "issue" => {
                let id = decoded
                    .issue
                    .parse::<Uuid>()
                    .map_err(|_| Denial::ServerError)?;
                Some(expand_issue(pool, slug, &id, tz, web_base).await?)
            }
            "actor" => match decoded.actor.as_deref() {
                Some(raw) => {
                    let id = raw.parse::<Uuid>().map_err(|_| Denial::ServerError)?;
                    Some(expand_actor(pool, &id).await?)
                }
                None => None,
            },
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
            // `parent` maps to `IssueLiteSerializer` over the parent
            // COMMENT (`base.py:104`): `id`/`sequence_id`/`project_id`,
            // with `sequence_id` skipped (comments have none). Null
            // parents supply `None` (the shape renders `{}`).
            "parent" => match decoded.parent.as_deref() {
                Some(raw) => {
                    let id = raw.parse::<Uuid>().map_err(|_| Denial::ServerError)?;
                    expand_parent_comment(pool, &id).await?
                }
                None => None,
            },
            _ => continue,
        };
        expansions.push((*name, value));
    }
    Ok(expansions)
}

// ---------------------------------------------------------------------------
// Request bodies
// ---------------------------------------------------------------------------

/// The write body spec: `labels` arrives as an array (DRF
/// `ListField.get_value` → `getlist`); no blank-skipping — a present-empty
/// form value stays `""` and each endpoint's validation applies the
/// `get_value` rule (`fields.py:407-429`) for its own fields.
const WRITE_BODY_SPEC: crate::v1_cycles_modules::body::BodySpec =
    crate::v1_cycles_modules::body::BodySpec {
        list_fields: &["labels"],
        skip_blank_fields: &[],
    };

/// A parsed write body: the JSON value plus whether it arrived as an HTML
/// form (JSON-string fields and blank arms differ per
/// `JSONField.get_value` / `Field.get_value`).
struct WriteBody {
    value: Value,
    from_form: bool,
}

/// Parse a link/comment write body: content-type dispatch (415 for the
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

/// The non-object body for the link PATCH path: DRF answers the same
/// `non_field_errors` shape as the create serializer, so reuse its exact
/// bytes (null → `No data provided`, else the JSON type name).
fn patch_not_a_dict_body(value: &Value) -> String {
    match validate_link_create(value) {
        Err(LinkWriteError::NotADict(body)) => body,
        // `value` is never an object here; the create rules never run.
        _ => unreachable!("patch non-object body always takes the NotADict arm"),
    }
}

// ---------------------------------------------------------------------------
// Link PATCH validation (Show serializer, partial)
// ---------------------------------------------------------------------------

/// Validated link PATCH writes: `None` = key absent (untouched).
#[derive(Debug)]
struct ValidatedLinkPatch {
    title: Option<Option<String>>,
    url: Option<String>,
    metadata: Option<Value>,
    deleted_at: Option<Option<DateTime<Utc>>>,
}

/// DRF `CharField` failure message, in check order (blank → null →
/// invalid → max_length → null-characters; `fields.py` `CharField` +
/// `ProhibitNullCharactersValidator`). `max_length` is `None` for the
/// `TextField`-backed `url`.
fn char_field_errors(
    value: &Value,
    allow_blank: bool,
    allow_null: bool,
    max_length: Option<usize>,
) -> Vec<String> {
    // `run_validation` strips before the blank check
    // (`data == '' or (trim_whitespace and str(data).strip() == '')`).
    if let Value::String(text) = value {
        if (text.is_empty() || crate::runner_runs::runs::py_strip(text).is_empty()) && !allow_blank
        {
            return vec!["This field may not be blank.".to_owned()];
        }
    } else if value.is_null() {
        if !allow_null {
            return vec!["This field may not be null.".to_owned()];
        }
        return Vec::new();
    }
    match value {
        Value::String(text) => {
            let stripped = crate::runner_runs::runs::py_strip(text);
            // `run_validators` extends ONE list across all validators, in
            // append order: `MaxLengthValidator` before
            // `ProhibitNullCharactersValidator`.
            let mut out = Vec::new();
            if let Some(max) = max_length {
                // `MaxLengthValidator`: `len()` counts code points.
                if stripped.chars().count() > max {
                    out.push(format!(
                        "Ensure this field has no more than {max} characters."
                    ));
                }
            }
            if stripped.contains('\0') {
                out.push("Null characters are not allowed.".to_owned());
            }
            out
        }
        Value::Bool(_) => vec!["Not a valid string.".to_owned()],
        Value::Number(number) => {
            let text = python_number_str(number);
            if let Some(max) = max_length {
                if text.chars().count() > max {
                    return vec![format!(
                        "Ensure this field has no more than {max} characters."
                    )];
                }
            }
            Vec::new()
        }
        _ => vec!["Not a valid string.".to_owned()],
    }
}

/// Coerce a validated string input (`str(data)` + strip; numbers stringify).
fn char_field_value(value: &Value) -> String {
    match value {
        Value::String(text) => crate::runner_runs::runs::py_strip(text).to_owned(),
        Value::Number(number) => python_number_str(number),
        _ => String::new(),
    }
}

/// Port of `IssueLinkSerializer` partial validation
/// (`serializers/issue.py:649-669`, BUG-2): writable keys are `deleted_at`
/// (nullable datetime), `title` (255-char, blank/null ok), `url` (non-blank
/// text, NO `validate_url`), `metadata` (any JSON, non-null). Read-only and
/// unknown keys are silently ignored. Errors combine in model-field order
/// (`deleted_at`, `title`, `url`, `metadata`). No `validate()` override and
/// no duplicate guard run here.
fn validate_link_patch(
    map: &Map<String, Value>,
    from_form: bool,
    tz: &Tz,
) -> Result<ValidatedLinkPatch, String> {
    let mut errors: Vec<(String, String)> = Vec::new();
    let mut title: Option<Option<String>> = None;
    let mut url: Option<String> = None;
    let mut metadata: Option<Value> = None;
    let mut deleted_at: Option<Option<DateTime<Utc>>> = None;
    // `deleted_at`: `DateTimeField(null=True)` — `None` input clears.
    if let Some(raw) = map.get("deleted_at") {
        let raw = if from_form && raw == &Value::String(String::new()) {
            // `get_value`: blank + `allow_null`, no `allow_blank` → None.
            &Value::Null
        } else {
            raw
        };
        if raw.is_null() {
            deleted_at = Some(None);
        } else if let Value::String(text) = raw {
            match parse_django_datetime(text, tz) {
                Ok(dt) => deleted_at = Some(Some(dt)),
                Err(ParseDatetimeError::Invalid) => errors.push((
                    "deleted_at".to_owned(),
                    "Datetime has wrong format. Use one of these formats instead: YYYY-MM-DDThh:mm[:ss[.uuuuuu]][+HH:MM|-HH:MM|Z]."
                        .to_owned(),
                )),
                Err(ParseDatetimeError::Nonexistent) => errors.push((
                    "deleted_at".to_owned(),
                    format!("Invalid datetime for the timezone \"{tz}\"."),
                )),
            }
        } else {
            errors.push((
                "deleted_at".to_owned(),
                "Datetime has wrong format. Use one of these formats instead: YYYY-MM-DDThh:mm[:ss[.uuuuuu]][+HH:MM|-HH:MM|Z]."
                    .to_owned(),
            ));
        }
    }
    // `title`: `CharField(max_length=255, null=True, blank=True)`.
    if let Some(raw) = map.get("title") {
        if raw.is_null() {
            title = Some(None);
        } else {
            let field_errors = char_field_errors(raw, true, true, Some(255));
            if field_errors.is_empty() {
                title = Some(Some(char_field_value(raw)));
            } else {
                for message in field_errors {
                    errors.push(("title".to_owned(), message));
                }
            }
        }
    }
    // `url`: `TextField()` — non-blank, non-null, unbounded, NO format
    // check on this path (BUG-2). `get_value` uses the STATIC `required`
    // flag (`partial` does not change it), so a blank form value still
    // 400s (verified live).
    if let Some(raw) = map.get("url") {
        let field_errors = char_field_errors(raw, false, false, None);
        if field_errors.is_empty() {
            url = Some(char_field_value(raw));
        } else {
            for message in field_errors {
                errors.push(("url".to_owned(), message));
            }
        }
    }
    // `metadata`: `JSONField(default=dict)` — any JSON value, non-null.
    if let Some(raw) = map.get("metadata") {
        let parsed: Option<Value>;
        let raw = if from_form {
            // `JSONField.get_value`: HTML input arrives as a JSON string.
            match raw {
                Value::String(text) => match serde_json::from_str(text) {
                    Ok(value) => {
                        parsed = Some(value);
                        parsed.as_ref().expect("parsed")
                    }
                    Err(_) => {
                        errors.push((
                            "metadata".to_owned(),
                            "Value must be valid JSON.".to_owned(),
                        ));
                        &Value::Null
                    }
                },
                other => other,
            }
        } else {
            raw
        };
        if errors.last().map(|(field, _)| field.as_str()) == Some("metadata") {
            // Parse failure already recorded; skip the null arm.
        } else if raw.is_null() {
            errors.push((
                "metadata".to_owned(),
                "This field may not be null.".to_owned(),
            ));
        } else {
            metadata = Some(raw.clone());
        }
    }
    if errors.is_empty() {
        Ok(ValidatedLinkPatch {
            title,
            url,
            metadata,
            deleted_at,
        })
    } else {
        Err(field_errors_body(&errors))
    }
}

/// Render `{"field": ["message", ...]}` in first-seen field order.
fn field_errors_body(errors: &[(String, String)]) -> String {
    let mut map: Map<String, Value> = Map::new();
    for (field, message) in errors {
        map.entry(field.clone())
            .or_insert_with(|| Value::Array(Vec::new()))
            .as_array_mut()
            .expect("error list")
            .push(Value::String(message.clone()));
    }
    serde_json::to_string(&map).expect("error body")
}

/// Django `parse_datetime` (`django/utils/dateparse.py`) over the default
/// `DATETIME_INPUT_FORMATS` (`iso-8601`): date-only, `T`/space separator,
/// optional seconds/fraction, `Z`/offset. Naive values attach the request
/// time zone (`enforce_timezone` under `USE_TZ`); aware values convert to
/// UTC. Returns the instant.
#[derive(Debug, PartialEq, Eq)]
enum ParseDatetimeError {
    Invalid,
    Nonexistent,
}

fn parse_django_datetime(text: &str, tz: &Tz) -> Result<DateTime<Utc>, ParseDatetimeError> {
    use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
    let text = text.trim();
    if text.is_empty() || text.len() > 100 {
        return Err(ParseDatetimeError::Invalid);
    }
    // Split a trailing zone designator (`Z` or `±HH[:]MM`).
    let (core, offset_seconds): (&str, Option<i32>) = if text.ends_with(['Z', 'z']) {
        (&text[..text.len() - 1], Some(0))
    } else {
        match text.rfind(['+', '-']) {
            Some(idx) if idx > 7 => {
                let (head, tail) = text.split_at(idx);
                let sign = if tail.starts_with('+') { 1 } else { -1 };
                let digits: String = tail[1..].chars().filter(|c| *c != ':').collect();
                if digits.len() == 2 || digits.len() == 4 {
                    let hours: i32 = digits[..2]
                        .parse()
                        .map_err(|_| ParseDatetimeError::Invalid)?;
                    let minutes: i32 = if digits.len() == 4 {
                        digits[2..]
                            .parse()
                            .map_err(|_| ParseDatetimeError::Invalid)?
                    } else {
                        0
                    };
                    if hours > 23 || minutes > 59 {
                        return Err(ParseDatetimeError::Invalid);
                    }
                    (head, Some(sign * (hours * 3600 + minutes * 60)))
                } else {
                    return Err(ParseDatetimeError::Invalid);
                }
            }
            _ => (text, None),
        }
    };
    // Date + optional time (`T` or space).
    let (date_part, time_part) = match core.find(['T', 't', ' ']) {
        Some(idx) => (&core[..idx], Some(&core[idx + 1..])),
        None => (core, None),
    };
    let date = NaiveDate::parse_from_str(date_part, "%Y-%m-%d")
        .map_err(|_| ParseDatetimeError::Invalid)?;
    let time = match time_part {
        None => NaiveTime::from_hms_opt(0, 0, 0).expect("midnight"),
        Some(part) => {
            let part = part.trim();
            NaiveTime::parse_from_str(part, "%H:%M:%S%.f")
                .or_else(|_| NaiveTime::parse_from_str(part, "%H:%M:%S"))
                .or_else(|_| NaiveTime::parse_from_str(part, "%H:%M"))
                .map_err(|_| ParseDatetimeError::Invalid)?
        }
    };
    let naive = NaiveDateTime::new(date, time);
    match offset_seconds {
        Some(offset) => {
            let utc = trunc_micros(naive.and_utc() - chrono::Duration::seconds(i64::from(offset)));
            Ok(utc)
        }
        None => match tz.from_local_datetime(&naive) {
            chrono::LocalResult::Single(aware) => Ok(trunc_micros(aware.with_timezone(&Utc))),
            chrono::LocalResult::Ambiguous(early, _) => Ok(trunc_micros(early.with_timezone(&Utc))),
            chrono::LocalResult::None => Err(ParseDatetimeError::Nonexistent),
        },
    }
}

// ---------------------------------------------------------------------------
// Pagination
// ---------------------------------------------------------------------------

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

/// `GET .../links/` (`views/issue.py:1593-1604`).
pub async fn get_link_list(
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
    match link_list_inner(&state, &headers, &slug, &project_id, &issue_id, &query).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

async fn link_list_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    project_id_raw: &str,
    issue_id: &Uuid,
    query: &QueryMap,
) -> Result<Response, Denial> {
    use pidash_services::v1_work_items::shape_links::LINK_SHOW_FIELDS;
    let pre = preamble(state, headers, slug).await?;
    let project_id = rewrite_project_id(&pre.pool, slug, project_id_raw).await?;
    let workspace_id = pre.workspace_id.ok_or(Denial::Forbidden)?;
    // Old (`issues/`) and new (`work-items/`) twins share the view class
    // and gate; either route variant decides identically.
    require_gate(
        &pre.pool,
        &workspace_id,
        slug,
        &pre.actor.id,
        &project_id,
        V1WorkItemsRoute::LinkList,
        "GET",
    )
    .await?;
    let tz = activate_timezone(pre.actor.timezone.as_deref())?;
    let per_page =
        crate::paginator::parse_per_page(query_last(query, "per_page").as_deref(), 1000, 1000)
            .map_err(page_denial)?;
    let cursor_raw = query_last(query, "cursor").unwrap_or_else(|| format!("{per_page}:0:0"));
    let cursor = crate::paginator::Cursor::from_string(&cursor_raw).map_err(page_denial)?;
    let rows = fetch_link_rows(&pre.pool, slug, &project_id, issue_id, &pre.actor.id).await?;
    let total_count = rows.len() as i64;
    let (page_rows, has_more) = window_rows(&rows, per_page, &cursor)?;
    let (field_specs, kept, expand) = field_selection(query, LINK_SHOW_FIELDS)?;
    let expand_refs: Vec<&str> = expand.iter().map(String::as_str).collect();
    let web_base = pidash_services::v1_work_items::shape_issue::web_base_url(
        state.settings().urls.web_url.as_deref(),
        state.settings().urls.app_base_url.as_deref(),
    );
    let mut rendered: Vec<Value> = Vec::with_capacity(page_rows.len());
    for row in page_rows {
        let decoded = decode_link(row, &tz)?;
        let expansions = link_expansions(
            &pre.pool,
            &decoded,
            slug,
            &tz,
            web_base.as_deref(),
            &expand_refs,
            &kept,
        )
        .await?;
        rendered.push(render_link_value(
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

/// `POST .../links/` (`views/issue.py:1626-1650`).
pub async fn post_link(
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
    let raw = match read_body(body).await {
        Ok(raw) => raw,
        Err(denial) => return denial.into_response(),
    };
    match link_post_inner(&state, &headers, &slug, &project_id, &issue_id, &raw).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

async fn link_post_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    project_id_raw: &str,
    issue_id: &Uuid,
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
        V1WorkItemsRoute::LinkList,
        "POST",
    )
    .await?;
    let tz = activate_timezone(pre.actor.timezone.as_deref())?;
    let parsed = parse_write_body(headers, raw_body)?;
    let validated = validate_link_create(&parsed.value)
        .map_err(|error| Denial::FieldErrors(error.body().to_owned()))?;
    if link_url_exists(&pre.pool, &validated.url, issue_id).await? {
        // `create()` raises `{"error": ...}` out of `save()` — DRF renders
        // the dict as-is (STRING value) with the 400
        // (`serializers/issue.py:617-620`, verified live).
        return Err(Denial::FieldErrors(LINK_DUP_BODY.to_owned()));
    }
    // `ProjectBaseModel.save` sets the workspace from the project.
    let link_workspace: Option<Uuid> =
        sqlx::query_scalar(r#"SELECT "workspace_id" FROM "projects" WHERE "id" = $1"#)
            .bind(project_id)
            .fetch_optional(&pre.pool)
            .await
            .map_err(|error| db_error(error, "link-post-workspace"))?
            .flatten();
    let Some(link_workspace) = link_workspace else {
        // Unreachable past the gate (membership implies the project), but
        // Python would `IntegrityError` the create here.
        return Err(Denial::FieldErrors(PAYLOAD_NOT_VALID_BODY.to_owned()));
    };
    let link_id = Uuid::new_v4();
    // Two `now()` calls like the two `auto_now`/`auto_now_add` pre_saves.
    let created_at = now_utc();
    let updated_at = now_utc();
    let title = validated.title.unwrap_or(None);
    sqlx::query(
        r#"INSERT INTO "issue_links" ("id", "created_at", "updated_at", "created_by_id", "updated_by_id",
            "project_id", "workspace_id", "issue_id", "title", "url", "metadata")
           VALUES ($1, $2, $3, $4, NULL, $5, $6, $7, $8, $9, $10)"#,
    )
    .bind(link_id)
    .bind(created_at)
    .bind(updated_at)
    .bind(pre.actor.id)
    .bind(project_id)
    .bind(link_workspace)
    .bind(issue_id)
    .bind(title.as_deref())
    .bind(&validated.url)
    .bind(serde_json::json!({}))
    .execute(&pre.pool)
    .await
    .map_err(|error| {
        // No issue lookup precedes the create: a bogus `issue_id`
        // violates the FK, and `handle_exception` answers the
        // `IntegrityError` 400 (`api/views/base.py:142-147`).
        if is_fk_violation(&error) {
            Denial::FieldErrors(PAYLOAD_NOT_VALID_BODY.to_owned())
        } else {
            db_error(error, "link-post-insert")
        }
    })?;
    // The `created_by` override (`:1637-1638`): `data.get("created_by",
    // user.id)`, saved with `update_fields=["created_by"]` — `updated_at`
    // is NOT re-stamped (Django skips `auto_now` outside `update_fields`).
    let created_by = override_user_id(parsed.value.get("created_by"), &pre.actor.id)?;
    if let Err(error) =
        sqlx::query(r#"UPDATE "issue_links" SET "created_by_id" = $1 WHERE "id" = $2"#)
            .bind(created_by)
            .bind(link_id)
            .execute(&pre.pool)
            .await
    {
        if is_fk_violation(&error) {
            return Err(Denial::FieldErrors(PAYLOAD_NOT_VALID_BODY.to_owned()));
        }
        return Err(db_error(error, "link-post-override"));
    }
    // Fan-out in source order: crawl (`:1635`), then activity (`:1639`).
    let crawl_args = work_tasks::crawl_link_title_args(&link_id.to_string(), &validated.url);
    enqueue_best_effort(
        &pre.pool,
        work_tasks::CRAWL_LINK_TITLE_TASK,
        crawl_args,
        Map::new(),
    )
    .await;
    let requested = pidash_jobs::tasks_webhooks::activity_dispatch::django_dumps(
        &link_create_data(title.as_deref(), &validated.url, &issue_id.to_string()),
    );
    // `actor_id=str(link.created_by_id)` — `"None"` when the override
    // nulled it (port the wart).
    let actor_text = created_by
        .map(|id| id.to_string())
        .unwrap_or_else(|| "None".to_owned());
    let kwargs = work_tasks::link_create_activity_kwargs(
        &requested,
        &issue_id.to_string(),
        &project_id.to_string(),
        &actor_text,
        Utc::now().timestamp(),
    );
    enqueue_best_effort(&pre.pool, work_tasks::ISSUE_ACTIVITY_TASK, vec![], kwargs).await;
    // The response renders the in-memory instance (BUG-5): override
    // `created_by`, in-memory `updated_by`, first-save timestamps.
    let decoded = DecodedLink {
        id: link_id.to_string(),
        created_at: crate::serializer::render_datetime_in(&created_at, &tz),
        updated_at: crate::serializer::render_datetime_in(&updated_at, &tz),
        deleted_at: None,
        title,
        url: validated.url,
        metadata: serde_json::json!({}),
        created_by: created_by.map(|id| id.to_string()),
        updated_by: Some(pre.actor.id.to_string()),
        project: project_id.to_string(),
        workspace: link_workspace.to_string(),
        issue: issue_id.to_string(),
    };
    let body = render_link_value(&decoded, None, &[], &[])?;
    Ok(json_created(
        serde_json::to_string(&body).map_err(|_| Denial::ServerError)?,
    ))
}

/// The `request.data.get("created_by", user.id)` override: absent → the
/// actor, explicit null → NULL (clears), a UUID string → that user.
/// `UUIDField.to_python` coerces ints AND bools via `uuid.UUID(int=…)` —
/// in-range values proceed to the save (usually the FK-violation 400);
/// negative ints fail with `ValueError`. Floats and anything else fail
/// `get_prep_value` → the `ValidationError` 400 (Django wraps the
/// `UUID(hex=…)` `AttributeError`). All verified live.
fn override_user_id(raw: Option<&Value>, actor: &Uuid) -> Result<Option<Uuid>, Denial> {
    match raw {
        None => Ok(Some(*actor)),
        Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => text
            .parse::<Uuid>()
            .map(Some)
            .map_err(|_| Denial::FieldErrors(VALID_DETAIL_BODY.to_owned())),
        Some(Value::Bool(flag)) => Ok(Some(Uuid::from_u128(u128::from(*flag as u8)))),
        Some(Value::Number(number)) => {
            if let Some(int) = number.as_u64() {
                Ok(Some(Uuid::from_u128(u128::from(int))))
            } else {
                Err(Denial::FieldErrors(VALID_DETAIL_BODY.to_owned()))
            }
        }
        Some(_) => Err(Denial::FieldErrors(VALID_DETAIL_BODY.to_owned())),
    }
}

/// Whether a sqlx failure is a foreign-key violation (`IntegrityError` →
/// `{"error": "The payload is not valid"}`).
fn is_fk_violation(error: &sqlx::Error) -> bool {
    match error {
        sqlx::Error::Database(db) => db.code().as_deref() == Some("23503"),
        _ => false,
    }
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

/// `GET .../links/<pk>/` (`views/issue.py:1697-1714`). The `pk is None`
/// list branch (BUG-6) is unreachable: the route always binds `pk`.
pub async fn get_link_detail(
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
    match link_detail_inner(&state, &headers, &slug, &project_id, &issue_id, &pk, &query).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

async fn link_detail_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    project_id_raw: &str,
    issue_id: &Uuid,
    pk: &Uuid,
    query: &QueryMap,
) -> Result<Response, Denial> {
    use pidash_services::v1_work_items::shape_links::LINK_SHOW_FIELDS;
    let pre = preamble(state, headers, slug).await?;
    let project_id = rewrite_project_id(&pre.pool, slug, project_id_raw).await?;
    let workspace_id = pre.workspace_id.ok_or(Denial::Forbidden)?;
    require_gate(
        &pre.pool,
        &workspace_id,
        slug,
        &pre.actor.id,
        &project_id,
        V1WorkItemsRoute::LinkDetail,
        "GET",
    )
    .await?;
    let tz = activate_timezone(pre.actor.timezone.as_deref())?;
    let Some(row) =
        fetch_link_detail(&pre.pool, slug, &project_id, issue_id, &pre.actor.id, pk).await?
    else {
        return Err(Denial::NotFound(RESOURCE_NOT_FOUND_BODY.to_owned()));
    };
    let (field_specs, kept, expand) = field_selection(query, LINK_SHOW_FIELDS)?;
    let expand_refs: Vec<&str> = expand.iter().map(String::as_str).collect();
    let web_base = pidash_services::v1_work_items::shape_issue::web_base_url(
        state.settings().urls.web_url.as_deref(),
        state.settings().urls.app_base_url.as_deref(),
    );
    let decoded = decode_link(&row, &tz)?;
    let expansions = link_expansions(
        &pre.pool,
        &decoded,
        slug,
        &tz,
        web_base.as_deref(),
        &expand_refs,
        &kept,
    )
    .await?;
    let body = render_link_value(&decoded, field_specs.as_deref(), &expand_refs, &expansions)?;
    Ok(json_response(
        StatusCode::OK,
        serde_json::to_string(&body).map_err(|_| Denial::ServerError)?,
    ))
}

/// `PATCH .../links/<pk>/` (`views/issue.py:1737-1761`).
pub async fn patch_link(
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
    let raw = match read_body(body).await {
        Ok(raw) => raw,
        Err(denial) => return denial.into_response(),
    };
    match link_patch_inner(&state, &headers, &slug, &project_id, &issue_id, &pk, &raw).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

async fn link_patch_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    project_id_raw: &str,
    issue_id: &Uuid,
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
        V1WorkItemsRoute::LinkDetail,
        "PATCH",
    )
    .await?;
    let tz = activate_timezone(pre.actor.timezone.as_deref())?;
    let Some(before) = fetch_link_direct(&pre.pool, slug, &project_id, issue_id, pk).await? else {
        return Err(Denial::NotFound(RESOURCE_NOT_FOUND_BODY.to_owned()));
    };
    let parsed = parse_write_body(headers, raw_body)?;
    let requested = pidash_jobs::tasks_webhooks::activity_dispatch::django_dumps(&parsed.value);
    let before_decoded = decode_link(&before, &tz)?;
    let current = pidash_jobs::tasks_webhooks::activity_dispatch::django_dumps(&render_link_value(
        &before_decoded,
        None,
        &[],
        &[],
    )?);
    let Some(map) = parsed.value.as_object() else {
        return Err(Denial::FieldErrors(patch_not_a_dict_body(&parsed.value)));
    };
    let validated = validate_link_patch(map, parsed.from_form, &tz).map_err(Denial::FieldErrors)?;
    // `serializer.save()` — full `save()`, so `updated_at`/`updated_by`
    // move even with zero validated fields.
    let now = now_utc();
    let new_url = validated
        .url
        .clone()
        .unwrap_or_else(|| before_decoded.url.clone());
    let new_title = validated
        .title
        .clone()
        .unwrap_or_else(|| before_decoded.title.clone());
    let new_metadata = validated
        .metadata
        .clone()
        .unwrap_or_else(|| before_decoded.metadata.clone());
    let new_deleted_at = validated.deleted_at.unwrap_or_else(|| {
        before
            .try_get::<Option<DateTime<Utc>>, _>("deleted_at")
            .unwrap_or(None)
    });
    sqlx::query(
        r#"UPDATE "issue_links" SET "title" = $1, "url" = $2, "metadata" = $3, "deleted_at" = $4,
            "updated_at" = $5, "updated_by_id" = $6 WHERE "id" = $7"#,
    )
    .bind(new_title.as_deref())
    .bind(&new_url)
    .bind(&new_metadata)
    .bind(new_deleted_at)
    .bind(now)
    .bind(pre.actor.id)
    .bind(pk)
    .execute(&pre.pool)
    .await
    .map_err(|error| db_error(error, "link-patch-update"))?;
    // Crawl uses the post-save `serializer.data` id/url (`:1749`).
    let crawl_args = work_tasks::crawl_link_title_args(&pk.to_string(), &new_url);
    enqueue_best_effort(
        &pre.pool,
        work_tasks::CRAWL_LINK_TITLE_TASK,
        crawl_args,
        Map::new(),
    )
    .await;
    let kwargs = work_tasks::issue_activity_kwargs(
        work_tasks::ACTIVITY_LINK_UPDATED,
        Some(&requested),
        &pre.actor.id.to_string(),
        &issue_id.to_string(),
        &project_id.to_string(),
        Some(&current),
        Utc::now().timestamp(),
    );
    enqueue_best_effort(&pre.pool, work_tasks::ISSUE_ACTIVITY_TASK, vec![], kwargs).await;
    // The response re-renders the in-memory instance (`:1759`) — a refetch
    // would MISS when the patch set `deleted_at` (the direct lookup is
    // live-scoped), while Python still answers 200.
    let decoded = DecodedLink {
        id: before_decoded.id.clone(),
        created_at: before_decoded.created_at.clone(),
        updated_at: crate::serializer::render_datetime_in(&now, &tz),
        deleted_at: new_deleted_at.map(|dt| crate::serializer::render_datetime_in(&dt, &tz)),
        title: new_title,
        url: new_url,
        metadata: new_metadata,
        created_by: before_decoded.created_by.clone(),
        updated_by: Some(pre.actor.id.to_string()),
        project: before_decoded.project.clone(),
        workspace: before_decoded.workspace.clone(),
        issue: before_decoded.issue.clone(),
    };
    let body = render_link_value(&decoded, None, &[], &[])?;
    Ok(json_response(
        StatusCode::OK,
        serde_json::to_string(&body).map_err(|_| Denial::ServerError)?,
    ))
}

/// `DELETE .../links/<pk>/` (`views/issue.py:1775-1793`).
pub async fn delete_link(
    State(state): State<AppState>,
    OriginalUri(original): OriginalUri,
    Path((slug, project_id, issue_id, pk)): Path<(String, String, String, String)>,
    headers: HeaderMap,
) -> Response {
    if !crate::runner_runs::is_uuid_path_segment(&issue_id)
        || !crate::runner_runs::is_uuid_path_segment(&pk)
    {
        return proxy_request(&state, "DELETE", original.to_string()).await;
    }
    let issue_id = issue_id.parse::<Uuid>().expect("checked segment");
    let pk = pk.parse::<Uuid>().expect("checked segment");
    match link_delete_inner(&state, &headers, &slug, &project_id, &issue_id, &pk).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

async fn link_delete_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    project_id_raw: &str,
    issue_id: &Uuid,
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
        V1WorkItemsRoute::LinkDetail,
        "DELETE",
    )
    .await?;
    let tz = activate_timezone(pre.actor.timezone.as_deref())?;
    let Some(before) = fetch_link_direct(&pre.pool, slug, &project_id, issue_id, pk).await? else {
        return Err(Denial::NotFound(RESOURCE_NOT_FOUND_BODY.to_owned()));
    };
    let before_decoded = decode_link(&before, &tz)?;
    let current = pidash_jobs::tasks_webhooks::activity_dispatch::django_dumps(&render_link_value(
        &before_decoded,
        None,
        &[],
        &[],
    )?);
    // The activity fans out BEFORE the write (`:1783-1792`).
    let mut requested_map = Map::with_capacity(1);
    requested_map.insert("link_id".to_owned(), Value::String(pk.to_string()));
    let requested =
        pidash_jobs::tasks_webhooks::activity_dispatch::django_dumps(&Value::Object(requested_map));
    let kwargs = work_tasks::issue_activity_kwargs(
        work_tasks::ACTIVITY_LINK_DELETED,
        Some(&requested),
        &pre.actor.id.to_string(),
        &issue_id.to_string(),
        &project_id.to_string(),
        Some(&current),
        Utc::now().timestamp(),
    );
    enqueue_best_effort(&pre.pool, work_tasks::ISSUE_ACTIVITY_TASK, vec![], kwargs).await;
    // `SoftDeleteModel.delete()`: `deleted_at` + full `save()` (so
    // `updated_at`/`updated_by` move) plus the related-objects sweep.
    // Two `now()` calls like `delete()`: `deleted_at` stamps first,
    // `updated_at` re-stamps on the `save()`.
    let deleted_at = now_utc();
    let updated_at = now_utc();
    sqlx::query(
        r#"UPDATE "issue_links" SET "deleted_at" = $1, "updated_at" = $2, "updated_by_id" = $3 WHERE "id" = $4"#,
    )
    .bind(deleted_at)
    .bind(updated_at)
    .bind(pre.actor.id)
    .bind(pk)
    .execute(&pre.pool)
    .await
    .map_err(|error| db_error(error, "link-delete"))?;
    let (sweep_args, sweep_kwargs) = soft_delete_sweep("issuelink", &pk.to_string());
    enqueue_best_effort(&pre.pool, SOFT_DELETE_TASK, sweep_args, sweep_kwargs).await;
    Ok(Response::builder()
        .status(StatusCode::NO_CONTENT)
        .body(axum::body::Body::empty())
        .expect("empty response"))
}

// ---------------------------------------------------------------------------
// Comment input helpers
// ---------------------------------------------------------------------------

/// Python truthiness for a `request.data.get()` value: missing/null are
/// falsy; `""`/`0`/`0.0`/`false`/`[]`/`{}` are falsy; everything else is
/// truthy.
fn is_truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(flag)) => *flag,
        Some(Value::Number(number)) => number.as_f64().map(|n| n != 0.0).unwrap_or(true),
        Some(Value::String(text)) => !text.is_empty(),
        Some(Value::Array(items)) => !items.is_empty(),
        Some(Value::Object(map)) => !map.is_empty(),
    }
}

/// `base_host(request, is_app=True)` (`utils/host.py:17-60`): the app base
/// URL when set, else the web origin; a missing pair raises
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

/// Python `str()` for a JSON scalar, for the external-dedupe guards:
/// `CharField.get_prep_value` stringifies filter values (ints stringify,
/// bools spell `True`/`False`), and the PATCH guard compares
/// `stored != str(incoming)` (`:2047`). Composites return `None`: their
/// `repr` can only match a stored repr-string, which no API write can
/// produce (the serializer 400s them first) — skipping the query is
/// equivalent to the filter miss. All verified live.
fn python_scalar_str(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(python_number_str(number)),
        Value::Bool(flag) => Some(if *flag { "True" } else { "False" }.to_owned()),
        _ => None,
    }
}

/// The comment-PATCH dup-guard input (`:2045-2061`): `None` skips the
/// query (falsy id, `str()`-equal to stored, or a composite the filter
/// could never match); otherwise the `(source, id)` filter pair.
/// `data.get("external_source", stored)` — present-null queries `IS
/// NULL`; present-but-empty still filters; absent inherits the stored
/// value, NULL included; composite sources skip like composite ids. All
/// verified live.
fn patch_external_guard(
    map: &Map<String, Value>,
    stored_id: Option<&str>,
    stored_source: Option<String>,
) -> Option<(Option<String>, String)> {
    if !is_truthy(map.get("external_id")) {
        return None;
    }
    let stored = stored_id.unwrap_or_default();
    let incoming = match map.get("external_id").and_then(python_scalar_str) {
        Some(text) if text == stored => return None,
        Some(text) => text,
        None => return None,
    };
    let source: Option<String> = match map.get("external_source") {
        None => stored_source,
        Some(Value::Null) => None,
        Some(Value::String(text)) => Some(text.clone()),
        // Other scalars stringify; composites skip (their filter would
        // miss; the serializer 400s either way).
        Some(value) => Some(python_scalar_str(value)?),
    };
    Some((source, incoming))
}

/// The comment external-dedupe guard input: both values truthy (Python
/// `and`), rendered as filter strings via `get_prep_value` (`str()`).
/// Composites skip (their filter would miss; the serializer 400s).
fn dedupe_pair(map: &Map<String, Value>, source_default: Option<&str>) -> Option<(String, String)> {
    let id_raw = map.get("external_id");
    let source_raw = map.get("external_source");
    let id_truthy = is_truthy(id_raw);
    let source_truthy = match source_raw {
        Some(_) => is_truthy(source_raw),
        // Patch default: the stored source (NULL counts as missing here —
        // the `and` needs a truthy stored value too).
        None => source_default.map(|s| !s.is_empty()).unwrap_or(false),
    };
    if !(id_truthy && source_truthy) {
        return None;
    }
    let id = id_raw.and_then(python_scalar_str)?;
    let source = match source_raw {
        Some(value) => python_scalar_str(value)?,
        None => source_default.unwrap_or_default().to_owned(),
    };
    Some((source, id))
}

/// Pre-validate `comment_json` for HTML-form bodies
/// (`JSONField.get_value` arrives as a JSON string): parse in place, or
/// answer the field's `invalid` 400. JSON bodies pass through untouched.
fn form_comment_json(map: &mut Map<String, Value>, from_form: bool) -> Result<(), Denial> {
    if !from_form {
        return Ok(());
    }
    if let Some(Value::String(text)) = map.get("comment_json").cloned() {
        match serde_json::from_str(&text) {
            Ok(value) => {
                map.insert("comment_json".to_owned(), value);
            }
            Err(_) => {
                return Err(Denial::FieldErrors(
                    r#"{"comment_json":["Value must be valid JSON."]}"#.to_owned(),
                ));
            }
        }
    }
    // `get_value`: blank + not-required + no `allow_blank` → absent (both
    // are `ChoiceField`s with defaults and no `blank=True`).
    for key in ["access", "speaker_type"] {
        if map.get(key) == Some(&Value::String(String::new())) {
            map.shift_remove(key);
        }
    }
    // `get_value`: blank + `allow_null`, no `allow_blank` → None (the
    // `UUIDField` takes no `allow_blank` from `blank=True`).
    if map.get("speaker_agent_run_id") == Some(&Value::String(String::new())) {
        map.insert("speaker_agent_run_id".to_owned(), Value::Null);
    }
    Ok(())
}

/// The comment-create 409 body: `{"error": ..., "id": ...}` in view order.
fn comment_dup_body(id: &str) -> String {
    let mut map = Map::with_capacity(2);
    map.insert(
        "error".to_owned(),
        Value::String(COMMENT_DUP_MESSAGE.to_owned()),
    );
    map.insert("id".to_owned(), Value::String(id.to_owned()));
    serde_json::to_string(&map).expect("dup body")
}

/// `GET .../comments/` (`views/issue.py:1851-1862`).
pub async fn get_comment_list(
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
    match comment_list_inner(&state, &headers, &slug, &project_id, &issue_id, &query).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

async fn comment_list_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    project_id_raw: &str,
    issue_id: &Uuid,
    query: &QueryMap,
) -> Result<Response, Denial> {
    use pidash_services::v1_work_items::shape_social::COMMENT_READ_FIELDS;
    let pre = preamble(state, headers, slug).await?;
    let project_id = rewrite_project_id(&pre.pool, slug, project_id_raw).await?;
    let workspace_id = pre.workspace_id.ok_or(Denial::Forbidden)?;
    require_gate(
        &pre.pool,
        &workspace_id,
        slug,
        &pre.actor.id,
        &project_id,
        V1WorkItemsRoute::CommentList,
        "GET",
    )
    .await?;
    let tz = activate_timezone(pre.actor.timezone.as_deref())?;
    let per_page =
        crate::paginator::parse_per_page(query_last(query, "per_page").as_deref(), 1000, 1000)
            .map_err(page_denial)?;
    let cursor_raw = query_last(query, "cursor").unwrap_or_else(|| format!("{per_page}:0:0"));
    let cursor = crate::paginator::Cursor::from_string(&cursor_raw).map_err(page_denial)?;
    let rows = fetch_comment_rows(&pre.pool, slug, &project_id, issue_id, &pre.actor.id).await?;
    let total_count = rows.len() as i64;
    let (page_rows, has_more) = window_rows(&rows, per_page, &cursor)?;
    let (field_specs, kept, expand) = field_selection(query, COMMENT_READ_FIELDS)?;
    let expand_refs: Vec<&str> = expand.iter().map(String::as_str).collect();
    let web_base = pidash_services::v1_work_items::shape_issue::web_base_url(
        state.settings().urls.web_url.as_deref(),
        state.settings().urls.app_base_url.as_deref(),
    );
    // One issue per list: a single `get_url` for every row.
    let list_url = comment_url(
        &pre.pool,
        &workspace_id,
        &project_id,
        issue_id,
        web_base.as_deref(),
    )
    .await?;
    let mut rendered: Vec<Value> = Vec::with_capacity(page_rows.len());
    for row in page_rows {
        let decoded = decode_comment(row, &tz)?;
        let expansions = comment_expansions(
            &pre.pool,
            &decoded,
            slug,
            &tz,
            web_base.as_deref(),
            &expand_refs,
            &kept,
        )
        .await?;
        rendered.push(render_comment_value(
            &decoded,
            list_url.clone(),
            field_specs.as_deref(),
            &expand_refs,
            &expansions,
        )?);
    }
    let next = crate::paginator::next_cursor(per_page, cursor.offset, has_more);
    let prev = crate::paginator::prev_cursor(per_page, cursor.offset);
    envelope(total_count, per_page, &next, &prev, Value::Array(rendered))
}

/// `GET .../comments/<pk>/` (`views/issue.py:2003-2010`).
pub async fn get_comment_detail(
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
    match comment_detail_inner(&state, &headers, &slug, &project_id, &issue_id, &pk, &query).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

async fn comment_detail_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    project_id_raw: &str,
    issue_id: &Uuid,
    pk: &Uuid,
    query: &QueryMap,
) -> Result<Response, Denial> {
    use pidash_services::v1_work_items::shape_social::COMMENT_READ_FIELDS;
    let pre = preamble(state, headers, slug).await?;
    let project_id = rewrite_project_id(&pre.pool, slug, project_id_raw).await?;
    let workspace_id = pre.workspace_id.ok_or(Denial::Forbidden)?;
    require_gate(
        &pre.pool,
        &workspace_id,
        slug,
        &pre.actor.id,
        &project_id,
        V1WorkItemsRoute::CommentDetail,
        "GET",
    )
    .await?;
    let tz = activate_timezone(pre.actor.timezone.as_deref())?;
    let Some(row) =
        fetch_comment_detail(&pre.pool, slug, &project_id, issue_id, &pre.actor.id, pk).await?
    else {
        return Err(Denial::NotFound(RESOURCE_NOT_FOUND_BODY.to_owned()));
    };
    let (field_specs, kept, expand) = field_selection(query, COMMENT_READ_FIELDS)?;
    let expand_refs: Vec<&str> = expand.iter().map(String::as_str).collect();
    let web_base = pidash_services::v1_work_items::shape_issue::web_base_url(
        state.settings().urls.web_url.as_deref(),
        state.settings().urls.app_base_url.as_deref(),
    );
    let decoded = decode_comment(&row, &tz)?;
    let url = comment_url(
        &pre.pool,
        &workspace_id,
        &project_id,
        issue_id,
        web_base.as_deref(),
    )
    .await?;
    let expansions = comment_expansions(
        &pre.pool,
        &decoded,
        slug,
        &tz,
        web_base.as_deref(),
        &expand_refs,
        &kept,
    )
    .await?;
    let body = render_comment_value(
        &decoded,
        url,
        field_specs.as_deref(),
        &expand_refs,
        &expansions,
    )?;
    Ok(json_response(
        StatusCode::OK,
        serde_json::to_string(&body).map_err(|_| Denial::ServerError)?,
    ))
}

/// `POST .../comments/` (`views/issue.py:1885-1949`).
pub async fn post_comment(
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
    let raw = match read_body(body).await {
        Ok(raw) => raw,
        Err(denial) => return denial.into_response(),
    };
    match comment_post_inner(&state, &headers, &slug, &project_id, &issue_id, &raw).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

async fn comment_post_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    project_id_raw: &str,
    issue_id: &Uuid,
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
        V1WorkItemsRoute::CommentList,
        "POST",
    )
    .await?;
    let tz = activate_timezone(pre.actor.timezone.as_deref())?;
    let parsed = parse_write_body(headers, raw_body)?;
    // The dup guard touches `request.data.get` first (`:1893`) — a
    // non-object body raises `AttributeError` into the 500.
    let Some(map) = parsed.value.as_object() else {
        return Err(Denial::ServerError);
    };
    if let Some((source, external_id)) = dedupe_pair(map, None) {
        if comment_external_exists(&pre.pool, &project_id, slug, Some(&source), &external_id)
            .await?
        {
            // The 409 `"id"` is the DUPLICATE's id (`:1902-1907`).
            let Some(first) =
                comment_external_first_id(&pre.pool, &project_id, slug, &source, &external_id)
                    .await?
            else {
                return Err(Denial::ServerError);
            };
            return Err(Denial::Conflict(comment_dup_body(&first.to_string())));
        }
    }
    let mut shape_map = map.clone();
    form_comment_json(&mut shape_map, parsed.from_form)?;
    let shape_value = Value::Object(shape_map);
    let validated = validate_comment_create(&CommentWriteInput {
        body: &shape_value,
        partial: false,
    })
    .map_err(|error| Denial::FieldErrors(error.body().to_owned()))?;
    // `ProjectBaseModel.save` sets the workspace from the project.
    let comment_workspace: Option<Uuid> =
        sqlx::query_scalar(r#"SELECT "workspace_id" FROM "projects" WHERE "id" = $1"#)
            .bind(project_id)
            .fetch_optional(&pre.pool)
            .await
            .map_err(|error| db_error(error, "comment-post-workspace"))?
            .flatten();
    let Some(comment_workspace) = comment_workspace else {
        return Err(Denial::FieldErrors(PAYLOAD_NOT_VALID_BODY.to_owned()));
    };
    let comment_id = Uuid::new_v4();
    let created_at = now_utc();
    let updated_at = now_utc();
    let comment_json = validated
        .comment_json
        .clone()
        .unwrap_or_else(|| serde_json::json!({}));
    let comment_html = validated
        .comment_html
        .clone()
        .unwrap_or_else(|| "<p></p>".to_owned());
    let stripped = strip_comment_html(&comment_html);
    let labels = validated.labels.clone().unwrap_or_default();
    let access = validated
        .access
        .clone()
        .unwrap_or_else(|| "INTERNAL".to_owned());
    let speaker_type = validated
        .speaker_type
        .clone()
        .unwrap_or_else(|| "human".to_owned());
    let speaker_label = validated.speaker_label.clone().unwrap_or_default();
    // Canonical UUID text per the shape contract; the column is `uuid`.
    let speaker_run_id: Option<Uuid> = validated
        .speaker_agent_run_id
        .clone()
        .unwrap_or(None)
        .map(|text| text.parse().expect("canonical run uuid"));
    // `IssueComment.save` runs in one `transaction.atomic()`: the comment
    // INSERT, the `Description` INSERT, and the `description_id` UPDATE.
    let mut tx = pre
        .pool
        .begin()
        .await
        .map_err(|error| db_error(error, "comment-post-tx"))?;
    sqlx::query(
        r#"INSERT INTO "issue_comments" ("id", "created_at", "updated_at", "created_by_id", "updated_by_id",
            "project_id", "workspace_id", "issue_id", "actor_id", "comment_json", "comment_html",
            "comment_stripped", "description_id", "attachments", "labels", "access", "external_source",
            "external_id", "speaker_type", "speaker_label", "speaker_agent_run_id", "edited_at", "parent_id")
           VALUES ($1, $2, $3, $4, NULL, $5, $6, $7, $4, $8, $9, $10, NULL, $11, $12, $13, $14, $15, $16, $17, $18, NULL, NULL)"#,
    )
    .bind(comment_id)
    .bind(created_at)
    .bind(updated_at)
    .bind(pre.actor.id)
    .bind(project_id)
    .bind(comment_workspace)
    .bind(issue_id)
    .bind(&comment_json)
    .bind(&comment_html)
    .bind(&stripped)
    .bind(Vec::<String>::new())
    .bind(&labels)
    .bind(&access)
    .bind(validated.external_source.clone().unwrap_or(None))
    .bind(validated.external_id.clone().unwrap_or(None))
    .bind(&speaker_type)
    .bind(&speaker_label)
    .bind(speaker_run_id)
    .execute(&mut *tx)
    .await
    .map_err(|error| {
        // No issue lookup precedes the create: a bogus `issue_id`
        // violates the FK, and `handle_exception` answers the
        // `IntegrityError` 400 (`api/views/base.py:142-147`). The
        // transaction rolls back on drop, like the failed ORM `save()`.
        if is_fk_violation(&error) {
            Denial::FieldErrors(PAYLOAD_NOT_VALID_BODY.to_owned())
        } else {
            db_error(error, "comment-post-insert")
        }
    })?;
    let description_id = Uuid::new_v4();
    let desc_created_at = now_utc();
    let desc_updated_at = now_utc();
    let desc_stripped = description_stripped_for_create(&comment_html);
    sqlx::query(
        r#"INSERT INTO "descriptions" ("id", "created_at", "updated_at", "created_by_id", "updated_by_id",
            "workspace_id", "project_id", "description_json", "description_html", "description_binary",
            "description_stripped")
           VALUES ($1, $2, $3, $4, NULL, $5, $6, $7, $8, NULL, $9)"#,
    )
    .bind(description_id)
    .bind(desc_created_at)
    .bind(desc_updated_at)
    .bind(pre.actor.id)
    .bind(comment_workspace)
    .bind(project_id)
    .bind(&comment_json)
    .bind(&comment_html)
    .bind(desc_stripped)
    .execute(&mut *tx)
    .await
    .map_err(|error| db_error(error, "comment-post-desc"))?;
    sqlx::query(r#"UPDATE "issue_comments" SET "description_id" = $1 WHERE "id" = $2"#)
        .bind(description_id)
        .bind(comment_id)
        .execute(&mut *tx)
        .await
        .map_err(|error| db_error(error, "comment-post-desc-link"))?;
    tx.commit().await.map_err(|error| {
        // Every FK here is `DEFERRABLE INITIALLY DEFERRED`, so the bogus
        // `issue_id` violation fires at COMMIT, not at the INSERT — and
        // `handle_exception` answers the `IntegrityError` 400
        // (`api/views/base.py:142-147`). Every other FK in this
        // transaction references a row verified above to exist.
        if is_fk_violation(&error) {
            Denial::FieldErrors(PAYLOAD_NOT_VALID_BODY.to_owned())
        } else {
            db_error(error, "comment-post-commit")
        }
    })?;
    // The overrides (`:1921-1924`): `created_at` (or `now()`), `created_by`
    // (or the actor), `actor_id` in memory only (BUG-3) — saved with
    // `update_fields=["created_at","created_by"]`.
    let (db_created_at, response_created_at) = override_created_at(map.get("created_at"), &tz)?;
    let created_by = override_user_id(map.get("created_by"), &pre.actor.id)?;
    if let Err(error) = sqlx::query(
        r#"UPDATE "issue_comments" SET "created_at" = $1, "created_by_id" = $2 WHERE "id" = $3"#,
    )
    .bind(db_created_at)
    .bind(created_by)
    .bind(comment_id)
    .execute(&pre.pool)
    .await
    {
        if is_fk_violation(&error) {
            return Err(Denial::FieldErrors(PAYLOAD_NOT_VALID_BODY.to_owned()));
        }
        return Err(db_error(error, "comment-post-override"));
    }
    // Response `actor` is the in-memory override-or-actor (BUG-3 — always
    // identical to the response `created_by`); the DB row keeps the
    // requester.
    let response_actor = created_by;
    let decoded = DecodedComment {
        id: comment_id.to_string(),
        is_member: None,
        created_at: response_created_at,
        updated_at: crate::serializer::render_datetime_in(&updated_at, &tz),
        deleted_at: None,
        comment_json: comment_json.clone(),
        comment_html: comment_html.clone(),
        attachments: Vec::new(),
        labels: labels.iter().map(|item| Some(item.clone())).collect(),
        access: access.clone(),
        external_source: validated.external_source.clone().unwrap_or(None),
        external_id: validated.external_id.clone().unwrap_or(None),
        speaker_type: speaker_type.clone(),
        speaker_label: speaker_label.clone(),
        speaker_agent_run_id: validated.speaker_agent_run_id.clone().unwrap_or(None),
        edited_at: None,
        created_by: created_by.map(|id| id.to_string()),
        updated_by: Some(pre.actor.id.to_string()),
        project: project_id.to_string(),
        workspace: comment_workspace.to_string(),
        description: Some(description_id.to_string()),
        issue: issue_id.to_string(),
        actor: response_actor.map(|id| id.to_string()),
        parent: None,
    };
    // `requested_data` is the 9-field create shape over the saved row; the
    // activity actor is the post-override `created_by` (`"None"` wart).
    let requested = pidash_jobs::tasks_webhooks::activity_dispatch::django_dumps(
        &render_comment_create_value(&decoded)?,
    );
    let actor_text = created_by
        .map(|id| id.to_string())
        .unwrap_or_else(|| "None".to_owned());
    let kwargs = work_tasks::issue_activity_kwargs(
        work_tasks::ACTIVITY_COMMENT_CREATED,
        Some(&requested),
        &actor_text,
        &issue_id.to_string(),
        &project_id.to_string(),
        None,
        Utc::now().timestamp(),
    );
    enqueue_best_effort(&pre.pool, work_tasks::ISSUE_ACTIVITY_TASK, vec![], kwargs).await;
    // The webhook carries the RAW body, the requester, and the app origin.
    let origin = app_origin(&state.settings().urls)?;
    let webhook = work_tasks::comment_model_activity_kwargs(
        &comment_id.to_string(),
        parsed.value.clone(),
        None,
        &pre.actor.id.to_string(),
        slug,
        &origin,
    );
    enqueue_best_effort(&pre.pool, work_tasks::MODEL_ACTIVITY_TASK, vec![], webhook).await;
    let web_base = pidash_services::v1_work_items::shape_issue::web_base_url(
        state.settings().urls.web_url.as_deref(),
        state.settings().urls.app_base_url.as_deref(),
    );
    let url = comment_url(
        &pre.pool,
        &comment_workspace,
        &project_id,
        issue_id,
        web_base.as_deref(),
    )
    .await?;
    let body = render_comment_value(&decoded, url, None, &[], &[])?;
    Ok(json_created(
        serde_json::to_string(&body).map_err(|_| Denial::ServerError)?,
    ))
}

/// `strip_tags(self.comment_html) if self.comment_html != "" else ""`
/// (`db/models/issue.py:606` over `utils/html_processor.py`, MLStripper).
fn strip_comment_html(html: &str) -> String {
    if html.is_empty() {
        String::new()
    } else {
        pidash_db::app_pages::strip::ml_strip_tags(html)
    }
}

/// The `Description` row's stripped text on comment create:
/// `Description.save` RECOMPUTES it with Django's own `strip_tags`
/// (`db/models/description.py:22-29`), which keeps entities verbatim —
/// unlike the comment column's entity-decoding MLStripper. Empty HTML
/// stores NULL. (PATCH syncs the decoded form via queryset `update()`,
/// which runs no `save()` — that path is untouched.)
fn description_stripped_for_create(comment_html: &str) -> Option<String> {
    if comment_html.is_empty() {
        None
    } else {
        Some(crate::space::sanitize::strip_tags(comment_html))
    }
}

/// The `created_at` override (`:1921`): absent → `now()`; a datetime
/// string stores its instant (naive values bypass serializer handling —
/// direct assignment — so Postgres reads them in the session zone, UTC),
/// while the RESPONSE renders the raw input VERBATIM: the in-memory
/// attribute keeps the assigned string and DRF returns strings untouched.
/// Returns `(db_value, response_text)`. Explicit null violates NOT NULL →
/// `IntegrityError` 400; unparseable strings → `ValidationError` 400;
/// non-string scalars reach `parse_datetime` raw, whose `fromisoformat`
/// raises an UNCAUGHT `TypeError` (only `ValueError` is caught) → 500.
fn override_created_at(raw: Option<&Value>, tz: &Tz) -> Result<(DateTime<Utc>, String), Denial> {
    match raw {
        None => {
            let now = now_utc();
            Ok((now, crate::serializer::render_datetime_in(&now, tz)))
        }
        Some(Value::Null) => Err(Denial::FieldErrors(PAYLOAD_NOT_VALID_BODY.to_owned())),
        Some(Value::String(text)) => {
            let naive = parse_naive_or_aware(text)
                .map_err(|_| Denial::FieldErrors(VALID_DETAIL_BODY.to_owned()))?;
            let db = match naive {
                NaiveOrAware::Aware(instant) => instant,
                NaiveOrAware::Naive(local) => trunc_micros(local.and_utc()),
            };
            Ok((db, text.clone()))
        }
        Some(_) => Err(Denial::ServerError),
    }
}

/// A parsed override datetime before zone handling.
enum NaiveOrAware {
    Naive(chrono::NaiveDateTime),
    Aware(DateTime<Utc>),
}

/// Split an override datetime string into naive wall time vs instant
/// (same grammar as [`parse_django_datetime`).
fn parse_naive_or_aware(text: &str) -> Result<NaiveOrAware, ParseDatetimeError> {
    use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
    let text = text.trim();
    if text.is_empty() || text.len() > 100 {
        return Err(ParseDatetimeError::Invalid);
    }
    let (core, offset_seconds): (&str, Option<i32>) = if text.ends_with(['Z', 'z']) {
        (&text[..text.len() - 1], Some(0))
    } else {
        match text.rfind(['+', '-']) {
            Some(idx) if idx > 7 => {
                let (head, tail) = text.split_at(idx);
                let sign = if tail.starts_with('+') { 1 } else { -1 };
                let digits: String = tail[1..].chars().filter(|c| *c != ':').collect();
                if digits.len() == 2 || digits.len() == 4 {
                    let hours: i32 = digits[..2]
                        .parse()
                        .map_err(|_| ParseDatetimeError::Invalid)?;
                    let minutes: i32 = if digits.len() == 4 {
                        digits[2..]
                            .parse()
                            .map_err(|_| ParseDatetimeError::Invalid)?
                    } else {
                        0
                    };
                    if hours > 23 || minutes > 59 {
                        return Err(ParseDatetimeError::Invalid);
                    }
                    (head, Some(sign * (hours * 3600 + minutes * 60)))
                } else {
                    return Err(ParseDatetimeError::Invalid);
                }
            }
            _ => (text, None),
        }
    };
    let (date_part, time_part) = match core.find(['T', 't', ' ']) {
        Some(idx) => (&core[..idx], Some(&core[idx + 1..])),
        None => (core, None),
    };
    let date = NaiveDate::parse_from_str(date_part, "%Y-%m-%d")
        .map_err(|_| ParseDatetimeError::Invalid)?;
    let time = match time_part {
        None => NaiveTime::from_hms_opt(0, 0, 0).expect("midnight"),
        Some(part) => {
            let part = part.trim();
            NaiveTime::parse_from_str(part, "%H:%M:%S%.f")
                .or_else(|_| NaiveTime::parse_from_str(part, "%H:%M:%S"))
                .or_else(|_| NaiveTime::parse_from_str(part, "%H:%M"))
                .map_err(|_| ParseDatetimeError::Invalid)?
        }
    };
    let naive = NaiveDateTime::new(date, time);
    match offset_seconds {
        Some(offset) => Ok(NaiveOrAware::Aware(trunc_micros(
            naive.and_utc() - chrono::Duration::seconds(i64::from(offset)),
        ))),
        None => Ok(NaiveOrAware::Naive(naive)),
    }
}

/// `PATCH .../comments/<pk>/` (`views/issue.py:2034-2089`).
pub async fn patch_comment(
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
    let raw = match read_body(body).await {
        Ok(raw) => raw,
        Err(denial) => return denial.into_response(),
    };
    match comment_patch_inner(&state, &headers, &slug, &project_id, &issue_id, &pk, &raw).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

async fn comment_patch_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    project_id_raw: &str,
    issue_id: &Uuid,
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
        V1WorkItemsRoute::CommentDetail,
        "PATCH",
    )
    .await?;
    let tz = activate_timezone(pre.actor.timezone.as_deref())?;
    let Some(before) = fetch_comment_direct(&pre.pool, slug, &project_id, issue_id, pk).await?
    else {
        return Err(Denial::NotFound(RESOURCE_NOT_FOUND_BODY.to_owned()));
    };
    let parsed = parse_write_body(headers, raw_body)?;
    let requested = pidash_jobs::tasks_webhooks::activity_dispatch::django_dumps(&parsed.value);
    // The before-image renders the direct (un-annotated) row with its real
    // `get_url`: no `is_member` key.
    let before_decoded = decode_comment(&before, &tz)?;
    let web_base = pidash_services::v1_work_items::shape_issue::web_base_url(
        state.settings().urls.web_url.as_deref(),
        state.settings().urls.app_base_url.as_deref(),
    );
    let before_url = comment_url(
        &pre.pool,
        &workspace_id,
        &project_id,
        issue_id,
        web_base.as_deref(),
    )
    .await?;
    let current = pidash_jobs::tasks_webhooks::activity_dispatch::django_dumps(
        &render_comment_value(&before_decoded, before_url, None, &[], &[])?,
    );
    let Some(map) = parsed.value.as_object() else {
        // `json.dumps` accepts any body, but the guard's `.get` raises
        // `AttributeError` into the 500 (`:2046`).
        return Err(Denial::ServerError);
    };
    // The dup guard (`:2045-2061`): truthy `external_id`, different from
    // the stored value (`str()`-compared), and an existing row under the
    // given-or-stored source. The 409 `"id"` is the CURRENT row's id.
    if let Some((source, incoming)) = patch_external_guard(
        map,
        before_decoded.external_id.as_deref(),
        before_decoded.external_source.clone(),
    ) {
        if comment_external_exists(&pre.pool, &project_id, slug, source.as_deref(), &incoming)
            .await?
        {
            return Err(Denial::Conflict(comment_dup_body(&pk.to_string())));
        }
    }
    let mut shape_map = map.clone();
    form_comment_json(&mut shape_map, parsed.from_form)?;
    let shape_value = Value::Object(shape_map);
    let validated = validate_comment_create(&CommentWriteInput {
        body: &shape_value,
        partial: true,
    })
    .map_err(|error| Denial::FieldErrors(error.body().to_owned()))?;
    // Merge validated keys over the before-image (untouched keys keep
    // their values — equivalent to the ORM's field-wise `save()`).
    let before_json: Value = before
        .try_get("comment_json")
        .map_err(|_| Denial::ServerError)?;
    let before_html: String = before
        .try_get("comment_html")
        .map_err(|_| Denial::ServerError)?;
    let before_stripped: String = before
        .try_get("comment_stripped")
        .map_err(|_| Denial::ServerError)?;
    let new_json = validated
        .comment_json
        .clone()
        .unwrap_or(before_json.clone());
    let new_html = validated
        .comment_html
        .clone()
        .unwrap_or(before_html.clone());
    let new_stripped = if validated.comment_html.is_some() {
        strip_comment_html(&new_html)
    } else {
        before_stripped.clone()
    };
    let new_access = validated
        .access
        .clone()
        .unwrap_or(before_decoded.access.clone());
    let new_source = validated
        .external_source
        .clone()
        .unwrap_or_else(|| before_decoded.external_source.clone());
    let new_external_id = validated
        .external_id
        .clone()
        .unwrap_or_else(|| before_decoded.external_id.clone());
    let new_labels = validated.labels.clone().unwrap_or_else(|| {
        before_decoded
            .labels
            .iter()
            .map(|item| item.clone().unwrap_or_default())
            .collect()
    });
    let new_speaker_type = validated
        .speaker_type
        .clone()
        .unwrap_or(before_decoded.speaker_type.clone());
    let new_speaker_label = validated
        .speaker_label
        .clone()
        .unwrap_or(before_decoded.speaker_label.clone());
    let new_run_id = validated
        .speaker_agent_run_id
        .clone()
        .unwrap_or_else(|| before_decoded.speaker_agent_run_id.clone());
    let now = now_utc();
    sqlx::query(
        r#"UPDATE "issue_comments" SET "comment_json" = $1, "comment_html" = $2, "comment_stripped" = $3,
            "access" = $4, "external_source" = $5, "external_id" = $6, "labels" = $7,
            "speaker_type" = $8, "speaker_label" = $9, "speaker_agent_run_id" = $10,
            "updated_at" = $11, "updated_by_id" = $12 WHERE "id" = $13"#,
    )
    .bind(&new_json)
    .bind(&new_html)
    .bind(&new_stripped)
    .bind(&new_access)
    .bind(new_source.as_deref())
    .bind(new_external_id.as_deref())
    .bind(&new_labels)
    .bind(&new_speaker_type)
    .bind(&new_speaker_label)
    .bind(new_run_id.as_deref().and_then(|text| text.parse::<Uuid>().ok()))
    .bind(now)
    .bind(pre.actor.id)
    .bind(pk)
    .execute(&pre.pool)
    .await
    .map_err(|error| db_error(error, "comment-patch-update"))?;
    // The `Description` sync (`db/models/issue.py:629-647`): only changed
    // tracked fields, plus `updated_by`/`updated_at` — via queryset
    // `update()`, so no `save()` override re-strips. `_changes_on_save`
    // compares VALUES: a present-but-identical key syncs nothing.
    let html_changed = validated
        .comment_html
        .as_ref()
        .is_some_and(|html| *html != before_html);
    let json_changed = validated
        .comment_json
        .as_ref()
        .is_some_and(|json| *json != before_json);
    let stripped_changed = new_stripped != before_stripped;
    // No `description_id` (legacy rows) skips the sync silently (`:631`).
    if html_changed || json_changed || stripped_changed {
        if let Some(description_id) = before_decoded.description.as_deref() {
            let description_id: Uuid = description_id.parse().map_err(|_| Denial::ServerError)?;
            let mut sets: Vec<String> = Vec::new();
            if html_changed {
                sets.push("description_html".to_owned());
            }
            if stripped_changed {
                sets.push("description_stripped".to_owned());
            }
            if json_changed {
                sets.push("description_json".to_owned());
            }
            let set_sql = sets
                .iter()
                .enumerate()
                .map(|(idx, col)| format!("\"{col}\" = ${}", idx + 1))
                .collect::<Vec<_>>()
                .join(", ");
            let sql = format!(
                "UPDATE \"descriptions\" SET {set_sql}, \"updated_by_id\" = ${}, \"updated_at\" = ${} WHERE \"id\" = ${}",
                sets.len() + 1,
                sets.len() + 2,
                sets.len() + 3,
            );
            let mut query = sqlx::query(&sql);
            for col in &sets {
                query = match col.as_str() {
                    "description_html" => query.bind(&new_html),
                    "description_stripped" => query.bind(&new_stripped),
                    _ => query.bind(&new_json),
                };
            }
            query
                .bind(pre.actor.id)
                .bind(now)
                .bind(description_id)
                .execute(&pre.pool)
                .await
                .map_err(|error| db_error(error, "comment-patch-desc"))?;
        }
    }
    let kwargs = work_tasks::issue_activity_kwargs(
        work_tasks::ACTIVITY_COMMENT_UPDATED,
        Some(&requested),
        &pre.actor.id.to_string(),
        &issue_id.to_string(),
        &project_id.to_string(),
        Some(&current),
        Utc::now().timestamp(),
    );
    enqueue_best_effort(&pre.pool, work_tasks::ISSUE_ACTIVITY_TASK, vec![], kwargs).await;
    let origin = app_origin(&state.settings().urls)?;
    let webhook = work_tasks::comment_model_activity_kwargs(
        &pk.to_string(),
        parsed.value.clone(),
        Some(&current),
        &pre.actor.id.to_string(),
        slug,
        &origin,
    );
    enqueue_best_effort(&pre.pool, work_tasks::MODEL_ACTIVITY_TASK, vec![], webhook).await;
    // The response refetches by id (`:2086`) and renders un-annotated.
    let Some(after) = fetch_comment_direct(&pre.pool, slug, &project_id, issue_id, pk).await?
    else {
        return Err(Denial::ServerError);
    };
    let decoded = decode_comment(&after, &tz)?;
    let url = comment_url(
        &pre.pool,
        &workspace_id,
        &project_id,
        issue_id,
        web_base.as_deref(),
    )
    .await?;
    let body = render_comment_value(&decoded, url, None, &[], &[])?;
    Ok(json_response(
        StatusCode::OK,
        serde_json::to_string(&body).map_err(|_| Denial::ServerError)?,
    ))
}

/// `DELETE .../comments/<pk>/` (`views/issue.py:2103-2121`).
pub async fn delete_comment(
    State(state): State<AppState>,
    OriginalUri(original): OriginalUri,
    Path((slug, project_id, issue_id, pk)): Path<(String, String, String, String)>,
    headers: HeaderMap,
) -> Response {
    if !crate::runner_runs::is_uuid_path_segment(&issue_id)
        || !crate::runner_runs::is_uuid_path_segment(&pk)
    {
        return proxy_request(&state, "DELETE", original.to_string()).await;
    }
    let issue_id = issue_id.parse::<Uuid>().expect("checked segment");
    let pk = pk.parse::<Uuid>().expect("checked segment");
    match comment_delete_inner(&state, &headers, &slug, &project_id, &issue_id, &pk).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

async fn comment_delete_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    project_id_raw: &str,
    issue_id: &Uuid,
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
        V1WorkItemsRoute::CommentDetail,
        "DELETE",
    )
    .await?;
    let tz = activate_timezone(pre.actor.timezone.as_deref())?;
    let Some(before) = fetch_comment_direct(&pre.pool, slug, &project_id, issue_id, pk).await?
    else {
        return Err(Denial::NotFound(RESOURCE_NOT_FOUND_BODY.to_owned()));
    };
    let before_decoded = decode_comment(&before, &tz)?;
    let web_base = pidash_services::v1_work_items::shape_issue::web_base_url(
        state.settings().urls.web_url.as_deref(),
        state.settings().urls.app_base_url.as_deref(),
    );
    let before_url = comment_url(
        &pre.pool,
        &workspace_id,
        &project_id,
        issue_id,
        web_base.as_deref(),
    )
    .await?;
    let current = pidash_jobs::tasks_webhooks::activity_dispatch::django_dumps(
        &render_comment_value(&before_decoded, before_url, None, &[], &[])?,
    );
    // `SoftDeleteModel.delete()`: `deleted_at` + full `save()` (so
    // `updated_at`/`updated_by` move) plus the related-objects sweep.
    // Python deletes FIRST, then enqueues (`:2111-2120`).
    // Two `now()` calls like `delete()`: `deleted_at` stamps first,
    // `updated_at` re-stamps on the `save()`.
    let deleted_at = now_utc();
    let updated_at = now_utc();
    sqlx::query(
        r#"UPDATE "issue_comments" SET "deleted_at" = $1, "updated_at" = $2, "updated_by_id" = $3 WHERE "id" = $4"#,
    )
    .bind(deleted_at)
    .bind(updated_at)
    .bind(pre.actor.id)
    .bind(pk)
    .execute(&pre.pool)
    .await
    .map_err(|error| db_error(error, "comment-delete"))?;
    // `delete()` emits the sweep before the view enqueues the activity
    // (`:2111-2120`): write, sweep, activity.
    let (sweep_args, sweep_kwargs) = soft_delete_sweep("issuecomment", &pk.to_string());
    enqueue_best_effort(&pre.pool, SOFT_DELETE_TASK, sweep_args, sweep_kwargs).await;
    let mut requested_map = Map::with_capacity(1);
    requested_map.insert("comment_id".to_owned(), Value::String(pk.to_string()));
    let requested =
        pidash_jobs::tasks_webhooks::activity_dispatch::django_dumps(&Value::Object(requested_map));
    let kwargs = work_tasks::issue_activity_kwargs(
        work_tasks::ACTIVITY_COMMENT_DELETED,
        Some(&requested),
        &pre.actor.id.to_string(),
        &issue_id.to_string(),
        &project_id.to_string(),
        Some(&current),
        Utc::now().timestamp(),
    );
    enqueue_best_effort(&pre.pool, work_tasks::ISSUE_ACTIVITY_TASK, vec![], kwargs).await;
    Ok(Response::builder()
        .status(StatusCode::NO_CONTENT)
        .body(axum::body::Body::empty())
        .expect("empty response"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const F18_11: &str =
        include_str!("../../../../fixtures/v1_work_items/handlers/F18-11.work_items.json");

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

    fn decoded_link(body: &Value) -> DecodedLink {
        DecodedLink {
            id: str_field(body, "id"),
            created_at: str_field(body, "created_at"),
            updated_at: str_field(body, "updated_at"),
            deleted_at: opt_str_field(body, "deleted_at"),
            title: opt_str_field(body, "title"),
            url: str_field(body, "url"),
            metadata: body.get("metadata").cloned().unwrap_or(Value::Null),
            created_by: opt_str_field(body, "created_by"),
            updated_by: opt_str_field(body, "updated_by"),
            project: str_field(body, "project"),
            workspace: str_field(body, "workspace"),
            issue: str_field(body, "issue"),
        }
    }

    fn decoded_comment(body: &Value, is_member: Option<bool>) -> DecodedComment {
        let attachments = body
            .get("attachments")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
            .iter()
            .map(|item| item.as_str().map(str::to_owned))
            .collect();
        let labels = body
            .get("labels")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
            .iter()
            .map(|item| item.as_str().map(str::to_owned))
            .collect();
        DecodedComment {
            id: str_field(body, "id"),
            is_member,
            created_at: str_field(body, "created_at"),
            updated_at: str_field(body, "updated_at"),
            deleted_at: opt_str_field(body, "deleted_at"),
            comment_json: body.get("comment_json").cloned().unwrap_or(Value::Null),
            comment_html: str_field(body, "comment_html"),
            attachments,
            labels,
            access: str_field(body, "access"),
            external_source: opt_str_field(body, "external_source"),
            external_id: opt_str_field(body, "external_id"),
            speaker_type: str_field(body, "speaker_type"),
            speaker_label: str_field(body, "speaker_label"),
            speaker_agent_run_id: opt_str_field(body, "speaker_agent_run_id"),
            edited_at: opt_str_field(body, "edited_at"),
            created_by: opt_str_field(body, "created_by"),
            updated_by: opt_str_field(body, "updated_by"),
            project: str_field(body, "project"),
            workspace: str_field(body, "workspace"),
            description: opt_str_field(body, "description"),
            issue: str_field(body, "issue"),
            actor: opt_str_field(body, "actor"),
            parent: opt_str_field(body, "parent"),
        }
    }

    fn keys_in_order(map: &Map<String, Value>) -> Vec<String> {
        map.keys().cloned().collect()
    }

    // --- F18-11 replays: status + body byte-identical per route ---

    #[tokio::test]
    async fn replay_link_list_envelope_and_rows() {
        let record = call("link_list");
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
        // Both full rows round-trip byte-identically through the row
        // mapping + shape (volatile values ride in from the fixture).
        let full = record
            .get("results_full")
            .and_then(Value::as_array)
            .expect("results_full");
        assert_eq!(full.len(), 2);
        for row in full {
            let rendered = render_link_value(&decoded_link(row), None, &[], &[]).expect("render");
            assert_eq!(
                serde_json::to_string(&rendered).expect("json"),
                serde_json::to_string(row).expect("json"),
            );
        }
        // The envelope builder reproduces the recorded envelope for the
        // same inputs (all rows ride in here, so `count` matches too).
        let rendered_rows: Vec<Value> = full
            .iter()
            .map(|row| render_link_value(&decoded_link(row), None, &[], &[]).expect("render"))
            .collect();
        let next = crate::paginator::next_cursor(1000, 0, false);
        let prev = crate::paginator::prev_cursor(1000, 0);
        let response =
            envelope(2, 1000, &next, &prev, Value::Array(rendered_rows)).expect("envelope");
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let mine: Value = serde_json::from_slice(&bytes).expect("envelope parses");
        for key in expected_keys {
            assert_eq!(mine.get(key), recorded.get(key), "{key}");
        }
        let mut recorded_full = recorded.clone();
        recorded_full.as_object_mut().expect("object").insert(
            "results".to_owned(),
            record.get("results_full").cloned().unwrap_or(Value::Null),
        );
        assert_eq!(
            serde_json::to_string(&mine).expect("json"),
            serde_json::to_string(&recorded_full).expect("json"),
        );
    }

    #[tokio::test]
    async fn replay_comment_list_envelope_and_rows() {
        let record = call("comment_list");
        assert_eq!(record.get("status").and_then(Value::as_u64), Some(200));
        let recorded = record.get("envelope").expect("envelope");
        let full = record
            .get("results_full")
            .and_then(Value::as_array)
            .expect("results_full");
        assert_eq!(full.len(), 2);
        for row in full {
            let is_member = row.get("is_member").and_then(Value::as_bool);
            let url = opt_str_field(row, "url");
            let rendered =
                render_comment_value(&decoded_comment(row, is_member), url, None, &[], &[])
                    .expect("render");
            assert_eq!(
                serde_json::to_string(&rendered).expect("json"),
                serde_json::to_string(row).expect("json"),
            );
        }
        let rendered_rows: Vec<Value> = full
            .iter()
            .map(|row| {
                let is_member = row.get("is_member").and_then(Value::as_bool);
                let url = opt_str_field(row, "url");
                render_comment_value(&decoded_comment(row, is_member), url, None, &[], &[])
                    .expect("render")
            })
            .collect();
        let next = crate::paginator::next_cursor(1000, 0, false);
        let prev = crate::paginator::prev_cursor(1000, 0);
        let response =
            envelope(2, 1000, &next, &prev, Value::Array(rendered_rows)).expect("envelope");
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let mine: Value = serde_json::from_slice(&bytes).expect("envelope parses");
        let mut recorded_full = recorded.clone();
        recorded_full.as_object_mut().expect("object").insert(
            "results".to_owned(),
            record.get("results_full").cloned().unwrap_or(Value::Null),
        );
        assert_eq!(
            serde_json::to_string(&mine).expect("json"),
            serde_json::to_string(&recorded_full).expect("json"),
        );
    }

    #[test]
    fn replay_link_detail_body() {
        let record = call("link_detail");
        assert_eq!(record.get("status").and_then(Value::as_u64), Some(200));
        let body = record.get("body").expect("body");
        let rendered = render_link_value(&decoded_link(body), None, &[], &[]).expect("render");
        assert_eq!(
            serde_json::to_string(&rendered).expect("json"),
            serde_json::to_string(body).expect("json"),
        );
    }

    #[test]
    fn replay_comment_detail_body() {
        let record = call("comment_detail");
        assert_eq!(record.get("status").and_then(Value::as_u64), Some(200));
        let body = record.get("body").expect("body");
        let is_member = body.get("is_member").and_then(Value::as_bool);
        let url = opt_str_field(body, "url");
        let rendered = render_comment_value(&decoded_comment(body, is_member), url, None, &[], &[])
            .expect("render");
        assert_eq!(
            serde_json::to_string(&rendered).expect("json"),
            serde_json::to_string(body).expect("json"),
        );
    }

    #[test]
    fn replay_link_create_body() {
        let record = call("link_create");
        assert_eq!(record.get("status").and_then(Value::as_u64), Some(201));
        let body = record.get("body").expect("body");
        let rendered = render_link_value(&decoded_link(body), None, &[], &[]).expect("render");
        assert_eq!(
            serde_json::to_string(&rendered).expect("json"),
            serde_json::to_string(body).expect("json"),
        );
        // The activity `requested_data` is the 3-key create read shape.
        let data = link_create_data(
            body.get("title").and_then(Value::as_str),
            body.get("url").and_then(Value::as_str).expect("url"),
            body.get("issue").and_then(Value::as_str).expect("issue"),
        );
        assert_eq!(
            pidash_jobs::tasks_webhooks::activity_dispatch::django_dumps(&data),
            format!(
                "{{\"title\": {}, \"url\": {}, \"issue_id\": {}}}",
                serde_json::to_string(body.get("title").expect("title")).expect("json"),
                serde_json::to_string(body.get("url").expect("url")).expect("json"),
                serde_json::to_string(body.get("issue").expect("issue")).expect("json"),
            ),
        );
    }

    #[test]
    fn replay_comment_create_body() {
        let record = call("comment_create");
        assert_eq!(record.get("status").and_then(Value::as_u64), Some(201));
        let body = record.get("body").expect("body");
        // Write responses render the un-annotated instance: no `is_member`.
        assert!(body.get("is_member").is_none());
        let url = opt_str_field(body, "url");
        let rendered = render_comment_value(&decoded_comment(body, None), url, None, &[], &[])
            .expect("render");
        assert_eq!(
            serde_json::to_string(&rendered).expect("json"),
            serde_json::to_string(body).expect("json"),
        );
    }

    #[test]
    fn replay_link_patch_body() {
        let record = call("link_patch");
        assert_eq!(record.get("status").and_then(Value::as_u64), Some(200));
        let body = record.get("body").expect("body");
        assert_eq!(body.get("title").and_then(Value::as_str), Some("t2"));
        let rendered = render_link_value(&decoded_link(body), None, &[], &[]).expect("render");
        assert_eq!(
            serde_json::to_string(&rendered).expect("json"),
            serde_json::to_string(body).expect("json"),
        );
    }

    #[test]
    fn replay_comment_patch_body() {
        let record = call("comment_patch");
        assert_eq!(record.get("status").and_then(Value::as_u64), Some(200));
        let body = record.get("body").expect("body");
        assert!(body.get("is_member").is_none());
        let url = opt_str_field(body, "url");
        let rendered = render_comment_value(&decoded_comment(body, None), url, None, &[], &[])
            .expect("render");
        assert_eq!(
            serde_json::to_string(&rendered).expect("json"),
            serde_json::to_string(body).expect("json"),
        );
    }

    #[test]
    fn replay_link_create_bad_url() {
        let record = call("link_create_bad_url");
        assert_eq!(record.get("status").and_then(Value::as_u64), Some(400));
        let request = record.get("request").expect("request");
        let error = validate_link_create(request).expect_err("bad url fails");
        assert_eq!(
            error.body(),
            &serde_json::to_string(record.get("body").expect("body")).expect("json"),
        );
    }

    #[test]
    fn replay_delete_statuses() {
        for name in ["link_delete", "comment_delete"] {
            let record = call(name);
            assert_eq!(
                record.get("status").and_then(Value::as_u64),
                Some(204),
                "{name}"
            );
            assert_eq!(
                record.get("body"),
                Some(&serde_json::json!({"<non-json>": ""})),
                "{name}"
            );
        }
    }

    #[test]
    fn replay_deprecated_twins_same_body() {
        let fixture: Value = serde_json::from_str(F18_11).expect("fixture parses");
        let twins = fixture
            .get("deprecated_twins")
            .and_then(Value::as_object)
            .expect("twins");
        for name in ["link_list", "comment_list", "link_detail", "comment_detail"] {
            assert_eq!(
                twins.get(name).and_then(|t| t.get("same_body")),
                Some(&Value::Bool(true)),
                "{name}"
            );
        }
    }

    // --- Denials: exact statuses + bodies ---

    #[test]
    fn denial_statuses_and_bodies() {
        assert_eq!(
            Denial::Unauthorized.status_and_body(),
            (
                StatusCode::UNAUTHORIZED,
                r#"{"detail":"Authentication credentials were not provided."}"#.to_owned()
            )
        );
        assert_eq!(
            Denial::InvalidToken.status_and_body(),
            (
                StatusCode::FORBIDDEN,
                r#"{"detail":"Given API token is not valid"}"#.to_owned()
            )
        );
        assert_eq!(
            Denial::Forbidden.status_and_body(),
            (
                StatusCode::FORBIDDEN,
                r#"{"detail":"You do not have permission to perform this action."}"#.to_owned()
            )
        );
        assert_eq!(
            Denial::ProjectNotFound.status_and_body(),
            (
                StatusCode::NOT_FOUND,
                r#"{"detail":"Project not found"}"#.to_owned()
            )
        );
        assert_eq!(
            Denial::BadDetail("JSON parse error - x".to_owned()).status_and_body(),
            (
                StatusCode::BAD_REQUEST,
                r#"{"detail":"JSON parse error - x"}"#.to_owned()
            )
        );
        assert_eq!(
            Denial::BadError("The required key does not exist.".to_owned()).status_and_body(),
            (
                StatusCode::BAD_REQUEST,
                r#"{"error":"The required key does not exist."}"#.to_owned()
            )
        );
        assert_eq!(
            Denial::UnsupportedMediaType("Unsupported media type".to_owned()).status_and_body(),
            (
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                r#"{"detail":"Unsupported media type"}"#.to_owned()
            )
        );
        assert_eq!(
            Denial::NotFound(RESOURCE_NOT_FOUND_BODY.to_owned()).status_and_body(),
            (StatusCode::NOT_FOUND, RESOURCE_NOT_FOUND_BODY.to_owned())
        );
        assert_eq!(
            Denial::Conflict(comment_dup_body("1")).status_and_body().0,
            StatusCode::CONFLICT
        );
        assert_eq!(
            Denial::ServerError.status_and_body(),
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                SERVER_ERROR_BODY.to_owned()
            )
        );
    }

    #[test]
    fn comment_dup_body_bytes() {
        assert_eq!(
            comment_dup_body("6c6c6c6c-0000-4000-8000-000000000000"),
            r#"{"error":"Work item comment with the same external id and external source already exists","id":"6c6c6c6c-0000-4000-8000-000000000000"}"#,
        );
    }

    #[test]
    fn sweep_payload_shape() {
        let (args, kwargs) = soft_delete_sweep("issuelink", "abc");
        assert_eq!(
            args,
            vec![
                Value::String("db".to_owned()),
                Value::String("issuelink".to_owned()),
                Value::String("abc".to_owned()),
            ]
        );
        assert_eq!(kwargs.get("using"), Some(&Value::Null));
        assert_eq!(kwargs.len(), 1);
        assert_eq!(
            SOFT_DELETE_TASK,
            "pi_dash.bgtasks.deletion_task.soft_delete_related_objects"
        );
    }

    #[tokio::test]
    async fn json_response_escapes_u2028() {
        let response = json_response(StatusCode::OK, "  ".to_owned());
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        assert_eq!(bytes.as_ref(), b"\\u2028\\u2029");
    }

    // --- Link PATCH validation (Show partial) ---

    fn utc_tz() -> Tz {
        "UTC".parse().expect("utc")
    }

    fn patch_map(pairs: &[(&str, Value)]) -> Map<String, Value> {
        pairs
            .iter()
            .map(|(key, value)| (key.to_string(), value.clone()))
            .collect()
    }

    #[test]
    fn link_patch_accepts_unvalidated_url() {
        // BUG-2: no `validate_url` on this path.
        let map = patch_map(&[("url", Value::String("not-a-url".to_owned()))]);
        let validated = validate_link_patch(&map, false, &utc_tz()).expect("valid");
        assert_eq!(validated.url.as_deref(), Some("not-a-url"));
        assert!(validated.title.is_none());
    }

    #[test]
    fn link_patch_trims_and_stringifies() {
        let map = patch_map(&[
            ("title", Value::String("  padded  ".to_owned())),
            ("url", Value::from(123)),
        ]);
        let validated = validate_link_patch(&map, false, &utc_tz()).expect("valid");
        assert_eq!(validated.title, Some(Some("padded".to_owned())));
        assert_eq!(validated.url.as_deref(), Some("123"));
    }

    #[test]
    fn link_patch_ignores_readonly_and_unknown() {
        let map = patch_map(&[
            ("id", Value::String("x".to_owned())),
            ("issue", Value::String("y".to_owned())),
            ("created_by", Value::String("z".to_owned())),
            ("nope", Value::from(1)),
        ]);
        let validated = validate_link_patch(&map, false, &utc_tz()).expect("valid");
        assert!(validated.title.is_none());
        assert!(validated.url.is_none());
        assert!(validated.metadata.is_none());
        assert!(validated.deleted_at.is_none());
    }

    #[test]
    fn link_patch_null_title_and_deleted_at() {
        let map = patch_map(&[("title", Value::Null), ("deleted_at", Value::Null)]);
        let validated = validate_link_patch(&map, false, &utc_tz()).expect("valid");
        assert_eq!(validated.title, Some(None));
        assert_eq!(validated.deleted_at, Some(None));
    }

    #[test]
    fn link_patch_deleted_at_parses() {
        let map = patch_map(&[(
            "deleted_at",
            Value::String("2026-10-01T10:00:00Z".to_owned()),
        )]);
        let validated = validate_link_patch(&map, false, &utc_tz()).expect("valid");
        let dt = validated.deleted_at.expect("set").expect("instant");
        assert_eq!(dt.to_rfc3339(), "2026-10-01T10:00:00+00:00");
    }

    #[test]
    fn link_patch_errors_in_model_field_order() {
        // `deleted_at`, `title`, `url`, `metadata` — however the keys arrive.
        let map = patch_map(&[
            ("metadata", Value::Null),
            ("url", Value::String(String::new())),
            ("title", Value::String("x".repeat(300))),
            ("deleted_at", Value::String("garbage".to_owned())),
        ]);
        let body = validate_link_patch(&map, false, &utc_tz()).expect_err("errors");
        let parsed: Value = serde_json::from_str(&body).expect("parses");
        assert_eq!(
            keys_in_order(parsed.as_object().expect("object")),
            ["deleted_at", "title", "url", "metadata"]
                .iter()
                .map(|key| key.to_string())
                .collect::<Vec<_>>()
        );
        assert_eq!(
            body,
            "{\"deleted_at\":[\"Datetime has wrong format. Use one of these formats instead: YYYY-MM-DDThh:mm[:ss[.uuuuuu]][+HH:MM|-HH:MM|Z].\"],\"title\":[\"Ensure this field has no more than 255 characters.\"],\"url\":[\"This field may not be blank.\"],\"metadata\":[\"This field may not be null.\"]}",
        );
    }

    #[test]
    fn link_patch_type_errors() {
        let map = patch_map(&[
            ("title", Value::Bool(true)),
            ("url", Value::Array(vec![])),
            ("deleted_at", Value::from(1)),
        ]);
        let body = validate_link_patch(&map, false, &utc_tz()).expect_err("errors");
        assert_eq!(
            body,
            "{\"deleted_at\":[\"Datetime has wrong format. Use one of these formats instead: YYYY-MM-DDThh:mm[:ss[.uuuuuu]][+HH:MM|-HH:MM|Z].\"],\"title\":[\"Not a valid string.\"],\"url\":[\"Not a valid string.\"]}",
        );
    }

    #[test]
    fn link_patch_nul_rejected() {
        let map = patch_map(&[("title", Value::String("a\0b".to_owned()))]);
        let body = validate_link_patch(&map, false, &utc_tz()).expect_err("errors");
        assert_eq!(body, "{\"title\":[\"Null characters are not allowed.\"]}");
    }

    #[test]
    fn link_patch_non_object_bodies() {
        assert_eq!(
            patch_not_a_dict_body(&Value::Array(vec![Value::from(1)])),
            "{\"non_field_errors\":[\"Invalid data. Expected a dictionary, but got list.\"]}",
        );
        assert_eq!(
            patch_not_a_dict_body(&Value::Null),
            "{\"non_field_errors\":[\"No data provided\"]}",
        );
    }

    #[test]
    fn link_patch_form_arms() {
        // Blank `deleted_at` becomes null; `metadata` arrives as a JSON
        // string; blank `url` still 400s (`get_value` uses the static
        // `required` flag — verified live).
        let map = patch_map(&[
            ("deleted_at", Value::String(String::new())),
            ("metadata", Value::String("{\"a\":1}".to_owned())),
        ]);
        let validated = validate_link_patch(&map, true, &utc_tz()).expect("valid");
        assert!(validated.url.is_none());
        assert_eq!(validated.deleted_at, Some(None));
        assert_eq!(validated.metadata, Some(serde_json::json!({"a": 1})));
        let blank = patch_map(&[("url", Value::String(String::new()))]);
        assert_eq!(
            validate_link_patch(&blank, true, &utc_tz()).expect_err("errors"),
            "{\"url\":[\"This field may not be blank.\"]}",
        );
        let bad = patch_map(&[("metadata", Value::String("{oops".to_owned()))]);
        assert_eq!(
            validate_link_patch(&bad, true, &utc_tz()).expect_err("errors"),
            "{\"metadata\":[\"Value must be valid JSON.\"]}",
        );
    }

    // --- Datetime + override + guard helpers ---

    #[test]
    fn django_datetime_grammar() {
        let utc = utc_tz();
        let instant = |text: &str| {
            parse_django_datetime(text, &utc)
                .expect("parses")
                .to_rfc3339()
        };
        assert_eq!(instant("2026-10-01T10:00:00Z"), "2026-10-01T10:00:00+00:00");
        assert_eq!(
            instant("2026-10-01T12:00:00+02:00"),
            "2026-10-01T10:00:00+00:00"
        );
        assert_eq!(instant("2026-10-01 10:00"), "2026-10-01T10:00:00+00:00");
        assert_eq!(instant("2026-10-01"), "2026-10-01T00:00:00+00:00");
        assert_eq!(
            instant("2026-10-01T10:00:00.123456Z"),
            "2026-10-01T10:00:00.123456+00:00"
        );
        assert_eq!(
            parse_django_datetime("garbage", &utc),
            Err(ParseDatetimeError::Invalid)
        );
        assert_eq!(
            parse_django_datetime("", &utc),
            Err(ParseDatetimeError::Invalid)
        );
        assert_eq!(
            parse_django_datetime("2026-13-01", &utc),
            Err(ParseDatetimeError::Invalid)
        );
        // A nonexistent wall time (spring-forward gap) takes the
        // `make_aware` arm, not the format arm.
        let eastern: Tz = "America/New_York".parse().expect("tz");
        assert_eq!(
            parse_django_datetime("2024-03-10T02:30:00", &eastern),
            Err(ParseDatetimeError::Nonexistent)
        );
        // An ambiguous wall time (fall-back fold) takes the earlier side.
        assert_eq!(
            parse_django_datetime("2024-11-03T01:30:00", &eastern)
                .expect("parses")
                .to_rfc3339(),
            "2024-11-03T05:30:00+00:00"
        );
        // Naive values attach the REQUEST zone on serializer paths.
        assert_eq!(
            parse_django_datetime("2026-10-01T10:00:00", &eastern)
                .expect("parses")
                .to_rfc3339(),
            "2026-10-01T14:00:00+00:00"
        );
    }

    #[test]
    fn created_by_override_arms() {
        let actor = Uuid::parse_str("11111111-1111-4111-8111-111111111111").expect("uuid");
        assert_eq!(override_user_id(None, &actor).expect("ok"), Some(actor));
        assert_eq!(
            override_user_id(Some(&Value::Null), &actor).expect("ok"),
            None
        );
        let other = "22222222-2222-4222-8222-222222222222";
        assert_eq!(
            override_user_id(Some(&Value::String(other.to_owned())), &actor).expect("ok"),
            Some(Uuid::parse_str(other).expect("uuid"))
        );
        assert_eq!(
            override_user_id(Some(&Value::String("garbage".to_owned())), &actor)
                .expect_err("errors"),
            Denial::FieldErrors(VALID_DETAIL_BODY.to_owned())
        );
        // Ints coerce via `UUID(int=…)` and proceed to the save (live
        // Django FK-violates into the payload 400); corrected from the
        // valid-detail pin by live probe in review.
        assert_eq!(
            override_user_id(Some(&Value::from(7)), &actor).expect("int"),
            Some(Uuid::from_u128(7))
        );
    }

    #[test]
    fn created_at_override_arms() {
        let utc = utc_tz();
        // Absent → now (both legs near-identical).
        let (db, text) = override_created_at(None, &utc).expect("now");
        assert!((Utc::now() - db).num_seconds().abs() < 60);
        assert!(text.ends_with('Z'));
        // Aware strings pass through as instants.
        let (db, text) = override_created_at(
            Some(&Value::String("2026-10-01T10:00:00Z".to_owned())),
            &utc,
        )
        .expect("ok");
        assert_eq!(db.to_rfc3339(), "2026-10-01T10:00:00+00:00");
        assert_eq!(text, "2026-10-01T10:00:00Z");
        // A date-only override also echoes verbatim.
        let (db, text) =
            override_created_at(Some(&Value::String("2024-05-01".to_owned())), &utc).expect("ok");
        assert_eq!(db.to_rfc3339(), "2024-05-01T00:00:00+00:00");
        assert_eq!(text, "2024-05-01");
        // Naive strings store UTC but echo verbatim (live Django truth).
        let eastern: Tz = "America/New_York".parse().expect("tz");
        let (db, text) = override_created_at(
            Some(&Value::String("2026-10-01T10:00:00".to_owned())),
            &eastern,
        )
        .expect("ok");
        assert_eq!(db.to_rfc3339(), "2026-10-01T10:00:00+00:00");
        assert_eq!(text, "2026-10-01T10:00:00");
        // Null violates NOT NULL; garbage fails `to_python`.
        assert_eq!(
            override_created_at(Some(&Value::Null), &utc).expect_err("errors"),
            Denial::FieldErrors(PAYLOAD_NOT_VALID_BODY.to_owned())
        );
        assert_eq!(
            override_created_at(Some(&Value::String("garbage".to_owned())), &utc)
                .expect_err("errors"),
            Denial::FieldErrors(VALID_DETAIL_BODY.to_owned())
        );
    }

    #[test]
    fn truthiness_matrix() {
        assert!(!is_truthy(None));
        assert!(!is_truthy(Some(&Value::Null)));
        assert!(!is_truthy(Some(&Value::Bool(false))));
        assert!(!is_truthy(Some(&Value::String(String::new()))));
        assert!(!is_truthy(Some(&Value::from(0))));
        assert!(!is_truthy(Some(&Value::Array(vec![]))));
        assert!(is_truthy(Some(&Value::Bool(true))));
        assert!(is_truthy(Some(&Value::String("x".to_owned()))));
        assert!(is_truthy(Some(&Value::from(7))));
    }

    #[test]
    fn dedupe_pair_arms() {
        // Both truthy strings → the pair.
        let map = patch_map(&[
            ("external_source", Value::String("gh".to_owned())),
            ("external_id", Value::String("1".to_owned())),
        ]);
        assert_eq!(
            dedupe_pair(&map, None),
            Some(("gh".to_owned(), "1".to_owned()))
        );
        // Either falsy → no check.
        let map = patch_map(&[("external_id", Value::String(String::new()))]);
        assert_eq!(dedupe_pair(&map, None), None);
        let map = patch_map(&[("external_id", Value::String("1".to_owned()))]);
        assert_eq!(dedupe_pair(&map, None), None);
        // Patch default: the stored source fills an absent key.
        let map = patch_map(&[("external_id", Value::String("1".to_owned()))]);
        assert_eq!(
            dedupe_pair(&map, Some("gh")),
            Some(("gh".to_owned(), "1".to_owned()))
        );
        // Scalars stringify via `get_prep_value` (`str()`); the serializer
        // then coerces numbers or 400s bools. Corrected from the 500 pin
        // by live probe in review.
        let map = patch_map(&[
            ("external_source", Value::String("gh".to_owned())),
            ("external_id", Value::from(7)),
        ]);
        assert_eq!(
            dedupe_pair(&map, None),
            Some(("gh".to_owned(), "7".to_owned()))
        );
        let map = patch_map(&[
            ("external_source", Value::from(true)),
            ("external_id", Value::String("1".to_owned())),
        ]);
        assert_eq!(
            dedupe_pair(&map, None),
            Some(("True".to_owned(), "1".to_owned()))
        );
        // Composites skip (their filter would miss; the serializer 400s).
        let map = patch_map(&[
            ("external_source", Value::String("gh".to_owned())),
            ("external_id", Value::Array(vec![Value::from(1)])),
        ]);
        assert_eq!(dedupe_pair(&map, None), None);
    }

    #[test]
    fn form_comment_json_arms() {
        let mut map = patch_map(&[
            ("comment_json", Value::String("{\"a\":1}".to_owned())),
            ("access", Value::String(String::new())),
            ("speaker_type", Value::String(String::new())),
            ("comment_html", Value::String(String::new())),
        ]);
        form_comment_json(&mut map, true).expect("ok");
        assert_eq!(map.get("comment_json"), Some(&serde_json::json!({"a": 1})));
        assert!(map.get("access").is_none());
        assert!(map.get("speaker_type").is_none());
        assert_eq!(map.get("comment_html"), Some(&Value::String(String::new())));
        let mut bad = patch_map(&[("comment_json", Value::String("{oops".to_owned()))]);
        assert_eq!(
            form_comment_json(&mut bad, true).expect_err("errors"),
            Denial::FieldErrors("{\"comment_json\":[\"Value must be valid JSON.\"]}".to_owned())
        );
        // JSON bodies pass through untouched.
        let mut json = patch_map(&[("access", Value::String(String::new()))]);
        form_comment_json(&mut json, false).expect("ok");
        assert_eq!(json.get("access"), Some(&Value::String(String::new())));
    }

    #[test]
    fn strip_comment_html_arms() {
        assert_eq!(strip_comment_html(""), "");
        assert_eq!(strip_comment_html("<p>hi</p>"), "hi");
    }

    #[test]
    fn query_helpers() {
        let mut query = QueryMap::new();
        query.insert("fields".to_owned(), OneOrMany::One("id,,url".to_owned()));
        query.insert(
            "cursor".to_owned(),
            OneOrMany::Many(vec!["a".to_owned(), "b".to_owned()]),
        );
        assert_eq!(
            fields_param(&query, "fields"),
            Some(vec!["id".to_owned(), "url".to_owned()])
        );
        assert_eq!(fields_param(&query, "missing"), None);
        assert_eq!(fields_param(&query, "cursor"), Some(vec!["b".to_owned()]));
        assert_eq!(query_last(&query, "cursor").as_deref(), Some("b"));
        assert_eq!(query_last(&query, "missing"), None);
    }

    #[test]
    fn pagination_helpers() {
        use crate::paginator::{Cursor, PageError};
        // First page: everything under the limit, no next page.
        let rows = vec![1, 2];
        let cursor = Cursor::from_string("1000:0:0").expect("cursor");
        let (page, has_more) = window_rows(&rows, 1000, &cursor).expect("window");
        assert_eq!(page, &[1, 2]);
        assert!(!has_more);
        // Over-limit windows trim with `has_more`.
        let rows = vec![1, 2, 3];
        let (page, has_more) = window_rows(&rows, 2, &cursor).expect("window");
        assert_eq!(page, &[1, 2]);
        assert!(has_more);
        // A backwards walk with a mismatched cursor value 500s.
        let cursor = Cursor::from_string("5:0:1").expect("cursor");
        assert!(matches!(
            window_rows(&rows, 2, &cursor),
            Err(Denial::ServerError)
        ));
        // Paginator failures map: parse arms 400, evaluation arms 500.
        assert!(matches!(
            page_denial(PageError::InvalidCursor),
            Denial::BadDetail(_)
        ));
        assert!(matches!(
            page_denial(PageError::ZeroLimit),
            Denial::ServerError
        ));
    }

    // --- Review regressions (live Django truth, review run) ---

    #[test]
    fn link_patch_title_collects_all_validator_errors() {
        // 256 chars + NUL → BOTH errors: DRF `run_validators` extends one
        // list across MaxLength then ProhibitNull (verified live).
        let title = "T".repeat(255) + "\0";
        let map = patch_map(&[("title", Value::String(title))]);
        let body = validate_link_patch(&map, false, &utc_tz()).expect_err("errors");
        assert_eq!(
            body,
            "{\"title\":[\"Ensure this field has no more than 255 characters.\",\"Null characters are not allowed.\"]}"
        );
    }

    #[test]
    fn char_field_uses_python_strip() {
        // `str.strip()` removes U+001C-U+001F; Rust `trim()` does not.
        let map = patch_map(&[("url", Value::String("\u{1c}".to_owned()))]);
        let body = validate_link_patch(&map, false, &utc_tz()).expect_err("blank");
        assert_eq!(body, "{\"url\":[\"This field may not be blank.\"]}");
        assert_eq!(
            char_field_value(&Value::String("\u{1c}x\u{1f}".to_owned())),
            "x"
        );
    }

    #[test]
    fn override_created_at_scalar_arms() {
        // Non-string scalars reach `parse_datetime` raw: `fromisoformat`
        // raises an UNCAUGHT `TypeError` → 500 (verified live).
        for raw in [
            Value::from(5),
            Value::from(true),
            Value::Array(vec![Value::from(1)]),
            Value::Object(Map::new()),
        ] {
            assert!(
                matches!(
                    override_created_at(Some(&raw), &utc_tz()),
                    Err(Denial::ServerError)
                ),
                "{raw:?}"
            );
        }
        // Null violates NOT NULL → 400; garbage strings → 400; absent → now.
        assert!(matches!(
            override_created_at(Some(&Value::Null), &utc_tz()),
            Err(Denial::FieldErrors(body)) if body == PAYLOAD_NOT_VALID_BODY
        ));
        assert!(matches!(
            override_created_at(Some(&Value::String("garbage".to_owned())), &utc_tz()),
            Err(Denial::FieldErrors(body)) if body == VALID_DETAIL_BODY
        ));
        assert!(override_created_at(None, &utc_tz()).is_ok());
    }

    #[test]
    fn override_user_id_int_bool_arms() {
        // `UUIDField.to_python` coerces ints AND bools via `UUID(int=…)`
        // (verified live); the save then FK-violates into the 400.
        let actor = Uuid::nil();
        assert_eq!(
            override_user_id(Some(&Value::from(7)), &actor).expect("int"),
            Some(Uuid::from_u128(7))
        );
        assert_eq!(
            override_user_id(Some(&Value::from(true)), &actor).expect("bool"),
            Some(Uuid::from_u128(1))
        );
        assert_eq!(
            override_user_id(Some(&Value::from(false)), &actor).expect("bool"),
            Some(Uuid::from_u128(0))
        );
        // Negative ints (`ValueError`) and floats (`ValidationError`, the
        // `UUID(hex=…)` `AttributeError` wrapped) → valid-detail 400.
        for raw in [Value::from(-7), Value::from(7.5), Value::from(7.0)] {
            assert!(
                matches!(
                    override_user_id(Some(&raw), &actor),
                    Err(Denial::FieldErrors(body)) if body == VALID_DETAIL_BODY
                ),
                "{raw:?}"
            );
        }
    }

    #[test]
    fn patch_external_guard_arms() {
        let stored_id = Some("extA");
        let stored_source = Some("srcA".to_owned());
        // Falsy id skips.
        let map = patch_map(&[("external_id", Value::String(String::new()))]);
        assert_eq!(
            patch_external_guard(&map, stored_id, stored_source.clone()),
            None
        );
        // `str()`-equal skips — including int 5 vs stored "5".
        let map = patch_map(&[("external_id", Value::String("extA".to_owned()))]);
        assert_eq!(
            patch_external_guard(&map, stored_id, stored_source.clone()),
            None
        );
        let map = patch_map(&[("external_id", Value::from(5))]);
        assert_eq!(
            patch_external_guard(&map, Some("5"), stored_source.clone()),
            None
        );
        let map = patch_map(&[("external_id", Value::from(true))]);
        assert_eq!(
            patch_external_guard(&map, Some("True"), stored_source.clone()),
            None
        );
        // Changed id + absent source inherits the stored source.
        let map = patch_map(&[("external_id", Value::String("extB".to_owned()))]);
        assert_eq!(
            patch_external_guard(&map, stored_id, stored_source.clone()),
            Some((Some("srcA".to_owned()), "extB".to_owned()))
        );
        // Explicit-null source queries IS NULL; empty still filters.
        let map = patch_map(&[
            ("external_id", Value::String("extB".to_owned())),
            ("external_source", Value::Null),
        ]);
        assert_eq!(
            patch_external_guard(&map, stored_id, stored_source.clone()),
            Some((None, "extB".to_owned()))
        );
        let map = patch_map(&[
            ("external_id", Value::String("extB".to_owned())),
            ("external_source", Value::String(String::new())),
        ]);
        assert_eq!(
            patch_external_guard(&map, stored_id, stored_source.clone()),
            Some((Some(String::new()), "extB".to_owned()))
        );
        // Differing scalars filter via `get_prep_value` (`str()`); the
        // serializer then coerces numbers or 400s bools (verified live).
        let map = patch_map(&[("external_id", Value::from(6))]);
        assert_eq!(
            patch_external_guard(&map, stored_id, None),
            Some((None, "6".to_owned()))
        );
        let map = patch_map(&[
            ("external_id", Value::String("extB".to_owned())),
            ("external_source", Value::from(7)),
        ]);
        assert_eq!(
            patch_external_guard(&map, stored_id, None),
            Some((Some("7".to_owned()), "extB".to_owned()))
        );
        // Composites skip (their filter would miss; the serializer 400s).
        for raw in [
            Value::Array(vec![Value::from(1)]),
            Value::Object(Map::new()),
        ] {
            let map = patch_map(&[("external_id", raw.clone())]);
            assert_eq!(patch_external_guard(&map, stored_id, None), None, "{raw:?}");
            let map = patch_map(&[
                ("external_id", Value::String("extB".to_owned())),
                ("external_source", raw.clone()),
            ]);
            assert_eq!(patch_external_guard(&map, stored_id, None), None, "{raw:?}");
        }
    }

    #[test]
    fn window_rows_negative_limit_500s() {
        use crate::paginator::Cursor;
        let rows = vec![1];
        // `results[:-1]` on the lazy queryset → 500 (verified live).
        let cursor = Cursor::from_string("-1:0:0").expect("cursor");
        assert!(matches!(
            window_rows(&rows, -1, &cursor),
            Err(Denial::ServerError)
        ));
        // A negative OFFSET still 400s first (`BadPaginationError`).
        let cursor = Cursor::from_string("-1:1:0").expect("cursor");
        assert!(matches!(
            window_rows(&rows, -1, &cursor),
            Err(Denial::BadDetail(_))
        ));
        // Zero limit still trims (the envelope `max_hits` 500s later).
        let cursor = Cursor::from_string("0:0:0").expect("cursor");
        let (page, _) = window_rows(&rows, 0, &cursor).expect("window");
        assert!(page.is_empty());
    }

    #[test]
    fn fk_violation_arms() {
        #[derive(Debug)]
        struct FakeDbError {
            code: &'static str,
        }
        impl std::fmt::Display for FakeDbError {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "fake {}", self.code)
            }
        }
        impl std::error::Error for FakeDbError {}
        impl sqlx::error::DatabaseError for FakeDbError {
            fn message(&self) -> &str {
                "fake"
            }
            fn code(&self) -> Option<std::borrow::Cow<'_, str>> {
                Some(self.code.into())
            }
            fn as_error(&self) -> &(dyn std::error::Error + Send + Sync + 'static) {
                self
            }
            fn as_error_mut(&mut self) -> &mut (dyn std::error::Error + Send + Sync + 'static) {
                self
            }
            fn into_error(self: Box<Self>) -> Box<dyn std::error::Error + Send + Sync + 'static> {
                self
            }
            fn kind(&self) -> sqlx::error::ErrorKind {
                sqlx::error::ErrorKind::ForeignKeyViolation
            }
        }
        let fk = sqlx::Error::Database(Box::new(FakeDbError { code: "23503" }));
        assert!(is_fk_violation(&fk));
        let other = sqlx::Error::Database(Box::new(FakeDbError { code: "23505" }));
        assert!(!is_fk_violation(&other));
        assert!(!is_fk_violation(&sqlx::Error::RowNotFound));
    }

    #[tokio::test]
    async fn link_expansions_cover_audit_fields() {
        // `expand=created_by|updated_by` supply values (null FKs → `{}` via
        // `None`); without them the shape 500s on `MissingExpansion`.
        let pool = PgPool::connect_lazy("postgres://127.0.0.1:1/nonexistent").expect("lazy pool");
        let decoded = DecodedLink {
            id: "id".to_owned(),
            created_at: "ts".to_owned(),
            updated_at: "ts".to_owned(),
            deleted_at: None,
            title: None,
            url: "https://example.com".to_owned(),
            metadata: Value::Object(Map::new()),
            created_by: None,
            updated_by: None,
            project: "p".to_owned(),
            workspace: "w".to_owned(),
            issue: "i".to_owned(),
        };
        let kept = ["created_by".to_owned(), "updated_by".to_owned()];
        let expansions = link_expansions(
            &pool,
            &decoded,
            "slug",
            &utc_tz(),
            None,
            &["created_by", "updated_by"],
            &kept,
        )
        .await
        .expect("expansions");
        assert_eq!(expansions, vec![("created_by", None), ("updated_by", None)]);
        let value = render_link_value(&decoded, None, &["created_by", "updated_by"], &expansions)
            .expect("renders");
        assert_eq!(value.get("created_by"), Some(&Value::Object(Map::new())));
        assert_eq!(value.get("updated_by"), Some(&Value::Object(Map::new())));
    }

    #[tokio::test]
    async fn comment_expansions_cover_parent() {
        // `expand=parent` with a null parent supplies `None` (renders `{}`).
        let pool = PgPool::connect_lazy("postgres://127.0.0.1:1/nonexistent").expect("lazy pool");
        let decoded = DecodedComment {
            id: "id".to_owned(),
            is_member: None,
            created_at: "ts".to_owned(),
            updated_at: "ts".to_owned(),
            deleted_at: None,
            comment_json: Value::Object(Map::new()),
            comment_html: "<p></p>".to_owned(),
            attachments: Vec::new(),
            labels: Vec::new(),
            access: "INTERNAL".to_owned(),
            external_source: None,
            external_id: None,
            speaker_type: "human".to_owned(),
            speaker_label: String::new(),
            speaker_agent_run_id: None,
            edited_at: None,
            created_by: None,
            updated_by: None,
            project: "p".to_owned(),
            workspace: "w".to_owned(),
            description: None,
            issue: "i".to_owned(),
            actor: None,
            parent: None,
        };
        let kept = ["parent".to_owned()];
        let expansions =
            comment_expansions(&pool, &decoded, "slug", &utc_tz(), None, &["parent"], &kept)
                .await
                .expect("expansions");
        assert_eq!(expansions, vec![("parent", None)]);
        let value =
            render_comment_value(&decoded, None, None, &["parent"], &expansions).expect("renders");
        assert_eq!(value.get("parent"), Some(&Value::Object(Map::new())));
    }

    #[test]
    fn form_comment_json_blank_speaker_clears() {
        // `get_value`: blank + `allow_null`, no `allow_blank` → None.
        let mut map = patch_map(&[("speaker_agent_run_id", Value::String(String::new()))]);
        form_comment_json(&mut map, true).expect("ok");
        assert_eq!(map.get("speaker_agent_run_id"), Some(&Value::Null));
        let mut map = patch_map(&[("speaker_agent_run_id", Value::String(String::new()))]);
        form_comment_json(&mut map, false).expect("ok");
        assert_eq!(
            map.get("speaker_agent_run_id"),
            Some(&Value::String(String::new()))
        );
    }

    #[test]
    fn description_stripped_for_create_keeps_entities() {
        // `Description.save` recomputes with Django's `strip_tags`
        // (verbatim entities), unlike the comment column (verified live).
        assert_eq!(description_stripped_for_create(""), None);
        assert_eq!(
            description_stripped_for_create("<p>R&amp;D</p>").as_deref(),
            Some("R&amp;D")
        );
        assert_eq!(
            description_stripped_for_create("<p>hi</p>").as_deref(),
            Some("hi")
        );
    }

    #[test]
    fn char_field_float_inputs_use_python_spelling() {
        // `str(1e100)` is `1e+100`: stored values, the 255 check and the
        // guard compares all see the Python spelling (PIDASHCONV-758).
        // `serde` keeps non-canonical layouts verbatim (`1.5e+3`, `1e-7`,
        // `0.00001`); only the shared helper spells them like Python.
        let float: Value = serde_json::from_str("1e100").expect("parses");
        assert_eq!(char_field_value(&float), "1e+100");
        assert!(char_field_errors(&float, true, true, Some(255)).is_empty());
        assert_eq!(python_scalar_str(&float).as_deref(), Some("1e+100"));
        let tiny: Value = serde_json::from_str("0.00001").expect("parses");
        assert_eq!(char_field_value(&tiny), "1e-05");
        let sci: Value = serde_json::from_str("1.5e3").expect("parses");
        assert_eq!(char_field_value(&sci), "1500.0");
        assert_eq!(python_scalar_str(&sci).as_deref(), Some("1500.0"));
        let fixed: Value = serde_json::from_str("100.0").expect("parses");
        assert_eq!(char_field_value(&fixed), "100.0");
        let map = patch_map(&[("title", float)]);
        let patch = validate_link_patch(&map, false, &utc_tz()).expect("float title coerces");
        assert_eq!(patch.title, Some(Some("1e+100".to_owned())));
    }

    #[test]
    fn char_field_big_int_renders_full_digits() {
        // Python ints are unbounded: `str(10**30)` is the full digits.
        let big: Value = serde_json::from_str("123456789012345678901234567890").expect("parses");
        assert_eq!(char_field_value(&big), "123456789012345678901234567890");
    }

    #[test]
    fn char_field_number_edge_literals_match_python() {
        // `arbitrary_precision` is always on in this crate: `-0` is int
        // `0`, and overflow floats spell `inf` like Python's `float()`.
        let negzero: Value = serde_json::from_str("-0").expect("parses");
        assert_eq!(char_field_value(&negzero), "0");
        let huge: Value = serde_json::from_str("1e999").expect("parses");
        assert_eq!(char_field_value(&huge), "inf");
        let neghuge: Value = serde_json::from_str("-1e999").expect("parses");
        assert_eq!(char_field_value(&neghuge), "-inf");
    }

    #[test]
    fn form_write_body_requested_data_keeps_first_seen_order() {
        // PIDASHCONV-757: link PATCH / comment writes dump the parsed body
        // with `django_dumps` into the activity `requested_data`. Form keys
        // must keep QueryDict first-seen order, never alphabetical.
        let mut headers = HeaderMap::new();
        headers.insert(
            "content-type",
            "application/x-www-form-urlencoded".parse().unwrap(),
        );
        let raw = b"url=https://example.com/x&title=hello";
        headers.insert("content-length", raw.len().to_string().parse().unwrap());
        let parsed = parse_write_body(&headers, raw).expect("form parses");
        assert!(parsed.from_form);
        let dumped = pidash_jobs::tasks_webhooks::activity_dispatch::django_dumps(&parsed.value);
        assert_eq!(
            dumped,
            r#"{"url": "https://example.com/x", "title": "hello"}"#
        );
    }
}
