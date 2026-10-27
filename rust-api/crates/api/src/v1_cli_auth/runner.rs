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
use sqlx::PgPool;
use uuid::Uuid;

use pidash_auth::permissions::runner as runner_perm;
use pidash_auth::scope::TenantScope;
use pidash_auth::token as token_kernel;
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

fn redis_client(state: &AppState) -> Option<redis::Client> {
    state
        .settings()
        .redis
        .url
        .as_deref()
        .filter(|url| !url.is_empty())
        .and_then(|url| pidash_db::redis::open_client(url).ok())
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

/// One `api_tokens` row for the lookup: `(id, user_id, is_active,
/// expired_at)`.
type ApiTokenLookupRow = (Uuid, Uuid, bool, Option<DateTime<Utc>>);

/// `validate_api_token` (`api_authentication.py:30-43`): exact token
/// match, `is_active`, unexpired (`expired_at__gt=now` or null) — then
/// stamp `last_used`. The `deleted_at IS NULL` predicate is the
/// `SoftDeleteModel` manager Python filters through.
async fn authenticate_api(pool: &PgPool, presented: &str) -> Result<Caller, Response> {
    let now = Utc::now();
    let row: Option<ApiTokenLookupRow> = sqlx::query_as(
        r#"SELECT id, user_id, is_active, expired_at FROM api_tokens WHERE token = $1 AND deleted_at IS NULL"#,
    )
    .bind(presented)
    .fetch_optional(pool)
    .await
    .map_err(|_| server_error())?;
    let Some((id, user_id, is_active, expired_at)) = row else {
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
    pidash_services::domain::tokens::touch_api_token_last_used_by_id(pool, id, now)
        .await
        .map_err(|_| server_error())?;
    // The token loggers stamp this id on the `process_logs` payload
    // (Django reads `request.user` post-view; PIDASHCONV-937).
    crate::middleware::publish_logger_user_id(&user_id.to_string());
    Ok(Caller { user_id })
}

/// One `machine_token` row for the lookup, with the linked dev-machine
/// revocation flag: `(id, user_id, workspace_id, revoked_at,
/// dev_machine_id, dev_machine_revoked, dev_machine_found)`.
type MachineTokenLookupRow = (
    Uuid,
    Uuid,
    Uuid,
    Option<DateTime<Utc>>,
    Option<Uuid>,
    Option<DateTime<Utc>>,
    Option<Uuid>,
);

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
    let row: Option<MachineTokenLookupRow> = sqlx::query_as(
        r#"SELECT mt.id, mt.user_id, mt.workspace_id, mt.revoked_at,
                  mt.dev_machine_id, dm.revoked_at, dm.id
           FROM machine_token mt
           LEFT JOIN dev_machine dm ON dm.id = mt.dev_machine_id
           WHERE mt.token_hash = $1"#,
    )
    .bind(&token_hash)
    .fetch_optional(pool)
    .await
    .map_err(|_| server_error())?;
    let Some((id, user_id, workspace_id, revoked_at, dev_machine_id, dm_revoked, dm_found)) = row
    else {
        return Err(invalid_token());
    };
    if revoked_at.is_some() {
        return Err(invalid_token());
    }
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
        pidash_services::domain::tokens::revoke_machine_token_by_id(pool, id, now)
            .await
            .map_err(|_| server_error())?;
        return Err(invalid_token());
    }
    // `MachineToken.objects.filter(pk=...).update(last_used_at=now)`.
    pidash_services::domain::tokens::touch_machine_token_last_used_by_id(pool, id, now)
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
    let exists: Option<bool> = sqlx::query_scalar(
        r#"SELECT EXISTS(SELECT 1 FROM workspace_members
           WHERE workspace_id = $1 AND member_id = $2 AND is_active AND deleted_at IS NULL)"#,
    )
    .bind(workspace_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| server_error())?;
    Ok(exists.unwrap_or(false))
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

/// `DELETE /api/v1/runners/<runner_id>/`: by-pk 404, view-guard 404,
/// manage-guard 403, flag-parse 400, then the delete service and an
/// The revoke runtime facts: the pool, the api-owned Redis client,
/// and runner settings. Frames and close run on the revoke tx (see
/// the jobs module docs).
fn revoke_ctx(pool: &PgPool, state: &AppState) -> RevokeCtx {
    RevokeCtx {
        pool: pool.clone(),
        redis: redis_client(state),
        runner: state.settings().runner.clone(),
        atomic_effects: true,
    }
}

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
    )
    .await
    .is_err()
    {
        return server_error();
    }
    no_content()
}
