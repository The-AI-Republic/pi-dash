#![forbid(unsafe_code)]
#![allow(clippy::explicit_auto_deref)]

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

use sea_orm::sea_query::{
    Alias, Expr, InsertStatement, OnConflict, Query as SeaQuery, SelectStatement,
};
use sea_orm::{ConnectionTrait, DbBackend, QueryResult};

use pidash_db::orm_tx;

use crate::queue::{enqueue_exec, NewJob};
use crate::schedule::{beat_schedule, due_entries, BeatEntry};

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

/// Last-run ledger state (`rust_job_schedule`, the schedule state table):
/// every entry's last fire time. SeaORM/`sea-query` expression of the same
/// `SELECT name, last_run_at FROM rust_job_schedule` the legacy SQL
/// strings spelled out.
fn last_runs_select() -> SelectStatement {
    SeaQuery::select()
        .column(Alias::new("name"))
        .column(Alias::new("last_run_at"))
        .from(Alias::new("rust_job_schedule"))
        .to_owned()
}

/// Last-run ledger write: upsert `(entry, now)` so the first fire
/// inserts and later fires overwrite. `ON CONFLICT (name) DO UPDATE SET
/// last_run_at = EXCLUDED.last_run_at` spelled through `OnConflict` —
/// `update_column` against the excluded row's inserting value.
fn mark_run_statement(name: &str, now: DateTime<Utc>) -> InsertStatement {
    SeaQuery::insert()
        .into_table(Alias::new("rust_job_schedule"))
        .columns([Alias::new("name"), Alias::new("last_run_at")])
        .values_panic([Expr::val(name).into(), Expr::val(now).into()])
        .on_conflict(
            OnConflict::column(Alias::new("name"))
                .update_column(Alias::new("last_run_at"))
                .to_owned(),
        )
        .to_owned()
}

/// Fire every due entry once: one transaction per entry holding the
/// enqueue plus the last-run bookkeeping. Returns the fired entry names.
pub async fn fire_due(
    pool: &PgPool,
    schedule: &[BeatEntry],
    now: &DateTime<Utc>,
) -> Result<Vec<String>, sqlx::Error> {
    let db = pidash_db::pool::sea_orm_connection(pool);
    let rows: Vec<QueryResult> = db
        .query_all(DbBackend::Postgres.build(&last_runs_select()))
        .await
        .map_err(orm_tx::into_sqlx)?;
    let mut last_runs: HashMap<String, DateTime<Utc>> = HashMap::with_capacity(rows.len());
    for row in &rows {
        // Named decode, never positional (orm-migration.md §3.2).
        let name: String = row.try_get("", "name").map_err(orm_tx::into_sqlx)?;
        let last_run: DateTime<Utc> = row.try_get("", "last_run_at").map_err(orm_tx::into_sqlx)?;
        last_runs.insert(name, last_run);
    }
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
        {
            // The mark runs on the same open transaction, so an enqueue
            // without its bookkeeping (or vice versa) still cannot
            // survive a crash between the two.
            let tx_conn = orm_tx::TxConn::new(&mut tx);
            tx_conn
                .execute(DbBackend::Postgres.build(&mark_run_statement(entry.name, *now)))
                .await
                .map_err(orm_tx::into_sqlx)?;
        }
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
        // PostgreSQL session advisory lock returns its row by
        // function, not by a table `SELECT`; spelling the function needs
        // raw-sql: session advisory lock by function, not a table SELECT; builder needs Expr::cust, itself a raw marker.
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
        // cleanup must not run on every one-second tick. Stale running
        // rows are reclaimed on the same cadence (PIDASHCONV-1327): a
        // worker that dies while already running leaves rows `running`
        // that `claim` never picks up again.
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
            match crate::queue::reclaim_stale_running(&pool, crate::queue::STALE_CLAIM_LEASE_SECS)
                .await
            {
                Ok(reclaimed) => {
                    if reclaimed > 0 {
                        tracing::info!(reclaimed, "scheduler reclaimed stale running jobs");
                    }
                }
                Err(error) => tracing::warn!(%error, "scheduler reclaim failed"),
            }
        }
        if tick(&mut shutdown).await {
            break;
        }
    }
    // Release the lock on the session that holds it. If the session is
    // already gone, its end released the lock for us.
    if let Some(mut conn) = lock_conn {
        // PostgreSQL session advisory unlock returns its row by
        // function, not by a table `SELECT`; spelling the function needs
        // raw-sql: session advisory unlock by function, not a table SELECT; builder needs Expr::cust, itself a raw marker.
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
        assert_eq!(default_schedule().len(), 31);
    }

    #[test]
    fn last_runs_statement_shape() {
        use sea_orm::DbBackend;
        let sql = DbBackend::Postgres.build(&last_runs_select()).to_string();
        assert!(sql.contains("FROM"), "select from: {sql}");
        assert!(
            sql.contains("\"rust_job_schedule\""),
            "schedule table: {sql}"
        );
        assert!(sql.contains("\"name\""), "name column: {sql}");
        assert!(sql.contains("\"last_run_at\""), "last_run_at column: {sql}");
    }

    #[test]
    fn mark_run_statement_shape() {
        use sea_orm::DbBackend;
        let now = Utc::now();
        let sql = DbBackend::Postgres
            .build(&mark_run_statement("prune-failed", now))
            .to_string();
        assert!(sql.contains("INSERT INTO"), "insert upsert into: {sql}");
        assert!(
            sql.contains("\"rust_job_schedule\""),
            "schedule table: {sql}"
        );
        assert!(
            sql.contains("ON CONFLICT (\"name\") DO UPDATE SET \"last_run_at\" = \"excluded\".\"last_run_at\""),
            "upsert overwrites last_run_at from excluded: {sql}"
        );
        assert!(sql.contains("prune-failed"), "entry name binds: {sql}");
    }
}
