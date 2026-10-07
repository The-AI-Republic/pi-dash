//! D-18 work-item core handlers: by-identifier, list/create, detail
//! (handlers A, PIDASHCONV-673).
//!
//! Ports `WorkspaceIssueAPIEndpoint.get` (`api/views/issue.py:244-270`),
//! `IssueListCreateAPIEndpoint.get` (`:339-444`) / `.post` (`:465-540`)
//! and `IssueDetailAPIEndpoint.get` (`:593-614`) / `.patch` (`:790-863`)
//! / `.delete` (`:878-909`), with the six route registrations (three
//! `work-items/` paths plus their deprecated `issues/` twins —
//! `api/urls/work_item.py:44-58,113-127`).
//!
//! `IssueDetailAPIEndpoint.put` (`:639-767`) is unreachable over HTTP:
//! both the old (`:54-58`) and new (`:123-127`) detail routes declare
//! `http_method_names=["get", "patch", "delete"]`, so Django answers PUT
//! with its 405 (`{"detail": "Method \"PUT\" not allowed."}`, pinned by
//! F18-11 `put_405`). The detail cutover owns GET + PATCH + DELETE only
//! and proxies PUT, preserving that 405 byte for byte — no PUT code is
//! ported here (the `put_upsert_*` helpers in `queries_core` belong to
//! PIDASHCONV-668's query layer).
//!
//! The file is self-contained (the `handlers_social` / `handlers_pr_links`
//! precedent): preamble, auth, gates, envelope and body parsing are local
//! so sibling handler issues never conflict on shared helpers. Only
//! [`super::perms`], the `pidash_services::v1_work_items` layer ports and
//! the cross-domain kernels are imported.
//!
//! Deliberate edges (all unpinned — no fixture or contract case sends them):
//!
//! * The orchestration `post_save` signal (`orchestration/signals.py`) is
//!   response-neutral (it swallows every exception) and owned by the
//!   orchestration domain; like the `recent_visited_task` precedent this
//!   port does not invoke it. `resolve_moved_by_run` still runs so its
//!   400s match.
//! * `PUT` on detail proxies to Django (see above).
//! * Non-string `created_at` overrides answer 500, as Django's uncaught
//!   `TypeError` out of `parse_datetime` does.
//!
//! Fixture: `F18-11` (`rust-api/fixtures/v1_work_items/handlers/` —
//! `by_identifier`, `by_identifier_dup_sequence`, `list`, `detail`,
//! `create`, `patch`, `patch_bad_state`, `detail_404`, `create_invalid`,
//! `issue_delete`, `issue_delete_again`, `put_405`, twins). Every `replay_*`
//! test below replays one: status + body byte-identical.
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

use std::collections::HashMap;

use axum::extract::{OriginalUri, Path, Query, State};
use axum::http::{header, HeaderMap, Request, StatusCode};
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, NaiveDate, TimeZone as _, Utc};
use chrono_tz::Tz;
use serde_json::{Map, Value};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use pidash_auth::permissions::project;
use pidash_auth::scope::TenantScope;
use pidash_services::v1_work_items::queries_core as core_queries;
use pidash_services::v1_work_items::shape_issue as shape;
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
/// (`api/views/base.py:149-153`): unparseable-string `created_by` /
/// `created_at` overrides (`UUIDField.to_python` /
/// `DateTimeField.to_python` raise `ValidationError`). Non-numeric
/// `sequence_id` lookups instead raise `ValueError`
/// (`IntegerField.get_prep_value`, Django 6.0) into the 500 branch.
pub const VALID_DETAIL_BODY: &str = r#"{"error":"Please provide valid detail"}"#;
/// Create/patch external-duplicate 409 message (`views/issue.py:505,836`).
pub const EXTERNAL_DUP_MESSAGE: &str =
    "Issue with the same external id and external source already exists";
/// Delete 403 (`views/issue.py:894-897`).
pub const DELETE_DENIAL_MESSAGE: &str = "Only admin or creator can delete the work item";
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
    /// 400, `{"Detail": ...}` (DRF `ParseError`: malformed JSON, bad
    /// `per_page`/`cursor`).
    BadDetail(String),
    /// 400, `{"error": ...}` (view-inline: filter errors, unknown timezones,
    /// run-header errors).
    BadError(String),
    /// 400, serializer `errors` dict (pre-rendered bytes, field order).
    FieldErrors(String),
    /// 415, `{"Detail": ...}` (DRF `UnsupportedMediaType`).
    UnsupportedMediaType(String),
    /// 404, view-inline `{"error": ...}` with the full body.
    NotFound(String),
    /// 403, view-inline body (delete guard).
    ForbiddenBody(String),
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
                r#"{"Detail":"Given API token is not valid"}"#.to_owned(),
            ),
            Denial::Forbidden => (
                StatusCode::FORBIDDEN,
                super::perms::CLASS_DENIAL_BODY.to_owned(),
            ),
            Denial::ProjectNotFound => (
                StatusCode::NOT_FOUND,
                r#"{"Detail":"Project not found"}"#.to_owned(),
            ),
            Denial::BadDetail(message) => (
                StatusCode::BAD_REQUEST,
                format!("{{\"Detail\":{}}}", json_string(message)),
            ),
            Denial::BadError(message) => (
                StatusCode::BAD_REQUEST,
                format!("{{\"error\":{}}}", json_string(message)),
            ),
            Denial::FieldErrors(body) => (StatusCode::BAD_REQUEST, body.clone()),
            Denial::UnsupportedMediaType(message) => (
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                format!("{{\"Detail\":{}}}", json_string(message)),
            ),
            Denial::NotFound(body) => (StatusCode::NOT_FOUND, body.clone()),
            Denial::ForbiddenBody(body) => (StatusCode::FORBIDDEN, body.clone()),
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
    tracing::warn!(%error, site, "v1_work_items core database failure");
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

/// The by-identifier paths own GET (`urls/work_item.py:44-48,113-117`,
/// `as_view(http_method_names=["get"])`).
pub fn owned_by_identifier(
    router: axum::routing::MethodRouter<AppState>,
) -> axum::routing::MethodRouter<AppState> {
    owned(router, &["GET"])
}

/// The list paths own GET + POST (`urls/work_item.py:49-53,118-122`,
/// `as_view(http_method_names=["get", "post"])`).
pub fn owned_issue_list(
    router: axum::routing::MethodRouter<AppState>,
) -> axum::routing::MethodRouter<AppState> {
    owned(router, &["GET", "POST"])
}

/// The detail paths own GET + PATCH + DELETE
/// (`urls/work_item.py:54-58,123-127`,
/// `as_view(http_method_names=["get", "patch", "delete"])`). PUT is
/// deliberately unowned: Django answers the pinned `put_405` body.
pub fn owned_issue_detail(
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
/// `{"Detail":"Project not found"}` 404. Reuses the `v1_projects`
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
/// project-scoped. [`decide`] applies the per-route arm.
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

/// The by-identifier `project_identifier` arm
/// (`app/permissions/project.py:91-98`): active membership on the project
/// carrying `project__identifier`, any role. The joined `projects` row
/// carries no liveness scope — Django never scopes joined tables.
async fn identifier_membership(
    pool: &PgPool,
    workspace_id: &uuid::Uuid,
    user_id: &uuid::Uuid,
    project_identifier: &str,
) -> Result<bool, Denial> {
    sqlx::query_scalar(
        r#"SELECT EXISTS(SELECT 1 FROM "project_members" pm INNER JOIN "projects" p ON p."id" = pm."project_id" WHERE pm."workspace_id" = $1 AND pm."member_id" = $2 AND p."identifier" = $3 AND pm."is_active" AND pm."deleted_at" IS NULL)"#,
    )
    .bind(workspace_id)
    .bind(user_id)
    .bind(project_identifier)
    .fetch_optional(pool)
    .await
    .map_err(|error| db_error(error, "identifier-membership"))
    .map(|found: Option<bool>| found.unwrap_or(false))
}

/// Run the route's gate; deny 403 on failure. All six core routes carry
/// `ProjectEntityPermission` (see [`gate_for`]); the by-identifier routes
/// additionally set the `project_identifier` arm.
async fn require_gate(
    pool: &PgPool,
    workspace_id: &uuid::Uuid,
    workspace_slug: &str,
    user_id: &uuid::Uuid,
    project_id: Option<&uuid::Uuid>,
    project_identifier: Option<&str>,
    route: V1WorkItemsRoute,
    method: &str,
) -> Result<(), Denial> {
    let gate = gate_for(route, method);
    let mut facts = match project_id {
        Some(pid) => {
            entity_facts(pool, workspace_id, workspace_slug, user_id, pid).await?
        }
        None => project::ProjectFacts {
            workspace: pidash_types::WorkspaceId::from(workspace_slug.to_owned()),
            project_id: pidash_types::ProjectId::from(String::new()),
            authenticated: true,
            is_workspace_member: false,
            has_workspace_admin_or_member: false,
            is_workspace_admin: false,
            is_project_member: false,
            is_project_admin: false,
            has_project_admin_or_member: false,
            has_identifier_membership: false,
            has_project_identifier: false,
        },
    };
    // `hasattr(view, "project_identifier") and view.project_identifier`
    // (`project.py:91`): only `WorkspaceIssueAPIEndpoint` carries the
    // property, and only a non-empty identifier arms it.
    if let Some(identifier) = project_identifier {
        if !identifier.is_empty() {
            facts.has_project_identifier = true;
            facts.has_identifier_membership =
                identifier_membership(pool, workspace_id, user_id, identifier).await?;
        }
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

fn row_date_opt(row: &sqlx::postgres::PgRow, column: &str) -> Result<Option<NaiveDate>, Denial> {
    row.try_get(column).map_err(|_| Denial::ServerError)
}

fn row_i32(row: &sqlx::postgres::PgRow, column: &str) -> Result<i32, Denial> {
    row.try_get(column).map_err(|_| Denial::ServerError)
}

fn row_i32_opt(row: &sqlx::postgres::PgRow, column: &str) -> Result<Option<i32>, Denial> {
    row.try_get(column).map_err(|_| Denial::ServerError)
}

fn row_f64(row: &sqlx::postgres::PgRow, column: &str) -> Result<f64, Denial> {
    row.try_get(column).map_err(|_| Denial::ServerError)
}

fn row_bool(row: &sqlx::postgres::PgRow, column: &str) -> Result<bool, Denial> {
    row.try_get(column).map_err(|_| Denial::ServerError)
}

fn row_bytes_opt(row: &sqlx::postgres::PgRow, column: &str) -> Result<Option<Vec<u8>>, Denial> {
    row.try_get(column).map_err(|_| Denial::ServerError)
}

/// A decoded `issues` row: every column the renderer or the write paths
/// read, datetimes already rendered as DRF strings in the request timezone
/// (the `app_issues` precedent — rendering here is a byte-exact
/// passthrough into [`shape::IssueRow`]).
#[derive(Debug, Clone)]
struct DecodedIssue {
    id: String,
    created_at: String,
    created_at_dt: DateTime<Utc>,
    updated_at: String,
    updated_at_dt: DateTime<Utc>,
    deleted_at: Option<String>,
    point: Option<i64>,
    name: String,
    description_html: String,
    description_binary: Option<Vec<u8>>,
    priority: String,
    complexity_score: i64,
    start_date: Option<String>,
    target_date: Option<String>,
    sequence_id: i64,
    sort_order: f64,
    completed_at: Option<String>,
    archived_at: Option<String>,
    is_draft: bool,
    external_source: Option<String>,
    external_id: Option<String>,
    git_work_branch: String,
    created_via: Option<String>,
    agent_executor: Option<String>,
    created_by: Option<String>,
    updated_by: Option<String>,
    project: String,
    workspace: String,
    parent: Option<String>,
    state: Option<String>,
    estimate_point: Option<String>,
    type_id: Option<String>,
    assigned_pod: Option<String>,
    project_identifier: String,
    workspace_slug: String,
    state_group: Option<String>,
}

fn render_dt(dt: &DateTime<Utc>, tz: &Tz) -> String {
    crate::serializer::render_datetime_in(dt, tz)
}

fn render_dt_opt(dt: Option<DateTime<Utc>>, tz: &Tz) -> Option<String> {
    dt.map(|dt| render_dt(&dt, tz))
}

/// Decode one list/detail row selected with [`ISSUE_SELECT_COLS`] plus the
/// `project_identifier` / `workspace_slug` / `state_group` extras.
fn decode_issue(row: &sqlx::postgres::PgRow, tz: &Tz) -> Result<DecodedIssue, Denial> {
    let created_at_dt = row_datetime(row, "created_at")?;
    let updated_at_dt = row_datetime(row, "updated_at")?;
    Ok(DecodedIssue {
        id: row_uuid(row, "id")?.to_string(),
        created_at: render_dt(&created_at_dt, tz),
        created_at_dt,
        updated_at: render_dt(&updated_at_dt, tz),
        updated_at_dt,
        deleted_at: render_dt_opt(row_datetime_opt(row, "deleted_at")?, tz),
        point: row_i32_opt(row, "point")?.map(i64::from),
        name: row_string(row, "name")?,
        description_html: row_string(row, "description_html")?,
        description_binary: row_bytes_opt(row, "description_binary")?,
        priority: row_string(row, "priority")?,
        complexity_score: i64::from(row_i32(row, "complexity_score")?),
        start_date: row_date_opt(row, "start_date")?.map(|d| d.to_string()),
        target_date: row_date_opt(row, "target_date")?.map(|d| d.to_string()),
        sequence_id: i64::from(row_i32(row, "sequence_id")?),
        sort_order: row_f64(row, "sort_order")?,
        completed_at: render_dt_opt(row_datetime_opt(row, "completed_at")?, tz),
        archived_at: row_date_opt(row, "archived_at")?.map(|d| d.to_string()),
        is_draft: row_bool(row, "is_draft")?,
        external_source: row_string_opt(row, "external_source")?,
        external_id: row_string_opt(row, "external_id")?,
        git_work_branch: row_string(row, "git_work_branch")?,
        created_via: row_string_opt(row, "created_via")?,
        agent_executor: row_string_opt(row, "agent_executor")?,
        created_by: row_uuid_opt(row, "created_by_id")?.map(|id| id.to_string()),
        updated_by: row_uuid_opt(row, "updated_by_id")?.map(|id| id.to_string()),
        project: row_uuid(row, "project_id")?.to_string(),
        workspace: row_uuid(row, "workspace_id")?.to_string(),
        parent: row_uuid_opt(row, "parent_id")?.map(|id| id.to_string()),
        state: row_uuid_opt(row, "state_id")?.map(|id| id.to_string()),
        estimate_point: row_uuid_opt(row, "estimate_point_id")?.map(|id| id.to_string()),
        type_id: row_uuid_opt(row, "type_id")?.map(|id| id.to_string()),
        assigned_pod: row_uuid_opt(row, "assigned_pod_id")?.map(|id| id.to_string()),
        project_identifier: row_string(row, "project_identifier")?,
        workspace_slug: row_string(row, "workspace_slug")?,
        state_group: row_string_opt(row, "state_group")?,
    })
}

/// The `issues.*` columns every read path selects (F18-05 column order).
/// `description_json` / `description_stripped` / `workpad` are unrendered
/// and unread here, so they stay out of the select list.
const ISSUE_SELECT_COLS: &str = r#""issues"."id", "issues"."created_at", "issues"."updated_at", "issues"."deleted_at", "issues"."point", "issues"."name", "issues"."description_html", "issues"."description_binary", "issues"."priority", "issues"."complexity_score", "issues"."start_date", "issues"."target_date", "issues"."sequence_id", "issues"."sort_order", "issues"."completed_at", "issues"."archived_at", "issues"."is_draft", "issues"."external_source", "issues"."external_id", "issues"."git_work_branch", "issues"."created_via", "issues"."agent_executor", "issues"."created_by_id", "issues"."updated_by_id", "issues"."project_id", "issues"."workspace_id", "issues"."parent_id", "issues"."state_id", "issues"."estimate_point_id", "issues"."type_id", "issues"."assigned_pod_id""#;

/// The joined extras every read path selects: the URL parts and the state
/// group (the completed-arm check on writes).
const ISSUE_EXTRA_COLS: &str = r#""projects"."identifier" AS "project_identifier", "workspaces"."slug" AS "workspace_slug", "states"."group" AS "state_group""#;

/// One `:named` bind value for [`push_where`].
#[derive(Debug)]
enum WhereBind<'a> {
    Uuid(&'a Uuid),
    Text(&'a str),
    Integer(i64),
    Uuids(&'a [Uuid]),
    Texts(&'a [String]),
}

/// Splice a `:named`-placeholder `WHERE` (the `queries_core` fragments)
/// into a [`sqlx::QueryBuilder`], pushing binds positionally. `IN (:list)`
/// becomes `= ANY($N)` (identical semantics; sqlx binds the whole slice
/// as one array parameter). `::` casts pass through untouched.
fn push_where<'a>(
    qb: &mut sqlx::QueryBuilder<'a, sqlx::Postgres>,
    where_text: &str,
    binds: &HashMap<&str, WhereBind<'a>>,
) -> Result<(), Denial> {
    let rewritten = where_text.replace("IN (:", "= ANY(:");
    let mut rest = rewritten.as_str();
    while let Some(colon) = rest.find(':') {
        let (head, tail) = rest.split_at(colon);
        qb.push(head);
        if tail.starts_with("::") {
            qb.push("::");
            rest = &tail[2..];
            continue;
        }
        let ident: String = tail[1..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        if ident.is_empty() {
            qb.push(":");
            rest = &tail[1..];
            continue;
        }
        match binds.get(ident.as_str()) {
            Some(WhereBind::Uuid(id)) => {
                qb.push_bind(*id);
            }
            Some(WhereBind::Text(text)) => {
                qb.push_bind(*text);
            }
            Some(WhereBind::Integer(number)) => {
                qb.push_bind(*number);
            }
            Some(WhereBind::Uuids(ids)) => {
                qb.push_bind(*ids);
            }
            Some(WhereBind::Texts(texts)) => {
                qb.push_bind(*texts);
            }
            None => return Err(Denial::ServerError),
        }
        rest = &tail[1 + ident.len()..];
    }
    qb.push(rest);
    Ok(())
}

/// Extract the `= ANY($N)` list binds for a compiled [`IssueFilter`].
/// Returns the per-placeholder vectors in a stable struct; unknown keys
/// cannot occur (the compiler is closed) and are skipped defensively.
struct FilterLists {
    states: Vec<Uuid>,
    state_groups: Vec<String>,
    priorities: Vec<String>,
    parents: Vec<Uuid>,
    label_ids: Vec<Uuid>,
    assignee_ids: Vec<Uuid>,
}

fn filter_lists(
    filters: &pidash_db::issue_filters::IssueFilter,
) -> Result<FilterLists, Denial> {
    use pidash_db::issue_filters::FilterValue;
    let mut lists = FilterLists {
        states: Vec::new(),
        state_groups: Vec::new(),
        priorities: Vec::new(),
        parents: Vec::new(),
        label_ids: Vec::new(),
        assignee_ids: Vec::new(),
    };
    for (key, value) in filters.predicates() {
        match (key.as_str(), value) {
            ("state__in", FilterValue::Uuids(ids)) => lists.states.clone_from(ids),
            ("state__group__in", FilterValue::Strings(groups)) => {
                lists.state_groups.clone_from(groups);
            }
            ("priority__in", FilterValue::Strings(priorities)) => {
                lists.priorities.clone_from(priorities);
            }
            ("parent__in", FilterValue::Uuids(ids)) => lists.parents.clone_from(ids),
            ("label_issue__label_id__in", FilterValue::Strings(ids)) => {
                // Resolved ids and `is_uuid`-passing tokens: all parseable.
                for id in ids {
                    lists
                        .label_ids
                        .push(id.parse::<Uuid>().map_err(|_| Denial::ServerError)?);
                }
            }
            ("issue_assignee__assignee_id__in", FilterValue::Strings(ids)) => {
                for id in ids {
                    lists
                        .assignee_ids
                        .push(id.parse::<Uuid>().map_err(|_| Denial::ServerError)?);
                }
            }
            _ => {}
        }
    }
    Ok(lists)
}

/// The extra select expression a `SELECT DISTINCT` list query needs when
/// the `ORDER BY` names a joined column outside the select list (Postgres
/// rejects `DISTINCT` + `ORDER BY` on unselected expressions; Django adds
/// them to the select). Returns `None` when the ordered expression is
/// already selected (every `issues.*` column, the four list annotations,
/// the inlined `CASE`/`MAX` annotations — parens are syntax, not AST —
/// and the three joined extras).
fn order_extra_select(order_by_sql: &str) -> Option<String> {
    let expr = order_by_sql
        .strip_suffix(" ASC")
        .or_else(|| order_by_sql.strip_suffix(" DESC"))
        .unwrap_or(order_by_sql);
    if expr.starts_with("\"issues\".")
        || expr.starts_with("\"sub_issues_count\"")
        || expr.starts_with("\"cycle_id\"")
        || expr.starts_with("\"link_count\"")
        || expr.starts_with("\"attachment_count\"")
        || expr.starts_with("CASE ")
        || expr.starts_with("MAX(")
    {
        return None;
    }
    if expr == "\"projects\".\"identifier\""
        || expr == "\"workspaces\".\"slug\""
        || expr == "\"states\".\"group\""
    {
        return None;
    }
    Some(format!("{expr} AS \"order_extra\""))
}

/// Fetch one list page: the `WHERE` from [`core_queries::list_queryset_where`]
/// plus [`core_queries::filter_where_sql`], the
/// [`core_queries::order_spec`] ordering, and the `[offset, stop)` window.
/// Returns the decoded rows plus the total count. `order_param` is the raw
/// `?order_by=` value (default `-created_at`).
async fn fetch_issue_page(
    pool: &PgPool,
    slug: &str,
    project_id: &Uuid,
    filters: &pidash_db::issue_filters::IssueFilter,
    order_param: &str,
    offset: i64,
    stop: i64,
    tz: &Tz,
) -> Result<(Vec<DecodedIssue>, i64), Denial> {
    use core_queries::OrderSpec;
    let (annotation_sql, joins_sql, order_by_sql, requires_grouping) = match core_queries::order_spec(order_param) {
        OrderSpec::Ordered {
            annotation_sql,
            joins_sql,
            order_by_sql,
            requires_grouping,
        } => (annotation_sql, joins_sql, order_by_sql, requires_grouping),
        // Multi-level `__` paths Django fans out with new joins: the
        // queryset raises `FieldError` into the 500 (ported quirk 12).
        OrderSpec::Unsupported => return Err(Denial::ServerError),
    };
    let mut where_text = core_queries::list_queryset_where();
    let extra = core_queries::filter_where_sql(filters);
    if !extra.is_empty() {
        where_text.push_str(" AND ");
        where_text.push_str(&extra);
    }
    let lists = filter_lists(filters)?;
    let mut binds: HashMap<&str, WhereBind<'_>> = HashMap::new();
    binds.insert("project_id", WhereBind::Uuid(project_id));
    binds.insert("slug", WhereBind::Text(slug));
    binds.insert("states", WhereBind::Uuids(&lists.states));
    binds.insert("state_groups", WhereBind::Texts(&lists.state_groups));
    binds.insert("priorities", WhereBind::Texts(&lists.priorities));
    binds.insert("parents", WhereBind::Uuids(&lists.parents));
    binds.insert("label_ids", WhereBind::Uuids(&lists.label_ids));
    binds.insert("assignee_ids", WhereBind::Uuids(&lists.assignee_ids));

    let filter_joins = core_queries::filter_joins_sql(filters);
    let mut page = sqlx::QueryBuilder::<sqlx::Postgres>::new("SELECT DISTINCT ");
    page.push(ISSUE_SELECT_COLS);
    page.push(", ");
    page.push(core_queries::sub_issues_count_sql());
    page.push(", ");
    page.push(core_queries::cycle_id_sql());
    page.push(", ");
    page.push(core_queries::link_count_sql());
    page.push(", ");
    page.push(core_queries::attachment_count_sql());
    if let Some(annotation) = annotation_sql.as_deref() {
        page.push(", ");
        page.push(annotation);
    }
    if let Some(extra_select) = order_extra_select(&order_by_sql) {
        page.push(", ");
        page.push(extra_select);
    }
    page.push(", ");
    page.push(ISSUE_EXTRA_COLS);
    page.push(" FROM \"issues\" ");
    page.push(core_queries::base_joins_sql());
    if !filter_joins.is_empty() {
        page.push(" ");
        page.push(filter_joins.clone());
    }
    if !joins_sql.is_empty() {
        page.push(" ");
        page.push(joins_sql);
    }
    page.push(" WHERE ");
    push_where(&mut page, &where_text, &binds)?;
    if requires_grouping {
        // The `MAX` branch aggregates over a join: Django groups by the
        // select list; grouping by the row key plus the joined extras
        // yields the same one-group-per-issue (scalar subqueries need no
        // `GROUP BY` membership).
        page.push(" GROUP BY \"issues\".\"id\", \"projects\".\"identifier\", \"workspaces\".\"slug\", \"states\".\"group\"");
    }
    page.push(" ORDER BY ");
    page.push(order_by_sql);
    page.push(" LIMIT ");
    page.push_bind(stop - offset);
    page.push(" OFFSET ");
    page.push_bind(offset);
    let rows: Vec<sqlx::postgres::PgRow> = page
        .build()
        .fetch_all(pool)
        .await
        .map_err(|error| db_error(error, "issue-list-page"))?;
    let mut decoded = Vec::with_capacity(rows.len());
    for row in &rows {
        decoded.push(decode_issue(row, tz)?);
    }

    // `total_count_queryset` (`:397-399`): the manager scope plus the
    // compiled filters, `.distinct()` only when filters are present.
    let (count_where, distinct) = core_queries::total_count_query(filters);
    let mut count = sqlx::QueryBuilder::<sqlx::Postgres>::new("SELECT ");
    if distinct {
        count.push("COUNT(DISTINCT \"issues\".\"id\")");
    } else {
        count.push("COUNT(*)");
    }
    count.push(" FROM \"issues\" ");
    count.push(core_queries::base_joins_sql());
    if !filter_joins.is_empty() {
        count.push(" ");
        count.push(filter_joins);
    }
    count.push(" WHERE ");
    push_where(&mut count, &count_where, &binds)?;
    let total: Option<i64> = count
        .build()
        .fetch_optional(pool)
        .await
        .map_err(|error| db_error(error, "issue-list-count"))?
        .map(|row: sqlx::postgres::PgRow| row.try_get::<i64, _>(0))
        .transpose()
        .map_err(|_| Denial::ServerError)?;
    Ok((decoded, total.unwrap_or(0)))
}

/// Fetch rows for the single-`WHERE` lookups (detail/by-identifier/
/// external/patch/delete): the `issues` columns plus the joined extras,
/// `Meta.ordering` (`-created_at`) like the inline `.get()`s. The caller
/// applies the `.get()` cardinality (0 → 404, 1 → row, 2+ → 500).
async fn fetch_lookup_rows(
    pool: &PgPool,
    joins_sql: &str,
    where_text: &str,
    binds: &HashMap<&str, WhereBind<'_>>,
    tz: &Tz,
) -> Result<Vec<DecodedIssue>, Denial> {
    let mut query = sqlx::QueryBuilder::<sqlx::Postgres>::new("SELECT ");
    query.push(ISSUE_SELECT_COLS);
    query.push(", ");
    query.push(ISSUE_EXTRA_COLS);
    query.push(" FROM \"issues\" ");
    query.push(joins_sql);
    query.push(" WHERE ");
    push_where(&mut query, where_text, binds)?;
    query.push(" ORDER BY ");
    query.push(core_queries::LOOKUP_ORDER_SQL);
    let rows: Vec<sqlx::postgres::PgRow> = query
        .build()
        .fetch_all(pool)
        .await
        .map_err(|error| db_error(error, "issue-lookup"))?;
    rows.iter().map(|row| decode_issue(row, tz)).collect()
}

/// Apply `.get()` cardinality to [`fetch_lookup_rows`] output: exactly one
/// row renders, none 404s (`ObjectDoesNotExist`), several 500
/// (`MultipleObjectsReturned` — the `by_identifier_dup_sequence` pin).
fn only_row(rows: Vec<DecodedIssue>) -> Result<DecodedIssue, Denial> {
    match rows.len() {
        1 => Ok(rows.into_iter().next().expect("one row")),
        0 => Err(Denial::NotFound(RESOURCE_NOT_FOUND_BODY.to_owned())),
        _ => Err(Denial::ServerError),
    }
}

// ---------------------------------------------------------------------------
// Write bodies
// ---------------------------------------------------------------------------

/// The issue-write HTML-input shape: `assignees`/`labels` are `ListField`s
/// (`getlist` arrays); the not-required, not-`allow_blank`, not-`allow_null`
/// scalars skip a present-but-empty form value as absent
/// (`Field.get_value`, `fields.py:407-429`). Dates and PKs keep `''` and
/// coerce it to the null arm; `allow_blank` fields keep `''` as `''`.
const WRITE_BODY_SPEC: crate::v1_cycles_modules::body::BodySpec =
    crate::v1_cycles_modules::body::BodySpec {
        list_fields: &["assignees", "labels"],
        skip_blank_fields: &[
            "priority",
            "complexity_score",
            "sequence_id",
            "sort_order",
            "is_draft",
        ],
    };

/// A parsed write body: the JSON value plus whether it arrived as an HTML
/// form (blank arms differ per `Field.get_value`).
struct WriteBody {
    value: Value,
    from_form: bool,
}

/// Parse an issue write body: content-type dispatch (415 for the rest),
/// empty bodies to `{}`, JSON through the CPython parser, forms through
/// the HTML-input kernel. Uploads are ignored (no file field exists here —
/// an unpinned edge; Python would 400 them as non-strings).
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

/// CPython `type(data).__name__` for the non-dict / non-list messages.
fn json_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "NoneType",
        Value::Bool(_) => "bool",
        Value::Number(number) => {
            if number.is_i64() || number.is_u64() {
                "int"
            } else if number.is_f64() {
                "float"
            } else {
                // Arbitrary-precision integers past `u64` (the `preserve_order`
                // + `arbitrary_precision` build): still Python `int`s.
                "int"
            }
        }
        Value::String(_) => "str",
        Value::Array(_) => "list",
        Value::Object(_) => "dict",
    }
}

/// Minimal CPython `str()` for error interpolation (`ChoiceField` misses,
/// UUID `"..." is not a valid UUID`): strings verbatim, numbers via
/// [`python_number_str`], `True`/`False`/`None`, composites via CPython
/// `repr` (single quotes, `True`/`False`/`None`, insertion order).
fn py_repr(value: &Value) -> String {
    match value {
        Value::Null => "None".to_owned(),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::Number(number) => python_number_str(number),
        Value::String(text) => text.clone(),
        Value::Array(items) => {
            let inner: Vec<String> = items.iter().map(py_repr_quoted).collect();
            format!("[{}]", inner.join(", "))
        }
        Value::Object(map) => {
            let inner: Vec<String> = map
                .iter()
                .map(|(key, item)| format!("{}: {}", py_repr_quoted(&Value::String(key.clone())), py_repr_quoted(item)))
                .collect();
            format!("{{{}}}", inner.join(", "))
        }
    }
}

/// CPython `repr()` of one value (see [`py_repr`]): strings single-quoted
/// with backslash/`'` escapes, everything else like [`py_repr`].
fn py_repr_quoted(value: &Value) -> String {
    match value {
        Value::String(text) => {
            // CPython picks single quotes unless the string holds one (then
            // double, escaping embedded doubles). Non-printables escape as
            // `\x..`/`\u....`; keep the common arms exact.
            let mut escaped = String::with_capacity(text.len() + 2);
            for ch in text.chars() {
                match ch {
                    '\\' => escaped.push_str("\\\\"),
                    '\n' => escaped.push_str("\\n"),
                    '\r' => escaped.push_str("\\r"),
                    '\t' => escaped.push_str("\\t"),
                    c if c.is_control() => {
                        let code = c as u32;
                        if code <= 0xff {
                            escaped.push_str(&format!("\\x{code:02x}"));
                        } else if code <= 0xffff {
                            escaped.push_str(&format!("\\u{code:04x}"));
                        } else {
                            escaped.push_str(&format!("\\U{code:08x}"));
                        }
                    }
                    c => escaped.push(c),
                }
            }
            if !text.contains('\'') || text.contains('"') && !text.contains('\'') {
                format!("'{escaped}'")
            } else if !text.contains('"') {
                format!("\"{}\"", escaped.replace('"', "\\\""))
            } else {
                format!("'{}'", escaped.replace('\'', "\\'"))
            }
        }
        Value::Array(items) => {
            let inner: Vec<String> = items.iter().map(py_repr_quoted).collect();
            format!("[{}]", inner.join(", "))
        }
        Value::Object(map) => {
            let inner: Vec<String> = map
                .iter()
                .map(|(key, item)| format!("{}: {}", py_repr_quoted(&Value::String(key.clone())), py_repr_quoted(item)))
                .collect();
            format!("{{{}}}", inner.join(", "))
        }
        _ => py_repr(value),
    }
}

/// Run `normalize_description_input` the way the views do (before the
/// serializer exists), with the exact non-dict control flow: `in` on
/// `None`/int/float/bool raises `TypeError` → 500; `dict(data)` on a
/// non-mapping raises `ValueError` → 500. Only mappings (and
/// markdown-key-free strings/lists) reach the serializer.
fn normalize_view_body(
    value: &Value,
    convert: &dyn Fn(&str) -> Result<String, String>,
) -> Result<(Value, bool), Denial> {
    use shape::{LegacyInput, MarkdownInput};
    // Python `in`: dicts test keys, strings test substrings, lists test
    // element equality, everything else raises `TypeError`.
    let contains = |key: &str| -> Result<bool, Denial> {
        match value {
            Value::Object(map) => Ok(map.contains_key(key)),
            Value::String(text) => Ok(text.contains(key)),
            Value::Array(items) => Ok(items.iter().any(|item| item == &Value::String(key.to_owned()))),
            _ => Err(Denial::ServerError),
        }
    };
    if !contains("description_markdown")? && !contains("description")? {
        return Ok((value.clone(), false));
    }
    // `dict(data)`: mappings pass through, everything else 500s.
    let Value::Object(map) = value else {
        return Err(Denial::ServerError);
    };
    let markdown = match map.get("description_markdown") {
        None => MarkdownInput::Absent,
        Some(Value::Null) => MarkdownInput::Null,
        Some(Value::String(text)) => MarkdownInput::Text(text),
        Some(_) => MarkdownInput::NonString,
    };
    let legacy = match map.get("description") {
        None => LegacyInput::Absent,
        Some(Value::Null) => LegacyInput::Null,
        Some(Value::String(text)) => LegacyInput::Text(text),
        Some(_) => LegacyInput::NonString,
    };
    let html_present = map.contains_key("description_html");
    match shape::normalize_description_input(markdown, legacy, html_present, convert) {
        Ok(outcome) => {
            use shape::NormalizeAction;
            let mut next = map.clone();
            match outcome.action {
                NormalizeAction::Unchanged => {}
                NormalizeAction::DropKeys => {
                    next.shift_remove("description_markdown");
                    next.shift_remove("description");
                }
                NormalizeAction::SetHtml(html) => {
                    next.shift_remove("description_markdown");
                    next.shift_remove("description");
                    next.insert("description_html".to_owned(), Value::String(html));
                }
            }
            Ok((Value::Object(next), outcome.from_markdown))
        }
        Err(error) => Err(Denial::FieldErrors(error.body())),
    }
}

/// The activity `requested_data` text for a normalized write body: form
/// bodies dump list fields as `QueryDict` last-wins scalars, JSON bodies
/// dump as-is (arrays are real data there).
fn requested_data_text(normalized: &Value) -> String {
    pidash_jobs::tasks_webhooks::activity_dispatch::django_dumps(normalized)
}

/// Project a form body the way `normalize_description_input` sees it:
/// `data.dict()` takes the LAST value per key — the kernel's list-field
/// arrays collapse to last-wins scalars before normalize runs.
fn form_dict_for_normalize(value: &Value) -> Value {
    let mut projected = value.clone();
    if let Value::Object(map) = &mut projected {
        crate::v1_cycles_modules::body::project_list_scalars(map, WRITE_BODY_SPEC.list_fields);
    }
    projected
}

// ---------------------------------------------------------------------------
// DRF scalar fields
// ---------------------------------------------------------------------------

/// Python `str.strip()` for the blank check and `CharField` coercion (the
/// `runner_runs` kernel — full Unicode whitespace).
fn py_strip(text: &str) -> &str {
    crate::runner_runs::runs::py_strip(text)
}

/// DRF `CharField` failure messages, in check order: blank → null →
/// invalid → max_length → null-characters (`fields.py` `CharField`,
/// `ProhibitNullCharactersValidator`). `stripped` reports whether the blank
/// arm trims (every model `CharField`/`TextField` here trims).
fn char_field_errors(
    value: &Value,
    allow_blank: bool,
    allow_null: bool,
    max_length: Option<usize>,
) -> Vec<String> {
    // `run_validation` tests blank before the null arm
    // (`data == '' or (trim_whitespace and str(data).strip() == '')`).
    if let Value::String(text) = value {
        if (text.is_empty() || py_strip(text).is_empty()) && !allow_blank {
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
            let stripped = py_strip(text);
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

/// The `CharField` blank/null/invalid arms only (no validators): for
/// fields with model validators, which run before `MaxLengthValidator`.
fn char_shape_errors(value: &Value, allow_blank: bool, allow_null: bool) -> Vec<String> {
    if let Value::String(text) = value {
        if (text.is_empty() || py_strip(text).is_empty()) && !allow_blank {
            return vec!["This field may not be blank.".to_owned()];
        }
        return Vec::new();
    }
    if value.is_null() {
        if !allow_null {
            return vec!["This field may not be null.".to_owned()];
        }
        return Vec::new();
    }
    match value {
        Value::Number(_) => Vec::new(),
        _ => vec!["Not a valid string.".to_owned()],
    }
}

/// Coerce a validated string input (`str(data)` + strip; numbers stringify).
fn char_field_value(value: &Value) -> String {
    match value {
        Value::String(text) => py_strip(text).to_owned(),
        Value::Number(number) => python_number_str(number),
        _ => String::new(),
    }
}

/// Parse a JSON value the way `int(re_decimal.sub('', str(data)))` does
/// (`IntegerField.to_internal_value`, `fields.py:910-918`): an optional
/// `.0*` + whitespace suffix is stripped, then Python `int()` runs
/// (whitespace, sign, digits with single underscores between digits).
/// Returns the unbounded value; out-of-`i64` magnitudes saturate (the
/// min/max arms keep their verdicts; `sequence_id` carries its own
/// overflow flag for the save-time 500).
fn parse_drf_int(value: &Value) -> Result<i64, &'static str> {
    parse_drf_int_full(value).map(|(number, _)| number)
}

/// [`parse_drf_int`] plus the save-time overflow flag: true when the
/// validated magnitude exceeds `i64` (Python validates it fine; the
/// `int4` column raises at save → 500).
fn parse_drf_int_full(value: &Value) -> Result<(i64, bool), &'static str> {
    const MAX_STRING_LENGTH: usize = 1000;
    if let Value::String(text) = value {
        if text.len() > MAX_STRING_LENGTH {
            return Err("String value too large.");
        }
    }
    let text: String = match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => python_number_str(number),
        _ => return Err("A valid integer is required."),
    };
    // `re_decimal = re.compile(r'\.0*\s*$')`: only a trailing `.0*`
    // suffix strips (the `$` anchors; `re.sub` scans but only the last
    // dot can match to the end).
    let mut core = text.as_str();
    if let Some(dot) = core.rfind('.') {
        let (head, tail) = core.split_at(dot);
        let tail = &tail[1..];
        let mut zeros = tail;
        while zeros.starts_with('0') {
            zeros = &zeros[1..];
        }
        if py_strip(zeros).is_empty() {
            core = head;
        }
    }
    let number = parse_python_int(core).ok_or("A valid integer is required.")?;
    Ok((number, python_int_overflows_i64(core)))
}

/// Python `int(text, 10)`: surrounding whitespace, an optional sign, then
/// digits with single `_` separators between digits only. Saturates past
/// `i64` (see [`parse_drf_int`]).
fn parse_python_int(text: &str) -> Option<i64> {
    // `int()` strips exactly `str.strip()`.
    let text = py_strip(text);
    let (negative, digits) = match text.strip_prefix(['+', '-']) {
        Some(rest) => (text.starts_with('-'), rest),
        None => (false, text),
    };
    if digits.is_empty() {
        return None;
    }
    // Single underscores between digits only (`int('1__0')` fails).
    let mut cleaned = String::with_capacity(digits.len());
    let mut prev_underscore = true;
    for ch in digits.chars() {
        if ch == '_' {
            if prev_underscore {
                return None;
            }
            prev_underscore = true;
            continue;
        }
        if !ch.is_ascii_digit() {
            return None;
        }
        prev_underscore = false;
        cleaned.push(ch);
    }
    if prev_underscore {
        return None;
    }
    let magnitude: i128 = cleaned.parse().ok()?;
    let signed = if negative { -magnitude } else { magnitude };
    Some(signed.clamp(i64::MIN as i128, i64::MAX as i128) as i64)
}

/// Whether `parse_python_int` saturated (the magnitude exceeds `i64`):
/// the save-time overflow arm for `sequence_id`.
fn python_int_overflows_i64(text: &str) -> bool {
    let text = py_strip(text);
    let digits = text.strip_prefix(['+', '-']).unwrap_or(text);
    let cleaned: String = digits.chars().filter(|ch| *ch != '_').collect();
    let trimmed = cleaned.trim_start_matches('0');
    if trimmed.len() > 19 {
        return true;
    }
    if trimmed.len() < 19 {
        return false;
    }
    if text.starts_with('-') {
        trimmed > "9223372036854775808"
    } else {
        trimmed > "9223372036854775807"
    }
}

/// Parse a JSON value the way `float(data)` does
/// (`FloatField.to_internal_value`, `fields.py:947-957`). Gigantic ints
/// overflow (`OverflowError`); `inf`/`nan` spellings are valid (the render
/// layer 500s them, per the S1 contract).
fn parse_drf_float(value: &Value) -> Result<f64, &'static str> {
    const MAX_STRING_LENGTH: usize = 1000;
    match value {
        Value::Bool(true) => return Ok(1.0),
        Value::Bool(false) => return Ok(0.0),
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                return Ok(int as f64);
            }
            if let Some(uint) = number.as_u64() {
                return Ok(uint as f64);
            }
            if number.is_f64() {
                // JSON `1e999` parses to inf in Python too — valid here.
                return Ok(number.as_f64().unwrap_or(f64::NAN));
            }
            // Arbitrary-precision integers past `u64`: `float()` overflows
            // past `f64::MAX`, else converts.
            let raw = number.to_string();
            match raw.parse::<f64>() {
                Ok(finite) if finite.is_finite() => return Ok(finite),
                _ => return Err("Integer value too large to convert to float"),
            }
        }
        Value::String(text) => {
            if text.len() > MAX_STRING_LENGTH {
                return Err("String value too large.");
            }
            parse_python_float(text).ok_or("A valid number is required.")
        }
        _ => Err("A valid number is required."),
    }
}

/// Python `float(text)`: whitespace, sign, digits with underscores,
/// fractions, exponents, `inf`/`infinity`/`nan` in any case.
fn parse_python_float(text: &str) -> Option<f64> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let lowered = text.to_ascii_lowercase();
    for special in ["inf", "infinity", "nan"] {
        if lowered == special || lowered == format!("+{special}") || lowered == format!("-{special}") {
            let negative = lowered.starts_with('-');
            return Some(match special {
                "nan" => f64::NAN,
                _ if negative => f64::NEG_INFINITY,
                _ => f64::INFINITY,
            });
        }
    }
    // Underscores between digits/around the point only; Rust parses the rest.
    let mut cleaned = String::with_capacity(text.len());
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let ch = bytes[index] as char;
        if ch == '_' {
            let prev = index.checked_sub(1).map(|i| bytes[i] as char);
            let next = bytes.get(index + 1).map(|b| *b as char);
            let ok_prev = matches!(prev, Some(c) if c.is_ascii_digit() || c == '.');
            let ok_next = matches!(next, Some(c) if c.is_ascii_digit() || c == '.');
            if !(ok_prev && ok_next) {
                return None;
            }
            index += 1;
            continue;
        }
        cleaned.push(ch);
        index += 1;
    }
    cleaned.parse::<f64>().ok()
}

/// Parse a JSON value the way `BooleanField.to_internal_value` does
/// (`fields.py:700-707`): the exact `TRUE_VALUES` / `FALSE_VALUES` sets
/// (note `0.0` is false but `1.0` is invalid, and `1`/`0` are ints only).
fn parse_drf_bool(value: &Value) -> Result<bool, &'static str> {
    const INVALID: &str = "Must be a valid boolean.";
    match value {
        Value::Bool(flag) => Ok(*flag),
        Value::Number(number) => {
            if number.as_i64() == Some(1) {
                return Ok(true);
            }
            if number.as_i64() == Some(0) {
                return Ok(false);
            }
            if number.as_f64() == Some(0.0) {
                return Ok(false);
            }
            Err(INVALID)
        }
        Value::String(text) => {
            match text.to_ascii_lowercase().as_str() {
                "t" | "y" | "yes" | "true" | "on" | "1" => Ok(true),
                "f" | "n" | "no" | "false" | "off" | "0" => Ok(false),
                _ => Err(INVALID),
            }
        }
        _ => Err(INVALID),
    }
}

// ---------------------------------------------------------------------------
// DRF date / datetime / choice / pk fields
// ---------------------------------------------------------------------------

/// `DateField` invalid message (`fields.py:1219` over
/// `DATE_INPUT_FORMATS = [iso-8601]`).
const DATE_INVALID_MESSAGE: &str = "Date has wrong format. Use one of these formats instead: YYYY-MM-DD.";
/// `DateTimeField` invalid message (`fields.py:1129` over
/// `DATETIME_INPUT_FORMATS = [iso-8601]`).
const DATETIME_INVALID_MESSAGE: &str =
    "Datetime has wrong format. Use one of these formats instead: YYYY-MM-DDThh:mm[:ss[.uuuuuu]][+HH:MM|-HH:MM|Z].";

/// Parse a JSON value the way `DateField.to_internal_value` does
/// (`fields.py:1231-1258`): `parse_date` (fromisoformat, then the
/// `date_re` fallback) or the invalid message. Non-strings always miss
/// (`TypeError` suppressed).
fn parse_drf_date(value: &Value) -> Result<NaiveDate, &'static str> {
    match value {
        Value::String(text) => parse_django_date(text).ok_or(DATE_INVALID_MESSAGE),
        _ => Err(DATE_INVALID_MESSAGE),
    }
}

/// `django.utils.dateparse.parse_date` (Django 4.2.30, `dateparse.py:67-78`):
/// `date.fromisoformat` first (extended, basic, ordinal and week dates),
/// then the `date_re` fallback (`YYYY-M-D`, 1-2 digit month/day). Well
/// formatted but impossible (month 13, Feb 30, year 0) is `None` too.
fn parse_django_date(text: &str) -> Option<NaiveDate> {
    fromisodate(text).or_else(|| date_re_fallback(text))
}

/// Python 3.12 `date.fromisoformat`: the four spellings, strictly.
fn fromisodate(text: &str) -> Option<NaiveDate> {
    let bytes = text.as_bytes();
    // Extended `YYYY-MM-DD`.
    if bytes.len() == 10 && bytes[4] == b'-' && bytes[7] == b'-' {
        let year: i32 = text[0..4].parse().ok()?;
        let month: u32 = text[5..7].parse().ok()?;
        let day: u32 = text[8..10].parse().ok()?;
        return NaiveDate::from_ymd_opt(year, month, day);
    }
    // Basic `YYYYMMDD`.
    if bytes.len() == 8 && bytes.iter().all(|b| b.is_ascii_digit()) {
        let year: i32 = text[0..4].parse().ok()?;
        let month: u32 = text[4..6].parse().ok()?;
        let day: u32 = text[6..8].parse().ok()?;
        return NaiveDate::from_ymd_opt(year, month, day);
    }
    // Ordinal `YYYY-DDD` / `YYYYDDD`.
    if bytes.len() >= 7 && bytes[0..4].iter().all(|b| b.is_ascii_digit()) && bytes[4] != b'W' {
        let rest = &text[4..];
        if let Some(ordinal) = ordinal_day(rest) {
            let year: i32 = text[0..4].parse().ok()?;
            return NaiveDate::from_yo_opt(year, ordinal);
        }
    }
    // Week `YYYY-Www-D` / `YYYYWwwD` (dash-consistent spellings only).
    isoweek_date(text)
}

/// The ordinal day of a `YYYY-DDD` / `YYYYDDD` tail, if well formed.
fn ordinal_day(rest: &str) -> Option<u32> {
    if let Some(day) = rest.strip_prefix('-') {
        if day.len() == 3 && day.bytes().all(|b| b.is_ascii_digit()) {
            return day.parse().ok();
        }
        return None;
    }
    if rest.len() == 3 && rest.bytes().all(|b| b.is_ascii_digit()) {
        return rest.parse().ok();
    }
    None
}

/// Python 3.12 week-date `fromisoformat`: `YYYY-Www-D` or `YYYYWwwD`.
fn isoweek_date(text: &str) -> Option<NaiveDate> {
    let bytes = text.as_bytes();
    let (year, week, weekday) = if bytes.len() == 10 && bytes[4] == b'-' && bytes[5] == b'W' && bytes[8] == b'-' {
        (
            text[0..4].parse::<i32>().ok()?,
            text[6..8].parse::<u32>().ok()?,
            text[9..10].parse::<u32>().ok()?,
        )
    } else if bytes.len() == 8 && bytes[0..4].iter().all(|b| b.is_ascii_digit()) && bytes[4] == b'W' {
        (
            text[0..4].parse::<i32>().ok()?,
            text[5..7].parse::<u32>().ok()?,
            text[7..8].parse::<u32>().ok()?,
        )
    } else {
        return None;
    };
    let weekday = chrono::Weekday::try_from(u8::try_from(weekday.saturating_sub(1)).ok()?).ok()?;
    NaiveDate::from_isoywd_opt(year, week, weekday)
}

/// The `date_re` fallback (`dateparse.py:13,76-78`):
/// `^\d{4}-\d{1,2}-\d{1,2}$` then the constructor (which rejects
/// impossible dates and year 0).
fn date_re_fallback(text: &str) -> Option<NaiveDate> {
    let mut parts = text.split('-');
    let (year, month, day) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() {
        return None;
    }
    if year.len() != 4 || !year.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if !(1..=2).contains(&month.len()) || !month.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if !(1..=2).contains(&day.len()) || !day.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    NaiveDate::from_ymd_opt(year.parse().ok()?, month.parse().ok()?, day.parse().ok()?)
}

/// Parse a JSON value the way `DateTimeField.to_internal_value` +
/// `enforce_timezone` do (`fields.py:1145-1190`): `parse_datetime`, then
/// naive inputs take the request timezone (`USE_TZ`, the activated user
/// zone) and aware inputs keep their instant. Returns the UTC instant.
fn parse_drf_datetime(value: &Value, tz: &Tz, tz_name: &str) -> Result<DateTime<Utc>, String> {
    let text = match value {
        Value::String(text) => text.as_str(),
        _ => return Err(DATETIME_INVALID_MESSAGE.to_owned()),
    };
    let parsed = crate::v1_cycles_modules::cycle::parse_iso_datetime(text)
        .ok_or_else(|| DATETIME_INVALID_MESSAGE.to_owned())?;
    if let Some(offset_micros) = parsed.offset_micros {
        let utc = parsed.naive - chrono::Duration::microseconds(offset_micros);
        return Ok(chrono::DateTime::<Utc>::from_naive_utc_and_offset(utc, Utc));
    }
    // Naive: `make_aware` in the request zone. Gaps answer the
    // `make_aware` message; folds take the earlier side (fold=0).
    match tz.from_local_datetime(&parsed.naive) {
        chrono::LocalResult::Single(aware) => Ok(aware.with_timezone(&Utc)),
        chrono::LocalResult::Ambiguous(first, _) => Ok(first.with_timezone(&Utc)),
        chrono::LocalResult::None => Err(format!("Invalid datetime for the timezone \"{tz_name}\".")),
    }
}

/// Parse a JSON value the way `ChoiceField.to_internal_value` does
/// (`fields.py:1400-1408`): `str(data)` must be a choice key. The caller
/// handles `""` first (`allow_blank` returns it verbatim). The miss
/// message echoes the raw input (`"{input}" is not a valid choice.`).
fn parse_drf_choice<'a>(value: &Value, choices: &[&'a str]) -> Result<&'a str, String> {
    let key = match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => python_number_str(number),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::Null => "None".to_owned(),
        Value::Array(_) | Value::Object(_) => py_repr(value),
    };
    match choices.iter().find(|choice| **choice == key) {
        Some(choice) => Ok(*choice),
        None => Err(format!("\"{key}\" is not a valid choice.")),
    }
}

/// `uuid.UUID(value)` for the `hex`/`int` input forms
/// (`django/db/models/fields/__init__.py:2684-2691`): braces, `urn:uuid:`
/// prefixes and hyphens-anywhere strip, then 32 hex chars; ints take the
/// `int=` form (negative or >2¹²⁸ fails). Anything else fails exactly like
/// Django's `ValidationError` arm.
fn parse_uuid_input(value: &Value) -> Result<Uuid, ()> {
    match value {
        Value::String(text) => parse_uuid_hex(text).ok_or(()),
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                if int < 0 {
                    return Err(());
                }
                return Ok(Uuid::from_u128(int as u128));
            }
            if let Some(uint) = number.as_u64() {
                return Ok(Uuid::from_u128(u128::from(uint)));
            }
            // Arbitrary-precision integers: the `int=` form up to 2¹²⁸-1.
            if !number.is_f64() {
                if let Ok(big) = number.to_string().parse::<u128>() {
                    return Ok(Uuid::from_u128(big));
                }
            }
            Err(())
        }
        _ => Err(()),
    }
}

/// The `hex` form of `uuid.UUID` (CPython 3.12 `uuid.py`): every `urn:`
/// and `uuid:` substring is removed (case-sensitive), brace characters
/// strip from both ends, hyphens strip anywhere, then exactly 32 hex
/// digits (case-insensitive) must remain. No whitespace stripping.
fn parse_uuid_hex(text: &str) -> Option<Uuid> {
    let mut hex = text.replace("urn:", "").replace("uuid:", "");
    hex = hex.trim_matches(|ch| ch == '{' || ch == '}').replace('-', "");
    if hex.len() != 32 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    Uuid::parse_str(&hex).ok()
}

/// Django's `“%(value)s” is not a valid UUID.` field message for a PK
/// input that fails `to_python` (fancy quotes verbatim).
fn invalid_uuid_message(value: &Value) -> String {
    match value {
        Value::String(text) => format!("\u{201c}{text}\u{201d} is not a valid UUID."),
        _ => format!("\u{201c}{}\u{201d} is not a valid UUID.", py_repr(value)),
    }
}

// ---------------------------------------------------------------------------
// Issue write parsing (`IssueSerializer.to_internal_value`)
// ---------------------------------------------------------------------------

/// The validated write: every writable field as `Option` (`None` = absent;
/// nullable fields nest a second `Option` for explicit null). Defaults are
/// applied by the caller on create only (partial skips absent keys).
#[derive(Debug, Clone, Default)]
struct ParsedWrite {
    assignees: Option<Vec<Uuid>>,
    labels: Option<Vec<Uuid>>,
    issue_type: Option<Option<Uuid>>,
    deleted_at: Option<Option<DateTime<Utc>>>,
    point: Option<Option<i64>>,
    name: Option<String>,
    description_html: Option<String>,
    priority: Option<String>,
    complexity_score: Option<i64>,
    start_date: Option<Option<NaiveDate>>,
    target_date: Option<Option<NaiveDate>>,
    sequence_id: Option<i64>,
    sequence_overflow: bool,
    sort_order: Option<f64>,
    completed_at: Option<Option<DateTime<Utc>>>,
    archived_at: Option<Option<NaiveDate>>,
    is_draft: Option<bool>,
    external_source: Option<Option<String>>,
    external_id: Option<Option<String>>,
    git_work_branch: Option<String>,
    created_via: Option<Option<String>>,
    agent_executor: Option<Option<String>>,
    created_by: Option<Option<Uuid>>,
    parent: Option<Option<Uuid>>,
    state: Option<Option<Uuid>>,
    estimate_point: Option<Option<Uuid>>,
    assigned_pod: Option<Option<Uuid>>,
}

/// One queued existence probe: the parsed UUID plus the raw echo for the
/// miss message.
#[derive(Debug, Clone)]
struct PkProbe {
    id: Uuid,
    raw: String,
}

/// The tables `PrimaryKeyRelatedField` existence checks read (each with
/// its Python manager scope).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum PkTable {
    Users,
    Labels,
    IssueTypes,
    States,
    Issues,
    EstimatePoints,
    Pods,
}

/// Batch one table's probes: returns the ids that exist under the
/// field's manager scope.
async fn probe_exists(pool: &PgPool, table: PkTable, ids: &[Uuid]) -> Result<Vec<Uuid>, Denial> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let sql = match table {
        // `User.objects` is Django's plain `UserManager` (no liveness
        // scope — deactivated users still validate).
        PkTable::Users => r#"SELECT "id" FROM "users" WHERE "id" = ANY($1)"#,
        PkTable::Labels => r#"SELECT "id" FROM "labels" WHERE "id" = ANY($1) AND "deleted_at" IS NULL"#,
        PkTable::IssueTypes => {
            r#"SELECT "id" FROM "issue_types" WHERE "id" = ANY($1) AND "deleted_at" IS NULL"#
        }
        // `State.objects` is the triage-excluding `StateManager`
        // (`db/models/state.py:91-100`): triage ids miss here (field
        // error), never reaching `validate()`.
        PkTable::States => {
            r#"SELECT "id" FROM "states" WHERE "id" = ANY($1) AND "deleted_at" IS NULL AND NOT ("group" = 'triage')"#
        }
        PkTable::Issues => r#"SELECT "id" FROM "issues" WHERE "id" = ANY($1) AND "deleted_at" IS NULL"#,
        PkTable::EstimatePoints => {
            r#"SELECT "id" FROM "estimate_points" WHERE "id" = ANY($1) AND "deleted_at" IS NULL"#
        }
        // `Pod.objects` is the soft-deletion `PodManager`.
        PkTable::Pods => r#"SELECT "id" FROM "pod" WHERE "id" = ANY($1) AND "deleted_at" IS NULL"#,
    };
    sqlx::query_scalar(sql)
        .bind(ids)
        .fetch_all(pool)
        .await
        .map_err(|error| db_error(error, "pk-exists"))
}

/// Parse one PK input (`PrimaryKeyRelatedField.to_internal_value`,
/// `relations.py:235-261`): bools fail the type arm, `uuid.UUID`
/// failures become the Django `"..." is not a valid UUID.` message
/// (caught per-field by the serializer), successes queue an existence
/// probe.
fn parse_pk_input(value: &Value) -> Result<PkProbe, String> {
    if value.is_boolean() {
        return Err("Incorrect type. Expected pk value, received bool.".to_owned());
    }
    match parse_uuid_input(value) {
        Ok(id) => Ok(PkProbe {
            id,
            raw: match value {
                Value::String(text) => text.clone(),
                _ => py_repr(value),
            },
        }),
        Err(()) => Err(invalid_uuid_message(value)),
    }
}

/// Parse one `assignees`/`labels` list (`ListField`, `fields.py:1772+`):
/// non-lists fail `not_a_list`, children parse as PKs, child failures
/// collect per index.
enum ListParse {
    Value(Vec<PkProbe>),
    Messages(Vec<String>),
    Indexed(Vec<(usize, Vec<String>)>),
}

fn parse_pk_list(value: &Value) -> ListParse {
    match value {
        Value::Array(items) => {
            let mut probes = Vec::with_capacity(items.len());
            let mut failures: Vec<(usize, Vec<String>)> = Vec::new();
            for (index, item) in items.iter().enumerate() {
                match parse_pk_input(item) {
                    Ok(probe) => probes.push(probe),
                    Err(message) => failures.push((index, vec![message])),
                }
            }
            if failures.is_empty() {
                ListParse::Value(probes)
            } else {
                ListParse::Indexed(failures)
            }
        }
        Value::Null => ListParse::Messages(vec!["This field may not be null.".to_owned()]),
        _ => ListParse::Messages(vec![format!(
            "Expected a list of items but got type \"{}\".",
            json_type_name(value)
        )]),
    }
}

/// The parsed shape of one `assignees`/`labels` list.
enum ListShape {
    Absent,
    Value(Vec<PkProbe>),
    Messages(Vec<String>),
    Indexed(Vec<(usize, Vec<String>)>),
}

/// Parse an optional `assignees`/`labels` list.
fn parse_optional_list(value: Option<&Value>) -> ListShape {
    match value {
        None => ListShape::Absent,
        Some(value) => match parse_pk_list(value) {
            ListParse::Value(probes) => ListShape::Value(probes),
            ListParse::Messages(messages) => ListShape::Messages(messages),
            ListParse::Indexed(failures) => ListShape::Indexed(failures),
        },
    }
}

/// The parsed shape of one field before existence probing: either a value
/// (possibly with queued probes) or immediate messages.
#[derive(Debug)]
enum FieldParse<T> {
    Absent,
    Value(T),
    Errors(Vec<String>),
}

/// Parse the normalized write body into [`ParsedWrite`]: shape first (in
/// `FIELDS_IN_ORDER`), then batched existence probes, then the ordered
/// error dict (or the value). `partial` is the PATCH flag (absent keys
/// skip; defaults never apply). `from_form` selects the HTML `get_value`
/// blank arms for the nullable fields (`''` → null).
async fn parse_issue_write(
    pool: &PgPool,
    body: &Map<String, Value>,
    partial: bool,
    from_form: bool,
    tz: &Tz,
    tz_name: &str,
) -> Result<ParsedWrite, Denial> {
    // Per-field shape results, in `FIELDS_IN_ORDER` (declared
    // `assignees`, `labels`, `type_id` first, then model order):
    // read-only (`id`, `url`, `created_at`, `updated_at`, `updated_by`,
    // `project`, `workspace`, `description_binary`) and unknown keys are
    // ignored, exactly like DRF's writable-fields iteration.
    let get = |key: &str| body.get(key);

    // --- assignees / labels (ListField, required=False) ---
    let assignees_shape = parse_optional_list(get("assignees"));
    let labels_shape = parse_optional_list(get("labels"));

    // --- type_id (declared PKRF, source=type, required=False, allow_null) ---
    let type_id_shape = parse_optional_pk(get("type_id"), from_form);

    // --- deleted_at (DateTime, allow_null) ---
    let deleted_at_shape = parse_optional_datetime(get("deleted_at"), from_form, tz, tz_name);

    // --- point (Integer, allow_null, 0..=12) ---
    let point_shape = parse_optional_int(get("point"), from_form, true, Some((0, 12)));

    // --- name (CharField max 255, REQUIRED) ---
    let name_shape: FieldParse<String> = match get("name") {
        None if partial => FieldParse::Absent,
        None => FieldParse::Errors(vec!["This field is required.".to_owned()]),
        Some(value) => {
            let errors = char_field_errors(value, false, false, Some(255));
            if errors.is_empty() {
                FieldParse::Value(char_field_value(value))
            } else {
                FieldParse::Errors(errors)
            }
        }
    };

    // --- description_html (TextField, allow_blank, default "<p></p>") ---
    let description_html_shape: FieldParse<String> = match get("description_html") {
        None => FieldParse::Absent,
        Some(value) => {
            let errors = char_field_errors(value, true, false, None);
            if errors.is_empty() {
                FieldParse::Value(char_field_value(value))
            } else {
                FieldParse::Errors(errors)
            }
        }
    };

    // --- priority (Choice, default "none") ---
    let priority_shape: FieldParse<String> = match get("priority") {
        None => FieldParse::Absent,
        Some(value) => {
            if value.is_null() {
                FieldParse::Errors(vec!["This field may not be null.".to_owned()])
            } else {
                match parse_drf_choice(value, &["urgent", "high", "medium", "low", "none"]) {
                    Ok(choice) => FieldParse::Value(choice.to_owned()),
                    Err(message) => FieldParse::Errors(vec![message]),
                }
            }
        }
    };

    // --- complexity_score (Integer, default 0, 0..=10) ---
    let complexity_shape: FieldParse<i64> = match parse_optional_int(
        get("complexity_score"),
        from_form,
        false,
        Some((0, 10)),
    ) {
        FieldParse::Absent => FieldParse::Absent,
        FieldParse::Value(Some(number)) => FieldParse::Value(number),
        // Non-nullable: null errors above, so `None` is unreachable.
        FieldParse::Value(None) => {
            FieldParse::Errors(vec!["This field may not be null.".to_owned()])
        }
        FieldParse::Errors(messages) => FieldParse::Errors(messages),
    };

    // --- start_date / target_date (Date, allow_null) ---
    let start_shape = parse_optional_date(get("start_date"), from_form);
    let target_shape = parse_optional_date(get("target_date"), from_form);

    // --- sequence_id (Integer, default 1) ---
    let sequence_shape = parse_optional_int_full(get("sequence_id"), from_form);

    // --- sort_order (Float, default 65535) ---
    let sort_shape: FieldParse<f64> = match get("sort_order") {
        None => FieldParse::Absent,
        Some(value) => {
            if value.is_null() {
                FieldParse::Errors(vec!["This field may not be null.".to_owned()])
            } else {
                match parse_drf_float(value) {
                    Ok(number) => FieldParse::Value(number),
                    Err(message) => FieldParse::Errors(vec![message.to_owned()]),
                }
            }
        }
    };

    // --- completed_at (DateTime, allow_null) ---
    let completed_shape = parse_optional_datetime(get("completed_at"), from_form, tz, tz_name);

    // --- archived_at (Date, allow_null) ---
    let archived_shape = parse_optional_date(get("archived_at"), from_form);

    // --- is_draft (Boolean, default false) ---
    let draft_shape: FieldParse<bool> = match get("is_draft") {
        None => FieldParse::Absent,
        Some(value) => {
            if value.is_null() {
                FieldParse::Errors(vec!["This field may not be null.".to_owned()])
            } else {
                match parse_drf_bool(value) {
                    Ok(flag) => FieldParse::Value(flag),
                    Err(message) => FieldParse::Errors(vec![message.to_owned()]),
                }
            }
        }
    };

    // --- external_source / external_id (Char 255, allow_null+allow_blank) ---
    let ext_source_shape = parse_optional_char(get("external_source"), Some(255));
    let ext_id_shape = parse_optional_char(get("external_id"), Some(255));

    // --- git_work_branch (Char 128, allow_blank, default "", +RegexValidator) ---
    let branch_shape: FieldParse<String> = match get("git_work_branch") {
        None => FieldParse::Absent,
        Some(value) => {
            let shape = char_shape_errors(value, true, false);
            if shape.is_empty() {
                let coerced = char_field_value(value);
                let errors = branch_errors(&coerced);
                if errors.is_empty() {
                    FieldParse::Value(coerced)
                } else {
                    FieldParse::Errors(errors)
                }
            } else {
                FieldParse::Errors(shape)
            }
        }
    };

    // --- created_via (Char 32, allow_null+allow_blank) ---
    let via_shape = parse_optional_char(get("created_via"), Some(32));

    // --- agent_executor (Choice, allow_null+allow_blank) ---
    let executor_shape: FieldParse<Option<String>> = match get("agent_executor") {
        None => FieldParse::Absent,
        Some(value) => {
            // HTML `''` with `allow_blank` stays `''` (never the null arm).
            if value.is_null() {
                FieldParse::Value(None)
            } else if value == &Value::String(String::new()) {
                FieldParse::Value(Some(String::new()))
            } else {
                match parse_drf_choice(
                    value,
                    &["local_runner", "cloud_agent", "managed_runner"],
                ) {
                    Ok(choice) => FieldParse::Value(Some(choice.to_owned())),
                    Err(message) => FieldParse::Errors(vec![message]),
                }
            }
        }
    };

    // --- created_by (PKRF User, allow_null) ---
    let created_by_shape = parse_optional_pk(get("created_by"), from_form);

    // --- parent (PKRF Issue, allow_null) ---
    let parent_shape = parse_optional_pk(get("parent"), from_form);

    // --- state (PKRF State, allow_null) ---
    let state_shape = parse_optional_pk(get("state"), from_form);

    // --- estimate_point (PKRF EstimatePoint, allow_null) ---
    let estimate_shape = parse_optional_pk(get("estimate_point"), from_form);

    // --- type (auto PKRF, source=type, allow_null; wins over type_id) ---
    let type_shape = parse_optional_pk(get("type"), from_form);

    // --- assigned_pod (PKRF Pod, allow_null) ---
    let pod_shape = parse_optional_pk(get("assigned_pod"), from_form);

    // Batch the existence probes (one query per table).
    let mut assignee_ids: Vec<Uuid> = Vec::new();
    let mut label_ids: Vec<Uuid> = Vec::new();
    let mut user_ids: Vec<Uuid> = Vec::new();
    let mut type_ids: Vec<Uuid> = Vec::new();
    let mut state_ids: Vec<Uuid> = Vec::new();
    let mut issue_ids: Vec<Uuid> = Vec::new();
    let mut estimate_ids: Vec<Uuid> = Vec::new();
    let mut pod_ids: Vec<Uuid> = Vec::new();
    if let ListShape::Value(probes) = &assignees_shape {
        assignee_ids.extend(probes.iter().map(|probe| probe.id));
    }
    if let ListShape::Value(probes) = &labels_shape {
        label_ids.extend(probes.iter().map(|probe| probe.id));
    }
    if let FieldParse::Value(Some(probe)) = &type_id_shape {
        type_ids.push(probe.id);
    }
    if let FieldParse::Value(Some(probe)) = &type_shape {
        type_ids.push(probe.id);
    }
    if let FieldParse::Value(Some(probe)) = &created_by_shape {
        user_ids.push(probe.id);
    }
    if let FieldParse::Value(Some(probe)) = &parent_shape {
        issue_ids.push(probe.id);
    }
    if let FieldParse::Value(Some(probe)) = &state_shape {
        state_ids.push(probe.id);
    }
    if let FieldParse::Value(Some(probe)) = &estimate_shape {
        estimate_ids.push(probe.id);
    }
    if let FieldParse::Value(Some(probe)) = &pod_shape {
        pod_ids.push(probe.id);
    }
    let assignee_ids = dedup(assignee_ids);
    let label_ids = dedup(label_ids);
    let user_ids = dedup(user_ids);
    let type_ids = dedup(type_ids);
    let state_ids = dedup(state_ids);
    let issue_ids = dedup(issue_ids);
    let estimate_ids = dedup(estimate_ids);
    let pod_ids = dedup(pod_ids);
    let (
        assignees_live,
        labels_live,
        users_live,
        types_live,
        states_live,
        issues_live,
        estimates_live,
        pods_live,
    ) = tokio::join!(
        probe_exists(pool, PkTable::Users, &assignee_ids),
        probe_exists(pool, PkTable::Labels, &label_ids),
        probe_exists(pool, PkTable::Users, &user_ids),
        probe_exists(pool, PkTable::IssueTypes, &type_ids),
        probe_exists(pool, PkTable::States, &state_ids),
        probe_exists(pool, PkTable::Issues, &issue_ids),
        probe_exists(pool, PkTable::EstimatePoints, &estimate_ids),
        probe_exists(pool, PkTable::Pods, &pod_ids),
    );
    let assignees_live = assignees_live?;
    let labels_live = labels_live?;
    let users_live = users_live?;
    let types_live = types_live?;
    let states_live = states_live?;
    let issues_live = issues_live?;
    let estimates_live = estimates_live?;
    let pods_live = pods_live?;
    let has = |live: &[Uuid], id: &Uuid| live.contains(id);

    // Assemble the ordered error dict (or the value). `type` wins over
    // `type_id` for the value; both report errors under their own names.
    let mut entries: Vec<(&str, Value)> = Vec::new();
    let mut parsed = ParsedWrite::default();

    // assignees
    match assignees_shape {
        ListShape::Absent => {}
        ListShape::Value(probes) => {
            let mut failures: Vec<(usize, Vec<String>)> = Vec::new();
            let mut ids = Vec::with_capacity(probes.len());
            for (index, probe) in probes.iter().enumerate() {
                if has(&assignees_live, &probe.id) {
                    ids.push(probe.id);
                } else {
                    failures.push((
                        index,
                        vec![format!(
                            "Invalid pk \"{}\" - object does not exist.",
                            probe.raw
                        )],
                    ));
                }
            }
            if failures.is_empty() {
                parsed.assignees = Some(ids);
            } else {
                entries.push(("assignees", index_errors_value(failures)));
            }
        }
        ListShape::Messages(messages) => {
            entries.push((
                "assignees",
                Value::Array(messages.into_iter().map(Value::String).collect()),
            ));
        }
        ListShape::Indexed(failures) => {
            entries.push(("assignees", index_errors_value(failures)));
        }
    }
    // labels
    match labels_shape {
        ListShape::Absent => {}
        ListShape::Value(probes) => {
            let mut failures: Vec<(usize, Vec<String>)> = Vec::new();
            let mut ids = Vec::with_capacity(probes.len());
            for (index, probe) in probes.iter().enumerate() {
                if has(&labels_live, &probe.id) {
                    ids.push(probe.id);
                } else {
                    failures.push((
                        index,
                        vec![format!(
                            "Invalid pk \"{}\" - object does not exist.",
                            probe.raw
                        )],
                    ));
                }
            }
            if failures.is_empty() {
                parsed.labels = Some(ids);
            } else {
                entries.push(("labels", index_errors_value(failures)));
            }
        }
        ListShape::Messages(messages) => {
            entries.push((
                "labels",
                Value::Array(messages.into_iter().map(Value::String).collect()),
            ));
        }
        ListShape::Indexed(failures) => {
            entries.push(("labels", index_errors_value(failures)));
        }
    }
    // type_id
    let mut type_value: Option<Option<Uuid>> = None;
    match type_id_shape {
        FieldParse::Absent => {}
        FieldParse::Value(None) => {
            type_value = Some(None);
        }
        FieldParse::Value(Some(probe)) => {
            if has(&types_live, &probe.id) {
                type_value = Some(Some(probe.id));
            } else {
                entries.push((
                    "type_id",
                    Value::Array(vec![Value::String(format!(
                        "Invalid pk \"{}\" - object does not exist.",
                        probe.raw
                    ))]),
                ));
            }
        }
        FieldParse::Errors(messages) => {
            entries.push((
                "type_id",
                Value::Array(messages.into_iter().map(Value::String).collect()),
            ));
        }
    }
    // type (wins over type_id for the value)
    match type_shape {
        FieldParse::Absent => {}
        FieldParse::Value(None) => {
            type_value = Some(None);
        }
        FieldParse::Value(Some(probe)) => {
            if has(&types_live, &probe.id) {
                type_value = Some(Some(probe.id));
            } else {
                entries.push((
                    "type",
                    Value::Array(vec![Value::String(format!(
                        "Invalid pk \"{}\" - object does not exist.",
                        probe.raw
                    ))]),
                ));
            }
        }
        FieldParse::Errors(messages) => {
            entries.push((
                "type",
                Value::Array(messages.into_iter().map(Value::String).collect()),
            ));
        }
    }
    // NOTE: `type` errors assemble here in field order, but the value
    // merge happens after every field below declares its shape — the
    // push order below follows `FIELDS_IN_ORDER` exactly. `type`'s entry
    // above is currently misplaced (it must come after `estimate_point`);
    // collect it aside and re-insert in order at the end.
    let mut type_entry: Option<(&str, Value)> = None;
    if entries.last().map(|(name, _)| *name) == Some("type") {
        type_entry = entries.pop();
    }
    parsed.issue_type = type_value;

    macro_rules! scalar_field {
        ($shape:expr, $name:literal, $slot:expr) => {
            match $shape {
                FieldParse::Absent => {}
                FieldParse::Value(value) => {
                    $slot = Some(value);
                }
                FieldParse::Errors(messages) => {
                    entries.push((
                        $name,
                        Value::Array(messages.into_iter().map(Value::String).collect()),
                    ));
                }
            }
        };
    }
    scalar_field!(deleted_at_shape, "deleted_at", parsed.deleted_at);
    scalar_field!(point_shape, "point", parsed.point);
    scalar_field!(name_shape, "name", parsed.name);
    scalar_field!(
        description_html_shape,
        "description_html",
        parsed.description_html
    );
    scalar_field!(priority_shape, "priority", parsed.priority);
    scalar_field!(
        complexity_shape,
        "complexity_score",
        parsed.complexity_score
    );
    scalar_field!(start_shape, "start_date", parsed.start_date);
    scalar_field!(target_shape, "target_date", parsed.target_date);
    match sequence_shape {
        FieldParse::Absent => {}
        FieldParse::Value((number, overflow)) => {
            parsed.sequence_id = Some(number);
            parsed.sequence_overflow = overflow;
        }
        FieldParse::Errors(messages) => {
            entries.push((
                "sequence_id",
                Value::Array(messages.into_iter().map(Value::String).collect()),
            ));
        }
    }
    scalar_field!(sort_shape, "sort_order", parsed.sort_order);
    scalar_field!(completed_shape, "completed_at", parsed.completed_at);
    scalar_field!(archived_shape, "archived_at", parsed.archived_at);
    scalar_field!(draft_shape, "is_draft", parsed.is_draft);
    scalar_field!(
        ext_source_shape,
        "external_source",
        parsed.external_source
    );
    scalar_field!(ext_id_shape, "external_id", parsed.external_id);
    scalar_field!(
        branch_shape,
        "git_work_branch",
        parsed.git_work_branch
    );
    scalar_field!(via_shape, "created_via", parsed.created_via);
    scalar_field!(
        executor_shape,
        "agent_executor",
        parsed.agent_executor
    );
    // created_by
    match created_by_shape {
        FieldParse::Absent => {}
        FieldParse::Value(None) => {
            parsed.created_by = Some(None);
        }
        FieldParse::Value(Some(probe)) => {
            if has(&users_live, &probe.id) {
                parsed.created_by = Some(Some(probe.id));
            } else {
                entries.push((
                    "created_by",
                    Value::Array(vec![Value::String(format!(
                        "Invalid pk \"{}\" - object does not exist.",
                        probe.raw
                    ))]),
                ));
            }
        }
        FieldParse::Errors(messages) => {
            entries.push((
                "created_by",
                Value::Array(messages.into_iter().map(Value::String).collect()),
            ));
        }
    }
    // parent
    match parent_shape {
        FieldParse::Absent => {}
        FieldParse::Value(None) => {
            parsed.parent = Some(None);
        }
        FieldParse::Value(Some(probe)) => {
            if has(&issues_live, &probe.id) {
                parsed.parent = Some(Some(probe.id));
            } else {
                entries.push((
                    "parent",
                    Value::Array(vec![Value::String(format!(
                        "Invalid pk \"{}\" - object does not exist.",
                        probe.raw
                    ))]),
                ));
            }
        }
        FieldParse::Errors(messages) => {
            entries.push((
                "parent",
                Value::Array(messages.into_iter().map(Value::String).collect()),
            ));
        }
    }
    // state
    match state_shape {
        FieldParse::Absent => {}
        FieldParse::Value(None) => {
            parsed.state = Some(None);
        }
        FieldParse::Value(Some(probe)) => {
            if has(&states_live, &probe.id) {
                parsed.state = Some(Some(probe.id));
            } else {
                entries.push((
                    "state",
                    Value::Array(vec![Value::String(format!(
                        "Invalid pk \"{}\" - object does not exist.",
                        probe.raw
                    ))]),
                ));
            }
        }
        FieldParse::Errors(messages) => {
            entries.push((
                "state",
                Value::Array(messages.into_iter().map(Value::String).collect()),
            ));
        }
    }
    // estimate_point
    match estimate_shape {
        FieldParse::Absent => {}
        FieldParse::Value(None) => {
            parsed.estimate_point = Some(None);
        }
        FieldParse::Value(Some(probe)) => {
            if has(&estimates_live, &probe.id) {
                parsed.estimate_point = Some(Some(probe.id));
            } else {
                entries.push((
                    "estimate_point",
                    Value::Array(vec![Value::String(format!(
                        "Invalid pk \"{}\" - object does not exist.",
                        probe.raw
                    ))]),
                ));
            }
        }
        FieldParse::Errors(messages) => {
            entries.push((
                "estimate_point",
                Value::Array(messages.into_iter().map(Value::String).collect()),
            ));
        }
    }
    // `type` re-inserted in field order (after `estimate_point`).
    if let Some(entry) = type_entry {
        entries.push(entry);
    }
    // assigned_pod
    match pod_shape {
        FieldParse::Absent => {}
        FieldParse::Value(None) => {
            parsed.assigned_pod = Some(None);
        }
        FieldParse::Value(Some(probe)) => {
            if has(&pods_live, &probe.id) {
                parsed.assigned_pod = Some(Some(probe.id));
            } else {
                entries.push((
                    "assigned_pod",
                    Value::Array(vec![Value::String(format!(
                        "Invalid pk \"{}\" - object does not exist.",
                        probe.raw
                    ))]),
                ));
            }
        }
        FieldParse::Errors(messages) => {
            entries.push((
                "assigned_pod",
                Value::Array(messages.into_iter().map(Value::String).collect()),
            ));
        }
    }

    if entries.is_empty() {
        Ok(parsed)
    } else {
        Err(Denial::FieldErrors(shape::field_errors_body(&entries)))
    }
}

/// Deduplicate batched probe ids (stable order).
fn dedup(mut ids: Vec<Uuid>) -> Vec<Uuid> {
    ids.sort();
    ids.dedup();
    ids
}

/// Build the `{index: [messages]}` detail for `ListField` child failures
/// (indices ascend; `preserve_order` keeps insertion order, and callers
/// pass ascending indices).
fn index_errors_value(failures: Vec<(usize, Vec<String>)>) -> Value {
    let mut detail = Map::new();
    for (index, messages) in failures {
        detail.insert(
            index.to_string(),
            Value::Array(messages.into_iter().map(Value::String).collect()),
        );
    }
    Value::Object(detail)
}

/// Parse an optional nullable PK: absent → `Absent`, null → `None`, form
/// `''` → `None` (`allow_null` arm of `get_value`), else the probe (or
/// its message).
fn parse_optional_pk(value: Option<&Value>, from_form: bool) -> FieldParse<Option<PkProbe>> {
    match value {
        None => FieldParse::Absent,
        Some(value) => {
            if value.is_null() {
                return FieldParse::Value(None);
            }
            if from_form && value == &Value::String(String::new()) {
                return FieldParse::Value(None);
            }
            match parse_pk_input(value) {
                Ok(probe) => FieldParse::Value(Some(probe)),
                Err(message) => FieldParse::Errors(vec![message]),
            }
        }
    }
}

/// Parse an optional integer with optional min/max validators
/// (`point`: nullable; `complexity_score`: non-nullable). Form `''` on
/// the nullable field coerces to the null arm; non-nullables never see
/// `''` (`skip_blank_fields` drops it first).
fn parse_optional_int(
    value: Option<&Value>,
    from_form: bool,
    allow_null: bool,
    min_max: Option<(i64, i64)>,
) -> FieldParse<Option<i64>> {
    match value {
        None => FieldParse::Absent,
        Some(value) => {
            if value.is_null() {
                if !allow_null {
                    return FieldParse::Errors(vec!["This field may not be null.".to_owned()]);
                }
                return FieldParse::Value(None);
            }
            if from_form && value == &Value::String(String::new()) {
                debug_assert!(allow_null);
                return FieldParse::Value(None);
            }
            match parse_drf_int(value) {
                Ok(number) => {
                    if let Some((min, max)) = min_max {
                        if number < min {
                            return FieldParse::Errors(vec![format!(
                                "Ensure this value is greater than or equal to {min}."
                            )]);
                        }
                        if number > max {
                            return FieldParse::Errors(vec![format!(
                                "Ensure this value is less than or equal to {max}."
                            )]);
                        }
                    }
                    FieldParse::Value(Some(number))
                }
                Err(message) => FieldParse::Errors(vec![message.to_owned()]),
            }
        }
    }
}

/// Parse the optional `sequence_id` (non-nullable, no min/max): null
/// errors; the overflow flag rides along for the save-time 500.
fn parse_optional_int_full(value: Option<&Value>, from_form: bool) -> FieldParse<(i64, bool)> {
    match value {
        None => FieldParse::Absent,
        Some(value) => {
            if value.is_null() {
                return FieldParse::Errors(vec!["This field may not be null.".to_owned()]);
            }
            debug_assert!(!(from_form && value == &Value::String(String::new())));
            match parse_drf_int_full(value) {
                Ok(pair) => FieldParse::Value(pair),
                Err(message) => FieldParse::Errors(vec![message.to_owned()]),
            }
        }
    }
}

/// Parse an optional nullable date: absent → `Absent`, null → `None`,
/// form `''` → `None`, else the date (or its message).
fn parse_optional_date(value: Option<&Value>, from_form: bool) -> FieldParse<Option<NaiveDate>> {
    match value {
        None => FieldParse::Absent,
        Some(value) => {
            if value.is_null() {
                return FieldParse::Value(None);
            }
            if from_form && value == &Value::String(String::new()) {
                return FieldParse::Value(None);
            }
            match parse_drf_date(value) {
                Ok(day) => FieldParse::Value(Some(day)),
                Err(message) => FieldParse::Errors(vec![message.to_owned()]),
            }
        }
    }
}

/// Parse an optional nullable datetime (naive → request zone).
fn parse_optional_datetime(
    value: Option<&Value>,
    from_form: bool,
    tz: &Tz,
    tz_name: &str,
) -> FieldParse<Option<DateTime<Utc>>> {
    match value {
        None => FieldParse::Absent,
        Some(value) => {
            if value.is_null() {
                return FieldParse::Value(None);
            }
            if from_form && value == &Value::String(String::new()) {
                return FieldParse::Value(None);
            }
            match parse_drf_datetime(value, tz, tz_name) {
                Ok(instant) => FieldParse::Value(Some(instant)),
                Err(message) => FieldParse::Errors(vec![message]),
            }
        }
    }
}

/// Parse an optional `allow_blank` + `allow_null` char (`external_source`,
/// `external_id`, `created_via`): absent → `Absent`, null → `None`,
/// else the coerced string (or its messages).
fn parse_optional_char(value: Option<&Value>, max_length: Option<usize>) -> FieldParse<Option<String>> {
    match value {
        None => FieldParse::Absent,
        Some(value) => {
            if value.is_null() {
                return FieldParse::Value(None);
            }
            let errors = char_field_errors(value, true, true, max_length);
            if errors.is_empty() {
                FieldParse::Value(Some(char_field_value(value)))
            } else {
                FieldParse::Errors(errors)
            }
        }
    }
}

/// The `git_work_branch` validators in run order: the model's
/// `RegexValidator` first, then `MaxLengthValidator`, then the
/// null-characters check (`fields.py:run_validators` collects all).
fn branch_errors(coerced: &str) -> Vec<String> {
    let mut errors = Vec::new();
    // `RegexValidator(regex=r"^[A-Za-z0-9._/-]*$")` — `[^...]` scan
    // avoids the `regex` crate for a single class.
    if !coerced
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'/' | b'-'))
    {
        errors.push("Branch name may contain only letters, numbers, and . _ / -".to_owned());
    }
    if coerced.chars().count() > 128 {
        errors.push("Ensure this field has no more than 128 characters.".to_owned());
    }
    if coerced.contains('\0') {
        errors.push("Null characters are not allowed.".to_owned());
    }
    errors
}

// ---------------------------------------------------------------------------
// Markdown → Tiptap HTML (`utils/markdown_converter.py:208-469`)
// ---------------------------------------------------------------------------

/// Tiptap's empty document (`markdown_converter.py:205`).
const EMPTY_DOCUMENT_HTML: &str = "<p></p>";

/// markdown-it's `escapeHtml`: exactly `&<>"` (`markdown-it-py`
/// `common/utils.py`). Notably NOT `'`.
fn escape_html(text: &str) -> String {
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

/// `_is_safe_url` (`markdown_converter.py:208-214`): relative URLs and the
/// sanitizer's schemes only (`SAFE_PROTOCOLS =
/// {"http","https","mailto","tel"}`, `content_validator.py:159`).
/// `urlsplit` strips ASCII tab/CR/LF everywhere, then leading C0 controls
/// and spaces; an invalid IPv6 literal raises → unsafe. The NFKC netloc
/// arm is not ported (it needs Unicode normalization tables; a residual
/// divergence on NFKC-unsafe non-ASCII hosts, where the sanitizer's own
/// independent scheme check still applies).
fn is_safe_url(url: &str) -> bool {
    // Strip ASCII \t\r\n everywhere, then leading C0 + space.
    let scrubbed: String = url.chars().filter(|ch| !matches!(ch, '\t' | '\r' | '\n')).collect();
    let scrubbed = scrubbed.trim_start_matches(|ch: char| ch.is_ascii_control() || ch == ' ');
    // Scheme: `ALPHA *( ALPHA / DIGIT / "+" / "-" / "." ) ":"`.
    let mut chars = scrubbed.chars();
    let Some(first) = chars.next() else {
        return true;
    };
    if !first.is_ascii_alphabetic() {
        return true;
    }
    let mut end = 1;
    for ch in chars {
        if ch.is_ascii_alphanumeric() || matches!(ch, '+' | '-' | '.') {
            end += ch.len_utf8();
        } else if ch == ':' {
            let scheme = scrubbed[..end].to_ascii_lowercase();
            if !matches!(scheme.as_str(), "http" | "https" | "mailto" | "tel") {
                return false;
            }
            // A recognized scheme: only an invalid IPv6 literal still
            // raises (`urlsplit` validates brackets eagerly).
            return ipv6_literally_valid(&scrubbed[end + 1..]);
        } else {
            return true;
        }
    }
    true
}

/// Whether every `[...]` host literal in the post-scheme rest is a valid
/// IPv6 literal (the `urlsplit` eager arm; ports raise → unsafe).
fn ipv6_literally_valid(rest: &str) -> bool {
    // The authority is the `//host` prefix when present.
    let authority = rest.strip_prefix("//").map(|after| {
        after
            .split(['/', '?', '#'])
            .next()
            .unwrap_or(after)
    });
    let Some(authority) = authority else {
        return true;
    };
    // Userinfo splits at the last `@`; zone ids (`%25eth0`) split at `%`.
    let host = authority.rsplit('@').next().unwrap_or(authority);
    if !host.starts_with('[') {
        return true;
    }
    let Some(close) = host.find(']') else {
        return false;
    };
    let inner = host[1..close].split('%').next().unwrap_or("");
    // After `]` only `:port` (digits, validated lazily — not here) or
    // nothing may follow; anything else raises too.
    let tail = &host[close + 1..];
    if !(tail.is_empty() || tail.starts_with(':')) {
        return false;
    }
    inner.parse::<std::net::Ipv6Addr>().is_ok()
}

/// One resolved markdown event: pulldown's stream after the task-list
/// pass (markers stripped or restored as literal text).
#[derive(Debug, Clone)]
enum MdEvent {
    StartHeading(u32),
    EndHeading,
    StartParagraph,
    EndParagraph,
    StartQuote,
    EndQuote,
    CodeBlock(Option<String>, String),
    StartList(Option<u64>, bool),
    EndList(bool),
    StartItem(bool, bool),
    EndItem(bool),
    StartTable,
    EndTable,
    StartHead,
    EndHead,
    StartRow,
    EndRow,
    StartCell(bool),
    EndCell(bool),
    StartEm,
    EndEm,
    StartStrong,
    EndStrong,
    StartStrike,
    EndStrike,
    StartLink(String, String),
    EndLink,
    Image(String, String, String),
    Text(String),
    Code(String),
    HtmlInline(String),
    HtmlBlock(String),
    SoftBreak,
    HardBreak,
    Rule,
}

/// Render agent-written markdown as Tiptap `description_html`
/// (`markdown_to_html`, `markdown_converter.py:444-469`): the CommonMark +
/// GFM-tables/strikethrough + tasklists parse, the `TiptapHTMLRenderer`
/// shapes, then `validate_html_content`. `Err` is the `ValueError`
/// message the serializer reports under `description_markdown`.
///
/// Engine note: the parser is pulldown-cmark (CommonMark + the same GFM
/// options) with the renderer rules ported token-for-token below; raw HTML
/// is disabled like `MarkdownIt("commonmark", {"html": False})`. The
/// `differential_markdown_corpus` test pins 228 oracle vectors from the
/// live Python converter.
fn markdown_to_html_port(markdown: &str) -> Result<String, String> {
    use pulldown_cmark::{Options, Parser};
    if markdown.trim().is_empty() {
        return Ok(EMPTY_DOCUMENT_HTML.to_owned());
    }
    let options = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let events: Vec<pulldown_cmark::Event> = Parser::new_ext(markdown, options).collect();
    let resolved = resolve_task_events(&events);
    let mut html = String::new();
    render_tiptap(&resolved, &mut html);
    if html.is_empty() {
        return Ok(EMPTY_DOCUMENT_HTML.to_owned());
    }
    if html.as_bytes().len() > 10 * 1024 * 1024 {
        return Err("HTML content exceeds maximum size limit (10MB)".to_owned());
    }
    match crate::space::sanitize::sanitize_html(&html) {
        crate::space::sanitize::Sanitize::Clean(clean) => {
            if clean.is_empty() {
                Ok(EMPTY_DOCUMENT_HTML.to_owned())
            } else {
                Ok(clean)
            }
        }
        crate::space::sanitize::Sanitize::Invalid => Err("Failed to sanitize HTML".to_owned()),
    }
}

/// Punycode-encode one DNS label (`codecs` punycode, as
/// `_punycode.to_ascii` calls it for labels matching `[^\0-\x7E]` —
/// note DEL and the C1 controls count as non-ASCII). `Ok(None)` when no
/// encoding is needed, `Err` on bootstring overflow (Python raises and
/// `normalizeLink` suppresses it, keeping the hostname). Basic code
/// points keep their case; constants are the RFC 3492 bootstring
/// parameters (`base=36, tmin=1, tmax=26, skew=38, damp=700`).
fn punycode_encode_label(label: &str) -> Result<Option<String>, ()> {
    if !label.chars().any(|c| c as u32 > 0x7E) {
        return Ok(None);
    }
    const BASE: u32 = 36;
    const TMIN: u32 = 1;
    const TMAX: u32 = 26;
    const SKEW: u32 = 38;
    const DAMP: u32 = 700;
    const INITIAL_BIAS: u32 = 72;
    const INITIAL_N: u32 = 128;
    fn adapt(mut delta: u32, num_points: u32, first: bool) -> u32 {
        delta = if first { delta / DAMP } else { delta / 2 };
        delta += delta / num_points;
        let mut k = 0u32;
        while delta > ((BASE - TMIN) * TMAX) / 2 {
            delta /= BASE - TMIN;
            k += BASE;
        }
        k + (BASE - TMIN + 1) * delta / (delta + SKEW)
    }
    fn digit_value(d: u32) -> char {
        (if d < 26 { b'a' + d as u8 } else { b'0' + (d - 26) as u8 }) as char
    }
    let points: Vec<char> = label.chars().collect();
    let mut out = String::new();
    for point in &points {
        if point.is_ascii() {
            out.push(*point);
        }
    }
    let basic_count = out.len() as u32;
    if basic_count > 0 {
        out.push('-');
    }
    let mut n = INITIAL_N;
    let mut delta = 0u32;
    let mut bias = INITIAL_BIAS;
    let mut handled = basic_count as usize;
    while handled < points.len() {
        let mut m = u32::MAX;
        for point in &points {
            let code = *point as u32;
            if code >= n && code < m {
                m = code;
            }
        }
        let Some(step) = m.checked_sub(n).and_then(|d| d.checked_mul((handled + 1) as u32)) else {
            return Err(());
        };
        delta = delta.checked_add(step).ok_or(())?;
        n = m;
        for point in &points {
            let code = *point as u32;
            if code < n {
                delta = delta.checked_add(1).ok_or(())?;
            } else if code == n {
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
                    out.push(digit_value(t + (q - t) % (BASE - t)));
                    q = (q - t) / (BASE - t);
                    k += BASE;
                }
                out.push(digit_value(q));
                bias = adapt(delta, (handled + 1) as u32, handled == basic_count as usize);
                delta = 0;
                handled += 1;
            }
        }
        delta = delta.checked_add(1).ok_or(())?;
        n = n.checked_add(1).ok_or(())?;
    }
    Ok(Some(format!("xn--{out}")))
}

/// Punycode-decode one `xn--` label body (`codecs` punycode decode, as
/// `_punycode.to_unicode` calls it). `None` on malformed input (Python
/// raises; the caller keeps the hostname).
fn punycode_decode_label(body: &str) -> Option<String> {
    const BASE: u32 = 36;
    const TMIN: u32 = 1;
    const TMAX: u32 = 26;
    const SKEW: u32 = 38;
    const DAMP: u32 = 700;
    const INITIAL_BIAS: u32 = 72;
    const INITIAL_N: u32 = 128;
    fn adapt(mut delta: u32, num_points: u32, first: bool) -> Option<u32> {
        delta = if first { delta.checked_div(DAMP)? } else { delta.checked_div(2)? };
        delta = delta.checked_add(delta.checked_div(num_points)?)?;
        let mut k = 0u32;
        while delta > ((BASE - TMIN) * TMAX) / 2 {
            delta = delta.checked_div(BASE - TMIN)?;
            k = k.checked_add(BASE)?;
        }
        k.checked_add((BASE - TMIN + 1).checked_mul(delta)?.checked_div(delta.checked_add(SKEW)?)?)
    }
    fn digit_value(byte: u8) -> Option<u32> {
        match byte {
            b'0'..=b'9' => Some(byte as u32 - b'0' as u32 + 26),
            b'A'..=b'Z' => Some(byte as u32 - b'A' as u32),
            b'a'..=b'z' => Some(byte as u32 - b'a' as u32),
            _ => None,
        }
    }
    if !body.is_ascii() {
        return None;
    }
    let bytes = body.as_bytes();
    let mut out: Vec<u32> = Vec::new();
    let mut extended_start = 0;
    if let Some(dash) = bytes.iter().rposition(|byte| *byte == b'-') {
        for byte in &bytes[..dash] {
            if *byte >= 0x80 {
                return None;
            }
            out.push(*byte as u32);
        }
        extended_start = dash + 1;
    }
    let mut n = INITIAL_N;
    let mut bias = INITIAL_BIAS;
    let mut index: usize = 0;
    let mut position = extended_start;
    let mut first = true;
    while position < bytes.len() {
        let old_index = index;
        let mut weight = 1u32;
        let mut k = BASE;
        loop {
            if position >= bytes.len() {
                return None;
            }
            let digit = digit_value(bytes[position])?;
            position += 1;
            index = index.checked_add(weight.checked_mul(digit)? as usize)?;
            let t = if k <= bias {
                TMIN
            } else if k >= bias + TMAX {
                TMAX
            } else {
                k - bias
            };
            if digit < t {
                break;
            }
            weight = weight.checked_mul(BASE - t)?;
            k = k.checked_add(BASE)?;
        }
        let out_len = out.len() as u32;
        bias = adapt(index as u32 - old_index as u32, out_len + 1, first)?;
        first = false;
        n = n.checked_add((index as u32).checked_div(out_len + 1)?)?;
        index = (index as u32 % (out_len + 1)) as usize;
        if n > 0x10FFFF || index > out.len() {
            return None;
        }
        out.insert(index, n);
        index += 1;
    }
    // The codec can emit surrogate code points; `mdurl.encode` pairs a
    // high surrogate followed by a low one and replaces lones with
    // U+FFFD. Fold pairs here so the tail sees real chars.
    let mut text = String::new();
    let mut position = 0;
    while position < out.len() {
        let code = out[position];
        if (0xD800..0xDC00).contains(&code)
            && position + 1 < out.len()
            && (0xDC00..0xE000).contains(&out[position + 1])
        {
            let high = code - 0xD800;
            let low = out[position + 1] - 0xDC00;
            text.push(char::from_u32(0x10000 + (high << 10) + low).unwrap_or('\u{FFFD}'));
            position += 2;
        } else {
            text.push(char::from_u32(code).unwrap_or('\u{FFFD}'));
            position += 1;
        }
    }
    Some(text)
}

/// `mdurl.encode` (`mdurl/_encode.py`, default `exclude`; surrogates
/// cannot occur in Rust `str`). Every ASCII char outside alnum +
/// `;/?:@&=+$,-_.!~*'()#` percent-encodes (uppercase hex); a `%`
/// followed by two hex digits (`i + 2 < l`, either case) passes the
/// triplet through, otherwise it encodes as `%25`; non-ASCII chars
/// encode their UTF-8 bytes.
fn mdurl_encode(raw: &str) -> String {
    fn keep(byte: u8) -> bool {
        matches!(byte, b'0'..=b'9' | b'A'..=b'Z' | b'a'..=b'z'
            | b';' | b'/' | b'?' | b':' | b'@' | b'&' | b'=' | b'+' | b'$'
            | b',' | b'-' | b'_' | b'.' | b'!' | b'~' | b'*' | b'\'' | b'(' | b')'
            | b'#')
    }
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let bytes = raw.as_bytes();
    let mut out = String::with_capacity(raw.len());
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte == b'%' && index + 2 < bytes.len() {
            let triplet = &bytes[index + 1..index + 3];
            if triplet[0].is_ascii_hexdigit() && triplet[1].is_ascii_hexdigit() {
                out.push_str(&raw[index..index + 3]);
                index += 3;
                continue;
            }
        }
        if byte < 0x80 {
            if keep(byte) {
                out.push(byte as char);
            } else {
                out.push('%');
                out.push(HEX[(byte >> 4) as usize] as char);
                out.push(HEX[(byte & 15) as usize] as char);
            }
            index += 1;
        } else {
            // UTF-8 multi-byte char: encode each byte (matches
            // `urllib.parse.quote` on the single char).
            let width = raw[index..].chars().next().map(|c| c.len_utf8()).unwrap_or(1);
            for offset in 0..width {
                let encoded = bytes[index + offset];
                out.push('%');
                out.push(HEX[(encoded >> 4) as usize] as char);
                out.push(HEX[(encoded & 15) as usize] as char);
            }
            index += width;
        }
    }
    out
}

/// `mdurl.decode` (`mdurl/_decode.py`): `re.sub` over maximal runs of
/// `%XX` triplets (either case); bare `%` passes through untouched.
/// Inside a run, sub-`0x80` bytes emit raw (re-encoded uppercase when in
/// `exclude`); multi-byte runs decode strict UTF-8 per sequence length
/// with `\u{FFFD}` (× sequence length) on failure; anything else emits
/// one `\u{FFFD}` per triplet.
fn mdurl_decode(raw: &str, exclude: &[u8]) -> String {
    fn hex_value(byte: u8) -> u8 {
        match byte {
            b'0'..=b'9' => byte - b'0',
            b'a'..=b'f' => byte - b'a' + 10,
            b'A'..=b'F' => byte - b'A' + 10,
            _ => 0,
        }
    }
    fn triplet_at(bytes: &[u8], index: usize) -> Option<u8> {
        if bytes.get(index) == Some(&b'%') {
            if let (Some(hi), Some(lo)) = (bytes.get(index + 1), bytes.get(index + 2)) {
                if hi.is_ascii_hexdigit() && lo.is_ascii_hexdigit() {
                    return Some(hex_value(*hi) << 4 | hex_value(*lo));
                }
            }
        }
        None
    }
    fn decode_run(seq: &[u8], exclude: &[u8]) -> String {
        const HEX: &[u8; 16] = b"0123456789ABCDEF";
        let mut out = String::new();
        let mut triplet = 0;
        while triplet < seq.len() {
            let b1 = seq[triplet];
            if b1 < 0x80 {
                if exclude.contains(&b1) {
                    out.push('%');
                    out.push(HEX[(b1 >> 4) as usize] as char);
                    out.push(HEX[(b1 & 15) as usize] as char);
                } else {
                    out.push(b1 as char);
                }
                triplet += 1;
                continue;
            }
            let remaining = seq.len() - triplet;
            let continuations_ok = |count: usize| -> bool {
                (1..=count)
                    .all(|offset| seq.get(triplet + offset).is_some_and(|byte| byte & 0xC0 == 0x80))
            };
            let mut consumed = 0;
            for (mask, prefix, count, need) in
                [(0xE0u8, 0xC0u8, 1usize, 2usize), (0xF0, 0xE0, 2, 3), (0xF8, 0xF0, 3, 4)]
            {
                if b1 & mask == prefix && remaining >= need && continuations_ok(count) {
                    let bytes: Vec<u8> =
                        (0..=count).map(|offset| seq[triplet + offset]).collect();
                    match std::str::from_utf8(&bytes) {
                        Ok(decoded) => out.push_str(decoded),
                        Err(_) => {
                            for _ in 0..=count {
                                out.push('\u{FFFD}');
                            }
                        }
                    }
                    consumed = need;
                    break;
                }
            }
            if consumed == 0 {
                out.push('\u{FFFD}');
                consumed = 1;
            }
            triplet += consumed;
        }
        out
    }
    let bytes = raw.as_bytes();
    let mut out = String::with_capacity(raw.len());
    let mut index = 0;
    while index < bytes.len() {
        if triplet_at(bytes, index).is_some() {
            let start = index;
            while triplet_at(bytes, index).is_some() {
                index += 3;
            }
            let values: Vec<u8> = bytes[start..index]
                .chunks_exact(3)
                .map(|triplet| hex_value(triplet[1]) << 4 | hex_value(triplet[2]))
                .collect();
            out.push_str(&decode_run(&values, exclude));
        } else {
            let point = raw[index..].chars().next().unwrap_or('\u{FFFD}');
            out.push(point);
            index += point.len_utf8();
        }
    }
    out
}

/// `mdurl.parse` result (`mdurl/_url.py`): the link-destination split
/// `normalizeLink`/`normalizeLinkText` round-trip through.
#[derive(Debug, Default, Clone)]
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

/// `mdurl.parse(url, slashes_denote_host=True)` (`mdurl/_parse.py`): the
/// Node-legacy-URL port with the six documented changes (no leading-slash
/// fixup, no backslash folding, trailing colon stays in path, no
/// pre-encoding, no query parsing, trimmed result). Operates on chars:
/// every index below is a char index like Python's. The
/// `not slashes_denote_host` fast path is unreachable here (both callers
/// pass `True`).
fn mdurl_parse(url: &str) -> MdUrl {
    fn is_hostless(proto: &str) -> bool {
        proto == "javascript" || proto == "javascript:"
    }
    fn is_slashed(proto: &str) -> bool {
        matches!(proto, "http" | "https" | "ftp" | "gopher" | "file" | "http:" | "https:" | "ftp:" | "gopher:" | "file:")
    }
    fn hostname_char_ok(point: char) -> bool {
        point.is_ascii_alphanumeric() || matches!(point, '+' | '-' | '_' )
    }
    let mut parsed = MdUrl::default();
    let mut rest = py_strip(url).to_owned();
    let mut proto = String::new();
    // `^([a-z0-9.+-]+:)` IGNORECASE; the protocol keeps its case.
    let prefix_len: usize = rest
        .chars()
        .take_while(|point| point.is_ascii_alphanumeric() || matches!(point, '.' | '+' | '-'))
        .map(|point| point.len_utf8())
        .sum();
    if prefix_len > 0 && rest[prefix_len..].starts_with(':') {
        proto = rest[..prefix_len + 1].to_owned();
        parsed.protocol = Some(proto.clone());
        rest = rest[prefix_len + 1..].to_owned();
    }
    // Host detection: `slashes_denote_host` is always true here, so the
    // `//user@host` regex arm never matters.
    let mut slashes = false;
    if rest.starts_with("//") && (proto.is_empty() || !is_hostless(&proto)) {
        rest = rest[2..].to_owned();
        parsed.slashes = true;
        slashes = true;
    }
    if !is_hostless(&proto) && (slashes || (!proto.is_empty() && !is_slashed(&proto))) {
        let chars: Vec<char> = rest.chars().collect();
        let find_first = |points: &[char], targets: &[char]| -> Option<usize> {
            points.iter().position(|point| targets.contains(point))
        };
        let host_end = find_first(&chars, &['/', '?', '#']);
        let at_sign = match host_end {
            None => chars.iter().rposition(|point| *point == '@'),
            Some(end) => chars[..end + 1].iter().rposition(|point| *point == '@'),
        };
        if let Some(at) = at_sign {
            let byte: usize = chars[..at].iter().map(|point| point.len_utf8()).sum();
            let at_bytes = byte + chars[at].len_utf8();
            parsed.auth = Some(rest[..byte].to_owned());
            rest = rest[at_bytes..].to_owned();
        }
        let chars: Vec<char> = rest.chars().collect();
        const NON_HOST: &[char] = &[
            '%', '/', '?', ';', '#', '\'', '{', '}', '|', '\\', '^', '`', '<', '>', '"', '`', ' ',
            '\r', '\n', '\t',
        ];
        let mut host_end = find_first(&chars, NON_HOST).unwrap_or(chars.len());
        if host_end > 0 && chars[host_end - 1] == ':' {
            host_end -= 1;
        }
        let host_bytes: usize = chars[..host_end].iter().map(|point| point.len_utf8()).sum();
        let host = rest[..host_bytes].to_owned();
        rest = rest[host_bytes..].to_owned();
        // `parse_host`: `:[0-9]*$` — the last colon with only digits after.
        let mut hostname = host.clone();
        if let Some(colon) = host.rfind(':') {
            if host[colon + 1..].chars().all(|point| point.is_ascii_digit()) {
                if colon + 1 < host.len() {
                    parsed.port = Some(host[colon + 1..].to_owned());
                }
                hostname = host[..colon].to_owned();
            }
        }
        if !hostname.is_empty() {
            parsed.hostname = Some(hostname.clone());
        }
        if parsed.hostname.is_none() {
            parsed.hostname = Some(String::new());
        }
        let hostname = parsed.hostname.clone().unwrap_or_default();
        let ipv6 = hostname.starts_with('[') && hostname.ends_with(']');
        if !ipv6 {
            let parts: Vec<&str> = hostname.split('.').collect();
            let mut index = 0;
            while index < parts.len() {
                let part = parts[index];
                if part.is_empty() {
                    index += 1;
                    continue;
                }
                let valid =
                    part.chars().count() <= 63 && part.chars().all(hostname_char_ok);
                if !valid {
                    let placeholder: String = part
                        .chars()
                        .map(|point| if (point as u32) > 127 { 'x' } else { point })
                        .collect();
                    let placeholder_valid = placeholder.chars().count() <= 63
                        && placeholder.chars().all(hostname_char_ok);
                    if !placeholder_valid {
                        let chars: Vec<char> = part.chars().collect();
                        let take = chars
                            .iter()
                            .take(63)
                            .take_while(|point| hostname_char_ok(**point))
                            .count();
                        let valid_prefix: String = chars[..take].iter().collect();
                        let remainder: String = chars[take..].iter().collect();
                        let mut valid_parts: Vec<String> =
                            parts[..index].iter().map(|part| part.to_string()).collect();
                        valid_parts.push(valid_prefix);
                        let mut not_host: Vec<String> =
                            parts[index + 1..].iter().map(|part| part.to_string()).collect();
                        not_host.insert(0, remainder);
                        // `if not_host:` is always true (the list never
                        // empties); the join can still be empty.
                        rest = format!("{}{}", not_host.join("."), rest);
                        parsed.hostname = Some(valid_parts.join("."));
                        break;
                    }
                }
                index += 1;
            }
        }
        let hostname = parsed.hostname.clone().unwrap_or_default();
        if hostname.chars().count() > 255 {
            parsed.hostname = Some(String::new());
        } else if ipv6 {
            let stripped = hostname[1..hostname.len() - 1].to_owned();
            parsed.hostname = Some(stripped);
        }
    }
    if let Some(hash_at) = rest.find('#') {
        parsed.hash = Some(rest[hash_at..].to_owned());
        rest = rest[..hash_at].to_owned();
    }
    if let Some(query_at) = rest.find('?') {
        parsed.search = Some(rest[query_at..].to_owned());
        rest = rest[..query_at].to_owned();
    }
    if !rest.is_empty() {
        parsed.pathname = Some(rest);
    }
    let lower_proto = proto.to_lowercase();
    if is_slashed(&lower_proto) && parsed.hostname.as_deref().is_some_and(|host| !host.is_empty()) && parsed.pathname.is_none() {
        parsed.pathname = Some(String::new());
    }
    parsed
}

/// `mdurl.format` (`mdurl/_format.py`): reassemble. A hostname containing
/// `:` re-brackets as IPv6; an empty auth drops its `@`.
fn mdurl_format(parsed: &MdUrl) -> String {
    let mut result = String::new();
    if let Some(protocol) = &parsed.protocol {
        result.push_str(protocol);
    }
    if parsed.slashes {
        result.push_str("//");
    }
    if let Some(auth) = &parsed.auth {
        if !auth.is_empty() {
            result.push_str(auth);
            result.push('@');
        }
    }
    match &parsed.hostname {
        Some(hostname) if hostname.contains(':') => {
            result.push('[');
            result.push_str(hostname);
            result.push(']');
        }
        Some(hostname) => result.push_str(hostname),
        None => {}
    }
    if let Some(port) = &parsed.port {
        result.push(':');
        result.push_str(port);
    }
    if let Some(pathname) = &parsed.pathname {
        result.push_str(pathname);
    }
    if let Some(search) = &parsed.search {
        result.push_str(search);
    }
    if let Some(hash) = &parsed.hash {
        result.push_str(hash);
    }
    result
}

/// `_punycode.map_domain` + `to_ascii` (`markdown_it/_punycode.py`): split
/// a single `@` (extra `@` tails drop), split labels on `.`/`\u3002`/
/// `\uFF0E`/`\uFF61` (rejoined with `.`), `xn--`-encode labels with any
/// char above U+007E. `None` on bootstring overflow (Python raises and
/// the caller suppresses it).
fn punycode_to_ascii(host: &str) -> Option<String> {
    let (prefix, domain) = match host.split_once('@') {
        Some((local, rest)) => {
            let domain = rest.split('@').next().unwrap_or("");
            (format!("{local}@"), domain)
        }
        None => (String::new(), host),
    };
    let mut labels = Vec::new();
    for label in domain.split(['.', '\u{3002}', '\u{FF0E}', '\u{FF61}']) {
        match punycode_encode_label(label).ok()? {
            Some(encoded) => labels.push(encoded),
            None => labels.push(label.to_owned()),
        }
    }
    Some(format!("{prefix}{}", labels.join(".")))
}

/// `_punycode.to_unicode`: decode `xn--` labels (lowercased first) back to
/// Unicode for autolink text. `None` when any label fails to decode.
fn punycode_to_unicode(host: &str) -> Option<String> {
    let (prefix, domain) = match host.split_once('@') {
        Some((local, rest)) => {
            let domain = rest.split('@').next().unwrap_or("");
            (format!("{local}@"), domain)
        }
        None => (String::new(), host),
    };
    let mut labels = Vec::new();
    for label in domain.split(['.', '\u{3002}', '\u{FF0E}', '\u{FF61}']) {
        if label.starts_with("xn--") {
            labels.push(punycode_decode_label(&label[4..].to_lowercase())?);
        } else {
            labels.push(label.to_owned());
        }
    }
    Some(format!("{prefix}{}", labels.join(".")))
}

/// `normalizeLink` (`markdown_it/common/normalize_url.py`): parse, IDNA
/// the hostname for empty/`http:`/`https:`/`mailto:` protocols, format,
/// `mdurl.encode`.
fn normalize_link(url: &str) -> String {
    let mut parsed = mdurl_parse(url);
    let recode = parsed.hostname.as_deref().is_some_and(|host| !host.is_empty())
        && parsed.protocol.as_deref().is_none_or(|protocol| {
            protocol == "http:" || protocol == "https:" || protocol == "mailto:"
        });
    if recode {
        if let Some(host) = parsed.hostname.clone() {
            if let Some(ascii) = punycode_to_ascii(&host) {
                parsed.hostname = Some(ascii);
            }
        }
    }
    mdurl_encode(&mdurl_format(&parsed))
}

/// `normalizeLinkText`: parse, Unicode the hostname, format,
/// `mdurl.decode` with `%` added to the excludes
/// (markdown-it/markdown-it#720).
fn normalize_link_text(url: &str) -> String {
    let mut parsed = mdurl_parse(url);
    let recode = parsed.hostname.as_deref().is_some_and(|host| !host.is_empty())
        && parsed.protocol.as_deref().is_none_or(|protocol| {
            protocol == "http:" || protocol == "https:" || protocol == "mailto:"
        });
    if recode {
        if let Some(host) = parsed.hostname.clone() {
            if let Some(unicode) = punycode_to_unicode(&host) {
                parsed.hostname = Some(unicode);
            }
        }
    }
    const EXCLUDE: &[u8] = b";/?:@&=+$,#%";
    mdurl_decode(&mdurl_format(&parsed), EXCLUDE)
}

/// `validateLink` on the normalized URL: strip + lowercase, then a bad
/// protocol (`vbscript:`/`javascript:`/`file:`/`data:`) passes only for
/// `data:image/{gif,png,jpeg,webp};` — note: no `svg+xml`, no
/// `video/`/`audio/`, and the `;` need not introduce `base64`.
fn validate_link(url: &str) -> bool {
    let lower = url.trim().to_lowercase();
    let bad = lower.starts_with("vbscript:")
        || lower.starts_with("javascript:")
        || lower.starts_with("file:")
        || lower.starts_with("data:");
    if !bad {
        return true;
    }
    lower.starts_with("data:image/gif;")
        || lower.starts_with("data:image/png;")
        || lower.starts_with("data:image/jpeg;")
        || lower.starts_with("data:image/webp;")
}

/// Lower pulldown events to [`MdEvent`], deciding task lists
/// (`_resolve_task_lists`, `markdown_converter.py:232-272`): a bullet list
/// becomes a `taskList` only when EVERY item carries a marker; elsewhere
/// the marker is restored as literal `[x]`/`[ ]` text. In task items the
/// marker is dropped and the following text's leading space lstrips (the
/// plugin strips `[ ]` but leaves the separator).
fn resolve_task_events(events: &[pulldown_cmark::Event]) -> Vec<MdEvent> {
    use pulldown_cmark::{Event, Tag, TagEnd};
    // First pass: per-list task decisions. A list (bullet only) is a task
    // list when it has at least one item and every item's first inline
    // payload is a `TaskListMarker`.
    #[derive(Debug)]
    struct ListFrame {
        ordered_start: Option<u64>,
        item_has_marker: Vec<bool>,
        item_indices: Vec<usize>,
    }
    let mut list_stack: Vec<ListFrame> = Vec::new();
    // `task_list[i]` / `task_item[i]` / `checked[i]` by event index.
    let mut list_decision: Vec<Option<bool>> = vec![None; events.len()];
    let mut item_task: Vec<Option<bool>> = vec![None; events.len()];
    for (index, event) in events.iter().enumerate() {
        match event {
            Event::Start(Tag::List(start)) => list_stack.push(ListFrame {
                ordered_start: *start,
                item_has_marker: Vec::new(),
                item_indices: Vec::new(),
            }),
            Event::End(TagEnd::List(_)) => {
                if let Some(frame) = list_stack.pop() {
                    let as_task = frame.ordered_start.is_none()
                        && !frame.item_indices.is_empty()
                        && frame.item_has_marker.iter().all(|has| *has);
                    // Find the matching start event to tag it (search back
                    // for the `Start(List)` at this depth).
                    let mut depth = 0;
                    for back in (0..index).rev() {
                        match &events[back] {
                            Event::End(TagEnd::List(_)) => depth += 1,
                            Event::Start(Tag::List(_)) => {
                                if depth == 0 {
                                    list_decision[back] = Some(as_task);
                                    break;
                                }
                                depth -= 1;
                            }
                            _ => {}
                        }
                    }
                    list_decision[index] = Some(as_task);
                    for (item_idx, has) in frame.item_indices.iter().zip(frame.item_has_marker.iter()) {
                        if *has {
                            item_task[*item_idx] = Some(as_task);
                        }
                    }
                }
            }
            Event::Start(Tag::Item) => {
                if let Some(frame) = list_stack.last_mut() {
                    frame.item_indices.push(index);
                    // The marker, when present, is the item's first event.
                    let has = matches!(events.get(index + 1), Some(Event::TaskListMarker(_)));
                    frame.item_has_marker.push(has);
                }
            }
            _ => {}
        }
    }
    // Second pass: lower, consuming markers.
    let mut out: Vec<MdEvent> = Vec::with_capacity(events.len());
    let mut index = 0;
    // Item (task state, checked flag, open-event index) by nesting stack.
    let mut item_stack: Vec<(bool, bool, usize)> = Vec::new();
    // Whether the next text's leading space lstrips (post-marker).
    let mut lstrip_next_text = false;
    // Image alt-text capture: (dest, title, alt parts).
    let mut image_stack: Vec<(String, String, String)> = Vec::new();
    // Table-cell head flags by nesting stack.
    let mut cell_stack: Vec<bool> = Vec::new();
    while index < events.len() {
        // Task-list markers are consumed here, never lowered directly.
        if let Event::TaskListMarker(checked) = events[index] {
            // Find the enclosing item's task state.
            let is_task = item_stack.last().map(|(task, _, _)| *task).unwrap_or(false);
            if !is_task {
                out.push(MdEvent::Text(if checked { "[x]".to_owned() } else { "[ ]".to_owned() }));
            } else if let Some(top) = item_stack.last_mut() {
                top.1 = checked;
            }
            lstrip_next_text = is_task;
            index += 1;
            continue;
        }
        match &events[index] {
            Event::Start(Tag::Heading { level, .. }) => out.push(MdEvent::StartHeading(*level as u32)),
            Event::End(TagEnd::Heading(_)) => out.push(MdEvent::EndHeading),
            Event::Start(Tag::Paragraph) => out.push(MdEvent::StartParagraph),
            Event::End(TagEnd::Paragraph) => out.push(MdEvent::EndParagraph),
            Event::Start(Tag::BlockQuote(_)) => out.push(MdEvent::StartQuote),
            Event::End(TagEnd::BlockQuote(_)) => out.push(MdEvent::EndQuote),
            Event::Start(Tag::CodeBlock(kind)) => {
                use pulldown_cmark::CodeBlockKind;
                let language = match kind {
                    CodeBlockKind::Fenced(info) => {
                        info.trim().split_whitespace().next().unwrap_or("").to_owned()
                    }
                    CodeBlockKind::Indented => String::new(),
                };
                // Join the code text (pulldown splits it across events).
                let mut body = String::new();
                let mut cursor = index + 1;
                while cursor < events.len() {
                    match &events[cursor] {
                        Event::Text(text) | Event::Code(text) => body.push_str(text),
                        Event::End(TagEnd::CodeBlock) => break,
                        _ => {}
                    }
                    cursor += 1;
                }
                // markdown-it keeps the newline before the closing fence;
                // the editor does not store one (`fence`, `:368-376`).
                if let Some(stripped) = body.strip_suffix('\n') {
                    body = stripped.to_owned();
                }
                let fenced = matches!(kind, CodeBlockKind::Fenced(_));
                out.push(MdEvent::CodeBlock(
                    if fenced && !language.is_empty() {
                        Some(language)
                    } else if fenced {
                        Some(String::new())
                    } else {
                        None
                    },
                    body,
                ));
                index = cursor;
            }
            Event::End(TagEnd::CodeBlock) => {}
            Event::Start(Tag::List(start)) => {
                let as_task = list_decision[index].unwrap_or(false);
                out.push(MdEvent::StartList(*start, as_task));
            }
            Event::End(TagEnd::List(_)) => {
                let as_task = list_decision[index].unwrap_or(false);
                out.push(MdEvent::EndList(as_task));
            }
            Event::Start(Tag::Item) => {
                let as_task = item_task[index].unwrap_or(false);
                let open = out.len();
                item_stack.push((as_task, false, open));
                out.push(MdEvent::StartItem(as_task, false));
            }
            Event::End(TagEnd::Item) => {
                let (as_task, checked, open) = item_stack.pop().unwrap_or((false, false, usize::MAX));
                // Patch the checked flag onto this item's open event.
                if let Some(MdEvent::StartItem(_, slot)) = out.get_mut(open) {
                    *slot = checked;
                }
                out.push(MdEvent::EndItem(as_task));
            }
            Event::Start(Tag::Table(_)) => out.push(MdEvent::StartTable),
            Event::End(TagEnd::Table) => out.push(MdEvent::EndTable),
            Event::Start(Tag::TableHead) => out.push(MdEvent::StartHead),
            Event::End(TagEnd::TableHead) => out.push(MdEvent::EndHead),
            Event::Start(Tag::TableRow) => out.push(MdEvent::StartRow),
            Event::End(TagEnd::TableRow) => out.push(MdEvent::EndRow),
            Event::Start(Tag::TableCell) => {
                // Header vs body: inside `TableHead` until its end.
                let mut in_head = false;
                let mut depth = 0;
                for back in (0..index).rev() {
                    match &events[back] {
                        Event::Start(Tag::TableHead) if depth == 0 => {
                            in_head = true;
                            break;
                        }
                        Event::End(TagEnd::TableHead) => depth += 1,
                        Event::Start(Tag::TableHead) => {
                            depth -= 1;
                        }
                        Event::Start(Tag::Table(_)) => break,
                        _ => {}
                    }
                }
                cell_stack.push(in_head);
                out.push(MdEvent::StartCell(in_head));
            }
            Event::End(TagEnd::TableCell) => {
                // The renderer pairs opens and closes structurally; carry
                // the flag from the matching open via the cell stack.
                let head = cell_stack.pop().unwrap_or(false);
                out.push(MdEvent::EndCell(head));
            }
            Event::Start(Tag::Emphasis) => out.push(MdEvent::StartEm),
            Event::End(TagEnd::Emphasis) => out.push(MdEvent::EndEm),
            Event::Start(Tag::Strong) => out.push(MdEvent::StartStrong),
            Event::End(TagEnd::Strong) => out.push(MdEvent::EndStrong),
            Event::Start(Tag::Strikethrough) => out.push(MdEvent::StartStrike),
            Event::End(TagEnd::Strikethrough) => out.push(MdEvent::EndStrike),
            Event::Start(Tag::Link { dest_url, title, .. }) => {
                out.push(MdEvent::StartLink(dest_url.to_string(), title.to_string()));
            }
            Event::End(TagEnd::Link) => out.push(MdEvent::EndLink),
            Event::Start(Tag::Image { dest_url, title, .. }) => {
                image_stack.push((dest_url.to_string(), title.to_string(), String::new()));
            }
            Event::End(TagEnd::Image) => {
                let (dest, title, alt) = image_stack.pop().unwrap_or_default();
                if image_stack.is_empty() {
                    out.push(MdEvent::Image(dest, title, alt));
                } else if let Some(parent) = image_stack.last_mut() {
                    parent.2.push_str(&alt);
                }
            }
            Event::Text(text) => {
                let mut content = text.to_string();
                if lstrip_next_text {
                    let stripped = content.trim_start_matches([' ', '\t']).to_owned();
                    content = stripped;
                    lstrip_next_text = false;
                }
                if let Some(image) = image_stack.last_mut() {
                    image.2.push_str(&content);
                } else {
                    out.push(MdEvent::Text(content));
                }
            }
            Event::Code(content) => {
                if let Some(image) = image_stack.last_mut() {
                    image.2.push_str(content);
                } else {
                    out.push(MdEvent::Code(content.to_string()));
                }
            }
            Event::Html(content) => out.push(MdEvent::HtmlBlock(content.to_string())),
            Event::InlineHtml(content) => {
                if image_stack.is_empty() {
                    out.push(MdEvent::HtmlInline(content.to_string()));
                }
            }
            Event::SoftBreak => {
                if let Some(image) = image_stack.last_mut() {
                    image.2.push(' ');
                } else {
                    out.push(MdEvent::SoftBreak);
                }
            }
            Event::HardBreak => out.push(MdEvent::HardBreak),
            Event::Rule => out.push(MdEvent::Rule),
            Event::FootnoteReference(_) => {}
            Event::TaskListMarker(_) => {}
            _ => {}
        }
        index += 1;
    }
    out
}

/// Render resolved events as the HTML Tiptap's editor emits
/// (`TiptapHTMLRenderer`, `markdown_converter.py:275-431`): compact (no
/// inter-block newlines), only the attributes the editor's parse rules
/// read. Tight-list paragraphs render too (the `hidden = False` pass,
/// `:456-460` — pulldown emits them regardless).
fn render_tiptap(events: &[MdEvent], html: &mut String) {
    let mut index = 0;
    while index < events.len() {
        match &events[index] {
            MdEvent::StartHeading(level) => html.push_str(&format!("<h{level}>")),
            MdEvent::EndHeading => {
                // The level is unknown at the close; re-scan for the open.
                let mut level = 1;
                for back in events[..index].iter().rev() {
                    match back {
                        MdEvent::StartHeading(open) => {
                            level = *open;
                            break;
                        }
                        MdEvent::EndHeading => break,
                        _ => {}
                    }
                }
                html.push_str(&format!("</h{level}>"));
            }
            MdEvent::StartParagraph => {
                // A lone safe image hoists out of the paragraph (Tiptap
                // images are blocks): `_lone_image`, `:313-322`.
                if !is_lone_image(events, index) {
                    html.push_str("<p>");
                }
            }
            MdEvent::EndParagraph => {
                if !is_lone_image_close(events, index) {
                    html.push_str("</p>");
                }
            }
            MdEvent::StartQuote => html.push_str("<blockquote>"),
            MdEvent::EndQuote => html.push_str("</blockquote>"),
            MdEvent::CodeBlock(language, body) => {
                let body = escape_html(body);
                match language {
                    Some(language) if !language.is_empty() => html.push_str(&format!(
                        "<pre><code class=\"language-{}\">{body}</code></pre>",
                        escape_html(language)
                    )),
                    _ => html.push_str(&format!("<pre><code>{body}</code></pre>")),
                }
            }
            MdEvent::StartList(start, as_task) => {
                // `bullet_list_open` / `ordered_list_open`, `:341-350`:
                // task lists are `<ul data-type="taskList">`; ordered
                // lists carry `start` unless it is 1.
                match start {
                    None if *as_task => html.push_str("<ul data-type=\"taskList\">"),
                    None => html.push_str("<ul>"),
                    Some(1) => html.push_str("<ol>"),
                    Some(number) => html.push_str(&format!("<ol start=\"{number}\">")),
                }
            }
            MdEvent::EndList(as_task) => {
                // Ordered lists close `</ol>`; bullet/task lists `</ul>`.
                // Re-scan for the open kind.
                let mut ordered = false;
                let mut depth = 0;
                for back in events[..index].iter().rev() {
                    match back {
                        MdEvent::EndList(_) => depth += 1,
                        MdEvent::StartList(start, _) => {
                            if depth == 0 {
                                ordered = start.is_some();
                                break;
                            }
                            depth -= 1;
                        }
                        _ => {}
                    }
                }
                let _ = as_task;
                html.push_str(if ordered { "</ol>" } else { "</ul>" });
            }
            MdEvent::StartItem(as_task, checked) => {
                if !as_task {
                    html.push_str("<li>");
                } else {
                    let checkbox = if *checked {
                        "<input type=\"checkbox\" checked=\"checked\">"
                    } else {
                        "<input type=\"checkbox\">"
                    };
                    html.push_str(&format!(
                        "<li data-type=\"taskItem\" data-checked=\"{}\"><label>{checkbox}<span></span></label><div>",
                        if *checked { "true" } else { "false" }
                    ));
                }
            }
            MdEvent::EndItem(as_task) => {
                html.push_str(if *as_task { "</div></li>" } else { "</li>" });
            }
            MdEvent::StartTable => html.push_str("<table><tbody>"),
            MdEvent::EndTable => html.push_str("</tbody></table>"),
            MdEvent::StartHead | MdEvent::EndHead => {}
            MdEvent::StartRow => html.push_str("<tr>"),
            MdEvent::EndRow => html.push_str("</tr>"),
            MdEvent::StartCell(head) => {
                html.push_str(if *head { "<th><p>" } else { "<td><p>" });
            }
            MdEvent::EndCell(head) => {
                html.push_str(if *head { "</p></th>" } else { "</p></td>" });
            }
            MdEvent::StartEm => html.push_str("<em>"),
            MdEvent::EndEm => html.push_str("</em>"),
            MdEvent::StartStrong => html.push_str("<strong>"),
            MdEvent::EndStrong => html.push_str("</strong>"),
            MdEvent::StartStrike => html.push_str("<s>"),
            MdEvent::EndStrike => html.push_str("</s>"),
            MdEvent::StartLink(dest, title) => {
                // `link_open`, `:408-415`: unsafe hrefs drop the attribute
                // (the link text stays); titles pass through when set.
                let mut attrs = String::new();
                if is_safe_url(dest) {
                    attrs.push_str(&format!(" href=\"{}\"", escape_html(dest)));
                }
                if !title.is_empty() {
                    attrs.push_str(&format!(" title=\"{}\"", escape_html(title)));
                }
                html.push_str(&format!("<a{attrs}>"));
            }
            MdEvent::EndLink => html.push_str("</a>"),
            MdEvent::Image(dest, title, alt) => {
                // `image`, `:417-431`: unsafe or empty sources keep the
                // alt text (the sanitizer would strip the src and the
                // editor would drop the node).
                if !is_safe_url(dest) || dest.is_empty() {
                    html.push_str(&escape_html(alt));
                } else {
                    let mut attrs = format!(" src=\"{}\"", escape_html(dest));
                    if !alt.is_empty() {
                        attrs.push_str(&format!(" alt=\"{}\"", escape_html(alt)));
                    }
                    if !title.is_empty() {
                        attrs.push_str(&format!(" title=\"{}\"", escape_html(title)));
                    }
                    html.push_str(&format!("<img{attrs}>"));
                }
            }
            MdEvent::Text(text) => html.push_str(&escape_html(text)),
            MdEvent::Code(code) => html.push_str(&format!("<code>{}</code>", escape_html(code))),
            // Raw HTML is disabled at parse time; escape defensively
            // (`html_inline` / `html_block`, `:305-309`).
            MdEvent::HtmlInline(content) => html.push_str(&escape_html(content)),
            MdEvent::HtmlBlock(content) => {
                html.push_str(&format!("<p>{}</p>", escape_html(content.trim())));
            }
            // ProseMirror collapses a paragraph newline to a space anyway.
            MdEvent::SoftBreak => html.push(' '),
            MdEvent::HardBreak => html.push_str("<br>"),
            // `CustomHorizontalRule` renders a wrapper div (`:336-337`).
            MdEvent::Rule => html.push_str("<div data-type=\"horizontalRule\"><div></div></div>"),
        }
        index += 1;
    }
}

/// Whether the paragraph at `open` holds a lone safe image
/// (`TiptapHTMLRenderer._lone_image`, `markdown_converter.py:313-332`):
/// the paragraph is top-level (depth 0) and its single child is an image
/// with a safe non-empty source. Depth here counts blockquote/list/table
/// nesting only (inline nesting never contains a paragraph open).
fn is_lone_image(events: &[MdEvent], open: usize) -> bool {
    if paragraph_depth(events, open) != 0 {
        return false;
    }
    // The paragraph's content is exactly one image event.
    if !matches!(events.get(open + 1), Some(MdEvent::Image(_, _, _))) {
        return false;
    }
    if !matches!(events.get(open + 2), Some(MdEvent::EndParagraph)) {
        return false;
    }
    if let Some(MdEvent::Image(dest, _, _)) = events.get(open + 1) {
        return !dest.is_empty() && is_safe_url(dest);
    }
    false
}

/// The close-side lone-image check (`paragraph_close`, `:329-332`).
fn is_lone_image_close(events: &[MdEvent], close: usize) -> bool {
    if close < 2 {
        return false;
    }
    if !matches!(events.get(close - 1), Some(MdEvent::Image(_, _, _))) {
        return false;
    }
    if let Some(MdEvent::StartParagraph) = events.get(close - 2) {
        return is_lone_image(events, close - 2);
    }
    false
}

/// Block-nesting depth of the paragraph opening at `open` (blockquotes,
/// list items and table cells; the top level is 0).
fn paragraph_depth(events: &[MdEvent], open: usize) -> usize {
    let mut depth: usize = 0;
    for event in &events[..open] {
        match event {
            MdEvent::StartQuote | MdEvent::StartItem(_, _) | MdEvent::StartCell(_) => depth += 1,
            MdEvent::EndQuote | MdEvent::EndItem(_) | MdEvent::EndCell(_) => {
                depth = depth.saturating_sub(1);
            }
            _ => {}
        }
    }
    depth
}

#[cfg(test)]
mod difftest {
    use super::*;

    #[test]
    fn differential_markdown_corpus_scratch() {
        let raw = std::fs::read_to_string("/tmp/md-corpus.json").expect("corpus");
        let vectors: Vec<Map<String, Value>> = serde_json::from_str(&raw).expect("json");
        let mut failures = 0;
        for (index, vector) in vectors.iter().enumerate() {
            let input = vector.get("input").and_then(Value::as_str).unwrap_or("");
            let expected = vector.get("output").and_then(Value::as_str).map(str::to_owned);
            let expected_error = vector.get("error").and_then(Value::as_str).map(str::to_owned);
            let actual = markdown_to_html_port(input);
            let ok = match (&actual, &expected, &expected_error) {
                (Ok(html), Some(want), _) => html == want,
                (Err(message), _, Some(want)) => message == want,
                _ => false,
            };
            if !ok {
                failures += 1;
                if failures <= 60 {
                    println!("--- vector {index} input={input:?}");
                    println!("    want: {expected:?} / err {expected_error:?}");
                    println!("    got:  {actual:?}");
                }
            }
        }
        println!("{}/{} passed", vectors.len() - failures, vectors.len());
        assert_eq!(failures, 0, "differential failures");
    }

    #[test]
    fn scratch_offsets() {
        use pulldown_cmark::{Options, Parser};
        for input in [
            "[a *b*](javascript:x)",
            "![a](javascript:x)",
            "[a][b]\n\n[b]: /url",
            "[a]",
            "- [ ]  two",
            "<http://example.com/a b>",
        ] {
            println!("=== {input:?}");
            let options =
                Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
            for (event, range) in Parser::new_ext(input, options).into_offset_iter() {
                println!("    {range:?} {event:?} slice={:?}", &input[range.clone()]);
            }
        }
    }
}

/// The `prefetch_related("assignees")` ids
/// (`IssueAssignee.objects.filter(issue=...)`, `-created_at` order).
async fn fetch_assignee_ids(pool: &PgPool, issue_id: &Uuid) -> Result<Vec<String>, Denial> {
    let ids: Vec<Uuid> = sqlx::query_scalar(
        r#"SELECT "assignee_id" FROM "issue_assignees" WHERE "issue_id" = $1 AND "deleted_at" IS NULL ORDER BY "created_at" DESC"#,
    )
    .bind(issue_id)
    .fetch_all(pool)
    .await
    .map_err(|error| db_error(error, "issue-assignees"))?;
    Ok(ids.iter().map(ToString::to_string).collect())
}

/// The `prefetch_related("labels")` ids (`IssueLabel.objects.filter(issue=...)`,
/// `-created_at` order).
async fn fetch_label_ids(pool: &PgPool, issue_id: &Uuid) -> Result<Vec<String>, Denial> {
    let ids: Vec<Uuid> = sqlx::query_scalar(
        r#"SELECT "label_id" FROM "issue_labels" WHERE "issue_id" = $1 AND "deleted_at" IS NULL ORDER BY "created_at" DESC"#,
    )
    .bind(issue_id)
    .fetch_all(pool)
    .await
    .map_err(|error| db_error(error, "issue-labels"))?;
    Ok(ids.iter().map(ToString::to_string).collect())
}

// ---------------------------------------------------------------------------
// Pagination
// ---------------------------------------------------------------------------

/// Map a paginator failure: parse errors answer the `ParseError` 400,
/// evaluation failures the generic 500 (the `v1_projects` mapping, shared
/// with `handlers_social`).
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

/// The `_requested_fields` plain names (`serializers/issue.py:172-176`):
/// every `Include` name, including non-field names like
/// `relations_summary`. Empty (no `fields=`) shows the summary keys.
fn requested_names(specs: Option<&[FieldSpec]>) -> Vec<&str> {
    specs
        .unwrap_or(&[])
        .iter()
        .filter_map(|spec| match spec {
            FieldSpec::Include(name) => Some(name.as_str()),
            FieldSpec::Nested(_, _) => None,
        })
        .collect()
}

async fn read_body(body: axum::body::Body) -> Result<Vec<u8>, Denial> {
    axum::body::to_bytes(body, usize::MAX)
        .await
        .map(|bytes| bytes.to_vec())
        .map_err(|error| db_error(error, "read-body"))
}

/// Rebuild the request against the ORIGINAL path for the proxy. The URI
/// must be the request's own: Django's 404 page echoes the path, so
/// rebuilding the `work-items` spelling for a deprecated `issues/` twin
/// answers the wrong bytes (the `handlers_social` precedent).
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

// ---------------------------------------------------------------------------
// Blocker summary (`orchestration/blockers.py:186-198`)
// ---------------------------------------------------------------------------

/// Decoded blocker-target row: issue id, project identifier, sequence,
/// state name, state group.
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
/// (`SUMMARY_LIMIT`), plus the uncapped `has_open_blockers`. Mirrors the
/// `handlers_social` fetch (same source, same rows).
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

// ---------------------------------------------------------------------------
// Grouped relations (`orchestration/relations.py:280-316`)
// ---------------------------------------------------------------------------

/// `get_actual_relation` (`utils/issue_relation_mapper.py`): the type
/// stored in the database for a forward-named relation.
fn stored_relation(relation_type: &str) -> &str {
    match relation_type {
        "start_after" => "start_before",
        "finish_after" => "finish_before",
        "blocking" => "blocked_by",
        "implements" => "implemented_by",
        _ => relation_type,
    }
}

/// `get_inverse_relation` (`utils/issue_relation_mapper.py`): the same
/// edge named from the other end.
fn inverse_relation(relation_type: &str) -> &str {
    match relation_type {
        "start_after" => "start_before",
        "finish_after" => "finish_before",
        "blocked_by" => "blocking",
        "blocking" => "blocked_by",
        "start_before" => "start_after",
        "finish_before" => "finish_after",
        "implemented_by" => "implements",
        "implements" => "implemented_by",
        _ => relation_type,
    }
}

/// `_type_from` (`orchestration/relations.py:100-117`): the relation a
/// stored row expresses, named from the viewpoint's side. Rows stored
/// under a reverse name (which the UI never writes but older data may
/// hold) normalize like `blockers` does.
fn type_from_viewpoint(
    stored: &str,
    issue_id: &Uuid,
    related_issue_id: &Uuid,
    viewpoint_id: &Uuid,
) -> String {
    // `REVERSE_TYPES` (`relations.py:67`).
    let (mut relation, mut anchor, mut _other) = (stored, issue_id, related_issue_id);
    if matches!(
        relation,
        "blocking" | "start_after" | "finish_after" | "implements"
    ) {
        relation = stored_relation(relation);
        (anchor, _other) = (related_issue_id, issue_id);
    }
    if anchor == viewpoint_id {
        relation.to_owned()
    } else {
        inverse_relation(relation).to_owned()
    }
}

/// One owned grouped-relation target: the `_item` fields plus the sort
/// key (`project.identifier`, `sequence_id`).
struct OwnedRelationItem {
    id: String,
    name: String,
    state: Option<String>,
    state_group: Option<String>,
    project_identifier: String,
    sequence_id: i64,
}

/// `grouped_relations(issue, member_project_issues(viewer, slug))`
/// (`relations.py:280-316` + `core/querysets.py:19-30`): every live
/// relation of the issue, grouped by type from its side, the other ends
/// narrowed to the viewer's member projects. Returns the ten
/// [`shape::RELATION_TYPES`] groups in order, each sorted by
/// `(project.identifier, sequence_id)` and capped at
/// [`shape::GROUP_LIMIT`]. Unknown stored types drop (the `in
/// other_by_type` guard).
async fn fetch_grouped_rows(
    pool: &PgPool,
    issue_id: &Uuid,
    workspace_id: &Uuid,
    viewer_id: &Uuid,
) -> Result<Vec<(String, Vec<OwnedRelationItem>)>, Denial> {
    // `IssueRelation.objects.filter(Q(issue|related) =
    // issue, workspace).exclude(self-loop)` — the default manager is the
    // soft-deletion scope.
    let edges: Vec<(Uuid, Uuid, String)> = sqlx::query_as(
        r#"SELECT "issue_id", "related_issue_id", "relation_type" FROM "issue_relations"
           WHERE ("issue_id" = $1 OR "related_issue_id" = $1) AND "workspace_id" = $2
             AND "deleted_at" IS NULL AND NOT ("issue_id" = $1 AND "related_issue_id" = $1)"#,
    )
    .bind(issue_id)
    .bind(workspace_id)
    .fetch_all(pool)
    .await
    .map_err(|error| db_error(error, "relations-edges"))?;
    let mut other_by_type: Vec<(String, std::collections::HashSet<Uuid>)> = shape::RELATION_TYPES
        .iter()
        .map(|name| (name.to_string(), std::collections::HashSet::new()))
        .collect();
    for (anchor, related, stored) in &edges {
        let other = if anchor == issue_id { *related } else { *anchor };
        let relation = type_from_viewpoint(stored, anchor, related, issue_id);
        if let Some(group) = other_by_type.iter_mut().find(|(name, _)| *name == relation) {
            group.1.insert(other);
        }
    }
    let wanted: Vec<Uuid> = other_by_type
        .iter()
        .flat_map(|(_, ids)| ids.iter().copied())
        .collect();
    // The member-project pool (`member_project_issues` + the
    // `workspace_id` narrow + the manager scope), keyed by id. Ordering
    // is cleared (`.order_by()`) — the Rust sort below applies.
    let mut others: HashMap<Uuid, OwnedRelationItem> = HashMap::new();
    if !wanted.is_empty() {
        let rows: Vec<(Uuid, String, i32, Option<String>, Option<String>, String)> = sqlx::query_as(
            format!(
                r#"SELECT "issues"."id", "issues"."name", "issues"."sequence_id", "states"."name", "states"."group", "projects"."identifier"
                   FROM "issues"
                   LEFT OUTER JOIN "states" ON ("issues"."state_id" = "states"."id")
                   INNER JOIN "projects" ON ("issues"."project_id" = "projects"."id")
                   WHERE "issues"."id" = ANY($1) AND "issues"."workspace_id" = $2 AND {} AND EXISTS(
                     SELECT 1 FROM "project_members" pm
                     WHERE pm."project_id" = "issues"."project_id" AND pm."member_id" = $3
                       AND pm."is_active" AND pm."deleted_at" IS NULL
                   )"#,
                core_queries::ISSUE_MANAGER_WHERE
            )
            .as_str(),
        )
        .bind(&wanted)
        .bind(workspace_id)
        .bind(viewer_id)
        .fetch_all(pool)
        .await
        .map_err(|error| db_error(error, "relations-pool"))?;
        for (id, name, sequence_id, state, state_group, project_identifier) in rows {
            others.insert(
                id,
                OwnedRelationItem {
                    id: id.to_string(),
                    name,
                    state,
                    state_group,
                    project_identifier,
                    sequence_id: i64::from(sequence_id),
                },
            );
        }
    }
    let mut groups = Vec::with_capacity(other_by_type.len());
    for (name, ids) in &other_by_type {
        let mut items: Vec<&OwnedRelationItem> =
            ids.iter().filter_map(|id| others.get(id)).collect();
        items.sort_by(|a, b| {
            (&a.project_identifier, a.sequence_id).cmp(&(&b.project_identifier, b.sequence_id))
        });
        items.truncate(shape::GROUP_LIMIT);
        let owned: Vec<OwnedRelationItem> = items
            .into_iter()
            .map(|item| OwnedRelationItem {
                id: item.id.clone(),
                name: item.name.clone(),
                state: item.state.clone(),
                state_group: item.state_group.clone(),
                project_identifier: item.project_identifier.clone(),
                sequence_id: item.sequence_id,
            })
            .collect();
        groups.push((name.clone(), owned));
    }
    Ok(groups)
}

// ---------------------------------------------------------------------------
// Expansions (`api/serializers/base.py:76-116`)
// ---------------------------------------------------------------------------

/// Decoded `file_assets` row for [`file_asset_url`]: entity type plus the
/// nullable scope ids.
type AssetUrlLookup = (Option<String>, Option<Uuid>, Option<Uuid>, Option<Uuid>);

/// `FileAsset.asset_url` (`db/models/asset.py:80-100`) for an asset id:
/// static types render `/api/assets/v2/static/<id>/`, attachments and
/// description assets join their workspace slug. A missing row (or slug)
/// renders null like `getattr` on a dead FK (the `v1_projects`
/// `file_asset_url` precedent, shared with `handlers_social`).
async fn file_asset_url(pool: &PgPool, asset_id: &Uuid) -> Result<Option<String>, Denial> {
    let row: Option<AssetUrlLookup> = sqlx::query_as(
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

/// `expand=state`: `StateLiteSerializer` (`state.py:44-55`) via the D-19
/// kernel. `State.objects` is the triage-excluding `StateManager`
/// (`db/models/state.py:79-83`); the triage arm is unreachable here (the
/// manager scope never yields triage-group issues), so the fetch carries
/// the soft-delete scope only. A missing row renders `{}` via `None`
/// (null FKs never reach here — the caller maps them to `None`).
async fn expand_state(pool: &PgPool, state_id: &Uuid) -> Result<Option<Value>, Denial> {
    use pidash_services::v1_projects::ser_workflow as workflow;
    let row: Option<(String, String, String)> = sqlx::query_as(
        r#"SELECT "name", "color", "group" FROM "states" WHERE "id" = $1 AND "deleted_at" IS NULL"#,
    )
    .bind(state_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| db_error(error, "expand-state"))?;
    let Some((name, color, group)) = row else {
        return Ok(None);
    };
    let id = state_id.to_string();
    let lite = workflow::StateLiteRow {
        id: &id,
        name: &name,
        color: &color,
        group: &group,
    };
    let view = workflow::state_lite_to_representation(&lite);
    serde_json::to_value(&view)
        .map(Some)
        .map_err(|_| Denial::ServerError)
}

/// `expand=project`: `ProjectLiteSerializer` (`project.py:354-377`):
/// `id`, `identifier`, `name`, `cover_image`, `icon_prop`, `emoji`,
/// `description`, `is_default`, `cover_image_url` in field order.
/// `Project.objects` is the plain manager — no liveness scope, so
/// soft-deleted rows still render (Django `getattr` mechanics). A
/// hard-missing row renders `{}` via `None`. Mirrors the
/// `handlers_social` fetch.
async fn expand_project(pool: &PgPool, project_id: &Uuid) -> Result<Option<Value>, Denial> {
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(
        r#"SELECT "id", "identifier", "name", "cover_image", "icon_prop", "emoji", "description", "is_default", "cover_image_asset_id" FROM "projects" WHERE "id" = $1"#,
    )
    .bind(project_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| db_error(error, "expand-project"))?;
    let Some(row) = row else {
        return Ok(None);
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
    Ok(Some(Value::Object(map)))
}

/// `expand=workspace`: `WorkspaceLiteSerializer`
/// (`workspace.py:10-21`): `name`, `slug`, `id` in field order. Plain
/// manager — no liveness scope. A hard-missing row renders `{}` via
/// `None`. Mirrors the `handlers_social` fetch.
async fn expand_workspace(pool: &PgPool, workspace_id: &Uuid) -> Result<Option<Value>, Denial> {
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
            map.insert(
                "id".to_owned(),
                Value::String(workspace_id.to_string()),
            );
            Some(Value::Object(map))
        }
        None => None,
    })
}

/// Decoded user row for [`expand_user`]: names, nullable `email`,
/// `COALESCE`d avatar, optional avatar asset, display name.
type ExpandUserLookup = (String, String, Option<String>, String, Option<Uuid>, Option<String>);

/// `expand=created_by` / `expand=updated_by`: `UserLiteSerializer`
/// (`user.py:13-38`) via the D-19 kernel. `User.objects` is the plain
/// `UserManager` — no liveness scope. A hard-missing row renders `{}`
/// via `None`. Mirrors the `handlers_social` fetch.
async fn expand_user(pool: &PgPool, user_id: &Uuid) -> Result<Option<Value>, Denial> {
    use pidash_services::v1_projects::ser_collab as collab;
    let row: Option<ExpandUserLookup> = sqlx::query_as(
        r#"SELECT "first_name", "last_name", "email", COALESCE("avatar", ''), "avatar_asset_id", "display_name" FROM "users" WHERE "id" = $1"#,
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| db_error(error, "expand-user"))?;
    let Some((first_name, last_name, email, avatar, avatar_asset, display_name)) = row else {
        return Ok(None);
    };
    let asset_url = match avatar_asset {
        Some(id) => file_asset_url(pool, &id).await?,
        None => None,
    };
    let avatar_url = collab::resolve_avatar_url(avatar_asset.is_some(), asset_url.as_deref(), &avatar);
    let id = user_id.to_string();
    let display_name = display_name.unwrap_or_default();
    let lite = collab::UserLiteRow {
        id: &id,
        first_name: &first_name,
        last_name: &last_name,
        email: email.as_deref(),
        avatar: &avatar,
        avatar_url,
        display_name: &display_name,
    };
    let view = collab::user_lite_to_representation(&lite);
    serde_json::to_value(&view)
        .map(Some)
        .map_err(|_| Denial::ServerError)
}

/// `expand=parent`: `IssueLiteSerializer` over the parent issue
/// (`serializers/issue.py:497-508`): `id`, `sequence_id`, `project_id`
/// in field order. `Issue.objects` (the forward-FK manager) is plain —
/// soft-deleted parents still render. A hard-missing row is unreachable
/// (`CASCADE`) and renders `{}` via `None`.
async fn expand_parent(pool: &PgPool, parent_id: &Uuid) -> Result<Option<Value>, Denial> {
    use pidash_services::v1_work_items::shape_labels as labels;
    let row: Option<(Uuid, i32, Uuid)> =
        sqlx::query_as(r#"SELECT "id", "sequence_id", "project_id" FROM "issues" WHERE "id" = $1"#)
            .bind(parent_id)
            .fetch_optional(pool)
            .await
            .map_err(|error| db_error(error, "expand-parent"))?;
    let Some((id, sequence_id, project_id)) = row else {
        return Ok(None);
    };
    let id_text = id.to_string();
    let project_text = project_id.to_string();
    let rendered = labels::render_issue_lite(&labels::IssueLiteRow {
        id: &id_text,
        sequence_id: Some(i64::from(sequence_id)),
        project_id: &project_text,
    });
    Ok(Some(Value::Object(rendered)))
}

/// `expand=estimate_point`: the API `EstimatePointSerializer` read shape
/// (`estimate.py:25-37`, `fields = "__all__"`) via the D-19 kernel, in
/// DRF wire order. Plain manager — no liveness scope. A hard-missing
/// row renders `{}` via `None`.
async fn expand_estimate_point(
    pool: &PgPool,
    estimate_point_id: &Uuid,
    tz: &Tz,
) -> Result<Option<Value>, Denial> {
    use pidash_services::v1_projects::ser_workflow as workflow;
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(
        r#"SELECT "id", "created_at", "updated_at", "created_by_id", "updated_by_id", "deleted_at", "project_id", "workspace_id", "estimate_id", "key", "description", "value" FROM "estimate_points" WHERE "id" = $1"#,
    )
    .bind(estimate_point_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| db_error(error, "expand-estimate-point"))?;
    let Some(row) = row else {
        return Ok(None);
    };
    let id = row_uuid(&row, "id")?.to_string();
    let created_at = render_dt(&row_datetime(&row, "created_at")?, tz);
    let updated_at = render_dt(&row_datetime(&row, "updated_at")?, tz);
    let created_by = row_uuid_opt(&row, "created_by_id")?.map(|id| id.to_string());
    let updated_by = row_uuid_opt(&row, "updated_by_id")?.map(|id| id.to_string());
    let deleted_at = render_dt_opt(row_datetime_opt(&row, "deleted_at")?, tz);
    let project = row_uuid(&row, "project_id")?.to_string();
    let workspace = row_uuid(&row, "workspace_id")?.to_string();
    let estimate = row_uuid(&row, "estimate_id")?.to_string();
    let key: i32 = row.try_get("key").map_err(|_| Denial::ServerError)?;
    let description = row_string(&row, "description")?;
    let value = row_string(&row, "value")?;
    let point_row = workflow::EstimatePointRow {
        id: &id,
        created_at: &created_at,
        updated_at: &updated_at,
        created_by: created_by.as_deref(),
        updated_by: updated_by.as_deref(),
        deleted_at: deleted_at.as_deref(),
        project: &project,
        workspace: &workspace,
        estimate: &estimate,
        key,
        description: &description,
        value: &value,
    };
    let view = workflow::estimate_point_to_representation(&point_row);
    serde_json::to_value(&view)
        .map(Some)
        .map_err(|_| Denial::ServerError)
}

/// One owned assignee row for `expand=assignees`: the `UserLite` inputs
/// plus the resolved avatar URL text. Fetched in
/// `User.objects.filter(pk__in=...)` order (`-created_at`,
/// `db/models/user.py:137`).
struct OwnedAssigneeRow {
    id: String,
    first_name: String,
    last_name: String,
    email: Option<String>,
    avatar: String,
    avatar_url: Option<String>,
    display_name: String,
}

/// `expand=assignees`: `UserLiteSerializer(User.objects.filter(pk__in=...),
/// many=True)` (`serializers/issue.py:445-452`). Unscoped, `-created_at`
/// order; deactivated users still render.
async fn fetch_assignee_rows(
    pool: &PgPool,
    issue_id: &Uuid,
) -> Result<Vec<OwnedAssigneeRow>, Denial> {
    let rows: Vec<(Uuid, String, String, Option<String>, String, Option<Uuid>, Option<String>)> =
        sqlx::query_as(
            r#"SELECT u."id", u."first_name", u."last_name", u."email", COALESCE(u."avatar", ''), u."avatar_asset_id", u."display_name"
               FROM "users" u WHERE u."id" IN (
                 SELECT "assignee_id" FROM "issue_assignees" WHERE "issue_id" = $1 AND "deleted_at" IS NULL
               ) ORDER BY u."created_at" DESC"#,
        )
        .bind(issue_id)
        .fetch_all(pool)
        .await
        .map_err(|error| db_error(error, "expand-assignees"))?;
    let mut out = Vec::with_capacity(rows.len());
    for (id, first_name, last_name, email, avatar, avatar_asset, display_name) in rows {
        let asset_url = match avatar_asset {
            Some(asset) => file_asset_url(pool, &asset).await?,
            None => None,
        };
        let resolved = pidash_services::v1_projects::ser_collab::resolve_avatar_url(
            avatar_asset.is_some(),
            asset_url.as_deref(),
            &avatar,
        );
        let avatar_url = resolved.map(str::to_owned);
        out.push(OwnedAssigneeRow {
            id: id.to_string(),
            first_name,
            last_name,
            email,
            avatar,
            avatar_url,
            display_name: display_name.unwrap_or_default(),
        });
    }
    Ok(out)
}

/// `expand=labels`: `LabelSerializer(Label.objects.filter(pk__in=...),
/// many=True)` (`serializers/issue.py:459-466`) — the full label read
/// shape per row, nested construction (no `fields=`, no `expand=`).
/// Unscoped, `-created_at` order (`db/models/label.py:44`).
async fn fetch_expanded_labels(
    pool: &PgPool,
    issue_id: &Uuid,
    tz: &Tz,
) -> Result<Vec<Value>, Denial> {
    use pidash_services::v1_work_items::shape_labels as labels;
    let rows: Vec<sqlx::postgres::PgRow> = sqlx::query(
        r#"SELECT l."id", l."created_at", l."updated_at", l."deleted_at", l."name", l."description", l."color", l."sort_order", l."external_source", l."external_id", l."created_by_id", l."updated_by_id", l."workspace_id", l."project_id", l."parent_id"
           FROM "labels" l WHERE l."id" IN (
             SELECT "label_id" FROM "issue_labels" WHERE "issue_id" = $1 AND "deleted_at" IS NULL
           ) ORDER BY l."created_at" DESC"#,
    )
    .bind(issue_id)
    .fetch_all(pool)
    .await
    .map_err(|error| db_error(error, "expand-labels"))?;
    let mut out = Vec::with_capacity(rows.len());
    for row in &rows {
        let id = row_uuid(row, "id")?.to_string();
        let created_at = render_dt(&row_datetime(row, "created_at")?, tz);
        let updated_at = render_dt(&row_datetime(row, "updated_at")?, tz);
        let deleted_at = render_dt_opt(row_datetime_opt(row, "deleted_at")?, tz);
        let name = row_string(row, "name")?;
        let description = row_string(row, "description")?;
        let color = row_string(row, "color")?;
        let sort_order = row_f64(row, "sort_order")?;
        let external_source = row_string_opt(row, "external_source")?;
        let external_id = row_string_opt(row, "external_id")?;
        let created_by = row_uuid_opt(row, "created_by_id")?.map(|id| id.to_string());
        let updated_by = row_uuid_opt(row, "updated_by_id")?.map(|id| id.to_string());
        let workspace = row_uuid(row, "workspace_id")?.to_string();
        let project = row_uuid_opt(row, "project_id")?.map(|id| id.to_string());
        let parent = row_uuid_opt(row, "parent_id")?.map(|id| id.to_string());
        let label_row = labels::LabelRow {
            id: &id,
            created_at: &created_at,
            updated_at: &updated_at,
            deleted_at: deleted_at.as_deref(),
            name: &name,
            description: &description,
            color: &color,
            sort_order,
            external_source: external_source.as_deref(),
            external_id: external_id.as_deref(),
            created_by: created_by.as_deref(),
            updated_by: updated_by.as_deref(),
            workspace: &workspace,
            project: project.as_deref(),
            parent: parent.as_deref(),
        };
        let no_expand: &[&str] = &[];
        let no_expansions: &[(&str, Option<Value>)] = &[];
        let rendered = labels::render_label(&labels::LabelRepresentationInput {
            row: &label_row,
            fields: None,
            expand: no_expand,
            expansions: no_expansions,
        })
        .map_err(|_| Denial::ServerError)?;
        out.push(Value::Object(rendered));
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Read rendering (`IssueSerializer.to_representation`)
// ---------------------------------------------------------------------------

/// What one `render_decoded` call renders: the field selection plus the
/// payload kind. `viewer` is the relations viewer (`Some` on the three
/// single-item reads that pass `RELATIONS_VIEWER_CONTEXT`, `None` on
/// lists and the external-id branch).
struct RenderRequest<'a> {
    field_specs: Option<&'a [FieldSpec]>,
    kept: &'a [String],
    expand: &'a [&'a str],
    is_list: bool,
    viewer: Option<Uuid>,
    web_base: Option<&'a str>,
}

/// Render one decoded row through [`shape::render_issue`]: the id lists,
/// the `?fields=`-gated blocker summary and viewer relations, and the
/// kept `expand` values. Every fetch below mirrors a Python query that
/// only runs when its gate passes, so gated-out keys cost no queries.
async fn render_decoded(
    pool: &PgPool,
    decoded: &DecodedIssue,
    tz: &Tz,
    req: &RenderRequest<'_>,
) -> Result<Value, Denial> {
    let kept = |name: &str| req.kept.iter().any(|kept| kept == name);
    let expand = |name: &str| req.expand.contains(&name);
    let requested = requested_names(req.field_specs);

    let url = shape::issue_url(
        req.web_base,
        Some(decoded.workspace_slug.as_str()),
        Some(decoded.project_identifier.as_str()),
        Some(decoded.sequence_id),
    );
    let row = shape::IssueRow {
        id: &decoded.id,
        type_id: decoded.type_id.as_deref(),
        url,
        created_at: &decoded.created_at,
        updated_at: &decoded.updated_at,
        deleted_at: decoded.deleted_at.as_deref(),
        point: decoded.point,
        name: &decoded.name,
        description_html: &decoded.description_html,
        description_binary: decoded.description_binary.as_deref(),
        priority: &decoded.priority,
        complexity_score: decoded.complexity_score,
        start_date: decoded.start_date.as_deref(),
        target_date: decoded.target_date.as_deref(),
        sequence_id: decoded.sequence_id,
        sort_order: decoded.sort_order,
        completed_at: decoded.completed_at.as_deref(),
        archived_at: decoded.archived_at.as_deref(),
        is_draft: decoded.is_draft,
        external_source: decoded.external_source.as_deref(),
        external_id: decoded.external_id.as_deref(),
        git_work_branch: &decoded.git_work_branch,
        created_via: decoded.created_via.as_deref(),
        agent_executor: decoded.agent_executor.as_deref(),
        created_by: decoded.created_by.as_deref(),
        updated_by: decoded.updated_by.as_deref(),
        project: &decoded.project,
        workspace: &decoded.workspace,
        parent: decoded.parent.as_deref(),
        state: decoded.state.as_deref(),
        estimate_point: decoded.estimate_point.as_deref(),
        assigned_pod: decoded.assigned_pod.as_deref(),
    };

    let issue_id = decoded.id.parse::<Uuid>().map_err(|_| Denial::ServerError)?;

    // `assignees` / `labels` (`issue.py:442-468`): ids always when kept,
    // objects under `expand=`.
    let mut assignee_ids: Vec<String> = Vec::new();
    let mut label_ids: Vec<String> = Vec::new();
    let mut owned_assignees: Vec<OwnedAssigneeRow> = Vec::new();
    let mut expanded_labels: Vec<Value> = Vec::new();
    if kept("assignees") {
        if expand("assignees") {
            owned_assignees = fetch_assignee_rows(pool, &issue_id).await?;
        } else {
            assignee_ids = fetch_assignee_ids(pool, &issue_id).await?;
        }
    }
    if kept("labels") {
        if expand("labels") {
            expanded_labels = fetch_expanded_labels(pool, &issue_id, tz).await?;
        } else {
            label_ids = fetch_label_ids(pool, &issue_id).await?;
        }
    }
    let assignee_refs: Vec<&str> = assignee_ids.iter().map(String::as_str).collect();
    let label_refs: Vec<&str> = label_ids.iter().map(String::as_str).collect();
    let user_rows: Vec<pidash_services::v1_projects::ser_collab::UserLiteRow<'_>> =
        owned_assignees
            .iter()
            .map(|row| pidash_services::v1_projects::ser_collab::UserLiteRow {
                id: &row.id,
                first_name: &row.first_name,
                last_name: &row.last_name,
                email: row.email.as_deref(),
                avatar: &row.avatar,
                avatar_url: row.avatar_url.as_deref(),
                display_name: &row.display_name,
            })
            .collect();

    // Blocker summary (`issue.py:470-481`): single payloads only,
    // `?fields=`-gated.
    let summary_gate = !req.is_list
        && (requested.is_empty()
            || requested
                .iter()
                .any(|name| shape::RELATIONS_SUMMARY_KEYS.contains(name)));
    let mut blocker_rows: Option<BlockerRows> = None;
    if summary_gate {
        blocker_rows = Some(fetch_blocker_rows(pool, &issue_id).await?);
    }
    let blockers = blocker_rows.as_ref().map(|rows| shape::BlockerSummary {
        blocked_by: rows
            .blocked_by
            .iter()
            .map(|(identifier, state, group)| shape::SummaryItem {
                identifier: identifier.clone(),
                state: state.as_deref(),
                state_group: group.as_deref(),
            })
            .collect(),
        blocking: rows
            .blocking
            .iter()
            .map(|(identifier, state, group)| shape::SummaryItem {
                identifier: identifier.clone(),
                state: state.as_deref(),
                state_group: group.as_deref(),
            })
            .collect(),
        has_open_blockers: rows.has_open_blockers,
    });

    // Viewer relations (`issue.py:483-492`): viewer set, single payload,
    // `?fields=`-gated.
    let relations_gate = req.viewer.is_some()
        && !req.is_list
        && (requested.is_empty() || requested.contains(&"relations"));
    let mut grouped: Vec<(String, Vec<OwnedRelationItem>)> = Vec::new();
    if relations_gate {
        let workspace_id = decoded
            .workspace
            .parse::<Uuid>()
            .map_err(|_| Denial::ServerError)?;
        grouped = fetch_grouped_rows(pool, &issue_id, &workspace_id, &req.viewer.expect("gated")).await?;
    }
    let mut relation_lists: Vec<Vec<shape::RelationItem<'_>>> = Vec::with_capacity(grouped.len());
    for (_, items) in &grouped {
        relation_lists.push(
            items
                .iter()
                .map(|item| shape::RelationItem {
                    id: &item.id,
                    identifier: format!("{}-{}", item.project_identifier, item.sequence_id),
                    name: &item.name,
                    state: item.state.as_deref(),
                    state_group: item.state_group.as_deref(),
                })
                .collect(),
        );
    }
    // The ten lists in `RELATION_TYPES` order (the fetch returns them in
    // that order); absent groups (never — the fetch always returns ten)
    // read as empty.
    let at = |index: usize| relation_lists.get(index).map(Vec::as_slice).unwrap_or(&[]);
    let relations = if relations_gate {
        Some(shape::GroupedRelations {
            blocked_by: at(0).to_vec(),
            blocking: at(1).to_vec(),
            relates_to: at(2).to_vec(),
            duplicate: at(3).to_vec(),
            start_before: at(4).to_vec(),
            start_after: at(5).to_vec(),
            finish_before: at(6).to_vec(),
            finish_after: at(7).to_vec(),
            implemented_by: at(8).to_vec(),
            implements: at(9).to_vec(),
        })
    } else {
        None
    };

    // Base expansions (`base.py:76-116`): kept map-hit names only. Null
    // FKs map to `None` (`{}`); the fetch fns map hard-missing rows the
    // same way.
    let mut expansions: Vec<(&str, Option<Value>)> = Vec::new();
    let parse_id = |text: &str| text.parse::<Uuid>().map_err(|_| Denial::ServerError);
    for name in shape::BASE_EXPANSION_NAMES {
        if !(req.expand.contains(name) && kept(name)) {
            continue;
        }
        let value: Option<Value> = match *name {
            "state" => match decoded.state.as_deref() {
                Some(id) => expand_state(pool, &parse_id(id)?).await?,
                None => None,
            },
            "project" => expand_project(pool, &parse_id(&decoded.project)?).await?,
            "workspace" => expand_workspace(pool, &parse_id(&decoded.workspace)?).await?,
            "created_by" => match decoded.created_by.as_deref() {
                Some(id) => expand_user(pool, &parse_id(id)?).await?,
                None => None,
            },
            "updated_by" => match decoded.updated_by.as_deref() {
                Some(id) => expand_user(pool, &parse_id(id)?).await?,
                None => None,
            },
            "parent" => match decoded.parent.as_deref() {
                Some(id) => expand_parent(pool, &parse_id(id)?).await?,
                None => None,
            },
            "estimate_point" => match decoded.estimate_point.as_deref() {
                Some(id) => expand_estimate_point(pool, &parse_id(id)?, tz).await?,
                None => None,
            },
            // Other map names (`user`, `issue`, `actor`, ...) are not
            // issue fields, so they are never kept — unreachable.
            _ => return Err(Denial::ServerError),
        };
        expansions.push((name, value));
    }

    // Every `RenderError` arm is 500-class: `Fields` is unreachable
    // (query strings only build `Include` specs), the binary/NaN arms
    // reproduce Django 500s, and the `Missing*` arms are unreachable by
    // construction (the gates above fetch what the branch needs).
    let rendered = shape::render_issue(&shape::RepresentationInput {
        row: &row,
        fields: req.field_specs,
        expand: req.expand,
        is_list: req.is_list,
        assignee_ids: &assignee_refs,
        assignee_rows: &user_rows,
        label_ids: &label_refs,
        expanded_labels: &expanded_labels,
        blockers: blockers.as_ref(),
        relations: relations.as_ref(),
        expansions: &expansions,
    })
    .map_err(|_| Denial::ServerError)?;
    Ok(Value::Object(rendered))
}

// ---------------------------------------------------------------------------
// Read handlers
// ---------------------------------------------------------------------------

/// Split a by-identifier segment at the LAST `-` (`<str>-<str>` in
/// `urls/work_item.py:44-48,113-117`): Django's greedy `[^/]+` arms
/// backtrack so the project part takes everything up to the last dash.
/// `None` (no dash, or an empty side) never matches a Django pattern —
/// the caller proxies so Django's 404 answers.
fn split_identifier_segment(segment: &str) -> Option<(&str, &str)> {
    let (project, issue) = segment.rsplit_once('-')?;
    if project.is_empty() || issue.is_empty() {
        return None;
    }
    Some((project, issue))
}

/// `GET .../work-items/<project>-<issue>/` and the deprecated twin
/// (`views/issue.py:244-270`).
pub async fn get_by_identifier(
    State(state): State<AppState>,
    OriginalUri(original): OriginalUri,
    Path((slug, segment)): Path<(String, String)>,
    Query(query): Query<QueryMap>,
    headers: HeaderMap,
) -> Response {
    // URL resolving precedes auth: an unmatchable segment proxies before
    // the 401 check runs.
    let Some((project_identifier, issue_identifier)) = split_identifier_segment(&segment) else {
        return proxy_request(&state, "GET", original.to_string()).await;
    };
    let project_identifier = project_identifier.to_owned();
    let issue_identifier = issue_identifier.to_owned();
    match by_identifier_inner(
        &state,
        &headers,
        &slug,
        &project_identifier,
        &issue_identifier,
        &query,
    )
    .await
    {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

async fn by_identifier_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    project_identifier: &str,
    issue_identifier: &str,
    query: &QueryMap,
) -> Result<Response, Denial> {
    let pre = preamble(state, headers, slug).await?;
    let workspace_id = pre.workspace_id.ok_or(Denial::Forbidden)?;
    require_gate(
        &pre.pool,
        &workspace_id,
        slug,
        &pre.actor.id,
        None,
        Some(project_identifier),
        V1WorkItemsRoute::ByIdentifier,
        "GET",
    )
    .await?;
    // `sequence_id=issue_identifier` (`:259`): `IntegerField` runs
    // `int()` — whitespace/sign/underscores accepted, anything else a
    // `ValueError` into the 500. Out-of-`i64` magnitudes saturate and
    // miss (Python's unbounded int misses the same way).
    let sequence = parse_python_int(issue_identifier).ok_or(Denial::ServerError)?;
    let mut binds: HashMap<&str, WhereBind<'_>> = HashMap::new();
    binds.insert("project_identifier", WhereBind::Text(project_identifier));
    binds.insert("sequence", WhereBind::Integer(sequence));
    binds.insert("slug", WhereBind::Text(slug));
    let rows = fetch_lookup_rows(
        &pre.pool,
        &core_queries::inline_lookup_joins_sql(),
        &core_queries::workspace_get_lookup_where(),
        &binds,
        &activate_timezone(pre.actor.timezone.as_deref())?,
    )
    .await?;
    let decoded = only_row(rows)?;
    let tz = activate_timezone(pre.actor.timezone.as_deref())?;
    let (field_specs, kept, expand) = field_selection(query, shape::FIELDS_IN_ORDER)?;
    let expand_refs: Vec<&str> = expand.iter().map(String::as_str).collect();
    let web_base = shape::web_base_url(
        state.settings().urls.web_url.as_deref(),
        state.settings().urls.app_base_url.as_deref(),
    );
    let rendered = render_decoded(
        &pre.pool,
        &decoded,
        &tz,
        &RenderRequest {
            field_specs: field_specs.as_deref(),
            kept: &kept,
            expand: &expand_refs,
            is_list: false,
            viewer: Some(pre.actor.id),
            web_base: web_base.as_deref(),
        },
    )
    .await?;
    Ok(json_response(
        StatusCode::OK,
        serde_json::to_string(&rendered).map_err(|_| Denial::ServerError)?,
    ))
}

/// Resolve the `state`/`labels` lookup rows plus one parent id per
/// `PROJ-123` token, then compile the list filters
/// (`utils/issue_filters.py:573-654`). `resolve_parent` is sync, so the
/// tokens pre-resolve through [`core_queries::resolve_parents`] with a
/// recording stub (token-shape errors surface identically) and the
/// compile closure reads the recorded map.
async fn compile_list_filters(
    pool: &PgPool,
    slug: &str,
    project_id: &Uuid,
    query: &QueryMap,
) -> Result<pidash_db::issue_filters::IssueFilter, Denial> {
    let mut params: HashMap<String, String> = HashMap::new();
    for key in core_queries::WORK_ITEM_LIST_FILTER_KEYS {
        if let Some(value) = query_last(query, key) {
            params.insert(key.to_string(), value);
        }
    }
    let mut binds: HashMap<&str, WhereBind<'_>> = HashMap::new();
    binds.insert("project_id", WhereBind::Uuid(project_id));
    binds.insert("workspace_slug", WhereBind::Text(slug));
    let mut states_query = sqlx::QueryBuilder::<sqlx::Postgres>::new("");
    push_where(&mut states_query, &core_queries::states_lookup_sql(), &binds)?;
    let state_rows: Vec<(Uuid, String)> = states_query
        .build_query_as()
        .fetch_all(pool)
        .await
        .map_err(|error| db_error(error, "filter-states"))?;
    let mut labels_query = sqlx::QueryBuilder::<sqlx::Postgres>::new("");
    push_where(&mut labels_query, &core_queries::labels_lookup_sql(), &binds)?;
    let label_rows: Vec<(Uuid, String)> = labels_query
        .build_query_as()
        .fetch_all(pool)
        .await
        .map_err(|error| db_error(error, "filter-labels"))?;
    let states: Vec<(String, String)> = state_rows
        .into_iter()
        .map(|(id, name)| (id.to_string(), name))
        .collect();
    let labels: Vec<(String, String)> = label_rows
        .into_iter()
        .map(|(id, name)| (id.to_string(), name))
        .collect();
    // Pre-resolve parent identifier tokens (see doc comment).
    let parent_tokens = core_queries::split_tokens(params.get("parent").map(String::as_str));
    let pairs = std::cell::RefCell::new(Vec::<(String, i64)>::new());
    core_queries::resolve_parents(&parent_tokens, |identifier: &str, sequence: i64| {
        pairs.borrow_mut().push((identifier.to_owned(), sequence));
        Some(String::new())
    })
    .map_err(filter_denial)?;
    let pairs = pairs.into_inner();
    let mut resolved: HashMap<(String, i64), Option<String>> = HashMap::new();
    for (identifier, sequence) in &pairs {
        if resolved.contains_key(&(identifier.clone(), *sequence)) {
            continue;
        }
        let mut parent_binds: HashMap<&str, WhereBind<'_>> = HashMap::new();
        parent_binds.insert("project_identifier", WhereBind::Text(identifier));
        parent_binds.insert("sequence", WhereBind::Integer(*sequence));
        parent_binds.insert("workspace_slug", WhereBind::Text(slug));
        let mut parent_query = sqlx::QueryBuilder::<sqlx::Postgres>::new("");
        push_where(
            &mut parent_query,
            &core_queries::parent_lookup_sql(),
            &parent_binds,
        )?;
        let found: Option<Uuid> = parent_query
            .build_query_scalar()
            .fetch_optional(pool)
            .await
            .map_err(|error| db_error(error, "filter-parent"))?
            .flatten();
        resolved.insert(
            (identifier.clone(), *sequence),
            found.map(|id| id.to_string()),
        );
    }
    let today = chrono::Utc::now().date_naive();
    core_queries::compile_work_item_filters(
        &params,
        &states,
        &labels,
        |identifier: &str, sequence: i64| {
            resolved
                .get(&(identifier.to_owned(), sequence))
                .cloned()
                .flatten()
        },
        today,
    )
    .map_err(filter_denial)
}

/// Map a filter compilation failure: message errors answer the
/// view-inline `{"error": ...}` 400 (`views/issue.py:369-370`), the
/// unreachable date overflow the generic 500.
fn filter_denial(error: core_queries::WorkItemFilterError) -> Denial {
    match error {
        core_queries::WorkItemFilterError::Message(text) => Denial::BadError(text),
        core_queries::WorkItemFilterError::DateOverflow => Denial::ServerError,
    }
}

/// `GET .../work-items/` and the deprecated twin
/// (`views/issue.py:339-444`): the external-id early return plus the
/// filtered, ordered, paginated list.
pub async fn get_issue_list(
    State(state): State<AppState>,
    Path((slug, project_id)): Path<(String, String)>,
    Query(query): Query<QueryMap>,
    headers: HeaderMap,
) -> Response {
    match issue_list_inner(&state, &headers, &slug, &project_id, &query).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

async fn issue_list_inner(
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
        Some(&project_id),
        None,
        V1WorkItemsRoute::IssueList,
        "GET",
    )
    .await?;
    let tz = activate_timezone(pre.actor.timezone.as_deref())?;
    let (field_specs, kept, expand) = field_selection(query, shape::FIELDS_IN_ORDER)?;
    let expand_refs: Vec<&str> = expand.iter().map(String::as_str).collect();
    let web_base = shape::web_base_url(
        state.settings().urls.web_url.as_deref(),
        state.settings().urls.app_base_url.as_deref(),
    );

    // The external-id early return (`:346-359`): plain `objects` scope,
    // no viewer relations.
    let external_id = query_last(query, "external_id").unwrap_or_default();
    let external_source = query_last(query, "external_source").unwrap_or_default();
    if !external_id.is_empty() && !external_source.is_empty() {
        let mut binds: HashMap<&str, WhereBind<'_>> = HashMap::new();
        binds.insert("external_id", WhereBind::Text(&external_id));
        binds.insert("external_source", WhereBind::Text(&external_source));
        binds.insert("project_id", WhereBind::Uuid(&project_id));
        binds.insert("slug", WhereBind::Text(slug));
        let rows = fetch_lookup_rows(
            &pre.pool,
            &core_queries::external_id_lookup_joins_sql(),
            &core_queries::external_id_lookup_where(),
            &binds,
            &tz,
        )
        .await?;
        let decoded = only_row(rows)?;
        let rendered = render_decoded(
            &pre.pool,
            &decoded,
            &tz,
            &RenderRequest {
                field_specs: field_specs.as_deref(),
                kept: &kept,
                expand: &expand_refs,
                is_list: false,
                viewer: None,
                web_base: web_base.as_deref(),
            },
        )
        .await?;
        return Ok(json_response(
            StatusCode::OK,
            serde_json::to_string(&rendered).map_err(|_| Denial::ServerError)?,
        ));
    }

    let order_param = query_last(query, "order_by");
    let order_param = order_param.as_deref().unwrap_or("-created_at");
    let filters = compile_list_filters(&pre.pool, slug, &project_id, query).await?;
    let per_page =
        crate::paginator::parse_per_page(query_last(query, "per_page").as_deref(), 1000, 1000)
            .map_err(page_denial)?;
    let cursor_raw = query_last(query, "cursor").unwrap_or_else(|| format!("{per_page}:0:0"));
    let cursor = crate::paginator::Cursor::from_string(&cursor_raw).map_err(page_denial)?;
    let window = crate::paginator::offset_window(
        per_page,
        cursor.offset,
        cursor.value,
        cursor.is_prev,
        None,
    )
    .map_err(page_denial)?;
    // `results[:limit]` runs on the lazy queryset, whose negative limit
    // raises `ValueError` into the 500 — after the offset checks above
    // (a negative offset still 400s first).
    if per_page < 0 {
        return Err(page_denial(crate::paginator::PageError::NegativeSlice));
    }
    let (rows, total) = fetch_issue_page(
        &pre.pool,
        slug,
        &project_id,
        &filters,
        order_param,
        window.offset,
        window.stop,
        &tz,
    )
    .await?;
    let has_more = rows.len() as i64 > per_page;
    let trim = (per_page as usize).min(rows.len());
    let mut rendered: Vec<Value> = Vec::with_capacity(trim);
    for decoded in &rows[..trim] {
        rendered.push(
            render_decoded(
                &pre.pool,
                decoded,
                &tz,
                &RenderRequest {
                    field_specs: field_specs.as_deref(),
                    kept: &kept,
                    expand: &expand_refs,
                    is_list: true,
                    viewer: None,
                    web_base: web_base.as_deref(),
                },
            )
            .await?,
        );
    }
    let next = crate::paginator::next_cursor(per_page, cursor.offset, has_more);
    let prev = crate::paginator::prev_cursor(per_page, cursor.offset);
    envelope(total, per_page, &next, &prev, Value::Array(rendered))
}

/// `GET .../work-items/<pk>/` and the deprecated twin
/// (`views/issue.py:593-614`).
pub async fn get_issue_detail(
    State(state): State<AppState>,
    OriginalUri(original): OriginalUri,
    Path((slug, project_id, pk)): Path<(String, String, String)>,
    Query(query): Query<QueryMap>,
    headers: HeaderMap,
) -> Response {
    // `<uuid:pk>` in Django (`urls/work_item.py:54-58,123-127`): a
    // non-UUID segment never resolves — proxy before auth runs.
    if !crate::runner_runs::is_uuid_path_segment(&pk) {
        return proxy_request(&state, "GET", original.to_string()).await;
    }
    let pk = pk.parse::<Uuid>().expect("checked segment");
    match issue_detail_inner(&state, &headers, &slug, &project_id, &pk, &query).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

async fn issue_detail_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    project_id_raw: &str,
    pk: &Uuid,
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
        Some(&project_id),
        None,
        V1WorkItemsRoute::IssueDetail,
        "GET",
    )
    .await?;
    let tz = activate_timezone(pre.actor.timezone.as_deref())?;
    let mut binds: HashMap<&str, WhereBind<'_>> = HashMap::new();
    binds.insert("pk", WhereBind::Uuid(pk));
    binds.insert("project_id", WhereBind::Uuid(&project_id));
    binds.insert("slug", WhereBind::Text(slug));
    let rows = fetch_lookup_rows(
        &pre.pool,
        &core_queries::inline_lookup_joins_sql(),
        &core_queries::detail_get_lookup_where(),
        &binds,
        &tz,
    )
    .await?;
    let decoded = only_row(rows)?;
    let (field_specs, kept, expand) = field_selection(query, shape::FIELDS_IN_ORDER)?;
    let expand_refs: Vec<&str> = expand.iter().map(String::as_str).collect();
    let web_base = shape::web_base_url(
        state.settings().urls.web_url.as_deref(),
        state.settings().urls.app_base_url.as_deref(),
    );
    let rendered = render_decoded(
        &pre.pool,
        &decoded,
        &tz,
        &RenderRequest {
            field_specs: field_specs.as_deref(),
            kept: &kept,
            expand: &expand_refs,
            is_list: false,
            viewer: Some(pre.actor.id),
            web_base: web_base.as_deref(),
        },
    )
    .await?;
    Ok(json_response(
        StatusCode::OK,
        serde_json::to_string(&rendered).map_err(|_| Denial::ServerError)?,
    ))
}
