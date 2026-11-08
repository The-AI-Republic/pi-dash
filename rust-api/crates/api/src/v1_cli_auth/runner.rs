//! `DELETE /api/v1/runners/<runner_id>/` — CLI-friendly cascade delete.
//!
//! Ports `RunnerDeleteEndpoint.delete` (`api/views/runner.py:44-57`)
//! with its guards (`runner/services/permissions.py:70-77,126-137`),
//! the `purge_local` flag (`runner/services/runner_delete.py:146-163`)
//! and the delete service (`runner_delete.py:47-95`, via the D-13
//! drivers — never inlined).
//!
//! Handler order (preserved, not redesigned): UUID resolve (garbage →
//! JSON 404, parseable-but-non-canonical → proxied to Django) → auth (401
//! anonymous, 403 bad token) → by-pk lookup (404) → `can_view` (404) →
//! `can_manage` (403) → flag parse (400) → `delete_runner` → 204 empty.
//! Guards run before the flag parse, so an unviewable runner with a bad
//! flag answers 404, not 400.
//!
//! Layering (all read-only): the by-pk lookup is
//! [`runner_lookup`](pidash_db::v1_cli_auth::queries::runner_lookup)
//! (PIDASHCONV-533); the guards run through the
//! [`runner`](pidash_auth::permissions::runner) kernel; the flag parses
//! via [`purge`](pidash_services::runner_enroll::purge) (PIDASHCONV-587);
//! the delete runs the [`delete`](pidash_services::runner_enroll::delete)
//! driver (PIDASHCONV-588) through the jobs revoke service
//! ([`revoke`](pidash_jobs::runner_enroll::revoke)), which owns the
//! pool store, the tx execution, and the post-commit fire.
//! Auth mirrors `device_session.rs` (401/403 DRF rendering); the
//! `api_tokens` / `workspace_members` lookups keep the managers'
//! `deleted_at IS NULL` predicate (the `runner_enroll::auth`
//! closer-to-Python precedent).
//!
//! Notes for the reviewer, each one line: `ApiKeyRateThrottle` is
//! cross-cutting infra no merged v1 handler enforces
//! (verified-not-ported, the `v1_projects::perms` precedent) —
//! `TimezoneMixin` renders no datetime here — the UUID parses before
//! auth because Django's `<uuid:>` converter rejects before any view
//! code — `is_workspace_admin` passes `false` with no query because the
//! kernel reads it only past the view gate, which non-PRIVATE rows never
//! pass (Python short-circuits identically, so no query fires there
//! either) — the frame/close store methods mirror the D-14 pubsub
//! drivers call-for-call on the open transaction instead of taking
//! `&PubsubStore`, whose `&self` shape cannot hold the `&mut` tx.
//!
//! Approximations: a garbage segment answers the view's JSON 404 —
//! Django's converter 404s with its HTML page, so only the status is
//! exact (the `runner_enroll::manage` precedent) — while a
//! parseable-but-non-canonical UUID proxies to Django so the resolver
//! 404 answers byte for byte (the `app_issues` PB-0340 precedent) —
//! unhandled failures answer the JSON 500 while Django renders HTML
//! (status-exact, the `runner_runs` precedent).
//!
//! Fixture ids: V1CLIAUTH-F1 (branches + flag mapping), V1CLIAUTH-F4
//! (guard truth tables, handler-expected behavior over the kernel).

// Every handler returns a fully-rendered `Response` by design (the
// intake `parse_body` precedent, which carries the same allow).
#![allow(clippy::result_large_err)]

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, Method, StatusCode, Uri};
use axum::response::Response;
use bytes::Bytes;
use chrono::{DateTime, Utc};
use sea_orm::sea_query::Expr;
use sea_orm::{
    ColumnTrait, EntityTrait, FromQueryResult, JoinType, PaginatorTrait, QueryFilter, QuerySelect,
};
use sqlx::PgPool;
use uuid::Uuid;

use pidash_auth::permissions::runner as runner_perm;
use pidash_auth::scope::TenantScope;
use pidash_auth::token as token_kernel;
use pidash_db::entity::{
    agent_run, api_tokens, dev_machine, issue_assignees, issues, machine_token, workspace_members,
    workspaces,
};
use pidash_db::redis::client_from_settings;
use pidash_db::v1_cli_auth::queries::runner_lookup;
use pidash_jobs::runner_enroll::{fire_revoke_bundle, revoke, RevokeCtx, RevokeScope};
use pidash_services::runner_enroll::purge;
use pidash_services::runner_runs::LifecycleEffect;
use pidash_types::{UserId, WorkspaceId};

use crate::license::{query_last, QueryMap, UNAUTHENTICATED_BODY};
use crate::runner_runs::run_endpoints::drain_lifecycle_effects;
use crate::runner_runs::LivePorts;
use crate::state::AppState;
use crate::uuid_path::is_canonical_uuid;
use pidash_compat::response::{json_response, no_content, server_error};

/// `{"error": "not found"}` — 404 (unknown id, or present-but-unviewable).
const NOT_FOUND_BODY: &str = r#"{"error":"not found"}"#;
/// `{"error": "forbidden"}` — 403 (viewable but not manageable;
/// unreachable with today's single PRIVATE visibility, ported anyway).
const FORBIDDEN_BODY: &str = r#"{"error":"forbidden"}"#;
/// `{"error":"workspace not found"}` — 404 unknown workspace slug (list).
const WORKSPACE_NOT_FOUND_BODY: &str = r#"{"error":"workspace not found"}"#;
/// DRF renders `AuthenticationFailed("Given API token is not valid")` as
/// 403 here (no `authenticate_header` on the class, so the 401 coerces;
/// probed live — the `device_session.rs` precedent).
const INVALID_TOKEN_BODY: &str = r#"{"detail":"Given API token is not valid"}"#;

fn not_found() -> Response {
    json_response(StatusCode::NOT_FOUND, NOT_FOUND_BODY.to_owned())
}

fn forbidden() -> Response {
    json_response(StatusCode::FORBIDDEN, FORBIDDEN_BODY.to_owned())
}

fn workspace_not_found() -> Response {
    json_response(StatusCode::NOT_FOUND, WORKSPACE_NOT_FOUND_BODY.to_owned())
}

fn unauthorized() -> Response {
    json_response(StatusCode::UNAUTHORIZED, UNAUTHENTICATED_BODY.to_owned())
}

fn invalid_token() -> Response {
    json_response(StatusCode::FORBIDDEN, INVALID_TOKEN_BODY.to_owned())
}

/// 400 `{"error": str(exc)}` (`views/runner.py:55`): the message is the
/// services const, so the handler cannot drift from the parser.
fn bad_purge_flag() -> Response {
    json_response(
        StatusCode::BAD_REQUEST,
        format!(
            "{{\"error\":{}}}",
            serde_json::to_string(purge::PURGE_LOCAL_ERROR).expect("static error serializes")
        ),
    )
}

fn pool_of(state: &AppState) -> Result<PgPool, Response> {
    state
        .pools()
        .map(|pools| pools.primary().clone())
        .ok_or_else(server_error)
}

/// Django-exact clock: Python datetimes are microsecond-exact, while
/// `Utc::now()` carries nanos Postgres would round on store (the
/// `runner_enroll::manage` precedent).
fn now_micros() -> DateTime<Utc> {
    let now = Utc::now();
    DateTime::from_timestamp_micros(now.timestamp_micros()).expect("micros in range")
}

// ---------------------------------------------------------------------------
// API-key authentication (`api_authentication.py:20-84`)
// ---------------------------------------------------------------------------

/// The authenticated caller. Only the user id survives: the guards read
/// ownership off the runner row, like the view reads `request.user`.
struct Caller {
    user_id: Uuid,
}

/// `APIKeyAuthentication.authenticate`: the `X-Api-Key` header carries an
/// `APIToken` or, when it starts with `mt_`, a `MachineToken`. No usable
/// credential is DRF's `NotAuthenticated` (401); a bad one is
/// `AuthenticationFailed` (403 here).
async fn authenticate(
    pool: &PgPool,
    secret_key: &[u8],
    headers: &HeaderMap,
) -> Result<Caller, Response> {
    // The 401 arm runs before any pool access in the caller; reaching
    // here with no key is impossible, but the check stays total.
    let presented = headers
        .get(token_kernel::API_KEY_HEADER)
        .map(|raw| raw.to_str().unwrap_or("\0"))
        .unwrap_or("");
    // Django decodes headers lossily to `str`, so undecodable bytes are
    // a lookup miss (403), never a missing credential (401); `"\0"` can
    // never match a stored token (the `device_session.rs` precedent).
    if presented.is_empty() {
        return Err(unauthorized());
    }
    if presented.starts_with(token_kernel::MACHINE_TOKEN_PREFIX) {
        authenticate_machine(pool, secret_key, presented).await
    } else {
        authenticate_api(pool, presented).await
    }
}

/// The `api_tokens` columns the validator reads (liveness, expiry).
/// `token` is the presented value, not read back.
#[derive(Debug, FromQueryResult)]
struct ApiTokenLookup {
    id: Uuid,
    user_id: Uuid,
    is_active: bool,
    expired_at: Option<DateTime<Utc>>,
}

/// The live `api_tokens` row with exactly this token (`deleted_at IS NULL`,
/// the `SoftDeletionManager` scope): a soft-deleted token is a miss.
async fn live_api_token(
    pool: &PgPool,
    presented: &str,
) -> Result<Option<ApiTokenLookup>, Response> {
    let db = pidash_db::pool::sea_orm_connection(pool);
    api_tokens::Entity::find()
        .select_only()
        .columns([
            api_tokens::Column::Id,
            api_tokens::Column::UserId,
            api_tokens::Column::IsActive,
            api_tokens::Column::ExpiredAt,
        ])
        .filter(api_tokens::Column::Token.eq(presented))
        .filter(api_tokens::Column::DeletedAt.is_null())
        .into_model::<ApiTokenLookup>()
        .one(&db)
        .await
        .map_err(|_| server_error())
}

/// `validate_api_token` (`api_authentication.py:30-43`): exact token
/// match, `is_active`, unexpired (`expired_at__gt=now` or null) — then
/// stamp `last_used`. The `deleted_at IS NULL` predicate is the
/// `SoftDeleteModel` manager Python filters through.
async fn authenticate_api(pool: &PgPool, presented: &str) -> Result<Caller, Response> {
    let now = Utc::now();
    let row = live_api_token(pool, presented).await?;
    let Some(ApiTokenLookup {
        id,
        user_id,
        is_active,
        expired_at,
    }) = row
    else {
        return Err(invalid_token());
    };
    if !is_active {
        return Err(invalid_token());
    }
    if let Some(expires) = expired_at {
        if expires <= now {
            return Err(invalid_token());
        }
    }
    // `api_token.last_used = now; save(update_fields=["last_used"])`.
    pidash_services::domain::tokens::touch_api_token_last_used_by_id(
        &pidash_db::pool::sea_orm_connection(pool),
        id,
        now,
    )
    .await
    .map_err(|_| server_error())?;
    // The token loggers stamp this id on the `process_logs` payload
    // (Django reads `request.user` post-view; PIDASHCONV-937).
    crate::middleware::publish_logger_user_id(&user_id.to_string());
    Ok(Caller { user_id })
}

/// One `machine_token` row for the lookup, with the linked dev-machine
/// revocation state (all three come from the `LEFT JOIN`: a token with
/// no device reads `None` for each).
#[derive(Debug, FromQueryResult)]
struct MachineTokenLookup {
    id: Uuid,
    user_id: Uuid,
    workspace_id: Uuid,
    revoked_at: Option<DateTime<Utc>>,
    expires_at: Option<DateTime<Utc>>,
    dev_machine_id: Option<Uuid>,
    dm_revoked: Option<DateTime<Utc>>,
    dm_found: Option<Uuid>,
}

/// `machine_token` by hash, LEFT JOIN `dev_machine` for its id and
/// revocation (`api_authentication.py:47-70`). No liveness scope: the
/// tables have no soft delete.
async fn machine_token_with_device(
    pool: &PgPool,
    token_hash: &str,
) -> Result<Option<MachineTokenLookup>, Response> {
    let db = pidash_db::pool::sea_orm_connection(pool);
    machine_token::Entity::find()
        .select_only()
        .columns([
            machine_token::Column::Id,
            machine_token::Column::UserId,
            machine_token::Column::WorkspaceId,
            machine_token::Column::RevokedAt,
            machine_token::Column::ExpiresAt,
            machine_token::Column::DevMachineId,
        ])
        .column_as(
            Expr::col((dev_machine::Entity, dev_machine::Column::RevokedAt)),
            "dm_revoked",
        )
        .column_as(
            Expr::col((dev_machine::Entity, dev_machine::Column::Id)),
            "dm_found",
        )
        .join(
            JoinType::LeftJoin,
            machine_token::Entity::belongs_to(dev_machine::Entity)
                .from(machine_token::Column::DevMachineId)
                .to(dev_machine::Column::Id)
                .into(),
        )
        .filter(machine_token::Column::TokenHash.eq(token_hash))
        .into_model::<MachineTokenLookup>()
        .one(&db)
        .await
        .map_err(|_| server_error())
}

/// `validate_machine_token` (`api_authentication.py:45-63`): match on
/// `token_hash`, unrevoked, dev-machine unrevoked, workspace member (a
/// non-member's token is revoked, then rejected) — then stamp
/// `last_used_at`. `machine_token` / `dev_machine` are plain
/// `models.Model` rows: no `deleted_at` predicate exists here.
async fn authenticate_machine(
    pool: &PgPool,
    secret_key: &[u8],
    presented: &str,
) -> Result<Caller, Response> {
    let now = Utc::now();
    let token_hash = token_kernel::hash_token(presented, secret_key);
    let row = machine_token_with_device(pool, &token_hash).await?;
    let Some(MachineTokenLookup {
        id,
        user_id,
        workspace_id,
        revoked_at,
        expires_at,
        dev_machine_id,
        dm_revoked,
        dm_found,
    }) = row
    else {
        return Err(invalid_token());
    };
    if revoked_at.is_some() {
        return Err(invalid_token());
    }
    if expires_at.is_some_and(|expires| expires <= now) {
        return Err(invalid_token());
    }
    // 60-day idle expiry (NEWPIDASH-50): `MachineToken.is_expired` is
    // the generic refusal on this opaque `X-Api-Key` surface, checked
    // after revoked and before dev-machine revocation.
    // `machine_token.dev_machine_id is not None and
    // machine_token.dev_machine.revoked_at is not None`. A set id with no
    // row is the `select_related` 500 (unreachable without FK surgery).
    if let Some(machine_id) = dev_machine_id {
        if dm_found != Some(machine_id) {
            return Err(server_error());
        }
        if dm_revoked.is_some() {
            return Err(invalid_token());
        }
    }
    if !is_member(pool, workspace_id, user_id).await? {
        // `machine_token.revoke()` on the way out (`revoked_at` is `None`
        // here, so the guard is a no-op and the write always lands).
        pidash_services::domain::tokens::revoke_machine_token_by_id(
            &pidash_db::pool::sea_orm_connection(pool),
            id,
            now,
        )
        .await
        .map_err(|_| server_error())?;
        return Err(invalid_token());
    }
    // `MachineToken.objects.filter(pk=...).update(last_used_at=now)`.
    pidash_services::domain::tokens::touch_machine_token_last_used_by_id(
        &pidash_db::pool::sea_orm_connection(pool),
        id,
        now,
    )
    .await
    .map_err(|_| server_error())?;
    // The token loggers stamp this id on the `process_logs` payload
    // (Django reads `request.user` post-view; PIDASHCONV-937).
    crate::middleware::publish_logger_user_id(&user_id.to_string());
    Ok(Caller { user_id })
}

/// `is_workspace_member(user, workspace_id)`
/// (`core/permissions.py:28-34`): an active, un-deleted membership row
/// for `(workspace, member)` (the manager's `deleted_at IS NULL`
/// included, like the `v1_projects` copy).
async fn is_member(pool: &PgPool, workspace_id: Uuid, user_id: Uuid) -> Result<bool, Response> {
    let db = pidash_db::pool::sea_orm_connection(pool);
    let hit: Option<Uuid> = workspace_members::Entity::find()
        .select_only()
        .column(workspace_members::Column::Id)
        .filter(workspace_members::Column::WorkspaceId.eq(workspace_id))
        .filter(workspace_members::Column::MemberId.eq(user_id))
        .filter(workspace_members::Column::IsActive.eq(true))
        .filter(workspace_members::Column::DeletedAt.is_null())
        .limit(1)
        .into_tuple()
        .one(&db)
        .await
        .map_err(|_| server_error())?;
    Ok(hit.is_some())
}

// ---------------------------------------------------------------------------
// Handler: GET /api/v1/workspaces/<slug>/runners/ (NEWPIDASH-146, f92fe3c0)
// ---------------------------------------------------------------------------

/// Busy statuses (`matcher.py: BUSY_STATUSES`): statuses that occupy the runner.
const BUSY_STATUSES: &[&str] = &[
    "assigned",
    "waiting_for_worktree",
    "running",
    "cancel_requested",
    "awaiting_approval",
    "awaiting_reauth",
];
/// Blocking statuses for E8 (`NON_TERMINAL minus QUEUED/PAUSED_AWAITING_INPUT`).
const E8_BLOCKING_STATUSES: &[&str] = &[
    "assigned",
    "waiting_for_worktree",
    "running",
    "cancel_requested",
    "awaiting_approval",
    "awaiting_reauth",
];

/// `Workspace.objects.filter(slug=slug).first()` with the manager's
/// `deleted_at IS NULL` scope.
async fn workspace_by_slug(pool: &PgPool, slug: &str) -> Result<Option<Uuid>, Response> {
    let db = pidash_db::pool::sea_orm_connection(pool);
    let row: Option<workspaces::Model> = workspaces::Entity::find()
        .select_only()
        .column(workspaces::Column::Id)
        .filter(workspaces::Column::Slug.eq(slug))
        .filter(workspaces::Column::DeletedAt.is_null())
        .one(&db)
        .await
        .map_err(|_| server_error())?;
    Ok(row.map(|m| m.id))
}

/// `is_busy` (`issue_assignment.py:65-70`): any run the runner is serving
/// in BUSY_STATUSES.
async fn runner_is_busy(pool: &PgPool, runner_id: Uuid) -> Result<bool, Response> {
    let db = pidash_db::pool::sea_orm_connection(pool);
    let hit: Option<Uuid> = agent_run::Entity::find()
        .select_only()
        .column(agent_run::Column::Id)
        .filter(agent_run::Column::RunnerId.eq(runner_id))
        .filter(agent_run::Column::Status.is_in(BUSY_STATUSES.to_vec()))
        .limit(1)
        .into_tuple()
        .one(&db)
        .await
        .map_err(|_| server_error())?;
    Ok(hit.is_some())
}

/// `queue_depth` (`issue_assignment.py:58-63`): QUEUED runs pinned to the runner.
async fn runner_queue_depth(pool: &PgPool, runner_id: Uuid) -> Result<i64, Response> {
    let db = pidash_db::pool::sea_orm_connection(pool);
    let count = agent_run::Entity::find()
        .filter(agent_run::Column::PinnedRunnerId.eq(runner_id))
        .filter(agent_run::Column::Status.eq("queued"))
        .count(&db)
        .await
        .map_err(|_| server_error())?;
    Ok(count as i64)
}

/// Evaluate E1-E8 (sans E6) for one runner against the list context.
/// Mirrors `_check_assignment_code` in `issue_assignment.py`.
#[allow(clippy::too_many_arguments)]
async fn assignable_code(
    pool: &PgPool,
    caller_user_id: Uuid,
    _runner_id: Uuid,
    runner_owner_id: Uuid,
    runner_status: &str,
    runner_revoked_at: Option<chrono::DateTime<chrono::Utc>>,
    runner_pod_project_id: Uuid,
    issue: Option<&issues::Model>,
) -> Result<&'static str, Response> {
    // E1: can_use_runner — today owns the runner.
    if runner_owner_id != caller_user_id {
        return Ok("runner_not_found");
    }
    // E2: pod.project matches issue.project (issue=None skips the check).
    if let Some(issue) = issue {
        if runner_pod_project_id != issue.project_id {
            return Ok("runner_wrong_project");
        }
    }
    // E4: revoked.
    if runner_status == "revoked" || runner_revoked_at.is_some() {
        return Ok("runner_revoked");
    }
    // E7: owner is creator or current assignee of the issue.
    if let Some(issue) = issue {
        let mut people: Vec<Uuid> = Vec::new();
        if let Some(creator) = issue.created_by_id {
            people.push(creator);
        }
        let db = pidash_db::pool::sea_orm_connection(pool);
        let assignees: Vec<issue_assignees::Model> = issue_assignees::Entity::find()
            .filter(issue_assignees::Column::IssueId.eq(issue.id))
            .filter(issue_assignees::Column::DeletedAt.is_null())
            .all(&db)
            .await
            .map_err(|_| server_error())?;
        for row in assignees {
            if !people.contains(&row.assignee_id) {
                people.push(row.assignee_id);
            }
        }
        if !people.contains(&runner_owner_id) {
            return Ok("runner_owner_not_on_issue");
        }
    }
    // E8: no blocking run on the issue.
    if let Some(issue) = issue {
        let db = pidash_db::pool::sea_orm_connection(pool);
        let hit: Option<Uuid> = agent_run::Entity::find()
            .select_only()
            .column(agent_run::Column::Id)
            .filter(agent_run::Column::WorkItemId.eq(issue.id))
            .filter(agent_run::Column::Status.is_in(E8_BLOCKING_STATUSES.to_vec()))
            .limit(1)
            .into_tuple()
            .one(&db)
            .await
            .map_err(|_| server_error())?;
        if hit.is_some() {
            return Ok("active_run_in_progress");
        }
    }
    Ok("")
}

/// `GET /api/v1/workspaces/<slug>/runners/` (`api/views/runner.py:68-116`).
/// CLI-friendly token list: workspace 404, membership 403, visible runners
/// in `-updated_at` order, optional `?project=` narrowing and
/// `?assignable_for=` eligibility fields. Handler order mirrors Django:
/// auth (401/403) → workspace lookup (404) → membership (403) → list.
pub async fn list_runners(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    Query(query): Query<QueryMap>,
    headers: HeaderMap,
) -> Response {
    if !headers
        .get(token_kernel::API_KEY_HEADER)
        .is_some_and(|value| !value.as_bytes().is_empty())
    {
        return unauthorized();
    }
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(resp) => return resp,
    };
    let caller = match authenticate(&pool, state.settings().secret_key.as_bytes(), &headers).await {
        Ok(c) => c,
        Err(resp) => return resp,
    };
    let workspace_id = match workspace_by_slug(&pool, &slug).await {
        Ok(Some(id)) => id,
        Ok(None) => return workspace_not_found(),
        Err(resp) => return resp,
    };
    let member = match is_member(&pool, workspace_id, caller.user_id).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    if !member {
        return forbidden();
    }
    // ?project= narrows to the project's pods. Empty/missing means no
    // filter; a non-UUID value narrows to nothing (Django filters by raw
    // string, matching no UUID rows).
    let project_filter: Option<Uuid> = match query_last(&query, "project") {
        Some(raw) if !raw.is_empty() => match raw.parse::<Uuid>() {
            Ok(id) => Some(id),
            _ => {
                // Bogus project: return empty list (matches the contract
                // test's 00000000-... narrowing to nothing).
                return json_response(StatusCode::OK, "[]".to_owned());
            }
        },
        _ => None,
    };

    // Query runner rows: same visibility as Django's
    // `runner_visible_to_user_q` (owner_id + PRIVATE) and `-updated_at` order.
    // Reuse the manage_reads R1 query (workspace, owner, visibility, project).
    let db = pidash_db::pool::sea_orm_connection(&pool);
    let rows: Vec<pidash_services::runner_enroll::queries::manage_reads::RunnerJoinRow> =
        match pidash_services::runner_enroll::queries::manage_reads::runner_list(
            &db,
            workspace_id,
            caller.user_id,
            runner_perm::VISIBILITY_PRIVATE as i16,
            None,
            None,
            project_filter,
        )
        .await
        .map_err(|_| server_error())
        {
            Ok(v) => v,
            Err(resp) => return resp,
        };

    // ?assignable_for= presence gates the four extra fields (Django:
    // `if assignable_for is not None`).
    let assignable_for_present = query.contains_key("assignable_for");
    let issue_opt: Option<Option<issues::Model>> = if assignable_for_present {
        let raw = query_last(&query, "assignable_for").unwrap_or_default();
        if raw == "new" {
            Some(None)
        } else {
            match raw.parse::<Uuid>() {
                Ok(uuid) => {
                    let found: Option<issues::Model> = match issues::Entity::find()
                        .filter(issues::Column::Id.eq(uuid))
                        .filter(issues::Column::DeletedAt.is_null())
                        .one(&db)
                        .await
                        .map_err(|_| server_error())
                    {
                        Ok(v) => v,
                        Err(resp) => return resp,
                    };
                    Some(found)
                }
                Err(_) => Some(None),
            }
        }
    } else {
        None
    };
    let assignable_issue: Option<&issues::Model> = match &issue_opt {
        Some(Some(m)) => Some(m),
        Some(None) => None,
        None => None,
    };

    // Render each runner via the shapes kernel (full 31-field wire shape).
    // Live state is not prefetched (N+1 per row) — render null like Django
    // does when the row is missing.
    let mut bodies: Vec<String> = Vec::with_capacity(rows.len());
    for row in &rows {
        // Pre-render datetime strings for the shapes layer (already ISO-8601).
        let last_heartbeat_str = row
            .last_heartbeat_at
            .as_ref()
            .map(crate::serializer::render_datetime);
        let enrolled_str = row
            .enrolled_at
            .as_ref()
            .map(crate::serializer::render_datetime);
        let revoked_str = row
            .revoked_at
            .as_ref()
            .map(crate::serializer::render_datetime);
        let paused_str = row
            .paused_at
            .as_ref()
            .map(crate::serializer::render_datetime);
        let created_str = crate::serializer::render_datetime(&row.created_at);
        let updated_str = crate::serializer::render_datetime(&row.updated_at);

        let id_str = row.id.to_string();
        let owner_str = row.owner_id.to_string();
        let pod_str = row.pod_id.to_string();
        let pod_project_str = row.pod_project_id.to_string();
        let pod_project_id = row.pod_project_id;
        let dev_machine_str = row.dev_machine_id.map(|id| id.to_string());

        // Derived flags mirroring `RunnerSerializer` / `runner_render`.
        let is_engine = row.provisioning == "desktop_bundled";
        let engine_paused = if is_engine {
            let by_owner = row.dm_engine_paused_by_owner.unwrap_or(false);
            let reason = row.dm_engine_auto_pause_reason.as_deref().unwrap_or("");
            by_owner || !reason.is_empty()
        } else {
            false
        };
        let dispatch_paused = row.paused_at.is_some();
        let machine_display_name: String = if let Some(label) = row
            .dm_label
            .as_deref()
            .filter(|v| !v.is_empty())
            .map(|v| v.to_string())
        {
            label
        } else if let Some(host) = row
            .dm_host_label
            .as_deref()
            .filter(|v| !v.is_empty())
            .map(|v| v.to_string())
        {
            host
        } else if !row.host_label.is_empty() {
            row.host_label.clone()
        } else {
            row.name.clone()
        };

        // Build the base JSON object directly (31 fields, wire order) so
        // optional assignable fields can be injected without lifetime gymnastics.
        let pod_detail = serde_json::json!({
            "id": pod_str,
            "name": row.pod_name,
            "is_default": row.pod_is_default,
            "project": pod_project_str,
            "project_identifier": row.project_identifier,
        });
        let dev_machine_detail = match row.dm_id {
            Some(dm_id) => {
                let dm_expires = row
                    .dm_engine_credential_expires_at
                    .as_ref()
                    .map(crate::serializer::render_datetime);
                serde_json::json!({
                    "id": dm_id.to_string(),
                    "host_label": row.dm_host_label.clone().unwrap_or_default(),
                    "label": row.dm_label.clone().unwrap_or_default(),
                    "engine_paused_by_owner": row.dm_engine_paused_by_owner.unwrap_or(false),
                    "engine_auto_pause_reason": row.dm_engine_auto_pause_reason.clone().unwrap_or_default(),
                    "engine_team_runs": row.dm_engine_team_runs.unwrap_or(false),
                    "engine_sign_in_state": row.dm_engine_sign_in_state.clone().unwrap_or_else(|| "never".to_owned()),
                    "engine_credential_expires_at": dm_expires,
                })
            }
            None => serde_json::Value::Null,
        };

        let mut obj = serde_json::json!({
            "id": id_str,
            "name": row.name,
            "status": row.status,
            "host_label": row.host_label,
            "is_engine": is_engine,
            "engine_paused": engine_paused,
            "dispatch_paused": dispatch_paused,
            "machine_display_name": machine_display_name,
            "provisioning": row.provisioning,
            "pod_dispatch": row.pod_dispatch,
            "os": row.os,
            "arch": row.arch,
            "runner_version": row.runner_version,
            "dev_metadata": row.dev_metadata,
            "protocol_version": i64::from(row.protocol_version),
            "capabilities": row.capabilities,
            "last_heartbeat_at": last_heartbeat_str,
            "owner": owner_str,
            "dev_machine": dev_machine_str,
            "dev_machine_detail": dev_machine_detail,
            "visibility": i64::from(row.visibility),
            "pod": pod_str,
            "pod_detail": pod_detail,
            "live_state": serde_json::Value::Null,
            "enrolled_at": enrolled_str,
            "revoked_at": revoked_str,
            "revoked_reason": row.revoked_reason,
            "paused_at": paused_str,
            "paused_by": row.paused_by_id.map(|id| id.to_string()),
            "created_at": created_str,
            "updated_at": updated_str,
        });

        if assignable_for_present {
            let code = match assignable_code(
                &pool,
                caller.user_id,
                row.id,
                row.owner_id,
                &row.status,
                row.revoked_at,
                pod_project_id,
                assignable_issue,
            )
            .await
            {
                Ok(v) => v,
                Err(resp) => return resp,
            };
            let busy = match runner_is_busy(&pool, row.id).await {
                Ok(v) => v,
                Err(resp) => return resp,
            };
            let depth = match runner_queue_depth(&pool, row.id).await {
                Ok(v) => v,
                Err(resp) => return resp,
            };
            if let serde_json::Value::Object(ref mut map) = obj {
                map.insert("assignable".to_owned(), serde_json::json!(code.is_empty()));
                map.insert("not_assignable_code".to_owned(), serde_json::json!(code));
                map.insert("is_busy".to_owned(), serde_json::json!(busy));
                map.insert("queue_depth".to_owned(), serde_json::json!(depth));
            }
        }
        bodies.push(serde_json::to_string(&obj).expect("runner JSON serializes"));
    }
    json_response(StatusCode::OK, format!("[{}]", bodies.join(",")))
}

// ---------------------------------------------------------------------------
// Handler (`views/runner.py:44-57`)
// ---------------------------------------------------------------------------

/// Rebuild the inbound request and proxy it to Django (non-canonical
/// UUID spellings: routing precedes auth, so Django's resolver 404
/// answers before any 401 — the `v1_cycles_modules` precedent).
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

/// The revoke runtime facts: the pool, the api-owned Redis client,
/// and runner settings. Frames and close run on the revoke tx (see
/// the jobs module docs).
fn revoke_ctx(pool: &PgPool, state: &AppState) -> RevokeCtx {
    RevokeCtx {
        pool: pool.clone(),
        redis: client_from_settings(state.settings()),
        runner: state.settings().runner.clone(),
        atomic_effects: true,
    }
}

/// `DELETE /api/v1/runners/<runner_id>/`: by-pk 404, view-guard 404,
/// manage-guard 403, flag-parse 400, then the delete service and an
/// empty 204.
pub async fn delete_runner(
    State(state): State<AppState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    Path(raw_id): Path<String>,
    Query(query): Query<QueryMap>,
    body: Bytes,
) -> Response {
    // Resolver-level: Django's `<uuid:>` converter rejects before any
    // authentication runs, so the segment classifies before the 401 arm:
    // garbage keeps the view's JSON 404 (status-exact, pinned by the
    // unit test), while a parseable-but-non-canonical spelling proxies
    // to Django so its resolver 404 answers byte for byte.
    let runner_id: Uuid = match raw_id.parse() {
        Ok(runner_id) if is_canonical_uuid(&raw_id) => runner_id,
        Ok(_) => return proxy_through(state, method, uri, headers, body).await,
        Err(_) => return not_found(),
    };
    // Anonymous callers 401 before any pool or database access. Any
    // bytes count as presented (undecodable bytes are a 403 lookup
    // miss, never a missing credential — the `device_session.rs` rule).
    if !headers
        .get(token_kernel::API_KEY_HEADER)
        .is_some_and(|value| !value.as_bytes().is_empty())
    {
        return unauthorized();
    }
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(response) => return response,
    };
    let caller = match authenticate(&pool, state.settings().secret_key.as_bytes(), &headers).await {
        Ok(caller) => caller,
        Err(denial) => return denial,
    };
    let runner = match runner_lookup::find_by_pk(&pool, runner_id).await {
        Ok(runner) => runner,
        Err(_) => return server_error(),
    };
    let Some(runner) = runner else {
        return not_found();
    };
    let facts = runner_lookup::guard_facts(&runner);
    let owned = facts.owner_id == caller.user_id;
    if !runner_perm::can_view_runner(&runner_perm::RunnerFacts {
        workspace: WorkspaceId::from(runner.workspace_id.to_string()),
        authenticated: true,
        visibility: i32::from(facts.visibility),
        owned_by_requester: owned,
    }) {
        return not_found();
    }
    let workspace = WorkspaceId::from(runner.workspace_id.to_string());
    let scope = TenantScope::new(workspace.clone());
    if !runner_perm::can_manage_runner(
        &scope,
        &runner_perm::ManageFacts {
            workspace,
            requester: Some(UserId::from(caller.user_id.to_string())),
            visibility: i32::from(facts.visibility),
            owned_by_requester: owned,
            // The `:137` arm is unreachable: the kernel reads this flag
            // only past the view gate, which non-PRIVATE rows never
            // pass — and Python short-circuits identically, firing no
            // membership query on this path either.
            is_workspace_admin: false,
        },
    ) {
        return forbidden();
    }
    let purge_local =
        match purge::parse_purge_local(query_last(&query, "purge_local").as_deref(), true) {
            Ok(flag) => flag,
            Err(_) => return bad_purge_flag(),
        };
    let ctx = revoke_ctx(&pool, &state);
    let mut tx = match pool.begin().await {
        Ok(tx) => tx,
        Err(_) => return server_error(),
    };
    let collected = match revoke(
        &ctx,
        &mut tx,
        RevokeScope::DeleteRunner {
            runner_id,
            revoked_at: runner.revoked_at,
            purge_local,
            now: now_micros(),
        },
    )
    .await
    {
        Ok(collected) => collected,
        Err(error) => {
            tracing::warn!(%error, "v1_cli_auth delete store failure");
            return server_error();
        }
    };
    for warning in &collected.warnings {
        tracing::warn!("{warning}");
    }
    let bundle = collected.bundle;
    if tx.commit().await.is_err() {
        return server_error();
    }
    // Post-commit drain in registration order: the per-run
    // terminal-effects publications first, then the handoffs, the pod
    // drains, and the stream-cleanup scheduling.
    // API-token request: the crum user is the token owner.
    let terminal_created_by = Some(caller.user_id);
    let ports = LivePorts::new(pool.clone(), &state);
    let pool_ref = &pool;
    let ports_ref = &ports;
    let state_ref = &state;
    // Each lifecycle arm below never fails (it logs internally and
    // answers `Ok`), so the results discard, as the merged drain did;
    // only drains and cleanup propagate, as the server error.
    if fire_revoke_bundle(
        &ctx,
        bundle,
        |run_id| async move {
            let _ = drain_lifecycle_effects(
                pool_ref,
                ports_ref,
                vec![LifecycleEffect::PublishTerminalEffects { run_id }],
                terminal_created_by,
            )
            .await;
        },
        |run_id| async move {
            let _ = drain_lifecycle_effects(
                pool_ref,
                ports_ref,
                vec![LifecycleEffect::ApplyTerminalEffectsInline { run_id }],
                terminal_created_by,
            )
            .await;
        },
        |_, run_id| async move {
            let _ = drain_lifecycle_effects(
                pool_ref,
                ports_ref,
                vec![LifecycleEffect::CompleteProjectMoveHandoff { run_id }],
                terminal_created_by,
            )
            .await;
        },
        // §13.1: the ticking-issue bounce for a lost runner.
        |issue_id, runner_name, reason, triggered_by| async move {
            let name = runner_name.unwrap_or_else(|| "the dedicated runner".to_owned());
            let now = Utc::now();
            match crate::v1_work_items::handlers_actions::dispatch_deps_for_ids(
                state_ref,
                pool_ref,
                Uuid::nil(),
                issue_id,
                now.timestamp(),
            )
            .await
            {
                Ok((deps, _)) => {
                    if let Err(error) = pidash_jobs::orchestration::bounce_issue_no_eligible_runner(
                        pool_ref,
                        deps,
                        now,
                        0.0,
                        issue_id,
                        &triggered_by,
                        &reason,
                        &name,
                    )
                    .await
                    {
                        tracing::error!(%error, %issue_id, "dedicated-runner bounce failed");
                    }
                }
                Err(_) => {
                    tracing::error!(%issue_id, "dedicated-runner bounce: dispatch deps failed");
                }
            }
        },
        // §13.1 `fall_back` step 2: recreate in the default pod.
        |issue_id, _pod_id, trigger, creator| async move {
            let now = Utc::now();
            match crate::v1_work_items::handlers_actions::dispatch_deps_for_ids(
                state_ref,
                pool_ref,
                creator.unwrap_or(Uuid::nil()),
                issue_id,
                now.timestamp(),
            )
            .await
            {
                Ok((deps, matcher)) => {
                    if let Err(error) = pidash_jobs::orchestration::dispatch_continuation_run(
                        pool_ref, deps, &matcher, now, 0.0, issue_id, &trigger, creator, false,
                        false,
                    )
                    .await
                    {
                        tracing::error!(%error, %issue_id, "dedicated-runner recreate failed");
                    }
                }
                Err(_) => {
                    tracing::error!(%issue_id, "dedicated-runner recreate: dispatch deps failed");
                }
            }
        },
    )
    .await
    .is_err()
    {
        return server_error();
    }
    no_content()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 60-day idle expiry (`NEWPIDASH-50`): `MachineToken.is_expired`
    /// is the generic refusal on `X-Api-Key` `mt_` paths, checked
    /// after revoked and before dev-machine revocation.
    #[test]
    fn machine_token_expiry_is_checked() {
        let now = Utc::now();
        let past = now - chrono::Duration::seconds(1);
        let future = now + chrono::Duration::days(1);
        assert!(Some(past).is_some_and(|expires| expires <= now));
        assert!(!Some(future).is_some_and(|expires| expires <= now));
        assert!(!None.is_some_and(|expires: DateTime<Utc>| expires <= now));
        assert!(Some(now).is_some_and(|expires| expires <= now));
    }
}
