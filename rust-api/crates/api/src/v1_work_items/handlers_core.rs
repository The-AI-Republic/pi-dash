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
//! * The orchestration `post_save` signal (`orchestration/signals.py`)
//!   fires on the patch, create and delete paths (PIDASHCONV-804,
//!   PIDASHCONV-1216): `capture_prior_state` before the save,
//!   `fire_state_transition` after, through the one live seam in
//!   [`crate::issue_transition`] (the jobs `LiveCreationStore` for the
//!   run-creation writers + the pool for the ticker/clock, preflight
//!   through the jobs driver with the actions `LiveMatcher`). Arm, retime,
//!   disarm, entry queueing, the no-eligible-runner bounce, and the entry
//!   `agent_run` creation all run; a failed handler is logged and the save
//!   stands, so `resolve_moved_by_run`'s 400s still match.
//! * `PUT` on detail proxies to Django (see above).
//! * Non-string `created_at` overrides answer 500, as Django's uncaught
//!   `TypeError` out of `parse_datetime` does.
//!
//! Fixture: `F18-11` (`rust-api/fixtures/v1_work_items/handlers/` —
//! `by_identifier`, `by_identifier_dup_sequence`, `list`, `detail`,
//! `create`, `patch`, `patch_bad_state`, `detail_404`, `create_invalid`,
//! `issue_delete`, `issue_delete_again`, `put_405`, twins). Every `replay_*`
//! test below replays one: status + body byte-identical. The patch
//! signal call is pinned by `patch_signals.golden.json` in the same
//! directory (PIDASHCONV-804).
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::{OriginalUri, Path, Query, State};
use axum::http::{HeaderMap, Request, StatusCode};
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, NaiveDate, TimeZone as _, Utc};
use chrono_tz::Tz;
use sea_orm::EntityTrait;
use serde_json::{Map, Value};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use pidash_auth::permissions::project;
use pidash_auth::scope::TenantScope;
use pidash_db::entity::issues;
use pidash_services::domain::activity::TaskEnqueueHook;
use pidash_services::domain::issues as domain_issues;
use pidash_services::domain::transition::IssueTransitionHook;
use pidash_services::domain::{self, WriteActor, WriteCtx, WriteScope};
/// Canonical home of the stored-HTML round-trip (PIDASHCONV-1554); kept as
/// a re-export so existing `handlers_core::lxml_roundtrip` callers keep working.
pub use pidash_services::html_normalize::lxml_roundtrip;
use pidash_services::orchestration::entries::{FireOutcome, FireRequest};
use pidash_services::read::{
    self, DecodedIssue, RenderRequest, ISSUE_EXTRA_COLS, ISSUE_SELECT_COLS,
};
use pidash_services::v1_work_items::queries_core as core_queries;
use pidash_services::v1_work_items::shape_issue as shape;
use pidash_services::v1_work_items::tasks as work_tasks;
use pidash_services::v1_work_items::{filter_fields, python_number_str, FieldSpec};

use crate::domain_hooks::AppTaskEnqueueHook;
use crate::issue_transition::AppTransitionHook;
use crate::state::AppState;

use super::perms::{decide, gate_for, resolve_moved_by_run, RunFacts, V1WorkItemsRoute};
use pidash_compat::datetime::render_datetime_in;
pub use pidash_compat::response::SERVER_ERROR_BODY;
use pidash_types::runner_runs::AgentRunStatus;

// ---------------------------------------------------------------------------
// Responses
// ---------------------------------------------------------------------------

/// `handle_exception`'s `ObjectDoesNotExist` branch
/// (`api/views/base.py:154-158`): every `.get()` miss on these endpoints.
pub const RESOURCE_NOT_FOUND_BODY: &str = r#"{"error":"The requested resource does not exist."}"#;
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
    pidash_compat::response::json_response(status, pidash_compat::response::escape_u2028(&body))
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
    let message = pidash_jobs::TaskMessage::new(task, args, kwargs);
    let job = pidash_jobs::queue::NewJob::new(
        message.task.clone(),
        Value::Array(message.args.clone()),
        Value::Object(message.kwargs.clone()),
    );
    if let Err(error) = pidash_jobs::queue::enqueue(pool, &job).await {
        tracing::warn!(%error, task, "task enqueue failed; response stands");
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

pub use pidash_compat::http::{fields_param, query_last, OneOrMany, QueryMap};

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
        let _ =
            pidash_services::domain::tokens::touch_api_token_last_used_by_token(pool, raw).await;
    }
    // The token loggers stamp this id on the `process_logs` payload
    // (Django reads `request.user` post-view; PIDASHCONV-937).
    crate::middleware::publish_logger_user_id(&user_id.to_string());
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
        let _ =
            pidash_services::domain::tokens::revoke_machine_token_by_hash(pool, &presented_hash)
                .await;
        return Err(Denial::InvalidToken);
    }
    let _ = pidash_services::domain::tokens::touch_machine_token_last_used_by_hash(
        pool,
        &presented_hash,
    )
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
#[allow(clippy::too_many_arguments)]
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
        Some(pid) => entity_facts(pool, workspace_id, workspace_slug, user_id, pid).await?,
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

fn render_dt(dt: &DateTime<Utc>, tz: &Tz) -> String {
    render_datetime_in(dt, tz)
}

// ---------------------------------------------------------------------------
// Issue reads/renders (the public read service owns the row decode, the
// fetchers and the renderers; the wrappers map failures onto Denial).
// ---------------------------------------------------------------------------

/// Decode one list/detail row via the public read service.
fn decode_issue(row: &sqlx::postgres::PgRow, tz: &Tz) -> Result<DecodedIssue, Denial> {
    read::decode_issue(row, tz).map_err(|_| Denial::ServerError)
}

/// Render one decoded row via the public read service.
async fn render_decoded(
    pool: &PgPool,
    decoded: &DecodedIssue,
    tz: &Tz,
    req: &RenderRequest<'_>,
) -> Result<Value, Denial> {
    read::render_decoded(pool, decoded, tz, req)
        .await
        .map_err(|error| match error {
            read::Error::NotFound | read::Error::Failed(_) => Denial::ServerError,
            read::Error::Db(err) => {
                let (site, source) = err.split();
                db_error(source, site)
            }
        })
}

/// Fetch one row for the write paths via the public read service.
async fn fetch_issue_for_write(
    pool: &PgPool,
    slug: &str,
    project_id: &Uuid,
    pk: &Uuid,
    tz: &Tz,
) -> Result<DecodedIssue, Denial> {
    read::fetch_issue_for_write(pool, slug, project_id, pk, tz)
        .await
        .map_err(|error| match error {
            read::Error::NotFound => Denial::NotFound(RESOURCE_NOT_FOUND_BODY.to_owned()),
            read::Error::Failed(_) => Denial::ServerError,
            read::Error::Db(err) => {
                let (site, source) = err.split();
                db_error(source, site)
            }
        })
}

/// Render a write response / `current_instance` image via the public
/// read service.
async fn render_write_response(
    pool: &PgPool,
    decoded: &DecodedIssue,
    tz: &Tz,
    web_base: Option<&str>,
) -> Result<Value, Denial> {
    read::render_write_response(pool, decoded, tz, web_base)
        .await
        .map_err(|error| match error {
            read::Error::NotFound | read::Error::Failed(_) => Denial::ServerError,
            read::Error::Db(err) => {
                let (site, source) = err.split();
                db_error(source, site)
            }
        })
}

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
            rest = tail.strip_prefix("::").expect("checked cast");
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

fn filter_lists(filters: &pidash_db::issue_filters::IssueFilter) -> Result<FilterLists, Denial> {
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
#[allow(clippy::too_many_arguments)]
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
    let (annotation_sql, joins_sql, order_by_sql, requires_grouping) =
        match core_queries::order_spec(order_param) {
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
    use crate::v1_cycles_modules::json::{
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
                .map(|(key, item)| {
                    format!(
                        "{}: {}",
                        py_repr_quoted(&Value::String(key.clone())),
                        py_repr_quoted(item)
                    )
                })
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
                .map(|(key, item)| {
                    format!(
                        "{}: {}",
                        py_repr_quoted(&Value::String(key.clone())),
                        py_repr_quoted(item)
                    )
                })
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
            Value::Array(items) => Ok(items
                .iter()
                .any(|item| item == &Value::String(key.to_owned()))),
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
        Value::Bool(true) => Ok(1.0),
        Value::Bool(false) => Ok(0.0),
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
                Ok(finite) if finite.is_finite() => Ok(finite),
                _ => Err("Integer value too large to convert to float"),
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
        if lowered == special
            || lowered == format!("+{special}")
            || lowered == format!("-{special}")
        {
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
        Value::String(text) => match text.to_ascii_lowercase().as_str() {
            "t" | "y" | "yes" | "true" | "on" | "1" => Ok(true),
            "f" | "n" | "no" | "false" | "off" | "0" => Ok(false),
            _ => Err(INVALID),
        },
        _ => Err(INVALID),
    }
}

// ---------------------------------------------------------------------------
// DRF date / datetime / choice / pk fields
// ---------------------------------------------------------------------------

/// `DateField` invalid message (`fields.py:1219` over
/// `DATE_INPUT_FORMATS = [iso-8601]`).
const DATE_INVALID_MESSAGE: &str =
    "Date has wrong format. Use one of these formats instead: YYYY-MM-DD.";
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
    let (year, week, weekday) =
        if bytes.len() == 10 && bytes[4] == b'-' && bytes[5] == b'W' && bytes[8] == b'-' {
            (
                text[0..4].parse::<i32>().ok()?,
                text[6..8].parse::<u32>().ok()?,
                text[9..10].parse::<u32>().ok()?,
            )
        } else if bytes.len() == 8
            && bytes[0..4].iter().all(|b| b.is_ascii_digit())
            && bytes[4] == b'W'
        {
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
        chrono::LocalResult::None => {
            Err(format!("Invalid datetime for the timezone \"{tz_name}\"."))
        }
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
    hex = hex
        .trim_matches(|ch| ch == '{' || ch == '}')
        .replace('-', "");
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
        PkTable::Labels => {
            r#"SELECT "id" FROM "labels" WHERE "id" = ANY($1) AND "deleted_at" IS NULL"#
        }
        PkTable::IssueTypes => {
            r#"SELECT "id" FROM "issue_types" WHERE "id" = ANY($1) AND "deleted_at" IS NULL"#
        }
        // `State.objects` is the triage-excluding `StateManager`
        // (`db/models/state.py:91-100`): triage ids miss here (field
        // error), never reaching `validate()`.
        PkTable::States => {
            r#"SELECT "id" FROM "states" WHERE "id" = ANY($1) AND "deleted_at" IS NULL AND NOT ("group" = 'triage')"#
        }
        PkTable::Issues => {
            r#"SELECT "id" FROM "issues" WHERE "id" = ANY($1) AND "deleted_at" IS NULL"#
        }
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
    let complexity_shape: FieldParse<i64> =
        match parse_optional_int(get("complexity_score"), from_form, false, Some((0, 10))) {
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
                match parse_drf_choice(value, &["local_runner", "cloud_agent", "managed_runner"]) {
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
    scalar_field!(ext_source_shape, "external_source", parsed.external_source);
    scalar_field!(ext_id_shape, "external_id", parsed.external_id);
    scalar_field!(branch_shape, "git_work_branch", parsed.git_work_branch);
    scalar_field!(via_shape, "created_via", parsed.created_via);
    scalar_field!(executor_shape, "agent_executor", parsed.agent_executor);
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
fn parse_optional_char(
    value: Option<&Value>,
    max_length: Option<usize>,
) -> FieldParse<Option<String>> {
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
    let scrubbed: String = url
        .chars()
        .filter(|ch| !matches!(ch, '\t' | '\r' | '\n'))
        .collect();
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
    let authority = rest
        .strip_prefix("//")
        .map(|after| after.split(['/', '?', '#']).next().unwrap_or(after));
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
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let events: Vec<pulldown_cmark::Event> = Parser::new_ext(markdown, options).collect();
    let resolved = resolve_task_events(&events);
    let mut html = String::new();
    render_tiptap(&resolved, &mut html);
    if html.is_empty() {
        return Ok(EMPTY_DOCUMENT_HTML.to_owned());
    }
    if html.len() > 10 * 1024 * 1024 {
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
                    for (item_idx, has) in
                        frame.item_indices.iter().zip(frame.item_has_marker.iter())
                    {
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
    // Item (task state, checked flag, open-event index, injected paragraph
    // open) by nesting stack.
    let mut item_stack: Vec<(bool, bool, usize, bool)> = Vec::new();
    // Whether the next text's leading space lstrips (post-marker).
    let mut lstrip_next_text = false;
    // Image alt-text capture: (dest, title, alt parts).
    let mut image_stack: Vec<(String, String, String)> = Vec::new();
    // Table-cell head flags by nesting stack.
    let mut cell_stack: Vec<bool> = Vec::new();
    // Real-paragraph and table-cell depth: injection only fires for bare
    // inline content directly inside a list item.
    let mut para_depth: u32 = 0;
    let mut cell_depth: u32 = 0;
    while index < events.len() {
        // Task-list markers are consumed here, never lowered directly.
        if let Event::TaskListMarker(checked) = events[index] {
            // The marker is the item's first event; its text (literal or
            // following) sits inside the injected paragraph.
            if para_depth == 0 && cell_depth == 0 {
                if let Some(frame) = item_stack.last_mut() {
                    if !frame.3 {
                        out.push(MdEvent::StartParagraph);
                        frame.3 = true;
                    }
                }
            }
            // Find the enclosing item's task state.
            let is_task = item_stack
                .last()
                .map(|(task, _, _, _)| *task)
                .unwrap_or(false);
            if !is_task {
                out.push(MdEvent::Text(if checked {
                    "[x]".to_owned()
                } else {
                    "[ ]".to_owned()
                }));
            } else if let Some(top) = item_stack.last_mut() {
                top.1 = checked;
            }
            lstrip_next_text = is_task;
            index += 1;
            continue;
        }
        // Tight-list paragraphs (`markdown_converter.py:456-460` renders
        // every paragraph token, `hidden` or not): pulldown omits
        // `Start/End(Paragraph)` inside tight items, so inject them
        // around bare inline content. Real paragraphs, table cells (the
        // renderer wraps those), nested-item opens and empty items never
        // inject.
        let is_item_end = matches!(&events[index], Event::End(TagEnd::Item));
        let is_block_start = matches!(
            &events[index],
            Event::Start(
                Tag::Paragraph
                    | Tag::Heading { .. }
                    | Tag::BlockQuote(_)
                    | Tag::CodeBlock(_)
                    | Tag::HtmlBlock
                    | Tag::List(_)
                    | Tag::FootnoteDefinition(_)
                    | Tag::Table(_)
            ) | Event::Rule
                | Event::Html(_)
        );
        let is_inline_content = matches!(
            &events[index],
            Event::Text(_)
                | Event::Code(_)
                | Event::SoftBreak
                | Event::HardBreak
                | Event::InlineHtml(_)
                | Event::FootnoteReference(_)
                | Event::InlineMath(_)
                | Event::DisplayMath(_)
                | Event::Start(
                    Tag::Emphasis
                        | Tag::Strong
                        | Tag::Strikethrough
                        | Tag::Link { .. }
                        | Tag::Image { .. }
                )
                | Event::End(
                    TagEnd::Emphasis
                        | TagEnd::Strong
                        | TagEnd::Strikethrough
                        | TagEnd::Link
                        | TagEnd::Image
                )
        );
        if is_item_end {
            if let Some(frame) = item_stack.last_mut() {
                if frame.3 {
                    out.push(MdEvent::EndParagraph);
                    frame.3 = false;
                }
            }
        } else if is_block_start {
            if para_depth == 0 {
                if let Some(frame) = item_stack.last_mut() {
                    if frame.3 {
                        out.push(MdEvent::EndParagraph);
                        frame.3 = false;
                    }
                }
            }
        } else if is_inline_content && para_depth == 0 && cell_depth == 0 {
            if let Some(frame) = item_stack.last_mut() {
                if !frame.3 {
                    out.push(MdEvent::StartParagraph);
                    frame.3 = true;
                }
            }
        }
        match &events[index] {
            Event::Start(Tag::Paragraph) => {
                para_depth += 1;
                out.push(MdEvent::StartParagraph);
            }
            Event::End(TagEnd::Paragraph) => {
                para_depth = para_depth.saturating_sub(1);
                out.push(MdEvent::EndParagraph);
            }
            Event::Start(Tag::Heading { level, .. }) => {
                out.push(MdEvent::StartHeading(*level as u32))
            }
            Event::End(TagEnd::Heading(_)) => out.push(MdEvent::EndHeading),
            Event::Start(Tag::BlockQuote(_)) => out.push(MdEvent::StartQuote),
            Event::End(TagEnd::BlockQuote(_)) => out.push(MdEvent::EndQuote),
            Event::Start(Tag::CodeBlock(kind)) => {
                use pulldown_cmark::CodeBlockKind;
                let language = match kind {
                    CodeBlockKind::Fenced(info) => {
                        info.split_whitespace().next().unwrap_or("").to_owned()
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
                item_stack.push((as_task, false, open, false));
                out.push(MdEvent::StartItem(as_task, false));
            }
            Event::End(TagEnd::Item) => {
                let (as_task, checked, open, _) =
                    item_stack
                        .pop()
                        .unwrap_or((false, false, usize::MAX, false));
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
                cell_depth += 1;
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
                cell_depth = cell_depth.saturating_sub(1);
                let head = cell_stack.pop().unwrap_or(false);
                out.push(MdEvent::EndCell(head));
            }
            Event::Start(Tag::Emphasis) => out.push(MdEvent::StartEm),
            Event::End(TagEnd::Emphasis) => out.push(MdEvent::EndEm),
            Event::Start(Tag::Strong) => out.push(MdEvent::StartStrong),
            Event::End(TagEnd::Strong) => out.push(MdEvent::EndStrong),
            Event::Start(Tag::Strikethrough) => out.push(MdEvent::StartStrike),
            Event::End(TagEnd::Strikethrough) => out.push(MdEvent::EndStrike),
            Event::Start(Tag::Link {
                dest_url, title, ..
            }) => {
                out.push(MdEvent::StartLink(dest_url.to_string(), title.to_string()));
            }
            Event::End(TagEnd::Link) => out.push(MdEvent::EndLink),
            Event::Start(Tag::Image {
                dest_url, title, ..
            }) => {
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

    /// Tight-list paragraphs (`markdown_converter.py:456-460` renders
    /// every paragraph token): pulldown omits them, the lowering injects
    /// them — pinned live against `markdown_to_html` (Django 4.2 venv).
    #[test]
    fn markdown_tight_list_paragraphs() {
        for (input, expected) in [
            ("- a\n- b", "<ul><li><p>a</p></li><li><p>b</p></li></ul>"),
            ("- a\n\n- b", "<ul><li><p>a</p></li><li><p>b</p></li></ul>"),
            ("- ", "<ul><li></li></ul>"),
            (
                "- a\n  - b",
                "<ul><li><p>a</p><ul><li><p>b</p></li></ul></li></ul>",
            ),
            (
                "- [x] done\n- [ ] todo",
                "<ul data-type=\"taskList\"><li data-type=\"taskItem\" data-checked=\"true\"><label><input type=\"checkbox\" checked=\"checked\"><span></span></label><div><p>done</p></div></li><li data-type=\"taskItem\" data-checked=\"false\"><label><input type=\"checkbox\"><span></span></label><div><p>todo</p></div></li></ul>",
            ),
        ] {
            assert_eq!(markdown_to_html_port(input).as_deref(), Ok(expected), "{input:?}");
        }
    }

    #[test]
    fn differential_markdown_corpus_scratch() {
        // Scratch corpus (regenerated by the markdown probe scripts, never
        // committed): skip loudly when absent so CI stays green.
        let Ok(raw) = std::fs::read_to_string("/tmp/md-corpus.json") else {
            eprintln!("skipping differential_markdown_corpus_scratch: /tmp/md-corpus.json absent");
            return;
        };
        let vectors: Vec<Map<String, Value>> = serde_json::from_str(&raw).expect("json");
        let mut failures = 0;
        for (index, vector) in vectors.iter().enumerate() {
            let input = vector.get("input").and_then(Value::as_str).unwrap_or("");
            let expected = vector
                .get("output")
                .and_then(Value::as_str)
                .map(str::to_owned);
            let expected_error = vector
                .get("error")
                .and_then(Value::as_str)
                .map(str::to_owned);
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

async fn read_body(body: axum::body::Body) -> Result<Vec<u8>, Denial> {
    pidash_compat::http::read_body(body)
        .await
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
    headers: &HeaderMap,
    body: Vec<u8>,
) -> impl std::future::Future<Output = Response> + 'a {
    // The original headers ride along: some proxied targets are authed
    // Django paths (`work-items/search/` lands here through the
    // by-identifier capture), and dropping `X-Api-Key` would 401 them.
    let mut req = Request::builder()
        .method(method)
        .uri(uri)
        .body(axum::body::Body::from(body))
        .expect("proxy request");
    *req.headers_mut() = headers.clone();
    crate::edge::proxy(State(state.clone()), req)
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
        return proxy_request(&state, "GET", original.to_string(), &headers, Vec::new()).await;
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
    push_where(
        &mut states_query,
        &core_queries::states_lookup_sql(),
        &binds,
    )?;
    let state_rows: Vec<(Uuid, String)> = states_query
        .build_query_as()
        .fetch_all(pool)
        .await
        .map_err(|error| db_error(error, "filter-states"))?;
    let mut labels_query = sqlx::QueryBuilder::<sqlx::Postgres>::new("");
    push_where(
        &mut labels_query,
        &core_queries::labels_lookup_sql(),
        &binds,
    )?;
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
        return proxy_request(&state, "GET", original.to_string(), &headers, Vec::new()).await;
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

// ---------------------------------------------------------------------------
// Write validation (`IssueSerializer.validate`, `serializers/issue.py:182-288`)
// ---------------------------------------------------------------------------

/// `base_host(request, is_app=True)` (`utils/host.py:18-66`) for the
/// `model_activity` origin: `APP_BASE_URL`, else `WEB_URL`, else the
/// `ImproperlyConfigured` 500 (raised after every write lands, so the row
/// persists and only the response is the 500).
fn app_origin(urls: &pidash_db::config::UrlSettings) -> Result<String, Denial> {
    if let Some(url) = urls.app_base_url.as_deref().filter(|s| !s.is_empty()) {
        return Ok(url.to_owned());
    }
    if let Some(url) = urls.web_url.as_deref().filter(|s| !s.is_empty()) {
        return Ok(url.to_owned());
    }
    Err(Denial::ServerError)
}

/// The non-dict write body (`serializers.py` `to_internal_value`): DRF
/// answers `{"non_field_errors": ["Invalid data. Expected a dictionary,
/// but got <type>."]}` with the CPython type name.
fn non_dict_body(value: &Value) -> String {
    format!(
        "{{\"non_field_errors\":[{}]}}",
        json_string(&format!(
            "Invalid data. Expected a dictionary, but got {}.",
            json_type_name(value)
        ))
    )
}

/// Map a save-time failure (`IntegrityError` → 400, anything else → 500):
/// Django's `IntegrityError` is pgcode class 23 (not-null 23502, FK 23503,
/// unique 23505, check 23514), so every `23xxx` answers the
/// payload-not-valid body.
fn save_error(error: &sqlx::Error, site: &str) -> Denial {
    let integrity = matches!(error, sqlx::Error::Database(db) if db.code().as_deref().is_some_and(|code| code.starts_with("23")));
    if integrity {
        tracing::warn!(%error, site, "v1_work_items core integrity failure");
        Denial::FieldErrors(PAYLOAD_NOT_VALID_BODY.to_owned())
    } else {
        db_error(error, site)
    }
}

/// The project facts the write paths read: `Project.objects.get(pk)`
/// (`views/issue.py:471,793`) 404s through `handle_exception`, and `post`
/// passes `workspace_id` / `default_assignee_id` into the serializer
/// context.
struct WriteProject {
    workspace_id: Uuid,
    default_assignee_id: Option<Uuid>,
}

async fn fetch_project_for_write(pool: &PgPool, project_id: &Uuid) -> Result<WriteProject, Denial> {
    let row: Option<(Uuid, Option<Uuid>)> =
        sqlx::query_as(r#"SELECT "workspace_id", "default_assignee_id" FROM "projects" WHERE "id" = $1 AND "deleted_at" IS NULL"#)
            .bind(project_id)
            .fetch_optional(pool)
            .await
            .map_err(|error| db_error(error, "write-project"))?;
    match row {
        Some((workspace_id, default_assignee_id)) => Ok(WriteProject {
            workspace_id,
            default_assignee_id,
        }),
        None => Err(Denial::NotFound(RESOURCE_NOT_FOUND_BODY.to_owned())),
    }
}

/// `Issue.save`'s default-state resolution (`db/models/issue.py:288-301`):
/// the `default=True` non-triage state of the project, else the first
/// non-triage state, both in `Meta.ordering = ("sequence",)`. The
/// `is_triage` exclusion rides beside the manager's `group != 'triage'`
/// scope; either miss resolves `None` (kept, never an error).
async fn fetch_default_state(
    pool: &PgPool,
    project_id: &Uuid,
) -> Result<Option<(Uuid, String)>, Denial> {
    for default_only in [true, false] {
        let sql = if default_only {
            r#"SELECT "id", "group" FROM "states" WHERE "project_id" = $1 AND "deleted_at" IS NULL AND NOT ("group" = 'triage') AND NOT "is_triage" AND "default" ORDER BY "sequence" ASC LIMIT 1"#
        } else {
            r#"SELECT "id", "group" FROM "states" WHERE "project_id" = $1 AND "deleted_at" IS NULL AND NOT ("group" = 'triage') AND NOT "is_triage" ORDER BY "sequence" ASC LIMIT 1"#
        };
        let row: Option<(Uuid, String)> = sqlx::query_as(sql)
            .bind(project_id)
            .fetch_optional(pool)
            .await
            .map_err(|error| db_error(error, "write-default-state"))?;
        if row.is_some() {
            return Ok(row);
        }
    }
    Ok(None)
}

/// The `self.state.group` read in `Issue.save`'s completed arm: the
/// validated state arrives as a model instance, so the group is an
/// in-memory attribute read (plain by-id lookup, no manager scope).
async fn fetch_state_group(pool: &PgPool, state_id: &Uuid) -> Result<Option<String>, Denial> {
    sqlx::query_scalar(r#"SELECT "group" FROM "states" WHERE "id" = $1"#)
        .bind(state_id)
        .fetch_optional(pool)
        .await
        .map_err(|error| db_error(error, "write-state-group"))
        .map(|row: Option<String>| row)
}

/// The create-time default type
/// (`serializers/issue.py:297-300`): `IssueType.objects.filter(
/// project_issue_types__project_id=..., is_default=True).first()` —
/// unordered `.first()` (no `Meta.ordering` on `IssueType`), both tables
/// under the soft-deletion scope.
async fn fetch_default_issue_type(
    pool: &PgPool,
    project_id: &Uuid,
) -> Result<Option<Uuid>, Denial> {
    sqlx::query_scalar(
        r#"SELECT "it"."id" FROM "issue_types" AS "it" WHERE "it"."deleted_at" IS NULL AND "it"."is_default" AND EXISTS(SELECT 1 FROM "project_issue_types" AS "pit" WHERE "pit"."issue_type_id" = "it"."id" AND "pit"."project_id" = $1 AND "pit"."deleted_at" IS NULL) LIMIT 1"#,
    )
    .bind(project_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| db_error(error, "write-default-type"))
    .map(|row: Option<Uuid>| row)
}

/// `Issue.has_active_run` (`db/models/issue.py:232-248`):
/// `AgentRun.objects.filter(work_item=self,
/// status__in=NON_TERMINAL_STATUSES).exists()` — the plain manager, no
/// liveness scope. The set is `runner/services/matcher.py:54-66` (the
/// retired `waiting_for_worktree` still gates, PDASHOSS01-137).
const NON_TERMINAL_STATUSES: &[&str] = &[
    "queued",
    "assigned",
    "waiting_for_worktree",
    "running",
    "cancel_requested",
    "awaiting_approval",
    "awaiting_reauth",
    "paused_awaiting_input",
];

async fn issue_has_active_run(pool: &PgPool, issue_id: &Uuid) -> Result<bool, Denial> {
    sqlx::query_scalar(
        r#"SELECT EXISTS(SELECT 1 FROM "agent_run" WHERE "work_item_id" = $1 AND "status" = ANY($2))"#,
    )
    .bind(issue_id)
    .bind(NON_TERMINAL_STATUSES)
    .fetch_optional(pool)
    .await
    .map_err(|error| db_error(error, "write-active-run"))
    .map(|found: Option<bool>| found.unwrap_or(false))
}

/// The pod's project for the `_same_uuid` check
/// (`serializers/issue.py:199-203`): both sides are typed UUIDs here, so
/// the canonical-compare fallback arms are unreachable — plain equality.
async fn fetch_pod_project_id(pool: &PgPool, pod_id: &Uuid) -> Result<Option<Uuid>, Denial> {
    sqlx::query_scalar(r#"SELECT "project_id" FROM "pod" WHERE "id" = $1"#)
        .bind(pod_id)
        .fetch_optional(pool)
        .await
        .map_err(|error| db_error(error, "write-pod-project"))
        .map(|row: Option<Uuid>| row)
}

/// The `ProjectMember` filter backing the assignee allow-list
/// (`serializers/issue.py:246-252`): active members with `role >= 15` in
/// `ProjectMember.Meta.ordering = ("-created_at",)`.
async fn filter_assignee_ids(
    pool: &PgPool,
    project_id: &Uuid,
    ids: &[Uuid],
) -> Result<Vec<Uuid>, Denial> {
    sqlx::query_scalar(
        r#"SELECT "member_id" FROM "project_members" WHERE "project_id" = $1 AND "is_active" AND "role" >= 15 AND "member_id" = ANY($2) AND "deleted_at" IS NULL ORDER BY "created_at" DESC"#,
    )
    .bind(project_id)
    .bind(ids)
    .fetch_all(pool)
    .await
    .map_err(|error| db_error(error, "write-assignee-filter"))
}

/// The `Label` filter backing the label allow-list
/// (`serializers/issue.py:255-259`), in `Label.Meta.ordering =
/// ("-created_at",)`.
async fn filter_label_ids(
    pool: &PgPool,
    project_id: &Uuid,
    ids: &[Uuid],
) -> Result<Vec<Uuid>, Denial> {
    sqlx::query_scalar(
        r#"SELECT "id" FROM "labels" WHERE "project_id" = $1 AND "id" = ANY($2) AND "deleted_at" IS NULL ORDER BY "created_at" DESC"#,
    )
    .bind(project_id)
    .bind(ids)
    .fetch_all(pool)
    .await
    .map_err(|error| db_error(error, "write-label-filter"))
}

/// Port of `IssueSerializer.validate()` (`serializers/issue.py:182-288`),
/// in source order — first failure wins, every arm a 400. `parsed` is the
/// field-level value (mutated in place exactly like `data`: the lxml and
/// sanitizer substitutes, the assignee/label allow-lists). `instance` is
/// the patch/delete current row (`None` on create, skipping the reassign
/// gate). The `description_binary` branch is dead: DRF maps `BinaryField`
/// to a read-only `ModelField` (probed live), so the key never reaches
/// `validated_data`.
async fn run_write_validate(
    pool: &PgPool,
    parsed: &mut ParsedWrite,
    project_id: &Uuid,
    workspace_id: &Uuid,
    instance: Option<&DecodedIssue>,
    from_markdown: bool,
) -> Result<(), Denial> {
    use shape::ValidateError;
    let fail = |error: ValidateError| Denial::FieldErrors(error.body().to_owned());

    // Start / target (`:183-188`).
    if let (Some(Some(start)), Some(Some(target))) = (parsed.start_date, parsed.target_date) {
        if start > target {
            return Err(fail(ValidateError::StartExceedsTarget));
        }
    }

    // Assigned pod (`:193-216`).
    if let Some(pod) = parsed.assigned_pod {
        if let Some(pod_id) = pod {
            let pod_project = fetch_pod_project_id(pool, &pod_id).await?;
            if pod_project.as_ref() != Some(project_id) {
                return Err(fail(ValidateError::PodWrongProject));
            }
        }
        // The reassign gate only exists on update (`self.instance`), and
        // `has_active_run` only queries on a real change — assign,
        // reassign, or clear, compared as `str()` (`None` spells `None`).
        if let Some(current) = instance {
            let new_text = pod.map(|id| id.to_string());
            if new_text != current.assigned_pod
                && issue_has_active_run(
                    pool,
                    &current
                        .id
                        .parse::<Uuid>()
                        .map_err(|_| Denial::ServerError)?,
                )
                .await?
            {
                return Err(fail(ValidateError::PodReassignActiveRun));
            }
        }
    }

    // lxml round-trip (`:222-229`): every non-`None` value round-trips
    // (empty strings raise `ParserError` like any other failure) unless
    // the HTML came from markdown.
    if let Some(html) = parsed.description_html.take() {
        let mut html = html;
        if !from_markdown {
            match lxml_roundtrip(&html) {
                Some(roundtripped) => html = roundtripped,
                None => return Err(fail(ValidateError::InvalidHtml)),
            }
            // Sanitizer substitute (`:232-238`): only truthy HTML
            // validates, and the clean output always substitutes.
            if !html.is_empty() {
                match crate::space::sanitize::sanitize_html(&html) {
                    crate::space::sanitize::Sanitize::Clean(clean) => html = clean,
                    crate::space::sanitize::Sanitize::Invalid => {
                        return Err(fail(ValidateError::HtmlContentInvalid));
                    }
                }
            }
        }
        parsed.description_html = Some(html);
    }

    // Assignee / label allow-lists (`:246-259`): falsy inputs (absent or
    // `[]`) skip the filter and keep their value.
    if let Some(ids) = parsed.assignees.take() {
        let filtered = if ids.is_empty() {
            ids
        } else {
            filter_assignee_ids(pool, project_id, &ids).await?
        };
        parsed.assignees = Some(filtered);
    }
    if let Some(ids) = parsed.labels.take() {
        let filtered = if ids.is_empty() {
            ids
        } else {
            filter_label_ids(pool, project_id, &ids).await?
        };
        parsed.labels = Some(filtered);
    }

    // State / parent / estimate project checks (`:261-286`), each under
    // its field's manager scope (`StateManager` excludes triage;
    // `Issue.objects` keeps drafts, archived and triage rows).
    if let Some(Some(state_id)) = parsed.state {
        let ok: Option<bool> = sqlx::query_scalar(
            r#"SELECT EXISTS(SELECT 1 FROM "states" WHERE "project_id" = $1 AND "id" = $2 AND "deleted_at" IS NULL AND NOT ("group" = 'triage'))"#,
        )
        .bind(project_id)
        .bind(state_id)
        .fetch_optional(pool)
        .await
        .map_err(|error| db_error(error, "write-state-check"))?;
        if !ok.unwrap_or(false) {
            return Err(fail(ValidateError::StateWrongProject));
        }
    }
    if let Some(Some(parent_id)) = parsed.parent {
        let ok: Option<bool> = sqlx::query_scalar(
            r#"SELECT EXISTS(SELECT 1 FROM "issues" WHERE "workspace_id" = $1 AND "project_id" = $2 AND "id" = $3 AND "deleted_at" IS NULL)"#,
        )
        .bind(workspace_id)
        .bind(project_id)
        .bind(parent_id)
        .fetch_optional(pool)
        .await
        .map_err(|error| db_error(error, "write-parent-check"))?;
        if !ok.unwrap_or(false) {
            return Err(fail(ValidateError::ParentWrongProject));
        }
    }
    if let Some(Some(estimate_id)) = parsed.estimate_point {
        let ok: Option<bool> = sqlx::query_scalar(
            r#"SELECT EXISTS(SELECT 1 FROM "estimate_points" WHERE "workspace_id" = $1 AND "project_id" = $2 AND "id" = $3 AND "deleted_at" IS NULL)"#,
        )
        .bind(workspace_id)
        .bind(project_id)
        .bind(estimate_id)
        .fetch_optional(pool)
        .await
        .map_err(|error| db_error(error, "write-estimate-check"))?;
        if !ok.unwrap_or(false) {
            return Err(fail(ValidateError::EstimatePointWrongProject));
        }
    }

    Ok(())
}

/// Coerce the raw `created_at` override
/// (`views/issue.py:511-514` — `DateTimeField.get_prep_value`, probed
/// live): strings parse as datetimes (date-only falls back to midnight),
/// naive takes the request zone, aware keeps its instant; `null` stores
/// `NULL` (the not-null column then 400s); every other JSON type raises
/// `TypeError` into the 500.
fn coerce_created_at_override(
    value: &Value,
    tz: &Tz,
    tz_name: &str,
) -> Result<Option<DateTime<Utc>>, Denial> {
    if value.is_null() {
        return Ok(None);
    }
    let Value::String(text) = value else {
        return Err(Denial::ServerError);
    };
    match parse_drf_datetime(value, tz, tz_name) {
        Ok(instant) => Ok(Some(instant)),
        Err(_) => match parse_django_date(text) {
            Some(date) => {
                let naive = date.and_hms_opt(0, 0, 0).expect("midnight exists");
                match tz.from_local_datetime(&naive) {
                    chrono::LocalResult::Single(aware) => Ok(Some(aware.with_timezone(&Utc))),
                    chrono::LocalResult::Ambiguous(first, _) => Ok(Some(first.with_timezone(&Utc))),
                    chrono::LocalResult::None => Err(Denial::ServerError),
                }
            }
            None => Err(Denial::BadError("Please provide valid detail".to_owned())),
        },
    }
}

/// Coerce the raw `created_by` override (`UUIDField.to_python`, probed
/// live): strings parse flexibly (braces, `urn:`, unhyphenated),
/// non-negative ints (bools spell 0/1) take the `int=` form, `null`
/// stores `NULL` (the column is nullable); everything else is the
/// `ValidationError` 400.
fn coerce_created_by_override(value: &Value) -> Result<Option<Uuid>, Denial> {
    let invalid = || Denial::BadError("Please provide valid detail".to_owned());
    match value {
        Value::Null => Ok(None),
        Value::String(text) => parse_uuid_hex(text).ok_or_else(invalid).map(Some),
        Value::Bool(flag) => Ok(Some(Uuid::from_u128(u128::from(*flag as u8)))),
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                if int < 0 {
                    return Err(invalid());
                }
                return Ok(Some(Uuid::from_u128(int as u128)));
            }
            if let Some(uint) = number.as_u64() {
                return Ok(Some(Uuid::from_u128(u128::from(uint))));
            }
            if number.is_f64() {
                return Err(invalid());
            }
            number
                .to_string()
                .parse::<u128>()
                .map(Uuid::from_u128)
                .map(Some)
                .map_err(|_| invalid())
        }
        Value::Array(_) | Value::Object(_) => Err(invalid()),
    }
}

/// Map an override-coercion failure to its chain stop: the constant 400
/// detail arm, else the 500 arm (the only two the coercions produce).
fn override_stop(denial: Denial) -> domain_issues::ChainStop {
    match denial {
        Denial::BadError(_) => domain_issues::ChainStop::OverrideDetail,
        _ => domain_issues::ChainStop::OverrideFailed,
    }
}

/// The domain-write context for a v1 work-item request: the API actor
/// (with their stored time zone name) plus the gated tenant scope.
fn domain_write_ctx(pre: &Preamble, project_id: &Uuid) -> WriteCtx {
    WriteCtx::new(WriteActor {
        user_id: pre.actor.id,
        timezone: pre.actor.timezone.clone(),
    })
    .with_scope(WriteScope {
        workspace_id: pre.workspace_id,
        project_id: Some(*project_id),
    })
}

/// Map a domain-write failure onto the save arms: integrity violations
/// and dangling-row misses (a state validated by probe, gone at save —
/// the port's `UPDATE` FK decided a 400 there too) are the
/// invalid-payload 400, anything else 500s.
fn domain_save_denial(error: domain::DomainError) -> Denial {
    if error.is_integrity_violation() || error.is_not_found() {
        Denial::FieldErrors(PAYLOAD_NOT_VALID_BODY.to_owned())
    } else {
        Denial::ServerError
    }
}

// ---------------------------------------------------------------------------
// Create (`IssueListCreateAPIEndpoint.post`, `views/issue.py:470-540`)
// ---------------------------------------------------------------------------

/// Python `str()` for the external-dupe guards' raw scalars (`CharField`
/// stringifies filter values: numbers spell plainly, bools `True`/`False`).
/// Composites are unreachable (the serializer 400s them before the guard).
fn raw_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(python_number_str(number)),
        Value::Bool(true) => Some("True".to_owned()),
        Value::Bool(false) => Some("False".to_owned()),
        _ => None,
    }
}

/// Python truthiness for the guards' `request.data.get(...)` tests.
fn raw_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                return int != 0;
            }
            if let Some(uint) = number.as_u64() {
                return uint != 0;
            }
            number.as_f64().is_some_and(|float| float != 0.0)
        }
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
    }
}

/// The create/patch external-duplicate guard's `exists()` + `.first()`
/// pair (`views/issue.py:484-503,823-841`): same filter (project, slug,
/// source, id under the soft-deletion scope), the first in
/// `Meta.ordering = ("-created_at",)`.
async fn external_dup_first(
    pool: &PgPool,
    project_id: &Uuid,
    slug: &str,
    source: &str,
    external_id: &str,
) -> Result<Option<Uuid>, Denial> {
    let exists: Option<bool> = sqlx::query_scalar(
        r#"SELECT EXISTS(SELECT 1 FROM "issues" INNER JOIN "workspaces" ON ("issues"."workspace_id" = "workspaces"."id") WHERE "issues"."project_id" = $1 AND "workspaces"."slug" = $2 AND "issues"."external_source" = $3 AND "issues"."external_id" = $4 AND "issues"."deleted_at" IS NULL)"#,
    )
    .bind(project_id)
    .bind(slug)
    .bind(source)
    .bind(external_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| db_error(error, "write-dup-exists"))?;
    if !exists.unwrap_or(false) {
        return Ok(None);
    }
    sqlx::query_scalar(
        r#"SELECT "issues"."id" FROM "issues" INNER JOIN "workspaces" ON ("issues"."workspace_id" = "workspaces"."id") WHERE "issues"."project_id" = $1 AND "workspaces"."slug" = $2 AND "issues"."external_source" = $3 AND "issues"."external_id" = $4 AND "issues"."deleted_at" IS NULL ORDER BY "issues"."created_at" DESC LIMIT 1"#,
    )
    .bind(project_id)
    .bind(slug)
    .bind(source)
    .bind(external_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| db_error(error, "write-dup-first"))
    .map(|row: Option<Uuid>| row)
}

/// The 409 body (`views/issue.py:502-508,836-841`): insertion order
/// `error`, `id`.
fn external_dup_body(dup_id: &Uuid) -> String {
    format!(
        "{{\"error\":{},\"id\":{}}}",
        json_string(EXTERNAL_DUP_MESSAGE),
        json_string(&dup_id.to_string())
    )
}

async fn create_issue_inner(
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
        Some(&project_id),
        None,
        V1WorkItemsRoute::IssueList,
        "POST",
    )
    .await?;
    let tz = activate_timezone(pre.actor.timezone.as_deref())?;
    let tz_name = pre.actor.timezone.as_deref().unwrap_or("UTC");
    // `Project.objects.get` precedes normalization (`:471-474`), so a
    // missing project 404s before a bad markdown body 400s.
    let project = fetch_project_for_write(&pre.pool, &project_id).await?;
    let parsed_body = parse_write_body(headers, raw_body)?;
    let norm_input = if parsed_body.from_form {
        form_dict_for_normalize(&parsed_body.value)
    } else {
        parsed_body.value.clone()
    };
    let (data, from_markdown) = normalize_view_body(&norm_input, &markdown_to_html_port)?;
    let Value::Object(map) = &data else {
        return Err(Denial::FieldErrors(non_dict_body(&data)));
    };
    let mut parsed =
        parse_issue_write(&pre.pool, map, false, parsed_body.from_form, &tz, tz_name).await?;
    // The create-only model default that reaches `validate()`: a missing
    // `description_html` validates (and stores) as `"<p></p>"`.
    if parsed.description_html.is_none() {
        parsed.description_html = Some("<p></p>".to_owned());
    }
    run_write_validate(
        &pre.pool,
        &mut parsed,
        &project_id,
        &workspace_id,
        None,
        from_markdown,
    )
    .await?;

    // External-duplicate guard (`:483-503`): raw truthy values, the
    // conflicting row's id in the 409.
    if let (Some(raw_id), Some(raw_source)) = (
        parsed_body.value.get("external_id"),
        parsed_body.value.get("external_source"),
    ) {
        if raw_truthy(raw_id) && raw_truthy(raw_source) {
            if let (Some(external_id), Some(source)) = (raw_text(raw_id), raw_text(raw_source)) {
                if let Some(dup_id) =
                    external_dup_first(&pre.pool, &project_id, slug, &source, &external_id).await?
                {
                    return Err(Denial::Conflict(external_dup_body(&dup_id)));
                }
            }
        }
    }

    // `serializer.save()` → `create()` (`:291-366`) + `Issue.save`
    // (`db/models/issue.py:267-351`): the assembled row rides
    // `domain::issues::create` — the save hooks resolve the pod/state
    // defaults, `completed_at`, stripped text, sequence and sort order.
    // Only the serializer-level default type stays driver-side.
    let issue_type = match parsed.issue_type {
        Some(Some(id)) => Some(id),
        _ => fetch_default_issue_type(&pre.pool, &project_id).await?,
    };
    let new_id = Uuid::new_v4();
    let created_at = now_utc();
    let updated_at = now_utc();
    let fields = domain_issues::IssueFieldSet {
        id: new_id,
        created_at,
        updated_at,
        created_by_id: None,
        updated_by_id: None,
        deleted_at: parsed.deleted_at.flatten(),
        project_id,
        workspace_id: project.workspace_id,
        parent_id: parsed.parent.flatten(),
        state_id: parsed.state.flatten(),
        point: parsed.point.flatten().map(|point| point as i32),
        estimate_point_id: parsed.estimate_point.flatten(),
        name: parsed.name.clone().unwrap_or_default(),
        description_json: Value::Object(Map::new()),
        description_html: parsed.description_html.clone().unwrap_or_default(),
        description_stripped: None,
        description_binary: None,
        priority: parsed.priority.clone().unwrap_or_else(|| "none".to_owned()),
        complexity_score: parsed.complexity_score.unwrap_or(0) as i32,
        start_date: parsed.start_date.flatten(),
        target_date: parsed.target_date.flatten(),
        sequence_id: 0,
        sort_order: parsed.sort_order.unwrap_or(65535.0),
        completed_at: parsed.completed_at.flatten(),
        archived_at: parsed.archived_at.flatten(),
        is_draft: parsed.is_draft.unwrap_or(false),
        external_source: parsed.external_source.clone().flatten(),
        external_id: parsed.external_id.clone().flatten(),
        type_id: issue_type,
        git_work_branch: parsed.git_work_branch.clone().unwrap_or_default(),
        workpad: String::new(),
        created_via: parsed.created_via.clone().flatten(),
        assigned_pod_id: parsed.assigned_pod.flatten(),
        agent_executor: parsed.agent_executor.clone().flatten(),
    };
    // The raw-data overrides resolve pre-save (pure), but their failures
    // surface post-commit in the chain leg, in port order — a bad override
    // 400s/500s with the row, fire and m2m legs already persisted. The
    // absent-key defaults evaluate here rather than past the m2m legs;
    // wall-clock only.
    let override_created_at = match parsed_body.value.get("created_at") {
        None => Ok(Some(now_utc())),
        Some(value) => coerce_created_at_override(value, &tz, tz_name).map_err(override_stop),
    };
    let override_created_by = match parsed_body.value.get("created_by") {
        None => Ok(Some(pre.actor.id)),
        Some(value) => coerce_created_by_override(value).map_err(override_stop),
    };
    let requested_text = requested_data_text(&data);
    // `base_host` raises past the save (the row + `issue_activity` persist
    // on a misconfigured host): pure over settings, resolved here,
    // effective in the chain's model leg.
    let origin = app_origin(&state.settings().urls).ok();
    // Django's `post_save` fires inside `Issue.objects.create`, before the
    // assignee/label `bulk_create`s (and before the `created_at` /
    // `created_by` second save, whose transition no-ops on `prev == current`),
    // so the entry prompt sees empty m2m. A new row has no prior state, so
    // creating directly in a ticking state arms the clock and creates the
    // entry run (PIDASHCONV-1216).
    let orm = state.orm().ok_or(Denial::ServerError)?;
    let ctx = domain_write_ctx(&pre, &project_id);
    let actor_id = pre.actor.id;
    let default_assignee_id = project.default_assignee_id;
    let transition: Arc<dyn IssueTransitionHook> =
        Arc::new(AppTransitionHook::new(state, &pre.pool));
    let enqueue: Arc<dyn TaskEnqueueHook> = Arc::new(AppTaskEnqueueHook::new(&pre.pool));
    let outcome = domain::run(&ctx, orm, |txn| {
        let transition = transition.clone();
        let enqueue = enqueue.clone();
        let pool = pre.pool.clone();
        let ctx = ctx.clone();
        let slug = slug.to_owned();
        Box::pin(async move {
            txn.hooks_mut().register(transition);
            txn.hooks_mut().register(enqueue);
            domain_issues::create(
                &ctx,
                txn,
                &pool,
                domain_issues::CreateIssue {
                    fields,
                    assignees: parsed.assignees.clone().unwrap_or_default(),
                    default_assignee: default_assignee_id,
                    labels: parsed.labels.clone().unwrap_or_default(),
                    fire: domain_issues::FireSpec {
                        dispatch_immediate: true,
                        created_by: Some(actor_id),
                        moved_by_run: None,
                        now: Utc::now(),
                    },
                    origin: None,
                    activity_requested: Value::String(requested_text),
                    activity_notification: None,
                    activity_require_origin: false,
                    model_activity: Some(domain_issues::ModelActivitySpec {
                        model: new_id,
                        requested: data,
                        current: Value::Null,
                        actor: actor_id,
                        slug,
                        origin,
                    }),
                    created_override: Some(domain_issues::CreatedOverride {
                        created_at: override_created_at,
                        created_by: override_created_by,
                    }),
                },
            )
            .await
        })
    })
    .await
    .map_err(domain_save_denial)?;
    match outcome.chain.stopped() {
        None => {}
        Some(domain_issues::ChainStop::OverrideConflict) => {
            return Err(Denial::FieldErrors(PAYLOAD_NOT_VALID_BODY.to_owned()));
        }
        Some(domain_issues::ChainStop::OverrideDetail) => {
            return Err(Denial::BadError("Please provide valid detail".to_owned()));
        }
        Some(_) => return Err(Denial::ServerError),
    }
    // The 201 renders the PRE-override in-memory instance: refetch the
    // row, then restore the insert-time `created_at` / `created_by`.
    let mut decoded = fetch_issue_for_write(&pre.pool, slug, &project_id, &new_id, &tz).await?;
    decoded.created_at = render_dt(&created_at, &tz);
    decoded.created_at_dt = created_at;
    decoded.created_by = Some(pre.actor.id.to_string());
    let web_base = shape::web_base_url(
        state.settings().urls.web_url.as_deref(),
        state.settings().urls.app_base_url.as_deref(),
    );
    let rendered = render_write_response(&pre.pool, &decoded, &tz, web_base.as_deref()).await?;
    Ok(json_created(
        serde_json::to_string(&rendered).map_err(|_| Denial::ServerError)?,
    ))
}

// ---------------------------------------------------------------------------
// Patch + delete (`IssueDetailAPIEndpoint`, `views/issue.py:790-909`)
// ---------------------------------------------------------------------------

/// The agent-run columns `resolve_moved_by_run` reads (the
/// `handlers_actions` fetch shape: `AgentRun` + the `runner` join for
/// ownership).
const RUN_FACTS_COLS: &str = r#"r."id", r."created_by_id", r."owner_id", r."runner_id", ru."owner_id" AS "runner_owner_id", r."work_item_id", r."status""#;

fn map_run_facts(row: &sqlx::postgres::PgRow) -> Result<RunFacts, Denial> {
    let status: String = row
        .try_get("status")
        .map_err(|error| db_error(error, "map-run"))?;
    Ok(RunFacts {
        id: row
            .try_get("id")
            .map_err(|error| db_error(error, "map-run"))?,
        created_by_id: row
            .try_get("created_by_id")
            .map_err(|error| db_error(error, "map-run"))?,
        owner_id: row
            .try_get("owner_id")
            .map_err(|error| db_error(error, "map-run"))?,
        runner_id: row
            .try_get("runner_id")
            .map_err(|error| db_error(error, "map-run"))?,
        runner_owner_id: row
            .try_get("runner_owner_id")
            .map_err(|error| db_error(error, "map-run"))?,
        work_item_id: row
            .try_get("work_item_id")
            .map_err(|error| db_error(error, "map-run"))?,
        status: AgentRunStatus::from_value(&status).unwrap_or(AgentRunStatus::Cancelled),
    })
}

/// `AgentRun.objects.select_related("runner").filter(pk=run_id).first()`
/// — the header path's row (`views/issue.py:1113`).
async fn fetch_run_by_id(pool: &PgPool, run_id: &Uuid) -> Result<Option<RunFacts>, Denial> {
    let sql = format!(
        "SELECT {RUN_FACTS_COLS} FROM \"agent_run\" r LEFT JOIN \"runner\" ru ON ru.\"id\" = r.\"runner_id\" WHERE r.\"id\" = $1 LIMIT 1"
    );
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(&sql)
        .bind(run_id)
        .fetch_optional(pool)
        .await
        .map_err(|error| db_error(error, "fetch-run"))?;
    row.map(|row| map_run_facts(&row)).transpose()
}

/// `AgentRun.objects.select_related("runner").filter(work_item_id=pk)
/// .order_by("-created_at")[:5]` — the no-header inference rows (`:1127`).
async fn fetch_newest_runs_on_issue(
    pool: &PgPool,
    issue_id: &Uuid,
) -> Result<Vec<RunFacts>, Denial> {
    let sql = format!(
        "SELECT {RUN_FACTS_COLS} FROM \"agent_run\" r LEFT JOIN \"runner\" ru ON ru.\"id\" = r.\"runner_id\" WHERE r.\"work_item_id\" = $1 ORDER BY r.\"created_at\" DESC LIMIT 5"
    );
    let rows: Vec<sqlx::postgres::PgRow> = sqlx::query(&sql)
        .bind(issue_id)
        .fetch_all(pool)
        .await
        .map_err(|error| db_error(error, "fetch-runs"))?;
    rows.iter().map(map_run_facts).collect()
}

/// The patch guard's nullable-`external_source` variant
/// (`request.data.get("external_source", issue.external_source)` —
/// `None` filters `IS NULL`).
async fn external_dup_first_nullable(
    pool: &PgPool,
    project_id: &Uuid,
    slug: &str,
    source: Option<&str>,
    external_id: &str,
) -> Result<Option<Uuid>, Denial> {
    let exists: Option<bool> = sqlx::query_scalar(
        r#"SELECT EXISTS(SELECT 1 FROM "issues" INNER JOIN "workspaces" ON ("issues"."workspace_id" = "workspaces"."id") WHERE "issues"."project_id" = $1 AND "workspaces"."slug" = $2 AND "issues"."external_source" IS NOT DISTINCT FROM $3 AND "issues"."external_id" = $4 AND "issues"."deleted_at" IS NULL)"#,
    )
    .bind(project_id)
    .bind(slug)
    .bind(source)
    .bind(external_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| db_error(error, "write-dup-exists"))?;
    if !exists.unwrap_or(false) {
        return Ok(None);
    }
    sqlx::query_scalar(
        r#"SELECT "issues"."id" FROM "issues" INNER JOIN "workspaces" ON ("issues"."workspace_id" = "workspaces"."id") WHERE "issues"."project_id" = $1 AND "workspaces"."slug" = $2 AND "issues"."external_source" IS NOT DISTINCT FROM $3 AND "issues"."external_id" = $4 AND "issues"."deleted_at" IS NULL ORDER BY "issues"."created_at" DESC LIMIT 1"#,
    )
    .bind(project_id)
    .bind(slug)
    .bind(source)
    .bind(external_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| db_error(error, "write-dup-first"))
    .map(|row: Option<Uuid>| row)
}

/// Resolve the post-save `(state_id, completed_at)` pair for the update
/// and delete saves (`Issue.save`'s state branch, `db/models/issue.py:288-
/// 310` — no `_state.adding` check, so it runs on every save): a provided
/// state takes the completed arm; a null state (provided or carried)
/// resolves the default and leaves `completed_at` untouched; an absent
/// state recomputes the arm from the current row. Returns the
/// `SET`-or-skip pair for each column.
async fn resolve_save_state(
    pool: &PgPool,
    project_id: &Uuid,
    current: &DecodedIssue,
    parsed_state: Option<Option<Uuid>>,
) -> Result<(Option<Option<Uuid>>, Option<Option<DateTime<Utc>>>), Denial> {
    let current_state: Option<Uuid> = current.state.as_deref().and_then(|id| id.parse().ok());
    match parsed_state {
        Some(Some(id)) => {
            // A vanished row (probe passed, row gone) reads as
            // non-completed and lets the `UPDATE` FK decide the 400.
            let group = fetch_state_group(pool, &id).await?.unwrap_or_default();
            let completed_at = if group == "completed" {
                Some(now_utc())
            } else {
                None
            };
            Ok((Some(Some(id)), Some(completed_at)))
        }
        Some(None) => {
            let resolved = fetch_default_state(pool, project_id).await?;
            Ok((Some(resolved.map(|(id, _)| id)), None))
        }
        None => match current_state {
            Some(_) => {
                let completed_at = if current.state_group.as_deref() == Some("completed") {
                    Some(now_utc())
                } else {
                    None
                };
                Ok((None, Some(completed_at)))
            }
            None => {
                let resolved = fetch_default_state(pool, project_id).await?;
                Ok((Some(resolved.map(|(id, _)| id)), None))
            }
        },
    }
}

async fn patch_issue_inner(
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
        Some(&project_id),
        None,
        V1WorkItemsRoute::IssueDetail,
        "PATCH",
    )
    .await?;
    let tz = activate_timezone(pre.actor.timezone.as_deref())?;
    let tz_name = pre.actor.timezone.as_deref().unwrap_or("UTC");
    // The issue fetch precedes the project fetch (`:792-793`).
    let current = fetch_issue_for_write(&pre.pool, slug, &project_id, pk, &tz).await?;
    fetch_project_for_write(&pre.pool, &project_id).await?;

    // The run header (`:794-807`): a blank/missing header infers the
    // caller's active run, a malformed id 400s. The resolved run id
    // stamps `MOVED_BY_RUN_ATTR` for the post-save orchestration
    // signal (PIDASHCONV-804); it has no wire effect on its own.
    let header = headers
        .get("X-Pi-Dash-Run-Id")
        .map(|value| value.to_str().unwrap_or("\u{fffd}").to_owned());
    let header_ref = header.as_deref();
    let run_by_id = match header_ref.map(str::trim).filter(|h| !h.is_empty()) {
        Some(raw) => match raw.parse::<Uuid>() {
            Ok(id) => fetch_run_by_id(&pre.pool, &id).await?,
            Err(_) => None,
        },
        None => None,
    };
    let newest_runs = if header_ref.map(str::trim).is_some_and(|h| !h.is_empty()) {
        Vec::new()
    } else {
        fetch_newest_runs_on_issue(&pre.pool, pk).await?
    };
    let (moved_by_run, header_error) = resolve_moved_by_run(
        header_ref,
        Some(pre.actor.id),
        *pk,
        run_by_id.as_ref(),
        &newest_runs,
    );
    if let Some(error) = header_error {
        return Err(Denial::BadError(error.message().to_owned()));
    }

    // `current_instance` renders before normalization (`:808-811`), with
    // the same settings-derived web base as the response (`get_url` reads
    // deployment config, never the request).
    let web_base = shape::web_base_url(
        state.settings().urls.web_url.as_deref(),
        state.settings().urls.app_base_url.as_deref(),
    );
    let current_rendered =
        render_write_response(&pre.pool, &current, &tz, web_base.as_deref()).await?;
    let current_text =
        pidash_jobs::tasks_webhooks::activity_dispatch::django_dumps(&current_rendered);

    let parsed_body = parse_write_body(headers, raw_body)?;
    let norm_input = if parsed_body.from_form {
        form_dict_for_normalize(&parsed_body.value)
    } else {
        parsed_body.value.clone()
    };
    let (data, from_markdown) = normalize_view_body(&norm_input, &markdown_to_html_port)?;
    let Value::Object(map) = &data else {
        return Err(Denial::FieldErrors(non_dict_body(&data)));
    };
    let mut parsed =
        parse_issue_write(&pre.pool, map, true, parsed_body.from_form, &tz, tz_name).await?;
    run_write_validate(
        &pre.pool,
        &mut parsed,
        &project_id,
        &workspace_id,
        Some(&current),
        from_markdown,
    )
    .await?;

    // External-duplicate guard (`:822-841`): only when the raw id is
    // truthy AND changed (`stored != str(incoming)`), the source
    // defaulting to the stored one — and the 409 carries the CURRENT
    // issue's id, not the conflicting row's.
    if let Some(raw_id) = parsed_body.value.get("external_id") {
        if raw_truthy(raw_id) {
            let incoming = raw_text(raw_id);
            if current.external_id.as_deref() != incoming.as_deref() {
                let source: Option<String> = match parsed_body.value.get("external_source") {
                    Some(value) => raw_text(value),
                    None => current.external_source.clone(),
                };
                if let Some(external_id) = incoming {
                    if external_dup_first_nullable(
                        &pre.pool,
                        &project_id,
                        slug,
                        source.as_deref(),
                        &external_id,
                    )
                    .await?
                    .is_some()
                    {
                        return Err(Denial::Conflict(external_dup_body(pk)));
                    }
                }
            }
        }
    }

    // `serializer.save()` → `update()` (`:368-416`): the m2m legs run ahead
    // of the row write inside `domain::issues::partial_update`, then the
    // full-column `save()` (`updated_at`/`updated_by` and the
    // stripped/completed recomputes always land, even for `{}`), the
    // completion pair when the state moved, the always-immediate fire, and
    // the two activities (no version task on this view).
    //
    // The save-time `sequence_id` int4 check resolves here (pure) but
    // surfaces past the m2m legs in the domain write — a bad sequence 500s
    // with the legs already persisted, as today.
    let sequence = if parsed.sequence_overflow {
        Some(Err(()))
    } else {
        parsed
            .sequence_id
            .map(|number| i32::try_from(number).map_err(|_| ()))
    };
    // The pre-save snapshot (`capture_prior_state`, `signals.py:61-71`):
    // the stored `state_id` re-read just before the save, past every
    // guard, as `pre_save` runs inside `serializer.save()`. A storage
    // failure 500s, as in Python where only the handler call is in
    // `try`.
    let prev_state_id = crate::issue_transition::capture_prior_state_pool(&pre.pool, Some(*pk))
        .await
        .map_err(|_| Denial::ServerError)?;
    // The full-save base row, re-read beside the snapshot: the `DecodedIssue`
    // above is render-lossy (tz-rendered datetimes, no `description_json` /
    // `workpad`), and the save writes every column. A miss 404s — the port
    // would run the m2m legs first and 404 on the final refetch instead, a
    // mid-flight-delete race either way.
    let orm = state.orm().ok_or(Denial::ServerError)?;
    let Some(base) = issues::Entity::find_by_id(*pk)
        .one(orm)
        .await
        .map_err(|error| db_error(error, "write-reread"))?
    else {
        return Err(Denial::NotFound(RESOURCE_NOT_FOUND_BODY.to_owned()));
    };
    let mut fields = domain_issues::IssueFieldSet::from(&base);
    let audit = (base.created_by_id, base.updated_by_id);
    // The validated overlay: present keys move (nullable columns take the
    // OUTER option — explicit null stores `NULL`, absent keeps the row),
    // `state`/`completed_at` assemble for the hook arms (a provided state
    // recompletes, explicit-null/absent resolves the default and keeps the
    // validated-or-current `completed_at`).
    fields.updated_at = now_utc();
    if let Some(value) = parsed.description_html.clone() {
        fields.description_html = value;
    }
    if let Some(value) = parsed.deleted_at {
        fields.deleted_at = value;
    }
    if let Some(value) = parsed.point {
        fields.point = value.map(|point| point as i32);
    }
    if let Some(value) = parsed.name.clone() {
        fields.name = value;
    }
    if let Some(value) = parsed.priority.clone() {
        fields.priority = value;
    }
    if let Some(value) = parsed.complexity_score {
        fields.complexity_score = value as i32;
    }
    if let Some(value) = parsed.start_date {
        fields.start_date = value;
    }
    if let Some(value) = parsed.target_date {
        fields.target_date = value;
    }
    if let Some(value) = parsed.sort_order {
        fields.sort_order = value;
    }
    let row_completed = fields.completed_at;
    fields.completed_at = parsed.completed_at.flatten().or(row_completed);
    if let Some(value) = parsed.archived_at {
        fields.archived_at = value;
    }
    if let Some(value) = parsed.is_draft {
        fields.is_draft = value;
    }
    if let Some(value) = parsed.external_source.clone() {
        fields.external_source = value;
    }
    if let Some(value) = parsed.external_id.clone() {
        fields.external_id = value;
    }
    if let Some(value) = parsed.git_work_branch.clone() {
        fields.git_work_branch = value;
    }
    if let Some(value) = parsed.created_via.clone() {
        fields.created_via = value;
    }
    if let Some(value) = parsed.agent_executor.clone() {
        fields.agent_executor = value;
    }
    if let Some(value) = parsed.created_by {
        fields.created_by_id = value;
    }
    if let Some(value) = parsed.parent {
        fields.parent_id = value;
    }
    if let Some(value) = parsed.estimate_point {
        fields.estimate_point_id = value;
    }
    if let Some(value) = parsed.issue_type {
        fields.type_id = value;
    }
    if let Some(value) = parsed.assigned_pod {
        fields.assigned_pod_id = value;
    }
    fields.state_id = match parsed.state {
        Some(value) => value,
        None => base.state_id,
    };
    // `base_host` raises past the fire (the save + `issue_activity` persist
    // on a misconfigured host): pure over settings, resolved here,
    // effective in the chain's model leg.
    let origin = app_origin(&state.settings().urls).ok();
    let requested_text = requested_data_text(&data);
    let ctx = domain_write_ctx(&pre, &project_id);
    let actor_id = pre.actor.id;
    let transition: Arc<dyn IssueTransitionHook> =
        Arc::new(AppTransitionHook::new(state, &pre.pool));
    let enqueue: Arc<dyn TaskEnqueueHook> = Arc::new(AppTaskEnqueueHook::new(&pre.pool));
    let outcome = domain::run(&ctx, orm, |txn| {
        let transition = transition.clone();
        let enqueue = enqueue.clone();
        let pool = pre.pool.clone();
        let ctx = ctx.clone();
        let slug = slug.to_owned();
        Box::pin(async move {
            txn.hooks_mut().register(transition);
            txn.hooks_mut().register(enqueue);
            domain_issues::partial_update(
                &ctx,
                txn,
                &pool,
                domain_issues::PartialIssue {
                    m2m_audit: audit,
                    sequence,
                    fields,
                    assignees: parsed.assignees.clone(),
                    labels: parsed.labels.clone(),
                    prev_state: prev_state_id,
                    fire: domain_issues::FireSpec {
                        dispatch_immediate: true,
                        created_by: Some(actor_id),
                        moved_by_run,
                        now: Utc::now(),
                    },
                    activity: Some(domain_issues::UpdateActivity {
                        origin,
                        requested: requested_text,
                        current_instance: current_text,
                        raw_body: data,
                        slug,
                        notification: None,
                        require_origin: false,
                        emit_version: false,
                    }),
                },
            )
            .await
        })
    })
    .await
    .map_err(domain_save_denial)?;
    if outcome.chain.stopped().is_some() {
        return Err(Denial::ServerError);
    }
    let decoded = fetch_issue_for_write(&pre.pool, slug, &project_id, pk, &tz).await?;
    let rendered = render_write_response(&pre.pool, &decoded, &tz, web_base.as_deref()).await?;
    Ok(json_response(
        StatusCode::OK,
        serde_json::to_string(&rendered).map_err(|_| Denial::ServerError)?,
    ))
}

async fn delete_issue_inner(
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
        Some(&project_id),
        None,
        V1WorkItemsRoute::IssueDetail,
        "DELETE",
    )
    .await?;
    let tz = activate_timezone(pre.actor.timezone.as_deref())?;
    let current = fetch_issue_for_write(&pre.pool, slug, &project_id, pk, &tz).await?;

    // Creator-or-admin (`:887-898`): the creator short-circuits, anyone
    // else needs an active `role=20` membership.
    let is_creator = current
        .created_by
        .as_deref()
        .and_then(|id| id.parse::<Uuid>().ok())
        .is_some_and(|id| id == pre.actor.id);
    if !is_creator {
        let is_admin: Option<bool> = sqlx::query_scalar(
            r#"SELECT EXISTS(SELECT 1 FROM "project_members" WHERE "workspace_id" = $1 AND "member_id" = $2 AND "role" = 20 AND "project_id" = $3 AND "is_active" AND "deleted_at" IS NULL)"#,
        )
        .bind(workspace_id)
        .bind(pre.actor.id)
        .bind(project_id)
        .fetch_optional(&pre.pool)
        .await
        .map_err(|error| db_error(error, "write-delete-admin"))?;
        if !is_admin.unwrap_or(false) {
            return Err(Denial::ForbiddenBody(format!(
                "{{\"error\":{}}}",
                json_string(DELETE_DENIAL_MESSAGE)
            )));
        }
    }

    let web_base = shape::web_base_url(
        state.settings().urls.web_url.as_deref(),
        state.settings().urls.app_base_url.as_deref(),
    );
    let current_rendered =
        render_write_response(&pre.pool, &current, &tz, web_base.as_deref()).await?;
    let current_text =
        pidash_jobs::tasks_webhooks::activity_dispatch::django_dumps(&current_rendered);

    // `issue.delete()` → `SoftDeleteModel.delete` (`db/mixins.py:72-78`):
    // `deleted_at` now plus a FULL `save()` (the state branch resolves or
    // recompletes, `updated_by`/`updated_at` stamp), then the sweep task.
    let prev_state_id = crate::issue_transition::capture_prior_state_pool(&pre.pool, Some(*pk))
        .await
        .map_err(|_| Denial::ServerError)?;
    let (set_state, set_completed) =
        resolve_save_state(&pre.pool, &project_id, &current, None).await?;
    let current_state_id: Option<Uuid> = match set_state {
        Some(inner) => inner,
        None => current.state.as_deref().and_then(|id| id.parse().ok()),
    };
    let stripped_new = if current.description_html.is_empty() {
        None
    } else {
        Some(crate::space::sanitize::strip_tags(
            &current.description_html,
        ))
    };
    let mut qb: sqlx::QueryBuilder<sqlx::Postgres> = sqlx::QueryBuilder::new("");
    qb.push("UPDATE \"issues\" SET ");
    let mut sep = qb.separated(", ");
    sep.push("\"deleted_at\" = ");
    sep.push_bind_unseparated(now_utc());
    sep.push("\"updated_at\" = ");
    sep.push_bind_unseparated(now_utc());
    sep.push("\"updated_by_id\" = ");
    sep.push_bind_unseparated(pre.actor.id);
    sep.push("\"description_stripped\" = ");
    sep.push_bind_unseparated(stripped_new);
    if let Some(completed) = set_completed {
        sep.push("\"completed_at\" = ");
        sep.push_bind_unseparated(completed);
    }
    if let Some(state) = set_state {
        sep.push("\"state_id\" = ");
        sep.push_bind_unseparated(state);
    }
    qb.push(" WHERE \"id\" = ");
    qb.push_bind(pk);
    qb.build()
        .execute(&pre.pool)
        .await
        .map(|_| ())
        .map_err(|error| save_error(&error, "write-delete"))?;

    // `github_signals.trigger_completion_comment`
    // (`bgtasks/github_signals.py:44-75`): the delete's full `save()` runs
    // the NULL-state default leg, so deleting a mirrored NULL-state issue
    // whose default state is completed-group enqueues the provider's
    // `post_completion_comment` — before the orchestration receiver, as
    // in Django. Best-effort: the delete stands.
    if prev_state_id != current_state_id {
        crate::issue_transition::enqueue_completion_comment(
            &pre.pool,
            pk,
            prev_state_id.as_ref(),
            current_state_id.as_ref(),
        )
        .await;
    }
    // `issue.delete()` saves the row, so the orchestration receiver fires
    // (state unchanged on a plain delete → no transition; a state leg that
    // moves it fires). PIDASHCONV-1216.
    fire_after_save(
        state,
        &pre.pool,
        *pk,
        pre.actor.id,
        prev_state_id,
        current_state_id,
        None,
        Some(pre.actor.id),
        Utc::now(),
    )
    .await?;

    let (sweep_args, sweep_kwargs) = soft_delete_sweep("issue", &pk.to_string());
    enqueue_best_effort(&pre.pool, SOFT_DELETE_TASK, sweep_args, sweep_kwargs).await;
    let mut requested = Map::with_capacity(1);
    requested.insert("issue_id".to_owned(), Value::String(pk.to_string()));
    let requested_text =
        pidash_jobs::tasks_webhooks::activity_dispatch::django_dumps(&Value::Object(requested));
    let kwargs = work_tasks::issue_activity_kwargs(
        work_tasks::ACTIVITY_ISSUE_DELETED,
        Some(&requested_text),
        &pre.actor.id.to_string(),
        &pk.to_string(),
        &project_id.to_string(),
        Some(&current_text),
        Utc::now().timestamp(),
    );
    enqueue_best_effort(&pre.pool, work_tasks::ISSUE_ACTIVITY_TASK, vec![], kwargs).await;

    Ok(Response::builder()
        .status(StatusCode::NO_CONTENT)
        .body(axum::body::Body::empty())
        .expect("empty response"))
}

// ---------------------------------------------------------------------------
// Write handlers
// ---------------------------------------------------------------------------

/// `POST .../projects/<project_id>/work-items/` and the deprecated
/// `issues/` twin (`urls/work_item.py:49-53,118-122`).
pub async fn post_issue_list(
    State(state): State<AppState>,
    Path((slug, project_id)): Path<(String, String)>,
    headers: HeaderMap,
    body: axum::body::Body,
) -> Response {
    let raw = match read_body(body).await {
        Ok(raw) => raw,
        Err(denial) => return denial.into_response(),
    };
    match create_issue_inner(&state, &headers, &slug, &project_id, &raw).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

/// `PATCH .../projects/<project_id>/work-items/<pk>/` and the deprecated
/// twin (`urls/work_item.py:54-58,123-127`).
pub async fn patch_issue_detail(
    State(state): State<AppState>,
    OriginalUri(original): OriginalUri,
    Path((slug, project_id, pk)): Path<(String, String, String)>,
    headers: HeaderMap,
    body: axum::body::Body,
) -> Response {
    let raw = match read_body(body).await {
        Ok(raw) => raw,
        Err(denial) => return denial.into_response(),
    };
    if !crate::runner_runs::is_uuid_path_segment(&pk) {
        return proxy_request(&state, "PATCH", original.to_string(), &headers, raw).await;
    }
    let pk = pk.parse::<Uuid>().expect("checked segment");
    match patch_issue_inner(&state, &headers, &slug, &project_id, &pk, &raw).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

/// `DELETE .../projects/<project_id>/work-items/<pk>/` and the deprecated
/// twin (`urls/work_item.py:54-58,123-127`).
pub async fn delete_issue_detail(
    State(state): State<AppState>,
    OriginalUri(original): OriginalUri,
    Path((slug, project_id, pk)): Path<(String, String, String)>,
    headers: HeaderMap,
) -> Response {
    if !crate::runner_runs::is_uuid_path_segment(&pk) {
        return proxy_request(&state, "DELETE", original.to_string(), &headers, Vec::new()).await;
    }
    let pk = pk.parse::<Uuid>().expect("checked segment");
    match delete_issue_inner(&state, &headers, &slug, &project_id, &pk).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

// ---------------------------------------------------------------------------
// Orchestration signals (explicit): `capture_prior_state` + `fire_state_transition`
// ---------------------------------------------------------------------------

/// Build the patch path's post-save fire (`signals.py:74-107`): the
/// snapshot pair around the save, the resolved header run, and the
/// receiver defaults this view never overrides. Pure so the fixture
/// pins the call exactly (`patch_signals.golden.json`).
fn patch_fire_request(
    issue_id: Uuid,
    prev_state_id: Option<Uuid>,
    current_state_id: Option<Uuid>,
    moved_by_run: Option<Uuid>,
    created_by: Option<Uuid>,
    now: DateTime<Utc>,
) -> FireRequest {
    FireRequest {
        issue_id,
        prev_state_id,
        current_state_id,
        // This view never stamps `_DISPATCH_IMMEDIATE_ATTR`, so the
        // receiver default (`True`) always applies.
        dispatch_immediate: true,
        moved_by_run,
        now,
        // Caller-drawn jitter (L5 quirk-5): the request path draws
        // none, like the D-26 and move fires.
        jitter_secs: 0.0,
        created_by,
    }
}

/// The post-save fire: a non-transition answers `NoTransition` without
/// I/O (the pure early return below, before any dispatch input is read — a
/// plain PATCH pays zero extra queries); a failed handler is logged with the
/// verbatim line (the counter already bumped inside) and the save stands;
/// only a *lookup* storage failure propagates to the 500 — exactly the `try`
/// placement in `fire_state_transition`. Drives the one live seam
/// (PIDASHCONV-1216).
#[allow(clippy::too_many_arguments)]
async fn fire_after_save(
    state: &AppState,
    pool: &PgPool,
    issue_id: Uuid,
    actor_id: Uuid,
    prev_state_id: Option<Uuid>,
    current_state_id: Option<Uuid>,
    moved_by_run: Option<Uuid>,
    created_by: Option<Uuid>,
    now: DateTime<Utc>,
) -> Result<(), Denial> {
    if prev_state_id == current_state_id {
        return Ok(());
    }
    let request = patch_fire_request(
        issue_id,
        prev_state_id,
        current_state_id,
        moved_by_run,
        created_by,
        now,
    );
    let outcome = crate::issue_transition::fire_issue_transition(state, pool, actor_id, request)
        .await
        .map_err(|_| Denial::ServerError)?;
    if let FireOutcome::Failed { log_line, .. } = outcome {
        tracing::error!("{log_line}");
    }
    Ok(())
}

#[cfg(test)]
mod signal_tests {
    use super::*;

    static PATCH_SIGNALS_FIXTURE: &str =
        include_str!("../../../../fixtures/v1_work_items/handlers/patch_signals.golden.json");

    fn fixture_cases() -> Value {
        let fixture: Value = serde_json::from_str(PATCH_SIGNALS_FIXTURE).expect("fixture parses");
        fixture["cases"].clone()
    }

    fn opt_uuid(value: &Value) -> Option<Uuid> {
        value
            .as_str()
            .map(|text| text.parse().expect("uuid parses"))
    }

    fn uid(n: u128) -> Uuid {
        Uuid::from_u128(n)
    }

    use crate::issue_transition::test_support::{LazyStateSeam, UnreachedPreflight};
    use pidash_services::orchestration::entries::fire_state_transition;

    /// The fixture pins the call: every case's inputs build exactly the
    /// pinned `FireRequest` — `dispatch_immediate` always true (this
    /// view never stamps the opt-out), the resolved header run passed
    /// through as `moved_by_run`, zero caller jitter.
    #[test]
    fn patch_fire_request_pins_fixture() {
        let cases = fixture_cases();
        let names = [
            "human_patch_no_transition",
            "agent_patch_no_transition",
            "human_patch_transition",
            "agent_patch_transition",
        ];
        assert_eq!(
            cases.as_object().expect("cases is a map").len(),
            names.len(),
            "fixture covers exactly the pinned cases"
        );
        for name in names {
            let case = &cases[name];
            let now = DateTime::parse_from_rfc3339(case["now"].as_str().expect("now"))
                .expect("now parses")
                .to_utc();
            let request = patch_fire_request(
                case["issue_id"]
                    .as_str()
                    .expect("issue_id")
                    .parse()
                    .expect("uuid"),
                opt_uuid(&case["prev_state_id"]),
                opt_uuid(&case["current_state_id"]),
                opt_uuid(&case["moved_by_run"]),
                opt_uuid(&case["created_by"]),
                now,
            );
            let expected = &case["expected"];
            assert_eq!(
                request.issue_id,
                expected["issue_id"]
                    .as_str()
                    .expect("expected issue_id")
                    .parse::<Uuid>()
                    .expect("uuid"),
                "{name}"
            );
            assert_eq!(
                request.prev_state_id,
                opt_uuid(&expected["prev_state_id"]),
                "{name}"
            );
            assert_eq!(
                request.current_state_id,
                opt_uuid(&expected["current_state_id"]),
                "{name}"
            );
            assert_eq!(
                request.dispatch_immediate,
                expected["dispatch_immediate"].as_bool().expect("bool"),
                "{name}"
            );
            assert_eq!(
                request.moved_by_run,
                opt_uuid(&expected["moved_by_run"]),
                "{name}"
            );
            assert_eq!(
                request.now,
                DateTime::parse_from_rfc3339(expected["now"].as_str().expect("now"))
                    .expect("now parses")
                    .to_utc(),
                "{name}"
            );
            assert_eq!(
                request.jitter_secs,
                expected["jitter_secs"].as_f64().expect("f64"),
                "{name}"
            );
            assert_eq!(
                request.created_by,
                opt_uuid(&expected["created_by"]),
                "{name}"
            );
        }
    }

    /// The `agent_patch_no_transition` row: the save fires the receiver
    /// and it no-ops (equal states) with zero seam I/O — the lazy pool
    /// is never touched, and the short-circuit precedes any run use.
    #[tokio::test]
    async fn fire_no_transition_needs_no_io() {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgresql://127.0.0.1:1/core_probe")
            .expect("lazy pool");
        let mut seam = LazyStateSeam { pool: &pool };
        let mut preflight = UnreachedPreflight;
        let now = chrono::Utc::now();
        let request = patch_fire_request(
            uid(9),
            Some(uid(3)),
            Some(uid(3)),
            Some(uid(7)),
            Some(uid(1)),
            now,
        );
        let outcome = fire_state_transition(&mut seam, &mut preflight, &request)
            .await
            .expect("no-transition never fails");
        assert_eq!(outcome, FireOutcome::NoTransition);
    }

    /// Lookup rule: a storage failure outside the handler `try`
    /// propagates (here: the unreachable lazy pool, short acquire
    /// budget so the test fails fast).
    #[tokio::test]
    async fn fire_lookup_failure_propagates() {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .acquire_timeout(std::time::Duration::from_secs(2))
            .connect_lazy("postgresql://127.0.0.1:1/core_probe")
            .expect("lazy pool");
        let mut seam = LazyStateSeam { pool: &pool };
        let mut preflight = UnreachedPreflight;
        let now = chrono::Utc::now();
        let request = patch_fire_request(
            uid(9),
            Some(uid(3)),
            Some(uid(4)),
            Some(uid(7)),
            Some(uid(1)),
            now,
        );
        let outcome = fire_state_transition(&mut seam, &mut preflight, &request).await;
        assert!(outcome.is_err());
    }
}
