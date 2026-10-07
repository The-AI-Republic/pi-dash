//! Orchestration action handlers (D-18 handlers F, PIDASHCONV-678).
//!
//! Ports `apps/api/pi_dash/api/views/issue.py` (4 units):
//!
//! * `IssueReTickAPIEndpoint.post` (`:940-986`) — re-grant budget and start a
//!   run via `scheduling.re_tick_ticker`; 200 `granted` envelope.
//! * `IssueWaitAPIEndpoint.post` (`:987-1069`) — buy back one tick via
//!   `scheduling.wait_ticker`; always 200 (`applied` + `reason`, ticker keys
//!   only when a row exists).
//! * `IssueRunAiAPIEndpoint.post` (`:1175-1254`) — dispatch a run via
//!   `scheduling.run_ai_for_human`; 201 with `{id, status, executor}` or 409
//!   with `{error, reason}`, 403 from inside an agent run.
//! * `AgentRunYieldAPIEndpoint.post` (`:1255-1313`) — record the run outcome
//!   (`RUN_OUTCOMES` / `normalize_outcome` + `done_payload` write).
//!
//! Registered by [`super::routes`] at the four
//! `apps/api/pi_dash/api/urls/work_item.py:133-151` paths.
//!
//! Layering (all foundation use is read-only): dispatch semantics come from
//! the merged D-12 drivers — the services control flow plus the jobs
//! transaction drivers (`pidash_jobs::orchestration::{re_tick_ticker,
//! run_ai_for_human, wait_ticker}`), outcomes from
//! `pidash_types::orchestration`, agent guards from [`super::perms`], gates
//! from the F-06 kernel. This module owns the HTTP shell: API-key auth, the
//! slug→UUID rewrite, permission wiring, the live seam inputs the drivers
//! need, and the DRF byte rendering.
//!
//! Request order (preserved, not redesigned): UUID-segment shape (proxy when
//! Django's `<uuid:>` converter would not match — before auth, as URL
//! resolving precedes it), API-key authentication, the slug→UUID rewrite
//! (`api/views/base.py:51-98`, skipped for anonymous callers), gate check,
//! then the handler body (`:954-984`, `:1033-1064`, `:1222-1252`,
//! `:1268-1311`).
//!
//! Live seam inputs for the drivers:
//!
//! * `PodRunnerMatcher` is implemented live over the merged D-14 guards
//!   helpers (`eligible_owner_ids`, `POD_HAS_RUNNER_MANAGED_SQL`,
//!   `pod_has_runner_general_sql`, `ISSUE_ASSIGNEE_IDS_SQL`).
//! * `has_usable_llm_config` / `managed_llm_profile` close over LLM-key
//!   presence pre-fetched for the actor plus every fallback-creator
//!   candidate (issue creator, project lead, default assignee, live
//!   assignees) — the only users the drivers ever query — decided through
//!   the `assistant::seams` pure functions over the
//!   `assistant_user_llm_config` row.
//! * `extra_toolsets_enabled` is `false`, exactly as CE
//!   (`ee/cloud_agent/toolsets.py:23-32`).
//! * The admission cache is request-scoped memory: the sync
//!   `AdmissionCache` trait cannot reach the async Redis client, and it is
//!   consulted only when building a cloud-executor run, so every CE/local
//!   path answers identically to Django; only concurrent cloud load could
//!   over-admit versus the shared cache.
//!
//! Ported bugs: none found in these four units on read-through (the
//! agent-guard ownership bug is P1's, listed on PIDASHCONV-671).
//!
//! Deliberate edges (both unpinned — no fixture or contract case sends them):
//!
//! * A non-object JSON body on yield answers the generic 500: Python calls
//!   `.get` on the parsed list and raises `AttributeError` into the base
//!   500 path.
//! * Datetime rendering uses the `to_rfc3339_opts(AutoSi, false)` isoformat
//!   port (the `app_integrations` precedent): identical to
//!   `datetime.isoformat()` except microseconds with trailing zeros, which
//!   CPython pads and `AutoSi` trims.
//!
//! Fixture: `F18-11` (`rust-api/fixtures/v1_work_items/handlers/` —
//! `retick`, `wait`, `run_ai`, `yield_*` calls).
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use axum::extract::{Path, Request, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use serde_json::Value;
use sqlx::{PgPool, Row};
use uuid::Uuid;

use pidash_auth::permissions::project;
use pidash_auth::scope::TenantScope;
use pidash_types::orchestration::{normalize_outcome, RUN_OUTCOMES};
use pidash_types::runner_runs::AgentRunStatus;

use crate::state::AppState;

use super::perms::{
    gate_for, refuse_agent_action, refuse_agent_retick, resolve_moved_by_run, run_belongs_to,
    RunFacts, V1WorkItemsRoute,
};

// ---------------------------------------------------------------------------
// Responses
// ---------------------------------------------------------------------------

/// Exact bytes of the DRF `IsAuthenticated` denial (anonymous on a guarded
/// endpoint).
pub const UNAUTHENTICATED_BODY: &str =
    r#"{"detail":"Authentication credentials were not provided."}"#;
/// `APIKeyAuthentication` failure (`api/middleware/api_authentication.py`):
/// every token rejection maps to this single 403 body.
pub const INVALID_TOKEN_BODY: &str = r#"{"detail":"Given API token is not valid"}"#;
/// `handle_exception`'s generic branch (`api/views/base.py:166-170`).
pub const SERVER_ERROR_BODY: &str = r#"{"error":"Something went wrong please try again later"}"#;
/// The three issue-scoped endpoints' lookup miss
/// (`views/issue.py:963,1042,1231`).
pub const WORK_ITEM_NOT_FOUND_BODY: &str = r#"{"error":"Work item not found"}"#;
/// The yield endpoint's lookup / membership / ownership miss
/// (`views/issue.py:1291-1296`).
pub const RUN_NOT_FOUND_BODY: &str = r#"{"error":"run not found"}"#;
/// The yield endpoint's inactive-run refusal (`views/issue.py:1298`).
pub const RUN_NOT_ACTIVE_BODY: &str = r#"{"error":"run is not active"}"#;
/// The yield endpoint's missing/invalid outcome (`views/issue.py:1275-1278`);
/// the `allowed` list renders `sorted(RUN_OUTCOMES)`.
pub const OUTCOME_REQUIRED_ERROR: &str = "outcome is required";
/// The yield endpoint's header/URL mismatch (`views/issue.py:1281-1284`).
pub const HEADER_MISMATCH_BODY: &str =
    r#"{"error":"X-Pi-Dash-Run-Id does not match the run in the URL"}"#;
/// `_RUN_AI_REASON_MESSAGES` (`views/issue.py:1168-1172`), plus the `.get`
/// default (`:1240`) for a reason outside the trio.
pub const RUN_AI_ACTIVE_RUN_MESSAGE: &str = "the work item already has an active or queued run";
pub const RUN_AI_NO_POD_MESSAGE: &str = "no pod is available to run this work item";
pub const RUN_AI_NO_ELIGIBLE_RUNNER_MESSAGE: &str =
    "no eligible runner or execution principal is available for this work item";
pub const RUN_AI_FALLBACK_MESSAGE: &str = "could not dispatch a run";

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
    /// 404, `{"detail":"Project not found"}` (identifier rewrite miss).
    ProjectNotFound,
    /// 400, `{"Detail": ...}` (DRF `ParseError`: malformed JSON).
    BadDetail(String),
    /// 400, view-inline `{"error": ...}` with a custom message.
    BadError(String),
    /// 404, view-inline `{"error": ...}` with a custom message.
    NotFoundError(String),
    /// 409, view-inline `{"error": ...}` with a custom message.
    ConflictError(String),
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
            Denial::ProjectNotFound => (
                StatusCode::NOT_FOUND,
                r#"{"detail":"Project not found"}"#.to_owned(),
            ),
            Denial::BadDetail(message) => (
                StatusCode::BAD_REQUEST,
                format!("{{\"Detail\":{}}}", json_string(message)),
            ),
            Denial::BadError(message) => (
                StatusCode::BAD_REQUEST,
                format!("{{\"error\":{}}}", json_string(message)),
            ),
            Denial::NotFoundError(message) => (
                StatusCode::NOT_FOUND,
                format!("{{\"error\":{}}}", json_string(message)),
            ),
            Denial::ConflictError(message) => (
                StatusCode::CONFLICT,
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

/// Render a JSON response with exact bytes and status.
fn json_response(status: StatusCode, body: String) -> Response {
    Response::builder()
        .status(status)
        .header(axum::http::header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body))
        .expect("handler json response")
}

/// Map a database/driver failure to the generic 500 while logging the site
/// and error for operators (no secrets: messages never include tokens).
fn db_error<E: std::fmt::Display>(error: E, site: &str) -> Denial {
    tracing::warn!(%error, site, "v1_work_items actions database failure");
    Denial::ServerError
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
    router
}

/// The four action paths own POST only (`urls/work_item.py:133-151`, each
/// `as_view(http_method_names=["post"])`).
pub fn owned_action(
    router: axum::routing::MethodRouter<AppState>,
) -> axum::routing::MethodRouter<AppState> {
    owned(router, &["POST"])
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
/// `KeyError` branch (`api/views/base.py:160-164`). An EMPTY zone 500s:
/// `ZoneInfo('')` raises `ValueError` (not `KeyError`), which falls
/// through to the generic 500 (`api/views/base.py:166-171`).
fn activate_timezone(timezone: Option<&str>) -> Result<Tz, Denial> {
    match timezone {
        None => Ok(chrono_tz::UTC),
        Some("") => Err(Denial::ServerError),
        Some(zone) => zone
            .parse()
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

/// Fetch the `ProjectEntityPermission` POST facts
/// (`app/permissions/project.py:85-116`): active project membership with
/// role Admin/Member, workspace- and project-scoped. The `*_member`
/// columns share one row read; the entity gate's POST branch only consults
/// `has_project_admin_or_member`.
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

/// Run the route's gate; deny 403 on failure. The three issue-action
/// routes carry `ProjectEntityPermission` (POST → project Admin/Member);
/// the yield route carries `IsAuthenticated` only (`AuthOnly`, always
/// true past the auth layer).
async fn require_gate(
    pool: &PgPool,
    workspace_id: &uuid::Uuid,
    workspace_slug: &str,
    user_id: &uuid::Uuid,
    project_id: &uuid::Uuid,
    route: V1WorkItemsRoute,
) -> Result<(), Denial> {
    let gate = gate_for(route, "POST");
    let facts = entity_facts(pool, workspace_id, workspace_slug, user_id, project_id).await?;
    let scope = TenantScope::new(pidash_types::WorkspaceId::from(workspace_slug.to_owned()));
    if super::perms::decide(gate, "POST", &scope, &facts) {
        Ok(())
    } else {
        Err(Denial::Forbidden)
    }
}

// ---------------------------------------------------------------------------
// Lookups
// ---------------------------------------------------------------------------

/// One issue row as the action endpoints read it:
/// `Issue.objects.select_related("project", "workspace", "state").filter(
/// workspace__slug, project_id, pk).first()` (`:957-961`, `:1036-1040`,
/// `:1225-1229`). `Issue.objects` is the plain `SoftDeletionManager`
/// (`deleted_at IS NULL` only — the triage/archived/draft exclusions live
/// on `issue_objects`, which these views do not use).
pub struct IssueRow {
    pub id: Uuid,
    pub project_id: Option<Uuid>,
    pub workspace_id: Uuid,
    pub created_by_id: Option<Uuid>,
    pub agent_executor: Option<String>,
    pub state_id: Option<Uuid>,
    pub state_name: Option<String>,
    pub state_group: Option<String>,
}

async fn fetch_issue(
    pool: &PgPool,
    slug: &str,
    project_id: &Uuid,
    pk: &Uuid,
) -> Result<Option<IssueRow>, Denial> {
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(
        r#"SELECT i."id", i."project_id", i."workspace_id", i."created_by_id", i."agent_executor",
                  i."state_id", s."name" AS "state_name", s."group" AS "state_group"
           FROM "issues" i JOIN "workspaces" w ON w."id" = i."workspace_id"
           LEFT JOIN "states" s ON s."id" = i."state_id"
           WHERE w."slug" = $1 AND i."project_id" = $2 AND i."id" = $3
             AND i."deleted_at" IS NULL LIMIT 1"#,
    )
    .bind(slug)
    .bind(project_id)
    .bind(pk)
    .fetch_optional(pool)
    .await
    .map_err(|error| db_error(error, "fetch-issue"))?;
    row.map(|row| {
        Ok(IssueRow {
            id: row.try_get("id").map_err(|e| db_error(e, "fetch-issue"))?,
            project_id: row
                .try_get("project_id")
                .map_err(|e| db_error(e, "fetch-issue"))?,
            workspace_id: row
                .try_get("workspace_id")
                .map_err(|e| db_error(e, "fetch-issue"))?,
            created_by_id: row
                .try_get("created_by_id")
                .map_err(|e| db_error(e, "fetch-issue"))?,
            agent_executor: row
                .try_get("agent_executor")
                .map_err(|e| db_error(e, "fetch-issue"))?,
            state_id: row
                .try_get("state_id")
                .map_err(|e| db_error(e, "fetch-issue"))?,
            state_name: row
                .try_get("state_name")
                .map_err(|e| db_error(e, "fetch-issue"))?,
            state_group: row
                .try_get("state_group")
                .map_err(|e| db_error(e, "fetch-issue"))?,
        })
    })
    .transpose()
}

/// Map one agent-run row (with the `runner` join Python's
/// `select_related("runner")` takes) onto [`RunFacts`]. An unknown stored
/// status maps to a non-active member, as the guards document.
fn map_run_facts(row: &sqlx::postgres::PgRow) -> Result<RunFacts, Denial> {
    let status: String = row.try_get("status").map_err(|e| db_error(e, "map-run"))?;
    Ok(RunFacts {
        id: row.try_get("id").map_err(|e| db_error(e, "map-run"))?,
        created_by_id: row
            .try_get("created_by_id")
            .map_err(|e| db_error(e, "map-run"))?,
        owner_id: row
            .try_get("owner_id")
            .map_err(|e| db_error(e, "map-run"))?,
        runner_id: row
            .try_get("runner_id")
            .map_err(|e| db_error(e, "map-run"))?,
        runner_owner_id: row
            .try_get("runner_owner_id")
            .map_err(|e| db_error(e, "map-run"))?,
        work_item_id: row
            .try_get("work_item_id")
            .map_err(|e| db_error(e, "map-run"))?,
        status: AgentRunStatus::from_value(&status).unwrap_or(AgentRunStatus::Cancelled),
    })
}

const RUN_FACTS_COLS: &str = r#"r."id", r."created_by_id", r."owner_id", r."runner_id",
       ru."owner_id" AS "runner_owner_id", r."work_item_id", r."status""#;

/// `AgentRun.objects.select_related("runner").filter(pk=run_id).first()`
/// (`:1113`, `:1127`, `:1153`): the header paths' row.
async fn fetch_run_by_id(pool: &PgPool, run_id: &Uuid) -> Result<Option<RunFacts>, Denial> {
    let sql = format!(
        r#"SELECT {RUN_FACTS_COLS} FROM "agent_run" r
           LEFT JOIN "runner" ru ON ru."id" = r."runner_id" WHERE r."id" = $1 LIMIT 1"#
    );
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(&sql)
        .bind(run_id)
        .fetch_optional(pool)
        .await
        .map_err(|error| db_error(error, "fetch-run"))?;
    row.map(|row| map_run_facts(&row)).transpose()
}

/// `AgentRun.objects.select_related("runner").filter(work_item_id=issue.pk)
/// .order_by("-created_at")[:5]` (`:1127`): the no-header inference rows.
async fn fetch_newest_runs_on_issue(
    pool: &PgPool,
    issue_id: &Uuid,
) -> Result<Vec<RunFacts>, Denial> {
    let sql = format!(
        r#"SELECT {RUN_FACTS_COLS} FROM "agent_run" r
           LEFT JOIN "runner" ru ON ru."id" = r."runner_id"
           WHERE r."work_item_id" = $1 ORDER BY r."created_at" DESC LIMIT 5"#
    );
    let rows: Vec<sqlx::postgres::PgRow> = sqlx::query(&sql)
        .bind(issue_id)
        .fetch_all(pool)
        .await
        .map_err(|error| db_error(error, "fetch-runs"))?;
    rows.iter().map(map_run_facts).collect()
}

/// One yield run row:
/// `AgentRun.objects.select_related("workspace", "work_item", "runner")
/// .filter(pk=run_id, workspace__slug=slug).first()` (`:1285-1289`).
pub struct YieldRun {
    pub facts: RunFacts,
    pub workspace_id: Uuid,
    pub done_payload: Option<Value>,
}

async fn fetch_yield_run(
    pool: &PgPool,
    slug: &str,
    run_id: &Uuid,
) -> Result<Option<YieldRun>, Denial> {
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(
        r#"SELECT r."id", r."created_by_id", r."owner_id", r."runner_id",
                  ru."owner_id" AS "runner_owner_id", r."work_item_id", r."status",
                  r."workspace_id", r."done_payload"
           FROM "agent_run" r JOIN "workspaces" w ON w."id" = r."workspace_id"
           LEFT JOIN "runner" ru ON ru."id" = r."runner_id"
           WHERE r."id" = $1 AND w."slug" = $2 LIMIT 1"#,
    )
    .bind(run_id)
    .bind(slug)
    .fetch_optional(pool)
    .await
    .map_err(|error| db_error(error, "fetch-yield-run"))?;
    row.map(|row| {
        let facts = map_run_facts(&row)?;
        Ok(YieldRun {
            facts,
            workspace_id: row
                .try_get("workspace_id")
                .map_err(|e| db_error(e, "fetch-yield-run"))?,
            done_payload: row
                .try_get("done_payload")
                .map_err(|e| db_error(e, "fetch-yield-run"))?,
        })
    })
    .transpose()
}

/// `is_workspace_member` (`core/permissions.py:28-34`): any active
/// membership row. `deleted_at IS NULL` is the manager scope.
async fn is_workspace_member(
    pool: &PgPool,
    user_id: &Uuid,
    workspace_id: &Uuid,
) -> Result<bool, Denial> {
    sqlx::query_scalar(
        r#"SELECT EXISTS(SELECT 1 FROM "workspace_members" WHERE "workspace_id" = $1 AND "member_id" = $2 AND "is_active" AND "deleted_at" IS NULL)"#,
    )
    .bind(workspace_id)
    .bind(user_id)
    .fetch_one(pool)
    .await
    .map_err(|error| db_error(error, "is-workspace-member"))
}

// ---------------------------------------------------------------------------
// Dispatch seam inputs
// ---------------------------------------------------------------------------

/// One project row as the action endpoints read it: the clock policy
/// (`PROJECT_CLOCK_POLICY_SQL` column order) plus the executor default
/// and the fallback-creator candidates.
pub struct ProjectRow {
    pub policy: pidash_services::orchestration::clock::ProjectClockPolicy,
    pub default_agent_executor: String,
    pub project_lead_id: Option<Uuid>,
    pub default_assignee_id: Option<Uuid>,
}

async fn fetch_project(pool: &PgPool, project_id: &Uuid) -> Result<Option<ProjectRow>, Denial> {
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(
        r#"SELECT "agent_ticking_enabled", "agent_default_max_ticks",
                  "agent_default_interval_seconds", "agent_review_default_interval_seconds",
                  "agent_test_default_interval_seconds", "default_agent_executor",
                  "project_lead_id", "default_assignee_id"
           FROM "projects" WHERE "id" = $1 LIMIT 1"#,
    )
    .bind(project_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| db_error(error, "fetch-project"))?;
    row.map(|row| {
        // The cadence columns are INT4; the policy widens them to i64
        // (the jobs `map_policy` precedent).
        let interval_impl: Option<i32> = row
            .try_get("agent_default_interval_seconds")
            .map_err(|e| db_error(e, "fetch-project"))?;
        let interval_review: Option<i32> = row
            .try_get("agent_review_default_interval_seconds")
            .map_err(|e| db_error(e, "fetch-project"))?;
        let interval_test: Option<i32> = row
            .try_get("agent_test_default_interval_seconds")
            .map_err(|e| db_error(e, "fetch-project"))?;
        let policy = pidash_services::orchestration::clock::ProjectClockPolicy {
            agent_ticking_enabled: row
                .try_get("agent_ticking_enabled")
                .map_err(|e| db_error(e, "fetch-project"))?,
            agent_default_max_ticks: row
                .try_get("agent_default_max_ticks")
                .map_err(|e| db_error(e, "fetch-project"))?,
            agent_default_interval_seconds: interval_impl.map(i64::from),
            agent_review_default_interval_seconds: interval_review.map(i64::from),
            agent_test_default_interval_seconds: interval_test.map(i64::from),
        };
        Ok(ProjectRow {
            policy,
            default_agent_executor: row
                .try_get("default_agent_executor")
                .map_err(|e| db_error(e, "fetch-project"))?,
            project_lead_id: row
                .try_get("project_lead_id")
                .map_err(|e| db_error(e, "fetch-project"))?,
            default_assignee_id: row
                .try_get("default_assignee_id")
                .map_err(|e| db_error(e, "fetch-project"))?,
        })
    })
    .transpose()
}

/// Request-scoped admission cache: the sync `AdmissionCache` trait cannot
/// reach the async Redis client, and the drivers consult it only when
/// building a cloud-executor run, so a per-request map answers identically
/// to Django on every CE/local path (see the module docs).
#[derive(Debug, Default)]
pub struct RequestCache {
    buckets: Mutex<HashMap<String, i64>>,
}

#[derive(Debug)]
pub struct CacheError(pub String);

impl std::fmt::Display for CacheError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for CacheError {}

impl pidash_services::dispatch::admission::AdmissionCache for RequestCache {
    type Error = CacheError;

    fn bucket_count(&self, key: &str) -> Result<Option<i64>, Self::Error> {
        self.buckets
            .lock()
            .map(|buckets| buckets.get(key).copied())
            .map_err(|e| CacheError(e.to_string()))
    }

    fn add_or_incr(&self, key: &str, _timeout_secs: i64) -> Result<(), Self::Error> {
        self.buckets
            .lock()
            .map(|mut buckets| {
                buckets
                    .entry(key.to_owned())
                    .and_modify(|n| *n += 1)
                    .or_insert(1);
            })
            .map_err(|e| CacheError(e.to_string()))
    }
}

/// LLM-key presence for the D-11 closures: the actor plus every
/// fallback-creator candidate the drivers may query (issue creator,
/// project lead, default assignee, live assignees), read from
/// `assistant_user_llm_config` the way the assistant gate reads it.
async fn fetch_llm_presence(
    pool: &PgPool,
    user_ids: &[Uuid],
) -> Result<HashMap<Uuid, bool>, Denial> {
    let mut presence: HashMap<Uuid, bool> = HashMap::with_capacity(user_ids.len());
    if user_ids.is_empty() {
        return Ok(presence);
    }
    let rows: Vec<(Uuid, Option<Vec<u8>>)> =
        sqlx::query_as(r#"SELECT "user_id", "api_key_encrypted" FROM "assistant_user_llm_config" WHERE "user_id" = ANY($1)"#)
            .bind(user_ids)
            .fetch_all(pool)
            .await
            .map_err(|error| db_error(error, "fetch-llm-presence"))?;
    for id in user_ids {
        presence.insert(*id, false);
    }
    for (id, key) in rows {
        presence.insert(id, pidash_db::assistant::models::has_secret(&key));
    }
    Ok(presence)
}

/// The creator-candidate universe the drivers may consult
/// (`_resolve_creator_for_trigger`, `scheduling.py:887-952`): the actor
/// first, then the fallback chain.
async fn creator_universe(
    pool: &PgPool,
    actor_id: Uuid,
    issue: &IssueRow,
    project: &ProjectRow,
) -> Result<Vec<Uuid>, Denial> {
    let mut ids: Vec<Uuid> = vec![actor_id];
    for id in [
        issue.created_by_id,
        project.project_lead_id,
        project.default_assignee_id,
    ]
    .into_iter()
    .flatten()
    {
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    let assignees: Vec<(Uuid, bool, bool)> =
        sqlx::query_as(pidash_services::orchestration::dispatch::LIVE_ASSIGNEE_CANDIDATES_SQL)
            .bind(issue.id)
            .fetch_all(pool)
            .await
            .map_err(|error| db_error(error, "fetch-assignees"))?;
    for (id, _, _) in assignees {
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    Ok(ids)
}

/// Live `PodRunnerMatcher` over the merged D-14 guards helpers
/// (`runner/services/matcher.py:375-412`): the managed narrow branch or
/// the general owner-set branch, with the same no-query short-circuits.
pub struct LiveMatcher {
    pool: PgPool,
}

impl LiveMatcher {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl pidash_services::orchestration::dispatch::PodRunnerMatcher for LiveMatcher {
    fn pod_has_runner_for_issue_principal(
        &self,
        pod_id: Uuid,
        issue_id: Uuid,
        creator_id: Option<Uuid>,
    ) -> Pin<
        Box<
            dyn Future<
                    Output = Result<bool, pidash_services::orchestration::dispatch::DispatchError>,
                > + Send
                + '_,
        >,
    > {
        let pool = self.pool.clone();
        Box::pin(async move {
            use pidash_services::orchestration::dispatch::DispatchError;
            use pidash_services::runner_sessions::guards as g;
            let db = |e: sqlx::Error| DispatchError::Db(e.to_string());
            let issue: Option<(Option<String>, Option<Uuid>, Option<Uuid>)> = sqlx::query_as(
                r#"SELECT "agent_executor", "created_by_id", "project_id" FROM "issues" WHERE "id" = $1"#,
            )
            .bind(issue_id)
            .fetch_optional(&pool)
            .await
            .map_err(db)?;
            let Some((agent_executor, created_by_id, project_id)) = issue else {
                return Ok(false);
            };
            let project_id = project_id.ok_or_else(|| {
                DispatchError::Store(
                    pidash_services::orchestration::creation::CreationError::MissingRow(
                        "issue has no project".to_owned(),
                    ),
                )
            })?;
            let default_executor: Option<String> = sqlx::query_scalar(
                r#"SELECT "default_agent_executor" FROM "projects" WHERE "id" = $1"#,
            )
            .bind(project_id)
            .fetch_optional(&pool)
            .await
            .map_err(db)?
            .flatten();
            let default_executor = default_executor.unwrap_or_default();
            if g::is_managed_runner_issue(agent_executor.as_deref(), &default_executor) {
                let Some(creator) = creator_id else {
                    return Ok(false);
                };
                let hit: Option<(i32,)> = sqlx::query_as(g::POD_HAS_RUNNER_MANAGED_SQL)
                    .bind(creator)
                    .bind(pod_id)
                    .fetch_optional(&pool)
                    .await
                    .map_err(db)?;
                return Ok(hit.is_some());
            }
            let assignees: Vec<Uuid> = sqlx::query_scalar(g::ISSUE_ASSIGNEE_IDS_SQL)
                .bind(issue_id)
                .fetch_all(&pool)
                .await
                .map_err(db)?;
            let owners = g::eligible_owner_ids(creator_id, created_by_id, &assignees);
            let Some(sql) = g::pod_has_runner_general_sql(owners.len()) else {
                return Ok(false);
            };
            let mut query = sqlx::query_as::<_, (i32,)>(&sql);
            for owner in &owners {
                query = query.bind(owner);
            }
            query = query.bind(pod_id);
            let hit: Option<(i32,)> = query.fetch_optional(&pool).await.map_err(db)?;
            Ok(hit.is_some())
        })
    }
}

/// LLM-key presence for the creator universe (see [`creator_universe`]).
async fn fetch_presence(
    pool: &PgPool,
    actor_id: Uuid,
    issue: &IssueRow,
    project: &ProjectRow,
) -> Result<Arc<HashMap<Uuid, bool>>, Denial> {
    let universe = creator_universe(pool, actor_id, issue, project).await?;
    Ok(Arc::new(fetch_llm_presence(pool, &universe).await?))
}

/// The `has_usable_llm_config` closure over pre-fetched presence. A user
/// outside the pre-fetched universe (only reachable through a concurrent
/// insert between the prefetch and the driver) answers keyless, the same
/// as a missing config row.
pub fn has_key_fn(
    presence: Arc<HashMap<Uuid, bool>>,
) -> impl Fn(Uuid) -> bool + Clone + Send + Sync {
    move |uid: Uuid| -> bool {
        pidash_services::assistant::seams::has_usable_llm_config(
            presence.get(&uid).copied().unwrap_or(false),
        )
    }
}

/// The `managed_llm_profile` closure over pre-fetched presence.
pub fn profile_fn(
    presence: Arc<HashMap<Uuid, bool>>,
) -> impl Fn(Option<Uuid>) -> pidash_services::dispatch::LlmProfile + Clone + Send + Sync {
    move |uid: Option<Uuid>| -> pidash_services::dispatch::LlmProfile {
        let key = uid
            .and_then(|uid| presence.get(&uid).copied())
            .unwrap_or(false);
        let profile = pidash_services::assistant::seams::agent_model_profile_for_user(key);
        pidash_services::dispatch::LlmProfile {
            available: profile.available,
            reason_code: profile.reason_code,
        }
    }
}

/// Live [`StoreDeps`](pidash_jobs::orchestration::StoreDeps): settings from
/// the app state, LLM presence for the creator universe, the CE
/// `extra_toolsets_enabled = false`, and the request admission cache.
/// Returns the opaque struct so each driver call site keeps its concrete
/// closure types.
#[allow(clippy::too_many_arguments)]
pub async fn dispatch_deps(
    state: &AppState,
    pool: &PgPool,
    actor_id: Uuid,
    issue: &IssueRow,
    project: &ProjectRow,
    now_unix_secs: i64,
) -> Result<
    (
        pidash_jobs::orchestration::StoreDeps<
            RequestCache,
            impl Fn(Uuid) -> bool + Clone + Send + Sync,
            impl Fn(Uuid) -> bool + Clone + Send + Sync,
            impl Fn(Option<Uuid>) -> pidash_services::dispatch::LlmProfile + Clone + Send + Sync,
        >,
        LiveMatcher,
    ),
    Denial,
> {
    let presence = fetch_presence(pool, actor_id, issue, project).await?;
    Ok((
        pidash_jobs::orchestration::StoreDeps {
            cloud: state.settings().cloud_agent.clone(),
            managed: state.settings().managed_runner.clone(),
            cache: RequestCache::default(),
            has_usable_llm_config: has_key_fn(Arc::clone(&presence)),
            extra_toolsets_enabled: |_: Uuid| false,
            llm_profile: profile_fn(presence),
            now_unix_secs,
        },
        LiveMatcher::new(pool.clone()),
    ))
}

/// Draw the call's jitter from the issue's effective interval
/// (`jitter_seconds(effective_interval_seconds())`), as the scheduling
/// functions do before each re-time.
pub fn draw_jitter(
    issue: &IssueRow,
    policy: &pidash_services::orchestration::clock::ProjectClockPolicy,
) -> f64 {
    let state = match (issue.state_group.as_deref(), issue.state_name.as_deref()) {
        (Some(group), Some(name)) => Some(pidash_types::orchestration::StateRef { group, name }),
        _ => None,
    };
    let interval =
        pidash_services::orchestration::clock::effective_interval_seconds(state.as_ref(), policy);
    pidash_db::tasks_ticker::jitter_seconds(interval, &mut rand08::thread_rng())
}

// ---------------------------------------------------------------------------
// Response shapers (pure; key order is the contract)
// ---------------------------------------------------------------------------

/// Render an aware datetime the way `ticker.next_run_at.isoformat()` /
/// `timezone.now().isoformat()` render: the stored instant with its offset,
/// microseconds only when nonzero (the `app_integrations` precedent).
fn isoformat(dt: &DateTime<Utc>) -> String {
    dt.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, false)
}

/// `{"granted","reason","run_id"}` plus the ticker keys when a row exists
/// (`:970-984`): `used`, `tick_count` (= `used`), `max_ticks`, `enabled`,
/// `pending_entry`, `next_run_at`. Always 200.
pub fn retick_response(
    granted: bool,
    reason: &str,
    run_id: Option<Uuid>,
    ticker: Option<(&pidash_db::tasks_ticker::IssueAgentTicker, i32)>,
) -> (StatusCode, String) {
    let mut map = serde_json::Map::with_capacity(9);
    map.insert("granted".to_owned(), Value::Bool(granted));
    map.insert("reason".to_owned(), Value::String(reason.to_owned()));
    map.insert(
        "run_id".to_owned(),
        run_id
            .map(|id| Value::String(id.to_string()))
            .unwrap_or(Value::Null),
    );
    if let Some((ticker, pool)) = ticker {
        map.insert("used".to_owned(), Value::from(ticker.used));
        map.insert("tick_count".to_owned(), Value::from(ticker.tick_count()));
        map.insert(
            "max_ticks".to_owned(),
            Value::from(ticker.effective_max_ticks(pool)),
        );
        map.insert("enabled".to_owned(), Value::Bool(ticker.enabled));
        map.insert(
            "pending_entry".to_owned(),
            Value::Bool(ticker.pending_entry),
        );
        map.insert(
            "next_run_at".to_owned(),
            ticker
                .next_run_at
                .as_ref()
                .map(|dt| Value::String(isoformat(dt)))
                .unwrap_or(Value::Null),
        );
    }
    let body = serde_json::to_string(&Value::Object(map)).expect("retick body");
    (StatusCode::OK, body)
}

/// `{"applied","reason"}` plus the ticker keys when a row exists
/// (`:1050-1063`): `used`, `granted`, `waited`, `cap`,
/// `wait_allowance_remaining`. Always 200.
pub fn wait_response(
    applied: bool,
    reason: &str,
    ticker: Option<(&pidash_db::tasks_ticker::IssueAgentTicker, i32)>,
) -> (StatusCode, String) {
    let mut map = serde_json::Map::with_capacity(7);
    map.insert("applied".to_owned(), Value::Bool(applied));
    map.insert("reason".to_owned(), Value::String(reason.to_owned()));
    if let Some((ticker, pool)) = ticker {
        map.insert("used".to_owned(), Value::from(ticker.used));
        map.insert("granted".to_owned(), Value::from(ticker.granted));
        map.insert("waited".to_owned(), Value::from(ticker.waited));
        map.insert(
            "cap".to_owned(),
            Value::from(ticker.effective_max_ticks(pool)),
        );
        map.insert(
            "wait_allowance_remaining".to_owned(),
            Value::from(ticker.wait_allowance(pool)),
        );
    }
    let body = serde_json::to_string(&Value::Object(map)).expect("wait body");
    (StatusCode::OK, body)
}

/// The run-ai refusal (`:1237-1244`): 409 `{"error","reason"}` with the
/// `_RUN_AI_REASON_MESSAGES` text, or the `.get` default. `None` (creation
/// refused after preflight passed, so `dispatch_run_ai_run_with_reason`
/// returns `(None, None)`) renders `"reason":null`, exactly as the view's
/// `{"error": ..., "reason": reason}` does for `reason=None`.
pub fn run_ai_refusal(reason: Option<&str>) -> (StatusCode, String) {
    let message = match reason {
        Some("active_run_exists") => RUN_AI_ACTIVE_RUN_MESSAGE,
        Some("no_pod") => RUN_AI_NO_POD_MESSAGE,
        Some("no_eligible_runner") => RUN_AI_NO_ELIGIBLE_RUNNER_MESSAGE,
        _ => RUN_AI_FALLBACK_MESSAGE,
    };
    let mut map = serde_json::Map::with_capacity(2);
    map.insert("error".to_owned(), Value::String(message.to_owned()));
    map.insert(
        "reason".to_owned(),
        reason
            .map(|reason| Value::String(reason.to_owned()))
            .unwrap_or(Value::Null),
    );
    let body = serde_json::to_string(&Value::Object(map)).expect("run-ai body");
    (StatusCode::CONFLICT, body)
}

/// The run-ai success (`:1245-1252`): 201 `{"id","status","executor"}`.
pub fn run_ai_created(run_id: Uuid, status: &str, executor_kind: &str) -> (StatusCode, String) {
    let mut map = serde_json::Map::with_capacity(3);
    map.insert("id".to_owned(), Value::String(run_id.to_string()));
    map.insert("status".to_owned(), Value::String(status.to_owned()));
    map.insert(
        "executor".to_owned(),
        Value::String(executor_kind.to_owned()),
    );
    let body = serde_json::to_string(&Value::Object(map)).expect("run-ai body");
    (StatusCode::CREATED, body)
}

/// The yield success (`:1308-1311`):
/// `{"ok","run_id","work_item_id","outcome"}`. Always 200.
pub fn yield_response(run_id: Uuid, work_item_id: Uuid, outcome: &str) -> (StatusCode, String) {
    let mut map = serde_json::Map::with_capacity(4);
    map.insert("ok".to_owned(), Value::Bool(true));
    map.insert("run_id".to_owned(), Value::String(run_id.to_string()));
    map.insert(
        "work_item_id".to_owned(),
        Value::String(work_item_id.to_string()),
    );
    map.insert("outcome".to_owned(), Value::String(outcome.to_owned()));
    let body = serde_json::to_string(&Value::Object(map)).expect("yield body");
    (StatusCode::OK, body)
}

/// The yield outcome refusal (`:1273-1278`): 400
/// `{"error":"outcome is required","allowed":sorted(RUN_OUTCOMES)}`.
pub fn outcome_required_response() -> (StatusCode, String) {
    let mut allowed: Vec<&str> = RUN_OUTCOMES.to_vec();
    allowed.sort_unstable();
    let mut map = serde_json::Map::with_capacity(2);
    map.insert(
        "error".to_owned(),
        Value::String(OUTCOME_REQUIRED_ERROR.to_owned()),
    );
    map.insert(
        "allowed".to_owned(),
        Value::Array(
            allowed
                .into_iter()
                .map(|s| Value::String(s.to_owned()))
                .collect(),
        ),
    );
    let body = serde_json::to_string(&Value::Object(map)).expect("yield body");
    (StatusCode::BAD_REQUEST, body)
}

/// `note.strip()[:2000]` (`:1304-1305`): blank notes are dropped, the rest
/// stripped and cut at 2000 code points (never mid-character).
pub fn truncate_note(note: &str) -> Option<String> {
    let stripped = note.trim();
    if stripped.is_empty() {
        return None;
    }
    if stripped.len() <= 2000 {
        return Some(stripped.to_owned());
    }
    Some(stripped.chars().take(2000).collect())
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// Read the whole body.
async fn read_body(body: axum::body::Body) -> Result<Vec<u8>, Denial> {
    axum::body::to_bytes(body, usize::MAX)
        .await
        .map(|bytes| bytes.to_vec())
        .map_err(|error| db_error(error, "read-body"))
}

/// `POST .../work-items/<pk>/re-tick/` (`views/issue.py:940-986`).
pub async fn post_retick(
    State(state): State<AppState>,
    Path((slug, project_id, pk)): Path<(String, String, String)>,
    headers: HeaderMap,
    req: Request,
) -> Response {
    if !crate::runner_runs::is_uuid_path_segment(&pk) {
        return crate::edge::proxy(State(state), req).await;
    }
    let pk = pk.parse::<Uuid>().expect("checked segment");
    let (_parts, body) = req.into_parts();
    if read_body(body).await.is_err() {
        return Denial::ServerError.into_response();
    }
    match retick_inner(&state, &headers, &slug, &project_id, &pk).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

async fn retick_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    project_id_raw: &str,
    pk: &Uuid,
) -> Result<Response, Denial> {
    let pre = preamble(state, headers, slug).await?;
    // Rewrite before the gate (`base.py:107-111`): an identifier miss 404s
    // even under an unknown slug, exactly as `Project.resolve` raising
    // inside `initial()` does before `check_permissions` runs.
    let project_id = rewrite_project_id(&pre.pool, slug, project_id_raw).await?;
    let workspace_id = pre.workspace_id.ok_or(Denial::Forbidden)?;
    require_gate(
        &pre.pool,
        &workspace_id,
        slug,
        &pre.actor.id,
        &project_id,
        V1WorkItemsRoute::IssueReTick,
    )
    .await?;
    // `TimezoneMixin.initial` runs after the gate: survivors with an
    // unknown stored zone 400 here (an empty zone 500s). No action
    // response renders a zone-shifted field, so the value is discarded.
    activate_timezone(pre.actor.timezone.as_deref())?;
    let issue = fetch_issue(&pre.pool, slug, &project_id, pk)
        .await?
        .ok_or(Denial::NotFoundError("Work item not found".to_owned()))?;
    let header = headers
        .get(super::perms::RUN_ID_HEADER)
        .and_then(|v| v.to_str().ok());
    let run_by_id = match header.map(str::trim).filter(|h| !h.is_empty()) {
        Some(raw) => match Uuid::parse_str(raw) {
            Ok(id) => fetch_run_by_id(&pre.pool, &id).await?,
            Err(_) => None,
        },
        None => None,
    };
    if let Some(refused) = refuse_agent_retick(header, issue.id, run_by_id.as_ref()) {
        return Ok(json_response(
            StatusCode::from_u16(refused.status).unwrap_or(StatusCode::FORBIDDEN),
            refused.body,
        ));
    }
    let project = fetch_project(&pre.pool, &project_id)
        .await?
        .ok_or(Denial::ServerError)?;
    let now = Utc::now();
    let jitter = draw_jitter(&issue, &project.policy);
    let (deps, matcher) = dispatch_deps(
        state,
        &pre.pool,
        pre.actor.id,
        &issue,
        &project,
        now.timestamp(),
    )
    .await?;
    let out = pidash_jobs::orchestration::re_tick_ticker(
        &pre.pool,
        deps,
        &matcher,
        now,
        jitter,
        issue.id,
        Some(pre.actor.id),
    )
    .await
    .map_err(|error| db_error(error, "retick-driver"))?;
    let outcome = out.outcome;
    let pool =
        pidash_db::tasks_ticker::pool_size_or_default(project.policy.agent_default_max_ticks);
    let (status, body) = retick_response(
        outcome.granted,
        &outcome.reason,
        outcome.run_id,
        outcome.ticker.as_ref().map(|ticker| (ticker, pool)),
    );
    Ok(json_response(status, body))
}

/// `POST .../work-items/<pk>/wait/` (`views/issue.py:987-1069`).
pub async fn post_wait(
    State(state): State<AppState>,
    Path((slug, project_id, pk)): Path<(String, String, String)>,
    headers: HeaderMap,
    req: Request,
) -> Response {
    if !crate::runner_runs::is_uuid_path_segment(&pk) {
        return crate::edge::proxy(State(state), req).await;
    }
    let pk = pk.parse::<Uuid>().expect("checked segment");
    let (_parts, body) = req.into_parts();
    if read_body(body).await.is_err() {
        return Denial::ServerError.into_response();
    }
    match wait_inner(&state, &headers, &slug, &project_id, &pk).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

async fn wait_inner(
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
        V1WorkItemsRoute::IssueWait,
    )
    .await?;
    // `TimezoneMixin.initial` runs after the gate: survivors with an
    // unknown stored zone 400 here (an empty zone 500s). No action
    // response renders a zone-shifted field, so the value is discarded.
    activate_timezone(pre.actor.timezone.as_deref())?;
    let issue = fetch_issue(&pre.pool, slug, &project_id, pk)
        .await?
        .ok_or(Denial::NotFoundError("Work item not found".to_owned()))?;
    // Best-effort attribution only (`:1044-1047`): the resolve error is
    // ignored and the wait still applies.
    let header = headers
        .get(super::perms::RUN_ID_HEADER)
        .and_then(|v| v.to_str().ok());
    let run_by_id = match header.map(str::trim).filter(|h| !h.is_empty()) {
        Some(raw) => match Uuid::parse_str(raw) {
            Ok(id) => fetch_run_by_id(&pre.pool, &id).await?,
            Err(_) => None,
        },
        None => None,
    };
    let newest = fetch_newest_runs_on_issue(&pre.pool, &issue.id).await?;
    let (run_id, _) = resolve_moved_by_run(
        header,
        Some(pre.actor.id),
        issue.id,
        run_by_id.as_ref(),
        &newest,
    );
    let project = fetch_project(&pre.pool, &project_id)
        .await?
        .ok_or(Denial::ServerError)?;
    let now = Utc::now();
    let jitter = draw_jitter(&issue, &project.policy);
    let epoch_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    // The wait driver takes no matcher (no dispatch leg); the underscore
    // keeps the shared constructor.
    let (deps, _matcher) = dispatch_deps(
        state,
        &pre.pool,
        pre.actor.id,
        &issue,
        &project,
        now.timestamp(),
    )
    .await?;
    let out = pidash_jobs::orchestration::wait_ticker(
        &pre.pool,
        deps,
        now,
        jitter,
        epoch_secs,
        issue.id,
        run_id,
        Some(pre.actor.id),
    )
    .await
    .map_err(|error| db_error(error, "wait-driver"))?;
    let outcome = out.outcome;
    let pool =
        pidash_db::tasks_ticker::pool_size_or_default(project.policy.agent_default_max_ticks);
    let (status, body) = wait_response(
        outcome.applied,
        &outcome.reason,
        outcome.ticker.as_ref().map(|ticker| (ticker, pool)),
    );
    Ok(json_response(status, body))
}

/// `POST .../work-items/<pk>/run-ai/` (`views/issue.py:1175-1254`).
pub async fn post_run_ai(
    State(state): State<AppState>,
    Path((slug, project_id, pk)): Path<(String, String, String)>,
    headers: HeaderMap,
    req: Request,
) -> Response {
    if !crate::runner_runs::is_uuid_path_segment(&pk) {
        return crate::edge::proxy(State(state), req).await;
    }
    let pk = pk.parse::<Uuid>().expect("checked segment");
    let (_parts, body) = req.into_parts();
    if read_body(body).await.is_err() {
        return Denial::ServerError.into_response();
    }
    match run_ai_inner(&state, &headers, &slug, &project_id, &pk).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

async fn run_ai_inner(
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
        V1WorkItemsRoute::IssueRunAi,
    )
    .await?;
    // `TimezoneMixin.initial` runs after the gate: survivors with an
    // unknown stored zone 400 here (an empty zone 500s). No action
    // response renders a zone-shifted field, so the value is discarded.
    activate_timezone(pre.actor.timezone.as_deref())?;
    let issue = fetch_issue(&pre.pool, slug, &project_id, pk)
        .await?
        .ok_or(Denial::NotFoundError("Work item not found".to_owned()))?;
    let header = headers
        .get(super::perms::RUN_ID_HEADER)
        .and_then(|v| v.to_str().ok());
    let run_by_id = match header.map(str::trim).filter(|h| !h.is_empty()) {
        Some(raw) => match Uuid::parse_str(raw) {
            Ok(id) => fetch_run_by_id(&pre.pool, &id).await?,
            Err(_) => None,
        },
        None => None,
    };
    if let Some(refused) = refuse_agent_action(header, issue.id, run_by_id.as_ref(), "Run AI") {
        return Ok(json_response(
            StatusCode::from_u16(refused.status).unwrap_or(StatusCode::FORBIDDEN),
            refused.body,
        ));
    }
    let project = fetch_project(&pre.pool, &project_id)
        .await?
        .ok_or(Denial::ServerError)?;
    let now = Utc::now();
    let jitter = draw_jitter(&issue, &project.policy);
    let (deps, matcher) = dispatch_deps(
        state,
        &pre.pool,
        pre.actor.id,
        &issue,
        &project,
        now.timestamp(),
    )
    .await?;
    let out = pidash_jobs::orchestration::run_ai_for_human(
        &pre.pool,
        deps,
        &matcher,
        now,
        jitter,
        issue.id,
        Some(pre.actor.id),
        Some(pre.actor.id),
    )
    .await
    .map_err(|error| db_error(error, "run-ai-driver"))?;
    let outcome = out.outcome;
    let Some(run_id) = outcome.run_id else {
        let (status, body) = run_ai_refusal(outcome.reason.as_deref());
        return Ok(json_response(status, body));
    };
    let row: Option<(Uuid, String, String)> = sqlx::query_as(
        r#"SELECT "id", "status", "executor_kind" FROM "agent_run" WHERE "id" = $1"#,
    )
    .bind(run_id)
    .fetch_optional(&pre.pool)
    .await
    .map_err(|error| db_error(error, "run-ai-readback"))?;
    let Some((id, status_text, executor)) = row else {
        return Err(Denial::ServerError);
    };
    let (status, body) = run_ai_created(id, &status_text, &executor);
    Ok(json_response(status, body))
}

/// `POST .../agent-runs/<run_id>/yield/` (`views/issue.py:1255-1313`).
pub async fn post_yield(
    State(state): State<AppState>,
    Path((slug, run_id)): Path<(String, String)>,
    headers: HeaderMap,
    req: Request,
) -> Response {
    if !crate::runner_runs::is_uuid_path_segment(&run_id) {
        return crate::edge::proxy(State(state), req).await;
    }
    let run_id = run_id.parse::<Uuid>().expect("checked segment");
    let (_parts, body) = req.into_parts();
    let bytes = match read_body(body).await {
        Ok(bytes) => bytes,
        Err(denial) => return denial.into_response(),
    };
    match yield_inner(&state, &headers, &slug, &run_id, &bytes).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

async fn yield_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    run_id: &Uuid,
    raw_body: &[u8],
) -> Result<Response, Denial> {
    let pre = preamble(state, headers, slug).await?;
    // `AuthOnly`: the base `IsAuthenticated` already passed, so the
    // authenticated caller reaches the handler body.
    let _ = pre.workspace_id;
    // `TimezoneMixin.initial` runs after auth, before the view body: an
    // unknown stored zone 400s here (an empty zone 500s), even before
    // the body parses. No action response renders a zone-shifted field,
    // so the value is discarded.
    activate_timezone(pre.actor.timezone.as_deref())?;
    let data: Value = if raw_body.is_empty() {
        Value::Object(serde_json::Map::new())
    } else {
        serde_json::from_slice(raw_body)
            .map_err(|error| Denial::BadDetail(format!("JSON parse error - {error}")))?
    };
    let obj = data.as_object().ok_or(Denial::ServerError)?;
    let outcome = normalize_outcome(obj.get("outcome").unwrap_or(&Value::Null), "");
    let Some(outcome) = outcome else {
        let (status, body) = outcome_required_response();
        return Ok(json_response(status, body));
    };
    let header_run = headers
        .get(super::perms::RUN_ID_HEADER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .trim();
    if !header_run.is_empty() && header_run.to_lowercase() != run_id.to_string().to_lowercase() {
        return Ok(json_response(
            StatusCode::BAD_REQUEST,
            HEADER_MISMATCH_BODY.to_owned(),
        ));
    }
    let run = fetch_yield_run(&pre.pool, slug, run_id)
        .await?
        .filter(|run| run.facts.work_item_id.is_some())
        .ok_or(Denial::NotFoundError("run not found".to_owned()))?;
    if !is_workspace_member(&pre.pool, &pre.actor.id, &run.workspace_id).await? {
        return Err(Denial::NotFoundError("run not found".to_owned()));
    }
    if !run_belongs_to(Some(pre.actor.id), Some(&run.facts)) {
        return Err(Denial::NotFoundError("run not found".to_owned()));
    }
    if !run.facts.status.is_active() {
        return Err(Denial::ConflictError("run is not active".to_owned()));
    }
    let mut payload: serde_json::Map<String, Value> = match run.done_payload.as_ref() {
        Some(Value::Object(map)) => map.clone(),
        _ => serde_json::Map::new(),
    };
    payload.insert("status".to_owned(), Value::String(outcome.to_owned()));
    payload.insert(
        "yielded_at".to_owned(),
        Value::String(isoformat(&Utc::now())),
    );
    if let Some(Value::String(note)) = obj.get("note") {
        if let Some(note) = truncate_note(note) {
            payload.insert("note".to_owned(), Value::String(note));
        }
    }
    sqlx::query(r#"UPDATE "agent_run" SET "done_payload" = $1 WHERE "id" = $2"#)
        .bind(Value::Object(payload))
        .bind(run.facts.id)
        .execute(&pre.pool)
        .await
        .map_err(|error| db_error(error, "yield-write"))?;
    let work_item_id = run.facts.work_item_id.expect("checked above");
    let (status, body) = yield_response(run.facts.id, work_item_id, outcome);
    Ok(json_response(status, body))
}

#[cfg(test)]
mod tests {
    use super::*;

    static F18_11: &str =
        include_str!("../../../../fixtures/v1_work_items/handlers/F18-11.work_items.json");

    fn calls() -> Value {
        let fx: Value = serde_json::from_str(F18_11).expect("F18-11 parses");
        fx.get("calls").expect("calls").clone()
    }

    fn call_body(name: &str) -> (u16, String) {
        let call = calls().get(name).unwrap_or(&Value::Null).clone();
        let status = call.get("status").and_then(Value::as_u64).expect("status") as u16;
        let body = serde_json::to_string(call.get("body").expect("body")).expect("body");
        (status, body)
    }

    fn ticker_row() -> pidash_db::tasks_ticker::IssueAgentTicker {
        pidash_db::tasks_ticker::IssueAgentTicker {
            id: Uuid::nil(),
            created_at: DateTime::from_timestamp(0, 0).expect("epoch"),
            updated_at: DateTime::from_timestamp(0, 0).expect("epoch"),
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            issue_id: Uuid::nil(),
            used: 3,
            granted: 10,
            waited: 2,
            user_disabled: false,
            next_run_at: Some(
                DateTime::parse_from_rfc3339("2026-10-04T12:34:56.789012+00:00")
                    .expect("dt")
                    .with_timezone(&Utc),
            ),
            last_tick_at: None,
            enabled: true,
            disarm_reason: String::new(),
            pending_entry: false,
            pending_entry_free: false,
            pending_entry_actor_id: None,
            pending_entry_trigger: String::new(),
            resume_parent_run_id: None,
        }
    }

    /// The 7 action-route calls this issue replays. Fails if the fixture
    /// drops or renames one, so coverage cannot silently shrink.
    #[test]
    fn replay_count() {
        let calls = calls();
        for name in [
            "retick",
            "wait",
            "run_ai",
            "yield_ok",
            "yield_bad_outcome",
            "yield_header_mismatch",
            "yield_inactive",
        ] {
            assert!(calls.get(name).is_some(), "fixture call {name}");
        }
    }

    #[test]
    fn replay_retick_no_ticker() {
        let (status, body) = retick_response(false, "no_ticker", None, None);
        let (fx_status, fx_body) = call_body("retick");
        assert_eq!(status.as_u16(), fx_status);
        assert_eq!(body, fx_body);
    }

    #[test]
    fn replay_wait_no_ticker() {
        let (status, body) = wait_response(false, "no_ticker", None);
        let (fx_status, fx_body) = call_body("wait");
        assert_eq!(status.as_u16(), fx_status);
        assert_eq!(body, fx_body);
    }

    #[test]
    fn replay_run_ai_no_pod() {
        let (status, body) = run_ai_refusal(Some("no_pod"));
        let (fx_status, fx_body) = call_body("run_ai");
        assert_eq!(status.as_u16(), fx_status);
        assert_eq!(body, fx_body);
    }

    #[test]
    fn replay_yield_ok() {
        let run_id: Uuid = "640d8cdf-08fb-4772-82d3-2c693d0e105f"
            .parse()
            .expect("uuid");
        let work_item_id: Uuid = "a9b6c9f9-2817-4967-a501-1e26caf4abea"
            .parse()
            .expect("uuid");
        let (status, body) = yield_response(run_id, work_item_id, "progressed");
        let (fx_status, fx_body) = call_body("yield_ok");
        assert_eq!(status.as_u16(), fx_status);
        assert_eq!(body, fx_body);
    }

    #[test]
    fn replay_yield_bad_outcome() {
        let (status, body) = outcome_required_response();
        let (fx_status, fx_body) = call_body("yield_bad_outcome");
        assert_eq!(status.as_u16(), fx_status);
        assert_eq!(body, fx_body);
    }

    #[test]
    fn replay_yield_header_mismatch() {
        let (fx_status, fx_body) = call_body("yield_header_mismatch");
        assert_eq!(400, fx_status);
        assert_eq!(HEADER_MISMATCH_BODY, fx_body);
    }

    #[test]
    fn replay_yield_inactive() {
        let (fx_status, fx_body) = call_body("yield_inactive");
        assert_eq!(409, fx_status);
        assert_eq!(RUN_NOT_ACTIVE_BODY, fx_body);
    }

    #[test]
    fn retick_with_ticker_shape() {
        let ticker = ticker_row();
        let (status, body) = retick_response(true, "granted", None, Some((&ticker, 10)));
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            body,
            r#"{"granted":true,"reason":"granted","run_id":null,"used":3,"tick_count":3,"max_ticks":22,"enabled":true,"pending_entry":false,"next_run_at":"2026-10-04T12:34:56.789012+00:00"}"#
        );
        let mut no_time = ticker_row();
        no_time.next_run_at = None;
        let (_, body) = retick_response(true, "granted", None, Some((&no_time, 10)));
        assert!(body.ends_with(r#""next_run_at":null}"#), "{body}");
    }

    #[test]
    fn wait_with_ticker_shape() {
        let ticker = ticker_row();
        let (status, body) = wait_response(true, "waited", Some((&ticker, 10)));
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            body,
            r#"{"applied":true,"reason":"waited","used":3,"granted":10,"waited":2,"cap":22,"wait_allowance_remaining":8}"#
        );
        let (_, body) = wait_response(false, "infinite_pool", Some((&ticker, -1)));
        assert_eq!(
            body,
            r#"{"applied":false,"reason":"infinite_pool","used":3,"granted":10,"waited":2,"cap":-1,"wait_allowance_remaining":0}"#
        );
    }

    #[test]
    fn run_ai_refusal_table() {
        for (reason, message) in [
            (
                Some("active_run_exists"),
                "the work item already has an active or queued run",
            ),
            (Some("no_pod"), "no pod is available to run this work item"),
            (
                Some("no_eligible_runner"),
                "no eligible runner or execution principal is available for this work item",
            ),
            (Some("something-new"), "could not dispatch a run"),
        ] {
            let (status, body) = run_ai_refusal(reason);
            assert_eq!(status, StatusCode::CONFLICT);
            assert_eq!(
                body,
                format!(
                    "{{\"error\":{message},\"reason\":{reason}}}",
                    message = serde_json::to_string(message).expect("str"),
                    reason = serde_json::to_string(&reason).expect("str"),
                )
            );
        }
    }

    #[test]
    fn run_ai_refusal_none_reason_renders_null() {
        // `dispatch_run_ai_run_with_reason` returns `(None, None)` when
        // creation refuses after preflight passed; the view renders
        // `{"error": <default>, "reason": None}` (`:1238-1244`).
        let (status, body) = run_ai_refusal(None);
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(
            body,
            r#"{"error":"could not dispatch a run","reason":null}"#
        );
    }

    #[test]
    fn run_ai_created_shape() {
        let id: Uuid = "640d8cdf-08fb-4772-82d3-2c693d0e105f"
            .parse()
            .expect("uuid");
        let (status, body) = run_ai_created(id, "queued", "local_runner");
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(
            body,
            r#"{"id":"640d8cdf-08fb-4772-82d3-2c693d0e105f","status":"queued","executor":"local_runner"}"#
        );
    }

    #[test]
    fn activate_timezone_empty_zone_500s() {
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
        assert_eq!(activate_timezone(None).expect("none"), chrono_tz::UTC);
        assert_eq!(activate_timezone(Some("UTC")).expect("utc"), chrono_tz::UTC);
    }

    #[test]
    fn truncate_note_edges() {
        assert_eq!(truncate_note(""), None);
        assert_eq!(truncate_note("   "), None);
        assert_eq!(truncate_note("  hi  "), Some("hi".to_owned()));
        let long = "é".repeat(3000);
        let cut = truncate_note(&long).expect("cut");
        assert_eq!(cut.chars().count(), 2000);
        assert!(cut.is_char_boundary(cut.len()));
        let short = "x".repeat(2000);
        assert_eq!(truncate_note(&short), Some(short));
    }
}
