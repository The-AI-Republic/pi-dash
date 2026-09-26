#![forbid(unsafe_code)]

//! Beat-equivalent scheduler loop.
//!
//! Every tick the loop computes the due entries of
//! [`beat_schedule`][crate::schedule::beat_schedule] and, for each one,
//! enqueues a queue job **and** records the fire time in the *same*
//! transaction — so a crash between enqueue and bookkeeping can neither
//! lose a fire nor double-fire it on restart.
//!
//! Beat must run as a singleton (the `scheduler.py` / `loop.py` module
//! docstrings say the same about Celery beat: two schedulers double the
//! scan rate). A Postgres advisory lock enforces it: a second scheduler
//! logs and waits instead of firing.

use std::collections::HashMap;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::PgPool;
use tokio::sync::watch;

use crate::queue::{enqueue_exec, NewJob};
use crate::schedule::{beat_schedule, due_entries, mark_run_sql, BeatEntry};

/// Advisory-lock key for the scheduler singleton. Arbitrary but fixed:
/// any second scheduler asking for this key waits on the first.
pub const BEAT_LOCK_KEY: i64 = 918_273_645_546_546;

/// How often the schedule is evaluated. One second keeps the 30-second
/// interval entries (the runner/assistant sweeps) within a second of
/// their nominal fire time without measurable database load: idle ticks
/// are one indexed `SELECT` each.
pub const BEAT_TICK: Duration = Duration::from_secs(1);

/// How long failed queue rows are kept before
/// [`purge`][crate::queue::purge_failed] removes them (7 days).
pub const FAILED_RETENTION_SECS: f64 = 7.0 * 24.0 * 3600.0;

fn try_lock_sql_inner() -> &'static str {
    "SELECT pg_try_advisory_lock($1)"
}

/// Try to take the scheduler singleton lock. True when this process is
/// the scheduler.
pub fn try_lock_sql() -> &'static str {
    try_lock_sql_inner()
}

fn unlock_sql_inner() -> &'static str {
    "SELECT pg_advisory_unlock($1)"
}

/// Release the scheduler singleton lock.
pub fn unlock_sql() -> &'static str {
    unlock_sql_inner()
}

/// Fire every due entry once: one transaction per entry holding the
/// enqueue plus the last-run bookkeeping. Returns the fired entry names.
pub async fn fire_due(
    pool: &PgPool,
    schedule: &[BeatEntry],
    now: &DateTime<Utc>,
) -> Result<Vec<String>, sqlx::Error> {
    let last_runs: HashMap<String, DateTime<Utc>> =
        sqlx::query_as::<_, (String, DateTime<Utc>)>(crate::schedule::fetch_last_runs_sql())
            .fetch_all(pool)
            .await?
            .into_iter()
            .collect();
    let mut fired = Vec::new();
    for entry in due_entries(schedule, now, &last_runs) {
        let mut tx = pool.begin().await?;
        enqueue_exec(
            &mut *tx,
            &NewJob::new(
                entry.task,
                Value::Array(vec![]),
                Value::Object(serde_json::Map::new()),
            ),
        )
        .await?;
        sqlx::query(mark_run_sql())
            .bind(entry.name)
            .bind(*now)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        fired.push(entry.name.to_owned());
    }
    Ok(fired)
}

/// Run the scheduler loop until `shutdown` flips to true: hold the
/// singleton lock, fire due entries every [`BEAT_TICK`], and purge old
/// failures whenever the minute rolls over. Without the lock, wait for
/// it instead of firing (the singleton rule above). Database errors are
/// logged with a backoff tick, like the worker loop.
pub async fn run_scheduler(
    pool: PgPool,
    schedule: Vec<BeatEntry>,
    mut shutdown: watch::Receiver<bool>,
) {
    let mut last_purge_minute: String = String::new();
    // Dedicated session for the singleton lock: advisory locks are
    // per-session, so checking out a random pool connection per tick
    // would flap (a second session never sees our lock). Firing itself
    // still uses the pool; only the lock lives here.
    let mut lock_conn: Option<sqlx::pool::PoolConnection<sqlx::Postgres>> = None;
    loop {
        if *shutdown.borrow() {
            break;
        }
        if lock_conn.is_none() {
            match pool.acquire().await {
                Ok(conn) => lock_conn = Some(conn),
                Err(error) => {
                    tracing::warn!(%error, "scheduler lock connection failed; retrying");
                    if tick(&mut shutdown).await {
                        break;
                    }
                    continue;
                }
            }
        }
        let locked: bool = match sqlx::query_scalar(try_lock_sql())
            .bind(BEAT_LOCK_KEY)
            .fetch_one(
                &mut **lock_conn
                    .as_mut()
                    .expect("acquired above, dropped only below"),
            )
            .await
        {
            Ok(locked) => locked,
            Err(error) => {
                // Session is dead: drop it so the next tick acquires a
                // fresh one (and re-locks, waiting if a peer holds it).
                tracing::warn!(%error, "scheduler lock check failed; retrying");
                lock_conn = None;
                if tick(&mut shutdown).await {
                    break;
                }
                continue;
            }
        };
        if !locked {
            tracing::debug!("another scheduler holds the lock; waiting");
            if tick(&mut shutdown).await {
                break;
            }
            continue;
        }
        let now = Utc::now();
        match fire_due(&pool, &schedule, &now).await {
            Ok(fired) => {
                for name in &fired {
                    tracing::info!(entry = name, "scheduler fired entry");
                }
            }
            Err(error) => tracing::warn!(%error, "scheduler fire failed"),
        }
        // Purge old failures at most once per minute: the retention
        // cleanup must not run on every one-second tick.
        let minute = now.format("%Y-%m-%d %H:%M").to_string();
        if minute != last_purge_minute {
            match crate::queue::purge_failed(&pool, FAILED_RETENTION_SECS).await {
                Ok(removed) => {
                    if removed > 0 {
                        tracing::info!(removed, "scheduler purged old failures");
                    }
                    last_purge_minute = minute;
                }
                Err(error) => tracing::warn!(%error, "scheduler purge failed"),
            }
        }
        if tick(&mut shutdown).await {
            break;
        }
    }
    // Release the lock on the session that holds it. If the session is
    // already gone, its end released the lock for us.
    if let Some(mut conn) = lock_conn {
        match sqlx::query_scalar::<_, bool>(unlock_sql())
            .bind(BEAT_LOCK_KEY)
            .fetch_one(&mut *conn)
            .await
        {
            Ok(_) => {}
            Err(error) => tracing::warn!(%error, "scheduler unlock failed"),
        }
    }
    tracing::info!("scheduler loop stopped");
}

/// The default schedule: every entry from `celery.py`.
pub fn default_schedule() -> Vec<BeatEntry> {
    beat_schedule()
}

/// Sleep one tick, or stop early on shutdown. True when stopping.
async fn tick(shutdown: &mut watch::Receiver<bool>) -> bool {
    tokio::select! {
        _ = shutdown.wait_for(|stop| *stop) => true,
        _ = tokio::time::sleep(BEAT_TICK) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn singleton_lock_key_is_fixed() {
        // A second scheduler must ask for the same key, or the
        // singleton rule silently stops working.
        assert_eq!(BEAT_LOCK_KEY, 918_273_645_546_546);
        assert!(try_lock_sql().contains("pg_try_advisory_lock"));
        assert!(unlock_sql().contains("pg_advisory_unlock"));
    }

    #[test]
    fn default_schedule_is_the_full_beat_table() {
        assert_eq!(default_schedule().len(), 26);
    }
}
