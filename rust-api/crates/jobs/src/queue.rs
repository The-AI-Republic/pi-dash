#![forbid(unsafe_code)]

//! Postgres-backed job queue with transactional enqueue.
//!
//! This is the Rust counterpart of the Celery broker during coexistence:
//! request handlers enqueue rows inside their own database transaction
//! (Django's `transaction.on_commit(task.delay(...))` becomes
//! [`enqueue_in`] on the F-04 [`Transaction`][pidash_db::tx::Transaction],
//! so a rolled-back request never emits a phantom job), and the worker
//! loop in [`crate::worker`] claims rows with `SELECT ... FOR UPDATE
//! SKIP LOCKED` — the Postgres equivalent of a broker prefetch, race-safe
//! across any number of worker processes.
//!
//! Lifecycle of a row: `queued` → `running` → deleted (ack). A handler
//! that asks for a retry returns the row to `queued` with `attempts + 1`
//! and a future `visible_at`; a row that exhausts [`DEFAULT_MAX_RETRIES`]
//! stays in the table as `failed` with its `last_error` for diagnosis
//! (Celery without a result backend discards successes the same way).
//! [`purge_failed`] removes old failures; it runs from the scheduler loop.
//!
//! The tables are owned by the Rust backend and created at worker boot by
//! [`ensure_schema`]. Django stays schema owner for its own tables until
//! switchover — no Django migration is touched.
//!
//! The SQL here is static, so it goes through `sqlx::query` (runtime
//! checked) rather than the `query!` macros: those need a live database
//! at compile time, which CI does not have. The statement texts are unit
//! tested against a Postgres-dialect SQL parser instead.

use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde_json::Value;
use sqlx::{FromRow, PgPool};

use pidash_db::tx::Transaction;

/// Table holding queued, running and failed jobs.
pub const QUEUE_TABLE: &str = "rust_job_queue";

/// How many attempts a job gets before it is parked as failed.
/// Mirrors Celery's `Task.max_retries` default (and the explicit
/// `bind=True, max_retries=3` on `git_sync_task`).
pub const DEFAULT_MAX_RETRIES: i32 = 3;

/// Delay before a default retry becomes visible, in seconds.
/// Mirrors Celery's `Task.default_retry_delay` (3 minutes). Task-specific
/// backoffs (e.g. the `60 * 2 ** retries` in `git_sync_task`) are chosen
/// by the D-port handlers via [`Verdict::Retry`][crate::worker::Verdict].
pub const DEFAULT_RETRY_DELAY_SECS: u64 = 180;

/// Queue name every entry is published under while there is a single
/// default queue — the same `celery` routing key the contract harness
/// `broker.py` publishes to.
pub const DEFAULT_QUEUE: &str = "celery";

/// Row states. There is no `done`: acknowledged jobs are deleted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobStatus {
    Queued,
    Running,
    Failed,
}

impl JobStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            JobStatus::Queued => "queued",
            JobStatus::Running => "running",
            JobStatus::Failed => "failed",
        }
    }
}

/// A job to enqueue.
#[derive(Debug, Clone)]
pub struct NewJob {
    pub task: String,
    pub args: Value,
    pub kwargs: Value,
    pub queue: String,
    /// First time the job may run. `None` means immediately — the
    /// `countdown`/`eta` path of `apply_async` sets this explicitly.
    pub visible_at: Option<DateTime<Utc>>,
    pub max_retries: i32,
}

impl NewJob {
    /// An immediately-visible job on the default queue with default retries
    /// (the `.delay()` path).
    pub fn new(task: impl Into<String>, args: Value, kwargs: Value) -> Self {
        Self {
            task: task.into(),
            args,
            kwargs,
            queue: DEFAULT_QUEUE.to_owned(),
            visible_at: None,
            max_retries: DEFAULT_MAX_RETRIES,
        }
    }

    /// Delay visibility by `delay_secs` from now (the `countdown` path).
    pub fn delayed(mut self, delay_secs: u64, now: DateTime<Utc>) -> Self {
        self.visible_at = Some(now + ChronoDuration::seconds(delay_secs as i64));
        self
    }
}

/// A claimed queue row handed to a handler.
#[derive(Debug, Clone, FromRow)]
pub struct JobRow {
    pub id: i64,
    /// Stable public id, also used as the Celery message id when the job
    /// is forwarded to the Python workers during coexistence.
    /// Stored as text: the id is generated client-side (no database
    /// extension required) and never joined on.
    pub celery_id: String,
    pub task: String,
    pub args: Value,
    pub kwargs: Value,
    pub queue: String,
    pub status: String,
    pub attempts: i32,
    pub max_retries: i32,
    pub visible_at: DateTime<Utc>,
    pub claimed_at: Option<DateTime<Utc>>,
    pub claimed_by: Option<String>,
    pub created_at: DateTime<Utc>,
    pub last_error: Option<String>,
}

impl JobRow {
    /// True while another attempt is allowed after a failure.
    pub fn retries_left(&self) -> bool {
        should_retry(self.attempts, self.max_retries)
    }
}

/// Pure retry policy: another attempt is allowed while the number of
/// attempts used is below the budget.
pub fn should_retry(attempts: i32, max_retries: i32) -> bool {
    attempts < max_retries
}

/// DDL for the queue table and its claim index. Idempotent: safe to run
/// on every worker boot.
pub fn ensure_schema_sql() -> &'static str {
    "CREATE TABLE IF NOT EXISTS rust_job_queue (\
        id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,\
        celery_id TEXT NOT NULL UNIQUE,\
        task TEXT NOT NULL,\
        args JSONB NOT NULL DEFAULT '[]',\
        kwargs JSONB NOT NULL DEFAULT '{}',\
        queue TEXT NOT NULL DEFAULT 'celery',\
        status TEXT NOT NULL DEFAULT 'queued',\
        attempts INTEGER NOT NULL DEFAULT 0,\
        max_retries INTEGER NOT NULL DEFAULT 3,\
        visible_at TIMESTAMPTZ NOT NULL DEFAULT now(),\
        claimed_at TIMESTAMPTZ,\
        claimed_by TEXT,\
        created_at TIMESTAMPTZ NOT NULL DEFAULT now(),\
        last_error TEXT\
    );\
    CREATE INDEX IF NOT EXISTS rust_job_queue_claim_idx \
        ON rust_job_queue (status, visible_at, id)"
}

/// Create both queue tables. Runs at worker boot, never from requests.
pub async fn ensure_schema(pool: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query(ensure_schema_sql()).execute(pool).await?;
    sqlx::query(crate::schedule::ensure_schedule_schema_sql())
        .execute(pool)
        .await?;
    Ok(())
}

fn enqueue_sql() -> &'static str {
    "INSERT INTO rust_job_queue (celery_id, task, args, kwargs, queue, visible_at, max_retries) \
     VALUES ($1, $2, $3, $4, $5, COALESCE($6, now()), $7) \
     RETURNING id"
}

/// Enqueue a job on a pool handle (outside any request transaction).
pub async fn enqueue(pool: &PgPool, job: &NewJob) -> Result<i64, sqlx::Error> {
    enqueue_exec(pool, job).await
}

/// Enqueue a job inside the caller's transaction: the row commits or
/// rolls back with the surrounding unit of work, so a failed request
/// never emits a phantom job. This is the `transaction.on_commit(...)`
/// half of the F-04 tx wrapper applied to task fan-out.
pub async fn enqueue_in(tx: &mut Transaction<'_>, job: &NewJob) -> Result<i64, sqlx::Error> {
    enqueue_exec(&mut **tx.inner(), job).await
}

/// Enqueue a job on any Postgres executor (pool, connection or open
/// transaction). [`enqueue`] and [`enqueue_in`] both go through here.
pub async fn enqueue_exec<'e, E>(executor: E, job: &NewJob) -> Result<i64, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let id: i64 = sqlx::query_scalar(enqueue_sql())
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(&job.task)
        .bind(&job.args)
        .bind(&job.kwargs)
        .bind(&job.queue)
        .bind(job.visible_at)
        .bind(job.max_retries)
        .fetch_one(executor)
        .await?;
    Ok(id)
}

fn claim_select_sql() -> &'static str {
    "SELECT id, celery_id, task, args, kwargs, queue, status, attempts, \
            max_retries, visible_at, claimed_at, claimed_by, created_at, last_error \
     FROM rust_job_queue \
     WHERE queue = $1 AND status = 'queued' AND visible_at <= now() \
     ORDER BY id LIMIT 1 \
     FOR UPDATE SKIP LOCKED"
}

fn claim_update_sql() -> &'static str {
    "UPDATE rust_job_queue \
     SET status = 'running', claimed_at = now(), claimed_by = $2 \
     WHERE id = $1"
}

/// Claim the oldest due job for `owner`, holding its row lock only for
/// the claim transaction. Concurrent workers never see the same row:
/// `SKIP LOCKED` skips rows locked by another claim.
pub async fn claim(pool: &PgPool, queue: &str, owner: &str) -> Result<Option<JobRow>, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let job: Option<JobRow> = sqlx::query_as(claim_select_sql())
        .bind(queue)
        .fetch_optional(&mut *tx)
        .await?;
    if let Some(ref job) = job {
        sqlx::query(claim_update_sql())
            .bind(job.id)
            .bind(owner)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(job)
}

fn ack_sql() -> &'static str {
    "DELETE FROM rust_job_queue WHERE id = $1"
}

/// Acknowledge a finished job: the row is deleted (no result backend).
pub async fn ack(pool: &PgPool, id: i64) -> Result<(), sqlx::Error> {
    sqlx::query(ack_sql()).bind(id).execute(pool).await?;
    Ok(())
}

fn retry_sql() -> &'static str {
    "UPDATE rust_job_queue \
     SET status = 'queued', attempts = attempts + 1, visible_at = $2, \
         claimed_at = NULL, claimed_by = NULL, last_error = $3 \
     WHERE id = $1"
}

/// Return a job to the queue with a future `visible_at` and the failure
/// recorded. The caller computes the delay (default:
/// [`DEFAULT_RETRY_DELAY_SECS`]).
pub async fn retry(
    pool: &PgPool,
    id: i64,
    visible_at: DateTime<Utc>,
    last_error: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(retry_sql())
        .bind(id)
        .bind(visible_at)
        .bind(last_error)
        .execute(pool)
        .await?;
    Ok(())
}

fn fail_sql() -> &'static str {
    "UPDATE rust_job_queue \
     SET status = 'failed', claimed_at = NULL, claimed_by = NULL, last_error = $2 \
     WHERE id = $1"
}

/// Park a job as failed after its retries are exhausted. The row stays
/// for diagnosis until [`purge_failed`] removes it.
pub async fn fail(pool: &PgPool, id: i64, last_error: &str) -> Result<(), sqlx::Error> {
    sqlx::query(fail_sql())
        .bind(id)
        .bind(last_error)
        .execute(pool)
        .await?;
    Ok(())
}

fn purge_failed_sql() -> &'static str {
    "DELETE FROM rust_job_queue \
     WHERE status = 'failed' AND created_at < now() - make_interval(secs => $1)"
}

/// Delete failures older than `retention_secs`.
pub async fn purge_failed(pool: &PgPool, retention_secs: f64) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(purge_failed_sql())
        .bind(retention_secs)
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn parse_postgres(sql: &str) -> Vec<sqlparser::ast::Statement> {
        sqlparser::parser::Parser::parse_sql(&sqlparser::dialect::PostgreSqlDialect {}, sql)
            .expect("queue SQL must parse as Postgres")
    }

    #[test]
    fn new_job_defaults_match_celery_delay() {
        let job = NewJob::new("t", json!([1]), json!({"a": 2}));
        assert_eq!(job.queue, "celery");
        assert_eq!(job.max_retries, 3);
        assert!(job.visible_at.is_none());
    }

    #[test]
    fn delayed_sets_absolute_visibility() {
        let now = Utc::now();
        let job = NewJob::new("t", json!([]), json!({})).delayed(300, now);
        assert_eq!(
            job.visible_at.expect("set"),
            now + ChronoDuration::seconds(300)
        );
    }

    #[test]
    fn retry_budget_is_attempts_below_max() {
        assert!(should_retry(0, 3));
        assert!(should_retry(2, 3));
        assert!(!should_retry(3, 3));
        assert!(!should_retry(4, 3));
    }

    #[test]
    fn status_strings_are_lowercase() {
        assert_eq!(JobStatus::Queued.as_str(), "queued");
        assert_eq!(JobStatus::Running.as_str(), "running");
        assert_eq!(JobStatus::Failed.as_str(), "failed");
    }

    #[test]
    fn all_statements_parse_as_postgres() {
        for sql in [
            ensure_schema_sql(),
            enqueue_sql(),
            claim_select_sql(),
            claim_update_sql(),
            ack_sql(),
            retry_sql(),
            fail_sql(),
            purge_failed_sql(),
            crate::schedule::ensure_schedule_schema_sql(),
            crate::schedule::fetch_last_runs_sql(),
            crate::schedule::mark_run_sql(),
            crate::scheduler::try_lock_sql(),
        ] {
            assert!(!parse_postgres(sql).is_empty(), "unparsed: {sql}");
        }
    }

    #[test]
    fn claim_skips_locked_rows() {
        assert!(claim_select_sql().contains("FOR UPDATE SKIP LOCKED"));
        assert!(claim_select_sql().contains("visible_at <= now()"));
        assert!(claim_select_sql().contains("ORDER BY id LIMIT 1"));
    }

    #[test]
    fn enqueue_is_visible_immediately_by_default() {
        assert!(enqueue_sql().contains("COALESCE($6, now())"));
    }
}
