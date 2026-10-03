//! Cycle handlers (D-20 handlers, PIDASHCONV-362).
//!
//! Ports `apps/api/pi_dash/api/views/cycle.py:80-1210` (6 view classes,
//! 8 wired routes) onto the merged D-20 foundation, registered by
//! [`routes`] at the eight `apps/api/pi_dash/api/urls/cycle.py:16-57`
//! paths.
//!
//! Layering (all foundation use is read-only): row SQL in
//! `pidash_db::v1_cycles_modules` (`cycle` models, `cycle_queries`
//! builders), read-choice + validation rules in
//! `pidash_services::v1_cycles_modules` (`cycle_shapes`,
//! `cycle_queries`), field lists + shapes in
//! `pidash_types::v1_cycles_modules::cycle_shapes`, gate decisions in
//! `super::gates` over the F-06 kernel (`pidash_auth::permissions`),
//! task kwargs in `pidash_jobs::v1_cycles_modules::publish`. This module
//! owns the HTTP shell: API-key auth, the slug→UUID rewrite, permission
//! wiring, DRF field coercion, the write statements, the read-shape
//! rendering and the paginated envelope. The shell mirrors the merged
//! D-20 `module.rs` handlers (PIDASHCONV-406) and the D-19 `v1_projects`
//! handlers (PIDASHCONV-369/371), which duplicate it per file rather
//! than cross-importing.
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
//! * `cycle_view=current` returns a bare list, not the paginated
//!   envelope every other view returns (`views/cycle.py:201-210`).
//! * Archiving a cycle with a null `end_date` compares `None >= now`,
//!   raising `TypeError` → the generic 500 envelope
//!   (`views/cycle.py:770`).
//! * PATCH on a completed cycle narrows the payload to `sort_order`,
//!   but the serializer is built from `request.data` and
//!   `CycleUpdateSerializer` has no `sort_order` field, so the edit is
//!   a 200 no-op (`views/cycle.py:512-527`); worse, a completed-cycle
//!   PATCH carrying `name`+`sort_order` edits the name.
//! * Transfer to a missing cycle dereferences `new_cycle.end_date` on
//!   the `None` from `.first()` → 500 (`utils/cycle_transfer_issues.py:
//!   59-62`); a null old-cycle `end_date` passes the completed check.
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
//! * The cycle-issues POST move path is live: `str(issue_id) in issues`
//!   compares `str` against the raw JSON strings, so cross-cycle
//!   bridges are re-pointed via `bulk_update(["cycle_id"])` (which
//!   touches only `cycle_id`, not `updated_at`/audit), and
//!   cross-project moves keep the bridge's old `project_id`
//!   (`views/cycle.py:946-987`).
//!
//! KNOWN GAPS (one root cause: serde's acceptance envelope is smaller
//! than CPython `json`'s, so these need a custom JSON parser — tracked
//! in PIDASHCONV-626, not fixable in a message table):
//!
//! * Bodies carrying `NaN`/`Infinity`/`-Infinity` or lone `\uD800-\uDFFF`
//!   surrogates: CPython accepts them (the views proceed), serde rejects
//!   them (this port 400s, keeping serde's text via the fallback).
//! * Nesting past serde's 128-deep cap: balanced deep input 500s (CPython
//!   accepts it); a mismatched closer past the cap keeps the 400 status
//!   but its text may differ (truncated deep input still recovers its
//!   exact EOF error).
//!
//! Fixture: `FX-CYCMOD-08`
//! (`rust-api/fixtures/v1_cycles_modules/handlers/cycle.golden.json`).
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

use std::collections::HashMap;

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use bytes::Bytes;
use chrono::{Datelike, LocalResult, TimeZone};
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
/// the cycle-issues POST path.
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
    /// 403 with an inline `{"error": ...}` body (the cycle DELETE
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

/// Map a write-path database failure: SQLSTATE class 23 (integrity
/// constraint violation — FK, not-null, unique, check, exclusion) is
/// Django's `IntegrityError` → 400 `{"error":"The payload is not
/// valid"}` (`handle_exception`, `api/views/base.py:136-141`); anything
/// else (bad casts like `uuid_in`, driver faults) is the generic 500.
fn db_write_error(error: sqlx::Error, site: &str) -> Denial {
    let integrity = error
        .as_database_error()
        .and_then(|db| db.code())
        .is_some_and(|code| code.starts_with("23"));
    if integrity {
        tracing::warn!(%error, site, "v1_cycles_modules integrity failure");
        Denial::BadError("The payload is not valid".to_owned())
    } else {
        db_error(error, site)
    }
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

/// `.../cycles/` owns GET+POST (`api/urls/cycle.py:17-21`).
pub fn owned_list(
    router: axum::routing::MethodRouter<AppState>,
) -> axum::routing::MethodRouter<AppState> {
    owned(router, &["GET", "POST"])
}

/// `.../cycles/<pk>/` owns GET+PATCH+DELETE (`api/urls/cycle.py:22-26`).
pub fn owned_detail(
    router: axum::routing::MethodRouter<AppState>,
) -> axum::routing::MethodRouter<AppState> {
    owned(router, &["GET", "PATCH", "DELETE"])
}

/// `.../cycles/<cycle_id>/cycle-issues/` owns GET+POST
/// (`api/urls/cycle.py:27-31`).
pub fn owned_issue_list(
    router: axum::routing::MethodRouter<AppState>,
) -> axum::routing::MethodRouter<AppState> {
    owned(router, &["GET", "POST"])
}

/// `.../cycle-issues/<issue_id>/` owns GET+DELETE
/// (`api/urls/cycle.py:32-36`).
pub fn owned_issue_detail(
    router: axum::routing::MethodRouter<AppState>,
) -> axum::routing::MethodRouter<AppState> {
    owned(router, &["GET", "DELETE"])
}

/// `.../cycles/<cycle_id>/transfer-issues/` owns POST
/// (`api/urls/cycle.py:37-41`).
pub fn owned_transfer(
    router: axum::routing::MethodRouter<AppState>,
) -> axum::routing::MethodRouter<AppState> {
    owned(router, &["POST"])
}

/// `.../cycles/<cycle_id>/archive/` owns POST (`api/urls/cycle.py:42-46`).
pub fn owned_archive(
    router: axum::routing::MethodRouter<AppState>,
) -> axum::routing::MethodRouter<AppState> {
    owned(router, &["POST"])
}

/// `.../archived-cycles/` owns GET (`api/urls/cycle.py:47-51`).
pub fn owned_archived_list(
    router: axum::routing::MethodRouter<AppState>,
) -> axum::routing::MethodRouter<AppState> {
    owned(router, &["GET"])
}

/// `.../archived-cycles/<cycle_id>/unarchive/` owns DELETE
/// (`api/urls/cycle.py:52-56`).
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

/// Gate-table path keys (`super::gates::GATES`): the route shapes the
/// permission layer matches on.
pub const PATH_CYCLES: &str = "workspaces/<slug>/projects/<id>/cycles/";
pub const PATH_CYCLE_DETAIL: &str = "workspaces/<slug>/projects/<id>/cycles/<uuid>/";
pub const PATH_CYCLE_ISSUES: &str = "workspaces/<slug>/projects/<id>/cycles/<uuid>/cycle-issues/";
pub const PATH_CYCLE_ISSUE_DETAIL: &str =
    "workspaces/<slug>/projects/<id>/cycles/<uuid>/cycle-issues/<uuid>/";
pub const PATH_CYCLE_TRANSFER: &str =
    "workspaces/<slug>/projects/<id>/cycles/<uuid>/transfer-issues/";
pub const PATH_CYCLE_ARCHIVE: &str = "workspaces/<slug>/projects/<id>/cycles/<uuid>/archive/";
pub const PATH_ARCHIVED_LIST: &str = "workspaces/<slug>/projects/<id>/archived-cycles/";
pub const PATH_CYCLE_UNARCHIVE: &str =
    "workspaces/<slug>/projects/<id>/archived-cycles/<uuid>/unarchive/";

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
/// for the `project` URL name, so cycle `pk` params never rewrite.
pub async fn rewrite_project_id(
    pool: &PgPool,
    workspace_slug: &str,
    raw: &str,
) -> Result<uuid::Uuid, Denial> {
    // Callers run this BEFORE the workspace-None 403: `_rewrite_project_kwarg`
    // runs before permission checks (`api/views/base.py:104-111`), so an
    // identifier lookup under an unknown slug 404s (`Project not found`)
    // instead of 403ing. UUID inputs skip the lookup either way.
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

fn row_f64_opt(
    row: &sqlx::postgres::PgRow,
    column: &str,
    site: &str,
) -> Result<Option<f64>, Denial> {
    row.try_get::<Option<f64>, _>(column)
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
// Cycle read shape
// ---------------------------------------------------------------------------

/// `CycleSerializer` wire order (types layer
/// `CYCLE_READ_FIELDS`, probed against live DRF): `id`, the 9 declared
/// metrics, concrete fields in model order, forward relations in model
/// order. The issue-count metrics are present only when the instance
/// carries the list annotations; the estimate metrics only on the
/// archived list, which is the sole annotator.
pub const CYCLE_READ_ORDER: [&str; 31] = [
    "id",
    "total_issues",
    "cancelled_issues",
    "completed_issues",
    "started_issues",
    "unstarted_issues",
    "backlog_issues",
    "total_estimates",
    "completed_estimates",
    "started_estimates",
    "created_at",
    "updated_at",
    "deleted_at",
    "name",
    "description",
    "start_date",
    "end_date",
    "view_props",
    "sort_order",
    "external_source",
    "external_id",
    "progress_snapshot",
    "archived_at",
    "logo_props",
    "timezone",
    "version",
    "created_by",
    "updated_by",
    "project",
    "workspace",
    "owned_by",
];

/// The annotation values of a list/detail/archived row, in wire order.
/// `estimates` is `Some` only on the archived list; the sums are integer
/// sums (`SUM("estimate_points"."key")`) rendered through DRF
/// `FloatField`, so `5` renders `5.0`.
#[derive(Debug, Clone, Copy)]
pub struct CycleAnnotations {
    pub total: i64,
    pub cancelled: i64,
    pub completed: i64,
    pub started: i64,
    pub unstarted: i64,
    pub backlog: i64,
    pub estimates: Option<CycleEstimateAnnotations>,
}

/// The three archived-list estimate sums (`None` = SQL NULL → JSON null).
#[derive(Debug, Clone, Copy)]
pub struct CycleEstimateAnnotations {
    pub total: Option<i64>,
    pub completed: Option<i64>,
    pub started: Option<i64>,
}

/// Decoded cycle row: the 22 `cycle::Cycle::COLUMNS` plus optional list
/// annotations (`None` for bare re-reads, whose missing attributes DRF
/// skips per field).
#[derive(Debug, Clone)]
pub struct CycleDetail {
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
    pub start_date: Option<chrono::DateTime<chrono::Utc>>,
    pub end_date: Option<chrono::DateTime<chrono::Utc>>,
    pub owned_by_id: uuid::Uuid,
    pub view_props: Value,
    pub sort_order: f64,
    pub external_source: Option<String>,
    pub external_id: Option<String>,
    pub progress_snapshot: Value,
    pub archived_at: Option<chrono::DateTime<chrono::Utc>>,
    pub logo_props: Value,
    pub timezone: String,
    pub version: i32,
    pub annotations: Option<CycleAnnotations>,
}

impl CycleDetail {
    pub fn decode(row: &sqlx::postgres::PgRow, site: &str) -> Result<Self, Denial> {
        Ok(CycleDetail {
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
            start_date: row_datetime_opt(row, "start_date", site)?,
            end_date: row_datetime_opt(row, "end_date", site)?,
            owned_by_id: row_uuid(row, "owned_by_id", site)?,
            view_props: row_json(row, "view_props", site)?,
            sort_order: row
                .try_get::<f64, _>("sort_order")
                .map_err(|error| db_error(error, site))?,
            external_source: row_string_opt(row, "external_source", site)?,
            external_id: row_string_opt(row, "external_id", site)?,
            progress_snapshot: row_json(row, "progress_snapshot", site)?,
            archived_at: row_datetime_opt(row, "archived_at", site)?,
            logo_props: row_json(row, "logo_props", site)?,
            timezone: row_string(row, "timezone", site)?,
            version: row_i32(row, "version", site)?,
            annotations: None,
        })
    }

    pub fn with_list_annotations(
        mut self,
        row: &sqlx::postgres::PgRow,
        site: &str,
    ) -> Result<Self, Denial> {
        self.annotations = Some(CycleAnnotations {
            total: row_i64(row, "total_issues", site)?,
            cancelled: row_i64(row, "cancelled_issues", site)?,
            completed: row_i64(row, "completed_issues", site)?,
            started: row_i64(row, "started_issues", site)?,
            unstarted: row_i64(row, "unstarted_issues", site)?,
            backlog: row_i64(row, "backlog_issues", site)?,
            estimates: None,
        });
        Ok(self)
    }

    pub fn with_archived_annotations(
        self,
        row: &sqlx::postgres::PgRow,
        site: &str,
    ) -> Result<Self, Denial> {
        let mut detail = self.with_list_annotations(row, site)?;
        let estimates = CycleEstimateAnnotations {
            total: row
                .try_get::<Option<i64>, _>("total_estimates")
                .map_err(|error| db_error(error, site))?,
            completed: row
                .try_get::<Option<i64>, _>("completed_estimates")
                .map_err(|error| db_error(error, site))?,
            started: row
                .try_get::<Option<i64>, _>("started_estimates")
                .map_err(|error| db_error(error, site))?,
        };
        if let Some(annotations) = detail.annotations.as_mut() {
            annotations.estimates = Some(estimates);
        }
        Ok(detail)
    }
}

/// Render one cycle in `CycleSerializer` wire order
/// (`serializers/cycle.py:124-155`, [`CYCLE_READ_ORDER`]).
/// `?fields=` filters first (order preserved), then `?expand=` applies to
/// the surviving keys (`serializers/base.py:59-118`).
pub async fn render_cycle(
    pool: &PgPool,
    detail: &CycleDetail,
    timezone: &Tz,
    fields: Option<&[String]>,
    expand: Option<&[String]>,
) -> Result<Value, Denial> {
    let mut map = serde_json::Map::with_capacity(32);
    if let Some(ann) = detail.annotations {
        map.insert("id".to_owned(), render_uuid(&detail.id));
        map.insert("total_issues".to_owned(), Value::from(ann.total));
        map.insert("cancelled_issues".to_owned(), Value::from(ann.cancelled));
        map.insert("completed_issues".to_owned(), Value::from(ann.completed));
        map.insert("started_issues".to_owned(), Value::from(ann.started));
        map.insert("unstarted_issues".to_owned(), Value::from(ann.unstarted));
        map.insert("backlog_issues".to_owned(), Value::from(ann.backlog));
        if let Some(est) = ann.estimates {
            map.insert(
                "total_estimates".to_owned(),
                est.total.map_or(Value::Null, |v| render_f64(v as f64)),
            );
            map.insert(
                "completed_estimates".to_owned(),
                est.completed.map_or(Value::Null, |v| render_f64(v as f64)),
            );
            map.insert(
                "started_estimates".to_owned(),
                est.started.map_or(Value::Null, |v| render_f64(v as f64)),
            );
        }
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
        "start_date".to_owned(),
        render_datetime_opt(&detail.start_date, timezone),
    );
    map.insert(
        "end_date".to_owned(),
        render_datetime_opt(&detail.end_date, timezone),
    );
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
        "progress_snapshot".to_owned(),
        detail.progress_snapshot.clone(),
    );
    map.insert(
        "archived_at".to_owned(),
        render_datetime_opt(&detail.archived_at, timezone),
    );
    map.insert("logo_props".to_owned(), detail.logo_props.clone());
    map.insert(
        "timezone".to_owned(),
        Value::String(detail.timezone.clone()),
    );
    map.insert("version".to_owned(), Value::from(detail.version));
    map.insert("created_by".to_owned(), render_uuid_opt(&detail.created_by));
    map.insert("updated_by".to_owned(), render_uuid_opt(&detail.updated_by));
    map.insert("project".to_owned(), render_uuid(&detail.project_id));
    map.insert("workspace".to_owned(), render_uuid(&detail.workspace_id));
    map.insert("owned_by".to_owned(), render_uuid(&detail.owned_by_id));
    // `BaseSerializer._filter_fields` (`serializers/base.py:59-88`):
    // keep only the requested keys (order preserved).
    if let Some(fields) = fields {
        map.retain(|key, _| fields.iter().any(|f| f == key));
    }
    // `BaseSerializer.to_representation` (`serializers/base.py:90-118`):
    // expand runs on the filtered keys.
    if let Some(expand) = expand {
        apply_cycle_expand(pool, detail, &mut map, expand).await?;
    }
    Ok(Value::Object(map))
}

/// `?expand=` for cycle rows: the `BaseSerializer.expansion` map hits
/// (`project`, `workspace`, `created_by`, `updated_by`, `owned_by`;
/// every other key falls through to
/// `getattr(instance, f"{expand}_id", None)`). No cycle key outside the
/// map has an `{expand}_id` attribute, so every fallthrough nulls the
/// key (the `expand=total_issues` precedent, verified live on modules).
/// A map hit on a null FK renders `{}` (single-object `None`).
pub async fn apply_cycle_expand(
    pool: &PgPool,
    detail: &CycleDetail,
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
            "owned_by" => {
                map.insert(
                    "owned_by".to_owned(),
                    expand_user(pool, &detail.owned_by_id).await?,
                );
            }
            _ => {
                map.insert(name.clone(), Value::Null);
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// CycleIssue (bridge) read shape
// ---------------------------------------------------------------------------

/// `CycleIssueSerializer` wire order (types layer
/// `CYCLE_ISSUE_READ_FIELDS`, probed): declared `id` +
/// `sub_issues_count`, concrete fields, forward relations in model order
/// (`created_by`, `updated_by`, `project`, `workspace`, `issue`, `cycle`).
/// `sub_issues_count` is annotation-fed: the list queryset and the POST
/// response carry it, but the detail GET (a bare `.get()` with no
/// annotation, `views/cycle.py:1069-1074`) omits the key.
#[derive(Debug, Clone)]
pub struct BridgeDetail {
    pub id: uuid::Uuid,
    pub sub_issues_count: Option<i64>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_by: Option<uuid::Uuid>,
    pub updated_by: Option<uuid::Uuid>,
    pub project_id: uuid::Uuid,
    pub workspace_id: uuid::Uuid,
    pub cycle_id: uuid::Uuid,
    pub issue_id: uuid::Uuid,
}

impl BridgeDetail {
    pub fn decode(row: &sqlx::postgres::PgRow, site: &str) -> Result<Self, Denial> {
        Ok(BridgeDetail {
            id: row_uuid(row, "id", site)?,
            sub_issues_count: None,
            created_at: row_datetime(row, "created_at", site)?,
            updated_at: row_datetime(row, "updated_at", site)?,
            deleted_at: row_datetime_opt(row, "deleted_at", site)?,
            created_by: row_uuid_opt(row, "created_by_id", site)?,
            updated_by: row_uuid_opt(row, "updated_by_id", site)?,
            project_id: row_uuid(row, "project_id", site)?,
            workspace_id: row_uuid(row, "workspace_id", site)?,
            cycle_id: row_uuid(row, "cycle_id", site)?,
            issue_id: row_uuid(row, "issue_id", site)?,
        })
    }

    pub fn with_sub_issues_count(
        mut self,
        row: &sqlx::postgres::PgRow,
        site: &str,
    ) -> Result<Self, Denial> {
        self.sub_issues_count = Some(row_i64(row, "sub_issues_count", site)?);
        Ok(self)
    }
}

/// Render one bridge row in `CycleIssueSerializer` wire order
/// (`serializers/cycle.py:158-171`). The POST response passes no
/// `fields`/`expand`; the detail GET passes both
/// (`views/cycle.py:1075`).
pub async fn render_bridge(
    pool: &PgPool,
    state: &AppState,
    detail: &BridgeDetail,
    timezone: &Tz,
    fields: Option<&[String]>,
    expand: Option<&[String]>,
) -> Result<Value, Denial> {
    let mut map = serde_json::Map::with_capacity(11);
    map.insert("id".to_owned(), render_uuid(&detail.id));
    if let Some(count) = detail.sub_issues_count {
        map.insert("sub_issues_count".to_owned(), Value::from(count));
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
    map.insert("created_by".to_owned(), render_uuid_opt(&detail.created_by));
    map.insert("updated_by".to_owned(), render_uuid_opt(&detail.updated_by));
    map.insert("project".to_owned(), render_uuid(&detail.project_id));
    map.insert("workspace".to_owned(), render_uuid(&detail.workspace_id));
    map.insert("issue".to_owned(), render_uuid(&detail.issue_id));
    map.insert("cycle".to_owned(), render_uuid(&detail.cycle_id));
    // `BaseSerializer._filter_fields` (`serializers/base.py:59-88`).
    if let Some(fields) = fields {
        map.retain(|key, _| fields.iter().any(|f| f == key));
    }
    // `BaseSerializer.to_representation` (`serializers/base.py:90-118`).
    if let Some(expand) = expand {
        apply_bridge_expand(pool, state, detail, timezone, &mut map, expand).await?;
    }
    Ok(Value::Object(map))
}

/// `?expand=` for bridge rows: the `BaseSerializer.expansion` map hits
/// (`project`, `workspace`, `created_by`, `updated_by`, and `issue` →
/// the full `IssueSerializer`, not the lite one); `cycle` falls through
/// to `getattr(instance, "cycle_id")` (the unchanged id); every other
/// key nulls (`getattr(instance, f"{expand}_id", None)`).
pub async fn apply_bridge_expand(
    pool: &PgPool,
    state: &AppState,
    detail: &BridgeDetail,
    timezone: &Tz,
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
            "issue" => {
                map.insert(
                    "issue".to_owned(),
                    expand_issue_full(pool, state, &detail.issue_id, timezone).await?,
                );
            }
            "cycle" => {
                map.insert("cycle".to_owned(), render_uuid(&detail.cycle_id));
            }
            _ => {
                map.insert(name.clone(), Value::Null);
            }
        }
    }
    Ok(())
}

/// Bridge `expand=issue`: the full `IssueSerializer` rendering of the
/// linked issue (`serializers/base.py:105-109`), with no nested
/// `fields`/`expand` (the expansion call passes none).
pub async fn expand_issue_full(
    pool: &PgPool,
    state: &AppState,
    issue_id: &uuid::Uuid,
    timezone: &Tz,
) -> Result<Value, Denial> {
    // No `deleted_at` filter: `getattr(instance, "issue")` resolves through
    // `_base_manager` (unfiltered), so a soft-deleted issue still renders
    // 200 here (verified live against Django).
    let row = sqlx::query(r#"SELECT * FROM "issues" WHERE "id" = $1"#)
        .bind(issue_id)
        .fetch_optional(pool)
        .await
        .map_err(|error| db_error(error, "bridge-expand-issue"))?;
    let Some(row) = row else {
        return Err(Denial::NotFound);
    };
    let detail = IssueDetail::decode(&row, "bridge-expand-issue")?;
    let assignees = fetch_assignees(pool, issue_id).await?;
    let labels = fetch_issue_labels(pool, issue_id).await?;
    // The `url` identifier resolves through the unfiltered descriptor
    // (`_base_manager`, PK-only): a soft-deleted project still renders its
    // url (Django 200 — F-N6). Only a hard-missing row 404s (unreachable
    // via the FK; preserves the previous 404).
    let identifier: Option<String> =
        sqlx::query_scalar(r#"SELECT "identifier" FROM "projects" WHERE "id" = $1"#)
            .bind(detail.project_id)
            .fetch_optional(pool)
            .await
            .map_err(|error| db_error(error, "bridge-expand-identifier"))?
            .flatten();
    let Some(identifier) = identifier else {
        return Err(Denial::NotFound);
    };
    let slug = workspace_slug(pool, &detail.workspace_id).await?;
    let url = issue_url(state, &slug, &identifier, detail.sequence_id);
    let mut rendered = render_issue(
        pool, &detail, &assignees, &labels, url, timezone, None, None,
    )
    .await?;
    // Single-item `IssueSerializer` appends the blocker keys
    // (`serializers/issue.py:474-481`; skipped only under `many=True`,
    // which the list path uses — this expansion is always single, with no
    // requested-fields gating and no relations viewer).
    if let Value::Object(ref mut map) = rendered {
        map.insert(
            "relations_summary".to_owned(),
            fetch_relations_summary(pool, issue_id).await?,
        );
        map.insert(
            "has_open_blockers".to_owned(),
            Value::Bool(has_open_blockers(pool, issue_id).await?),
        );
    }
    Ok(rendered)
}

/// Per-direction cap on `relations_summary` lists
/// (`orchestration/blockers.py:SUMMARY_LIMIT`).
const SUMMARY_LIMIT: i64 = 100;

/// `{"relations_summary": {"blocked_by": [...], "blocking": [...]}}`
/// (`orchestration/blockers.py:relations_summary`): live targets ordered
/// open-first, then project identifier, then sequence, capped per
/// direction. Each item is `{identifier, state, state_group}`.
pub async fn fetch_relations_summary(
    pool: &PgPool,
    issue_id: &uuid::Uuid,
) -> Result<Value, Denial> {
    let mut summary = serde_json::Map::with_capacity(2);
    summary.insert(
        "blocked_by".to_owned(),
        Value::Array(fetch_summary_list(pool, issue_id, false).await?),
    );
    summary.insert(
        "blocking".to_owned(),
        Value::Array(fetch_summary_list(pool, issue_id, true).await?),
    );
    Ok(Value::Object(summary))
}

/// One direction of `relations_summary` (`_summary_list` over
/// `blockers_queryset` / `dependents_queryset`): live relation rows
/// (self-refs excluded), forward `blocked_by` plus stored-reversed
/// `blocking` edges, targets in `Issue.issue_objects` (live, non-triage,
/// unarchived issue and project, non-draft) in the relation's workspace.
/// Open targets first (a stateless target is open), then project
/// identifier, then sequence; capped at [`SUMMARY_LIMIT`].
pub async fn fetch_summary_list(
    pool: &PgPool,
    issue_id: &uuid::Uuid,
    blocking: bool,
) -> Result<Vec<Value>, Denial> {
    // `_blocked_by_edges` / `_blocking_edges`: in the forward rows the
    // target sits on one end, in the stored-reversed rows on the other.
    let edges = if blocking {
        r#"(r."related_issue_id" = $1 AND r."relation_type" = 'blocked_by' AND t."id" = r."issue_id")
        OR (r."issue_id" = $1 AND r."relation_type" = 'blocking' AND t."id" = r."related_issue_id")"#
    } else {
        r#"(r."issue_id" = $1 AND r."relation_type" = 'blocked_by' AND t."id" = r."related_issue_id")
        OR (r."related_issue_id" = $1 AND r."relation_type" = 'blocking' AND t."id" = r."issue_id")"#
    };
    let sql = format!(
        r#"SELECT DISTINCT p."identifier", t."sequence_id", s."name", s."group",
                  CASE WHEN s."group" IN ('completed', 'cancelled') THEN 1 ELSE 0 END AS "resolved"
           FROM "issue_relations" r
           JOIN "issues" t ON ({edges})
           JOIN "projects" p ON p."id" = t."project_id"
           LEFT JOIN "states" s ON s."id" = t."state_id"
           WHERE r."deleted_at" IS NULL AND r."issue_id" <> r."related_issue_id"
             AND t."workspace_id" = r."workspace_id"
             AND t."deleted_at" IS NULL AND t."archived_at" IS NULL AND NOT t."is_draft"
             AND (s."id" IS NULL OR s."group" <> 'triage')
             AND p."archived_at" IS NULL
           ORDER BY "resolved", p."identifier", t."sequence_id"
           LIMIT {SUMMARY_LIMIT}"#,
    );
    let rows: Vec<sqlx::postgres::PgRow> = sqlx::query(&sql)
        .bind(issue_id)
        .fetch_all(pool)
        .await
        .map_err(|error| db_error(error, "relations-summary"))?;
    let mut items = Vec::with_capacity(rows.len());
    for row in &rows {
        let identifier = row_string(row, "identifier", "relations-summary")?;
        let sequence_id: i32 = row
            .try_get("sequence_id")
            .map_err(|error| db_error(error, "relations-summary"))?;
        let mut item = serde_json::Map::with_capacity(3);
        item.insert(
            "identifier".to_owned(),
            Value::String(format!("{identifier}-{sequence_id}")),
        );
        item.insert(
            "state".to_owned(),
            render_string_opt(&row_string_opt(row, "name", "relations-summary")?),
        );
        item.insert(
            "state_group".to_owned(),
            render_string_opt(&row_string_opt(row, "group", "relations-summary")?),
        );
        items.push(Value::Object(item));
    }
    Ok(items)
}

/// `has_open_blockers` over the FULL blocker set (never the capped list):
/// a live `blocked_by` target whose state group is neither `completed`
/// nor `cancelled` — a stateless target counts as open.
pub async fn has_open_blockers(pool: &PgPool, issue_id: &uuid::Uuid) -> Result<bool, Denial> {
    let open: bool = sqlx::query_scalar(
        r#"SELECT EXISTS(
             SELECT 1 FROM "issue_relations" r
             JOIN "issues" t ON ((r."issue_id" = $1 AND r."relation_type" = 'blocked_by' AND t."id" = r."related_issue_id")
               OR (r."related_issue_id" = $1 AND r."relation_type" = 'blocking' AND t."id" = r."issue_id"))
             JOIN "projects" p ON p."id" = t."project_id"
             LEFT JOIN "states" s ON s."id" = t."state_id"
             WHERE r."deleted_at" IS NULL AND r."issue_id" <> r."related_issue_id"
               AND t."workspace_id" = r."workspace_id"
               AND t."deleted_at" IS NULL AND t."archived_at" IS NULL AND NOT t."is_draft"
               AND (s."id" IS NULL OR s."group" <> 'triage')
               AND p."archived_at" IS NULL
               AND (s."group" IS NULL OR s."group" NOT IN ('completed', 'cancelled')))"#,
    )
    .bind(issue_id)
    .fetch_one(pool)
    .await
    .map_err(|error| db_error(error, "has-open-blockers"))?;
    Ok(open)
}

/// The workspace slug for `IssueSerializer.get_url`
/// (`serializers/issue.py:126-137`): `instance.workspace.slug`.
pub async fn workspace_slug(pool: &PgPool, workspace_id: &uuid::Uuid) -> Result<String, Denial> {
    // Deliberately unfiltered: `instance.workspace` resolves through
    // `_base_manager`, so a soft-deleted workspace still renders its slug.
    let slug: Option<String> =
        sqlx::query_scalar(r#"SELECT "slug" FROM "workspaces" WHERE "id" = $1"#)
            .bind(workspace_id)
            .fetch_optional(pool)
            .await
            .map_err(|error| db_error(error, "workspace-slug"))?
            .flatten();
    slug.ok_or(Denial::NotFound)
}

// ---------------------------------------------------------------------------
// Issue read shape (cycle-issues GET)
// ---------------------------------------------------------------------------

/// Decoded issue row for the cycle-issues GET: `IssueSerializer`
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
    // Unlike the module shape, the issue appends are conditional on the keys
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
    // `Label.objects.filter(...)` (`serializers/issue.py:456-461`) is an
    // explicit `objects` query, so the soft-deletion filter applies and
    // deleted labels are excluded (verified live; the id list in
    // `fetch_issue_labels` keeps them, matching `values_list` on the
    // bridge table alone).
    let rows: Vec<sqlx::postgres::PgRow> = sqlx::query(
        r#"SELECT l."id", l."created_at", l."updated_at", l."deleted_at", l."name", l."description", l."color",
                  l."sort_order", l."external_source", l."external_id", l."created_by_id", l."updated_by_id",
                  l."workspace_id", l."project_id", l."parent_id"
           FROM "labels" l
           WHERE l."id" IN (SELECT "label_id" FROM "issue_labels" WHERE "issue_id" = $1 AND "deleted_at" IS NULL)
             AND l."deleted_at" IS NULL
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

/// Parse the request body into `request.data`: an empty body is `{}`
/// (DRF's empty-stream default); malformed JSON is the DRF `ParseError`
/// whose suffix is CPython's `json` error text ([`json_parse_denial`]).
pub fn parse_body_value(raw: &[u8]) -> Result<Value, Denial> {
    if raw.is_empty() {
        return Ok(Value::Object(serde_json::Map::new()));
    }
    serde_json::from_slice::<Value>(raw).map_err(|error| json_parse_denial(raw, &error))
}

/// Raw `request.data` object for the serializer-free views (create/add/
/// transfer): empty is `{}`, unparseable is the DRF `ParseError`, and a
/// non-object body 500s on `.get` (`AttributeError` — verified live for
/// `[]`/`null`/`"x"`/`5`/`true` on all three paths).
pub fn parse_object_or_500(raw: &[u8]) -> Result<serde_json::Map<String, Value>, Denial> {
    match parse_body_value(raw)? {
        Value::Object(map) => Ok(map),
        _ => Err(Denial::ServerError),
    }
}

/// Parse the request body (`is_valid()` input stage): [`parse_body_value`]
/// plus the serializer `non_field_errors` for non-object JSON values —
/// with DRF's per-type names (`int`/`float`/`bool`/`str`/`list`) and
/// `null` answering `No data provided` (all verified live).
pub fn parse_body(raw: &[u8]) -> Result<serde_json::Map<String, Value>, Denial> {
    coerce_body_object(parse_body_value(raw)?)
}

/// The serializer input stage over an already-parsed value: objects pass
/// through, every other JSON type is its DRF `non_field_errors` shape.
/// (Split from [`parse_body`] so PATCH can run the completed gate on the
/// raw value first — `views/cycle.py:512-520`.)
pub fn coerce_body_object(value: Value) -> Result<serde_json::Map<String, Value>, Denial> {
    match value {
        Value::Object(map) => Ok(map),
        Value::Null => Err(Denial::FieldErrors(
            r#"{"non_field_errors":["No data provided"]}"#.to_owned(),
        )),
        other => {
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
    }
}

/// Prefix of every DRF `ParseError` detail (`rest_framework/parsers.py`).
const JSON_PARSE_PREFIX: &str = "JSON parse error - ";

/// Map a body-parse failure to DRF's `ParseError` shape. The suffix is
/// CPython's `json` error text, not serde's: Django answers e.g.
/// `Expecting property name enclosed in double quotes: line 1 column 2
/// (char 1)` where serde says `key must be a string at line 1 column 2`.
/// Positions are recomputed as char (not byte) offsets, since serde
/// columns count bytes. Past serde's 128-deep recursion cap a truncated
/// input still recovers its plain EOF error; a balanced deep input takes
/// the generic 500 (CPython accepts it, or `RecursionError`s — not a
/// `ValueError`, so DRF does not catch it — at extreme depths).
/// (Fuzzed against CPython 3.12 over structured mutations + random bytes;
/// see `json_parse_cpython_parity`. serde_json 1.0.151 message texts —
/// the battery pins them.)
pub fn json_parse_denial(raw: &[u8], error: &serde_json::Error) -> Denial {
    // DRF decodes the stream before parsing, so a codec failure wins over
    // any syntax error.
    if std::str::from_utf8(raw).is_err() {
        return Denial::BadDetail(format!("{JSON_PARSE_PREFIX}{}", utf8_decode_detail(raw)));
    }
    let text = std::str::from_utf8(raw).expect("UTF-8 checked");
    let message = error.to_string();
    if message.starts_with("recursion limit exceeded") {
        // Serde caps nesting at 128; CPython goes far deeper. Past the cap
        // a TRUNCATED input is still a plain EOF error (recover it); a
        // balanced deep input is beyond serde (CPython accepts it, or
        // RecursionErrors to Django's 500 at extreme depths — the generic
        // 500 is the closest single answer).
        if bracket_depth(text) > 0 {
            let (template, pos) = eof_detail(text);
            let (line, column, char) = cpython_pos(text, pos);
            return Denial::BadDetail(format!(
                "{JSON_PARSE_PREFIX}{template}: line {line} column {column} (char {char})"
            ));
        }
        return Denial::ServerError;
    }
    match cpython_json_detail(text, &message, error.line(), error.column()) {
        Some(detail) => Denial::BadDetail(format!("{JSON_PARSE_PREFIX}{detail}")),
        // Unmapped arm (lone-surrogate / NaN accept-divergences, future
        // serde codes): serde text. CPython ACCEPTS those inputs, so no
        // 400 text is right; see KNOWN GAPS in the module docs.
        None => Denial::BadDetail(format!("{JSON_PARSE_PREFIX}{message}")),
    }
}

/// CPython's `UnicodeDecodeError` text for the first bad sequence
/// (`codecs.getreader("utf-8")`, strict): the lead byte selects the
/// reason, the valid-continuation run selects the byte/bytes form.
fn utf8_decode_detail(raw: &[u8]) -> String {
    let start = std::str::from_utf8(raw)
        .expect_err("invalid UTF-8 checked")
        .valid_up_to();
    let lead = raw[start];
    let expected: Option<usize> = match lead {
        0xC2..=0xDF => Some(2),
        0xE0..=0xEF => Some(3),
        0xF0..=0xF4 => Some(4),
        _ => None,
    };
    let Some(expected) = expected else {
        // Stray continuation, overlong C0/C1, or F5-FF lead.
        return format!(
            "'utf-8' codec can't decode byte 0x{lead:02x} in position {start}: invalid start byte"
        );
    };
    let mut run = 1;
    while run < expected && start + run < raw.len() && (0x80..=0xBF).contains(&raw[start + run]) {
        run += 1;
    }
    // The second byte has range checks (overlong E0/F0, surrogate ED,
    // above-maximum F4): out of range fails at the lead even when
    // truncated (`\xed\xa0` + EOF → invalid continuation, not end of
    // data).
    if run >= 2 {
        let second = raw[start + 1];
        let in_range = match lead {
            0xE0 => (0xA0..=0xBF).contains(&second),
            0xED => (0x80..=0x9F).contains(&second),
            0xF0 => (0x90..=0xBF).contains(&second),
            0xF4 => (0x80..=0x8F).contains(&second),
            _ => true,
        };
        if !in_range {
            return format!("'utf-8' codec can't decode byte 0x{lead:02x} in position {start}: invalid continuation byte");
        }
    }
    if run == expected {
        // Full-length but range-invalid (overlong E0/F0, surrogate ED,
        // above-maximum F4): reported at the lead byte.
        return format!("'utf-8' codec can't decode byte 0x{lead:02x} in position {start}: invalid continuation byte");
    }
    if start + run == raw.len() {
        if run == 1 {
            return format!("'utf-8' codec can't decode byte 0x{lead:02x} in position {start}: unexpected end of data");
        }
        return format!(
            "'utf-8' codec can't decode bytes in position {start}-{}: unexpected end of data",
            start + run - 1
        );
    }
    if run == 1 {
        return format!("'utf-8' codec can't decode byte 0x{lead:02x} in position {start}: invalid continuation byte");
    }
    format!(
        "'utf-8' codec can't decode bytes in position {start}-{}: invalid continuation byte",
        start + run - 1
    )
}

/// CPython `json` error text for a serde failure: the template plus the
/// char-based `(line, column, char)` triple. `None` marks an arm with no
/// CPython error (lone surrogates, NaN/Infinity — all accepted) or an
/// unknown serde code.
fn cpython_json_detail(text: &str, message: &str, line: usize, column: usize) -> Option<String> {
    // A leading BOM is CPython's one special case (anywhere else it is an
    // ordinary char, and inside strings a literal).
    if text.starts_with('\u{FEFF}') {
        return Some(
            "Unexpected UTF-8 BOM (decode using utf-8-sig): line 1 column 1 (char 0)".to_owned(),
        );
    }
    let offset = json_byte_offset(text, line, column);
    let prefix = message.split(" at line ").next().unwrap_or(message);
    let (template, pos) = match prefix {
        "key must be a string" => ("Expecting property name enclosed in double quotes", offset),
        "expected `:`" => ("Expecting ':' delimiter", offset),
        "expected `,` or `]`" | "expected `,` or `}`" => ("Expecting ',' delimiter", offset),
        "trailing characters" => ("Extra data", offset),
        "expected value" => ("Expecting value", offset),
        "trailing comma" => match innermost_bracket(text, offset) {
            Some(b'[') => ("Expecting value", offset),
            Some(b'{') => ("Expecting property name enclosed in double quotes", offset),
            _ => ("Extra data", offset),
        },
        "expected ident" => ("Expecting value", json_token_start(text, offset)),
        "invalid number" => invalid_number_detail(text, offset),
        "control character (\\u0000-\\u001F) found while parsing a string" => {
            ("Invalid control character at", offset)
        }
        "invalid escape" => invalid_escape_detail(text, offset)?,
        "EOF while parsing a string" => eof_string_detail(text)?,
        "EOF while parsing a value"
        | "EOF while parsing a list"
        | "EOF while parsing an object" => eof_detail(text),
        // Surrogate escapes: terminated ones CPython accepts (known gap),
        // but an UNCLOSED string after them is still an unterminated (or
        // truncated-escape) error, handled like EOF-in-string.
        "unexpected end of hex escape"
        | "lone leading surrogate in hex escape"
        | "invalid unicode code point" => {
            if string_closed_after(text, offset) {
                return None;
            }
            eof_string_detail(text)?
        }
        _ => return None,
    };
    let (line, column, char) = cpython_pos(text, pos);
    Some(format!(
        "{template}: line {line} column {column} (char {char})"
    ))
}

/// Byte offset of serde's 1-based `(line, column)` (columns count bytes).
/// Column 0 reports the `\n` itself (serde advances the line before
/// resetting the column), so it maps to the previous byte.
fn json_byte_offset(text: &str, line: usize, column: usize) -> usize {
    let mut start = 0;
    for _ in 1..line.max(1) {
        match text[start..].find('\n') {
            Some(index) => start += index + 1,
            None => return text.len(),
        }
    }
    if column == 0 {
        return start.saturating_sub(1);
    }
    start.saturating_add(column - 1).min(text.len())
}

/// CPython's 1-based `(line, column)` + 0-based char index for a byte
/// offset (chars, not bytes, past any multibyte text).
fn cpython_pos(text: &str, pos: usize) -> (usize, usize, usize) {
    let pos = pos.min(text.len());
    let prefix = &text[..pos];
    let line = prefix.bytes().filter(|b| *b == b'\n').count() + 1;
    let column = prefix.rsplit('\n').next().unwrap_or("").chars().count() + 1;
    (line, column, prefix.chars().count())
}

/// Whether a byte continues a broken token when scanning back from a
/// serde offset: anything but structure, quotes and whitespace.
fn is_token_byte(byte: u8) -> bool {
    !matches!(byte, b'[' | b']' | b'{' | b'}' | b',' | b':' | b'"')
        && !matches!(byte, b' ' | b'\t' | b'\n' | b'\r')
}

/// Start of the broken token containing `offset` (partial idents,
/// broken numbers): scan back over token bytes.
fn json_token_start(text: &str, offset: usize) -> usize {
    let bytes = text.as_bytes();
    let mut start = offset.min(bytes.len());
    while start > 0 && is_token_byte(bytes[start - 1]) {
        start -= 1;
    }
    start
}

/// Net unclosed-bracket depth (string-aware; closers saturate at zero):
/// tells truncated deep input (plain EOF error) from balanced deep input
/// (past serde's recursion cap).
fn bracket_depth(text: &str) -> usize {
    let bytes = text.as_bytes();
    let mut depth: usize = 0;
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'"' => {
                index += 1;
                while index < bytes.len() && bytes[index] != b'"' {
                    if bytes[index] == b'\\' {
                        index += 1;
                    }
                    index += 1;
                }
                index += 1;
            }
            b'{' | b'[' => {
                depth += 1;
                index += 1;
            }
            b'}' | b']' => {
                depth = depth.saturating_sub(1);
                index += 1;
            }
            _ => index += 1,
        }
    }
    depth
}

/// Innermost unclosed bracket before `offset` (string-aware): the
/// trailing-comma context and the top-level test.
fn innermost_bracket(text: &str, offset: usize) -> Option<u8> {
    let bytes = text.as_bytes();
    let end = offset.min(bytes.len());
    let mut stack: Vec<u8> = Vec::new();
    let mut index = 0;
    while index < end {
        match bytes[index] {
            b'"' => {
                index += 1;
                while index < end && bytes[index] != b'"' {
                    if bytes[index] == b'\\' {
                        index += 1;
                    }
                    index += 1;
                }
                index += 1;
            }
            b'{' | b'[' => {
                stack.push(bytes[index]);
                index += 1;
            }
            b'}' | b']' => {
                stack.pop();
                index += 1;
            }
            _ => index += 1,
        }
    }
    stack.pop()
}

/// Match CPython's `number_re` at `start`: the valid-prefix end, if any
/// (`-?(0|[1-9]\d*)(\.\d+)?([eE][-+]?\d+)?`).
fn number_prefix_end(bytes: &[u8], start: usize) -> Option<usize> {
    let mut index = start;
    if bytes.get(index) == Some(&b'-') {
        index += 1;
    }
    match bytes.get(index) {
        Some(b'0') => index += 1,
        Some(b'1'..=b'9') => {
            while matches!(bytes.get(index), Some(b'0'..=b'9')) {
                index += 1;
            }
        }
        _ => return None,
    }
    if bytes.get(index) == Some(&b'.') && matches!(bytes.get(index + 1), Some(b'0'..=b'9')) {
        index += 2;
        while matches!(bytes.get(index), Some(b'0'..=b'9')) {
            index += 1;
        }
    }
    if matches!(bytes.get(index), Some(b'e' | b'E')) {
        let mut end = index + 1;
        if matches!(bytes.get(end), Some(b'+' | b'-')) {
            end += 1;
        }
        if matches!(bytes.get(end), Some(b'0'..=b'9')) {
            end += 1;
            while matches!(bytes.get(end), Some(b'0'..=b'9')) {
                end += 1;
            }
            index = end;
        }
    }
    Some(index)
}

/// CPython text for serde's `invalid number`: with no valid prefix the
/// token start wants a value (`-x`); else CPython consumed the prefix
/// and wants a delimiter — `Extra data` at top level, `Expecting ','`
/// inside (CPython says `,` even in objects).
fn invalid_number_detail(text: &str, offset: usize) -> (&'static str, usize) {
    let start = json_token_start(text, offset);
    match number_prefix_end(text.as_bytes(), start) {
        Some(end) if end > start => {
            if innermost_bracket(text, start).is_none() {
                ("Extra data", end)
            } else {
                ("Expecting ',' delimiter", end)
            }
        }
        _ => ("Expecting value", start),
    }
}

/// `Invalid \escape` vs `Invalid \uXXXX escape`: the culprit backslash is
/// the nearest `\` before the offset; `\u` reports the `u` index.
fn invalid_escape_detail(text: &str, offset: usize) -> Option<(&'static str, usize)> {
    let bytes = text.as_bytes();
    let mut index = offset.min(bytes.len());
    for _ in 0..16 {
        if index == 0 {
            return None;
        }
        index -= 1;
        if bytes[index] == b'\\' {
            if bytes.get(index + 1) == Some(&b'u') {
                return Some(("Invalid \\uXXXX escape", index + 1));
            }
            return Some(("Invalid \\escape", index));
        }
    }
    None
}

/// Opening quote of the string unterminated at `offset`: the nearest `"`
/// with an even run of preceding backslashes.
fn json_opening_quote(text: &str, offset: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut index = offset.min(bytes.len());
    while index > 0 {
        index -= 1;
        if bytes[index] == b'"' {
            let mut slashes = 0;
            let mut cursor = index;
            while cursor > 0 && bytes[cursor - 1] == b'\\' {
                slashes += 1;
                cursor -= 1;
            }
            if slashes % 2 == 0 {
                return Some(index);
            }
        }
    }
    None
}

/// Whether an unescaped `"` follows `offset` (escape-aware): tells a
/// terminated lone surrogate (CPython accepts) from an unterminated
/// string (CPython reports it).
fn string_closed_after(text: &str, offset: usize) -> bool {
    let bytes = text.as_bytes();
    let mut index = offset.min(bytes.len());
    while index < bytes.len() {
        if bytes[index] == b'\\' {
            index += 2;
        } else if bytes[index] == b'"' {
            return true;
        } else {
            index += 1;
        }
    }
    false
}

/// `Unterminated string starting at` the opening quote — unless the tail
/// holds a truncated `\u` escape, which CPython reports instead. (Serde
/// misreports a bad `\u` escape in a CLOSED string as EOF-in-string —
/// `"\ud800\u12"` — so a quote at the very end is content-checked as a
/// closer first: with no bad escape it is a genuine opener, `"a""`.)
fn eof_string_detail(text: &str) -> Option<(&'static str, usize)> {
    let bytes = text.as_bytes();
    let open = json_opening_quote(text, bytes.len())?;
    if open + 1 == bytes.len() {
        if let Some(inner) = json_opening_quote(text, open) {
            if let Some(pos) = truncated_hex_escape(text, inner, open) {
                return Some(("Invalid \\uXXXX escape", pos));
            }
        }
        return Some(("Unterminated string starting at", open));
    }
    if let Some(pos) = truncated_hex_escape(text, open, bytes.len()) {
        return Some(("Invalid \\uXXXX escape", pos));
    }
    Some(("Unterminated string starting at", open))
}

/// Index of the `u` when the string content's last `\u` escape is
/// truncated: fewer than 4 hex digits — or a complete escape ending
/// exactly at `end` (the C scanner reads past it: `"\u0041` →
/// `Invalid \uXXXX escape`).
fn truncated_hex_escape(text: &str, open: usize, end: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    // Last unescaped `\u` in the string content.
    let mut last_u: Option<usize> = None;
    let mut index = open + 1;
    while index < end {
        if bytes[index] == b'\\' {
            if bytes.get(index + 1) == Some(&b'u') {
                last_u = Some(index + 1);
            }
            index += 2;
        } else {
            index += 1;
        }
    }
    let u = last_u?;
    let mut hex = 0;
    while hex < 4 && bytes.get(u + 1 + hex).is_some_and(u8::is_ascii_hexdigit) {
        hex += 1;
    }
    if hex < 4 || u + 5 == end {
        return Some(u);
    }
    None
}

/// JSON whitespace for the EOF region scan (`\x0c` is not JSON
/// whitespace — neither engine skips it).
fn is_json_ws(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r')
}

/// Whether the string ending at `end` (exclusive) is an object key: its
/// opening quote follows `{` or `,` (else it is a value). The scan
/// excludes the closing quote itself, which is the nearest quote back.
fn is_key_string(text: &str, end: usize) -> bool {
    let bytes = text.as_bytes();
    if end == 0 {
        return false;
    }
    let Some(open) = json_opening_quote(text, end - 1) else {
        return false;
    };
    let mut index = open;
    while index > 0 && is_json_ws(bytes[index - 1]) {
        index -= 1;
    }
    index > 0 && matches!(bytes[index - 1], b'{' | b',')
}

/// End-of-input failures: CPython reports what it wanted next (a value,
/// a key, a colon, a delimiter) at the token start or the end.
fn eof_detail(text: &str) -> (&'static str, usize) {
    let bytes = text.as_bytes();
    let end = bytes.len();
    let mut start = end;
    while start > 0 && (is_token_byte(bytes[start - 1]) || is_json_ws(bytes[start - 1])) {
        start -= 1;
    }
    let first = bytes[start..end]
        .iter()
        .position(|byte| !is_json_ws(*byte))
        .map_or(end, |offset| start + offset);
    if first == end {
        return eof_after_char(text, start, end);
    }
    // A token trails: the context before it decides.
    if start == 0 {
        return eof_value_token(text, first, end, true);
    }
    match bytes[start - 1] {
        b'[' | b':' => eof_value_token(text, first, end, false),
        b'{' => ("Expecting property name enclosed in double quotes", first),
        b',' => match innermost_bracket(text, start - 1) {
            Some(b'[') => eof_value_token(text, first, end, false),
            _ => ("Expecting property name enclosed in double quotes", first),
        },
        b'"' => {
            // A complete string precedes the token (an opening quote here
            // would be an EOF-string error instead): after a key CPython
            // wants the colon, after a value the delimiter.
            if is_key_string(text, start) {
                ("Expecting ':' delimiter", first)
            } else {
                ("Expecting ',' delimiter", first)
            }
        }
        _ => ("Expecting ',' delimiter", first),
    }
}

/// Nothing but whitespace trails: the last structural char (or the whole
/// input) decides what CPython wanted at the end.
fn eof_after_char(text: &str, start: usize, end: usize) -> (&'static str, usize) {
    if start == 0 {
        return ("Expecting value", end);
    }
    let bytes = text.as_bytes();
    match bytes[start - 1] {
        b'[' => ("Expecting value", end),
        b'{' => ("Expecting property name enclosed in double quotes", end),
        b',' => match innermost_bracket(text, start - 1) {
            Some(b'[') => ("Expecting value", end),
            _ => ("Expecting property name enclosed in double quotes", end),
        },
        b':' => {
            if colon_after_key(text, start - 1) {
                ("Expecting value", end)
            } else {
                // A stray colon after a value: CPython wants the
                // delimiter at the colon itself.
                ("Expecting ',' delimiter", start - 1)
            }
        }
        b'"' => {
            if is_key_string(text, start) {
                ("Expecting ':' delimiter", end)
            } else {
                ("Expecting ',' delimiter", end)
            }
        }
        _ => ("Expecting ',' delimiter", end),
    }
}

/// Whether the colon at `pos` follows an object key (else it is stray).
fn colon_after_key(text: &str, pos: usize) -> bool {
    let bytes = text.as_bytes();
    let mut index = pos;
    while index > 0 && is_json_ws(bytes[index - 1]) {
        index -= 1;
    }
    index > 0 && bytes[index - 1] == b'"' && is_key_string(text, index)
}

/// A value-position token at end of input: a valid number prefix plus
/// trailing garbage wants the delimiter at the prefix end (`Extra data`
/// at top level); a complete value wants the delimiter at the end; a
/// partial token wants a value at its start.
fn eof_value_token(text: &str, first: usize, end: usize, top: bool) -> (&'static str, usize) {
    let bytes = text.as_bytes();
    // Complete literals (`true`/`false`/`null`) behave like complete
    // numbers; a literal plus trailing garbage delimits after it.
    for literal in [b"true".as_slice(), b"false".as_slice(), b"null".as_slice()] {
        if bytes[first..end].starts_with(literal) {
            let after = first + literal.len();
            if bytes[after..end].iter().all(|b| is_json_ws(*b)) {
                return ("Expecting ',' delimiter", end);
            }
            if top {
                return ("Extra data", after);
            }
            return ("Expecting ',' delimiter", after);
        }
    }
    match number_prefix_end(bytes, first) {
        Some(prefix_end) if prefix_end > first => {
            if bytes[prefix_end..end].iter().all(|b| is_json_ws(*b)) {
                ("Expecting ',' delimiter", end)
            } else if top {
                ("Extra data", prefix_end)
            } else {
                ("Expecting ',' delimiter", prefix_end)
            }
        }
        _ => ("Expecting value", first),
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

/// Validated write fields for create/update, in `Meta.fields` order. `None`
/// means absent (PATCH leaves it); `Some(None)` means explicit null (PATCH
/// clears nullable fields).
#[derive(Debug, Clone, Default)]
pub struct CycleWrite {
    pub name: Option<String>,
    pub description: Option<String>,
    pub start_date: Option<Option<chrono::DateTime<chrono::Utc>>>,
    pub end_date: Option<Option<chrono::DateTime<chrono::Utc>>>,
    pub owned_by: Option<Option<uuid::Uuid>>,
    pub external_source: Option<Option<String>>,
    pub external_id: Option<Option<String>>,
    pub timezone: Option<String>,
}

/// DRF `DateTimeField` failure messages (DRF 3.15 `fields.py:1128-1133` +
/// `humanize_datetime.py:10`, oracle-pinned).
pub const DATETIME_INVALID_MESSAGE: &str = "Datetime has wrong format. Use one of these formats instead: YYYY-MM-DDThh:mm[:ss[.uuuuuu]][+HH:MM|-HH:MM|Z].";
pub const DATETIME_OVERFLOW_MESSAGE: &str = "Datetime value out of range.";
pub fn datetime_make_aware_message(timezone: &str) -> String {
    format!("Invalid datetime for the timezone \"{timezone}\".")
}

/// `TIMEZONE_CHOICES` (`db/models/cycle.py:78`, `pytz.common_timezones`, 433 entries):
/// exact-match membership for the `timezone` ChoiceField. Generated from the
/// oracle env pytz (no human transcription).
pub const PYTZ_COMMON_TIMEZONES: &[&str] = &[
    "Africa/Abidjan",
    "Africa/Accra",
    "Africa/Addis_Ababa",
    "Africa/Algiers",
    "Africa/Asmara",
    "Africa/Bamako",
    "Africa/Bangui",
    "Africa/Banjul",
    "Africa/Bissau",
    "Africa/Blantyre",
    "Africa/Brazzaville",
    "Africa/Bujumbura",
    "Africa/Cairo",
    "Africa/Casablanca",
    "Africa/Ceuta",
    "Africa/Conakry",
    "Africa/Dakar",
    "Africa/Dar_es_Salaam",
    "Africa/Djibouti",
    "Africa/Douala",
    "Africa/El_Aaiun",
    "Africa/Freetown",
    "Africa/Gaborone",
    "Africa/Harare",
    "Africa/Johannesburg",
    "Africa/Juba",
    "Africa/Kampala",
    "Africa/Khartoum",
    "Africa/Kigali",
    "Africa/Kinshasa",
    "Africa/Lagos",
    "Africa/Libreville",
    "Africa/Lome",
    "Africa/Luanda",
    "Africa/Lubumbashi",
    "Africa/Lusaka",
    "Africa/Malabo",
    "Africa/Maputo",
    "Africa/Maseru",
    "Africa/Mbabane",
    "Africa/Mogadishu",
    "Africa/Monrovia",
    "Africa/Nairobi",
    "Africa/Ndjamena",
    "Africa/Niamey",
    "Africa/Nouakchott",
    "Africa/Ouagadougou",
    "Africa/Porto-Novo",
    "Africa/Sao_Tome",
    "Africa/Tripoli",
    "Africa/Tunis",
    "Africa/Windhoek",
    "America/Adak",
    "America/Anchorage",
    "America/Anguilla",
    "America/Antigua",
    "America/Araguaina",
    "America/Argentina/Buenos_Aires",
    "America/Argentina/Catamarca",
    "America/Argentina/Cordoba",
    "America/Argentina/Jujuy",
    "America/Argentina/La_Rioja",
    "America/Argentina/Mendoza",
    "America/Argentina/Rio_Gallegos",
    "America/Argentina/Salta",
    "America/Argentina/San_Juan",
    "America/Argentina/San_Luis",
    "America/Argentina/Tucuman",
    "America/Argentina/Ushuaia",
    "America/Aruba",
    "America/Asuncion",
    "America/Atikokan",
    "America/Bahia",
    "America/Bahia_Banderas",
    "America/Barbados",
    "America/Belem",
    "America/Belize",
    "America/Blanc-Sablon",
    "America/Boa_Vista",
    "America/Bogota",
    "America/Boise",
    "America/Cambridge_Bay",
    "America/Campo_Grande",
    "America/Cancun",
    "America/Caracas",
    "America/Cayenne",
    "America/Cayman",
    "America/Chicago",
    "America/Chihuahua",
    "America/Ciudad_Juarez",
    "America/Costa_Rica",
    "America/Creston",
    "America/Cuiaba",
    "America/Curacao",
    "America/Danmarkshavn",
    "America/Dawson",
    "America/Dawson_Creek",
    "America/Denver",
    "America/Detroit",
    "America/Dominica",
    "America/Edmonton",
    "America/Eirunepe",
    "America/El_Salvador",
    "America/Fort_Nelson",
    "America/Fortaleza",
    "America/Glace_Bay",
    "America/Goose_Bay",
    "America/Grand_Turk",
    "America/Grenada",
    "America/Guadeloupe",
    "America/Guatemala",
    "America/Guayaquil",
    "America/Guyana",
    "America/Halifax",
    "America/Havana",
    "America/Hermosillo",
    "America/Indiana/Indianapolis",
    "America/Indiana/Knox",
    "America/Indiana/Marengo",
    "America/Indiana/Petersburg",
    "America/Indiana/Tell_City",
    "America/Indiana/Vevay",
    "America/Indiana/Vincennes",
    "America/Indiana/Winamac",
    "America/Inuvik",
    "America/Iqaluit",
    "America/Jamaica",
    "America/Juneau",
    "America/Kentucky/Louisville",
    "America/Kentucky/Monticello",
    "America/Kralendijk",
    "America/La_Paz",
    "America/Lima",
    "America/Los_Angeles",
    "America/Lower_Princes",
    "America/Maceio",
    "America/Managua",
    "America/Manaus",
    "America/Marigot",
    "America/Martinique",
    "America/Matamoros",
    "America/Mazatlan",
    "America/Menominee",
    "America/Merida",
    "America/Metlakatla",
    "America/Mexico_City",
    "America/Miquelon",
    "America/Moncton",
    "America/Monterrey",
    "America/Montevideo",
    "America/Montserrat",
    "America/Nassau",
    "America/New_York",
    "America/Nome",
    "America/Noronha",
    "America/North_Dakota/Beulah",
    "America/North_Dakota/Center",
    "America/North_Dakota/New_Salem",
    "America/Nuuk",
    "America/Ojinaga",
    "America/Panama",
    "America/Paramaribo",
    "America/Phoenix",
    "America/Port-au-Prince",
    "America/Port_of_Spain",
    "America/Porto_Velho",
    "America/Puerto_Rico",
    "America/Punta_Arenas",
    "America/Rankin_Inlet",
    "America/Recife",
    "America/Regina",
    "America/Resolute",
    "America/Rio_Branco",
    "America/Santarem",
    "America/Santiago",
    "America/Santo_Domingo",
    "America/Sao_Paulo",
    "America/Scoresbysund",
    "America/Sitka",
    "America/St_Barthelemy",
    "America/St_Johns",
    "America/St_Kitts",
    "America/St_Lucia",
    "America/St_Thomas",
    "America/St_Vincent",
    "America/Swift_Current",
    "America/Tegucigalpa",
    "America/Thule",
    "America/Tijuana",
    "America/Toronto",
    "America/Tortola",
    "America/Vancouver",
    "America/Whitehorse",
    "America/Winnipeg",
    "America/Yakutat",
    "Antarctica/Casey",
    "Antarctica/Davis",
    "Antarctica/DumontDUrville",
    "Antarctica/Macquarie",
    "Antarctica/Mawson",
    "Antarctica/McMurdo",
    "Antarctica/Palmer",
    "Antarctica/Rothera",
    "Antarctica/Syowa",
    "Antarctica/Troll",
    "Antarctica/Vostok",
    "Arctic/Longyearbyen",
    "Asia/Aden",
    "Asia/Almaty",
    "Asia/Amman",
    "Asia/Anadyr",
    "Asia/Aqtau",
    "Asia/Aqtobe",
    "Asia/Ashgabat",
    "Asia/Atyrau",
    "Asia/Baghdad",
    "Asia/Bahrain",
    "Asia/Baku",
    "Asia/Bangkok",
    "Asia/Barnaul",
    "Asia/Beirut",
    "Asia/Bishkek",
    "Asia/Brunei",
    "Asia/Chita",
    "Asia/Choibalsan",
    "Asia/Colombo",
    "Asia/Damascus",
    "Asia/Dhaka",
    "Asia/Dili",
    "Asia/Dubai",
    "Asia/Dushanbe",
    "Asia/Famagusta",
    "Asia/Gaza",
    "Asia/Hebron",
    "Asia/Ho_Chi_Minh",
    "Asia/Hong_Kong",
    "Asia/Hovd",
    "Asia/Irkutsk",
    "Asia/Jakarta",
    "Asia/Jayapura",
    "Asia/Jerusalem",
    "Asia/Kabul",
    "Asia/Kamchatka",
    "Asia/Karachi",
    "Asia/Kathmandu",
    "Asia/Khandyga",
    "Asia/Kolkata",
    "Asia/Krasnoyarsk",
    "Asia/Kuala_Lumpur",
    "Asia/Kuching",
    "Asia/Kuwait",
    "Asia/Macau",
    "Asia/Magadan",
    "Asia/Makassar",
    "Asia/Manila",
    "Asia/Muscat",
    "Asia/Nicosia",
    "Asia/Novokuznetsk",
    "Asia/Novosibirsk",
    "Asia/Omsk",
    "Asia/Oral",
    "Asia/Phnom_Penh",
    "Asia/Pontianak",
    "Asia/Pyongyang",
    "Asia/Qatar",
    "Asia/Qostanay",
    "Asia/Qyzylorda",
    "Asia/Riyadh",
    "Asia/Sakhalin",
    "Asia/Samarkand",
    "Asia/Seoul",
    "Asia/Shanghai",
    "Asia/Singapore",
    "Asia/Srednekolymsk",
    "Asia/Taipei",
    "Asia/Tashkent",
    "Asia/Tbilisi",
    "Asia/Tehran",
    "Asia/Thimphu",
    "Asia/Tokyo",
    "Asia/Tomsk",
    "Asia/Ulaanbaatar",
    "Asia/Urumqi",
    "Asia/Ust-Nera",
    "Asia/Vientiane",
    "Asia/Vladivostok",
    "Asia/Yakutsk",
    "Asia/Yangon",
    "Asia/Yekaterinburg",
    "Asia/Yerevan",
    "Atlantic/Azores",
    "Atlantic/Bermuda",
    "Atlantic/Canary",
    "Atlantic/Cape_Verde",
    "Atlantic/Faroe",
    "Atlantic/Madeira",
    "Atlantic/Reykjavik",
    "Atlantic/South_Georgia",
    "Atlantic/St_Helena",
    "Atlantic/Stanley",
    "Australia/Adelaide",
    "Australia/Brisbane",
    "Australia/Broken_Hill",
    "Australia/Darwin",
    "Australia/Eucla",
    "Australia/Hobart",
    "Australia/Lindeman",
    "Australia/Lord_Howe",
    "Australia/Melbourne",
    "Australia/Perth",
    "Australia/Sydney",
    "Canada/Atlantic",
    "Canada/Central",
    "Canada/Eastern",
    "Canada/Mountain",
    "Canada/Newfoundland",
    "Canada/Pacific",
    "Europe/Amsterdam",
    "Europe/Andorra",
    "Europe/Astrakhan",
    "Europe/Athens",
    "Europe/Belgrade",
    "Europe/Berlin",
    "Europe/Bratislava",
    "Europe/Brussels",
    "Europe/Bucharest",
    "Europe/Budapest",
    "Europe/Busingen",
    "Europe/Chisinau",
    "Europe/Copenhagen",
    "Europe/Dublin",
    "Europe/Gibraltar",
    "Europe/Guernsey",
    "Europe/Helsinki",
    "Europe/Isle_of_Man",
    "Europe/Istanbul",
    "Europe/Jersey",
    "Europe/Kaliningrad",
    "Europe/Kirov",
    "Europe/Kyiv",
    "Europe/Lisbon",
    "Europe/Ljubljana",
    "Europe/London",
    "Europe/Luxembourg",
    "Europe/Madrid",
    "Europe/Malta",
    "Europe/Mariehamn",
    "Europe/Minsk",
    "Europe/Monaco",
    "Europe/Moscow",
    "Europe/Oslo",
    "Europe/Paris",
    "Europe/Podgorica",
    "Europe/Prague",
    "Europe/Riga",
    "Europe/Rome",
    "Europe/Samara",
    "Europe/San_Marino",
    "Europe/Sarajevo",
    "Europe/Saratov",
    "Europe/Simferopol",
    "Europe/Skopje",
    "Europe/Sofia",
    "Europe/Stockholm",
    "Europe/Tallinn",
    "Europe/Tirane",
    "Europe/Ulyanovsk",
    "Europe/Vaduz",
    "Europe/Vatican",
    "Europe/Vienna",
    "Europe/Vilnius",
    "Europe/Volgograd",
    "Europe/Warsaw",
    "Europe/Zagreb",
    "Europe/Zurich",
    "GMT",
    "Indian/Antananarivo",
    "Indian/Chagos",
    "Indian/Christmas",
    "Indian/Cocos",
    "Indian/Comoro",
    "Indian/Kerguelen",
    "Indian/Mahe",
    "Indian/Maldives",
    "Indian/Mauritius",
    "Indian/Mayotte",
    "Indian/Reunion",
    "Pacific/Apia",
    "Pacific/Auckland",
    "Pacific/Bougainville",
    "Pacific/Chatham",
    "Pacific/Chuuk",
    "Pacific/Easter",
    "Pacific/Efate",
    "Pacific/Fakaofo",
    "Pacific/Fiji",
    "Pacific/Funafuti",
    "Pacific/Galapagos",
    "Pacific/Gambier",
    "Pacific/Guadalcanal",
    "Pacific/Guam",
    "Pacific/Honolulu",
    "Pacific/Kanton",
    "Pacific/Kiritimati",
    "Pacific/Kosrae",
    "Pacific/Kwajalein",
    "Pacific/Majuro",
    "Pacific/Marquesas",
    "Pacific/Midway",
    "Pacific/Nauru",
    "Pacific/Niue",
    "Pacific/Norfolk",
    "Pacific/Noumea",
    "Pacific/Pago_Pago",
    "Pacific/Palau",
    "Pacific/Pitcairn",
    "Pacific/Pohnpei",
    "Pacific/Port_Moresby",
    "Pacific/Rarotonga",
    "Pacific/Saipan",
    "Pacific/Tahiti",
    "Pacific/Tarawa",
    "Pacific/Tongatapu",
    "Pacific/Wake",
    "Pacific/Wallis",
    "US/Alaska",
    "US/Arizona",
    "US/Central",
    "US/Eastern",
    "US/Hawaii",
    "US/Mountain",
    "US/Pacific",
    "UTC",
];

/// One parsed datetime input: the wall time plus an optional fixed offset
/// in microseconds east of UTC (`None` = naive → the request user's zone).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParsedDatetime {
    pub naive: chrono::NaiveDateTime,
    pub offset_micros: Option<i64>,
}

/// `django.utils.dateparse.parse_datetime` on the oracle (Django 4.2,
/// Python 3.12, `dateparse.py:104-129`, probed): `fromisoformat` first,
/// the `datetime_re` fallback second. Both `None` and `ValueError` mean
/// "invalid" to DRF (suppressed at `fields.py:1179`), so one `Option`.
pub fn parse_iso_datetime(text: &str) -> Option<ParsedDatetime> {
    parse_fromisoformat(text).or_else(|| parse_django_datetime_re(text))
}

/// Python 3.12 `datetime.fromisoformat` (probed): extended/basic calendar
/// and ISO week dates, any single char separator (digits included:
/// `2026-10-01110:30:00` parses), `HH[MM[SS]]` times with `.`/`,`
/// fractions of any length (truncated to micros), `Z` or numeric offsets
/// (hours, hours+minutes, hours+minutes+seconds, each with an optional
/// fraction on the last unit). Years 1-9999, offsets strictly inside ±24h.
pub fn parse_fromisoformat(text: &str) -> Option<ParsedDatetime> {
    if text.is_empty() {
        return None;
    }
    let bytes = text.as_bytes();
    // Date prefix: `YYYY-MM-DD` | `YYYYMMDD` | `YYYY-Www[-D]` | `YYYYWww[D]`.
    // Only a `W` at 4/5 opens a week date; anywhere else it is an
    // ordinary char (`2026-10-01W10:30:00` parses with `W` as separator).
    if matches!(text.find('W'), Some(4) | Some(5)) {
        return parse_fromiso_week_datetime(text);
    }
    let (date, rest) = parse_fromiso_date(bytes)?;
    parse_fromiso_rest(date, rest, false).map(|(parsed, _)| parsed)
}

/// Everything after a `fromisoformat` date: bare (midnight naive) or one
/// separator char plus time plus the tz tail, plus the basic time's digit
/// count (`None` for extended times and bare dates). With `shape_only`
/// the components are grammar-checked but range checks (hour ≤ 23,
/// offset inside ±24h, ...) are skipped, so a well-formed but
/// out-of-range rest still parses; the basic week fallback uses that to
/// tell range failures (hard, unless the digit count is odd) from grammar
/// failures (fall back).
fn parse_fromiso_rest(
    date: chrono::NaiveDate,
    rest: &str,
    shape_only: bool,
) -> Option<(ParsedDatetime, Option<usize>)> {
    if rest.is_empty() {
        return Some((
            ParsedDatetime {
                naive: date.and_hms_opt(0, 0, 0)?,
                offset_micros: None,
            },
            None,
        ));
    }
    // Exactly one separator char, always (`20261001110:30:00` and
    // `2026-10-01110:30:00` both parse, probed).
    let sep_len = rest.chars().next()?.len_utf8();
    let (naive_time, rest, frac_len, basic_digits) =
        parse_fromiso_time(&rest[sep_len..], shape_only)?;
    let naive = date.and_time(naive_time);
    let offset_micros = parse_fromiso_tz_tail(rest, frac_len, shape_only)?;
    Some((
        ParsedDatetime {
            naive,
            offset_micros,
        },
        basic_digits,
    ))
}

/// The tz remainder after a `fromisoformat` time (probed): empty (naive),
/// `Z`, or a numeric offset — or skipped chars followed by `Z` or a
/// numeric offset. The allowance depends on the fraction that was just
/// consumed (`frac_len` digits, dotted or dotless, on any unit): none →
/// one skip (`T10:30:00X+05:30` parses; even a bare `.` skips:
/// `T10:30:00.+00:00` parses); 1-5 digits → no skip (`T10:30:00.5X+05:30`
/// fails); 6+ digits → unlimited skips (`T10:30:00.000000ABCDEFG+05:30`
/// parses — there is no second fraction, the middle is skipped). A
/// marker-first remainder never skips, so `Z+00:00` fails, and a `Z`
/// reached by skipping poisons the parse (`T10:30:00.000000XYZ+05:30`
/// fails: after `XY` comes `Z+05:30`); a skip with no tz after it fails
/// (`T1033404` fails); there is no skip inside the offset itself
/// (`+05X+06:00` fails). Up to `allowance` trailing NULs are tolerated
/// (`T10:30:00\0` parses naive, `T10:30:00\0\0` and `T10:30:00.5\0` fail,
/// `T10:30:00.000000\0\0` parses naive).
fn parse_fromiso_tz_tail(rest: &str, frac_len: usize, shape_only: bool) -> Option<Option<i64>> {
    let allowance = tz_skip_allowance(frac_len);
    let mut tail = rest;
    let mut stripped: u32 = 0;
    while allowance.is_none_or(|max| stripped < max) {
        match tail.strip_suffix('\0') {
            Some(shorter) => {
                tail = shorter;
                stripped += 1;
            }
            None => break,
        }
    }
    match tail {
        "" => Some(None),
        "Z" => Some(Some(0)),
        _ => {
            let mut rest = tail;
            let mut skips: u32 = 0;
            loop {
                if rest == "Z" {
                    return Some(Some(0));
                }
                let first = rest.as_bytes().first()?;
                if *first == b'+' || *first == b'-' {
                    return Some(Some(parse_fromiso_offset(rest, shape_only)?));
                }
                if *first == b'Z' || !allowance.is_none_or(|max| skips < max) {
                    return None;
                }
                let skip = rest.chars().next()?.len_utf8();
                rest = &rest[skip..];
                skips += 1;
                if rest.is_empty() {
                    return None;
                }
            }
        }
    }
}

/// Skip allowance from the consumed fraction's digit count (probed):
/// `None` means unlimited.
fn tz_skip_allowance(frac_len: usize) -> Option<u32> {
    if frac_len == 0 {
        Some(1)
    } else if frac_len < 6 {
        Some(0)
    } else {
        None
    }
}

/// The `fromisoformat` calendar date prefix plus the unparsed remainder
/// (week dates, which contain `W`, are handled before this is reached).
fn parse_fromiso_date(bytes: &[u8]) -> Option<(chrono::NaiveDate, &str)> {
    let text = std::str::from_utf8(bytes).ok()?;
    if bytes.len() >= 10 && bytes[4] == b'-' && bytes[7] == b'-' {
        let year = digits_to_i32(&bytes[0..4])?;
        if !(1..=9999).contains(&year) {
            return None;
        }
        let month = digits_to_u32(&bytes[5..7])?;
        let day = digits_to_u32(&bytes[8..10])?;
        let date = chrono::NaiveDate::from_ymd_opt(year, month, day)?;
        return Some((date, &text[10..]));
    }
    if bytes.len() >= 8 && bytes[0..8].iter().all(|b| b.is_ascii_digit()) {
        let year = digits_to_i32(&bytes[0..4])?;
        if !(1..=9999).contains(&year) {
            return None;
        }
        let month = digits_to_u32(&bytes[4..6])?;
        let day = digits_to_u32(&bytes[6..8])?;
        let date = chrono::NaiveDate::from_ymd_opt(year, month, day)?;
        return Some((date, &text[8..]));
    }
    None
}

/// A `fromisoformat` ISO week datetime. The weekday takes the date's
/// own shape only — `-D` after `YYYY-Www`, a trailing `D` after `YYYYWww`
/// — and it backtracks: the with-weekday parse is tried first, then the
/// bare-week (Monday) parse (`2026W404110:30:00` is Thursday, `2026W40610:30`
/// is Monday, `2026-W40-11:30:00` is Monday, all probed). The basic `D`
/// wins outright only with an even basic digit count (or an extended
/// time). On an odd count Monday wins when it parses (`2026W404103340Z`
/// is Monday), a Monday grammar failure keeps a valid with-day parse
/// (`2026W404Z103+05.5` is Thursday), and a Monday range failure fails
/// hard. A with-day grammar failure falls back to Monday-or-None, and a
/// with-day range failure does the same on odd counts but fails hard on
/// even counts (`2026W401030:00` fails even though Monday 03:00 would
/// parse). The extended `-D` additionally needs
/// end-or-nondigit after D (`2026-W40-4110:30:00` fails the with-day
/// attempt and falls back to Monday, probed). With no dash the bare week
/// takes any separator, digits included (`2026-W40610:30:00` is Monday,
/// probed).
fn parse_fromiso_week_datetime(text: &str) -> Option<ParsedDatetime> {
    let bytes = text.as_bytes();
    let wpos = text.find('W')?;
    if wpos != 4 && wpos != 5 {
        return None;
    }
    let extended = wpos == 5;
    if extended && bytes[4] != b'-' {
        return None;
    }
    let year = digits_to_i32(&bytes[0..4])?;
    if !(1..=9999).contains(&year) {
        return None;
    }
    let rest = &text[wpos + 1..];
    if rest.len() < 2 || !rest.as_bytes()[0..2].iter().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let week: u32 = rest[0..2].parse().ok()?;
    let tail = &rest[2..];
    let monday = chrono::NaiveDate::from_isoywd_opt(year, week, chrono::Weekday::Mon)?;
    if extended {
        match tail.strip_prefix('-') {
            Some(day) => {
                // Try `-D` (D must be followed by end or a non-digit),
                // fall back to Monday with the dash as separator.
                let day_end = day.len() <= 1 || !day.as_bytes()[1].is_ascii_digit();
                if day_end {
                    if let Some((parsed, _)) = weekday_try_backtrack(year, week, day, false) {
                        return Some(parsed);
                    }
                }
                parse_fromiso_rest(monday, tail, false).map(|(parsed, _)| parsed)
            }
            None => parse_fromiso_rest(monday, tail, false).map(|(parsed, _)| parsed),
        }
    } else {
        // Try `D`: it wins outright only with an even basic digit count
        // (or an extended time). On an odd count Monday wins when it
        // parses, a Monday grammar failure keeps a valid with-day parse,
        // and a Monday range failure fails hard. A with-day grammar
        // failure falls back to Monday-or-None, and a with-day range
        // failure does the same on odd counts but fails hard on even
        // counts — so check the shape when the full parse fails, and the
        // parity in both cases.
        let with_full = weekday_try_backtrack(year, week, tail, false);
        let with_odd = match &with_full {
            Some((_, basic_digits)) => basic_digits.is_some_and(|count| count % 2 == 1),
            None => match weekday_try_backtrack(year, week, tail, true) {
                None => {
                    return parse_fromiso_rest(monday, tail, false).map(|(parsed, _)| parsed);
                }
                Some((_, basic_digits)) => basic_digits.is_some_and(|count| count % 2 == 1),
            },
        };
        if !with_odd {
            return with_full.map(|(parsed, _)| parsed);
        }
        let without_full = parse_fromiso_rest(monday, tail, false);
        if without_full.is_some() {
            return without_full.map(|(parsed, _)| parsed);
        }
        if parse_fromiso_rest(monday, tail, true).is_none() {
            return with_full.map(|(parsed, _)| parsed);
        }
        None
    }
}

/// The with-weekday attempt: a leading digit 1-7 names the day, and the
/// whole rest (separator, time, tz) must parse after it.
fn weekday_try_backtrack(
    year: i32,
    week: u32,
    text: &str,
    shape_only: bool,
) -> Option<(ParsedDatetime, Option<usize>)> {
    let digit = *text.as_bytes().first()?;
    if !(b'1'..=b'7').contains(&digit) {
        return None;
    }
    let weekday = chrono::Weekday::try_from(digit - b'0' - 1).ok()?;
    let date = chrono::NaiveDate::from_isoywd_opt(year, week, weekday)?;
    parse_fromiso_rest(date, &text[1..], shape_only)
}

/// The `fromisoformat` time plus the tz remainder plus the consumed
/// fraction's digit count (which drives the tz skip allowance):
/// `HH[[:]MM[[:]SS]][.,frac]` (a fraction after any unit means sub-second
/// micros, probed: `T10:30.5` is 10:30:00.5), plus a basic-only dotless
/// fraction: 2+ digits glued after SS (`T103340040` is .040s; a single
/// glued digit ends the time instead, probed: `T1033404` fails). Mixed
/// basic/extended forms never parse (`T10:3000`, `T1030:00` fail), and a
/// colon is never backtracked (`T10:30:0+00:00` fails). With
/// `shape_only` the units are grammar-checked (two digits each) but the
/// clock ranges are not enforced (midnight stands in for the time). The
/// digit count covers basic unit digits plus the dotless run (including a
/// lone run-1 digit that stays in the tz tail) plus one lone tail digit
/// after a short basic time; dotted fractions never count, and extended
/// times report `None`.
fn parse_fromiso_time(
    text: &str,
    shape_only: bool,
) -> Option<(chrono::NaiveTime, &str, usize, Option<usize>)> {
    let bytes = text.as_bytes();
    if bytes.len() < 2 || !bytes[0..2].iter().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let hour: u32 = text[0..2].parse().ok()?;
    let mut rest = &text[2..];
    let mut minute: u32 = 0;
    let mut second: u32 = 0;
    let mut digits: Option<usize> = Some(2);
    // Optional `:MM` / `MM`.
    if let Some(tail) = rest.strip_prefix(':') {
        digits = None;
        if tail.len() < 2 || !tail.as_bytes()[0..2].iter().all(|b| b.is_ascii_digit()) {
            return None;
        }
        minute = tail[0..2].parse().ok()?;
        rest = &tail[2..];
        // Optional `:SS`.
        if let Some(tail) = rest.strip_prefix(':') {
            if tail.len() < 2 || !tail.as_bytes()[0..2].iter().all(|b| b.is_ascii_digit()) {
                return None;
            }
            second = tail[0..2].parse().ok()?;
            rest = &tail[2..];
        }
    } else if rest.len() >= 2 && rest.as_bytes()[0..2].iter().all(|b| b.is_ascii_digit()) {
        digits = Some(4);
        minute = rest[0..2].parse().ok()?;
        rest = &rest[2..];
        if rest.len() >= 2 && rest.as_bytes()[0..2].iter().all(|b| b.is_ascii_digit()) {
            digits = Some(6);
            second = rest[0..2].parse().ok()?;
            rest = &rest[2..];
            // Basic dotless fraction: a run of 2+ digits.
            let run: usize = rest.bytes().take_while(|b| b.is_ascii_digit()).count();
            if run >= 2 {
                let mut micros: i64 = 0;
                for (index, byte) in rest.bytes().take(6).enumerate() {
                    if !byte.is_ascii_digit() {
                        break;
                    }
                    micros += (byte - b'0') as i64 * 10_i64.pow(5 - index as u32);
                }
                let time = time_or_shape_midnight(hour, minute, second, micros, shape_only)?;
                return Some((time, &rest[run..], run, Some(6 + run)));
            }
        }
    }
    // A lone digit left in the tz tail still counts (a run-1 after SS or
    // after a short basic time).
    if digits.is_some() && rest.as_bytes().first().is_some_and(|b| b.is_ascii_digit()) {
        digits = digits.map(|count| count + 1);
    }
    let (micros, frac_len, rest) = parse_frac_micros_opt(rest)?;
    let time = time_or_shape_midnight(hour, minute, second, micros, shape_only)?;
    Some((time, rest, frac_len, digits))
}

/// Build the clock time, or midnight when only the shape is wanted.
fn time_or_shape_midnight(
    hour: u32,
    minute: u32,
    second: u32,
    micros: i64,
    shape_only: bool,
) -> Option<chrono::NaiveTime> {
    if shape_only {
        return chrono::NaiveTime::from_hms_opt(0, 0, 0);
    }
    chrono::NaiveTime::from_hms_micro_opt(hour, minute, second, micros as u32)
}

/// The optional dotted fraction after a time component: `.`/`,` plus ≥1
/// digit (any length, truncated to micros), returning micros, digit count
/// and remainder. No dot — or a dot with no digits — means `(0, 0, rest)`
/// unconsumed: a bare dot ends the time and the tz tail skips it
/// (`T10:30:00.+00:00` parses, probed).
fn parse_frac_micros_opt(text: &str) -> Option<(i64, usize, &str)> {
    let tail = match text.strip_prefix('.').or_else(|| text.strip_prefix(',')) {
        Some(tail) => tail,
        None => return Some((0, 0, text)),
    };
    let digits: usize = tail.bytes().take_while(|b| b.is_ascii_digit()).count();
    if digits == 0 {
        return Some((0, 0, text));
    }
    let mut micros: i64 = 0;
    for (index, byte) in tail.bytes().take(6).enumerate() {
        if !byte.is_ascii_digit() {
            break;
        }
        micros += (byte - b'0') as i64 * 10_i64.pow(5 - index as u32);
    }
    Some((micros, digits, &tail[digits..]))
}

/// The `fromisoformat` numeric offset in micros east of UTC:
/// `±HH[MM[SS]]` basic or `±HH:MM[:SS]` extended (the mode locks on the
/// first separator: `+0530:15` and `+05:3015` fail, probed), with an
/// optional `.`/`,` fraction after any unit (always sub-second micros,
/// probed: `+05.5` is 5h + 0.5s) — except a zero `HH:MM:SS` drops the
/// fraction entirely (`+00:00:00.5` is UTC, probed). Components are never
/// range-checked (`+05:99` normalises to +6:39); only the total must sit
/// strictly inside ±24h (skipped with `shape_only`).
fn parse_fromiso_offset(text: &str, shape_only: bool) -> Option<i64> {
    let bytes = text.as_bytes();
    let sign: i64 = match bytes.first()? {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    let rest = &text[1..];
    if rest.len() < 2 || !rest.as_bytes()[0..2].iter().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let hours: i64 = rest[0..2].parse().ok()?;
    let mut rest = &rest[2..];
    if rest.starts_with('.') || rest.starts_with(',') {
        let (frac, _, tail) = parse_frac_micros_opt(rest)?;
        return finish_fromiso_offset(sign, hours, 0, 0, frac, tail, shape_only);
    }
    let mut minutes: i64 = 0;
    let mut seconds: i64 = 0;
    if rest.starts_with(':') {
        // Extended: `HH:MM[:SS]`.
        rest = &rest[1..];
        if rest.len() < 2 || !rest.as_bytes()[0..2].iter().all(|b| b.is_ascii_digit()) {
            return None;
        }
        minutes = rest[0..2].parse().ok()?;
        rest = &rest[2..];
        if rest.starts_with('.') || rest.starts_with(',') {
            let (frac, _, tail) = parse_frac_micros_opt(rest)?;
            return finish_fromiso_offset(sign, hours, minutes, 0, frac, tail, shape_only);
        }
        if let Some(tail) = rest.strip_prefix(':') {
            if tail.len() < 2 || !tail.as_bytes()[0..2].iter().all(|b| b.is_ascii_digit()) {
                return None;
            }
            seconds = tail[0..2].parse().ok()?;
            rest = &tail[2..];
        }
    } else {
        // Basic: `HH[MM[SS]]`.
        if rest.len() >= 2 && rest.as_bytes()[0..2].iter().all(|b| b.is_ascii_digit()) {
            minutes = rest[0..2].parse().ok()?;
            rest = &rest[2..];
            if rest.starts_with('.') || rest.starts_with(',') {
                let (frac, _, tail) = parse_frac_micros_opt(rest)?;
                return finish_fromiso_offset(sign, hours, minutes, 0, frac, tail, shape_only);
            }
            if rest.len() >= 2 && rest.as_bytes()[0..2].iter().all(|b| b.is_ascii_digit()) {
                seconds = rest[0..2].parse().ok()?;
                rest = &rest[2..];
            }
        }
    }
    let (frac, _, tail) = parse_frac_micros_opt(rest)?;
    finish_fromiso_offset(sign, hours, minutes, seconds, frac, tail, shape_only)
}

/// Total a `fromisoformat` offset: the sign applies to the whole-unit part
/// and the fraction alike; a zero whole part drops the fraction; the total
/// must sit strictly inside ±24h; nothing may remain.
fn finish_fromiso_offset(
    sign: i64,
    hours: i64,
    minutes: i64,
    seconds: i64,
    frac_micros: i64,
    rest: &str,
    shape_only: bool,
) -> Option<i64> {
    if !rest.is_empty() {
        return None;
    }
    if hours == 0 && minutes == 0 && seconds == 0 {
        return Some(0);
    }
    let micros = sign * (hours * 3_600_000_000 + minutes * 60_000_000 + seconds * 1_000_000)
        + sign * frac_micros;
    if !shape_only && micros.abs() >= 24 * 3_600_000_000 {
        return None;
    }
    Some(micros)
}

/// Django's `datetime_re` fallback (`dateparse.py`, exact):
/// padded-or-not `YYYY-M-D`, `T`/space, `H:M[:S[.,frac]]]`, optional
/// whitespace, optional `Z|±HH[[:]MM]`. The frac is `\d{1,6}\d{0,6}`
/// (micros come from the first 6, left-justified). Offsets are whole minutes
/// strictly inside ±24h (`timezone()` raises past that).
pub fn parse_django_datetime_re(text: &str) -> Option<ParsedDatetime> {
    let bytes = text.as_bytes();
    if bytes.len() < 5 {
        return None;
    }
    if !bytes[0..4].iter().all(|b| b.is_ascii_digit()) || bytes[4] != b'-' {
        return None;
    }
    let year = digits_to_i32(&bytes[0..4])?;
    // Django builds `datetime(...)` from the groups, which raises for year
    // 0 (and DRF turns the raise into invalid, probed).
    if !(1..=9999).contains(&year) {
        return None;
    }
    let mut rest = &text[5..];
    let (month, tail) = take_1_or_2_digits(rest)?;
    rest = tail.strip_prefix('-')?;
    let (day, tail) = take_1_or_2_digits(rest)?;
    rest = tail;
    let sep = rest.as_bytes().first()?;
    if *sep != b'T' && *sep != b' ' {
        return None;
    }
    rest = &rest[1..];
    let (hour, tail) = take_1_or_2_digits(rest)?;
    rest = tail.strip_prefix(':')?;
    let (minute, tail) = take_1_or_2_digits(rest)?;
    rest = tail;
    let mut second: u32 = 0;
    let mut micros: i64 = 0;
    if let Some(tail) = rest.strip_prefix(':') {
        let (value, tail) = take_1_or_2_digits(tail)?;
        second = value;
        rest = tail;
        if let Some(tail) = rest.strip_prefix('.').or_else(|| rest.strip_prefix(',')) {
            let digits: usize = tail.bytes().take_while(|b| b.is_ascii_digit()).count();
            if digits == 0 || digits > 12 {
                return None;
            }
            let (frac, _, tail) = parse_frac_micros_opt(rest)?;
            micros = frac;
            rest = tail;
        }
    }
    // `\s*` then the optional tzinfo, then end.
    let trimmed = rest.trim_start_matches(|c: char| c.is_whitespace());
    let offset_micros = match trimmed {
        "" => None,
        "Z" => Some(0),
        _ => Some(parse_django_offset(trimmed)?),
    };
    let date = chrono::NaiveDate::from_ymd_opt(year, month, day)?;
    let time = chrono::NaiveTime::from_hms_micro_opt(hour, minute, second, micros as u32)?;
    let naive = date.and_time(time);
    Some(ParsedDatetime {
        naive,
        offset_micros,
    })
}

/// Django's tzinfo arm: `±HH[[:]MM]` whole minutes, strictly inside ±24h.
fn parse_django_offset(text: &str) -> Option<i64> {
    let bytes = text.as_bytes();
    let sign: i64 = match bytes.first()? {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    let rest = &text[1..];
    if rest.len() < 2 || !rest.as_bytes()[0..2].iter().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let hours: i64 = rest[0..2].parse().ok()?;
    let rest = &rest[2..];
    let (colon, rest) = match rest.strip_prefix(':') {
        Some(tail) => (true, tail),
        None => (false, rest),
    };
    let minutes: i64 = if rest.is_empty() {
        if colon {
            return None;
        }
        0
    } else {
        if rest.len() != 2 || !rest.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        rest.parse().ok()?
    };
    let micros = (hours * 3_600 + minutes * 60) * 1_000_000;
    if micros.abs() >= 24 * 3_600_000_000 {
        return None;
    }
    Some(sign * micros)
}

/// 1-2 ASCII digits plus the remainder (`\d{1,2}`).
fn take_1_or_2_digits(text: &str) -> Option<(u32, &str)> {
    let bytes = text.as_bytes();
    let first = *bytes.first()?;
    if !first.is_ascii_digit() {
        return None;
    }
    let mut value: u32 = (first - b'0') as u32;
    let mut rest = &text[1..];
    if let Some(second) = rest.as_bytes().first() {
        if second.is_ascii_digit() {
            value = value * 10 + (second - b'0') as u32;
            rest = &rest[1..];
        }
    }
    Some((value, rest))
}

fn digits_to_i32(digits: &[u8]) -> Option<i32> {
    if digits.is_empty() || !digits.iter().all(|b| b.is_ascii_digit()) {
        return None;
    }
    std::str::from_utf8(digits).ok()?.parse().ok()
}

fn digits_to_u32(digits: &[u8]) -> Option<u32> {
    if digits.is_empty() || !digits.iter().all(|b| b.is_ascii_digit()) {
        return None;
    }
    std::str::from_utf8(digits).ok()?.parse().ok()
}

/// DRF `DateTimeField.enforce_timezone` failure: an aware input whose
/// user-zone shift leaves years 1-9999, or (unreachable in practice) a
/// gap-probe underflow. Naive fold-hour inputs PASS at fold 0 — see
/// [`enforce_user_timezone`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnforceFail {
    Overflow,
    Ambiguous,
}

/// `DateTimeField.enforce_timezone` (DRF 3.15 `fields.py:1145-1171`): the
/// serializer's `__init__` timezone override never fires on these views
/// (no view passes `project` in context), so the field zone is always
/// `default_timezone()` = the active request zone = the actor's zone
/// (`TimezoneMixin`). Naive inputs attach there (`make_aware` +
/// `valid_datetime`, which ACCEPTS ambiguous folds at fold 0 and
/// imaginary gap times at the pre-transition offset); aware inputs
/// shift there (`astimezone`, whose `OverflowError` is the range check).
pub fn enforce_user_timezone(
    parsed: &ParsedDatetime,
    user_tz: &Tz,
) -> Result<chrono::DateTime<chrono::Utc>, EnforceFail> {
    match parsed.offset_micros {
        Some(offset) => {
            let naive = parsed
                .naive
                .checked_sub_signed(chrono::Duration::microseconds(offset))
                .ok_or(EnforceFail::Overflow)?;
            let local = user_tz.from_utc_datetime(&naive);
            if local.year() < 1 || local.year() > 9999 {
                return Err(EnforceFail::Overflow);
            }
            Ok(chrono::DateTime::from_naive_utc_and_offset(
                naive,
                chrono::Utc,
            ))
        }
        None => match user_tz.from_local_datetime(&parsed.naive) {
            LocalResult::Single(local) => Ok(local.with_timezone(&chrono::Utc)),
            // Fold-hour walls PASS at fold 0 (the pre-transition offset):
            // DRF's `valid_datetime` accepts them because CPython's
            // `datetime_exists` (`dt.astimezone(utc) == dt`) is False for
            // fold hours, so `datetime_ambiguous` is False and the value
            // passes (`rest_framework/utils/timezone.py`; verified live:
            // Django 201s `2026-11-01T01:30:00` in America/New_York).
            // Chrono's `early` is the first occurrence = Python fold 0.
            LocalResult::Ambiguous(early, _) => Ok(early.with_timezone(&chrono::Utc)),
            LocalResult::None => {
                // Imaginary gap time: Python's `replace(tzinfo=zone)`
                // keeps fold 0 (the pre-transition offset) and
                // `valid_datetime` accepts it.
                match pre_transition_offset(user_tz, &parsed.naive) {
                    Some(offset_secs) => shift_wall_to_utc(&parsed.naive, offset_secs),
                    None => Err(EnforceFail::Ambiguous),
                }
            }
        },
    }
}

/// The offset a gap wall takes: the pre-transition side. Walk back for
/// the nearest valid wall (real gaps run ≤ 24h; 72h is plenty) and take
/// its earliest offset.
fn pre_transition_offset(tz: &Tz, naive: &chrono::NaiveDateTime) -> Option<i64> {
    let mut probe = *naive;
    for _ in 0..144 {
        probe = probe.checked_sub_signed(chrono::Duration::minutes(30))?;
        match tz.from_local_datetime(&probe) {
            LocalResult::Single(local) => {
                return Some((local.naive_local() - local.naive_utc()).num_seconds());
            }
            LocalResult::Ambiguous(early, _) => {
                return Some((early.naive_local() - early.naive_utc()).num_seconds());
            }
            LocalResult::None => {}
        }
    }
    None
}

/// Shift a wall time into UTC by a whole-second zone offset (the fold-0
/// probe path of [`enforce_user_timezone`]).
fn shift_wall_to_utc(
    naive: &chrono::NaiveDateTime,
    offset_secs: i64,
) -> Result<chrono::DateTime<chrono::Utc>, EnforceFail> {
    naive
        .checked_sub_signed(chrono::Duration::seconds(offset_secs))
        .map(|shifted| chrono::DateTime::from_naive_utc_and_offset(shifted, chrono::Utc))
        .ok_or(EnforceFail::Ambiguous)
}

/// Coerce the create/update body field by field (DRF field order =
/// `Meta.fields` order; errors keyed the same way). `partial` selects PATCH
/// semantics (every field optional). Datetimes parse through
/// [`parse_iso_datetime`] and enforce into the actor's zone; unknown body
/// keys are ignored exactly like DRF.
pub async fn coerce_write(
    pool: &PgPool,
    body: &serde_json::Map<String, Value>,
    partial: bool,
    user_timezone: &Tz,
) -> Result<CycleWrite, Denial> {
    let mut errors: Vec<(String, String)> = Vec::new();
    let mut write = CycleWrite::default();

    // name: CharField(max 255, blank=False, null=False); required unless partial.
    match body.get("name") {
        None if partial => {}
        value => match coerce_char(value, false, false, Some(255)) {
            Ok(Some(name)) => write.name = Some(name),
            Ok(None) => {}
            Err(fail) => errors.push(("name".to_owned(), fail.body)),
        },
    }
    // description: TextField(blank=True, null=False); missing absent.
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
    // start_date / end_date: DateTimeField(blank=True, null=True); missing
    // absent, explicit null clears, strings parse + enforce into the
    // actor's zone (every other JSON type is the invalid message —
    // `to_internal_value` only accepts strings).
    for key in ["start_date", "end_date"] {
        match body.get(key) {
            None => {}
            Some(Value::Null) => {
                if key == "start_date" {
                    write.start_date = Some(None);
                } else {
                    write.end_date = Some(None);
                }
            }
            Some(Value::String(text)) => match parse_iso_datetime(text) {
                Some(parsed) => match enforce_user_timezone(&parsed, user_timezone) {
                    Ok(instant) => {
                        if key == "start_date" {
                            write.start_date = Some(Some(instant));
                        } else {
                            write.end_date = Some(Some(instant));
                        }
                    }
                    Err(EnforceFail::Overflow) => errors.push((
                        key.to_owned(),
                        format!("[{}]", json_string(DATETIME_OVERFLOW_MESSAGE)),
                    )),
                    Err(EnforceFail::Ambiguous) => errors.push((
                        key.to_owned(),
                        format!(
                            "[{}]",
                            json_string(&datetime_make_aware_message(user_timezone.name()))
                        ),
                    )),
                },
                None => errors.push((
                    key.to_owned(),
                    format!("[{}]", json_string(DATETIME_INVALID_MESSAGE)),
                )),
            },
            Some(_) => errors.push((
                key.to_owned(),
                format!("[{}]", json_string(DATETIME_INVALID_MESSAGE)),
            )),
        }
    }
    // owned_by: user PK (required=False, allow_null=True); missing absent;
    // "" -> None (then defaulted to the requester by `validate()`).
    match body.get("owned_by") {
        None => {}
        Some(value) => match coerce_pk_value(value, true) {
            Ok(PkValue::Null) => write.owned_by = Some(None),
            Ok(PkValue::Id(id)) => {
                let echo = match value {
                    Value::Number(_) => py_repr(value),
                    Value::String(s) => s.clone(),
                    _ => py_repr(value),
                };
                match check_user_exists(pool, &id, &echo, "owned-by-exists").await {
                    Ok(()) => write.owned_by = Some(Some(id)),
                    Err(fail) => {
                        if fail.body.is_empty() {
                            return Err(Denial::ServerError);
                        }
                        errors.push(("owned_by".to_owned(), fail.body));
                    }
                }
            }
            Err(fail) => errors.push(("owned_by".to_owned(), fail.body)),
        },
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
    // timezone: ChoiceField(pytz.common_timezones, required=False via the
    // "UTC" model default, allow_null=False). Missing stays absent (the
    // caller applies the default on create); explicit null fails.
    match body.get("timezone") {
        None => {}
        value => match coerce_choice(value, PYTZ_COMMON_TIMEZONES) {
            Ok(Some(timezone)) => write.timezone = Some(timezone),
            Ok(None) => {}
            Err(fail) => errors.push(("timezone".to_owned(), fail.body)),
        },
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

/// Resolve the cycle-issues GET `?order_by=` (default `created_at`,
/// ascending) against what Django's `.order_by()` accepts at
/// `views/cycle.py:879` (PIDASHCONV-522; every arm verified against live
/// `str(query)` output — the PIDASHCONV-510/511 module precedent, plus
/// the traversal-with-join arms the module port left residual): the
/// exact `?` (random — `-?` is a `FieldError`, never random), the two
/// annotations that exist at order time (`sub_issues_count`, `bridge_id`
/// — `link_count`/`attachment_count` are annotated after and `FieldError`),
/// bare-FK names by the related model's `Meta.ordering` (`type` excepted:
/// `IssueType` has no ordering, so Django orders it by the local
/// `type_id`), bare-M2M names (`assignees`, `labels`), single-level
/// traversals onto the already-joined tables, and single-level
/// traversals needing a fresh join (`created_by__<col>` et al.,
/// `assignees__<col>`). Anything else runs quoted onto its table and
/// fails at the database, exactly like Django's `FieldError`-at-evaluation
/// → generic 500 — except reverse relations other than `issue_cycle`
/// (`parent_issue`, `issue_comments`, ...), which Django 200s via
/// their related ordering (verified `str(query)`): recorded residual,
/// out of scope here (neither listed arms nor the 510/511 shape).
pub fn resolve_cycle_issue_order(
    raw: Option<&str>,
) -> pidash_db::v1_cycles_modules::cycle_queries::OrderBy {
    use pidash_db::v1_cycles_modules::cycle_queries::{
        issue_join_traversal, issue_m2m_order, issue_traversal_table, OrderBy, RelatedOrder,
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
    // Bare FKs (and the reverse FK `issue_cycle`) order by the
    // related `Meta.ordering` (PIDASHCONV-522).
    if let Some(which) = match column {
        "state" => Some(RelatedOrder::State),
        "project" => Some(RelatedOrder::Project),
        "workspace" => Some(RelatedOrder::Workspace),
        "parent" => Some(RelatedOrder::Parent),
        "created_by" => Some(RelatedOrder::CreatedBy),
        "updated_by" => Some(RelatedOrder::UpdatedBy),
        "estimate_point" => Some(RelatedOrder::EstimatePoint),
        "assigned_pod" => Some(RelatedOrder::AssignedPod),
        "issue_cycle" => Some(RelatedOrder::IssueCycle),
        _ => None,
    } {
        return OrderBy::related(which, descending);
    }
    if let Some(which) = issue_m2m_order(column) {
        return OrderBy::m2m(which, descending);
    }
    if let Some((head, tail)) = column.split_once("__") {
        if let Some(table) = issue_traversal_table(head) {
            return OrderBy::table(table, tail, descending);
        }
        if let Some(which) = issue_join_traversal(head) {
            return OrderBy::traversal(which, tail, descending);
        }
        if let Some(which) = issue_m2m_order(head) {
            return OrderBy::m2m_traversal(which, tail, descending);
        }
    }
    OrderBy::new(column, descending)
}

// NOTE (PIDASHCONV-522 rebase): `order_term` (with the F-N1 quoter) is
// gone with the splice it served — sea-query identifiers double `"`.
/// Envelope totals for the cycle-issues page: `rows.len()` — except
/// the M2M orderings multiply rows per through-row while Django's
/// `queryset.count()` trims the ordering-only joins, so M2M totals
/// count distinct issue ids (PIDASHCONV-522; live bridges are unique
/// per issue+cycle, so only ordering joins can multiply here). The
/// PIDASHCONV-522 related-ordering and FK-traversal joins are to-one
/// `LEFT JOIN`s, so they never multiply and keep `rows.len()`. The page
/// window itself still slices the multiplied rows, exactly like Django's
/// `queryset[offset:stop]`.
fn envelope_total(
    order: &pidash_db::v1_cycles_modules::cycle_queries::OrderBy,
    rows: &[sqlx::postgres::PgRow],
) -> Result<usize, Denial> {
    use pidash_db::v1_cycles_modules::cycle_queries::OrderTarget;
    if !matches!(
        order.target,
        OrderTarget::M2M(_) | OrderTarget::M2MTraversal(_)
    ) {
        return Ok(rows.len());
    }
    let mut ids = Vec::with_capacity(rows.len());
    for row in rows {
        ids.push(row_uuid(row, "id", "cycle-issues-total")?);
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

/// The `Project` columns the cycle views touch: `cycle_view` gates
/// `validate()` (`serializers/cycle.py:83-84`), `timezone` drives the
/// `convert_to_utc` rewrite, `identifier` renders issue URLs.
#[derive(Debug, Clone)]
pub struct ProjectRow {
    pub id: uuid::Uuid,
    pub workspace_id: uuid::Uuid,
    pub cycle_view: bool,
    pub timezone: Option<String>,
    pub identifier: String,
}

impl ProjectRow {
    pub fn decode(row: &sqlx::postgres::PgRow, site: &str) -> Result<Self, Denial> {
        Ok(ProjectRow {
            id: row_uuid(row, "id", site)?,
            workspace_id: row_uuid(row, "workspace_id", site)?,
            cycle_view: row
                .try_get::<bool, _>("cycle_view")
                .map_err(|error| db_error(error, site))?,
            timezone: row_string_opt(row, "timezone", site)?,
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
        r#"SELECT "id", "workspace_id", "cycle_view", "timezone", "identifier" FROM "projects"
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

// ---------------------------------------------------------------------------
// Reads: cycle list / detail / archived list
// ---------------------------------------------------------------------------

/// Renumber `$N` placeholders to `$1..$k` in order of first appearance
/// (single-quoted literals untouched): the db layer's fixed slots (`$1`
/// slug, `$2` project, `$3` cycle, `$4` actor, `$5` now) leave gaps on
/// Q1/Q3, which Postgres rejects (`could not determine data type of
/// parameter $3`, verified), so handlers compact before binding
/// positionally in slot order.
pub fn compact_binds(sql: &str) -> String {
    let mut out = String::with_capacity(sql.len());
    let mut mapping: Vec<u32> = Vec::new();
    let mut chars = sql.chars().peekable();
    let mut in_string = false;
    while let Some(ch) = chars.next() {
        if in_string {
            out.push(ch);
            if ch == '\'' {
                if chars.peek() == Some(&'\'') {
                    out.push(chars.next().expect("peeked quote"));
                } else {
                    in_string = false;
                }
            }
            continue;
        }
        if ch == '\'' {
            in_string = true;
            out.push(ch);
            continue;
        }
        if ch == '$' {
            let mut digits = String::new();
            while let Some(digit) = chars.peek() {
                if !digit.is_ascii_digit() {
                    break;
                }
                digits.push(*digit);
                chars.next();
            }
            if digits.is_empty() {
                out.push('$');
                continue;
            }
            let slot: u32 = digits.parse().unwrap_or(u32::MAX);
            let position = match mapping.iter().position(|seen| *seen == slot) {
                Some(index) => index + 1,
                None => {
                    mapping.push(slot);
                    mapping.len()
                }
            };
            out.push('$');
            out.push_str(&position.to_string());
            continue;
        }
        out.push(ch);
    }
    out
}

/// `GET .../cycles/` (`views/cycle.py:190-281`): gate → project 404 →
/// `cycle_view` filter → the `current` arm answers a bare list (ported
/// bug) while every other arm paginates. `?order_by=` is ignored (the
/// queryset hardcodes `-created_at` from kwargs). The `current` arm's two
/// `timezone.now()` samples become one `$5` bind (the builder's single
/// slot; the microsecond skew between Django's two samples is
/// unobservable outside a boundary microsecond).
pub async fn list_cycles_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    project_raw: &str,
    query: &QueryMap,
) -> Result<Response, Denial> {
    use pidash_db::v1_cycles_modules::cycle_queries as queries;
    use pidash_services::v1_cycles_modules::cycle_queries as shapes;
    let pre = preamble(state, headers, slug).await?;
    let project_id = rewrite_project_id(&pre.pool, slug, project_raw).await?;
    let workspace_id = pre.workspace_id.ok_or(Denial::Forbidden)?;
    require_gate(
        &pre.pool,
        &workspace_id,
        slug,
        &pre.actor.id,
        &project_id,
        "GET",
        PATH_CYCLES,
    )
    .await?;
    fetch_project(&pre.pool, &project_id, &workspace_id).await?;
    let view = shapes::parse_cycle_view(query_last(query, "cycle_view").as_deref());
    let sql = compact_binds(&queries::cycle_list_filtered_sql(
        &queries::OrderBy::default_cycle(),
        view,
    ));
    let now = micros_now();
    let mut request = sqlx::query(&sql)
        .bind(slug)
        .bind(project_id)
        .bind(pre.actor.id);
    if !matches!(view, queries::CycleView::All | queries::CycleView::Draft) {
        request = request.bind(now);
    }
    let rows: Vec<sqlx::postgres::PgRow> = request
        .fetch_all(&pre.pool)
        .await
        .map_err(|error| db_error(error, "cycles-list"))?;
    let fields = fields_param(query, "fields");
    let expand = fields_param(query, "expand");
    if !shapes::cycle_view_is_paginated(view) {
        let mut results = Vec::with_capacity(rows.len());
        for row in &rows {
            let detail = CycleDetail::decode(row, "cycles-list")?
                .with_list_annotations(row, "cycles-list")?;
            results.push(
                render_cycle(
                    &pre.pool,
                    &detail,
                    &pre.actor.timezone,
                    fields.as_deref(),
                    expand.as_deref(),
                )
                .await?,
            );
        }
        return serde_json::to_string(&Value::Array(results))
            .map(json_ok)
            .map_err(|error| db_error(error, "cycles-list-render"));
    }
    let window = page_window(query, rows.len())?;
    let mut results = Vec::new();
    for row in &rows[window.start..window.stop] {
        let detail =
            CycleDetail::decode(row, "cycles-list")?.with_list_annotations(row, "cycles-list")?;
        results.push(
            render_cycle(
                &pre.pool,
                &detail,
                &pre.actor.timezone,
                fields.as_deref(),
                expand.as_deref(),
            )
            .await?,
        );
    }
    page_envelope(&window, rows.len(), results)
}

/// `GET .../cycles/<pk>/` (`views/cycle.py:462-476`): live cycles only
/// (an archived pk 404s through the queryset filter).
pub async fn retrieve_cycle_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    project_raw: &str,
    pk: &uuid::Uuid,
    query: &QueryMap,
) -> Result<Response, Denial> {
    use pidash_db::v1_cycles_modules::cycle_queries as queries;
    let pre = preamble(state, headers, slug).await?;
    let project_id = rewrite_project_id(&pre.pool, slug, project_raw).await?;
    let workspace_id = pre.workspace_id.ok_or(Denial::Forbidden)?;
    require_gate(
        &pre.pool,
        &workspace_id,
        slug,
        &pre.actor.id,
        &project_id,
        "GET",
        PATH_CYCLE_DETAIL,
    )
    .await?;
    fetch_project(&pre.pool, &project_id, &workspace_id).await?;
    let sql = queries::cycle_detail_sql(&queries::OrderBy::default_cycle());
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(&sql)
        .bind(slug)
        .bind(project_id)
        .bind(pk)
        .bind(pre.actor.id)
        .fetch_optional(&pre.pool)
        .await
        .map_err(|error| db_error(error, "cycle-detail"))?;
    let Some(row) = row else {
        return Err(Denial::NotFound);
    };
    let detail =
        CycleDetail::decode(&row, "cycle-detail")?.with_list_annotations(&row, "cycle-detail")?;
    let fields = fields_param(query, "fields");
    let expand = fields_param(query, "expand");
    let value = render_cycle(
        &pre.pool,
        &detail,
        &pre.actor.timezone,
        fields.as_deref(),
        expand.as_deref(),
    )
    .await?;
    serde_json::to_string(&value)
        .map(json_ok)
        .map_err(|error| db_error(error, "cycle-detail-render"))
}

/// `GET .../archived-cycles/` (`views/cycle.py:741-751`): the sole
/// estimate-sums annotator (all 9 metric keys render).
pub async fn list_archived_cycles_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    project_raw: &str,
    query: &QueryMap,
) -> Result<Response, Denial> {
    use pidash_db::v1_cycles_modules::cycle_queries as queries;
    let pre = preamble(state, headers, slug).await?;
    let project_id = rewrite_project_id(&pre.pool, slug, project_raw).await?;
    let workspace_id = pre.workspace_id.ok_or(Denial::Forbidden)?;
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
    let sql = compact_binds(&queries::archived_cycle_list_sql(
        &queries::OrderBy::default_cycle(),
    ));
    let rows: Vec<sqlx::postgres::PgRow> = sqlx::query(&sql)
        .bind(slug)
        .bind(project_id)
        .bind(pre.actor.id)
        .fetch_all(&pre.pool)
        .await
        .map_err(|error| db_error(error, "cycles-archived"))?;
    let window = page_window(query, rows.len())?;
    let fields = fields_param(query, "fields");
    let expand = fields_param(query, "expand");
    let mut results = Vec::new();
    for row in &rows[window.start..window.stop] {
        let detail = CycleDetail::decode(row, "cycles-archived")?
            .with_archived_annotations(row, "cycles-archived")?;
        results.push(
            render_cycle(
                &pre.pool,
                &detail,
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

/// `CycleCreateSerializer.validate` (`serializers/cycle.py:61-106`) over a
/// coerced write: project resolution (context → body → instance) + gates,
/// microsecond date ordering, the `convert_to_utc` rewrite (both dates
/// only), the `owned_by` default. Built from the granular types/services
/// fns (`resolve_create_project_id`, `validate_project_gate`,
/// `convert_start_to_utc`, `convert_end_to_utc`, `rewrite_inputs_present`,
/// `non_field_error_body`) rather than `run_create_validate`, whose epoch
/// seconds truncate three exact arms: the today-start arm returns
/// full-micros `now()`, lone dates pass through unmodified, and the order
/// check compares microsecond datetimes.
#[derive(Debug, Clone, Copy)]
pub struct ValidatedDates {
    pub start: Option<chrono::DateTime<chrono::Utc>>,
    pub end: Option<chrono::DateTime<chrono::Utc>>,
    pub owned_by: uuid::Uuid,
}

#[allow(clippy::too_many_arguments)]
pub fn run_validate(
    project_id: &uuid::Uuid,
    body_project_id: Option<&str>,
    instance_project_id: Option<&str>,
    project: Option<&ProjectRow>,
    start: Option<chrono::DateTime<chrono::Utc>>,
    end: Option<chrono::DateTime<chrono::Utc>>,
    owned_by: Option<uuid::Uuid>,
    actor: &uuid::Uuid,
    user_timezone: &Tz,
    now: &chrono::DateTime<chrono::Utc>,
) -> Result<ValidatedDates, Denial> {
    use pidash_types::v1_cycles_modules::cycle_shapes as shapes;
    // `cycle.py:67-84`: resolve + gates in raise order. The context id
    // (the rewritten URL kwarg) is always truthy on the wire, so the body
    // and instance arms below only matter structurally.
    let context_id = project_id.to_string();
    let resolved = shapes::resolve_create_project_id(
        Some(context_id.as_str()),
        body_project_id,
        instance_project_id,
    );
    if let Err(message) = shapes::validate_project_gate(resolved, project.map(|row| row.cycle_view))
    {
        return Err(Denial::FieldErrors(shapes::non_field_error_body(message)));
    }
    // `cycle.py:85-90`: both dates set and start > end (microsecond
    // datetimes, not epoch seconds).
    if let (Some(start), Some(end)) = (start, end) {
        if start > end {
            return Err(Denial::FieldErrors(shapes::non_field_error_body(
                shapes::START_AFTER_END_MESSAGE,
            )));
        }
    }
    // `cycle.py:92-101`: the rewrite fires only when BOTH dates are set; a
    // lone date passes through as parsed (reachable on PATCH, whose view
    // has no both-or-neither gate).
    let (start_out, end_out) = match (start, end) {
        (Some(start), Some(end)) => {
            let project = project.ok_or(Denial::ServerError)?;
            let project_tz_name = project.timezone.clone().unwrap_or_default();
            if !shapes::rewrite_inputs_present(Some("set"), Some(project_tz_name.as_str())) {
                // `convert_to_utc` raises `ValueError` → generic 500.
                return Err(Denial::ServerError);
            }
            let project_tz: Tz = project_tz_name.parse().map_err(|error| {
                tracing::warn!(%error, "unknown project timezone");
                Denial::ServerError
            })?;
            // The rewrite day is the ENFORCED date: the instant in the
            // actor's zone (`data["start_date"].date()` post-`enforce`).
            let start_day = start.with_timezone(user_timezone).date_naive();
            let end_day = end.with_timezone(user_timezone).date_naive();
            let start_midnight = project_midnight_utc(&project_tz, &start_day)?;
            let end_midnight = project_midnight_utc(&project_tz, &end_day)?;
            let now_project_day = now.with_timezone(&project_tz).date_naive();
            let start_out = if start_day == now_project_day {
                // Same-day arm (`timezone_converter.py:82-83`): the
                // current instant, micros and all.
                *now
            } else {
                let epoch = shapes::convert_start_to_utc(start_midnight.timestamp(), false, 0);
                chrono::DateTime::from_timestamp(epoch, 0).ok_or(Denial::ServerError)?
            };
            let end_out = {
                let epoch = shapes::convert_end_to_utc(end_midnight.timestamp());
                chrono::DateTime::from_timestamp(epoch, 0).ok_or(Denial::ServerError)?
            };
            (Some(start_out), Some(end_out))
        }
        (start, end) => (start, end),
    };
    // `cycle.py:103-104`: any provided owner wins, else the requester.
    Ok(ValidatedDates {
        start: start_out,
        end: end_out,
        owned_by: owned_by.unwrap_or(*actor),
    })
}

/// One project-zone midnight as UTC (`convert_to_utc`'s
/// `local_tz.localize(midnight)`, whose `is_dst` defaults to `False`):
/// folds take the late (standard) side, gaps the pre-transition offset —
/// pytz never raises here (fuzzed against every pytz fold/gap midnight,
/// 1990-2035, across `pytz.common_timezones`; see
/// `project_midnight_pytz_parity`).
pub fn project_midnight_utc(
    project_tz: &Tz,
    day: &chrono::NaiveDate,
) -> Result<chrono::DateTime<chrono::Utc>, Denial> {
    let midnight = day.and_hms_opt(0, 0, 0).ok_or(Denial::ServerError)?;
    match project_tz.from_local_datetime(&midnight) {
        LocalResult::Single(local) => Ok(local.with_timezone(&chrono::Utc)),
        LocalResult::Ambiguous(_, late) => Ok(late.with_timezone(&chrono::Utc)),
        LocalResult::None => {
            let offset_secs =
                pre_transition_offset(project_tz, &midnight).ok_or(Denial::ServerError)?;
            midnight
                .checked_sub_signed(chrono::Duration::seconds(offset_secs))
                .map(|shifted| chrono::DateTime::from_naive_utc_and_offset(shifted, chrono::Utc))
                .ok_or(Denial::ServerError)
        }
    }
}

/// `POST .../cycles/` (`views/cycle.py:299-356`): gate → both-or-neither
/// shape gate → field coercion → `validate()` → external-dup 409 → insert
/// → `model_activity` → bare re-read → 201.
pub async fn create_cycle_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    project_raw: &str,
    body: &[u8],
) -> Result<Response, Denial> {
    use pidash_services::v1_cycles_modules::cycle_shapes as shapes;
    let pre = preamble(state, headers, slug).await?;
    let project_id = rewrite_project_id(&pre.pool, slug, project_raw).await?;
    let workspace_id = pre.workspace_id.ok_or(Denial::Forbidden)?;
    require_gate(
        &pre.pool,
        &workspace_id,
        slug,
        &pre.actor.id,
        &project_id,
        "POST",
        PATH_CYCLES,
    )
    .await?;
    // The both-or-neither gate runs on `request.data` BEFORE any
    // serializer (`views/cycle.py:305`), so a non-object body 500s on
    // `.get` instead of answering serializer errors.
    let raw = parse_object_or_500(body)?;
    // Both-or-neither shape gate on the RAW body (`views/cycle.py:305-356`):
    // present means the key exists with a non-null value.
    let start_present = raw.get("start_date").is_some_and(|v| !v.is_null());
    let end_present = raw.get("end_date").is_some_and(|v| !v.is_null());
    if !shapes::create_dates_shape_ok(start_present, end_present) {
        return Err(Denial::BadError(
            shapes::CREATE_DATES_SHAPE_MESSAGE.to_owned(),
        ));
    }
    let write = coerce_write(&pre.pool, &raw, false, &pre.actor.timezone).await?;
    // `validate()` (`serializers/cycle.py:61-106`): the project row comes
    // from `filter().first()` (missing → the 400 arm, unreachable past the
    // gate but ported), the legacy body id from the raw `project_id` key.
    let project: Option<ProjectRow> = sqlx::query(
        r#"SELECT "id", "workspace_id", "cycle_view", "timezone", "identifier" FROM "projects"
           WHERE "id" = $1 AND "deleted_at" IS NULL"#,
    )
    .bind(project_id)
    .fetch_optional(&pre.pool)
    .await
    .map_err(|error| db_error(error, "cycle-create-project"))?
    .map(|row| ProjectRow::decode(&row, "cycle-create-project"))
    .transpose()?;
    let body_project_id = raw.get("project_id").and_then(|v| v.as_str());
    let now = micros_now();
    let validated = run_validate(
        &project_id,
        body_project_id,
        None,
        project.as_ref(),
        write.start_date.flatten(),
        write.end_date.flatten(),
        write.owned_by.flatten(),
        &pre.actor.id,
        &pre.actor.timezone,
        &now,
    )?;
    let project = project.ok_or(Denial::ServerError)?;
    // External-dup check on the RAW body values (both truthy): answers the
    // clash row's id (`views/cycle.py:313-335`).
    if let (Some(raw_id), Some(raw_source)) = (raw.get("external_id"), raw.get("external_source")) {
        if py_truthy(raw_id) && py_truthy(raw_source) {
            if let (Some(external_id), Some(external_source)) =
                (prep_text(raw_id), prep_text(raw_source))
            {
                let clash: Option<uuid::Uuid> = sqlx::query_scalar(
                    r#"SELECT c."id" FROM "cycles" c
                       INNER JOIN "workspaces" w ON c."workspace_id" = w."id"
                       WHERE w."slug" = $1 AND c."project_id" = $2 AND c."external_source" = $3 AND c."external_id" = $4 AND c."deleted_at" IS NULL
                       ORDER BY c."created_at" DESC LIMIT 1"#,
                )
                .bind(slug)
                .bind(project_id)
                .bind(external_source)
                .bind(external_id)
                .fetch_optional(&pre.pool)
                .await
                .map_err(|error| db_error(error, "cycle-create-extdup"))?;
                if let Some(clash) = clash {
                    let body = format!(
                        "{{\"error\":{},\"id\":{}}}",
                        json_string(
                            "Cycle with the same external id and external source already exists"
                        ),
                        json_string(&clash.to_string()),
                    );
                    return Err(Denial::Conflict(body));
                }
            }
        }
    }
    // `Cycle.save`: `sort_order = min - 10000`, or the 65535 default for
    // the first cycle (`db/models/cycle.py:88-97`).
    let cycle_id = uuid::Uuid::new_v4();
    let min_sort: Option<f64> = sqlx::query_scalar(
        r#"SELECT MIN("sort_order") FROM "cycles" WHERE "project_id" = $1 AND "deleted_at" IS NULL"#,
    )
    .bind(project_id)
    .fetch_optional(&pre.pool)
    .await
    .map_err(|error| db_error(error, "cycle-create-sort"))?
    .flatten();
    let sort_order = min_sort.map_or(65535.0, |min| min - 10000.0);
    // `serializer.save(project_id=...)`: model defaults fill the absent
    // fields (`description` "", `timezone` "UTC", dates/externals NULL);
    // `BaseModel.save` stamps `created_by` and leaves `updated_by` NULL.
    // `auto_now_add`/`auto_now` sample separately, like Django's two
    // `pre_save` calls.
    let name = write.name.clone().ok_or(Denial::ServerError)?;
    let created_at = micros_now();
    let updated_at = micros_now();
    let timezone = write.timezone.clone().unwrap_or_else(|| "UTC".to_owned());
    sqlx::query(
        r#"INSERT INTO "cycles" ("id", "created_at", "updated_at", "created_by_id", "updated_by_id", "deleted_at",
               "project_id", "workspace_id", "name", "description", "start_date", "end_date", "owned_by_id",
               "view_props", "sort_order", "external_source", "external_id", "progress_snapshot",
               "archived_at", "logo_props", "timezone", "version")
           VALUES ($1, $2, $3, $4, NULL, NULL, $5, $6, $7, $8, $9, $10, $11, '{}', $12, $13, $14, '{}', NULL, '{}', $15, 1)"#,
    )
    .bind(cycle_id)
    .bind(created_at)
    .bind(updated_at)
    .bind(pre.actor.id)
    .bind(project_id)
    .bind(project.workspace_id)
    .bind(&name)
    .bind(write.description.clone().unwrap_or_default())
    .bind(validated.start)
    .bind(validated.end)
    .bind(validated.owned_by)
    .bind(sort_order)
    .bind(write.external_source.clone().flatten())
    .bind(write.external_id.clone().flatten())
    .bind(&timezone)
    .execute(&pre.pool)
    .await
    .map_err(|error| db_write_error(error, "cycle-create-insert"))?;
    let job = pidash_jobs::v1_cycles_modules::publish::model_created_job(
        "cycle",
        &cycle_id.to_string(),
        &raw,
        &pre.actor.id.to_string(),
        slug,
        &app_origin(state),
    );
    enqueue_best_effort(&pre.pool, &job).await;
    // `Cycle.objects.get(pk=...)` + bare `CycleSerializer` (no annotations:
    // all 9 metric keys are absent).
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(
        r#"SELECT "id", "created_at", "updated_at", "created_by_id", "updated_by_id", "deleted_at",
                  "project_id", "workspace_id", "name", "description", "start_date", "end_date", "owned_by_id",
                  "view_props", "sort_order", "external_source", "external_id", "progress_snapshot",
                  "archived_at", "logo_props", "timezone", "version"
           FROM "cycles" WHERE "id" = $1 AND "deleted_at" IS NULL"#,
    )
    .bind(cycle_id)
    .fetch_optional(&pre.pool)
    .await
    .map_err(|error| db_error(error, "cycle-create-reread"))?;
    let Some(row) = row else {
        return Err(Denial::ServerError);
    };
    let detail = CycleDetail::decode(&row, "cycle-create-reread")?;
    let value = render_cycle(&pre.pool, &detail, &pre.actor.timezone, None, None).await?;
    serde_json::to_string(&value)
        .map(json_created)
        .map_err(|error| db_error(error, "cycle-create-render"))
}

/// `PATCH .../cycles/<pk>/` (`views/cycle.py:494-561`): plain `.get`
/// (no archived filter) → snapshot → archived 400 → completed gate →
/// field coercion → `validate()` → external-dup 409 → update →
/// `model_activity` → bare re-serialised `CycleSerializer`, 200.
///
/// The completed gate narrows a dead local (`request_data`); the
/// serializer, the dup check and the save all read the FULL `request.data`
/// — so a completed-cycle PATCH carrying `sort_order` proceeds, and one
/// carrying `name`+`sort_order` edits the name (ported bugs).
///
/// The gate itself runs Python `in` on the RAW value (`views/cycle.py:513`):
/// dicts test the key, lists test membership (`==`), strings test
/// substring — and a hit on a list/str 500s on the narrowing `.get`
/// (neither type has one). `in` on null/number/bool raises `TypeError` →
/// 500. (All six non-dict shapes verified live against Django.)
pub fn completed_gate(value: &Value) -> Result<(), Denial> {
    const MESSAGE: &str = "The Cycle has already been completed so it cannot be edited";
    let reject = || Denial::BadError(MESSAGE.to_owned());
    match value {
        Value::Object(map) => {
            if map.contains_key("sort_order") {
                Ok(())
            } else {
                Err(reject())
            }
        }
        Value::Array(items) => {
            if items
                .iter()
                .any(|item| item == &Value::String("sort_order".to_owned()))
            {
                Err(Denial::ServerError)
            } else {
                Err(reject())
            }
        }
        Value::String(text) => {
            if text.contains("sort_order") {
                Err(Denial::ServerError)
            } else {
                Err(reject())
            }
        }
        Value::Null | Value::Number(_) | Value::Bool(_) => Err(Denial::ServerError),
    }
}

pub async fn patch_cycle_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    project_raw: &str,
    pk: &uuid::Uuid,
    body: &[u8],
) -> Result<Response, Denial> {
    let pre = preamble(state, headers, slug).await?;
    let project_id = rewrite_project_id(&pre.pool, slug, project_raw).await?;
    let workspace_id = pre.workspace_id.ok_or(Denial::Forbidden)?;
    require_gate(
        &pre.pool,
        &workspace_id,
        slug,
        &pre.actor.id,
        &project_id,
        "PATCH",
        PATH_CYCLE_DETAIL,
    )
    .await?;
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(
        r#"SELECT c."id", c."created_at", c."updated_at", c."created_by_id", c."updated_by_id", c."deleted_at",
                  c."project_id", c."workspace_id", c."name", c."description", c."start_date", c."end_date", c."owned_by_id",
                  c."view_props", c."sort_order", c."external_source", c."external_id", c."progress_snapshot",
                  c."archived_at", c."logo_props", c."timezone", c."version"
           FROM "cycles" c INNER JOIN "workspaces" w ON c."workspace_id" = w."id"
           WHERE w."slug" = $1 AND c."project_id" = $2 AND c."id" = $3 AND c."deleted_at" IS NULL"#,
    )
    .bind(slug)
    .bind(project_id)
    .bind(pk)
    .fetch_optional(&pre.pool)
    .await
    .map_err(|error| db_error(error, "cycle-patch-get"))?;
    let Some(row) = row else {
        return Err(Denial::NotFound);
    };
    let before = CycleDetail::decode(&row, "cycle-patch-get")?;
    // `current_instance`: `json.dumps(CycleSerializer(cycle).data)` of the
    // BARE pre-save instance (no annotations), Python separators — taken
    // BEFORE the archived check (`views/cycle.py:502`).
    let snapshot = render_cycle(&pre.pool, &before, &pre.actor.timezone, None, None).await?;
    let snapshot_text = pidash_jobs::tasks_webhooks::activity_dispatch::django_dumps(&snapshot);
    if before.archived_at.is_some() {
        return Err(Denial::BadError(
            "Archived cycle cannot be edited".to_owned(),
        ));
    }
    let value = parse_body_value(body)?;
    // Completed gate (`views/cycle.py:512-520`) on the RAW value, before
    // the serializer: without `sort_order` in it the edit is rejected;
    // with it the (dead) narrowing runs and the FULL body proceeds below.
    if let Some(end_date) = before.end_date {
        if end_date < micros_now() {
            completed_gate(&value)?;
        }
    }
    let raw = coerce_body_object(value)?;
    let write = coerce_write(&pre.pool, &raw, true, &pre.actor.timezone).await?;
    // `validate()`: the project row comes from `filter().first()`; the
    // instance arm carries the cycle's own project id (same value here).
    let project: Option<ProjectRow> = sqlx::query(
        r#"SELECT "id", "workspace_id", "cycle_view", "timezone", "identifier" FROM "projects"
           WHERE "id" = $1 AND "deleted_at" IS NULL"#,
    )
    .bind(project_id)
    .fetch_optional(&pre.pool)
    .await
    .map_err(|error| db_error(error, "cycle-patch-project"))?
    .map(|row| ProjectRow::decode(&row, "cycle-patch-project"))
    .transpose()?;
    let body_project_id = raw.get("project_id").and_then(|v| v.as_str());
    let instance_project_id = before.project_id.to_string();
    let now = micros_now();
    let validated = run_validate(
        &project_id,
        body_project_id,
        Some(instance_project_id.as_str()),
        project.as_ref(),
        write.start_date.flatten(),
        write.end_date.flatten(),
        write.owned_by.flatten(),
        &pre.actor.id,
        &pre.actor.timezone,
        &now,
    )?;
    // PATCH external-dup (`views/cycle.py:529-545`): the RAW external id
    // must be truthy and differ from the stored one (Python equality: a
    // stored `"5"` differs from a raw `5`); the source is the raw value
    // when the key is present (explicit null filters `IS NULL`) else the
    // stored one. Answers the CURRENT cycle's id (the POST/PATCH
    // asymmetry).
    if let Some(raw_id) = raw.get("external_id") {
        if py_truthy(raw_id) && !py_equals_stored(&before.external_id, raw_id) {
            let source: Option<String> = match raw.get("external_source") {
                None => before.external_source.clone(),
                Some(Value::Null) => None,
                Some(value) => prep_text(value),
            };
            let clash: bool = match (prep_text(raw_id), source) {
                (Some(external_id), Some(external_source)) => sqlx::query_scalar(
                    r#"SELECT EXISTS(SELECT 1 FROM "cycles" c
                       INNER JOIN "workspaces" w ON c."workspace_id" = w."id"
                       WHERE w."slug" = $1 AND c."project_id" = $2 AND c."external_source" = $3 AND c."external_id" = $4 AND c."deleted_at" IS NULL)"#,
                )
                .bind(slug)
                .bind(project_id)
                .bind(external_source)
                .bind(external_id)
                .fetch_one(&pre.pool)
                .await
                .map_err(|error| db_error(error, "cycle-patch-extdup"))?,
                (Some(external_id), None) => sqlx::query_scalar(
                    r#"SELECT EXISTS(SELECT 1 FROM "cycles" c
                       INNER JOIN "workspaces" w ON c."workspace_id" = w."id"
                       WHERE w."slug" = $1 AND c."project_id" = $2 AND c."external_source" IS NULL AND c."external_id" = $3 AND c."deleted_at" IS NULL)"#,
                )
                .bind(slug)
                .bind(project_id)
                .bind(external_id)
                .fetch_one(&pre.pool)
                .await
                .map_err(|error| db_error(error, "cycle-patch-extdup"))?,
                _ => false,
            };
            if clash {
                let body = format!(
                    "{{\"error\":{},\"id\":{}}}",
                    json_string(
                        "Cycle with the same external id and external source already exists"
                    ),
                    json_string(&before.id.to_string()),
                );
                return Err(Denial::Conflict(body));
            }
        }
    }
    // `super().update` + `save()`: provided fields plus `owned_by`
    // (`validate()` always sets it: submitted or the requester —
    // `serializers/cycle.py:103-104`), `updated_at` auto, `updated_by`
    // stamped by CRUM.
    let now = micros_now();
    update_cycle_fields(
        &pre.pool,
        pk,
        &write,
        &validated,
        &pre.actor.id,
        &now,
        "cycle-patch-update",
    )
    .await?;
    let job = pidash_jobs::v1_cycles_modules::publish::model_updated_job(
        "cycle",
        &pk.to_string(),
        &raw,
        &snapshot_text,
        &pre.actor.id.to_string(),
        slug,
        &app_origin(state),
    );
    enqueue_best_effort(&pre.pool, &job).await;
    // `Cycle.objects.get(pk=...)` + bare `CycleSerializer` (no annotations).
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(
        r#"SELECT "id", "created_at", "updated_at", "created_by_id", "updated_by_id", "deleted_at",
                  "project_id", "workspace_id", "name", "description", "start_date", "end_date", "owned_by_id",
                  "view_props", "sort_order", "external_source", "external_id", "progress_snapshot",
                  "archived_at", "logo_props", "timezone", "version"
           FROM "cycles" WHERE "id" = $1 AND "deleted_at" IS NULL"#,
    )
    .bind(pk)
    .fetch_optional(&pre.pool)
    .await
    .map_err(|error| db_error(error, "cycle-patch-reread"))?;
    let Some(row) = row else {
        return Err(Denial::ServerError);
    };
    let after = CycleDetail::decode(&row, "cycle-patch-reread")?;
    let value = render_cycle(&pre.pool, &after, &pre.actor.timezone, None, None).await?;
    serde_json::to_string(&value)
        .map(json_ok)
        .map_err(|error| db_error(error, "cycle-patch-render"))
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
/// present keys move) plus the `save()` stamps. Dates move only when
/// submitted, in their validated (possibly rewritten) form; `owned_by`
/// ALWAYS moves (`validate()` defaults it to the requester).
pub async fn update_cycle_fields(
    pool: &PgPool,
    pk: &uuid::Uuid,
    write: &CycleWrite,
    validated: &ValidatedDates,
    actor: &uuid::Uuid,
    now: &chrono::DateTime<chrono::Utc>,
    site: &str,
) -> Result<(), Denial> {
    // One statement per field keeps the partial-update semantics obvious;
    // Django issues a single `UPDATE` with the same assignments.
    if let Some(ref name) = write.name {
        sqlx::query(r#"UPDATE "cycles" SET "name" = $1 WHERE "id" = $2"#)
            .bind(name)
            .bind(pk)
            .execute(pool)
            .await
            .map_err(|error| db_write_error(error, site))?;
    }
    if let Some(ref description) = write.description {
        sqlx::query(r#"UPDATE "cycles" SET "description" = $1 WHERE "id" = $2"#)
            .bind(description)
            .bind(pk)
            .execute(pool)
            .await
            .map_err(|error| db_write_error(error, site))?;
    }
    if write.start_date.is_some() {
        sqlx::query(r#"UPDATE "cycles" SET "start_date" = $1 WHERE "id" = $2"#)
            .bind(validated.start)
            .bind(pk)
            .execute(pool)
            .await
            .map_err(|error| db_write_error(error, site))?;
    }
    if write.end_date.is_some() {
        sqlx::query(r#"UPDATE "cycles" SET "end_date" = $1 WHERE "id" = $2"#)
            .bind(validated.end)
            .bind(pk)
            .execute(pool)
            .await
            .map_err(|error| db_write_error(error, site))?;
    }
    if let Some(ref external_source) = write.external_source {
        sqlx::query(r#"UPDATE "cycles" SET "external_source" = $1 WHERE "id" = $2"#)
            .bind(external_source)
            .bind(pk)
            .execute(pool)
            .await
            .map_err(|error| db_write_error(error, site))?;
    }
    if let Some(ref external_id) = write.external_id {
        sqlx::query(r#"UPDATE "cycles" SET "external_id" = $1 WHERE "id" = $2"#)
            .bind(external_id)
            .bind(pk)
            .execute(pool)
            .await
            .map_err(|error| db_write_error(error, site))?;
    }
    if let Some(ref timezone) = write.timezone {
        sqlx::query(r#"UPDATE "cycles" SET "timezone" = $1 WHERE "id" = $2"#)
            .bind(timezone)
            .bind(pk)
            .execute(pool)
            .await
            .map_err(|error| db_write_error(error, site))?;
    }
    // `validate()` always sets `owned_by` (submitted or the requester), so
    // every PATCH re-stamps the owner (`serializers/cycle.py:103-104`).
    sqlx::query(
        r#"UPDATE "cycles" SET "owned_by_id" = $1, "updated_at" = $2, "updated_by_id" = $3 WHERE "id" = $4"#,
    )
    .bind(validated.owned_by)
    .bind(now)
    .bind(actor)
    .bind(pk)
    .execute(pool)
    .await
    .map_err(|error| db_write_error(error, site))?;
    Ok(())
}

/// `DELETE .../cycles/<pk>/` (`views/cycle.py:571-613`): plain `.get` →
/// owner-or-project-admin 403 → collect live bridge issue ids →
/// `issue_activity` → soft-delete (+ its fan-out task) → bridge + favorite
/// queryset soft-deletes → 204.
pub async fn delete_cycle_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    project_raw: &str,
    pk: &uuid::Uuid,
) -> Result<Response, Denial> {
    let pre = preamble(state, headers, slug).await?;
    let project_id = rewrite_project_id(&pre.pool, slug, project_raw).await?;
    let workspace_id = pre.workspace_id.ok_or(Denial::Forbidden)?;
    require_gate(
        &pre.pool,
        &workspace_id,
        slug,
        &pre.actor.id,
        &project_id,
        "DELETE",
        PATH_CYCLE_DETAIL,
    )
    .await?;
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(
        r#"SELECT c."id", c."name", c."owned_by_id" FROM "cycles" c
           INNER JOIN "workspaces" w ON c."workspace_id" = w."id"
           WHERE w."slug" = $1 AND c."project_id" = $2 AND c."id" = $3 AND c."deleted_at" IS NULL"#,
    )
    .bind(slug)
    .bind(project_id)
    .bind(pk)
    .fetch_optional(&pre.pool)
    .await
    .map_err(|error| db_error(error, "cycle-delete-get"))?;
    let Some(row) = row else {
        return Err(Denial::NotFound);
    };
    let name = row_string(&row, "name", "cycle-delete-get")?;
    let owned_by = row_uuid(&row, "owned_by_id", "cycle-delete-get")?;
    // Owner or project admin (role 20), else the view's own inline
    // 403 (`views/cycle.py:578-584` — a `Response`, not `PermissionDenied`,
    // so the `{"error": ...}` key, not the class body).
    if owned_by != pre.actor.id {
        let admin: bool = sqlx::query_scalar(
            r#"SELECT EXISTS(SELECT 1 FROM "project_members" WHERE "workspace_id" = $1 AND "member_id" = $2 AND "role" = 20 AND "project_id" = $3 AND "is_active" AND "deleted_at" IS NULL)"#,
        )
        .bind(workspace_id)
        .bind(pre.actor.id)
        .bind(project_id)
        .fetch_one(&pre.pool)
        .await
        .map_err(|error| db_error(error, "cycle-delete-admin"))?;
        if !admin {
            return Err(Denial::ForbiddenError(
                "Only admin or creator can delete the cycle".to_owned(),
            ));
        }
    }
    // The payload reads the LIVE bridge issue ids before the delete
    // (`views/cycle.py:586-590`, default `-created_at` order).
    let issues: Vec<uuid::Uuid> = sqlx::query_scalar(
        r#"SELECT "issue_id" FROM "cycle_issues" WHERE "cycle_id" = $1 AND "deleted_at" IS NULL ORDER BY "created_at" DESC"#,
    )
    .bind(pk)
    .fetch_all(&pre.pool)
    .await
    .map_err(|error| db_error(error, "cycle-delete-issues"))?;
    let now = micros_now();
    let issue_texts: Vec<String> = issues.iter().map(|id| id.to_string()).collect();
    let issue_refs: Vec<&str> = issue_texts.iter().map(String::as_str).collect();
    let job = pidash_jobs::v1_cycles_modules::publish::cycle_deleted_job(
        &pk.to_string(),
        &name,
        &issue_refs,
        &pre.actor.id.to_string(),
        &project_id.to_string(),
        now.timestamp(),
    );
    enqueue_best_effort(&pre.pool, &job).await;
    // Instance `delete()`: `deleted_at = now()` then `save()` (whose
    // `auto_now` samples again) plus the CRUM `updated_by`.
    let deleted_at = micros_now();
    let updated_at = micros_now();
    sqlx::query(
        r#"UPDATE "cycles" SET "deleted_at" = $1, "updated_at" = $2, "updated_by_id" = $3 WHERE "id" = $4"#,
    )
    .bind(deleted_at)
    .bind(updated_at)
    .bind(pre.actor.id)
    .bind(pk)
    .execute(&pre.pool)
    .await
    .map_err(|error| db_write_error(error, "cycle-delete-soft"))?;
    enqueue_soft_delete(&pre.pool, "cycle", pk).await;
    // The eager `soft_delete_related_objects` cascade (`cycle.delete()`
    // at `views/cycle.py:610` → `bgtasks/deletion_task.py:18-97`): the
    // reverse-FK manager filters by `cycle_id` only, so moved bridges
    // keeping an old `project_id` die too — no project scope. The cascade
    // `.save()` would re-stamp `updated_at`, but no read observes a
    // soft-deleted bridge (all filter `deleted_at IS NULL`), so a
    // `deleted_at`-only stamp is equivalent. (The `UserFavorite` cleanup
    // below IS project-scoped, per `views/cycle.py:612`.)
    sqlx::query(
        r#"UPDATE "cycle_issues" SET "deleted_at" = $1 WHERE "cycle_id" = $2 AND "deleted_at" IS NULL"#,
    )
    .bind(micros_now())
    .bind(pk)
    .execute(&pre.pool)
    .await
    .map_err(|error| db_write_error(error, "cycle-delete-bridges"))?;
    sqlx::query(
        r#"UPDATE "user_favorites" SET "deleted_at" = $1 WHERE "entity_type" = 'cycle' AND "entity_identifier" = $2 AND "project_id" = $3 AND "deleted_at" IS NULL"#,
    )
    .bind(micros_now())
    .bind(pk)
    .bind(project_id)
    .execute(&pre.pool)
    .await
    .map_err(|error| db_write_error(error, "cycle-delete-favorites"))?;
    Ok(no_content())
}

// ---------------------------------------------------------------------------
// Cycle issues: list / add / detail / remove
// ---------------------------------------------------------------------------

/// `GET .../cycles/<cycle_id>/cycle-issues/` (`views/cycle.py:803-856`):
/// the Q4 GET builder with `?order_by=` (default `created_at` ascending,
/// resolved by [`resolve_cycle_issue_order`] into the builder's
/// [`OrderBy`](pidash_db::v1_cycles_modules::cycle_queries::OrderBy)),
/// rendered as `IssueSerializer` rows. One row per live bridge (the
/// bridge unique constraint forbids fan-out) — except the M2M orderings
/// multiply rows per through-row, so the envelope total counts distinct
/// issue ids there.
pub async fn list_cycle_issues_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    project_raw: &str,
    cycle_id: &uuid::Uuid,
    query: &QueryMap,
) -> Result<Response, Denial> {
    let pre = preamble(state, headers, slug).await?;
    let project_id = rewrite_project_id(&pre.pool, slug, project_raw).await?;
    let workspace_id = pre.workspace_id.ok_or(Denial::Forbidden)?;
    require_gate(
        &pre.pool,
        &workspace_id,
        slug,
        &pre.actor.id,
        &project_id,
        "GET",
        PATH_CYCLE_ISSUES,
    )
    .await?;
    let order = resolve_cycle_issue_order(query_last(query, "order_by").as_deref());
    let sql = pidash_db::v1_cycles_modules::cycle_queries::cycle_issue_list_get_sql(&order);
    let rows: Vec<sqlx::postgres::PgRow> = sqlx::query(&sql)
        .bind(slug)
        .bind(project_id)
        .bind(cycle_id)
        .fetch_all(&pre.pool)
        .await
        .map_err(|error| db_error(error, "cycle-issues-list"))?;
    // The `url` needs the project identifier (plain fetch, deliberately
    // unfiltered: the list queryset `select_related("project")` caches the
    // row, so a soft-deleted project still renders its url — verified live
    // 200 against Django).
    let identifier: Option<String> =
        sqlx::query_scalar(r#"SELECT "identifier" FROM "projects" WHERE "id" = $1"#)
            .bind(project_id)
            .fetch_optional(&pre.pool)
            .await
            .map_err(|error| db_error(error, "cycle-issues-identifier"))?
            .flatten();
    let total = envelope_total(&order, &rows)?;
    let window = page_window(query, rows.len())?;
    let fields = fields_param(query, "fields");
    let expand = fields_param(query, "expand");
    let mut results = Vec::new();
    for row in &rows[window.start..window.stop] {
        let detail = IssueDetail::decode(row, "cycle-issues-list")?;
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

/// One validated `pk__in` candidate for the cycle-issues POST re-query:
/// a UUID to match, or a JSON NULL (renders `IN (..., NULL)`, matching
/// nothing, exactly like Django).
#[derive(Debug, Clone, Copy)]
pub enum IssueCandidate {
    Id(uuid::Uuid),
    Null,
}

/// Validate a truthy raw `issues` value for the add path
/// (`views/cycle.py:946`): truthy scalars (`int`/`bool`/`float`) raise
/// through the `__in` iteration to the generic 500; strings iterate into
/// chars and dicts into keys, where an invalid UUID answers
/// `"Please provide valid detail"`. The falsy gate (missing/empty/null/
/// 0/false → the two-key 400) runs in the caller.
pub fn coerce_add_issues(value: &Value) -> Result<Vec<IssueCandidate>, Denial> {
    match value {
        Value::Null | Value::Number(_) | Value::Bool(_) => Err(Denial::ServerError),
        Value::String(s) => s
            .chars()
            .map(|ch| coerce_issue_candidate(&Value::String(ch.to_string())))
            .collect(),
        Value::Array(items) => items.iter().map(coerce_issue_candidate).collect(),
        Value::Object(map) => map
            .keys()
            .map(|key| coerce_issue_candidate(&Value::String(key.clone())))
            .collect(),
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

/// Django `serializers.serialize("json", ...)` for created bridges: one
/// `{"model", "pk", "fields"}` object per row in list order, Python
/// separators (`", "`, `": "`), datetimes via `DjangoJSONEncoder`
/// (`isoformat` with `+00:00`, micros omitted when zero). The audit pair
/// renders `null`: `bulk_create` skips `save()`, and unlike the module
/// view the cycle view passes no `created_by`/`updated_by`
/// (`views/cycle.py:953-965` vs `views/module.py:697-707`). Field order
/// follows the model (`issue` before `cycle`, `db/models/cycle.py:104`).
/// `issue` renders the RAW request value, not the folded id: the instance
/// attribute is never folded (only the SQL prep is), so protected types
/// pass through as-is (`5`, `true`) and strings verbatim (an uppercase
/// UUID stays uppercase — `handle_field`/`is_protected_type`).
pub fn render_bridge_dump(bridges: &[CreatedBridge]) -> String {
    let mut out = String::from("[");
    for (index, bridge) in bridges.iter().enumerate() {
        if index > 0 {
            out.push_str(", ");
        }
        let issue = serde_json::to_string(&bridge.issue_raw).unwrap_or_else(|_| "null".to_owned());
        out.push_str(&format!(
            "{{\"model\": \"db.cycleissue\", \"pk\": \"{}\", \"fields\": {{\"created_at\": {}, \"updated_at\": {}, \"created_by\": {}, \"updated_by\": {}, \"deleted_at\": null, \"project\": \"{}\", \"workspace\": \"{}\", \"issue\": {}, \"cycle\": \"{}\"}}}}",
            bridge.id,
            render_django_datetime(&bridge.created_at),
            render_django_datetime(&bridge.updated_at),
            render_uuid_opt(&bridge.created_by),
            render_uuid_opt(&bridge.updated_by),
            bridge.project_id,
            bridge.workspace_id,
            issue,
            bridge.cycle_id,
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
/// `issue_id` is the FOLDED id (what the ORM's `to_python` binds at both
/// the filter and the insert); `issue_raw` is the RAW request value, which
/// the dump renders verbatim (the instance attribute is never folded).
#[derive(Debug, Clone)]
pub struct CreatedBridge {
    pub id: uuid::Uuid,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub created_by: Option<uuid::Uuid>,
    pub updated_by: Option<uuid::Uuid>,
    pub project_id: uuid::Uuid,
    pub workspace_id: uuid::Uuid,
    pub cycle_id: uuid::Uuid,
    pub issue_id: uuid::Uuid,
    pub issue_raw: Value,
}

/// One live bridge fetched for the move path (`views/cycle.py:946`).
#[derive(Debug, Clone)]
pub struct ExistingBridge {
    pub id: uuid::Uuid,
    pub issue_id: uuid::Uuid,
    pub cycle_id: uuid::Uuid,
}

/// `POST .../cycles/<cycle_id>/cycle-issues/` (`views/cycle.py:920-1010`):
/// truthiness gate → cycle 404 → completed 400 → existing-bridges lookup
/// (invalid UUIDs 400) → create the new (batches of 10,
/// `ignore_conflicts`) → move the existing (`bulk_update(["cycle_id"])`,
/// audit untouched) → `issue_activity` → the FULL bridge list, 200.
///
/// Unlike the module view (whose move path is dead), the cycle move path
/// is LIVE: `str(issue_id) in issues` compares against the raw JSON
/// strings. Cross-project bridges move keeping their old `project_id`
/// (the update touches only `cycle_id`). The view never runs
/// `CycleIssueRequestSerializer`: the raw body drives everything, so a
/// non-object body 500s (`AttributeError` on `.get`).
///
/// `new_issues` is a `list(set(...))`: multi-issue hash order varies per
/// Django process (unmatchable in principle — Django disagrees with
/// itself across restarts), so the port keeps first-seen order. The
/// single-issue contract path is deterministic either way.
pub async fn add_cycle_issues_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    project_raw: &str,
    cycle_id: &uuid::Uuid,
    body: &[u8],
) -> Result<Response, Denial> {
    let pre = preamble(state, headers, slug).await?;
    let project_id = rewrite_project_id(&pre.pool, slug, project_raw).await?;
    let workspace_id = pre.workspace_id.ok_or(Denial::Forbidden)?;
    require_gate(
        &pre.pool,
        &workspace_id,
        slug,
        &pre.actor.id,
        &project_id,
        "POST",
        PATH_CYCLE_ISSUES,
    )
    .await?;
    // Raw `request.data` (no serializer): empty is `{}`, unparseable is the
    // DRF `ParseError`, a non-object 500s on `.get`.
    let raw = parse_object_or_500(body)?;
    // `if not issues:` — truthiness, not length: missing/empty/null/0/false
    // share the two-key 400 (`views/cycle.py:928-932`).
    let issues_value = raw.get("issues");
    if !issues_value.is_some_and(py_truthy) {
        let body = format!(
            "{{\"error\":{},\"code\":{}}}",
            json_string("Work items are required"),
            json_string("MISSING_WORK_ITEMS"),
        );
        return Err(Denial::FieldErrors(body));
    }
    let issues_value = issues_value.expect("truthy issues checked");
    // Order matters: Python runs `Cycle.objects.get` (404) and the
    // completed gate BEFORE the `issue_id__in` filter validates UUIDs
    // (400), so the coercion runs after both (`views/cycle.py:934-946`).
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(
        r#"SELECT c."id", c."workspace_id", c."end_date" FROM "cycles" c
           INNER JOIN "workspaces" w ON c."workspace_id" = w."id"
           WHERE w."slug" = $1 AND c."project_id" = $2 AND c."id" = $3 AND c."deleted_at" IS NULL"#,
    )
    .bind(slug)
    .bind(project_id)
    .bind(cycle_id)
    .fetch_optional(&pre.pool)
    .await
    .map_err(|error| db_error(error, "cycle-issues-add-cycle"))?;
    let Some(row) = row else {
        return Err(Denial::NotFound);
    };
    let cycle_workspace_id = row_uuid(&row, "workspace_id", "cycle-issues-add-cycle")?;
    if let Some(end_date) = row_datetime_opt(&row, "end_date", "cycle-issues-add-cycle")? {
        if end_date < micros_now() {
            let body = format!(
                "{{\"code\":{},\"message\":{}}}",
                json_string("CYCLE_COMPLETED"),
                json_string("The Cycle has already been completed so no new issues can be added"),
            );
            return Err(Denial::FieldErrors(body));
        }
    }
    let candidates: Vec<IssueCandidate> = coerce_add_issues(issues_value)?;
    // Existing bridges anywhere but this cycle (`~Q(cycle_id)`, no
    // project/workspace scope, default `-created_at` order).
    let wanted: Vec<uuid::Uuid> = candidates
        .iter()
        .filter_map(|candidate| match candidate {
            IssueCandidate::Id(id) => Some(*id),
            IssueCandidate::Null => None,
        })
        .collect();
    let existing: Vec<ExistingBridge> = if wanted.is_empty() {
        Vec::new()
    } else {
        sqlx::query(
            r#"SELECT "id", "issue_id", "cycle_id" FROM "cycle_issues"
               WHERE "cycle_id" <> $1 AND "issue_id" = ANY($2) AND "deleted_at" IS NULL
               ORDER BY "created_at" DESC"#,
        )
        .bind(cycle_id)
        .bind(&wanted)
        .fetch_all(&pre.pool)
        .await
        .map_err(|error| db_error(error, "cycle-issues-add-existing"))?
        .iter()
        .map(|row| {
            Ok(ExistingBridge {
                id: row_uuid(row, "id", "cycle-issues-add-existing")?,
                issue_id: row_uuid(row, "issue_id", "cycle-issues-add-existing")?,
                cycle_id: row_uuid(row, "cycle_id", "cycle-issues-add-existing")?,
            })
        })
        .collect::<Result<Vec<_>, Denial>>()?
    };
    // `existing_issues`: the bridge issue ids whose `str()` is `in` the raw
    // `issues` (Python `==`: strings only — a raw `5` never matches).
    let existing_ids: Vec<String> = existing
        .iter()
        .map(|bridge| bridge.issue_id.to_string())
        .filter(|text| raw_contains(issues_value, text))
        .collect();
    // `new_issues`: the raw values minus the existing strings, deduped in
    // first-seen order (Django's `set()` order is hash-random per process).
    // Dict `issues` iterate into KEYS for the set difference too.
    let raw_items: Vec<Value> = match issues_value {
        Value::Array(items) => items.clone(),
        Value::Object(map) => map.keys().map(|key| Value::String(key.clone())).collect(),
        _ => Vec::new(),
    };
    let mut seen: Vec<&Value> = Vec::new();
    let mut new_values: Vec<&Value> = Vec::new();
    for value in &raw_items {
        if seen.iter().any(|known| json_value_eq(known, value)) {
            continue;
        }
        seen.push(value);
        let dropped = match value {
            Value::String(text) => existing_ids.iter().any(|known| known == text),
            _ => false,
        };
        if !dropped {
            new_values.push(value);
        }
    }
    // `bulk_create(..., batch_size=10, ignore_conflicts=True)`: per-row
    // `pre_save` stamps, NULL audit (no `save()`/CRUM), the URL project id,
    // the CYCLE's workspace. Batches commit independently (a later batch's
    // bad row 500s with earlier batches already in).
    let mut bridges: Vec<CreatedBridge> = Vec::with_capacity(new_values.len());
    for value in &new_values {
        bridges.push(CreatedBridge {
            id: uuid::Uuid::new_v4(),
            created_at: micros_now(),
            updated_at: micros_now(),
            created_by: None,
            updated_by: None,
            project_id,
            workspace_id: cycle_workspace_id,
            cycle_id: *cycle_id,
            issue_id: coerce_new_issue_id(value)?,
            issue_raw: (*value).clone(),
        });
    }
    for batch in bridges.chunks(10) {
        let ids: Vec<uuid::Uuid> = batch.iter().map(|b| b.id).collect();
        let created: Vec<chrono::DateTime<chrono::Utc>> =
            batch.iter().map(|b| b.created_at).collect();
        let updated: Vec<chrono::DateTime<chrono::Utc>> =
            batch.iter().map(|b| b.updated_at).collect();
        // The ORM compiles `issue_id=<raw>` through
        // `UUIDField.get_db_prep_value` → `to_python` at INSERT as well as
        // at the filter, so ints/bools arrive folded (`UUID(int=...)`): a
        // folded miss is an FK 400, a live clash is ignored — never a
        // `uuid_in` 500. NULL binds NULL (not-null → the same 400).
        let issue_ids: Vec<Option<uuid::Uuid>> = batch
            .iter()
            .map(|b| match &b.issue_raw {
                Value::Null => None,
                _ => Some(b.issue_id),
            })
            .collect();
        sqlx::query(
            r#"INSERT INTO "cycle_issues" ("id", "created_at", "updated_at", "created_by_id", "updated_by_id", "deleted_at",
                     "project_id", "workspace_id", "cycle_id", "issue_id")
               SELECT unnest($1::uuid[]), unnest($2::timestamptz[]), unnest($3::timestamptz[]), NULL, NULL, NULL, $4, $5, $6, unnest($7::uuid[])
               ON CONFLICT DO NOTHING"#,
        )
        .bind(&ids)
        .bind(&created)
        .bind(&updated)
        .bind(project_id)
        .bind(cycle_workspace_id)
        .bind(cycle_id)
        .bind(&issue_ids)
        .execute(&pre.pool)
        .await
        .map_err(|error| db_write_error(error, "cycle-issues-add-insert"))?;
    }
    // The move: EVERY fetched bridge re-points at this cycle
    // (`bulk_update(["cycle_id"])` touches only `cycle_id`, not
    // `updated_at`/audit), in `-created_at` fetch order.
    let mut activity: Vec<String> = Vec::with_capacity(existing.len());
    if !existing.is_empty() {
        let move_ids: Vec<uuid::Uuid> = existing.iter().map(|b| b.id).collect();
        sqlx::query(r#"UPDATE "cycle_issues" SET "cycle_id" = $1 WHERE "id" = ANY($2)"#)
            .bind(cycle_id)
            .bind(&move_ids)
            .execute(&pre.pool)
            .await
            .map_err(|error| db_write_error(error, "cycle-issues-add-move"))?;
        for bridge in &existing {
            activity.push(format!(
                "{{\"old_cycle_id\": {}, \"new_cycle_id\": {}, \"issue_id\": {}}}",
                json_string(&bridge.cycle_id.to_string()),
                json_string(&cycle_id.to_string()),
                json_string(&bridge.issue_id.to_string()),
            ));
        }
    }
    // `issue_activity`: `requested_data` echoes the RAW `issues` value under
    // `cycles_list`; `project_id` is the rewritten UUID. Epoch whole
    // seconds, like `int(now().timestamp())`.
    let now = micros_now();
    let requested_text =
        pidash_jobs::tasks_webhooks::activity_dispatch::django_dumps(&serde_json::json!({
            "cycles_list": issues_value,
        }));
    let dump_text = render_bridge_dump(&bridges);
    let current_text =
        pidash_jobs::tasks_webhooks::activity_dispatch::django_dumps(&serde_json::json!({
            "updated_cycle_issues": activity_raw(&activity),
            "created_cycle_issues": dump_text,
        }));
    let job = pidash_jobs::v1_cycles_modules::publish::cycle_issue_added_job(
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
    let sql = pidash_db::v1_cycles_modules::cycle_queries::cycle_issue_queryset_sql(
        &pidash_db::v1_cycles_modules::cycle_queries::OrderBy::default_cycle(),
    );
    let rows: Vec<sqlx::postgres::PgRow> = sqlx::query(&sql)
        .bind(slug)
        .bind(project_id)
        .bind(cycle_id)
        .bind(pre.actor.id)
        .fetch_all(&pre.pool)
        .await
        .map_err(|error| db_error(error, "cycle-issues-add-reread"))?;
    let mut results = Vec::with_capacity(rows.len());
    for row in &rows {
        let detail = BridgeDetail::decode(row, "cycle-issues-add-reread")?
            .with_sub_issues_count(row, "cycle-issues-add-reread")?;
        results
            .push(render_bridge(&pre.pool, state, &detail, &pre.actor.timezone, None, None).await?);
    }
    serde_json::to_string(&Value::Array(results))
        .map(json_ok)
        .map_err(|error| db_error(error, "cycle-issues-add-render"))
}

/// Python `in` of a `str()` bridge id against the raw `issues` value:
/// list elements (type-strict `==`), dict keys, or string substring.
pub fn raw_contains(issues: &Value, text: &str) -> bool {
    match issues {
        Value::Array(items) => items
            .iter()
            .any(|item| matches!(item, Value::String(s) if s == text)),
        Value::Object(map) => map.contains_key(text),
        Value::String(s) => s.contains(text),
        _ => false,
    }
}

/// Python `==` between two raw JSON values (for the `set(issues)`
/// dedupe): `True == 1` and `1 == 1.0` in Python, but JSON ints and
/// floats stay distinct here — the only dedupe that matters in practice
/// is string identity, and Python string equality is exact equality.
pub fn json_value_eq(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Null, Value::Null) => true,
        (Value::Bool(a), Value::Bool(b)) => a == b,
        (Value::Bool(a), Value::Number(n)) | (Value::Number(n), Value::Bool(a)) => {
            let bit = i64::from(*a);
            n.as_i64().is_some_and(|v| v == bit)
                || n.as_u64().is_some_and(|v| v == bit as u64)
                || n.as_f64().is_some_and(|v| v == bit as f64)
        }
        (Value::Number(a), Value::Number(b)) => {
            a.as_i64().zip(b.as_i64()).is_some_and(|(x, y)| x == y)
                || a.as_u64().zip(b.as_u64()).is_some_and(|(x, y)| x == y)
                || a.as_f64().zip(b.as_f64()).is_some_and(|(x, y)| x == y)
        }
        (Value::String(a), Value::String(b)) => a == b,
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| json_value_eq(x, y))
        }
        _ => false,
    }
}

/// The coerced issue id for a new raw value (the filter already validated
/// every value: strings parse, ints/bools fold via `UUID(int=...)`, null
/// stays null — but null never reaches the dump, it fails the insert).
pub fn coerce_new_issue_id(value: &Value) -> Result<uuid::Uuid, Denial> {
    match coerce_issue_candidate(value)? {
        IssueCandidate::Id(id) => Ok(id),
        IssueCandidate::Null => Ok(uuid::Uuid::nil()),
    }
}

/// Python `str()` of a raw id value, for activity payloads that stringify
/// the request value rather than the folded id (`str(new_cycle_id)` in the
/// transfer move entries): strings verbatim, ints/bools in `str()` form
/// (`True`, not `true`).
pub fn raw_id_text(value: &Value) -> String {
    match value {
        Value::Null => "None".to_owned(),
        Value::String(s) => s.clone(),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::Number(_) | Value::Array(_) | Value::Object(_) => py_repr(value),
    }
}

/// Splice pre-rendered activity objects into the `current_instance` JSON
/// (each entry already carries Python separators).
pub fn activity_raw(entries: &[String]) -> Value {
    let text = format!("[{}]", entries.join(", "));
    serde_json::from_str(&text).unwrap_or(Value::Array(Vec::new()))
}

/// `GET .../cycles/<cycle_id>/cycle-issues/<issue_id>/`
/// (`views/cycle.py:1063-1083`): the bridge `.get` (scoped by workspace
/// slug + project id + cycle id + issue id) rendered WITHOUT the
/// `sub_issues_count` annotation (a bare `.get()` carries none), with the
/// request `fields`/`expand`.
pub async fn get_cycle_issue_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    project_raw: &str,
    cycle_id: &uuid::Uuid,
    issue_id: &uuid::Uuid,
    query: &QueryMap,
) -> Result<Response, Denial> {
    let pre = preamble(state, headers, slug).await?;
    let project_id = rewrite_project_id(&pre.pool, slug, project_raw).await?;
    let workspace_id = pre.workspace_id.ok_or(Denial::Forbidden)?;
    require_gate(
        &pre.pool,
        &workspace_id,
        slug,
        &pre.actor.id,
        &project_id,
        "GET",
        PATH_CYCLE_ISSUE_DETAIL,
    )
    .await?;
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(
        r#"SELECT ci."id", ci."created_at", ci."updated_at", ci."deleted_at", ci."created_by_id", ci."updated_by_id",
                  ci."project_id", ci."workspace_id", ci."cycle_id", ci."issue_id" FROM "cycle_issues" ci
           INNER JOIN "workspaces" w ON ci."workspace_id" = w."id"
           WHERE w."slug" = $1 AND ci."project_id" = $2 AND ci."cycle_id" = $3 AND ci."issue_id" = $4 AND ci."deleted_at" IS NULL"#,
    )
    .bind(slug)
    .bind(project_id)
    .bind(cycle_id)
    .bind(issue_id)
    .fetch_optional(&pre.pool)
    .await
    .map_err(|error| db_error(error, "cycle-issue-detail-get"))?;
    let Some(row) = row else {
        return Err(Denial::NotFound);
    };
    // No annotation: the detail GET omits `sub_issues_count` entirely.
    let detail = BridgeDetail::decode(&row, "cycle-issue-detail-get")?;
    let fields = fields_param(query, "fields");
    let expand = fields_param(query, "expand");
    let value = render_bridge(
        &pre.pool,
        state,
        &detail,
        &pre.actor.timezone,
        fields.as_deref(),
        expand.as_deref(),
    )
    .await?;
    serde_json::to_string(&value)
        .map(json_ok)
        .map_err(|error| db_error(error, "cycle-issue-detail-render"))
}

/// `DELETE .../cycles/<cycle_id>/cycle-issues/<issue_id>/`
/// (`views/cycle.py:1084-1136`): bridge `.get` → soft-delete
/// (+ fan-out) → `issue_activity` → 204. The delete never reads the
/// cycle itself, so a missing cycle name cannot 404 here.
pub async fn remove_cycle_issue_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    project_raw: &str,
    cycle_id: &uuid::Uuid,
    issue_id: &uuid::Uuid,
) -> Result<Response, Denial> {
    let pre = preamble(state, headers, slug).await?;
    let project_id = rewrite_project_id(&pre.pool, slug, project_raw).await?;
    let workspace_id = pre.workspace_id.ok_or(Denial::Forbidden)?;
    require_gate(
        &pre.pool,
        &workspace_id,
        slug,
        &pre.actor.id,
        &project_id,
        "DELETE",
        PATH_CYCLE_ISSUE_DETAIL,
    )
    .await?;
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(
        r#"SELECT ci."id", ci."cycle_id", ci."issue_id" FROM "cycle_issues" ci
           INNER JOIN "workspaces" w ON ci."workspace_id" = w."id"
           WHERE w."slug" = $1 AND ci."project_id" = $2 AND ci."cycle_id" = $3 AND ci."issue_id" = $4 AND ci."deleted_at" IS NULL"#,
    )
    .bind(slug)
    .bind(project_id)
    .bind(cycle_id)
    .bind(issue_id)
    .fetch_optional(&pre.pool)
    .await
    .map_err(|error| db_error(error, "cycle-issue-remove-get"))?;
    let Some(row) = row else {
        return Err(Denial::NotFound);
    };
    let bridge_id = row_uuid(&row, "id", "cycle-issue-remove-get")?;
    // The view never touches `.cycle` (`views/cycle.py:1086-1114`): the
    // activity payload carries URL kwargs only, so a missing or
    // soft-deleted cycle row still 204s (verified live).
    // Instance `delete()`: `deleted_at = now()` then `save()` (whose
    // `auto_now` samples again) plus the CRUM `updated_by`.
    let deleted_at = micros_now();
    let updated_at = micros_now();
    sqlx::query(
        r#"UPDATE "cycle_issues" SET "deleted_at" = $1, "updated_at" = $2, "updated_by_id" = $3 WHERE "id" = $4"#,
    )
    .bind(deleted_at)
    .bind(updated_at)
    .bind(pre.actor.id)
    .bind(bridge_id)
    .execute(&pre.pool)
    .await
    .map_err(|error| db_write_error(error, "cycle-issue-remove-soft"))?;
    enqueue_soft_delete(&pre.pool, "cycleissue", &bridge_id).await;
    let job = pidash_jobs::v1_cycles_modules::publish::cycle_issue_removed_job(
        &cycle_id.to_string(),
        &issue_id.to_string(),
        &pre.actor.id.to_string(),
        &project_id.to_string(),
        micros_now().timestamp(),
    );
    enqueue_best_effort(&pre.pool, &job).await;
    Ok(no_content())
}

// ---------------------------------------------------------------------------
// Archive / unarchive
// ---------------------------------------------------------------------------

/// `POST .../cycles/<pk>/archive/` (`views/cycle.py:765-792`): plain
/// `.get` → completed check (a null `end_date` raises `TypeError` → the
/// generic 500, ported) → stamp `archived_at` (+ the `save()` stamps) →
/// favorite queryset soft-delete → 204.
pub async fn archive_cycle_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    project_raw: &str,
    pk: &uuid::Uuid,
) -> Result<Response, Denial> {
    let pre = preamble(state, headers, slug).await?;
    let project_id = rewrite_project_id(&pre.pool, slug, project_raw).await?;
    let workspace_id = pre.workspace_id.ok_or(Denial::Forbidden)?;
    require_gate(
        &pre.pool,
        &workspace_id,
        slug,
        &pre.actor.id,
        &project_id,
        "POST",
        PATH_CYCLE_ARCHIVE,
    )
    .await?;
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(
        r#"SELECT c."id", c."end_date" FROM "cycles" c
           INNER JOIN "workspaces" w ON c."workspace_id" = w."id"
           WHERE w."slug" = $1 AND c."project_id" = $2 AND c."id" = $3 AND c."deleted_at" IS NULL"#,
    )
    .bind(slug)
    .bind(project_id)
    .bind(pk)
    .fetch_optional(&pre.pool)
    .await
    .map_err(|error| db_error(error, "cycle-archive-get"))?;
    let Some(row) = row else {
        return Err(Denial::NotFound);
    };
    // `if cycle.end_date >= timezone.now():` — a null `end_date` raises
    // `TypeError` (None >= datetime), which `handle_exception` does not
    // catch → the generic 500 (`views/cycle.py:770`).
    let end_date = row_datetime_opt(&row, "end_date", "cycle-archive-get")?;
    let Some(end_date) = end_date else {
        return Err(Denial::ServerError);
    };
    if end_date >= micros_now() {
        return Err(Denial::BadError(
            "Only completed cycles can be archived".to_owned(),
        ));
    }
    // `archived_at = now()` then `save()` (whose `auto_now` samples again)
    // plus the CRUM `updated_by`.
    let archived_at = micros_now();
    let updated_at = micros_now();
    sqlx::query(
        r#"UPDATE "cycles" SET "archived_at" = $1, "updated_at" = $2, "updated_by_id" = $3 WHERE "id" = $4"#,
    )
    .bind(archived_at)
    .bind(updated_at)
    .bind(pre.actor.id)
    .bind(pk)
    .execute(&pre.pool)
    .await
    .map_err(|error| db_write_error(error, "cycle-archive-stamp"))?;
    sqlx::query(
        r#"UPDATE "user_favorites" SET "deleted_at" = $1
           WHERE "entity_type" = 'cycle' AND "entity_identifier" = $2 AND "project_id" = $3
             AND "workspace_id" = (SELECT "id" FROM "workspaces" WHERE "slug" = $4)
             AND "deleted_at" IS NULL"#,
    )
    .bind(micros_now())
    .bind(pk)
    .bind(project_id)
    .bind(slug)
    .execute(&pre.pool)
    .await
    .map_err(|error| db_write_error(error, "cycle-archive-favorites"))?;
    Ok(no_content())
}

/// `DELETE .../archived-cycles/<pk>/unarchive/` (`views/cycle.py:794-802`):
/// plain `.get` (no archived check: unarchiving a live cycle still 204s
/// and bumps `updated_at`) → clear `archived_at` (+ the `save()` stamps) →
/// 204.
pub async fn unarchive_cycle_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    project_raw: &str,
    pk: &uuid::Uuid,
) -> Result<Response, Denial> {
    let pre = preamble(state, headers, slug).await?;
    let project_id = rewrite_project_id(&pre.pool, slug, project_raw).await?;
    let workspace_id = pre.workspace_id.ok_or(Denial::Forbidden)?;
    require_gate(
        &pre.pool,
        &workspace_id,
        slug,
        &pre.actor.id,
        &project_id,
        "DELETE",
        PATH_CYCLE_UNARCHIVE,
    )
    .await?;
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(
        r#"SELECT c."id" FROM "cycles" c
           INNER JOIN "workspaces" w ON c."workspace_id" = w."id"
           WHERE w."slug" = $1 AND c."project_id" = $2 AND c."id" = $3 AND c."deleted_at" IS NULL"#,
    )
    .bind(slug)
    .bind(project_id)
    .bind(pk)
    .fetch_optional(&pre.pool)
    .await
    .map_err(|error| db_error(error, "cycle-unarchive-get"))?;
    if row.is_none() {
        return Err(Denial::NotFound);
    }
    let now = micros_now();
    sqlx::query(
        r#"UPDATE "cycles" SET "archived_at" = NULL, "updated_at" = $1, "updated_by_id" = $2 WHERE "id" = $3"#,
    )
    .bind(now)
    .bind(pre.actor.id)
    .bind(pk)
    .execute(&pre.pool)
    .await
    .map_err(|error| db_write_error(error, "cycle-unarchive-stamp"))?;
    Ok(no_content())
}

// ---------------------------------------------------------------------------
// Transfer + burndown
// ---------------------------------------------------------------------------

/// Python number typing for the burndown chart: `sum([])` is int `0` while
/// any non-empty float sum is float (the D-27 `ChartValue` precedent).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ChartNumber {
    Int(i64),
    Float(f64),
}

pub fn chart_add(left: ChartNumber, right: ChartNumber) -> ChartNumber {
    match (left, right) {
        (ChartNumber::Int(left), ChartNumber::Int(right)) => ChartNumber::Int(left + right),
        (ChartNumber::Int(left), ChartNumber::Float(right)) => {
            ChartNumber::Float(left as f64 + right)
        }
        (ChartNumber::Float(left), ChartNumber::Int(right)) => {
            ChartNumber::Float(left + right as f64)
        }
        (ChartNumber::Float(left), ChartNumber::Float(right)) => ChartNumber::Float(left + right),
    }
}

pub fn chart_sub(total: ChartNumber, completed: ChartNumber) -> ChartNumber {
    match (total, completed) {
        (ChartNumber::Int(left), ChartNumber::Int(right)) => ChartNumber::Int(left - right),
        (ChartNumber::Int(left), ChartNumber::Float(right)) => {
            ChartNumber::Float(left as f64 - right)
        }
        (ChartNumber::Float(left), ChartNumber::Int(right)) => {
            ChartNumber::Float(left - right as f64)
        }
        (ChartNumber::Float(left), ChartNumber::Float(right)) => ChartNumber::Float(left - right),
    }
}

pub fn chart_number_value(number: ChartNumber) -> Value {
    match number {
        ChartNumber::Int(value) => Value::from(value),
        ChartNumber::Float(value) => serde_json::json!(value),
    }
}

/// `burndown_plot` (`utils/analytics_plot.py:157-264`, cycle branch):
/// cumulative pending per date over the inclusive UTC date range
/// (`(end.date() - start.date()).days + 1` days; empty when either end is
/// missing). `completed` carries one entry per completion in distribution
/// order (date-ascending, physical ties); `None` dates are uncompleted
/// rows, skipped like Python's `is not None` guard. Future dates (past
/// `today`, the actor-zone day — see the caller) render `null`.
pub fn burndown_chart(
    total: ChartNumber,
    completed: &[(Option<chrono::NaiveDate>, ChartNumber)],
    start: Option<chrono::DateTime<chrono::Utc>>,
    end: Option<chrono::DateTime<chrono::Utc>>,
    today: chrono::NaiveDate,
) -> Value {
    let mut map = serde_json::Map::new();
    let (Some(start), Some(end)) = (start, end) else {
        return Value::Object(map);
    };
    let (start_day, end_day) = (start.date_naive(), end.date_naive());
    let count = (end_day - start_day).num_days() + 1;
    let mut day = start_day;
    for _ in 0..count.max(0) {
        // Python sums left to right from int 0; the accumulator below
        // reproduces the int-until-first-float typing exactly.
        let mut done = ChartNumber::Int(0);
        for (when, value) in completed {
            if let Some(done_day) = when {
                if *done_day <= day {
                    done = chart_add(done, *value);
                }
            }
        }
        let pending = chart_sub(total, done);
        let key = day.to_string();
        if day > today {
            map.insert(key, Value::Null);
        } else {
            map.insert(key, chart_number_value(pending));
        }
        day = day.succ_opt().unwrap_or(day);
    }
    Value::Object(map)
}

/// The burndown scope: `Issue.issue_objects` (live, non-triage,
/// non-archived, project-live, non-draft) bridged live to this cycle in
/// this project/workspace (`analytics_plot.py:171-196`).
pub const BURNDOWN_SCOPE_FROM: &str = r#"FROM "issues" i
     INNER JOIN "cycle_issues" ci ON ci."issue_id" = i."id" AND ci."cycle_id" = $3 AND ci."deleted_at" IS NULL
     INNER JOIN "workspaces" w ON w."id" = i."workspace_id"
     INNER JOIN "projects" p ON p."id" = i."project_id"
     LEFT JOIN "states" st ON st."id" = i."state_id""#;
pub const BURNDOWN_SCOPE_WHERE: &str = r#"i."deleted_at" IS NULL
     AND NOT (st."group" = 'triage' AND st."group" IS NOT NULL)
     AND i."archived_at" IS NULL AND p."archived_at" IS NULL AND NOT i."is_draft"
     AND i."project_id" = $2 AND w."slug" = $1"#;

/// Points distribution: one `(date, value)` per estimated issue, date
/// ascending with physical ties (Django's `.order_by("date")` over the
/// `TruncDate`, which renders in the ACTIVE request zone — the actor's,
/// via `TimezoneMixin`, `datetime.py:27-38`). `CAST(... AS FLOAT)` matches
/// Python's `float(value)` (correctly rounded both sides).
pub async fn fetch_burndown_points(
    pool: &PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    cycle_id: &uuid::Uuid,
    actor_tz: &Tz,
) -> Result<Vec<(Option<chrono::NaiveDate>, ChartNumber)>, Denial> {
    let sql = format!(
        "SELECT (i.\"completed_at\" AT TIME ZONE $4)::date AS d, CAST(ep.\"value\" AS FLOAT) AS v \
         {from} \
         INNER JOIN \"estimate_points\" ep ON ep.\"id\" = i.\"estimate_point_id\" \
         WHERE {scope} ORDER BY d, i.\"ctid\"",
        from = BURNDOWN_SCOPE_FROM,
        scope = BURNDOWN_SCOPE_WHERE,
    );
    let rows: Vec<sqlx::postgres::PgRow> = sqlx::query(&sql)
        .bind(slug)
        .bind(project_id)
        .bind(cycle_id)
        .bind(actor_tz.name())
        .fetch_all(pool)
        .await
        .map_err(|error| db_error(error, "burndown-points"))?;
    let mut out = Vec::with_capacity(rows.len());
    for row in &rows {
        let day: Option<chrono::NaiveDate> = row
            .try_get("d")
            .map_err(|error| db_error(error, "burndown-points"))?;
        let value: Option<f64> = row
            .try_get("v")
            .map_err(|error| db_error(error, "burndown-points"))?;
        if let Some(number) = value {
            out.push((day, ChartNumber::Float(number)));
        }
    }
    Ok(out)
}

/// Points total: `sum(float(v))` in `Issue.Meta.ordering` order
/// (`-created_at`, carried by the `values_list` — `db/models/issue.py`;
/// NOT the D-27 `ctid` precedent, which is app-path-specific). Empty
/// sums to int `0`, exactly like Python's `sum([])`.
pub async fn fetch_burndown_points_total(
    pool: &PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    cycle_id: &uuid::Uuid,
) -> Result<ChartNumber, Denial> {
    let sql = format!(
        "SELECT CAST(ep.\"value\" AS FLOAT) AS v {from} \
         INNER JOIN \"estimate_points\" ep ON ep.\"id\" = i.\"estimate_point_id\" \
         WHERE {scope} ORDER BY i.\"created_at\" DESC",
        from = BURNDOWN_SCOPE_FROM,
        scope = BURNDOWN_SCOPE_WHERE,
    );
    let rows: Vec<sqlx::postgres::PgRow> = sqlx::query(&sql)
        .bind(slug)
        .bind(project_id)
        .bind(cycle_id)
        .fetch_all(pool)
        .await
        .map_err(|error| db_error(error, "burndown-points-total"))?;
    let mut total = ChartNumber::Int(0);
    let mut seen = false;
    for row in &rows {
        let value: Option<f64> = row
            .try_get("v")
            .map_err(|error| db_error(error, "burndown-points-total"))?;
        if let Some(number) = value {
            total = chart_add(total, ChartNumber::Float(number));
            seen = true;
        }
    }
    if seen {
        Ok(total)
    } else {
        Ok(ChartNumber::Int(0))
    }
}

/// Issues distribution: completed counts per actor-zone date (uncompleted
/// rows excluded — Python fetches the null group but skips it in every
/// sum, so the outcome is identical).
pub async fn fetch_burndown_issues(
    pool: &PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    cycle_id: &uuid::Uuid,
    actor_tz: &Tz,
) -> Result<Vec<(Option<chrono::NaiveDate>, ChartNumber)>, Denial> {
    let sql = format!(
        "SELECT (i.\"completed_at\" AT TIME ZONE $4)::date AS d, COUNT(*) AS n {from} \
         WHERE {scope} AND i.\"completed_at\" IS NOT NULL GROUP BY d ORDER BY d",
        from = BURNDOWN_SCOPE_FROM,
        scope = BURNDOWN_SCOPE_WHERE,
    );
    let rows: Vec<sqlx::postgres::PgRow> = sqlx::query(&sql)
        .bind(slug)
        .bind(project_id)
        .bind(cycle_id)
        .bind(actor_tz.name())
        .fetch_all(pool)
        .await
        .map_err(|error| db_error(error, "burndown-issues"))?;
    let mut out = Vec::with_capacity(rows.len());
    for row in &rows {
        let day: Option<chrono::NaiveDate> = row
            .try_get("d")
            .map_err(|error| db_error(error, "burndown-issues"))?;
        let count: i64 = row_i64(row, "n", "burndown-issues")?;
        out.push((day, ChartNumber::Int(count)));
    }
    Ok(out)
}

/// `POST .../cycles/<cycle_id>/transfer-issues/` (view
/// `views/cycle.py:1167-1210` plus `utils/cycle_transfer_issues.py`):
/// new-cycle required, old cycle 404, old-cycle completion gate, target
/// guard (missing target 500s, ended target 400s), source recount
/// (missing 400s), estimate-gated distributions + burndown, snapshot save
/// (no stamps), move the open bridges (`bulk_update(["cycle_id"])`),
/// `issue_activity`, then 200 `{"message": "Success"}`.
pub async fn transfer_cycle_issues_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    project_raw: &str,
    cycle_id: &uuid::Uuid,
    body: &[u8],
) -> Result<Response, Denial> {
    use pidash_db::v1_cycles_modules::cycle_queries as queries;
    use pidash_services::v1_cycles_modules::cycle_queries as shapes;
    let pre = preamble(state, headers, slug).await?;
    let project_id = rewrite_project_id(&pre.pool, slug, project_raw).await?;
    let workspace_id = pre.workspace_id.ok_or(Denial::Forbidden)?;
    require_gate(
        &pre.pool,
        &workspace_id,
        slug,
        &pre.actor.id,
        &project_id,
        "POST",
        PATH_CYCLE_TRANSFER,
    )
    .await?;
    // Raw `request.data` (no serializer): `.get("new_cycle_id", False)`
    // 500s on a non-object body; falsy answers the 400.
    let raw = parse_object_or_500(body)?;
    let new_raw = raw.get("new_cycle_id");
    if !new_raw.is_some_and(py_truthy) {
        return Err(Denial::BadError("New Cycle Id is required".to_owned()));
    }
    let new_raw = new_raw.expect("truthy new_cycle_id checked");
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(
        r#"SELECT c."id", c."end_date" FROM "cycles" c
           INNER JOIN "workspaces" w ON c."workspace_id" = w."id"
           WHERE w."slug" = $1 AND c."project_id" = $2 AND c."id" = $3 AND c."deleted_at" IS NULL"#,
    )
    .bind(slug)
    .bind(project_id)
    .bind(cycle_id)
    .fetch_optional(&pre.pool)
    .await
    .map_err(|error| db_error(error, "transfer-old-get"))?;
    let Some(row) = row else {
        return Err(Denial::NotFound);
    };
    // `if old_cycle.end_date is not None and old_cycle.end_date > now():`
    // a dateless source transfers freely (the archive asymmetry).
    if let Some(end_date) = row_datetime_opt(&row, "end_date", "transfer-old-get")? {
        if end_date > micros_now() {
            return Err(Denial::BadError(
                "The old cycle is not completed yet".to_owned(),
            ));
        }
    }
    // `transfer_cycle_issues`: the target id coerces like a `pk` lookup
    // (strings parse, ints/bools fold via `UUID(int=...)`, everything else
    // 400s through the `ValidationError` branch).
    let new_cycle_id = match coerce_transfer_target(new_raw)? {
        Some(id) => id,
        None => return Err(Denial::ServerError),
    };
    // The move activity stringifies the RAW request value
    // (`str(new_cycle_id)`), not the folded lookup id: an uppercase UUID
    // stays uppercase, an int/bool renders in `str()` form.
    let new_cycle_text = raw_id_text(new_raw);
    // Target lookup + guard (`:59-66`): missing → 500 (ported bug T1),
    // ended → 400, dateless → proceeds.
    let target: Option<sqlx::postgres::PgRow> = sqlx::query(queries::TRANSFER_CYCLE_LOOKUP_SQL)
        .bind(slug)
        .bind(project_id)
        .bind(new_cycle_id)
        .fetch_optional(&pre.pool)
        .await
        .map_err(|error| db_error(error, "transfer-target"))?;
    let facts = shapes::NewCycleFacts {
        exists: target.is_some(),
        end_epoch_micros: match target {
            Some(ref row) => row_datetime_opt(row, "end_date", "transfer-target")?
                .map(|end| end.timestamp_micros()),
            None => None,
        },
    };
    if let Err(block) = shapes::check_new_cycle(&facts, micros_now().timestamp_micros()) {
        return Err(match block.message() {
            Some(message) => Denial::BadError(message.to_owned()),
            None => Denial::ServerError,
        });
    }
    // Source recount (`:68-143`): missing → the 400 (unreachable past the
    // view's `.get`, but ported).
    let recount: Option<sqlx::postgres::PgRow> = sqlx::query(queries::TRANSFER_OLD_CYCLE_SQL)
        .bind(slug)
        .bind(project_id)
        .bind(cycle_id)
        .fetch_optional(&pre.pool)
        .await
        .map_err(|error| db_error(error, "transfer-recount"))?;
    let Some(recount) = recount else {
        return Err(Denial::BadError(
            shapes::TRANSFER_SOURCE_CYCLE_NOT_FOUND_ERROR.to_owned(),
        ));
    };
    let counts = shapes::TransferCounts {
        total: row_i64(&recount, "total_issues", "transfer-recount")?,
        completed: row_i64(&recount, "completed_issues", "transfer-recount")?,
        cancelled: row_i64(&recount, "cancelled_issues", "transfer-recount")?,
        started: row_i64(&recount, "started_issues", "transfer-recount")?,
        unstarted: row_i64(&recount, "unstarted_issues", "transfer-recount")?,
        backlog: row_i64(&recount, "backlog_issues", "transfer-recount")?,
    };
    let start_date = row_datetime_opt(&recount, "start_date", "transfer-recount")?;
    let end_date = row_datetime_opt(&recount, "end_date", "transfer-recount")?;
    // `burndown_plot` compares each chart day against
    // `timezone.now().date()` with the ACTOR zone active
    // (`TimezoneMixin`, `analytics_plot.py:247-257`) — not the UTC day.
    let today = micros_now().with_timezone(&pre.actor.timezone).date_naive();
    // Estimate gate (`:153-159`): the EXISTS the snapshot branch reads.
    let estimate_type: bool = sqlx::query_scalar::<_, i32>(queries::TRANSFER_ESTIMATE_TYPE_SQL)
        .bind(slug)
        .bind(project_id)
        .fetch_optional(&pre.pool)
        .await
        .map_err(|error| db_error(error, "transfer-estimate-gate"))?
        .is_some();
    // Issue distributions (`:338-410`, always loaded).
    let label_issues = fetch_label_issue_rows(
        &pre.pool,
        slug,
        &project_id,
        cycle_id,
        queries::TRANSFER_LABEL_ISSUE_SQL,
    )
    .await?;
    let assignee_issues = fetch_assignee_issue_rows(
        &pre.pool,
        slug,
        &project_id,
        cycle_id,
        queries::TRANSFER_ASSIGNEE_ISSUE_SQL,
    )
    .await?;
    let issues_burndown =
        fetch_burndown_issues(&pre.pool, slug, &project_id, cycle_id, &pre.actor.timezone).await?;
    let completion_chart = burndown_chart(
        ChartNumber::Int(counts.total),
        &issues_burndown,
        start_date,
        end_date,
        today,
    );
    // Estimate branch (`:164-336`, gated): owned vectors that outlive
    // the snapshot build.
    let label_estimates = if estimate_type {
        fetch_label_estimate_rows(
            &pre.pool,
            slug,
            &project_id,
            cycle_id,
            queries::TRANSFER_LABEL_ESTIMATE_SQL,
        )
        .await?
    } else {
        Vec::new()
    };
    let assignee_estimates = if estimate_type {
        fetch_assignee_estimate_rows(
            &pre.pool,
            slug,
            &project_id,
            cycle_id,
            queries::TRANSFER_ASSIGNEE_ESTIMATE_SQL,
        )
        .await?
    } else {
        Vec::new()
    };
    let points_chart = if estimate_type {
        let points_burndown =
            fetch_burndown_points(&pre.pool, slug, &project_id, cycle_id, &pre.actor.timezone)
                .await?;
        let points_total =
            fetch_burndown_points_total(&pre.pool, slug, &project_id, cycle_id).await?;
        burndown_chart(points_total, &points_burndown, start_date, end_date, today)
    } else {
        Value::Object(serde_json::Map::new())
    };
    let estimates = if estimate_type {
        Some(shapes::EstimateBranch {
            label_estimates: &label_estimates,
            assignee_estimates: &assignee_estimates,
            completion_chart: points_chart,
        })
    } else {
        None
    };
    let snapshot = shapes::build_progress_snapshot(&shapes::ProgressSnapshotInput {
        counts,
        label_issues: &label_issues,
        assignee_issues: &assignee_issues,
        completion_chart,
        estimates,
    });
    // `current_cycle.save(update_fields=["progress_snapshot"])`: the column
    // alone — `updated_at`/`updated_by` do NOT move.
    sqlx::query(r#"UPDATE "cycles" SET "progress_snapshot" = $1 WHERE "id" = $2"#)
        .bind(&snapshot)
        .bind(cycle_id)
        .execute(&pre.pool)
        .await
        .map_err(|error| db_write_error(error, "transfer-snapshot"))?;
    // The move (`:437-456`): open-state bridges re-point at the target in
    // `-created_at` select order; `bulk_update(["cycle_id"])` touches only
    // `cycle_id`.
    let moves: Vec<sqlx::postgres::PgRow> = sqlx::query(queries::TRANSFER_MOVE_SELECT_SQL)
        .bind(slug)
        .bind(project_id)
        .bind(cycle_id)
        .fetch_all(&pre.pool)
        .await
        .map_err(|error| db_error(error, "transfer-moves"))?;
    let mut move_ids: Vec<uuid::Uuid> = Vec::with_capacity(moves.len());
    let mut activity: Vec<pidash_jobs::v1_cycles_modules::publish::CycleMove> =
        Vec::with_capacity(moves.len());
    for row in &moves {
        let bridge_id = row_uuid(row, "id", "transfer-moves")?;
        let issue_id = row_uuid(row, "issue_id", "transfer-moves")?;
        move_ids.push(bridge_id);
        activity.push(pidash_jobs::v1_cycles_modules::publish::CycleMove::new(
            &cycle_id.to_string(),
            &new_cycle_text,
            &issue_id.to_string(),
        ));
    }
    if !move_ids.is_empty() {
        sqlx::query(r#"UPDATE "cycle_issues" SET "cycle_id" = $1 WHERE "id" = ANY($2)"#)
            .bind(new_cycle_id)
            .bind(&move_ids)
            .execute(&pre.pool)
            .await
            .map_err(|error| db_write_error(error, "transfer-move-update"))?;
    }
    let job = pidash_jobs::v1_cycles_modules::publish::cycle_issues_transferred_job(
        &pre.actor.id.to_string(),
        &project_id.to_string(),
        &activity,
        micros_now().timestamp(),
        &app_origin(state),
    );
    enqueue_best_effort(&pre.pool, &job).await;
    serde_json::to_string(&serde_json::json!({"message": "Success"}))
        .map(json_ok)
        .map_err(|error| db_error(error, "transfer-render"))
}

/// Coerce the raw `new_cycle_id` like the `pk=new_cycle_id` lookup:
/// strings parse as UUIDs, ints/bools fold via `UUID(int=...)`,
/// everything else 400s through the `ValidationError` branch.
pub fn coerce_transfer_target(value: &Value) -> Result<Option<uuid::Uuid>, Denial> {
    match coerce_issue_candidate(value)? {
        IssueCandidate::Id(id) => Ok(Some(id)),
        IssueCandidate::Null => Err(Denial::BadError(INVALID_DETAIL_BODY_TRIMMED.to_owned())),
    }
}

/// One transfer distribution fetch: label issue rows.
pub async fn fetch_label_issue_rows(
    pool: &PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    cycle_id: &uuid::Uuid,
    sql: &str,
) -> Result<Vec<pidash_services::v1_cycles_modules::cycle_queries::LabelIssueRow>, Denial> {
    use pidash_services::v1_cycles_modules::cycle_queries::LabelIssueRow;
    let rows: Vec<sqlx::postgres::PgRow> = sqlx::query(sql)
        .bind(slug)
        .bind(project_id)
        .bind(cycle_id)
        .fetch_all(pool)
        .await
        .map_err(|error| db_error(error, "transfer-dist"))?;
    let mut out = Vec::with_capacity(rows.len());
    for row in &rows {
        out.push(LabelIssueRow {
            label_name: row_string_opt(row, "label_name", "transfer-dist")?,
            color: row_string_opt(row, "color", "transfer-dist")?,
            label_id: row_uuid_opt(row, "label_id", "transfer-dist")?.map(|id| id.to_string()),
            total_issues: row_i64(row, "total_issues", "transfer-dist")?,
            completed_issues: row_i64(row, "completed_issues", "transfer-dist")?,
            pending_issues: row_i64(row, "pending_issues", "transfer-dist")?,
        });
    }
    Ok(out)
}

/// One transfer distribution fetch: assignee issue rows.
pub async fn fetch_assignee_issue_rows(
    pool: &PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    cycle_id: &uuid::Uuid,
    sql: &str,
) -> Result<Vec<pidash_services::v1_cycles_modules::cycle_queries::AssigneeIssueRow>, Denial> {
    use pidash_services::v1_cycles_modules::cycle_queries::AssigneeIssueRow;
    let rows: Vec<sqlx::postgres::PgRow> = sqlx::query(sql)
        .bind(slug)
        .bind(project_id)
        .bind(cycle_id)
        .fetch_all(pool)
        .await
        .map_err(|error| db_error(error, "transfer-dist"))?;
    let mut out = Vec::with_capacity(rows.len());
    for row in &rows {
        out.push(AssigneeIssueRow {
            display_name: row_string_opt(row, "display_name", "transfer-dist")?,
            assignee_id: row_uuid_opt(row, "assignee_id", "transfer-dist")?
                .map(|id| id.to_string()),
            avatar_url: row_string_opt(row, "avatar_url", "transfer-dist")?,
            total_issues: row_i64(row, "total_issues", "transfer-dist")?,
            completed_issues: row_i64(row, "completed_issues", "transfer-dist")?,
            pending_issues: row_i64(row, "pending_issues", "transfer-dist")?,
        });
    }
    Ok(out)
}

/// One transfer distribution fetch: label estimate rows.
pub async fn fetch_label_estimate_rows(
    pool: &PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    cycle_id: &uuid::Uuid,
    sql: &str,
) -> Result<Vec<pidash_services::v1_cycles_modules::cycle_queries::LabelEstimateRow>, Denial> {
    use pidash_services::v1_cycles_modules::cycle_queries::LabelEstimateRow;
    let rows: Vec<sqlx::postgres::PgRow> = sqlx::query(sql)
        .bind(slug)
        .bind(project_id)
        .bind(cycle_id)
        .fetch_all(pool)
        .await
        .map_err(|error| db_error(error, "transfer-dist"))?;
    let mut out = Vec::with_capacity(rows.len());
    for row in &rows {
        out.push(LabelEstimateRow {
            label_name: row_string_opt(row, "label_name", "transfer-dist")?,
            color: row_string_opt(row, "color", "transfer-dist")?,
            label_id: row_uuid_opt(row, "label_id", "transfer-dist")?.map(|id| id.to_string()),
            total_estimates: row_f64_opt(row, "total_estimates", "transfer-dist")?,
            completed_estimates: row_f64_opt(row, "completed_estimates", "transfer-dist")?,
            pending_estimates: row_f64_opt(row, "pending_estimates", "transfer-dist")?,
        });
    }
    Ok(out)
}

/// One transfer distribution fetch: assignee estimate rows.
pub async fn fetch_assignee_estimate_rows(
    pool: &PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    cycle_id: &uuid::Uuid,
    sql: &str,
) -> Result<Vec<pidash_services::v1_cycles_modules::cycle_queries::AssigneeEstimateRow>, Denial> {
    use pidash_services::v1_cycles_modules::cycle_queries::AssigneeEstimateRow;
    let rows: Vec<sqlx::postgres::PgRow> = sqlx::query(sql)
        .bind(slug)
        .bind(project_id)
        .bind(cycle_id)
        .fetch_all(pool)
        .await
        .map_err(|error| db_error(error, "transfer-dist"))?;
    let mut out = Vec::with_capacity(rows.len());
    for row in &rows {
        out.push(AssigneeEstimateRow {
            display_name: row_string_opt(row, "display_name", "transfer-dist")?,
            assignee_id: row_uuid_opt(row, "assignee_id", "transfer-dist")?
                .map(|id| id.to_string()),
            avatar_url: row_string_opt(row, "avatar_url", "transfer-dist")?,
            total_estimates: row_f64_opt(row, "total_estimates", "transfer-dist")?,
            completed_estimates: row_f64_opt(row, "completed_estimates", "transfer-dist")?,
            pending_estimates: row_f64_opt(row, "pending_estimates", "transfer-dist")?,
        });
    }
    Ok(out)
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

/// `GET .../cycles/`.
pub async fn list_cycles(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((slug, project_id)): Path<(String, String)>,
    Query(query): Query<QueryMap>,
) -> Response {
    match list_cycles_inner(&state, &headers, &slug, &project_id, &query).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

/// `POST .../cycles/`.
pub async fn create_cycle(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((slug, project_id)): Path<(String, String)>,
    body: Bytes,
) -> Response {
    match create_cycle_inner(&state, &headers, &slug, &project_id, &body).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

/// `GET .../cycles/<pk>/`.
pub async fn retrieve_cycle(
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
    match retrieve_cycle_inner(&state, &headers, &slug, &project_id, &pk, &query).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

/// `PATCH .../cycles/<pk>/`.
pub async fn patch_cycle(
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
    match patch_cycle_inner(&state, &headers, &slug, &project_id, &pk, &body).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

/// `DELETE .../cycles/<pk>/`.
pub async fn delete_cycle(
    State(state): State<AppState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    Path((slug, project_id, pk)): Path<(String, String, String)>,
) -> Response {
    let Ok(pk) = path_uuid(&pk) else {
        return proxy_through(state, method, uri, headers, Bytes::new()).await;
    };
    match delete_cycle_inner(&state, &headers, &slug, &project_id, &pk).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

/// `GET .../cycles/<cycle_id>/cycle-issues/`.
pub async fn list_cycle_issues(
    State(state): State<AppState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    Path((slug, project_id, cycle_id)): Path<(String, String, String)>,
    Query(query): Query<QueryMap>,
) -> Response {
    let Ok(cycle_id) = path_uuid(&cycle_id) else {
        return proxy_through(state, method, uri, headers, Bytes::new()).await;
    };
    match list_cycle_issues_inner(&state, &headers, &slug, &project_id, &cycle_id, &query).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

/// `POST .../cycles/<cycle_id>/cycle-issues/`.
pub async fn add_cycle_issues(
    State(state): State<AppState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    Path((slug, project_id, cycle_id)): Path<(String, String, String)>,
    body: Bytes,
) -> Response {
    let Ok(cycle_id) = path_uuid(&cycle_id) else {
        return proxy_through(state, method, uri, headers, body).await;
    };
    match add_cycle_issues_inner(&state, &headers, &slug, &project_id, &cycle_id, &body).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

/// `GET .../cycles/<cycle_id>/cycle-issues/<issue_id>/`.
pub async fn get_cycle_issue(
    State(state): State<AppState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    Path((slug, project_id, cycle_id, issue_id)): Path<(String, String, String, String)>,
    Query(query): Query<QueryMap>,
) -> Response {
    let (Ok(cycle_id), Ok(issue_id)) = (path_uuid(&cycle_id), path_uuid(&issue_id)) else {
        return proxy_through(state, method, uri, headers, Bytes::new()).await;
    };
    match get_cycle_issue_inner(
        &state,
        &headers,
        &slug,
        &project_id,
        &cycle_id,
        &issue_id,
        &query,
    )
    .await
    {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

/// `DELETE .../cycles/<cycle_id>/cycle-issues/<issue_id>/`.
pub async fn remove_cycle_issue(
    State(state): State<AppState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    Path((slug, project_id, cycle_id, issue_id)): Path<(String, String, String, String)>,
) -> Response {
    let (Ok(cycle_id), Ok(issue_id)) = (path_uuid(&cycle_id), path_uuid(&issue_id)) else {
        return proxy_through(state, method, uri, headers, Bytes::new()).await;
    };
    match remove_cycle_issue_inner(&state, &headers, &slug, &project_id, &cycle_id, &issue_id).await
    {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

/// `POST .../cycles/<cycle_id>/transfer-issues/`.
pub async fn transfer_cycle_issues(
    State(state): State<AppState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    Path((slug, project_id, cycle_id)): Path<(String, String, String)>,
    body: Bytes,
) -> Response {
    let Ok(cycle_id) = path_uuid(&cycle_id) else {
        return proxy_through(state, method, uri, headers, body).await;
    };
    match transfer_cycle_issues_inner(&state, &headers, &slug, &project_id, &cycle_id, &body).await
    {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

/// `POST .../cycles/<pk>/archive/`.
pub async fn archive_cycle(
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
    match archive_cycle_inner(&state, &headers, &slug, &project_id, &pk).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

/// `GET .../archived-cycles/`.
pub async fn list_archived_cycles(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((slug, project_id)): Path<(String, String)>,
    Query(query): Query<QueryMap>,
) -> Response {
    match list_archived_cycles_inner(&state, &headers, &slug, &project_id, &query).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

/// `DELETE .../archived-cycles/<pk>/unarchive/`.
pub async fn unarchive_cycle(
    State(state): State<AppState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    Path((slug, project_id, pk)): Path<(String, String, String)>,
) -> Response {
    let Ok(pk) = path_uuid(&pk) else {
        return proxy_through(state, method, uri, headers, Bytes::new()).await;
    };
    match unarchive_cycle_inner(&state, &headers, &slug, &project_id, &pk).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

/// The eight cycle paths (`api/urls/cycle.py:16-57`), each with
/// `owned()` cutover: owned methods serve from Rust, the rest proxy to
/// Django.
pub fn routes() -> axum::Router<AppState> {
    use axum::routing::{delete, get, post};
    axum::Router::new()
        .route(
            "/api/v1/workspaces/{slug}/projects/{project_id}/cycles/",
            owned_list(get(list_cycles).post(create_cycle)),
        )
        .route(
            "/api/v1/workspaces/{slug}/projects/{project_id}/cycles/{pk}/",
            owned_detail(get(retrieve_cycle).patch(patch_cycle).delete(delete_cycle)),
        )
        .route(
            "/api/v1/workspaces/{slug}/projects/{project_id}/cycles/{cycle_id}/cycle-issues/",
            owned_issue_list(get(list_cycle_issues).post(add_cycle_issues)),
        )
        .route(
            "/api/v1/workspaces/{slug}/projects/{project_id}/cycles/{cycle_id}/cycle-issues/{issue_id}/",
            owned_issue_detail(get(get_cycle_issue).delete(remove_cycle_issue)),
        )
        .route(
            "/api/v1/workspaces/{slug}/projects/{project_id}/cycles/{cycle_id}/transfer-issues/",
            owned_transfer(post(transfer_cycle_issues)),
        )
        .route(
            "/api/v1/workspaces/{slug}/projects/{project_id}/cycles/{pk}/archive/",
            owned_archive(post(archive_cycle)),
        )
        .route(
            "/api/v1/workspaces/{slug}/projects/{project_id}/archived-cycles/",
            owned_archived_list(get(list_archived_cycles)),
        )
        .route(
            "/api/v1/workspaces/{slug}/projects/{project_id}/archived-cycles/{pk}/unarchive/",
            owned_unarchive(delete(unarchive_cycle)),
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

    /// FX-CYCMOD-08 route table: the 8 cycle paths, owned methods only
    /// (anonymous callers 401 at the Rust auth layer, never a proxy 502).
    #[tokio::test]
    async fn owned_methods_answer_401_anonymous() {
        let pid = "11111111-1111-1111-1111-111111111111";
        let cid = "22222222-2222-2222-2222-222222222222";
        let iid = "33333333-3333-3333-3333-333333333333";
        let base = |suffix: &str| format!("/api/v1/workspaces/acme/projects/{pid}/{suffix}");
        for (method, uri) in [
            ("GET", base("cycles/")),
            ("POST", base("cycles/")),
            ("GET", base(&format!("cycles/{cid}/"))),
            ("PATCH", base(&format!("cycles/{cid}/"))),
            ("DELETE", base(&format!("cycles/{cid}/"))),
            ("GET", base(&format!("cycles/{cid}/cycle-issues/"))),
            ("POST", base(&format!("cycles/{cid}/cycle-issues/"))),
            ("GET", base(&format!("cycles/{cid}/cycle-issues/{iid}/"))),
            ("DELETE", base(&format!("cycles/{cid}/cycle-issues/{iid}/"))),
            ("POST", base(&format!("cycles/{cid}/transfer-issues/"))),
            ("POST", base(&format!("cycles/{cid}/archive/"))),
            ("GET", base("archived-cycles/")),
            ("DELETE", base(&format!("archived-cycles/{cid}/unarchive/"))),
        ] {
            assert_eq!(
                status(method, &uri).await,
                StatusCode::UNAUTHORIZED,
                "{method} {uri}"
            );
        }
    }

    /// Unowned methods proxy to Django (502 fail-closed with the test
    /// edge): PUT/PATCH anywhere unowned, POST on detail paths, GET on
    /// write-only paths.
    #[tokio::test]
    async fn unowned_methods_proxy() {
        let pid = "11111111-1111-1111-1111-111111111111";
        let cid = "22222222-2222-2222-2222-222222222222";
        let iid = "33333333-3333-3333-3333-333333333333";
        for (method, uri) in [
            (
                "PUT",
                format!("/api/v1/workspaces/acme/projects/{pid}/cycles/"),
            ),
            (
                "POST",
                format!("/api/v1/workspaces/acme/projects/{pid}/cycles/{cid}/"),
            ),
            (
                "PUT",
                format!("/api/v1/workspaces/acme/projects/{pid}/cycles/{cid}/cycle-issues/"),
            ),
            (
                "PATCH",
                format!("/api/v1/workspaces/acme/projects/{pid}/cycles/{cid}/cycle-issues/{iid}/"),
            ),
            (
                "GET",
                format!("/api/v1/workspaces/acme/projects/{pid}/cycles/{cid}/transfer-issues/"),
            ),
            (
                "GET",
                format!("/api/v1/workspaces/acme/projects/{pid}/cycles/{cid}/archive/"),
            ),
            (
                "POST",
                format!("/api/v1/workspaces/acme/projects/{pid}/archived-cycles/"),
            ),
            (
                "GET",
                format!("/api/v1/workspaces/acme/projects/{pid}/archived-cycles/{cid}/unarchive/"),
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
                format!("/api/v1/workspaces/acme/projects/{pid}/cycles/not-a-uuid/"),
            ),
            (
                "PATCH",
                format!("/api/v1/workspaces/acme/projects/{pid}/cycles/not-a-uuid/"),
            ),
            (
                "DELETE",
                format!("/api/v1/workspaces/acme/projects/{pid}/cycles/not-a-uuid/"),
            ),
            (
                "GET",
                format!("/api/v1/workspaces/acme/projects/{pid}/cycles/not-a-uuid/cycle-issues/"),
            ),
            (
                "POST",
                format!("/api/v1/workspaces/acme/projects/{pid}/cycles/not-a-uuid/cycle-issues/"),
            ),
            (
                "GET",
                format!("/api/v1/workspaces/acme/projects/{pid}/cycles/22222222-2222-2222-2222-222222222222/cycle-issues/not-a-uuid/"),
            ),
            (
                "DELETE",
                format!("/api/v1/workspaces/acme/projects/{pid}/cycles/22222222-2222-2222-2222-222222222222/cycle-issues/not-a-uuid/"),
            ),
            (
                "POST",
                format!("/api/v1/workspaces/acme/projects/{pid}/cycles/not-a-uuid/transfer-issues/"),
            ),
            (
                "POST",
                format!("/api/v1/workspaces/acme/projects/{pid}/cycles/not-a-uuid/archive/"),
            ),
            (
                "DELETE",
                format!("/api/v1/workspaces/acme/projects/{pid}/archived-cycles/not-a-uuid/unarchive/"),
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
            Denial::BadError("Work items are required".to_owned()).status_and_body(),
            (
                StatusCode::BAD_REQUEST,
                r#"{"error":"Work items are required"}"#.to_owned()
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

    /// F-N1: quote-breakout payloads stay inside the identifier — the
    /// `pg_sleep` timing probe (200/2.04s) now 500s on the unknown column.
    /// PIDASHCONV-522 renders through sea-query identifiers (which double
    /// `"`), so this pins the final SQL, not the resolver string.
    #[test]
    fn order_by_injection_escaped() {
        use pidash_db::v1_cycles_modules::cycle_queries::cycle_issue_list_get_sql;
        for (raw, term) in [
            ("created_at", r#""issues"."created_at" ASC"#),
            ("-created_at", r#""issues"."created_at" DESC"#),
            (
                "created_at\", \"created_at",
                r#""issues"."created_at"", ""created_at" ASC"#,
            ),
            (
                "state__sequence\", \"sequence",
                r#""states"."sequence"", ""sequence" ASC"#,
            ),
            (
                "created_at\", (SELECT pg_sleep(2))::text--",
                r#""issues"."created_at"", (SELECT pg_sleep(2))::text--" ASC"#,
            ),
        ] {
            let order = resolve_cycle_issue_order(Some(raw));
            let sql = cycle_issue_list_get_sql(&order);
            assert!(sql.contains(term), "{raw}: {sql}");
        }
    }

    /// F-N2: the serializer-free create path 500s on non-object bodies
    /// (`.get` on `request.data`); malformed JSON stays the DRF ParseError.
    #[test]
    fn raw_object_parse_shapes() {
        assert!(parse_object_or_500(b"").expect("empty").is_empty());
        for raw in [b"[]".as_slice(), b"null", b"\"x\"", b"5", b"true"] {
            match parse_object_or_500(raw).expect_err("non-object 500s") {
                Denial::ServerError => {}
                denial => panic!("{raw:?}: expected 500, got {denial:?}"),
            }
        }
        match parse_object_or_500(b"{bad").expect_err("malformed") {
            Denial::BadDetail(message) => assert_eq!(
                message,
                "JSON parse error - Expecting property name enclosed in double quotes: line 1 column 2 (char 1)"
            ),
            denial => panic!("expected BadDetail, got {denial:?}"),
        }
    }

    /// F-N3: the completed gate runs Python `in` on the raw JSON value —
    /// dict keys, list membership, string substring (a hit 500s on the
    /// narrowing `.get`); `in` on null/number/bool raises → 500.
    #[test]
    fn completed_gate_shapes() {
        const GATE: &str = "The Cycle has already been completed so it cannot be edited";
        assert!(completed_gate(&serde_json::json!({"sort_order": 1})).is_ok());
        for value in [
            serde_json::json!({}),
            serde_json::json!({"name": "x"}),
            serde_json::json!(["x"]),
            serde_json::json!([["sort_order"]]),
        ] {
            match completed_gate(&value).expect_err("gate rejects") {
                Denial::BadError(message) => assert_eq!(message, GATE),
                denial => panic!("{value}: expected gate 400, got {denial:?}"),
            }
        }
        for value in [
            serde_json::json!(["sort_order"]),
            serde_json::json!(["sort_order", 0]),
            serde_json::json!("sort_order"),
            serde_json::json!("xxsort_orderxx"),
            serde_json::Value::Null,
            serde_json::json!(5),
            serde_json::json!(5.5),
            serde_json::json!(true),
        ] {
            assert!(
                matches!(completed_gate(&value), Err(Denial::ServerError)),
                "{value}: expected 500"
            );
        }
        match completed_gate(&serde_json::json!("x")).expect_err("miss rejects") {
            Denial::BadError(message) => assert_eq!(message, GATE),
            denial => panic!("expected gate 400, got {denial:?}"),
        }
    }

    /// F-N5: project-midnight UTC parity with pytz `localize(midnight)`
    /// (`is_dst=False` default): folds take the late side, gaps the
    /// pre-transition offset, pytz never raises. Oracle pytz 2026.4 over
    /// every fold/gap midnight 1990-2035 across `common_timezones` (1126
    /// rows, 0 mismatches); this table pins the review cases plus a
    /// stratified sample (the scratch-oracle pytz 2024.1 disagrees on 20
    /// rows where its tzdata predates IANA revisions — Asuncion permanent
    /// -03, Mongolia 1990s — current pytz agrees with the port on all).
    #[test]
    fn project_midnight_pytz_parity() {
        for (zone, day, expected) in [
            (
                "America/Santiago",
                "2023-09-03",
                "2023-09-03T04:00:00+00:00",
            ), // gap
            (
                "America/Scoresbysund",
                "2023-10-29",
                "2023-10-29T01:00:00+00:00",
            ), // fold
            ("Africa/Tunis", "1990-09-30", "1990-09-29T23:00:00+00:00"), // fold
            (
                "America/Goose_Bay",
                "1990-10-28",
                "1990-10-28T04:00:00+00:00",
            ), // fold
            (
                "America/Goose_Bay",
                "1991-10-27",
                "1991-10-27T04:00:00+00:00",
            ), // fold
            (
                "America/Goose_Bay",
                "1992-10-25",
                "1992-10-25T04:00:00+00:00",
            ), // fold
            (
                "America/Goose_Bay",
                "1993-10-31",
                "1993-10-31T04:00:00+00:00",
            ), // fold
            (
                "America/Goose_Bay",
                "1994-10-30",
                "1994-10-30T04:00:00+00:00",
            ), // fold
            (
                "America/Goose_Bay",
                "1995-10-29",
                "1995-10-29T04:00:00+00:00",
            ), // fold
            (
                "America/Goose_Bay",
                "1996-10-27",
                "1996-10-27T04:00:00+00:00",
            ), // fold
            (
                "America/Goose_Bay",
                "1997-10-26",
                "1997-10-26T04:00:00+00:00",
            ), // fold
            ("Africa/Cairo", "1995-04-28", "1995-04-27T22:00:00+00:00"), // gap
            ("Africa/Cairo", "1996-04-26", "1996-04-25T22:00:00+00:00"), // gap
            ("Africa/Cairo", "1997-04-25", "1997-04-24T22:00:00+00:00"), // gap
            ("Africa/Cairo", "1998-04-24", "1998-04-23T22:00:00+00:00"), // gap
            ("Africa/Cairo", "1999-04-30", "1999-04-29T22:00:00+00:00"), // gap
            ("Africa/Cairo", "2000-04-28", "2000-04-27T22:00:00+00:00"), // gap
            ("Africa/Cairo", "2001-04-27", "2001-04-26T22:00:00+00:00"), // gap
            ("Africa/Cairo", "2002-04-26", "2002-04-25T22:00:00+00:00"), // gap
            ("Africa/Cairo", "2003-04-25", "2003-04-24T22:00:00+00:00"), // gap
            ("Pacific/Apia", "2010-09-26", "2010-09-26T11:00:00+00:00"), // gap
            ("Asia/Gaza", "1990-03-25", "1990-03-24T22:00:00+00:00"),    // gap
            ("America/Havana", "1990-04-01", "1990-04-01T05:00:00+00:00"), // gap
            ("Atlantic/Azores", "1990-03-25", "1990-03-25T01:00:00+00:00"), // gap
            ("Africa/Cairo", "2004-04-30", "2004-04-29T22:00:00+00:00"), // gap
            (
                "America/Asuncion",
                "1990-10-01",
                "1990-10-01T04:00:00+00:00",
            ), // gap
            ("Asia/Beirut", "1990-05-01", "1990-04-30T22:00:00+00:00"),  // gap
            ("Asia/Choibalsan", "1990-03-25", "1990-03-24T16:00:00+00:00"), // gap (tzdata-revised Mongolia; new-pytz-verified)
        ] {
            let tz: chrono_tz::Tz = zone.parse().expect("zone");
            let date: chrono::NaiveDate = day.parse().expect("day");
            let got = project_midnight_utc(&tz, &date).expect("never raises");
            assert_eq!(
                got.to_rfc3339_opts(chrono::SecondsFormat::Secs, false),
                expected,
                "{zone} {day}"
            );
        }
    }

    /// F-N7: malformed bodies answer CPython's `json` error texts, not
    /// serde's (fuzzed: 3674 structured/random/UTF-8 cases, 0 mismatches,
    /// 0 reverse-divergences; serde_json 1.0.151 texts — this battery pins
    /// them, so a serde upgrade that rewords errors fails loudly).
    #[test]
    fn json_parse_cpython_parity() {
        for (raw, want) in [
            (b"{bad" as &[u8], "JSON parse error - Expecting property name enclosed in double quotes: line 1 column 2 (char 1)"),
            (b"{]" as &[u8], "JSON parse error - Expecting property name enclosed in double quotes: line 1 column 2 (char 1)"),
            (b"{{}}" as &[u8], "JSON parse error - Expecting property name enclosed in double quotes: line 1 column 2 (char 1)"),
            (b"{'a':1}" as &[u8], "JSON parse error - Expecting property name enclosed in double quotes: line 1 column 2 (char 1)"),
            (b"{\"a\" 1}" as &[u8], "JSON parse error - Expecting ':' delimiter: line 1 column 6 (char 5)"),
            (b"{\"a\":}" as &[u8], "JSON parse error - Expecting value: line 1 column 6 (char 5)"),
            (b"{\"a\":1} trailing" as &[u8], "JSON parse error - Extra data: line 1 column 9 (char 8)"),
            (b"truex" as &[u8], "JSON parse error - Extra data: line 1 column 5 (char 4)"),
            (b"\"a\"b" as &[u8], "JSON parse error - Extra data: line 1 column 4 (char 3)"),
            (b"\"\"x" as &[u8], "JSON parse error - Extra data: line 1 column 3 (char 2)"),
            (b"1 2" as &[u8], "JSON parse error - Extra data: line 1 column 3 (char 2)"),
            (b"{\"a\":1}{\"b\":2}" as &[u8], "JSON parse error - Extra data: line 1 column 8 (char 7)"),
            (b"[1][2]" as &[u8], "JSON parse error - Extra data: line 1 column 4 (char 3)"),
            (b"01 2" as &[u8], "JSON parse error - Extra data: line 1 column 2 (char 1)"),
            (b"[01]" as &[u8], "JSON parse error - Expecting ',' delimiter: line 1 column 3 (char 2)"),
            (b"{\"a\":01}" as &[u8], "JSON parse error - Expecting ',' delimiter: line 1 column 7 (char 6)"),
            (b"01" as &[u8], "JSON parse error - Extra data: line 1 column 2 (char 1)"),
            (b"0x1" as &[u8], "JSON parse error - Extra data: line 1 column 2 (char 1)"),
            (b"{\"a\":0x1}" as &[u8], "JSON parse error - Expecting ',' delimiter: line 1 column 7 (char 6)"),
            (b"-x" as &[u8], "JSON parse error - Expecting value: line 1 column 1 (char 0)"),
            (b"[1e]" as &[u8], "JSON parse error - Expecting ',' delimiter: line 1 column 3 (char 2)"),
            (b"12e" as &[u8], "JSON parse error - Extra data: line 1 column 3 (char 2)"),
            (b"12e+" as &[u8], "JSON parse error - Extra data: line 1 column 3 (char 2)"),
            (b"1e" as &[u8], "JSON parse error - Extra data: line 1 column 2 (char 1)"),
            (b"{\"a\": 1e}" as &[u8], "JSON parse error - Expecting ',' delimiter: line 1 column 8 (char 7)"),
            (b"[12e+]" as &[u8], "JSON parse error - Expecting ',' delimiter: line 1 column 4 (char 3)"),
            (b"[-12e" as &[u8], "JSON parse error - Expecting ',' delimiter: line 1 column 5 (char 4)"),
            (b"[0.5." as &[u8], "JSON parse error - Expecting ',' delimiter: line 1 column 5 (char 4)"),
            (b"[0.5.5]" as &[u8], "JSON parse error - Expecting ',' delimiter: line 1 column 5 (char 4)"),
            (b"[-.5]" as &[u8], "JSON parse error - Expecting value: line 1 column 2 (char 1)"),
            (b"{\"a\":-.5}" as &[u8], "JSON parse error - Expecting value: line 1 column 6 (char 5)"),
            (b"[.5]" as &[u8], "JSON parse error - Expecting value: line 1 column 2 (char 1)"),
            (b"{\"a\":.5}" as &[u8], "JSON parse error - Expecting value: line 1 column 6 (char 5)"),
            (b"." as &[u8], "JSON parse error - Expecting value: line 1 column 1 (char 0)"),
            (b"{\"a\":.}" as &[u8], "JSON parse error - Expecting value: line 1 column 6 (char 5)"),
            (b"+" as &[u8], "JSON parse error - Expecting value: line 1 column 1 (char 0)"),
            (b"{\"a\": +}" as &[u8], "JSON parse error - Expecting value: line 1 column 7 (char 6)"),
            (b"[--1]" as &[u8], "JSON parse error - Expecting value: line 1 column 2 (char 1)"),
            (b"nul" as &[u8], "JSON parse error - Expecting value: line 1 column 1 (char 0)"),
            (b"tru" as &[u8], "JSON parse error - Expecting value: line 1 column 1 (char 0)"),
            (b"nulx" as &[u8], "JSON parse error - Expecting value: line 1 column 1 (char 0)"),
            (b"nullx" as &[u8], "JSON parse error - Extra data: line 1 column 5 (char 4)"),
            (b"[nulx]" as &[u8], "JSON parse error - Expecting value: line 1 column 2 (char 1)"),
            (b"{\"a\":nulx}" as &[u8], "JSON parse error - Expecting value: line 1 column 6 (char 5)"),
            (b"[truex]" as &[u8], "JSON parse error - Expecting ',' delimiter: line 1 column 6 (char 5)"),
            (b"falsey" as &[u8], "JSON parse error - Extra data: line 1 column 6 (char 5)"),
            (b"[falsey]" as &[u8], "JSON parse error - Expecting ',' delimiter: line 1 column 7 (char 6)"),
            (b"nulL" as &[u8], "JSON parse error - Expecting value: line 1 column 1 (char 0)"),
            (b"True" as &[u8], "JSON parse error - Expecting value: line 1 column 1 (char 0)"),
            (b"None" as &[u8], "JSON parse error - Expecting value: line 1 column 1 (char 0)"),
            (b"nulll" as &[u8], "JSON parse error - Extra data: line 1 column 5 (char 4)"),
            (b"true2" as &[u8], "JSON parse error - Extra data: line 1 column 5 (char 4)"),
            (b"[true2]" as &[u8], "JSON parse error - Expecting ',' delimiter: line 1 column 6 (char 5)"),
            (b"{" as &[u8], "JSON parse error - Expecting property name enclosed in double quotes: line 1 column 2 (char 1)"),
            (b"{\"a\"" as &[u8], "JSON parse error - Expecting ':' delimiter: line 1 column 5 (char 4)"),
            (b"{\"a\":" as &[u8], "JSON parse error - Expecting value: line 1 column 6 (char 5)"),
            (b"[1" as &[u8], "JSON parse error - Expecting ',' delimiter: line 1 column 3 (char 2)"),
            (b"{\"a\": \"abc" as &[u8], "JSON parse error - Unterminated string starting at: line 1 column 7 (char 6)"),
            (b"{\"a\": tru" as &[u8], "JSON parse error - Expecting value: line 1 column 7 (char 6)"),
            (b"-" as &[u8], "JSON parse error - Expecting value: line 1 column 1 (char 0)"),
            (b"{\"a\": 12" as &[u8], "JSON parse error - Expecting ',' delimiter: line 1 column 9 (char 8)"),
            (b"{\"a\": -" as &[u8], "JSON parse error - Expecting value: line 1 column 7 (char 6)"),
            (b"[ 2" as &[u8], "JSON parse error - Expecting ',' delimiter: line 1 column 4 (char 3)"),
            (b"[-2" as &[u8], "JSON parse error - Expecting ',' delimiter: line 1 column 4 (char 3)"),
            (b"[1,2" as &[u8], "JSON parse error - Expecting ',' delimiter: line 1 column 5 (char 4)"),
            (b"{\"a\":1," as &[u8], "JSON parse error - Expecting property name enclosed in double quotes: line 1 column 8 (char 7)"),
            (b"{\"a\" :" as &[u8], "JSON parse error - Expecting value: line 1 column 7 (char 6)"),
            (b"[1," as &[u8], "JSON parse error - Expecting value: line 1 column 4 (char 3)"),
            (b"{\"a\":1 " as &[u8], "JSON parse error - Expecting ',' delimiter: line 1 column 8 (char 7)"),
            (b"[1 " as &[u8], "JSON parse error - Expecting ',' delimiter: line 1 column 4 (char 3)"),
            (b"{\"a\":\"b\"" as &[u8], "JSON parse error - Expecting ',' delimiter: line 1 column 9 (char 8)"),
            (b"{\"a\":true" as &[u8], "JSON parse error - Expecting ',' delimiter: line 1 column 10 (char 9)"),
            (b"{\"a\":1.5" as &[u8], "JSON parse error - Expecting ',' delimiter: line 1 column 9 (char 8)"),
            (b"{\"a\":[1]" as &[u8], "JSON parse error - Expecting ',' delimiter: line 1 column 9 (char 8)"),
            (b"{\"a\":1,\"b\":2" as &[u8], "JSON parse error - Expecting ',' delimiter: line 1 column 13 (char 12)"),
            (b"[tru" as &[u8], "JSON parse error - Expecting value: line 1 column 2 (char 1)"),
            (b"{\"a\":1 :" as &[u8], "JSON parse error - Expecting ',' delimiter: line 1 column 8 (char 7)"),
            (b"{\"a\":\"b\" " as &[u8], "JSON parse error - Expecting ',' delimiter: line 1 column 10 (char 9)"),
            (b"[[[" as &[u8], "JSON parse error - Expecting value: line 1 column 4 (char 3)"),
            (b"{\"a\":{\"b\":}" as &[u8], "JSON parse error - Expecting value: line 1 column 11 (char 10)"),
            (b"{\"a\": 1e5x}" as &[u8], "JSON parse error - Expecting ',' delimiter: line 1 column 10 (char 9)"),
            (b"\"abc" as &[u8], "JSON parse error - Unterminated string starting at: line 1 column 1 (char 0)"),
            (b"\"" as &[u8], "JSON parse error - Unterminated string starting at: line 1 column 1 (char 0)"),
            (b"\"\\" as &[u8], "JSON parse error - Unterminated string starting at: line 1 column 1 (char 0)"),
            (b"\"\\u" as &[u8], "JSON parse error - Invalid \\uXXXX escape: line 1 column 3 (char 2)"),
            (b"\"\\u12" as &[u8], "JSON parse error - Invalid \\uXXXX escape: line 1 column 3 (char 2)"),
            (b"\"\\ud800" as &[u8], "JSON parse error - Invalid \\uXXXX escape: line 1 column 3 (char 2)"),
            (b"\"\\u0041" as &[u8], "JSON parse error - Invalid \\uXXXX escape: line 1 column 3 (char 2)"),
            (b"\"\\ud800\\u12\"" as &[u8], "JSON parse error - Invalid \\uXXXX escape: line 1 column 9 (char 8)"),
            (b"{\"a\": \"\\q\"}" as &[u8], "JSON parse error - Invalid \\escape: line 1 column 8 (char 7)"),
            (b"{\"a\": \"\\u12\"}" as &[u8], "JSON parse error - Invalid \\uXXXX escape: line 1 column 9 (char 8)"),
            (b"\"a" as &[u8], "JSON parse error - Unterminated string starting at: line 1 column 1 (char 0)"),
            (b"\"ab" as &[u8], "JSON parse error - Unterminated string starting at: line 1 column 1 (char 0)"),
            (b"\"\\u12345" as &[u8], "JSON parse error - Unterminated string starting at: line 1 column 1 (char 0)"),
            (b"{\"a\": \"x\x01y\"}" as &[u8], "JSON parse error - Invalid control character at: line 1 column 9 (char 8)"),
            (b"\"a\nb\"" as &[u8], "JSON parse error - Invalid control character at: line 1 column 3 (char 2)"),
            (b"\"a\tb\"" as &[u8], "JSON parse error - Invalid control character at: line 1 column 3 (char 2)"),
            (b"[{\"a\":1},]" as &[u8], "JSON parse error - Expecting value: line 1 column 10 (char 9)"),
            (b"{\"a\":1,}" as &[u8], "JSON parse error - Expecting property name enclosed in double quotes: line 1 column 8 (char 7)"),
            (b"{\"a\":[1,}" as &[u8], "JSON parse error - Expecting value: line 1 column 9 (char 8)"),
            (b"[[1,]]" as &[u8], "JSON parse error - Expecting value: line 1 column 5 (char 4)"),
            (b"{\"a\":{\"b\":1,}}" as &[u8], "JSON parse error - Expecting property name enclosed in double quotes: line 1 column 13 (char 12)"),
            (b"[1,]" as &[u8], "JSON parse error - Expecting value: line 1 column 4 (char 3)"),
            (b"{\"a\":,}" as &[u8], "JSON parse error - Expecting value: line 1 column 6 (char 5)"),
            (b"{,}" as &[u8], "JSON parse error - Expecting property name enclosed in double quotes: line 1 column 2 (char 1)"),
            (b"{\n\"a\"\n1\n}" as &[u8], "JSON parse error - Expecting ':' delimiter: line 3 column 1 (char 6)"),
            (b"[\n1\n2\n]" as &[u8], "JSON parse error - Expecting ',' delimiter: line 3 column 1 (char 4)"),
            (b"{\"\xc3\xa9\":}" as &[u8], "JSON parse error - Expecting value: line 1 column 6 (char 5)"),
            (b"\"\xc3\xa9" as &[u8], "JSON parse error - Unterminated string starting at: line 1 column 1 (char 0)"),
            (b"\xef\xbb\xbf{\"a\":1}" as &[u8], "JSON parse error - Unexpected UTF-8 BOM (decode using utf-8-sig): line 1 column 1 (char 0)"),
            (b"\xef\xbb\xbf" as &[u8], "JSON parse error - Unexpected UTF-8 BOM (decode using utf-8-sig): line 1 column 1 (char 0)"),
            (b"[\xef\xbb\xbf]" as &[u8], "JSON parse error - Expecting value: line 1 column 2 (char 1)"),
            (b"{\xef\xbb\xbf\"a\":1}" as &[u8], "JSON parse error - Expecting property name enclosed in double quotes: line 1 column 2 (char 1)"),
            (b"1\xef\xbb\xbf" as &[u8], "JSON parse error - Extra data: line 1 column 2 (char 1)"),
            (b"\xff\xfe" as &[u8], "JSON parse error - 'utf-8' codec can't decode byte 0xff in position 0: invalid start byte"),
            (b"\xc3" as &[u8], "JSON parse error - 'utf-8' codec can't decode byte 0xc3 in position 0: unexpected end of data"),
            (b"\xc3(" as &[u8], "JSON parse error - 'utf-8' codec can't decode byte 0xc3 in position 0: invalid continuation byte"),
            (b"\xe2(\xa1" as &[u8], "JSON parse error - 'utf-8' codec can't decode byte 0xe2 in position 0: invalid continuation byte"),
            (b"\x80" as &[u8], "JSON parse error - 'utf-8' codec can't decode byte 0x80 in position 0: invalid start byte"),
            (b"\xed\xa0\x80" as &[u8], "JSON parse error - 'utf-8' codec can't decode byte 0xed in position 0: invalid continuation byte"),
            (b"\xf0\x9f\x98" as &[u8], "JSON parse error - 'utf-8' codec can't decode bytes in position 0-2: unexpected end of data"),
            (b"\xe2\x82(" as &[u8], "JSON parse error - 'utf-8' codec can't decode bytes in position 0-1: invalid continuation byte"),
            (b"\xe2\x82" as &[u8], "JSON parse error - 'utf-8' codec can't decode bytes in position 0-1: unexpected end of data"),
            (b"\xc0\x80" as &[u8], "JSON parse error - 'utf-8' codec can't decode byte 0xc0 in position 0: invalid start byte"),
            (b"\xed\xbf\xbf" as &[u8], "JSON parse error - 'utf-8' codec can't decode byte 0xed in position 0: invalid continuation byte"),
            (b"\xf4\x90\x80\x80" as &[u8], "JSON parse error - 'utf-8' codec can't decode byte 0xf4 in position 0: invalid continuation byte"),
            (b"\xdf" as &[u8], "JSON parse error - 'utf-8' codec can't decode byte 0xdf in position 0: unexpected end of data"),
            (b"\xef\xbf" as &[u8], "JSON parse error - 'utf-8' codec can't decode bytes in position 0-1: unexpected end of data"),
            (b"{\"a\": \"\xed\xa0\x80\"}" as &[u8], "JSON parse error - 'utf-8' codec can't decode byte 0xed in position 7: invalid continuation byte"),
            (b"\xe0\x80\x80" as &[u8], "JSON parse error - 'utf-8' codec can't decode byte 0xe0 in position 0: invalid continuation byte"),
            (b"a\x80b" as &[u8], "JSON parse error - 'utf-8' codec can't decode byte 0x80 in position 1: invalid start byte"),
            (b"\xc2\xc2" as &[u8], "JSON parse error - 'utf-8' codec can't decode byte 0xc2 in position 0: invalid continuation byte"),
        ] {
            let raw: &[u8] = raw;
            let error = serde_json::from_slice::<Value>(raw).expect_err("battery is errors-only");
            match json_parse_denial(raw, &error) {
                Denial::BadDetail(message) => assert_eq!(message, want, "{raw:?}"),
                denial => panic!("{raw:?}: expected BadDetail, got {denial:?}"),
            }
        }
        // Past serde's 128-deep cap a truncated input still recovers its
        // plain EOF error; a balanced deep input takes the generic 500
        // (CPython accepts it — PIDASHCONV-626, same as surrogates).
        let deep_open = "[".repeat(129);
        let error =
            serde_json::from_slice::<Value>(deep_open.as_bytes()).expect_err("truncated deep");
        match json_parse_denial(deep_open.as_bytes(), &error) {
            Denial::BadDetail(message) => assert_eq!(
                message,
                "JSON parse error - Expecting value: line 1 column 130 (char 129)"
            ),
            denial => panic!("expected EOF recovery, got {denial:?}"),
        }
        let deep_shut = format!("{}1{}", "[".repeat(200), "]".repeat(200));
        let error =
            serde_json::from_slice::<Value>(deep_shut.as_bytes()).expect_err("balanced deep");
        assert!(matches!(
            json_parse_denial(deep_shut.as_bytes(), &error),
            Denial::ServerError
        ));
        // Terminated lone surrogates: CPython ACCEPTS them, so no 400 text
        // is right — the fallback keeps serde text (known gap, pinned).
        let error =
            serde_json::from_slice::<Value>(br#"{"a": "\ud800"}"#).expect_err("lone surrogate");
        match json_parse_denial(br#"{"a": "\ud800"}"#, &error) {
            Denial::BadDetail(message) => assert_eq!(
                message,
                "JSON parse error - unexpected end of hex escape at line 1 column 14"
            ),
            denial => panic!("expected fallback, got {denial:?}"),
        }
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
    fn timezone_membership_is_exact() {
        // The embedded `pytz.common_timezones` (433 entries, oracle dump):
        // exact-match membership, case-sensitive.
        assert_eq!(PYTZ_COMMON_TIMEZONES.len(), 433);
        for zone in ["UTC", "America/New_York", "US/Eastern", "Asia/Kathmandu"] {
            assert!(
                PYTZ_COMMON_TIMEZONES.contains(&zone),
                "{zone} should be a valid choice"
            );
        }
        for zone in ["Etc/UTC", "EST", "utc", "Mars/Olympus"] {
            assert!(
                !PYTZ_COMMON_TIMEZONES.contains(&zone),
                "{zone} should be an invalid choice"
            );
        }
        assert_eq!(
            coerce_choice(
                Some(&Value::String("Etc/UTC".to_owned())),
                PYTZ_COMMON_TIMEZONES
            )
            .expect_err("invalid-choice")
            .body,
            r#"["\"Etc/UTC\" is not a valid choice."]"#
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
        // Empty string is None when nulls are allowed (owned_by), the null
        // error otherwise.
        assert_eq!(
            coerce_pk_value(&Value::String(String::new()), true).expect("owned-by-empty"),
            PkValue::Null
        );
        assert_eq!(
            coerce_pk_value(&Value::String(String::new()), false)
                .expect_err("no-null")
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
    fn datetime_parser_shapes() {
        // Every vector below is probed against Django 4.2
        // `parse_datetime` on Python 3.12 (the oracle).
        let utc = |text: &str| {
            let parsed = parse_iso_datetime(text).expect(text);
            let naive = parsed.naive;
            let offset = parsed.offset_micros.unwrap_or(0);
            naive
                .checked_sub_signed(chrono::Duration::microseconds(offset))
                .map(|shifted| {
                    chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(shifted, chrono::Utc)
                })
        };
        // Canonical inputs.
        assert_eq!(
            utc("2026-10-01T00:00:00Z").expect("z").to_rfc3339(),
            "2026-10-01T00:00:00+00:00"
        );
        assert_eq!(
            utc("2026-10-01 10:30:00")
                .expect("space")
                .format("%Y-%m-%dT%H:%M:%S")
                .to_string(),
            "2026-10-01T10:30:00"
        );
        // Date-only means midnight.
        assert_eq!(
            utc("2026-10-01").expect("date-only").to_rfc3339(),
            "2026-10-01T00:00:00+00:00"
        );
        // Basic + week dates.
        assert_eq!(
            utc("20261001T103000")
                .expect("basic")
                .format("%Y-%m-%dT%H:%M:%S")
                .to_string(),
            "2026-10-01T10:30:00"
        );
        assert_eq!(
            utc("2026-W40-4")
                .expect("week")
                .format("%Y-%m-%d")
                .to_string(),
            "2026-10-01"
        );
        assert_eq!(
            utc("2026W404")
                .expect("week-basic")
                .format("%Y-%m-%d")
                .to_string(),
            "2026-10-01"
        );
        // Bare week means Monday.
        assert_eq!(
            utc("2026-W40")
                .expect("bare-week")
                .format("%Y-%m-%d")
                .to_string(),
            "2026-09-28"
        );
        // Fractions truncate to micros (any length); comma works.
        assert_eq!(
            utc("2026-10-01T10:30:00.12345678901234567890123")
                .expect("long-frac")
                .timestamp_subsec_micros(),
            123456
        );
        assert_eq!(
            utc("2026-10-01T10:30:00,123")
                .expect("comma")
                .timestamp_subsec_micros(),
            123000
        );
        // A fraction after any time unit is sub-second.
        assert_eq!(
            utc("2026-10-01T10:30.5")
                .expect("min-frac")
                .format("%H:%M:%S%.6f")
                .to_string(),
            "10:30:00.500000"
        );
        assert_eq!(
            utc("2026-10-01T10.5")
                .expect("hour-frac")
                .format("%H:%M:%S%.6f")
                .to_string(),
            "10:00:00.500000"
        );
        // Hour-only and minute times.
        assert_eq!(
            utc("2026-10-01T10")
                .expect("hour-only")
                .format("%H:%M:%S")
                .to_string(),
            "10:00:00"
        );
        // Any single char separates date and time, digits included.
        assert!(parse_iso_datetime("2026-10-01X10:30:00").is_some());
        assert!(parse_iso_datetime("2026-10-01t10:30:00Z").is_some());
        assert!(parse_iso_datetime("20261001110:30:00").is_some());
        assert!(parse_iso_datetime("2026-10-01110:30:00").is_some());
        // ... but the char after it must still start a time.
        assert!(parse_iso_datetime("2026-10-0110:30:00").is_none());
        // Skips before the tz depend on the fraction: none → one skip,
        // 1-5 digits → none, 6+ digits → unlimited; never after a marker.
        assert!(parse_iso_datetime("2026-10-01T10:30:00X+05:30").is_some());
        assert!(parse_iso_datetime("2026-10-01T10:30:00.+00:00").is_some());
        assert!(parse_iso_datetime("2026-10-01T10Z").is_some());
        assert!(parse_iso_datetime("2026-10-01T10:30:00Z+00:00").is_none());
        assert!(parse_iso_datetime("2026-10-01T10:30:00XY+05:30").is_none());
        assert!(parse_iso_datetime("2026-10-01T10:30:00+05X+06:00").is_none());
        assert!(parse_iso_datetime("2026-10-01T10:30:00.5X+05:30").is_none());
        assert!(parse_iso_datetime("2026-10-01T10:30:00.00000X+05:30").is_none());
        assert!(parse_iso_datetime("2026-10-01T10:30:00.000000X+05:30").is_some());
        assert!(parse_iso_datetime("2026-10-01T10:30:00.000000XY+05:30").is_some());
        assert!(parse_iso_datetime("2026-10-01T10:30:00.000000XYW+05:30").is_some());
        assert!(parse_iso_datetime("2026-10-01T10:30:00.000000ABCDEFG+05:30").is_some());
        // ... but a Z reached by skipping poisons the parse (Z with junk).
        assert!(parse_iso_datetime("2026-10-01T10:30:00.000000XYZ+05:30").is_none());
        assert!(parse_iso_datetime("2026-10-01T10:30:00.000000QQZ+05:30").is_none());
        assert!(parse_iso_datetime("2026-10-01T10:30:00.000000QQZ").is_some());
        assert!(parse_iso_datetime("2026-10-01T10:30:00.000000X").is_none());
        assert!(parse_iso_datetime("2026-10-01T10:30.000000X+05:30").is_some());
        assert!(parse_iso_datetime("2026-10-01T10.000000X+05:30").is_some());
        assert!(parse_iso_datetime("2026-10-01T103000000000X+05:30").is_some());
        assert!(parse_iso_datetime("2026-10-01T10300000000X+05:30").is_none());
        assert!(parse_iso_datetime("2026-10-01T103000000000Z+05:30").is_none());
        // Trailing NULs are tolerated up to the skip allowance.
        let nul = parse_iso_datetime("2026-10-01T10:30:00\0").expect("nul naive");
        assert_eq!(
            nul.naive.format("%Y-%m-%dT%H:%M:%S").to_string(),
            "2026-10-01T10:30:00"
        );
        assert_eq!(nul.offset_micros, None);
        assert!(parse_iso_datetime("2026-10-01T10:30:00\0\0").is_none());
        assert!(parse_iso_datetime("2026-10-01T10:30:00.5\0").is_none());
        assert!(parse_iso_datetime("2026-10-01T10:30:00.000000\0\0\0").is_some());
        assert!(parse_iso_datetime("2026-10-01T10:30:00.000000Q\0").is_none());
        assert!(parse_iso_datetime("2026-1-1T1:1\0").is_none());
        assert!(parse_iso_datetime("2026-10-01\0").is_none());
        // Basic dotless fraction: 2+ digits; one digit ends the time.
        assert_eq!(
            utc("20261001T103340040")
                .expect("dotless frac")
                .format("%H:%M:%S%.6f")
                .to_string(),
            "10:33:40.040000"
        );
        assert!(parse_iso_datetime("20261001T1033404").is_none());
        // No colon backtrack in fromisoformat — but the Django regex saves
        // one-digit seconds at the parse_datetime layer.
        assert!(parse_fromisoformat("2026-10-01T10:30:0+00:00").is_none());
        assert!(parse_iso_datetime("2026-10-01T10:30:0+00:00").is_some());
        // The weekday backtracks: with-day first, then bare-week Monday.
        let backtrack = parse_iso_datetime("2026W404110Z").expect("week backtrack");
        assert_eq!(
            backtrack.naive.format("%Y-%m-%dT%H:%M:%S").to_string(),
            "2026-10-01T10:00:00"
        );
        assert_eq!(backtrack.offset_micros, Some(0));
        let ext_fallback = parse_iso_datetime("2026-W40-11:30:00").expect("ext fallback");
        assert_eq!(
            ext_fallback.naive.format("%Y-%m-%d").to_string(),
            "2026-09-28"
        );
        assert!(parse_iso_datetime("2026W404X10:30").is_some());
        assert!(parse_iso_datetime("2026-W40-4T10:30").is_some());
        assert!(parse_iso_datetime("2026W40610:30").is_some());
        assert!(parse_iso_datetime("2026-W40610:30:00").is_some());
        // The basic `D` wins only with an even digit count (or an
        // extended time): odd counts go Monday even when fully valid.
        let even_valid = parse_iso_datetime("2026W4041200000+05:30").expect("even valid");
        assert_eq!(
            even_valid.naive.format("%Y-%m-%dT%H:%M:%S").to_string(),
            "2026-10-01T20:00:00"
        );
        let odd_valid = parse_iso_datetime("2026W404103340Z").expect("odd valid");
        assert_eq!(
            odd_valid.naive.format("%Y-%m-%dT%H:%M:%S").to_string(),
            "2026-09-28T10:33:40"
        );
        assert!(parse_iso_datetime("2026W40630000000+05:30").is_none());
        assert!(parse_iso_datetime("2026W404300000Z").is_none());
        assert!(parse_iso_datetime("2026W4043000+05:30").is_none());
        // On an odd count a Monday grammar failure keeps the with-day
        // parse, but a Monday range failure fails hard.
        let odd_kept = parse_iso_datetime("2026W404Z103+05.5").expect("odd kept");
        assert_eq!(
            odd_kept.naive.format("%Y-%m-%dT%H:%M:%S").to_string(),
            "2026-10-01T10:00:00"
        );
        assert_eq!(odd_kept.offset_micros, Some(18_000_500_000));
        assert!(parse_iso_datetime("2026W404 103340040").is_some());
        // ... while an even with-day parse wins outright, whatever
        // Monday does.
        assert!(parse_iso_datetime("2026W4041192359+05:30").is_some());
        assert!(parse_iso_datetime("2026W404Z100000+05:30").is_some());
        // A well-formed but out-of-range with-day rest fails hard —
        // unless the basic time has an odd digit count.
        assert!(parse_iso_datetime("2026W401030:00").is_none());
        assert!(parse_iso_datetime("2026W401030:00+05:30").is_none());
        assert!(parse_iso_datetime("2026W404130000000").is_none());
        assert!(parse_iso_datetime("2026W404110990000").is_none());
        assert!(parse_iso_datetime("2026W404110339900").is_none());
        assert!(parse_iso_datetime("2026W40113334+05:30").is_none());
        assert!(parse_iso_datetime("2026W401130:00.000+05:30").is_none());
        assert!(parse_iso_datetime("2026W401130.00000+05:30").is_none());
        assert!(parse_iso_datetime("2026W40103340040Z").is_some());
        assert!(parse_iso_datetime("2026W40103340040+05:30").is_some());
        assert!(parse_iso_datetime("2026W401133334+05:30").is_some());
        assert!(parse_iso_datetime("2026W4010300000000+05:30").is_some());
        assert!(parse_iso_datetime("2026W4011334.00000+05:30").is_some());
        assert!(parse_iso_datetime("2026-10-01W10:30:00").is_some());
        // ... while grammar failures (even tz-structural ones) fall back.
        let tz_fallback =
            parse_iso_datetime("2026W404103000X+05:30").expect("tz-structural fallback");
        assert_eq!(
            tz_fallback.naive.format("%Y-%m-%dT%H:%M:%S").to_string(),
            "2026-09-28T10:30:00"
        );
        assert!(parse_iso_datetime("2026W40610:30+05:30").is_some());
        assert!(parse_iso_datetime("2026-W40-4110:30:00").is_none());
        let dash_fallback = parse_iso_datetime("2026-W40-20:30:00").expect("dash fallback");
        assert_eq!(
            dash_fallback.naive.format("%Y-%m-%dT%H:%M:%S").to_string(),
            "2026-09-28T20:30:00"
        );
        assert!(parse_iso_datetime("2026W4011:30:00").is_none());
        assert!(parse_iso_datetime("2026W400").is_none());
        // Year zero fails everywhere.
        assert!(parse_iso_datetime("0000-10-01T10:30:00").is_none());
        assert!(parse_iso_datetime("00001001T103000").is_none());
        // Mixed basic/extended times never parse.
        assert!(parse_iso_datetime("2026-10-01T10:3000").is_none());
        assert!(parse_iso_datetime("2026-10-01T1030:00").is_none());
        // Lowercase z never parses; uppercase Z does.
        assert!(parse_iso_datetime("2026-10-01T10:30:00z").is_none());
        // Offsets: hours, hours+minutes, +seconds, fractions (sub-second).
        let off = |text: &str| {
            parse_iso_datetime(&format!("2026-10-01T10:30:00{text}"))
                .expect(text)
                .offset_micros
        };
        assert_eq!(off("Z"), Some(0));
        assert_eq!(off("+05"), Some(5 * 3_600_000_000));
        assert_eq!(off("+0530"), Some((5 * 3600 + 30 * 60) * 1_000_000));
        assert_eq!(off("+05:30"), Some((5 * 3600 + 30 * 60) * 1_000_000));
        assert_eq!(
            off("+05:30:15"),
            Some((5 * 3600 + 30 * 60 + 15) * 1_000_000)
        );
        assert_eq!(off("+05.5"), Some(5 * 3_600_000_000 + 500_000));
        // A zero whole part drops the fraction; the sign covers the rest.
        assert_eq!(off("+00:00:00.5"), Some(0));
        assert_eq!(off("+00.5"), Some(0));
        assert_eq!(off("-00:00:01.5"), Some(-1_500_000));
        // Basic offsets take seconds too; mixed basic/extended never parse.
        assert_eq!(off("+053015"), Some((5 * 3600 + 30 * 60 + 15) * 1_000_000));
        assert!(parse_iso_datetime("2026-10-01T10:30:00+0530:15").is_none());
        assert!(parse_iso_datetime("2026-10-01T10:30:00+05:3015").is_none());
        // Tz components are never range-checked (they normalise); only the
        // ±24h total matters.
        assert_eq!(
            off("+05:30:99"),
            Some((5 * 3600 + 30 * 60 + 99) * 1_000_000)
        );
        assert_eq!(off("+05:99"), Some((5 * 3600 + 99 * 60) * 1_000_000));
        assert_eq!(off("+23:59"), Some((23 * 3600 + 59 * 60) * 1_000_000));
        assert!(parse_iso_datetime("2026-10-01T10:30:00+24:00").is_none());
        assert!(parse_iso_datetime("2026-10-01T10:30:00+99:99").is_none());
        assert!(parse_iso_datetime("2026-10-01T10:30:00+05:3").is_none());
        // Bad calendars fail (Django raises, DRF suppresses → invalid).
        assert!(parse_iso_datetime("2026-10-01T25:00:00").is_none());
        assert!(parse_iso_datetime("2026-13-01T10:00:00").is_none());
        assert!(parse_iso_datetime("2026-02-30T10:00:00").is_none());
        assert!(parse_iso_datetime("2026-10-01T10:30:60").is_none());
        assert!(parse_iso_datetime("2026-10-01T24:00:00").is_none());
        // The Django-regex fallback: non-padded dates and trailing space.
        assert!(parse_iso_datetime("2026-1-1T1:1").is_some());
        assert!(parse_iso_datetime("2026-10-01T10:30:00   ").is_some());
        // Ordinal dates fail both grammars; time-only fails.
        assert!(parse_iso_datetime("2026-275").is_none());
        assert!(parse_iso_datetime("2026275").is_none());
        assert!(parse_iso_datetime("12:30:00").is_none());
        // Trailing dots/commas fail.
        assert!(parse_iso_datetime("2026-10-01T10:30:00.").is_none());
        assert!(parse_iso_datetime("2026-10-01T10:30:00,").is_none());
        assert!(parse_iso_datetime("").is_none());
    }

    #[test]
    fn enforce_user_timezone_edges() {
        use chrono_tz::Tz;
        let utc: Tz = "UTC".parse().expect("utc");
        let eastern: Tz = "America/New_York".parse().expect("eastern");
        // Naive attaches to the actor zone (EST, UTC-5 in January).
        let parsed = parse_iso_datetime("2026-01-15T10:30:00").expect("naive");
        assert_eq!(
            enforce_user_timezone(&parsed, &eastern)
                .expect("enforce")
                .to_rfc3339(),
            "2026-01-15T15:30:00+00:00"
        );
        assert_eq!(
            enforce_user_timezone(&parsed, &utc)
                .expect("enforce-utc")
                .to_rfc3339(),
            "2026-01-15T10:30:00+00:00"
        );
        // Aware shifts through the instant (offset math is exact).
        let parsed = parse_iso_datetime("2026-10-01T00:30:00+05:30").expect("aware");
        assert_eq!(
            enforce_user_timezone(&parsed, &utc)
                .expect("aware")
                .to_rfc3339(),
            "2026-09-30T19:00:00+00:00"
        );
        // Ambiguous folds PASS at fold 0 (the pre-transition offset):
        // 2026-11-01 01:30 in New York is EDT (-4) on first occurrence.
        let parsed = parse_iso_datetime("2026-11-01T01:30:00").expect("fold");
        assert_eq!(
            enforce_user_timezone(&parsed, &eastern)
                .expect("fold-ok")
                .to_rfc3339(),
            "2026-11-01T05:30:00+00:00"
        );
        assert_eq!(
            datetime_make_aware_message(eastern.name()),
            "Invalid datetime for the timezone \"America/New_York\"."
        );
        // Imaginary gap times pass at fold 0 (pre-transition offset):
        // 2026-03-08 02:30 never happened in New York; EST (-5) applies.
        let parsed = parse_iso_datetime("2026-03-08T02:30:00").expect("gap");
        assert_eq!(
            enforce_user_timezone(&parsed, &eastern)
                .expect("gap-ok")
                .to_rfc3339(),
            "2026-03-08T07:30:00+00:00"
        );
        // Aware extremes whose user-zone shift leaves years 1-9999 fail.
        let parsed = parse_iso_datetime("0001-01-01T00:00:00Z").expect("min");
        assert_eq!(
            enforce_user_timezone(&parsed, &eastern).expect_err("overflow"),
            EnforceFail::Overflow
        );
        assert!(enforce_user_timezone(&parsed, &utc).is_ok());
    }

    #[test]
    fn add_issues_coercion_edges() {
        // Truthy scalars 500 (the `__in` iteration raises `TypeError`).
        assert!(matches!(
            coerce_add_issues(&serde_json::json!(5)).expect_err("int"),
            Denial::ServerError
        ));
        assert!(matches!(
            coerce_add_issues(&Value::Bool(true)).expect_err("bool"),
            Denial::ServerError
        ));
        // Bad UUIDs 400.
        assert!(matches!(
            coerce_add_issues(&serde_json::json!(["nope"])).expect_err("bad"),
            Denial::BadError(_)
        ));
        // Null items pass through; dicts iterate keys; strings iterate
        // chars (each fails UUID → 400).
        let out = coerce_add_issues(&Value::Array(vec![Value::Null])).expect("null-item");
        assert!(matches!(out.as_slice(), [IssueCandidate::Null]));
        assert!(coerce_add_issues(&serde_json::json!({"nope": 1})).is_err());
        assert!(coerce_add_issues(&serde_json::json!("abc")).is_err());
        // Ints fold via UUID(int=...), like Django's `to_python`.
        let out = coerce_add_issues(&serde_json::json!([5])).expect("int-item");
        assert!(matches!(
            out.as_slice(),
            [IssueCandidate::Id(id)] if *id == uuid::Uuid::from_u128(5)
        ));
    }

    /// Minimal `DatabaseError` carrying just a SQLSTATE code, for the
    /// write-error mapping test.
    #[derive(Debug)]
    struct CodedDbError {
        code: &'static str,
    }

    impl std::fmt::Display for CodedDbError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "coded {}", self.code)
        }
    }

    impl std::error::Error for CodedDbError {}

    impl sqlx::error::DatabaseError for CodedDbError {
        fn message(&self) -> &str {
            "coded"
        }

        fn code(&self) -> Option<std::borrow::Cow<'_, str>> {
            Some(std::borrow::Cow::Borrowed(self.code))
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
            sqlx::error::ErrorKind::Other
        }
    }

    #[test]
    fn write_error_integrity_mapping() {
        // SQLSTATE class 23 (FK / not-null / unique) answers Django's
        // `IntegrityError` branch: 400 "The payload is not valid".
        for code in ["23503", "23502", "23505", "23514"] {
            let denial = db_write_error(
                sqlx::Error::Database(Box::new(CodedDbError { code })),
                "test",
            );
            assert!(
                matches!(denial, Denial::BadError(message) if message == "The payload is not valid"),
                "code {code}"
            );
        }
        // Anything else (bad casts, driver faults, un-coded errors) stays
        // the generic 500.
        let denial = db_write_error(
            sqlx::Error::Database(Box::new(CodedDbError { code: "22P02" })),
            "test",
        );
        assert!(matches!(denial, Denial::ServerError));
        let denial = db_write_error(sqlx::Error::RowNotFound, "test");
        assert!(matches!(denial, Denial::ServerError));
    }

    #[test]
    fn cycle_issue_order_resolution() {
        use pidash_db::v1_cycles_modules::cycle_queries::{
            M2MOrder, OrderTarget, RelatedOrder, TraversalOrder,
        };
        assert_eq!(resolve_cycle_issue_order(None).column, "created_at");
        assert!(!resolve_cycle_issue_order(None).descending);
        assert_eq!(resolve_cycle_issue_order(None).target, OrderTarget::Base);
        let order = resolve_cycle_issue_order(Some("-created_at"));
        assert!(order.descending);
        assert_eq!(order.column, "created_at");
        assert_eq!(order.target, OrderTarget::Base);
        // PIDASHCONV-522: bare FKs (and the reverse FK `issue_cycle`)
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
            ("issue_cycle", RelatedOrder::IssueCycle),
        ] {
            let order = resolve_cycle_issue_order(Some(name));
            assert_eq!(order.target, OrderTarget::Related(which), "{name}");
            assert_eq!(order.column, name);
            assert!(!order.descending);
            let negated = format!("-{name}");
            let order = resolve_cycle_issue_order(Some(&negated));
            assert_eq!(order.target, OrderTarget::Related(which), "{negated}");
            assert!(order.descending);
        }
        // `type` is the exception: `IssueType` has no `Meta.ordering`,
        // so Django orders by the local `type_id` with no join.
        let order = resolve_cycle_issue_order(Some("type"));
        assert_eq!(order.target, OrderTarget::Base);
        assert_eq!(order.column, "type_id");
        assert!(!order.descending);
        let order = resolve_cycle_issue_order(Some("-type"));
        assert_eq!(order.target, OrderTarget::Base);
        assert_eq!(order.column, "type_id");
        assert!(order.descending);
        // Exact `?` is random; `-?` passes through to 500 exactly like
        // Django's FieldError for it.
        let order = resolve_cycle_issue_order(Some("?"));
        assert_eq!(order.target, OrderTarget::Random);
        assert!(!order.descending);
        let order = resolve_cycle_issue_order(Some("-?"));
        assert_eq!(order.target, OrderTarget::Base);
        assert_eq!(order.column, "?");
        assert!(order.descending);
        // Only the annotations that exist at `.order_by()` time order by
        // alias; the late ones pass through (Django FieldErrors → 500).
        let order = resolve_cycle_issue_order(Some("sub_issues_count"));
        assert_eq!(order.target, OrderTarget::Alias);
        assert_eq!(order.column, "sub_issues_count");
        assert!(!order.descending);
        let order = resolve_cycle_issue_order(Some("-bridge_id"));
        assert_eq!(order.target, OrderTarget::Alias);
        assert!(order.descending);
        let order = resolve_cycle_issue_order(Some("link_count"));
        assert_eq!(order.target, OrderTarget::Base);
        assert_eq!(order.column, "link_count");
        let order = resolve_cycle_issue_order(Some("attachment_count"));
        assert_eq!(order.target, OrderTarget::Base);
        // Bare M2M names carry the request direction (the builder inverts
        // it onto the related `-created_at` ordering).
        let order = resolve_cycle_issue_order(Some("assignees"));
        assert_eq!(order.target, OrderTarget::M2M(M2MOrder::Assignees));
        assert!(!order.descending);
        let order = resolve_cycle_issue_order(Some("-assignees"));
        assert_eq!(order.target, OrderTarget::M2M(M2MOrder::Assignees));
        assert!(order.descending);
        let order = resolve_cycle_issue_order(Some("labels"));
        assert_eq!(order.target, OrderTarget::M2M(M2MOrder::Labels));
        // Single-level traversals onto already-joined tables; the tail
        // passes raw (bad tails 500 at the database like FieldError).
        let order = resolve_cycle_issue_order(Some("state__group"));
        assert_eq!(order.target, OrderTarget::Table("states"));
        assert_eq!(order.column, "group");
        assert!(!order.descending);
        let order = resolve_cycle_issue_order(Some("-state__group"));
        assert_eq!(order.target, OrderTarget::Table("states"));
        assert!(order.descending);
        let order = resolve_cycle_issue_order(Some("parent__created_at"));
        assert_eq!(order.target, OrderTarget::Table("T7"));
        let order = resolve_cycle_issue_order(Some("issue_cycle__id"));
        assert_eq!(order.target, OrderTarget::Table("cycle_issues"));
        let order = resolve_cycle_issue_order(Some("project__name"));
        assert_eq!(order.target, OrderTarget::Table("projects"));
        let order = resolve_cycle_issue_order(Some("workspace__slug"));
        assert_eq!(order.target, OrderTarget::Table("workspaces"));
        let order = resolve_cycle_issue_order(Some("state__nope"));
        assert_eq!(order.target, OrderTarget::Table("states"));
        assert_eq!(order.column, "nope");
        // PIDASHCONV-522: traversals needing a fresh to-one join.
        for (name, tail, which) in [
            ("created_by__email", "email", TraversalOrder::CreatedBy),
            ("updated_by__email", "email", TraversalOrder::UpdatedBy),
            (
                "estimate_point__value",
                "value",
                TraversalOrder::EstimatePoint,
            ),
            ("assigned_pod__name", "name", TraversalOrder::AssignedPod),
        ] {
            let order = resolve_cycle_issue_order(Some(name));
            assert_eq!(order.target, OrderTarget::Traversal(which), "{name}");
            assert_eq!(order.column, tail);
            assert!(!order.descending);
            let negated = format!("-{name}");
            let order = resolve_cycle_issue_order(Some(&negated));
            assert_eq!(order.target, OrderTarget::Traversal(which), "{negated}");
            assert!(order.descending);
        }
        // PIDASHCONV-522: M2M traversals share the bare arm's joins.
        let order = resolve_cycle_issue_order(Some("assignees__email"));
        assert_eq!(order.target, OrderTarget::M2MTraversal(M2MOrder::Assignees));
        assert_eq!(order.column, "email");
        assert!(!order.descending);
        let order = resolve_cycle_issue_order(Some("-labels__name"));
        assert_eq!(order.target, OrderTarget::M2MTraversal(M2MOrder::Labels));
        assert_eq!(order.column, "name");
        assert!(order.descending);
        // Unknown and empty names pass through to the database 500.
        assert_eq!(resolve_cycle_issue_order(Some("bogus")).column, "bogus");
        assert_eq!(
            resolve_cycle_issue_order(Some("bogus")).target,
            OrderTarget::Base
        );
        let order = resolve_cycle_issue_order(Some(""));
        assert_eq!(order.target, OrderTarget::Base);
        assert_eq!(order.column, "");
    }

    #[test]
    fn m2m_envelope_total_counts_distinct() {
        // PIDASHCONV-522: M2M-multiplied rows collapse to distinct base
        // rows in the envelope totals (Django's count trims the
        // ordering-only joins).
        let a = uuid::Uuid::parse_str("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa").expect("uuid");
        let b = uuid::Uuid::parse_str("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb").expect("uuid");
        assert_eq!(count_distinct(vec![a, a, b]), 2);
        assert_eq!(count_distinct(vec![a, b]), 2);
        assert_eq!(count_distinct(vec![]), 0);
    }

    #[test]
    fn compact_binds_edges() {
        // Gaps close in order of first appearance; literals untouched.
        assert_eq!(
            compact_binds("SELECT $1, $2, $4, $5, $4"),
            "SELECT $1, $2, $3, $4, $3"
        );
        assert_eq!(compact_binds("SELECT '$4', $2, $1"), "SELECT '$4', $1, $2");
        assert_eq!(compact_binds("SELECT 1"), "SELECT 1");
        assert_eq!(compact_binds("SELECT $10, $2"), "SELECT $1, $2");
    }

    #[test]
    fn bridge_dump_rendering() {
        assert_eq!(render_bridge_dump(&[]), "[]");
        let id = uuid::Uuid::parse_str("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa").expect("uuid");
        let bridge = CreatedBridge {
            id,
            created_at: chrono::DateTime::parse_from_rfc3339("2026-10-01T07:07:25.057256+00:00")
                .expect("dt")
                .to_utc(),
            updated_at: chrono::DateTime::parse_from_rfc3339("2026-10-01T07:07:25+00:00")
                .expect("dt")
                .to_utc(),
            created_by: None,
            updated_by: None,
            project_id: id,
            workspace_id: id,
            cycle_id: id,
            issue_id: id,
            issue_raw: Value::String(id.to_string()),
        };
        let dump = render_bridge_dump(&[bridge]);
        assert!(dump.contains(r#""model": "db.cycleissue""#), "{dump}");
        // Audit renders null (bulk_create skips save()/CRUM).
        assert!(dump.contains(r#""created_by": null"#), "{dump}");
        assert!(dump.contains(r#""updated_by": null"#), "{dump}");
        // Model field order: issue before cycle.
        let issue_pos = dump.find(r#""issue":"#).expect("issue key");
        let cycle_pos = dump.find(r#""cycle":"#).expect("cycle key");
        assert!(issue_pos < cycle_pos, "{dump}");
        // Zero micros omit the fraction (Django `isoformat`).
        assert!(
            dump.contains(r#""created_at": "2026-10-01T07:07:25.057256+00:00""#),
            "{dump}"
        );
        assert!(
            dump.contains(r#""updated_at": "2026-10-01T07:07:25+00:00""#),
            "{dump}"
        );
    }

    #[test]
    fn bridge_dump_renders_raw_issue_value() {
        // The dump stringifies the RAW request value, not the folded id:
        // protected types pass through as-is, strings verbatim.
        let mk = |raw: Value| CreatedBridge {
            id: uuid::Uuid::nil(),
            created_at: chrono::DateTime::from_timestamp(0, 0)
                .expect("epoch")
                .to_utc(),
            updated_at: chrono::DateTime::from_timestamp(0, 0)
                .expect("epoch")
                .to_utc(),
            created_by: None,
            updated_by: None,
            project_id: uuid::Uuid::nil(),
            workspace_id: uuid::Uuid::nil(),
            cycle_id: uuid::Uuid::nil(),
            issue_id: uuid::Uuid::from_u128(5),
            issue_raw: raw,
        };
        let dump = render_bridge_dump(&[mk(Value::from(5))]);
        assert!(dump.contains(r#""issue": 5"#), "{dump}");
        let dump = render_bridge_dump(&[mk(Value::Bool(true))]);
        assert!(dump.contains(r#""issue": true"#), "{dump}");
        let dump = render_bridge_dump(&[mk(Value::String(
            "AAAAAAAA-AAAA-AAAA-AAAA-AAAAAAAAAAAA".to_owned(),
        ))]);
        assert!(
            dump.contains(r#""issue": "AAAAAAAA-AAAA-AAAA-AAAA-AAAAAAAAAAAA""#),
            "{dump}"
        );
    }

    #[test]
    fn raw_id_text_matches_python_str() {
        assert_eq!(raw_id_text(&Value::from(5)), "5");
        assert_eq!(raw_id_text(&Value::Bool(true)), "True");
        assert_eq!(raw_id_text(&Value::Bool(false)), "False");
        assert_eq!(raw_id_text(&Value::Null), "None");
        assert_eq!(
            raw_id_text(&Value::String("C000-URL".to_owned())),
            "C000-URL"
        );
    }

    #[test]
    fn cycle_read_order_matches_fixture_shape() {
        // FX-CYCMOD-08 handler_shapes: 31 keys in wire order (1 id + 9
        // declared metrics + 15 concrete fields + 6 forward relations).
        assert_eq!(CYCLE_READ_ORDER.len(), 31);
        assert_eq!(
            &CYCLE_READ_ORDER[..11],
            [
                "id",
                "total_issues",
                "cancelled_issues",
                "completed_issues",
                "started_issues",
                "unstarted_issues",
                "backlog_issues",
                "total_estimates",
                "completed_estimates",
                "started_estimates",
                "created_at"
            ]
        );
        assert_eq!(
            &CYCLE_READ_ORDER[25..],
            [
                "version",
                "created_by",
                "updated_by",
                "project",
                "workspace",
                "owned_by"
            ]
        );
    }

    #[test]
    fn python_truthiness_and_raw_membership() {
        assert!(!py_truthy(&Value::Null));
        assert!(!py_truthy(&serde_json::json!("")));
        assert!(!py_truthy(&serde_json::json!(0)));
        assert!(!py_truthy(&serde_json::json!([])));
        assert!(!py_truthy(&serde_json::json!({})));
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
        // `str(issue_id) in issues`: type-strict list membership, dict keys,
        // string substring. A raw `5` never matches its UUID text.
        let id = "00000000-0000-0000-0000-000000000005";
        assert!(raw_contains(&serde_json::json!([id]), id));
        assert!(!raw_contains(&serde_json::json!([5]), id));
        assert!(raw_contains(&serde_json::json!({"a": 1}), "a"));
        // JSON dedupe follows Python equality (True == 1, 1 == 1.0).
        assert!(json_value_eq(&Value::Bool(true), &serde_json::json!(1)));
        assert!(json_value_eq(
            &serde_json::json!(1),
            &serde_json::json!(1.0)
        ));
        assert!(!json_value_eq(
            &serde_json::json!("5"),
            &serde_json::json!(5)
        ));
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

    #[test]
    fn burndown_chart_edges() {
        let day = |text: &str| chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d").expect("date");
        let dt = |text: &str| {
            chrono::DateTime::parse_from_rfc3339(text)
                .expect("dt")
                .to_utc()
        };
        // Issues chart: ints throughout; future dates null.
        let chart = burndown_chart(
            ChartNumber::Int(3),
            &[
                (Some(day("2026-10-02")), ChartNumber::Int(1)),
                (Some(day("2026-10-03")), ChartNumber::Int(2)),
                (None, ChartNumber::Int(99)),
            ],
            Some(dt("2026-10-01T00:00:00Z")),
            Some(dt("2026-10-04T00:00:00Z")),
            day("2026-10-03"),
        );
        assert_eq!(
            serde_json::to_string(&chart).expect("json"),
            r#"{"2026-10-01":3,"2026-10-02":2,"2026-10-03":0,"2026-10-04":null}"#
        );
        // Points chart: int 0 until the first float lands.
        let chart = burndown_chart(
            ChartNumber::Float(5.0),
            &[(Some(day("2026-10-02")), ChartNumber::Float(2.5))],
            Some(dt("2026-10-01T00:00:00Z")),
            Some(dt("2026-10-02T00:00:00Z")),
            day("2026-10-02"),
        );
        assert_eq!(
            serde_json::to_string(&chart).expect("json"),
            r#"{"2026-10-01":5.0,"2026-10-02":2.5}"#
        );
        // Missing ends or reversed ranges render empty.
        assert_eq!(
            burndown_chart(ChartNumber::Int(3), &[], None, None, day("2026-10-03")),
            Value::Object(serde_json::Map::new())
        );
        assert_eq!(
            burndown_chart(
                ChartNumber::Int(3),
                &[],
                Some(dt("2026-10-05T00:00:00Z")),
                Some(dt("2026-10-01T00:00:00Z")),
                day("2026-10-03")
            ),
            Value::Object(serde_json::Map::new())
        );
        // Same-day reversed ends still cover the one day.
        let chart = burndown_chart(
            ChartNumber::Int(3),
            &[],
            Some(dt("2026-10-01T10:00:00Z")),
            Some(dt("2026-10-01T09:00:00Z")),
            day("2026-10-01"),
        );
        assert_eq!(
            serde_json::to_string(&chart).expect("json"),
            r#"{"2026-10-01":3}"#
        );
    }

    #[test]
    fn burndown_today_actor_zone_split() {
        // F-C: the transfer caller derives `today` in the ACTOR zone, not
        // UTC (`timezone.now().date()` under `TimezoneMixin`,
        // `analytics_plot.py:247-257`). At 2026-10-02T00:30:00Z the UTC
        // day is 10-02 while America/New_York still sits on 10-01, and
        // the 10-02 chart entry flips between a number and `null`.
        use chrono_tz::Tz;
        let ny: Tz = "America/New_York".parse().expect("tz");
        let now = chrono::DateTime::parse_from_rfc3339("2026-10-02T00:30:00Z")
            .expect("dt")
            .to_utc();
        let utc_today = now.date_naive();
        let actor_today = now.with_timezone(&ny).date_naive();
        assert_eq!(utc_today.to_string(), "2026-10-02");
        assert_eq!(actor_today.to_string(), "2026-10-01");
        let start = Some(
            chrono::DateTime::parse_from_rfc3339("2026-10-01T00:00:00Z")
                .expect("dt")
                .to_utc(),
        );
        let end = Some(
            chrono::DateTime::parse_from_rfc3339("2026-10-02T00:00:00Z")
                .expect("dt")
                .to_utc(),
        );
        let actor_chart = burndown_chart(ChartNumber::Int(3), &[], start, end, actor_today);
        assert_eq!(
            serde_json::to_string(&actor_chart).expect("json"),
            r#"{"2026-10-01":3,"2026-10-02":null}"#
        );
        let utc_chart = burndown_chart(ChartNumber::Int(3), &[], start, end, utc_today);
        assert_eq!(
            serde_json::to_string(&utc_chart).expect("json"),
            r#"{"2026-10-01":3,"2026-10-02":3}"#
        );
    }

    #[test]
    fn run_validate_edges() {
        use chrono_tz::Tz;
        let utc: Tz = "UTC".parse().expect("utc");
        let actor = uuid::Uuid::parse_str("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa").expect("uuid");
        let project_id =
            uuid::Uuid::parse_str("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb").expect("uuid");
        let project = ProjectRow {
            id: project_id,
            workspace_id: uuid::Uuid::nil(),
            cycle_view: true,
            timezone: Some("UTC".to_owned()),
            identifier: "ENG".to_owned(),
        };
        let dt = |text: &str| {
            chrono::DateTime::parse_from_rfc3339(text)
                .expect("dt")
                .to_utc()
        };
        let now = dt("2026-10-05T12:00:00Z");
        // Both dates rewrite to project-midnight boundaries.
        let validated = run_validate(
            &project_id,
            None,
            None,
            Some(&project),
            Some(dt("2026-10-01T10:00:00Z")),
            Some(dt("2026-10-08T10:00:00Z")),
            None,
            &actor,
            &utc,
            &now,
        )
        .expect("valid");
        assert_eq!(
            validated.start.expect("start").to_rfc3339(),
            "2026-10-01T00:00:01+00:00"
        );
        assert_eq!(
            validated.end.expect("end").to_rfc3339(),
            "2026-10-08T23:59:00+00:00"
        );
        assert_eq!(validated.owned_by, actor);
        // Same-day starts arm to the current instant, micros and all.
        let now = dt("2026-10-01T12:34:56.123456Z");
        let validated = run_validate(
            &project_id,
            None,
            None,
            Some(&project),
            Some(dt("2026-10-01T10:00:00Z")),
            Some(dt("2026-10-08T10:00:00Z")),
            None,
            &actor,
            &utc,
            &now,
        )
        .expect("today");
        assert_eq!(validated.start.expect("today-start"), now);
        // Reversed ends fail even within one second (micros, not secs).
        let err = run_validate(
            &project_id,
            None,
            None,
            Some(&project),
            Some(dt("2026-10-01T00:00:00.500000Z")),
            Some(dt("2026-10-01T00:00:00.100000Z")),
            None,
            &actor,
            &utc,
            &now,
        )
        .expect_err("reversed");
        assert!(matches!(err, Denial::FieldErrors(_)));
        // Lone dates pass through byte-identical.
        let lone = dt("2026-10-01T10:20:30.400000Z");
        let validated = run_validate(
            &project_id,
            None,
            None,
            Some(&project),
            Some(lone),
            None,
            None,
            &actor,
            &utc,
            &now,
        )
        .expect("lone");
        assert_eq!(validated.start.expect("lone-start"), lone);
        assert_eq!(validated.end, None);
        // Disabled views and missing projects gate in order.
        let off = ProjectRow {
            cycle_view: false,
            ..project.clone()
        };
        assert!(matches!(
            run_validate(
                &project_id,
                None,
                None,
                Some(&off),
                None,
                None,
                None,
                &actor,
                &utc,
                &now
            )
            .expect_err("view-off"),
            Denial::FieldErrors(_)
        ));
        assert!(matches!(
            run_validate(
                &project_id,
                None,
                None,
                None,
                None,
                None,
                None,
                &actor,
                &utc,
                &now
            )
            .expect_err("project-missing"),
            Denial::FieldErrors(_)
        ));
        // A provided owner wins over the requester.
        let owner = uuid::Uuid::parse_str("cccccccc-cccc-cccc-cccc-cccccccccccc").expect("uuid");
        let validated = run_validate(
            &project_id,
            None,
            None,
            Some(&project),
            None,
            None,
            Some(owner),
            &actor,
            &utc,
            &now,
        )
        .expect("owner");
        assert_eq!(validated.owned_by, owner);
    }
}
