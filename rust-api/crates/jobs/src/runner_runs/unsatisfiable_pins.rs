#![forbid(unsafe_code)]

//! Unsatisfiable-pin recovery beat task (PDASHOSS01-233).
//!
//! Executor for `runner.reconcile_unsatisfiable_pins`
//! (`apps/api/pi_dash/runner/tasks.py`) over
//! `matcher.reconcile_unsatisfiable_pins` (PR #541): release wedged-busy
//! pinned runners, clear pins held past `RUNNER_PIN_AUTO_RELEASE_SECS`,
//! then drain every affected pod so the recovered runs are assigned in
//! the same pass. SQL text lives in the services planners
//! (`pidash_services::runner_sessions::unsatisfiable_pins`); this layer
//! holds the settings reader, the pool-direct driver (the
//! `offline_recovery` precedent — no store trait), the worker handler
//! and the registration.
//!
//! # Ported quirks (translate, don't redesign)
//!
//! * Each wedged release autocommits on its own (Django runs the
//!   `.update()` outside any `atomic()`); only the stale-pin pass is one
//!   transaction (lock, pin clear, parent clear).
//! * Each pod drain is isolated (logged, not propagated): the recovered
//!   pins are not revisited by the next pass, so one failed drain (e.g.
//!   the Assign send refused for a runner with no live session) must not
//!   strand the other pods — the `release_pins_on_offline_runners` rule.
//! * Pod ids drain in first-seen order (same membership as the source's
//!   set, deterministic); the return value is the affected-pod count.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use pidash_services::runner_sessions::drain::RELEASE_RUNNER_IF_IDLE_SQL;
use pidash_services::runner_sessions::guards::HEARTBEAT_GRACE_SECS;
use pidash_services::runner_sessions::unsatisfiable_pins as kernel;

use super::sweeps_chat::{RunnerRunsSeams, SweepError};
use super::sweeps_runs::offline_cutoff;
use crate::queue::JobRow;
use crate::worker::{HandlerError, Registry, Verdict};

/// `runner.reconcile_unsatisfiable_pins` (`tasks.py`, PDASHOSS01-233).
pub const RECONCILE_UNSATISFIABLE_PINS_TASK: &str = "runner.reconcile_unsatisfiable_pins";

/// Beat entry for [`RECONCILE_UNSATISFIABLE_PINS_TASK`] (`celery.py`,
/// every minute).
pub const RECONCILE_UNSATISFIABLE_PINS_BEAT_NAME: &str = "runner-reconcile-unsatisfiable-pins";

/// Env knob for the auto-release bound (same name as Django).
pub const PIN_AUTO_RELEASE_ENV: &str = "RUNNER_PIN_AUTO_RELEASE_SECS";

/// `int(getattr(settings, "RUNNER_PIN_AUTO_RELEASE_SECS", 600))`.
/// Unparseable input falls back to the default, read per tick (the
/// `offline_recovery::pin_release_secs_raw` precedent).
pub fn pin_auto_release_secs_raw(raw: Option<&str>) -> i64 {
    raw.and_then(|v| v.trim().parse::<i64>().ok())
        .unwrap_or(kernel::DEFAULT_PIN_AUTO_RELEASE_SECS)
}

/// Auto-release bound in seconds. `<= 0` disables remedy 2 only; the
/// wedged-busy release always runs.
pub fn pin_auto_release_secs() -> i64 {
    pin_auto_release_secs_raw(std::env::var(PIN_AUTO_RELEASE_ENV).ok().as_deref())
}

fn note_pod(pod_ids: &mut Vec<Uuid>, pod_id: Option<Uuid>) {
    if let Some(pod_id) = pod_id {
        if !pod_ids.contains(&pod_id) {
            pod_ids.push(pod_id);
        }
    }
}

/// `matcher.reconcile_unsatisfiable_pins()` (no pod filter — the beat
/// task's call): returns the pods whose queue or capacity changed, in
/// first-seen order.
pub async fn reconcile_unsatisfiable_pins_pods(
    pool: &PgPool,
    now: &DateTime<Utc>,
    bound_secs: i64,
) -> Result<Vec<Uuid>, SweepError> {
    let alive = offline_cutoff(now, HEARTBEAT_GRACE_SECS);
    let mut pod_ids: Vec<Uuid> = Vec::new();

    // (1) Wedged-busy pinned runners that are still alive: release them.
    let wedged: Vec<(Uuid, Option<Uuid>, DateTime<Utc>)> =
        sqlx::query_as(kernel::wedged_pinned_runners_select())
            .bind(alive)
            .fetch_all(pool)
            .await?;
    for (runner_id, pod_id, _) in wedged {
        let released = sqlx::query(RELEASE_RUNNER_IF_IDLE_SQL)
            .bind(runner_id)
            .execute(pool)
            .await?
            .rows_affected()
            > 0;
        if released {
            tracing::info!("reconcile_unsatisfiable_pins: released wedged busy runner {runner_id}");
            note_pod(&mut pod_ids, pod_id);
        }
    }

    // (2) Bounded pin auto-release.
    if bound_secs > 0 {
        let cutoff = offline_cutoff(now, bound_secs);
        let mut tx = pool.begin().await?;
        let stale: Vec<(Uuid, Option<Uuid>, Option<Uuid>)> =
            sqlx::query_as(kernel::stale_pins_lock_select())
                .bind(cutoff)
                .bind(alive)
                .fetch_all(&mut *tx)
                .await?;
        if !stale.is_empty() {
            let ids: Vec<Uuid> = stale.iter().map(|(id, _, _)| *id).collect();
            let clear_pins = kernel::clear_pins_sql(ids.len());
            let mut query = sqlx::query(&clear_pins);
            for id in &ids {
                query = query.bind(*id);
            }
            query.execute(&mut *tx).await?;
            // Same as the manual release-pin endpoint: whichever runner
            // takes the run next has no session of the parent's to
            // resume against, so drop the resume hint.
            let clear_parents = kernel::clear_parent_threads_sql(ids.len());
            let mut query = sqlx::query(&clear_parents);
            for id in &ids {
                query = query.bind(*id);
            }
            query.execute(&mut *tx).await?;
        }
        tx.commit().await?;
        for (run_id, runner_id, pod_id) in stale {
            let runner = runner_id.map(|id| id.to_string()).unwrap_or_default();
            tracing::info!(
                "reconcile_unsatisfiable_pins: auto-released pin run={run_id} runner={runner} after {bound_secs}s bound"
            );
            note_pod(&mut pod_ids, pod_id);
        }
    }

    Ok(pod_ids)
}

/// `runner.reconcile_unsatisfiable_pins`: recover the pins, then drain
/// each affected pod (each drain isolated). Returns the affected-pod
/// count.
pub async fn reconcile_unsatisfiable_pins(
    pool: &PgPool,
    seams: &dyn RunnerRunsSeams,
    now: &DateTime<Utc>,
    bound_secs: i64,
) -> Result<usize, SweepError> {
    let pod_ids = reconcile_unsatisfiable_pins_pods(pool, now, bound_secs).await?;
    for pod_id in &pod_ids {
        // The pins are already cleared and the runners released, so the
        // next pass will not come back to these pods: one pod's failed
        // drain must not skip the others.
        if let Err(error) = seams.drain_pod(*pod_id).await {
            tracing::error!(%error, "reconcile_unsatisfiable_pins: drain failed for pod {pod_id}");
        }
    }
    Ok(pod_ids.len())
}

/// Register [`RECONCILE_UNSATISFIABLE_PINS_TASK`] on the live worker.
pub fn register_unsatisfiable_pins_tasks(
    registry: &mut Registry,
    pool: PgPool,
    seams: Arc<dyn RunnerRunsSeams>,
) {
    registry.register(
        RECONCILE_UNSATISFIABLE_PINS_TASK,
        Arc::new(move |_job: JobRow| {
            let pool = pool.clone();
            let seams = seams.clone();
            let fut: Pin<Box<dyn Future<Output = Result<Verdict, HandlerError>> + Send>> =
                Box::pin(async move {
                    let now = Utc::now();
                    reconcile_unsatisfiable_pins(
                        &pool,
                        seams.as_ref(),
                        &now,
                        pin_auto_release_secs(),
                    )
                    .await
                    .map(|_| Verdict::Ack)
                    .map_err(|error| error.to_string())
                });
            fut
        }),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_name_matches_django() {
        assert_eq!(
            RECONCILE_UNSATISFIABLE_PINS_TASK,
            "runner.reconcile_unsatisfiable_pins"
        );
        assert_eq!(
            RECONCILE_UNSATISFIABLE_PINS_BEAT_NAME,
            "runner-reconcile-unsatisfiable-pins"
        );
        let entries = crate::schedule::beat_schedule();
        let entry = entries
            .iter()
            .find(|e| e.name == RECONCILE_UNSATISFIABLE_PINS_BEAT_NAME)
            .expect("beat entry");
        assert_eq!(entry.task, RECONCILE_UNSATISFIABLE_PINS_TASK);
    }

    #[test]
    fn settings_reader_falls_back_per_tick() {
        assert_eq!(pin_auto_release_secs_raw(None), 600);
        assert_eq!(pin_auto_release_secs_raw(Some("60")), 60);
        assert_eq!(pin_auto_release_secs_raw(Some("0")), 0);
        assert_eq!(pin_auto_release_secs_raw(Some("-1")), -1);
        assert_eq!(pin_auto_release_secs_raw(Some("bad")), 600);
    }
}
