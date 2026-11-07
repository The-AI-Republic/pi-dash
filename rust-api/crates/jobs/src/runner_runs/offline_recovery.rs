#![forbid(unsafe_code)]

//! Offline-runner recovery sweeps (PIDASHCONV-1063, stage 8 drift).
//!
//! Executors for the `70d684af` task bodies
//! (`apps/api/pi_dash/runner/tasks.py`): [`RELEASE_PINS_TASK`] returns
//! QUEUED runs pinned to a long-silent runner to the pod queue, and
//! [`FAIL_RUNS_TASK`] terminates BUSY runs whose runner never came
//! back. SQL text lives in the services planners
//! (`pidash_services::runner_sessions::offline_recovery`); this layer
//! holds the settings readers, the pool-direct drivers (the
//! `reconcile_stalled_runs` precedent — no store trait), the worker
//! handlers and the registration.
//!
//! Drains and project-move handoffs run through
//! [`RunnerRunsSeams`](super::sweeps_chat::RunnerRunsSeams) (live
//! providers in `oss_worker::runner_seams`); terminal effects reuse
//! the L6b pair (queue emit + inline apply) exactly like the stall
//! watchdog.
//!
//! # Ported quirks (translate, don't redesign)
//!
//! * The release sweep has no per-run guard: a lock/write failure
//!   aborts the task (retry), while each pod drain is isolated. The
//!   fail sweep guards per runner instead.
//! * A drain failure after a runner's commit keeps that runner's
//!   `finished` count: Django increments before `on_commit` fires.
//! * The fail sweep's runner ids dedupe in first-seen order (the
//!   `DISTINCT`-triple quirk); pod sets drain in first-seen order
//!   (same membership as the source's set iteration, deterministic).

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use sea_orm::sea_query::Expr;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use sqlx::PgPool;
use uuid::Uuid;

use pidash_db::entity as orm;
use pidash_db::orm_tx::TxConn;
use pidash_db::pool::sea_orm_connection;
use pidash_services::runner_runs::finalization;
use pidash_services::runner_runs::{LifecycleEffect, SetValue};
use pidash_services::runner_sessions::offline_recovery as kernel;
use pidash_services::runner_sessions::session_service::RUNNER_OFFLINE_ERROR_CODE;
use pidash_types::dispatch::AgentExecutorKind;
use pidash_types::runner_runs::AgentRunStatus;

use super::sweeps_chat::{
    apply_agent_run_terminal_effects, terminal_effects_job, RunnerRunsSeams, SweepError,
};
use super::sweeps_runs::offline_cutoff;
use crate::queue::{enqueue, JobRow};
use crate::worker::{HandlerError, Registry, Verdict};

// ---------------------------------------------------------------------------
// Task names + beat names
// ---------------------------------------------------------------------------

/// `runner.release_pins_on_offline_runners` (`tasks.py`, `70d684af`).
pub const RELEASE_PINS_TASK: &str = "runner.release_pins_on_offline_runners";
/// `runner.fail_runs_on_offline_runners` (`tasks.py`, `70d684af`).
pub const FAIL_RUNS_TASK: &str = "runner.fail_runs_on_offline_runners";

/// Beat entry for [`RELEASE_PINS_TASK`] (`celery.py`, every minute).
pub const RELEASE_PINS_BEAT_NAME: &str = "runner-release-pins-on-offline-runners";
/// Beat entry for [`FAIL_RUNS_TASK`] (`celery.py`, every minute).
pub const FAIL_RUNS_BEAT_NAME: &str = "runner-fail-runs-on-offline-runners";

// ---------------------------------------------------------------------------
// Settings (keys, defaults, readers)
// ---------------------------------------------------------------------------

/// `RUNNER_OFFLINE_PIN_RELEASE_SECS` default (`settings/common.py`).
pub const DEFAULT_PIN_RELEASE_SECS: i64 = 300;
/// `RUNNER_OFFLINE_RUN_FAIL_SECS` default (`settings/common.py`).
pub const DEFAULT_RUN_FAIL_SECS: i64 = 3600;

/// Env knob for the pin-release grace (same name as Django).
pub const PIN_RELEASE_ENV: &str = "RUNNER_OFFLINE_PIN_RELEASE_SECS";
/// Env knob for the run-fail grace (same name as Django).
pub const RUN_FAIL_ENV: &str = "RUNNER_OFFLINE_RUN_FAIL_SECS";

/// `int(getattr(settings, "RUNNER_OFFLINE_PIN_RELEASE_SECS", 300))`.
/// Unparseable input falls back to the default: Python parses the
/// same knob once at Django boot (where an invalid value refuses to
/// boot), while here the value is read per tick and the task must
/// never crash on it (the loop-scan settings precedent).
pub fn pin_release_secs_raw(raw: Option<&str>) -> i64 {
    raw.and_then(|v| v.trim().parse::<i64>().ok())
        .unwrap_or(DEFAULT_PIN_RELEASE_SECS)
}

/// `int(getattr(settings, "RUNNER_OFFLINE_RUN_FAIL_SECS", 3600))`;
/// same per-tick fallback as [`pin_release_secs_raw`].
pub fn run_fail_secs_raw(raw: Option<&str>) -> i64 {
    raw.and_then(|v| v.trim().parse::<i64>().ok())
        .unwrap_or(DEFAULT_RUN_FAIL_SECS)
}

/// Pin-release grace in seconds. `<= 0` disables the sweep.
pub fn pin_release_secs() -> i64 {
    pin_release_secs_raw(std::env::var(PIN_RELEASE_ENV).ok().as_deref())
}

/// Run-fail grace in seconds. `<= 0` disables the sweep.
pub fn run_fail_secs() -> i64 {
    run_fail_secs_raw(std::env::var(RUN_FAIL_ENV).ok().as_deref())
}

// ---------------------------------------------------------------------------
// Release-pins sweep
// ---------------------------------------------------------------------------

/// One stuck pin's atomic block: lock the still-queued, still-pinned,
/// still-silent run (`FOR UPDATE OF "agent_run"`), clear the parent's
/// `thread_id` when set, clear the pin. Returns the run's pod id when
/// a row was released (`None` pod when the column is null — the
/// source's `if run.pod_id is not None` gate), or `None` on a lock
/// miss. A lock miss rolls back the empty transaction.
async fn release_pin_atomic(
    pool: &PgPool,
    run_id: Uuid,
    cutoff: DateTime<Utc>,
) -> Result<Option<Option<Uuid>>, SweepError> {
    let mut tx = pool.begin().await?;
    let row = kernel::release_pin_lock_select(run_id, cutoff)
        .one(&TxConn::new(&mut tx))
        .await?;
    let Some(row) = row else {
        tx.rollback().await?;
        return Ok(None);
    };
    // Same as the manual release-pin endpoint: whichever runner takes
    // the run next has no session of the parent's to resume against.
    // Parent write first, pin write second (source order). The parent
    // join is a LEFT OUTER JOIN: no parent, no thread.
    let conn = TxConn::new(&mut tx);
    if let Some(parent_id) = row.parent_run_id.filter(|_| {
        row.parent_thread_id
            .as_deref()
            .is_some_and(|t| !t.is_empty())
    }) {
        orm::agent_run::Entity::update_many()
            .col_expr(orm::agent_run::Column::ThreadId, Expr::value(String::new()))
            .filter(orm::agent_run::Column::Id.eq(parent_id))
            .exec(&conn)
            .await?;
    }
    orm::agent_run::Entity::update_many()
        .col_expr(
            orm::agent_run::Column::PinnedRunnerId,
            Expr::value(Option::<Uuid>::None),
        )
        .filter(orm::agent_run::Column::Id.eq(run_id))
        .exec(&conn)
        .await?;
    // `pod_id` is NOT NULL: the source's `if run.pod_id is not None`
    // gate always passes.
    let pod_id = Some(row.pod_id);
    tx.commit().await?;
    Ok(Some(pod_id))
}

/// `runner.release_pins_on_offline_runners`: unpin QUEUED runs stuck
/// on long-silent runners (500 per pass), then drain each affected
/// pod — each drain isolated, so one failed drain never skips the
/// rest. Returns the released count.
pub async fn release_pins_on_offline_runners(
    pool: &PgPool,
    seams: &dyn RunnerRunsSeams,
    now: &DateTime<Utc>,
    grace_secs: i64,
) -> Result<usize, SweepError> {
    if grace_secs <= 0 {
        return Ok(0);
    }
    let cutoff = offline_cutoff(now, grace_secs);
    let stuck: Vec<Uuid> = kernel::stuck_queued_run_ids_select(cutoff)
        .all(&sea_orm_connection(pool))
        .await?;
    let mut released = 0usize;
    let mut pod_ids: Vec<Uuid> = Vec::new();
    for run_id in stuck {
        // No per-run guard (unlike the fail sweep): a lock/write
        // failure aborts the task, exactly as the source.
        if let Some(pod_id) = release_pin_atomic(pool, run_id, cutoff).await? {
            released += 1;
            if let Some(pod_id) = pod_id {
                if !pod_ids.contains(&pod_id) {
                    pod_ids.push(pod_id);
                }
            }
        }
    }
    for pod_id in pod_ids {
        // The runs are already unpinned, so this sweep will not come
        // back to them: one pod's failed drain must not skip the
        // others.
        if let Err(error) = seams.drain_pod(pod_id).await {
            tracing::error!(%error, "release pins: drain failed for pod {pod_id}");
        }
    }
    if released > 0 {
        tracing::info!("release_pins_on_offline_runners unpinned {released} queued run(s)");
    }
    Ok(released)
}

// ---------------------------------------------------------------------------
// Fail-runs sweep (`_finish_unclaimed_runs` with `stamp_cancelled=True`)
// ---------------------------------------------------------------------------

/// What one runner's finish transaction committed: the finalized run
/// ids (terminal-effects targets, in reap order), the stopped-cancel
/// ids (handoff targets, in barrier order), the drain pod ids
/// (first-seen order), and the `_finish_unclaimed_runs` count.
struct FinishOutcome {
    finalized: Vec<Uuid>,
    handoffs: Vec<Uuid>,
    pods: Vec<Uuid>,
    count: usize,
}

impl FinishOutcome {
    fn empty() -> Self {
        Self {
            finalized: Vec::new(),
            handoffs: Vec::new(),
            pods: Vec::new(),
            count: 0,
        }
    }
}

/// `finalize_agent_run(run_id, FAILED, updates, expected_runner_id)`
/// (`agent_run_finalization.py:47-87`): lock the non-terminal row for
/// this runner, write the values, insert the cloud-only terminal
/// event. A lock miss finalizes nothing (the source's `False`) —
/// silently: unlike `finalize_run_terminal`, there is no
/// late-transition log line here. No `done_payload` clause, so the
/// merge step is a no-op.
async fn finalize_offline_run(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    run_id: &Uuid,
    runner_id: &Uuid,
    detail: &str,
) -> Result<bool, SweepError> {
    let values = finalization::plan_finalize_values(
        AgentRunStatus::Failed,
        &[
            ("error", SetValue::Text(detail.to_owned())),
            (
                "error_code",
                SetValue::Text(RUNNER_OFFLINE_ERROR_CODE.to_owned()),
            ),
        ],
    )
    .map_err(|_| SweepError::BadRow("finalize plan"))?;
    let conn = TxConn::new(tx);
    let Some(locked) =
        finalization::lock_run_for_finalize(&conn, *run_id, Some(*runner_id), None).await?
    else {
        return Ok(false);
    };
    let now = Utc::now();
    finalization::finalize_update(&conn, &values, *run_id, now).await?;
    if AgentExecutorKind::from_value(&locked.executor_kind) == Some(AgentExecutorKind::CloudAgent) {
        finalization::append_terminal_event_once(
            &conn,
            *run_id,
            AgentRunStatus::Failed,
            &finalization::finalize_error_code(&values),
            now,
        )
        .await?;
    }
    Ok(true)
}

/// One runner's finish transaction: re-check the silence under the
/// row lock, barrier stopped cancels (stamped), finalize each reaped
/// run. A lock miss rolls back the empty transaction and finishes
/// nothing — the runner came back since the list was read.
async fn fail_runs_txn(
    pool: &PgPool,
    runner_id: Uuid,
    cutoff: DateTime<Utc>,
    detail: &str,
) -> Result<FinishOutcome, SweepError> {
    let mut tx = pool.begin().await?;
    // The poll takes the same row lock before it writes the
    // heartbeat, so re-checking under it means a runner that came
    // back since the list was read keeps its runs.
    let conn = TxConn::new(&mut tx);
    let locked = kernel::runner_silent_lock_select(runner_id, cutoff)
        .one(&conn)
        .await?;
    if locked.is_none() {
        tx.rollback().await?;
        return Ok(FinishOutcome::empty());
    }
    let cancel_ids: Vec<Uuid> = kernel::sweep_stale_cancel_ids_sql(cutoff, runner_id)
        .all(&conn)
        .await?;
    let cancel_pods: Vec<Uuid> = kernel::sweep_stale_cancel_pods_sql(cutoff, runner_id)
        .all(&conn)
        .await?;
    if !cancel_ids.is_empty() {
        kernel::sweep_cancel_barrier_stamped_sql(
            Utc::now(),
            RUNNER_OFFLINE_ERROR_CODE,
            &cancel_ids,
        )
        .exec(&conn)
        .await?;
    }
    let reaped: Vec<(Uuid, Uuid)> = kernel::sweep_stale_pairs_sql(cutoff, runner_id, &cancel_ids)
        .all(&conn)
        .await?;
    if reaped.is_empty() && cancel_ids.is_empty() {
        tx.rollback().await?;
        return Ok(FinishOutcome::empty());
    }
    let mut finalized = Vec::new();
    for (run_id, _) in &reaped {
        if finalize_offline_run(&mut tx, run_id, &runner_id, detail).await? {
            finalized.push(*run_id);
        }
    }
    tx.commit().await?;
    // First-seen pod order: reaped runs, then stopped cancels —
    // skipping nulls. Same membership as the source's set union.
    let mut pods = Vec::new();
    for maybe in reaped.iter().map(|(_, pod)| pod).chain(cancel_pods.iter()) {
        if !pods.contains(maybe) {
            pods.push(*maybe);
        }
    }
    let count = finalized.len() + cancel_ids.len();
    Ok(FinishOutcome {
        finalized,
        handoffs: cancel_ids,
        pods,
        count,
    })
}

/// `_publish_effects` (`agent_run_finalization.py:89-103`) for one
/// sweep-finalized run: the Celery emit first, then the inline apply
/// — each isolated, in order.
async fn publish_offline_effects(pool: &PgPool, seams: &dyn RunnerRunsSeams, run_id: &Uuid) {
    for effect in finalization::plan_publish_effects(*run_id) {
        match effect {
            LifecycleEffect::PublishTerminalEffects { run_id } => {
                if let Err(error) = enqueue(pool, &terminal_effects_job(&run_id)).await {
                    tracing::error!(%error, "failed to publish terminal effects for run {run_id}");
                }
            }
            LifecycleEffect::ApplyTerminalEffectsInline { run_id } => {
                if let Err(error) = apply_agent_run_terminal_effects(pool, seams, &run_id).await {
                    tracing::error!(%error, "failed to apply terminal effects for run {run_id}");
                }
            }
            // Unreachable: `plan_publish_effects` emits exactly these two.
            _ => {}
        }
    }
}

/// `_drain_after_commit`: one handoff per stopped-cancel id in
/// barrier order, then the runner drain, then one pod drain per pod
/// id — sequential and unisolated, so the first failure skips the
/// rest and propagates to the per-runner guard.
async fn run_finish_drains(
    seams: &dyn RunnerRunsSeams,
    runner_id: Uuid,
    outcome: &FinishOutcome,
) -> Result<(), String> {
    for handoff_id in &outcome.handoffs {
        seams
            .complete_project_move_handoff(*handoff_id)
            .await
            .map_err(|error| error.to_string())?;
    }
    seams
        .drain_runner(runner_id)
        .await
        .map_err(|error| error.to_string())?;
    for pod_id in &outcome.pods {
        seams
            .drain_pod(*pod_id)
            .await
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

/// `runner.fail_runs_on_offline_runners`: terminate BUSY runs whose
/// runner has been silent past the grace. Per-runner guard: one bad
/// row never stops the sweep from recovering the rest. Returns the
/// finished count.
pub async fn fail_runs_on_offline_runners(
    pool: &PgPool,
    seams: &dyn RunnerRunsSeams,
    now: &DateTime<Utc>,
    grace_secs: i64,
) -> Result<usize, SweepError> {
    if grace_secs <= 0 {
        return Ok(0);
    }
    let cutoff = offline_cutoff(now, grace_secs);
    let rows: Vec<(Uuid, Option<DateTime<Utc>>, DateTime<Utc>)> =
        kernel::silent_runner_ids_select(cutoff)
            .all(&sea_orm_connection(pool))
            .await?;
    // Dedupe the `DISTINCT`-triple rows in first-seen order.
    let mut runner_ids = Vec::new();
    for (runner_id, _, _) in rows {
        if !runner_ids.contains(&runner_id) {
            runner_ids.push(runner_id);
        }
    }
    let detail = kernel::offline_fail_detail(grace_secs);
    let mut finished = 0usize;
    for runner_id in runner_ids {
        // Per-runner guard, as in `reconcile_stalled_runs`: one bad
        // row must not stop the sweep from recovering the rest.
        match fail_runs_txn(pool, runner_id, cutoff, &detail).await {
            Ok(outcome) => {
                // Terminal effects first (registered before the drain
                // callback), each isolated — then the drains, whose
                // first failure keeps this runner's count (Django
                // increments before `on_commit` fires).
                for run_id in &outcome.finalized {
                    publish_offline_effects(pool, seams, run_id).await;
                }
                if let Err(error) = run_finish_drains(seams, runner_id, &outcome).await {
                    tracing::error!(%error, "fail_runs_on_offline_runners: failed for runner {runner_id}");
                }
                finished += outcome.count;
            }
            Err(error) => {
                tracing::error!(%error, "fail_runs_on_offline_runners: failed for runner {runner_id}");
            }
        }
    }
    if finished > 0 {
        tracing::info!(
            "fail_runs_on_offline_runners finished {finished} run(s) (grace={grace_secs}s)"
        );
    }
    Ok(finished)
}

// ---------------------------------------------------------------------------
// Handlers + registration
// ---------------------------------------------------------------------------

/// Register the two offline-recovery task handlers. The pool feeds
/// the drivers; `seams` is the D-14/D-12 provider the binary owns
/// (the `register_runner_runs_tasks` precedent).
pub fn register_offline_recovery_tasks(
    registry: &mut Registry,
    pool: PgPool,
    seams: Arc<dyn RunnerRunsSeams>,
) {
    {
        let pool = pool.clone();
        let seams = seams.clone();
        registry.register(
            RELEASE_PINS_TASK,
            Arc::new(move |_job: JobRow| {
                let pool = pool.clone();
                let seams = seams.clone();
                let fut: Pin<Box<dyn Future<Output = Result<Verdict, HandlerError>> + Send>> =
                    Box::pin(async move {
                        let now = Utc::now();
                        release_pins_on_offline_runners(
                            &pool,
                            seams.as_ref(),
                            &now,
                            pin_release_secs(),
                        )
                        .await
                        .map(|_| Verdict::Ack)
                        .map_err(|error| error.to_string())
                    });
                fut
            }),
        );
    }
    {
        let pool = pool.clone();
        registry.register(
            FAIL_RUNS_TASK,
            Arc::new(move |_job: JobRow| {
                let pool = pool.clone();
                let seams = seams.clone();
                let fut: Pin<Box<dyn Future<Output = Result<Verdict, HandlerError>> + Send>> =
                    Box::pin(async move {
                        let now = Utc::now();
                        fail_runs_on_offline_runners(&pool, seams.as_ref(), &now, run_fail_secs())
                            .await
                            .map(|_| Verdict::Ack)
                            .map_err(|error| error.to_string())
                    });
                fut
            }),
        );
    }
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::pin::Pin;

    use chrono::Utc;
    use sqlx::PgPool;
    use uuid::Uuid;

    use super::super::sweeps_chat::SeamError;
    use super::*;

    #[test]
    fn task_names_match_django() {
        assert_eq!(RELEASE_PINS_TASK, "runner.release_pins_on_offline_runners");
        assert_eq!(FAIL_RUNS_TASK, "runner.fail_runs_on_offline_runners");
        assert_eq!(
            RELEASE_PINS_BEAT_NAME,
            "runner-release-pins-on-offline-runners"
        );
        assert_eq!(FAIL_RUNS_BEAT_NAME, "runner-fail-runs-on-offline-runners");
        // Both beat entries exist in the transcribed schedule.
        let entries = crate::schedule::beat_schedule();
        for (beat, task) in [
            (RELEASE_PINS_BEAT_NAME, RELEASE_PINS_TASK),
            (FAIL_RUNS_BEAT_NAME, FAIL_RUNS_TASK),
        ] {
            let entry = entries.iter().find(|e| e.name == beat).expect(beat);
            assert_eq!(entry.task, task);
        }
    }

    #[test]
    fn settings_readers_fall_back_per_tick() {
        assert_eq!(pin_release_secs_raw(None), 300);
        assert_eq!(pin_release_secs_raw(Some("60")), 60);
        assert_eq!(pin_release_secs_raw(Some("0")), 0);
        assert_eq!(pin_release_secs_raw(Some("-1")), -1);
        assert_eq!(pin_release_secs_raw(Some("bad")), 300);
        assert_eq!(run_fail_secs_raw(None), 3600);
        assert_eq!(run_fail_secs_raw(Some("60")), 60);
        assert_eq!(run_fail_secs_raw(Some("0")), 0);
        assert_eq!(run_fail_secs_raw(Some("bad")), 3600);
    }

    /// Seams that must never fire: the disabled sweep returns before
    /// any seam (or DB) use.
    struct NeverSeams;

    impl RunnerRunsSeams for NeverSeams {
        fn drain_runner(
            &self,
            _runner_id: Uuid,
        ) -> Pin<Box<dyn Future<Output = Result<(), SeamError>> + Send + '_>> {
            panic!("disabled sweep must not drain")
        }
        fn drain_pod(
            &self,
            _pod_id: Uuid,
        ) -> Pin<Box<dyn Future<Output = Result<(), SeamError>> + Send + '_>> {
            panic!("disabled sweep must not drain")
        }
        fn dispatch_waiting(
            &self,
            _workspace_id: Uuid,
        ) -> Pin<Box<dyn Future<Output = Result<(), SeamError>> + Send + '_>> {
            panic!("disabled sweep must not dispatch")
        }
        fn complete_project_move_handoff(
            &self,
            _run_id: Uuid,
        ) -> Pin<Box<dyn Future<Output = Result<(), SeamError>> + Send + '_>> {
            panic!("disabled sweep must not hand off")
        }
        fn disarm_on_terminal_signal(
            &self,
            _run_id: Uuid,
        ) -> Pin<Box<dyn Future<Output = Result<(), SeamError>> + Send + '_>> {
            panic!("disabled sweep must not disarm")
        }
        fn apply_deferred_pause(
            &self,
            _run_id: Uuid,
        ) -> Pin<Box<dyn Future<Output = Result<(), SeamError>> + Send + '_>> {
            panic!("disabled sweep must not pause")
        }
        fn agent_system_user(
            &self,
        ) -> Pin<Box<dyn Future<Output = Result<Uuid, SeamError>> + Send + '_>> {
            panic!("disabled sweep must not comment")
        }
        fn publish_chat_event(
            &self,
            _channel: String,
            _payload: String,
        ) -> Pin<Box<dyn Future<Output = Result<(), SeamError>> + Send + '_>> {
            panic!("disabled sweep must not publish")
        }
    }

    /// `grace <= 0` disables the sweep (`override_settings` cases in
    /// Django's file). The pool points at a refused port, so `Ok(0)`
    /// proves the early return precedes all DB use; the seams panic
    /// on any call.
    #[tokio::test]
    async fn disabled_sweeps_return_zero_without_touching_db() {
        let pool =
            PgPool::connect_lazy("postgres://127.0.0.1:1/pidash_disabled_test").expect("lazy pool");
        let seams = NeverSeams;
        let now = Utc::now();
        for grace in [0, -1] {
            assert_eq!(
                release_pins_on_offline_runners(&pool, &seams, &now, grace)
                    .await
                    .expect("release disabled"),
                0
            );
            assert_eq!(
                fail_runs_on_offline_runners(&pool, &seams, &now, grace)
                    .await
                    .expect("fail disabled"),
                0
            );
        }
    }
}
