//! Module handlers (D-20 handlers, PIDASHCONV-406).
//!
//! Ports `apps/api/pi_dash/api/views/module.py:76-1077` (5 view classes,
//! 7 wired routes) onto the merged D-20 foundation, registered by
//! [`routes`] at the seven `apps/api/pi_dash/api/urls/module.py:15-51`
//! paths.
//!
//! Layering (all foundation use is read-only): row SQL in
//! `pidash_db::v1_cycles_modules` (`module` models, `module_queries`
//! builders), read-choice + validation rules in
//! `pidash_services::v1_cycles_modules` (`module_shapes`,
//! `module_queries`), field lists + status values in
//! `pidash_types::v1_cycles_modules::module_shapes`, gate decisions in
//! `super::gates` over the F-06 kernel (`pidash_auth::permissions`),
//! task kwargs in `pidash_jobs::v1_cycles_modules::publish`. This module
//! owns the HTTP shell: API-key auth, the slug→UUID rewrite, permission
//! wiring, DRF field coercion, the write statements, the read-shape
//! rendering and the paginated envelope. The shell mirrors the merged
//! D-19 `v1_projects` handlers (PIDASHCONV-369/371), which duplicate it
//! per file rather than cross-importing.
//!
//! Request order (preserved, not redesigned): routing first (a bad-UUID
//! path param proxies to Django, whose resolver 404 precedes auth — the
//! `app_cycles` precedent), then API-key authentication, then the
//! slug→UUID rewrite (`api/views/base.py:51-98`, skipped for anonymous
//! callers so slugs cannot be probed via 404-vs-401), then
//! `check_permissions`, then the handler body.
//!
//! Ported bugs and deliberate warts (also listed in the PR):
//!
//! * The five grouped counts are `COUNT(DISTINCT "states"."group")`, so
//!   each renders 0 or 1, never a real count (db layer, FX-CYCMOD-05
//!   bug 1; live: 1 unstarted issue renders `unstarted_issues: 1`, a
//!   second would still render 1).
//! * The module-issues POST "move" path is dead: `str(module_issue.id)
//!   in issues` compares `str` against UUIDs and is always false
//!   (`views/module.py:684`), so every call is create-only and
//!   `updated_module_issues` is always `[]`. The loop is ported as its
//!   outcome (no updates), not its text.
//! * PATCH returns the *update-serializer* data (8 write fields, no `id`)
//!   instead of a re-serialised `ModuleSerializer`
//!   (`views/module.py:449`).
//! * POST external-dup answers the *clash row's* id while PATCH
//!   external-dup answers the *current module's* id (`:223` vs `:433`).
//! * `ModuleSerializer.to_representation` appends `members` after field
//!   filtering, so `?fields=name` still renders `members`
//!   (`serializers/module.py:203-206`); it also silently undoes
//!   `?expand=members` (the expanded list is overwritten with id
//!   strings).
//! * `?expand=<unknown>` overwrites the named key with `None`
//!   (`getattr(instance, f"{expand}_id", None)`); a map hit on a null FK
//!   renders `{}` (single-object `None` serialises empty).
//! * Instance `delete()` fires `soft_delete_related_objects` with
//!   `using=None` as a kwarg; queryset deletes do not fire it at all.
//! * `BaseModel.save` stamps `created_by` on create (leaving
//!   `updated_by` NULL) and `updated_by` on every later save, via CRUM.
//! * `validate()`'s `"Project not found"` branch is unreachable
//!   (`Project.objects.get` raises first); the gate 403s the missing
//!   project before the body runs anyway.
//! * The members rewrite and every default-ordered read use `-created_at`
//!   ordering, including the `str(queryset)` payload text.
//!
//! Fixture: `FX-CYCMOD-08`
//! (`rust-api/fixtures/v1_cycles_modules/handlers/module.golden.json`).
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

use std::collections::HashMap;

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use bytes::Bytes;
use chrono_tz::Tz;
use serde_json::Value;
use sqlx::{PgPool, Row};

use pidash_auth::permissions::project;

use crate::state::AppState;

// ---------------------------------------------------------------------------
// Responses
// ---------------------------------------------------------------------------

/// Exact bytes of the DRF `NotAuthenticated` denial (anonymous on a guarded
/// endpoint: `APIKeyAuthentication.authenticate` returns `None`, so
/// `permission_denied` raises `NotAuthenticated`, not `PermissionDenied`).
pub const UNAUTHENTICATED_BODY: &str =
    r#"{"detail":"Authentication credentials were not provided."}"#;
/// `APIKeyAuthentication` failure (`api/middleware/api_authentication.py`):
/// every token rejection maps to this single 403 body.
pub const INVALID_TOKEN_BODY: &str = r#"{"detail":"Given API token is not valid"}"#;
/// `handle_exception`'s `ObjectDoesNotExist` branch
/// (`api/views/base.py:154-158`): `.get()` misses on detail/patch/delete/
/// archive/lookup paths.
pub const NOT_FOUND_BODY: &str = r#"{"error":"The requested resource does not exist."}"#;
/// DRF's `Http404` rendering with an explicit message: `Project.resolve`
/// misses raise `Http404("Project not found")`, which DRF's
/// `exception_handler` re-raises as `NotFound(*exc.args)`, so the message
/// survives instead of the `"Not found."` default
/// (`db/models/project.py:213-218`).
pub const PROJECT_NOT_FOUND_BODY: &str = r#"{"detail":"Project not found"}"#;
/// `handle_exception`'s generic branch (`api/views/base.py:166-170`).
pub const SERVER_ERROR_BODY: &str = r#"{"error":"Something went wrong please try again later"}"#;
/// `handle_exception`'s Django-`ValidationError` branch
/// (`api/views/base.py:148-152`): invalid UUIDs inside `pk__in` lookups on
/// the module-issues POST path.
pub const INVALID_DETAIL_BODY: &str = r#"{"error":"Please provide valid detail"}"#;

/// Handler failure with its exact status + body.
#[derive(Debug)]
pub enum Denial {
    /// 401, DRF `NotAuthenticated` (no `X-Api-Key` header).
    Unauthorized,
    /// 403, invalid/expired/inactive API or machine token.
    InvalidToken,
    /// 403, the DRF-default `PermissionDenied` body (`ProjectEntityPermission`
    /// sets no `message`).
    Forbidden,
    /// 403 with an inline `{"error": ...}` body (the module DELETE
    /// creator-or-admin check answers its own message).
    ForbiddenError(String),
    /// 404, `ObjectDoesNotExist` branch.
    NotFound,
    /// 404, `{"detail":"Project not found"}` (identifier rewrite miss).
    ProjectNotFound,
    /// 400, `{"detail": ...}` lowercase (DRF `ParseError`: pagination, JSON).
    BadDetail(String),
    /// 400, `{"error": ...}` (view-inline).
    BadError(String),
    /// 400, serializer `errors` dict (pre-rendered bytes, field order).
    FieldErrors(String),
    /// 409, `{"error": ...}` external-id clash (pre-rendered bytes).
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
                super::gates::CLASS_DENIAL_BODY.to_owned(),
            ),
            Denial::ForbiddenError(message) => (
                StatusCode::FORBIDDEN,
                format!("{{\"error\":{}}}", json_string(message)),
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
            Denial::FieldErrors(body) => (StatusCode::BAD_REQUEST, body.clone()),
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

/// Map a database/driver failure to the generic 500 while logging the site
/// and error for operators (no secrets: messages never include tokens).
fn db_error<E: std::fmt::Display>(error: E, site: &str) -> Denial {
    tracing::warn!(%error, site, "v1_cycles_modules database failure");
    Denial::ServerError
}

// ---------------------------------------------------------------------------
// Cutover wiring
// ---------------------------------------------------------------------------

/// Route registration is the cutover granularity (the pilot `owned()`
/// pattern shared with the D-19 `v1_projects` family): the owned methods
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

/// `.../modules/` owns GET+POST (`api/urls/module.py:15-20`).
pub fn owned_list(
    router: axum::routing::MethodRouter<AppState>,
) -> axum::routing::MethodRouter<AppState> {
    owned(router, &["GET", "POST"])
}

/// `.../modules/<pk>/` owns GET+PATCH+DELETE (`api/urls/module.py:21-25`).
pub fn owned_detail(
    router: axum::routing::MethodRouter<AppState>,
) -> axum::routing::MethodRouter<AppState> {
    owned(router, &["GET", "PATCH", "DELETE"])
}

/// `.../modules/<module_id>/module-issues/` owns GET+POST
/// (`api/urls/module.py:26-30`).
pub fn owned_issue_list(
    router: axum::routing::MethodRouter<AppState>,
) -> axum::routing::MethodRouter<AppState> {
    owned(router, &["GET", "POST"])
}

/// `.../module-issues/<issue_id>/` owns DELETE only
/// (`api/urls/module.py:31-35`): the view defines `get`
/// (`views/module.py:800`) but no route reaches it, so GET proxies to
/// Django and 405s after auth exactly as before.
pub fn owned_issue_detail(
    router: axum::routing::MethodRouter<AppState>,
) -> axum::routing::MethodRouter<AppState> {
    owned(router, &["DELETE"])
}

/// `.../modules/<pk>/archive/` owns POST (`api/urls/module.py:36-40`).
pub fn owned_archive(
    router: axum::routing::MethodRouter<AppState>,
) -> axum::routing::MethodRouter<AppState> {
    owned(router, &["POST"])
}

/// `.../archived-modules/` owns GET (`api/urls/module.py:41-45`).
pub fn owned_archived_list(
    router: axum::routing::MethodRouter<AppState>,
) -> axum::routing::MethodRouter<AppState> {
    owned(router, &["GET"])
}

/// `.../archived-modules/<pk>/unarchive/` owns DELETE
/// (`api/urls/module.py:46-50`).
pub fn owned_unarchive(
    router: axum::routing::MethodRouter<AppState>,
) -> axum::routing::MethodRouter<AppState> {
    owned(router, &["DELETE"])
}

// ---------------------------------------------------------------------------
// Query params
// ---------------------------------------------------------------------------

/// One query value, repeated or not (same shape as the D-19 `v1_projects`
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

/// Gate-table path keys (`super::gates::MATRIX`): the route shapes the
/// permission layer matches on.
pub const PATH_MODULES: &str = "workspaces/<slug>/projects/<id>/modules/";
pub const PATH_MODULE_DETAIL: &str = "workspaces/<slug>/projects/<id>/modules/<uuid>/";
pub const PATH_MODULE_ISSUES: &str =
    "workspaces/<slug>/projects/<id>/modules/<uuid>/module-issues/";
pub const PATH_MODULE_ISSUE_DETAIL: &str =
    "workspaces/<slug>/projects/<id>/modules/<uuid>/module-issues/<uuid>/";
pub const PATH_MODULE_ARCHIVE: &str = "workspaces/<slug>/projects/<id>/modules/<uuid>/archive/";
pub const PATH_ARCHIVED_LIST: &str = "workspaces/<slug>/projects/<id>/archived-modules/";
pub const PATH_MODULE_UNARCHIVE: &str =
    "workspaces/<slug>/projects/<id>/archived-modules/<uuid>/unarchive/";

/// The authenticated actor: user id plus active time zone
/// (`TimezoneMixin.initial` activates `request.user.user_timezone`,
/// `api/views/base.py:43-48`).
pub struct Actor {
    pub id: uuid::Uuid,
    pub timezone: Tz,
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
    let timezone = request_timezone(pool, &user_id).await?;
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
async fn resolve_api_token(pool: &PgPool, presented: &str) -> Result<uuid::Uuid, Denial> {
    let row: Option<ApiTokenLookup> = sqlx::query_as(
        r#"SELECT "token", "is_active", "expired_at", "user_id" FROM "api_tokens" WHERE "token" = $1"#,
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
    // No `users.is_active` check: `validate_api_token` + permissions never
    // consult it (stock `UserManager`, no filtering), so Django serves a
    // deactivated user's token when membership passes — port the wart.
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

/// The request time zone (`TimezoneMixin.initial`): the user's stored zone;
/// an invalid stored zone 500s like `zoneinfo.ZoneInfo` raising.
async fn request_timezone(pool: &PgPool, user_id: &uuid::Uuid) -> Result<Tz, Denial> {
    let name: Option<Option<String>> =
        sqlx::query_scalar(r#"SELECT "user_timezone" FROM "users" WHERE "id" = $1"#)
            .bind(user_id)
            .fetch_optional(pool)
            .await
            .map_err(|error| db_error(error, "request-timezone"))?;
    let name: Option<String> = name.unwrap_or(None);
    name.as_deref()
        .unwrap_or("UTC")
        .parse::<Tz>()
        .map_err(|error| db_error(error, "request-timezone"))
}

// ---------------------------------------------------------------------------
// Identifier rewrite + permissions
// ---------------------------------------------------------------------------

/// `_rewrite_project_kwarg` (`api/views/base.py:51-98`): a slug-or-UUID
/// `project_id` becomes the canonical project UUID before permission
/// checks. UUID-looking input passes through unverified (the view body
/// 404/403s it as before); identifier misses answer the
/// `{"detail":"Project not found"}` 404. The `pk`-rewrite arm only fires
/// for the `project` URL name, so module `pk` params never rewrite.
pub async fn rewrite_project_id(
    pool: &PgPool,
    workspace_slug: &str,
    raw: &str,
) -> Result<uuid::Uuid, Denial> {
    // `uuid.UUID(str(raw))` accepts hyphenated, plain-hex, braced and
    // `urn:uuid:` forms; `parse_str` accepts the same set.
    if let Ok(id) = raw.parse::<uuid::Uuid>() {
        return Ok(id);
    }
    // `Project.resolve` (`db/models/project.py:192-219`): upper-cased
    // identifier match on the live row, else `Http404("Project not found")`.
    let identifier = raw.trim().to_uppercase();
    let row: Option<uuid::Uuid> = sqlx::query_scalar(
        r#"SELECT p."id" FROM "projects" p
           INNER JOIN "workspaces" w ON p."workspace_id" = w."id"
           WHERE w."slug" = $1 AND p."identifier" = $2 AND p."deleted_at" IS NULL"#,
    )
    .bind(workspace_slug)
    .bind(identifier)
    .fetch_optional(pool)
    .await
    .map_err(|error| db_error(error, "rewrite-project"))?;
    row.ok_or(Denial::ProjectNotFound)
}

/// Fetch the `ProjectEntityPermission` facts exactly as the guard filters
/// them (`app/permissions/project.py:85-116`): project membership (any
/// role, active, workspace-scoped) for safe methods, ADMIN/MEMBER-only for
/// writes. `role` is `smallint`: sqlx does not widen `INT2` into `i32` on
/// decode, so read `i16` and compare as integers (same hazard as the
/// `member_role` fix in PIDASHCONV-478).
pub async fn project_entity_facts(
    pool: &PgPool,
    workspace_id: &uuid::Uuid,
    workspace_slug: &str,
    user_id: &uuid::Uuid,
    project_id: &uuid::Uuid,
) -> Result<project::ProjectFacts, Denial> {
    let roles: Vec<i16> = sqlx::query_scalar(
        r#"SELECT "role" FROM "project_members" WHERE "workspace_id" = $1 AND "member_id" = $2 AND "project_id" = $3 AND "is_active" AND "deleted_at" IS NULL"#,
    )
    .bind(workspace_id)
    .bind(user_id)
    .bind(project_id)
    .fetch_all(pool)
    .await
    .map_err(|error| db_error(error, "entity-facts"))?;
    Ok(project::ProjectFacts {
        workspace: pidash_types::WorkspaceId::from(workspace_slug.to_owned()),
        project_id: pidash_types::ProjectId::from(project_id.to_string()),
        authenticated: true,
        is_workspace_member: true,
        has_workspace_admin_or_member: false,
        is_workspace_admin: false,
        is_project_member: !roles.is_empty(),
        is_project_admin: roles.contains(&20),
        has_project_admin_or_member: roles.iter().any(|r| *r == 20 || *r == 15),
        has_identifier_membership: false,
        // No D-20 view defines `project_identifier`, so the kernel's
        // identifier branch is dead here (gates FX-CYCMOD-06).
        has_project_identifier: false,
    })
}

/// Check the `ProjectEntityPermission` gate for one route+method through
/// `super::gates`; deny with the class body on failure.
pub async fn require_gate(
    pool: &PgPool,
    workspace_id: &uuid::Uuid,
    workspace_slug: &str,
    user_id: &uuid::Uuid,
    project_id: &uuid::Uuid,
    method: &str,
    path: &str,
) -> Result<(), Denial> {
    let row = super::gates::gate_for(method, path).ok_or(Denial::ServerError)?;
    let facts =
        project_entity_facts(pool, workspace_id, workspace_slug, user_id, project_id).await?;
    let scope = super::gates::tenant_context(workspace_slug);
    if super::gates::decide(&row.gate, method, &scope, &facts) {
        Ok(())
    } else {
        Err(Denial::Forbidden)
    }
}

// ---------------------------------------------------------------------------
// Shared handler plumbing
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

/// `web_base_url()` (`utils/host.py:70-86`): `WEB_URL` first, then
/// `APP_BASE_URL` (the opposite order from `base_host(is_app=True)`),
/// trailing slash stripped; `None` when neither is configured, in which
/// case callers omit the URL field rather than emitting a wrong one.
pub fn web_base_url(state: &AppState) -> Option<String> {
    let base = state
        .settings()
        .urls
        .web_url
        .clone()
        .or_else(|| state.settings().urls.app_base_url.clone())?;
    if base.is_empty() {
        return None;
    }
    Some(base.trim_end_matches('/').to_owned())
}

/// Best-effort post-commit task fan-out (the `.delay()` calls): without a
/// queue table the response still stands (same precedent as the D-19
/// project handlers). Python publishes to RabbitMQ outside any request
/// transaction, so best-effort-after-write is the faithful port.
pub async fn enqueue_best_effort(pool: &PgPool, job: &pidash_jobs::queue::NewJob) {
    if let Err(error) = pidash_jobs::queue::enqueue(pool, job).await {
        tracing::warn!(%error, task = job.task.as_str(), "task enqueue failed; response stands");
    }
}

/// Soft-delete fan-out (`db/mixins.py:72-78`): instance `delete()` stamps
/// `deleted_at` + `save()` (so `updated_at`/`updated_by` move) and enqueues
/// `soft_delete_related_objects(app_label, model_name, pk, using=None)` —
/// `using` as a null kwarg, exactly as Python's
/// `.delay(label, name, pk, using=using)` call sends it. (The D-19 project
/// handlers send a positional `"default"` instead; the worker accepts both
/// forms, but this port follows the Python call.)
pub async fn enqueue_soft_delete(pool: &PgPool, model: &str, pk: &uuid::Uuid) {
    let mut kwargs = serde_json::Map::with_capacity(1);
    kwargs.insert("using".to_owned(), Value::Null);
    let job = pidash_jobs::queue::NewJob::new(
        pidash_jobs::tasks_cleanup::deletion::SOFT_DELETE_TASK,
        Value::Array(vec![
            Value::String("db".to_owned()),
            Value::String(model.to_owned()),
            Value::String(pk.to_string()),
        ]),
        Value::Object(kwargs),
    );
    enqueue_best_effort(pool, &job).await;
}

/// Truncate a timestamp to microseconds: Postgres `timestamptz` stores
/// micros, and Django's `timezone.now()` is microsecond-precise, so the
/// inserted value and every payload rendering of it agree.
pub fn micros_now() -> chrono::DateTime<chrono::Utc> {
    let now = chrono::Utc::now();
    now - chrono::Duration::nanoseconds(now.timestamp_subsec_nanos() as i64 % 1000)
}

// ---------------------------------------------------------------------------
// Row decoding + rendering primitives
// ---------------------------------------------------------------------------

fn row_uuid(row: &sqlx::postgres::PgRow, column: &str, site: &str) -> Result<uuid::Uuid, Denial> {
    row.try_get::<uuid::Uuid, _>(column)
        .map_err(|error| db_error(error, site))
}

fn row_uuid_opt(
    row: &sqlx::postgres::PgRow,
    column: &str,
    site: &str,
) -> Result<Option<uuid::Uuid>, Denial> {
    row.try_get::<Option<uuid::Uuid>, _>(column)
        .map_err(|error| db_error(error, site))
}

fn row_string(row: &sqlx::postgres::PgRow, column: &str, site: &str) -> Result<String, Denial> {
    row.try_get::<String, _>(column)
        .map_err(|error| db_error(error, site))
}

fn row_string_opt(
    row: &sqlx::postgres::PgRow,
    column: &str,
    site: &str,
) -> Result<Option<String>, Denial> {
    row.try_get::<Option<String>, _>(column)
        .map_err(|error| db_error(error, site))
}

fn row_datetime(
    row: &sqlx::postgres::PgRow,
    column: &str,
    site: &str,
) -> Result<chrono::DateTime<chrono::Utc>, Denial> {
    row.try_get::<chrono::DateTime<chrono::Utc>, _>(column)
        .map_err(|error| db_error(error, site))
}

fn row_datetime_opt(
    row: &sqlx::postgres::PgRow,
    column: &str,
    site: &str,
) -> Result<Option<chrono::DateTime<chrono::Utc>>, Denial> {
    row.try_get::<Option<chrono::DateTime<chrono::Utc>>, _>(column)
        .map_err(|error| db_error(error, site))
}

fn row_date_opt(
    row: &sqlx::postgres::PgRow,
    column: &str,
    site: &str,
) -> Result<Option<chrono::NaiveDate>, Denial> {
    row.try_get::<Option<chrono::NaiveDate>, _>(column)
        .map_err(|error| db_error(error, site))
}

fn row_i64(row: &sqlx::postgres::PgRow, column: &str, site: &str) -> Result<i64, Denial> {
    row.try_get::<i64, _>(column)
        .map_err(|error| db_error(error, site))
}

fn row_i32(row: &sqlx::postgres::PgRow, column: &str, site: &str) -> Result<i32, Denial> {
    row.try_get::<i32, _>(column)
        .map_err(|error| db_error(error, site))
}

fn row_i32_opt(
    row: &sqlx::postgres::PgRow,
    column: &str,
    site: &str,
) -> Result<Option<i32>, Denial> {
    row.try_get::<Option<i32>, _>(column)
        .map_err(|error| db_error(error, site))
}

fn row_json(row: &sqlx::postgres::PgRow, column: &str, site: &str) -> Result<Value, Denial> {
    row.try_get::<Value, _>(column)
        .map_err(|error| db_error(error, site))
}

fn row_json_opt(
    row: &sqlx::postgres::PgRow,
    column: &str,
    site: &str,
) -> Result<Option<Value>, Denial> {
    row.try_get::<Option<Value>, _>(column)
        .map_err(|error| db_error(error, site))
}

/// `sort_order` is a `FloatField` (`float8`): decode `f64` and render with
/// `.0` for integral values exactly like Python's `repr(float)`.
fn render_f64(value: f64) -> Value {
    serde_json::Number::from_f64(value).map_or(Value::Null, Value::Number)
}

fn render_uuid(value: &uuid::Uuid) -> Value {
    Value::String(value.to_string())
}

fn render_uuid_opt(value: &Option<uuid::Uuid>) -> Value {
    value.as_ref().map_or(Value::Null, render_uuid)
}

fn render_string_opt(value: &Option<String>) -> Value {
    value
        .as_ref()
        .map_or(Value::Null, |v| Value::String(v.clone()))
}

fn render_date_opt(value: &Option<chrono::NaiveDate>) -> Value {
    value
        .map(|d| Value::String(d.format("%Y-%m-%d").to_string()))
        .unwrap_or(Value::Null)
}

/// DRF `DateTimeField` rendering in the request time zone (`Z` for UTC,
/// six-digit micros only when nonzero): the shared
/// `crate::serializer::render_datetime_in` kernel.
fn render_datetime_opt(value: &Option<chrono::DateTime<chrono::Utc>>, timezone: &Tz) -> Value {
    value.map_or(Value::Null, |dt| {
        Value::String(crate::serializer::render_datetime_in(&dt, timezone))
    })
}

// ---------------------------------------------------------------------------
// Module read shape
// ---------------------------------------------------------------------------

/// `ModuleSerializer` wire order (verified against live Django):
/// DRF's `get_default_field_names` (declared fields, then concrete
/// non-relational fields in model order, then forward relations in model
/// order), with `members` appended by `to_representation`
/// (`serializers/module.py:203-206`). The six metric keys are present only
/// when the instance carries the list annotations.
pub const MODULE_READ_ORDER: [&str; 29] = [
    "id",
    "total_issues",
    "cancelled_issues",
    "completed_issues",
    "started_issues",
    "unstarted_issues",
    "backlog_issues",
    "created_at",
    "updated_at",
    "deleted_at",
    "name",
    "description",
    "description_text",
    "description_html",
    "start_date",
    "target_date",
    "status",
    "view_props",
    "sort_order",
    "external_source",
    "external_id",
    "archived_at",
    "logo_props",
    "created_by",
    "updated_by",
    "project",
    "workspace",
    "lead",
    "members",
];

/// The six annotation values of a list/detail/archived row, in wire order.
/// Each grouped count is `COUNT(DISTINCT "states"."group")` (0 or 1, never
/// a real count — the ported aggregation bug).
#[derive(Debug, Clone, Copy)]
pub struct ModuleAnnotations {
    pub total: i64,
    pub cancelled: i64,
    pub completed: i64,
    pub started: i64,
    pub unstarted: i64,
    pub backlog: i64,
}

/// Decoded module row: the 24 `module::Module::COLUMNS` plus optional list
/// annotations (`None` for bare re-reads, whose missing attributes DRF
/// skips per field).
#[derive(Debug, Clone)]
pub struct ModuleDetail {
    pub id: uuid::Uuid,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub created_by: Option<uuid::Uuid>,
    pub updated_by: Option<uuid::Uuid>,
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    pub project_id: uuid::Uuid,
    pub workspace_id: uuid::Uuid,
    pub name: String,
    pub description: String,
    pub description_text: Option<Value>,
    pub description_html: Option<Value>,
    pub start_date: Option<chrono::NaiveDate>,
    pub target_date: Option<chrono::NaiveDate>,
    pub status: String,
    pub lead_id: Option<uuid::Uuid>,
    pub view_props: Value,
    pub sort_order: f64,
    pub external_source: Option<String>,
    pub external_id: Option<String>,
    pub archived_at: Option<chrono::DateTime<chrono::Utc>>,
    pub logo_props: Value,
    pub annotations: Option<ModuleAnnotations>,
}

impl ModuleDetail {
    pub fn decode(row: &sqlx::postgres::PgRow, site: &str) -> Result<Self, Denial> {
        Ok(ModuleDetail {
            id: row_uuid(row, "id", site)?,
            created_at: row_datetime(row, "created_at", site)?,
            updated_at: row_datetime(row, "updated_at", site)?,
            created_by: row_uuid_opt(row, "created_by_id", site)?,
            updated_by: row_uuid_opt(row, "updated_by_id", site)?,
            deleted_at: row_datetime_opt(row, "deleted_at", site)?,
            project_id: row_uuid(row, "project_id", site)?,
            workspace_id: row_uuid(row, "workspace_id", site)?,
            name: row_string(row, "name", site)?,
            description: row_string(row, "description", site)?,
            description_text: row_json_opt(row, "description_text", site)?,
            description_html: row_json_opt(row, "description_html", site)?,
            start_date: row_date_opt(row, "start_date", site)?,
            target_date: row_date_opt(row, "target_date", site)?,
            status: row_string(row, "status", site)?,
            lead_id: row_uuid_opt(row, "lead_id", site)?,
            view_props: row_json(row, "view_props", site)?,
            sort_order: row
                .try_get::<f64, _>("sort_order")
                .map_err(|error| db_error(error, site))?,
            external_source: row_string_opt(row, "external_source", site)?,
            external_id: row_string_opt(row, "external_id", site)?,
            archived_at: row_datetime_opt(row, "archived_at", site)?,
            logo_props: row_json(row, "logo_props", site)?,
            annotations: None,
        })
    }

    pub fn with_list_annotations(
        mut self,
        row: &sqlx::postgres::PgRow,
        site: &str,
    ) -> Result<Self, Denial> {
        self.annotations = Some(ModuleAnnotations {
            total: row_i64(row, "total_issues", site)?,
            cancelled: row_i64(row, "cancelled_issues", site)?,
            completed: row_i64(row, "completed_issues", site)?,
            started: row_i64(row, "started_issues", site)?,
            unstarted: row_i64(row, "unstarted_issues", site)?,
            backlog: row_i64(row, "backlog_issues", site)?,
        });
        Ok(self)
    }
}

/// Render one module in `ModuleSerializer` wire order
/// (`serializers/module.py:172-206`). `members` is always appended, even
/// under `?fields=` (the `to_representation` quirk); `?expand=` applies to
/// the filtered keys before `members` is appended, so `expand=members` is
/// silently undone (verified live).
#[allow(clippy::too_many_arguments)]
pub async fn render_module(
    pool: &PgPool,
    detail: &ModuleDetail,
    members: &[uuid::Uuid],
    timezone: &Tz,
    fields: Option<&[String]>,
    expand: Option<&[String]>,
) -> Result<Value, Denial> {
    let mut map = serde_json::Map::with_capacity(30);
    if let Some(ann) = detail.annotations {
        map.insert("id".to_owned(), render_uuid(&detail.id));
        map.insert("total_issues".to_owned(), Value::from(ann.total));
        map.insert("cancelled_issues".to_owned(), Value::from(ann.cancelled));
        map.insert("completed_issues".to_owned(), Value::from(ann.completed));
        map.insert("started_issues".to_owned(), Value::from(ann.started));
        map.insert("unstarted_issues".to_owned(), Value::from(ann.unstarted));
        map.insert("backlog_issues".to_owned(), Value::from(ann.backlog));
    } else {
        map.insert("id".to_owned(), render_uuid(&detail.id));
    }
    map.insert(
        "created_at".to_owned(),
        Value::String(crate::serializer::render_datetime_in(
            &detail.created_at,
            timezone,
        )),
    );
    map.insert(
        "updated_at".to_owned(),
        Value::String(crate::serializer::render_datetime_in(
            &detail.updated_at,
            timezone,
        )),
    );
    map.insert(
        "deleted_at".to_owned(),
        render_datetime_opt(&detail.deleted_at, timezone),
    );
    map.insert("name".to_owned(), Value::String(detail.name.clone()));
    map.insert(
        "description".to_owned(),
        Value::String(detail.description.clone()),
    );
    map.insert(
        "description_text".to_owned(),
        detail.description_text.clone().unwrap_or(Value::Null),
    );
    map.insert(
        "description_html".to_owned(),
        detail.description_html.clone().unwrap_or(Value::Null),
    );
    map.insert("start_date".to_owned(), render_date_opt(&detail.start_date));
    map.insert(
        "target_date".to_owned(),
        render_date_opt(&detail.target_date),
    );
    map.insert("status".to_owned(), Value::String(detail.status.clone()));
    map.insert("view_props".to_owned(), detail.view_props.clone());
    map.insert("sort_order".to_owned(), render_f64(detail.sort_order));
    map.insert(
        "external_source".to_owned(),
        render_string_opt(&detail.external_source),
    );
    map.insert(
        "external_id".to_owned(),
        render_string_opt(&detail.external_id),
    );
    map.insert(
        "archived_at".to_owned(),
        render_datetime_opt(&detail.archived_at, timezone),
    );
    map.insert("logo_props".to_owned(), detail.logo_props.clone());
    map.insert("created_by".to_owned(), render_uuid_opt(&detail.created_by));
    map.insert("updated_by".to_owned(), render_uuid_opt(&detail.updated_by));
    map.insert("project".to_owned(), render_uuid(&detail.project_id));
    map.insert("workspace".to_owned(), render_uuid(&detail.workspace_id));
    map.insert("lead".to_owned(), render_uuid_opt(&detail.lead_id));
    // `BaseSerializer._filter_fields` (`serializers/base.py:121-128`):
    // keep only the requested keys (order preserved).
    if let Some(fields) = fields {
        map.retain(|key, _| fields.iter().any(|f| f == key));
    }
    // `BaseSerializer._expand_fields` (`serializers/base.py:130-143`):
    // expand runs on the filtered keys, before `members` is appended.
    if let Some(expand) = expand {
        apply_module_expand(pool, detail, &mut map, expand).await?;
    }
    // `ModuleSerializer.to_representation` always sets `members` last,
    // even under `?fields=` (`serializers/module.py:203-206`).
    map.insert(
        "members".to_owned(),
        Value::Array(
            members
                .iter()
                .map(|id| Value::String(id.to_string()))
                .collect(),
        ),
    );
    Ok(Value::Object(map))
}

/// `?expand=` for module rows: the `BaseSerializer.expansion` map hits
/// (`project`, `workspace`, `created_by`, `updated_by`; every other key,
/// including `lead`, falls through to
/// `getattr(instance, f"{expand}_id", None)`). A map hit on a null FK
/// renders `{}` (single-object `None`); `expand=members` is skipped here
/// because `to_representation` overwrites it with id strings anyway.
pub async fn apply_module_expand(
    pool: &PgPool,
    detail: &ModuleDetail,
    map: &mut serde_json::Map<String, Value>,
    expand: &[String],
) -> Result<(), Denial> {
    for name in expand {
        if !map.contains_key(name.as_str()) {
            continue;
        }
        match name.as_str() {
            "project" => {
                map.insert(
                    "project".to_owned(),
                    expand_project(pool, &detail.project_id).await?,
                );
            }
            "workspace" => {
                map.insert(
                    "workspace".to_owned(),
                    expand_workspace(pool, &detail.workspace_id).await?,
                );
            }
            "created_by" => {
                map.insert(
                    "created_by".to_owned(),
                    expand_user_opt(pool, &detail.created_by).await?,
                );
            }
            "updated_by" => {
                map.insert(
                    "updated_by".to_owned(),
                    expand_user_opt(pool, &detail.updated_by).await?,
                );
            }
            "members" => {
                // Expanded then overwritten with id strings by
                // `ModuleSerializer.to_representation`; skip the query.
            }
            _ => {
                // `getattr(instance, f"{expand}_id", None)`: only `lead`
                // resolves (to the unchanged id); everything else nulls
                // the key (verified live: `expand=total_issues` renders
                // the metric null).
                if name == "lead" {
                    map.insert("lead".to_owned(), render_uuid_opt(&detail.lead_id));
                } else {
                    map.insert(name.clone(), Value::Null);
                }
            }
        }
    }
    Ok(())
}

/// The PATCH 200 body: `ModuleUpdateSerializer.data`
/// (`views/module.py:449`) — the 8 write fields in `Meta.fields` order,
/// no `id`.
pub fn render_module_update_shape(detail: &ModuleDetail) -> Value {
    let mut map = serde_json::Map::with_capacity(8);
    map.insert("name".to_owned(), Value::String(detail.name.clone()));
    map.insert(
        "description".to_owned(),
        Value::String(detail.description.clone()),
    );
    map.insert("start_date".to_owned(), render_date_opt(&detail.start_date));
    map.insert(
        "target_date".to_owned(),
        render_date_opt(&detail.target_date),
    );
    map.insert("status".to_owned(), Value::String(detail.status.clone()));
    map.insert("lead".to_owned(), render_uuid_opt(&detail.lead_id));
    map.insert(
        "external_source".to_owned(),
        render_string_opt(&detail.external_source),
    );
    map.insert(
        "external_id".to_owned(),
        render_string_opt(&detail.external_id),
    );
    Value::Object(map)
}

// ---------------------------------------------------------------------------
// ModuleIssue (bridge) read shape
// ---------------------------------------------------------------------------

/// `ModuleIssueSerializer` wire order (verified live): declared `id` +
/// `sub_issues_count`, non-relational fields, forward relations.
#[derive(Debug, Clone)]
pub struct BridgeDetail {
    pub id: uuid::Uuid,
    pub sub_issues_count: i64,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_by: Option<uuid::Uuid>,
    pub updated_by: Option<uuid::Uuid>,
    pub project_id: uuid::Uuid,
    pub workspace_id: uuid::Uuid,
    pub module_id: uuid::Uuid,
    pub issue_id: uuid::Uuid,
}

impl BridgeDetail {
    pub fn decode(row: &sqlx::postgres::PgRow, site: &str) -> Result<Self, Denial> {
        Ok(BridgeDetail {
            id: row_uuid(row, "id", site)?,
            sub_issues_count: row_i64(row, "sub_issues_count", site)?,
            created_at: row_datetime(row, "created_at", site)?,
            updated_at: row_datetime(row, "updated_at", site)?,
            deleted_at: row_datetime_opt(row, "deleted_at", site)?,
            created_by: row_uuid_opt(row, "created_by_id", site)?,
            updated_by: row_uuid_opt(row, "updated_by_id", site)?,
            project_id: row_uuid(row, "project_id", site)?,
            workspace_id: row_uuid(row, "workspace_id", site)?,
            module_id: row_uuid(row, "module_id", site)?,
            issue_id: row_uuid(row, "issue_id", site)?,
        })
    }
}

/// Render one bridge row. Neither bridge call site passes `fields`/`expand`
/// (list queryset `ModuleIssueSerializer(queryset, many=True)` and the POST
/// response both omit them), so no filtering applies.
pub fn render_bridge(detail: &BridgeDetail, timezone: &Tz) -> Value {
    let mut map = serde_json::Map::with_capacity(11);
    map.insert("id".to_owned(), render_uuid(&detail.id));
    map.insert(
        "sub_issues_count".to_owned(),
        Value::from(detail.sub_issues_count),
    );
    map.insert(
        "created_at".to_owned(),
        Value::String(crate::serializer::render_datetime_in(
            &detail.created_at,
            timezone,
        )),
    );
    map.insert(
        "updated_at".to_owned(),
        Value::String(crate::serializer::render_datetime_in(
            &detail.updated_at,
            timezone,
        )),
    );
    map.insert(
        "deleted_at".to_owned(),
        render_datetime_opt(&detail.deleted_at, timezone),
    );
    map.insert("created_by".to_owned(), render_uuid_opt(&detail.created_by));
    map.insert("updated_by".to_owned(), render_uuid_opt(&detail.updated_by));
    map.insert("project".to_owned(), render_uuid(&detail.project_id));
    map.insert("workspace".to_owned(), render_uuid(&detail.workspace_id));
    map.insert("module".to_owned(), render_uuid(&detail.module_id));
    map.insert("issue".to_owned(), render_uuid(&detail.issue_id));
    Value::Object(map)
}

// ---------------------------------------------------------------------------
// Issue read shape (module-issues GET)
// ---------------------------------------------------------------------------

/// Decoded issue row for the module-issues GET: `IssueSerializer`
/// (`api/serializers/issue.py:109-261`) with `Meta.exclude =
/// [description_json, description_stripped, workpad]`, rendered through the
/// same DRF field order (declared first, then concrete non-relational in
/// model order, then forward relations in model order). The M4 GET
/// annotations (`sub_issues_count`, `bridge_id`, `link_count`,
/// `attachment_count`) exist only for ordering; the serializer never reads
/// them.
#[derive(Debug, Clone)]
pub struct IssueDetail {
    pub id: uuid::Uuid,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    pub point: Option<i32>,
    pub name: String,
    pub description_html: String,
    pub description_binary: Option<Vec<u8>>,
    pub priority: Option<String>,
    pub complexity_score: Option<i32>,
    pub start_date: Option<chrono::NaiveDate>,
    pub target_date: Option<chrono::NaiveDate>,
    pub sequence_id: i32,
    pub sort_order: f64,
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
    /// `archived_at` on `Issue` is a `DateField`, not a datetime.
    pub archived_at: Option<chrono::NaiveDate>,
    pub is_draft: bool,
    pub external_source: Option<String>,
    pub external_id: Option<String>,
    pub git_work_branch: String,
    pub created_via: Option<String>,
    pub agent_executor: Option<String>,
    pub created_by: Option<uuid::Uuid>,
    pub updated_by: Option<uuid::Uuid>,
    pub project_id: uuid::Uuid,
    pub workspace_id: uuid::Uuid,
    pub parent_id: Option<uuid::Uuid>,
    pub state_id: Option<uuid::Uuid>,
    pub estimate_point_id: Option<uuid::Uuid>,
    pub type_id: Option<uuid::Uuid>,
    pub assigned_pod_id: Option<uuid::Uuid>,
}

impl IssueDetail {
    pub fn decode(row: &sqlx::postgres::PgRow, site: &str) -> Result<Self, Denial> {
        Ok(IssueDetail {
            id: row_uuid(row, "id", site)?,
            created_at: row_datetime(row, "created_at", site)?,
            updated_at: row_datetime(row, "updated_at", site)?,
            deleted_at: row_datetime_opt(row, "deleted_at", site)?,
            point: row_i32_opt(row, "point", site)?,
            name: row_string(row, "name", site)?,
            description_html: row_string(row, "description_html", site)?,
            description_binary: row
                .try_get::<Option<Vec<u8>>, _>("description_binary")
                .map_err(|error| db_error(error, site))?,
            priority: row_string_opt(row, "priority", site)?,
            complexity_score: row_i32_opt(row, "complexity_score", site)?,
            start_date: row_date_opt(row, "start_date", site)?,
            target_date: row_date_opt(row, "target_date", site)?,
            sequence_id: row
                .try_get::<i32, _>("sequence_id")
                .map_err(|error| db_error(error, site))?,
            sort_order: row
                .try_get::<f64, _>("sort_order")
                .map_err(|error| db_error(error, site))?,
            completed_at: row_datetime_opt(row, "completed_at", site)?,
            archived_at: row_date_opt(row, "archived_at", site)?,
            is_draft: row
                .try_get::<bool, _>("is_draft")
                .map_err(|error| db_error(error, site))?,
            external_source: row_string_opt(row, "external_source", site)?,
            external_id: row_string_opt(row, "external_id", site)?,
            git_work_branch: row_string(row, "git_work_branch", site)?,
            created_via: row_string_opt(row, "created_via", site)?,
            agent_executor: row_string_opt(row, "agent_executor", site)?,
            created_by: row_uuid_opt(row, "created_by_id", site)?,
            updated_by: row_uuid_opt(row, "updated_by_id", site)?,
            project_id: row_uuid(row, "project_id", site)?,
            workspace_id: row_uuid(row, "workspace_id", site)?,
            parent_id: row_uuid_opt(row, "parent_id", site)?,
            state_id: row_uuid_opt(row, "state_id", site)?,
            estimate_point_id: row_uuid_opt(row, "estimate_point_id", site)?,
            type_id: row_uuid_opt(row, "type_id", site)?,
            assigned_pod_id: row_uuid_opt(row, "assigned_pod_id", site)?,
        })
    }
}

/// Render one issue in `IssueSerializer` wire order (verified live):
/// `id`, declared `type_id` + `url` (omitted when no base URL is
/// configured), concrete non-relational fields in model order, forward
/// relations in model order, then `assignees`/`labels` id lists appended
/// by `to_representation` only when the keys survived filtering
/// (`serializers/issue.py:242-248`).
#[allow(clippy::too_many_arguments)]
pub async fn render_issue(
    pool: &PgPool,
    detail: &IssueDetail,
    assignees: &[uuid::Uuid],
    labels: &[uuid::Uuid],
    url: Option<String>,
    timezone: &Tz,
    fields: Option<&[String]>,
    expand: Option<&[String]>,
) -> Result<Value, Denial> {
    let mut map = serde_json::Map::with_capacity(36);
    map.insert("id".to_owned(), render_uuid(&detail.id));
    map.insert("type_id".to_owned(), render_uuid_opt(&detail.type_id));
    if let Some(url) = url {
        map.insert("url".to_owned(), Value::String(url));
    }
    map.insert(
        "created_at".to_owned(),
        Value::String(crate::serializer::render_datetime_in(
            &detail.created_at,
            timezone,
        )),
    );
    map.insert(
        "updated_at".to_owned(),
        Value::String(crate::serializer::render_datetime_in(
            &detail.updated_at,
            timezone,
        )),
    );
    map.insert(
        "deleted_at".to_owned(),
        render_datetime_opt(&detail.deleted_at, timezone),
    );
    map.insert(
        "point".to_owned(),
        detail.point.map_or(Value::Null, Value::from),
    );
    map.insert("name".to_owned(), Value::String(detail.name.clone()));
    map.insert(
        "description_html".to_owned(),
        Value::String(detail.description_html.clone()),
    );
    // `description_binary` maps to DRF's `ModelField`, whose
    // `to_representation` answers `BinaryField.value_to_string` — standard
    // base64 ASCII (verified live: 200, never a 500). `None` renders null.
    map.insert(
        "description_binary".to_owned(),
        detail
            .description_binary
            .as_ref()
            .map_or(Value::Null, |bytes| {
                use base64::Engine as _;
                Value::String(base64::engine::general_purpose::STANDARD.encode(bytes))
            }),
    );
    map.insert("priority".to_owned(), render_string_opt(&detail.priority));
    map.insert(
        "complexity_score".to_owned(),
        detail.complexity_score.map_or(Value::Null, Value::from),
    );
    map.insert("start_date".to_owned(), render_date_opt(&detail.start_date));
    map.insert(
        "target_date".to_owned(),
        render_date_opt(&detail.target_date),
    );
    map.insert("sequence_id".to_owned(), Value::from(detail.sequence_id));
    map.insert("sort_order".to_owned(), render_f64(detail.sort_order));
    map.insert(
        "completed_at".to_owned(),
        render_datetime_opt(&detail.completed_at, timezone),
    );
    map.insert(
        "archived_at".to_owned(),
        render_date_opt(&detail.archived_at),
    );
    map.insert("is_draft".to_owned(), Value::Bool(detail.is_draft));
    map.insert(
        "external_source".to_owned(),
        render_string_opt(&detail.external_source),
    );
    map.insert(
        "external_id".to_owned(),
        render_string_opt(&detail.external_id),
    );
    map.insert(
        "git_work_branch".to_owned(),
        Value::String(detail.git_work_branch.clone()),
    );
    map.insert(
        "created_via".to_owned(),
        render_string_opt(&detail.created_via),
    );
    map.insert(
        "agent_executor".to_owned(),
        render_string_opt(&detail.agent_executor),
    );
    map.insert("created_by".to_owned(), render_uuid_opt(&detail.created_by));
    map.insert("updated_by".to_owned(), render_uuid_opt(&detail.updated_by));
    map.insert("project".to_owned(), render_uuid(&detail.project_id));
    map.insert("workspace".to_owned(), render_uuid(&detail.workspace_id));
    map.insert("parent".to_owned(), render_uuid_opt(&detail.parent_id));
    map.insert("state".to_owned(), render_uuid_opt(&detail.state_id));
    map.insert(
        "estimate_point".to_owned(),
        render_uuid_opt(&detail.estimate_point_id),
    );
    map.insert("type".to_owned(), render_uuid_opt(&detail.type_id));
    map.insert(
        "assigned_pod".to_owned(),
        render_uuid_opt(&detail.assigned_pod_id),
    );
    if let Some(fields) = fields {
        map.retain(|key, _| fields.iter().any(|f| f == key));
    }
    if let Some(expand) = expand {
        apply_issue_expand(pool, detail, &mut map, expand, timezone).await?;
    }
    // Unlike modules, the issue appends are conditional on the keys
    // surviving filtering (`serializers/issue.py:242-248`).
    if let Some(fields) = fields {
        if fields.iter().any(|f| f == "assignees") {
            map.insert(
                "assignees".to_owned(),
                Value::Array(
                    assignees
                        .iter()
                        .map(|id| Value::String(id.to_string()))
                        .collect(),
                ),
            );
        }
        if fields.iter().any(|f| f == "labels") {
            map.insert(
                "labels".to_owned(),
                Value::Array(
                    labels
                        .iter()
                        .map(|id| Value::String(id.to_string()))
                        .collect(),
                ),
            );
        }
    } else {
        map.insert(
            "assignees".to_owned(),
            Value::Array(
                assignees
                    .iter()
                    .map(|id| Value::String(id.to_string()))
                    .collect(),
            ),
        );
        map.insert(
            "labels".to_owned(),
            Value::Array(
                labels
                    .iter()
                    .map(|id| Value::String(id.to_string()))
                    .collect(),
            ),
        );
    }
    // `?expand=assignees|labels` renders the full UserLite / Label lists
    // (verified live); applied after the id-list appends.
    if let Some(expand) = expand {
        apply_issue_list_expand(pool, detail, &mut map, expand, timezone).await?;
    }
    Ok(Value::Object(map))
}

/// `?expand=` for scalar issue keys: map hits render the Lite shapes (null
/// FK → `{}`); every other key falls through to
/// `getattr(instance, f"{expand}_id", None)` — `type` resolves to the
/// unchanged id, everything else nulls the key.
pub async fn apply_issue_expand(
    pool: &PgPool,
    detail: &IssueDetail,
    map: &mut serde_json::Map<String, Value>,
    expand: &[String],
    timezone: &Tz,
) -> Result<(), Denial> {
    for name in expand {
        if !map.contains_key(name.as_str()) {
            continue;
        }
        match name.as_str() {
            "state" => {
                map.insert(
                    "state".to_owned(),
                    expand_state_opt(pool, &detail.state_id).await?,
                );
            }
            "project" => {
                map.insert(
                    "project".to_owned(),
                    expand_project(pool, &detail.project_id).await?,
                );
            }
            "workspace" => {
                map.insert(
                    "workspace".to_owned(),
                    expand_workspace(pool, &detail.workspace_id).await?,
                );
            }
            "parent" => {
                map.insert(
                    "parent".to_owned(),
                    expand_issue_lite_opt(pool, &detail.parent_id).await?,
                );
            }
            "created_by" => {
                map.insert(
                    "created_by".to_owned(),
                    expand_user_opt(pool, &detail.created_by).await?,
                );
            }
            "updated_by" => {
                map.insert(
                    "updated_by".to_owned(),
                    expand_user_opt(pool, &detail.updated_by).await?,
                );
            }
            "estimate_point" => {
                map.insert(
                    "estimate_point".to_owned(),
                    expand_estimate_point_opt(pool, &detail.estimate_point_id, timezone).await?,
                );
            }
            "assignees" | "labels" => {
                // Handled after the id-list appends (see `render_issue`).
            }
            "type" => {
                // Fallthrough `getattr(instance, "type_id")`: the unchanged id.
                map.insert("type".to_owned(), render_uuid_opt(&detail.type_id));
            }
            "assigned_pod" => {
                // Fallthrough `getattr(instance, "assigned_pod_id")`: the
                // column exists, so Django re-renders the unchanged id.
                map.insert(
                    "assigned_pod".to_owned(),
                    render_uuid_opt(&detail.assigned_pod_id),
                );
            }
            _ => {
                map.insert(name.clone(), Value::Null);
            }
        }
    }
    Ok(())
}

/// `?expand=assignees|labels` after the id lists are appended: full UserLite
/// / Label lists (verified live).
pub async fn apply_issue_list_expand(
    pool: &PgPool,
    detail: &IssueDetail,
    map: &mut serde_json::Map<String, Value>,
    expand: &[String],
    timezone: &Tz,
) -> Result<(), Denial> {
    for name in expand {
        if !map.contains_key(name.as_str()) {
            continue;
        }
        match name.as_str() {
            "assignees" => {
                map.insert(
                    "assignees".to_owned(),
                    expand_assignees(pool, &detail.id).await?,
                );
            }
            "labels" => {
                map.insert(
                    "labels".to_owned(),
                    expand_labels(pool, &detail.id, timezone).await?,
                );
            }
            _ => {}
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Expand (Lite) shapes
// ---------------------------------------------------------------------------

/// `FileAsset.asset_url` for one asset row (same resolution as the D-19
/// project handlers, which own this helper per file).
pub async fn file_asset_url(
    pool: &PgPool,
    asset_id: &uuid::Uuid,
) -> Result<Option<String>, Denial> {
    let row: Option<FileAssetLookup> = sqlx::query_as(
        r#"SELECT fa."entity_type", fa."workspace_id", fa."project_id", fa."issue_id" FROM "file_assets" fa WHERE fa."id" = $1"#,
    )
    .bind(asset_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| db_error(error, "file-asset"))?;
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
                        .map_err(|error| db_error(error, "file-asset-workspace"))?
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
                        .map_err(|error| db_error(error, "file-asset-workspace"))?
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

/// `avatar_url` property, text arm (`db/models/user.py:149-151`): the
/// `avatar` text when non-empty, else null. (The asset arm is DB-bound and
/// covered by the live differential battery, not unit tests.)
pub fn render_avatar_text(avatar: &str) -> Value {
    if avatar.is_empty() {
        Value::Null
    } else {
        Value::String(avatar.to_owned())
    }
}

/// `UserLiteSerializer` (`api/serializers/user.py`): `id`, `first_name`,
/// `last_name`, `email`, `avatar`, `avatar_url`, `display_name` (the
/// duplicated `email` in `Meta.fields` renders once).
///
/// `avatar_url` is a model property, not a column
/// (`db/models/user.py:142-151`): the asset URL when `avatar_asset` is set
/// (returned as-is, even when the asset type maps to no URL), else the
/// `avatar` text when non-empty, else null. `email` is nullable
/// (`CharField(null=True)`).
pub async fn expand_user(pool: &PgPool, user_id: &uuid::Uuid) -> Result<Value, Denial> {
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(
        r#"SELECT "id", "first_name", "last_name", "email", "avatar", "avatar_asset_id", "display_name" FROM "users" WHERE "id" = $1"#,
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| db_error(error, "expand-user"))?;
    let Some(row) = row else {
        return Ok(Value::Object(serde_json::Map::new()));
    };
    let avatar_asset_id = row_uuid_opt(&row, "avatar_asset_id", "expand-user")?;
    let avatar = row_string(&row, "avatar", "expand-user")?;
    let avatar_url = match avatar_asset_id {
        Some(asset_id) => file_asset_url(pool, &asset_id)
            .await?
            .map(Value::String)
            .unwrap_or(Value::Null),
        None => render_avatar_text(&avatar),
    };
    let mut map = serde_json::Map::with_capacity(7);
    map.insert(
        "id".to_owned(),
        render_uuid(&row_uuid(&row, "id", "expand-user")?),
    );
    map.insert(
        "first_name".to_owned(),
        Value::String(row_string(&row, "first_name", "expand-user")?),
    );
    map.insert(
        "last_name".to_owned(),
        Value::String(row_string(&row, "last_name", "expand-user")?),
    );
    map.insert(
        "email".to_owned(),
        render_string_opt(&row_string_opt(&row, "email", "expand-user")?),
    );
    map.insert(
        "avatar".to_owned(),
        Value::String(row_string(&row, "avatar", "expand-user")?),
    );
    map.insert("avatar_url".to_owned(), avatar_url);
    map.insert(
        "display_name".to_owned(),
        Value::String(row_string(&row, "display_name", "expand-user")?),
    );
    Ok(Value::Object(map))
}

/// Map hit on a null FK renders `{}` (verified live).
pub async fn expand_user_opt(pool: &PgPool, user_id: &Option<uuid::Uuid>) -> Result<Value, Denial> {
    match user_id {
        None => Ok(Value::Object(serde_json::Map::new())),
        Some(id) => expand_user(pool, id).await,
    }
}

/// `WorkspaceLiteSerializer`: `name`, `slug`, `id`.
pub async fn expand_workspace(pool: &PgPool, workspace_id: &uuid::Uuid) -> Result<Value, Denial> {
    let row: Option<sqlx::postgres::PgRow> =
        sqlx::query(r#"SELECT "name", "slug", "id" FROM "workspaces" WHERE "id" = $1"#)
            .bind(workspace_id)
            .fetch_optional(pool)
            .await
            .map_err(|error| db_error(error, "expand-workspace"))?;
    let Some(row) = row else {
        return Ok(Value::Object(serde_json::Map::new()));
    };
    let mut map = serde_json::Map::with_capacity(3);
    map.insert(
        "name".to_owned(),
        Value::String(row_string(&row, "name", "expand-workspace")?),
    );
    map.insert(
        "slug".to_owned(),
        Value::String(row_string(&row, "slug", "expand-workspace")?),
    );
    map.insert(
        "id".to_owned(),
        render_uuid(&row_uuid(&row, "id", "expand-workspace")?),
    );
    Ok(Value::Object(map))
}

/// `ProjectLiteSerializer`: `id`, `identifier`, `name`, `cover_image`,
/// `icon_prop`, `emoji`, `description`, `is_default`, `cover_image_url`.
pub async fn expand_project(pool: &PgPool, project_id: &uuid::Uuid) -> Result<Value, Denial> {
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(
        r#"SELECT "id", "identifier", "name", "cover_image", "cover_image_asset_id", "icon_prop", "emoji", "description", "is_default" FROM "projects" WHERE "id" = $1"#,
    )
    .bind(project_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| db_error(error, "expand-project"))?;
    let Some(row) = row else {
        return Ok(Value::Object(serde_json::Map::new()));
    };
    let asset_id = row_uuid_opt(&row, "cover_image_asset_id", "expand-project")?;
    let mut cover_image_url = Value::Null;
    if let Some(asset_id) = asset_id {
        cover_image_url = file_asset_url(pool, &asset_id)
            .await?
            .map(Value::String)
            .unwrap_or(Value::Null);
    }
    let mut map = serde_json::Map::with_capacity(9);
    map.insert(
        "id".to_owned(),
        render_uuid(&row_uuid(&row, "id", "expand-project")?),
    );
    map.insert(
        "identifier".to_owned(),
        Value::String(row_string(&row, "identifier", "expand-project")?),
    );
    map.insert(
        "name".to_owned(),
        Value::String(row_string(&row, "name", "expand-project")?),
    );
    map.insert(
        "cover_image".to_owned(),
        render_string_opt(&row_string_opt(&row, "cover_image", "expand-project")?),
    );
    map.insert(
        "icon_prop".to_owned(),
        row_json_opt(&row, "icon_prop", "expand-project")?.unwrap_or(Value::Null),
    );
    map.insert(
        "emoji".to_owned(),
        row_json_opt(&row, "emoji", "expand-project")?.unwrap_or(Value::Null),
    );
    map.insert(
        "description".to_owned(),
        Value::String(row_string(&row, "description", "expand-project")?),
    );
    map.insert(
        "is_default".to_owned(),
        Value::Bool(
            row.try_get::<bool, _>("is_default")
                .map_err(|error| db_error(error, "expand-project"))?,
        ),
    );
    map.insert("cover_image_url".to_owned(), cover_image_url);
    Ok(Value::Object(map))
}

/// `StateLiteSerializer` for a nullable state: null renders `{}`.
pub async fn expand_state_opt(
    pool: &PgPool,
    state_id: &Option<uuid::Uuid>,
) -> Result<Value, Denial> {
    match state_id {
        None => Ok(Value::Object(serde_json::Map::new())),
        Some(id) => expand_state(pool, id).await,
    }
}

/// `StateLiteSerializer`: `id`, `name`, `color`, `group`.
pub async fn expand_state(pool: &PgPool, state_id: &uuid::Uuid) -> Result<Value, Denial> {
    let row: Option<sqlx::postgres::PgRow> =
        sqlx::query(r#"SELECT "id", "name", "color", "group" FROM "states" WHERE "id" = $1"#)
            .bind(state_id)
            .fetch_optional(pool)
            .await
            .map_err(|error| db_error(error, "expand-state"))?;
    let Some(row) = row else {
        return Ok(Value::Object(serde_json::Map::new()));
    };
    let mut map = serde_json::Map::with_capacity(4);
    map.insert(
        "id".to_owned(),
        render_uuid(&row_uuid(&row, "id", "expand-state")?),
    );
    map.insert(
        "name".to_owned(),
        Value::String(row_string(&row, "name", "expand-state")?),
    );
    map.insert(
        "color".to_owned(),
        Value::String(row_string(&row, "color", "expand-state")?),
    );
    map.insert(
        "group".to_owned(),
        render_string_opt(&row_string_opt(&row, "group", "expand-state")?),
    );
    Ok(Value::Object(map))
}

/// Render one `LabelSerializer` row (`__all__`, verified live): the full
/// label row with `workspace` before `project` (the `WorkspaceBaseModel`
/// field order), datetimes in the request time zone.
pub fn render_label_row(row: &sqlx::postgres::PgRow, timezone: &Tz) -> Result<Value, Denial> {
    let mut map = serde_json::Map::with_capacity(15);
    map.insert(
        "id".to_owned(),
        render_uuid(&row_uuid(row, "id", "expand-label")?),
    );
    map.insert(
        "created_at".to_owned(),
        Value::String(crate::serializer::render_datetime_in(
            &row_datetime(row, "created_at", "expand-label")?,
            timezone,
        )),
    );
    map.insert(
        "updated_at".to_owned(),
        Value::String(crate::serializer::render_datetime_in(
            &row_datetime(row, "updated_at", "expand-label")?,
            timezone,
        )),
    );
    map.insert(
        "deleted_at".to_owned(),
        render_datetime_opt(
            &row_datetime_opt(row, "deleted_at", "expand-label")?,
            timezone,
        ),
    );
    map.insert(
        "name".to_owned(),
        Value::String(row_string(row, "name", "expand-label")?),
    );
    map.insert(
        "description".to_owned(),
        Value::String(row_string(row, "description", "expand-label")?),
    );
    map.insert(
        "color".to_owned(),
        Value::String(row_string(row, "color", "expand-label")?),
    );
    map.insert(
        "sort_order".to_owned(),
        render_f64(
            row.try_get::<f64, _>("sort_order")
                .map_err(|error| db_error(error, "expand-label"))?,
        ),
    );
    map.insert(
        "external_source".to_owned(),
        render_string_opt(&row_string_opt(row, "external_source", "expand-label")?),
    );
    map.insert(
        "external_id".to_owned(),
        render_string_opt(&row_string_opt(row, "external_id", "expand-label")?),
    );
    map.insert(
        "created_by".to_owned(),
        render_uuid_opt(&row_uuid_opt(row, "created_by_id", "expand-label")?),
    );
    map.insert(
        "updated_by".to_owned(),
        render_uuid_opt(&row_uuid_opt(row, "updated_by_id", "expand-label")?),
    );
    map.insert(
        "workspace".to_owned(),
        render_uuid(&row_uuid(row, "workspace_id", "expand-label")?),
    );
    map.insert(
        "project".to_owned(),
        render_uuid_opt(&row_uuid_opt(row, "project_id", "expand-label")?),
    );
    map.insert(
        "parent".to_owned(),
        render_uuid_opt(&row_uuid_opt(row, "parent_id", "expand-label")?),
    );
    Ok(Value::Object(map))
}

/// `IssueLiteSerializer`: `id`, `sequence_id`, `project_id` (the raw attname).
pub async fn expand_issue_lite_opt(
    pool: &PgPool,
    issue_id: &Option<uuid::Uuid>,
) -> Result<Value, Denial> {
    let Some(issue_id) = issue_id else {
        return Ok(Value::Object(serde_json::Map::new()));
    };
    let row: Option<sqlx::postgres::PgRow> =
        sqlx::query(r#"SELECT "id", "sequence_id", "project_id" FROM "issues" WHERE "id" = $1"#)
            .bind(issue_id)
            .fetch_optional(pool)
            .await
            .map_err(|error| db_error(error, "expand-issuelite"))?;
    let Some(row) = row else {
        return Ok(Value::Object(serde_json::Map::new()));
    };
    let mut map = serde_json::Map::with_capacity(3);
    map.insert(
        "id".to_owned(),
        render_uuid(&row_uuid(&row, "id", "expand-issuelite")?),
    );
    map.insert(
        "sequence_id".to_owned(),
        Value::from(row_i32(&row, "sequence_id", "expand-issuelite")?),
    );
    map.insert(
        "project_id".to_owned(),
        render_uuid(&row_uuid(&row, "project_id", "expand-issuelite")?),
    );
    Ok(Value::Object(map))
}

/// `EstimatePointSerializer(None).data` (verified live): the null-FK
/// expand renders these six fields in this order — `id`, the timestamps,
/// and the relations are skipped.
pub fn estimate_point_none() -> Value {
    let mut map = serde_json::Map::with_capacity(6);
    map.insert("deleted_at".to_owned(), Value::Null);
    map.insert("key".to_owned(), Value::Null);
    map.insert("description".to_owned(), Value::String(String::new()));
    map.insert("value".to_owned(), Value::String(String::new()));
    map.insert("created_by".to_owned(), Value::Null);
    map.insert("updated_by".to_owned(), Value::Null);
    Value::Object(map)
}

/// `EstimatePointSerializer` (`__all__`): full row, declared `id` first,
/// then concrete fields, then forward relations.
pub async fn expand_estimate_point_opt(
    pool: &PgPool,
    estimate_point_id: &Option<uuid::Uuid>,
    timezone: &Tz,
) -> Result<Value, Denial> {
    let Some(estimate_point_id) = estimate_point_id else {
        // Unlike the Lite shapes (`{}`), the full `EstimatePointSerializer`
        // over `None` renders the fields that survive `get_attribute`
        // (verified live, in this order).
        return Ok(estimate_point_none());
    };
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(
        r#"SELECT "id", "created_at", "updated_at", "deleted_at", "key", "description",
                  "value", "created_by_id", "updated_by_id", "workspace_id", "project_id", "estimate_id"
           FROM "estimate_points" WHERE "id" = $1"#,
    )
    .bind(estimate_point_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| db_error(error, "expand-estimate"))?;
    let Some(row) = row else {
        return Ok(Value::Object(serde_json::Map::new()));
    };
    let mut map = serde_json::Map::with_capacity(12);
    map.insert(
        "id".to_owned(),
        render_uuid(&row_uuid(&row, "id", "expand-estimate")?),
    );
    map.insert(
        "created_at".to_owned(),
        Value::String(crate::serializer::render_datetime_in(
            &row_datetime(&row, "created_at", "expand-estimate")?,
            timezone,
        )),
    );
    map.insert(
        "updated_at".to_owned(),
        Value::String(crate::serializer::render_datetime_in(
            &row_datetime(&row, "updated_at", "expand-estimate")?,
            timezone,
        )),
    );
    map.insert(
        "deleted_at".to_owned(),
        render_datetime_opt(
            &row_datetime_opt(&row, "deleted_at", "expand-estimate")?,
            timezone,
        ),
    );
    // `key` is an `IntegerField` (`db/models/estimate.py:45`): a JSON
    // number, never null (no `null=True`).
    map.insert(
        "key".to_owned(),
        Value::from(row_i32(&row, "key", "expand-estimate")?),
    );
    map.insert(
        "description".to_owned(),
        Value::String(row_string(&row, "description", "expand-estimate")?),
    );
    map.insert(
        "value".to_owned(),
        Value::String(row_string(&row, "value", "expand-estimate")?),
    );
    map.insert(
        "created_by".to_owned(),
        render_uuid_opt(&row_uuid_opt(&row, "created_by_id", "expand-estimate")?),
    );
    map.insert(
        "updated_by".to_owned(),
        render_uuid_opt(&row_uuid_opt(&row, "updated_by_id", "expand-estimate")?),
    );
    // Relation order follows the model: `ProjectBaseModel` declares
    // `project` before `workspace`, then the child's own `estimate` FK
    // (`__all__` serializer order, verified live).
    map.insert(
        "project".to_owned(),
        render_uuid(&row_uuid(&row, "project_id", "expand-estimate")?),
    );
    map.insert(
        "workspace".to_owned(),
        render_uuid(&row_uuid(&row, "workspace_id", "expand-estimate")?),
    );
    map.insert(
        "estimate".to_owned(),
        render_uuid(&row_uuid(&row, "estimate_id", "expand-estimate")?),
    );
    Ok(Value::Object(map))
}

/// `?expand=assignees`: the issue's assignees as a UserLite list, in User
/// `-created_at` order (the `pk__in` queryset's default ordering).
pub async fn expand_assignees(pool: &PgPool, issue_id: &uuid::Uuid) -> Result<Value, Denial> {
    let ids: Vec<uuid::Uuid> = sqlx::query_scalar(
        r#"SELECT u."id" FROM "users" u
           WHERE u."id" IN (SELECT "assignee_id" FROM "issue_assignees" WHERE "issue_id" = $1 AND "deleted_at" IS NULL)
           ORDER BY u."created_at" DESC"#,
    )
    .bind(issue_id)
    .fetch_all(pool)
    .await
    .map_err(|error| db_error(error, "expand-assignees"))?;
    let mut items = Vec::with_capacity(ids.len());
    for id in &ids {
        items.push(expand_user(pool, id).await?);
    }
    Ok(Value::Array(items))
}

/// `?expand=labels`: the issue's labels as a full Label list, in Label
/// `-created_at` order.
pub async fn expand_labels(
    pool: &PgPool,
    issue_id: &uuid::Uuid,
    timezone: &Tz,
) -> Result<Value, Denial> {
    let rows: Vec<sqlx::postgres::PgRow> = sqlx::query(
        r#"SELECT l."id", l."created_at", l."updated_at", l."deleted_at", l."name", l."description", l."color",
                  l."sort_order", l."external_source", l."external_id", l."created_by_id", l."updated_by_id",
                  l."workspace_id", l."project_id", l."parent_id"
           FROM "labels" l
           WHERE l."id" IN (SELECT "label_id" FROM "issue_labels" WHERE "issue_id" = $1 AND "deleted_at" IS NULL)
           ORDER BY l."created_at" DESC"#,
    )
    .bind(issue_id)
    .fetch_all(pool)
    .await
    .map_err(|error| db_error(error, "expand-labels"))?;
    let mut items = Vec::with_capacity(rows.len());
    for row in &rows {
        items.push(render_label_row(row, timezone)?);
    }
    Ok(Value::Array(items))
}

// ---------------------------------------------------------------------------
// Request bodies + DRF field coercion
// ---------------------------------------------------------------------------

/// Parse the request body (`is_valid()` input stage): an empty body is `{}`,
/// malformed JSON is the DRF `ParseError` (with its `JSON parse error - `
/// prefix), and any non-object JSON value is the serializer
/// `non_field_errors` — with DRF's per-type names (`int`/`float`/`bool`/
/// `str`/`list`) and `null` answering `No data provided` (all verified live).
pub fn parse_body(raw: &[u8]) -> Result<serde_json::Map<String, Value>, Denial> {
    if raw.is_empty() {
        return Ok(serde_json::Map::new());
    }
    match serde_json::from_slice::<Value>(raw) {
        Ok(Value::Object(map)) => Ok(map),
        Ok(Value::Null) => Err(Denial::FieldErrors(
            r#"{"non_field_errors":["No data provided"]}"#.to_owned(),
        )),
        Ok(other) => {
            let kind = match &other {
                Value::Array(_) => "list",
                Value::String(_) => "str",
                // `arbitrary_precision`: int-vs-float is `is_f64` — plain
                // integer literals (even past u64) are Python `int`.
                Value::Number(n) => {
                    if n.is_f64() {
                        "float"
                    } else {
                        "int"
                    }
                }
                Value::Bool(_) => "bool",
                Value::Null | Value::Object(_) => unreachable!("handled above"),
            };
            Err(Denial::FieldErrors(format!(
                r#"{{"non_field_errors":["Invalid data. Expected a dictionary, but got {kind}."]}}"#
            )))
        }
        Err(error) => Err(Denial::BadDetail(format!("JSON parse error - {error}"))),
    }
}

/// A single field-coercion failure: the pre-rendered message plus whether it
/// is a per-field list (the usual shape) or an index-keyed object (list
/// children, e.g. `members`).
#[derive(Debug, Clone)]
pub struct CoerceFail {
    pub body: String,
}

/// Python `str(value)` for error echoes, over JSON values: strings verbatim,
/// bools as `True`/`False`, null as `None`, floats via
/// `paginator::py_float_str`, containers recursively with single-quoted
/// strings (verified: `"['a']" is not a valid choice.`,
/// `"[]" is not a valid UUID.`).
pub fn py_repr(value: &Value) -> String {
    match value {
        Value::Null => "None".to_owned(),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::Number(n) => {
            if n.is_i64() {
                n.as_i64().expect("i64").to_string()
            } else if n.is_u64() {
                n.as_u64().expect("u64").to_string()
            } else {
                // `arbitrary_precision`: integer literals beyond u64 keep
                // their exact text (Python ints are unbounded); fraction /
                // exponent literals render as floats.
                let text = n.as_str();
                if text.bytes().any(|b| b == b'.' || b == b'e' || b == b'E') {
                    crate::paginator::py_float_str(n.as_f64().unwrap_or(f64::NAN))
                } else {
                    text.to_owned()
                }
            }
        }
        Value::String(s) => s.clone(),
        Value::Array(items) => {
            let parts: Vec<String> = items.iter().map(py_repr_quoted).collect();
            format!("[{}]", parts.join(", "))
        }
        Value::Object(map) => {
            let parts: Vec<String> = map
                .iter()
                .map(|(k, v)| {
                    format!(
                        "{}: {}",
                        py_repr_quoted(&Value::String(k.clone())),
                        py_repr_quoted(v)
                    )
                })
                .collect();
            format!("{{{}}}", parts.join(", "))
        }
    }
}

/// Python `repr()` for a value nested inside a container echo: strings gain
/// single quotes (with `\'`/`\\` escapes); everything else is `py_repr`.
fn py_repr_quoted(value: &Value) -> String {
    match value {
        Value::String(s) => {
            let mut out = String::with_capacity(s.len() + 2);
            out.push('\'');
            for ch in s.chars() {
                match ch {
                    '\'' => out.push_str("\\'"),
                    '\\' => out.push_str("\\\\"),
                    '\n' => out.push_str("\\n"),
                    '\r' => out.push_str("\\r"),
                    '\t' => out.push_str("\\t"),
                    c => out.push(c),
                }
            }
            out.push('\'');
            out
        }
        other => py_repr(other),
    }
}

/// DRF `CharField` over one JSON value (verified live): `null` fails unless
/// `allow_null`; bools and containers fail `Not a valid string.`; numbers
/// stringify (`str(data)`); strings strip (`trim_whitespace`, the default —
/// whitespace counts as blank for `allow_blank=False`) and the stripped value
/// is stored, length-checked, and null-char guarded (the model
/// `ProhibitNullCharactersValidator` runs through the `ModelSerializer`).
/// Returns the validated string, or `None` when explicitly null.
pub fn coerce_char(
    value: Option<&Value>,
    allow_blank: bool,
    allow_null: bool,
    max_length: Option<usize>,
) -> Result<Option<String>, CoerceFail> {
    let fail = |body: &str| CoerceFail {
        body: body.to_owned(),
    };
    let Some(value) = value else {
        return Err(fail(r#"["This field is required."]"#));
    };
    match value {
        Value::Null => {
            if allow_null {
                Ok(None)
            } else {
                Err(fail(r#"["This field may not be null."]"#))
            }
        }
        Value::Bool(_) | Value::Array(_) | Value::Object(_) => {
            Err(fail(r#"["Not a valid string."]"#))
        }
        Value::Number(n) => {
            let text = py_repr(&Value::Number(n.clone()));
            check_max_length(&text, max_length)?;
            Ok(Some(text))
        }
        Value::String(raw) => {
            if raw.is_empty() || raw.trim().is_empty() {
                if !allow_blank {
                    return Err(fail(r#"["This field may not be blank."]"#));
                }
                return Ok(Some(String::new()));
            }
            let trimmed = raw.trim().to_owned();
            check_max_length(&trimmed, max_length)?;
            if trimmed.contains('\u{0}') {
                return Err(fail(r#"["Null characters are not allowed."]"#));
            }
            Ok(Some(trimmed))
        }
    }
}

fn check_max_length(text: &str, max_length: Option<usize>) -> Result<(), CoerceFail> {
    if let Some(max) = max_length {
        // DRF counts Unicode scalar values (`len(str)`).
        if text.chars().count() > max {
            return Err(CoerceFail {
                body: format!(r#"["Ensure this field has no more than {max} characters."]"#),
            });
        }
    }
    Ok(())
}

/// DRF `ChoiceField` over one JSON value (verified live): `null` fails
/// unless `allow_null`; anything whose `str()` is not a valid choice fails
/// `"{input}" is not a valid choice.` with the Python rendering (not JSON:
/// `"['a']"`, `"True"`, `"1.5"`).
pub fn coerce_choice(
    value: Option<&Value>,
    choices: &[&str],
) -> Result<Option<String>, CoerceFail> {
    let fail = |body: String| CoerceFail { body };
    let Some(value) = value else {
        return Err(fail(r#"["This field is required."]"#.to_owned()));
    };
    if value.is_null() {
        return Err(fail(r#"["This field may not be null."]"#.to_owned()));
    }
    let text = py_repr(value);
    if choices.contains(&text.as_str()) {
        Ok(Some(text))
    } else {
        Err(fail(format!(
            "[{}]",
            json_string(&format!(r#""{text}" is not a valid choice."#))
        )))
    }
}

/// The outcome of coercing one user-PK input (the `lead` field and each
/// `members` child): a validated user id, or explicit null (lead only).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PkValue {
    Id(uuid::Uuid),
    Null,
}

/// DRF user-PK coercion (verified live against every JSON scalar): the
/// `ModelSerializer`-built field routes through the UUID pk-field first,
/// then the existence check over `User.objects.all()` (no `is_active`
/// filter — inactive users validate, verified live):
///
/// * missing → required; `null` → null error unless `allow_null`;
/// * `""` → `None` for `allow_null` lead (proceeds), the null error for
///   `members` children (which declare no `allow_null`);
/// * bool → `Incorrect type. Expected pk value, received bool.`;
/// * int → `UUID(int=...)` (negative/huge fail the invalid-UUID message),
///   then existence (`Invalid pk "5" - object does not exist.`);
/// * float/list/dict/str → the UUID parse, failing
///   `“<value>” is not a valid UUID.` (DRF's curly quotes) with the Python
///   rendering, else the existence check with the raw echo.
pub fn coerce_pk_shape(value: &Value, allow_null: bool) -> Result<PkValue, CoerceFail> {
    let fail = |body: String| CoerceFail { body };
    match value {
        Value::Null => {
            if allow_null {
                Ok(PkValue::Null)
            } else {
                Err(fail(r#"["This field may not be null."]"#.to_owned()))
            }
        }
        Value::Bool(_) => Err(fail(format!(
            "[{}]",
            json_string("Incorrect type. Expected pk value, received bool.")
        ))),
        Value::Number(n) => {
            // `UUID(int=...)` accepts the u128 range (negatives and huge
            // values fail); the digit echo is exact even past u64 thanks to
            // `arbitrary_precision`.
            let digits = py_repr(&Value::Number(n.clone()));
            match digits.parse::<u128>().map(uuid::Uuid::from_u128) {
                Ok(id) => Ok(PkValue::Id(id)),
                Err(_) => Err(fail(invalid_uuid_message(&digits))),
            }
        }
        Value::String(raw) => {
            if raw.is_empty() {
                if allow_null {
                    return Ok(PkValue::Null);
                }
                return Err(fail(r#"["This field may not be null."]"#.to_owned()));
            }
            match raw.parse::<uuid::Uuid>() {
                Ok(id) => Ok(PkValue::Id(id)),
                Err(_) => Err(fail(invalid_uuid_message(raw))),
            }
        }
        Value::Array(_) | Value::Object(_) => Err(fail(invalid_uuid_message(&py_repr(value)))),
    }
}

/// DRF UUID-field invalid message with curly quotes (verified live).
pub fn invalid_uuid_message(rendered: &str) -> String {
    format!(
        "[{}]",
        json_string(&format!("\u{201c}{rendered}\u{201d} is not a valid UUID."))
    )
}

/// Float UUID echo check: a float always fails the UUID parse (verified
/// live: `“1.5” is not a valid UUID.`). `coerce_pk_shape` routes numbers
/// through the int path, so floats need their own arm — handled by testing
/// `is_f64` before calling it. This helper keeps that decision in one place.
pub fn coerce_pk_value(value: &Value, allow_null: bool) -> Result<PkValue, CoerceFail> {
    if let Value::Number(n) = value {
        if n.is_f64() {
            return Err(CoerceFail {
                body: invalid_uuid_message(&py_repr(value)),
            });
        }
    }
    coerce_pk_shape(value, allow_null)
}

/// Check one coerced user id against `User.objects.all()`: the full `users`
/// table with no `is_active` filter (verified live: inactive users validate
/// as lead/members).
pub async fn check_user_exists(
    pool: &PgPool,
    id: &uuid::Uuid,
    raw_echo: &str,
    site: &str,
) -> Result<(), CoerceFail> {
    let found: bool = sqlx::query_scalar(r#"SELECT EXISTS(SELECT 1 FROM "users" WHERE "id" = $1)"#)
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(|_| CoerceFail {
            body: String::new(),
        })?
        .unwrap_or(false);
    if found {
        Ok(())
    } else {
        let _ = site;
        Err(CoerceFail {
            body: format!(
                "[{}]",
                json_string(&format!(
                    r#"Invalid pk "{raw_echo}" - object does not exist."#
                ))
            ),
        })
    }
}

/// Coerce the `members` list (`ListField(child=PK, write_only)`): missing
/// is absent (not an error); null fails; non-lists fail the type message;
/// children validate as user PKs (no `allow_null`), failing index-keyed
/// (`{"0": [...]}`).
pub fn coerce_members_shape(value: Option<&Value>) -> Result<Option<Vec<Value>>, CoerceFail> {
    let Some(value) = value else {
        return Ok(None);
    };
    match value {
        Value::Null => Err(CoerceFail {
            body: r#"["This field may not be null."]"#.to_owned(),
        }),
        Value::Array(items) => Ok(Some(items.clone())),
        Value::Bool(_) => Err(CoerceFail {
            body: r#"["Expected a list of items but got type \"bool\"."]"#.to_owned(),
        }),
        // `type(data).__name__`: ints (even past u64) are `int`, fraction /
        // exponent literals are `float` — `is_f64`, not `as_i64`.
        Value::Number(n) => Err(CoerceFail {
            body: if n.is_f64() {
                r#"["Expected a list of items but got type \"float\"."]"#.to_owned()
            } else {
                r#"["Expected a list of items but got type \"int\"."]"#.to_owned()
            },
        }),
        Value::Object(_) => Err(CoerceFail {
            body: r#"["Expected a list of items but got type \"dict\"."]"#.to_owned(),
        }),
        Value::String(_) => Err(CoerceFail {
            body: r#"["Expected a list of items but got type \"str\"."]"#.to_owned(),
        }),
    }
}

/// Validated write fields for create/update, in `Meta.fields` order. `None`
/// means absent (PATCH leaves it); `Some(None)` means explicit null (PATCH
/// clears nullable fields).
#[derive(Debug, Clone, Default)]
pub struct ModuleWrite {
    pub name: Option<String>,
    pub description: Option<String>,
    pub start_date: Option<Option<chrono::NaiveDate>>,
    pub target_date: Option<Option<chrono::NaiveDate>>,
    pub status: Option<String>,
    pub lead: Option<Option<uuid::Uuid>>,
    pub members: Option<Vec<uuid::Uuid>>,
    pub external_source: Option<Option<String>>,
    pub external_id: Option<Option<String>>,
}

/// Django `parse_date` (the `iso-8601` input format DRF's `DateField`
/// uses): `date.fromisoformat` first, so besides `YYYY-MM-DD` it also
/// accepts basic `YYYYMMDD` and ISO week dates (`YYYY-Www[-D]` /
/// `YYYYWww[D]`, the day defaulting to Monday). Ordinal dates are rejected
/// by CPython and stay rejected here. Returns the parsed date for writes.
pub fn parse_module_date(text: &str) -> Option<chrono::NaiveDate> {
    parse_calendar_date(text)
        .or_else(|| parse_basic_date(text))
        .or_else(|| parse_week_date(text))
}

/// Extended calendar `YYYY-M-D`: 4-digit year, 1-2 digit ASCII month/day,
/// plus a real calendar day.
fn parse_calendar_date(text: &str) -> Option<chrono::NaiveDate> {
    let parts: Vec<&str> = text.split('-').collect();
    if parts.len() != 3 {
        return None;
    }
    let (year, month, day) = (parts[0], parts[1], parts[2]);
    if year.len() != 4
        || month.is_empty()
        || month.len() > 2
        || day.is_empty()
        || day.len() > 2
        || !year.bytes().all(|b| b.is_ascii_digit())
        || !month.bytes().all(|b| b.is_ascii_digit())
        || !day.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let year: i32 = year.parse().ok()?;
    let month: u32 = month.parse().ok()?;
    let day: u32 = day.parse().ok()?;
    chrono::NaiveDate::from_ymd_opt(year, month, day)
}

/// Basic calendar `YYYYMMDD`: exactly 8 ASCII digits (CPython rejects any
/// other width), year 1-9999, plus a real calendar day.
fn parse_basic_date(text: &str) -> Option<chrono::NaiveDate> {
    if text.len() != 8 || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let year: i32 = text[0..4].parse().ok()?;
    let month: u32 = text[4..6].parse().ok()?;
    let day: u32 = text[6..8].parse().ok()?;
    if !(1..=9999).contains(&year) {
        return None;
    }
    chrono::NaiveDate::from_ymd_opt(year, month, day)
}

/// ISO week dates: `YYYY-Www[-D]` and `YYYYWww[D]` (uppercase `W`,
/// zero-padded week, 1-digit day defaulting to Monday). Mixed dashes,
/// week 00, day 0/8, year 0, week 53 in short years, and results past
/// `9999-12-31` are all rejected, matching CPython.
fn parse_week_date(text: &str) -> Option<chrono::NaiveDate> {
    let bytes = text.as_bytes();
    // Fixed shapes only; unpadded weeks, 2-digit days, lowercase `w` and
    // mixed-dash forms fall out here.
    let (year, week, day): (&[u8], &[u8], &[u8]) = match bytes.len() {
        10 if bytes[4] == b'-' && bytes[5] == b'W' && bytes[8] == b'-' => {
            (&bytes[0..4], &bytes[6..8], &bytes[9..10])
        }
        8 if bytes[4] == b'-' && bytes[5] == b'W' => (&bytes[0..4], &bytes[6..8], b"1"),
        8 if bytes[4] == b'W' => (&bytes[0..4], &bytes[5..7], &bytes[7..8]),
        7 if bytes[4] == b'W' => (&bytes[0..4], &bytes[5..7], b"1"),
        _ => return None,
    };
    if !year.iter().all(|b| b.is_ascii_digit())
        || !week.iter().all(|b| b.is_ascii_digit())
        || !day.iter().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let year: i32 = std::str::from_utf8(year).ok()?.parse().ok()?;
    let week: u32 = std::str::from_utf8(week).ok()?.parse().ok()?;
    let day: u32 = std::str::from_utf8(day).ok()?.parse().ok()?;
    if !(1..=9999).contains(&year) || !(1..=53).contains(&week) {
        return None;
    }
    let weekday = match day {
        1 => chrono::Weekday::Mon,
        2 => chrono::Weekday::Tue,
        3 => chrono::Weekday::Wed,
        4 => chrono::Weekday::Thu,
        5 => chrono::Weekday::Fri,
        6 => chrono::Weekday::Sat,
        7 => chrono::Weekday::Sun,
        _ => return None,
    };
    let date = chrono::NaiveDate::from_isoywd_opt(year, week, weekday)?;
    // CPython overflows past year 9999 (e.g. `9999-W52-7` is out of range
    // for `date`); chrono's range is wider, so the cap is enforced here.
    use chrono::Datelike;
    if date.year() > 9999 {
        return None;
    }
    Some(date)
}

/// Coerce the create/update body field by field (DRF field order =
/// `Meta.fields` order; errors keyed the same way). `partial` selects PATCH
/// semantics (every field optional). Date failures reuse the services
/// layer's `DRF_DATE_FORMAT_MESSAGE` (verified against DRF's `DateField`).
pub async fn coerce_write(
    pool: &PgPool,
    body: &serde_json::Map<String, Value>,
    partial: bool,
) -> Result<ModuleWrite, Denial> {
    use pidash_services::v1_cycles_modules::module_shapes as shapes;
    let mut errors: Vec<(String, String)> = Vec::new();
    let mut write = ModuleWrite::default();

    // name: CharField(max 255, blank=False, null=False); required unless partial.
    match body.get("name") {
        None if partial => {}
        value => match coerce_char(value, false, false, Some(255)) {
            Ok(Some(name)) => write.name = Some(name),
            Ok(None) => {}
            Err(fail) => errors.push(("name".to_owned(), fail.body)),
        },
    }
    // description: CharField(blank=True); missing/None handled.
    match body.get("description") {
        None => {}
        Some(Value::Null) => errors.push((
            "description".to_owned(),
            r#"["This field may not be null."]"#.to_owned(),
        )),
        value => match coerce_char(value, true, false, None) {
            Ok(Some(description)) => write.description = Some(description),
            Ok(None) => {}
            Err(fail) => errors.push(("description".to_owned(), fail.body)),
        },
    }
    // start_date / target_date: DateField(null=True); missing absent,
    // explicit null clears (PATCH distinguishes the two, so the null arm
    // runs before validation).
    for key in ["start_date", "target_date"] {
        match body.get(key) {
            None => {}
            Some(Value::Null) => {
                if key == "start_date" {
                    write.start_date = Some(None);
                } else {
                    write.target_date = Some(None);
                }
            }
            Some(Value::String(text)) => match parse_module_date(text) {
                Some(date) => {
                    if key == "start_date" {
                        write.start_date = Some(Some(date));
                    } else {
                        write.target_date = Some(Some(date));
                    }
                }
                None => errors.push((
                    key.to_owned(),
                    format!("[{}]", json_string(shapes::DRF_DATE_FORMAT_MESSAGE)),
                )),
            },
            Some(_) => errors.push((
                key.to_owned(),
                format!("[{}]", json_string(shapes::DRF_DATE_FORMAT_MESSAGE)),
            )),
        }
    }
    // status: ChoiceField(6); missing absent. The choice list comes from
    // the types layer's `ModuleStatus::ALL` (single source of truth).
    match body.get("status") {
        None => {}
        value => {
            use pidash_types::v1_cycles_modules::module_shapes::ModuleStatus;
            let choices: [&str; 6] = std::array::from_fn(|index| ModuleStatus::ALL[index].as_str());
            match coerce_choice(value, &choices) {
                Ok(Some(status)) => write.status = Some(status),
                Ok(None) => {}
                Err(fail) => errors.push(("status".to_owned(), fail.body)),
            }
        }
    }
    // lead: user PK (allow_null); missing absent; "" -> None.
    match body.get("lead") {
        None => {}
        Some(value) => match coerce_pk_value(value, true) {
            Ok(PkValue::Null) => write.lead = Some(None),
            Ok(PkValue::Id(id)) => {
                let echo = match value {
                    Value::Number(_) => py_repr(value),
                    Value::String(s) => s.clone(),
                    _ => py_repr(value),
                };
                match check_user_exists(pool, &id, &echo, "lead-exists").await {
                    Ok(()) => write.lead = Some(Some(id)),
                    Err(fail) => {
                        if fail.body.is_empty() {
                            return Err(Denial::ServerError);
                        }
                        errors.push(("lead".to_owned(), fail.body));
                    }
                }
            }
            Err(fail) => errors.push(("lead".to_owned(), fail.body)),
        },
    }
    // members: list of user PKs; missing absent.
    match coerce_members_shape(body.get("members")) {
        Ok(None) => {}
        Ok(Some(items)) => {
            let mut ids = Vec::with_capacity(items.len());
            let mut child_errors: Vec<(usize, String)> = Vec::new();
            for (index, item) in items.iter().enumerate() {
                match coerce_pk_value(item, false) {
                    Ok(PkValue::Null) => {
                        child_errors.push((index, r#"["This field may not be null."]"#.to_owned()))
                    }
                    Ok(PkValue::Id(id)) => {
                        let echo = match item {
                            Value::Number(_) => py_repr(item),
                            Value::String(s) => s.clone(),
                            _ => py_repr(item),
                        };
                        match check_user_exists(pool, &id, &echo, "members-exists").await {
                            Ok(()) => ids.push(id),
                            Err(fail) => {
                                if fail.body.is_empty() {
                                    return Err(Denial::ServerError);
                                }
                                child_errors.push((index, fail.body));
                            }
                        }
                    }
                    Err(fail) => child_errors.push((index, fail.body)),
                }
            }
            if child_errors.is_empty() {
                write.members = Some(ids);
            } else {
                let parts: Vec<String> = child_errors
                    .iter()
                    .map(|(index, body)| format!("\"{index}\":{body}"))
                    .collect();
                errors.push(("members".to_owned(), format!("{{{}}}", parts.join(","))));
            }
        }
        Err(fail) => errors.push(("members".to_owned(), fail.body)),
    }
    // external_source / external_id: CharField(max 255, blank+null).
    for key in ["external_source", "external_id"] {
        match body.get(key) {
            None => {}
            Some(Value::Null) => {
                if key == "external_source" {
                    write.external_source = Some(None);
                } else {
                    write.external_id = Some(None);
                }
            }
            value => match coerce_char(value, true, true, Some(255)) {
                Ok(value) => {
                    if key == "external_source" {
                        write.external_source = Some(value);
                    } else {
                        write.external_id = Some(value);
                    }
                }
                Err(fail) => errors.push((key.to_owned(), fail.body)),
            },
        }
    }

    if !errors.is_empty() {
        let parts: Vec<String> = errors
            .iter()
            .map(|(key, body)| format!("{}:{body}", json_string(key)))
            .collect();
        return Err(Denial::FieldErrors(format!("{{{}}}", parts.join(","))));
    }
    Ok(write)
}

// ---------------------------------------------------------------------------
// Pagination + ordering
// ---------------------------------------------------------------------------

/// One paginated window over a fully-fetched row list
/// (`utils/paginator.py:402-463`). Reads fetch the whole project-scoped
/// list like the D-19 handlers (bounded data, exact `total_count`), slice
/// the window, render the page rows (async: `?expand=` queries), then wrap
/// the envelope.
pub struct PageWindow {
    pub per_page: i64,
    pub cursor: crate::paginator::Cursor,
    pub start: usize,
    pub stop: usize,
    pub has_more: bool,
}

/// Parse `per_page`/`cursor` (400s) and slice the window (negative slices
/// 500, exactly like Django). `total` is the full row count.
pub fn page_window(query: &QueryMap, total: usize) -> Result<PageWindow, Denial> {
    use crate::paginator::{offset_window, parse_per_page, Cursor};
    let per_page = parse_per_page(query_last(query, "per_page").as_deref(), 1000, 1000)
        .map_err(page_denial)?;
    let cursor_raw = query_last(query, "cursor").unwrap_or_else(|| format!("{per_page}:0:0"));
    let cursor = Cursor::from_string(&cursor_raw).map_err(page_denial)?;
    let window = offset_window(per_page, cursor.offset, cursor.value, cursor.is_prev, None)
        .map_err(page_denial)?;
    // `queryset[offset:stop]` over the evaluated rows.
    let start = (window.offset as usize).min(total);
    let stop = (window.stop as usize).min(total);
    // Backwards walk with a mismatched cursor value reads nothing
    // (`results[-(limit+1):]` on the lazy queryset raises into the 500).
    if !cursor.value.equals_limit(per_page) && cursor.is_prev {
        return Err(Denial::ServerError);
    }
    let window_len = stop.saturating_sub(start);
    let has_more = window_len as i64 > per_page;
    // `results[:limit]` over the evaluated window (negative limits already
    // errored in `offset_window`).
    let trim = usize::try_from(per_page)
        .unwrap_or(usize::MAX)
        .min(window_len);
    Ok(PageWindow {
        per_page,
        cursor,
        start,
        stop: start.saturating_add(trim),
        has_more,
    })
}

/// Wrap rendered page rows in the 12-key envelope.
pub fn page_envelope(
    window: &PageWindow,
    total_count: usize,
    results: Vec<Value>,
) -> Result<Response, Denial> {
    use crate::paginator::{max_hits, next_cursor, prev_cursor, PageResponse};
    let total = i64::try_from(total_count).unwrap_or(i64::MAX);
    let total_pages = max_hits(total, window.per_page).map_err(page_denial)?;
    let next = next_cursor(window.per_page, window.cursor.offset, window.has_more);
    let prev = prev_cursor(window.per_page, window.cursor.offset);
    let body = PageResponse {
        grouped_by: None,
        sub_grouped_by: None,
        total_count: total,
        next_cursor: next.to_string(),
        prev_cursor: prev.to_string(),
        next_page_results: window.has_more,
        prev_page_results: window.cursor.offset > 0,
        count: results.len(),
        total_pages,
        total_results: total,
        extra_stats: None,
        results,
    };
    serde_json::to_string(&body)
        .map(json_ok)
        .map_err(|error| db_error(error, "page-render"))
}

/// Map paginator failures to the exact Django responses (same mapping as
/// the D-19 handlers): `ParseError` 400s for `per_page`/`cursor`/offset
/// errors, the generic 500 for zero limits and negative slices.
pub fn page_denial(error: crate::paginator::PageError) -> Denial {
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

/// Resolve the module-issues GET `?order_by=` (default `created_at`,
/// ascending) against what Django's `.order_by()` accepts at
/// `views/module.py:618` (PIDASHCONV-510/511; every arm verified against
/// live `str(query)` output): the exact `?` (random — `-?` is a
/// `FieldError`, never random), the two annotations that exist at order
/// time (`sub_issues_count`, `bridge_id` — `link_count`/
/// `attachment_count` are annotated after and `FieldError`), bare-FK
/// names by the related model's `Meta.ordering` (`type` excepted:
/// `IssueType` has no ordering, so Django orders it by the local
/// `type_id`), bare-M2M names (`assignees`, `labels`), and
/// single-level traversals onto the already-joined tables. Anything
/// else runs quoted onto its table and fails at the database, exactly
/// like Django's `FieldError`-at-evaluation → generic 500.
pub fn resolve_issue_order(
    raw: Option<&str>,
) -> pidash_db::v1_cycles_modules::module_queries::OrderBy {
    use pidash_db::v1_cycles_modules::module_queries::{
        issue_traversal_table, M2MOrder, OrderBy, RelatedOrder,
    };
    if raw == Some("?") {
        return OrderBy::random();
    }
    const EARLY_ANNOTATIONS: [&str; 2] = ["sub_issues_count", "bridge_id"];
    let text = raw.unwrap_or("created_at");
    let (descending, column) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    if EARLY_ANNOTATIONS.contains(&column) {
        return OrderBy::alias(column, descending);
    }
    // `type` is the only FK Django orders by its local column:
    // `IssueType` has no `Meta.ordering`, so `order_by=type` renders
    // `ORDER BY "issues"."type_id"` with no join (live str(query)).
    if column == "type" {
        return OrderBy::new("type_id", descending);
    }
    // Bare FKs (and the reverse FK `issue_module`) order by the
    // related `Meta.ordering` (PIDASHCONV-511).
    if let Some(which) = match column {
        "state" => Some(RelatedOrder::State),
        "project" => Some(RelatedOrder::Project),
        "workspace" => Some(RelatedOrder::Workspace),
        "parent" => Some(RelatedOrder::Parent),
        "created_by" => Some(RelatedOrder::CreatedBy),
        "updated_by" => Some(RelatedOrder::UpdatedBy),
        "estimate_point" => Some(RelatedOrder::EstimatePoint),
        "assigned_pod" => Some(RelatedOrder::AssignedPod),
        "issue_module" => Some(RelatedOrder::IssueModule),
        _ => None,
    } {
        return OrderBy::related(which, descending);
    }
    if column == "assignees" {
        return OrderBy::m2m(M2MOrder::Assignees, descending);
    }
    if column == "labels" {
        return OrderBy::m2m(M2MOrder::Labels, descending);
    }
    if let Some((head, tail)) = column.split_once("__") {
        if let Some(table) = issue_traversal_table(head) {
            return OrderBy::table(table, tail, descending);
        }
    }
    OrderBy::new(column, descending)
}

/// Envelope totals for the module-issues page: `rows.len()` — except
/// the M2M orderings multiply rows per through-row while Django's
/// `queryset.count()` trims the ordering-only joins, so M2M totals
/// count distinct issue ids (PIDASHCONV-510; live bridges are unique
/// per issue+module, so only ordering joins can multiply here). The
/// PIDASHCONV-511 related-ordering joins are to-one `LEFT JOIN`s, so
/// they never multiply and keep `rows.len()`. The page window itself
/// still slices the multiplied rows, exactly like Django's
/// `queryset[offset:stop]`.
fn envelope_total(
    order: &pidash_db::v1_cycles_modules::module_queries::OrderBy,
    rows: &[sqlx::postgres::PgRow],
) -> Result<usize, Denial> {
    use pidash_db::v1_cycles_modules::module_queries::OrderTarget;
    if !matches!(order.target, OrderTarget::M2M(_)) {
        return Ok(rows.len());
    }
    let mut ids = Vec::with_capacity(rows.len());
    for row in rows {
        ids.push(row_uuid(row, "id", "module-issues-total")?);
    }
    Ok(count_distinct(ids))
}

/// Count distinct values (the M2M envelope total).
fn count_distinct(ids: Vec<uuid::Uuid>) -> usize {
    use std::collections::HashSet;
    ids.into_iter().collect::<HashSet<_>>().len()
}

// ---------------------------------------------------------------------------
// Reads
// ---------------------------------------------------------------------------

/// The ten `Project` columns the views touch (`module_view`,
/// `archive_view` gate the serializers; `workspace_id` re-stamps).
#[derive(Debug, Clone)]
pub struct ProjectRow {
    pub id: uuid::Uuid,
    pub workspace_id: uuid::Uuid,
    pub module_view: bool,
    pub identifier: String,
}

impl ProjectRow {
    pub fn decode(row: &sqlx::postgres::PgRow, site: &str) -> Result<Self, Denial> {
        Ok(ProjectRow {
            id: row_uuid(row, "id", site)?,
            workspace_id: row_uuid(row, "workspace_id", site)?,
            module_view: row
                .try_get::<bool, _>("module_view")
                .map_err(|error| db_error(error, site))?,
            identifier: row_string(row, "identifier", site)?,
        })
    }
}

/// `Project.objects.get(pk=..., workspace__slug=...)`: the live project row
/// in this workspace, else the 404 branch.
pub async fn fetch_project(
    pool: &PgPool,
    project_id: &uuid::Uuid,
    workspace_id: &uuid::Uuid,
) -> Result<ProjectRow, Denial> {
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(
        r#"SELECT "id", "workspace_id", "module_view", "identifier" FROM "projects"
           WHERE "id" = $1 AND "workspace_id" = $2 AND "deleted_at" IS NULL"#,
    )
    .bind(project_id)
    .bind(workspace_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| db_error(error, "project-get"))?;
    row.map(|row| ProjectRow::decode(&row, "project-get"))
        .transpose()?
        .ok_or(Denial::NotFound)
}

/// `instance.members.all()`: live membership ids in `User` `-created_at`
/// order (the M2M queryset's default ordering, verified live).
pub async fn fetch_members(
    pool: &PgPool,
    module_id: &uuid::Uuid,
) -> Result<Vec<uuid::Uuid>, Denial> {
    sqlx::query_scalar(
        r#"SELECT mm."member_id" FROM "module_members" mm
           INNER JOIN "users" u ON u."id" = mm."member_id"
           WHERE mm."module_id" = $1 AND mm."deleted_at" IS NULL
           ORDER BY u."created_at" DESC"#,
    )
    .bind(module_id)
    .fetch_all(pool)
    .await
    .map_err(|error| db_error(error, "members-list"))
}

/// `IssueAssignee` / `IssueLabel` id lists in `-created_at` order (both
/// models' default ordering).
pub async fn fetch_assignees(
    pool: &PgPool,
    issue_id: &uuid::Uuid,
) -> Result<Vec<uuid::Uuid>, Denial> {
    sqlx::query_scalar(
        r#"SELECT "assignee_id" FROM "issue_assignees" WHERE "issue_id" = $1 AND "deleted_at" IS NULL ORDER BY "created_at" DESC"#,
    )
    .bind(issue_id)
    .fetch_all(pool)
    .await
    .map_err(|error| db_error(error, "assignees-list"))
}

pub async fn fetch_issue_labels(
    pool: &PgPool,
    issue_id: &uuid::Uuid,
) -> Result<Vec<uuid::Uuid>, Denial> {
    sqlx::query_scalar(
        r#"SELECT "label_id" FROM "issue_labels" WHERE "issue_id" = $1 AND "deleted_at" IS NULL ORDER BY "created_at" DESC"#,
    )
    .bind(issue_id)
    .fetch_all(pool)
    .await
    .map_err(|error| db_error(error, "issue-labels-list"))
}

/// `IssueSerializer.get_url` (`serializers/issue.py:126-137`):
/// `{base}/{slug}/browse/{identifier}-{sequence}`; omitted when neither
/// `WEB_URL` nor `APP_BASE_URL` is configured.
pub fn issue_url(
    state: &AppState,
    slug: &str,
    identifier: &str,
    sequence_id: i32,
) -> Option<String> {
    web_base_url(state).map(|base| format!("{base}/{slug}/browse/{identifier}-{sequence_id}"))
}

/// The rewrite for the members list (`validate()`): keep only ids that are
/// project members, in `ProjectMember` `-created_at` order (the queryset's
/// default ordering, verified live).
pub async fn rewrite_members(
    pool: &PgPool,
    project_id: &uuid::Uuid,
    member_ids: &[uuid::Uuid],
) -> Result<Vec<uuid::Uuid>, Denial> {
    if member_ids.is_empty() {
        return Ok(Vec::new());
    }
    sqlx::query_scalar(
        r#"SELECT "member_id" FROM "project_members" WHERE "project_id" = $1 AND "member_id" = ANY($2) AND "deleted_at" IS NULL ORDER BY "created_at" DESC"#,
    )
    .bind(project_id)
    .bind(member_ids)
    .fetch_all(pool)
    .await
    .map_err(|error| db_error(error, "members-rewrite"))
}

// ---------------------------------------------------------------------------
// Reads: module list / detail / archived list
// ---------------------------------------------------------------------------

/// `GET .../modules/` (`views/module.py:76-132`): the gate runs before
/// pagination parsing (a denied caller gets 403 even with a bad cursor);
/// `?order_by=` is ignored (the queryset hardcodes `-created_at`).
pub async fn list_modules_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    project_raw: &str,
    query: &QueryMap,
) -> Result<Response, Denial> {
    let pre = preamble(state, headers, slug).await?;
    let workspace_id = pre.workspace_id.ok_or(Denial::Forbidden)?;
    let project_id = rewrite_project_id(&pre.pool, slug, project_raw).await?;
    require_gate(
        &pre.pool,
        &workspace_id,
        slug,
        &pre.actor.id,
        &project_id,
        "GET",
        PATH_MODULES,
    )
    .await?;
    let sql = pidash_db::v1_cycles_modules::module_queries::module_list_sql(
        &pidash_db::v1_cycles_modules::module_queries::OrderBy::default_module(),
        pidash_db::v1_cycles_modules::module_queries::ArchivedFilter::Live,
    );
    let rows: Vec<sqlx::postgres::PgRow> = sqlx::query(&sql)
        .bind(slug)
        .bind(project_id)
        .fetch_all(&pre.pool)
        .await
        .map_err(|error| db_error(error, "modules-list"))?;
    let window = page_window(query, rows.len())?;
    let fields = fields_param(query, "fields");
    let expand = fields_param(query, "expand");
    let mut results = Vec::new();
    for row in &rows[window.start..window.stop] {
        let detail = ModuleDetail::decode(row, "modules-list")?
            .with_list_annotations(row, "modules-list")?;
        let members = fetch_members(&pre.pool, &detail.id).await?;
        results.push(
            render_module(
                &pre.pool,
                &detail,
                &members,
                &pre.actor.timezone,
                fields.as_deref(),
                expand.as_deref(),
            )
            .await?,
        );
    }
    page_envelope(&window, rows.len(), results)
}

/// `GET .../modules/<pk>/` (`views/module.py:271-286`).
pub async fn retrieve_module_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    project_raw: &str,
    pk: &uuid::Uuid,
    query: &QueryMap,
) -> Result<Response, Denial> {
    let pre = preamble(state, headers, slug).await?;
    let workspace_id = pre.workspace_id.ok_or(Denial::Forbidden)?;
    let project_id = rewrite_project_id(&pre.pool, slug, project_raw).await?;
    require_gate(
        &pre.pool,
        &workspace_id,
        slug,
        &pre.actor.id,
        &project_id,
        "GET",
        PATH_MODULE_DETAIL,
    )
    .await?;
    let sql = pidash_db::v1_cycles_modules::module_queries::module_detail_sql(
        &pidash_db::v1_cycles_modules::module_queries::OrderBy::default_module(),
    );
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(&sql)
        .bind(slug)
        .bind(project_id)
        .bind(pk)
        .fetch_optional(&pre.pool)
        .await
        .map_err(|error| db_error(error, "module-detail"))?;
    let Some(row) = row else {
        return Err(Denial::NotFound);
    };
    let detail = ModuleDetail::decode(&row, "module-detail")?
        .with_list_annotations(&row, "module-detail")?;
    let members = fetch_members(&pre.pool, &detail.id).await?;
    let fields = fields_param(query, "fields");
    let expand = fields_param(query, "expand");
    let value = render_module(
        &pre.pool,
        &detail,
        &members,
        &pre.actor.timezone,
        fields.as_deref(),
        expand.as_deref(),
    )
    .await?;
    serde_json::to_string(&value)
        .map(json_ok)
        .map_err(|error| db_error(error, "module-detail-render"))
}

/// `GET .../archived-modules/` (`views/module.py:990-993`).
pub async fn list_archived_modules_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    project_raw: &str,
    query: &QueryMap,
) -> Result<Response, Denial> {
    let pre = preamble(state, headers, slug).await?;
    let workspace_id = pre.workspace_id.ok_or(Denial::Forbidden)?;
    let project_id = rewrite_project_id(&pre.pool, slug, project_raw).await?;
    require_gate(
        &pre.pool,
        &workspace_id,
        slug,
        &pre.actor.id,
        &project_id,
        "GET",
        PATH_ARCHIVED_LIST,
    )
    .await?;
    let sql = pidash_db::v1_cycles_modules::module_queries::archived_module_list_sql(
        &pidash_db::v1_cycles_modules::module_queries::OrderBy::default_module(),
    );
    let rows: Vec<sqlx::postgres::PgRow> = sqlx::query(&sql)
        .bind(slug)
        .bind(project_id)
        .fetch_all(&pre.pool)
        .await
        .map_err(|error| db_error(error, "modules-archived"))?;
    let window = page_window(query, rows.len())?;
    let fields = fields_param(query, "fields");
    let expand = fields_param(query, "expand");
    let mut results = Vec::new();
    for row in &rows[window.start..window.stop] {
        let detail = ModuleDetail::decode(row, "modules-archived")?
            .with_list_annotations(row, "modules-archived")?;
        let members = fetch_members(&pre.pool, &detail.id).await?;
        results.push(
            render_module(
                &pre.pool,
                &detail,
                &members,
                &pre.actor.timezone,
                fields.as_deref(),
                expand.as_deref(),
            )
            .await?,
        );
    }
    page_envelope(&window, rows.len(), results)
}

// ---------------------------------------------------------------------------
// Writes: module create / patch / delete
// ---------------------------------------------------------------------------

/// Python truthiness over raw JSON request values (the external-dup checks
/// read `request.data`, not the coerced fields).
pub fn py_truthy(value: &Value) -> bool {
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
            } else {
                true
            }
        }
        Value::String(s) => !s.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
    }
}

/// Django `CharField.get_prep_value` for filter values: only str/int/float
/// survive field coercion, and each stringifies.
pub fn prep_text(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Number(_) => Some(py_repr(value)),
        _ => None,
    }
}

/// `POST .../modules/` (`views/module.py:134-268`): gate → project 404 →
/// field coercion → `validate()` → external-dup 409 → dup-name 400 → insert
/// → members → `model_activity` → bare re-read → 201.
pub async fn create_module_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    project_raw: &str,
    body: &[u8],
) -> Result<Response, Denial> {
    let pre = preamble(state, headers, slug).await?;
    let workspace_id = pre.workspace_id.ok_or(Denial::Forbidden)?;
    let project_id = rewrite_project_id(&pre.pool, slug, project_raw).await?;
    require_gate(
        &pre.pool,
        &workspace_id,
        slug,
        &pre.actor.id,
        &project_id,
        "POST",
        PATH_MODULES,
    )
    .await?;
    let project = fetch_project(&pre.pool, &project_id, &workspace_id).await?;
    let raw = parse_body(body)?;
    let write = coerce_write(&pre.pool, &raw, false).await?;
    // `validate()`: the project gates (the context id is always present
    // and the row was just fetched; `module_view` is the live arm),
    // then the date order, then the members rewrite.
    if !project.module_view {
        return Err(Denial::FieldErrors(
            r#"{"non_field_errors":["Modules are not enabled for this project"]}"#.to_owned(),
        ));
    }
    if let (Some(Some(start)), Some(Some(target))) =
        (write.start_date.as_ref(), write.target_date.as_ref())
    {
        if start > target {
            return Err(Denial::FieldErrors(
                r#"{"non_field_errors":["Start date cannot exceed target date"]}"#.to_owned(),
            ));
        }
    }
    let members = match write.members {
        Some(ref ids) if !ids.is_empty() => rewrite_members(&pre.pool, &project_id, ids).await?,
        Some(_) => Vec::new(),
        None => Vec::new(),
    };
    let members_present = write.members.is_some();
    // External-dup check on the RAW body values (both truthy): answers the
    // clash row's id (`views/module.py:210-223`).
    if let (Some(raw_id), Some(raw_source)) = (raw.get("external_id"), raw.get("external_source")) {
        if py_truthy(raw_id) && py_truthy(raw_source) {
            if let (Some(external_id), Some(external_source)) =
                (prep_text(raw_id), prep_text(raw_source))
            {
                let clash: Option<uuid::Uuid> = sqlx::query_scalar(
                    r#"SELECT m."id" FROM "modules" m
                       INNER JOIN "workspaces" w ON m."workspace_id" = w."id"
                       WHERE w."slug" = $1 AND m."project_id" = $2 AND m."external_source" = $3 AND m."external_id" = $4 AND m."deleted_at" IS NULL
                       ORDER BY m."created_at" DESC LIMIT 1"#,
                )
                .bind(slug)
                .bind(project_id)
                .bind(external_source)
                .bind(external_id)
                .fetch_optional(&pre.pool)
                .await
                .map_err(|error| db_error(error, "module-create-extdup"))?;
                if let Some(clash) = clash {
                    let body = format!(
                        "{{\"error\":{},\"id\":{}}}",
                        json_string(
                            "Module with the same external id and external source already exists"
                        ),
                        json_string(&clash.to_string()),
                    );
                    return Err(Denial::Conflict(body));
                }
            }
        }
    }
    // Dup-name check inside `create()` (`serializers/module.py:120-132`):
    // the four-key body.
    let name = write.name.clone().ok_or(Denial::ServerError)?;
    let clash: Option<uuid::Uuid> = sqlx::query_scalar(
        r#"SELECT "id" FROM "modules" WHERE "name" = $1 AND "project_id" = $2 AND "deleted_at" IS NULL ORDER BY "created_at" DESC LIMIT 1"#,
    )
    .bind(&name)
    .bind(project_id)
    .fetch_optional(&pre.pool)
    .await
    .map_err(|error| db_error(error, "module-create-namedup"))?;
    if let Some(clash) = clash {
        let body = format!(
            "{{\"id\":{},\"code\":{},\"error\":{},\"message\":{}}}",
            json_string(&clash.to_string()),
            json_string("MODULE_NAME_ALREADY_EXISTS"),
            json_string("Module with this name already exists"),
            json_string("Module with this name already exists"),
        );
        return Err(Denial::FieldErrors(body));
    }
    // `Module.save`: `sort_order = min - 10000`, or the 65535 default for
    // the first module (`db/models/module.py:91-102`).
    let now = micros_now();
    let module_id = uuid::Uuid::new_v4();
    let min_sort: Option<f64> = sqlx::query_scalar(
        r#"SELECT MIN("sort_order") FROM "modules" WHERE "project_id" = $1 AND "deleted_at" IS NULL"#,
    )
    .bind(project_id)
    .fetch_optional(&pre.pool)
    .await
    .map_err(|error| db_error(error, "module-create-sort"))?
    .flatten();
    let sort_order = min_sort.map_or(65535.0, |min| min - 10000.0);
    // `Module.objects.create(**validated_data, project_id=...)`: model
    // defaults fill the absent fields; `BaseModel.save` stamps
    // `created_by` and leaves `updated_by` NULL; `save` re-stamps the
    // workspace from the project (identical here).
    let status = write.status.clone().unwrap_or_else(|| "planned".to_owned());
    sqlx::query(
        r#"INSERT INTO "modules" ("id", "created_at", "updated_at", "created_by_id", "updated_by_id", "deleted_at",
               "project_id", "workspace_id", "name", "description", "description_text", "description_html",
               "start_date", "target_date", "status", "lead_id", "view_props", "sort_order",
               "external_source", "external_id", "archived_at", "logo_props")
           VALUES ($1, $2, $2, $3, NULL, NULL, $4, $5, $6, $7, NULL, NULL, $8, $9, $10, $11, '{}', $12, $13, $14, NULL, '{}')"#,
    )
    .bind(module_id)
    .bind(now)
    .bind(pre.actor.id)
    .bind(project_id)
    .bind(project.workspace_id)
    .bind(&name)
    .bind(write.description.clone().unwrap_or_default())
    .bind(write.start_date.flatten())
    .bind(write.target_date.flatten())
    .bind(&status)
    .bind(write.lead.flatten())
    .bind(sort_order)
    .bind(write.external_source.clone().flatten())
    .bind(write.external_id.clone().flatten())
    .execute(&pre.pool)
    .await
    .map_err(|error| db_error(error, "module-create-insert"))?;
    // `ModuleMember.objects.bulk_create(..., ignore_conflicts=True)` in
    // batches of 10 (`serializers/module.py:140-150`); one statement with
    // `ON CONFLICT DO NOTHING` is equivalent. `created_by` is the module's
    // creator (the actor), `updated_by` NULL.
    if members_present && !members.is_empty() {
        sqlx::query(
            r#"INSERT INTO "module_members" ("id", "created_at", "updated_at", "created_by_id", "updated_by_id", "deleted_at",
                   "project_id", "workspace_id", "module_id", "member_id")
               SELECT gen_random_uuid(), $1, $1, $2, NULL, NULL, $3, $4, $5, unnest($6::uuid[])
               ON CONFLICT DO NOTHING"#,
        )
        .bind(now)
        .bind(pre.actor.id)
        .bind(project_id)
        .bind(project.workspace_id)
        .bind(module_id)
        .bind(&members)
        .execute(&pre.pool)
        .await
        .map_err(|error| db_error(error, "module-create-members"))?;
    }
    let job = pidash_jobs::v1_cycles_modules::publish::model_created_job(
        "module",
        &module_id.to_string(),
        raw,
        &pre.actor.id.to_string(),
        slug,
        &app_origin(state),
    );
    enqueue_best_effort(&pre.pool, &job).await;
    // `Module.objects.get(pk=...)` + bare `ModuleSerializer` (no
    // annotations: the six metric keys are absent).
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(
        r#"SELECT "id", "created_at", "updated_at", "created_by_id", "updated_by_id", "deleted_at",
                  "project_id", "workspace_id", "name", "description", "description_text", "description_html",
                  "start_date", "target_date", "status", "lead_id", "view_props", "sort_order",
                  "external_source", "external_id", "archived_at", "logo_props"
           FROM "modules" WHERE "id" = $1 AND "deleted_at" IS NULL"#,
    )
    .bind(module_id)
    .fetch_optional(&pre.pool)
    .await
    .map_err(|error| db_error(error, "module-create-reread"))?;
    let Some(row) = row else {
        return Err(Denial::ServerError);
    };
    let detail = ModuleDetail::decode(&row, "module-create-reread")?;
    let members = fetch_members(&pre.pool, &detail.id).await?;
    let value = render_module(
        &pre.pool,
        &detail,
        &members,
        &pre.actor.timezone,
        None,
        None,
    )
    .await?;
    serde_json::to_string(&value)
        .map(json_created)
        .map_err(|error| db_error(error, "module-create-render"))
}

/// `PATCH .../modules/<pk>/` (`views/module.py:391-449`): plain `.get`
/// (no archived filter) → snapshot → archived 400 → field coercion →
/// `validate()` → external-dup 409 → dup-name 400 → members replace →
/// update → `model_activity` → the update-serializer data, 200.
pub async fn patch_module_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    project_raw: &str,
    pk: &uuid::Uuid,
    body: &[u8],
) -> Result<Response, Denial> {
    let pre = preamble(state, headers, slug).await?;
    let workspace_id = pre.workspace_id.ok_or(Denial::Forbidden)?;
    let project_id = rewrite_project_id(&pre.pool, slug, project_raw).await?;
    require_gate(
        &pre.pool,
        &workspace_id,
        slug,
        &pre.actor.id,
        &project_id,
        "PATCH",
        PATH_MODULE_DETAIL,
    )
    .await?;
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(
        r#"SELECT m."id", m."created_at", m."updated_at", m."created_by_id", m."updated_by_id", m."deleted_at",
                  m."project_id", m."workspace_id", m."name", m."description", m."description_text", m."description_html",
                  m."start_date", m."target_date", m."status", m."lead_id", m."view_props", m."sort_order",
                  m."external_source", m."external_id", m."archived_at", m."logo_props"
           FROM "modules" m INNER JOIN "workspaces" w ON m."workspace_id" = w."id"
           WHERE w."slug" = $1 AND m."project_id" = $2 AND m."id" = $3 AND m."deleted_at" IS NULL"#,
    )
    .bind(slug)
    .bind(project_id)
    .bind(pk)
    .fetch_optional(&pre.pool)
    .await
    .map_err(|error| db_error(error, "module-patch-get"))?;
    let Some(row) = row else {
        return Err(Denial::NotFound);
    };
    let before = ModuleDetail::decode(&row, "module-patch-get")?;
    // `current_instance`: `json.dumps(ModuleSerializer(module).data)` of
    // the BARE pre-save instance (no annotations), Python separators.
    let before_members = fetch_members(&pre.pool, &before.id).await?;
    let snapshot = render_module(
        &pre.pool,
        &before,
        &before_members,
        &pre.actor.timezone,
        None,
        None,
    )
    .await?;
    let snapshot_text = pidash_jobs::tasks_webhooks::activity_dispatch::django_dumps(&snapshot);
    if before.archived_at.is_some() {
        return Err(Denial::BadError(
            "Archived module cannot be edited".to_owned(),
        ));
    }
    let raw = parse_body(body)?;
    let write = coerce_write(&pre.pool, &raw, true).await?;
    // `validate()`: the project must still be live (its soft-delete
    // between gate and body would 404 here, matching `DoesNotExist`), the
    // module view must be on, the provided dates ordered; the members list
    // rewrites through project membership.
    let project = fetch_project(&pre.pool, &project_id, &workspace_id).await?;
    if !project.module_view {
        return Err(Denial::FieldErrors(
            r#"{"non_field_errors":["Modules are not enabled for this project"]}"#.to_owned(),
        ));
    }
    if let (Some(Some(start)), Some(Some(target))) =
        (write.start_date.as_ref(), write.target_date.as_ref())
    {
        if start > target {
            return Err(Denial::FieldErrors(
                r#"{"non_field_errors":["Start date cannot exceed target date"]}"#.to_owned(),
            ));
        }
    }
    let members = match write.members {
        Some(ref ids) if !ids.is_empty() => rewrite_members(&pre.pool, &project_id, ids).await?,
        Some(_) => Vec::new(),
        None => Vec::new(),
    };
    // PATCH external-dup (`views/module.py:422-440`): the RAW external id
    // must be truthy and differ from the stored one (Python equality: a
    // stored `"5"` differs from a raw `5`); the source is the raw value
    // when the key is present (explicit null filters `IS NULL`) else the
    // stored one. Answers the CURRENT module's id.
    if let Some(raw_id) = raw.get("external_id") {
        if py_truthy(raw_id) && !py_equals_stored(&before.external_id, raw_id) {
            let source: Option<String> = match raw.get("external_source") {
                None => before.external_source.clone(),
                Some(Value::Null) => None,
                Some(value) => prep_text(value),
            };
            let clash: bool = match (prep_text(raw_id), source) {
                (Some(external_id), Some(external_source)) => sqlx::query_scalar(
                    r#"SELECT EXISTS(SELECT 1 FROM "modules" m
                       INNER JOIN "workspaces" w ON m."workspace_id" = w."id"
                       WHERE w."slug" = $1 AND m."project_id" = $2 AND m."external_source" = $3 AND m."external_id" = $4 AND m."deleted_at" IS NULL)"#,
                )
                .bind(slug)
                .bind(project_id)
                .bind(external_source)
                .bind(external_id)
                .fetch_one(&pre.pool)
                .await
                .map_err(|error| db_error(error, "module-patch-extdup"))?,
                (Some(external_id), None) => sqlx::query_scalar(
                    r#"SELECT EXISTS(SELECT 1 FROM "modules" m
                       INNER JOIN "workspaces" w ON m."workspace_id" = w."id"
                       WHERE w."slug" = $1 AND m."project_id" = $2 AND m."external_source" IS NULL AND m."external_id" = $3 AND m."deleted_at" IS NULL)"#,
                )
                .bind(slug)
                .bind(project_id)
                .bind(external_id)
                .fetch_one(&pre.pool)
                .await
                .map_err(|error| db_error(error, "module-patch-extdup"))?,
                _ => false,
            };
            if clash {
                let body = format!(
                    "{{\"error\":{},\"id\":{}}}",
                    json_string(
                        "Module with the same external id and external source already exists"
                    ),
                    json_string(&before.id.to_string()),
                );
                return Err(Denial::Conflict(body));
            }
        }
    }
    // Dup-name check inside `update()` (self excluded): the error-only body.
    if let Some(ref name) = write.name {
        let clash: bool = sqlx::query_scalar(
            r#"SELECT EXISTS(SELECT 1 FROM "modules" WHERE "name" = $1 AND "project_id" = $2 AND "id" <> $3 AND "deleted_at" IS NULL)"#,
        )
        .bind(name)
        .bind(project_id)
        .bind(pk)
        .fetch_one(&pre.pool)
        .await
        .map_err(|error| db_error(error, "module-patch-namedup"))?;
        if clash {
            return Err(Denial::BadError(
                "Module with this name already exists".to_owned(),
            ));
        }
    }
    // Members replace (`serializers/module.py:152-168`): soft-delete every
    // live row, then bulk-create the rewritten list (`created_by`/`updated_by`
    // from the module instance, possibly NULL).
    if write.members.is_some() {
        sqlx::query(r#"UPDATE "module_members" SET "deleted_at" = now() WHERE "module_id" = $1 AND "deleted_at" IS NULL"#)
            .bind(pk)
            .execute(&pre.pool)
            .await
            .map_err(|error| db_error(error, "module-patch-members-del"))?;
        if !members.is_empty() {
            sqlx::query(
                r#"INSERT INTO "module_members" ("id", "created_at", "updated_at", "created_by_id", "updated_by_id", "deleted_at",
                       "project_id", "workspace_id", "module_id", "member_id")
                   SELECT gen_random_uuid(), now(), now(), $1, $2, NULL, $3, $4, $5, unnest($6::uuid[])"#,
            )
            .bind(before.created_by)
            .bind(before.updated_by)
            .bind(project_id)
            .bind(project.workspace_id)
            .bind(pk)
            .bind(&members)
            .execute(&pre.pool)
            .await
            .map_err(|error| db_error(error, "module-patch-members-add"))?;
        }
    }
    // `super().update` + `save()`: provided fields, `updated_at` auto,
    // `updated_by` stamped by CRUM.
    let now = micros_now();
    update_module_fields(
        &pre.pool,
        pk,
        &write,
        &pre.actor.id,
        &now,
        "module-patch-update",
    )
    .await?;
    let job = pidash_jobs::v1_cycles_modules::publish::model_updated_job(
        "module",
        &pk.to_string(),
        raw,
        &snapshot_text,
        &pre.actor.id.to_string(),
        slug,
        &app_origin(state),
    );
    enqueue_best_effort(&pre.pool, &job).await;
    // `serializer.data`: the update-serializer shape over the saved row.
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(
        r#"SELECT "id", "created_at", "updated_at", "created_by_id", "updated_by_id", "deleted_at",
                  "project_id", "workspace_id", "name", "description", "description_text", "description_html",
                  "start_date", "target_date", "status", "lead_id", "view_props", "sort_order",
                  "external_source", "external_id", "archived_at", "logo_props"
           FROM "modules" WHERE "id" = $1 AND "deleted_at" IS NULL"#,
    )
    .bind(pk)
    .fetch_optional(&pre.pool)
    .await
    .map_err(|error| db_error(error, "module-patch-reread"))?;
    let Some(row) = row else {
        return Err(Denial::ServerError);
    };
    let after = ModuleDetail::decode(&row, "module-patch-reread")?;
    let value = render_module_update_shape(&after);
    serde_json::to_string(&value)
        .map(json_ok)
        .map_err(|error| db_error(error, "module-patch-render"))
}

/// Python `==` between a stored `Option<String>` and a raw JSON value: only
/// equal-type strings compare equal (`"5" != 5`).
pub fn py_equals_stored(stored: &Option<String>, raw: &Value) -> bool {
    match (stored, raw) {
        (None, Value::Null) => true,
        (Some(current), Value::String(next)) => current == next,
        _ => false,
    }
}

/// Apply the provided write fields (`super().update` semantics: only
/// present keys move) plus the `save()` stamps.
pub async fn update_module_fields(
    pool: &PgPool,
    pk: &uuid::Uuid,
    write: &ModuleWrite,
    actor: &uuid::Uuid,
    now: &chrono::DateTime<chrono::Utc>,
    site: &str,
) -> Result<(), Denial> {
    // One statement per provided field keeps the partial-update semantics
    // obvious; Django issues a single `UPDATE` with the same assignments.
    if let Some(ref name) = write.name {
        sqlx::query(r#"UPDATE "modules" SET "name" = $1 WHERE "id" = $2"#)
            .bind(name)
            .bind(pk)
            .execute(pool)
            .await
            .map_err(|error| db_error(error, site))?;
    }
    if let Some(ref description) = write.description {
        sqlx::query(r#"UPDATE "modules" SET "description" = $1 WHERE "id" = $2"#)
            .bind(description)
            .bind(pk)
            .execute(pool)
            .await
            .map_err(|error| db_error(error, site))?;
    }
    if let Some(start) = write.start_date {
        sqlx::query(r#"UPDATE "modules" SET "start_date" = $1 WHERE "id" = $2"#)
            .bind(start)
            .bind(pk)
            .execute(pool)
            .await
            .map_err(|error| db_error(error, site))?;
    }
    if let Some(target) = write.target_date {
        sqlx::query(r#"UPDATE "modules" SET "target_date" = $1 WHERE "id" = $2"#)
            .bind(target)
            .bind(pk)
            .execute(pool)
            .await
            .map_err(|error| db_error(error, site))?;
    }
    if let Some(ref status) = write.status {
        sqlx::query(r#"UPDATE "modules" SET "status" = $1 WHERE "id" = $2"#)
            .bind(status)
            .bind(pk)
            .execute(pool)
            .await
            .map_err(|error| db_error(error, site))?;
    }
    if let Some(lead) = write.lead {
        sqlx::query(r#"UPDATE "modules" SET "lead_id" = $1 WHERE "id" = $2"#)
            .bind(lead)
            .bind(pk)
            .execute(pool)
            .await
            .map_err(|error| db_error(error, site))?;
    }
    if let Some(ref external_source) = write.external_source {
        sqlx::query(r#"UPDATE "modules" SET "external_source" = $1 WHERE "id" = $2"#)
            .bind(external_source)
            .bind(pk)
            .execute(pool)
            .await
            .map_err(|error| db_error(error, site))?;
    }
    if let Some(ref external_id) = write.external_id {
        sqlx::query(r#"UPDATE "modules" SET "external_id" = $1 WHERE "id" = $2"#)
            .bind(external_id)
            .bind(pk)
            .execute(pool)
            .await
            .map_err(|error| db_error(error, site))?;
    }
    sqlx::query(r#"UPDATE "modules" SET "updated_at" = $1, "updated_by_id" = $2 WHERE "id" = $3"#)
        .bind(now)
        .bind(actor)
        .bind(pk)
        .execute(pool)
        .await
        .map_err(|error| db_error(error, site))?;
    Ok(())
}

/// `DELETE .../modules/<pk>/` (`views/module.py:451-494`): plain `.get` →
/// creator-or-project-admin 403 → collect live bridge issue ids →
/// `issue_activity` → soft-delete (+ its fan-out task) → bridge + favorite
/// queryset soft-deletes → 204.
pub async fn delete_module_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    project_raw: &str,
    pk: &uuid::Uuid,
) -> Result<Response, Denial> {
    let pre = preamble(state, headers, slug).await?;
    let workspace_id = pre.workspace_id.ok_or(Denial::Forbidden)?;
    let project_id = rewrite_project_id(&pre.pool, slug, project_raw).await?;
    require_gate(
        &pre.pool,
        &workspace_id,
        slug,
        &pre.actor.id,
        &project_id,
        "DELETE",
        PATH_MODULE_DETAIL,
    )
    .await?;
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(
        r#"SELECT m."id", m."name", m."created_by_id" FROM "modules" m
           INNER JOIN "workspaces" w ON m."workspace_id" = w."id"
           WHERE w."slug" = $1 AND m."project_id" = $2 AND m."id" = $3 AND m."deleted_at" IS NULL"#,
    )
    .bind(slug)
    .bind(project_id)
    .bind(pk)
    .fetch_optional(&pre.pool)
    .await
    .map_err(|error| db_error(error, "module-delete-get"))?;
    let Some(row) = row else {
        return Err(Denial::NotFound);
    };
    let name = row_string(&row, "name", "module-delete-get")?;
    let created_by = row_uuid_opt(&row, "created_by_id", "module-delete-get")?;
    // Creator or project admin (role 20), else the view's own inline
    // 403 (`views/module.py:503-507` — a `Response`, not `PermissionDenied`,
    // so the `{"error": ...}` key, not the class body).
    if created_by != Some(pre.actor.id) {
        let admin: bool = sqlx::query_scalar(
            r#"SELECT EXISTS(SELECT 1 FROM "project_members" WHERE "workspace_id" = $1 AND "member_id" = $2 AND "role" = 20 AND "project_id" = $3 AND "is_active" AND "deleted_at" IS NULL)"#,
        )
        .bind(workspace_id)
        .bind(pre.actor.id)
        .bind(project_id)
        .fetch_one(&pre.pool)
        .await
        .map_err(|error| db_error(error, "module-delete-admin"))?;
        if !admin {
            return Err(Denial::ForbiddenError(
                "Only admin or creator can delete the module".to_owned(),
            ));
        }
    }
    // The payload reads the LIVE bridge issue ids before the delete
    // (`views/module.py:461-464`, default `-created_at` order).
    let issues: Vec<uuid::Uuid> = sqlx::query_scalar(
        r#"SELECT "issue_id" FROM "module_issues" WHERE "module_id" = $1 AND "deleted_at" IS NULL ORDER BY "created_at" DESC"#,
    )
    .bind(pk)
    .fetch_all(&pre.pool)
    .await
    .map_err(|error| db_error(error, "module-delete-issues"))?;
    let now = micros_now();
    let issue_texts: Vec<String> = issues.iter().map(|id| id.to_string()).collect();
    let issue_refs: Vec<&str> = issue_texts.iter().map(String::as_str).collect();
    let job = pidash_jobs::v1_cycles_modules::publish::module_deleted_job(
        &pk.to_string(),
        &name,
        &issue_refs,
        &pre.actor.id.to_string(),
        &project_id.to_string(),
        now.timestamp(),
        &app_origin(state),
    );
    enqueue_best_effort(&pre.pool, &job).await;
    // Instance `delete()`: `deleted_at` + `save()` stamps.
    sqlx::query(
        r#"UPDATE "modules" SET "deleted_at" = $1, "updated_at" = $1, "updated_by_id" = $2 WHERE "id" = $3"#,
    )
    .bind(now)
    .bind(pre.actor.id)
    .bind(pk)
    .execute(&pre.pool)
    .await
    .map_err(|error| db_error(error, "module-delete-soft"))?;
    enqueue_soft_delete(&pre.pool, "module", pk).await;
    // Queryset deletes (`views/module.py:478-488`): `deleted_at` only
    // (`QuerySet.update` does not auto-stamp), no fan-out tasks.
    sqlx::query(
        r#"UPDATE "module_issues" SET "deleted_at" = $1 WHERE "module_id" = $2 AND "project_id" = $3 AND "deleted_at" IS NULL"#,
    )
    .bind(now)
    .bind(pk)
    .bind(project_id)
    .execute(&pre.pool)
    .await
    .map_err(|error| db_error(error, "module-delete-bridges"))?;
    sqlx::query(
        r#"UPDATE "user_favorites" SET "deleted_at" = $1 WHERE "entity_type" = 'module' AND "entity_identifier" = $2 AND "project_id" = $3 AND "deleted_at" IS NULL"#,
    )
    .bind(now)
    .bind(pk)
    .bind(project_id)
    .execute(&pre.pool)
    .await
    .map_err(|error| db_error(error, "module-delete-favorites"))?;
    Ok(no_content())
}

// ---------------------------------------------------------------------------
// Module issues: list / add / remove
// ---------------------------------------------------------------------------

/// `GET .../modules/<module_id>/module-issues/` (`views/module.py:610-654`):
/// the M4 GET builder with `?order_by=` (default `created_at` ascending),
/// rendered as `IssueSerializer` rows.
pub async fn list_module_issues_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    project_raw: &str,
    module_id: &uuid::Uuid,
    query: &QueryMap,
) -> Result<Response, Denial> {
    let pre = preamble(state, headers, slug).await?;
    let workspace_id = pre.workspace_id.ok_or(Denial::Forbidden)?;
    let project_id = rewrite_project_id(&pre.pool, slug, project_raw).await?;
    require_gate(
        &pre.pool,
        &workspace_id,
        slug,
        &pre.actor.id,
        &project_id,
        "GET",
        PATH_MODULE_ISSUES,
    )
    .await?;
    let order = resolve_issue_order(query_last(query, "order_by").as_deref());
    let sql = pidash_db::v1_cycles_modules::module_queries::module_issue_list_get_sql(&order);
    let rows: Vec<sqlx::postgres::PgRow> = sqlx::query(&sql)
        .bind(slug)
        .bind(project_id)
        .bind(module_id)
        .fetch_all(&pre.pool)
        .await
        .map_err(|error| db_error(error, "module-issues-list"))?;
    // The `url` needs the project identifier (plain fetch: the M4 path
    // never 404s on the project itself).
    let identifier: Option<String> =
        sqlx::query_scalar(r#"SELECT "identifier" FROM "projects" WHERE "id" = $1"#)
            .bind(project_id)
            .fetch_optional(&pre.pool)
            .await
            .map_err(|error| db_error(error, "module-issues-identifier"))?
            .flatten();
    let total = envelope_total(&order, &rows)?;
    let window = page_window(query, rows.len())?;
    let fields = fields_param(query, "fields");
    let expand = fields_param(query, "expand");
    let mut results = Vec::new();
    for row in &rows[window.start..window.stop] {
        let detail = IssueDetail::decode(row, "module-issues-list")?;
        let assignees = fetch_assignees(&pre.pool, &detail.id).await?;
        let labels = fetch_issue_labels(&pre.pool, &detail.id).await?;
        let url = identifier
            .as_deref()
            .and_then(|identifier| issue_url(state, slug, identifier, detail.sequence_id));
        results.push(
            render_issue(
                &pre.pool,
                &detail,
                &assignees,
                &labels,
                url,
                &pre.actor.timezone,
                fields.as_deref(),
                expand.as_deref(),
            )
            .await?,
        );
    }
    page_envelope(&window, total, results)
}

/// One validated `pk__in` candidate for the module-issues POST re-query:
/// a UUID to match, or a JSON NULL (renders `IN (..., NULL)`, matching
/// nothing, exactly like Django).
#[derive(Debug, Clone, Copy)]
pub enum IssueCandidate {
    Id(uuid::Uuid),
    Null,
}

/// Validate the raw `issues` value (`views/module.py:657-678`): missing or
/// empty (`[]`, `{}`, `""`) answers `"Issues are required."`; null and
/// non-sized scalars (`int`/`bool`/`float`) raise through `len()` to the
/// generic 500; anything else iterates into `pk__in` candidates, where an
/// invalid UUID answers `"Please provide valid detail"`.
pub fn coerce_issue_list(value: Option<&Value>) -> Result<Vec<IssueCandidate>, Denial> {
    let Some(value) = value else {
        return Err(Denial::BadError("Issues are required".to_owned()));
    };
    match value {
        Value::Null | Value::Number(_) | Value::Bool(_) => Err(Denial::ServerError),
        Value::String(s) => {
            if s.is_empty() {
                return Err(Denial::BadError("Issues are required".to_owned()));
            }
            // `pk__in` iterates the string into chars; every char fails the
            // UUID parse.
            Err(Denial::BadError(INVALID_DETAIL_BODY_TRIMMED.to_owned()))
        }
        Value::Array(items) => {
            if items.is_empty() {
                return Err(Denial::BadError("Issues are required".to_owned()));
            }
            items.iter().map(coerce_issue_candidate).collect()
        }
        Value::Object(map) => {
            if map.is_empty() {
                return Err(Denial::BadError("Issues are required".to_owned()));
            }
            // `pk__in` iterates the dict into its keys.
            map.keys()
                .map(|key| coerce_issue_candidate(&Value::String(key.clone())))
                .collect()
        }
    }
}

/// Trimmed `{"error":...}` message shared with [`INVALID_DETAIL_BODY`].
const INVALID_DETAIL_BODY_TRIMMED: &str = "Please provide valid detail";

/// Validate one `pk__in` item: strings parse as UUIDs, ints/bools coerce
/// via `UUID(int=...)`, nulls pass through as NULL, floats and containers
/// fail the UUID parse (all verified live).
pub fn coerce_issue_candidate(value: &Value) -> Result<IssueCandidate, Denial> {
    let invalid = || Denial::BadError(INVALID_DETAIL_BODY_TRIMMED.to_owned());
    match value {
        Value::Null => Ok(IssueCandidate::Null),
        Value::Bool(b) => Ok(IssueCandidate::Id(uuid::Uuid::from_u128(u128::from(
            *b as u8,
        )))),
        Value::Number(n) => {
            if n.is_f64() {
                return Err(invalid());
            }
            match py_repr(value).parse::<u128>().map(uuid::Uuid::from_u128) {
                Ok(id) => Ok(IssueCandidate::Id(id)),
                Err(_) => Err(invalid()),
            }
        }
        Value::String(raw) => match raw.parse::<uuid::Uuid>() {
            Ok(id) => Ok(IssueCandidate::Id(id)),
            Err(_) => Err(invalid()),
        },
        Value::Array(_) | Value::Object(_) => Err(invalid()),
    }
}

/// `str(queryset)` for the `modules_list` payload: `<SoftDeletionQuerySet
/// [UUID('...'), ...]>` in the re-query order (verified live against the
/// `Issue.objects` manager).
pub fn render_queryset_str(ids: &[uuid::Uuid]) -> String {
    let parts: Vec<String> = ids.iter().map(|id| format!("UUID('{id}')")).collect();
    format!("<SoftDeletionQuerySet [{}]>", parts.join(", "))
}

/// Django `serializers.serialize("json", ...)` for created bridges
/// (verified live): one `{"model", "pk", "fields"}` object per row in list
/// order, Python separators, datetimes via `DjangoJSONEncoder`
/// (`isoformat` with `+00:00`, micros omitted when zero).
pub fn render_bridge_dump(bridges: &[CreatedBridge]) -> String {
    let mut out = String::from("[");
    for (index, bridge) in bridges.iter().enumerate() {
        if index > 0 {
            out.push_str(", ");
        }
        out.push_str(&format!(
            "{{\"model\": \"db.moduleissue\", \"pk\": \"{}\", \"fields\": {{\"created_at\": {}, \"updated_at\": {}, \"created_by\": \"{}\", \"updated_by\": \"{}\", \"deleted_at\": null, \"project\": \"{}\", \"workspace\": \"{}\", \"module\": \"{}\", \"issue\": \"{}\"}}}}",
            bridge.id,
            render_django_datetime(&bridge.created_at),
            render_django_datetime(&bridge.updated_at),
            bridge.created_by,
            bridge.updated_by,
            bridge.project_id,
            bridge.workspace_id,
            bridge.module_id,
            bridge.issue_id,
        ));
    }
    out.push(']');
    out
}

/// `DjangoJSONEncoder` datetime: `isoformat()` in UTC (aware), micros only
/// when nonzero — note `+00:00`, never DRF's `Z`.
pub fn render_django_datetime(value: &chrono::DateTime<chrono::Utc>) -> String {
    let micros = value.timestamp_subsec_micros();
    let base = value.format("%Y-%m-%dT%H:%M:%S").to_string();
    if micros == 0 {
        format!("\"{base}+00:00\"")
    } else {
        format!("\"{base}.{micros:06}+00:00\"")
    }
}

/// One bridge row staged for insert (ids + timestamps generated up front so
/// the `serialize("json", ...)` payload renders the inserted values).
#[derive(Debug, Clone)]
pub struct CreatedBridge {
    pub id: uuid::Uuid,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub created_by: uuid::Uuid,
    pub updated_by: uuid::Uuid,
    pub project_id: uuid::Uuid,
    pub workspace_id: uuid::Uuid,
    pub module_id: uuid::Uuid,
    pub issue_id: uuid::Uuid,
}

/// `POST .../modules/<module_id>/module-issues/` (`views/module.py:656-735`):
/// issues-required → module 404 → re-query (invalid UUIDs 400) → create
/// (the "move" path is dead: the `str`-in-UUIDs comparison is always false,
/// so every call only creates and `updated_module_issues` is always `[]`)
/// → `issue_activity` → the FULL bridge list, 200. Note the view never runs
/// `ModuleIssueRequestSerializer`: the raw body drives everything, so a
/// non-object body 500s (`AttributeError` on `.get`) instead of answering
/// the serializer shape.
pub async fn add_module_issues_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    project_raw: &str,
    module_id: &uuid::Uuid,
    body: &[u8],
) -> Result<Response, Denial> {
    let pre = preamble(state, headers, slug).await?;
    let workspace_id = pre.workspace_id.ok_or(Denial::Forbidden)?;
    let project_id = rewrite_project_id(&pre.pool, slug, project_raw).await?;
    require_gate(
        &pre.pool,
        &workspace_id,
        slug,
        &pre.actor.id,
        &project_id,
        "POST",
        PATH_MODULE_ISSUES,
    )
    .await?;
    // Raw `request.data` (no serializer): empty is `{}`, unparseable is the
    // DRF `ParseError`, a non-object 500s on `.get`.
    let raw: serde_json::Map<String, Value> = if body.is_empty() {
        serde_json::Map::new()
    } else {
        let value: Value = serde_json::from_slice(body)
            .map_err(|error| Denial::BadDetail(format!("JSON parse error - {error}")))?;
        match value {
            Value::Object(map) => map,
            _ => return Err(Denial::ServerError),
        }
    };
    let candidates = coerce_issue_list(raw.get("issues"))?;
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(
        r#"SELECT m."id", m."workspace_id" FROM "modules" m
           INNER JOIN "workspaces" w ON m."workspace_id" = w."id"
           WHERE w."slug" = $1 AND m."project_id" = $2 AND m."id" = $3 AND m."deleted_at" IS NULL"#,
    )
    .bind(slug)
    .bind(project_id)
    .bind(module_id)
    .fetch_optional(&pre.pool)
    .await
    .map_err(|error| db_error(error, "module-issues-add-module"))?;
    let Some(row) = row else {
        return Err(Denial::NotFound);
    };
    let module_workspace_id = row_uuid(&row, "workspace_id", "module-issues-add-module")?;
    // The re-query (`Issue.objects`: soft-delete scope only, so archived
    // and draft issues can be added): default `-created_at` order, which is
    // also the `str(queryset)` and creation order.
    let wanted: Vec<uuid::Uuid> = candidates
        .iter()
        .filter_map(|candidate| match candidate {
            IssueCandidate::Id(id) => Some(*id),
            IssueCandidate::Null => None,
        })
        .collect();
    let matched: Vec<uuid::Uuid> = if wanted.is_empty() {
        Vec::new()
    } else {
        sqlx::query_scalar(
            r#"SELECT i."id" FROM "issues" i
               INNER JOIN "workspaces" w ON i."workspace_id" = w."id"
               WHERE w."slug" = $1 AND i."project_id" = $2 AND i."id" = ANY($3) AND i."deleted_at" IS NULL
               ORDER BY i."created_at" DESC"#,
        )
        .bind(slug)
        .bind(project_id)
        .bind(&wanted)
        .fetch_all(&pre.pool)
        .await
        .map_err(|error| db_error(error, "module-issues-add-requery"))?
    };
    // The existing-bridges lookup and the move comparison are dead code
    // (`views/module.py:681-707`): `str(...) in issues` is always false, so
    // `records_to_update` is always empty. Only the create list is built.
    let now = micros_now();
    let bridges: Vec<CreatedBridge> = matched
        .iter()
        .map(|issue_id| CreatedBridge {
            id: uuid::Uuid::new_v4(),
            created_at: now,
            updated_at: now,
            created_by: pre.actor.id,
            updated_by: pre.actor.id,
            project_id,
            workspace_id: module_workspace_id,
            module_id: *module_id,
            issue_id: *issue_id,
        })
        .collect();
    if !bridges.is_empty() {
        // `bulk_create(..., batch_size=10, ignore_conflicts=True)`: one
        // statement with `ON CONFLICT DO NOTHING` is equivalent.
        let ids: Vec<uuid::Uuid> = bridges.iter().map(|b| b.id).collect();
        let issue_ids: Vec<uuid::Uuid> = bridges.iter().map(|b| b.issue_id).collect();
        sqlx::query(
            r#"INSERT INTO "module_issues" ("id", "created_at", "updated_at", "created_by_id", "updated_by_id", "deleted_at",
                     "project_id", "workspace_id", "module_id", "issue_id")
               SELECT unnest($1::uuid[]), $2, $2, $3, $3, NULL, $4, $5, $6, unnest($7::uuid[])
               ON CONFLICT DO NOTHING"#,
        )
        .bind(&ids)
        .bind(now)
        .bind(pre.actor.id)
        .bind(project_id)
        .bind(module_workspace_id)
        .bind(module_id)
        .bind(&issue_ids)
        .execute(&pre.pool)
        .await
        .map_err(|error| db_error(error, "module-issues-add-insert"))?;
    }
    // `issue_activity`: `requested_data` double-encodes the queryset text,
    // `current_instance` double-encodes the Django dump; `project_id` is the
    // rewritten UUID (`str(self.kwargs.get("project_id"))` renders the
    // canonical form).
    let requested_text =
        pidash_jobs::tasks_webhooks::activity_dispatch::django_dumps(&serde_json::json!({
            "modules_list": render_queryset_str(&matched),
        }));
    let dump_text = render_bridge_dump(&bridges);
    let current_text =
        pidash_jobs::tasks_webhooks::activity_dispatch::django_dumps(&serde_json::json!({
            "updated_module_issues": [],
            "created_module_issues": dump_text,
        }));
    let job = pidash_jobs::v1_cycles_modules::publish::module_issue_added_job(
        &requested_text,
        &pre.actor.id.to_string(),
        &project_id.to_string(),
        &current_text,
        now.timestamp(),
        &app_origin(state),
    );
    enqueue_best_effort(&pre.pool, &job).await;
    // The FULL bridge list (`self.get_queryset()`, including pre-existing
    // rows), 200.
    let sql = pidash_db::v1_cycles_modules::module_queries::module_issue_queryset_sql(
        &pidash_db::v1_cycles_modules::module_queries::OrderBy::default_module(),
    );
    let rows: Vec<sqlx::postgres::PgRow> = sqlx::query(&sql)
        .bind(slug)
        .bind(project_id)
        .bind(module_id)
        .bind(pre.actor.id)
        .fetch_all(&pre.pool)
        .await
        .map_err(|error| db_error(error, "module-issues-add-reread"))?;
    let mut results = Vec::with_capacity(rows.len());
    for row in &rows {
        let detail = BridgeDetail::decode(row, "module-issues-add-reread")?;
        results.push(render_bridge(&detail, &pre.actor.timezone));
    }
    serde_json::to_string(&Value::Array(results))
        .map(json_ok)
        .map_err(|error| db_error(error, "module-issues-add-render"))
}

/// `DELETE .../modules/<module_id>/module-issues/<issue_id>/`
/// (`views/module.py:803-868`): bridge `.get` → module-name read →
/// soft-delete (+ fan-out) → `issue_activity` → 204.
pub async fn remove_module_issue_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    project_raw: &str,
    module_id: &uuid::Uuid,
    issue_id: &uuid::Uuid,
) -> Result<Response, Denial> {
    let pre = preamble(state, headers, slug).await?;
    let workspace_id = pre.workspace_id.ok_or(Denial::Forbidden)?;
    let project_id = rewrite_project_id(&pre.pool, slug, project_raw).await?;
    require_gate(
        &pre.pool,
        &workspace_id,
        slug,
        &pre.actor.id,
        &project_id,
        "DELETE",
        PATH_MODULE_ISSUE_DETAIL,
    )
    .await?;
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(
        r#"SELECT mi."id", mi."module_id", mi."issue_id" FROM "module_issues" mi
           INNER JOIN "workspaces" w ON mi."workspace_id" = w."id"
           WHERE w."slug" = $1 AND mi."project_id" = $2 AND mi."module_id" = $3 AND mi."issue_id" = $4 AND mi."deleted_at" IS NULL"#,
    )
    .bind(slug)
    .bind(project_id)
    .bind(module_id)
    .bind(issue_id)
    .fetch_optional(&pre.pool)
    .await
    .map_err(|error| db_error(error, "module-issue-remove-get"))?;
    let Some(row) = row else {
        return Err(Denial::NotFound);
    };
    let bridge_id = row_uuid(&row, "id", "module-issue-remove-get")?;
    // `module_issue.module.name`: the FK descriptor re-reads the live
    // module row (a miss 404s like `DoesNotExist`).
    let module_name: Option<String> = sqlx::query_scalar(
        r#"SELECT "name" FROM "modules" WHERE "id" = $1 AND "deleted_at" IS NULL"#,
    )
    .bind(module_id)
    .fetch_optional(&pre.pool)
    .await
    .map_err(|error| db_error(error, "module-issue-remove-module"))?
    .flatten();
    let Some(module_name) = module_name else {
        return Err(Denial::NotFound);
    };
    let now = micros_now();
    sqlx::query(
        r#"UPDATE "module_issues" SET "deleted_at" = $1, "updated_at" = $1, "updated_by_id" = $2 WHERE "id" = $3"#,
    )
    .bind(now)
    .bind(pre.actor.id)
    .bind(bridge_id)
    .execute(&pre.pool)
    .await
    .map_err(|error| db_error(error, "module-issue-remove-soft"))?;
    enqueue_soft_delete(&pre.pool, "moduleissue", &bridge_id).await;
    let job = pidash_jobs::v1_cycles_modules::publish::module_issue_removed_job(
        &module_id.to_string(),
        &module_name,
        &issue_id.to_string(),
        &pre.actor.id.to_string(),
        &project_id.to_string(),
        now.timestamp(),
    );
    enqueue_best_effort(&pre.pool, &job).await;
    Ok(no_content())
}

// ---------------------------------------------------------------------------
// Archive / unarchive
// ---------------------------------------------------------------------------

/// `POST .../modules/<pk>/archive/` (`views/module.py:994-1021`): plain
/// `.get` → status must be completed/cancelled → stamp `archived_at` (+ the
/// `save()` stamps) → favorite queryset soft-delete → 204.
pub async fn archive_module_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    project_raw: &str,
    pk: &uuid::Uuid,
) -> Result<Response, Denial> {
    let pre = preamble(state, headers, slug).await?;
    let workspace_id = pre.workspace_id.ok_or(Denial::Forbidden)?;
    let project_id = rewrite_project_id(&pre.pool, slug, project_raw).await?;
    require_gate(
        &pre.pool,
        &workspace_id,
        slug,
        &pre.actor.id,
        &project_id,
        "POST",
        PATH_MODULE_ARCHIVE,
    )
    .await?;
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(
        r#"SELECT m."id", m."status" FROM "modules" m
           INNER JOIN "workspaces" w ON m."workspace_id" = w."id"
           WHERE w."slug" = $1 AND m."project_id" = $2 AND m."id" = $3 AND m."deleted_at" IS NULL"#,
    )
    .bind(slug)
    .bind(project_id)
    .bind(pk)
    .fetch_optional(&pre.pool)
    .await
    .map_err(|error| db_error(error, "module-archive-get"))?;
    let Some(row) = row else {
        return Err(Denial::NotFound);
    };
    let status = row_string(&row, "status", "module-archive-get")?;
    if status != "completed" && status != "cancelled" {
        return Err(Denial::BadError(
            "Only completed or cancelled modules can be archived".to_owned(),
        ));
    }
    let now = micros_now();
    sqlx::query(
        r#"UPDATE "modules" SET "archived_at" = $1, "updated_at" = $1, "updated_by_id" = $2 WHERE "id" = $3"#,
    )
    .bind(now)
    .bind(pre.actor.id)
    .bind(pk)
    .execute(&pre.pool)
    .await
    .map_err(|error| db_error(error, "module-archive-stamp"))?;
    sqlx::query(
        r#"UPDATE "user_favorites" SET "deleted_at" = $1
           WHERE "entity_type" = 'module' AND "entity_identifier" = $2 AND "project_id" = $3
             AND "workspace_id" = (SELECT "id" FROM "workspaces" WHERE "slug" = $4)
             AND "deleted_at" IS NULL"#,
    )
    .bind(now)
    .bind(pk)
    .bind(project_id)
    .bind(slug)
    .execute(&pre.pool)
    .await
    .map_err(|error| db_error(error, "module-archive-favorites"))?;
    Ok(no_content())
}

/// `DELETE .../archived-modules/<pk>/unarchive/` (`views/module.py:1040-1077`):
/// plain `.get` (no archived check: unarchiving a live module still 204s
/// and bumps `updated_at`) → clear `archived_at` (+ the `save()` stamps) →
/// 204.
pub async fn unarchive_module_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    project_raw: &str,
    pk: &uuid::Uuid,
) -> Result<Response, Denial> {
    let pre = preamble(state, headers, slug).await?;
    let workspace_id = pre.workspace_id.ok_or(Denial::Forbidden)?;
    let project_id = rewrite_project_id(&pre.pool, slug, project_raw).await?;
    require_gate(
        &pre.pool,
        &workspace_id,
        slug,
        &pre.actor.id,
        &project_id,
        "DELETE",
        PATH_MODULE_UNARCHIVE,
    )
    .await?;
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(
        r#"SELECT m."id" FROM "modules" m
           INNER JOIN "workspaces" w ON m."workspace_id" = w."id"
           WHERE w."slug" = $1 AND m."project_id" = $2 AND m."id" = $3 AND m."deleted_at" IS NULL"#,
    )
    .bind(slug)
    .bind(project_id)
    .bind(pk)
    .fetch_optional(&pre.pool)
    .await
    .map_err(|error| db_error(error, "module-unarchive-get"))?;
    if row.is_none() {
        return Err(Denial::NotFound);
    }
    let now = micros_now();
    sqlx::query(
        r#"UPDATE "modules" SET "archived_at" = NULL, "updated_at" = $1, "updated_by_id" = $2 WHERE "id" = $3"#,
    )
    .bind(now)
    .bind(pre.actor.id)
    .bind(pk)
    .execute(&pre.pool)
    .await
    .map_err(|error| db_error(error, "module-unarchive-stamp"))?;
    Ok(no_content())
}

// ---------------------------------------------------------------------------
// Axum handlers + routes
// ---------------------------------------------------------------------------

/// Rebuild the inbound request and proxy it to Django (bad-UUID path
/// params: routing precedes auth, so Django's resolver 404 answers before
/// any 401 — the `app_cycles` precedent).
async fn proxy_through(
    state: AppState,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let mut req = axum::http::Request::builder()
        .method(method)
        .uri(uri)
        .body(axum::body::Body::from(body))
        .expect("rebuild proxy request");
    *req.headers_mut() = headers;
    crate::edge::proxy(State(state), req).await
}

/// Parse one UUID path param, proxying to Django on failure (routing
/// precedes auth).
fn path_uuid(raw: &str) -> Result<uuid::Uuid, ()> {
    raw.parse::<uuid::Uuid>().map_err(|_| ())
}

/// `GET .../modules/`.
pub async fn list_modules(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((slug, project_id)): Path<(String, String)>,
    Query(query): Query<QueryMap>,
) -> Response {
    match list_modules_inner(&state, &headers, &slug, &project_id, &query).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

/// `POST .../modules/`.
pub async fn create_module(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((slug, project_id)): Path<(String, String)>,
    body: Bytes,
) -> Response {
    match create_module_inner(&state, &headers, &slug, &project_id, &body).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

/// `GET .../modules/<pk>/`.
pub async fn retrieve_module(
    State(state): State<AppState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    Path((slug, project_id, pk)): Path<(String, String, String)>,
    Query(query): Query<QueryMap>,
) -> Response {
    let Ok(pk) = path_uuid(&pk) else {
        return proxy_through(state, method, uri, headers, Bytes::new()).await;
    };
    match retrieve_module_inner(&state, &headers, &slug, &project_id, &pk, &query).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

/// `PATCH .../modules/<pk>/`.
pub async fn patch_module(
    State(state): State<AppState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    Path((slug, project_id, pk)): Path<(String, String, String)>,
    body: Bytes,
) -> Response {
    let Ok(pk) = path_uuid(&pk) else {
        return proxy_through(state, method, uri, headers, body).await;
    };
    match patch_module_inner(&state, &headers, &slug, &project_id, &pk, &body).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

/// `DELETE .../modules/<pk>/`.
pub async fn delete_module(
    State(state): State<AppState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    Path((slug, project_id, pk)): Path<(String, String, String)>,
) -> Response {
    let Ok(pk) = path_uuid(&pk) else {
        return proxy_through(state, method, uri, headers, Bytes::new()).await;
    };
    match delete_module_inner(&state, &headers, &slug, &project_id, &pk).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

/// `GET .../modules/<module_id>/module-issues/`.
pub async fn list_module_issues(
    State(state): State<AppState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    Path((slug, project_id, module_id)): Path<(String, String, String)>,
    Query(query): Query<QueryMap>,
) -> Response {
    let Ok(module_id) = path_uuid(&module_id) else {
        return proxy_through(state, method, uri, headers, Bytes::new()).await;
    };
    match list_module_issues_inner(&state, &headers, &slug, &project_id, &module_id, &query).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

/// `POST .../modules/<module_id>/module-issues/`.
pub async fn add_module_issues(
    State(state): State<AppState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    Path((slug, project_id, module_id)): Path<(String, String, String)>,
    body: Bytes,
) -> Response {
    let Ok(module_id) = path_uuid(&module_id) else {
        return proxy_through(state, method, uri, headers, body).await;
    };
    match add_module_issues_inner(&state, &headers, &slug, &project_id, &module_id, &body).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

/// `DELETE .../modules/<module_id>/module-issues/<issue_id>/`.
pub async fn remove_module_issue(
    State(state): State<AppState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    Path((slug, project_id, module_id, issue_id)): Path<(String, String, String, String)>,
) -> Response {
    let (Ok(module_id), Ok(issue_id)) = (path_uuid(&module_id), path_uuid(&issue_id)) else {
        return proxy_through(state, method, uri, headers, Bytes::new()).await;
    };
    match remove_module_issue_inner(&state, &headers, &slug, &project_id, &module_id, &issue_id)
        .await
    {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

/// `POST .../modules/<pk>/archive/`.
pub async fn archive_module(
    State(state): State<AppState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    Path((slug, project_id, pk)): Path<(String, String, String)>,
    body: Bytes,
) -> Response {
    let Ok(pk) = path_uuid(&pk) else {
        return proxy_through(state, method, uri, headers, body).await;
    };
    // The archive body is ignored; drain it for the proxy path above.
    let _ = body;
    match archive_module_inner(&state, &headers, &slug, &project_id, &pk).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

/// `GET .../archived-modules/`.
pub async fn list_archived_modules(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((slug, project_id)): Path<(String, String)>,
    Query(query): Query<QueryMap>,
) -> Response {
    match list_archived_modules_inner(&state, &headers, &slug, &project_id, &query).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

/// `DELETE .../archived-modules/<pk>/unarchive/`.
pub async fn unarchive_module(
    State(state): State<AppState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    Path((slug, project_id, pk)): Path<(String, String, String)>,
) -> Response {
    let Ok(pk) = path_uuid(&pk) else {
        return proxy_through(state, method, uri, headers, Bytes::new()).await;
    };
    match unarchive_module_inner(&state, &headers, &slug, &project_id, &pk).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

/// The seven module paths (`api/urls/module.py:15-51`), each with
/// `owned()` cutover: owned methods serve from Rust, the rest proxy to
/// Django.
pub fn routes() -> axum::Router<AppState> {
    use axum::routing::{delete, get, post};
    axum::Router::new()
        .route(
            "/api/v1/workspaces/{slug}/projects/{project_id}/modules/",
            owned_list(get(list_modules).post(create_module)),
        )
        .route(
            "/api/v1/workspaces/{slug}/projects/{project_id}/modules/{pk}/",
            owned_detail(get(retrieve_module).patch(patch_module).delete(delete_module)),
        )
        .route(
            "/api/v1/workspaces/{slug}/projects/{project_id}/modules/{module_id}/module-issues/",
            owned_issue_list(get(list_module_issues).post(add_module_issues)),
        )
        .route(
            "/api/v1/workspaces/{slug}/projects/{project_id}/modules/{module_id}/module-issues/{issue_id}/",
            owned_issue_detail(delete(remove_module_issue)),
        )
        .route(
            "/api/v1/workspaces/{slug}/projects/{project_id}/modules/{pk}/archive/",
            owned_archive(post(archive_module)),
        )
        .route(
            "/api/v1/workspaces/{slug}/projects/{project_id}/archived-modules/",
            owned_archived_list(get(list_archived_modules)),
        )
        .route(
            "/api/v1/workspaces/{slug}/projects/{project_id}/archived-modules/{pk}/unarchive/",
            owned_unarchive(delete(unarchive_module)),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;
    use tower::ServiceExt;

    fn app() -> axum::Router {
        // Port 1 is never bound, so proxied (unowned) methods fail closed
        // with 502 instead of reaching a dev server.
        crate::routes::with_routes(
            AppState::with_edge(
                "0.1.0",
                crate::edge::EdgeHandle::for_tests("http://127.0.0.1:1"),
            ),
            routes(),
        )
    }

    async fn status(method: &str, uri: &str) -> StatusCode {
        let request = axum::http::Request::builder()
            .method(method)
            .uri(uri)
            .body(axum::body::Body::empty())
            .expect("request");
        app().oneshot(request).await.expect("serve").status()
    }

    /// FX-CYCMOD-08 route table: the 7 module paths, owned methods only
    /// (anonymous callers 401 at the Rust auth layer, never a proxy 502).
    #[tokio::test]
    async fn owned_methods_answer_401_anonymous() {
        let pid = "11111111-1111-1111-1111-111111111111";
        let mid = "22222222-2222-2222-2222-222222222222";
        let iid = "33333333-3333-3333-3333-333333333333";
        let base = |suffix: &str| format!("/api/v1/workspaces/acme/projects/{pid}/{suffix}");
        for (method, uri) in [
            ("GET", base("modules/")),
            ("POST", base("modules/")),
            ("GET", base(&format!("modules/{mid}/"))),
            ("PATCH", base(&format!("modules/{mid}/"))),
            ("DELETE", base(&format!("modules/{mid}/"))),
            ("GET", base(&format!("modules/{mid}/module-issues/"))),
            ("POST", base(&format!("modules/{mid}/module-issues/"))),
            (
                "DELETE",
                base(&format!("modules/{mid}/module-issues/{iid}/")),
            ),
            ("POST", base(&format!("modules/{mid}/archive/"))),
            ("GET", base("archived-modules/")),
            (
                "DELETE",
                base(&format!("archived-modules/{mid}/unarchive/")),
            ),
        ] {
            assert_eq!(
                status(method, &uri).await,
                StatusCode::UNAUTHORIZED,
                "{method} {uri}"
            );
        }
    }

    /// Unowned methods proxy to Django (502 fail-closed with the test
    /// edge): the DELETE-only issue-detail GET, PUT/PATCH anywhere, and
    /// GET on the archive path.
    #[tokio::test]
    async fn unowned_methods_proxy() {
        let pid = "11111111-1111-1111-1111-111111111111";
        let mid = "22222222-2222-2222-2222-222222222222";
        let iid = "33333333-3333-3333-3333-333333333333";
        for (method, uri) in [
            (
                "PUT",
                format!("/api/v1/workspaces/acme/projects/{pid}/modules/"),
            ),
            (
                "POST",
                format!("/api/v1/workspaces/acme/projects/{pid}/modules/{mid}/"),
            ),
            (
                "GET",
                format!(
                    "/api/v1/workspaces/acme/projects/{pid}/modules/{mid}/module-issues/{iid}/"
                ),
            ),
            (
                "GET",
                format!("/api/v1/workspaces/acme/projects/{pid}/modules/{mid}/archive/"),
            ),
            (
                "POST",
                format!("/api/v1/workspaces/acme/projects/{pid}/archived-modules/"),
            ),
            (
                "GET",
                format!("/api/v1/workspaces/acme/projects/{pid}/archived-modules/{mid}/unarchive/"),
            ),
        ] {
            assert_eq!(
                status(method, &uri).await,
                StatusCode::BAD_GATEWAY,
                "{method} {uri}"
            );
        }
    }

    /// Bad-UUID path params proxy before auth (Django's resolver 404
    /// precedes any 401): 502 fail-closed with the test edge, anonymous or
    /// not.
    #[tokio::test]
    async fn bad_uuid_params_proxy_before_auth() {
        let pid = "11111111-1111-1111-1111-111111111111";
        for (method, uri) in [
            (
                "GET",
                format!("/api/v1/workspaces/acme/projects/{pid}/modules/not-a-uuid/"),
            ),
            (
                "PATCH",
                format!("/api/v1/workspaces/acme/projects/{pid}/modules/not-a-uuid/"),
            ),
            (
                "DELETE",
                format!("/api/v1/workspaces/acme/projects/{pid}/modules/not-a-uuid/"),
            ),
            (
                "GET",
                format!("/api/v1/workspaces/acme/projects/{pid}/modules/not-a-uuid/module-issues/"),
            ),
            (
                "POST",
                format!("/api/v1/workspaces/acme/projects/{pid}/modules/not-a-uuid/module-issues/"),
            ),
            (
                "DELETE",
                format!("/api/v1/workspaces/acme/projects/{pid}/modules/22222222-2222-2222-2222-222222222222/module-issues/not-a-uuid/"),
            ),
            (
                "POST",
                format!("/api/v1/workspaces/acme/projects/{pid}/modules/not-a-uuid/archive/"),
            ),
            (
                "DELETE",
                format!("/api/v1/workspaces/acme/projects/{pid}/archived-modules/not-a-uuid/unarchive/"),
            ),
        ] {
            assert_eq!(
                status(method, &uri).await,
                StatusCode::BAD_GATEWAY,
                "{method} {uri}"
            );
        }
    }

    #[test]
    fn denial_bodies_are_exact() {
        for (denial, status, body) in [
            (
                Denial::Unauthorized,
                StatusCode::UNAUTHORIZED,
                UNAUTHENTICATED_BODY,
            ),
            (
                Denial::InvalidToken,
                StatusCode::FORBIDDEN,
                INVALID_TOKEN_BODY,
            ),
            (
                Denial::Forbidden,
                StatusCode::FORBIDDEN,
                crate::v1_cycles_modules::gates::CLASS_DENIAL_BODY,
            ),
            (Denial::NotFound, StatusCode::NOT_FOUND, NOT_FOUND_BODY),
            (
                Denial::ProjectNotFound,
                StatusCode::NOT_FOUND,
                PROJECT_NOT_FOUND_BODY,
            ),
            (
                Denial::ServerError,
                StatusCode::INTERNAL_SERVER_ERROR,
                SERVER_ERROR_BODY,
            ),
        ] {
            let (actual_status, actual_body) = denial.status_and_body();
            assert_eq!(actual_status, status);
            assert_eq!(actual_body, body);
        }
        assert_eq!(
            Denial::BadDetail("Invalid per_page parameter.".to_owned()).status_and_body(),
            (
                StatusCode::BAD_REQUEST,
                r#"{"detail":"Invalid per_page parameter."}"#.to_owned()
            )
        );
        assert_eq!(
            Denial::BadError("Issues are required.".to_owned()).status_and_body(),
            (
                StatusCode::BAD_REQUEST,
                r#"{"error":"Issues are required."}"#.to_owned()
            )
        );
    }

    #[test]
    fn parse_body_shapes() {
        assert!(parse_body(b"").expect("empty").is_empty());
        let map = parse_body(br#"{"name":"x"}"#).expect("object");
        assert_eq!(map.get("name"), Some(&Value::String("x".to_owned())));
        // DRF per-type names, byte-exact (null has its own message).
        for (raw, kind) in [
            ("[1]", "list"),
            ("\"s\"", "str"),
            ("5", "int"),
            ("5.5", "float"),
            ("true", "bool"),
            // Huge ints are still Python `int`, not `float`.
            ("1361129467683753853853498429727072845824", "int"),
            ("5e3", "float"),
        ] {
            let body = match parse_body(raw.as_bytes()).expect_err(raw) {
                Denial::FieldErrors(body) => body,
                denial => panic!("{raw}: expected FieldErrors, got {denial:?}"),
            };
            assert_eq!(
                body,
                format!(
                    r#"{{"non_field_errors":["Invalid data. Expected a dictionary, but got {kind}."]}}"#
                ),
                "{raw}"
            );
        }
        let body = match parse_body(b"null").expect_err("null") {
            Denial::FieldErrors(body) => body,
            denial => panic!("null: expected FieldErrors, got {denial:?}"),
        };
        assert_eq!(body, r#"{"non_field_errors":["No data provided"]}"#);
        // Malformed JSON carries the DRF prefix.
        let message = match parse_body(b"{oops").expect_err("unparseable") {
            Denial::BadDetail(message) => message,
            denial => panic!("expected BadDetail, got {denial:?}"),
        };
        assert!(
            message.starts_with("JSON parse error - "),
            "missing prefix: {message}"
        );
    }

    #[test]
    fn python_repr_echoes() {
        assert_eq!(py_repr(&Value::Null), "None");
        assert_eq!(py_repr(&Value::Bool(true)), "True");
        assert_eq!(py_repr(&serde_json::json!(5)), "5");
        assert_eq!(py_repr(&serde_json::json!(1.5)), "1.5");
        assert_eq!(py_repr(&serde_json::json!(["a"])), "['a']");
        assert_eq!(py_repr(&serde_json::json!([])), "[]");
        assert_eq!(py_repr(&serde_json::json!({"a": 1})), "{'a': 1}");
        let huge: Value =
            serde_json::from_str("1361129467683753853853498429727072845824").expect("big int");
        assert_eq!(py_repr(&huge), "1361129467683753853853498429727072845824");
    }

    #[test]
    fn char_coercion_edges() {
        // Required.
        assert_eq!(
            coerce_char(None, false, false, Some(255))
                .expect_err("required")
                .body,
            r#"["This field is required."]"#
        );
        // Blank.
        assert_eq!(
            coerce_char(
                Some(&Value::String("   ".to_owned())),
                false,
                false,
                Some(255)
            )
            .expect_err("blank")
            .body,
            r#"["This field may not be blank."]"#
        );
        // Blank allowed collapses to "".
        assert_eq!(
            coerce_char(Some(&Value::String("  ".to_owned())), true, false, None)
                .expect("blank-ok"),
            Some(String::new())
        );
        // Numbers stringify; bools fail.
        assert_eq!(
            coerce_char(Some(&serde_json::json!(123)), false, false, Some(255)).expect("int"),
            Some("123".to_owned())
        );
        assert_eq!(
            coerce_char(Some(&Value::Bool(true)), false, false, Some(255))
                .expect_err("bool")
                .body,
            r#"["Not a valid string."]"#
        );
        // Max length counts chars.
        assert_eq!(
            coerce_char(
                Some(&Value::String("n".repeat(256))),
                false,
                false,
                Some(255)
            )
            .expect_err("long")
            .body,
            r#"["Ensure this field has no more than 255 characters."]"#
        );
        // Values strip; max_length counts the stripped value.
        assert_eq!(
            coerce_char(
                Some(&Value::String("  ENG  ".to_owned())),
                false,
                false,
                Some(12)
            )
            .expect("trim"),
            Some("ENG".to_owned())
        );
        assert_eq!(
            coerce_char(
                Some(&Value::String(format!("  {}  ", "n".repeat(255)))),
                false,
                false,
                Some(255)
            )
            .expect("trimmed-fits"),
            Some("n".repeat(255))
        );
        // Embedded null chars fail (model validator via ModelSerializer).
        assert_eq!(
            coerce_char(
                Some(&Value::String("a\u{0}b".to_owned())),
                false,
                false,
                Some(255)
            )
            .expect_err("null-char")
            .body,
            r#"["Null characters are not allowed."]"#
        );
    }

    #[test]
    fn choice_coercion_echoes_python() {
        let choices = ["planned", "in-progress"];
        assert_eq!(
            coerce_choice(Some(&serde_json::json!(["a"])), &choices)
                .expect_err("list")
                .body,
            r#"["\"['a']\" is not a valid choice."]"#
        );
        assert_eq!(
            coerce_choice(Some(&Value::Bool(true)), &choices)
                .expect_err("bool")
                .body,
            r#"["\"True\" is not a valid choice."]"#
        );
        assert_eq!(
            coerce_choice(Some(&Value::Null), &choices)
                .expect_err("null")
                .body,
            r#"["This field may not be null."]"#
        );
        assert_eq!(
            coerce_choice(Some(&Value::String("planned".to_owned())), &choices).expect("ok"),
            Some("planned".to_owned())
        );
    }

    #[test]
    fn pk_coercion_edges() {
        // Bool is the only incorrect_type.
        assert_eq!(
            coerce_pk_value(&Value::Bool(true), true)
                .expect_err("bool")
                .body,
            r#"["Incorrect type. Expected pk value, received bool."]"#
        );
        // Empty string is None for lead, the null error for members children
        // (`RelatedField.run_validation` forces `''` to `None` before the
        // `allow_null` check — verified live against the real field classes).
        assert_eq!(
            coerce_pk_value(&Value::String(String::new()), true).expect("lead-empty"),
            PkValue::Null
        );
        assert_eq!(
            coerce_pk_value(&Value::String(String::new()), false)
                .expect_err("child-empty")
                .body,
            r#"["This field may not be null."]"#
        );
        // Curly quotes on invalid UUIDs.
        assert_eq!(
            coerce_pk_value(&Value::String("not-a-uuid".to_owned()), true)
                .expect_err("bad-uuid")
                .body,
            "[\"\u{201c}not-a-uuid\u{201d} is not a valid UUID.\"]"
        );
        assert_eq!(
            coerce_pk_value(&serde_json::json!([]), true)
                .expect_err("list")
                .body,
            "[\"\u{201c}[]\u{201d} is not a valid UUID.\"]"
        );
        // Ints coerce via UUID(int=...).
        assert_eq!(
            coerce_pk_value(&serde_json::json!(5), true).expect("int"),
            PkValue::Id(uuid::Uuid::from_u128(5))
        );
        assert!(coerce_pk_value(&serde_json::json!(-5), true).is_err());
        // Floats always fail.
        assert!(coerce_pk_value(&serde_json::json!(1.5), true).is_err());
        // Valid UUIDs pass through.
        let id = uuid::Uuid::parse_str("11111111-1111-1111-1111-111111111111").expect("uuid");
        assert_eq!(
            coerce_pk_value(&Value::String(id.to_string()), true).expect("uuid"),
            PkValue::Id(id)
        );
    }

    #[test]
    fn members_shape_edges() {
        assert_eq!(coerce_members_shape(None).expect("absent"), None);
        assert_eq!(
            coerce_members_shape(Some(&Value::Null))
                .expect_err("null")
                .body,
            r#"["This field may not be null."]"#
        );
        assert_eq!(
            coerce_members_shape(Some(&serde_json::json!({})))
                .expect_err("dict")
                .body,
            r#"["Expected a list of items but got type \"dict\"."]"#
        );
        // Numbers echo their Python type names.
        assert_eq!(
            coerce_members_shape(Some(&serde_json::json!(5)))
                .expect_err("int")
                .body,
            r#"["Expected a list of items but got type \"int\"."]"#
        );
        assert_eq!(
            coerce_members_shape(Some(&serde_json::json!(5.5)))
                .expect_err("float")
                .body,
            r#"["Expected a list of items but got type \"float\"."]"#
        );
        let items = coerce_members_shape(Some(&serde_json::json!(["a"]))).expect("list");
        assert_eq!(items, Some(vec![Value::String("a".to_owned())]));
    }

    #[test]
    fn estimate_point_none_shape() {
        // `EstimatePointSerializer(None).data`, byte-exact (key order kept).
        assert_eq!(
            serde_json::to_string(&estimate_point_none()).expect("json"),
            r#"{"deleted_at":null,"key":null,"description":"","value":"","created_by":null,"updated_by":null}"#
        );
    }

    #[test]
    fn avatar_text_arm_edges() {
        // `db/models/user.py:149-151`: `if self.avatar:` — only the empty
        // string is falsy; whitespace-only text renders as-is.
        assert_eq!(render_avatar_text(""), Value::Null);
        assert_eq!(
            render_avatar_text("https://cdn.example/x.png"),
            Value::String("https://cdn.example/x.png".to_owned())
        );
        assert_eq!(render_avatar_text("   "), Value::String("   ".to_owned()));
    }

    #[test]
    fn module_date_shapes() {
        // Django `parse_date` allows 1-2 digit month/day.
        assert_eq!(
            parse_module_date("2024-1-5"),
            chrono::NaiveDate::from_ymd_opt(2024, 1, 5)
        );
        assert_eq!(
            parse_module_date("2024-01-05"),
            chrono::NaiveDate::from_ymd_opt(2024, 1, 5)
        );
        // Calendar check + strict shape still apply.
        assert_eq!(parse_module_date("2024-13-5"), None);
        assert_eq!(parse_module_date("2024-02-30"), None);
        assert_eq!(parse_module_date("2024-1-5x"), None);
        assert_eq!(parse_module_date("24-01-05"), None);
        assert_eq!(parse_module_date("2024-001-05"), None);
        assert_eq!(parse_module_date("2024-01"), None);
    }

    #[test]
    fn module_date_iso_extras() {
        // `fromisoformat` extras Django accepts (every value verified live
        // vs Django 4.2.30): basic `YYYYMMDD`, week dates with and without
        // the day (dayless means Monday), extended and basic.
        let accepted = [
            ("2024-W05-6", (2024, 2, 3)),
            ("20240105", (2024, 1, 5)),
            ("2024-W05", (2024, 1, 29)),
            ("2024W05", (2024, 1, 29)),
            ("2024W056", (2024, 2, 3)),
            ("2024-W05-7", (2024, 2, 4)),
            ("2024-W01-1", (2024, 1, 1)),
            ("2023-W01-1", (2023, 1, 2)),
            ("2019-W01-1", (2018, 12, 31)),
            ("2021-W52-7", (2022, 1, 2)),
            ("2015-W52-7", (2015, 12, 27)),
            ("2020-W53-1", (2020, 12, 28)),
            ("2020-W53-7", (2021, 1, 3)),
            ("2015-W53-1", (2015, 12, 28)),
            ("0001-W01-1", (1, 1, 1)),
            ("0001W011", (1, 1, 1)),
            ("00010101", (1, 1, 1)),
            ("99991231", (9999, 12, 31)),
            ("9999-W52-1", (9999, 12, 27)),
            ("9999-W52-5", (9999, 12, 31)),
            ("9999W521", (9999, 12, 27)),
        ];
        for (text, (year, month, day)) in accepted {
            assert_eq!(
                parse_module_date(text),
                chrono::NaiveDate::from_ymd_opt(year, month, day),
                "{}",
                text
            );
        }
        // Rejected on both sides (verified live): ordinals, week 53 in
        // short years, week 00, day 0/8, unpadded or lowercase weeks,
        // mixed dashes, bad widths, year 0, overflow past 9999-12-31.
        let rejected = [
            "2024-036",
            "2024-060",
            "2024056",
            "2021-W53-1",
            "2024-W53-7",
            "9999W527",
            "9999-W52-6",
            "9999-W52-7",
            "2024-W05-0",
            "2024-W05-8",
            "2024-W00-1",
            "2024-W5-6",
            "2024-w05-6",
            "2024W05-6",
            "2024-W056",
            "2024-W05-06",
            "2024-W05-",
            "2024-W5",
            "2024W5",
            "2024-W",
            "2024W",
            "2024010",
            "202402031",
            "020240203",
            "20240010",
            "20240100",
            "20240230",
            "20241301",
            "00000101",
            "0000-W01-1",
            "20244-W05-6",
            "+2024-W05-6",
            "20240105\n",
            "2024-W05-6\n",
            "20240105 ",
            " 20240105",
            "2024-02-03T00:00:00",
            "",
        ];
        for text in rejected {
            assert_eq!(parse_module_date(text), None, "{}", text);
        }
    }

    #[test]
    fn issue_list_edges() {
        // Missing / empty / null.
        assert!(matches!(
            coerce_issue_list(None).expect_err("missing"),
            Denial::BadError(_)
        ));
        assert!(matches!(
            coerce_issue_list(Some(&serde_json::json!([]))).expect_err("empty"),
            Denial::BadError(_)
        ));
        assert!(matches!(
            coerce_issue_list(Some(&Value::Null)).expect_err("null"),
            Denial::ServerError
        ));
        assert!(matches!(
            coerce_issue_list(Some(&serde_json::json!(5))).expect_err("int"),
            Denial::ServerError
        ));
        // Bad UUIDs 400.
        assert!(matches!(
            coerce_issue_list(Some(&serde_json::json!(["nope"]))).expect_err("bad"),
            Denial::BadError(_)
        ));
        // Null items pass through; dicts iterate keys.
        let out = coerce_issue_list(Some(&Value::Array(vec![Value::Null]))).expect("null-item");
        assert!(matches!(out.as_slice(), [IssueCandidate::Null]));
        assert!(coerce_issue_list(Some(&serde_json::json!({"nope": 1}))).is_err());
    }

    #[test]
    fn issue_order_resolution() {
        use pidash_db::v1_cycles_modules::module_queries::{M2MOrder, OrderTarget, RelatedOrder};
        assert_eq!(resolve_issue_order(None).column, "created_at");
        assert!(!resolve_issue_order(None).descending);
        assert_eq!(resolve_issue_order(None).target, OrderTarget::Base);
        let order = resolve_issue_order(Some("-created_at"));
        assert!(order.descending);
        assert_eq!(order.column, "created_at");
        assert_eq!(order.target, OrderTarget::Base);
        // PIDASHCONV-511: bare FKs (and the reverse FK `issue_module`)
        // order by the related `Meta.ordering`, carrying the request
        // direction (the builder XORs it onto each term).
        for (name, which) in [
            ("state", RelatedOrder::State),
            ("project", RelatedOrder::Project),
            ("workspace", RelatedOrder::Workspace),
            ("parent", RelatedOrder::Parent),
            ("created_by", RelatedOrder::CreatedBy),
            ("updated_by", RelatedOrder::UpdatedBy),
            ("estimate_point", RelatedOrder::EstimatePoint),
            ("assigned_pod", RelatedOrder::AssignedPod),
            ("issue_module", RelatedOrder::IssueModule),
        ] {
            let order = resolve_issue_order(Some(name));
            assert_eq!(order.target, OrderTarget::Related(which), "{name}");
            assert_eq!(order.column, name);
            assert!(!order.descending);
            let negated = format!("-{name}");
            let order = resolve_issue_order(Some(&negated));
            assert_eq!(order.target, OrderTarget::Related(which), "{negated}");
            assert!(order.descending);
        }
        // `type` is the exception: `IssueType` has no `Meta.ordering`,
        // so Django orders by the local `type_id` with no join.
        let order = resolve_issue_order(Some("type"));
        assert_eq!(order.target, OrderTarget::Base);
        assert_eq!(order.column, "type_id");
        assert!(!order.descending);
        let order = resolve_issue_order(Some("-type"));
        assert_eq!(order.target, OrderTarget::Base);
        assert_eq!(order.column, "type_id");
        assert!(order.descending);
        // PIDASHCONV-510: exact `?` is random; `-?` passes through to
        // 500 exactly like Django's FieldError for it.
        let order = resolve_issue_order(Some("?"));
        assert_eq!(order.target, OrderTarget::Random);
        assert!(!order.descending);
        let order = resolve_issue_order(Some("-?"));
        assert_eq!(order.target, OrderTarget::Base);
        assert_eq!(order.column, "?");
        assert!(order.descending);
        // Only the annotations that exist at `.order_by()` time order by
        // alias; the late ones pass through (Django FieldErrors → 500).
        let order = resolve_issue_order(Some("sub_issues_count"));
        assert_eq!(order.target, OrderTarget::Alias);
        assert_eq!(order.column, "sub_issues_count");
        assert!(!order.descending);
        let order = resolve_issue_order(Some("-bridge_id"));
        assert_eq!(order.target, OrderTarget::Alias);
        assert!(order.descending);
        let order = resolve_issue_order(Some("link_count"));
        assert_eq!(order.target, OrderTarget::Base);
        assert_eq!(order.column, "link_count");
        let order = resolve_issue_order(Some("attachment_count"));
        assert_eq!(order.target, OrderTarget::Base);
        // Bare M2M names carry the request direction (the builder inverts
        // it onto the related `-created_at` ordering).
        let order = resolve_issue_order(Some("assignees"));
        assert_eq!(order.target, OrderTarget::M2M(M2MOrder::Assignees));
        assert!(!order.descending);
        let order = resolve_issue_order(Some("-assignees"));
        assert_eq!(order.target, OrderTarget::M2M(M2MOrder::Assignees));
        assert!(order.descending);
        let order = resolve_issue_order(Some("labels"));
        assert_eq!(order.target, OrderTarget::M2M(M2MOrder::Labels));
        // Single-level traversals onto already-joined tables; the tail
        // passes raw (bad tails 500 at the database like FieldError).
        let order = resolve_issue_order(Some("state__group"));
        assert_eq!(order.target, OrderTarget::Table("states"));
        assert_eq!(order.column, "group");
        assert!(!order.descending);
        let order = resolve_issue_order(Some("-state__group"));
        assert_eq!(order.target, OrderTarget::Table("states"));
        assert!(order.descending);
        let order = resolve_issue_order(Some("parent__created_at"));
        assert_eq!(order.target, OrderTarget::Table("T7"));
        let order = resolve_issue_order(Some("issue_module__id"));
        assert_eq!(order.target, OrderTarget::Table("module_issues"));
        let order = resolve_issue_order(Some("project__name"));
        assert_eq!(order.target, OrderTarget::Table("projects"));
        let order = resolve_issue_order(Some("workspace__slug"));
        assert_eq!(order.target, OrderTarget::Table("workspaces"));
        let order = resolve_issue_order(Some("state__nope"));
        assert_eq!(order.target, OrderTarget::Table("states"));
        assert_eq!(order.column, "nope");
        // Heads needing new joins pass through (residual divergences:
        // Django 200s, Rust 500s — documented in issue_traversal_table).
        let order = resolve_issue_order(Some("created_by__email"));
        assert_eq!(order.target, OrderTarget::Base);
        assert_eq!(order.column, "created_by__email");
        // Unknown and empty names pass through to the database 500.
        assert_eq!(resolve_issue_order(Some("bogus")).column, "bogus");
        assert_eq!(resolve_issue_order(Some("bogus")).target, OrderTarget::Base);
        let order = resolve_issue_order(Some(""));
        assert_eq!(order.target, OrderTarget::Base);
        assert_eq!(order.column, "");
    }

    #[test]
    fn m2m_envelope_total_counts_distinct() {
        // PIDASHCONV-510: M2M-multiplied rows collapse to distinct base
        // rows in the envelope totals (Django's count trims the
        // ordering-only joins).
        let a = uuid::Uuid::parse_str("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa").expect("uuid");
        let b = uuid::Uuid::parse_str("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb").expect("uuid");
        assert_eq!(count_distinct(vec![a, a, b]), 2);
        assert_eq!(count_distinct(vec![a, b]), 2);
        assert_eq!(count_distinct(vec![]), 0);
    }

    #[test]
    fn queryset_and_dump_rendering() {
        let id = uuid::Uuid::parse_str("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa").expect("uuid");
        assert_eq!(
            render_queryset_str(&[id]),
            "<SoftDeletionQuerySet [UUID('aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa')]>"
        );
        assert_eq!(render_queryset_str(&[]), "<SoftDeletionQuerySet []>");
        assert_eq!(render_bridge_dump(&[]), "[]");
        let bridge = CreatedBridge {
            id,
            created_at: chrono::DateTime::parse_from_rfc3339("2026-10-01T07:07:25.057256+00:00")
                .expect("dt")
                .to_utc(),
            updated_at: chrono::DateTime::parse_from_rfc3339("2026-10-01T07:07:25+00:00")
                .expect("dt")
                .to_utc(),
            created_by: id,
            updated_by: id,
            project_id: id,
            workspace_id: id,
            module_id: id,
            issue_id: id,
        };
        let dump = render_bridge_dump(&[bridge]);
        assert!(dump.contains(r#""model": "db.moduleissue""#), "{dump}");
        assert!(
            dump.contains(r#""created_at": "2026-10-01T07:07:25.057256+00:00""#),
            "{dump}"
        );
        // Zero micros omit the fraction (Django `isoformat`).
        assert!(
            dump.contains(r#""updated_at": "2026-10-01T07:07:25+00:00""#),
            "{dump}"
        );
    }

    #[test]
    fn module_read_order_matches_fixture_shape() {
        // FX-CYCMOD-08 handler_shapes: 29 keys in wire order (7 declared +
        // 15 non-relational + 6 relations + members).
        assert_eq!(MODULE_READ_ORDER.len(), 29);
        assert_eq!(
            &MODULE_READ_ORDER[..7],
            [
                "id",
                "total_issues",
                "cancelled_issues",
                "completed_issues",
                "started_issues",
                "unstarted_issues",
                "backlog_issues"
            ]
        );
        assert_eq!(
            &MODULE_READ_ORDER[23..],
            [
                "created_by",
                "updated_by",
                "project",
                "workspace",
                "lead",
                "members"
            ]
        );
    }

    #[test]
    fn python_truthiness_and_stored_equality() {
        assert!(!py_truthy(&Value::Null));
        assert!(!py_truthy(&serde_json::json!("")));
        assert!(!py_truthy(&serde_json::json!(0)));
        assert!(py_truthy(&serde_json::json!(5)));
        assert!(py_truthy(&serde_json::json!("x")));
        assert!(py_equals_stored(
            &Some("5".to_owned()),
            &serde_json::json!("5")
        ));
        assert!(!py_equals_stored(
            &Some("5".to_owned()),
            &serde_json::json!(5)
        ));
        assert!(py_equals_stored(&None, &Value::Null));
    }

    #[test]
    fn fields_param_edges() {
        let query: QueryMap = HashMap::new();
        assert_eq!(fields_param(&query, "fields"), None);
        let mut query: QueryMap = HashMap::new();
        query.insert(
            "fields".to_owned(),
            OneOrMany::One("name,,status".to_owned()),
        );
        assert_eq!(
            fields_param(&query, "fields"),
            Some(vec!["name".to_owned(), "status".to_owned()])
        );
    }
}
