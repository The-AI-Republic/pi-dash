//! Project handlers (D-19 handlers A, PIDASHCONV-369).
//!
//! Ports `apps/api/pi_dash/api/views/project.py:163-285` (list/create),
//! `:373-509` (detail get/patch/delete with identifier routing), `:528-574`
//! (archive/unarchive) and `:580-677` (summary with
//! `_get_all_summary_counts`), registered by [`super::routes`] at the four
//! `apps/api/pi_dash/api/urls/project.py:15-33` paths.
//!
//! Layering (all foundation use is read-only): row SQL + pure transforms in
//! `pidash_db::v1_projects` (`models`, `queries_projmem`), shapes and check
//! order in `pidash_services::v1_projects::ser_project`, gate decisions in
//! `super::perms` over the F-06 kernel (`pidash_auth::permissions`), task
//! kwargs in `pidash_services::v1_projects::tasks`. This module owns the
//! HTTP shell: API-key auth, the slug→UUID rewrite, permission wiring,
//! DRF field coercion, the write transactions, the read-shape rendering and
//! the paginated envelope.
//!
//! Request order (preserved, not redesigned): API-key authentication, then
//! the slug→UUID rewrite (`api/views/base.py:51-98`, skipped for anonymous
//! callers so slugs cannot be probed via 404-vs-401), then
//! `check_permissions`, then the handler body.
//!
//! Ported bugs and deliberate warts (also listed in the PR):
//!
//! * BUG-1 (`serializers/project.py:174-175,187-194`): a taken identifier
//!   usually answers the NAME conflict body, because
//!   `ProjectCreateSerializer.create` never writes the `ProjectIdentifier`
//!   row its own pre-check reads, so the clash trips the `projects` unique
//!   index instead. Only a clash against a `project_identifiers` row
//!   answers the identifier body (fixture `handlers/project.golden.json`,
//!   contract `test_create_conflicts`).
//! * `update()`-raised `ValidationError`s (default-state/estimate scope,
//!   unset-default, identifier-chars on the update path —
//!   `serializers/project.py:214-248`) are caught by the view's generic
//!   `except ValidationError` and answer the identifier-taken 409 body
//!   (`views/project.py:463-467`), not their own message.
//! * The model `save()` backstop (`db/models/project.py:274-290`) raises
//!   Django's `ValidationError`, which is a different class from DRF's, so
//!   it escapes the view's `except` chain into the base
//!   `handle_exception` 400 (`api/views/base.py:148-152`).
//! * The summary `counts` dict is built from a `set`
//!   (`views/project.py:589-595`), so its key order varies per process;
//!   the port emits `ALLOWED_PROJECT_SUMMARY_FIELDS` order deterministically.
//! * The PATCH before-image (`views/project.py:412`) serializes a plain
//!   (annotation-less) instance; missing annotation attributes render null.
//!
//! Fixture: `FX-H-PROJ`
//! (`rust-api/fixtures/v1_projects/handlers/project.golden.json`).
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

use std::collections::HashMap;

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use chrono_tz::Tz;
use serde_json::Value;
use sqlx::{PgPool, Row};

use pidash_auth::permissions::{project, workspace};
use pidash_auth::scope::TenantScope;

use crate::state::AppState;

// ---------------------------------------------------------------------------
// Responses
// ---------------------------------------------------------------------------

/// Exact bytes of the DRF `IsAuthenticated` denial (anonymous on a guarded
/// endpoint: `APIKeyAuthentication.authenticate` returns `None`, so
/// `permission_denied` raises `NotAuthenticated`, not `PermissionDenied`).
pub const UNAUTHENTICATED_BODY: &str =
    r#"{"detail":"Authentication credentials were not provided."}"#;
/// `APIKeyAuthentication` failure (`api/middleware/api_authentication.py`):
/// every token rejection maps to this single 403 body.
pub const INVALID_TOKEN_BODY: &str = r#"{"detail":"Given API token is not valid"}"#;
/// `handle_exception`'s `ObjectDoesNotExist` branch
/// (`api/views/base.py:154-158`): `.get()` misses on detail/archive/delete.
pub const NOT_FOUND_BODY: &str = r#"{"error":"The requested resource does not exist."}"#;
/// DRF's `Http404` rendering with an explicit message: `Project.resolve`
/// misses raise `Http404("Project not found")`, which DRF's
/// `exception_handler` re-raises as `NotFound(*exc.args)`, so the message
/// survives instead of the `"Not found."` default
/// (`db/models/project.py:213-218`).
pub const PROJECT_NOT_FOUND_BODY: &str = r#"{"detail":"Project not found"}"#;
/// `handle_exception`'s generic branch (`api/views/base.py:166-170`).
pub const SERVER_ERROR_BODY: &str = r#"{"error":"Something went wrong please try again later"}"#;

/// Handler failure with its exact status + body.
#[derive(Debug)]
pub enum Denial {
    /// 401, DRF `NotAuthenticated` (no `X-Api-Key` header).
    Unauthorized,
    /// 403, invalid/expired/inactive API or machine token.
    InvalidToken,
    /// 403, the DRF-default `PermissionDenied` body (no D-19 guard class
    /// sets `message`).
    Forbidden,
    /// 404, `ObjectDoesNotExist` branch.
    NotFound,
    /// 404, `{"detail":"Project not found"}` (identifier rewrite miss).
    ProjectNotFound,
    /// 400, `{"detail": ...}` (DRF `ParseError`: pagination, JSON).
    BadDetail(String),
    /// 400, `{"error": ...}` (view-inline).
    BadError(String),
    /// 400, serializer `errors` dict (pre-rendered bytes, field order).
    FieldErrors(String),
    /// 404, view-inline `{"error": ...}` with a custom message.
    NotFoundError(String),
    /// 409, `{"name": ...}` / `{"identifier": ...}` (pre-rendered bytes).
    Conflict(String),
    /// 500, generic branch.
    ServerError,
}

impl Denial {
    fn status_and_body(&self) -> (StatusCode, String) {
        match self {
            Denial::Unauthorized => (StatusCode::UNAUTHORIZED, UNAUTHENTICATED_BODY.to_owned()),
            Denial::InvalidToken => (StatusCode::FORBIDDEN, INVALID_TOKEN_BODY.to_owned()),
            Denial::Forbidden => (
                StatusCode::FORBIDDEN,
                super::perms::CLASS_DENIAL_BODY.to_owned(),
            ),
            Denial::NotFound => (StatusCode::NOT_FOUND, NOT_FOUND_BODY.to_owned()),
            Denial::ProjectNotFound => (StatusCode::NOT_FOUND, PROJECT_NOT_FOUND_BODY.to_owned()),
            Denial::BadDetail(message) => (
                StatusCode::BAD_REQUEST,
                format!("{{\"detail\":{}}}", json_string(message)),
            ),
            Denial::BadError(message) => (
                StatusCode::BAD_REQUEST,
                format!("{{\"error\":{}}}", json_string(message)),
            ),
            Denial::FieldErrors(body) | Denial::Conflict(body) => {
                let status = match self {
                    Denial::Conflict(_) => StatusCode::CONFLICT,
                    _ => StatusCode::BAD_REQUEST,
                };
                (status, body.clone())
            }
            Denial::NotFoundError(message) => (
                StatusCode::NOT_FOUND,
                format!("{{\"error\":{}}}", json_string(message)),
            ),
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
        Response::builder()
            .status(status)
            .header(axum::http::header::CONTENT_TYPE, "application/json")
            .body(axum::body::Body::from(body))
            .expect("static denial response")
    }
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).expect("json string")
}

/// Render a 200 JSON response with exact bytes.
fn json_ok(body: String) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(axum::http::header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body))
        .expect("handler json response")
}

/// Render a 201 JSON response with exact bytes.
fn json_created(body: String) -> Response {
    Response::builder()
        .status(StatusCode::CREATED)
        .header(axum::http::header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body))
        .expect("handler json response")
}

/// Render an empty 204 response.
fn no_content() -> Response {
    Response::builder()
        .status(StatusCode::NO_CONTENT)
        .body(axum::body::Body::empty())
        .expect("handler empty response")
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
/// pattern shared with the D-26 `app_issues` family): the owned methods
/// serve from Rust, every other method on the path proxies to Django so its
/// 405-after-auth and metadata responses are preserved byte for byte.
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
    router
}

/// `workspaces/<slug>/projects/` owns GET+POST (`urls/project.py:16-20`).
#[allow(dead_code)]
pub fn owned_list(
    router: axum::routing::MethodRouter<AppState>,
) -> axum::routing::MethodRouter<AppState> {
    owned(router, &["GET", "POST"])
}

/// `workspaces/<slug>/projects/<pk>/` owns GET+PATCH+DELETE
/// (`urls/project.py:21-25`).
#[allow(dead_code)]
pub fn owned_detail(
    router: axum::routing::MethodRouter<AppState>,
) -> axum::routing::MethodRouter<AppState> {
    owned(router, &["GET", "PATCH", "DELETE"])
}

/// `.../<project_id>/archive/` owns POST+DELETE (`urls/project.py:26-30`).
#[allow(dead_code)]
pub fn owned_archive(
    router: axum::routing::MethodRouter<AppState>,
) -> axum::routing::MethodRouter<AppState> {
    owned(router, &["POST", "DELETE"])
}

/// `.../<project_id>/summary/` owns GET (`urls/project.py:31-35`).
#[allow(dead_code)]
pub fn owned_summary(
    router: axum::routing::MethodRouter<AppState>,
) -> axum::routing::MethodRouter<AppState> {
    owned(router, &["GET"])
}

// ---------------------------------------------------------------------------
// Query params
// ---------------------------------------------------------------------------

/// One query value, repeated or not (same shape as the D-26 `app_issues`
/// family: axum's `Query` backend does not coerce a lone `?key=value` into
/// a sequence, so callers read first/last like Django's `QueryDict`).
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

/// Decoded `file_assets` row for [`file_asset_url`].
type FileAssetLookup = (
    Option<String>,
    Option<uuid::Uuid>,
    Option<uuid::Uuid>,
    Option<uuid::Uuid>,
);

/// Decoded user row for [`expand_user`]: names, nullable `email`
/// (`db/models/user.py:61`), `avatar` text, avatar-asset FK.
type ExpandUserLookup = (String, String, Option<String>, String, Option<uuid::Uuid>);

/// Map a database/driver failure to the generic 500 while logging the site
/// and error for operators (no secrets: messages never include tokens).
fn db_error<E: std::fmt::Display>(error: E, site: &str) -> Denial {
    tracing::warn!(%error, site, "v1_projects database failure");
    Denial::ServerError
}

/// The authenticated actor: user id plus the RAW stored time-zone name.
/// The zone is NOT parsed here — `TimezoneMixin.initial`
/// (`api/views/base.py:43-48`) calls `super().initial()`
/// (auth + permissions) first and only then activates the zone, so a
/// denying gate answers 403 even when the stored zone is unknown (which
/// 400s only for survivors, via [`activate_timezone`]).
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

/// `_rewrite_project_kwarg` (`api/views/base.py:51-98`): a slug-or-UUID
/// `project_id` (or `pk` on the `project` URL name) becomes the canonical
/// project UUID before permission checks. UUID-looking input passes through
/// unverified (the view body 404/403s it as before); misses answer the
/// `{"detail":"Project not found"}` 404. Only the detail/archive/summary
/// routes rewrite; the list route has no project kwarg.
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
            .map_err(|error| db_error(error, "ws-role"))?
            .ok_or(Denial::ProjectNotFound),
    }
}

/// Fetch the `ProjectBasePermission` facts for `(workspace, user,
/// project)` exactly as the guard filters them
/// (`app/permissions/project.py:13-55`): workspace membership (any role,
/// active), workspace admin-or-member, workspace admin, project membership
/// (any role / admin, active, workspace-scoped).
pub async fn project_base_facts(
    pool: &PgPool,
    workspace_id: &uuid::Uuid,
    workspace_slug: &str,
    user_id: &uuid::Uuid,
    project_id: &uuid::Uuid,
) -> Result<project::ProjectFacts, Denial> {
    let ws_member: bool = ws_has_role(pool, workspace_id, user_id, &[20, 15, 5]).await?;
    let ws_admin_or_member: bool = ws_has_role(pool, workspace_id, user_id, &[20, 15]).await?;
    let ws_admin: bool = ws_has_role(pool, workspace_id, user_id, &[20]).await?;
    let (proj_member, proj_admin, proj_admin_or_member) =
        project_roles(pool, workspace_id, user_id, project_id).await?;
    Ok(project::ProjectFacts {
        workspace: pidash_types::WorkspaceId::from(workspace_slug.to_owned()),
        project_id: pidash_types::ProjectId::from(project_id.to_string()),
        authenticated: true,
        is_workspace_member: ws_member,
        has_workspace_admin_or_member: ws_admin_or_member,
        is_workspace_admin: ws_admin,
        is_project_member: proj_member,
        is_project_admin: proj_admin,
        has_project_admin_or_member: proj_admin_or_member,
        has_identifier_membership: false,
        has_project_identifier: false,
    })
}

async fn ws_has_role(
    pool: &PgPool,
    workspace_id: &uuid::Uuid,
    user_id: &uuid::Uuid,
    roles: &[i32],
) -> Result<bool, Denial> {
    // `role` is `smallint` (`PositiveSmallIntegerField`); the bound
    // `Vec<i32>` encodes as `int4[]`, and Postgres has no
    // `smallint = integer` operator, so the column casts up.
    let exists: bool = sqlx::query_scalar(
        r#"SELECT EXISTS(SELECT 1 FROM "workspace_members" WHERE "workspace_id" = $1 AND "member_id" = $2 AND "role"::int = ANY($3) AND "is_active" AND "deleted_at" IS NULL)"#,
    )
    .bind(workspace_id)
    .bind(user_id)
    .bind(roles.to_vec())
    .fetch_one(pool)
    .await
    .map_err(|error| db_error(error, "project-roles"))?;
    Ok(exists)
}

async fn project_roles(
    pool: &PgPool,
    workspace_id: &uuid::Uuid,
    user_id: &uuid::Uuid,
    project_id: &uuid::Uuid,
) -> Result<(bool, bool, bool), Denial> {
    // `role` is `smallint`: sqlx does not widen `INT2` into `i32` on
    // decode, so read `i16` and compare as integers.
    let roles: Vec<i16> = sqlx::query_scalar(
        r#"SELECT "role" FROM "project_members" WHERE "workspace_id" = $1 AND "member_id" = $2 AND "project_id" = $3 AND "is_active" AND "deleted_at" IS NULL"#,
    )
    .bind(workspace_id)
    .bind(user_id)
    .bind(project_id)
    .fetch_all(pool)
    .await
    .map_err(|error| db_error(error, "project-roles"))?;
    Ok((
        !roles.is_empty(),
        roles.contains(&20),
        roles.iter().any(|r| *r == 20 || *r == 15),
    ))
}

/// Fetch the `WorkSpaceAdminPermission` facts (`workspace.py:61-71`):
/// active workspace membership with role ADMIN or MEMBER.
pub async fn workspace_admin_facts(
    pool: &PgPool,
    workspace_slug: &str,
    workspace_id: &uuid::Uuid,
    user_id: &uuid::Uuid,
) -> Result<workspace::WorkspaceFacts, Denial> {
    let admin_or_member = ws_has_role(pool, workspace_id, user_id, &[20, 15]).await?;
    let admin = ws_has_role(pool, workspace_id, user_id, &[20]).await?;
    let member = ws_has_role(pool, workspace_id, user_id, &[20, 15, 5]).await?;
    Ok(workspace::WorkspaceFacts {
        workspace: pidash_types::WorkspaceId::from(workspace_slug.to_owned()),
        authenticated: true,
        has_admin_or_member_role: admin_or_member,
        has_admin_role: admin,
        is_member: member,
        is_admin_unfiltered: admin,
    })
}

// ---------------------------------------------------------------------------
// Read-shape rendering (`ProjectSerializer`, serializers/project.py:251-353)
// ---------------------------------------------------------------------------

/// Canonical wire order: `[pk] + declared annotation fields + concrete
/// model fields + forward relations` (DRF `get_default_field_names`,
/// verified against DRF 3.18.1). The renderer walks this order and keeps
/// the `fields` subset, so `?fields=` answers stay in declaration order
/// exactly like `_filter_fields` popping from the ordered field dict.
const READ_FIELD_ORDER: &[&str] = &[
    "id",
    "total_members",
    "total_cycles",
    "total_modules",
    "is_member",
    "sort_order",
    "member_role",
    "is_deployed",
    "cover_image_url",
    "created_at",
    "updated_at",
    "deleted_at",
    "name",
    "description",
    "description_text",
    "description_html",
    "network",
    "identifier",
    "emoji",
    "icon_prop",
    "module_view",
    "cycle_view",
    "issue_views_view",
    "page_view",
    "intake_view",
    "is_time_tracking_enabled",
    "is_issue_type_enabled",
    "is_default",
    "guest_view_all_features",
    "members_can_edit_states",
    "cover_image",
    "archive_in",
    "close_in",
    "logo_props",
    "archived_at",
    "timezone",
    "external_source",
    "external_id",
    "repo_url",
    "base_branch",
    "agent_default_interval_seconds",
    "agent_default_max_ticks",
    "agent_review_default_interval_seconds",
    "agent_test_default_interval_seconds",
    "agent_ticking_enabled",
    "default_agent_executor",
    "created_by",
    "updated_by",
    "workspace",
    "default_assignee",
    "project_lead",
    "cover_image_asset",
    "estimate",
    "default_state",
];

/// Annotation values for one project row. `sort_order` is `Some` only on
/// the list-GET statement; every other response reads null (fixture
/// `FX-PROJ-SER`, `ser_project::READ_ANNOTATION_FIELDS`).
pub struct ReadAnnotations {
    pub total_members: i64,
    pub total_cycles: i64,
    pub total_modules: i64,
    pub is_member: bool,
    pub sort_order: Option<f64>,
    pub member_role: Option<i32>,
    pub is_deployed: bool,
}

fn render_uuid(value: uuid::Uuid) -> Value {
    Value::String(value.to_string())
}

fn render_uuid_opt(value: Option<uuid::Uuid>) -> Value {
    value.map(render_uuid).unwrap_or(Value::Null)
}

fn render_datetime_opt(value: Option<chrono::DateTime<chrono::Utc>>, tz: &Tz) -> Value {
    value
        .map(|dt| Value::String(crate::serializer::render_datetime_in(&dt, tz)))
        .unwrap_or(Value::Null)
}

fn row_string(row: &sqlx::postgres::PgRow, col: &str) -> Result<String, Denial> {
    row.try_get(col).map_err(|_| Denial::ServerError)
}

fn row_string_opt(row: &sqlx::postgres::PgRow, col: &str) -> Result<Option<String>, Denial> {
    row.try_get(col).map_err(|_| Denial::ServerError)
}

fn row_uuid_opt(row: &sqlx::postgres::PgRow, col: &str) -> Result<Option<uuid::Uuid>, Denial> {
    row.try_get(col).map_err(|_| Denial::ServerError)
}

fn row_bool(row: &sqlx::postgres::PgRow, col: &str) -> Result<bool, Denial> {
    row.try_get(col).map_err(|_| Denial::ServerError)
}

fn row_i32(row: &sqlx::postgres::PgRow, col: &str) -> Result<i32, Denial> {
    row.try_get(col).map_err(|_| Denial::ServerError)
}

/// `projects.network` is `PositiveSmallIntegerField` (INT2); sqlx does not
/// widen INT2 into i32, so decode i16 and widen (same hazard as the
/// `member_role` fix in PIDASHCONV-478).
fn row_i16_as_i32(row: &sqlx::postgres::PgRow, col: &str) -> Result<i32, Denial> {
    let v: i16 = row.try_get(col).map_err(|_| Denial::ServerError)?;
    Ok(i32::from(v))
}

fn row_json_opt(row: &sqlx::postgres::PgRow, col: &str) -> Result<Value, Denial> {
    let value: Option<Value> = row.try_get(col).map_err(|_| Denial::ServerError)?;
    Ok(value.unwrap_or(Value::Null))
}

/// `Project.cover_image_url` (`db/models/project.py:175-185`): the cover
/// asset's URL when an asset is attached, else the `cover_image` text, else
/// `None`. The asset branch applies `FileAsset.asset_url`
/// (`db/models/asset.py:80-98`).
async fn cover_image_url(
    pool: &PgPool,
    asset_id: Option<uuid::Uuid>,
    cover_image: Option<&str>,
) -> Result<Value, Denial> {
    if let Some(id) = asset_id {
        if let Some(url) = file_asset_url(pool, &id).await? {
            return Ok(Value::String(url));
        }
    }
    Ok(cover_image
        .filter(|s| !s.is_empty())
        .map(|s| Value::String(s.to_owned()))
        .unwrap_or(Value::Null))
}

/// `FileAsset.asset_url` for one asset row.
async fn file_asset_url(pool: &PgPool, asset_id: &uuid::Uuid) -> Result<Option<String>, Denial> {
    let row: Option<FileAssetLookup> =
        sqlx::query_as(
            r#"SELECT fa."entity_type", fa."workspace_id", fa."project_id", fa."issue_id" FROM "file_assets" fa WHERE fa."id" = $1"#,
        )
        .bind(asset_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let Some((entity_type, workspace_id, project_id, issue_id)) = row else {
        return Ok(None);
    };
    match entity_type.as_deref() {
        Some("WORKSPACE_LOGO")
        | Some("USER_AVATAR")
        | Some("USER_COVER")
        | Some("PROJECT_COVER") => Ok(Some(format!("/api/assets/v2/static/{asset_id}/"))),
        Some("ISSUE_ATTACHMENT") => {
            let slug: Option<String> = match workspace_id {
                Some(id) => {
                    sqlx::query_scalar(r#"SELECT "slug" FROM "workspaces" WHERE "id" = $1"#)
                        .bind(id)
                        .fetch_optional(pool)
                        .await
                        .map_err(|_| Denial::ServerError)?
                        .flatten()
                }
                None => None,
            };
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
            let slug: Option<String> = match workspace_id {
                Some(id) => {
                    sqlx::query_scalar(r#"SELECT "slug" FROM "workspaces" WHERE "id" = $1"#)
                        .bind(id)
                        .fetch_optional(pool)
                        .await
                        .map_err(|_| Denial::ServerError)?
                        .flatten()
                }
                None => None,
            };
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

/// Render one project row in `ProjectSerializer` bytes: annotations, then
/// every model field, FKs as pk strings, datetimes in the request zone,
/// `fields`-filtered in declaration order, `expand` applied afterwards.
///
/// Annotation presence (`Field.get_attribute` → `SkipField` on missing
/// attributes, `fields.py:448-453`): `ann=None` (the PATCH before-image
/// over a plain instance) omits all 8 annotation keys;
/// `with_sort_order=false` (detail/create/update querysets, which never
/// annotate it) omits only `sort_order`; the list GET renders all 8
/// (`sort_order` null when the caller holds no membership row).
#[allow(clippy::too_many_arguments)]
pub async fn render_project(
    pool: &PgPool,
    row: &sqlx::postgres::PgRow,
    ann: Option<&ReadAnnotations>,
    with_sort_order: bool,
    tz: &Tz,
    fields: Option<&[String]>,
    expand: Option<&[String]>,
) -> Result<String, Denial> {
    let keep = |key: &str| fields.map(|f| f.iter().any(|v| v == key)).unwrap_or(true);
    let mut map = serde_json::Map::with_capacity(READ_FIELD_ORDER.len());
    let put = |map: &mut serde_json::Map<String, Value>, key: &str, value: Value| {
        if keep(key) {
            map.insert(key.to_owned(), value);
        }
    };

    let id: uuid::Uuid = row.try_get("id").map_err(|_| Denial::ServerError)?;
    put(&mut map, "id", render_uuid(id));
    if let Some(ann) = ann {
        put(&mut map, "total_members", Value::from(ann.total_members));
        put(&mut map, "total_cycles", Value::from(ann.total_cycles));
        put(&mut map, "total_modules", Value::from(ann.total_modules));
        put(&mut map, "is_member", Value::from(ann.is_member));
        if with_sort_order {
            put(
                &mut map,
                "sort_order",
                ann.sort_order
                    .map(|v| {
                        serde_json::Number::from_f64(v)
                            .map(Value::Number)
                            .unwrap_or(Value::Null)
                    })
                    .unwrap_or(Value::Null),
            );
        }
        put(
            &mut map,
            "member_role",
            ann.member_role.map(Value::from).unwrap_or(Value::Null),
        );
        put(&mut map, "is_deployed", Value::from(ann.is_deployed));
    }
    let asset_id: Option<uuid::Uuid> = row_uuid_opt(row, "cover_image_asset_id")?;
    let cover_text: Option<String> = row_string_opt(row, "cover_image")?;
    put(
        &mut map,
        "cover_image_url",
        cover_image_url(pool, asset_id, cover_text.as_deref()).await?,
    );

    let created_at: chrono::DateTime<chrono::Utc> =
        row.try_get("created_at").map_err(|_| Denial::ServerError)?;
    let updated_at: chrono::DateTime<chrono::Utc> =
        row.try_get("updated_at").map_err(|_| Denial::ServerError)?;
    let deleted_at: Option<chrono::DateTime<chrono::Utc>> =
        row.try_get("deleted_at").map_err(|_| Denial::ServerError)?;
    let archived_at: Option<chrono::DateTime<chrono::Utc>> = row
        .try_get("archived_at")
        .map_err(|_| Denial::ServerError)?;
    put(
        &mut map,
        "created_at",
        Value::String(crate::serializer::render_datetime_in(&created_at, tz)),
    );
    put(
        &mut map,
        "updated_at",
        Value::String(crate::serializer::render_datetime_in(&updated_at, tz)),
    );
    put(&mut map, "deleted_at", render_datetime_opt(deleted_at, tz));
    put(&mut map, "name", Value::String(row_string(row, "name")?));
    put(
        &mut map,
        "description",
        Value::String(row_string_opt(row, "description")?.unwrap_or_default()),
    );
    put(
        &mut map,
        "description_text",
        row_json_opt(row, "description_text")?,
    );
    put(
        &mut map,
        "description_html",
        row_json_opt(row, "description_html")?,
    );
    put(
        &mut map,
        "network",
        Value::from(row_i16_as_i32(row, "network")?),
    );
    put(
        &mut map,
        "identifier",
        Value::String(row_string(row, "identifier")?),
    );
    put(
        &mut map,
        "emoji",
        row_string_opt(row, "emoji")?
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    put(&mut map, "icon_prop", row_json_opt(row, "icon_prop")?);
    for key in [
        "module_view",
        "cycle_view",
        "issue_views_view",
        "page_view",
        "intake_view",
        "is_time_tracking_enabled",
        "is_issue_type_enabled",
        "is_default",
        "guest_view_all_features",
        "members_can_edit_states",
    ] {
        put(&mut map, key, Value::from(row_bool(row, key)?));
    }
    put(
        &mut map,
        "cover_image",
        cover_text.map(Value::String).unwrap_or(Value::Null),
    );
    put(
        &mut map,
        "archive_in",
        Value::from(row_i32(row, "archive_in")?),
    );
    put(&mut map, "close_in", Value::from(row_i32(row, "close_in")?));
    put(&mut map, "logo_props", row_json_opt(row, "logo_props")?);
    put(
        &mut map,
        "archived_at",
        render_datetime_opt(archived_at, tz),
    );
    put(
        &mut map,
        "timezone",
        Value::String(row_string(row, "timezone")?),
    );
    put(
        &mut map,
        "external_source",
        row_string_opt(row, "external_source")?
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    put(
        &mut map,
        "external_id",
        row_string_opt(row, "external_id")?
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    put(
        &mut map,
        "repo_url",
        Value::String(row_string_opt(row, "repo_url")?.unwrap_or_default()),
    );
    put(
        &mut map,
        "base_branch",
        Value::String(row_string_opt(row, "base_branch")?.unwrap_or("main".to_owned())),
    );
    for key in [
        "agent_default_interval_seconds",
        "agent_default_max_ticks",
        "agent_review_default_interval_seconds",
        "agent_test_default_interval_seconds",
    ] {
        let v: i32 = row.try_get(key).map_err(|_| Denial::ServerError)?;
        put(&mut map, key, Value::from(v));
    }
    put(
        &mut map,
        "agent_ticking_enabled",
        Value::from(row_bool(row, "agent_ticking_enabled")?),
    );
    put(
        &mut map,
        "default_agent_executor",
        Value::String(row_string(row, "default_agent_executor")?),
    );
    put(
        &mut map,
        "created_by",
        render_uuid_opt(row_uuid_opt(row, "created_by_id")?),
    );
    put(
        &mut map,
        "updated_by",
        render_uuid_opt(row_uuid_opt(row, "updated_by_id")?),
    );
    let workspace_id: uuid::Uuid = row
        .try_get("workspace_id")
        .map_err(|_| Denial::ServerError)?;
    put(&mut map, "workspace", render_uuid(workspace_id));
    put(
        &mut map,
        "default_assignee",
        render_uuid_opt(row_uuid_opt(row, "default_assignee_id")?),
    );
    put(
        &mut map,
        "project_lead",
        render_uuid_opt(row_uuid_opt(row, "project_lead_id")?),
    );
    put(&mut map, "cover_image_asset", render_uuid_opt(asset_id));
    put(
        &mut map,
        "estimate",
        render_uuid_opt(row_uuid_opt(row, "estimate_id")?),
    );
    put(
        &mut map,
        "default_state",
        render_uuid_opt(row_uuid_opt(row, "default_state_id")?),
    );

    // `BaseSerializer.to_representation` expand (`serializers/base.py:71+`):
    // each requested key present in the output is replaced in place (order
    // kept): mapped FKs render their lite serializer, anything else reads
    // the `<key>_id` attribute (null when absent).
    if let Some(keys) = expand {
        for key in keys {
            if !map.contains_key(key) {
                continue;
            }
            let expanded = match key.as_str() {
                "workspace" => expand_workspace(pool, &workspace_id).await?,
                "default_assignee" | "project_lead" | "created_by" | "updated_by" => {
                    let col = format!("{key}_id");
                    // `created_by`/`updated_by` share the audit attnames.
                    let attname = match key.as_str() {
                        "created_by" => "created_by_id",
                        "updated_by" => "updated_by_id",
                        _ => col.as_str(),
                    };
                    match row_uuid_opt(row, attname)? {
                        Some(uid) => expand_user(pool, &uid).await?,
                        // `expansion[expand](None).data` renders `{}` for a
                        // null FK (`serializers/base.py:108-113`, probed:
                        // `UserLiteSerializer(None).data == {}`).
                        None => Value::Object(serde_json::Map::new()),
                    }
                }
                _ => {
                    let attname = format!("{key}_id");
                    match row.try_get::<Option<uuid::Uuid>, _>(attname.as_str()) {
                        Ok(v) => render_uuid_opt(v),
                        Err(_) => match row.try_get::<Option<String>, _>(key.as_str()) {
                            Ok(v) => v.map(Value::String).unwrap_or(Value::Null),
                            Err(_) => Value::Null,
                        },
                    }
                }
            };
            map.insert(key.clone(), expanded);
        }
    }

    serde_json::to_string(&Value::Object(map)).map_err(|_| Denial::ServerError)
}

/// `expand=workspace`: `WorkspaceLiteSerializer` (`workspace.py:10-21`):
/// `{"name", "slug", "id"}` in field order.
async fn expand_workspace(pool: &PgPool, workspace_id: &uuid::Uuid) -> Result<Value, Denial> {
    let row: Option<(String, String)> =
        sqlx::query_as(r#"SELECT "name", "slug" FROM "workspaces" WHERE "id" = $1"#)
            .bind(workspace_id)
            .fetch_optional(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    Ok(match row {
        Some((name, slug)) => {
            let mut map = serde_json::Map::with_capacity(3);
            map.insert("name".to_owned(), Value::String(name));
            map.insert("slug".to_owned(), Value::String(slug));
            map.insert("id".to_owned(), render_uuid(*workspace_id));
            Value::Object(map)
        }
        None => Value::Null,
    })
}

/// `expand=<user fk>`: `UserLiteSerializer` via the ported
/// `user_lite_to_representation` kernel (FX-COLLAB-SER); a missing user
/// row renders null like `getattr` on a dead FK. `email` is nullable
/// (`CharField(null=True)`, `db/models/user.py:61`); `None` renders null.
async fn expand_user(pool: &PgPool, user_id: &uuid::Uuid) -> Result<Value, Denial> {
    let row: Option<ExpandUserLookup> = sqlx::query_as(
        r#"SELECT "first_name", "last_name", "email", COALESCE("avatar", ''), "avatar_asset_id" FROM "users" WHERE "id" = $1"#,
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
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
    // `display_name`: the model property (`user.py`); fetched raw.
    let display_name: Option<String> =
        sqlx::query_scalar(r#"SELECT "display_name" FROM "users" WHERE "id" = $1"#)
            .bind(user_id)
            .fetch_optional(pool)
            .await
            .map_err(|_| Denial::ServerError)?
            .flatten();
    let id = user_id.to_string();
    let lite_row = pidash_services::v1_projects::ser_collab::UserLiteRow {
        id: &id,
        first_name: &first_name,
        last_name: &last_name,
        email: email.as_deref(),
        avatar: &avatar,
        avatar_url,
        display_name: display_name.as_deref().unwrap_or(""),
    };
    let view = pidash_services::v1_projects::ser_collab::user_lite_to_representation(&lite_row);
    serde_json::to_value(&view).map_err(|_| Denial::ServerError)
}

/// Check the `ProjectBasePermission` gate for `method`; deny with the
/// class body on failure.
pub async fn require_project_base(
    pool: &PgPool,
    workspace_id: &uuid::Uuid,
    workspace_slug: &str,
    user_id: &uuid::Uuid,
    project_id: Option<&uuid::Uuid>,
    method: &str,
) -> Result<(), Denial> {
    use super::perms::{decide, gate_for, V1Route};
    // The gate needs a project id; list/create carry none, so the POST
    // branch (workspace-scoped only) decides on empty project facts.
    let nil = uuid::Uuid::nil();
    let pid = project_id.unwrap_or(&nil);
    let facts = project_base_facts(pool, workspace_id, workspace_slug, user_id, pid).await?;
    let scope = TenantScope::new(pidash_types::WorkspaceId::from(workspace_slug.to_owned()));
    // Route identity only selects the gate family here; all four project
    // routes carry `ProjectBasePermission`.
    let gate = gate_for(V1Route::ProjectList, method);
    let mutation = project::StateMutationFacts {
        authenticated: true,
        project_role: None,
        members_can_edit_states: false,
        is_workspace_admin: false,
    };
    if decide(
        gate,
        method,
        &scope,
        &facts,
        &workspace::WorkspaceFacts {
            workspace: pidash_types::WorkspaceId::from(workspace_slug.to_owned()),
            authenticated: true,
            has_admin_or_member_role: false,
            has_admin_role: false,
            is_member: false,
            is_admin_unfiltered: false,
        },
        &mutation,
    ) {
        Ok(())
    } else {
        Err(Denial::Forbidden)
    }
}

// ---------------------------------------------------------------------------
// Request bodies + DRF field coercion
// ---------------------------------------------------------------------------

/// Parse the request body the way DRF's `JSONParser` does for these views:
/// empty → `{}`; malformed → `ParseError` 400; non-object JSON → the
/// `non_field_errors` shape `is_valid` answers for non-dict data (DRF type
/// names). The text after `JSON parse error - ` comes from this engine's
/// scanner, not CPython's — status and key are the contract (same precedent
/// as the assistant config handlers).
pub fn parse_body(raw: &[u8]) -> Result<serde_json::Map<String, Value>, Denial> {
    if raw.is_empty() {
        return Ok(serde_json::Map::new());
    }
    match serde_json::from_slice::<Value>(raw) {
        Ok(Value::Object(map)) => Ok(map),
        Ok(other) => {
            let kind = match &other {
                Value::Array(_) => "list",
                Value::String(_) => "str",
                Value::Number(_) => {
                    if other.as_i64().is_some() {
                        "int"
                    } else {
                        "float"
                    }
                }
                Value::Bool(_) => "bool",
                Value::Null => "NoneType",
                Value::Object(_) => unreachable!("matched above"),
            };
            Err(Denial::FieldErrors(format!(
                "{{\"non_field_errors\":[\"Invalid data. Expected a dictionary, but got {kind}.\"]}}"
            )))
        }
        Err(error) => Err(Denial::BadDetail(format!("JSON parse error - {error}"))),
    }
}

/// How a field failed coercion.
#[derive(Debug)]
enum CoerceFail {
    /// A field error message for the errors dict.
    Msg(String),
}

/// One coerced value.
#[derive(Debug, Clone)]
pub enum FieldValue {
    Str(String),
    OptStr(Option<String>),
    Bool(bool),
    Int(i32),
    Fk(Option<uuid::Uuid>),
    Json(Value),
}

pub const REQUIRED_MSG: &str = "This field is required.";
pub const NULL_MSG: &str = "This field may not be null.";
pub const BLANK_MSG: &str = "This field may not be blank.";
pub const INVALID_STR_MSG: &str = "Not a valid string.";
pub const INVALID_BOOL_MSG: &str = "Must be a valid boolean.";
pub const INVALID_INT_MSG: &str = "A valid integer is required.";
pub const INVALID_JSON_MSG: &str = "Value must be valid JSON.";
pub const NULL_CHAR_MSG: &str = "Null characters are not allowed.";

fn max_length_msg(max: usize) -> String {
    format!("Ensure this field has no more than {max} characters.")
}

fn min_value_msg(min: i32) -> String {
    format!("Ensure this value is greater than or equal to {min}.")
}

fn max_value_msg(max: i32) -> String {
    format!("Ensure this value is less than or equal to {max}.")
}

fn invalid_choice_msg(input: &str) -> String {
    format!("\"{input}\" is not a valid choice.")
}

/// `CharField.run_validation` + `to_internal_value` (DRF `fields.py`):
/// whitespace-only input fails `blank` first (when not allowed), numbers
/// coerce to strings, bools/composites fail, over-long values fail after
/// trimming, then the null/surrogate guards run as validators.
fn coerce_char(
    value: &Value,
    allow_blank: bool,
    max_length: Option<usize>,
) -> Result<String, CoerceFail> {
    let raw = match value {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => return Err(CoerceFail::Msg(INVALID_STR_MSG.to_owned())),
    };
    if raw.is_empty() || raw.trim().is_empty() {
        if !allow_blank {
            return Err(CoerceFail::Msg(BLANK_MSG.to_owned()));
        }
        return Ok(String::new());
    }
    let trimmed = raw.trim().to_owned();
    if let Some(max) = max_length {
        if trimmed.chars().count() > max {
            return Err(CoerceFail::Msg(max_length_msg(max)));
        }
    }
    if trimmed.contains('\u{0}') {
        return Err(CoerceFail::Msg(NULL_CHAR_MSG.to_owned()));
    }
    if let Some(ch) = trimmed
        .chars()
        .find(|c| (0xD800..=0xDFFF).contains(&(*c as u32)))
    {
        return Err(CoerceFail::Msg(format!(
            "Surrogate characters are not allowed: U+{:X}.",
            ch as u32
        )));
    }
    Ok(trimmed)
}

/// `BooleanField.to_internal_value`: the `TRUE_VALUES` / `FALSE_VALUES`
/// sets (case-insensitive for strings); `1`/`1.0` count as true,
/// `0`/`0.0` as false; null and everything else fail.
fn coerce_bool(value: &Value) -> Result<bool, CoerceFail> {
    const TRUES: &[&str] = &["t", "y", "yes", "true", "on", "1"];
    const FALSES: &[&str] = &["f", "n", "no", "false", "off", "0"];
    match value {
        Value::Bool(b) => Ok(*b),
        Value::Number(n) => {
            if n.as_i64() == Some(1) || n.as_f64() == Some(1.0) {
                Ok(true)
            } else if n.as_i64() == Some(0) || n.as_f64() == Some(0.0) {
                Ok(false)
            } else {
                Err(CoerceFail::Msg(INVALID_BOOL_MSG.to_owned()))
            }
        }
        Value::String(s) => {
            let lower = s.to_lowercase();
            if TRUES.contains(&lower.as_str()) {
                Ok(true)
            } else if FALSES.contains(&lower.as_str()) {
                Ok(false)
            } else {
                Err(CoerceFail::Msg(INVALID_BOOL_MSG.to_owned()))
            }
        }
        _ => Err(CoerceFail::Msg(INVALID_BOOL_MSG.to_owned())),
    }
}

/// `IntegerField.to_internal_value` + min/max validators: `int()` over the
/// decimal-stripped string form (`"1.0"` passes, `"1.2"` fails), bools
/// fail, over-1000-char strings fail, then the range validators run.
fn coerce_int(value: &Value, min: i32, max: i32) -> Result<i32, CoerceFail> {
    if value.is_boolean() {
        return Err(CoerceFail::Msg(INVALID_INT_MSG.to_owned()));
    }
    let text = match value {
        Value::Number(n) => n.to_string(),
        Value::String(s) => {
            if s.len() > 1000 {
                return Err(CoerceFail::Msg("String value too large.".to_owned()));
            }
            s.clone()
        }
        _ => return Err(CoerceFail::Msg(INVALID_INT_MSG.to_owned())),
    };
    // `re_decimal = re.compile(r'\.0*\s*$')`: trailing `.0…` (+ spaces)
    // stripped before `int()`.
    let stripped = strip_decimal_suffix(&text);
    let parsed: i64 = stripped
        .trim()
        .parse()
        .map_err(|_| CoerceFail::Msg(INVALID_INT_MSG.to_owned()))?;
    if parsed < i64::from(min) {
        return Err(CoerceFail::Msg(min_value_msg(min)));
    }
    if parsed > i64::from(max) {
        return Err(CoerceFail::Msg(max_value_msg(max)));
    }
    i32::try_from(parsed).map_err(|_| CoerceFail::Msg(INVALID_INT_MSG.to_owned()))
}

fn strip_decimal_suffix(text: &str) -> String {
    let trimmed = text.trim_end();
    let bytes = trimmed.as_bytes();
    let mut end = bytes.len();
    // Strip trailing whitespace already done; strip `.0*` run.
    let mut i = end;
    while i > 0 && bytes[i - 1] == b'0' {
        i -= 1;
    }
    if i > 0 && bytes[i - 1] == b'.' && i != end {
        end = i - 1;
    }
    trimmed[..end].to_owned()
}

/// `PrimaryKeyRelatedField.to_internal_value` without `pk_field`
/// (`relations.py`, which is what `ModelSerializer` builds for these FKs):
/// bools/lists/dicts fail `incorrect_type`; anything else goes to
/// `queryset.get(pk=data)`, where unparseable input raises Django's
/// `ValidationError` (`"..." is not a valid UUID.`, curly quotes,
/// `db/models/fields/__init__.py`) — caught per-field by
/// `Serializer.to_internal_value` into a field error, *not* the
/// base-handler 400. Misses fail `does_not_exist` with the raw input
/// echoed.
async fn coerce_fk_user(
    pool: &PgPool,
    field: &str,
    value: &Value,
) -> Result<Option<uuid::Uuid>, CoerceFail> {
    if value.is_null() {
        return Ok(None);
    }
    if value.is_boolean() {
        return Err(CoerceFail::Msg(
            "Incorrect type. Expected pk value, received bool.".to_owned(),
        ));
    }
    if let Some(kind) = match value {
        Value::Array(_) => Some("list"),
        Value::Object(_) => Some("dict"),
        _ => None,
    } {
        return Err(CoerceFail::Msg(format!(
            "Incorrect type. Expected pk value, received {kind}."
        )));
    }
    let text = match value {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => {
            return Err(CoerceFail::Msg(
                "Incorrect type. Expected pk value, received str.".to_owned(),
            ));
        }
    };
    let parsed = match text.parse::<uuid::Uuid>() {
        Ok(id) => id,
        Err(_) => {
            return Err(CoerceFail::Msg(format!("“{text}” is not a valid UUID.")));
        }
    };
    // Each FK reads its own queryset: leads/assignees are users;
    // `default_state` goes through the default `StateManager` (triage
    // excluded, `db/models/state.py:82-83`); estimates use their plain
    // soft-delete manager.
    let exists: bool = match field {
        "default_state" => sqlx::query_scalar(
            r#"SELECT EXISTS(SELECT 1 FROM "states" WHERE "id" = $1 AND "group" != 'triage' AND "deleted_at" IS NULL)"#,
        )
        .bind(parsed)
        .fetch_one(pool)
        .await
        .map_err(|_| CoerceFail::Msg("Invalid pk.".to_owned()))?,
        "estimate" => sqlx::query_scalar(
            r#"SELECT EXISTS(SELECT 1 FROM "estimates" WHERE "id" = $1 AND "deleted_at" IS NULL)"#,
        )
        .bind(parsed)
        .fetch_one(pool)
        .await
        .map_err(|_| CoerceFail::Msg("Invalid pk.".to_owned()))?,
        _ => sqlx::query_scalar(
            r#"SELECT EXISTS(SELECT 1 FROM "users" WHERE "id" = $1 AND "is_active")"#,
        )
        .bind(parsed)
        .fetch_one(pool)
        .await
        .map_err(|_| CoerceFail::Msg("Invalid pk.".to_owned()))?,
    };
    if !exists {
        return Err(CoerceFail::Msg(format!(
            "Invalid pk \"{text}\" - object does not exist."
        )));
    }
    Ok(Some(parsed))
}

/// `JSONField.to_internal_value` (non-binary): any JSON value passes as-is
/// (strings must already be parsed — the transport gives values, and
/// `json.dumps` round-trips everything JSON holds).
fn coerce_json(value: &Value) -> Result<Value, CoerceFail> {
    match serde_json::to_string(value) {
        Ok(_) => Ok(value.clone()),
        Err(_) => Err(CoerceFail::Msg(INVALID_JSON_MSG.to_owned())),
    }
}

/// `ChoiceField.to_internal_value`: `str(data)` looked up in the choice
/// strings; misses echo the raw input.
fn coerce_choice(value: &Value, choices: &[&str]) -> Result<String, CoerceFail> {
    let text = match value {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => {
            if *b {
                "True".to_owned()
            } else {
                "False".to_owned()
            }
        }
        _ => {
            return Err(CoerceFail::Msg(invalid_choice_msg(&render_input(value))));
        }
    };
    if choices.contains(&text.as_str()) {
        Ok(text)
    } else {
        Err(CoerceFail::Msg(invalid_choice_msg(&text)))
    }
}

fn render_input(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Null => "None".to_owned(),
        _ => value.to_string(),
    }
}

/// Validated write data: one entry per writable field that was present
/// (partial updates) or defaulted (creates).
#[derive(Debug, Default, Clone)]
pub struct WriteData {
    pub fields: HashMap<String, FieldValue>,
}

/// `ProjectCreateSerializer.Meta.fields` in source order
/// (`serializers/project.py:100-127`); field errors render in this order.
pub const CREATE_FIELD_ORDER: &[&str] = &[
    "name",
    "description",
    "project_lead",
    "default_assignee",
    "identifier",
    "icon_prop",
    "emoji",
    "cover_image",
    "module_view",
    "cycle_view",
    "issue_views_view",
    "page_view",
    "intake_view",
    "guest_view_all_features",
    "members_can_edit_states",
    "archive_in",
    "close_in",
    "timezone",
    "external_source",
    "external_id",
    "is_issue_type_enabled",
    "is_time_tracking_enabled",
    "is_default",
    "repo_url",
    "base_branch",
    "default_agent_executor",
];

/// `ProjectUpdateSerializer` extra writable fields
/// (`serializers/project.py:207-210`), validated after the create fields.
pub const UPDATE_EXTRA_ORDER: &[&str] = &["default_state", "estimate"];

/// Coerce one writable field. `partial` skips absent fields (PATCH);
/// creates fail them when required. Returns the Django-branch signal for
/// FK garbage separately so the caller answers the base-handler 400.
async fn coerce_field(
    pool: &PgPool,
    field: &str,
    input: &serde_json::Map<String, Value>,
) -> Result<Option<FieldValue>, CoerceFail> {
    let raw = input.get(field);
    // Read-only fields are dropped from input (`Meta.read_only_fields`);
    // unknown fields are ignored by `ModelSerializer`.
    let Some(value) = raw else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(Some(match field {
            "project_lead" | "default_assignee" | "default_state" | "estimate" => {
                FieldValue::Fk(None)
            }
            "icon_prop" => FieldValue::Json(Value::Null),
            "emoji" | "cover_image" | "external_source" | "external_id" => FieldValue::OptStr(None),
            _ => return Err(CoerceFail::Msg(NULL_MSG.to_owned())),
        }));
    }
    Ok(Some(match field {
        "name" => FieldValue::Str(coerce_char(value, false, Some(255))?),
        "identifier" => FieldValue::Str(coerce_char(value, false, Some(12))?),
        "description" => FieldValue::Str(coerce_char(value, true, None)?),
        "project_lead" | "default_assignee" | "default_state" | "estimate" => {
            FieldValue::Fk(coerce_fk_user(pool, field, value).await?)
        }
        "icon_prop" => FieldValue::Json(coerce_json(value)?),
        "emoji" | "external_source" | "external_id" => {
            FieldValue::OptStr(Some(coerce_char(value, true, Some(255))?))
        }
        "cover_image" => FieldValue::OptStr(Some(coerce_char(value, true, None)?)),
        "module_view"
        | "cycle_view"
        | "issue_views_view"
        | "page_view"
        | "intake_view"
        | "guest_view_all_features"
        | "is_issue_type_enabled"
        | "is_time_tracking_enabled"
        | "is_default"
        | "members_can_edit_states" => FieldValue::Bool(coerce_bool(value)?),
        "archive_in" | "close_in" => FieldValue::Int(coerce_int(value, 0, 12)?),
        "timezone" => FieldValue::Str({
            let text = coerce_char(value, false, Some(255))?;
            coerce_choice(
                &Value::String(text.clone()),
                super::tz_zones::PYTZ_COMMON_TIMEZONES,
            )
            .map(|_| text)?
        }),
        "repo_url" => FieldValue::Str(coerce_char(value, true, Some(512))?),
        "base_branch" => {
            let text = coerce_char(value, true, Some(128))?;
            if !text
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '/' || c == '-')
            {
                return Err(CoerceFail::Msg(
                    "Branch name may contain only letters, numbers, and . _ / -".to_owned(),
                ));
            }
            FieldValue::Str(text)
        }
        "default_agent_executor" => FieldValue::Str(coerce_choice(
            value,
            &["local_runner", "cloud_agent", "managed_runner"],
        )?),
        _ => return Ok(None),
    }))
}

/// Run field coercion over the write fields in declaration order,
/// collecting the first failure per field (`serializers/project.py` field
/// machinery). Missing required fields fail `required` on creates.
pub async fn coerce_write(
    pool: &PgPool,
    input: &serde_json::Map<String, Value>,
    partial: bool,
    for_update: bool,
) -> Result<WriteData, Denial> {
    let mut errors: Vec<(String, Vec<String>)> = Vec::new();
    let mut data = WriteData::default();
    let mut order: Vec<&str> = CREATE_FIELD_ORDER.to_vec();
    if for_update {
        order.extend_from_slice(UPDATE_EXTRA_ORDER);
    }
    for field in order {
        // `default_state`/`estimate` exist only on the update serializer.
        if !for_update && (field == "default_state" || field == "estimate") {
            continue;
        }
        let present = input.contains_key(field);
        if !present {
            if !partial && (field == "name" || field == "identifier") {
                errors.push((field.to_owned(), vec![REQUIRED_MSG.to_owned()]));
            }
            continue;
        }
        match coerce_field(pool, field, input).await {
            Ok(Some(value)) => {
                data.fields.insert(field.to_owned(), value);
            }
            Ok(None) => {}
            Err(CoerceFail::Msg(message)) => {
                errors.push((field.to_owned(), vec![message]));
            }
        }
    }
    if !errors.is_empty() {
        let mut map = serde_json::Map::with_capacity(errors.len());
        for (field, messages) in errors {
            map.insert(
                field,
                Value::Array(messages.into_iter().map(Value::String).collect()),
            );
        }
        let body = serde_json::to_string(&Value::Object(map)).map_err(|_| Denial::ServerError)?;
        return Err(Denial::FieldErrors(body));
    }
    Ok(data)
}

// ---------------------------------------------------------------------------
// Shared handler plumbing
// ---------------------------------------------------------------------------

/// Request preamble: pool, actor and optional workspace id. An unknown slug
/// yields `None`: every gate then denies 403, exactly like the
/// slug-filtered membership checks missing in Python (the create path's
/// `Workspace.DoesNotExist` 404 is unreachable for the same reason, and is
/// still ported where Python has it).
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
            .map_err(|_| Denial::ServerError)?
            .flatten();
    Ok(Preamble {
        pool,
        actor,
        workspace_id,
    })
}

/// `base_host(request, is_app=True)` (`utils/host.py:17-67`): the app
/// origin for task kwargs, never the inbound host.
pub fn app_origin(state: &AppState) -> String {
    if let Some(url) = state.settings().urls.app_base_url.clone() {
        return url;
    }
    if let Some(url) = state.settings().urls.web_url.clone() {
        return url;
    }
    "http://localhost".to_owned()
}

/// Best-effort post-commit task fan-out (the `.delay()` calls): without a
/// queue table the response still stands (same precedent as the space
/// intake handlers).
pub async fn enqueue_best_effort(
    pool: &PgPool,
    task: &str,
    kwargs: serde_json::Map<String, Value>,
) {
    let message = pidash_jobs::celery::CeleryTaskMessage::new(task, vec![], kwargs);
    let job = pidash_jobs::queue::NewJob::new(
        message.task.clone(),
        Value::Array(message.args.clone()),
        Value::Object(message.kwargs.clone()),
    );
    if let Err(error) = pidash_jobs::queue::enqueue(pool, &job).await {
        tracing::warn!(%error, task, "task enqueue failed; response stands");
    }
}

/// `POST .../<project_id>/archive/` (`views/project.py:528-538`): stamp
/// `archived_at`, soft-delete the workspace's project favorites, answer
/// 204. Misses raise `DoesNotExist` into the base-handler 404.
pub async fn archive_project_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    project_id_raw: &str,
) -> Result<Response, Denial> {
    let pre = preamble(state, headers, slug).await?;
    let workspace_id = pre.workspace_id.ok_or(Denial::Forbidden)?;
    let project_id = rewrite_project_id(&pre.pool, slug, project_id_raw).await?;
    // Archive is a POST: the workspace admin-or-member branch.
    require_project_base(
        &pre.pool,
        &workspace_id,
        slug,
        &pre.actor.id,
        Some(&project_id),
        "POST",
    )
    .await?;
    // Gate passed: activate the stored zone now (`TimezoneMixin.initial`
    // runs after permissions; an unknown zone 400s only for survivors).
    activate_timezone(pre.actor.timezone.as_deref())?;
    let now = chrono::Utc::now();
    let updated: u64 = sqlx::query(
        r#"UPDATE "projects" SET "archived_at" = $1, "updated_at" = $1, "updated_by_id" = $2 WHERE "id" = $3 AND "workspace_id" = $4 AND "deleted_at" IS NULL"#,
    )
    .bind(now)
    .bind(pre.actor.id)
    .bind(project_id)
    .bind(workspace_id)
    .execute(&pre.pool)
    .await
    .map_err(|_| Denial::ServerError)?
    .rows_affected();
    if updated == 0 {
        return Err(Denial::NotFound);
    }
    // `UserFavorite` cleanup (`views/project.py:537`): every favorite of
    // the project in this workspace soft-deletes (no `entity_type`
    // predicate on this path, exactly as written).
    sqlx::query(
        r#"UPDATE "user_favorites" SET "deleted_at" = $1 WHERE "workspace_id" = $2 AND "project_id" = $3 AND "deleted_at" IS NULL"#,
    )
    .bind(now)
    .bind(workspace_id)
    .bind(project_id)
    .execute(&pre.pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(no_content())
}

/// `DELETE .../<project_id>/archive/` (`views/project.py:552-561`):
/// clear `archived_at`, answer 204.
pub async fn unarchive_project_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    project_id_raw: &str,
) -> Result<Response, Denial> {
    let pre = preamble(state, headers, slug).await?;
    let workspace_id = pre.workspace_id.ok_or(Denial::Forbidden)?;
    let project_id = rewrite_project_id(&pre.pool, slug, project_id_raw).await?;
    require_project_base(
        &pre.pool,
        &workspace_id,
        slug,
        &pre.actor.id,
        Some(&project_id),
        "DELETE",
    )
    .await?;
    // Gate passed: activate the stored zone now (`TimezoneMixin.initial`
    // runs after permissions; an unknown zone 400s only for survivors).
    activate_timezone(pre.actor.timezone.as_deref())?;
    let now = chrono::Utc::now();
    let updated: u64 = sqlx::query(
        r#"UPDATE "projects" SET "archived_at" = NULL, "updated_at" = $1, "updated_by_id" = $2 WHERE "id" = $3 AND "workspace_id" = $4 AND "deleted_at" IS NULL"#,
    )
    .bind(now)
    .bind(pre.actor.id)
    .bind(project_id)
    .bind(workspace_id)
    .execute(&pre.pool)
    .await
    .map_err(|_| Denial::ServerError)?
    .rows_affected();
    if updated == 0 {
        return Err(Denial::NotFound);
    }
    Ok(no_content())
}

/// Allowed `?fields=` names for the summary (`views/project.py:564-573`),
/// in source order (the response dict follows this order; Python builds it
/// from a set, so its order varies per process).
pub const ALLOWED_SUMMARY_FIELDS: &[&str] = &[
    "members", "states", "labels", "cycles", "modules", "issues", "intakes", "pages",
];

/// `GET .../<project_id>/summary/` (`views/project.py:580-603`) with
/// `_get_all_summary_counts` (`:605-677`): one round trip with a
/// `Coalesce(Subquery, 0)` count per requested field.
pub async fn summary_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    project_id_raw: &str,
    query: &QueryMap,
) -> Result<Response, Denial> {
    let pre = preamble(state, headers, slug).await?;
    let workspace_id = pre.workspace_id.ok_or(Denial::Forbidden)?;
    // The slug->UUID rewrite runs in `initial()` before `check_permissions`
    // (`api/views/base.py:110`), so identifier misses 404 even for callers
    // the admin gate would deny.
    let project_id = rewrite_project_id(&pre.pool, slug, project_id_raw).await?;
    // `WorkSpaceAdminPermission`: workspace ADMIN or MEMBER, active.
    {
        use super::perms::{decide, gate_for, V1Route};
        use pidash_auth::permissions::project as kernel;
        let facts = workspace_admin_facts(&pre.pool, slug, &workspace_id, &pre.actor.id).await?;
        let scope = TenantScope::new(pidash_types::WorkspaceId::from(slug));
        let mutation = kernel::StateMutationFacts {
            authenticated: true,
            project_role: None,
            members_can_edit_states: false,
            is_workspace_admin: false,
        };
        // The summary route has no project kwarg for the gate; the facts
        // decide it alone.
        let ws_project_facts = kernel::ProjectFacts {
            workspace: pidash_types::WorkspaceId::from(slug),
            project_id: pidash_types::ProjectId::from(""),
            authenticated: true,
            is_workspace_member: facts.is_member,
            has_workspace_admin_or_member: facts.has_admin_or_member_role,
            is_workspace_admin: facts.has_admin_role,
            is_project_member: false,
            is_project_admin: false,
            has_project_admin_or_member: false,
            has_identifier_membership: false,
            has_project_identifier: false,
        };
        if !decide(
            gate_for(V1Route::ProjectSummary, "GET"),
            "GET",
            &scope,
            &ws_project_facts,
            &facts,
            &mutation,
        ) {
            return Err(Denial::Forbidden);
        }
    }
    // Gate passed: activate the stored zone now (`TimezoneMixin.initial`
    // runs after permissions; an unknown zone 400s only for survivors).
    activate_timezone(pre.actor.timezone.as_deref())?;
    let row: Option<(uuid::Uuid, String, String)> = sqlx::query_as(
        r#"SELECT "id", "name", "identifier" FROM "projects" WHERE "id" = $1 AND "workspace_id" = $2 AND "deleted_at" IS NULL"#,
    )
    .bind(project_id)
    .bind(workspace_id)
    .fetch_optional(&pre.pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let Some((id, name, identifier)) = row else {
        return Err(Denial::NotFoundError("Project not found".to_owned()));
    };
    // `?fields=` intersected with the allowed names; empty/unknown → all.
    let raw_fields = query_last(query, "fields").unwrap_or_default();
    let mut requested: Vec<&str> = raw_fields
        .split(',')
        .map(str::trim)
        .filter(|f| ALLOWED_SUMMARY_FIELDS.contains(f))
        .collect();
    if requested.is_empty() {
        requested = ALLOWED_SUMMARY_FIELDS.to_vec();
    }
    let counts = summary_counts(&pre.pool, &project_id, &requested).await?;
    let mut counts_map = serde_json::Map::with_capacity(requested.len());
    for field in ALLOWED_SUMMARY_FIELDS.iter() {
        if let Some(value) = counts.get(*field) {
            counts_map.insert((*field).to_owned(), Value::from(*value));
        }
    }
    let mut body = serde_json::Map::with_capacity(4);
    body.insert("id".to_owned(), render_uuid(id));
    body.insert("name".to_owned(), Value::String(name));
    body.insert("identifier".to_owned(), Value::String(identifier));
    body.insert("counts".to_owned(), Value::Object(counts_map));
    let text = serde_json::to_string(&Value::Object(body)).map_err(|_| Denial::ServerError)?;
    Ok(json_ok(text))
}

/// One-round-trip summary counts (`_get_all_summary_counts`): a
/// `COALESCE(subquery, 0)` column per requested field. Soft-deleted rows
/// never count (the default managers); issues exclude the triage
/// state-group (`views/project.py:643-649`).
pub async fn summary_counts(
    pool: &PgPool,
    project_id: &uuid::Uuid,
    requested: &[&str],
) -> Result<HashMap<String, i64>, Denial> {
    fn subquery(field: &str) -> &'static str {
        match field {
            "members" => {
                r#"(SELECT COUNT(*) FROM "project_members" WHERE "project_id" = $1 AND "is_active" AND "deleted_at" IS NULL)"#
            }
            "states" => {
                r#"(SELECT COUNT(*) FROM "states" WHERE "project_id" = $1 AND "deleted_at" IS NULL)"#
            }
            "labels" => {
                r#"(SELECT COUNT(*) FROM "labels" WHERE "project_id" = $1 AND "deleted_at" IS NULL)"#
            }
            "cycles" => {
                r#"(SELECT COUNT(*) FROM "cycles" WHERE "project_id" = $1 AND "deleted_at" IS NULL)"#
            }
            "modules" => {
                r#"(SELECT COUNT(*) FROM "modules" WHERE "project_id" = $1 AND "deleted_at" IS NULL)"#
            }
            "issues" => {
                r#"(SELECT COUNT(*) FROM "issues" i JOIN "states" s ON s."id" = i."state_id" WHERE i."project_id" = $1 AND i."deleted_at" IS NULL AND s."group" != 'triage')"#
            }
            "intakes" => {
                r#"(SELECT COUNT(*) FROM "intake_issues" WHERE "project_id" = $1 AND "deleted_at" IS NULL)"#
            }
            "pages" => {
                r#"(SELECT COUNT(*) FROM "project_pages" WHERE "project_id" = $1 AND "deleted_at" IS NULL)"#
            }
            _ => "(SELECT 0)",
        }
    }
    // Annotation alias is `pages_count` for `pages` (the `Project.pages`
    // M2M clash, `views/project.py:608-610`).
    fn alias(field: &str) -> &str {
        if field == "pages" {
            "pages_count"
        } else {
            field
        }
    }
    let mut ordered: Vec<&str> = ALLOWED_SUMMARY_FIELDS
        .iter()
        .filter(|f| requested.contains(f))
        .copied()
        .collect();
    ordered.sort();
    let select: Vec<String> = ordered
        .iter()
        .map(|f| format!("COALESCE({}, 0) AS \"{}\"", subquery(f), alias(f)))
        .collect();
    if select.is_empty() {
        return Ok(HashMap::new());
    }
    let sql = format!("SELECT {}", select.join(", "));
    let row: sqlx::postgres::PgRow = sqlx::query(&sql)
        .bind(project_id)
        .fetch_one(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let mut out = HashMap::with_capacity(ordered.len());
    for field in ordered {
        let value: i64 = row.try_get(alias(field)).map_err(|_| Denial::ServerError)?;
        out.insert(field.to_owned(), value);
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Axum entry points
// ---------------------------------------------------------------------------

/// `GET workspaces/<slug>/projects/`.
pub async fn list_projects(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(slug): Path<String>,
    Query(query): Query<QueryMap>,
) -> Response {
    match list_projects_inner(&state, &headers, &slug, &query, &[]).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

/// `POST workspaces/<slug>/projects/`.
pub async fn create_project(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(slug): Path<String>,
    body: axum::body::Bytes,
) -> Response {
    match create_project_inner(&state, &headers, &slug, &body).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

/// `GET workspaces/<slug>/projects/<pk>/`.
pub async fn retrieve_project(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((slug, pk)): Path<(String, String)>,
    Query(query): Query<QueryMap>,
) -> Response {
    match retrieve_project_inner(&state, &headers, &slug, &pk, &query).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

/// `PATCH workspaces/<slug>/projects/<pk>/`.
pub async fn patch_project(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((slug, pk)): Path<(String, String)>,
    body: axum::body::Bytes,
) -> Response {
    match patch_project_inner(&state, &headers, &slug, &pk, &body).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

/// `DELETE workspaces/<slug>/projects/<pk>/`.
pub async fn delete_project(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((slug, pk)): Path<(String, String)>,
) -> Response {
    match delete_project_inner(&state, &headers, &slug, &pk).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

/// `POST .../<project_id>/archive/`.
pub async fn archive_project(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((slug, project_id)): Path<(String, String)>,
    body: axum::body::Bytes,
) -> Response {
    let _ = body;
    match archive_project_inner(&state, &headers, &slug, &project_id).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

/// `DELETE .../<project_id>/archive/`.
pub async fn unarchive_project(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((slug, project_id)): Path<(String, String)>,
) -> Response {
    match unarchive_project_inner(&state, &headers, &slug, &project_id).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

/// `GET .../<project_id>/summary/`.
pub async fn summary(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((slug, project_id)): Path<(String, String)>,
    Query(query): Query<QueryMap>,
) -> Response {
    match summary_inner(&state, &headers, &slug, &project_id, &query).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

/// Soft-delete fan-out (`db/mixins.py:72-78`): instance `delete()` stamps
/// `deleted_at` + `save()` (so `updated_at`/`updated_by` move) and enqueues
/// `soft_delete_related_objects("db", <model>, <pk>, "default")`.
pub async fn enqueue_soft_delete(pool: &PgPool, model: &str, pk: &uuid::Uuid) {
    let message = pidash_jobs::celery::CeleryTaskMessage::new(
        pidash_jobs::tasks_cleanup::deletion::SOFT_DELETE_TASK,
        vec![
            Value::String("db".to_owned()),
            Value::String(model.to_owned()),
            Value::String(pk.to_string()),
            Value::String("default".to_owned()),
        ],
        Default::default(),
    );
    enqueue_best_effort(pool, &message.task.clone(), message.kwargs.clone()).await;
}

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

/// Envelope metadata for the 12-key `BasePaginator.paginate` body
/// (`utils/paginator.py:714-731`), keys in order via the kernel.
pub fn envelope(
    total_count: i64,
    per_page: i64,
    next: &crate::paginator::Cursor,
    prev: &crate::paginator::Cursor,
    results: Value,
) -> Result<Response, Denial> {
    use crate::paginator::{max_hits, PageResponse};
    // `max_hits = ceil(count / limit)` with the request `per_page`
    // (`utils/paginator.py:183`); `0` divides by zero into the 500.
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
    Ok(json_ok(body))
}

/// Validate the list-GET `order_by` key. Django resolves it against the
/// queryset (annotations included); unknown keys raise `FieldError` into
/// the generic 500. The eight queryset arms render through the queries
/// kernel; any other concrete project column orders directly.
pub fn resolve_list_order(key: &str) -> Result<String, Denial> {
    use pidash_db::v1_projects::queries_projmem as q;
    const ANNOTATIONS: &[&str] = &[
        "total_members",
        "total_cycles",
        "total_modules",
        "is_member",
        "sort_order",
        "member_role",
        "is_deployed",
    ];
    const KNOWN: &[&str] = &[
        "sort_order",
        "-sort_order",
        "created_at",
        "-created_at",
        "updated_at",
        "-updated_at",
        "name",
        "-name",
    ];
    if KNOWN.contains(&key) {
        return Ok(q::project_list_get_sql(key));
    }
    let (desc, name) = match key.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, key),
    };
    if ANNOTATIONS.contains(&name)
        || pidash_db::v1_projects::models::project::COLUMNS.contains(&name)
    {
        // The base statement already selects `p.*` plus every annotation;
        // Django's `.order_by(key)` only appends the clause.
        let direction = if desc { "DESC" } else { "ASC" };
        return Ok(format!(
            "{}\nORDER BY \"{name}\" {direction}",
            q::PROJECT_LIST_BASE_SQL
        ));
    }
    Err(Denial::ServerError)
}

/// Fetch one project row with the detail queryset (annotations, no
/// `sort_order`), or `None`.
pub async fn fetch_detail_row(
    pool: &PgPool,
    slug: &str,
    actor_id: &uuid::Uuid,
    project_id: &uuid::Uuid,
) -> Result<Option<sqlx::postgres::PgRow>, Denial> {
    use pidash_db::v1_projects::queries_projmem as q;
    // NB: the detail base ends with `GROUP BY p.id, w.id` (and
    // `project_detail_sql()` appends `ORDER BY`), so the pk predicate must
    // be spliced into the WHERE clause — appending AND after GROUP BY is a
    // boolean-type error (`w.id AND ...`).
    let sql = format!(
        "{}\n{}",
        q::PROJECT_DETAIL_BASE_SQL.replacen(
            "GROUP BY p.id, w.id",
            "AND p.id = $3 GROUP BY p.id, w.id",
            1
        ),
        q::ORDER_BASE
    );
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(&sql)
        .bind(slug)
        .bind(actor_id)
        .bind(project_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(row)
}

pub fn detail_annotations(row: &sqlx::postgres::PgRow) -> Result<ReadAnnotations, Denial> {
    use pidash_db::v1_projects::queries_projmem as q;
    let mapped =
        q::map_project_annotations(row).map_err(|error| db_error(error, "map-annotations"))?;
    Ok(ReadAnnotations {
        total_members: mapped.total_members,
        total_cycles: mapped.total_cycles,
        total_modules: mapped.total_modules,
        is_member: mapped.is_member,
        sort_order: None,
        member_role: mapped.member_role,
        is_deployed: mapped.is_deployed,
    })
}

// ---------------------------------------------------------------------------
// GET + POST `workspaces/<slug>/projects/`
// ---------------------------------------------------------------------------

/// `GET workspaces/<slug>/projects/` (`views/project.py:163-194`): the
/// member-scoped queryset with the `sort_order` annotation, ordered by
/// `?order_by=` (default `sort_order`), paginated through the shared
/// envelope with `ProjectSerializer(many, fields, expand)` rows.
pub async fn list_projects_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    query: &QueryMap,
    body: &[u8],
) -> Result<Response, Denial> {
    let _ = body;
    let pre = preamble(state, headers, slug).await?;
    let workspace_id = pre.workspace_id.ok_or(Denial::Forbidden)?;
    require_project_base(&pre.pool, &workspace_id, slug, &pre.actor.id, None, "GET").await?;
    // Gate passed: activate the stored zone now (`TimezoneMixin.initial`
    // runs after permissions; an unknown zone 400s only for survivors).
    let timezone = activate_timezone(pre.actor.timezone.as_deref())?;
    let per_page =
        crate::paginator::parse_per_page(query_last(query, "per_page").as_deref(), 1000, 1000)
            .map_err(page_denial)?;
    let cursor_raw = query_last(query, "cursor").unwrap_or_else(|| format!("{per_page}:0:0"));
    let cursor = crate::paginator::Cursor::from_string(&cursor_raw).map_err(page_denial)?;
    let order_key = query_last(query, "order_by").unwrap_or_else(|| "sort_order".to_owned());
    let sql = resolve_list_order(&order_key)?;
    let window = crate::paginator::offset_window(
        per_page,
        cursor.offset,
        cursor.value,
        cursor.is_prev,
        None,
    )
    .map_err(page_denial)?;
    let rows: Vec<sqlx::postgres::PgRow> = sqlx::query(&sql)
        .bind(slug)
        .bind(pre.actor.id)
        .fetch_all(&pre.pool)
        .await
        .map_err(|_| Denial::ServerError)?;
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
    // `results[:limit]` over the evaluated window (negative limits already
    // errored in `offset_window`).
    let trim = usize::try_from(per_page)
        .unwrap_or(usize::MAX)
        .min(window_rows.len());
    let page_rows = &window_rows[..trim];
    let fields = fields_param(query, "fields");
    let expand = fields_param(query, "expand");
    let mut rendered: Vec<Value> = Vec::with_capacity(page_rows.len());
    for row in page_rows {
        use pidash_db::v1_projects::queries_projmem as q;
        let mapped =
            q::map_project_annotations(row).map_err(|error| db_error(error, "map-annotations"))?;
        let ann = ReadAnnotations {
            total_members: mapped.total_members,
            total_cycles: mapped.total_cycles,
            total_modules: mapped.total_modules,
            is_member: mapped.is_member,
            sort_order: mapped.sort_order,
            member_role: mapped.member_role,
            is_deployed: mapped.is_deployed,
        };
        let text = render_project(
            &pre.pool,
            row,
            Some(&ann),
            true,
            &timezone,
            fields.as_deref(),
            expand.as_deref(),
        )
        .await?;
        rendered.push(serde_json::from_str(&text).map_err(|_| Denial::ServerError)?);
    }
    let next = crate::paginator::next_cursor(per_page, cursor.offset, has_more);
    let prev = crate::paginator::prev_cursor(per_page, cursor.offset);
    envelope(total_count, per_page, &next, &prev, Value::Array(rendered))
}

/// Map a serializer-layer failure to its HTTP shape on the create/update
/// paths: the `validate()` phase answers the error's own status/body;
/// failures raised inside `create()`/`update()` are swallowed by the
/// view's `except ValidationError` into the identifier-taken 409
/// (`views/project.py:280-284,463-467` — the ported BUG-2 surface).
pub fn ser_denial(error: pidash_services::v1_projects::ser_project::ProjectSerError) -> Denial {
    match error.status() {
        409 => Denial::Conflict(error.body().to_owned()),
        _ => Denial::FieldErrors(error.body().to_owned()),
    }
}

/// `WorkspaceMember` existence for the lead/assignee checks
/// (`serializers/project.py:152-156,160-164`): filtered on
/// `(workspace_id, member_id)` with no `is_active` predicate, exactly as
/// written.
pub async fn workspace_has_member(
    pool: &PgPool,
    workspace_id: &uuid::Uuid,
    member_id: &uuid::Uuid,
) -> Result<bool, Denial> {
    let exists: bool = sqlx::query_scalar(
        r#"SELECT EXISTS(SELECT 1 FROM "workspace_members" WHERE "workspace_id" = $1 AND "member_id" = $2 AND "deleted_at" IS NULL)"#,
    )
    .bind(workspace_id)
    .bind(member_id)
    .fetch_one(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(exists)
}

fn field_str(data: &WriteData, field: &str) -> Option<String> {
    match data.fields.get(field) {
        Some(FieldValue::Str(s)) => Some(s.clone()),
        _ => None,
    }
}

fn field_opt_str(data: &WriteData, field: &str) -> Option<Option<String>> {
    match data.fields.get(field) {
        Some(FieldValue::OptStr(v)) => Some(v.clone()),
        _ => None,
    }
}

fn field_bool(data: &WriteData, field: &str) -> Option<bool> {
    match data.fields.get(field) {
        Some(FieldValue::Bool(b)) => Some(*b),
        _ => None,
    }
}

fn field_int(data: &WriteData, field: &str) -> Option<i32> {
    match data.fields.get(field) {
        Some(FieldValue::Int(v)) => Some(*v),
        _ => None,
    }
}

fn field_fk(data: &WriteData, field: &str) -> Option<Option<uuid::Uuid>> {
    match data.fields.get(field) {
        Some(FieldValue::Fk(v)) => Some(*v),
        _ => None,
    }
}

fn field_json(data: &WriteData, field: &str) -> Option<Value> {
    match data.fields.get(field) {
        Some(FieldValue::Json(v)) => Some(v.clone()),
        _ => None,
    }
}

/// Default `view_props`/`default_props` document
/// (`db/models/project.py:43-65`, `get_default_props`).
pub fn default_props_json() -> Value {
    serde_json::json!({
        "filters": {
            "priority": null, "state": null, "state_group": null,
            "assignees": null, "created_by": null, "labels": null,
            "start_date": null, "target_date": null, "subscriber": null
        },
        "display_filters": {
            "group_by": null, "order_by": "-created_at", "type": null,
            "sub_issue": true, "show_empty_groups": true,
            "layout": "list", "calendar_date_range": ""
        }
    })
}

/// Default `preferences` document (`get_default_preferences`,
/// `db/models/project.py:66-67`).
pub fn default_preferences_json() -> Value {
    serde_json::json!({
        "pages": {"block_display": true},
        "navigation": {"default_tab": "work_items", "hide_in_more_menu": []}
    })
}

/// `ProjectUserProperty` JSON defaults (`db/models/issue.py:50-88`).
pub fn user_property_defaults() -> (Value, Value, Value) {
    (
        serde_json::json!({
            "priority": null, "state": null, "state_group": null,
            "assignees": null, "created_by": null, "labels": null,
            "start_date": null, "target_date": null, "subscriber": null
        }),
        serde_json::json!({
            "group_by": null, "order_by": "-created_at", "type": null,
            "sub_issue": true, "show_empty_groups": true,
            "layout": "list", "calendar_date_range": ""
        }),
        serde_json::json!({
            "assignee": true, "attachment_count": true, "created_on": true,
            "due_date": true, "estimate": true, "key": true, "labels": true,
            "link": true, "priority": true, "start_date": true, "state": true,
            "sub_issue": true, "updated_on": true
        }),
    )
}

/// Pick a pseudo-random logo icon+color (`serializers/project.py:177-185`
/// uses `random.choice`; values are unobservable beyond shape, so a
/// time-seeded pick keeps the bytes valid without a `rand` dependency).
pub fn random_logo_props() -> Value {
    use pidash_services::v1_projects::ser_project as ser;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as usize)
        .unwrap_or(0);
    let icons = ser::PROJECT_ICON_DEFAULT_ICONS;
    let colors = ser::PROJECT_ICON_DEFAULT_COLORS;
    let icon = icons[(nanos / 7) % icons.len().max(1)];
    let color = colors[nanos % colors.len().max(1)];
    serde_json::to_value(ser::logo_props_default(icon, color)).unwrap_or(Value::Null)
}

/// `POST workspaces/<slug>/projects/` (`views/project.py:214-285`).
pub async fn create_project_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    raw_body: &[u8],
) -> Result<Response, Denial> {
    use pidash_services::v1_projects::ser_project as ser;
    let pre = preamble(state, headers, slug).await?;
    let workspace_id = pre.workspace_id.ok_or(Denial::Forbidden)?;
    require_project_base(&pre.pool, &workspace_id, slug, &pre.actor.id, None, "POST").await?;
    // Gate passed: activate the stored zone now (`TimezoneMixin.initial`
    // runs after permissions; an unknown zone 400s only for survivors).
    // (Named `render_tz`: the write body below has its own `timezone`.)
    let render_tz = activate_timezone(pre.actor.timezone.as_deref())?;
    // `Workspace.objects.get(slug=slug)` — unreachable after a passing
    // permission (membership implies the row), still ported.
    let ws_exists: bool =
        sqlx::query_scalar(r#"SELECT EXISTS(SELECT 1 FROM "workspaces" WHERE "id" = $1)"#)
            .bind(workspace_id)
            .fetch_one(&pre.pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    if !ws_exists {
        return Err(Denial::NotFoundError("Workspace does not exist".to_owned()));
    }
    let input = parse_body(raw_body)?;
    let data = coerce_write(&pre.pool, &input, false, false).await?;
    // `validate()` (`serializers/project.py:139-166`).
    let name = field_str(&data, "name");
    let identifier_raw = field_str(&data, "identifier");
    let executor = field_str(&data, "default_agent_executor");
    let lead = field_fk(&data, "project_lead").unwrap_or(None);
    let assignee = field_fk(&data, "default_assignee").unwrap_or(None);
    let lead_member = match lead {
        Some(id) => Some(workspace_has_member(&pre.pool, &workspace_id, &id).await?),
        None => None,
    };
    let assignee_member = match assignee {
        Some(id) => Some(workspace_has_member(&pre.pool, &workspace_id, &id).await?),
        None => None,
    };
    let shared = ser::SharedChecks {
        executor: executor.as_deref(),
        cloud_configured: state.settings().cloud_agent.enabled,
        managed_enabled: state.settings().managed_runner.enabled,
        name: name.as_deref(),
        identifier: identifier_raw.as_deref(),
        project_lead_is_member: lead_member,
        default_assignee_is_member: assignee_member,
    };
    ser::validate_shared(&shared).map_err(ser_denial)?;
    // `create()` (`serializers/project.py:168-194`).
    let identifier = ser::normalize_identifier(identifier_raw.as_deref().unwrap_or(""));
    ser::require_identifier(&identifier).map_err(ser_denial)?;
    let ident_taken: bool = sqlx::query_scalar(
        r#"SELECT EXISTS(SELECT 1 FROM "project_identifiers" WHERE "name" = $1 AND "workspace_id" = $2 AND "deleted_at" IS NULL)"#,
    )
    .bind(&identifier)
    .bind(workspace_id)
    .fetch_one(&pre.pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    ser::check_identifier_taken(ident_taken).map_err(ser_denial)?;
    let logo_props = field_json(&data, "logo_props").unwrap_or_else(random_logo_props);
    let requested_default = field_bool(&data, "is_default").unwrap_or(false);
    let ws_tz: Option<String> =
        sqlx::query_scalar(r#"SELECT "timezone" FROM "workspaces" WHERE "id" = $1"#)
            .bind(workspace_id)
            .fetch_optional(&pre.pool)
            .await
            .map_err(|_| Denial::ServerError)?
            .flatten();
    let timezone = field_str(&data, "timezone")
        .or(ws_tz)
        .unwrap_or_else(|| "UTC".to_owned());
    let default_executor = {
        let candidate = field_str(&data, "default_agent_executor")
            .unwrap_or_else(|| state.settings().default_agent_executor.clone());
        if ["local_runner", "cloud_agent", "managed_runner"].contains(&candidate.as_str()) {
            candidate
        } else {
            "local_runner".to_owned()
        }
    };
    // `save()` auto-default (`db/models/project.py:261-270`): the first
    // project of a workspace becomes default.
    let has_default: bool = sqlx::query_scalar(
        r#"SELECT EXISTS(SELECT 1 FROM "projects" WHERE "workspace_id" = $1 AND "is_default" AND "deleted_at" IS NULL)"#,
    )
    .bind(workspace_id)
    .fetch_one(&pre.pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let is_default = requested_default || !has_default;
    let now = chrono::Utc::now();
    let project_id = uuid::Uuid::new_v4();
    let description = field_str(&data, "description").unwrap_or_default();
    // `transaction.atomic()`: unset-others + insert (`:187-194`, plus the
    // `save()` unset arm `:277-284`).
    let mut tx = pre.pool.begin().await.map_err(|_| Denial::ServerError)?;
    if ser::unsets_other_defaults(Some(requested_default)) || is_default {
        sqlx::query(
            r#"UPDATE "projects" SET "is_default" = FALSE WHERE "workspace_id" = $1 AND "is_default" AND "id" != $2 AND "deleted_at" IS NULL"#,
        )
        .bind(workspace_id)
        .bind(project_id)
        .execute(&mut *tx)
        .await
        .map_err(|_| Denial::ServerError)?;
    }
    let insert = sqlx::query(
        r#"INSERT INTO "projects" ("id", "created_at", "updated_at", "created_by_id", "updated_by_id",
            "name", "description", "description_text", "description_html", "network", "workspace_id",
            "identifier", "default_assignee_id", "project_lead_id", "emoji", "icon_prop",
            "module_view", "cycle_view", "issue_views_view", "page_view", "intake_view",
            "is_time_tracking_enabled", "is_issue_type_enabled", "is_default",
            "guest_view_all_features", "members_can_edit_states", "cover_image", "cover_image_asset_id",
            "estimate_id", "archive_in", "close_in", "logo_props", "default_state_id", "timezone",
            "external_source", "external_id", "repo_url", "base_branch",
            "agent_default_interval_seconds", "agent_default_max_ticks",
            "agent_review_default_interval_seconds", "agent_test_default_interval_seconds",
            "agent_ticking_enabled", "default_agent_executor")
         VALUES ($1,$2,$3,$4,NULL,$5,$6,NULL,NULL,2,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22,$23,NULL,NULL,$24,$25,$26,NULL,$27,$28,$29,$30,$31,10800,10,10800,10800,TRUE,$32)"#,
    )
    .bind(project_id)
    .bind(now)
    .bind(now)
    .bind(pre.actor.id)
    .bind(name.clone().unwrap_or_default())
    .bind(description)
    .bind(workspace_id)
    .bind(&identifier)
    .bind(assignee)
    .bind(lead)
    .bind(field_opt_str(&data, "emoji").unwrap_or(None))
    .bind(field_json(&data, "icon_prop").unwrap_or(Value::Null))
    .bind(field_bool(&data, "module_view").unwrap_or(false))
    .bind(field_bool(&data, "cycle_view").unwrap_or(false))
    .bind(field_bool(&data, "issue_views_view").unwrap_or(false))
    .bind(field_bool(&data, "page_view").unwrap_or(true))
    .bind(field_bool(&data, "intake_view").unwrap_or(false))
    .bind(field_bool(&data, "is_time_tracking_enabled").unwrap_or(false))
    .bind(field_bool(&data, "is_issue_type_enabled").unwrap_or(false))
    .bind(is_default)
    .bind(field_bool(&data, "guest_view_all_features").unwrap_or(false))
    .bind(field_bool(&data, "members_can_edit_states").unwrap_or(true))
    .bind(field_opt_str(&data, "cover_image").unwrap_or(None))
    .bind(field_int(&data, "archive_in").unwrap_or(0))
    .bind(field_int(&data, "close_in").unwrap_or(0))
    .bind(logo_props)
    .bind(&timezone)
    .bind(field_opt_str(&data, "external_source").unwrap_or(None))
    .bind(field_opt_str(&data, "external_id").unwrap_or(None))
    .bind(field_str(&data, "repo_url").unwrap_or_default())
    .bind(field_str(&data, "base_branch").unwrap_or_else(|| "main".to_owned()))
    .bind(&default_executor);
    let insert_result = insert.execute(&mut *tx).await;
    match insert_result {
        Ok(_) => {}
        Err(sqlx::Error::Database(db)) if db.code().as_deref() == Some("23505") => {
            // `DETAIL: Key (...) already exists.` — the 409 name body,
            // which also swallows identifier clashes (BUG-1).
            return Err(Denial::Conflict(ser::NAME_TAKEN_BODY.to_owned()));
        }
        Err(_) => return Err(Denial::ServerError),
    }
    tx.commit().await.map_err(|_| Denial::ServerError)?;
    // Creator (+ differing lead) become project admins
    // (`views/project.py:228-238`); each membership stamps its
    // `ProjectUserProperty` row (`db/models/project.py:348-364`).
    add_project_admin(
        &pre.pool,
        &workspace_id,
        &project_id,
        &pre.actor.id,
        &pre.actor.id,
    )
    .await?;
    if let Some(lead_id) = lead {
        if lead_id != pre.actor.id {
            add_project_admin(
                &pre.pool,
                &workspace_id,
                &project_id,
                &lead_id,
                &pre.actor.id,
            )
            .await?;
        }
    }
    // The 8 `DEFAULT_STATES` (`views/project.py:240-254`, bypassing
    // `save()` — slugs still slugified per row).
    for (name, color, sequence, group, default) in
        pidash_db::v1_projects::models::state::DEFAULT_STATES.iter()
    {
        insert_default_state(
            &pre.pool,
            &project_id,
            &workspace_id,
            &pre.actor.id,
            name,
            color,
            *sequence,
            group,
            *default,
            &now,
        )
        .await?;
    }
    let row = fetch_detail_row(&pre.pool, slug, &pre.actor.id, &project_id)
        .await?
        .ok_or(Denial::ServerError)?;
    let ann = detail_annotations(&row)?;
    // `model_activity.delay(...)` (`views/project.py:258-267`):
    // `requested_data` is the raw body, `current_instance` is null.
    let requested = Value::Object(input);
    let kwargs = pidash_services::v1_projects::tasks::create_kwargs(
        &project_id.to_string(),
        requested,
        &pre.actor.id.to_string(),
        slug,
        &app_origin(state),
    );
    enqueue_best_effort(
        &pre.pool,
        pidash_services::v1_projects::tasks::MODEL_ACTIVITY_TASK,
        kwargs,
    )
    .await;
    let text = render_project(&pre.pool, &row, Some(&ann), false, &render_tz, None, None).await?;
    Ok(json_created(text))
}

/// Add a `role=20` membership row plus its `ProjectUserProperty` row
/// (`views/project.py:229` + `db/models/project.py:348-364`).
pub async fn add_project_admin(
    pool: &PgPool,
    workspace_id: &uuid::Uuid,
    project_id: &uuid::Uuid,
    member_id: &uuid::Uuid,
    actor_id: &uuid::Uuid,
) -> Result<(), Denial> {
    let now = chrono::Utc::now();
    let props = default_props_json();
    let prefs = default_preferences_json();
    sqlx::query(
        r#"INSERT INTO "project_members" ("id", "created_at", "updated_at", "created_by_id",
            "project_id", "workspace_id", "member_id", "role", "view_props", "default_props",
            "preferences", "sort_order", "is_active")
         VALUES ($1,$2,$3,$4,$5,$6,$7,20,$8,$8,$9,65535,TRUE)"#,
    )
    .bind(uuid::Uuid::new_v4())
    .bind(now)
    .bind(now)
    .bind(actor_id)
    .bind(project_id)
    .bind(workspace_id)
    .bind(member_id)
    .bind(&props)
    .bind(&prefs)
    .execute(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let min_sort: Option<f64> = sqlx::query_scalar(
        r#"SELECT MIN("sort_order") FROM "project_user_properties" WHERE "workspace_id" = $1 AND "user_id" = $2 AND "deleted_at" IS NULL"#,
    )
    .bind(workspace_id)
    .bind(member_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?
    .flatten();
    let sort_order = min_sort.map(|m| m - 10000.0).unwrap_or(65535.0);
    let (filters, display_filters, display_properties) = user_property_defaults();
    sqlx::query(
        r#"INSERT INTO "project_user_properties" ("id", "created_at", "updated_at", "created_by_id",
            "workspace_id", "project_id", "user_id", "filters", "display_filters",
            "display_properties", "rich_filters", "preferences", "sort_order")
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,'{}',$11,$12)"#,
    )
    .bind(uuid::Uuid::new_v4())
    .bind(now)
    .bind(now)
    .bind(actor_id)
    .bind(workspace_id)
    .bind(project_id)
    .bind(member_id)
    .bind(&filters)
    .bind(&display_filters)
    .bind(&display_properties)
    .bind(&prefs)
    .bind(sort_order)
    .execute(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(())
}

/// `GET workspaces/<slug>/projects/<pk>/` (`views/project.py:373-380`):
/// the identifier is rewritten pre-permissions; the detail queryset
/// renders through `ProjectSerializer(fields, expand)`.
pub async fn retrieve_project_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    pk: &str,
    query: &QueryMap,
) -> Result<Response, Denial> {
    let pre = preamble(state, headers, slug).await?;
    let workspace_id = pre.workspace_id.ok_or(Denial::Forbidden)?;
    let project_id = rewrite_project_id(&pre.pool, slug, pk).await?;
    require_project_base(
        &pre.pool,
        &workspace_id,
        slug,
        &pre.actor.id,
        Some(&project_id),
        "GET",
    )
    .await?;
    // Gate passed: activate the stored zone now (`TimezoneMixin.initial`
    // runs after permissions; an unknown zone 400s only for survivors).
    let timezone = activate_timezone(pre.actor.timezone.as_deref())?;
    // `.get()` misses raise `DoesNotExist` into the base-handler 404.
    let row = fetch_detail_row(&pre.pool, slug, &pre.actor.id, &project_id)
        .await?
        .ok_or(Denial::NotFound)?;
    let ann = detail_annotations(&row)?;
    let fields = fields_param(query, "fields");
    let expand = fields_param(query, "expand");
    let text = render_project(
        &pre.pool,
        &row,
        Some(&ann),
        false,
        &timezone,
        fields.as_deref(),
        expand.as_deref(),
    )
    .await?;
    Ok(json_ok(text))
}

/// `PATCH workspaces/<slug>/projects/<pk>/` (`views/project.py:403-467`).
pub async fn patch_project_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    pk: &str,
    raw_body: &[u8],
) -> Result<Response, Denial> {
    use pidash_services::v1_projects::ser_project as ser;
    let pre = preamble(state, headers, slug).await?;
    let workspace_id = pre.workspace_id.ok_or(Denial::Forbidden)?;
    let project_id = rewrite_project_id(&pre.pool, slug, pk).await?;
    require_project_base(
        &pre.pool,
        &workspace_id,
        slug,
        &pre.actor.id,
        Some(&project_id),
        "PATCH",
    )
    .await?;
    // Gate passed: activate the stored zone now (`TimezoneMixin.initial`
    // runs after permissions; an unknown zone 400s only for survivors).
    let timezone = activate_timezone(pre.actor.timezone.as_deref())?;
    // `Workspace.objects.get` then `Project.objects.get(pk=pk)` — either
    // miss answers `{"error":"Project does not exist"}`.
    let ws_exists: bool =
        sqlx::query_scalar(r#"SELECT EXISTS(SELECT 1 FROM "workspaces" WHERE "id" = $1)"#)
            .bind(workspace_id)
            .fetch_one(&pre.pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    let plain: Option<sqlx::postgres::PgRow> =
        sqlx::query(r#"SELECT * FROM "projects" WHERE "id" = $1 AND "deleted_at" IS NULL"#)
            .bind(project_id)
            .fetch_optional(&pre.pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    let Some(plain) = plain else {
        return Err(Denial::NotFoundError("Project does not exist".to_owned()));
    };
    if !ws_exists {
        return Err(Denial::NotFoundError("Project does not exist".to_owned()));
    }
    // Before-image for the activity call (`views/project.py:412`): a plain
    // instance, so the annotation keys are absent (`SkipField`).
    let snapshot = render_project(&pre.pool, &plain, None, false, &timezone, None, None).await?;
    let project_name: String = plain.try_get("name").map_err(|_| Denial::ServerError)?;
    let was_default: bool = plain
        .try_get("is_default")
        .map_err(|_| Denial::ServerError)?;
    let was_archived: Option<chrono::DateTime<chrono::Utc>> = plain
        .try_get("archived_at")
        .map_err(|_| Denial::ServerError)?;
    // `intake_view` defaults to the stored value (`views/project.py:414`).
    // The merge only feeds the serializer: `requested_data` below stays the
    // raw body exactly as sent.
    let input = parse_body(raw_body)?;
    let mut merged = input.clone();
    if !merged.contains_key("intake_view") {
        let current: bool = plain
            .try_get("intake_view")
            .map_err(|_| Denial::ServerError)?;
        merged.insert("intake_view".to_owned(), Value::Bool(current));
    }
    if was_archived.is_some() {
        return Err(Denial::BadError(
            "Archived project cannot be updated".to_owned(),
        ));
    }
    let data = coerce_write(&pre.pool, &merged, true, true).await?;
    // Inherited `validate()` first (`ProjectCreateSerializer.validate`).
    let name = field_str(&data, "name");
    let identifier_raw = field_str(&data, "identifier");
    let executor = field_str(&data, "default_agent_executor");
    let lead = field_fk(&data, "project_lead").unwrap_or(None);
    let assignee = field_fk(&data, "default_assignee").unwrap_or(None);
    let lead_member = match lead {
        Some(id) => Some(workspace_has_member(&pre.pool, &workspace_id, &id).await?),
        None => None,
    };
    let assignee_member = match assignee {
        Some(id) => Some(workspace_has_member(&pre.pool, &workspace_id, &id).await?),
        None => None,
    };
    let shared = ser::SharedChecks {
        executor: executor.as_deref(),
        cloud_configured: state.settings().cloud_agent.enabled,
        managed_enabled: state.settings().managed_runner.enabled,
        name: name.as_deref(),
        identifier: identifier_raw.as_deref(),
        project_lead_is_member: lead_member,
        default_assignee_is_member: assignee_member,
    };
    ser::validate_shared(&shared).map_err(ser_denial)?;
    // `update()` tail (`serializers/project.py:214-248`), probed against
    // live Django rather than read:
    // * `default_state` was provided (field validation already proved the
    //   row exists): the scope filter passes the State *instance* as the
    //   UUID lookup (`:227`), so Django's `ValidationError` escapes the
    //   view's DRF-only `except` into the base-handler 400 — for a
    //   same-project state as well as a foreign one.
    // * `estimate` outside the project misses the scope filter (`:234`,
    //   correct `.id` usage) → DRF `ValidationError` → the view's generic
    //   `except` → the identifier-taken 409 (ported bug).
    // * Unsetting the default without a replacement (`:238`) → DRF
    //   `ValidationError` → the same 409.
    // * The name/identifier rechecks (`:218-222`) repeat values `validate()`
    //   already passed — unreachable.
    let default_state = field_fk(&data, "default_state").unwrap_or(None);
    if default_state.is_some() {
        return Err(Denial::BadError("Please provide valid detail".to_owned()));
    }
    let estimate = field_fk(&data, "estimate").unwrap_or(None);
    if let Some(id) = estimate {
        if !estimate_in_project(&pre.pool, &project_id, &id).await? {
            return Err(Denial::Conflict(
                pidash_services::v1_projects::ser_project::IDENTIFIER_TAKEN_BODY.to_owned(),
            ));
        }
    }
    let wants_default = field_bool(&data, "is_default");
    if was_default && wants_default == Some(false) {
        return Err(Denial::Conflict(
            pidash_services::v1_projects::ser_project::IDENTIFIER_TAKEN_BODY.to_owned(),
        ));
    }
    apply_project_update(
        &pre.pool,
        &project_id,
        &workspace_id,
        &pre.actor.id,
        &data,
        was_default,
    )
    .await?;
    // `intake_view` newly truthy + no default `Intake` → create it
    // (`views/project.py:431-438`, pre-save name in the title).
    let intake_view_now: bool =
        sqlx::query_scalar(r#"SELECT "intake_view" FROM "projects" WHERE "id" = $1"#)
            .bind(project_id)
            .fetch_optional(&pre.pool)
            .await
            .map_err(|_| Denial::ServerError)?
            .flatten()
            .unwrap_or(false);
    if intake_view_now {
        let has_default_intake: bool = sqlx::query_scalar(
            r#"SELECT EXISTS(SELECT 1 FROM "intakes" WHERE "project_id" = $1 AND "is_default" AND "deleted_at" IS NULL)"#,
        )
        .bind(project_id)
        .fetch_one(&pre.pool)
        .await
        .map_err(|_| Denial::ServerError)?;
        if !has_default_intake {
            let now = chrono::Utc::now();
            sqlx::query(
                r#"INSERT INTO "intakes" ("id", "created_at", "updated_at", "created_by_id",
                    "project_id", "workspace_id",
                    "name", "description", "is_default", "view_props", "logo_props")
                 VALUES ($1,$2,$3,$4,$5,$6,$7,'',TRUE,'{}','{}')"#,
            )
            .bind(uuid::Uuid::new_v4())
            .bind(now)
            .bind(now)
            .bind(pre.actor.id)
            .bind(project_id)
            .bind(workspace_id)
            .bind(format!("{project_name} Intake"))
            .execute(&pre.pool)
            .await
            .map_err(|_| Denial::ServerError)?;
        }
    }
    let row = fetch_detail_row(&pre.pool, slug, &pre.actor.id, &project_id)
        .await?
        .ok_or(Denial::ServerError)?;
    let ann = detail_annotations(&row)?;
    let requested = Value::Object(input);
    let kwargs = pidash_services::v1_projects::tasks::update_kwargs(
        &project_id.to_string(),
        requested,
        &snapshot,
        &pre.actor.id.to_string(),
        slug,
        &app_origin(state),
    );
    enqueue_best_effort(
        &pre.pool,
        pidash_services::v1_projects::tasks::MODEL_ACTIVITY_TASK,
        kwargs,
    )
    .await;
    let text = render_project(&pre.pool, &row, Some(&ann), false, &timezone, None, None).await?;
    Ok(json_ok(text))
}

/// `Estimate.objects.filter(project=instance, id=...)`.
pub async fn estimate_in_project(
    pool: &PgPool,
    project_id: &uuid::Uuid,
    estimate_id: &uuid::Uuid,
) -> Result<bool, Denial> {
    let exists: bool = sqlx::query_scalar(
        r#"SELECT EXISTS(SELECT 1 FROM "estimates" WHERE "id" = $1 AND "project_id" = $2 AND "deleted_at" IS NULL)"#,
    )
    .bind(estimate_id)
    .bind(project_id)
    .fetch_one(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(exists)
}

/// Apply the PATCH writes (`ProjectUpdateSerializer.update` +
/// `Project.save`): only provided fields, identifier re-normalised, the
/// `is_default` atomic unset, the model backstop, `updated_by` stamping.
/// Unique violations answer the 409 name body.
pub async fn apply_project_update(
    pool: &PgPool,
    project_id: &uuid::Uuid,
    workspace_id: &uuid::Uuid,
    actor_id: &uuid::Uuid,
    data: &WriteData,
    was_default: bool,
) -> Result<(), Denial> {
    use pidash_services::v1_projects::ser_project as ser;
    let now = chrono::Utc::now();
    let mut sets: Vec<String> = vec![
        "\"updated_at\" = $1".to_owned(),
        "\"updated_by_id\" = $2".to_owned(),
    ];
    // (bind index, value) pairs appended in order.
    let mut binds: Vec<Bound> = Vec::new();
    let push_str = |sets: &mut Vec<String>, binds: &mut Vec<Bound>, col: &str, v: String| {
        sets.push(format!("\"{col}\" = ${}", binds.len() + 3));
        binds.push(Bound::Str(v));
    };
    let push_opt_str =
        |sets: &mut Vec<String>, binds: &mut Vec<Bound>, col: &str, v: Option<String>| {
            sets.push(format!("\"{col}\" = ${}", binds.len() + 3));
            binds.push(Bound::OptStr(v));
        };
    let push_bool = |sets: &mut Vec<String>, binds: &mut Vec<Bound>, col: &str, v: bool| {
        sets.push(format!("\"{col}\" = ${}", binds.len() + 3));
        binds.push(Bound::Bool(v));
    };
    let push_int = |sets: &mut Vec<String>, binds: &mut Vec<Bound>, col: &str, v: i32| {
        sets.push(format!("\"{col}\" = ${}", binds.len() + 3));
        binds.push(Bound::Int(v));
    };
    let push_fk =
        |sets: &mut Vec<String>, binds: &mut Vec<Bound>, col: &str, v: Option<uuid::Uuid>| {
            sets.push(format!("\"{col}\" = ${}", binds.len() + 3));
            binds.push(Bound::OptUuid(v));
        };
    let push_json = |sets: &mut Vec<String>, binds: &mut Vec<Bound>, col: &str, v: Value| {
        sets.push(format!("\"{col}\" = ${}", binds.len() + 3));
        binds.push(Bound::Json(v));
    };
    if let Some(v) = field_str(data, "name") {
        push_str(&mut sets, &mut binds, "name", v);
    }
    if let Some(v) = field_str(data, "description") {
        push_str(&mut sets, &mut binds, "description", v);
    }
    for key in [
        "project_lead_id",
        "default_assignee_id",
        "default_state_id",
        "estimate_id",
    ] {
        let field = match key {
            "project_lead_id" => "project_lead",
            "default_assignee_id" => "default_assignee",
            "default_state_id" => "default_state",
            _ => "estimate",
        };
        if let Some(v) = field_fk(data, field) {
            push_fk(&mut sets, &mut binds, key, v);
        }
    }
    if let Some(v) = field_str(data, "identifier") {
        push_str(
            &mut sets,
            &mut binds,
            "identifier",
            ser::normalize_identifier(&v),
        );
    }
    if let Some(v) = field_json(data, "icon_prop") {
        push_json(&mut sets, &mut binds, "icon_prop", v);
    }
    for key in ["emoji", "cover_image", "external_source", "external_id"] {
        if let Some(v) = field_opt_str(data, key) {
            push_opt_str(&mut sets, &mut binds, key, v);
        }
    }
    for key in [
        "module_view",
        "cycle_view",
        "issue_views_view",
        "page_view",
        "intake_view",
        "guest_view_all_features",
        "members_can_edit_states",
        "is_issue_type_enabled",
        "is_time_tracking_enabled",
        "is_default",
    ] {
        if let Some(v) = field_bool(data, key) {
            push_bool(&mut sets, &mut binds, key, v);
        }
    }
    for key in ["archive_in", "close_in"] {
        if let Some(v) = field_int(data, key) {
            push_int(&mut sets, &mut binds, key, v);
        }
    }
    if let Some(v) = field_str(data, "timezone") {
        push_str(&mut sets, &mut binds, "timezone", v);
    }
    if let Some(v) = field_str(data, "repo_url") {
        push_str(&mut sets, &mut binds, "repo_url", v);
    }
    if let Some(v) = field_str(data, "base_branch") {
        push_str(&mut sets, &mut binds, "base_branch", v);
    }
    if let Some(v) = field_str(data, "default_agent_executor") {
        push_str(&mut sets, &mut binds, "default_agent_executor", v);
    }
    // New `is_default` value (or the stored one when untouched).
    let new_default: bool = field_bool(data, "is_default").unwrap_or(was_default);
    // Model backstop (`db/models/project.py:274-290`): unsetting the last
    // default raises Django's `ValidationError` → the base-handler 400.
    if !new_default && was_default {
        let replacement: bool = sqlx::query_scalar(
            r#"SELECT EXISTS(SELECT 1 FROM "projects" WHERE "workspace_id" = $1 AND "is_default" AND "id" != $2 AND "deleted_at" IS NULL)"#,
        )
        .bind(workspace_id)
        .bind(project_id)
        .fetch_one(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
        if !replacement {
            return Err(Denial::BadError("Please provide valid detail".to_owned()));
        }
    }
    let mut tx = pool.begin().await.map_err(|_| Denial::ServerError)?;
    if ser::unsets_other_defaults(field_bool(data, "is_default")) {
        sqlx::query(
            r#"UPDATE "projects" SET "is_default" = FALSE WHERE "workspace_id" = $1 AND "is_default" AND "id" != $2 AND "deleted_at" IS NULL"#,
        )
        .bind(workspace_id)
        .bind(project_id)
        .execute(&mut *tx)
        .await
        .map_err(|_| Denial::ServerError)?;
    }
    let sql = format!(
        "UPDATE \"projects\" SET {} WHERE \"id\" = ${} AND \"deleted_at\" IS NULL",
        sets.join(", "),
        binds.len() + 3
    );
    let mut query = sqlx::query(&sql).bind(now).bind(actor_id);
    for bind in binds {
        query = match bind {
            Bound::Str(v) => query.bind(v),
            Bound::OptStr(v) => query.bind(v),
            Bound::Bool(v) => query.bind(v),
            Bound::Int(v) => query.bind(v),
            Bound::OptUuid(v) => query.bind(v),
            Bound::Json(v) => query.bind(v),
        };
    }
    // `$N` in the WHERE clause is the project id (the SET placeholders are
    // `$1..=$N-1`); without this bind the update matches nothing.
    query = query.bind(project_id);
    match query.execute(&mut *tx).await {
        Ok(_) => {}
        Err(sqlx::Error::Database(db)) if db.code().as_deref() == Some("23505") => {
            return Err(Denial::Conflict(ser::NAME_TAKEN_BODY.to_owned()));
        }
        Err(_) => return Err(Denial::ServerError),
    }
    tx.commit().await.map_err(|_| Denial::ServerError)?;
    Ok(())
}

#[derive(Debug, Clone)]
enum Bound {
    Str(String),
    OptStr(Option<String>),
    Bool(bool),
    Int(i32),
    OptUuid(Option<uuid::Uuid>),
    Json(Value),
}

/// `DELETE workspaces/<slug>/projects/<pk>/`
/// (`views/project.py:480-508`): default projects refuse with 400,
/// favorites soft-delete by the triple filter, then the project
/// soft-deletes (with its related-objects fan-out) and the webhook
/// activity fires.
pub async fn delete_project_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    pk: &str,
) -> Result<Response, Denial> {
    let pre = preamble(state, headers, slug).await?;
    let workspace_id = pre.workspace_id.ok_or(Denial::Forbidden)?;
    let project_id = rewrite_project_id(&pre.pool, slug, pk).await?;
    require_project_base(
        &pre.pool,
        &workspace_id,
        slug,
        &pre.actor.id,
        Some(&project_id),
        "DELETE",
    )
    .await?;
    // Gate passed: activate the stored zone now (`TimezoneMixin.initial`
    // runs after permissions; an unknown zone 400s only for survivors).
    activate_timezone(pre.actor.timezone.as_deref())?;
    let row: Option<(bool,)> = sqlx::query_as(
        r#"SELECT "is_default" FROM "projects" WHERE "id" = $1 AND "workspace_id" = $2 AND "deleted_at" IS NULL"#,
    )
    .bind(project_id)
    .bind(workspace_id)
    .fetch_optional(&pre.pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let Some((is_default,)) = row else {
        return Err(Denial::NotFound);
    };
    if is_default {
        return Err(Denial::BadError(
            "Default project cannot be deleted".to_owned(),
        ));
    }
    let now = chrono::Utc::now();
    // `UserFavorite` cascade (`views/project.py:493`): queryset
    // soft-delete over the triple filter.
    sqlx::query(
        r#"UPDATE "user_favorites" SET "deleted_at" = $1 WHERE "entity_type" = 'project' AND "entity_identifier" = $2 AND "project_id" = $3 AND "deleted_at" IS NULL"#,
    )
    .bind(now)
    .bind(project_id)
    .bind(project_id)
    .execute(&pre.pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    // Instance `delete()` → `save()`: `deleted_at` + `updated_*`.
    sqlx::query(
        r#"UPDATE "projects" SET "deleted_at" = $1, "updated_at" = $1, "updated_by_id" = $2 WHERE "id" = $3"#,
    )
    .bind(now)
    .bind(pre.actor.id)
    .bind(project_id)
    .execute(&pre.pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    enqueue_soft_delete(&pre.pool, "project", &project_id).await;
    // `webhook_activity.delay(...)` (`views/project.py:495-507`).
    let kwargs = pidash_services::v1_projects::tasks::delete_kwargs(
        &pre.actor.id.to_string(),
        slug,
        &app_origin(state),
        &project_id.to_string(),
    );
    enqueue_best_effort(
        &pre.pool,
        pidash_services::v1_projects::tasks::WEBHOOK_ACTIVITY_TASK,
        kwargs,
    )
    .await;
    Ok(no_content())
}

/// One `DEFAULT_STATES` row (`views/project.py:240-254`).
#[allow(clippy::too_many_arguments)]
pub async fn insert_default_state(
    pool: &PgPool,
    project_id: &uuid::Uuid,
    workspace_id: &uuid::Uuid,
    actor_id: &uuid::Uuid,
    name: &str,
    color: &str,
    sequence: f64,
    group: &str,
    default: bool,
    now: &chrono::DateTime<chrono::Utc>,
) -> Result<(), Denial> {
    let slug = pidash_db::v1_projects::models::state::slugify_name(name);
    sqlx::query(
        r#"INSERT INTO "states" ("id", "created_at", "updated_at", "created_by_id",
            "project_id", "workspace_id", "name", "description", "color", "slug",
            "sequence", "group", "is_triage", "default")
         VALUES ($1,$2,$3,$4,$5,$6,$7,'',$8,$9,$10,$11,FALSE,$12)"#,
    )
    .bind(uuid::Uuid::new_v4())
    .bind(now)
    .bind(now)
    .bind(actor_id)
    .bind(project_id)
    .bind(workspace_id)
    .bind(name)
    .bind(color)
    .bind(&slug)
    .bind(sequence)
    .bind(group)
    .bind(default)
    .execute(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn denial_body(denial: Denial) -> (u16, String) {
        let (status, body) = denial.status_and_body();
        (status.as_u16(), body)
    }

    #[test]
    fn denial_bodies_are_byte_identical() {
        assert_eq!(
            denial_body(Denial::Unauthorized),
            (
                401,
                r#"{"detail":"Authentication credentials were not provided."}"#.to_owned()
            )
        );
        assert_eq!(
            denial_body(Denial::InvalidToken),
            (
                403,
                r#"{"detail":"Given API token is not valid"}"#.to_owned()
            )
        );
        assert_eq!(
            denial_body(Denial::Forbidden),
            (
                403,
                r#"{"detail":"You do not have permission to perform this action."}"#.to_owned()
            )
        );
        assert_eq!(
            denial_body(Denial::NotFound),
            (
                404,
                r#"{"error":"The requested resource does not exist."}"#.to_owned()
            )
        );
        assert_eq!(
            denial_body(Denial::ProjectNotFound),
            (404, r#"{"detail":"Project not found"}"#.to_owned())
        );
        assert_eq!(
            denial_body(Denial::ServerError),
            (
                500,
                r#"{"error":"Something went wrong please try again later"}"#.to_owned()
            )
        );
        assert_eq!(
            denial_body(Denial::BadError(
                "Archived project cannot be updated".to_owned()
            )),
            (
                400,
                r#"{"error":"Archived project cannot be updated"}"#.to_owned()
            )
        );
        assert_eq!(
            denial_body(Denial::NotFoundError("Project not found".to_owned())),
            (404, r#"{"error":"Project not found"}"#.to_owned())
        );
        assert_eq!(
            denial_body(Denial::Conflict(
                r#"{"identifier":"The project identifier is already taken"}"#.to_owned()
            )),
            (
                409,
                r#"{"identifier":"The project identifier is already taken"}"#.to_owned()
            )
        );
    }

    #[test]
    fn parse_body_shapes() {
        assert!(parse_body(b"").expect("empty").is_empty());
        let map = parse_body(br#"{"name":"N"}"#).expect("object");
        assert_eq!(map.get("name"), Some(&Value::String("N".to_owned())));
        let Err(Denial::FieldErrors(body)) = parse_body(b"[1]") else {
            panic!("non-object is a field error");
        };
        assert_eq!(
            body,
            r#"{"non_field_errors":["Invalid data. Expected a dictionary, but got list."]}"#
        );
        let Err(Denial::BadDetail(_)) = parse_body(b"{oops") else {
            panic!("malformed json is a parse error");
        };
    }

    #[test]
    fn char_coercion_trims_and_guards() {
        assert_eq!(
            coerce_char(&Value::String("  ENG  ".to_owned()), false, Some(12)).expect("trim"),
            "ENG"
        );
        assert_eq!(
            coerce_char(&Value::from(5), false, Some(255)).expect("number"),
            "5"
        );
        let Err(CoerceFail::Msg(message)) =
            coerce_char(&Value::String("   ".to_owned()), false, Some(255))
        else {
            panic!("whitespace-only is blank");
        };
        assert_eq!(message, "This field may not be blank.");
        let long = "x".repeat(13);
        let Err(CoerceFail::Msg(message)) = coerce_char(&Value::String(long), false, Some(12))
        else {
            panic!("over-long fails");
        };
        assert_eq!(message, "Ensure this field has no more than 12 characters.");
        // Booleans never coerce to strings.
        assert!(matches!(
            coerce_char(&Value::Bool(true), false, Some(255)),
            Err(CoerceFail::Msg(_))
        ));
    }

    #[test]
    fn bool_coercion_sets() {
        assert!(coerce_bool(&Value::Bool(true)).expect("bool"));
        assert!(coerce_bool(&Value::String("YES".to_owned())).expect("yes"));
        assert!(!coerce_bool(&Value::from(0)).expect("zero"));
        assert!(coerce_bool(&Value::from(1)).expect("one"));
        assert!(matches!(
            coerce_bool(&Value::String("maybe".to_owned())),
            Err(CoerceFail::Msg(_))
        ));
        assert!(matches!(coerce_bool(&Value::Null), Err(CoerceFail::Msg(_))));
    }

    #[test]
    fn int_coercion_decimal_suffix_and_range() {
        assert_eq!(coerce_int(&Value::from(3), 0, 12).expect("int"), 3);
        assert_eq!(
            coerce_int(&Value::String("1.0".to_owned()), 0, 12).expect("decimal"),
            1
        );
        assert!(matches!(
            coerce_int(&Value::String("1.2".to_owned()), 0, 12),
            Err(CoerceFail::Msg(_))
        ));
        assert!(matches!(
            coerce_int(&Value::Bool(true), 0, 12),
            Err(CoerceFail::Msg(_))
        ));
        let Err(CoerceFail::Msg(message)) = coerce_int(&Value::from(13), 0, 12) else {
            panic!("max validator fires");
        };
        assert_eq!(message, "Ensure this value is less than or equal to 12.");
        let Err(CoerceFail::Msg(message)) = coerce_int(&Value::from(-1), 0, 12) else {
            panic!("min validator fires");
        };
        assert_eq!(message, "Ensure this value is greater than or equal to 0.");
    }

    #[test]
    fn choice_and_timezone_membership() {
        assert_eq!(
            coerce_choice(
                &Value::String("cloud_agent".to_owned()),
                &["local_runner", "cloud_agent", "managed_runner"]
            )
            .expect("member"),
            "cloud_agent"
        );
        let Err(CoerceFail::Msg(message)) =
            coerce_choice(&Value::String("hyper".to_owned()), &["local_runner"])
        else {
            panic!("non-member fails");
        };
        assert_eq!(message, "\"hyper\" is not a valid choice.");
        assert!(
            super::super::tz_zones::PYTZ_COMMON_TIMEZONES
                .windows(2)
                .all(|w| w[0] < w[1]),
            "zone list stays sorted for binary search"
        );
        assert!(super::super::tz_zones::PYTZ_COMMON_TIMEZONES
            .binary_search(&"UTC")
            .is_ok());
    }

    #[test]
    fn list_order_validation() {
        // Known queryset arms render SQL.
        for key in ["sort_order", "-created_at", "name"] {
            assert!(
                resolve_list_order(key).expect("known").contains("ORDER BY"),
                "{key}"
            );
        }
        // Concrete columns and annotations order directly.
        assert!(resolve_list_order("-identifier")
            .expect("column")
            .contains("ORDER BY \"identifier\" DESC"));
        // Unknown keys are Django's `FieldError` → the generic 500.
        assert!(matches!(
            resolve_list_order("nope"),
            Err(Denial::ServerError)
        ));
    }

    #[test]
    fn read_field_order_pins_wire_order() {
        // `[pk] + declared annotations + concrete fields + forward relations`.
        assert_eq!(
            &READ_FIELD_ORDER[..9],
            &[
                "id",
                "total_members",
                "total_cycles",
                "total_modules",
                "is_member",
                "sort_order",
                "member_role",
                "is_deployed",
                "cover_image_url",
            ]
        );
        assert_eq!(
            &READ_FIELD_ORDER[9..13],
            &["created_at", "updated_at", "deleted_at", "name",]
        );
        assert_eq!(
            &READ_FIELD_ORDER[READ_FIELD_ORDER.len() - 8..],
            &[
                "created_by",
                "updated_by",
                "workspace",
                "default_assignee",
                "project_lead",
                "cover_image_asset",
                "estimate",
                "default_state",
            ]
        );
        assert_eq!(ALLOWED_SUMMARY_FIELDS.len(), 8);
    }

    #[test]
    fn ser_error_mapping_keeps_phases() {
        use pidash_services::v1_projects::ser_project::ProjectSerError as E;
        // `validate()`-phase failures keep their own bodies.
        assert!(matches!(
            ser_denial(E::NameForbidden),
            Denial::FieldErrors(_)
        ));
        assert!(matches!(
            ser_denial(E::CloudAgentUnavailable),
            Denial::Conflict(_)
        ));
        // `update()`-tail failures: estimate-scope and unset-default
        // collapse into the identifier 409 (probed against live Django);
        // a provided `default_state` escapes into the base-handler 400
        // because the scope filter passes the State instance as the UUID.
        assert_eq!(
            denial_body(Denial::Conflict(
                pidash_services::v1_projects::ser_project::IDENTIFIER_TAKEN_BODY.to_owned()
            )),
            (
                409,
                r#"{"identifier":"The project identifier is already taken"}"#.to_owned()
            )
        );
        assert_eq!(
            denial_body(Denial::BadError("Please provide valid detail".to_owned())),
            (400, r#"{"error":"Please provide valid detail"}"#.to_owned())
        );
    }
}
