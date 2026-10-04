//! `DELETE /api/v1/runners/<runner_id>/` — CLI-friendly cascade delete.
//!
//! Ports `RunnerDeleteEndpoint.delete` (`api/views/runner.py:44-57`)
//! with its guards (`runner/services/permissions.py:70-77,126-137`),
//! the `purge_local` flag (`runner/services/runner_delete.py:146-163`)
//! and the delete service (`runner_delete.py:47-95`, via the D-13
//! drivers — never inlined).
//!
//! Handler order (preserved, not redesigned): UUID resolve → auth (401
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
//! driver (PIDASHCONV-588) on the pool-backed [`DeleteStore`] below.
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
//! Approximations: a non-UUID segment answers the view's JSON 404 —
//! Django's converter 404s with its HTML page, so only the status is
//! exact (the `runner_enroll::manage` precedent) — unhandled failures
//! answer the JSON 500 while Django renders HTML (status-exact, the
//! `runner_runs` precedent).
//!
//! Fixture ids: V1CLIAUTH-F1 (branches + flag mapping), V1CLIAUTH-F4
//! (guard truth tables, handler-expected behavior over the kernel).

// Every handler returns a fully-rendered `Response` by design (the
// intake `parse_body` precedent, which carries the same allow).
#![allow(clippy::result_large_err)]

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use pidash_auth::permissions::runner as runner_perm;
use pidash_auth::scope::TenantScope;
use pidash_auth::token as token_kernel;
use pidash_db::runner_sessions::models::runner_session as rs_models;
use pidash_db::runner_sessions::outbox;
use pidash_db::runner_sessions::outbox::OutboxError;
use pidash_db::v1_cli_auth::queries::runner_lookup;
use pidash_services::runner_enroll::{delete as delete_svc, purge, revoke};
use pidash_services::runner_runs::{finalization, LifecycleEffect, SetValue};
use pidash_services::runner_sessions::pubsub;
use pidash_types::dispatch::AgentExecutorKind;
use pidash_types::runner_runs::{AgentRunStatus, TERMINAL_RUN_STATUSES};
use pidash_types::runner_sessions::{remove_runner_frame, revoke_frame};
use pidash_types::{UserId, WorkspaceId};

use crate::license::{query_last, QueryMap, SERVER_ERROR_BODY, UNAUTHENTICATED_BODY};
use crate::runner_runs::run_endpoints::drain_lifecycle_effects;
use crate::runner_runs::LivePorts;
use crate::state::AppState;

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

fn server_error() -> Response {
    json_response(
        StatusCode::INTERNAL_SERVER_ERROR,
        SERVER_ERROR_BODY.to_owned(),
    )
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

fn json_response(status: StatusCode, body: String) -> Response {
    Response::builder()
        .status(status)
        .header(axum::http::header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body))
        .expect("handler response")
}

/// Empty 204 (`Response(status=204)` with no data; no content type).
fn no_content() -> Response {
    Response::builder()
        .status(StatusCode::NO_CONTENT)
        .body(axum::body::Body::empty())
        .expect("handler empty response")
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
        .and_then(|url| redis::Client::open(url).ok())
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
    sqlx::query(r#"UPDATE api_tokens SET last_used = $1 WHERE id = $2"#)
        .bind(now)
        .bind(id)
        .execute(pool)
        .await
        .map_err(|_| server_error())?;
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
        sqlx::query(r#"UPDATE machine_token SET revoked_at = $1 WHERE id = $2"#)
            .bind(now)
            .bind(id)
            .execute(pool)
            .await
            .map_err(|_| server_error())?;
        return Err(invalid_token());
    }
    // `MachineToken.objects.filter(pk=...).update(last_used_at=now)`.
    sqlx::query(r#"UPDATE machine_token SET last_used_at = $1 WHERE id = $2"#)
        .bind(now)
        .bind(id)
        .execute(pool)
        .await
        .map_err(|_| server_error())?;
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
// Delete store: the first pool-backed `RevokeStore + RunnerDeleteStore`
// ---------------------------------------------------------------------------

/// Storage failure → the executor's 500 detail (Python lets any DB/Redis
/// exception propagate out of the atomic, rolling it back).
fn store_err<E: std::fmt::Display>(error: E) -> revoke::RevokeError {
    revoke::RevokeError::Store(error.to_string())
}

/// The executing store for [`delete_svc::delete_runner`]: the one open
/// transaction (the atomic tail — the frame methods touch Redis only, so
/// calling them first on it is unobservable), the api-owned Redis
/// client, and the post-commit collectors (per-run terminal-effects
/// publications, then handoffs → drains → cleanup, in registration
/// order).
struct DeleteStore<'t> {
    tx: sqlx::Transaction<'t, sqlx::Postgres>,
    redis: Option<redis::Client>,
    runner_settings: pidash_db::config::RunnerSettings,
    handoffs: Vec<Uuid>,
    drains: Vec<Uuid>,
    cleanups: Vec<Uuid>,
    run_effects: Vec<LifecycleEffect>,
}

/// `SET` column order for the revoke finalize (`:52-59` base plus the
/// `:631-640` updates in dict order); the binds below are positional, so
/// a mismatch fails loudly instead of binding wrong (the L6a sweeps
/// precedent).
const REVOKE_FINALIZE_COLUMNS: [&str; 8] = [
    "status",
    "ended_at",
    "queue_position",
    "terminal_hooks_applied_at",
    "terminal_capacity_released_at",
    "error",
    "error_code",
    "cancel_reason",
];

/// Bind `ids` positionally (`$1..$N`, fetch order) and fetch one UUID
/// column. Callers skip empty lists (an `IN ()` would be invalid SQL).
async fn fetch_in_ids(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    sql: &str,
    ids: &[Uuid],
) -> Result<Vec<Uuid>, revoke::RevokeError> {
    debug_assert!(!ids.is_empty(), "callers skip empty id lists");
    let mut query = sqlx::query_scalar::<_, Uuid>(sql);
    for id in ids {
        query = query.bind(id);
    }
    query.fetch_all(&mut **tx).await.map_err(store_err)
}

/// Bind `ids` positionally (`$1..$N`, fetch order) and execute a
/// collector write. Callers skip empty lists.
async fn exec_in_ids(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    sql: &str,
    ids: &[Uuid],
) -> Result<(), revoke::RevokeError> {
    debug_assert!(!ids.is_empty(), "callers skip empty id lists");
    let mut query = sqlx::query(sql);
    for id in ids {
        query = query.bind(id);
    }
    query.execute(&mut **tx).await.map_err(store_err)?;
    Ok(())
}

impl revoke::RevokeStore for DeleteStore<'_> {
    async fn mark_runner_revoked(
        &mut self,
        runner_id: Uuid,
        now: DateTime<Utc>,
        stored_reason: &str,
    ) -> Result<(), revoke::RevokeError> {
        sqlx::query(revoke::MARK_RUNNER_REVOKED_SQL)
            .bind(now)
            .bind(stored_reason)
            .bind(runner_id)
            .execute(&mut *self.tx)
            .await
            .map_err(store_err)?;
        Ok(())
    }

    async fn revoke_active_sessions(
        &mut self,
        runner_id: Uuid,
        now: DateTime<Utc>,
        stored_reason: &str,
    ) -> Result<(), revoke::RevokeError> {
        sqlx::query(revoke::REVOKE_ACTIVE_SESSIONS_SQL)
            .bind(now)
            .bind(stored_reason)
            .bind(runner_id)
            .execute(&mut *self.tx)
            .await
            .map_err(store_err)?;
        Ok(())
    }

    async fn lock_active_runs(
        &mut self,
        runner_id: Uuid,
    ) -> Result<Vec<(Uuid, Option<Uuid>)>, revoke::RevokeError> {
        sqlx::query_as::<_, (Uuid, Option<Uuid>)>(&revoke::active_runs_lock_sql())
            .bind(runner_id)
            .fetch_all(&mut *self.tx)
            .await
            .map_err(store_err)
    }

    /// Per-run cancel via the D-15 finalize builders (the
    /// `runs.rs::finalize_cancelled` precedent): first-writer-wins lock
    /// (a miss is `Ok(())` — Python ignores the `False`), update,
    /// cloud-only terminal event, and — on success only — the
    /// terminal-effects publication captured for the post-commit drain.
    /// No `done_payload` merge: these updates carry none.
    async fn finalize_cancelled_run(
        &mut self,
        run_id: Uuid,
        runner_id: Uuid,
        stored_reason: &str,
    ) -> Result<(), revoke::RevokeError> {
        let values = finalization::plan_finalize_values(
            AgentRunStatus::Cancelled,
            &[
                ("error", SetValue::Text(revoke::FINALIZE_ERROR.to_owned())),
                (
                    "error_code",
                    SetValue::Text(revoke::FINALIZE_ERROR_CODE.to_owned()),
                ),
                ("cancel_reason", SetValue::Text(stored_reason.to_owned())),
            ],
        )
        .map_err(store_err)?;
        let columns: Vec<&str> = values.clauses.iter().map(|clause| clause.column).collect();
        if columns.as_slice() != REVOKE_FINALIZE_COLUMNS {
            return Err(store_err("revoke finalize column order"));
        }
        let lock_sql = finalization::lock_run_for_finalize_sql(true, false);
        let mut lock = sqlx::query(&lock_sql).bind(run_id);
        for status in TERMINAL_RUN_STATUSES {
            lock = lock.bind(status.value());
        }
        let locked: Option<sqlx::postgres::PgRow> = lock
            .bind(runner_id)
            .fetch_optional(&mut *self.tx)
            .await
            .map_err(store_err)?;
        let Some(row) = locked else {
            return Ok(());
        };
        let executor_kind: String = row.try_get("executor_kind").map_err(store_err)?;
        sqlx::query(&finalization::finalize_update_sql(&values))
            .bind(AgentRunStatus::Cancelled.value())
            .bind(now_micros())
            .bind(None::<i16>)
            .bind(None::<DateTime<Utc>>)
            .bind(None::<DateTime<Utc>>)
            .bind(revoke::FINALIZE_ERROR)
            .bind(revoke::FINALIZE_ERROR_CODE)
            .bind(stored_reason)
            .bind(run_id)
            .execute(&mut *self.tx)
            .await
            .map_err(store_err)?;
        if executor_kind == AgentExecutorKind::CloudAgent.value() {
            let exists = sqlx::query_scalar::<_, i32>(finalization::terminal_event_exists_sql())
                .bind(run_id)
                .bind("terminal")
                .fetch_optional(&mut *self.tx)
                .await
                .map_err(store_err)?;
            if exists.is_none() {
                let max_seq =
                    sqlx::query_scalar::<_, i32>(finalization::terminal_event_max_seq_sql())
                        .bind(run_id)
                        .fetch_optional(&mut *self.tx)
                        .await
                        .map_err(store_err)?;
                let plan = finalization::plan_terminal_event(
                    max_seq,
                    AgentRunStatus::Cancelled,
                    &finalization::finalize_error_code(&values),
                );
                sqlx::query(&finalization::terminal_event_insert_sql())
                    .bind(run_id)
                    .bind(plan.seq)
                    .bind("terminal")
                    .bind(plan.payload)
                    .bind(now_micros())
                    .execute(&mut *self.tx)
                    .await
                    .map_err(store_err)?;
            }
        }
        self.run_effects
            .extend(finalization::plan_publish_effects(run_id));
        Ok(())
    }

    async fn pinned_queued_pod_ids(
        &mut self,
        runner_id: Uuid,
    ) -> Result<Vec<Option<Uuid>>, revoke::RevokeError> {
        sqlx::query_scalar::<_, Option<Uuid>>(revoke::PINNED_QUEUED_PODS_SQL)
            .bind(runner_id)
            .fetch_all(&mut *self.tx)
            .await
            .map_err(store_err)
    }

    async fn unpin_queued_runs(&mut self, runner_id: Uuid) -> Result<(), revoke::RevokeError> {
        sqlx::query(revoke::UNPIN_QUEUED_RUNS_SQL)
            .bind(runner_id)
            .execute(&mut *self.tx)
            .await
            .map_err(store_err)?;
        Ok(())
    }

    fn complete_handoff_after_commit(&mut self, run_id: Uuid) {
        self.handoffs.push(run_id);
    }

    fn drain_pod_after_commit(&mut self, pod_id: Uuid) {
        self.drains.push(pod_id);
    }

    fn schedule_stream_cleanup_after_commit(&mut self, runner_id: Uuid) {
        self.cleanups.push(runner_id);
    }
}

impl delete_svc::RunnerDeleteStore for DeleteStore<'_> {
    /// `send_runner_revoke`, mirrored call-for-call on the open
    /// transaction (frame carries no `mid` — the outbox mints it).
    async fn send_revoke_frame(
        &mut self,
        runner_id: Uuid,
        reason: &str,
    ) -> Result<Vec<String>, revoke::RevokeError> {
        let frame = revoke_frame(reason);
        match outbox::enqueue_for_runner(
            self.redis.as_ref(),
            &mut *self.tx,
            &self.runner_settings,
            runner_id,
            &frame,
        )
        .await
        {
            Ok(_) => Ok(Vec::new()),
            Err(OutboxError::RunnerOffline { .. }) => Ok(vec![format!(
                "revoke enqueue rejected as offline for {runner_id}"
            )]),
            Err(error) => Ok(vec![format!(
                "send_runner_revoke failed for {runner_id}: {error}"
            )]),
        }
    }

    /// `send_runner_remove`, mirrored call-for-call on the open
    /// transaction (frame carries no `mid` — the outbox mints it).
    async fn send_remove_frame(
        &mut self,
        runner_id: Uuid,
        reason: &str,
    ) -> Result<Vec<String>, revoke::RevokeError> {
        let frame = remove_runner_frame(&runner_id.to_string(), reason);
        match outbox::enqueue_for_runner(
            self.redis.as_ref(),
            &mut *self.tx,
            &self.runner_settings,
            runner_id,
            &frame,
        )
        .await
        {
            Ok(_) => Ok(Vec::new()),
            Err(OutboxError::RunnerOffline { .. }) => Ok(vec![format!(
                "remove_runner enqueue rejected as offline for {runner_id}"
            )]),
            Err(error) => Ok(vec![format!(
                "send_runner_remove failed for {runner_id}: {error}"
            )]),
        }
    }

    /// `close_runner_session`, mirrored call-for-call on the open
    /// transaction: per active session (newest first) revoke the row
    /// with a per-row `now()`, clear the PEL marker, publish the
    /// eviction. The first failure aborts and propagates.
    async fn close_runner_session(&mut self, runner_id: Uuid) -> Result<(), revoke::RevokeError> {
        let rows = sqlx::query(pubsub::CLOSE_ACTIVE_SESSIONS_SQL)
            .bind(runner_id)
            .fetch_all(&mut *self.tx)
            .await
            .map_err(store_err)?;
        for row in rows {
            let session = rs_models::runner_session_from_row(&row).map_err(store_err)?;
            let sid = session.id.to_string();
            sqlx::query(rs_models::REVOKE_SQL)
                .bind(now_micros())
                .bind(pubsub::FORCE_CLOSE_REASON)
                .bind(session.id)
                .execute(&mut *self.tx)
                .await
                .map_err(store_err)?;
            outbox::clear_session_marker(self.redis.as_ref(), &sid)
                .await
                .map_err(store_err)?;
            outbox::publish_session_eviction(
                self.redis.as_ref(),
                &runner_id.to_string(),
                Some(&sid),
                "",
            )
            .await
            .map_err(store_err)?;
        }
        Ok(())
    }

    async fn revoke_machine_tokens(
        &mut self,
        machine_id: Uuid,
        now: DateTime<Utc>,
    ) -> Result<(), revoke::RevokeError> {
        sqlx::query(delete_svc::REVOKE_MACHINE_TOKENS_SQL)
            .bind(now)
            .bind(machine_id)
            .execute(&mut *self.tx)
            .await
            .map_err(store_err)?;
        Ok(())
    }

    async fn lock_machine_runners(
        &mut self,
        machine_id: Uuid,
    ) -> Result<Vec<delete_svc::LockedRunner>, revoke::RevokeError> {
        let rows = sqlx::query(delete_svc::LOCK_MACHINE_RUNNERS_SQL)
            .bind(machine_id)
            .fetch_all(&mut *self.tx)
            .await
            .map_err(store_err)?;
        rows.iter()
            .map(|row| {
                Ok(delete_svc::LockedRunner {
                    id: row.try_get("id").map_err(store_err)?,
                    revoked_at: row.try_get("revoked_at").map_err(store_err)?,
                })
            })
            .collect()
    }

    async fn fetch_runner_for_delete(
        &mut self,
        runner_id: Uuid,
    ) -> Result<bool, revoke::RevokeError> {
        let row: Option<sqlx::postgres::PgRow> = sqlx::query(delete_svc::COLLECT_RUNNER_BY_ID_SQL)
            .bind(runner_id)
            .fetch_optional(&mut *self.tx)
            .await
            .map_err(store_err)?;
        Ok(row.is_some())
    }

    async fn fetch_machine_runner_ids_for_delete(
        &mut self,
        machine_id: Uuid,
    ) -> Result<Vec<Uuid>, revoke::RevokeError> {
        let rows = sqlx::query(delete_svc::COLLECT_RUNNERS_BY_MACHINE_SQL)
            .bind(machine_id)
            .fetch_all(&mut *self.tx)
            .await
            .map_err(store_err)?;
        rows.iter()
            .map(|row| row.try_get("id").map_err(store_err))
            .collect()
    }

    async fn fetch_chat_ids(
        &mut self,
        runner_ids: &[Uuid],
    ) -> Result<Vec<Uuid>, revoke::RevokeError> {
        fetch_in_ids(
            &mut self.tx,
            &delete_svc::collect_chat_ids_sql(runner_ids.len()),
            runner_ids,
        )
        .await
    }

    async fn fetch_message_ids(
        &mut self,
        chat_ids: &[Uuid],
    ) -> Result<Vec<Uuid>, revoke::RevokeError> {
        fetch_in_ids(
            &mut self.tx,
            &delete_svc::collect_message_ids_sql(chat_ids.len()),
            chat_ids,
        )
        .await
    }

    async fn delete_chat_events(&mut self, chat_ids: &[Uuid]) -> Result<(), revoke::RevokeError> {
        exec_in_ids(
            &mut self.tx,
            &delete_svc::delete_chat_events_sql(chat_ids.len()),
            chat_ids,
        )
        .await
    }

    async fn delete_chat_approvals(
        &mut self,
        chat_ids: &[Uuid],
    ) -> Result<(), revoke::RevokeError> {
        exec_in_ids(
            &mut self.tx,
            &delete_svc::delete_chat_approvals_sql(chat_ids.len()),
            chat_ids,
        )
        .await
    }

    async fn delete_chat_dedupes(&mut self, chat_ids: &[Uuid]) -> Result<(), revoke::RevokeError> {
        exec_in_ids(
            &mut self.tx,
            &delete_svc::delete_chat_dedupes_sql(chat_ids.len()),
            chat_ids,
        )
        .await
    }

    async fn delete_runner_sessions(
        &mut self,
        runner_ids: &[Uuid],
    ) -> Result<(), revoke::RevokeError> {
        exec_in_ids(
            &mut self.tx,
            &delete_svc::delete_runner_sessions_sql(runner_ids.len()),
            runner_ids,
        )
        .await
    }

    async fn delete_force_refresh(
        &mut self,
        runner_ids: &[Uuid],
    ) -> Result<(), revoke::RevokeError> {
        exec_in_ids(
            &mut self.tx,
            &delete_svc::delete_force_refresh_sql(runner_ids.len()),
            runner_ids,
        )
        .await
    }

    async fn delete_live_state(&mut self, runner_ids: &[Uuid]) -> Result<(), revoke::RevokeError> {
        exec_in_ids(
            &mut self.tx,
            &delete_svc::delete_live_state_sql(runner_ids.len()),
            runner_ids,
        )
        .await
    }

    async fn null_run_runners(&mut self, runner_ids: &[Uuid]) -> Result<(), revoke::RevokeError> {
        exec_in_ids(
            &mut self.tx,
            &delete_svc::null_run_runners_sql(runner_ids.len()),
            runner_ids,
        )
        .await
    }

    async fn null_run_pins(&mut self, runner_ids: &[Uuid]) -> Result<(), revoke::RevokeError> {
        exec_in_ids(
            &mut self.tx,
            &delete_svc::null_run_pins_sql(runner_ids.len()),
            runner_ids,
        )
        .await
    }

    async fn null_event_messages(
        &mut self,
        message_ids: &[Uuid],
    ) -> Result<(), revoke::RevokeError> {
        exec_in_ids(
            &mut self.tx,
            &delete_svc::null_event_messages_sql(message_ids.len()),
            message_ids,
        )
        .await
    }

    async fn delete_chat_messages(
        &mut self,
        message_ids: &[Uuid],
    ) -> Result<(), revoke::RevokeError> {
        exec_in_ids(
            &mut self.tx,
            &delete_svc::delete_chat_messages_sql(message_ids.len()),
            message_ids,
        )
        .await
    }

    async fn delete_chat_sessions(&mut self, chat_ids: &[Uuid]) -> Result<(), revoke::RevokeError> {
        exec_in_ids(
            &mut self.tx,
            &delete_svc::delete_chat_sessions_sql(chat_ids.len()),
            chat_ids,
        )
        .await
    }

    async fn delete_runner_rows(&mut self, runner_ids: &[Uuid]) -> Result<(), revoke::RevokeError> {
        exec_in_ids(
            &mut self.tx,
            &delete_svc::delete_runner_rows_sql(runner_ids.len()),
            runner_ids,
        )
        .await
    }

    async fn fetch_machine_for_delete(
        &mut self,
        machine_id: Uuid,
    ) -> Result<bool, revoke::RevokeError> {
        let row: Option<sqlx::postgres::PgRow> = sqlx::query(delete_svc::COLLECT_MACHINE_BY_ID_SQL)
            .bind(machine_id)
            .fetch_optional(&mut *self.tx)
            .await
            .map_err(store_err)?;
        Ok(row.is_some())
    }

    async fn delete_machine_sessions(
        &mut self,
        machine_id: Uuid,
    ) -> Result<(), revoke::RevokeError> {
        sqlx::query(delete_svc::DELETE_MACHINE_SESSIONS_SQL)
            .bind(machine_id)
            .execute(&mut *self.tx)
            .await
            .map_err(store_err)?;
        Ok(())
    }

    async fn null_runner_machines(&mut self, machine_id: Uuid) -> Result<(), revoke::RevokeError> {
        sqlx::query(delete_svc::NULL_RUNNER_MACHINES_SQL)
            .bind(machine_id)
            .execute(&mut *self.tx)
            .await
            .map_err(store_err)?;
        Ok(())
    }

    async fn null_token_machines(&mut self, machine_id: Uuid) -> Result<(), revoke::RevokeError> {
        sqlx::query(delete_svc::NULL_TOKEN_MACHINES_SQL)
            .bind(machine_id)
            .execute(&mut *self.tx)
            .await
            .map_err(store_err)?;
        Ok(())
    }

    async fn delete_machine_row(&mut self, machine_id: Uuid) -> Result<(), revoke::RevokeError> {
        sqlx::query(delete_svc::DELETE_MACHINE_ROW_SQL)
            .bind(machine_id)
            .execute(&mut *self.tx)
            .await
            .map_err(store_err)?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Handler (`views/runner.py:44-57`)
// ---------------------------------------------------------------------------

/// `DELETE /api/v1/runners/<runner_id>/`: by-pk 404, view-guard 404,
/// manage-guard 403, flag-parse 400, then the delete service and an
/// empty 204.
pub async fn delete_runner(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(raw_id): Path<String>,
    Query(query): Query<QueryMap>,
) -> Response {
    // Resolver-level: Django's `<uuid:>` converter rejects before any
    // authentication runs, so the segment parses before the 401 arm.
    let runner_id: Uuid = match raw_id.parse() {
        Ok(runner_id) => runner_id,
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
    let mut store = DeleteStore {
        tx: match pool.begin().await {
            Ok(tx) => tx,
            Err(_) => return server_error(),
        },
        redis: redis_client(&state),
        runner_settings: state.settings().runner.clone(),
        handoffs: Vec::new(),
        drains: Vec::new(),
        cleanups: Vec::new(),
        run_effects: Vec::new(),
    };
    let outcome = match delete_svc::delete_runner(
        &mut store,
        runner_id,
        runner.revoked_at,
        purge_local,
        now_micros(),
    )
    .await
    {
        Ok(outcome) => outcome,
        Err(error) => {
            tracing::warn!(%error, "v1_cli_auth delete store failure");
            return server_error();
        }
    };
    for warning in &outcome.warnings {
        tracing::warn!("{warning}");
    }
    let DeleteStore {
        tx,
        redis,
        handoffs,
        drains,
        cleanups,
        mut run_effects,
        ..
    } = store;
    if tx.commit().await.is_err() {
        return server_error();
    }
    // Post-commit drain in registration order: the per-run
    // terminal-effects publications first, then the handoffs, the pod
    // drains, and the stream-cleanup scheduling.
    for run_id in handoffs {
        run_effects.push(LifecycleEffect::CompleteProjectMoveHandoff { run_id });
    }
    for pod_id in drains {
        run_effects.push(LifecycleEffect::DrainPod { pod_id });
    }
    if let Err(response) =
        drain_lifecycle_effects(&pool, &LivePorts::new(pool.clone(), &state), run_effects).await
    {
        return response;
    }
    for runner_id in cleanups {
        if outbox::schedule_stream_cleanup_for_runner(
            redis.as_ref(),
            &state.settings().runner,
            &runner_id.to_string(),
        )
        .await
        .is_err()
        {
            return server_error();
        }
    }
    no_content()
}
