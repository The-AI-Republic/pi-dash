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
use chrono::{DateTime, Utc};
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
use pidash_services::operations;
use pidash_services::operations::issues::parse_python_int;
use pidash_services::read::{
    self, DecodedIssue, RenderRequest, WriteProject, ISSUE_EXTRA_COLS, ISSUE_SELECT_COLS,
};
use pidash_services::v1_work_items::queries_core as core_queries;
use pidash_services::v1_work_items::shape_issue as shape;
use pidash_services::v1_work_items::{filter_fields, FieldSpec};

use crate::domain_hooks::AppTaskEnqueueHook;
use crate::issue_transition::AppTransitionHook;
use crate::state::AppState;

use super::perms::{decide, gate_for, resolve_moved_by_run, RunFacts, V1WorkItemsRoute};
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
/// Delete 403 (`views/issue.py:894-897`).
pub const DELETE_DENIAL_MESSAGE: &str = "Only admin or creator can delete the work item";
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

/// Map an operation failure onto the `Denial` arms the moved handlers
/// used (OVL-14, PIDASHCONV-1649). The issue operations never return
/// the move-only variants (`ProjectNotFound`, `Rejection`), so those
/// arms 500 instead of rendering a body this module cannot spell.
impl From<operations::Error> for Denial {
    fn from(error: operations::Error) -> Self {
        match error {
            operations::Error::BadError(message) => Denial::BadError(message),
            operations::Error::FieldErrors(body) => Denial::FieldErrors(body),
            operations::Error::NotFound => Denial::NotFound(RESOURCE_NOT_FOUND_BODY.to_owned()),
            operations::Error::NotFoundBody(body) => Denial::NotFound(body),
            operations::Error::Conflict(body) => Denial::Conflict(body),
            operations::Error::ServerError => Denial::ServerError,
            operations::Error::ProjectNotFound | operations::Error::Rejection { .. } => {
                Denial::ServerError
            }
        }
    }
}

/// Wrap an operation outcome in the v1 response.
fn op_response(outcome: operations::Outcome) -> Result<Response, Denial> {
    let status = StatusCode::from_u16(outcome.status).map_err(|_| Denial::ServerError)?;
    Ok(json_response(status, outcome.body))
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

/// Fetch the project row for the write paths via the public read service.
async fn fetch_project_for_write(pool: &PgPool, project_id: &Uuid) -> Result<WriteProject, Denial> {
    read::fetch_project_for_write(pool, project_id)
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
    // `TimezoneMixin.initial` precedes the view body: an unknown stored
    // zone 400s here, before the project fetch 404s below.
    activate_timezone(pre.actor.timezone.as_deref())?;
    // `Project.objects.get` precedes body parsing (`:471-474`), so a
    // missing project 404s before a malformed body 400s.
    let project = fetch_project_for_write(&pre.pool, &project_id).await?;
    let parsed_body = parse_write_body(headers, raw_body)?;
    let orm = state.orm().ok_or(Denial::ServerError)?;
    let ctx = domain_write_ctx(&pre, &project_id);
    let transition: Arc<dyn IssueTransitionHook> =
        Arc::new(AppTransitionHook::new(state, &pre.pool));
    let enqueue: Arc<dyn TaskEnqueueHook> = Arc::new(AppTaskEnqueueHook::new(&pre.pool));
    let deps = operations::Deps {
        pool: pre.pool.clone(),
        orm: orm.clone(),
        transition,
        enqueue,
    };
    let write = operations::issues::IssueWrite {
        slug,
        project_id,
        workspace_id,
        project,
        body: &parsed_body.value,
        from_form: parsed_body.from_form,
        origin: app_origin(&state.settings().urls).ok(),
        web_base: shape::web_base_url(
            state.settings().urls.web_url.as_deref(),
            state.settings().urls.app_base_url.as_deref(),
        ),
    };
    let outcome = operations::issues::create(
        &deps,
        &ctx,
        &write,
        operations::issues::CreateOptions {
            emit_activity: true,
            emit_model_activity: true,
        },
    )
    .await?;
    op_response(outcome)
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
    // The issue fetch precedes the project fetch (`:792-793`).
    let current = fetch_issue_for_write(&pre.pool, slug, &project_id, pk, &tz).await?;
    let project = fetch_project_for_write(&pre.pool, &project_id).await?;

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

    // `current_instance` renders before body parsing (`:808-811`), with
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
    let orm = state.orm().ok_or(Denial::ServerError)?;
    let ctx = domain_write_ctx(&pre, &project_id);
    let transition: Arc<dyn IssueTransitionHook> =
        Arc::new(AppTransitionHook::new(state, &pre.pool));
    let enqueue: Arc<dyn TaskEnqueueHook> = Arc::new(AppTaskEnqueueHook::new(&pre.pool));
    let deps = operations::Deps {
        pool: pre.pool.clone(),
        orm: orm.clone(),
        transition,
        enqueue,
    };
    let write = operations::issues::IssueWrite {
        slug,
        project_id,
        workspace_id,
        project,
        body: &parsed_body.value,
        from_form: parsed_body.from_form,
        origin: app_origin(&state.settings().urls).ok(),
        web_base,
    };
    let target = operations::issues::UpdateTarget {
        current: &current,
        current_instance: current_text,
        moved_by_run,
    };
    let outcome = operations::issues::update(
        &deps,
        &ctx,
        *pk,
        &write,
        &target,
        operations::issues::UpdateOptions {
            emit_activity: true,
        },
    )
    .await?;
    op_response(outcome)
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
    // `deleted_at` and `updated_at` stamp separately — two `now()` calls,
    // as Django's `delete()` + `save()` make.
    let prev_state_id = crate::issue_transition::capture_prior_state_pool(&pre.pool, Some(*pk))
        .await
        .map_err(|_| Denial::ServerError)?;
    // The full-save base row, re-read beside the snapshot (the `DecodedIssue`
    // above is render-lossy and the save writes every column). A miss 404s —
    // the port would retarget zero rows and still 204, a mid-flight-delete
    // race either way.
    let orm = state.orm().ok_or(Denial::ServerError)?;
    let Some(base) = issues::Entity::find_by_id(*pk)
        .one(orm)
        .await
        .map_err(|error| db_error(error, "write-reread"))?
    else {
        return Err(Denial::NotFound(RESOURCE_NOT_FOUND_BODY.to_owned()));
    };
    let mut fields = domain_issues::IssueFieldSet::from(&base);
    fields.deleted_at = Some(now_utc());
    fields.updated_at = now_utc();
    let ctx = domain_write_ctx(&pre, &project_id);
    let actor_id = pre.actor.id;
    let slug = slug.to_owned();
    let transition: Arc<dyn IssueTransitionHook> =
        Arc::new(AppTransitionHook::new(state, &pre.pool));
    let enqueue: Arc<dyn TaskEnqueueHook> = Arc::new(AppTaskEnqueueHook::new(&pre.pool));
    let outcome = domain::run(&ctx, orm, |txn| {
        let transition = transition.clone();
        let enqueue = enqueue.clone();
        let pool = pre.pool.clone();
        let ctx = ctx.clone();
        Box::pin(async move {
            txn.hooks_mut().register(transition);
            txn.hooks_mut().register(enqueue);
            domain_issues::soft_delete(
                &ctx,
                txn,
                &pool,
                domain_issues::SoftDeleteIssue {
                    fields,
                    prev_state: prev_state_id,
                    fire: domain_issues::FireSpec {
                        dispatch_immediate: true,
                        created_by: Some(actor_id),
                        moved_by_run: None,
                        now: Utc::now(),
                    },
                    origin: None,
                    slug,
                    completion: true,
                    visit_delete: false,
                    activity_current: Value::String(current_text),
                    activity_notification: None,
                    activity_require_origin: false,
                    activity_subscriber: None,
                    skip_activity: false,
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

#[cfg(test)]
mod signal_tests {
    use super::*;

    fn uid(n: u128) -> Uuid {
        Uuid::from_u128(n)
    }

    use crate::issue_transition::test_support::{LazyStateSeam, UnreachedPreflight};
    use pidash_services::orchestration::entries::{
        fire_state_transition, FireOutcome, FireRequest,
    };

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
        let request = FireRequest {
            issue_id: uid(9),
            prev_state_id: Some(uid(3)),
            current_state_id: Some(uid(3)),
            dispatch_immediate: true,
            moved_by_run: Some(uid(7)),
            now,
            jitter_secs: 0.0,
            created_by: Some(uid(1)),
        };
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
        let request = FireRequest {
            issue_id: uid(9),
            prev_state_id: Some(uid(3)),
            current_state_id: Some(uid(4)),
            dispatch_immediate: true,
            moved_by_run: Some(uid(7)),
            now,
            jitter_secs: 0.0,
            created_by: Some(uid(1)),
        };
        let outcome = fire_state_transition(&mut seam, &mut preflight, &request).await;
        assert!(outcome.is_err());
    }
}
