//! v1 scheduler endpoints: reads (stage 8 drift, PIDASHCONV-1061), the
//! binding / definition writes PDASHOSS01-278 added, and the create /
//! install / uninstall / delete writes of PDASHOSS01-318 ([`writes`]).
//!
//! Ports `apps/api/pi_dash/api/views/scheduler.py`,
//! `api/serializers/scheduler.py` and `api/urls/scheduler.py` (04920ea3,
//! PDASHOSS01-225) onto the merged layers:
//!
//! * `GET workspaces/<slug>/schedulers/`
//!   (`WorkspaceSchedulerListAPIEndpoint`)
//! * `GET workspaces/<slug>/projects/<project_id>/schedulers/`
//!   (`ProjectSchedulerListAPIEndpoint`)
//! * `GET .../schedulers/<uuid:scheduler_id>/`
//!   (`ProjectSchedulerDetailAPIEndpoint`)
//! * `GET .../schedulers/<uuid:scheduler_id>/runs/`
//!   (`ProjectSchedulerRunsAPIEndpoint`)
//!
//! Shape of the port (translate, don't redesign):
//!
//! * Route registration is the cutover granularity (Porting guide cutover
//!   row, `v1_projects` precedent): the owned GET serves Rust, every other
//!   method falls through to [`crate::edge::proxy`] so Django answers the
//!   405s (`Method "POST" not allowed`), OPTIONS metadata and resolver 404s
//!   exactly as before.
//! * Authentication is `APIKeyAuthentication`
//!   (`api/middleware/api_authentication.py`) — the `X-Api-Key` header only;
//!   there is no session auth on these routes. Missing/empty header → 401;
//!   unknown/revoked/expired/inactive token → 403 `Given API token is not
//!   valid`.
//! * Order per request mirrors DRF `initial()` + the view bodies: authN
//!   (401/403), the slug→UUID rewrite (`base.py:51-98`, 404 `Project not
//!   found` on an unresolvable identifier — UUID-looking input passes
//!   through unchecked; anonymous callers skip the rewrite), then
//!   `check_permissions` (403 [`perms::CLASS_DENIAL_BODY`]), then the
//!   `SCHEDULER_ENABLED` flag guard (404
//!   [`crate::app_scheduler::gate::DISABLED_BODY`], checked in every `get()`
//!   body here — unlike the app-surface occurrences quirk), then the
//!   timezone activation, then the body.
//! * A `<uuid:scheduler_id>` segment that is not the canonical
//!   lowercase-hyphenated spelling never reaches the Django view (the
//!   `<uuid:>` converter's regex is that form only), so the detail/runs
//!   handlers proxy it before auth for Django's exact resolver-404 bytes —
//!   non-UUID and parseable-but-non-canonical (uppercase, simple, braced,
//!   URN) alike (the `v1_projects` member-detail precedent; PIDASHCONV-1205).
//!
//! Ported bugs and quirks (translate, don't redesign; also listed in the PR):
//!
//! * The workspace list hides bindings on archived projects
//!   (`project__archived_at__isnull=True`), but the project list/detail do
//!   not — an archived project still lists its schedulers.
//! * `?fields=` / `?expand=` apply to the two list serializers only; the
//!   detail and runs endpoints construct their serializers without them and
//!   ignore both params. `expand=bindings` (or any name outside the base
//!   expansion map) replaces the field with `null` via the `<name>_id`
//!   getattr fallback.
//! * Membership spans (`project__project_projectmember__...`) are plain
//!   joins: no `deleted_at` filter on the spanned rows, exactly as Django
//!   builds them.
//!
//! Ported from `04920ea31d0178b61aa87926e54795634c7cb4ce`.

pub mod handlers;
pub mod perms;
pub mod writes;

use std::collections::HashMap;

use axum::body::Bytes;
use axum::extract::State;
use axum::http::{header, HeaderMap, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use chrono_tz::Tz;
use serde_json::Value;
use sqlx::PgPool;

use crate::state::AppState;

/// Domain router: the four scheduler paths in
/// [`handlers::routes`]. Owned GETs serve from Rust (cutover granularity);
/// everything else keeps proxying to Django through the edge fallback.
pub fn routes() -> axum::Router<AppState> {
    handlers::routes().merge(writes::routes())
}

// ---------------------------------------------------------------------------
// Exact bodies
// ---------------------------------------------------------------------------

/// DRF `NotAuthenticated` (anonymous on a guarded endpoint).
pub const UNAUTHENTICATED_BODY: &str =
    r#"{"detail":"Authentication credentials were not provided."}"#;
/// `APIKeyAuthentication` failure: every token rejection maps here.
pub const INVALID_TOKEN_BODY: &str = r#"{"detail":"Given API token is not valid"}"#;
/// `Project.resolve` miss (`Http404("Project not found")` via DRF, no
/// trailing period).
pub const PROJECT_NOT_FOUND_BODY: &str = r#"{"detail":"Project not found"}"#;
/// `get_object_or_404(Scheduler ...)` miss: Django raises
/// `Http404("No Scheduler matches the given query.")` and DRF propagates
/// the args into the body (`{"detail": ...}`, lowercase key).
pub const SCHEDULER_NOT_FOUND_BODY: &str = r#"{"detail":"No Scheduler matches the given query."}"#;
/// `get_object_or_404(SchedulerBinding ...)` miss (the runs install
/// guard): same shape, binding model name.
pub const BINDING_NOT_FOUND_BODY: &str =
    r#"{"detail":"No SchedulerBinding matches the given query."}"#;
/// `handle_exception`'s `KeyError` branch (unknown `user_timezone`).
pub const MISSING_KEY_BODY: &str = r#"{"error":"The required key does not exist."}"#;
/// `handle_exception`'s `ObjectDoesNotExist` branch.
pub const OBJECT_NOT_FOUND_BODY: &str = r#"{"error":"The requested resource does not exist."}"#;
/// `handle_exception`'s generic 500 branch.
pub const SERVER_ERROR_BODY: &str = r#"{"error":"Something went wrong please try again later"}"#;

/// Handler failure with its exact status + body.
#[derive(Debug)]
pub enum Denial {
    /// 401, DRF `NotAuthenticated` (no `X-Api-Key` header).
    Unauthorized,
    /// 403, invalid/expired/inactive API or machine token.
    InvalidToken,
    /// 403, the DRF-default `PermissionDenied` body (neither scheduler
    /// guard class sets `message`).
    Forbidden,
    /// 404, `{"detail":"Project not found"}` (identifier rewrite miss).
    ProjectNotFound,
    /// 404, `{"detail":"No Scheduler matches the given query."}`
    /// (detail `get_object_or_404` miss).
    SchedulerNotFound,
    /// 404, `{"detail":"No SchedulerBinding matches the given query."}`
    /// (runs install-guard miss).
    BindingNotFound,
    /// 404, the scheduler kill switch
    /// (`crate::app_scheduler::gate::DISABLED_BODY`).
    FeatureDisabled,
    /// 400, `{"detail": ...}` (DRF `ParseError`: pagination).
    BadDetail(String),
    /// 400, `{"error": ...}` (view-inline / `KeyError` branch).
    BadError(String),
    /// 500, generic branch.
    ServerError,
    /// 403, `@allow_permission` refusal
    /// ([`crate::permissions::PERMISSION_DENIED_BODY`]).
    RoleDenied,
    /// 404, `handle_exception`'s `ObjectDoesNotExist` branch.
    ObjectNotFound,
    /// A view-built body with its status (write 400/409/412s).
    Body(StatusCode, Value),
    /// A response the web app's shared write path already rendered.
    App(Box<Response>),
}

impl Denial {
    fn status_and_body(&self) -> (StatusCode, String) {
        match self {
            Denial::Unauthorized => (StatusCode::UNAUTHORIZED, UNAUTHENTICATED_BODY.to_owned()),
            Denial::InvalidToken => (StatusCode::FORBIDDEN, INVALID_TOKEN_BODY.to_owned()),
            Denial::Forbidden => (StatusCode::FORBIDDEN, perms::CLASS_DENIAL_BODY.to_owned()),
            Denial::ProjectNotFound => (StatusCode::NOT_FOUND, PROJECT_NOT_FOUND_BODY.to_owned()),
            Denial::SchedulerNotFound => {
                (StatusCode::NOT_FOUND, SCHEDULER_NOT_FOUND_BODY.to_owned())
            }
            Denial::BindingNotFound => (StatusCode::NOT_FOUND, BINDING_NOT_FOUND_BODY.to_owned()),
            Denial::FeatureDisabled => (
                StatusCode::NOT_FOUND,
                crate::app_scheduler::gate::DISABLED_BODY.to_owned(),
            ),
            Denial::BadDetail(detail) => (
                StatusCode::BAD_REQUEST,
                format!("{{\"detail\":{}}}", json_string(detail)),
            ),
            Denial::BadError(message) => (
                StatusCode::BAD_REQUEST,
                format!("{{\"error\":{}}}", json_string(message)),
            ),
            Denial::ServerError => (
                StatusCode::INTERNAL_SERVER_ERROR,
                SERVER_ERROR_BODY.to_owned(),
            ),
            Denial::RoleDenied => (
                StatusCode::FORBIDDEN,
                crate::permissions::PERMISSION_DENIED_BODY.to_owned(),
            ),
            Denial::ObjectNotFound => (StatusCode::NOT_FOUND, OBJECT_NOT_FOUND_BODY.to_owned()),
            Denial::Body(status, body) => (
                *status,
                serde_json::to_string(body).expect("serializable denial"),
            ),
            // Rendered by the caller; `into_response` returns it as is.
            Denial::App(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                SERVER_ERROR_BODY.to_owned(),
            ),
        }
    }
}

impl IntoResponse for Denial {
    fn into_response(self) -> Response {
        if let Denial::App(response) = self {
            return *response;
        }
        let (status, body) = self.status_and_body();
        Response::builder()
            .status(status)
            .header(header::CONTENT_TYPE, "application/json")
            .body(axum::body::Body::from(body))
            .expect("static denial response")
    }
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).expect("json string")
}

/// 200 with a pre-rendered JSON body (DRF content type; key order is the
/// construction order via `preserve_order`).
pub fn json_ok(body: String) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body))
        .expect("json response")
}

/// 200 JSON from a value (key order is the construction order).
pub fn ok_json(value: Value) -> Response {
    let body = serde_json::to_string(&value).expect("json body");
    json_ok(body)
}

/// Map a database/driver failure to the generic 500 while logging the site
/// and error for operators (no secrets: messages never include tokens).
pub fn db_error<E: std::fmt::Display>(error: E, site: &str) -> Denial {
    tracing::warn!(%error, site, "v1_schedulers database failure");
    Denial::ServerError
}

// ---------------------------------------------------------------------------
// Query params
// ---------------------------------------------------------------------------

/// One query value, repeated or not (the `v1_projects` shape: axum's `Query`
/// backend does not coerce a lone `?key=value` into a sequence, so callers
/// read last like Django's `QueryDict`).
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
// Authentication (`APIKeyAuthentication`)
// ---------------------------------------------------------------------------

/// The authenticated actor: user id plus the RAW stored time-zone name.
/// The zone is NOT parsed here — `TimezoneMixin.initial`
/// (`api/views/base.py:43-48`) calls `super().initial()` (auth +
/// permissions) first and only then activates the zone, so a denying
/// gate answers 403 even when the stored zone is unknown (which 400s
/// only for survivors, via [`activate_timezone`]).
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
    if !raw.starts_with(pidash_auth::token::MACHINE_TOKEN_PREFIX) {
        let _ = sqlx::query(r#"UPDATE "api_tokens" SET "last_used" = now() WHERE "token" = $1"#)
            .bind(raw)
            .execute(pool)
            .await;
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
    let row: Option<(String, bool, Option<chrono::DateTime<chrono::Utc>>, uuid::Uuid)> =
        sqlx::query_as(
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
    // the scheduler permission classes consult it — Django serves a
    // deactivated user's token when membership passes. Port the wart.
    Ok(user_id)
}

/// One `machine_token` + `dev_machine` lookup row: hash, user, dev machine,
/// workspace, token revoke, dev-machine revoke flag.
type MachineTokenLookupRow = (
    String,
    uuid::Uuid,
    Option<uuid::Uuid>,
    uuid::Uuid,
    Option<chrono::DateTime<chrono::Utc>>,
    Option<bool>,
);

/// The machine-token path (`api_authentication.py:47-70`, table
/// `machine_token`): HMAC lookup, revoke checks, dev-machine check, then
/// the workspace-membership check (revoke-then-deny like Python).
async fn resolve_machine_token(
    pool: &PgPool,
    presented: &str,
    secret_key: &[u8],
) -> Result<uuid::Uuid, Denial> {
    let presented_hash = pidash_auth::token::hash_token(presented, secret_key);
    let row: Option<MachineTokenLookupRow> = sqlx::query_as(
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
    // `is_workspace_member(user, workspace_id)`: active row, any role. A
    // non-member is revoked first, then denied — exactly like Python.
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
/// `KeyError` branch (`api/views/base.py:160-164`). An EMPTY zone 500s:
/// `ZoneInfo('')` raises `ValueError` (not `KeyError`), which falls
/// through to the generic 500 (`api/views/base.py:166-171`).
pub fn activate_timezone(timezone: Option<&str>) -> Result<Tz, Denial> {
    match timezone {
        None => Ok(chrono_tz::UTC),
        Some("") => Err(Denial::ServerError),
        Some(zone) => zone
            .parse()
            .map_err(|_| Denial::BadError("The required key does not exist.".to_owned())),
    }
}

// ---------------------------------------------------------------------------
// Preamble + rewrite + gates + flag
// ---------------------------------------------------------------------------

pub fn pool_of(state: &AppState) -> Result<PgPool, Denial> {
    state
        .pools()
        .map(|pools| pools.primary().clone())
        .ok_or(Denial::ServerError)
}

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

/// `_rewrite_project_kwarg` (`api/views/base.py:51-98`): a slug-or-UUID
/// `project_id` becomes the canonical project UUID before permission
/// checks. UUID-looking input passes through unverified (the gate 403s it
/// as before); misses answer the `{"detail":"Project not found"}` 404.
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
            .map_err(|error| db_error(error, "project-resolve"))?
            .ok_or(Denial::ProjectNotFound),
    }
}

/// `WorkspaceEntityPermission` GET (`workspace.py:79-83`): any active
/// workspace membership passes. An unknown slug denies (the slug-scoped
/// `exists()` misses, like Python).
pub async fn require_workspace_entity(
    pool: &PgPool,
    workspace_id: Option<&uuid::Uuid>,
    workspace_slug: &str,
    user_id: &uuid::Uuid,
) -> Result<(), Denial> {
    use pidash_auth::permissions::workspace;
    use pidash_auth::scope::TenantScope;
    let is_member = match workspace_id {
        None => false,
        Some(wid) => {
            sqlx::query_scalar(
                r#"SELECT EXISTS(SELECT 1 FROM "workspace_members" WHERE "workspace_id" = $1 AND "member_id" = $2 AND "is_active" AND "deleted_at" IS NULL)"#,
            )
            .bind(wid)
            .bind(user_id)
            .fetch_optional(pool)
            .await
            .map_err(|error| db_error(error, "ws-member"))?
            .unwrap_or(false)
        }
    };
    let scope = TenantScope::new(pidash_types::WorkspaceId::from(workspace_slug.to_owned()));
    let facts = workspace::WorkspaceFacts {
        workspace: pidash_types::WorkspaceId::from(workspace_slug.to_owned()),
        authenticated: true,
        has_admin_or_member_role: false,
        has_admin_role: false,
        is_member,
        is_admin_unfiltered: false,
    };
    let pf = project_facts_none(workspace_slug);
    if perms::decide(
        perms::gate_for(perms::V1Route::WorkspaceList, "GET"),
        "GET",
        &scope,
        &pf,
        &facts,
    ) {
        Ok(())
    } else {
        Err(Denial::Forbidden)
    }
}

/// `ProjectEntityPermission` GET (`project.py:100-106`): active membership
/// on the rewritten project passes.
pub async fn require_project_entity(
    pool: &PgPool,
    workspace_id: Option<&uuid::Uuid>,
    workspace_slug: &str,
    user_id: &uuid::Uuid,
    project_id: &uuid::Uuid,
    route: perms::V1Route,
) -> Result<(), Denial> {
    use pidash_auth::permissions::{project, workspace};
    use pidash_auth::scope::TenantScope;
    let is_member = match workspace_id {
        None => false,
        Some(wid) => {
            sqlx::query_scalar(
                r#"SELECT EXISTS(SELECT 1 FROM "project_members" WHERE "workspace_id" = $1 AND "member_id" = $2 AND "project_id" = $3 AND "is_active" AND "deleted_at" IS NULL)"#,
            )
            .bind(wid)
            .bind(user_id)
            .bind(project_id)
            .fetch_optional(pool)
            .await
            .map_err(|error| db_error(error, "project-member"))?
            .unwrap_or(false)
        }
    };
    let scope = TenantScope::new(pidash_types::WorkspaceId::from(workspace_slug.to_owned()));
    let pf = project::ProjectFacts {
        workspace: pidash_types::WorkspaceId::from(workspace_slug.to_owned()),
        project_id: pidash_types::ProjectId::from(project_id.to_string()),
        authenticated: true,
        is_workspace_member: false,
        has_workspace_admin_or_member: false,
        is_workspace_admin: false,
        is_project_member: is_member,
        is_project_admin: false,
        has_project_admin_or_member: false,
        has_identifier_membership: false,
        // The scheduler views set no `project_identifier`: the identifier
        // branch is dead here.
        has_project_identifier: false,
    };
    let wf = workspace::WorkspaceFacts {
        workspace: pidash_types::WorkspaceId::from(workspace_slug.to_owned()),
        authenticated: true,
        has_admin_or_member_role: false,
        has_admin_role: false,
        is_member: false,
        is_admin_unfiltered: false,
    };
    if perms::decide(perms::gate_for(route, "GET"), "GET", &scope, &pf, &wf) {
        Ok(())
    } else {
        Err(Denial::Forbidden)
    }
}

fn project_facts_none(workspace_slug: &str) -> pidash_auth::permissions::project::ProjectFacts {
    pidash_auth::permissions::project::ProjectFacts {
        workspace: pidash_types::WorkspaceId::from(workspace_slug.to_owned()),
        project_id: pidash_types::ProjectId::from(uuid::Uuid::nil().to_string()),
        authenticated: true,
        is_workspace_member: false,
        has_workspace_admin_or_member: false,
        is_workspace_admin: false,
        is_project_member: false,
        is_project_admin: false,
        has_project_admin_or_member: false,
        has_identifier_membership: false,
        has_project_identifier: false,
    }
}

/// The `SCHEDULER_ENABLED` kill switch (`views/scheduler.py`, every `get()`
/// body): a disabled instance answers 404
/// [`crate::app_scheduler::gate::DISABLED_BODY`]. Runs after the gate (DRF
/// `initial()` precedes the body), before any object lookup.
pub fn ensure_feature_enabled(settings: &pidash_db::config::Settings) -> Result<(), Denial> {
    if settings.scheduler_enabled {
        Ok(())
    } else {
        Err(Denial::FeatureDisabled)
    }
}

// ---------------------------------------------------------------------------
// Pagination envelope + proxy-out
// ---------------------------------------------------------------------------

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

/// An owned GET-only path: reads serve Rust, everything else falls through
/// to Django (its 405s/OPTIONS live there), following the
/// `v1_projects::owned` precedent.
pub fn owned(
    get_handler: axum::routing::MethodRouter<AppState>,
) -> axum::routing::MethodRouter<AppState> {
    get_handler
        .post(crate::edge::proxy)
        .put(crate::edge::proxy)
        .patch(crate::edge::proxy)
        .delete(crate::edge::proxy)
        .head(crate::edge::proxy)
        .options(crate::edge::proxy)
}

/// An owned GET + POST path (the two list routes, whose POST is a
/// PDASHOSS01-318 write): every other method falls through to Django.
pub fn owned_get_post(
    get_handler: axum::routing::MethodRouter<AppState>,
    post_handler: axum::routing::MethodRouter<AppState>,
) -> axum::routing::MethodRouter<AppState> {
    get_handler
        .merge(post_handler)
        .put(crate::edge::proxy)
        .patch(crate::edge::proxy)
        .delete(crate::edge::proxy)
        .head(crate::edge::proxy)
        .options(crate::edge::proxy)
}

/// Proxy a request Django must answer itself (non-UUID `<uuid:>` segments:
/// the converter rejects them before any view runs).
pub async fn proxy_through(
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

pub fn into_result_response(result: Result<Response, Denial>) -> Response {
    match result {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}
