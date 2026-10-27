#![forbid(unsafe_code)]

//! Runner revoke/delete executor (OVL-06, PIDASHCONV-1544): the revoke
//! bundle as one public service call.
//!
//! Reconciles the three pool stores behind the services drivers
//! ([`revoke::revoke_runner`](pidash_services::runner_enroll::revoke::revoke_runner)
//! +
//! [`delete_svc::delete_runner`](pidash_services::runner_enroll::delete::delete_runner)
//! /
//! [`delete_dev_machine`](pidash_services::runner_enroll::delete::delete_dev_machine))
//! — the teardown `TxStores`, the enroll `DeleteStore`, and the v1
//! `DeleteStore` — into one canonical [`PoolRevokeStore`] plus the
//! [`revoke`] service and the [`fire_revoke_bundle`] post-commit drain.
//! The handlers keep their shells (gates, tx boundaries, warning logs,
//! response mapping) and call [`revoke`], then fire the collected
//! bundle after their commit.
//!
//! # Preserved divergences (translate, don't redesign)
//!
//! The three surfaces disagree in four places; each is preserved
//! exactly behind an explicit parameter, and each is listed in the PR:
//!
//! * Frame/close placement: the enroll surface sends frames and closes
//!   sessions on the pool (escaped from the tx); the v1 surface and the
//!   teardown machine-revoke run them on the revoke tx (atomic).
//!   Python places runner-delete frames pre-atomic and machine-delete
//!   frames in-atomic, so each surface matches Python on one flow and
//!   diverges (rollback-path only) on another. [`RevokeCtx::atomic_effects`]
//!   selects the placement per caller.
//! * Fire logs: the handoff/emit/apply failure lines differ per surface
//!   (warn vs error, `handoff_failure_log` vs static text, the "for
//!   run" suffix). The service owns fire ORDER; each log line stays in
//!   its surface's [`fire_revoke_bundle`] callback.
//! * Row decode: by position (enroll) vs by name (v1, teardown). Same
//!   values; only a decode-failure message (unreachable on uuid
//!   columns) could differ. The canonical store decodes by name.
//! * Finalize clocks: the enroll/teardown finalizes bind raw
//!   `Utc::now()` where v1 binds micros-truncated time. sqlx encodes
//!   `DateTime` via `num_microseconds()` (truncation —
//!   `sqlx-postgres-0.8.6/src/types/chrono/datetime.rs:36-42`), so the
//!   wire bytes are identical; the canonical store binds truncated
//!   time, the Python shape.
//!
//! # Transactions
//!
//! The service never owns the transaction: the caller lends its
//! [`sqlx::Transaction`] (the caller's own begin for deletes, the outer
//! rotation/machine tx for teardown revokes) and commits it before
//! firing the bundle — the `HandoffStore` precedent. Post-commit fire
//! order is the services [`fire_plan`](pidash_services::runner_enroll::fire::fire_plan)
//! order: publish pairs, handoffs, drains, cleanup; drains and cleanup
//! propagate (Django's bare `on_commit` chain), everything else is
//! isolated.
//!
//! Re-exported through `pidash-sdk::ops`.

use std::future::Future;

use chrono::{DateTime, Utc};
use serde_json::{Map, Value};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use pidash_db::config::RunnerSettings;
use pidash_db::runner_sessions::machine_outbox;
use pidash_db::runner_sessions::machine_outbox::MachineOutboxError;
use pidash_db::runner_sessions::models::runner_session;
use pidash_db::runner_sessions::outbox as runner_outbox;
use pidash_db::runner_sessions::outbox::OutboxError;
use pidash_db::runner_sessions::RunnerSession;
use pidash_services::runner_enroll::delete as delete_svc;
use pidash_services::runner_enroll::revoke::{self, RevokeError, RevokeStore};
use pidash_services::runner_runs::finalization as finalize_kernel;
use pidash_services::runner_runs::LifecycleEffect;
use pidash_services::runner_sessions::live_drains;
use pidash_services::runner_sessions::pubsub as pubsub_svc;
use pidash_services::runner_sessions::pubsub::PubsubStore;
use pidash_types::dispatch::AgentExecutorKind;
use pidash_types::runner_runs::{AgentRunStatus, TERMINAL_RUN_STATUSES};
use pidash_types::runner_sessions::{remove_runner_frame, revoke_frame};

// ---------------------------------------------------------------------------
// Public service
// ---------------------------------------------------------------------------

/// The runtime facts a revoke/delete runs with: the pool (frame/close
/// placement when `atomic_effects` is false, drains, cleanup), the
/// Redis client, and runner settings. The transaction is lent per
/// call, never owned (see the module docs).
pub struct RevokeCtx {
    pub pool: PgPool,
    pub redis: Option<redis::Client>,
    pub runner: RunnerSettings,
    /// Frame/close placement: `true` runs them on the revoke tx
    /// (v1, teardown machine-revoke), `false` on the pool (enroll).
    /// Preserved per caller (see the module docs).
    pub atomic_effects: bool,
}

/// What to revoke: a bare `Runner.revoke()` on the lent tx, a runner
/// delete, or a dev-machine delete. `now` stamps S1/S2 (D1 for
/// machines); machine nested revokes sample their own micros clock,
/// as Python does per `runner.revoke()` call.
pub enum RevokeScope<'a> {
    Runner {
        runner_id: Uuid,
        revoked_at: Option<DateTime<Utc>>,
        reason: &'a str,
        now: DateTime<Utc>,
    },
    DeleteRunner {
        runner_id: Uuid,
        revoked_at: Option<DateTime<Utc>>,
        purge_local: bool,
        now: DateTime<Utc>,
    },
    DeleteMachine {
        machine_id: Uuid,
        purge_local: bool,
        now: DateTime<Utc>,
    },
}

/// One revoke's post-commit effects, in registration order: the
/// per-run publish pairs (S3 order), handoffs as `(runner_id, run_id)`
/// pairs, pod drains, and stream cleanups.
pub struct RevokeBundle {
    pub publish: Vec<LifecycleEffect>,
    pub handoffs: Vec<(Uuid, Uuid)>,
    pub drains: Vec<Uuid>,
    pub cleanups: Vec<Uuid>,
}

/// The driver's collected post-commit bundle plus its warnings. The
/// caller logs the warnings (each surface keeps its own render) and
/// fires the bundle after its commit.
pub struct RevokeCollected {
    pub bundle: RevokeBundle,
    pub warnings: Vec<String>,
}

/// Run the revoke/delete drivers on the lent tx and collect the
/// post-commit bundle. The caller commits its tx, logs
/// [`RevokeCollected::warnings`], then fires the bundle with
/// [`fire_revoke_bundle`].
pub async fn revoke(
    ctx: &RevokeCtx,
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    scope: RevokeScope<'_>,
) -> Result<RevokeCollected, RevokeError> {
    let mut store = PoolRevokeStore {
        tx,
        pool: &ctx.pool,
        redis: ctx.redis.as_ref(),
        runner: &ctx.runner,
        atomic_effects: ctx.atomic_effects,
        current_runner: None,
        s2_revoked: Vec::new(),
        publish: Vec::new(),
        handoffs: Vec::new(),
        drains: Vec::new(),
        cleanups: Vec::new(),
    };
    let warnings = match scope {
        RevokeScope::Runner {
            runner_id,
            revoked_at,
            reason,
            now,
        } => {
            revoke::revoke_runner(&mut store, runner_id, revoked_at, reason, now)
                .await?
                .warnings
        }
        RevokeScope::DeleteRunner {
            runner_id,
            revoked_at,
            purge_local,
            now,
        } => {
            delete_svc::delete_runner(&mut store, runner_id, revoked_at, purge_local, now)
                .await?
                .warnings
        }
        RevokeScope::DeleteMachine {
            machine_id,
            purge_local,
            now,
        } => {
            // Python evaluates `timezone.now()` per nested
            // `runner.revoke()`.
            let mut revoke_clock = || now_micros();
            delete_svc::delete_dev_machine(
                &mut store,
                machine_id,
                purge_local,
                now,
                &mut revoke_clock,
            )
            .await?
            .warnings
        }
    };
    Ok(RevokeCollected {
        bundle: store.into_bundle(),
        warnings,
    })
}

/// Why the post-commit fire failed: a pod drain or the stream
/// cleanup. Both propagate (the caller answers its server error);
/// every other step is isolated.
#[derive(Debug)]
pub enum RevokeFireError {
    Drain,
    Cleanup,
}

/// Fire one collected bundle in `fire_plan` order: publish pairs,
/// handoffs, drains, cleanup. Drains run the D-14 live recipe and
/// cleanup runs the outbox scheduler, both propagating; every other
/// step runs its surface callback, isolated. Each callback performs
/// its step and logs its own failure lines (which differ per surface
/// — see the module docs): `emit_terminal` fires one
/// `PublishTerminalEffects` pair member, `apply_inline` one
/// `ApplyTerminalEffectsInline` member, `handoff` one
/// `(runner_id, run_id)` handoff.
#[allow(clippy::too_many_arguments)]
pub async fn fire_revoke_bundle<E, EFut, A, AFut, H, HFut>(
    ctx: &RevokeCtx,
    bundle: RevokeBundle,
    mut emit_terminal: E,
    mut apply_inline: A,
    mut handoff: H,
) -> Result<(), RevokeFireError>
where
    E: FnMut(Uuid) -> EFut,
    EFut: Future<Output = ()>,
    A: FnMut(Uuid) -> AFut,
    AFut: Future<Output = ()>,
    H: FnMut(Uuid, Uuid) -> HFut,
    HFut: Future<Output = ()>,
{
    for effect in bundle.publish {
        match effect {
            LifecycleEffect::PublishTerminalEffects { run_id } => {
                emit_terminal(run_id).await;
            }
            LifecycleEffect::ApplyTerminalEffectsInline { run_id } => {
                apply_inline(run_id).await;
            }
            // The bundle holds publish pairs only (see
            // `finalize_cancelled_run`); anything else is ignored, as
            // the enroll drain's wildcard arm does.
            _ => {}
        }
    }
    for (runner_id, run_id) in bundle.handoffs {
        handoff(runner_id, run_id).await;
    }
    for pod_id in bundle.drains {
        live_drains::drain_pod_by_id(&ctx.pool, ctx.redis.as_ref(), &ctx.runner, pod_id)
            .await
            .map(|_| ())
            .map_err(|_| RevokeFireError::Drain)?;
    }
    for runner_id in bundle.cleanups {
        runner_outbox::schedule_stream_cleanup_for_runner(
            ctx.redis.as_ref(),
            &ctx.runner,
            &runner_id.to_string(),
        )
        .await
        .map_err(|_| RevokeFireError::Cleanup)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Canonical pool store
// ---------------------------------------------------------------------------

/// Pool-backed [`RevokeStore`] + [`delete_svc::RunnerDeleteStore`]:
/// revoke/delete SQL runs on the caller's lent transaction; frames and
/// session close run on the tx or the pool per
/// [`RevokeCtx::atomic_effects`]; collectors accumulate for the
/// post-commit fire.
struct PoolRevokeStore<'t, 'x, 'c> {
    tx: &'t mut sqlx::Transaction<'x, sqlx::Postgres>,
    pool: &'c PgPool,
    redis: Option<&'c redis::Client>,
    runner: &'c RunnerSettings,
    atomic_effects: bool,
    /// The runner whose nested revoke is running (set by S1, read by
    /// the handoff collector to pair each handoff with its runner).
    current_runner: Option<Uuid>,
    /// Runners whose S2 ran on this transaction: their pool close is
    /// skipped (see `close_runner_session`).
    s2_revoked: Vec<Uuid>,
    publish: Vec<LifecycleEffect>,
    handoffs: Vec<(Uuid, Uuid)>,
    drains: Vec<Uuid>,
    cleanups: Vec<Uuid>,
}

impl PoolRevokeStore<'_, '_, '_> {
    fn into_bundle(self) -> RevokeBundle {
        RevokeBundle {
            publish: self.publish,
            handoffs: self.handoffs,
            drains: self.drains,
            cleanups: self.cleanups,
        }
    }
}

fn store_err(error: impl std::fmt::Display) -> RevokeError {
    RevokeError::Store(error.to_string())
}

/// `timezone.now()` truncated to microseconds (the S1/S2 stamp shape).
fn now_micros() -> DateTime<Utc> {
    let now = Utc::now();
    DateTime::from_timestamp_micros(now.timestamp_micros()).expect("micros in range")
}

/// The revoke-finalize SET order (planned `status`, `ended_at`,
/// `queue_position`, the two markers, then the updates in dict order):
/// pinned so a kernel drift fails loudly instead of mis-binding.
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

#[allow(async_fn_in_trait)]
impl RevokeStore for PoolRevokeStore<'_, '_, '_> {
    async fn mark_runner_revoked(
        &mut self,
        runner_id: Uuid,
        now: DateTime<Utc>,
        stored_reason: &str,
    ) -> Result<(), RevokeError> {
        self.current_runner = Some(runner_id);
        sqlx::query(revoke::MARK_RUNNER_REVOKED_SQL)
            .bind(now)
            .bind(stored_reason)
            .bind(runner_id)
            .execute(&mut **self.tx)
            .await
            .map(|_| ())
            .map_err(store_err)
    }

    async fn revoke_active_sessions(
        &mut self,
        runner_id: Uuid,
        now: DateTime<Utc>,
        stored_reason: &str,
    ) -> Result<(), RevokeError> {
        pidash_services::domain::tokens::revoke_runner_sessions_for_runner(
            &mut **self.tx,
            runner_id,
            now,
            stored_reason,
        )
        .await
        .map_err(store_err)?;
        // S2 ran: every active session for this runner is revoked on
        // the tx, so the driver's later `close_runner_session` must
        // skip the pool close (see its comment).
        if !self.s2_revoked.contains(&runner_id) {
            self.s2_revoked.push(runner_id);
        }
        Ok(())
    }

    async fn lock_active_runs(
        &mut self,
        runner_id: Uuid,
    ) -> Result<Vec<(Uuid, Option<Uuid>)>, RevokeError> {
        sqlx::query_as::<_, (Uuid, Option<Uuid>)>(&revoke::active_runs_lock_sql())
            .bind(runner_id)
            .fetch_all(&mut **self.tx)
            .await
            .map_err(store_err)
    }

    /// Per-run cancel via the D-15 finalize builders: first-writer-wins
    /// lock (a miss is `Ok(())` — Python ignores the `False`), update,
    /// cloud-only terminal event, and — on success only — the
    /// terminal-effects publication captured for the post-commit drain.
    /// No `done_payload` merge: these updates carry none.
    ///
    /// Reconciles the three surfaces: the plan inputs and column pin are
    /// teardown/v1's, the positional binds are v1/teardown's, the clock
    /// is micros-truncated (v1's — wire-identical to the raw binds
    /// elsewhere since sqlx truncates to micros; see the module docs).
    async fn finalize_cancelled_run(
        &mut self,
        run_id: Uuid,
        runner_id: Uuid,
        stored_reason: &str,
    ) -> Result<(), RevokeError> {
        use pidash_services::runner_runs::SetValue;
        let values = finalize_kernel::plan_finalize_values(
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
            return Err(RevokeError::Store(format!(
                "revoke finalize columns drifted: {columns:?}"
            )));
        }
        // First-writer-wins lock with the expected-runner predicate; a
        // miss is `Ok(())` — Python ignores the `False`.
        let lock_sql = finalize_kernel::lock_run_for_finalize_sql(true, false);
        let mut lock = sqlx::query(&lock_sql).bind(run_id);
        for status in TERMINAL_RUN_STATUSES {
            lock = lock.bind(status.value());
        }
        let locked: Option<sqlx::postgres::PgRow> = lock
            .bind(runner_id)
            .fetch_optional(&mut **self.tx)
            .await
            .map_err(store_err)?;
        let Some(row) = locked else {
            return Ok(());
        };
        let executor_kind: String = row.try_get("executor_kind").map_err(store_err)?;
        sqlx::query(&finalize_kernel::finalize_update_sql(&values))
            .bind(AgentRunStatus::Cancelled.value())
            .bind(now_micros())
            .bind(None::<i16>)
            .bind(None::<DateTime<Utc>>)
            .bind(None::<DateTime<Utc>>)
            .bind(revoke::FINALIZE_ERROR)
            .bind(revoke::FINALIZE_ERROR_CODE)
            .bind(stored_reason)
            .bind(run_id)
            .execute(&mut **self.tx)
            .await
            .map_err(store_err)?;
        if executor_kind == AgentExecutorKind::CloudAgent.value() {
            let exists = sqlx::query_scalar::<_, i32>(finalize_kernel::terminal_event_exists_sql())
                .bind(run_id)
                .bind("terminal")
                .fetch_optional(&mut **self.tx)
                .await
                .map_err(store_err)?;
            if exists.is_none() {
                let max_seq =
                    sqlx::query_scalar::<_, i32>(finalize_kernel::terminal_event_max_seq_sql())
                        .bind(run_id)
                        .fetch_optional(&mut **self.tx)
                        .await
                        .map_err(store_err)?;
                let plan = finalize_kernel::plan_terminal_event(
                    max_seq,
                    AgentRunStatus::Cancelled,
                    &finalize_kernel::finalize_error_code(&values),
                );
                sqlx::query(&finalize_kernel::terminal_event_insert_sql())
                    .bind(run_id)
                    .bind(plan.seq)
                    .bind("terminal")
                    .bind(plan.payload)
                    .bind(now_micros())
                    .execute(&mut **self.tx)
                    .await
                    .map_err(store_err)?;
            }
        }
        // D-15's terminal-effects `on_commit` (registered in-tx, fires
        // first post-commit).
        self.publish
            .extend(finalize_kernel::plan_publish_effects(run_id));
        Ok(())
    }

    async fn pinned_queued_pod_ids(
        &mut self,
        runner_id: Uuid,
    ) -> Result<Vec<Option<Uuid>>, RevokeError> {
        sqlx::query_scalar(revoke::PINNED_QUEUED_PODS_SQL)
            .bind(runner_id)
            .fetch_all(&mut **self.tx)
            .await
            .map_err(store_err)
    }

    async fn unpin_queued_runs(&mut self, runner_id: Uuid) -> Result<(), RevokeError> {
        sqlx::query(revoke::UNPIN_QUEUED_RUNS_SQL)
            .bind(runner_id)
            .execute(&mut **self.tx)
            .await
            .map(|_| ())
            .map_err(store_err)
    }

    fn complete_handoff_after_commit(&mut self, run_id: Uuid) {
        if let Some(runner_id) = self.current_runner {
            self.handoffs.push((runner_id, run_id));
        }
    }

    fn drain_pod_after_commit(&mut self, pod_id: Uuid) {
        self.drains.push(pod_id);
    }

    fn schedule_stream_cleanup_after_commit(&mut self, runner_id: Uuid) {
        self.cleanups.push(runner_id);
    }
}

#[allow(async_fn_in_trait)]
impl delete_svc::RunnerDeleteStore for PoolRevokeStore<'_, '_, '_> {
    /// `send_runner_revoke`, mirrored call-for-call: on the revoke tx
    /// when `atomic_effects`, on the pool otherwise (see the module docs).
    async fn send_revoke_frame(
        &mut self,
        runner_id: Uuid,
        reason: &str,
    ) -> Result<Vec<String>, RevokeError> {
        if self.atomic_effects {
            let frame = revoke_frame(reason);
            match runner_outbox::enqueue_for_runner(
                self.redis,
                &mut **self.tx,
                self.runner,
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
        } else {
            let shared = PoolPubsub {
                pool: self.pool,
                redis: self.redis,
                runner: self.runner,
            };
            Ok(pubsub_svc::send_runner_revoke(&shared, runner_id, reason)
                .await
                .warnings)
        }
    }

    /// `send_runner_remove`, mirrored call-for-call: on the revoke tx
    /// when `atomic_effects`, on the pool otherwise (see the module docs).
    async fn send_remove_frame(
        &mut self,
        runner_id: Uuid,
        reason: &str,
    ) -> Result<Vec<String>, RevokeError> {
        if self.atomic_effects {
            let frame = remove_runner_frame(&runner_id.to_string(), reason);
            match runner_outbox::enqueue_for_runner(
                self.redis,
                &mut **self.tx,
                self.runner,
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
        } else {
            let shared = PoolPubsub {
                pool: self.pool,
                redis: self.redis,
                runner: self.runner,
            };
            Ok(pubsub_svc::send_runner_remove(&shared, runner_id, reason)
                .await
                .warnings)
        }
    }

    /// `close_runner_session`: on the revoke tx when `atomic_effects`
    /// (the in-tx re-select finds nothing past S2 and no-ops
    /// naturally); on the pool otherwise, skipping runners whose S2
    /// ran (their uncommitted revokes are invisible to the pool and
    /// the re-select would self-deadlock on S2's row locks).
    async fn close_runner_session(&mut self, runner_id: Uuid) -> Result<(), RevokeError> {
        if self.atomic_effects {
            let rows = sqlx::query(pubsub_svc::CLOSE_ACTIVE_SESSIONS_SQL)
                .bind(runner_id)
                .fetch_all(&mut **self.tx)
                .await
                .map_err(store_err)?;
            for row in rows {
                let session = runner_session::runner_session_from_row(&row).map_err(store_err)?;
                let sid = session.id.to_string();
                pidash_services::domain::tokens::revoke_runner_session(
                    &mut **self.tx,
                    session.id,
                    now_micros(),
                    pubsub_svc::FORCE_CLOSE_REASON,
                )
                .await
                .map_err(store_err)?;
                runner_outbox::clear_session_marker(self.redis, &sid)
                    .await
                    .map_err(store_err)?;
                runner_outbox::publish_session_eviction(
                    self.redis,
                    &runner_id.to_string(),
                    Some(&sid),
                    "",
                )
                .await
                .map_err(store_err)?;
            }
            Ok(())
        } else {
            // S2 already revoked every active session for this runner on
            // the tx: Python's in-tx re-select (`runner_delete.py:91-94`
            // tail) finds none and no-ops — reproduced here by skipping.
            // A pool close would be wrong twice over: its SELECT cannot
            // see S2's uncommitted revokes, and its per-session UPDATE
            // would block on S2's row locks while the tx awaits close
            // (self-deadlock; no lock timeout is configured). Runners
            // whose nested revoke early-returned (already revoked, no S2)
            // keep the real close — the tx holds no locks on their
            // session rows, and Python genuinely processes any actives.
            if self.s2_revoked.contains(&runner_id) {
                return Ok(());
            }
            let shared = PoolPubsub {
                pool: self.pool,
                redis: self.redis,
                runner: self.runner,
            };
            pubsub_svc::close_runner_session(
                &shared,
                runner_id,
                pubsub_svc::CLOSE_RUNNER_SESSION_DEFAULT_CODE,
            )
            .await
            .map_err(store_err)
        }
    }

    async fn revoke_machine_tokens(
        &mut self,
        machine_id: Uuid,
        now: DateTime<Utc>,
    ) -> Result<(), RevokeError> {
        pidash_services::domain::tokens::revoke_tokens_for_dev_machine(
            &mut **self.tx,
            machine_id,
            now,
        )
        .await
        .map_err(store_err)
    }

    async fn lock_machine_runners(
        &mut self,
        machine_id: Uuid,
    ) -> Result<Vec<delete_svc::LockedRunner>, RevokeError> {
        let rows = sqlx::query(delete_svc::LOCK_MACHINE_RUNNERS_SQL)
            .bind(machine_id)
            .fetch_all(&mut **self.tx)
            .await
            .map_err(store_err)?;
        rows.iter()
            .map(|row| {
                Ok(delete_svc::LockedRunner {
                    id: row.try_get(0).map_err(store_err)?,
                    revoked_at: row.try_get(28).map_err(store_err)?,
                })
            })
            .collect()
    }

    async fn fetch_runner_for_delete(&mut self, runner_id: Uuid) -> Result<bool, RevokeError> {
        sqlx::query(delete_svc::COLLECT_RUNNER_BY_ID_SQL)
            .bind(runner_id)
            .fetch_optional(&mut **self.tx)
            .await
            .map(|row| row.is_some())
            .map_err(store_err)
    }

    async fn fetch_machine_runner_ids_for_delete(
        &mut self,
        machine_id: Uuid,
    ) -> Result<Vec<Uuid>, RevokeError> {
        let rows = sqlx::query(delete_svc::COLLECT_RUNNERS_BY_MACHINE_SQL)
            .bind(machine_id)
            .fetch_all(&mut **self.tx)
            .await
            .map_err(store_err)?;
        rows.iter()
            .map(|row| row.try_get(0).map_err(store_err))
            .collect()
    }

    async fn fetch_chat_ids(&mut self, runner_ids: &[Uuid]) -> Result<Vec<Uuid>, RevokeError> {
        let sql = delete_svc::collect_chat_ids_sql(runner_ids.len());
        let mut query = sqlx::query(&sql);
        for id in runner_ids {
            query = query.bind(*id);
        }
        let rows = query.fetch_all(&mut **self.tx).await.map_err(store_err)?;
        rows.iter()
            .map(|row| row.try_get(0).map_err(store_err))
            .collect()
    }

    async fn fetch_message_ids(&mut self, chat_ids: &[Uuid]) -> Result<Vec<Uuid>, RevokeError> {
        let sql = delete_svc::collect_message_ids_sql(chat_ids.len());
        let mut query = sqlx::query(&sql);
        for id in chat_ids {
            query = query.bind(*id);
        }
        let rows = query.fetch_all(&mut **self.tx).await.map_err(store_err)?;
        rows.iter()
            .map(|row| row.try_get(0).map_err(store_err))
            .collect()
    }

    async fn delete_chat_events(&mut self, chat_ids: &[Uuid]) -> Result<(), RevokeError> {
        let sql = delete_svc::delete_chat_events_sql(chat_ids.len());
        let mut query = sqlx::query(&sql);
        for id in chat_ids {
            query = query.bind(*id);
        }
        query
            .execute(&mut **self.tx)
            .await
            .map(|_| ())
            .map_err(store_err)
    }

    async fn delete_chat_approvals(&mut self, chat_ids: &[Uuid]) -> Result<(), RevokeError> {
        let sql = delete_svc::delete_chat_approvals_sql(chat_ids.len());
        let mut query = sqlx::query(&sql);
        for id in chat_ids {
            query = query.bind(*id);
        }
        query
            .execute(&mut **self.tx)
            .await
            .map(|_| ())
            .map_err(store_err)
    }

    async fn delete_chat_dedupes(&mut self, chat_ids: &[Uuid]) -> Result<(), RevokeError> {
        let sql = delete_svc::delete_chat_dedupes_sql(chat_ids.len());
        let mut query = sqlx::query(&sql);
        for id in chat_ids {
            query = query.bind(*id);
        }
        query
            .execute(&mut **self.tx)
            .await
            .map(|_| ())
            .map_err(store_err)
    }

    async fn delete_runner_sessions(&mut self, runner_ids: &[Uuid]) -> Result<(), RevokeError> {
        pidash_services::domain::tokens::delete_runner_sessions_for_runners(
            &mut **self.tx,
            runner_ids,
        )
        .await
        .map_err(store_err)
    }

    async fn delete_force_refresh(&mut self, runner_ids: &[Uuid]) -> Result<(), RevokeError> {
        let sql = delete_svc::delete_force_refresh_sql(runner_ids.len());
        let mut query = sqlx::query(&sql);
        for id in runner_ids {
            query = query.bind(*id);
        }
        query
            .execute(&mut **self.tx)
            .await
            .map(|_| ())
            .map_err(store_err)
    }

    async fn delete_live_state(&mut self, runner_ids: &[Uuid]) -> Result<(), RevokeError> {
        let sql = delete_svc::delete_live_state_sql(runner_ids.len());
        let mut query = sqlx::query(&sql);
        for id in runner_ids {
            query = query.bind(*id);
        }
        query
            .execute(&mut **self.tx)
            .await
            .map(|_| ())
            .map_err(store_err)
    }

    async fn null_run_runners(&mut self, runner_ids: &[Uuid]) -> Result<(), RevokeError> {
        let sql = delete_svc::null_run_runners_sql(runner_ids.len());
        let mut query = sqlx::query(&sql);
        for id in runner_ids {
            query = query.bind(*id);
        }
        query
            .execute(&mut **self.tx)
            .await
            .map(|_| ())
            .map_err(store_err)
    }

    async fn null_run_pins(&mut self, runner_ids: &[Uuid]) -> Result<(), RevokeError> {
        let sql = delete_svc::null_run_pins_sql(runner_ids.len());
        let mut query = sqlx::query(&sql);
        for id in runner_ids {
            query = query.bind(*id);
        }
        query
            .execute(&mut **self.tx)
            .await
            .map(|_| ())
            .map_err(store_err)
    }

    async fn null_event_messages(&mut self, message_ids: &[Uuid]) -> Result<(), RevokeError> {
        let sql = delete_svc::null_event_messages_sql(message_ids.len());
        let mut query = sqlx::query(&sql);
        for id in message_ids {
            query = query.bind(*id);
        }
        query
            .execute(&mut **self.tx)
            .await
            .map(|_| ())
            .map_err(store_err)
    }

    async fn delete_chat_messages(&mut self, message_ids: &[Uuid]) -> Result<(), RevokeError> {
        let sql = delete_svc::delete_chat_messages_sql(message_ids.len());
        let mut query = sqlx::query(&sql);
        for id in message_ids {
            query = query.bind(*id);
        }
        query
            .execute(&mut **self.tx)
            .await
            .map(|_| ())
            .map_err(store_err)
    }

    async fn delete_chat_sessions(&mut self, chat_ids: &[Uuid]) -> Result<(), RevokeError> {
        let sql = delete_svc::delete_chat_sessions_sql(chat_ids.len());
        let mut query = sqlx::query(&sql);
        for id in chat_ids {
            query = query.bind(*id);
        }
        query
            .execute(&mut **self.tx)
            .await
            .map(|_| ())
            .map_err(store_err)
    }

    async fn delete_runner_rows(&mut self, runner_ids: &[Uuid]) -> Result<(), RevokeError> {
        let sql = delete_svc::delete_runner_rows_sql(runner_ids.len());
        let mut query = sqlx::query(&sql);
        for id in runner_ids {
            query = query.bind(*id);
        }
        query
            .execute(&mut **self.tx)
            .await
            .map(|_| ())
            .map_err(store_err)
    }

    async fn fetch_machine_for_delete(&mut self, machine_id: Uuid) -> Result<bool, RevokeError> {
        sqlx::query(delete_svc::COLLECT_MACHINE_BY_ID_SQL)
            .bind(machine_id)
            .fetch_optional(&mut **self.tx)
            .await
            .map(|row| row.is_some())
            .map_err(store_err)
    }

    async fn delete_machine_sessions(&mut self, machine_id: Uuid) -> Result<(), RevokeError> {
        sqlx::query(delete_svc::DELETE_MACHINE_SESSIONS_SQL)
            .bind(machine_id)
            .execute(&mut **self.tx)
            .await
            .map(|_| ())
            .map_err(store_err)
    }

    async fn null_runner_machines(&mut self, machine_id: Uuid) -> Result<(), RevokeError> {
        sqlx::query(delete_svc::NULL_RUNNER_MACHINES_SQL)
            .bind(machine_id)
            .execute(&mut **self.tx)
            .await
            .map(|_| ())
            .map_err(store_err)
    }

    async fn null_token_machines(&mut self, machine_id: Uuid) -> Result<(), RevokeError> {
        pidash_services::domain::tokens::clear_token_dev_machines(&mut **self.tx, machine_id)
            .await
            .map_err(store_err)
    }

    async fn delete_machine_row(&mut self, machine_id: Uuid) -> Result<(), RevokeError> {
        sqlx::query(delete_svc::DELETE_MACHINE_ROW_SQL)
            .bind(machine_id)
            .execute(&mut **self.tx)
            .await
            .map(|_| ())
            .map_err(store_err)
    }
}

/// Pool-backed [`PubsubStore`] for the escaped frame/close path
/// (moved verbatim from the enroll delete executor).
struct PoolPubsub<'a> {
    pool: &'a PgPool,
    redis: Option<&'a redis::Client>,
    runner: &'a pidash_db::config::RunnerSettings,
}

#[allow(async_fn_in_trait)]
impl PubsubStore for PoolPubsub<'_> {
    async fn enqueue_for_runner(
        &self,
        runner_id: Uuid,
        message: &Map<String, Value>,
    ) -> Result<Option<String>, OutboxError> {
        runner_outbox::enqueue_for_runner(self.redis, self.pool, self.runner, runner_id, message)
            .await
    }

    async fn enqueue_for_machine(
        &self,
        dev_machine_id: Uuid,
        message: &Map<String, Value>,
    ) -> Result<Option<String>, MachineOutboxError> {
        machine_outbox::enqueue_for_machine(
            self.redis,
            self.pool,
            self.runner,
            dev_machine_id,
            message,
        )
        .await
    }

    async fn active_runner_sessions(
        &self,
        runner_id: Uuid,
    ) -> Result<Vec<RunnerSession>, OutboxError> {
        let rows = sqlx::query(pubsub_svc::CLOSE_ACTIVE_SESSIONS_SQL)
            .bind(runner_id)
            .fetch_all(self.pool)
            .await?;
        rows.iter()
            .map(runner_session::runner_session_from_row)
            .collect::<Result<Vec<_>, _>>()
            .map_err(OutboxError::from)
    }

    async fn revoke_runner_session(
        &self,
        session_id: Uuid,
        reason: &str,
    ) -> Result<(), OutboxError> {
        pidash_services::domain::tokens::revoke_runner_session(
            self.pool,
            session_id,
            Utc::now(),
            reason,
        )
        .await
        .map_err(OutboxError::from)
    }

    async fn clear_session_marker(&self, session_id: Uuid) -> Result<(), OutboxError> {
        runner_outbox::clear_session_marker(self.redis, &session_id.to_string()).await
    }

    async fn publish_session_eviction(
        &self,
        runner_id: Uuid,
        old_session_id: Uuid,
        new_session_id: &str,
    ) -> Result<(), OutboxError> {
        runner_outbox::publish_session_eviction(
            self.redis,
            &runner_id.to_string(),
            Some(&old_session_id.to_string()),
            new_session_id,
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    fn test_ctx() -> RevokeCtx {
        RevokeCtx {
            pool: sqlx::postgres::PgPoolOptions::new()
                .acquire_timeout(std::time::Duration::from_secs(1))
                .connect_lazy("postgres://127.0.0.1:1/revoke_test")
                .expect("lazy pool never connects"),
            redis: None,
            runner: RunnerSettings {
                long_poll_interval_secs: 25,
                access_token_ttl_secs: 900,
                offline_threshold_secs: 90,
                offline_pin_release_secs: 60,
                offline_run_fail_secs: 300,
                offline_stream_ttl_secs: 86400,
                offline_stream_maxlen: 1000,
                stream_min_retention_secs: 3600,
                event_batch_max_age_ms: 5000,
                event_batch_max_bytes: 65536,
                run_message_dedupe_ttl_secs: 3600,
                latest_runner_version: None,
                min_runner_version: None,
                agent_stall_threshold_secs: 300,
                agent_observability_stale_secs: 600,
            },
            atomic_effects: false,
        }
    }

    /// Fire order is the `fire_plan` order (publish pairs, handoffs,
    /// drains, cleanup), with no I/O on this bundle (empty drains,
    /// redis-less cleanup).
    #[tokio::test]
    async fn fire_order_is_publish_handoffs_drains_cleanup() {
        let ctx = test_ctx();
        let run = Uuid::new_v4();
        let runner = Uuid::new_v4();
        let bundle = RevokeBundle {
            publish: vec![
                LifecycleEffect::PublishTerminalEffects { run_id: run },
                LifecycleEffect::ApplyTerminalEffectsInline { run_id: run },
            ],
            handoffs: vec![(runner, run)],
            drains: Vec::new(),
            cleanups: vec![runner],
        };
        let fired: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let emit_fired = fired.clone();
        let apply_fired = fired.clone();
        let handoff_fired = fired.clone();
        let outcome = fire_revoke_bundle(
            &ctx,
            bundle,
            |run_id| {
                let fired = emit_fired.clone();
                async move {
                    fired.lock().unwrap().push(format!("emit {run_id}"));
                }
            },
            |run_id| {
                let fired = apply_fired.clone();
                async move {
                    fired.lock().unwrap().push(format!("apply {run_id}"));
                }
            },
            |runner_id, run_id| {
                let fired = handoff_fired.clone();
                async move {
                    fired
                        .lock()
                        .unwrap()
                        .push(format!("handoff {runner_id} {run_id}"));
                }
            },
        )
        .await;
        assert!(outcome.is_ok());
        assert_eq!(
            *fired.lock().unwrap(),
            vec![
                format!("emit {run}"),
                format!("apply {run}"),
                format!("handoff {runner} {run}"),
            ]
        );
    }

    /// A failing drain propagates and skips the cleanup (Django's bare
    /// `on_commit` chain); the lazy pool fails every query.
    #[tokio::test]
    async fn fire_drain_failure_propagates() {
        let ctx = test_ctx();
        let bundle = RevokeBundle {
            publish: Vec::new(),
            handoffs: Vec::new(),
            drains: vec![Uuid::new_v4()],
            cleanups: vec![Uuid::new_v4()],
        };
        let outcome =
            fire_revoke_bundle(&ctx, bundle, |_| async {}, |_| async {}, |_, _| async {}).await;
        assert!(matches!(outcome, Err(RevokeFireError::Drain)));
    }

    /// A failing cleanup propagates: redis points at a closed port.
    #[tokio::test]
    async fn fire_cleanup_failure_propagates() {
        let mut ctx = test_ctx();
        ctx.redis = Some(
            redis::Client::open("redis://127.0.0.1:1/")
                .expect("client construction never connects"),
        );
        let bundle = RevokeBundle {
            publish: Vec::new(),
            handoffs: Vec::new(),
            drains: Vec::new(),
            cleanups: vec![Uuid::new_v4()],
        };
        let outcome =
            fire_revoke_bundle(&ctx, bundle, |_| async {}, |_| async {}, |_, _| async {}).await;
        assert!(matches!(outcome, Err(RevokeFireError::Cleanup)));
    }
}
